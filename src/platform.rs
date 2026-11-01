//! taskq on coreq: install layout, self-update, launcher and OS units come from the shared crate (coreq.md); this file
//! keeps taskq's own parts: the error format, the install sweep and the load readings (taskq.md § 3, § 8).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)] pub use coreq::service::Unit;
pub use coreq::{Tool, quiet, write_atomic};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// #1445: temp file, flush, rename, then the folder entry; a power loss leaves the old or the new record.
/// ponytail: a temp torn by a power cut stays beside the record (one name per pid); sweep them if they pile up.
pub fn write_synced(p: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let name = p.file_name().ok_or("record has no file name")?.to_string_lossy();
    let tmp = p.with_file_name(format!("{name}.{}.tmp", std::process::id()));
    let written = fs::File::create(&tmp).and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all()))
        .and_then(|_| fs::rename(&tmp, p));
    if written.is_err() { let _ = fs::remove_file(&tmp); }
    written.map_err(|e| format!("{}: {e}", p.display()))?;
    #[cfg(unix)]
    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::File::open(dir).and_then(|d| d.sync_all()).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(())
}

/// coreq's status.json path and text, written through `write_synced` (#1445).
pub fn write_status(t: &Tool, root: &Path, status: &serde_json::Value) -> Result<(), String> {
    use std::hash::BuildHasher;
    let subject = std::hash::BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default().hash_one(root);
    coreq::telemetry::capture::poll("status.write", subject, || {
        let p = coreq::status::path(t, root);
        fs::create_dir_all(p.parent().ok_or("status.json has no folder")?).map_err(|e| e.to_string())?;
        write_synced(&p, &serde_json::to_string_pretty(status).map_err(|e| e.to_string())?)
    }, |result| if result.is_ok() { "written" } else { "status-write" })
}

/// #1445: whether a recorded process start (coreq native identity) lies before the current boot; Err: unknown.
#[cfg(target_os = "linux")]
pub fn started_before_boot(started: &str) -> std::io::Result<bool> {
    let (id,_) = started.rsplit_once('-').filter(|(id,ticks)| id.len()==36
        && id.bytes().enumerate().all(|(i,b)| if [8,13,18,23].contains(&i) {b==b'-'} else {b.is_ascii_hexdigit()})
        && ticks.parse::<u64>().is_ok()).ok_or_else(|| std::io::Error::other("invalid process start"))?;
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok(id != boot.trim())
}

#[cfg(target_os = "macos")]
pub fn started_before_boot(started: &str) -> std::io::Result<bool> {
    let secs: i64 = started.split('-').next().and_then(|s| s.parse().ok()).ok_or_else(|| std::io::Error::other("invalid process start"))?;
    let mut boot = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let mut size = std::mem::size_of::<libc::timeval>();
    // SAFETY: kern.boottime fills one timeval of the given size.
    if unsafe { libc::sysctlbyname(c"kern.boottime".as_ptr(), (&mut boot as *mut libc::timeval).cast(), &mut size, std::ptr::null_mut(), 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if boot.tv_sec <= 0 { return Err(std::io::Error::other("boot time unknown")); }
    Ok(secs < boot.tv_sec)
}

#[cfg(windows)]
pub fn started_before_boot(started: &str) -> std::io::Result<bool> {
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetTickCount64() -> u64; fn GetSystemTimeAsFileTime(time: *mut u64); }
    let created: u64 = started.parse().map_err(|_| std::io::Error::other("invalid process start"))?;
    let mut now = 0u64;
    // SAFETY: FILETIME is two u32 words; a u64 holds it with stricter alignment.
    let up = unsafe { GetSystemTimeAsFileTime(&mut now); GetTickCount64() };
    // FILETIME counts 100 ns; uptime includes sleep, so this is the boot instant up to clock changes.
    Ok(created < now.saturating_sub(up.saturating_mul(10_000)))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn started_before_boot(_started: &str) -> std::io::Result<bool> { Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "boot time unsupported")) }

/// Built in, never from config: project files are written by workers, so config must not choose the code hosts run.
pub const RELEASE_REPO: &str = "taskqorg/taskq";

/// taskq as a coreq tool installed in `install`.
pub fn tool(install: &Path) -> Tool {
    Tool { name:"taskq",title:"TaskQ",repo:RELEASE_REPO,version:VERSION,install:install.into(),fault:Some(crate::incident::launcher) }
}

pub fn default_install() -> PathBuf {
    coreq::install_dir("taskq","TaskQ",&[]).unwrap_or_else(|error| {
        use std::io::Write;
        let _=writeln!(std::io::stderr(),"taskq: {error}; set TASKQ_INSTALL explicitly to inspect or repair this installation");
        coreq::telemetry::capture::exit(1)
    })
}

/// M15: the OS default install (`%LOCALAPPDATA%\taskq`, `~/.local/share/taskq`), whatever `TASKQ_INSTALL` says: only
/// its tray writes the user's launchers (Start menu `TaskQ.lnk`, `~/Applications/TaskQ.app`).
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub fn home_install() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "LOCALAPPDATA" } else { "HOME" }).map(PathBuf::from).filter(|p| p.is_absolute())?;
    Some(if cfg!(windows) { home.join("taskq") } else { home.join(".local/share/taskq") })
}

/// `install` is the OS default install; a path the exe location resolved (`\\?\` on Windows) still matches.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub fn is_home(install: &Path, home: Option<&Path>) -> bool {
    home.is_some_and(|h| h == install || fs::canonicalize(h).is_ok_and(|h| fs::canonicalize(install).is_ok_and(|i| i == h)))
}

/// #1858: a Windows path without the verbatim prefix canonicalize adds (`\\?\C:\` is `C:\`, `\\?\UNC\s\x` is
/// `\\s\x`), the form older registry entries and Claude's project keys use; other platforms keep the path.
pub fn plain_path(p: &Path) -> PathBuf {
    let Some(text) = p.to_str().filter(|_| cfg!(windows)) else { return p.into() };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") { return PathBuf::from(format!(r"\\{rest}")); }
    text.strip_prefix(r"\\?\").map_or_else(|| p.into(), PathBuf::from)
}

static SERVICE_LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// From here on this process is the service: each log line is stamped and folded (§ 4).
pub fn service_log() {
    SERVICE_LOG.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_service_log() -> bool {
    SERVICE_LOG.load(std::sync::atomic::Ordering::Relaxed)
}

/// `<UTC time> <text>` on one line: the lines of `text`, trimmed, joined by a space (§ 4).
pub fn log_line(at: i64, text: &str) -> String {
    format!("{} {}", crate::board::stamp(at), text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
}

thread_local!(static PROJECT_LOG: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) });

pub fn project_log(path: PathBuf) { PROJECT_LOG.set(Some(path)); }

pub fn log(text: &str) {
    if is_service_log() {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let line = log_line(at, text);
        PROJECT_LOG.with_borrow(|p| {
            let stderr_file = p.as_ref().and_then(|p| p.parent()).and_then(stderr_file);
            emit(&line, p.as_deref(), &mut std::io::stderr(), stderr_file.as_deref());
        });
    } else {
        eprintln!("{text}");
    }
}

/// The file the service's stderr lands in: the host unit sends it to <install>/taskq.log on macOS and Windows (coreq `unit(.., "")`); Linux uses the journal.
pub fn stderr_file(install: &Path) -> Option<PathBuf> {
    use std::io::IsTerminal;
    (cfg!(any(target_os = "macos", windows)) && !std::io::stderr().is_terminal()).then(|| install.join("taskq.log"))
}

/// One line to the project log and to stderr, once when both are the same file; stderr keeps a line the project log refused.
fn emit(line: &str, project: Option<&Path>, stderr: &mut dyn std::io::Write, stderr_file: Option<&Path>) {
    use std::io::Write;
    let written = project.is_some_and(|p| fs::OpenOptions::new().create(true).append(true).open(p).and_then(|mut f| writeln!(f, "{line}")).is_ok());
    if !(written && project == stderr_file) { let _ = writeln!(stderr, "{line}"); }
}

pub fn die(msg: &str) -> ! {
    log(&error_text(msg, crate::find_root().ok().as_deref()));
    exit(1);
}

/// § 4: a service exit first logs its code and reason (#1625: neither the launcher nor the Windows unit records them).
pub fn service_exit(code: i32, reason: &str) -> ! {
    log(&format!("service exits {code}: {reason}"));
    exit(code)
}

/// Every taskq exit. #1566: Windows `ExitProcess` first kills the other threads, then runs this thread's
/// TLS destructors; a thread killed while it held a lock one of them takes (sharded_slab's thread-id
/// registry) left that exit waiting forever. Seal telemetry, flush, then end without destructors.
pub fn exit(code: i32) -> ! {
    #[cfg(windows)] {drain_telemetry(); coreq::telemetry::capture::before_exec(); terminate(code)}
    #[cfg(not(windows))] std::process::exit(code)
}

/// #1566: CoreQ's exit flush waits 3 s, but one Windows append can outlast it (its writer retries a busy store for 10 s);
/// ending the process then loses the queued observations. Retry the unacknowledged flush, bounded, before that flush.
pub fn drain_telemetry() {
    use coreq::telemetry::capture::Capture;
    let Some(capture) = tracing::dispatcher::get_default(|d| d.downcast_ref::<Capture>().cloned()) else { return };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while let Err(e) = capture.flush() && std::time::Instant::now() < until
        && (e.starts_with("telemetry flush unacknowledged (") || e == "telemetry flush queue deadline") {}
}

/// M15: the console was made for this process alone (Explorer, Start menu, taskbar pin), not a terminal's; then it is
/// freed, so its window closes. The std handles are reset as CoreQ's launcher does: stale console handles failed later writes.
#[cfg(windows)]
pub fn own_console() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetConsoleProcessList(list: *mut u32, n: u32) -> u32; fn FreeConsole() -> i32; fn SetStdHandle(id: u32, h: *mut std::ffi::c_void) -> i32; }
    let mut list = [0u32; 2];
    // SAFETY: the buffer holds the two ids asked for; a larger count is only returned, never written.
    if unsafe { GetConsoleProcessList(list.as_mut_ptr(), 2) } != 1 { return false; }
    unsafe {
        FreeConsole();
        for id in [-10i32, -11, -12] { SetStdHandle(id as u32, std::ptr::null_mut()); }
    }
    true
}

/// #2049: the one way taskq starts a child. On Windows it carries CREATE_NO_WINDOW, so a console tool (git, tasklist,
/// codex, the launcher) opens no window of its own when the service or tray starts it; a later `creation_flags` keeps
/// that bit. Output still reaches piped or null handles. `spawn_sites` keeps every start on this path.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    quiet(&mut c);
    c
}

/// #2049: a child that shares this process's console, as the launcher's CLI pass-through does: a terminal keeps
/// showing its output, so CREATE_NO_WINDOW is set only when there is no console to share (a windowless start).
pub fn console_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" { fn GetConsoleWindow() -> *mut std::ffi::c_void; }
        // SAFETY: no arguments; it only reads this process's console window handle.
        if unsafe { GetConsoleWindow() }.is_null() { quiet(&mut c); }
    }
    c
}

/// Ends this process now: no other thread or destructor runs after the flush.
#[cfg(windows)]
pub fn terminate(code: i32) -> ! {
    use std::io::Write;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetCurrentProcess() -> isize; fn TerminateProcess(process: isize, code: u32) -> i32; }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { TerminateProcess(GetCurrentProcess(), code as u32); }
    std::process::abort()
}

pub fn error_text(msg: &str, root: Option<&Path>) -> String {
    // A message that already ends in its own fitting `Next:` line keeps it (#1894).
    if let Some((problem, next)) = msg.split_once("\nNext: ")
        && !problem.contains(['\r', '\n']) && !next.contains(['\r', '\n'])
        && problem.chars().count() <= 153 && next.chars().count() <= 154
    {
        return format!("Error: {problem}\nNext: {next}");
    }
    let next = if msg.chars().count() > 153 || msg.contains(['\r', '\n']) {
        use std::io::Write;
        let saved = root.ok_or_else(|| "outside a taskq project".to_string()).and_then(|root| {
            let d = crate::dot(root);
            let p = d.join("cli-errors.log");
            fs::create_dir_all(&d).and_then(|_| writeln!(fs::OpenOptions::new().create(true).append(true).open(&p)?, "{msg}")).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok(p)
        });
        match saved {
            Ok(p) => format!("Read `{}`; use `taskq help` for syntax and retry.", p.display()),
            // #1555: the full text is the diagnostic; say why no copy exists instead of a bare failure
            Err(why) => return format!("Error: {msg}\nNext: the full diagnostic is above; no cli-errors.log copy ({why})."),
        }
    } else { "Check the input and board/service status, then retry; use `taskq help` for syntax.".into() };
    format!("Error: {}\nNext: {next}", crate::compact(msg, 153))
}

/// Complete bounded dependency output, including nonzero exits whose receipt the caller must inspect.
pub fn output(name: &'static str, command: &mut std::process::Command, budget: std::time::Duration) -> Result<std::process::Output,String> {
    match coreq::process::output(name,command,budget) {
        Ok(output) | Err(coreq::process::Failure::Exit(output)) => Ok(output),
        Err(error) => Err(format!("{name}: {error}; outcome unknown")),
    }
}

/// § 8 update steps (coreq), then the sweep: Ok(Some(version)) when a new release became current; with `check`, when
/// one is available.
pub fn update(install: &Path, check: bool) -> Result<Option<String>, String> {
    let _lock = update_lock(install)?;
    transition_ready(install)?;
    crate::profile::measure("update", || update_inner(install, check))
}

pub(crate) fn update_lock(install: &Path) -> Result<coreq::file_lock::Lock, String> {update_lock_within(install, std::time::Duration::ZERO)}

/// `update_lock` for install commands that cannot retry on a later tick: a host service's update step holds the
/// lock briefly on every tick, most often right after it re-execs into a new build, so wait at most `wait` (#1415).
pub(crate) fn update_lock_within(install: &Path, wait: std::time::Duration) -> Result<coreq::file_lock::Lock, String> {
    fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let until = std::time::Instant::now() + wait;
    loop {
        match coreq::file_lock::try_lock(&install.join("update.lock")) {
            Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => std::thread::sleep(std::time::Duration::from_millis(100)),
            other => return other.map_err(|e| format!("update busy or lock unavailable: {e}")),
        }
    }
}

/// An interrupted registry/install transition retains admission and active release selection together.
pub(crate) fn transition_ready(install:&Path)->Result<(),String> {
    let prepare=format!("{}.pending",crate::host::INSTALL_PENDING);
    for name in ["projects.pending",crate::host::INSTALL_PENDING,prepare.as_str()] {
        match fs::symlink_metadata(install.join(name)) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            Err(e)=>return Err(format!("installation transition unknown: {e}; update held")),
            Ok(_)=>return Err(format!("{name} retained; installation reconciliation required before update")),
        }
    }
    Ok(())
}

fn update_inner(install: &Path, check: bool) -> Result<Option<String>, String> {
    let v = update_result(coreq::update::update(&tool(install), check))?;
    if v.is_some() && !check {
        let shim = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/taskq")).unwrap_or_default();
        sweep(install, &shim);
    }
    #[cfg(target_os = "macos")]
    if let Some(v) = v.as_deref().filter(|_| !check) {
        sign_copies(&tool(install), v);
    }
    Ok(v)
}

/// Freeze the source before local signing; an immutable release retains the original source binding.
pub fn prepare_install(t:&Tool)->Result<String,String> {
    let source=fs::read(std::env::current_exe().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let hash=|bytes:&[u8]|->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()};
    let source_hash=hash(&source);
    let binding=t.release_exe(t.version).with_file_name("install-source");
    match fs::symlink_metadata(&binding) {
        Ok(meta) if meta.is_file()&&meta.len()<=256=>{},Ok(_)=>return Err("prepared source binding unsupported; kept".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
    }
    match fs::read_to_string(&binding) {
        Ok(old)=>{
            let bound:Vec<_>=old.lines().collect();
            if bound.len()!=2||bound.iter().any(|v|v.len()!=64||!v.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())) {return Err("prepared release source binding invalid; kept".into());}
            match fs::read(t.release_exe(t.version)) {
                Ok(bytes)=>{
                    let candidate_hash=hash(&bytes);
                    if bound[1]!=candidate_hash||(bound[0]!=source_hash&&candidate_hash!=source_hash) {return Err("prepared release source differs; use a new version, existing file kept".into());}
                    coreq::update::prepare_install(t,&bytes)?;
                    #[cfg(all(windows,target_env="gnu"))] prepare_webview2_loader(t)?;
                    return Ok(candidate_hash);
                },
                Err(e) if e.kind()==std::io::ErrorKind::NotFound&&bound[0]==source_hash=>{},Err(e)=>return Err(e.to_string()),
            }
        },
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
    }
    if fs::read(t.release_exe(t.version)).ok().as_deref()==Some(source.as_slice()) {
        coreq::update::prepare_install(t,&source)?;
        coreq::write_durable(&binding,format!("{source_hash}\n{source_hash}\n"))?;
        #[cfg(all(windows,target_env="gnu"))] prepare_webview2_loader(t)?;
        return Ok(source_hash);
    }
    #[cfg(target_os="macos")]
    let source={
        let scratch=std::env::temp_dir().join(format!("taskq-install-sign-{}",crate::runtime::uuid()));
        fs::write(&scratch,&source).map_err(|e|e.to_string())?;
        let result=codesign(SIGNING_IDENTITY,&scratch,"org.taskq.taskq").and_then(|signed| {
            if !signed {log("signing: local identity absent; candidate keeps its original signature");}
            fs::read(&scratch).map_err(|e|e.to_string())
        });
        let cleanup=fs::remove_file(&scratch).map_err(|e|format!("signing scratch cleanup: {e}"));
        let bytes=result?;cleanup?;bytes
    };
    let candidate=hash(&source);
    match fs::symlink_metadata(t.release_exe(t.version)) {
        Ok(meta) if meta.is_file()&&fs::read(t.release_exe(t.version)).map_err(|e|e.to_string())?==source=>{},
        Ok(_)=>return Err("prepared release differs; use a new version, existing file kept".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
    }
    // The source binding precedes immutable publication, so a crash cannot leave an unbound signed candidate.
    fs::create_dir_all(binding.parent().ok_or("prepared release directory missing")?).map_err(|e|e.to_string())?;
    coreq::write_durable(&binding,format!("{source_hash}\n{candidate}\n"))?;
    coreq::update::prepare_install(t,&source)?;
    #[cfg(all(windows,target_env="gnu"))] prepare_webview2_loader(t)?;
    Ok(candidate)
}

/// Activate the immutable executable and its prepared GNU loader before any native start.
pub fn activate_install(t:&Tool)->Result<(),String> {
    coreq::update::activate_install(t)?;
    #[cfg(all(windows,target_env="gnu"))] {
        let bytes=fs::read(t.release_exe(t.version).with_file_name("WebView2Loader.dll")).map_err(|e|e.to_string())?;
        let target=t.install.join("WebView2Loader.dll");
        if fs::read(&target).ok().as_deref()!=Some(bytes.as_slice()) {coreq::write_durable(&target,bytes)?;}
    }
    Ok(())
}

/// § 8 step 5: the local code-signing identity in the login keychain; absent, copies stay ad-hoc.
#[cfg(target_os = "macos")]
pub const SIGNING_IDENTITY: &str = "TaskQ Local Signing";

/// Signs `path` in place as `id` with `identity` and verifies it against `identifier <id> and certificate leaf`.
/// Ok(false): the identity is absent, `path` is untouched; Err: present but signing or verification failed.
#[cfg(target_os = "macos")]
pub fn codesign(identity: &str, path: &Path, id: &str) -> Result<bool, String> {
    use std::process::Command;
    let ids = coreq::process::output("signing.identity", command("security").args(["find-identity", "-p", "codesigning"]), std::time::Duration::from_secs(30)).map_err(|e| format!("security: {e}"))?;
    let Some(sha1) = identity_hash(identity, &ids)? else { return Ok(false) };
    let run = |c: &mut Command| match coreq::process::output("signing.command", c, std::time::Duration::from_secs(30)) {
        Ok(_) => Ok(()),
        Err(coreq::process::Failure::Exit(o)) => Err(format!("{}: {}", path.display(), String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    };
    run(command("codesign").args(["-f", "-s", &sha1, "-i", id]).arg(path))?;
    run(command("codesign").arg(format!("-R=identifier \"{id}\" and certificate leaf = H\"{sha1}\"")).args(["-v", "--strict"]).arg(path))?;
    Ok(true)
}

/// The SHA-1 of `identity` in `security find-identity` output; Err when `security` failed, so an unreadable
/// keychain is not reported as an absent identity.
#[cfg(target_os = "macos")]
fn identity_hash(identity: &str, ids: &std::process::Output) -> Result<Option<String>, String> {
    if !ids.status.success() {
        return Err(format!("security find-identity: {}: {}", ids.status, String::from_utf8_lossy(&ids.stderr).trim()));
    }
    let quoted = format!("\"{identity}\"");
    Ok(String::from_utf8_lossy(&ids.stdout).lines().find(|l| l.contains(&quoted))
        .and_then(|l| l.split_whitespace().nth(1).map(str::to_owned)))
}

/// § 8 step 5: TCC keys an ad-hoc grant to the cdhash, so each release would need a new grant. Signs a copy of the
/// release `v` and of the launcher as `org.taskq.taskq`, then renames it into place (step 5); logs an absent identity
/// and each copy that failed, which keeps its ad-hoc signature.
#[cfg(target_os = "macos")]
fn sign_copies(t: &Tool, v: &str) {
    for exe in [t.release_exe(v), t.install.join(t.exe_name())] {
        match sign_copy(&exe) {
            Ok(true) => {}
            Ok(false) => return log(&format!("signing: no `{SIGNING_IDENTITY}` identity; {v} stays ad-hoc, macOS grants will not survive the next release")),
            Err(e) => log(&format!("signing failed, copy stays ad-hoc: {e}")),
        }
    }
}

/// Signs a copy of `exe` as `org.taskq.taskq` and renames it into place, so a running `exe` is never rewritten.
#[cfg(target_os = "macos")]
fn sign_copy(exe: &Path) -> Result<bool, String> {
    let tmp = exe.with_extension("signing");
    let r = fs::copy(exe, &tmp).map_err(|e| format!("{}: {e}", exe.display()))
        .and_then(|_| codesign(SIGNING_IDENTITY, &tmp, "org.taskq.taskq"))
        .and_then(|s| if s { fs::rename(&tmp, exe).map(|_| true).map_err(|e| format!("{}: {e}", exe.display())) } else { Ok(false) });
    let _ = fs::remove_file(&tmp);
    r
}

/// § 8 step 5 for local builds (#1329): the cargo runner `scripts/macos-runner.sh` calls `taskq sign-local <exe>..`
/// before cargo run or a test binary starts. Without the identity it says so once per target directory (marker
/// beside the signer) and the builds stay ad-hoc; a failed signature fails the run.
#[cfg(target_os = "macos")]
pub fn sign_local(paths: &[String]) -> Result<(), String> {
    for p in paths {
        if !sign_copy(Path::new(p))? {
            let marker = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name(".taskq-unsigned");
            if fs::OpenOptions::new().write(true).create_new(true).open(&marker).is_ok() {
                eprintln!("signing: no `{SIGNING_IDENTITY}` identity; local builds stay ad-hoc and macOS grants will not match releases");
            }
            return Ok(());
        }
    }
    Ok(())
}

/// GNU links the DLL; MSVC release artifacts link the loader statically.
#[cfg(all(windows, target_env = "gnu"))]
fn loaded_webview2_loader() -> Result<Vec<u8>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows::{core::w, Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW}};
    let mut path = vec![0u16; 32768];
    // SAFETY: the imported loader remains loaded for this process; the API writes within the supplied buffer.
    let len = unsafe {
        let module = GetModuleHandleW(w!("WebView2Loader.dll")).map_err(|e| e.to_string())?;
        GetModuleFileNameW(Some(module), &mut path) as usize
    };
    if len == 0 || len >= path.len() {
        return Err("cannot locate loaded WebView2Loader.dll".into());
    }
    fs::read(PathBuf::from(std::ffi::OsString::from_wide(&path[..len]))).map_err(|e|e.to_string())
}

#[cfg(all(windows,target_env="gnu"))]
fn prepare_webview2_loader(t:&Tool)->Result<(),String> {
    let bytes=loaded_webview2_loader()?;
    let target=t.release_exe(t.version).with_file_name("WebView2Loader.dll");
    match fs::symlink_metadata(&target) {
        Ok(meta) if meta.is_file()&&fs::read(&target).map_err(|e|e.to_string())?==bytes=>Ok(()),
        Ok(_)=>Err("prepared WebView2 loader differs; existing release kept, use a new version".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>coreq::write_durable(&target,bytes),
        Err(e)=>Err(e.to_string()),
    }
}

/// Service step 7 also catches a release made current by another service on this install.
pub fn service_update(install: &Path) -> Result<Option<String>, String> {
    let _lock=update_lock(install)?;
    transition_ready(install)?;
    crate::profile::measure("update",||service_update_locked(install))
}

fn service_update_locked(install:&Path)->Result<Option<String>,String> {
    channel_update(install,&|name,to| coreq::update::download(&format!("https://github.com/{RELEASE_REPO}/releases/latest/download/{name}"),to))
}

/// § 8 item 1 host channel: host.json `channel` `dev|live`; a missing file or key reads `live`.
pub fn channel(install:&Path)->Result<&'static str,String> {
    let v:serde_json::Value=match fs::read(install.join("host.json")) {
        Ok(bytes)=>serde_json::from_slice(&bytes).map_err(|e|format!("host.json: {e}"))?,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok("live"),
        Err(e)=>return Err(format!("host.json: {e}")),
    };
    channel_of(&v)
}

pub fn channel_of(v:&serde_json::Value)->Result<&'static str,String> {
    match v["channel"].as_str() {
        _ if v["channel"].is_null()=>Ok("live"),
        Some("dev")=>Ok("dev"),
        Some("live")=>Ok("live"),
        _=>Err("host.json: channel must be dev or live".into()),
    }
}

/// `fetch(name, to)` reads one file of the latest release; tests pass a local one.
fn channel_update(install:&Path,fetch:&dyn Fn(&str,&Path)->Result<(),String>)->Result<Option<String>,String> {
    static NOT_APPLIED:std::sync::Mutex<String>=std::sync::Mutex::new(String::new());
    let t=tool(install);
    if let Some(v) = t.current().filter(|v| coreq::update::newer(v, VERSION)) {
        log!("current is {v}; restarting");
        return Ok(Some(v));
    }
    let running=t.current().unwrap_or_else(||VERSION.into());
    let live=channel(install)?=="live";
    // a dev host, or a live host on a `-dev` build that coreq would never replace by an older release
    if !live||running.contains('-') {
        let rel=install.join("releases");
        fs::create_dir_all(&rel).map_err(|e|e.to_string())?;
        let probe=rel.join("latest.json.tmp");
        fetch("latest.json",&probe)?;
        let latest:serde_json::Value=serde_json::from_slice(&fs::read(&probe).map_err(|e|e.to_string())?).map_err(|e|format!("latest.json: {e}"))?;
        let _=fs::remove_file(&probe);
        let ver=latest["version"].as_str().ok_or("latest.json: no version")?.to_string();
        let name=format!("taskq-{ver}-{}{}",coreq::update::target(),std::env::consts::EXE_SUFFIX);
        if ver.is_empty()||ver.contains("..")||!ver.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'.'||c==b'-')||latest["files"][coreq::update::target()]!=name.as_str() {
            return Err(format!("latest.json: unexpected version {ver:?} or file"));
        }
        if !live {
            let mut logged=NOT_APPLIED.lock().unwrap_or_else(|e|e.into_inner());
            if coreq::update::newer(&ver,&running)&&*logged!=ver {log!("channel dev: release {ver} not applied");*logged=ver;}
            return Ok(None);
        }
        if ver==running {return Ok(None);}
        if !coreq::update::newer(&ver,&running) {
            let t=Tool{version:Box::leak(ver.clone().into_boxed_str()),..t};
            if !t.release_exe(&ver).is_file() {
                let tmp=rel.join(format!("{ver}.channel"));
                fs::create_dir_all(&tmp).map_err(|e|e.to_string())?;
                let staged=(|| {
                    for f in ["SHA256SUMS",name.as_str()] {fetch(f,&tmp.join(f))?;}
                    let bytes=fs::read(tmp.join(&name)).map_err(|e|e.to_string())?;
                    coreq::update::verify(&fs::read(tmp.join("SHA256SUMS")).map_err(|e|e.to_string())?,&name,&bytes)?;
                    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(tmp.join(&name),fs::Permissions::from_mode(0o755)).map_err(|e|e.to_string())?;}
                    let smoke=match coreq::process::output("release.selftest",command(tmp.join(&name)).arg("selftest").env("TASKQ_INSTALL",install).env("TASKQ_SELFTEST_OFFLINE","1"),std::time::Duration::from_secs(30)) {
                        Ok(out)|Err(coreq::process::Failure::Exit(out))=>out,
                        Err(e)=>return Err(format!("{ver} selftest failed: {e}")),
                    };
                    if !smoke.status.success() {return Err(format!("{ver} selftest failed: {}",String::from_utf8_lossy(&smoke.stderr).trim()));}
                    coreq::update::prepare_install(&t,&bytes).map(|_|None)
                })();
                let _=fs::remove_dir_all(&tmp);
                update_result(staged)?;
                #[cfg(target_os="macos")] sign_copies(&t,&ver);
            }
            // coreq refuses a downgrade of `current`: select the release first, then activate its launcher
            coreq::write_durable(&install.join("previous"),&running)?;
            coreq::write_durable(&install.join("current"),&ver)?;
            coreq::update::activate_install(&t)?;
            log!("channel live: replaced {running} with release {ver}; restarting");
            return Ok(Some(ver));
        }
    }
    if update_inner(install, true)?.is_some() {
        let v = update_inner(install, false)?;
        if let Some(v) = &v {
            log!("updated to {v}; restarting");
        }
        return Ok(v);
    }
    Ok(None)
}

/// § 8 item 1 dev install: the SHA to start now. None on `live`, while a build runs, or for the installed or an
/// already started SHA (a started SHA never restarts, so a failed build is not retried); `ci` is read only after that.
pub fn dev_next(channel:&str,main:Option<&str>,installed:Option<&str>,started:Option<&str>,building:bool,ci:&mut dyn FnMut(&str)->Result<(),String>)->Option<String> {
    let sha=main.filter(|m|channel=="dev"&&!building&&Some(*m)!=installed&&Some(*m)!=started)?;
    ci(sha).ok().map(|_|sha.to_string())
}

/// § 8 item 1 dev install gate: repo and channel only; the script comes from the fetched SHA (`dev_spawn`), so a
/// root checked out before the script existed still installs (#1406).
pub fn dev_enabled(repo:&str,install:&Path)->bool { repo==RELEASE_REPO&&channel(install)==Ok("dev") }

/// One project pass of the dev install ([detail](docs/design/host-service.md#dev-install)): fold the script's
/// report, start the next SHA, return the `status.json` `dev` value. State lives in `<install>/dev/`.
#[cfg(unix)]
pub fn dev_tick(install:&Path,root:&Path,ci:&mut dyn FnMut(&str)->Result<(),String>)->serde_json::Value {
    let dir=install.join("dev");
    let _=fs::create_dir_all(&dir);
    let read=|f:&str|fs::read_to_string(dir.join(f)).ok().map(|s|s.trim().to_string()).filter(|s|!s.is_empty());
    // the script's exit report is read once: one log line, then `installed` or `failed`
    if let Some(r)=read("result") {
        let (sha,outcome)=r.split_once(' ').unwrap_or((&r,"failed"));
        let ok=outcome=="installed";
        log!("dev install: {sha} {}",if ok {"installed"} else {"failed; see dev/install.log"});
        let _=coreq::write_durable(&dir.join(if ok {"installed"} else {"failed"}),sha);
        if ok {let _=fs::remove_file(dir.join("failed"));}
        let _=fs::remove_file(dir.join("result"));
    }
    let building=read("lock/pid").and_then(|p|p.parse().ok()).is_some_and(crate::runtime::alive);
    // #1856: a failed or empty read is logged with its command; `main` then reads unknown and nothing starts
    let main=crate::tools::run("dev.main",command("git").arg("-C").arg(root).args(["ls-remote","origin","refs/heads/main"]),std::time::Duration::from_secs(30))
        .and_then(|o|String::from_utf8_lossy(&o).split_whitespace().next().map(String::from).filter(|s|s.len()==40&&s.bytes().all(|c|c.is_ascii_hexdigit()))
            .ok_or_else(||format!("`git ls-remote origin refs/heads/main` printed no main SHA: {:?}",String::from_utf8_lossy(&o).trim())))
        .map_err(|e|log!("dev install: {e}")).ok();
    let (installed,started,failed)=(read("installed"),read("started"),read("failed"));
    if let Some(sha)=dev_next("dev",main.as_deref(),installed.as_deref(),started.as_deref(),building,ci) {
        match coreq::write_durable(&dir.join("started"),&sha).and_then(|_|dev_spawn(root,&sha,&dir)) {
            Ok(())=>log!("dev install: start {sha}"),
            Err(e)=>log!("dev install: {sha} not started: {e}"),
        }
    }
    let pending=main.filter(|m|Some(m)!=installed.as_ref()&&Some(m)!=failed.as_ref());
    serde_json::json!({"installed":installed,"pending":pending,"failed":failed,"building":building})
}

/// Runs the `scripts/dev-install.sh` of `sha` itself, so the primary checkout's own edits never matter. The build
/// outlives this service's re-exec into it: its own process group, reparented by the background `&`.
#[cfg(unix)]
fn dev_spawn(root:&Path,sha:&str,dir:&Path)->Result<(),String> {
    use std::os::unix::process::CommandExt;
    let st=command("sh").arg("-c")
        // `--refmap=`: fetch main's objects without updating origin/main, whose ref lock a concurrent fetch may hold (#1633)
        .arg(r#"(git -C "$1" fetch -q --refmap= --no-write-fetch-head origin main && git -C "$1" show "$2:scripts/dev-install.sh" >"$3/install.sh" && TASKQ_DEV_REPO="$1" exec sh "$3/install.sh" "$2" || echo "$2 failed" >"$3/result") >"$3/install.log" 2>&1 </dev/null &"#)
        .arg("sh").arg(root).arg(sha).arg(dir)
        .env_remove("TASKQ_ROOT").env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .process_group(0).status().map_err(|e|e.to_string())?;
    if st.success() {Ok(())} else {Err(format!("sh {st}"))}
}

fn update_result(result: Result<Option<String>, String>) -> Result<Option<String>, String> {
    if let Err(e) = &result {
        if let Some((release, _)) = e.split_once(" selftest failed:") {
            let field = format!("release:{release}");
            crate::incident::detect("update.smoke", concat!("src/platform.rs:", line!()), "none", &[&field], &["smoke:failed"]);
        }
    }
    result
}

/// Releases in use: current, previous, the running version and each registered project's published version; `None`
/// when the registry or a project version is unreadable, so release ownership is unknown and every release stays.
pub fn releases_in_use(install: &Path) -> Option<Vec<String>> {
    let read = |f: &str| fs::read_to_string(install.join(f)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut keep: Vec<String> = [read("current"), read("previous"), Some(VERSION.into())].into_iter().flatten().collect();
    // a corrupt status.json leaves its project's version unknown: every release stays (§ 4 Data)
    let registry=coreq::telemetry::capture::operation("update.projects",||crate::host::project_paths(install),|_|"registry-unreadable");
    let mut unknown=registry.is_err();
    for root in registry.unwrap_or_default() {
        let version=coreq::telemetry::capture::operation("update.project-version",|| {
            crate::data::json(&root.join(".taskq/status.json"))?.as_ref()
                .and_then(|st|st["version"].as_str()).filter(|v|!v.is_empty()).map(String::from)
                .ok_or_else(||"project version unknown".to_string())
        },|_|"version-unknown");
        match version {Ok(version)=>keep.push(version),Err(_)=>unknown=true}
    }
    match crate::runtime::protected_app_server::versions_in_use(install) {Ok(versions)=>keep.extend(versions),Err(_)=>unknown=true}
    (!unknown).then_some(keep)
}

/// § 8 step 2 sweep: everything in `<install>` the current version does not own, and the v3 shim. Best effort: a file
/// in use (a running Windows exe) stays until a later update.
pub fn sweep(install: &Path, shim: &Path) {
    let keep = releases_in_use(install);
    let owned = |n: &str| {
        ["releases", "current", "previous", "projects", "token", "gitlab-token", "installation-id", "link-key", "link", "connection", "taskq", "taskq.exe", "taskqw.exe", "WebView2Loader.dll", "TaskQ.app", "capacity", "resources", "resource-state", "windows-native", crate::policy::FILE, "taskq-profile.lock", "hold", "admission.json", "admission.lock", "update.lock", "host.json", "host.json.pending", "host-policy-migration", "native", "service", "service.lock", "poke", "restart", "update", "projects.lock", "projects.pending", "rollback-floor", "login-off", "accounts.json", "accounts-state.json", "accounts-state.pending.json", "accounts-state.lock"].contains(&n)
            ||["setup","messages.json","messages.tmp"].contains(&n)
            ||n==crate::host::INSTALL_PENDING||n.strip_suffix(".pending")==Some(crate::host::INSTALL_PENDING)
            || n.ends_with(".log") || n.ends_with(".tmp") || n.starts_with("tray") || (n.starts_with("TaskQ-") && n.ends_with(".xml"))
    };
    let gone = |p: &Path| {
        let _ = if p.is_dir() && !p.is_symlink() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
    };
    for e in fs::read_dir(install).into_iter().flatten().flatten() {
        if !owned(&e.file_name().to_string_lossy()) {
            gone(&e.path());
        }
    }
    for e in fs::read_dir(install.join("releases")).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if keep.as_ref().is_some_and(|keep| !n.ends_with(".tmp") && !keep.contains(&n)) {
            gone(&e.path());
        }
    }
    if fs::read_to_string(shim).is_ok_and(|t| t.contains("taskq.py launch") && t.contains(&install.display().to_string())) {
        gone(shim);
    }
}

// ---------- host load for the gate (§ 3): native APIs, no executables ----------

/// `/proc/stat` aggregate `cpu` line: (busy, total) ticks; idle includes iowait, guest ticks are already in user/nice.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn proc_stat(text: &str) -> Option<(u64, u64)> {
    let f: Vec<u64> = text.lines().next()?.strip_prefix("cpu ")?.split_whitespace().take(8).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let total: u64 = f.iter().sum();
    Some((total - f.get(3)? - f.get(4).unwrap_or(&0), total))
}

/// `/proc/meminfo`: `MemAvailable / MemTotal` in %; no `MemAvailable` is unknown.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn meminfo(text: &str) -> Option<f64> {
    let kb = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)?.trim().strip_suffix("kB")?.trim().parse::<f64>().ok());
    let (avail, total) = (kb("MemAvailable:")?, kb("MemTotal:")?);
    (total > 0.0 && avail <= total).then(|| avail * 100.0 / total)
}

#[cfg(target_os = "linux")]
pub fn cpu_ticks() -> Option<(u64, u64)> {
    proc_stat(&fs::read_to_string("/proc/stat").ok()?)
}

#[cfg(target_os = "linux")]
pub fn memory_available() -> Option<f64> {
    meminfo(&fs::read_to_string("/proc/meminfo").ok()?)
}

/// Available GiB and the memory pressure level: `MemAvailable`; Linux has no pressure level (0 never holds).
#[cfg(target_os = "linux")]
pub fn host_memory() -> Option<(f64, u32)> {
    let t = fs::read_to_string("/proc/meminfo").ok()?;
    let kb = t.lines().find_map(|l| l.strip_prefix("MemAvailable:")?.trim().strip_suffix("kB")?.trim().parse::<f64>().ok())?;
    Some((kb / GIB * 1024.0, 0))
}

/// The gui probe (§ 4 Filter): Screen Recording TCC state allowed for this process and `screencapture -x` writes a PNG.
#[cfg(target_os = "macos")]
pub fn gui_probe() -> Result<(), String> {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    // SAFETY: no arguments; reads this process's TCC state without a prompt
    gated(unsafe { CGPreflightScreenCaptureAccess() }, capture_png)
}

/// The gui probe's preflight refusal: Screen Recording is not allowed for this process.
pub const NOT_ALLOWED: &str = "Screen Recording not allowed (TCC)";

/// #1803 A4: the capture runs only after an allowed preflight, so the probe never raises a Screen Recording prompt.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn gated(preflight: bool, capture: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if preflight { capture() } else { Err(NOT_ALLOWED.into()) }
}

#[cfg(target_os = "macos")]
fn capture_png() -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("taskq-gui-{}", crate::runtime::uuid()));
    fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| format!("GUI capture directory: {e}"))?;
    let png = dir.join("capture.png");
    let ran = output("gui.capture", command("/usr/sbin/screencapture")
        .args(["-x", "-t", "png"]).arg(&png), std::time::Duration::from_secs(5));
    let ok = fs::read(&png).is_ok_and(|b| b.starts_with(b"\x89PNG\r\n\x1a\n"));
    let cleaned = fs::remove_dir_all(&dir);
    cleaned.map_err(|e| format!("GUI capture cleanup: {e}"))?;
    match ran {
        Ok(s) if s.status.success() && ok => Ok(()),
        Ok(_) => Err("screencapture wrote no PNG".into()),
        Err(e) => Err(e),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn gui_probe() -> Result<(), String> {
    Err("no gui probe on this OS".into())
}

/// Stale grant (#1803, [worker permissions](docs/design/worker-permissions.md#macos-signing-identity)): the preflight
/// is refused while `<install>/taskq` meets the stable requirement (`org.taskq.taskq` and the local identity's
/// certificate leaf), so the TCC row still pins an older cdhash. An unreadable keychain or signature reads false.
#[cfg(target_os = "macos")]
pub fn stale_grant(install: &Path, probe: &Result<(), String>) -> bool {
    if probe.as_ref().err().is_none_or(|e| e != NOT_ALLOWED) { return false; }
    // #1856: an unreadable keychain or a codesign that did not run is logged; a requirement mismatch is the answer
    let sha1 = match output("signing.identity", command("security").args(["find-identity", "-p", "codesigning"]), std::time::Duration::from_secs(30))
        .and_then(|ids| identity_hash(SIGNING_IDENTITY, &ids)) {
        Ok(Some(sha1)) => sha1,
        Ok(None) => return false,
        Err(e) => { log(&format!("stale grant unknown: {e}")); return false }
    };
    match coreq::process::output("signing.command", command("codesign").arg(format!("-R=identifier \"org.taskq.taskq\" and certificate leaf = H\"{sha1}\""))
        .args(["-v", "--strict"]).arg(install.join("taskq")), std::time::Duration::from_secs(30)) {
        Ok(_) => true,
        Err(coreq::process::Failure::Exit(_)) => false,
        Err(e) => { log(&format!("stale grant unknown: `codesign -v --strict` failed: {e}")); false }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn stale_grant(_: &Path, _: &Result<(), String>) -> bool {
    false
}

/// TCC grants a gui worker needs besides Screen Recording (#1845): each `allowed`, `missing`, `stale` or `unknown`.
pub const GRANTS: [(&str, &str); 2] = [("accessibility", "kTCCServiceAccessibility"), ("app_data", "kTCCServiceSystemPolicyAppData")];

/// Accessibility reads `AXIsProcessTrusted` (no prompt); a denial is `stale` when tccd logged a failed requirement match
/// for `<install>/taskq`, else `missing`. App Data has no preflight API: `stale` on such a tccd line, else `unknown`.
#[cfg(target_os = "macos")]
pub fn grants(install: &Path) -> BTreeMap<String, String> {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    // ponytail: one `log show` over the last hour per probe (300 s); an App Data grant no worker touched within it reads unknown
    // #1856: an unreadable log is logged with its command; the grants then read as without it
    let log = crate::tools::run("gui.tcc_log", command("/usr/bin/log").args(["show", "--last", "1h", "--style", "compact", "--predicate",
        "subsystem == \"com.apple.TCC\" AND eventMessage CONTAINS \"Failed to match existing code requirement\""]), std::time::Duration::from_secs(30))
        .map_err(|e| crate::platform::log(&format!("gui grants: {e}"))).ok().map(|o| String::from_utf8_lossy(&o).into_owned());
    // SAFETY: no arguments; reads this process's TCC state without a prompt
    grant_states(unsafe { AXIsProcessTrusted() }, log.as_deref(), &install.join("taskq"))
}

#[cfg(not(target_os = "macos"))]
pub fn grants(_: &Path) -> BTreeMap<String, String> {
    BTreeMap::new()
}

/// `log`: tccd lines, `None` when unreadable (Accessibility denials then read `missing`, App Data `unknown`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn grant_states(trusted: bool, log: Option<&str>, binary: &Path) -> BTreeMap<String, String> {
    let subject = format!("for subject {} ", binary.display());
    let stale = |service: &str| log.is_some_and(|l| l.lines().any(|x| x.contains(&subject) && x.contains(service)));
    GRANTS.iter().map(|(k, service)| {
        let state = match (*k, stale(service)) {
            (_, true) if !(*k == "accessibility" && trusted) => "stale",
            ("accessibility", _) => if trusted { "allowed" } else { "missing" },
            _ => "unknown",
        };
        (k.to_string(), state.to_string())
    }).collect()
}

#[cfg_attr(not(any(target_os = "linux", target_os = "macos", windows)), allow(dead_code))]
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[cfg(target_os = "macos")]
mod mach {
    unsafe extern "C" {
        pub fn mach_host_self() -> u32;
        pub fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        pub fn host_statistics64(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        pub fn host_page_size(host: u32, size: *mut usize) -> i32;
    }

    /// One send right for the life of the process.
    pub fn host() -> u32 {
        static H: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        // SAFETY: no arguments; returns this task's host port
        *H.get_or_init(|| unsafe { mach_host_self() })
    }
}

/// `HOST_CPU_LOAD_INFO`: user, system, idle, nice ticks over all logical CPUs.
#[cfg(target_os = "macos")]
pub fn cpu_ticks() -> Option<(u64, u64)> {
    let (mut t, mut n) = ([0u32; 4], 4u32);
    // SAFETY: flavor 3 fills CPU_STATE_MAX natural_t values into a buffer of that size
    if unsafe { mach::host_statistics(mach::host(), 3, t.as_mut_ptr().cast(), &mut n) } != 0 || n != 4 {
        return None;
    }
    let total: u64 = t.iter().map(|x| *x as u64).sum();
    Some((total - t[2] as u64, total))
}

/// `HOST_VM_INFO64` (free + inactive pages) × page size / `hw.memsize`. `ponytail:` free + inactive approximates
/// reclaimable memory, not Apple's pressure metric; compressed and purgeable pages are not counted again.
#[cfg(target_os = "macos")]
pub fn memory_available() -> Option<f64> {
    vm().map(|(avail, total)| avail as f64 * 100.0 / total as f64)
}

/// Available GiB (as [`memory_available`]) and `kern.memorystatus_vm_pressure_level` (1 normal, 2 warning, 4 critical).
#[cfg(target_os = "macos")]
pub fn host_memory() -> Option<(f64, u32)> {
    let (mut level, mut len) = (0u32, std::mem::size_of::<u32>());
    // SAFETY: the level is a plain int output of the given size
    let ok = unsafe { libc::sysctlbyname(c"kern.memorystatus_vm_pressure_level".as_ptr(), (&mut level as *mut u32).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
    (ok && len == 4).then_some(())?;
    Some((vm()?.0 as f64 / GIB, level))
}

/// `(available, total)` bytes.
#[cfg(target_os = "macos")]
fn vm() -> Option<(u64, u64)> {
    let (mut v, mut n, mut page) = ([0i32; 38], 38u32, 0usize);
    let (mut total, mut len) = (0u64, std::mem::size_of::<u64>());
    // SAFETY: flavor 4 fills HOST_VM_INFO64_COUNT (38) integer_t; the size and memsize outputs are plain integers
    let ok = unsafe {
        mach::host_statistics64(mach::host(), 4, v.as_mut_ptr(), &mut n) == 0
            && mach::host_page_size(mach::host(), &mut page) == 0
            && libc::sysctlbyname(c"hw.memsize".as_ptr(), (&mut total as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0) == 0
    };
    let avail = (v[0] as u32 as u64 + v[2] as u32 as u64) * page as u64;
    (ok && total > 0 && avail <= total).then_some((avail, total))
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    pub struct MemoryStatusEx {
        pub length: u32,
        pub load: u32,
        pub total_phys: u64,
        pub avail_phys: u64,
        pub rest: [u64; 5],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetSystemTimes(idle: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
        pub fn GlobalMemoryStatusEx(m: *mut MemoryStatusEx) -> i32;
        pub fn GetTickCount64() -> u64;
    }
}

/// `GetSystemTimes`: kernel time already includes idle.
#[cfg(windows)]
pub fn cpu_ticks() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (0u64, 0u64, 0u64);
    // SAFETY: three FILETIME outputs, each the layout of a little-endian u64
    if unsafe { win::GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let total = kernel.checked_add(user)?;
    Some((total.checked_sub(idle)?, total))
}

#[cfg(windows)]
pub fn memory_available() -> Option<f64> {
    let mut m = win::MemoryStatusEx { length: std::mem::size_of::<win::MemoryStatusEx>() as u32, load: 0, total_phys: 0, avail_phys: 0, rest: [0; 5] };
    // SAFETY: dwLength set as the API requires
    if unsafe { win::GlobalMemoryStatusEx(&mut m) } == 0 || m.total_phys == 0 || m.avail_phys > m.total_phys {
        return None;
    }
    Some(m.avail_phys as f64 * 100.0 / m.total_phys as f64)
}

/// Available GiB; Windows has no pressure level here (0 never holds).
#[cfg(windows)]
pub fn host_memory() -> Option<(f64, u32)> {
    let mut m = win::MemoryStatusEx { length: std::mem::size_of::<win::MemoryStatusEx>() as u32, load: 0, total_phys: 0, avail_phys: 0, rest: [0; 5] };
    // SAFETY: dwLength set as the API requires
    (unsafe { win::GlobalMemoryStatusEx(&mut m) } != 0).then(|| (m.avail_phys as f64 / GIB, 0))
}

/// Host boot as Unix seconds (#1386): proof that every process of an earlier intent ended.
#[cfg(target_os = "linux")]
pub fn boot_time() -> Result<i64, String> {
    let t = fs::read_to_string("/proc/stat").map_err(|e| format!("/proc/stat: {e}"))?;
    t.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok()).ok_or("/proc/stat: no btime".into())
}

#[cfg(target_os = "macos")]
pub fn boot_time() -> Result<i64, String> {
    let mut tv = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let mut len = std::mem::size_of::<libc::timeval>();
    // SAFETY: kern.boottime fills one struct timeval of the given size
    let ok = unsafe { libc::sysctlbyname(c"kern.boottime".as_ptr(), (&mut tv as *mut libc::timeval).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
    (ok && tv.tv_sec > 0).then_some(tv.tv_sec as i64).ok_or("sysctl kern.boottime failed".into())
}

/// Wall clock minus `GetTickCount64`. `ponytail:` Fast Startup resumes a hibernated kernel without resetting the
/// tick count, so its boot reads older and keeps executions held; never the unsafe direction.
#[cfg(windows)]
pub fn boot_time() -> Result<i64, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?;
    // SAFETY: no arguments
    let up = std::time::Duration::from_millis(unsafe { win::GetTickCount64() });
    now.checked_sub(up).map(|t| t.as_secs() as i64).ok_or("GetTickCount64 exceeds the wall clock".into())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn boot_time() -> Result<i64, String> {
    Err("boot time unsupported on this OS".into())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn cpu_ticks() -> Option<(u64, u64)> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn memory_available() -> Option<f64> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn host_memory() -> Option<(f64, u32)> {
    None
}

/// Seconds east of UTC on this host's clock at board time `t` (R6 local time); 0 when the platform does not say.
#[cfg(unix)]
pub fn utc_offset(t: i64) -> i64 {
    let (at, mut tm) = (t as libc::time_t, unsafe { std::mem::zeroed::<libc::tm>() });
    if unsafe { libc::localtime_r(&at, &mut tm) }.is_null() { 0 } else { tm.tm_gmtoff as i64 }
}

#[cfg(windows)]
pub fn utc_offset(t: i64) -> i64 {
    // SYSTEMTIME: year, month, weekday, day, hour, minute, second, millisecond
    #[link(name = "kernel32")]
    unsafe extern "system" { fn SystemTimeToTzSpecificLocalTime(zone: *const std::ffi::c_void, utc: *const [u16; 8], local: *mut [u16; 8]) -> i32; }
    let s = crate::board::stamp(t);
    let f = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<u16>().ok()).unwrap_or(0);
    let (utc, mut l) = ([f(0, 4), f(5, 7), 0, f(8, 10), f(11, 13), f(14, 16), f(17, 19), 0], [0u16; 8]);
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut l) } == 0 { return 0; }
    crate::board::iso(&format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", l[0], l[1], l[3], l[4], l[5], l[6])).map_or(0, |x| x - t)
}

#[cfg(not(any(unix, windows)))]
pub fn utc_offset(_: i64) -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines of `text` before its `#[cfg(test)] mod … {`: test processes may start children their own way.
    fn product(text: &str) -> Vec<&str> {
        let lines: Vec<&str> = text.lines().collect();
        let end = (0..lines.len()).find(|&i| lines[i].trim_start().starts_with("#[cfg(test)]") && {
            let next = lines[i + 1..].iter().map(|l| l.trim()).find(|l| !l.starts_with("#[")).unwrap_or("");
            next.trim_start_matches("pub(crate) ").starts_with("mod ") && next.ends_with('{')
        }).unwrap_or(lines.len());
        lines[..end].to_vec()
    }

    // Defect #2049 A2: tasklist, git, the launcher and codex started without CREATE_NO_WINDOW, so the windowless
    // service flashed a console window per start. Every product start goes through `command` (or the pass-through's
    // `console_command`); this enumerates the spawn sites and names any raw `Command::new` or a flag that drops the bit.
    #[test]
    fn spawn_sites() {
        let (mut sites, mut raw) = (vec![], vec![]);
        let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() { dirs.push(path); continue; }
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with(".rs") || name.ends_with("_tests.rs") { continue; }
                let text = fs::read_to_string(&path).unwrap();
                let file = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display().to_string();
                for (i, line) in product(&text).iter().enumerate() {
                    let at = format!("{file}:{}", i + 1);
                    if line.contains("platform::command(") || line.contains("platform::console_command(") { sites.push(at.clone()); }
                    let helper = name == "platform.rs" && line.trim() == "let mut c = std::process::Command::new(program);";
                    if line.contains("Command::new(") && !helper { raw.push(format!("{at}: {}", line.trim())); }
                    if let Some(flags) = line.split("creation_flags(0x").nth(1) {
                        let hex: String = flags.chars().take_while(|c| c.is_ascii_hexdigit() || *c == '_').filter(|c| *c != '_').collect();
                        if u32::from_str_radix(&hex, 16).map_or(true, |f| f & 0x0800_0000 == 0) { raw.push(format!("{at}: drops CREATE_NO_WINDOW: {}", line.trim())); }
                    }
                }
            }
        }
        assert!(raw.is_empty(), "start these through platform::command:\n{}", raw.join("\n"));
        // the scan found the starts: a moved src or a misread test boundary would pass with nothing checked
        for site in ["src/runtime/owner.rs", "src/board/github.rs", "src/main.rs", "src/windows_sandbox.rs", "src/runtime.rs"] {
            assert!(sites.iter().any(|s| s.replace('\\', "/").starts_with(site)), "no spawn site in {site}: {sites:?}");
        }
        assert!(sites.len() > 50, "{} spawn sites: {sites:?}", sites.len());
    }

    // Defect #1803 A4: a gui probe that captured without an allowed preflight raised a Screen Recording prompt.
    #[test]
    fn gui_capture_runs_only_after_an_allowed_preflight() {
        let mut ran = false;
        assert_eq!(gated(false, || { ran = true; Ok(()) }), Err(NOT_ALLOWED.to_string()));
        assert!(!ran, "capture ran after a refused preflight");
        assert_eq!(gated(true, || Err("screencapture wrote no PNG".into())), Err("screencapture wrote no PNG".to_string()));
    }

    // Defect #1845 A1: only Screen Recording was probed; a stale Accessibility or App Data grant surfaced as a worker
    // ask. Risk: a state misread lets a gui task run into the denial or blocks it on a healthy grant.
    #[test]
    fn grants_read_allowed_missing_stale_and_unknown() {
        let bin = Path::new("/i/taskq");
        let line = |s: &str| format!("tccd: Failed to match existing code requirement for subject /i/taskq and service {s}\n");
        let st = |t, l: Option<&str>| { let g = grant_states(t, l, bin); (g["accessibility"].clone(), g["app_data"].clone()) };
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(st(true, Some("")), s("allowed", "unknown"));
        assert_eq!(st(false, None), s("missing", "unknown"), "unreadable log: denial is missing, App Data unknown");
        assert_eq!(st(false, Some(&line("kTCCServiceAccessibility"))), s("stale", "unknown"));
        assert_eq!(st(true, Some(&line("kTCCServiceAccessibility"))), s("allowed", "unknown"), "a trusted process is never stale");
        assert_eq!(st(true, Some(&line("kTCCServiceSystemPolicyAppData"))), s("allowed", "stale"));
        assert_eq!(st(true, Some(&line("kTCCServiceSystemPolicyAppData").replace("/i/taskq", "/other/taskq"))), s("allowed", "unknown"), "another binary's line");
    }

    // Defect #1445: service and status.json were written in place without a flush; a power cut left zeros.
    #[test]
    fn synced_write_replaces_whole_record_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("taskq-synced-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("service");
        write_synced(&p, "old").unwrap();
        write_synced(&p, "new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "temp file removed");
        let me = coreq::process::identity(std::process::id()).unwrap().unwrap();
        assert!(!started_before_boot(&me.started).unwrap(), "this process started in this boot");
        for invalid in ["", "invented-previous-incarnation"] {assert!(started_before_boot(invalid).is_err(),"malformed start cannot prove a later boot");}
        fs::remove_dir_all(dir).unwrap();
    }

    // #1381: project taskq's log is the host unit's stderr file; a line written to both landed twice.
    #[test]
    fn service_line_lands_once_when_project_log_is_stderr_target() {
        let dir = std::env::temp_dir().join(format!("taskq-log-once-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let (own, other) = (dir.join("taskq.log"), dir.join("demo.log"));
        let mut stderr = fs::OpenOptions::new().create(true).append(true).open(&own).unwrap();
        emit("a", Some(&own), &mut stderr, Some(&own));
        emit("b", Some(&own), &mut stderr, Some(&own));
        assert_eq!(fs::read_to_string(&own).unwrap(), "a\nb\n");
        emit("c", Some(&other), &mut stderr, Some(&own));
        assert_eq!((fs::read_to_string(&own).unwrap(), fs::read_to_string(&other).unwrap()), ("a\nb\nc\n".into(), "c\n".into()));
        emit("d", Some(&dir.join("missing/x.log")), &mut stderr, Some(&dir.join("missing/x.log")));
        assert!(fs::read_to_string(&own).unwrap().ends_with("c\nd\n"));
        fs::remove_dir_all(dir).unwrap();
    }

    // Chain: § 4 service log lines. Risk: a line without its UTC time, or one message split over lines.
    // Chain: short adapter -> silent/nonzero dependency -> bounded response and private failure evidence.
    #[test]
    fn short_dependency_outputs_are_bounded_and_preserve_nonzero_receipts() {
        crate::with_test_capture(|store,_| {
            let fixture=|mode:&str| {
                let mut command=std::process::Command::new(std::env::current_exe().unwrap());
                command.args(["--exact","platform::tests::short_dependency_child","--ignored","--nocapture"]).env("TASKQ_SHORT_DEPENDENCY",mode);command
            };
            let began=std::time::Instant::now();
            let error=output("fixture.silent",&mut fixture("silent"),std::time::Duration::from_millis(100)).unwrap_err();
            assert!(error.contains("timeout")&&error.contains("unknown"));
            assert!(began.elapsed()<std::time::Duration::from_secs(2));
            let receipt=output("fixture.nonzero",&mut fixture("nonzero"),std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(receipt.status.code(),Some(7));
            assert!(String::from_utf8_lossy(&receipt.stderr).contains("private dependency receipt"));
            tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().unwrap().flush().unwrap());
            let export=store.export(0,0,1024).unwrap();let rows=export["events"].as_array().unwrap();
            for class in ["timeout","exit"] {assert!(rows.iter().any(|r|r["observation"]["error"]==class));}
            assert!(!export.to_string().contains("private dependency receipt"));
        });
    }

    #[test]
    #[ignore="owned silent/nonzero subprocess for the short dependency adapter"]
    fn short_dependency_child() {
        if std::env::var("TASKQ_SHORT_DEPENDENCY").unwrap()=="nonzero" {eprintln!("private dependency receipt");std::process::exit(7);}
        loop {std::thread::park();}
    }

    // Chain: #1625 § 4 service exit -> stderr redirected to taskq.log by the launcher -> process ends with the code.
    // Risk: a service exit leaves no reason in taskq.log, or the line is lost when Windows terminates the process.
    #[test]
    fn service_exit_logs_reason_before_process_ends() {
        let dir = std::env::temp_dir().join(format!("taskq-service-exit-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("taskq.log");
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::tests::service_exit_child", "--ignored", "--nocapture"])
            .env("TASKQ_SERVICE_EXIT", "1").stderr(fs::File::create(&log).unwrap()).output().unwrap();
        assert_eq!(o.status.code(), Some(75));
        let text = fs::read_to_string(&log).unwrap();
        assert_eq!(text.lines().filter(|l| l.ends_with(" service exits 75: restart to 9.9.9")).count(), 1, "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "owned subprocess that ends through service_exit"]
    fn service_exit_child() {
        if std::env::var_os("TASKQ_SERVICE_EXIT").is_none() { return; }
        service_log();
        service_exit(75, "restart to 9.9.9");
    }

    #[test]
    fn service_log_lines_start_with_utc_time_and_fold() {
        let at = crate::board::iso("2026-10-14T04:27:05Z").unwrap();
        assert_eq!(log_line(at, "pass: PATCH x: HTTP 502 <html>\n  <p>bad</p>\r\n\n</html>"),"2026-10-14T04:27:05Z pass: PATCH x: HTTP 502 <html> <p>bad</p> </html>");
    }

    // Chain: #1386 boot release. Risk: a wrong boot reading releases a live execution or never releases a dead one.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn boot_time_reads_on_this_host() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let boot = boot_time().unwrap();
        assert!(boot > 1_000_000_000 && boot <= now, "boot {boot} now {now}");
        assert!((boot_time().unwrap() - boot).abs() <= 2, "boot time is stable");
    }

    // Chain: § 3 Auto memory floor. Risk: the floor or pressure reading fails on a real host and Auto never admits.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn host_memory_reads_on_this_host() {
        let memory = host_memory();
        #[cfg(target_os = "macos")]
        if memory.is_none() {
            use std::io::Write;
            writeln!(std::io::stderr(), "sandbox-blocked: host_memory_reads_on_this_host: OS memory probe unavailable").unwrap();
            return;
        }
        let (gib, level) = memory.unwrap();
        assert!(gib > 0.0 && level <= 4, "{gib} GiB, level {level}");
    }

    // Chain: § 8 shared update across services. Risk: a second service downloads or misses the update.
    #[test]
    fn two_services_share_current_without_download_or_smoke() {
        struct FakeService { version: String, starts: usize }
        let install = std::env::temp_dir().join(format!("taskq-shared-update-{}", std::process::id()));
        fs::create_dir_all(&install).unwrap();
        write_atomic(&install.join("current"), VERSION).unwrap();
        let mut first = FakeService { version: VERSION.into(), starts: 1 };
        let mut second = FakeService { version: VERSION.into(), starts: 1 };
        // First service's successful update publishes current; the second still runs the old image.
        let next = "999.0.0";
        write_atomic(&install.join("current"), next).unwrap();
        first.version = next.into();
        first.starts += 1;
        assert!(coreq::update::newer(&tool(&install).current().unwrap(), &second.version));
        if let Some(v) = service_update(&install).unwrap() {
            second.version = v;
            second.starts += 1;
        }
        assert_eq!(second.starts, 2);
        assert_eq!(second.version, first.version);
        // No release executable exists to smoke, and even the download directory was never created.
        assert!(!install.join("releases").exists());
        assert!(!coreq::update::newer(next, &tool(&install).current().unwrap()));
        assert_eq!(fs::read_to_string(install.join("current")).unwrap().trim(), next);
        fs::remove_dir_all(&install).unwrap();
    }

    // Chain: § 8 item 1 dev install (#1388). Risk: a dev Mac never installs a green main merge, installs one with red or
    // pending CI, reinstalls the same or a failed SHA in a loop, starts a second build, or a live host builds at all.
    #[test]
    fn dev_install_starts_each_green_main_sha_once() {
        let (a,b)=("a".repeat(40),"b".repeat(40));
        let mut reads=vec![];
        let mut green=|sha:&str|{reads.push(sha.to_string());Ok(())};
        assert_eq!(dev_next("dev",Some(&b),Some(&a),None,false,&mut green),Some(b.clone()),"new green SHA installs");
        assert_eq!(dev_next("dev",Some(&a),Some(&a),None,false,&mut green),None,"same SHA: no-op");
        assert_eq!(dev_next("dev",Some(&b),Some(&a),Some(&b),false,&mut green),None,"started or failed SHA: not retried");
        assert_eq!(dev_next("dev",Some(&b),Some(&a),None,true,&mut green),None,"one build at a time");
        assert_eq!(dev_next("live",Some(&b),Some(&a),None,false,&mut green),None,"channel live: no-op");
        assert_eq!(dev_next("dev",None,Some(&a),None,false,&mut green),None,"unknown main SHA waits");
        assert_eq!(reads,vec![b.clone()],"CI is read only for a SHA that could start");
        for ci in ["merge_group CI not green on merge result","CI tests pending"] {
            assert_eq!(dev_next("dev",Some(&b),Some(&a),None,false,&mut |_|Err(ci.into())),None,"red or pending CI waits");
        }
        // the script's report folds once into installed/failed; a started SHA stays started, so it never restarts
        #[cfg(unix)] {
        let install=std::env::temp_dir().join(format!("taskq-dev-{}",std::process::id()));
        let _=fs::remove_dir_all(&install);
        fs::create_dir_all(install.join("dev")).unwrap();
        fs::write(install.join("dev/installed"),&a).unwrap();
        fs::write(install.join("dev/started"),&b).unwrap();
        fs::write(install.join("dev/result"),format!("{b} failed\n")).unwrap();
        let v=dev_tick(&install,&install,&mut |_|panic!("no main SHA outside a checkout"));
        assert_eq!((v["installed"].as_str(),v["failed"].as_str(),v["building"].as_bool()),(Some(a.as_str()),Some(b.as_str()),Some(false)));
        assert!(!install.join("dev/result").exists(),"report read once");
        fs::write(install.join("dev/result"),format!("{b} installed")).unwrap();
        let v=dev_tick(&install,&install,&mut |_|Ok(()));
        assert_eq!((v["installed"].as_str(),v["failed"].as_str()),(Some(b.as_str()),None),"success clears the failed SHA");
        fs::remove_dir_all(&install).unwrap();
        }
    }

    // Chain: § 8 item 1 dev install gate (#1406). Risk: a root checked out before scripts/dev-install.sh existed never
    // installs a green main merge, or the install runs the root's own copy or touches its working tree.
    #[cfg(unix)]
    #[test]
    fn dev_install_runs_the_fetched_script_from_a_root_without_one() {
        let base=std::env::temp_dir().join(format!("taskq-dev-gate-{}",std::process::id()));
        let _=fs::remove_dir_all(&base);
        let (origin,root,w,install)=(base.join("origin.git"),base.join("root"),base.join("w"),base.join("install"));
        fs::create_dir_all(install.join("dev")).unwrap();
        let git=|dir:&Path,args:&[&str]|{
            let o=std::process::Command::new("git").arg("-C").arg(dir).args(["-c","user.name=t","-c","user.email=t@example.invalid"]).args(args).output().unwrap();
            assert!(o.status.success(),"git {args:?}: {}",String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&base,&["init","-q","--bare","-b","main",origin.to_str().unwrap()]);
        git(&base,&["clone","-q",origin.to_str().unwrap(),w.to_str().unwrap()]);
        git(&w,&["checkout","-q","-b","main"]);
        fs::write(w.join("README"),"old main\n").unwrap();
        git(&w,&["add","README"]);git(&w,&["commit","-qm","old"]);git(&w,&["push","-q","origin","main"]);
        git(&base,&["clone","-q",origin.to_str().unwrap(),root.to_str().unwrap()]);
        // the new main adds the script; the root stays at the old commit without it
        let script="touch \"$0.done\"\n";
        fs::create_dir_all(w.join("scripts")).unwrap();
        fs::write(w.join("scripts/dev-install.sh"),script).unwrap();
        git(&w,&["add","scripts"]);git(&w,&["commit","-qm","new"]);git(&w,&["push","-q","origin","main"]);
        let sha=git(&w,&["rev-parse","HEAD"]);
        assert!(!root.join("scripts/dev-install.sh").exists());
        // #1633: another fetch in the root holds origin/main's ref lock; the install must not need that ref
        let tracking=git(&root,&["rev-parse","origin/main"]);
        fs::write(root.join(".git/refs/remotes/origin/main.lock"),"").unwrap();
        fs::write(install.join("host.json"),r#"{"channel":"dev"}"#).unwrap();
        assert!(dev_enabled(RELEASE_REPO,&install),"gate needs no script in the root");
        assert!(!dev_enabled("other/repo",&install)&&!dev_enabled(RELEASE_REPO,&base),"other repo or channel live: off");
        // the start line goes to this thread's project log
        service_log();
        project_log(base.join("project.log"));
        let mut read=None;
        dev_tick(&install,&root,&mut |s|{read=Some(s.to_string());Ok(())});
        assert_eq!(read.as_deref(),Some(sha.as_str()),"CI read for the new main SHA");
        assert_eq!(fs::read_to_string(install.join("dev/started")).unwrap().trim(),sha);
        assert!(fs::read_to_string(base.join("project.log")).unwrap().contains(&format!("dev install: start {sha}")));
        let done=install.join("dev/install.sh.done");
        for _ in 0..100 { if done.exists() {break;} std::thread::sleep(std::time::Duration::from_millis(100)); }
        assert!(done.exists(),"fetched script ran: {}",fs::read_to_string(install.join("dev/install.log")).unwrap_or_default());
        assert_eq!(fs::read_to_string(install.join("dev/install.sh")).unwrap(),script,"script taken from the fetched SHA");
        assert!(!root.join("scripts/dev-install.sh").exists()&&git(&root,&["status","--porcelain"]).is_empty(),"root working tree untouched");
        assert_eq!(git(&root,&["rev-parse","origin/main"]),tracking,"shared remote-tracking ref untouched");
        fs::remove_dir_all(&base).unwrap();
    }

    // Chain: § 8 item 1 host channel dev -> live. Risk: a dev host installs a release, a live host stays on a -dev
    // build forever, or a missing channel stops releases.
    #[test]
    fn channel_dev_holds_releases_and_live_replaces_a_dev_build() {
        let install=std::env::temp_dir().join(format!("taskq-channel-{}",std::process::id()));
        let _=fs::remove_dir_all(&install);
        let t=tool(&install);
        let (dev,release)=(format!("{VERSION}-dev.1"),"0.9.0");
        // #2049 A1: a Windows update derives the GUI launcher from the release, so the fixture is a console PE image.
        let image=|v:&str|-> Vec<u8> {
            if !cfg!(windows) {return v.as_bytes().to_vec();}
            let mut b=vec![0u8;0x40+24+240];
            b[..2].copy_from_slice(b"MZ");b[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());b[0x40..0x44].copy_from_slice(b"PE\0\0");
            b[0x40+20..0x40+22].copy_from_slice(&240u16.to_le_bytes());b[0x40+24..0x40+26].copy_from_slice(&0x20bu16.to_le_bytes());
            b[0x40+24+68..0x40+24+70].copy_from_slice(&3u16.to_le_bytes());b.extend_from_slice(v.as_bytes());b
        };
        for v in [dev.as_str(),release] {fs::create_dir_all(t.release_exe(v).parent().unwrap()).unwrap();fs::write(t.release_exe(v),image(v)).unwrap();}
        write_atomic(&install.join("current"),&dev).unwrap();
        let latest=std::cell::RefCell::new(release.to_string());
        let fetch=|name:&str,to:&Path| -> Result<(),String> {
            if name!="latest.json" {return Err("no network in tests".into());}
            let v=latest.borrow().clone();
            let file=format!("taskq-{v}-{}{}",coreq::update::target(),std::env::consts::EXE_SUFFIX);
            fs::write(to,serde_json::json!({"version":v,"files":{coreq::update::target():file}}).to_string()).map_err(|e|e.to_string())
        };
        // dev: neither a newer nor an older release replaces the running version
        fs::write(install.join("host.json"),r#"{"channel":"dev"}"#).unwrap();
        assert_eq!(channel(&install).unwrap(),"dev");
        for v in ["999.0.0",release] {
            *latest.borrow_mut()=v.into();
            assert_eq!(channel_update(&install,&fetch).unwrap(),None);
            assert_eq!(t.current().unwrap(),dev);
        }
        // dev still re-execs into a newer local current written by dev-install
        write_atomic(&install.join("current"),"999.0.1-dev.2").unwrap();
        assert_eq!(channel_update(&install,&fetch).unwrap().as_deref(),Some("999.0.1-dev.2"));
        write_atomic(&install.join("current"),&dev).unwrap();
        // missing channel = live: the latest release replaces the higher -dev build at the next check
        fs::remove_file(install.join("host.json")).unwrap();
        assert_eq!(channel(&install).unwrap(),"live");
        assert_eq!(channel_update(&install,&fetch).unwrap().as_deref(),Some(release));
        assert_eq!((t.current().unwrap(),fs::read_to_string(install.join("previous")).unwrap()),(release.to_string(),dev.clone()));
        assert_eq!(fs::read(install.join(t.exe_name())).unwrap(),image(release));
        if cfg!(windows) {assert!(install.join(t.gui_exe_name()).is_file(),"the update places the GUI launcher the Scheduled Tasks start");}
        fs::write(install.join("host.json"),r#"{"channel":"beta"}"#).unwrap();
        assert!(channel(&install).is_err());
        fs::remove_dir_all(&install).unwrap();
    }

    // Chain: incident update.smoke. Risk: download failures reported as smoke failures.
    #[test]
    fn incident_smoke_detection_excludes_download_and_security_failures() {
        crate::incident::take_detected();
        let smoke = Err("4.2.0 selftest failed: private diagnostic".into());
        assert_eq!(update_result(smoke.clone()), smoke);
        assert_eq!(crate::incident::take_detected(), ["update.smoke"]);
        for e in ["download failed", "sha256 not in SHA256SUMS", "curl: timeout"] {
            let failure = Err(e.into());
            assert_eq!(update_result(failure.clone()), failure);
        }
        assert!(crate::incident::take_detected().is_empty());
    }

    // Chain: § 3 load gate readings. Risk: load read wrong or missing on this OS.
    #[test]
    fn load_readings_parse_and_read_on_this_host() {
        assert_eq!(proc_stat("cpu  10 2 8 70 5 1 1 3 4 0\ncpu0 1 1 1 1 1 1 1 1 1 1\n"), Some((25, 100)), "idle + iowait; guest not added");
        assert_eq!(proc_stat("intr 1"), None);
        assert_eq!(meminfo("MemTotal:  1000 kB\nMemFree: 100 kB\nMemAvailable:   250 kB\n"), Some(25.0));
        assert_eq!(meminfo("MemTotal:  1000 kB\nMemFree: 100 kB\n"), None, "no MemAvailable: unknown");
        // R12: a native reading on this platform, plausible bounds only
        let (busy, total) = cpu_ticks().expect("cpu ticks");
        assert!(busy <= total && total > 0);
        let m = memory_available().expect("available memory");
        assert!(m > 0.0 && m < 100.0, "{m}");
    }

    // H-A5/#973: local signing and repeated preparation cannot rewrite the immutable release or active files.
    #[test]
    fn repeated_install_preparation_preserves_immutable_release() {
        let install=std::env::temp_dir().join(format!("taskq-install-self-{}",crate::runtime::uuid()));
        let t=tool(&install);
        let first=prepare_install(&t).unwrap();
        assert!(!install.join("current").exists()&&!install.join(t.exe_name()).exists());
        let bytes=fs::read(t.release_exe(VERSION)).unwrap();
        #[cfg(unix)] let inode={use std::os::unix::fs::MetadataExt;fs::metadata(t.release_exe(VERSION)).unwrap().ino()};
        assert_eq!(prepare_install(&t).unwrap(),first);
        assert_eq!(fs::read(t.release_exe(VERSION)).unwrap(),bytes);
        #[cfg(unix)] {use std::os::unix::fs::MetadataExt;assert_eq!(fs::metadata(t.release_exe(VERSION)).unwrap().ino(),inode);}
        activate_install(&t).unwrap();
        assert_eq!(fs::read(install.join(t.exe_name())).unwrap(),bytes);
        assert_eq!(fs::read_to_string(install.join("current")).unwrap(),VERSION);
        fs::write(t.release_exe(VERSION),"changed-same-version").unwrap();
        assert!(prepare_install(&t).unwrap_err().contains("differs"));
        assert_eq!(fs::read(t.release_exe(VERSION)).unwrap(),b"changed-same-version");
        fs::remove_dir_all(install).unwrap();
    }

    // Windows install layout: relocated executable/real launcher must find GNU's loader without Cargo's PATH.
    #[cfg(windows)]
    #[test]
    fn installed_windows_release_starts_without_build_paths() {
        let base=std::env::temp_dir().join(format!("taskq-windows-install-{}",crate::runtime::uuid()));
        let appdata=base.join("appdata");let install=appdata.join("taskq");let t=tool(&install);
        prepare_install(&t).unwrap();activate_install(&t).unwrap();
        let system32=PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        for exe in [t.release_exe(VERSION),install.join(t.exe_name())] {
            let out=coreq::process::output("test.installed-windows-binary",std::process::Command::new(&exe)
                .args(["--exact","platform::tests::installed_windows_binary_fixture","--ignored","--nocapture"])
                .current_dir(&base).env("PATH",&system32).env("LOCALAPPDATA",&appdata).env_remove("TASKQ_ROOT")
                .env("TASKQ_INSTALL_FIXTURE",&install).env("TASKQ_SELFTEST_OFFLINE","1").env("TASKQ_INCIDENT_REPORTS","off"),std::time::Duration::from_secs(30));
            assert!(out.is_ok(),"{}: {out:?}",exe.display());
            assert!(String::from_utf8_lossy(&out.unwrap().stdout).contains("golden traces PASS"));
        }
        #[cfg(target_env="gnu")] for folder in [&install,t.release_exe(VERSION).parent().unwrap()] {assert!(folder.join("WebView2Loader.dll").is_file());}
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore="owned relocated-binary fixture; requires TASKQ_INSTALL_FIXTURE"]
    fn installed_windows_binary_fixture() {
        let install=PathBuf::from(std::env::var_os("TASKQ_INSTALL_FIXTURE").expect("owned fixture installation"));
        if std::env::current_exe().unwrap().parent()==Some(install.as_path()) {
            coreq::launcher::launch(&tool(&install),&std::env::args().skip(1).collect::<Vec<_>>());
        }
        let _telemetry=coreq::telemetry::capture::Runtime::install_bounded(
            &install.join("telemetry.log/universal.sqlite3"),"taskq",VERSION,crate::telemetry::budget(&install,"telemetry.sqlite3").unwrap(),
        ).unwrap();
        crate::run(&["selftest".into()]).unwrap();
    }

    // Chain: § 8 step 5 macOS signing (#1089). Risk: an absent identity or a failed signature passes silently, or two
    // releases end up with requirements TCC does not match as one app.
    #[cfg(target_os = "macos")]
    #[test]
    fn local_identity_signs_releases_under_one_requirement() {
        let dir = std::env::temp_dir().join(format!("taskq-sign-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        fs::copy("/usr/bin/true", &a).unwrap();
        fs::copy("/usr/bin/false", &b).unwrap();
        let before = fs::read(&a).unwrap();
        assert_eq!(codesign("TaskQ No Such Identity", &a, "org.taskq.taskq"), Ok(false));
        assert_eq!(fs::read(&a).unwrap(), before, "an absent identity touched the file");
        let failed = std::process::Command::new("/bin/sh").args(["-c", "echo boom >&2; exit 1"]).output().unwrap();
        assert!(identity_hash(SIGNING_IDENTITY, &failed).unwrap_err().contains("boom"));
        // ponytail: CI has no identity; the rest runs on a host where `service install` signs
        let signed = codesign(SIGNING_IDENTITY, &a, "org.taskq.taskq");
        assert!(signed.is_ok(), "{signed:?}");
        if signed == Ok(true) {
            assert_eq!(codesign(SIGNING_IDENTITY, &b, "org.taskq.taskq"), Ok(true));
            let req = |p: &Path| String::from_utf8(std::process::Command::new("codesign").args(["-d", "-r-"]).arg(p).output().unwrap().stdout).unwrap();
            assert_eq!(req(&a), req(&b));
            assert!(req(&a).contains("identifier \"org.taskq.taskq\" and certificate leaf"), "{}", req(&a));
            assert!(codesign(SIGNING_IDENTITY, &dir.join("missing"), "org.taskq.taskq").is_err());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    // H-A5: all update entrypoints respect installation ownership, including the current-version fast path.
    #[test]
    fn installation_transition_fences_updates_before_release_selection_or_effects() {
        let install=std::env::temp_dir().join(format!("taskq-install-update-{}",crate::runtime::uuid()));
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("current"),"999.0.0").unwrap();
        let lock=update_lock(&install).unwrap();
        assert!(service_update(&install).unwrap_err().contains("busy"));
        for check in [true,false] {assert!(update(&install,check).unwrap_err().contains("busy"));}
        drop(lock);
        // An installer crash releases the OS lock but leaves its recovery/admission marker.
        let prepare=format!("{}.pending",crate::host::INSTALL_PENDING);
        for name in ["projects.pending",crate::host::INSTALL_PENDING,prepare.as_str()] {
            fs::write(install.join(name),"retained-install-binding").unwrap();
            assert!(service_update(&install).unwrap_err().contains(name));
            for check in [true,false] {assert!(update(&install,check).unwrap_err().contains(name));}
            assert_eq!(fs::read_to_string(install.join("current")).unwrap(),"999.0.0");
            assert!(!install.join("releases").exists(),"a held transition must not download or prune");
            fs::remove_file(install.join(name)).unwrap();
        }
        assert_eq!(service_update(&install).unwrap(),Some("999.0.0".into()));
        fs::remove_dir_all(install).unwrap();
    }

    // #1415 pattern: a dev install right after the host re-execs meets each project's update step holding the lock.
    #[test]
    fn installer_waits_a_bounded_time_for_a_brief_update_holder() {
        let install = std::env::temp_dir().join(format!("taskq-update-wait-{}", crate::runtime::uuid()));
        let lock = update_lock(&install).unwrap();
        let started = std::time::Instant::now();
        assert!(update_lock_within(&install, std::time::Duration::from_millis(300)).err().is_some_and(|e| e.contains("busy")));
        assert!(started.elapsed() >= std::time::Duration::from_millis(300));
        let holder = std::thread::spawn(move || {std::thread::sleep(std::time::Duration::from_millis(200)); drop(lock)});
        assert!(update_lock_within(&install, std::time::Duration::from_secs(5)).is_ok(), "a brief holder must not fail an install");
        holder.join().unwrap();
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: CLI/service update ownership -> cleanup -> the same metadata and activation lock stays exclusive.
    #[test]
    fn updates_share_one_lock_through_cleanup_and_release_it() {
        let install = std::env::temp_dir().join(format!("taskq-update-lock-{}", crate::runtime::uuid()));
        let lock = update_lock(&install).unwrap();
        assert!(update_lock(&install).is_err(), "a competing updater must return without blocking service heartbeats");
        sweep(&install, &install.join("absent-shim"));
        let other = fs::OpenOptions::new().write(true).open(install.join("update.lock")).unwrap();
        assert!(other.try_lock().is_err(), "cleanup must not unlink the live update lock");
        #[cfg(unix)]
        let child = {
            let mut pipe = [0; 2];
            assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                // Keep the inherited descriptor alive using only async-signal-safe calls.
                unsafe {
                    libc::close(pipe[1]);
                    let mut byte = 0u8;
                    libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
                    libc::_exit(0);
                }
            }
            unsafe { libc::close(pipe[0]); }
            (pid, pipe[1])
        };
        drop(lock);
        let released = other.try_lock();
        #[cfg(unix)]
        unsafe {
            libc::write(child.1, b"x".as_ptr().cast(), 1);
            libc::close(child.1);
            let mut status = 0;
            assert_eq!(libc::waitpid(child.0, &mut status, 0), child.0);
            assert_eq!(status, 0);
        }
        released.unwrap();
        drop(other);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: § 8 install sweep. Risk: sweep deletes a live release or owned file.
    #[test]
    fn sweep_keeps_the_allowlist_and_live_releases() {
        let t = std::env::temp_dir().join(format!("taskq-sweep-{}", std::process::id()));
        let (install, root) = (t.join("install"), t.join("proj"));
        let _ = fs::remove_dir_all(&t);
        // the running version's release stays, whatever the build sets it to (release CI: the tag)
        let mine = format!("releases/{VERSION}");
        let dirs = ["releases/4.2.0", "releases/4.1.0", "releases/4.0.5", &mine, "releases/cc33ae6", "releases/4.3.0.tmp", "obsolete-service/x", "mac-pre-update-backup", "capacity", "resources", "resource-state", "windows-native/state", "failed-release-evidence"];
        for d in dirs {
            fs::create_dir_all(install.join(d)).unwrap();
        }
        fs::create_dir_all(root.join(".taskq")).unwrap();
        // a project service still on 4.0.5 with a live child: its release stays (A3)
        fs::write(root.join(".taskq/status.json"), r#"{"version": "4.0.5", "kids": [{"task": 1}]}"#).unwrap();
        // #2049 A1: the GUI launcher the Scheduled Tasks start stays; a renamed running one (`.old`) goes
        let files = ["current", "previous", "token", "taskq", "taskqw.exe", "taskqw.exe.old", "taskq.log", "taskq.log.tmp", "proj.log", "tray", "tray.json", "TaskQ-proj.xml", "worker-policy.json", "hold", "admission.json", "admission.lock", "update.lock", ".freshness.json", "current.json", "mac-scheduled-pass.py"];
        for f in files {
            fs::write(install.join(f), "").unwrap();
        }
        fs::write(install.join("hold"), "invented incident hold").unwrap();
        fs::write(install.join("current"), "4.2.0").unwrap();
        fs::write(install.join("previous"), "4.1.0").unwrap();
        fs::write(install.join("windows-native/state/cap_sid"), "native control state").unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        // H-A5: update keeps policy, interrupted writes, exact native recovery and the qualified rollback floor.
        // #1897: the GitLab board token (§ 9) survives byte-identical, like the GitHub `token`.
        let protected=[("host.json","{\"limits\":{\"codex\":2},\"owner_field\":\"keep\"}"),
            ("host.json.pending","interrupted policy; preserve"),("service","123"),("service.lock",""),("poke","wake"),
            ("restart","restart requested"),("update","update requested"),("projects.lock",""),("link-key","host link key; keep"),
            ("gitlab-token","invented-gitlab-token\n"),
            ("accounts-state.pending.json","pending quota observation; preserve"),("accounts-state.lock","quota writer lock; preserve")];
        for (name,text) in protected {fs::write(install.join(name),text).unwrap();}
        fs::create_dir_all(install.join("host-policy-migration")).unwrap();
        fs::write(install.join("host-policy-migration/original.json"),"original configuration").unwrap();
        fs::create_dir_all(install.join("link")).unwrap();
        let native=crate::kids_path(&install,&root).unwrap();fs::create_dir_all(native.parent().unwrap()).unwrap();
        let owner=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let recovery=format!("pid {} {} executor-v1\n7 41 codex 123 child 43 40\n",owner.pid,owner.started);
        fs::write(&native,&recovery).unwrap();
        coreq::launcher::set_rollback_floor(&tool(&install),"4.2.0").unwrap();
        let (shim, other) = (t.join("bin-taskq"), t.join("bin-other"));
        fs::write(&shim, format!("exec python3 {}/releases/cc33ae6/taskq.py launch --install-dir {} -- \"$@\"", install.display(), install.display())).unwrap();
        fs::write(&other, "#!/bin/sh\nexec something").unwrap();
        sweep(&install, &shim);
        sweep(&install, &other);
        let ls = |d: &Path| {
            let mut v: Vec<String> = fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
            v.sort();
            v
        };
        let mut want = vec![VERSION.to_string(), "4.0.5".into(), "4.1.0".into(), "4.2.0".into(), "4.3.0.tmp".into()];
        want.sort();
        want.dedup();
        assert_eq!(ls(&install.join("releases")), want);
        for (name,text) in protected {assert_eq!(fs::read_to_string(install.join(name)).ok().as_deref(),Some(text),"update lost {name}");}
        assert_eq!(fs::read_to_string(install.join("host-policy-migration/original.json")).unwrap(),"original configuration");
        assert_eq!(fs::read_to_string(&native).unwrap(),recovery);
        assert_eq!(crate::kids_file(&install,&root).unwrap()[0].execution,43,"native execution identity survives update");
        assert_eq!(fs::read_to_string(install.join("rollback-floor")).unwrap(),"4.2.0");
        let mut owned=vec!["TaskQ-proj.xml", "admission.json", "admission.lock", "capacity", "current", "hold", "previous", "proj.log", "projects", "releases", "resource-state", "resources", "taskq", "taskqw.exe", "taskq.log", "taskq.log.tmp", "token", "tray", "tray.json", "update.lock", "windows-native", "worker-policy.json", "native", "host-policy-migration", "rollback-floor", "link"];
        owned.extend(protected.iter().map(|(name,_)|*name));owned.sort();
        assert_eq!(ls(&install),owned);
        assert_eq!(crate::admission::hold(&install).as_deref(), Some("invented incident hold"), "update cleanup must never release an emergency hold");
        assert_eq!(fs::read_to_string(install.join("windows-native/state/cap_sid")).unwrap(), "native control state");
        assert!(!shim.exists() && other.exists(), "only the v3 shim of this install goes");
        // § 4 Data: a corrupt status.json leaves the project's version unknown, so every release stays and so does the file
        fs::write(root.join(".taskq/status.json"), "{\"version\": \"4.0").unwrap();
        fs::create_dir_all(install.join("releases/4.0.1")).unwrap();
        sweep(&install, &shim);
        assert!(install.join("releases/4.0.1").exists() && install.join("releases/4.0.5").exists());
        assert_eq!(fs::read_to_string(root.join(".taskq/status.json")).unwrap(), "{\"version\": \"4.0");
        crate::with_test_capture(|store,_| {
            fs::write(root.join(".taskq/status.json"),r#"{"version":"4.0.5"}"#).unwrap();
            fs::write(install.join("projects"),b"\xff").unwrap();
            fs::create_dir_all(install.join("releases/4.0.2")).unwrap();sweep(&install,&shim);
            assert!(install.join("releases/4.0.2").exists(),"unreadable registry cannot authorize release deletion");
            fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(install.join("projects.pending"),"retained registry prepare").unwrap();
            sweep(&install,&shim);
            assert!(install.join("releases/4.0.2").exists(),"interrupted registry cannot authorize release deletion");
            assert_eq!(fs::read_to_string(install.join("projects.pending")).unwrap(),"retained registry prepare");
            fs::remove_file(install.join("projects.pending")).unwrap();
            fs::remove_file(root.join(".taskq/status.json")).unwrap();
            sweep(&install,&shim);
            assert!(install.join("releases/4.0.2").exists(),"missing project status cannot authorize release deletion");
            tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().unwrap().flush().unwrap());
            let export=store.export(0,0,1024).unwrap();
            for class in ["registry-unreadable","version-unknown"] {
                assert!(export["events"].as_array().unwrap().iter().any(|row|row["observation"]["error"]==class),"missing cleanup failure {class}");
            }
            let private=serde_json::to_string(root.to_str().unwrap()).unwrap();
            assert!(!export.to_string().contains(private.trim_matches('"')),"private local paths entered telemetry");
        });
        let _ = fs::remove_dir_all(&t);
    }
}
