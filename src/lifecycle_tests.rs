use super::*;
fn fixture() -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("taskq-restart-{}", crate::runtime::uuid()));
    let install = dir.join("install");
    let root = dir.join("project");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(
        install.join("projects"),
        format!("{}\n", root.canonicalize().unwrap().display()),
    )
    .unwrap();
    fs::write(install.join("current"), crate::platform::VERSION).unwrap();
    fs::write(install.join("host.json"), "{}").unwrap();
    fs::write(root.join("taskq.json"), r#"{"repo":"fixture/project"}"#).unwrap();
    publish(&install, "original");
    (dir, install, root)
}

fn fixture_with_other() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let (dir, install, root) = fixture();
    let (dir, install, root) = (
        dir.canonicalize().unwrap(),
        install.canonicalize().unwrap(),
        root.canonicalize().unwrap(),
    );
    let other = dir.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("taskq.json"), r#"{"repo":"fixture/other"}"#).unwrap();
    let other = other.canonicalize().unwrap();
    fs::write(
        install.join("projects"),
        format!("{}\n{}\n", root.display(), other.display()),
    )
    .unwrap();
    (dir, install, root, other)
}
fn publish(install: &Path, instance: &str) {
    let process = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    fs::write(install.join("service"),json!({"schema":1,"pid":process.pid,"started":process.started,"instance":instance,"at":now().unwrap(),"version":"fixture","restart_protocol":1,"stop_protocol":2,"uninstall_protocol":1}).to_string()).unwrap();
}

// R11/E2: a durable scoped intent never becomes an exit receipt or a broad release.
#[test]
fn full_stop_hold_retains_scope_and_refuses_payload_replacement() {
    let (dir, install, root, other) = fixture_with_other();
    let at = now().unwrap();
    let value = json!({"id":"full-stop","instance":"original","roots":[root],"issued_at":at,"expires_at":at+300,"action":"stop","mode":"full"});
    let request = typed(&value).unwrap();
    save_stop_hold(&install, &request).unwrap();
    assert!(full_stop(&install, &root).unwrap());
    assert!(!full_stop(&install, &other).unwrap());
    assert!(release_guard(&install).is_err());
    let saved = fs::read(install.join("hold")).unwrap();
    save_stop_hold(&install, &request).unwrap();
    assert_eq!(
        fs::read(install.join("hold")).unwrap(),
        saved,
        "same request remains idempotent"
    );
    let mut changed = request.clone();
    changed.mode = StopMode::Drain;
    assert!(save_stop_hold(&install, &changed).is_err());
    assert_eq!(fs::read(install.join("hold")).unwrap(), saved);
    assert!(
        receipt(&install, "full-stop").unwrap().is_none(),
        "local intent cannot fabricate a lifecycle exit receipt"
    );
    let mut recovered: Value = serde_json::from_slice(&saved).unwrap();
    recovered["recovery"] = json!({"invented":{"state":"accepted"}});
    recovered["recovery_start"] = json!({"state":"requested"});
    recovered["previous_hold"] = json!("prior operator hold");
    fs::write(install.join("hold"), recovered.to_string()).unwrap();
    let mut next = request.clone();
    next.id = "next-reviewed-stop".into();
    save_stop_hold(&install, &next).unwrap();
    let next_hold: Value =
        serde_json::from_str(&fs::read_to_string(install.join("hold")).unwrap()).unwrap();
    assert!(next_hold.get("recovery").is_none());
    assert!(next_hold.get("recovery_start").is_none());
    assert_eq!(
        next_hold["previous_hold"], "prior operator hold",
        "new Stop revokes recovery, never an older operator hold"
    );
    fs::write(install.join("hold"), "{broken").unwrap();
    assert!(full_stop(&install, &root).is_err());
    assert!(release_guard(&install).is_err());
    assert!(
        save_stop_hold(&install, &request).is_err(),
        "corrupt Stop identity cannot be overwritten"
    );
    fs::remove_dir_all(dir).unwrap();
}

// #1769 A2: a kickstart left the drain Stop of the ended generation queued, and the new generation held admission
// on it ("Stop belongs to another host generation") until the op was removed by hand. Risk kept: the origin
// generation, an applied Stop before its expiry and a non-owner process never drop it.
#[test]
fn new_generation_drops_the_stop_of_an_ended_generation() {
    let (dir, install, root) = fixture();
    let stop = |id: &str, applied: bool, expires_at: u64| {
        let at = now().unwrap();
        let request=typed(&json!({"id":id,"instance":"original","roots":[root.canonicalize().unwrap()],"issued_at":expires_at-300,"expires_at":expires_at,"action":"stop"})).unwrap();
        let trace = crate::runtime::uuid().replace('-', "");
        let command=CommandReceipt::decode(&json!({"wire":1,"product":"taskq","version":crate::platform::VERSION,"operation":"taskq.host.stop.request",
                "trace":format!("{trace}.1"),"span":format!("{trace}.1"),"parent":null,"started_ms":at*1000}).to_string()).unwrap();
        let mut receipt = json!({"id":id,"state":"queued","received_receipt":command.event_id("received").unwrap()});
        if applied {
            receipt["state"] = json!("applied");
            receipt["applied_receipt"] = json!(command.event_id("applied").unwrap());
            receipt["native_pid"] = json!(1);
            receipt["native_started"] = json!("gone");
            receipt["unit_revision"] = json!("0".repeat(64));
        }
        let op = resources::Op {
            bound: json!({"local_stop":{"request":request,"command":command.encode()}}),
            receipt,
            expires_at,
            saved_at: Some(at),
            writing: false,
            sent: true,
        };
        resources::with(&install, |state| {
            state.ops.insert(id.into(), op);
            Ok(())
        })
        .unwrap();
    };
    let at = now().unwrap();
    stop("queued-stop", false, at + 300);
    assert_eq!(
        supersede(&install, "original").unwrap(),
        None,
        "the origin generation still owns its Stop"
    );
    assert!(
        supersede(&install, "next").is_err(),
        "a process that is not the published owner drops nothing"
    );
    assert!(pending(&install).unwrap().is_some());
    publish(&install, "next");
    assert_eq!(
        supersede(&install, "next").unwrap().as_deref(),
        Some("queued-stop")
    );
    assert!(
        pending(&install).unwrap().is_none(),
        "admission no longer waits on the ended generation"
    );
    stop("applied-stop", true, at + 300);
    assert_eq!(
        supersede(&install, "next").unwrap(),
        None,
        "an applied Stop keeps its controller window"
    );
    resources::with(&install, |state| {
        state.ops.remove("applied-stop");
        Ok(())
    })
    .unwrap();
    stop("expired-stop", true, at - 1);
    assert_eq!(
        supersede(&install, "next").unwrap().as_deref(),
        Some("expired-stop")
    );
    assert!(pending(&install).unwrap().is_none());
    fs::remove_dir_all(dir).unwrap();
}

// #1769 A4: past its expiry a waiting Stop names the project waits in its receipt reason.
#[test]
fn late_stop_reason_names_the_blocking_task() {
    let (dir, install, root) = fixture();
    let at = now().unwrap();
    let request=typed(&json!({"id":"late-stop","instance":"original","roots":[root.canonicalize().unwrap()],"issued_at":at-301,"expires_at":at-1,"action":"stop"})).unwrap();
    let trace = crate::runtime::uuid().replace('-', "");
    let command=CommandReceipt::decode(&json!({"wire":1,"product":"taskq","version":crate::platform::VERSION,"operation":"taskq.host.stop.request",
            "trace":format!("{trace}.1"),"span":format!("{trace}.1"),"parent":null,"started_ms":at*1000}).to_string()).unwrap();
    let op = resources::Op {
        bound: json!({"local_stop":{"request":request,"command":command.encode()}}),
        receipt: json!({"id":"late-stop","state":"queued","received_receipt":command.event_id("received").unwrap()}),
        expires_at: at - 1,
        saved_at: Some(at),
        writing: false,
        sent: true,
    };
    resources::with(&install, |state| {
        state.ops.insert("late-stop".into(), op);
        Ok(())
    })
    .unwrap();
    handoff_wait(
        &install,
        "original",
        Some(HandoffWait::Execution),
        "project #1758",
    )
    .unwrap();
    let reason = receipt(&install, "late-stop").unwrap().unwrap()["reason"].clone();
    assert_eq!(
        reason,
        json!(format!(
            "{}; over expiry, waits for project #1758",
            HandoffWait::Execution.reason()
        ))
    );
    handoff_wait(
        &install,
        "original",
        Some(HandoffWait::Project),
        "project #1759 unresolved ownership",
    )
    .unwrap();
    assert!(
        receipt(&install, "late-stop").unwrap().unwrap()["reason"]
            .as_str()
            .unwrap()
            .ends_with("waits for project #1759 unresolved ownership"),
        "a later wait refreshes the reason"
    );
    handoff_wait(&install, "original", None, "").unwrap();
    assert!(receipt(&install, "late-stop").unwrap().unwrap()["reason"].is_null());
    fs::remove_dir_all(dir).unwrap();
}

// M15: Open clears only the tray's Quit hold once its generation is gone and restores the older operator hold;
// a reviewed Stop hold stays and the service stays stopped.
#[test]
fn open_clears_only_a_gone_quit_hold() {
    let (dir, install, root) = fixture();
    let at = now().unwrap();
    let quit=typed(&json!({"id":"quit-stop","instance":"original","roots":[root],"issued_at":at,"expires_at":at+300,"action":"stop","mode":"quit"})).unwrap();
    fs::write(install.join("hold"), "operator hold").unwrap();
    save_stop_hold(&install, &quit).unwrap();
    assert!(
        !full_stop(&install, &root).unwrap(),
        "Quit drains, never cancels"
    );
    assert!(
        release_guard(&install).is_err(),
        "ordinary release still refuses"
    );
    let started = std::cell::Cell::new(false);
    assert_eq!(
        open(&install, |_| {
            started.set(true);
            Ok("started".into())
        })
        .unwrap(),
        None,
        "Quit generation still draining"
    );
    assert!(stop_hold(&install).unwrap().is_some());
    publish(&install, "next-generation");
    assert_eq!(
        open(&install, |_| {
            started.set(true);
            Ok("started".into())
        })
        .unwrap(),
        None,
        "a running service needs no start"
    );
    assert_eq!(
        fs::read_to_string(install.join("hold")).unwrap(),
        "operator hold"
    );
    let reviewed=typed(&json!({"id":"reviewed-stop","instance":"original","roots":[root],"issued_at":at,"expires_at":at+300,"action":"stop"})).unwrap();
    fs::remove_file(install.join("hold")).unwrap();
    save_stop_hold(&install, &reviewed).unwrap();
    fs::remove_file(install.join("service")).unwrap();
    assert!(
        open(&install, |_| {
            started.set(true);
            Ok("started".into())
        })
        .is_err()
    );
    assert!(
        !started.get(),
        "reviewed Stop hold keeps the service stopped"
    );
    assert_eq!(stop_hold(&install).unwrap(), Some(reviewed));
    fs::remove_file(install.join("hold")).unwrap();
    assert_eq!(
        open(&install, |r| {
            assert_eq!(r, root.canonicalize().unwrap());
            Ok("started".into())
        })
        .unwrap()
        .as_deref(),
        Some("started")
    );
    fs::remove_dir_all(dir).unwrap();
}

// H-A5: a non-last root remains registered until one drained thread has an applied receipt; a crash after
// registry removal finalizes the original receipt without re-draining or recreating project work.
#[test]
fn project_uninstall_drains_before_registry_removal_and_replays_exact_receipt() {
    let (dir, install, root, other) = fixture_with_other();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let request = uninstall_review(&install, "drain-project", Some(&root)).unwrap();
    let received = submit(&install, &request).unwrap();
    assert_eq!(received["state"], "queued");
    assert!(
        prepare(&install, "original")
            .unwrap_err()
            .contains("qualified project drain")
    );
    assert_eq!(
        fs::read_to_string(install.join("projects")).unwrap(),
        format!("{}\n{}\n", root.display(), other.display())
    );
    assert!(
        finish_project_uninstall(&install, "drain-project", "original", false)
            .unwrap_err()
            .contains("project drain not observed")
    );
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        assert!(
            finish_project_uninstall(&install, "drain-project", "original", true)
                .unwrap_err()
                .contains("collector unavailable")
        );
    });
    assert_eq!(
        receipt(&install, "drain-project").unwrap().unwrap()["state"],
        "queued",
        "receipt refusal must leave a readable queued request"
    );
    assert!(
        resources::try_with(&install, |state| Ok(state.ops["drain-project"]
            .receipt
            .get("registry_owner")
            .is_none()))
        .unwrap()
    );

    let db = rusqlite::Connection::open(install.join("telemetry.log/universal.sqlite3")).unwrap();
    // #1351: refused receipt commits stay unknown; the drained uninstall proceeds.
    db.execute_batch("CREATE TRIGGER refuse_uninstall_receipts BEFORE INSERT ON events WHEN NEW.kind IN ('command.applied','command.observed') BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    finish_project_uninstall(&install, "drain-project", "original", true).unwrap();
    assert_eq!(
        fs::read_to_string(install.join("projects")).unwrap(),
        format!("{}\n", other.display()),
        "applied registry removal preserves unrelated roots"
    );
    assert!(crate::telemetry::settle(std::time::Duration::from_secs(30)));
    db.execute_batch("DROP TRIGGER refuse_uninstall_receipts")
        .unwrap();
    drop(db);
    let observed = receipt(&install, "drain-project").unwrap().unwrap();
    assert_eq!(observed["state"], "observed");
    assert_eq!(observed["received_receipt"], received["received_receipt"]);
    assert_eq!(
        submit(&install, &request).unwrap(),
        observed,
        "same ID returns the original terminal receipt"
    );
    assert!(capture.flush().unwrap_err().contains("injected"));
    drop(guard);
    drop(capture);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn project_uninstall_refuses_foreign_scope_without_registry_effect() {
    let (dir, install, root, other) = fixture_with_other();
    let request = uninstall_review(&install, "foreign-project", Some(&root)).unwrap();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    submit(&install, &request).unwrap();
    let outsider = dir.join("outsider");
    fs::create_dir_all(&outsider).unwrap();
    fs::write(
        outsider.join("taskq.json"),
        r#"{"repo":"fixture/outsider"}"#,
    )
    .unwrap();
    let changed = format!(
        "{}\n{}\n{}\n",
        root.display(),
        other.display(),
        outsider.display()
    );
    fs::write(install.join("projects"), &changed).unwrap();
    assert!(
        finish_project_uninstall(&install, "foreign-project", "original", true)
            .unwrap_err()
            .contains("scope changed")
    );
    assert_eq!(
        fs::read_to_string(install.join("projects")).unwrap(),
        changed
    );
    capture.flush().unwrap();
    drop(guard);
    drop(capture);
    fs::remove_dir_all(dir).unwrap();
}

// H-A5: Stop and both native Uninstall scopes retain the original receipt across owner exit and replay.
#[test]
fn stop_keeps_original_receipts_and_requires_attributable_native_exit() {
    use std::{
        process::{Child, Command, Stdio},
        time::Duration,
    };
    struct Owner(Child);
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for mode in ["stop", "degraded", "project", "host"] {
        let (dir, install, _) = if mode == "host" {
            let (d, i, r, _) = fixture_with_other();
            (d, i, r)
        } else {
            fixture()
        };
        fs::remove_file(install.join("service")).unwrap();
        let registry = fs::read(install.join("projects")).unwrap();
        let ready = dir.join("prepared");
        let mut child = Owner(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "lifecycle::tests::stop_owner_probe",
                    "--ignored",
                    "--nocapture",
                ])
                .env("TASKQ_STOP_FIXTURE", &install)
                .env("TASKQ_STOP_MODE", mode)
                .env("TASKQ_STOP_READY", &ready)
                .env("HOME", install.join("home"))
                .env_remove("TASKQ_ATTEMPT")
                .env_remove("COREQ_TRACE_CONTEXT")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let until = Instant::now() + Duration::from_secs(10);
        while !ready.is_file() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "Stop owner fixture exited before preparation"
            );
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
        let collecting = (mode != "degraded").then(|| crate::profile::tests::collecting(&install));
        let original = receipt(&install, "exact-stop").unwrap().unwrap();
        assert_eq!(
            original["action"],
            if mode == "stop" || mode == "degraded" {
                "stop"
            } else {
                "uninstall"
            }
        );
        assert_eq!(original["state"], "applied");
        controller_exited(
            &install,
            "exact-stop",
            "controller exited before recording its cause",
        )
        .unwrap();
        let fallback = receipt(&install, "exact-stop").unwrap().unwrap();
        assert!(
            fallback["reason"]
                .as_str()
                .unwrap()
                .contains("controller exited before recording its cause")
        );
        assert!(
            fallback["reason"]
                .as_str()
                .unwrap()
                .contains("--request exact-stop")
        );
        if mode == "stop" || mode == "degraded" {
            assert!(
                original["restart_condition"]
                    .as_str()
                    .unwrap()
                    .contains("service install")
            );
            assert_eq!(
                original["restart_condition"]
                    .as_str()
                    .unwrap()
                    .contains("logon"),
                !cfg!(target_os = "linux")
            );
            if cfg!(target_os = "macos") {
                assert!(
                    original["restart_condition"]
                        .as_str()
                        .unwrap()
                        .contains("admission remains held until reviewed recovery (#1285)")
                );
            }
        }
        let native = coreq::process::identity(child.0.id()).unwrap().unwrap();
        assert!(
            saved_launcher(&install, "exact-stop", &native)
                .unwrap()
                .is_none()
        );
        let launcher = json!({"pid":native.pid,"started":"1234567890","instance":"{01234567-89ab-cdef-0123-456789abcdef}"});
        let mut invalid = launcher.clone();
        invalid["instance"] = json!("{{01234567-89ab-cdef-0123-456789abcdef}}");
        assert!(save_launcher(&install, "exact-stop", &native, &invalid).is_err());
        assert!(
            saved_launcher(&install, "exact-stop", &native)
                .unwrap()
                .is_none()
        );
        save_launcher(&install, "exact-stop", &native, &launcher).unwrap();
        save_launcher(&install, "exact-stop", &native, &launcher).unwrap();
        let mut replacement = launcher.clone();
        replacement["started"] = json!("1234567891");
        assert!(save_launcher(&install, "exact-stop", &native, &replacement).is_err());
        let foreign = coreq::process::Identity {
            pid: native.pid,
            started: format!("{}-foreign", native.started),
        };
        assert!(saved_launcher(&install, "exact-stop", &foreign).is_err());
        assert!(save_launcher(&install, "exact-stop", &foreign, &launcher).is_err());
        assert_eq!(
            saved_launcher(&install, "exact-stop", &native).unwrap(),
            Some(launcher)
        );
        #[cfg(target_os = "macos")]
        {
            // The test harness refuses service CLI args; its failed controller must still be owned and reaped.
            let mut control =
                crate::host_control::dispatch(&install, &pending(&install).unwrap().unwrap())
                    .unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            let result = loop {
                if let Some(result) = control.poll() {
                    break result;
                }
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(10));
            };
            assert!(result.is_err());
            assert!(
                control.poll().is_none(),
                "controller completion is delivered once"
            );
        }
        assert!(
            complete_stop_with(&install, "exact-stop", |_, _| Err(
                "native effect failed".into()
            ))
            .unwrap_err()
            .contains("native effect failed")
        );
        failed(&install, "exact-stop", "native host path scope differs").unwrap();
        let failure = receipt(&install, "exact-stop").unwrap().unwrap()["reason"].clone();
        assert!(
            failure
                .as_str()
                .unwrap()
                .contains("native host path scope differs"),
            "a new concrete error replaces the earlier fallback"
        );
        controller_exited(&install, "exact-stop", "process exited with exit status: 1").unwrap();
        assert_eq!(
            receipt(&install, "exact-stop").unwrap().unwrap()["reason"],
            failure,
            "outer controller exit cannot hide its retained inner cause"
        );
        let origin = pending(&install).unwrap().unwrap().origin().to_string();
        handoff_wait(&install, &origin, Some(HandoffWait::Maintenance), "").unwrap();
        assert_eq!(
            receipt(&install, "exact-stop").unwrap().unwrap()["reason"],
            failure,
            "background maintenance cannot hide a prepared controller failure"
        );
        assert_eq!(fs::read(install.join("projects")).unwrap(), registry);
        assert!(
            complete_stop_with(&install, "exact-stop", |_, _| Ok(()))
                .unwrap_err()
                .contains("native exit")
        );
        assert_eq!(fs::read(install.join("projects")).unwrap(), registry);
        assert_eq!(status(&install, None)["state"], "applied");
        let lock = coreq::file_lock::try_lock(&install.join("projects.lock")).unwrap();
        assert!(
            complete_stop_with(&install, "exact-stop", |_, _| panic!(
                "busy registry cannot stop a native owner"
            ))
            .is_err()
        );
        drop(lock);
        observe(&install, "replacement", true).unwrap();
        assert_eq!(
            status(&install, None)["state"],
            "applied",
            "Restart observer cannot complete Stop"
        );
        let final_receipt = complete_stop_with(&install, "exact-stop", |native, revision| {
            assert_eq!(native.pid, child.0.id());
            assert_eq!(revision, original["unit_revision"].as_str().unwrap());
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            Ok(())
        })
        .unwrap();
        if mode == "degraded" {
            assert_eq!(final_receipt["telemetry"], "unknown");
            for phase in ["received", "applied", "observed"] {
                assert!(final_receipt[format!("{phase}_receipt")].is_null());
            }
        }
        assert_eq!(final_receipt["state"], "observed");
        assert_eq!(final_receipt["native_exit"], true);
        assert_eq!(
            final_receipt["received_receipt"],
            original["received_receipt"]
        );
        assert!(
            final_receipt["reason"].is_null(),
            "observed native completion clears the prior failed effect"
        );
        assert!(
            final_receipt.get("service_instance").is_none(),
            "native exit cannot claim a new service generation"
        );
        assert_eq!(
            fs::read(install.join("projects")).unwrap(),
            if mode == "project" {
                Vec::new()
            } else {
                registry
            }
        );
        if mode != "stop" && mode != "degraded" {
            assert_eq!(final_receipt["unit_removed"], true);
            if mode == "project" {
                assert_eq!(final_receipt["unregistered"], true);
            }
        }
        assert_eq!(
            complete_stop_with(&install, "exact-stop", |_, _| panic!(
                "observed Stop must never repeat the native effect"
            ))
            .unwrap(),
            final_receipt
        );
        if let Some((capture, guard)) = collecting {
            capture.flush().unwrap();
            drop(guard);
            drop(capture);
        }
        drop(child);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
#[ignore = "owned native service/receipt fixture; no OS service registration"]
fn stop_owner_probe() {
    use std::io::Read;
    let install = PathBuf::from(std::env::var_os("TASKQ_STOP_FIXTURE").unwrap());
    let _owner = coreq::file_lock::try_lock(&install.join("service.lock")).unwrap();
    publish(&install, "original");
    let unit = if cfg!(windows) {
        install.join("TaskQ.xml")
    } else {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(home, install.join("home"));
        home.join(if cfg!(target_os = "macos") {
            "Library/LaunchAgents/com.taskq.service.plist"
        } else {
            ".config/systemd/user/taskq.service"
        })
    };
    assert!(
        stop_review(&install, "before-unit").is_err(),
        "missing host unit cannot accept Stop"
    );
    fs::create_dir_all(unit.parent().unwrap()).unwrap();
    fs::write(&unit, "owned fixture unit").unwrap();
    let mode = std::env::var("TASKQ_STOP_MODE").unwrap_or_else(|_| "stop".into());
    let collecting = (mode != "degraded").then(|| crate::profile::tests::collecting(&install));
    if mode == "degraded" {
        let mut old: Value =
            serde_json::from_slice(&fs::read(install.join("service")).unwrap()).unwrap();
        old["stop_protocol"] = json!(1);
        fs::write(install.join("service"), old.to_string()).unwrap();
        let request = stop_review(&install, "old-service").unwrap();
        assert!(
            submit(&install, &request)
                .unwrap_err()
                .contains("upgrade the service")
        );
        assert!(
            pending(&install).unwrap().is_none(),
            "unsupported peer must not receive a degraded record"
        );
        publish(&install, "original");
    }
    let request = if mode == "stop" || mode == "degraded" {
        stop_review(&install, "exact-stop")
    } else {
        let root = scope(&install).unwrap().remove(0);
        uninstall_review(
            &install,
            "exact-stop",
            if mode == "project" { Some(&root) } else { None },
        )
    }
    .unwrap();
    let queued = submit(&install, &request).unwrap();
    assert_eq!(queued["state"], "queued");
    assert_eq!(submit(&install, &request).unwrap(), queued);
    if mode == "host" {
        assert_queued_uninstall_wait_tracks_ownership(&install, scope(&install).unwrap().remove(0));
    }
    let mut changed = request.clone();
    changed["action"] = json!("restart");
    assert!(
        submit(&install, &changed).is_err(),
        "same ID cannot change lifecycle action"
    );
    assert!(
        complete_stop_with(&install, "exact-stop", |_, _| panic!(
            "unprepared Stop cannot have an effect"
        ))
        .is_err()
    );
    prepare_with(
        &install,
        "original",
        Some(&crate::host_control::revision(&install).unwrap()),
    )
    .unwrap();
    if let Some((capture, guard)) = collecting {
        capture.flush().unwrap();
        drop(guard);
        drop(capture);
    }
    fs::write(
        PathBuf::from(std::env::var_os("TASKQ_STOP_READY").unwrap()),
        "prepared",
    )
    .unwrap();
    let _ = std::io::stdin().read(&mut [0]);
}

// H-A5: maintenance progress must not hide the unresolved execution blocking this exact Uninstall.
fn assert_queued_uninstall_wait_tracks_ownership(install: &Path, root: PathBuf) {
    let mut h = host::Host::new(Err("fixture policy unused".into()));
    h.restart = true;
    h.stopping = true;
    h.busy.insert(root.clone(), false);
    h.prepared.insert(root.clone());
    h.updating = Some(std::thread::spawn(|| Ok(host::Maintenance::Update(None))));
    handoff_wait(install, "original", h.handoff_wait(), "").unwrap();
    assert_eq!(
        receipt(install, "exact-stop").unwrap().unwrap()["reason"],
        HandoffWait::Maintenance.reason()
    );
    h.sync(&root, vec![(1, 3, "codex".into())]);
    for _ in 0..2 {
        handoff_wait(install, "original", h.handoff_wait(), "").unwrap();
        let r = receipt(install, "exact-stop").unwrap().unwrap();
        assert_eq!(r["state"], "queued");
        assert_eq!(r["reason"], HandoffWait::Execution.reason());
        assert!(!h.handoff_ready());
        assert!(h.stop_controller.is_none());
    }
    h.updating.take().unwrap().join().unwrap().unwrap();
    handoff_wait(install, "original", h.handoff_wait(), "").unwrap();
    assert_eq!(
        receipt(install, "exact-stop").unwrap().unwrap()["reason"],
        HandoffWait::Execution.reason()
    );
    h.sync(&root, vec![]);
    h.rebuilding = true;
    assert_eq!(h.handoff_wait(), Some(HandoffWait::Recovery));
    h.rebuilding = false;
    h.prepared.clear();
    assert_eq!(h.handoff_wait(), Some(HandoffWait::Project));
    h.prepared.insert(root.clone());
    assert!(h.handoff_ready());
    assert_eq!(h.handoff_wait(), None);
    handoff_wait(install, "original", h.handoff_wait(), "").unwrap();
    assert!(
        receipt(install, "exact-stop").unwrap().unwrap()["reason"].is_null(),
        "ready handoff clears its own provisional reason"
    );
    failed(install, "exact-stop", "native scope denied").unwrap();
    let failure = receipt(install, "exact-stop").unwrap().unwrap()["reason"].clone();
    for wait in [
        Some(HandoffWait::Execution),
        Some(HandoffWait::Recovery),
        Some(HandoffWait::Project),
        Some(HandoffWait::Maintenance),
        None,
    ] {
        handoff_wait(install, "original", wait, "").unwrap();
        assert_eq!(
            receipt(install, "exact-stop").unwrap().unwrap()["reason"],
            failure
        );
    }
    h.stopping = false;
    h.sync(&root, vec![(1, 3, "codex".into())]);
    assert!(h.handoff_ready());
    assert_eq!(
        h.handoff_wait(),
        None,
        "non-stopping reexec preserves prepared live CLI work"
    );
    // The shared native-controller fixture continues with this same queued request.
}

// H-A5: one durable request -> qualified preparation -> new owner recovery -> the original receipt.
#[test]
fn restart_replay_scope_and_exact_generation() {
    let (dir, install, root) = fixture();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let request = review(&install, "exact-restart").unwrap();
    let received = submit(&install, &request).unwrap();
    assert_eq!(received["state"], "queued");
    let bytes = fs::read(resources::dir(&install).join("state.json")).unwrap();
    assert_eq!(submit(&install, &request).unwrap(), received);
    assert_eq!(
        fs::read(resources::dir(&install).join("state.json")).unwrap(),
        bytes
    );
    let mut changed = request.clone();
    changed["instance"] = json!("other");
    assert!(submit(&install, &changed).is_err());
    changed["id"] = json!("another");
    assert!(submit(&install, &changed).is_err());
    assert!(host::ProjectEdit::new(&install, &root, false).is_err());
    let original = fs::read(install.join("projects")).unwrap();
    fs::write(install.join("projects"), "").unwrap();
    assert!(prepare(&install, "original").is_err());
    assert_eq!(
        receipt(&install, "exact-restart").unwrap().unwrap()["state"],
        "queued"
    );
    fs::write(install.join("projects"), original).unwrap();
    failed(&install, "exact-restart", "native definition unavailable").unwrap();
    assert!(
        receipt(&install, "exact-restart").unwrap().unwrap()["reason"]
            .as_str()
            .unwrap()
            .contains("retry service restart --request exact-restart")
    );
    let failure = receipt(&install, "exact-restart").unwrap().unwrap()["reason"].clone();
    handoff_wait(&install, "original", Some(HandoffWait::Maintenance), "").unwrap();
    assert_eq!(
        receipt(&install, "exact-restart").unwrap().unwrap()["reason"],
        failure,
        "queued failure survives a background progress tick"
    );
    prepare(&install, "original").unwrap();
    assert_eq!(
        receipt(&install, "exact-restart").unwrap().unwrap()["state"],
        "applied"
    );
    assert!(
        receipt(&install, "exact-restart").unwrap().unwrap()["reason"].is_null(),
        "qualified preparation clears the resolved queued failure"
    );
    observe(&install, "original", true).unwrap();
    assert_eq!(status(&install, None)["state"], "applied");
    publish(&install, "replacement");
    observe(&install, "replacement", false).unwrap();
    assert_eq!(status(&install, None)["state"], "applied");
    assert!(
        prepare(&install, "replacement").is_err(),
        "an already replaced origin must never trigger a second switch"
    );
    observe(&install, "replacement", true).unwrap();
    let completed = receipt(&install, "exact-restart").unwrap().unwrap();
    assert_eq!(completed["state"], "observed");
    assert_eq!(completed["service_instance"], "replacement");
    assert_eq!(completed["received_receipt"], received["received_receipt"]);
    assert_eq!(
        submit(&install, &request).unwrap(),
        completed,
        "same-ID recovery cannot create another command or effect"
    );
    drop(host::ProjectEdit::new(&install, &root, false).unwrap());
    capture.flush().unwrap();
    drop(guard);
    drop(capture);
    fs::remove_dir_all(dir).unwrap();
}

// H-A5 trust boundary: no receipt, stale review or unsupported protocol cannot create restart intent.
#[test]
fn restart_refuses_unqualified_or_unrecordable_requests() {
    let (dir, install, _) = fixture();
    let request = review(&install, "no-collector").unwrap();
    let dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    // #1336: without a collector Restart falls back to the untracked restart file, never to receipt intent.
    let untracked =
        tracing::dispatcher::with_default(&dispatch, || submit(&install, &request)).unwrap();
    assert_eq!(
        (&untracked["state"], &untracked["telemetry"]),
        (&json!("untracked"), &json!("unknown"))
    );
    assert!(install.join("restart").exists());
    fs::remove_file(install.join("restart")).unwrap();
    assert!(pending(&install).unwrap().is_none());
    let (capture, guard) = crate::profile::tests::collecting(&install);
    for case in [
        "old-protocol",
        "stale-instance",
        "expired",
        "foreign-root",
        "extra",
        "oversized",
        "bad-id",
    ] {
        let mut input = request.clone();
        input["id"] = json!(case);
        match case {
            "old-protocol" => {
                let mut record: Value =
                    serde_json::from_slice(&fs::read(install.join("service")).unwrap()).unwrap();
                record.as_object_mut().unwrap().remove("restart_protocol");
                fs::write(install.join("service"), record.to_string()).unwrap();
            }
            "stale-instance" => input["instance"] = json!("another"),
            "expired" => {
                input["issued_at"] = json!(now().unwrap() - 301);
                input["expires_at"] = json!(now().unwrap() - 1);
            }
            "foreign-root" => input["roots"] = json!([dir.canonicalize().unwrap()]),
            "extra" => input["force"] = json!(true),
            "oversized" => input["instance"] = json!("x".repeat(20 << 10)),
            "bad-id" => input["id"] = json!("bad/id"),
            _ => unreachable!(),
        }
        assert!(submit(&install, &input).is_err(), "{case}");
        assert!(pending(&install).unwrap().is_none());
        publish(&install, "original");
    }
    let db = rusqlite::Connection::open(install.join("telemetry.log/universal.sqlite3")).unwrap();
    // #1351: an unrecordable received receipt stays unknown; the reviewed request is still accepted.
    db.execute_batch("CREATE TRIGGER refuse_restart_receipt BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert_eq!(submit(&install, &request).unwrap()["state"], "queued");
    assert!(pending(&install).unwrap().is_some());
    assert!(crate::telemetry::settle(std::time::Duration::from_secs(30)));
    db.execute_batch("DROP TRIGGER refuse_restart_receipt")
        .unwrap();
    drop(db);
    assert!(capture.flush().unwrap_err().contains("injected"));
    drop(guard);
    drop(capture);
    fs::remove_dir_all(dir).unwrap();
}

// Readback/retention must preserve a long drain, refuse corruption and recover an observer death.
#[test]
fn restart_pending_survives_retention_and_receipt_failure() {
    let (dir, install, _) = fixture();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let input = review(&install, "long-restart").unwrap();
    submit(&install, &input).unwrap();
    let held = coreq::file_lock::try_lock(&resources::dir(&install).join("lock")).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let observed = install.clone();
    let reader = std::thread::spawn(move || send.send(pending(&observed).map(|_| ())).unwrap());
    let result = receive.recv_timeout(std::time::Duration::from_secs(2));
    drop(held);
    reader.join().unwrap();
    assert!(
        result
            .unwrap()
            .unwrap_err()
            .starts_with("resources lock unavailable:"),
        "host observer waited behind a policy writer"
    );
    resources::with(&install, |state| {
        resources::prepare(state, &install, now()? + 3 * 86400)
    })
    .unwrap();
    assert!(pending(&install).unwrap().is_some());
    prepare(&install, "original").unwrap();
    publish(&install, "new-owner");
    let db = rusqlite::Connection::open(install.join("telemetry.log/universal.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER refuse_restart_observation BEFORE INSERT ON events WHEN NEW.kind='command.observed' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    // #1351: a refused observed commit stays unknown in the host log; observation proceeds.
    observe(&install, "new-owner", true).unwrap();
    assert_eq!(status(&install, None)["state"], "observed");
    assert!(status(&install, None)["reason"].is_null());
    assert!(crate::telemetry::settle(std::time::Duration::from_secs(30)));
    db.execute_batch("DROP TRIGGER refuse_restart_observation")
        .unwrap();
    drop(db);
    let path = resources::dir(&install).join("state.json");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, "{").unwrap();
    assert_eq!(status(&install, None)["state"], "unknown");
    assert!(pending(&install).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{");
    fs::write(&path, bytes).unwrap();
    fs::write(
        resources::dir(&install).join("state.json.pending"),
        "interrupted",
    )
    .unwrap();
    assert!(pending(&install).is_err());
    fs::remove_file(resources::dir(&install).join("state.json.pending")).unwrap();
    resources::with(&install, |state| {
        resources::prepare(state, &install, now()? + 3 * 86400)
    })
    .unwrap();
    assert!(receipt(&install, "long-restart").unwrap().is_none());
    assert!(capture.flush().unwrap_err().contains("injected"));
    drop(guard);
    drop(capture);
    fs::remove_dir_all(dir).unwrap();
}
