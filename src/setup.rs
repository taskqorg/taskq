//! Host setup sessions (taskq.md § 4, [detail](../docs/design/standalone-onboarding.md#contract-detail)): each `service setup`
//! writes a new `<install>/setup/<id>.json`; the host service launches each once, then its runtime (the Claude session
//! manager or the Codex app) owns it.
//! Tray AI requests and `taskq retro` take the same path with a project `root` ([AI requests](../docs/design/ai-workspace.md#contract-detail)).
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
    /// The registered project an AI request or retro session runs in; none: a setup session.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub root: Option<String>,
}

const STATES: [&str; 6] = ["pending", "launching", "launched", "failed", "interrupted", "cancelled"];
pub(crate) fn owner_id(id:&str)->bool {crate::resources::id(id)&&!["request","lock"].contains(&id)}

pub fn dir(install: &Path) -> PathBuf { install.join("setup") }
fn file(install: &Path, id: &str) -> PathBuf { dir(install).join(format!("{id}.json")) }

pub(crate) fn read(path: &Path) -> Res<Request> {
    if !crate::messages::plain(path)? {return Err("Setup request absent".into());}
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

pub(crate) fn locked<T>(install: &Path, f: impl FnOnce() -> Res<T>) -> Res<T> {
    crate::messages::plain(&dir(install))?;
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

/// A project session's brief: the owner's request (tray) or the retro task, inside the fixed project frame.
fn project_brief(root: &str, task: &str) -> String {
    format!("You are a TaskQ session opened by the host service because the owner asked for it.
Project: {root}
Read its AGENTS.md and taskq.md first and follow them; you may act as the TaskQ PM for this project.
Confirm concrete external effects with the owner first (taskq.md section 6). Never run gh auth login.
Touch only this project, ~/.claude, ~/.codex and the OS temp dir. Task-state writes use taskq commands.

{task}
")
}

pub const RETRO: &str = "Retrospective (`taskq retro`): read the Proposals section of docs/use-cases/README.md and the board's
recent `ask`, `answer` and `requeue` entries (`taskq status --json`, `taskq inbox N`). Give the owner numbered
recommendations: simpler flows, missing options, quick commands. File each one the owner accepts with `taskq add`.";

/// `service setup`: a new `pending` request every call.
pub fn submit(install: &Path, runtime: &str, client: Option<&str>, targets: &[String]) -> Res<Request> {
    let targets = targets.iter().map(|t| {
        let p = Path::new(t);
        if !p.is_absolute() { return Err(format!("setup target {t}: expected an absolute path")); }
        p.canonicalize().map(|p| p.display().to_string()).map_err(|e| format!("setup target {t}: {e}"))
    }).collect::<Res<Vec<_>>>()?;
    save(install, runtime, client, targets, None, |r, id| brief(r, &dir(install).join(id)))
}

/// Agentq OK: the existing setup writer, with exact displayed targets and this click's retry identity.
pub fn agentq(install:&Path,id:&str,runtime:&str,targets:Vec<String>)->Res<Request> {
    if !crate::resources::id(id) || !crate::runtime::setup_runtimes().any(|rt|rt==runtime) || targets.len()!=2 || targets.iter().any(|p|!Path::new(p).is_absolute()) {return Err("invalid agentq setup scope".into());}
    save_id(install,runtime,None,targets,None,Some((id,false)),|r,id|format!("Set up agentq from https://github.com/taskqorg/agentq on this host now.\nThe owner selected OK in TaskQ after reviewing installation of the verified agentq release, its engines and host service, and supported configuration, hooks and MCP for the selected client only.\nConsent already names these exact displayed effects and targets (including creating the absent agentq install). Do not ask again for these same effects. Ask before other effects, conflicts, legacy removal or missing access.\nDo not change TaskQ configuration, admission, runtime policy or board state.\n{}",brief(r,&dir(install).join(id))))
}

/// Tray AI request or `taskq retro`: a new `pending` session in project `root` with `task` as its brief.
pub fn open(install: &Path, root: &Path, task: &str) -> Res<Request> {
    if task.trim().is_empty() { return Err("Write a request before sending".into()); }
    let root = root.canonicalize().map_err(|e| format!("project {}: {e}", root.display()))?.display().to_string();
    save(install, crate::runtime::OWNER, None, vec![], Some(root.clone()), |_, _| project_brief(&root, task.trim()))
}

/// A workspace retry reads the same owner-session request; its identity never means a second launch.
pub fn open_id(install: &Path, root: &Path, task: &str, id: &str, runtime: &str, read_only: bool) -> Res<Request> {
    if !crate::resources::id(id) { return Err("Invalid owner request ID".into()); }
    if task.trim().is_empty() { return Err("Write a request before sending".into()); }
    let root = root.canonicalize().map_err(|e| format!("project {}: {e}", root.display()))?.display().to_string();
    save_id(install, runtime, None, vec![], Some(root.clone()), Some((id, read_only)), |_, _| project_brief(&root, task))
}

fn save(install: &Path, runtime: &str, client: Option<&str>, targets: Vec<String>, root: Option<String>, brief: impl FnOnce(&Request, &str) -> String) -> Res<Request> {
    save_id(install, runtime, client, targets, root, None, brief)
}

fn save_id(install: &Path, runtime: &str, client: Option<&str>, targets: Vec<String>, root: Option<String>, requested_id: Option<(&str, bool)>, brief: impl FnOnce(&Request, &str) -> String) -> Res<Request> {
    // #1522: a visible Codex session is a Codex app thread, so only a setup request on macOS or Windows takes it.
    let a = crate::runtime::of(runtime);
    if !a.remote && !(a.codex && a.child && root.is_none() && cfg!(any(target_os = "macos", windows))) {
        return Err(format!("setup runtime {runtime}: no qualified visible session here; use claude, or codex on macOS or Windows"));
    }
    let id = requested_id.map(|(id,_)|String::from(id)).unwrap_or_else(crate::runtime::uuid);
    if !owner_id(&id) {return Err("Reserved or invalid owner request ID; use a new ID".into());}
    let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "setup clock unknown")?.as_millis();
    let platform = format!("{} {}{}", std::env::consts::OS, std::env::consts::ARCH, if cfg!(target_os = "linux") && (std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()) { " wsl" } else { "" });
    let mut r = Request { schema: 2, id: id.clone(), created, host: crate::hostname(), platform, client: client.unwrap_or(a.client).into(), runtime: runtime.into(),
        targets, brief: String::new(), state: "pending".into(), job: None, session: None, url: None, reason: None, root };
    r.brief = brief(&r, &id);
    locked(install, || {
        if crate::messages::request_deleted(install,&id)? {return Err("Owner request was permanently deleted; it cannot launch again".into());}
        if requested_id.is_some() {
            match fs::metadata(file(install, &id)) {
                Ok(_) => {
                    let previous = read(&file(install, &id))?;
                    if previous.root != r.root || previous.brief != r.brief || previous.runtime != r.runtime {
                        return Err("Owner request ID already binds different input".into());
                    }
                    r = previous;
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && requested_id.is_some_and(|(_,read_only)|!read_only) => {},
                Err(e) => return Err(format!("Owner request delivery unknown: {e}")),
            }
        }
        // Same-millisecond calls keep call order: created is strictly after the newest request.
        r.created = all(install)?.0.last().map_or(r.created, |n| r.created.max(n.created + 1));
        write(install, &r)
    })?;
    crate::platform::write_atomic(&install.join("poke"), "").map_err(|e| format!("setup request saved; host notification unknown: {e}"))?;
    let observed=read(&file(install,&id))?;
    if observed.root!=r.root || observed.brief!=r.brief || observed.runtime!=r.runtime {return Err("Owner request readback changed; delivery unknown".into());}
    Ok(observed)
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

/// #1580: cancel only a not-yet-dispatched owner request. Launch and cancel share the same lock.
pub(crate) fn cancel_pending(install:&Path,id:&str)->Res<()> {
    if !crate::resources::id(id){return Err("Invalid owner request ID".into());}
    locked(install,||{
        let mut r=read(&file(install,id))?;
        if r.root.is_none(){return Err("This is not a project owner request".into());}
        if r.state=="cancelled" {return Ok(());}
        if r.state!="pending" {return Err("Cancellation unavailable after launch begins; use the native session".into());}
        r.state="cancelled".into();r.reason=Some("Launch cancelled before native dispatch; no runtime work was rolled back".into());write(install,&r)?;
        if read(&file(install,id))?!=r {return Err("Cancellation readback unknown".into());}Ok(())
    })
}

/// The native call with the request's own folder, replaceable in tests.
pub type Launcher = fn(&Path, &Request) -> Launched;

/// One host pass: launch the oldest `pending` request. Owner sessions are for the human (owner, #1426, #1427):
/// no § 3 grant and no hold, Stop or restart waits for it.
pub fn step(install: &Path, launch: Launcher) -> Res<()> {
    let (requests, bad) = all(install)?;
    let Some(mut r) = requests.into_iter().find(|r| r.state == "pending") else { return bad.into_iter().next().map_or(Ok(()), Err) };
    let id = r.id.clone();
    locked(install, || {
        let cur = read(&file(install, &id))?;
        if cur.state != "pending" { return Err("setup request changed before launch".into()); }
        r.state = "launching".into(); r.reason = None;
        write(install, &r)
    })?;
    let own = dir(install).join(&id);
    let outcome = match fs::create_dir_all(&own) {
        Ok(()) => launch(&own, &r),
        Err(e) => Launched::NoStart(format!("setup directory: {e}")),
    };
    match outcome {
        Launched::Job { job, session, url } => { r.state = "launched".into(); r.job = Some(job); r.session = session; r.url = url; }
        Launched::NoStart(e) => { r.state = "failed".into(); r.reason = Some(e); }
        Launched::Unknown(e) => { r.state = "interrupted".into(); r.reason = Some(e); }
    }
    // A lost write leaves `launching`; the next service start turns it into `interrupted`, never a second launch.
    locked(install, || {
        if read(&file(install,&id))?.state!="launching" {return Err("Setup launch record changed; late result not written".into());}
        write(install, &r)
    })
}

/// The request's runtime adapter (§ 8 table).
pub fn launch(own: &Path, r: &Request) -> Launched {
    if crate::runtime::of(&r.runtime).codex { codex_launch(own, r) } else { claude_launch(own, r) }
}

/// The brief is a file in the session folder; the first message only points at it.
fn read_brief(path: &Path) -> String {
    format!("Before any work, use Read to read {} in full. It is this session's brief; follow it.", serde_json::to_string(&path.to_string_lossy()).unwrap_or_default())
}

/// Codex app thread (§ 8 Codex app, #1522): a `codex exec` handshake turn in the session folder creates the thread, then
/// the app shows it and gets the brief. Access is the `codex-app` grant, as for app workers; nothing falls back (R9).
pub fn codex_launch(own: &Path, r: &Request) -> Launched {
    let a = crate::runtime::of(&r.runtime);
    let bin = crate::runtime::which(a.bin);
    if !bin.is_absolute() { return Launched::NoStart(format!("{MISSING} {} is not on the host service PATH", a.bin)); }
    if crate::runtime::login_state(a.bin, &bin) == "not logged in" { return Launched::NoStart(format!("{LOGIN} {} is not logged in on this host", a.client)); }
    let install = own.parent().and_then(Path::parent).unwrap_or(own);
    let codex = crate::host::Policy::read_for_hostname(install, crate::hostname).map_or(vec![], |p| p.codex);
    let exe = std::env::current_exe().unwrap_or_default();
    let env = crate::runtime::Env { root: own, exe: &exe, install, permission_mode: "", codex: &codex, publish: "", board_env: &[] };
    let access = match crate::runtime::access(&env, a.setup) { Ok(a) => a, Err(e) => return Launched::NoStart(e.message) };
    let path = own.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) { return Launched::NoStart(format!("setup brief: {e}")); }
    let mut c = match crate::runtime::cli(&r.runtime, &access, &env) { Ok(c) => c, Err(e) => return Launched::NoStart(format!("Codex launch: {e}")) };
    crate::platform::quiet(&mut c);
    c.current_dir(own).env_remove("CLAUDE_CODE_SESSION_ID").env_remove("CODEX_THREAD_ID")
        .env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT")
        // the session folder is no repository; skipping that check trusts nothing and leaves sandbox and approvals as granted
        .args(["exec", "--json", "--skip-git-repo-check"]).args(access.codex.as_deref().unwrap_or(&codex)).arg("-C").arg(own).arg("-");
    use coreq::process::Failure;
    let out = match coreq::process::exchange("setup.codex-launch", &mut c, crate::runtime::HANDSHAKE.as_bytes(), std::time::Duration::from_secs(60)) {
        Ok(out) | Err(Failure::Exit(out)) => out,
        Err(e @ (Failure::InvalidDeadline | Failure::Capacity | Failure::Owner(_) | Failure::Spawn(_) | Failure::InputLimit)) => return Launched::NoStart(format!("Codex launch: {e}")),
        Err(e) => return Launched::Unknown(format!("Codex launch outcome unknown: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::write(own.join("launch.log"), format!("{stdout}\n{stderr}"));
    codex_thread(&stdout, &stderr, out.status.code(), |thread| crate::runtime::app_submit(thread, &read_brief(&path)))
}

/// The handshake's answer: no thread is a definite no-start; a thread whose brief is not queued is unknown.
fn codex_thread(stdout: &str, stderr: &str, code: Option<i32>, submit: impl FnOnce(&str) -> Res<()>) -> Launched {
    let thread = stdout.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["type"] == "thread.started").and_then(|v| v["thread_id"].as_str().map(String::from));
    let Some(thread) = thread else {
        let text = format!("{stdout}{stderr}").to_lowercase();
        if text.contains("401") || text.contains("login") || text.contains("log in") { return Launched::NoStart(format!("{LOGIN} Codex asked for login")); }
        return Launched::NoStart(format!("Codex handshake started no thread (exit {})", code.map_or("unknown".into(), |c| c.to_string())));
    };
    // the id reaches the app URL and, on Windows, a shell: only a plain id
    if !crate::runtime::plain_thread(&thread) { return Launched::Unknown(format!("Codex returned thread {thread:?}; outcome unknown")); }
    match submit(&thread) {
        Ok(()) => Launched::Job { job: thread.clone(), session: Some(thread.clone()), url: Some(crate::runtime::thread_url(&thread)) },
        Err(e) => Launched::Unknown(format!("{e}; brief delivery unknown, inspect Codex thread {thread}")),
    }
}

/// Claude background session with Remote Control (§ 8 table); access and permission mode come from § 8 Access.
pub fn claude_launch(own: &Path, r: &Request) -> Launched {
    let workdir = r.root.as_deref().map_or(own, Path::new);
    let a = crate::runtime::of(&r.runtime);
    let bin = crate::runtime::which(a.bin);
    if !bin.is_absolute() { return Launched::NoStart(format!("{MISSING} {} is not on the host service PATH", a.bin)); }
    if crate::runtime::login_state(a.bin, &bin) == "not logged in" { return Launched::NoStart(format!("{LOGIN} {} is not logged in on this host", a.client)); }
    let install = own.parent().and_then(Path::parent).unwrap_or(own);
    let policy = crate::host::Policy::read_for_hostname(install, crate::hostname).ok();
    let mode = policy.as_ref().map_or("dontAsk".to_string(), |p| p.permission_mode.clone());
    let exe = std::env::current_exe().unwrap_or_default();
    let env = crate::runtime::Env { root: workdir, exe: &exe, install, permission_mode: &mode, codex: &[], publish: "", board_env: &[] };
    let access = match crate::runtime::access(&env, &r.runtime) { Ok(a) => a, Err(e) => return Launched::NoStart(e.message) };
    let path = own.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) { return Launched::NoStart(format!("setup brief: {e}")); }
    let mut c = match crate::runtime::cli(&r.runtime, &access, &env) { Ok(c) => c, Err(e) => return Launched::NoStart(e) };
    c.current_dir(workdir).env_remove("CLAUDE_CODE_SESSION_ID").env_remove("CODEX_THREAD_ID")
        .env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT")
        .args(["--bg", "--settings", &serde_json::json!({"env": crate::runtime::claude_env(install, None, None)}).to_string(), "--remote-control", if r.root.is_some() { "taskq request" } else { "taskq setup" }, "--permission-mode", access.claude.as_deref().unwrap_or(&mode)]);
    let input = read_brief(&path);
    use coreq::process::Failure;
    let out = match coreq::process::exchange("setup.claude-launch", &mut c, input.as_bytes(), std::time::Duration::from_secs(30)) {
        Ok(out) | Err(Failure::Exit(out)) => out,
        Err(e @ (Failure::InvalidDeadline | Failure::Capacity | Failure::Owner(_) | Failure::Spawn(_) | Failure::InputLimit)) => return Launched::NoStart(format!("Claude launch: {e}")),
        Err(e) => return Launched::Unknown(format!("Claude launch outcome unknown: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::write(own.join("launch.log"), format!("{stdout}\n{stderr}"));
    // Claude refuses before it starts anything: a definite no-start, never `interrupted`.
    if !out.status.success() && stderr.contains("Workspace not trusted") { return Launched::NoStart(format!("{ACCESS} Claude workspace not trusted: {}", workdir.display())); }
    if !out.status.success() && (stderr.contains("/login") || stderr.contains("Not logged in")) { return Launched::NoStart(format!("{LOGIN} Claude Code asked for login")); }
    match stdout.lines().find_map(|l| l.strip_prefix("backgrounded · ").and_then(|s| s.split_whitespace().next())) {
        // One link read at launch, never after: the Claude session manager owns the session from here.
        Some(id) if out.status.success() && crate::runtime::native_job_id(id) => {
            let (session, url) = claude_link(workdir, id);
            Launched::Job { job: id.into(), session, url }
        }
        _ => Launched::Unknown("Claude returned no background job; outcome unknown, inspect native sessions".into()),
    }
}

/// A launched job's native session and Remote Control link, each unknown until Claude publishes it.
pub fn claude_link(workdir: &Path, job: &str) -> (Option<String>, Option<String>) {
    let session = crate::runtime::claude_agents(workdir).ok()
        .and_then(|rows| rows.iter().find(|v| v["id"] == job).and_then(|v| v["sessionId"].as_str().map(String::from)));
    let url = session.as_deref().and_then(crate::runtime::claude_remote);
    (session, url)
}

/// The native read, replaceable in tests.
pub type Linker = fn(&Path, &str) -> (Option<String>, Option<String>);

/// How long after its request a launched session's missing link is still read.
const LINK_WINDOW_MS: u128 = 10 * 60 * 1000;

/// Remote Control publishes the link after `--bg` returns (#1409): later passes fill a missing session or link
/// within the window, then stop; the session itself is never tracked.
pub fn link(install: &Path, now: u128, observe: Linker) -> Res<()> {
    for r in all(install)?.0.into_iter().filter(|r| r.state == "launched" && crate::runtime::of(&r.runtime).remote && r.url.is_none() && now.saturating_sub(r.created) < LINK_WINDOW_MS) {
        let Some(job) = r.job.clone() else { continue };
        let own = r.root.as_deref().map_or_else(|| dir(install).join(&r.id), PathBuf::from);
        let (session, url) = observe(&own, &job);
        if session.is_none() && url.is_none() { continue; }
        locked(install, || {
            let mut cur = read(&file(install, &r.id))?;
            if cur.state != "launched" || cur.job.as_deref() != Some(&job) { return Ok(()); }
            cur.session = cur.session.or_else(|| session.clone());
            cur.url = cur.url.or_else(|| url.clone());
            write(install, &cur)
        })?;
    }
    Ok(())
}

/// Onboard `Skipped:` line. Windows plain ssh cannot follow the nvm4w node symlink, so Claude reads not installed there.
pub fn onboard_skipped(rt: &str, windows_ssh: bool) -> String {
    let ssh = if windows_ssh { "; over plain Windows ssh a symlinked install reads missing, so run onboard through a Scheduled Task first" } else { "" };
    format!("Skipped: {rt} not installed. Next, only to run {rt} here: install it for this user, then run onboard again{ssh}")
}

/// Onboard `Access <scope>:` lines, refusals only (#1555): grants are per folder, so each line names the folder it judged (#1546).
pub fn onboard_access(scope: &str, es: &[crate::policy::Effective]) -> Vec<String> {
    es.iter().filter(|e| e.refused.is_some()).flat_map(|e| {
        // the setup folder is no project root: only a host grant (the default scope) covers it
        let how = if scope == "setup" { String::new() } else { format!(" --scope project` in {scope}; or `--scope host") };
        [format!("Access {scope}: {}", e.line()), format!("Next: grant {0} with `taskq service permissions --runtime {0} --mode <mode>{how}`", e.runtime)]
    }).collect()
}

/// Claude's project key: forward slashes on every platform; a Windows backslash key never matches (#1546).
pub fn claude_key(p: &Path) -> String { p.display().to_string().replace('\\', "/") }

/// The owner step for an untrusted folder that works while other Claude sessions run (#1555): Claude saves its own trust.
pub fn trust_step(folder: &str) -> String { format!("Next: run `claude` in {folder} and accept its trust prompt; other Claude sessions keep running") }

/// `service install` (#1555): the new root's trust, read only; nothing when trusted or Claude has no config here.
pub fn trust_gap(home: &Path, root: &Path) -> Vec<String> {
    let key = claude_key(root);
    let v: serde_json::Value = match fs::read_to_string(home.join(".claude.json")) {
        Ok(t) => match serde_json::from_str(&t) { Ok(v) => v, Err(e) => return vec![format!("Trust: {key} unknown (Claude config unreadable: {e})")] },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
        Err(e) => return vec![format!("Trust: {key} unknown (Claude config unreadable: {e})")],
    };
    // Claude walks parent folders: a trusted parent covers the root
    if root.ancestors().any(|p| v["projects"][claude_key(p)]["hasTrustDialogAccepted"] == true) { return vec![]; }
    vec![format!("Trust: {key} not trusted; Claude workers cannot start here"), trust_step(&key)]
}

/// Onboarding trust (#1546): `<install>/setup` and every registered root get `hasTrustDialogAccepted` in
/// `<home>/.claude.json`. Nothing trusted yet changes only while no Claude runs, after a backup; gap lines only.
pub fn trust(install: &Path, home: &Path, claude_pids: impl FnOnce() -> Res<Vec<String>>) -> Res<Vec<String>> {
    let roots = fs::read_to_string(install.join("projects")).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(String::new()) } else { Err(format!("project registry unreadable: {e}")) })?;
    let keys: Vec<String> = std::iter::once(dir(install)).chain(roots.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from)).map(|p| claude_key(&p)).collect();
    let path = home.join(".claude.json");
    let text = match fs::read_to_string(&path) { Ok(t) => Some(t), Err(e) if e.kind() == std::io::ErrorKind::NotFound => None, Err(e) => return Err(format!("Claude config unreadable: {e}")) };
    let (next, changed) = trusted(text.as_deref(), &keys)?;
    let mut lines = vec![];
    if changed.is_empty() { return Ok(lines); }
    // Claude rewrites the whole file: a running one would drop the change, so the owner lets Claude write it (#1555)
    let pids = claude_pids().map_err(|e| format!("Claude processes unknown ({e}); trust unchanged"))?;
    if !pids.is_empty() {
        return Ok(changed.iter().flat_map(|k| [format!("Trust: {k} not trusted; Claude runs (pid {}), so onboard leaves the file", pids.join(", ")), trust_step(k)]).collect());
    }
    if let Some(t) = &text {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let backup = home.join(format!(".claude.json.bak-onboard-{ms}"));
        // copy keeps the config's owner-only mode; a plain write would leave it world-readable
        fs::copy(&path, &backup).map_err(|e| format!("Claude config backup: {e}; trust unchanged"))?;
        lines.push(format!("Backup: {}", backup.display()));
    }
    let tmp = home.join(".claude.json.onboard-tmp");
    let mut open = fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    if text.is_some() { fs::copy(&path, &tmp).map_err(|e| format!("Claude config write: {e}"))?; }
    std::io::Write::write_all(&mut open.open(&tmp).map_err(|e| format!("Claude config write: {e}"))?, next.as_bytes()).map_err(|e| format!("Claude config write: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Claude config write: {e}"))?;
    let (_, left) = trusted(fs::read_to_string(&path).ok().as_deref(), &keys)?;
    if !left.is_empty() { return Err(format!("Claude config readback: {} not trusted", left.join(", "))); }
    lines.extend(changed.iter().map(|k| format!("Trust: {k} trusted")));
    Ok(lines)
}

/// The config with every key trusted, and the keys that were not.
fn trusted(text: Option<&str>, keys: &[String]) -> Res<(String, Vec<String>)> {
    let mut v: serde_json::Value = match text { Some(t) => serde_json::from_str(t).map_err(|e| format!("Claude config unreadable: {e}"))?, None => serde_json::json!({}) };
    let projects = v.as_object_mut().ok_or("Claude config is not an object")?.entry("projects").or_insert_with(|| serde_json::json!({}));
    let projects = projects.as_object_mut().ok_or("Claude config projects is not an object")?;
    let mut changed = vec![];
    for k in keys {
        let p = projects.entry(k.clone()).or_insert_with(|| serde_json::json!({})).as_object_mut().ok_or(format!("Claude config project {k} is not an object"))?;
        if p.get("hasTrustDialogAccepted") != Some(&serde_json::Value::Bool(true)) {
            p.insert("hasTrustDialogAccepted".into(), true.into());
            changed.push(k.clone());
        }
    }
    Ok((serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?, changed))
}

/// Running Claude processes of this user's machine; an unreadable list is an error, never none (R12).
pub fn claude_pids() -> Res<Vec<String>> {
    let out = if cfg!(windows) {
        std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq claude.exe", "/FO", "CSV", "/NH"]).output()
    } else {
        // ponytail: process name `claude` (native installs); an npm `node` Claude is missed, match the command line if one shows up
        std::process::Command::new("pgrep").args(["-x", crate::runtime::of(crate::runtime::OWNER).bin]).output()
    }.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    if cfg!(windows) {
        if !out.status.success() { return Err(format!("tasklist exit {:?}", out.status.code())); }
        return Ok(text.lines().filter_map(|l| l.split("\",\"").nth(1).filter(|_| l.starts_with("\"claude.exe\"")).map(String::from)).collect());
    }
    match out.status.code() { Some(0) => Ok(text.split_whitespace().map(String::from).collect()), Some(1) => Ok(vec![]), c => Err(format!("pgrep exit {c:?}")) }
}

const MISSING: &str = "runtime missing:";
const LOGIN: &str = "sign-in needed:";
const ACCESS: &str = "access refused:";

/// What the tray shows (A1): one distinct kind per actionable state and the owner's next step.
pub fn view(r: &Request) -> serde_json::Value {
    let reason = r.reason.as_deref().unwrap_or("");
    let again = if r.root.is_some() { "send the request again" } else { "run `taskq service setup`" };
    let a = crate::runtime::of(&r.runtime);
    let (name, login, sessions) = (a.client, a.login, a.sessions);
    let (kind, next) = match r.state.as_str() {
        "pending" | "launching" => ("starting", "The host service is starting the session".into()),
        "cancelled" => ("cancelled", "Launch cancelled before native dispatch; continue with a new request".into()),
        "launched" if r.url.is_some() => ("launched", format!("Open the session to read and answer; archive it in {name} when done")),
        "launched" => ("launched", format!("Session started; find it in {sessions}, archive it when done")),
        "failed" if reason.starts_with(MISSING) => ("runtime", format!("Install {name} for the service user, then {again}")),
        "failed" if reason.starts_with(LOGIN) => ("login", format!("Run `{login}` as the service user, then {again}")),
        "failed" if reason.starts_with(ACCESS) => ("access", format!("Grant the access named here yourself (taskq never widens it), then {again}")),
        "failed" => ("failed", format!("Read the reason, fix it, then {again}")),
        _ => ("interrupted", format!("Check {sessions} for a live session first; only then {again}")),
    };
    serde_json::json!({"id": r.id, "state": r.state, "kind": kind, "next": next, "reason": r.reason, "url": r.url, "host": r.host, "runtime": r.runtime, "session": r.session})
}

/// The tray row: the newest setup (`project` false) or project request, or unknown when none reads and some record is unreadable (R12).
pub fn last(install: &Path, project: bool) -> serde_json::Value {
    match all(install) {
        Ok((ok, bad)) => ok.iter().rfind(|r| r.root.is_some() == project).map(view).or_else(|| bad.first().map(|e| serde_json::json!({"kind": "unknown", "reason": e}))).unwrap_or(serde_json::Value::Null),
        Err(e) => serde_json::json!({"kind": "unknown", "reason": e}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(_: &Path, _: &Request) -> Launched { Launched::Job { job: "abcd1234".into(), session: None, url: None } }
    fn unknown(_: &Path, _: &Request) -> Launched { Launched::Unknown("timeout".into()) }
    fn never(_: &Path, _: &Request) -> Launched { panic!("setup launched again") }

    // #1464 A5: a duplicate/crossed request never creates a second session, even after service launch.
    #[test]
    fn owner_request_retries_read_the_same_record_and_refuse_changed_input() {
        let install=std::env::temp_dir().join(format!("taskq-request-id-{}",crate::runtime::uuid()));
        fs::create_dir_all(&install).unwrap();
        let first=open_id(&install,&install,"literal request","owner-1464","claude",false).unwrap();
        assert_eq!(open_id(&install,&install,"literal request","owner-1464","claude",true).unwrap(),first);
        step(&install,ok).unwrap();
        let observed=open_id(&install,&install,"literal request","owner-1464","claude",true).unwrap();
        assert_eq!(observed.state,"launched");
        assert_eq!(all(&install).unwrap().0.len(),1);
        for (text,id,rt) in [("changed","owner-1464","claude"),(" literal request","owner-1464","claude"),("literal request","../escape","claude"),("literal request","owner-1464","codex")] {
            assert!(open_id(&install,&install,text,id,rt,false).is_err());
        }
        assert_eq!(all(&install).unwrap().0.len(),1);
        fs::write(file(&install,"owner-1464"),"corrupt record").unwrap();
        assert!(open_id(&install,&install,"literal request","owner-1464","claude",true).is_err());
        assert_eq!(fs::read_to_string(file(&install,"owner-1464")).unwrap(),"corrupt record");
        fs::remove_file(file(&install,"owner-1464")).unwrap();
        assert!(open_id(&install,&install,"literal request","owner-1464","claude",true).is_err(),"missing readback must never recreate a request");
        assert!(all(&install).unwrap().0.is_empty());
        fs::remove_dir_all(install).unwrap();
    }

    // #1409: a Mac launch recorded `launched` with null session and url; Remote Control publishes the link later.
    // Risk: no Open session, a link written to another job, or rereads that never stop.
    #[test]
    fn later_passes_fill_a_missing_link_within_the_window() {
        fn none(_: &Path, _: &str) -> (Option<String>, Option<String>) { (None, None) }
        fn found(_: &Path, job: &str) -> (Option<String>, Option<String>) {
            assert_eq!(job, "abcd1234");
            (Some("abcd1234-0000-0000-0000-000000000000".into()), Some("https://claude.ai/code/session_x".into()))
        }
        fn never(_: &Path, _: &str) -> (Option<String>, Option<String>) { panic!("read outside the window") }
        let t = tmp();
        let r = submit(&t, "claude", None, &[]).unwrap();
        step(&t, ok).unwrap();
        link(&t, r.created, none).unwrap();
        assert_eq!(read(&file(&t, &r.id)).unwrap().url, None);
        link(&t, r.created + 1000, found).unwrap();
        let cur = read(&file(&t, &r.id)).unwrap();
        assert_eq!((cur.session.as_deref(), cur.url.as_deref()), (Some("abcd1234-0000-0000-0000-000000000000"), Some("https://claude.ai/code/session_x")));
        assert_eq!(view(&cur)["kind"], "launched");
        link(&t, r.created + 2000, never).unwrap();
        let late = submit(&t, "claude", None, &[]).unwrap();
        step(&t, ok).unwrap();
        link(&t, late.created + LINK_WINDOW_MS, never).unwrap();
        fs::remove_dir_all(t).unwrap();
    }

    // #1391 A3: transport retry is one request; a new OK is another session; every brief retains selected scope.
    #[test]
    fn agentq_clicks_reuse_shared_setup_without_replaying_installation() {
        let i=tmp();let targets=vec![i.join("agentq").display().to_string(),i.join(".claude").display().to_string()];
        let a=agentq(&i,"click-one","claude",targets.clone()).unwrap();
        assert_eq!(agentq(&i,"click-one","claude",targets.clone()).unwrap(),a);
        assert!(a.brief.contains("Consent already names these exact displayed effects"));
        assert!(a.brief.contains("Do not kill or restart existing agent sessions"));
        assert!(a.brief.contains("Selected client: Claude Code"));
        assert_eq!(a.targets,targets);assert!(a.root.is_none());
        agentq(&i,"click-two","claude",a.targets.clone()).unwrap();
        assert_eq!(all(&i).unwrap().0.len(),2);
        assert!(agentq(&i,"click-one","claude",vec![i.display().to_string(),i.display().to_string()]).is_err());
        fs::remove_dir_all(i).unwrap();
    }

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-setup-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }

    // Risk (PM run on FRA): an Access line without its folder reads as a host refusal while projects hold grants;
    // Windows ssh reads Claude missing with no hint to use a Scheduled Task.
    #[test]
    fn onboard_lines_name_their_folder_and_the_windows_ssh_case() {
        let (install, root) = (tmp(), tmp());
        let refused = crate::policy::resolve(&Err("x".into()), &crate::policy::Host::here(&install, &root, &install), "codex", "", &[]);
        let ok = crate::policy::Effective { refused: None, ..refused.clone() };
        let r = root.display().to_string();
        // A2 (#1555): a working runtime prints nothing
        let lines = onboard_access(&r, &[refused.clone(), ok]);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with(&format!("Access {r}: ")), "{lines:?}");
        assert!(lines[1].contains("--scope project` in ") && lines[1].contains(&r), "{lines:?}");
        let setup = onboard_access("setup", &[refused]);
        assert!(setup[0].starts_with("Access setup: ") && !setup[1].contains("--scope project"), "{setup:?}");
        assert!(onboard_skipped("claude", true).contains("Scheduled Task"));
        assert!(!onboard_skipped("claude", false).contains("Scheduled Task"));
    }

    #[test]
    fn onboarding_trusts_forward_slash_keys_once_with_no_claude_running() {
        let (install, home) = (tmp(), tmp());
        fs::write(install.join("projects"), "C:\\Users\\a\\p\n/srv/q\n").unwrap();
        // the hand-made backslash key never matched on Windows; it stays as it was
        let old = r#"{"numStartups": 3, "projects": {"C:\\Users\\a\\p": {"hasTrustDialogAccepted": true}, "/srv/q": {"allowedTools": ["x"]}}}"#;
        fs::write(home.join(".claude.json"), old).unwrap();
        #[cfg(unix)]
        fs::set_permissions(home.join(".claude.json"), std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        let setup = claude_key(&dir(&install));
        assert!(!setup.contains('\\'));
        // #1555: a running Claude leaves the file; each untrusted key names the step that needs no Claude to close
        let gaps = trust(&install, &home, || Ok(vec!["42".into()])).unwrap();
        let want: Vec<String> = [setup.as_str(), "C:/Users/a/p", "/srv/q"].iter()
            .flat_map(|k| [format!("Trust: {k} not trusted; Claude runs (pid 42), so onboard leaves the file"), trust_step(k)]).collect();
        assert_eq!(gaps, want);
        assert!(trust(&install, &home, || Err("no tasklist".into())).unwrap_err().contains("unknown"));
        assert_eq!(fs::read_to_string(home.join(".claude.json")).unwrap(), old);

        let lines = trust(&install, &home, || Ok(vec![])).unwrap();
        assert!(lines.iter().any(|l| l.starts_with("Backup: ")), "{lines:?}");
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
        for k in [setup.as_str(), "C:/Users/a/p", "/srv/q"] { assert_eq!(v["projects"][k]["hasTrustDialogAccepted"], true, "{k}"); }
        assert_eq!(v["projects"]["/srv/q"]["allowedTools"][0], "x");
        assert_eq!(v["numStartups"], 3);
        let backup = fs::read_dir(&home).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with(".claude.json.bak-onboard-")).unwrap();
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), old);
        // the config may hold account data: neither file opens to other users
        #[cfg(unix)]
        for p in [home.join(".claude.json"), backup.path()] { assert_eq!(std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&p).unwrap().permissions()) & 0o077, 0, "{}", p.display()); }

        // idempotent: nothing to change reads no processes and writes nothing
        // A2 (#1555): a working host prints no trust line
        assert!(trust(&install, &home, || panic!("process list read with nothing to change")).unwrap().is_empty());
    }

    // Risk (#1555, pilotq#3): `service install` registered a root Claude did not trust and said nothing.
    #[test]
    fn install_reports_an_untrusted_root_and_honours_a_trusted_parent() {
        let home = tmp();
        let root = Path::new("/srv/p/new");
        assert!(trust_gap(&home, root).is_empty(), "no Claude config: Claude is not used here");
        fs::write(home.join(".claude.json"), r#"{"projects": {"/srv/q": {"hasTrustDialogAccepted": true}}}"#).unwrap();
        assert_eq!(trust_gap(&home, root), ["Trust: /srv/p/new not trusted; Claude workers cannot start here".to_string(), trust_step("/srv/p/new")]);
        fs::write(home.join(".claude.json"), r#"{"projects": {"/srv/p": {"hasTrustDialogAccepted": true}}}"#).unwrap();
        assert!(trust_gap(&home, root).is_empty(), "a trusted parent covers the root");
        fs::write(home.join(".claude.json"), "{").unwrap();
        assert!(trust_gap(&home, root)[0].contains("unknown"), "unreadable config is unknown, never trusted");
    }
    fn states(i: &Path) -> Vec<String> { all(i).unwrap().0.into_iter().map(|r| r.state).collect() }

    // Chain: service setup to host launch (A2). Risk: a repeat returns an old session or a request launches twice.
    #[test]
    fn each_request_launches_once() {
        let t = tmp();
        let i = t.as_path();
        let first = submit(i, "claude", None, &[]).unwrap();
        let second = submit(i, "claude", None, &[]).unwrap();
        assert_ne!(first.id, second.id, "every call writes a new request");
        assert!(submit(i, "gemini", None, &[]).is_err(), "unqualified runtime refused");
        // #1522: Codex setup is a Codex app thread, so only native macOS and Windows take it.
        let codex = submit(i, "codex", None, &[]);
        assert_eq!(codex.is_ok(), cfg!(any(target_os = "macos", windows)));
        if let Ok(c) = codex { assert_eq!((c.client.as_str(), first.client.as_str()), ("Codex", "Claude Code")); fs::remove_file(file(i, &c.id)).unwrap(); }
        assert!(first.brief.contains("full PM chat") && first.brief.contains(&first.host));
        step(i, ok).unwrap();
        assert_eq!(all(i).unwrap().0[0].id, first.id, "oldest first");
        assert_eq!(states(i), ["launched", "pending"]);
        step(i, ok).unwrap();
        assert_eq!(states(i), ["launched", "launched"]);
        // A restarted service never relaunches a launched request.
        recover(i).unwrap();
        step(i, never).unwrap();
        assert_eq!(last(i, false)["kind"], "launched");
    }

    // Chain: crash boundary between `launching` and the native answer (A2). Risk: an uncertain launch is replayed.
    #[test]
    fn uncertain_launch_is_interrupted() {
        let t = tmp();
        let i = t.as_path();
        submit(i, "claude", None, &[]).unwrap();
        step(i, unknown).unwrap();
        assert_eq!(states(i), ["interrupted"]);
        // Crash after `launching` was written: recovery reads interrupted, the next pass launches nothing.
        let mut r = all(i).unwrap().0.remove(0);
        r.state = "launching".into();
        write(i, &r).unwrap();
        recover(i).unwrap();
        assert_eq!(states(i), ["interrupted"]);
        step(i, never).unwrap();
        // The pre-#1426 slot is kept unread; another unreadable record is unknown, kept and reported.
        fs::write(dir(i).join("request.json"), r#"{"schema":1}"#).unwrap();
        step(i, never).unwrap();
        fs::write(dir(i).join("x.json"), r#"{"schema":3}"#).unwrap();
        assert!(step(i, never).is_err());
        assert_eq!(last(i, false)["kind"], "interrupted", "the newest readable request still shows");
        fs::remove_file(file(i, &r.id)).unwrap();
        assert_eq!(last(i, false)["kind"], "unknown");
    }

    // Chain: host policy and holds to launch (#1426 A5, #1427 A4). Risk: a session the owner opened (setup or AI request)
    // waits on queue limits or holds. `step` takes no host state, so Claude capacity 0, the emergency hold, Stop and restart cannot reach it.
    #[test]
    fn limits_and_holds_do_not_wait() {
        let t = tmp();
        let i = t.as_path();
        fs::write(i.join("host.json"), r#"{"limits":{"claude":0},"admission":{"max":0}}"#).unwrap();
        fs::write(i.join("hold"), "emergency").unwrap();
        fs::write(i.join("restart"), "").unwrap();
        submit(i, "claude", None, &[]).unwrap();
        let ai = open(i, i, "  Why is nothing starting?  ").unwrap();
        step(i, ok).unwrap();
        step(i, ok).unwrap();
        assert_eq!(states(i), ["launched", "launched"]);
        assert_eq!(last(i, true)["id"], ai.id.as_str(), "the AI request row shows the project request, not setup");
        assert!(ai.brief.contains("\nWhy is nothing starting?\n") && ai.root.is_some());
        assert!(open(i, i, " ").is_err(), "empty request writes nothing");
        assert_eq!(states(i).len(), 2);
    }

    // Chain: launch outcome to tray view (A4). Risk: missing runtime, login and access read as one generic failure.
    #[test]
    fn view_names_each_actionable_state() {
        let mut r = Request { schema: 2, id: "a".into(), created: 0, host: "h".into(), platform: "p".into(), client: "c".into(), runtime: "claude".into(),
            targets: vec![], brief: String::new(), state: "failed".into(), job: None, session: None, url: None, reason: None, root: None };
        let kind = |r: &Request| view(r)["kind"].as_str().unwrap().to_string();
        let mut kinds = vec![];
        for reason in [format!("{MISSING} x"), format!("{LOGIN} x"), format!("{ACCESS} x"), "spawn".into()] { r.reason = Some(reason); kinds.push(kind(&r)); }
        r.state = "interrupted".into(); kinds.push(kind(&r));
        assert_eq!(kinds, ["runtime", "login", "access", "failed", "interrupted"]);
        r.state = "launched".into(); r.reason = None; r.url = Some("https://claude.ai/code/session_x".into());
        assert_eq!((kind(&r).as_str(), view(&r)["url"].as_str()), ("launched", Some("https://claude.ai/code/session_x")));
        assert!(view(&r)["next"].as_str().unwrap().contains("Open the session"));
        // #1522: a Codex request names Codex steps, never Claude ones.
        r.runtime = "codex".into(); r.state = "failed".into(); r.url = None;
        for (reason, step) in [(MISSING, "Install Codex"), (LOGIN, "`codex login`")] {
            r.reason = Some(format!("{reason} x"));
            assert!(view(&r)["next"].as_str().unwrap().contains(step));
        }
    }

    // #1522 A4: the Codex handshake answer to the record. Risk: a login prompt reads as a generic failure, a thread whose
    // brief was not queued reads as launched or is retried, or an unsafe id reaches the app URL.
    #[test]
    fn codex_outcomes_stay_distinct() {
        let started = r#"{"type":"thread.started","thread_id":"019a-b2"}"#;
        let queued = |t: &str| { assert_eq!(t, "019a-b2"); Ok(()) };
        let refused = |_: &str| -> Res<()> { Err("codex queue failed for thread 019a-b2".into()) };
        let never = |_: &str| -> Res<()> { panic!("no thread to submit") };
        let Launched::Job { job, session, url } = codex_thread(started, "", Some(0), queued) else { panic!("thread not launched") };
        assert_eq!((job.as_str(), session.as_deref(), url.as_deref()), ("019a-b2", Some("019a-b2"), Some("codex://threads/019a-b2")));
        assert!(matches!(codex_thread("", "Error: 401 Unauthorized", Some(1), never), Launched::NoStart(r) if r.starts_with(LOGIN)));
        assert!(matches!(codex_thread("", "model overloaded", Some(1), never), Launched::NoStart(r) if !r.starts_with(LOGIN)));
        assert!(matches!(codex_thread(started, "", Some(0), refused), Launched::Unknown(r) if r.contains("019a-b2")));
        assert!(matches!(codex_thread(r#"{"type":"thread.started","thread_id":"x&calc"}"#, "", Some(0), never), Launched::Unknown(_)));
    }
}
