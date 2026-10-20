//! Host setup session (taskq.md § 4, [detail](../docs/design/standalone-onboarding.md#contract-detail)): one request in
//! `<install>/setup/request.json`, written by `service setup`, launched only by the host service through the § 3 grant.
//! A crash between `launching` and the native answer reads `interrupted`; nothing relaunches without `--new`.

use crate::host::Policy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: u8,
    pub id: String,
    pub host: String,
    pub platform: String,
    pub client: String,
    pub runtime: String,
    pub targets: Vec<String>,
    pub brief: String,
    pub state: String,
    #[serde(default)] pub job: Option<String>,
    #[serde(default)] pub session: Option<String>,
    #[serde(default)] pub url: Option<String>,
    #[serde(default)] pub reason: Option<String>,
}

const STATES: [&str; 6] = ["pending", "launching", "running", "ended", "failed", "interrupted"];
/// Setup belongs to no project: only host limits apply.
fn no_project_limits() -> crate::service::Capacity {
    crate::service::Capacity { limits: Default::default(), gate: crate::service::GATE_DEFAULT, auto: true, max: None }
}
fn terminal(state: &str) -> bool { matches!(state, "ended" | "failed" | "interrupted") }

pub fn dir(install: &Path) -> PathBuf { install.join("setup") }

/// The occupancy key: one setup session per host, so task and attempt are constant.
fn slot(rt: &str) -> Vec<crate::admission::Res> { vec![(1, 1, rt.into())] }

pub fn read(install: &Path) -> Res<Option<Request>> {
    let text = match fs::read_to_string(dir(install).join("request.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        r => r.map_err(|e| format!("setup request unknown: {e}"))?,
    };
    let r: Request = serde_json::from_str(&text).map_err(|e| format!("setup request unknown: {e}"))?;
    if r.schema != 1 || !STATES.contains(&r.state.as_str()) || !crate::resources::id(&r.id) { return Err("setup request unknown: unsupported record; kept".into()); }
    Ok(Some(r))
}

fn write(install: &Path, r: &Request) -> Res<()> {
    let text = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    crate::platform::write_atomic(&dir(install).join("request.json"), &(text + "\n")).map_err(|e| format!("setup request write uncertain: {e}"))
}

fn locked<T>(install: &Path, f: impl FnOnce() -> Res<T>) -> Res<T> {
    fs::create_dir_all(dir(install)).map_err(|e| format!("setup: {e}"))?;
    let _lock = coreq::file_lock::try_lock(&dir(install).join("lock")).map_err(|e| format!("setup request busy, retry: {e}"))?;
    f()
}

/// The PM chat scope (owner) around the standalone-onboarding agentq brief; values are the exact request fields.
fn brief(r: &Request, workdir: &Path) -> String {
    let targets = if r.targets.is_empty() { "none; ask the owner before touching any other folder".into() } else { r.targets.join(", ") };
    format!("You are the TaskQ PM for this host, opened by the host service because the owner asked for setup.
Host: {host}
Native platform: {platform}
Selected client: {client}
Runtime: {runtime}
Setup working directory: {workdir}
Approved targets: {targets}
This is a full PM chat, not a wizard. No TaskQ project may be registered yet and no board token may exist.
Offer, and do on the owner's request: set up an existing folder as a TaskQ project or create a new one
(taskq.md section 6; confirm concrete external effects first), show local telemetry (`taskq profile`),
show the host service (`taskq service status`, `taskq service list`), and set up agentq.
Never run gh auth login; a missing board token is an owner item: say what the owner must install.
Touch only the setup working directory, the approved targets, ~/.claude, ~/.codex and the OS temp dir.
Do not search home, the disk or unrelated projects. Do not change TaskQ admission, runtime policy,
model, effort or permissions. Do not kill or restart existing agent sessions.
For agentq: set up agentq from https://github.com/taskqorg/agentq on this host for the selected client only.
Read the official INSTALL.md and agentq.md before acting. Follow the current release procedure and verify
checksums; stop on verification failure. Run read-only onboarding discovery first. If the discovered plan
needs other clients, legacy removal, foreign optimizer/provider changes, new access or other effects,
explain them and ask before those changes. Preserve existing files, ownership, logins, models, effort and
permissions. Do not install another agent runtime or silently retry with broader permissions. Never request
or print credentials in chat. Do not enroll in DashQ or configure optional telemetry credentials.
Read back the installed version and native service status; report configured separately from verified by a
real turn and report unavailable checks and partial effects explicitly. A successful exit is not proof of
installation or savings; TaskQ independently rereads status.
", host = r.host, platform = r.platform, client = r.client, runtime = r.runtime, workdir = workdir.display())
}

/// `service setup`: the current request, or a new `pending` one. Only a terminal request is replaced, and only with `new`.
pub fn submit(install: &Path, runtime: &str, client: Option<&str>, targets: &[String], new: bool) -> Res<Request> {
    if runtime != "claude" { return Err(format!("setup runtime {runtime}: only claude has a qualified visible session")); }
    let targets = targets.iter().map(|t| {
        let p = Path::new(t);
        if !p.is_absolute() { return Err(format!("setup target {t}: expected an absolute path")); }
        p.canonicalize().map(|p| p.display().to_string()).map_err(|e| format!("setup target {t}: {e}"))
    }).collect::<Res<Vec<_>>>()?;
    let r = locked(install, || {
        if let Some(r) = read(install)? {
            if !new { return Ok(r); }
            if !terminal(&r.state) { return Err(format!("setup request {} is {}; --new replaces only ended, failed or interrupted", r.id, r.state)); }
        }
        let id = crate::runtime::uuid();
        let platform = format!("{} {}{}", std::env::consts::OS, std::env::consts::ARCH, if cfg!(target_os = "linux") && (std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()) { " wsl" } else { "" });
        let mut r = Request { schema: 1, id: id.clone(), host: crate::hostname(), platform, client: client.unwrap_or("Claude Code").into(), runtime: runtime.into(),
            targets, brief: String::new(), state: "pending".into(), job: None, session: None, url: None, reason: None };
        r.brief = brief(&r, &dir(install).join(&id));
        write(install, &r)?;
        Ok(r)
    })?;
    if r.state == "pending" {
        crate::platform::write_atomic(&install.join("poke"), "").map_err(|e| format!("setup request saved; host notification unknown: {e}"))?;
    }
    Ok(r)
}

/// Service start: a launch without a recorded native answer has an unknown outcome and is never replayed (A5).
pub fn recover(install: &Path) -> Res<()> {
    locked(install, || {
        let Some(mut r) = read(install)? else { return Ok(()) };
        if r.state != "launching" { return Ok(()); }
        r.state = "interrupted".into();
        r.reason = Some("service stopped during launch; outcome unknown, inspect native sessions before --new".into());
        write(install, &r)
    })
}

pub enum Launched { Job(String), NoStart(String), Unknown(String) }

/// The native call, replaceable in tests.
pub type Launcher = fn(&Path, &Request) -> Launched;
/// The native job state (`claude agents`), replaceable in tests.
pub type Observer = fn(&Path, &str) -> Res<(String, Option<String>)>;

/// One host pass. `fresh`: no host.json and no registered project, so host.json defaults serve this grant only.
pub fn step(install: &Path, host: &crate::host::Shared, fresh: bool, observe: bool, launch: Launcher, native: Observer) -> Res<()> {
    let project = dir(install);
    let r = match read(install) {
        Ok(r) => r,
        // Unknown state keeps whatever occupancy was synced last (R12).
        Err(e) => return Err(e),
    };
    let mut h = host.lock().unwrap_or_else(|e| e.into_inner());
    h.prepared.insert(project.clone());
    let Some(mut r) = r else { h.sync(&project, vec![]); return Ok(()) };
    match r.state.as_str() {
        // Stop waits for children, not for a native background chat; nothing new launches while it runs.
        "launching" | "running" if !h.stopping => { h.sync(&project, slot(&r.runtime)); }
        "pending" => {}
        _ => { h.sync(&project, vec![]); }
    }
    if r.state == "running" && observe {
        drop(h);
        let Some(job) = r.job.clone() else { return Ok(()) };
        let (state, session) = match native(&project.join(&r.id), &job) {
            Ok(v) => v,
            Err(e) => { return note(install, &r.id, Some(format!("native state unknown: {e}"))); }
        };
        let next = match state.as_str() {
            "working" | "idle" | "blocked" => "running",
            "done" | "stopped" => "ended",
            "failed" | "crashed" => "failed",
            _ => return note(install, &r.id, Some(format!("native state {state:?} unknown"))),
        };
        let url = match (&r.url, &session) { (None, Some(s)) => crate::runtime::claude_remote(s), (u, _) => u.clone() };
        return locked(install, || {
            let mut cur = read(install)?.ok_or("setup request vanished")?;
            if cur.id != r.id || cur.state != "running" { return Ok(()); }
            let changed = cur.state != next || cur.session != session || cur.url != url || cur.reason.is_some();
            cur.state = next.into(); cur.session = session.clone(); cur.url = url.clone(); cur.reason = None;
            if changed { write(install, &cur)?; }
            if terminal(next) { host.lock().unwrap_or_else(|e| e.into_inner()).sync(&project, vec![]); }
            Ok(())
        });
    }
    if r.state != "pending" { return Ok(()); }
    let now = h.elapsed();
    let policy = match &h.policy {
        Ok(p) => Ok(p.clone()),
        Err(e) if fresh => Policy::parse(serde_json::json!({}), &crate::hostname()).map_err(|_| e.clone()).inspect(|p| h.gate.configure(p.cap.gate)),
        Err(e) => Err(e.clone()),
    };
    let granted = policy.and_then(|p| h.grant(&p, &project, 1, 1, &r.runtime, &no_project_limits(), false, now));
    if let Err(reason) = granted {
        drop(h);
        let reason = format!("waiting: {reason}");
        return if r.reason.as_deref() == Some(reason.as_str()) { Ok(()) } else { note(install, &r.id, Some(reason)) };
    }
    drop(h);
    let id = r.id.clone();
    let release = || host.lock().unwrap_or_else(|e| e.into_inner()).sync(&project, vec![]);
    let marked = locked(install, || {
        let cur = read(install)?.ok_or("setup request vanished")?;
        if cur.id != id || cur.state != "pending" { return Err("setup request changed before launch".into()); }
        r.state = "launching".into(); r.reason = None;
        write(install, &r)
    });
    if let Err(e) = marked { release(); return Err(e); }
    let workdir = project.join(&id);
    let outcome = match fs::create_dir_all(&workdir) {
        Ok(()) => launch(&workdir, &r),
        Err(e) => Launched::NoStart(format!("setup directory: {e}")),
    };
    let (state, job, reason) = match outcome {
        Launched::Job(job) => ("running", Some(job), None),
        Launched::NoStart(e) => ("failed", None, Some(e)),
        Launched::Unknown(e) => ("interrupted", None, Some(e)),
    };
    r.state = state.into(); r.job = job; r.reason = reason;
    if state != "running" { release(); }
    // A lost write leaves `launching`; the next service start turns it into `interrupted`, never a second launch.
    locked(install, || write(install, &r))
}

fn note(install: &Path, id: &str, reason: Option<String>) -> Res<()> {
    locked(install, || {
        let Some(mut r) = read(install)? else { return Ok(()) };
        if r.id != id || r.reason == reason { return Ok(()); }
        r.reason = reason;
        write(install, &r)
    })
}

/// Claude background session with Remote Control (§ 8 table); access and permission mode come from § 8 Access.
pub fn claude_launch(workdir: &Path, r: &Request) -> Launched {
    let install = workdir.parent().and_then(Path::parent).unwrap_or(workdir);
    let policy = Policy::read_for_hostname(install, crate::hostname).ok();
    let mode = policy.as_ref().map_or("dontAsk".to_string(), |p| p.permission_mode.clone());
    let exe = std::env::current_exe().unwrap_or_default();
    let env = crate::runtime::Env { root: workdir, exe: &exe, install, permission_mode: &mode, codex: &[], publish: "", board_env: &[] };
    let access = match crate::runtime::access(&env, &r.runtime) { Ok(a) => a, Err(e) => return Launched::NoStart(e.message) };
    let path = workdir.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) { return Launched::NoStart(format!("setup brief: {e}")); }
    let mut c = match crate::runtime::executable(crate::runtime::which("claude"), &access, &env) { Ok(c) => c, Err(e) => return Launched::NoStart(e) };
    c.current_dir(workdir).env_remove("CLAUDE_CODE_SESSION_ID").env_remove("CODEX_THREAD_ID")
        .env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT")
        .args(["--bg", "--remote-control", "taskq setup", "--permission-mode", access.claude.as_deref().unwrap_or(&mode)]);
    let input = format!("Before any work, use Read to read {} in full. It is this session's brief; follow it.", serde_json::to_string(&path.to_string_lossy()).unwrap_or_default());
    use coreq::process::Failure;
    let out = match coreq::process::exchange("setup.claude-launch", &mut c, input.as_bytes(), std::time::Duration::from_secs(30)) {
        Ok(out) | Err(Failure::Exit(out)) => out,
        Err(e @ (Failure::InvalidDeadline | Failure::Capacity | Failure::Owner(_) | Failure::Spawn(_) | Failure::InputLimit)) => return Launched::NoStart(format!("Claude launch: {e}")),
        Err(e) => return Launched::Unknown(format!("Claude launch outcome unknown: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let _ = fs::write(workdir.join("launch.log"), format!("{stdout}\n{}", String::from_utf8_lossy(&out.stderr)));
    match stdout.lines().find_map(|l| l.strip_prefix("backgrounded · ").and_then(|s| s.split_whitespace().next())) {
        Some(id) if out.status.success() && crate::runtime::native_job_id(id) => Launched::Job(id.into()),
        _ => Launched::Unknown("Claude returned no background job; outcome unknown, inspect native sessions".into()),
    }
}

pub fn claude_state(workdir: &Path, job: &str) -> Res<(String, Option<String>)> {
    let rows = crate::runtime::claude_agents(workdir)?;
    let row = rows.iter().find(|v| v["id"] == job).ok_or("setup job not listed")?;
    Ok((row["state"].as_str().ok_or("setup job state missing")?.into(), row["sessionId"].as_str().map(String::from)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};

    static LAUNCHES: AtomicUsize = AtomicUsize::new(0);
    fn ok(_: &Path, _: &Request) -> Launched { LAUNCHES.fetch_add(1, Ordering::SeqCst); Launched::Job("abcd1234".into()) }
    fn unknown(_: &Path, _: &Request) -> Launched { Launched::Unknown("timeout".into()) }
    fn never(_: &Path, _: &Request) -> Launched { panic!("setup launched again") }
    fn live(_: &Path, _: &str) -> Res<(String, Option<String>)> { Ok(("idle".into(), None)) }
    fn done(_: &Path, _: &str) -> Res<(String, Option<String>)> { Ok(("done".into(), None)) }
    fn gone(_: &Path, _: &str) -> Res<(String, Option<String>)> { Err("setup job not listed".into()) }

    // An unconfigured gate is open: these hosts test the grant, not CPU sampling.
    fn host(json: serde_json::Value) -> crate::host::Shared { Arc::new(Mutex::new(Host::new(Policy::parse(json, "h")))) }
    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-setup-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }
    fn state(i: &Path) -> String { read(i).unwrap().unwrap().state }

    // Chain: service setup to host launch to recovery (A2, A5). Risk: a repeat or restart launches a second installation.
    #[test]
    fn repeat_and_restart_never_relaunch() {
        let t = tmp();
        let i = t.as_path();
        let first = submit(i, "claude", None, &[], false).unwrap();
        assert_eq!(submit(i, "claude", Some("Codex"), &[], false).unwrap(), first, "repeat returns the same request");
        assert!(submit(i, "codex", None, &[], false).is_err(), "unqualified runtime refused");
        assert!(submit(i, "claude", None, &[], true).is_err(), "--new refused while pending");
        assert!(first.brief.contains("full PM chat") && first.brief.contains(&first.host));
        let h = host(serde_json::json!({}));
        h.lock().unwrap().rebuilding = false;
        let before = LAUNCHES.load(Ordering::SeqCst);
        step(i, &h, false, false, ok, live).unwrap();
        assert_eq!(LAUNCHES.load(Ordering::SeqCst), before + 1);
        let r = read(i).unwrap().unwrap();
        assert_eq!((r.state.as_str(), r.job.as_deref()), ("running", Some("abcd1234")));
        assert_eq!(h.lock().unwrap().used().get("claude"), Some(&1));
        // A restarted service re-syncs the running slot from the request and observes; it never launches.
        let h = host(serde_json::json!({}));
        recover(i).unwrap();
        step(i, &h, false, true, never, live).unwrap();
        assert_eq!(state(i), "running");
        assert_eq!(h.lock().unwrap().used().get("claude"), Some(&1));
        step(i, &h, false, true, never, gone).unwrap();
        assert_eq!(state(i), "running", "unreadable native state stays running (R12)");
        assert_eq!(h.lock().unwrap().used().get("claude"), Some(&1), "unknown keeps the slot");
        step(i, &h, false, true, never, done).unwrap();
        assert_eq!(state(i), "ended");
        assert_eq!(h.lock().unwrap().used().get("claude"), None, "ended releases the slot");
        assert_eq!(submit(i, "claude", None, &[], false).unwrap().state, "ended", "a repeat after the end still returns it");
        assert_ne!(submit(i, "claude", None, &[], true).unwrap().id, first.id, "--new after a terminal state");
    }

    // Chain: crash boundary between `launching` and the native answer (A5). Risk: an uncertain launch is replayed.
    #[test]
    fn uncertain_launch_is_interrupted() {
        let t = tmp();
        let i = t.as_path();
        submit(i, "claude", None, &[], false).unwrap();
        let h = host(serde_json::json!({}));
        h.lock().unwrap().rebuilding = false;
        step(i, &h, false, false, unknown, live).unwrap();
        assert_eq!(state(i), "interrupted");
        assert!(h.lock().unwrap().used().is_empty());
        // Crash after `launching` was written: recovery reads interrupted, the next pass launches nothing.
        let mut r = read(i).unwrap().unwrap();
        r.state = "launching".into();
        write(i, &r).unwrap();
        recover(i).unwrap();
        assert_eq!(state(i), "interrupted");
        step(i, &h, false, false, never, live).unwrap();
        // Stale identity: a record for another schema or id is unknown and kept, never replaced.
        fs::write(dir(i).join("request.json"), r#"{"schema":2}"#).unwrap();
        assert!(read(i).is_err() && submit(i, "claude", None, &[], true).is_err());
        assert!(step(i, &h, false, false, never, live).is_err());
    }

    // Chain: setup and project work share one host grant (A3). Risk: setup bypasses hold, limits or spacing, or counts twice.
    #[test]
    fn shared_admission() {
        let t = tmp();
        let i = t.as_path();
        submit(i, "claude", None, &[], false).unwrap();
        // Unknown policy holds unless the host is fresh.
        let h = Arc::new(Mutex::new(Host::new(Err("host.json unknown".into()))));
        step(i, &h, false, false, never, live).unwrap();
        assert!(read(i).unwrap().unwrap().reason.unwrap().contains("host.json unknown"));
        let h = host(serde_json::json!({"admission": {"max": 1}}));
        {let mut x = h.lock().unwrap(); x.rebuilding = false; x.held = Some("emergency hold".into());}
        step(i, &h, false, false, never, live).unwrap();
        assert_eq!(state(i), "pending");
        assert!(read(i).unwrap().unwrap().reason.unwrap().contains("emergency hold"));
        {let mut x = h.lock().unwrap(); x.held = None; x.sync(Path::new("/p"), vec![(7, 1, "codex".into())]);}
        step(i, &h, false, false, never, live).unwrap();
        assert!(read(i).unwrap().unwrap().reason.unwrap().contains("capacity full"), "admission.max counts project work");
        {let mut x = h.lock().unwrap(); x.sync(Path::new("/p"), vec![]); let now = x.elapsed();
         x.reserve(Path::new("/p"), 8, 0, "codex", &no_project_limits(), true, now).unwrap(); x.sync(Path::new("/p"), vec![]);}
        step(i, &h, false, false, never, live).unwrap();
        assert!(read(i).unwrap().unwrap().reason.unwrap().contains("30 s"), "launch spacing after the project grant");
        let h = host(serde_json::json!({"limits": {"claude": 0}}));
        h.lock().unwrap().rebuilding = false;
        step(i, &h, false, false, never, live).unwrap();
        assert!(read(i).unwrap().unwrap().reason.unwrap().contains("capacity full"), "runtime limit 0 holds setup");
        // A fresh host grants under host.json defaults, load gate included; they never become project policy.
        let h = Arc::new(Mutex::new(Host::new(Err("host.json unknown: no registered policy inputs".into()))));
        h.lock().unwrap().rebuilding = false;
        step(i, &h, true, false, never, live).unwrap();
        assert!(read(i).unwrap().unwrap().reason.unwrap().contains("load gate"), "defaults keep the load gate");
        assert!(h.lock().unwrap().policy.is_err());
    }
}
