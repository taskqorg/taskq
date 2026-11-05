//! One sandbox per daemon (#2432, docs/design/worker-permissions.md#one-sandbox-per-daemon): a macOS job under a shared
//! per-account daemon inherits the daemon's Seatbelt, so it launches only when that Seatbelt is its own grant's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long a waiting job holds new starts of other grants after it last waited.
const HOLD: Duration = Duration::from_secs(600);
/// Readback and launch stay together across the host's projects, so two grants cannot race an idle daemon.
static LAUNCH: Mutex<()> = Mutex::new(());
/// The first waiting grant of each account home, until its hold lapses.
static WAITERS: Mutex<BTreeMap<PathBuf, (String, Instant)>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
thread_local! {
    static FAKE: std::cell::RefCell<Option<Fit>> = const { std::cell::RefCell::new(None) };
}

/// The live daemon against one job's grant. `Unknown` holds (R12); its text is fixed and names no path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fit {
    /// No live daemon: this launch starts it under the job's own profile.
    None,
    Fits,
    Differs { pid: u32, reason: &'static str },
    Unknown(String),
}

/// Native readback of a live process's sandbox.
pub trait Seatbelt {
    fn confined(&self, pid: u32) -> Result<bool, String>;
    fn may_read(&self, pid: u32, path: &Path) -> Result<bool, String>;
}

struct Native;

impl Seatbelt for Native {
    fn confined(&self, pid: u32) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        { mac::confined(pid) }
        #[cfg(not(target_os = "macos"))]
        { let _ = pid; Err("sandbox readback exists only on macOS".into()) }
    }

    fn may_read(&self, pid: u32, path: &Path) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        { mac::may_read(pid, path) }
        #[cfg(not(target_os = "macos"))]
        { let _ = (pid, path); Err("sandbox readback exists only on macOS".into()) }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

    // Sandbox SPI declarations match WebKit's Source/WTF/wtf/spi/darwin/SandboxSPI.h.
    #[link(name = "sandbox")]
    unsafe extern "C" {
        fn sandbox_check(pid: i32, operation: *const libc::c_char, filter: i32, ...) -> i32;
        static SANDBOX_CHECK_NO_REPORT: i32;
    }

    pub fn confined(pid: u32) -> Result<bool, String> {
        match unsafe { sandbox_check(pid as i32, std::ptr::null(), 0) } {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("daemon sandbox readback failed".into()),
        }
    }

    pub fn may_read(pid: u32, path: &Path) -> Result<bool, String> {
        let op = CString::new("file-read-data").map_err(|_| "invalid sandbox operation")?;
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| "invalid sandbox path")?;
        match unsafe { sandbox_check(pid as i32, op.as_ptr(), 1 | SANDBOX_CHECK_NO_REPORT, path.as_ptr()) } {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err("daemon sandbox readback failed".into()),
        }
    }
}

/// `profile` None is a Full job: it fits only an unconfined daemon.
pub fn fit(daemon: Result<Option<u32>, String>, profile: Option<&str>, s: &dyn Seatbelt) -> Fit {
    let pid = match daemon {
        Ok(None) => return Fit::None,
        Ok(Some(pid)) => pid,
        Err(e) => return Fit::Unknown(e),
    };
    let differs = |reason| Fit::Differs { pid, reason };
    let Some(profile) = profile else {
        return match s.confined(pid) {
            Ok(false) => Fit::Fits,
            Ok(true) => differs("daemon runs under a Seatbelt profile, job has a Full grant"),
            Err(e) => Fit::Unknown(e),
        };
    };
    let Some((allow, deny)) = crate::policy::identity(profile) else { return Fit::Unknown("job profile has no identity".into()) };
    match (s.may_read(pid, &allow), s.may_read(pid, &deny)) {
        (Ok(true), Ok(false)) => Fit::Fits,
        (Ok(_), Ok(_)) => match s.confined(pid) {
            Ok(false) => differs("daemon runs unconfined, job has a Seatbelt grant"),
            Ok(true) => differs("daemon runs under another Seatbelt profile"),
            Err(e) => Fit::Unknown(e),
        },
        (Err(e), _) | (_, Err(e)) => Fit::Unknown(e),
    }
}

/// `<bin> daemon status`: `not running`, or a `pid:` line among others.
pub fn parse_status(out: &str) -> Result<Option<u32>, String> {
    if out.lines().map(str::trim).find(|l| !l.is_empty()) == Some("not running") { return Ok(None); }
    out.lines().find_map(|l| l.trim().strip_prefix("pid:")?.trim().parse::<u32>().ok()).filter(|pid| *pid > 1).map(Some).ok_or_else(|| "daemon status unreadable".into())
}

/// The live daemon of `home` against the job's `profile`. Only macOS has this inheritance.
pub fn read(rt: &str, home: Option<&Path>, profile: Option<&str>) -> Fit {
    #[cfg(test)]
    if let Some(fit) = FAKE.with(|f| f.borrow().clone()) { return fit; }
    if cfg!(test) || !cfg!(target_os = "macos") { return Fit::None; }
    let bin = super::which(super::of(rt).bin);
    if !bin.is_absolute() { return Fit::None; } // the launch reports its missing CLI
    let Some(home) = home else { return Fit::Unknown("account home unknown".into()) };
    let out = super::probe_in("runtime.daemon-sandbox", &bin, &["daemon", "status"], super::accounts::environment(rt, home));
    fit(out.ok_or_else(|| "daemon status did not answer".to_string()).and_then(|(_, text)| parse_status(&text)), profile, &Native)
}

/// Why a job waits: its daemon's state, or another grant holding that daemon so it can drain. A resume of a live
/// attempt passes the hold: it keeps its own daemon busy only while it runs.
pub fn verdict(waiter: Option<&str>, grant: &str, fit: &Fit, resume: bool) -> Result<(), String> {
    match fit {
        Fit::Unknown(e) => Err(format!("daemon sandbox unknown: {e}")),
        Fit::Differs { pid, reason } => Err(format!("live daemon pid {pid}: {reason}")),
        Fit::None | Fit::Fits if !resume && waiter.is_some_and(|w| w != grant) => Err("another grant waits for this daemon to drain".into()),
        _ => Ok(()),
    }
}

fn slot(home: Option<&Path>) -> PathBuf {
    let home = home.unwrap_or(Path::new(""));
    // parallel tests share these statics: each test thread has its own daemons
    if cfg!(test) { home.join(format!("{:?}", std::thread::current().id())) } else { home.to_path_buf() }
}

fn waiters() -> MutexGuard<'static, BTreeMap<PathBuf, (String, Instant)>> {
    WAITERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// [`verdict`] under the current holder of `home`.
pub fn check(home: Option<&Path>, profile: Option<&str>, resume: bool, fit: &Fit) -> Result<(), String> {
    let mut map = waiters();
    let slot = slot(home);
    if map.get(&slot).is_some_and(|w| w.1 <= Instant::now()) { map.remove(&slot); }
    verdict(map.get(&slot).map(|w| w.0.as_str()), profile.unwrap_or(""), fit, resume)
}

/// A job truly waits on `home`: the first waiting grant holds it, and each wait renews that hold.
pub fn wait(home: Option<&Path>, profile: Option<&str>) {
    let (mut map, grant, until) = (waiters(), profile.unwrap_or("").to_string(), Instant::now() + HOLD);
    let slot = slot(home);
    if map.get(&slot).is_none_or(|w| w.0 == grant || w.1 <= Instant::now()) { map.insert(slot, (grant, until)); }
}

/// Held across one launch. Dropping it ends this grant's wait.
pub struct Gate {
    _lock: MutexGuard<'static, ()>,
    home: Option<PathBuf>,
    grant: String,
}

impl Drop for Gate {
    fn drop(&mut self) {
        let mut map = waiters();
        let slot = slot(self.home.as_deref());
        if map.get(&slot).is_some_and(|w| w.0 == self.grant) { map.remove(&slot); }
    }
}

/// Before a daemon-owned launch. `Err` starts nothing: the caller holds the launch and retries it on a later pass.
pub fn enter(rt: &str, home: Option<&Path>, profile: Option<&str>, resume: bool) -> Result<Option<Gate>, String> {
    if !(cfg!(target_os = "macos") || cfg!(test)) { return Ok(None); }
    enter_with(home, profile, resume, || read(rt, home, profile)).map(Some)
}

fn enter_with(home: Option<&Path>, profile: Option<&str>, resume: bool, read: impl FnOnce() -> Fit) -> Result<Gate, String> {
    let lock = LAUNCH.lock().unwrap_or_else(|e| e.into_inner());
    let fit = read();
    if let Err(e) = check(home, profile, resume, &fit) {
        if matches!(fit, Fit::Differs { .. }) { wait(home, profile); }
        return Err(e);
    }
    Ok(Gate { _lock: lock, home: home.map(Path::to_path_buf), grant: profile.unwrap_or("").into() })
}

/// Tests replace this thread's readback; `None` restores it.
#[cfg(test)]
pub fn fake(fit: Option<Fit>) {
    FAKE.with(|f| *f.borrow_mut() = fit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{self, Effective, Grant, Policy};
    use std::cell::RefCell;

    /// A daemon that holds the Seatbelt profile of the job that started it. It evaluates a taskq profile by its rules:
    /// denies take precedence over allows, unmatched operations are allowed.
    #[derive(Default)]
    struct Sim(RefCell<Option<(u32, Option<String>)>>);

    fn permits(profile: Option<&str>, operation: &str, path: &str) -> bool {
        let Some(profile) = profile else { return true };
        let under = |p: &str| path == p || path.strip_prefix(p).is_some_and(|r| r.starts_with(['/', '\\']));
        let quoted = |rule: &str, kind: &str| -> Vec<String> {
            rule.split(&format!("({kind} \"")).skip(1).filter_map(|r| {
                let mut out = String::new();
                let mut chars = r.chars();
                while let Some(c) = chars.next() {
                    match c { '"' => return Some(out), '\\' => out.extend(chars.next()), c => out.push(c) }
                }
                None
            }).collect()
        };
        let mut allowed = true;
        for rule in profile.lines() {
            let Some(rule) = rule.strip_prefix(&format!("(deny {operation} ")) else { continue };
            if rule.starts_with("(require-not") {
                if !quoted(rule, "subpath").iter().any(|p| under(p)) { allowed = false; }
            } else if rule.starts_with("(subpath") {
                if quoted(rule, "subpath").iter().any(|p| under(p)) { allowed = false; }
            } else if rule.starts_with("(literal") {
                if quoted(rule, "literal").iter().any(|p| p == path) { allowed = false; }
            }
        }
        allowed
    }

    impl Seatbelt for Sim {
        fn confined(&self, _: u32) -> Result<bool, String> {
            Ok(self.0.borrow().as_ref().is_some_and(|d| d.1.is_some()))
        }
        fn may_read(&self, _: u32, path: &Path) -> Result<bool, String> {
            Ok(permits(self.0.borrow().as_ref().and_then(|d| d.1.as_deref()), "file-read*", &path.display().to_string()))
        }
    }

    impl Sim {
        fn enter(&self, profile: Option<&str>, resume: bool) -> Result<Gate, String> {
            enter_with(None, profile, resume, || fit(Ok(self.0.borrow().as_ref().map(|d| d.0)), profile, self))
        }
        /// One job launch: the daemon starts on demand under the job's own profile. Returns the job's write limits.
        fn launch(&self, profile: Option<&str>) -> Result<Option<String>, String> {
            let _gate = self.enter(profile, false)?;
            let mut daemon = self.0.borrow_mut();
            Ok(daemon.get_or_insert_with(|| (4242, profile.map(String::from))).1.clone())
        }
        fn exit(&self) { *self.0.borrow_mut() = None; }
    }

    struct Projects { granted: PathBuf, ungranted: PathBuf, granted_access: Effective, ungranted_access: Effective, dir: PathBuf }

    fn projects() -> Projects {
        // OS temp is shared writable space on macOS, so it cannot model separate project write limits.
        let dir = std::env::current_dir().unwrap().join(".taskq").join(format!("daemon-sandbox-{}-{:?}", std::process::id(), std::thread::current().id()));
        let (install, home, granted, ungranted) = (dir.join("install"), dir.join("home"), dir.join("granted"), dir.join("ungranted"));
        for d in [&install, &home, &granted, &ungranted] { std::fs::create_dir_all(d).unwrap(); }
        let (granted, ungranted) = (granted.canonicalize().unwrap(), ungranted.canonicalize().unwrap());
        let grant = Grant { mode: "workspace".into(), runtime: "claude".into(), project: Some(granted.display().to_string()), by: "owner".into(), at: 1 };
        let store: Result<Policy, String> = Ok(Policy { revision: 1, grants: vec![grant] });
        let of = |root: &Path| policy::resolve(&store, &policy::Host { os: "macos", install: &install, root, home: &home }, "claude", "dontAsk", &[]);
        Projects { granted_access: of(&granted), ungranted_access: of(&ungranted), granted, ungranted, dir }
    }

    // Chain: § 8 Access, R9, #2432 A1. Risk: the first `--bg` launch of one project sets the Seatbelt of every later job in
    // the shared daemon: an ungranted project loses its worktree writes, or a granted project runs without its limits.
    #[test]
    fn a_jobs_write_limits_come_from_its_own_grant_whichever_project_started_the_daemon() {
        let p = projects();
        let (g, u) = (p.granted_access.profile.as_deref().unwrap(), p.ungranted_access.profile.as_deref().unwrap());
        let (g_root, u_root) = (p.granted.display().to_string(), p.ungranted.display().to_string());
        assert_ne!(g, u);
        // The defect this gate closes: under the other project's profile each job has the wrong limits.
        assert!(!permits(Some(g), "file-write*", &u_root), "the ungranted project would lose its worktree writes");
        assert!(permits(Some(u), "file-write*", &g_root) && permits(Some(u), "file-write*", &u_root), "the granted project would run without its limits");

        // The granted project starts the daemon.
        let sim = Sim::default();
        let limits = sim.launch(Some(g)).unwrap();
        assert_eq!(limits.as_deref(), Some(g));
        assert!(permits(limits.as_deref(), "file-write*", &g_root) && !permits(limits.as_deref(), "file-write*", &u_root));
        assert_eq!(sim.launch(Some(g)).unwrap().as_deref(), Some(g), "the same grant shares its daemon");
        assert_eq!(sim.launch(Some(u)).unwrap_err(), "live daemon pid 4242: daemon runs under another Seatbelt profile");
        assert_eq!(sim.0.borrow().as_ref().unwrap().1.as_deref(), Some(g), "a refused job starts nothing");
        sim.exit();

        // The ungranted project starts it next: both orders hold.
        let limits = sim.launch(Some(u)).unwrap();
        assert!(permits(limits.as_deref(), "file-write*", &u_root));
        assert!(sim.launch(Some(g)).unwrap_err().contains("another Seatbelt profile"), "the granted job never runs under the wider profile");
        std::fs::remove_dir_all(&p.dir).ok();
    }

    // Chain: R9, #2432. Risk: a Full job counts as a fit for a sandboxed daemon, or a sandboxed job for an unconfined
    // daemon, which runs it with no limits at all.
    #[test]
    fn full_and_sandboxed_grants_never_share_a_daemon() {
        let p = projects();
        let g = p.granted_access.profile.as_deref().unwrap();
        let sim = Sim::default();
        assert_eq!(sim.launch(None).unwrap(), None, "Full starts it unconfined");
        assert_eq!(sim.launch(None).unwrap(), None);
        assert_eq!(sim.launch(Some(g)).unwrap_err(), "live daemon pid 4242: daemon runs unconfined, job has a Seatbelt grant");
        sim.exit();
        sim.launch(Some(g)).unwrap();
        assert_eq!(sim.launch(None).unwrap_err(), "live daemon pid 4242: daemon runs under a Seatbelt profile, job has a Full grant");
        std::fs::remove_dir_all(&p.dir).ok();
    }

    // Chain: R12, #2432. Risk: an unreadable daemon or sandbox reads as a fit and a job gets another project's limits.
    #[test]
    fn an_unverified_daemon_holds_the_launch() {
        struct Broken;
        impl Seatbelt for Broken {
            fn confined(&self, _: u32) -> Result<bool, String> { Err("daemon sandbox readback failed".into()) }
            fn may_read(&self, _: u32, _: &Path) -> Result<bool, String> { Err("daemon sandbox readback failed".into()) }
        }
        let p = projects();
        let g = p.granted_access.profile.as_deref();
        let unknown = Fit::Unknown("daemon sandbox readback failed".into());
        assert_eq!(fit(Ok(Some(9)), g, &Broken), unknown);
        assert_eq!(fit(Ok(Some(9)), None, &Broken), unknown);
        assert_eq!(fit(Err("daemon status unreadable".into()), g, &Broken), Fit::Unknown("daemon status unreadable".into()));
        assert_eq!(fit(Ok(None), g, &Broken), Fit::None, "no daemon: nothing to read");
        assert_eq!(fit(Ok(Some(9)), Some("(version 1)"), &Sim::default()), Fit::Unknown("job profile has no identity".into()));
        assert!(enter_with(None, g, false, || unknown.clone()).is_err_and(|e| e.starts_with("daemon sandbox unknown")));
        assert!(enter_with(None, g, true, || unknown.clone()).is_err(), "a resume holds too");
        std::fs::remove_dir_all(&p.dir).ok();
    }

    // Chain: § 4 selection, #2432. Risk: a busy project keeps the daemon alive forever and a waiting grant starves, or a
    // waiter that went away blocks the daemon for good, or the hold cuts a live attempt's resume.
    #[test]
    fn a_waiting_grant_drains_the_daemon() {
        let (a, b) = (Some("grant a"), Some("grant b"));
        let differs = Fit::Differs { pid: 7, reason: "daemon runs under another Seatbelt profile" };
        assert!(enter_with(None, b, false, || differs.clone()).is_err());
        assert_eq!(check(None, a, false, &Fit::Fits).unwrap_err(), "another grant waits for this daemon to drain");
        assert_eq!(check(None, a, false, &Fit::None).unwrap_err(), "another grant waits for this daemon to drain");
        assert_eq!(check(None, a, true, &Fit::Fits), Ok(()), "a resume of a live attempt continues");
        assert_eq!(check(None, b, false, &Fit::None), Ok(()), "the waiter itself goes first");
        wait(None, a);
        assert_eq!(check(None, a, false, &Fit::None).unwrap_err(), "another grant waits for this daemon to drain", "the first waiter keeps the hold");
        // The daemon exits: the waiter launches, its gate ends the hold, and the other grant may wait in turn.
        drop(enter_with(None, b, false, || Fit::None).unwrap());
        assert_eq!(check(None, a, false, &Fit::None), Ok(()));
        // A waiter that never returns loses its hold when the lease ends.
        wait(None, b);
        waiters().insert(slot(None), ("grant b".into(), Instant::now()));
        assert_eq!(check(None, a, false, &Fit::Fits), Ok(()));
        assert!(waiters().get(&slot(None)).is_none());
    }

    // Chain: #2432. Risk: Claude's status wording drifts and the readback invents a pid or reads no daemon as a live one.
    #[test]
    fn daemon_status_output_parses() {
        assert_eq!(parse_status("not running\n"), Ok(None));
        assert_eq!(parse_status("pid:     41036\nversion: 2.1.318\nuptime:  5m\norigin:  transient\n"), Ok(Some(41036)));
        assert_eq!(parse_status("error: unknown command 'daemon'\n"), Err("daemon status unreadable".to_string()));
        assert_eq!(parse_status(""), Err("daemon status unreadable".to_string()));
        assert_eq!(parse_status("pid: 1\n"), Err("daemon status unreadable".to_string()));
    }

    // Chain: § 8 Access, #2432 A1 native proof. Risk: `sandbox_check` reads differ from a real sandboxed process, the
    // marker rules widen the grant, or the profile limits do not hold under `sandbox-exec`.
    #[cfg(target_os = "macos")]
    #[test]
    fn native_readback_reads_a_real_seatbelt() {
        use std::{io::Write, process::Command};
        let probe = Command::new("sandbox-exec").args(["-p", "(version 1)(allow default)", "/usr/bin/true"]).output().unwrap();
        if !probe.status.success() && String::from_utf8_lossy(&probe.stderr).contains("Operation not permitted") {
            writeln!(std::io::stderr(), "sandbox-blocked: native_readback_reads_a_real_seatbelt: Operation not permitted").unwrap();
            return;
        }
        assert!(probe.status.success(), "sandbox-exec probe failed: {probe:?}");
        let p = projects();
        let (a, b) = (p.granted_access.profile.as_deref().unwrap(), p.ungranted_access.profile.as_deref().unwrap());
        assert_ne!(a, b);
        for (own, other) in [(a, b), (b, a)] {
            let mut daemon = Command::new("sandbox-exec").args(["-p", own, "/bin/sleep", "30"]).spawn().unwrap();
            let pid = daemon.id();
            let begun = Instant::now();
            while Native.confined(pid) != Ok(true) && begun.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(50)); }
            let confined = Native.confined(pid);
            let fits = fit(Ok(Some(pid)), Some(own), &Native);
            let differs = fit(Ok(Some(pid)), Some(other), &Native);
            let full = fit(Ok(Some(pid)), None, &Native);
            daemon.kill().ok();
            daemon.wait().ok();
            assert_eq!(confined, Ok(true));
            assert_eq!(fits, Fit::Fits);
            assert_eq!(differs, Fit::Differs { pid, reason: "daemon runs under another Seatbelt profile" });
            assert!(matches!(full, Fit::Differs { .. }), "a Full job does not fit");
        }
        let mut plain = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let full = fit(Ok(Some(plain.id())), None, &Native);
        let differs = fit(Ok(Some(plain.id())), Some(a), &Native);
        plain.kill().ok();
        plain.wait().ok();
        assert_eq!(full, Fit::Fits, "Full fits an unconfined process");
        assert!(matches!(differs, Fit::Differs { reason: "daemon runs unconfined, job has a Seatbelt grant", .. }));
        let touch = |profile: &str, path: &Path| Command::new("sandbox-exec").args(["-p", profile, "/usr/bin/touch"]).arg(path).status().unwrap().success();
        assert!(touch(a, &p.granted.join("own")), "own worktree stays writable");
        assert!(!touch(a, &p.ungranted.join("other")), "another project's worktree is closed");
        assert!(touch(b, &p.ungranted.join("own")), "the ungranted project keeps its legacy worktree writes");
        assert!(touch(b, &p.granted.join("legacy")), "the granted job must not inherit these wider limits");
        let (allow, _) = policy::identity(a).unwrap();
        assert!(!touch(a, &allow), "the marker rule grants nothing");
        std::fs::remove_dir_all(&p.dir).ok();
    }
}
