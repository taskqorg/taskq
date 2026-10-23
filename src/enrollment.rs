//! DashQ observation connection (docs/design/enrollment.md): coreq pairing and delivery of the board projection.
//! No command authority or task-state storage.

use crate::{Res, core, resources, snapshot::Snapshot};
use coreq::enroll::{self, Backoff, Delivery, Https};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io::{Read, Write}, path::{Path, PathBuf}, thread::JoinHandle};

pub const SCHEMA: &str = "taskq-board/1";
/// The `taskq.resources/1` projection as a DashQ record (shared-resource-controls.md#project-overlay); DashQ names
/// record schemas `<product>-<name>/<n>` and accepts only those its grant lists.
pub const RESOURCES: &str = "taskq-resources/1";
/// DashQ C4 metadata: at most 256 array items.
const MAX_TASKS: usize = 256;
/// An unchanged projection is resent after this many seconds, so DashQ's observation time stays fresh.
const RESEND: u64 = 300;

/// Protected coreq connection state; `platform::sweep` and data pruning keep it.
pub fn dir(install: &Path) -> PathBuf {
    install.join("connection")
}

/// DashQ serves machine routes under `/machine/`; its page prints the bare origin.
pub fn destination(url: &str) -> Res<String> {
    let u = url.trim_end_matches('/');
    if !u.starts_with("https://") || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("not an https URL: {url}"));
    }
    Ok(if u.ends_with("/machine") { u.into() } else { format!("{u}/machine") })
}

/// R6 3: the DashQ progress page of an off-host session, as a prefix the task number completes (DashQ's board task
/// row anchor); `None` without an active connection.
pub fn progress_page(install: &Path, repo: &str) -> Option<String> {
    let dest = active(install).ok()??["destination"].as_str()?.to_string();
    Some(format!("{}/projects#board-{}-task-", dest.trim_end_matches("/machine"), repo.replace('/', "%2F")))
}

/// The active connection, read without creating state (coreq's first read writes a new file).
pub fn active(install: &Path) -> Res<Option<Value>> {
    if state(install)?.is_none() { return Ok(None) }
    enroll::active(&dir(install))
}

/// Connection file for `connection status`; never printed whole (it holds the credential). A file that is not
/// coreq's state shape reads unknown and is kept, never as "no connection".
pub fn state(install: &Path) -> Res<Option<Value>> {
    let bad = || "connection: unknown (connection state unreadable; file kept)".to_string();
    let s: Value = match fs::read_to_string(dir(install).join("connection.json")) {
        Ok(t) => serde_json::from_str(&t).map_err(|_| bad())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("connection: unknown (connection state: {e})")),
    };
    let text = |v: &Value, k: &str| v[k].as_str().is_some_and(|t| !t.is_empty());
    let conn = |v: &Value, keys: &[&str]| v.is_null() || (v.is_object() && keys.iter().all(|k| text(v, k)));
    let valid = s.is_object() && text(&s, "installation")
        && conn(&s["pending"], &["destination"]) && conn(&s["pending"]["staged"], &["destination", "credential"])
        && conn(&s["active"], &["destination", "credential"])
        && s["revoke"].as_array().is_some_and(|r| r.iter().all(|v| v.is_object() && conn(v, &["destination", "credential"])));
    if valid { Ok(Some(s)) } else { Err(bad()) }
}

/// `taskq enroll`: browser or headless approval of this board's scope, then the first snapshot until DashQ
/// projects it. Capabilities are none: the grant can neither answer tasks nor run commands.
/// `telemetry` adds the opt-in [`crate::telemetry_sync::SCHEMA`]; a changed schema list is a new approval.
pub fn pair(install: &Path, url: &str, host: &str, board: &BoardScope, browser: bool, telemetry: bool) -> Res<Value> {
    let dest = destination(url)?;
    let id = installation_id(install)?;
    let value = board.value();
    let schemas: &[&str] = if telemetry { &[SCHEMA, RESOURCES, crate::telemetry_sync::SCHEMA] } else { &[SCHEMA, RESOURCES] };
    let req = enroll::Request { installation: Some(&id), product: "taskq", host_label: host, schemas, scope: Some((SCHEMA, &value)), capabilities: &[] };
    enroll::pair(&dir(install), &dest, &req, &mut resources::Fenced::new(install, Https { browser }))
}

/// Sends `record` once and asks for its receipt until DashQ projects it (or `tries` run out).
pub fn first_receipt(install: &Path, record: &Value, tries: u32) -> Res<&'static str> {
    let d = dir(install);
    let mut w = resources::Fenced::new(install, Https { browser: false });
    match enroll::deliver(&d, std::slice::from_ref(record), &mut Backoff::default(), &mut w) {
        Ok(r) if r[0]["state"] == "accepted" => {}
        Ok(r) => return Err(format!("first snapshot rejected: {}", r[0]["reason"].as_str().unwrap_or("unknown"))),
        Err(Delivery::Retry(_)) => return Err("first snapshot not delivered; the service retries".into()),
        Err(Delivery::Stopped(e)) => return Err(format!("first snapshot not delivered: {e}")),
    }
    let mut state = "accepted";
    for i in 0..tries {
        state = enroll::receipt(&d, record, &mut w)?;
        if state == "projected" { break }
        if i + 1 < tries { std::thread::sleep(std::time::Duration::from_secs(2)) }
    }
    Ok(state)
}

/// `taskq disconnect`: cancels a pairing in progress (so `enroll` cannot resume it), stops sending at once and
/// reports what was revoked; a revoke DashQ has not confirmed stays queued for the service, and a repeated
/// disconnect retries and reports it instead of "connection: none".
pub fn disconnect(install: &Path) -> Res<Option<String>> {
    disconnect_with(install, &mut Https { browser: false })
}

pub(crate) fn disconnect_with(install: &Path, w: &mut dyn enroll::Wire) -> Res<Option<String>> {
    let Some(s) = state(install)? else { return Ok(None) };
    let (active, pending) = (s["active"].is_object(), s["pending"].is_object());
    let queued = |e: &String| e.ends_with("revoke pending");
    if !active && !pending {
        if s["revoke"].as_array().is_none_or(Vec::is_empty) { return Ok(None) }
        enroll::retry_revokes(&dir(install), w).or_else(|e| if queued(&e) { Ok(()) } else { Err(e) })?;
        let left = state(install)?.map_or(0, |s| s["revoke"].as_array().map_or(0, Vec::len));
        return Ok(Some(if left > 0 { format!("connection: none; {left} revoke pending, the service retries it") }
            else { "connection: none; revoke confirmed".into() }));
    }
    if pending {
        // a staged credential DashQ already granted is queued for revoke by coreq
        enroll::cancel(&dir(install), w).or_else(|e| if queued(&e) { Ok(()) } else { Err(e) })?;
    }
    resources::locked(install, || enroll::disconnect(&dir(install), w).or_else(|e| if queued(&e) { Ok(()) } else { Err(e) }))?;
    let left = state(install)?.map_or(0, |s| s["revoke"].as_array().map_or(0, Vec::len));
    let mut out = vec![];
    if pending { out.push("pairing cancelled".to_string()) }
    if active { out.push("sending stopped".into()) }
    out.push(if left > 0 { format!("{left} revoke pending, the service retries it") }
        else if active || s["pending"]["staged"].is_object() || s["revoke"].as_array().is_some_and(|r| !r.is_empty()) { "revoke confirmed".into() }
        else { "nothing granted to revoke".into() });
    Ok(Some(format!("disconnected: {}", out.join("; "))))
}

/// The approved board of an active connection; `None` for a scope this version cannot read.
pub fn approved(active: &Value) -> Option<BoardScope> {
    let v = active["scope"]["value"].as_str().filter(|_| active["scope"]["name"] == SCHEMA)?;
    let (kind, rest) = v.split_once(':')?;
    let (host, repo) = rest.split_once('/')?;
    BoardScope::new(kind, host, repo).ok().filter(|b| b.value() == v)
}

/// Whether the service holding `snap` is the host's one DashQ speaker: its board is the connection's approved one.
pub fn speaks(active: &Value, snap: Option<&Snapshot>) -> bool {
    snap.is_some_and(|s| approved(active).is_some_and(|b| BoardScope::snapshot(s).is_ok_and(|x| x == b)))
}

/// The record of one snapshot: payload without `observed` names its content, so equal content dedupes by digest.
pub fn record(install: &Path, s: &Snapshot, active: &Value, local: &Local) -> Res<(String, Value)> {
    let (digest, at, payload) = payload(s, active, local)?;
    Ok((digest.clone(), stamp(install, &digest, at, payload)?))
}

/// The board payload of [`record`]: pure, so a service pass builds it without touching connection storage.
fn payload(s: &Snapshot, active: &Value, local: &Local) -> Res<(String, u64, Value)> {
    let board = approved(active).ok_or("connection has no readable board scope")?;
    let id = active["installation"].as_str().unwrap_or_default();
    let mut payload = serde_json::to_value(project(s, id, &[board], local)?).map_err(|e| e.to_string())?;
    let observed = payload.as_object_mut().unwrap().remove("observed");
    let digest = enroll::digest(&payload);
    payload["observed"] = observed.unwrap_or_default();
    Ok((digest, u64::try_from(s.observed).unwrap_or(0), payload))
}

/// coreq loads (and on Windows re-protects through `whoami` and `icacls`) the connection state to bind the grant.
fn stamp(install: &Path, digest: &str, at: u64, payload: Value) -> Res<Value> {
    enroll::stamp(&dir(install), SCHEMA, &format!("{at}-{}", &digest[..16]), at, payload)
}

/// The resources record, only when the grant lists [`RESOURCES`] (a connection paired before it needs `taskq enroll`
/// again). DashQ's metadata allowlist nests objects at most four deep, so each blocker drops its `scope`: the
/// projection always scopes it to its own target. Payload `observed_at` keeps the status readback time.
pub fn resources_record(install: &Path, active: &Value, now: u64) -> Res<(String, Value)> {
    if !active["schemas"].as_array().is_some_and(|a| a.iter().any(|s| s == RESOURCES)) {
        return Err(format!("grant lacks {RESOURCES}; rerun taskq enroll"));
    }
    let mut p = resources::projection(install, now, false)?;
    for b in p["targets"].as_array_mut().into_iter().flatten().filter_map(|t| t["blockers"].as_array_mut()).flatten() {
        b.as_object_mut().map(|o| o.remove("scope"));
    }
    let observed = p.as_object_mut().unwrap().remove("observed_at");
    let digest = enroll::digest(&p);
    p["observed_at"] = observed.unwrap_or_default();
    let r = enroll::stamp(&dir(install), RESOURCES, &format!("{now}-{}", &digest[..16]), now, p)?;
    Ok((digest, r))
}

/// Backoff, content digest (empty for a revoke retry) and outcome; `Ok(None)`: unchanged and sent recently.
type Job = JoinHandle<(Backoff, String, Result<Option<String>, Delivery>)>;

/// The service's sender: one delivery at a time on its own thread, never blocking a pass; each try sends the
/// latest snapshot (an older unsent one is dropped); a failure only waits (docs/design/enrollment.md).
#[derive(Default)]
pub struct Sender {
    /// Start time, thread.
    job: Option<(u64, Job)>,
    backoff: Backoff,
    wait_until: u64,
    sent: Option<(String, u64)>,
    /// Last outcome for `status.json` `connection`.
    pub outcome: Option<String>,
}

impl Sender {
    /// One step per service pass; returns a log line when the outcome changes.
    pub fn tick(&mut self, install: &Path, snap: Option<&Snapshot>, local: &Local, now: u64) -> Option<String> {
        let before = self.outcome.clone();
        if self.job.as_ref().is_some_and(|j| j.1.is_finished()) {
            let (started, job) = self.job.take().unwrap();
            let (digest, r) = job.join().map(|(b, d, r)| { self.backoff = b; (d, r) }).unwrap_or_else(|_| (String::new(), Err(Delivery::Retry(60))));
            match r {
                Ok(None) => {}
                Ok(Some(state)) => {
                    if digest.is_empty() { self.wait_until = now + RESEND } else { self.sent = Some((digest, started)) }
                    self.outcome = Some(state);
                }
                Err(Delivery::Retry(secs)) => { self.wait_until = now + secs; self.outcome = Some(format!("retry in {secs} s")) }
                Err(Delivery::Stopped(e)) => { self.wait_until = now + RESEND; self.outcome = Some(format!("stopped: {e}")) }
            }
        }
        if self.job.is_none() && now >= self.wait_until {
            if let Err(e) = self.start(install, snap, local, now) {
                self.outcome = Some(e);
                self.wait_until = now + RESEND;
            }
        }
        (self.outcome != before).then(|| format!("dashq connection: {}", self.outcome.as_deref().unwrap_or("none")))
    }

    fn start(&mut self, install: &Path, snap: Option<&Snapshot>, local: &Local, now: u64) -> Result<(), String> {
        let d = dir(install);
        // #1328: the pass reads the state file only; every coreq load (Windows: `whoami` and `icacls` per load)
        // runs on the delivery thread
        let st = state(install)?;
        let Some(a) = st.as_ref().map(|s| s["active"].clone()).filter(Value::is_object) else {
            if self.sent.take().is_some() { self.outcome = Some("disconnected".into()) }
            // disconnected offline: queued revokes retry until DashQ confirms them
            if st.is_some_and(|s| s["revoke"].as_array().is_some_and(|r| !r.is_empty())) {
                let mut backoff = std::mem::take(&mut self.backoff);
                self.job = Some((now, std::thread::spawn(move || {
                    let r = enroll::retry_revokes(&d, &mut Https { browser: false });
                    let wait = backoff.next(None);
                    (backoff, String::new(), r.map(|_| Some("revoke confirmed".into())).map_err(|_| Delivery::Retry(wait)))
                })));
            }
            return Ok(());
        };
        // another project's board, or no board read yet: nothing to send from here
        let Some(s) = snap.filter(|s| speaks(&a, Some(s))) else { return Ok(()) };
        let (board, at, payload) = payload(s, &a, local).map_err(|e| format!("not sent: {e}"))?;
        let mut backoff = std::mem::take(&mut self.backoff);
        let (install, sent) = (install.to_path_buf(), self.sent.clone());
        self.job = Some((now, std::thread::spawn(move || {
            let rec = match stamp(&install, &board, at, payload) {
                Ok(r) => r,
                Err(e) => return (backoff, String::new(), Err(Delivery::Stopped(format!("not sent: {e}")))),
            };
            let (mut digest, mut recs) = (board, vec![rec]);
            // the board record goes alone when the resources one cannot be built; the outcome names why
            let skipped = match resources_record(&install, &a, now) {
                Ok((d, r)) => { digest = format!("{digest}:{d}"); recs.push(r); None }
                Err(e) => Some(format!("resources not sent: {e}")),
            };
            if sent.is_some_and(|(d, at)| d == digest && now < at + RESEND) { return (backoff, digest, Ok(None)) }
            let r = enroll::deliver(&d, &recs, &mut backoff, &mut resources::Fenced::new(&install, Https { browser: false })).map(|r| {
                let state = |x: &Value| match x["state"].as_str() {
                    Some("accepted") => "accepted".to_string(),
                    _ => format!("rejected: {}", x["reason"].as_str().unwrap_or("unknown")),
                };
                let res = r.get(1).map(|x| format!("resources {}", state(x))).or(skipped);
                Some(std::iter::once(state(&r[0])).chain(res).collect::<Vec<_>>().join("; "))
            });
            (backoff, digest, r)
        })));
        Ok(())
    }
}

/// Create once, outside release folders; a corrupt identity never silently becomes a new installation.
pub fn installation_id(install: &Path) -> Res<String> {
    let path = install.join("installation-id");
    let read = || -> Res<Option<String>> {
        let bytes = fs::File::open(&path).and_then(|f| {
            let mut bytes = Vec::new();
            f.take(65).read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        match bytes {
            Ok(b) if b.len() == 64 && b.iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c)) =>
                Ok(Some(String::from_utf8(b).unwrap())),
            Ok(_) => Err("installation-id: corrupt; file kept".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("installation-id: unreadable ({e}); file kept")),
        }
    };
    if let Some(id) = read()? { return Ok(id) }
    fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let mut bytes = [0; 32];
    SystemRandom::new().fill(&mut bytes).map_err(|_| "installation-id: randomness unavailable")?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let temp = install.join(format!("installation-id-{id}.tmp"));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let publish = || -> Res<()> {
        let mut file = opts.open(&temp).map_err(|e| e.to_string())?;
        file.write_all(id.as_bytes()).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        drop(file);
        // A hard link publishes complete bytes without overwriting another project's winner.
        match fs::hard_link(&temp, &path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(format!("installation-id: publish failed ({e})")),
        }
    };
    let result = publish();
    let _ = fs::remove_file(&temp);
    result?;
    read()?.ok_or("installation-id: missing after publication".into())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct BoardScope {
    kind: String,
    host: String,
    repo: String,
}

impl BoardScope {
    /// `<kind>:<host>/<repo>`: the scope DashQ's approval page shows and its grant stores.
    pub fn value(&self) -> String {
        format!("{}:{}/{}", self.kind, self.host, self.repo)
    }

    pub fn new(kind: &str, host: &str, repo: &str) -> Res<Self> {
        let safe = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)) && s != "." && s != "..";
        if !matches!(kind, "github" | "gitlab") || !safe(host) || repo.split('/').count() < 2 || !repo.split('/').all(safe) {
            return Err("invalid board scope".into());
        }
        Ok(Self { kind: kind.into(), host: host.to_ascii_lowercase(), repo: if kind == "github" { repo.to_ascii_lowercase() } else { repo.into() } })
    }

    fn snapshot(s: &Snapshot) -> Res<Self> {
        let field = |k: &str| s.board[k].as_str().ok_or("missing board identity".to_string());
        Self::new(field("kind")?, field("host")?, field("repo")?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Choice { number: usize, label: String, recommended: bool }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct TaskObservation {
    n: u64,
    state: String,
    revision: u64,
    choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<Value>,
    /// #1616 admission of a ready or waiting task: code, board read time, deciding host; reason only for `unknown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    admission: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Observation {
    schema: u64,
    installation: String,
    board: BoardScope,
    observed: i64,
    complete: bool,
    reason: Option<&'static str>,
    totals: Option<BTreeMap<&'static str, usize>>,
    tasks: Vec<TaskObservation>,
    /// Host `online`, `login` and `queue` observations (docs/design/enrollment.md#contract-detail).
    observations: Vec<Value>,
}

/// What the sending service knows beyond the board: its host name, the board `ttl` and its last runtime probe
/// (`status.json` `agents`: `{at, rows}`; `Null`: none yet).
pub struct Local {
    pub host: String,
    pub service: Option<String>,
    pub ttl: u64,
    pub agents: Value,
    /// Session progress per task of this host's live attempts ([`progress`]).
    pub progress: BTreeMap<u64, Value>,
}

/// R6 3 off-host sessions: per live attempt on `host`, the stage and the attempt log's update time; never worker
/// line text, the log, a path or a session id (#1617).
pub fn progress(s: &Snapshot, root: &Path, host: &str) -> BTreeMap<u64, Value> {
    let mut out = BTreeMap::new();
    for r in s.tasks.iter().take(MAX_TASKS).filter(|r| matches!(r.task.state.name(), "doing" | "review")) {
        let (Some(a), Some(session)) = (r.task.attempt, r.task.session.as_deref()) else { continue };
        if session.rsplit(' ').next() != Some(host) { continue }
        let runtime = session.split(':').next().unwrap_or_default();
        let log = crate::runtime::attempt_log(root, r.task.role.as_deref().unwrap_or_default(), r.n, a);
        let at = fs::metadata(&log).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
        out.insert(r.n, json!({"host": host, "runtime": runtime, "stage": r.task.state.name(), "at": at}));
    }
    out
}

/// A login probe answers for two probe intervals (§ 4: at most every 5 minutes).
const LOGIN_VALID: u64 = 600;

/// One observation per host and kind; every one names producer, host scope, time, `valid_until` and `complete`.
/// `online` and `queue` come from the board read and cover every heartbeat host; `login` only this host's probe.
fn observations(s: &Snapshot, l: &Local) -> Vec<Value> {
    let until = s.observed.saturating_add(l.ttl as i64);
    let mut names: Vec<&str> = s.hosts.iter().map(|h| h.name.as_str()).chain([l.host.as_str()]).collect();
    names.sort();
    names.dedup();
    let mut out = Vec::new();
    for name in names {
        let beat = s.hosts.iter().filter(|h| h.name == name).max_by_key(|h| h.updated);
        let age = beat.map(|h| s.observed - h.updated);
        let service = beat.and_then(|h| h.service.clone());
        let (state, reason) = match age {
            None => ("unknown", Some("no heartbeat")),
            Some(a) if a < 0 || s.observed <= 0 => ("unknown", Some("heartbeat time unknown")),
            Some(a) if a > l.ttl as i64 => ("offline", None),
            _ if service.is_none() => ("online", Some("no service identity")),
            _ => ("online", None),
        };
        out.push(json!({"host": name, "kind": "online", "state": state, "producer": service, "observed": s.observed,
            "heartbeat": beat.map(|h| h.updated), "valid_until": until, "complete": state != "unknown" && reason.is_none(), "reason": reason}));
        // Assigned queue: live attempts whose session names this host, and ready or waiting tasks labelled for it.
        let rows = || s.tasks.iter().take(MAX_TASKS);
        let running = rows().filter(|r| r.task.attempt.is_some() && matches!(r.task.state.name(), "doing" | "review")
            && r.task.session.as_deref().and_then(|x| x.rsplit(' ').next()) == Some(name)).count();
        let queued = rows().filter(|r| matches!(r.task.state.name(), "ready" | "waiting")
            && r.labels.iter().any(|x| x.strip_prefix("host-") == Some(name))).count();
        let (state, reason) = match (&s.reason, s.tasks.len() > MAX_TASKS) {
            (Some(_), _) => ("unknown", Some("source incomplete")),
            (None, true) => (if running > 0 { "busy" } else { "idle" }, Some("task limit")),
            _ => (if running > 0 { "busy" } else { "idle" }, None),
        };
        let known = state != "unknown";
        out.push(json!({"host": name, "kind": "queue", "state": state, "producer": l.service, "observed": s.observed,
            "valid_until": until, "complete": known && reason.is_none(), "reason": reason,
            "running": known.then_some(running), "queued": known.then_some(queued)}));
    }
    let at = l.agents["at"].as_u64();
    let mut o = json!({"host": l.host, "kind": "login", "producer": l.service, "observed": at, "valid_until": at.map(|t| t + LOGIN_VALID)});
    let mut seen = Vec::new();
    for r in l.agents["rows"].as_array().into_iter().flatten() {
        let Some(rt) = r["runtime"].as_str() else { continue };
        let v = match r["login"].as_str() { Some(x @ ("logged in" | "not logged in" | "not installed")) => x, _ => "unknown" };
        o[rt] = json!(v);
        seen.push(v);
    }
    let unknown = at.is_none() || seen.is_empty() || seen.contains(&"unknown");
    let known: Vec<_> = seen.iter().filter(|v| **v != "unknown" && **v != "not installed").collect();
    o["state"] = json!(if at.is_none() || known.is_empty() && unknown { "unknown" }
        else if known.iter().any(|v| **v == "not logged in") || known.is_empty() { "not logged in" } else { "logged in" });
    o["complete"] = json!(!unknown);
    o["reason"] = json!(if at.is_none() { Some("no probe yet") } else if unknown { Some("runtime login unknown") } else { None });
    out.push(o);
    out
}

/// The scope is the approved board list, never inferred from host names or projects on disk.
pub fn project(s: &Snapshot, installation: &str, approved: &[BoardScope], local: &Local) -> Res<Observation> {
    let board = BoardScope::snapshot(s)?;
    if !approved.contains(&board) { return Err("board outside approved scope".into()) }
    if installation.len() != 64 || !installation.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err("invalid installation identity".into());
    }
    let over = s.tasks.len() > MAX_TASKS;
    Ok(Observation {
        schema: 1, installation: installation.into(), board, observed: s.observed,
        complete: s.reason.is_none() && !over,
        reason: if s.reason.is_some() { Some("source incomplete") } else if over { Some("task limit") } else { None },
        totals: s.reason.is_none().then(|| s.totals()),
        tasks: s.tasks.iter().take(MAX_TASKS).map(|r| TaskObservation {
            n: r.n, state: r.task.state.name().into(), revision: r.revision,
            choices: core::options(r.task.card.as_deref().unwrap_or_default()).into_iter()
                .map(|(number, label, recommended)| Choice { number, label, recommended }).collect(),
            session: local.progress.get(&r.n).cloned(),
            admission: r.admission.as_ref().map(crate::snapshot::Admission::projection),
        }).collect(),
        observations: observations(s, local),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Chain: identity creation -> concurrent project startup -> update/data pruning -> reread.
    #[test]
    fn identity_survives_pruning_and_concurrent_creation() {
        let root = std::env::temp_dir().join(format!("taskq-enrollment-{}", std::process::id()));
        let install = root.join("install");
        let _ = fs::remove_dir_all(&root);
        let ids = std::thread::scope(|s| {
            let threads: Vec<_> = (0..8).map(|_| s.spawn(|| installation_id(&install).unwrap())).collect();
            threads.into_iter().map(|t| t.join().unwrap()).collect::<Vec<_>>()
        });
        assert!(ids.iter().all(|id| id == &ids[0]));
        crate::platform::sweep(&install, &root.join("absent-shim"));
        crate::data::prune(&root, &install, "p", &[]);
        assert_eq!(installation_id(&install).unwrap(), ids[0]);
        fs::write(install.join("installation-id"), "broken").unwrap();
        assert!(installation_id(&install).unwrap_err().contains("corrupt"));
        assert_eq!(fs::read_to_string(install.join("installation-id")).unwrap(), "broken");
        // connection state survives the sweep and pruning too
        fs::create_dir_all(dir(&install)).unwrap();
        fs::write(dir(&install).join("connection.json"), "{}").unwrap();
        crate::platform::sweep(&install, &root.join("absent-shim"));
        assert!(dir(&install).join("connection.json").exists());
        fs::remove_file(install.join("installation-id")).unwrap();
        assert_ne!(installation_id(&install).unwrap(), ids[0]);
        fs::remove_dir_all(root).unwrap();
    }

    /// DashQ stand-in: answers revokes (or nothing, offline) and records the credentials it was asked to revoke.
    struct Dash { online: bool, revoked: Vec<String> }
    impl enroll::Wire for Dash {
        fn post(&mut self, _: &str, route: &str, auth: Option<&str>, _: &Value) -> Result<(u16, Value), String> {
            if !self.online { return Err("offline".into()) }
            assert_eq!(route, "/connection/revoke", "disconnect only revokes");
            self.revoked.push(auth.unwrap_or_default().into());
            Ok((200, json!({"protocol": 1})))
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    // Chain: enroll interrupted mid-pairing -> taskq disconnect -> enroll rerun. Risk: disconnect reporting the
    // revoke confirmed while a pending pairing (or its staged, already granted credential) survives and resumes.
    #[test]
    fn disconnect_cancels_pending_pairing() {
        let install = std::env::temp_dir().join(format!("taskq-disconnect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&install);
        let write = |pending: Value, active: Value, revoke: Value| {
            fs::create_dir_all(dir(&install)).unwrap();
            fs::write(dir(&install).join("connection.json"), json!({"installation": "i", "pending": pending, "active": active, "revoke": revoke}).to_string()).unwrap();
        };
        let conn = |c: &str| json!({"destination": "https://d/machine", "credential": c});
        let mut dash = Dash { online: true, revoked: vec![] };
        assert_eq!(disconnect_with(&install, &mut dash).unwrap(), None, "no state: none");
        // code shown, not approved yet: nothing granted
        write(json!({"destination": "https://d/machine", "device_code": "x", "staged": null}), Value::Null, json!([]));
        assert_eq!(disconnect_with(&install, &mut dash).unwrap().as_deref(), Some("disconnected: pairing cancelled; nothing granted to revoke"));
        assert!(state(&install).unwrap().unwrap()["pending"].is_null(), "enroll cannot resume the pairing");
        assert_eq!(disconnect_with(&install, &mut dash).unwrap(), None, "second disconnect says none");
        // approved and staged, ack not sent, beside an active connection: both credentials revoked
        write(json!({"destination": "https://d/machine", "device_code": "x", "staged": conn("staged")}), conn("active"), json!([]));
        assert_eq!(disconnect_with(&install, &mut dash).unwrap().as_deref(), Some("disconnected: pairing cancelled; sending stopped; revoke confirmed"));
        assert_eq!(dash.revoked, ["staged", "active"]);
        // offline: the revoke stays queued for the service, reported as pending
        write(json!({"destination": "https://d/machine", "device_code": "x", "staged": conn("s2")}), Value::Null, json!([]));
        let mut off = Dash { online: false, revoked: vec![] };
        assert_eq!(disconnect_with(&install, &mut off).unwrap().as_deref(), Some("disconnected: pairing cancelled; 1 revoke pending, the service retries it"));
        let s = state(&install).unwrap().unwrap();
        assert!(s["pending"].is_null() && s["revoke"][0]["credential"] == "s2");
        // repeated disconnect with only the queued revoke: retried and reported, never "connection: none"
        assert_eq!(disconnect_with(&install, &mut off).unwrap().as_deref(), Some("connection: none; 1 revoke pending, the service retries it"));
        let mut back = Dash { online: true, revoked: vec![] };
        assert_eq!(disconnect_with(&install, &mut back).unwrap().as_deref(), Some("connection: none; revoke confirmed"));
        assert_eq!(back.revoked, ["s2"]);
        assert_eq!(disconnect_with(&install, &mut back).unwrap(), None, "nothing left: none");
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: damaged connection.json -> connection status, disconnect, service pass. Risk: valid JSON of the wrong
    // shape reading as "connection: none" (R12) or being overwritten, losing a credential DashQ still honours.
    #[test]
    fn malformed_connection_state_reads_unknown_and_is_kept() {
        let install = std::env::temp_dir().join(format!("taskq-malformed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&install);
        fs::create_dir_all(dir(&install)).unwrap();
        let ok = json!({"installation": "i", "pending": null, "active": {"destination": "https://d/machine", "credential": "c"}, "revoke": []});
        let mut bad = vec![json!({}), json!([]), json!(null), json!("x")];
        for (k, v) in [("installation", json!(1)), ("pending", json!("x")), ("pending", json!({"staged": null})),
            ("active", json!({"destination": "https://d/machine"})), ("active", json!([])), ("revoke", json!(null)), ("revoke", json!([1])),
            ("revoke", json!([{"credential": "c"}])), ("revoke", json!([{"destination": "", "credential": "c"}]))] {
            let mut s = ok.clone();
            s[k] = v;
            bad.push(s);
        }
        let path = dir(&install).join("connection.json");
        fs::write(&path, ok.to_string()).unwrap();
        assert!(state(&install).is_ok() && active(&install).unwrap().is_some(), "coreq's own shape reads");
        for b in bad {
            fs::write(&path, b.to_string()).unwrap();
            let unknown = |r: Res<String>| assert!(r.unwrap_err().starts_with("connection: unknown"), "{b}");
            unknown(state(&install).map(|_| String::new()));
            unknown(active(&install).map(|_| String::new()));
            unknown(disconnect_with(&install, &mut Dash { online: true, revoked: vec![] }).map(|_| String::new()));
            let mut s = Sender::default();
            assert!(s.tick(&install, Some(&snap("o/a")), &local(), 1).unwrap().contains("unknown"), "{b}");
            assert!(s.job.is_none());
            assert_eq!(fs::read_to_string(&path).unwrap(), b.to_string(), "file kept");
        }
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: board heartbeats, task rows and the local runtime probe -> host observations -> DashQ reader expiry.
    // Risk: DashQ infers online, logged in or an empty queue from silence, an old record or an incomplete read.
    #[test]
    fn host_observations_match_the_fixture() {
        use crate::{core::{State, Task}, snapshot::{Host, Row}};
        let beat = |name: &str, updated, service: Option<&str>| Host { name: name.into(), updated, service: service.map(String::from), ..Default::default() };
        let row = |n, state, session: Option<&str>, labels: &[&str]| Row { n, labels: labels.iter().map(|l| l.to_string()).collect(),
            task: Task { state, attempt: session.map(|_| n), session: session.map(String::from), ..Default::default() }, ..Default::default() };
        let base = || Snapshot { source: "host", observed: 1000, reason: None, board: json!({"kind": "github", "host": "github.com", "repo": "o/r"}),
            tasks: vec![row(1, State::Doing, Some("claude:x mac"), &[]), row(2, State::Ready, None, &["host-mac"]),
                row(3, State::Waiting, None, &["host-win"]), row(4, State::Review, Some("codex:y win"), &[]), row(5, State::Ready, None, &[])],
            // mac online, win offline (heartbeat older than ttl), fra partial (no service line), wsl unknown (no heartbeat)
            hosts: vec![beat("mac", 950, Some("aaaaaaaaaaaa")), beat("win", 600, Some("bbbbbbbbbbbb")), beat("fra", 980, None)] };
        let probe = |codex: &str| json!({"at": 990, "rows": [{"runtime": "claude", "login": "logged in"}, {"runtime": "codex", "login": codex}]});
        let local = |agents| Local { host: "wsl".into(), service: Some("cccccccccccc".into()), ttl: 300, agents, progress: BTreeMap::new() };
        let scope = [BoardScope::new("github", "github.com", "o/r").unwrap()];
        let obs = |s: &Snapshot, agents| project(s, &"a".repeat(64), &scope, &local(agents)).unwrap();
        let mut incomplete = base();
        incomplete.reason = Some("board read failed".into());
        let mut over = base();
        over.tasks.extend((10..10 + MAX_TASKS as u64).map(|n| row(n, State::Ready, None, &[])));
        let mac = obs(&base(), probe("logged in"));
        let got = json!({
            "online offline unknown partial; queue busy idle; login logged in": mac.observations,
            "login not logged in": obs(&base(), probe("not logged in")).observations.last(),
            "login unknown: no probe": obs(&base(), Value::Null).observations.last(),
            "login partial: codex unknown": obs(&base(), probe("unknown")).observations.last(),
            "queue unknown: incomplete read": obs(&incomplete, Value::Null).observations.iter().filter(|o| o["kind"] == "queue").collect::<Vec<_>>(),
            "queue partial: task limit": obs(&over, Value::Null).observations.iter().filter(|o| o["kind"] == "queue").collect::<Vec<_>>(),
        });
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/host-observations.json");
        if std::env::var_os("TASKQ_BLESS").is_some() { fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap(); }
        let want: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
        assert!(!got.to_string().contains("x mac") && !got.to_string().contains("codex:y"), "no session ids");
    }

    // Chain: snapshot admission -> taskq-board/1 task rows -> shipped fixture (#1616). Risk: a consumer meets a code
    // outside the fixture, free reason text leaves the host, or a row nobody decided reads as decided.
    #[test]
    fn admission_codes_match_the_fixture() {
        use crate::{core::{State, Task}, snapshot::{Admission, Row, ADMISSION_CODES, ADMISSION_UNKNOWN}};
        let row = |n, state, admission| Row { n, task: Task { state, ..Default::default() }, admission, ..Default::default() };
        let decided = |code, reason: &str, host: Option<&str>| Some(Admission { code, reason: reason.into(), observed: 1000, host: host.map(String::from) });
        let mut tasks: Vec<Row> = ADMISSION_CODES.iter().map(|c| c.0).filter(|c| !matches!(*c, "dependency" | "unknown")).enumerate()
            .map(|(i, c)| row(i as u64 + 1, State::Ready, decided(c, "free text from the host", Some("mac")))).collect();
        tasks.push(row(20, State::Waiting, decided("dependency", "#1 open", None)));
        tasks.extend(ADMISSION_UNKNOWN.iter().enumerate().map(|(i, r)| row(30 + i as u64, State::Ready, decided("unknown", r, (i > 0).then_some("mac")))));
        tasks.push(row(40, State::Doing, None));
        let snap = Snapshot { source: "host", observed: 1000, reason: None, board: json!({"kind": "github", "host": "github.com", "repo": "o/r"}), tasks, hosts: vec![] };
        let local = Local { host: "mac".into(), service: None, ttl: 300, agents: Value::Null, progress: BTreeMap::new() };
        let scope = [BoardScope::new("github", "github.com", "o/r").unwrap()];
        let o = project(&snap, &"a".repeat(64), &scope, &local).unwrap();
        let got = json!({
            "codes": ADMISSION_CODES.iter().map(|(code, meaning)| json!({"code": code, "meaning": meaning})).collect::<Vec<_>>(),
            "unknown_reasons": ADMISSION_UNKNOWN,
            "tasks": o.tasks,
        });
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/admission-codes.json");
        if std::env::var_os("TASKQ_BLESS").is_some() { fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap(); }
        let want: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
        let rows = got["tasks"].as_array().unwrap();
        let mut seen: Vec<&str> = rows.iter().filter_map(|t| t["admission"]["code"].as_str()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), ADMISSION_CODES.len(), "the fixture shows every code");
        assert!(!got["tasks"].to_string().contains("free text") && !got["tasks"].to_string().contains("#1 open"), "free reason text stays on the host");
        assert!(rows.iter().filter(|t| t["admission"]["code"] == "unknown").all(|t| t["admission"]["reason"].is_string()), "unknown names its reason");
        assert!(rows.iter().find(|t| t["n"] == 40).unwrap().get("admission").is_none(), "a doing row carries none");
    }

    // Chain: live attempt log -> R6 3 session progress -> DashQ record allowlist (#1565, #1617). Risk: another host's or an
    // idle task's progress, worker line text, the log itself, a session id or a path leaving the host.
    #[test]
    fn progress_carries_only_stage_and_age_of_this_hosts_live_attempts() {
        use crate::{core::{State, Task}, snapshot::Row};
        let root = std::env::temp_dir().join(format!("taskq-progress-{}", std::process::id()));
        fs::create_dir_all(root.join(".taskq")).unwrap();
        let row = |n, state, session: &str| Row { n, task: Task { state, attempt: Some(n * 10), role: Some("worker".into()), session: Some(session.into()), ..Default::default() }, ..Default::default() };
        let say = |n: u64, text: &str| fs::write(crate::runtime::attempt_log(&root, "worker", n, n * 10),
            json!({"type": "item.completed", "item": {"type": "agent_message", "text": text}}).to_string() + "\n").unwrap();
        say(1, &format!("Running tests in {}/src now", root.display()));
        say(2, "secret worker words");
        say(3, "other host");
        say(4, "done");
        let s = Snapshot { tasks: vec![row(1, State::Doing, "codex:thread-a mac"), row(2, State::Review, "codex:- mac"), row(3, State::Doing, "codex:b win"),
            row(4, State::Ready, "codex:c mac")], ..snap("o/r") };
        let p = progress(&s, &root, "mac");
        assert_eq!(p.keys().copied().collect::<Vec<_>>(), vec![1, 2], "only this host's doing or review attempts");
        assert_eq!((p[&1]["stage"].as_str(), p[&1]["runtime"].as_str(), p[&1]["host"].as_str()), (Some("doing"), Some("codex"), Some("mac")));
        assert!(p[&1]["at"].as_u64().is_some(), "update time");
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&p[&2]), ["host", "runtime", "stage", "at"], "host, runtime, stage and age only");
        let mut l = local();
        l.progress = p;
        let o = project(&s, &"a".repeat(64), &[BoardScope::new("github", "github.com", "o/r").unwrap()], &l).unwrap();
        let text = serde_json::to_string(&o).unwrap();
        for leak in ["thread-a", &*root.to_string_lossy(), "Running tests", "secret worker words", "\"line\""] {
            assert!(!text.contains(leak), "{leak} left the host: {text}");
        }
        assert!(serde_json::to_value(&o).unwrap()["tasks"][2].get("session").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    fn local() -> Local { Local { host: "mac".into(), service: None, ttl: 300, agents: Value::Null, progress: BTreeMap::new() } }

    fn snap(repo: &str) -> Snapshot {
        Snapshot { source: "host", observed: 1_700_000_000, reason: None, board: json!({"kind": "github", "host": "github.com", "repo": repo}), tasks: vec![], hosts: vec![] }
    }

    // Chain: enrolled board scope -> service pass of each project -> delivery thread. Risk: another project's board
    // leaking into the grant, or an unreachable DashQ stalling the pass that launches workers.
    #[test]
    fn sender_sends_only_the_approved_board_and_never_blocks() {
        let install = std::env::temp_dir().join(format!("taskq-sender-{}", std::process::id()));
        let _ = fs::remove_dir_all(&install);
        assert_eq!(Sender::default().tick(&install, Some(&snap("o/a")), &local(), 1), None, "no connection: nothing sent");
        assert!(!dir(&install).exists(), "a status read creates no connection state");
        let board = BoardScope::new("github", "GitHub.com", "O/A").unwrap();
        assert_eq!(board.value(), "github:github.com/o/a");
        let id = installation_id(&install).unwrap();
        let conn = json!({"destination": "https://127.0.0.1:9/machine", "credential": "c", "generation": 1, "installation": id, "scope": {"name": SCHEMA, "value": board.value()}});
        fs::create_dir_all(dir(&install)).unwrap();
        fs::write(dir(&install).join("connection.json"), json!({"installation": id, "pending": null, "active": conn, "revoke": []}).to_string()).unwrap();
        assert_eq!(approved(&conn), Some(board));
        assert_eq!(approved(&json!({"scope": {"name": SCHEMA, "value": "github:github.com/O/A"}})), None, "non-canonical scope");
        let mut s = Sender::default();
        assert_eq!(s.tick(&install, Some(&snap("o/b")), &local(), 1), None, "another board: nothing sent");
        assert!(s.job.is_none());
        let (_, rec) = record(&install, &snap("o/a"), &conn, &local()).unwrap();
        assert_eq!((rec["schema"].as_str(), rec["payload"]["board"]["repo"].as_str()), (Some(SCHEMA), Some("o/a")));
        // #1328: each step's cost on this OS, printed on success and failure; only the state read and the payload
        // stay on the pass
        let timed = |f: &dyn Fn()| { let t = std::time::Instant::now(); f(); t.elapsed() };
        let steps = format!("state read {:?}, coreq active read {:?}, record build {:?}, resources read {:?}",
            timed(&|| { state(&install).unwrap(); }), timed(&|| { active(&install).unwrap(); }),
            timed(&|| { record(&install, &snap("o/a"), &conn, &local()).unwrap(); }), timed(&|| { let _ = resources_record(&install, &conn, 1); }));
        let t = std::time::Instant::now();
        assert_eq!(s.tick(&install, Some(&snap("o/a")), &local(), 1), None);
        let steps = format!("{steps}, tick with thread spawn {:?}", t.elapsed());
        eprintln!("sender steps: {steps}");
        assert!(s.job.is_some() && t.elapsed().as_secs() < 2, "delivery runs on its own thread: {steps}");
        while !s.job.as_ref().unwrap().1.is_finished() { std::thread::sleep(std::time::Duration::from_millis(50)) }
        assert_eq!(s.tick(&install, Some(&snap("o/a")), &local(), 2).as_deref(), Some("dashq connection: retry in 1 s"), "outage only waits");
        fs::remove_dir_all(install).unwrap();
    }
}
