use super::*;

// Chain: #2522 A1/A3, R6/R12. Risk: an executable removed during a live attempt stays healthy,
// or a replacement hides the old execution's missing dependency. Observation must not submit input.
#[test]
fn runtime_2522_deleted_executable_keeps_its_path() {
    let root = std::env::temp_dir().join(format!("taskq-2522-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let old = root.join("old-runtime");
    let new = root.join("new-runtime");
    std::fs::write(&old, "invented executable fixture").unwrap();
    let mut monitor = Monitor::default();
    let kids = vec![
        (5, 50, "codex".into(), None, false),
        (6, 60, "codex".into(), None, false),
    ];
    for (n, x, _, _, _) in &kids {
        monitor.paths.insert((*n, *x), old.clone());
    }
    monitor.observe_with(&kids, |_, _| panic!("CLI child has no daemon"));
    assert!(monitor.faults.is_empty());
    std::fs::remove_file(&old).unwrap();
    std::fs::write(&new, "invented replacement fixture").unwrap();
    monitor.observe_with(&kids, |_, _| {
        panic!("missing executable must not be invoked")
    });
    assert_eq!(
        monitor.faults,
        vec![json!({"runtime":"codex","path":old,"reason":"executable missing","tasks":[5,6]})]
    );
    monitor.observe_with(&[], |_, _| panic!());
    assert!(monitor.faults.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: #2522 A1/A3, R6/R12. Risk: stale working jobs survive daemon death, a probe runs for every
// worker instead of each account, an unreadable daemon reads healthy, or another account masks the outage.
#[test]
fn runtime_2522_dead_daemon_is_not_a_running_worker() {
    let root = std::env::temp_dir().join(format!("taskq-2522-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let bin = root.join("runtime");
    std::fs::write(&bin, "invented executable fixture").unwrap();
    let mut monitor = Monitor::default();
    let kids = vec![
        (5, 50, "claude".into(), None, true),
        (6, 60, "claude".into(), None, true),
        (
            7,
            70,
            "claude".into(),
            Some(root.join("other-account")),
            true,
        ),
    ];
    for (n, x, _, _, _) in &kids {
        monitor.paths.insert((*n, *x), bin.clone());
    }
    let mut kids = kids;
    kids.push((8, 80, "claude".into(), None, false));
    monitor.paths.insert((8, 80), bin.clone());
    let mut calls = 0;
    monitor.observe_with(&kids, |_, home| {
        calls += 1;
        Ok(home.is_some().then_some(77))
    });
    assert_eq!(calls, 2);
    assert_eq!(
        monitor.faults,
        vec![json!({"runtime":"claude","path":bin,"reason":"daemon not running","tasks":[5,6]})]
    );
    monitor.observe_with(&kids, |_, _| Err("status unreadable".into()));
    assert_eq!(monitor.reason(7), Some("daemon unknown: status unreadable"));
    monitor.observe_with(&kids, |_, _| Ok(Some(77)));
    assert!(monitor.faults.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: #2522 A2/A3, §8: real CLI boundary, isolated PATH -> fresh resolution and read-only daemon probe.
// Risk: a vanished binary is cached until service restart, or a health probe starts/resumes a worker.
#[cfg(any(unix, windows))]
#[test]
fn runtime_2522_native_probe_and_fresh_resolution() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("taskq-2522-native-{}", uuid()));
    for dir in ["old", "current"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    for dir in ["old", "current"] {
        let bin = root.join(dir).join(if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        });
        #[cfg(unix)]
        std::fs::write(&bin, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKQ_HEALTH_CALLS\"\n[ \"$1 $2\" = 'daemon status' ] || exit 91\nprintf 'not running\\n'\nexit 1\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        #[cfg(windows)]
        std::fs::write(&bin, "@echo off\r\necho %*>> \"%TASKQ_HEALTH_CALLS%\"\r\nif not \"%*\"==\"daemon status\" exit /b 91\r\necho not running\r\nexit /b 1\r\n").unwrap();
    }
    let mut paths = vec![root.join("old"), root.join("current")];
    #[cfg(windows)]
    paths.extend(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|dir| {
            // Keep the test executable's DLL directories without exposing an installed Claude fallback.
            !["claude.exe", "claude.cmd", "claude.bat", "claude.com"]
                .iter()
                .any(|name| dir.join(name).exists())
        }),
    );
    let out = coreq::process::output(
        "qualification.runtime-dependency",
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime::health::tests::runtime_2522_native_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_HEALTH_ROOT", &root)
            .env("TASKQ_HEALTH_CALLS", root.join("calls"))
            .env("PATH", std::env::join_paths(paths).unwrap()),
        std::time::Duration::from_secs(30),
    );
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("calls"))
            .unwrap()
            .replace("\r\n", "\n"),
        "daemon status\n",
        "no worker input was submitted"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(unix, windows))]
#[test]
#[ignore = "isolated runtime dependency fixture"]
fn runtime_2522_native_child() {
    let root = PathBuf::from(std::env::var_os("TASKQ_HEALTH_ROOT").unwrap());
    let bin = if cfg!(windows) {
        "claude.cmd"
    } else {
        "claude"
    };
    let old = root.join("old").join(bin);
    let current = root.join("current").join(bin);
    assert_eq!(binary("claude").unwrap(), old);
    assert_eq!(read_daemon("claude", &old, None), Ok(None));
    std::fs::remove_file(old).unwrap();
    assert_eq!(
        binary("claude").unwrap(),
        current,
        "next launch uses the current executable"
    );
    std::fs::remove_file(current).unwrap();
    assert!(
        binary("claude")
            .unwrap_err()
            .contains("claude executable missing: claude")
    );
}
