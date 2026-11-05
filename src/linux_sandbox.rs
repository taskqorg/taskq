//! Linux §8: fixed launcher, entered only by the root-owned helper copy.
use std::{path::{Path, PathBuf}, process::Command};
use std::os::unix::{fs::MetadataExt, process::ExitStatusExt};

pub const HELPER: &str = "/usr/local/libexec/taskq-sandbox";
const BWRAP: &str = "/usr/local/libexec/taskq-bwrap";

fn trusted(path: &Path) -> Result<(), String> {
    for p in path.ancestors() {
        let m = std::fs::symlink_metadata(p).map_err(|e| format!("linux sandbox {}: {e}", p.display()))?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o6022 != 0 {
            return Err(format!("linux sandbox requires root-owned, non-writable, non-setid {}", p.display()));
        }
    }
    Ok(())
}

pub fn ready(root: &Path) -> Result<(), String> {
    for p in [HELPER, BWRAP] { trusted(Path::new(p))?; }
    prerequisites(root, &crate::runtime::accounts::active_in(&crate::platform::default_install(), "codex", &codex_home()?))
}

fn prerequisites(root: &Path, home: &Path) -> Result<(), String> {
    for p in [home.join("config.toml"), root.join(".codex")] {
        if std::fs::symlink_metadata(&p).map_err(|e| format!("linux sandbox prerequisite {}: {e}", p.display()))?.file_type().is_symlink() {
            return Err(format!("linux sandbox refuses symlink {}", p.display()));
        }
    }
    if std::fs::symlink_metadata(home).map_err(|e| format!("linux sandbox prerequisite {}: {e}", home.display()))?.file_type().is_symlink() {
        return Err(format!("linux sandbox refuses symlink {}", home.display()));
    }
    Ok(())
}

fn codex_home() -> Result<PathBuf, String> {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
        .ok_or("linux sandbox: Codex home unknown")?.canonicalize().map_err(|e| e.to_string())
}

// auth.json stays writable: Codex rotates its single-use refresh token and must persist the new one (#1424).
fn read_only_entry(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "toml") || ["hooks.json", "AGENTS.md", "plugins", "skills", "packages", "app-server-control", "app-server-daemon"].iter().any(|n| p.ends_with(n))
}

/// Workspace: the project's git common dir (`<root>/.git`), so fetch, commit and push work from linked worktrees (#1490).
// ponytail: a directory `.git` only; a root whose `.git` is a pointer file keeps Codex's read-only default.
fn git_dir(mode: &str, root: &Path) -> Option<PathBuf> {
    // The worker writes `<root>` itself: a symlink planted in an earlier turn would redirect these mounts.
    // ponytail: check-then-mount races a concurrent worker of this project; open-by-fd mounts if that matters.
    let real = |p: &Path, dir: bool| std::fs::symlink_metadata(p).is_ok_and(|m| if dir { m.is_dir() } else { m.is_file() });
    let g = root.join(".git");
    let [hooks, config] = git_read_only(&g);
    (mode == "workspace" && real(&g, true) && real(&hooks, true) && real(&config, false)).then_some(g)
}

/// Host-side git runs hooks and config outside the sandbox: both stay read-only, and must exist to be masked.
fn git_read_only(git: &Path) -> [PathBuf; 2] {
    [git.join("hooks"), git.join("config")]
}

/// Write mounts, in order; read-only mounts follow them.
fn write_mounts(mode: &str, root: &Path, git: Option<&Path>, ch: &Path, home: &Path) -> Vec<PathBuf> {
    let workspace = if mode == "workspace" { root.to_path_buf() } else { root.join(".taskq") };
    let mut v = vec![workspace];
    v.extend(git.map(Path::to_path_buf));
    v.push(ch.to_path_buf());
    v.extend([".cargo", ".rustup", ".npm", ".cache"].map(|s| home.join(s)));
    v
}

/// Native Codex workspace-write keeps `<cwd>/.git` read-only; its bwrap binds a deeper writable root after that mask.
fn codex_args(git: Option<&Path>, args: &[String]) -> Vec<String> {
    let Some(g) = git else { return args.to_vec() };
    let roots = format!("sandbox_workspace_write.writable_roots=[{}]", serde_json::to_string(&g.to_string_lossy()).unwrap_or_default());
    args.iter().map(|a| if a == "sandbox_workspace_write.writable_roots=[]" { roots.clone() } else { a.clone() }).collect()
}

pub fn run(args: &[String]) -> Result<i32, String> {
    if args.len() < 6 { return Err("linux sandbox: expected root, install, mode, Codex executable and exec arguments".into()); }
    let root = Path::new(&args[0]).canonicalize().map_err(|e| e.to_string())?;
    let install = Path::new(&args[1]).canonicalize().map_err(|e| e.to_string())?;
    let mode = &args[2];
    if !["workspace", "restricted"].contains(&mode.as_str()) { return Err("linux sandbox: restricted/workspace only".into()); }
    ready(&root)?;
    let p = crate::policy::load(&install)?;
    let scope = root.to_str().ok_or("linux sandbox: root must be UTF-8")?;
    let g = p.grants.iter().find(|g| crate::runtime::of(&g.runtime).has("sandbox-args") && crate::runtime::of(&g.runtime).has("cli") && g.project.as_deref() == Some(scope))
        .ok_or("linux sandbox: explicit project grant required")?;
    if g.mode != *mode && !(g.mode == "workspace" && mode == "restricted") { return Err("linux sandbox: grant changed or does not permit this mode".into()); }
    let bin = Path::new(&args[3]).canonicalize().map_err(|e| e.to_string())?;
    if bin != crate::runtime::agent(&g.runtime)?.executable(None)?.canonicalize().map_err(|e| e.to_string())? || args[4] != "exec" || args[5..].iter().any(|a| ["danger-full-access", "--yolo", "--dangerously-bypass-approvals-and-sandbox"].contains(&a.as_str())) {
        return Err("linux sandbox: only bounded Codex exec allowed".into());
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("linux sandbox: HOME missing")?);
    let ch = crate::runtime::accounts::active_in(&install, "codex", &codex_home()?);
    let mut c = crate::platform::command(BWRAP);
    c.args(["--die-with-parent", "--new-session", "--unshare-user", "--unshare-pid", "--unshare-ipc", "--cap-drop", "ALL", "--ro-bind", "/", "/", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--tmpfs", "/run", "--tmpfs", "/var/tmp"]);
    let dns = Path::new("/etc/resolv.conf").canonicalize().map_err(|e| format!("linux sandbox DNS: {e}"))?;
    c.arg("--ro-bind").arg(&dns).arg(&dns);
    // One check feeds the write mount, its masks and Codex, so they cannot disagree.
    let git = git_dir(mode, &root);
    for p in write_mounts(mode, &root, git.as_deref(), &ch, &home) {
        if p.exists() { let p = p.canonicalize().map_err(|e| e.to_string())?; c.arg("--bind").arg(&p).arg(&p); }
    }
    // Read-only mounts follow every write mount; no caller supplies mount arguments.
    let mut protected = vec![install, root.join(".codex")];
    protected.extend(git.iter().flat_map(|g| git_read_only(g)));
    protected.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|p| p.is_absolute() && p.is_dir()));
    if let Some(d) = bin.parent() { protected.push(if d.ends_with("bin") { d.parent().unwrap_or(d) } else { d }.to_path_buf()); }
    for e in std::fs::read_dir(&ch).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        if read_only_entry(&p) { protected.push(p); }
    }
    for p in protected {
        let p = p.canonicalize().map_err(|e| e.to_string())?;
        c.arg("--ro-bind").arg(&p).arg(&p);
    }
    c.args(["--setenv", "TMPDIR", "/tmp", "--chdir"]).arg(&root).arg("--").arg(bin).args(codex_args(git.as_deref(), &args[4..]));
    supervise(c)
}

/// The launcher stays bwrap's parent: `--die-with-parent` kills the sandbox when the thread that forked bwrap ends,
/// and a service re-exec ends the project thread that started this launcher (#1920). Killing the launcher still ends it.
pub(crate) fn supervise(mut c: Command) -> Result<i32, String> {
    // No sudo/setid escape, even when the host account has broader privileges.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 { return Err(std::io::Error::last_os_error().to_string()); }
    let s = c.status().map_err(|e| format!("linux sandbox: {e}"))?;
    coreq::telemetry::capture::before_exec();
    Ok(s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0)))
}

/// Host side of #1920: a private parent process between the host and the launcher. Self-updates never refresh the
/// root-owned helper copy, so a helper older than `supervise` still execs bwrap, and `--die-with-parent` then fires
/// when the service re-exec ends the project thread that spawned it (#2071). Forked inside the spawned child, the
/// parent is a single-thread process the host owns and the re-exec hands over; it waits for the launcher and ends
/// with its status. Killing it still ends the sandbox: the launcher carries PR_SET_PDEATHSIG SIGKILL.
pub(crate) fn own(c: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Only async-signal-safe calls: this runs between fork and exec of a multithreaded host.
    unsafe {
        c.pre_exec(|| {
            let parent = libc::getpid();
            match libc::fork() {
                -1 => Err(std::io::Error::last_os_error()),
                0 => {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 { return Err(std::io::Error::last_os_error()); }
                    if libc::getppid() != parent { return Err(std::io::Error::from_raw_os_error(libc::ESRCH)); }
                    Ok(())
                }
                launcher => {
                    // The host waits for exec on a pipe and reads stdio until EOF: hold no descriptor.
                    if libc::syscall(libc::SYS_close_range, 0u32, u32::MAX, 0u32) != 0 { for fd in 0..65536 { libc::close(fd); } }
                    // Never run a copied host handler: a stop signal ends this parent, and with it the launcher.
                    for sig in 1..32 { libc::signal(sig, libc::SIG_DFL); }
                    let mut st = 0;
                    while libc::waitpid(launcher, &mut st, 0) < 0 {
                        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) { libc::_exit(127); }
                    }
                    if libc::WIFSIGNALED(st) { libc::kill(parent, libc::WTERMSIG(st)); }
                    libc::_exit(if libc::WIFEXITED(st) { libc::WEXITSTATUS(st) } else { 128 + libc::WTERMSIG(st) })
                }
            }
        });
    }
}

#[cfg(test)]
#[path = "linux_sandbox_tests.rs"]
mod tests;
