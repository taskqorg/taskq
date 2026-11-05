//! The host service (taskq.md § 3, § 4): one loop around `fold`. It owns this host's children and its heartbeat issue;
//! everything it knows about other hosts comes from the board.

use crate::board::{Board, Comment, Issue};
use crate::core::{self, State, Task};
use crate::runtime::{Launch, Proc};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use crate::admission::{Entry, Res, Want};
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub install: PathBuf,
    pub board: &'static crate::board::Kind,
    pub web_host: String,
    pub repo: String,
    /// Publication mode `direct|pr`, from the delivery preset (§ 7).
    pub publish: String,
    /// § 7 delivery preset and its policy revision.
    pub delivery: String,
    pub delivery_revision: u64,
    pub me: String,
    pub limits: BTreeMap<String, u32>,
    /// What this host offers for `needs-<x>` labels.
    pub has: Vec<String>,
    /// Host task-claim mode (§4/§9); manager/owner turns are independent.
    pub claim: crate::host::ClaimMode,
    pub spillover_grace: u64,
    pub weights: crate::balance::Weights,
    pub allowed_runtimes: Option<Vec<String>>,
    pub auto_reroute: bool,
    /// host.json `default_runtime` (§ 9): the runtime of reviewers and of workers without `run-*` (§ 4).
    pub default_runtime: String,
    pub routing: Vec<crate::labels::Routing>,
    pub permission_mode: String,
    pub codex: Vec<String>,
    pub ttl: i64,
    pub poll: u64,
    pub update_every: u64,
    pub rollback_after: u64,
    /// `workspace: external` (§ 9): the hourly tidy skips unowned worktrees and touches only TaskQ's own task worktrees.
    pub external: bool,
    /// `host_setup` (§ 6 Shared project bootstrap): the project's one-time host setup command, run by `service install`.
    pub host_setup: Option<String>,
}

impl Config {
    /// §4: only an explicit operator disable permits the project auto_reroute exception.
    pub fn runtime_disabled(&self, rt: &str, limits: Option<&BTreeMap<String, u32>>) -> bool {
        self.allowed_runtimes.as_ref().is_some_and(|a| !a.iter().any(|r| r == rt))
            || limits.is_some_and(|l| l.get(rt) == Some(&0))
            || { let a = crate::runtime::accounts::Host::read(&self.install); a.error.is_none() && a.off(rt, crate::runtime::accounts::now()) }
    }
}

pub const HOST_LABEL: &str = "taskq-host";
/// The capacity reason of an empty focus (§ 4).
pub const FOCUS_OFF: &str = "focus off: nothing admitted; set `taskq service focus all|<label..>`";
pub const TASK_LABEL: &str = "taskq";
/// #1935: the first words of the one board comment on a PR that waits for a person to merge it.
const MANUAL_MERGE_NOTE: &str = "Waiting for a manual merge";
const NATIVE_PASS_BUDGET:std::time::Duration=std::time::Duration::from_secs(20);

/// The code the host's tray wrote on an interface fault (#1755) until a new tray clears it: a fixed code, else none.
fn tray_fault(install: &std::path::Path) -> Option<String> {
    let code = std::fs::read_to_string(install.join("tray.fault")).ok()?;
    let code = code.trim();
    (!code.is_empty() && code.len() <= 40 && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')).then(|| code.to_string())
}

/// docs/design/worktrees.md Findings: one held finding per root and tidy condition that has lasted over a day, nothing
/// more while that task is open. `.taskq/findings.json` keeps `{condition: {since, issue}}` and the last reason a
/// finding could not be filed, so the reason logs once (a cache, R1).
fn tidy_findings(b: &dyn Board, cfg: &Config, conditions: &BTreeMap<&'static str, Vec<String>>, now: u64, log: &mut Vec<String>) {
    const DAY: u64 = 86_400;
    let path = crate::paths::dot(&cfg.root).join("findings.json");
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object);
    let last = read(&path).unwrap_or_else(|| json!({}));
    let mut next = json!({});
    if let Some(reason) = last.get("unfiled") { next["unfiled"] = reason.clone(); }
    // a source part: 1-48 of `[A-Za-z0-9._-]`, never only dots
    let part = |s: &str| -> String {
        let p: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).take(48).collect();
        if p.bytes().all(|c| c == b'.') { "x".into() } else { p }
    };
    let (host, project) = (part(&cfg.me), part(&crate::paths::project_name(&cfg.root)));
    for (cond, lines) in conditions {
        let mut entry = last.get(*cond).cloned().filter(|e| e["since"].is_u64()).unwrap_or_else(|| json!({"since": now}));
        if let Some(n) = entry["issue"].as_u64() {
            // open or unreadable: nothing more; closed while the condition holds: a new day starts
            if b.issue(n).is_ok_and(|i| !i.open) { entry = json!({"since": now}); }
            next[*cond] = entry;
            continue;
        }
        let since = entry["since"].as_u64().unwrap_or(now);
        if now.saturating_sub(since) > DAY {
            let (what, goal) = match *cond {
                "dirty" => ("Task worktree dirty", format!("A task worktree on host {} stays dirty while no child of that host runs its task: keep or drop its changes so the tidy pass can remove it.", cfg.me)),
                "stale" => ("Root stale", format!("The project root on host {} has not followed its origin default branch for over a day: settle the stale reason so the tidy pass can fast-forward it.", cfg.me)),
                _ => ("Closed task worktree kept", format!("A closed task's worktree on host {} stays after each tidy pass: settle its kept reason so the worktree goes.", cfg.me)),
            };
            let new = crate::cli::task_cmd::NewTask {
                conversation: None,
                title: format!("{what} over a day: {project} on {host}").chars().take(80).collect(),
                goal,
                acceptance: vec![format!("taskq cleanup --dry-run on host {} no longer prints the evidence lines of this task.", cfg.me)],
                scope: vec![], deps: vec![], kind: None, runtime: None, priority: None, host: None, labels: vec![], rules: None,
            };
            let evidence: Vec<String> = lines.iter().take(4).map(|l| l.replace(['\r', '\n'], " ").chars().take(238).collect()).collect();
            let source = format!("{host}/tidy-{cond}/{project}-{since}");
            let raw = read(&cfg.root.join("taskq.json")).unwrap_or(Value::Null);
            let mut op = crate::dispatch::Op::default();
            let reason = match crate::cli::task_cmd::submit_finding(b, &raw, &cfg.root, cfg.board.name, &cfg.web_host, &cfg.repo, &new, &source, &evidence, &mut op, false) {
                Ok((r, _)) if r.ok() => {
                    entry["issue"] = json!(r.task);
                    log.push(format!("#{} finding filed: tidy {cond} over a day", r.task));
                    None
                }
                Ok((r, _)) => Some(r.reason),
                Err(e) => Some(e),
            };
            match reason {
                Some(r) => {
                    let line = format!("tidy finding not filed: {r}");
                    if last["unfiled"].as_str() != Some(&line) { log.push(line.clone()); }
                    next["unfiled"] = json!(line);
                }
                None => { next.as_object_mut().unwrap().remove("unfiled"); }
            }
        }
        next[*cond] = entry;
    }
    if next != last && let Err(e) = std::fs::create_dir_all(crate::paths::dot(&cfg.root)).and_then(|_| std::fs::write(&path, next.to_string())) {
        log.push(format!("findings cache: {e}"));
    }
}

/// Only issues labelled `taskq` are tasks (§ 1): a user issue never starts by itself.
/// Heartbeat lines a write compares: the `at` stamp and diagnostics observation times change every pass (#1358).
/// A value that is the same for the whole host travels with the next write, or one change writes every project's
/// board (§ 4 step 3, #2352).
fn stable(body: &str) -> Vec<String> {
    // A changed usage row never forces a write; rows travel with the periodic write (capacity.md).
    body.lines().filter(|l| !l.starts_with("at ") && crate::runtime::usage::Row::parse(l).is_none()
        && !l.starts_with("headroom ") && !l.starts_with("requests ") && *l != "has gui").map(|l| {
        if let Some(mut balancing)=l.strip_prefix("balancing ").and_then(|s|serde_json::from_str::<Value>(s).ok()) {
            if let Some(b)=balancing.as_object_mut() {b.remove("has");}
            if let Some(runtimes)=balancing.get_mut("runtimes").and_then(Value::as_object_mut) {
                for r in runtimes.values_mut().filter_map(Value::as_object_mut) {
                    for key in ["code","reason","load"] {r.remove(key);}
                }
            }
            return format!("balancing {balancing}");
        }
        if let Some(queue)=l.strip_prefix("queue ").and_then(|s|serde_json::from_str::<Value>(s).ok()) {
            return format!("queue {}",queue["restart"]);
        }
        if let Some((_,status))=l.strip_prefix("doctor ").and_then(|s|s.split_once(' ')) {
            return format!("doctor {status}");
        }
        if let Some(mut binary)=l.strip_prefix("binary-evidence ").and_then(|s|serde_json::from_str::<Value>(s).ok()) {
            if let Some(o)=binary.as_object_mut() {o.remove("observed");o.remove("native_observed");}
            return format!("binary-evidence {binary}");
        }
        if let Some(mut execution)=l.strip_prefix("execution ").and_then(|s|serde_json::from_str::<Value>(s).ok()) {
            if let Some(progress)=execution.get_mut("progress").and_then(Value::as_object_mut) {progress.remove("observed");}
            return format!("execution {execution}");
        }
        match l.strip_prefix("diagnostics ").and_then(|t| serde_json::from_str::<Value>(t).ok()) {
        Some(Value::Object(mut d)) => {
            d.remove("observed");
            d.values_mut().filter_map(Value::as_object_mut).for_each(|f| { f.remove("at"); });
            format!("diagnostics {}", Value::Object(d))
        }
        _ => l.to_string(),
    }}).collect()
}

/// § 4 step 3: a lease needs the wider margin only while this heartbeat holds an attempt (#2352).
fn renewal(body: &str, ttl: i64) -> i64 {
    if body.lines().any(|l| l.starts_with('#')) {ttl / 3} else {ttl / 2}
}

/// #1616: a host admission refusal (`admission.rs` code, or `host.rs` reason under code `host`) as an admission code.
fn refusal_admission(code: &str, text: &str) -> (&'static str, String) {
    let has = |w: &str| text.contains(w);
    let code = match code {
        "max" => "capacity",
        "spacing" => "load-gate",
        _ if has("recovery pending") || has("poisoned") || has("admission file") => return ("unknown", crate::snapshot::ADMISSION_UNKNOWN[2].into()),
        _ if has("load gate") => "load-gate",
        _ if has("capacity") || has("round-robin") || has("disabled") => "capacity",
        // `version`, an operator hold, draining and a pending restart hold every launch
        _ => "hold",
    };
    (code, text.into())
}

/// R12/§4: task-facing effective availability, published on the existing per-board heartbeat.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Balancing {
    schema: u8,
    #[serde(default,skip_serializing_if="BTreeMap::is_empty")]
    weights: crate::balance::Weights,
    #[serde(default,skip_serializing_if="Option::is_none")]
    last_spawn: Option<u64>,
    mode: String,
    default_runtime: String,
    allowed_runtimes: Option<Vec<String>>,
    auto_reroute: bool,
    has: Vec<String>,
    focus: Option<Vec<String>>,
    routing: Vec<crate::labels::Routing>,
    runtimes: BTreeMap<String, RuntimeAvailability>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeAvailability {
    code: String, reason: String, full: bool, disabled: bool, quota: Vec<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    load: Option<crate::balance::Load>,
}

impl Balancing {
    fn read(body: &str) -> Option<Self> {
        let rows:Vec<_>=body.lines().filter_map(|l|l.strip_prefix("balancing ")).collect();
        if rows.len()!=1 {return None;}
        let b:Self=serde_json::from_str(rows[0]).ok()?;
        if ![1,2].contains(&b.schema) || (b.schema==1)!=b.weights.is_empty()
            || crate::balance::weights(&json!({"weights":b.weights})).is_err()
            || b.runtimes.values().any(|r|r.load.as_ref().is_some_and(|l|!l.valid()) || b.schema==2 && r.code=="admitted" && r.load.is_none())
            || crate::host::ClaimMode::parse(&b.mode).is_err()
            || !crate::runtime::CLI_RUNTIMES.contains(&b.default_runtime.as_str())
            || b.allowed_runtimes.as_ref().is_some_and(|a|a.iter().any(|rt|!RUNTIMES.contains(&rt.as_str())) || a.iter().collect::<BTreeSet<_>>().len()!=a.len())
            || crate::labels::routing(&json!({"routing":b.routing})).is_err()
            || b.runtimes.len()!=RUNTIMES.len() || RUNTIMES.iter().any(|rt|!b.runtimes.contains_key(*rt))
            || b.runtimes.values().any(|r|!crate::snapshot::ADMISSION_CODES.iter().any(|(c,_)|*c==r.code) || r.reason.chars().count()>160 || r.code=="admitted" && r.disabled) {return None;}
        Some(b)
    }

    fn available(&self,v:&View) -> Result<(String,Option<String>),(&'static str,String)> {self.available_for(v,false)}

    fn available_for(&self,v:&View,local:bool) -> Result<(String,Option<String>),(&'static str,String)> {
        let focus=self.focus.as_ref().ok_or(("unknown","primary focus unknown".into()))?;
        if priority(&v.issue)!=0 && !focus.iter().any(|f|f=="all" || crate::labels::matches(&v.issue.labels,f)) || focus.is_empty() {
            return Err(("focus","primary focus excludes task".into()));
        }
        if let Some(need)=v.issue.labels.iter().filter_map(|l|l.strip_prefix("needs-")).find(|n|*n!="gate" && !self.has.iter().any(|h|h==n)) {
            return Err(("capability",format!("primary lacks needs-{need}")));
        }
        // Assignment is private to the primary's board account, so the projection cannot prove eligibility.
        if !local && v.issue.labels.iter().any(|l|l=="assignee-only") {return Err(("unknown","primary assignee eligibility unknown".into()));}
        let reviewer=v.projection.task.state==State::Review;
        let default=crate::labels::project_default(&v.issue.labels,reviewer,&self.routing,&self.default_runtime);
        // #2264: a refused runtime reports its occupancy too; only admitted ones rank, as before
        let load=self.runtimes.iter().filter(|(_,r)|r.code=="admitted").filter_map(|(rt,r)|r.load.clone().map(|l|(rt.clone(),l))).collect();
        crate::balance::select(&v.issue.labels,reviewer,default,self.auto_reroute,&self.weights,&load,
            |rt| self.runtimes.get(rt).is_some_and(|r|r.disabled),
            |rt| {
                if self.allowed_runtimes.as_ref().is_some_and(|a|!a.iter().any(|r|r==rt)) {return Err(("capability",format!("{rt} excluded by primary project")));}
                let r=self.runtimes.get(rt).ok_or(("unknown","primary runtime availability missing".into()))?;
                if r.code=="unknown" {return Err(("unknown",r.reason.clone()));}
                if r.code!="admitted" {return Err(("capacity",format!("{}{}",r.reason,if r.quota.is_empty() {String::new()}else{format!("; {}",r.quota.join("; "))})));}
                if needs_gui(&v.issue) && !r.full {return Err(("capability",format!("{rt} has no primary Full grant")));}
                Ok(rt.into())
            })
    }

    fn weighted_capacity(&self,v:&View,local:bool)->Result<f64,String> {
        self.available_for(v,local).map_err(|(_,reason)|reason)?;
        let reviewer=v.projection.task.state==State::Review;
        let default=crate::labels::project_default(&v.issue.labels,reviewer,&self.routing,&self.default_runtime);
        let ignore_pin=v.issue.labels.iter().find_map(|l|l.strip_prefix("run-"))
            .is_some_and(|rt|self.auto_reroute && crate::runtime::CLI_RUNTIMES.contains(&rt) && self.runtimes.get(rt).is_some_and(|r|r.disabled));
        let labels:Vec<_>=v.issue.labels.iter().filter(|l|!ignore_pin || !l.starts_with("run-")).cloned().collect();
        let mut weighted=0.0;
        let mut fallback=0.0;
        for rt in crate::labels::candidates(&labels,reviewer,default) {
            let r=&self.runtimes[&rt];
            if r.code!="admitted" || self.allowed_runtimes.as_ref().is_some_and(|a|!a.contains(&rt)) || needs_gui(&v.issue) && !r.full {continue;}
            let load=r.load.as_ref().ok_or("weighted host occupancy unknown")?;
            // Weight 0 (#2240): no unpinned capacity; a run-* pin counts its free slots like an unweighted runtime.
            match self.weights.get(&rt) {
                Some(0) if !labels.iter().any(|l|l.starts_with("run-"))=>{},
                Some(w) if *w>0=>weighted+=load.free as f64*load.effective(*w),
                _=>fallback+=load.free as f64,
            }
        }
        Ok(if weighted>0.0 {weighted}else{fallback})
    }
}

pub(crate) fn balancing_status(body:&str)->Value {
    let mut v=Balancing::read(body).map_or(Value::Null,|b|serde_json::to_value(b).unwrap_or(Value::Null));
    headroom_causes(&mut v,body);
    v
}

/// #2263: `headroom <rt> <cause>` heartbeat lines name why a weighted runtime has no headroom fraction.
/// A separate line, not a `Load` field: N-1 readers deny unknown balancing fields but ignore unknown lines.
pub(crate) fn headroom_causes(v:&mut Value,body:&str) {
    for (rt,cause) in body.lines().filter_map(|l|l.strip_prefix("headroom ")?.split_once(' ')) {
        if let Some(load)=v["runtimes"].get_mut(rt).and_then(|r|r.get_mut("load")).filter(|l|l["headroom"].is_null()).and_then(|l|l.as_object_mut()) {
            load.insert("headroom_reason".into(),json!(cause.chars().take(160).collect::<String>()));
        }
    }
}

pub fn is_task(i: &Issue) -> bool {
    i.trusted && i.labels.iter().any(|l| l == TASK_LABEL)
}

/// The held label projection (§ 1), including a start whose eligibility-label write failed.
pub fn is_held(i: &Issue) -> bool {
    i.trusted && !is_task(i) && i.labels.iter().any(|l| l == "q-held")
}

/// Only an accepted start authorizes repairing eligibility (§ 1).
pub fn start_logged(n: u64, log: &[core::Entry], p: &core::Projection) -> bool {
    log.iter().any(|e| p.accepts(e.id) && core::kind(n, &e.body) == "start")
}

pub fn is_host(i: &Issue) -> bool {
    i.labels.iter().any(|l| l == HOST_LABEL) && i.title.starts_with("taskq host ")
}

/// The host name of a heartbeat issue (§ 4 step 3); `None` for other issues.
pub fn host_name(i: &Issue) -> Option<&str> {
    is_host(i).then(|| i.title.trim_start_matches("taskq host "))
}

/// `who` is an assignee; an issue listed without assignees (github.com, § 4 budget) costs one `issue(n)` read.
pub fn assigned(board: &dyn Board, i: &Issue, who: &str) -> Result<bool, String> {
    Ok(i.assignees.iter().any(|a| a == who) || i.assignees.is_empty() && board.issue(i.number)?.assignees.iter().any(|a| a == who))
}

fn needs_gui(i: &Issue) -> bool {
    i.labels.iter().any(|l| l == "needs-gui")
}

fn label(i: &Issue, prefix: &str) -> Option<String> {
    i.labels.iter().find_map(|l| l.strip_prefix(prefix).map(String::from))
}

fn route(i: &Issue, reviewer: bool, default: &str) -> String {
    crate::labels::route(&i.labels, reviewer, default)
}

pub fn priority(i: &Issue) -> u8 {
    crate::display::priority(&i.labels)
}

/// The host named in a `spawn` session (`<rt>:<session> <host>`).
pub fn host_of(t: &Task) -> Option<&str> {
    t.session.as_deref()?.rsplit(' ').next()
}

/// `rt` and the session id of a `spawn` session.
pub(crate) fn rt_of(t: &Task) -> Option<(&str, &str)> {
    t.session.as_deref()?.split(' ').next()?.split_once(':')
}

/// Set the `q-<state>` label from the fold; a closed task loses its `q-` labels and its issue is closed (§ 2).
pub fn project(board: &dyn Board, issue: &Issue, t: &Task) -> Result<(), String> {
    let want = board.state_label(t.state.name())?;
    let stale: Vec<String> = issue.labels.iter().filter(|l| l.starts_with("q-") && **l != want).cloned().collect();
    if t.state == State::Closed {
        board.labels(issue.number, &[], &stale)?;
        board.close(issue.number)
    } else if !stale.is_empty() || !issue.labels.contains(&want) {
        board.labels(issue.number, &[want], &stale)
    } else {
        Ok(())
    }
}

pub struct View {
    pub issue: Issue,
    pub projection: core::Projection,
    pub comments: Vec<Comment>,
    pub log: Vec<core::Entry>,
    /// `Depends on` issues that keep a ready task waiting (§ 1).
    pub blockers: Vec<Blocker>,
}

/// A `Depends on` issue that is not done (§ 1): `reason` is `open` or `unknown: <why>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Blocker {
    pub dep: core::Dep,
    pub url: String,
    pub reason: String,
}

/// Closed dependencies already read, reason `None` when done (§ 4 budget), with the board close time of that same read
/// (#2321 dev request): an issue of this board until the open-issue list shows it again; of another repository only
/// when done. `ponytail:` a done foreign issue reopened later is not seen.
pub type DepCache = HashMap<core::Dep, (Option<String>, Option<i64>)>;

/// The `Depends on` issues of `body` that are not done (§ 1): open, closed other than completed, or unreadable.
/// `open` is this board's open-issue list when the caller read it.
pub fn blockers(b: &dyn Board, body: &str, open: Option<&HashSet<u64>>, done: &mut DepCache) -> Vec<Blocker> {
    core::depends(body).into_iter().filter_map(|dep| {
        let listed = dep.repo.is_none() && open.is_some_and(|o| o.contains(&dep.n));
        if listed {
            done.remove(&dep);
        }
        let reason = match done.get(&dep) {
            _ if listed => Some("open".to_string()),
            Some((r, _)) => r.clone(),
            None => match dep.repo.as_deref().map_or_else(|| b.issue(dep.n), |r| b.foreign(r, dep.n)) {
                Err(e) => Some(format!("unknown: {e}")),
                Ok(i) if i.open => Some("open".into()),
                Ok(i) => {
                    let mut r = i.reason.filter(|r| r != "completed").map(|r| format!("unknown: closed {}", r.replace('_', " ")));
                    // #1950: a close during a worker attempt is not done; its attempt host reopens it (§ 4 step 5)
                    if r.is_none() && dep.repo.is_none() {
                        match crate::board::entries(b, dep.n) {
                            Err(e) => return Some(Blocker { url: b.url(dep.n), reason: format!("unknown: {e}"), dep }),
                            Ok(log) => r = (core::fold(dep.n, &log, false).state == State::Doing).then(|| "unknown: closed during a worker attempt".into()),
                        }
                    }
                    if dep.repo.is_none() || r.is_none() {
                        done.insert(dep.clone(), (r.clone(), i.closed));
                    }
                    r
                }
            },
        };
        let url = dep.repo.as_deref().map_or_else(|| b.url(dep.n), |r| b.foreign_url(r, dep.n));
        reason.map(|reason| Blocker { dep, url, reason })
    }).collect()
}

/// #2321 dev request: the newest board close time of a done `Depends on` issue of this board among ready tasks, from
/// the closes `views` already read (no extra board request); unknown close times count as none.
pub fn newest_dep_close(views: &[View], deps: &DepCache) -> Option<i64> {
    views.iter().filter(|v| v.projection.task.state == State::Ready)
        .flat_map(|v| core::depends(&core::dep_body(&v.issue.body, &v.projection.task)))
        .filter(|d| d.repo.is_none())
        .filter_map(|d| deps.get(&d).and_then(|(r, at)| r.is_none().then_some(*at).flatten()))
        .max()
}

/// § 4 step 5 ask card of a task closed during a worker attempt (#1950); its answer resumes the attempt.
pub(crate) fn closed_card(n: u64, at: &str) -> String {
    crate::cli::text::service_card(n, &format!("reopened; the issue was closed at {at} during this worker attempt, with no result or review."),
        "only the reviewer's close or a person ends a task (§ 1); a close during an attempt never counts as done.", &[
        "redo: finish the work - + the attempt hands in a result for review; - the attempt runs on".into(),
        format!("later: park it - + nothing runs; `taskq close {n}` then ends it unmerged; - the work stays undone"),
    ], 1)
}

/// § 2 ask card of a start the rules cap refused (#1946); an answer starts a fresh attempt.
pub(crate) fn refused_card(n: u64, why: &str) -> String {
    crate::cli::text::service_card(n, "the service refused to start this attempt; nothing ran.", &format!("start refused: {}", why.replace("\nNext: ", "; ")), &[
        "retry after trimming - + a fresh attempt starts; - it is refused again until the rules fit the cap".into(),
        core::LATER.into(),
    ], 1)
}

/// § 2 ask card of resumed input that failed to launch `failures` times; the input stays for a retry.
pub(crate) fn input_card(n: u64, text: &str, failures: usize) -> String {
    crate::cli::text::service_card(n, &format!("the input is retained; {text}"), &format!("input launch failed {failures} times; input retained."), &[
        "retry the input - + resends the undelivered input in this session; - it may fail the same way".into(),
        core::LATER.into(),
    ], 1)
}

/// `git -C dir <args>`: stdout, else the command and its error (#1856: a failed call never reads as empty).
fn git(dir: &std::path::Path, args: &[&str]) -> Result<Vec<u8>, String> {
    crate::tools::run("seed.git", crate::platform::command("git").arg("-C").arg(dir).args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"]).args(args), std::time::Duration::from_secs(60))
}

/// § 7 rule 2 (#2078): before a worker's first start the service, outside the worker sandbox (which may not write the
/// shared `.git`), creates `.worktrees/taskq-<N>` on branch `taskq-<N>` from `origin/main`, or on the existing branch.
/// An existing worktree is continued untouched. #1620 (docs/testing.md#build-cache): a new one gets a copy-on-write
/// clone of the newest `target/debug` of the root or another worktree: a recent build shares the current Cargo.lock,
/// so only the workspace crate recompiles. The clone only reads its source, and each worktree owns its copy. Any
/// failure only logs; the worker brief names the host gap. `ponytail:` a source mid-build clones as is; cargo's
/// fingerprints rebuild whatever does not match.
pub fn seed(root: &std::path::Path, n: u64) -> Option<String> {
    let name = format!("taskq-{n}");
    let wt = root.join(".worktrees").join(&name);
    if wt.exists() { return None; }
    // a stale registration of a deleted folder would refuse the add
    let _ = git(root, &["worktree", "prune"]);
    // offline: the last fetched origin/main still gives the worker its branch; the worker rebases before its push
    let fetched = git(root, &["fetch", "-q", "origin", "main"]).err().map_or(String::new(), |e| format!(" (fetch failed: {e})"));
    let rel = format!(".worktrees/{name}");
    let added = if git(root, &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")]).is_ok() {
        git(root, &["worktree", "add", "-q", &rel, &name])
    } else {
        git(root, &["worktree", "add", "-q", "-b", &name, &rel, "origin/main"])
    };
    if let Err(e) = added { return Some(format!("#{n} worktree not created: {e}{fetched}")); }
    let others = std::fs::read_dir(root.join(".worktrees")).into_iter().flatten().flatten().filter(|e| e.path() != wt).map(|e| e.path().join("target/debug"));
    let Some(src) = std::iter::once(root.join("target/debug")).chain(others)
        .filter_map(|p| Some((std::fs::metadata(&p).ok().filter(|m| m.is_dir())?.modified().ok()?, p))).max().map(|m| m.1)
    else { return Some(format!("#{n} worktree created{fetched}")); };
    let dst = wt.join("target/debug");
    if let Err(e) = std::fs::create_dir_all(wt.join("target")) { return Some(format!("#{n} seed skipped: {e}")); }
    let cloned = clone_dir(&src, &dst);
    if cloned.is_err() { let _ = std::fs::remove_dir_all(&dst); }
    Some(match cloned { Ok(()) => format!("#{n} seeded target/debug{fetched}"), Err(e) => format!("#{n} seed skipped: {e}{fetched}") })
}

#[cfg(target_os = "macos")]
fn clone_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &std::path::Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string());
    let (s, d) = (c(src)?, c(dst)?);
    // One call clones the whole tree on APFS; other volumes fail and skip.
    if unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), 0) } == 0 { Ok(()) } else { Err(format!("clonefile: {}", std::io::Error::last_os_error())) }
}

#[cfg(target_os = "linux")]
fn clone_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    // #2051: `auto` clones on btrfs and XFS and falls back to a plain copy where the volume has no reflink (ext4).
    // ponytail: the plain copy runs inside the 300 s budget; a slower volume times out and skips.
    crate::tools::run("seed.cp", crate::platform::command("cp").args(["-a", "--reflink=auto"]).arg(src).arg(dst), std::time::Duration::from_secs(300))
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn clone_dir(_: &std::path::Path, _: &std::path::Path) -> Result<(), String> {
    // ponytail: Windows ReFS block cloning is the upgrade path; a full copy costs more than sccache-backed builds.
    Err("no copy-on-write clone on this OS".into())
}

/// Seconds after an issue's `updated` before its comment list counts as settled: a list read sooner may lag the write
/// that set `updated`, so the next pass reads it again instead of reusing it for the unchanged `updated` (#1718).
const SETTLE: i64 = 30;

/// Comments per task: the `updated` they were read for, the board time of that read, the list.
pub type CommentCache = HashMap<u64, (i64, i64, Vec<Comment>)>;

/// Fold every open task (heartbeat issues and untrusted authors excluded).
pub fn views(board: &dyn Board, cache: &mut CommentCache, deps: &mut DepCache, ttl: i64) -> Result<(Vec<View>, Vec<Issue>), String> {
    views_asked(board, cache, deps, ttl).map(|(views, hosts, _)| (views, hosts))
}

/// [`views`] and the open `taskq doctor --hosts` request id ([`crate::health::request_of`], #2194).
pub fn views_asked(board: &dyn Board, cache: &mut CommentCache, deps: &mut DepCache, ttl: i64) -> Result<(Vec<View>, Vec<Issue>, Option<String>), String> {
    let mut issues = board.issues(None)?;
    // #2083 § 3: a kept heartbeat is never newer than the board; one fresh when listed and past `ttl` now may have been
    // written since, so it relists before a lease reads it stale. One already stale when listed relists no more.
    if let Some(listed) = board.kept() {
        let now = board.now()?;
        if issues.iter().any(|h| is_host(h) && now - h.updated > ttl && listed - h.updated <= ttl) {
            board.forget();
            issues = board.issues(None)?;
        }
    }
    // #2291: a page boundary that shifts mid-list repeats an issue; one view per number keeps Running rows unique
    let mut seen = HashSet::new();
    issues.retain(|i| seen.insert(i.number));
    let open: HashSet<u64> = issues.iter().map(|i| i.number).collect();
    let request = crate::health::request_of(&issues);
    let (hosts, tasks): (Vec<Issue>, Vec<Issue>) = issues.into_iter().filter(|i| i.trusted).partition(is_host);
    let mut out = vec![];
    for issue in tasks.into_iter().filter(|i| is_task(i) || is_held(i) || i.labels.iter().any(|l| matches!(l.as_str(), "q-ready" | "q-blocked"))) {
        let (read, comments) = match cache.get(&issue.number) {
            Some((u, at, c)) if *u == issue.updated && *at >= issue.updated + SETTLE => (*at, c.clone()),
            _ => { let c = board.comments(issue.number)?; (board.now().unwrap_or(issue.updated), c) },
        };
        cache.insert(issue.number, (issue.updated, read, comments.clone()));
        let log = crate::board::reply::log(issue.number, &comments)?;
        // only a task that would be ready reads its dependencies (§ 4 budget)
        let mut projection = core::project(issue.number, &log, false);
        let started = start_logged(issue.number, &log, &projection);
        if !is_task(&issue) && !is_held(&issue) && !started { continue; }
        let held = is_held(&issue) && !started;
        let ready = !held && projection.task.state == State::Ready;
        let blockers = if ready { blockers(board, &core::dep_body(&issue.body, &projection.task), Some(&open), deps) } else { vec![] };
        if !blockers.is_empty() { projection.task.state = State::Waiting; }
        if held {
            projection.task.state = State::Held;
        }
        out.push(View { projection, issue, comments, log, blockers });
    }
    Ok((out, hosts, request))
}

/// Release only a known submission, with idempotent write and authoritative readback.
/// #1886: the host step that can release an unresolved execution on `host`, seen from host `me`.
pub(crate) fn release_step(host: &str, me: &str) -> String {
    if host == me { format!("reboot {me} to prove exit") } else { format!("start the taskq service on {host} once, after a reboot if the run had started") }
}

fn release_execution(board: &dyn Board, task: u64, attempt: u64, execution: u64, proof: &str) -> Result<(), String> {
    release_execution_text(board, task, attempt, execution, proof, "")
}

pub(crate) fn release_execution_text(board: &dyn Board, task: u64, attempt: u64, execution: u64, proof: &str, text: &str) -> Result<(), String> {
    let expected = match proof { "exit" => core::Release::Exit, "no-start" => core::Release::NoStart, _ => return Err("invalid release proof".into()) };
    let released = |log: &[core::Entry]| core::executions(task, log).into_iter()
        .find(|x| x.id == execution && x.attempt == attempt).map(|x| x.release);
    match released(&crate::board::entries(board, task)?) {
        Some(Some(found)) if found == expected => return Ok(()),
        Some(Some(_)) => return Err(format!("execution {execution} has contradictory release proof")),
        Some(None) => {},
        None => return Err(format!("execution {execution} ownership unknown; release refused")),
    }
    let op = format!("release-{execution}");
    let body = core::with_fields(core::entry("released", &[&execution.to_string(), proof], Some(attempt), text), Some(&op), None);
    let written = board.comment(task, &body);
    if released(&crate::board::entries(board, task)?) == Some(Some(expected)) { Ok(()) }
    else { Err(format!("execution {execution} release unconfirmed: {}", written.err().unwrap_or_else(|| "readback missing".into()))) }
}

/// `seen`: a log this pass already read after the execution's own write; only the acceptance readback reads again (#1359).
fn accept_execution(board: &dyn Board, task: u64, attempt: u64, execution: u64, seen: Option<&[core::Entry]>) -> Result<u64, String> {
    let accepted = |log: &[core::Entry]| core::executions(task, log).into_iter()
        .find(|x| x.id == execution && x.attempt == attempt && x.accepted).map(|x| x.through);
    let log = match seen { Some(l) => l.to_vec(), None => crate::board::entries(board, task)? };
    let known = core::executions(task,&log).into_iter().find(|x| x.id == execution && x.attempt == attempt)
        .ok_or_else(|| format!("execution {execution} ownership unknown; acceptance refused"))?;
    if known.release == Some(core::Release::NoStart) { return Err(format!("execution {execution} confirmed no-start; acceptance refused")); }
    if known.accepted { return Ok(known.through); }
    let op = format!("accept-{execution}");
    let body = core::with_fields(core::entry("accepted", &[&execution.to_string()], Some(attempt), ""), Some(&op), None);
    let written = board.comment(task, &body);
    accepted(&crate::board::entries(board, task)?).ok_or_else(|| format!("execution {execution} acceptance unconfirmed: {}",
        written.err().unwrap_or_else(|| "readback missing or contradictory".into())))
}

pub struct Kid {
    pub execution: u64,
    pub task: u64,
    pub attempt: u64,
    pub rt: String,
    pub proc: Box<dyn Proc>,
    /// Last native observation; absence of evidence never releases this exact execution.
    pub unknown: Option<String>,
    pub stopping: bool,
    /// #2261: the session link of this live execution, or its runtime's absence cause (`<cause>[: <detail>]`).
    pub session_url: Option<Result<String, String>>,
}

impl Kid {
    fn release(&self,board:&dyn Board,code:i32,evidence:&str)->Result<bool,String> {
        let refusal=self.proc.refused(self.task,self.attempt,self.execution,code);
        release_execution_text(board,self.task,self.attempt,self.execution,if refusal.is_some() {"no-start"} else {"exit"},refusal.as_deref().unwrap_or(evidence))?;
        Ok(refusal.is_some())
    }
}

pub struct Service<'a> {
    last_spawn: Option<u64>,
    /// #2255: this project's queue row and the restart pending since, in board time.
    queue: crate::host::Queue,
    restart_since: Option<i64>,
    headroom_error: Option<String>,
    progress: HashMap<(u64,u64,u64),(crate::runtime::progress::Reader,Value,bool)>,
    // Native UTC sampling is independent of the board reply/pass-start clock.
    progress_clock: fn(&dyn Board)->i64,
    // R6/R12: wall-clock bound per progress scan; a cut scan reads unknown and resumes at its cursor next pass.
    progress_budget: std::time::Duration,
    pub host: Option<crate::host::Shared>,
    pub board: &'a dyn Board,
    pub cfg: Config,
    pub version: String,
    started: u64,
    diagnostics: std::cell::RefCell<Option<(std::time::Instant,Value)>>,
    binary: std::cell::RefCell<Option<(std::time::Instant,Value)>>,
    /// #2194: when the health checks last ran and their heartbeat `doctor` line.
    health: Option<(std::time::Instant, String)>,
    /// #2194: the last `taskq doctor --hosts` request these health checks answered.
    answered: Option<String>,
    pub kids: Vec<Kid>,
    /// `service <id>` in the heartbeat (§ 4): a hash of `<install>/installation-id`; empty writes no line.
    pub instance: String,
    /// `link <installation-id> <public key>` in the heartbeat ([host link](../docs/design/host-link.md)); empty writes no line.
    pub link: String,
    /// `machine <hostname>[ wsl]` in the heartbeat (§ 4): the agentq `host.name` DashQ joins hosts by; empty writes no line.
    pub machine: String,
    /// Labels from `<root>/.taskq/focus`; empty: admit nothing, `all`: any task; `p0` passes any non-empty focus (§ 4).
    pub focus: Vec<String>,
    /// Why `focus` could not be read; it then admits nothing (§ 4 Data).
    pub focus_error: Option<String>,
    /// The last inbox id handed to each attempt's child.
    delivered: HashMap<u64, u64>,
    cache: CommentCache,
    deps: DepCache,
    /// #2321: [`newest_dep_close`] of the last full pass; the taskq-repo dev host requests a build from it.
    pub dep_closed: Option<i64>,
    /// Label fix streaks: wanted label, stale labels, passes, board time after the last write, reported.
    label_fixes: HashMap<u64, (String, Vec<String>, u32, i64, bool)>,
    cancelled_publications: HashMap<u64, u64>,
    /// § 2 Summary: tasks this service saw with a summary, and its consecutive rewrites of each (a peer that renders
    /// another text is rewritten once per 60 passes, like a reverted label).
    summarized: HashSet<u64>,
    summary_fixes: HashMap<u64, u32>,
    /// #1820 each publication's last recorded wait reason: one `checks` observation per reason, never one per pass.
    publication_waits: HashMap<u64, String>,
    /// The last invalid `taskq.json` delivery, logged once (#1942).
    delivery_error: Option<String>,
    /// Closed-task terminal rows `repair_ended` proved are not this host's (#1359).
    unowned: HashSet<(u64, u64)>,
    /// The last worktree tidy (docs/design/worktrees.md); `None` runs one on the next pass.
    tidied: Option<std::time::Instant>,
    start: Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>, crate::runtime::StartError> + 'a>,
    pub recover: Option<Box<dyn FnMut(u64, &core::Execution) -> Result<Box<dyn Proc>, String> + 'a>>,
    /// Host boot in Unix seconds (#1386); tests replace the OS read.
    pub boot: fn() -> Result<i64, String>,
    #[cfg(test)]
    agent_status: Value,
    pub log: Vec<String>,
    /// The read model of the last successful pass (for `status.json` and the tray).
    pub snapshot: Option<crate::snapshot::Snapshot>,
    pub tasks_read: Option<std::time::Instant>,
    /// A complete board ownership read is required before drain or handover.
    ownership_known: bool,
    /// This host's heartbeat issue, from the last board read: a lease write needs no second list (§ 4 budget).
    host_issue: Option<u64>,
    /// Rebuilt from board ownership; heartbeat projection only, never a launch/release authority.
    observations: BTreeMap<(u64,u64),core::Execution>,
    ownership_waits: BTreeMap<(u64,u64),String>,
    /// Owner comments of the last pass (§ 4): task, comment id, text, card.
    /// State of each open task in the previous pass, for `events`.
    seen: HashMap<u64, State>,
    /// R6 stuck tasks already logged, by the board time of their newest log entry: one line until the task moves.
    stuck: HashMap<u64, i64>,
    /// § 4 Churn tasks already logged: one line and one `service.churn` record until the flag clears.
    churned: HashSet<u64>,
    /// The newest comment id an events pass saw; `gone` events come only from newer comments (§ 4)
    gone_after: Option<u64>,
    /// Newest comment id of the previous pass, for queue transitions; None before the first (§ 4 Profiling).
    moved_after: Option<u64>,
    /// First time covered by `<root>/.taskq/events` (§ 4).
    pub events_since: Option<i64>,
    /// Capacity reloaded before each pass (§ 3); an error holds every launch.
    pub cap: Result<Capacity, String>,
    pub gate: Gate,
    /// Monotonic seconds for the gate.
    pub mono: u64,
    /// Won claims the gate holds before launch.
    held: Vec<Launch>,
    /// Attempts without a child that keep their lease: held claims and resumes (§ 3, § 4 heartbeat).
    pending: BTreeSet<(u64, u64)>,
    /// Spawns this process posted whose readback lacked them: no new claim of the task until one shows (#1903).
    /// ponytail: an entry clears at the next claim of its task, so closed tasks keep theirs until restart.
    unseen: HashMap<u64, u64>,
    /// Wall seconds for the host authority, set by the loop.
    pub wall: i64,
    /// Auto reservations the host granted, not yet a child; refused requests of this pass and their reason (§ 3).
    reserved: Vec<Res>,
    requested: Vec<Res>,
    refused: Option<(&'static str, String)>,
    /// Held reservations of the whole host at the last host write; unknown when it failed.
    host_used: Option<u32>,
    /// The host policy revision for this root that set this pass's capacity (#1282 applied readback).
    pub project_revision: Option<String>,
    /// The emergency hold read at the start of each pass (§ 3): refuses every launch.
    pub hold: Option<String>,
    pub stop_progress: Vec<Value>,
    /// Runtimes whose access policy refuses a launch, with the reason (§ 8 Access): no claim, resume or reviewer.
    pub refused_access: BTreeMap<String, String>,
    /// Runtimes held after a usage limit, with board time the hold ends and whether the runtime reported it
    /// ([usage limit](docs/design/capacity.md#usage-limit)): no claim, resume or reviewer until then.
    /// Rebuilt from host-wide account windows and execution release evidence after restart.
    pub limited: BTreeMap<String, (i64, bool)>,
    /// Runtimes whose last probe read `not logged in` (§ 5 `agents`, #1429): no claim, resume or reviewer until a
    /// probe reads them logged in again; unknown login admits as before.
    pub logged_out: BTreeSet<String>,
    /// § 4 heartbeat `cli <bin> <version>`: each CLI's version at the last runtime probe; `unknown` when unread (R12).
    pub clis: BTreeMap<String, String>,
    /// § 3 Runtime accounts: the active account of each listed runtime at the last pass (switch record).
    pub accounts: BTreeMap<String, Option<String>>,
    fleet: crate::runtime::usage::FleetCache,
    pub gui_checked: Option<std::time::Instant>,
    pub gui_probe: Result<(), String>,
    /// #1803: the last gui probe read a stale grant (`platform::stale_grant`); the tray notifies once.
    pub gui_stale: bool,
    /// #1845: `platform::grants` of the last gui probe; a `missing` or `stale` one holds gui work.
    pub grants: BTreeMap<String, String>,
    pub full: BTreeSet<String>,
    pub waits: BTreeMap<u64, String>,
    pub execution_waits: BTreeMap<u64, String>,
    pending_releases: BTreeSet<(u64, u64, u64)>,
    /// Spawn reason by execution of a child whose PID is not known yet: `record_known_pid` records it at the readback.
    unknown_pids: HashMap<u64, &'static str>,
    /// Children by attempt: role, runtime, and start with attempt log offset; `None` when adopted across a re-exec
    /// (§ 4 Profiling stages).
    begun: HashMap<u64, (String, String, Option<(std::time::Instant, u64)>)>,
    /// This host's tasks seen moving into `ask`: since when, attempt, runtime (§ 4 Profiling stages).
    asked: HashMap<u64, (std::time::Instant, u64, String)>,
    /// Tasks on the last open list, and those gone from it not yet read (§ 4 step 5, #1950).
    listed: HashSet<u64>,
    unlisted: BTreeSet<u64>,
    reduced_at: Option<u64>,
    recovery_after: Option<u64>,
}

const MONTH: i64 = 30 * 86400;
/// Seconds a usage-limited runtime stays held when it reported no reset.
const USAGE_PROBE: i64 = 1800;
/// #1843: continuing steers per attempt after a transient model failure (§ 4 step 1).
const CONTINUE_MAX: usize = 2;
/// #2198: continuing steers per attempt after a Claude turn ended with its own live background job (§ 4 step 1).
/// `ponytail:` a fixed bound keeps a worker that backgrounds again from looping; past it the exit is `gone`.
const BACKGROUND_MAX: usize = 3;
const BACKGROUND_STEER: &str = "Continue the task: the last turn ended while its own background jobs still ran";

/// Host readback stays separate from a project's effective narrowing limits.
pub fn host_capacity(capacity:&Value)->Option<Value> {
    if !capacity["host_policy"].is_object() {return None;}
    let mut host=capacity.clone();
    for field in ["limits","load_gate","admission"] {host[field]=capacity["host_policy"][field].clone();}
    host["used"]=capacity["host_runtime_used"].clone();
    Some(host)
}

/// Load gate settings (§ 9).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GateCfg {
    pub enabled: bool,
    pub cpu_max: u32,
    pub memory_min: u32,
    /// Auto: CPU is the 60 s mean; the latest 30 s interval at 100 % still vetoes; memory also holds below
    /// [`FLOOR_GIB`] available or at macOS pressure level [`PRESSURE_CRITICAL`] (D03).
    pub mean: bool,
}

pub const GATE_DEFAULT: GateCfg = GateCfg { enabled: false, cpu_max: 85, memory_min: 15, mean: false };
/// Auto admission defaults from the D03 calibration (docs/design/admission-calibration.md).
pub const GATE_AUTO: GateCfg = GateCfg { enabled: true, cpu_max: 90, memory_min: 10, mean: true };
pub const FLOOR_GIB: f64 = 2.5;
pub const PRESSURE_CRITICAL: u32 = 4;
pub use crate::runtime::RUNTIMES;

/// Effective capacity of one host (§ 3, § 9).
#[derive(Clone, PartialEq, Debug)]
pub struct Capacity {
    pub limits: BTreeMap<String, u32>,
    pub gate: GateCfg,
    /// `admission` mode `auto`: the host authority admits; a runtime without a limit has no count ceiling.
    pub auto: bool,
    /// Auto: optional host-wide session ceiling.
    pub max: Option<u32>,
}

impl Capacity {
    pub fn fixed(limits: BTreeMap<String, u32>, gate: GateCfg) -> Capacity {
        Capacity { limits, gate, auto: false, max: None }
    }

    pub fn json(&self) -> Value {
        json!({"limits": self.limits, "load_gate": {"enabled": self.gate.enabled, "cpu_max": self.gate.cpu_max, "memory_min": self.gate.memory_min},
            "admission": {"mode": if self.auto { "auto" } else { "fixed" }, "max": self.max}})
    }
}

/// Capacity from host policy text (§ 9): omitted gate fields take the defaults. Invalid values are an error.
pub fn capacity(v: &Value) -> Result<Capacity, String> {
    let limits = |x: &Value, at: &str| -> Result<BTreeMap<String, u32>, String> {
        match x {
            Value::Null => Ok(BTreeMap::new()),
            Value::Object(m) => m.iter().map(|(k, n)| n.as_u64().and_then(|n| u32::try_from(n).ok()).map(|n| (k.clone(), n)).ok_or(format!("{at}.{k}: not an integer 0..{}", u32::MAX))).collect(),
            _ => Err(format!("{at}: not an object")),
        }
    };
    let out = limits(&v["limits"], "limits")?;
    let a = &v["admission"];
    if !matches!(a, Value::Null | Value::Object(_)) {
        return Err("admission: not an object".into());
    }
    let auto = match &a["mode"] {
        Value::Null => false,
        x if x == "auto" => true,
        x if x == "fixed" => false,
        _ => return Err("admission.mode: auto or fixed".into()),
    };
    let max = match &a["max"] {
        Value::Null => None,
        x => Some(x.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(format!("admission.max: not an integer 0..{}", u32::MAX))?),
    };
    let d = if auto { GATE_AUTO } else { GATE_DEFAULT };
    let g = &v["load_gate"];
    if !matches!(g, Value::Null | Value::Object(_)) {
        return Err("load_gate: not an object".into());
    }
    let pct = |k: &str, d: u32, lo: u64, hi: u64| match &g[k] {
        Value::Null => Ok(d),
        x => x.as_u64().filter(|n| (lo..=hi).contains(n)).map(|n| n as u32).ok_or(format!("load_gate.{k}: not an integer {lo}..{hi}")),
    };
    let enabled = match &g["enabled"] {
        Value::Null => d.enabled,
        x => x.as_bool().ok_or("load_gate.enabled: not true or false")?,
    };
    if auto && !enabled {
        return Err("load_gate.enabled: Auto admission needs the load gate; use admission mode fixed".into());
    }
    let gate = GateCfg { enabled, cpu_max: pct("cpu_max", d.cpu_max, 10, 100)?, memory_min: pct("memory_min", d.memory_min, 1, 95)?, mean: d.mean };
    Ok(Capacity { limits: out, gate, auto, max })
}

/// The load gate (§ 3): samples on a monotonic clock in seconds, decisions on unrounded percentages.
#[derive(Default, Clone)]
pub struct Gate {
    cfg: Option<GateCfg>,
    /// CPU (busy, total) ticks of the previous reading: the baseline of the next.
    base: Option<(u64, u64)>,
    /// The reading before `base`: with it CPU is the mean over two intervals (60 s, `GateCfg::mean`).
    prev: Option<(u64, u64)>,
    /// Latest sample: monotonic time, wall time, CPU busy %, available memory %.
    pub last: Option<(u64, i64, Option<f64>, Option<f64>)>,
    /// Auto: available GiB and pressure level read with the latest sample; `None` is a failed reading.
    pub host_mem: Option<(f64, u32)>,
    /// Consecutive calm samples toward reopening.
    good: u32,
    open: bool,
    /// Monotonic time of the last launch and whether a sample followed it.
    launched: Option<u64>,
    sampled: bool,
}

impl Gate {
    /// Enabling or changing thresholds starts held.
    pub fn configure(&mut self, c: GateCfg) {
        if self.cfg != Some(c) {
            (self.cfg, self.open, self.good) = (Some(c), false, 0);
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.is_some_and(|c| c.enabled)
    }

    /// One sample: CPU ticks `(busy, total)` and available memory %; `None` is a failed reading.
    pub fn observe(&mut self, now: u64, wall: i64, ticks: Option<(u64, u64)>, memory: Option<f64>) {
        // decreasing counters or an empty interval are no reading; the baseline restarts either way
        let busy = |from: Option<(u64, u64)>| match (from, ticks) {
            (Some((b0, t0)), Some((b, t))) if b >= b0 && t > t0 && b - b0 <= t - t0 => Some((b - b0) as f64 * 100.0 / (t - t0) as f64),
            _ => None,
        };
        let raw = busy(self.base);
        let c = self.cfg.unwrap_or(GATE_DEFAULT);
        // the 60 s mean, unless the latest interval alone is at 100 % (veto) or no older reading exists
        let cpu = if c.mean && raw.is_some_and(|p| p < 100.0) { busy(self.prev).or(raw) } else { raw };
        let late = self.last.is_some_and(|l| now.saturating_sub(l.0) > 60);
        self.prev = if raw.is_some() { self.base } else { None };
        self.base = ticks;
        let memory = memory.filter(|m| (0.0..=100.0).contains(m));
        self.last = Some((now, wall, cpu, memory));
        self.sampled = true;
        let (max, min) = (c.cpu_max as f64, c.memory_min as f64);
        let floor = !c.mean || self.host_mem.is_some_and(|(g, l)| g >= FLOOR_GIB && l < PRESSURE_CRITICAL);
        match (cpu, memory) {
            (Some(p), Some(m)) if !late && floor && p < max && m > min => {
                if p <= max - 10.0 && m >= min + 5.0 {
                    self.good += 1;
                    self.open |= self.good >= 3;
                } else {
                    self.good = 0;
                }
            }
            _ => (self.open, self.good) = (false, 0),
        }
    }

    /// `off`, `open`, `held` or `unknown` with every blocker `(code, text)` (§ 4 `capacity`).
    pub fn state(&self, now: u64) -> (&'static str, Vec<(&'static str, String)>) {
        let Some(c) = self.cfg.filter(|c| c.enabled) else { return ("off", vec![]) };
        let mut r = vec![];
        match self.last {
            None => r.push(("sample", "no sample yet".to_string())),
            Some((at, ..)) if now.saturating_sub(at) > 60 => r.push(("sample", format!("last sample {} s old", now - at))),
            Some((_, _, cpu, mem)) => {
                match cpu {
                    None => r.push(("sample", "no CPU reading yet".into())),
                    Some(p) if p >= c.cpu_max as f64 => r.push(("cpu", format!("CPU {p:.0} % >= {} %", c.cpu_max))),
                    _ => {}
                }
                match mem {
                    None => r.push(("sample", "no memory reading".into())),
                    Some(m) if m <= c.memory_min as f64 => r.push(("memory", format!("available memory {m:.0} % <= {} %", c.memory_min))),
                    _ => {}
                }
                match self.host_mem.filter(|_| c.mean) {
                    None if c.mean => r.push(("sample", "no memory floor reading".into())),
                    Some((g, _)) if g < FLOOR_GIB => r.push(("memory", format!("available memory {g:.1} GiB < {FLOOR_GIB} GiB"))),
                    Some((_, l)) if l >= PRESSURE_CRITICAL => r.push(("memory", format!("memory pressure level {l} (critical)"))),
                    _ => {}
                }
            }
        }
        if r.is_empty() && !self.open {
            r.push(("cooldown", format!("{} of 3 calm samples", self.good)));
        }
        (if r.iter().any(|x| x.0 == "sample") { "unknown" } else if r.is_empty() { "open" } else { "held" }, r)
    }

    /// Off, or open with the previous launch at least 30 s ago and sampled since (§ 3).
    pub fn may_launch(&self, now: u64) -> bool {
        !self.enabled() || (self.state(now).0 == "open" && self.launched.is_none_or(|t| self.sampled && now.saturating_sub(t) >= 30))
    }

    pub fn launch(&mut self, now: u64) {
        (self.launched, self.sampled) = (Some(now), false);
    }
}

/// Append `(task, kind, id, outcome)` moves to the events file at board time `now` (§ 4): one line per id and kind
/// (`ask`: its card entry, else the attempt), a `close` followed by its § 7 outcome text;
/// lines older than 30 days dropped. Returns the first time covered.
pub fn record(path: &std::path::Path, now: i64, moves: &[(u64, &str, u64, String)]) -> Result<i64, String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines();
    let since = lines.next().and_then(|l| l.strip_prefix("since ")?.parse().ok()).unwrap_or(now).max(now - MONTH);
    let mut keep: Vec<String> = lines.filter(|l| l.split(' ').next().and_then(|t| t.parse::<i64>().ok()).is_some_and(|t| t >= since)).map(String::from).collect();
    for (n, kind, a, outcome) in moves {
        if !keep.iter().any(|l| l.split(' ').skip(2).take(2).eq([*kind, a.to_string().as_str()])) {
            keep.push(format!("{now} #{n} {kind} {a} {outcome}").trim_end().into());
        }
    }
    let out: String = std::iter::once(format!("since {since}")).chain(keep).map(|l| l + "\n").collect();
    if out != text {
        crate::platform::write_atomic(path, &out)?;
    }
    Ok(since)
}

fn record_hold(cfg: &Config, rt: &str, role: &str, task: u64, attempt: u64, pid: u32, reason: &str) {
    crate::telemetry::record_worker(&cfg.install, &cfg.repo, &cfg.me, rt, role, "slot-hold", task, attempt, pid, "ok", reason);
    crate::telemetry::record_worker(&cfg.install, &cfg.repo, &cfg.me, rt, role, "spawn", task, attempt, pid, "ok", reason);
}

/// R12/#2380: a launch can return before its process is known, and Claude reports the PID at a later agents readback.
/// slot-hold, spawn and the heartbeat of the launch carry a real PID, so they wait for this readback.
fn record_known_pid(cfg: &Config, unknown_pids: &mut HashMap<u64, &'static str>, k: &Kid, role: &str) {
    let pid = k.proc.id();
    if pid <= 1 {return;}
    let Some(reason) = unknown_pids.remove(&k.execution) else {return};
    record_hold(cfg, &k.rt, role, k.task, k.attempt, pid, reason);
    crate::telemetry::record_worker(&cfg.install, &cfg.repo, &cfg.me, &k.rt, role, "heartbeat", k.task, k.attempt, pid, "ok", if k.stopping { "stopping" } else { "running" });
}

/// R12: retirement and a slot with no handle name only the saved process identity of that execution; 0 when none is saved.
fn saved_pid(cfg: &Config, task: u64, attempt: u64, execution: u64) -> u32 {
    crate::paths::kids_path(&cfg.install, &cfg.root).ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| crate::data::kids(&text).ok())
        .and_then(|(_, kids)| kids.into_iter().find(|k| k.task == task && k.attempt == attempt && k.execution == execution))
        .map_or(0, |k| k.process.pid)
}

/// #2380: without a saved PID the slot stays held and records nothing; a later launch or adoption records it.
fn record_unresolved_hold(cfg: &Config, task: u64, attempt: u64, execution: u64, rt: &str) {
    let pid = saved_pid(cfg, task, attempt, execution);
    if pid > 1 {
        crate::telemetry::record_worker(&cfg.install, &cfg.repo, &cfg.me, rt, "work", "slot-hold", task, attempt, pid, "ok", "unresolved-ownership");
    }
}

impl<'a> Service<'a> {
    pub fn new(board: &'a dyn Board, cfg: &'a Config, version: &str, start: Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>, crate::runtime::StartError> + 'a>) -> Self {
        let cap = Ok(Capacity::fixed(cfg.limits.clone(), GATE_DEFAULT));
        Service { progress_clock: |_| crate::runtime::now(), progress_budget: std::time::Duration::from_millis(50), last_spawn: None, queue: Default::default(), restart_since: None, headroom_error: None, progress: HashMap::new(), recovery_after: None, reduced_at: None, host: None, wall: 0, reserved: vec![], requested: vec![], refused: None, host_used: None, project_revision: None, board, cfg: cfg.clone(), version: version.into(), diagnostics: Default::default(), binary: Default::default(), health: None, answered: None, started: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs()), kids: vec![], instance: String::new(), link: String::new(), machine: String::new(), focus: vec![], focus_error: None, delivered: HashMap::new(), cache: HashMap::new(), deps: HashMap::new(), dep_closed: None, label_fixes: HashMap::new(), cancelled_publications: HashMap::new(), summarized: HashSet::new(), summary_fixes: HashMap::new(), publication_waits: HashMap::new(), delivery_error: None, unowned: HashSet::new(), tidied: None, start, recover: None, boot: if cfg!(test) {|| Err("boot time not read in tests".into())} else {crate::platform::boot_time as fn() -> Result<i64, String>}, #[cfg(test)] agent_status: json!([]), log: vec![], snapshot: None, tasks_read: None, ownership_known: false, host_issue: None, observations: BTreeMap::new(), ownership_waits: BTreeMap::new(), seen: HashMap::new(), stuck: HashMap::new(), churned: HashSet::new(), gone_after: None, moved_after: None, events_since: None, cap, gate: Gate::default(), mono: 0, held: vec![], pending: BTreeSet::new(), unseen: HashMap::new(), hold: None, stop_progress: vec![], refused_access: BTreeMap::new(), limited: BTreeMap::new(), logged_out: BTreeSet::new(), clis: BTreeMap::new(), accounts: BTreeMap::new(), fleet: Default::default(), gui_checked: None, gui_probe: Err("not probed".into()), gui_stale: false, grants: BTreeMap::new(), full: BTreeSet::new(), waits: BTreeMap::new(), execution_waits: BTreeMap::new(), pending_releases: BTreeSet::new(), unknown_pids: HashMap::new(), begun: HashMap::new(), asked: HashMap::new(), listed: HashSet::new(), unlisted: BTreeSet::new(), }
    }

    /// Rebuild board-derived state after unwind; live handles, pending launches and delivery cursors stay owned.
    pub fn rebuild(&mut self) {
        self.unready();
        self.cache.clear();
        self.deps.clear();
        self.label_fixes.clear();
        self.snapshot = None;
        self.tasks_read = None;
    }

    /// Quarantine/backoff: only verified reaping, leases and status; no claims, resumes or recovery writes.
    pub fn reduced(&mut self) -> Result<(), String> {
        self.unready();
        self.full_stop_owned();
        let before = self.kids.len();
        self.reap_owned();
        self.reconcile_ownership()?;
        self.sync_host();
        self.beat_reduced(before != self.kids.len())
    }

    /// #1766: the Stop barrier keeps an idle prepared project out of passes (#1402); it still reads the board for
    /// status.json and beats. No claim, resume, recovery, label or event write; ownership stays as last reported.
    pub fn observe(&mut self) -> Result<(), String> {
        self.unready();
        let (views, hosts) = views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?;
        let now = self.board.now()?;
        self.snapshot = Some(crate::snapshot::from_views("host", self.board, &self.cfg, now, &views, &hosts));
        self.copy_images();
        self.tasks_read = Some(std::time::Instant::now());
        self.beat_reduced(false)
    }

    /// The heartbeat of a reduced or observe pass: read at most every `ttl / 5`, at once when its children changed;
    /// written like a full pass (§ 4 step 3): at each `renewal` or on a change `stable` compares.
    fn beat_reduced(&mut self, changed: bool) -> Result<(), String> {
        if changed || self.reduced_at.is_none_or(|t| self.mono.saturating_sub(t) >= (self.cfg.ttl / 5).max(1) as u64) {
            self.reduced_at = Some(self.mono);
            if let Some(n) = self.host_issue {
                let hb = self.board.issue(n)?;
                let now = self.board.now()?;
                let body = self.heartbeat_body();
                if now - hb.updated >= renewal(&body, self.cfg.ttl) || stable(&hb.body) != stable(&body) {
                    self.board.edit(n, &format!("{body}\nat {now}"))?;
                }
            }
        }
        Ok(())
    }

    pub fn unready(&self) {
        if let Some(host) = &self.host { if let Ok(mut h) = host.lock() { h.ready(&self.cfg.root, false); } }
    }

    /// R11/E2: cancellation precedes board access, including reduced/offline passes. No release is fabricated.
    pub fn full_stop_owned(&mut self) {
        let full=match crate::lifecycle::full_stop(&self.cfg.install,&self.cfg.root) {
            Ok(value)=>value,Err(reason)=>{self.log.push(reason);return;}
        };
        if !full {return;}
        self.stop_progress.clear();
        for k in &mut self.kids {
            if crate::recovery::selected(&self.cfg.install,&self.cfg.root,k.task) {continue;}
            k.stopping=true;
            let observed=match k.proc.exited() {
                Ok(Some(code))=>Ok(code),
                _=>crate::runtime::stop(k.proc.as_mut()),
            };
            let child=crate::snapshot::Child {exited:observed.is_ok(),unknown:observed.as_ref().err().map(String::as_str),stopping:true};
            let (state,reason)=match crate::snapshot::verdict(&crate::snapshot::Signals {local:true,child:Some(child),..Default::default()}) {
                ("ended",_)=>("stopped",Some("native exit observed; board release checked separately".to_owned())),
                (_,reason)=>{k.unknown=Some(reason.clone());("unknown",Some(reason))},
            };
            self.stop_progress.push(json!({"task":k.task,"attempt":k.attempt,"execution":k.execution,"rt":k.rt,"state":state,"reason":reason}));
        }
    }

    /// Liveness monitoring never frees ownership before the exact execution release is read back.
    pub fn reap_owned(&mut self) {self.reap_owned_within(NATIVE_PASS_BUDGET);}

    fn reap_owned_within(&mut self,budget:std::time::Duration) {
        let until=std::time::Instant::now()+budget;
        let mut deferred=None;
        let mut retired=vec![];
        self.kids.retain_mut(|k| {
            if std::time::Instant::now()>=until {
                deferred.get_or_insert(k.execution);
                k.unknown=Some("native observation deferred; reduced pass budget exhausted".into());return true;
            }
            let status = crate::runtime::observe_until(until,||k.proc.exited());
            record_known_pid(&self.cfg, &mut self.unknown_pids, k, self.begun.get(&k.attempt).map_or("work", |(r, _, _)| if r == "reviewer" { "review" } else { "work" }));
            let mut acceptance = None;
            match crate::runtime::observe_until(until,||k.proc.accepted()) {
                Ok(true) => match accept_execution(self.board,k.task,k.attempt,k.execution,None) {
                    Ok(through) => { self.delivered.entry(k.attempt).and_modify(|v| *v = (*v).max(through)).or_insert(through); },
                    Err(e) => { k.unknown=Some(e); return true; }
                },
                Ok(false) => {},
                Err(e) => acceptance = Some(e),
            }
            // #1594: a failed exit read, then a failed acceptance read, then a cancellation's earlier failure stays unknown
            let unknown = status.as_ref().err().or(acceptance.as_ref()).or(k.unknown.as_ref().filter(|_| k.stopping)).cloned();
            let child = crate::snapshot::Child { exited: matches!(status, Ok(Some(_))), unknown: unknown.as_deref(), stopping: k.stopping };
            match crate::snapshot::verdict(&crate::snapshot::Signals { local: true, child: Some(child), ..Default::default() }) {
            ("ended", _) => match k.release(self.board,status.as_ref().unwrap().unwrap(),&k.proc.lost_input().unwrap_or_default()) {
                Ok(_)=>{
                    let code = status.as_ref().unwrap().unwrap();
                    let role_code = self.begun.get(&k.attempt).map_or("work", |(r, _, _)| if r == "reviewer" { "review" } else { "work" });
                    let exit_reason = k.proc.exit_error().map_or_else(|| format!("exit-{code}"), String::from);
                    crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "exit", k.task, k.attempt, k.proc.id(), if code == 0 { "ok" } else { "failed" }, &exit_reason);
                    retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).has("cli"),k.proc.id()));
                    false
                },
                Err(e)=>{k.unknown=Some(e);true}
            },
            ("unknown", e) => { k.unknown=Some(e); true }
            _ => { k.unknown=None; true }
            }
        });
        for (task,attempt,execution,native,pid) in retired { self.retired(task,attempt,execution,native,pid); }
        if let Some(first)=deferred.and_then(|id|self.kids.iter().position(|k|k.execution==id)) {self.kids.rotate_left(first);}
    }

    /// Called only after release readback; retained children keep all their live metadata and capacity.
    fn retired(&mut self, task:u64, attempt:u64, execution:u64, native:bool, pid:u32) {
        let rt = self.observations.get(&(task, execution)).map(|x| x.runtime.clone())
            .or_else(|| self.reserved.iter().find(|(n, a, _)| *n == task && *a == attempt).map(|(_, _, r)| r.clone()))
            .or_else(|| self.begun.get(&attempt).map(|(_, r, _)| r.clone()))
            .unwrap_or_else(|| "other".into());
        // R12: retirement is historical identity, never a liveness query. Recovery may have no handle.
        let pid = if pid > 1 { pid } else { saved_pid(&self.cfg, task, attempt, execution) };
        let role = self.begun.get(&attempt).map_or("work", |(r, _, _)| if r == "reviewer" { "review" } else { "work" });
        crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &rt, role, "slot-release", task, attempt, pid, if pid > 1 { "ok" } else { "unknown" }, if pid > 1 { "execution-released" } else { "execution-released-pid-unknown" });
        if native {
            if let Err(e)=crate::service_run::retire_kid(&self.cfg,task,attempt,execution) { self.log.push(format!("native recovery cleanup: {e}")); }
        }
        self.pending_releases.remove(&(task,attempt,execution));
        self.unknown_pids.remove(&execution);
        self.observations.remove(&(task,execution));
        self.ownership_waits.remove(&(task,execution));
        self.cache.remove(&task);
        // The execution receipt above already reports this slot; do not emit an identity-free duplicate.
        self.retire_reservations(BTreeSet::from([(task,attempt)]));
    }

    fn retire_reservations(&mut self, released:BTreeSet<(u64,u64)>) {
        for (task,attempt) in released {
            if self.kids.iter().any(|k|k.task==task && k.attempt==attempt)
                || self.observations.iter().any(|((n,_),x)|*n==task && x.attempt==attempt)
                || self.held.iter().any(|l|l.task==task && l.attempt==attempt) {continue;}
            self.reserved.retain(|(n,a,_)|*n!=task || *a!=attempt);
            self.pending.remove(&(task,attempt));
        }
    }

    /// New capacity config (§ 3): a gate change starts held.
    pub fn reload(&mut self, cap: Result<Capacity, String>) {
        if let Ok(c) = &cap {
            self.gate.configure(c.gate);
        }
        self.cap = cap;
    }

    /// Rereads `taskq.json` for a pass (§ 3 capacity) and takes its § 7 delivery preset, so a pending publication
    /// follows an owner's preset switch without a restart (#1942). An invalid preset keeps the last one, logged once.
    pub fn reread_project(&mut self) -> Result<Value, String> {
        let v = crate::config::project_value(&self.cfg.root)?;
        match crate::config::delivery(&v) {
            Ok(d) => {
                if d != (self.cfg.publish.clone(), self.cfg.delivery.clone(), self.cfg.delivery_revision) {
                    log!("delivery {} r{} (was {} r{})", d.1, d.2, self.cfg.delivery, self.cfg.delivery_revision);
                    (self.cfg.publish, self.cfg.delivery, self.cfg.delivery_revision) = d;
                }
                self.delivery_error = None;
            }
            Err(e) => {
                if self.delivery_error.as_ref() != Some(&e) { log!("{e}; keeping delivery {} r{}", self.cfg.delivery, self.cfg.delivery_revision); }
                self.delivery_error = Some(e);
            }
        }
        Ok(v)
    }

    fn may_launch(&self,task:u64) -> bool {
        self.launch_refusal(task).is_none()
    }

    /// Why no launch of `task` may start now, as a #1616 admission code and reason; the trace keeps its § 3 code
    /// under the `may_launch` span every launch check emits.
    #[tracing::instrument(name="service.Service.may_launch",skip_all)]
    fn launch_refusal(&self,task:u64) -> Option<(&'static str,String)> {
        let hold = self.hold.clone().filter(|_| !crate::recovery::allowed(&self.cfg.install,&self.cfg.root,task))
            .or_else(|| crate::admission::task_hold(&self.cfg.install,&self.cfg.root,task));
        let refusal = if let Some(h) = hold { Some(("hold", "hold", h)) }
            else if self.cap.is_err() { Some(("config", "unknown", crate::snapshot::ADMISSION_UNKNOWN[1].to_string())) }
            else if !self.gate.may_launch(self.mono) {
                let (code, text) = self.gate.state(self.mono).1.into_iter().next().unwrap_or(("launch-interval", "launch interval".into()));
                Some((code, "load-gate", format!("{code}: {text}")))
            } else { None };
        let (code, admission, reason) = refusal?;
        tracing::info!(coreq_refusal = code);
        Some((admission, reason))
    }

    fn auto(&self) -> bool {
        self.cap.as_ref().is_ok_and(|c| c.auto)
    }

    /// This service's entry in the host authority: children and granted reservations are held.
    fn entry(&self) -> Entry {
        let held = self.kids.iter().map(|k| (k.task, k.attempt, k.rt.clone())).chain(self.reserved.iter().cloned())
            .collect::<BTreeSet<_>>().into_iter().collect();
        Entry { version: self.version.clone(), at: 0, held, requested: self.requested.clone() }
    }

    /// Lost native handles still hold reservations and must retain their recovery inputs.
    pub fn retained_attempts(&self)->Vec<u64> {
        self.entry().held.into_iter().map(|(_,attempt,_)|attempt).collect()
    }

    fn observe_ownership(&mut self, views:&[View]) -> Result<(),String> {
        let mut released=BTreeSet::new();
        for task in self.observations.keys().map(|(task,_)|*task).collect::<BTreeSet<_>>() {
            let read;
            let current = if let Some(v)=views.iter().find(|v|v.issue.number==task) { &v.projection.executions }
                else { read=core::project(task, &crate::board::entries(self.board,task)?, false); &read.executions };
            // Open-list absence, label removal and missing history never release known ownership.
            self.observations.retain(|(n,id),old| {
                if *n!=task {return true;}
                let reason = match current.iter().find(|x|x.id==*id) {
                    None => Some("execution absent from trusted history".into()),
                    Some(x) if x.attempt!=old.attempt => Some(format!("attempt mismatch: observed {}, board {}",old.attempt,x.attempt)),
                    Some(x) if x.host!=old.host => Some(format!("host mismatch: observed {}, board {}",old.host,x.host)),
                    Some(x) if x.runtime!=old.runtime => Some(format!("runtime mismatch: observed {}, board {}",old.runtime,x.runtime)),
                    Some(x) if x.release.is_none() => Some("matching terminal release absent from trusted history".into()),
                    _ => None,
                };
                let terminal=reason.is_none() && !self.kids.iter().any(|k|k.task==task && k.execution==*id);
                if let Some(reason)=reason {self.ownership_waits.insert((*n,*id),reason);}
                else {self.ownership_waits.remove(&(*n,*id));}
                if terminal {released.insert((task,old.attempt,*id));}
                !terminal
            });
        }
        self.observations.extend(views.iter().flat_map(|v|v.projection.executions.iter().cloned()
            .filter(|x|x.host==self.cfg.me && x.release.is_none()).map(|x|((v.issue.number,x.id),x))));
        for (task,attempt,execution) in released {self.retired(task,attempt,execution,false,0);}
        for ((task,_),x) in &self.observations {
            let reservation=(*task,x.attempt,x.runtime.clone());
            if !self.kids.iter().any(|k|k.task==*task && k.execution==x.id) && !self.reserved.contains(&reservation) {
                record_unresolved_hold(&self.cfg, *task, x.attempt, x.id, &x.runtime);
                self.reserved.push(reservation);
            }
        }
        self.ownership_known=true;
        Ok(())
    }

    /// Recovery monitoring reads ownership only; it cannot resume ordinary task effects.
    pub fn reconcile_ownership(&mut self) -> Result<(),String> {
        if self.ownership_known && (self.handoff_waits(false,false).is_empty()
            || self.tasks_read.is_some_and(|at|at.elapsed().as_secs()<self.cfg.poll.max(1))) {return Ok(());}
        self.ownership_known=false;
        let (views,hosts)=views(self.board,&mut self.cache,&mut self.deps,self.cfg.ttl)?;
        let now=self.board.now()?;
        self.observe_ownership(&views)?;
        self.snapshot=Some(crate::snapshot::from_views("host",self.board,&self.cfg,now,&views,&hosts));
        self.tasks_read=Some(std::time::Instant::now());
        Ok(())
    }

    /// Handles may cross a qualified image switch; childless uncertainty cannot. `handover` (#1601): a pending re-exec
    /// refuses every reservation, so a childless reserved or pending attempt passes to the next image, which reads it
    /// from the board and resumes it from the inbox; held launches are released first (`release_held`) and wait until
    /// then. `exit` (#1435): a Windows image exit hands over no handle and the next image reads the same unresolved
    /// executions from the board, so childless board observations pass too; release state still waits.
    pub fn handoff_waits(&self, handover:bool, exit:bool) -> Vec<String> {
        let owns=|task,attempt|self.kids.iter().any(|k|k.task==task && k.attempt==attempt);
        let mut waits=BTreeSet::new();
        if !self.ownership_known {waits.insert("ownership unknown".into());}
        for (task,attempt) in self.reserved.iter().map(|(n,a,_)|(*n,*a)).chain(self.pending.iter().copied()).filter(|_|!handover)
            .chain(self.held.iter().map(|l|(l.task,l.attempt))) {
            if !owns(task,attempt) {waits.insert(format!("#{task} unresolved ownership"));}
        }
        // #1594: a child of this process hands over through the runtime adapter (lifecycle_waits); the rest wait on the verdict
        for ((task,id),x) in self.observations.iter().filter(|_|!exit) {
            let signals=self.signals(*task,x);
            if signals.child.is_none() && crate::snapshot::verdict(&signals).0!="ended" {
                let reason=self.ownership_waits.get(&(*task,*id)).map(String::as_str).unwrap_or("matching terminal release not yet observed");
                waits.insert(format!("#{task} unresolved execution {id} attempt {} ({reason})",x.attempt));
            }
        }
        for (task,_,_) in &self.pending_releases {waits.insert(format!("#{task} release unconfirmed"));}
        waits.into_iter().collect()
    }

    /// #1601: a pending restart refuses every reservation, so a held launch could never start and the handover would
    /// wait on it. Its start was never called: release it `no-start` and let the next image act on the board.
    // ponytail: a released initial claim ends as `gone` (+1 retry); a held-claim handover record would keep the retry.
    fn release_held(&mut self) -> Vec<u64> {
        if !self.host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.restart)) {return vec![];}
        let mut released=vec![];
        while let Some(l)=self.held.first() {
            let (task,attempt,execution)=(l.task,l.attempt,l.execution.unwrap_or(l.attempt));
            if let Err(e)=release_execution(self.board,task,attempt,execution,"no-start") {
                self.log.push(format!("#{task} held launch release: {e}"));
                break;
            }
            self.held.remove(0);
            self.log.push(format!("#{task} held launch released for the restart"));
            self.retired(task,attempt,execution,false,0);
            released.push(task);
        }
        if !released.is_empty() {self.sync_host();}
        released
    }

    pub fn idle(&self) -> bool {self.kids.is_empty() && self.handoff_waits(false,false).is_empty()}

    /// § 4 Focus: this host admits nothing; an unreadable focus file stays unknown.
    pub fn focus_off(&self) -> bool {self.focus.is_empty() && self.focus_error.is_none()}

    /// Admission of one launch (§ 3): the load gate, then under Auto a reservation from the host authority, which
    /// refuses with a reason; the request stays `requested`. Fixed admits as before.
    fn reserve(&mut self, task: u64, attempt: u64, rt: &str) -> bool {
        if !self.may_launch(task) {
            return false;
        }
        if let Some(host) = self.host.clone() {
            let r = host.lock().map_err(|_| "host admission poisoned; launches held".to_string()).and_then(|mut h| {
                let now = h.elapsed();
                h.reserve(&self.cfg.root, task, attempt, rt, self.cap.as_ref().unwrap(), attempt == 0, now)
            });
            return match r {
                Ok(used) => {
                    let res = (task, attempt, rt.to_string());
                    if !self.reserved.contains(&res) { self.reserved.push(res); }
                    self.host_used = Some(used as u32);
                    true
                }
                Err(e) => { self.refused = Some(("host", e)); false }
            };
        }
        if !self.auto() { return true; }
        let old = crate::admission::old_services(&self.cfg.install);
        let res = (task, attempt, rt.to_string());
        let want = Want { max: self.cap.as_ref().ok().and_then(|c| c.max), sample_at: self.gate.last.map(|l| l.1), old: &old, res: res.clone() };
        let (r, used) = crate::admission::sync(&self.cfg.install, &self.cfg.root.display().to_string(), self.entry(), Some(want), self.wall);
        self.host_used = used;
        match r {
            Ok(()) => {
                self.reserved.push(res);
                true
            }
            Err(e) => {
                self.requested.push(res);
                self.refused = Some(e);
                false
            }
        }
    }

    /// Rewrite this service's host entry: released reservations leave it (§ 3). Fixed services write too, so Auto
    /// counts their children; a failed write only matters to Auto, whose next request refuses.
    pub fn sync_host(&mut self) {
        if let Some(host) = &self.host {
            let recovered=self.handoff_waits(false,false).is_empty();
            let held=self.entry().held;
            match host.lock() {
                Ok(mut h) => {
                    if recovered {h.recovered.insert(self.cfg.root.clone());}else {h.recovered.remove(&self.cfg.root);}
                    if h.rebuilding&&self.ownership_known {h.rebuild_slots(&self.cfg.root,held);}else {h.sync(&self.cfg.root,held);}
                    self.host_used=(!h.rebuilding).then(||h.used().values().sum::<usize>() as u32);
                }
                Err(e) => {
                    self.host_used = None;
                    self.refused = Some(("host", "host admission poisoned; rebuilding from owned handles".into()));
                    let mut h = e.into_inner();
                    h.reload(crate::host::Policy::read(&self.cfg.install));
                    if h.rebuild_slots(&self.cfg.root, self.entry().held) { host.clear_poison(); }
                }
            }
            return;
        }
        let (r, used) = crate::admission::sync(&self.cfg.install, &self.cfg.root.display().to_string(), self.entry(), None, self.wall);
        self.host_used = used;
        if let (Err((_, e)), true) = (r, self.auto()) {
            self.refused = Some(("host", e));
        }
    }

    /// The `capacity` object of `status.json` (§ 4).
    pub fn capacity_json(&self) -> Value {
        let (mut gate, mut reasons) = match &self.cap {
            Err(e) => ("unknown", vec![("config", e.clone())]),
            Ok(_) => self.gate.state(self.mono),
        };
        if let Some(h) = &self.hold {
            (gate, reasons) = ("held", [vec![("hold", h.clone())], reasons].concat());
        }
        if let Some((code, text)) = self.refused.clone().filter(|_| self.auto()) {
            gate = if code == "host" { "unknown" } else if gate == "open" { "held" } else { gate };
            reasons.push((code, text));
        }
        // § 4 Focus: an empty focus admits nothing, so it is a reason like any refusal (#974)
        if self.focus.is_empty() {
            reasons.push(("focus", self.focus_error.clone().unwrap_or_else(|| FOCUS_OFF.into())));
        }
        reasons.extend(self.refused_access.iter().map(|(rt, r)| ("access", format!("{rt}: {r}"))));
        reasons.extend(self.limited.keys().map(|rt| ("usage-limit", format!("{rt}: {}", self.limit_line(rt)))));
        reasons.extend(self.logged_out.iter().map(|rt| ("login", format!("{rt}: not logged in; {}", crate::runtime::relogin(rt)))));
        let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
        if accounts.error.is_some() { gate="unknown"; }
        reasons.extend(accounts.heartbeat(self.board.now().unwrap_or(self.wall)).into_iter().filter(|l|l.starts_with("quota ")).map(|l|("usage-limit",l)));
        let off_at = self.board.now().unwrap_or(self.wall);
        reasons.extend(crate::runtime::CLI_RUNTIMES.iter().filter(|rt| accounts.off(rt, off_at)).map(|rt| ("capability", format!("{rt}: {}", accounts.off_reason(rt)))));
        let res =|r: &Res, state: &str| json!({"task": r.0, "attempt": r.1, "rt": r.2, "state": state});
        let reservations: Vec<Value> = self.kids.iter().map(|k| res(&(k.task, k.attempt, k.rt.clone()), "held"))
            .chain(self.reserved.iter().map(|r| res(r, "held"))).chain(self.requested.iter().map(|r| res(r, "requested"))).collect();
        let limits = self.cap.as_ref().map(|c| c.limits.clone()).unwrap_or_default();
        let mut used: BTreeMap<String, u32> = limits.keys().map(|k| (k.clone(), 0)).collect();
        for (_, _, rt) in self.entry().held {
            *used.entry(rt).or_default() += 1;
        }
        let last = self.gate.last.filter(|_| self.gate.enabled());
        let (host_policy,host_used,host_command,project_revision)=self.host.as_ref().and_then(|h|h.lock().ok().map(|h| {
            if h.rebuilding {gate="unknown";reasons.push(("host","host occupancy recovery pending".into()));}
            if let Some(reason)=&h.held {gate="unknown";reasons.push(("host",reason.clone()));}
            (h.policy.as_ref().map(|p|p.cap.json()).unwrap_or(Value::Null),if h.rebuilding {Value::Null}else{json!(h.used())},json!(h.capacity_command),json!(self.project_revision.as_ref().filter(|_|self.cap.is_ok())))
        })).unwrap_or((Value::Null,Value::Null,Value::Null,Value::Null));
        json!({"host_policy":host_policy,"host_runtime_used":host_used,"host_command":host_command,"project_revision":project_revision,"project_limits":self.cap.as_ref().ok().map(|_| &self.cfg.limits),"limits": limits, "used": used, "load_gate": self.cap.as_ref().map(|c| c.json()["load_gate"].clone()).unwrap_or(Value::Null),
            "admission": self.cap.as_ref().map(|c| c.json()["admission"].clone()).unwrap_or(Value::Null), "host_used": self.host_used,
            "min_service_version": crate::admission::MIN_SERVICE, "reservations": reservations,
            "gate": gate, "reasons": reasons.iter().map(|(c, t)| json!({"code": c, "text": t})).collect::<Vec<_>>(),
            "cpu_percent": last.and_then(|l| l.2), "memory_available_percent": last.and_then(|l| l.3),
            "sample_at": last.map(|l| l.1), "sample_age_seconds": last.map(|l| self.mono.saturating_sub(l.0))})
    }

    /// Handover carries execution identity; only board acceptance establishes delivery.
    pub fn adopt(&mut self, kid: Kid) -> Result<(),String> {
        let log = crate::board::entries(self.board,kid.task)?;
        let projection=core::project(kid.task,&log,false);
        let x=projection.executions.iter().find(|x|x.id==kid.execution && x.attempt==kid.attempt && x.host==self.cfg.me && x.runtime==kid.rt && x.release.is_none())
            .ok_or_else(||format!("execution {} handover does not match unresolved board ownership",kid.execution))?;
        if self.kids.iter().any(|k|k.task==kid.task || k.execution==kid.execution) { return Err("duplicate native handover".into()); }
        if let Some(through) = projection.delivered(kid.attempt) { self.delivered.insert(kid.attempt, through); }
        // the role is the attempt log that exists; its start and offset stayed in the replaced image
        let role = if crate::runtime::attempt_log(&self.cfg.root, "reviewer", kid.task, kid.attempt).exists() { "reviewer" } else { "worker" };
        self.begun.insert(kid.attempt, (role.into(), kid.rt.clone(), None));
        self.observations.insert((kid.task,kid.execution),x.clone());
        let role_code = if role == "reviewer" { "review" } else { "work" };
        if kid.proc.id() > 1 {
            record_hold(&self.cfg, &kid.rt, role_code, kid.task, kid.attempt, kid.proc.id(), "adopted");
        } else {
            self.unknown_pids.insert(kid.execution, "adopted");
        }
        self.kids.push(kid);
        Ok(())
    }

    pub fn delivered(&self, attempt: u64) -> u64 {
        self.delivered.get(&attempt).copied().unwrap_or(0)
    }

    /// The `build` or `review` stage of a child that exited or was stopped (§ 4 Profiling); `v` is its task's view.
    fn ended(&mut self, n: u64, a: u64, v: Option<&View>, failure: Option<&'static str>) {
        let Some((role, rt, begun)) = self.begun.remove(&a) else { return };
        // an issue that left the board read (closed, label removed) is read back once for its hand-in
        let back = if v.is_none() { crate::board::entries(self.board, n).map_err(|_| "unreadable") } else { Ok(vec![]) };
        let log = match v {
            Some(v) => Ok((v.log.as_slice(), Some(v.projection.task.attempt == Some(a)))),
            None => back.as_deref().map(|l| (l, None)).map_err(|e| *e),
        };
        let (outcome, err) = core::stage_outcome(n, log, a, a.max(self.delivered(a)));
        let err = if outcome == "failed" { failure.unwrap_or(err) } else { err };
        let log = crate::runtime::attempt_log(&self.cfg.root, &role, n, a);
        let stage = if role == "reviewer" { "review" } else { "build" };
        let (took, usage) = match begun {
            Some((began, pos)) => (Some(began.elapsed()), crate::runtime::usage(&log, pos, &rt)),
            None => (None, crate::profile::Usage::Unknown("adopted")),
        };
        crate::profile::stage(stage, n, a, &rt, took, outcome, err, usage);
    }

    /// The worker's last message in the attempt log as a public `gone` reason line of at most 240 characters
    /// (§ 4 step 1); empty when unreadable or blank.
    fn last_message(&self, n: u64, a: u64, t: &Task) -> String {
        const LABEL: &str = "Last message: ";
        let log = crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a);
        let line = crate::runtime::last_message(&log, rt_of(t).map_or("", |(rt, _)| rt)).map(|m| crate::runtime::public_line(&m, &self.cfg.root, 240 - LABEL.len()));
        line.filter(|m| !m.is_empty()).map_or_else(String::new, |m| format!("\n{LABEL}{m}"))
    }

    /// End a released attempt; only ordinary failures spend the retry budget (§ 2).
    fn gone(&mut self, n: u64, t: &Task, a: u64, args: &[&str], text: &str) -> Result<u64, String> {
        let text = if args.is_empty() { core::limit_text(n, t, "gone", text) } else { text.to_string() };
        let kind = if args == ["usage-limit"] { "usage-limit" } else { "gone" };
        let id = self.post(n, core::entry(kind, &[], Some(a), &text))?;
        if let Ok(log) = crate::board::entries(self.board, n) {
            let through: Vec<_> = log.into_iter().filter(|e| e.id <= id).collect();
            let before: Vec<_> = through.iter().filter(|e| e.id < id).cloned().collect();
            if core::fold(n, &before, false).attempt == Some(a) && core::fold(n, &through, false).attempt != Some(a) {
                crate::profile::repair(&self.cfg.install, &self.cfg.repo, host_of(t).unwrap_or(&self.cfg.me), n, a,
                    rt_of(t).map_or("other", |r| r.0), t.role.as_deref() == Some("reviewer"));
            }
        } else {
            eprintln!("Profiling: unknown (gone readback failed; terminal record not repaired)");
        }
        Ok(id)
    }

    /// #1879: `x` is task `n`'s initial spawn on this host and no start reached its native call: every start writes the
    /// attempt log's rules line first, and every start error before it is a no-start, so a missing log with no child, held
    /// launch or acceptance is a confirmed no-start.
    fn lost_spawn(&self, n: u64, x: &core::Execution) -> bool {
        x.id == x.attempt && x.host == self.cfg.me && !x.accepted && x.release.is_none()
            && !self.kids.iter().any(|k| k.task == n) && !self.held.iter().any(|l| l.task == n)
            && ["worker", "reviewer"].iter().all(|r| crate::runtime::attempt_log(&self.cfg.root, r, n, x.attempt).try_exists().is_ok_and(|e| !e))
    }

    fn exit_outcome(&mut self, n: u64, t: &Task, a: u64, hold: Option<(i64, bool)>, now: i64, text: &str) -> Result<u64, String> {
        let local = host_of(t) == Some(self.cfg.me.as_str());
        let hold = hold.or_else(|| local.then(|| {
            let log = crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a);
            crate::runtime::usage_limit(&log, rt_of(t).map_or("", |(rt, _)| rt)).map(|reset| reset.filter(|r| *r > now).map_or((now + USAGE_PROBE, false), |r| (r, true)))
        }).flatten());
        if let (Some(hold), Some((rt, _))) = (hold, rt_of(t)) {
            let (at, reported) = hold;
            // § 3 Runtime accounts: the limit is the attempt account's; a spare enabled account takes the next start
            let (next, hold) = if local { self.account_limit(n, t, a, rt, hold, now) } else { (None, hold) };
            if local && next.is_none() { self.limited.insert(rt.to_string(), hold); }
            let next = next.map(|l| format!("; next account {l}")).unwrap_or_default();
            let reason = format!("Usage limit: {rt} {} {}{next}", if reported { "until" } else { "probe" }, crate::board::stamp(at));
            self.gone(n, t, a, &["usage-limit"], &reason)
        } else if local && matches!(t.state,State::Doing|State::Review) && rt_of(t).is_some_and(|(rt,_)| crate::runtime::CLI_RUNTIMES.contains(&rt))
            && !crate::runtime::quota_windows(&crate::runtime::attempt_log(&self.cfg.root,t.role.as_deref().unwrap_or_default(),n,a),rt_of(t).unwrap().0).is_empty() {
            let rt = rt_of(t).unwrap().0;
            self.account_limit(n,t,a,rt,(now+USAGE_PROBE,false),now);
            self.post(n,core::entry("steer",&["quota"],Some(a),"Continue this same task and native session after the original account quota hold; preserve completed work."))
        } else if let Some((bin, reason)) = local.then(|| rt_of(t).map(|(rt, _)| (rt, crate::runtime::of(rt))).filter(|(_, r)| r.has("sandbox-args"))
            .and_then(|(rt, r)| crate::runtime::auth_rejected(&crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a), rt).map(|e| (r.bin, e)))).flatten() {
            // #1446: a rejected login holds Codex here at once and releases the attempt like a usage limit, retries kept
            if let Err(e) = crate::runtime::mark_login_off(&self.cfg.install, bin, &reason) { eprintln!("login-off not saved: {e}"); }
            self.logged_out.extend(crate::runtime::agent_runtimes().iter().filter(|r| crate::runtime::of(r).has("sandbox-args")).map(|r| r.to_string()));
            self.gone(n, t, a, &["usage-limit"], &format!("Login off: {bin} {reason}; {}", crate::runtime::relogin(bin)))
        } else if let Some(steer) = local.then(|| self.continuation(n, t, a)).flatten() {
            self.post(n, steer)
        } else {
            self.gone(n, t, a, &[], text)
        }
    }

    /// #1843: the `steer` that resumes worker attempt `a` after a [transient model failure](docs/design/capacity.md#transient-model-failure)
    /// with its Codex session known; None past [`CONTINUE_MAX`] or when the board log is unread.
    fn continuation(&self, n: u64, t: &Task, a: u64) -> Option<String> {
        if t.state != State::Doing || t.role.as_deref() != Some("worker") { return None; }
        let log = crate::runtime::attempt_log(&self.cfg.root, "worker", n, a);
        // #2198: a Claude Code transcript (the `remote` adapter) shows the background jobs its last turn left running.
        if rt_of(t).is_some_and(|(rt, _)| crate::runtime::of(rt).has("remote-control")) { return self.background_continuation(n, a, &log); }
        if !rt_of(t).is_some_and(|(rt, _)| crate::runtime::of(rt).has("thread-link")) { return None; }
        let failure = crate::runtime::transient_failure(&log, rt_of(t)?.0)?;
        crate::runtime::codex_thread(&log)?;
        let head = core::entry("steer", &[], Some(a), "");
        let prior = crate::board::entries(self.board, n).ok()?.iter().filter(|e| !e.edited && e.body.lines().next() == Some(head.as_str())).count();
        (prior < CONTINUE_MAX).then(|| core::entry("steer", &[], Some(a), &format!(
            "Continue the task from where the last turn stopped; it failed on a transient model error: {}",
            crate::runtime::public_line(&failure, &self.cfg.root, 160))))
    }

    /// #2198: the `steer` that resumes Claude worker attempt `a` whose turn ended with its own
    /// [live background job](docs/design/capacity.md#background-job-at-turn-end); None without one, past
    /// [`BACKGROUND_MAX`] or when the board log is unread.
    fn background_continuation(&self, n: u64, a: u64, log: &std::path::Path) -> Option<String> {
        let jobs = crate::runtime::live_background(log);
        if jobs.is_empty() { return None; }
        let head = core::entry("steer", &[], Some(a), "");
        let prior = crate::board::entries(self.board, n).ok()?.iter()
            .filter(|e| !e.edited && e.body.lines().next() == Some(head.as_str()) && e.body.contains(BACKGROUND_STEER)).count();
        (prior < BACKGROUND_MAX).then(|| core::entry("steer", &[], Some(a), &format!(
            "{BACKGROUND_STEER}: {}. The turn end did not wait for them; finish each in the foreground with a long Bash timeout \
             (600000 ms), never run_in_background or Monitor, then hand in.",
            crate::runtime::public_line(&jobs.join("; "), &self.cfg.root, 160))))
    }

    /// § 4 step 5 (#1950): read each task gone from the open list once; a failed read or write retries next pass.
    /// Returns whether one was reopened. `ponytail:` a close while the attempt's host runs no service is not seen;
    /// § 1 still reads that dependency unknown, never done.
    fn unlisted_tasks(&mut self) -> bool {
        let mut reopened = false;
        for n in std::mem::take(&mut self.unlisted) {
            match self.unlisted_task(n) {
                Ok(r) => reopened |= r,
                Err(e) => {
                    self.log.push(format!("#{n} gone from the open list, unread: {e}"));
                    self.unlisted.insert(n);
                }
            }
        }
        reopened
    }

    /// Closed in `doing` on this host: reopen it, then one `ask` of that attempt with the close as evidence, so the
    /// close never counts as done (desiign#50: a worker closed its own task). Closed otherwise: drop stale `q-*` labels.
    fn unlisted_task(&mut self, n: u64) -> Result<bool, String> {
        let issue = self.board.issue(n)?;
        let t = core::fold(n, &crate::board::entries(self.board, n)?, false);
        match t.attempt.filter(|_| t.state == State::Doing && host_of(&t) == Some(self.cfg.me.as_str())) {
            Some(a) => {
                if !issue.open { self.board.reopen(n)?; }
                let at = issue.closed.map_or_else(|| "an unknown time".to_string(), crate::board::stamp);
                self.post(n, core::entry("ask", &[], Some(a), &closed_card(n, &at)))?;
                Ok(true)
            }
            None if !issue.open => {
                let stale: Vec<String> = issue.labels.iter().filter(|l| l.starts_with("q-")).cloned().collect();
                if !stale.is_empty() { self.board.labels(n, &[], &stale)?; }
                Ok(false)
            }
            None => Ok(false),
        }
    }

    fn post(&mut self, n: u64, body: String) -> Result<u64, String> {
        self.log.push(format!("#{n} {}", body.lines().next().unwrap_or_default()));
        self.cache.remove(&n);
        self.board.comment(n, &body)
    }

    /// #1594 the signals this host holds for one of its executions.
    fn signals(&self, task:u64, x:&core::Execution)->crate::snapshot::Signals<'_> {
        let child=self.kids.iter().find(|k|k.execution==x.id && k.task==task && k.attempt==x.attempt);
        crate::snapshot::Signals {local:true,
            child:child.map(|k|crate::snapshot::Child {unknown:k.unknown.as_deref(),stopping:k.stopping,..Default::default()}),
            held:self.held.iter().any(|l|l.task==task && l.attempt==x.attempt),
            wait:self.execution_waits.get(&task).map(String::as_str),
            input_pending:self.delivered(x.attempt)<x.through,
            ask:self.snapshot.as_ref().is_some_and(|s|s.tasks.iter().any(|r|r.n==task && r.task.state==State::Ask)),
            ..Default::default()}
    }

    fn observe_progress(&mut self) {
        if self.snapshot.is_none() {return;}
        for kid in &self.kids {
            let key=(kid.task,kid.attempt,kid.execution);
            let previous=self.snapshot.as_ref().and_then(|s|s.tasks.iter().find(|r|r.n==kid.task))
                .and_then(|r|r.executions.iter().find(|x|x["id"].as_u64()==Some(kid.execution) && x["attempt"].as_u64()==Some(kid.attempt)))
                .map(|x|&x["progress"]);
            let restored=!self.progress.contains_key(&key) && previous.is_some_and(|p|p["notice_open"]==true);
            if restored {
                if let Some(since)=previous.and_then(|p|p["overdue_since"].as_i64()) {self.stuck.insert(kid.task,since);}
            }
            let entry=self.progress.entry(key).or_default();
            entry.2|=restored;
            let mut value=crate::runtime::observe_until(std::time::Instant::now()+self.progress_budget,|| {
                match kid.proc.progress_source() {
                    Ok((path,start))=>entry.0.read(&path,start,|| (self.progress_clock)(self.board),self.cfg.poll),
                    Err(_)=>crate::runtime::progress::unknown("native execution progress source unavailable"),
                }
            });
            if value["observed"].is_null() {value["observed"]=json!((self.progress_clock)(self.board));}
            let transition=match value["status"].as_str() {
                Some("overdue") if !entry.2=>{entry.2=true;Some("service.progress.overdue")},
                Some("active"|"quiet") if entry.2=>{entry.2=false;Some("service.progress.recovered")},_=>None
            };
            value["notice_open"]=json!(entry.2);
            if entry.2 && value["overdue_since"].is_null() {value["overdue_since"]=entry.1["overdue_since"].clone();}
            entry.1=value;
            if let Some(op)=transition {
                crate::profile::measure_counted(op,|| ((),[("task".into(),kid.task),("attempt".into(),kid.attempt),("execution".into(),kid.execution)].into_iter().collect()));
            }
        }
        self.progress.retain(|key,_|self.kids.iter().any(|k|*key==(k.task,k.attempt,k.execution)));
    }

    fn execution_statuses(&self)->Vec<crate::snapshot::ExecutionStatus> {
        let mut statuses:Vec<_>=self.observations.iter().map(|((task,_),x)| {
            let (state,reason)=crate::snapshot::verdict(&self.signals(*task,x));
            crate::snapshot::ExecutionStatus {progress:self.progress.get(&(*task,x.attempt,x.id)).map(|p|p.1.clone()),task:*task,attempt:x.attempt,id:x.id,through:x.through,state:state.into(),reason}
        }).collect();
        if let Some(snapshot)=&self.snapshot {
            statuses.extend(snapshot.tasks.iter().flat_map(|r|&r.executions).filter(|x|x["host"]==self.cfg.me && x["release"]=="exit")
                .filter_map(|x|serde_json::from_value(x.clone()).ok()));
        }
        statuses
    }

    /// #2247: `selected <version> waits <reason>` while the host runs another version than CoreQ `current` selects:
    /// the live attempts the handover waits for, else the host handover reason, else the pending restart.
    pub fn selected_line(&self)->Option<String> {
        let selected=crate::platform::tool(&self.cfg.install).current().filter(|v|*v!=self.version && crate::service_versions::version(v))?;
        let waits:Vec<String>=crate::service_run::lifecycle_waits(self,false).into_iter().map(|w| {
            match w.strip_prefix('#').and_then(|n|n.parse::<u64>().ok()).and_then(|n|self.kids.iter().find(|k|k.task==n)) {
                Some(k)=>format!("#{} attempt {}",k.task,k.attempt),
                None=>w,
            }
        }).collect();
        let host=self.host.as_ref().and_then(|h|h.lock().ok().and_then(|h|h.handoff_wait())).map(|w|w.reason().to_string());
        Some(format!("selected {selected} waits {}",if !waits.is_empty() {waits.join(", ")} else {host.unwrap_or_else(||"restart pending".into())}))
    }

    pub fn binary_evidence(&self)->Value {
        if let Some((_,value))=self.binary.borrow().as_ref().filter(|(at,_)|at.elapsed().as_secs()<60) {return value.clone();}
        let generation=self.host.as_ref().and_then(|h|h.lock().ok().map(|h|h.instance.clone()));
        let value=crate::service_versions::binary(&self.cfg.install,&self.instance,generation.as_deref());
        *self.binary.borrow_mut()=Some((std::time::Instant::now(),value.clone()));value
    }

    pub fn service_diagnostics(&self)->Value {
        if let Some((at,value))=self.diagnostics.borrow().as_ref().filter(|(at,_)|at.elapsed().as_secs()<60) {let _=at;return value.clone();}
        let selected=crate::platform::tool(&self.cfg.install).current();
        let pending=selected.as_deref().is_some_and(|v|v!=self.version)
            || self.cfg.install.join("restart").exists() || crate::paths::dot(&self.cfg.root).join("restart").exists();
        let waiting=if let Some(host)=&self.host {
            host.lock().map(|h|h.restart&&h.handoff_wait().is_some()).unwrap_or(false)
        } else {pending&&!crate::service_run::lifecycle_waits(self,false).is_empty()};
        let value=crate::service_versions::collect(&self.cfg.install,&self.version,&self.instance,self.started,waiting);
        *self.diagnostics.borrow_mut()=Some((std::time::Instant::now(),value.clone()));
        value
    }

    /// § 3 Runtime accounts: records attempt `a`'s usage limit on the account its log names. Returns the spare account
    /// the next start takes, else the hold: the earliest limit end of an enabled account when there are several.
    fn account_limit(&mut self, n: u64, t: &Task, a: u64, rt: &str, hold: (i64, bool), now: i64) -> (Option<String>, (i64, bool)) {
        if !crate::runtime::CLI_RUNTIMES.contains(&rt) { return (None, hold); }
        let log = crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a);
        let label = crate::runtime::accounts::of_log(&log);
        let (end, reported) = hold;
        let seen = if reported { now } else { end - USAGE_PROBE };
        let mut windows = crate::runtime::quota_windows(&log, rt);
        if windows.is_empty() { windows.push(("unknown".into(),reported.then_some(end))); }
        if let Err(e) = crate::runtime::accounts::record_windows(&self.cfg.install, rt, &label, seen, &windows) {
            self.log.push(format!("account {rt} {label} limit not saved: {e}"));
        }
        let h = crate::runtime::accounts::Host::read(&self.cfg.install);
        match h.spare(rt, now) {
            Some(next) => (Some(next), hold),
            None => (None, h.hold(rt, now).unwrap_or(hold)),
        }
    }

    /// § 3 Runtime accounts, every pass: holds of runtimes with several accounts follow the account record, and each
    /// change of a runtime's active account goes to the service log and telemetry.
    fn account_pass(&mut self, now: i64) {
        if let Err(e)=crate::runtime::accounts::recover_pending(&self.cfg.install) {self.log.push(format!("account quota recovery unknown: {e}"));}
        let h = crate::runtime::accounts::Host::read(&self.cfg.install);
        for rt in h.policy.keys().chain(h.state.keys()).collect::<BTreeSet<_>>() {
            match (h.active(rt, now), h.hold(rt, now)) {
                (None, Some(hold)) => { self.limited.insert(rt.clone(), hold); }
                _ => { self.limited.remove(rt); }
            }
        }
        for s in crate::runtime::accounts::switches(&h, now, &mut self.accounts) {
            self.log.push(s.line());
            crate::profile::measure_counted("account.switch", || ((), s.counts(&self.cfg.me)));
        }
    }

    /// `until <time>` for a reported reset, else `probe <time>`, of a held runtime.
    fn limit_line(&self, rt: &str) -> String {
        self.limited.get(rt).map(|(at, reset)| format!("{} {}", if *reset { "until" } else { "probe" }, crate::board::stamp(*at))).unwrap_or_default()
    }

    /// #2263: one `headroom <rt> <cause>` line per weighted runtime whose load has no headroom fraction.
    fn headroom_lines(&self) -> Vec<String> {
        let Ok(now)=self.board.now() else {return vec![]};
        self.cfg.weights.keys().filter(|rt|self.runtime_load(rt).is_ok_and(|l|l.headroom.is_none()))
            .map(|rt|format!("headroom {rt} {}",crate::runtime::headroom::cause(&self.cfg.install,rt,now))).collect()
    }

    fn heartbeat_body(&self) -> String {
        let mut lines = vec![format!("version {}", self.version),format!("balancing {}",serde_json::to_string(&self.balancing()).unwrap())];
        lines.extend(self.headroom_lines());
        if !self.instance.is_empty() {
            lines.push(format!("service {}", self.instance));
            lines.push(format!("diagnostics {}", self.service_diagnostics()));
            lines.push(format!("binary-evidence {}", self.binary_evidence()));
        }
        lines.extend(self.selected_line());
        if !self.machine.is_empty() {
            lines.push(format!("machine {}", self.machine));
        }
        if !self.link.is_empty() {
            lines.push(self.link.clone());
        }
        if self.focus.is_empty() {
            lines.push(format!("focus {}", if self.focus_error.is_some() { "unknown" } else { "off" }));
        }
        if self.gui().is_ok() {
            lines.push("has gui".into());
        }
        lines.extend(self.limited.keys().map(|rt| format!("limit {rt} {}", self.limit_line(rt))));
        lines.extend(self.logged_out.iter().map(|rt| format!("login {rt} off")));
        lines.extend(self.clis.iter().map(|(bin, v)| format!("cli {bin} {v}")));
        // § 3 Runtime accounts: labels and state only, never an email, path or token
        let (accounts, at) = (crate::runtime::accounts::Host::read(&self.cfg.install), self.board.now().unwrap_or_else(|_| crate::runtime::accounts::now()));
        lines.extend(accounts.heartbeat(at));
        lines.extend(accounts.usage_lines(at));
        lines.extend(tray_fault(&self.cfg.install).map(|code| format!("tray {code}")));
        lines.extend(self.health.as_ref().map(|(_, line)| line.clone()));
        lines.extend(self.answered.as_deref().map(crate::health::answer_line));
        // #1985, #2084: this host's billed requests of its last full hour, in all and per API host: its share of each
        // board user's quota
        lines.extend(self.host.as_ref().and_then(|h| { let h = h.lock().ok()?; Some(crate::requests::heartbeat(h.last_hour.as_ref()?, h.hosts_last_hour.as_ref())) }).unwrap_or_default());
        let kids: BTreeSet<_> = self.kids.iter().map(|k| (k.task, k.attempt)).chain(self.pending.iter().copied()).collect();
        lines.extend(kids.iter().map(|(t, a)| format!("#{t} {a}")));
        lines.extend(self.kids.iter().filter_map(|k| k.session_url.as_ref().map(|u| match u {
            Ok(u) => format!("session #{} {} {u}", k.task, k.attempt),
            Err(cause) => format!("session #{} {} unknown: {cause}", k.task, k.attempt),
        })));
        lines.extend(self.execution_statuses().iter().map(|s|format!("execution {}",serde_json::to_string(s).unwrap())));
        lines.push(format!("queue {}", serde_json::to_string(&self.queues()).unwrap()));
        lines.join("\n")
    }

    /// #2255: this project's row first, named; every other project of the service follows anonymous.
    fn queues(&self) -> crate::host::Queues {
        let mut projects = vec![crate::host::Queue { project: Some(self.cfg.repo.clone()), ..self.queue.clone() }];
        if let Some(h) = self.host.as_ref().and_then(|h| h.lock().ok()) {
            projects.extend(h.queues.iter().filter(|(r, _)| **r != self.cfg.root).map(|(_, q)| q.clone()));
        }
        crate::host::Queues { restart: self.restart_since, projects }
    }

    /// #2194 [health checks](../docs/design/host-service.md#health-checks-2194): token reads, version against the
    /// latest release the host doctor read, report collection and the last pass error, as the heartbeat `doctor` line.
    fn refresh_health(&mut self) {
        let latest = crate::health::cached_latest(&self.cfg.install);
        let items = crate::health::board_items(self.board, &self.cfg.repo, self.cfg.board.name == "gitlab", &self.cfg.root, &self.cfg.install, &latest);
        let at = self.board.now().unwrap_or_else(|_| crate::runtime::accounts::now());
        let mut line = crate::health::line(at, &items);
        // #2246: a reports gap names its cause and reports on the heartbeat
        if let Some(d) = crate::health::reports_detail(&crate::reports::cached(&self.cfg.root)) { line.push_str(&format!("\ndoctor-reports {d}")); }
        self.health = Some((std::time::Instant::now(), line));
    }

    /// #1819: the next pass reads `task`'s comments from the board, not from the cache.
    pub fn reread(&mut self, task: u64) {
        self.cache.remove(&task);
    }

    /// One pass of § 4. Tasks written to in a step are left alone by later steps until the next pass re-reads them.
    #[tracing::instrument(name="service.Service.pass",skip_all)]
    pub fn pass(&mut self) -> Result<(), String> {
        self.full_stop_owned();
        self.ownership_known = false;
        if self.host.is_none() {
            if let Err(e) = crate::profile::prune(&self.cfg.install) { eprintln!("taskq: telemetry unknown: {e}"); }
        }
        // #2194: an installed service checks its board's health every `health::EVERY` seconds for its heartbeat
        if crate::paths::installed(&self.cfg.install) && self.health.as_ref().is_none_or(|(at, _)| at.elapsed().as_secs() >= crate::health::EVERY) {
            self.refresh_health();
        }
        let r = crate::profile::measure("service.pass", || self.pass_inner(NATIVE_PASS_BUDGET));
        if r.is_err() { self.unready(); }
        self.observe_progress();
        let observations=self.execution_statuses();
        if let Some(snapshot) = &mut self.snapshot {
            for row in &mut snapshot.tasks {
                row.executions.retain(|value|value["host"].as_str()!=Some(&self.cfg.me)
                    || observations.iter().any(|s|s.task==row.n && Some(s.id)==value["id"].as_u64()));
                for value in &mut row.executions {
                    if let Some(status)=observations.iter().find(|s|s.task==row.n && Some(s.id)==value["id"].as_u64()
                        && Some(s.through)==value["through"].as_u64()) {
                        value["state"]=json!(status.state);value["reason"]=json!(status.reason);value["progress"]=json!(status.progress);
                        value["host"]=json!(self.cfg.me);value["observed"]=json!(snapshot.observed);
                    }
                    if value["host"].as_str()==Some(&self.cfg.me) && value["attempt"].as_u64()==row.task.attempt {
                        let live_pid = self.kids.iter().find(|k| k.task == row.n && Some(k.attempt) == row.task.attempt && k.proc.id() > 1 && crate::runtime::alive(k.proc.id()))
                            .map(|k| k.proc.id())
                            .or_else(|| crate::telemetry::live_worker_pid(&self.cfg.install, Some(&self.cfg.root), row.n, row.task.attempt.unwrap_or(0), value["id"].as_u64().unwrap_or(0)));
                        if let Some(pid) = live_pid {
                            value["process_id"] = json!(pid);
                            if value["state"] == "unknown" {
                                value["state"] = json!("running");
                                value["reason"] = json!("");
                            }
                        }
                    }
                }
                if row.executions.iter().any(|x|x["host"].as_str()==Some(&self.cfg.me)) {
                    row.execution_wait=row.executions.iter().find(|x|x["state"]!="running" || x["attempt"].as_u64()!=row.task.attempt)
                        .map(|x|format!("execution {} on {}: {}: {}",x["id"],self.cfg.me,
                            x["state"].as_str().unwrap_or("unknown"),x["reason"].as_str().unwrap_or("observation unavailable")));
                }
                let live_local = self.kids.iter().any(|k| k.task == row.n && row.task.attempt == Some(k.attempt))
                    || row.task.attempt.is_some_and(|a| crate::telemetry::live_worker_pid(&self.cfg.install, Some(&self.cfg.root), row.n, a, a).is_some());
                if !live_local {
                    if let Some(reason)=self.execution_waits.get(&row.n).or_else(|| self.waits.get(&row.n)) {row.execution_wait=Some(reason.clone());}
                }
                if let Some(x)=row.executions.iter().find(|x|x["attempt"].as_u64()==row.task.attempt) {row.heartbeat["progress"]=x["progress"].clone();}
                let now=observations.iter().filter(|s|s.task==row.n && self.progress.contains_key(&(s.task,s.attempt,s.id)))
                    .filter_map(|s|s.progress.as_ref()?.get("observed")?.as_i64()).max().unwrap_or(snapshot.observed);
                row.heartbeat["executor"]=crate::snapshot::executor(&row.task,&row.executions,row.heartbeat["input_pending"]==true,now,self.cfg.ttl);
                crate::snapshot::refresh_heartbeat(&mut row.heartbeat,now,matches!(row.task.state,State::Doing|State::Review)&&row.task.attempt.is_some());
            }
        }
        if let Some(now)=self.snapshot.as_ref().map(|s|s.observed) {self.log_stuck(now);}
        r
    }

    /// #1386: a boot strictly later than its intent proves every local process of an execution ended. A failed boot read
    /// or a missing intent time writes nothing; the execution stays in `execution_waits`. True when a release was written.
    fn release_before_boot(&mut self, views: &[View]) -> bool {
        let mut wrote = false;
        let mut boot = None;
        for v in views {
            for x in &v.projection.executions {
                if x.host != self.cfg.me || x.release.is_some() { continue; }
                if self.kids.iter().any(|k| k.execution == x.id) { continue; }
                let Some(intent) = v.comments.iter().find(|c| c.id == x.id && c.trusted).map(|c| c.created).filter(|t| *t > 0) else { continue };
                let Ok(at) = *boot.get_or_insert_with(self.boot) else { continue };
                let ("ended", proof) = crate::snapshot::verdict(&crate::snapshot::Signals { local: true, boot: Some((at, intent)), ..Default::default() }) else { continue };
                match release_execution_text(self.board, v.issue.number, x.attempt, x.id, "exit", &proof) {
                    Ok(()) => {
                        wrote = true;
                        self.retired(v.issue.number, x.attempt, x.id, false, 0);
                        self.log.push(format!("#{} released {} exit: {proof}", v.issue.number, x.id));
                    }
                    Err(e) => self.log.push(format!("#{} boot release: {e}", v.issue.number)),
                }
            }
        }
        wrote
    }

    fn pass_inner(&mut self, native_budget:std::time::Duration) -> Result<(), String> {
        for (task, attempt, execution) in self.pending_releases.clone() {
            release_execution(self.board, task, attempt, execution, "no-start")?;
            self.retired(task,attempt,execution,false,0);
        }
        let previous_fixes = std::mem::take(&mut self.label_fixes);
        let me = self.cfg.me.clone();
        let ttl = self.cfg.ttl;
        let (mut views, mut hosts, request) = views_asked(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?;
        // #2194 A2: a new `taskq doctor --hosts` request runs the health checks now; this pass's heartbeat answers it
        if let Some(id) = request.filter(|id| self.answered.as_ref() != Some(id)) {
            self.refresh_health();
            self.answered = Some(id);
        }
        // §5: an interrupted owner reroute remains fenced and finishes without a second owner request.
        let mut rerouted=false;
        for v in &views {
            if v.projection.reroute.is_some() {
                let receipt=crate::dispatch::finish_reroute(self.board,v.issue.number);
                if receipt.ok() {self.cache.remove(&v.issue.number);rerouted=true;}
                else {self.log.push(receipt.text());}
            }
        }
        if rerouted {(views,hosts)=self::views(self.board,&mut self.cache,&mut self.deps,self.cfg.ttl)?;}
        if self.release_before_boot(&views) { (views, hosts) = self::views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?; }
        // § 4 step 5 (#1950): a task reopened here is listed again before its live attempt could read as gone below
        let open: HashSet<u64> = views.iter().map(|v| v.issue.number).collect();
        self.unlisted.extend(self.listed.difference(&open).copied());
        self.listed = open;
        if self.unlisted_tasks() {
            (views, hosts) = self::views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?;
            self.listed = views.iter().map(|v| v.issue.number).collect();
        }
        self.dep_closed = newest_dep_close(&views, &self.deps);
        let now = self.board.now()?;
        let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
        // § 3 Runtime accounts: a runtime with several accounts takes its hold from the account record (`account_pass`)
        for x in views.iter().flat_map(|v| &v.projection.executions).filter(|x| x.host == me && !accounts.multi(&x.runtime)) {
            if let Some(hold) = x.usage_hold.filter(|(at, _)| now < *at) {
                self.limited.entry(x.runtime.clone()).and_modify(|old| { if hold.0 > old.0 { *old = hold; } }).or_insert(hold);
            }
        }
        self.account_pass(now);
        self.limited.retain(|_, (at, _)| now < *at);
        self.observe_ownership(&views)?;
        self.snapshot = Some(crate::snapshot::from_views("host", self.board, &self.cfg, now, &views, &hosts));
        self.copy_images();
        self.tasks_read = Some(std::time::Instant::now());
        self.log_churn();
        // Local eligibility only: another host's heartbeat cannot qualify this runtime or hide its refusal.
        self.waits.clear();
        for v in &views {
            let t = &v.projection.task;
            if !matches!(t.state, State::Ready | State::Doing | State::Review)
                || label(&v.issue, "host-").is_some_and(|h| h != me)
                || t.attempt.is_some() && host_of(t) != Some(me.as_str())
                || self.kids.iter().any(|k| k.task == v.issue.number) { continue; }
            let rt = rt_of(t).map(|(rt, _)| rt.to_string()).unwrap_or_else(|| self.select_runtime(v).map(|(rt, _)| rt)
                .unwrap_or_else(|_| route(&v.issue, t.state == State::Review, &self.cfg.default_runtime)));
            if let Some(reason) = self.gui_reason(&v.issue, Some(&rt)) {
                self.waits.insert(v.issue.number, reason);
            }
        }
        // § 4 one service per host name: another install's fresh heartbeat under this name holds every write (#933)
        let title = format!("taskq host {me}");
        let other = hosts.iter().filter(|h| h.title == title && now - h.updated < ttl).flat_map(|h| h.body.lines()).find_map(|l| l.strip_prefix("service ")).filter(|id| *id != self.instance);
        if let Some(id) = other {
            return Err(format!("another service {id} runs as host {me}; stop it or set TASKQ_HOST"));
        }
        self.events(&views, now)?;
        if let Some(b)=hosts.iter().find(|h|h.trusted && host_name(h)==Some(self.cfg.me.as_str())).and_then(|h|Balancing::read(&h.body)) {
            self.last_spawn=self.last_spawn.max(b.last_spawn);
        }
        let by_n: HashMap<u64, &View> = views.iter().map(|v| (v.issue.number, v)).collect();
        let mut touched = HashSet::new();
        let mut released = HashSet::new();
        self.execution_waits.clear();
        // #1886: a lost spawn no longer the live attempt (requeued, parked) waits on nothing; any run of its host, a
        // retired one too, releases it no-start so another host can claim the task. A reached start (attempt log or a
        // hand-in of that attempt) stays unknown (R12).
        let stale_spawns: Vec<_> = views.iter().flat_map(|v| {
            let (n, t) = (v.issue.number, &v.projection.task);
            v.projection.executions.iter().filter(move |x| t.attempt != Some(x.attempt) || !matches!(t.state, State::Doing | State::Review))
                .filter(move |x| !core::handed_in(n, &v.log, x.attempt, x.attempt)).map(move |x| (n, x))
        }).filter(|(n, x)| self.lost_spawn(*n, x)).map(|(n, x)| (n, x.attempt)).collect();
        for (n, a) in stale_spawns {
            release_execution(self.board, n, a, a, "no-start")?;
            self.retired(n, a, a, false, 0);
            released.insert(a);
            self.log.push(format!("#{n} lost spawn {a}: no attempt log on {me} and no longer the live attempt; released no-start"));
        }
        let native_until=std::time::Instant::now()+native_budget;
        // Cold scans leave time for already owned work and control.
        let recovery_until=std::time::Instant::now()+native_budget/4;
        let pivot=self.recovery_after.and_then(|n|views.iter().position(|v|v.issue.number==n)).map_or(0,|i|i+1);
        for v in views[pivot..].iter().chain(&views[..pivot]) {
            let task = v.issue.number;
            let unresolved: Vec<_> = v.projection.executions.iter().filter(|x| x.release.is_none()).collect();
            if self.kids.iter().any(|k| k.task == task) || self.held.iter().any(|l| l.task == task) { continue; }
            let Some(x) = unresolved.iter().find(|x| x.host == me && { let a = crate::runtime::of(&x.runtime); a.has("remote-control") || a.has("app-turn") }) else { continue };
            if unresolved.len() != 1 {
                self.execution_waits.insert(task, "multiple unresolved executions; verify native ownership before recovery".into());
                continue;
            }
            // A definite trust no-start keeps its pending input/attempt for service setup repair (R9).
            if let Some(refusal) = crate::runtime::refused_launch(&self.cfg.root, task, x) {
                let t = &v.projection.task;
                if refusal.contains("Workspace not trusted") && t.attempt == Some(x.attempt) && matches!(t.state, State::Doing | State::Review) {
                    self.retry_trust(v, x)?;
                } else {
                    release_execution(self.board, task, x.attempt, x.id, "no-start")?;
                    self.retired(task, x.attempt, x.id, false, 0);
                    if t.attempt == Some(x.attempt) && matches!(t.state, State::Doing | State::Review) {
                        self.gone(task, t, x.attempt, &[], &format!("start failed: Claude background launch refused: {refusal}"))?;
                    }
                }
                continue;
            }
            // #1879: nothing native to recover; the resume step releases a lost spawn
            if self.lost_spawn(task, x) { continue; }
            let Some(recover) = &mut self.recover else { continue };
            if std::time::Instant::now()>=recovery_until {
                self.execution_waits.insert(task,format!("execution {}: native observation deferred; recovery budget exhausted",x.id));
                continue;
            }
            self.recovery_after=Some(task);
            let result = crate::runtime::observe_until(recovery_until,||recover(task,x))
                .and_then(|proc| self.adopt(Kid { unknown:None, stopping:false, session_url:None, task, attempt: x.attempt, execution: x.id, rt: x.runtime.clone(), proc }));
            if let Err(e) = result {
                self.execution_waits.insert(task, format!("execution {} on {}: recovery unknown: {e}", x.id, x.host));
            }
        }

        self.hold = crate::admission::hold(&self.cfg.install);
        let mut pending_acceptance = HashSet::new();
        // 1. Observe native state once; retry lost evidence writes without resubmitting input.
        let mut exited = vec![];
        let mut retired = vec![];
        let needs_stop=self.kids.iter().any(|k|!by_n.get(&k.task).is_some_and(|v|v.projection.task.attempt==Some(k.attempt)));
        let observation_until=if needs_stop {std::time::Instant::now()+native_until.saturating_duration_since(std::time::Instant::now())/2} else {native_until};
        let mut deferred=None;
        self.headroom_error=None;
        if !self.cfg.weights.is_empty() {
            for k in &self.kids {
                if let Some(v)=by_n.get(&k.task) {
                    let log=crate::runtime::attempt_log(&self.cfg.root,v.projection.task.role.as_deref().unwrap_or_default(),k.task,k.attempt);
                    if let Err(e)=crate::runtime::headroom::observe(&self.cfg.install,&k.rt,&log,now) {self.headroom_error=Some(e);}
                }
            }
        }
        let quota:Vec<_> = self.kids.iter().filter_map(|k| {
            let log=crate::runtime::attempt_log(&self.cfg.root,by_n.get(&k.task)?.projection.task.role.as_deref().unwrap_or_default(),k.task,k.attempt);
            let windows=crate::runtime::quota_windows(&log,&k.rt);
            (!windows.is_empty()).then(||(k.rt.clone(),crate::runtime::accounts::of_log(&log),windows))
        }).collect();
        for (rt,label,windows) in quota {
            if let Err(e)=crate::runtime::accounts::record_windows(&self.cfg.install,&rt,&label,now,&windows) {self.log.push(format!("account {rt} {label} quota unknown: {e}"));}
        }
        self.kids.retain_mut(|k| {
            if std::time::Instant::now()>=observation_until {
                deferred.get_or_insert(k.execution);
                k.unknown=Some("native observation deferred; pass budget exhausted".into());return true;
            }
            // #2221: a hand-in of this attempt after the execution began is board evidence it took its input; the
            // handle learns it before observation so recovery and exit can use it
            let handed = by_n.get(&k.task).is_some_and(|v| core::handed_in(k.task, &v.log, k.attempt, k.execution));
            if handed { k.proc.accept(); }
            let status = crate::runtime::observe_until(observation_until,||k.proc.exited());
            let role_code = self.begun.get(&k.attempt).map_or("work", |(r, _, _)| if r == "reviewer" { "review" } else { "work" });
            record_known_pid(&self.cfg, &mut self.unknown_pids, k, role_code);
            if handed {
                crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "result", k.task, k.attempt, k.proc.id(), "ok", "handed-in");
                if !matches!(status, Ok(Some(_))) && (k.proc.id() == 1 || (k.proc.id() > 1 && crate::runtime::alive(k.proc.id()))) {
                    crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "alive-after-result", k.task, k.attempt, k.proc.id(), "failed", "pid-alive-after-result");
                    self.log.push(format!("#{} worker pid {} alive after result; stopping", k.task, k.proc.id()));
                }
            }
            let confirmed = by_n.get(&k.task).and_then(|v| v.projection.executions.iter().find(|x| x.id == k.execution && x.accepted));
            let accepted = if let Some(x) = confirmed { Ok(Some(x.through)) } else {
                crate::runtime::observe_until(observation_until,||k.proc.accepted()).and_then(|yes| if yes {
                    accept_execution(self.board, k.task, k.attempt, k.execution, None).map(Some).inspect_err(|_| { pending_acceptance.insert(k.attempt); })
                } else { Ok(None) })
            };
            if let Ok(Some(through)) = accepted { self.delivered.entry(k.attempt).and_modify(|v| *v = (*v).max(through)).or_insert(through); self.cache.remove(&k.task); }
            // #1594: an exit counts once input acceptance is resolved; failed reads, then a cancellation's earlier failure, stay unknown
            let unknown = status.as_ref().err().or(accepted.as_ref().err()).or(k.unknown.as_ref().filter(|_| k.stopping)).cloned();
            let child = crate::snapshot::Child { exited: matches!(status, Ok(Some(_))) && !pending_acceptance.contains(&k.attempt), unknown: unknown.as_deref(), stopping: k.stopping };
            match (crate::snapshot::verdict(&crate::snapshot::Signals { local: true, child: Some(child), ..Default::default() }), status) {
            (("ended", _), Ok(Some(code))) => {
                let hold = by_n.get(&k.task).and_then(|v| {
                    let log = crate::runtime::attempt_log(&self.cfg.root, v.projection.task.role.as_deref().unwrap_or_default(), k.task, k.attempt);
                    crate::runtime::usage_limit(&log, &k.rt)
                }).map(|reset| reset.filter(|r| *r > now).map_or((now + USAGE_PROBE, false), |r| (r, true)));
                // #2272: a lost resumed input names its cause in the release and in the `gone` that requeues the task
                let lost = k.proc.lost_input();
                let evidence = hold.map_or_else(|| lost.clone().unwrap_or_default(), |(at, reported)| format!("usage-limit {} {at}", if reported { "until" } else { "probe" }));
                let no_start=match k.release(self.board,code,&evidence) {Ok(no_start)=>no_start,Err(e)=>{k.unknown=Some(e);return true;}};
                let exit_reason = k.proc.exit_error().map_or_else(|| format!("exit-{code}"), String::from);
                crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "exit", k.task, k.attempt, k.proc.id(), if code == 0 { "ok" } else { "failed" }, &exit_reason);
                retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).has("cli"),k.proc.id()));
                released.insert(k.execution);
                // This pass's view predates release; it cannot authorize a continuation before acceptance is resolved.
                if by_n.get(&k.task).is_some_and(|v|v.projection.task.attempt==Some(k.attempt))
                    && (no_start || !matches!(accepted,Ok(Some(_)))) {touched.insert(k.task);}
                if !no_start {exited.push((k.task, k.attempt, code, k.proc.exit_error(), lost.map(|l| (k.execution, l))));}
                false
            }
            (("unknown", e), _) => { k.unknown=Some(e); true }
            _ => { k.unknown=None; true }
            }
        });
        for (task,attempt,execution,native,pid) in retired.drain(..) { self.retired(task,attempt,execution,native,pid); }
        for (n, a, code, failure, lost) in exited {
            self.ended(n, a, by_n.get(&n).copied(), failure);
            if code == 0 && let Some(v) = by_n.get(&n) && let Some((rt,_)) = rt_of(&v.projection.task) {
                let log = crate::runtime::attempt_log(&self.cfg.root,v.projection.task.role.as_deref().unwrap_or_default(),n,a);
                if crate::runtime::quota_windows(&log,rt).is_empty() && let Err(e) = crate::runtime::accounts::confirm_probe(&self.cfg.install,rt,&crate::runtime::accounts::of_log(&log),&log) {
                    self.log.push(format!("account {rt} recovery unknown: {e}"));
                }
            }
            if let Some(v) = by_n.get(&n)
                && v.projection.task.attempt == Some(a)
                && matches!(v.projection.task.state, State::Doing | State::Review)
                // #2272: a lost resumed input counts only hand-ins after its own intent, not the earlier `ask` it answers
                && !core::handed_in(n, &v.log, a, lost.as_ref().map_or(a.max(self.delivered(a)), |(x, _)| *x))
            {
                let t = &v.projection.task;
                // #2272: and is requeued as `gone`, never continued with the same resume
                if let Some((_, lost)) = lost { self.gone(n, t, a, &[], &format!("{lost}{}", self.last_message(n, a, t)))?; }
                else { self.exit_outcome(n, t, a, None, now, &format!("exit {code}{}", self.last_message(n, a, t)))?; }
                touched.insert(n);
            }
        }

        // A replaced, once-accepted attempt is ordinary cancellation, not a double claim.
        for k in &self.kids {
            if let Some(v) = by_n.get(&k.task) {
                let history: Vec<_> = v.log.iter().filter(|e| e.id <= k.attempt).cloned().collect();
                if core::fold(k.task, &history, false).attempt != Some(k.attempt)
                {
                    crate::incident::detect("invariant.double_attempt", concat!("src/service.rs:", line!()), &k.rt, &[], &["claim:rejected"]);
                }
            }
        }

        // 2. stop children of replaced attempts (R11)
        let mut stopped = vec![];
        self.kids.retain_mut(|k| {
            let current = by_n.get(&k.task).is_some_and(|v| v.projection.task.attempt == Some(k.attempt));
            if !current {
                k.stopping=true;
                if std::time::Instant::now()>=native_until {
                    deferred.get_or_insert(k.execution);
                    k.unknown=Some("native observation deferred; cancellation pending".into());return true;
                }
                // #2285: a closed or replaced attempt whose exact execution the board already released by Exit needs
                // only native exit (runtime-qualification.md:465); a live or unreadable process stays a named wait
                let exit=match by_n.get(&k.task) {
                    Some(v)=>Ok(v.projection.executions.clone()),
                    None=>crate::board::entries(self.board,k.task).map(|l|core::project(k.task,&l,false).executions),
                };
                let exit=exit.map(|x|x.into_iter().any(|x|x.id==k.execution && x.attempt==k.attempt && x.host==self.cfg.me
                    && x.runtime==k.rt && x.release==Some(core::Release::Exit)));
                if let Ok(true)=exit {
                    k.proc.released();
                    match crate::runtime::observe_until(native_until,||k.proc.exited()) {
                        Ok(Some(_))=>{
                            retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).has("cli"),k.proc.id()));
                            released.insert(k.execution);stopped.push((k.task,k.attempt));
                            return false;
                        }
                        Ok(None)=>{k.unknown=Some(format!("execution {} released by Exit on the board, native process still running",k.execution));return true;}
                        Err(e)=>{k.unknown=Some(format!("execution {} released by Exit on the board, native exit unconfirmed: {e}",k.execution));return true;}
                    }
                }
                let handed = by_n.get(&k.task).is_some_and(|v| core::handed_in(k.task, &v.log, k.attempt, k.execution));
                let role_code = self.begun.get(&k.attempt).map_or("work", |(r, _, _)| if r == "reviewer" { "review" } else { "work" });
                let pid_before = k.proc.id();
                let stop=crate::runtime::observe_until(native_until,||crate::runtime::stop(k.proc.as_mut()));
                record_known_pid(&self.cfg, &mut self.unknown_pids, k, role_code);
                let pid_before = pid_before.max(k.proc.id());
                let kill_reason = if handed { "pid-alive-after-result" } else { "attempt-replaced" };
                crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "kill", k.task, k.attempt, pid_before, if stop.is_ok() { "ok" } else { "failed" }, kill_reason);
                let child=crate::snapshot::Child {exited:stop.is_ok(),unknown:stop.as_ref().err().map(String::as_str),stopping:true};
                match crate::snapshot::verdict(&crate::snapshot::Signals {local:true,child:Some(child),..Default::default()}) {
                    _ if stop.is_ok() && pending_acceptance.contains(&k.attempt) => return true,
                    ("ended",_) => match k.release(self.board,stop.unwrap(),"") {
                        Ok(_)=>{
                            crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "exit", k.task, k.attempt, pid_before, "ok", kill_reason);
                            retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).has("cli"),k.proc.id()));
                            released.insert(k.execution);stopped.push((k.task,k.attempt));
                        },
                        Err(e)=>{k.unknown=Some(e);return true;}
                    },
                    (_,e) => { k.unknown=Some(e); return true; }
                }
            }
            current
        });
        for (task,attempt,execution,native,pid) in retired { self.retired(task,attempt,execution,native,pid); }
        for (n, a) in stopped {
            self.ended(n, a, by_n.get(&n).copied(), None);
        }
        self.held.retain(|l| by_n.get(&l.task).is_some_and(|v| v.projection.task.attempt == Some(l.attempt)));
        // #2261: the session link belongs to this exact live execution's native session and job, read by its runtime adapter;
        // until a link appears the heartbeat names the adapter's cause.
        // shortcut: bounded native reads restart each pass until a link appears; retain a cursor if costly.
        for k in self.kids.iter_mut().filter(|k| std::time::Instant::now()<native_until && crate::runtime::of(&k.rt).has("remote-control") && !matches!(k.session_url, Some(Ok(_)))) {
            let (Ok(adapter), Some((spec, job))) = (crate::runtime::agent(&k.rt), k.proc.session_spec()) else { continue };
            k.session_url = Some(crate::runtime::observe_until(native_until, || adapter.session_link(&spec, job.as_deref()))
                .map_err(|(cause, detail)| crate::runtime::absence(cause.0, detail.as_deref())));
        }
        if let Some(first)=deferred.and_then(|id|self.kids.iter().position(|k|k.execution==id)) {self.kids.rotate_left(first);}

        // 4. Expired heartbeat is not evidence that an external executor stopped.
        // A lease newer than the accepted attempt is a spawn that lost the claim and still runs. An older one is a
        // replaced attempt its host lists until its next pass: normal lag, not a second live attempt (#1140).
        for hb in hosts.iter().filter(|h| now - h.updated <= ttl) {
            for line in hb.body.lines() {
                let Some((n, a)) = line.strip_prefix('#').and_then(|l| l.split_once(' ')) else { continue };
                let (Ok(n), Ok(a)) = (n.parse::<u64>(), a.parse::<u64>()) else { continue };
                if by_n.get(&n).and_then(|v| v.projection.task.attempt).is_some_and(|c| a > c) {
                    crate::incident::detect("invariant.double_attempt", concat!("src/service.rs:", line!()), "none", &[], &["heartbeat:conflict"]);
                }
            }
        }
        let mut unresolved_local = BTreeSet::new();
        for ((task,_),x) in &self.observations {
            let reservation=(*task,x.attempt,x.runtime.clone());
            unresolved_local.insert(reservation.clone());
            if !self.kids.iter().any(|k|k.task==*task && k.execution==x.id) && !self.reserved.contains(&reservation) {
                record_unresolved_hold(&self.cfg, *task, x.attempt, x.id, &x.runtime);
                self.reserved.push(reservation);
            }
        }
        let held_slots = self.entry().held;
        let held_rts: BTreeSet<String> = held_slots.iter().map(|(_, _, r)| r.clone()).collect();
        for rt in held_rts {
            let running_count = held_slots.iter().filter(|(_, _, r)| r == &rt).count();
            // R12: a child whose PID is not read yet is unknown, never a slot without a process (#2380)
            let alive_count = self.kids.iter().filter(|k| k.rt == rt && (k.proc.id() <= 1 || crate::runtime::alive(k.proc.id()))).count();
            if running_count > 0 && alive_count == 0 {
                for (task, attempt, _) in held_slots.iter().filter(|(_, _, r)| r == &rt) {
                    let dead_pid = self.kids.iter().find(|k| k.task == *task && k.attempt == *attempt).map_or(0, |k| k.proc.id());
                    crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &rt, "work", "running-no-process", *task, *attempt, dead_pid, "failed", &format!("running-{running_count}-alive-0"));
                    self.log.push(format!("{me} {rt} running {running_count}, none alive (#{task} attempt {attempt})"));
                }
            }
        }
        for v in &views {
            let (n,t) = (v.issue.number,&v.projection.task);
            if let Some(a) = t.attempt.filter(|_| host_of(t) != Some(&me) && matches!(t.state,State::Doing|State::Review)) {
                let delivered = v.projection.delivered(a).unwrap_or(0);
                if t.inbox.iter().all(|id| *id <= delivered) {
                    let executions = &v.projection.executions;
                    if let Some(x) = executions.iter().rev().find(|x| x.attempt == a).filter(|_| executions.iter().all(|x| x.release.is_some())) {
                        if !core::handed_in(n,&v.log,a,x.through) {
                            self.exit_outcome(n,t,a,x.usage_hold,now,&format!("execution {} confirmed {:?} without task outcome",x.id,x.release.unwrap()))?;
                            touched.insert(n);
                        }
                    }
                }
            }
            // #1491: the current attempt that its owner's fresh heartbeat lists as running is observed, not a wait
            let remote_running = |x: &core::Execution| x.host != me && Some(x.attempt) == v.projection.task.attempt
                && crate::snapshot::execution_status(v.issue.number, x, &hosts, now, ttl)["state"] == "running";
            let unresolved: Vec<_> = v.projection.executions.iter()
                .filter(|x| x.release.is_none() && !released.contains(&x.id) && !remote_running(x)).collect();
            if let Some(x) = unresolved.first().filter(|x| !self.held.iter().any(|l| l.execution.unwrap_or(l.attempt) == x.id)) {
                // #1886: name the host step that can resolve it
                let step = release_step(&x.host, &me);
                self.execution_waits.entry(v.issue.number).or_insert_with(|| format!("execution {} on {}: termination unconfirmed; {step}", x.id, x.host));
            } else if let Some(x) = crate::snapshot::input_recovery(v) {
                self.execution_waits.insert(v.issue.number, format!("execution {} on {}: agent exited; {}", x.id, x.host,crate::runtime::acceptance_action(&x.runtime)));
            } else if let (Some(a),Some(owner)) = (v.projection.task.attempt,host_of(&v.projection.task)) {
                if owner != me && matches!(v.projection.task.state,State::Doing|State::Review) && v.projection.task.inbox.iter().any(|id| *id > v.projection.delivered(a).unwrap_or(0)) {
                    self.execution_waits.insert(v.issue.number,format!("input pending on {owner}; await or restore the owning service"));
                }
            }
        }

        // § 3 Runtime accounts: only the project the connection approved reads fleet policy for this host
        if self.fleet.due(now) && crate::enrollment::approves(&self.cfg.install, self.cfg.board.name, &self.cfg.web_host, &self.cfg.repo) {
            if let Err(e) = self.fleet.refresh(self.board, &self.cfg.install, &hosts, &me, now) { self.log.push(format!("account policy: {e}")); }
        }

        // 3. heartbeat
        let body = self.heartbeat_body();
        self.host_issue = hosts.iter().find(|h| h.title == title).map(|h| h.number);
        match hosts.iter().find(|h| h.title == title) {
            // execution rows refresh every 60 s, read within ttl (snapshot.rs); without them `renewal` keeps every lease fresh (#1358)
            Some(h) if stable(&h.body) == stable(&body)
                && now - h.updated < if body.lines().any(|l| l.starts_with("execution ")) { 60.min(ttl / 3) } else { renewal(&body, ttl) } => {}
            // a same-body edit leaves `updated_at` as is (GitHub), so the stamp keeps the heartbeat fresh
            Some(h) => {
                self.board.edit(h.number, &format!("{body}\nat {now}"))?;
                self.record_heartbeat_events();
                // #1729 A3: the snapshot read this heartbeat before the write; after a wake it kept the pre-sleep age
                // until the next read, so this host's row takes the board time of the write it just made
                if let Some(s) = self.snapshot.as_mut() {
                    let observed = s.observed;
                    if let Some(row) = s.hosts.iter_mut().find(|r| r.name == me && r.updated < now) {
                        row.updated = now;
                        row.diagnostics = crate::service_versions::read(&body, now, observed, ttl as i64, false);
                    }
                }
            }
            None => {
                self.host_issue = Some(self.board.create(&title, &body, &[HOST_LABEL.to_string()])?);
                self.record_heartbeat_events();
            }
        }
        // § 4 renamed host: another open heartbeat of this service is a retired name (#1483)
        for h in hosts.iter().filter(|h| h.title != title && !self.instance.is_empty() && h.body.lines().any(|l| l.strip_prefix("service ") == Some(&self.instance))) {
            self.board.comment(h.number, &format!("Retired: this service now runs as host {me}."))?;
            self.board.close(h.number)?;
        }

        let open: HashSet<u64> = views.iter().map(|v| v.issue.number).collect();
        crate::profile::repair_ended(&self.cfg.install, &self.cfg.root, &self.cfg.repo, &me, self.board, &open, &mut self.unowned);
        self.tidy(&by_n);

        // 5. projected labels
        let written = touched.clone();
        for v in &views {
            let n = v.issue.number;
            if touched.contains(&n) {
                continue;
            }
            // § 2 timed park: at its board time the service requeues it; one op id per park, so hosts write it once
            if v.projection.task.state == State::Later && let Some(until) = v.projection.task.until.as_deref().filter(|u| crate::board::iso(u).is_some_and(|at| at <= now)) {
                let rev = v.projection.revision;
                let op = crate::dispatch::Op { id: Some(format!("wake-{rev}")), revision: Some(rev) };
                let r = crate::dispatch::act(self.board, &self.cfg.root, n, "requeue", &[], &format!("Reason: Timed park ended at {until}."), &op);
                if !r.ok() { log!("#{n} timed park wake: {}", r.reason); }
                touched.insert(n);
                continue;
            }
            // § 1: a person who adds `taskq` to a held issue starts it; the next pass folds `ready`
            if is_task(&v.issue) && v.projection.task.state == State::Held {
                self.board.comment(n, "taskq: start")?;
                touched.insert(n);
                continue;
            }
            // Keep the failed start discoverable until its explicit retry repairs eligibility.
            if !is_task(&v.issue) && start_logged(n, &v.log, &v.projection) { continue; }
            let want = self.board.state_label(v.projection.task.state.name())?;
            let mut stale: Vec<_> = v.issue.labels.iter().filter(|l| l.starts_with("q-") && **l != want).cloned().collect();
            stale.sort();
            let fix = !stale.is_empty() || !v.issue.labels.contains(&want);
            if !fix || v.projection.task.state == State::Closed {
                project(self.board, &v.issue, &v.projection.task)?;
            } else {
                // Adapter success includes assignment readback; one hand-moved label is expected. The same fix again on
                // the next pass means another writer folds differently (#1550: N-1 host): back off, log and report once.
                // ponytail: retry every 60 passes so a fixed peer is caught up; a peer-written-label signal if that lags
                let (count, mut fixed_at, mut reported) = previous_fixes.get(&n).filter(|(w, old, ..)| w == &want && old == &stale)
                    .map_or((1, 0, false), |(_, _, count, at, reported)| (count.saturating_add(1), *at, *reported));
                // #1873: a revert is a later write without the wanted label. A list read from before the fix (reused or
                // lagging) or a peer still moving the same way (its comment lands seconds before its labels) is no loop.
                let reverted = count > 1 && v.issue.updated > fixed_at && !v.issue.labels.contains(&want);
                if count % 60 == 1 {
                    project(self.board, &v.issue, &v.projection.task)?;
                    fixed_at = self.board.now()?;
                } else if reverted && !reported {
                    log!("#{n} label {want} reverted by another writer after this host's fix; backing off (§ 2)");
                    crate::incident::detect("invariant.label_loop", concat!("src/service.rs:", line!()), "none", &[v.projection.task.state.name()], &["passes:2"]);
                    reported = true;
                }
                self.label_fixes.insert(n, (want, stale, count, fixed_at, reported));
            }
            if v.projection.task.state == State::Closed {
                // § 2 Deploy: a close typed on the board gets its waiting label like a written one
                if let Err(e) = crate::deploy::label(self.board, &v.issue, &v.log, crate::deploy::config(&self.cfg.root).as_ref()) { log!("#{n} {} label not written: {e}", crate::deploy::LABEL); }
                touched.insert(n);
            }
        }
        // § 2 Summary: rewrite a stale one from the log like a label
        let previous = std::mem::take(&mut self.summary_fixes);
        let deploy = crate::deploy::config(&self.cfg.root);
        // the label step marks a closed task touched without writing to it
        let skip = |v: &&View| touched.contains(&v.issue.number) && (v.projection.task.state != State::Closed || written.contains(&v.issue.number));
        for v in views.iter().filter(|v| self.board.summaries() && !skip(v)) {
            let n = v.issue.number;
            let have = crate::summary::find(&v.comments);
            // § 2: a missing one is created at a state change (an older task's next write) or when this service saw it
            let moved = self.seen.get(&n).is_some_and(|s| *s != v.projection.task.state);
            if have.is_none() && !moved && !self.summarized.contains(&n) { continue; }
            self.summarized.insert(n);
            let want = crate::summary::text(self.board, n, &v.issue.body, &v.log, &v.projection.task, deploy.as_ref());
            if have.is_some_and(|c| crate::summary::same(&c.body, &want)) { continue; }
            // a missing one is always written; a peer that renders another text is edited back once per 60 passes
            let streak = if have.is_none() { 1 } else { previous.get(&n).map_or(1, |s| s.saturating_add(1)) };
            if streak % 60 == 1 && let Err(e) = crate::summary::put(self.board, n, &v.comments, &want) {
                log!("#{n} summary not written: {e}");
            }
            self.summary_fixes.insert(n, streak);
        }

        for v in &views {
            let n = v.issue.number;
            if v.projection.task.publication.is_some() { continue; }
            let changes = core::changes(n, &v.log);
            let Some(at) = changes.iter().rposition(|(_, before, after)| before.publication.is_some() && after.publication.is_none() && matches!(after.state, State::Ready | State::Later | State::Failed)) else { continue };
            let (id, before, _) = &changes[at];
            // #1535: one cancellation per cancelling entry; a later result supersedes it, so its publication is never dequeued
            if self.cancelled_publications.get(&n) == Some(id) || changes[at + 1..].iter().any(|(_, _, after)| after.result.is_some()) { continue; }
            if let Err(e) = self.board.cancel_publication(n, before.result.as_deref().unwrap_or_default()) {
                log!("#{n} publication cancellation unknown: {e}");
                touched.insert(n);
                continue;
            }
            self.cancelled_publications.insert(n, *id);
        }

        // § 7 publication intent is board state; every host can reconcile the queue after a restart, even with focus off.
        // The attempt host polls while its heartbeat is fresh; others take over only when it is stale or gone (#1359).
        let live = |h: &str| h != me && hosts.iter().any(|i| i.title == format!("taskq host {h}") && now - i.updated <= ttl);
        let mut waiting = BTreeMap::new();
        for v in views.iter().filter(|v| v.projection.task.publication.is_some() && v.projection.task.state == State::Review && !touched.contains(&v.issue.number)) {
            let n = v.issue.number;
            // the publish verdict ends the attempt, so its last execution names the attempt host
            if v.projection.executions.last().is_some_and(|x| live(&x.host)) { continue; }
            // written since `views` read it: the next pass resumes from a fresh read
            if !self.cache.contains_key(&n) { continue; }
            let began = std::time::Instant::now();
            let r = crate::dispatch::resume_publication(self.board, &self.cfg, &v.issue, &v.projection.task, &v.log);
            if r.ok() { self.publication_waits.remove(&n); continue; }
            let published = v.comments.iter().rposition(|c| c.trusted && c.body.lines().next().and_then(|h| h.trim().strip_prefix("taskq:"))
                .and_then(|h| h.split_whitespace().next()) == Some("publish"));
            // #1820 A2 (pilotq #106): a wait the pass cannot end is shown with its reason and age, never only logged
            // #1942: logged when the reason changes, not every pass
            if self.publication_waits.get(&n) != Some(&r.reason) {
                log!("#{n} publication: {}", r.reason);
                let (a, rt) = v.projection.executions.last().map_or((0, "other"), |x| (x.attempt, x.runtime.as_str()));
                crate::profile::stage("checks", n, a, rt, Some(began.elapsed()), "retried", crate::profile::class(&r.reason), crate::profile::Usage::NoModel);
                // #1935: a wait only a person ends gets one plain board comment per PR head (the `publish` entry before it),
                // never a log entry; the board keeps it across restarts (R1)
                if r.reason.contains(crate::board::MANUAL_MERGE) && !v.comments[published.map_or(0, |i| i + 1)..].iter().any(|c| c.trusted && c.body.starts_with(MANUAL_MERGE_NOTE)) {
                    if let Err(e) = self.board.comment(n, &format!("{MANUAL_MERGE_NOTE}: {}", r.reason)) { log!("#{n} manual-merge note unknown: {e}"); }
                }
                self.publication_waits.insert(n, r.reason.clone());
            }
            let since = published.map(|i| v.comments[i].created);
            waiting.insert(n, crate::snapshot::PublicationWait { reason: r.reason, since, age_s: since.map(|t| now - t) });
        }
        self.publication_waits.retain(|n, _| views.iter().any(|v| v.issue.number == *n && v.projection.task.publication.is_some()));
        if let Some(snapshot) = &mut self.snapshot {
            for r in &mut snapshot.tasks { r.publication = waiting.remove(&r.n); }
        }
        touched.extend(views.iter().filter(|v| v.projection.task.publication.is_some()).map(|v| v.issue.number));

        // 6. route to own attempts, then admit within capacity; the gate holds launches, the lease stays (§ 3)
        let mut pending = BTreeSet::new();
        // a reservation lives within one pass: it becomes a child or a held claim, or is released here
        self.reserved.retain(|r| self.held.iter().any(|l| l.task == r.0 && l.attempt == r.1) || unresolved_local.contains(r));
        (self.requested, self.refused) = (vec![], None);
        if self.host.is_some() { self.sync_host(); }
        touched.extend(self.release_held());
        for v in &views {
            let (n, t) = (v.issue.number, &v.projection.task);
            let (Some(a), Some(h)) = (t.attempt, host_of(t)) else { continue };
            if h != me || !matches!(t.state, State::Doing | State::Review) || touched.contains(&n) || self.kids.iter().any(|k| k.task == n) {
                continue;
            }
            if let Some(i) = self.held.iter().position(|l| l.attempt == a) {
                let rt = self.held[i].rt.clone();
                if !self.gui_refused(&v.issue, &rt) && self.reserve(n, a, &rt) {
                    let l = self.held.remove(i);
                    let log = self.seen_log(v)?;
                    self.launch(l, a, log)?;
                } else {
                    pending.insert((n, a));
                }
                continue;
            }
            // #1879 (pilotq#133): a spawn whose start was never reached waits one ttl under its own reason, then ends gone
            if let Some(x) = v.projection.executions.iter().rev().find(|x| x.attempt == a)
                .filter(|x| self.lost_spawn(n, x) && v.projection.executions.iter().filter(|x| x.release.is_none()).count() == 1) {
                let age = now - v.comments.iter().find(|c| c.id == x.id).map_or(now, |c| c.created);
                if age < ttl {
                    self.execution_waits.insert(n, format!("spawn {a} not started on {me}: input not accepted; lost spawn released no-start {ttl} s after the spawn"));
                    pending.insert((n, a));
                } else {
                    self.log.push(format!("#{n} lost spawn {a}: no attempt log on {me} {age} s after the spawn; released no-start"));
                    release_execution(self.board, n, a, a, "no-start")?;
                    self.retired(n, a, a, false, 0);
                    self.execution_waits.remove(&n);
                    self.gone(n, t, a, &[], &format!("start failed: lost spawn {a}: no native start on {me} {age} s after the spawn"))?;
                    touched.insert(n);
                }
                continue;
            }
            if self.execution_waits.contains_key(&n) { pending.insert((n, a)); continue; }
            let seen = v.projection.delivered(a).unwrap_or(0);
            let new: Vec<&Comment> = v.comments.iter().filter(|c| t.inbox.contains(&c.id) && c.id > seen).collect();
            let Some(last) = new.last().map(|c| c.id) else {
                let last = v.projection.executions.iter().rev().find(|x| x.attempt == a);
                if let Some(x) = last.filter(|x| x.release.is_some()) {
                    let text = format!("execution {} confirmed {:?} without task outcome{}", x.id, x.release.unwrap(), self.last_message(n, a, t));
                    self.exit_outcome(n, t, a, x.usage_hold, now, &text)?;
                    touched.insert(n);
                } else {
                    self.execution_waits.insert(n,"native execution evidence missing; reconcile the owning runtime".into());
                }
                continue;
            };
            let quota = new.iter().any(|c| c.body.lines().next() == Some(core::entry("steer", &["quota"], Some(a), "").as_str()));
            if quota && v.projection.executions.iter().rev().find(|x| x.attempt == a).is_some_and(|x| x.release == Some(core::Release::NoStart)) {
                self.execution_waits.insert(n,"quota continuation unavailable; recover the original native session without replay or migration".into());
                pending.insert((n,a));
                continue;
            }
            // #1946: a never-started attempt has no session to resume; the reply to its start refusal starts afresh
            let runs: Vec<_> = v.projection.executions.iter().filter(|x| x.attempt == a).collect();
            let refused:Vec<_>=runs.iter().filter(|x|x.through==last && x.release==Some(core::Release::NoStart)).collect();
            if refused.len()>=core::GONE_MAX as usize {
                let x=refused.last().unwrap();
                let reason=v.log.iter().rev().find(|e|e.body.lines().next().is_some_and(|h|h==format!("taskq: released {} no-start attempt {a}",x.id)))
                    .map(|e|core::entry_text(n,&e.body)).unwrap_or("native launch refused");
                let card=input_card(n,reason,refused.len());
                self.post(n,core::with_fields(core::entry("ask",&[],Some(a),&card),Some(&format!("input-retry-{}",x.id)),None))?;
                touched.insert(n);continue;
            }
            if !runs.is_empty() && runs.iter().all(|x| !x.accepted && x.release == Some(core::Release::NoStart)) {
                let text = "Reason: start refused before launch; the reply starts a fresh attempt";
                // a reviewer gone keeps the result (§ 2); a worker requeue ends the attempt
                if t.state == State::Review { self.gone(n, t, a, &[], text)?; } else { self.post(n, core::entry("requeue", &[], Some(a), text))?; }
                touched.insert(n);
                continue;
            }
            let (rt, session) = rt_of(t).unwrap_or(("", ""));
            let mut session = session.to_string();
            let mut l = Launch { files: String::new(), execution: None, task: n, title: v.issue.title.clone(), attempt: a, role: t.role.clone().unwrap_or_default(), rt: rt.into(), session: session.clone(), url: self.board.url(n), resume: None };
            if session == "-" || crate::runtime::of(rt).has("remote-control") {
                session = crate::runtime::native_session(rt, &crate::runtime::log_path(&self.cfg.root, &l)).unwrap_or_else(|| if session == "-" { String::new() } else { session });
            }
            let text: Vec<String> = new.iter().map(|c| core::delivery(n, &v.log, c.id)).collect();
            let inbox: Vec<(&str, &str)> = new.iter().zip(&text).map(|(c, t)| (core::kind(n, &c.body), t.as_str())).collect();
            l.session = session;
            l.resume = Some(format!("{}\n{}", text.join("\n\n"), crate::runtime::turn_marker(&inbox)));
            if l.session.is_empty() {
                self.execution_waits.insert(n,"session identity unknown; recover the original native session".into());
                pending.insert((n,a));
                continue;
            }
            let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
            let account = crate::runtime::accounts::of_log(&crate::runtime::log_path(&self.cfg.root,&l));
            if accounts.error.is_some() || accounts.state.get(&l.rt).and_then(|m|m.get(&account)).is_some_and(|q|q.end() > now) || self.refused_access.contains_key(&l.rt) || self.limited.contains_key(&l.rt) || self.logged_out.contains(&l.rt) || self.gui_refused(&v.issue, &l.rt) || !self.reserve(n, a, &l.rt) {
                pending.insert((n, a)); // the inbox stays undelivered
                continue;
            }
            let log = self.seen_log(v)?;
            self.launch(l, last, log)?;
        }
        pending.extend(self.held.iter().map(|l| (l.task, l.attempt)));
        if pending != self.pending {
            self.pending = pending;
            self.beat()?;
        }
        // #1616: every ready task this pass reaches gets one admission code, written into the snapshot rows
        let mut decided: BTreeMap<u64, (&'static str, String)> = BTreeMap::new();
        let focus_off = self.focus.is_empty();
        if focus_off { tracing::info!(coreq_refusal = "focus"); }
        let user = if focus_off { String::new() } else { self.board.me().unwrap_or_default() };
        let mut queue: Vec<&View> = vec![];
        for v in &views {
            let (n, t) = (v.issue.number, &v.projection.task);
            if touched.contains(&n) || self.kids.iter().any(|k| k.task == n) { continue; }
            if !(t.state == State::Ready || (t.state == State::Review && t.attempt.is_none() && t.publication.is_none())) { continue; }
            // `needs-gate`: every host of this version has gates; N-1 lacks them and leaves the task alone (§ 8)
            let lacks = || v.issue.labels.iter().filter_map(|l| l.strip_prefix("needs-"))
                .find(|x| !(*x == "gate" || if *x == "gui" { self.gui().is_ok() } else { self.cfg.has.iter().any(|h| h == x) }));
            let refusal = if !is_task(&v.issue) { Some(("labels", crate::snapshot::START_LABELS.into())) }
                else if self.execution_waits.contains_key(&n) { Some(("unknown", crate::snapshot::ADMISSION_UNKNOWN[3].to_string())) }
                else if let Some(r) = self.launch_refusal(n) { Some(r) }
                else if let Err(e) = crate::labels::validate(&v.issue.labels) { log!("#{n}: {e}; not admitted"); Some(("labels", e.to_string())) }
                else if let Some(reason)=self.host_claim(v,&hosts,now).err() {Some(("host",reason))}
                else if let Some(x) = lacks() { Some(("capability", format!("needs-{x}"))) }
                else if focus_off { Some(("focus", self.focus_error.clone().unwrap_or_else(|| FOCUS_OFF.into()))) }
                else if priority(&v.issue) != 0 && !self.focus.iter().any(|f| f == "all" || crate::labels::matches(&v.issue.labels, f)) {
                    Some(("focus", format!("focus {}", self.focus.join(","))))
                }
                // last: the github.com list carries no assignees (§ 4 budget), so only a candidate reads them
                else if v.issue.labels.iter().any(|l| l == "assignee-only") && !assigned(self.board, &v.issue, &user).unwrap_or(false) {
                    Some(("host", "assignee-only: not assigned to this board user".into()))
                }
                else { None };
            match refusal { Some(r) => { decided.insert(n, r); } None => queue.push(v) }
        }
        if focus_off {
            if let Some(host) = &self.host { if let Ok(mut h) = host.lock() { h.ready(&self.cfg.root, false); } }
            self.decided(decided);
            return Ok(());
        }
        // reviews before ready tasks, then priority and number within each (§ 4 step 6)
        queue.sort_by_key(|v| (v.projection.task.state != State::Review, priority(&v.issue), v.issue.number));
        let ready = queue.iter().any(|v| self.select_runtime(v).is_ok());
        if let Some(host) = &self.host {
            if let Ok(mut h) = host.lock() { h.ready(&self.cfg.root, ready); }
        }
        // a refused reservation ends admission for this pass; the rest of the queue reads the same refusal
        let mut refused: Option<(&'static str, String)> = None;
        let mut last_primary=views.iter().flat_map(|v|v.projection.changes.iter())
            .filter(|(_,_,t)|t.state==State::Doing && t.role.as_deref()==Some("worker"))
            .filter_map(|(id,_,t)|host_of(t).map(|h|(*id,h.to_string())))
            .chain(hosts.iter().filter(|h|h.trusted).filter_map(|h| {
                let b=Balancing::read(&h.body)?;
                (b.mode=="any").then_some((b.last_spawn?,host_name(h)?.to_string()))
            }))
            .max_by_key(|(id,_)|*id).map(|(_,h)|h);
        for v in queue {
            let n = v.issue.number;
            if let Some(r) = refused.clone().or_else(|| self.launch_refusal(n)) {
                decided.insert(n, r);
                continue;
            }
            let role = if v.projection.task.state == State::Review { "reviewer" } else { "worker" };
            if role == "worker" && label(&v.issue, "run-").is_none() && v.issue.labels.iter().filter(|l| l.starts_with("prefer-")).count() > 1 {
                self.log.push(format!("#{n}: conflicting prefer-* labels; preference ignored"));
            }
            let (rt, fallback) = match self.select_runtime(v) { Ok(rt) => rt, Err(r) => { decided.insert(n, r); continue; } };
            let host_reason=match self.host_claim(v,&hosts,now) {Ok(reason)=>reason,Err(reason)=>{decided.insert(n,("host",reason));continue;}};
            let spread=match self.weighted_host(v,&hosts,now,last_primary.as_deref()) {Ok(r)=>r,Err(r)=>{decided.insert(n,("host",r));continue;}};
            let reasons:Vec<_>=host_reason.into_iter().chain(spread).chain(fallback).collect();
            let fallback=(!reasons.is_empty()).then(||format!("{}; selected {rt}",reasons.join("; ")));
            if !self.reserve(n, 0, &rt) {
                let r = self.launch_refusal(n).unwrap_or_else(|| self.refused.as_ref()
                    .map_or(("unknown", crate::snapshot::ADMISSION_UNKNOWN[2].into()), |(c, t)| refusal_admission(c, t)));
                decided.insert(n, r.clone());
                refused = Some(r);
                continue;
            }
            let previous_spawn=self.last_spawn;
            self.claim(v, role, &rt, fallback.as_deref().unwrap_or_default())?;
            if self.last_spawn!=previous_spawn {last_primary=Some(self.cfg.me.clone());}
            let reason = fallback.map_or_else(|| format!("claimed on {rt}"), |why| { self.log.push(format!("#{n} {why}")); why });
            decided.insert(n, ("admitted", reason));
        }
        self.decided(decided);
        Ok(())
    }

    /// Moves to closed, `failed`, `ask` or `waiting` since the previous pass go to `<root>/.taskq/events` (§ 4); a task
    /// new since then counts from its first state. A task gone from the open list counts only when a fresh read shows it
    /// closed (R12).
    fn events(&mut self, views: &[View], now: i64) -> Result<(), String> {
        // the attempt current at the move; a `close` or `failed` that ended it leaves the last spawn
        let attempt = |t: &Task, c: &[Comment]| t.attempt.or_else(|| c.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: spawn ")).map(|c| c.id));
        let now_seen: HashMap<u64, State> = views.iter().map(|v| (v.issue.number, v.projection.task.state)).collect();
        let first = self.seen.is_empty() && self.events_since.is_none();
        let mut ns: Vec<u64> = self.seen.keys().copied().collect();
        if !first {
            ns.extend(views.iter().map(|v| v.issue.number).filter(|n| !self.seen.contains_key(n)));
        }
        let (mut moves, mut closed) = (vec![], vec![]);
        for n in ns {
            let (to, a) = match views.iter().find(|v| v.issue.number == n) {
                // #1712: `ask` is its card's entry (§ 2 revision), so a second ask of one attempt is its own event
                Some(v) if v.projection.task.state == State::Ask => (State::Ask, Some(v.projection.revision).filter(|r| *r > 0).or(Some(n))),
                // `waiting` without an attempt: the last comment, else the issue number
                Some(v) if v.projection.task.state == State::Waiting => (State::Waiting, attempt(&v.projection.task, &v.comments).or(v.comments.last().map(|c| c.id)).or(Some(n))),
                Some(v) => (v.projection.task.state, attempt(&v.projection.task, &v.comments)),
                None => match self.board.issue(n) {
                    Ok(i) if !i.open => {
                        let c = self.board.comments(n)?;
                        let a = attempt(&Task::default(), &c);
                        closed.push((n, i.created, c));
                        (State::Closed, a)
                    }
                    _ => continue,
                },
            };
            let kind = match to {
                State::Closed => "close",
                State::Failed => "failed",
                State::Ask => "ask",
                State::Waiting => "blocked",
                _ => continue,
            };
            if let Some(a) = a.filter(|_| self.seen.get(&n) != Some(&to)) {
                moves.push((n, kind, a, String::new()));
            }
        }
        // `gone`: each trusted `gone attempt` entry newer than the previous pass; the first pass adds none
        let newest = views.iter().flat_map(|v| v.comments.iter().map(|c| c.id)).max();
        if let Some(after) = self.gone_after {
            for v in views {
                for c in v.comments.iter().filter(|c| c.trusted && c.id > after) {
                    if let Some(a) = c.body.lines().next().and_then(|l| l.strip_prefix("taskq: gone attempt ")).and_then(|a| a.parse().ok()) {
                        moves.push((v.issue.number, "gone", a, String::new()));
                    }
                }
            }
        }
        // queue transitions (§ 4 Profiling): accepted entries newer than the previous pass that changed the fold
        if let Some(after) = self.moved_after {
            let closed: Vec<_> = closed.iter().map(|(n,created,c)| {
                let log: Vec<_> = c.iter().filter(|c| c.trusted).map(|c| core::Entry { id:c.id, body:c.body.clone(), edited:c.edited }).collect();
                (*n, *created, c, core::project(*n, &log, false))
            }).collect();
            let histories = views.iter().map(|v| (v.issue.number, v.issue.created, &v.comments, &v.projection))
                .chain(closed.iter().map(|(n,created,c,p)| (*n,*created,*c,p)));
            // review with a deferred publication is the `merge-queue` state: its wait is not reviewer work (#1344)
            let name = |t: &Task| if t.state == State::Review && t.publication.is_some() { "merge-queue" } else { t.state.name() };
            for (n, created, comments, projection) in histories.filter(|(_,_,c,_)| c.iter().any(|c| c.trusted && c.id > after)) {
                let at = |id: u64| comments.iter().find(|c| c.id == id).map(|c| c.created);
                // the issue creation is the entry into the first state, ready; a board without it leaves it unknown
                let mut since = Some(created).filter(|c| *c > 0);
                let mut moved: Vec<_> = projection.changes.iter().chain(&projection.queue).collect();
                moved.sort_by_key(|m| m.0);
                for (id, from, to) in moved {
                    let id = *id;
                    let entered = std::mem::replace(&mut since, at(id));
                    // `held` is outside the queue model (§ Executable behavioral model): `hold` and `start` add none
                    if id <= after || from.state == State::Held || to.state == State::Held { continue; }
                    let side = if to.session.is_some() { to } else { from };
                    let host = host_of(side);
                    if host.is_none_or(|h| h == self.cfg.me) {
                        let prev = entered.zip(at(id)).and_then(|(a, b)| u64::try_from(b - a).ok());
                        // § 4 Profiling: a worker ask records its kind and cause
                        let ask = (from.state == State::Doing && to.state == State::Ask).then(|| crate::profile::ask_of(comments.iter().find(|c| c.id == id).map_or("", |c| c.body.as_str())));
                        crate::profile::transition(n, id, name(from), name(to), prev, rt_of(side).map(|r| r.0), host, ask);
                    }
                }
            }
        }
        // § 7: a task closed without merging its PR leaves none open; `requeue`, `later` and `failed` keep it.
        // ponytail: a close while no service ran is not seen; a sweep of open `taskq-*` PRs is the upgrade path
        for (n, kind, _, outcome) in &mut moves {
            let n = *n;
            if *kind != "close" { continue; }
            let Some(comments) = views.iter().find(|v| v.issue.number == n).map(|v| &v.comments).or_else(|| closed.iter().find(|x| x.0 == n).map(|x| &x.2)) else { continue };
            let entries: Vec<core::Entry> = comments.iter().filter(|c| c.trusted).map(|c| core::Entry { id: c.id, body: c.body.clone(), edited: c.edited }).collect();
            let verdict = comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: close")).map(|c| core::verdict(n, &entries, c.body.split_once('\n').map_or("", |x| x.1)));
            let verdict = verdict.as_deref();
            // § 7 outcome: a replaced task names its successor's state, read from this pass or the board
            let next = verdict.and_then(crate::display::successor).map(|m| views.iter().find(|v| v.issue.number == m).map(|v| v.projection.task.state.name())
                .or_else(|| crate::dispatch::current(self.board, m).ok().map(|(i, t, _)| if i.open { t.state.name() } else { "closed" })).unwrap_or("unknown"));
            *outcome = crate::display::close_outcome(verdict, next);
            let reason = verdict.and_then(|v| v.lines().find(|l| !l.trim().is_empty())).map_or("issue closed on the board".into(), |l| l.chars().take(160).collect::<String>());
            match self.board.close_task_prs(n, &reason) {
                Ok(prs) => self.log.extend(prs.iter().map(|p| format!("#{n} closed unmerged PR #{p}"))),
                Err(e) => self.log.push(format!("#{n} task PR left open, state unknown: {e}")),
            }
        }
        self.gone_after = self.gone_after.max(newest);
        self.moved_after = Some(self.gone_after.unwrap_or(0));
        moves.sort();
        // `ask` stage (§ 4 Profiling): from the move into `ask` of this host's attempt to the move out
        for v in views.iter().filter(|v| v.projection.task.state == State::Ask && self.seen.get(&v.issue.number).is_some_and(|s| *s != State::Ask)) {
            if let (Some(a), Some((rt, _)), true) = (v.projection.task.attempt, rt_of(&v.projection.task), host_of(&v.projection.task) == Some(self.cfg.me.as_str())) {
                self.asked.insert(v.issue.number, (std::time::Instant::now(), a, rt.to_string()));
            }
        }
        let left: Vec<u64> = self.asked.keys().filter(|n| now_seen.get(n) != Some(&State::Ask)).copied().collect();
        for n in left {
            let (began, a, rt) = self.asked.remove(&n).unwrap();
            let outcome = if matches!(now_seen.get(&n), Some(State::Doing | State::Review | State::Ready)) { "ok" } else { "cancelled" };
            crate::profile::stage("ask", n, a, &rt, Some(began.elapsed()), outcome, "none", crate::profile::Usage::NoModel);
        }
        self.seen = now_seen;
        if first || !moves.is_empty() || self.events_since.is_none_or(|s| s < now - MONTH) {
            let path = crate::paths::dot(&self.cfg.root).join("events");
            match record(&path, now, &moves) {
                Ok(s) => self.events_since = Some(s),
                Err(e) => self.log.push(format!("events: {e}")),
            }
        }
        for (n, kind, a, outcome) in moves {
            self.log.push(format!("#{n} event {kind} {a} {outcome}").trim_end().into());
        }
        Ok(())
    }

    /// § 2 visual evidence: the app has no board token, so the service copies each open ask's images once into
    /// `.taskq/images`; the tray shows them inline. A failed read leaves an empty file: the app keeps the link, and the
    /// next service start retries. Copies of closed asks are removed.
    fn copy_images(&self) {
        let Some(s) = &self.snapshot else { return };
        let urls: Vec<String> = s.tasks.iter().filter_map(|r| r.task.card.as_deref()).flat_map(crate::snapshot::card_images).collect();
        let keep: Vec<PathBuf> = urls.iter().map(|u| crate::snapshot::image_copy(&self.cfg.root, u)).collect();
        for (url, path) in urls.iter().zip(&keep) {
            if path.exists() { continue; }
            let bytes = match self.board.file(url) {
                Ok(b) if crate::board::image_kind(&b).is_some() => b,
                Ok(_) => { eprintln!("ask image {url}: not an image"); vec![] }
                Err(e) => { eprintln!("ask image {e}"); vec![] }
            };
            if let Err(e) = std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(path, bytes)) { eprintln!("ask image copy {}: {e}", path.display()); }
        }
        for old in std::fs::read_dir(crate::paths::dot(&self.cfg.root).join("images")).into_iter().flatten().flatten() {
            if !keep.contains(&old.path()) { let _ = std::fs::remove_file(old.path()); }
        }
    }
    /// R6 stuck: one `#N event stuck <state> <age>` line per task, again only after it moves; observe only.
    fn log_stuck(&mut self, now: i64) {
        let Some(snapshot) = &self.snapshot else { return };
        let stuck: HashMap<u64, (i64, &str, i64)> = snapshot.tasks.iter().filter_map(|r| r.stuck_age().map(|a| {
            let since=if r.stuck.is_none() {r.heartbeat["progress"]["overdue_since"].as_i64()} else {None};
            (r.n,(since.unwrap_or(now-a),r.task.state.name(),a))
        })).collect();
        for (n, (last, state, a)) in &stuck {
            if self.stuck.insert(*n, *last) != Some(*last) {
                self.log.push(format!("#{n} event stuck {state} {}", crate::display::age(*a)));
            }
        }
        self.stuck.retain(|n, _| stuck.contains_key(n) || self.progress.iter().any(|((task,_,_),p)|task==n && p.2));
    }
    /// § 4 Churn: one `#N event churn ...` line and one `service.churn` telemetry record per flagged task until it clears.
    fn log_churn(&mut self) {
        let Some(snapshot) = &self.snapshot else { return };
        let flagged: Vec<(u64, crate::snapshot::Churn)> = snapshot.tasks.iter().filter_map(|r| r.churn.map(|c| (r.n, c))).collect();
        for (n, c) in &flagged {
            if self.churned.insert(*n) {
                self.log.push(format!("#{n} event churn notes {} spawns {} accepted {}", c.notes, c.spawns, c.accepted));
                crate::profile::measure_counted("service.churn", || ((), [("task", *n), ("notes", c.notes), ("spawns", c.spawns), ("accepted", c.accepted)]
                    .into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
            }
        }
        self.churned.retain(|n| flagged.iter().any(|(m, _)| m == n));
    }

    /// docs/design/worktrees.md: one tidy on the first pass after start, then at most hourly. A child this service
    /// runs keeps its worktree; an open task's state comes from this pass's read.
    fn tidy(&mut self, open: &HashMap<u64, &View>) {
        if self.tidied.is_some_and(|t| t.elapsed().as_secs() < crate::tidy::EVERY) { return; }
        self.tidied = Some(std::time::Instant::now());
        let kids: Vec<u64> = self.kids.iter().map(|k| k.task).collect();
        let states: HashMap<u64, State> = open.iter().map(|(n, v)| (*n, v.projection.task.state)).collect();
        let t = crate::tidy::Tidy { board: self.board, root: &self.cfg.root, me: &self.cfg.me, boot: self.boot, external: self.cfg.external, force: false, dry: false };
        let o = crate::profile::measure_counted("service.tidy", || {
            let o = t.run(&mut |n| kids.contains(&n), &|n| states.get(&n).copied());
            let counts = o.counts.clone();
            (o, counts)
        });
        let skipped = o.report["outcome"].as_str().is_some_and(|s| s.starts_with("skipped"));
        // #1549: a refusal or stale root logs once per changed line, across restarts; `.taskq/cleanup` is a cache (R1)
        let path = crate::paths::dot(&self.cfg.root).join("cleanup");
        let last: BTreeSet<String> = std::fs::read_to_string(&path).unwrap_or_default().lines().map(String::from).collect();
        let sticky = |l: &String| [" kept: ", " unconfirmed: ", "root stale: ", "tidy skipped: "].iter().any(|k| l.contains(k));
        let mut next: BTreeSet<String> = if skipped { last.clone() } else { BTreeSet::new() };
        for line in o.log {
            if sticky(&line) {
                next.insert(line.clone());
                if last.contains(&line) { continue; }
            }
            self.log.push(line);
        }
        if next != last && let Err(e) = std::fs::create_dir_all(crate::paths::dot(&self.cfg.root)).and_then(|_| std::fs::write(&path, next.into_iter().map(|l| l + "\n").collect::<String>())) {
            self.log.push(format!("cleanup cache: {e}"));
        }
        if !skipped {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            tidy_findings(self.board, &self.cfg, &o.conditions, now, &mut self.log);
        }
    }

    fn host_claim(&self,v:&View,hosts:&[Issue],now:i64) -> Result<Option<String>,String> {
        use crate::host::ClaimMode;
        if self.cfg.claim==ClaimMode::None {return Err("manager-only: task execution disabled".into());}
        if let Some(host)=label(&v.issue,"host-") {return if host==self.cfg.me {Ok(None)} else {Err(format!("labelled host-{host}"))};}
        match self.cfg.claim {
            ClaimMode::Any=>return Ok(None),
            ClaimMode::Labelled=>return Err("labelled claim: no host-* label".into()),
            _=>{},
        }
        let since=v.projection.changes.last().and_then(|(id,_,_)|v.comments.iter().find(|c|c.id==*id)).map_or(v.issue.created,|c|c.created);
        if now>=since && now-since>=self.cfg.spillover_grace as i64 {return Ok(Some(format!("spillover: grace {} s elapsed",self.cfg.spillover_grace)));}
        let mut reasons=vec![];
        let mut exhausted=vec![];
        let mut known=false;
        for host in hosts.iter().filter(|h|h.trusted && host_name(h).is_some_and(|name|name!=self.cfg.me)) {
            let Some(balance)=Balancing::read(&host.body) else {reasons.push("primary availability unknown: incompatible heartbeat".into());continue;};
            if balance.mode!="any" {continue;}
            known=true;
            let name=host_name(host).unwrap();
            if !crate::snapshot::live(now-host.updated,self.cfg.ttl) {reasons.push(format!("primary {name} availability unknown: stale heartbeat"));continue;}
            match balance.available(v) {
                Ok((rt,_))=>return Err(format!("spillover waits: primary {name} has effective {rt} capacity; grace {} s",self.cfg.spillover_grace)),
                Err(("unknown",reason))=>reasons.push(format!("primary {name}: {reason}")),
                Err((_,reason))=>exhausted.push(format!("primary {name}: {reason}")),
            }
        }
        if !known && reasons.is_empty() {reasons.push("primary availability unknown: no primary heartbeat".into());}
        if reasons.is_empty() {Ok(Some(format!("spillover: zero effective primary availability; {}",exhausted.join("; "))))}
        else {Err(format!("spillover waits: {}; grace {} s",reasons.join("; "),self.cfg.spillover_grace))}
    }

    fn weighted_host(&self,v:&View,hosts:&[Issue],now:i64,last:Option<&str>)->Result<Option<String>,String> {
        if self.cfg.weights.is_empty() || self.cfg.claim!=crate::host::ClaimMode::Any
            || label(&v.issue,"host-").is_some() || v.projection.task.state!=State::Ready {return Ok(None);}
        let local=self.balancing();
        let score=local.weighted_capacity(v,true)?;
        let mut candidates=if score>0.0 {vec![(self.cfg.me.clone(),score)]}else{vec![]};
        let mut unknown=vec![];
        for h in hosts.iter().filter(|h|h.trusted && host_name(h).is_some_and(|n|n!=self.cfg.me)) {
            let name=host_name(h).unwrap();
            let Some(b)=Balancing::read(&h.body) else {unknown.push(format!("{name} incompatible/unknown"));continue;};
            if b.mode!="any" {continue;}
            if !crate::snapshot::live(now-h.updated,self.cfg.ttl) {unknown.push(format!("{name} stale/unknown"));continue;}
            if b.weights.is_empty() {unknown.push(format!("{name} weights/occupancy unknown"));continue;}
            match b.weighted_capacity(v,false) {
                Ok(n) if n>0.0=>candidates.push((name.into(),n)),
                Err(e)=>unknown.push(format!("{name}: {e}")),
                _=>{},
            }
        }
        let (host,score)=crate::balance::host(candidates,last).ok_or("weighted host availability unknown or unavailable")?;
        let reason=format!("weighted host {host}: free weighted capacity {score:.3}{}",if unknown.is_empty(){String::new()}else{format!("; excluded {}",unknown.join("; "))});
        if host==self.cfg.me {Ok(Some(reason))} else {Err(format!("await {reason}"))}
    }

    fn runtime_disabled(&self,rt:&str) -> bool {
        self.cfg.runtime_disabled(rt, self.cap.as_ref().ok().map(|c| &c.limits))
    }

    fn balancing(&self) -> Balancing {
        let accounts=crate::runtime::accounts::Host::read(&self.cfg.install).heartbeat(self.board.now().unwrap_or(0));
        let runtimes=RUNTIMES.iter().map(|rt| {
            // Focus off needs no runtime probe; routine refusals otherwise materialize every idle poll.
            let outcome=if self.focus.is_empty() {
                Err((if self.focus_error.is_some() {"unknown"}else{"focus"},"task focus admits nothing".into()))
            } else {self.launch_refusal(0).map_or_else(||self.admit_runtime(rt,false),Err)};
            let (mut code,mut reason)=match outcome {Ok(_)=>("admitted",String::new()),Err(e)=>e};
            if code=="load-gate" && self.gate.state(self.mono).0=="unknown" {code="unknown";}
            if matches!(code,"access"|"unknown"|"hold") {reason=format!("{rt}: {code}; see host diagnostics");}
            // #2264: the heartbeat reports occupancy even when headroom, capacity policy or a hold refuses admission
            let mut load=(!self.cfg.weights.is_empty()).then(||self.load(rt,false).ok()).flatten();
            if !self.cfg.weights.is_empty() && (self.headroom_error.is_some() || code=="admitted" && load.is_none()) {code="unknown";reason="weighted runtime load/headroom unknown".into();}
            if code!="admitted" {if let Some(load)=&mut load {load.free=0;}}
            ((*rt).into(),RuntimeAvailability {load,code:code.into(),reason:reason.chars().take(160).collect(),full:self.full.contains(*rt),disabled:self.runtime_disabled(rt),quota:accounts.iter().filter(|l|l.starts_with(&format!("quota {rt} "))).cloned().collect()})
        }).collect();
        let mut has=self.cfg.has.clone();has.retain(|h|h!="gui");if self.gui().is_ok() {has.push("gui".into());}
        Balancing {schema:if self.cfg.weights.is_empty(){1}else{2},weights:self.cfg.weights.clone(),last_spawn:(!self.cfg.weights.is_empty()).then_some(self.last_spawn).flatten(),mode:self.cfg.claim.name().into(),default_runtime:self.cfg.default_runtime.clone(),allowed_runtimes:self.cfg.allowed_runtimes.clone(),
            auto_reroute:self.cfg.auto_reroute,has,focus:self.focus_error.is_none().then(||self.focus.clone()),routing:self.cfg.routing.clone(),runtimes}
    }

    fn select_runtime(&self, v: &View) -> Result<(String, Option<String>), (&'static str, String)> {
        if v.projection.reroute.is_some() {return Err(("unknown",crate::snapshot::ROUTING_WAIT.into()));}
        let reviewer = v.projection.task.state == State::Review;
        let default = crate::labels::project_default(&v.issue.labels, reviewer, &self.cfg.routing, &self.cfg.default_runtime);
        let mut load=BTreeMap::new();
        if !self.cfg.weights.is_empty() && !v.issue.labels.iter().any(|l|l.starts_with("run-")) {
            if let Some(e)=&self.headroom_error {return Err(("unknown",e.clone()));}
            for rt in self.cfg.weights.iter().filter(|w|*w.1>0).map(|w|w.0) {
                match self.runtime_load(rt) {
                    Ok(l)=>{load.insert(rt.clone(),l);},
                    Err(e) if e.0=="unknown"=>return Err(e),
                    Err(_)=>{},
                }
            }
        }
        crate::balance::select(&v.issue.labels, reviewer, default, self.cfg.auto_reroute,&self.cfg.weights,&load,
            |rt|self.runtime_disabled(rt), |rt| self.admit_runtime(rt, needs_gui(&v.issue)))
    }

    /// `rt` when it has free capacity: `limits.<rt>` minus live children of it (§ 3), else the #1616 admission code
    /// and reason. `codex-app` only on a host whose `has` lists it (§ 9 opt-in).
    /// A `needs-gui` task gets only a runtime with effective Full access (§ 4 Filter): never a sandboxed one.
    fn admit_runtime(&self, rt: &str, gui: bool) -> Result<String, (&'static str, String)> {
        if self.cfg.allowed_runtimes.as_ref().is_some_and(|a|!a.iter().any(|r|r==rt)) {return Err(("capability",format!("{rt} excluded by project allowed_runtimes")));}
        if crate::runtime::of(rt).has("app-turn") && !(cfg!(any(target_os = "macos", windows)) && self.cfg.has.iter().any(|h| h == rt)) {
            return Err(("capability", format!("{rt} not in has")));
        }
        if let Some(r) = self.refused_access.get(rt) { return Err(("access", format!("{rt}: {r}"))); }
        let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
        if let Some(e) = &accounts.error { return Err(("unknown", format!("account availability unknown: {e}"))); }
        if accounts.off(rt,self.board.now().map_err(|e| ("unknown",e))?) {return Err(("capability",format!("{rt}: {}",accounts.off_reason(rt))));}
        if accounts.active(rt, self.board.now().map_err(|e| ("unknown",e))?).is_none() {
            if let Some(hold) = accounts.hold(rt, self.board.now().map_err(|e| ("unknown",e))?) {
                return Err(("usage-limit",format!("{rt}: {} {}; account quotas {}", if hold.1 {"until"} else {"probe"},crate::board::stamp(hold.0),accounts.heartbeat(self.board.now().unwrap_or(0)).join("; "))));
            }
        }
        if self.limited.contains_key(rt) { return Err(("usage-limit", format!("{rt}: {}", self.limit_line(rt)))); }
        if self.logged_out.contains(rt) { return Err(("login", format!("{rt}: not logged in"))); }
        if gui && !self.full.contains(rt) { return Err(("capability", format!("needs-gui: {rt} has no effective Full grant"))); }
        if self.cap.as_ref().is_ok_and(|c|c.limits.get(rt)==Some(&0)) {return Err(("capacity",format!("{rt} disabled (limit 0)")));}
        let load=self.runtime_load(rt)?;
        if load.free>0 {Ok(rt.to_string())} else {Err(("capacity",format!("{rt} slots full")))}
    }

    fn runtime_load(&self,rt:&str)->Result<crate::balance::Load,(&'static str,String)> {self.load(rt,true)}

    /// `strict`: admission's load, refusing on any unknown input. Otherwise (#2264, heartbeat report) only unknown
    /// occupancy refuses: unknown limits, policy or a hold report `free` 0, unknown headroom reports none.
    fn load(&self,rt:&str,strict:bool)->Result<crate::balance::Load,(&'static str,String)> {
        let limit=self.cap.as_ref().map_err(|_|("unknown",crate::snapshot::ADMISSION_UNKNOWN[1].to_string())).and_then(|c|
            if c.auto {RUNTIMES.contains(&rt).then(||c.limits.get(rt).copied().unwrap_or(u32::MAX)).ok_or(("capability",format!("{rt} unknown runtime")))}
            else {c.limits.get(rt).copied().ok_or(("capacity",format!("{rt} not in limits")))});
        let n=match limit {Ok(n)=>n,Err(e) if strict=>return Err(e),Err(_)=>0};
        let own=self.entry().held.iter().filter(|r|r.2==rt).count();
        let mut running=own;
        let mut free=(n as usize).saturating_sub(own);
        if let Some(host)=&self.host {
            let h=host.lock().map_err(|_|("unknown",crate::snapshot::ADMISSION_UNKNOWN[2].to_string()))?;
            if h.rebuilding || strict && h.policy.is_err() {return Err(("unknown","host occupancy/policy recovery pending".into()));}
            let hold=h.restart || h.draining.contains(&self.cfg.root) || h.held.is_some();
            if strict && hold {return Err(("hold","host draining, restart or hold".into()));}
            running=h.used().get(rt).copied().unwrap_or(0);
            free=match (&self.cap,hold || h.policy.is_err()) {(Ok(c),false)=>free.min(h.remaining(&self.cfg.root,rt,c)),_=>0};
        }
        let headroom=if self.cfg.weights.is_empty() {None}else {
            let h=self.board.now().map_err(|e|("unknown",e)).and_then(|now|crate::runtime::headroom::remaining(&self.cfg.install,rt,now).map_err(|e|("unknown",e)));
            match h {Ok(h)=>h,Err(e) if strict=>return Err(e),Err(_)=>None}
        };
        Ok(crate::balance::Load {running:running as u64,free:free as u64,headroom})
    }

    /// #1616: each ready row of this pass's snapshot carries this host's decision; a row it did not reach keeps
    /// `unknown`, reason `no host decision`. A review awaiting its reviewer carries it too (#1685).
    fn decided(&mut self, decided: BTreeMap<u64, (&'static str, String)>) {
        self.track_queue(&decided);
        let me = self.cfg.me.clone();
        let mut balancing=serde_json::to_value(self.balancing()).unwrap_or(Value::Null);
        headroom_causes(&mut balancing,&self.headroom_lines().join("\n"));
        let Some(snapshot) = &mut self.snapshot else { return };
        if let Some(host)=snapshot.hosts.iter_mut().find(|h|h.name==me) {host.balancing=balancing;}
        let observed = snapshot.observed;
        for r in snapshot.tasks.iter_mut().filter(|r| matches!(r.task.state, State::Ready | State::Review)) {
            // Board-level refusals hold on every host, including an unfinished routing writer.
            if r.admission.as_ref().is_some_and(|a| a.host.is_none() && (a.code=="host" || a.reason==crate::snapshot::ROUTING_WAIT)) { continue; }
            if let Some((code, reason)) = decided.get(&r.n) {
                r.admission = Some(crate::snapshot::Admission { code, reason: reason.clone(), observed, host: Some(me.clone()) });
            }
        }
    }

    /// #2255: ready work this host may serve that this pass did not admit, and since when it waits without a spawn. Labels, routing, missing
    /// capabilities and focus refuse on purpose and do not count; a pending restart names itself as the reason.
    fn track_queue(&mut self, decided: &BTreeMap<u64, (&'static str, String)>) {
        let now = self.board.now().unwrap_or_else(|_| crate::runtime::now());
        let restart = self.host.as_ref().and_then(|h| h.lock().ok().map(|h| h.restart)).unwrap_or(false);
        self.restart_since = if restart { self.restart_since.or(Some(now)) } else { None };
        let counted: Vec<_> = decided.values().filter(|(c, _)| !matches!(*c, "admitted" | "labels" | "host" | "capability" | "focus")).collect();
        let q = &mut self.queue;
        q.ready = counted.len() as u64;
        q.waiting = if counted.is_empty() { None } else { q.waiting.or(Some(now)) };
        q.reason = counted.first().map(|(_, r)| if restart { "host restart pending".into() } else { r.clone() });
        // #2290: every unadmitted ready task names this host's reason on its own board, refusals by labels or focus too
        q.tasks = decided.iter().filter(|(_, (c, _))| *c != "admitted").map(|(n, (c, r))| (*n, crate::cli::text::compact(&format!("{c}: {r}"), 120))).collect();
        // other boards see this row anonymous and with the admission code only: a reason may quote this project's tasks
        let generic = counted.first().map(|(c, _)| if restart { "host restart pending".into() } else { c.to_string() });
        let row = crate::host::Queue { project: None, reason: generic, tasks: BTreeMap::new(), ..q.clone() };
        if let Some(mut h) = self.host.as_ref().and_then(|h| h.lock().ok()) { h.queues.insert(self.cfg.root.clone(), row); }
    }

    /// The view's log while nothing this pass wrote the task (every write drops its cache entry), else a fresh read:
    /// a claim or launch never rereads what `views` just read (#1359).
    fn seen_log(&self, v: &View) -> Result<Vec<core::Entry>, String> {
        if self.cache.contains_key(&v.issue.number) { Ok(v.log.clone()) } else { crate::board::entries(self.board, v.issue.number) }
    }

    /// Post `spawn`, re-read, start the child only if `fold` accepted this spawn (§ 3 claim).
    fn claim(&mut self, v: &View, role: &str, rt: &str, reason: &str) -> Result<(), String> {
        let (n, began) = (v.issue.number, std::time::Instant::now());
        let r = crate::profile::measure("dispatch", || self.claim_inner(v, role, rt, reason));
        let held = self.held.iter().find(|l| l.task == n).map(|l| l.attempt);
        let (a, outcome, err) = match &r {
            Ok(Some((l, _))) => (l.attempt, "ok", "none"),
            Ok(None) => held.map_or((0, "cancelled", "none"), |a| (a, "ok", "none")),
            Err(e) => (0, "failed", crate::profile::class(e)),
        };
        crate::profile::stage("claim", n, a, rt, Some(began.elapsed()), outcome, err, crate::profile::Usage::NoModel);
        match r? {
            Some((l, log)) => { let through = l.attempt; self.launch(l, through, log) },
            None => Ok(()),
        }
    }

    fn claim_inner(&mut self, v: &View, role: &str, rt: &str, reason: &str) -> Result<Option<(Launch, Vec<core::Entry>)>, String> {
        let n = v.issue.number;
        let before = self.seen_log(v)?;
        // #1903: a spawn the readback missed stays unknown; claiming again would post a spawn every pass
        if let Some(&id) = self.unseen.get(&n) {
            if !before.iter().any(|e| e.id == id) { return Ok(None); }
            self.unseen.remove(&n);
        }
        if core::executions(n, &before).iter().any(|x| x.release.is_none()) { return Ok(None); }
        let routing=core::project(n,&before,false);
        if routing.reroute.is_some() {return Ok(None);}
        // Read labels after the routing revision: a completed reroute invalidates a stale runtime selection.
        if routing.routing_revision!=0 {
            let fresh=self.board.issue(n)?;
            if fresh.labels.iter().filter(|l|!l.starts_with("q-")).collect::<BTreeSet<_>>() != v.issue.labels.iter().filter(|l|!l.starts_with("q-")).collect::<BTreeSet<_>>() || !fresh.open {self.cache.remove(&n);return Ok(None);}
        }
        let session = crate::runtime::new_session(rt);
        let reason=if routing.routing_revision==0 {reason.to_string()} else {format!("{reason}\nrouting-revision: {}",routing.routing_revision)};
        let id = self.post(n, core::entry("spawn", &[role, &format!("{rt}:{session}"), &self.cfg.me], None, &reason))?;
        let comments = self.board.comments(n)?;
        let seen = comments.iter().any(|c| c.id == id);
        if !seen {
            self.unseen.insert(n, id);
            crate::incident::detect("adapter.read_after_write", concat!("src/service.rs:", line!()), rt, &[], &["spawn:missing"]);
        }
        let log = crate::board::reply::log(n, &comments)?;
        let t = core::fold(n, &log, false); // only ready or review tasks are claimed
        if t.attempt != Some(id) {
            self.reserved.retain(|r| r.0 != n); // released
            self.sync_host();
            // a readback without the spawn leaves its claim unknown; a won claim ends as a lost spawn (#1879)
            self.log.push(if seen { format!("#{n} spawn {id} lost the claim") } else { format!("#{n} spawn {id} claim unknown: readback lacks the spawn") });
            return Ok(None);
        }
        if role=="worker" && self.cfg.claim==crate::host::ClaimMode::Any && label(&v.issue,"host-").is_none() {
            self.last_spawn=self.last_spawn.max(Some(id));
        }
        let at = self.board.now().unwrap_or_else(|_| crate::runtime::now());
        (self.queue.spawn, self.queue.waiting) = (Some(at), Some(at));
        for r in self.reserved.iter_mut().filter(|r| r.0 == n && r.1 == 0) { r.1 = id; }
        if let Some(host) = &self.host { if let Ok(mut h) = host.lock() { h.claimed(&self.cfg.root, n, id); } }
        let l = Launch { files: String::new(), execution: None, task: n, title: v.issue.title.clone(), attempt: id, role: role.into(), rt: rt.into(), session, url: self.board.url(n), resume: None };
        if !self.may_launch(n) || self.gui_refused(&v.issue, rt) {
            self.pending.insert((n, id));
            self.held.push(l);
            self.beat()?;
            return Ok(None);
        }
        Ok(Some((l, log)))
    }

    /// This host lists `gui` (§ 4 Filter): the probe holds and a runtime runs with Full access.
    pub fn gui(&self) -> Result<(), String> {
        self.gui_probe.clone()?;
        if let Some((k, v)) = self.grants.iter().find(|(_, v)| *v == "missing" || *v == "stale") {
            return Err(format!("{} grant {v} (TCC)", if k == "app_data" { "App Data" } else { "Accessibility" }));
        }
        if self.gui_checked.is_none_or(|at| at.elapsed().as_secs() >= 300) {
            return Err("GUI capture evidence missing or stale (300 s)".into());
        }
        if self.full.is_empty() { Err("no runtime with Full access".into()) } else { Ok(()) }
    }

    /// A `needs-gui` attempt may not run on `rt` now (§ 4 Filter): the probe fails or `rt` lost Full access.
    /// Its resume or held launch waits with the inbox and lease kept.
    fn gui_refused(&self, i: &Issue, rt: &str) -> bool {
        self.gui_reason(i, Some(rt)).is_some()
    }

    fn gui_reason(&self, i: &Issue, rt: Option<&str>) -> Option<String> {
        if !needs_gui(i) { return None; }
        if let Err(reason) = self.gui() { return Some(format!("GUI unavailable on {}: {reason}", self.cfg.me)); }
        if let Some(rt) = rt {
            if let Some(reason) = self.refused_access.get(rt) { return Some(format!("GUI runtime {rt} refused: {reason}")); }
            if !self.full.contains(rt) { return Some(format!("GUI runtime {rt} has no effective Full grant")); }
        }
        None
    }

    fn record_heartbeat_events(&self) {
        for k in self.kids.iter().filter(|k| !self.unknown_pids.contains_key(&k.execution)) {
            let role_code = self.begun.get(&k.attempt).map_or("work", |(r, _, _)| if r == "reviewer" { "review" } else { "work" });
            let reason = if k.stopping { "stopping" } else { "running" };
            crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &k.rt, role_code, "heartbeat", k.task, k.attempt, k.proc.id(), "ok", reason);
        }
    }

    /// Write the heartbeat now: the lease of a new child or held launch is listed before the next pass judges it.
    fn beat(&self) -> Result<(), String> {
        match self.host_issue {
            Some(n) => {
                self.board.edit(n, &self.heartbeat_body())?;
                self.record_heartbeat_events();
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// R9: recover only a definite trust no-start, keeping the same attempt and exact pending notes.
    fn retry_trust(&mut self, v: &View, x: &core::Execution) -> Result<(), String> {
        let t = &v.projection.task;
        let (rt, session) = rt_of(t).ok_or("trust retry runtime missing")?;
        let mut l = Launch { files: String::new(), execution: Some(x.id), task: v.issue.number, title: v.issue.title.clone(),
            attempt: x.attempt, role: t.role.clone().unwrap_or_default(), rt: rt.into(), session: session.into(), url: self.board.url(v.issue.number), resume: None };
        if x.through != x.attempt {
            l.session = crate::runtime::native_session(rt, &crate::runtime::log_path(&self.cfg.root, &l)).ok_or("trust retry native session unknown")?;
            let seen = v.projection.delivered(x.attempt).unwrap_or(0);
            let notes: Vec<_> = v.comments.iter().filter(|c| t.inbox.contains(&c.id) && c.id > seen && c.id <= x.through).collect();
            let text: Vec<_> = notes.iter().map(|c| core::delivery(v.issue.number, &v.log, c.id)).collect();
            let inbox: Vec<_> = notes.iter().zip(&text).map(|(c, text)| (core::kind(v.issue.number, &c.body), text.as_str())).collect();
            l.resume = Some(format!("{}\n{}", text.join("\n\n"), crate::runtime::turn_marker(&inbox)));
        }
        self.pending.insert((l.task, l.attempt));
        self.execution_waits.insert(l.task, "Claude trust setup pending; service retries automatically".into());
        self.held.push(l);
        Ok(())
    }

    #[tracing::instrument(name="service.Service.launch",skip_all)]
    /// `log`: this pass's read of the task (the view, or the claim's spawn readback); launch reads it again only to
    /// confirm its own writes (#1359).
    fn launch(&mut self, mut l: Launch, through: u64, log: Vec<core::Entry>) -> Result<(), String> {
        let mut readback = None;
        let projection = core::project(l.task, &log, false);
        let (executions, t) = (&projection.executions, &projection.task);
        if t.attempt != Some(l.attempt) || !matches!(t.state, State::Doing | State::Review) {
            if l.resume.is_none() { release_execution(self.board, l.task, l.attempt, l.attempt, "no-start")?; }
            return Ok(());
        }
        match crate::reply_files::prepare(self.board,&self.cfg.root,l.task,&log) {
            Ok(files)=>l.files=files,
            Err(e)=>{
                self.log.push(format!("#{} attachments unknown: {e}",l.task));
                self.pending.insert((l.task,l.attempt));self.held.push(l);
                return self.beat(); // no native submission, execution intent or inbox acknowledgement
            }
        }
        // Only an in-memory admission deferral proves the external start was never called.
        let deferred=l.execution.map(|id|executions.iter().find(|x|x.id==id&&x.attempt==l.attempt&&x.release.is_none())
            .ok_or("deferred execution ownership missing")).transpose()?;
        let through=deferred.map_or(through,|x|x.through);
        let execution = if let Some(x)=deferred {x.id} else if l.resume.is_some() {
            if executions.iter().any(|x| x.release.is_none()) { return Err(format!("#{} prior execution termination unconfirmed", l.task)); }
            let previous = executions.last().ok_or("execution ownership missing")?.id;
            let op = format!("execute-{}-{through}-{previous}", l.attempt);
            // A previously submitted intent is uncertain, never permission to repeat the external call.
            if log.iter().any(|e| core::op_id(l.task, &e.body) == Some(&op)) { return Err("execution intent already submitted; reconcile native ownership".into()); }
            let id = self.post(l.task, core::with_fields(core::entry("executing", &[&through.to_string()], Some(l.attempt), ""), Some(&op), None))?;
            let fresh = crate::board::entries(self.board, l.task)?;
            if !core::executions(l.task, &fresh).iter().any(|x| x.id == id && x.release.is_none()) { return Err("execution intent readback missing".into()); }
            readback = Some(fresh);
            id
        } else {
            if !executions.iter().any(|x| x.id == l.attempt && x.release.is_none()) { return Err("initial execution ownership missing".into()); }
            if executions.iter().any(|x| x.release.is_none() && x.id != l.attempt) {
                release_execution(self.board, l.task, l.attempt, l.attempt, "no-start")?;
                return Err("previous execution termination unconfirmed".into());
            }
            l.attempt
        };
        // The reservation becomes the child; uncertain handoff keeps its execution fence.
        l.execution=Some(execution);
        self.observations.insert((l.task,execution),core::Execution {id:execution,attempt:l.attempt,through,
            host:self.cfg.me.clone(),runtime:l.rt.clone(),accepted:false,release:None,usage_hold:None});
        if l.role == "worker" && l.resume.is_none() && !self.cfg.external { if let Some(line) = seed(&self.cfg.root, l.task) { self.log.push(line); } }
        let pos = std::fs::metadata(crate::runtime::attempt_log(&self.cfg.root, &l.role, l.task, l.attempt)).map_or(0, |m| m.len());
        let began = std::time::Instant::now();
        let started = (self.start)(&l).map(|proc| {
            self.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution, task: l.task, attempt: l.attempt, rt: l.rt.clone(), proc });
            if crate::runtime::of(&l.rt).has("cli") {
                // A failed checkpoint retains the handle; polling retries persistence, never launch.
                if let Err(e)=self.kids.last_mut().unwrap().proc.checkpoint(&self.cfg,&l,execution) { self.kids.last_mut().unwrap().unknown=Some(e); }
            }
        });
        if started.as_ref().is_err_and(|e|e.class()=="admission") {
            if started.as_ref().err().is_some_and(|e| e.to_string().starts_with("Claude trust setup")) {
                self.execution_waits.insert(l.task, "Claude trust setup pending; service retries automatically".into());
            }
            self.pending.insert((l.task,l.attempt));
            self.held.push(l);
            self.sync_host();
            return self.beat();
        }
        let err = started.as_ref().err().map_or("none", crate::runtime::StartError::class);
        crate::profile::stage("start", l.task, l.attempt, &l.rt, Some(began.elapsed()), if started.is_ok() { "ok" } else { "failed" }, err, crate::profile::Usage::NoModel);
        let role_code = if l.role == "reviewer" { "review" } else { "work" };
        let spawn_reason = if l.resume.is_some() { "resume" } else { "claimed" };
        match started {
            Ok(()) => {
                self.reserved.retain(|r| r.0 != l.task || r.1 != l.attempt);
                self.sync_host();
                let pid = self.kids.last().map_or(0, |k| k.proc.id());
                if pid > 1 {
                    record_hold(&self.cfg, &l.rt, role_code, l.task, l.attempt, pid, spawn_reason);
                } else {
                    self.unknown_pids.insert(execution, spawn_reason);
                }
                crate::profile::open(&self.cfg.install, &self.cfg.repo, &self.cfg.me, l.task, l.attempt, &l.rt, l.role == "reviewer");
                self.begun.insert(l.attempt, (l.role.clone(), l.rt.clone(), Some((std::time::Instant::now(), pos))));
                self.pending.remove(&(l.task, l.attempt));
                self.gate.launch(self.mono);
                // Keep the owned handle even if acceptance/readback fails.
                let k = self.kids.last_mut().unwrap();
                match k.proc.accepted().and_then(|yes| if yes { accept_execution(self.board, l.task, l.attempt, execution, Some(readback.as_deref().unwrap_or(&log))).map(Some) } else { Ok(None) }) {
                    Ok(Some(cursor)) => { self.delivered.insert(l.attempt, cursor); self.cache.remove(&l.task); },
                    Ok(None) => {},
                    Err(e) => { k.unknown=Some(e); },
                }
                self.beat()
            }
            Err(e) => {
                crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &l.rt, role_code, "spawn", l.task, l.attempt, 0, "failed", e.class());
                if !e.no_start {
                    let reason = format!("execution {execution} launch outcome unknown: {e}");
                    self.execution_waits.insert(l.task, reason.clone());
                    let reservation = (l.task, l.attempt, l.rt.clone());
                    if !self.reserved.contains(&reservation) {
                        crate::telemetry::record_worker(&self.cfg.install, &self.cfg.repo, &self.cfg.me, &l.rt, role_code, "slot-hold", l.task, l.attempt, 0, "failed", "launch-outcome-unknown");
                        self.reserved.push(reservation);
                    }
                    self.pending.insert((l.task, l.attempt));
                    self.sync_host();
                    self.beat()?;
                    return Err(reason);
                }
                if e.class() == "trust-setup" {
                    self.execution_waits.insert(l.task, "Claude trust setup pending; service retries automatically".into());
                    self.pending.insert((l.task, l.attempt));
                    self.held.push(l);
                    return self.beat();
                }
                self.pending_releases.insert((l.task, l.attempt, execution));
                release_execution(self.board, l.task, l.attempt, execution, "no-start")?;
                self.retired(l.task,l.attempt,execution,false,0);
                self.sync_host();
                if e.reportable {
                    #[cfg(not(test))]
                    let agents = crate::runtime::agents(&self.cfg.install);
                    #[cfg(test)]
                    let agents = self.agent_status.clone();
                    if agents.as_array().is_some_and(|rows| rows.iter().any(|r| r["runtime"] == l.rt && r["path"].is_string() && r["login"] == "logged in")) {
                        crate::incident::detect("dispatch.start", concat!("src/service.rs:", line!()), &l.rt, &[], &["runtime:installed"]);
                    }
                }
                let text = format!("start failed: {e}");
                // without a fresh fold a gone could park the task on a card without options: post nothing, report the
                // read error; the next pass reconciles the no-start proof and posts the limit card (#1123).
                let log = crate::board::entries(self.board, l.task).map_err(|r| format!("#{} {text}; gone not posted: {r}", l.task))?;
                if l.resume.is_some() && log.iter().any(|e| t.inbox.contains(&e.id)
                    && e.body.lines().next() == Some(core::entry("steer", &["quota"], Some(l.attempt), "").as_str())) {
                    self.execution_waits.insert(l.task,format!("quota continuation unavailable: {e}; preserve original session/account"));
                    self.pending.insert((l.task,l.attempt));
                    return self.beat();
                }
                // #1946 (pilotq#170): the cap refusal repeats until the contract shrinks, so one ask replaces gone retries
                if e.class() == "rules-cap" {
                    let card = refused_card(l.task, &e.to_string());
                    self.post(l.task, core::with_fields(core::entry("ask", &[], Some(l.attempt), &card), Some(&format!("rules-cap-{execution}")), None))?;
                    return Ok(());
                }
                if l.resume.is_some() {
                    let failures = core::executions(l.task, &log).iter().filter(|x| x.attempt == l.attempt && x.through == through && x.release == Some(core::Release::NoStart)).count();
                    if failures >= core::GONE_MAX as usize {
                        let op = format!("input-retry-{execution}");
                        let card = input_card(l.task, &text, failures);
                        self.post(l.task, core::with_fields(core::entry("ask", &[], Some(l.attempt), &card), Some(&op), None))?;
                        return Ok(());
                    }
                    return Err(format!("#{} {text}; input retained for retry", l.task));
                }
                self.gone(l.task, &core::fold(l.task, &log, false), l.attempt, &[], &text).map(|_| ())
            }
        }
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
