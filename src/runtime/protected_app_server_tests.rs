use super::*;
// Chain: R8 R9 R12 R13 #2151 A1/A2 — a launch receipt cannot create its own expected authority.
// Risk: a missing independent preparation is accepted before native dispatch.
#[test]
fn protected_unprepared_launch_refuses_before_intent() {
    let r = Request {
        schema: SCHEMA.into(),
        slot: json!({"binding":{"runtime":"codex"}}),
        policy_revision: 29,
        account: "default".into(),
        preparation: String::new(),
    };
    let id = coreq::process::Identity {
        pid: 42,
        started: "invented-creation".into(),
    };
    let mut s = resources::State::default();
    s.ops.insert(
        "slot".into(),
        resources::Op {
            bound: json!({"local_slot":{}}),
            receipt: json!({"occupied":true,"instance":"invented-host"}),
            expires_at: 1,
            saved_at: None,
            writing: false,
            sent: true,
        },
    );
    let before = s.clone();
    assert!(
        consume(&mut s, "slot", &r, &json!({"invented":"authority"}), &id).is_err(),
        "missing independent preparation must refuse before launch intent"
    );
    assert_eq!(s, before);
}
fn model() -> (resources::State, Request, Value, coreq::process::Identity) {
    let authority = json!({"project":"invented-project","saved":{"revision":29,"grant":{"by":"invented-operator","mode":"workspace"}},"account":"default","account_home":"invented-home","executable_sha256":"invented-native","profile_sha256":"invented-profile","adapter":"invented-adapter"});
    let mut r = Request {
        schema: SCHEMA.into(),
        slot: json!({"binding":{"caller":"invented-consumer","op_id":"invented-effect","runtime":"codex","pid":17,"started":"invented-consumer-start","boot":10}}),
        policy_revision: 29,
        account: "default".into(),
        preparation: String::new(),
    };
    let pre = preparation(&r, &authority, &json!("invented-host"));
    r.preparation = digest(pre.to_string().as_bytes());
    let mut s = resources::State::default();
    s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{"binding":r.slot["binding"],"key_hash":"invented-private-hash","prepared":pre}}),receipt:json!({"occupied":true,"instance":"invented-host"}),expires_at:1,saved_at:None,writing:false,sent:true});
    (
        s,
        r,
        authority,
        coreq::process::Identity {
            pid: 42,
            started: "invented-supervisor-start".into(),
        },
    )
}
// Chain: R8 R9 R12 R13 #2151 A1/A2/A5 — preparation is independent, immutable and bound to caller/artifacts/grant.
// Risk: prepare/launch drift or a copied launch receipt supplies its own authority.
#[test]
fn protected_preparation_model_drift_and_recovery() {
    let (s, r, authority, id) = model();
    for field in [
        "project",
        "saved",
        "account",
        "account_home",
        "executable_sha256",
        "profile_sha256",
        "adapter",
    ] {
        let mut changed = authority.clone();
        changed[field] = json!("changed-or-revoked");
        let mut trial = s.clone();
        let before = trial.clone();
        assert!(
            consume(&mut trial, "slot", &r, &changed, &id).is_err(),
            "{field}"
        );
        assert_eq!(trial, before);
    }
    for field in ["pid", "started", "boot", "op_id"] {
        let mut changed = r.clone();
        changed.slot["binding"][field] = json!("changed-incarnation");
        assert!(
            consume(&mut s.clone(), "slot", &changed, &authority, &id).is_err(),
            "{field}"
        );
    }
    let mut changed = s.clone();
    changed.ops.get_mut("slot").unwrap().receipt["instance"] = json!("new-host");
    assert!(consume(&mut changed, "slot", &r, &authority, &id).is_err());
    let mut started = s.clone();
    consume(&mut started, "slot", &r, &authority, &id).unwrap();
    let restored: resources::State =
        serde_json::from_str(&serde_json::to_string(&started).unwrap()).unwrap();
    for state in ["unknown", "verified-effective", "cancelled", "completed"] {
        let mut trial = restored.clone();
        trial.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["state"] = json!(state);
        let before = trial.clone();
        assert!(consume(&mut trial, "slot", &r, &authority, &id).is_err());
        assert_eq!(trial, before);
        assert!(release_ready(&trial.ops["slot"]).is_err());
    }
}
// Chain: R8 R9 R12 R13 #2151 A3/A4/A5 — cancellation/root exit is not descendant completion or slot release.
// Risk: crashes, PID reuse, reordered observations or a cleanup intent release uncertain work.
#[test]
fn protected_completion_model_survivors_crashes_and_no_replay() {
    let (mut s, r, a, id) = model();
    consume(&mut s, "slot", &r, &a, &id).unwrap();
    assert!(release_ready(&s.ops["slot"]).is_err());
    cancel(&mut s, "slot", &r).unwrap();
    assert!(release_ready(&s.ops["slot"]).is_err());
    let binding = json!({"boot":10,"coalition":101});
    let launch = &mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"];
    launch["native"] = json!({"pid":43,"started":"invented-native-start"});
    launch["input_closed"] = json!(true);
    launch["scope"] = json!({"binding":binding,"dispatch_ack":true,"cleanup_intent":false,"cleanup_verified":false});
    // Native root exited; two surviving detached descendants plus the sealed sentinel remain.
    assert!(!certify(&mut s, "slot", &r, &id, &binding, [9, 6]).unwrap());
    assert!(release_ready(&s.ops["slot"]).is_err());
    let before = s.clone();
    for (supervisor, binding, counts) in [
        (
            coreq::process::Identity {
                pid: 42,
                started: "reused-pid".into(),
            },
            binding.clone(),
            [9, 8],
        ),
        (id.clone(), json!({"boot":11,"coalition":101}), [9, 8]),
        (id.clone(), binding.clone(), [8, 8]),
        (id.clone(), binding.clone(), [9, 10]),
    ] {
        assert!(certify(&mut s, "slot", &r, &supervisor, &binding, counts).is_err());
        assert_eq!(s, before);
    }
    s = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert!(certify(&mut s, "slot", &r, &id, &binding, [9, 8]).unwrap());
    let mut late_input = s.clone();
    late_input.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["cancel_requested"] =
        json!(false);
    assert!(
        input_gate(&late_input, "slot", &r).is_err(),
        "a sealed input cannot reopen after cancellation changes"
    );
    let certified = s.clone();
    assert!(certify(&mut s, "slot", &r, &id, &binding, [9, 8]).unwrap());
    assert_eq!(s, certified);
    assert!(certify(&mut s, "slot", &r, &id, &binding, [9, 7]).is_err());
    assert_eq!(s, certified);
    assert!(certify(&mut s, "slot", &r, &id, &binding, [10, 9]).is_err());
    assert_eq!(s, certified);
    assert!(
        release_ready(&s.ops["slot"]).is_err(),
        "native certificate alone is not cleanup acknowledgment"
    );
    s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["cleanup_intent"] =
        json!(true);
    assert!(
        release_ready(&s.ops["slot"]).is_err(),
        "crash before/after bootout without acknowledgment stays occupied"
    );
    s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["bootout_ack"] =
        json!(true);
    assert!(
        release_ready(&s.ops["slot"]).is_err(),
        "acknowledged bootout still needs exact supervisor exit"
    );
    s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["cleanup_verified"] =
        json!(true);
    release_ready(&s.ops["slot"]).unwrap();
    assert_eq!(
        s.ops["slot"].bound["local_slot"]["binding"],
        r.slot["binding"]
    );
    assert_eq!(
        s.ops["slot"].bound["local_slot"]["key_hash"],
        "invented-private-hash"
    );
    assert!(consume(&mut s, "slot", &r, &a, &id).is_err());
    for field in ["native", "supervisor", "input_closed", "scope"] {
        let mut corrupt = s.clone();
        corrupt.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"][field] = Value::Null;
        assert!(release_ready(&corrupt.ops["slot"]).is_err(), "{field}");
    }
}
// Chain: R9 R12 #2179 A1 — a supervisor that sealed input before native spawn completes only on kernel no-spawn proof.
// Risk: an absent native identity is read as "nothing ran" (forged completion), or a true pre-native abort stays occupied forever.
#[test]
fn protected_pre_native_abort_certifies_only_on_unchanged_started_count() {
    let (mut s, r, a, id) = model();
    consume(&mut s, "slot", &r, &a, &id).unwrap();
    let binding = json!({"boot":10,"coalition":101});
    let launch = &mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"];
    launch["input_closed"] = json!(true);
    launch["scope"] = json!({"binding":binding,"dispatch_ack":true,"cleanup_intent":false,"cleanup_verified":false});
    let legacy = s.clone();
    s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["prelaunch_counts"] =
        json!([1, 0]);
    // Legacy launches without a baseline and any started task after the baseline (a spawned native whose identity was lost) stay unknown.
    assert!(certify(&mut legacy.clone(), "slot", &r, &id, &binding, [1, 0]).is_err());
    let before = s.clone();
    for counts in [[2, 1], [3, 2]] {
        assert!(certify(&mut s, "slot", &r, &id, &binding, counts).is_err());
        assert_eq!(s, before);
    }
    assert!(certify(&mut s, "slot", &r, &id, &binding, [1, 0]).unwrap());
    assert!(
        s.ops["slot"].bound["local_slot"]["launch"]["scope"]["certificate"]["native"].is_null()
    );
    for (field, value) in [
        ("native", json!({"pid":43,"started":"late"})),
        ("input_closed", json!(false)),
    ] {
        let mut forged = s.clone();
        forged.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"][field] = value;
        assert!(
            certified(&forged.ops["slot"].bound["local_slot"]["launch"]).is_err(),
            "{field}"
        );
    }
    let mut moved = s.clone();
    moved.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["prelaunch_counts"] =
        json!([2, 1]);
    assert!(certified(&moved.ops["slot"].bound["local_slot"]["launch"]).is_err());
    let scope = &mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"];
    for field in ["cleanup_intent", "bootout_ack", "cleanup_verified"] {
        scope[field] = json!(true);
    }
    release_ready(&s.ops["slot"]).unwrap();
}
// Chain: R8 R9 R12 #2151 A2/A5 — output descriptors cannot read input before authority validation.
// Risk: a duplex native stdout bypasses the supervisor's input gate.
#[cfg(unix)]
#[test]
fn protected_native_output_cannot_read_ungated_input() {
    use std::{
        io::Write,
        os::{fd::AsRawFd, unix::net::UnixStream},
        time::Duration,
    };
    let (mut consumer, mut supervisor) = UnixStream::pair().unwrap();
    consumer
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    supervisor
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    consumer.write_all(b"ungated native input").unwrap();
    let (output, pump) = native_output(supervisor.try_clone().unwrap()).unwrap();
    let mut output = fs::File::from(output);
    let mut bytes = [0u8; 20];
    let read = unsafe { libc::read(output.as_raw_fd(), bytes.as_mut_ptr().cast(), bytes.len()) };
    assert_eq!(
        read, -1,
        "native output must not read queued, ungated input"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EBADF)
    );
    supervisor.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"ungated native input");
    output.write_all(b"native output").unwrap();
    drop(output);
    let mut received = [0u8; 13];
    consumer.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"native output");
    assert!(pump.join().unwrap().unwrap());
}
// Chain: R9 R12 #2175 A1/A2 — frontend loss after verified-effective must not abort descendant completion.
// Risk: an output write error after the consumer disconnects ends the supervisor before its certificate, leaving the original slot unreconcilable.
#[cfg(unix)]
#[test]
fn protected_native_output_drains_after_frontend_loss() {
    use std::{io::Write, os::unix::net::UnixStream};
    let (consumer, supervisor) = UnixStream::pair().unwrap();
    let (output, pump) = native_output(supervisor).unwrap();
    drop(consumer);
    // More than any socket buffer: native output must never block or break once the frontend is gone.
    let mut output = fs::File::from(output);
    for _ in 0..64 {
        output.write_all(&[b'x'; 16384]).unwrap();
    }
    drop(output);
    assert!(
        !pump.join().unwrap().unwrap(),
        "lost frontend output is drained and reported undelivered"
    );
}
struct Fixture {
    dir: PathBuf,
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    r: Request,
    p: policy::Policy,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("taskq-protected-{}", runtime::uuid()));
        fs::create_dir(&dir).unwrap();
        let dir = canonical(&dir).unwrap();
        let root = dir.join("project");
        let home = dir.join("home");
        let bin = dir.join("bin/codex");
        fs::create_dir(bin.parent().unwrap()).unwrap();
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(&bin, b"invented native fixture").unwrap();
        fs::write(dir.join("taskq"), b"invented launcher").unwrap();
        fs::write(home.join(".codex/config.toml"), b"# invented config").unwrap();
        fs::write(dir.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(dir.join("host.json"),json!({"codex":["-s","danger-full-access","-m","invented-model","-c","model_reasoning_effort=high"]}).to_string()).unwrap();
        let p = policy::Policy {
            revision: 29,
            grants: vec![policy::Grant {
                runtime: "codex".into(),
                mode: "workspace".into(),
                project: Some(root.to_str().unwrap().into()),
                by: "invented-operator".into(),
                at: 1,
            }],
        };
        let r = Request {
            schema: SCHEMA.into(),
            slot: json!({"binding":{"root":root,"runtime":"codex","caller":"invented-consumer","op_id":"invented-operation"}}),
            policy_revision: 29,
            account: "default".into(),
            preparation: String::new(),
        };
        let f = Self {
            dir,
            root,
            home,
            bin,
            r,
            p,
        };
        f.save();
        f
    }
    fn save(&self) {
        fs::write(
            self.dir.join(policy::FILE),
            serde_json::to_vec(&self.p).unwrap(),
        )
        .unwrap();
    }
    fn plan(&self) -> Res<Plan> {
        resolve(&self.dir, &self.home, &self.r, "macos", &self.bin)
    }
}
// Chain: R8 R9 A1-A3: a request is a match requirement; missing, host-wide, widened or stale authority cannot launch.
// Risk: accepting a copied receipt, task text or default grant as protected project authority.
#[test]
fn protected_authority_refuses_missing_tampered_stale_and_unknown() {
    let f = Fixture::new();
    let p = f.plan().unwrap();
    assert_eq!(p.authority["saved"]["grant"]["by"], "invented-operator");
    assert_eq!(p.authority["account_home"], json!(f.home.join(".codex")));
    assert!(p.args.contains(&"model=\"invented-model\"".to_string()));
    assert!(p.args.contains(&"model_reasoning_effort=high".to_string()));
    assert!(
        p.access
            .profile
            .as_ref()
            .unwrap()
            .contains("deny file-write")
    );
    for case in [
        "missing",
        "corrupt",
        "full",
        "host",
        "revision",
        "approver",
        "duplicate",
        "account",
        "quota",
        "quota-shape",
        "path",
        "windows",
        "linux",
    ] {
        let mut f = Fixture::new();
        let os = match case {
            "windows" => "windows",
            "linux" => "linux",
            _ => "macos",
        };
        match case {
            "missing" => f.p.grants.clear(),
            "full" => f.p.grants[0].mode = "full".into(),
            "host" => f.p.grants[0].project = None,
            "revision" => f.r.policy_revision += 1,
            "approver" => f.p.grants[0].by = "unknown".into(),
            "duplicate" => f.p.grants.push(f.p.grants[0].clone()),
            "account" => f.r.account = "foreign".into(),
            "path" => f.r.slot["binding"]["root"] = json!(f.root.join("missing")),
            "quota" => {
                fs::write(f.dir.join(runtime::accounts::STATE), "unreadable limits").unwrap()
            }
            "quota-shape" => fs::write(f.dir.join(runtime::accounts::STATE), "[]").unwrap(),
            _ => {}
        }
        f.save();
        if case == "corrupt" {
            fs::write(f.dir.join(policy::FILE), "receipt text is not a grant").unwrap();
        }
        assert!(
            resolve(&f.dir, &f.home, &f.r, os, &f.bin).is_err(),
            "{case}"
        );
    }
    let mut f = Fixture::new();
    fs::write(f.dir.join("accounts.json"), "unreadable authority").unwrap();
    assert!(f.plan().is_err());
    fs::remove_file(f.dir.join("accounts.json")).unwrap();
    f.p.grants.clear();
    f.save();
    assert!(f.plan().is_err());
    let text=json!({"schema":SCHEMA,"slot":{"binding":{"runtime":"codex"}},"policy_revision":29,"account":"default","grant":"workspace"}).to_string();
    assert!(Request::parse(&text).is_err());
    assert!(Request::parse(&"x".repeat(16385)).is_err());
}
// Chain: R8 R9 R13 #2135 A1/A2: the existing Workspace/network request resolves without widening authority.
// Risk: rejecting approved host config, losing model/effort/account, or forwarding an unsupported override.
#[test]
fn protected_workspace_network_host_request_retains_authority_and_config() {
    let f = Fixture::new();
    let args = [
        "-s",
        "workspace-write",
        "-c",
        "sandbox_workspace_write.network_access=true",
    ];
    fs::write(f.dir.join("host.json"), json!({"codex":args}).to_string()).unwrap();
    let p = f
        .plan()
        .unwrap_or_else(|e| panic!("existing host Workspace/network request must resolve: {e}"));
    assert_eq!(
        p.args,
        [
            "-c",
            "sandbox_mode=\"danger-full-access\"",
            "-c",
            "approval_policy=\"never\"",
            "-c",
            "sandbox_workspace_write.network_access=true"
        ]
    );
    assert_eq!(p.access.mode, "workspace");
    assert!(
        p.access
            .profile
            .as_ref()
            .unwrap()
            .contains("deny file-write")
    );
    assert_eq!(p.authority["requested"], args.join(" "));
    assert_eq!(p.authority["account"], "default");
    assert_eq!(p.authority["account_home"], json!(f.home.join(".codex")));
    let profile = p.authority["profile_sha256"].clone();
    fs::write(f.dir.join("host.json"),json!({"codex":["--sandbox","workspace-write","--config","sandbox_workspace_write.network_access=true","--model","invented-model","--config","model_reasoning_effort=high"]}).to_string()).unwrap();
    let p = f.plan().unwrap();
    assert_eq!(p.authority["profile_sha256"], profile);
    assert_eq!(
        p.args,
        [
            "-c",
            "sandbox_mode=\"danger-full-access\"",
            "-c",
            "approval_policy=\"never\"",
            "--config",
            "sandbox_workspace_write.network_access=true",
            "-c",
            "model=\"invented-model\"",
            "--config",
            "model_reasoning_effort=high"
        ]
    );
    for config in [
        "sandbox_workspace_write.network_access=false",
        "sandbox_workspace_write.network_access=true.extra",
        "sandbox_workspace_write.network_access=\"true\"",
        "sandbox_workspace_write.writable_roots=[]",
        "sandbox_mode=\"danger-full-access\"",
        "approval_policy=\"never\"",
        "unknown=true",
    ] {
        fs::write(
            f.dir.join("host.json"),
            json!({"codex":["-s","workspace-write","-c",config]}).to_string(),
        )
        .unwrap();
        assert!(
            f.plan().is_err(),
            "unsupported config must refuse: {config}"
        );
    }
    for args in [
        vec!["-s", "read-only"],
        vec!["-c"],
        vec!["--unknown"],
        vec!["--add-dir", "/invented"],
    ] {
        fs::write(f.dir.join("host.json"), json!({"codex":args}).to_string()).unwrap();
        assert!(f.plan().is_err(), "{args:?}");
    }
}
// Chain: R8 R12 #2135 A2: query errors and permission mismatches remain closed and attributable without private paths.
// Risk: treating unknown enforcement as allowed or hiding the native query needed to diagnose containment.
#[test]
fn protected_sandbox_refusal_names_query_and_result_without_paths() {
    for (result, errno, denied, observed, expected) in [
        (-1, Some(22), true, "unknown", "denied"),
        (0, None, true, "allowed", "denied"),
        (1, None, false, "denied", "allowed"),
    ] {
        let error = sandbox_result("file-write-create", "account-config", result, errno, denied)
            .unwrap_err();
        assert_eq!(
            error,
            format!(
                "native sandbox readback: operation=file-write-create category=account-config expected={expected} observed={observed} return={result} errno={}; input refused",
                errno.map_or("none".into(), |v| v.to_string())
            )
        );
        assert!(
            !error.contains('/') && !error.contains('\\'),
            "private paths must not reach diagnostics"
        );
    }
    assert!(sandbox_result("file-write-data", "workspace-probe", 0, None, false).is_ok());
    assert!(sandbox_result("file-write-unlink", "policy-store", 1, None, true).is_ok());
    assert!(sandbox_result("file-write-unlink", "policy-store", 2, None, true).is_ok());
    assert!(sandbox_result("file-write-data", "workspace-probe", -1, Some(3), false).is_err());
}
// Chain: R8 #2135 A2: absent paths can report denial even when creation is allowed on macOS.
// Risk: rejecting the workspace or accepting nonexistent outside markers as containment evidence.
#[test]
fn protected_sandbox_queries_use_existing_trusted_objects() {
    let f = Fixture::new();
    let p = f.plan().unwrap();
    let paths = query_paths(&f.dir, &p);
    assert_eq!(paths.len(), 6);
    assert_eq!(paths[0], ("workspace-probe", f.root.clone(), false));
    assert_eq!(paths[5], ("outside-workspace", f.home.clone(), true));
    for (category, path, _) in &paths {
        assert!(path.exists(), "{category} must already exist");
    }
    assert_eq!(p.authority["sandbox_paths"], json!(p.queries));
    let mut calls = vec![];
    for q in &p.queries {
        for &op in if q.denied {
            &["file-write-data", "file-write-create", "file-write-unlink"][..]
        } else {
            &["file-write-data"][..]
        } {
            q.check(op, |path| {
                assert!(path.exists());
                calls.push((q.category, op, q.denied));
                Ok((i32::from(q.denied), None))
            })
            .unwrap();
        }
    }
    assert_eq!(calls.len(), 16);
    assert_eq!(calls[0], ("workspace-probe", "file-write-data", false));
    assert!(calls[1..].iter().all(|(_, _, denied)| *denied));
    let account = f
        .home
        .join(".codex")
        .join(runtime::accounts::HOMES)
        .join("invented-account");
    fs::create_dir_all(&account).unwrap();
    fs::write(account.join("config.toml"), b"# invented account config").unwrap();
    fs::write(
        f.dir.join("accounts.json"),
        r#"{"codex":[{"label":"invented-account","enabled":true}]}"#,
    )
    .unwrap();
    let mut r = f.r.clone();
    r.account = "invented-account".into();
    let p = resolve(&f.dir, &f.home, &r, "macos", &f.bin).unwrap();
    assert_eq!(p.queries[4].path, account.join("config.toml"));
}
// Chain: R8 R9 #2135 A2: unknown/missing/replaced query objects close both allowed and denied observations.
// Risk: treating ENOENT as enforcement or accepting a changed path before input delivery.
#[test]
fn protected_sandbox_query_identities_refuse_missing_changed_and_racing_paths() {
    for index in 1..5 {
        let f = Fixture::new();
        let p = f.plan().unwrap();
        let q = &p.queries[index];
        fs::remove_file(&q.path).unwrap();
        fs::create_dir(&q.path).unwrap();
        assert!(
            f.plan().is_err(),
            "a directory is not a required control file: {}",
            q.category
        );
    }
    for index in 0..6 {
        let f = Fixture::new();
        let p = f.plan().unwrap();
        let q = &p.queries[index];
        if q.path.is_dir() {
            fs::remove_dir_all(&q.path).unwrap();
        } else {
            fs::remove_file(&q.path).unwrap();
        }
        let error = q
            .check("file-write-data", |_| {
                panic!("a missing object must not reach the kernel query")
            })
            .unwrap_err();
        assert!(error.contains(q.category) && error.contains("unknown-or-changed"));
        assert!(!error.contains('/') && !error.contains('\\'));
        assert!(
            f.plan().is_err(),
            "a missing object must also refuse before launch"
        );
    }
    for result in [0, 1] {
        let f = Fixture::new();
        let p = f.plan().unwrap();
        let q = &p.queries[4];
        assert!(
            q.check("file-write-data", |path| {
                fs::write(path, b"# changed invented config").unwrap();
                Ok((result, None))
            })
            .is_err()
        );
        assert_ne!(
            f.plan().unwrap().authority,
            p.authority,
            "config changes must invalidate original launch authority"
        );
    }
    let f = Fixture::new();
    let p = f.plan().unwrap();
    let q = &p.queries[0];
    fs::write(f.root.join("ordinary-workspace-output"), b"invented output").unwrap();
    q.check("file-write-data", |_| Ok((0, None))).unwrap();
    assert_eq!(
        f.plan().unwrap().authority,
        p.authority,
        "directory content changes preserve identity"
    );
    #[cfg(unix)]
    {
        let original = f.dir.join("original-project");
        fs::rename(&f.root, &original).unwrap();
        fs::create_dir(&f.root).unwrap();
        assert!(
            q.check("file-write-data", |_| panic!("replacement must refuse"))
                .is_err()
        );
        fs::remove_dir(&f.root).unwrap();
        std::os::unix::fs::symlink(&original, &f.root).unwrap();
        assert!(
            q.check("file-write-data", |_| panic!(
                "retargeted alias must refuse"
            ))
            .is_err()
        );
    }
}
// R9/R12 A2/A7: intent precedes spawn; interrupted/unknown/terminal records all consume the same launch.
// Risk: restarting a supervisor replays an AssetQ operation or a stale PID is treated as live authority.
#[test]
fn protected_interruption_never_replays_and_unknown_identity_refuses() {
    let mut f = Fixture::new();
    let p = f.plan().unwrap();
    let id = coreq::process::Identity {
        pid: 42,
        started: "invented-creation".into(),
    };
    let mut s = resources::State::default();
    s.ops.insert(
        "slot".into(),
        resources::Op {
            bound: json!({"local_slot":{}}),
            receipt: json!({"occupied":true,"instance":"invented-host"}),
            expires_at: 1,
            saved_at: None,
            writing: false,
            sent: true,
        },
    );
    let pre = preparation(&f.r, &p.authority, &s.ops["slot"].receipt["instance"]);
    f.r.preparation = digest(pre.to_string().as_bytes());
    s.ops.get_mut("slot").unwrap().bound["local_slot"]["prepared"] = pre;
    consume(&mut s, "slot", &f.r, &p.authority, &id).unwrap();
    input_gate(&s, "slot", &f.r).unwrap();
    let original = s.clone();
    let effective = json!({"state":"verified-effective","scope":"invented-native-observation"});
    assert!(bound_gate(&s, "slot", &f.r, &p.authority, &id, &id, Some(&effective)).is_err());
    let launch = &mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"];
    launch["native"] = json!({"pid":id.pid,"started":id.started});
    launch["state"] = json!("verified-effective");
    launch["verified_effective"] = effective.clone();
    bound_gate(&s, "slot", &f.r, &p.authority, &id, &id, Some(&effective)).unwrap();
    let verified = s.clone();
    for field in [
        "authority",
        "native",
        "supervisor",
        "state",
        "verified_effective",
        "cancel_requested",
        "host_instance",
    ] {
        let mut changed = verified.clone();
        changed.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"][field] =
            json!("tampered-or-unknown");
        assert!(
            bound_gate(
                &changed,
                "slot",
                &f.r,
                &p.authority,
                &id,
                &id,
                Some(&effective)
            )
            .is_err(),
            "{field}"
        );
    }
    for field in ["executable_sha256", "adapter"] {
        let mut changed = verified.clone();
        changed.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["authority"][field] =
            json!("tampered-or-unknown");
        assert!(
            bound_gate(
                &changed,
                "slot",
                &f.r,
                &p.authority,
                &id,
                &id,
                Some(&effective)
            )
            .is_err(),
            "{field}"
        );
    }
    s = original.clone();
    for state in ["unknown", "verified-effective", "exited", "cancelled"] {
        s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["state"] = json!(state);
        let before = s.clone();
        assert!(consume(&mut s, "slot", &f.r, &p.authority, &id).is_err());
        assert_eq!(s, before);
    }
    assert_eq!(
        original.ops["slot"].bound["local_slot"]["launch"]["state"],
        "unknown"
    );
    assert!(exact(&id, Err("native observation denied".into())).is_err());
    assert!(exact(&id, Ok(None)).is_err());
    assert!(
        exact(
            &id,
            Ok(Some(coreq::process::Identity {
                pid: 42,
                started: "reused-pid".into()
            }))
        )
        .is_err()
    );
    assert!(exact(&id, Ok(Some(id.clone()))).is_ok());
    assert!(native(&json!({"pid":u64::MAX,"started":"invented"})).is_err());
    let mut changed = f.r.clone();
    changed.policy_revision += 1;
    assert!(launch_record(&s, "slot", &changed).is_err());
    s = original.clone();
    cancel(&mut s, "slot", &f.r).unwrap();
    assert!(input_gate(&s, "slot", &f.r).is_err());
    assert!(
        native(&s.ops["slot"].bound["local_slot"]["launch"]["native"]).is_err(),
        "pre-spawn cancellation remains durable without a guessed child"
    );
    assert!(consume(&mut s, "slot", &f.r, &p.authority, &id).is_err());
    s = original.clone();
    s.ops.get_mut("slot").unwrap().receipt["instance"] = json!("restarted-host");
    assert!(input_gate(&s, "slot", &f.r).is_err());
    resources::within(&f.dir, resources::WAIT, |stored| {
        *stored = s.clone();
        Ok(())
    })
    .unwrap();
    resources::within(&f.dir, resources::WAIT, |stored| {
        assert!(consume(stored, "slot", &f.r, &p.authority, &id).is_err());
        Ok(())
    })
    .unwrap();
    fs::write(
        resources::dir(&f.dir).join("state.json.pending"),
        "interrupted save",
    )
    .unwrap();
    assert!(resources::within(&f.dir, resources::WAIT, |_| Ok(())).is_err());
}
// R9 A3: the canonical grant writer and launch input gate cannot race a revocation.
#[test]
fn protected_grant_writer_shares_the_input_authority_lock() {
    let f = Fixture::new();
    let held = resources::acquire(&f.dir, resources::WAIT).unwrap();
    let before = fs::read(f.dir.join(policy::FILE)).unwrap();
    assert!(
        policy::grant(
            &f.dir,
            "codex",
            "none",
            Some(&f.root),
            "invented-operator",
            2
        )
        .is_err()
    );
    assert_eq!(fs::read(f.dir.join(policy::FILE)).unwrap(), before);
    drop(held);
    policy::grant(
        &f.dir,
        "codex",
        "none",
        Some(&f.root),
        "invented-operator",
        2,
    )
    .unwrap();
    assert!(f.plan().is_err());
}
// Chain: R11 §8 — update retention includes the external adapter until whole-turn release, even if launch is unknown.
#[test]
fn protected_unknown_launch_retains_its_release_until_slot_release() {
    let f = Fixture::new();
    fs::create_dir(f.root.join(".taskq")).unwrap();
    fs::write(
        f.root.join(".taskq/status.json"),
        json!({"version":crate::platform::VERSION}).to_string(),
    )
    .unwrap();
    resources::within(&f.dir,resources::WAIT,|s| {
            s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{"launch":{"authority":{"adapter":{"version":"invented-retained"}}}}}),
                receipt:json!({"occupied":true}),expires_at:1,saved_at:None,writing:false,sent:true});Ok(())
        }).unwrap();
    assert!(
        crate::platform::releases_in_use(&f.dir)
            .unwrap()
            .contains(&"invented-retained".into())
    );
    resources::within(&f.dir, resources::WAIT, |s| {
        s.ops.get_mut("slot").unwrap().receipt["occupied"] = json!(false);
        Ok(())
    })
    .unwrap();
    assert!(
        !crate::platform::releases_in_use(&f.dir)
            .unwrap()
            .contains(&"invented-retained".into())
    );
    fs::write(
        resources::dir(&f.dir).join("state.json.pending"),
        "interrupted save",
    )
    .unwrap();
    assert!(
        crate::platform::releases_in_use(&f.dir).is_none(),
        "uncertain ownership retains every release"
    );
}
