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
    fn may_lookup(&self, pid: u32, name: &str) -> Result<bool, String>;
}

struct Native;

impl Seatbelt for Native {
    fn confined(&self, pid: u32) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        { mac::confined(pid) }
        #[cfg(not(target_os = "macos"))]
        { let _ = pid; Err("sandbox readback exists only on macOS".into()) }
    }

    fn may_lookup(&self, pid: u32, name: &str) -> Result<bool, String> {
        #[cfg(target_os = "macos")]
        { mac::may_lookup(pid, name) }
        #[cfg(not(target_os = "macos"))]
        { let _ = (pid, name); Err("sandbox readback exists only on macOS".into()) }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CString;

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

    pub fn may_lookup(pid: u32, name: &str) -> Result<bool, String> {
        let op = CString::new("mach-lookup").map_err(|_| "invalid sandbox operation")?;
        let name = CString::new(name).map_err(|_| "invalid sandbox lookup name")?;
        // SANDBOX_FILTER_GLOBAL_NAME = 2; queries policy without performing a lookup or resolving a path.
        match unsafe { sandbox_check(pid as i32, op.as_ptr(), 2 | SANDBOX_CHECK_NO_REPORT, name.as_ptr()) } {
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
    match (s.may_lookup(pid, &allow), s.may_lookup(pid, &deny)) {
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
#[path = "daemon_sandbox_tests.rs"]
mod tests;
