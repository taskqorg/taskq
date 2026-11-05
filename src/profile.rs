//! §5 profiling: fixed numeric observations in bounded local SQLite, never operation input/error text.
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering}};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use crate::telemetry::Sample;

pub const OPS: [&str; 13] = ["service.pass", "dispatch", "spawn", "forge.call", "update", "tray.render", "service.tidy", "service.churn", "service.progress.overdue", "service.progress.recovered", "service.doctor", "account.switch", "conversations.source"];
#[cfg(test)] const PREFIX: &str = "taskq-profile ";
pub(crate) const ERRORS: [&str; 9] = ["none", "http4xx", "http5xx", "timeout", "permission", "invalid", "other", "panic", "argument-too-long"];
static LOG: OnceLock<PathBuf> = OnceLock::new();
static TRACE: OnceLock<bool> = OnceLock::new();
static RUN: OnceLock<Option<u64>> = OnceLock::new();
thread_local!(static PROJECT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) });
thread_local!(static HOST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) });
static SPAN: AtomicU64 = AtomicU64::new(1);
thread_local!(static PARENT: Cell<u64> = const { Cell::new(0) });

#[cfg(test)]
thread_local!(static OBSERVED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) });
#[cfg(test)]
thread_local!(pub(crate) static STAGED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) });
#[cfg(test)]
thread_local!(static MOVED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) });
/// Transition records of this thread and their report section.
#[cfg(test)]
pub fn take_moves() -> (String, String) {
    let text = MOVED.with(|v| std::mem::take(&mut *v.borrow_mut()));
    let report = render_system(&text).1;
    (text, report)
}
#[cfg(test)]
pub fn take_report() -> String {
    OBSERVED.with(|v| render(&std::mem::take(&mut *v.borrow_mut())).unwrap())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    op: String,
    us: u64,
    error: String,
    at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<Trace>,
    /// Per-pass counts of an operation that reports them (`service.tidy`); never paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    counts: Option<BTreeMap<String, u64>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Trace { run: Option<u64>, process: u32, span: u64, parent: u64 }

impl Record {
    pub(crate) fn valid(&self)->bool {OPS.contains(&self.op.as_str())&&ERRORS.contains(&self.error.as_str())}
}

pub fn init(install: &Path) {
    let _ = LOG.set(install.join("taskq.log"));
    let _ = TRACE.set(std::env::var_os("TASKQ_PROFILE_TRACE").is_some_and(|v| v == "1"));
}

/// This process's board `owner/name` for stage and transition records (#1174); an unsafe name is kept as `other`.
pub fn project(repo: &str) {
    let ok = repo.len() <= 128 && repo.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b));
    PROJECT.set(Some(if ok { repo } else { "other" }.into()));
}

pub fn host(name: &str) {
    HOST.set(Some(name.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).take(128).collect()));
}

// Compatibility String errors have no structured cause; unclassified text stays other.
fn error(text: &str) -> &'static str {
    if text.contains("ENAMETOOLONG") { "argument-too-long" }
    else if text.contains("HTTP 4") { "http4xx" }
    else if text.contains("HTTP 5") { "http5xx" }
    else if text.contains("timed out") || text.contains("timeout") { "timeout" }
    else if text.contains("denied") || text.contains("refused") { "permission" }
    else if text.contains("invalid") || text.contains("bad JSON") { "invalid" }
    else { "other" }
}

pub fn measure<T, E: std::fmt::Display>(op: &'static str, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    measure_at(LOG.get().map(PathBuf::as_path), TRACE.get().copied().unwrap_or(false), op, f)
}

pub(crate) fn measure_classified<T,E>(op:&'static str,f:impl FnOnce()->Result<T,E>,class:impl FnOnce(&E)->&'static str)->Result<T,E> {
    observe(LOG.get().map(PathBuf::as_path),TRACE.get().copied().unwrap_or(false),op,f,class)
}

pub(crate) fn io_class(error:&std::io::Error)->&'static str {
    #[cfg(unix)]
    if matches!(error.raw_os_error(),Some(libc::ENAMETOOLONG)|Some(libc::E2BIG)) {return "argument-too-long";}
    #[cfg(windows)]
    if error.raw_os_error()==Some(206) {return "argument-too-long";}
    match error.kind() {
        std::io::ErrorKind::PermissionDenied=>"permission",
        std::io::ErrorKind::TimedOut=>"timeout",
        std::io::ErrorKind::InvalidInput|std::io::ErrorKind::InvalidData=>"invalid",
        _=>"other",
    }
}

struct Observation<'a> { path: Option<&'a Path>, began: Instant, record: Record }
impl Drop for Observation<'_> {
    fn drop(&mut self) {
        if let Some(t) = &self.record.trace { PARENT.set(t.parent); }
        self.record.us = self.began.elapsed().as_micros().min(u64::MAX as u128) as u64;
        #[cfg(test)]
        OBSERVED.with(|v| { *v.borrow_mut() += &format!("{PREFIX}{}\n", serde_json::to_string(&self.record).unwrap()); });
        if self.path.is_some() {
            if let Err(e) = crate::telemetry::record(crate::telemetry::Sample::Operation(self.record.clone())) {
                let _ = writeln!(std::io::stderr().lock(), "Profiling: unknown ({})", append_error(&e));
            }
        }
    }
}

fn measure_at<T, E: std::fmt::Display>(path: Option<&Path>, detailed: bool, op: &'static str, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    observe(path,detailed,op,f,|e|error(&e.to_string()))
}

fn observe<T,E>(path:Option<&Path>,detailed:bool,op:&'static str,f:impl FnOnce()->Result<T,E>,class:impl FnOnce(&E)->&'static str)->Result<T,E> {
    let mut observation = begin(path, detailed, op);
    let result = f();
    observation.record.error = result.as_ref().err().map_or("none", class).into();
    result
}

fn begin<'a>(path: Option<&'a Path>, detailed: bool, op: &'static str) -> Observation<'a> {
    let trace = detailed.then(|| {
        let span = SPAN.fetch_add(1, Ordering::Relaxed);
        Trace { run: *RUN.get_or_init(|| SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|d| d.as_nanos().try_into().ok())),
            process: std::process::id(), span, parent: PARENT.replace(span) }
    });
    Observation { path, began: Instant::now(), record: Record { op: op.into(), us: 0, error: "panic".into(), at_ms: now_ms(), trace, counts: None } }
}

thread_local!(static SOURCE_WORK: std::cell::RefCell<BTreeMap<String,u64>> = const {std::cell::RefCell::new(BTreeMap::new())});
#[cfg(test)]
thread_local!(pub(crate) static LAST_SOURCE_WORK: std::cell::RefCell<BTreeMap<String,u64>> = const {std::cell::RefCell::new(BTreeMap::new())});
#[cfg(test)]
thread_local!(pub(crate) static SOURCE_BASELINE: Cell<bool> = const {Cell::new(false)});
/// A1/A5: fixed numeric I/O counters only, never transcript content, session ids or paths.
pub(crate) fn source_count(name:&'static str,n:u64) {SOURCE_WORK.with(|counts|*counts.borrow_mut().entry(name.into()).or_default()+=n);}
pub(crate) fn source_read<T,E:std::fmt::Display>(read:impl FnOnce()->Result<T,E>)->Result<T,E> {
    let prior=SOURCE_WORK.with(|c|std::mem::take(&mut *c.borrow_mut()));
    let mut observation=begin(LOG.get().map(PathBuf::as_path),TRACE.get().copied().unwrap_or(false),"conversations.source");
    let result=read();let counts=SOURCE_WORK.with(|c|std::mem::take(&mut *c.borrow_mut()));
    observation.record.error=result.as_ref().err().map_or("none".into(),|e|error(&e.to_string()).into());
    #[cfg(test)] LAST_SOURCE_WORK.with(|c|*c.borrow_mut()=counts.clone());
    observation.record.counts=Some(counts);SOURCE_WORK.with(|c|*c.borrow_mut()=prior);result
}

/// [`measure`] of an infallible operation that also records its counts.
pub fn measure_counted<T>(op: &'static str, f: impl FnOnce() -> (T, BTreeMap<String, u64>)) -> T {
    let mut observation = begin(LOG.get().map(PathBuf::as_path), TRACE.get().copied().unwrap_or(false), op);
    let (out, counts) = f();
    observation.record.error = "none".into();
    observation.record.counts = Some(counts);
    out
}

/// § 4 Profiling attempt stages; the behavioral model (§ 2) names where each starts and ends.
pub const STAGES: [&str; 7] = ["claim", "start", "build", "checks", "review", "ask", "close"];
#[cfg(test)] const STAGE_PREFIX: &str = "taskq-stage ";
const OUTCOMES: [&str; 6] = ["ok", "ask", "retried", "failed", "cancelled", "unknown"];
/// Stage-only error classes: a child ended without hand-in; its issue left the board read or could not be read back.
pub(crate) const STAGE_ERRORS: [&str; 3] = ["exit", "left-board", "unreadable"];
pub(crate) const RUNTIMES: [&str; 5] = { let r = crate::runtime::RUNTIMES; [r[0], r[1], r[2], r[3], "other"] };
const UNKNOWN: [&str; 6] = ["no-model", "no-usage", "log-unreadable", "app-turn", "adopted", "open-stage"];

/// Model use of one stage: service stages run no model; a runtime that reports none reads unknown, never zero;
/// `tokens` (context: input with cache reads and writes), its split `input`, `cache_read`, `cache_write`, and `output`
/// are `None` when any counted call lacked them; `model` is the model of most calls.
pub enum Usage { NoModel, Unknown(&'static str), Known { calls: Option<u64>, tokens: Option<u64>, model: Option<String>,
    input: Option<u64>, cache_read: Option<u64>, cache_write: Option<u64>, output: Option<u64> } }

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    stage: String,
    task: u64,
    attempt: u64,
    rt: String,
    /// `None`: the stage began in a replaced service image (adopted child), reason in `unknown`
    us: Option<u64>,
    outcome: String,
    error: String,
    at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unknown: Option<String>,
}

impl Stage {
    pub(crate) fn valid(&self)->bool {
        STAGES.contains(&self.stage.as_str())&&RUNTIMES.contains(&self.rt.as_str())&&OUTCOMES.contains(&self.outcome.as_str())
            &&(ERRORS.contains(&self.error.as_str())||STAGE_ERRORS.contains(&self.error.as_str()))
            &&self.unknown.as_deref().is_none_or(|u|UNKNOWN.contains(&u))
    }
}

pub(crate) fn now_ms() -> Option<u64> {
    SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis().min(u64::MAX as u128) as u64)
}

/// An open lifetime marker, not a completed duration sample.
pub fn open(install: &Path, project: &str, host: &str, task: u64, attempt: u64, rt: &str, reviewer: bool) {
    let r = Stage { project: Some(project.into()), host: Some(host.into()), open: true,
        stage: if reviewer { "review" } else { "build" }.into(), task, attempt, rt: RUNTIMES.into_iter().find(|r| *r == rt).unwrap_or("other").into(),
        us: None, outcome: "unknown".into(), error: "none".into(), at_ms: now_ms(), calls: None, tokens: None, model: None, output_tokens: None, input_tokens: None, cache_read_tokens: None, cache_write_tokens: None, unknown: Some("open-stage".into()) };
    write_stage(&install.join("taskq.log"), &r);
}

fn write_stage(path: &Path, r: &Stage) {
    if !path.parent().is_some_and(Path::is_dir) {
        let _ = writeln!(std::io::stderr().lock(), "Profiling: unknown (install directory missing)");
        return;
    }
    if let Err(e) = crate::telemetry::record(crate::telemetry::Sample::Stage(r.clone())) {
        let _ = writeln!(std::io::stderr().lock(), "Profiling: unknown ({})", append_error(&e));
    }
}

fn terminal(r: &Stage) -> bool {
    !r.open && (matches!(r.outcome.as_str(), "failed" | "cancelled")
        || (matches!(r.stage.as_str(), "build" | "review" | "close") && matches!(r.outcome.as_str(), "ok" | "ask" | "retried")))
}

fn unresolved_samples(samples:&[Sample]) -> Result<Vec<Stage>, String> {
    let mut last = BTreeMap::new();
    for sample in samples {
        let r=match sample {Sample::Stage(r)=>r.clone(),Sample::Corrupt("stage")=>return Err("corrupt stage record".into()),_=>continue};
        if !STAGES.contains(&r.stage.as_str()) || !OUTCOMES.contains(&r.outcome.as_str())
            || !RUNTIMES.contains(&r.rt.as_str()) || !(ERRORS.contains(&r.error.as_str()) || STAGE_ERRORS.contains(&r.error.as_str()))
            || r.unknown.as_deref().is_some_and(|u| !UNKNOWN.contains(&u)) {
            return Err("unknown stage code".into());
        }
        if r.attempt != 0 {
            let key = (r.project.clone(), r.task, r.attempt);
            // Auxiliary stages cannot erase a completed turn; a new lifetime resets its evidence.
            if r.open || terminal(&r) || matches!(r.stage.as_str(), "build" | "review" | "close")
                || !last.get(&key).is_some_and(terminal) {
                last.insert(key, r);
            }
        }
    }
    Ok(last.into_values().filter(|r| !terminal(r)).collect())
}

/// Only called after normal board recovery accepted `gone`; observations cannot trigger recovery.
pub fn repair(install: &Path, project: &str, host: &str, task: u64, attempt: u64, rt: &str, reviewer: bool) {
    let pending = crate::telemetry::report_samples(install,&["stage"]).and_then(|t| unresolved_samples(&t));
    let Ok(rows) = pending else {
        let _ = writeln!(std::io::stderr().lock(), "Profiling: unknown (terminal records unreadable)");
        return;
    };
    if !rows.iter().any(|r| r.project.as_deref() == Some(project) && r.task == task && r.attempt == attempt) { return; }
    let r = Stage { project: Some(project.into()), host: Some(host.into()), open: false,
        stage: if reviewer { "review" } else { "build" }.into(), task, attempt, rt: RUNTIMES.into_iter().find(|r| *r == rt).unwrap_or("other").into(), us: None,
        outcome: "failed".into(), error: "exit".into(), at_ms: now_ms(), calls: None, tokens: None, model: None, output_tokens: None, input_tokens: None, cache_read_tokens: None, cache_write_tokens: None, unknown: Some("adopted".into()) };
    write_stage(&install.join("taskq.log"), &r);
}

/// Repair retained attempts only when a fresh issue read proves closure or board removal.
/// Unreadable issues/logs and attempts still on the board remain unknown.
/// `open`: tasks this pass listed open, so no issue read; `unowned`: closed-task rows already proved another host's,
/// so no reread each pass (#1359). ponytail: in-process; a restart checks each once again.
pub fn repair_ended(install: &Path, root: &Path, project: &str, host: &str, board: &dyn crate::board::Board,
    open: &std::collections::HashSet<u64>, unowned: &mut std::collections::HashSet<(u64, u64)>) {
    let Ok(rows) = ended_rows(install, false) else { return };
    let mut fresh = None;
    for mut r in rows.into_iter().filter(|r| r.project.as_deref() == Some(project)
        && r.host.as_deref().is_none_or(|h| h == host) && !open.contains(&r.task)) {
        if unowned.contains(&(r.task, r.attempt)) { continue; }
        let Ok(issue) = board.issue(r.task) else { continue };
        if issue.open && crate::service::is_task(&issue) { continue; }
        let Ok(log) = crate::board::entries(board, r.task) else { continue };
        if r.host.is_none() {
            let through: Vec<_> = log.iter().filter(|e| e.id <= r.attempt).cloned().collect();
            let owner = crate::core::fold(r.task, &through, false);
            if owner.attempt != Some(r.attempt) || crate::service::host_of(&owner) != Some(host)
                || owner.session.as_deref().and_then(|s| s.split_once(':')).map(|s| s.0) != Some(r.rt.as_str()) {
                unowned.insert((r.task, r.attempt));
                continue;
            }
            let Some(role) = owner.role.as_deref() else { unowned.insert((r.task, r.attempt)); continue };
            let path = crate::runtime::attempt_log(root, role, r.task, r.attempt);
            if !fs::read_to_string(path).is_ok_and(|text| !text.is_empty()) { unowned.insert((r.task, r.attempt)); continue; }
            r.host = Some(host.into());
            r.stage = if role == "reviewer" { "review" } else { "build" }.into();
        }
        // A cached row may have ended normally since the read; only a fresh snapshot may justify the repair.
        if fresh.is_none() { let Ok(rows) = ended_rows(install, true) else { return }; fresh = Some(rows); }
        if !fresh.as_ref().is_some_and(|rows| rows.iter().any(|f| same(f, &r))) { continue; }
        let (outcome, error) = crate::core::stage_outcome(r.task, Ok((&log, Some(false))), r.attempt, r.attempt);
        r.stage = if r.stage == "review" { "review" } else { "build" }.into();
        r.open = false;
        r.outcome = outcome.into();
        r.error = error.into();
        r.at_ms = now_ms();
        r.us = None;
        r.calls = None;
        r.tokens = None;
        (r.model, r.output_tokens, r.input_tokens, r.cache_read_tokens, r.cache_write_tokens) = (None, None, None, None, None);
        r.unknown = Some("adopted".into());
        write_stage(&install.join("taskq.log"), &r);
        if let Some((_, rows)) = UNRESOLVED.lock().unwrap_or_else(|e| e.into_inner()).get_mut(install) { rows.retain(|f| !same(f, &r)); }
    }
}

/// One full-store read serves every project thread for `REPAIR_READ`: a full read scans the whole store and, before
/// coreq v0.14.5, pinned the WAL and failed the writer's checkpoint (#1341). `fresh` bypasses the cache.
const REPAIR_READ: std::time::Duration = std::time::Duration::from_secs(300);
static UNRESOLVED: std::sync::Mutex<BTreeMap<PathBuf, (Instant, Vec<Stage>)>> = std::sync::Mutex::new(BTreeMap::new());
#[cfg(test)]
thread_local!(pub(crate) static STORE_READS: Cell<usize> = const { Cell::new(0) });

fn ended_rows(install: &Path, fresh: bool) -> Result<Vec<Stage>, String> {
    let mut cache = UNRESOLVED.lock().unwrap_or_else(|e| e.into_inner());
    if !fresh && let Some((at, rows)) = cache.get(install) && at.elapsed() < REPAIR_READ { return Ok(rows.clone()); }
    #[cfg(test)] STORE_READS.with(|n| n.set(n.get() + 1));
    let rows = crate::telemetry::report_samples(install,&["stage"]).and_then(|t| unresolved_samples(&t))?;
    cache.insert(install.to_owned(), (Instant::now(), rows.clone()));
    Ok(rows)
}

fn same(a: &Stage, b: &Stage) -> bool { (&a.project, a.task, a.attempt) == (&b.project, b.task, b.attempt) }

type Live = Vec<(String, u64)>;

fn defects_samples(samples:&[Sample], live: &Live, project: Option<&str>, now: u64, ttl: u64) -> Result<(String, usize), String> {
    if !samples.iter().any(|s|s.kind()=="stage") { return Ok(("Attempt terminal records: unknown (no stage observations)\n".into(), 0)); }
    let mut out = String::from("Attempt terminal records: retained window only; live exclusions require fresh evidence\n\n| Project | Task | Attempt | Runtime | Host | Age s | Defect |\n|---|---|---|---|---|---|---|\n");
    let mut overdue = 0;
    for r in unresolved_samples(samples)? {
        if project.is_some_and(|p| r.project.as_deref() != Some(p))
            || live.iter().any(|(p, a)| r.project.as_deref() == Some(p) && r.attempt == *a) { continue; }
        let age = r.at_ms.filter(|at| *at <= now).map(|at| (now - at) / 1000);
        if age.is_none() { return Err("unresolved attempt age unknown".into()); }
        overdue += usize::from(age.is_some_and(|a| a > ttl));
        out += &format!("| {} | #{} | {} | {} | {} | {} | missing terminal record |\n", crate::cell(r.project.as_deref().unwrap_or("unknown")),
            r.task, r.attempt, crate::cell(&r.rt), crate::cell(r.host.as_deref().unwrap_or("unknown (not recorded)")), age.unwrap());
    }
    Ok((out, overdue))
}

/// Existing registered-project snapshots are caches, not a new liveness store; stale ones exclude nothing.
fn local_live(install: &Path) -> Live {
    let tool = crate::platform::tool(install);
    fs::read_to_string(install.join("projects")).unwrap_or_default().lines().filter_map(|root| {
        coreq::status::read(&tool, Path::new(root), 60)
    }).filter(|v| v["complete"] == true).flat_map(|v| {
        let project = v["board"]["repo"].as_str().unwrap_or_default().to_string();
        v["kids"].as_array().into_iter().flatten().filter(|k| k["live"] == "running")
            .filter_map(|k| Some((project.clone(), k["attempt"].as_u64()?))).collect::<Live>()
    }).collect()
}

pub fn selftest(install: &Path, project: &str, live: &Live, ttl: u64) -> Result<String, String> {
    let now = now_ms().ok_or("profile: clock unknown")?;
    let samples=crate::telemetry::report_samples(install,&["stage"])?;
    if !samples.iter().any(|s|s.kind()=="stage") { return Ok("attempt terminal records: unknown (no stage observations)".into()); }
    let (report, overdue) = defects_samples(&samples, live, Some(project), now, ttl)?;
    // Telemetry gaps never block install/update (#1389); capture health still fails selftest in main.rs.
    if overdue > 0 { Ok(format!("attempt terminal records: WARN ({overdue} older than ttl {ttl}s; not blocking)\n{report}")) }
    else { Ok("attempt terminal records: no overdue nonlive defects in retained window".into()) }
}

/// The error class of a stage that failed with `text`, never the text itself.
pub fn class(text: &str) -> &'static str { error(text) }

/// One typed stage observation; optional collection failure never fails the stage.
pub fn stage(name: &'static str, task: u64, attempt: u64, rt: &str, took: Option<std::time::Duration>, outcome: &'static str, err: &'static str, usage: Usage) {
    #[cfg(test)]
    STAGED.with(|v| v.borrow_mut().push(format!("{name} {task} {attempt} {rt} {outcome} {err}")));
    stage_at(LOG.get().map(PathBuf::as_path), name, task, attempt, rt, took, outcome, err, usage)
}

#[allow(clippy::too_many_arguments)]
fn stage_at(path: Option<&Path>, name: &'static str, task: u64, attempt: u64, rt: &str, took: Option<std::time::Duration>, outcome: &'static str, err: &'static str, usage: Usage) {
    let none = (None, None, None, None, None, None, None);
    let ((calls, tokens, model, input_tokens, cache_read_tokens, cache_write_tokens, output_tokens), unknown) = match usage {
        Usage::NoModel => (none, Some("no-model")),
        Usage::Unknown(r) => (none, Some(r)),
        Usage::Known { calls, tokens, model, input, cache_read, cache_write, output } =>
            ((calls, tokens, model, input, cache_read, cache_write, output), tokens.is_none().then_some("no-usage")),
    };
    let r = Stage { host: HOST.with_borrow(Clone::clone), open: false, project: PROJECT.with_borrow(Clone::clone), stage: name.into(), task, attempt, rt: RUNTIMES.into_iter().find(|k| *k == rt).unwrap_or("other").into(),
        us: took.map(|d| d.as_micros().min(u64::MAX as u128) as u64), outcome: outcome.into(), error: err.into(), at_ms: now_ms(), calls, tokens, model, output_tokens,
        input_tokens, cache_read_tokens, cache_write_tokens, unknown: unknown.map(Into::into) };
    if path.is_none() {return;}
    if let Err(e) = crate::telemetry::record(crate::telemetry::Sample::Stage(r)) {
        let _ = writeln!(std::io::stderr().lock(), "Profiling: unknown ({})", append_error(&e));
    }
}

/// Attempt stages ranked per runtime (§ 4 Profiling); a corrupt record makes the whole section unknown.
fn render_stages_samples(samples:&[Sample]) -> Result<String, &'static str> {
    let mut rows: Vec<((usize, &str), Vec<Stage>)> = vec![];
    for sample in samples {
        let r=match sample {Sample::Stage(r)=>r.clone(),Sample::Corrupt("stage")=>return Err("corrupt stage record; log kept"),_=>continue};
        let i = STAGES.iter().position(|s| *s == r.stage).ok_or("unknown stage; log kept")?;
        let rt = RUNTIMES.into_iter().find(|k| *k == r.rt).ok_or("unknown stage runtime; log kept")?;
        if !OUTCOMES.contains(&r.outcome.as_str()) || !(ERRORS.contains(&r.error.as_str()) || STAGE_ERRORS.contains(&r.error.as_str()))
            || r.unknown.as_deref().is_some_and(|u| !UNKNOWN.contains(&u)) { return Err("unknown stage code; log kept"); }
        if r.open { continue; }
        match rows.iter_mut().find(|(k, _)| *k == (i, rt)) {
            Some((_, v)) => v.push(r),
            None => rows.push(((i, rt), vec![r])),
        }
    }
    if rows.is_empty() { return Ok("Attempt stages: unknown (no observations)\n".into()); }
    rows.sort_by_key(|(k, _)| *k);
    for (_, v) in &mut rows { v.sort_unstable_by_key(|r| r.us); }
    let share = |v: &[Stage], o: &str| v.iter().filter(|r| r.outcome == o).count() as f64 / v.len() as f64;
    // percentiles over timed records only (sorted, untimed first); untimed rows rank last
    let p = |v: &[Stage], p: usize| { let t: Vec<u64> = v.iter().filter_map(|r| r.us).collect(); (!t.is_empty()).then(|| t[(t.len() * p).div_ceil(100) - 1]) };
    let rank = |key: &dyn Fn(&[Stage]) -> f64| {
        let mut r: Vec<_> = rows.iter().collect();
        r.sort_by(|a, b| key(&b.1).total_cmp(&key(&a.1)).then(a.0.cmp(&b.0)));
        r.iter().map(|((i, rt), _)| format!("{}/{rt}", STAGES[*i])).collect::<Vec<_>>().join(" > ")
    };
    let mut out = format!("Attempt stages: retained observations; all budgets unknown\nSlow stages (p95): {}\nFailing stages (rate): {}\nRetried stages (rate): {}\n\n| Stage | Runtime | Count | p50/p95/max ms | Outcomes | Model calls | Context tokens | Budget |\n|---|---|---:|---|---|---|---|---|\n",
        rank(&|v| p(v, 95).map_or(-1., |u| u as f64)), rank(&|v| share(v, "failed")), rank(&|v| share(v, "retried")));
    for ((i, rt), v) in &rows {
        let mut counts = std::collections::BTreeMap::new();
        for r in v.iter() {
            *counts.entry(if r.error == "none" { r.outcome.clone() } else { format!("{} {}", r.outcome, r.error) }).or_insert(0) += 1;
        }
        let outcomes = counts.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(", ");
        let unknown = |field: fn(&Stage) -> Option<u64>| {
            let known: Vec<u64> = v.iter().filter_map(field).collect();
            let mut reasons: Vec<&str> = v.iter().filter(|r| field(r).is_none()).map(|r| r.unknown.as_deref().unwrap_or("not-reported")).collect();
            reasons.sort_unstable();
            reasons.dedup();
            match known.len() {
                0 if reasons == ["no-model"] => "none (no model)".to_string(),
                0 => format!("unknown ({})", reasons.join(", ")),
                k if k == v.len() => known.iter().sum::<u64>().to_string(),
                k => format!("{} ({k} of {} known; {})", known.iter().sum::<u64>(), v.len(), reasons.join(", ")),
            }
        };
        let ms = |u: u64| u as f64 / 1000.;
        let timed = v.iter().filter(|r| r.us.is_some()).count();
        let took = match (p(v, 50), p(v, 95), v.last().and_then(|r| r.us)) {
            (Some(a), Some(b), Some(c)) if timed == v.len() => format!("{:.3}/{:.3}/{:.3}", ms(a), ms(b), ms(c)),
            (Some(a), Some(b), Some(c)) => format!("{:.3}/{:.3}/{:.3} ({timed} of {} timed; adopted)", ms(a), ms(b), ms(c), v.len()),
            _ => "unknown (adopted)".into(),
        };
        out += &format!("| {} | {rt} | {} | {took} | {outcomes} | {} | {} | unknown |\n", STAGES[*i], v.len(), unknown(|r| r.calls), unknown(|r| r.tokens));
    }
    Ok(out)
}

/// § 4 Profiling board operation kinds, one per board adapter call.
pub const BOARD_OPS: [&str; 10] = ["list", "inbox", "read", "claim", "comment", "label", "close", "write", "publish", "history"];
#[cfg(test)] const BOARD_PREFIX: &str = "taskq-board ";
#[cfg(test)] const TRANSITION_PREFIX: &str = "taskq-transition ";
const STATES: [&str; 10] = ["ready", "waiting", "doing", "review", "merge-queue", "ask", "later", "failed", "closed", "held"];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BoardOp { op: String, us: u64, outcome: String, error: String, remaining: Option<u64>, at_ms: Option<u64> }

impl BoardOp {
    pub(crate) fn valid(&self)->bool {BOARD_OPS.contains(&self.op.as_str())&&ERRORS.contains(&self.error.as_str())&&["ok","retried","ask","failed"].contains(&self.outcome.as_str())}
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Transition { #[serde(default, skip_serializing_if = "Option::is_none")] project: Option<String>, task: u64, entry: u64, from: String, to: String, prev_s: Option<u64>, rt: Option<String>, host: Option<String>, at_ms: Option<u64>,
    /// § 4 Profiling: a worker ask as `{"ask.<kind>.<cause>": 1}`; CoreQ admits `counts` without a new field
    #[serde(default, skip_serializing_if = "Option::is_none")] counts: Option<BTreeMap<String, u64>> }

const ASK_CAUSES: [&str; 6] = ["unknown-term", "rule-gate", "budget", "retry-limit", "spec-conflict", "other"];

/// Kind and cause of a transition's `ask.<kind>.<cause>` count.
fn ask_key(t: &Transition) -> Option<(&str, &str)> {
    let c = t.counts.as_ref()?;
    let (key, _) = c.iter().next().filter(|_| c.len() == 1)?;
    key.strip_prefix("ask.")?.split_once('.').filter(|(k, c)| ["technical", "owner"].contains(k) && ASK_CAUSES.contains(c))
}

/// § 4 Profiling: kind (`technical`/`owner`) and cause of the worker ask that log `entry` text raised.
pub(crate) fn ask_of(entry: &str) -> (&'static str, &'static str) {
    // ponytail: keyword heuristic over the entry text; a typed cause field on `ask` if it misfiles often
    let t = entry.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    let cause = if has(&["retry limit"]) { "retry-limit" }
        else if has(&["unknown term"]) { "unknown-term" }
        else if has(&["rule gate", "rule-change gate", "does not name"]) { "rule-gate" }
        else if has(&["budget", "quota", "usage limit", "rate limit"]) { "budget" }
        else if has(&["conflicts with taskq.md", "contract conflict", "spec conflict", "conflicts with the contract", "r13"]) { "spec-conflict" }
        else { "other" };
    // an owner action card has no options (§ 2 Ask card); a spec conflict is the owner's decision (R13)
    let owner = cause == "spec-conflict" || !t.lines().any(|l| l.trim_start().starts_with("1. "));
    (if owner { "owner" } else { "technical" }, cause)
}

impl Transition {
    pub(crate) fn valid(&self)->bool {STATES.contains(&self.from.as_str())&&STATES.contains(&self.to.as_str())&&self.rt.as_deref().is_none_or(|rt|RUNTIMES.contains(&rt))&&(self.counts.is_none()||ask_key(self).is_some())}
}

/// Admit typed metadata; optional collection failure never changes the observed operation.
fn record(r: crate::telemetry::Sample) {
    if LOG.get().is_none() {return;}
    if let Err(e) = crate::telemetry::record(r) {
        let _ = writeln!(std::io::stderr().lock(), "Profiling: unknown ({})", append_error(&e));
    }
}

/// One board operation of `kind`; `failed` is its error text (only its class is kept).
pub fn board(kind: &'static str, took: std::time::Duration, failed: Option<&str>, remaining: Option<u64>) {
    board_as(kind, took, if failed.is_some() { "failed" } else { "ok" }, failed, remaining)
}

/// `outcome` `ok`, `retried` or `ask` (not in `Failed %`) or `failed`; `err` keeps the class of a non-ok call.
pub fn board_as(kind: &'static str, took: std::time::Duration, outcome: &'static str, err: Option<&str>, remaining: Option<u64>) {
    record(crate::telemetry::Sample::Board(BoardOp { op: kind.into(), us: took.as_micros().min(u64::MAX as u128) as u64,
        outcome: outcome.into(), error: err.map_or("none", error).into(), remaining, at_ms: now_ms() }));
}

/// One queue transition of `task` caused by log `entry`; `rt`/`host` of the attempt on either side.
pub fn transition(task: u64, entry: u64, from: &'static str, to: &'static str, prev_s: Option<u64>, rt: Option<&str>, host: Option<&str>, ask: Option<(&str, &str)>) {
    // a host name is the operator's own short label; anything else is not kept
    let host = host.map(|h| if h.len() <= 64 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) { h } else { "other" });
    let r = Transition { project: PROJECT.with_borrow(Clone::clone), task, entry, from: from.into(), to: to.into(), prev_s,
        rt: rt.map(|r| RUNTIMES.into_iter().find(|k| *k == r).unwrap_or("other").into()), host: host.map(Into::into), at_ms: now_ms(),
        counts: ask.map(|(k, c)| BTreeMap::from([(format!("ask.{k}.{c}"), 1)])) };
    #[cfg(test)]
    MOVED.with(|v| { *v.borrow_mut() += &format!("{TRANSITION_PREFIX}{}\n", serde_json::to_string(&r).unwrap()); });
    record(crate::telemetry::Sample::Transition(r));
}

/// p50/p95/max of sorted `v`, each divided by `div`.
fn spread(v: &[u64], div: f64) -> String {
    let p = |p: usize| v[(v.len() * p).div_ceil(100) - 1] as f64 / div;
    format!("{:.3}/{:.3}/{:.3}", p(50), p(95), *v.last().unwrap() as f64 / div)
}

/// Seconds in the previous state of sorted `s` plus `unknown` moves without an entry time.
fn in_state(s: &[u64], unknown: usize) -> String {
    match (s.is_empty(), unknown) {
        (true, _) => "unknown (no entry time)".into(),
        (false, 0) => spread(s, 1.),
        (false, u) => format!("{} ({} of {} known; no entry time)", spread(s, 1.), s.len(), s.len() + u),
    }
}

/// Board operations by p95 and queue transitions (§ 4 Profiling); a corrupt record makes its section unknown.
fn render_system_summary(summary:&Summary) -> (String, String) {
    let samples=summary.samples();
    let board = || -> Result<String, &'static str> {
        if summary.corrupt_board {return Err("corrupt board record; log kept");}
        let rows=&summary.board;
        if rows.is_empty() { return Ok("Board operations: unknown (no observations)\n".into()); }
        let mut out = "Board operations: retained observations by p95; all budgets unknown\n\n| Operation | Count | p50/p95/max ms | Failed | Errors | Lowest remaining | Budget |\n|---|---:|---|---:|---|---|---|\n".to_string();
        for (op, us, errors, low) in rows {
            let e = errors.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(", ");
            out += &format!("| {op} | {} | {} | {} | {} | {} | unknown |\n", us.len(), spread(&us, 1000.), errors.values().sum::<usize>(),
                if e.is_empty() { "none" } else { &e }, low.map_or("unknown (not reported)".into(), |l| l.to_string()));
        }
        Ok(out)
    };
    let moves = || -> Result<String, &'static str> {
        let mut rows: BTreeMap<(usize, usize, String), (Vec<u64>, usize)> = BTreeMap::new();
        for sample in samples {
            let r=match sample {Sample::Transition(r)=>r.clone(),Sample::Corrupt("transition")=>return Err("corrupt transition record; log kept"),_=>continue};
            let state = |s: &str| STATES.iter().position(|k| *k == s).ok_or("unknown transition state; log kept");
            if r.rt.as_deref().is_some_and(|rt| !RUNTIMES.contains(&rt)) { return Err("unknown transition runtime; log kept"); }
            let row = rows.entry((state(&r.from)?, state(&r.to)?, r.rt.unwrap_or_else(|| "none".into()))).or_default();
            match r.prev_s { Some(s) => row.0.push(s), None => row.1 += 1 }
        }
        if rows.is_empty() { return Ok("Queue transitions: unknown (no observations)\n".into()); }
        let mut asks: BTreeMap<(String, String, String), usize> = BTreeMap::new();
        for sample in samples {
            if let Some((k, c, at_ms)) = match sample { Sample::Transition(t) => ask_key(t).map(|(k, c)| (k.to_string(), c.to_string(), t.at_ms)), _ => None } {
                let day = at_ms.map_or("unknown".into(), |ms| crate::board::stamp((ms / 1000) as i64)[..10].to_string());
                *asks.entry((day, k, c)).or_default() += 1;
            }
        }
        let mut tail = String::new();
        if !asks.is_empty() {
            tail = "\nWorker asks: per UTC day, kind and cause\n\n| Day | Kind | Cause | Count |\n|---|---|---|---:|\n".into();
            for ((day, k, c), n) in asks { tail += &format!("| {day} | {k} | {c} | {n} |\n"); }
        }
        let mut out = "Queue transitions: retained observations; moves while no service ran unknown\n\n| From | To | Runtime | Count | p50/p95/max s in previous state |\n|---|---|---|---:|---|\n".to_string();
        for ((f, t, rt), (mut s, unknown)) in rows {
            s.sort_unstable();
            out += &format!("| {} | {} | {rt} | {} | {} |\n", STATES[f], STATES[t], s.len() + unknown, in_state(&s, unknown));
        }
        Ok(out + &tail)
    };
    (board().unwrap_or_else(|e| format!("Board operations: unknown ({e})\n")), moves().unwrap_or_else(|e| format!("Queue transitions: unknown ({e})\n")))
}

pub struct LogLock(fs::File);
impl Drop for LogLock {
    fn drop(&mut self) {
        // A child can inherit the descriptor on Unix; close alone can leave its lock held.
        let _ = self.0.unlock();
    }
}

pub fn log_lock(path: &Path) -> std::io::Result<LogLock> {
    acquire_log_lock(path, false)
}

fn acquire_log_lock(path: &Path, wait: bool) -> std::io::Result<LogLock> {
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path.parent().unwrap().join("taskq-profile.lock"))?;
    lock_log(lock,wait,false)
}

pub(crate) fn read_log_lock(install: &Path) -> std::io::Result<LogLock> {
    lock_log(fs::File::open(install.join("taskq-profile.lock"))?,true,true)
}

fn lock_log(lock: fs::File, wait: bool, shared: bool) -> std::io::Result<LogLock> {
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match if shared {lock.try_lock_shared()} else {lock.try_lock()} {
            Ok(()) => return Ok(LogLock(lock)),
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
            Err(std::fs::TryLockError::WouldBlock) if !wait => return Err(std::io::ErrorKind::WouldBlock.into()),
            Err(std::fs::TryLockError::WouldBlock) => {
                if Instant::now() >= deadline { return Err(std::io::ErrorKind::TimedOut.into()); }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

pub fn append_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::TimedOut => "taskq-profile.lock timeout after 1 s".into(),
        _ => format!("telemetry append failed: {e}"),
    }
}

#[cfg(test)]
fn append(_path:&Path,r:&Record)->std::io::Result<()> {
    crate::telemetry::record(crate::telemetry::Sample::Operation(r.clone()))
}

/// Historical compatibility input used by the regression fixtures, never by a live producer.
#[cfg(test)]
pub fn append_line(_path: &Path, line: &str) -> std::io::Result<()> {
    crate::telemetry::observe(line).map_err(std::io::Error::other)
}

struct Stats { op: &'static str, times: Vec<u64>, errors: std::collections::BTreeMap<String, usize> }
impl Stats {
    fn percentile(&self, p: usize) -> u64 { self.times[(self.times.len() * p).div_ceil(100) - 1] }
    fn failures(&self) -> usize { self.errors.values().sum() }
    fn rate(&self) -> f64 { self.failures() as f64 / self.times.len() as f64 }
    fn variance(&self) -> f64 {
        let mean = self.times.iter().map(|t| *t as f64).sum::<f64>() / self.times.len() as f64;
        self.times.iter().map(|t| (*t as f64 - mean).powi(2)).sum::<f64>() / self.times.len() as f64
    }
}

type BoardStats=(&'static str,Vec<u64>,BTreeMap<String,usize>,Option<u64>);

struct Summary {
    operations:Vec<Stats>,board:Vec<BoardStats>,first:Option<u64>,last:Option<u64>,traces:usize,
    corrupt_operation:bool,corrupt_board:bool,ordered:[Vec<Sample>;3],
}
impl Summary {
    fn new()->Self {Self {operations:OPS.into_iter().map(|op|Stats {op,times:vec![],errors:Default::default()}).collect(),board:vec![],first:None,last:None,traces:0,corrupt_operation:false,corrupt_board:false,ordered:Default::default()}}
    fn push(&mut self,lane:usize,sample:Sample) {
        match sample {
            Sample::Operation(r)=>{
                let Some(stats)=self.operations.iter_mut().find(|s|s.op==r.op) else {self.corrupt_operation=true;return};
                if let Some(at)=r.at_ms {self.first=Some(self.first.map_or(at,|v|v.min(at)));self.last=Some(self.last.map_or(at,|v|v.max(at)));}
                self.traces+=usize::from(r.trace.is_some());stats.times.push(r.us);
                if r.error!="none" {*stats.errors.entry(r.error).or_default()+=1;}
            },
            Sample::Board(r)=>{
                let Some(op)=BOARD_OPS.into_iter().find(|op|*op==r.op) else {self.corrupt_board=true;return};
                let i=self.board.iter().position(|v|v.0==op).unwrap_or_else(||{self.board.push((op,vec![],Default::default(),None));self.board.len()-1});
                let row=&mut self.board[i];row.1.push(r.us);if r.outcome=="failed" {*row.2.entry(r.error).or_default()+=1;}
                if let Some(m)=r.remaining {row.3=Some(row.3.map_or(m,|v|v.min(m)));}
            },
            Sample::Corrupt("operation")=>self.corrupt_operation=true,Sample::Corrupt("board")=>self.corrupt_board=true,
            sample=>self.ordered[lane].push(sample),
        }
    }
    fn finish(&mut self) {
        for stats in &mut self.operations {stats.times.sort_unstable();}
        for row in &mut self.board {row.1.sort_unstable();}
        self.board.sort_by_key(|r|(std::cmp::Reverse(r.1[(r.1.len()*95).div_ceil(100)-1]),r.0));
        let [mut history,mut archive,mut live]=std::mem::take(&mut self.ordered);history.append(&mut archive);history.append(&mut live);self.ordered[0]=history;
    }
    fn samples(&self)->&[Sample] {&self.ordered[0]}
    #[cfg(test)]
    fn fixture(text:&str)->Self {let mut summary=Self::new();for sample in Sample::archive(text) {summary.push(0,sample);}summary.finish();summary}
}

fn render_summary(summary:&Summary) -> Result<String, &'static str> {
    if summary.corrupt_operation {return Err("corrupt profile record; log kept");}
    let stats=&summary.operations;let (first,last,traces)=(summary.first,summary.last,summary.traces);
    let rank = |key: fn(&Stats) -> f64| {
        let mut rows: Vec<_> = stats.iter().filter(|s| !s.times.is_empty()).collect();
        rows.sort_by(|a, b| key(b).total_cmp(&key(a)).then(a.op.cmp(b.op)));
        rows.iter().map(|s| s.op).collect::<Vec<_>>().join(" > ")
    };
    let ranking = |s: String| if s.is_empty() { "unknown (no observations)".into() } else { s };
    let window = first.zip(last).map_or("unknown (no timestamps)".into(), |(f, l)| format!("{f}..{l} Unix ms; earlier/pruned history unknown"));
    let mut out = format!("Profile: retained observations; all budgets unknown\nWindow: {window}\nCollection completeness: unknown (best-effort writes; check CLI/service diagnostics)\nDetailed traces: {traces} retained; switch {} for this run\nRetries/cancellations: unknown (not exposed by adapters)\nSlow (p95): {}\nFailing (rate): {}\nUnstable (variance): {}\n\n| Operation | Count | p50/p95/max ms | Success/failure | Failure % | Variance ms² | Errors | Budget |\n|---|---:|---|---|---:|---:|---|---|\n",
        if TRACE.get().copied().unwrap_or(false) { "on" } else { "off" },
        ranking(rank(|s| s.percentile(95) as f64)), ranking(rank(Stats::rate)), ranking(rank(Stats::variance)));
    for s in stats {
        if s.times.is_empty() {
            out += &format!("| {} | unknown (no observations) | unknown | unknown | unknown | unknown | unknown | unknown |\n", s.op);
        } else {
            let errors = s.errors.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(", ");
            out += &format!("| {} | {} | {:.3}/{:.3}/{:.3} | {}/{} | {:.2} | {:.6} | {} | unknown |\n", s.op, s.times.len(),
                s.percentile(50) as f64 / 1000., s.percentile(95) as f64 / 1000., *s.times.last().unwrap() as f64 / 1000.,
                s.times.len() - s.failures(), s.failures(), s.rate() * 100., s.variance() / 1_000_000., if errors.is_empty() { "none" } else { &errors });
        }
    }
    Ok(out)
}

/// Committed universal observations and the read-only historical archive.
#[cfg(test)]
pub(crate) fn read_log(install: &Path) -> Result<String, String> {
    crate::telemetry::report_input(install)
}

/// Service-owned pruning uses the same writer boundary as profiling and legacy log rotation.
pub fn prune(install: &Path) -> Result<(), String> {
    fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let _lock = acquire_log_lock(&install.join("taskq.log"), true).map_err(|e| e.to_string())?;
    crate::telemetry::prune(install, now_ms().ok_or("telemetry clock unknown")?)?;
    crate::data::trim_tail_locked(&install.join("taskq.log"),crate::data::LOG_MAX).map(|_|())
}

pub fn report(install: &Path) -> String {
    let read = || -> Result<String, String> {
        let mut summary=Summary::new();crate::telemetry::visit_samples(install,&["operation","stage","board","transition","requests"],|lane,sample|summary.push(lane,sample))?;summary.finish();
        let samples=summary.samples();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64);
        let profile = render_summary(&summary).unwrap_or_else(|e| format!("Profile: unknown ({e}); all budgets unknown\n"));
        let stages = render_stages_samples(&samples).unwrap_or_else(|e| format!("Attempt stages: unknown ({e})\n"));
        let (board, moves) = render_system_summary(&summary);
        let terminal = defects_samples(&samples, &local_live(install), None, now, 300).map(|r| r.0)
            .unwrap_or_else(|e| format!("Attempt terminal records: unknown ({e})\n"));
        Ok(format!("{profile}\n{board}\n{moves}\n{stages}\n{terminal}\n{}", crate::requests::sources_samples(&samples, now)))
    };
    let legacy=read().unwrap_or_else(|e| format!("Profile: unknown ({e}); all budgets unknown\n\nBoard operations: unknown ({e})\n\nQueue transitions: unknown ({e})\n\nAttempt stages: unknown ({e})\n\nBoard requests by source: unknown ({e})\n"));
    let actions=crate::telemetry::diagnostic::actions(install,None).unwrap_or_else(|e|format!("Actions: unknown ({e})\n"));
    format!("{legacy}\n{actions}")
}

/// § 5 `taskq profile export` columns, in order; every row has each of them, null where its kind has none. A new
/// column is a new [`EXPORT`] schema.
const COLUMNS: [&str; 24] = ["kind", "at_ms", "project", "task", "attempt", "entry", "name", "from", "to", "runtime", "host", "us", "prev_s",
    "outcome", "error", "remaining", "calls", "tokens", "input_tokens", "cache_read_tokens", "cache_write_tokens", "model", "output_tokens", "unknown"];
/// Export schema: 2 added `model` and `output_tokens`; 3 the context split `input_tokens`, `cache_read_tokens`, `cache_write_tokens`.
const EXPORT: u64 = 3;

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Row { kind: String, at_ms: Option<u64>, project: Option<String>, task: Option<u64>, attempt: Option<u64>, entry: Option<u64>,
    name: Option<String>, from: Option<String>, to: Option<String>, runtime: Option<String>, host: Option<String>, us: Option<u64>,
    prev_s: Option<u64>, outcome: Option<String>, error: Option<String>, remaining: Option<u64>, calls: Option<u64>, tokens: Option<u64>,
    input_tokens: Option<u64>, cache_read_tokens: Option<u64>, cache_write_tokens: Option<u64>, model: Option<String>, output_tokens: Option<u64>,
    unknown: Option<String> }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header { schema: u64, columns: Vec<String>, since_ms: Option<u64>, window_ms: Option<[u64; 2]>, samples: usize, corrupt: usize }

/// The export row of one log line: `None` for other lines, `Some(None)` for a corrupt record or an unknown code.
fn row_sample(sample:&Sample) -> Option<Option<Row>> {
    let s = |v: &str| Some(v.to_string());
    Some(Some(match sample {
        Sample::Operation(r)=>Row {kind: "operation".into(), at_ms: r.at_ms, name: s(&r.op), us: Some(r.us), outcome: s(if r.error == "none" { "ok" } else { "failed" }), error: s(&r.error), ..Default::default()},
        Sample::Stage(r)=>{let r=r.clone();Row {
            kind: "stage".into(), at_ms: r.at_ms, project: r.project, task: Some(r.task), attempt: Some(r.attempt), name: Some(r.stage), runtime: Some(r.rt),
            host: r.host, us: r.us, outcome: Some(r.outcome), error: Some(r.error), calls: r.calls, tokens: r.tokens, model: r.model,
            input_tokens: r.input_tokens, cache_read_tokens: r.cache_read_tokens, cache_write_tokens: r.cache_write_tokens,
            output_tokens: r.output_tokens, unknown: r.unknown, ..Default::default() }},
        Sample::Board(r)=>Row {kind: "board".into(), at_ms: r.at_ms, name: Some(r.op.clone()), us: Some(r.us), outcome: Some(r.outcome.clone()), error: Some(r.error.clone()), remaining: r.remaining, ..Default::default()},
        Sample::Transition(r)=>{let r=r.clone();Row {
            kind: "transition".into(), at_ms: r.at_ms, project: r.project, task: Some(r.task), entry: Some(r.entry), from: Some(r.from), to: Some(r.to),
            runtime: r.rt, host: r.host, prev_s: r.prev_s, ..Default::default() }},
        Sample::Requests(_)|Sample::Corrupt("requests")=>return None,
        Sample::Corrupt(_)=>return Some(None),
    }))
}

/// Canonical typed legacy observation; reuse report validation before common-store conversion.
pub(crate) fn legacy_value(line: &str) -> Result<serde_json::Value, String> {
    let sample=crate::telemetry::Sample::legacy(line)?;
    if sample.kind()=="requests" {return Err("unknown legacy profile kind".into());}
    sample.canonical()
}

#[cfg(test)]
pub(crate) fn legacy_reports(text: &str, now: u64) -> Result<Vec<String>,String> {
    let exported=export(text,None);let (board,moves)=render_system(text);
    Ok(vec![render(text)?,render_stages(text)?,board,moves,exported.clone(),render_tasks(&exported)?,render_board(&exported)?,defects(text,&vec![],None,now,300)?.0,crate::requests::sources(text,now)])
}

/// § 5 `taskq profile export`: a header naming window and sample count, then one JSON line per retained observation at or after `since_ms`.
struct Projection {header:Header,rows:[Vec<Row>;3],selected:Option<&'static str>}
impl Projection {
    fn new(since_ms:Option<u64>,selected:Option<&'static str>)->Self {
        Self {header:Header {schema:EXPORT,columns:COLUMNS.map(String::from).to_vec(),since_ms,window_ms:None,samples:0,corrupt:0},rows:Default::default(),selected}
    }
    fn push(&mut self,lane:usize,sample:Sample) {
        match row_sample(&sample) {
            Some(Some(row)) if self.header.since_ms.is_none_or(|s|row.at_ms.is_some_and(|a|a>=s))=>{
                self.header.samples+=1;
                if let Some(at)=row.at_ms {self.header.window_ms=Some(self.header.window_ms.map_or([at,at],|[a,b]|[a.min(at),b.max(at)]));}
                if self.selected.is_none_or(|kind|kind==row.kind) {self.rows[lane].push(row);}
            },
            Some(None)=>self.header.corrupt+=1,_=>{},
        }
    }
    fn finish(self)->(Header,Vec<Row>) {(self.header,self.rows.into_iter().flatten().collect())}
}

#[cfg(test)]
fn report_rows(samples:&[Sample],since_ms:Option<u64>)->(Header,Vec<Row>) {
    let mut projection=Projection::new(since_ms,None);for sample in samples {projection.push(0,sample.clone());}projection.finish()
}

#[cfg(test)]
fn export(text:&str,since_ms:Option<u64>)->String {
    let (h,rows)=report_rows(&Sample::archive(text),since_ms);
    let mut out = serde_json::to_string(&h).unwrap() + "\n";
    for r in rows { out += &(serde_json::to_string(&r).unwrap() + "\n"); }
    out
}

#[cfg(test)]
fn read_export(export: &str) -> Result<(Header, Vec<Row>), &'static str> {
    let mut lines = export.lines();
    let h: Header = lines.next().and_then(|l| serde_json::from_str(l).ok()).filter(|h: &Header| h.schema == EXPORT && h.columns == COLUMNS).ok_or("export header unreadable")?;
    let rows = lines.map(|l| serde_json::from_str(l).map_err(|_| "export row unreadable")).collect::<Result<Vec<Row>, _>>()?;
    Ok((h, rows))
}

fn window(h: &Header) -> String {
    let since = h.since_ms.map_or("all retained".into(), |s| format!("since {s}"));
    let w = h.window_ms.map_or("unknown (no timestamps)".into(), |[a, b]| format!("{a}..{b} Unix ms"));
    format!("Window: {w} ({since}); {} samples, {} corrupt skipped; earlier/pruned history unknown\n", h.samples, h.corrupt)
}

/// § 5 `taskq profile tasks`: time in state per state, runtime and host, then the tasks with the most time per state.
fn render_tasks_rows(h:&Header,rows:&[Row]) -> Result<String, &'static str> {
    let none = |v: &Option<String>| v.clone().unwrap_or_else(|| "none".into());
    let mut states: BTreeMap<(String, String, String), (Vec<u64>, usize)> = BTreeMap::new();
    let mut tasks: BTreeMap<(String, u64, String, String, String), (Vec<u64>, usize)> = BTreeMap::new();
    // § 4 Profiling: several services may record one move; one per project, task and entry counts, a known time wins
    let mut unique: BTreeMap<(Option<String>, Option<u64>, Result<u64, usize>), &Row> = BTreeMap::new();
    let moves: Vec<_> = rows.iter().filter(|r| r.kind == "transition").collect();
    for (i, r) in moves.iter().enumerate() {
        let kept = unique.entry((r.project.clone(), r.task, r.entry.ok_or(i))).or_insert(*r);
        if kept.prev_s.is_none() { *kept = *r; }
    }
    for r in unique.values() {
        let (from, rt, host) = (none(&r.from), none(&r.runtime), none(&r.host));
        let project = r.project.clone().unwrap_or_else(|| "unknown (not recorded)".into());
        for g in [states.entry((from.clone(), rt.clone(), host.clone())).or_default(), tasks.entry((project, r.task.unwrap_or(0), from, rt, host)).or_default()] {
            match r.prev_s { Some(s) => g.0.push(s), None => g.1 += 1 }
        }
    }
    let mut out = format!("Time in state: retained queue transitions; moves while no service ran unknown\n{}Transitions: {} unique of {} rows (deduplicated by project, task and entry)\n",
        window(&h), unique.len(), moves.len());
    if states.is_empty() { return Ok(out + "States by time: unknown (no transitions)\n"); }
    // unknown-only groups rank after every known one, never as zero
    let p95 = |s: &[u64]| (s.is_empty(), std::cmp::Reverse(s.get((s.len() * 95).div_ceil(100).max(1) - 1).copied()));
    let mut states: Vec<_> = states.into_iter().map(|(k, (mut s, u))| { s.sort_unstable(); (k, s, u) }).collect();
    states.sort_by(|a, b| p95(&a.1).cmp(&p95(&b.1)).then(a.0.cmp(&b.0)));
    out += "\nStates by p95\n\n| State | Runtime | Host | Count | p50/p95/max s |\n|---|---|---|---:|---|\n";
    for ((state, rt, host), s, u) in &states { out += &format!("| {state} | {rt} | {host} | {} | {} |\n", s.len() + u, in_state(s, *u)); }
    let mut tasks: Vec<_> = tasks.into_iter().map(|(k, (mut s, u))| { s.sort_unstable(); (k, s, u) }).collect();
    let sum = |s: &[u64]| (s.is_empty(), std::cmp::Reverse(s.iter().sum::<u64>()));
    tasks.sort_by(|a, b| sum(&a.1).cmp(&sum(&b.1)).then(a.0.cmp(&b.0)));
    out += "\nTasks by total time in state\n\n| Project | Task | State | Runtime | Host | Count | Total s | p50/p95/max s |\n|---|---|---|---|---|---:|---:|---|\n";
    for ((project, task, state, rt, host), s, u) in tasks.iter().take(20) {
        let total = match (s.is_empty(), *u) {
            (true, _) => "unknown (no entry time)".into(),
            (false, 0) => s.iter().sum::<u64>().to_string(),
            (false, u) => format!("{} partial ({} of {} known)", s.iter().sum::<u64>(), s.len(), s.len() + u),
        };
        out += &format!("| {project} | #{task} | {state} | {rt} | {host} | {} | {total} | {} |\n", s.len() + u, in_state(s, *u));
    }
    if tasks.len() > 20 { out += &format!("{} more task states\n", tasks.len() - 20); }
    Ok(out)
}

/// § 5 `taskq profile board`: board operations ranked by p95, failure rate and lowest rate-limit `remaining`.
fn render_board_rows(h:&Header,rows:&[Row]) -> Result<String, &'static str> {
    let mut ops: BTreeMap<String, (Vec<u64>, BTreeMap<String, usize>, Option<u64>)> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.kind == "board") {
        let o = ops.entry(r.name.clone().unwrap_or_default()).or_default();
        o.0.push(r.us.ok_or("export board row without duration")?);
        if r.outcome.as_deref() == Some("failed") { *o.1.entry(r.error.clone().unwrap_or_default()).or_default() += 1; }
        if let Some(m) = r.remaining { o.2 = Some(o.2.map_or(m, |x| x.min(m))); }
    }
    let mut out = format!("Board operations: retained observations; all budgets unknown\n{}", window(&h));
    if ops.is_empty() { return Ok(out + "Board operations: unknown (no observations)\n"); }
    for o in ops.values_mut() { o.0.sort_unstable(); }
    let failed = |o: &(Vec<u64>, BTreeMap<String, usize>, Option<u64>)| o.1.values().sum::<usize>() as f64 / o.0.len() as f64;
    let rank = |key: &dyn Fn(&(Vec<u64>, BTreeMap<String, usize>, Option<u64>)) -> Option<f64>| {
        let mut r: Vec<_> = ops.iter().filter_map(|(k, o)| key(o).map(|v| (k, v))).collect();
        r.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        if r.is_empty() { "unknown (not reported)".into() } else { r.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(" > ") }
    };
    out += &format!("Slow (p95): {}\nFailing (rate): {}\nRate-limit use (lowest remaining): {}\n\n| Operation | Count | p50/p95/max ms | Failed % | Errors | Lowest remaining |\n|---|---:|---|---:|---|---|\n",
        rank(&|o| Some(o.0[(o.0.len() * 95).div_ceil(100) - 1] as f64)), rank(&|o| Some(failed(o))), rank(&|o| o.2.map(|m| -(m as f64))));
    for (op, o) in &ops {
        let e = o.1.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>().join(", ");
        out += &format!("| {op} | {} | {} | {:.2} | {} | {} |\n", o.0.len(), spread(&o.0, 1000.), failed(o) * 100., if e.is_empty() { "none" } else { &e },
            o.2.map_or("unknown (not reported)".into(), |m| m.to_string()));
    }
    Ok(out)
}

/// `--since 30m|24h|7d` as Unix ms before `now`.
fn since_ms(s: &str, now: u64) -> Result<u64, String> {
    let bad = || format!("--since {s}: use a number with m, h or d, e.g. 24h");
    let mut chars = s.chars();
    let mins: u64 = match chars.next_back() { Some('m') => 1, Some('h') => 60, Some('d') => 1440, _ => return Err(bad()) };
    let n = chars.as_str();
    Ok(now.saturating_sub(n.parse::<u64>().map_err(|_| bad())?.saturating_mul(mins * 60_000)))
}

/// #1345 `profile health`: this CLI's collector, the host service's from its fresh `status.json`, and the shared store.
fn health(install:&Path)->String {
    let now=SystemTime::now().duration_since(UNIX_EPOCH).map_or(0,|d|d.as_secs());
    let statuses=crate::host::projects(install).map(|roots|roots.iter().map(|r|crate::data::json(&crate::dot(r).join("status.json"))).collect::<Vec<_>>());
    let service=statuses.and_then(|s|service_health(&s,now,crate::runtime::alive));
    let mut out=health_lines("CLI telemetry",&Ok((crate::telemetry::diagnostic::health(),std::process::id() as u64,now)));
    out+=&health_lines("Service telemetry",&service);
    out+=&store_line(install);
    // docs/design/worktrees.md: one line per root from its last tidy report; #1766: then its status age
    for root in crate::host::projects(install).unwrap_or_default() {
        let name=root.file_name().map_or("project".into(),|n|n.to_string_lossy().into_owned());
        let tidy=crate::data::json(&crate::dot(&root).join("tidy.json")).ok().flatten().unwrap_or_default();
        out+=&crate::tidy::health_line(&name,&tidy,now);
        out.push('\n');
        let status=crate::data::json(&crate::dot(&root).join("status.json"));
        out+=&status_line(&name,&status,now);
        out.push('\n');
        // § 4 Churn: one line per flagged task of this root
        for l in status.ok().flatten().map(|s|crate::snapshot::churn_lines(&s)).unwrap_or_default() {out+=&format!("{name}: {l}\n");}
    }
    out
}

/// #1766: a project's status.json age against its refresh `poll`; older than two refreshes is stale, read from the file
/// alone, so a project whose passes stopped shows without any operation recorded.
fn status_line(project:&str,status:&Result<Option<serde_json::Value>,String>,now:u64)->String {
    let st=match status {Ok(Some(st))=>st,Ok(None)=>return format!("{project}: status unknown (status.json missing)"),
        Err(e)=>return format!("{project}: status unknown (unreadable: {e})")};
    let Some(at)=st["at"].as_u64() else {return format!("{project}: status unknown (no time)")};
    // a status written before #1766 has no `poll`: the § 9 default
    let poll=st["poll"].as_u64().unwrap_or(60).max(1);
    let age=now.saturating_sub(at);
    format!("{project}: status {} old · refresh {poll}s{}",crate::age(age as i64),if age>2*poll {" · stale"} else {""})
}

/// The freshest status (under 60 s, live pid) among registered projects; anything else is unknown with its reason (R12).
fn service_health(statuses:&[Result<Option<serde_json::Value>,String>],now:u64,alive:impl Fn(u32)->bool)->Result<(serde_json::Value,u64,u64),String> {
    let mut reason=String::from("no registered project");
    let mut best:Option<(serde_json::Value,u64,u64)>=None;
    for status in statuses {
        let st=match status {Ok(Some(st))=>st,Ok(None)=>{reason="service status.json missing".into();continue}Err(e)=>{reason=format!("service status unreadable: {e}");continue}};
        let (Some(at),Some(pid))=(st["at"].as_u64(),st["pid"].as_u64()) else {reason="service status has no pid or time".into();continue};
        if at>now||now-at>60 {reason=format!("service status {} old",crate::age(now.saturating_sub(at) as i64));continue}
        if !alive(pid as u32) {reason=format!("service pid {pid} not running");continue}
        if !st["telemetry"].is_object() {reason="service status has no telemetry snapshot".into();continue}
        if best.as_ref().is_none_or(|b|at>b.2) {best=Some((st["telemetry"].clone(),pid,at));}
    }
    best.ok_or(reason)
}

/// Service status style: one field per line; unknown keeps its reason, never zero.
fn health_lines(label:&str,health:&Result<(serde_json::Value,u64,u64),String>)->String {
    let (h,pid,at)=match health {Ok(v)=>v,Err(reason)=>return format!("{label}: unknown ({reason})\n")};
    let state=match h["state"].as_str() {
        Some("collecting")=>"collecting".to_string(),
        _ if h["reason"].is_string()=>format!("unknown ({})",h["reason"].as_str().unwrap_or_default()),
        _=>"unknown (collector lost records or failed batches)".into(),
    };
    let mut out=format!("{label}: {state} · scope {} · pid {pid} · at {} UTC\n",h["scope"].as_str().unwrap_or("unknown"),crate::board::stamp(*at as i64)[..16].replace('T'," "));
    let n=|k:&str|h[k].as_u64();
    if let (Some(lost),Some(failed)) = (n("lost"),n("failed_batches")) {
        let append=match (n("append_us"),n("append_batches"),n("max_append_us")) {
            (Some(_),Some(0),_)=>"append none".into(),
            (Some(us),Some(b),Some(max))=>format!("append avg {} us · max {max} us",us/b),
            _=>"append unknown".into(),
        };
        // ponytail: CoreQ counts no commit timeouts; a counter there replaces this unknown.
        out+=&format!("{label} counts: lost {lost} · failed batches {failed} · timeouts unknown (not counted) · {append}\n");
    }
    out
}

/// Installation store size against its budget shares; unreadable sizes or limits are unknown.
/// #1640: a read-only snapshot (no writer lock, no checkpoint) adds live WAL bytes, so a high-water WAL file is not read as full.
fn store_line(install:&Path)->String {
    let read=|| -> Result<String,String> {
        let budget=crate::telemetry::budget(install,"telemetry.sqlite3")?;
        let s=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?.storage()?;
        let n=|k:&str|s[k].as_u64().ok_or(format!("storage has no {k}"));
        let (db,wal,live)=(n("database_bytes")?,n("wal_bytes")?+n("shm_bytes")?,n("wal_live_bytes")?);
        let (db_max,wal_max)=(budget.database_bytes()? as u64,budget.wal_bytes()?);
        let mib=|b:u64|format!("{:.1} MiB",b as f64/1048576.0);
        Ok(format!("Telemetry store: database {} of {} · WAL file {} (live {}) of {} · budget {}\n",mib(db),mib(db_max),mib(wal),mib(live),mib(wal_max),
            if db<=db_max&&wal<=wal_max {"within"} else {"over"}))
    };
    read().unwrap_or_else(|e|format!("Telemetry store: unknown ({e})\n"))
}

/// The PM telemetry procedure that `profile watch` serves and `taskq help` links.
pub const WATCH_DOC: &str = "https://github.com/taskqorg/taskq/blob/main/docs/design/profiling.md#pm-telemetry-watch";

/// #1910 one stall line for a `status.json` task row at `now` (Unix s), thresholds of [`WATCH_DOC`]; a row matches its first rule.
pub(crate) fn stall(t:&serde_json::Value,now:u64)->Option<String> {
    let since=|k:&str|t[k].as_u64().map(|s|now.saturating_sub(s));
    let age=|s:u64|crate::age(s as i64);
    let health=if matches!(t["state"].as_str(),Some("doing"|"review")) && t["heartbeat"].is_object() {
        let mut h=t["heartbeat"].clone();crate::snapshot::refresh_heartbeat(&mut h,now as i64,t["attempt"].is_u64());
        h["warning"].as_str().map(String::from)
    } else {None};
    let reason=health.or_else(||match t["state"].as_str()? {
        "doing" if !t["heartbeat"].is_object() => since("updated").filter(|s|*s>3600).map(|s|format!("doing, no board event for {}",age(s))),
        "ready" => since("since").filter(|s|*s>900).map(|s|format!("ready for {}{}",age(s),
            t["admission"]["code"].as_str().map_or(String::new(),|c|format!(" (admission {c})")))),
        "ask" => since("since").filter(|s|*s>1800).map(|s|format!("ask unanswered for {}",age(s))),
        "review" => t["publication"]["since"].as_u64().map(|s|now.saturating_sub(s)).filter(|s|*s>1800).map(|s|format!("merge queue for {}",age(s))),
        "later" => t["until"].as_str().filter(|u|*u<crate::board::stamp(now as i64).as_str()).map(|u|format!("later past its until {u}")),
        _ => None,
    });
    let reason=reason.or_else(|| (t["stuck"]==true).then(|| format!("stuck in {} for {}",t["state"].as_str().unwrap_or("unknown"),age(t["age_s"].as_u64().unwrap_or(0)))))?;
    Some(format!("#{} {reason}",t["n"]))
}

/// #1910 action errors that are new (none in the previous window) or growing against it; counts are [previous, current].
fn action_lines(counts:&BTreeMap<(String,String),[u64;2]>)->Vec<String> {
    counts.iter().filter(|(_,[p,c])|c>p).map(|((op,err),[p,c])|
        format!("{op} {err}: {c}{}",if *p==0 {" (new)".into()} else {format!(" (previous {p})")})).collect()
}

/// #1910 one cadence line from a root's `events` lines and `status.json` rows over `[from, now]` (Unix s).
fn cadence(project:&str,events:&str,tasks:&[serde_json::Value],from:u64)->(String,Vec<String>) {
    let mut closed=0;
    let mut health=Vec::new();
    for l in events.lines() {
        let mut w=l.split_whitespace();
        let (Some(at),Some(n),Some(kind))=(w.next().and_then(|t|t.parse::<u64>().ok()),w.next(),w.next()) else {continue};
        if at<from {continue;}
        match kind {"close"=>closed+=1,"failed"|"gone"=>health.push(format!("{project} {n} {kind}")),_=>{}}
    }
    let new=tasks.iter().filter(|t|t["created"].as_u64().is_some_and(|c|c>=from)).count();
    let net=new as i64-closed as i64;
    (format!("{project}: closed {closed} · new {new} · net {net:+} · open {}",tasks.len()),health)
}

/// #1910 `taskq profile watch [--since W]`: the PM telemetry pass over this install, no board calls ([`WATCH_DOC`]).
pub fn watch(install:&Path,since:Option<&str>)->Result<String,String> {
    let now_ms=now_ms().ok_or("system clock before 1970")?;
    let window=since.unwrap_or("1h");
    let from=since_ms(window,now_ms)?;
    let (now,start)=(now_ms/1000,from/1000);
    let mut out=format!("PM telemetry watch: this install, last {window}; growth against the {window} before; no board calls\nProcedure: {WATCH_DOC}\n");
    let (mut stalls,mut health,mut cadences)=(Vec::new(),Vec::new(),Vec::new());
    for root in crate::host::projects(install).map_err(|e|e.to_string())? {
        let name=root.file_name().map_or("project".into(),|n|n.to_string_lossy().into_owned());
        let status=crate::data::json(&crate::dot(&root).join("status.json"));
        let st=match &status {Ok(Some(st)) if st["at"].as_u64().is_some_and(|at|now.saturating_sub(at)<=2*st["poll"].as_u64().unwrap_or(60).max(1))=>st,
            _=>{stalls.push(status_line(&name,&status,now));continue}};
        let project=st["project"].as_str().unwrap_or(&name).to_owned();
        let tasks=st["tasks"].as_array().cloned().unwrap_or_default();
        stalls.extend(tasks.iter().filter_map(|t|stall(t,now)).map(|s|format!("{project} {s}")));
        let events=fs::read_to_string(crate::dot(&root).join("events")).unwrap_or_default();
        let (line,h)=cadence(&project,&events,&tasks,start);
        cadences.push(line);health.extend(h);
    }
    let list=|title:&str,v:&[String]|if v.is_empty() {format!("{title}: none\n")} else {format!("{title}:\n{}",v.iter().map(|l|format!("- {l}\n")).collect::<String>())};
    out+=&list("Stalls",&stalls);
    out+=&match crate::telemetry::diagnostic::action_errors(install,from.saturating_sub(now_ms-from),from) {
        Ok(counts)=>list("Action errors, new or growing",&action_lines(&counts)),
        Err(e)=>format!("Action errors: unknown ({e})\n"),
    };
    let mut projection=Projection::new(Some(from),Some("board"));
    out+=&match crate::telemetry::visit_samples(install,&["board"],|lane,sample|projection.push(lane,sample)) {
        Ok(())=>board_watch(&projection.finish().1),
        Err(e)=>format!("Board: unknown ({e})\n"),
    };
    out+=&list("Failed or gone",&health);
    out+=&list(&format!("Cadence per board, last {window} (new: open tasks created in the window)"),&cadences);
    Ok(out)
}

/// #1910 board calls in the window: totals, then each operation over 1 % failed, and a lowest `remaining` under 500.
fn board_watch(rows:&[Row])->String {
    let mut ops:BTreeMap<&str,(u64,u64)>=BTreeMap::new();
    let mut low:Option<u64>=None;
    for r in rows.iter().filter(|r|r.kind=="board") {
        let o=ops.entry(r.name.as_deref().unwrap_or("unknown")).or_default();
        o.0+=1;
        if r.outcome.as_deref()==Some("failed") {o.1+=1;}
        if let Some(m)=r.remaining {low=Some(low.map_or(m,|l|l.min(m)));}
    }
    let (calls,failed)=ops.values().fold((0,0),|a,o|(a.0+o.0,a.1+o.1));
    if calls==0 {return "Board: unknown (no board calls recorded in the window)\n".into();}
    let pct=|f:u64,n:u64|f as f64*100./n as f64;
    let mut out=format!("Board: {calls} calls · failed {failed} ({:.2}%) · lowest remaining {}\n",pct(failed,calls),low.map_or("unknown (not reported)".into(),|l|l.to_string()));
    for (op,(n,f)) in ops.iter().filter(|(_,(n,f))|pct(*f,*n)>1.) {out+=&format!("- {op} failed {f} of {n} ({:.2}%)\n",pct(*f,*n));}
    if low.is_some_and(|l|l<500) {out+="- rate limit: remaining under 500\n";}
    out
}

/// § 5 `taskq profile export|tasks|board [--since D]` over this host's retained log; no board access.
pub fn view(install: &Path, name: &str, since: Option<&str>) -> Result<String, String> {
    if name == "guide" {
        if since.is_some() { return Err("profile guide does not accept --since".into()); }
        return Ok(include_str!("../docs/design/profile-guide.md").into());
    }
    if name=="health" {
        if since.is_some() {return Err("profile health does not accept --since".into());}
        return Ok(health(install));
    }
    if name=="watch" {return watch(install,since);}
    if !["export", "tasks", "board", "actions"].contains(&name) { return Err(format!("unknown profile view: {name} (guide, export, tasks, board, actions, health or watch)")); }
    let now = now_ms().ok_or("system clock before 1970")?;
    let since = since.map(|s| since_ms(s, now)).transpose()?;
    if name=="export" {return crate::telemetry::export(install,since);}
    if name=="actions" {return crate::telemetry::diagnostic::actions(install,since);}
    let mut projection=Projection::new(since,Some(if name=="tasks" {"transition"}else{"board"}));
    crate::telemetry::visit_samples(install,&["operation","stage","board","transition"],|lane,sample|projection.push(lane,sample))?;
    let (header,rows)=projection.finish();
    match name {
        "tasks" => render_tasks_rows(&header,&rows).map_err(String::from),
        "board" => render_board_rows(&header,&rows).map_err(String::from),
        _ => Err(format!("unknown profile view: {name} (guide, export, tasks, board, actions, health or watch)")),
    }
}

#[cfg(test)]
fn render(text:&str)->Result<String,&'static str> {render_summary(&Summary::fixture(text))}
#[cfg(test)]
fn render_stages(text:&str)->Result<String,&'static str> {render_stages_samples(&Sample::archive(text))}
#[cfg(test)]
fn render_system(text:&str)->(String,String) {render_system_summary(&Summary::fixture(text))}
#[cfg(test)]
fn unresolved(text:&str)->Result<Vec<Stage>,String> {unresolved_samples(&Sample::archive(text))}
#[cfg(test)]
fn defects(text:&str,live:&Live,project:Option<&str>,now:u64,ttl:u64)->Result<(String,usize),String> {defects_samples(&Sample::archive(text),live,project,now,ttl)}
#[cfg(test)]
fn render_tasks(text:&str)->Result<String,&'static str> {let (h,rows)=read_export(text)?;render_tasks_rows(&h,&rows)}
#[cfg(test)]
fn render_board(text:&str)->Result<String,&'static str> {let (h,rows)=read_export(text)?;render_board_rows(&h,&rows)}

#[cfg(test)]
#[path = "profile_tests.rs"]
pub(crate) mod tests;
