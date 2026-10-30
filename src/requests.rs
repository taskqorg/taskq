//! Board request accounting and rate-limit pause (taskq.md § 4 Log and board budget): counts per operation of this
//! process and a pause, on the monotonic clock, while the board's shared request budget is exhausted.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Historical source-record prefix; live summaries use capture once per CLI exit or service hour.
const RECORD: &str = "taskq-requests ";
/// Requests of this process by operation, for its exit record.
static PROCESS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Usage { at_ms: u64, source: String, ops: BTreeMap<String, u64> }

/// Longest pause: GitHub and GitLab budgets reset within the hour.
const MAX_PAUSE: i64 = 3600;
/// Operations named in the hourly log line; the rest add up to `other`.
const TOP: usize = 8;
/// Suffix of an operation the board does not bill: a 304 reply, GitHub `GET /rate_limit` (#1985).
pub const UNBILLED: &str = " unbilled";

/// Whether `op` is a billed operation, not an [`UNBILLED`] one.
pub fn billed(op: &str) -> bool { !op.ends_with(UNBILLED) }

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

/// The cross-process cache of one API host and credential in `install`: its login and conditional replies (#1985).
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
    /// The first `remaining` seen per reset of the lowest bucket and when: the burn of every host sharing the board user (#1985).
    windows: BTreeMap<i64, (Instant, u64)>,
    /// The last reply's board time and when it came.
    seen: Option<(Instant, i64)>,
}

/// Remaining requests a paced service leaves to CLI, agents and the other hosts at the reset: 10 % of `limit`.
const RESERVE: u64 = 10;

/// The pass wait that spends `remaining - reserve` by the reset at `burn` requests/s of all hosts, from `wait` up to `cap`;
/// None while that burn reaches the reset (#1985).
pub fn paced(wait: u64, cap: u64, remaining: u64, limit: u64, to_reset: i64, burn: f64) -> Option<u64> {
    if to_reset <= 0 || burn <= 0.0 { return None; }
    let usable = remaining.saturating_sub(limit / RESERVE) as f64;
    let need = burn * to_reset as f64;
    if need <= usable { return None; }
    let cap = cap.max(wait);
    let w = if usable < 1.0 { cap } else { ((wait as f64 * need / usable).ceil() as u64).clamp(wait, cap) };
    (w > wait).then_some(w)
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

    /// Moves one counted `op` to its [`UNBILLED`] count: the reply was a 304 or a free read.
    pub fn unbill(&mut self, op: &str) {
        unbill(&mut self.ops, op);
    }

    /// Billed requests since the process started.
    pub fn total(&self) -> u64 {
        self.ops.iter().filter(|(k, _)| billed(k)).map(|(_, v)| v).sum()
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
                if let Some(t) = t {
                    self.windows.entry(t).or_insert((now, r));
                    while self.windows.len() > 4 { self.windows.pop_first(); }
                }
            }
        }
        if board_now > 0 { self.seen = Some((now, board_now)); }
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

    /// A pass wait longer than `wait` (at most `cap`) while the burn since this reset window opened, at least 60 s ago,
    /// would spend the reserve before the reset (#1985).
    pub fn pace(&self, wait: u64, cap: u64, now: Instant) -> Option<u64> {
        let (remaining, limit, reset) = (self.remaining?, self.limit?, self.reset?);
        let (at, first) = *self.windows.get(&reset)?;
        let (seen, board) = self.seen?;
        let elapsed = now.saturating_duration_since(at).as_secs_f64();
        if elapsed < 60.0 { return None; }
        let to_reset = reset - board - now.saturating_duration_since(seen).as_secs() as i64;
        paced(wait, cap, remaining, limit, to_reset, first.saturating_sub(remaining) as f64 / elapsed)
    }

    /// The budget alarm (§ 4): the last `remaining` is at most 20 % of `limit`.
    pub fn low(&self) -> bool {
        self.unknown.is_none() && matches!((self.remaining, self.limit), (Some(r), Some(l)) if l > 0 && r * 5 <= l)
    }

    /// The largest operation since the cumulative counts `start` (this process's current hour).
    pub fn top(&self, start: &BTreeMap<String, u64>) -> Option<(String, u64)> {
        diff(&self.ops, start).into_iter().filter(|(k, _)| billed(k)).max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
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

/// `<total><unit>; <op> <n>, ..` of billed requests with the largest operations first, the rest as `other`, then
/// `; unbilled <n>` for 304 replies and free reads (#1985).
pub fn summary(ops: &BTreeMap<String, u64>, unit: &str) -> String {
    let mut v: Vec<(&String, &u64)> = ops.iter().filter(|(k, _)| billed(k)).collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut parts: Vec<String> = v.iter().take(TOP).map(|(k, n)| format!("{k} {n}")).collect();
    let other: u64 = v.iter().skip(TOP).map(|x| *x.1).sum();
    if other > 0 { parts.push(format!("other {other}")); }
    let free: u64 = ops.iter().filter(|(k, _)| !billed(k)).map(|(_, v)| v).sum();
    let free = if free > 0 { format!("; unbilled {free}") } else { String::new() };
    format!("{}{unit}{}{}{free}", v.iter().map(|x| *x.1).sum::<u64>(), if parts.is_empty() { "" } else { "; " }, parts.join(", "))
}

fn unbill(ops: &mut BTreeMap<String, u64>, op: &str) {
    if !billed(op) { return; }
    if let Some(n) = ops.get_mut(op).filter(|n| **n > 0) {
        *n -= 1;
        if *n == 0 { ops.remove(op); }
    }
    *ops.entry(format!("{op}{UNBILLED}")).or_default() += 1;
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

pub fn totals() -> BTreeMap<String, u64> { PROCESS.lock().map(|p| p.clone()).unwrap_or_default() }

/// Counts one request of this process for its exit record.
pub fn tally(op: &str) {
    if let Ok(mut m) = PROCESS.lock() { *m.entry(op.into()).or_default() += 1; }
}

/// Moves one tallied `op` of this process to its [`UNBILLED`] count.
pub fn untally(op: &str) {
    if let Ok(mut m) = PROCESS.lock() { unbill(&mut m, op); }
}

/// The source of a CLI process: `<role> <command>` (`service` adds its subcommand), from the positionals as dispatch
/// parses them, so flags anywhere do not change it. Role `worker` or `reviewer` from `TASKQ_ROLE`, else `cli` (owner,
/// PM, operator). None for `service run`: the service records each hour.
pub fn source(argv: &[String], role: Option<&str>) -> Option<String> {
    let args = crate::Args::parse(argv).map(|a| a.pos).unwrap_or_default();
    let word = |s: Option<&String>| s.filter(|s| !s.is_empty() && s.len() <= 30 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'))
        .map_or("*".to_string(), Clone::clone);
    let mut cmd = word(args.first());
    if cmd == "service" {
        if args.get(1).is_some_and(|s| s == "run") { return None; }
        cmd = format!("service {}", word(args.get(1)));
    }
    let role = role.filter(|r| matches!(*r, "worker" | "reviewer")).unwrap_or("cli");
    Some(format!("{role} {cmd}"))
}

/// Submit one typed source observation of the billed requests; nothing when none was billed.
pub fn record(source: &str, ops: &BTreeMap<String, u64>) {
    let ops: BTreeMap<String, u64> = ops.iter().filter(|(k, _)| billed(k)).map(|(k, v)| (k.clone(), *v)).collect();
    if ops.is_empty() { return; }
    let at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64);
    let usage = Usage { at_ms, source: source.into(), ops };
    if let Err(e) = crate::telemetry::record(crate::telemetry::Sample::Requests(usage)) {
        use std::io::Write;
        let _=writeln!(std::io::stderr(),"Board request record: unknown ({})", crate::profile::append_error(&e));
    }
}

/// The exit record of this CLI process.
pub fn flush(source: &str) {
    let ops = PROCESS.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
    record(source, &ops);
}

/// Validate historical metadata with the same normalization used by the request writer.
pub(crate) fn legacy_value(line: &str) -> Result<Value,String> {
    let usage:Usage=serde_json::from_str(line.strip_prefix(RECORD).ok_or("unknown request record")?).map_err(|e|e.to_string())?;
    usage.validate()?;
    serde_json::to_value(usage).map_err(|e|e.to_string())
}

impl Usage {
pub(crate) fn validate(&self)->Result<(),String> {
    let usage=self;
    let words=usage.source.split(' ').collect::<Vec<_>>();
    let command=|word:&&str|*word=="*"||(!word.is_empty()&&word.len()<=30&&word.bytes().all(|b|b.is_ascii_lowercase()||b==b'-'));
    if usage.source!="service"&&!(matches!(words.first(),Some(&"cli"|&"worker"|&"reviewer"))
        &&((words.len()==2&&command(&words[1]))||(words.len()==3&&words[1]=="service"&&command(&words[2])))) {return Err("invalid request source".into());}
    for name in usage.ops.keys() {
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
        let u=match sample {
            crate::telemetry::Sample::Requests(r)=>r.clone(),
            crate::telemetry::Sample::Corrupt("requests")=>return "Board requests by source: unknown (corrupt request record; log kept)\n".into(),
            _=>continue,
        };
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
        a CLI process counts at its exit, a service each hour; billed requests only, 304 replies apart; agent `gh`, PM chat tools and DashQ not counted\n\n\
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
pub fn sources(text:&str,now_ms:u64)->String {sources_samples(&crate::telemetry::Sample::archive(text),now_ms)}

#[cfg(test)]
mod tests {
    use super::*;

    // Chain: #1084 A1 CLI process -> source record -> `taskq profile` hour table. Risk: a source is merged with
    // another, a request outside the hour is counted, a missing or corrupt record reads as zero, or free text
    // from arguments reaches the log.
    #[test]
    fn requests_by_source_for_the_last_hour() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(source(&args(&["result", "1084"]), Some("worker")).unwrap(), "worker result");
        assert_eq!(source(&args(&["status"]), Some("operator")).unwrap(), "cli status");
        assert_eq!(source(&args(&["service", "status"]), None).unwrap(), "cli service status");
        assert_eq!(source(&args(&["service", "run"]), None), None, "the service records its own hours");
        assert_eq!(source(&args(&["C:\\secret path"]), None).unwrap(), "cli *");
        assert_eq!(source(&[], None).unwrap(), "cli *");
        assert_eq!(source(&args(&["--json", "status"]), None).unwrap(), "cli status", "a flag before the command");
        assert_eq!(source(&args(&["--json", "service", "run"]), None), None, "service run with a flag first");
        assert_eq!(source(&args(&["service", "--json", "run"]), None), None, "service run with a flag between");
        let line = |at: u64, src: &str, ops: &[(&str, u64)]| format!("{RECORD}{}\n", serde_json::to_string(&Usage { at_ms: at,
            source: src.into(), ops: ops.iter().map(|(k, v)| (k.to_string(), *v)).collect() }).unwrap());
        let now = 10_000_000;
        assert!(sources("", now).contains("unknown (no request records"));
        let text = [line(now - 3_700_000, "cli status", &[("GET issues", 50)]), "taskq-profile {}\nother line\n".into(),
            line(now - 60_000, "worker result", &[("POST issues/N/comments", 1), ("GET issues/N/comments", 2)]),
            line(now - 50_000, "worker result", &[("GET issues/N/comments", 2)]),
            line(now - 40_000, "service", &[("POST graphql", 60), ("PATCH issues/N", 30)])].concat();
        let report = sources(&text, now);
        assert!(report.starts_with("Board requests by source: 95 in 6400000..10000000 Unix ms (full hour)"), "{report}");
        assert!(report.contains("| service | 1 | 90 | POST graphql 60, PATCH issues/N 30 |\n| worker result | 2 | 5 | GET issues/N/comments 4, POST issues/N/comments 1 |"), "{report}");
        assert!(!report.contains("cli status"), "a record before the hour is not counted");
        assert!(sources(&text[line(now - 3_700_000, "cli status", &[("GET issues", 50)]).len()..], now).contains("(partial: records start 9940000"));
        assert!(sources(&format!("{text}{RECORD}{{\"at_ms\":1}}\n"), now).starts_with("Board requests by source: unknown (corrupt"));
        let dir = std::env::temp_dir().join(format!("taskq-sources-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        std::fs::write(dir.join("taskq.log"), line(wall - 60_000, "worker result", &[("GET issues/N/comments", 5)]).repeat(2)).unwrap();
        drop(crate::profile::log_lock(&dir.join("taskq.log")).unwrap());
        crate::telemetry::read(&dir,wall).unwrap();
        assert!(crate::profile::report(&dir).contains("| worker result | 2 | 10 |"), "`taskq profile` prints the table");
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: § 4 board budget. Risk: an exhausted budget keeps sending requests, or a key leaks a branch or SHA.
    #[test]
    fn exhausted_budget_pauses_until_reset_and_resumes() {
        let t0 = Instant::now();
        let mut b = Budget::default();
        let h = |pairs: &'static [(&'static str, &'static str)]| move |n: &str| pairs.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string());
        assert_eq!(b.observe(200, false, h(&[("x-ratelimit-remaining", "12"), ("x-ratelimit-limit", "5000")]), 1000, t0), None);
        assert_eq!((b.remaining, b.limit, b.paused(t0)), (Some(12), Some(5000), None));
        // a 403 without budget headers is an ordinary refusal
        assert_eq!(b.observe(403, false, h(&[]), 1000, t0), None);
        // primary limit: wait for the reset header against board time, not the local clock
        assert_eq!(b.observe(403, false, h(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1840")]), 1000, t0), Some(1840));
        assert_eq!(b.paused(t0 + Duration::from_secs(839)), Some(1840));
        assert_eq!(b.paused(t0 + Duration::from_secs(840)), None, "resumes at the reset without a restart");
        // secondary limit: retry-after wins; a far reset is capped at one hour
        assert_eq!(b.observe(429, false, h(&[("retry-after", "30"), ("x-ratelimit-remaining", "0")]), 1000, t0), Some(1030));
        assert_eq!(b.observe(403, false, h(&[("ratelimit-remaining", "0"), ("ratelimit-reset", "999999")]), 1000, t0), Some(4600));
        assert_eq!(op("PATCH", "/issues/77"), "PATCH issues/N");
        assert_eq!(op("GET", "/issues/5/comments?per_page=100"), "GET issues/N/comments");
        assert_eq!(op("GET", "/git/ref/heads/taskq-1009"), "GET git/ref/heads");
        assert_eq!(op("GET", &format!("/commits/{}/check-runs", "a1".repeat(20))), "GET commits/*/check-runs");
        let mut later = BTreeMap::from([("GET issues".to_string(), 7), ("PATCH issues/N".to_string(), 60)]);
        let hour = diff(&later, &BTreeMap::from([("GET issues".to_string(), 7)]));
        assert_eq!(summary(&hour, ""), "60; PATCH issues/N 60");
        later.extend((0..9).map(|i| (format!("GET x{i}"), 1)));
        assert!(summary(&later, "").ends_with(", GET x5 1, other 3"));
    }

    // Chain: § 4 board budget to `service status` and R6. Risk: missing accounting reads as zero requests.
    #[test]
    fn request_lines_keep_unknown_and_name_the_pause() {
        assert_eq!(lines(&json!({})), ["Board requests: unknown (no request accounting in status)"]);
        let s = json!({"host": "mac", "requests": {"total": 3, "last_hour": null, "paused_until": "2026-10-14T04:41:00Z"}});
        assert_eq!(lines(&s), ["Board requests: unknown (no full hour yet)", "Rate limit: board requests paused until 2026-10-14T04:41:00Z (mac)"]);
        let s = json!({"requests": {"last_hour": {"PATCH issues/N": 60}, "paused_until": null}});
        assert_eq!(lines(&s), ["Board requests: 60/h; PATCH issues/N 60"]);
    }

    // Chain: § 4 budget alarm to `service status` and R6. Risk: a draining budget stays silent, or names no consumer.
    #[test]
    fn low_budget_alarm_names_the_top_consumer() {
        let h = |r: &'static str| move |n: &str| match n { "x-ratelimit-remaining" => Some(r.to_string()), "x-ratelimit-limit" => Some("5000".into()),
            "x-ratelimit-reset" => Some("1791964800".into()), _ => None };
        let mut b = Budget::default();
        b.observe(200, false, h("1001"), 0, Instant::now());
        assert!(!b.low(), "above 20 % is quiet");
        b.observe(200, false, h("1000"), 0, Instant::now());
        assert!(b.low());
        let start = BTreeMap::from([("PATCH issues/N".to_string(), 50)]);
        b.ops = BTreeMap::from([("PATCH issues/N".to_string(), 80), ("GET issues".to_string(), 20)]);
        assert_eq!(b.top(&start), Some(("PATCH issues/N".into(), 30)));
        let s = json!({"host": "mac", "requests": b.json(0, None, &start)});
        assert_eq!(warning(&s).unwrap(), "Board budget low: 1000/5000 until 2026-10-14T08:00:00Z (mac); top here: PATCH issues/N 30");
    }

    // Chain: § 4 per-resource budget. Risk: a drained GraphQL bucket hides behind a full REST one, or the reverse.
    #[test]
    fn budget_follows_the_lowest_resource() {
        let h = |res: &'static str, r: &'static str| move |n: &str| match n { "x-ratelimit-resource" => Some(res.to_string()),
            "x-ratelimit-remaining" => Some(r.to_string()), "x-ratelimit-limit" => Some("5000".into()), _ => None };
        let mut b = Budget::default();
        b.observe(200, false, h("core", "900"), 0, Instant::now());
        b.observe(200, false, h("graphql", "4000"), 0, Instant::now());
        assert_eq!((b.resource.as_deref(), b.remaining), (Some("core"), Some(900)), "a full bucket never hides a low one");
        b.observe(200, false, h("graphql", "500"), 0, Instant::now());
        b.observe(200, false, h("core", "4990"), 0, Instant::now());
        assert_eq!((b.resource.as_deref(), b.remaining, b.low()), (Some("graphql"), Some(500), true));
        let s = json!({"host": "mac", "requests": b.json(0, None, &BTreeMap::new())});
        assert_eq!(warning(&s).unwrap(), "Board budget low: 500/5000 graphql until unknown (mac); top here: none");
    }

    // Chain: #1985 304 replies apart. Risk: free conditional replies inflate the billed total, the hourly line and the
    // profile records, so the count no longer matches what the board user spends.
    #[test]
    fn unbilled_requests_count_apart() {
        let mut b = Budget::default();
        for _ in 0..3 { b.count("GET issues/N".into()); }
        b.unbill("GET issues/N");
        b.unbill("GET issues/N unbilled");
        assert_eq!((b.total(), b.ops.get("GET issues/N"), b.ops.get("GET issues/N unbilled")), (2, Some(&2), Some(&1)), "an unbilled op is never moved twice");
        assert_eq!(summary(&b.ops, "/h"), "2/h; GET issues/N 2; unbilled 1");
        assert_eq!(b.top(&BTreeMap::new()), Some(("GET issues/N".to_string(), 2)));
    }

    // Chain: #1985 A3 four hosts share one quota. Risk: the service keeps its pass rate until the shared remaining hits 0
    // and every board pauses, or it slows while the quota lasts to the reset.
    #[test]
    fn passes_slow_before_the_shared_quota_runs_out() {
        assert_eq!(paced(60, 200, 3000, 5000, 1800, 1.0), None, "1800 more requests fit in 2500 usable");
        assert_eq!(paced(60, 200, 3000, 5000, 1800, 2.0), Some(87), "3600 needed for 2500 usable stretches the wait");
        assert_eq!(paced(60, 200, 600, 5000, 1800, 2.0), Some(200), "inside the reserve the wait is the cap");
        assert_eq!(paced(60, 200, 3000, 5000, 0, 9.0), None, "a reset now needs no pacing");
        assert_eq!(paced(300, 200, 600, 5000, 1800, 2.0), None, "the cap never shortens a longer wait");
        let h = |r: &'static str| move |n: &str| match n { "x-ratelimit-resource" => Some("core".into()), "x-ratelimit-remaining" => Some(r.to_string()),
            "x-ratelimit-limit" => Some("5000".into()), "x-ratelimit-reset" => Some("4600".into()), _ => None };
        let start = Instant::now();
        let mut b = Budget::default();
        b.observe(200, false, h("4000"), 1000, start);
        assert_eq!(b.pace(60, 200, start + Duration::from_secs(30)), None, "under a minute of samples gives no burn");
        // 2000 spent by all hosts in 100 s: 20/s for 3500 s left of 1500 usable
        b.observe(200, false, h("2000"), 1100, start + Duration::from_secs(100));
        assert_eq!(b.pace(60, 200, start + Duration::from_secs(100)), Some(200));
        // a new reset window starts its own burn
        let fresh = |n: &str| match n { "x-ratelimit-reset" => Some("8200".into()), "x-ratelimit-remaining" => Some("5000".into()),
            "x-ratelimit-resource" => Some("core".into()), "x-ratelimit-limit" => Some("5000".into()), _ => None };
        b.observe(200, false, fresh, 4700, start + Duration::from_secs(3800));
        assert_eq!(b.pace(60, 200, start + Duration::from_secs(3900)), None);
    }
}
