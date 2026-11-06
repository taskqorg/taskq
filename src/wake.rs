//! Service wake-ups: the poke file, the tray reread requests and the poll wait (§ 4).

use crate::Res;
use crate::core::State;
use crate::paths::dot;
use crate::{control, snapshot, telemetry};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub(crate) fn poke(root: &Path) -> Res<()> {
    poke_locked(&wake_lock(root)?)
}

pub(crate) fn wake_lock(root: &Path) -> Res<coreq::file_lock::Lock> {
    #[cfg(windows)]
    wake_filesystem(root)?;
    coreq::telemetry::capture::operation(
        "wake.lock",
        || {
            fs::create_dir_all(dot(root)).map_err(|error| ("wake-directory", error))?;
            let file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(dot(root).join("poke"))
                .map_err(|error| ("wake-open", error))?;
            coreq::file_lock::try_lock_file(file)
                .map_err(|error| ("wake-lock", std::io::Error::from(error)))
        },
        |(class, _)| *class,
    )
    .map_err(|(class, error)| wake_refusal(class, &error))
}

/// A filesystem without file locks can never grant the wake lock, so retrying or checking the service cannot help.
/// The lock stays mandatory: a refused poke is never replaced by a lock-free one.
fn wake_refusal(class: &str, error: &std::io::Error) -> String {
    // ERROR_INVALID_FUNCTION is what LockFileEx returns from a redirector without byte-range locks.
    let no_locks = error.kind() == std::io::ErrorKind::Unsupported
        || (cfg!(windows) && error.raw_os_error() == Some(1));
    if class == "wake-lock" && no_locks {
        return format!(
            "service wake unsupported ({class}: {error}): this filesystem has no file locks; use a checkout on a local disk"
        );
    }
    format!("service wake failed ({class}: {error}); check service status")
}

#[cfg(windows)]
pub(crate) fn wake_filesystem(root: &Path) -> Res<()> {
    use std::path::{Component, Prefix};
    let wsl = |path: &Path| match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _) => {
                let server = server.to_string_lossy();
                server.eq_ignore_ascii_case("wsl.localhost") || server.eq_ignore_ascii_case("wsl$")
            }
            _ => false,
        },
        _ => false,
    };
    for path in [root.to_path_buf(), dot(root), dot(root).join("poke")] {
        if wsl(&path) {
            return Err(
                "service wake unsupported on Windows WSL UNC; use a native Windows checkout".into(),
            );
        }
        match path.canonicalize() {
            Ok(actual) if wsl(&actual) => {
                return Err(
                    "service wake unsupported on Windows WSL UNC; use a native Windows checkout"
                        .into(),
                );
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("service wake filesystem unknown: {e}")),
        }
    }
    Ok(())
}

#[tracing::instrument(name = "poke", skip_all)]
pub(crate) fn poke_locked(locked: &coreq::file_lock::Lock) -> Res<()> {
    use std::io::Write;
    coreq::telemetry::capture::operation("wake.write", || {
        let context = coreq::telemetry::capture::TraceContext::current().map(|c| c.encode()).unwrap_or_else(|| ".".into());
        let mut file = locked.file();
        file.set_len(0).map_err(|error| ("wake-truncate", error))?;
        file.write_all(context.as_bytes()).map_err(|error| ("wake-write", error))
    }, |(class, _)| *class).map_err(|(class, error)| format!("service wake failed ({class}: {error}); state may already be saved; check service status"))
}

#[tracing::instrument(name = "control.read", skip_all)]
pub(crate) fn wake_read<T>(
    path: &Path,
    seen: &mut Option<String>,
    read: impl FnOnce() -> Res<T>,
) -> Res<(Option<coreq::telemetry::capture::TraceContext>, T)> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            *seen = None;
            return read().map(|state| (None, state));
        }
        Err(_) => return Err("wakeup context unreadable".into()),
    };
    let locked = coreq::file_lock::try_lock_shared_file(file).map_err(|_| "wakeup context busy")?;
    let mut text = String::new();
    locked
        .file()
        .take(129)
        .read_to_string(&mut text)
        .map_err(|_| "wakeup context unreadable")?;
    let link = if seen.as_ref() == Some(&text) || matches!(text.as_str(), "" | ".") {
        None
    } else {
        Some(coreq::telemetry::capture::TraceContext::decode(&text)?)
    };
    let state = read()?;
    *seen = Some(text);
    Ok((link, state))
}

pub(crate) fn change_focus<T>(
    root: &Path,
    restrictive: bool,
    initialize: bool,
    change: impl FnOnce(&Path) -> Res<T>,
) -> Res<(T, String)> {
    let receipt = coreq::telemetry::capture::CommandReceipt::current();
    let received = receipt
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|r| telemetry::receipt(r, "received"));
    if !restrictive {
        if let Err(error) = &received {
            return Err(format!(
                "command not applied: receipt unavailable ({error}); check service status"
            ));
        }
    }
    // Retain the settings lock through terminal commit, including partial failures.
    let mut locked = None;
    let mut tracking = None;
    let mut tracking_error = None;
    let result = (|| {
        if initialize {
            fs::create_dir_all(dot(root)).map_err(|e| e.to_string())?;
        }
        fs::metadata(dot(root))
            .map_err(|e| format!("focus unchanged: state directory unavailable: {e}"))?;
        locked = Some(wake_lock(root)?);
        let lock = locked.as_ref().unwrap();
        let prepared = received
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|_| receipt.as_ref().map_err(Clone::clone))
            .and_then(|r| control::begin(root, r));
        match prepared {
            Ok(pending) => tracking = Some(pending),
            Err(error) if restrictive => tracking_error = Some(error),
            Err(error) => {
                return Err(format!(
                    "command not applied: service tracking unavailable ({error})"
                ));
            }
        }
        // A failed mutation must never leave its new state attributed to the preceding signal.
        lock.file()
            .set_len(0)
            .map_err(|e| format!("focus unchanged: cannot invalidate previous signal: {e}"))?;
        let result = change(&dot(root));
        let notified = poke_locked(lock);
        match (result, notified) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(format!(
                "{error}; focus may be partially saved; check service status"
            )),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(wake)) => Err(format!("{error}; {wake}")),
        }
    })();
    let terminal = receipt
        .and_then(|r| telemetry::receipt(&r, if result.is_ok() { "applied" } else { "failed" }));
    if terminal.is_ok() {
        if let Some(pending) = tracking {
            if let Err(error) = control::finish(root, pending, result.is_ok()) {
                tracking_error = Some(error);
            }
        }
    }
    let uncertain = received
        .err()
        .or_else(|| terminal.as_ref().err().cloned())
        .map(|e| format!("durable receipt unknown ({e})"))
        .into_iter()
        .chain(tracking_error.map(|e| format!("service tracking unknown ({e})")))
        .collect::<Vec<_>>()
        .join("; ");
    let uncertain = (!uncertain.is_empty()).then_some(uncertain);
    match (result, uncertain) {
        (Ok(value), None) => {
            let applied = terminal.unwrap();
            println!("Command applied locally; service observation pending; receipt {applied}");
            Ok((value, applied))
        }
        (Ok(_), Some(error)) => Err(format!(
            "command applied locally; {error}; check service status before retry"
        )),
        (Err(error), None) => Err(format!("{error}; failure receipt {}", terminal.unwrap())),
        (Err(error), Some(receipt)) => Err(format!(
            "{error}; {receipt}; check service status before retry"
        )),
    }
}

pub(crate) fn poke_times(paths: &[PathBuf; 2]) -> [Option<SystemTime>; 2] {
    paths
        .each_ref()
        .map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Compare with the pre-pass snapshot: a command arriving during work must not become the sleep baseline.
/// Returns true when a poke (any local command) ended the wait.
pub(crate) fn sleep_until_poke(
    paths: &[PathBuf; 2],
    start: [Option<SystemTime>; 2],
    until: Instant,
    tick: &mut dyn FnMut(),
) -> bool {
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if poke_times(paths) != start {
            return true;
        }
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1).min(left));
        tick();
    }
}

/// § 4 step 8 (#1819): the board is read at most this many seconds apart while a tray window is open.
pub(crate) const MENU_POLL: u64 = 15;

/// A tray window touches `<install>/menu-open` every tick while visible and focused.
pub(crate) fn menu_open(install: &Path) -> bool {
    coreq::status::age(&install.join("menu-open")).is_some_and(|a| a < 30)
}

/// The tray asks the next pass to reread `task` after its write (#1819), then wakes the service.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) fn ask_reread(root: &Path, task: u64) -> Res<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dot(root).join("reread"))
        .map_err(|e| format!("reread: {e}"))?;
    writeln!(f, "{task}").map_err(|e| format!("reread: {e}"))?;
    poke(root)
}

/// Tasks the tray wrote to (`<root>/.taskq/reread`, one number per line), taken once: renamed first so a concurrent append is kept for the next pass.
pub(crate) fn take_reread(dot: &Path) -> Vec<u64> {
    let taken = dot.join("reread.taken");
    if fs::rename(dot.join("reread"), &taken).is_err() {
        return vec![];
    }
    let tasks = fs::read_to_string(&taken)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    let _ = fs::remove_file(&taken);
    tasks
}

/// § 4 step 8 (#1431, #2352): an idle pass waits 3×`poll`; board-only changes may show up to 2×`poll` later.
pub(crate) fn poll_wait(
    poll: u64,
    complete: bool,
    children: bool,
    poked: bool,
    focus_off: bool,
    snapshot: Option<&snapshot::Snapshot>,
) -> u64 {
    let active = snapshot.is_none_or(|s| {
        s.reason.is_some()
            || !focus_off
                && s.tasks
                    .iter()
                    .any(|r| matches!(r.task.state, State::Ready | State::Doing | State::Review))
    });
    if !complete || children || poked || active {
        poll
    } else {
        poll * 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // #2495: a filesystem without byte-range locks looped on "check service status", which could never help.
    #[test]
    fn lock_refusal_names_a_filesystem_without_locks() {
        let unsupported = wake_refusal(
            "wake-lock",
            &std::io::Error::from(std::io::ErrorKind::Unsupported),
        );
        assert!(unsupported.starts_with("service wake unsupported"), "{unsupported}");
        assert!(!unsupported.contains("check service status"), "{unsupported}");
        #[cfg(windows)]
        {
            let invalid = wake_refusal("wake-lock", &std::io::Error::from_raw_os_error(1));
            assert!(invalid.starts_with("service wake unsupported"), "{invalid}");
            assert!(invalid.contains("wake-lock"), "{invalid}");
        }
        let held = wake_refusal(
            "wake-lock",
            &std::io::Error::from(std::io::ErrorKind::WouldBlock),
        );
        assert!(held.starts_with("service wake failed (wake-lock"), "{held}");
        let open = wake_refusal(
            "wake-open",
            &std::io::Error::from(std::io::ErrorKind::Unsupported),
        );
        assert!(open.starts_with("service wake failed (wake-open"), "{open}");
    }

    #[test]
    fn poke_wakes_and_a_live_holder_stays_refused() {
        let root = std::env::temp_dir().join(format!(
            "taskq-wake-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        poke(&root).unwrap();
        let path = dot(&root).join("poke");
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        let holder = wake_lock(&root).unwrap();
        let refused = poke(&root).unwrap_err();
        assert!(refused.contains("wake-lock"), "{refused}");
        assert!(!refused.contains("unsupported"), "{refused}");
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            before,
            "a refused poke writes nothing"
        );
        drop(holder);
        let paths = [path.clone(), path.clone()];
        let baseline = poke_times(&paths);
        std::thread::sleep(Duration::from_millis(20));
        poke(&root).unwrap();
        assert!(
            sleep_until_poke(&paths, baseline, Instant::now() + Duration::from_secs(2), &mut || {}),
            "the released lock lets the next poke wake the service"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
