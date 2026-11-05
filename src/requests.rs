//! Board request accounting and rate-limit pause (taskq.md § 4 Log and board budget): counts per operation of this
//! process and a pause, on the monotonic clock, while the board's shared request budget is exhausted.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Historical source-record prefix; live summaries use capture at each CLI exit and each service record.
/// The source of the service's records.
pub const SERVICE: &str = "service";
/// §4 Board request records: the 5-minute loss bound less a margin for host ticks and the store's commit.
const RECORD_SECONDS: u64 = 240;
const _: () = assert!(RECORD_SECONDS + 60 <= 300, "a killed service loses at most its last 5 minutes");
/// How often the running service records; short under test so a fixture host reaches it.
#[cfg(not(test))] pub const RECORD_EVERY: Duration = Duration::from_secs(RECORD_SECONDS);
#[cfg(test)] pub const RECORD_EVERY: Duration = Duration::from_secs(1);
/// Requests of this process by operation since it started, for the hourly log and heartbeat.
static PROCESS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
/// Requests of this process by operation not yet recorded to telemetry.
static UNRECORDED: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
/// Billed requests of this process by API host (#2084): a host with GitHub and GitLab boards splits its rate.
static HOSTS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
/// The one operation GitHub never bills: the budget read itself.
const FREE: &str = "GET rate_limit";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Usage { at_ms: u64, source: String, ops: BTreeMap<String, u64> }

/// Longest pause: GitHub and GitLab budgets reset within the hour.
const MAX_PAUSE: i64 = 3600;
/// The longest wait before one request while the budget is low (§ 4 pacing).
const MAX_PACE: f64 = 5.0;
/// Operations named in the hourly log line; the rest add up to `other`.
const TOP: usize = 8;

/// One counter/pause per API host and credential, shared by all project threads; never store the token as a key.
pub struct Shared(std::sync::Arc<Mutex<Budget>>);
impl Default for Shared {
    fn default() -> Self { Self(std::sync::Arc::new(Mutex::new(Budget::default()))) }
}
/// The key of one API host and credential: a SHA-256, never the token.
pub fn key(root: &str, auth: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, format!("{root}\0{auth}").as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// The pause file of one API host and credential in `install`: the reset (Unix seconds) every process there honours.
pub fn pause_file(install: &std::path::Path, root: &str, auth: &str) -> std::path::PathBuf {
    install.join(format!("requests-paused-{}", &key(root, auth)[..16]))
}

/// The board login of one API host and credential in `install`: every process reuses it instead of `GET /user`.
pub fn user_file(install: &std::path::Path, root: &str, auth: &str) -> std::path::PathBuf {
    install.join(format!("board-user-{}", &key(root, auth)[..16]))
}

/// The conditional replies of one API host and credential in `install` that every process shares (#1985).
pub fn cache_dir(install: &std::path::Path, root: &str, auth: &str) -> std::path::PathBuf {
    install.join(format!("board-cache-{}", &key(root, auth)[..16]))
}

/// A pause another process recorded in `file`: its reset while in the future, at most [`MAX_PAUSE`] ahead.
pub fn recorded_pause(file: &std::path::Path, wall: i64) -> Option<(i64, i64)> {
    let reset = std::fs::read_to_string(file).ok()?.trim().parse::<i64>().ok()?;
    (reset > wall).then(|| (reset, (reset - wall).min(MAX_PAUSE)))
}

impl Shared {
    pub fn for_api(root: &str, auth: &str) -> Self {
        if !crate::host::active() { return Self::default(); }
        static BUDGETS: std::sync::OnceLock<Mutex<BTreeMap<String, std::sync::Arc<Mutex<Budget>>>>> = std::sync::OnceLock::new();
        Self(BUDGETS.get_or_init(Default::default).lock().expect("board budget registry poisoned").entry(key(root, auth)).or_default().clone())
    }
    pub fn borrow(&self) -> std::sync::MutexGuard<'_, Budget> {
        match self.0.lock() {
            Ok(b) => b,
            Err(e) => { let mut b = e.into_inner(); b.unknown = Some("board budget state poisoned".into()); b }
        }
    }
    pub fn borrow_mut(&self) -> std::sync::MutexGuard<'_, Budget> { self.borrow() }
}

#[derive(Clone, Debug, Default)]
pub struct Budget {
    pub unknown: Option<String>,
    /// Requests sent since the process started, by operation.
    pub ops: BTreeMap<String, u64>,
    /// Last `remaining`/`limit`/reset (board seconds) per budget (`x-ratelimit-resource`: GitHub `core`, `graphql`).
    buckets: BTreeMap<String, (u64, u64, Option<i64>)>,
    /// The budget with the smallest remaining share: its name, `remaining`, `limit` and reset.
    pub resource: Option<String>,
    pub remaining: Option<u64>,
    pub limit: Option<u64>,
    pub reset: Option<i64>,
    /// Paused until this instant; the reset in board seconds.
    paused: Option<(Instant, i64)>,
}

/// `<METHOD> <path>` of `path` (relative to the board API): numbers become `N`, other ids `*`, at most 3 segments.
/// No branch, SHA or text reaches a key.
pub fn op(method: &str, path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segs: Vec<String> = path.split('/').filter(|s| !s.is_empty()).take(3).map(|s| {
        if s.bytes().all(|b| b.is_ascii_digit()) { "N".into() }
        else if s.len() <= 30 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'_') { s.into() }
        else { "*".into() }
    }).collect();
    format!("{method} {}", segs.join("/"))
}

impl Budget {
    pub fn count(&mut self, op: String) {
        *self.ops.entry(op).or_default() += 1;
    }

    /// Moves one count of `op` to `<op> 304`: a 304 to a conditional GET, which GitHub does not bill (#1985).
    pub fn unbilled(&mut self, op: &str) {
        unbill(&mut self.ops, op);
    }

    pub fn total(&self) -> u64 {
        self.ops.values().sum()
    }

    /// The reset (board seconds) while paused at `now`; a pause that ran out is cleared.
    pub fn paused(&mut self, now: Instant) -> Option<i64> {
        match self.paused {
            Some((until, reset)) if now < until => Some(reset),
            _ => { self.paused = None; None }
        }
    }

    /// Pauses for `wait` seconds until `reset`, as another process of this install recorded (§ 4).
    pub fn hold(&mut self, reset: i64, wait: i64, now: Instant) {
        if self.paused(now).is_none() { self.paused = Some((now + Duration::from_secs(wait.clamp(1, MAX_PAUSE) as u64), reset)); }
    }

    pub fn paused_until(&self) -> Option<i64> {
        self.paused.filter(|(until, _)| Instant::now() < *until).map(|p| p.1)
    }

    /// Reads the budget headers of one reply. A 403 or 429 with `retry-after`, or with no requests remaining, pauses
    /// until the reset: `retry-after`, else the reset header against the reply's board time. Returns the reset when
    /// this reply started a pause. A `limited` reply (the board's own rate-limit error) pauses without those headers too.
    pub fn observe(&mut self, status: u16, limited: bool, header: impl Fn(&str) -> Option<String>, board_now: i64, now: Instant) -> Option<i64> {
        let num = |names: &[&str]| names.iter().find_map(|n| header(n)?.trim().parse::<i64>().ok());
        let remaining = num(&["x-ratelimit-remaining", "ratelimit-remaining"]);
        let limit = num(&["x-ratelimit-limit", "ratelimit-limit"]);
        if let Some(r) = remaining.and_then(|r| u64::try_from(r).ok()) {
            let name = header("x-ratelimit-resource").unwrap_or_default();
            let l = limit.and_then(|l| u64::try_from(l).ok()).or(self.buckets.get(&name).map(|b| b.1)).unwrap_or(0);
            self.buckets.insert(name, (r, l, num(&["x-ratelimit-reset", "ratelimit-reset"])));
            let low = self.buckets.iter().min_by_key(|(_, (r, l, _))| r * 10_000 / (*l).max(1)).map(|(k, v)| (k.clone(), *v));
            if let Some((name, (r, l, t))) = low {
                (self.resource, self.remaining, self.limit, self.reset) = ((!name.is_empty()).then_some(name), Some(r), (l > 0).then_some(l), t);
            }
        }
        if !matches!(status, 403 | 429) { return None; }
        let wait = match (num(&["retry-after"]), remaining) {
            (Some(s), _) => s,
            (None, r) if limited || r == Some(0) => num(&["x-ratelimit-reset", "ratelimit-reset"]).map_or(60, |reset| reset - board_now),
            _ => return None,
        }.clamp(1, MAX_PAUSE);
        let reset = board_now + wait;
        self.paused = Some((now + Duration::from_secs(wait as u64), reset));
        Some(reset)
    }

    /// The wait before the next request while the budget is low: the time to the reset spread over the requests left,
    /// at most [`MAX_PACE`]. `remaining` is per board user, so every host slows together before it reaches 0 (§ 4).
    pub fn pace(&self, board_now: i64) -> Option<Duration> {
        let (Some(r), Some(reset)) = (self.remaining, self.reset) else { return None };
        (self.low() && board_now > 0 && reset > board_now).then(|| Duration::from_secs_f64(((reset - board_now) as f64 / r.max(1) as f64).min(MAX_PACE)))
    }

    /// The budget alarm (§ 4): the last `remaining` is at most 20 % of `limit`.
    pub fn low(&self) -> bool {
        self.unknown.is_none() && matches!((self.remaining, self.limit), (Some(r), Some(l)) if l > 0 && r * 5 <= l)
    }

    /// The largest operation since the cumulative counts `start` (this process's current hour).
    pub fn top(&self, start: &BTreeMap<String, u64>) -> Option<(String, u64)> {
        diff(&self.ops, start).into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
    }

    /// `status.json` `requests` (§ 4): `hour` the counts of the last full hour, `start` the cumulative counts at its end.
    pub fn json(&self, since: u64, hour: Option<&BTreeMap<String, u64>>, start: &BTreeMap<String, u64>) -> Value {
        if let Some(reason) = &self.unknown { return json!({"reason": reason, "since": since, "total": null, "ops": null, "last_hour": null}); }
        json!({"since": since, "total": self.total(), "ops": self.ops, "last_hour": hour, "remaining": self.remaining, "limit": self.limit, "resource": self.resource,
            "reset": self.reset.map(crate::board::stamp), "top": self.top(start).map(|(op, n)| format!("{op} {n}")),
            "paused_until": self.paused_until().map(crate::board::stamp)})
    }
}

/// The refusal of a paused request: names the reset in UTC.
pub fn paused_text(reset: i64) -> String {
    format!("rate limit: board requests paused until {}", crate::board::stamp(reset))
}

/// Requests per operation between two cumulative counts.
pub fn diff(now: &BTreeMap<String, u64>, before: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    now.iter().map(|(k, v)| (k.clone(), v.saturating_sub(before.get(k).copied().unwrap_or(0)))).filter(|(_, v)| *v > 0).collect()
}

/// `<total><unit>; <op> <n>, ..` with the largest operations first, the rest as `other`.
pub fn summary(ops: &BTreeMap<String, u64>, unit: &str) -> String {
    let mut v: Vec<(&String, &u64)> = ops.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut parts: Vec<String> = v.iter().take(TOP).map(|(k, n)| format!("{k} {n}")).collect();
    let other: u64 = v.iter().skip(TOP).map(|x| *x.1).sum();
    if other > 0 { parts.push(format!("other {other}")); }
    format!("{}{unit}{}{}", ops.values().sum::<u64>(), if parts.is_empty() { "" } else { "; " }, parts.join(", "))
}

/// Owner lines of a `status.json` `requests` (§ 4, § 5 `service status`): the last full hour, else unknown, and a pause.
pub fn lines(status: &Value) -> Vec<String> {
    let r = &status["requests"];
    if let Some(reason) = r["reason"].as_str() { return vec![format!("Board requests: unknown ({reason})")]; }
    let mut out = vec![match r["last_hour"].as_object() {
        Some(h) => format!("Board requests: {}", summary(&h.iter().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect(), "/h")),
        None if r.is_object() => "Board requests: unknown (no full hour yet)".into(),
        None => "Board requests: unknown (no request accounting in status)".into(),
    }];
    out.extend(warning(status));
    out
}

/// `Rate limit: paused until <time> (<host>)` while the service reports a pause, else the low-budget alarm (R6, § 4).
pub fn warning(status: &Value) -> Option<String> {
    let r = &status["requests"];
    let host = status["host"].as_str().unwrap_or("this host");
    if let Some(until) = r["paused_until"].as_str() {
        return Some(format!("Rate limit: board requests paused until {until} ({host})"));
    }
    let (remaining, limit) = (r["remaining"].as_u64()?, r["limit"].as_u64()?);
    let resource = r["resource"].as_str().map(|s| format!(" {s}")).unwrap_or_default();
    (limit > 0 && remaining * 5 <= limit).then(|| format!("Board budget low: {remaining}/{limit}{resource} until {} ({host}); top here: {}",
        r["reset"].as_str().unwrap_or("unknown"), r["top"].as_str().unwrap_or("none")))
}

fn unbill(ops: &mut BTreeMap<String, u64>, op: &str) {
    if let Some(n) = ops.get_mut(op).filter(|n| **n > 0) { *n -= 1; *ops.entry(format!("{op} 304")).or_default() += 1; }
}

/// Whether the board bills `op`: not a `304` (#1985), not `GET rate_limit` (#2084).
fn bills(op: &str) -> bool { !op.ends_with(" 304") && op != FREE }

/// Billed requests of `ops` (#1985, #2084).
pub fn billed(ops: &BTreeMap<String, u64>) -> u64 {
    ops.iter().filter(|(k, _)| bills(k)).map(|(_, n)| n).sum()
}

/// `Board quota: ..` of this process's own reads (#1985): the `core` budget, else the one unnamed budget (GitLab),
/// never `graphql` (#2084).
pub fn quota(b: &Budget) -> String {
    match b.buckets.get("core").map(|v| ("core", v)).or_else(|| b.buckets.get("").map(|v| ("", v))) {
        Some((name, (r, l, reset))) if *l > 0 => format!("Board quota: {}/{l}{} used, {r} left until {}", l.saturating_sub(*r),
            if name.is_empty() { String::new() } else { format!(" {name}") }, reset.map_or("unknown".into(), crate::board::stamp)),
        _ => "Board quota: unknown (no core budget headers)".into(),
    }
}

impl Budget {
    /// Whether a reply carried the `core` or the unnamed budget, the one [`quota`] shows.
    pub fn has_core(&self) -> bool { self.buckets.contains_key("core") || self.buckets.contains_key("") }
}

pub fn totals() -> BTreeMap<String, u64> { PROCESS.lock().map(|p| p.clone()).unwrap_or_default() }

/// Billed requests of this process by API host (#2084).
pub fn host_totals() -> BTreeMap<String, u64> { HOSTS.lock().map(|p| p.clone()).unwrap_or_default() }

/// The API host of a board API `root`, as the heartbeat names it: `api.github.com`, `gitlab.com`; never a path.
pub fn api_host(root: &str) -> String {
    let authority = root.split_once("://").map_or(root, |x| x.1).split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default().split(':').next().unwrap_or_default().to_ascii_lowercase();
    if !host.is_empty() && host.len() <= 100 && host.bytes().all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b)) { host } else { "*".into() }
}

/// Counts one request of this process to API host `host` for its next record.
pub fn tally(op: &str, host: &str) {
    if let Ok(mut m) = PROCESS.lock() { *m.entry(op.into()).or_default() += 1; }
    if let Ok(mut m) = UNRECORDED.lock() { *m.entry(op.into()).or_default() += 1; }
    if bills(op) && let Ok(mut m) = HOSTS.lock() { *m.entry(host.into()).or_default() += 1; }
}

/// Moves one request of this process to `<op> 304` (#1985): unbilled, so `host` loses it.
pub fn untally(op: &str, host: &str) {
    if let Ok(mut m) = PROCESS.lock() { unbill(&mut m, op); }
    // ponytail: a 304 answered after its request was recorded stays billed in that record; the request is still counted once.
    if let Ok(mut m) = UNRECORDED.lock() { unbill(&mut m, op); }
    if bills(op) && let Ok(mut m) = HOSTS.lock() && let Some(n) = m.get_mut(host) { *n = n.saturating_sub(1); }
}

/// Heartbeat lines of the last full hour (#1985, #2084): `requests <n>/h` over all API hosts, the line older readers
/// know, then `requests <api host> <n>/h` per API host.
pub fn heartbeat(hour: &BTreeMap<String, u64>, hosts: Option<&BTreeMap<String, u64>>) -> Vec<String> {
    std::iter::once(format!("requests {}/h", billed(hour)))
        .chain(hosts.into_iter().flatten().map(|(h, n)| format!("requests {h} {n}/h"))).collect()
}

/// The source of a CLI process: `<role> <command>` (`service` adds its subcommand), from the positionals as dispatch
/// parses them, so flags anywhere do not change it. Role `worker` or `reviewer` from `TASKQ_ROLE`, else `cli` (owner,
/// PM, operator). `service run` is the service itself, whichever way its process ends.
pub fn source(argv: &[String], role: Option<&str>) -> String {
    let args = crate::Args::parse(argv).map(|a| a.pos).unwrap_or_default();
    let word = |s: Option<&String>| s.filter(|s| !s.is_empty() && s.len() <= 30 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'))
        .map_or("*".to_string(), Clone::clone);
    let mut cmd = word(args.first());
    if cmd == "service" {
        if args.get(1).is_some_and(|s| s == "run") { return SERVICE.into(); }
        cmd = format!("service {}", word(args.get(1)));
    }
    let role = role.filter(|r| matches!(*r, "worker" | "reviewer")).unwrap_or("cli");
    format!("{role} {cmd}")
}

/// Submit one typed source observation; nothing when no request was sent. False when the collector did not admit it.
pub(crate) fn record(source: &str, ops: &BTreeMap<String, u64>) -> bool {
    if ops.is_empty() { return true; }
    let at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64);
    let usage = Usage { at_ms, source: source.into(), ops: ops.clone() };
    let submitted = crate::telemetry::record(crate::telemetry::Sample::Requests(usage));
    if let Err(e) = &submitted {
        use std::io::Write;
        let _=writeln!(std::io::stderr(),"Board request record: unknown ({})", crate::profile::append_error(e));
    }
    submitted.is_ok()
}

/// Records the counts not yet recorded, at a CLI exit or on the service's cadence and clean ends. The lock is held
/// across the submit: a concurrent flush cannot record the same counts, and a refused submit keeps them for the next.
pub fn flush(source: &str) {
    let Ok(mut unrecorded) = UNRECORDED.lock() else { return };
    if record(source, &unrecorded) { unrecorded.clear(); }
}

impl Usage {
pub(crate) fn validate(&self)->Result<(),String> {
    let usage=self;
    let words=usage.source.split(' ').collect::<Vec<_>>();
    let command=|word:&&str|*word=="*"||(!word.is_empty()&&word.len()<=30&&word.bytes().all(|b|b.is_ascii_lowercase()||b==b'-'));
    if usage.source!="service"&&!(matches!(words.first(),Some(&"cli"|&"worker"|&"reviewer"))
        &&((words.len()==2&&command(&words[1]))||(words.len()==3&&words[1]=="service"&&command(&words[2])))) {return Err("invalid request source".into());}
    for name in usage.ops.keys() {
        // #1985: an unbilled 304 counts as `<op> 304`
        let name=name.strip_suffix(" 304").unwrap_or(name);
        let (method,path)=name.split_once(' ').ok_or("invalid request operation")?;
        if !matches!(method,"GET"|"POST"|"PUT"|"PATCH"|"DELETE"|"HEAD"|"OPTIONS")||op(method,&path.split('/').map(|part|if part=="N" {"0"} else {part}).collect::<Vec<_>>().join("/"))!=*name {return Err("invalid request operation".into());}
    }
    Ok(())
}
}

/// `taskq profile` section: board requests of the hour before `now_ms` by source, from the retained records.
pub(crate) fn sources_samples(samples:&[crate::telemetry::Sample], now_ms: u64) -> String {
    let start = now_ms.saturating_sub(3_600_000);
    let (mut rows, mut first) = (BTreeMap::<String, (u64, BTreeMap<String, u64>)>::new(), None::<u64>);
    for sample in samples {
        let crate::telemetry::Sample::Requests(u)=sample else {continue};let u=u.clone();
        first = Some(first.map_or(u.at_ms, |f| f.min(u.at_ms)));
        if u.at_ms <= start || u.at_ms > now_ms { continue; }
        let row = rows.entry(u.source).or_default();
        row.0 += 1;
        for (op, n) in u.ops { *row.1.entry(op).or_default() += n; }
    }
    if rows.is_empty() {
        return "Board requests by source: unknown (no request records in the last hour)\n".into();
    }
    let coverage = match first {
        Some(f) if f <= start => "full hour".to_string(),
        _ => format!("partial: records start {} Unix ms; earlier requests unknown", first.unwrap_or(start)),
    };
    let mut rows: Vec<_> = rows.into_iter().map(|(s, (n, ops))| (s, n, ops.values().sum::<u64>(), ops)).collect();
    rows.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let total: u64 = rows.iter().map(|r| r.2).sum();
    let mut out = format!("Board requests by source: {total} in {start}..{now_ms} Unix ms ({coverage}); this host only; \
        a CLI process counts at its exit, a service every 4 min and at a clean end, so a killed service loses at most its last 5 min; agent `gh`, PM chat tools and DashQ not counted\n\n\
        | Source | Records | Requests | Top operations |\n|---|---:|---:|---|\n");
    for (source, n, sum, ops) in rows {
        let mut ops: Vec<_> = ops.into_iter().collect();
        ops.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let top = ops.iter().take(3).map(|(op, n)| format!("{op} {n}")).collect::<Vec<_>>().join(", ");
        out += &format!("| {source} | {n} | {sum} | {top} |\n");
    }
    out
}

#[cfg(test)]
#[path = "requests_tests.rs"]
pub(crate) mod tests;
