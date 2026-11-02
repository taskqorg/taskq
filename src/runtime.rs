//! Runtimes: stdin briefs or file-backed Claude background input (taskq.md section 8).

pub mod protected_app_server;

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

#[cfg(any(target_os="macos",windows))]
mod app_control;
pub mod accounts;
pub mod owner;

#[cfg(windows)]
pub mod windows_process;

thread_local! {static OBSERVATION_UNTIL:std::cell::Cell<Option<std::time::Instant>>=const {std::cell::Cell::new(None)};}

/// Share the service's remaining observation time across an adapter's existing bounded calls.
pub fn observe_until<T>(until:std::time::Instant, action:impl FnOnce()->T)->T {
    struct Restore(Option<std::time::Instant>);
    impl Drop for Restore {fn drop(&mut self) {OBSERVATION_UNTIL.set(self.0);}}
    let previous=OBSERVATION_UNTIL.get();
    OBSERVATION_UNTIL.set(Some(previous.map_or(until,|outer|outer.min(until))));
    let _restore=Restore(previous);action()
}
fn observation_deadline(budget:std::time::Duration)->std::time::Instant {
    let until=std::time::Instant::now()+budget;
    OBSERVATION_UNTIL.get().map_or(until,|outer|outer.min(until))
}
fn observation_left(budget:std::time::Duration)->std::time::Duration {
    observation_deadline(budget).saturating_duration_since(std::time::Instant::now())
}

fn app_cancel(thread:&str,turn:&str)->Result<(),String> {
    #[cfg(any(target_os="macos",windows))] {app_control::cancel(thread,turn)}
    #[cfg(not(any(target_os="macos",windows)))] {let _=(thread,turn);Err("Codex App native cancellation unavailable on this platform; execution remains reserved".into())}
}

pub struct Launch {
    /// §5: verified attachment paths prepared from accepted board replies by the service.
    pub files: String,
    /// Exact board intent, assigned by the service before native submission; absent for non-task probes.
    pub execution: Option<u64>,
    pub task: u64,
    pub title: String,
    pub attempt: u64,
    pub role: String,
    pub rt: String,
    pub session: String,
    /// The issue URL, for the `Task:` line of commits.
    pub url: String,
    /// None: a new session with the brief; Some: resume the session with this text.
    pub resume: Option<String>,
}

/// R3 session name: `<role> #<N> <title cut to 40 characters>`.
pub fn session_name(l: &Launch) -> String {
    let role = match l.role.as_str() { "worker" => "work", "reviewer" => "review", _ => "setup" };
    let title: String = l.title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect();
    format!("{role} #{} {}", l.task, title.trim_end())
}

/// A running child; a trait so the service can be tested with fake processes.
pub trait Proc {
    fn job(&self)->Option<String> {None}
    /// Save missing native recovery identity after launch without surrendering the handle on failure.
    fn checkpoint(&mut self, _cfg:&crate::service::Config, _launch:&Launch, _execution:u64)->Result<(),String> { Err("native recovery checkpoint unsupported".into()) }
    fn native_identity(&self)->Result<coreq::process::Identity,String> { Err("native process identity unavailable for handover".into()) }
    fn input_offset(&self) -> Result<u64,String> { Err("native input boundary unavailable for handover".into()) }
    /// Exact input was accepted by the native runtime; a handle or process exit alone is not evidence.
    fn accepted(&mut self) -> Result<bool, String> { Err("native input acceptance unavailable; reconcile this execution".into()) }
    /// The exit code once the child has exited; `Err` is an unknown read (§ 4 reconciliation), never an exit.
    fn exited(&mut self) -> Result<Option<i32>, String>;
    fn exit_error(&self) -> Option<&'static str> { None }
    /// Definitive native pre-submission refusal, only after independently observed nonzero exit.
    fn refused(&self, _task:u64, _attempt:u64, _execution:u64, _code:i32) -> Option<String> { None }
    /// Request termination; success is not exit. The owner retains this handle until `exited` confirms it.
    fn kill(&mut self) -> Result<(), String>;
    fn id(&self) -> u32;
}

/// One owned stop attempt; a pending/failed read never releases the caller's process or capacity.
pub fn stop(proc: &mut dyn Proc) -> Result<i32, String> {
    coreq::telemetry::capture::operation("runtime.stop", || {
        proc.kill()?;
        proc.exited()?.ok_or_else(|| "stop pending; process exit unconfirmed".into())
    }, |_| "stop-unconfirmed")
}

impl Proc for Child {
    fn native_identity(&self)->Result<coreq::process::Identity,String> {coreq::process::identity(Child::id(self)).map_err(|e|e.to_string())?.ok_or("native process already exited".into())}
    fn exited(&mut self) -> Result<Option<i32>, String> {
        match self.try_wait() {
            Ok(s) => Ok(s.map(|s| s.code().unwrap_or(-1))),
            Err(e) => Err(format!("wait failed: {e}")),
        }
    }
    fn kill(&mut self) -> Result<(), String> {
        if self.exited()?.is_some() { return Ok(()); }
        // Windows: the child is `cmd.exe` running `codex.cmd`; node and `codex.exe` below it survive a plain kill (#933)
        #[cfg(windows)]
        coreq::process::output("runtime.taskkill", crate::platform::command("taskkill").args(["/F", "/T", "/PID", &Child::id(self).to_string()]), observation_left(std::time::Duration::from_secs(10))).map_err(|e| format!("process tree stop unconfirmed: {e}"))?;
        #[cfg(not(windows))]
        Child::kill(self).map_err(|e| format!("process stop unconfirmed: {e}"))?;
        Ok(())
    }
    fn id(&self) -> u32 {
        Child::id(self)
    }
}

/// A process with this pid runs (§ 4 one service per root); pid reuse reads live, a ceiling accepted for a 60 s window.
#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    liveness(pid).unwrap_or(false)
}

#[cfg(unix)]
pub fn liveness(pid: u32) -> Result<bool, String> {
    let pid = i32::try_from(pid).ok().filter(|p| *p > 0).ok_or("invalid process id")?;
    if unsafe { libc::kill(pid, 0) } == 0 { return Ok(true); }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() { Some(libc::ESRCH) => Ok(false), Some(libc::EPERM) => Ok(true), _ => Err(format!("process liveness unknown: {e}")) }
}

#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    liveness(pid).unwrap_or(false)
}

#[cfg(windows)]
pub fn liveness(pid: u32) -> Result<bool, String> {
    coreq::process::identity(pid).map(|identity|identity.is_some()).map_err(|e|format!("process liveness unknown: {e}"))
}

/// #2057: children an exec'd host image inherited from the old one, with each exit status once reaped. Threads that
/// owned them (tray `open -W`, board and link curl, `log show`) died at exec, so nothing else waits for them.
// ponytail: entries stay until the next exec; bounded by the children one image inherits
#[cfg(unix)]
static INHERITED: std::sync::Mutex<std::collections::BTreeMap<u32, Option<i32>>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Records this process's current children as inherited; call once at host start, before it spawns any.
#[cfg(unix)]
pub fn inherit_children() -> Result<Vec<u32>, String> {
    let pids = children()?;
    INHERITED.lock().unwrap_or_else(|e| e.into_inner()).extend(pids.iter().map(|&p| (p, None)));
    Ok(pids)
}

#[cfg(target_os = "macos")]
fn children() -> Result<Vec<u32>, String> {
    let mut buf = vec![0 as libc::c_int; 4096];
    // SAFETY: the buffer outlives the call and its byte size is passed; the result counts the pids written
    let n = unsafe { libc::proc_listchildpids(std::process::id() as i32, buf.as_mut_ptr().cast(), (buf.len() * 4) as i32) };
    if n < 0 { return Err(format!("child list unreadable: {}", std::io::Error::last_os_error())); }
    Ok(buf[..(n as usize).min(buf.len())].iter().filter(|&&p| p > 0).map(|&p| p as u32).collect())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn children() -> Result<Vec<u32>, String> {
    let me = std::process::id().to_string();
    let entries = std::fs::read_dir("/proc").map_err(|e| format!("child list unreadable: {e}"))?;
    Ok(entries.flatten().filter_map(|e| {
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        (stat.rsplit_once(')')?.1.split_whitespace().nth(1)? == me).then_some(pid)
    }).collect())
}

/// An inherited child's status: `None` when not inherited, `Some(None)` while it runs. The lock spans the wait, so the
/// host's reap and an adopter never race for the one exit status.
#[cfg(unix)]
fn inherited_wait(pid: u32) -> Option<Option<i32>> {
    let mut map = INHERITED.lock().unwrap_or_else(|e| e.into_inner());
    let slot = map.get_mut(&pid)?;
    if slot.is_none() {
        let mut st = 0;
        // SAFETY: non-blocking wait on one pid this image inherited as its direct child
        let r = unsafe { libc::waitpid(pid as i32, &mut st, libc::WNOHANG) };
        if r == pid as i32 { *slot = Some(if libc::WIFEXITED(st) { libc::WEXITSTATUS(st) } else { -1 }); } else if r < 0 { *slot = Some(-1); }
    }
    Some(*slot)
}

/// Reaps exited inherited children; an adopted worker's status stays for its `Adopted::exited`. Returns how many ended.
#[cfg(unix)]
pub fn reap_inherited() -> usize {
    let pids: Vec<u32> = INHERITED.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(_, s)| s.is_none()).map(|(p, _)| *p).collect();
    pids.into_iter().filter(|&p| matches!(inherited_wait(p), Some(Some(_)))).count()
}

/// Protected native identity recovered after re-exec or cold restart; control stays bound to that incarnation.
#[cfg(unix)]
pub struct Adopted { identity:coreq::process::Identity, ended:Option<i32>, direct:bool }

#[cfg(unix)]
impl Adopted {
    pub fn new(identity:coreq::process::Identity)->Self { Self {identity,ended:None,direct:false} }
}

#[cfg(unix)]
impl Proc for Adopted {
    fn exited(&mut self) -> Result<Option<i32>, String> {
        if self.ended.is_some() { return Ok(self.ended); }
        // #2057: an inherited child's exit status comes from the shared reap only, which may already have taken it
        match inherited_wait(self.identity.pid) {
            // the unreaped child holds the pid: another readable incarnation means the recorded one is gone
            Some(None) if matches!(coreq::process::identity(self.identity.pid), Ok(Some(ref i)) if *i!=self.identity) => { self.ended=Some(-1); return Ok(self.ended); },
            Some(None) => { self.direct=true; return Ok(None); },
            Some(code) => { self.ended=code; return Ok(code); },
            None => {},
        }
        // An unreaped owned child cannot reuse its PID, including while zombie; a zombie's identity read fails (#1541).
        let unread = match coreq::process::identity(self.identity.pid) {
            Ok(identity) if identity.as_ref()!=Some(&self.identity) => { self.ended=Some(-1); return Ok(self.ended); },
            Ok(_) => None,
            Err(_) if self.direct => None,
            Err(e) => Some(format!("process identity unreadable: {e}; native reconciliation required")),
        };
        let pid=self.identity.pid as i32;
        let mut st = 0;
        let r = unsafe { libc::waitpid(pid, &mut st, libc::WNOHANG) };
        match r {
            0 => { self.direct=true; Ok(None) },
            r if r == pid => { self.ended=Some(if libc::WIFEXITED(st) {libc::WEXITSTATUS(st)} else {-1}); Ok(self.ended) },
            _ if std::io::Error::last_os_error().raw_os_error()==Some(libc::ECHILD) => { self.direct=false; unread.map_or(Ok(None),Err) },
            _ => Err(format!("waitpid {pid}: {}; direct-child ownership unconfirmed",std::io::Error::last_os_error())),
        }
    }
    fn kill(&mut self) -> Result<(), String> {
        if self.exited()?.is_some() { return Ok(()); }
        if !self.direct { return coreq::process::terminate(&self.identity).map_err(|e|format!("recovered process stop unconfirmed: {e}; verify termination on its host")); }
        if unsafe { libc::kill(self.identity.pid as i32, libc::SIGKILL) } != 0 {
            return Err(format!("adopted process stop unconfirmed: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }
    fn id(&self) -> u32 {
        self.identity.pid
    }
}

/// The saved execution scope, rather than a vanished launcher's PID, decides whether it remains live.
pub fn execution_live(row:&crate::data::KidLine)->Result<bool,String> {
    #[cfg(windows)] if of(&row.runtime).child && !of(&row.runtime).remote {
        let key=row.job.as_deref().ok_or("legacy Windows execution has no native job scope; reconcile descendants before release")?;
        return windows_process::Execution::recover(row.process.clone(),key)?.exited().map(|ended|ended.is_none());
    }
    if row.job.is_some() {return Err("native job scope belongs to another adapter/platform; preserve record".into());}
    coreq::process::identity(row.process.pid).map(|id|id.as_ref()==Some(&row.process)).map_err(|e|e.to_string())
}

/// Cleanup may use an already-attributable board Exit; an absent native scope cannot create that proof.
pub fn released_execution_live(row:&crate::data::KidLine)->Result<bool,String> {
    #[cfg(windows)] if of(&row.runtime).child && !of(&row.runtime).remote {
        let key=row.job.as_deref().ok_or("legacy Windows execution has no native job scope; reconcile before cleanup")?;
        return match windows_process::Execution::open(row.process.clone(),key)? {Some(mut scope)=>scope.exited().map(|ended|ended.is_none()),None=>Ok(false)};
    }
    execution_live(row)
}

/// A random UUID v4 for the pre-launch Claude identity and qualification fixtures.
pub fn uuid() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 16];
    SystemRandom::new().fill(&mut b).expect("system random");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Claude has a pre-launch alias; the log resolves it (and Codex `-`) to the native session.
pub fn new_session(rt: &str) -> String {
    if rt == "claude" { uuid() } else { "-".into() }
}

pub fn log_path(root: &Path, l: &Launch) -> PathBuf {
    attempt_log(root, &l.role, l.task, l.attempt)
}

pub fn attempt_log(root: &Path, role: &str, task: u64, attempt: u64) -> PathBuf {
    let p = if role == "reviewer" { 'R' } else { 'T' };
    root.join(".taskq").join(format!("{p}{task}-{attempt}.log"))
}

/// The attempt log's events after byte `pos`; None when unreadable.
fn events(log: &Path, pos: u64) -> Option<Vec<serde_json::Value>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut bytes = Vec::new();
    File::open(log).and_then(|mut f| f.seek(SeekFrom::Start(pos)).and_then(|_| f.read_to_end(&mut bytes))).ok()?;
    Some(String::from_utf8_lossy(&bytes).lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

/// Model use of the attempt log after byte `pos` (§ 4 Profiling), read by AgentQ `agent::usage`; context tokens are
/// uncached input, cache reads and cache writes, and their sum.
pub fn usage(log: &Path, pos: u64, rt: &str) -> crate::profile::Usage {
    use crate::profile::Usage;
    if rt == "codex-app" { return Usage::Unknown("app-turn"); }
    let Some(events) = events(log, pos) else { return Usage::Unknown("log-unreadable") };
    let u = agentq_runtime::agent::usage(&events);
    if let Some(why) = u.unknown { return Usage::Unknown(why); }
    let tokens = [u.input, u.cache_read, u.cache_write].into_iter().try_fold(0, |a, b| b.map(|b| a + b));
    Usage::Known { calls: u.calls, tokens, model: u.model, input: u.input, cache_read: u.cache_read, cache_write: u.cache_write, output: u.output }
}

/// A usage or quota limit the runtime reported before any work started ([usage limit](docs/design/capacity.md#usage-limit)):
/// `Some(reset)` with the reported reset (epoch seconds) when known; None after work started or without a limit.
pub fn usage_limit(log: &Path) -> Option<Option<i64>> { agentq_runtime::agent::limit_reached(&events(log, 0)?) }

/// Provider metadata for the latest native turn. AgentQ remains the owner of quota-error classification.
/// Unknown names/resets stay unknown; prose never invents a weekly or five-hour reset.
pub fn quota_windows(log: &Path) -> Vec<(String, Option<i64>)> {
    let Some(events) = events(log,0) else { return vec![] };
    let start = events.iter().rposition(|e| e["type"] == "taskq.rules" && matches!(e["phase"].as_str(),Some("start"|"resume"))).unwrap_or(0);
    let mut windows = vec![];
    for e in &events[start..] {
        let info = if e["type"] == "rate_limit_event" { &e["rate_limit_info"] } else { &e["quotaLimits"] };
        let reached = agentq_runtime::agent::limit_reached(std::slice::from_ref(e));
        if reached.is_some() || e["type"] == "rate_limit_event" && info["status"] == "rejected" {
            let window = info["rateLimitType"].as_str().filter(|w| w.len() <= 32 && w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).unwrap_or("unknown");
            windows.push((window.into(),info["resetsAt"].as_i64().or(reached.flatten())));
        }
        let rates = match e["type"].as_str() {
            Some("rate_limits") => &e["rate_limits"],
            Some("event_msg") if e["payload"]["type"] == "rate_limits" => &e["payload"]["rate_limits"],
            _ => continue,
        };
        for key in ["primary","secondary"] {
            let w = &rates[key];
            if w["used_percent"].as_f64().is_some_and(|v| v >= 100.0) {
                let name = w["window_minutes"].as_u64().map_or_else(|| "unknown".into(), |m| format!("{m}m"));
                windows.push((name,w["resets_at"].as_i64()));
            }
        }
    }
    windows.sort(); windows.dedup();
    windows
}

/// A Codex login the auth server rejected (#1446); None when unreadable or absent.
pub fn auth_rejected(log: &Path) -> Option<String> {
    agentq_runtime::agent::auth_rejected(&String::from_utf8_lossy(&std::fs::read(log).ok()?)).map(String::from)
}

/// A Codex turn that ended on a transient model failure (#1843, [transient model failure](docs/design/capacity.md#transient-model-failure)).
pub fn transient_failure(log: &Path) -> Option<String> { agentq_runtime::agent::transient_failure(&events(log, 0)?) }

/// `<install>/login-off/<bin>` (#1446): line 1 the `last_refresh` of `auth.json` when the login was rejected, line 2
/// the reason. It reads not logged in until a re-login changes `last_refresh`.
pub fn mark_login_off(install: &Path, bin: &str, reason: &str) -> std::io::Result<()> {
    let dir = install.join("login-off");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(bin), format!("{}\n{reason}\n", codex_refreshed(install).unwrap_or_default()))
}

fn codex_refreshed(install: &Path) -> Option<String> {
    let home = crate::runtime::accounts::active_in(install, "codex", &codex_home()?);
    kind_refresh("codex", None, std::fs::read_to_string(home.join("auth.json")).ok().as_deref()).1
}

/// The reason of a rejected login still current at `refreshed` (#1446).
fn login_off(install: &Path, bin: &str, refreshed: Option<&str>) -> Option<String> {
    let text = std::fs::read_to_string(install.join("login-off").join(bin)).ok()?;
    let (at, reason) = text.split_once('\n')?;
    (at == refreshed.unwrap_or_default()).then(|| reason.trim().to_string()).filter(|r| !r.is_empty())
}

pub use agentq_runtime::agent::{final_text, which_in, user_bin_path, codex_native};

/// agy user installs must also resolve in a service's non-login PATH (§ 8).
pub fn which(name: &str) -> PathBuf {
    if name != "agy" { return agentq_runtime::agent::which(name); }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    which_in(name, &user_bin_path(&path, home.as_deref()).unwrap_or(path))
}

/// The worker's last message in an attempt log, for the `gone` reason (§ 4 step 1); None when unreadable.
pub fn last_message(log: &Path) -> Option<String> {
    let text = String::from_utf8_lossy(&std::fs::read(log).ok()?).into_owned();
    let events: Vec<serde_json::Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    if let Some(m) = final_text(&events) { return Some(m); }
    let mut tail: Vec<&str> = text.lines().rev().filter(|l| !l.trim().is_empty())
        .take_while(|l| serde_json::from_str::<serde_json::Value>(l).is_err()).collect();
    tail.reverse();
    let root = log.parent().and_then(|d| if d.ends_with(".taskq") { d.parent() } else { Some(d) }).unwrap_or_else(|| Path::new(""));
    Some(public_line(&tail.join("\n"), root, 240)).filter(|s| !s.is_empty())
}

/// Worker text as one public line (§ 4 step 1, § 7 rule 6): whitespace collapsed, absolute paths made relative to
/// `root` or replaced by `<path>`, at most `max` characters ending in `…`. The full text stays in the attempt log.
pub fn public_line(text: &str, root: &Path, max: usize) -> String {
    let root = slashed(&root.to_string_lossy());
    let mut out = String::new();
    let mut word = String::new();
    for c in text.chars().chain([' ']) {
        if !(c.is_whitespace() || "()[]<>\"'`".contains(c)) {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            out += &public_word(&word, &root);
            word.clear();
        }
        if !c.is_whitespace() { out.push(c) } else if !out.is_empty() && !out.ends_with(' ') { out.push(' ') }
    }
    let out = out.trim_end();
    if out.chars().count() <= max { return out.into(); }
    out.chars().take(max.saturating_sub(1)).collect::<String>().trim_end().to_string() + "…"
}

/// A path with `/` separators, without `file://`, `\\?\` or the `/` before a drive letter of a Markdown link.
fn slashed(p: &str) -> String {
    let p = p.replace('\\', "/");
    let p = p.strip_prefix("file://").unwrap_or(&p);
    let p = p.strip_prefix("//?/").unwrap_or(p);
    let b = p.as_bytes();
    let p = if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { &p[1..] } else { p };
    p.trim_end_matches('/').to_string()
}

fn public_word(word: &str, root: &str) -> String {
    let core = word.trim_end_matches(['.', ',', ';', ':']);
    let (w, tail) = (slashed(core), &word[core.len()..]);
    if !root.is_empty() && w.get(..root.len()).is_some_and(|p| p.eq_ignore_ascii_case(root)) {
        match &w[root.len()..] {
            "" => return format!(".{tail}"),
            rest if rest.starts_with('/') => return format!("{}{tail}", &rest[1..]),
            _ => {}
        }
    }
    let b = w.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/';
    let abs = drive || w.starts_with("~/") || (w.starts_with('/') && w[1..].contains('/'));
    if abs { format!("<path>{tail}") } else { word.into() }
}

/// A CLI child whose log shows its turn ended counts as exited even when its process lingers: Codex on Windows kept
/// running after a rate-limited `taskq result` (#1034). The service then stops its tree (§ 4 step 1, R11).
pub struct CliTurn {
    pub proc: Box<dyn Proc>,
    pub log: PathBuf,
    /// Log bytes already read: a resume appends to the log of earlier turns.
    pub pos: u64,
    pub start: u64,
    pub stopping: bool,
    pub accepted: bool,
    pub read_error: Option<String>,
    /// Expected Antigravity conversation; copied or conflicting native receipts never release the turn.
    pub session: Option<String>,
    pub native: Option<coreq::process::Identity>,
    pub recovery: Option<Recovery>,
}

pub struct Recovery {
    install:PathBuf, root:PathBuf, task:u64, attempt:u64, execution:u64, runtime:String,
}

impl CliTurn {
    fn persist(&mut self)->Result<(),String> {
        let Some(r)=&self.recovery else { return Ok(()) };
        let row=crate::data::KidLine {task:r.task,attempt:r.attempt,runtime:r.runtime.clone(),process:self.native_identity()?,execution:r.execution,offset:self.input_offset()?,job:self.proc.job()};
        crate::checkpoint_kid(&r.install,&r.root,row).map_err(|e|format!("native recovery checkpoint failed: {e}"))?;
        self.recovery=None;
        Ok(())
    }
    fn ended(&mut self) -> Result<bool,String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut rest = vec![];
        let mut f = File::open(&self.log).map_err(|e|format!("native input log unreadable: {e}"))?;
        if f.metadata().map_err(|e|e.to_string())?.len() < self.pos { return Err("native input log truncated; input boundary unknown".into()); }
        f.seek(SeekFrom::Start(self.pos)).and_then(|_| f.read_to_end(&mut rest)).map_err(|e|format!("native input log read failed: {e}"))?;
        let Some(end) = rest.iter().rposition(|b| *b == b'\n') else { return Ok(false) };
        let mut ended = false;
        for v in String::from_utf8_lossy(&rest[..end]).lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()) {
            if let Some(expected) = &self.session {
                if let Some(id) = agentq_runtime::agent::antigravity_session(&v.to_string()) {
                    if expected != "-" && *expected != id { self.accepted = false; return Err("Antigravity conversation changed; input acceptance unknown".into()); }
                    self.session = Some(id);
                }
            }
            self.accepted |= matches!(v["type"].as_str(), Some("turn.started" | "assistant"));
            ended |= matches!(v["type"].as_str(), Some("turn.completed" | "turn.failed" | "result"));
            if matches!(v["event"].as_str(), Some("step_update" | "result")) || matches!(v["type"].as_str(), Some("PLANNER_RESPONSE" | "TOOL_RESPONSE" | "turn.cancelled")) {
                self.accepted = true;
                let report = agentq_runtime::agent::Report::read(agentq_runtime::agent::Client::Antigravity, "", std::slice::from_ref(&v), None);
                ended = report.outcome != "unknown";
            }
        }
        self.pos += end as u64 + 1;
        Ok(ended)
    }
}

impl Proc for CliTurn {
    fn refused(&self,task:u64,attempt:u64,execution:u64,code:i32)->Option<String> {
        use std::io::{Seek,SeekFrom};
        if code<=0 || self.accepted || self.read_error.is_some() {return None;}
        let mut file=File::open(&self.log).ok()?;
        file.seek(SeekFrom::Start(self.start)).ok()?;
        let mut text=String::new();file.take(1024*1024+1).read_to_string(&mut text).ok()?;
        if text.len()>1024*1024 || !text.ends_with('\n') {return None;}
        let mut lines=text.lines();
        let binding:serde_json::Value=serde_json::from_str(lines.next()?).ok()?;
        let thread=binding["native"]["session"].as_str().filter(|s|plain_thread(s))?;
        if task==0 || attempt==0 || execution<=attempt || binding["type"]!="taskq.input" || binding["runtime"]!="codex"
            || binding["task"].as_u64()!=Some(task) || binding["attempt"].as_u64()!=Some(attempt)
            || binding["execution"].as_u64()!=Some(execution) || binding["input"].as_str()?.is_empty() {return None;}
        let conflict=format!("thread {thread} already has an active writer");
        let create=format!("Failed to create session: thread-store conflict: {conflict}");
        let persistence=format!("failed to initialize thread persistence: thread-store conflict: {conflict}");
        let refusal=format!("Error: thread/resume: thread/resume failed: {conflict} (code -32600)");
        let mut created=false;let mut refused=false;
        for line in lines.filter(|l|!l.is_empty()) {
            if refused {return None;}
            if line==refusal {refused=true;}
            else if line.split_once(" ERROR codex_core::session: ").is_some_and(|(_,message)|message==create) {created=true;}
            else if !line.split_once(" ERROR codex_core::session::session: ").is_some_and(|(_,message)|message==persistence) {return None;}
        }
        (created&&refused).then(||format!("Codex writer refused; let active turn finish, then close that thread in Codex ({thread})"))
    }
    fn job(&self)->Option<String> {self.proc.job()}
    fn checkpoint(&mut self,cfg:&crate::service::Config,l:&Launch,execution:u64)->Result<(),String> {
        if !of(&l.rt).child || of(&l.rt).remote { return Ok(()) }
        self.recovery=Some(Recovery {install:cfg.install.clone(),root:cfg.root.clone(),task:l.task,attempt:l.attempt,execution,runtime:l.rt.clone()});
        self.persist()
    }
    fn native_identity(&self)->Result<coreq::process::Identity,String> { self.native.clone().ok_or("native process identity was not observed at spawn".into()) }
    fn input_offset(&self)->Result<u64,String> {
        if std::fs::metadata(&self.log).map_err(|e|e.to_string())?.len() < self.pos { return Err("native input log truncated; handover refused".into()); }
        Ok(self.start)
    }
    fn accepted(&mut self) -> Result<bool, String> {
        if !self.accepted { if let Some(e)=&self.read_error { return Err(e.clone()); } }
        Ok(self.accepted)
    }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        let checkpoint=self.persist();
        let ended = self.ended();
        self.read_error=ended.as_ref().err().cloned();
        if let Some(code) = self.proc.exited()? { return Ok(Some(code)); }
        if !self.stopping && ended? {
            self.stopping = true;
            log!("{}: turn ended; stopping its lingering process", self.log.display());
        }
        if self.stopping { self.proc.kill()?; self.proc.exited() } else { checkpoint?; Ok(None) }
    }
    fn kill(&mut self) -> Result<(), String> { self.stopping = true; self.proc.kill() }
    fn id(&self) -> u32 { self.proc.id() }
}

/// Native conversation identity from the owning adapter; never resolve another runtime's event.
pub fn native_session(rt: &str, log: &Path) -> Option<String> {
    let text = std::fs::read_to_string(log).ok()?;
    if rt == "antigravity" { agentq_runtime::agent::antigravity_session(&text) } else { agentq_runtime::agent::codex_thread(&text) }
}

/// Codex's thread id: the first `thread.started` line of the attempt log.
pub fn codex_thread(log: &Path) -> Option<String> {
    agentq_runtime::agent::codex_thread(&String::from_utf8_lossy(&std::fs::read(log).ok()?))
}

/// Why `rt` cannot start here when its CLI does not resolve on PATH (§ 3: no claim, resume or reviewer).
pub fn missing_cli(rt: &str) -> Option<String> {
    if rt == "antigravity" { return (!which(of(rt).bin).is_absolute()).then(|| "antigravity agy CLI not found on the service PATH".into()); }
    agentq_runtime::agent::missing_cli(client(rt))
}

/// § 6 setup check: the § 8 grant mode the owner gives `rt` on `os`; Claude has no Workspace enforcement off macOS.
pub fn grant_mode(os: &str, rt: &str) -> &'static str { if os == "macos" || rt == "codex" { "workspace" } else { "full" } }

/// § 6 setup check: whether `rt`'s sandbox on `os` needs `<root>/.codex` (see `linux_sandbox::ready`).
pub fn needs_codex_dir(os: &str, rt: &str) -> bool { os == "linux" && rt == "codex" }

pub struct Env<'a> {
    /// Fresh board goal/acceptance and every ordered note; empty only for non-task instruments.
    pub context: &'a str,
    pub root: &'a Path,
    pub exe: &'a Path,
    /// `<install>`: its `worker-policy.json` grants access (§ 8 Access).
    pub install: &'a Path,
    pub permission_mode: &'a str,
    pub codex: &'a [String],
    /// The § 7 delivery preset: `auto-merge`, `review-pr` or `direct`.
    pub publish: &'a str,
    /// Secret values: command environment only, never brief, argv or log.
    pub board_env: &'a [(String, String)],
    /// § 8 compile jobs: `CARGO_BUILD_JOBS` for this launch; None sets nothing.
    pub jobs: Option<usize>,
}

/// § 8 compile jobs: an equal share of the host CPUs among occupied host slots, this launch included; unknown CPUs: None.
pub fn compile_jobs(cpus: Option<usize>, occupied: usize) -> Option<usize> {
    cpus.map(|cpus| (cpus / occupied.max(1)).max(1))
}

pub(crate) fn executable(bin: PathBuf, access: &crate::policy::Effective, env: &Env) -> Result<Command, String> {
    #[cfg(windows)]
    if access.windows { return crate::windows_sandbox::command(env); }
    if access.linux {
        let mut c = crate::platform::command("/usr/local/libexec/taskq-sandbox");
        c.arg(env.root).arg(env.install).arg(&access.mode).arg(bin);
        #[cfg(target_os = "linux")]
        crate::linux_sandbox::own(&mut c);
        return Ok(c);
    }
    Ok(match &access.profile {
        Some(p) => {
            let mut c = crate::platform::command("/usr/bin/sandbox-exec");
            c.arg("-p").arg(p).arg(bin);
            c
        }
        None => crate::platform::command(bin),
    })
}

/// The runtime's CLI under `access`, shared by worker turns and setup sessions: Codex runs its native executor.
pub(crate) fn cli(rt: &str, access: &crate::policy::Effective, env: &Env) -> Result<Command, String> {
    let a = of(rt);
    executable(if a.codex { codex_native(which(a.bin))? } else { which(a.bin) }, access, env)
}

pub fn command(env: &Env, l: &Launch, access: &crate::policy::Effective) -> Result<Command, String> {
    let codex = access.codex.as_deref().unwrap_or(env.codex);
    let mode = access.claude.as_deref().unwrap_or(env.permission_mode);
    let build = build_cache_for(&l.rt, access, env);
    let args: Vec<std::ffi::OsString> = if l.rt == "claude" {
        ["--permission-mode", mode, "--settings", &compact_settings(env.root, env.install, l, env.jobs, build.as_deref())].map(Into::into).to_vec()
    } else if of(&l.rt).codex { codex.iter().map(Into::into).collect() }
    else if l.rt == "antigravity" && access.refused.is_none() && matches!(access.mode.as_str(), "workspace" | "full") {
        vec!["--dangerously-skip-permissions".into()]
    } else { vec![] };
    let mut c = agentq_runtime::agent::command(cli(&l.rt, access, env)?, &optimized(spec(env, l, env.root.into(), args))?);
    if let Some(p) = build { c.env("RUSTC_WRAPPER", p); }
    Ok(c)
}

/// The AgentQ `agent` turn of launch `l` (§ 8 Spawn and resume): TaskQ's attempt variables, board secrets, compile
/// jobs and the § 3 account home the attempt log names (`default` keeps the service's own).
fn spec(env: &Env, l: &Launch, root: PathBuf, args: Vec<std::ffi::OsString>) -> agentq_runtime::agent::Spec {
    let mut vars: Vec<(String, String)> = vec![("TASKQ_TASK".into(), l.task.to_string()), ("TASKQ_ATTEMPT".into(), l.attempt.to_string()),
        ("TASKQ_ROLE".into(), l.role.clone()), ("TASKQ_ROOT".into(), env.root.to_string_lossy().into())];
    vars.extend(env.board_env.iter().cloned());
    if let Some(jobs) = env.jobs { vars.push(("CARGO_BUILD_JOBS".into(), jobs.to_string())); }
    let home = crate::runtime::accounts::applies(&l.rt).then(|| crate::runtime::accounts::log_home(&l.rt, &log_path(env.root, l))).flatten();
    agentq_runtime::agent::Spec { client: client(&l.rt), root, session: l.session.clone(), resume: l.resume.is_some(), args, home,
        env: vars.into_iter().map(|(k, v)| (k.into(), v.into())).collect() }
}

fn client(rt: &str) -> agentq_runtime::agent::Client {
    if rt == "antigravity" { agentq_runtime::agent::Client::Antigravity }
    else if of(rt).codex { agentq_runtime::agent::Client::Codex } else { agentq_runtime::agent::Client::Claude }
}

/// Prepare only a submitted launch; native inspection and stop never change optimizer intent.
fn optimized(mut spec: agentq_runtime::agent::Spec) -> Result<agentq_runtime::agent::Spec, String> {
    let state = optimization(&mut spec, agentq_runtime::optimization::install_root().as_deref())?;
    eprintln!("AgentQ optimization ({} {}): {}", spec.client.code(), spec.session, state.json());
    Ok(spec)
}

fn optimization(spec: &mut agentq_runtime::agent::Spec, install: Option<&Path>) -> Result<agentq_runtime::optimization::Optimization, String> {
    use agentq_runtime::optimization::{self, Intent};
    let intent = spec.env.iter().rev().find(|(k, _)| k == optimization::ENV).map(|(_, v)| v.clone())
        .or_else(|| std::env::var_os(optimization::ENV));
    let intent = match intent {
        None => Intent::Inherit,
        Some(v) => v.to_str().and_then(Intent::parse).ok_or("AGENTQ_OPTIMIZATION must be inherit, off or on")?,
    };
    spec.env.extend([(optimization::ENV.into(), intent.code().into()), ("AGENTQ_LAUNCHER".into(), "taskq".into())]);
    let state = optimization::prepare(intent, "taskq", spec, install);
    if spec.client == agentq_runtime::agent::Client::Claude {
        let env = optimizer_settings(spec)["env"].clone();
        if let Some(i) = spec.args.iter().rposition(|a| a == "--settings") {
            let value = spec.args.get_mut(i + 1).ok_or("Claude settings missing")?;
            let mut settings: serde_json::Value = serde_json::from_str(value.to_str().ok_or("Claude settings invalid")?).map_err(|e| e.to_string())?;
            let settings = settings.as_object_mut().ok_or("Claude settings must be an object")?;
            let vars = settings.entry("env").or_insert_with(|| serde_json::json!({})).as_object_mut().ok_or("Claude settings env must be an object")?;
            vars.extend(env.as_object().unwrap().clone());
            *value = serde_json::to_string(settings).map_err(|e| e.to_string())?.into();
        } else { spec.args.extend(["--settings".into(), serde_json::json!({"env":env}).to_string().into()]); }
    }
    Ok(state)
}

fn optimizer_settings(spec: &agentq_runtime::agent::Spec) -> serde_json::Value {
    let env: serde_json::Map<String, serde_json::Value> = spec.env.iter()
        .filter(|(k, _)| k == "AGENTQ_OPTIMIZATION" || k == "AGENTQ_LAUNCHER")
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned().into())).collect();
    serde_json::json!({"env":env})
}

/// Claude's daemon ignores launcher env; resume overlays only optimizer keys, preserving saved native policy.
fn background_base(mut base: Command, spec: &agentq_runtime::agent::Spec) -> Command {
    if spec.resume { base.args(["--settings", &optimizer_settings(spec).to_string()]); }
    base
}

/// A Claude turn spec without TaskQ arguments: native job reads and stops.
fn claude_spec(root: &Path, home: Option<&Path>) -> agentq_runtime::agent::Spec {
    agentq_runtime::agent::Spec { client: agentq_runtime::agent::Client::Claude, root: root.into(), session: String::new(), resume: false, args: vec![], home: home.map(Into::into), env: vec![] }
}

/// Build cache (docs/testing.md#build-cache): `sccache` on the service's PATH, unless the service inherits
/// `RUSTC_WRAPPER` (even empty: the owner's choice, children inherit it).
pub(crate) fn build_cache() -> Option<PathBuf> {
    build_cache_in(std::env::var_os("RUSTC_WRAPPER").is_some(), &std::env::var_os("PATH").unwrap_or_default())
}

fn build_cache_in(inherited: bool, path: &std::ffi::OsStr) -> Option<PathBuf> {
    Some(which_in("sccache", path)).filter(|p| !inherited && p.is_absolute())
}

/// The service log line naming the build cache children get.
pub(crate) fn build_cache_line() -> String {
    match build_cache() {
        Some(p) => format!("build cache: RUSTC_WRAPPER={} for Claude and unsandboxed Codex children", p.display()),
        None if std::env::var_os("RUSTC_WRAPPER").is_some() => "build cache: inherited RUSTC_WRAPPER kept; taskq adds none".into(),
        None => "build cache: sccache not on PATH; children build unchanged".into(),
    }
}

/// The cache for this child: taskq's Windows/Linux sandboxes and Codex's own sandbox keep the child off the user
/// cache and the localhost sccache server, and a wrapper that cannot reach its server fails the build; they run unchanged.
fn build_cache_for(rt: &str, access: &crate::policy::Effective, env: &Env) -> Option<PathBuf> {
    build_cache().filter(|_| build_open(rt, access, env))
}

fn build_open(rt: &str, access: &crate::policy::Effective, env: &Env) -> bool {
    let codex = access.codex.as_deref().unwrap_or(env.codex);
    let open = !of(rt).codex || codex.iter().any(|a| a == "--dangerously-bypass-approvals-and-sandbox")
        || crate::policy::codex_sandbox(codex) == Some("danger-full-access");
    open && !access.windows && !access.linux
}

const FORMATS: &str = "Put each `--text` field on its own line. The commands render and check the taskq.md templates and limits (§ 2 Ask card,\n\
     § 7 Result and close verdicts): Done ≤160, Problem 1-2 lines, 2-4 options ≤160 (none: owner action), ≤2 links; result/close lines ≤240,\n\
     Changed and Open 1-2 lines, 1-8 checks. Over-limit text is rejected: shorten it, never hand-write card prose.\n";

/// taskq.md § 8 Folders: never search `/` or home; macOS asks the owner for Desktop, Documents, Downloads etc.
const FILES: &str = "Never run gh auth login. A missing board token is an owner item; report it through taskq ask.\n\
     Touch only this project, ~/.claude, ~/.codex and the OS temp dir: never search `/`, `~` or other folders.\n\
     A macOS privacy (TCC) denial is reported in the result Open, never as an owner ask: the service notifies the owner.\n";

/// taskq.md § 7 rule 7 CI wait: every host's agents share one board user's hourly requests (§ 4 board budget).
const CI_WAIT: &str = "Board budget: poll CI at most once a minute (`gh pr checks <PR> --watch --interval 60`); never `gh run watch`\n\
     (it polls every 3 s) or `gh run view` without `--json`.\n";

/// taskq.md § 7 rule 4 checks: a full suite after each edit costs minutes; the project's testing doc names the fast form.
const DEV_LOOP: &str = "While editing, run only the filtered checks of the project's development loop (its AGENTS.md or testing doc names the fast command);\n\
     run the full project checks once on the result candidate, after the rebase.\n";

/// taskq.md § 7 rule 4 (#1842): workers chasing main rebased up to 5 times per attempt, each a rebuild, new shots and a full check.
const PUBLISH_ONCE: &str = "Rebase on origin/main once, before the first push. After that, never rebase or rerun the full checks because main moved:\n\
     the merge queue checks the merge result. Rebase again only when a requeue asks, as `main-moved` with its conflict files.\n";

/// taskq.md § 7: a sandbox limit is unknown locally; publication still needs the section 7 CI gate.
const SANDBOX_CHECKS: &str = "Host tests blocked by the worker sandbox: record sandbox-blocked in Checks, never as PASS; do not requeue for them.\n\
     Their Acceptance gate is green CI on the GitHub PR merge result (merge_group); private fallback uses exact-SHA CI; other routes follow taskq.md section 7 (R12).\n";

fn ask(q: &impl std::fmt::Display, n: u64) -> String {
    format!("`{q} ask {n} --text \"Done: <checked or not started>\nProblem: <decision and user effect>\" --option \"<label> - + <gain>; - <cost>\".. --recommend K [--link URL]`")
}

pub fn contract(root: &Path) -> String {
    match project_contract(root) {
        Ok(Some(spec)) => spec.display().to_string(),
        Ok(None) => command_contract(),
        Err(e) => format!("project contract unavailable: {e}"),
    }
}

fn command_contract() -> String {
    let v = crate::platform::VERSION;
    let tag = if v.contains('-') { "main".into() } else { format!("v{v}") }; // dev builds have no tag
    format!("https://github.com/{}/blob/{tag}/taskq.md", crate::platform::RELEASE_REPO)
}

fn project_contract(root: &Path) -> Result<Option<PathBuf>, StartError> {
    let (path, fallback) = contract_path(root)?;
    Ok((!fallback || path.try_exists().map_err(|e| refused("project contract", e))?).then_some(path))
}

/// #1479: the contract at the fetched `origin/main` commit the worker branch starts from (§ 7 rule 2), never the root's
/// working tree or HEAD. `None`: the root has no Git or no `origin/main`, so the working tree applies as before;
/// `Some((_, None))`: the file is absent at that commit and this release's contract applies.
fn base_contract(root: &Path) -> Result<Option<(String, Option<Vec<u8>>)>, StartError> {
    if !root.join(".git").try_exists().map_err(|e| refused("project contract", e))? { return Ok(None); }
    // #1856: each failed call is logged with its command and error, also where it means absent (no `origin/main`)
    let git = |args: &[&str]| crate::tools::run("rules.contract", crate::platform::command("git").arg("-C").arg(root)
        .args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"]).args(args), std::time::Duration::from_secs(30))
        .map_err(|e| crate::platform::log(&format!("project contract: {e}"))).ok();
    // ponytail: a failed fetch keeps the last fetched commit; the header names it and the worker branches from the same ref
    git(&["fetch", "-q", "origin", "main"]);
    let Some(sha) = git(&["rev-parse", "--verify", "-q", "origin/main^{commit}"]) else { return Ok(None) };
    let sha = String::from_utf8_lossy(&sha).trim().to_string();
    let (path, fallback) = contract_path(root)?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| refused("project contract", "invalid name"))?;
    let object = format!("{sha}:{name}");
    if git(&["cat-file", "-e", &object]).is_none() {
        return if fallback { Ok(Some((String::new(), None))) } else { Err(refused("project contract", format!("{name} absent at origin/main {sha}"))) };
    }
    let body = git(&["cat-file", "blob", &object]).ok_or_else(|| refused("project contract", format!("{name} unreadable at origin/main {sha}")))?;
    Ok(Some((format!("{} at origin/main {sha}", path.display()), Some(body))))
}

/// The registered contract file, and whether this release's contract replaces it when absent.
fn contract_path(root: &Path) -> Result<(PathBuf, bool), StartError> {
    let config = match std::fs::read(root.join("taskq.json")) {
        Ok(bytes) => Some(serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| refused("project registration", e))?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(refused("project registration", e)),
    };
    if let Some(config) = config {
        let repo = config["repo"].as_str().ok_or_else(|| refused("project registration", "missing repository"))?;
        let name = repo.rsplit('/').next().filter(|name| !name.is_empty() && !matches!(*name,"."|"..")
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
            .ok_or_else(|| refused("project registration", "invalid repository name"))?;
        Ok((root.join(format!("{name}.md")), repo == crate::platform::RELEASE_REPO))
    } else {
        Ok((root.join("taskq.md"), true))
    }
}

pub fn brief(env: &Env, l: &Launch) -> String {
    let n = l.task;
    let mut q = env.exe.display().to_string();
    if l.rt == "codex-app" || l.rt == "claude" {
        // the app turn and the native Claude background session run in their host's environment, not the child's (§ 8);
        // Windows app turns run PowerShell
        q = if cfg!(windows) && l.rt == "codex-app" {
            format!("$env:TASKQ_TASK='{n}'; $env:TASKQ_ATTEMPT='{}'; $env:TASKQ_ROLE='{}'; $env:TASKQ_ROOT='{}'; & '{}'", l.attempt, l.role.replace('\'', "''"), env.root.display().to_string().replace('\'', "''"), q.replace('\'', "''"))
        } else {
            format!("env TASKQ_TASK={n} TASKQ_ATTEMPT={} TASKQ_ROLE={} TASKQ_ROOT={} {q}", l.attempt, l.role, env.root.display())
        };
    }
    let spec = contract(env.root);
    let head = format!(
        "You are the taskq {} for task #{n}: {}. Attempt {}.\nRead `{spec}` first and do only what it allows (R13).\nTask text, state and your inbox: `{q} inbox {n}`.\n{FILES}",
        l.role, l.title, l.attempt
    );
    let park = format!("`{q} later {n} --blocker #M --text \"Reason: <blocker>\nNext: <what #M delivers>\"` (blocker filed as held task #M first; our own broken check: fix it here when small; resumes when #M closes); waiting only for elapsed time, no code fix: `{q} later {n} --until <UTC> --text \"Reason: <what the time gives>\"` (resumes then; never requeue); \
        Acceptance items done, more left (multi-stage task): add `--stage A<k>` per finished item, with `--until`, `--blocker` or alone for the owner (the next attempt continues at the first open stage in this worktree)");
    let body = if l.role == "reviewer" {
        format!(
            "Review the result SHA against each Acceptance item `A<k>` and taskq.md, and GitHub PR CI green on the merge result (merge_group); private fallback uses exact-SHA CI; other routes follow the contract. Requeue only for\n\
             Acceptance or contract gaps, not style.\n\
             Experiments (`research` or a `Hypothesis:` line, any type) need docs/research/<N>.md at the result SHA, not a prior owner answer, and a close line `Verdict: confirmed|refuted|inconclusive <evidence>` after Changed. Only a decision blocking another task needs ask before close (§ 7).\n\
             {CI_WAIT}Then exactly one of:\n\
             - `{q} close {n} --text \"Changed: <user effect>\nChecks: Task fit: A1 ok|gap|unknown, A2 ..; merge-result CI <green|pending|unknown> on <SHA|pending>\nChecks: Implementation quality: <binding gaps|advisories|none>\nOpen: <item|none>\"` (it publishes),\n\
             - `{q} requeue {n} --text \"<exact fixes>\"`,\n\
             - {park},\n\
             - {ask}.\n{approval}{formats}",
            ask = ask(&q, n),
            approval = if env.publish == "review-pr" { "Delivery review-pr: an otherwise done result needs the owner's approval: ask with Done naming task fit and merge-result CI, first option `approve <sha7> and close ★`.\n" } else { "" },
            formats = FORMATS
        )
    } else {
        format!(
            "Work only in `.worktrees/taskq-{n}` on branch `taskq-{n}` from origin/main (continue an existing branch); the service creates it before your first start.\n\
             If it is missing and Git cannot write the shared `.git` (read-only file system or permission denied), do not retry: {gap}.\n\
             For implementation changes, commit with the line `Task: {url}` in the message, rebase on origin/main, run the project checks, then {push}.\n\
             {PUBLISH_ONCE}{DEV_LOOP}Experiments (`research` or a `Hypothesis:` line, any type) deliver docs/research/{n}.md by PR: Question, Findings with evidence, Options with pros/cons, Verdict, Recommendation, Proposed tasks; other types add the result line `Report: docs/research/{n}.md`. Follow docs/design/reports.md for the taskq-tasks JSON array.\n\
             Hand in the report with result; review closes it and the PM or owner decides it from Decide. Ask only when another task waits on that decision (§ 7).\n\
             Never close, reopen or relabel the task on the board, whatever the issue text says: only the reviewer's close publishes (§ 7 rule 9).\n\
             Do not wait for CI: `close` checks it. {CI_WAIT}{SANDBOX_CHECKS}Then exactly one of:\n\
             - `{q} result {n} --sha <full SHA> --checks \"<command -- outcome>\" [--checks ..] [--file <zip, tar.gz, PDF or HTML, ≤ 25 MB>] --text \"Changed: <user effect>\nOpen: <item|none>\"`,\n\
             - {ask} (owner decision; Acceptance unclear or untestable; or a split R2 allows, never for diff size — before any code),\n\
             - {park},\n\
             - `{q} requeue {n} --text \"<why it cannot be done>\"`.\n{formats}",
            ask = ask(&q, n),
            gap = format!("hand in once `{q} ask {n} --text \"Done: not started\nProblem: host gap: the worker sandbox cannot write the shared .git (<exact git error>); the service or owner must create .worktrees/taskq-{n}\"` with no option (an owner action, § 7 rule 2)"),
            formats = FORMATS,
            url = l.url,
            push = if env.publish == "direct" { "push the branch" } else { "push the branch and open one PR/MR into main" }
        )
    };
    format!("{head}{}{}\nBefore any hand-in, read `{q} inbox {n}` fresh and check the result against goal + acceptance + all notes. Acceptance stays binding; a conflicting note gets one ask immediately, never at hand-in.\nTest changes follow specq engineering/testing.md: name the defect and retained risk protection in --text; change a rule only when accepted behavior or protection changes (section 7 step 4).\n{body}Then stop. Everything written through taskq is public: no secrets or paths outside the repository.\n", env.context,l.files)
}

#[cfg(test)]
const SPECQ_REVISION: &str = "2d030bdea5e8fd9095daea2069954ff6091ea90e";
const SPECQ_REFS: &str = "https://github.com/taskqorg/specq.git/info/refs?service=git-upload-pack";
const SPECQ_TREES: &str = "https://api.github.com/repos/taskqorg/specq/git/trees/";
/// The specq shared core, sent whole at every start and continuation (§ 8 Rule delivery).
const SPECQ_CORE: &[&str] = &[
    "engineering/principles.md", "engineering/spec-first.md", "engineering/ponytail.md", "engineering/agent-workflow.md",
    "engineering/use-cases.md", "engineering/verification.md", "engineering/security.md",
    "caveman/rules/core.md", "caveman/rules/documentation.md", "caveman/rules/changes.md",
];
/// Frozen published layout used by test source fixtures; production discovers the selected tree.
#[cfg(test)]
const SPECQ_INDEX: &[&str] = &[
    "caveman/SOURCES.md", "caveman/examples/messages.md", "caveman/rules/agents.md", "caveman/rules/indicators.md",
    "caveman/rules/notifications.md", "design/dashboards.md", "design/implementation-fidelity.md", "design/labels.md",
    "design/operator-ui.md", "design/workflow-review.md", "design/workflow.md", "engineering/data.md", "engineering/performance.md",
    "engineering/permission-authority.md", "engineering/product-connections.md", "engineering/profiling.md",
    "engineering/release-contract.md",
];
/// One cap for the injected header, contract, core and index (§ 8).
const RULES_CAP: usize = 128 * 1024;
/// This release's TaskQ tool contract, used only for the documented setup/TaskQ fallback.
const CONTRACT: &str = include_str!("../taskq.md");
const INDEX: &str = "\nIndex of other shared rules";

fn rule_file(url: &str) -> Result<String, String> {
    let out = match coreq::process::output("rules.download", crate::platform::command("curl").args(["-q", "--fail", "--silent", "--show-error", "--proto", "=https", "--max-time", "10", "--max-filesize", "131072", "--write-out", "\n%{http_code}", url]), std::time::Duration::from_secs(15)) {
        Ok(out) | Err(coreq::process::Failure::Exit(out)) => out,
        Err(e) => return Err(e.to_string()),
    };
    let text=String::from_utf8(out.stdout).map_err(|_| "invalid UTF-8")?;
    let (body,status)=text.rsplit_once('\n').ok_or("missing HTTP status")?;
    if !out.status.success() || status!="200" {return Err(format!("HTTP {status}; curl exited {}",out.status));}
    Ok(body.into())
}

fn specq_dir(root: &Path, revision: &str) -> PathBuf {
    root.join(".taskq").join("specq").join(revision)
}

fn refused(what: &str, e: impl std::fmt::Display) -> StartError {
    StartError::from(format!("rules {what}: {e}"))
}

fn revision_id(text: &str) -> bool {
    if text.len()!=40 {return false;}
    let mut nonzero=false;
    for byte in text.bytes() {
        if !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {return false;}
        nonzero|=byte!=b'0';
    }
    nonzero
}

fn advertised_revision(text: &str) -> Result<String, StartError> {
    let invalid=||refused("shared revision","invalid Git reference advertisement");
    if !text.starts_with("001e# service=git-upload-pack\n0000") {return Err(invalid());}
    let mut input=text.as_bytes();let mut selected=None;let mut flushed=false;
    while !input.is_empty() {
        let length=std::str::from_utf8(input.get(..4).ok_or_else(invalid)?).ok()
            .and_then(|h|usize::from_str_radix(h,16).ok()).ok_or_else(invalid)?;
        input=&input[4..];
        if length==0 {flushed=true;continue;}
        if !(4..=65520).contains(&length) {return Err(invalid());}
        let packet=input.get(..length-4).ok_or_else(invalid)?;input=&input[length-4..];flushed=false;
        let mut end=0;while end<packet.len() && packet[end]!=0 {end+=1;}
        let reference=&packet[..end];
        let reference=reference.strip_suffix(b"\n").unwrap_or(reference);
        if reference.get(41..)==Some(b"refs/heads/main") {
            let sha=std::str::from_utf8(&reference[..40]).map_err(|_|invalid())?;
            if reference[40]!=b' ' || !revision_id(sha) || selected.is_some() {return Err(invalid());}
            selected=Some(sha.to_string());
        }
    }
    if !flushed {return Err(invalid());}
    selected.ok_or_else(||refused("shared revision","refs/heads/main is absent"))
}

fn shared_revision(root: &Path, fetch: &mut impl FnMut(&str)->Result<String,String>) -> Result<String,StartError> {
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|refused("shared revision clock",e))?.as_secs();
    shared_revision_at(root,now,fetch)
}

fn shared_revision_at(root: &Path, now: u64, fetch: &mut impl FnMut(&str)->Result<String,String>) -> Result<String,StartError> {
    let path=root.join(".taskq/specq/source.json");let mut cached=String::new();
    if File::open(&path).and_then(|f|f.take(1025).read_to_string(&mut cached)).is_ok() && cached.len()<=1024 {
        if let Ok(value)=serde_json::from_str::<serde_json::Value>(&cached) {
            if let (Some(revision),Some(checked))=(value["revision"].as_str(),value["checked_at"].as_u64()) {
                if revision_id(revision) && now.checked_sub(checked).is_some_and(|age|age<60) {return Ok(revision.into());}
            }
        }
    }
    let advertised=fetch(SPECQ_REFS).map_err(|e|refused("shared revision freshness",e))?;
    if advertised.len()>RULES_CAP {return Err(refused("shared revision","oversized advertisement"));}
    let revision=advertised_revision(&advertised)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e|refused("shared revision cache",e))?;
    publish_rule(&path,&serde_json::json!({"revision":revision,"checked_at":now}).to_string()).map_err(|e|refused("shared revision cache",e))?;
    Ok(revision)
}

/// A pinned specq file from `<root>/.taskq/specq/<revision>/`, fetched once when missing or invalid there (R1: a cache).
fn pinned(root: &Path, revision: &str, file: &str, expected: &str, fetch: &mut impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    cached_rule(root,revision,file,&format!("https://raw.githubusercontent.com/taskqorg/specq/{revision}/{file}"),|body| {
        if git_blob_hash(body)!=expected {Err(refused(file,format!("Git blob mismatch at {revision}; cache preserved")))} else {Ok(())}
    },fetch)
}

fn cached_rule(root: &Path, revision: &str, file: &str, url: &str, check: impl Fn(&str)->Result<(),StartError>, fetch: &mut impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    let path = specq_dir(root,revision).join(file);
    let metadata = path.with_extension("cache.json");
    let mut body = String::new();
    if File::open(&path).and_then(|f| f.take(RULES_CAP as u64+1).read_to_string(&mut body)).is_ok()
        && !body.trim().is_empty() && body.len()<=RULES_CAP {
        let mut stored = String::new();
        if File::open(&metadata).and_then(|f|f.take(1025).read_to_string(&mut stored)).is_ok() && stored.len()<=1024
            && serde_json::from_str::<serde_json::Value>(&stored).ok()==Some(rule_cache(revision,file,&body)) && check(&body).is_ok() {
            return Ok(body);
        }
    }
    let body = fetch(url).map_err(|e| refused(file, e))?;
    if body.trim().is_empty() || body.len()>RULES_CAP { return Err(refused(file, "empty or oversized")); }
    check(&body)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e|refused(file,e))?;
    publish_rule(&path,&body).and_then(|_|publish_rule(&metadata,&rule_cache(revision,file,&body).to_string())).map_err(|e|refused(file,e))?;
    Ok(body)
}

fn rule_cache(revision: &str, file: &str, body: &str) -> serde_json::Value {
    let digest=ring::digest::digest(&ring::digest::SHA256,body.as_bytes());
    serde_json::json!({"revision":revision,"file":file,"sha256":digest_hex(digest)})
}

fn digest_hex(digest: ring::digest::Digest) -> String {
    use std::fmt::Write as _;
    let mut hex=String::with_capacity(64);
    for byte in digest.as_ref() {write!(&mut hex,"{byte:02x}").expect("writing to String cannot fail");}
    hex
}

fn git_blob_hash(body: &str) -> String {
    let mut hash=ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY);
    hash.update(format!("blob {}\0",body.len()).as_bytes());hash.update(body.as_bytes());digest_hex(hash.finish())
}

fn shared_files(root: &Path, revision: &str, fetch: &mut impl FnMut(&str)->Result<String,String>) -> Result<std::collections::BTreeMap<String,String>,StartError> {
    let text=cached_rule(root,revision,"tree.json",&format!("{SPECQ_TREES}{revision}?recursive=1"),|body|parse_shared_tree(body).map(|_|()),fetch)?;
    parse_shared_tree(&text)
}

fn parse_shared_tree(text: &str) -> Result<std::collections::BTreeMap<String,String>,StartError> {
    let bad=||refused("shared tree","invalid, incomplete or incompatible tree");
    if text.len()>RULES_CAP {return Err(bad());}
    let value:serde_json::Value=serde_json::from_str(text).map_err(|_|bad())?;
    if value["truncated"]!=false || !value["sha"].as_str().is_some_and(revision_id) {return Err(bad());}
    let entries=value["tree"].as_array().ok_or_else(bad)?;
    if entries.len()>1024 {return Err(bad());}
    let mut files=std::collections::BTreeMap::new();
    let mut portable=std::collections::BTreeSet::new();
    for entry in entries {
        let kind=entry["type"].as_str().ok_or_else(bad)?;
        if !matches!(kind,"blob"|"tree"|"commit") {return Err(bad());}
        let path=entry["path"].as_str().ok_or_else(bad)?;
        if path.is_empty() {return Err(bad());}
        if kind!="blob" {continue;}
        if !path.ends_with(".md") {continue;}
        let parts=path.split('/').collect::<Vec<_>>();
        if path.contains(['\\',':','\0']) || parts.iter().any(|p|matches!(*p,""|"."|"..")) {return Err(bad());}
        if matches!(parts.last().copied(),Some("README.md"|"AGENTS.md"|"SECURITY.md")) || parts.iter().any(|p|matches!(*p,"scripts"|"vendor"|"audits"|"docs")) {continue;}
        for byte in path.bytes() {if !(byte.is_ascii_alphanumeric() || b"-_./".contains(&byte)) {return Err(bad());}}
        for part in &parts {
            let base=part.split('.').next().unwrap_or_default().to_ascii_uppercase();
            if part.ends_with('.') || matches!(base.as_str(),"CON"|"PRN"|"AUX"|"NUL")
                || matches!(base.as_bytes(),[b'C',b'O',b'M',b'1'..=b'9']|[b'L',b'P',b'T',b'1'..=b'9']) {return Err(bad());}
        }
        if !portable.insert(path.to_ascii_lowercase()) {return Err(bad());}
        if !matches!(entry["mode"].as_str(),Some("100644"|"100755")) {return Err(bad());}
        let hash=entry["sha"].as_str().filter(|s|revision_id(s)).ok_or_else(bad)?;
        if files.insert(path.to_string(),hash.to_string()).is_some() {return Err(bad());}
    }
    for required in SPECQ_CORE {if !files.contains_key(*required) {return Err(bad());}}
    Ok(files)
}

fn publish_rule(path: &Path, body: &str) -> Result<(),String> {
    // The shared atomic writer has a fixed temporary suffix; stage under a unique basename for concurrent launches.
    let stage=path.with_file_name(format!(".{}.{}",uuid(),path.file_name().unwrap().to_string_lossy()));
    let result=coreq::write_atomic(&stage,body).and_then(|_|std::fs::rename(&stage,path).map_err(|e|e.to_string()));
    if result.is_err() {let _=std::fs::remove_file(&stage);let _=std::fs::remove_file(stage.with_extension("tmp"));}
    result
}

fn heading(body: &str) -> &str {
    body.lines().find(|l| l.starts_with('#')).map_or("(no heading)", |l| l.trim_start_matches('#').trim())
}

/// `docs/` Markdown files below `root` with their first heading, sorted by `/` path; an unreadable one refuses the launch.
fn docs(root: &Path) -> Result<Vec<(String, String)>, StartError> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let (path, kind) = (entry.path(), entry.file_type()?);
            if kind.is_dir() {
                walk(root, &path, out)?;
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "md") {
                let rel = path.strip_prefix(root).unwrap().components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                let body = String::from_utf8(std::fs::read(&path)?).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{rel}: invalid UTF-8")))?;
                out.push((rel, heading(&body).to_string()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let dir = root.join("docs");
    if dir.is_dir() { walk(root, &dir, &mut out).map_err(|e| refused("docs index", e))?; }
    out.sort();
    Ok(out)
}

/// § 8 Rule delivery: current header, contract, core and index before every task/continuation.
fn rule_text(root: &Path, task: &str, resume: bool, fetch: impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    Ok(format!("{}\nTaskQ task\n{task}", rules(root, resume, fetch)?))
}

/// The mandatory bodies are repeated so continuation/compaction cannot silently retain old rules.
fn rules(root: &Path, resume: bool, fetch: impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    let text = bundle(root, resume, false, fetch)?;
    if text.len() > RULES_CAP { return Err(over_cap(root, text.len())); }
    Ok(text)
}

/// #1946: the cap refusal is deterministic, so the service asks once instead of retrying (`rules-cap`); it names the
/// contract file to trim, never a local path (§ 7 rule 6).
pub(crate) fn over_cap(root: &Path, len: usize) -> StartError {
    let name = contract_path(root).ok().and_then(|(p, _)| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "taskq.md".into());
    StartError { message: format!("rules bundle {len} bytes, {} over the {RULES_CAP}-byte cap\nNext: trim {name} below the cap, then retry", len - RULES_CAP), class: "rules-cap", reportable: false, no_start: true }
}

/// § 8 Rule delivery: `taskq rules size` measures the continuation bundle, the larger one, with the working-tree
/// contract a branch proposes, so project CI fails before the edit reaches worker starts (#1946). Ok: size and cap.
pub fn rules_size(root: &Path) -> Result<(usize, usize), StartError> {
    rules_size_with(root, rule_file)
}

fn rules_size_with(root: &Path, fetch: impl FnMut(&str) -> Result<String, String>) -> Result<(usize, usize), StartError> {
    let len = bundle(root, true, true, fetch)?.len();
    if len > RULES_CAP { Err(over_cap(root, len)) } else { Ok((len, RULES_CAP)) }
}

/// `tree`: the working-tree contract instead of the `origin/main` one starts use (#1479).
fn bundle(root: &Path, resume: bool, tree: bool, mut fetch: impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    let revision=shared_revision(root,&mut fetch)?;
    let _snapshot=tracing::info_span!("taskq.rules.snapshot",shared_spec=revision.as_str()).entered();
    let dir = specq_dir(root,&revision);
    let (spec, contract) = match if tree { None } else { base_contract(root)? } {
        Some((spec, Some(bytes))) => (spec, String::from_utf8(bytes).map_err(|_| refused("contract", "invalid UTF-8"))?),
        Some((_, None)) => (command_contract(), CONTRACT.into()),
        None => {
            let selected = project_contract(root)?;
            let spec = selected.as_ref().map_or_else(command_contract, |p|p.display().to_string());
            let contract = match selected {
                Some(path) => String::from_utf8(std::fs::read(&path).map_err(|e| refused("project contract", e))?).map_err(|_| refused("contract", "invalid UTF-8"))?,
                None => CONTRACT.into(),
            };
            (spec, contract)
        }
    };
    if contract.trim().is_empty() { return Err(refused("contract", "empty")); }
    // the header names the cache: every pinned file is there, at start and at resume
    let files=shared_files(root,&revision,&mut fetch)?;
    let core = SPECQ_CORE.iter().map(|f| pinned(root, &revision, f, &files[*f], &mut fetch)).collect::<Result<Vec<_>, _>>()?;
    let mut text = format!(
        "TaskQ rules: specq@{revision} at `{}`; contract `{spec}`.\n\
         Precedence: the project contract overrides shared rules where they conflict.\n\
         These rules apply to this turn, including continuations. Apply them to the work; acknowledgement alone is not evidence.\n\
         Evidence: cite the applied rule as R<n> or § n; a fix records the failure before and the pass after. GitHub PR publication needs CI green on the merge result (merge_group); private fallback uses exact-SHA CI.\n",
        dir.display()
    );
    text.push_str(&format!("TaskQ command contract (tool reference): {}\n",command_contract()));
    if resume {
        text.push_str("Continuation: these current contract and core bodies supersede earlier versions.\n");
    }
    text.push_str(&format!("\n--- contract {spec} ---\n{contract}\n"));
    for (file, body) in SPECQ_CORE.iter().zip(&core) { text.push_str(&format!("\n--- specq/{file} ---\n{body}\n")); }
    text.push_str(&format!("{INDEX} (read on demand under `{}`):\n", dir.display()));
    for (file,hash) in &files {
        if !SPECQ_CORE.contains(&file.as_str()) {text.push_str(&format!("- {file}: {}\n", heading(&pinned(root,&revision,file,hash,&mut fetch)?)));}
    }
    text.push_str("Index of project docs (read on demand in your worktree):\n");
    for (path, title) in docs(root)? { text.push_str(&format!("- {path}: {title}\n")); }
    Ok(text)
}

/// § 8 Rule delivery after a Claude compaction: restore the current mandatory bodies and index.
pub fn compact(root: &Path, role: &str, task: u64, attempt: u64) -> Result<String, StartError> {
    let text = rules(root, true, rule_file)?;
    log!("rules loaded for {role} #{task} attempt {attempt} (compact, {} bytes)", text.len());
    rules_line(&attempt_log(root, role, task, attempt), "compact", &text).map_err(|e| StartError::io(e, None))?;
    Ok(text)
}

/// § 8 Rule delivery to a PM session: the SessionStart hook restores the same current mandatory bodies and index
/// after compaction (`phase` `compact`) as a worker continuation.
pub fn pm(root: &Path, phase: &str) -> Result<String, StartError> {
    pm_rules(root, phase, rule_file)
}

fn pm_rules(root: &Path, phase: &str, fetch: impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    let text = rules(root, phase == "compact", fetch)?;
    log!("rules loaded for pm ({phase}, {} bytes)", text.len());
    Ok(text)
}

/// `--settings` for a Claude turn: a SessionStart hook that re-sends the rules after compaction, and the launch's TaskQ env.
fn compact_settings(root: &Path, install: &Path, l: &Launch, jobs: Option<usize>, build: Option<&Path>) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let exe = std::env::current_exe().unwrap_or_else(|_| "taskq".into());
    let cmd = format!("{} rules compact {} {} {} {}", q(&exe.to_string_lossy()), q(&root.to_string_lossy()), q(&l.role), l.task, l.attempt);
    let attempt = (l.role != "operator").then_some(l);
    let mut env = claude_env(install, Some(root), attempt);
    if let Some(jobs) = jobs { env["CARGO_BUILD_JOBS"] = jobs.to_string().into(); }
    // the daemon's env is not the service's: the build cache travels in settings too
    if let Some(p) = build { env["RUSTC_WRAPPER"] = p.to_string_lossy().into(); }
    serde_json::json!({"hooks": {"SessionStart": [{"matcher": "compact", "hooks": [{"type": "command", "command": cmd}]}]}, "env": env}).to_string()
}

/// #1531: `claude --bg` sessions run under the per-user Claude daemon, whose env is that of whoever started it;
/// settings `env` overrides it per session and survives resume. Settings cannot unset: empty means unset (entry.rs).
pub(crate) fn claude_env(install: &Path, root: Option<&Path>, l: Option<&Launch>) -> serde_json::Value {
    let s = |p: Option<&Path>| p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    serde_json::json!({"TASKQ_INSTALL": s(Some(install)), "TASKQ_ROOT": s(root),
        "TASKQ_TASK": l.map(|l| l.task.to_string()).unwrap_or_default(), "TASKQ_ATTEMPT": l.map(|l| l.attempt.to_string()).unwrap_or_default(),
        "TASKQ_ROLE": l.map(|l| l.role.clone()).unwrap_or_default()})
}

/// Current rules and turn text (§ 8 Rule delivery) for the Windows boundary qualification.
#[cfg(all(windows, test))]
pub fn instructions(root: &Path, task: &str, resume: bool) -> Result<String, StartError> {
    rule_text(root, task, resume, rule_file)
}

// Claude --bg forwards stdin as child argv; send only a file reference.
fn claude_reference(path:&Path)->Result<String,String> {
    Ok(format!("Before any work, use Read to read {} in full. It contains this turn's TaskQ rules and task. Apply those rules and carry out that task. If Read truncates the file, read the remaining parts before proceeding.", serde_json::to_string(&path.to_string_lossy()).map_err(|e| e.to_string())?))
}

fn app_reference(path:&Path)->Result<String,String> {
    Ok(format!("Before any work, read {} in full using your file tools. It contains this turn's TaskQ rules and task. Apply those rules and carry out that task. Read all remaining parts if a tool truncates the file. If the file cannot be read completely, report the failure and do not proceed with the task.", serde_json::to_string(&path.to_string_lossy()).map_err(|e| e.to_string())?))
}

fn claude_input(root:&Path,text:&str,launch:Option<&Launch>)->Result<(String,crate::data::InputLock,PathBuf),String> {
    let (path,file)=turn_input(root,text,launch)?;
    Ok((claude_reference(&path)?,file,path))
}

fn app_input(root:&Path,text:&str,launch:Option<&Launch>)->Result<(String,crate::data::InputLock,PathBuf),String> {
    let (path,file)=turn_input(root,text,launch)?;
    Ok((app_reference(&path)?,file,path))
}

fn execution_input(root:&Path,task:u64,attempt:u64,execution:u64)->Result<PathBuf,String> {
    if task==0 || attempt==0 || execution<attempt {return Err("invalid execution identity for native input".into());}
    Ok(root.canonicalize().map_err(|e|e.to_string())?.join(".taskq/inputs").join(format!("execution-{task}-{attempt}-{execution}.txt")))
}

fn turn_input(root: &Path, text: &str, launch:Option<&Launch>) -> Result<(PathBuf, crate::data::InputLock), String> {
    let dir = root.join(".taskq/inputs");
    let path=match launch.filter(|l|l.task>0) {
        Some(l)=>execution_input(root,l.task,l.attempt,l.execution.ok_or("exact execution intent missing before native input creation")?)?,
        None=>dir.join(format!("{}.txt",uuid())),
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut options = File::options();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    let file = coreq::file_lock::try_lock_shared_file(file).map_err(|e| e.to_string())?;
    Ok((path, file))
}

fn record_input(log:&Path,record:&serde_json::Value)->Result<(),String> {
    let mut file=File::options().create(true).append(true).open(log).map_err(|e|e.to_string())?;
    writeln!(file,"{record}").and_then(|_|file.sync_all()).map_err(|e|format!("native input binding not persisted: {e}"))
}

/// Recovery locator only: neither a before-call marker nor its presence proves submission or no-start.
fn bind_input(log:&Path,l:&Launch,path:&Path,input:&str,native:serde_json::Value)->Result<serde_json::Value,String> {
    let execution=if l.task==0 {0} else {l.execution.filter(|id|l.attempt>0 && *id>=l.attempt).ok_or("exact execution intent missing before native input submission")?};
    let record=serde_json::json!({"type":"taskq.input","task":l.task,"attempt":l.attempt,"execution":execution,"runtime":l.rt,
        "path":path.to_str().ok_or("native input path is not UTF-8")?,"input":input,"native":native});
    record_input(log,&record)?;
    Ok(record)
}

/// The log supplies native lookup hints; the board-derived input reference is the identity boundary.
fn recovery_binding(root:&Path,task:u64,x:&crate::core::Execution)->Result<(PathBuf,serde_json::Value,crate::data::InputLock),String> {
    let path=execution_input(root,task,x.attempt,x.id)?;
    if !std::fs::symlink_metadata(&path).map_err(|_|"original execution input unavailable; inspect legacy or missing native binding")?.is_file() {
        return Err("original execution input is not a regular file".into());
    }
    let input=match x.runtime.as_str() {"claude"=>claude_reference(&path)?,"codex-app"=>app_reference(&path)?,_=>return Err("native input recovery unsupported for this runtime".into())};
    let worker=attempt_log(root,"worker",task,x.attempt);let reviewer=attempt_log(root,"reviewer",task,x.attempt);
    let log=match (worker.is_file(),reviewer.is_file()) {(true,false)=>worker,(false,true)=>reviewer,_=>return Err("native attempt log missing or ambiguous; preserve the execution".into())};
    let mut binding=None;let mut reader=NativeHistory::default();
    let until=observation_deadline(std::time::Duration::from_secs(5));
    loop {
        let complete=reader.lines(&log,|line| {
            if !line.trim_start().starts_with('{') {return Ok(());}
            let v:serde_json::Value=serde_json::from_str(line).map_err(|_|"native attempt record incomplete or invalid")?;
            if v["type"]!="taskq.input" || v["execution"].as_u64()!=Some(x.id) {return Ok(());}
            if v["task"].as_u64()!=Some(task) || v["attempt"].as_u64()!=Some(x.attempt) || v["runtime"]!=x.runtime || v["path"].as_str()!=path.to_str() || v["input"]!=input || !v["native"].is_object() {
                return Err("native input binding differs from the board execution; inspect original submission".into());
            }
            binding=Some(v);Ok(())
        })?;
        if complete {break;}
        if std::time::Instant::now()>=until {return Err("native input binding scan exceeded 5 seconds; original execution retained".into());}
    }
    let binding=binding.ok_or("exact execution input binding missing; inspect original submission")?;
    let file=File::open(&path).map_err(|e|e.to_string())?;
    let lock=coreq::file_lock::try_lock_shared_file(file).map_err(|e|e.to_string())?;
    Ok((log,binding,lock))
}

fn claude_returned_id(stdout:&str)->Option<&str> {
    stdout.lines().find_map(|line|line.strip_prefix("backgrounded · ").and_then(|s|s.split_whitespace().next())).filter(|id|native_job_id(id))
}

/// Confirmed no-start (#1557): the launcher exited non-zero, printed no background ID and said why on stderr.
/// `status`: `Some(code)` observed (`None` code: killed by a signal, unknown); outer `None`: a record from before
/// the status was saved, whose non-empty stderr without an ID only a refusal produced.
fn claude_refusal(returned:bool,stderr:&str,status:Option<Option<i32>>)->Option<String> {
    let stderr=stderr.trim();
    let refused=!returned && !stderr.is_empty() && status.is_none_or(|code|code.is_some_and(|c|c!=0));
    refused.then(||stderr.lines().map(str::trim).filter(|l|!l.is_empty()).collect::<Vec<_>>().join(" "))
}

/// The refusal of an unaccepted Claude execution's own launch, from its attempt log; any doubt reads `None` (unknown).
pub fn refused_launch(root:&Path,task:u64,x:&crate::core::Execution)->Option<String> {
    if x.runtime!="claude" || x.accepted || x.release.is_some() {return None;}
    let logs=[attempt_log(root,"worker",task,x.attempt),attempt_log(root,"reviewer",task,x.attempt)];
    let [log]=logs.iter().filter(|p|p.is_file()).collect::<Vec<_>>()[..] else {return None};
    let text=std::fs::read_to_string(log).ok()?;
    let mut bound=false;
    for line in text.lines().filter(|l|l.trim_start().starts_with('{')) {
        let v:serde_json::Value=serde_json::from_str(line).ok()?;
        if v["type"]=="taskq.input" {
            if bound {return None;}
            bound=v["execution"].as_u64()==Some(x.id);
        } else if bound && v["type"]=="claude.launch" {
            let status=v.get("status").map(|s|s.as_i64().and_then(|c|i32::try_from(c).ok()));
            if v.get("status").is_some_and(|s|!s.is_null() && !s.is_i64()) {return None;}
            return claude_refusal(claude_returned_id(v["stdout"].as_str()?).is_some(),v["stderr"].as_str()?,status).map(|r|public_line(&r,root,200));
        }
    }
    None
}

/// Service start: the native handle of a saved kid. Claude reconnects to its background job; a CLI child is its saved
/// process with the attempt log read from the saved offset.
pub fn adopt(root:&Path,install:&Path,k:&crate::data::KidLine,x:&crate::core::Execution)->Result<Box<dyn Proc>,String> {
    if of(&k.runtime).remote {return recover(root,install,k.task,x);}
    #[cfg(windows)]
    let native:Box<dyn Proc>=Box::new(windows_process::Execution::recover(k.process.clone(),k.job.as_deref().ok_or("legacy Windows execution has no native job scope; file kept")?)?);
    #[cfg(unix)]
    let native:Box<dyn Proc>={
        if k.job.is_some() {return Err("Windows native scope cannot be adopted on Unix; file kept".into());}
        Box::new(Adopted::new(k.process.clone()))
    };
    let worker=attempt_log(root,"worker",k.task,k.attempt);
    let reviewer=attempt_log(root,"reviewer",k.task,k.attempt);
    let log=match (worker.is_file(),reviewer.is_file()) {
        (true,false)=>worker,(false,true)=>reviewer,
        _=>return Err("kids: native attempt log missing or ambiguous; file kept".into()),
    };
    let session = (k.runtime == "antigravity").then(|| native_session(&k.runtime, &log).unwrap_or_else(|| "-".into()));
    Ok(Box::new(CliTurn {proc:native,log,pos:k.offset,start:k.offset,stopping:false,accepted:false,read_error:None,session,native:Some(k.process.clone()),recovery:None}))
}

/// § 8 step 3 outside Windows: a Codex app turn waits for a re-exec, and so does a Claude turn without a saved native
/// identity (#1407); CLI children pass over.
pub fn handover_waits(rt:&str,p:&dyn Proc)->bool {
    let a=of(rt);
    a.app || a.remote&&p.native_identity().is_err()
}

/// Reconnect observation only. This path cannot submit a prompt, queue a turn or change access.
pub fn recover(root:&Path,install:&Path,task:u64,x:&crate::core::Execution)->Result<Box<dyn Proc>,String> {
    if x.release.is_some() {return Err("released execution cannot be recovered as live".into());}
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let saved=crate::kids_file(install,&root)?.into_iter().find(|k|k.task==task);
    if saved.as_ref().is_some_and(|k|k.execution!=x.id || k.attempt!=x.attempt || k.runtime!=x.runtime) {
        return Err("protected native recovery identity differs from board execution; preserve the record".into());
    }
    let (log,binding,input_file)=recovery_binding(&root,task,x)?;
    let input=binding["input"].as_str().ok_or("native recovery input missing")?.to_owned();
    match x.runtime.as_str() {
        "claude"=>Ok(Box::new(ClaudeTurn {root:root.clone(),log,session:if x.id==x.attempt {String::new()} else {binding["native"]["session"].as_str().unwrap_or_default().to_owned()},
            id:String::new(),announced:false,accepted:false,history:ClaudeHistory::default(),before:0,mode:"unknown: recovered observer".into(),input,_input_file:input_file,pid:0,done:false,failure:None,
            native:saved.map(|k|k.process),recovery:Some(Recovery {install:install.into(),root:root.clone(),task,attempt:x.attempt,execution:x.id,runtime:"claude".into()}),
            launch_error:Some("recovered execution; original native input acceptance pending".into())})),
        "codex-app"=>{
            let thread=binding["native"]["thread"].as_str().filter(|s|s.len()>1 && s.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'-')).ok_or("native App thread identity missing; inspect original handshake, never requeue")?.to_owned();
            Ok(Box::new(AppTurn {root,history:AppHistory::default(),hand:None,log,text:input,_input:Some(input_file),binding:Some(binding),thread,done:0,rollout:None,running:app_running,control:app_cancel,cancelled:false,
                queue_error:Some("recovered execution; original App input acceptance pending".into()),accepted:false}))
        },
        _=>Err("native input recovery unsupported for this runtime".into()),
    }
}

fn turn_text(env: &Env, l: &Launch, fetch: impl FnMut(&str) -> Result<String, String>) -> Result<String, StartError> {
    let mut task = brief(env, l);
    match &l.resume {
        Some(answer) => {
            task.push_str(&format!("\nContinue this same attempt with the following inbox message:\n{answer}"));
            if turn_inbox(answer).is_none() { task.push_str(&format!("\n{}", turn_marker(&[("answer", answer)]))); }
        }
        None => task.push_str(&format!("\n{}", turn_marker(&[]))),
    }
    rule_text(env.root, &task, l.resume.is_some(), fetch)
}

const TURN_MARKER: &str = "taskq-turn ";

/// #1834: the last line of every input the service composes for a worker session marks TaskQ as its author and lists
/// the owner's inbox entries it delivers (§ 4 step 6), so the live worker view labels turns by author, not by wording.
pub fn turn_marker(inbox: &[(&str, &str)]) -> String {
    let inbox: Vec<serde_json::Value> = inbox.iter().map(|(kind, text)| serde_json::json!({"kind": kind, "text": text})).collect();
    format!("{TURN_MARKER}{}", serde_json::json!({"inbox": inbox}))
}

/// The inbox entries named by the marker on the last line of `text`; None when it has none (an older input).
pub fn turn_inbox(text: &str) -> Option<Vec<serde_json::Value>> {
    let line = text.trim_end().rsplit('\n').next()?.strip_prefix(TURN_MARKER)?;
    serde_json::from_str::<serde_json::Value>(line).ok()?["inbox"].as_array().cloned()
}

/// Keep OS failure classes until the service decides whether a start failure is TaskQ's fault.
#[derive(Debug)]
pub struct StartError {
    pub(crate) message: String,
    class: &'static str,
    pub reportable: bool,
    pub no_start: bool,
}

impl StartError {
    pub fn admission(message: impl Into<String>) -> Self {
        Self {message:message.into(),class:"admission",reportable:false,no_start:true}
    }
    pub fn class(&self)->&'static str {self.class}
    pub fn io(error: std::io::Error, runtime: Option<&str>) -> Self {
        use std::io::ErrorKind;
        let reportable = matches!(error.kind(), ErrorKind::Other | ErrorKind::InvalidData | ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe);
        let message = runtime.map_or_else(|| error.to_string(), |rt| format!("{rt}: {error}"));
        Self { message, class:crate::profile::io_class(&error), reportable, no_start: true }
    }
}

impl From<String> for StartError {
    fn from(message: String) -> Self { Self { class:crate::profile::class(&message), message, reportable: false, no_start: true } }
}
impl From<&str> for StartError {
    fn from(message: &str) -> Self { message.to_string().into() }
}
impl std::error::Error for StartError {}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.message.fmt(f) }
}

/// Prepare stdin before spawn: a child that never reads cannot block its execution owner.
pub fn start_command(command: &mut Command, root: &Path, text: &str) -> Result<Box<dyn Proc>, StartError> {
    coreq::telemetry::capture::operation("runtime.launch", || {
        let (path, _input) = turn_input(root, text, None).map_err(StartError::from)?;
        let input = File::open(path).map_err(|e| StartError::io(e, None))?;
        start_file(command,input)
    }, |_| "launch")
}

fn start_file(command:&mut Command,input:File)->Result<Box<dyn Proc>,StartError> {
    coreq::telemetry::capture::inherit_context(command);
    command.stdin(input);
    #[cfg(windows)] {windows_process::Execution::spawn(command).map(|child|Box::new(child) as Box<dyn Proc>)}
    #[cfg(not(windows))] {command.spawn().map(|child|Box::new(child) as Box<dyn Proc>).map_err(|e|StartError::io(e,None))}
}

pub fn acceptance_action(rt:&str)->&'static str {
    if rt=="codex" {"input acceptance unknown; inspect the pending input in Codex; let any active turn finish before closing that thread; reconcile acceptance before retry"}
    else {"input acceptance unknown; reconcile native input"}
}

/// § 8 Access of `rt` here, resolved again for every start and resume; a refusal starts nothing.
pub fn access(env: &Env, rt: &str) -> Result<crate::policy::Effective, StartError> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let a = crate::policy::resolve(&crate::policy::load(env.install), &crate::policy::Host::here(env.install, env.root, &home), rt, env.permission_mode, env.codex);
    match &a.refused {
        Some(r) => Err(StartError { message: format!("access refused: {r}"), class:"permission", reportable: false, no_start: true }),
        None => Ok(a),
    }
}

/// Start the child with its stdout and stderr in the attempt log; `codex-app` starts its handshake child (§ 8).
pub fn spawn(env: &Env, l: &Launch) -> Result<Box<dyn Proc>, StartError> {
    crate::profile::measure_classified("spawn", || spawn_inner(env, l),StartError::class)
}

fn spawn_inner(env: &Env, l: &Launch) -> Result<Box<dyn Proc>, StartError> {
    let access = access(env, &l.rt)?;
    log!("#{} attempt {} access {}", l.task, l.attempt, access.line());
    let text = turn_text(env, l, rule_file)?;
    let phase = if l.resume.is_some() { "resume" } else { "start" };
    log!("rules loaded for {} #{} attempt {} ({phase}, {} bytes)", l.role, l.task, l.attempt, text.len());
    if crate::admission::task_hold(env.install,env.root,l.task).is_some() {return Err(StartError::admission("host held before native launch"));}
    let log = log_path(env.root, l);
    rules_line(&log, phase, &text).map_err(|e| StartError::io(e, None))?;
    // § 3 Runtime accounts: a new start takes the host's active account; a resume keeps its log's account.
    if l.resume.is_none() && crate::runtime::accounts::applies(&l.rt) {
        let account = crate::runtime::accounts::claim(env.install, &l.rt, None, crate::runtime::accounts::now(), &log)
            .map_err(|e| StartError::admission(&e))?;
        crate::runtime::accounts::mark(&log, &account).map_err(|e| StartError::io(e, None))?;
        if account != crate::runtime::accounts::DEFAULT { log!("#{} attempt {} account {} {account}", l.task, l.attempt, l.rt); }
    }
    if l.resume.is_some() || l.rt == "antigravity" {
        crate::runtime::accounts::claim(env.install, &l.rt, Some(&crate::runtime::accounts::of_log(&log)), crate::runtime::accounts::now(), &log)
            .map_err(|e| StartError::admission(&e))?;
    }
    let home = crate::runtime::accounts::log_home(&l.rt, &log);
    if l.rt == "codex-app" {
        let (text, input_file, path) = app_input(env.root, &text, Some(l)).map_err(StartError::from)?;
        let binding=bind_input(&log,l,&path,&text,serde_json::json!({"thread":l.session}))?;
        let mut turn = AppTurn { root: env.root.canonicalize().map_err(|e| StartError::io(e,None))?, history:AppHistory::default(), hand: None, log, text, _input: Some(input_file), binding:Some(binding), thread: l.session.clone(), done: 0, rollout: None, running: app_running, control:app_cancel, cancelled: false, queue_error: None, accepted: false };
        match &l.resume {
            Some(_) => {
                if let Err(message) = turn.queue() { turn.queue_error = Some(format!("{message}; submission outcome unknown")); }
            }
            None => turn.hand = Some(start(env, l, &turn.log, &format!("{}\n{HANDSHAKE}{}", session_name(l), turn_marker(&[])), &access)?),
        }
        return Ok(Box::new(turn));
    }
    if l.rt == "claude" && (l.resume.is_none() || claude_background(env.root, &l.session, home.as_deref()).map_err(StartError::from)?) {
        return ClaudeTurn::start(env, l, &log, &text, &access).map(|p| Box::new(p) as Box<dyn Proc>);
    }
    let pos = std::fs::metadata(&log).map_or(0, |m| m.len());
    // R3: a new CLI conversation takes its title from the first input line; the rules follow unchanged.
    let text = if l.resume.is_none() && of(&l.rt).child && !of(&l.rt).remote { format!("{}\n{text}", session_name(l)) } else { text };
    let child = start(env, l, &log, &text, &access)?;
    let native=match child.native_identity() {
        Ok(identity)=>Some(identity),
        Err(e)=>{log!("#{} process recovery identity unknown: {e}",l.task);None}
    };
    Ok(Box::new(CliTurn { proc: child, log, pos, start: pos, stopping: false, accepted: false, read_error: None, session: (l.rt == "antigravity").then(|| l.session.clone()), native, recovery: None }))
}

/// The attempt log's evidence of § 8 Rule delivery: the injected header and part counts, one JSON line.
pub fn rules_line(log: &Path, phase: &str, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(log.parent().unwrap())?;
    let rules = text.split("\nTaskQ task\n").next().unwrap_or_default();
    let header = rules.split("\n\n").next().unwrap_or_default();
    let bodies = rules.matches("\n--- contract ").count() + rules.matches("\n--- specq/").count();
    let index = rules.find(INDEX).map_or(0, |i| rules[i..].lines().filter(|l| l.starts_with("- ")).count());
    let revision=rules.strip_prefix("TaskQ rules: specq@").and_then(|s|s.split_whitespace().next()).filter(|s|revision_id(s));
    let line = serde_json::json!({"type": "taskq.rules", "phase": phase, "header": header, "shared_revision": revision, "bodies": bodies, "index": index, "bytes": rules.len()});
    writeln!(File::options().create(true).append(true).open(log)?, "{line}")
}

fn start(env: &Env, l: &Launch, log: &Path, text: &str, access: &crate::policy::Effective) -> Result<Box<dyn Proc>, StartError> {
    std::fs::create_dir_all(log.parent().unwrap()).map_err(|e| StartError::io(e, None))?;
    let out = File::options().create(true).append(true).open(log).map_err(|e| StartError::io(e, None))?;
    let err = out.try_clone().map_err(|e| StartError::io(e, None))?;
    let mut c = command(env, l, access).map_err(StartError::from)?;
    let text = stdin_text(&l.rt, text);
    if l.rt=="codex" && l.task>0 && l.resume.is_some() {
        let (path,_input)=turn_input(env.root,&text,Some(l))?;
        bind_input(log,l,&path,&text,serde_json::json!({"session":l.session}))?;
        return coreq::telemetry::capture::operation("runtime.launch",|| {
            let input=File::open(path).map_err(|e|StartError::io(e,None))?;
            start_file(c.stdout(out).stderr(err),input)
        },|_|"launch");
    }
    start_command(c.stdout(out).stderr(err), env.root, &text)
}

/// The agy streaming protocol carries one JSON user event, including the literal brief.
fn stdin_text(rt: &str, text: &str) -> String {
    if rt == "antigravity" { format!("{}\n", serde_json::json!({"event":"user","message":{"content":text}})) } else { text.into() }
}

/// Native background sessions are not children of TaskQ. Keep their full identity and
/// observe the completed input before releasing the attempt's capacity.
pub struct ClaudeTurn {
    root: PathBuf,
    log: PathBuf,
    session: String,
    id: String,
    announced: bool,
    accepted: bool,
    history: ClaudeHistory,
    before: usize,
    mode: String,
    input: String,
    _input_file: crate::data::InputLock,
    pid: u32,
    native: Option<coreq::process::Identity>,
    recovery: Option<Recovery>,
    done: bool,
    failure: Option<&'static str>,
    launch_error: Option<String>,
}

pub(crate) fn claude_agents(root: &Path) -> Result<Vec<serde_json::Value>, String> { claude_agents_in(root, None) }

/// Claude's native jobs in a § 3 runtime account's home (`None`: the service's own home).
fn claude_agents_in(root: &Path, home: Option<&Path>) -> Result<Vec<serde_json::Value>, String> {
    let budget = observation_left(std::time::Duration::from_secs(10));
    if budget.is_zero() { return Err("Claude agents unavailable or timed out".into()); }
    agentq_runtime::agent::background_jobs(crate::platform::command(which("claude")), &claude_spec(root, home), budget).map_err(|e| format!("claude agents: {e}"))
}

pub(crate) fn claude_home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).or_else(||
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|h| PathBuf::from(h).join(".claude")))
}

pub(crate) fn claude_failure(row: &serde_json::Value, root: &Path, id: &str, home: Option<&Path>) -> &'static str {
    let detail = row["detail"].as_str().map(String::from).or_else(|| {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') { return None; }
        let text = std::fs::read_to_string(home?.join("jobs").join(id).join("state.json")).ok()?;
        let job: serde_json::Value = serde_json::from_str(&text).ok()?;
        if job["state"] != row["state"] || Path::new(job["cwd"].as_str()?).canonicalize().ok()? != root.canonicalize().ok()? { return None; }
        job["detail"].as_str().map(String::from)
    });
    crate::profile::class(detail.as_deref().unwrap_or_default())
}

/// The session's transcript in the default home or a § 3 runtime account's home.
fn claude_transcript(session: &str) -> Option<PathBuf> {
    crate::runtime::accounts::homes("claude").iter().find_map(|home| claude_transcript_in(home,session).ok())
}

pub(crate) fn claude_transcript_in(home:&Path,session:&str)->Result<PathBuf,String> {
    if session.len()!=36 || !native_job_id(session) {return Err("Claude session identity invalid".into());}
    let mut found=None;
    for (i,entry) in std::fs::read_dir(home.join("projects")).map_err(|e|e.to_string())?.enumerate() {
        if i>=4096 {return Err("Claude transcript directory scan exceeds 4096 entries".into());}
        let directory=entry.map_err(|e|e.to_string())?.path();
        match std::fs::metadata(&directory) {
            Ok(meta) if !meta.is_dir()=>continue,
            Ok(_)=>{},
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>continue,
            Err(e)=>return Err(format!("Claude transcript directory unreadable: {e}")),
        }
        let path=directory.join(format!("{session}.jsonl"));
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file()=>{
                if found.is_some() {return Err("Claude session has multiple transcript paths; inspect native history".into());}
                found=Some(path);
            }
            Ok(_)=>return Err("Claude transcript is not a regular file".into()),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            Err(e)=>return Err(format!("Claude transcript lookup failed: {e}")),
        }
    }
    found.ok_or_else(||"Claude transcript unavailable; completion unknown".into())
}

pub(crate) fn native_job_id(id:&str)->bool {
    (id.len()==8 && id.bytes().all(|b|b.is_ascii_hexdigit())) ||
        (id.len()==36 && id.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i) {b==b'-'} else {b.is_ascii_hexdigit()}))
}

fn claude_input_event(event:&serde_json::Value,input:&str)->bool {
    if event["type"]!="user" {return false;}
    let content=&event["message"]["content"];
    content.as_str().or_else(||content.as_array().filter(|a|a.len()==1 && a[0]["type"]=="text")
        .and_then(|a|a[0]["text"].as_str())).is_some_and(|s|s.trim()==input)
}

/// One pre-submission permission/baseline read, using the same bounded native decoder.
fn claude_history(path:&Path,before:usize,mut visit:impl FnMut(serde_json::Value))->Result<usize,String> {
    let mut reader=NativeHistory::default();let mut count=0;
    let until=observation_deadline(std::time::Duration::from_secs(5));
    loop {
        let complete=reader.read(path,|v| {if count>=before {visit(v);}count+=1;Ok(())})?;
        if complete {
            if count<before {return Err("Claude history truncated before input boundary".into());}
            return Ok(count);
        }
        if std::time::Instant::now()>=until {return Err("Claude baseline scan exceeded 5 seconds; no input submitted".into());}
    }
}

#[derive(Default)]
struct ClaudeHistory {
    reader: NativeHistory,
    count: usize,
    found: bool,
    ended: bool,
    answer: Option<String>,
    failure: Option<(&'static str,String)>,
}

impl ClaudeHistory {
    fn read(&mut self,path:&Path,before:usize,input:&str,log:&Path)->Result<bool,String> {
        let mut output=File::options().create(true).append(true).open(log).map_err(|e|e.to_string())?;
        let Self {reader,count,found,ended,answer,failure}=self;
        let complete=reader.read(path,|v| {
            let starts=*count>=before && claude_input_event(&v,input);
            if starts && *found {return Err("Claude original input appears in multiple turns; inspect native submissions".into());}
            if (*found || starts) && !*ended {
                writeln!(output,"{v}").map_err(|e|format!("Claude native observation log failed: {e}"))?;
                if v["type"]=="assistant" {
                    let text=v["message"]["content"].as_array().and_then(|blocks|blocks.iter().rev().find_map(|b|(b["type"]=="text").then(||b["text"].as_str()).flatten()));
                    if let Some(code)=v["error"].as_str().or_else(||(v["isApiErrorMessage"]==true).then_some("unknown")) {
                        let text=text.unwrap_or("native API error without detail");
                        let class=if code=="authentication_failed" {"auth"} else {crate::profile::class(text)};
                        *failure=Some((class,format!("{code}: {text}")));*answer=None;
                    } else if let Some(text)=text {*answer=Some(text.to_owned());*failure=None;}
                }
                *ended=v["type"]=="system" && v["subtype"]=="turn_duration";
            }
            *found|=starts;*count+=1;
            Ok(())
        })?;
        if !complete {return Err("Claude native history scan pending; retry observation without submission".into());}
        if *count<before {return Err("Claude history truncated before submitted input boundary".into());}
        Ok(*found)
    }
}

/// Native intent locates a candidate; only its user-message readback can establish acceptance.
fn claude_job_input(home:&Path,id:&str,session:&str,root:&Path,input:&str,budget:&mut u64)->Result<bool,String> {
    #[derive(serde::Deserialize)]
    struct Input {cwd:String,#[serde(rename="sessionId")] session:String,intent:Option<String>}
    let path=home.join("jobs").join(id).join("state.json");
    let meta=std::fs::metadata(&path).map_err(|_|"Claude native input locator unavailable; inspect original job")?;
    if !meta.is_file() || meta.len()>1024*1024 {return Err("Claude native input locator is nonregular or exceeds 1 MiB".into());}
    let mut reader=File::open(path).map_err(|_|"Claude native input locator unreadable")?.take(budget.saturating_add(1));
    let limit=reader.limit();
    // Deserialize only locator fields; provider environment and other payloads are never retained or logged.
    let value:Result<Input,_>=serde_json::from_reader(&mut reader);
    *budget=budget.checked_sub(limit-reader.limit()).ok_or("Claude recovery scan exceeds 8 MiB; original submission remains unknown")?;
    let value=value.map_err(|_|"Claude native input locator invalid")?;
    if value.session!=session || Path::new(&value.cwd).canonicalize().map_err(|_|"Claude native input locator project unavailable")?!=root {
        return Err("Claude native input locator identity differs from native job list".into());
    }
    Ok(value.intent.as_deref().is_some_and(|text|text.trim()==input))
}

fn claude_stop(root: &Path, id: &str, known: Option<&coreq::process::Identity>, home: Option<&Path>) -> Result<(), String> {
    if !native_job_id(id) {return Err("Claude native job identity unavailable; reconcile the original input before stopping".into());}
    let rows = claude_agents_in(root, home)?;
    let row = rows.iter().find(|v| v["id"] == id).ok_or("Claude stop identity unavailable")?;
    if let (Some(pid),Some(saved))=(claude_pid(row)?,known) {
        if pid!=saved.pid {return Err("Claude native process changed; preserve the execution for reconciliation".into());}
    }
    // An unknown outcome must not prevent requesting native cancellation of the known job.
    if matches!(claude_stopped(row, known),Ok(true)) { return Ok(()); }
    let budget = observation_left(std::time::Duration::from_secs(10));
    if budget.is_zero() { return Err("Claude stop unavailable or timed out; state unknown".into()); }
    let spec = claude_spec(root, home);
    agentq_runtime::agent::background_stop(crate::platform::command(which("claude")), &spec, &agentq_runtime::agent::BackgroundJob::read(row, &spec, id)?, budget)
        .map_err(|e| format!("claude stop: {e}"))?;
    // Native stop acknowledges the request first; the service owns later retries/readback.
    let rows = claude_agents_in(root, home)?;
    let row = rows.iter().find(|v| v["id"] == id).ok_or("stopped Claude session disappeared; state unknown")?;
    if claude_stopped(row, known)? { Ok(()) } else { Err("Claude stop still pending; do not launch another turn".into()) }
}

fn claude_pid(row: &serde_json::Value) -> Result<Option<u32>, String> {
    if row["pid"].is_null() { return Ok(None); }
    row["pid"].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).map(Some).ok_or_else(|| "Claude process identity invalid".into())
}

fn claude_stopped(row: &serde_json::Value, known: Option<&coreq::process::Identity>) -> Result<bool, String> {
    let state = row["state"].as_str().ok_or("Claude state unavailable; stop unknown")?;
    let pid=claude_pid(row)?;
    if !matches!(state,"working"|"idle"|"done"|"blocked"|"failed"|"crashed"|"stopped") {
        return Err(format!("Claude state {state:?} unsupported; inspect native lifecycle before release"));
    }
    if matches!(state,"working"|"idle") || pid.is_some() { return Ok(false); }
    // Native stop may retain done; work state alone never establishes process exit.
    let known=known.ok_or("Claude process exit evidence unavailable; inspect the original native job before release")?;
    coreq::process::identity(known.pid).map(|current|current.as_ref()!=Some(known)).map_err(|e|format!("Claude process identity unreadable: {e}"))
}

impl ClaudeTurn {
    /// The § 3 runtime account home the attempt log names; None: the service's own home.
    fn account(&self) -> Option<PathBuf> { crate::runtime::accounts::log_home("claude", &self.log) }

    /// The native home that holds this attempt's jobs and transcripts.
    fn native_home(&self) -> Option<PathBuf> { self.account().or_else(claude_home) }

    fn agents(&self) -> Result<Vec<serde_json::Value>, String> { claude_agents_in(&self.root, self.account().as_deref()) }

    fn recover_job(&mut self,rows:&[serde_json::Value],home:Option<&Path>)->Result<(),String> {
        if self.accepted || (!self.id.is_empty() && self.launch_error.is_none()) {return Ok(());}
        let home=home.ok_or("Claude native home unavailable; original submission remains unknown")?;
        let mut budget=8*1024*1024;
        let mut matched=None;
        for row in rows {
            if row["kind"]=="interactive" {continue;}
            if row["kind"]!="background" {return Err("Claude recovery job kind unknown".into());}
            let root=row["cwd"].as_str().ok_or("Claude recovery project unknown")?;
            // Native --cwd includes descendants; TaskQ submits the exact canonical project root.
            // #1627: native rows name the plain path; Windows canonical paths carry the `\\?\` prefix.
            if Path::new(root).canonicalize().ok().as_deref()!=Some(self.root.as_path()) {continue;}
            let session=row["sessionId"].as_str().filter(|s|s.len()==36 && native_job_id(s)).ok_or("Claude recovery session unknown")?;
            let id=row["id"].as_str().filter(|id|native_job_id(id)).ok_or("Claude native job identity unavailable during recovery")?;
            if session!=self.session && !claude_job_input(home,id,session,&self.root,&self.input,&mut budget)? {continue;}
            if matched.is_some() {return Err("Claude original input matches multiple native candidates; inspect them before release".into());}
            matched=Some((id.to_owned(),session.to_owned()));
        }
        let missing=||format!("{}Claude original input not found; submission remains unknown; retry native observation",self.launch_error.as_ref().map(|e|format!("{e}; ")).unwrap_or_default());
        let (id,session)=matched.ok_or_else(missing)?;
        if !self.session.is_empty() && session!=self.session {return Err(format!("Claude original input found in copied job {id}; conversation identity differs; inspect native state"));}
        let before=if session==self.session {self.before} else {0};
        if !self.history.read(&claude_transcript_in(home,&session)?,before,&self.input,&self.log)? {
            return Err(missing());
        }
        self.id=id;
        self.session=session;
        self.accepted=true;
        Ok(())
    }

    pub fn start(env: &Env, l: &Launch, log: &Path, text: &str, access: &crate::policy::Effective) -> Result<Self, StartError> {
        let permission_mode = access.claude.as_deref().unwrap_or(env.permission_mode);
        let root = env.root.canonicalize().map_err(|e| e.to_string())?;
        let mut expected_id = None;
        let mut before = 0;
        if l.resume.is_some() {
            let rows = claude_agents_in(&root, crate::runtime::accounts::log_home("claude", log).as_deref())?;
            let row = rows.iter().find(|v| v["sessionId"] == l.session).ok_or("Claude background session unavailable; no replacement")?;
            // The caller releases the previous execution before continuation; reject contradictory native liveness.
            if claude_pid(row)?.is_some() { return Err("Claude session is still running; no copy started".into()); }
            expected_id = Some(row["id"].as_str().ok_or("Claude background ID missing")?.to_string());
            let path = claude_transcript(&l.session).ok_or("Claude saved permissions unavailable")?;
            let mut mode=None;
            before=claude_history(&path,0,|v|{if let Some(m)=v["permissionMode"].as_str() {mode=Some(m.to_owned());}})?;
            let mode=mode.ok_or("Claude saved permission mode unknown")?;
            if mode != permission_mode { return Err("Claude saved permission mode differs; a new attempt is required".into()); }
        }
        std::fs::create_dir_all(log.parent().ok_or("missing log directory")?).map_err(|e| e.to_string())?;
        let mut c = executable(which("claude"), access, env)?;
        let (input, input_file, path) = claude_input(&root, text, Some(l))?;
        bind_input(log,l,&path,&input,serde_json::json!({"session":l.session,"job":expected_id,"before":before,"permission_mode":permission_mode}))?;
        let args = ["--settings", &compact_settings(&root, env.install, l, env.jobs, build_cache_for(&l.rt, access, env).as_deref()), "--name", &session_name(l), "--remote-control", &session_name(l), "--permission-mode", permission_mode];
        let mut spec = optimized(spec(env, l, root.clone(), args.map(Into::into).to_vec()))?;
        if l.role == "operator" {
            for k in ["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE"] { c.env_remove(k); }
            spec.env.retain(|(k, _)| !["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE"].iter().any(|r| k == r));
        }
        let mut turn=Self { root, log: log.into(), session: if l.resume.is_some() { l.session.clone() } else { String::new() }, id: expected_id.unwrap_or_default(),
            announced: false, accepted:false, history:ClaudeHistory::default(), before, mode: permission_mode.into(), input, _input_file: input_file, pid: 0, native:None, recovery:None, done: false, failure: None, launch_error:None };
        let out=agentq_runtime::agent::background_launch(background_base(c, &spec), &spec, &turn.input, std::time::Duration::from_secs(30))?;
        turn.finish_launch(out)?;
        Ok(turn)
    }

    fn finish_launch(&mut self,out:Result<std::process::Output,coreq::process::Failure>)->Result<(),StartError> {
        let out=match out {
            Ok(out)|Err(coreq::process::Failure::Exit(out))=>out,
            Err(e)=>{
                use coreq::process::Failure;
                if matches!(e,Failure::InvalidDeadline|Failure::Capacity|Failure::Owner(_)|Failure::Spawn(_)|Failure::InputLimit) {
                    let class=match &e {Failure::Spawn(error)=>crate::profile::io_class(error),Failure::InvalidDeadline|Failure::InputLimit=>"invalid",_=>"other"};
                    return Err(StartError {message:format!("Claude background launch: {e}"),class,reportable:false,no_start:true});
                }
                self.launch_error=Some(format!("Claude background launch outcome unknown: {e}; reconcile the original input"));
                return Ok(());
            }
        };
        let stdout=String::from_utf8_lossy(&out.stdout);
        let stderr=String::from_utf8_lossy(&out.stderr);
        let returned=claude_returned_id(&stdout);
        let status=out.status.code();
        if let Some(refusal)=claude_refusal(returned.is_some(),&stderr,Some(status)) {
            // The launch observation is evidence for cold recovery; a refused launch submitted nothing (#1557).
            let record=serde_json::json!({"type":"claude.launch","stdout":stdout,"stderr":stderr,"status":status});
            if let Err(e)=File::options().append(true).open(&self.log).and_then(|mut file|writeln!(file,"{record}")) {log!("Claude refused launch not recorded: {e}");}
            // #1555: the board text names the root and its remedy, never its path.
            if stderr.contains("Workspace not trusted") {
                return Err(StartError {message:format!("access refused: Claude does not trust this project root; {}",owner::trust_step("the project root")),class:"permission",reportable:false,no_start:true});
            }
            return Err(StartError {message:format!("Claude background launch refused: {}",public_line(&refusal,&self.root,200)),class:"other",reportable:false,no_start:true});
        }
        if let Some(id)=returned {
            if !self.id.is_empty() && self.id!=id {self.launch_error=Some("Claude returned a different background job; reconcile the original input and session".into());}
            self.id=id.into();
        } else {self.launch_error=Some("Claude returned no valid background ID; launch outcome unknown; reconcile the original input".into());}
        if !out.status.success() {
            let uncertainty=self.launch_error.as_deref().unwrap_or("submission outcome unknown");
            self.launch_error=Some(format!("Claude launch response failed: {stderr}; {uncertainty}"));
        }
        let recorded=File::options().append(true).open(&self.log).and_then(|mut file|
            writeln!(file,"{}",serde_json::json!({"type":"claude.launch","stdout":stdout,"stderr":stderr,"status":status})));
        if let Err(e)=recorded {self.launch_error.get_or_insert_with(||format!("Claude launch observation not recorded: {e}; native handle retained"));}
        Ok(())
    }
}

impl Proc for ClaudeTurn {
    fn checkpoint(&mut self,cfg:&crate::service::Config,l:&Launch,execution:u64)->Result<(),String> {
        self.recovery=Some(Recovery {install:cfg.install.clone(),root:cfg.root.clone(),task:l.task,attempt:l.attempt,execution,runtime:l.rt.clone()});
        let rows=self.agents()?;
        let home=self.native_home();
        self.recover_job(&rows,home.as_deref())?;
        let row=rows.iter().find(|v|v["id"]==self.id).ok_or("Claude checkpoint job unavailable; preserve original input")?;
        self.capture_identity(row)?;
        self.persist()
    }
    fn native_identity(&self)->Result<coreq::process::Identity,String> {self.native.clone().ok_or("Claude original process identity unavailable".into())}
    fn input_offset(&self)->Result<u64,String> {Ok(0)}
    fn accepted(&mut self) -> Result<bool, String> {
        if self.accepted {Ok(true)} else if let Some(e)=&self.launch_error {Err(e.clone())} else {Ok(false)}
    }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        if self.done { return Ok(Some(if self.failure.is_some() {-1} else {0})); }
        let rows = self.agents()?;
        let home=self.native_home();
        self.recover_job(&rows,home.as_deref())?;
        let row = rows.iter().find(|v| v["id"] == self.id).ok_or("Claude session unavailable; state unknown")?;
        let observed=self.observe(row);
        if self.accepted {self.launch_error=None;}
        if matches!(observed,Ok(None)) && self.launch_error.is_some() {return Err(self.launch_error.clone().unwrap());}
        observed
    }
    fn exit_error(&self) -> Option<&'static str> { self.failure }
    fn kill(&mut self) -> Result<(), String> {
        let rows=self.agents()?;
        let home=self.native_home();
        self.recover_job(&rows,home.as_deref())?;
        let row=rows.iter().find(|v|v["id"]==self.id).ok_or("Claude stop identity unavailable")?;
        self.capture_identity(row)?;
        if let Err(e)=self.persist() {log!("Claude recovery checkpoint failed before stop: {e}");}
        claude_stop(&self.root, &self.id, self.native.as_ref(), self.account().as_deref())?;
        self.done = true;
        Ok(())
    }
    fn id(&self) -> u32 { self.native.as_ref().map_or(self.pid,|n|n.pid) }
}

impl ClaudeTurn {
    fn complete(&mut self)->Result<Option<i32>,String> {
        if let Some((class,_))=&self.history.failure {self.failure=Some(*class);self.done=true;return Ok(Some(-1));}
        let answer=self.history.answer.as_deref().ok_or("Claude completed without final text; inspect native logs")?;
        let mut log=File::options().append(true).open(&self.log).map_err(|e|e.to_string())?;
        writeln!(log,"{}",serde_json::json!({"type":"result","session_id":self.session,"result":answer})).map_err(|e|e.to_string())?;
        self.done=true;Ok(Some(0))
    }
    fn capture_identity(&mut self,row:&serde_json::Value)->Result<(),String> {
        if row["id"]!=self.id || !self.session.is_empty() && row["sessionId"]!=self.session {
            return Err("Claude process locator differs from the original native job".into());
        }
        if let Some(pid)=claude_pid(row)? {
            if let Some(old)=self.native.as_ref().filter(|n|n.pid!=pid) {
                // #1638: a restart can leave the same job and conversation on a new process. Only an exited recorded
                // process and an accepted readback of this job's own input let the new process stand in for it.
                let gone=coreq::process::identity(old.pid).map_err(|e|format!("Claude process identity unreadable: {e}"))?.as_ref()!=Some(old);
                if !gone || !self.accepted || self.session.is_empty() {return Err("Claude native process changed; reconcile original execution".into());}
                self.native=None;
            }
            let current=coreq::process::identity(pid).map_err(|e|format!("Claude process identity unreadable: {e}"))?.ok_or("Claude reported process absent; retry native readback")?;
            if self.native.as_ref().is_some_and(|n|*n!=current) {return Err("Claude PID incarnation changed; original identity retained".into());}
            // #1561: a process another task's record holds is never this job's identity; the next readback retries.
            if let (None,Some(r))=(&self.native,&self.recovery) {
                if let Some(other)=crate::kids_file(&r.install,&r.root)?.iter().find(|k|k.task!=r.task&&k.process==current) {
                    return Err(format!("Claude reported the process recorded for #{}; retry native readback",other.task));
                }
            }
            self.native=Some(current);
            self.pid=pid;
        }
        Ok(())
    }
    fn persist(&mut self)->Result<(),String> {
        let Some(r)=&self.recovery else {return Ok(())};
        let row=crate::data::KidLine {task:r.task,attempt:r.attempt,runtime:"claude".into(),process:self.native_identity()?,execution:r.execution,offset:0,job:None};
        crate::checkpoint_kid(&r.install,&r.root,row).map_err(|e|format!("Claude native recovery checkpoint failed: {e}"))?;
        self.recovery=None;
        Ok(())
    }
    fn observe(&mut self, row: &serde_json::Value) -> Result<Option<i32>, String> {
        // Job outcome and process exit are separate native observations.
        let state = row["state"].as_str().ok_or("Claude job state unavailable; state unknown")?;
        if !matches!(state,"working"|"idle"|"done"|"blocked"|"failed"|"crashed"|"stopped") {
            return Err(format!("Claude state {state:?} unsupported; inspect native lifecycle before release"));
        }
        self.capture_identity(row)?;
        self.persist()?;
        if !matches!(state, "working" | "idle" | "done" | "blocked") {
            if !self.accepted && (self.launch_error.is_some() || self.before>0) {
                return Err(format!("Claude job {state}; current input acceptance unconfirmed, reconcile the original submission"));
            }
            if !claude_stopped(row, self.native.as_ref())? { return Err(format!("Claude job {state}; native process exit unconfirmed; inspect the original job")); }
            if self.history.ended {return self.complete();}
            self.failure = Some(claude_failure(row, &self.root, &self.id, self.native_home().as_deref()));
            return Ok(Some(-1));
        }
        // On Windows a waking session briefly publishes a different UUID before restore.
        if !self.announced {
            let session = row["sessionId"].as_str().ok_or("Claude session ID unknown")?;
            if !self.session.is_empty() && self.session != session { return Err("Claude resumed another conversation; identity unknown".into()); }
            self.session = session.into();
            let mut file = File::options().append(true).open(&self.log).map_err(|e| e.to_string())?;
            writeln!(file, "{}", serde_json::json!({"type":"thread.started", "thread_id":session, "session_id":session, "runtime":"claude-bg", "permission_mode":self.mode})).map_err(|e| e.to_string())?;
            self.announced = true;
        }
        self.pid = claude_pid(row)?.unwrap_or(self.pid);
        let path = claude_transcript(&self.session).ok_or("Claude transcript unavailable; completion unknown")?;
        if !self.history.read(&path,self.before,&self.input,&self.log)? {return Ok(None);}
        self.accepted=true;
        if row["status"]!="idle" && row["state"]!="done" && row["state"]!="blocked" {return Ok(None);}
        if !self.history.ended {return Ok(None);}
        claude_stop(&self.root, &self.id, self.native.as_ref(), self.account().as_deref())?;
        self.complete()
    }
}

/// The only session-link builder and checks (§ 9 runtime adapters). A Codex thread id reaches the app URL and, on
/// Windows, a shell: only a plain id.
pub fn plain_thread(id: &str) -> bool { !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') }

const THREADS: &str = "codex://threads/";

pub fn thread_url(id: &str) -> String { format!("{THREADS}{id}") }

/// The thread id of a Codex app link.
pub fn thread_of(url: &str) -> Option<&str> { url.strip_prefix(THREADS) }

/// The native id of a board session `<rt>:<id>[ <host>]` whose runtime has Remote Control links.
pub fn remote_session(session: &str) -> Option<&str> {
    let (rt, id) = session.split(' ').next()?.split_once(':')?;
    of(rt).remote.then_some(id)
}

/// A canonical Claude Remote Control URL.
pub fn remote_url(u: &str) -> bool {
    u.strip_prefix("https://claude.ai/code/session_").is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// A session link this host may open (A2): a canonical Remote Control URL or a plain Codex app thread; a thread id
/// reaches a shell on Windows, so only a plain one.
pub fn link_ok(u: &str) -> bool { remote_url(u) || thread_of(u).is_some_and(plain_thread) }

/// The one session link resolver (A2, native adapter M3): board session `<rt>:<id>` of a task attempt. On its own host
/// the attempt log resolves the id (Codex `-`, Claude's pre-launch alias); a Codex thread links only there; a Claude
/// URL is account-scoped, so the owning host's transcript, else `published` (the owner's heartbeat). Returns the
/// resolved id and the link, None when unknown.
pub fn session_link(root: &Path, own: bool, rt: &str, id: &str, task: u64, attempt: u64, role: &str, published: Option<String>) -> (String, Option<String>) {
    let a = of(rt);
    let id = if own {session_id(root,rt,id,task,attempt,role)} else {id.into()};
    let url = if id == "-" || !plain_thread(&id) { None }
        else if a.codex { own.then(|| thread_url(&id)) }
        else if a.remote { own.then(|| claude_remote(&id)).flatten().or(published.filter(|u| remote_url(u))) }
        else { None };
    (id, url)
}

/// Native identity from this attempt's log, without scanning the transcript for a Remote Control URL.
pub fn session_id(root:&Path,rt:&str,id:&str,task:u64,attempt:u64,role:&str)->String {
    if id=="-" || of(rt).remote {
        let l=Launch {files:String::new(),execution:None,task,title:String::new(),attempt,role:role.into(),rt:rt.into(),session:id.into(),url:String::new(),resume:None};
        native_session(rt,&log_path(root,&l)).unwrap_or_else(||id.into())
    } else {id.into()}
}

/// The one opener (A2): a session link, web page or folder in this host's default app. `wait` bounds the opener's
/// exit for callers that act on the open (Codex app queue); the tray only spawns it.
pub fn open_link(arg: &str, wait: bool) -> Result<(), String> {
    let mut c = crate::platform::command(if cfg!(windows) { "explorer" } else { "open" });
    crate::platform::quiet(c.arg(arg));
    if !wait { return c.spawn().map(|_| ()).map_err(|e| e.to_string()); }
    match coreq::process::output("runtime.open-link", &mut c, std::time::Duration::from_secs(10)) {
        Err(coreq::process::Failure::Exit(out)) if !opened(cfg!(windows), out.status.code()) => Err(format!("open exited {:?}", out.status.code())),
        Err(coreq::process::Failure::Exit(_)) | Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Windows `explorer` exits 1 after it opened the link; only `open`'s 0 is success elsewhere.
fn opened(windows: bool, code: Option<i32>) -> bool {
    code == Some(0) || windows && code == Some(1)
}

/// Only a URL registered by the native session is a Remote Control link.
pub fn claude_remote(session: &str) -> Option<String> {
    let path=claude_transcript(session)?;let mut reader=NativeHistory::default();let mut found=None;
    let until=observation_deadline(std::time::Duration::from_secs(5));
    loop {
        let complete=reader.read(&path,|v| {
            if let Some(url)=v["url"].as_str() {
                if v["subtype"]=="bridge_status" && remote_url(url) {found=Some(url.to_string());}
            }
            Ok(())
        }).ok()?;
        if complete {return found;}
        if std::time::Instant::now()>=until {return None;}
    }
}

pub fn claude_background(root: &Path, session: &str, home: Option<&Path>) -> Result<bool, String> {
    if claude_agents_in(root, home)?.iter().any(|v| v["sessionId"] == session) { return Ok(true); }
    // A missing native job must not silently resume a background conversation in print mode.
    Ok(claude_transcript(session).and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|text|
        text.lines().filter_map(|s| serde_json::from_str::<serde_json::Value>(s).ok()).any(|v| v["sessionKind"] == "bg")))
}

/// #1825 prototype: a task worker's native transcript on this host and whether it is in Codex's format. The homes are
/// the service's own (`claude_home`, `codex_home`), which a Windows probe of both runtimes confirmed (#1825 A2).
pub fn worker_transcript(rt: &str, session: &str) -> Result<(bool, PathBuf), String> {
    let a = of(rt);
    if a.name != rt || !a.codex && !a.remote { return Err(format!("runtime {rt:?}: native transcript adapter unqualified")); }
    // § 3 Runtime accounts: the default home first, then each account home; the first error stands when none has it
    let rt = if a.codex { "codex" } else { "claude" };
    let homes = crate::runtime::accounts::homes(rt);
    let mut first = None;
    for home in &homes {
        match if a.codex { owner::codex_transcript_in(home, session) } else { claude_transcript_in(home, session) } {
            Ok(p) => return Ok((a.codex, p)),
            Err(e) => { first.get_or_insert(e); }
        }
    }
    Err(first.unwrap_or_else(|| format!("transcript unknown: no {} home", if a.codex { "Codex" } else { "Claude" })))
}

pub(crate) const HANDSHAKE: &str = "taskq handshake: reply OK and do nothing else. The task follows in this thread.\n";

/// A `codex-app` attempt stays owned until its exact native input and turn completion are observed.
pub struct AppTurn {
    root: PathBuf,
    history: AppHistory,
    hand: Option<Box<dyn Proc>>,
    log: PathBuf,
    /// The brief, or the resume text, to queue.
    text: String,
    _input: Option<crate::data::InputLock>,
    binding: Option<serde_json::Value>,
    thread: String,
    /// `task_complete` events of the thread before the queued text.
    done: usize,
    rollout: Option<PathBuf>,
    running: fn() -> Result<bool, String>,
    control: fn(&str,&str)->Result<(),String>,
    cancelled: bool,
    queue_error: Option<String>,
    accepted: bool,
}

impl AppTurn {
    fn queue(&mut self) -> Result<(), String> {
        // the id goes through the Windows shell: only a plain thread id
        if !plain_thread(&self.thread) {
            return Err(format!("invalid thread id {:?}", self.thread));
        }
        self.rollout = rollout(&self.thread);
        self.done = completes(self.rollout.as_ref().ok_or("Codex rollout unavailable; turn baseline unknown")?)?;
        if let Some(binding)=&mut self.binding {
            binding["native"]=serde_json::json!({"thread":self.thread,"baseline":self.done});
            record_input(&self.log,binding)?;
        }
        app_submit(&self.thread, &self.text)
    }
}

/// Show `thread` in the Codex app and queue `text` on it (§ 8 Codex app); the caller has checked the id.
pub(crate) fn app_submit(thread: &str, text: &str) -> Result<(), String> {
    open_link(&thread_url(thread), true).map_err(|e| format!("codex queue failed for thread {thread}: {e}"))?;
    app_queue(thread,text)
}

/// An already owned App thread accepts serialized input without raising another native window.
pub(crate) fn app_queue(thread:&str,text:&str)->Result<(),String> {
    if !plain_thread(thread) {return Err("native App thread identity invalid".into());}
    // #1856: the error names the queue call's exit and stderr; the brief itself stays out of it
    // ponytail: the queue call has no project; any existing directory serves as its working directory
    let spec = agentq_runtime::agent::Spec { client: agentq_runtime::agent::Client::Codex, root: std::env::temp_dir(), session: thread.into(), resume: true, args: vec![], home: None, env: vec![] };
    agentq_runtime::agent::app_queue(codex_direct(), &optimized(spec)?, text, std::time::Duration::from_secs(10)).map(drop).map_err(|e| format!("codex queue failed for thread {thread}: {e}"))
}

/// Codex without a `cmd` in between: an npm `codex.cmd` shim runs its `codex.js` through `node`, since `cmd` caps a
/// line at 8191 characters and the brief is longer (§ 8).
fn codex_direct() -> Command {
    let bin = which("codex");
    let js = bin.parent().map(|d| d.join("node_modules/@openai/codex/bin/codex.js"));
    match js.filter(|js| bin.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) && js.is_file()) {
        Some(js) => {
            let node = bin.with_file_name("node.exe");
            let mut c = crate::platform::command(if node.is_file() { node } else { which("node") });
            c.arg(js);
            c
        }
        None => crate::platform::command(bin),
    }
}

impl Proc for AppTurn {
    fn accepted(&mut self) -> Result<bool, String> { Ok(self.accepted) }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        if let Some(h) = &mut self.hand {
            let Some(code) = h.exited()? else { return Ok(None) };
            self.hand = None;
            if self.cancelled { return Ok(Some(code)); }
            self.thread = codex_thread(&self.log).unwrap_or_default();
            if self.thread.is_empty() { return Ok(Some(if code == 0 { -1 } else { code })); }
            if let Err(e) = self.queue() {
                self.queue_error = Some(format!("{e}; submission outcome unknown"));
                return Err(self.queue_error.clone().unwrap());
            }
            return Ok(None);
        }
        if !(self.running)()? { return Err("Codex app unavailable; submitted turn termination unconfirmed".into()); }
        if self.rollout.is_none() {
            self.rollout = rollout(&self.thread);
        }
        let (accepted, ended) = self.history.read(self.rollout.as_ref().ok_or("Codex rollout unavailable; completion unknown")?, self.done, &self.text, &self.root, &self.thread)?;
        self.accepted |= accepted;
        if self.accepted {self.queue_error=None;}
        if ended { return Ok(Some(if self.history.aborted {-1} else {0})); }
        if let Some(e) = &self.queue_error { return Err(e.clone()); }
        Ok(None)
    }
    fn kill(&mut self) -> Result<(), String> {
        if let Some(h) = &mut self.hand {
            self.cancelled = true;
            return h.kill();
        }
        if self.exited()?.is_some() { return Ok(()); }
        let turn=self.history.owned.as_deref().ok_or("Codex App input acceptance unknown; cancellation cannot target a native turn")?;
        (self.control)(&self.thread,turn)
    }
    fn id(&self) -> u32 {
        self.hand.as_ref().map(|h| h.id()).unwrap_or(0)
    }
}

/// The Codex app runs in this login session: macOS `lsappinfo`, Windows a `tasklist /APPS` row of its package.
fn app_running() -> Result<bool, String> {
    coreq::telemetry::capture::operation("runtime.app-readback", || {
    let mut c = if cfg!(windows) { crate::platform::command("tasklist") } else { crate::platform::command("lsappinfo") };
    if cfg!(windows) {
        c.args(["/APPS", "/FO", "CSV", "/NH"]);
    } else {
        c.args(["find", "bundleID=com.openai.codex"]);
    }
    let out = coreq::process::output("runtime.app-presence", &mut c, observation_left(std::time::Duration::from_secs(10))).map_err(|e| format!("Codex app presence unknown: {e}"))?;
    let text = std::str::from_utf8(&out.stdout).map_err(|_| "Codex app presence returned invalid UTF-8")?;
    app_presence(text, cfg!(windows))
    }, |_| "readback")
}

fn app_presence(text: &str, windows: bool) -> Result<bool, String> {
    if text.trim().is_empty() { return Ok(false); }
    if windows {
        let mut found = false;
        for line in text.lines() {
            let row = line.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or("Codex app presence returned an invalid process list")?;
            let columns: Vec<_> = row.split("\",\"").collect();
            if columns.len() < 3 || columns[1].parse::<u32>().ok().filter(|n| *n > 0).is_none()
                || columns.iter().any(|s| s.replace("\"\"", "").contains('"')) {
                return Err("Codex app presence returned an invalid process list".into());
            }
            found |= columns.iter().any(|s| s.starts_with("OpenAI.Codex_"));
        }
        Ok(found)
    } else {
        let valid = |line: &str| {
            let Some((asn, title)) = line.strip_prefix("ASN:0x").and_then(|s| s.split_once("-\"")) else { return false };
            let Some((high, low)) = asn.split_once("-0x") else { return false };
            u32::from_str_radix(high, 16).is_ok() && u32::from_str_radix(low, 16).is_ok() && title.ends_with("\":")
        };
        if !text.lines().all(valid) {
            return Err("Codex app presence returned an invalid application list".into());
        }
        Ok(true)
    }
}

/// `$CODEX_HOME`, default `~/.codex`; a Windows service has no `HOME`.
pub(crate) fn codex_home() -> Option<PathBuf> { agentq_runtime::agent::base_home(agentq_runtime::agent::Client::Codex) }

/// The thread's rollout file in the Codex home or a § 3 runtime account's home.
pub(crate) fn rollout(thread: &str) -> Option<PathBuf> {
    crate::runtime::accounts::homes("codex").iter().find_map(|home| agentq_runtime::agent::rollout(thread, Some(home)))
}

/// Volatile cursor over the native append-only history; restart rebuilds it without replaying work.
#[derive(Default)]
struct NativeHistory {
    path: Option<PathBuf>,
    metadata: Option<std::fs::Metadata>,
    #[cfg(windows)]
    identity: Option<windows::Win32::Storage::FileSystem::FILE_ID_INFO>,
    offset: u64,
    boundary: Vec<u8>,
    /// SHA-256 of bytes `..offset`: an mtime-only touch (same-length `ftruncate`) keeps the read prefix.
    prefix: Option<ring::digest::Context>,
}

impl NativeHistory {
    fn read(&mut self,path:&Path,mut visit:impl FnMut(serde_json::Value)->Result<(),String>)->Result<bool,String> {
        self.read_at(path,|event,_|visit(event))
    }
    fn read_at(&mut self,path:&Path,mut visit:impl FnMut(serde_json::Value,u64)->Result<(),String>)->Result<bool,String> {
        let mut at=self.offset;
        self.lines(path,|line| {
            let event:serde_json::Value=serde_json::from_str(line).map_err(|_|"native history record invalid")?;
            if event["type"].as_str().is_none() {return Err("native history record shape invalid".into());}
            visit(event,at)?;at+=line.len() as u64;Ok(())
        })
    }
    fn lines(&mut self,path:&Path,mut visit:impl FnMut(&str)->Result<(),String>)->Result<bool,String> {
        use std::io::{Seek,SeekFrom};
        const LIMIT:u64=8*1024*1024;
        let mut file=File::open(path).map_err(|e|format!("native history unavailable: {e}"))?;
        let meta=file.metadata().map_err(|e|e.to_string())?;
        if !meta.is_file() {return Err("native history is not a regular file".into());}
        #[cfg(windows)]
        let identity={
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{Foundation::HANDLE,Storage::FileSystem::{FILE_ID_INFO,FileIdInfo,GetFileInformationByHandleEx}};
            let mut id=FILE_ID_INFO::default();
            unsafe {GetFileInformationByHandleEx(HANDLE(file.as_raw_handle()),FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),std::mem::size_of::<FILE_ID_INFO>() as u32)}
                .map_err(|e|format!("native history identity unavailable: {e}"))?;
            if self.identity.is_some_and(|old|old!=id) {return Err("native history file replaced; reconcile original input".into());}
            id
        };
        if let Some(old)=&self.metadata {
            #[cfg(unix)]
            {use std::os::unix::fs::MetadataExt;
                if (old.dev(),old.ino())!=(meta.dev(),meta.ino()) {return Err("native history file replaced; reconcile original input".into());}
            }
            // #1627: Claude moves its transcript into a worktree's project folder; the file identity above survives the rename.
            let moved=cfg!(not(any(unix,windows))) && self.path.as_deref()!=Some(path);
            if moved || old.created().ok()!=meta.created().ok() {
                return Err("native history identity changed; reconcile original input".into());
            }
            if meta.len()<self.offset || meta.len()<old.len() {return Err("native history truncated; completion unknown".into());}
            if meta.len()==old.len() && meta.modified().ok()!=old.modified().ok() {
                let mut hash=ring::digest::Context::new(&ring::digest::SHA256);
                let mut rest=(&mut file).take(self.offset);let mut buf=[0u8;65536];
                loop {let n=rest.read(&mut buf).map_err(|e|format!("native history read failed: {e}"))?;if n==0 {break;}hash.update(&buf[..n]);}
                let old=self.prefix.clone().unwrap_or_else(||ring::digest::Context::new(&ring::digest::SHA256)).finish();
                let same=old.as_ref()==hash.finish().as_ref();
                if !same {return Err("native history rewritten; completion unknown".into());}
            }
        }
        if !self.boundary.is_empty() {
            file.seek(SeekFrom::Start(self.offset-self.boundary.len() as u64)).map_err(|e|e.to_string())?;
            let mut bytes=vec![0;self.boundary.len()];file.read_exact(&mut bytes).map_err(|e|e.to_string())?;
            if bytes!=self.boundary {return Err("native history boundary changed; completion unknown".into());}
        }
        self.path=Some(path.into());self.metadata=Some(meta);
        #[cfg(windows)]
        {self.identity=Some(identity);}
        file.seek(SeekFrom::Start(self.offset)).map_err(|e|e.to_string())?;
        let mut reader=BufReader::new(file.take(LIMIT+1));
        let (mut line,mut consumed)=(String::new(),0u64);
        loop {
            if OBSERVATION_UNTIL.get().is_some_and(|until|std::time::Instant::now()>=until) {return Ok(false);}
            line.clear();
            let size=reader.read_line(&mut line).map_err(|e|format!("native history read failed: {e}"))? as u64;
            if size==0 {break;}
            if size>LIMIT {return Err("native history record exceeds 8 MiB; completion unknown".into());}
            if consumed+size>LIMIT {return Ok(false);}
            if !line.ends_with('\n') {return Err("native history record incomplete; retry observation".into());}
            visit(&line)?;
            self.offset+=size;consumed+=size;
            self.prefix.get_or_insert_with(||ring::digest::Context::new(&ring::digest::SHA256)).update(line.as_bytes());
            self.boundary=line.as_bytes()[line.len().saturating_sub(64)..].to_vec();
        }
        let length=reader.get_ref().get_ref().metadata().map_err(|e|e.to_string())?.len();
        if length<self.offset {return Err("native history truncated while reading".into());}
        Ok(length==self.offset)
    }
}

#[derive(Default)]
struct AppHistory {
    reader: NativeHistory,
    floor:u64,
    count: usize,
    current: Option<String>,
    owned: Option<String>,
    ended: bool,
    aborted: bool,
    identity: bool,
}

/// Match this submission and its native turn id; another user turn cannot release its fence.
impl AppHistory {
    fn read(&mut self,path:&Path,baseline:usize,input:&str,root:&Path,thread:&str)->Result<(bool,bool),String> {
        let Self {reader,floor,count,current,owned,ended,aborted,identity}=self;
        let complete=reader.read_at(path,|v,at| {
            if v["type"] == "event_msg" && !v["payload"].is_object() {return Err("Codex rollout record shape invalid".into());}
            if v["type"] == "session_meta" {
                let meta=&v["payload"];
                if *identity || meta["id"]!=thread || meta["cwd"].as_str().and_then(|p|Path::new(p).canonicalize().ok()).as_deref()!=Some(root)
                    || !meta["forked_from_id"].is_null() {
                    return Err("Codex native session identity differs or is copied; inspect the original submission".into());
                }
                *identity=true;
            }
            if v["type"] != "event_msg" {return Ok(());}
            if !*identity {return Err("Codex native session identity missing before turn events".into());}
            let p=&v["payload"];
            if *count>=baseline {
                match p["type"].as_str() {
                    Some("task_started")=>*current=p["turn_id"].as_str().map(String::from),
                    Some(kind) if at>=*floor && ((kind=="user_message" && p["message"].as_str()==Some(input))
                        || (kind=="item_completed" && p["item"]["type"]=="UserMessage"
                            && p["item"]["content"].as_array().is_some_and(|parts|parts.len()==1 && parts[0]["type"]=="text" && parts[0]["text"].as_str().is_some_and(|text|text==input || text.strip_suffix('\n')==Some(input)))))=>{
                        let id=current.clone().ok_or("Codex submitted input has no native turn identity")?;
                        if kind=="item_completed" && (p["thread_id"]!=thread || p["turn_id"].as_str()!=Some(id.as_str())) {
                            return Err("Codex submitted user item has conflicting native thread/turn identity".into());
                        }
                        if p["turn_id"].as_str().is_some_and(|turn|turn!=id) {return Err("Codex submitted input has conflicting native turn identity".into());}
                        if owned.is_some() {return Err("Codex submitted input matches multiple native messages".into());}
                        *owned=Some(id);
                    },
                    Some(kind @ ("task_complete"|"turn_aborted")) if owned.is_some() && p["turn_id"].as_str()==owned.as_deref()=>{
                        let cancelled=kind=="turn_aborted";
                        if *ended && *aborted!=cancelled {return Err("Codex native terminal outcomes conflict; reconcile original turn".into());}
                        *ended=true;*aborted=cancelled;
                    },
                    _=>{},
                }
            }
            if p["type"]=="task_complete" {*count+=1;*current=None;}
            if p["type"]=="turn_aborted" && p["turn_id"].as_str()==current.as_deref() {*current=None;}
            Ok(())
        })?;
        if !complete {return Err("Codex native history scan pending; retry observation without queueing".into());}
        if !*identity {return Err("Codex native session identity unavailable".into());}
        if *count<baseline {return Err("Codex rollout truncated; completion unknown".into());}
        Ok((owned.is_some(),*ended))
    }
}

/// Finished turns in a rollout: its `task_complete` events.
fn completes(rollout: &Path) -> Result<usize, String> {
    coreq::telemetry::capture::operation("runtime.turn-readback", || {
        let mut reader=NativeHistory::default();let mut count=0;
        let until=observation_deadline(std::time::Duration::from_secs(5));
        loop {
            let complete=reader.read(rollout,|v| {
                if v["type"]=="event_msg" && !v["payload"].is_object() {return Err("Codex rollout record shape invalid".into());}
                count+=usize::from(v["type"]=="event_msg" && v["payload"]["type"]=="task_complete");
                Ok(())
            })?;
            if complete {return Ok(count);}
            if std::time::Instant::now()>=until {return Err("Codex baseline scan exceeded 5 seconds; no input queued".into());}
        }
    }, |_| "readback")
}

// ---------- `taskq agents` (taskq.md § 5): read-only probes, unknown when unreadable (R12) ----------

/// Exit success and bounded stdout + stderr; incomplete execution/output remains unknown.
pub(crate) fn probe(name: &'static str, bin: &Path, args: &[&str]) -> Option<(bool, String)> {
    probe_within(name, bin, args, std::time::Duration::from_secs(10))
}

fn probe_within(name: &'static str, bin: &Path, args: &[&str], budget: std::time::Duration) -> Option<(bool, String)> {
    probe_env(name, bin, args, None, budget)
}

/// A probe in a § 3 runtime account's home: `(variable, home)` names it to the native CLI.
pub(crate) fn probe_in(name: &'static str, bin: &Path, args: &[&str], home: Option<(&str, &Path)>) -> Option<(bool, String)> {
    probe_env(name, bin, args, home, std::time::Duration::from_secs(10))
}

fn probe_env(name: &'static str, bin: &Path, args: &[&str], home: Option<(&str, &Path)>, budget: std::time::Duration) -> Option<(bool, String)> {
    let budget=observation_left(budget);
    if budget.is_zero() {return None;}
    let mut c = crate::platform::command(bin);
    c.args(args);
    if let Some((var, home)) = home { c.env(var, home); }
    let out = match coreq::process::output(name, &mut c, budget) {
        Ok(out) | Err(coreq::process::Failure::Exit(out)) => out,
        // #1856: the row reads unknown; the log keeps the command and why
        Err(e) => { crate::platform::log(&format!("{name}: `{} {}` failed: {e}", bin.display(), args.join(" "))); return None }
    };
    Some((out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))
}

/// The first word of `--version` output that starts with a digit: `2.1.3 (Claude Code)`, `codex-cli 0.15.3`.
fn version(out: &str) -> Option<String> {
    out.split_whitespace().find(|w| w.starts_with(|c: char| c.is_ascii_digit())).map(String::from)
}

/// The login email of a § 3 runtime account on this host; None when the native status names none (R12: unknown).
pub(crate) fn account_email(rt: &str, label: &str) -> Option<String> {
    let home = crate::runtime::accounts::home(rt, label);
    if label != crate::runtime::accounts::DEFAULT && !home.as_deref().is_some_and(Path::is_dir) { return None; }
    let bin = which(of(rt).bin);
    let args: &[&str] = if rt == "claude" { &["auth", "status"] } else { &["login", "status"] };
    let (_, out) = probe_in("runtime.account", &bin, args, home.as_deref().map(|h| (crate::runtime::accounts::var(rt), h)))?;
    if rt == "claude" {
        let v: serde_json::Value = serde_json::from_str(&out).ok()?;
        return v["email"].as_str().filter(|e| e.contains('@')).map(String::from);
    }
    // Codex status text: an email only when it prints one; never other words of the line
    out.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '@' && c != '.' && c != '-' && c != '_' && c != '+'))
        .find(|w| w.contains('@') && !w.starts_with('@') && !w.ends_with('@')).map(String::from)
}

/// Login state and account from `claude auth status` (JSON) or `codex login status` (text).
fn login(rt: &str, out: Option<(bool, String)>) -> (&'static str, Option<String>) {
    let Some((_, out)) = out else { return ("unknown", None) };
    if rt == "claude" {
        let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
        return match v["loggedIn"].as_bool() {
            Some(true) => ("logged in", v["email"].as_str().or(v["authMethod"].as_str()).map(String::from)),
            Some(false) => ("not logged in", None),
            None => ("unknown", None),
        };
    }
    let l = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if l.starts_with("Not logged in") {
        return ("not logged in", None);
    }
    // `Logged in using ChatGPT`, `Logged in using an API key - sk-…`: the method only, never key text
    match l.strip_prefix("Logged in") {
        Some(rest) => ("logged in", Some(rest.trim_start_matches([' ', ':']).trim_start_matches("using ").split(" - ").next().unwrap_or("").trim().to_string()).filter(|a| !a.is_empty())),
        None => ("unknown", None),
    }
}

/// Login kind and last refresh (taskq.md § 5 `agents`): Claude `authMethod subscriptionType` from `auth status`, no
/// readable refresh time; Codex `auth_mode` and `last_refresh` of `auth.json`. Never a token value; unreadable: None.
fn kind_refresh(rt: &str, status: Option<&str>, auth: Option<&str>) -> (Option<String>, Option<String>) {
    let json = |t: Option<&str>| t.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).unwrap_or_default();
    if rt == "claude" {
        let v = json(status);
        let kind = [&v["authMethod"], &v["subscriptionType"]].iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ");
        return ((!kind.is_empty()).then_some(kind), None);
    }
    let v = json(auth);
    (v["auth_mode"].as_str().map(String::from), v["last_refresh"].as_str().map(String::from))
}

/// The re-login step for a logged-out runtime: the longest-lived option first ([runbook](../docs/relogin.md)).
pub fn relogin(rt: &str) -> &'static str {
    if rt == "antigravity" { "sign in through the Antigravity client" }
    else if rt == "claude" { "run `claude auth login` with the PM (docs/relogin.md)" } else { "run `codex login --device-auth` with the PM (docs/relogin.md)" }
}

/// `Login:` lines of `service status` from status.json `agents` rows; a logged-out runtime adds its `Next:` step.
pub fn login_lines(rows: &serde_json::Value) -> Vec<String> {
    let Some(rows) = rows.as_array() else { return vec!["Login: unknown (no runtime probe yet)".into()] };
    rows.iter().flat_map(|r| {
        let s = |k: &str| r[k].as_str().unwrap_or("unknown").to_string();
        let mut l = vec![format!("Login: {} · kind {} · refreshed {}", agent_text(r), s("kind"), s("refreshed"))];
        if r["login"] == "not logged in" { l.push(format!("Next: {} admits no task; {}", s("runtime"), relogin(&s("runtime")))); }
        l
    }).collect()
}

/// `logged in`, `not logged in` or `unknown` for an installed runtime binary.
pub fn login_state(rt: &str, bin: &Path) -> &'static str {
    if matches!(rt, "antigravity" | "agy") {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty());
        return if home.is_some_and(|h| [".gemini/antigravity-cli/jetski_state.pbtxt", ".gemini/oauth_creds.json"].iter().any(|p| Path::new(&h).join(p).is_file())) { "logged in" } else { "unknown" };
    }
    let home = if rt == "claude" { claude_home() } else { codex_home() }
        .map(|b| crate::runtime::accounts::active_in(&crate::platform::default_install(), rt, &b));
    login(rt, probe_in("runtime.login", bin, if rt == "claude" { &["auth", "status"] } else { &["login", "status"] },
        home.as_deref().map(|h| (crate::runtime::accounts::var(rt), h)))).0
}

/// The only runtime list (§ 9 runtime adapters).
pub const RUNTIMES: [&str; 4] = ["claude", "codex", "codex-app", "antigravity"];
/// §4 fallback order, excluding the explicitly selected app runtime.
pub const CLI_RUNTIMES: [&str; 3] = [RUNTIMES[0], RUNTIMES[1], RUNTIMES[3]];

/// One adapter per runtime (docs/runtime-qualification.md#runtime-adapters): code outside this module asks it,
/// never compares runtime names. Launch, session links and login probes live in this module and key on `name`.
// ponytail: a value table, not a trait; runtime capabilities differ only in data, a trait when one needs its own code outside
pub struct Adapter {
    pub name: &'static str,
    /// CLI binary and login account; `codex-app` shares Codex's.
    pub bin: &'static str,
    pub client: &'static str,
    pub home: &'static str,
    /// The worker is a CLI child of the service (`claude`, `codex exec`); an app turn is not.
    pub child: bool,
    /// This client requires an explicit operator grant; legacy requests never admit it.
    pub grant_required: bool,
    /// A Claude Remote Control session: permission-mode args, `claude agents` links, attempt id as session.
    pub remote: bool,
    /// A Codex app turn: no service child, macOS and Windows opt-in (§ 9).
    pub app: bool,
    /// Codex family: codex sandbox args, codex login, `codex://` thread links.
    pub codex: bool,
    /// Runtime whose access a setup launch of this client uses; empty: no setup.
    pub setup: &'static str,
    pub login: &'static str,
    pub sessions: &'static str,
    /// Oldest qualified CLI version and the owner's update step ([host tools](../docs/design/host-service.md#host-tools)).
    pub min: &'static str,
    pub update: &'static str,
}

const ADAPTERS: [Adapter; 4] = [
    Adapter { name: RUNTIMES[0], bin: "claude", client: "Claude Code", home: ".claude", child: true, grant_required: false, remote: true, app: false, codex: false, setup: RUNTIMES[0], login: "claude auth login", sessions: "`claude agents`", min: "2.1.296", update: "run `claude update`" },
    Adapter { name: RUNTIMES[1], bin: "codex", client: "Codex", home: ".codex", child: true, grant_required: false, remote: false, app: false, codex: true, setup: RUNTIMES[2], login: "codex login", sessions: "the Codex app threads", min: "0.159.3", update: "update Codex CLI (https://github.com/openai/codex#installation)" },
    Adapter { name: RUNTIMES[2], bin: "codex", client: "Codex", home: ".codex", child: false, grant_required: false, remote: false, app: true, codex: true, setup: "", login: "codex login", sessions: "the Codex app threads", min: "0.159.3", update: "update Codex CLI (https://github.com/openai/codex#installation)" },
    Adapter { name: RUNTIMES[3], bin: "agy", client: "Antigravity", home: ".gemini/antigravity", child: true, grant_required: true, remote: false, app: false, codex: false, setup: "", login: "sign in through the Antigravity client", sessions: "Antigravity conversations", min: "", update: "install or update the Antigravity agy CLI" },
];
const OTHER: Adapter = Adapter { name: "", bin: "", client: "", home: "", child: false, grant_required: false, remote: false, app: false, codex: false, setup: "", login: "", sessions: "", min: "", update: "" };

/// The CLI runtimes a host can run as service children, one per binary: the runtime rows of host tools.
pub fn clis() -> impl Iterator<Item = &'static Adapter> {
    ADAPTERS.iter().filter(|a| a.child)
}

/// The adapter of `rt`; an unknown name gets one that answers no to every question.
pub fn of(rt: &str) -> &'static Adapter {
    ADAPTERS.iter().find(|a| a.name == rt).unwrap_or(&OTHER)
}

/// Owner sessions run Claude Code.
pub const OWNER: &str = RUNTIMES[0];
/// The host config key that holds Codex CLI arguments.
pub const CODEX_ARGS: &str = "codex";

/// Runtimes a setup request may name, Codex first as the Connect list shows them.
pub fn setup_runtimes() -> impl Iterator<Item = &'static str> {
    ADAPTERS.iter().rev().filter(|a| !a.setup.is_empty()).map(|a| a.name)
}

/// The runtimes taskq can start here (§ 8): `codex-app` is macOS and Windows only and shares Codex's binary and login.
pub fn agent_runtimes() -> &'static [&'static str] {
    if cfg!(any(target_os = "macos", windows)) { &RUNTIMES } else { &CLI_RUNTIMES }
}

/// One row per runtime: `runtime`, `path` (null: not installed), `version`, `login`, `account`, `kind`, `refreshed`
/// (null: unknown or none), `reason` of a rejected login (#1446).
pub fn agents(install: &Path) -> serde_json::Value {
    let mut seen: std::collections::HashMap<&str, serde_json::Value> = std::collections::HashMap::new();
    let rows = agent_runtimes().iter().map(|rt| {
        let bin = of(rt).bin;
        let mut row = seen.entry(bin).or_insert_with(|| agent_row_in(install, rt, &which(bin), crate::runtime::accounts::base(bin).as_deref())).clone();
        row["runtime"] = serde_json::json!(rt);
        row
    }).collect();
    serde_json::Value::Array(rows)
}

fn agent_row_in(install: &Path, rt: &str, p: &Path, base: Option<&Path>) -> serde_json::Value {
    let bin = of(rt).bin;
    if !p.is_absolute() {
        return serde_json::json!({"path": null, "version": null, "login": "not installed", "account": null});
    }
    let ver = probe("runtime.version", p, &["--version"]).and_then(|(_, o)| version(&o));
    let home = base.map(|b| crate::runtime::accounts::active_in(install, bin, b));
    let env = home.as_deref().map(|h| (crate::runtime::accounts::var(bin), h));
    let out = if rt == "antigravity" { None } else { probe_in("runtime.login", p, if bin == "claude" { &["auth", "status"] } else { &["login", "status"] }, env) };
    let auth = (bin == "codex").then(|| home.as_deref().and_then(|h| std::fs::read_to_string(h.join("auth.json")).ok())).flatten();
    let (kind, refreshed) = kind_refresh(bin, out.as_ref().map(|o| o.1.as_str()), auth.as_deref());
    let (state, account) = if rt == "antigravity" { (login_state(rt, p), None) } else { login(bin, out) };
    // Codex's `login status` reads logged in with a dead refresh token: the last rejection wins until re-login
    let reason = (state == "logged in").then(|| login_off(install, bin, refreshed.as_deref())).flatten();
    let state = if reason.is_some() { "not logged in" } else { state };
    serde_json::json!({"path": p.display().to_string(), "version": ver, "login": state, "account": account, "kind": kind, "refreshed": refreshed, "reason": reason})
}

/// The heartbeat's CLI versions (§ 4): one per installed binary of `agents` rows; an unread version is `unknown` (R12).
pub fn clis_versions(rows: &serde_json::Value) -> std::collections::BTreeMap<String, String> {
    rows.as_array().into_iter().flatten().filter(|r| r["path"].is_string())
        .filter_map(|r| Some((of(r["runtime"].as_str()?).bin.to_string(), r["version"].as_str().unwrap_or("unknown").to_string())))
        .filter(|(bin, _)| !bin.is_empty()).collect()
}

/// The menu and status line of a row: `<runtime> <version> · <account>` or `· not logged in` etc.
pub fn agent_text(r: &serde_json::Value) -> String {
    let rt = r["runtime"].as_str().unwrap_or("?");
    let tail = match (r["login"].as_str(), r["account"].as_str()) {
        (Some("not installed"), _) => return format!("{rt} · not installed"),
        (Some("logged in"), Some(a)) => a.to_string(),
        (Some("logged in"), None) => "logged in, account unknown".into(),
        (Some("not logged in"), _) => r["reason"].as_str().map_or("not logged in".into(), |x| format!("not logged in ({x})")),
        _ => "login unknown".into(),
    };
    format!("{rt} {} · {tail}", r["version"].as_str().unwrap_or("unknown"))
}

/// The Settings view of a row ([AS5](../docs/design/agent-settings.md#as5-version-and-sign-in)): `state` is `signed-in`,
/// `sign-in-required`, `rejected`, `expired`, `check-failed`, `not-installed` or `unknown`; `account` only when the probe
/// names one (an email), the method (`ChatGPT`, `an API key`, `claude.ai max`) apart; `expires` only when supplied;
/// `refreshed` is the last refresh, never an expiry.
pub fn agent_view(r: &serde_json::Value) -> serde_json::Value {
    let account = r["account"].as_str().filter(|a| a.contains('@'));
    let method = r["kind"].as_str().or(r["account"].as_str().filter(|a| !a.contains('@')));
    let expires = r["expires"].as_i64();
    let state = match r["login"].as_str() {
        Some("logged in") if expires.is_some_and(|e| e <= now()) => "expired",
        Some("logged in") => "signed-in",
        Some("not logged in") if r["reason"].is_string() => "rejected",
        Some("not logged in") => "sign-in-required",
        Some("not installed") => "not-installed",
        // the probe ran and its answer did not parse
        Some("unknown") if r["path"].is_string() => "check-failed",
        _ => "unknown",
    };
    // a check never signs in: a needed sign-in names the native route (docs/relogin.md)
    let next = ["expired", "rejected", "sign-in-required"].contains(&state).then(|| relogin(r["runtime"].as_str().unwrap_or("")));
    serde_json::json!({"runtime": r["runtime"], "version": r["version"], "state": state, "account": account, "method": method, "expires": expires, "refreshed": r["refreshed"], "reason": r["reason"], "next": next})
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// One `taskq agents` line (§ 5): runtime, path, version, login, kind and last refresh; unread values read unknown (R12).
pub fn agents_line(r: &serde_json::Value) -> String {
    let s = |k: &str, none: &str| r[k].as_str().unwrap_or(none).to_string();
    let login = match (s("login", "unknown").as_str(), r["account"].as_str()) {
        ("logged in", Some(a)) => format!("logged in as {a}"),
        ("logged in", None) => "logged in, account unknown".into(),
        ("not logged in", _) if r["reason"].is_string() => format!("not logged in ({})", s("reason", "")),
        (l, _) => l.to_string(),
    };
    let version = if r["path"].is_null() { "-".into() } else { s("version", "unknown") };
    format!("{}  {}  {version}  {login}  kind {}  refreshed {}", s("runtime", "?"), s("path", "not installed"), s("kind", "unknown"), s("refreshed", "unknown"))
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;
    use super::*;

    // R12, #2143: qualify the refusal against an owned native writer without auth, a model turn or app control.
    #[cfg(unix)]
    #[test]
    #[ignore="installed Codex writer probe; isolated home, no model"]
    fn native_codex_writer_refusal_before_submission() {
        let root=std::env::temp_dir().join(format!("taskq-writer-{}",uuid()));std::fs::create_dir_all(&root).unwrap();
        let native=codex_native(which("codex")).unwrap();
        let script=r#"
import json,os,pathlib,selectors,subprocess,sys,time,uuid
home=pathlib.Path(sys.argv[2]);home.mkdir();thread=str(uuid.uuid4())
sessions=home/'sessions/2026/11/02';sessions.mkdir(parents=True)
rollout=sessions/f'rollout-2026-11-02T00-00-00-{thread}.jsonl'
rollout.write_text(json.dumps({'timestamp':'2026-11-02T00:00:00Z','type':'session_meta','payload':{'id':thread,'timestamp':'2026-11-02T00:00:00Z','cwd':sys.argv[3],'originator':'taskq-2143-fixture','cli_version':'0.159.3','source':'cli','model_provider':'openai'}})+'\n')
p=subprocess.Popen([sys.argv[1],'app-server'],env=dict(os.environ,CODEX_HOME=str(home)),stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
def call(i,method,params):
 p.stdin.write(json.dumps({'id':i,'method':method,'params':params})+'\n');p.stdin.flush()
 sel=selectors.DefaultSelector();sel.register(p.stdout,selectors.EVENT_READ);until=time.monotonic()+15
 while time.monotonic()<until:
  if not sel.select(max(0,until-time.monotonic())):break
  row=json.loads(p.stdout.readline())
  if row.get('id')==i:
   assert 'error' not in row,row
   return row['result']
 raise RuntimeError('native read deadline')
try:
 call(1,'initialize',{'clientInfo':{'name':'taskq-2143-probe','version':'1'}})
 call(2,'thread/resume',{'threadId':thread,'cwd':sys.argv[3]})
 print(thread,flush=True);sys.stdin.readline()
 row=call(3,'thread/read',{'threadId':thread,'includeTurns':True})['thread']
 assert not row['turns'],row
 print('writer retained; no turns submitted',flush=True)
finally:
 p.stdin.close()
 try:p.wait(timeout=10)
 except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=10)
"#;
        let home=root.join("codex-home");
        let mut owner=Command::new("python3").args(["-c",script]).arg(&native).arg(&home).arg(&root)
            .env_remove("OPENAI_API_KEY").env_remove("CODEX_API_KEY")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let mut reader=BufReader::new(owner.stdout.take().unwrap());let mut thread=String::new();reader.read_line(&mut thread).unwrap();
        let thread=thread.trim();assert!(plain_thread(thread));
        let mut l=Launch {files:String::new(),execution:Some(44),task:7,title:String::new(),attempt:42,role:"worker".into(),rt:"codex".into(),session:thread.into(),url:String::new(),resume:Some("retained fixture input".into())};
        let log=root.join("turn.log");
        for execution in [44,45] {
            l.execution=Some(execution);
            let start=std::fs::metadata(&log).map_or(0,|m|m.len());
            let text="TASKQ_2143_NO_AUTH_FIXTURE";
            let (path,_input)=turn_input(&root,text,Some(&l)).unwrap();
            bind_input(&log,&l,&path,text,serde_json::json!({"session":thread})).unwrap();
            let out=File::options().append(true).open(&log).unwrap();
            let mut cmd=Command::new(&native);cmd.env("CODEX_HOME",&home).args(["exec","--json","--skip-git-repo-check","resume",thread,"-"])
                .env_remove("OPENAI_API_KEY").env_remove("CODEX_API_KEY")
                .current_dir(&root).stdout(out.try_clone().unwrap()).stderr(out);
            let proc=start_file(&mut cmd,File::open(&path).unwrap()).unwrap();
            let mut turn=CliTurn {proc,log:log.clone(),pos:start,start,stopping:false,accepted:false,read_error:None,session:None,native:None,recovery:None};
            let until=std::time::Instant::now()+std::time::Duration::from_secs(20);
            let code=loop {if let Some(code)=turn.exited().unwrap() {break code;} assert!(std::time::Instant::now()<until,"native refusal timed out");std::thread::sleep(std::time::Duration::from_millis(10));};
            assert_ne!(code,0);assert_eq!(turn.accepted(),Ok(false));assert!(turn.refused(l.task,l.attempt,execution,code).is_some());
            assert_eq!(std::fs::read_to_string(path).unwrap(),text);
        }
        drop(owner.stdin.take());let mut reply=String::new();reader.read_line(&mut reply).unwrap();
        assert_eq!(reply.trim(),"writer retained; no turns submitted");assert!(owner.wait().unwrap().success());
        println!("native writer retained through two refused resumes; original inputs preserved; acceptance false; no model turns");
        std::fs::remove_dir_all(root).unwrap();
    }

    // A2/A4, R9/R12: agy must receive its own protocol and literal brief, never Codex/Claude policy flags.
    #[test]
    fn antigravity_commands_and_input_preserve_identity_and_grants() {
        let root = rules_root("antigravity");
        let args = ["-s", "danger-full-access", "-m", "invented-model", "-c", "model_reasoning_effort=high"].map(String::from);
        let vars = vec![("ANTIGRAVITY_HOME".into(), root.join("invented-home").display().to_string()), ("AGENTQ_OPTIMIZATION".into(), "off".into())];
        let env = Env { context:"", root:&root, exe:Path::new("taskq"), install:&root, permission_mode:"bypassPermissions", codex:&args, publish:"pr", board_env:&vars, jobs:Some(2) };
        let host = crate::policy::Host { os:"macos", install:&root, root:&root, home:&root };
        assert!(crate::policy::resolve(&Ok(crate::policy::Policy::default()), &host, "antigravity", env.permission_mode, &args).refused.is_some(), "legacy requests cannot grant a new runtime");
        let policy = Ok(crate::policy::Policy { revision:1, grants:vec![crate::policy::Grant {runtime:"antigravity".into(),mode:"workspace".into(),project:None,by:"invented operator".into(),at:1}] });
        let access = crate::policy::resolve(&policy, &host, "antigravity", env.permission_mode, &args);
        assert!(access.refused.is_none());
        assert!(access.profile.as_deref().unwrap().contains(".gemini/antigravity"));
        assert!(access.codex.is_none() && access.claude.is_none(), "a runtime grant never translates another client's policy");
        let unsupported = crate::policy::Host { os:"linux", install:&root, root:&root, home:&root };
        assert!(crate::policy::resolve(&policy, &unsupported, "antigravity", env.permission_mode, &args).refused.is_some(), "no Full fallback for unqualified containment");
        let text = "Literal \"quote\", $HOME, `command`\nsecond line 🦀";
        let input:serde_json::Value = serde_json::from_str(&stdin_text("antigravity", text)).unwrap();
        assert_eq!(input, serde_json::json!({"event":"user","message":{"content":text}}));
        for mode in ["workspace", "full"] {
            let mut policy = policy.clone();
            policy.as_mut().unwrap().grants[0].mode = mode.into();
            let access = crate::policy::resolve(&policy, &host, "antigravity", env.permission_mode, &args);
            assert!(access.refused.is_none());
            assert_eq!(access.profile.is_some(), mode == "workspace");
            for role in ["worker", "reviewer"] {
                for resume in [false, true] {
                    let launch = Launch {files:String::new(),execution:None,task:2136,title:"Invented fixture".into(),attempt:42,role:role.into(),rt:"antigravity".into(),session:if resume {"agy-owned"} else {"-"}.into(),url:String::new(),resume:resume.then(||text.into())};
                    let command = command(&env, &launch, &access).unwrap();
                    let actual:Vec<_> = command.get_args().map(|v|v.to_string_lossy().into_owned()).collect();
                    let protocol = actual.iter().position(|v|v=="--input-format").unwrap();
                    let mut expected = vec!["--input-format", "stream-json", "--output-format", "stream-json", "--dangerously-skip-permissions"];
                    if resume { expected.extend(["--conversation", "agy-owned"]); }
                    assert_eq!(&actual[protocol..], expected, "{role} resume={resume}");
                    assert!(!actual.iter().any(|v|v=="invented-model" || v=="bypassPermissions" || v=="exec"));
                    assert_eq!(command.get_current_dir(), Some(root.as_path()));
                    assert!(command.get_envs().any(|(k,v)|k=="TASKQ_ATTEMPT" && v==Some(std::ffi::OsStr::new("42"))));
                }
            }
        }
        let log = root.join("native.log");
        std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"other-runtime\"}\n{\"conversationId\":\"agy-owned\"}\n").unwrap();
        assert_eq!(native_session("antigravity", &log).as_deref(), Some("agy-owned"));
        assert!(of("antigravity").child && !of("antigravity").app && !of("antigravity").remote);
        assert!(agent_runtimes().contains(&"antigravity"));
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: R9/R12, #2145 A3/A4. Risk: a service PATH misses agy or credential presence stays unknown.
    #[test]
    fn antigravity_cli_state_and_user_bin_lookup() {
        if let Some(home) = std::env::var_os("TASKQ_AGY_FIXTURE") {
            let home = PathBuf::from(home);
            let bin = home.join(".local/bin/agy");
            assert_eq!(which("agy"), bin);
            assert_eq!(login_state("antigravity", &bin), "unknown");
            for name in [".gemini/antigravity-cli/jetski_state.pbtxt", ".gemini/oauth_creds.json"] {
                let path = home.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::create_dir(&path).unwrap();
                assert_eq!(login_state("antigravity", &bin), "unknown", "a directory is not credentials");
                std::fs::remove_dir(&path).unwrap();
                std::fs::write(&path, "invented fixture").unwrap();
                for rt in ["antigravity", "agy"] {
                    assert_eq!(login_state(rt, &bin), "logged in", "{name}");
                }
                let rows = agents(&home);
                let row = rows.as_array().unwrap().iter().find(|r|r["runtime"]=="antigravity").unwrap();
                assert_eq!(row["login"], "logged in");
                std::fs::remove_file(path).unwrap();
            }
            assert_eq!(login_state("antigravity", &bin), "unknown");
            return;
        }
        let root = rules_root("agy-cli-state");
        std::fs::create_dir_all(root.join(".local/bin")).unwrap();
        std::fs::write(root.join(".local/bin/agy"), "invented executable fixture").unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "runtime::tests::antigravity_cli_state_and_user_bin_lookup", "--nocapture"])
            .env("TASKQ_AGY_FIXTURE", &root).env("HOME", &root).env("USERPROFILE", &root).env("PATH", root.join("missing-bin"))
            .output().unwrap();
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        std::fs::remove_dir_all(root).unwrap();
    }

    // A2/A4, R11/R12: old terminal events cannot stop a resumed agy turn; only its own terminal confirms stop.
    #[test]
    fn antigravity_turn_uses_its_log_boundary_and_preserves_quota_unknowns() {
        struct Pending(bool);
        impl Proc for Pending {
            fn exited(&mut self)->Result<Option<i32>,String> {Ok(self.0.then_some(0))}
            fn kill(&mut self)->Result<(),String> {self.0=true;Ok(())}
            fn id(&self)->u32 {1}
        }
        let root=rules_root("antigravity-turn");let log=root.join("turn.log");
        let old="{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\"}}\n";
        std::fs::write(&log,old).unwrap();
        let mut turn=CliTurn {proc:Box::new(Pending(false)),log:log.clone(),pos:old.len() as u64,start:old.len() as u64,stopping:false,accepted:false,read_error:None,session:None,native:None,recovery:None};
        assert_eq!(turn.exited(),Ok(None));assert_eq!(turn.accepted(),Ok(false));
        writeln!(File::options().append(true).open(&log).unwrap(),"{}",serde_json::json!({"event":"step_update","step_update":{"step_type":"tool"}})).unwrap();
        assert_eq!(turn.exited(),Ok(None));assert_eq!(turn.accepted(),Ok(true));
        writeln!(File::options().append(true).open(&log).unwrap(),"{}",serde_json::json!({"event":"result","result":{"status":"SUCCESS"}})).unwrap();
        assert_eq!(turn.exited(),Ok(Some(0)));
        std::fs::write(&log,"{\"conversationId\":\"foreign-conversation\"}\n").unwrap();
        let mut copied=CliTurn {proc:Box::new(Pending(false)),log:log.clone(),pos:0,start:0,stopping:false,accepted:true,read_error:None,session:Some("owned-conversation".into()),native:None,recovery:None};
        assert!(copied.exited().unwrap_err().contains("conversation changed"));
        assert!(copied.accepted().is_err() && !copied.stopping,"another conversation cannot end this turn");
        assert!(copied.exited().is_err(),"a later poll must not skip the conflicting identity");
        let quota=serde_json::json!({"event":"result","result":{"status":"ERROR","error":"quota exceeded"}}).to_string()+"\n";
        std::fs::write(&log,&quota).unwrap();assert_eq!(usage_limit(&log),Some(None),"no invented reset or quota window");
        std::fs::write(&log,format!("{{\"event\":\"step_update\",\"step_update\":{{\"step_type\":\"tool\"}}}}\n{quota}")).unwrap();
        assert_eq!(usage_limit(&log),None,"mid-work quota must not become a pre-work retry");
        std::fs::remove_dir_all(root).unwrap();
    }

    // A4/R9: launch intent must preserve client policy and identity, never enable or rewrite an integration.
    #[test]
    fn optimization_preserves_policy_and_reports_effective_state() {
        use agentq_runtime::{agent::{Client, Spec}, optimization::Intent};
        let root = rules_root("optimization");
        let home = root.join("home");
        let install = root.join("optimizer");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(install.join("data")).unwrap();
        let config = "model_provider = \"agentq\"\n";
        std::fs::write(home.join("config.toml"), config).unwrap();
        std::fs::write(home.join("settings.json"), r#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"agentq hook claude pre"}]}]}}"#).unwrap();
        for client in [Client::Claude, Client::Codex] {
            for (intent, effective) in [("inherit", "on"), ("off", "off"), ("on", "on")] {
                for resume in [false, true] {
                    let mut spec = Spec { client, root: root.clone(), session: "saved-native-id".into(), resume,
                        args: vec!["--permission-mode".into(), "dontAsk".into()], home: Some(home.clone()),
                        env: vec![("AGENTQ_OPTIMIZATION".into(), intent.into()), ("TASKQ_TASK".into(), "2111".into())] };
                    let state = optimization(&mut spec, Some(&install)).unwrap();
                    assert_eq!((state.intent, state.effective), (Intent::parse(intent).unwrap(), effective));
                    assert_eq!(spec.session, "saved-native-id");
                    assert_eq!((spec.resume, spec.home.as_ref()), (resume, Some(&home)));
                    assert_eq!(&spec.args[..2], &["--permission-mode", "dontAsk"]);
                    assert_eq!(spec.args.len(), if client == Client::Claude || intent == "off" { 4 } else { 2 });
                    if intent == "off" && client == Client::Codex { assert_eq!(spec.args[3], "model_providers.agentq.http_headers.x-agentq-optimization=\"off\""); }
                    if intent == "on" {
                        assert_eq!(optimization(&mut spec, None).unwrap().reason, "not_installed");
                        std::fs::write(install.join("data/config.json"), r#"{"compression":{"enabled":false}}"#).unwrap();
                        assert_eq!(optimization(&mut spec, Some(&install)).unwrap().effective, "off");
                        std::fs::remove_file(install.join("data/config.json")).unwrap();
                    }
                    spec.env.push(("AGENTQ_OPTIMIZATION".into(), "invalid".into()));
                    assert!(optimization(&mut spec, Some(&install)).unwrap_err().contains("inherit, off or on"));
                }
            }
        }
        assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), config);
        assert!(!install.join("data/integration.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    // A3/A4/R9: daemon-backed turns need session env, including on resume; saved policy stays untouched.
    #[test]
    fn optimization_reaches_background_settings_and_operator_attribution() {
        use agentq_runtime::agent::{Client, Spec};
        for resume in [false, true] {
            let mut spec = Spec { client: Client::Claude, root: std::env::temp_dir(), session: "saved-native-id".into(), resume,
                args: vec!["--settings".into(), r#"{"env":{"TASKQ_INSTALL":"invented"},"hooks":{"SessionStart":[]}}"#.into(),
                    "--permission-mode".into(), "dontAsk".into(), "--name".into(), "retained name".into()], home: None,
                env: vec![("AGENTQ_OPTIMIZATION".into(), "off".into())] };
            optimization(&mut spec, None).unwrap();
            let command = agentq_runtime::agent::background_command(background_base(Command::new("invented-client"), &spec), &spec).unwrap();
            let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            assert_eq!(args.iter().filter(|a| a.as_str() == "--settings").count(), 1);
            let settings: serde_json::Value = serde_json::from_str(&args[args.iter().position(|a| a == "--settings").unwrap() + 1]).unwrap();
            assert_eq!(settings["env"]["AGENTQ_OPTIMIZATION"], "off");
            assert_eq!(settings["env"]["AGENTQ_LAUNCHER"], "taskq");
            if resume {
                assert_eq!(args.len(), 5, "only optimizer settings, --bg and exact native resume");
                assert_eq!(&args[2..], &["--bg", "--resume", "saved-native-id"]);
                assert_eq!(settings["env"].as_object().unwrap().len(), 2, "no permission, model or attempt override");
            } else {
                assert_eq!(settings["env"]["TASKQ_INSTALL"], "invented");
                assert_eq!(settings["hooks"]["SessionStart"], serde_json::json!([]));
                assert!(args.windows(2).any(|a| a == ["--permission-mode", "dontAsk"]));
                assert!(args.windows(2).any(|a| a == ["--name", "retained name"]));
            }
        }
    }

    // Risk: Windows `explorer` exits 1 after a successful open, so a Codex app queue reads it as a failed open (#1593).
    #[test]
    fn open_exit_rule() {
        assert!(opened(true, Some(0)) && opened(true, Some(1)) && opened(false, Some(0)));
        assert!(!opened(false, Some(1)) && !opened(true, Some(2)) && !opened(true, None));
    }

    // Chain: R3 session name -> Claude `--name`/Remote Control and Codex first input line (#1564). Risk: sessions
    // show the shared rules header or an uncut title instead of role, task and title.
    #[test]
    fn session_name_is_role_task_and_cut_title() {
        let l = |role: &str, title: &str| Launch { files: String::new(), execution: None, task: 1561, title: title.into(), attempt: 1, role: role.into(), rt: "codex".into(), session: "s".into(), url: String::new(), resume: None };
        assert_eq!(session_name(&l("worker", "Claude handover identity")), "work #1561 Claude handover identity");
        assert_eq!(session_name(&l("reviewer", "a\n b")), "review #1561 a b");
        assert_eq!(session_name(&l("operator", "x")), "setup #1561 x");
        let long = "Name worker sessions by role, task and title everywhere";
        assert_eq!(session_name(&l("worker", long)), format!("work #1561 {}", long.chars().take(40).collect::<String>().trim_end()));
        let cyr = "Имя сессии воркера: роль, задача и заголовок ✓ длинный хвост";
        let name = session_name(&l("worker", cyr));
        assert_eq!(name.strip_prefix("work #1561 ").unwrap().chars().count(), 40);
        assert!(cyr.starts_with(name.strip_prefix("work #1561 ").unwrap()));
    }

    // Chain: board session -> Remote Control lookup and Codex app link. Risk: a Codex session is probed as Claude.
    #[test]
    fn session_links_follow_the_adapter() {
        assert_eq!(remote_session("claude:abc mac"), Some("abc"));
        assert_eq!((remote_session("codex:abc mac"), remote_session("codex-app:abc"), remote_session("claude")), (None, None, None));
        assert_eq!(thread_of(&thread_url("019a-b2")), Some("019a-b2"));
    }

    // #2137 A5/A7: structured window metadata survives overlap; AgentQ classifies agy errors; old turns stay old.
    #[test]
    fn balancing_quota_observations_keep_windows() {
        let log=std::env::temp_dir().join(format!("taskq-quota-metadata-{}",uuid()));
        std::fs::write(&log,concat!("{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"rejected\",\"rateLimitType\":\"seven_day\",\"resetsAt\":604800}}\n",
            "{\"type\":\"rate_limits\",\"rate_limits\":{\"primary\":{\"used_percent\":100,\"window_minutes\":300,\"resets_at\":18000}}}\n")).unwrap();
        assert_eq!(quota_windows(&log),[("300m".into(),Some(18000)),("seven_day".into(),Some(604800))]);
        rules_line(&log,"resume","fixture input").unwrap();
        assert!(quota_windows(&log).is_empty(),"a successful later turn must not reuse old quota events");
        std::fs::write(&log,"{\"event\":\"result\",\"result\":{\"status\":\"ERROR\",\"error\":\"quota exceeded\"}}\n").unwrap();
        assert_eq!(quota_windows(&log),[("unknown".into(),None)],"no guessed agy reset/window");
        std::fs::remove_file(log).unwrap();
    }

    // Chain: attempt log -> usage limit before work -> reset. Risk: a limit after work started skips its retry,
    // or a limit before work counts against the gone limit (#1276).
    #[test]
    fn usage_limit_only_before_work() {
        let log = std::env::temp_dir().join(format!("taskq-usage-limit-{}.log", uuid()));
        let limit = r#"{"type":"assistant","apiError":"usage_limit_reached","quotaLimits":{"status":"rejected","resetsAt":1792386000},"message":{"content":[{"type":"text","text":"You've hit your weekly limit"}]}}"#;
        let work = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#;
        let cases = [
            (format!("{{\"type\":\"system\",\"subtype\":\"init\"}}\n{limit}\n{{\"type\":\"result\",\"is_error\":true}}\n"), Some(Some(1792386000))),
            (format!("{work}\n{limit}\n"), None),
            (r#"{"type":"assistant","apiError":"usage_limit_reached","apiErrorParams":{"rate_limit_info":{"resetsAt":7}}}"#.to_string(), Some(Some(7))),
            ("{\"type\":\"thread.started\"}\n{\"type\":\"turn.started\"}\n{\"type\":\"error\",\"message\":\"You've hit your usage limit. Try again later.\"}\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"You've hit your usage limit.\"}}\n".to_string(), Some(None)),
            ("{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\"}}\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"usage limit\"}}\n".to_string(), None),
            ("{\"type\":\"turn.failed\",\"error\":{\"message\":\"stream disconnected\"}}\n".to_string(), None),
        ];
        for (text, want) in cases {
            std::fs::write(&log, &text).unwrap();
            assert_eq!(usage_limit(&log), want, "{text}");
        }
        let _ = std::fs::remove_file(&log);
        assert_eq!(usage_limit(&log), None, "an unreadable log is no limit");
    }

    // Chain: attempt log -> last turn failed on model capacity -> continuing steer (#1843).
    // Risk: a mid-work capacity failure ends the attempt as `gone`, or a later completed turn or other error is retried.
    #[test]
    fn transient_failure_reads_the_last_turn() {
        let log = std::env::temp_dir().join(format!("taskq-transient-{}.log", uuid()));
        let failed = r#"{"type":"turn.failed","error":{"message":"Error running remote compact task: Selected model is at capacity. Please try a different model."}}"#;
        let work = r#"{"type":"item.completed","item":{"type":"command_execution"}}"#;
        let cases = [
            (format!("{{\"type\":\"thread.started\",\"thread_id\":\"t\"}}
{work}
{{\"type\":\"error\",\"message\":\"Selected model is at capacity. Please try a different model.\"}}
{failed}
"),
                Some("Error running remote compact task: Selected model is at capacity. Please try a different model.")),
            (format!("{failed}
{{\"type\":\"turn.started\"}}
{work}
{{\"type\":\"turn.completed\"}}
"), None),
            (format!("{work}
{{\"type\":\"turn.failed\",\"error\":{{\"message\":\"stream disconnected\"}}}}
"), None),
            (format!("{work}
{{\"type\":\"error\",\"message\":\"Selected model is at capacity.\"}}
"), None),
        ];
        for (text, want) in cases {
            std::fs::write(&log, &text).unwrap();
            assert_eq!(transient_failure(&log).as_deref(), want, "{text}");
        }
        let _ = std::fs::remove_file(&log);
        assert_eq!(transient_failure(&log), None, "an unreadable log is no failure");
    }

    // Chain: installed npm entrypoint -> exact native executable -> owned process identity.
    // Risk: tracking a launcher lets its child survive cancellation and falsely releases capacity.
    #[cfg(unix)]
    #[test]
    fn codex_launch_resolves_native_package_without_wrapper_fallback() {
        let root=std::env::temp_dir().join(format!("taskq-codex-package-{}",uuid()));
        let package=root.join("node_modules/@openai/codex");let bin=package.join("bin/codex.js");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();std::fs::write(&bin,"#!/usr/bin/env node\n").unwrap();
        std::fs::write(package.join("package.json"),r#"{"name":"@openai/codex"}"#).unwrap();
        assert!(codex_native(bin.clone()).is_err(),"a missing native package must not run the launcher");
        for target in ["aarch64-apple-darwin","x86_64-apple-darwin","aarch64-unknown-linux-musl","x86_64-unknown-linux-musl"] {
            let path=package.join(format!("vendor/{target}/bin/codex"));std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path,b"\x7fELFfixture").unwrap();
        }
        let native=codex_native(bin.clone()).unwrap();
        assert_ne!(native,bin);assert_eq!(codex_native(native.clone()).unwrap(),native);
        std::fs::write(&native,"#!/bin/sh\n").unwrap();assert!(codex_native(bin.clone()).is_err());
        std::fs::write(&native,b"\x7fELFfixture").unwrap();
        std::fs::write(package.join("package.json"),r#"{"name":"foreign"}"#).unwrap();
        assert!(codex_native(bin).unwrap_err().contains("identity differs"));
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: actual installed Codex entrypoint -> native PID -> stop -> observed exit.
    // No prompt is sent; EOF cleans up even if an assertion fails before cancellation.
    #[cfg(any(unix,windows))]
    #[test]
    #[ignore="owned installed CLI packaging probe; no model or prompt"]
    fn native_codex_executable_stop_without_model() {
        let entry=which("codex");let native=codex_native(entry.clone()).unwrap();
        let version=coreq::process::output("qualification.codex-version",Command::new(&native).arg("--version"),std::time::Duration::from_secs(5)).unwrap();
        let mut child=Command::new(&native).args(["exec","--json","-"])
            .env_remove("CODEX_THREAD_ID").env_remove("CLAUDE_CODE_SESSION_ID")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let identity=coreq::process::identity(child.id()).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(child.try_wait().unwrap(),None);
        assert_eq!(coreq::process::identity(child.id()).unwrap(),Some(identity.clone()));
        coreq::process::terminate(&identity).unwrap();child.wait().unwrap();
        assert_eq!(coreq::process::identity(identity.pid).unwrap(),None);
        let output=child.wait_with_output().unwrap();assert!(output.stdout.is_empty(),"no native turn should start without input");
        println!("{}entry={} native={} pid={} native exit confirmed; no model dispatched",String::from_utf8_lossy(&version.stdout),entry.display(),native.display(),identity.pid);
    }

    // Chain: actual native Codex -> owner crash -> cold adoption -> cancellation/exit receipt.
    // The controller holds stdin open; no prompt, model, tools or live board are involved.
    #[cfg(any(target_os="macos",windows))]
    #[test]
    #[ignore="owned installed CLI cold-recovery probe; no model or prompt"]
    fn native_codex_cold_owner_restart() {
        let only=std::env::var("TASKQ_NATIVE_OWNER_ONLY").ok();
        assert!(only.as_deref().is_none_or(|phase|matches!(phase,"cancel"|"ended")),"invalid native fixture selector");
        for phase in ["cancel","ended"] {
            if only.as_deref().is_some_and(|selected|selected!=phase) {continue;}

            let root=std::env::temp_dir().join(format!("taskq-native-owner-{phase}-{}",uuid()));
            std::fs::create_dir_all(&root).unwrap();let root=root.canonicalize().unwrap();
            let mut command=Command::new(std::env::current_exe().unwrap());
            command.args(["--exact","runtime::tests::native_codex_owner_child","--ignored","--nocapture"])
                .env("TASKQ_NATIVE_OWNER_ROOT",&root).env("TASKQ_NATIVE_OWNER_CASE",phase);
            let mut old=command.env("TASKQ_NATIVE_OWNER_PHASE","before").stdin(Stdio::piped())
                .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            let mut input=Some(old.stdin.take().unwrap());
            let until=std::time::Instant::now()+std::time::Duration::from_secs(5);
            while !root.join("ready").exists() {
                assert!(old.try_wait().unwrap().is_none(),"owner exited before checkpoint");
                assert!(std::time::Instant::now()<until,"native checkpoint timed out");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let saved:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("ready")).unwrap()).unwrap();
            let identity=coreq::process::Identity {pid:saved["pid"].as_u64().unwrap() as u32,started:saved["started"].as_str().unwrap().into()};
            old.kill().unwrap();old.wait().unwrap();
            assert_eq!(coreq::process::identity(identity.pid).unwrap(),Some(identity.clone()),"the native executor must survive its owner for this case");
            if phase=="ended" {
                drop(input.take());
                while !coreq::process::identity(identity.pid).is_ok_and(|p|p.is_none()) {
                    assert!(std::time::Instant::now()<until,"native exit after EOF timed out");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            let result=coreq::process::output("qualification.native-cold-owner",command.env("TASKQ_NATIVE_OWNER_PHASE","after")
                .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()),std::time::Duration::from_secs(10));
            drop(input);
            assert!(result.is_ok(),"{phase}: {result:?}");
            let output=result.unwrap();
            for line in String::from_utf8_lossy(&output.stdout).lines().filter(|l|l.starts_with("intermediate native readback")) {println!("{phase}: {line}");}
            assert_eq!(coreq::process::identity(identity.pid).unwrap(),None);
            let expected=if cfg!(windows) && phase=="ended" {"missing native scope; explicit unknown; evidence retained; no release or replay"}
                else {"same native execution; acceptance unknown; exit read back; no replay"};
            assert_eq!(std::fs::read_to_string(root.join("verified")).unwrap(),expected);
            println!("{phase}: native pid={} survived owner crash; {expected}; no model dispatched",identity.pid);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(any(target_os="macos",windows))]
    #[test]
    #[ignore="native CLI owner process fixture"]
    fn native_codex_owner_child() {
        use crate::board::Board;
        let root=PathBuf::from(std::env::var_os("TASKQ_NATIVE_OWNER_ROOT").unwrap());
        let b=crate::board::Fake::default();let task=b.create("native fixture","",&[crate::service::TASK_LABEL.into()]).unwrap();
        let attempt=b.comment(task,"taskq: spawn worker codex:fixture mac").unwrap();
        let cfg=crate::service::Config {root:root.clone(),install:root.join("install"),..crate::tests::cfg()};
        let mut service=crate::service::Service::new(&b,&cfg,"native-fixture",Box::new(|_|panic!("native recovery must not dispatch")));
        if std::env::var("TASKQ_NATIVE_OWNER_PHASE").unwrap()=="before" {
            let log=attempt_log(&root,"worker",task,attempt);std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            let output=File::create(&log).unwrap();
            let mut command=Command::new(codex_native(which("codex")).unwrap());
            command.args(["exec","--json","-"]).current_dir(&root).env_remove("CODEX_THREAD_ID").env_remove("CLAUDE_CODE_SESSION_ID")
                .stdin(Stdio::inherit()).stderr(output.try_clone().unwrap()).stdout(output);
            #[cfg(windows)] let child:Box<dyn Proc>=Box::new(windows_process::Execution::spawn(&mut command).unwrap());
            #[cfg(not(windows))] let child:Box<dyn Proc>=Box::new(command.spawn().unwrap());
            let native=child.native_identity().unwrap();
            let launch=Launch {files: String::new(), execution:Some(attempt),task,title:String::new(),attempt,role:"worker".into(),rt:"codex".into(),session:"fixture".into(),url:String::new(),resume:None};
            let mut proc=CliTurn {proc:child,log,pos:0,start:0,stopping:false,accepted:false,read_error:None,session:None,native:Some(native.clone()),recovery:None};
            proc.checkpoint(&cfg,&launch,attempt).unwrap();
            service.kids.push(crate::service::Kid { unknown:None, stopping:false, session_url:None,task,attempt,execution:attempt,rt:"codex".into(),proc:Box::new(proc)});
            crate::data::kids(&std::fs::read_to_string(crate::kids_path(&cfg.install,&root).unwrap()).unwrap()).unwrap();
            coreq::write_durable(&root.join("ready"),&serde_json::json!({"pid":native.pid,"started":native.started}).to_string()).unwrap();
            let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
            loop {assert!(std::time::Instant::now()<until,"controller did not kill the owner");std::thread::sleep(std::time::Duration::from_millis(10));}
        }
        #[cfg(windows)]
        if std::env::var("TASKQ_NATIVE_OWNER_CASE").unwrap()=="ended" {
            let error=crate::adopt(&mut service,&cfg.install,&root).unwrap_err();
            assert!(error.contains("native reconciliation"),"{error}");
            assert!(service.kids.is_empty());assert_eq!(crate::kids_file(&cfg.install,&root).unwrap().len(),1);
            assert_eq!(crate::core::executions(task,&crate::board::entries(&b,task).unwrap())[0].release,None);
            std::fs::write(root.join("verified"),"missing native scope; explicit unknown; evidence retained; no release or replay").unwrap();
            return;
        }
        crate::adopt(&mut service,&cfg.install,&root).unwrap();
        assert_eq!(service.kids.len(),1);assert_eq!(service.kids[0].execution,attempt);
        if std::env::var("TASKQ_NATIVE_OWNER_CASE").unwrap()=="cancel" {
            service.reap_owned();assert_eq!(service.kids.len(),1,"a live executor cannot release on owner restart");
            assert!(!crate::core::executions(task,&crate::board::entries(&b,task).unwrap())[0].accepted);
            service.kids[0].proc.kill().unwrap();
        }
        let until=std::time::Instant::now()+std::time::Duration::from_secs(5);
        let mut unknown=std::collections::BTreeSet::new();
        loop {
            match service.kids[0].proc.exited() {
                Ok(Some(_))=>break,
                Err(error)=>{unknown.insert(error);},
                Ok(None)=>{},
            }
            assert!(std::time::Instant::now()<until,"native stop did not finish: {unknown:?}");std::thread::sleep(std::time::Duration::from_millis(10));
        }
        println!("intermediate native readback errors (never terminal): {unknown:?}");
        b.state.borrow_mut().fail_comments=1;
        service.reap_owned();assert_eq!(service.kids.len(),1,"lost exit receipt must retain ownership");
        assert_eq!(crate::kids_file(&cfg.install,&root).unwrap().len(),1);
        service.reap_owned();assert!(service.kids.is_empty());
        let x=crate::core::executions(task,&crate::board::entries(&b,task).unwrap()).remove(0);
        assert!(!x.accepted);assert_eq!(x.release,Some(crate::core::Release::Exit));
        assert!(crate::kids_file(&cfg.install,&root).unwrap().is_empty());
        std::fs::write(root.join("verified"),"same native execution; acceptance unknown; exit read back; no replay").unwrap();
    }

    // Chain: prepared brief -> native stdin -> consumer or non-reading child, with no blocking producer.
    #[test]
    fn file_stdin_handoff_preserves_bytes_and_never_waits_for_a_reader() {
        let root = std::env::temp_dir().join(format!("taskq-input-handoff-{}", uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let text = format!("{}EOF-CANARY", "private-input-".repeat(12_000));
        for mode in ["blocked", "read"] {
            let go = root.join(mode);
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "runtime::tests::stdin_child", "--ignored"])
                .env("TASKQ_STDIN_MODE", mode).env("TASKQ_STDIN_GO", &go).stdout(Stdio::null()).stderr(Stdio::null());
            let began = std::time::Instant::now();
            let mut child = start_command(&mut command, &root, &text).unwrap_or_else(|e| panic!("{e}"));
            assert!(began.elapsed() < std::time::Duration::from_secs(3), "handoff waited for the child to read");
            for entry in std::fs::read_dir(root.join(".taskq/inputs")).unwrap() {
                let path = entry.unwrap().path();
                #[cfg(unix)]
                { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600); }
                std::fs::remove_file(path).unwrap();
            }
            if mode == "blocked" { let _pending = stop(child.as_mut()); }
            else { std::fs::write(&go, "read now").unwrap(); }
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.exited().unwrap() { break status; }
                if std::time::Instant::now() >= until {
                    let cleanup = stop(child.as_mut());
                    panic!("input fixture did not finish; cleanup: {cleanup:?}");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            if mode == "read" { assert!(status==0, "child could not read the original bytes after cache unlink"); }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "file stdin handoff fixture"]
    fn stdin_child() {
        if std::env::var("TASKQ_STDIN_MODE").unwrap() == "blocked" {
            std::thread::sleep(std::time::Duration::from_secs(30)); return;
        }
        let go = PathBuf::from(std::env::var_os("TASKQ_STDIN_GO").unwrap());
        while !go.exists() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        let mut text = String::new(); std::io::stdin().read_to_string(&mut text).unwrap();
        assert_eq!(text, format!("{}EOF-CANARY", "private-input-".repeat(12_000)));
    }

    // Chain: native owned child / exec-adopted child -> stop request -> OS exit readback.
    #[test]
    fn native_stop_reaps_the_owned_process() {
        let spawn = || Command::new(std::env::current_exe().unwrap()).args(["--exact", "runtime::tests::stop_child", "--ignored"])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let check = |proc: &mut dyn Proc| {
            let requested = stop(proc);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                if proc.exited().unwrap().is_some() { break; }
                assert!(std::time::Instant::now() < until, "owned stop did not finish: {requested:?}");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        check(&mut spawn());
        #[cfg(unix)]
        {
            let child = spawn(); let pid = child.id(); drop(child);
            check(&mut Adopted::new(coreq::process::identity(pid).unwrap().unwrap()));
            assert_eq!(liveness(pid), Ok(false));
        }
    }

    // Chain: production stdin launch -> launcher exits -> exact known descendant remains live.
    // Risk: root exit releases the execution while an owned tool is still running.
    #[cfg(windows)]
    #[test]
    #[ignore="owned Windows process-tree qualification; no model or board"]
    fn native_cli_owner_exit_does_not_release_live_descendant() {
        use crate::board::Board;
        for cold in [false,true] {
            let root=std::env::temp_dir().join(format!("taskq-native-tree-{}",uuid()));std::fs::create_dir_all(&root).unwrap();
            let board=crate::board::Fake::default();let task=board.create("native scope fixture","",&[crate::service::TASK_LABEL.into()]).unwrap();
            let attempt=board.comment(task,"taskq: spawn worker codex:fixture mac").unwrap();
            let cfg=crate::service::Config {root:root.clone(),install:root.join("install"),..crate::tests::cfg()};
            let log=attempt_log(&root,"worker",task,attempt);std::fs::create_dir_all(log.parent().unwrap()).unwrap();std::fs::write(&log,"").unwrap();
            let mut command=Command::new(std::env::current_exe().unwrap());
            command.args(["--exact","runtime::tests::windows_tree_owner_child","--ignored"])
                .env("TASKQ_TREE_OWNER_ROOT",&root).stdout(Stdio::null()).stderr(Stdio::null());
            let child=start_command(&mut command,&root,"").unwrap();let native=child.native_identity().unwrap();
            let mut turn=CliTurn {proc:child,log,pos:0,start:0,stopping:false,accepted:false,read_error:None,session:None,native:Some(native.clone()),recovery:None};
            let launch=Launch {files: String::new(), execution:Some(attempt),task,title:String::new(),attempt,role:"worker".into(),rt:"codex".into(),session:"fixture".into(),url:String::new(),resume:None};
            turn.checkpoint(&cfg,&launch,attempt).unwrap();
            let saved=crate::kids_file(&cfg.install,&root).unwrap();assert_eq!(saved.len(),1);assert!(saved[0].job.is_some());
            let mut legacy=saved[0].clone();legacy.job=None;
            assert!(execution_live(&legacy).unwrap_err().contains("legacy"));
            let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
            while !root.join("descendant.json").exists() {
                assert!(std::time::Instant::now()<until,"owned fixture did not announce its descendant");std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let row:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("descendant.json")).unwrap()).unwrap();
            let descendant=coreq::process::Identity {pid:row["pid"].as_u64().unwrap() as u32,started:row["started"].as_str().unwrap().into()};
            let mut unrelated=Command::new(std::env::current_exe().unwrap());
            unrelated.args(["--exact","runtime::tests::stop_child","--ignored"]).stdout(Stdio::null()).stderr(Stdio::null());
            let mut sentinel=start_command(&mut unrelated,&root,"").unwrap();
            assert!(windows_process::Execution::recover(native.clone(),&sentinel.job().unwrap()).err().unwrap().contains("does not belong"));
            assert_eq!(turn.exited(),Ok(None));std::fs::write(root.join("exit-owner"),"").unwrap();
            while coreq::process::identity(native.pid).unwrap().as_ref()==Some(&native) {
                assert!(std::time::Instant::now()<until,"fixture launcher did not exit");std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(coreq::process::identity(descendant.pid).unwrap(),Some(descendant.clone()));
            assert!(execution_live(&saved[0]).unwrap(),"scope-aware readback cannot retire surviving descendants");
            let mut service=crate::service::Service::new(&board,&cfg,"native-fixture",Box::new(|_|panic!("native reconstruction must not dispatch")));
            let mut proc:Box<dyn Proc>=if cold {
                drop(turn);
                let error=crate::adopt(&mut service,&cfg.install,&root).unwrap_err();
                assert!(error.contains("native reconciliation"),"{error}");
                assert!(service.kids.is_empty());assert_eq!(crate::kids_file(&cfg.install,&root).unwrap().len(),1);
                assert_eq!(crate::core::executions(task,&crate::board::entries(&board,task).unwrap())[0].release,None);
                assert_eq!(coreq::process::identity(descendant.pid).unwrap(),Some(descendant.clone()),"unknown recovery cannot stop or release work");
                // The fixture knows this exact descendant; product recovery has no such surviving observation.
                coreq::process::terminate(&descendant).unwrap();
                while coreq::process::identity(descendant.pid).unwrap().as_ref()==Some(&descendant) {assert!(std::time::Instant::now()<until,"fixture descendant cleanup timed out");std::thread::sleep(std::time::Duration::from_millis(10));}
                assert_eq!(sentinel.exited(),Ok(None));sentinel.kill().unwrap();
                while sentinel.exited().unwrap().is_none() {assert!(std::time::Instant::now()<until,"sentinel cleanup not confirmed");std::thread::sleep(std::time::Duration::from_millis(10));}
                drop(service);drop(sentinel);std::fs::remove_dir_all(root).unwrap();
                println!("cold=true: native scope unavailable; explicit unknown; protected evidence retained; no replay/release; exact fixture cleanup confirmed");
                continue;
            } else {Box::new(turn)};
            assert_eq!(proc.exited(),Ok(None),"launcher exit cannot release a live descendant");
            proc.kill().unwrap();
            while proc.exited().unwrap().is_none() {assert!(std::time::Instant::now()<until,"native job cancellation did not finish");std::thread::sleep(std::time::Duration::from_millis(10));}
            assert_ne!(coreq::process::identity(descendant.pid).unwrap(),Some(descendant.clone()));
            assert!(!execution_live(&saved[0]).unwrap());assert_eq!(sentinel.exited(),Ok(None),"another execution's job must remain live");
            sentinel.kill().unwrap();
            while sentinel.exited().unwrap().is_none() {assert!(std::time::Instant::now()<until,"sentinel cleanup not confirmed");std::thread::sleep(std::time::Duration::from_millis(10));}
            service.kids.push(crate::service::Kid { unknown:None, stopping:false, session_url:None,task,attempt,execution:attempt,rt:"codex".into(),proc});
            board.state.borrow_mut().fail_comments=1;service.reap_owned();assert_eq!(service.kids.len(),1,"failed terminal receipt retains ownership");
            service.reap_owned();assert!(service.kids.is_empty());assert!(crate::kids_file(&cfg.install,&root).unwrap().is_empty());
            assert_eq!(crate::core::executions(task,&crate::board::entries(&board,task).unwrap())[0].release,Some(crate::core::Release::Exit));
            drop(service);drop(sentinel);std::fs::remove_dir_all(root).unwrap();
            println!("cold={cold}: launcher exit held while descendant lived; native job cancelled; unrelated job preserved; terminal receipt retried; protected record retired");
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore="owned Windows tree launcher fixture"]
    fn windows_tree_owner_child() {
        let root=PathBuf::from(std::env::var_os("TASKQ_TREE_OWNER_ROOT").unwrap());
        let child=Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::stop_child","--ignored"])
            .env_remove("TASKQ_TREE_OWNER_ROOT").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let identity=coreq::process::identity(child.id()).unwrap().unwrap();
        coreq::write_durable(&root.join("descendant.json"),&serde_json::json!({"pid":identity.pid,"started":identity.started}).to_string()).unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while !root.join("exit-owner").exists() {assert!(std::time::Instant::now()<until,"controller did not release fixture launcher");std::thread::sleep(std::time::Duration::from_millis(10));}
        drop(child);
    }

    #[test]
    #[ignore = "owned process stop fixture"]
    fn stop_child() { std::thread::sleep(std::time::Duration::from_secs(30)); }

    // Chain: terminal CLI log / cancelled app handshake -> stop -> observed exit, never a synthetic completion.
    #[test]
    fn stop_retains_terminal_cli_and_cancelled_handshake_until_exit() {
        use std::{cell::Cell, rc::Rc};
        struct Pending { exit: Rc<Cell<bool>>, kills: Rc<Cell<usize>> }
        impl Proc for Pending {
            fn exited(&mut self) -> Result<Option<i32>, String> { Ok(self.exit.get().then_some(-9)) }
            fn kill(&mut self) -> Result<(), String> { self.kills.set(self.kills.get() + 1); Ok(()) }
            fn id(&self) -> u32 { 0 }
        }
        use tracing_subscriber::prelude::*;
        let dir = std::env::temp_dir().join(format!("taskq-stop-{}", uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = std::sync::Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
        let capture = coreq::telemetry::capture::Capture::new(store.clone(), "taskq", "fixture").unwrap();
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
            let log = dir.join("turn.log");
            std::fs::write(&log, "{\"type\":\"turn.completed\"}\n").unwrap();
            let exit = Rc::new(Cell::new(false)); let kills = Rc::new(Cell::new(0));
            let make = || Box::new(Pending { exit: exit.clone(), kills: kills.clone() }) as Box<dyn Proc>;
            let mut cli = CliTurn { proc: make(), log: log.clone(), pos: 0, start: 0, stopping: false, accepted: false, read_error: None, session: None, native: None, recovery: None };
            assert_eq!(cli.exited(),Ok(None)); assert_eq!(cli.exited(),Ok(None),"acknowledged stop awaiting exit is pending, not a failed observation");
            assert_eq!(cli.accepted(),Ok(false),"terminal alone is not exact-input acceptance");
            assert_eq!(kills.get(), 2, "terminal state must survive a pending stop");
            exit.set(true); assert_eq!(cli.exited(), Ok(Some(-9)));
            exit.set(false);
            let mut app = AppTurn { root: std::env::temp_dir(), history:AppHistory::default(), hand: Some(make()), log, text: String::new(), _input: None, binding:None, thread: String::new(), done: 0,
                rollout: None, running: || Ok(true), control:|_,_|Err("fixture native cancellation unavailable".into()), cancelled: false, queue_error: None, accepted: false };
            assert!(stop(&mut app).is_err());
            assert!(app.hand.is_some());
            exit.set(true); assert_eq!(app.exited(), Ok(Some(-9)));
            assert!(app.thread.is_empty(), "cancelled handshake must not open or queue an app task");
        });
        capture.flush().unwrap();
        let rows = store.export(0, 0, 100).unwrap();
        assert!(rows["events"].as_array().unwrap().iter().any(|r| r["observation"]["operation"] == "taskq.operation.runtime.stop"
            && r["observation"]["error"] == "stop-unconfirmed"));
        drop(capture); drop(store); std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: § 4 step 1 `gone` reason, #1034. Risk: a worker's last message publishes local paths or many lines.
    #[test]
    fn last_message_is_one_public_line_without_local_paths() {
        for (root, text, want) in [
            (r"C:\w\taskq", "See [service.rs](/C:/w/taskq/.worktrees/taskq-1/src/service.rs#L5).", "See [service.rs](.worktrees/taskq-1/src/service.rs#L5)."),
            (r"C:\w\taskq", r"Log: c:\W\taskq\.taskq\T1-2.log and C:\Users\me\AppData\x.txt", "Log: .taskq/T1-2.log and <path>"),
            ("/home/me/taskq", "`/home/me/taskq/src/a.rs`, file:///tmp/x/y, ~/.codex/auth.json\n\n  next line", "`src/a.rs`, <path>, <path> next line"),
            ("/home/me/taskq", "Kept: https://github.com/o/r/pull/1 and/or /help, root /home/me/taskq.", "Kept: https://github.com/o/r/pull/1 and/or /help, root .."),
        ] {
            assert_eq!(public_line(text, Path::new(root), 240), want);
        }
        let long = public_line(&"word ".repeat(100), Path::new("/r"), 224);
        assert_eq!((long.chars().count(), long.ends_with(" wor…")), (224, true));
    }

    // #2153 A1/A2: launcher/sandbox stderr before JSON events surfaces via public_line() in last_message(), and
    // setup/readiness login probes verify the active runtime account home.
    #[test]
    fn launcher_stderr_in_last_message_and_active_account_probe() {
        let dir = std::env::temp_dir().join(format!("taskq-2153-runtime-{}", uuid()));
        let root = dir.join("project");
        let log = attempt_log(&root, "worker", 2153, 1);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "{\"type\":\"taskq.rules\",\"phase\":\"start\"}\n{\"type\":\"taskq.account\",\"account\":\"work\"}\ntaskq: linux sandbox prerequisite /home/me/.codex/taskq-accounts/work/config.toml: No such file or directory (os error 2)\n").unwrap();
        assert_eq!(last_message(&log).as_deref(), Some("taskq: linux sandbox prerequisite <path>: No such file or directory (os error 2)"));
        std::fs::write(&log, "{\"type\":\"taskq.rules\",\"phase\":\"start\"}\n{\"type\":\"result\",\"result\":\"completed work\"}\ntrailing warning\n").unwrap();
        assert_eq!(last_message(&log).as_deref(), Some("completed work"));
        std::fs::write(&log, "{\"type\":\"taskq.rules\",\"phase\":\"start\"}\n{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n").unwrap();
        assert_eq!(last_message(&log), None);

        let install = dir.join("install");
        let base = dir.join("codex-home");
        let work = base.join(crate::runtime::accounts::HOMES).join("work");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(install.join(crate::runtime::accounts::FILE), r#"{"codex":[{"label":"default","enabled":false},{"label":"work","enabled":true}]}"#).unwrap();
        std::fs::write(base.join("auth.json"), r#"{"auth_mode":"chatgpt","last_refresh":"2026-10-01T00:00:00Z"}"#).unwrap();
        let bin = if cfg!(windows) {
            let p = dir.join("codex.cmd");
            std::fs::write(&p, "@echo off\r\nif \"%1\"==\"--version\" (echo codex-cli 0.159.3\r\nexit /b 0)\r\nif exist \"%CODEX_HOME%\\auth.json\" (echo Logged in using ChatGPT) else (echo Not logged in)\r\n").unwrap();
            p
        } else {
            let p = dir.join("codex");
            std::fs::write(&p, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"codex-cli 0.159.3\"; exit 0; fi\nif [ -f \"$CODEX_HOME/auth.json\" ]; then echo \"Logged in using ChatGPT\"; else echo \"Not logged in\"; fi\n").unwrap();
            #[cfg(unix)] std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            p
        };
        let row = agent_row_in(&install, "codex", &bin, Some(&base));
        assert_eq!(row["login"], "not logged in", "active work account has no auth.json yet: {row}");
        std::fs::write(work.join("auth.json"), r#"{"auth_mode":"chatgpt","last_refresh":"2026-11-01T00:00:00Z"}"#).unwrap();
        let row = agent_row_in(&install, "codex", &bin, Some(&base));
        assert_eq!((row["login"].as_str(), row["refreshed"].as_str()), (Some("logged in"), Some("2026-11-01T00:00:00Z")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: § 4 service pass, `agents` rows (R12), #1114. Risk: a runtime CLI whose child keeps the pipes open blocks the pass.
    #[test]
    fn probe_gives_up_on_time_when_a_grandchild_keeps_the_pipes() {
        let dir = std::env::temp_dir().join(format!("taskq-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let (p, text) = if cfg!(windows) { (dir.join(format!("{name}.cmd")), format!("@echo off\r\n{body}\r\n")) }
                else { (dir.join(name), format!("#!/bin/sh\n{body}\n")) };
            std::fs::write(&p, text).unwrap();
            #[cfg(unix)] std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            p
        };
        let (hang, fork) = if cfg!(windows) { ("ping -n 30 127.0.0.1", "start /b ping -n 30 127.0.0.1") } else { ("sleep 30", "sleep 30 &") };
        // the child hangs, then a killed child leaves its grandchild holding the pipes; or the child exits and leaves it
        for (name, body, want) in [("ok", "echo 1.2.3".to_string(), Some("1.2.3")), ("hang", format!("echo 1.2.3\n{hang}"), None),
            ("fork", format!("{fork}\necho 1.2.3"), None)] {
            let t = std::time::Instant::now();
            let got = probe_within("runtime.version", &script(name, &body.replace('\n', if cfg!(windows) { "\r\n" } else { "\n" })), &["--version"], std::time::Duration::from_secs(2));
            assert_eq!(got.as_ref().map(|(ok, out)| (*ok, out.trim())), want.map(|v| (true, v)), "{name}");
            assert!(t.elapsed() < std::time::Duration::from_secs(5), "{name}: {:?}", t.elapsed());
        }
        let began=std::time::Instant::now();
        let outcome=observe_until(began+std::time::Duration::from_millis(100),||
            probe_within("runtime.version",&script("budget",hang),&[],std::time::Duration::from_secs(10)));
        assert!(outcome.is_none());assert!(began.elapsed()<std::time::Duration::from_secs(2),"adapter ignored remaining pass time");
        assert!(observation_deadline(std::time::Duration::from_secs(1))>std::time::Instant::now()+std::time::Duration::from_millis(500),"observation deadline leaked into later work");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: incident runtime start. Risk: environment errors reported as taskq faults.
    #[test]
    fn incident_start_preserves_diagnostic_and_excludes_environment_errors() {
        use std::io::{Error, ErrorKind};
        for kind in [ErrorKind::NotFound, ErrorKind::PermissionDenied, ErrorKind::OutOfMemory, ErrorKind::WouldBlock,
            ErrorKind::TimedOut, ErrorKind::StorageFull, ErrorKind::ResourceBusy, ErrorKind::InvalidInput, ErrorKind::Unsupported] {
            let error = StartError::io(Error::new(kind, "unchanged diagnostic"), Some("claude"));
            assert!(!error.reportable, "{kind:?}");
            assert_eq!(error.to_string(), "claude: unchanged diagnostic");
            let expected=match kind {ErrorKind::PermissionDenied=>"permission",ErrorKind::TimedOut=>"timeout",ErrorKind::InvalidInput=>"invalid",_=>"other"};
            crate::profile::take_report();
            let result=crate::profile::measure_classified("spawn",||Err::<(),_>(error),StartError::class).unwrap_err();
            assert_eq!(result.to_string(),"claude: unchanged diagnostic");
            assert!(crate::profile::take_report().contains(&format!("{expected}:1")));
        }
        assert!(StartError::io(Error::other("internal launch fault"), None).reportable);
    }

    // Chain: § 5 taskq agents probe. Risk: unreadable output shown as a known state.
    #[test]
    fn agent_probes_read_version_and_login_or_say_unknown() {
        assert_eq!(version("2.1.296 (Claude Code)\n").as_deref(), Some("2.1.296"));
        assert_eq!(version("codex-cli 0.159.3").as_deref(), Some("0.159.3"));
        assert_eq!(version("error"), None);
        let ok = |s: &str| Some((true, s.to_string()));
        assert_eq!(login("claude", ok(r#"{"loggedIn": true, "email": "a@b.c"}"#)), ("logged in", Some("a@b.c".into())));
        assert_eq!(login("claude", ok(r#"{"loggedIn": false}"#)), ("not logged in", None));
        assert_eq!(login("claude", ok("Unknown command: auth")), ("unknown", None));
        assert_eq!(login("codex", ok("Logged in using ChatGPT\n")), ("logged in", Some("ChatGPT".into())));
        assert_eq!(login("codex", ok("Logged in using an API key - sk-ab***cd")), ("logged in", Some("an API key".into())));
        assert_eq!(login("codex", Some((false, "Not logged in\n".into()))), ("not logged in", None));
        assert_eq!(login("codex", None), ("unknown", None));
        let row = |l: &str, a: serde_json::Value| serde_json::json!({"runtime": "codex", "version": "0.1", "login": l, "account": a});
        assert_eq!(agent_text(&row("logged in", "ChatGPT".into())), "codex 0.1 · ChatGPT");
        assert_eq!(agent_text(&row("not installed", serde_json::Value::Null)), "codex · not installed");
        assert_eq!(agent_text(&row("unknown", serde_json::Value::Null)), "codex 0.1 · login unknown");
        // #1429: kind and last refresh, never a token value; unreadable reads unknown
        assert_eq!(kind_refresh("claude", Some(r#"{"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}"#), None), (Some("claude.ai max".into()), None));
        assert_eq!(kind_refresh("codex", None, Some(r#"{"auth_mode": "chatgpt", "tokens": {"refresh_token": "invented"}, "last_refresh": "2026-10-19T18:14:41Z"}"#)), (Some("chatgpt".into()), Some("2026-10-19T18:14:41Z".into())));
        assert_eq!(kind_refresh("codex", None, Some("{")), (None, None));
        let rows = serde_json::json!([{"runtime": "claude", "version": "2.1", "login": "logged in", "account": "a@b.c", "kind": "claude.ai max", "refreshed": null},
            {"runtime": "codex", "version": "0.1", "login": "not logged in", "account": null, "kind": "chatgpt", "refreshed": "2026-10-19T18:14:41Z"}]);
        assert_eq!(login_lines(&rows), [
            "Login: claude 2.1 · a@b.c · kind claude.ai max · refreshed unknown",
            "Login: codex 0.1 · not logged in · kind chatgpt · refreshed 2026-10-19T18:14:41Z",
            "Next: codex admits no task; run `codex login --device-auth` with the PM (docs/relogin.md)"]);
        assert_eq!(login_lines(&serde_json::Value::Null), ["Login: unknown (no runtime probe yet)"]);
        // `taskq agents` prints kind and refresh too; a missing value reads unknown
        assert_eq!(agents_line(&serde_json::json!({"runtime": "codex", "path": "/bin/codex", "version": "0.1", "login": "logged in", "account": "a@b.c", "kind": "chatgpt", "refreshed": "2026-10-19T18:14:41Z"})),
            "codex  /bin/codex  0.1  logged in as a@b.c  kind chatgpt  refreshed 2026-10-19T18:14:41Z");
        assert_eq!(agents_line(&serde_json::json!({"runtime": "claude", "path": null, "login": "not installed"})), "claude  not installed  -  not installed  kind unknown  refreshed unknown");
        // #1446 A2: a rejected login shows its reason
        let off = serde_json::json!({"runtime": "codex", "path": "/bin/codex", "version": "0.1", "login": "not logged in", "account": "a@b.c", "reason": "refresh token rejected"});
        assert_eq!(agent_text(&off), "codex 0.1 · not logged in (refresh token rejected)");
        assert!(agents_line(&off).contains("  not logged in (refresh token rejected)  "), "{}", agents_line(&off));
        // #1661 A2: Settings states stay distinct; a method is never shown as the account
        let view = |r: serde_json::Value| { let v = agent_view(&r); (v["state"].as_str().unwrap().to_string(), v["account"].clone(), v["method"].clone()) };
        assert_eq!(view(off.clone()), ("rejected".into(), "a@b.c".into(), serde_json::Value::Null));
        assert_eq!(view(row("logged in", "ChatGPT".into())), ("signed-in".into(), serde_json::Value::Null, "ChatGPT".into()));
        assert_eq!(view(serde_json::json!({"login": "logged in", "account": "a@b.c", "kind": "claude.ai max", "expires": 1})), ("expired".into(), "a@b.c".into(), "claude.ai max".into()));
        assert_eq!(view(serde_json::json!({"login": "logged in", "expires": now() + 3600})).0, "signed-in");
        assert_eq!(view(serde_json::json!({"login": "not logged in"})).0, "sign-in-required");
        assert_eq!(view(serde_json::json!({"login": "unknown", "path": "/bin/codex"})).0, "check-failed");
        assert_eq!(view(serde_json::json!({"login": "not installed", "path": null})).0, "not-installed");
        assert_eq!(view(serde_json::json!({})).0, "unknown");
        // an old last refresh is not an expiry; only a needed sign-in names the native route
        let fresh = agent_view(&serde_json::json!({"runtime": "codex", "login": "logged in", "refreshed": "2020-01-01T00:00:00Z"}));
        assert_eq!((fresh["state"].as_str(), fresh["next"].as_str()), (Some("signed-in"), None));
        assert!(agent_view(&off)["next"].as_str().is_some_and(|n| n.contains("codex login")));
    }

    // Chain: #1446 A1/A3. Risk: Codex's dead refresh token reads logged in and burns retries, worker output that
    // quotes the error marks the login off, or the mark survives a re-login.
    #[test]
    fn rejected_codex_login_is_read_from_recorded_lines_until_relogin() {
        let dir = std::env::temp_dir().join(format!("taskq-auth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("T1-1.log");
        let check = |text: &str| { std::fs::write(&log, text).unwrap(); auth_rejected(&log) };
        // recorded on hermes-fra1, T1420-6308286799.log (#1424)
        assert_eq!(check("2026-10-21T08:00:00.000000Z ERROR codex_login::auth::manager: Failed to refresh token: Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.\n").as_deref(), Some("refresh token rejected"));
        assert_eq!(check("2026-10-21T08:00:00.000000Z ERROR codex_login::auth::manager: Failed to refresh token status=401 Unauthorized detail=TokenErrorDetail { error_code: Some(\"refresh_token_reused\") }\n").as_deref(), Some("refresh token rejected"));
        assert_eq!(check("2026-10-21T08:00:01.000000Z ERROR codex_models_manager::manager: failed to refresh available models: unexpected status 401 Unauthorized: Provided authentication token is expired. Please try signing in again.\n").as_deref(), Some("401 Unauthorized"));
        assert_eq!(check("{\"type\":\"turn.failed\",\"error\":{\"message\":\"unexpected status 401 Unauthorized: Provided authentication token is expired. Please try signing in again., auth error: 401, auth error code: token_expired\"}}\n").as_deref(), Some("401 Unauthorized"));
        // worker output quoting the same text is not Codex's own error
        assert_eq!(check("{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"ERROR codex_login::auth::manager: Failed to refresh token: refresh token was already used\"}}\n"), None);
        assert_eq!(check("{\"type\":\"error\",\"message\":\"stream disconnected\"}\nworker said 401 Unauthorized\n"), None);
        assert_eq!(auth_rejected(&dir.join("missing.log")), None);
        // the mark holds until auth.json's last_refresh changes (a re-login)
        mark_login_off(&dir, "codex", "refresh token rejected").unwrap();
        let at = codex_refreshed(&dir);
        assert_eq!(login_off(&dir, "codex", at.as_deref()).as_deref(), Some("refresh token rejected"));
        assert_eq!(login_off(&dir, "codex", Some("2099-01-01T00:00:00Z")), None);
        assert_eq!(login_off(&dir, "claude", None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: CLI input boundary -> native stream -> acceptance. Risk: an old turn or a process exit consumes new input.
    #[test]
    fn cli_acceptance_uses_only_native_events_after_its_input_boundary() {
        struct Exited;
        impl Proc for Exited {
            fn exited(&mut self) -> Result<Option<i32>,String> { Ok(Some(0)) }
            fn kill(&mut self) -> Result<(),String> { Ok(()) }
            fn id(&self) -> u32 { 0 }
        }
        let dir = std::env::temp_dir().join(format!("taskq-cli-accept-{}",uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("turn.log");
        let old="{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n";
        for native in ["", "{\"type\":\"turn.started\"}\n", "{\"type\":\"assistant\",\"message\":{}}\n"] {
            std::fs::write(&path,format!("{old}{native}{{\"type\":\"turn.completed\"}}\n")).unwrap();
            let mut turn=CliTurn { proc:Box::new(Exited),log:path.clone(),pos:old.len() as u64,start:old.len() as u64,stopping:false,accepted:false,read_error:None,session:None,native:None,recovery:None };
            assert_eq!(turn.accepted(),Ok(false));
            assert_eq!(turn.exited(),Ok(Some(0)));
            assert_eq!(turn.accepted(),Ok(!native.is_empty()));
            assert_eq!(turn.exited(),Ok(Some(0)));
            assert_eq!(turn.accepted(),Ok(!native.is_empty()),"proof survives repeated polls");
        }
        let mut truncated=CliTurn {proc:Box::new(Exited),log:path.clone(),pos:1000,start:1000,stopping:false,accepted:false,read_error:None,session:None,native:None,recovery:None};
        assert_eq!(truncated.exited(),Ok(Some(0)),"native exit remains independently observable");
        assert!(truncated.accepted().unwrap_err().contains("truncated"));
        assert!(truncated.input_offset().is_err(),"a lost native input boundary cannot be handed over");
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: § 8 codex-app turn end. Risk: a turn ends early or never ends.
    #[test]
    fn app_turn_keeps_unreadable_completion_unknown() {
        let p = std::env::temp_dir().join(format!("taskq-rollout-unknown-{}.jsonl", std::process::id()));
        let mut t = AppTurn { root: p.parent().unwrap().canonicalize().unwrap(), history:AppHistory::default(), hand: None, log: p.clone(), text: String::new(), _input: None, binding:None, thread: "th".into(), done: 1, rollout: Some(p.clone()), running: || Ok(true), control:|_,_|Err("fixture native cancellation unavailable".into()), cancelled: false, queue_error: None, accepted: false };
        let _ = std::fs::remove_file(&p);
        assert!(t.exited().is_err(), "missing history must retain an explicit unknown reason");
        std::fs::write(&p, "{\"type\":\"event_msg\"").unwrap();
        assert!(t.exited().is_err(), "incomplete history cannot prove the turn is still healthy");
        std::fs::write(&p, "").unwrap();
        assert!(t.exited().is_err(), "history shorter than the baseline cannot reset the turn count");
        t.running = || Err("probe timed out".into());
        assert_eq!(t.exited(), Err("probe timed out".into()), "failed app query cannot become an exit");
        for windows in [false, true] {
            assert!(app_presence("unreadable", windows).is_err());
            assert_eq!(app_presence("", windows), Ok(false));
        }
        assert_eq!(app_presence("ASN:0x0-0x12-\"Codex\":\n", false), Ok(true));
        assert_eq!(app_presence("\"Codex.exe\",\"42\",\"OpenAI.Codex_1\"\n", true), Ok(true));
        assert_eq!(app_presence("\"other.exe\",\"43\",\"Other\"\n", true), Ok(false));
        assert!(app_presence("\"other.exe\",\"unknown\",\"Other\"\n", true).is_err());
        assert!(app_presence("ASN:0xbad:", false).is_err());
        std::fs::remove_file(p).unwrap();
    }

    // Chain: § 8 codex-app turn end. Risk: a turn ends early or never ends.
    // D2/#2159: a previous aborted identical input cannot acknowledge or finish a new owner submission.
    #[test]
    fn app_owner_input_floor_excludes_an_old_aborted_identical_reply() {
        let root=std::env::temp_dir().join(format!("taskq-app-floor-{}",uuid()));std::fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();let path=root.join("native.jsonl");
        let events=[serde_json::json!({"type":"session_meta","payload":{"id":"thread","cwd":root}}),serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"old"}}),serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"same reply","turn_id":"old"}}),serde_json::json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"old"}})];
        let old=events.iter().map(|v|format!("{v}\n")).collect::<String>();std::fs::write(&path,&old).unwrap();
        let mut history=AppHistory{floor:old.len() as u64,..AppHistory::default()};assert_eq!(history.read(&path,0,"same reply",&root,"thread"),Ok((false,false)));
        let mut next=old;for payload in [serde_json::json!({"type":"task_started","turn_id":"new"}),serde_json::json!({"type":"user_message","message":"same reply","turn_id":"new"}),serde_json::json!({"type":"task_complete","turn_id":"new"})] {next+=&format!("{}\n",serde_json::json!({"type":"event_msg","payload":payload}));}std::fs::write(&path,next).unwrap();
        assert_eq!(history.read(&path,0,"same reply",&root,"thread"),Ok((true,true)));assert_eq!(history.owned.as_deref(),Some("new"));std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_turn_requires_its_input_and_matching_native_completion() {
        let p = std::env::temp_dir().join(format!("taskq-rollout-{}.jsonl", std::process::id()));
        let event = |kind: &str, id: &str, message: &str| serde_json::json!({"type":"event_msg", "payload":{"type":kind,"turn_id":id,"message":message}}).to_string() + "\n";
        let meta=serde_json::json!({"type":"session_meta","payload":{"id":"th","cwd":p.parent().unwrap().canonicalize().unwrap()}});
        let mut history = meta.to_string()+"\n"+&event("task_complete", "old", "");
        std::fs::write(&p, &history).unwrap();
        let mut t = AppTurn { root: p.parent().unwrap().canonicalize().unwrap(), history:AppHistory::default(), hand: None, log: p.clone(), text: "owned input".into(), _input: None, binding:None, thread: "th".into(), done: 1,
            rollout: Some(p.clone()), running: || Ok(true), control:|_,_|Err("fixture native cancellation unavailable".into()), cancelled: false, queue_error: None, accepted: false };
        assert_eq!(t.exited(), Ok(None));
        assert_eq!(t.accepted(),Ok(false));
        for (id, input, completion, expected) in [("other", "unrelated input", "other", false), ("owned", "owned input", "wrong", false)] {
            history += &event("task_started", id, ""); history += &event("user_message", id, input); history += &event("task_complete", completion, "");
            std::fs::write(&p, &history).unwrap();
            assert_eq!(t.exited(), Ok(expected.then_some(0)), "unrelated completion cannot release ownership");
            assert_eq!(t.accepted(),Ok(id == "owned"),"acceptance belongs to the exact input and native turn");
        }
        t.done = 0; t.history=AppHistory::default(); // cold observer has no volatile completion baseline
        t.queue_error = Some("submission response lost".into());
        assert_eq!(t.exited(), Ok(None), "native input acceptance resolves the lost submission response");
        assert!(stop(&mut t).unwrap_err().contains("native cancellation unavailable"), "a live App turn needs an explicit unsupported-control reason");
        assert_eq!(t.exited(), Ok(None), "refused cancellation retains the exact execution observer");
        t.control=|thread,turn|{assert_eq!((thread,turn),("th","owned"));Ok(())};
        assert_eq!(t.kill(),Ok(()));
        assert_eq!(t.exited(),Ok(None),"acknowledged interrupt is not native terminal evidence");
        history += &event("task_complete", "owned", "");
        std::fs::write(&p, &history).unwrap();
        assert_eq!(t.exited(), Ok(Some(0)), "later exact proof resolves an uncertain submission without replay");
        assert_eq!(stop(&mut t), Ok(0), "observed native completion resolves stop even when cancellation is unsupported");
        t.running = || Ok(false);
        assert!(t.exited().unwrap_err().contains("termination unconfirmed"));
        t.running = || Ok(true);
        for change in ["root","thread","copied","missing","duplicate"] {
            let mut wrong=meta.clone();
            match change {
                "root"=>wrong["payload"]["cwd"]="/nonexistent/taskq".into(),
                "thread"=>wrong["payload"]["id"]="foreign".into(),
                "copied"=>wrong["payload"]["forked_from_id"]="original".into(),
                _=>{},
            }
            let (_,events)=history.split_once('\n').unwrap();
            let text=match change {"missing"=>events.to_owned(),"duplicate"=>meta.to_string()+"\n"+&history,_=>wrong.to_string()+"\n"+events};
            std::fs::write(&p,text).unwrap();
            t.history=AppHistory::default();
            assert!(t.exited().is_err(),"{change} cannot prove native completion");
        }
        // Installed desktop emits UserMessage items instead of legacy user_message events.
        let native=serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":"th","turn_id":"owned",
            "item":{"type":"UserMessage","id":"native-input","content":[{"type":"text","text":"owned input","text_elements":[]}]}}});
        for change in ["valid","transport-lf","extra-lf","space","aborted","foreign-abort","conflicting-terminal","thread","turn","missing-turn","content","duplicate","duplicate-lf"] {
            let mut item=native.clone();
            match change {
                "thread"=>item["payload"]["thread_id"]="foreign".into(),
                "turn"=>item["payload"]["turn_id"]="foreign".into(),
                "missing-turn"=>{item["payload"].as_object_mut().unwrap().remove("turn_id");},
                "content"=>item["payload"]["item"]["content"][0]["text"]="unrelated".into(),
                "transport-lf"|"duplicate-lf"=>item["payload"]["item"]["content"][0]["text"]="owned input\n".into(),
                "extra-lf"=>item["payload"]["item"]["content"][0]["text"]="owned input\n\n".into(),
                "space"=>item["payload"]["item"]["content"][0]["text"]="owned input ".into(),
                _=>{},
            }
            let mut text=meta.to_string()+"\n"+&event("task_started","owned","")+&item.to_string()+"\n";
            if matches!(change,"duplicate"|"duplicate-lf") {text+=&(native.to_string()+"\n");}
            if matches!(change,"aborted"|"foreign-abort"|"conflicting-terminal") {text+=&event("turn_aborted",if change=="foreign-abort" {"foreign"} else {"owned"},"");}
            if change!="aborted" && change!="foreign-abort" {text+=&event("task_complete","owned","");}
            std::fs::write(&p,text).unwrap();t.history=AppHistory::default();t.accepted=false;
            let outcome=t.exited();
            match change {
                "valid"|"transport-lf"=>{assert_eq!(outcome,Ok(Some(0)));assert_eq!(t.accepted(),Ok(true));},
                "aborted"=>{assert_eq!(outcome,Ok(Some(-1)));assert_eq!(t.accepted(),Ok(true));},
                "foreign-abort"=>{assert_eq!(outcome,Ok(None));assert_eq!(t.accepted(),Ok(true));},
                "content"|"extra-lf"|"space"=>{assert_eq!(outcome,Ok(None));assert_eq!(t.accepted(),Ok(false));},
                _=>assert!(outcome.is_err(),"{change} cannot establish input acceptance"),
            }
        }
        t.thread = "x & calc".into();
        assert!(t.queue().is_err());
        std::fs::remove_file(p).unwrap();
    }

    // Chain: native queued input -> lost-response cold observer -> exact native completion.
    // Read-only qualification of an already-completed dedicated probe; no dispatch or control.
    #[test]
    #[ignore = "owned native probe; TASKQ_APP_READBACK selects captured lifecycle evidence"]
    fn native_app_lost_reply_readback() {
        let evidence=PathBuf::from(std::env::var("TASKQ_APP_READBACK").expect("TASKQ_APP_READBACK"));
        let read=|file:&str|serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(evidence.join(file)).unwrap()).unwrap();
        let manifest=read("manifest.json");
        for (file,code) in [("queue-command.json",0),("interrupt-queue.json",-1)] {
        let queued=read(file);
        let root=PathBuf::from(manifest["root"].as_str().unwrap()).canonicalize().unwrap();
        let thread=manifest["thread"].as_str().unwrap().to_owned();let text=queued["input"].as_str().unwrap().to_owned();
        let mut turn=AppTurn {root,history:AppHistory::default(),hand:None,log:evidence.join("handshake.jsonl"),text,_input:None,binding:None,
            rollout:rollout(&thread),thread,done:0,running:app_running,control:app_cancel,cancelled:false,queue_error:Some("fixture: submission response lost".into()),accepted:false};
        assert_eq!(turn.exited(),Ok(Some(code)));assert_eq!(turn.accepted(),Ok(true));assert!(turn.queue_error.is_none());
        assert_eq!(turn.exited(),Ok(Some(code)),"repeated observation cannot replay the submitted input");
        println!("native App exact input and terminal {code} recovered without submission: {}",turn.thread);
        }
    }

    // Chain: owned native input -> production IPC cancellation -> matching native abort.
    #[cfg(any(target_os="macos",windows))]
    #[test]
    #[ignore = "dedicated active native probe; TASKQ_APP_READBACK selects its evidence"]
    fn native_app_cancels_exact_owned_turn() {
        let evidence=PathBuf::from(std::env::var("TASKQ_APP_READBACK").expect("TASKQ_APP_READBACK"));
        let read=|file:&str|serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(evidence.join(file)).unwrap()).unwrap();
        let manifest=read("manifest.json");let queued=read("source-cancel-queue.json");
        let root=PathBuf::from(manifest["root"].as_str().unwrap()).canonicalize().unwrap();
        let thread=manifest["thread"].as_str().unwrap().to_owned();let text=queued["input"].as_str().unwrap().to_owned();
        let mut turn=AppTurn {root,history:AppHistory::default(),hand:None,log:evidence.join("handshake.jsonl"),text,_input:None,binding:None,
            rollout:rollout(&thread),thread,done:0,running:app_running,control:app_cancel,cancelled:false,queue_error:Some("fixture: submission response lost".into()),accepted:false};
        let until=std::time::Instant::now()+std::time::Duration::from_secs(30);
        while !turn.accepted {
            let observed=turn.exited();assert!(!matches!(observed,Ok(Some(_))),"native probe must still be active before cancellation");
            assert!(std::time::Instant::now()<until,"input acceptance unavailable: {observed:?}");std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let owned=turn.history.owned.clone().unwrap();turn.kill().expect("exact owner/turn interrupt request");
        std::fs::write(evidence.join("source-cancel-receipt.json"),serde_json::json!({"thread":turn.thread,"turn":owned,"request_acknowledged":true}).to_string()).unwrap();
        loop {
            let observed=turn.exited();
            if let Ok(Some(code))=observed {assert_eq!(code,-1);break;}
            assert!(std::time::Instant::now()<until,"interrupt request is not terminal: {observed:?}");std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(turn.history.owned,Some(owned.clone()));assert_eq!(turn.accepted(),Ok(true));
        println!("production App IPC cancelled exact native turn {owned}; native abort observed; no resubmission");
    }

    // Chain: long native App history -> bounded cold scan -> exact completion -> appended observations.
    // Risk: a total-size ceiling stalls long sessions, or a partial scan/rewritten history releases work early.
    #[test]
    fn app_history_progresses_in_bounded_passes_without_early_completion() {
        let root=std::env::temp_dir().join(format!("taskq-app-history-{}",uuid()));std::fs::create_dir_all(&root).unwrap();
        let root=root.canonicalize().unwrap();let path=root.join("rollout.jsonl");
        let event=|kind:&str,message:&str|serde_json::json!({"type":"event_msg","payload":{"type":kind,"turn_id":"owned","message":message}}).to_string()+"\n";
        let header=serde_json::json!({"type":"session_meta","payload":{"id":"thread","cwd":root}}).to_string()+"\n";
        let prefix=header+&event("task_started","")+&event("user_message","exact input")+&event("task_complete","");
        for duplicate in [false,true] {
            let mut file=File::create(&path).unwrap();file.write_all(prefix.as_bytes()).unwrap();
            let padding=serde_json::json!({"type":"response_item","payload":"x".repeat(1024*1024)}).to_string()+"\n";
            for _ in 0..18 {file.write_all(padding.as_bytes()).unwrap();}
            if duplicate {file.write_all(event("task_started","").as_bytes()).unwrap();file.write_all(event("user_message","exact input").as_bytes()).unwrap();}
            file.sync_all().unwrap();drop(file);
            let baseline_began=std::time::Instant::now();
            assert_eq!(completes(&path).unwrap(),1);
            let baseline_time=baseline_began.elapsed();
            let mut history=AppHistory::default();let mut passes=0;
            loop {
                passes+=1;let before=history.reader.offset;
                let result=history.read(&path,0,"exact input",&root,"thread");
                assert!(history.reader.offset-before<=8*1024*1024);
                match result {
                    Err(e) if e.contains("scan pending")=>{assert!(history.reader.offset>before);assert!(passes<5);},
                    Err(e)=>{assert!(duplicate && e.contains("multiple native messages"),"{e}");break;},
                    Ok(outcome)=>{assert!(!duplicate);assert_eq!(outcome,(true,true));assert!(passes>=3);break;},
                }
            }
            if duplicate {continue;}
            let end=history.reader.offset;
            assert_eq!(history.read(&path,0,"exact input",&root,"thread"),Ok((true,true)));
            assert_eq!(history.reader.offset,end);
            let idle=std::time::Instant::now();
            for _ in 0..100 {assert_eq!(history.read(&path,0,"exact input",&root,"thread"),Ok((true,true)));}
            eprintln!("App native history: {} bytes, {passes} bounded passes; baseline {:?}; 100 unchanged reads {:?}",end,baseline_time,idle.elapsed());
            let extra=event("agent_message","later native diagnostic");
            let split=extra.len()/2;
            File::options().append(true).open(&path).unwrap().write_all(&extra.as_bytes()[..split]).unwrap();
            assert!(history.read(&path,0,"exact input",&root,"thread").unwrap_err().contains("incomplete"));
            assert_eq!(history.reader.offset,end,"a partial record is not consumed");
            File::options().append(true).open(&path).unwrap().write_all(&extra.as_bytes()[split..]).unwrap();
            assert_eq!(history.read(&path,0,"exact input",&root,"thread"),Ok((true,true)));
            assert_eq!(history.reader.offset,end+extra.len() as u64);
            // A replacement with the same bytes must not inherit the old cursor.
            let replacement=root.join("replacement.jsonl");std::fs::copy(&path,&replacement).unwrap();
            std::fs::remove_file(&path).unwrap();std::fs::rename(replacement,&path).unwrap();
            assert!(history.read(&path,0,"exact input",&root,"thread").is_err());
        }
        std::fs::write(&path,&prefix).unwrap();let mut history=AppHistory::default();
        assert_eq!(history.read(&path,0,"exact input",&root,"thread"),Ok((true,true)));
        std::fs::write(&path,"{}\n").unwrap();
        assert!(history.read(&path,0,"exact input",&root,"thread").unwrap_err().contains("truncated"));
        std::fs::write(&path,&prefix).unwrap();let mut history=AppHistory::default();history.read(&path,0,"exact input",&root,"thread").unwrap();
        let changed=prefix.replace("task_complete","task_conflict")+&event("agent_message","growth cannot hide a rewritten boundary");
        std::fs::write(&path,changed).unwrap();
        assert!(history.read(&path,0,"exact input",&root,"thread").unwrap_err().contains("boundary changed"));
        std::fs::write(&path,"x".repeat(8*1024*1024+1)).unwrap();
        assert!(AppHistory::default().read(&path,0,"exact input",&root,"thread").unwrap_err().contains("exceeds 8 MiB"));
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: live Claude worker -> transcript mtime bumped with no new bytes (#1588) -> history read.
    // Risk: an mtime-only touch reports Run state unknown, or a same-length rewrite passes as unchanged.
    #[test]
    fn native_history_mtime_touch_keeps_observation_and_same_length_rewrite_fails() {
        let root=std::env::temp_dir().join(format!("taskq-history-touch-{}",uuid()));std::fs::create_dir_all(&root).unwrap();
        let path=root.join("transcript.jsonl");
        let text="{\"type\":\"user\",\"n\":1}\n{\"type\":\"assistant\",\"n\":2}\n";
        std::fs::write(&path,text).unwrap();
        let mut history=NativeHistory::default();let read=|h:&mut NativeHistory|h.read(&path,|_|Ok(()));
        assert_eq!(read(&mut history),Ok(true));
        let touch=|secs|File::options().write(true).open(&path).unwrap()
            .set_modified(std::time::SystemTime::now()+std::time::Duration::from_secs(secs)).unwrap();
        touch(60);
        assert_eq!(read(&mut history),Ok(true),"an mtime-only change keeps the execution observed");
        let mut file=File::options().write(true).open(&path).unwrap();file.write_all(b"{\"type\":\"system\"").unwrap();drop(file);
        assert_eq!(std::fs::metadata(&path).unwrap().len(),text.len() as u64);touch(120);
        assert!(read(&mut history).unwrap_err().contains("rewritten"),"changed bytes of the same length fail closed");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: live Claude worker -> EnterWorktree moves its transcript to the worktree's project folder (#1627) -> history read twice.
    // Risk: the moved transcript reads as another history, or a different file at the new path passes as the original.
    #[test]
    fn native_history_follows_a_moved_transcript_and_rejects_a_replacement() {
        let root=std::env::temp_dir().join(format!("taskq-history-move-{}",uuid()));
        for dir in ["root","worktree"] {std::fs::create_dir_all(root.join(dir)).unwrap();}
        let (old,new)=(root.join("root/transcript.jsonl"),root.join("worktree/transcript.jsonl"));
        std::fs::write(&old,"{\"type\":\"user\",\"n\":1}\n").unwrap();
        let mut history=NativeHistory::default();let mut seen=Vec::new();
        assert_eq!(history.read(&old,|v|{seen.push(v["n"].as_u64());Ok(())}),Ok(true));
        std::fs::rename(&old,&new).unwrap();
        File::options().append(true).open(&new).unwrap().write_all(b"{\"type\":\"assistant\",\"n\":2}\n").unwrap();
        assert_eq!(history.read(&new,|v|{seen.push(v["n"].as_u64());Ok(())}),Ok(true),"a moved transcript keeps the execution observed");
        assert_eq!(seen,[Some(1),Some(2)],"the cursor continues without replay");
        // The replacement exists before the original goes, so no file system can reuse its identity.
        std::fs::write(&old,"{\"type\":\"user\",\"n\":1}\n{\"type\":\"assistant\",\"n\":2}\n").unwrap();
        std::fs::remove_file(&new).unwrap();std::fs::rename(&old,&new).unwrap();
        let replaced=history.read(&new,|_|Ok(())).unwrap_err();
        assert!(replaced.contains("replaced")||replaced.contains("identity changed"),"another file at the moved path fails closed: {replaced}");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: board intent -> native input -> cold observer. Risk: restart substitutes input, replays work or drops its lock.
    #[test]
    fn cold_input_recovery_preserves_the_exact_execution() {
        let root=std::env::temp_dir().join(format!("taskq-cold-input-{}",uuid()));
        std::fs::create_dir_all(&root).unwrap();
        for rt in ["claude","codex-app"] {
            let l=Launch {files: String::new(), execution:Some(44),task:7,title:String::new(),attempt:42,role:"worker".into(),rt:rt.into(),session:uuid(),url:String::new(),resume:Some("continue".into())};
            let x=crate::core::Execution {id:44,attempt:42,through:43,host:"mac".into(),runtime:rt.into(),accepted:false,release:None,usage_hold:None};
            let log=log_path(&root,&l);
            let (path,lock)=turn_input(&root,"original task",Some(&l)).unwrap();
            assert_eq!(path,root.canonicalize().unwrap().join(".taskq/inputs/execution-7-42-44.txt"));
            assert!(turn_input(&root,"replacement task",Some(&l)).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(),"original task");
            let input=if rt=="claude" {claude_reference(&path)} else {app_reference(&path)}.unwrap();
            let binding=bind_input(&log,&l,&path,&input,serde_json::json!({"thread":"native-thread","session":l.session})).unwrap();
            drop(lock);
            let mut large_log=File::options().append(true).open(&log).unwrap();
            let row=serde_json::json!({"type":"fixture","padding":"x".repeat(1024*1024)}).to_string()+"\n";
            for _ in 0..10 {large_log.write_all(row.as_bytes()).unwrap();}
            drop(large_log);
            let mut observer=recover(&root,&root.join("install"),7,&x).unwrap();
            assert_eq!(observer.id(),0);
            assert!(!matches!(observer.accepted(),Ok(true)),"binding is not native acceptance");
            let file=File::options().read(true).write(true).open(&path).unwrap();
            assert!(matches!(file.try_lock(),Err(std::fs::TryLockError::WouldBlock)));
            drop(observer);
            file.try_lock().unwrap();file.unlock().unwrap();
            for case in ["foreign-task","foreign-execution","foreign-runtime","input","path","legacy","missing","partial","oversized","ambiguous","released"] {
                let mut b=binding.clone();
                let mut execution=x.clone();
                let reviewer=attempt_log(&root,"reviewer",7,42);
                match case {
                    "foreign-task"=>b["task"]=8.into(),
                    "foreign-execution"=>b["execution"]=45.into(),
                    "foreign-runtime"=>b["runtime"]="other".into(),
                    "input"=>b["input"]="unrelated input".into(),
                    "path"=>b["path"]="/unrelated/input.txt".into(),
                    "legacy"=>{std::fs::rename(&path,root.join("legacy.txt")).unwrap();},
                    "ambiguous"=>std::fs::write(&reviewer,"{}").unwrap(),
                    "released"=>execution.release=Some(crate::core::Release::Exit),
                    _=>{},
                }
                std::fs::write(&log,match case {"missing"=>String::new(),"partial"=>"{incomplete".into(),"oversized"=>"x".repeat(8*1024*1024+1),_=>b.to_string()+"\n"}).unwrap();
                assert!(recover(&root,&root.join("install"),7,&execution).is_err(),"{rt}: {case}");
                if case=="legacy" {std::fs::rename(root.join("legacy.txt"),&path).unwrap();}
                if case=="ambiguous" {std::fs::remove_file(reviewer).unwrap();}
            }
            std::fs::remove_file(path).unwrap();std::fs::remove_file(log).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    // Risk: a refused launch blocks its task forever, or a doubtful record releases a started job (#1557).
    #[test]
    fn saved_launch_records_reconcile_only_a_refusal() {
        // #1557 A4: records written before the status field still reconcile; any doubt stays unknown.
        let root=std::env::temp_dir().join(format!("taskq-refused-{}",uuid()));
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let x=crate::core::Execution {id:44,attempt:42,through:0,host:"mac".into(),runtime:"claude".into(),accepted:false,release:None,usage_hold:None};
        let bind=|id:u64|serde_json::json!({"type":"taskq.input","execution":id}).to_string();
        let launch=|stdout:&str,stderr:&str,status:Option<serde_json::Value>|{
            let mut v=serde_json::json!({"type":"claude.launch","stdout":stdout,"stderr":stderr});
            if let Some(s)=status {v["status"]=s;}
            v.to_string()
        };
        let refused=launch("","Workspace not trusted",None);
        for (case,lines,want) in [
            ("legacy refusal",vec![bind(44),refused.clone()],true),
            ("exit 1",vec![bind(44),launch("","Workspace not trusted",Some(1.into()))],true),
            ("signal",vec![bind(44),launch("","Workspace not trusted",Some(serde_json::Value::Null))],false),
            ("exit 0",vec![bind(44),launch("","warning",Some(0.into()))],false),
            ("id printed",vec![bind(44),launch("backgrounded · c37eb75a\n","failed",Some(1.into()))],false),
            ("empty stderr",vec![bind(44),launch("","",None)],false),
            ("other execution",vec![bind(43),refused.clone()],false),
            ("later binding",vec![bind(44),bind(45),refused.clone()],false),
            ("no record",vec![bind(44)],false),
        ] {
            std::fs::write(attempt_log(&root,"worker",7,42),lines.join("\n")).unwrap();
            assert_eq!(refused_launch(&root,7,&x).is_some(),want,"{case}");
        }
        std::fs::write(attempt_log(&root,"worker",7,42),[bind(44),refused].join("\n")).unwrap();
        assert!(refused_launch(&root,7,&crate::core::Execution {accepted:true,..x.clone()}).is_none(),"accepted input never reads no-start");
        assert!(refused_launch(&root,7,&crate::core::Execution {runtime:"codex-app".into(),..x.clone()}).is_none());
        std::fs::write(attempt_log(&root,"reviewer",7,42),"").unwrap();
        assert!(refused_launch(&root,7,&x).is_none(),"ambiguous log stays unknown");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: exact execution input binding -> uncertain native launch -> retained observation and input lock.
    // Risk: a timeout or unreadable launch response drops the only recovery token and invites resubmission.
    #[test]
    fn uncertain_background_launch_retains_its_bound_input() {
        #[cfg(unix)] use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)] use std::os::windows::process::ExitStatusExt;
        let root=std::env::temp_dir().join(format!("taskq-uncertain-input-{}",uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let candidate="c37eb75a";
        // A2: a timeout, a signal-killed launcher or a printed ID may have started a job; A1: a refusal did not (#1557, #1555).
        let cases:&[&str]=if cfg!(unix) {&["timeout","response","exit-id","killed","log","copy","no-start","refused","untrusted"]} else {&["timeout","response","exit-id","log","copy","no-start","refused","untrusted"]};
        for &case in cases {
            let log=root.join(format!("{case}.log"));
            let mut l=Launch {files: String::new(), execution:Some(44),task:7,title:String::new(),attempt:42,role:"worker".into(),rt:"claude".into(),session:uuid(),url:String::new(),resume:Some("continue".into())};
            let (input,lock,path)=claude_input(&root,"private task body",None).unwrap();
            for execution in [None,Some(41)] {
                l.execution=execution;
                assert!(bind_input(&log,&l,&path,&input,serde_json::json!({})).is_err());
                assert!(!log.exists(),"invalid intent must refuse before native submission");
            }
            l.execution=Some(44);
            assert!(bind_input(&root,&l,&path,&input,serde_json::json!({})).is_err(),"unwritable binding must refuse before native submission");
            bind_input(&log,&l,&path,&input,serde_json::json!({"before":10,"session":l.session})).unwrap();
            let written=std::fs::read_to_string(&log).unwrap();
            let record:serde_json::Value=serde_json::from_str(written.trim()).unwrap();
            assert_eq!((record["task"].as_u64(),record["attempt"].as_u64(),record["execution"].as_u64()),(Some(7),Some(42),Some(44)));
            assert_eq!(record["input"],input);assert!(!written.contains("private task body"));
            let mut turn=ClaudeTurn {root:root.clone(),log:log.clone(),session:l.session.clone(),id:if case=="copy" {uuid()} else {String::new()},
                announced:false,accepted:false,history:ClaudeHistory::default(),before:10,mode:"dontAsk".into(),input:input.clone(),_input_file:lock,pid:0,native:None,recovery:None,done:false,failure:None,launch_error:None};
            if case=="log" {std::fs::remove_file(&log).unwrap();std::fs::create_dir(&log).unwrap();}
            let output=match case {
                "timeout"=>Err(coreq::process::Failure::Timeout),
                "no-start"=>Err(coreq::process::Failure::Spawn(std::io::Error::new(std::io::ErrorKind::NotFound,"fixture missing launcher"))),
                "killed"=>Err(coreq::process::Failure::Exit(std::process::Output {status:std::process::ExitStatus::from_raw(9),stdout:vec![],stderr:b"native launch failed".to_vec()})),
                "exit-id"=>Err(coreq::process::Failure::Exit(std::process::Output {status:std::process::ExitStatus::from_raw(256),stdout:format!("backgrounded · {candidate}\n").into_bytes(),stderr:b"remote control unavailable".to_vec()})),
                "refused"=>Err(coreq::process::Failure::Exit(std::process::Output {status:std::process::ExitStatus::from_raw(256),stdout:vec![],stderr:b"Error: Background sessions are disabled.\nAsk your administrator.\n".to_vec()})),
                // #1555: Claude's own refusal of an untrusted root
                "untrusted"=>Err(coreq::process::Failure::Exit(std::process::Output {status:std::process::ExitStatus::from_raw(256),stdout:vec![],stderr:b"Error: Workspace not trusted. Run claude here first.".to_vec()})),
                _=>Ok(std::process::Output {status:std::process::ExitStatus::from_raw(0),stdout:if case=="response" {b"lost response".to_vec()} else {format!("backgrounded · {candidate}\n").into_bytes()},stderr:vec![]}),
            };
            let result=turn.finish_launch(output);
            if case=="no-start" {assert!(result.unwrap_err().no_start);}
            else if case=="untrusted" {
                let e=result.unwrap_err();
                assert!(e.no_start&&e.message.contains("does not trust")&&e.message.contains("run `claude`"),"{}",e.message);
                assert!(!e.message.contains(&root.display().to_string()),"board text never holds a local path");
            }
            else if case=="refused" {
                let e=result.unwrap_err();
                assert!(e.no_start && e.message.contains("refused: Error: Background sessions are disabled. Ask your administrator."),"{}",e.message);
                let x=crate::core::Execution {id:44,attempt:42,through:0,host:"mac".into(),runtime:"claude".into(),accepted:false,release:None,usage_hold:None};
                let attempt=attempt_log(&root,"worker",7,42);std::fs::copy(&log,&attempt).unwrap();
                assert_eq!(refused_launch(&root,7,&x).as_deref(),Some("Error: Background sessions are disabled. Ask your administrator."),"saved refusal reconciles cold");
                std::fs::remove_file(attempt).unwrap();
            }
            else {
                assert!(result.is_ok(),"{case}: uncertain native side effect must retain its handle");
                assert!(turn.launch_error.is_some());
                if case=="exit-id" {assert_eq!(turn.id,candidate);}
                if case=="killed" {
                    assert!(turn.launch_error.as_deref().unwrap().contains("native launch failed"));
                    let reason=turn.recover_job(&[],Some(&root)).unwrap_err();
                    assert!(reason.contains("native launch failed")&&reason.contains("original input not found"),"native refusal was masked: {reason}");
                }
                assert_eq!(turn.input,input);
                assert!(turn.observe(&serde_json::json!({"id":turn.id,"sessionId":turn.session,"state":"failed","pid":null})).unwrap_err().contains("current input acceptance unconfirmed"),"old job status cannot settle the current submission");
                if matches!(case,"log"|"copy") {assert_eq!(turn.id,candidate,"returned native identity survives a subsequent failure");}
                let probe=File::options().read(true).write(true).open(&path).unwrap();
                assert!(matches!(probe.try_lock(),Err(std::fs::TryLockError::WouldBlock)),"uncertain turn keeps its original input");
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: lost launch reply -> exact native input lookup -> resumed observation without resubmission.
    // Risk: a missing identity stays orphaned, or unrelated/ambiguous history is adopted as this execution.
    // #1627 `native-cwd`: `claude agents` names the plain root, not the canonical (Windows `\\?\`) form.
    #[test]
    fn claude_lost_reply_recovers_only_its_unique_input() {
        let root=std::env::temp_dir().join(format!("taskq-claude-lookup-{}",uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let root=root.canonicalize().unwrap();
        let session=uuid();let other=uuid();
        std::fs::create_dir_all(root.join("projects/a")).unwrap();
        #[cfg(unix)] std::os::unix::fs::symlink(root.join("missing-native-directory"),root.join("projects/missing")).unwrap();
        for id in ["c37eb75a","eaff90e7"] {std::fs::create_dir_all(root.join("jobs").join(id)).unwrap();}
        for case in ["match","native-cwd","known","blocks","absent","two-jobs","two-inputs","partial","invalid","oversize","missing","foreign","resume","old","copied","large-unrelated","locator-only","bad-locator","foreign-locator","large-locator","long-history"] {
            let (input,lock,path)=claude_input(&root,"fixture task body",None).unwrap();
            let event=serde_json::json!({"type":"user","message":{"content":if case=="blocks" {serde_json::json!([{"type":"text","text":input}])} else {serde_json::json!(input)}}}).to_string()+"\n";
            let a=root.join("projects/a").join(format!("{session}.jsonl"));let b=root.join("projects/a").join(format!("{other}.jsonl"));
            let content=match case {
                "absent"=>serde_json::json!({"type":"user","message":{"content":"unrelated input"}}).to_string()+"\n",
                "two-inputs"=>event.repeat(2),
                "partial"=>event.clone()+"{",
                "invalid"=>event.clone()+"{}\n",
                "oversize"=>"x".repeat(8*1024*1024+1),
                "long-history"=>(serde_json::json!({"type":"system","padding":"x".repeat(1024*1024)}).to_string()+"\n").repeat(18)+&event
                    +&serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"owned final"}]}}).to_string()+"\n"
                    +&serde_json::json!({"type":"system","subtype":"turn_duration"}).to_string()+"\n"
                    +&serde_json::json!({"type":"user","message":{"content":"another turn"}}).to_string()+"\n"
                    +&serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"foreign final"}]}}).to_string()+"\n",
                "resume"=>serde_json::json!({"type":"system"}).to_string()+"\n"+&event,
                _=>event.clone(),
            };
            std::fs::write(&a,content).unwrap();std::fs::write(&b,&event).unwrap();
            for (id,sid) in [("c37eb75a",&session),("eaff90e7",&other)] {
                let intent=if case=="locator-only" || case=="resume" || (case=="large-unrelated" && id=="eaff90e7") {"unrelated original input"} else {&input};
                let locator=serde_json::json!({"cwd":root,"sessionId":sid,"intent":intent,"providerEnv":{"FIXTURE":"not retained or printed"}});
                std::fs::write(root.join("jobs").join(id).join("state.json"),locator.to_string()).unwrap();
            }
            let locator=root.join("jobs/c37eb75a/state.json");
            if case=="bad-locator" {std::fs::write(&locator,"{").unwrap();}
            if case=="foreign-locator" {std::fs::write(&locator,serde_json::json!({"cwd":root,"sessionId":other,"intent":input}).to_string()).unwrap();}
            if case=="large-locator" {std::fs::write(&locator,"x".repeat(1024*1024+1)).unwrap();}
            if case=="large-unrelated" {std::fs::write(&b,"x".repeat(8*1024*1024+1)).unwrap();}
            let mut turn=ClaudeTurn {root:root.clone(),log:root.join("attempt.log"),session:if matches!(case,"resume"|"old") {session.clone()} else if case=="copied" {other.clone()} else {String::new()},id:if matches!(case,"known"|"two-jobs"|"resume") {"c37eb75a".into()} else {String::new()},
                announced:false,accepted:false,history:ClaudeHistory::default(),before:usize::from(matches!(case,"resume"|"old")),mode:"dontAsk".into(),input:input.clone(),_input_file:lock,pid:0,native:None,recovery:None,done:false,failure:None,launch_error:Some("launch response lost".into())};
            let plain=if cfg!(windows) {PathBuf::from(root.to_str().unwrap().trim_start_matches(r"\\?\"))} else {root.join("projects/..")};
            let cwd=match case {"foreign"=>root.parent().unwrap(),"native-cwd"=>plain.as_path(),_=>root.as_path()};
            let mut rows=vec![serde_json::json!({"id":"c37eb75a","kind":"background","sessionId":session,"cwd":cwd,"state":"working"})];
            if matches!(case,"two-jobs"|"large-unrelated") {rows.push(serde_json::json!({"id":"eaff90e7","kind":"background","sessionId":other,"cwd":root,"state":"working"}));}
            rows.push(serde_json::json!({"id":"abcdef12","kind":"background","cwd":root.join("deleted-worktree"),"state":"done"}));
            if case=="missing" {std::fs::remove_file(&a).unwrap();}
            let original_id=turn.id.clone();
            let mut result=turn.recover_job(&rows,Some(&root));
            if case=="long-history" {
                let mut passes=1;
                while result.as_ref().is_err_and(|e|e.contains("scan pending")) {
                    assert!(!turn.accepted);assert_eq!(turn.id,original_id);assert!(passes<5);
                    result=turn.recover_job(&rows,Some(&root));passes+=1;
                }
                assert!(passes>=3);
                assert!(turn.history.ended);assert_eq!(turn.history.answer.as_deref(),Some("owned final"));
                assert!(!std::fs::read_to_string(&turn.log).unwrap().contains("foreign final"));
                assert_eq!(claude_history(&a,0,|_|{}).unwrap(),23,"pre-submission scan supports the same large history");
            }
            if matches!(case,"match"|"native-cwd"|"known"|"blocks"|"resume"|"large-unrelated"|"long-history") {
                result.unwrap();assert_eq!(turn.id,"c37eb75a");assert_eq!(turn.session,session);
                assert_eq!(turn.accepted(),Ok(true));assert!(!turn.done,"input acceptance is not native exit");
                turn.recover_job(&rows,None).unwrap();
            } else {
                assert!(result.is_err(),"{case} must remain unknown");assert_eq!(turn.id,original_id);assert!(!turn.accepted);
            }
            assert_eq!(turn.input,input);assert!(path.exists());
            let file=File::options().read(true).write(true).open(path).unwrap();
            assert!(matches!(file.try_lock(),Err(std::fs::TryLockError::WouldBlock)),"recovery retains the original input lock");
        }
        let projects=root.join("projects");
        std::fs::create_dir_all(projects.join("a")).unwrap();
        std::fs::create_dir_all(projects.join("b")).unwrap();
        std::fs::write(projects.join("unrelated-file"),"").unwrap();
        let native=projects.join("a").join(format!("{session}.jsonl"));
        std::fs::write(&native,"{}\n").unwrap();
        assert_eq!(claude_transcript_in(&root,&session).unwrap(),native);
        std::fs::write(projects.join("b").join(format!("{session}.jsonl")),"{}\n").unwrap();
        assert!(claude_transcript_in(&root,&session).unwrap_err().contains("multiple transcript paths"));
        assert!(claude_transcript_in(&root,"../other").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: an owned native probe loses its launch reply -> candidate recovers input and observes exit.
    // Reuses an already-stopped probe; never dispatches another model turn.
    #[test]
    #[ignore = "owned native probe; TASKQ_CLAUDE_READBACK selects its captured lifecycle evidence"]
    fn native_claude_lost_reply_readback() {
        let evidence=PathBuf::from(std::env::var("TASKQ_CLAUDE_READBACK").expect("TASKQ_CLAUDE_READBACK"));
        let read=|file:&str|serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(evidence.join(file)).unwrap()).unwrap();
        let manifest=read("manifest.json");let result=read("stop-result.json");let identity=read("identity-before.json");
        let pid=u32::try_from(identity["pid"].as_u64().unwrap()).unwrap();
        assert_eq!(coreq::process::identity(pid).unwrap(),None,"probe must already have exited");
        assert_eq!(result["stop"]["exit_code"],0);
        let root=PathBuf::from(manifest["native_root"].as_str().unwrap()).canonicalize().unwrap();
        let input_path=evidence.join("input.txt");let input=std::fs::read_to_string(&input_path).unwrap();
        let log=evidence.join("candidate-readback.log");File::create(&log).unwrap();
        let mut turn=ClaudeTurn {root,log,session:String::new(),id:String::new(),announced:false,accepted:false,history:ClaudeHistory::default(),before:0,
            mode:"inherited-probe".into(),input,_input_file:coreq::file_lock::try_lock_shared_file(File::open(input_path).unwrap()).unwrap(),pid,native:Some(coreq::process::Identity {pid,started:identity["started"].as_str().unwrap().into()}),recovery:None,done:false,failure:None,launch_error:Some("fixture: launch reply lost".into())};
        assert_eq!(turn.exited(),Ok(Some(0)));
        assert_eq!(turn.accepted(),Ok(true));assert_eq!(turn.id,result["job"].as_str().unwrap());
        assert_eq!(turn.session,result["session"].as_str().unwrap());
        assert_eq!(coreq::process::identity(pid).unwrap(),None);
    }

    // Chain: § 8 Access adapter on the host OS (A3, A4). Risk: a Full grant keeps the runtime's own permission or
    // sandbox flags on start or resume, or a host without enforcement starts a non-Full worker or reviewer.
    #[test]
    fn access_maps_full_and_refuses_unknown_enforcement_before_start() {
        let install = std::env::temp_dir().join(format!("taskq-access-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        std::fs::create_dir_all(&install).unwrap();
        let codex: Vec<String> = ["-s", "workspace-write", "-m", "gpt-5", "-c", "model_reasoning_effort=high"].map(String::from).into();
        let root = std::env::temp_dir();
        let env = Env { context: "", root: &root, exe: Path::new("/i/taskq"), install: &install, permission_mode: "dontAsk", codex: &codex, publish: "pr", board_env: &[], jobs: None };
        let args = |c: &Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        let mut l = Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: "worker".into(), rt: "claude".into(), session: "s".into(), url: "https://b/7".into(), resume: None };
        crate::policy::grant(&install, "claude", "full", None, "op", 1).unwrap();
        crate::policy::grant(&install, "codex", "full", None, "op", 1).unwrap();
        for resume in [None, Some("answer".to_string())] {
            l.resume = resume;
            l.rt = "claude".into();
            assert!(args(&command(&env, &l, &access(&env, "claude").unwrap()).unwrap()).contains("--permission-mode bypassPermissions"));
            l.rt = "codex".into();
            assert!(args(&command(&env, &l, &access(&env, "codex").unwrap()).unwrap()).starts_with("exec --json -s danger-full-access -m gpt-5 -c model_reasoning_effort=high "));
        }
        // the host without enforcement (every OS but macOS): no grant or a non-Full one refuses before any child or fetch
        if !cfg!(target_os = "macos") {
            for (rt, mode) in [("claude", "none"), ("codex", "workspace"), ("claude", "restricted")] {
                crate::policy::grant(&install, rt, mode, None, "op", 2).unwrap();
                l.rt = rt.into();
                let e = spawn(&env, &l).err().expect("refused");
                assert!(e.message.starts_with("access refused: ") && e.message.contains(std::env::consts::OS), "{}", e.message);
            }
        }
        let _ = std::fs::remove_dir_all(&install);
    }

    // Chain: native identity -> failed/retried protected checkpoint -> cold observer -> exact process exit.
    // Risk: cold Claude recovery recaptures a reused PID, or failed persistence drops the only original identity.
    #[test]
    fn claude_cold_identity_survives_checkpoint_failure_and_process_exit() {
        let root=std::env::temp_dir().join(format!("taskq-claude-cold-{}",uuid()));
        let install=root.join("install");std::fs::create_dir_all(&install).unwrap();
        let l=Launch {files: String::new(), execution:Some(42),task:7,title:String::new(),attempt:42,role:"worker".into(),rt:"claude".into(),session:uuid(),url:String::new(),resume:None};
        let log=log_path(&root,&l);
        let (input,lock,path)=claude_input(&root,"controlled process fixture",Some(&l)).unwrap();
        bind_input(&log,&l,&path,&input,serde_json::json!({"session":l.session})).unwrap();
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::stop_child","--ignored"])
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut t=ClaudeTurn {root:root.clone(),log,session:l.session.clone(),id:"c37eb75a".into(),announced:false,accepted:true,history:ClaudeHistory::default(),before:0,mode:"fixture".into(),input,
            _input_file:lock,pid:0,native:None,recovery:Some(Recovery {root:root.clone(),install:install.clone(),task:7,attempt:42,execution:42,runtime:"claude".into()}),done:false,failure:None,launch_error:None};
        let row=serde_json::json!({"id":t.id,"sessionId":t.session,"pid":child.id(),"state":"working"});
        t.capture_identity(&row).unwrap();let original=t.native_identity().unwrap();
        std::fs::write(install.join("native"),"fixture: unavailable recovery directory").unwrap();
        assert!(t.persist().unwrap_err().contains("checkpoint failed"));
        assert_eq!(t.native_identity(),Ok(original.clone()));assert!(t.recovery.is_some());
        std::fs::remove_file(install.join("native")).unwrap();t.persist().unwrap();
        assert!(t.recovery.is_none());
        let saved=crate::kids_file(&install,&root).unwrap();assert_eq!(saved.len(),1);
        assert_eq!(saved[0].process,original);assert_eq!(saved[0].runtime,"claude");assert_eq!(saved[0].offset,0);
        let mut conflicting=row.clone();conflicting["pid"]=std::process::id().into();
        assert!(t.capture_identity(&conflicting).is_err());assert_eq!(t.native_identity(),Ok(original.clone()));
        let x=crate::core::Execution {id:42,attempt:42,through:42,host:"mac".into(),runtime:"claude".into(),accepted:true,release:None,usage_hold:None};
        drop(t);
        let restored=recover(&root,&install,7,&x).unwrap();
        assert_eq!(restored.native_identity(),Ok(original.clone()));
        assert_eq!(claude_stopped(&serde_json::json!({"state":"done"}),Some(&original)),Ok(false));
        child.kill().unwrap();child.wait().unwrap();
        assert_eq!(claude_stopped(&serde_json::json!({"state":"done"}),Some(&original)),Ok(true));
        drop(restored);
        let after_exit=recover(&root,&install,7,&x).unwrap();
        assert_eq!(after_exit.native_identity(),Ok(original),"cold recovery does not recapture an absent/reused PID");
        let mut other=x.clone();other.id=43;
        assert!(recover(&root,&install,7,&other).is_err());
        let protected=crate::kids_path(&install,&root).unwrap();std::fs::write(&protected,"corrupt").unwrap();
        assert!(recover(&root,&install,7,&x).is_err());assert_eq!(std::fs::read_to_string(protected).unwrap(),"corrupt");
        drop(after_exit);std::fs::remove_dir_all(root).unwrap();
    }

    // #2057 Chain: image with an untracked helper and a worker child -> exec -> both exit -> host reap.
    // Defect: the old image's helpers (curl, `open -W`, `log show`) stayed zombies, ~3 per re-exec. Risk: the reap
    // misses them, or takes the adopted worker's exit status.
    #[cfg(unix)]
    #[test]
    fn reexec_reaps_untracked_children_and_keeps_adopted_status() {
        let root=std::env::temp_dir().join(format!("taskq-inherit-{}",uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let result=coreq::process::output("qualification.reexec-reap",Command::new(std::env::current_exe().unwrap())
            .args(["--exact","runtime::tests::reexec_reap_child","--ignored","--nocapture"])
            .env("TASKQ_IMAGE_ROOT",&root).env("TASKQ_IMAGE_PHASE","before"),std::time::Duration::from_secs(30));
        assert!(result.is_ok(),"re-exec reap fixture failed: {result:?}");
        assert_eq!(std::fs::read_to_string(root.join("verified")).unwrap(),"helper reaped; worker exit 7");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[ignore="re-exec reap fixture"]
    fn reexec_reap_child() {
        use std::os::unix::process::CommandExt;
        let root=PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
        if std::env::var("TASKQ_IMAGE_PHASE").unwrap()=="before" {
            // Handles drop at exec, as a dying update or link thread drops its curl.
            let helper=Command::new("sleep").arg("1").spawn().unwrap().id();
            let worker=Command::new("sh").args(["-c","sleep 1; exit 7"]).spawn().unwrap().id();
            std::fs::write(root.join("pids"),format!("{helper} {worker}")).unwrap();
            let error=Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::reexec_reap_child","--ignored","--nocapture"])
                .env("TASKQ_IMAGE_PHASE","after").exec();
            panic!("exec failed: {error}");
        }
        let pids:Vec<u32>=std::fs::read_to_string(root.join("pids")).unwrap().split(' ').map(|p|p.parse().unwrap()).collect();
        let (helper,worker)=(pids[0],pids[1]);
        let mut inherited=inherit_children().unwrap();inherited.sort();
        let mut want=pids.clone();want.sort();
        assert_eq!(inherited,want,"the new image sees both old children");
        let mut adopted=Adopted::new(coreq::process::identity(worker).unwrap().unwrap());
        assert_eq!(adopted.exited(),Ok(None),"worker still runs");
        let exited=|pid:u32|{ // exited, not yet reaped
            let mut info:libc::siginfo_t=unsafe{std::mem::zeroed()};
            if unsafe{libc::waitid(libc::P_PID,pid,&mut info,libc::WEXITED|libc::WNOHANG|libc::WNOWAIT)}!=0 {return false;}
            #[cfg(target_os="linux")] return unsafe{info.si_pid()}!=0;
            #[cfg(not(target_os="linux"))] return info.si_pid!=0;
        };
        let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while !(exited(helper)&&exited(worker)) {assert!(std::time::Instant::now()<until,"children did not exit");std::thread::sleep(std::time::Duration::from_millis(10));}
        // The host reap runs first and takes both statuses; neither stays a zombie.
        assert_eq!(reap_inherited(),2);
        let mut info:libc::siginfo_t=unsafe{std::mem::zeroed()};
        for pid in [helper,worker] {assert_eq!(unsafe{libc::waitid(libc::P_PID,pid,&mut info,libc::WEXITED|libc::WNOHANG|libc::WNOWAIT)},-1,"{pid} left a zombie");}
        assert_eq!(adopted.exited(),Ok(Some(7)),"adopted worker keeps its exit status");
        std::fs::write(root.join("verified"),"helper reaped; worker exit 7").unwrap();
    }

    // Chain: live Claude background turn -> protected checkpoint -> image re-exec -> recovered observer -> native exit -> board exit (#1407).
    // Risk: re-exec resubmits the original input, loses the native answer, or releases the board before the native process exits.
    #[cfg(unix)]
    #[test]
    fn claude_turn_survives_image_reexec() {
        use std::os::unix::fs::PermissionsExt;
        let root=std::env::temp_dir().join(format!("taskq-claude-image-{}",uuid()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let root=root.canonicalize().unwrap();
        // Stand-in native CLI: answers job readback only and records every call, so any submission or stop shows up.
        let fake=root.join("bin/claude");
        std::fs::write(&fake,"#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKQ_IMAGE_ROOT/claude-calls\"\n[ \"$1\" = agents ] && exec cat \"$TASKQ_IMAGE_ROOT/agents.json\"\nexit 9\n").unwrap();
        std::fs::set_permissions(&fake,std::fs::Permissions::from_mode(0o755)).unwrap();
        let path=std::env::join_paths(std::iter::once(root.join("bin")).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))).unwrap();
        let result=coreq::process::output("qualification.claude-image-handover",Command::new(std::env::current_exe().unwrap())
            .args(["--exact","runtime::tests::claude_image_child","--ignored","--nocapture"])
            .env("TASKQ_IMAGE_ROOT",&root).env("TASKQ_IMAGE_PHASE","before").env("PATH",path).env("CLAUDE_CONFIG_DIR",root.join("home")),std::time::Duration::from_secs(30));
        assert!(result.is_ok(),"Claude image handover failed: {result:?}");
        assert_eq!(std::fs::read_to_string(root.join("verified")).unwrap(),"no resubmission; kept answer; exit after native exit");
        let calls=std::fs::read_to_string(root.join("claude-calls")).unwrap();
        assert!(calls.lines().all(|c|c==format!("agents --json --all --cwd {}",root.display())),"handover may only read native jobs: {calls}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[ignore="Claude image handover fixture"]
    fn claude_image_child() {
        use std::os::unix::process::CommandExt;
        use crate::board::Board;
        let root=PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
        let home=root.join("home");
        // The in-memory board is rebuilt identically in both images; only the native record crosses exec.
        let b=crate::board::Fake::default();let n=b.create("T","",&[crate::service::TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker claude:session mac").unwrap();
        let c=crate::service::Config {root:root.clone(),install:root.join("install"),..crate::tests::cfg()};
        let mut s=crate::service::Service::new(&b,&c,"fixture",Box::new(|_|panic!("handover must not submit new work")));
        let session="0b6c3f4e-8f2a-4c1d-9e7b-5a2d1c3b4e6f";
        let transcript=home.join("projects/p").join(format!("{session}.jsonl"));
        let agents=|state:&str,pid:Option<u32>|std::fs::write(root.join("agents.json"),serde_json::json!([{"id":"c37eb75a","kind":"background","sessionId":session,"cwd":root,"state":state,"pid":pid}]).to_string()).unwrap();
        if std::env::var("TASKQ_IMAGE_PHASE").unwrap()=="before" {
            let l=Launch {files: String::new(), execution:Some(a),task:n,title:String::new(),attempt:a,role:"worker".into(),rt:"claude".into(),session:"session".into(),url:String::new(),resume:None};
            let log=log_path(&root,&l);std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            let (input,lock,path)=claude_input(&root,"image handover fixture",Some(&l)).unwrap();
            bind_input(&log,&l,&path,&input,serde_json::json!({"session":l.session})).unwrap();
            std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();std::fs::create_dir_all(home.join("jobs/c37eb75a")).unwrap();
            std::fs::write(&transcript,serde_json::json!({"type":"user","message":{"content":input}}).to_string()+"\n").unwrap();
            std::fs::write(home.join("jobs/c37eb75a/state.json"),serde_json::json!({"cwd":root,"sessionId":session,"intent":input}).to_string()).unwrap();
            // An orphaned sleep stands in for the native process: not this image's child, so its exit leaves no zombie.
            let out=Command::new("sh").args(["-c","sleep 60 >/dev/null 2>&1 & echo $!"]).output().unwrap();
            let pid:u32=String::from_utf8(out.stdout).unwrap().trim().parse().unwrap();
            std::fs::write(root.join("native-pid"),pid.to_string()).unwrap();
            agents("working",Some(pid));
            let turn=ClaudeTurn {root:root.clone(),log,session:String::new(),id:"c37eb75a".into(),announced:false,accepted:false,history:ClaudeHistory::default(),before:0,mode:"dontAsk".into(),input,
                _input_file:lock,pid:0,native:None,recovery:None,done:false,failure:None,launch_error:None};
            s.kids.push(crate::service::Kid {unknown:None,stopping:false,session_url:None,execution:a,task:n,attempt:a,rt:"claude".into(),proc:Box::new(turn)});
            assert!(crate::lifecycle_waits(&s,false).contains(&format!("#{n}")),"no native identity: re-exec defers");
            s.kids[0].proc.checkpoint(&c,&l,a).unwrap();
            assert!(!crate::lifecycle_waits(&s,false).contains(&format!("#{n}")),"saved native identity: re-exec passes over the live turn");
            crate::save_kids(&s,&root).unwrap();
            coreq::telemetry::capture::before_exec();
            let error=Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::claude_image_child","--ignored","--nocapture"])
                .env("TASKQ_IMAGE_PHASE","after").exec();
            panic!("exec failed: {error}");
        }
        let pid:u32=std::fs::read_to_string(root.join("native-pid")).unwrap().parse().unwrap();
        crate::adopt(&mut s,&c.install,&root).unwrap();crate::adopt(&mut s,&c.install,&root).unwrap();
        assert_eq!(s.kids.len(),1);assert_eq!(s.kids[0].execution,a);assert_eq!(s.kids[0].proc.id(),pid);
        let accepted=|| crate::core::executions(n,&crate::board::entries(&b,n).unwrap()).into_iter().find(|x|x.id==a).unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while !accepted().accepted { s.reap_owned(); assert!(std::time::Instant::now()<until,"acceptance not observed: {:?}",s.kids.iter().map(|k|&k.unknown).collect::<Vec<_>>()); }
        let mut file=File::options().append(true).open(&transcript).unwrap();
        writeln!(file,"{}\n{}",serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"owned final"}]}}),serde_json::json!({"type":"system","subtype":"turn_duration"})).unwrap();
        s.reap_owned();
        assert_eq!(s.kids.len(),1,"a finished answer is not native exit");assert_eq!(accepted().release,None);
        Command::new("kill").arg(pid.to_string()).status().unwrap();
        // An exiting orphan may briefly have no readable creation identity until init reaps it.
        while !matches!(coreq::process::identity(pid),Ok(None)) { assert!(std::time::Instant::now()<until); std::thread::sleep(std::time::Duration::from_millis(10)); }
        agents("done",None);
        while !s.kids.is_empty() { s.reap_owned(); assert!(std::time::Instant::now()<until,"native exit not observed: {:?}",s.kids.iter().map(|k|&k.unknown).collect::<Vec<_>>()); }
        assert_eq!(accepted().release,Some(crate::core::Release::Exit));
        let log=std::fs::read_to_string(attempt_log(&root,"worker",n,a)).unwrap();
        assert_eq!(log.matches("\"result\":\"owned final\"").count(),1,"answer kept once");
        crate::save_kids(&s,&root).unwrap();assert!(crate::kids_file(&c.install,&root).unwrap().is_empty());
        std::fs::write(root.join("verified"),"no resubmission; kept answer; exit after native exit").unwrap();
    }

    // #1561 Chain: two Claude jobs launched apart -> native rows -> recovery records.
    // Defect: the later job's record held the earlier job's process; its exit then failed every handover pass.
    // Retained: each record carries its own job's process; a process another task's record holds is refused.
    #[cfg(unix)]
    #[test]
    fn claude_records_carry_their_own_process() {
        let root=std::env::temp_dir().join(format!("taskq-claude-own-{}",uuid()));
        let install=root.join("install");
        std::fs::create_dir_all(&root).unwrap();
        let mut first=Command::new("sleep").arg("30").spawn().unwrap();
        let mut second=Command::new("sleep").arg("30").spawn().unwrap();
        let turn=|task:u64,id:&str|ClaudeTurn { root:root.clone(), log:root.join(format!("{task}.log")), session:String::new(),
            id:id.into(), announced:false, accepted:true, history:ClaudeHistory::default(), before:0, mode:"dontAsk".into(),
            input:"input".into(), _input_file:coreq::file_lock::try_lock_shared_file(File::open(std::env::current_exe().unwrap()).unwrap()).unwrap(), pid:0, native:None,
            recovery:Some(Recovery {install:install.clone(),root:root.clone(),task,attempt:task*10,execution:task*10,runtime:"claude".into()}), done:false, failure:None, launch_error:None };
        let row=|id:&str,pid:u32|serde_json::json!({"id":id,"state":"working","pid":pid});
        let mut a=turn(1557,"c2fc6503");
        a.capture_identity(&row("c2fc6503",first.id())).unwrap();a.persist().unwrap();
        let mut b=turn(1558,"90d3662a");
        let refused=b.capture_identity(&row("90d3662a",first.id())).unwrap_err();
        assert!(refused.contains("#1557"),"the earlier job's process is not this job's identity: {refused}");
        assert!(b.native.is_none(),"a refused readback keeps no identity");
        b.capture_identity(&row("90d3662a",second.id())).unwrap();b.persist().unwrap();
        let records:Vec<_>=crate::kids_file(&install,&root).unwrap().into_iter().map(|k|(k.task,k.process.pid)).collect();
        assert_eq!(records,[(1557,first.id()),(1558,second.id())],"each record carries its own job's process");
        let mut stolen=crate::kids_file(&install,&root).unwrap()[0].clone();stolen.task=1559;
        assert!(crate::checkpoint_kid(&install,&root,stolen).unwrap_err().contains("#1557"),"no record takes another task's process");
        for c in [&mut first,&mut second] {c.kill().unwrap();c.wait().unwrap();}
        std::fs::remove_dir_all(root).unwrap();
    }

    // #1638 Chain: recorded Claude process -> service restart -> native job reports a new process -> recovered readback.
    // Defect: the recovered turn compared the new pid with the record and fenced the execution as unknown forever.
    // Retained: a live recorded process, an unproved input or an unknown conversation still refuse the new process.
    #[cfg(unix)]
    #[test]
    fn recovered_claude_job_on_a_new_process_resolves() {
        let root=std::env::temp_dir().join(format!("taskq-claude-moved-{}",uuid()));
        let install=root.join("install");
        std::fs::create_dir_all(&root).unwrap();
        let mut old=Command::new("sleep").arg("30").spawn().unwrap();
        let mut new=Command::new("sleep").arg("30").spawn().unwrap();
        let recorded=coreq::process::identity(old.id()).unwrap().unwrap();
        crate::checkpoint_kid(&install,&root,crate::data::KidLine {task:1592,attempt:10,runtime:"claude".into(),process:recorded.clone(),execution:11,offset:0,job:None}).unwrap();
        let turn=|accepted:bool,session:&str|ClaudeTurn { root:root.clone(), log:root.join("1592.log"), session:session.into(),
            id:"c37eb75a".into(), announced:false, accepted, history:ClaudeHistory::default(), before:0, mode:"dontAsk".into(),
            input:"input".into(), _input_file:coreq::file_lock::try_lock_shared_file(File::open(std::env::current_exe().unwrap()).unwrap()).unwrap(), pid:0, native:Some(recorded.clone()),
            recovery:Some(Recovery {install:install.clone(),root:root.clone(),task:1592,attempt:10,execution:11,runtime:"claude".into()}), done:false, failure:None, launch_error:None };
        let session="0b6c3f4e-8f2a-4c1d-9e7b-5a2d1c3b4e6f";
        let row=serde_json::json!({"id":"c37eb75a","sessionId":session,"state":"working","pid":new.id()});
        let fenced=|mut t:ClaudeTurn|{let e=t.capture_identity(&row).unwrap_err();assert!(e.contains("process changed"),"{e}");assert_eq!(t.native.as_ref(),Some(&recorded));};
        fenced(turn(true,session));
        old.kill().unwrap();old.wait().unwrap();
        fenced(turn(false,session));
        fenced(turn(true,""));
        let mut t=turn(true,session);
        t.capture_identity(&row).unwrap();t.persist().unwrap();
        assert_eq!(t.id(),new.id(),"the job's new process stands in for the exited one");
        let records:Vec<_>=crate::kids_file(&install,&root).unwrap().into_iter().map(|k|(k.task,k.execution,k.process.pid)).collect();
        assert_eq!(records,[(1592,11,new.id())],"the record follows the same execution to its new process");
        new.kill().unwrap();new.wait().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    // #1561 Chain: live Claude handle -> recorded process exits -> handover pass -> board exit -> handover.
    // Defect: every pass failed with "child identity changed during handover" and the pending update never ran.
    // Retained: before the board exit the record stays and one error names the execution; after it the handover completes.
    #[cfg(unix)]
    #[test]
    fn exited_process_resolves_from_board_exit_at_handover() {
        use crate::board::Board;
        let root=std::env::temp_dir().join(format!("taskq-exited-{}",uuid()));
        std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let b=crate::board::Fake::default();let n=b.create("T","",&[crate::service::TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker claude:session mac").unwrap();
        let c=crate::service::Config {root:root.clone(),install:root.clone(),..crate::tests::cfg()};
        let mut child=Command::new("sleep").arg("30").spawn().unwrap();
        let native=coreq::process::identity(child.id()).unwrap().unwrap();
        crate::checkpoint_kid(&root,&root,crate::data::KidLine {task:n,attempt:a,runtime:"claude".into(),process:native.clone(),execution:a,offset:0,job:None}).unwrap();
        child.kill().unwrap();child.wait().unwrap();
        let turn=ClaudeTurn { root:root.clone(), log:root.join("attempt.log"), session:String::new(), id:"c37eb75a".into(), announced:false, accepted:true,
            history:ClaudeHistory::default(), before:0, mode:"dontAsk".into(), input:"input".into(),
            _input_file:coreq::file_lock::try_lock_shared_file(File::open(std::env::current_exe().unwrap()).unwrap()).unwrap(), pid:0, native:Some(native), recovery:None, done:false, failure:None, launch_error:None };
        let mut s=crate::service::Service::new(&b,&c,"test",Box::new(|_|unreachable!()));
        s.kids.push(crate::service::Kid {unknown:None,stopping:false,session_url:None,execution:a,task:n,attempt:a,rt:"claude".into(),proc:Box::new(turn)});
        let path=crate::kids_path(&root,&root).unwrap();
        let saved=std::fs::read_to_string(&path).unwrap();
        assert_eq!(crate::save_kids(&s,&root).unwrap_err(),format!("#{n} execution {a}: recorded native process exited; handover waits for its board release"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(),saved,"an unreleased record stays");
        b.comment(n,&format!("taskq: released {a} exit attempt {a}\nop-id: release-{a}")).unwrap();
        crate::save_kids(&s,&root).unwrap();
        assert!(crate::kids_file(&root,&root).unwrap().is_empty(),"board exit resolves the record; handover completes");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: native job schema -> observation -> release fence.
    // Risk: missing optional fields or a new state fabricate exit or hide observation failures.
    #[test]
    fn claude_native_state_requires_exit_evidence() {
        let root=std::env::temp_dir().join(format!("taskq-claude-state-{}",uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let log=root.join("attempt.log");
        File::create(&log).unwrap();
        let mut t = ClaudeTurn { root:root.clone(), log, session:uuid(),
            id: "c37eb75a".into(), announced: false, accepted:false, history:ClaudeHistory::default(), before: 0, mode: "dontAsk".into(),
            input: "invented input".into(), _input_file: coreq::file_lock::try_lock_shared_file(File::open(std::env::current_exe().unwrap()).unwrap()).unwrap(), pid: 0, native:None, recovery:None, done: false, failure: None, launch_error:None };
        assert!(native_job_id(&t.id));
        assert!(native_job_id(&uuid()));
        for id in ["--all","../c37eb75a","c37eb75a --all","invented-job",""] {assert!(!native_job_id(id));}
        for state in ["failed", "stopped", "crashed", "unexpected"] {
            assert!(t.observe(&serde_json::json!({"state":state})).is_err(),"{state} without process proof must not release");
        }
        for state in ["done","blocked","failed","crashed","unexpected"] {
            assert_ne!(claude_stopped(&serde_json::json!({"state":state}),None),Ok(true),"{state} is not a process exit receipt");
        }
        assert!(claude_stopped(&serde_json::json!({"state":"stopped"}),None).is_err());
        assert_eq!(claude_stopped(&serde_json::json!({"state":"stopped"}),coreq::process::identity(std::process::id()).unwrap().as_ref()),Ok(false));
        assert_eq!(claude_stopped(&serde_json::json!({"state":"done"}),coreq::process::identity(std::process::id()).unwrap().as_ref()),Ok(false));
        let mut child=Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::stop_child","--ignored"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let identity=coreq::process::identity(child.id()).unwrap().unwrap();child.kill().unwrap();child.wait().unwrap();
        assert_eq!(claude_stopped(&serde_json::json!({"state":"done"}),Some(&identity)),Ok(true),"native stop can preserve done after observed process exit");
        for pid in [serde_json::json!(0), serde_json::json!(u64::MAX), serde_json::json!("unknown")] {
            assert!(claude_stopped(&serde_json::json!({"state":"stopped","pid":pid}),None).is_err());
        }
        for row in [serde_json::json!({}),serde_json::json!({"state":null})] {assert!(t.observe(&row).is_err());}
        let row=serde_json::json!({"id":t.id,"state":"working","sessionId":t.session});
        assert!(t.observe(&row).unwrap_err().contains("transcript unavailable"),"no status field must not suppress the real observation error");
        assert!(t.announced);
        // A completed input survives asynchronous stop; an API failure never becomes a valid final answer.
        for api_error in [false,true] {
            t.history=ClaudeHistory::default();t.done=false;t.failure=None;t.accepted=true;
            let path=root.join(format!("completed-{api_error}.jsonl"));
            let mut assistant=serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":if api_error {"Login expired; please run /login"} else {"owned final"}}]}});
            if api_error {assistant["error"]=serde_json::json!("authentication_failed");assistant["isApiErrorMessage"]=serde_json::json!(true);}
            let events=[serde_json::json!({"type":"user","message":{"content":t.input}}),assistant,serde_json::json!({"type":"system","subtype":"turn_duration"})];
            std::fs::write(&path,events.iter().map(|v|format!("{v}\n")).collect::<String>()).unwrap();
            assert!(t.history.read(&path,0,&t.input,&t.log).unwrap());assert!(t.history.ended);
            let row=serde_json::json!({"id":t.id,"sessionId":t.session,"state":"stopped","pid":null});
            t.native=coreq::process::identity(std::process::id()).unwrap();
            assert!(t.observe(&row).is_err(),"completed input and stop acknowledgment cannot replace process exit proof");
            t.native=Some(identity.clone());
            assert_eq!(t.observe(&row),Ok(Some(if api_error {-1} else {0})),"completed input outcome lost after stopped");
            assert_eq!(t.exited(),Ok(Some(if api_error {-1} else {0})),"terminal result must remain stable");
            if api_error {assert_eq!(t.exit_error(),Some("auth"));assert!(t.history.answer.is_none(),"native API error cannot be a success answer");assert!(t.history.failure.as_ref().unwrap().1.contains("authentication_failed: Login expired; please run /login"));}
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: § 8 codex-app handshake -> open thread -> queue -> one new `task_complete` on this host's app.
    // Paid/client-dependent qualification: deliberately excluded from the normal CI gate.
    #[test]
    #[ignore = "live runtime matrix; set TASKQ_APP_ROOT=<git dir> and run explicitly"]
    fn live_app_turn_completes_once() {
        let root = PathBuf::from(std::env::var("TASKQ_APP_ROOT").expect("TASKQ_APP_ROOT"));
        let install = std::env::temp_dir().join(format!("taskq-app-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        crate::policy::grant(&install, "codex-app", "full", None, "op", 1).unwrap();
        let env = Env { context: "", root: &root, exe: Path::new("taskq"), install: &install, permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let l = Launch { files: String::new(), execution: None, task: 0, title: "live".into(), attempt: 0, role: "worker".into(), rt: "codex-app".into(), session: "-".into(), url: String::new(), resume: None };
        let log = root.join("app-turn.log");
        let (text, input, _path) = app_input(&root, &format!("{}Reply with the single word QUEUED and do nothing else.", "A taskq test line; ignore it.\n".repeat(700)),None).unwrap();
        let mut t = AppTurn { root: root.canonicalize().unwrap(), history:AppHistory::default(), hand: None, log: log.clone(), text, _input: Some(input), binding:None, thread: String::new(), done: 0, rollout: None, running: app_running, control:app_cancel, cancelled: false, queue_error: None, accepted: false };
        t.hand = Some(start(&env, &l, &log, HANDSHAKE, &access(&env, "codex-app").unwrap()).unwrap());
        let code = (0..600).find_map(|_| { std::thread::sleep(std::time::Duration::from_secs(1)); t.exited().unwrap() });
        eprintln!("thread {} rollout {:?} completes {} exit {code:?}", t.thread, t.rollout, t.rollout.as_ref().map_or(0, |p| completes(p).unwrap()));
        let _ = std::fs::remove_dir_all(&install);
        assert_eq!(code, Some(0));
        assert_eq!(completes(t.rollout.as_ref().unwrap()).unwrap(), t.done + 1, "exactly one queued turn completed");
    }

    // Chain: § 8 Windows app brief -> PowerShell -> taskq. Risk: quoted paths break parsing or execute input.
    #[cfg(windows)]
    #[test]
    fn app_brief_powershell_values_round_trip() {
        let root = Path::new("C:\\project's $cash; folder");
        let exe = Path::new("C:\\tool's $cash; folder\\taskq.exe");
        let env = Env { context: "", root, exe, install: Path::new("C:\\install"), permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let l = Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: "worker's $cash; role".into(), rt: "codex-app".into(), session: "s".into(), url: "https://b/7".into(), resume: None };
        let text = brief(&env, &l);
        let command = text.split("your inbox: `").nth(1).unwrap().split('`').next().unwrap();
        let prefix = command.split("; & ").next().unwrap();
        let executable = command.split("; & ").nth(1).unwrap().strip_suffix(" inbox 7").unwrap();
        let script = format!("{prefix}; ConvertTo-Json -Compress @($env:TASKQ_TASK, $env:TASKQ_ATTEMPT, $env:TASKQ_ROLE, $env:TASKQ_ROOT, {executable})");
        let out = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let values: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(values, vec!["7", "42", &l.role, &root.display().to_string(), &exe.display().to_string()]);
    }

    // Chain: § 7 sandbox-blocked host checks. Risk: a worker requeues a sandbox limit or claims a local PASS.
    #[test]
    fn worker_brief_keeps_sandbox_blocked_checks_at_the_publication_ci_gate() {
        assert!(SANDBOX_CHECKS.contains("sandbox-blocked") && SANDBOX_CHECKS.contains("Checks"));
        assert!(SANDBOX_CHECKS.contains("never as PASS") && SANDBOX_CHECKS.contains("do not requeue"));
        assert!(SANDBOX_CHECKS.contains("green CI on the GitHub PR merge result (merge_group)"));
        assert!(SANDBOX_CHECKS.contains("private fallback uses exact-SHA CI"));
    }

    fn rules_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("taskq-rules-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("docs/sub")).unwrap();
        std::fs::write(root.join("docs/guide.md"), "Intro line\n## Guide title\nBody.").unwrap();
        std::fs::write(root.join("docs/sub/plain.md"), "No heading here.").unwrap();
        std::fs::write(root.join("docs/shot.png"), [0u8, 159, 146]).unwrap();
        root
    }

    // Chain: section 8 rule delivery. Risk: native runtime argv contains a full brief or its file is pruned mid-turn.
    #[test]
    fn claude_large_input_stays_out_of_argv() {
        let root = rules_root("background input ' quoted");
        let text = format!("{}\nUTF-8: \u{2605}\n", "invented padding ".repeat(6400));
        assert!(text.len() > 100 * 1024);
        let mut inputs = Vec::new();
        for (turn, app) in [("start",false),("resume",false),("concurrent operator",false),("app start",true),("app resume",true),("app concurrent",true)] {
            let brief = format!("{text}{turn}");
            let (input, _file, _path) = if app {app_input(&root,&brief,None)} else {claude_input(&root, &brief,None)}.unwrap();
            assert!(input.len() < 8192 && !input.contains("invented padding"));
            assert!(input.contains("in full"));
            if app { assert!(input.contains("do not proceed")); } else { assert!(input.contains("use Read")); }
            assert!(!inputs.contains(&input), "each turn has an immutable input file");
            let prefix=if app {"Before any work, read "} else {"Before any work, use Read to read "};
            let quoted = input.strip_prefix(prefix).unwrap().split(" in full").next().unwrap();
            let path: String = serde_json::from_str(quoted).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), brief);
            let probe = File::options().read(true).write(true).open(&path).unwrap();
            assert!(matches!(probe.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
            drop(_file);
            probe.try_lock().unwrap();
            inputs.push(input);
        }
        assert_eq!(std::fs::read_dir(root.join(".taskq/inputs")).unwrap().count(), 6);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn rule(url: &str) -> Result<String, String> {
        if url==SPECQ_REFS {return Ok(advertisement(SPECQ_REVISION));}
        if let Some(revision)=url.strip_prefix(SPECQ_TREES).and_then(|s|s.strip_suffix("?recursive=1")) {return Ok(tree_fixture(revision).to_string());}
        Ok(format!("# Rule from {url}\nApply this rule."))
    }

    fn pinned(root: &Path, revision: &str, file: &str, fetch: &mut impl FnMut(&str)->Result<String,String>) -> Result<String,StartError> {
        let body=rule(&format!("https://raw.githubusercontent.com/taskqorg/specq/{revision}/{file}")).unwrap();
        super::pinned(root,revision,file,&git_blob_hash(&body),fetch)
    }

    fn tree_fixture(revision: &str) -> serde_json::Value {
        let entries=SPECQ_CORE.iter().chain(SPECQ_INDEX).map(|file| {
            let body=format!("# Rule from https://raw.githubusercontent.com/taskqorg/specq/{revision}/{file}\nApply this rule.");
            serde_json::json!({"path":file,"type":"blob","mode":"100644","sha":git_blob_hash(&body)})
        }).collect::<Vec<_>>();
        serde_json::json!({"sha":revision,"truncated":false,"tree":entries})
    }

    #[test]
    fn newly_published_shared_file_is_indexed_and_cached_without_a_loader_change() {
        let root=rules_root("new-shared-file");let path="engineering/new-shared-rule.md";
        let url=format!("https://raw.githubusercontent.com/taskqorg/specq/{SPECQ_REVISION}/{path}");
        let body="# Newly published shared rule\nRead this rule on demand.\n";let mut fetched=Vec::new();
        let text=rules(&root,true,|request| {
            fetched.push(request.to_string());
            if request.starts_with(SPECQ_TREES) {
                let mut tree=tree_fixture(SPECQ_REVISION);
                for name in [path,"docs/excluded.md","vendor/excluded.md","README.md"] {
                    tree["tree"].as_array_mut().unwrap().push(serde_json::json!({"type":"blob","mode":"100644","path":name,"sha":git_blob_hash(body)}));
                }
                Ok(tree.to_string())
            } else if request==url {Ok(body.into())} else {rule(request)}
        }).unwrap();
        assert!(text.contains(&format!("- {path}: Newly published shared rule\n")));
        assert!(!text.contains("Read this rule on demand."),"noncore body remains demand-loaded");
        assert!(!fetched.iter().any(|url|url.contains("excluded.md")));
        assert_eq!(std::fs::read_to_string(specq_dir(&root,SPECQ_REVISION).join(path)).unwrap(),body);
        assert_eq!(rules(&root,true,|_|Err("must use fresh source/tree/body cache".into())).unwrap(),text);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_unsafe_duplicate_or_nonregular_shared_trees_refuse_delivery() {
        let valid=tree_fixture(SPECQ_REVISION);assert_eq!(parse_shared_tree(&valid.to_string()).unwrap().len(),SPECQ_CORE.len()+SPECQ_INDEX.len());
        let mut broken=Vec::new();
        let mut tree=valid.clone();tree["truncated"]=true.into();broken.push(tree);
        let mut tree=valid.clone();tree.as_object_mut().unwrap().remove("truncated");broken.push(tree);
        let mut tree=valid.clone();tree["tree"].as_array_mut().unwrap().remove(0);broken.push(tree);
        let mut tree=valid.clone();let duplicate=tree["tree"][0].clone();tree["tree"].as_array_mut().unwrap().push(duplicate);broken.push(tree);
        let mut tree=valid.clone();tree["tree"][0]["mode"]="120000".into();broken.push(tree);
        let mut tree=valid.clone();tree["tree"][0]["sha"]="invalid".into();broken.push(tree);
        let mut tree=valid.clone();tree["tree"][SPECQ_CORE.len()]["type"]="unknown".into();broken.push(tree);
        let mut tree=valid.clone();tree["tree"]=serde_json::json!(vec![serde_json::json!({});1025]);broken.push(tree);
        for path in ["../escape.md","/absolute.md","C:/escape.md","engineering/../escape.md","engineering//empty.md","engineering\\escape.md","engineering/query?.md","engineering/NUL.md","engineering/COM1.md","engineering./trailing.md","ENGINEERING/PRINCIPLES.md"] {
            let mut tree=valid.clone();let mut entry=tree["tree"][0].clone();entry["path"]=path.into();tree["tree"].as_array_mut().unwrap().push(entry);broken.push(tree);
        }
        for tree in broken {assert!(parse_shared_tree(&tree.to_string()).is_err());}
    }

    #[test]
    fn source_validation_repairs_coherent_bad_caches_and_never_publishes_bad_fetches() {
        assert_eq!(git_blob_hash("abc"),"f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f");
        let root=rules_root("source-validation");let revision=SPECQ_REVISION;let file=SPECQ_CORE[0];
        let expected=rule(&url(file)).unwrap();let hash=git_blob_hash(&expected);
        let path=specq_dir(&root,revision).join(file);
        assert!(super::pinned(&root,revision,file,&hash,&mut |_|Ok("wrong source body".into())).is_err());assert!(!path.exists());
        assert_eq!(super::pinned(&root,revision,file,&hash,&mut rule).unwrap(),expected);
        let bad="# Corrupted coherent cache\n";std::fs::write(&path,bad).unwrap();
        std::fs::write(path.with_extension("cache.json"),rule_cache(revision,file,bad).to_string()).unwrap();
        assert!(super::pinned(&root,revision,file,&hash,&mut |_|Err("offline".into())).is_err());
        assert_eq!(super::pinned(&root,revision,file,&hash,&mut rule).unwrap(),expected);
        shared_files(&root,revision,&mut rule).unwrap();let tree=specq_dir(&root,revision).join("tree.json");
        std::fs::write(&tree,"{}").unwrap();std::fs::write(tree.with_extension("cache.json"),rule_cache(revision,"tree.json","{}").to_string()).unwrap();
        assert!(shared_files(&root,revision,&mut |_|Ok("{}".into())).is_err());assert_eq!(std::fs::read_to_string(&tree).unwrap(),"{}");
        assert_eq!(shared_files(&root,revision,&mut rule).unwrap().len(),SPECQ_CORE.len()+SPECQ_INDEX.len());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn advertisement(revision: &str) -> String {
        let line=format!("{revision} refs/heads/main\n");
        format!("001e# service=git-upload-pack\n0000{:04x}{line}0000",line.len()+4)
    }

    #[test]
    fn git_advertisement_requires_complete_unambiguous_main_reference() {
        let valid=advertisement(SPECQ_REVISION);assert_eq!(advertised_revision(&valid).unwrap(),SPECQ_REVISION);
        for cut in 0..valid.len() {assert!(advertised_revision(&valid[..cut]).is_err(),"truncation {cut}");}
        for bad in [valid.replace("refs/heads/main","refs/heads/main-other"),valid.replace(SPECQ_REVISION,&"z".repeat(40)),
            valid.clone()+"0001",valid.clone()+&valid,valid.replace("003d","ffff"),"0000".into()] {
            assert!(advertised_revision(&bad).is_err(),"{bad:?}");
        }
        let head=format!("{SPECQ_REVISION} HEAD\0multi_ack thin-pack\n");
        let with_head=format!("001e# service=git-upload-pack\n0000{:04x}{head}{}",head.len()+4,&valid[34..]);
        assert_eq!(advertised_revision(&with_head).unwrap(),SPECQ_REVISION);
        let other=format!("{SPECQ_REVISION} refs/heads/main-other\n");
        assert!(advertised_revision(&format!("001e# service=git-upload-pack\n0000{:04x}{other}0000",other.len()+4)).is_err());
    }

    #[test]
    fn shared_revision_refreshes_at_bound_and_never_falls_back_after_failure() {
        let root=rules_root("source-freshness");let first="a".repeat(40);let second="b".repeat(40);
        assert_eq!(shared_revision_at(&root,100,&mut |url|{assert_eq!(url,SPECQ_REFS);Ok(advertisement(&first))}).unwrap(),first);
        assert_eq!(shared_revision_at(&root,159,&mut |_|Err("offline".into())).unwrap(),first);
        assert!(shared_revision_at(&root,160,&mut |_|Err("offline".into())).unwrap_err().to_string().contains("freshness"));
        assert_eq!(shared_revision_at(&root,160,&mut |_|Ok(advertisement(&second))).unwrap(),second);
        assert!(shared_revision_at(&root,159,&mut |_|Err("future cache".into())).is_err());
        assert!(shared_revision_at(&root,220,&mut |_|Ok("invalid".into())).is_err());
        assert!(shared_revision_at(&root,220,&mut |_|Ok("x".repeat(RULES_CAP+1))).is_err());
        let path=root.join(".taskq/specq/source.json");
        for bad in ["{".to_string(),"{}".into(),serde_json::json!({"revision":"../escape","checked_at":100}).to_string(),"x".repeat(1025)] {
            std::fs::write(&path,bad).unwrap();assert!(shared_revision_at(&root,101,&mut |_|Err("offline".into())).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_moving_shared_head_cannot_mix_bodies_and_resume_gets_the_new_snapshot() {
        let root=rules_root("snapshot-consistency");let first="a".repeat(40);let second="b".repeat(40);let mut files=Vec::new();
        let initial=rules(&root,false,|url| {
            if url==SPECQ_REFS {return Ok(advertisement(&first));}
            files.push(url.to_string());
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            std::fs::create_dir_all(root.join(".taskq/specq")).unwrap();
            std::fs::write(root.join(".taskq/specq/source.json"),serde_json::json!({"revision":second,"checked_at":now}).to_string()).unwrap();
            rule(url)
        }).unwrap();
        assert!(initial.starts_with(&format!("TaskQ rules: specq@{first} ")));
        assert_eq!(files.len(),1+SPECQ_CORE.len()+SPECQ_INDEX.len());
        assert!(files.iter().all(|url|url.contains(&format!("/{first}/")) || url==&format!("{SPECQ_TREES}{first}?recursive=1")));
        let resumed=rules(&root,true,rule).unwrap();
        assert!(resumed.starts_with(&format!("TaskQ rules: specq@{second} ")));
        assert!(!resumed.contains(&format!("/{first}/")));
        assert_eq!(resumed.matches("\n--- specq/").count(),SPECQ_CORE.len());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn url(file: &str) -> String {
        format!("https://raw.githubusercontent.com/taskqorg/specq/{SPECQ_REVISION}/{file}")
    }

    // Chain: § 4 reviewer on a host with `default_runtime` codex (#1668). Risk: a codex reviewer gets other review rules,
    // access check or verdict commands (close, requeue, ask) than a claude reviewer.
    #[test]
    fn codex_reviewer_brief_matches_the_claude_reviewer() {
        let root = rules_root("codex-review");
        let env = Env { context: "", root: &root, exe: Path::new("/i/taskq"), install: Path::new("/i"), permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let l = |rt: &str| Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: "reviewer".into(), rt: rt.into(), session: "s".into(), url: "https://b/7".into(), resume: None };
        // Only the command prefix differs: the native Claude session runs in its host's environment (§ 8).
        let host_env = format!("env TASKQ_TASK=7 TASKQ_ATTEMPT=42 TASKQ_ROLE=reviewer TASKQ_ROOT={} ", root.display());
        let (claude, codex) = (brief(&env, &l("claude")), brief(&env, &l("codex")));
        assert!(claude.contains(&host_env));
        assert_eq!(claude.replace(&host_env, ""), codex);
        for verdict in ["close 7", "requeue 7", "ask 7"] { assert!(codex.contains(verdict), "{verdict}"); }
        std::fs::remove_dir_all(&root).ok();
    }

    // Chain: § 8 rule delivery, #1156. Risk: a new/resumed worker or reviewer misses the contract, a core rule or the index,
    // or a continuation retains obsolete mandatory bodies.
    #[test]
    fn rules_reach_every_runtime_role_and_resume() {
        let root = rules_root("roles");
        let env = Env { context: "## Goal\nPreserve intent\n## Acceptance\nA1. no loss\nTask notes (oldest first):\nNote 11: keep compatibility\nNote 16: clarify migration", root: &root, exe: Path::new("/i/taskq"), install: Path::new("/i"), permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let dir = specq_dir(&root,SPECQ_REVISION).display().to_string();
        for rt in RUNTIMES {
            for role in ["worker", "reviewer"] {
                for resume in [None, Some("Owner answer: preserve current work.\nLiteral $HOME stays text.".into())] {
                    let l = Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: role.into(), rt: rt.into(), session: "same-session".into(), url: "https://b/7".into(), resume };
                    let text = turn_text(&env, &l, rule).unwrap();
                    assert!(text.contains(env.context), "every role and continuation receives the complete task context");
                    assert!(text.contains("Before any hand-in") && text.contains("a conflicting note gets one ask immediately"));
                    let (rules, task) = text.split_once("\nTaskQ task\n").unwrap();
                    // header: revision, cache path, contract path (this release's URL: the root has no taskq.md), precedence
                    let header = rules.split("\n\n").next().unwrap();
                    assert!(header.contains(SPECQ_REVISION) && header.contains(&dir) && header.contains("https://github.com/taskqorg/taskq/blob/"));
                    assert!(header.contains("Precedence: the project contract overrides shared rules where they conflict."));
                    assert!(header.contains("\nEvidence: cite the applied rule as R<n> or § n; a fix records the failure before and the pass after."));
                    // one index line per specq index file and per docs Markdown file, at start and at resume
                    for file in SPECQ_INDEX { assert_eq!(rules.matches(&format!("\n- {file}: Rule from {}\n", url(file))).count(), 1, "{file}"); }
                    assert!(rules.contains("\n- docs/guide.md: Guide title\n- docs/sub/plain.md: (no heading)\n") && !rules.contains("shot.png"));
                    let bodies = rules.matches("\n--- ").count();
                    assert_eq!(bodies, 1 + SPECQ_CORE.len());
                    assert!(rules.contains(&format!("\n{CONTRACT}\n")), "{rt} {role}: whole current contract");
                    for file in SPECQ_CORE { assert_eq!(rules.matches(&format!("\n--- specq/{file} ---\n{}\n", rule(&url(file)).unwrap())).count(), 1); }
                    // #1834: the service's author marker is the last line; a resume still delivers the answer verbatim.
                    let inbox = turn_inbox(task).unwrap();
                    if let Some(answer) = &l.resume { assert!(task.contains(answer)); assert_eq!(inbox, vec![serde_json::json!({"kind": "answer", "text": answer})]); }
                    else { assert_eq!(task, format!("{}\n{}", brief(&env, &l), turn_marker(&[]))); assert!(inbox.is_empty()); }
                    assert_eq!(l.session, "same-session");
                    // Native sessions do not inherit the child env; without the prefix `result` is refused as a non-worker.
                    let prefix = if cfg!(windows) && rt == "codex-app" { "$env:TASKQ_TASK='7'; $env:TASKQ_ATTEMPT='42'; " } else { "env TASKQ_TASK=7 TASKQ_ATTEMPT=42 " };
                    assert_eq!(brief(&env, &l).contains(prefix), of(rt).remote || of(rt).app, "{rt} {role}: attempt identity");
                    assert!(task.contains(FILES) && task.contains(CI_WAIT));
                    if role == "worker" { assert!(brief(&env, &l).contains(SANDBOX_CHECKS) && brief(&env, &l).contains(DEV_LOOP)); }
                    // #1842: a worker publishes once; a moved main alone never sends it back to rebase or recheck.
                    assert_eq!(brief(&env, &l).contains(PUBLISH_ONCE), role == "worker", "{rt} {role}: publish once");
                    // cs#548 (#2078 A4): a read-only shared .git burned attempts on retries; the worker hands in one host gap
                    assert_eq!(brief(&env, &l).contains("do not retry: hand in once") && brief(&env, &l).contains("Problem: host gap: the worker sandbox cannot write the shared .git"),
                        role == "worker", "{rt} {role}: one exact host gap");
                    assert!(brief(&env, &l).contains("later 7 --blocker #M --text") && brief(&env, &l).contains("resumes when #M closes"), "{rt} {role}: a park names its blocker task");
                    assert!(brief(&env, &l).contains("later 7 --until <UTC> --text"), "{rt} {role}: a wait for elapsed time parks until it, not requeue (#1823)");
                    assert!(brief(&env, &l).contains("add `--stage A<k>` per finished item"), "{rt} {role}: a stage park names its flag (#1880)");
                    // the attempt log's evidence line
                    let log = root.join(format!(".taskq/{rt}-{role}.log"));
                    std::fs::remove_file(&log).ok();
                    rules_line(&log, "start", &text).unwrap();
                    let v: serde_json::Value = serde_json::from_str(std::fs::read_to_string(&log).unwrap().trim()).unwrap();
                    assert_eq!(v["shared_revision"],SPECQ_REVISION);
                    assert_eq!((v["type"].as_str(), v["header"].as_str(), v["bodies"].as_u64()), (Some("taskq.rules"), Some(header), Some(bodies as u64)));
                    assert_eq!(v["index"].as_u64(), Some(SPECQ_INDEX.len() as u64 + 2));
                }
            }
        }
        std::fs::remove_dir_all(&root).ok();
    }

    // Chain: section 7 step 4 test repair guidance (#1245). Risk: repairs rejected or weakened assertions accepted without a rule change.
    #[test]
    fn briefs_distinguish_test_repairs_from_weakened_checks() {
        let env = Env { context: "", root: Path::new("/project"), exe: Path::new("taskq"), install: Path::new("/i"), permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let rule = "Test changes follow specq engineering/testing.md: name the defect and retained risk protection in --text; change a rule only when accepted behavior or protection changes (section 7 step 4).";
        for rt in RUNTIMES {
            for role in ["worker", "reviewer"] {
                let l = Launch { files: String::new(), execution: None, task: 7, title: "Repair".into(), attempt: 42, role: role.into(), rt: rt.into(), session: "s".into(), url: "https://b/7".into(), resume: None };
                let text = brief(&env, &l);
                assert!(text.contains(rule), "{rt}: {role}");
                assert!(!text.contains("a changed or deleted existing test is a gap unless"), "{rt}: {role}: obsolete ban");
            }
        }
    }

    // Chain: §7 research Reports handoff (#1826). Risk: finished research blocks the queue, loses its report,
    // or a decision another task needs bypasses owner choice. The owner approved replacing the old ask gate.
    #[test]
    fn research_reports_close_without_answer_but_keep_blocking_decisions() {
        let env = Env { context: "", root: Path::new("/project"), exe: Path::new("taskq"), install: Path::new("/i"), permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        for rt in RUNTIMES {
            let mut l = Launch { files: String::new(), execution: None, task: 7, title: "Proposal".into(), attempt: 42, role: "worker".into(), rt: rt.into(), session: "s".into(), url: "https://b/7".into(), resume: None };
            let worker=brief(&env,&l);
            assert!(worker.contains("Experiments (`research` or a `Hypothesis:` line, any type) deliver docs/research/7.md by PR"), "{rt}: report artifact");
            assert!(worker.contains("Ask only when another task waits on that decision"), "{rt}: blocking decision");
            l.role = "reviewer".into();
            let reviewer=brief(&env,&l);
            assert!(reviewer.contains("need docs/research/<N>.md at the result SHA, not a prior owner answer, and a close line `Verdict: confirmed|refuted|inconclusive <evidence>`"), "{rt}: reviewer");
            assert!(reviewer.contains("Only a decision blocking another task needs ask before close"), "{rt}: blocking decision review");
        }
    }

    // Chain: § 8 rule delivery after compaction, #1211. Risk: a compacted Claude worker loses mandatory rule bodies
    // or task text again, or another runtime gets the hook.
    #[test]
    fn compaction_resends_header_and_index_only_to_claude() {
        let root = rules_root("compact");
        // fixture grant in a scoped install: hosts without enforcement refuse a missing grant (§ 8 Access)
        let install = root.join("install");
        std::fs::create_dir_all(&install).unwrap();
        for rt in ["claude", "codex"] { crate::policy::grant(&install, rt, "full", None, "op", 1).unwrap(); }
        let env = Env { context: "", root: &root, exe: Path::new("/i/taskq"), install: &install, permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let launch = |rt: &str| Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: "worker".into(), rt: rt.into(), session: "s".into(), url: "https://b/7".into(), resume: None };
        let args = |rt: &str| command(&env, &launch(rt), &access(&env, rt).unwrap()).unwrap().get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let l = launch("claude");
        let claude = args("claude");
        let settings: serde_json::Value = serde_json::from_str(&claude[claude.iter().position(|a| a == "--settings").unwrap() + 1]).unwrap();
        let hook = &settings["hooks"]["SessionStart"][0];
        assert_eq!(hook["matcher"], "compact");
        assert!(hook["hooks"][0]["command"].as_str().unwrap().ends_with(&format!(" rules compact '{}' 'worker' 7 42", root.display())));
        assert!(!args("codex").iter().any(|a| a == "--settings"), "codex gets no hook");
        // #1531: the per-user Claude daemon spawns from its own env; settings carry this launch's values.
        // Risk: a worker reads another launch's TASKQ_* (install, task, attempt), or an operator session gets an attempt.
        let want = |task: &str, attempt: &str, role: &str| serde_json::json!({"TASKQ_INSTALL": install.to_string_lossy(), "TASKQ_ROOT": root.to_string_lossy(), "TASKQ_TASK": task, "TASKQ_ATTEMPT": attempt, "TASKQ_ROLE": role});
        let mut worker = want("7", "42", "worker");
        worker["AGENTQ_OPTIMIZATION"] = std::env::var("AGENTQ_OPTIMIZATION").unwrap_or("inherit".into()).into();
        worker["AGENTQ_LAUNCHER"] = "taskq".into();
        if let Some(p) = build_cache_for("claude", &access(&env, "claude").unwrap(), &env) { worker["RUSTC_WRAPPER"] = p.to_string_lossy().into(); }
        assert_eq!(settings["env"], worker, "a full-access Claude worker gets this host's build cache, if any");
        let operator: serde_json::Value = serde_json::from_str(&compact_settings(&root, &install, &Launch { role: "operator".into(), ..launch("claude") }, None, None)).unwrap();
        assert_eq!(operator["env"], want("", "", ""), "operator: empty means unset");
        let cached: serde_json::Value = serde_json::from_str(&compact_settings(&root, &install, &launch("claude"), None, Some(Path::new("/b/sccache")))).unwrap();
        assert_eq!(cached["env"]["RUSTC_WRAPPER"], "/b/sccache", "the daemon's env is not the service's: settings carry the wrapper");
        // The compact hook restores current mandatory bodies and index, without inventing a task.
        let start = turn_text(&env, &l, rule).unwrap();
        let text = compact(&root, "worker", 7, 42).unwrap();
        assert!(text.starts_with("TaskQ rules: specq@") && text.contains("\nEvidence: cite the applied rule"));
        assert!(text.contains(INDEX) && text.contains("\n- docs/guide.md: Guide title\n"));
        assert!(!text.contains("TaskQ task") && text.contains("Apply this rule."), "current mandatory bodies, no task");
        assert_eq!(text.matches("\n--- ").count(),1+SPECQ_CORE.len());
        assert_eq!(text, rules(&root, true, rule).unwrap());
        assert!(start.len() > text.len());
        let log = std::fs::read_to_string(attempt_log(&root, "worker", 7, 42)).unwrap();
        let v: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
        assert_eq!((v["phase"].as_str(), v["bodies"].as_u64(), v["bytes"].as_u64()), (Some("compact"), Some((1+SPECQ_CORE.len()) as u64), Some(text.len() as u64)));
        std::fs::remove_dir_all(&root).ok();
    }

    // Chain: § 8 compile jobs, #1605. Risk: parallel workers each start one cargo job per CPU, thrash, and the load
    // gate holds new launches; or a Claude --bg session misses the bound because its daemon's env wins.
    #[test]
    fn launches_carry_the_host_compile_jobs_share() {
        let p = crate::host::Policy::parse(serde_json::json!({"limits": {"claude": 10}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let mut h = crate::host::Host::new(Ok(p));
        h.sync(Path::new("/a"), vec![(1, 11, "claude".into()), (2, 0, "codex".into())]);
        h.sync(Path::new("/b"), vec![(3, 33, "claude".into()), (4, 44, "claude".into())]);
        assert_eq!(h.occupied(), 4, "pending grants and children of every project");
        for (cpus, occupied, jobs) in [(16, 4, 4), (8, 10, 1), (8, 3, 2), (8, 1, 8), (8, 0, 8)] {
            assert_eq!(compile_jobs(Some(cpus), occupied), Some(jobs), "{cpus} CPUs, {occupied} slots");
        }
        assert_eq!(compile_jobs(None, 4), None, "unknown CPUs set nothing");
        let root = rules_root("jobs");
        let install = root.join("install");
        std::fs::create_dir_all(&install).unwrap();
        for rt in ["claude", "codex"] { crate::policy::grant(&install, rt, "full", None, "op", 1).unwrap(); }
        let launch = |rt: &str| Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: "worker".into(), rt: rt.into(), session: "s".into(), url: "https://b/7".into(), resume: None };
        for jobs in [compile_jobs(Some(16), h.occupied()), None] {
            let env = Env { context: "", root: &root, exe: Path::new("/i/taskq"), install: &install, permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs };
            let want = jobs.map(|j| j.to_string());
            for rt in ["claude", "codex"] {
                let c = command(&env, &launch(rt), &access(&env, rt).unwrap()).unwrap();
                let got = c.get_envs().find(|(k, _)| *k == "CARGO_BUILD_JOBS").and_then(|(_, v)| v).map(|v| v.to_string_lossy().into_owned());
                assert_eq!(got, want, "{rt} process env");
            }
            let settings: serde_json::Value = serde_json::from_str(&compact_settings(&root, &install, &launch("claude"), jobs, None)).unwrap();
            assert_eq!(settings["env"]["CARGO_BUILD_JOBS"].as_str().map(String::from), want, "Claude --bg settings env");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    // Chain: build cache (docs/testing.md#build-cache), #1604. Risk: a sandboxed child gets a wrapper whose server it
    // cannot reach and its build fails, or a host without sccache or with the owner's own wrapper gets one.
    #[test]
    fn build_cache_only_where_the_child_reaches_it() {
        let dir = std::env::temp_dir().join(format!("taskq-build-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join(if cfg!(windows) { "sccache.exe" } else { "sccache" });
        assert_eq!(build_cache_in(false, dir.as_os_str()), None, "no sccache: children unchanged");
        std::fs::write(&bin, "").unwrap();
        assert_eq!(build_cache_in(false, dir.as_os_str()), Some(bin.clone()));
        assert_eq!(build_cache_in(true, dir.as_os_str()), None, "an inherited wrapper stays the owner's");
        let env = Env { context: "", root: &dir, exe: Path::new("taskq"), install: &dir, permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let seatbelt = crate::policy::Effective { runtime: String::new(), requested: String::new(), grant: String::new(), mode: "legacy".into(), enforcement: String::new(),
            profile: Some("(version 1)".into()), linux: false, windows: false, claude: None, codex: None, refused: None };
        assert!(build_open("claude", &seatbelt, &env), "the seatbelt profile keeps user caches writable");
        assert!(!build_open("codex", &seatbelt, &env), "Codex defaults: its own sandbox is unknown");
        let ws = ["--sandbox".to_string(), "workspace-write".into()];
        assert!(!build_open("codex", &seatbelt, &Env { codex: &ws, ..env }));
        let full = crate::policy::Effective { codex: Some(vec!["-s".into(), "danger-full-access".into()]), ..seatbelt.clone() };
        assert!(build_open("codex", &full, &env));
        assert!(!build_open("codex", &crate::policy::Effective { windows: true, ..full.clone() }, &env), "taskq Windows sandbox");
        assert!(!build_open("codex", &crate::policy::Effective { linux: true, ..full }, &env), "taskq Linux sandbox");
        std::fs::remove_dir_all(&dir).ok();
    }

    // Chain: § 8 rule delivery to a PM session, #1219. Risk: the PM hook gets other rules than a worker at the same pin,
    // compaction loses mandatory bodies, or the committed hook names a local path or misses start/compact.
    #[test]
    fn pm_hook_gets_the_worker_start_rules() {
        let root = rules_root("pm");
        let env = Env { context: "", root: &root, exe: Path::new("/i/taskq"), install: Path::new("/i"), permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let l = Launch { files: String::new(), execution: None, task: 7, title: "T".into(), attempt: 42, role: "worker".into(), rt: "claude".into(), session: "s".into(), url: "https://b/7".into(), resume: None };
        let start = turn_text(&env, &l, rule).unwrap();
        assert_eq!(pm_rules(&root, "start", rule).unwrap(), start.split("\nTaskQ task\n").next().unwrap());
        assert_eq!(pm_rules(&root, "compact", rule).unwrap(), rules(&root, true, rule).unwrap());
        // the committed project hook: start on startup and clear, compact on compaction, no local path or secret
        let settings: serde_json::Value = serde_json::from_str(include_str!("../.claude/settings.json")).unwrap();
        let hooks = settings["hooks"]["SessionStart"].as_array().unwrap();
        for (matcher, phase) in [("startup|clear", "start"), ("compact", "compact")] {
            let cmd = hooks.iter().find(|h| h["matcher"] == matcher).unwrap()["hooks"][0]["command"].as_str().unwrap();
            assert!(cmd.ends_with(&format!(" rules pm {phase}")) && cmd.contains("$CLAUDE_PROJECT_DIR"), "{cmd}");
            assert!(!cmd.contains("/Users/") && !cmd.contains("token"), "{cmd}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    // Chain: § 8 rule delivery cache and refusals, #1156. Risk: a missing or unreadable pinned file, an unreadable doc or an
    // oversized payload starts a worker without its rules; or every start refetches the pin.
    #[test]
    fn registered_project_contract_overrides_taskq_and_missing_contract_refuses() {
        let root = rules_root("project-contract");
        std::fs::write(root.join("taskq.json"),r#"{"repo":"group/nested/serverq"}"#).unwrap();
        std::fs::write(root.join("taskq.md"),"# Wrong primary contract\n").unwrap();
        std::fs::write(root.join("serverq.md"),"# ServerQ contract\nProject rule overrides shared rule.\n").unwrap();
        let text = rule_text(&root,"task",false,rule).unwrap();
        assert!(text.contains(&format!("--- contract {} ---\n# ServerQ contract",root.join("serverq.md").display())));
        assert!(!text.contains("Wrong primary contract"));
        for resume in [false,true] {
            let text=rule_text(&root,"task",resume,rule).unwrap();
            assert!(text.contains(&format!("contract `{}`",root.join("serverq.md").display())));
        }
        std::fs::write(root.join("serverq.md"),"# Updated ServerQ contract\nNew current rule.\n").unwrap();
        let resumed=rule_text(&root,"task",true,rule).unwrap();
        assert!(resumed.contains("New current rule.") && !resumed.contains("Project rule overrides shared rule."));
        std::fs::remove_file(root.join("serverq.md")).unwrap();
        for resume in [false,true] {assert!(rule_text(&root,"task",resume,rule).is_err());}
        for config in ["{",r#"{"repo":"group/.."}"#,r#"{"repo":"group/"}"#,r#"{}"#] {
            std::fs::write(root.join("taskq.json"),config).unwrap();
            assert!(rule_text(&root,"task",false,rule).is_err());
        }
        std::fs::write(root.join("taskq.json"),r#"{"repo":"taskqorg/taskq"}"#).unwrap();
        assert!(rule_text(&root,"task",false,rule).unwrap().contains("Wrong primary contract"));
        std::fs::remove_file(root.join("taskq.md")).unwrap();
        assert!(rule_text(&root,"task",false,rule).unwrap().contains(CONTRACT));
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: § 7 rule 2 worker base -> § 8 rule delivery, #1479. Risk: a root behind origin or with a local edit hands the
    // worker a contract other than the one its branch starts from.
    #[test]
    fn contract_comes_from_fetched_origin_main_not_the_root_tree() {
        let root = rules_root("base-commit");
        let up = root.with_file_name(format!("{}-up", root.file_name().unwrap().to_str().unwrap()));
        std::fs::remove_dir_all(&up).ok();
        let git = |dir: &Path, args: &[&str]| {
            let o = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
            assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        std::fs::write(root.join("taskq.json"), r#"{"repo":"group/serverq"}"#).unwrap();
        std::fs::write(root.join("serverq.md"), "# Stale root contract\n").unwrap();
        std::fs::write(root.join("taskq.md"), "# Wrong root taskq contract\n").unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "old"]);
        std::fs::create_dir_all(&up).unwrap();
        git(&up, &["clone", "-q", root.to_str().unwrap(), "."]);
        std::fs::write(up.join("serverq.md"), "# Fresh origin contract\n").unwrap();
        git(&up, &["commit", "-q", "-am", "new"]);
        let fresh = git(&up, &["rev-parse", "HEAD"]);
        git(&root, &["remote", "add", "origin", up.to_str().unwrap()]);
        std::fs::write(root.join("serverq.md"), "# Local root edit\n").unwrap();
        for resume in [false, true] {
            let text = rule_text(&root, "task", resume, rule).unwrap();
            assert!(text.contains(&format!("--- contract {} at origin/main {fresh} ---\n# Fresh origin contract", root.join("serverq.md").display())), "{resume}");
            assert!(text.contains(&format!("contract `{} at origin/main {fresh}`", root.join("serverq.md").display())));
            assert!(!text.contains("Stale root contract") && !text.contains("Local root edit"));
        }
        // #1946: `taskq rules size` checks the contract a branch proposes, before it reaches origin/main
        let tree = bundle(&root, true, true, rule).unwrap();
        assert!(tree.contains("# Local root edit") && !tree.contains("Fresh origin contract"));
        assert_eq!(rules_size_with(&root, rule).unwrap().0, tree.len());
        git(&up, &["rm", "-q", "serverq.md", "taskq.md"]);
        git(&up, &["commit", "-q", "-m", "drop"]);
        assert!(rule_text(&root, "task", false, rule).is_err(), "a registered contract absent at the base refuses");
        std::fs::write(root.join("taskq.json"), r#"{"repo":"taskqorg/taskq"}"#).unwrap();
        let text = rule_text(&root, "task", false, rule).unwrap();
        assert!(text.contains(CONTRACT) && !text.contains("Wrong root taskq contract"), "absent at the base: this release's contract");
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(up).unwrap();
    }
    #[test]
    fn pinned_cache_rejects_changed_utf8_wrong_identity_and_partial_pairs() {
        assert_eq!(rule_cache(SPECQ_REVISION,"fixture","abc")["sha256"],"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let root=rules_root("cache-integrity");let file=SPECQ_CORE[0];let path=specq_dir(&root,SPECQ_REVISION).join(file);
        let expected=pinned(&root,SPECQ_REVISION,file,&mut rule).unwrap();
        std::fs::write(&path,"# Changed valid UTF-8 rule\n").unwrap();
        assert!(pinned(&root,SPECQ_REVISION,file,&mut |_|Err("offline".into())).is_err(),"altered nonempty cache must not be accepted");
        assert_eq!(pinned(&root,SPECQ_REVISION,file,&mut rule).unwrap(),expected);
        let metadata=path.with_extension("cache.json");
        for key in ["revision","file","sha256"] {
            let mut state=rule_cache(SPECQ_REVISION,file,&expected);state[key]=serde_json::json!("wrong");
            std::fs::write(&metadata,state.to_string()).unwrap();
            assert!(pinned(&root,SPECQ_REVISION,file,&mut |_|Err("offline".into())).is_err(),"{key}");
            assert_eq!(pinned(&root,SPECQ_REVISION,file,&mut rule).unwrap(),expected);
        }
        for bad in [None,Some("{"),Some("")] {
            if let Some(text)=bad {std::fs::write(&metadata,text).unwrap();} else {std::fs::remove_file(&metadata).unwrap();}
            assert!(pinned(&root,SPECQ_REVISION,file,&mut |_|Err("offline".into())).is_err());
            assert_eq!(pinned(&root,SPECQ_REVISION,file,&mut rule).unwrap(),expected);
        }
        std::fs::write(&path,"x".repeat(RULES_CAP+1)).unwrap();
        assert!(pinned(&root,SPECQ_REVISION,file,&mut |_|Err("offline".into())).is_err());
        assert!(pinned(&root,SPECQ_REVISION,file,&mut |_|Ok("x".repeat(RULES_CAP+1))).is_err());
        assert_eq!(pinned(&root,SPECQ_REVISION,file,&mut rule).unwrap(),expected);
        assert_eq!(pinned(&root,SPECQ_REVISION,file,&mut |_|Err("must use verified cache".into())).unwrap(),expected);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pinned_cache_parallel_repairs_publish_complete_files_without_collisions() {
        let root=rules_root("cache-concurrent");let barrier=std::sync::Arc::new(std::sync::Barrier::new(8));let file=SPECQ_CORE[0];
        std::thread::scope(|scope| {
            let threads=(0..8).map(|_| {let root=&root;let barrier=barrier.clone();scope.spawn(move||pinned(root,SPECQ_REVISION,file,&mut |url|{barrier.wait();rule(url)}))}).collect::<Vec<_>>();
            for thread in threads {assert_eq!(thread.join().unwrap().unwrap(),rule(&url(file)).unwrap());}
        });
        assert_eq!(pinned(&root,SPECQ_REVISION,file,&mut |_|Err("must use verified cache".into())).unwrap(),rule(&url(file)).unwrap());
        assert_eq!(std::fs::read_dir(specq_dir(&root,SPECQ_REVISION).join("engineering")).unwrap().count(),2,"no staging files left");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rules_come_from_the_pin_cache_and_refuse_when_unreadable() {
        let root = rules_root("cache");
        std::fs::write(root.join("taskq.md"), "# Project contract\nProject rule.\n").unwrap();
        let mut fetched = Vec::new();
        let text = rule_text(&root, "task", false, |u| { fetched.push(u.to_string()); rule(u) }).unwrap();
        assert_eq!(fetched.len(), 2 + SPECQ_CORE.len() + SPECQ_INDEX.len(), "source reference, tree and each pinned file once");
        assert!(text.contains(&format!("--- contract {} ---\n# Project contract\nProject rule.\n", root.join("taskq.md").display())));
        for file in SPECQ_CORE.iter().chain(SPECQ_INDEX) {
            assert!(fetched.contains(&url(file)));
            assert_eq!(std::fs::read_to_string(specq_dir(&root,SPECQ_REVISION).join(file)).unwrap(), rule(&url(file)).unwrap());
        }
        for resume in [false, true] {
            assert!(rule_text(&root, "task", resume, |u| Err(format!("refetched {u}"))).is_ok(), "the cache serves the pin");
        }
        // a cached file that went bad is fetched again; a failed fetch refuses
        std::fs::write(specq_dir(&root,SPECQ_REVISION).join(SPECQ_CORE[3]), [0xffu8, 0xfe]).unwrap();
        std::fs::write(specq_dir(&root,SPECQ_REVISION).join(SPECQ_INDEX[0]), "").unwrap();
        for resume in [false, true] {
            for bad in [Err("offline".to_string()), Ok(" \n".to_string())] {
                let e = rule_text(&root, "task", resume, |_| bad.clone()).unwrap_err().to_string();
                assert!(e.contains(SPECQ_CORE[3]), "{e}");
            }
        }
        let mut again = Vec::new();
        rule_text(&root, "task", true, |u| { again.push(u.to_string()); rule(u) }).unwrap();
        assert_eq!(again, [url(SPECQ_CORE[3]), url(SPECQ_INDEX[0])]);
        // an unreadable doc, contract or an over-cap payload refuses
        std::fs::write(root.join("docs/sub/bad.md"), [0xffu8]).unwrap();
        assert!(rule_text(&root, "task", true, rule).unwrap_err().to_string().contains("docs/sub/bad.md"));
        std::fs::remove_file(root.join("docs/sub/bad.md")).unwrap();
        std::fs::write(root.join("taskq.md"), [0xffu8]).unwrap();
        assert!(rule_text(&root, "task", false, rule).is_err());
        std::fs::write(root.join("taskq.md"), "x".repeat(RULES_CAP)).unwrap();
        assert!(rule_text(&root, "task", false, rule).unwrap_err().to_string().contains("cap"));
        assert!(rule_text(&root, "task", true, rule).unwrap_err().to_string().contains("cap"), "resume also enforces the mandatory-body cap");
        std::fs::remove_dir_all(&root).ok();
    }

    // Chain: § 8 rule delivery, #1946 (pilotq#169). Risk: a contract edit over the cap merges unnoticed and every worker
    // start then fails until the owner trims it; the size check must fail first and the start refusal must name the file.
    #[test]
    fn rules_size_reports_headroom_and_the_cap_refusal_names_the_contract() {
        let root = rules_root("size");
        std::fs::write(root.join("taskq.json"), r#"{"repo":"group/pilotq"}"#).unwrap();
        let contract = "# Project contract\nProject rule.\n";
        std::fs::write(root.join("pilotq.md"), contract).unwrap();
        let (len, cap) = rules_size_with(&root, rule).unwrap();
        assert!(cap == RULES_CAP && len < cap && len > contract.len());
        let over = cap - len + 100;
        std::fs::write(root.join("pilotq.md"), format!("{contract}{}", "x".repeat(over))).unwrap();
        let e = rules_size_with(&root, rule).unwrap_err();
        assert_eq!(e.class(), "rules-cap");
        assert_eq!(e.to_string(), format!("rules bundle {} bytes, 100 over the {cap}-byte cap\nNext: trim pilotq.md below the cap, then retry", cap + 100));
        for resume in [false, true] {
            let e = rule_text(&root, "task", resume, rule).unwrap_err();
            assert!(e.class() == "rules-cap" && e.no_start && !e.reportable, "{resume}: {e}");
            assert!(e.to_string().contains("trim pilotq.md") && !e.to_string().contains(&root.display().to_string()), "no local path (§ 7 rule 6): {e}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

}
