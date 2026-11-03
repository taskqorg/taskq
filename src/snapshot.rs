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
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub progress: Option<Value>,
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
        {
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
    let recovery=(x.release==Some(core::Release::Exit)&&!x.accepted).then(||format!("agent exited; {}",runtime::acceptance_action(&x.runtime)));
    let (state,reason)=if let Some(reason)=&recovery {verdict(&Signals {local:true,wait:Some(reason),..Default::default()})}
        else {verdict(&Signals {beat:beat.as_ref().filter(|(_,at)|live(now-*at,ttl)).map(|b|&b.0),..Default::default()})};
    let observed=recovery.as_ref().map(|_|now).or_else(||beat.as_ref().filter(|_|state!="unknown" || reason!=STALE).map(|b|b.1));
    let progress=beat.as_ref().and_then(|b|b.0.progress.clone()).map(|p|if runtime::progress::valid(&p) {p} else {runtime::progress::unknown("execution progress heartbeat invalid")});
    let status=ExecutionStatus {progress,task,attempt:x.attempt,id:x.id,through:x.through,state:state.into(),reason};
    let mut value=serde_json::to_value(status).unwrap();
    value["host"]=json!(x.host);value["runtime"]=json!(x.runtime);value["observed"]=json!(observed);
    if recovery.is_some() {value["release"]=json!("exit");}
    value
}

/// R6: ownership observation and progress remain independent advisory facts.
pub fn heartbeat(t:&Task,n:u64,hosts:&[board::Issue],executions:&[Value],now:i64,ttl:i64)->Value {
    let host=service::host_of(t);
    let owners:Vec<_>=hosts.iter().filter(|h|h.trusted && Some(h.title.trim_start_matches("taskq host "))==host).collect();
    let owner=if let [h]=owners[..] {Some(h)} else {None};
    let observed=owner.map(|h|h.updated);
    let age=observed.map(|at|now-at);
    let leased=owner.is_some_and(|h|t.attempt.is_some_and(|a|h.body.lines().any(|l|l==format!("#{n} {a}"))));
    let active=matches!(t.state,State::Doing|State::Review)&&t.attempt.is_some();
    let (freshness,reason)=if !active {("not applicable","no live attempt")}
        else if owner.is_none() {("unknown","owning host heartbeat missing or ambiguous")}
        else if age.is_none_or(|a|!live(a,ttl)) {("stale","owning host heartbeat stale or time unknown")}
        else if !leased {("unknown","matching attempt heartbeat lease missing")}
        else {("fresh","matching host/attempt ownership lease observed; progress is separate")};
    let progress=if !active {json!({"source":"not applicable","status":"not applicable","reason":"no live attempt"})} else {executions.iter().find(|x|x["attempt"].as_u64()==t.attempt && x["host"].as_str()==host)
        .and_then(|x|x.get("progress")).cloned().unwrap_or_else(||runtime::progress::unknown("matching execution progress missing"))};
    let mut h=json!({"host":host,"attempt":t.attempt,"source":"board host heartbeat","observed":observed,"age_s":age,
        "freshness":freshness,"reason":reason,"ttl_s":ttl,"progress":progress});
    refresh_heartbeat(&mut h,now,active);
    h
}

/// The board records input intent; only an exact fresh execution observation confirms an executor.
pub fn executor(t:&Task,executions:&[Value],pending:bool,now:i64,ttl:i64)->Value {
    let current:Vec<_>=executions.iter().filter(|x|x["attempt"].as_u64()==t.attempt).collect();
    let (state,reason,observed,source)=if !matches!(t.state,State::Doing|State::Review)||t.attempt.is_none() {
        ("not applicable","no live attempt",None,"board task projection")
    } else if current.is_empty() && pending {
        ("awaiting execution","input recorded; awaiting execution, no current native work confirmed",Some(now),"board input projection")
    } else if let [x]=current[..] {
        let observed=x["observed"].as_i64();
        let fresh=observed.is_some_and(|at|live(now-at,ttl));
        let state=if x["release"]=="exit"||!fresh {"unknown"}
            else if x["state"]=="running" {"active"}
            else if x["state"]=="waiting" && pending {"awaiting acceptance"}
            else if x["state"]=="waiting" {"waiting"} else {"unknown"};
        (state,x["reason"].as_str().unwrap_or("current executor observation unavailable"),observed,"owning-host execution observation")
    } else {
        ("unknown","current executor missing or ambiguous; logical task state is not native work",None,"board execution projection")
    };
    json!({"state":state,"reason":reason,"observed":observed,"source":source})
}

/// Re-age cached observations; reading a snapshot never renews either heartbeat.
pub fn refresh_heartbeat(h:&mut Value,now:i64,active:bool) {
    let age=h["observed"].as_i64().map(|at|now-at);
    h["age_s"]=json!(age);h["as_of"]=json!(now);
    if active && age.is_some_and(|a|!live(a,h["ttl_s"].as_i64().unwrap_or(0))) {
        h["freshness"]=json!("stale");h["reason"]=json!("owning host heartbeat stale or time unknown");
    }
    if h["executor"]["observed"].as_i64().is_some_and(|at|!live(now-at,h["ttl_s"].as_i64().unwrap_or(0))) {
        h["executor"]["state"]=json!("unknown");h["executor"]["reason"]=json!("current executor observation stale");
    }
    let progress=&h["progress"];
    let progress_live=progress["observed"].as_i64().is_some_and(|at|live(now-at,h["ttl_s"].as_i64().unwrap_or(0)));
    let warning=if !active {None} else if h["freshness"]!="fresh" {h["reason"].as_str()}
        else if matches!(h["executor"]["state"].as_str(),Some("awaiting execution"|"awaiting acceptance")) {h["executor"]["reason"].as_str()}
        else if h["executor"]["state"]=="unknown" {Some("current executor unknown; inspect execution health")}
        else if !progress_live {Some("execution progress observation missing or stale")}
        else if !matches!(progress["status"].as_str(),Some("quiet"|"active")) {progress["reason"].as_str().or(Some("execution progress unknown"))}
        else {None};
    h["warning"]=json!(warning);
}

#[derive(Default)]
pub struct Row {
    pub heartbeat: Value,
    /// Last accepted worker source for read-only history, never current task authority.
    pub worker: Option<Value>,
    pub conversation: Option<Value>,
    pub n: u64,
    pub title: String,
    pub labels: Vec<String>,
    pub url: String,
    pub task: Task,
    /// § 2 revision: the expected `--revision` of the next command.
    pub revision: u64,
    /// Newest trusted, unedited board comment; missing time never borrows issue creation (D-A19).
    pub updated: Option<i64>,
    /// An unresolved external execution blocks the next submission even after a task state change.
    pub execution_wait: Option<String>,
    /// #2257: host step that releases the worker execution blocking a missing reviewer (R6 stuck row).
    pub missing_reviewer: Option<String>,
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
    /// Why that heartbeat names no URL yet, from the attempt log (#2261).
    pub session_reason: Option<String>,
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
    /// § 7 text of the accepted `result` entry behind the current result; `None` without a result.
    pub verdict: Option<String>,
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

pub const ROUTING_WAIT: &str = "runtime reroute pending; service rechecks writer ownership before completion";

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
pub const WAIT_TEXTS: [&str; 7] = ["Waiting for a free slot", "Waiting for GUI access", "Waiting for the agent to accept input",
    "Waiting for your answer", "Waiting for the previous run to end", "Run state unknown; see service log", "Agent exited; input recovery required"];

/// #1820 owner text of a publication wait, followed by its age and reason.
pub const PUBLICATION_WAIT: &str = "Publication waiting";

/// The owner text of a raw wait diagnostic; text no class names reads as the previous run's end.
pub fn wait_text(raw: &str) -> &'static str {
    let has = |w: &[&str]| w.iter().any(|w| raw.contains(w));
    WAIT_TEXTS[if has(&["agent exited;"]) { 6 } else if has(&["GUI"]) { 1 } else if has(&["admission"]) { 0 } else if has(&["owner answer"]) { 3 } else if has(&["input"]) { 2 }
        else if has(&["unknown", "stale", "missing", "unavailable", "deferred"]) { 5 } else { 4 }]
}

/// A `review` result with no reviewer attempt and no pending publication: no reviewer spawned yet (§ 2 row 13).
fn handover(t: &Task) -> bool {
    t.state == State::Review && t.result.is_some() && t.attempt.is_none() && t.publication.is_none()
}

/// R6: no movement for this long marks a task stuck.
pub const STUCK_AFTER: i64 = 2 * 3600;

/// Assignment survives an unaccepted Exit; its pending input is blocked regardless of the host lease's age.
pub(crate) fn input_recovery(v:&View)->Option<&core::Execution> {
    let t=&v.projection.task;
    if !matches!(t.state,State::Doing|State::Review) {return None;}
    let attempt=t.attempt?;
    if !t.inbox.iter().any(|id|*id>v.projection.delivered(attempt).unwrap_or(0)) {return None;}
    v.projection.executions.iter().find(|x|x.attempt==attempt && x.release==Some(core::Release::Exit) && !x.accepted)
}

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
    pub balancing: Value,
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
    /// Heartbeat `requests <n>/h`: billed board requests of this host's last full hour (#1985).
    pub requests: Option<u64>,
    /// Heartbeat `requests api.github.com <n>/h`: the github.com share of `requests` (#2084); none from older hosts.
    pub github: Option<u64>,
    /// Heartbeat `link <installation-id> <public key>` line ([host link](../docs/design/host-link.md), #1832).
    pub link: Option<String>,
    /// Heartbeat `doctor <time> ...` text after `doctor ` ([health checks](../docs/design/host-service.md#health-checks-2194), #2194).
    pub doctor: Option<String>,
    /// Runtimes of the heartbeat's `login <rt> off` lines.
    pub logins: Vec<String>,
    /// The heartbeat issue number.
    pub issue: u64,
    /// Heartbeat `doctor-answer <id>`: the last `taskq doctor --hosts` request this host answered (#2194).
    pub answer: Option<String>,
    /// Heartbeat `doctor-reports <cause> #<N>..`: why the last `reports` check was not ok (#2246).
    pub reports: Option<String>,
    /// The text after `account ` of the heartbeat's account lines ([`crate::runtime::accounts::heartbeat_rows`]).
    pub accounts: Vec<String>,
    /// Heartbeat `selected <version> waits <reason>` (#2247): CoreQ selected another version than the host runs.
    pub selected: Option<String>,
    /// Heartbeat `queue {json}` (#2255): every project of the host service; none from older hosts.
    pub queue: Option<crate::host::Queues>,
    /// The project ttl this row was read with: the heartbeat's liveness.
    pub ttl: i64,
}

/// R6 anomaly kinds whose source defect has an open held fix task (#2256 answer 1): the row names it.
// ponytail: drop each entry once its task lands; the anomaly row then shows without a fix link.
pub const ANOMALY_FIXES: [(&str, u64); 4] = [("remote-control", 2261), ("progress", 2262), ("headroom", 2263), ("running", 2264)];

/// R10 A1: hosts serving projects no board of this view carries: each host's largest `queue` row count against the
/// boards that carry its heartbeat. Rows of other projects stay anonymous (private names), so only counts compare.
pub fn unseen(boards: &[Vec<(String, usize)>]) -> Vec<String> {
    let mut hosts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (name, n) in boards.iter().flatten() { let e = hosts.entry(name).or_default(); e.0 = e.0.max(*n); e.1 += 1; }
    hosts.into_iter().filter(|(_, (n, seen))| n > seen).map(|(h, (n, seen))| {
        let k = n - seen;
        format!("- host {h} · serves {k} project{} outside this view · Next: add {} to `<install>/projects` here (R10) or run `taskq status` on {h}", if k == 1 { "" } else { "s" }, if k == 1 { "it" } else { "them" })
    }).collect()
}

/// #2255: ready work without a spawn this long on a live host with free capacity reads STALLED.
pub const STALL_AFTER: i64 = 600;

impl Host {
    /// #2255: one row per project the host service serves; STALLED needs a live heartbeat, ready work waiting
    /// [`STALL_AFTER`] without a spawn and a runtime neither full nor disabled.
    pub fn queue_rows(&self, observed: i64) -> Vec<Value> {
        let Some(q) = &self.queue else { return vec![] };
        let fresh = live(observed - self.updated, self.ttl);
        let runtimes = self.balancing["runtimes"].as_object();
        let free = runtimes.map(|r| r.values().any(|v| v["full"] == false && v["disabled"] == false));
        q.projects.iter().map(|p| {
            let waited = p.waiting.filter(|_| p.ready > 0).map(|w| observed - w);
            let stalled = fresh && free == Some(true) && waited.is_some_and(|w| w >= STALL_AFTER);
            json!({"project": p.project, "ready": fresh.then_some(p.ready), "spawn": p.spawn, "waiting": waited, "restart": q.restart.map(|r| observed - r),
                "free": free, "stalled": stalled, "reason": p.reason})
        }).collect()
    }

    /// #2247: the running version, and while a handover is pending the selected one with what it waits for. A
    /// heartbeat without the `selected` line falls back to fresh diagnostics, its wait unknown.
    pub fn version_text(&self) -> String {
        if let Some(s) = &self.selected { return format!("{} (selected {s})", self.version); }
        let d = &self.diagnostics;
        match d["selected"]["value"].as_str() {
            Some(v) if d["selected"]["state"] == "ok" && v != self.version => format!("{} (selected {v} waits unknown: older host)", self.version),
            _ => self.version.clone(),
        }
    }
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
        let published = v.projection.task.attempt.zip(v.projection.task.session.as_deref().and_then(|s| s.rsplit(' ').next()))
            .filter(|_| v.projection.task.session.as_deref().is_some_and(|s| crate::runtime::remote_session(s).is_some()))
            .and_then(|(a, h)| urls.get(&(h.to_string(), a)).cloned());
        let recovery=input_recovery(v);
        let last=v.comments.iter().filter(|c| c.trusted && c.body.starts_with("taskq: ")).map(|c| c.created).max().unwrap_or(v.issue.created);
        // #2257: a result whose worker execution stays unreleased past ttl gets no reviewer (§ 3 Recovery); name the host step
        let missing_reviewer=handover(&v.projection.task).then(|| v.projection.executions.iter().find(|x| x.release.is_none())).flatten()
            .filter(|_| observed-last >= cfg.ttl).map(|x| service::release_step(&x.host,&cfg.me));
        let executions:Vec<_>=v.projection.executions.iter().filter(|x|x.release.is_none() || recovery.is_some_and(|r|r.id==x.id))
            .map(|x| {
                let mut value=execution_status(v.issue.number,x,hosts,observed,cfg.ttl);
                if x.host == cfg.me && Some(x.attempt) == v.projection.task.attempt {
                    if let Some(pid) = crate::telemetry::live_worker_pid(&cfg.install, Some(&cfg.root), v.issue.number, x.attempt, x.id) {
                        value["process_id"] = json!(pid);
                        if value["state"] == "unknown" {
                            value["state"] = json!("running");
                            value["reason"] = json!("");
                        }
                    }
                }
                // #1775 A3: `spawn reviewer` keeps `review`, so `changes` (state moves only) misses it; the current attempt's role is the fold's
                let task=&v.projection.task;
                value["role"]=json!(if task.attempt==Some(x.attempt) {task.role.as_deref()}
                    else {v.projection.changes.iter().find(|(id,_,_)| *id==x.attempt).and_then(|(_,_,t)|t.role.as_deref())});
                value
            }).collect();
        let mut heartbeat=heartbeat(&v.projection.task,v.issue.number,hosts,&executions,observed,cfg.ttl);
        let pending=v.projection.task.attempt.is_some_and(|a|v.projection.task.inbox.iter().any(|id|*id>v.projection.delivered(a).unwrap_or(0)));
        heartbeat["input_pending"]=json!(pending);
        heartbeat["executor"]=executor(&v.projection.task,&executions,pending,observed,cfg.ttl);
        refresh_heartbeat(&mut heartbeat,observed,matches!(v.projection.task.state,State::Doing|State::Review)&&v.projection.task.attempt.is_some());
        Row {
        heartbeat,
        worker: std::iter::once(&v.projection.task).chain(v.projection.changes.iter().rev().map(|(_,_,t)|t))
            .find(|t|t.role.as_deref()==Some("worker") && t.attempt.is_some() && t.session.is_some())
            .map(|t|json!({"attempt":t.attempt,"session":t.session,"role":"worker"})),
        n: v.issue.number,
        conversation: core::conversation(v.issue.number,&v.log).map(|id| json!({"id":id,"brief":v.issue.body,"verdict":v.projection.changes.iter().rev().find_map(|(id,_,_)|v.log.iter().find(|e|e.id==*id && ["taskq: result ","taskq: close","taskq: publish"].iter().any(|prefix|e.body.starts_with(prefix))).and_then(|e|e.body.split_once('\n').map(|(_,text)|core::verdict(v.issue.number,&v.log,text))))})),
        title: v.issue.title.clone(),
        labels: v.issue.labels.clone(),
        url: b.url(v.issue.number),
        task: v.projection.task.clone(),
        revision: v.projection.revision,
        updated: v.comments.iter().filter(|c|c.trusted&&!c.edited).max_by_key(|c|c.id).map(|c|c.created).filter(|t|*t>0),
        execution_wait: executions.iter().find(|x|x["state"]!="running" || x["attempt"].as_u64()!=v.projection.task.attempt)
            .map(|x|format!("execution {} on {}: {}: {}",x["id"],x["host"].as_str().unwrap_or("?"),
                x["state"].as_str().unwrap_or("unknown"),x["reason"].as_str().unwrap_or("observation unavailable"))),
        executions,
        missing_reviewer: missing_reviewer.clone(),
        accepted: v.projection.executions.iter().any(|x| Some(x.attempt) == v.projection.task.attempt && x.accepted),
        deps: v.blockers.clone(),
        admission: if v.projection.task.state==State::Ready && v.projection.reroute.is_some() {
            Some(Admission {code:"unknown",reason:ROUTING_WAIT.into(),observed,host:None})
        } else {board_admission(&v.projection.task, &v.issue.labels, &v.blockers, hosts, observed)},
        answered: crate::dispatch::answered_in(v.issue.number, &v.log, &v.projection, &v.comments),
        history: crate::dispatch::history(v.issue.number, &v.log, &v.projection, &v.comments),
        ops: v.log.iter().rev().filter_map(|e| crate::core::op_id(v.issue.number, &e.body)).take(OPS).map(String::from).collect(),
        publication: None,
        stage: crate::core::stage(&v.issue.body, &v.projection.task.stages),
        session_url: published.clone().and_then(Result::ok),
        session_reason: published.and_then(Result::err),
        stuck: recovery.map(|_|observed-last).or(missing_reviewer.as_ref().map(|_|observed-last)).or_else(||stuck(v.issue.number, &v.projection.task, &v.blockers,last,hosts, observed, cfg.ttl)),
        churn: churn(&v.comments, observed),
        verdict: v.projection.task.result.as_ref().and_then(|_| v.projection.changes.iter().rev()
            .find_map(|(id, _, _)| v.log.iter().find(|e| e.id == *id && e.body.starts_with("taskq: result "))))
            .map(|e| core::entry_text(v.issue.number, &e.body).to_string()),
        created: Some(v.issue.created).filter(|t| *t > 0),
        steps: stage_history(v),
        since: v.projection.changes.last().and_then(|(id, ..)| v.comments.iter().find(|c| c.id == *id)).map(|c| c.created),
        failed: (v.projection.task.state == State::Failed).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: failed")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
        later: (v.projection.task.state == State::Later).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: later")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
    }}).collect();
    let hosts = hosts.iter().map(|h| host(h, observed, cfg.ttl as i64)).collect();
    Snapshot { source, observed, reason: None, board: identity(b, cfg), tasks, hosts }
}

/// One host of a heartbeat issue `h` read at `observed`.
pub fn host(h: &board::Issue, observed: i64, ttl: i64) -> Host {
    Host {
        balancing: crate::service::balancing_status(&h.body),
        diagnostics: crate::service_versions::read(&h.body,h.updated,observed,ttl,false),
        name: h.title.trim_start_matches("taskq host ").to_string(),
        service: h.body.lines().find_map(|l| l.strip_prefix("service ")).map(str::to_string),
        version: h.body.lines().find_map(|l| l.strip_prefix("version ")).unwrap_or("?").to_string(),
        updated: h.updated,
        focus: h.body.lines().find_map(|l| l.strip_prefix("focus ")).map(str::to_string),
        limits: h.body.lines().filter_map(|l| l.strip_prefix("limit ").map(str::to_string).or_else(|| l.starts_with("quota ").then(||l.to_string()))).collect(),
        tray: h.body.lines().find_map(|l| l.strip_prefix("tray ")).map(str::to_string),
        requests: h.body.lines().find_map(|l| l.strip_prefix("requests ")?.strip_suffix("/h")?.parse::<u64>().ok()),
        github: h.body.lines().find_map(|l| l.strip_prefix("requests api.github.com ")?.strip_suffix("/h")?.parse::<u64>().ok()),
        // only a heartbeat the board trusts may name a key a host serves
        link: h.body.lines().find_map(|l| l.strip_prefix("link ")).filter(|_| h.trusted).map(str::to_string),
        doctor: h.body.lines().find_map(|l| l.strip_prefix("doctor ")).map(str::to_string),
        logins: h.body.lines().filter_map(|l| l.strip_prefix("login ")?.strip_suffix(" off")).map(str::to_string).collect(),
        issue: h.number,
        answer: h.body.lines().find_map(|l| l.strip_prefix("doctor-answer ")).map(str::to_string),
        reports: h.body.lines().find_map(|l| l.strip_prefix("doctor-reports ")).map(str::to_string),
        accounts: h.body.lines().filter_map(|l| l.strip_prefix("account ")).map(str::to_string).collect(),
        selected: h.body.lines().find_map(|l| l.strip_prefix("selected ")).map(str::to_string),
        queue: h.body.lines().find_map(|l| l.strip_prefix("queue ")).filter(|_| h.trusted).and_then(|j| serde_json::from_str(j).ok()),
        ttl,
    }
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
/// #2261: `session #<N> <attempt> none <cause>` names why the attempt log has no URL yet (Err).
pub fn session_urls(hosts: &[board::Issue]) -> HashMap<(String, u64), Result<String, String>> {
    hosts.iter().flat_map(|h| {
        let name = h.title.trim_start_matches("taskq host ").to_string();
        h.body.lines().filter_map(move |l| {
            let mut w = l.strip_prefix("session #")?.splitn(3, ' ');
            let (_, a, rest) = (w.next()?, w.next()?.parse().ok()?, w.next()?);
            let v = match rest.strip_prefix("none ") {
                Some(why) => Err(why.to_string()),
                None if runtime::remote_url(rest) => Ok(rest.to_string()),
                None => return None,
            };
            Some(((name.clone(), a), v))
        })
    }).collect()
}

fn identity(b: &dyn Board, cfg: &Config) -> Value {
    json!({"kind": cfg.board.name, "host": cfg.web_host, "repo": cfg.repo, "url": b.url(0).trim_end_matches("/0")})
}

/// `taskq status`: one board read; a failed read is an incomplete snapshot with its reason, never zero tasks.
pub fn read(b: &dyn Board, cfg: &Config, raw: &Value, only: &[String]) -> Snapshot {
    let attempt = || -> Res<Snapshot> {
        let (mut views, hosts) = service::views(b, &mut HashMap::new(), &mut HashMap::new(), cfg.ttl)?;
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
    fn health_warning(&self)->Option<&str> {
        matches!(self.task.state,State::Doing|State::Review).then(||self.heartbeat["warning"].as_str()).flatten()
    }
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
        if let Some(reason) = &self.execution_wait {
            // #2257: the worker-to-reviewer handover has no live agent to accept input
            let text = wait_text(reason);
            return Some(if text == WAIT_TEXTS[2] && handover(&self.task) { WAIT_TEXTS[4] } else { text }.into());
        }
        let a = self.admission.as_ref()?;
        match (a.code, &a.host) {
            ("capacity", _) => Some(format!("{} · {}", WAIT_TEXTS[0], a.reason)),
            ("host", None) => Some(a.reason.replacen("no host", "No host", 1)),
            ("login" | "usage-limit" | "access" | "capability", _) => Some(a.reason.clone()),
            _ => None,
        }
    }

    pub fn stuck_age(&self)->Option<i64> {
        self.stuck.or_else(|| {
            let h=&self.heartbeat;
            (matches!(self.task.state,State::Doing|State::Review) && h["freshness"]=="fresh" && h["progress"]["status"]=="overdue"
                && h["progress"]["observed"].as_i64().zip(h["as_of"].as_i64()).is_some_and(|(at,now)|live(now-at,h["ttl_s"].as_i64().unwrap_or(0))))
                .then(||h["as_of"].as_i64().unwrap_or(0)-h["progress"]["overdue_since"].as_i64().unwrap_or(0))
        })
    }

    pub fn json(&self) -> Value {
        let mut row = json!({"n": self.n, "title": self.title, "state": self.task.state.name(), "labels": self.labels, "url": self.url, "revision": self.revision, "updated": self.updated,
            "session": self.task.session, "attempt": self.task.attempt, "role": self.task.role,
            "deps": self.deps.iter().filter(|b| b.dep.repo.is_none() && b.reason == "open").map(|b| b.dep.n).collect::<Vec<_>>(),
            "blocked": self.deps.iter().map(|b| json!({"ref": b.dep.name(), "url": b.url, "reason": b.reason})).collect::<Vec<_>>(),
            "card": self.task.card, "question": self.task.card.as_deref().map(question), "failed": self.failed,
            "heartbeat":self.heartbeat,"controls": controls(self.task.state.name()), "verdict": self.verdict.as_deref().map(verdict_fields)});
        if let Some(worker) = &self.worker { row["worker"] = worker.clone(); }
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
        if let Some(step) = &self.missing_reviewer { row["missing_reviewer"] = json!(step); }
        if let Some(w) = self.wait() { row["wait"] = json!(w); }
        if let Some(u) = &self.session_url { row["session_url"] = json!(u); }
        if let Some(w) = &self.session_reason { row["session_reason"] = json!(w); }
        row["since"] = json!(self.since);
        row["created"] = json!(self.created);
        row["steps"] = json!(self.steps);
        row["stuck"] = json!(self.stuck_age().is_some());
        if let Some(a) = self.stuck_age() { row["age_s"] = json!(a); }
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

/// § 7 typed result verdict (status-report.md Snapshot contract): `Changed:` and `Open:` text (1-2 lines, joined by a
/// newline) and one `Checks:` item per line, each without its prefix. A line without a prefix continues the field
/// before it; another `Word: ` field (`Verdict:`, `Report:`) or the blank line before a card ends it. Missing reads null.
pub fn verdict_fields(text: &str) -> Value {
    const KEYS: [(&str, &str); 3] = [("Changed: ", "changed"), ("Checks: ", "checks"), ("Open: ", "open")];
    let mut f: [Vec<String>; 3] = Default::default();
    let mut cur = None;
    for l in text.lines().map(str::trim_end) {
        if l.trim().is_empty() { if cur.is_some() { break } else { continue } }
        if let Some((i, rest)) = KEYS.iter().enumerate().find_map(|(i, k)| l.strip_prefix(k.0).map(|r| (i, r))) {
            cur = Some(i);
            if i == 1 || f[i].is_empty() { f[i].push(rest.trim().to_string()); continue }
            f[i].last_mut().unwrap().push_str(&format!("
{}", rest.trim()));
        } else if l.split_once(": ").is_some_and(|(w, _)| !w.is_empty() && w.starts_with(|c: char| c.is_ascii_uppercase()) && w.chars().all(|c| c.is_alphanumeric())) {
            cur = None;
        } else if let Some(last) = cur.and_then(|i| f[i].last_mut()) {
            last.push_str(&format!("
{}", l.trim()));
        }
    }
    let one = |v: &Vec<String>| v.first().cloned();
    json!({"changed": one(&f[0]), "checks": f[1], "open": one(&f[2])})
}

/// Published vocabularies of the snapshot (status-report.md Snapshot contract): open states in counter order with
/// owner name and projected `q-` label, and the telemetry error, stage error and runtime codes readers accept.
pub fn vocabulary() -> Value {
    let label = |s: &str| if s == "waiting" { "q-blocked".to_string() } else { format!("q-{s}") };
    json!({"states": crate::display::STATES.iter().map(|(id, name)| json!({"id": id, "name": name, "label": label(id)})).collect::<Vec<_>>(),
        "telemetry": {"errors": crate::profile::ERRORS, "stage_errors": crate::profile::STAGE_ERRORS, "runtimes": crate::profile::RUNTIMES}})
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

// Chain: § 7 result verdict -> typed snapshot fields (#2014). Risk: a consumer re-parses verdict text and drifts.
#[test]
fn verdict_fields_split_changed_checks_and_open() {
    let v = verdict_fields("Changed: a\nb\nVerdict: confirmed x\nChecks: t -- pass\nChecks: u -- fail\nOpen: none\nReport: docs/research/7.md\n\nDone: d\nProblem: p\n1. x ★");
    assert_eq!(v, json!({"changed": "a\nb", "checks": ["t -- pass", "u -- fail"], "open": "none"}));
    let w = vocabulary();
    assert_eq!(w["states"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect::<Vec<_>>(),
        ["doing", "review", "ask", "failed", "ready", "waiting", "later", "held"], "counter order");
    assert_eq!(w["states"][5]["label"], "q-blocked");
    assert!(w["telemetry"]["errors"].as_array().unwrap().iter().any(|e| e == "argument-too-long"));
    assert_eq!(verdict_fields("free text"), json!({"changed": null, "checks": [], "open": null}), "a hand-written result without fields reads null");
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

    /// A3: a ready row the board alone leaves `unknown` takes the live admission of this host's `status.json`.
    pub fn overlay_admission(&mut self, status: &Value) {
        for r in self.tasks.iter_mut().filter(|r| r.admission.as_ref().is_some_and(|a| a.code == "unknown")) {
            let Some(a) = status["tasks"].as_array().into_iter().flatten().find(|t| t["n"] == r.n).map(|t| &t["admission"]) else { continue };
            let Some(code) = ADMISSION_CODES.iter().map(|c| c.0).find(|c| *c != "unknown" && a["code"] == *c) else { continue };
            r.admission = Some(Admission { code, reason: a["reason"].as_str().unwrap_or_default().into(), observed: a["observed"].as_i64().unwrap_or(self.observed), host: a["host"].as_str().map(String::from) });
        }
    }

    /// R10 A1: each live host's count of served projects (its heartbeat `queue` rows) as read on this board.
    pub fn served(&self) -> Vec<(String, usize)> {
        self.hosts.iter().filter(|h| live(self.observed - h.updated, h.ttl)).filter_map(|h| h.queue.as_ref().map(|q| (h.name.clone(), q.projects.len()))).collect()
    }

    /// `status --json` (§ 5).
    pub fn json(&self) -> Value {
        let caps: &[&str] = if self.source == "host" { &HOST_CAPS } else { &BOARD_CAPS };
        json!({"schema": SCHEMA, "source": self.source, "observed": self.observed, "complete": self.reason.is_none(), "reason": self.reason,
            "board": self.board, "totals": self.reason.is_none().then(|| self.totals()), "vocabulary": vocabulary(), "tasks": self.tasks.iter().map(Row::json).collect::<Vec<_>>(),
            "hosts": self.hosts.iter().map(|h| {
                let mut r = json!({"name": h.name, "version": h.version, "updated": h.updated, "diagnostics": h.diagnostics});
                if let Some(s) = &h.selected { r["selected"] = json!(s) }
                if h.balancing["schema"]==2 {r["balancing"]=h.balancing.clone();}
                if let Some(l) = &h.link { r["link"] = json!(l) }
                if h.queue.is_some() { r["queue"] = json!(h.queue_rows(self.observed)) }
                r
            }).collect::<Vec<_>>(),
            "capabilities": caps})
    }

    /// The snapshot keys of `status.json` (§ 4) after a pass; `error`: the pass failed, so tasks are the last read's.
    pub fn status_keys(&self, error: Option<&str>) -> [(&'static str, Value); 9] {
        let reason = error.map(String::from).or(self.reason.clone());
        [("schema", json!(SCHEMA)), ("source", json!("host")), ("observed", json!(self.observed)), ("complete", json!(reason.is_none())),
            ("reason", json!(reason)), ("totals", json!(reason.is_none().then(|| self.totals()))),
            ("tasks", json!(self.tasks.iter().map(Row::json).collect::<Vec<_>>())), ("hosts", self.json()["hosts"].clone()), ("vocabulary", vocabulary())]
    }

    /// R6 text report.
    pub fn text(&self, cfg: &Config, raw: &Value) -> Res<String> { self.text_with(cfg, raw, None) }

    /// R6 text report with the `Decide:` part of [`crate::reports::decide`] after Questions (#1979).
    pub fn text_with(&self, cfg: &Config, raw: &Value, decide: Option<&Value>) -> Res<String> { self.text_extra(cfg, raw, decide, vec![]) }

    /// [`Self::text_with`] plus anomaly rows the caller read outside the snapshot (deploy, board quota; R6 A3).
    pub fn text_extra(&self, cfg: &Config, raw: &Value, decide: Option<&Value>, extra: Vec<String>) -> Res<String> {
        if let Some(r) = &self.reason {
            return Err(r.clone());
        }
        let base = self.board["url"].as_str().unwrap_or_default();
        let project = cfg.root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut out = vec![format!("{project} · [board]({base})"), counts_line(&self.totals())];
        let link = |r: &Row| format!("[#{} {}]({})", r.n, cell(&compact(&r.title, 60)), r.url);
        fn part(out: &mut Vec<String>, head: &str, rows: Vec<String>) {
            if !rows.is_empty() { out.push(String::new()); out.push(head.into()); out.extend(rows); }
        }
        // R6 Owner: questions and review cards, each with the answer command
        let owner: Vec<String> = self.tasks.iter()
            .filter(|r| r.task.state == State::Ask || (r.task.state == State::Review && r.task.card.is_some()))
            .map(|r| {
                let card = r.task.card.clone().unwrap_or_default();
                let (n, url) = (r.n, &r.url);
                let opts = options(&card);
                let o: Vec<String> = opts.iter().map(|(k, t, star)| {
                    let label = cell(&compact(t, 120));
                    let label = if t.chars().count() > 120 { format!("[{label}]({url})") } else { label };
                    format!("{n}.{k} {label}{}", if *star { " ★" } else { "" })
                }).collect();
                // § 2: an `ask` card without options is an owner action
                let action = opts.is_empty() && r.task.state == State::Ask;
                let o = if action { cell(&format!("waiting for owner: {}", compact(&brief(&card), 120))) } else { o.join(" · ") };
                let o = match &r.task.pick { Some((k, why)) => format!("{o} · PM {n}.{k}: {}", cell(&compact(why, 160))), None => o };
                let next = match (&r.task.pick, opts.iter().find(|x| x.2)) {
                    (Some((k, _)), _) => format!("`taskq answer {n}.{k}`"),
                    (None, Some((k, _, _))) => format!("`taskq answer {n}.{k}`"),
                    _ if action => format!("`taskq answer {n} --text <done>`"),
                    _ => format!("answer on [the board]({url})"),
                };
                format!("| {} | [{}]({}) | {o} | {next} |", link(r), cell(&compact(&brief(&card), 160)), url)
            })
            .collect();
        part(&mut out, "Owner (answer N.M):\n| Task | Question | Options | Next |\n|---|---|---|---|", owner);
        // R6 PM: Decide, then one Held and stuck list with its reason (A5)
        let decide = decide.and_then(decide_text);
        let (decide, decide_unknown) = match decide { Some(d) if d.starts_with("Decide: unknown") => (None, Some(d)), d => (d, None) };
        let held: Vec<String> = self.tasks.iter().filter_map(|r| {
            let n = r.n;
            let (reason, next) = match r.task.state {
                State::Held => ("PM".to_string(), format!("`taskq start {n}`")),
                State::Waiting => {
                    let deps: Vec<String> = r.deps.iter().map(|b| format!("[{}]({}) {}", b.dep.name(), b.url, cell(&compact(&b.reason, 80)))).collect();
                    (format!("dependency ({})", deps.join(", ")), if r.stuck.is_some() { "re-read the dependencies; every blocker is closed".into() } else { "wait for the dependency".into() })
                }
                State::Later => (match r.task.until.as_deref().and_then(crate::board::iso) {
                    Some(at) => format!("owner until {}: {}", local_time(at), cell(&compact(r.later.as_deref().unwrap_or("").trim_start_matches("Reason: "), 160))),
                    None => "owner".into(),
                }, format!("`taskq requeue {n}`")),
                State::Ask if r.stuck.is_some() => ("owner".into(), "answer the Owner row".into()),
                _ => return None,
            };
            let stuck = r.stuck.map(|a| format!(" · stuck {}", age(a))).unwrap_or_default();
            Some(format!("| {}{} | {reason}{stuck} | {next} |", link(r), r.stage_text()))
        }).collect();
        let mut pm = vec![];
        if let Some(d) = decide { pm.push(d); }
        if !held.is_empty() { pm.push(format!("Held and stuck:\n| Task | Reason | Next |\n|---|---|---|\n{}", held.join("\n"))); }
        if !pm.is_empty() { out.push(String::new()); out.push("PM:".into()); out.push(pm.join("\n\n")); }
        // R6 Anomalies (A3/A4): every stale, unknown or missing source, with its reason, action and open fix task
        let fix = |kind: &str| ANOMALY_FIXES.iter().find(|f| f.0 == kind).map(|f| format!(" · fix [taskq#{}](https://github.com/taskqorg/taskq/issues/{})", f.1, f.1)).unwrap_or_default();
        let doctor = "Next: inspect the host service (`taskq doctor --hosts`)";
        let mut anomalies = vec![];
        for h in &self.hosts {
            let fresh = live(self.observed - h.updated, cfg.ttl);
            if !fresh { anomalies.push(format!("- host {} · heartbeat stale {} · {doctor}", h.name, age(self.observed - h.updated))); continue; }
            if h.focus.as_deref() == Some("unknown") { anomalies.push(format!("- host {} · focus unknown: it admits nothing · {doctor}", h.name)); }
            let rts: Vec<&String> = h.balancing["weights"].as_object().into_iter().flat_map(|w| w.keys()).collect();
            for (kind, key, what) in [("running", "running", "running count"), ("headroom", "headroom", "quota headroom")] {
                let missing: Vec<&str> = rts.iter().filter(|rt| { let v = &h.balancing["runtimes"][rt.as_str()]["load"][key]; v.is_null() }).map(|rt| rt.as_str()).collect();
                if !missing.is_empty() { anomalies.push(format!("- host {} · {what} not reported for {} · {doctor}{}", h.name, missing.join(", "), fix(kind))); }
            }
            for q in h.queue_rows(self.observed) {
                let name = cell(&compact(q["project"].as_str().unwrap_or("other project"), 80));
                if q["stalled"] == true {
                    anomalies.push(format!("- host {} {name} · STALLED {}: ready {} without a spawn: {} · {doctor}", h.name, age(q["waiting"].as_i64().unwrap_or(0)), q["ready"], cell(&compact(q["reason"].as_str().unwrap_or("no reason logged"), 160))));
                } else if q["restart"].as_i64().is_some_and(|r| r >= STALL_AFTER) {
                    anomalies.push(format!("- host {} {name} · restart pending {} · {doctor}", h.name, age(q["restart"].as_i64().unwrap_or(0))));
                }
            }
        }
        if let Some(d) = decide_unknown { anomalies.push(format!("- {} · Next: rerun `taskq status` after the next service pass", d.trim_end())); }
        let stamp=|v:&Value|v.as_i64().filter(|at|*at>0).map(crate::board::stamp).unwrap_or("unknown".into());
        for r in &self.tasks {
            if let Some(warning) = r.health_warning() {
                let field=|v:&Value| {
                    let full=v.as_str().unwrap_or("unknown");let text=cell(&compact(full,160));
                    if full.chars().count()>160 {format!("[{text}]({})",r.url)} else {text}
                };
                let h=&r.heartbeat;let p=&h["progress"];
                let now=h["as_of"].as_i64().unwrap_or(self.observed);
                let observed=|v:&Value|v.as_i64().filter(|at|*at>0).map(|at| {
                    let elapsed=now.saturating_sub(at);
                    format!("{} · age {}",stamp(v),if elapsed<0 {format!("future by {}",age(elapsed.saturating_neg()))} else {age(elapsed)})
                }).unwrap_or("unknown · age unknown".into());
                let rt=r.task.session.as_deref().and_then(|s|s.split_once(':')).map(|p|p.0).unwrap_or("unknown");
                let current:Vec<_>=r.executions.iter().filter(|x|x["attempt"].as_u64()==r.task.attempt).map(|x|x["id"].as_u64().map(|id|id.to_string()).unwrap_or("unknown".into())).collect();
                let execution=match current.len() {0=>"unknown".into(),1=>current[0].clone(),_=>format!("{} (ambiguous)",current.join(", "))};
                let action=if h["freshness"]!="fresh" {"Inspect the owning host heartbeat and service status; preserve execution."}
                    else if matches!(h["executor"]["state"].as_str(),Some("awaiting execution"|"awaiting acceptance")) {"Inspect input acceptance on the owning host; do not resubmit."}
                    else if p["status"]=="overdue" {"Inspect the pending tool in the owning session; preserve execution."}
                    else {"Inspect the owning session and native progress source; preserve execution."};
                let source_fix = if h["freshness"]=="fresh" && !matches!(p["status"].as_str(),Some("quiet"|"active"|"overdue")) { fix("progress") } else { String::new() };
                anomalies.push(format!("- [{} #{} {}]({}) · {} · Next: {action}{source_fix}",cell(&project),r.n,cell(&compact(&r.title,60)),r.url,cell(&compact(warning,160))));
                anomalies.push(format!("  Task: runtime {} · host {} · attempt {} · execution {execution}",cell(rt),field(&h["host"]),r.task.attempt.map(|a|a.to_string()).unwrap_or("unknown".into())));
                anomalies.push(format!("  Heartbeat: {} · {} · {} · {}",field(&h["freshness"]),field(&h["source"]),observed(&h["observed"]),field(&h["reason"])));
                anomalies.push(format!("  Executor: {} · {} · {}",field(&h["executor"]["state"]),field(&h["executor"]["source"]),observed(&h["executor"]["observed"])));
                let source=if p["clock"].is_string() {format!("{} ({})",field(&p["source"]),field(&p["clock"]))} else {field(&p["source"])};
                anomalies.push(format!("  Progress: {} · {source} · {} · last event {} · {}",field(&p["status"]),observed(&p["observed"]),stamp(&p["last_event"]),field(&p["reason"])));
            } else if matches!(r.task.state, State::Doing | State::Review) {
                if let Some(a) = r.stuck_age() {
                    // #2257: a result no reviewer can take yet names the host step that releases it
                    anomalies.push(match &r.missing_reviewer {
                        Some(step) => format!("- {} · review stuck {}; reviewer not started · Next: {step}", link(r), age(a)),
                        None => format!("- {} · {} stuck {}: no live lease or overdue progress · Next: inspect the owning host; preserve execution", link(r), r.task.state.name(), age(a)),
                    });
                }
            }
            if matches!(r.task.state, State::Doing | State::Review) {
                if let Some(host) = session_link(cfg, raw, r).2 {
                    let why = r.session_reason.as_deref().map_or_else(|| "Remote Control URL not in the heartbeat".into(), |w| format!("no Remote Control URL: {w}"));
                    anomalies.push(format!("- {} · {host} {why} · Next: open the session on {host}{}", link(r), fix("remote-control")));
                }
            }
        }
        anomalies.extend(extra);
        part(&mut out, "Anomalies:", anomalies);
        // R6 Running: who acts on each doing/review row
        let running: Vec<String> = self.tasks.iter()
            .filter(|r| r.task.state == State::Doing || (r.task.state == State::Review && r.task.card.is_none()))
            .map(|r| {
                let actor = match r.task.state {
                    State::Review if r.task.enqueued.is_some() || r.publication.is_some() => "merge queue",
                    _ if r.task.session.is_none() => "none",
                    State::Review => "reviewer",
                    _ => "worker",
                };
                let sha = r.task.result.as_deref().map(|s| format!(" · result {}", &s[..s.len().min(7)])).unwrap_or_default();
                let wait = r.wait().map(|w| format!(" · {}", cell(&w))).unwrap_or_default();
                let health = if r.health_warning().is_some() { " · see Anomalies" } else { "" };
                let (rt, s, _) = session_link(cfg, raw, r);
                let next = if r.health_warning().is_some() || actor == "none" { "see Anomalies".into() }
                    else if r.task.result.is_some() { format!("`taskq results {} --json`", r.n) } else { "wait".into() };
                format!("| {} | {}{sha}{wait}{}{health} | {actor} | {} | {s} | {next} |", link(r), r.task.state.name(), r.stage_text(), cell(&rt))
            }).collect();
        part(&mut out, "Running:\n| Task | State | Actor | Runtime | Session | Next |\n|---|---|---|---|---|---|", running);
        // R6 Ready: why the service has not started it yet
        let ready: Vec<String> = self.tasks.iter().filter(|r| r.task.state == State::Ready).map(|r| {
            let a = r.admission.as_ref();
            let why = r.wait().or_else(|| a.map(|a| if a.reason.is_empty() { a.code.to_string() } else if a.code == "unknown" { a.reason.to_string() } else { format!("{}: {}", a.code, a.reason) })).unwrap_or_else(|| "no host decision".into());
            let next = match a.map(|a| a.code) {
                Some("admitted") => "starting",
                Some("capacity" | "load-gate") => "wait for a free slot",
                Some("host" | "capability" | "labels" | "focus") => "fix the labels or the host focus",
                Some("login") => "log the runtime in on the host",
                Some("usage-limit") => "wait for the usage limit reset",
                Some("access") => "check the runtime access grant",
                Some("hold") => "wait for the hold to clear",
                Some("dependency") => "wait for the dependency",
                _ => "wait for the next host pass",
            };
            format!("| {}{} | {} | {next} |", link(r), r.stage_text(), cell(&compact(&why, 160)))
        }).collect();
        part(&mut out, "Ready:\n| Task | Why not started | Next |\n|---|---|---|", ready);
        let failed: Vec<String> = self.tasks.iter().filter(|r| r.task.state == State::Failed)
            .map(|r| format!("| {} | [{}]({}) | `taskq requeue {}` |", link(r), cell(&compact(r.failed.as_deref().unwrap_or(""), 160)), r.url, r.n))
            .collect();
        part(&mut out, "Failed:\n| Task | Reason | Next |\n|---|---|---|", failed);
        // #1625: a heartbeat older than ttl is an anomaly row; unknown load values move there too
        let hs: Vec<String> = self.hosts.iter().map(|h| format!("{} v{} {}{}", h.name, h.version_text(), age(self.observed - h.updated),
            h.focus.as_ref().filter(|f| *f != "unknown").map(|f| format!(" focus {f}")).unwrap_or_default()
            + &h.limits.iter().map(|l| if l.starts_with("quota ") {format!(" {l}")} else {l.split_once(' ').map_or_else(|| format!(" {l}"), |(rt, at)| format!(" {rt} limit {at}"))}).collect::<String>()
            + &h.balancing["weights"].as_object().map(|weights|weights.iter().map(|(rt,w)| {
                let load=&h.balancing["runtimes"][rt]["load"];
                let running=load["running"].as_u64().map(|n|format!(" running {n} /")).unwrap_or_default();
                format!(" {rt}{running} weight {w}{}",load["headroom"].as_f64().map(|n|format!(" (headroom {:.1}%)",n*100.0)).unwrap_or_default())
            }).collect::<String>()).unwrap_or_default()
            + &h.tray.as_ref().map(|code| format!(" tray {code}")).unwrap_or_default()
            + &h.requests.as_ref().map(|n| format!(" {n} req/h{}", h.github.map(|g| format!(" (github.com {g})")).unwrap_or_default())).unwrap_or_default())).collect();
        if !hs.is_empty() {
            out.push(String::new());
            out.push(format!("Hosts: {}", hs.join(", ")));
        }
        // quiet rows (no ready work, no pending restart) stay in `--json` only; stale hosts read in Anomalies
        let queues: Vec<String> = self.hosts.iter().filter(|h| live(self.observed - h.updated, cfg.ttl)).flat_map(|h| h.queue_rows(self.observed).into_iter().filter(|r| r["ready"] != 0 || r["restart"].is_i64()).map(move |r| {
            let when = |v: &Value| v.as_i64().map_or("none".into(), |s| age(s));
            format!("- {} {} ready {} · last spawn {} · restart pending {}{}", h.name, r["project"].as_str().unwrap_or("other project"),
                r["ready"], r["spawn"].as_i64().map_or("none".into(), crate::board::stamp),
                when(&r["restart"]), if r["stalled"] == true { format!(" · STALLED {}", when(&r["waiting"])) } else { String::new() })
        })).collect();
        part(&mut out, "Queues:", queues);
        Ok(out.join("\n"))
    }
}

/// R6 Decide: one row per undecided report, as [`crate::reports::decide`] orders them; unknown collection says so.
pub fn decide_text(d: &Value) -> Option<String> {
    if d["complete"] != true {
        return Some(format!("Decide: unknown ({})", compact(d["reason"].as_str().unwrap_or("reports not collected yet"), 160)));
    }
    let rows: Vec<String> = d["rows"].as_array()?.iter().map(|r| format!("| [#{} {}]({}) | {} | {} | {} |",
        r["n"], cell(&compact(r["title"].as_str().unwrap_or(""), 60)), r["url"].as_str().unwrap_or(""), r["verdict"].as_str().unwrap_or("none"),
        age(r["age_s"].as_i64().unwrap_or(0)), cell(r["recommendation"].as_str().unwrap_or("")))).collect();
    if rows.is_empty() { return None; }
    Some(format!("Decide:\n| Report | Verdict | Age | Recommendation |\n|---|---|---|---|\n{}", rows.join("\n")))
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

/// Runtime, session cell and, for a remote session without its Remote Control URL, the host (an anomaly row).
fn session_link(cfg: &Config, raw: &Value, r: &Row) -> (String, String, Option<String>) {
    let (t, n) = (&r.task, r.n);
    let Some((rt, id)) = t.session.as_deref().and_then(|s| s.split(' ').next()).and_then(|s| s.split_once(':')) else { return (String::new(), String::new(), None) };
    let host = t.session.as_deref().and_then(|s| s.rsplit(' ').next()).unwrap_or_default();
    let own = host == cfg.me;
    let (id, url) = runtime::session_link(&cfg.root, own, rt, id, n, t.attempt.unwrap_or(0), t.role.as_deref().unwrap_or_default(), r.session_url.clone());
    let host = cell(host);
    let link = match rt {
        _ if id == "-" && r.accepted => format!("{host}: running"),
        _ if id == "-" => format!("{host}: starting"),
        _ if runtime::of(rt).remote => match url { Some(url) => format!("[{host}]({url})"), None => return (rt.to_string(), host.clone(), Some(host)) },
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
    (rt.to_string(), link, None)
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
        let comments=vec![board::Comment{id:1,body:log[0].body.clone(),created:100,trusted:true,edited:false,author:String::new(),read:None},
            board::Comment{id:2,body:log[1].body.clone(),created:150,trusted:true,edited:false,author:String::new(),read:None}];
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
        let mut r = Row { heartbeat: Value::Null, worker: None, conversation: None, n: 1, title: String::new(), labels: vec![], url: String::new(), task, revision: 5, updated: None, execution_wait: None, missing_reviewer: None,
            executions: vec![], accepted: false, deps: vec![], failed: None, later: None, session_url: None, session_reason: None, since: None, created: None, steps: vec![], stuck: None, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None, verdict: None };
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
        r.answered = Some(crate::dispatch::Answered { by: "alex".into(), text: "use x".into(), ..Default::default() });
        assert_eq!(r.json()["answered"], json!({"by": "alex", "text": "use x"}));
    }

    // Chain: R6 3 session cell per runtime and host (#1565). Risk: a Codex thread link shown off its host, where it
    // cannot open, or the DashQ progress page replacing the owning host's thread link.
    #[test]
    fn codex_thread_links_only_on_its_host_else_dashq_progress() {
        use std::fs;
        let task = |host: &str| Task { attempt: Some(5), session: Some(format!("codex:thread-1 {host}")), ..Default::default() };
        let row = |host: &str| Row { heartbeat: Value::Null, worker: None, conversation: None, n: 3, title: String::new(), labels: vec![], url: String::new(), task: task(host), revision: 5, updated: None, execution_wait: None, missing_reviewer: None,
            executions: vec![], accepted: true, deps: vec![], failed: None, later: None, session_url: None, session_reason: None, since: None, created: None, steps: vec![], stuck: None, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None, verdict: None };
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
        let row = |n, stuck| Row { heartbeat: Value::Null, worker: None, conversation: None, n, title: "T".into(), labels: vec![], url: format!("u{n}"), task: Task { state: State::Ask, ..Default::default() }, revision: 0, updated: None,
            execution_wait: None, missing_reviewer: None, executions: vec![], accepted: false, deps: vec![], failed: None, later: None, session_url: None, session_reason: None, since: None, created: None, steps: vec![], stuck, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None, verdict: None };
        let snap = Snapshot { source: "board", observed: 0, reason: None, board: json!({"url": "b"}), tasks: vec![row(1, Some(9000)), row(2, None)], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("| ") && l.contains("stuck")).collect::<Vec<_>>(), ["| [#1 T](u1) | owner · stuck 2h | answer the Owner row |"], "{text}");
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
        assert!(line.contains("mac v1.0 ") && !line.contains("stale"), "{line}");
        assert_eq!(text.lines().filter(|l| l.contains("stale")).collect::<Vec<_>>(), ["- host win · heartbeat stale 5m · Next: inspect the host service (`taskq doctor --hosts`)"], "R6 A3: a stale host is one anomaly row");
    }

    // Chain: heartbeat `selected` line -> status Hosts line and JSON (#2247 A1, A2). Risk: during a handover status
    // showed only the running version, so a rollout read as done or stuck. Protection: equal versions show one value;
    // a pending handover shows running, selected and the attempts it waits for; an older host without the line falls
    // back to fresh diagnostics with its wait unknown, never to the running version alone.
    #[test]
    fn hosts_line_shows_selected_version_and_its_waits_during_handover() {
        let beat = |title: &str, body: &str| board::Issue { title: title.into(), body: body.into(), updated: 1000, trusted: true, ..Default::default() };
        let snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), 1000, &[], &[
            beat("taskq host mac", "version 4.6.62"),
            beat("taskq host win", "version 4.6.61\nselected 4.6.62 waits #12 attempt 41, #15 attempt 52")]);
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let line = text.lines().find(|l| l.starts_with("Hosts: ")).unwrap();
        assert!(line.contains("mac v4.6.62 ") && !line.split(", win").next().unwrap().contains("selected"), "{line}");
        assert!(line.contains("win v4.6.61 (selected 4.6.62 waits #12 attempt 41, #15 attempt 52) "), "{line}");
        let hosts = snap.json()["hosts"].clone();
        assert_eq!((hosts[0].get("selected"), hosts[1]["selected"].as_str()), (None, Some("4.6.62 waits #12 attempt 41, #15 attempt 52")));
        let old = Host { version: "4.6.61".into(), diagnostics: json!({"selected": {"value": "4.6.62", "state": "ok"}}), ..Default::default() };
        assert_eq!(old.version_text(), "4.6.61 (selected 4.6.62 waits unknown: older host)");
        let same = Host { version: "4.6.62".into(), diagnostics: json!({"selected": {"value": "4.6.62", "state": "ok"}}), ..Default::default() };
        assert_eq!(same.version_text(), "4.6.62");
        let stale = Host { version: "4.6.61".into(), diagnostics: json!({"selected": {"value": "4.6.62", "state": "stale"}}), ..Default::default() };
        assert_eq!(stale.version_text(), "4.6.61", "a stale selection is not reported as pending");
    }

    // Chain: #2256 A1 heartbeat `queue` counts -> installation anomaly row. Case: win serves desiign, a project
    // missing from this host's list, and its rows elsewhere are anonymous. Risk: that project drops out of the view.
    #[test]
    fn unseen_projects_are_an_anomaly_row() {
        let w = |n| ("win".to_string(), n);
        assert_eq!(super::unseen(&[vec![w(3)], vec![w(3), ("mac".into(), 1)]]),
            ["- host win · serves 1 project outside this view · Next: add it to `<install>/projects` here (R10) or run `taskq status` on win"]);
        assert!(super::unseen(&[vec![w(2)], vec![w(2)]]).is_empty(), "every served project on a read board: no row");
    }

    // Chain: #2255 heartbeat `queue` -> status `Queues:` rows. Case 2026-11-03: win hung on re-exec 00:42-01:51Z,
    // ready work in taskq and another project never spawned, status showed only `running unknown`. Risk: a host with
    // ready work and free capacity stalls unseen; a short wait, an empty queue or a full host reads as a stall.
    #[test]
    fn queue_rows_flag_the_2026_11_03_win_stall_on_every_project() {
        let at = |t: &str| crate::board::iso(&format!("2026-11-03T{t}:00Z")).unwrap();
        let body = |full: bool, ready: u64, waiting: &str| format!("version 4.6.61\nbalancing {}\nqueue {}",
            json!({"schema": 1, "mode": "any", "default_runtime": "claude", "allowed_runtimes": null, "auto_reroute": false, "has": [], "focus": ["all"], "routing": [],
                "runtimes": crate::runtime::RUNTIMES.iter().map(|rt| (rt.to_string(), json!({"code": "hold", "reason": format!("{rt}: hold; see host diagnostics"),
                    "full": full, "disabled": false, "quota": []}))).collect::<serde_json::Map<_, _>>()}),
            json!({"restart": at("00:42"), "projects": [
                {"project": "taskqorg/taskq", "ready": ready, "spawn": at("00:30"), "waiting": at(waiting), "reason": "host restart pending"},
                {"project": null, "ready": 1, "spawn": null, "waiting": at(waiting), "reason": "host restart pending"}]}));
        let snap = |observed: &str, full, ready, waiting: &str| {
            let beat = board::Issue { title: "taskq host win".into(), body: body(full, ready, waiting), updated: at(observed) - 30, trusted: true, ..Default::default() };
            from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), at(observed), &[], &[beat])
        };
        let stalled = |s: &Snapshot| s.json()["hosts"][0]["queue"].as_array().unwrap().iter().map(|r| r["stalled"] == true).collect::<Vec<_>>();
        let hang = snap("00:55", false, 2, "00:42");
        let text = hang.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("Queues:\n- win taskqorg/taskq ready 2 · last spawn 2026-11-03T00:30:00Z · restart pending 13m · STALLED 13m\n- win other project ready 1 · last spawn none · restart pending 13m · STALLED 13m"), "{text}");
        assert!(text.contains("Anomalies:\n- host win taskqorg/taskq · STALLED 13m: ready 2 without a spawn: host restart pending · Next: inspect the host service (`taskq doctor --hosts`)\n- host win other project · STALLED 13m: ready 1"), "A4: the win hang replay is an anomaly row: {text}");
        assert_eq!(stalled(&hang), [true, true]);
        assert_eq!(stalled(&snap("00:51", false, 2, "00:42")), [false, false], "9 min is not yet a stall");
        assert_eq!(stalled(&snap("00:55", true, 2, "00:42")), [false, false], "a full host is not stalled");
        assert_eq!(stalled(&snap("00:55", false, 0, "00:42")), [false, true], "no ready work in taskq, the other project still waits");
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
        assert!(text.contains(&format!("| [#1 T1](u1) | owner until {}: 168 qualified hours | `taskq requeue 1` |\n| [#2 T2](u2) | owner | `taskq requeue 2` |", local_time(at))), "{text}");
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
        assert!(text.contains("| [#1 T1](u1) · stage 2/4 | owner |") && text.contains("| [#2 T2](u2) · stage 3/4 | no host decision |"), "{text}");
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
        let table: Vec<&str> = text.lines().filter(|l| l.contains("| no host decision |")).collect();
        assert_eq!(table.len(), ready.len(), "{text}");
        assert!(table.iter().zip(&ready).all(|(l, r)| l.starts_with(&format!("| {r}"))), "ready order {ready:?}\n{text}");
        for (t, reason) in rows.iter().filter_map(|t| match t["state"].as_str() { Some("held") => Some((t, "PM")), Some("later") => Some((t, "owner")), _ => None }) {
            assert!(text.lines().any(|l| l.starts_with(&format!("| [#{} ", t["n"])) && l.contains(&format!(") | {reason} |"))), "{text}");
        }
        let ask = rows.iter().find(|t| t["state"] == "ask").unwrap();
        assert!(text.contains(&format!("| owner · stuck {} |", ask["age"].as_str().unwrap())), "{text}");
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
            ("execution 3 on mac: unknown: agent exited; input acceptance unknown; reconcile native input", 6),
        ];
        for (raw, k) in cases { assert_eq!(wait_text(raw), WAIT_TEXTS[k], "{raw}"); }
        // status text and row JSON: owner text only in the row, raw reason kept in --json
        let raw = cases[11].0;
        let row = Row { heartbeat: Value::Null, worker: None, conversation: None, n: 1, title: "T".into(), url: "u1".into(), task: Task { state: State::Doing, ..Default::default() }, execution_wait: Some(raw.into()), ..Default::default() };
        let snap = Snapshot { source: "board", observed: 0, reason: None, board: json!({"url": "b"}), tasks: vec![row], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("| doing · Run state unknown; see service log |") && !text.contains("termination") && !text.contains("execution 3"), "{text}");
        let j = &snap.json()["tasks"][0];
        assert_eq!((j["wait"].as_str(), j["execution_wait"].as_str()), (Some(WAIT_TEXTS[5]), Some(raw)));
        // #2257: a result with no reviewer has no live agent, so an input-class diagnostic reads as the handover
        let handover = Task { state: State::Review, result: Some("abc".into()), ..Default::default() };
        let row = Row { task: handover.clone(), execution_wait: Some(cases[4].0.into()), ..Default::default() };
        assert_eq!(row.wait().as_deref(), Some(WAIT_TEXTS[4]));
        let row = Row { task: Task { attempt: Some(9), ..handover }, execution_wait: Some(cases[4].0.into()), ..Default::default() };
        assert_eq!(row.wait().as_deref(), Some(WAIT_TEXTS[2]), "a live reviewer keeps the input wait");
    }

    // #2159 A4/R1: last accepted worker survives terminal state projection, without becoming current authority.
    #[test]
    fn retained_worker_identity_comes_only_from_accepted_board_projection() {
        let log=vec![core::Entry{id:9,body:"taskq: spawn worker codex:- mac".into(),edited:false},
            core::Entry{id:10,body:"taskq: later\nReason: checkpoint".into(),edited:false},
            core::Entry{id:11,body:"taskq: spawn worker codex:forged other".into(),edited:false}];
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let snap=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[]);
        let row=snap.tasks[0].json();
        assert!(row["attempt"].is_null() && row["session"].is_null(),"read identity cannot restore task authority");
        assert_eq!(row["worker"],json!({"attempt":9,"session":"codex:- mac","role":"worker"}),"ignored spawn cannot select retained history");
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

    // R6/R12, #2173: a fresh lease must not hide missing execution progress.
    // R6/R12, #2173 note 6574454403: an accepted board answer is not a native executor.
    #[test]
    fn logical_doing_after_answer_waits_for_an_exact_execution() {
        let log=vec![core::Entry{id:5,body:"taskq: spawn worker codex:invented win".into(),edited:false},
            core::Entry{id:6,body:"taskq: accepted 5 attempt 5".into(),edited:false},
            core::Entry{id:7,body:"taskq: ask attempt 5\nDone: checked\nProblem: owner decision".into(),edited:false},
            core::Entry{id:8,body:"taskq: released 5 exit attempt 5".into(),edited:false},
            core::Entry{id:9,body:"taskq: answer\nContinue".into(),edited:false}];
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log:log.clone(),comments:vec![],blockers:vec![]};
        let host=board::Issue{trusted:true,title:"taskq host win".into(),updated:1000,body:"#1 5".into(),..Default::default()};
        let row=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[host]).tasks[0].json();
        assert_eq!(row["state"],"doing");assert!(row["executions"].is_null());
        assert_eq!(row["heartbeat"]["executor"]["state"],"awaiting execution");
        assert!(row["heartbeat"]["executor"]["reason"].as_str().unwrap().contains("input recorded"));
        assert_eq!(row["stuck"],false,"ordinary delivery wait is not a failed task");
        assert_eq!(core::executions(1,&log).len(),1,"projection cannot submit another execution");
        let mut log=log;
        log.push(core::Entry{id:10,body:"taskq: executing 9 attempt 5".into(),edited:false});
        log.push(core::Entry{id:11,body:"taskq: accepted 10 attempt 5".into(),edited:false});
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let host=board::Issue{trusted:true,title:"taskq host win".into(),updated:1000,
            body:format!("#1 5\nexecution {}",json!({"task":1,"attempt":5,"id":10,"through":9,"state":"running","reason":"native executor observed"})),..Default::default()};
        for (now,expected) in [(1000,"active"),(1301,"unknown")] {
            let row=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),now,std::slice::from_ref(&v),std::slice::from_ref(&host)).tasks[0].json();
            assert_eq!(row["heartbeat"]["executor"]["state"],expected,"a stale remote executor cannot read active");
        }
    }

    #[test]
    fn heartbeat_health_is_explicit_even_without_an_execution_row() {
        let log=vec![core::Entry{id:5,body:"taskq: spawn worker codex:- mac".into(),edited:false}];
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let snap=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[]);
        let row=snap.tasks[0].json();
        assert_eq!(row["heartbeat"]["freshness"],"unknown");
        assert!(row["heartbeat"]["reason"].as_str().unwrap().contains("missing"));
        assert!(row["heartbeat"]["warning"].is_string());
        assert_eq!(row["state"],"doing","health never changes board state");
    }

    // Chain: R6 manager status -> shared heartbeat warnings. Risk: health is hidden, duplicated or re-detected differently.
    #[test]
    fn stale_heartbeats_block_reuses_shared_health_and_clears() {
        let now=10_000;let mut cfg=crate::tests::cfg();cfg.root="invented-project".into();
        let quiet=json!({"source":"native execution events","clock":"native timestamp or append mtime lower bound","observed":9990,"status":"quiet","last_event":9990,"pending_tools":0,"reason":"no pending tool; quiet work is not failure"});
        let row=|n,host_age:Option<i64>,progress:Value,pending:bool| {
            let task=Task{state:State::Doing,attempt:Some(5),session:Some("codex:invented mac".into()),..Default::default()};
            let executions=if pending {vec![]} else {vec![json!({"id":10,"attempt":5,"host":"mac","observed":now,"state":"running","reason":"native executor observed","progress":progress})]};
            let hosts=host_age.map(|age|vec![board::Issue{trusted:true,title:"taskq host mac".into(),updated:now-age,body:format!("#{n} 5"),..Default::default()}]).unwrap_or_default();
            let mut health=heartbeat(&task,n,&hosts,&executions,now,cfg.ttl);
            health["executor"]=executor(&task,&executions,pending,now,cfg.ttl);refresh_heartbeat(&mut health,now,true);
            Row{n,title:format!("Invented task {n}"),url:format!("https://github.com/invented/project/issues/{n}"),task,executions,heartbeat:health,..Default::default()}
        };
        let mut error=crate::runtime::progress::unknown("native progress record invalid");error["observed"]=json!(9990);
        let overdue=json!({"source":"native execution events","observed":9990,"status":"overdue","last_event":8800,"tool_since":8000,"pending_tools":1,"overdue_since":8900,"reason":"tool progress overdue; preserve execution and inspect owning session"});
        let mut stale=quiet.clone();stale["observed"]=json!(now-cfg.ttl-1);stale["last_event"]=stale["observed"].clone();
        for progress in [&quiet,&stale,&overdue] {assert!(crate::runtime::progress::valid(progress),"fixture retains native future guards");}
        let mut unknown=row(6,Some(10),error.clone(),false);unknown.executions[0]["state"]=json!("unknown");
        unknown.executions[0]["reason"]=json!("native identity unavailable");
        unknown.heartbeat["executor"]=executor(&unknown.task,&unknown.executions,false,now,cfg.ttl);refresh_heartbeat(&mut unknown.heartbeat,now,true);
        let mut question=Row{n:9,title:"Owner question".into(),url:"q9".into(),task:Task{state:State::Ask,card:Some("Problem: owner choice\n1. keep ★\n2. change".into()),..Default::default()},stuck:Some(9000),..Default::default()};
        question.heartbeat=Value::Null;
        let mut snap=Snapshot{source:"board",observed:now,reason:None,board:json!({"url":"https://github.com/invented/project/issues"}),hosts:vec![],
            tasks:vec![row(1,Some(10),quiet.clone(),false),row(2,Some(cfg.ttl+1),quiet.clone(),false),row(3,None,error.clone(),false),row(4,Some(10),error,false),row(5,Some(10),overdue,false),unknown,row(7,Some(10),Value::Null,true),row(8,Some(10),stale,false),question]};
        snap.tasks[5].task.state=State::Review;
        let decide=json!({"complete":true,"rows":[{"n":20,"title":"Completed result","url":"r20","verdict":"confirmed","age_s":600,"recommendation":"Discuss result"}]});
        let text=snap.text_with(&cfg,&json!({}),Some(&decide)).unwrap();
        assert!(text.contains("Anomalies:"),"manager status must expose health without opening a native task detail: {text}");
        assert!(text.find("Owner (").unwrap()<text.find("Decide:").unwrap() && text.find("Decide:").unwrap()<text.find("Anomalies:").unwrap() && text.find("Anomalies:").unwrap()<text.find("Running:").unwrap());
        assert!(text.contains("Completed result") && text.contains("Discuss result"),"completed results remain discoverable");
        assert_eq!(text.lines().filter(|l|l.contains("stuck 2h")).collect::<Vec<_>>(),["| [#9 Owner question](q9) | owner · stuck 2h | answer the Owner row |"],"health tasks must not duplicate the stuck row");
        let block=format!("Anomalies:{}\n",text.split_once("Anomalies:").unwrap().1.split("\n\n").next().unwrap());
        let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/stale-heartbeats.txt");
        if std::env::var_os("TASKQ_BLESS").as_deref()==Some(std::ffi::OsStr::new("1")) {std::fs::write(&path,&block).unwrap();}
        assert_eq!(block,std::fs::read_to_string(path).unwrap().replace("\r\n","\n"));
        assert!(!block.contains("Invented task 1"));
        let long="x".repeat(200);snap.tasks[5].heartbeat["progress"]["clock"]=json!(long);
        assert!(snap.text(&cfg,&json!({})).unwrap().contains(&format!("native execution events ([{}]({}))",cell(&compact(&long,160)),snap.tasks[5].url)),"long source metadata links to its full source");
        snap.tasks[1].heartbeat["observed"]=json!(now+60);refresh_heartbeat(&mut snap.tasks[1].heartbeat,now,true);
        assert!(snap.text(&cfg,&json!({})).unwrap().contains("future by 1m"),"future observations never display as zero age");
        let json_before=snap.json();
        for r in &mut snap.tasks[..8] {
            r.heartbeat["freshness"]=json!("fresh");r.heartbeat["observed"]=json!(now);r.heartbeat["executor"]=json!({"state":"active","observed":now});r.heartbeat["progress"]=quiet.clone();
            refresh_heartbeat(&mut r.heartbeat,now,true);
        }
        assert!(!snap.text(&cfg,&json!({})).unwrap().contains("Anomalies:"),"recovery omits an empty block");
        assert_eq!(json_before["tasks"][4]["heartbeat"]["progress"]["status"],"overdue","JSON/PM consumers retain the same typed projection");
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
            let row = ExecutionStatus { progress: None, task: 1, attempt: 5, id: 7, through: 7, state: got.into(), reason: why.clone() };
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
        // #1930 A4 explicitly removes the issue-creation fallback while retaining trusted/unedited filtering.
        v.comments.push(board::Comment{id:4,created:0,trusted:true,edited:false,..Default::default()});
        assert!(read(View{issue:v.issue.clone(),projection:crate::core::project(1,&[],false),log:vec![],comments:v.comments.clone(),blockers:vec![]}).is_null(),"a missing newest event cannot borrow an older comment time");
        v.comments.clear();assert!(read(v).is_null(),"issue creation is not conversation activity");
    }

}
