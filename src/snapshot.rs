//! Canonical read model (R6, § 4, § 5): one snapshot of the open tasks renders `status`, `status --json`, the
//! `status.json` tasks and totals, `service status` and the tray counts.

use crate::board::{self, Board};
use crate::core::{self, State, Task, options};
use crate::runtime::Cap;
use crate::service::{self, Config, View};
use crate::{Res, cli::text::cell, cli::text::compact, runtime, time::age};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub const SCHEMA: u64 = 1;

/// A5: accepted spawns include reviewer attempts, which do not change the folded state.
pub(crate) fn attempts(
    projection: &core::Projection,
    comments: &[board::Comment],
    role: &str,
) -> Vec<Value> {
    comments
        .iter()
        .filter(|c| projection.accepts(c.id))
        .filter_map(|c| {
            let mut words = c
                .body
                .lines()
                .next()?
                .trim()
                .strip_prefix("taskq:")?
                .split_whitespace();
            if words.next() != Some("spawn") || words.next() != Some(role) {
                return None;
            }
            let session = format!("{} {}", words.next()?, words.next()?);
            Some(json!({"attempt":c.id,"session":session,"role":role,"started":c.created}))
        })
        .collect()
}

/// #1591 the one freshness rule (§ 3 lease, § 4 status, R6) for CLI, tray and menu: an observation `age` seconds old
/// is live within the project ttl, else stale; a negative age (clock skew) is stale.
pub fn live(age: i64, ttl: i64) -> bool {
    (0..=ttl).contains(&age)
}

/// The project ttl of `root`; an unreadable config uses the default.
pub fn ttl(root: &std::path::Path) -> i64 {
    crate::config::load(root).map_or(crate::config::TTL, |(c, _)| c.ttl)
}

/// A `status.json` file at `path` is live when its file age passes [`live`].
pub fn status_live(path: &std::path::Path, ttl: i64) -> bool {
    coreq::status::age(path).is_some_and(|a| live(a as i64, ttl))
}
/// Every open state; `totals` names each one, zero included.
pub const STATES: [&str; 8] = [
    "held", "ready", "waiting", "doing", "review", "ask", "later", "failed",
];
/// What a reader finds in a snapshot of each source.
const BOARD_CAPS: [&str; 4] = ["tasks", "totals", "questions", "hosts"];
const HOST_CAPS: [&str; 6] = ["tasks", "totals", "focus", "capacity", "agents", "events"];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecutionStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub process: Option<(
        &'a coreq::process::Identity,
        Result<Option<coreq::process::Identity>, String>,
    )>,
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

/// A live worker process settles an unknown verdict; a published row always carries a reason.
pub const LIVE_PROCESS: &str = "worker process observed";
const STALE: &str = "termination unconfirmed; status missing or stale; restore owning service";

/// The one execution verdict (§ 3 Recovery): state and a public reason of at most 240 characters.
pub fn verdict(s: &Signals) -> (&'static str, String) {
    let (state, reason): (&'static str, String) =
        if let Some((at, intent)) = s.boot.filter(|(at, intent)| *intent > 0 && at > intent) {
            (
                "ended",
                format!(
                    "host booted {} after intent {}",
                    crate::time::utc(at),
                    crate::time::utc(intent)
                ),
            )
        } else if s.child.as_ref().is_some_and(|c| c.exited) {
            ("ended", "native exit observed".into())
        } else if let Some((recorded, now)) = &s.process {
            match now {
                Err(e) => (
                    "unknown",
                    format!("recorded native process identity unknown: {e}"),
                ),
                Ok(now) if now.as_ref() != Some(*recorded) => {
                    ("ended", "recorded native process exited".into())
                }
                Ok(_) => ("running", "recorded native process observed".into()),
            }
        } else if !s.local {
            match s.beat {
                Some(row)
                    if !row.reason.is_empty()
                        && row.reason.chars().count() <= 240
                        && !row.reason.chars().any(char::is_control) =>
                {
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
                Some(Child {
                    unknown: Some(e), ..
                }) => ("unknown", e.to_string()),
                Some(c) if c.stopping => (
                    "waiting",
                    "cancellation pending; awaiting native termination".into(),
                ),
                _ if s.held => ("waiting", "launch held by admission gate".into()),
                None => (
                    "unknown",
                    s.wait
                        .unwrap_or("native observation unavailable; restore owning runtime")
                        .into(),
                ),
                _ if s.input_pending => ("waiting", "native input acceptance pending".into()),
                _ if s.ask => ("waiting", "owner answer required".into()),
                _ => (
                    "running",
                    "native execution observed; termination pending".into(),
                ),
            }
        };
    (
        state,
        reason
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(240)
            .collect(),
    )
}

/// Board identity and terminal evidence fence every advisory heartbeat observation.
/// A host row counts within the project ttl (§ 3 lease); older reads unknown.
pub(crate) fn execution_status(
    task: u64,
    x: &crate::core::Execution,
    hosts: &[board::Issue],
    now: i64,
    ttl: i64,
) -> Value {
    let owners: Vec<_> = hosts
        .iter()
        .filter(|h| h.trusted && h.title == format!("taskq host {}", x.host))
        .collect();
    let mut beat = None;
    if let [host] = owners[..] {
        {
            let rows = host
                .body
                .lines()
                .filter_map(|l| l.strip_prefix("execution "))
                .map(|text| {
                    if text.len() > 4096 {
                        return Err("oversized execution status".to_string());
                    }
                    serde_json::from_str::<ExecutionStatus>(text).map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>();
            if let Ok(rows) = rows {
                let matching: Vec<_> = rows
                    .into_iter()
                    .filter(|r| {
                        r.task == task
                            && r.attempt == x.attempt
                            && r.id == x.id
                            && r.through == x.through
                    })
                    .collect();
                if let [row] = &matching[..] {
                    beat = Some((row.clone(), host.updated));
                }
            }
        }
    }
    let recovery = (x.release == Some(core::Release::Exit) && !x.accepted)
        .then(|| format!("agent exited; {}", runtime::acceptance_action(&x.runtime)));
    let (state, reason) = if let Some(reason) = &recovery {
        verdict(&Signals {
            local: true,
            wait: Some(reason),
            ..Default::default()
        })
    } else {
        verdict(&Signals {
            beat: beat
                .as_ref()
                .filter(|(_, at)| live(now - *at, ttl))
                .map(|b| &b.0),
            ..Default::default()
        })
    };
    let observed = recovery.as_ref().map(|_| now).or_else(|| {
        beat.as_ref()
            .filter(|_| state != "unknown" || reason != STALE)
            .map(|b| b.1)
    });
    let progress = beat.as_ref().and_then(|b| b.0.progress.clone()).map(|p| {
        if runtime::progress::valid(&p) {
            p
        } else {
            runtime::progress::unknown("execution progress heartbeat invalid")
        }
    });
    let status = ExecutionStatus {
        progress,
        task,
        attempt: x.attempt,
        id: x.id,
        through: x.through,
        state: state.into(),
        reason,
    };
    let mut value = serde_json::to_value(status).unwrap();
    value["host"] = json!(x.host);
    value["runtime"] = json!(x.runtime);
    value["observed"] = json!(observed);
    if recovery.is_some() {
        value["release"] = json!("exit");
    }
    value
}

/// R6: ownership observation and progress remain independent advisory facts.
pub fn heartbeat(
    t: &Task,
    n: u64,
    hosts: &[board::Issue],
    executions: &[Value],
    now: i64,
    ttl: i64,
) -> Value {
    let host = service::host_of(t);
    let owners: Vec<_> = hosts
        .iter()
        .filter(|h| h.trusted && Some(h.title.trim_start_matches("taskq host ")) == host)
        .collect();
    let owner = if let [h] = owners[..] { Some(h) } else { None };
    let observed = owner.map(|h| h.updated);
    let age = observed.map(|at| now - at);
    let leased = owner.is_some_and(|h| {
        t.attempt
            .is_some_and(|a| h.body.lines().any(|l| l == format!("#{n} {a}")))
    });
    let active = matches!(t.state, State::Doing | State::Review) && t.attempt.is_some();
    let (freshness, reason) = if !active {
        ("not applicable", "no live attempt")
    } else if owner.is_none() {
        ("unknown", "owning host heartbeat missing or ambiguous")
    } else if age.is_none_or(|a| !live(a, ttl)) {
        ("stale", "owning host heartbeat stale or time unknown")
    } else if !leased {
        ("unknown", "matching attempt heartbeat lease missing")
    } else {
        (
            "fresh",
            "matching host/attempt ownership lease observed; progress is separate",
        )
    };
    let progress = if !active {
        json!({"source":"not applicable","status":"not applicable","reason":"no live attempt"})
    } else {
        executions
            .iter()
            .find(|x| x["attempt"].as_u64() == t.attempt && x["host"].as_str() == host)
            .and_then(|x| x.get("progress"))
            .cloned()
            .unwrap_or_else(|| runtime::progress::unknown("matching execution progress missing"))
    };
    let mut h = json!({"host":host,"attempt":t.attempt,"source":"board host heartbeat","observed":observed,"age_s":age,
        "freshness":freshness,"reason":reason,"ttl_s":ttl,"progress":progress});
    refresh_heartbeat(&mut h, now, active);
    h
}

/// The board records input intent; only an exact fresh execution observation confirms an executor.
pub fn executor(t: &Task, executions: &[Value], pending: bool, now: i64, ttl: i64) -> Value {
    let current: Vec<_> = executions
        .iter()
        .filter(|x| x["attempt"].as_u64() == t.attempt)
        .collect();
    let (state, reason, observed, source) =
        if !matches!(t.state, State::Doing | State::Review) || t.attempt.is_none() {
            (
                "not applicable",
                "no live attempt",
                None,
                "board task projection",
            )
        } else if current.is_empty() && pending {
            (
                "awaiting execution",
                "input recorded; awaiting execution, no current native work confirmed",
                Some(now),
                "board input projection",
            )
        } else if let [x] = current[..] {
            let observed = x["observed"].as_i64();
            let fresh = observed.is_some_and(|at| live(now - at, ttl));
            let state = if x["release"] == "exit" || !fresh {
                "unknown"
            } else if x["state"] == "running" {
                "active"
            } else if x["state"] == "waiting" && pending {
                "awaiting acceptance"
            } else if x["state"] == "waiting" {
                "waiting"
            } else {
                "unknown"
            };
            (
                state,
                x["reason"]
                    .as_str()
                    .unwrap_or("current executor observation unavailable"),
                observed,
                "owning-host execution observation",
            )
        } else {
            (
                "unknown",
                "current executor missing or ambiguous; logical task state is not native work",
                None,
                "board execution projection",
            )
        };
    json!({"state":state,"reason":reason,"observed":observed,"source":source})
}

/// Re-age cached observations; reading a snapshot never renews either heartbeat.
pub fn refresh_heartbeat(h: &mut Value, now: i64, active: bool) {
    let age = h["observed"].as_i64().map(|at| now - at);
    h["age_s"] = json!(age);
    h["as_of"] = json!(now);
    if active && age.is_some_and(|a| !live(a, h["ttl_s"].as_i64().unwrap_or(0))) {
        h["freshness"] = json!("stale");
        h["reason"] = json!("owning host heartbeat stale or time unknown");
    }
    if h["executor"]["observed"]
        .as_i64()
        .is_some_and(|at| !live(now - at, h["ttl_s"].as_i64().unwrap_or(0)))
    {
        h["executor"]["state"] = json!("unknown");
        h["executor"]["reason"] = json!("current executor observation stale");
    }
    let progress = &h["progress"];
    let progress_live = progress["observed"]
        .as_i64()
        .is_some_and(|at| live(now - at, h["ttl_s"].as_i64().unwrap_or(0)));
    let warning = if !active {
        None
    } else if h["freshness"] != "fresh" {
        h["reason"].as_str()
    } else if matches!(
        h["executor"]["state"].as_str(),
        Some("awaiting execution" | "awaiting acceptance")
    ) {
        h["executor"]["reason"].as_str()
    } else if h["executor"]["state"] == "unknown" {
        Some("current executor unknown; inspect execution health")
    } else if !progress_live {
        Some("execution progress observation missing or stale")
    } else if !matches!(progress["status"].as_str(), Some("quiet" | "active")) {
        progress["reason"]
            .as_str()
            .or(Some("execution progress unknown"))
    } else {
        None
    };
    h["warning"] = json!(warning);
}

#[derive(Default)]
pub struct Row {
    pub heartbeat: Value,
    /// Last accepted worker source for read-only history, never current task authority.
    pub worker: Option<Value>,
    /// Accepted worker sources in board order; read-only history, never execution authority.
    pub workers: Vec<Value>,
    pub reviewers: Vec<Value>,
    pub conversation: Option<Value>,
    pub n: u64,
    pub title: String,
    pub brief: String,
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
    /// Claude Remote Control URL of the current attempt from its host's heartbeat `session` line (§ 4), else that line's `unknown: <cause>` (#2261).
    pub session_url: Option<String>,
    /// Board time of the entry behind the last logged state change (§ 4 Tray age); `None`: no logged change.
    pub since: Option<i64>,
    /// O1: board issue creation and accepted state changes; no inferred execution timings.
    pub created: Option<i64>,
    pub steps: Vec<Value>,
    pub timing: Value,
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
    /// #2288: open tasks a held body names without any `Depends on` line; start it with `Depends on` instead of holding.
    pub unlinked: Vec<u64>,
}

/// § 4 Churn counts of one task's board notes: all notes, `spawn` and `accepted` entries.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Churn {
    pub notes: u64,
    pub spawns: u64,
    pub accepted: u64,
}

/// § 4 Churn: over `CHURN_NOTES` notes, or more than `CHURN_SPAWNS` spawns in the last hour lacking their `accepted`.
pub const CHURN_NOTES: u64 = 100;
pub const CHURN_SPAWNS: usize = 5;

/// § 4 Churn of one task from the comments already read at board time `now`; `None`: not flagged.
pub(crate) fn churn(comments: &[board::Comment], now: i64) -> Option<Churn> {
    let entry = |c: &&board::Comment| c.trusted && !c.edited;
    let kind = |c: &board::Comment, k: &str| {
        c.body
            .lines()
            .next()
            .is_some_and(|l| l.starts_with(&format!("taskq: {k} ")))
    };
    let accepted: Vec<&str> = comments
        .iter()
        .filter(entry)
        .filter(|c| kind(c, "accepted"))
        .filter_map(|c| c.body.split_whitespace().nth(2))
        .collect();
    let spawns: Vec<&board::Comment> = comments
        .iter()
        .filter(entry)
        .filter(|c| kind(c, "spawn"))
        .collect();
    let unaccepted = spawns
        .iter()
        .filter(|c| now - c.created <= 3600 && !accepted.contains(&c.id.to_string().as_str()))
        .count();
    let c = Churn {
        notes: comments.len() as u64,
        spawns: spawns.len() as u64,
        accepted: accepted.len() as u64,
    };
    (c.notes > CHURN_NOTES || unaccepted > CHURN_SPAWNS).then_some(c)
}

impl Churn {
    pub fn line(n: u64, c: &Value) -> String {
        format!(
            "#{n} churn: notes {} · spawns {} · accepted {}",
            c["notes"], c["spawns"], c["accepted"]
        )
    }
}

/// § 4 Churn lines of a `status.json` task list, one per flagged task.
pub fn churn_lines(status: &Value) -> Vec<String> {
    status["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["churn"].is_object())
        .map(|t| Churn::line(t["n"].as_u64().unwrap_or(0), &t["churn"]))
        .collect()
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
    (
        "hold",
        "an emergency hold, draining, a pending restart or an older service holds launches",
    ),
    (
        "host",
        "a host-* label, a labelled claim or assignee-only assigns the task to another host",
    ),
    (
        "capability",
        "a needs-* label or the routed runtime is not available on this host",
    ),
    ("labels", "the task labels are invalid; no host admits it"),
    (
        "focus",
        "the host focus excludes the task or admits nothing",
    ),
    (
        "capacity",
        "runtime, project or host slots are full or another project has the turn",
    ),
    (
        "load-gate",
        "the load gate or launch spacing holds launches",
    ),
    ("login", "the routed runtime is not logged in"),
    (
        "usage-limit",
        "the routed runtime is held after a usage limit",
    ),
    ("access", "the routed runtime refused its access grant"),
    ("unknown", "no decision is known; reason names why"),
];

/// The fixed reasons of `unknown` (A3): a host that did not decide never reads as admitted or refused.
pub const ADMISSION_UNKNOWN: [&str; 4] = [
    "no host decision",
    "capacity config unreadable",
    "host admission unknown",
    "execution unresolved",
];

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
        debug_assert!(
            ADMISSION_CODES.iter().any(|c| c.0 == self.code),
            "admission code outside the vocabulary"
        );
        json!({"code": self.code, "observed": self.observed, "host": self.host, "reason": self.reason})
    }

    /// The `taskq-board/1` form: free reason text stays on the host; `unknown` keeps its fixed reason.
    pub fn projection(&self) -> Value {
        let mut v = json!({"code": self.code, "observed": self.observed, "host": self.host});
        if self.code == "unknown" {
            v["reason"] = json!(self.reason);
        }
        v
    }
}

pub const ROUTING_WAIT: &str =
    "runtime reroute pending; service rechecks writer ownership before completion";

pub const START_LABELS: &str = "start eligibility labels incomplete";

/// The board-only admission of a row: `waiting` is the fold's dependency decision, `ready` waits for a host decision
/// unless its `host-<name>` label names no heartbeat (#1721): no host ever claims it.
fn board_admission(
    t: &Task,
    labels: &[String],
    deps: &[service::Blocker],
    hosts: &[board::Issue],
    observed: i64,
) -> Option<Admission> {
    let orphan = labels
        .iter()
        .filter_map(|l| l.strip_prefix("host-"))
        .find(|h| !hosts.iter().any(|i| service::host_name(i) == Some(*h)));
    let (code, reason) = match t.state {
        State::Ready | State::Waiting if !labels.iter().any(|l| l == service::TASK_LABEL) => {
            ("labels", START_LABELS.into())
        }
        State::Waiting => (
            "dependency",
            deps.iter()
                .map(|b| format!("{} {}", b.dep.name(), b.reason))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        State::Ready => match orphan {
            Some(h) => ("host", format!("no host {h}")),
            None => ("unknown", ADMISSION_UNKNOWN[0].to_string()),
        },
        _ => return None,
    };
    Some(Admission {
        code,
        reason,
        observed,
        host: None,
    })
}

/// Owner texts of a row's wait (docs/design/menu.md M3 Task row text, #1524); the raw reason stays in `execution_wait`.
pub const WAIT_TEXTS: [&str; 7] = [
    "Waiting for a free slot",
    "Waiting for GUI access",
    "Waiting for the agent to accept input",
    "Waiting for your answer",
    "Waiting for the previous run to end",
    "Run state unknown; see service log",
    "Agent exited; input recovery required",
];

/// #1820 owner text of a publication wait, followed by its age and reason.
pub const PUBLICATION_WAIT: &str = "Publication waiting";

/// The owner text of a raw wait diagnostic; text no class names reads as the previous run's end.
pub fn wait_text(raw: &str) -> &'static str {
    let has = |w: &[&str]| w.iter().any(|w| raw.contains(w));
    WAIT_TEXTS[if has(&["agent exited;"]) {
        6
    } else if has(&["GUI"]) {
        1
    } else if has(&["admission"]) {
        0
    } else if has(&["owner answer"]) {
        3
    } else if has(&["input"]) {
        2
    } else if has(&["unknown", "stale", "missing", "unavailable", "deferred"]) {
        5
    } else {
        4
    }]
}

/// A `review` result with no reviewer attempt and no pending publication: no reviewer spawned yet (§ 2 row 13).
fn handover(t: &Task) -> bool {
    t.state == State::Review && t.result.is_some() && t.attempt.is_none() && t.publication.is_none()
}

/// R6: no movement for this long marks a task stuck.
pub const STUCK_AFTER: i64 = 2 * 3600;

/// Assignment survives an unaccepted Exit; its pending input is blocked regardless of the host lease's age.
pub(crate) fn input_recovery(v: &View) -> Option<&core::Execution> {
    let t = &v.projection.task;
    if !matches!(t.state, State::Doing | State::Review) {
        return None;
    }
    let attempt = t.attempt?;
    if !t
        .inbox
        .iter()
        .any(|id| *id > v.projection.delivered(attempt).unwrap_or(0))
    {
        return None;
    }
    v.projection
        .executions
        .iter()
        .find(|x| x.attempt == attempt && x.release == Some(core::Release::Exit) && !x.accepted)
}

/// R6 stuck age of task `n` whose newest log entry is at `last`: observe only, never a gate.
pub(crate) fn stuck(
    n: u64,
    t: &Task,
    deps: &[service::Blocker],
    last: i64,
    hosts: &[board::Issue],
    now: i64,
    ttl: i64,
) -> Option<i64> {
    let age = now - last;
    // § 3 lease: a fresh heartbeat lists `#<N> <attempt>`
    let leased = || {
        t.attempt.is_some_and(|a| {
            hosts.iter().any(|h| {
                h.trusted
                    && live(now - h.updated, ttl)
                    && h.body.lines().any(|l| l == format!("#{n} {a}"))
            })
        })
    };
    match t.state {
        State::Waiting => deps.iter().all(|b| b.reason.starts_with("unknown: closed")),
        State::Held | State::Ask | State::Later => age >= STUCK_AFTER,
        // #1752: a review whose reviewer no host leases waits silently otherwise
        State::Doing | State::Review => age >= STUCK_AFTER && !leased(),
        _ => false,
    }
    .then_some(age)
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
    /// § 3 Runtime accounts: `use|avoid|unmet <rt> <id>` or `local <reason>` of this host's fleet policy.
    pub account_policy: Vec<String>,
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
    /// Heartbeat `daemon-sandbox {json}` (#2432): each daemon that differs from the grant of this project's jobs.
    pub daemon_sandbox: Vec<Value>,
    /// The project ttl this row was read with: the heartbeat's liveness.
    pub ttl: i64,
}

/// R6 anomaly kinds whose source defect has an open held fix task (#2256 answer 1): the row names it.
// ponytail: drop each entry once its task lands; the anomaly row then shows without a fix link.
pub const ANOMALY_FIXES: [(&str, u64); 4] = [
    ("remote-control", 2261),
    ("progress", 2262),
    ("running", 2264),
    ("start-labels", 2362),
];

/// R10 A1: hosts serving projects no board of this view carries: each host's largest `queue` row count against the
/// boards that carry its heartbeat. Rows of other projects stay anonymous (private names), so only counts compare.
pub fn unseen(boards: &[Vec<(String, usize)>]) -> Vec<String> {
    let mut hosts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (name, n) in boards.iter().flatten() {
        let e = hosts.entry(name).or_default();
        e.0 = e.0.max(*n);
        e.1 += 1;
    }
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
        let free = runtimes.map(|r| {
            r.values()
                .any(|v| v["full"] == false && v["disabled"] == false)
        });
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
        if let Some(s) = &self.selected {
            return format!("{} (selected {s})", self.version);
        }
        let d = &self.diagnostics;
        match d["selected"]["value"].as_str() {
            Some(v) if d["selected"]["state"] == "ok" && v != self.version => {
                format!("{} (selected {v} waits unknown: older host)", self.version)
            }
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
    /// The [`board::Reason::code`] of a failed board read.
    pub reason_code: Option<&'static str>,
    pub board: Value,
    pub tasks: Vec<Row>,
    pub hosts: Vec<Host>,
}

/// #1918: how many newest op ids a row lists; more writes than this between two board reads of one task are not expected.
const OPS: usize = 16;

/// The one snapshot function: open tasks by priority then number, plus host heartbeats.
pub fn from_views(
    source: &'static str,
    b: &dyn Board,
    cfg: &Config,
    observed: i64,
    views: &[View],
    hosts: &[board::Issue],
) -> Snapshot {
    let mut open: Vec<&View> = views
        .iter()
        .filter(|v| v.projection.task.state != State::Closed)
        .collect();
    open.sort_by_key(|v| (service::priority(&v.issue), v.issue.number));
    let urls = session_urls(hosts);
    let numbers: Vec<u64> = open.iter().map(|v| v.issue.number).collect();
    let tasks = open.into_iter().map(|v| {
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
                            value["reason"] = json!(LIVE_PROCESS);
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
        workers: attempts(&v.projection, &v.comments, "worker"),
        reviewers: attempts(&v.projection, &v.comments, "reviewer"),
        n: v.issue.number,
        conversation: core::conversation(v.issue.number,&v.log).map(|id| json!({"id":id,"brief":v.issue.body,"verdict":v.projection.changes.iter().rev().find_map(|(id,_,_)|v.log.iter().find(|e|e.id==*id && ["taskq: result ","taskq: close","taskq: publish"].iter().any(|prefix|e.body.starts_with(prefix))).and_then(|e|e.body.split_once('\n').map(|(_,text)|core::verdict(v.issue.number,&v.log,text))))})),
        title: v.issue.title.clone(),
        brief: v.issue.body.clone(),
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
        history: crate::dispatch::timeline(v.issue.number, &v.log, &v.projection, &v.comments),
        ops: v.log.iter().rev().filter_map(|e| crate::core::op_id(v.issue.number, &e.body)).take(OPS).map(String::from).collect(),
        publication: None,
        stage: crate::core::stage(&v.issue.body, &v.projection.task.stages),
        session_url: v.projection.task.attempt.zip(v.projection.task.session.as_deref().and_then(|s| s.rsplit(' ').next()))
            .filter(|_| v.projection.task.session.as_deref().is_some_and(|s| crate::runtime::remote_session(s).is_some()))
            .and_then(|(a, h)| urls.get(&(h.to_string(), a)).cloned()),
        stuck: recovery.map(|_|observed-last).or(missing_reviewer.as_ref().map(|_|observed-last)).or_else(||stuck(v.issue.number, &v.projection.task, &v.blockers,last,hosts, observed, cfg.ttl)),
        churn: churn(&v.comments, observed),
        verdict: v.projection.task.result.as_ref().and_then(|_| v.projection.changes.iter().rev()
            .find_map(|(id, _, _)| v.log.iter().find(|e| e.id == *id && e.body.starts_with("taskq: result "))))
            .map(|e| core::entry_text(v.issue.number, &e.body).to_string()),
        unlinked: if v.projection.task.state == State::Held { unlinked(v.issue.number, &v.issue.body, &numbers) } else { vec![] },
        created: Some(v.issue.created).filter(|t| *t > 0),
        steps: stage_history(v),
        timing: task_timing(&v.projection,&v.comments,false,None),
        since: v.projection.changes.last().and_then(|(id, ..)| v.comments.iter().find(|c| c.id == *id)).map(|c| c.created),
        failed: (v.projection.task.state == State::Failed).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: failed")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
        later: (v.projection.task.state == State::Later).then(|| v.comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: later")).and_then(|c| c.body.split_once('\n')).map(|x| x.1.to_string()).unwrap_or_default()),
    }}).collect();
    let hosts = hosts
        .iter()
        .map(|h| host(h, observed, cfg.ttl as i64))
        .collect();
    Snapshot {
        source,
        observed,
        reason: None,
        reason_code: None,
        board: identity(b, cfg),
        tasks,
        hosts,
    }
}

/// #2288: open task numbers `#N` in a held `body` that has no `Depends on` line (§ 1); a local `#N` only, never `repo#N`.
// ponytail: names only open tasks of this snapshot; an open non-task issue goes unflagged
fn unlinked(n: u64, body: &str, open: &[u64]) -> Vec<u64> {
    if !core::depends(body).is_empty() {
        return vec![];
    }
    let mut out: Vec<u64> = body
        .match_indices('#')
        .filter(|(i, _)| {
            !body[..*i].ends_with(|c: char| c.is_alphanumeric() || c == '-' || c == '_' || c == '/')
        })
        .filter_map(|(i, _)| {
            body[i + 1..]
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .filter(|m| *m != n && open.contains(m))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// One host of a heartbeat issue `h` read at `observed`.
pub fn host(h: &board::Issue, observed: i64, ttl: i64) -> Host {
    Host {
        balancing: crate::service::balancing_status(&h.body),
        diagnostics: crate::service_versions::read(&h.body, h.updated, observed, ttl, false),
        name: h.title.trim_start_matches("taskq host ").to_string(),
        service: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("service "))
            .map(str::to_string),
        version: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("version "))
            .unwrap_or("?")
            .to_string(),
        updated: h.updated,
        focus: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("focus "))
            .map(str::to_string),
        // usage rows share the `limit ` prefix; only a hold line is a limit here
        limits: h
            .body
            .lines()
            .filter(|l| crate::runtime::usage::Row::parse(l).is_none())
            .filter_map(|l| {
                l.strip_prefix("limit ")
                    .map(str::to_string)
                    .or_else(|| l.starts_with("quota ").then(|| l.to_string()))
            })
            .collect(),
        account_policy: h
            .body
            .lines()
            .filter_map(|l| l.strip_prefix("account-policy "))
            .map(str::to_string)
            .collect(),
        tray: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("tray "))
            .map(str::to_string),
        requests: h.body.lines().find_map(|l| {
            l.strip_prefix("requests ")?
                .strip_suffix("/h")?
                .parse::<u64>()
                .ok()
        }),
        github: h.body.lines().find_map(|l| {
            l.strip_prefix("requests api.github.com ")?
                .strip_suffix("/h")?
                .parse::<u64>()
                .ok()
        }),
        // only a heartbeat the board trusts may name a key a host serves
        link: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("link "))
            .filter(|_| h.trusted)
            .map(str::to_string),
        doctor: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("doctor "))
            .map(str::to_string),
        logins: h
            .body
            .lines()
            .filter_map(|l| l.strip_prefix("login ")?.strip_suffix(" off"))
            .map(str::to_string)
            .collect(),
        issue: h.number,
        answer: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("doctor-answer "))
            .map(str::to_string),
        reports: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("doctor-reports "))
            .map(str::to_string),
        accounts: h
            .body
            .lines()
            .filter_map(|l| l.strip_prefix("account "))
            .map(str::to_string)
            .collect(),
        selected: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("selected "))
            .map(str::to_string),
        queue: h
            .body
            .lines()
            .find_map(|l| l.strip_prefix("queue "))
            .filter(|_| h.trusted)
            .and_then(|j| serde_json::from_str(j).ok()),
        daemon_sandbox: h
            .body
            .lines()
            .filter_map(|l| l.strip_prefix("daemon-sandbox "))
            .filter(|_| h.trusted)
            .filter_map(|j| serde_json::from_str(j).ok())
            .collect(),
        ttl,
    }
}

/// O1: first accepted worker ownership and real state boundaries, never comment count or issue age.
pub(crate) fn task_timing(
    projection: &core::Projection,
    comments: &[board::Comment],
    completed: bool,
    closed: Option<i64>,
) -> Value {
    let at = |id: u64| {
        comments
            .iter()
            .find(|c| c.id == id && c.trusted && !c.edited)
            .map(|c| c.created)
            .filter(|t| *t > 0)
    };
    let workers = projection
        .changes
        .iter()
        .filter(|(id, _, t)| t.role.as_deref() == Some("worker") && t.attempt == Some(*id))
        .map(|(id, _, _)| *id)
        .collect::<Vec<_>>();
    let first = projection
        .accepted_inputs
        .iter()
        .filter(|(execution, _)| {
            projection
                .executions
                .iter()
                .any(|x| x.id == *execution && workers.contains(&x.attempt))
        })
        .map(|(_, entry)| *entry)
        .min();
    let started = if first.is_some() {
        Some(true)
    } else if workers.is_empty() {
        Some(false)
    } else {
        None
    };
    let completed = completed || projection.task.state == State::Closed;
    let extra_close = completed && projection.task.state != State::Closed;
    let boundary = projection
        .changes
        .last()
        .filter(|(_, _, t)| t.state == projection.task.state)
        .and_then(|(id, _, _)| at(*id));
    json!({"started":started,"first":first.and_then(at),
        "changes":first.filter(|_|boundary.is_some()||extra_close).map(|id|projection.changes.iter().filter(|(entry,_,_)|*entry>id).count()+usize::from(extra_close)),
        "current":if extra_close {closed.filter(|t|*t>0)} else {boundary},
        "completed":completed,"closed":closed.filter(|t|*t>0)})
}

/// O1: ordered board stages, with unknown boundaries retained. The fold still owns transitions.
fn stage_history(v: &View) -> Vec<Value> {
    let mut steps = Vec::new();
    let mut state = v
        .projection
        .changes
        .first()
        .map_or(v.projection.task.state, |(_, before, _)| before.state);
    let mut start = Some(v.issue.created).filter(|t| *t > 0);
    for (id, _, after) in &v.projection.changes {
        let end = v
            .comments
            .iter()
            .find(|c| c.id == *id)
            .map(|c| c.created)
            .filter(|t| *t > 0);
        let valid = start.zip(end).is_some_and(|(a, b)| b >= a);
        steps.push(json!({"state": state.name(), "start": if valid { start } else { None }, "end": end, "complete": true}));
        state = after.state;
        start = end;
    }
    steps.push(json!({"state": state.name(), "start": start, "end": null, "complete": false}));
    steps
}

/// `session #<N> <attempt> <url>|unknown: <cause>` lines of trusted heartbeats (§ 4) by host name and attempt: a canonical claude.ai
/// URL, or the `unknown: <cause>` text (#2261), which is never a link.
pub fn session_urls(hosts: &[board::Issue]) -> HashMap<(String, u64), String> {
    hosts
        .iter()
        .filter(|h| h.trusted)
        .flat_map(|h| {
            let name = h.title.trim_start_matches("taskq host ").to_string();
            h.body.lines().filter_map(move |l| {
                let (_, rest) = l.strip_prefix("session #")?.split_once(' ')?;
                let (a, value) = rest.split_once(' ')?;
                (runtime::remote_url(value) || value.starts_with("unknown: ")).then_some(())?;
                Some(((name.clone(), a.parse().ok()?), value.to_string()))
            })
        })
        .collect()
}

fn identity(b: &dyn Board, cfg: &Config) -> Value {
    json!({"kind": cfg.board.name, "host": cfg.web_host, "repo": cfg.repo, "url": b.url(0).trim_end_matches("/0")})
}

/// `taskq status`: one board read; a failed read is an incomplete snapshot with its reason, never zero tasks.
pub fn read(b: &dyn Board, cfg: &Config, raw: &Value, only: &[String]) -> Snapshot {
    read_cached(b, cfg, raw, only, HashMap::new())
}

/// [`read`] with comments already fetched (#2291: `taskq status` reads them in parallel).
pub fn read_cached(
    b: &dyn Board,
    cfg: &Config,
    raw: &Value,
    only: &[String],
    mut comments: service::CommentCache,
) -> Snapshot {
    let mut attempt = || -> board::Result<Snapshot> {
        let (mut views, hosts) = service::views(b, &mut comments, &mut HashMap::new(), cfg.ttl)?;
        views.retain(|v| in_scope(&v.issue, only));
        if let Some(who) = raw["assignee"].as_str() {
            let who = if who == "me" {
                b.me()?
            } else {
                who.to_string()
            };
            let mut keep = vec![];
            for v in views {
                if service::assigned(b, &v.issue, &who)? {
                    keep.push(v);
                }
            }
            views = keep;
        }
        Ok(from_views("board", b, cfg, b.now()?, &views, &hosts))
    };
    attempt().unwrap_or_else(|e| Snapshot {
        source: "board",
        observed: b.now().unwrap_or(0),
        reason: Some(e.to_string()),
        reason_code: Some(e.reason().code()),
        board: identity(b, cfg),
        tasks: vec![],
        hosts: vec![],
    })
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
    format!(
        "In work {} · Waiting for answer {} · Ready {}",
        t["doing"] + t["review"],
        t["ask"],
        t["ready"]
    )
}

/// A `status.json` is complete when its service's last pass read the board; N-1 files have no `complete` key.
pub fn complete(status: &Value) -> bool {
    status["complete"]
        .as_bool()
        .unwrap_or(status["error"].is_null())
        && status["tasks"].is_array()
}

/// Totals of a `status.json`; `None` when it is incomplete (unknown is never zero).
pub fn status_totals(status: &Value) -> Option<BTreeMap<&'static str, usize>> {
    complete(status).then(|| {
        totals(
            status["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["state"].as_str()),
        )
    })
}

impl Row {
    fn health_warning(&self) -> Option<&str> {
        matches!(self.task.state, State::Doing | State::Review)
            .then(|| self.heartbeat["warning"].as_str())
            .flatten()
    }
    /// R6: ` · stage k/n` for a task with recorded stages (§ 2 Stages), else nothing.
    fn stage_text(&self) -> String {
        self.stage
            .map(|(k, n)| format!(" · stage {k}/{n}"))
            .unwrap_or_default()
    }

    /// The owner text of the row's wait: a publication wait, else the execution diagnostic, else a capacity refusal with its runtime (#1685),
    /// else the board's own host refusal (R6, #1721).
    fn wait(&self) -> Option<String> {
        // #1820: the reviewer has ended, so a publication wait outranks its execution diagnostic
        if let Some(p) = &self.publication {
            return Some(format!(
                "{} {} · {}",
                PUBLICATION_WAIT,
                p.age_s.map_or("?".into(), crate::time::age),
                compact(&p.reason, 200)
            ));
        }
        if let Some(reason) = &self.execution_wait {
            // #2257: the worker-to-reviewer handover has no live agent to accept input
            let text = wait_text(reason);
            return Some(
                if text == WAIT_TEXTS[2] && handover(&self.task) {
                    WAIT_TEXTS[4]
                } else {
                    text
                }
                .into(),
            );
        }
        let a = self.admission.as_ref()?;
        match (a.code, &a.host) {
            ("capacity", _) => Some(format!("{} · {}", WAIT_TEXTS[0], a.reason)),
            ("host", None) => Some(a.reason.replacen("no host", "No host", 1)),
            ("login" | "usage-limit" | "access" | "capability", _) => Some(a.reason.clone()),
            _ => None,
        }
    }

    pub fn stuck_age(&self) -> Option<i64> {
        self.stuck.or_else(|| {
            let h = &self.heartbeat;
            (matches!(self.task.state, State::Doing | State::Review)
                && h["freshness"] == "fresh"
                && h["progress"]["status"] == "overdue"
                && h["progress"]["observed"]
                    .as_i64()
                    .zip(h["as_of"].as_i64())
                    .is_some_and(|(at, now)| live(now - at, h["ttl_s"].as_i64().unwrap_or(0))))
            .then(|| {
                h["as_of"].as_i64().unwrap_or(0)
                    - h["progress"]["overdue_since"].as_i64().unwrap_or(0)
            })
        })
    }

    pub fn json(&self) -> Value {
        let mut row = json!({"n": self.n, "title": self.title, "state": self.task.state.name(), "labels": self.labels, "url": self.url, "revision": self.revision, "updated": self.updated,
            "session": self.task.session, "attempt": self.task.attempt, "role": self.task.role,
            "deps": self.deps.iter().filter(|b| b.dep.repo.is_none() && b.reason == "open").map(|b| b.dep.n).collect::<Vec<_>>(),
            "blocked": self.deps.iter().map(|b| json!({"ref": b.dep.name(), "url": b.url, "reason": b.reason})).collect::<Vec<_>>(),
            "card": self.task.card, "question": self.task.card.as_deref().map(question), "failed": self.failed,
            "heartbeat":self.heartbeat,"controls": controls(self.task.state.name()), "verdict": self.verdict.as_deref().map(verdict_fields)});
        if !self.brief.is_empty() {
            row["brief"] = json!(self.brief);
        }
        if let Some(worker) = &self.worker {
            row["worker"] = worker.clone();
        }
        if !self.workers.is_empty() {
            row["workers"] = json!(self.workers);
        }
        if !self.reviewers.is_empty() {
            row["reviewers"] = json!(self.reviewers);
        }
        if let Some(c) = &self.conversation {
            row["conversation"] = c.clone();
        }
        if let Some(id) = &self.task.enqueued {
            row["enqueued"] = json!(id);
        }
        if let Some(sha) = &self.task.result {
            row["result"] = json!(sha);
        }
        // R6 timed park: the UTC time and its reason; clients show it in local time
        if let Some(u) = &self.task.until {
            row["until"] = json!(u);
            row["later"] = json!(self.later);
        }
        // R6 / § 2 Stages: `stage k/n` and the finished stages
        if let Some((k, n)) = self.stage {
            row["stage"] = json!({"k": k, "n": n, "done": self.task.stages});
        }
        // § 2: the PM's pick sits beside the worker's star; it is never an answer
        if let Some((k, reason)) = &self.task.pick {
            row["pick"] = json!({"id": k, "reason": reason});
        }
        if !self.executions.is_empty() {
            row["executions"] = json!(self.executions);
        }
        if let Some(reason) = &self.execution_wait {
            row["execution_wait"] = json!(reason);
        }
        if let Some(step) = &self.missing_reviewer {
            row["missing_reviewer"] = json!(step);
        }
        if let Some(w) = self.wait() {
            row["wait"] = json!(w);
        }
        if !self.unlinked.is_empty() {
            row["unlinked"] = json!(self.unlinked);
        }
        if let Some(u) = self.session_url.as_ref().filter(|u| runtime::remote_url(u)) {
            row["session_url"] = json!(u);
        }
        row["since"] = json!(self.since);
        row["created"] = json!(self.created);
        row["steps"] = json!(self.steps);
        row["timing"] = self.timing.clone();
        row["stuck"] = json!(self.stuck_age().is_some());
        if let Some(a) = self.stuck_age() {
            row["age_s"] = json!(a);
        }
        if let Some(c) = self.churn {
            row["churn"] = json!({"notes": c.notes, "spawns": c.spawns, "accepted": c.accepted});
        }
        if let Some(a) = &self.admission {
            row["admission"] = a.json();
        }
        if let Some(a) = &self.answered {
            row["answered"] = json!(a);
        }
        if !self.history.is_empty() {
            row["history"] = json!(self.history.iter().map(|(c, a)| json!({"question": c.as_deref().map_or(Value::Null, question), "answered": a})).collect::<Vec<_>>());
        }
        if !self.ops.is_empty() {
            row["ops"] = json!(self.ops);
        }
        if let Some(p) = &self.publication {
            row["publication"] = json!({"reason": p.reason, "since": p.since, "age_s": p.age_s});
        }
        row
    }
}

/// § 7 typed result verdict (status-report.md Snapshot contract): `Changed:` and `Open:` text (1-2 lines, joined by a
/// newline) and one `Checks:` item per line, each without its prefix. A line without a prefix continues the field
/// before it; another `Word: ` field (`Verdict:`, `Report:`) or the blank line before a card ends it. Missing reads null.
pub fn verdict_fields(text: &str) -> Value {
    const KEYS: [(&str, &str); 3] = [
        ("Changed: ", "changed"),
        ("Checks: ", "checks"),
        ("Open: ", "open"),
    ];
    let mut f: [Vec<String>; 3] = Default::default();
    let mut cur = None;
    for l in text.lines().map(str::trim_end) {
        if l.trim().is_empty() {
            if cur.is_some() { break } else { continue }
        }
        if let Some((i, rest)) = KEYS
            .iter()
            .enumerate()
            .find_map(|(i, k)| l.strip_prefix(k.0).map(|r| (i, r)))
        {
            cur = Some(i);
            if i == 1 || f[i].is_empty() {
                f[i].push(rest.trim().to_string());
                continue;
            }
            f[i].last_mut().unwrap().push_str(&format!(
                "
{}",
                rest.trim()
            ));
        } else if l.split_once(": ").is_some_and(|(w, _)| {
            !w.is_empty()
                && w.starts_with(|c: char| c.is_ascii_uppercase())
                && w.chars().all(|c| c.is_alphanumeric())
        }) {
            cur = None;
        } else if let Some(last) = cur.and_then(|i| f[i].last_mut()) {
            last.push_str(&format!(
                "
{}",
                l.trim()
            ));
        }
    }
    let one = |v: &Vec<String>| v.first().cloned();
    json!({"changed": one(&f[0]), "checks": f[1], "open": one(&f[2])})
}

/// Published vocabularies of the snapshot (status-report.md Snapshot contract): open states in counter order with
/// owner name and projected `q-` label, and the telemetry error, stage error and runtime codes readers accept.
pub fn vocabulary() -> Value {
    let label = |s: &str| {
        if s == "waiting" {
            "q-blocked".to_string()
        } else {
            format!("q-{s}")
        }
    };
    json!({"states": crate::display::STATES.iter().map(|(id, name)| json!({"id": id, "name": name, "label": label(id)})).collect::<Vec<_>>(),
        "telemetry": {"errors": crate::profile::ERRORS, "stage_errors": crate::profile::STAGE_ERRORS, "runtimes": crate::profile::RUNTIMES}})
}

/// Published task controls `taskq.task-controls/1` (docs/design/task-controls.md): action, CLI command, fixed reason,
/// states that offer it. Close is never a control: it stays a board operation.
const LIVE: &[&str] = &["ready", "waiting", "doing", "review", "ask"];
pub const CONTROLS: [(&str, &str, &str, &[&str]); 4] = [
    ("start", "start", "", &["held"]),
    ("stop", "failed", "Reason: Stopped by the owner.", LIVE),
    ("later", "later", "Reason: Parked by the owner.", LIVE),
    (
        "retry",
        "requeue",
        "Reason: Retried by the owner.",
        &["later", "failed"],
    ),
];

/// Control actions a row in `state` offers, in [`CONTROLS`] order.
pub fn controls(state: &str) -> Vec<&'static str> {
    CONTROLS
        .iter()
        .filter(|c| c.3.contains(&state))
        .map(|c| c.0)
        .collect()
}

/// § 5 board line width; human input has no line-count cap.
pub const ANSWER_CHARS: usize = 240;

/// § 2 visual evidence: the service's copy of an ask image `url`, which the app (no board token) shows inline.
pub fn image_copy(root: &std::path::Path, url: &str) -> std::path::PathBuf {
    let key: String = ring::digest::digest(&ring::digest::SHA256, url.as_bytes()).as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    crate::paths::dot(root).join("images").join(key)
}

/// The image URLs of a card's Evidence line.
pub fn card_images(card: &str) -> Vec<String> {
    question(card)["evidence"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["image"] == true)
        .filter_map(|e| e["url"].as_str().map(String::from))
        .collect()
}

/// Canonical parsed fields of an open card (§ 2, docs/design/answer-card.md), the one parse every card UI reads.
/// `parsed` false: a reader shows `text` with the board link and no write controls.
pub fn question(card: &str) -> Value {
    let field = |p: &str| card.lines().find_map(|l| l.strip_prefix(p)).map(str::trim);
    let evidence: Vec<Value> = field("Evidence: ")
        .unwrap_or("")
        .split("), ")
        .filter_map(|l| {
            // `![alt](url)`: an image the card UIs show inline (§ 2 visual evidence)
            let (image, l) = l.strip_prefix('!').map_or((false, l), |l| (true, l));
            let (label, url) = l.strip_prefix('[')?.split_once("](")?;
            let url = url.trim_end_matches(')');
            // a video the board plays inline (#1844) stays a link in the apps
            let video = [".mp4", ".webm"]
                .iter()
                .any(|e| url.to_ascii_lowercase().ends_with(e));
            Some(json!({"label": label, "url": url, "image": image && !video}))
        })
        .collect();
    let o = options(card);
    // Publication and exact-SHA approval choices stay on the board.
    // § 2: `<label> - <detail>`; a card without ` - ` (written before #1787, or a service card) shows its whole text as the label
    let choices: Vec<Value> = o.iter().map(|(k, t, star)| {
        let (label, detail) = t.split_once(" - ").map_or((t.as_str(), None), |(l, d)| (l.trim(), Some(d.trim())));
        json!({"id": k, "text": t, "label": label, "detail": detail, "recommended": star, "consequential": t.to_lowercase().starts_with("close") || core::approval_sha(t).is_some()})
    }).collect();
    // § 2: options numbered 1..n in order, no gap or repeat; none is an owner action answered with text
    let numbered = o.iter().enumerate().all(|(i, x)| x.0 == i + 1);
    let parsed = field("Problem: ").is_some()
        && numbered
        && (o.is_empty() || (2..=4).contains(&o.len()) && o.iter().filter(|x| x.2).count() == 1);
    json!({"parsed": parsed, "text": card, "done": field("Done: "), "problem": field("Problem: ").map(|_| brief(card)), "evidence": evidence,
        "choices": choices, "limits": {"chars": ANSWER_CHARS}})
}

// Chain: § 7 result verdict -> typed snapshot fields (#2014). Risk: a consumer re-parses verdict text and drifts.
#[test]
fn verdict_fields_split_changed_checks_and_open() {
    let v = verdict_fields(
        "Changed: a\nb\nVerdict: confirmed x\nChecks: t -- pass\nChecks: u -- fail\nOpen: none\nReport: docs/research/7.md\n\nDone: d\nProblem: p\n1. x ★",
    );
    assert_eq!(
        v,
        json!({"changed": "a\nb", "checks": ["t -- pass", "u -- fail"], "open": "none"})
    );
    let w = vocabulary();
    assert_eq!(
        w["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "doing", "review", "ask", "failed", "ready", "waiting", "later", "held"
        ],
        "counter order"
    );
    assert_eq!(w["states"][5]["label"], "q-blocked");
    assert!(
        w["telemetry"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "argument-too-long")
    );
    assert_eq!(
        verdict_fields("free text"),
        json!({"changed": null, "checks": [], "open": null}),
        "a hand-written result without fields reads null"
    );
}

#[test]
fn review_approval_choices_require_the_board() {
    for action in [
        "close: ship".to_string(),
        format!("approve {}", "a".repeat(40)),
    ] {
        let q = question(&format!(
            "Problem: Decide\n1. {action} ★\n2. keep reviewing"
        ));
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
        for r in self
            .tasks
            .iter_mut()
            .filter(|r| r.admission.as_ref().is_some_and(|a| a.code == "unknown"))
        {
            let Some(a) = status["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|t| t["n"] == r.n)
                .map(|t| &t["admission"])
            else {
                continue;
            };
            let Some(code) = ADMISSION_CODES
                .iter()
                .map(|c| c.0)
                .find(|c| *c != "unknown" && a["code"] == *c)
            else {
                continue;
            };
            r.admission = Some(Admission {
                code,
                reason: a["reason"].as_str().unwrap_or_default().into(),
                observed: a["observed"].as_i64().unwrap_or(self.observed),
                host: a["host"].as_str().map(String::from),
            });
        }
    }

    /// R10 A1: each live host's count of served projects (its heartbeat `queue` rows) as read on this board.
    pub fn served(&self) -> Vec<(String, usize)> {
        self.hosts
            .iter()
            .filter(|h| live(self.observed - h.updated, h.ttl))
            .filter_map(|h| h.queue.as_ref().map(|q| (h.name.clone(), q.projects.len())))
            .collect()
    }

    /// `status --json` (§ 5).
    pub fn json(&self) -> Value {
        let caps: &[&str] = if self.source == "host" {
            &HOST_CAPS
        } else {
            &BOARD_CAPS
        };
        json!({"schema": SCHEMA, "source": self.source, "observed": self.observed, "complete": self.reason.is_none(), "reason": self.reason, "reason_code": self.reason_code,
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
        [
            ("schema", json!(SCHEMA)),
            ("source", json!("host")),
            ("observed", json!(self.observed)),
            ("complete", json!(reason.is_none())),
            ("reason", json!(reason)),
            ("totals", json!(reason.is_none().then(|| self.totals()))),
            (
                "tasks",
                json!(self.tasks.iter().map(Row::json).collect::<Vec<_>>()),
            ),
            ("hosts", self.json()["hosts"].clone()),
            ("vocabulary", vocabulary()),
        ]
    }

    /// R6 text report.
    pub fn text(&self, cfg: &Config, raw: &Value) -> Res<String> {
        self.text_with(cfg, raw, None)
    }

    /// R6 text report with the `Decide:` part of [`crate::reports::decide`] after Questions (#1979).
    pub fn text_with(&self, cfg: &Config, raw: &Value, decide: Option<&Value>) -> Res<String> {
        self.text_extra(cfg, raw, decide, vec![])
    }

    /// [`Self::text_with`] plus anomaly rows the caller read outside the snapshot (deploy, board quota; R6 A3).
    pub fn text_extra(
        &self,
        cfg: &Config,
        raw: &Value,
        decide: Option<&Value>,
        extra: Vec<String>,
    ) -> Res<String> {
        if let Some(r) = &self.reason {
            return Err(self
                .reason_code
                .map_or_else(|| r.clone(), |code| format!("{r} (reason: {code})")));
        }
        let base = self.board["url"].as_str().unwrap_or_default();
        let project = cfg
            .root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut out = vec![
            format!("{project} · [board]({base})"),
            counts_line(&self.totals()),
        ];
        let link = |r: &Row| format!("[#{} {}]({})", r.n, cell(&compact(&r.title, 60)), r.url);
        fn part(out: &mut Vec<String>, head: &str, rows: Vec<String>) {
            if !rows.is_empty() {
                out.push(String::new());
                out.push(head.into());
                out.extend(rows);
            }
        }
        // R6 Owner: questions and review cards, each with the answer command
        let owner: Vec<String> = self
            .tasks
            .iter()
            .filter(|r| {
                r.task.state == State::Ask
                    || (r.task.state == State::Review && r.task.card.is_some())
            })
            .map(|r| {
                let card = r.task.card.clone().unwrap_or_default();
                let (n, url) = (r.n, &r.url);
                let opts = options(&card);
                let o: Vec<String> = opts
                    .iter()
                    .map(|(k, t, star)| {
                        let label = cell(&compact(t, 120));
                        let label = if t.chars().count() > 120 {
                            format!("[{label}]({url})")
                        } else {
                            label
                        };
                        format!("{n}.{k} {label}{}", if *star { " ★" } else { "" })
                    })
                    .collect();
                // § 2: an `ask` card without options is an owner action
                let action = opts.is_empty() && r.task.state == State::Ask;
                let o = if action {
                    cell(&format!(
                        "waiting for owner: {}",
                        compact(&brief(&card), 120)
                    ))
                } else {
                    o.join(" · ")
                };
                let o = match &r.task.pick {
                    Some((k, why)) => format!("{o} · PM {n}.{k}: {}", cell(&compact(why, 160))),
                    None => o,
                };
                let next = match (&r.task.pick, opts.iter().find(|x| x.2)) {
                    (Some((k, _)), _) => format!("`taskq answer {n}.{k}`"),
                    (None, Some((k, _, _))) => format!("`taskq answer {n}.{k}`"),
                    _ if action => format!("`taskq answer {n} --text <done>`"),
                    _ => format!("answer on [the board]({url})"),
                };
                format!(
                    "| {} | [{}]({}) | {o} | {next} |",
                    link(r),
                    cell(&compact(&brief(&card), 160)),
                    url
                )
            })
            .collect();
        part(
            &mut out,
            "Owner (answer N.M):\n| Task | Question | Options | Next |\n|---|---|---|---|",
            owner,
        );
        // R6 PM: Decide, then one Held and stuck list with its reason (A5)
        let decide = decide.and_then(decide_text);
        let (decide, decide_unknown) = match decide {
            Some(d) if d.starts_with("Decide: unknown") => (None, Some(d)),
            d => (d, None),
        };
        let held: Vec<String> = self
            .tasks
            .iter()
            .filter_map(|r| {
                let n = r.n;
                let (reason, next) = match r.task.state {
                    State::Held if !r.unlinked.is_empty() => {
                        let m: Vec<String> = r.unlinked.iter().map(|m| format!("#{m}")).collect();
                        (
                            format!("PM: names open {} without Depends on", m.join(", ")),
                            format!("add `Depends on {}`, then `taskq start {n}`", m[0]),
                        )
                    }
                    State::Held => ("PM".to_string(), format!("`taskq start {n}`")),
                    State::Waiting => {
                        let deps: Vec<String> = r
                            .deps
                            .iter()
                            .map(|b| {
                                format!(
                                    "[{}]({}) {}",
                                    b.dep.name(),
                                    b.url,
                                    cell(&compact(&b.reason, 80))
                                )
                            })
                            .collect();
                        (
                            format!("dependency ({})", deps.join(", ")),
                            if r.stuck.is_some() {
                                "re-read the dependencies; every blocker is closed".into()
                            } else {
                                "wait for the dependency".into()
                            },
                        )
                    }
                    State::Later => (
                        match r.task.until.as_deref().and_then(crate::time::iso) {
                            Some(at) => format!(
                                "owner {} · until {}: {}",
                                crate::time::left(at - self.observed),
                                crate::time::local(at, self.observed),
                                cell(&compact(
                                    r.later
                                        .as_deref()
                                        .unwrap_or("")
                                        .trim_start_matches("Reason: "),
                                    160
                                ))
                            ),
                            None => "owner".into(),
                        },
                        format!("`taskq requeue {n}`"),
                    ),
                    State::Ask if r.stuck.is_some() => {
                        ("owner".into(), "answer the Owner row".into())
                    }
                    _ => return None,
                };
                let stuck = r
                    .stuck
                    .map(|a| format!(" · stuck {}", age(a)))
                    .unwrap_or_default();
                Some(format!(
                    "| {}{} | {reason}{stuck} | {next} |",
                    link(r),
                    r.stage_text()
                ))
            })
            .collect();
        let mut pm = vec![];
        if let Some(d) = decide {
            pm.push(d);
        }
        if !held.is_empty() {
            pm.push(format!(
                "Held and stuck:\n| Task | Reason | Next |\n|---|---|---|\n{}",
                held.join("\n")
            ));
        }
        if !pm.is_empty() {
            out.push(String::new());
            out.push("PM:".into());
            out.push(pm.join("\n\n"));
        }
        // R6 Anomalies (A3/A4): every stale, unknown or missing source, with its reason, action and open fix task
        let fix = |kind: &str| {
            ANOMALY_FIXES
                .iter()
                .find(|f| f.0 == kind)
                .map(|f| {
                    format!(
                        " · fix [taskq#{}](https://github.com/taskqorg/taskq/issues/{})",
                        f.1, f.1
                    )
                })
                .unwrap_or_default()
        };
        let doctor = "Next: inspect the host service (`taskq doctor --hosts`)";
        let mut anomalies = vec![];
        for h in &self.hosts {
            let fresh = live(self.observed - h.updated, cfg.ttl);
            if !fresh {
                anomalies.push(format!(
                    "- host {} · heartbeat stale {} · {doctor}",
                    h.name,
                    age(self.observed - h.updated)
                ));
                continue;
            }
            if h.focus.as_deref() == Some("unknown")
                && h.balancing["mode"] != crate::host::ClaimMode::None.name()
            {
                anomalies.push(format!(
                    "- host {} · focus unknown: it admits nothing · {doctor}",
                    h.name
                ));
            }
            let rts: Vec<&String> = h.balancing["weights"]
                .as_object()
                .into_iter()
                .flat_map(|w| w.keys())
                .collect();
            for (kind, key, what) in [
                ("running", "running", "running count"),
                ("headroom", "headroom", "quota headroom"),
            ] {
                let missing: Vec<&str> = rts
                    .iter()
                    .filter(|rt| {
                        let l = &h.balancing["runtimes"][rt.as_str()]["load"];
                        l[key].is_null() && (key != "headroom" || l["headroom_reason"].is_null())
                    })
                    .map(|rt| rt.as_str())
                    .collect();
                if !missing.is_empty() {
                    anomalies.push(format!(
                        "- host {} · {what} not reported for {} · {doctor}{}",
                        h.name,
                        missing.join(", "),
                        fix(kind)
                    ));
                }
            }
            for q in h.queue_rows(self.observed) {
                let name = cell(&compact(
                    q["project"].as_str().unwrap_or("other project"),
                    80,
                ));
                if q["stalled"] == true {
                    anomalies.push(format!(
                        "- host {} {name} · STALLED {}: ready {} without a spawn: {} · {doctor}",
                        h.name,
                        age(q["waiting"].as_i64().unwrap_or(0)),
                        q["ready"],
                        cell(&compact(
                            q["reason"].as_str().unwrap_or("no reason logged"),
                            160
                        ))
                    ));
                } else if q["restart"].as_i64().is_some_and(|r| r >= STALL_AFTER) {
                    anomalies.push(format!(
                        "- host {} {name} · restart pending {} · {doctor}",
                        h.name,
                        age(q["restart"].as_i64().unwrap_or(0))
                    ));
                }
            }
            for d in &h.daemon_sandbox {
                let bin = cell(&compact(d["bin"].as_str().unwrap_or("runtime"), 40));
                let tasks = d["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|n| format!("#{n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let reason = cell(&compact(
                    d["reason"].as_str().unwrap_or("no reason reported"),
                    160,
                ));
                let next = format!(
                    "Next: jobs wait until the daemon exits; run `{bin} daemon status` on {}",
                    h.name
                );
                anomalies.push(match d["pid"].as_u64() {
                    Some(pid) => format!("- host {} · live {bin} daemon pid {pid} sandbox differs from the grant of {tasks}: {reason} · {next}", h.name),
                    None => format!("- host {} · {bin} daemon sandbox unknown for {tasks}: {reason} · {next}", h.name),
                });
            }
        }
        if let Some(d) = decide_unknown {
            anomalies.push(format!(
                "- {} · Next: rerun `taskq status` after the next service pass",
                d.trim_end()
            ));
        }
        let stamp = |v: &Value| {
            v.as_i64()
                .filter(|at| *at > 0)
                .map(crate::time::utc)
                .unwrap_or(crate::time::UNKNOWN.into())
        };
        for r in &self.tasks {
            if r.admission
                .as_ref()
                .is_some_and(|a| a.code == "labels" && a.reason == START_LABELS)
            {
                anomalies.push(format!(
                    "- {} · log {}; {START_LABELS} · Next: `taskq start {}`{}",
                    link(r),
                    r.task.state.name(),
                    r.n,
                    fix("start-labels")
                ));
            }
            if let Some(warning) = r.health_warning() {
                let field = |v: &Value| {
                    let full = v.as_str().unwrap_or("unknown");
                    let text = cell(&compact(full, 160));
                    if full.chars().count() > 160 {
                        format!("[{text}]({})", r.url)
                    } else {
                        text
                    }
                };
                let h = &r.heartbeat;
                let p = &h["progress"];
                let now = h["as_of"].as_i64().unwrap_or(self.observed);
                let observed = |v: &Value| {
                    v.as_i64()
                        .filter(|at| *at > 0)
                        .map(|at| format!("{} ago · {}", age(now.saturating_sub(at)), stamp(v)))
                        .unwrap_or(crate::time::UNKNOWN.into())
                };
                let rt = r
                    .task
                    .session
                    .as_deref()
                    .and_then(|s| s.split_once(':'))
                    .map(|p| p.0)
                    .unwrap_or("unknown");
                let current: Vec<_> = r
                    .executions
                    .iter()
                    .filter(|x| x["attempt"].as_u64() == r.task.attempt)
                    .map(|x| {
                        x["id"]
                            .as_u64()
                            .map(|id| id.to_string())
                            .unwrap_or("unknown".into())
                    })
                    .collect();
                let execution = match current.len() {
                    0 => "unknown".into(),
                    1 => current[0].clone(),
                    _ => format!("{} (ambiguous)", current.join(", ")),
                };
                let action = if h["freshness"] != "fresh" {
                    "Inspect the owning host heartbeat and service status; preserve execution."
                } else if matches!(
                    h["executor"]["state"].as_str(),
                    Some("awaiting execution" | "awaiting acceptance")
                ) {
                    "Inspect input acceptance on the owning host; do not resubmit."
                } else if p["status"] == "overdue" {
                    "Inspect the pending tool in the owning session; preserve execution."
                } else {
                    "Inspect the owning session and native progress source; preserve execution."
                };
                let source_fix = if h["freshness"] == "fresh"
                    && !matches!(p["status"].as_str(), Some("quiet" | "active" | "overdue"))
                {
                    fix("progress")
                } else {
                    String::new()
                };
                anomalies.push(format!(
                    "- [{} #{} {}]({}) · {} · Next: {action}{source_fix}",
                    cell(&project),
                    r.n,
                    cell(&compact(&r.title, 60)),
                    r.url,
                    cell(&compact(warning, 160))
                ));
                anomalies.push(format!(
                    "  Task: runtime {} · host {} · attempt {} · execution {execution}",
                    cell(rt),
                    field(&h["host"]),
                    r.task
                        .attempt
                        .map(|a| a.to_string())
                        .unwrap_or("unknown".into())
                ));
                anomalies.push(format!(
                    "  Heartbeat: {} · {} · {} · {}",
                    field(&h["freshness"]),
                    field(&h["source"]),
                    observed(&h["observed"]),
                    field(&h["reason"])
                ));
                anomalies.push(format!(
                    "  Executor: {} · {} · {}",
                    field(&h["executor"]["state"]),
                    field(&h["executor"]["source"]),
                    observed(&h["executor"]["observed"])
                ));
                let source = if p["clock"].is_string() {
                    format!("{} ({})", field(&p["source"]), field(&p["clock"]))
                } else {
                    field(&p["source"])
                };
                anomalies.push(format!(
                    "  Progress: {} · {source} · {} · last event {} · {}",
                    field(&p["status"]),
                    observed(&p["observed"]),
                    stamp(&p["last_event"]),
                    field(&p["reason"])
                ));
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
                    // #2261: the owning host's heartbeat names why the link is missing; none means that host never read it.
                    let why = r
                        .session_url
                        .as_deref()
                        .and_then(|u| u.strip_prefix("unknown: "))
                        .map_or_else(
                            || "not in the heartbeat".to_string(),
                            |c| format!("unknown: {}", cell(&compact(c, 160))),
                        );
                    anomalies.push(format!("- {} · {host} Remote Control URL {why} · Next: open the session on {host}{}", link(r), fix("remote-control")));
                }
            }
        }
        anomalies.extend(extra);
        part(&mut out, "Anomalies:", anomalies);
        // R6 Running: who acts on each doing/review row
        let running: Vec<String> = self
            .tasks
            .iter()
            .filter(|r| {
                r.task.state == State::Doing
                    || (r.task.state == State::Review && r.task.card.is_none())
            })
            .map(|r| {
                let actor = match r.task.state {
                    State::Review if r.task.enqueued.is_some() || r.publication.is_some() => {
                        "merge queue"
                    }
                    _ if r.task.session.is_none() => "none",
                    State::Review => "reviewer",
                    _ => "worker",
                };
                let sha = r
                    .task
                    .result
                    .as_deref()
                    .map(|s| format!(" · result {}", &s[..s.len().min(7)]))
                    .unwrap_or_default();
                let wait = r
                    .wait()
                    .map(|w| format!(" · {}", cell(&w)))
                    .unwrap_or_default();
                let health = if r.health_warning().is_some() {
                    " · see Anomalies"
                } else {
                    ""
                };
                let (rt, s, _) = session_link(cfg, raw, r);
                let next = if r.health_warning().is_some() || actor == "none" {
                    "see Anomalies".into()
                } else if r.task.result.is_some() {
                    format!("`taskq results {} --json`", r.n)
                } else {
                    "wait".into()
                };
                format!(
                    "| {} | {}{sha}{wait}{}{health} | {actor} | {} | {s} | {next} |",
                    link(r),
                    r.task.state.name(),
                    r.stage_text(),
                    cell(&rt)
                )
            })
            .collect();
        part(
            &mut out,
            "Running:\n| Task | State | Actor | Runtime | Session | Next |\n|---|---|---|---|---|---|",
            running,
        );
        // R6 Ready: why the service has not started it yet
        let ready: Vec<String> = self
            .tasks
            .iter()
            .filter(|r| r.task.state == State::Ready)
            .map(|r| {
                let a = r.admission.as_ref();
                // #2290: each live host's own reason from its heartbeat queue row, so another host's refusal shows too;
                // a board-level refusal (orphan host label, routing writer) holds on every host and keeps its own text
                let board = a.is_some_and(|a| {
                    a.host.is_none() && (a.code == "host" || a.reason == ROUTING_WAIT)
                });
                let hosts: Vec<(&str, &String)> = self
                    .hosts
                    .iter()
                    .filter(|h| !board && live(self.observed - h.updated, cfg.ttl))
                    .filter_map(|h| {
                        Some((
                            h.name.as_str(),
                            h.queue
                                .as_ref()?
                                .projects
                                .iter()
                                .find(|p| p.project.as_deref() == Some(&cfg.repo))?
                                .tasks
                                .get(&r.n)?,
                        ))
                    })
                    .collect();
                // every host keeps its share of the 160-character reason
                let hosts: Vec<String> = hosts
                    .iter()
                    .map(|(h, why)| {
                        format!(
                            "{h}: {}",
                            compact(
                                why,
                                (150 / hosts.len())
                                    .saturating_sub(h.chars().count() + 2)
                                    .max(12)
                            )
                        )
                    })
                    .collect();
                let why = (!hosts.is_empty())
                    .then(|| hosts.join("; "))
                    .or_else(|| r.wait())
                    .or_else(|| {
                        a.map(|a| {
                            if a.reason.is_empty() {
                                a.code.to_string()
                            } else if a.code == "unknown" {
                                a.reason.to_string()
                            } else {
                                format!("{}: {}", a.code, a.reason)
                            }
                        })
                    })
                    .unwrap_or_else(|| "no host decision".into());
                let next = match a.map(|a| a.code) {
                    Some("admitted") => "starting",
                    Some("capacity" | "load-gate") => "wait for a free slot",
                    Some("host" | "capability" | "labels" | "focus") => {
                        "fix the labels or the host focus"
                    }
                    Some("login") => "log the runtime in on the host",
                    Some("usage-limit") => "wait for the usage limit reset",
                    Some("access") => "check the runtime access grant",
                    Some("hold") => "wait for the hold to clear",
                    Some("dependency") => "wait for the dependency",
                    _ => "wait for the next host pass",
                };
                format!(
                    "| {}{} | {} | {next} |",
                    link(r),
                    r.stage_text(),
                    cell(&compact(&why, 160))
                )
            })
            .collect();
        part(
            &mut out,
            "Ready:\n| Task | Why not started | Next |\n|---|---|---|",
            ready,
        );
        let failed: Vec<String> = self
            .tasks
            .iter()
            .filter(|r| r.task.state == State::Failed)
            .map(|r| {
                format!(
                    "| {} | [{}]({}) | `taskq requeue {}` |",
                    link(r),
                    cell(&compact(r.failed.as_deref().unwrap_or(""), 160)),
                    r.url,
                    r.n
                )
            })
            .collect();
        part(
            &mut out,
            "Failed:\n| Task | Reason | Next |\n|---|---|---|",
            failed,
        );
        // #1625: a heartbeat older than ttl is an anomaly row; unknown load values move there too
        let hs: Vec<String> = self
            .hosts
            .iter()
            .map(|h| {
                format!(
                    "{} v{} {}{}",
                    h.name,
                    h.version_text(),
                    age(self.observed - h.updated),
                    h.focus
                        .as_ref()
                        .filter(|f| *f != "unknown")
                        .map(|f| format!(" focus {f}"))
                        .unwrap_or_default()
                        + &h.limits
                            .iter()
                            .map(|l| if l.starts_with("quota ") {
                                format!(" {l}")
                            } else {
                                l.split_once(' ').map_or_else(
                                    || format!(" {l}"),
                                    |(rt, at)| {
                                        format!(
                                            " {rt} limit {}",
                                            at.split(' ')
                                                .map(|w| if w.ends_with('Z') {
                                                    crate::time::shown(w)
                                                } else {
                                                    w.to_string()
                                                })
                                                .collect::<Vec<_>>()
                                                .join(" ")
                                        )
                                    },
                                )
                            })
                            .collect::<String>()
                        + &h.account_policy
                            .iter()
                            .filter(|l| l.starts_with("unmet "))
                            .map(|l| format!(" account-policy {l}"))
                            .collect::<String>()
                        + &h.balancing["weights"]
                            .as_object()
                            .map(|weights| weights
                                .iter()
                                .map(|(rt, w)| {
                                    let load = &h.balancing["runtimes"][rt]["load"];
                                    let running = load["running"]
                                        .as_u64()
                                        .map(|n| format!(" running {n} /"))
                                        .unwrap_or_default();
                                    format!(
                                        " {rt}{running} weight {w}{}",
                                        load["headroom"]
                                            .as_f64()
                                            .map(|n| format!(" (headroom {:.1}%)", n * 100.0))
                                            .or_else(|| load["headroom_reason"]
                                                .as_str()
                                                .map(|c| format!(" (headroom n/a: {})", cell(c))))
                                            .unwrap_or_default()
                                    )
                                })
                                .collect::<String>())
                            .unwrap_or_default()
                        + &h.tray
                            .as_ref()
                            .map(|code| format!(" tray {code}"))
                            .unwrap_or_default()
                        + &h.requests
                            .as_ref()
                            .map(|n| format!(
                                " {n} req/h{}",
                                h.github
                                    .map(|g| format!(" (github.com {g})"))
                                    .unwrap_or_default()
                            ))
                            .unwrap_or_default()
                )
            })
            .collect();
        if !hs.is_empty() {
            out.push(String::new());
            out.push(format!("Hosts: {}", hs.join(", ")));
        }
        // quiet rows (no ready work, no pending restart) stay in `--json` only; stale hosts read in Anomalies
        let queues: Vec<String> = self
            .hosts
            .iter()
            .filter(|h| live(self.observed - h.updated, cfg.ttl))
            .flat_map(|h| {
                h.queue_rows(self.observed)
                    .into_iter()
                    .filter(|r| r["ready"] != 0 || r["restart"].is_i64())
                    .map(move |r| {
                        let when = |v: &Value| v.as_i64().map_or("none".into(), |s| age(s));
                        format!(
                            "- {} {} ready {} · last spawn {} · restart pending {}{}",
                            h.name,
                            r["project"].as_str().unwrap_or("other project"),
                            r["ready"],
                            r["spawn"].as_i64().map_or("none".into(), crate::time::utc),
                            when(&r["restart"]),
                            if r["stalled"] == true {
                                format!(" · STALLED {}", when(&r["waiting"]))
                            } else {
                                String::new()
                            }
                        )
                    })
            })
            .collect();
        part(&mut out, "Queues:", queues);
        Ok(out.join("\n"))
    }
}

/// R6 Decide: one row per undecided report, as [`crate::reports::decide`] orders them; unknown collection says so.
pub fn decide_text(d: &Value) -> Option<String> {
    if d["complete"] != true {
        return Some(format!(
            "Decide: unknown ({})",
            compact(
                d["reason"].as_str().unwrap_or("reports not collected yet"),
                160
            )
        ));
    }
    let rows: Vec<String> = d["rows"]
        .as_array()?
        .iter()
        .map(|r| {
            format!(
                "| [#{} {}]({}) | {} | {} | {} |",
                r["n"],
                cell(&compact(r["title"].as_str().unwrap_or(""), 60)),
                r["url"].as_str().unwrap_or(""),
                r["verdict"].as_str().unwrap_or("none"),
                age(r["age_s"].as_i64().unwrap_or(0)),
                cell(r["recommendation"].as_str().unwrap_or(""))
            )
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    Some(format!(
        "Decide:\n| Report | Verdict | Age | Recommendation |\n|---|---|---|---|\n{}",
        rows.join("\n")
    ))
}

/// The `Problem:` text of a card, else its first line.
pub fn brief(card: &str) -> String {
    let mut lines = card.lines();
    while let Some(l) = lines.next() {
        if let Some(p) = l.strip_prefix("Problem: ") {
            return std::iter::once(p)
                .chain(lines.take_while(|l| {
                    !l.starts_with("Evidence:")
                        && !l.starts_with("To answer:")
                        && options(l).is_empty()
                }))
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    card.lines().next().unwrap_or_default().to_string()
}

/// Runtime, session cell and, for a remote session without its Remote Control URL, the host (an anomaly row).
fn session_link(cfg: &Config, raw: &Value, r: &Row) -> (String, String, Option<String>) {
    let (t, n) = (&r.task, r.n);
    let Some((rt, id)) = t
        .session
        .as_deref()
        .and_then(|s| s.split(' ').next())
        .and_then(|s| s.split_once(':'))
    else {
        return (String::new(), String::new(), None);
    };
    let host = t
        .session
        .as_deref()
        .and_then(|s| s.rsplit(' ').next())
        .unwrap_or_default();
    let own = host == cfg.me;
    let (id, url) = runtime::session_link(
        &cfg.root,
        own,
        rt,
        id,
        n,
        t.attempt.unwrap_or(0),
        t.role.as_deref().unwrap_or_default(),
        r.session_url.clone(),
    );
    let host = cell(host);
    let link = match rt {
        _ if id == "-" && r.accepted => format!("{host}: running"),
        _ if id == "-" => format!("{host}: starting"),
        _ if runtime::of(rt).has(Cap::RemoteControl) => match url {
            Some(url) => format!("[{host}]({url})"),
            None => return (rt.to_string(), host.clone(), Some(host)),
        },
        // R6 3: a Codex thread opens only on its own host; elsewhere the DashQ progress page, else the host alone
        _ if !own => crate::enrollment::progress_page(&cfg.install, &cfg.repo)
            .map_or_else(|| host.clone(), |p| format!("[{host}]({p}{n})")),
        _ => match url {
            None => host.clone(),
            Some(url)
                if std::env::var("TASKQ_CLIENT").is_ok_and(|c| {
                    runtime::of(&c).has(Cap::ThreadLink) && runtime::of(&c).has(Cap::Cli)
                }) =>
            {
                format!("[{host}]({url})")
            }
            Some(url) => match raw["pages"].as_str() {
                Some(p) => format!("[{host}]({}/open.html#{url})", p.trim_end_matches('/')),
                None => format!("{host}: `{url}`"),
            },
        },
    };
    (rt.to_string(), link, None)
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
