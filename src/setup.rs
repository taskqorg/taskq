//! Host setup sessions (taskq.md § 4, [detail](../docs/design/standalone-onboarding.md#contract-detail)): each `service setup`
//! writes a new `<install>/setup/<id>.json`; the host service launches each once, then its runtime (the Claude session
//! manager or the Codex app) owns it.
//! Tray AI requests and `taskq retro` take the same path with a project `root` ([AI requests](../docs/design/ai-workspace.md#contract-detail)).
//! A crash between `launching` and the native answer reads `interrupted`; nothing relaunches.

use crate::runtime::Cap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;
pub use crate::runtime::owner::{claude_link, launch, Launched};
use crate::runtime::owner::{ACCESS, LOGIN, MISSING};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default,skip_serializing_if="Option::is_none")] pub choice:Option<crate::runtime::choices::Choice>,
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub pm: bool,
}

const STATES: [&str; 6] = ["pending", "launching", "launched", "failed", "interrupted", "cancelled"];
pub(crate) fn owner_id(id:&str)->bool {crate::resources::id(id)&&!["request","lock"].contains(&id)}

pub fn dir(install: &Path) -> PathBuf { install.join("setup") }
fn file(install: &Path, id: &str) -> PathBuf { dir(install).join(format!("{id}.json")) }

pub(crate) fn read(path: &Path) -> Res<Request> {
    let mut pending=path.as_os_str().to_os_string();pending.push(".pending");
    match fs::symlink_metadata(PathBuf::from(pending)) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        _=>return Err("Owner request durable write interrupted; files kept".into()),
    }
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
    if r.pm {return coreq::write_durable(&file(install,&r.id),&(text+"\n")).map_err(|_|"PM native request write unknown; reconcile only".into());}
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
Touch only the setup working directory, approved targets, the runtime-owned folders in taskq.md section 8 and the OS temp dir.
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
Touch only this project, the runtime-owned folders in taskq.md section 8 and the OS temp dir. Task-state writes use taskq commands.

{task}
")
}

fn conversation_brief(id: &str) -> String {
    format!("Originating TaskQ chat: {id}. When filing work from this chat, use taskq add --conversation {id} --op-id pm.{id}.<stable-request-key> with the ordinary title, goal and acceptance. Reconcile uncertain creation with the identical operation; never invent another ID. Present the held brief here and wait for the owner to Assign it; do not start chat-origin tasks from prose. Assignment uses the board Start receipt. Relay questions and outcomes here. Never infer task state from your own prose.")
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
    save_id(install,runtime,None,targets,None,Some((id,false)),false,|r,id|format!("Set up agentq from https://github.com/taskqorg/agentq on this host now.\nThe owner selected OK in TaskQ after reviewing installation of the verified agentq release, its engines and host service, and supported configuration, hooks and MCP for the selected client only.\nConsent already names these exact displayed effects and targets (including creating the absent agentq install). Do not ask again for these same effects. Ask before other effects, conflicts, legacy removal or missing access.\nDo not change TaskQ configuration, admission, runtime policy or board state.\n{}",brief(r,&dir(install).join(id))))
}

/// Tray AI request or `taskq retro`: a new `pending` session in project `root` with `task` as its brief.
pub fn open(install: &Path, root: &Path, task: &str) -> Res<Request> {
    if task.trim().is_empty() { return Err("Write a request before sending".into()); }
    let root = root.canonicalize().map_err(|e| format!("project {}: {e}", root.display()))?.display().to_string();
    save(install, crate::runtime::OWNER, None, vec![], Some(root.clone()), |r, _| format!("{}\n{}", project_brief(&root, task.trim()), conversation_brief(&r.id)))
}

/// A workspace retry reads the same owner-session request; its identity never means a second launch.
pub fn open_id(install: &Path, root: &Path, task: &str, id: &str, runtime: &str, read_only: bool) -> Res<Request> {
    if !crate::resources::id(id) { return Err("Invalid owner request ID".into()); }
    if task.trim().is_empty() { return Err("Write a request before sending".into()); }
    let root = root.canonicalize().map_err(|e| format!("project {}: {e}", root.display()))?.display().to_string();
    save_id_choice(install, runtime, None, vec![], Some(root.clone()), Some((id, read_only)), false, None, |r, _| format!("{}\n{}", project_brief(&root, task), conversation_brief(&r.id)))
}

pub(crate) fn open_choice(install:&Path,root:&Path,task:&str,id:&str,runtime:&str,read_only:bool,choice:Option<&crate::runtime::choices::Choice>)->Res<Request> {
    let root=root.canonicalize().map_err(|_|"Launch project unavailable")?.display().to_string();
    save_id_choice(install,runtime,None,vec![],Some(root.clone()),Some((id,read_only)),false,choice.cloned(),|r,_|format!("{}\n{}",project_brief(&root,task),conversation_brief(&r.id)))
}

pub(crate) fn open_pm(install:&Path,root:&Path,task:&str,id:&str,runtime:&str)->Res<Request> {
    let root=root.canonicalize().map_err(|_|"PM project unavailable")?.display().to_string();
    save_id(install,runtime,None,vec![],Some(root.clone()),Some((id,false)),true,|r,_|format!("{}\n{}",project_brief(&root,task),conversation_brief(&r.id)))
}

fn save(install: &Path, runtime: &str, client: Option<&str>, targets: Vec<String>, root: Option<String>, brief: impl FnOnce(&Request, &str) -> String) -> Res<Request> {
    save_id(install, runtime, client, targets, root, None, false, brief)
}

fn save_id(install: &Path, runtime: &str, client: Option<&str>, targets: Vec<String>, root: Option<String>, requested_id: Option<(&str, bool)>, pm:bool, brief: impl FnOnce(&Request, &str) -> String) -> Res<Request> {
    save_id_choice(install,runtime,client,targets,root,requested_id,pm,None,brief)
}

fn save_id_choice(install:&Path,runtime:&str,client:Option<&str>,targets:Vec<String>,root:Option<String>,requested_id:Option<(&str,bool)>,pm:bool,choice:Option<crate::runtime::choices::Choice>,brief:impl FnOnce(&Request,&str)->String)->Res<Request> {
    // #1522: a visible Codex session is a Codex app thread, so requests take it only on macOS or Windows.
    let a = crate::runtime::of(runtime);
    if !crate::runtime::owner::supported(runtime) {
        return Err(format!("setup runtime {runtime}: no qualified visible session here; use an installed supported owner-session runtime"));
    }
    let id = requested_id.map(|(id,_)|String::from(id)).unwrap_or_else(crate::runtime::uuid);
    if !owner_id(&id) {return Err("Reserved or invalid owner request ID; use a new ID".into());}
    let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "setup clock unknown")?.as_millis();
    let platform = format!("{} {}{}", std::env::consts::OS, std::env::consts::ARCH, if cfg!(target_os = "linux") && (std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()) { " wsl" } else { "" });
    let mut r = Request { schema: 2, id: id.clone(), created, host: crate::config::hostname(), platform, client: client.unwrap_or(a.client).into(), runtime: runtime.into(),
        targets, brief: String::new(), state: "pending".into(), job: None, session: None, url: None, reason: None, root, pm, choice };
    r.brief = brief(&r, &id);
    locked(install, || {
        if crate::messages::request_deleted(install,&id)? {return Err("Owner request was permanently deleted; it cannot launch again".into());}
        if requested_id.is_some() {
            match fs::metadata(file(install, &id)) {
                Ok(_) => {
                    let previous = read(&file(install, &id))?;
                    if previous.root != r.root || previous.brief != r.brief || previous.runtime != r.runtime || previous.pm != r.pm || previous.choice != r.choice {
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
    if observed.root!=r.root || observed.brief!=r.brief || observed.runtime!=r.runtime || observed.choice!=r.choice {return Err("Owner request readback changed; delivery unknown".into());}
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
    let intent=crate::pm::launch_guard(install, &r.clone(), || locked(install, || {
        let cur = read(&file(install, &id))?;
        if cur.state != "pending" { return Err("setup request changed before launch".into()); }
        r.state = "launching".into(); r.reason = None;
        write(install, &r)
    }));
    if let Err(e)=intent {
        if !r.pm {return Err(e);}
        return locked(install,|| {
            let mut cur=read(&file(install,&id))?;
            if cur.state=="pending" {cur.state=if e=="PM launch authority expired, revoked or stale; request kept" {"failed"}else{"interrupted"}.into();cur.reason=Some("PM launch not dispatched: authority or storage unavailable; reconcile only".into());write(install,&cur)?;}
            Ok(())
        });
    }
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

/// The native read, replaceable in tests.
pub type Linker = fn(&Path, &str) -> (Option<String>, Option<String>);

/// How long after its request a launched session's missing link is still read.
const LINK_WINDOW_MS: u128 = 10 * 60 * 1000;

/// Remote Control publishes the link after `--bg` returns (#1409): later passes fill a missing session or link
/// within the window, then stop; the session itself is never tracked.
pub fn link(install: &Path, now: u128, observe: Linker) -> Res<()> {
    for r in all(install)?.0.into_iter().filter(|r| r.state == "launched" && crate::runtime::of(&r.runtime).has(Cap::RemoteControl) && r.url.is_none() && now.saturating_sub(r.created) < LINK_WINDOW_MS) {
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
#[path = "setup_tests.rs"]
mod tests;
