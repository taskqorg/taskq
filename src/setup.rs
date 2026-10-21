//! Host setup sessions (taskq.md § 4, [detail](../docs/design/standalone-onboarding.md#contract-detail)): each `service setup`
//! writes a new `<install>/setup/<id>.json`; the host service launches each once, then the Claude session manager owns it.
//! A crash between `launching` and the native answer reads `interrupted`; nothing relaunches.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: u8,
    pub id: String,
    pub created: u128,
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

const STATES: [&str; 5] = ["pending", "launching", "launched", "failed", "interrupted"];

pub fn dir(install: &Path) -> PathBuf { install.join("setup") }
fn file(install: &Path, id: &str) -> PathBuf { dir(install).join(format!("{id}.json")) }

fn read(path: &Path) -> Res<Request> {
    let text = fs::read_to_string(path).map_err(|e| format!("setup request unknown: {e}"))?;
    let r: Request = serde_json::from_str(&text).map_err(|e| format!("setup request {} unknown: {e}", path.display()))?;
    if r.schema != 2 || !STATES.contains(&r.state.as_str()) || !crate::resources::id(&r.id) || path.file_stem() != Some(r.id.as_ref()) {
        return Err(format!("setup request {} unknown: unsupported record; kept", path.display()));
    }
    Ok(r)
}

/// Every request, oldest first, and the records that read unknown (R12). `request.json` is the pre-#1426 single slot: kept, unread.
pub fn all(install: &Path) -> Res<(Vec<Request>, Vec<String>)> {
    let entries = match fs::read_dir(dir(install)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], vec![])),
        r => r.map_err(|e| format!("setup requests unknown: {e}"))?,
    };
    let (mut ok, mut bad) = (vec![], vec![]);
    for entry in entries {
        let path = entry.map_err(|e| format!("setup requests unknown: {e}"))?.path();
        if path.extension().is_none_or(|x| x != "json") || path.file_name().is_some_and(|n| n == "request.json") { continue; }
        match read(&path) { Ok(r) => ok.push(r), Err(e) => bad.push(e) }
    }
    ok.sort_by(|a: &Request, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
    Ok((ok, bad))
}

fn write(install: &Path, r: &Request) -> Res<()> {
    let text = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    crate::platform::write_atomic(&file(install, &r.id), &(text + "\n")).map_err(|e| format!("setup request write uncertain: {e}"))
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

/// `service setup`: a new `pending` request every call.
pub fn submit(install: &Path, runtime: &str, client: Option<&str>, targets: &[String]) -> Res<Request> {
    if runtime != "claude" { return Err(format!("setup runtime {runtime}: only claude has a qualified visible session")); }
    let targets = targets.iter().map(|t| {
        let p = Path::new(t);
        if !p.is_absolute() { return Err(format!("setup target {t}: expected an absolute path")); }
        p.canonicalize().map(|p| p.display().to_string()).map_err(|e| format!("setup target {t}: {e}"))
    }).collect::<Res<Vec<_>>>()?;
    let id = crate::runtime::uuid();
    let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "setup clock unknown")?.as_millis();
    let platform = format!("{} {}{}", std::env::consts::OS, std::env::consts::ARCH, if cfg!(target_os = "linux") && (std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()) { " wsl" } else { "" });
    let mut r = Request { schema: 2, id: id.clone(), created, host: crate::hostname(), platform, client: client.unwrap_or("Claude Code").into(), runtime: runtime.into(),
        targets, brief: String::new(), state: "pending".into(), job: None, session: None, url: None, reason: None };
    r.brief = brief(&r, &dir(install).join(&id));
    locked(install, || {
        // Same-millisecond calls keep call order: created is strictly after the newest request.
        r.created = all(install)?.0.last().map_or(r.created, |n| r.created.max(n.created + 1));
        write(install, &r)
    })?;
    crate::platform::write_atomic(&install.join("poke"), "").map_err(|e| format!("setup request saved; host notification unknown: {e}"))?;
    Ok(r)
}

/// Service start: a launch without a recorded native answer has an unknown outcome and is never replayed.
pub fn recover(install: &Path) -> Res<()> {
    locked(install, || {
        for mut r in all(install)?.0.into_iter().filter(|r| r.state == "launching") {
            r.state = "interrupted".into();
            r.reason = Some("service stopped during launch; outcome unknown, inspect native sessions before a new setup".into());
            write(install, &r)?;
        }
        Ok(())
    })
}

pub enum Launched { Job { job: String, session: Option<String>, url: Option<String> }, NoStart(String), Unknown(String) }

/// The native call, replaceable in tests.
pub type Launcher = fn(&Path, &Request) -> Launched;

/// One host pass: launch the oldest `pending` request. Setup takes no § 3 grant (owner, #1426): only a hold, Stop or restart waits.
pub fn step(install: &Path, host: &crate::host::Shared, launch: Launcher) -> Res<()> {
    let (requests, bad) = all(install)?;
    let Some(mut r) = requests.into_iter().find(|r| r.state == "pending") else { return bad.into_iter().next().map_or(Ok(()), Err) };
    let waiting = {
        let h = host.lock().unwrap_or_else(|e| e.into_inner());
        h.held.clone().or_else(|| h.stopping.then(|| "host Stop pending".into())).or_else(|| h.restart.then(|| "host restart pending".into()))
    };
    if let Some(reason) = waiting {
        let reason = format!("waiting: {reason}");
        if r.reason.as_deref() == Some(reason.as_str()) { return Ok(()); }
        r.reason = Some(reason);
        return locked(install, || write(install, &r));
    }
    let id = r.id.clone();
    locked(install, || {
        let cur = read(&file(install, &id))?;
        if cur.state != "pending" { return Err("setup request changed before launch".into()); }
        r.state = "launching".into(); r.reason = None;
        write(install, &r)
    })?;
    let workdir = dir(install).join(&id);
    let outcome = match fs::create_dir_all(&workdir) {
        Ok(()) => launch(&workdir, &r),
        Err(e) => Launched::NoStart(format!("setup directory: {e}")),
    };
    match outcome {
        Launched::Job { job, session, url } => { r.state = "launched".into(); r.job = Some(job); r.session = session; r.url = url; }
        Launched::NoStart(e) => { r.state = "failed".into(); r.reason = Some(e); }
        Launched::Unknown(e) => { r.state = "interrupted".into(); r.reason = Some(e); }
    }
    // A lost write leaves `launching`; the next service start turns it into `interrupted`, never a second launch.
    locked(install, || write(install, &r))
}

/// Claude background session with Remote Control (§ 8 table); access and permission mode come from § 8 Access.
pub fn claude_launch(workdir: &Path, r: &Request) -> Launched {
    let bin = crate::runtime::which("claude");
    if !bin.is_absolute() { return Launched::NoStart(format!("{MISSING} claude is not on the host service PATH")); }
    if crate::runtime::login_state("claude", &bin) == "not logged in" { return Launched::NoStart(format!("{LOGIN} Claude Code is not logged in on this host")); }
    let install = workdir.parent().and_then(Path::parent).unwrap_or(workdir);
    let policy = crate::host::Policy::read_for_hostname(install, crate::hostname).ok();
    let mode = policy.as_ref().map_or("dontAsk".to_string(), |p| p.permission_mode.clone());
    let exe = std::env::current_exe().unwrap_or_default();
    let env = crate::runtime::Env { root: workdir, exe: &exe, install, permission_mode: &mode, codex: &[], publish: "", board_env: &[] };
    let access = match crate::runtime::access(&env, &r.runtime) { Ok(a) => a, Err(e) => return Launched::NoStart(e.message) };
    let path = workdir.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) { return Launched::NoStart(format!("setup brief: {e}")); }
    let mut c = match crate::runtime::executable(bin, &access, &env) { Ok(c) => c, Err(e) => return Launched::NoStart(e) };
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
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::write(workdir.join("launch.log"), format!("{stdout}\n{stderr}"));
    // Claude refuses before it starts anything: a definite no-start, never `interrupted`.
    if !out.status.success() && stderr.contains("Workspace not trusted") { return Launched::NoStart(format!("{ACCESS} Claude workspace not trusted: {}", workdir.display())); }
    if !out.status.success() && (stderr.contains("/login") || stderr.contains("Not logged in")) { return Launched::NoStart(format!("{LOGIN} Claude Code asked for login")); }
    match stdout.lines().find_map(|l| l.strip_prefix("backgrounded · ").and_then(|s| s.split_whitespace().next())) {
        // One link read at launch, never after: the Claude session manager owns the session from here.
        Some(id) if out.status.success() && crate::runtime::native_job_id(id) => {
            let session = crate::runtime::claude_agents(workdir).ok()
                .and_then(|rows| rows.iter().find(|v| v["id"] == id).and_then(|v| v["sessionId"].as_str().map(String::from)));
            let url = session.as_deref().and_then(crate::runtime::claude_remote);
            Launched::Job { job: id.into(), session, url }
        }
        _ => Launched::Unknown("Claude returned no background job; outcome unknown, inspect native sessions".into()),
    }
}

const MISSING: &str = "runtime missing:";
const LOGIN: &str = "sign-in needed:";
const ACCESS: &str = "access refused:";

/// What the tray shows (A1): one distinct kind per actionable state and the owner's next step.
pub fn view(r: &Request) -> serde_json::Value {
    let reason = r.reason.as_deref().unwrap_or("");
    let (kind, next) = match r.state.as_str() {
        "pending" if reason.starts_with("waiting:") => ("waiting", "Waits for the hold, Stop or restart named here"),
        "pending" | "launching" => ("starting", "The host service is starting the session"),
        "launched" if r.url.is_some() => ("launched", "Open the session to read and answer; archive it in Claude when done"),
        "launched" => ("launched", "Session started; find it in Claude (`claude agents`), archive it when done"),
        "failed" if reason.starts_with(MISSING) => ("runtime", "Install Claude Code for the service user, then run `taskq service setup`"),
        "failed" if reason.starts_with(LOGIN) => ("login", "Run `claude auth login` as the service user, then `taskq service setup`"),
        "failed" if reason.starts_with(ACCESS) => ("access", "Grant the access named here yourself (taskq never widens it), then `taskq service setup`"),
        "failed" => ("failed", "Read the reason, fix it, then run `taskq service setup`"),
        _ => ("interrupted", "Check `claude agents` for a live session first; only then run `taskq service setup`"),
    };
    serde_json::json!({"id": r.id, "state": r.state, "kind": kind, "next": next, "reason": r.reason, "url": r.url, "host": r.host, "runtime": r.runtime, "session": r.session})
}

/// The tray row: the newest request, or unknown when no request reads and some record is unreadable (R12).
pub fn last(install: &Path) -> serde_json::Value {
    match all(install) {
        Ok((ok, bad)) => ok.last().map(view).or_else(|| bad.first().map(|e| serde_json::json!({"kind": "unknown", "reason": e}))).unwrap_or(serde_json::Value::Null),
        Err(e) => serde_json::json!({"kind": "unknown", "reason": e}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Host, Policy};
    use std::sync::{Arc, Mutex};

    fn ok(_: &Path, _: &Request) -> Launched { Launched::Job { job: "abcd1234".into(), session: None, url: None } }
    fn unknown(_: &Path, _: &Request) -> Launched { Launched::Unknown("timeout".into()) }
    fn never(_: &Path, _: &Request) -> Launched { panic!("setup launched again") }

    fn host(json: serde_json::Value) -> crate::host::Shared { Arc::new(Mutex::new(Host::new(Policy::parse(json, "h")))) }
    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-setup-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }
    fn states(i: &Path) -> Vec<String> { all(i).unwrap().0.into_iter().map(|r| r.state).collect() }

    // Chain: service setup to host launch (A2, A5). Risk: a repeat returns an old session, a request launches twice,
    // or setup waits on (or takes) a queue slot.
    #[test]
    fn each_request_launches_once_outside_capacity() {
        let t = tmp();
        let i = t.as_path();
        let first = submit(i, "claude", None, &[]).unwrap();
        let second = submit(i, "claude", None, &[]).unwrap();
        assert_ne!(first.id, second.id, "every call writes a new request");
        assert!(submit(i, "codex", None, &[]).is_err(), "unqualified runtime refused");
        assert!(first.brief.contains("full PM chat") && first.brief.contains(&first.host));
        // Claude capacity 0, admission.max 0 and an unbuilt occupancy count only queue work.
        let h = host(serde_json::json!({"limits": {"claude": 0}, "admission": {"max": 0}}));
        step(i, &h, ok).unwrap();
        assert_eq!(all(i).unwrap().0[0].id, first.id, "oldest first");
        assert_eq!(states(i), ["launched", "pending"]);
        step(i, &h, ok).unwrap();
        assert_eq!(states(i), ["launched", "launched"]);
        assert!(h.lock().unwrap().used().is_empty(), "setup holds no slot");
        // A restarted service never relaunches a launched request.
        recover(i).unwrap();
        step(i, &h, never).unwrap();
        assert_eq!(last(i)["kind"], "launched");
    }

    // Chain: crash boundary between `launching` and the native answer (A2). Risk: an uncertain launch is replayed.
    #[test]
    fn uncertain_launch_is_interrupted() {
        let t = tmp();
        let i = t.as_path();
        submit(i, "claude", None, &[]).unwrap();
        let h = host(serde_json::json!({}));
        step(i, &h, unknown).unwrap();
        assert_eq!(states(i), ["interrupted"]);
        // Crash after `launching` was written: recovery reads interrupted, the next pass launches nothing.
        let mut r = all(i).unwrap().0.remove(0);
        r.state = "launching".into();
        write(i, &r).unwrap();
        recover(i).unwrap();
        assert_eq!(states(i), ["interrupted"]);
        step(i, &h, never).unwrap();
        // The pre-#1426 slot is kept unread; another unreadable record is unknown, kept and reported.
        fs::write(dir(i).join("request.json"), r#"{"schema":1}"#).unwrap();
        step(i, &h, never).unwrap();
        fs::write(dir(i).join("x.json"), r#"{"schema":3}"#).unwrap();
        assert!(step(i, &h, never).is_err());
        assert_eq!(last(i)["kind"], "interrupted", "the newest readable request still shows");
        fs::remove_file(file(i, &r.id)).unwrap();
        assert_eq!(last(i)["kind"], "unknown");
    }

    // Chain: hold, Stop or restart to launch (A2). Risk: setup launches through an emergency hold.
    #[test]
    fn hold_waits() {
        let t = tmp();
        let i = t.as_path();
        submit(i, "claude", None, &[]).unwrap();
        let h = host(serde_json::json!({}));
        h.lock().unwrap().held = Some("emergency hold".into());
        step(i, &h, never).unwrap();
        assert_eq!((last(i)["kind"].as_str(), last(i)["reason"].as_str()), (Some("waiting"), Some("waiting: emergency hold")));
        h.lock().unwrap().held = None;
        step(i, &h, ok).unwrap();
        assert_eq!(states(i), ["launched"]);
    }

    // Chain: launch outcome to tray view (A4). Risk: missing runtime, login and access read as one generic failure.
    #[test]
    fn view_names_each_actionable_state() {
        let mut r = Request { schema: 2, id: "a".into(), created: 0, host: "h".into(), platform: "p".into(), client: "c".into(), runtime: "claude".into(),
            targets: vec![], brief: String::new(), state: "failed".into(), job: None, session: None, url: None, reason: None };
        let kind = |r: &Request| view(r)["kind"].as_str().unwrap().to_string();
        let mut kinds = vec![];
        for reason in [format!("{MISSING} x"), format!("{LOGIN} x"), format!("{ACCESS} x"), "spawn".into()] { r.reason = Some(reason); kinds.push(kind(&r)); }
        r.state = "interrupted".into(); kinds.push(kind(&r));
        assert_eq!(kinds, ["runtime", "login", "access", "failed", "interrupted"]);
        r.state = "launched".into(); r.reason = None; r.url = Some("https://claude.ai/code/session_x".into());
        assert_eq!((kind(&r).as_str(), view(&r)["url"].as_str()), ("launched", Some("https://claude.ai/code/session_x")));
        assert!(view(&r)["next"].as_str().unwrap().contains("Open the session"));
    }
}
