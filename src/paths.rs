//! Project and install locations: the project root, its `.taskq` directory and the running binary.

use crate::{platform};
use crate::Res;
use std::fs;
use std::path::{Path, PathBuf};

thread_local!(pub(crate) static PROJECT_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) });

pub(crate) fn find_root() -> Res<PathBuf> {
    if let Some(root) = PROJECT_ROOT.with_borrow(Clone::clone) { return Ok(root); }
    root_for(std::env::var_os("TASKQ_ROOT").map(PathBuf::from), std::env::current_dir().map_err(|e| e.to_string())?)
}

/// `TASKQ_ROOT` wins only when it contains `cwd` or `cwd` is in no project: a reused session's stale value is refused (§ 5).
pub(crate) fn root_for(env: Option<PathBuf>, cwd: PathBuf) -> Res<PathBuf> {
    if let Some(r) = env {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let (rc, dc) = (canon(&r), canon(&cwd));
        if dc.starts_with(&rc) || dc.ancestors().all(|a| !a.join("taskq.json").is_file()) {
            return Ok(r);
        }
        return Err(format!("TASKQ_ROOT {} is not the project of the current directory {}", r.display(), cwd.display()));
    }
    let mut d = cwd;
    loop {
        let in_worktree = d.parent().and_then(Path::file_name).is_some_and(|n| n == ".worktrees");
        if d.join("taskq.json").is_file() && !in_worktree {
            return Ok(d);
        }
        if !d.pop() {
            return Err("no taskq.json here or above".into());
        }
    }
}

/// § 5 Board-only root: a root without `.git` holds no code checkout, so commands that would spawn a worker or tidy
/// worktrees there refuse with one line naming the reason; the board commands run as in any root.
pub(crate) fn needs_checkout(root: &Path, what: &str) -> Res<()> {
    if root.join(".git").exists() { return Ok(()); }
    Err(format!("{what}: board-only root: no code checkout (.git) here\nNext: run it in a checkout of this project on a host that serves it"))
}

pub(crate) fn dot(root: &Path) -> PathBuf {
    root.join(".taskq")
}

/// Bump `p`'s mtime. Not `fs::write(p, "")`: on Windows rewriting an empty file keeps its mtime.
pub(crate) fn touch(p: &Path) {
    let _ = fs::write(p, ".");
}

pub(crate) fn project_name(root: &Path) -> String {
    root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "taskq".into())
}

pub(crate) fn installed(install: &Path) -> bool {
    std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).zip(install.join("releases").canonicalize().ok()).is_some_and(|(e, r)| e.starts_with(r))
}

pub(crate) fn launcher(install: &Path) -> PathBuf {
    let l = install.join(platform::tool(install).exe_name());
    if l.is_file() { l } else { std::env::current_exe().unwrap_or_else(|_| PathBuf::from("taskq")) }
}

/// Read-only recovery evidence; failed identity reads and a still-live foreign owner prevent takeover.
pub(crate) fn kids_path(install:&Path,root:&Path)->Res<PathBuf> {
    let root=root.canonicalize().map_err(|e|format!("native recovery root unavailable: {e}"))?;
    let root=root.to_str().ok_or("native recovery root is not UTF-8")?;
    let hash:String=ring::digest::digest(&ring::digest::SHA256,root.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect();
    Ok(install.join("native").join(format!("{hash}.kids")))
}
