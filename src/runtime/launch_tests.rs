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
    let vars = vec![("INVENTED_BOARD_SECRET".into(), "invented-secret".into())];
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
        let mut saved_session = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_string();
        for resume in [false, true] {
            let l = Launch {
                files: String::new(),
                execution: None,
                task: 2427,
                title: "Invented fixture".into(),
                attempt: 42,
                role: "worker".into(),
                rt: rt.into(),
                session: saved_session.clone(),
                url: String::new(),
                resume: resume.then(|| "continue".into()),
            };
            let record = root.join(format!("{rt}-{resume}.json"));
            let log = root.join(format!("{rt}-{resume}.log"));
            let mut req = request(&env, &l, &log, &input, &access).unwrap();
            req.command = vec![python.clone().into(), fixture.clone().into()];
            let home = root.join(format!("fixture-home-{rt}"));
            std::fs::create_dir_all(&home).unwrap();
            req.home = Some(home);
            req.env.extend([
                ("TASKQ_FIXTURE_RT".into(), rt.into()),
                ("TASKQ_FIXTURE_RECORD".into(), record.clone().into()),
                ("PYTHONIOENCODING".into(), "utf-8".into()),
                ("TASKQ_FIXTURE_LINGER".into(), "1".into()),
            ]);
            assert!(!format!("{:?}", req.host_args).contains("invented-secret"));
            if resume {
                agent(rt).unwrap().adopt(&l.session, &req, &[]).unwrap();
            }
            let mut child = native(rt, resume.then_some(l.session.as_str()), &mut req).unwrap();
            child.install = Some(root.clone());
            let identity = child.native_identity().unwrap();
            let held = coreq::file_lock::lock(&root.join("taskq-profile.lock")).unwrap();
            let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                let began = std::time::Instant::now();
                if let Some(exit) = child.termination().unwrap() {
                    let elapsed = began.elapsed();
                    drop(held);
                    assert!(
                        elapsed < std::time::Duration::from_millis(500),
                        "terminal polling waited on telemetry: {elapsed:?}"
                    );
                    assert_eq!(
                        exit.code, None,
                        "cleanup never invents a numeric signal exit"
                    );
                    assert!(child.exited().unwrap_err().contains("exit code unknown"));
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
                        child.termination().unwrap().is_some(),
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
            let observed_session = agent(rt).unwrap().session_id(&child.run).unwrap();
            if resume {
                assert_eq!(observed_session, saved_session, "resume must retain the saved native identity");
            }
            saved_session = observed_session;
            let row: serde_json::Value =
                serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
            assert_eq!(row["input"], input);
            if rt == "antigravity" {
                assert_eq!(row["wire"]["method"], "session/prompt");
                assert_eq!(row["wire"]["params"]["sessionId"], l.session);
                assert!(
                    row["args"]
                        .as_array()
                        .unwrap()
                        .contains(&serde_json::json!("acp"))
                );
            }
            assert_eq!(row["env"]["TASKQ_ATTEMPT"], "42");
            assert_eq!(row["env"]["CARGO_BUILD_JOBS"], "2");
            assert_eq!(row["env"]["INVENTED_BOARD_SECRET"], "invented-secret");
            assert!(row["env"]["AGENTQ_OPTIMIZATION"].is_null());
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
                Some(saved_session.as_str())
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
        if resume {
            adapter.adopt(session, &request, &[]).unwrap();
        }
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
            let args = row["args"].as_array().unwrap();
            assert_eq!(args.iter().filter(|arg| *arg == "--settings").count(), 1);
            let at = args.iter().position(|arg| arg == "--settings").unwrap();
            let settings: serde_json::Value = serde_json::from_str(args[at + 1].as_str().unwrap()).unwrap();
            assert_eq!(settings["env"]["TASKQ_ATTEMPT"], "42");
            assert_eq!(settings["env"]["CARGO_BUILD_JOBS"], "2");
            assert_eq!(settings["env"]["AGENTQ_LAUNCHER"], "taskq");
            assert!(settings["env"]["AGENTQ_OPTIMIZATION"].is_null());
            assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "dontAsk"]));
            assert!(args.windows(2).any(|pair| pair == ["--name", "invented title"]));
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
            while !self.0.termination().is_ok_and(|exit| exit.is_some())
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
    assert_eq!(child.termination(), Ok(None));
    assert_eq!(
        coreq::process::identity(pid).unwrap(),
        Some(descendant.clone())
    );
    child.kill().unwrap();
    while child.termination().unwrap().is_none() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_ne!(coreq::process::identity(pid).unwrap(), Some(descendant));
    assert_eq!(
        child.termination(),
        Ok(Some(Exit { code: None })),
        "scope termination preserves the unknown numeric exit"
    );
    assert_eq!(
        coreq::process::identity(sentinel.id()).unwrap(),
        Some(sentinel.native_identity().unwrap())
    );
    sentinel.kill().unwrap();
    while sentinel.termination().unwrap().is_none() {
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
    let cancel_continuation = std::env::var("TASKQ_NATIVE_CANCEL_CONTINUATION").as_deref() == Ok("1");
    if cancel_continuation { assert_eq!(rt, CLAUDE, "owned background caller qualification is Claude-only"); }
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
    if cancel_continuation {
        assert!(std::env::var_os("TASKQ_NATIVE_ROOT").is_some(), "explicit disposable project required");
        assert!(install.starts_with(root) && install != root, "preauthorized disposable install must be inside project");
    }
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
        install: if cancel_continuation { &install } else { &fixture },
        ..authority
    };
    let log = evidence.join("native.log");
    File::create(&log).unwrap();
    let mut session = String::from("-");
    let mut rows = vec![];
    let mut submitted_inputs = vec![];
    for resume in [false, true] {
        let access = access(&authority, &rt).unwrap();
        let canary = format!("TaskQ-Qualification-2427-{}", uuid());
        let mut text = format!(
            "This is an approved non-task TaskQ Qualification probe. Reply with exactly {canary}. Do no work, edit no files, run no commands and make no taskq or board writes. Read this turn's input file if the launcher refers to it; use no other tools."
        );
        if cancel_continuation && resume {
            text = format!("Approved disposable qualification continuation {canary}. Produce a long numbered explanation of arithmetic, at least 20000 words. Use no tools, edit no files and make no board writes. The controller will cancel this turn after native acceptance.");
        }
        let l = Launch {
            files: String::new(),
            execution: cancel_continuation.then_some(if resume { 2 } else { 1 }),
            task: if cancel_continuation { if resume { 8 } else { 7 } } else { 0 },
            title: "TaskQ Qualification 2427".into(),
            attempt: if cancel_continuation { 1 } else { 0 },
            role: if cancel_continuation { "worker" } else { "operator" }.into(),
            rt: rt.clone(),
            session: session.clone(),
            url: String::new(),
            resume: resume.then(|| text.clone()),
        };
        std::fs::write(evidence.join(format!("input-{resume}.json")),serde_json::to_vec_pretty(&serde_json::json!({"runtime":rt,"resume":resume,"session":session,"input":text,"access":access.json()})).unwrap()).unwrap();
        if cancel_continuation {
            submitted_inputs.push(claude_reference(&execution_input(root, l.task, l.attempt, l.execution.unwrap()).unwrap()).unwrap());
        }
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
        let mut cancellation = None;
        let mut original = None;
        let cfg = crate::service::Config { root:root.into(), install:install.clone(), ..crate::tests::cfg() };
        let exit = loop {
            if cancel_continuation && cancellation.is_none() {
                proc.checkpoint(&cfg, &l, l.execution.unwrap()).expect("unknown checkpoint stops qualification; retain owned evidence");
                original = Some(proc.native_identity().expect("original creation identity required"));
                if resume && cancellation.is_none() && proc.accepted().unwrap() {
                    assert!(proc.termination().unwrap().is_none(), "continuation completed before cancel; no extra submission allowed");
                    let identity = original.as_ref().unwrap();
                    std::fs::write(evidence.join("cancel-original.json"), serde_json::to_vec_pretty(&serde_json::json!({"pid":identity.pid,"started":identity.started,"session":session})).unwrap()).unwrap();
                    let result = proc.kill();
                    std::fs::write(evidence.join("cancel-request.json"), serde_json::to_vec_pretty(&serde_json::json!({"result":result,"acceptance":true,"submissions":2})).unwrap()).unwrap();
                    if let Err(error) = &result {
                        assert_eq!(error, "Claude stop still pending; native process exit unconfirmed", "cancel refused/unknown; retain evidence without retry");
                    }
                    cancellation = Some(std::time::Instant::now());
                }
            }
            match proc.termination() {
                Ok(Some(exit)) => break exit.code,
                Ok(None) => {}
                Err(reason) => eprintln!("qualification observation pending: {reason}"),
            }
            if std::time::Instant::now() >= until {
                let cleanup = if cancel_continuation { Err("native qualification unknown; retain evidence, no repeated control".into()) } else { stop(proc.as_mut()) };
                panic!(
                    "native qualification timed out; cleanup={cleanup:?}; evidence={}",
                    evidence.display()
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        if cancel_continuation && resume {
            assert!(cancellation.is_some(), "continuation ended before cancellation; no extra submission allowed");
            let original = original.as_ref().unwrap();
            assert_ne!(coreq::process::identity(original.pid).unwrap().as_ref(), Some(original), "cancel acknowledgement is not process exit");
        }
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
        if cancel_continuation {
            let adapter = agent(&rt).unwrap();
            let mut source = adapter.history(&session, None, Some(&native), None).unwrap();
            let mut counts = vec![0; submitted_inputs.len()];
            for page_number in 0..16 {
                let page = source.poll().unwrap();
                for (input, count) in submitted_inputs.iter().zip(counts.iter_mut()) {
                    *count += page.events.iter().filter(|event| event["author"] == "user"
                        && event["blocks"].as_array().is_some_and(|blocks| blocks.iter().any(|block| block["text"].as_str().is_some_and(|value| value.trim() == input.trim())))).count();
                }
                if !page.more { assert_eq!(page.pending_bytes, 0, "partial source remains unknown"); break; }
                assert!(page_number < 15, "bounded history exhausted; no replay conclusion");
            }
            assert!(counts.iter().all(|count| *count == 1), "exactly one native input per original reference, including prior turn; no replay: {counts:?}");
            std::fs::write(evidence.join(format!("source-{resume}.json")), serde_json::to_vec_pretty(&source.checkpoint()).unwrap()).unwrap();
            if resume {
                rows.push(serde_json::json!({"resume":true,"session":session,"accepted":true,"exit":exit,"cancel_requested":true,"native_inputs":counts,"original_gone":true}));
                drop(proc);
                continue;
            }
        }
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

// R9/R12: canonical ACP must bind session/load before any prompt, replacing the native stream startup fence.
#[test]
fn acp_load_identity_mismatch_refuses_before_prompt() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("taskq-acp-load-{}", uuid()));
    std::fs::create_dir_all(root.join("home")).unwrap();
    let record = root.join("exchange.json");
    let mut request = Request::new(
        &root,
        root.join("source.jsonl"),
        "invented input that must not be sent",
        "taskq",
    );
    request.command = vec![
        python().into(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/runtime-launch.py")
            .into(),
    ];
    request.home = Some(root.join("home"));
    request.env = vec![
        ("TASKQ_FIXTURE_RT".into(), "antigravity".into()),
        ("TASKQ_FIXTURE_RECORD".into(), record.clone().into()),
        ("TASKQ_FIXTURE_WRONG_SESSION".into(), "1".into()),
    ];
    request.deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(20));
    let adapter = agent("antigravity").unwrap();
    let session = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    adapter.adopt(session, &request, &[]).unwrap();
    let error = adapter
        .resume(session, &request)
        .err()
        .expect("foreign session must refuse");
    assert!(error.detail.contains("loaded session differs"), "{error:?}");
    assert_eq!(error.stage, agentq_runtime::agent::InputStage::Unknown);
    assert_eq!(error.session.as_deref(), Some(session));
    let observed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
    assert_eq!(
        observed["messages"],
        serde_json::json!(["initialize", "session/load"])
    );
    assert!(
        observed["input"].is_null(),
        "identity refusal must happen before prompt/input"
    );
    std::fs::remove_dir_all(root).unwrap();
}
