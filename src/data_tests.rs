use super::*;
use serde_json::json;

fn temp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("taskq-data-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join(".taskq/incidents")).unwrap();
    d
}

// Chain: service pass, prune. Risk: a growing file fills the disk or a live attempt loses its log
#[test]
fn every_growing_file_has_a_bound() {
    let root = temp("prune");
    let install = root.join("install");
    fs::create_dir_all(&install).unwrap();
    let d = crate::paths::dot(&root);
    let line = "x".repeat(99) + "\n";
    fs::write(
        install.join("taskq.log"),
        line.repeat((LOG_MAX / 100 + 10) as usize),
    )
    .unwrap();
    fs::write(install.join("p.log"), "small\n").unwrap();
    fs::write(
        d.join("cli-errors.log"),
        line.repeat((CLI_ERRORS_MAX / 100 + 10) as usize),
    )
    .unwrap();
    for a in 0..ATTEMPT_LOGS as u64 + 5 {
        fs::write(d.join(format!("T7-{a}.log")), "").unwrap();
        fs::write(d.join(format!("T7-{a}.headroom")), "{}").unwrap();
        fs::write(d.join("incidents").join(format!("{a}.json")), "{}").unwrap();
        // mtimes order the files: 0 is the oldest
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + a);
        fs::File::options()
            .write(true)
            .open(d.join(format!("T7-{a}.log")))
            .unwrap()
            .set_modified(t)
            .unwrap();
        fs::File::options()
            .write(true)
            .open(d.join("incidents").join(format!("{a}.json")))
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    fs::write(d.join("status.json"), "{}").unwrap();
    let out = prune(&root, &install, "p", &[0]);
    assert_eq!(out.len(), 4, "{out:?}");
    let big = fs::metadata(install.join("taskq.log")).unwrap().len();
    assert!(
        big <= LOG_MAX / 2
            && big > LOG_MAX / 2 - 200
            && fs::read(install.join("taskq.log"))
                .unwrap()
                .starts_with(b"xxx")
    );
    assert_eq!(
        fs::read_to_string(install.join("p.log")).unwrap(),
        "small\n"
    );
    assert!(fs::metadata(d.join("cli-errors.log")).unwrap().len() <= CLI_ERRORS_MAX / 2);
    // live attempt 0 stays though oldest; 1..=4 go
    assert!(
        d.join("T7-0.log").exists()
            && !d.join("T7-1.log").exists()
            && !d.join("T7-4.log").exists()
            && d.join("T7-5.log").exists()
    );
    assert!(
        d.join("T7-0.headroom").exists()
            && !d.join("T7-1.headroom").exists()
            && d.join("T7-5.headroom").exists()
    );
    assert_eq!(
        fs::read_dir(d.join("incidents")).unwrap().count(),
        INCIDENTS
    );
    assert!(d.join("status.json").exists());
    assert!(
        prune(&root, &install, "p", &[0]).is_empty(),
        "a second round finds nothing"
    );
    let _ = fs::remove_dir_all(&root);
}

// Chain: §4 hourly input-cache pruning. Risk: unbounded briefs or deletion during a live turn.
#[test]
fn input_cache_is_bounded_and_live_files_stay() {
    let root = temp("inputs");
    let dir = root.join(".taskq/inputs");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..55 {
        let path = dir.join(format!("{i}.txt"));
        fs::write(&path, "invented brief").unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i))
            .unwrap();
    }
    let active = fs::File::options()
        .read(true)
        .write(true)
        .open(dir.join("0.txt"))
        .unwrap();
    let active = coreq::file_lock::try_lock_shared_file(active).unwrap();
    fs::write(root.join(".taskq/T7-42.log"),format!("{}\n",serde_json::json!({"type":"taskq.input","task":7,"attempt":42,"execution":44,"runtime":"claude","path":dir.join("1.txt")}))).unwrap();
    prune(&root, &root.join("install"), "p", &[42]);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 52);
    assert!(dir.join("0.txt").exists());
    assert!(
        dir.join("1.txt").exists(),
        "restart loses the lock, not the pending input binding"
    );
    assert!(!dir.join("2.txt").exists());
    drop(active);
    prune(&root, &root.join("install"), "p", &[42]);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 51);
    assert!(dir.join("1.txt").exists());
    let log = root.join(".taskq/T7-42.log");
    for bad in [
        "{\"type\":\"taskq.input\"".to_owned(),
        serde_json::json!({"type":"taskq.input","attempt":42,"execution":41}).to_string(),
        "x".repeat(8 * 1024 * 1024 + 1),
    ] {
        fs::write(&log, bad).unwrap();
        assert!(
            prune(&root, &root.join("install"), "p", &[42])
                .iter()
                .any(|s| s.starts_with("input retention unknown:"))
        );
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            51,
            "uncertain recovery evidence must preserve the unlocked input"
        );
    }
    fs::remove_file(log).unwrap();
    assert!(
        prune(&root, &root.join("install"), "p", &[42])
            .iter()
            .any(|s| s.contains("retained attempt log missing"))
    );
    assert!(dir.join("1.txt").exists());
    prune(&root, &root.join("install"), "p", &[]);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 50);
    fs::remove_dir_all(root).unwrap();
}

// Chain: §4 input pruning after a turn ends. Risk: a pre-exec child retains the turn's lock.
#[cfg(unix)]
#[test]
fn input_cache_unlocks_while_fork_child_lives() {
    let root = temp("inputs-fork");
    let path = root.join("input.txt");
    let file = fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let held = coreq::file_lock::try_lock_shared_file(file).unwrap();
    let mut pipe = [0; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // Only async-signal-safe calls after fork; wait until the parent finishes probing.
        unsafe {
            libc::close(pipe[1]);
            let mut byte = 0u8;
            libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
    }
    unsafe {
        libc::close(pipe[0]);
    }
    let probe = fs::File::options()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let before = probe.try_lock();
    drop(held);
    let after = probe.try_lock();
    unsafe {
        libc::write(pipe[1], b"x".as_ptr().cast(), 1);
        libc::close(pipe[1]);
    }
    let mut status = 0;
    assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
    assert_eq!(status, 0);
    assert!(matches!(before, Err(std::fs::TryLockError::WouldBlock)));
    after.unwrap();
    fs::remove_dir_all(root).unwrap();
}

// Chain: status.json, tray.json, kids, focus readers. Risk: corrupt state reads as zero or default and is reset
#[test]
fn corrupt_state_reads_unknown_and_stays() {
    let root = temp("corrupt");
    let d = crate::paths::dot(&root);
    assert_eq!(json(&d.join("status.json")), Ok(None));
    fs::write(d.join("status.json"), "{\"tasks\": [").unwrap();
    assert!(
        json(&d.join("status.json"))
            .unwrap_err()
            .starts_with("status.json: corrupt")
    );
    fs::write(d.join("status.json"), "[]").unwrap();
    assert!(
        json(&d.join("status.json"))
            .unwrap_err()
            .contains("not a JSON object")
    );
    assert_eq!(json(&d.join("status.json")).ok(), None);
    assert!(d.join("status.json").exists());
    assert_eq!(focus(&root), Ok(vec![]));
    fs::write(d.join("focus"), "area-x off p0\n").unwrap();
    assert_eq!(focus(&root), Ok(vec!["area-x".to_string(), "p0".into()]));
    for (bad, why) in [
        (&b"\xff\xfe"[..], "not UTF-8"),
        (b"area\0x", "control"),
        (&[b'a'; 5000][..], "over"),
    ] {
        fs::write(d.join("focus"), bad).unwrap();
        assert!(focus(&root).unwrap_err().contains(why));
        assert_eq!(fs::read(d.join("focus")).unwrap(), bad);
    }
    let owner = coreq::process::Identity {
        pid: 9,
        started: "service".into(),
    };
    let row = KidLine {
        control: None,
        task: 7,
        attempt: 41,
        runtime: "codex".into(),
        process: coreq::process::Identity {
            pid: 123,
            started: "child".into(),
        },
        execution: 43,
        offset: 40,
        job: None,
    };
    assert_eq!(
        kids("pid 9 service executor-v1\n7 41 codex 123 child 43 40\n"),
        Ok((owner.clone(), vec![row.clone()]))
    );
    let key = "11111111-1111-4111-8111-111111111111";
    let scoped =
        format!("pid 9 service executor-v1\n7 41 codex 123 child 43 40 job-global:{key}\n");
    let mut with_job = row.clone();
    with_job.job = Some(format!("job-global:{key}"));
    assert_eq!(kids(&scoped), Ok((owner.clone(), vec![with_job.clone()])));
    with_job.job = Some(format!("job-kill:{key}"));
    assert_eq!(
        kids(&format!(
            "pid 9 service executor-v1\n7 41 codex 123 child 43 40 job-kill:{key}\n"
        )),
        Ok((owner.clone(), vec![with_job]))
    );
    // #2266 A2: only a kill-on-close scope turns a vanished job name into exit; an older scope stays unknown.
    assert_eq!(missing_scope_exit(&format!("job-kill:{key}")), Ok(()));
    assert!(
        missing_scope_exit(&format!("job-global:{key}"))
            .unwrap_err()
            .contains("native reconciliation required")
    );
    assert!(missing_scope_exit("job-kill:../foreign").is_err());
    for scope in [
        "job-global:",
        "job-kill:",
        "job-global:../foreign",
        "job-global:11111111-1111-4111-8111-11111111111G",
        "job:11111111-1111-4111-8111-111111111111",
    ] {
        assert!(
            kids(&format!(
                "pid 9 service executor-v1\n7 41 codex 123 child 43 40 {scope}\n"
            ))
            .is_err()
        );
    }
    assert!(
        kids(&(scoped + &format!("8 42 codex 124 other 44 0 job-global:{key}\n"))).is_err(),
        "one native scope cannot own two tasks"
    );
    assert!(
        kids(&format!(
            "pid 9 service executor-v1\n7 41 claude 123 child 43 0 job-global:{key}\n"
        ))
        .is_err()
    );
    assert!(
        kids("pid 9\n7 41 codex 123 40\n").is_err(),
        "legacy cursors never identify an execution"
    );
    assert_eq!(kids("pid 9 service executor-v1\n"), Ok((owner, vec![])));
    assert!(
        kids("pid 9 service\n7 41 codex 123 child 43 40\n")
            .unwrap_err()
            .contains("legacy launcher"),
        "a historical wrapper PID is not native exit proof"
    );
    for row in [
        "7 41 codex 9 service 43 40",
        "7 41 codex 123 child 40 0",
        "7 41 codex 123 ../child 43 0",
        "7 41 claude 123 child 43 1",
        "7 41 codex-app 123 child 43 0",
    ] {
        assert!(
            kids(&format!("pid 9 service executor-v1\n{row}\n")).is_err(),
            "{row}"
        );
    }
    assert!(
        kids(
            "pid 9 service executor-v1\n7 41 codex 123 child 43 40\n7 41 codex 124 child2 44 40\n"
        )
        .is_err()
    );
    assert!(
        kids(&"x".repeat(KIDS_MAX as usize + 1))
            .unwrap_err()
            .contains("1 MiB")
    );
    assert!(kids("pid x\n").unwrap_err().contains("identity"));
    assert!(
        kids("pid 9 service executor-v1\n7 41 claude\n")
            .unwrap_err()
            .contains("line 2")
    );
    assert_eq!(json(&root.join("missing")), Ok(None));
    assert_eq!(
        json(&{
            fs::write(d.join("t.json"), json!({"a": 1}).to_string()).unwrap();
            d.join("t.json")
        }),
        Ok(Some(json!({"a": 1})))
    );
    let _ = fs::remove_dir_all(&root);
}

// R9/R12: protected control evidence requires explicit admission/version; old absence never means fresh.
#[test]
fn background_control_proof_requires_version_and_admission() {
    use base64::Engine;
    let root =
        std::env::temp_dir().join(format!("taskq-control-record-{}", crate::runtime::uuid()));
    fs::create_dir_all(&root).unwrap();
    let project = root.canonicalize().unwrap();
    let proof = BackgroundControlProof {
        admission: BackgroundAdmission::Fresh,
        grant: crate::policy::Grant {
            runtime: "claude".into(),
            mode: "workspace".into(),
            project: Some(project.to_string_lossy().into()),
            by: "invented fixture".into(),
            at: 1,
        },
        controller: agentq_runtime::agent::NativeProcess {
            pid: 9,
            started: "controller".into(),
        },
        session_store: project.join("runtime-sessions"),
        lifetime: agentq_runtime::agent::Lifetime::Recoverable,
        project: project.clone(),
        home: project.join("home"),
        cwd: project,
        job: "c37eb75a".into(),
        native_session: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
        before: 17,
        input_sha256: "a".repeat(64),
        owner: serde_json::json!({"receipt":"invented"}),
        source: serde_json::json!({"checkpoint":"invented codec payload, never control proof"}),
    };
    let encode = |value: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(value).unwrap())
    };
    let payload = serde_json::to_value(&proof).unwrap();
    let mut missing_store = payload.clone();
    missing_store
        .as_object_mut()
        .unwrap()
        .remove("session_store");
    assert!(serde_json::from_value::<BackgroundControlProof>(missing_store).is_err());
    let row = format!(
        "7 41 claude 123 child 43 99 control-v1:{}\n",
        encode(&payload)
    );
    let text = format!("pid 9 controller executor-v2\n{row}");
    let parsed = kids(&text).unwrap().1;
    assert_eq!(parsed[0].control.as_ref(), Some(&proof));
    assert_eq!(parsed[0].offset, 99);
    assert!(kids(&text.replace("executor-v2", "executor-v1")).is_err());
    let mut missing = payload.clone();
    missing.as_object_mut().unwrap().remove("admission");
    assert!(
        kids(&format!(
            "pid 9 controller executor-v2\n7 41 claude 123 child 43 99 control-v1:{}\n",
            encode(&missing)
        ))
        .is_err()
    );
    assert!(
        kids("pid 9 controller executor-v1\n7 41 claude 123 child 43 0\n")
            .unwrap()
            .1[0]
            .control
            .is_none()
    );
    assert!(!text.contains("protected input body"));
    fs::remove_dir_all(root).unwrap();
}
