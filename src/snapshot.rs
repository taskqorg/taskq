//! Canonical read model (R6, § 4, § 5): one snapshot of the open tasks renders `status`, `status --json`, the
//! `status.json` tasks and totals, `service status` and the tray counts.

use crate::board::{self, Board};
use crate::core::{State, Task, options};
use crate::service::{self, Config, View};
use crate::{Res, age, cell, compact, runtime};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub const SCHEMA: u64 = 1;
/// Every open state; `totals` names each one, zero included.
pub const STATES: [&str; 8] = ["held", "ready", "waiting", "doing", "review", "ask", "later", "failed"];
/// What a reader finds in a snapshot of each source.
const BOARD_CAPS: [&str; 4] = ["tasks", "totals", "questions", "hosts"];
const HOST_CAPS: [&str; 6] = ["tasks", "totals", "focus", "capacity", "agents", "events"];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecutionStatus {
    pub task: u64,
    pub attempt: u64,
    pub id: u64,
    pub through: u64,
    pub state: String,
    pub reason: String,
}

/// Board identity and terminal evidence fence every advisory heartbeat observation.
/// A host row counts within the project ttl (§ 3 lease); older reads unknown.
pub(crate) fn execution_status(task:u64, x:&crate::core::Execution, hosts:&[board::Issue], now:i64, ttl:i64)->Value {
    let mut status=ExecutionStatus {task,attempt:x.attempt,id:x.id,through:x.through,state:"unknown".into(),reason:"termination unconfirmed; status missing or stale; restore owning service".into()};
    let owners:Vec<_>=hosts.iter().filter(|h|h.trusted && h.title==format!("taskq host {}",x.host)).collect();
    let mut observed=None;
    if let [host]=owners[..] {
        if (0..=ttl).contains(&(now-host.updated)) {
            let rows=host.body.lines().filter_map(|l|l.strip_prefix("execution ")).map(|text| {
                if text.len()>4096 {return Err("oversized execution status".to_string());}
                serde_json::from_str::<ExecutionStatus>(text).map_err(|e|e.to_string())
            }).collect::<Result<Vec<_>,_>>();
            if let Ok(rows)=rows {
                let matching:Vec<_>=rows.iter().filter(|r|r.task==task && r.attempt==x.attempt && r.id==x.id && r.through==x.through).collect();
                if let [row]=matching[..] {
                    if matches!(row.state.as_str(),"running"|"waiting"|"unknown") && !row.reason.is_empty()
                        && row.reason.chars().count()<=240 && !row.reason.chars().any(char::is_control) {
                        status=row.clone();observed=Some(host.updated);
                    }
                }
            }
        }
    }
    let mut value=serde_json::to_value(status).unwrap();
    value["host"]=json!(x.host);value["runtime"]=json!(x.runtime);value["observed"]=json!(observed);value
}

#[derive(Default)]
pub struct Row {
    pub n: u64,
    pub title: String,
    pub labels: Vec<String>,
    pub url: String,
    pub task: Task,
    /// § 2 revision: the expected `--revision` of the next command.
    pub revision: u64,
    /// An unresolved external execution blocks the next submission even after a task state change.
    pub execution_wait: Option<String>,
    pub executions: Vec<Value>,
    /// The runtime accepted input of the current attempt (`accepted` entry): a codex id `-` then reads running, not starting.
    pub accepted: bool,
    /// `Depends on` issues that keep it waiting (§ 1).
    pub deps: Vec<service::Blocker>,
    /// Text of the last trusted `failed` entry.
    pub failed: Option<String>,
    /// Claude Remote Control URL of the current attempt from its host's heartbeat `session` line (§ 4).
    pub session_url: Option<String>,
    /// Board time of the entry behind the last logged state change (§ 4 Tray age); `None`: no logged change.
    pub since: Option<i64>,
    /// R6 stuck: seconds since the newest log entry; `None`: not stuck.
    pub stuck: Option<i64>,
}

/// Owner texts of a row's wait (docs/design/menu.md M3 Task row text, #1524); the raw reason stays in `execution_wait`.
pub const WAIT_TEXTS: [&str; 6] = ["Waiting for a free slot", "Waiting for GUI access", "Waiting for the agent to accept input",
    "Waiting for your answer", "Waiting for the previous run to end", "Run state unknown; see service log"];

/// The owner text of a raw wait diagnostic; text no class names reads as the previous run's end.
pub fn wait_text(raw: &str) -> &'static str {
    let has = |w: &[&str]| w.iter().any(|w| raw.contains(w));
    WAIT_TEXTS[if has(&["GUI"]) { 1 } else if has(&["admission"]) { 0 } else if has(&["owner answer"]) { 3 } else if has(&["input"]) { 2 }
        else if has(&["unknown", "stale", "missing", "unavailable", "deferred"]) { 5 } else { 4 }]
}

/// R6: no movement for this long marks a task stuck.
pub const STUCK_AFTER: i64 = 2 * 3600;

/// R6 stuck age of task `n` whose newest log entry is at `last`: observe only, never a gate.
pub(crate) fn stuck(n: u64, t: &Task, deps: &[service::Blocker], last: i64, hosts: &[board::Issue], now: i64, ttl: i64) -> Option<i64> {
    let age = now - last;
    // § 3 lease: a fresh heartbeat lists `#<N> <attempt>`
    let leased = || t.attempt.is_some_and(|a| hosts.iter().any(|h| h.trusted && now - h.updated < ttl && h.body.lines().any(|l| l == format!("#{n} {a}"))));
    match t.state {
        State::Waiting => deps.iter().all(|b| b.reason.starts_with("unknown: closed")),
        State::Held | State::Ask | State::Later => age >= STUCK_AFTER,
        State::Doing => age >= STUCK_AFTER && !leased(),
        _ => false,
    }.then_some(age)
}

#[derive(Default)]
pub struct Host {
    pub diagnostics: Value,
    pub name: String,
    /// Heartbeat `service <id>` line (§ 4 step 3): the service that claims this host name.
    pub service: Option<String>,
    pub version: String,
    pub updated: i64,
    /// `off` or `unknown` when the host admits nothing by focus (§ 4).
    pub focus: Option<String>,
    /// `<rt> until|probe <time>` per runtime held after a usage limit (§ 4 step 1).
    pub limits: Vec<String>,
}

pub struct Snapshot {
    /// `board`: this command read the board; `host`: the service's `status.json`.
    pub source: &'static str,
    /// Board time of the read.
    pub observed: i64,
    /// Why the snapshot is incomplete; `None`: complete.
    pub reason: Option<String>,
    pub board: Value,
    pub tasks: Vec<Row>,
    pub hosts: Vec<Host>,
}

/// The one snapshot function: open tasks by priority then number, plus host heartbeats.
pub fn from_views(source: &'static str, b: &dyn Board, cfg: &Config, observed: i64, views: &[View], hosts: &[board::Issue]) -> Snapshot {
    let mut open: Vec<&View> = views.iter().filter(|v| v.projection.task.state != State::Closed).collect();
    open.sort_by_key(|v| (service::priority(&v.issue), v.issue.number));
    let urls = session_urls(hosts);
    let tasks = open.into_iter().map(|v| {
        let executions:Vec<_>=v.projection.executions.iter().filter(|x|x.release.is_none())
            .map(|x| {
                let mut value=execution_status(v.issue.number,x,hosts,observed,cfg.ttl);
                value["role"]=json!(v.projection.changes.iter().find(|(id,_,_)| *id==x.attempt).and_then(|(_,_,t)|t.role.as_deref()));
                value
            }).collect();
        Row {
        n: v.issue.number,
        title: v.issue.title.clone(),
        labels: v.issue.labels.clone(),
        url: b.url(v.issue.number),
        task: v.projection.task.clone(),
        revision: v.projection.revision,
        execution_wait: executions.iter().find(|x|x["state"]!="running" || x["attempt"].as_u64()!=v.projection.task.attempt)
            .map(|x|format!("execution {} on {}: {}: {}",x["id"],x["host"].as_str().unwrap_or("?"),
                x["state"].as_str().unwrap_or("unknown"),x["reason"].as_str().unwrap_or("observation unavailable")))
            .or_else(|| v.projection.executions.iter().find(|x| Some(x.attempt) == v.projection.task.attempt && !v.projection.task.inbox.is_empty()
                && x.release == Some(crate::core::Release::Exit) && !x.accepted)
                .map(|x| format!("execution {} on {}: input acceptance unknown; reconcile native input", x.id, x.host))),
        executions,
        accepted: v.projection.executions.iter().any(|x| Some(x.attempt) == v.projection.task.attempt && x.accepted),
        deps: v.blockers.clone(),
        session_url: v.projection.task.attempt.zip(v.projection.task.session.as_deref().and_then(|s| s.rsplit(' ').next()))
            .filter(|_| v.projection.task.session.as_deref().is_some_and(|s| crate::runtime::remote_session(s).is_some()))
            .and_then(|(a, h)| urls.get(&(h.to_string(), a)).cloned()),
        stuck: stuck(v.issue.number, &v.projection.task, &v.blockers,
            v.comments.iter().filter(|c| c.trusted && c.body.starts_with("taskq: ")).map(|c| c.created).max().unwrap_or(v.issue.created),
            hosts, observed, cfg.ttl),
        since: v.projection.changes.last().and_then(|(id, ..)| v.comments.iter().find(|c| c.id == *id)).map(|c| c.created),
        failed: (v.projection.task.state == State::Failed).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: failed")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
    }}).collect();
    let hosts = hosts.iter().map(|h| Host {
        diagnostics: crate::service_versions::read(&h.body,h.updated,observed,cfg.ttl as i64,false),
        name: h.title.trim_start_matches("taskq host ").to_string(),
        service: h.body.lines().find_map(|l| l.strip_prefix("service ")).map(str::to_string),
        version: h.body.lines().find_map(|l| l.strip_prefix("version ")).unwrap_or("?").to_string(),
        updated: h.updated,
        focus: h.body.lines().find_map(|l| l.strip_prefix("focus ")).map(str::to_string),
        limits: h.body.lines().filter_map(|l| l.strip_prefix("limit ")).map(str::to_string).collect(),
    }).collect();
    Snapshot { source, observed, reason: None, board: identity(b, cfg), tasks, hosts }
}

/// `session #<N> <attempt> <url>` heartbeat lines (§ 4) by host name and attempt; only a canonical claude.ai URL.
pub fn session_urls(hosts: &[board::Issue]) -> HashMap<(String, u64), String> {
    hosts.iter().flat_map(|h| {
        let name = h.title.trim_start_matches("taskq host ").to_string();
        h.body.lines().filter_map(move |l| {
            let w: Vec<&str> = l.strip_prefix("session #")?.split(' ').collect();
            let [_, a, url] = w[..] else { return None };
            runtime::remote_url(url).then_some(())?;
            Some(((name.clone(), a.parse().ok()?), url.to_string()))
        })
    }).collect()
}

fn identity(b: &dyn Board, cfg: &Config) -> Value {
    json!({"kind": cfg.board.name, "host": cfg.web_host, "repo": cfg.repo, "url": b.url(0).trim_end_matches("/0")})
}

/// `taskq status`: one board read; a failed read is an incomplete snapshot with its reason, never zero tasks.
pub fn read(b: &dyn Board, cfg: &Config, raw: &Value, only: &[String]) -> Snapshot {
    let attempt = || -> Res<Snapshot> {
        let (mut views, hosts) = service::views(b, &mut HashMap::new(), &mut HashMap::new())?;
        views.retain(|v| in_scope(&v.issue, only));
        if let Some(who) = raw["assignee"].as_str() {
            let who = if who == "me" { b.me()? } else { who.to_string() };
            let mut keep = vec![];
            for v in views { if service::assigned(b, &v.issue, &who)? { keep.push(v); } }
            views = keep;
        }
        Ok(from_views("board", b, cfg, b.now()?, &views, &hosts))
    };
    attempt().unwrap_or_else(|e| Snapshot { source: "board", observed: b.now().unwrap_or(0), reason: Some(e), board: identity(b, cfg), tasks: vec![], hosts: vec![] })
}

/// A PM's scope (§ 5 `--only`): no labels means every task.
pub fn in_scope(i: &board::Issue, only: &[String]) -> bool {
    only.is_empty() || only.iter().any(|f| crate::labels::matches(&i.labels, f))
}

/// Per-state totals of open task states; the one count used by every reader.
pub fn totals<'a>(states: impl IntoIterator<Item = &'a str>) -> BTreeMap<&'static str, usize> {
    let mut t: BTreeMap<&'static str, usize> = STATES.iter().map(|s| (*s, 0)).collect();
    for s in states {
        if let Some(k) = STATES.iter().find(|x| **x == s) {
            *t.get_mut(k).unwrap() += 1;
        }
    }
    t
}

/// R6 line 2 from totals.
pub fn counts_line(t: &BTreeMap<&'static str, usize>) -> String {
    format!("In work {} · Waiting for answer {} · Ready {}", t["doing"] + t["review"], t["ask"], t["ready"])
}

/// A `status.json` is complete when its service's last pass read the board; N-1 files have no `complete` key.
pub fn complete(status: &Value) -> bool {
    status["complete"].as_bool().unwrap_or(status["error"].is_null()) && status["tasks"].is_array()
}

/// Totals of a `status.json`; `None` when it is incomplete (unknown is never zero).
pub fn status_totals(status: &Value) -> Option<BTreeMap<&'static str, usize>> {
    complete(status).then(|| totals(status["tasks"].as_array().into_iter().flatten().filter_map(|t| t["state"].as_str())))
}

impl Row {
    pub fn json(&self) -> Value {
        let mut row = json!({"n": self.n, "title": self.title, "state": self.task.state.name(), "labels": self.labels, "url": self.url, "revision": self.revision,
            "session": self.task.session, "attempt": self.task.attempt, "role": self.task.role,
            "deps": self.deps.iter().filter(|b| b.dep.repo.is_none() && b.reason == "open").map(|b| b.dep.n).collect::<Vec<_>>(),
            "blocked": self.deps.iter().map(|b| json!({"ref": b.dep.name(), "url": b.url, "reason": b.reason})).collect::<Vec<_>>(),
            "card": self.task.card, "question": self.task.card.as_deref().map(question), "failed": self.failed,
            "controls": controls(self.task.state.name())});
        // § 2: the PM's pick sits beside the worker's star; it is never an answer
        if let Some((k, reason)) = &self.task.pick { row["pick"] = json!({"id": k, "reason": reason}); }
        if !self.executions.is_empty() { row["executions"]=json!(self.executions); }
        if let Some(reason) = &self.execution_wait { row["execution_wait"] = json!(reason); row["wait"] = json!(wait_text(reason)); }
        if let Some(u) = &self.session_url { row["session_url"] = json!(u); }
        row["since"] = json!(self.since);
        row["stuck"] = json!(self.stuck.is_some());
        if let Some(a) = self.stuck { row["age_s"] = json!(a); }
        row
    }
}

/// Published task controls `taskq.task-controls/1` (docs/design/task-controls.md): action, CLI command, fixed reason,
/// states that offer it. Close is never a control: it stays a board operation.
pub const CONTROLS_VERSION: &str = "taskq.task-controls/1";
const LIVE: &[&str] = &["ready", "waiting", "doing", "review", "ask"];
pub const CONTROLS: [(&str, &str, &str, &[&str]); 4] = [
    ("start", "start", "", &["held"]),
    ("stop", "failed", "Reason: Stopped by the owner.", LIVE),
    ("later", "later", "Reason: Parked by the owner.", LIVE),
    ("retry", "requeue", "Reason: Retried by the owner.", &["later", "failed"]),
];

/// Control actions a row in `state` offers, in [`CONTROLS`] order.
pub fn controls(state: &str) -> Vec<&'static str> {
    CONTROLS.iter().filter(|c| c.3.contains(&state)).map(|c| c.0).collect()
}

/// Free-text answer limits (`answer N --text`, § 2): nonblank lines, characters per line.
pub const ANSWER_LINES: usize = 4;
pub const ANSWER_CHARS: usize = 240;

/// Canonical parsed fields of an open card (§ 2, docs/design/answer-card.md), the one parse every card UI reads.
/// `parsed` false: a reader shows `text` with the board link and no write controls.
pub fn question(card: &str) -> Value {
    let field = |p: &str| card.lines().find_map(|l| l.strip_prefix(p)).map(str::trim);
    let evidence: Vec<Value> = field("Evidence: ").unwrap_or("").split("), ").filter_map(|l| {
        let (label, url) = l.strip_prefix('[')?.split_once("](")?;
        Some(json!({"label": label, "url": url.trim_end_matches(')')}))
    }).collect();
    let o = options(card);
    // an option that starts with `close` closes the task after the answer (dispatch::answer_picks): consequential
    let choices: Vec<Value> = o.iter().map(|(k, t, star)| json!({"id": k, "text": t, "recommended": star, "consequential": t.to_lowercase().starts_with("close")})).collect();
    let parsed = field("Problem: ").is_some() && (2..=4).contains(&o.len()) && o.iter().filter(|x| x.2).count() == 1;
    json!({"parsed": parsed, "text": card, "done": field("Done: "), "problem": field("Problem: ").map(|_| brief(card)), "evidence": evidence,
        "choices": choices, "limits": {"lines": ANSWER_LINES, "chars": ANSWER_CHARS}})
}

impl Snapshot {
    pub fn totals(&self) -> BTreeMap<&'static str, usize> {
        totals(self.tasks.iter().map(|r| r.task.state.name()))
    }

    /// `status --json` (§ 5).
    pub fn json(&self) -> Value {
        let caps: &[&str] = if self.source == "host" { &HOST_CAPS } else { &BOARD_CAPS };
        json!({"schema": SCHEMA, "source": self.source, "observed": self.observed, "complete": self.reason.is_none(), "reason": self.reason,
            "board": self.board, "totals": self.reason.is_none().then(|| self.totals()), "tasks": self.tasks.iter().map(Row::json).collect::<Vec<_>>(),
            "hosts": self.hosts.iter().map(|h| json!({"name": h.name, "version": h.version, "updated": h.updated, "diagnostics": h.diagnostics})).collect::<Vec<_>>(),
            "capabilities": caps})
    }

    /// The snapshot keys of `status.json` (§ 4) after a pass; `error`: the pass failed, so tasks are the last read's.
    pub fn status_keys(&self, error: Option<&str>) -> [(&'static str, Value); 8] {
        let reason = error.map(String::from).or(self.reason.clone());
        [("schema", json!(SCHEMA)), ("source", json!("host")), ("observed", json!(self.observed)), ("complete", json!(reason.is_none())),
            ("reason", json!(reason)), ("totals", json!(reason.is_none().then(|| self.totals()))),
            ("tasks", json!(self.tasks.iter().map(Row::json).collect::<Vec<_>>())), ("hosts", self.json()["hosts"].clone())]
    }

    /// R6 text report.
    pub fn text(&self, cfg: &Config, raw: &Value) -> Res<String> {
        if let Some(r) = &self.reason {
            return Err(r.clone());
        }
        let base = self.board["url"].as_str().unwrap_or_default();
        let project = cfg.root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut out = vec![format!("{project} · [board]({base})"), counts_line(&self.totals())];
        let link = |r: &Row| format!("[#{} {}]({})", r.n, cell(&compact(&r.title, 60)), r.url);
        let rows: Vec<String> = self.tasks.iter()
            .filter(|r| !matches!(r.task.state, State::Held | State::Ask | State::Later | State::Failed))
            .map(|r| {
                let state = if let Some(reason) = &r.execution_wait {
                    format!("{} · {}", r.task.state.name(), wait_text(reason))
                } else if r.task.state == State::Waiting {
                    let deps: Vec<String> = r.deps.iter().map(|b| format!("[{}]({}) {}", b.dep.name(), b.url, cell(&compact(&b.reason, 80)))).collect();
                    format!("blocked ({})", deps.join(", "))
                } else {
                    r.task.state.name().to_string()
                };
                let (rt, s) = session_link(cfg, raw, r);
                format!("| {} | {state} | {} | {s} |", link(r), cell(&rt))
            })
            .collect();
        if !rows.is_empty() {
            out.push(String::new());
            out.push("| Task | State | Runtime | Session |\n|---|---|---|---|".into());
            out.extend(rows);
        }
        let qs: Vec<String> = self.tasks.iter()
            .filter(|r| r.task.state == State::Ask || (r.task.state == State::Review && r.task.card.is_some()))
            .map(|r| {
                let card = r.task.card.clone().unwrap_or_default();
                let (n, url) = (r.n, &r.url);
                let o: Vec<String> = options(&card).iter().map(|(k, t, star)| {
                    let label = cell(&compact(t, 120));
                    let label = if t.chars().count() > 120 { format!("[{label}]({url})") } else { label };
                    format!("{n}.{k} {label}{}", if *star { " ★" } else { "" })
                }).collect();
                // § 2: an `ask` card without options is an owner action
                let o = if o.is_empty() && r.task.state == State::Ask { cell(&format!("waiting for owner: {}", compact(&brief(&card), 120))) } else { o.join(" · ") };
                let o = match &r.task.pick { Some((k, why)) => format!("{o} · PM {n}.{k}: {}", cell(&compact(why, 160))), None => o };
                format!("| {} | [{}]({}) | {o} |", link(r), cell(&compact(&brief(&card), 160)), url)
            })
            .collect();
        if !qs.is_empty() {
            out.push(String::new());
            out.push("Questions (answer N.M):\n| Question | Brief reason | Options |\n|---|---|---|".into());
            out.extend(qs);
        }
        let mut parked = vec![];
        for (name, state) in [("Held", State::Held), ("Later", State::Later)] {
            let rows: Vec<String> = self.tasks.iter().filter(|r| r.task.state == state).map(link).collect();
            if !rows.is_empty() { parked.push(format!("{name}: {}", rows.join(", "))); }
        }
        if !parked.is_empty() {
            out.push(String::new());
            out.extend(parked);
        }
        let failed: Vec<String> = self.tasks.iter().filter(|r| r.task.state == State::Failed)
            .map(|r| format!("| {} | [{}]({}) |", link(r), cell(&compact(r.failed.as_deref().unwrap_or(""), 160)), r.url))
            .collect();
        if !failed.is_empty() {
            out.push(String::new());
            out.push("Failed:\n| Task | Reason |\n|---|---|".into());
            out.extend(failed);
        }
        let stuck: Vec<String> = self.tasks.iter().filter_map(|r| r.stuck.map(|a| format!("{} ({}, {})", link(r), r.task.state.name(), age(a)))).collect();
        if !stuck.is_empty() {
            out.push(String::new());
            out.push(format!("Stuck: {}", stuck.join(", ")));
        }
        let hs: Vec<String> = self.hosts.iter().map(|h| format!("{} v{} {}{}", h.name, h.version, age(self.observed - h.updated), h.focus.as_ref().map(|f| format!(" focus {f}")).unwrap_or_default()
            + &h.limits.iter().map(|l| l.split_once(' ').map_or_else(|| format!(" {l}"), |(rt, at)| format!(" {rt} limit {at}"))).collect::<String>())).collect();
        if !hs.is_empty() {
            out.push(String::new());
            out.push(format!("Hosts: {}", hs.join(", ")));
        }
        Ok(out.join("\n"))
    }
}

/// The `Problem:` text of a card, else its first line.
pub fn brief(card: &str) -> String {
    let mut lines = card.lines();
    while let Some(l) = lines.next() {
        if let Some(p) = l.strip_prefix("Problem: ") {
            return std::iter::once(p).chain(lines.take_while(|l| !l.starts_with("Evidence:") && !l.starts_with("To answer:") && options(l).is_empty())).collect::<Vec<_>>().join(" ");
        }
    }
    card.lines().next().unwrap_or_default().to_string()
}

fn session_link(cfg: &Config, raw: &Value, r: &Row) -> (String, String) {
    let (t, n) = (&r.task, r.n);
    let Some((rt, id)) = t.session.as_deref().and_then(|s| s.split(' ').next()).and_then(|s| s.split_once(':')) else { return (String::new(), String::new()) };
    let host = t.session.as_deref().and_then(|s| s.rsplit(' ').next()).unwrap_or_default();
    let mut id = id.to_string();
    if (id == "-" || runtime::of(rt).remote) && host == cfg.me
        && let Some(a) = t.attempt
    {
        let l = runtime::Launch { execution: None, task: n, title: String::new(), attempt: a, role: t.role.clone().unwrap_or_default(), rt: rt.into(), session: id.clone(), url: String::new(), resume: None };
        id = runtime::codex_thread(&runtime::log_path(&cfg.root, &l)).unwrap_or(id);
    }
    let host = cell(host);
    let link = match rt {
        _ if id == "-" && r.accepted => format!("{host}: running"),
        _ if id == "-" => format!("{host}: starting"),
        _ if runtime::of(rt).remote => runtime::claude_remote(&id).filter(|_| host == cell(&cfg.me)).or_else(|| r.session_url.clone()).map_or_else(|| format!("{host}: Remote Control unknown"), |url| format!("[{host}]({url})")),
        _ if std::env::var("TASKQ_CLIENT").is_ok_and(|c| runtime::of(&c).codex && runtime::of(&c).child) => format!("[{host}]({})", runtime::thread_url(&id)),
        _ => match raw["pages"].as_str() {
            Some(p) => format!("[{host}]({}/open.html#{})", p.trim_end_matches('/'), runtime::thread_url(&id)),
            None => format!("{host}: `{}`", runtime::thread_url(&id)),
        },
    };
    (rt.to_string(), link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec() -> crate::core::Execution {
        crate::core::Execution { id: 7, attempt: 5, through: 7, host: "win".into(), runtime: "codex".into(), accepted: true, release: None, usage_hold: None }
    }

    fn host(age: i64) -> board::Issue {
        let row = r#"execution {"task":1,"attempt":5,"id":7,"through":7,"state":"running","reason":"turn running"}"#;
        board::Issue { number: 9, title: "taskq host win".into(), body: format!("version 4\n{row}"), updated: 1000 - age, trusted: true, ..Default::default() }
    }

    #[test]
    fn remote_codex_reads_starting_until_accepted() {
        let task = Task { attempt: Some(5), session: Some("codex:- win".into()), ..Default::default() };
        let mut r = Row { n: 1, title: String::new(), labels: vec![], url: String::new(), task, revision: 5, execution_wait: None,
            executions: vec![], accepted: false, deps: vec![], failed: None, session_url: None, since: None, stuck: None };
        let cfg = crate::tests::cfg();
        assert_eq!(session_link(&cfg, &json!({}), &r).1, "win: starting");
        r.accepted = true;
        assert_eq!(session_link(&cfg, &json!({}), &r).1, "win: running");
    }

    // Chain: R6 stuck. Risk: a silent task never shows, or a moving or leased one is flagged.
    #[test]
    fn stuck_flags_each_state_from_two_hours_and_never_a_moving_task() {
        let t = |state, attempt| Task { state, attempt, ..Default::default() };
        let dep = |reason: &str| service::Blocker { dep: crate::core::depends("Depends on #2").remove(0), url: String::new(), reason: reason.into() };
        let beat = |age: i64| board::Issue { title: "taskq host mac".into(), body: "version 4\n#1 5".into(), updated: 10_000 - age, trusted: true, ..Default::default() };
        let at = |task: &Task, deps: &[service::Blocker], age: i64, hosts: &[board::Issue]| stuck(1, task, deps, 10_000 - age, hosts, 10_000, 300);
        for s in [State::Held, State::Ask, State::Later, State::Doing] {
            assert_eq!(at(&t(s, Some(5)), &[], STUCK_AFTER - 1, &[]), None, "{s:?} under 2 h");
            assert_eq!(at(&t(s, Some(5)), &[], STUCK_AFTER, &[]), Some(STUCK_AFTER), "{s:?} at 2 h");
            assert_eq!(at(&t(s, Some(5)), &[], 60, &[]), None, "{s:?} moving");
        }
        // a doing attempt its fresh heartbeat lists is leased; a stale heartbeat or another attempt is not
        assert_eq!(at(&t(State::Doing, Some(5)), &[], 3 * STUCK_AFTER, &[beat(10)]), None);
        assert_eq!(at(&t(State::Doing, Some(5)), &[], 3 * STUCK_AFTER, &[beat(300)]), Some(3 * STUCK_AFTER));
        assert_eq!(at(&t(State::Doing, Some(6)), &[], 3 * STUCK_AFTER, &[beat(10)]), Some(3 * STUCK_AFTER));
        for s in [State::Ready, State::Review, State::Failed] {
            assert_eq!(at(&t(s, None), &[], 3 * STUCK_AFTER, &[]), None, "{s:?} never");
        }
        // waiting: at once when every blocker is closed, never while one is open or unreadable
        let w = t(State::Waiting, None);
        assert_eq!(at(&w, &[dep("unknown: closed not planned")], 0, &[]), Some(0));
        assert_eq!(at(&w, &[dep("unknown: closed not planned"), dep("open")], 3 * STUCK_AFTER, &[]), None);
        assert_eq!(at(&w, &[dep("unknown: 502")], 3 * STUCK_AFTER, &[]), None);
    }

    #[test]
    fn stuck_row_prints_one_line_and_json_age() {
        let row = |n, stuck| Row { n, title: "T".into(), labels: vec![], url: format!("u{n}"), task: Task { state: State::Ask, ..Default::default() }, revision: 0,
            execution_wait: None, executions: vec![], accepted: false, deps: vec![], failed: None, session_url: None, since: None, stuck };
        let snap = Snapshot { source: "board", observed: 0, reason: None, board: json!({"url": "b"}), tasks: vec![row(1, Some(9000)), row(2, None)], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("Stuck: ")).collect::<Vec<_>>(), ["Stuck: [#1 T](u1) (ask, 2h)"], "{text}");
        let rows = snap.json()["tasks"].clone();
        assert_eq!((rows[0]["stuck"].clone(), rows[0]["age_s"].clone()), (json!(true), json!(9000)));
        assert_eq!((rows[1]["stuck"].clone(), rows[1].get("age_s")), (json!(false), None));
    }

    // Chain: #1524 menu.md M3 Task row text. Risk: a raw execution, fence or wait diagnostic reaches a tray or status row.
    #[test]
    fn every_wait_producer_maps_to_a_spec_named_owner_text() {
        let spec = include_str!("../docs/design/menu.md");
        for t in WAIT_TEXTS { assert!(spec.contains(&format!("`{t}`")), "menu.md names {t}"); }
        // every raw producer of `execution_wait`: service waits, execution statuses, gui eligibility, snapshot fallbacks
        let cases = [
            ("launch held by admission gate", 0),
            ("GUI unavailable on mac: no session", 1), ("GUI runtime codex refused: sandbox", 1), ("GUI runtime claude has no effective Full grant", 1),
            ("native input acceptance pending", 2), ("execution 3 on mac: input acceptance unknown; reconcile native input", 2),
            ("input pending on win; await or restore the owning service", 2),
            ("execution 3 on mac: waiting: owner answer required", 3),
            ("execution 3 on mac: termination unconfirmed", 4), ("cancellation pending; awaiting native termination", 4),
            ("execution 3 on mac: running: native execution observed; termination pending", 4),
            ("execution 3 on mac: unknown: termination unconfirmed; status missing or stale; restore owning service", 5),
            ("native observation unavailable; restore owning runtime", 5), ("execution 3: native observation deferred; recovery budget exhausted", 5),
            ("execution 3 on mac: recovery unknown: io", 5), ("multiple unresolved executions; verify native ownership before recovery", 4),
            ("native execution evidence missing; reconcile the owning runtime", 5), ("session identity unknown; recover the original native session", 5),
            ("Claude returned no valid background ID; launch outcome unknown; reconcile the original input", 2),
        ];
        for (raw, k) in cases { assert_eq!(wait_text(raw), WAIT_TEXTS[k], "{raw}"); }
        // status text and row JSON: owner text only in the row, raw reason kept in --json
        let raw = cases[11].0;
        let row = Row { n: 1, title: "T".into(), url: "u1".into(), task: Task { state: State::Doing, ..Default::default() }, execution_wait: Some(raw.into()), ..Default::default() };
        let snap = Snapshot { source: "board", observed: 0, reason: None, board: json!({"url": "b"}), tasks: vec![row], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("| doing · Run state unknown; see service log |") && !text.contains("termination") && !text.contains("execution 3"), "{text}");
        let j = &snap.json()["tasks"][0];
        assert_eq!((j["wait"].as_str(), j["execution_wait"].as_str()), (Some(WAIT_TEXTS[5]), Some(raw)));
    }

    #[test]
    fn execution_row_within_ttl_reads_running() {
        let v = execution_status(1, &exec(), &[host(90)], 1000, 300);
        assert_eq!((v["state"].as_str(), v["observed"].as_i64()), (Some("running"), Some(910)));
    }

    #[test]
    fn execution_row_older_than_ttl_reads_unknown() {
        let v = execution_status(1, &exec(), &[host(301)], 1000, 300);
        assert_eq!(v["state"], "unknown");
        assert_eq!(v["reason"], "termination unconfirmed; status missing or stale; restore owning service");
        assert!(v["observed"].is_null());
    }
}
