//! Polled account usage and fleet account policy ([capacity.md](../docs/design/capacity.md#account-usage-and-fleet-policy)).
//! One host thread polls each enabled account; the rows it publishes carry a fingerprint, never an email or a token.
//! A poll only observes: it writes no login file and clears no quota record or hold.
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

/// One account's poll: its identity fingerprint, windows, or the fixed reason it has none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poll {
    pub id: Option<String>,
    pub windows: Vec<Window>,
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
    Unknown(String),
}

/// One heartbeat `limit <rt> <label> id <id|-> ...` row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rt: String,
    pub label: String,
    pub id: Option<String>,
    pub usage: Usage,
}

impl Row {
    pub fn line(&self) -> String {
        let head = format!("limit {} {} id {}", self.rt, self.label, self.id.as_deref().unwrap_or("-"));
        match &self.usage {
            Usage::Window { window, at } => format!("{head} window {} used {} reset {} at {}", window.name, window.used,
                window.reset.map_or("-".to_string(), crate::board::stamp), crate::board::stamp(*at)),
            Usage::Unknown(reason) => format!("{head} unknown {reason}"),
        }
    }

    /// A usage row; `None` for anything else, the `limit <rt> until|probe <time>` hold line included.
    pub fn parse(line: &str) -> Option<Row> {
        let w: Vec<&str> = line.strip_prefix("limit ")?.split(' ').collect();
        let [rt, label, "id", id, rest @ ..] = w.as_slice() else { return None };
        if !accounts::applies(rt) || !accounts::valid_label(label) || !(*id == "-" || valid_id(id)) { return None; }
        let time = |t: &str| crate::board::iso(t).filter(|at| crate::board::stamp(*at) == t);
        let usage = match rest {
            ["window", name, "used", used, "reset", reset, "at", at] if valid_window(name) => Usage::Window {
                window: Window { name: name.to_string(), used: used.parse().ok().filter(|u| *u <= 100)?,
                    reset: if *reset == "-" { None } else { Some(time(reset)?) } },
                at: time(at)?,
            },
            ["unknown", reason @ ..] if !reason.is_empty() => Usage::Unknown(reason.join(" ")),
            _ => return None,
        };
        Some(Row { rt: rt.to_string(), label: label.to_string(), id: (*id != "-").then(|| id.to_string()), usage })
    }

    /// The usage cell of `taskq accounts`.
    pub fn text(&self) -> String {
        match &self.usage {
            Usage::Window { window, .. } => format!("{} used {}% reset {}", window.name, window.used, window.reset.map_or("-".to_string(), crate::board::stamp)),
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
        if let Some(reason) = &r.unknown { return vec![row(Usage::Unknown(reason.clone()))]; }
        r.windows.iter().map(|w| row(Usage::Window { window: w.clone(), at: r.at })).collect()
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
        store.entry(rt.clone()).or_default().insert(label.clone(), Record { id, at: now, windows: poll.windows.clone(), unknown: poll.unknown.clone() });
    }
    // an account dropped from the list leaves no row; a disabled one keeps its id
    store.retain(|rt, a| { a.retain(|label, _| list(rt).iter().any(|e| e.label == *label)); !a.is_empty() });
    crate::platform::write_synced(&install.join(FILE), &serde_json::to_string(&store).map_err(|e| e.to_string())?)?;
    let observations: Vec<_> = results.iter().map(|(rt, label, poll)| (rt.as_str(), label.as_str(), poll.windows.as_slice())).collect();
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
    if a.has("usage-code-assist") {
        return if super::which(a.bin).is_absolute() { code_assist(rt) } else { Poll::failed("not installed") };
    }
    let Some(home) = accounts::home(rt, label).or_else(|| accounts::base(rt)) else { return Poll::failed("no login") };
    if !home.is_dir() { return Poll::failed("no login"); }
    if a.has("usage-rpc") {
        let bin = super::which(a.bin);
        if !bin.is_absolute() { return Poll::failed("not installed"); }
        let mut c = crate::platform::command(&bin);
        c.arg("app-server");
        if let Some((var, h)) = accounts::environment(rt, &home) { c.env(var, h); }
        let run = || rpc(c, RPC_DEADLINE);
        return if a.has("auth-file") { guarded(&home.join("auth.json"), run) } else { run() };
    }
    if a.has("usage-oauth") {
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
    Poll { id: poll.id, windows: vec![], unknown: Some("login file changed".into()) }
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
        Ok(windows) => Poll { id, windows, unknown: None },
        Err(reason) => Poll { id, windows: vec![], unknown: Some(reason.into()) },
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
        Ok(windows) => Poll { windows, ..Default::default() },
        Err(reason) => Poll::failed(&reason),
    }
}

/// One curl call whose config, with the token, goes through stdin and never an argument; the reply or a fixed reason.
fn https_json(config: &str) -> Result<Value, String> {
    let out = match coreq::process::exchange("account.usage", crate::platform::command("curl").args(["-q", "-sS", "--proto", "=https", "-w", "\n%{http_code}", "--config", "-"]),
        config.as_bytes(), Duration::from_secs(HTTP_DEADLINE + 5)) {
        Ok(o) | Err(coreq::process::Failure::Exit(o)) => o,
        Err(_) => return Err("network failed".into()),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    match code.trim().parse::<u16>() {
        Ok(200) => serde_json::from_str(body).map_err(|_| "usage shape changed".into()),
        Ok(0) | Err(_) => Err("network failed".into()),
        Ok(code) => Err(format!("call refused {code}")),
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
        Ok(windows) => Poll { id, windows, unknown: None },
        Err(reason) => Poll { id, ..Poll::failed(&reason) },
    }
}

fn code_assist_call(host: &str, method: &str, token: &str, body: &str) -> String {
    format!("url = \"https://{host}/v1internal:{method}\"\nheader = \"Authorization: Bearer {token}\"\nheader = \"Content-Type: application/json\"\nheader = \"User-Agent: antigravity\"\ndata = \"{body}\"\nmax-time = {HTTP_DEADLINE}\n")
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
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty());
    home.map_or(vec![], |h| [".gemini/antigravity-cli/antigravity-oauth-token", ".gemini/oauth_creds.json"].iter().map(|f| Path::new(&h).join(f)).collect())
}

fn agy_credentials(rt: &str) -> Result<String, String> {
    if cfg!(target_os = "macos") {
        if let Some(text) = keychain_read("gemini", rt)? { return Ok(text); }
    }
    agy_login_files().iter().find_map(|f| std::fs::read_to_string(f).ok()).ok_or_else(|| "no login".to_string())
}

/// Plan and model windows, including scoped `limits[]`; credits are no window.
fn oauth_windows(v: &Value) -> Result<Vec<Window>, &'static str> {
    let shape = "usage shape changed";
    let mut out = vec![];
    for (name, w) in v.as_object().ok_or(shape)? {
        if !(name.starts_with("five_hour") || name.starts_with("seven_day")) || w.is_null() { continue; }
        if !valid_window(name) { return Err(shape); }
        let used = w["utilization"].as_f64().filter(|n| n.is_finite()).ok_or(shape)?;
        out.push(Window { name: name.clone(), used: percent(used), reset: w["resets_at"].as_str().and_then(utc) });
    }
    if let Some(limits) = v.get("limits").filter(|l| !l.is_null()) {
        for l in limits.as_array().ok_or(shape)? {
            let mut name = match l["group"].as_str() {
                Some("session") => "five_hour".to_string(),
                Some("weekly") => "seven_day".to_string(),
                _ => return Err(shape),
            };
            let scope = l.get("scope").filter(|s| !s.is_null()).map(|s| s.as_object().ok_or(shape)).transpose()?;
            if scope.is_some_and(|s| s.keys().any(|k| k != "model")) { return Err(shape); }
            if let Some(model) = scope.and_then(|s| s.get("model")).filter(|m| !m.is_null()) {
                let model = model["display_name"].as_str().or_else(|| model["id"].as_str()).filter(|m| !m.is_empty()).ok_or(shape)?;
                name.push('_');
                name.push_str(&model.to_ascii_lowercase().replace(' ', "_"));
            }
            if !valid_window(&name) { return Err(shape); }
            let used = l["percent"].as_f64().filter(|n| n.is_finite()).ok_or(shape)?;
            let w = Window { name, used: percent(used), reset: l["resets_at"].as_str().and_then(utc) };
            if let Some(old) = out.iter_mut().find(|old| old.name == w.name) { *old = w; } else { out.push(w); }
        }
    }
    if out.is_empty() { Err(shape) } else { Ok(out) }
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
mod tests {
    use super::*;
    use crate::runtime::accounts::{Host, claim, of_log, mark, record_limit};

    const NOW: i64 = 1_800_000_000;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("taskq-usage-{name}-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn list(install: &Path, labels: &[(&str, bool)]) {
        let rows: Vec<Value> = labels.iter().map(|(l, e)| serde_json::json!({"label": l, "enabled": e})).collect();
        std::fs::write(install.join(accounts::FILE), serde_json::json!({"codex": rows}).to_string()).unwrap();
    }

    /// `used` per label of the codex list; a label left out fails its poll.
    fn poll(install: &Path, now: i64, used: &[(&str, u8)]) {
        poll_with(install, now, |rt, label| {
            if rt != "codex" { return Poll::failed("no login"); }
            match used.iter().find(|(l, _)| *l == label) {
                Some((l, u)) => Poll { id: Some(fingerprint(&format!("{l}@example.com"))), windows: vec![Window { name: "10080m".into(), used: *u, reset: Some(now + 3600) }], unknown: None },
                None => Poll { id: Some(fingerprint(&format!("{label}@example.com"))), ..Poll::failed("token expired") },
            }
        }).unwrap();
    }

    fn id(label: &str) -> String { fingerprint(&format!("{label}@example.com")) }

    // Chain: capacity.md § Account usage and fleet policy, owner note on #2341: the `agy` login polled read-only. Risk: an
    // undocumented reply reads as a healthy account, or a token reaches a row. Protection: any other shape is `unknown`
    // with a fixed reason, a missing reading fails the whole poll, and the row holds only bucket, percent, reset and id.
    #[test]
    fn code_assist_reads_buckets_or_a_fixed_reason() {
        let summary = serde_json::json!({"groups": [
            {"displayName": "Gemini Models", "description": "provider text", "buckets": [
                {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.8, "resetTime": "2026-11-04T10:00:00.123456Z", "description": "provider text"},
                {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.0, "resetTime": "2026-11-09T00:00:00+01:00"}]},
            {"displayName": "Claude and GPT models", "buckets": [{"bucketId": "3p-5h", "remainingFraction": 1}]}]});
        let w = quota_windows(&summary).unwrap();
        assert_eq!(w.iter().map(|w| (w.name.as_str(), w.used)).collect::<Vec<_>>(), [("gemini-5h", 20), ("gemini-weekly", 100), ("3p-5h", 0)]);
        assert_eq!(w[0].reset, utc("2026-11-04T10:00:00Z"));
        assert_eq!(w[1].reset, utc("2026-11-08T23:00:00Z"), "an offset reset reads as UTC");
        assert_eq!(w[2].reset, None);
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"groups": []}),
            serde_json::json!({"groups": [{"buckets": []}]}),
            serde_json::json!({"groups": [{"buckets": [{"bucketId": "gemini-5h"}]}]}),
            serde_json::json!({"groups": [{"buckets": [{"bucketId": "gemini-5h", "remainingFraction": 1.5}]}]}),
            serde_json::json!({"groups": [{"buckets": [{"bucketId": "has space", "remainingFraction": 0.5}]}]}),
            serde_json::json!({"groups": [{"buckets": [{"bucketId": "gemini-5h", "remainingFraction": 0.5}, {"remainingFraction": 0.5}]}]}),
        ] { assert_eq!(quota_windows(&bad), Err("usage shape changed"), "{bad}"); }

        assert_eq!(code_assist_project(&serde_json::json!({"cloudaicompanionProject": "fine-project-1"})), Some("fine-project-1"));
        for bad in [serde_json::json!({}), serde_json::json!({"cloudaicompanionProject": ""}), serde_json::json!({"cloudaicompanionProject": "a\"\nurl = \"x"})] {
            assert_eq!(code_assist_project(&bad), None, "{bad}");
        }
        let call = code_assist_call("cloudcode-pa.googleapis.com", "loadCodeAssist", "tok", "{}");
        assert!(call.contains("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist") && call.contains("Bearer tok"), "{call}");

        use base64::Engine;
        let now = crate::board::iso("2026-11-04T10:00:00").unwrap();
        let file = r#"{"token":{"access_token":"ya29.a-b_c","refresh_token":"never-used","expiry":"2026-11-04T11:00:00.5+00:00"}}"#;
        assert_eq!(agy_token(file, now).as_deref(), Ok("ya29.a-b_c"));
        let keyring = format!("go-keyring-base64:{}", base64::engine::general_purpose::STANDARD.encode(file));
        assert_eq!(agy_token(&keyring, now).as_deref(), Ok("ya29.a-b_c"));
        assert_eq!(agy_token(r#"{"access_token":"tok","expiry_date":1794787200000}"#, 1794787199).as_deref(), Ok("tok"));
        assert_eq!(agy_token(r#"{"access_token":"tok","expiry_date":1794787200000}"#, 1794787200), Err("token expired"));
        assert_eq!(agy_token(r#"{"token":{"access_token":"tok","expiry":"2026-11-04T09:00:00Z"}}"#, now), Err("token expired"));
        assert_eq!(agy_token(r#"{"refresh_token":"only"}"#, now), Err("no login"));
        assert_eq!(agy_token("not json", now), Err("no login"));
        assert_eq!(agy_token(r#"{"access_token":"a\"b"}"#, now), Err("usage shape changed"));
        assert_eq!(agy_token("go-keyring-base64:!!", now), Err("usage shape changed"));
    }

    // Chain: R12, #2359 A1/A2: structured Claude caps and consumer Gemini quotas reach every public heartbeat row.
    // Risk: a partial native reply looks healthy while silently dropping a model's more restrictive window.
    #[test]
    fn model_windows_survive_poll_to_heartbeat() {
        let d = dir("model-windows");
        let reset = crate::board::stamp(NOW + 3600);
        let claude = serde_json::json!({"seven_day": {"utilization": 34, "resets_at": reset}, "seven_day_fable": null,
            "limits": [{"group": "weekly", "percent": 2, "resets_at": reset, "scope": {"model": {"display_name": "Fable"}}}]});
        let summary = serde_json::json!({"groups": [
            {"buckets": [{"bucketId": "gemini-5h", "remainingFraction": 1, "resetTime": reset},
                {"bucketId": "gemini-weekly", "remainingFraction": 0.26, "resetTime": reset}]},
            {"buckets": [{"bucketId": "3p-5h", "remainingFraction": 1}, {"bucketId": "3p-weekly", "remainingFraction": 1}]}]});
        let p = code_assist_with("invented-token", |call| {
            if call.contains(":loadCodeAssist") { return Ok(serde_json::json!({"cloudaicompanionProject": "invented-project"})); }
            assert!(call.contains("https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary"), "consumer quotas need the daily endpoint");
            Ok(summary.clone())
        });
        assert_eq!(p.unknown, None);
        poll_with(&d, NOW, |rt, _| match rt {
            "claude" => Poll { windows: oauth_windows(&claude).unwrap(), ..Default::default() },
            "antigravity" => p.clone(),
            _ => Poll::failed("no login"),
        }).unwrap();
        let rows: Vec<_> = Host::read(&d).usage_lines(NOW).iter().filter_map(|l| Row::parse(l)).filter_map(|r| match r.usage {
            Usage::Window { window, at } => Some((r.rt, window.name, window.used, window.reset, at)),
            _ => None,
        }).collect();
        assert_eq!(rows, [
            ("claude".into(), "seven_day".into(), 34, Some(NOW + 3600), NOW),
            ("claude".into(), "seven_day_fable".into(), 2, Some(NOW + 3600), NOW),
            ("antigravity".into(), "gemini-5h".into(), 0, Some(NOW + 3600), NOW),
            ("antigravity".into(), "gemini-weekly".into(), 74, Some(NOW + 3600), NOW),
            ("antigravity".into(), "3p-5h".into(), 0, None, NOW),
            ("antigravity".into(), "3p-weekly".into(), 0, None, NOW),
        ]);
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: R12: new structured limits retain legacy names and cannot turn an unreadable model cap into headroom.
    #[test]
    fn model_windows_claude_scoped_limits() {
        let v = serde_json::json!({"seven_day": {"utilization": 34}, "seven_day_fable": {"utilization": 1},
            "limits": [{"group": "weekly", "percent": 2, "scope": {"model": {"display_name": "Fable"}}},
                {"group": "session", "percent": 10, "scope": {"model": {"id": "new-model"}}},
                {"group": "session", "percent": 4}]});
        assert_eq!(oauth_windows(&v).unwrap().iter().map(|w| (w.name.as_str(), w.used)).collect::<Vec<_>>(),
            [("seven_day", 34), ("seven_day_fable", 2), ("five_hour_new-model", 10), ("five_hour", 4)]);
        for limit in [
            serde_json::json!({"group": "monthly", "percent": 2}),
            serde_json::json!({"group": "weekly", "scope": {"model": {"display_name": "Fable"}}}),
            serde_json::json!({"group": "weekly", "percent": 2, "scope": "changed"}),
            serde_json::json!({"group": "weekly", "percent": 2, "scope": {"unsupported": true}}),
            serde_json::json!({"group": "weekly", "percent": 2, "scope": {"model": {"display_name": "private@example.invalid"}}}),
        ] {
            assert_eq!(oauth_windows(&serde_json::json!({"seven_day": {"utilization": 34}, "limits": [limit]})), Err("usage shape changed"));
        }
    }

    // Chain: R12: GCP keeps production quotas; a failed call or unreadable model family never becomes a partial poll.
    #[test]
    fn model_windows_code_assist_routes_and_unknown() {
        let valid = serde_json::json!({"buckets": [{"bucketId": "3p-weekly", "remainingFraction": 1}]});
        for gcp in [false, true] {
            let p = code_assist_with("invented-token", |call| {
                if call.contains(":loadCodeAssist") { return Ok(serde_json::json!({"currentTier": {"usesGcpTos": gcp}})); }
                let host = if gcp { "cloudcode-pa.googleapis.com" } else { "daily-cloudcode-pa.googleapis.com" };
                assert!(call.contains(&format!("https://{host}/v1internal:retrieveUserQuotaSummary")));
                Ok(serde_json::json!({"groups": [valid]}))
            });
            assert_eq!(p.unknown, None);
            assert_eq!(p.windows.len(), 1);
        }
        for bad in [serde_json::json!({}), serde_json::json!({"buckets": "changed"}), serde_json::json!({"buckets": []}),
            serde_json::json!({"buckets": [{"bucketId": "gemini-weekly"}]})] {
            let p = code_assist_with("invented-token", |call| {
                if call.contains(":loadCodeAssist") { return Ok(serde_json::json!({})); }
                Ok(serde_json::json!({"groups": [valid, bad]}))
            });
            assert_eq!((p.windows.len(), p.unknown.as_deref()), (0, Some("usage shape changed")));
        }
        let p = code_assist_with("invented-token", |call| {
            if call.contains(":loadCodeAssist") { return Ok(serde_json::json!({})); }
            Err("call refused 403".into())
        });
        assert_eq!((p.windows.len(), p.unknown.as_deref()), (0, Some("call refused 403")));
        let p = code_assist_with("invented-token", |_| Ok(serde_json::json!({"currentTier": {"usesGcpTos": "changed"}})));
        assert_eq!((p.windows.len(), p.unknown.as_deref()), (0, Some("usage shape changed")));
    }

    // Chain: capacity.md: Antigravity keeps one default account per host until a per-account `agy` home is qualified. Risk:
    // a second label or a missing native status reaches the launch path, or a poll row lists a token. Protection: every
    // entry point keeps `default`, a fleet `use` of another id is unmet, and rows show only the polled facts.
    #[test]
    fn antigravity_has_one_default_account_with_rows_and_policy() {
        let rt = "antigravity";
        let d = dir("one-account");
        assert!(accounts::add(&d, rt, "work").unwrap_err().contains("one `default` account"), "add refuses before any login");
        std::fs::write(d.join(accounts::FILE), serde_json::json!({rt: [{"label": "default", "enabled": true}, {"label": "work", "enabled": true}]}).to_string()).unwrap();
        assert!(accounts::read_policy(&d).unwrap_err().contains("one `default` account"), "a hand-edited list stays refused");
        std::fs::remove_file(d.join(accounts::FILE)).unwrap();
        assert_eq!(accounts::environment(rt, Path::new("elsewhere")), None, "no empty variable name reaches a child");
        assert_eq!(super::super::account_email(rt, accounts::DEFAULT), None, "the bare binary is never run as a status probe");
        assert_eq!(accounts::home(rt, accounts::DEFAULT), None);

        let at = |line: &str| line.starts_with(&format!("limit {rt} "));
        let poll = |id: Option<String>, used: u8| poll_with(&d, NOW, |r, label| {
            if r != rt { return Poll::failed("no login"); }
            assert_eq!(label, accounts::DEFAULT);
            Poll { id: id.clone(), windows: vec![Window { name: "gemini-5h".into(), used, reset: None }], unknown: None }
        }).unwrap();
        poll(Some(fingerprint("project-1")), 30);
        let lines = Host::read(&d).usage_lines(NOW);
        let row = format!("limit {rt} default id {} window gemini-5h used 30 reset - at {}", fingerprint("project-1"), crate::board::stamp(NOW));
        assert_eq!(lines.iter().filter(|l| at(l)).cloned().collect::<Vec<_>>(), [row]);
        assert_eq!(Host::read(&d).active(rt, NOW).as_deref(), Some(accounts::DEFAULT));
        let rows = Host::read(&d).rows("h", NOW, |_, _| None);
        let own = rows.iter().find(|r| r.runtime == rt && r.label == accounts::DEFAULT).unwrap();
        assert_eq!(own.id.as_deref(), Some(fingerprint("project-1").as_str()), "`taskq accounts` shows the polled id");

        let policy = |preferred: Option<String>, avoid: &[String]| BTreeMap::from([(rt.to_string(), RuntimePolicy { preferred, avoid: avoid.iter().cloned().collect() })]);
        fleet(&d, NOW, policy(Some(id("elsewhere")), &[]), vec![], None);
        assert!(Host::read(&d).usage_lines(NOW).contains(&format!("account-policy unmet {rt} {}", id("elsewhere"))), "another account has no home here");
        assert_eq!(Host::read(&d).active(rt, NOW).as_deref(), Some(accounts::DEFAULT), "unmet keeps the default");
        fleet(&d, NOW, policy(None, &[fingerprint("project-1")]), vec![], None);
        assert!(Host::read(&d).off(rt, NOW), "an avoided only account starts no new work");
    }

    fn fleet(install: &Path, at: i64, policy: Policy, rows: Vec<String>, error: Option<&str>) {
        std::fs::write(install.join(FLEET), serde_json::to_string(&Fleet { at, policy, rows, error: error.map(String::from) }).unwrap()).unwrap();
    }

    fn comment(id: u64, body: &str, trusted: bool, edited: bool) -> Comment {
        Comment { id, created: 0, body: body.into(), trusted, edited, author: String::new(), read: None }
    }

    // Chain: §3 Runtime accounts (#2341 A1): each native reply shape reads as windows or one fixed reason. Risk: a
    // changed reply reads as a healthy account, or a provider's text reaches the board.
    #[test]
    fn routes_read_windows_or_a_fixed_reason() {
        let account = serde_json::json!({"id": 2, "result": {"account": {"type": "chatgpt", "email": "Owner@Example.com"}}});
        let limits = serde_json::json!({"id": 3, "result": {"rateLimits": {"primary": {"usedPercent": 67.6, "windowDurationMins": 10080, "resetsAt": 1794379777}, "secondary": null}}});
        let p = rpc_poll(&account, &limits);
        assert_eq!(p.id.as_deref(), Some(fingerprint("owner@example.com").as_str()), "the fingerprint ignores case");
        assert_eq!(p.windows, [Window { name: "10080m".into(), used: 68, reset: Some(1794379777) }]);
        assert_eq!(rpc_poll(&serde_json::json!({"id": 2, "result": {"account": null}}), &limits).unknown.as_deref(), Some("no login"));
        assert_eq!(rpc_poll(&account, &serde_json::json!({"id": 3, "error": {"message": "secret words"}})).unknown.as_deref(), Some("call refused"));
        assert_eq!(rpc_poll(&account, &serde_json::json!({"id": 3, "result": {"rateLimits": {}}})).unknown.as_deref(), Some("usage shape changed"));
        let by_workspace = rpc_poll(&serde_json::json!({"id": 2, "result": {"account": {"type": "chatgpt"}, "workspaceRouting": {"chatgptAccountId": "acc-1"}}}), &limits);
        assert_eq!(by_workspace.id, Some(fingerprint("acc-1")));

        let v = serde_json::json!({"five_hour": {"utilization": 12.4, "resets_at": "2026-11-04T10:00:00.123456+00:00"}, "seven_day": {"utilization": 100.0, "resets_at": null},
            "seven_day_opus": null, "extra_usage": {"utilization": 90.0}});
        assert_eq!(oauth_windows(&v).unwrap(), [
            Window { name: "five_hour".into(), used: 12, reset: crate::board::iso("2026-11-04T10:00:00Z") },
            Window { name: "seven_day".into(), used: 100, reset: None },
        ], "credits and empty windows are no window");
        assert_eq!(oauth_windows(&serde_json::json!({"renamed": {"utilization": 1}})), Err("usage shape changed"));
        assert_eq!(utc("2026-11-04T10:00:00+02:00"), crate::board::iso("2026-11-04T08:00:00Z"));
        assert_eq!(utc("2026-11-04T10:00:00Z"), crate::board::iso("2026-11-04T10:00:00Z"));
        assert_eq!(utc("soon"), None);

        let token = |extra: &str| format!(r#"{{"claudeAiOauth":{{"accessToken":"sk-a_b.c"{extra}}}}}"#);
        assert_eq!(access_token(&token(",\"expiresAt\":1800000001000"), NOW).as_deref(), Ok("sk-a_b.c"));
        assert_eq!(access_token(&token(",\"expiresAt\":1800000000000"), NOW), Err("token expired"), "an expired token is never refreshed");
        assert_eq!(access_token("{}", NOW), Err("no login"));
        assert_eq!(access_token(r#"{"claudeAiOauth":{"accessToken":"a\"\nurl = evil"}}"#, NOW), Err("usage shape changed"), "nothing reaches the curl config but token characters");
    }

    // Chain: §3 Runtime accounts (#2341 A1): the app-server replies out of order among notifications while stdin stays
    // open; a silent server is killed at the deadline; a changed login file voids the result. Risk: a hung child blocks
    // every later poll, or a token refresh passes unnoticed.
    #[test]
    fn rpc_keeps_stdin_open_and_bounds_a_silent_server() {
        let python = crate::runtime::which("python3");
        if !python.is_absolute() { return; }
        let script = r#"
import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("id") == 3:
        print(json.dumps({"method": "remoteControl/status/changed"}), flush=True)
        print(json.dumps({"id": 3, "result": {"rateLimits": {"primary": {"usedPercent": 40, "windowDurationMins": 300}}}}), flush=True)
        print(json.dumps({"id": 2, "result": {"account": {"email": "a@example.com"}}}), flush=True)
"#;
        let mut c = crate::platform::command(&python);
        c.args(["-c", script]);
        let p = rpc(c, Duration::from_secs(20));
        assert_eq!((p.id, p.windows.len(), p.unknown), (Some(fingerprint("a@example.com")), 1, None));

        let mut silent = crate::platform::command(&python);
        silent.args(["-c", "import time; time.sleep(60)"]);
        let started = Instant::now();
        assert_eq!(rpc(silent, Duration::from_millis(300)).unknown.as_deref(), Some("timeout"));
        assert!(started.elapsed() < Duration::from_secs(20), "the deadline ends the child");

        let d = dir("guard");
        let file = d.join("auth.json");
        std::fs::write(&file, "one").unwrap();
        let ok = Poll { id: Some("00000000".into()), windows: vec![Window { name: "300m".into(), used: 1, reset: None }], unknown: None };
        assert_eq!(guarded(&file, || ok.clone()), ok, "an unchanged login file passes");
        let refreshed = guarded(&file, || { std::fs::write(&file, "two").unwrap(); ok.clone() });
        assert_eq!((refreshed.windows.len(), refreshed.unknown.as_deref()), (0, Some("login file changed")));
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A2): rows carry a fingerprint and fixed reasons, never an email or a token; a
    // poll never clears a hold. Risk: a public heartbeat leaks a login, or a good poll restarts a limited account.
    #[test]
    fn rows_carry_no_secret_and_a_poll_never_clears_a_hold() {
        let d = dir("rows");
        list(&d, &[("default", true), ("work", true), ("old", false)]);
        record_limit(&d, "codex", "work", NOW - 10, Some(NOW + 9000)).unwrap();
        let held = std::fs::read(d.join(accounts::STATE)).unwrap();
        poll(&d, NOW, &[("work", 1)]);
        assert_eq!(std::fs::read(d.join(accounts::STATE)).unwrap(), held, "a poll writes no quota record");
        let h = Host::read(&d);
        assert_eq!(h.limit_end("codex", "work"), Some(NOW + 9000));
        assert_ne!(h.active("codex", NOW).as_deref(), Some("work"), "the best usage cannot lift a hold");

        let lines = h.usage_lines(NOW);
        let text = lines.join("\n");
        assert!(!text.contains('@') && !text.contains("example.com") && !text.contains("sk-"), "{text}");
        let stamp = crate::board::stamp;
        assert!(lines.contains(&format!("limit codex work id {} window 10080m used 1 reset {} at {}", id("work"), stamp(NOW + 3600), stamp(NOW))), "{text}");
        assert!(lines.contains(&format!("limit codex default id {} unknown token expired", id("default"))), "{text}");
        assert!(lines.contains(&"limit codex old id - unknown disabled".to_string()), "{text}");
        assert!(lines.contains(&"limit claude default id - unknown no login".to_string()), "{text}");
        for l in lines.iter().filter(|l| l.starts_with("limit ")) {
            assert_eq!(Row::parse(l).unwrap().line(), *l, "a row round-trips");
        }
        assert_eq!(Row::parse("limit codex until 2026-11-04T10:00:00Z"), None, "the hold line is no usage row");
        assert_eq!(Row::parse("limit codex probe 2026-11-04T10:00:00Z"), None);
        assert!(Host::read(&d).usage_lines(NOW + TTL + 1).iter().any(|l| l.ends_with("unknown poll stale")), "an old poll is unknown, never healthy");
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A3): usage ranks new starts, the floor orders and never holds, and a resume keeps
    // its log's account. Risk: a low account refuses work, or a resume changes login mid-attempt.
    #[test]
    fn ranking_groups_floor_and_kept_accounts() {
        let d = dir("rank");
        list(&d, &[("default", true), ("work", true), ("spare", true)]);
        let active = |now: i64| Host::read(&d).active("codex", now);
        assert_eq!(active(NOW).as_deref(), Some("default"), "no usage: list order");

        poll(&d, NOW, &[("default", 90), ("work", 40), ("spare", 40)]);
        assert_eq!(active(NOW).as_deref(), Some("work"), "greatest remaining above the floor; a tie keeps list order");

        poll(&d, NOW, &[("default", 90), ("work", 90)]);
        assert_eq!(Host::read(&d).usage.remaining("codex", "spare", NOW), Some(60), "a failed poll keeps the reading until it expires");
        std::fs::remove_file(d.join(super::super::headroom::FILE)).unwrap();
        poll(&d, NOW, &[("default", 90), ("work", 90)]);
        assert_eq!(active(NOW).as_deref(), Some("spare"), "an unknown account beats one below the floor");

        poll(&d, NOW, &[("default", 95), ("work", 90), ("spare", 100)]);
        assert_eq!(active(NOW).as_deref(), Some("work"), "every account below the floor still takes work: greatest first");
        std::fs::write(d.join("host.json"), r#"{"account_floor":5}"#).unwrap();
        assert_eq!(active(NOW).as_deref(), Some("work"));
        std::fs::write(d.join("host.json"), r#"{"account_floor":12}"#).unwrap();
        assert_eq!(Host::read(&d).usage.remaining("codex", "work", NOW), Some(10));
        assert_eq!(active(NOW).as_deref(), Some("work"), "the floor from host.json orders, it never refuses");
        assert_eq!(active(NOW + TTL + 1).as_deref(), Some("default"), "expired usage is unknown: list order");

        // A new start takes the ranked account, a resume keeps its log's account even when ranked last.
        let log = d.join("T1-2.log");
        std::fs::write(&log, "{\"type\":\"taskq.rules\"}\n").unwrap();
        let start = claim(&d, "codex", None, NOW, &log).unwrap();
        assert_eq!(start, "work");
        mark(&log, "spare").unwrap();
        assert_eq!(claim(&d, "codex", Some(of_log(&log).as_str()), NOW, &log).unwrap(), "spare", "resume and recovery keep the log's account");
        std::fs::write(d.join(FLEET), serde_json::to_string(&Fleet { at: NOW, policy: BTreeMap::from([("codex".to_string(), RuntimePolicy { preferred: None, avoid: BTreeSet::from([id("spare")]) })]), ..Default::default() }).unwrap()).unwrap();
        assert_eq!(claim(&d, "codex", Some("spare"), NOW, &log).unwrap(), "spare", "policy moves new starts only, never a running child");

        // A limited account is skipped whatever its usage; with every account limited the hold still stops the runtime.
        list(&d, &[("work", true)]);
        std::fs::remove_file(d.join(FLEET)).unwrap();
        record_limit(&d, "codex", "work", NOW, Some(NOW + 99)).unwrap();
        assert_eq!(active(NOW), None);
        assert_eq!(Host::read(&d).hold("codex", NOW), Some((NOW + 99, true)));
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A3, A5): a trusted heartbeat row with the same id ranks an account down on the
    // other hosts; no id, no merge. Risk: a label match merges two different logins.
    #[test]
    fn other_hosts_rows_merge_by_id_only() {
        let d = dir("merge");
        list(&d, &[("default", true), ("work", true)]);
        poll(&d, NOW, &[("default", 10), ("work", 30)]);
        let other = |who: &str, used: u8, at: i64| Row { rt: "codex".into(), label: "x".into(), id: Some(id(who)), usage: Usage::Window { window: Window { name: "300m".into(), used, reset: None }, at } }.line();
        fleet(&d, NOW, Policy::new(), vec![other("default", 95, NOW - 10), other("nobody", 99, NOW)], None);
        let h = Host::read(&d);
        assert_eq!(h.usage.remaining("codex", "default", NOW), Some(5), "the same id on another host counts");
        assert_eq!(h.usage.remaining("codex", "work", NOW), Some(70), "another id does not");
        assert_eq!(h.active("codex", NOW).as_deref(), Some("work"));
        fleet(&d, NOW, Policy::new(), vec![other("default", 95, NOW - TTL - 1)], None);
        assert_eq!(Host::read(&d).active("codex", NOW).as_deref(), Some("default"), "an old row counts for nothing");
        fleet(&d, NOW - FLEET_TTL - 1, Policy::new(), vec![other("default", 95, NOW)], None);
        assert_eq!(Host::read(&d).active("codex", NOW).as_deref(), Some("default"), "a stale file counts for nothing");
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A4): policy folds from trusted unedited comments in id order and every write is
    // refused under TASKQ_ROLE. Risk: a task comment or a worker redirects every host's login.
    #[test]
    fn fleet_fold_write_and_refusal() {
        let (a, b) = ("aaaaaaaa", "bbbbbbbb");
        let p = fold(&[
            comment(5, &format!("taskq: account use codex {b}"), true, false),
            comment(1, &format!("taskq: account use codex {a}"), true, false),
            comment(2, &format!("taskq: account avoid codex {a}"), true, false),
            comment(3, &format!("taskq: account avoid claude {b}"), false, false),
            comment(4, &format!("taskq: account avoid claude {a}"), true, true),
            comment(6, "taskq: account avoid codex nothex!!", true, false),
            comment(7, &format!("taskq: account use codex-app {a}"), true, false),
        ]);
        assert_eq!(p["codex"].preferred.as_deref(), Some(b), "the later use replaces the earlier one");
        assert_eq!(p["codex"].avoid, BTreeSet::from([a.to_string()]));
        assert_eq!(p.keys().collect::<Vec<_>>(), ["codex"], "untrusted, edited, malformed and account-less rows fold to nothing");
        let cleared = fold(&[comment(1, &format!("taskq: account avoid codex {a}"), true, false), comment(2, &format!("taskq: account clear codex {a}"), true, false)]);
        assert!(cleared.is_empty(), "clear removes either");

        let role = std::ffi::OsString::from("worker");
        assert!(refusal(Some(&role), None).is_some() && refusal(None, Some(&role)).is_some() && refusal(None, None).is_none());
        let pm = std::ffi::OsString::from("pm");
        assert!(refusal(Some(&pm), None).is_some(), "every role, not only a worker");

        let board = crate::board::Fake::default();
        assert!(fleet_write(&board, "use", "codex-app", a).is_err() && fleet_write(&board, "use", "codex", "UPPER").is_err() && fleet_write(&board, "drop", "codex", a).is_err());
        assert!(board.issues(None).unwrap().is_empty(), "a refused write creates nothing");
        fleet_write(&board, "use", "codex", a).unwrap();
        fleet_write(&board, "avoid", "codex", b).unwrap();
        let issues = board.issues(None).unwrap();
        assert_eq!(issues.len(), 1, "one policy issue");
        assert!(issues[0].labels.iter().any(|l| l == POLICY_LABEL) && !crate::service::is_task(&issues[0]));
        let comments = board.comments(issues[0].number).unwrap();
        assert_eq!(comments.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), [format!("taskq: account use codex {a}"), format!("taskq: account avoid codex {b}")]);

        // The refresh reads the issue, this board's other hosts, and one comment read per change.
        let d = dir("fleet");
        let mut cache = FleetCache::default();
        cache.refresh(&board, &d, &[], "me", NOW).unwrap();
        assert!(!cache.due(NOW + EVERY - 1) && cache.due(NOW + EVERY));
        let v = View::read(&d);
        assert_eq!(v.local_reason(NOW), None);
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A4, A5): a host applies policy to its own homes, reports what it cannot, and falls
    // back to local ranking out loud. Risk: a `use` with no login here silently runs elsewhere, or a dead link holds work.
    #[test]
    fn policy_applies_to_own_homes_and_says_when_it_cannot() {
        let d = dir("policy");
        list(&d, &[("default", true), ("work", true)]);
        poll(&d, NOW, &[("default", 10), ("work", 90)]);
        let lines = |d: &Path| Host::read(d).usage_lines(NOW).into_iter().filter(|l| l.starts_with("account-policy ")).collect::<Vec<_>>();
        assert_eq!(lines(&d), ["account-policy local no connection"], "no connection keeps local ranking and says so");
        let policy = |preferred: Option<String>, avoid: &[String]| BTreeMap::from([("codex".to_string(), RuntimePolicy { preferred, avoid: avoid.iter().cloned().collect() })]);

        fleet(&d, NOW, policy(Some(id("work")), &[]), vec![], None);
        assert_eq!(Host::read(&d).active("codex", NOW).as_deref(), Some("work"), "`use` beats better usage");
        assert_eq!(lines(&d), [format!("account-policy use codex {}", id("work"))]);

        fleet(&d, NOW, policy(Some(id("elsewhere")), &[]), vec![], None);
        assert_eq!(lines(&d), [format!("account-policy unmet codex {}", id("elsewhere"))], "no home for the id: unmet, never a copied login");
        assert_eq!(Host::read(&d).active("codex", NOW).as_deref(), Some("default"), "unmet keeps ranking");

        fleet(&d, NOW, policy(None, &[id("default")]), vec![], None);
        assert_eq!(Host::read(&d).active("codex", NOW).as_deref(), Some("work"), "avoid moves new starts");
        let h = Host::read(&d);
        assert!(!h.off("codex", NOW));

        fleet(&d, NOW, policy(None, &[id("default"), id("work")]), vec![], None);
        let h = Host::read(&d);
        assert_eq!((h.active("codex", NOW), h.off("codex", NOW)), (None, true), "avoid of every account admits nothing");
        assert_eq!(h.off_reason("codex"), "every account disabled or avoided on this host");

        fleet(&d, NOW, policy(None, &[id("default"), id("work")]), vec![], Some("policy issue unreadable"));
        let h = Host::read(&d);
        assert_eq!(h.active("codex", NOW).as_deref(), Some("default"), "an unreadable issue keeps local ranking");
        assert_eq!(lines(&d), ["account-policy local policy issue unreadable"]);

        fleet(&d, NOW - FLEET_TTL - 1, policy(None, &[id("default")]), vec![], None);
        assert_eq!(lines(&d), ["account-policy local policy stale"], "a dead speaker cannot pin old policy");
        assert_eq!(Host::read(&d).active("codex", NOW).as_deref(), Some("default"));
        std::fs::write(d.join(FLEET), "corrupt").unwrap();
        assert_eq!(lines(&d), ["account-policy local policy file unreadable"]);
        assert!(Host::read(&d).error.is_none(), "policy loss never turns into unknown availability");
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A3, A5): a poll moves a new start with reason `usage`, policy with `fleet`, and
    // `taskq accounts` reads id, usage and policy locally and from a heartbeat. Risk: the owner cannot see why starts moved.
    #[test]
    fn usage_and_fleet_switches_and_rows() {
        let d = dir("switches");
        list(&d, &[("default", true), ("work", true)]);
        poll(&d, NOW, &[("default", 50), ("work", 50)]);
        let mut seen = BTreeMap::new();
        accounts::switches(&Host::read(&d), NOW, &mut seen);
        poll(&d, NOW, &[("default", 95), ("work", 20)]);
        let lines = |h: &Host, seen: &mut BTreeMap<String, Option<String>>| accounts::switches(h, NOW, seen).iter().map(|s| s.line()).collect::<Vec<_>>();
        assert_eq!(lines(&Host::read(&d), &mut seen), ["account codex default -> work (usage)"]);
        assert!(Host::read(&d).usable("codex", "default", NOW), "a prepared launch keeps its account when ranking moves");
        assert!(!Host::read(&d).usable("codex", "absent", NOW));

        let policy = |preferred: Option<String>, avoid: &[String]| BTreeMap::from([("codex".to_string(), RuntimePolicy { preferred, avoid: avoid.iter().cloned().collect() })]);
        fleet(&d, NOW, policy(Some(id("default")), &[]), vec![], None);
        assert_eq!(lines(&Host::read(&d), &mut seen), ["account codex work -> default (fleet)"]);
        fleet(&d, NOW, policy(None, &[id("default")]), vec![], None);
        assert_eq!(lines(&Host::read(&d), &mut seen), ["account codex default -> work (fleet)"]);

        fleet(&d, NOW, policy(Some(id("work")), &[]), vec![], None);
        let h = Host::read(&d);
        let rows = h.rows("mac", NOW, |_, _| None);
        let work = rows.iter().find(|r| r.runtime == "codex" && r.label == "work").unwrap();
        assert_eq!((work.id.as_deref(), work.policy.as_deref()), (Some(id("work").as_str()), Some("use")));
        assert_eq!(work.usage.len(), 1);
        assert!(work.usage[0].contains("used 20%"), "{:?}", work.usage);
        assert!(work.text().contains(&format!("id {}", id("work"))) && work.text().contains("policy use"), "{}", work.text());
        assert_eq!(work.json()["id"], id("work"));
        assert_eq!(work.json()["policy"], "use");
        assert!(rows.iter().find(|r| r.label == "default" && r.runtime == "codex").unwrap().policy.is_none());

        // Another host's heartbeat: `account` lines stay as they were; usage rows and policy join them; a row with no
        // `account` line still shows, and a hold line is no usage row.
        let body = format!("account codex work on 2026-01-01T00:00:00Z\nlimit codex 2026-01-01T00:00:00Z\nlimit codex work id {} window 10080m used 20 reset - at 2026-01-01T00:00:00Z\nlimit claude default id - unknown no login\naccount-policy use codex {}\naccount-policy unmet codex {}\n", id("work"), id("work"), id("elsewhere"));
        let rows = accounts::heartbeat_rows("win", &body);
        let work = rows.iter().find(|r| r.runtime == "codex" && r.label == "work").unwrap();
        assert_eq!((work.id.as_deref(), work.usage.len(), work.policy.as_deref()), (Some(id("work").as_str()), 1, Some("use")));
        let claude = rows.iter().find(|r| r.runtime == "claude").unwrap();
        assert_eq!((claude.label.as_str(), claude.usage.as_slice()), ("default", ["unknown no login".to_string()].as_slice()));
        let unmet = rows.iter().find(|r| r.label == "-").unwrap();
        assert_eq!((unmet.id.as_deref(), unmet.policy.as_deref(), unmet.enabled), (Some(id("elsewhere").as_str()), Some("unmet"), false));

        std::fs::remove_file(d.join(FLEET)).unwrap();
        let h = Host::read(&d);
        assert!(h.rows("mac", NOW, |_, _| None).iter().all(|r| r.policy.as_deref() == Some("local no connection")), "no policy says why on every row");
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: §3 Runtime accounts (#2341 A3): `account_floor` is owner config with one shape. Risk: a typo silently changes ranking.
    #[test]
    fn account_floor_shape() {
        assert_eq!(floor_of(&serde_json::json!({})), Ok(None));
        assert_eq!(floor_of(&serde_json::json!({"account_floor": 0})), Ok(Some(0)));
        assert_eq!(floor_of(&serde_json::json!({"account_floor": 100})), Ok(Some(100)));
        for bad in [serde_json::json!({"account_floor": 101}), serde_json::json!({"account_floor": -1}), serde_json::json!({"account_floor": 1.5}), serde_json::json!({"account_floor": "15"})] {
            assert!(floor_of(&bad).is_err(), "{bad}");
        }
    }

    // #2341 A1 instrument: a real read-only poll on this host, run by hand on each host from a checkout:
    #[test]
    fn keychain_reads_the_item_and_never_reads_a_prompt_as_a_login() {
        assert_eq!(keychain_args("Claude Code-credentials", ""), ["find-generic-password", "-s", "Claude Code-credentials", "-w"]);
        assert_eq!(keychain_args("gemini", "antigravity"), ["find-generic-password", "-s", "gemini", "-a", "antigravity", "-w"]);
        assert_eq!(keychain_reply(Some(0), b"{\"a\":1}\n"), Ok(Some("{\"a\":1}".to_string())));
        assert_eq!(keychain_reply(Some(0), b"\n"), Ok(None));
        assert_eq!(keychain_reply(Some(44), b""), Ok(None));
        assert_eq!(keychain_reply(Some(36), b"").unwrap_err(), "keychain would prompt");
        assert_eq!(keychain_reply(Some(51), b"").unwrap_err(), "keychain read failed 51");
        assert_eq!(keychain_reply(None, b"").unwrap_err(), "keychain read failed");
    }

    #[test]
    fn app_server_failure_names_its_exit_code_only_when_it_exited() {
        assert_eq!(rpc_failure("app-server failed", Some(1)), "app-server exit 1");
        assert_eq!(rpc_failure("app-server failed", None), "app-server failed");
        assert_eq!(rpc_failure("timeout", Some(1)), "timeout");
    }

    // `cargo test --locked --bin taskq native_poll_qualifies -- --ignored --nocapture`. It prints rows only: fingerprints,
    // percents and fixed reasons, and fails when a login file changes or a row carries an email.
    #[test]
    #[ignore = "reads this host's real logins and calls the providers"]
    fn native_poll_qualifies() {
        let install = crate::platform::default_install();
        let policy = accounts::read_policy(&install).unwrap();
        let mut ids = Ids::default();
        for rt in super::super::CLI_RUNTIMES.iter().filter(|rt| accounts::applies(rt)) {
            let labels = policy.get(*rt).map_or(vec![accounts::DEFAULT.to_string()], |l| l.iter().filter(|e| e.enabled).map(|e| e.label.clone()).collect());
            for label in labels {
                let home = accounts::home(rt, &label).or_else(|| accounts::base(rt)).unwrap();
                let files: Vec<PathBuf> = ["auth.json", ".credentials.json"].iter().map(|f| home.join(f)).chain(if super::super::of(rt).has("usage-code-assist") && super::super::which(super::super::of(rt).bin).is_absolute() { agy_login_files() } else { vec![] }).collect();
                let sign = || files.iter().map(|f| std::fs::read(f).ok().map(|b| sha256(&b))).collect::<Vec<_>>();
                let before = sign();
                let started = Instant::now();
                let p = fetch(rt, &label, &mut ids);
                assert_eq!(sign(), before, "{rt} {label}: a poll changed a login file");
                let at = accounts::now();
                let id = p.id.clone();
                let row = |usage| Row { rt: rt.to_string(), label: label.clone(), id: id.clone(), usage }.line();
                let rows: Vec<String> = match &p.unknown {
                    Some(reason) => vec![row(Usage::Unknown(reason.clone()))],
                    None => p.windows.iter().map(|w| row(Usage::Window { window: w.clone(), at })).collect(),
                };
                for r in rows {
                    assert!(!r.contains('@'), "a row carries an email");
                    println!("{r}  ({} ms, login files unchanged)", started.elapsed().as_millis());
                }
            }
        }
    }
}
