//! Installation-local telemetry. All storage mutations share the installation budget lock; export is read-only.
mod semantic;
pub(crate) use semantic::Sample;
pub(crate) mod diagnostic;

use std::path::Path;

const LINE_MAX: usize = 64 << 10;

/// 1 GiB installation budget, half held in reserve; the peer share counts the service log and a retired archive left on disk.
pub(crate) fn budget(install: &Path, peer: &str) -> Result<coreq::telemetry::Budget, String> {
    let install = std::path::absolute(install).map_err(|e| e.to_string())?;
    let dir = install.join("telemetry.log");
    Ok(coreq::telemetry::Budget {
        lock: install.join("taskq-profile.lock"),
        peers: vec![
            dir.join(peer),
            dir.join(format!("{peer}-wal")),
            dir.join(format!("{peer}-shm")),
            install.join("taskq.log"),
        ],
        total: coreq::telemetry::INSTALLATION_BYTES,
        reserved: coreq::telemetry::PEER_BYTES,
    })
}

/// #1351: receipts are mandatory but never block; the id is deterministic, so callers keep it without waiting for the ack.
/// A slow or failed commit leaves that phase unknown in the host log; the operation proceeds.
pub(crate) fn receipt(
    command: &coreq::telemetry::capture::CommandReceipt,
    phase: &str,
) -> Result<String, String> {
    let id = command.event_id(phase)?;
    // A missing collector is known at once, so its existing refusal policy stays synchronous.
    if !tracing::dispatcher::get_default(|d| {
        d.downcast_ref::<coreq::telemetry::capture::Capture>()
            .is_some()
    }) {
        return Err("command receipt collector unavailable".into());
    }
    let job: Receipt = (
        command.clone(),
        phase.to_owned(),
        tracing::dispatcher::get_default(Clone::clone),
    );
    let (pending, _) = &RECEIPTS;
    *pending.lock().unwrap() += 1;
    if RECEIPT_QUEUE
        .lock()
        .unwrap()
        .get_or_insert_with(receipt_worker)
        .send(job)
        .is_err()
    {
        settled(1);
        crate::platform::log(&format!(
            "command receipt {phase} unknown: receipt worker unavailable"
        ));
    }
    Ok(id)
}

type Receipt = (
    coreq::telemetry::capture::CommandReceipt,
    String,
    tracing::Dispatch,
);
static RECEIPTS: (std::sync::Mutex<usize>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());
static RECEIPT_QUEUE: std::sync::Mutex<Option<std::sync::mpsc::Sender<Receipt>>> =
    std::sync::Mutex::new(None);

// ponytail: one FIFO worker keeps phase order; a stalled store delays later receipts by coreq's append deadline each, never callers.
// A receipt still queued at process exit stays unknown.
fn receipt_worker() -> std::sync::mpsc::Sender<Receipt> {
    let (send, jobs) = std::sync::mpsc::channel::<Receipt>();
    std::thread::Builder::new()
        .name("taskq-receipts".into())
        .spawn(move || {
            for (command, phase, dispatch) in jobs {
                tracing::dispatcher::with_default(&dispatch, || {
                    if let Err(error) = command.checkpoint(&phase) {
                        crate::platform::log(&format!("command receipt {phase} unknown: {error}"));
                    }
                });
                drop(dispatch); // Settlement also releases the collector's SQLite handles.
                settled(1);
            }
        })
        .expect("receipt worker");
    send
}

fn settled(n: usize) {
    let (pending, done) = &RECEIPTS;
    *pending.lock().unwrap() -= n;
    done.notify_all();
}

/// Waits until submitted receipts have committed or failed; readers of this process's receipts call it first.
pub(crate) fn settle(limit: std::time::Duration) -> bool {
    let (pending, done) = &RECEIPTS;
    !done
        .wait_timeout_while(pending.lock().unwrap(), limit, |n| *n > 0)
        .unwrap()
        .1
        .timed_out()
}

pub(crate) fn install_capture(
    install: &Path,
) -> Result<coreq::telemetry::capture::Runtime, String> {
    coreq::telemetry::capture::Runtime::install_bounded(
        &install.join("telemetry.log/universal.sqlite3"),
        "taskq",
        crate::platform::VERSION,
        budget(install, "telemetry.sqlite3")?,
    )
}

/// Called only after the host has acquired its installation owner lock.
pub(crate) fn host_capture(
    install: &Path,
) -> Result<Option<coreq::telemetry::capture::Runtime>, String> {
    if tracing::dispatcher::get_default(|d| {
        d.downcast_ref::<coreq::telemetry::capture::Capture>()
            .is_some()
    }) {
        return Ok(None);
    }
    let mut error = match install_capture(install) {
        Ok(runtime) => return Ok(Some(runtime)),
        Err(error) => error,
    };
    let recoverable = |error: &str| {
        matches!(
            error,
            "telemetry IPC unsupported wire version; quiesce the old writer"
                | "telemetry IPC installation budget mismatch; quiesce incompatible writer"
                | "telemetry IPC writer election unavailable"
        )
    };
    if !recoverable(&error) {
        return Err(error);
    }
    let heartbeat = install.join("tray");
    let previous =
        std::fs::read_to_string(&heartbeat).map_err(|_| format!("{error}; tray owner unknown"))?;
    let pid = previous
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{error}; tray owner unknown"))?;
    if pid == std::process::id() || !coreq::status::age(&heartbeat).is_some_and(|age| age < 10) {
        return Err(format!("{error}; tray owner not fresh"));
    }
    let identity = coreq::process::identity(pid)
        .map_err(|_| format!("{error}; tray identity unknown"))?
        .ok_or_else(|| format!("{error}; tray already exited"))?;
    if std::fs::read_to_string(&heartbeat).ok().as_ref() != Some(&previous)
        || coreq::process::identity(pid).ok().flatten().as_ref() != Some(&identity)
    {
        return Err(format!("{error}; tray owner changed"));
    }
    // Older trays already leave when a different PID owns their fresh heartbeat (tray::alone).
    crate::platform::write_atomic(&heartbeat, &std::process::id().to_string())?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match install_capture(install) {
            Ok(runtime) => return Ok(Some(runtime)),
            Err(next) => {
                error = next;
                if !recoverable(&error) || std::time::Instant::now() >= until {
                    return Err(error);
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// #1336: a host without a collector retries `host_capture` each pass, so a tray that becomes fresh
/// later still hands over its writer; `pass` returns one log line per state change.
#[derive(Default)]
pub(crate) struct HostCapture {
    runtime: Option<coreq::telemetry::capture::Runtime>,
    state: String,
}
impl HostCapture {
    pub(crate) fn pass(&mut self, install: &Path) -> Option<String> {
        if self.runtime.is_some() {
            return None;
        }
        let state = match host_capture(install) {
            Ok(runtime) => {
                self.runtime = runtime;
                "Telemetry: collecting".to_string()
            }
            Err(error) => format!("Telemetry: unknown ({error}); retrying each pass"),
        };
        (state != self.state).then(|| {
            self.state.clone_from(&state);
            state
        })
    }
}

/// #1755: the tray has no service pass, so its own thread (never the UI loop) retries a failed install, and a
/// collector that lost batches says when commits resume; `log` gets one line per state change.
pub(crate) fn keep_capture(
    install: std::path::PathBuf,
    every: std::time::Duration,
    log: impl Fn(&str) + Send + 'static,
) {
    use coreq::telemetry::capture::Capture;
    std::thread::spawn(move || {
        // Holds a late collector for the process lifetime; the exit path drains it like the startup one.
        let mut _runtime = None;
        let mut seen = (0, 0);
        let mut state =
            if tracing::dispatcher::get_default(|d| d.downcast_ref::<Capture>().is_some()) {
                "Telemetry: collecting".to_string()
            } else {
                String::new()
            };
        loop {
            let next = match tracing::dispatcher::get_default(|d| {
                d.downcast_ref::<Capture>().map(Capture::health)
            }) {
                Some(health) => {
                    let now = (
                        health["lost"].as_u64().unwrap_or(0),
                        health["committed"].as_u64().unwrap_or(0),
                    );
                    let next = capture_state(seen, now, &state);
                    seen = now;
                    next
                }
                None => match install_capture(&install) {
                    Ok(runtime) => {
                        _runtime = Some(runtime);
                        "Telemetry: collecting".to_string()
                    }
                    Err(error) => format!(
                        "Telemetry: unknown ({error}); retrying every {}s",
                        every.as_secs().max(1)
                    ),
                },
            };
            if next != state {
                log(&next);
                state = next;
            }
            std::thread::sleep(every);
        }
    });
}

/// Health counts are cumulative: a new loss makes the state unknown until a later commit lands.
fn capture_state(seen: (u64, u64), now: (u64, u64), state: &str) -> String {
    if now.0 > seen.0 {
        "Telemetry: unknown (observations lost); collector retries".to_string()
    } else if now.1 > seen.1 {
        "Telemetry: collecting".to_string()
    } else {
        state.to_string()
    }
}

/// Live producers submit typed records directly.
pub(crate) fn record(sample: Sample) -> std::io::Result<()> {
    let prepare = || -> Result<serde_json::Value, String> {
        let kind = sample.kind();
        let original = sample.canonical()?;
        if semantic::encoded_size(&original)? > LINE_MAX {
            return Err("telemetry append exceeds 64 KiB".into());
        }
        let mut value = semantic::encode(kind, original)?;
        // A failed claim has no attempt; zero remains only the legacy report's in-memory sentinel.
        if kind == "stage" && value["attempt"] == 0 {
            value["attempt"] = serde_json::Value::Null;
            value["unknown"]["attempt"] = serde_json::json!("no-attempt");
        }
        Ok(value)
    };
    prepare()
        .map(|value| coreq::telemetry::shared(&value))
        .and_then(coreq::telemetry::capture::observe)
        .map_err(std::io::Error::other)
}

pub(crate) fn visit_samples(
    install: &Path,
    kinds: &[&str],
    accept: impl FnMut(usize, Sample),
) -> Result<(), String> {
    // Include this process's admitted records before taking the committed snapshot.
    tracing::dispatcher::get_default(|d| {
        d.downcast_ref::<coreq::telemetry::capture::Capture>()
            .cloned()
    })
    .map(|capture| capture.flush())
    .transpose()?;
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        let path = install.join("telemetry.log/universal.sqlite3");
        let reader = path
            .try_exists()
            .map_err(|e| e.to_string())?
            .then(|| coreq::telemetry::Reader::open(&path))
            .transpose()?;
        semantic::visit_samples(reader.as_ref(), kinds, accept)
    })
}

pub(crate) fn report_samples(install: &Path, kinds: &[&str]) -> Result<Vec<Sample>, String> {
    let mut lanes: [Vec<Sample>; 2] = Default::default();
    visit_samples(install, kinds, |lane, sample| lanes[lane].push(sample))?;
    Ok(lanes.into_iter().flatten().collect())
}

/// Read committed metadata without acquiring write permission, retention or network access.
#[tracing::instrument(name = "telemetry.export", skip_all)]
pub fn export(install: &Path, since: Option<u64>) -> Result<String, String> {
    // Record the export boundary, not a recursive trace for each decoded telemetry row.
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        export_snapshot(install, since)
    })
}

fn export_snapshot(install: &Path, since: Option<u64>) -> Result<String, String> {
    use serde_json::json;
    let reader = coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
    let since_i64 = since.unwrap_or(0).min(i64::MAX as u64) as i64;
    let mut page = reader.export(since_i64, 0, 1024)?;
    let mut universal = page.clone();
    for key in ["events", "after", "next", "more", "schema"] {
        universal.as_object_mut().unwrap().remove(key);
    }
    universal["state"] = json!("read");
    let mut out = String::new();
    let mut samples = 0;
    loop {
        for row in page["events"]
            .as_array()
            .ok_or("invalid telemetry export page")?
        {
            let mut row = row.clone();
            row["source"] = json!("universal");
            out.push_str(&row.to_string());
            out.push('\n');
            samples += 1;
        }
        if page["more"] == false {
            break;
        }
        page = reader.export(
            since_i64,
            page["next"]
                .as_i64()
                .ok_or("invalid telemetry export cursor")?,
            1024,
        )?;
    }
    universal["samples"] = json!(samples);
    let header = json!({"schema":2,"coverage":"committed-snapshot","since_ms":since,"sources":{"universal":universal}});
    Ok(header.to_string() + "\n" + &out)
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkerEvent {
    pub at_ms: u64,
    pub project: String,
    pub host: String,
    pub runtime: String,
    pub role: String,
    pub event: String,
    pub task: u64,
    pub attempt: u64,
    pub pid: u32,
    pub outcome: String,
    pub reason: String,
}

fn code_token(s: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-') {
            out.push(ch);
            prev_dash = ch == '-';
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 128 {
            break;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn project_token(s: &str) -> String {
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 128
            && part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
            && !matches!(part, "." | "..")
    };
    if s.len() <= 128 && s.contains('/') && s.split('/').all(valid_part) {
        s.to_string()
    } else {
        "other".to_string()
    }
}

pub fn worker_observation(ev: &WorkerEvent) -> serde_json::Value {
    let outcome = match ev.outcome.as_str() {
        "ok" | "failed" | "refused" | "cancelled" | "retried" | "unknown" | "ask" => {
            ev.outcome.as_str()
        }
        _ => "unknown",
    };
    let event = code_token(&ev.event, "other");
    let reason = code_token(&ev.reason, "none");
    let error = if outcome == "ok" {
        "none".to_string()
    } else {
        reason.clone()
    };
    serde_json::json!({
        "product": "taskq",
        "version": null,
        "kind": "worker",
        "operation": event,
        "trace": null,
        "span": null,
        "started_ms": ev.at_ms.min(i64::MAX as u64),
        "project": project_token(&ev.project),
        "host": code_token(&ev.host, "unknown"),
        "runtime": code_token(&ev.runtime, "other"),
        "task": ev.task,
        "attempt": ev.attempt,
        "process_id": ev.pid,
        "source": code_token(&ev.role, "work"),
        "from": event,
        "to": reason,
        "outcome": outcome,
        "error": error,
        "unknown": {
            "version": "not-recorded",
            "trace": "not-recorded",
            "span": "not-recorded"
        }
    })
}

fn decode_worker_observation(v: &serde_json::Value) -> Option<WorkerEvent> {
    if v["kind"].as_str() != Some("worker") {
        return None;
    }
    Some(WorkerEvent {
        at_ms: v["started_ms"].as_u64().unwrap_or(0),
        project: v["project"].as_str().unwrap_or("other").to_string(),
        host: v["host"].as_str().unwrap_or("unknown").to_string(),
        runtime: v["runtime"].as_str().unwrap_or("other").to_string(),
        role: v["source"].as_str().unwrap_or("work").to_string(),
        event: v["operation"].as_str().unwrap_or("other").to_string(),
        task: v["task"].as_u64().unwrap_or(0),
        attempt: v["attempt"].as_u64().unwrap_or(0),
        pid: v["process_id"].as_u64().unwrap_or(0) as u32,
        outcome: v["outcome"].as_str().unwrap_or("ok").to_string(),
        reason: v["to"].as_str().unwrap_or("none").to_string(),
    })
}

static WORKER_EVENTS: std::sync::Mutex<
    std::collections::BTreeMap<std::path::PathBuf, Vec<WorkerEvent>>,
> = std::sync::Mutex::new(std::collections::BTreeMap::new());

#[allow(clippy::too_many_arguments)]
pub fn record_worker(
    install: &Path,
    project: &str,
    host: &str,
    runtime: &str,
    role: &str,
    event: &str,
    task: u64,
    attempt: u64,
    pid: u32,
    outcome: &str,
    reason: &str,
) {
    let at_ms = crate::profile::now_ms().unwrap_or(0);
    let raw = WorkerEvent {
        at_ms,
        project: project.to_string(),
        host: host.to_string(),
        runtime: runtime.to_string(),
        role: role.to_string(),
        event: event.to_string(),
        task,
        attempt,
        pid,
        outcome: outcome.to_string(),
        reason: reason.to_string(),
    };
    let obs = worker_observation(&raw);
    let normalized = decode_worker_observation(&obs).unwrap_or(raw);
    if let Ok(mut map) = WORKER_EVENTS.lock() {
        let list = map.entry(install.to_path_buf()).or_default();
        if list.len() >= 1024 {
            list.drain(..512);
        }
        list.push(normalized);
    }
    let _ = coreq::telemetry::capture::observe(obs);
}

pub fn worker_events(install: &Path) -> Vec<WorkerEvent> {
    let _ = tracing::dispatcher::get_default(|d| {
        d.downcast_ref::<coreq::telemetry::capture::Capture>()
            .cloned()
    })
    .map(|capture| capture.flush());
    let mut out = Vec::new();
    let path = install.join("telemetry.log/universal.sqlite3");
    if path.try_exists().unwrap_or(false) {
        if let Ok(reader) =
            tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
                coreq::telemetry::Reader::open(&path)
            })
        {
            let mut after = 0;
            while let Ok(page) = reader.export_kinds(&["worker"], 0, after, 512) {
                let Some(rows) = page["events"].as_array() else {
                    break;
                };
                if rows.is_empty() {
                    break;
                }
                for row in rows {
                    if let Some(ev) = decode_worker_observation(&row["observation"]) {
                        out.push(ev);
                    }
                }
                if page["more"] == false {
                    break;
                }
                let Some(next) = page["next"].as_i64() else {
                    break;
                };
                after = next;
            }
        }
    }
    if let Ok(map) = WORKER_EVENTS.lock() {
        if let Some(mem) = map.get(install) {
            for ev in mem {
                if !out.contains(ev) {
                    out.push(ev.clone());
                }
            }
        }
    }
    out
}

/// Live worker pid for `(task, attempt, execution)` from telemetry or `.kids`; returns `Some(pid)` while the OS process lives (#2237).
pub fn live_worker_pid(
    install: &Path,
    root: Option<&Path>,
    task: u64,
    attempt: u64,
    execution: u64,
) -> Option<u32> {
    let check_events = |events: &[WorkerEvent]| -> Option<Option<u32>> {
        let last = events.iter().rev().find(|e| {
            e.task == task
                && (attempt == 0
                    || e.attempt == 0
                    || e.attempt == attempt
                    || e.attempt == execution)
                && matches!(
                    e.event.as_str(),
                    "spawn"
                        | "heartbeat"
                        | "slot-hold"
                        | "result"
                        | "alive-after-result"
                        | "exit"
                        | "slot-release"
                        | "kill"
                )
        })?;
        if matches!(last.event.as_str(), "exit" | "slot-release") {
            return Some(None);
        }
        if last.pid > 1 && crate::runtime::alive(last.pid) {
            return Some(Some(last.pid));
        }
        None
    };
    if let Ok(map) = WORKER_EVENTS.lock() {
        if let Some(mem) = map.get(install) {
            if let Some(res) = check_events(mem) {
                return res;
            }
        }
    }
    if let Some(r) = root {
        if let Ok(path) = crate::paths::kids_path(install, r) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok((_, kids)) = crate::data::kids(&text) {
                    if let Some(k) = kids.into_iter().find(|k| {
                        k.task == task
                            && (attempt == 0
                                || k.attempt == attempt
                                || k.execution == execution
                                || k.execution == attempt)
                    }) {
                        if k.process.pid > 1 && crate::runtime::alive(k.process.pid) {
                            return Some(k.process.pid);
                        }
                    }
                }
            }
        }
    }
    let db_events = worker_events(install);
    if let Some(res) = check_events(&db_events) {
        return res;
    }
    None
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
