//! Canonical read model (R6, § 4, § 5): one snapshot of the open tasks renders `status`, `status --json`, the
//! `status.json` tasks and totals, `service status` and the tray counts.

use crate::board::{self, Board};
use crate::core::{self, State, Task, options};
use crate::service::{self, Config, View};
use crate::{Res, age, cell, compact, runtime};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub const SCHEMA: u64 = 1;

/// #1591 the one freshness rule (§ 3 lease, § 4 status, R6) for CLI, tray and menu: an observation `age` seconds old
/// is live within the project ttl, else stale; a negative age (clock skew) is stale.
pub fn live(age: i64, ttl: i64) -> bool { (0..=ttl).contains(&age) }

/// The project ttl of `root`; an unreadable config uses the default.
pub fn ttl(root: &std::path::Path) -> i64 { crate::load(root).map_or(crate::TTL, |(c, _)| c.ttl) }

/// A `status.json` file at `path` is live when its file age passes [`live`].
pub fn status_live(path: &std::path::Path, ttl: i64) -> bool { crate::file_age(path).is_some_and(|a| live(a as i64, ttl)) }
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

/// #1594 every signal one execution's state rests on. [`verdict`] is the only code that turns signals into
/// `running`, `waiting`, `unknown` or `ended`; service, CLI status, tray (via `status.json`), cleanup and the
/// handover wait read its result.
#[derive(Default)]
pub struct Signals<'a> {
    /// This host owns the execution; otherwise only the owner's heartbeat row speaks for it.
    pub local: bool,
    /// Host boot time and the execution's intent time (#1386): a later boot proves the process ended.
    pub boot: Option<(i64, i64)>,
    /// This host's child process of the execution.
    pub child: Option<Child<'a>>,
    /// The launch waits at the admission gate.
    pub held: bool,
    /// The pass's recovery reason for a local execution without a child.
    pub wait: Option<&'a str>,
    /// Native input acceptance is still pending.
    pub input_pending: bool,
    /// The task waits for the owner's answer.
    pub ask: bool,
    /// The owner's matching heartbeat row, only from a heartbeat within the ttl (§ 3 lease).
    pub beat: Option<&'a ExecutionStatus>,
    /// A recorded native process (handover and boot records) and the identity read at its pid now.
    pub process: Option<(&'a coreq::process::Identity, Result<Option<coreq::process::Identity>, String>)>,
}

#[derive(Default)]
pub struct Child<'a> {
    /// The native process exit is observed (or the process started before this boot).
    pub exited: bool,
    /// The native observation failed with this reason.
    pub unknown: Option<&'a str>,
    /// Cancellation was requested; native termination is not yet confirmed.
    pub stopping: bool,
}

const STALE: &str = "termination unconfirmed; status missing or stale; restore owning service";

/// The one execution verdict (§ 3 Recovery): state and a public reason of at most 240 characters.
pub fn verdict(s: &Signals) -> (&'static str, String) {
    let (state, reason): (&'static str, String) = if let Some((at, intent)) = s.boot.filter(|(at, intent)| *intent > 0 && at > intent) {
        ("ended", format!("host booted {} after intent {}", board::stamp(at), board::stamp(intent)))
    } else if s.child.as_ref().is_some_and(|c| c.exited) {
        ("ended", "native exit observed".into())
    } else if let Some((recorded, now)) = &s.process {
        match now {
            Err(e) => ("unknown", format!("recorded native process identity unknown: {e}")),
            Ok(now) if now.as_ref() != Some(*recorded) => ("ended", "recorded native process exited".into()),
            Ok(_) => ("running", "recorded native process observed".into()),
        }
    } else if !s.local {
        match s.beat {
            Some(row) if !row.reason.is_empty() && row.reason.chars().count() <= 240 && !row.reason.chars().any(char::is_control) => {
                match row.state.as_str() {
                    "running" => ("running", row.reason.clone()),
                    "waiting" => ("waiting", row.reason.clone()),
                    "unknown" => ("unknown", row.reason.clone()),
                    _ => ("unknown", STALE.into()),
                }
            }
            _ => ("unknown", STALE.into()),
        }
    } else {
        match &s.child {
            Some(Child { unknown: Some(e), .. }) => ("unknown", e.to_string()),
            Some(c) if c.stopping => ("waiting", "cancellation pending; awaiting native termination".into()),
            _ if s.held => ("waiting", "launch held by admission gate".into()),
            None => ("unknown", s.wait.unwrap_or("native observation unavailable; restore owning runtime").into()),
            _ if s.input_pending => ("waiting", "native input acceptance pending".into()),
            _ if s.ask => ("waiting", "owner answer required".into()),
            _ => ("running", "native execution observed; termination pending".into()),
        }
    };
    (state, reason.chars().map(|c| if c.is_control() { ' ' } else { c }).take(240).collect())
}

/// Board identity and terminal evidence fence every advisory heartbeat observation.
/// A host row counts within the project ttl (§ 3 lease); older reads unknown.
pub(crate) fn execution_status(task:u64, x:&crate::core::Execution, hosts:&[board::Issue], now:i64, ttl:i64)->Value {
    let owners:Vec<_>=hosts.iter().filter(|h|h.trusted && h.title==format!("taskq host {}",x.host)).collect();
    let mut beat=None;
    if let [host]=owners[..] {
        if live(now-host.updated,ttl) {
            let rows=host.body.lines().filter_map(|l|l.strip_prefix("execution ")).map(|text| {
                if text.len()>4096 {return Err("oversized execution status".to_string());}
                serde_json::from_str::<ExecutionStatus>(text).map_err(|e|e.to_string())
            }).collect::<Result<Vec<_>,_>>();
            if let Ok(rows)=rows {
                let matching:Vec<_>=rows.into_iter().filter(|r|r.task==task && r.attempt==x.attempt && r.id==x.id && r.through==x.through).collect();
                if let [row]=&matching[..] {beat=Some((row.clone(),host.updated));}
            }
        }
    }
    let (state,reason)=verdict(&Signals {beat:beat.as_ref().map(|b|&b.0),..Default::default()});
    let observed=beat.filter(|_|state!="unknown" || reason!=STALE).map(|b|b.1);
    let status=ExecutionStatus {task,attempt:x.attempt,id:x.id,through:x.through,state:state.into(),reason};
    let mut value=serde_json::to_value(status).unwrap();
    value["host"]=json!(x.host);value["runtime"]=json!(x.runtime);value["observed"]=json!(observed);value
}

#[derive(Default)]
pub struct Row {
    pub conversation: Option<Value>,
    pub n: u64,
    pub title: String,
    pub labels: Vec<String>,
    pub url: String,
    pub task: Task,
    /// § 2 revision: the expected `--revision` of the next command.
    pub revision: u64,
    /// Latest trusted board comment or issue creation, never the snapshot/poll time (CS2).
    pub updated: Option<i64>,
    /// An unresolved external execution blocks the next submission even after a task state change.
    pub execution_wait: Option<String>,
    pub executions: Vec<Value>,
    /// The runtime accepted input of the current attempt (`accepted` entry): a codex id `-` then reads running, not starting.
    pub accepted: bool,
    /// `Depends on` issues that keep it waiting (§ 1).
    pub deps: Vec<service::Blocker>,
    /// Text of the last trusted `failed` entry.
    pub failed: Option<String>,
    /// Text of the last trusted `later` entry while `later` (R6 timed park reason).
    pub later: Option<String>,
    /// Claude Remote Control URL of the current attempt from its host's heartbeat `session` line (§ 4).
    pub session_url: Option<String>,
    /// Board time of the entry behind the last logged state change (§ 4 Tray age); `None`: no logged change.
    pub since: Option<i64>,
    /// O1: board issue creation and accepted state changes; no inferred execution timings.
    pub created: Option<i64>,
    pub steps: Vec<Value>,
    /// R6 stuck: seconds since the newest log entry; `None`: not stuck.
    pub stuck: Option<i64>,
    /// #1616 TaskQ's own admission decision of a ready or waiting task, or a review awaiting its reviewer (#1685); `None` for other states.
    pub admission: Option<Admission>,
    /// § 5 late answer: outside `ask`, the answer that closed the last question, its writer and text (#1785).
    pub answered: Option<crate::dispatch::Answered>,
    /// #1918: every answered question in board order: the card as asked and the answer that closed it; the
    /// conversation shows each, then the open question, never rebuilt from the task state.
    pub history: Vec<(Option<String>, crate::dispatch::Answered)>,
    /// #1918: op ids of the newest log entries; the tray confirms an accepted write once its op id is here.
    pub ops: Vec<String>,
    /// #1820 a published review this host's service pass left without a merge-queue entry; `None` otherwise.
    pub publication: Option<PublicationWait>,
    /// R6 `stage k/n` (§ 2 Stages): first open stage and Acceptance item count; `None` before any recorded stage.
    pub stage: Option<(usize, usize)>,
    /// § 4 Churn (#1905): set only while the task is flagged.
    pub churn: Option<Churn>,
}

/// § 4 Churn counts of one task's board notes: all notes, `spawn` and `accepted` entries.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Churn { pub notes: u64, pub spawns: u64, pub accepted: u64 }

/// § 4 Churn: over `CHURN_NOTES` notes, or more than `CHURN_SPAWNS` spawns in the last hour lacking their `accepted`.
pub const CHURN_NOTES: u64 = 100;
pub const CHURN_SPAWNS: usize = 5;

/// § 4 Churn of one task from the comments already read at board time `now`; `None`: not flagged.
pub(crate) fn churn(comments: &[board::Comment], now: i64) -> Option<Churn> {
    let entry = |c: &&board::Comment| c.trusted && !c.edited;
    let kind = |c: &board::Comment, k: &str| c.body.lines().next().is_some_and(|l| l.starts_with(&format!("taskq: {k} ")));
    let accepted: Vec<&str> = comments.iter().filter(entry).filter(|c| kind(c, "accepted"))
        .filter_map(|c| c.body.split_whitespace().nth(2)).collect();
    let spawns: Vec<&board::Comment> = comments.iter().filter(entry).filter(|c| kind(c, "spawn")).collect();
    let unaccepted = spawns.iter().filter(|c| now - c.created <= 3600 && !accepted.contains(&c.id.to_string().as_str())).count();
    let c = Churn { notes: comments.len() as u64, spawns: spawns.len() as u64, accepted: accepted.len() as u64 };
    (c.notes > CHURN_NOTES || unaccepted > CHURN_SPAWNS).then_some(c)
}

impl Churn {
    pub fn line(n: u64, c: &Value) -> String {
        format!("#{n} churn: notes {} · spawns {} · accepted {}", c["notes"], c["spawns"], c["accepted"])
    }
}

/// § 4 Churn lines of a `status.json` task list, one per flagged task.
pub fn churn_lines(status: &Value) -> Vec<String> {
    status["tasks"].as_array().into_iter().flatten().filter(|t| t["churn"].is_object())
        .map(|t| Churn::line(t["n"].as_u64().unwrap_or(0), &t["churn"])).collect()
}

/// #1820 (pilotq #106): why a publication waits, from this host's last service pass; never inferred from silence.
#[derive(Clone, Debug, PartialEq)]
pub struct PublicationWait {
    pub reason: String,
    /// Board time of the `publish` entry; `None`: not in the log read.
    pub since: Option<i64>,
    /// Seconds from `since` to the snapshot's board time.
    pub age_s: Option<i64>,
}

/// #1616 fixed admission vocabulary of ready and waiting rows (docs/design/enrollment.md#contract-detail): code and
/// meaning. Consumers read the code and never infer a reason from silence, activity or board totals.
pub const ADMISSION_CODES: [(&str, &str); 13] = [
    ("admitted", "this host claimed the task in this pass"),
    ("dependency", "a Depends on issue is open or unknown"),
    ("hold", "an emergency hold, draining, a pending restart or an older service holds launches"),
    ("host", "a host-* label, a labelled claim or assignee-only assigns the task to another host"),
    ("capability", "a needs-* label or the routed runtime is not available on this host"),
    ("labels", "the task labels are invalid; no host admits it"),
    ("focus", "the host focus excludes the task or admits nothing"),
    ("capacity", "runtime, project or host slots are full or another project has the turn"),
    ("load-gate", "the load gate or launch spacing holds launches"),
    ("login", "the routed runtime is not logged in"),
    ("usage-limit", "the routed runtime is held after a usage limit"),
    ("access", "the routed runtime refused its access grant"),
    ("unknown", "no decision is known; reason names why"),
];

/// The fixed reasons of `unknown` (A3): a host that did not decide never reads as admitted or refused.
pub const ADMISSION_UNKNOWN: [&str; 4] = ["no host decision", "capacity config unreadable", "host admission unknown", "execution unresolved"];

/// One admission decision: `observed` is the board read time it rests on, `host` the deciding host (`None`: TaskQ's
/// board fold, or nobody decided). `reason` is free text except for `unknown`, whose reason is one of
/// [`ADMISSION_UNKNOWN`].
#[derive(Clone, Debug, PartialEq)]
pub struct Admission {
    pub code: &'static str,
    pub reason: String,
    pub observed: i64,
    pub host: Option<String>,
}

impl Admission {
    pub fn json(&self) -> Value {
        debug_assert!(ADMISSION_CODES.iter().any(|c| c.0 == self.code), "admission code outside the vocabulary");
        json!({"code": self.code, "observed": self.observed, "host": self.host, "reason": self.reason})
    }

    /// The `taskq-board/1` form: free reason text stays on the host; `unknown` keeps its fixed reason.
    pub fn projection(&self) -> Value {
        let mut v = json!({"code": self.code, "observed": self.observed, "host": self.host});
        if self.code == "unknown" { v["reason"] = json!(self.reason); }
        v
    }
}

/// The board-only admission of a row: `waiting` is the fold's dependency decision, `ready` waits for a host decision
/// unless its `host-<name>` label names no heartbeat (#1721): no host ever claims it.
fn board_admission(t: &Task, labels: &[String], deps: &[service::Blocker], hosts: &[board::Issue], observed: i64) -> Option<Admission> {
    let orphan = labels.iter().filter_map(|l| l.strip_prefix("host-")).find(|h| !hosts.iter().any(|i| service::host_name(i) == Some(*h)));
    let (code, reason) = match t.state {
        State::Waiting => ("dependency", deps.iter().map(|b| format!("{} {}", b.dep.name(), b.reason)).collect::<Vec<_>>().join(", ")),
        State::Ready => match orphan { Some(h) => ("host", format!("no host {h}")), None => ("unknown", ADMISSION_UNKNOWN[0].to_string()) },
        _ => return None,
    };
    Some(Admission { code, reason, observed, host: None })
}

/// Owner texts of a row's wait (docs/design/menu.md M3 Task row text, #1524); the raw reason stays in `execution_wait`.
pub const WAIT_TEXTS: [&str; 6] = ["Waiting for a free slot", "Waiting for GUI access", "Waiting for the agent to accept input",
    "Waiting for your answer", "Waiting for the previous run to end", "Run state unknown; see service log"];

/// #1820 owner text of a publication wait, followed by its age and reason.
pub const PUBLICATION_WAIT: &str = "Publication waiting";

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
    let leased = || t.attempt.is_some_and(|a| hosts.iter().any(|h| h.trusted && live(now - h.updated, ttl) && h.body.lines().any(|l| l == format!("#{n} {a}"))));
    match t.state {
        State::Waiting => deps.iter().all(|b| b.reason.starts_with("unknown: closed")),
        State::Held | State::Ask | State::Later => age >= STUCK_AFTER,
        // #1752: a review whose reviewer no host leases waits silently otherwise
        State::Doing | State::Review => age >= STUCK_AFTER && !leased(),
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
    /// The code of this host's tray interface fault (§ 4 step 3, #1755).
    pub tray: Option<String>,
    /// Heartbeat `link <installation-id> <public key>` line ([host link](../docs/design/host-link.md), #1832).
    pub link: Option<String>,
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

/// #1918: how many newest op ids a row lists; more writes than this between two board reads of one task are not expected.
const OPS: usize = 16;

/// The one snapshot function: open tasks by priority then number, plus host heartbeats.
pub fn from_views(source: &'static str, b: &dyn Board, cfg: &Config, observed: i64, views: &[View], hosts: &[board::Issue]) -> Snapshot {
    let mut open: Vec<&View> = views.iter().filter(|v| v.projection.task.state != State::Closed).collect();
    open.sort_by_key(|v| (service::priority(&v.issue), v.issue.number));
    let urls = session_urls(hosts);
    let tasks = open.into_iter().map(|v| {
        let executions:Vec<_>=v.projection.executions.iter().filter(|x|x.release.is_none())
            .map(|x| {
                let mut value=execution_status(v.issue.number,x,hosts,observed,cfg.ttl);
                // #1775 A3: `spawn reviewer` keeps `review`, so `changes` (state moves only) misses it; the current attempt's role is the fold's
                let task=&v.projection.task;
                value["role"]=json!(if task.attempt==Some(x.attempt) {task.role.as_deref()}
                    else {v.projection.changes.iter().find(|(id,_,_)| *id==x.attempt).and_then(|(_,_,t)|t.role.as_deref())});
                value
            }).collect();
        Row {
        n: v.issue.number,
        conversation: core::conversation(v.issue.number,&v.log).map(|id| json!({"id":id,"brief":v.issue.body,"verdict":v.projection.changes.iter().rev().find_map(|(id,_,_)|v.log.iter().find(|e|e.id==*id && ["taskq: result ","taskq: close","taskq: publish"].iter().any(|prefix|e.body.starts_with(prefix))).and_then(|e|e.body.split_once('\n').map(|(_,text)|text)))})),
        title: v.issue.title.clone(),
        labels: v.issue.labels.clone(),
        url: b.url(v.issue.number),
        task: v.projection.task.clone(),
        revision: v.projection.revision,
        updated: v.comments.iter().filter(|c|c.trusted&&!c.edited).map(|c|c.created).chain(Some(v.issue.created)).filter(|t|*t>0).max(),
        execution_wait: executions.iter().find(|x|x["state"]!="running" || x["attempt"].as_u64()!=v.projection.task.attempt)
            .map(|x|format!("execution {} on {}: {}: {}",x["id"],x["host"].as_str().unwrap_or("?"),
                x["state"].as_str().unwrap_or("unknown"),x["reason"].as_str().unwrap_or("observation unavailable")))
            .or_else(|| v.projection.executions.iter().find(|x| Some(x.attempt) == v.projection.task.attempt && !v.projection.task.inbox.is_empty()
                && x.release == Some(crate::core::Release::Exit) && !x.accepted)
                .map(|x| format!("execution {} on {}: input acceptance unknown; reconcile native input", x.id, x.host))),
        executions,
        accepted: v.projection.executions.iter().any(|x| Some(x.attempt) == v.projection.task.attempt && x.accepted),
        deps: v.blockers.clone(),
        admission: board_admission(&v.projection.task, &v.issue.labels, &v.blockers, hosts, observed),
        answered: crate::dispatch::answered_in(v.issue.number, &v.log, &v.projection, &v.comments),
        history: crate::dispatch::history(v.issue.number, &v.log, &v.projection, &v.comments),
        ops: v.log.iter().rev().filter_map(|e| crate::core::op_id(v.issue.number, &e.body)).take(OPS).map(String::from).collect(),
        publication: None,
        stage: crate::core::stage(&v.issue.body, &v.projection.task.stages),
        session_url: v.projection.task.attempt.zip(v.projection.task.session.as_deref().and_then(|s| s.rsplit(' ').next()))
            .filter(|_| v.projection.task.session.as_deref().is_some_and(|s| crate::runtime::remote_session(s).is_some()))
            .and_then(|(a, h)| urls.get(&(h.to_string(), a)).cloned()),
        stuck: stuck(v.issue.number, &v.projection.task, &v.blockers,
            v.comments.iter().filter(|c| c.trusted && c.body.starts_with("taskq: ")).map(|c| c.created).max().unwrap_or(v.issue.created),
            hosts, observed, cfg.ttl),
        churn: churn(&v.comments, observed),
        created: Some(v.issue.created).filter(|t| *t > 0),
        steps: stage_history(v),
        since: v.projection.changes.last().and_then(|(id, ..)| v.comments.iter().find(|c| c.id == *id)).map(|c| c.created),
        failed: (v.projection.task.state == State::Failed).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: failed")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
        later: (v.projection.task.state == State::Later).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: later")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
    }}).collect();
    let hosts = hosts.iter().map(|h| Host {
        diagnostics: crate::service_versions::read(&h.body,h.updated,observed,cfg.ttl as i64,false),
        name: h.title.trim_start_matches("taskq host ").to_string(),
        service: h.body.lines().find_map(|l| l.strip_prefix("service ")).map(str::to_string),
        version: h.body.lines().find_map(|l| l.strip_prefix("version ")).unwrap_or("?").to_string(),
        updated: h.updated,
        focus: h.body.lines().find_map(|l| l.strip_prefix("focus ")).map(str::to_string),
        limits: h.body.lines().filter_map(|l| l.strip_prefix("limit ")).map(str::to_string).collect(),
        tray: h.body.lines().find_map(|l| l.strip_prefix("tray ")).map(str::to_string),
        // only a heartbeat the board trusts may name a key a host serves
        link: h.body.lines().find_map(|l| l.strip_prefix("link ")).filter(|_| h.trusted).map(str::to_string),
    }).collect();
    Snapshot { source, observed, reason: None, board: identity(b, cfg), tasks, hosts }
}

/// O1: ordered board stages, with unknown boundaries retained. The fold still owns transitions.
fn stage_history(v: &View) -> Vec<Value> {
    let mut steps = Vec::new();
    let mut state = v.projection.changes.first().map_or(v.projection.task.state, |(_, before, _)| before.state);
    let mut start = Some(v.issue.created).filter(|t| *t > 0);
    for (id, _, after) in &v.projection.changes {
        let end = v.comments.iter().find(|c| c.id == *id).map(|c| c.created).filter(|t| *t > 0);
        let valid = start.zip(end).is_some_and(|(a,b)| b >= a);
        steps.push(json!({"state": state.name(), "start": if valid { start } else { None }, "end": end, "complete": true}));
        state = after.state;
        start = end;
    }
    steps.push(json!({"state": state.name(), "start": start, "end": null, "complete": false}));
    steps
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
    /// R6: ` · stage k/n` for a task with recorded stages (§ 2 Stages), else nothing.
    fn stage_text(&self) -> String {
        self.stage.map(|(k, n)| format!(" · stage {k}/{n}")).unwrap_or_default()
    }

    /// The owner text of the row's wait: a publication wait, else the execution diagnostic, else a capacity refusal with its runtime (#1685),
    /// else the board's own host refusal (R6, #1721).
    fn wait(&self) -> Option<String> {
        // #1820: the reviewer has ended, so a publication wait outranks its execution diagnostic
        if let Some(p) = &self.publication {
            return Some(format!("{} {} · {}", PUBLICATION_WAIT, p.age_s.map_or("?".into(), crate::display::age), compact(&p.reason, 200)));
        }
        if let Some(reason) = &self.execution_wait { return Some(wait_text(reason).into()); }
        let a = self.admission.as_ref()?;
        match (a.code, &a.host) {
            ("capacity", _) => Some(format!("{} · {}", WAIT_TEXTS[0], a.reason)),
            ("host", None) => Some(a.reason.replacen("no host", "No host", 1)),
            _ => None,
        }
    }

    pub fn json(&self) -> Value {
        let mut row = json!({"n": self.n, "title": self.title, "state": self.task.state.name(), "labels": self.labels, "url": self.url, "revision": self.revision, "updated": self.updated,
            "session": self.task.session, "attempt": self.task.attempt, "role": self.task.role,
            "deps": self.deps.iter().filter(|b| b.dep.repo.is_none() && b.reason == "open").map(|b| b.dep.n).collect::<Vec<_>>(),
            "blocked": self.deps.iter().map(|b| json!({"ref": b.dep.name(), "url": b.url, "reason": b.reason})).collect::<Vec<_>>(),
            "card": self.task.card, "question": self.task.card.as_deref().map(question), "failed": self.failed,
            "controls": controls(self.task.state.name())});
        if let Some(c) = &self.conversation { row["conversation"] = c.clone(); }
        if let Some(id) = &self.task.enqueued { row["enqueued"] = json!(id); }
        if let Some(sha) = &self.task.result { row["result"] = json!(sha); }
        // R6 timed park: the UTC time and its reason; clients show it in local time
        if let Some(u) = &self.task.until { row["until"] = json!(u); row["later"] = json!(self.later); }
        // R6 / § 2 Stages: `stage k/n` and the finished stages
        if let Some((k, n)) = self.stage { row["stage"] = json!({"k": k, "n": n, "done": self.task.stages}); }
        // § 2: the PM's pick sits beside the worker's star; it is never an answer
        if let Some((k, reason)) = &self.task.pick { row["pick"] = json!({"id": k, "reason": reason}); }
        if !self.executions.is_empty() { row["executions"]=json!(self.executions); }
        if let Some(reason) = &self.execution_wait { row["execution_wait"] = json!(reason); }
        if let Some(w) = self.wait() { row["wait"] = json!(w); }
        if let Some(u) = &self.session_url { row["session_url"] = json!(u); }
        row["since"] = json!(self.since);
        row["created"] = json!(self.created);
        row["steps"] = json!(self.steps);
        row["stuck"] = json!(self.stuck.is_some());
        if let Some(a) = self.stuck { row["age_s"] = json!(a); }
        if let Some(c) = self.churn { row["churn"] = json!({"notes": c.notes, "spawns": c.spawns, "accepted": c.accepted}); }
        if let Some(a) = &self.admission { row["admission"] = a.json(); }
        if let Some(a) = &self.answered { row["answered"] = json!(a); }
        if !self.history.is_empty() {
            row["history"] = json!(self.history.iter().map(|(c, a)| json!({"question": c.as_deref().map_or(Value::Null, question), "answered": a})).collect::<Vec<_>>());
        }
        if !self.ops.is_empty() { row["ops"] = json!(self.ops); }
        if let Some(p) = &self.publication { row["publication"] = json!({"reason": p.reason, "since": p.since, "age_s": p.age_s}); }
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

/// § 5 board line width; human input has no line-count cap.
pub const ANSWER_CHARS: usize = 240;

/// § 2 visual evidence: the service's copy of an ask image `url`, which the app (no board token) shows inline.
pub fn image_copy(root: &std::path::Path, url: &str) -> std::path::PathBuf {
    let key: String = ring::digest::digest(&ring::digest::SHA256, url.as_bytes()).as_ref()[..8].iter().map(|b| format!("{b:02x}")).collect();
    crate::dot(root).join("images").join(key)
}

/// The image URLs of a card's Evidence line.
pub fn card_images(card: &str) -> Vec<String> {
    question(card)["evidence"].as_array().into_iter().flatten().filter(|e| e["image"] == true).filter_map(|e| e["url"].as_str().map(String::from)).collect()
}

/// Canonical parsed fields of an open card (§ 2, docs/design/answer-card.md), the one parse every card UI reads.
/// `parsed` false: a reader shows `text` with the board link and no write controls.
pub fn question(card: &str) -> Value {
    let field = |p: &str| card.lines().find_map(|l| l.strip_prefix(p)).map(str::trim);
    let evidence: Vec<Value> = field("Evidence: ").unwrap_or("").split("), ").filter_map(|l| {
        // `![alt](url)`: an image the card UIs show inline (§ 2 visual evidence)
        let (image, l) = l.strip_prefix('!').map_or((false, l), |l| (true, l));
        let (label, url) = l.strip_prefix('[')?.split_once("](")?;
        let url = url.trim_end_matches(')');
        // a video the board plays inline (#1844) stays a link in the apps
        let video = [".mp4", ".webm"].iter().any(|e| url.to_ascii_lowercase().ends_with(e));
        Some(json!({"label": label, "url": url, "image": image && !video}))
    }).collect();
    let o = options(card);
    // Publication and exact-SHA approval choices stay on the board.
    // § 2: `<label> - <detail>`; a card without ` - ` (written before #1787, or a service card) shows its whole text as the label
    let choices: Vec<Value> = o.iter().map(|(k, t, star)| {
        let (label, detail) = t.split_once(" - ").map_or((t.as_str(), None), |(l, d)| (l.trim(), Some(d.trim())));
        json!({"id": k, "text": t, "label": label, "detail": detail, "recommended": star, "consequential": t.to_lowercase().starts_with("close") || core::approval_sha(t).is_some()})
    }).collect();
    // § 2: options numbered 1..n in order, no gap or repeat; none is an owner action answered with text
    let numbered = o.iter().enumerate().all(|(i, x)| x.0 == i + 1);
    let parsed = field("Problem: ").is_some() && numbered && (o.is_empty() || (2..=4).contains(&o.len()) && o.iter().filter(|x| x.2).count() == 1);
    json!({"parsed": parsed, "text": card, "done": field("Done: "), "problem": field("Problem: ").map(|_| brief(card)), "evidence": evidence,
        "choices": choices, "limits": {"chars": ANSWER_CHARS}})
}

#[test]
fn review_approval_choices_require_the_board() {
    for action in ["close: ship".to_string(), format!("approve {}", "a".repeat(40))] {
        let q = question(&format!("Problem: Decide\n1. {action} ★\n2. keep reviewing"));
        assert_eq!(q["choices"][0]["consequential"], true);
        assert_eq!(q["choices"][1]["consequential"], false);
    }
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
            "hosts": self.hosts.iter().map(|h| {
                let mut r = json!({"name": h.name, "version": h.version, "updated": h.updated, "diagnostics": h.diagnostics});
                if let Some(l) = &h.link { r["link"] = json!(l) }
                r
            }).collect::<Vec<_>>(),
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
                let state = if let Some(w) = r.wait() {
                    format!("{} · {}", r.task.state.name(), cell(&w))
                } else if r.task.state == State::Waiting {
                    let deps: Vec<String> = r.deps.iter().map(|b| format!("[{}]({}) {}", b.dep.name(), b.url, cell(&compact(&b.reason, 80)))).collect();
                    format!("blocked ({})", deps.join(", "))
                } else {
                    r.task.state.name().to_string()
                };
                let state = format!("{state}{}", r.stage_text());
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
        for state in [State::Held, State::Later] {
            let rows: Vec<String> = self.tasks.iter().filter(|r| r.task.state == state).map(|r| match r.task.until.as_deref().and_then(crate::board::iso) {
                Some(at) => format!("{}{} until {}: {}", link(r), r.stage_text(), local_time(at), compact(r.later.as_deref().unwrap_or("").trim_start_matches("Reason: "), 160)),
                None => format!("{}{}", link(r), r.stage_text()),
            }).collect();
            if !rows.is_empty() { parked.push(format!("{}: {}", crate::display::name(state.name()), rows.join(", "))); }
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
        // #1625: a heartbeat older than ttl reads stale, so a stopped service shows within ttl of its last beat
        let hs: Vec<String> = self.hosts.iter().map(|h| format!("{} v{} {}{}{}", h.name, h.version, age(self.observed - h.updated),
            if live(self.observed - h.updated, cfg.ttl) { "" } else { " stale" }, h.focus.as_ref().map(|f| format!(" focus {f}")).unwrap_or_default()
            + &h.limits.iter().map(|l| l.split_once(' ').map_or_else(|| format!(" {l}"), |(rt, at)| format!(" {rt} limit {at}"))).collect::<String>()
            + &h.tray.as_ref().map(|code| format!(" tray {code}")).unwrap_or_default())).collect();
        if !hs.is_empty() {
            out.push(String::new());
            out.push(format!("Hosts: {}", hs.join(", ")));
        }
        Ok(out.join("\n"))
    }
}

/// R6 timed park: board time `t` on this host's clock, `2026-11-05 10:00`.
pub fn local_time(t: i64) -> String {
    let s = crate::board::stamp(t + crate::platform::utc_offset(t));
    format!("{} {}", &s[..10], &s[11..16])
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
    let own = host == cfg.me;
    let (id, url) = runtime::session_link(&cfg.root, own, rt, id, n, t.attempt.unwrap_or(0), t.role.as_deref().unwrap_or_default(), r.session_url.clone());
    let host = cell(host);
    let link = match rt {
        _ if id == "-" && r.accepted => format!("{host}: running"),
        _ if id == "-" => format!("{host}: starting"),
        _ if runtime::of(rt).remote => url.map_or_else(|| format!("{host}: Remote Control unknown"), |url| format!("[{host}]({url})")),
        // R6 3: a Codex thread opens only on its own host; elsewhere the DashQ progress page, else the host alone
        _ if !own => crate::enrollment::progress_page(&cfg.install, &cfg.repo).map_or_else(|| host.clone(), |p| format!("[{host}]({p}{n})")),
        _ => match url {
            None => host.clone(),
            Some(url) if std::env::var("TASKQ_CLIENT").is_ok_and(|c| runtime::of(&c).codex && runtime::of(&c).child) => format!("[{host}]({url})"),
            Some(url) => match raw["pages"].as_str() {
                Some(p) => format!("[{host}]({}/open.html#{url})", p.trim_end_matches('/')),
                None => format!("{host}: `{url}`"),
            },
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

    // O1/R1/R12: retain accepted board stages, including missing dates; ignored entries cannot invent a step.
    #[test]
    fn dashboard_stages_use_only_accepted_board_changes() {
        let log=vec![crate::core::Entry{id:1,body:"taskq: hold".into(),edited:false},
            crate::core::Entry{id:2,body:"taskq: start".into(),edited:false},
            crate::core::Entry{id:3,body:"taskq: spawn worker codex:s mac".into(),edited:false},
            crate::core::Entry{id:4,body:"taskq: result deadbeef attempt 999".into(),edited:false}];
        let comments=vec![board::Comment{id:1,body:log[0].body.clone(),created:100,trusted:true,edited:false,author:String::new()},
            board::Comment{id:2,body:log[1].body.clone(),created:150,trusted:true,edited:false,author:String::new()}];
        let v=View {issue:board::Issue{number:1,created:100,..Default::default()},projection:crate::core::project(1,&log,false),log,comments,blockers:vec![]};
        let steps=stage_history(&v);
        assert_eq!(steps.iter().map(|s|s["state"].as_str().unwrap()).collect::<Vec<_>>(),["ready","held","ready","doing"]);
        assert_eq!(steps[1],json!({"state":"held","start":100,"end":150,"complete":true}));
        assert!(steps[2]["start"].is_null() && steps[2]["end"].is_null(),"unknown boundary never becomes a duration");
        assert!(steps[3]["start"].is_null() && steps[3]["end"].is_null() && steps[3]["complete"]==false);
    }

    #[test]
    fn remote_codex_reads_starting_until_accepted() {
        let task = Task { attempt: Some(5), session: Some("codex:- win".into()), ..Default::default() };
        let mut r = Row { conversation: None, n: 1, title: String::new(), labels: vec![], url: String::new(), task, revision: 5, updated: None, execution_wait: None,
            executions: vec![], accepted: false, deps: vec![], failed: None, later: None, session_url: None, since: None, created: None, steps: vec![], stuck: None, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None };
        let cfg = crate::tests::cfg();
        assert_eq!(session_link(&cfg, &json!({}), &r).1, "win: starting");
        r.accepted = true;
        assert_eq!(session_link(&cfg, &json!({}), &r).1, "win: running");
    }

    // Chain: § 5 late answer -> status row -> tray, DashQ, PilotQ (#1785). Risk: a closed question that only the
    // answering client's receipt names, so every other surface still offers it or shows a stale error.
    #[test]
    fn rows_name_the_answer_that_closed_the_question() {
        let mut r = Row { n: 1, ..Default::default() };
        assert!(r.json().get("answered").is_none(), "no answer, no field");
        r.answered = Some(crate::dispatch::Answered { by: "alex".into(), text: "use x".into() });
        assert_eq!(r.json()["answered"], json!({"by": "alex", "text": "use x"}));
    }

    // Chain: R6 3 session cell per runtime and host (#1565). Risk: a Codex thread link shown off its host, where it
    // cannot open, or the DashQ progress page replacing the owning host's thread link.
    #[test]
    fn codex_thread_links_only_on_its_host_else_dashq_progress() {
        use std::fs;
        let task = |host: &str| Task { attempt: Some(5), session: Some(format!("codex:thread-1 {host}")), ..Default::default() };
        let row = |host: &str| Row { conversation: None, n: 3, title: String::new(), labels: vec![], url: String::new(), task: task(host), revision: 5, updated: None, execution_wait: None,
            executions: vec![], accepted: true, deps: vec![], failed: None, later: None, session_url: None, since: None, created: None, steps: vec![], stuck: None, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None };
        let mut cfg = crate::tests::cfg();
        cfg.me = "mac".into();
        let pages = json!({"pages": "https://p"});
        assert_eq!(session_link(&cfg, &pages, &row("mac")).1, "[mac](https://p/open.html#codex://threads/thread-1)");
        assert_eq!(session_link(&cfg, &pages, &row("win")).1, "win", "no connection: the host alone");
        cfg.install = std::env::temp_dir().join(format!("taskq-progress-page-{}", std::process::id()));
        let id = crate::enrollment::installation_id(&cfg.install).unwrap();
        let conn = json!({"destination": "https://dash.example/machine", "credential": "c"});
        fs::create_dir_all(crate::enrollment::dir(&cfg.install)).unwrap();
        fs::write(crate::enrollment::dir(&cfg.install).join("connection.json"), json!({"installation": id, "pending": null, "active": conn, "revoke": []}).to_string()).unwrap();
        assert_eq!(session_link(&cfg, &pages, &row("win")).1, "[win](https://dash.example/projects#board-o%2Fr-task-3)");
        fs::remove_dir_all(&cfg.install).unwrap();
    }

    // Chain: R6 stuck. Risk: a silent task never shows, or a moving or leased one is flagged.
    #[test]
    fn stuck_flags_each_state_from_two_hours_and_never_a_moving_task() {
        let t = |state, attempt| Task { state, attempt, ..Default::default() };
        let dep = |reason: &str| service::Blocker { dep: crate::core::depends("Depends on #2").remove(0), url: String::new(), reason: reason.into() };
        let beat = |age: i64| board::Issue { title: "taskq host mac".into(), body: "version 4\n#1 5".into(), updated: 10_000 - age, trusted: true, ..Default::default() };
        let at = |task: &Task, deps: &[service::Blocker], age: i64, hosts: &[board::Issue]| stuck(1, task, deps, 10_000 - age, hosts, 10_000, 300);
        for s in [State::Held, State::Ask, State::Later, State::Doing, State::Review] {
            assert_eq!(at(&t(s, Some(5)), &[], STUCK_AFTER - 1, &[]), None, "{s:?} under 2 h");
            assert_eq!(at(&t(s, Some(5)), &[], STUCK_AFTER, &[]), Some(STUCK_AFTER), "{s:?} at 2 h");
            assert_eq!(at(&t(s, Some(5)), &[], 60, &[]), None, "{s:?} moving");
        }
        // a doing attempt its fresh heartbeat lists is leased; a stale heartbeat or another attempt is not
        assert_eq!(at(&t(State::Doing, Some(5)), &[], 3 * STUCK_AFTER, &[beat(10)]), None);
        assert_eq!(at(&t(State::Doing, Some(5)), &[], 3 * STUCK_AFTER, &[beat(301)]), Some(3 * STUCK_AFTER)); // #1591: live through ttl, stale after it
        assert_eq!(at(&t(State::Doing, Some(6)), &[], 3 * STUCK_AFTER, &[beat(10)]), Some(3 * STUCK_AFTER));
        // #1752: a review with a leased reviewer is not stuck; one whose reviewer never starts is
        assert_eq!(at(&t(State::Review, Some(5)), &[], 3 * STUCK_AFTER, &[beat(10)]), None);
        assert_eq!(at(&t(State::Review, Some(6)), &[], 3 * STUCK_AFTER, &[beat(10)]), Some(3 * STUCK_AFTER));
        for s in [State::Ready, State::Failed] {
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
        let row = |n, stuck| Row { conversation: None, n, title: "T".into(), labels: vec![], url: format!("u{n}"), task: Task { state: State::Ask, ..Default::default() }, revision: 0, updated: None,
            execution_wait: None, executions: vec![], accepted: false, deps: vec![], failed: None, later: None, session_url: None, since: None, created: None, steps: vec![], stuck, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None };
        let snap = Snapshot { source: "board", observed: 0, reason: None, board: json!({"url": "b"}), tasks: vec![row(1, Some(9000)), row(2, None)], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("Stuck: ")).collect::<Vec<_>>(), ["Stuck: [#1 T](u1) (ask, 2h)"], "{text}");
        let rows = snap.json()["tasks"].clone();
        assert_eq!((rows[0]["stuck"].clone(), rows[0]["age_s"].clone()), (json!(true), json!(9000)));
        assert_eq!((rows[1]["stuck"].clone(), rows[1].get("age_s")), (json!(false), None));
    }

    // Chain: #1625 R6 6 Hosts line. Risk: a host whose service stopped keeps reading as a plain age, never as down.
    #[test]
    fn hosts_line_marks_heartbeat_older_than_ttl_stale() {
        let ttl = crate::tests::cfg().ttl;
        let host = |name: &str, updated| Host { name: name.into(), version: "1.0".into(), updated, ..Default::default() };
        let snap = Snapshot { source: "board", observed: 10_000, reason: None, board: json!({"url": "b"}), tasks: vec![],
            hosts: vec![host("mac", 10_000 - ttl), host("win", 10_000 - ttl - 1)] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let line = text.lines().find(|l| l.starts_with("Hosts: ")).unwrap();
        assert!(line.contains("mac v1.0 ") && !line.split(", ").next().unwrap().contains("stale") && line.split(", ").nth(1).unwrap().ends_with(" stale"), "{line}");
    }

    // Chain: § 4 Host link heartbeat key. Risk: an untrusted `taskq host` issue publishes a key that hosts then serve.
    #[test]
    fn host_link_keys_come_only_from_trusted_heartbeats() {
        let beat = |title: &str, trusted| board::Issue { title: title.into(), body: "version 4
link aa bb".into(), updated: 1000, trusted, ..Default::default() };
        let snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), 1000, &[], &[beat("taskq host mac", true), beat("taskq host evil", false)]);
        let hosts = snap.json()["hosts"].clone();
        assert_eq!((hosts[0]["link"].clone(), hosts[1].get("link")), (json!("aa bb"), None));
    }

    // Chain: R6 timed park -> status `Later:` line and JSON row (#1823). Risk: a timed park reads as open-ended, or
    // the owner sees the raw UTC time instead of this host's clock.
    #[test]
    fn timed_park_shows_its_local_time_and_reason() {
        let at = crate::board::iso("2026-11-05T09:00:00Z").unwrap();
        let row = |n, until: Option<&str>| Row { n, title: format!("T{n}"), url: format!("u{n}"), later: Some("Reason: 168 qualified hours".into()),
            task: Task { state: State::Later, until: until.map(str::to_string), ..Default::default() }, ..Default::default() };
        let snap = Snapshot { source: "board", observed: at, reason: None, board: json!({"url": "https://github.com/o/r/issues"}), hosts: vec![],
            tasks: vec![row(1, Some("2026-11-05T09:00:00Z")), row(2, None)] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains(&format!("Later: [#1 T1](u1) until {}: 168 qualified hours, [#2 T2](u2)", local_time(at))), "{text}");
        let off = crate::platform::utc_offset(at);
        assert!(off.abs() <= 14 * 3600 && off % 900 == 0, "a real zone offset: {off}");
        assert_eq!(local_time(at), format!("{} {}", &crate::board::stamp(at + off)[..10], &crate::board::stamp(at + off)[11..16]));
        let rows = snap.json()["tasks"].clone();
        assert_eq!((rows[0]["until"].as_str(), rows[0]["later"].as_str()), (Some("2026-11-05T09:00:00Z"), Some("Reason: 168 qualified hours")));
        assert!(rows[1].get("until").is_none(), "an untimed park adds no keys");
    }

    // Chain: § 2 Stages -> R6 status State cell, `Later:` line and JSON row (#1880). Risk: a multi-stage task hides
    // its progress, or a task without stage records gains stage text or keys.
    #[test]
    fn staged_task_shows_stage_k_of_n() {
        let at = crate::board::iso("2026-11-05T09:00:00Z").unwrap();
        let row = |n, state, stage| Row { n, title: format!("T{n}"), url: format!("u{n}"), stage,
            task: Task { state, stages: if stage.is_some() { vec!["A1".into()] } else { vec![] }, ..Default::default() }, ..Default::default() };
        let snap = Snapshot { source: "board", observed: at, reason: None, board: json!({"url": "https://github.com/o/r/issues"}), hosts: vec![],
            tasks: vec![row(1, State::Later, Some((2, 4))), row(2, State::Ready, Some((3, 4))), row(3, State::Ready, None)] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("Later: [#1 T1](u1) · stage 2/4") && text.contains("ready · stage 3/4"), "{text}");
        assert_eq!(text.matches("stage ").count(), 2, "{text}");
        let rows = snap.json()["tasks"].clone();
        assert_eq!(rows[0]["stage"], json!({"k": 2, "n": 4, "done": ["A1"]}));
        assert!(rows[2].get("stage").is_none(), "no stage record adds no key");
    }

    // Chain: #1592 A4 one display model. Risk: the CLI and the tray page disagree on counts, order, state names or age
    // text for one board. The golden fixture feeds the page tests (tests/display-model.test.cjs) the same model.
    #[test]
    fn cli_and_page_model_agree_on_one_board() {
        let now = 200_000;
        let row = |n, state, labels: &[&str], ago: i64| Row { n, title: format!("T{n}"), labels: labels.iter().map(|l| l.to_string()).collect(), url: format!("https://github.com/o/r/issues/{n}"),
            task: Task { state, ..Default::default() }, since: Some(now - ago), stuck: (state == State::Ask).then_some(ago), ..Default::default() };
        let snap = Snapshot { source: "board", observed: now, reason: None, board: json!({"url": "https://github.com/o/r/issues"}), hosts: vec![], tasks: vec![
            row(3, State::Ready, &["p1"], 300), row(5, State::Ready, &[], 7200), row(7, State::Doing, &[], 60), row(8, State::Ready, &["p0"], 30),
            row(9, State::Ask, &[], 9000), row(2, State::Held, &[], 90_000), row(4, State::Later, &[], 600)] };
        // the `from_views` key: priority, then number
        let mut snap = snap;
        snap.tasks.sort_by_key(|r| (crate::display::priority(&r.labels), r.n));
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let mut ps = [json!({"name": "a", "root": "a", "issues": "https://github.com/o/r/issues", "board_kind": "github", "complete": true, "focus": [], "saved": true,
            "tasks": snap.json()["tasks"]})];
        let display = crate::display::model(&mut ps, now);
        let n = |s: &str| display["all"]["counts"].as_array().unwrap().iter().find(|c| c["state"] == s).unwrap()["n"].as_u64().unwrap();
        assert_eq!(text.lines().nth(1).unwrap(), format!("In work {} · Waiting for answer {} · Ready {}", n("doing") + n("review"), n("ask"), n("ready")));
        let rows = ps[0]["tasks"].as_array().unwrap();
        let ready: Vec<String> = rows.iter().filter(|t| t["state"] == "ready").map(|t| format!("[#{} ", t["n"])).collect();
        let table: Vec<&str> = text.lines().filter(|l| l.contains("| ready |")).collect();
        assert_eq!(table.len(), ready.len(), "{text}");
        assert!(table.iter().zip(&ready).all(|(l, r)| l.starts_with(&format!("| {r}"))), "ready order {ready:?}\n{text}");
        for t in rows.iter().filter(|t| ["held", "later"].contains(&t["state"].as_str().unwrap())) {
            assert!(text.contains(&format!("{}: [#{} ", t["state_name"].as_str().unwrap(), t["n"])), "{text}");
        }
        let ask = rows.iter().find(|t| t["state"] == "ask").unwrap();
        assert!(text.contains(&format!("(ask, {})", ask["age"].as_str().unwrap())), "{text}");
        assert_eq!(ps[0]["mode"], "paused");
        // the first line names the per-test project directory
        let got = json!({"cli": text.lines().skip(1).collect::<Vec<_>>().join("\n"), "now": now, "display": display, "projects": ps});
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/display-model.json");
        if std::env::var_os("TASKQ_BLESS").is_some() { std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap(); }
        let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
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
            ("spawn 3 not started on mac: input not accepted; lost spawn released no-start 300 s after the spawn", 2),
            ("execution 3 on mac: termination unconfirmed; reboot mac to prove exit", 4),
            ("execution 3 on hermes: termination unconfirmed; start the taskq service on hermes once, after a reboot if the run had started", 4),
        ];
        for (raw, k) in cases { assert_eq!(wait_text(raw), WAIT_TEXTS[k], "{raw}"); }
        // status text and row JSON: owner text only in the row, raw reason kept in --json
        let raw = cases[11].0;
        let row = Row { conversation: None, n: 1, title: "T".into(), url: "u1".into(), task: Task { state: State::Doing, ..Default::default() }, execution_wait: Some(raw.into()), ..Default::default() };
        let snap = Snapshot { source: "board", observed: 0, reason: None, board: json!({"url": "b"}), tasks: vec![row], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("| doing · Run state unknown; see service log |") && !text.contains("termination") && !text.contains("execution 3"), "{text}");
        let j = &snap.json()["tasks"][0];
        assert_eq!((j["wait"].as_str(), j["execution_wait"].as_str()), (Some(WAIT_TEXTS[5]), Some(raw)));
    }

    // #1775 A3: a reviewer spawned in review leaves `changes` (state moves only). Risk: its execution row has no role,
    // the menu host strip reads the host as unknown and hides its Claude/Codex counts.
    #[test]
    fn reviewer_execution_row_carries_reviewer_role() {
        let sha = "a".repeat(40);
        let log = vec![crate::core::Entry{id:9,body:"taskq: spawn worker claude:s0 mac".into(),edited:false},
            crate::core::Entry{id:10,body:format!("taskq: result {sha} attempt 9\nChanged: x"),edited:false},
            crate::core::Entry{id:11,body:"taskq: spawn reviewer codex:- mac".into(),edited:false}];
        let v = View {issue:board::Issue{number:1,..Default::default()},projection:crate::core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), 1000, &[v], &[]);
        let roles: Vec<_> = snap.tasks[0].executions.iter().map(|x| (x["attempt"].as_u64(), x["role"].as_str())).collect();
        assert!(roles.contains(&(Some(11), Some("reviewer"))), "{roles:?}");
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

    // #1594 A3: the owner's verdict, published in its heartbeat and read by another host or the tray, stays the same;
    // only a childless verdict other than `ended` holds a handover. Risk: a caller re-derives liveness and a fix lands
    // in one place only (#947 foreign or childless execution, #1036 exited child, #1386 boot proof, #1481/#1491
    // heartbeat ttl, #1551 cleanup after reboot).
    #[test]
    fn one_verdict_through_every_caller() {
        let (done, failed, stopping) = (Child { exited: true, ..Default::default() }, Child { unknown: Some("io"), ..Default::default() }, Child { stopping: true, ..Default::default() });
        let (rec, reused) = (coreq::process::Identity { pid: 9, started: "1-0".into() }, coreq::process::Identity { pid: 9, started: "2-0".into() });
        let cases: Vec<(&str, Signals, &str, &str)> = vec![
            ("#947 childless", Signals { local: true, ..Default::default() }, "unknown", "native observation unavailable; restore owning runtime"),
            ("#947 recovery wait", Signals { local: true, wait: Some("termination unconfirmed"), ..Default::default() }, "unknown", "termination unconfirmed"),
            ("#1036 exited", Signals { local: true, child: Some(done), ..Default::default() }, "ended", "native exit observed"),
            ("#1036 observation failed", Signals { local: true, child: Some(failed), ..Default::default() }, "unknown", "io"),
            ("stopping", Signals { local: true, child: Some(stopping), ..Default::default() }, "waiting", "cancellation pending; awaiting native termination"),
            ("running", Signals { local: true, child: Some(Child::default()), ..Default::default() }, "running", "native execution observed; termination pending"),
            ("input", Signals { local: true, child: Some(Child::default()), input_pending: true, ..Default::default() }, "waiting", "native input acceptance pending"),
            ("ask", Signals { local: true, child: Some(Child::default()), ask: true, ..Default::default() }, "waiting", "owner answer required"),
            ("held", Signals { local: true, held: true, ..Default::default() }, "waiting", "launch held by admission gate"),
            ("#1386/#1551 boot after intent", Signals { local: true, boot: Some((200, 100)), ..Default::default() }, "ended", "host booted 1970-01-01T00:03:20Z after intent 1970-01-01T00:01:40Z"),
            ("#1386 boot before intent", Signals { local: true, boot: Some((100, 200)), ..Default::default() }, "unknown", "native observation unavailable; restore owning runtime"),
            ("#1481 no heartbeat", Signals::default(), "unknown", STALE),
            ("#1561 recorded process exited", Signals { local: true, process: Some((&rec, Ok(None))), ..Default::default() }, "ended", "recorded native process exited"),
            ("#1561 recorded pid reused", Signals { local: true, process: Some((&rec, Ok(Some(reused.clone())))), ..Default::default() }, "ended", "recorded native process exited"),
            ("#1561 recorded process live", Signals { local: true, process: Some((&rec, Ok(Some(rec.clone())))), ..Default::default() }, "running", "recorded native process observed"),
            ("#1561 identity read failed", Signals { local: true, process: Some((&rec, Err("io".into()))), ..Default::default() }, "unknown", "recorded native process identity unknown: io"),
        ];
        for (name, s, state, reason) in cases {
            let (got, why) = verdict(&s);
            assert_eq!((got, why.as_str()), (state, reason), "{name}");
            if got == "ended" { continue; }
            // the owner publishes `execution_statuses` rows; the reader and the tray read them back through `execution_status`
            let row = ExecutionStatus { task: 1, attempt: 5, id: 7, through: 7, state: got.into(), reason: why.clone() };
            let issue = board::Issue { body: format!("execution {}", serde_json::to_string(&row).unwrap()), ..host(10) };
            let v = execution_status(1, &exec(), &[issue.clone()], 1000, 300);
            assert_eq!((v["state"].as_str(), v["reason"].as_str()), (Some(got), Some(why.as_str())), "{name} via heartbeat");
            // #1491: the same row past the ttl reads unknown
            let v = execution_status(1, &exec(), &[issue], 1301, 300);
            assert_eq!((v["state"].as_str(), v["reason"].as_str()), (Some("unknown"), Some(STALE)), "{name} stale");
        }
    }
    // CS2/R12: poll, labels, edited and untrusted comments cannot manufacture conversation activity.
    #[test]
    fn sidebar_activity_projects_trusted_comment_time_not_observation() {
        let log=vec![];let mut v=View {issue:board::Issue{number:1,created:100,updated:999,..Default::default()},projection:crate::core::project(1,&log,false),log,comments:vec![
            board::Comment{id:1,created:150,trusted:true,edited:false,..Default::default()},
            board::Comment{id:2,created:500,trusted:false,edited:false,..Default::default()},
            board::Comment{id:3,created:600,trusted:true,edited:true,..Default::default()}],blockers:vec![]};
        let read=|v:View|from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[]).json()["tasks"][0]["updated"].clone();
        assert_eq!(read(View{issue:v.issue.clone(),projection:crate::core::project(1,&[],false),log:vec![],comments:v.comments.clone(),blockers:vec![]}),150);
        v.comments.clear();assert_eq!(read(v),100);
    }

}
