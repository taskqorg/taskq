//! Polled account usage and fleet account policy ([capacity.md](../docs/design/capacity.md#account-usage-and-fleet-policy)).
//! One host thread polls each enabled account; the rows it publishes carry a fingerprint, never an email or a token.
//! A poll only observes: it writes no login file and clears no quota record or hold.
use crate::runtime::Cap;
use super::accounts;
use crate::board::{Board, Comment, Issue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The last poll per runtime and label, in `<install>`.
pub const FILE: &str = "account-usage.json";
/// The approved board's policy and other hosts' rows, written by the speaking project, in `<install>`.
pub const FLEET: &str = "account-fleet.json";
/// The board's one policy issue carries this label.
pub const POLICY_LABEL: &str = "taskq-accounts";
const POLICY_TITLE: &str = "taskq account policy";
const POLICY_BODY: &str = "TaskQ fleet account policy (docs/design/capacity.md): `taskq accounts fleet use|avoid|clear <runtime> <id>` comments here.\n\nKeep this issue. Never edit or delete its comments.";
/// Poll and fleet refresh period, seconds.
pub const EVERY: i64 = 120;
pub const DEFAULT_FLOOR: u8 = 15;
/// A reading older than this is unknown: the same expiry as `quota-headroom.json`.
const TTL: i64 = 300;
/// ponytail: wall clocks of hosts differ; a reading this far ahead still counts, more reads unknown.
const SKEW: i64 = 60;
/// A fleet file older than this keeps local ranking: two refreshes missed and more.
const FLEET_TTL: i64 = 600;
/// A comment written in the second of a read may be missing from it; the views cache settles the same way.
const SETTLE: i64 = 30;
const RPC_DEADLINE: Duration = Duration::from_secs(30);
const HTTP_DEADLINE: u64 = 20;
/// ponytail: `claude auth status` starts a process; a login changes identity rarely.
const ID_REFRESH: i64 = 600;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub name: String,
    /// Whole percent used, 0..100.
    pub used: u8,
    pub reset: Option<i64>,
}

/// One account's poll: readable windows, per-window diagnostics, or a whole-response failure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poll {
    pub id: Option<String>,
    pub windows: Vec<Window>,
    pub unreadable: Vec<String>,
    pub unknown: Option<String>,
}

impl Poll {
    fn failed(reason: &str) -> Poll { Poll { unknown: Some(reason.into()), ..Default::default() } }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Record {
    id: Option<String>,
    at: i64,
    windows: Vec<Window>,
    /// A partial poll's readable windows. A binary through v4.6.79 ignores this field, so it republishes none of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    partial: Vec<Window>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unreadable: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unreadable_at: Option<i64>,
    unknown: Option<String>,
}

type Store = BTreeMap<String, BTreeMap<String, Record>>;

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn sha256(bytes: &[u8]) -> String { hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref()) }

/// The account `id`: the first 8 hex digits of SHA-256 over the lowercased email or account id.
pub fn fingerprint(identity: &str) -> String { sha256(identity.trim().to_lowercase().as_bytes())[..8].to_string() }

pub fn valid_id(id: &str) -> bool { id.len() == 8 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }

fn valid_window(name: &str) -> bool {
    !name.is_empty() && name.len() <= 40 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn percent(used: f64) -> u8 { used.round().clamp(0.0, 100.0) as u8 }

fn fresh(at: i64, reset: Option<i64>, now: i64) -> bool {
    now - at <= TTL && at - now <= SKEW && reset.is_none_or(|r| now < r)
}

// ---------- heartbeat rows ----------

#[derive(Clone, Debug, PartialEq)]
pub enum Usage {
    Window { window: Window, at: i64 },
    /// A readable window of an incomplete poll: published, never aggregated. Its `limit-partial` prefix keeps it from
    /// readers through v4.6.79, which ignore `unknown` rows and would take the readable windows for the whole account.
    Partial { window: Window, at: i64 },
    Unknown(String),
}

/// One heartbeat `limit <rt> <label> id <id|-> ...` or `limit-partial <rt> <label> id <id|-> window ...` row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rt: String,
    pub label: String,
    pub id: Option<String>,
    pub usage: Usage,
}

impl Row {
    pub fn line(&self) -> String {
        let head = |kind: &str| format!("{kind} {} {} id {}", self.rt, self.label, self.id.as_deref().unwrap_or("-"));
        let reading = |kind: &str, window: &Window, at: i64| format!("{} window {} used {} reset {} at {}", head(kind), window.name, window.used,
            window.reset.map_or("-".to_string(), crate::board::stamp), crate::board::stamp(at));
        match &self.usage {
            Usage::Window { window, at } => reading("limit", window, *at),
            Usage::Partial { window, at } => reading("limit-partial", window, *at),
            Usage::Unknown(reason) => format!("{} unknown {reason}", head("limit")),
        }
    }

    /// A usage row; `None` for anything else, the `limit <rt> until|probe <time>` hold line included.
    pub fn parse(line: &str) -> Option<Row> {
        let (partial, body) = match line.strip_prefix("limit-partial ") {
            Some(body) => (true, body),
            None => (false, line.strip_prefix("limit ")?),
        };
        let w: Vec<&str> = body.split(' ').collect();
        let [rt, label, "id", id, rest @ ..] = w.as_slice() else { return None };
        if !accounts::applies(rt) || !accounts::valid_label(label) || !(*id == "-" || valid_id(id)) { return None; }
        let time = |t: &str| crate::board::iso(t).filter(|at| crate::board::stamp(*at) == t);
        let usage = match rest {
            ["window", name, "used", used, "reset", reset, "at", at] if valid_window(name) => {
                let window = Window { name: name.to_string(), used: used.parse().ok().filter(|u| *u <= 100)?,
                    reset: if *reset == "-" { None } else { Some(time(reset)?) } };
                let at = time(at)?;
                if partial { Usage::Partial { window, at } } else { Usage::Window { window, at } }
            }
            ["unknown", reason @ ..] if !partial && !reason.is_empty() => Usage::Unknown(reason.join(" ")),
            _ => return None,
        };
        Some(Row { rt: rt.to_string(), label: label.to_string(), id: (*id != "-").then(|| id.to_string()), usage })
    }

    /// The usage cell of `taskq accounts`.
    pub fn text(&self) -> String {
        match &self.usage {
            Usage::Window { window, .. } | Usage::Partial { window, .. } => format!("{} used {}% reset {}", window.name, window.used, window.reset.map_or("-".to_string(), crate::board::stamp)),
            Usage::Unknown(reason) => format!("unknown {reason}"),
        }
    }
}

// ---------- fleet policy ----------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuntimePolicy {
    #[serde(rename = "use")]
    pub preferred: Option<String>,
    pub avoid: BTreeSet<String>,
}

pub type Policy = BTreeMap<String, RuntimePolicy>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fleet {
    pub at: i64,
    pub policy: Policy,
    /// Usage rows of other hosts' trusted fresh heartbeats on the approved board.
    pub rows: Vec<String>,
    /// Why this file holds no policy this round.
    pub error: Option<String>,
}

/// The policy of an issue's trusted, unedited comments, folded in comment-id order per runtime and id.
pub fn fold(comments: &[Comment]) -> Policy {
    let mut ordered: Vec<&Comment> = comments.iter().filter(|c| c.trusted && !c.edited).collect();
    ordered.sort_by_key(|c| c.id);
    let mut policy = Policy::new();
    for c in ordered {
        let Some(rest) = c.body.trim().strip_prefix("taskq: account ") else { continue };
        let [op, rt, id] = rest.split(' ').collect::<Vec<_>>()[..] else { continue };
        if !accounts::applies(rt) || !valid_id(id) { continue; }
        let p = policy.entry(rt.to_string()).or_default();
        match op {
            "use" => { p.preferred = Some(id.to_string()); p.avoid.remove(id); }
            "avoid" => { if p.preferred.as_deref() == Some(id) { p.preferred = None; } p.avoid.insert(id.to_string()); }
            "clear" => { if p.preferred.as_deref() == Some(id) { p.preferred = None; } p.avoid.remove(id); }
            _ => {}
        }
    }
    policy.retain(|_, p| p.preferred.is_some() || !p.avoid.is_empty());
    policy
}

fn refusal(role: Option<&std::ffi::OsStr>, attempt: Option<&std::ffi::OsStr>) -> Option<&'static str> {
    (role.is_some() || attempt.is_some()).then_some("accounts fleet is refused under TASKQ_ROLE: only the owner changes fleet policy")
}

/// Whether this process may write fleet policy; checked before the board opens.
pub fn fleet_allowed() -> Result<(), String> {
    match refusal(std::env::var_os("TASKQ_ROLE").as_deref(), std::env::var_os("TASKQ_ATTEMPT").as_deref()) {
        Some(reason) => Err(reason.into()),
        None => Ok(()),
    }
}

fn policy_issue(issues: &[Issue]) -> Option<&Issue> {
    issues.iter().filter(|i| i.open && i.trusted && i.labels.iter().any(|l| l == POLICY_LABEL)).min_by_key(|i| i.number)
}

/// `taskq accounts fleet`: one trusted comment on the board's one policy issue, created when absent.
pub fn fleet_write(board: &dyn Board, op: &str, rt: &str, id: &str) -> Result<String, String> {
    if !matches!(op, "use" | "avoid" | "clear") { return Err("usage: taskq accounts fleet use|avoid|clear <runtime> <id>".into()); }
    if !accounts::applies(rt) { return Err(format!("runtime {rt} has no accounts; use claude, codex or antigravity")); }
    if !valid_id(id) { return Err("id: 8 hex digits, as `taskq accounts` shows it".into()); }
    let labels = [POLICY_LABEL.to_string()];
    board.ensure_labels(&labels)?;
    let issues = board.issues(None)?;
    let issue = match policy_issue(&issues) {
        Some(i) => i.number,
        None => board.create(POLICY_TITLE, POLICY_BODY, &labels)?,
    };
    board.comment(issue, &format!("taskq: account {op} {rt} {id}"))?;
    Ok(format!("accounts fleet: {op} {rt} {id} on #{issue}; hosts whose connection approved this board apply it within {EVERY} s"))
}

/// What one service keeps between fleet refreshes.
#[derive(Default)]
pub struct FleetCache {
    next: i64,
    /// Issue number, its `updated`, the read time and the fold.
    read: Option<(u64, i64, i64, Policy)>,
}

impl FleetCache {
    /// Whether a refresh is due; a due check starts the next period, so another project's pass costs one compare.
    pub fn due(&mut self, now: i64) -> bool {
        if now < self.next { return false; }
        self.next = now + EVERY;
        true
    }

    fn policy(&mut self, board: &dyn Board, now: i64) -> Result<Policy, String> {
        let issues = board.issues(None)?;
        let Some(issue) = policy_issue(&issues) else { self.read = None; return Ok(Policy::new()) };
        if let Some((number, updated, read, policy)) = &self.read {
            if *number == issue.number && *updated == issue.updated && *read >= issue.updated + SETTLE { return Ok(policy.clone()); }
        }
        let policy = fold(&board.comments(issue.number)?);
        self.read = Some((issue.number, issue.updated, now, policy.clone()));
        Ok(policy)
    }

    /// The speaking project's refresh: this board's policy and its other hosts' rows, for one host's ranking.
    pub fn refresh(&mut self, board: &dyn Board, install: &Path, hosts: &[Issue], me: &str, now: i64) -> Result<(), String> {
        self.next = now + EVERY;
        let (policy, error) = match self.policy(board, now) {
            Ok(p) => (p, None),
            Err(_) => (Policy::new(), Some("policy issue unreadable".to_string())),
        };
        let rows = hosts.iter().filter(|h| h.trusted && now - h.updated <= TTL && crate::service::host_name(h).is_some_and(|n| n != me))
            .flat_map(|h| h.body.lines()).filter(|l| Row::parse(l).is_some()).map(str::to_string).collect();
        let fleet = Fleet { at: now, policy, rows, error };
        crate::platform::write_synced(&install.join(FLEET), &serde_json::to_string(&fleet).map_err(|e| e.to_string())?)
    }
}

// ---------- what ranking reads ----------

/// The inputs of account ranking on this host.
#[derive(Clone, Debug, Default)]
pub struct View {
    floor: Option<u8>,
    store: Store,
    observed: super::headroom::Observed,
    fleet: Option<Fleet>,
    fleet_error: Option<String>,
}

/// `account_floor` of host.json: absent is none; anything but an integer 0..100 is refused.
pub fn floor_of(v: &Value) -> Result<Option<u8>, String> {
    match v.get("account_floor") {
        None => Ok(None),
        Some(n) => n.as_u64().filter(|n| *n <= 100).map(|n| Some(n as u8)).ok_or_else(|| "host.json: account_floor must be an integer 0..100".to_string()),
    }
}

fn read_store(install: &Path) -> Store {
    crate::data::json(&install.join(FILE)).ok().flatten().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

impl View {
    /// Unreadable inputs read as unknown usage or local ranking: neither holds work.
    pub fn read(install: &Path) -> View {
        let floor = crate::data::json(&install.join("host.json")).ok().flatten().and_then(|v| floor_of(&v).ok().flatten());
        let (fleet, fleet_error) = match crate::data::json(&install.join(FLEET)) {
            Ok(None) => (None, None),
            Ok(Some(v)) => match serde_json::from_value(v) { Ok(f) => (Some(f), None), Err(_) => (None, Some("policy file unreadable".to_string())) },
            Err(_) => (None, Some("policy file unreadable".to_string())),
        };
        View { floor, store: read_store(install), observed: super::headroom::observed(install), fleet, fleet_error }
    }

    fn fleet(&self, now: i64) -> Option<&Fleet> { self.fleet.as_ref().filter(|f| now - f.at <= FLEET_TTL) }

    /// Why the host ranks locally, `None` when fleet policy applies.
    pub fn local_reason(&self, now: i64) -> Option<String> {
        if let Some(e) = &self.fleet_error { return Some(e.clone()); }
        let Some(f) = &self.fleet else { return Some("no connection".into()) };
        if now - f.at > FLEET_TTL { return Some("policy stale".into()); }
        f.error.clone()
    }

    fn policy(&self, rt: &str, now: i64) -> Option<&RuntimePolicy> {
        self.fleet(now).filter(|f| f.error.is_none())?.policy.get(rt)
    }

    pub fn id(&self, rt: &str, label: &str) -> Option<&str> { self.store.get(rt)?.get(label)?.id.as_deref() }

    pub fn avoided(&self, rt: &str, label: &str, now: i64) -> bool {
        self.id(rt, label).zip(self.policy(rt, now)).is_some_and(|(id, p)| p.avoid.contains(id))
    }

    pub fn preferred(&self, rt: &str, label: &str, now: i64) -> bool {
        self.id(rt, label).zip(self.policy(rt, now)).is_some_and(|(id, p)| p.preferred.as_deref() == Some(id))
    }

    /// The smallest window's fresh remaining percent: per window the freshest reading of this home and of other hosts'
    /// rows with the same `id`. `None` is unknown.
    pub fn remaining(&self, rt: &str, label: &str, now: i64) -> Option<u8> {
        if self.incomplete(rt, label, now) { return None; }
        let mut windows: BTreeMap<&str, (i64, u8)> = BTreeMap::new();
        let mut add = |name, at, left| { if windows.get(name).is_none_or(|(old, _)| *old < at) { windows.insert(name, (at, left)); } };
        for (name, (at, remaining, reset)) in self.observed.get(rt).and_then(|a| a.get(label)).into_iter().flatten() {
            if fresh(*at, *reset, now) { add(name.as_str(), *at, percent(remaining * 100.0)); }
        }
        let rows: Vec<Row> = match (self.id(rt, label), self.fleet(now)) {
            (Some(id), Some(f)) => f.rows.iter().filter_map(|l| Row::parse(l)).filter(|r| r.rt == rt && r.id.as_deref() == Some(id)).collect(),
            _ => vec![],
        };
        for r in &rows {
            if let Usage::Window { window, at } = &r.usage {
                if fresh(*at, window.reset, now) { add(window.name.as_str(), *at, 100 - window.used); }
            }
        }
        windows.values().map(|(_, left)| *left).min()
    }

    /// A partial poll cannot establish aggregate headroom, locally or on a host sharing this account.
    pub(crate) fn incomplete(&self, rt: &str, label: &str, now: i64) -> bool {
        if self.store.get(rt).and_then(|a| a.get(label)).is_some_and(|r| !r.unreadable.is_empty() && fresh(r.unreadable_at.unwrap_or(r.at), None, now)) { return true; }
        let Some(id) = self.id(rt, label) else { return false };
        self.fleet(now).is_some_and(|f| f.rows.iter().filter_map(|l| Row::parse(l)).any(|r|
            r.rt == rt && r.id.as_deref() == Some(id) && match r.usage {
                Usage::Partial { .. } => true,
                Usage::Unknown(reason) => reason.starts_with("quota unreadable "),
                Usage::Window { .. } => false,
            }))
    }

    /// The best of `candidates` (enabled, limit ended, not avoided) in list order: `use`, then fresh remaining at or
    /// above the floor, then unknown, then fresh remaining below the floor. The floor only orders; it holds nothing.
    pub fn rank(&self, rt: &str, candidates: &[String], now: i64) -> Option<String> {
        if let Some(l) = candidates.iter().find(|l| self.preferred(rt, l, now)) { return Some(l.clone()); }
        let floor = self.floor.unwrap_or(DEFAULT_FLOOR);
        let left: Vec<(&String, Option<u8>)> = candidates.iter().map(|l| (l, self.remaining(rt, l, now))).collect();
        let greatest = |above: bool| {
            let mut best: Option<(&String, u8)> = None;
            for (l, r) in &left {
                if let Some(r) = r.filter(|r| (*r >= floor) == above) {
                    if best.is_none_or(|(_, b)| r > b) { best = Some((l, r)); }
                }
            }
            best.map(|(l, _)| l.clone())
        };
        greatest(true).or_else(|| left.iter().find(|(_, r)| r.is_none()).map(|(l, _)| (*l).clone())).or_else(|| greatest(false))
    }

    /// The heartbeat rows of one listed account; `None` of its windows is fresh when its last poll is old.
    pub fn lines(&self, rt: &str, label: &str, enabled: bool, now: i64) -> Vec<String> {
        let record = self.store.get(rt).and_then(|a| a.get(label));
        let id = record.and_then(|r| r.id.clone());
        let row = |usage| Row { rt: rt.into(), label: label.into(), id: id.clone(), usage }.line();
        if !enabled { return vec![row(Usage::Unknown("disabled".into()))]; }
        let Some(r) = record else { return vec![row(Usage::Unknown("not polled".into()))] };
        if !fresh(r.at, None, now) { return vec![row(Usage::Unknown("poll stale".into()))]; }
        let mut lines: Vec<_> = match &r.unknown {
            Some(reason) => vec![row(Usage::Unknown(reason.clone()))],
            None => r.windows.iter().map(|w| row(Usage::Window { window: w.clone(), at: r.at }))
                .chain(r.partial.iter().map(|w| row(Usage::Partial { window: w.clone(), at: r.at }))).collect(),
        };
        if fresh(r.unreadable_at.unwrap_or(r.at), None, now) {
            lines.extend(r.unreadable.iter().map(|reason| row(Usage::Unknown(format!("quota unreadable {reason}")))));
        }
        lines
    }

    /// The `account-policy` heartbeat lines: `ids` lists the fingerprints of a runtime's listed homes.
    pub fn policy_lines(&self, now: i64, ids: impl Fn(&str) -> Vec<String>) -> Vec<String> {
        if let Some(reason) = self.local_reason(now) { return vec![format!("account-policy local {reason}")]; }
        let mut lines = vec![];
        for (rt, p) in self.fleet(now).map(|f| &f.policy).into_iter().flatten() {
            let homes = ids(rt);
            if let Some(id) = &p.preferred {
                lines.push(format!("account-policy {} {rt} {id}", if homes.contains(id) { "use" } else { "unmet" }));
            }
            lines.extend(p.avoid.iter().filter(|id| homes.contains(*id)).map(|id| format!("account-policy avoid {rt} {id}")));
        }
        lines
    }
}

// ---------- the poll ----------

/// One poll of this host's enabled accounts. `fetch` runs outside the account lock: a slow route never blocks a start.
pub fn poll_with(install: &Path, now: i64, mut fetch: impl FnMut(&str, &str) -> Poll) -> Result<(), String> {
    let policy = accounts::read_policy(install)?;
    let list = |rt: &str| policy.get(rt).cloned().unwrap_or_else(|| vec![accounts::Entry { label: accounts::DEFAULT.into(), enabled: true }]);
    let mut results = vec![];
    for rt in super::CLI_RUNTIMES.iter().filter(|rt| accounts::applies(rt)) {
        for e in list(rt).into_iter().filter(|e| e.enabled) {
            let poll = fetch(rt, &e.label);
            results.push((rt.to_string(), e.label, poll));
        }
    }
    let _lock = lock(install)?;
    let mut store = read_store(install);
    for (rt, label, poll) in &results {
        let old = store.get(rt).and_then(|a| a.get(label));
        if let Some(reason) = poll.unknown.as_ref().filter(|r| old.is_none_or(|o| o.unknown.as_ref() != Some(r))) {
            crate::platform::log(&format!("account usage {rt} {label}: unknown {reason}"));
        }
        let id = poll.id.clone().or_else(|| old.and_then(|o| o.id.clone()));
        // Only a complete poll reaches `windows`: older binaries read that field and ignore every `unreadable` diagnostic.
        let (windows, partial) = if poll.unreadable.is_empty() { (poll.windows.clone(), vec![]) } else { (vec![], poll.windows.clone()) };
        let mut record = Record { id, at: now, windows, partial, unreadable: poll.unreadable.clone(),
            unreadable_at: (!poll.unreadable.is_empty()).then_some(now), unknown: poll.unknown.clone() };
        if poll.unknown.is_some() && poll.unreadable.is_empty() {
            if let Some(old) = old.filter(|o| o.id == record.id && !o.unreadable.is_empty()) {
                record.unreadable = old.unreadable.clone();
                record.unreadable_at = Some(old.unreadable_at.unwrap_or(old.at));
            }
        }
        store.entry(rt.clone()).or_default().insert(label.clone(), record);
    }
    // an account dropped from the list leaves no row; a disabled one keeps its id
    store.retain(|rt, a| { a.retain(|label, _| list(rt).iter().any(|e| e.label == *label)); !a.is_empty() });
    crate::platform::write_synced(&install.join(FILE), &serde_json::to_string(&store).map_err(|e| e.to_string())?)?;
    let observations: Vec<_> = results.iter().map(|(rt, label, poll)| {
        let complete: &[Window] = if poll.unreadable.is_empty() { &poll.windows } else { &[] };
        (rt.as_str(), label.as_str(), complete)
    }).collect();
    super::headroom::record_polled(install, now, &observations)
}

fn lock(install: &Path) -> Result<coreq::file_lock::Lock, String> {
    std::fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let path = install.join("accounts-state.lock");
    let mut last = String::new();
    for _ in 0..20 {
        match coreq::file_lock::try_lock(&path) {
            Ok(l) => return Ok(l),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("account usage save busy: {last}"))
}

/// Starts the host's one poll thread; a failed round logs and the next one retries.
pub fn spawn(install: PathBuf) {
    let thread = std::thread::Builder::new().name("account-usage".into()).spawn(move || {
        let mut ids = Ids::default();
        loop {
            if let Err(e) = poll_with(&install, accounts::now(), |rt, label| fetch(rt, label, &mut ids)) {
                crate::platform::log(&format!("account usage: {e}"));
            }
            std::thread::sleep(Duration::from_secs(EVERY as u64));
        }
    });
    if let Err(e) = thread { crate::platform::log(&format!("account usage: not started: {e}")); }
}

/// Native-probe fingerprints per runtime and label.
#[derive(Default)]
pub struct Ids(BTreeMap<(String, String), (i64, Option<String>)>);

impl Ids {
    fn get(&mut self, rt: &str, label: &str) -> Option<String> {
        let now = accounts::now();
        let key = (rt.to_string(), label.to_string());
        if let Some((_, id)) = self.0.get(&key).filter(|(at, id)| id.is_some() && now - at < ID_REFRESH) { return id.clone(); }
        let id = super::account_email(rt, label).map(|email| fingerprint(&email));
        self.0.insert(key, (now, id.clone()));
        id
    }
}

/// The native read-only route of one account, chosen by adapter capability.
pub fn fetch(rt: &str, label: &str, ids: &mut Ids) -> Poll {
    let a = super::of(rt);
    // The `agy` login has no account home: one default account per host, read from the user's home.
    if a.has(Cap::UsageCodeAssist) {
        return if super::which(a.bin).is_absolute() { code_assist(rt) } else { Poll::failed("not installed") };
    }
    let Some(home) = accounts::home(rt, label).or_else(|| accounts::base(rt)) else { return Poll::failed("no login") };
    if !home.is_dir() { return Poll::failed("no login"); }
    if a.has(Cap::UsageRpc) {
        let bin = super::which(a.bin);
        if !bin.is_absolute() { return Poll::failed("not installed"); }
        let mut c = crate::platform::command(&bin);
        c.arg("app-server");
        if let Some((var, h)) = accounts::environment(rt, &home) { c.env(var, h); }
        let run = || rpc(c, RPC_DEADLINE);
        return if a.has(Cap::AuthFile) { guarded(&home.join("auth.json"), run) } else { run() };
    }
    if a.has(Cap::UsageOauth) {
        let mut poll = oauth(rt, &home);
        poll.id = ids.get(rt, label);
        return poll;
    }
    Poll::failed("no usage route")
}

/// A login file changed during the poll: the route refreshed it, so the result says so and the owner sees it.
fn guarded(file: &Path, run: impl FnOnce() -> Poll) -> Poll {
    let sign = || std::fs::read(file).ok().map(|b| sha256(&b));
    let before = sign();
    let poll = run();
    if sign() == before { return poll; }
    crate::platform::log("account usage: a poll changed a login file");
    Poll { id: poll.id, ..Poll::failed("login file changed") }
}

/// `codex app-server`: stdin must stay open until the replies arrive, or the server exits without answering.
fn rpc(mut c: std::process::Command, deadline: Duration) -> Poll {
    use std::process::Stdio;
    let Ok(mut child) = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return Poll::failed("app-server failed") };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { let _ = child.kill(); let _ = child.wait(); return Poll::failed("app-server failed") };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() { break; }
        }
    });
    let sent = [
        r#"{"id":1,"method":"initialize","params":{"clientInfo":{"name":"taskq","title":"TaskQ","version":"0"}}}"#,
        r#"{"method":"initialized"}"#,
        r#"{"id":2,"method":"account/read","params":{"refreshToken":false}}"#,
        r#"{"id":3,"method":"account/rateLimits/read"}"#,
    ].iter().all(|m| writeln!(stdin, "{m}").and_then(|_| stdin.flush()).is_ok());
    let end = Instant::now() + deadline;
    let (mut account, mut limits) = (None, None);
    let mut failed = (!sent).then_some("app-server failed");
    while failed.is_none() && (account.is_none() || limits.is_none()) {
        match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(line) => {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                match v["id"].as_u64() { Some(2) => account = Some(v), Some(3) => limits = Some(v), _ => {} }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => failed = Some("timeout"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => failed = Some("app-server failed"),
        }
    }
    drop(stdin);
    let exit = Instant::now() + Duration::from_secs(3);
    while Instant::now() < exit && matches!(child.try_wait(), Ok(None)) { std::thread::sleep(Duration::from_millis(50)); }
    if matches!(child.try_wait(), Ok(None)) { let _ = child.kill(); }
    let code = child.wait().ok().and_then(|status| status.code());
    if let Some(reason) = failed { return Poll::failed(&rpc_failure(reason, code)); }
    rpc_poll(&account.unwrap_or_default(), &limits.unwrap_or_default())
}

/// A server that ended before it answered names its exit code, so a host's row shows more than that it failed.
fn rpc_failure(reason: &str, code: Option<i32>) -> String {
    match code {
        Some(code) if reason == "app-server failed" => format!("app-server exit {code}"),
        _ => reason.to_string(),
    }
}

fn rpc_poll(account: &Value, limits: &Value) -> Poll {
    if account["error"].is_null() && account["result"]["account"].is_null() { return Poll::failed("no login"); }
    let a = &account["result"]["account"];
    let identity = a["email"].as_str().or_else(|| account["result"]["workspaceRouting"]["chatgptAccountId"].as_str()).filter(|s| !s.trim().is_empty());
    let id = identity.map(fingerprint);
    match rate_windows(limits) {
        Ok(windows) => Poll { id, windows, ..Default::default() },
        Err(reason) => Poll { id, ..Poll::failed(reason) },
    }
}

fn rate_windows(reply: &Value) -> Result<Vec<Window>, &'static str> {
    if !reply["error"].is_null() { return Err("call refused"); }
    let limits = &reply["result"]["rateLimits"];
    let mut out = vec![];
    for key in ["primary", "secondary"] {
        let w = &limits[key];
        if w.is_null() { continue; }
        let used = w["usedPercent"].as_f64().filter(|n| n.is_finite()).ok_or("usage shape changed")?;
        let name = w["windowDurationMins"].as_u64().map_or_else(|| key.to_string(), |mins| format!("{mins}m"));
        out.push(Window { name, used: percent(used), reset: w["resetsAt"].as_i64().filter(|r| *r >= 0) });
    }
    if out.is_empty() { Err("usage shape changed") } else { Ok(out) }
}

/// The OAuth route: read the access token, never refresh or store it; it travels to the provider on curl's stdin only.
fn oauth(rt: &str, home: &Path) -> Poll {
    let token = match credentials(rt, home).and_then(|text| access_token(&text, accounts::now()).map_err(String::from)) {
        Ok(token) => token,
        Err(reason) => return Poll::failed(&reason),
    };
    let config = format!("url = \"https://api.anthropic.com/api/oauth/usage\"\nheader = \"Authorization: Bearer {token}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"\nmax-time = {HTTP_DEADLINE}\n");
    match https_json(&config).and_then(|v| oauth_windows(&v).map_err(String::from)) {
        Ok(poll) => poll,
        Err(reason) => Poll::failed(&reason),
    }
}

/// One curl call whose config, with the token, goes through stdin and never an argument; the reply or a fixed reason.
fn https_json(config: &str) -> Result<Value, String> {
    let out = crate::http::with_config("account.usage", &["-q", "-sS", "--proto", "=https", "-w", "\n%{http_code}", "--config", "-"], config, Duration::from_secs(HTTP_DEADLINE + 5))
        .map_err(|_| "network failed")?;
    match crate::http::status_tail(&out) {
        (body, Some(200)) => serde_json::from_str(&body).map_err(|_| "usage shape changed".into()),
        (_, None) => Err("network failed".into()),
        (_, Some(code)) => Err(format!("call refused {code}")),
    }
}

/// The two Code Assist calls with the `agy` login. Both endpoints are undocumented, so any other reply fails closed.
fn code_assist(rt: &str) -> Poll {
    let token = match agy_credentials(rt).and_then(|text| agy_token(&text, accounts::now()).map_err(String::from)) {
        Ok(token) => token,
        Err(reason) => return Poll::failed(&reason),
    };
    code_assist_with(&token, https_json)
}

fn code_assist_with(token: &str, mut read: impl FnMut(&str) -> Result<Value, String>) -> Poll {
    let load = match read(&code_assist_call("cloudcode-pa.googleapis.com", "loadCodeAssist", token, r#"{\"metadata\":{\"ideType\":\"ANTIGRAVITY\"}}"#)) {
        Ok(v) => v,
        Err(reason) => return Poll::failed(&reason),
    };
    let project = code_assist_project(&load);
    let body = project.map_or(r#"{}"#.to_string(), |p| format!(r#"{{\"project\":\"{p}\"}}"#));
    let id = project.map(fingerprint);
    let host = match load.pointer("/currentTier/usesGcpTos") {
        Some(Value::Bool(true)) => "cloudcode-pa.googleapis.com",
        None | Some(Value::Bool(false)) => "daily-cloudcode-pa.googleapis.com",
        _ => return Poll { id, ..Poll::failed("usage shape changed") },
    };
    match read(&code_assist_call(host, "retrieveUserQuotaSummary", token, &body)).and_then(|v| quota_windows(&v).map_err(String::from)) {
        Ok(windows) => Poll { id, windows, ..Default::default() },
        Err(reason) => Poll { id, ..Poll::failed(&reason) },
    }
}

fn code_assist_call(host: &str, method: &str, token: &str, body: &str) -> String {
    format!("url = \"https://{host}/v1internal:{method}\"\nheader = \"Authorization: Bearer {token}\"\nheader = \"Content-Type: application/json\"\nheader = \"User-Agent: antigravity/hub/2.28.2\"\ndata = \"{body}\"\nmax-time = {HTTP_DEADLINE}\n")
}

/// The account's Code Assist project; only characters safe inside a curl config line pass. The reply names no email,
/// so its fingerprint is the account id.
fn code_assist_project(load: &Value) -> Option<&str> {
    load["cloudaicompanionProject"].as_str().filter(|p| !p.is_empty() && p.len() <= 128 && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':')))
}

/// Every bucket of every model family, as `gemini-5h` or `3p-weekly`. A bucket with no reading fails the whole poll:
/// a dropped bucket could be the exhausted one, and unknown is never healthy.
fn quota_windows(v: &Value) -> Result<Vec<Window>, &'static str> {
    let shape = "usage shape changed";
    let mut out = vec![];
    for group in v["groups"].as_array().ok_or(shape)? {
        let buckets = group["buckets"].as_array().filter(|b| !b.is_empty()).ok_or(shape)?;
        for bucket in buckets {
            let name = bucket["bucketId"].as_str().filter(|n| valid_window(n)).ok_or(shape)?;
            let left = bucket["remainingFraction"].as_f64().filter(|f| (0.0..=1.0).contains(f)).ok_or(shape)?;
            out.push(Window { name: name.to_string(), used: percent((1.0 - left) * 100.0), reset: bucket["resetTime"].as_str().and_then(utc) });
        }
    }
    if out.is_empty() { Err(shape) } else { Ok(out) }
}

/// The `agy` login's access token: a token file, or the same text in the OS keyring, optionally base64 with go-keyring's
/// prefix. Only characters safe inside a curl config line pass.
fn agy_token(text: &str, now: i64) -> Result<String, &'static str> {
    use base64::Engine;
    let decoded = match text.trim().strip_prefix("go-keyring-base64:") {
        Some(b) => base64::engine::general_purpose::STANDARD.decode(b.trim()).ok().and_then(|b| String::from_utf8(b).ok()).ok_or("usage shape changed")?,
        None => text.to_string(),
    };
    let v: Value = serde_json::from_str(&decoded).map_err(|_| "no login")?;
    let t = if v["token"].is_object() { &v["token"] } else { &v };
    let token = t["access_token"].as_str().filter(|t| !t.is_empty()).ok_or("no login")?;
    if !token.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) { return Err("usage shape changed"); }
    let expiry = t["expiry"].as_str().and_then(utc).or_else(|| t["expiry_date"].as_i64().map(|ms| ms / 1000));
    if expiry.is_some_and(|at| at <= now) { return Err("token expired"); }
    Ok(token.to_string())
}

/// The login files of the `agy` CLI under the user's home, read only. A login held only in the Windows credential store
/// reads as no login.
fn agy_login_files() -> Vec<PathBuf> {
    crate::platform::user_home().map_or(vec![], |h| [".gemini/antigravity-cli/antigravity-oauth-token", ".gemini/oauth_creds.json"].iter().map(|f| h.join(f)).collect())
}

fn agy_credentials(rt: &str) -> Result<String, String> {
    if cfg!(target_os = "macos") {
        if let Some(text) = keychain_read("gemini", rt)? { return Ok(text); }
    }
    agy_login_files().iter().find_map(|f| std::fs::read_to_string(f).ok()).ok_or_else(|| "no login".to_string())
}

/// Plan and model windows, including scoped `limits[]`; credits are no window.
fn oauth_windows(v: &Value) -> Result<Poll, &'static str> {
    let shape = "usage shape changed";
    let mut poll = Poll::default();
    for (index, (name, w)) in v.as_object().ok_or(shape)?.iter().enumerate() {
        if !(name.starts_with("five_hour") || name.starts_with("seven_day")) || name == "seven_day_breakdown" || w.is_null() { continue; }
        if !valid_window(name) {
            poll.unreadable.push(format!("legacy[{index}]: unsafe window name"));
            continue;
        }
        match oauth_window(name.clone(), &w["utilization"], &w["resets_at"]) {
            Ok(w) => poll.windows.push(w),
            Err(reason) => poll.unreadable.push(format!("{name}: {reason}")),
        }
    }
    if let Some(limits) = v.get("limits").filter(|l| !l.is_null()) {
        if let Some(limits) = limits.as_array() {
            for (index, l) in limits.iter().enumerate() {
                let mut source = format!("limits[{index}]");
                let parsed = (|| {
                    let mut name = match l["group"].as_str() {
                        Some("session") => "five_hour".to_string(),
                        Some("weekly") => "seven_day".to_string(),
                        _ => return Err("unsupported group"),
                    };
                    let scope = l.get("scope").filter(|s| !s.is_null()).map(|s| s.as_object().ok_or("unreadable scope")).transpose()?;
                    if let Some(model) = scope.and_then(|s| s.get("model")).filter(|m| !m.is_null()) {
                        let model = model["display_name"].as_str().or_else(|| model["id"].as_str()).filter(|m| !m.is_empty()).ok_or("unreadable model")?;
                        name.push('_');
                        name.push_str(&model.to_ascii_lowercase().replace(' ', "_"));
                    }
                    if !valid_window(&name) { return Err("unsafe window name"); }
                    source.push(' ');
                    source.push_str(&name);
                    if scope.is_some_and(|s| s.iter().any(|(key, value)| key != "model" && !value.is_null())) { return Err("unsupported scope"); }
                    // An authoritative structured reading must not leave a healthier legacy value behind.
                    poll.windows.retain(|w| w.name != name);
                    poll.unreadable.retain(|reason| !reason.starts_with(&format!("{name}: ")));
                    oauth_window(name, &l["percent"], &l["resets_at"])
                })();
                match parsed {
                    Ok(w) => poll.windows.push(w),
                    Err(reason) => poll.unreadable.push(format!("{source}: {reason}")),
                }
            }
        } else {
            poll.unreadable.push("limits: unreadable list".into());
        }
    }
    if poll.windows.is_empty() && poll.unreadable.is_empty() { Err(shape) } else { Ok(poll) }
}

fn oauth_window(name: String, used: &Value, reset: &Value) -> Result<Window, &'static str> {
    let used = used.as_f64().filter(|n| n.is_finite() && *n >= 0.0).ok_or("unreadable utilization")?;
    Ok(Window { name, used: percent(used), reset: reset.as_str().and_then(utc) })
}

/// `2026-11-04T10:00:00.123+00:00` or `...Z` as board seconds; `None` for anything else.
fn utc(s: &str) -> Option<i64> {
    let base = s.get(..19).filter(|b| b.as_bytes()[10] == b'T')?;
    let at = crate::board::iso(base)?;
    let rest = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if rest.is_empty() || rest == "Z" { return Some(at); }
    let (sign, rest) = match rest.as_bytes()[0] { b'+' => (1, &rest[1..]), b'-' => (-1, &rest[1..]), _ => return None };
    let (h, m) = rest.split_once(':')?;
    Some(at - sign * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60))
}

/// The access token of a credentials document; only characters safe inside a curl config line pass.
fn access_token(text: &str, now: i64) -> Result<String, &'static str> {
    let v: Value = serde_json::from_str(text).map_err(|_| "no login")?;
    let oauth = &v["claudeAiOauth"];
    let token = oauth["accessToken"].as_str().filter(|t| !t.is_empty()).ok_or("no login")?;
    if !token.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) { return Err("usage shape changed"); }
    if oauth["expiresAt"].as_i64().is_some_and(|ms| ms <= now.saturating_mul(1000)) { return Err("token expired"); }
    Ok(token.to_string())
}

fn credentials(rt: &str, home: &Path) -> Result<String, String> {
    if cfg!(target_os = "macos") {
        // A home named by the home variable has its own keychain service, as the native login stores it.
        let suffix = accounts::environment(rt, home).map_or(String::new(), |(_, h)| format!("-{}", &sha256(h.to_string_lossy().as_bytes())[..8]));
        if let Some(text) = keychain_read(&format!("Claude Code-credentials{suffix}"), "")? { return Ok(text); }
    }
    std::fs::read_to_string(home.join(".credentials.json")).map_err(|_| "no login".to_string())
}

const KEYCHAIN: &str = "/usr/bin/security";
/// A silent read takes milliseconds, so a read this long waits on a dialog.
const KEYCHAIN_DEADLINE: Duration = Duration::from_secs(5);
/// ponytail: one latch per service process; per-item latches if a host ever mixes trusted and untrusted items.
static KEYCHAIN_PROMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A macOS keychain item, read through the tool both native logins store it with, the one the item's access list trusts.
/// `None` is no item. A read that outlasts its deadline would prompt: it stops, and this process reads no item again.
fn keychain_read(service: &str, account: &str) -> Result<Option<String>, String> {
    use std::sync::atomic::Ordering;
    if KEYCHAIN_PROMPTED.load(Ordering::Relaxed) { return Err("keychain would prompt".into()); }
    match coreq::process::output("account.usage", crate::platform::command(KEYCHAIN).args(keychain_args(service, account)), KEYCHAIN_DEADLINE) {
        Ok(o) | Err(coreq::process::Failure::Exit(o)) => keychain_reply(o.status.code(), &o.stdout),
        Err(coreq::process::Failure::Timeout) => {
            KEYCHAIN_PROMPTED.store(true, Ordering::Relaxed);
            Err("keychain would prompt".into())
        }
        Err(_) => Err("keychain read failed".into()),
    }
}

fn keychain_args(service: &str, account: &str) -> Vec<String> {
    let mut args = vec!["find-generic-password".to_string(), "-s".to_string(), service.to_string()];
    if !account.is_empty() { args.extend(["-a".to_string(), account.to_string()]); }
    args.push("-w".to_string());
    args
}

/// The tool exits with the low byte of the keychain status: 44 no item, 36 interaction not allowed.
fn keychain_reply(code: Option<i32>, stdout: &[u8]) -> Result<Option<String>, String> {
    match code {
        Some(0) => Ok(Some(String::from_utf8_lossy(stdout).trim().to_string()).filter(|t| !t.is_empty())),
        Some(44) => Ok(None),
        Some(36) => Err("keychain would prompt".into()),
        Some(code) => Err(format!("keychain read failed {code}")),
        None => Err("keychain read failed".into()),
    }
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
