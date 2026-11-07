use super::*;

/// Stock macOS and Linux ship only `python3`.
fn python() -> PathBuf {
    which(if cfg!(windows) { "python" } else { "python3" })
}

// §4/A3: delayed persistence must not recapture another execution's transcript or queue delay.
#[test]
fn agentq_delayed_record_excludes_later_turn_bytes() {
    let root = std::env::temp_dir().join(format!("taskq-agentq-freeze-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let log = root.join("native.log");
    let mut request = Request::new(&root, &log, "invented input", "taskq");
    request.command = vec![
        python().into(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/runtime-launch.py")
            .into(),
    ];
    request.optimization = agentq_runtime::optimization::Intent::Off;
    request.env = vec![
        ("TASKQ_FIXTURE_RT".into(), "claude".into()),
        (
            "TASKQ_FIXTURE_RECORD".into(),
            root.join("request.json").into(),
        ),
    ];
    let adapter = agent("claude").unwrap();
    let mut run = submit("claude", None, &request).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while adapter.poll(&mut run).unwrap() == agentq_runtime::agent::State::Running {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let before = adapter.report(&run).unwrap();
    assert_eq!(before.usage.input, Some(2));
    writeln!(File::options().append(true).open(&log).unwrap(), "{}",
        serde_json::json!({"type":"assistant","message":{"id":"next-execution","model":"invented-model","usage":{"input_tokens":100,"output_tokens":1}}})).unwrap();
    let store = coreq::telemetry::Store::open(&root.join("records.db")).unwrap();
    run.snapshot(&record_context())
        .unwrap()
        .append(&store)
        .unwrap();
    let rows = store.export_kinds(&["agent.turn"], 0, 0, 100).unwrap();
    let input = rows["events"][0]["observation"]["input_tokens"].clone();
    drop(store);
    drop(run);
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(
        input,
        serde_json::json!(2),
        "AgentQ record reread a later execution after terminal polling"
    );
}

// §8/R9/R12: trait launch and resume retain literal input, host flags, daemon env and native scope.
#[test]
fn trait_launch_preserves_input_policy_environment_and_exit() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("taskq-launch-{}", uuid()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    std::fs::create_dir_all(root.join("telemetry.log")).unwrap();
    use tracing_subscriber::prelude::*;
    let store =
        std::sync::Arc::new(coreq::telemetry::Store::open(&root.join("caller.db")).unwrap());
    let capture = coreq::telemetry::capture::Capture::new(store.clone(), "taskq", "test").unwrap();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let caller = tracing::info_span!(
        "fixture.launch",
        project = "fixture/project",
        task = 2427u64,
        attempt = 42u64,
        actor = "worker",
        entry = 43u64
    );
    let _caller = caller.enter();
    let python = python();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-launch.py");
    let args = [
        "-s",
        "danger-full-access",
        "-m",
        "invented-model",
        "-c",
        "model_reasoning_effort=high",
    ]
    .map(String::from);
    let vars = vec![
        ("AGENTQ_OPTIMIZATION".into(), "off".into()),
        ("INVENTED_BOARD_SECRET".into(), "invented-secret".into()),
    ];
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("taskq"),
        install: &root,
        permission_mode: "dontAsk",
        codex: &args,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &vars,
        jobs: Some(2),
    };
    // The common boundary must bind even refused background/app launches, before adapter selection.
    let refused = Launch {
        files: String::new(),
        execution: Some(88),
        task: 55,
        title: "Invented refusal".into(),
        attempt: 66,
        role: "reviewer".into(),
        rt: "codex-app".into(),
        session: "-".into(),
        url: String::new(),
        resume: None,
    };
    assert!(
        spawn(&env, &refused).is_err(),
        "no operator grant: no launch"
    );
    capture.flush().unwrap();
    let boundaries = store.export_kinds(&["method.start"], 0, 0, 100).unwrap();
    let boundary = boundaries["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["observation"]["operation"] == "taskq.runtime.agent")
        .expect("all launch paths need the shared boundary");
    assert_eq!(boundary["observation"]["task"], 55);
    assert_eq!(boundary["observation"]["attempt"], 66);
    assert_eq!(boundary["observation"]["actor"], "reviewer");
    assert_eq!(boundary["observation"]["entry"], 88);
    let input = format!(
        "literal \"quote\", $HOME, `command`\n{}EOF-CANARY",
        "private-input-".repeat(12000)
    );
    for rt in ["codex", "claude", "antigravity"] {
        crate::policy::grant(&root, rt, "full", None, "invented operator", 1).unwrap();
        let access = access(&env, rt).unwrap();
        for resume in [false, true] {
            let l = Launch {
                files: String::new(),
                execution: None,
                task: 2427,
                title: "Invented fixture".into(),
                attempt: 42,
                role: "worker".into(),
                rt: rt.into(),
                session: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),
                url: String::new(),
                resume: resume.then(|| "continue".into()),
            };
            let record = root.join(format!("{rt}-{resume}.json"));
            let log = root.join(format!("{rt}-{resume}.log"));
            let mut req = request(&env, &l, &log, &input, &access).unwrap();
            req.command = vec![python.clone().into(), fixture.clone().into()];
            req.env.extend([
                ("TASKQ_FIXTURE_RT".into(), rt.into()),
                ("TASKQ_FIXTURE_RECORD".into(), record.clone().into()),
                ("PYTHONIOENCODING".into(), "utf-8".into()),
                ("TASKQ_FIXTURE_LINGER".into(), "1".into()),
            ]);
            assert!(!format!("{:?}", req.host_args).contains("invented-secret"));
            let mut child = native(rt, resume.then_some(l.session.as_str()), &mut req).unwrap();
            child.install = Some(root.clone());
            let identity = child.native_identity().unwrap();
            let held = coreq::file_lock::lock(&root.join("taskq-profile.lock")).unwrap();
            let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                let began = std::time::Instant::now();
                if let Some(code) = child.exited().unwrap() {
                    let elapsed = began.elapsed();
                    drop(held);
                    assert!(
                        elapsed < std::time::Duration::from_millis(500),
                        "terminal polling waited on telemetry: {elapsed:?}"
                    );
                    assert_ne!(
                        code, 0,
                        "a completed turn stops the deliberately lingering client"
                    );
                    assert_ne!(
                        coreq::process::identity(identity.pid).unwrap(),
                        Some(identity)
                    );
                    assert_eq!(
                        classify(rt, &std::fs::read_to_string(&log).unwrap())
                            .unwrap()
                            .outcome,
                        "ok"
                    );
                    assert!(
                        child.exited().unwrap().is_some(),
                        "terminal reread must not duplicate records"
                    );
                    break;
                }
                if std::time::Instant::now() >= until {
                    let _ = child.kill();
                    panic!("{rt} resume={resume}: no exit");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let row: serde_json::Value =
                serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
            if rt == "antigravity" {
                let wire: serde_json::Value =
                    serde_json::from_str(row["input"].as_str().unwrap()).unwrap();
                assert_eq!(wire["message"]["content"], input);
            } else {
                assert_eq!(row["input"], input);
            }
            assert_eq!(row["env"]["TASKQ_ATTEMPT"], "42");
            assert_eq!(row["env"]["CARGO_BUILD_JOBS"], "2");
            assert_eq!(row["env"]["INVENTED_BOARD_SECRET"], "invented-secret");
            assert_eq!(row["env"]["AGENTQ_OPTIMIZATION"], "off");
            assert_eq!(row["env"]["AGENTQ_LAUNCHER"], "taskq");
            let argv: Vec<_> = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            if rt == "codex" {
                for arg in &args {
                    assert!(argv.contains(&arg.as_str()), "host flag {arg}");
                }
            }
            if rt == "claude" {
                let settings: serde_json::Value = serde_json::from_str(
                    argv[argv.iter().position(|s| *s == "--settings").unwrap() + 1],
                )
                .unwrap();
                assert_eq!(settings["env"]["TASKQ_ATTEMPT"], "42");
                assert_eq!(settings["env"]["CARGO_BUILD_JOBS"], "2");
                assert!(
                    settings["hooks"]["SessionStart"][0]["hooks"][0]["command"]
                        .as_str()
                        .unwrap()
                        .contains("rules compact")
                );
                assert!(!settings.to_string().contains("invented-secret"));
            }
            assert_eq!(
                native_session(rt, &log).as_deref(),
                Some(l.session.as_str())
            );
            #[cfg(windows)]
            assert!(child.job().unwrap().starts_with("job-kill:"));
        }
    }
    assert!(super::super::records::settle(
        std::time::Duration::from_secs(30)
    ));
    let reader =
        coreq::telemetry::Reader::open(&root.join("telemetry.log/universal.sqlite3")).unwrap();
    let rows = reader.export_kinds(&["agent.turn"], 0, 0, 100).unwrap();
    assert_eq!(
        rows["events"].as_array().unwrap().len(),
        6,
        "one AgentQ turn per launch/resume"
    );
    for row in rows["events"].as_array().unwrap() {
        let v = &row["observation"];
        for (field, want) in [
            ("project", serde_json::json!("fixture/project")),
            ("task", serde_json::json!(2427)),
            ("attempt", serde_json::json!(42)),
            ("actor", serde_json::json!("worker")),
            ("entry", serde_json::json!(43)),
        ] {
            assert_eq!(v[field], want, "{v}");
        }
        if v["runtime"] != "claude" {
            assert!(
                v["input_tokens"].is_null() && v["unknown"]["input_tokens"].is_string(),
                "unreported usage stays unknown"
            );
        }
    }
    let calls = reader.export_kinds(&["agent.call"], 0, 0, 100).unwrap();
    assert_eq!(
        calls["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["observation"]["runtime"] == "claude")
            .count(),
        2,
        "AgentQ captures model calls on launch and resume"
    );
    for row in calls["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["observation"]["runtime"] == "claude")
    {
        assert_eq!(row["observation"]["input_tokens"], 2);
        assert_eq!(row["observation"]["output_tokens"], 4);
        assert_eq!(row["observation"]["entry"], 43);
    }
    assert!(
        !rows.to_string().contains("private-input-")
            && !rows.to_string().contains("invented-secret")
    );
    drop(reader);
    drop(_caller);
    drop(caller);
    drop(_subscriber);
    drop(capture);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

// §8/R11: a background resume carries no new flags; raw stderr/exit stays available to TaskQ's refusal guard.
#[test]
fn background_trait_retains_launcher_output_and_saved_session() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("taskq-background-launch-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let session = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let home = root.join("home");
    let plain = root
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .unwrap_or(&root.to_string_lossy())
        .to_owned();
    let project: String = plain
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let transcript = home
        .join("projects")
        .join(project)
        .join(format!("{session}.jsonl"));
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "").unwrap();
    let adapter = agent("claude").unwrap();
    for resume in [false, true] {
        let record = root.join(format!("{resume}.json"));
        let mut request = Request::new(
            &root,
            root.join("launch.log"),
            "invented protected input reference",
            "taskq",
        );
        request.command = vec![
            python().into(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/runtime-launch.py")
                .into(),
        ];
        request.mode = Mode::Background;
        request.home = Some(home.clone());
        request.policy.approval = Approval::Deny;
        request.title = Some("invented title".into());
        request.settings_env = vec![
            ("TASKQ_ATTEMPT".into(), "42".into()),
            ("CARGO_BUILD_JOBS".into(), "2".into()),
        ];
        request.env = vec![
            ("TASKQ_FIXTURE_RT".into(), "claude".into()),
            ("TASKQ_FIXTURE_RECORD".into(), record.clone().into()),
            ("PYTHONIOENCODING".into(), "utf-8".into()),
        ];
        let mut run = submit("claude", resume.then_some(session), &request).unwrap();
        let output = adapter.launcher_output(&mut run).unwrap().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap().trim(),
            "invented launcher diagnostic"
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "backgrounded · aaaaaaaa"
        );
        assert!(
            adapter.launcher_output(&mut run).is_err(),
            "launcher receipt is handed over once"
        );
        let row: serde_json::Value =
            serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
        assert_eq!(row["input"], request.input);
        if resume {
            assert_eq!(
                row["args"],
                serde_json::json!(["--bg", "--resume", session])
            );
        } else {
            assert!(
                row["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|arg| arg == "--settings")
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

// §3/R11: a trait-owned launcher cannot release its surviving descendants or stop an unrelated scope.
#[cfg(windows)]
#[test]
fn trait_scope_retains_descendants_after_launcher_exit() {
    struct Owned(Native);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !self.0.exited().is_ok_and(|exit| exit.is_some())
                && std::time::Instant::now() < until
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    let root = std::env::temp_dir().join(format!("taskq-trait-scope-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let tree = root.join("descendant");
    let request = |name: &str, tree: Option<&Path>| {
        let mut request = Request::new(
            &root,
            root.join(format!("{name}.log")),
            "invented input",
            "taskq",
        );
        request.command = vec![
            python().into(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/runtime-launch.py")
                .into(),
        ];
        request.env = vec![
            ("TASKQ_FIXTURE_RT".into(), "codex".into()),
            (
                "TASKQ_FIXTURE_RECORD".into(),
                root.join(format!("{name}.json")).into(),
            ),
            ("TASKQ_FIXTURE_LINGER".into(), "1".into()),
        ];
        if let Some(tree) = tree {
            request.env.push(("TASKQ_FIXTURE_TREE".into(), tree.into()));
        }
        request
    };
    let mut owned_child = Owned(native("codex", None, &mut request("tree", Some(&tree))).unwrap());
    let mut owned_sentinel = Owned(native("codex", None, &mut request("sentinel", None)).unwrap());
    let child = &mut owned_child.0;
    let sentinel = &mut owned_sentinel.0;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !tree.exists() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let pid = std::fs::read_to_string(&tree).unwrap().parse().unwrap();
    let descendant = coreq::process::identity(pid).unwrap().unwrap();
    let launcher = child.native_identity().unwrap();
    std::fs::write(tree.with_extension("exit"), "").unwrap();
    while coreq::process::identity(launcher.pid).unwrap().as_ref() == Some(&launcher) {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(child.exited(), Ok(None));
    assert_eq!(
        coreq::process::identity(pid).unwrap(),
        Some(descendant.clone())
    );
    child.kill().unwrap();
    while child.exited().unwrap().is_none() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_ne!(coreq::process::identity(pid).unwrap(), Some(descendant));
    assert_eq!(
        child.exited(),
        Ok(Some(-1)),
        "cancelled scope preserves its terminal verdict"
    );
    assert_eq!(
        coreq::process::identity(sentinel.id()).unwrap(),
        Some(sentinel.native_identity().unwrap())
    );
    sentinel.kill().unwrap();
    while sentinel.exited().unwrap().is_none() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(owned_child);
    drop(owned_sentinel);
    std::fs::remove_dir_all(root).unwrap();
}

// A7/R9/R12: paid native start/resume and transcript evidence, under existing operator authority only.
#[test]
#[ignore = "paid native qualification; TASKQ_NATIVE_RUNTIME and TASKQ_NATIVE_INSTALL require owner approval"]
fn native_trait_start_resume_and_transcript() {
    let rt = std::env::var("TASKQ_NATIVE_RUNTIME").expect("TASKQ_NATIVE_RUNTIME");
    assert!(RuntimeId::parse(&rt).is_some());
    let install =
        PathBuf::from(std::env::var_os("TASKQ_NATIVE_INSTALL").expect("TASKQ_NATIVE_INSTALL"));
    let host = crate::host::Policy::read_for_hostname(&install, crate::config::hostname).unwrap();
    assert!(
        host.cap.limits.get(&rt).is_none_or(|limit| *limit > 0),
        "disabled runtime is not probe authority"
    );
    let root = std::env::var_os("TASKQ_NATIVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let root = root.as_path();
    let exe = std::env::current_exe().unwrap();
    let authority = Env {
        context: "",
        root,
        exe: &exe,
        install: &install,
        permission_mode: &host.permission_mode,
        codex: &host.codex,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    access(&authority, &rt).unwrap();
    let evidence = std::env::temp_dir().join(format!("taskq-native-qualification-2427-{}", uuid()));
    let fixture = evidence.join("install");
    std::fs::create_dir_all(&fixture).unwrap();
    println!("evidence={}", evidence.display());
    let env = Env {
        install: &fixture,
        ..authority
    };
    let log = evidence.join("native.log");
    File::create(&log).unwrap();
    let mut session = String::from("-");
    let mut rows = vec![];
    for resume in [false, true] {
        let access = access(&authority, &rt).unwrap();
        let canary = format!("TaskQ-Qualification-2427-{}", uuid());
        let text = format!(
            "This is an approved non-task TaskQ Qualification probe. Reply with exactly {canary}. Do no work, edit no files, run no commands and make no taskq or board writes. Read this turn's input file if the launcher refers to it; use no other tools."
        );
        let l = Launch {
            files: String::new(),
            execution: None,
            task: 0,
            title: "TaskQ Qualification 2427".into(),
            attempt: 0,
            role: "operator".into(),
            rt: rt.clone(),
            session: session.clone(),
            url: String::new(),
            resume: resume.then(|| text.clone()),
        };
        std::fs::write(evidence.join(format!("input-{resume}.json")),serde_json::to_vec_pretty(&serde_json::json!({"runtime":rt,"resume":resume,"session":session,"input":text,"access":access.json()})).unwrap()).unwrap();
        let pos = std::fs::metadata(&log).unwrap().len();
        let mut proc: Box<dyn Proc> = if of(&rt).has(Cap::AppTurn) {
            assert!(
                app_running().unwrap(),
                "the existing signed-in app must be running"
            );
            let mut turn = AppTurn {
                root: root.canonicalize().unwrap(),
                history: AppHistory::default(),
                hand: None,
                log: log.clone(),
                text: text.clone(),
                _input: None,
                binding: None,
                thread: session.clone(),
                done: 0,
                rollout: None,
                running: app_running,
                control: app_cancel,
                cancelled: false,
                queue_error: None,
                accepted: false,
            };
            if resume {
                turn.queue().unwrap();
            } else {
                turn.hand = Some(start(&env, &l, &log, HANDSHAKE, &access).unwrap());
            }
            Box::new(turn)
        } else if of(&rt).has(Cap::Background) {
            Box::new(ClaudeTurn::start(&env, &l, &log, &text, &access).unwrap())
        } else {
            let child = start(&env, &l, &log, &text, &access).unwrap();
            let native = child.native_identity().ok();
            Box::new(CliTurn {
                proc: child,
                log: log.clone(),
                pos,
                start: pos,
                stopping: false,
                accepted: false,
                read_error: None,
                session: of(&rt).has(Cap::StreamInput).then(|| l.session.clone()),
                native,
                recovery: None,
                rt: rt.clone(),
            })
        };
        let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let exit = loop {
            match proc.exited() {
                Ok(Some(code)) => break code,
                Ok(None) => {}
                Err(reason) => eprintln!("qualification observation pending: {reason}"),
            }
            if std::time::Instant::now() >= until {
                let cleanup = stop(proc.as_mut());
                panic!(
                    "native qualification timed out; cleanup={cleanup:?}; evidence={}",
                    evidence.display()
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        let accepted = proc.accepted();
        std::fs::write(evidence.join(format!("exit-{resume}.json")),serde_json::to_vec_pretty(&serde_json::json!({"exit":exit,"accepted":accepted,"session":native_session(&rt,&log)})).unwrap()).unwrap();
        assert!(
            accepted.unwrap(),
            "native input must be accepted independently of exit"
        );
        let id = native_session(&rt, &log).expect("native session receipt");
        if resume {
            assert_eq!(id, session, "resume cannot create a replacement session");
        }
        session = id;
        let (_, native) = worker_transcript(&rt, &session, &log).unwrap();
        assert!(
            std::fs::read_to_string(&native).unwrap().contains(&canary),
            "native transcript must contain this turn's fresh canary"
        );
        let page = crate::dialogue::worker_transcript(&format!("{rt}:{session}"), &log, None, None)
            .unwrap();
        assert!(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["role"] == "assistant"
                    && item["text"]
                        .as_str()
                        .is_some_and(|text| text.contains(&canary))),
            "TaskQ transcript must show the same native reply"
        );
        rows.push(serde_json::json!({"resume":resume,"session":session,"accepted":true,"exit":exit,"canary":canary,"access":access.mode}));
        drop(proc);
    }
    let manifest = serde_json::json!({"runtime":rt,"os":std::env::consts::OS,"source":env!("CARGO_MANIFEST_DIR"),"turns":rows,"scope":"native start/resume and transcript; no live board or installed host service"});
    std::fs::write(
        evidence.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    println!("{}\nevidence={}", manifest, evidence.display());
}

// A2 (#2448): paid real worker -> owner SIGKILL -> a new process adopts it -> native exit -> Exit release, with no new submission.
// The board is in memory and the installed service is untouched; `live-probe.py` remains the installed-service proof.
#[cfg(unix)]
#[test]
#[ignore = "paid native qualification; TASKQ_NATIVE_RUNTIME and TASKQ_NATIVE_INSTALL require owner approval"]
fn native_worker_survives_service_restart() {
    use crate::board::Board;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let rt = std::env::var("TASKQ_NATIVE_RUNTIME").expect("TASKQ_NATIVE_RUNTIME");
    let install =
        PathBuf::from(std::env::var_os("TASKQ_NATIVE_INSTALL").expect("TASKQ_NATIVE_INSTALL"));
    let Some(phase) = std::env::var_os("TASKQ_RESTART_PHASE") else {
        let evidence =
            std::env::temp_dir().join(format!("taskq-native-qualification-2448-{}", uuid()));
        std::fs::create_dir_all(evidence.join("install")).unwrap();
        println!("evidence={}", evidence.display());
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "runtime::launch::tests::native_worker_survives_service_restart",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_RESTART_EVIDENCE", &evidence)
            .stdin(Stdio::null());
        let mut old = command
            .env("TASKQ_RESTART_PHASE", "before")
            .stdout(File::create(evidence.join("before.log")).unwrap())
            .stderr(File::create(evidence.join("before.err")).unwrap())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(120);
        while !evidence.join("ready").exists() {
            assert!(old.try_wait().unwrap().is_none(), "owner exited early");
            assert!(Instant::now() < until, "no native identity in time");
            std::thread::sleep(Duration::from_millis(50));
        }
        old.kill().unwrap();
        old.wait().unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(evidence.join("ready")).unwrap()).unwrap();
        let pid = saved["pid"].as_u64().unwrap() as u32;
        println!(
            "owner killed; native pid {pid} alive: {}",
            coreq::process::identity(pid).unwrap().is_some()
        );
        let after = coreq::process::output(
            "qualification.native-worker-restart",
            command
                .env("TASKQ_RESTART_PHASE", "after")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
            Duration::from_secs(420),
        );
        assert!(after.is_ok(), "adoption failed: {after:?}");
        println!(
            "{}",
            std::fs::read_to_string(evidence.join("verified")).unwrap()
        );
        return;
    };
    let evidence = PathBuf::from(std::env::var_os("TASKQ_RESTART_EVIDENCE").unwrap());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let exe = std::env::current_exe().unwrap();
    let fixture = evidence.join("install");
    let host = crate::host::Policy::read_for_hostname(&install, crate::config::hostname).unwrap();
    // The in-memory board is rebuilt identically in both processes; only the native record crosses.
    let b = crate::board::Fake::default();
    let task = b
        .create(
            "TaskQ Qualification 2448",
            "",
            &[crate::service::TASK_LABEL.into()],
        )
        .unwrap();
    let attempt = b
        .comment(task, &format!("taskq: spawn worker {rt}:fixture mac"))
        .unwrap();
    let cfg = crate::service::Config {
        root: root.clone(),
        install: fixture.clone(),
        ..crate::tests::cfg()
    };
    let mut service = crate::service::Service::new(
        &b,
        &cfg,
        "native-fixture",
        Box::new(|_| panic!("restart must not submit new work")),
    );
    let l = Launch {
        files: String::new(),
        execution: Some(attempt),
        task,
        title: "TaskQ Qualification 2448".into(),
        attempt,
        role: "worker".into(),
        rt: rt.clone(),
        session: "-".into(),
        url: String::new(),
        resume: None,
    };
    let log = log_path(&root, &l);
    if phase == "before" {
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        File::create(&log).unwrap();
        let authority = Env {
            context: "",
            root: &root,
            exe: &exe,
            install: &install,
            permission_mode: &host.permission_mode,
            codex: &host.codex,
            delivery: crate::config::Delivery::AutoMerge,
            board_env: &[],
            jobs: None,
        };
        let access = access(&authority, &rt).unwrap();
        let env = Env {
            install: &fixture,
            ..authority
        };
        let canary = format!("TaskQ-Qualification-2448-{}", uuid());
        let text = format!(
            "This is an approved non-task TaskQ Qualification probe. Run the shell command `sleep 45` once, edit no files, then reply with exactly {canary}. If the command is refused, wait and still reply with exactly {canary}."
        );
        let mut proc: Box<dyn Proc> = if of(&rt).has(Cap::Background) {
            Box::new(ClaudeTurn::start(&env, &l, &log, &text, &access).unwrap())
        } else {
            let child = start(&env, &l, &log, &text, &access).unwrap();
            let native = child.native_identity().ok();
            Box::new(CliTurn {
                proc: child,
                log: log.clone(),
                pos: 0,
                start: 0,
                stopping: false,
                accepted: false,
                read_error: None,
                session: None,
                native,
                recovery: None,
                rt: rt.clone(),
            })
        };
        let until = Instant::now() + Duration::from_secs(100);
        let native = loop {
            let _ = proc.exited();
            if let Ok(native) = proc.native_identity()
                && proc.checkpoint(&cfg, &l, attempt).is_ok()
            {
                break native;
            }
            assert!(Instant::now() < until, "no native identity");
            std::thread::sleep(Duration::from_millis(100));
        };
        service.kids.push(crate::service::Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            task,
            attempt,
            execution: attempt,
            rt: rt.clone(),
            proc,
        });
        crate::service_run::save_kids(&service, &root).unwrap();
        coreq::write_durable(
            &evidence.join("ready"),
            &serde_json::json!({"pid":native.pid,"started":native.started,"canary":canary})
                .to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(60));
        panic!("controller did not kill the owner");
    }
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.join("ready")).unwrap()).unwrap();
    let canary = saved["canary"].as_str().unwrap();
    crate::service_run::adopt(&mut service, &fixture, &root).unwrap();
    assert_eq!(service.kids.len(), 1, "the record must adopt its worker");
    assert_eq!(service.kids[0].execution, attempt);
    assert_eq!(
        service.kids[0].proc.id() as u64,
        saved["pid"].as_u64().unwrap()
    );
    let until = Instant::now() + Duration::from_secs(400);
    while !service.kids.is_empty() {
        service.reap_owned();
        assert!(
            Instant::now() < until,
            "native exit not observed: {:?}",
            service.kids.iter().map(|k| &k.unknown).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    let row = crate::core::executions(task, &crate::board::entries(&b, task).unwrap())
        .into_iter()
        .find(|x| x.id == attempt)
        .unwrap();
    let session = native_session(&rt, &log).expect("native session receipt");
    let page =
        crate::dialogue::worker_transcript(&format!("{rt}:{session}"), &log, None, None).unwrap();
    let replied = page["items"].as_array().unwrap().iter().any(|item| {
        item["role"] == "assistant" && item["text"].as_str().is_some_and(|t| t.contains(canary))
    });
    let limits = agent(&rt).unwrap().log_limits(
        &std::fs::read_to_string(&log).unwrap(),
        Some(&session),
        None,
    );
    let verified = serde_json::json!({"runtime":rt,"pid":saved["pid"],"session":session,"accepted":row.accepted,"release":format!("{:?}",row.release),"replied":replied,"log_windows":limits.len()});
    std::fs::write(evidence.join("verified"), verified.to_string()).unwrap();
    assert_eq!(row.release, Some(crate::core::Release::Exit));
    assert!(
        replied,
        "the adopted worker's reply must reach the transcript"
    );
    std::fs::remove_file(&log).unwrap();
}
