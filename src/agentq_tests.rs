use super::*;
// #1391 A2: absence, stale/corrupt/denied observations and PID reuse cannot clear a warning as success.
#[test]
fn detection_requires_fresh_compatible_status_and_native_executable() {
    // #2011 A1: statuses derive from the fixture agentq publishes (its tests/fixtures/status.json), not a TaskQ-authored shape
    let published: Value =
        serde_json::from_str(include_str!("../tests/fixtures/agentq-status.json")).unwrap();
    let with = |version: &str, at: u64| {
        let mut s = published.clone();
        s["version"] = json!(version);
        s["at"] = json!(at);
        s
    };
    let release = |version: &str| {
        Path::new("/invented/agentq")
            .join("releases")
            .join(version)
            .join(if cfg!(windows) {
                "agentq.exe"
            } else {
                "agentq"
            })
    };
    let root = Path::new("/invented/agentq");
    let exe = release("0.9.0");
    let good = with("0.9.0", 100);
    let check = |launcher, status, process: Res<Option<PathBuf>>| {
        inspect(root, launcher, status, 100, |_| process)["kind"].clone()
    };
    assert_eq!(check(Ok(false), Ok(None), Ok(None)), "shown");
    assert_eq!(check(Ok(true), Ok(None), Ok(None)), "unknown");
    assert_eq!(check(Err("denied".into()), Ok(None), Ok(None)), "unknown");
    assert_eq!(check(Ok(false), Err("corrupt".into()), Ok(None)), "unknown");
    assert_eq!(
        check(
            Ok(true),
            Ok(Some((good.clone(), 90))),
            Ok(Some(exe.clone()))
        ),
        "running"
    );
    assert_eq!(
        check(
            Ok(true),
            Ok(Some((good.clone(), 91))),
            Ok(Some(exe.clone()))
        ),
        "unknown"
    );
    assert_eq!(
        check(Ok(true), Ok(Some((good.clone(), 91))), Ok(None)),
        "stopped",
        "#1702 A2: a disabled service's stale status reads as off"
    );
    // #1702 A1: installed AgentQ 0.10.0 read as 'unsupported agentq status version or shape'
    assert_eq!(
        check(
            Ok(true),
            Ok(Some((with("0.10.0", 100), 0))),
            Ok(Some(release("0.10.0")))
        ),
        "running"
    );
    // #2011 A2: agentq 1.0 read as unsupported; the published fixture itself is a 1.x status
    let at = published["at"].as_u64().unwrap();
    let v = published["version"].as_str().unwrap();
    assert_eq!(
        inspect(
            root,
            Ok(true),
            Ok(Some((published.clone(), 0))),
            at,
            |_| Ok(Some(release(v)))
        )["kind"],
        "running"
    );
    assert_eq!(
        inspect(
            root,
            Ok(true),
            Ok(Some((with("0.4.0", 100), 0))),
            100,
            |_| Ok(None)
        )["reason"],
        "unsupported agentq status version \"0.4.0\""
    );
    assert_eq!(
        check(Ok(true), Ok(Some((good.clone(), 0))), Ok(None)),
        "stopped"
    );
    assert_eq!(
        check(
            Ok(true),
            Ok(Some((good.clone(), 0))),
            Ok(Some("/other/agentq".into()))
        ),
        "unknown"
    );
    assert_eq!(
        check(Ok(true), Ok(Some((good.clone(), 0))), Err("denied".into())),
        "unknown"
    );
    assert_eq!(
        check(
            Ok(false),
            Ok(Some((good.clone(), 0))),
            Ok(Some(exe.clone()))
        ),
        "unknown"
    );
    let mut cached = inspect(root, Ok(true), Ok(Some((good.clone(), 90))), 100, |_| {
        Ok(Some(exe.clone()))
    });
    cached["checked"] = json!(100);
    assert_eq!(fresh(cached.clone(), 100)["kind"], "running");
    assert_eq!(
        fresh(cached, 101)["kind"],
        "unknown",
        "cache cannot extend the source deadline"
    );
    let cached = json!({"kind":"shown","warning":true,"checked":100});
    assert_eq!(fresh(cached.clone(), 105)["kind"], "shown");
    let stale = fresh(cached, 106);
    assert_eq!(stale["kind"], "unknown");
    assert_eq!(
        stale["warning"], true,
        "slow runtime discovery keeps the previous warning, never fresh health"
    );
    for bad in [
        json!({}),
        with("2.0.0", 100),
        with("1.0", 100),
        with("0.9.0", 101),
    ] {
        assert_eq!(
            check(Ok(true), Ok(Some((bad, 0))), Ok(Some(exe.clone()))),
            "unknown"
        );
    }
}
#[cfg(any(target_os = "macos", windows))]
#[test]
fn native_process_image_matches_creation_identity() {
    assert_eq!(
        process(std::process::id()).unwrap().unwrap(),
        std::env::current_exe().unwrap()
    );
}
#[test]
fn bounded_files_keep_malformed_and_future_observations_unknown() {
    let dir = std::env::temp_dir().join(format!("taskq-agentq-status-{}", crate::runtime::uuid()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("status.json");
    assert!(status(&path).unwrap().is_none());
    fs::write(&path, b"{bad").unwrap();
    assert!(status(&path).is_err());
    fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(status(&path).unwrap_err().contains("1 MiB"));
    fs::remove_dir_all(dir).unwrap();
}

// #1391 A4: preferences survive restart; only verified running resets an episode, setup records never replay.
#[test]
fn dismissal_and_setup_recovery_preserve_unknown_effects() {
    let dir = std::env::temp_dir().join(format!("taskq-agentq-{}", crate::runtime::uuid()));
    fs::create_dir(&dir).unwrap();
    let shown = project(&dir, json!({"kind":"shown"}), false);
    assert_eq!(shown["warning"], true);
    action(&dir, "agentq-dismiss", "", "").unwrap();
    assert_eq!(
        project(&dir, json!({"kind":"shown"}), false)["warning"],
        false
    );
    let u = project(&dir, unknown("read denied"), false);
    assert_eq!(u["kind"], "unknown");
    assert_eq!(u["dismissed"], true);
    let mut p = preferences(&dir).unwrap();
    p.request = Some("missing-request".into());
    save(&dir, &p).unwrap();
    assert_eq!(
        project(&dir, json!({"kind":"shown"}), false)["kind"],
        "interrupted"
    );
    assert!(crate::setup::all(&dir).unwrap().0.is_empty());
    assert_eq!(
        project(&dir, json!({"kind":"running"}), false)["warning"],
        false
    );
    assert!(!preferences(&dir).unwrap().dismissed);
    assert_eq!(
        project(&dir, json!({"kind":"shown"}), false)["warning"],
        true
    );
    fs::write(dir.join(FILE), "{bad").unwrap();
    let unreadable = project(&dir, json!({"kind":"running"}), true);
    assert_eq!(unreadable["kind"], "unknown");
    assert_eq!(unreadable["warning"], true);
    assert_eq!(
        project(&dir, json!({"kind":"running"}), false)["warning"],
        false,
        "unknown preferences alone must not invent an absence"
    );
    assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), "{bad");
    fs::remove_dir_all(dir).unwrap();
}
