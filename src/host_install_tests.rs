use super::*;
// Chain: §8 Project attachment (A1/A4, H-A5): missing GitLab credentials cannot hold the healthy host.
#[test]
fn unauthenticated_gitlab_attachment_preserves_running_host() {
    let (install, root, paths) = fixture();
    snapshot(&install, vec![root.clone()], paths).unwrap();
    fs::write(
        root.join("taskq.json"),
        json!({"board":"gitlab","repo":"invented/project","host":"gitlab.invalid"}).to_string(),
    )
    .unwrap();
    let existing = [
        install.parent().unwrap().join("existing-a"),
        install.parent().unwrap().join("existing-b"),
    ];
    for root in &existing {
        fs::create_dir_all(crate::paths::dot(root)).unwrap();
        fs::write(root.join("taskq.json"), "{}").unwrap();
        fs::write(crate::paths::dot(root).join("focus"), "off").unwrap();
    }
    // Existing GitHub loops use in-memory boards; their worker environment gets only this invented value.
    fs::write(install.join("token"), "invented-fixture-not-a-credential").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(install.join("token"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    let original = existing
        .iter()
        .map(|root| format!("{}\n", root.display()))
        .collect::<String>();
    fs::write(install.join("projects"), &original).unwrap();
    let mut live = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "host::installation::tests::attachment_live_host_actor",
            "--ignored",
            "--nocapture",
        ])
        .env("TASKQ_ATTACHMENT_CASE", &install)
        .env("TASKQ_INSTALL", &install)
        .env_remove("TASKQ_ATTEMPT")
        .env_remove("TASKQ_ROOT")
        .env_remove("COREQ_TRACE_CONTEXT")
        .stdout(fs::File::create(install.join("fixture-host.log")).unwrap())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "host::installation::tests::unauthenticated_attachment_actor",
        "--ignored",
        "--nocapture",
    ])
    .env("TASKQ_ATTACHMENT_CASE", &install)
    .env("TASKQ_ATTACHMENT_ROOT", &root)
    .env("TASKQ_INSTALL", &install)
    .env_remove("TASKQ_ATTEMPT")
    .env_remove("TASKQ_ROOT")
    .env_remove("GITLAB_TOKEN")
    .env_remove("GLAB_TOKEN")
    .env_remove("TASKQ_TOKEN")
    .env_remove("COREQ_TRACE_CONTEXT");
    // Exclude CLI login fallback while retaining Cargo's runtime DLL directories.
    let path = std::env::join_paths(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|dir| {
            !dir.join(if cfg!(windows) { "glab.exe" } else { "glab" })
                .is_file()
        }),
    )
    .unwrap();
    cmd.env("PATH", path);
    let result = (|| -> Res<()> {
        wait_attachment(&mut live, || {
            existing.iter().all(|root| {
                crate::data::json(&crate::paths::dot(root).join("status.json"))
                    .ok()
                    .flatten()
                    .is_some_and(|s| s["host_instance"] == "invented-auth-host")
            })
        })?;
        coreq::process::output(
            "test.attachment-auth",
            &mut cmd,
            std::time::Duration::from_secs(60),
        )
        .map_err(|e| format!("credential preflight: {e:?}"))?;
        cmd.env("TASKQ_ATTACHMENT_WAKE_CASE", "1");
        coreq::process::output(
            "test.attachment-wake",
            &mut cmd,
            std::time::Duration::from_secs(60),
        )
        .map_err(|e| format!("wake preflight: {e:?}"))?;
        coreq::write_durable(&install.join("verify-continuity"), "")?;
        wait_attachment(&mut live, || install.join("continuity-ok").exists())
    })();
    fs::write(install.join("exit-fixture"), "").unwrap();
    let stopped = wait_attachment(&mut live, || install.join("fixture-joined").exists());
    if stopped.is_err() && live.try_wait().unwrap().is_none() {
        live.kill().unwrap();
    }
    let exit = live.wait().unwrap();
    assert!(
        result.is_ok() && stopped.is_ok() && exit.success(),
        "attachment: {result:?}; drain: {stopped:?}; host: {exit}\n{}",
        fs::read_to_string(install.join("fixture-host.log")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(install.join("projects")).unwrap(),
        original
    );
    assert!(!install.join(INSTALL_PENDING).exists() && !install.join(UPGRADE_INTENT).exists());
    assert!(!crate::paths::dot(&root).join("focus").exists());
    assert_eq!(service_status(&install)["instance"], "invented-auth-host");
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}
fn wait_attachment(child: &mut std::process::Child, ready: impl Fn() -> bool) -> Res<()> {
    let until = Instant::now() + std::time::Duration::from_secs(60);
    while !ready() {
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("attachment fixture exited: {exit}"));
        }
        if Instant::now() >= until {
            return Err("attachment fixture completion signal expired".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}
// A4 process boundary: real multi-project loops, in-memory boards; no native manager or agent launches.
#[test]
#[ignore = "owned live attachment fixture host"]
fn attachment_live_host_actor() {
    let install = PathBuf::from(std::env::var_os("TASKQ_ATTACHMENT_CASE").unwrap());
    assert!(
        install
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("taskq-install-originals-")
    );
    let _owner = owner(&install).unwrap().unwrap();
    let native = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let policy = Policy::parse(
        json!({"limits":{"codex":4},"load_gate":{"enabled":false}}),
        "invented",
    )
    .unwrap();
    let mut h = Host::new(Ok(policy.clone()));
    h.instance = "invented-auth-host".into();
    let shared = Arc::new(Mutex::new(h));
    let roots = projects(&install).unwrap();
    let mut threads = vec![];
    for root in &roots {
        shared.lock().unwrap().attach(root);
        let (root, install, host, policy) = (
            root.clone(),
            install.clone(),
            shared.clone(),
            policy.clone(),
        );
        threads.push(std::thread::spawn(move || {
            enter(&root, &install);
            let mut cfg = crate::tests::cfg();
            cfg.root = root;
            cfg.install = install;
            cfg.poll = 1;
            policy.apply(&mut cfg);
            crate::service_run::service_run(
                &crate::config::Ctx {
                    cfg,
                    raw: json!({}),
                    board: Box::new(crate::board::Fake::default()),
                },
                Some(host),
            )
        }));
    }
    let until = Instant::now() + std::time::Duration::from_secs(120);
    loop {
        publish_service(&install, &native, "invented-auth-host").unwrap();
        let mut h = shared.lock().unwrap();
        h.held = admission_reason(&install);
        if let Some(index) = threads.iter().position(|t| t.is_finished()) {
            panic!(
                "existing project loop stopped: {:?}",
                threads.swap_remove(index).join()
            );
        }
        if install.join("verify-continuity").exists()
            && !install.join("continuity-ok").exists()
            && h.publications_ready(&roots)
        {
            h.reserve(&roots[0], 99, 100, "codex", &policy.cap, true, 0)
                .unwrap();
            h.sync(&roots[0], vec![]);
            coreq::write_durable(&install.join("continuity-ok"), "").unwrap();
        }
        if install.join("exit-fixture").exists() {
            h.draining.extend(roots.iter().cloned());
            drop(h);
            for root in &roots {
                crate::wake::poke(root).unwrap();
            }
            break;
        }
        assert!(Instant::now() < until, "live attachment fixture expired");
        drop(h);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for thread in threads {
        while !thread.is_finished() {
            assert!(Instant::now() < until, "project fixture drain expired");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(thread.join().unwrap().unwrap_err(), "project drained");
    }
    coreq::write_durable(&install.join("fixture-joined"), "").unwrap();
}
#[test]
#[ignore = "owned credential-less attachment process"]
fn unauthenticated_attachment_actor() {
    let install = PathBuf::from(std::env::var_os("TASKQ_ATTACHMENT_CASE").unwrap());
    assert!(
        install
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("taskq-install-originals-")
    );
    let root = PathBuf::from(std::env::var_os("TASKQ_ATTACHMENT_ROOT").unwrap());
    let wake = std::env::var_os("TASKQ_ATTACHMENT_WAKE_CASE")
        .map(|_| crate::wake::wake_lock(&root).unwrap());
    for _ in 0..2 {
        let error = install_project(&install, &root).unwrap_err();
        if wake.is_some() {
            assert!(
                error.contains("wake-lock"),
                "wake eligibility must precede host transition: {error}"
            );
        } else {
            assert!(
                error.contains("no GitLab token"),
                "missing credential must precede host transition: {error}"
            );
        }
        assert!(admission_reason(&install).is_none());
    }
}

// Chain: §8 Project attachment (A2/A3, H-A5): registered failure -> cancel -> joined -> restored -> failed -> replay.
// Retain occupied children of other roots, original registry bytes, and every original command receipt.
#[test]
fn failed_attachment_cancellation_replays_only_after_owned_empty_drain() {
    for initial in ["registered", "owned"] {
        let committed = initial == "registered";
        let (install, root, paths) = fixture();
        let baseline = snapshot(&install, vec![root.clone()], paths).unwrap();
        let original_bytes = fs::read(manifest(&install)).unwrap();
        let existing = install.parent().unwrap().join("existing");
        fs::create_dir(&existing).unwrap();
        let existing = existing.canonicalize().unwrap();
        let original = format!("{}\r\n\n", existing.display());
        let next = format!(
            "{original}{}\n",
            crate::platform::plain_path(&root).display()
        );
        let native = coreq::process::identity(std::process::id())
            .unwrap()
            .unwrap();
        let _owner = owner(&install).unwrap().unwrap();
        publish_service(&install, &native, "invented-cancel-host").unwrap();
        let (capture, guard) = crate::profile::tests::collecting(&install);
        let span = tracing::info_span!("host.install");
        let entered = span.enter();
        let p = Pending {
            schema: 1,
            id: "invented-failed-attachment".into(),
            baseline,
            root: root.clone(),
            version: crate::platform::VERSION.into(),
            candidate: "a".repeat(64),
            original: Some(original.clone()),
            next: next.clone(),
            phase: initial.into(),
            restore: false,
            attach: true,
            owner: Some(InstallOwner {
                pid: native.pid,
                started: native.started.clone(),
                instance: "invented-cancel-host".into(),
            }),
            activation: None,
            command: CommandReceipt::current().unwrap().encode(),
        };
        let registry = if committed { &next } else { &original };
        save(&install, &p).unwrap();
        fs::write(install.join("projects"), registry).unwrap();
        record(&install, &p, "received").unwrap();
        if committed {
            record(&install, &p, "applied").unwrap();
        }
        let applied = retained(&install, &p).unwrap();
        fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        startup_status(&install, &root, "no GitLab token", "invented-cancel-host").unwrap();
        let policy = Policy::parse(
            json!({"limits":{"codex":4},"load_gate":{"enabled":false}}),
            "invented",
        )
        .unwrap();
        let mut h = Host::new(Ok(policy.clone()));
        h.instance = "invented-cancel-host".into();
        h.sync(&existing, vec![(7, 8, "codex".into())]);
        h.busy.insert(existing.clone(), true);
        h.rebuilt.insert(existing.clone());
        h.attach(&root);
        h.held = admission_reason(&install);
        assert!(
            h.reserve(&existing, 9, 10, "codex", &policy.cap, true, 0)
                .is_err()
        );
        assert!(
            cancel_install(&install, &root, "wrong-command")
                .unwrap_err()
                .contains("differs")
        );
        assert_eq!(
            fs::read_to_string(install.join("projects")).unwrap(),
            *registry
        );
        assert_eq!(
            installation_status(&install, Some(&p.id))["state"],
            if committed { "applied" } else { "queued" }
        );
        fs::write(
            install.join("projects"),
            if committed { &original } else { &next },
        )
        .unwrap();
        assert!(
            cancel_install(&install, &root, &p.id)
                .unwrap_err()
                .contains("registry changed")
        );
        fs::write(install.join("projects"), registry).unwrap();
        // Corrupt native recovery, a prior generation, and legacy execution all refuse before cursor mutation.
        let kids = crate::paths::kids_path(&install, &root).unwrap();
        fs::create_dir_all(kids.parent().unwrap()).unwrap();
        fs::write(&kids, "unknown native execution").unwrap();
        assert!(cancel_install(&install, &root, &p.id).is_err());
        fs::remove_file(&kids).unwrap();
        publish_service(&install, &native, "another-host-generation").unwrap();
        assert!(
            cancel_install(&install, &root, &p.id)
                .unwrap_err()
                .contains("ownership uncertain")
        );
        publish_service(&install, &native, "invented-cancel-host").unwrap();
        fs::write(
            crate::paths::dot(&root).join("kids"),
            "unknown legacy execution",
        )
        .unwrap();
        assert!(
            cancel_install(&install, &root, &p.id)
                .unwrap_err()
                .contains("legacy")
        );
        fs::remove_file(crate::paths::dot(&root).join("kids")).unwrap();
        assert_eq!(pending(&install).unwrap().phase, initial);
        for _ in 0..2 {
            assert!(
                cancel_install(&install, &root, &p.id)
                    .unwrap()
                    .contains("cancellation requested")
            );
            assert_eq!(pending(&install).unwrap().command, p.command);
            assert_eq!(projects(&install).unwrap(), vec![existing.clone()]);
            assert_eq!(
                install_acknowledgement(&install, &root, "invented-cancel-host"),
                None
            );
            assert!(!acknowledge(&install, &p).unwrap());
            assert!(
                finish_cancellation(&install, &mut h, true)
                    .unwrap_err()
                    .contains("drain/execution"),
                "cached membership/thread still fences retirement"
            );
            assert_eq!(
                h.occupied(),
                1,
                "existing child remains owned and supervised"
            );
        }
        h.detached(&root);
        assert!(
            !h.rebuilding,
            "the removed startup failure cannot strand original occupancy recovery"
        );
        assert!(
            finish_cancellation(&install, &mut h, true).is_err(),
            "cached membership/thread still fences retirement"
        );
        h.sync(&root, vec![(11, 12, "codex".into())]);
        assert!(
            finish_cancellation(&install, &mut h, false).is_err(),
            "unknown occupied execution refuses removal"
        );
        h.sync(&root, vec![]);
        // Crash after restoring registry: same cursor and command must finish the remaining audit writes.
        if committed {
            replace_owned(&install.join("projects"), &next, &original).unwrap();
        }
        assert!(admission_reason(&install).is_some());
        finish_cancellation(&install, &mut h, false).unwrap();
        assert_eq!(
            fs::read_to_string(install.join("projects")).unwrap(),
            original
        );
        assert_eq!(fs::read(manifest(&install)).unwrap(), original_bytes);
        let failed = installation_status(&install, Some(&p.id));
        assert_eq!(failed["state"], "failed");
        assert!(failed.get("observed_receipt").is_none());
        assert_eq!(failed["received_receipt"], applied["received_receipt"]);
        assert_eq!(failed["applied_receipt"], applied["applied_receipt"]);
        if !committed {
            assert!(
                failed.get("applied_receipt").is_none(),
                "uncommitted cancellation never fabricates application"
            );
        }
        assert_eq!(failed["cancelled_attachment"]["original"], original);
        assert_eq!(failed["cancelled_attachment"]["command"], p.command);
        // Crash after the terminal receipt but before cursor retirement still needs host reconciliation.
        let cancelled: Pending =
            serde_json::from_value(failed["cancelled_attachment"].clone()).unwrap();
        save(&install, &cancelled).unwrap();
        assert!(
            cancel_install(&install, &root, &p.id)
                .unwrap()
                .contains("cancellation requested")
        );
        assert!(admission_reason(&install).is_some());
        finish_cancellation(&install, &mut h, false).unwrap();
        for _ in 0..2 {
            assert!(
                cancel_install(&install, &root, &p.id)
                    .unwrap()
                    .contains("cancelled")
            );
        }
        assert_eq!(
            crate::resources::with(&install, |state| Ok(state.ops.len())).unwrap(),
            1
        );
        h.reload_admission(
            Ok(policy.clone()),
            admission_reason(&install),
            std::slice::from_ref(&existing),
        );
        assert!(
            h.reserve(&existing, 9, 10, "codex", &policy.cap, true, 0)
                .is_ok(),
            "unrelated project can launch after cancellation"
        );
        assert_eq!(h.occupied(), 2, "the prior child keeps its slot");
        drop(entered);
        drop(span);
        capture.flush().unwrap();
        drop(guard);
        drop(capture);
        drop(_owner);
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
}

// Chain: §8 Project attachment (A3/R12): worker_env failure -> host join -> exact cancellation -> unrelated grant.
// Risk: a joined pre-execution failure strands busy/stopped; cancellation must still fence uncertain execution.
#[test]
fn joined_startup_failure_cancellation_preserves_existing_execution() {
    for cancel_before_join in [false, true] {
        let (install, root, paths) = fixture();
        let baseline = snapshot(&install, vec![root.clone()], paths).unwrap();
        let originals = fs::read(manifest(&install)).unwrap();
        let existing = install.parent().unwrap().join("existing");
        fs::create_dir(&existing).unwrap();
        let existing = existing.canonicalize().unwrap();
        let original = format!("{}\r\n\n", existing.display());
        let next = format!(
            "{original}{}\n",
            crate::platform::plain_path(&root).display()
        );
        let native = coreq::process::identity(std::process::id())
            .unwrap()
            .unwrap();
        let _owner = owner(&install).unwrap().unwrap();
        publish_service(&install, &native, "invented-joined-host").unwrap();
        let (capture, guard) = crate::profile::tests::collecting(&install);
        let span = tracing::info_span!("host.install");
        let entered = span.enter();
        let p = Pending {
            schema: 1,
            id: "invented-joined-attachment".into(),
            baseline,
            root: root.clone(),
            version: crate::platform::VERSION.into(),
            candidate: "a".repeat(64),
            original: Some(original.clone()),
            next: next.clone(),
            phase: "registered".into(),
            restore: false,
            attach: true,
            owner: Some(InstallOwner {
                pid: native.pid,
                started: native.started.clone(),
                instance: "invented-joined-host".into(),
            }),
            activation: None,
            command: CommandReceipt::current().unwrap().encode(),
        };
        save(&install, &p).unwrap();
        fs::write(install.join("projects"), &next).unwrap();
        record(&install, &p, "received").unwrap();
        record(&install, &p, "applied").unwrap();
        let applied = retained(&install, &p).unwrap();
        let policy = Policy::parse(
            json!({"limits":{"codex":4},"load_gate":{"enabled":false}}),
            "invented",
        )
        .unwrap();
        let mut h = Host::new(Ok(policy.clone()));
        h.instance = "invented-joined-host".into();
        h.sync(&existing, vec![(7, 8, "codex".into())]);
        h.busy.insert(existing.clone(), true);
        h.rebuilt.insert(existing.clone());
        h.attach(&root);
        h.held = admission_reason(&install);
        let shared = Arc::new(Mutex::new(h));
        let (project, store, host) = (root.clone(), install.clone(), shared.clone());
        let thread = std::thread::spawn(move || {
            enter(&project, &store);
            let mut cfg = crate::tests::cfg();
            cfg.root = project;
            cfg.install = store;
            cfg.board = &crate::board::github::KIND;
            let result = crate::service_run::service_run(
                &crate::config::Ctx {
                    cfg,
                    raw: json!({}),
                    board: Box::new(crate::board::Fake::default()),
                },
                Some(host),
            );
            assert_eq!(
                result.as_ref().unwrap_err(),
                crate::board::github::TOKEN_ERROR
            );
            result
        });
        let until = Instant::now() + std::time::Duration::from_secs(30);
        while !thread.is_finished() {
            assert!(Instant::now() < until, "startup failure did not finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let mut threads = BTreeMap::from([(root.clone(), thread)]);
        let mut stopped = BTreeSet::new();
        let mut drained = BTreeSet::new();
        if !cancel_before_join {
            reconcile_projects(
                &install,
                &shared,
                &mut threads,
                &[existing.clone(), root.clone()],
                &mut stopped,
                &mut drained,
            )
            .unwrap();
            assert!(stopped.contains(&root));
            assert!(shared.lock().unwrap().busy.contains_key(&root));
        }
        assert!(
            cancel_install(&install, &root, &p.id)
                .unwrap()
                .contains("cancellation requested")
        );
        let roots = projects(&install).unwrap();
        assert_eq!(roots, vec![existing.clone()]);
        // Recheck execution and native ownership after the cancellation request, before retiring cached state.
        shared
            .lock()
            .unwrap()
            .sync(&root, vec![(11, 12, "codex".into())]);
        assert!(
            reconcile_projects(
                &install,
                &shared,
                &mut threads,
                &roots,
                &mut stopped,
                &mut drained
            )
            .is_err()
        );
        shared.lock().unwrap().sync(&root, vec![]);
        let kids = crate::paths::kids_path(&install, &root).unwrap();
        fs::create_dir_all(kids.parent().unwrap()).unwrap();
        fs::write(&kids, "unknown execution").unwrap();
        assert!(
            reconcile_projects(
                &install,
                &shared,
                &mut threads,
                &roots,
                &mut stopped,
                &mut drained
            )
            .is_err()
        );
        fs::remove_file(&kids).unwrap();
        publish_service(&install, &native, "another-host-generation").unwrap();
        assert!(
            reconcile_projects(
                &install,
                &shared,
                &mut threads,
                &roots,
                &mut stopped,
                &mut drained
            )
            .is_err()
        );
        assert!(shared.lock().unwrap().busy.contains_key(&root));
        assert!(stopped.contains(&root));
        assert_eq!(fs::read_to_string(install.join("projects")).unwrap(), next);
        assert_eq!(
            installation_status(&install, Some(&p.id))["state"],
            "applied"
        );
        publish_service(&install, &native, "invented-joined-host").unwrap();
        reconcile_projects(
            &install,
            &shared,
            &mut threads,
            &roots,
            &mut stopped,
            &mut drained,
        )
        .unwrap();
        assert!(threads.is_empty());
        assert!(
            !stopped.contains(&root),
            "a later attachment may retry this root"
        );
        let mut h = shared.lock().unwrap();
        assert!(!h.busy.contains_key(&root));
        assert!(!h.rebuilding);
        assert_eq!(
            h.slots.get(&slot(&existing, 7, 8)).map(String::as_str),
            Some("codex")
        );
        h.reload_admission(Ok(policy.clone()), admission_reason(&install), &roots);
        h.reserve(&existing, 9, 10, "codex", &policy.cap, true, 0)
            .unwrap();
        assert_eq!(h.occupied(), 2);
        assert_eq!(
            fs::read_to_string(install.join("projects")).unwrap(),
            original
        );
        assert_eq!(fs::read(manifest(&install)).unwrap(), originals);
        let failed = installation_status(&install, Some(&p.id));
        assert_eq!(failed["state"], "failed");
        assert!(failed.get("observed_receipt").is_none());
        for phase in ["received_receipt", "applied_receipt"] {
            assert_eq!(failed[phase], applied[phase]);
        }
        assert_eq!(failed["cancelled_attachment"]["command"], p.command);
        assert!(
            cancel_install(&install, &root, &p.id)
                .unwrap()
                .contains("cancelled")
        );
        assert_eq!(
            crate::resources::with(&install, |state| Ok(state.ops.len())).unwrap(),
            1
        );
        drop(h);
        drop(entered);
        drop(span);
        capture.flush().unwrap();
        drop(guard);
        drop(capture);
        drop(_owner);
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
}

// Chain: §8 Project attachment: reject known WSL UNC paths before any filesystem or native lock probe.
#[cfg(windows)]
#[test]
fn wsl_unc_attachment_refuses_before_native_wake() {
    let install =
        std::env::temp_dir().join(format!("taskq-unc-preflight-{}", crate::runtime::uuid()));
    for path in [
        r"\\wsl.localhost\InventedDistro\project",
        r"\\WSL$\InventedDistro\project",
        r"\\?\UNC\wsl.localhost\InventedDistro\project",
        r"\\?\UNC\WSL$\InventedDistro\project",
    ] {
        let error = install_project(&install, Path::new(path)).unwrap_err();
        assert!(error.contains("unsupported on Windows WSL UNC"), "{error}");
        assert!(
            !install.exists(),
            "no installation hold or receipt can precede path eligibility"
        );
    }
}
pub(super) fn interrupt(install: &Path, phase: &str) -> Res<()> {
    if std::env::var("TASKQ_INSTALL_INTERRUPT").ok().as_deref() != Some(phase) {
        return Ok(());
    }
    if std::env::var_os("TASKQ_INSTALL_PROCESS_CASE").as_deref() != Some(install.as_os_str())
        || !install
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name.to_string_lossy().starts_with("taskq-install-process-"))
    {
        return Err("owned installation process fixture required".into());
    }
    coreq::write_durable(&install.join("interrupted-at"), phase)?;
    let until = Instant::now() + std::time::Duration::from_secs(90);
    while !install.join("release-interruption").exists() {
        if Instant::now() >= until {
            return Err("installation fixture interruption expired".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}

pub(super) fn activation_result(install: &Path) -> Res<()> {
    interrupt(install, "activation")?;
    if std::env::var("TASKQ_INSTALL_FAIL_ACTIVATION").as_deref() == Ok("1") {
        if std::env::var_os("TASKQ_INSTALL_PROCESS_CASE").as_deref() != Some(install.as_os_str()) {
            return Err("owned activation fixture required".into());
        }
        return Err("owned fixture activation failure after replacement".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn process_case(case: &str) {
    let report = std::env::temp_dir().join(format!(
        "taskq-install-report-{}.json",
        crate::runtime::uuid()
    ));
    let result = coreq::process::output(
        "test.install-restore",
        std::process::Command::new("python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/install-process-macos.py"),
            )
            .arg("--binary")
            .arg(std::env::current_exe().unwrap())
            .arg("--output")
            .arg(&report)
            .arg(case),
        std::time::Duration::from_secs(150),
    );
    let evidence = fs::read_to_string(&report).unwrap_or_default();
    if report.exists() {
        fs::remove_file(&report).unwrap();
    }
    assert!(
        result.is_ok(),
        "installation {case}: {result:?}\n{evidence}"
    );
}

// H-A5: death between restore writes must finish rollback, never reactivate or start a native unit.
#[cfg(target_os = "macos")]
#[test]
fn interrupted_restore_never_reactivates_the_failed_installation() {
    process_case("interrupted-restore");
}

// H-A5 process instrument: real installer/cursor/receipt, isolated native-manager substitute.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an owned TASKQ_INSTALL_PROCESS_CASE; native manager is substituted, not OS qualification"]
fn installation_process_fixture() {
    let install = PathBuf::from(
        std::env::var_os("TASKQ_INSTALL_PROCESS_CASE").expect("owned installation fixture"),
    );
    assert!(
        install
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("taskq-install-process-")
    );
    let base = install.parent().unwrap();
    assert_eq!(
        std::env::var_os("HOME"),
        Some(base.join("home").into_os_string())
    );
    assert_eq!(
        std::env::split_paths(&std::env::var_os("PATH").unwrap()).next(),
        Some(base.join("bin"))
    );
    assert!(
        fs::symlink_metadata(base.join("bin/launchctl"))
            .unwrap()
            .file_type()
            .is_file()
    );
    let role = std::env::var("TASKQ_INSTALL_PROCESS_ROLE").unwrap();
    if role == "host" {
        let _owner = owner(&install).unwrap().expect("one fixture host");
        let native = coreq::process::identity(std::process::id())
            .unwrap()
            .unwrap();
        let shared = Arc::new(Mutex::new(Host::new(Policy::read(&install))));
        let instance = shared.lock().unwrap().instance.clone();
        let mut threads = BTreeMap::<PathBuf, std::thread::JoinHandle<Res<()>>>::new();
        let until = Instant::now() + std::time::Duration::from_secs(120);
        loop {
            publish_service(&install, &native, &instance).unwrap();
            shared.lock().unwrap().held = admission_reason(&install);
            if !install.join("hold-project-ack").exists() {
                for root in projects(&install).unwrap_or_default() {
                    if threads.contains_key(&root) {
                        continue;
                    }
                    shared.lock().unwrap().attach(&root);
                    let host = shared.clone();
                    let install = install.clone();
                    threads.insert(
                        root.clone(),
                        std::thread::spawn(move || {
                            enter(&root, &install);
                            let mut cfg = crate::tests::cfg();
                            cfg.root = root;
                            cfg.install = install;
                            cfg.poll = 1;
                            Policy::read(&cfg.install)?.apply(&mut cfg);
                            let board = crate::board::Fake::default();
                            crate::board::Board::create(
                                &board,
                                "Must remain ready",
                                "",
                                &["taskq".into()],
                            )?;
                            crate::service_run::service_run(
                                &crate::config::Ctx {
                                    cfg,
                                    raw: json!({}),
                                    board: Box::new(board),
                                },
                                Some(host),
                            )
                        }),
                    );
                }
            }
            for thread in threads.values() {
                assert!(
                    !thread.is_finished(),
                    "project loop exited before fixture drain"
                );
            }
            if install.join("exit-host").exists() {
                shared
                    .lock()
                    .unwrap()
                    .draining
                    .extend(threads.keys().cloned());
                // A pass reading the wake signal refuses the poke; the next poll still sees `draining`.
                for root in threads.keys() {
                    let _ = crate::wake::poke(root);
                }
                break;
            }
            assert!(Instant::now() < until, "fixture host expired");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for (root, thread) in threads {
            while !thread.is_finished() {
                assert!(
                    Instant::now() < until,
                    "project drain expired: {}",
                    root.display()
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(thread.join().unwrap().unwrap_err(), "project drained");
        }
        return;
    }
    let root = PathBuf::from(std::env::var_os("TASKQ_INSTALL_PROCESS_ROOT").expect("fixture root"));
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let span = tracing::info_span!("host.install");
    let entered = span.enter();
    let request = pending(&install).ok().map(|p| p.id);
    // This native-manager instrument uses in-memory boards; the credential process case owns real preflight.
    let result = if role == "recover" {
        request
            .as_deref()
            .ok_or_else(|| "original installation cursor absent".to_string())
            .and_then(|id| install_bound(&install, &root, Some(id)))
    } else {
        upgrade(&install, &root).and_then(|_| install_bound(&install, &root, None))
    };
    let operation_count = crate::resources::with(&install, |state| Ok(state.ops.len())).unwrap();
    fs::write(install.join("installer-result.json"),serde_json::to_vec(&json!({"ok":result.is_ok(),"result":result.as_ref().ok(),"error":result.as_ref().err(),"status":installation_status(&install,request.as_deref()),"operation_count":operation_count})).unwrap()).unwrap();
    drop(entered);
    drop(span);
    capture.flush().unwrap();
    drop(guard);
    drop(capture);
}

fn fixture() -> (PathBuf, PathBuf, Vec<PathBuf>) {
    let base = std::env::temp_dir().join(format!(
        "taskq-install-originals-{}",
        crate::runtime::uuid()
    ));
    let install = base.join("install");
    let root = base.join("project");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&root).unwrap();
    let paths = vec![
        install.join(crate::platform::tool(&install).exe_name()),
        install.join("current"),
        install.join("previous"),
    ];
    #[cfg(all(windows, target_env = "gnu"))]
    let paths = {
        let mut paths = paths;
        paths.push(install.join("WebView2Loader.dll"));
        paths
    };
    fs::write(&paths[0], [0, 255, 13, 10, 42]).unwrap();
    fs::write(&paths[1], "original-current\r\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths[0], fs::Permissions::from_mode(0o751)).unwrap();
    }
    (install, root.canonicalize().unwrap(), paths)
}
// H-A5: partial activation and a failed retry restore bytes, metadata and original absence together.
#[test]
fn snapshot_restore_preserves_binary_bytes_metadata_and_absence() {
    let (install, root, paths) = fixture();
    let original: Vec<_> = paths.iter().map(|p| fs::read(p).ok()).collect();
    let hash = snapshot(&install, vec![root.clone()], paths.clone()).unwrap();
    let manifest_bytes = fs::read(manifest(&install)).unwrap();
    for path in &paths {
        fs::write(path, "partial-activation").unwrap();
    }
    let baseline: Baseline = serde_json::from_slice(&manifest_bytes).unwrap();
    restore_files(&install, &directory(&install), &baseline).unwrap();
    for (path, bytes) in paths.iter().zip(original) {
        assert_eq!(fs::read(path).ok(), bytes);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&paths[0]).unwrap().permissions().mode() & 0o777,
            0o751
        );
    }
    assert_eq!(
        snapshot(&install, vec![root], paths.clone()).unwrap(),
        hash,
        "repeat/attach cannot overwrite topology originals"
    );
    assert_eq!(fs::read(manifest(&install)).unwrap(), manifest_bytes);
    for label in ["later installation", "new owner before intent"] {
        for path in &paths {
            fs::write(path, label).unwrap();
        }
        let folder = activation_directory(&install);
        let latest = capture_originals(&folder, vec![], paths.clone()).unwrap();
        for path in &paths {
            fs::write(path, "partial later activation").unwrap();
        }
        restore_files(&install, &folder, &latest).unwrap();
        for path in &paths {
            assert_eq!(fs::read_to_string(path).unwrap(), label);
        }
        assert_eq!(
            fs::read_dir(folder).unwrap().count(),
            paths.len(),
            "reuse one fixed copy set; no per-command archive"
        );
        assert_eq!(
            fs::read(manifest(&install)).unwrap(),
            manifest_bytes,
            "latest-state recovery cannot replace first migration originals"
        );
    }
    #[cfg(windows)]
    {
        let writable = fs::metadata(&paths[1]).unwrap().permissions();
        let mut readonly = writable.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&paths[1], readonly).unwrap();
        let folder = activation_directory(&install);
        let latest = capture_originals(&folder, vec![], paths.clone()).unwrap();
        fs::set_permissions(&paths[1], writable.clone()).unwrap();
        fs::write(&paths[1], "partial readonly activation").unwrap();
        for _ in 0..2 {
            restore_files(&install, &folder, &latest).unwrap();
            assert_eq!(
                fs::read_to_string(&paths[1]).unwrap(),
                "new owner before intent"
            );
            assert!(fs::metadata(&paths[1]).unwrap().permissions().readonly());
            assert_eq!(ownership(&paths[1]).unwrap(), latest.files[1].owner);
        }
        fs::set_permissions(&paths[1], writable.clone()).unwrap();
        fs::set_permissions(folder.join("1"), writable).unwrap();
    }
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}
// H-A5: corrupt evidence must be detected before the first restore effect, rather than halfway through.
#[test]
fn corrupt_snapshot_refuses_restore_before_any_active_write() {
    let (install, root, paths) = fixture();
    snapshot(&install, vec![root], paths.clone()).unwrap();
    let baseline: Baseline =
        serde_json::from_slice(&fs::read(manifest(&install)).unwrap()).unwrap();
    fs::write(directory(&install).join("1"), "corrupt").unwrap();
    fs::write(&paths[0], "current-launcher").unwrap();
    assert!(verify_originals(&install).unwrap_err().contains("checksum"));
    assert!(
        restore_files(&install, &directory(&install), &baseline)
            .unwrap_err()
            .contains("checksum")
    );
    assert_eq!(fs::read(&paths[0]).unwrap(), b"current-launcher");
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}
// #1306: under an older running host the receipt comes first, then `current`, rollback floor and the host's own
// update poke; no `projects.installing` exists until the host runs the candidate, so an older host keeps its
// transitions. Replay keeps the command, the attach under the candidate adopts it, and an equal host stays attach-only.
#[test]
fn older_running_host_upgrades_only_after_the_receipt() {
    let (install, root, paths) = fixture();
    snapshot(&install, vec![root.clone()], paths).unwrap();
    let root = root.canonicalize().unwrap();
    let tool = crate::platform::tool(&install);
    fs::create_dir_all(install.join("releases/0.0.1")).unwrap();
    fs::write(tool.release_exe("0.0.1"), "invented-release").unwrap();
    fs::write(install.join("current"), "0.0.1").unwrap();
    let native = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let host = |version: &str| {
        fs::write(
            install.join("service"),
            json!({"schema":1,"pid":native.pid,"started":native.started,"instance":"invented-host",
            "at":wall().unwrap(),"version":version,"install_protocol":1})
            .to_string(),
        )
        .unwrap();
        service_status(&install)
    };
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let span = tracing::info_span!("host.install");
    let entered = span.enter();
    let old = host("0.0.1");
    let v = request_upgrade(&install, &tool, &root, &old, &native).unwrap();
    assert_eq!(v.as_deref(), Some(crate::platform::VERSION));
    let p = upgrade_intent(&install).unwrap().unwrap();
    for _ in 0..2 {
        assert_eq!(
            installation_status(&install, Some(&p.id))["state"],
            "queued",
            "the receipt precedes activation"
        );
        assert!(
            !install.join(INSTALL_PENDING).exists(),
            "an older host keeps its transitions"
        );
        assert_eq!(tool.current().as_deref(), Some(crate::platform::VERSION));
        assert_eq!(
            fs::read_to_string(install.join("previous")).unwrap(),
            "0.0.1"
        );
        assert_eq!(
            fs::read_to_string(install.join("rollback-floor")).unwrap(),
            "0.0.1"
        );
        assert!(
            install.join("update").exists() && !install.join("restart").exists(),
            "the host's own update step re-execs"
        );
        assert_eq!(
            request_upgrade(&install, &tool, &root, &old, &native).unwrap(),
            v,
            "replay repeats nothing destructive"
        );
        assert_eq!(upgrade_intent(&install).unwrap().unwrap().id, p.id);
    }
    assert!(
        installation_status(&install, Some(&p.id))["reason"]
            .as_str()
            .unwrap()
            .contains("re-exec")
    );
    // #1601: the waiting host names its real re-exec waits on the receipt; a replayed install keeps them.
    assert!(upgrade_waits(
        &install,
        "0.0.1",
        "taskq #1580 unresolved ownership"
    ));
    assert_eq!(
        request_upgrade(&install, &tool, &root, &old, &native).unwrap(),
        v
    );
    let reason = installation_status(&install, Some(&p.id))["reason"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        reason.ends_with("waits for taskq #1580 unresolved ownership"),
        "{reason}"
    );
    // #2209: a superseding intent starts on the placeholder while the waits stay the same; the next tick names them again.
    retain_reason(
        &install,
        &p,
        "host update step pending; the host names its re-exec waits here",
    )
    .unwrap();
    assert!(upgrade_waits(
        &install,
        "0.0.1",
        "taskq #1580 unresolved ownership"
    ));
    assert_eq!(
        installation_status(&install, Some(&p.id))["reason"]
            .as_str()
            .unwrap(),
        reason,
        "same waits retained again"
    );
    assert!(
        adopt_upgrade(&install, &root, "0.0.1").unwrap().is_none(),
        "the old host never binds the candidate's command"
    );
    let new = host(crate::platform::VERSION);
    assert_eq!(
        request_upgrade(&install, &tool, &root, &new, &native).unwrap(),
        None
    );
    // #1402: the host that runs the candidate drops the pre-re-exec reason at once, before the 300 s report.
    assert!(
        stale_upgrade(&install, false)
            .unwrap()
            .contains("the waiting service install")
    );
    let reason = installation_status(&install, Some(&p.id))["reason"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        !reason.contains("re-exec waits") && reason.contains(&p.id),
        "{reason}"
    );
    assert!(
        stale_upgrade(&install, true)
            .unwrap()
            .contains("repeat service install")
    );
    // #1415: another host writer holding the resources lock delays the re-exec'd host's receipt instead of failing it;
    // one held past the bound is refused with the holder named.
    let hold = |release: std::sync::mpsc::Receiver<()>| {
        let (held, ready) = std::sync::mpsc::channel();
        let install = install.clone();
        let writer = std::thread::spawn(move || {
            crate::resources::locked(&install, || {
                held.send(()).unwrap();
                release.recv().unwrap();
                Ok(())
            })
            .unwrap()
        });
        ready.recv().unwrap();
        writer
    };
    let (release, receive) = std::sync::mpsc::channel();
    let writer = hold(receive);
    let reason = installation_status(&install, Some(&p.id))["reason"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        reason.starts_with("resources lock unavailable:")
            && reason.contains(&format!("held by pid {}", std::process::id())),
        "{reason}"
    );
    let late = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        release.send(()).unwrap();
    });
    let adopted = adopt_upgrade(&install, &root, crate::platform::VERSION)
        .unwrap()
        .unwrap();
    late.join().unwrap();
    writer.join().unwrap();
    assert_eq!(
        (adopted.id.as_str(), adopted.command.as_str()),
        (p.id.as_str(), p.command.as_str()),
        "the attach continues the same command"
    );
    fs::remove_file(install.join(UPGRADE_INTENT)).unwrap();
    assert_eq!(
        request_upgrade(&install, &tool, &root, &new, &native).unwrap(),
        None,
        "attach-only: an equal host is not upgraded"
    );
    assert!(upgrade_intent(&install).unwrap().is_none());
    // #1601: a queued upgrade to an older build failed every newer install until that build repeated it by hand.
    // The newer build supersedes it; the old receipt keeps its failure and reason.
    let mut stale: Pending = serde_json::from_value(json!(adopted)).unwrap();
    stale.id = crate::runtime::uuid();
    stale.version = "0.0.1".into();
    coreq::write_durable(
        &install.join(UPGRADE_INTENT),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();
    record(&install, &stale, "received").unwrap();
    assert_eq!(
        request_upgrade(&install, &tool, &root, &new, &native).unwrap(),
        None
    );
    let old_receipt = installation_status(&install, Some(&stale.id));
    assert_eq!(old_receipt["state"], "failed");
    assert!(
        old_receipt["reason"]
            .as_str()
            .unwrap()
            .contains("superseded by"),
        "{old_receipt}"
    );
    assert!(upgrade_intent(&install).unwrap().is_none());
    drop(entered);
    drop(span);
    capture.flush().unwrap();
    drop(guard);
    drop(capture);
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}
// H-A5: an interrupted pre-effect snapshot resumes, while an unknown file is preserved and refused.
#[test]
fn snapshot_rebuilds_only_its_uncommitted_slots() {
    let (install, root, paths) = fixture();
    fs::create_dir_all(directory(&install)).unwrap();
    fs::write(directory(&install).join("0"), "partial-copy").unwrap();
    fs::write(directory(&install).join("owner-note"), "preserve").unwrap();
    assert!(
        snapshot(&install, vec![root.clone()], paths.clone())
            .unwrap_err()
            .contains("unknown")
    );
    assert_eq!(
        fs::read_to_string(directory(&install).join("owner-note")).unwrap(),
        "preserve"
    );
    fs::remove_file(directory(&install).join("owner-note")).unwrap();
    snapshot(&install, vec![root], paths).unwrap();
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}
// H-A5: only this persisted registration and host generation can publish an installation acknowledgement.
#[test]
fn registered_cursor_holds_admission_and_binds_project_acknowledgement() {
    let (install, root, paths) = fixture();
    let baseline = snapshot(&install, vec![root.clone()], paths).unwrap();
    let mut p=Pending {schema:1,id:"invented-registration".into(),baseline,root:root.clone(),version:crate::platform::VERSION.into(),candidate:"a".repeat(64),
            original:None,next:format!("{}\n",root.display()),phase:"prepared".into(),restore:true,attach:false,
            owner:Some(InstallOwner {pid:7,started:"invented".into(),instance:"invented-host".into()}),activation:None,command:json!({"wire":1,"product":"taskq","version":"fixture","operation":"taskq.host.install","trace":format!("{}.1","a".repeat(32)),"span":format!("{}.2","a".repeat(32)),"parent":null,"started_ms":1}).to_string()};
    save(&install, &p).unwrap();
    assert!(registered_projects(&install).is_err());
    p.phase = "registered".into();
    save(&install, &p).unwrap();
    assert!(
        registered_projects(&install).is_err(),
        "cursor cannot fabricate a registry commit"
    );
    let lock = coreq::file_lock::try_lock(&install.join("projects.lock")).unwrap();
    ProjectEdit {
        install: install.clone(),
        original: None,
        next: p.next.clone(),
        removed: vec![],
        adding: true,
        _lock: lock,
    }
    .commit()
    .unwrap();
    assert_eq!(registered_projects(&install).unwrap(), Some(p.next.clone()));
    assert!(
        admission_reason(&install).is_some(),
        "membership is readable for recovery; launches remain held"
    );
    assert_eq!(
        install_acknowledgement(&install, &root, "invented-host"),
        Some(p.id.clone())
    );
    assert_eq!(install_acknowledgement(&install, &root, "prior-host"), None);
    let prepare = install.join(format!("{INSTALL_PENDING}.pending"));
    fs::write(&prepare, "incomplete cursor publication").unwrap();
    assert!(
        registered_projects(&install)
            .unwrap_err()
            .contains("publication incomplete")
    );
    assert!(project_text(&install).is_err());
    assert_eq!(
        install_acknowledgement(&install, &root, "invented-host"),
        None
    );
    fs::remove_file(prepare).unwrap();
    p.version = "4.4.0".into();
    p.attach = true;
    save(&install, &p).unwrap();
    assert_eq!(
        install_acknowledgement(&install, &root, "invented-host"),
        Some(p.id.clone()),
        "protocol-compatible reader accepts another installer version"
    );
    assert!(
        ProjectEdit::new(&install, &root, true).is_err(),
        "another editor cannot consume a registered cursor"
    );
    let retained = fs::read(install.join(INSTALL_PENDING)).unwrap();
    fs::write(manifest(&install), "changed-originals").unwrap();
    assert!(
        registered_projects(&install)
            .unwrap_err()
            .contains("originals changed")
    );
    assert_eq!(
        install_acknowledgement(&install, &root, "invented-host"),
        None
    );
    assert_eq!(fs::read(install.join(INSTALL_PENDING)).unwrap(), retained);
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}

// H-A5: the original observed receipt remains queryable after the installer/cursor disappears.
#[test]
fn installation_receipt_survives_cursor_removal_and_rejects_missing_phase_proof() {
    let (install, root, paths) = fixture();
    let baseline = snapshot(&install, vec![root.clone()], paths.clone()).unwrap();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let span = tracing::info_span!("host.install");
    let entered = span.enter();
    let command = CommandReceipt::current().unwrap().encode();
    let mut p = Pending {
        schema: 1,
        id: "invented-install-receipt".into(),
        baseline,
        root: root.clone(),
        version: crate::platform::VERSION.into(),
        candidate: "a".repeat(64),
        original: None,
        next: format!("{}\n", root.display()),
        phase: "initial".into(),
        restore: false,
        attach: true,
        owner: Some(InstallOwner {
            pid: 7,
            started: "invented".into(),
            instance: "invented-host".into(),
        }),
        command,
        activation: None,
    };
    assert!(
        receive_initial(&install, &mut p).is_err(),
        "no receipt before durable intent"
    );
    save(&install, &p).unwrap();
    receive_initial(&install, &mut p).unwrap();
    assert_eq!(p.phase, "owned");
    p.phase = "initial".into();
    save(&install, &p).unwrap();
    receive_initial(&install, &mut p).unwrap();
    assert_eq!(
        crate::resources::with(&install, |state| Ok(state.ops.len())).unwrap(),
        1,
        "received replay cannot orphan another command"
    );
    p.activation = Some(capture_originals(&activation_directory(&install), vec![], paths).unwrap());
    assert!(
        retained(&install, &p)
            .unwrap_err()
            .contains("binding changed"),
        "latest restore metadata is part of the original command binding"
    );
    p.activation = None;
    retain_reason(
        &install,
        &p,
        "native start response lost; original installation held",
    )
    .unwrap();
    assert_eq!(
        installation_status(&install, Some(&p.id))["reason"],
        "native start response lost; original installation held"
    );
    for phase in ["applied", "observed"] {
        record(&install, &p, phase).unwrap();
    }
    record(&install, &p, "applied").unwrap();
    assert_eq!(
        installation_status(&install, Some(&p.id))["state"],
        "observed",
        "a late observer cannot regress the original outcome"
    );
    fs::remove_file(install.join(INSTALL_PENDING)).unwrap();
    let receipt = installation_status(&install, Some(&p.id));
    assert_eq!(receipt["state"], "observed");
    assert!(receipt["observed_receipt"].is_string());
    assert!(
        receipt.get("reason").is_none(),
        "verified progress clears the old wait reason"
    );
    let (first_id, first_command) = (p.id.clone(), p.command.clone());
    let second_span = tracing::info_span!("host.install");
    let second_enter = second_span.enter();
    p.id = "invented-other-installation".into();
    p.command = CommandReceipt::current().unwrap().encode();
    record(&install, &p, "received").unwrap();
    crate::resources::with(&install, |state| {
        for id in [&first_id, &p.id] {
            state.ops.get_mut(id).unwrap().saved_at = Some(42);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        installation_status(&install, None)["state"],
        "unknown",
        "equal observation clocks cannot attribute one command's result to another"
    );
    assert_eq!(
        installation_status(&install, Some(&first_id))["state"],
        "observed"
    );
    assert_eq!(
        installation_status(&install, Some(&p.id))["state"],
        "queued"
    );
    drop(second_enter);
    drop(second_span);
    p.id = first_id;
    p.command = first_command;
    crate::resources::with(&install, |state| {
        state
            .ops
            .get_mut(&p.id)
            .unwrap()
            .receipt
            .as_object_mut()
            .unwrap()
            .remove("observed_receipt");
        Ok(())
    })
    .unwrap();
    assert_eq!(
        installation_status(&install, Some(&p.id))["state"],
        "unknown"
    );
    drop(entered);
    drop(span);
    capture.flush().unwrap();
    drop(guard);
    drop(capture);
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}

// #2495: a cursor gone before its terminal record left `queued` and a wake failure in status, pinned forever.
#[test]
fn orphaned_receipt_ends_failed_while_any_owning_cursor_keeps_it() {
    let (install, root, paths) = fixture();
    let baseline = snapshot(&install, vec![root.clone()], paths.clone()).unwrap();
    assert_eq!(
        installation_status(&install, None),
        json!({"state":"none"}),
        "no install was asked about and none is retained"
    );
    assert_eq!(
        installation_status(&install, Some("invented-absent"))["state"],
        "unknown",
        "a requested ID without a receipt stays unknown"
    );
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let span = tracing::info_span!("host.install");
    let entered = span.enter();
    let mut p = Pending {
        schema: 1,
        id: "invented-orphan-receipt".into(),
        baseline,
        root: root.clone(),
        version: crate::platform::VERSION.into(),
        candidate: "a".repeat(64),
        original: None,
        next: format!("{}\n", root.display()),
        phase: "initial".into(),
        restore: false,
        attach: true,
        owner: None,
        command: CommandReceipt::current().unwrap().encode(),
        activation: None,
    };
    save(&install, &p).unwrap();
    receive_initial(&install, &mut p).unwrap();
    let old =
        "service wake failed (wake-lock: Incorrect function. (os error 1)); check service status";
    retain_reason(&install, &p, old).unwrap();

    // Every cursor form that may own the receipt keeps it queued.
    assert!(retire_orphans(&install).unwrap().is_empty());
    fs::rename(install.join(INSTALL_PENDING), install.join(UPGRADE_INTENT)).unwrap();
    assert!(
        retire_orphans(&install).unwrap().is_empty(),
        "an upgrade intent owns its receipt"
    );
    fs::write(install.join(UPGRADE_INTENT), "not a cursor").unwrap();
    assert!(
        retire_orphans(&install).unwrap().is_empty(),
        "an unreadable cursor is unknown ownership, not absence"
    );
    let live = installation_status(&install, Some(&p.id));
    assert_eq!(live["state"], "queued");
    assert_eq!(live["reason"], old);

    fs::remove_file(install.join(UPGRADE_INTENT)).unwrap();
    let saved_at = crate::resources::with(&install, |state| Ok(state.ops[&p.id].saved_at)).unwrap();
    assert_eq!(retire_orphans(&install).unwrap(), vec![p.id.clone()]);
    let retired = installation_status(&install, None);
    assert_eq!(retired["state"], "failed", "{retired}");
    assert!(retired["failed_receipt"].is_string(), "{retired}");
    let reason = retired["reason"].as_str().unwrap();
    assert!(reason.contains("repeat service install"), "{reason}");
    assert!(reason.contains("last: service wake failed"), "{reason}");
    assert_eq!(
        crate::cli::service_cmd::install_rows(&retired),
        vec![
            format!("Install: {} failed", p.id),
            format!("Install: failed ({reason})"),
        ],
        "a known outcome does not read as unknown"
    );
    assert!(
        crate::cli::service_cmd::install_rows(&json!({"state":"none"})).is_empty(),
        "no retained receipt prints no install row"
    );
    assert_eq!(
        crate::cli::service_cmd::install_rows(&json!({"state":"unknown","reason":"why"})),
        vec!["Install: unknown (why)"],
        "true unknown stays unknown"
    );
    assert!(
        retire_orphans(&install).unwrap().is_empty(),
        "a terminal receipt cannot end twice"
    );
    assert_eq!(
        crate::resources::with(&install, |state| Ok(state.ops[&p.id].saved_at)).unwrap(),
        saved_at,
        "the old command keeps its observation time and cannot shadow a newer one"
    );
    // The terminal state ends the pin: the normal retention now removes the receipt.
    let later = wall().unwrap() + 25 * 3600;
    fs::write(install.join("projects"), &p.next).unwrap();
    crate::resources::with(&install, |state| {
        crate::resources::prepare(state, &install, later)
    })
    .unwrap();
    assert_eq!(installation_status(&install, None)["state"], "none");
    drop(entered);
    drop(span);
    drop(guard);
    drop(capture);
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}

// H-A5 model: caller/host observers race after registry commit; only the original command may complete.
#[test]
fn detached_install_observers_share_one_acknowledgement_and_cannot_create_a_command() {
    let (install, root, paths) = fixture();
    let baseline = snapshot(&install, vec![root.clone()], paths.clone()).unwrap();
    let original = fs::read(&paths[0]).unwrap();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let span = tracing::info_span!("host.install");
    let entered = span.enter();
    let native = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    publish_service(&install, &native, "invented-install-host").unwrap();
    let mut p = Pending {
        schema: 1,
        id: "invented-detached-install".into(),
        baseline,
        root: root.clone(),
        version: crate::platform::VERSION.into(),
        candidate: "a".repeat(64),
        original: None,
        next: format!("{}\n", root.display()),
        phase: "registered".into(),
        restore: false,
        attach: true,
        owner: Some(InstallOwner {
            pid: native.pid,
            started: native.started,
            instance: "invented-install-host".into(),
        }),
        command: CommandReceipt::current().unwrap().encode(),
        activation: None,
    };
    save(&install, &p).unwrap();
    fs::write(install.join("projects"), &p.next).unwrap();
    fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    record(&install, &p, "received").unwrap();
    record(&install, &p, "applied").unwrap();
    let mut status = json!({"pid":p.owner.as_ref().unwrap().pid,"host_instance":"invented-install-host","host_install":"prior-registration","complete":true,"tasks":[]});
    let path = crate::paths::dot(&root).join("status.json");
    fs::write(&path, status.to_string()).unwrap();
    assert!(
        !acknowledge(&install, &p).unwrap(),
        "same PID/generation cannot acknowledge a different registration"
    );
    status["host_install"] = json!(p.id);
    status["host_instance"] = json!("prior-host");
    fs::write(&path, status.to_string()).unwrap();
    assert!(
        !acknowledge(&install, &p).unwrap(),
        "old project generation cannot acknowledge the installation"
    );
    status["host_instance"] = json!("invented-install-host");
    fs::write(&path, status.to_string()).unwrap();
    let mut h = Host::new(Err("publication fixture".into()));
    h.instance = "invented-install-host".into();
    let shared = Arc::new(Mutex::new(h));
    for (key, value) in [
        ("complete", json!(false)),
        ("error", json!("board credential rejected")),
        ("focus_error", json!("focus unreadable")),
    ] {
        let mut failed = status.clone();
        failed[key] = value;
        assert!(!crate::service_run::publish_status(
            &crate::platform::tool(&install),
            &root,
            &failed,
            Some(&shared)
        ));
        assert!(
            crate::data::json(&path).unwrap().unwrap()["host_install"].is_null(),
            "failed producer cannot stamp acknowledgement"
        );
        fs::write(&path, failed.to_string()).unwrap(); // an older publisher's matching ID still cannot pass the consumer
        assert!(
            !acknowledge(&install, &p).unwrap(),
            "failed project publication cannot observe installation: {key}"
        );
        assert_eq!(
            installation_status(&install, Some(&p.id))["state"],
            "applied"
        );
        assert!(install.join(INSTALL_PENDING).exists());
    }
    assert!(crate::service_run::publish_status(
        &crate::platform::tool(&install),
        &root,
        &status,
        Some(&shared)
    ));
    let lock = coreq::file_lock::try_lock(&install.join("projects.lock")).unwrap();
    assert!(
        !acknowledge(&install, &p).unwrap(),
        "observer never waits on the registry owner"
    );
    drop(lock);
    assert!(acknowledge(&install, &p).unwrap());
    assert!(
        acknowledge(&install, &p).unwrap(),
        "the other observer reads the retained outcome"
    );
    assert!(
        install_bound(&install, &root, Some(&p.id))
            .unwrap()
            .contains("already observed")
    );
    assert!(!install.join(INSTALL_PENDING).exists());
    assert_eq!(fs::read(&paths[0]).unwrap(), original);
    assert_eq!(
        crate::resources::with(&install, |state| Ok(state.ops.len())).unwrap(),
        1
    );
    p.id = "another-installation".into();
    save(&install, &p).unwrap();
    let marker = fs::read(install.join(INSTALL_PENDING)).unwrap();
    assert!(
        install_bound(&install, &root, Some("invented-detached-install"))
            .unwrap_err()
            .contains("another installation")
    );
    assert_eq!(fs::read(install.join(INSTALL_PENDING)).unwrap(), marker);
    capture.flush().unwrap();
    drop(entered);
    drop(span);
    drop(guard);
    drop(capture);
    fs::remove_dir_all(install.parent().unwrap()).unwrap();
}
