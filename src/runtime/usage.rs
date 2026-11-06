//! Polled account usage and fleet account policy ([capacity.md](../docs/design/capacity.md#account-usage-and-fleet-policy)).
//! One host thread polls each enabled account; the rows it publishes carry a fingerprint, never an email or a token.
//! A poll only observes: it writes no login file and clears no quota record or hold.
use super::accounts;
use crate::board::{Board, Comment, Issue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
/// ponytail: an identity read starts a process; a login changes identity rarely.
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
    fn failed(reason: &str) -> Poll {
        Poll {
            unknown: Some(reason.into()),
            ..Default::default()
        }
    }
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

/// The account `id`: the first 8 hex digits of SHA-256 over the lowercased email or account id.
pub fn fingerprint(identity: &str) -> String {
    sha256(identity.trim().to_lowercase().as_bytes())[..8].to_string()
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 8
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_window(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn percent(used: f64) -> u8 {
    used.round().clamp(0.0, 100.0) as u8
}

fn fresh(at: i64, reset: Option<i64>, now: i64) -> bool {
    now - at <= TTL && at - now <= SKEW && reset.is_none_or(|r| now < r)
}

// ---------- heartbeat rows ----------

#[derive(Clone, Debug, PartialEq)]
pub enum Usage {
    Window {
        window: Window,
        at: i64,
    },
    /// A readable window of an incomplete poll: published, never aggregated. Its `limit-partial` prefix keeps it from
    /// readers through v4.6.79, which ignore `unknown` rows and would take the readable windows for the whole account.
    Partial {
        window: Window,
        at: i64,
    },
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
        let head = |kind: &str| {
            format!(
                "{kind} {} {} id {}",
                self.rt,
                self.label,
                self.id.as_deref().unwrap_or("-")
            )
        };
        let reading = |kind: &str, window: &Window, at: i64| {
            format!(
                "{} window {} used {} reset {} at {}",
                head(kind),
                window.name,
                window.used,
                window.reset.map_or("-".to_string(), crate::time::stamp),
                crate::time::stamp(at)
            )
        };
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
        let [rt, label, "id", id, rest @ ..] = w.as_slice() else {
            return None;
        };
        if !accounts::applies(rt) || !accounts::valid_label(label) || !(*id == "-" || valid_id(id))
        {
            return None;
        }
        let time = |t: &str| crate::time::iso(t).filter(|at| crate::time::stamp(*at) == t);
        let usage = match rest {
            ["window", name, "used", used, "reset", reset, "at", at] if valid_window(name) => {
                let window = Window {
                    name: name.to_string(),
                    used: used.parse().ok().filter(|u| *u <= 100)?,
                    reset: if *reset == "-" {
                        None
                    } else {
                        Some(time(reset)?)
                    },
                };
                let at = time(at)?;
                if partial {
                    Usage::Partial { window, at }
                } else {
                    Usage::Window { window, at }
                }
            }
            ["unknown", reason @ ..] if !partial && !reason.is_empty() => {
                Usage::Unknown(reason.join(" "))
            }
            _ => return None,
        };
        Some(Row {
            rt: rt.to_string(),
            label: label.to_string(),
            id: (*id != "-").then(|| id.to_string()),
            usage,
        })
    }

    /// The usage cell of `taskq accounts`.
    pub fn text(&self) -> String {
        match &self.usage {
            Usage::Window { window, .. } | Usage::Partial { window, .. } => format!(
                "{} used {}% reset {}",
                window.name,
                window.used,
                window.reset.map_or("-".to_string(), crate::time::utc)
            ),
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
        let Some(rest) = c.body.trim().strip_prefix("taskq: account ") else {
            continue;
        };
        let [op, rt, id] = rest.split(' ').collect::<Vec<_>>()[..] else {
            continue;
        };
        if !accounts::applies(rt) || !valid_id(id) {
            continue;
        }
        let p = policy.entry(rt.to_string()).or_default();
        match op {
            "use" => {
                p.preferred = Some(id.to_string());
                p.avoid.remove(id);
            }
            "avoid" => {
                if p.preferred.as_deref() == Some(id) {
                    p.preferred = None;
                }
                p.avoid.insert(id.to_string());
            }
            "clear" => {
                if p.preferred.as_deref() == Some(id) {
                    p.preferred = None;
                }
                p.avoid.remove(id);
            }
            _ => {}
        }
    }
    policy.retain(|_, p| p.preferred.is_some() || !p.avoid.is_empty());
    policy
}

fn refusal(
    role: Option<&std::ffi::OsStr>,
    attempt: Option<&std::ffi::OsStr>,
) -> Option<&'static str> {
    (role.is_some() || attempt.is_some()).then_some(
        "accounts fleet is refused under TASKQ_ROLE: only the owner changes fleet policy",
    )
}

/// Whether this process may write fleet policy; checked before the board opens.
pub fn fleet_allowed() -> Result<(), String> {
    match refusal(
        std::env::var_os("TASKQ_ROLE").as_deref(),
        std::env::var_os("TASKQ_ATTEMPT").as_deref(),
    ) {
        Some(reason) => Err(reason.into()),
        None => Ok(()),
    }
}

fn policy_issue(issues: &[Issue]) -> Option<&Issue> {
    issues
        .iter()
        .filter(|i| i.open && i.trusted && i.labels.iter().any(|l| l == POLICY_LABEL))
        .min_by_key(|i| i.number)
}

/// `taskq accounts fleet`: one trusted comment on the board's one policy issue, created when absent.
pub fn fleet_write(board: &dyn Board, op: &str, rt: &str, id: &str) -> Result<String, String> {
    if !matches!(op, "use" | "avoid" | "clear") {
        return Err("usage: taskq accounts fleet use|avoid|clear <runtime> <id>".into());
    }
    if !accounts::applies(rt) {
        return Err(format!(
            "runtime {rt} has no accounts; use claude, codex or antigravity"
        ));
    }
    if !valid_id(id) {
        return Err("id: 8 hex digits, as `taskq accounts` shows it".into());
    }
    let labels = [POLICY_LABEL.to_string()];
    board.ensure_labels(&labels)?;
    let issues = board.issues(None)?;
    let issue = match policy_issue(&issues) {
        Some(i) => i.number,
        None => board.create(POLICY_TITLE, POLICY_BODY, &labels)?,
    };
    board.comment(issue, &format!("taskq: account {op} {rt} {id}"))?;
    Ok(format!(
        "accounts fleet: {op} {rt} {id} on #{issue}; hosts whose connection approved this board apply it within {EVERY} s"
    ))
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
        if now < self.next {
            return false;
        }
        self.next = now + EVERY;
        true
    }

    fn policy(&mut self, board: &dyn Board, now: i64) -> Result<Policy, String> {
        let issues = board.issues(None)?;
        let Some(issue) = policy_issue(&issues) else {
            self.read = None;
            return Ok(Policy::new());
        };
        if let Some((number, updated, read, policy)) = &self.read {
            if *number == issue.number
                && *updated == issue.updated
                && *read >= issue.updated + SETTLE
            {
                return Ok(policy.clone());
            }
        }
        let policy = fold(&board.comments(issue.number)?);
        self.read = Some((issue.number, issue.updated, now, policy.clone()));
        Ok(policy)
    }

    /// The speaking project's refresh: this board's policy and its other hosts' rows, for one host's ranking.
    pub fn refresh(
        &mut self,
        board: &dyn Board,
        install: &Path,
        hosts: &[Issue],
        me: &str,
        now: i64,
    ) -> Result<(), String> {
        self.next = now + EVERY;
        let (policy, error) = match self.policy(board, now) {
            Ok(p) => (p, None),
            Err(_) => (Policy::new(), Some("policy issue unreadable".to_string())),
        };
        let rows = hosts
            .iter()
            .filter(|h| {
                h.trusted
                    && now - h.updated <= TTL
                    && crate::service::host_name(h).is_some_and(|n| n != me)
            })
            .flat_map(|h| h.body.lines())
            .filter(|l| Row::parse(l).is_some())
            .map(str::to_string)
            .collect();
        let fleet = Fleet {
            at: now,
            policy,
            rows,
            error,
        };
        crate::platform::write_synced(
            &install.join(FLEET),
            &serde_json::to_string(&fleet).map_err(|e| e.to_string())?,
        )
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
        Some(n) => n
            .as_u64()
            .filter(|n| *n <= 100)
            .map(|n| Some(n as u8))
            .ok_or_else(|| "host.json: account_floor must be an integer 0..100".to_string()),
    }
}

fn read_store(install: &Path) -> Store {
    crate::data::json(&install.join(FILE))
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

impl View {
    /// Unreadable inputs read as unknown usage or local ranking: neither holds work.
    pub fn read(install: &Path) -> View {
        let floor = crate::data::json(&install.join("host.json"))
            .ok()
            .flatten()
            .and_then(|v| floor_of(&v).ok().flatten());
        let (fleet, fleet_error) = match crate::data::json(&install.join(FLEET)) {
            Ok(None) => (None, None),
            Ok(Some(v)) => match serde_json::from_value(v) {
                Ok(f) => (Some(f), None),
                Err(_) => (None, Some("policy file unreadable".to_string())),
            },
            Err(_) => (None, Some("policy file unreadable".to_string())),
        };
        View {
            floor,
            store: read_store(install),
            observed: super::headroom::observed(install),
            fleet,
            fleet_error,
        }
    }

    fn fleet(&self, now: i64) -> Option<&Fleet> {
        self.fleet.as_ref().filter(|f| now - f.at <= FLEET_TTL)
    }

    /// Why the host ranks locally, `None` when fleet policy applies.
    pub fn local_reason(&self, now: i64) -> Option<String> {
        if let Some(e) = &self.fleet_error {
            return Some(e.clone());
        }
        let Some(f) = &self.fleet else {
            return Some("no connection".into());
        };
        if now - f.at > FLEET_TTL {
            return Some("policy stale".into());
        }
        f.error.clone()
    }

    fn policy(&self, rt: &str, now: i64) -> Option<&RuntimePolicy> {
        self.fleet(now)
            .filter(|f| f.error.is_none())?
            .policy
            .get(rt)
    }

    pub fn id(&self, rt: &str, label: &str) -> Option<&str> {
        self.store.get(rt)?.get(label)?.id.as_deref()
    }

    pub fn avoided(&self, rt: &str, label: &str, now: i64) -> bool {
        self.id(rt, label)
            .zip(self.policy(rt, now))
            .is_some_and(|(id, p)| p.avoid.contains(id))
    }

    pub fn preferred(&self, rt: &str, label: &str, now: i64) -> bool {
        self.id(rt, label)
            .zip(self.policy(rt, now))
            .is_some_and(|(id, p)| p.preferred.as_deref() == Some(id))
    }

    /// The smallest window's fresh remaining percent: per window the freshest reading of this home and of other hosts'
    /// rows with the same `id`. `None` is unknown.
    pub fn remaining(&self, rt: &str, label: &str, now: i64) -> Option<u8> {
        if self.incomplete(rt, label, now) {
            return None;
        }
        let mut windows: BTreeMap<&str, (i64, u8)> = BTreeMap::new();
        let mut add = |name, at, left| {
            if windows.get(name).is_none_or(|(old, _)| *old < at) {
                windows.insert(name, (at, left));
            }
        };
        for (name, (at, remaining, reset)) in self
            .observed
            .get(rt)
            .and_then(|a| a.get(label))
            .into_iter()
            .flatten()
        {
            if fresh(*at, *reset, now) {
                add(name.as_str(), *at, percent(remaining * 100.0));
            }
        }
        let rows: Vec<Row> = match (self.id(rt, label), self.fleet(now)) {
            (Some(id), Some(f)) => f
                .rows
                .iter()
                .filter_map(|l| Row::parse(l))
                .filter(|r| r.rt == rt && r.id.as_deref() == Some(id))
                .collect(),
            _ => vec![],
        };
        for r in &rows {
            if let Usage::Window { window, at } = &r.usage {
                if fresh(*at, window.reset, now) {
                    add(window.name.as_str(), *at, 100 - window.used);
                }
            }
        }
        windows.values().map(|(_, left)| *left).min()
    }

    /// A partial poll cannot establish aggregate headroom, locally or on a host sharing this account.
    pub(crate) fn incomplete(&self, rt: &str, label: &str, now: i64) -> bool {
        if self
            .store
            .get(rt)
            .and_then(|a| a.get(label))
            .is_some_and(|r| {
                !r.unreadable.is_empty() && fresh(r.unreadable_at.unwrap_or(r.at), None, now)
            })
        {
            return true;
        }
        let Some(id) = self.id(rt, label) else {
            return false;
        };
        self.fleet(now).is_some_and(|f| {
            f.rows.iter().filter_map(|l| Row::parse(l)).any(|r| {
                r.rt == rt
                    && r.id.as_deref() == Some(id)
                    && match r.usage {
                        Usage::Partial { .. } => true,
                        Usage::Unknown(reason) => reason.starts_with("quota unreadable "),
                        Usage::Window { .. } => false,
                    }
            })
        })
    }

    /// The best of `candidates` (enabled, limit ended, not avoided) in list order: `use`, then fresh remaining at or
    /// above the floor, then unknown, then fresh remaining below the floor. The floor only orders; it holds nothing.
    pub fn rank(&self, rt: &str, candidates: &[String], now: i64) -> Option<String> {
        if let Some(l) = candidates.iter().find(|l| self.preferred(rt, l, now)) {
            return Some(l.clone());
        }
        let floor = self.floor.unwrap_or(DEFAULT_FLOOR);
        let left: Vec<(&String, Option<u8>)> = candidates
            .iter()
            .map(|l| (l, self.remaining(rt, l, now)))
            .collect();
        let greatest = |above: bool| {
            let mut best: Option<(&String, u8)> = None;
            for (l, r) in &left {
                if let Some(r) = r.filter(|r| (*r >= floor) == above) {
                    if best.is_none_or(|(_, b)| r > b) {
                        best = Some((l, r));
                    }
                }
            }
            best.map(|(l, _)| l.clone())
        };
        greatest(true)
            .or_else(|| {
                left.iter()
                    .find(|(_, r)| r.is_none())
                    .map(|(l, _)| (*l).clone())
            })
            .or_else(|| greatest(false))
    }

    /// The heartbeat rows of one listed account; `None` of its windows is fresh when its last poll is old.
    pub fn lines(&self, rt: &str, label: &str, enabled: bool, now: i64) -> Vec<String> {
        let record = self.store.get(rt).and_then(|a| a.get(label));
        let id = record.and_then(|r| r.id.clone());
        let row = |usage| {
            Row {
                rt: rt.into(),
                label: label.into(),
                id: id.clone(),
                usage,
            }
            .line()
        };
        if !enabled {
            return vec![row(Usage::Unknown("disabled".into()))];
        }
        let Some(r) = record else {
            return vec![row(Usage::Unknown("not polled".into()))];
        };
        if !fresh(r.at, None, now) {
            return vec![row(Usage::Unknown("poll stale".into()))];
        }
        let mut lines: Vec<_> = match &r.unknown {
            Some(reason) => vec![row(Usage::Unknown(reason.clone()))],
            None => r
                .windows
                .iter()
                .map(|w| {
                    row(Usage::Window {
                        window: w.clone(),
                        at: r.at,
                    })
                })
                .chain(r.partial.iter().map(|w| {
                    row(Usage::Partial {
                        window: w.clone(),
                        at: r.at,
                    })
                }))
                .collect(),
        };
        if fresh(r.unreadable_at.unwrap_or(r.at), None, now) {
            lines.extend(
                r.unreadable
                    .iter()
                    .map(|reason| row(Usage::Unknown(format!("quota unreadable {reason}")))),
            );
        }
        lines
    }

    /// The `account-policy` heartbeat lines: `ids` lists the fingerprints of a runtime's listed homes.
    pub fn policy_lines(&self, now: i64, ids: impl Fn(&str) -> Vec<String>) -> Vec<String> {
        if let Some(reason) = self.local_reason(now) {
            return vec![format!("account-policy local {reason}")];
        }
        let mut lines = vec![];
        for (rt, p) in self.fleet(now).map(|f| &f.policy).into_iter().flatten() {
            let homes = ids(rt);
            if let Some(id) = &p.preferred {
                lines.push(format!(
                    "account-policy {} {rt} {id}",
                    if homes.contains(id) { "use" } else { "unmet" }
                ));
            }
            lines.extend(
                p.avoid
                    .iter()
                    .filter(|id| homes.contains(*id))
                    .map(|id| format!("account-policy avoid {rt} {id}")),
            );
        }
        lines
    }
}

// ---------- the poll ----------

/// One poll of this host's enabled accounts. `fetch` runs outside the account lock: a slow route never blocks a start.
pub fn poll_with(
    install: &Path,
    now: i64,
    mut fetch: impl FnMut(&str, &str) -> Poll,
) -> Result<(), String> {
    let policy = accounts::read_policy(install)?;
    let list = |rt: &str| {
        policy.get(rt).cloned().unwrap_or_else(|| {
            vec![accounts::Entry {
                label: accounts::DEFAULT.into(),
                enabled: true,
            }]
        })
    };
    let mut results = vec![];
    for rt in super::CLI_RUNTIMES
        .iter()
        .filter(|rt| accounts::applies(rt))
    {
        for e in list(rt).into_iter().filter(|e| e.enabled) {
            let poll = fetch(rt, &e.label);
            results.push((rt.to_string(), e.label, poll));
        }
    }
    let _lock = lock(install)?;
    let mut store = read_store(install);
    for (rt, label, poll) in &results {
        let old = store.get(rt).and_then(|a| a.get(label));
        if let Some(reason) = poll
            .unknown
            .as_ref()
            .filter(|r| old.is_none_or(|o| o.unknown.as_ref() != Some(r)))
        {
            crate::platform::log(&format!("account usage {rt} {label}: unknown {reason}"));
        }
        let id = poll.id.clone().or_else(|| old.and_then(|o| o.id.clone()));
        // Only a complete poll reaches `windows`: older binaries read that field and ignore every `unreadable` diagnostic.
        let (windows, partial) = if poll.unreadable.is_empty() {
            (poll.windows.clone(), vec![])
        } else {
            (vec![], poll.windows.clone())
        };
        let mut record = Record {
            id,
            at: now,
            windows,
            partial,
            unreadable: poll.unreadable.clone(),
            unreadable_at: (!poll.unreadable.is_empty()).then_some(now),
            unknown: poll.unknown.clone(),
        };
        if poll.unknown.is_some() && poll.unreadable.is_empty() {
            if let Some(old) = old.filter(|o| o.id == record.id && !o.unreadable.is_empty()) {
                record.unreadable = old.unreadable.clone();
                record.unreadable_at = Some(old.unreadable_at.unwrap_or(old.at));
            }
        }
        store
            .entry(rt.clone())
            .or_default()
            .insert(label.clone(), record);
    }
    // an account dropped from the list leaves no row; a disabled one keeps its id
    store.retain(|rt, a| {
        a.retain(|label, _| list(rt).iter().any(|e| e.label == *label));
        !a.is_empty()
    });
    crate::platform::write_synced(
        &install.join(FILE),
        &serde_json::to_string(&store).map_err(|e| e.to_string())?,
    )?;
    let observations: Vec<_> = results
        .iter()
        .map(|(rt, label, poll)| {
            let complete: &[Window] = if poll.unreadable.is_empty() {
                &poll.windows
            } else {
                &[]
            };
            (rt.as_str(), label.as_str(), complete)
        })
        .collect();
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
    let thread = std::thread::Builder::new()
        .name("account-usage".into())
        .spawn(move || {
            let mut ids = Ids::default();
            loop {
                if let Err(e) = poll_with(&install, accounts::now(), |rt, label| {
                    fetch(rt, label, &mut ids)
                }) {
                    crate::platform::log(&format!("account usage: {e}"));
                }
                std::thread::sleep(Duration::from_secs(EVERY as u64));
            }
        });
    if let Err(e) = thread {
        crate::platform::log(&format!("account usage: not started: {e}"));
    }
}

/// Identity fingerprints per runtime and label.
#[derive(Default)]
pub struct Ids(BTreeMap<(String, String), (i64, Option<String>)>);

impl Ids {
    fn get(&mut self, rt: &str, label: &str) -> Option<String> {
        let now = accounts::now();
        let key = (rt.to_string(), label.to_string());
        if let Some((_, id)) = self
            .0
            .get(&key)
            .filter(|(at, id)| id.is_some() && now - at < ID_REFRESH)
        {
            return id.clone();
        }
        let home = accounts::home(rt, label).or_else(|| accounts::base(rt));
        let id = super::agent(rt)
            .ok()
            .and_then(|a| a.account_identity(explicit(rt, home.as_deref())))
            .or_else(|| super::account_email(rt, label))
            .map(|identity| fingerprint(&identity));
        self.0.insert(key, (now, id.clone()));
        id
    }
}

/// One account's windows through AgentQ's read-only `account_limits`; its failure reasons stay as reported.
pub fn fetch(rt: &str, label: &str, ids: &mut Ids) -> Poll {
    let a = super::of(rt);
    if !super::which(a.bin).is_absolute() {
        return Poll::failed("not installed");
    }
    let home = accounts::home(rt, label).or_else(|| accounts::base(rt));
    // The `agy` login has no account home: one default account per host, read from the user's home.
    if !accounts::var(rt).is_empty() && !home.as_deref().is_some_and(Path::is_dir) {
        return Poll::failed("no login");
    }
    let Ok(agent) = super::agent(rt) else {
        return Poll::failed("no usage route");
    };
    let mut poll = poll_of(agent.account_limits(explicit(rt, home.as_deref()), accounts::now()));
    poll.id = ids.get(rt, label);
    poll
}

/// The home AgentQ must name: the default account keeps its native default, as its keychain item does.
fn explicit<'a>(rt: &str, home: Option<&'a Path>) -> Option<&'a Path> {
    home.and_then(|home| accounts::environment(rt, home).map(|(_, home)| home))
}

/// AgentQ windows as a poll: a window with a name and reading is readable, a named one without is unreadable, and a
/// nameless reason fails the whole poll when nothing was readable.
fn poll_of(windows: Vec<agentq_runtime::agent::Window>) -> Poll {
    let mut poll = Poll::default();
    let mut reasons = vec![];
    for w in super::headroom::unique_names(windows) {
        let reason = w.unknown.as_deref().unwrap_or("unreadable");
        match (w.window.as_deref().filter(|n| valid_window(n)), w.used) {
            (Some(name), Some(used)) if used.is_finite() => poll.windows.push(Window {
                name: name.to_string(),
                used: percent(used * 100.0),
                reset: w.resets_at.and_then(|r| i64::try_from(r).ok()),
            }),
            (Some(name), _) => poll.unreadable.push(format!("{name}: {reason}")),
            (None, _) => reasons.push(reason.to_string()),
        }
    }
    if poll.windows.is_empty() && poll.unreadable.is_empty() {
        poll.unknown = Some(
            reasons
                .first()
                .map_or("not-reported", String::as_str)
                .to_string(),
        );
    } else {
        poll.unreadable.extend(reasons);
    }
    poll
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
