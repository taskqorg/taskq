use super::*;
use crate::runtime::Cap;
use std::process::Stdio;

// Chain: §8, R2, R9, R11, R12, #2157 A1/A2 -> Codex resume keeps its bound CODEX_HOME without copying auth,
// releases a passive unheld writer lock on a completed turn, and retains live writers for no-start continuation retry.
#[test]
fn codex_resume_releases_passive_writer_and_preserves_bound_home_and_input() {
    let root = std::env::temp_dir().join(format!("taskq-2157-resume-{}", uuid()));
    let install = root.join("install");
    let base = root.join("codex-home");
    let work = base.join(crate::runtime::accounts::HOMES).join("work");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        base.join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"default-only"}}"#,
    )
    .unwrap();
    let thread = uuid();
    for (account, home) in [("default", &base), ("work", &work)] {
        let project = root.join(format!("project-{account}"));
        std::fs::create_dir_all(&project).unwrap();
        let sessions = home.join("sessions/2026/11/02");
        std::fs::create_dir_all(&sessions).unwrap();
        let rollout = sessions.join(format!("rollout-2026-11-02T00-00-00-{thread}.jsonl"));
        let lines = [
            serde_json::json!({"timestamp":"2026-11-02T00:00:00Z","type":"session_meta","payload":{"id":thread,"timestamp":"2026-11-02T00:00:00Z","cwd":project,"originator":"taskq-2157","cli_version":"0.159.3","source":"cli","model_provider":"openai"}}),
            serde_json::json!({"timestamp":"2026-11-02T00:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}),
            serde_json::json!({"timestamp":"2026-11-02T00:00:02Z","type":"event_msg","payload":{"type":"user_message","turn_id":"turn-1","message":"first turn"}}),
            serde_json::json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}),
        ];
        std::fs::write(
            &rollout,
            lines.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        let lock_dir = home.join("thread-writer-locks");
        std::fs::create_dir_all(&lock_dir).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&lock_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let lock_path = lock_dir.join(format!("{thread}.lock"));
        std::fs::write(&lock_path, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let env = Env {
            context: "",
            root: &project,
            exe: Path::new("taskq"),
            install: &install,
            permission_mode: "dontAsk",
            codex: &[],
            delivery: crate::config::Delivery::AutoMerge,
            board_env: &[],
            jobs: None,
        };
        let l = Launch {
            files: String::new(),
            execution: Some(44),
            task: 2157,
            title: String::new(),
            attempt: 42,
            role: "worker".into(),
            rt: "codex".into(),
            session: thread.clone(),
            url: String::new(),
            resume: Some("retained answer".into()),
        };
        let log = log_path(&project, &l);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        crate::runtime::accounts::mark(&log, account).unwrap();
        assert_eq!(crate::runtime::accounts::of_log(&log), account);
        let turn_spec = spec(&env, &l, project.clone(), vec![]);
        assert_eq!(turn_spec.session, thread);
        assert!(turn_spec.resume);
        assert!(
            !work.join("auth.json").exists(),
            "never copy auth.json across homes"
        );

        struct Exited;
        impl Proc for Exited {
            fn exited(&mut self) -> Result<Option<i32>, String> {
                Ok(Some(1))
            }
            fn kill(&mut self) -> Result<(), String> {
                Ok(())
            }
            fn id(&self) -> u32 {
                0
            }
        }
        let (path, _input) = turn_input(&project, "retained answer", Some(&l)).unwrap();
        let start = std::fs::metadata(&log).map_or(0, |m| m.len());
        bind_input(
            &log,
            &l,
            &path,
            "retained answer",
            serde_json::json!({"session":thread}),
        )
        .unwrap();
        let held_file = File::options()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        let held = coreq::file_lock::try_lock_file(held_file).unwrap();
        assert!(matches!(
            app_control::release_passive_writer(home, &project, &thread),
            app_control::PassiveWriterOutcome::Retained(_)
        ));
        assert!(lock_path.exists(), "live OS writer lock stays untouched");
        writeln!(File::options().append(true).open(&log).unwrap(),
                "2026-11-02T00:00:00Z ERROR codex_core::session: Failed to create session: thread-store conflict: thread {thread} already has an active writer\nError: thread/resume: thread/resume failed: thread {thread} already has an active writer (code -32600)").unwrap();
        let turn = CliTurn {
            proc: Box::new(Exited),
            log: log.clone(),
            pos: start,
            start,
            stopping: false,
            accepted: false,
            read_error: None,
            session: None,
            native: None,
            recovery: None,
            rt: "codex".into(),
        };
        assert!(turn.refused(l.task, l.attempt, 44, 1).is_some());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "retained answer",
            "retained input file stays intact across writer refusal"
        );
        drop(held);

        assert_eq!(
            app_control::release_passive_writer(home, &project, &thread),
            app_control::PassiveWriterOutcome::Released
        );
        assert!(
            !lock_path.exists(),
            "passive unheld writer lock on completed turn is released"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

// R12, #2143, #2157: qualify live writer retention/refusal and passive writer release against an owned native writer without auth, a model turn or app control.
#[cfg(unix)]
#[test]
#[ignore = "installed Codex writer probe; isolated home, no model"]
fn native_codex_writer_refusal_before_submission() {
    let root = std::env::temp_dir().join(format!("taskq-writer-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let native = agent(crate::runtime::RuntimeId::Codex.name())
        .unwrap()
        .executable(None)
        .unwrap();
    let script = r#"
import json,os,pathlib,selectors,subprocess,sys,time,uuid
home=pathlib.Path(sys.argv[2]);home.mkdir();thread=str(uuid.uuid4())
sessions=home/'sessions/2026/11/02';sessions.mkdir(parents=True)
rollout=sessions/f'rollout-2026-11-02T00-00-00-{thread}.jsonl'
events=[
 {'timestamp':'2026-11-02T00:00:00Z','type':'session_meta','payload':{'id':thread,'timestamp':'2026-11-02T00:00:00Z','cwd':sys.argv[3],'originator':'taskq-2143-fixture','cli_version':'0.159.3','source':'cli','model_provider':'openai'}},
 {'timestamp':'2026-11-02T00:00:01Z','type':'event_msg','payload':{'type':'task_started','turn_id':'turn-0'}},
 {'timestamp':'2026-11-02T00:00:02Z','type':'event_msg','payload':{'type':'user_message','turn_id':'turn-0','message':'completed fixture turn'}},
 {'timestamp':'2026-11-02T00:00:03Z','type':'event_msg','payload':{'type':'task_complete','turn_id':'turn-0'}},
]
rollout.write_text(''.join(json.dumps(e)+'\n' for e in events))
p=subprocess.Popen([sys.argv[1],'app-server'],env=dict(os.environ,CODEX_HOME=str(home)),stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
def call(i,method,params):
 p.stdin.write(json.dumps({'id':i,'method':method,'params':params})+'\n');p.stdin.flush()
 sel=selectors.DefaultSelector();sel.register(p.stdout,selectors.EVENT_READ);until=time.monotonic()+15
 while time.monotonic()<until:
  if not sel.select(max(0,until-time.monotonic())):break
  row=json.loads(p.stdout.readline())
  if row.get('id')==i:
   assert 'error' not in row,row
   return row['result']
 raise RuntimeError('native read deadline')
try:
 call(1,'initialize',{'clientInfo':{'name':'taskq-2143-probe','version':'1'}})
 call(2,'thread/resume',{'threadId':thread,'cwd':sys.argv[3]})
 print(thread,flush=True);sys.stdin.readline()
 row=call(3,'thread/read',{'threadId':thread,'includeTurns':True})['thread']
 assert len(row['turns'])==1,row
 print('writer retained; no turns submitted',flush=True)
finally:
 p.stdin.close()
 try:p.wait(timeout=10)
 except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=10)
"#;
    let home = root.join("codex-home");
    let mut owner = Command::new("python3")
        .args(["-c", script])
        .arg(&native)
        .arg(&home)
        .arg(&root)
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(owner.stdout.take().unwrap());
    let mut thread = String::new();
    reader.read_line(&mut thread).unwrap();
    let thread = thread.trim();
    assert!(plain_thread(thread));
    assert!(
        matches!(
            app_control::release_passive_writer(&home, &root, thread),
            app_control::PassiveWriterOutcome::Retained(_)
        ),
        "live native app-server writer lock must remain retained even on a completed turn"
    );
    let mut l = Launch {
        files: String::new(),
        execution: Some(44),
        task: 7,
        title: String::new(),
        attempt: 42,
        role: "worker".into(),
        rt: "codex".into(),
        session: thread.into(),
        url: String::new(),
        resume: Some("retained fixture input".into()),
    };
    let log = root.join("turn.log");
    for execution in [44, 45] {
        l.execution = Some(execution);
        let start = std::fs::metadata(&log).map_or(0, |m| m.len());
        let text = "TASKQ_2143_NO_AUTH_FIXTURE";
        let (path, _input) = turn_input(&root, text, Some(&l)).unwrap();
        bind_input(&log, &l, &path, text, serde_json::json!({"session":thread})).unwrap();
        let out = File::options().append(true).open(&log).unwrap();
        let mut cmd = Command::new(&native);
        cmd.env("CODEX_HOME", &home)
            .args([
                "exec",
                "--json",
                "--skip-git-repo-check",
                "resume",
                thread,
                "-",
            ])
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .current_dir(&root)
            .stdout(out.try_clone().unwrap())
            .stderr(out);
        let proc = start_file(&mut cmd, File::open(&path).unwrap()).unwrap();
        let mut turn = CliTurn {
            proc,
            log: log.clone(),
            pos: start,
            start,
            stopping: false,
            accepted: false,
            read_error: None,
            session: None,
            native: None,
            recovery: None,
            rt: "codex".into(),
        };
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let code = loop {
            if let Some(code) = turn.exited().unwrap() {
                break code;
            }
            assert!(
                std::time::Instant::now() < until,
                "native refusal timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_ne!(code, 0);
        assert_eq!(turn.accepted(), Ok(false));
        assert!(turn.refused(l.task, l.attempt, execution, code).is_some());
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
    drop(owner.stdin.take());
    let mut reply = String::new();
    reader.read_line(&mut reply).unwrap();
    assert_eq!(reply.trim(), "writer retained; no turns submitted");
    assert!(owner.wait().unwrap().success());
    let lock_dir = home.join("thread-writer-locks");
    std::fs::create_dir_all(&lock_dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&lock_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let passive_lock = lock_dir.join(format!("{thread}.lock"));
        std::fs::write(&passive_lock, b"").unwrap();
        std::fs::set_permissions(&passive_lock, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            app_control::release_passive_writer(&home, &root, thread),
            app_control::PassiveWriterOutcome::Released
        );
        assert!(!passive_lock.exists());
    }
    println!(
        "native writer retained through two refused resumes; passive unheld lock released after owner exit; original inputs preserved; acceptance false; no model turns"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// A2/A4, R9/R12: agy must receive its own protocol and literal brief, never Codex/Claude policy flags.
#[test]
fn antigravity_commands_and_input_preserve_identity_and_grants() {
    let root = rules_root("antigravity");
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
        (
            "ANTIGRAVITY_HOME".into(),
            root.join("invented-home").display().to_string(),
        ),
        ("AGENTQ_OPTIMIZATION".into(), "off".into()),
    ];
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("taskq"),
        install: &root,
        permission_mode: "bypassPermissions",
        codex: &args,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &vars,
        jobs: Some(2),
    };
    let host = crate::policy::Host {
        os: "macos",
        install: &root,
        root: &root,
        home: &root,
    };
    assert!(
        crate::policy::resolve(
            &Ok(crate::policy::Policy::default()),
            &host,
            "antigravity",
            env.permission_mode,
            &args
        )
        .refused
        .is_some(),
        "legacy requests cannot grant a new runtime"
    );
    let policy = Ok(crate::policy::Policy {
        revision: 1,
        grants: vec![crate::policy::Grant {
            runtime: "antigravity".into(),
            mode: "workspace".into(),
            project: None,
            by: "invented operator".into(),
            at: 1,
        }],
    });
    let access = crate::policy::resolve(&policy, &host, "antigravity", env.permission_mode, &args);
    assert!(access.refused.is_none());
    assert!(
        access
            .profile
            .as_deref()
            .unwrap()
            .contains(".gemini/antigravity")
    );
    assert!(
        access.codex.is_none() && access.claude.is_none(),
        "a runtime grant never translates another client's policy"
    );
    let unsupported = crate::policy::Host {
        os: "linux",
        install: &root,
        root: &root,
        home: &root,
    };
    assert!(
        crate::policy::resolve(
            &policy,
            &unsupported,
            "antigravity",
            env.permission_mode,
            &args
        )
        .refused
        .is_some(),
        "no Full fallback for unqualified containment"
    );
    let text = "Literal \"quote\", $HOME, `command`\nsecond line 🦀";
    let input: serde_json::Value = serde_json::from_str(&stdin_text("antigravity", text)).unwrap();
    assert_eq!(
        input,
        serde_json::json!({"event":"user","message":{"content":text}})
    );
    for mode in ["workspace", "full"] {
        let mut policy = policy.clone();
        policy.as_mut().unwrap().grants[0].mode = mode.into();
        let access =
            crate::policy::resolve(&policy, &host, "antigravity", env.permission_mode, &args);
        assert!(access.refused.is_none());
        assert_eq!(access.profile.is_some(), mode == "workspace");
        for role in ["worker", "reviewer"] {
            for resume in [false, true] {
                let launch = Launch {
                    files: String::new(),
                    execution: None,
                    task: 2136,
                    title: "Invented fixture".into(),
                    attempt: 42,
                    role: role.into(),
                    rt: "antigravity".into(),
                    session: if resume { "agy-owned" } else { "-" }.into(),
                    url: String::new(),
                    resume: resume.then(|| text.into()),
                };
                let command = command(&env, &launch, &access).unwrap();
                let actual: Vec<_> = command
                    .get_args()
                    .map(|v| v.to_string_lossy().into_owned())
                    .collect();
                let protocol = actual.iter().position(|v| v == "--input-format").unwrap();
                let mut expected = vec![
                    "--input-format",
                    "stream-json",
                    "--output-format",
                    "stream-json",
                    "--dangerously-skip-permissions",
                ];
                if resume {
                    expected.extend(["--conversation", "agy-owned"]);
                }
                assert_eq!(&actual[protocol..], expected, "{role} resume={resume}");
                assert!(
                    !actual
                        .iter()
                        .any(|v| v == "invented-model" || v == "bypassPermissions" || v == "exec")
                );
                assert_eq!(command.get_current_dir(), Some(root.as_path()));
                assert!(command.get_envs().any(|(k,v)|k=="TASKQ_ATTEMPT" && v==Some(std::ffi::OsStr::new("42"))));
            }
        }
    }
    let log = root.join("native.log");
    std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"other-runtime\"}\n{\"conversationId\":\"agy-owned\"}\n").unwrap();
    assert_eq!(
        native_session("antigravity", &log).as_deref(),
        Some("agy-owned")
    );
    assert!(
        of("antigravity").has(Cap::Cli)
            && !of("antigravity").has(Cap::AppTurn)
            && !of("antigravity").has(Cap::RemoteControl)
    );
    assert!(agent_runtimes().contains(&"antigravity"));
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: R9/R12, #2145 A3/A4. Risk: a service PATH misses agy or credential presence stays unknown.
#[test]
fn antigravity_cli_state_and_user_bin_lookup() {
    if let Some(home) = std::env::var_os("TASKQ_AGY_FIXTURE") {
        let home = PathBuf::from(home);
        let bin = home.join(".local/bin/agy");
        assert_eq!(which("agy"), bin);
        assert_eq!(login_state("antigravity", &bin), "unknown");
        for name in [
            ".gemini/antigravity-cli/jetski_state.pbtxt",
            ".gemini/oauth_creds.json",
        ] {
            let path = home.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::create_dir(&path).unwrap();
            assert_eq!(
                login_state("antigravity", &bin),
                "unknown",
                "a directory is not credentials"
            );
            std::fs::remove_dir(&path).unwrap();
            std::fs::write(&path, "invented fixture").unwrap();
            for rt in ["antigravity", "agy"] {
                assert_eq!(login_state(rt, &bin), "logged in", "{name}");
            }
            let rows = agents(&home);
            let row = rows
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["runtime"] == "antigravity")
                .unwrap();
            assert_eq!(row["login"], "logged in");
            std::fs::remove_file(path).unwrap();
        }
        assert_eq!(login_state("antigravity", &bin), "unknown");
        return;
    }
    let root = rules_root("agy-cli-state");
    std::fs::create_dir_all(root.join(".local/bin")).unwrap();
    std::fs::write(root.join(".local/bin/agy"), "invented executable fixture").unwrap();
    #[cfg(not(all(windows, target_env = "gnu")))]
    let exe = std::env::current_exe().unwrap();
    // GNU needs its loader beside the child when this fixture removes Cargo's PATH.
    #[cfg(all(windows, target_env = "gnu"))]
    let exe = {
        let tool = crate::platform::tool(&root.join("install"));
        crate::platform::prepare_install(&tool).unwrap();
        tool.release_exe(tool.version)
    };
    let out = Command::new(exe)
        .args([
            "--exact",
            "runtime::tests::antigravity_cli_state_and_user_bin_lookup",
            "--nocapture",
        ])
        .env("TASKQ_AGY_FIXTURE", &root)
        .env("HOME", &root)
        .env("USERPROFILE", &root)
        .env("PATH", root.join("missing-bin"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(root).unwrap();
}

// A2/A4, R11/R12: old terminal events cannot stop a resumed agy turn; only its own terminal confirms stop.
#[test]
fn antigravity_turn_uses_its_log_boundary_and_preserves_quota_unknowns() {
    struct Pending(bool);
    impl Proc for Pending {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(self.0.then_some(0))
        }
        fn kill(&mut self) -> Result<(), String> {
            self.0 = true;
            Ok(())
        }
        fn id(&self) -> u32 {
            1
        }
    }
    let root = rules_root("antigravity-turn");
    let log = root.join("turn.log");
    let old = "{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\"}}\n";
    std::fs::write(&log, old).unwrap();
    let mut turn = CliTurn {
        proc: Box::new(Pending(false)),
        log: log.clone(),
        pos: old.len() as u64,
        start: old.len() as u64,
        stopping: false,
        accepted: false,
        read_error: None,
        session: None,
        native: None,
        recovery: None,
        rt: "antigravity".into(),
    };
    assert_eq!(turn.exited(), Ok(None));
    assert_eq!(turn.accepted(), Ok(false));
    writeln!(
        File::options().append(true).open(&log).unwrap(),
        "{}",
        serde_json::json!({"event":"step_update","step_update":{"step_type":"tool"}})
    )
    .unwrap();
    assert_eq!(turn.exited(), Ok(None));
    assert_eq!(turn.accepted(), Ok(true));
    writeln!(
        File::options().append(true).open(&log).unwrap(),
        "{}",
        serde_json::json!({"event":"result","result":{"status":"SUCCESS"}})
    )
    .unwrap();
    assert_eq!(turn.exited(), Ok(Some(0)));
    std::fs::write(&log, "{\"conversationId\":\"foreign-conversation\"}\n").unwrap();
    let mut copied = CliTurn {
        proc: Box::new(Pending(false)),
        log: log.clone(),
        pos: 0,
        start: 0,
        stopping: false,
        accepted: true,
        read_error: None,
        session: Some("owned-conversation".into()),
        native: None,
        recovery: None,
        rt: "antigravity".into(),
    };
    assert!(
        copied
            .exited()
            .unwrap_err()
            .contains("conversation changed")
    );
    assert!(
        copied.accepted().is_err() && !copied.stopping,
        "another conversation cannot end this turn"
    );
    assert!(
        copied.exited().is_err(),
        "a later poll must not skip the conflicting identity"
    );
    let quota =
        serde_json::json!({"event":"result","result":{"status":"ERROR","error":"quota exceeded"}})
            .to_string()
            + "\n";
    std::fs::write(&log, &quota).unwrap();
    assert_eq!(
        usage_limit(&log, "antigravity"),
        Some(None),
        "no invented reset or quota window"
    );
    std::fs::write(
        &log,
        format!(
            "{{\"event\":\"step_update\",\"step_update\":{{\"step_type\":\"tool\"}}}}\n{quota}"
        ),
    )
    .unwrap();
    assert_eq!(
        usage_limit(&log, "antigravity"),
        None,
        "mid-work quota must not become a pre-work retry"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// A4/R9: launch intent must preserve client policy and identity, never enable or rewrite an integration.
#[test]
fn optimization_preserves_policy_and_reports_effective_state() {
    use agentq_runtime::optimization::Intent;
    let root = rules_root("optimization");
    let home = root.join("home");
    let install = root.join("optimizer");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(install.join("data")).unwrap();
    let config = "model_provider = \"agentq\"\n";
    std::fs::write(home.join("config.toml"), config).unwrap();
    std::fs::write(
        home.join("settings.json"),
        r#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"agentq hook claude pre"}]}]}}"#,
    )
    .unwrap();
    for name in ["claude", "codex"] {
        for (intent, effective) in [("inherit", "on"), ("off", "off"), ("on", "on")] {
            for resume in [false, true] {
                let mut spec = NativeSpec {
                    root: root.clone(),
                    session: "saved-native-id".into(),
                    resume,
                    args: vec!["--permission-mode".into(), "dontAsk".into()],
                    home: Some(home.clone()),
                    env: vec![
                        ("AGENTQ_OPTIMIZATION".into(), intent.into()),
                        ("TASKQ_TASK".into(), "2111".into()),
                    ],
                };
                let state = optimization(name, &mut spec, Some(&install)).unwrap();
                assert_eq!(
                    (state.intent, state.effective),
                    (Intent::parse(intent).unwrap(), effective)
                );
                assert_eq!(spec.session, "saved-native-id");
                assert_eq!((spec.resume, spec.home.as_ref()), (resume, Some(&home)));
                assert_eq!(&spec.args[..2], &["--permission-mode", "dontAsk"]);
                assert_eq!(
                    spec.args.len(),
                    if name == "claude" || intent == "off" {
                        4
                    } else {
                        2
                    }
                );
                if intent == "off" && name == "codex" {
                    assert_eq!(
                        spec.args[3],
                        "model_providers.agentq.http_headers.x-agentq-optimization=\"off\""
                    );
                }
                if intent == "on" {
                    assert_eq!(
                        optimization(name, &mut spec, None).unwrap().reason,
                        "not_installed"
                    );
                    std::fs::write(
                        install.join("data/config.json"),
                        r#"{"compression":{"enabled":false}}"#,
                    )
                    .unwrap();
                    assert_eq!(
                        optimization(name, &mut spec, Some(&install))
                            .unwrap()
                            .effective,
                        "off"
                    );
                    std::fs::remove_file(install.join("data/config.json")).unwrap();
                }
                spec.env
                    .push(("AGENTQ_OPTIMIZATION".into(), "invalid".into()));
                assert!(
                    optimization(name, &mut spec, Some(&install))
                        .unwrap_err()
                        .contains("inherit, off or on")
                );
            }
        }
    }
    assert_eq!(
        std::fs::read_to_string(home.join("config.toml")).unwrap(),
        config
    );
    assert!(!install.join("data/integration.json").exists());
    std::fs::remove_dir_all(root).unwrap();
}

// A3/A4/R9: a daemon-backed start carries session env; a resume passes no flags, because Claude copies a job resumed
// with flags into a new conversation (#2262), so the job keeps the settings saved at its start.
#[cfg(unix)]
#[test]
fn optimization_reaches_background_settings_and_operator_attribution() {
    use std::os::unix::fs::PermissionsExt;
    let client = std::env::temp_dir().join(format!("taskq-background-args-{}", uuid()));
    std::fs::write(
        &client,
        "#!/bin/sh\ncat >/dev/null\nfor a; do printf '%s\\n' \"$a\"; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
    for resume in [false, true] {
        let mut spec = NativeSpec {
            root: std::env::temp_dir(),
            session: "saved-native-id".into(),
            resume,
            args: vec![
                "--settings".into(),
                r#"{"env":{"TASKQ_INSTALL":"invented"},"hooks":{"SessionStart":[]}}"#.into(),
                "--permission-mode".into(),
                "dontAsk".into(),
                "--name".into(),
                "retained name".into(),
            ],
            home: None,
            env: vec![("AGENTQ_OPTIMIZATION".into(), "off".into())],
        };
        optimization("claude", &mut spec, None).unwrap();
        let out = agent("claude")
            .unwrap()
            .background_launch(
                Command::new(&client),
                &spec,
                "fixture input",
                std::time::Duration::from_secs(10),
            )
            .unwrap()
            .unwrap();
        let args: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        if resume {
            assert_eq!(
                args,
                ["--bg", "--resume", "saved-native-id"],
                "a flagged resume starts a copy job (#2262)"
            );
            continue;
        }
        assert_eq!(
            args.iter().filter(|a| a.as_str() == "--settings").count(),
            1
        );
        let settings: serde_json::Value =
            serde_json::from_str(&args[args.iter().position(|a| a == "--settings").unwrap() + 1])
                .unwrap();
        assert_eq!(settings["env"]["AGENTQ_OPTIMIZATION"], "off");
        assert_eq!(settings["env"]["AGENTQ_LAUNCHER"], "taskq");
        assert_eq!(settings["env"]["TASKQ_INSTALL"], "invented");
        assert_eq!(settings["hooks"]["SessionStart"], serde_json::json!([]));
        assert!(
            args.windows(2)
                .any(|a| a == ["--permission-mode", "dontAsk"])
        );
        assert!(args.windows(2).any(|a| a == ["--name", "retained name"]));
    }
    std::fs::remove_file(client).unwrap();
}

// Risk: Windows `explorer` exits 1 after a successful open, so a Codex app queue reads it as a failed open (#1593).
#[test]
fn open_exit_rule() {
    assert!(opened(true, Some(0)) && opened(true, Some(1)) && opened(false, Some(0)));
    assert!(!opened(false, Some(1)) && !opened(true, Some(2)) && !opened(true, None));
}

// Chain: R3 session name -> Claude `--name`/Remote Control and Codex first input line (#1564). Risk: sessions
// show the shared rules header or an uncut title instead of role, task and title.
#[test]
fn session_name_is_role_task_and_cut_title() {
    let l = |role: &str, title: &str| Launch {
        files: String::new(),
        execution: None,
        task: 1561,
        title: title.into(),
        attempt: 1,
        role: role.into(),
        rt: "codex".into(),
        session: "s".into(),
        url: String::new(),
        resume: None,
    };
    assert_eq!(
        session_name(&l("worker", "Claude handover identity")),
        "work #1561 Claude handover identity"
    );
    assert_eq!(session_name(&l("reviewer", "a\n b")), "review #1561 a b");
    assert_eq!(session_name(&l("operator", "x")), "setup #1561 x");
    let long = "Name worker sessions by role, task and title everywhere";
    assert_eq!(
        session_name(&l("worker", long)),
        format!(
            "work #1561 {}",
            long.chars().take(40).collect::<String>().trim_end()
        )
    );
    let cyr = "Имя сессии воркера: роль, задача и заголовок ✓ длинный хвост";
    let name = session_name(&l("worker", cyr));
    assert_eq!(
        name.strip_prefix("work #1561 ").unwrap().chars().count(),
        40
    );
    assert!(cyr.starts_with(name.strip_prefix("work #1561 ").unwrap()));
}

// Chain: board session -> Remote Control lookup and Codex app link. Risk: a Codex session is probed as Claude.
#[test]
fn session_links_follow_the_adapter() {
    assert_eq!(remote_session("claude:abc mac"), Some("abc"));
    assert_eq!(
        (
            remote_session("codex:abc mac"),
            remote_session("codex-app:abc"),
            remote_session("claude")
        ),
        (None, None, None)
    );
    assert_eq!(thread_of(&thread_url("019a-b2")), Some("019a-b2"));
}

// #2137 A5/A7: structured window metadata survives overlap; AgentQ classifies agy errors; old turns stay old.
#[test]
fn balancing_quota_observations_keep_windows() {
    let log = std::env::temp_dir().join(format!("taskq-quota-metadata-{}", uuid()));
    std::fs::write(&log,"{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"status\":\"rejected\",\"rateLimitType\":\"seven_day\",\"resetsAt\":604800}}\n").unwrap();
    assert_eq!(
        quota_windows(&log, "claude"),
        [("seven_day".into(), Some(604800))]
    );
    std::fs::write(&log,"{\"type\":\"rate_limits\",\"rate_limits\":{\"primary\":{\"used_percent\":100,\"window_minutes\":300,\"resets_at\":18000}}}\n").unwrap();
    assert_eq!(quota_windows(&log, "codex"), [("300m".into(), Some(18000))]);
    rules_line(&log, "resume", "fixture input").unwrap();
    assert!(
        quota_windows(&log, "codex").is_empty(),
        "a successful later turn must not reuse old quota events"
    );
    std::fs::write(
        &log,
        "{\"event\":\"result\",\"result\":{\"status\":\"ERROR\",\"error\":\"quota exceeded\"}}\n",
    )
    .unwrap();
    assert_eq!(
        quota_windows(&log, "antigravity"),
        [("unknown".into(), None)],
        "no guessed agy reset/window"
    );
    std::fs::remove_file(log).unwrap();
}

// Chain: attempt log -> usage limit before work -> reset. Risk: a limit after work started skips its retry,
// or a limit before work counts against the gone limit (#1276).
#[test]
fn usage_limit_only_before_work() {
    let log = std::env::temp_dir().join(format!("taskq-usage-limit-{}.log", uuid()));
    let limit = r#"{"type":"assistant","apiError":"usage_limit_reached","quotaLimits":{"status":"rejected","resetsAt":1792386000},"message":{"content":[{"type":"text","text":"You've hit your weekly limit"}]}}"#;
    let work = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#;
    let cases = [
            (format!("{{\"type\":\"system\",\"subtype\":\"init\"}}\n{limit}\n{{\"type\":\"result\",\"is_error\":true}}\n"), Some(Some(1792386000))),
            (format!("{work}\n{limit}\n"), None),
            (r#"{"type":"assistant","apiError":"usage_limit_reached","apiErrorParams":{"rate_limit_info":{"resetsAt":7}}}"#.to_string(), Some(Some(7))),
            ("{\"type\":\"thread.started\"}\n{\"type\":\"turn.started\"}\n{\"type\":\"error\",\"message\":\"You've hit your usage limit. Try again later.\"}\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"You've hit your usage limit.\"}}\n".to_string(), Some(None)),
            ("{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\"}}\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"usage limit\"}}\n".to_string(), None),
            ("{\"type\":\"turn.failed\",\"error\":{\"message\":\"stream disconnected\"}}\n".to_string(), None),
        ];
    for (text, want) in cases {
        std::fs::write(&log, &text).unwrap();
        assert_eq!(
            usage_limit(
                &log,
                if text.contains("\"turn.") || text.contains("thread.") {
                    "codex"
                } else {
                    "claude"
                }
            ),
            want,
            "{text}"
        );
    }
    let _ = std::fs::remove_file(&log);
    assert_eq!(
        usage_limit(&log, "claude"),
        None,
        "an unreadable log is no limit"
    );
}

// Chain: attempt log -> last turn failed on model capacity -> continuing steer (#1843).
// Risk: a mid-work capacity failure ends the attempt as `gone`, or a later completed turn or other error is retried.
#[test]
fn transient_failure_reads_the_last_turn() {
    let log = std::env::temp_dir().join(format!("taskq-transient-{}.log", uuid()));
    let failed = r#"{"type":"turn.failed","error":{"message":"Error running remote compact task: Selected model is at capacity. Please try a different model."}}"#;
    let work = r#"{"type":"item.completed","item":{"type":"command_execution"}}"#;
    let cases = [
        (
            format!(
                "{{\"type\":\"thread.started\",\"thread_id\":\"t\"}}
{work}
{{\"type\":\"error\",\"message\":\"Selected model is at capacity. Please try a different model.\"}}
{failed}
"
            ),
            Some(
                "Error running remote compact task: Selected model is at capacity. Please try a different model.",
            ),
        ),
        (
            format!(
                "{failed}
{{\"type\":\"turn.started\"}}
{work}
{{\"type\":\"turn.completed\"}}
"
            ),
            None,
        ),
        (
            format!(
                "{work}
{{\"type\":\"turn.failed\",\"error\":{{\"message\":\"stream disconnected\"}}}}
"
            ),
            None,
        ),
        (
            format!(
                "{work}
{{\"type\":\"error\",\"message\":\"Selected model is at capacity.\"}}
"
            ),
            None,
        ),
    ];
    for (text, want) in cases {
        std::fs::write(&log, &text).unwrap();
        assert_eq!(transient_failure(&log, "codex").as_deref(), want, "{text}");
    }
    let _ = std::fs::remove_file(&log);
    assert_eq!(
        transient_failure(&log, "codex"),
        None,
        "an unreadable log is no failure"
    );
}

// Chain: #2198 § 4 step 1 -> Claude log -> live background job at turn end -> continuing steer.
// Risk: a turn that ended while its own check ran goes `gone` again (#2176, #2180), or a finished,
// stopped or never-started job or an earlier turn's job keeps resuming the worker.
#[test]
fn live_background_reads_jobs_the_last_turn_left_running() {
    let log = std::env::temp_dir().join(format!("taskq-background-{}.log", uuid()));
    let start = r#"{"type":"taskq.rules","phase":"start"}"#;
    let resume = r#"{"type":"taskq.rules","phase":"resume"}"#;
    let bash = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"python3 scripts/check.py","description":"Full check","run_in_background":true}}]}}"#;
    let fg = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"git status"}}]}}"#;
    let fg_done = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_2","content":"clean"}]}}"#;
    let started = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Command running in background with ID: b7x9. Output is being written to: out"}]}}"#;
    let failed = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"denied"}]}}"#;
    let monitor = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_3","name":"Monitor","input":{"description":"CI watch","command":"gh pr checks 1"}}]}}"#;
    let monitor_started = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_3","content":[{"type":"text","text":"Monitor started (task m42, expires in 20m)"}]}]}}"#;
    let done = r#"{"type":"queue-operation","content":"<task-notification>\n<task-id>b7x9</task-id>\n<tool-use-id>toolu_1</tool-use-id>\n<status>completed</status>\n</task-notification>"}"#;
    let running = r#"{"type":"queue-operation","content":"<task-notification>\n<task-id>b7x9</task-id>\n<status>running</status>\n</task-notification>"}"#;
    let killed = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_4","name":"KillShell","input":{"shell_id":"b7x9"}}]}}"#;
    let cases: [(Vec<&str>, Vec<&str>); 8] = [
        (vec![start, fg, fg_done, bash, started], vec!["Bash"]),
        (vec![start, bash, started, running], vec!["Bash"]),
        (
            vec![start, bash, started, monitor, monitor_started],
            vec!["Bash", "Monitor"],
        ),
        (vec![start, bash, started, done], vec![]),
        (vec![start, bash, started, killed], vec![]),
        (vec![start, bash, failed], vec![]),
        (vec![start, bash, started, resume, fg, fg_done], vec![]),
        (vec![start, fg, fg_done], vec![]),
    ];
    for (lines, want) in cases {
        let text = lines.join("\n") + "\n";
        std::fs::write(&log, &text).unwrap();
        assert_eq!(live_background(&log, "claude"), want, "{text}");
    }
    let _ = std::fs::remove_file(&log);
    assert!(
        live_background(&log, "claude").is_empty(),
        "an unreadable log has no live job"
    );
}

// Chain: #2198 worker rule -> full check in the foreground. Risk: the brief drops the rule or its cause.
#[test]
fn worker_brief_runs_the_full_check_in_the_foreground() {
    for part in [
        "foreground",
        "600000 ms",
        "never run_in_background or Monitor",
        "does not wait for its own background jobs",
        "gone",
    ] {
        assert!(DEV_LOOP.contains(part), "{part}");
    }
}

// Chain: actual installed Codex entrypoint -> native PID -> stop -> observed exit.
// No prompt is sent; EOF cleans up even if an assertion fails before cancellation.
#[cfg(any(unix, windows))]
#[test]
#[ignore = "owned installed CLI packaging probe; no model or prompt"]
fn native_codex_executable_stop_without_model() {
    let entry = which("codex");
    let native = agent(crate::runtime::RuntimeId::Codex.name())
        .unwrap()
        .executable(None)
        .unwrap();
    let version = coreq::process::output(
        "qualification.codex-version",
        Command::new(&native).arg("--version"),
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    let mut child = Command::new(&native)
        .args(["exec", "--json", "-"])
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let identity = coreq::process::identity(child.id()).unwrap().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(child.try_wait().unwrap(), None);
    assert_eq!(
        coreq::process::identity(child.id()).unwrap(),
        Some(identity.clone())
    );
    coreq::process::terminate(&identity).unwrap();
    child.wait().unwrap();
    assert_eq!(coreq::process::identity(identity.pid).unwrap(), None);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.stdout.is_empty(),
        "no native turn should start without input"
    );
    println!(
        "{}entry={} native={} pid={} native exit confirmed; no model dispatched",
        String::from_utf8_lossy(&version.stdout),
        entry.display(),
        native.display(),
        identity.pid
    );
}

// Chain: actual native Codex -> owner crash -> cold adoption -> cancellation/exit receipt.
// The controller holds stdin open; no prompt, model, tools or live board are involved.
#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "owned installed CLI cold-recovery probe; no model or prompt"]
fn native_codex_cold_owner_restart() {
    let only = std::env::var("TASKQ_NATIVE_OWNER_ONLY").ok();
    assert!(
        only.as_deref()
            .is_none_or(|phase| matches!(phase, "cancel" | "ended")),
        "invalid native fixture selector"
    );
    for phase in ["cancel", "ended"] {
        if only.as_deref().is_some_and(|selected| selected != phase) {
            continue;
        }

        let root = std::env::temp_dir().join(format!("taskq-native-owner-{phase}-{}", uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "runtime::tests::native_codex_owner_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_NATIVE_OWNER_ROOT", &root)
            .env("TASKQ_NATIVE_OWNER_CASE", phase);
        let mut old = command
            .env("TASKQ_NATIVE_OWNER_PHASE", "before")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut input = Some(old.stdin.take().unwrap());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !root.join("ready").exists() {
            assert!(
                old.try_wait().unwrap().is_none(),
                "owner exited before checkpoint"
            );
            assert!(
                std::time::Instant::now() < until,
                "native checkpoint timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("ready")).unwrap()).unwrap();
        let identity = coreq::process::Identity {
            pid: saved["pid"].as_u64().unwrap() as u32,
            started: saved["started"].as_str().unwrap().into(),
        };
        old.kill().unwrap();
        old.wait().unwrap();
        assert_eq!(
            coreq::process::identity(identity.pid).unwrap(),
            Some(identity.clone()),
            "the native executor must survive its owner for this case"
        );
        if phase == "ended" {
            drop(input.take());
            while !coreq::process::identity(identity.pid).is_ok_and(|p| p.is_none()) {
                assert!(
                    std::time::Instant::now() < until,
                    "native exit after EOF timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let result = coreq::process::output(
            "qualification.native-cold-owner",
            command
                .env("TASKQ_NATIVE_OWNER_PHASE", "after")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
            std::time::Duration::from_secs(10),
        );
        drop(input);
        assert!(result.is_ok(), "{phase}: {result:?}");
        let output = result.unwrap();
        for line in String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| l.starts_with("intermediate native readback"))
        {
            println!("{phase}: {line}");
        }
        assert_eq!(coreq::process::identity(identity.pid).unwrap(), None);
        let expected = if cfg!(windows) && phase == "ended" {
            "vanished kill-on-close scope; ended; Exit released; record retired; no replay"
        } else {
            "same native execution; acceptance unknown; exit read back; no replay"
        };
        assert_eq!(
            std::fs::read_to_string(root.join("verified")).unwrap(),
            expected
        );
        println!(
            "{phase}: native pid={} survived owner crash; {expected}; no model dispatched",
            identity.pid
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "native CLI owner process fixture"]
fn native_codex_owner_child() {
    use crate::board::Board;
    let root = PathBuf::from(std::env::var_os("TASKQ_NATIVE_OWNER_ROOT").unwrap());
    let b = crate::board::Fake::default();
    let task = b
        .create("native fixture", "", &[crate::service::TASK_LABEL.into()])
        .unwrap();
    let attempt = b
        .comment(task, "taskq: spawn worker codex:fixture mac")
        .unwrap();
    let cfg = crate::service::Config {
        root: root.clone(),
        install: root.join("install"),
        ..crate::tests::cfg()
    };
    let mut service = crate::service::Service::new(
        &b,
        &cfg,
        "native-fixture",
        Box::new(|_| panic!("native recovery must not dispatch")),
    );
    if std::env::var("TASKQ_NATIVE_OWNER_PHASE").unwrap() == "before" {
        let log = attempt_log(&root, "worker", task, attempt);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        let output = File::create(&log).unwrap();
        let mut command = Command::new(
            agent(crate::runtime::RuntimeId::Codex.name())
                .unwrap()
                .executable(None)
                .unwrap(),
        );
        command
            .args(["exec", "--json", "-"])
            .current_dir(&root)
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .stdin(Stdio::inherit())
            .stderr(output.try_clone().unwrap())
            .stdout(output);
        #[cfg(windows)]
        let child: Box<dyn Proc> =
            Box::new(windows_process::Execution::spawn(&mut command).unwrap());
        #[cfg(not(windows))]
        let child: Box<dyn Proc> = Box::new(command.spawn().unwrap());
        let native = child.native_identity().unwrap();
        let launch = Launch {
            files: String::new(),
            execution: Some(attempt),
            task,
            title: String::new(),
            attempt,
            role: "worker".into(),
            rt: "codex".into(),
            session: "fixture".into(),
            url: String::new(),
            resume: None,
        };
        let mut proc = CliTurn {
            proc: child,
            log,
            pos: 0,
            start: 0,
            stopping: false,
            accepted: false,
            read_error: None,
            session: None,
            native: Some(native.clone()),
            recovery: None,
            rt: "codex".into(),
        };
        proc.checkpoint(&cfg, &launch, attempt).unwrap();
        service.kids.push(crate::service::Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            task,
            attempt,
            execution: attempt,
            rt: "codex".into(),
            proc: Box::new(proc),
        });
        crate::data::kids(
            &std::fs::read_to_string(crate::paths::kids_path(&cfg.install, &root).unwrap())
                .unwrap(),
        )
        .unwrap();
        coreq::write_durable(
            &root.join("ready"),
            &serde_json::json!({"pid":native.pid,"started":native.started}).to_string(),
        )
        .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            assert!(
                std::time::Instant::now() < until,
                "controller did not kill the owner"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[cfg(windows)]
    if std::env::var("TASKQ_NATIVE_OWNER_CASE").unwrap() == "ended" {
        // #2266 A1: owner and launcher killed -> the kill-on-close job is gone -> adopted as ended -> Exit release.
        crate::service_run::adopt(&mut service, &cfg.install, &root).unwrap();
        assert_eq!(service.kids.len(), 1);
        assert_eq!(service.kids[0].proc.exited(), Ok(Some(-1)));
        service.reap_owned();
        assert!(service.kids.is_empty());
        assert_eq!(
            crate::core::executions(task, &crate::board::entries(&b, task).unwrap())[0].release,
            Some(crate::core::Release::Exit)
        );
        assert!(
            crate::service_run::kids_file(&cfg.install, &root)
                .unwrap()
                .is_empty()
        );
        std::fs::write(
            root.join("verified"),
            "vanished kill-on-close scope; ended; Exit released; record retired; no replay",
        )
        .unwrap();
        return;
    }
    crate::service_run::adopt(&mut service, &cfg.install, &root).unwrap();
    assert_eq!(service.kids.len(), 1);
    assert_eq!(service.kids[0].execution, attempt);
    if std::env::var("TASKQ_NATIVE_OWNER_CASE").unwrap() == "cancel" {
        service.reap_owned();
        assert_eq!(
            service.kids.len(),
            1,
            "a live executor cannot release on owner restart"
        );
        assert!(
            !crate::core::executions(task, &crate::board::entries(&b, task).unwrap())[0].accepted
        );
        service.kids[0].proc.kill().unwrap();
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut unknown = std::collections::BTreeSet::new();
    loop {
        match service.kids[0].proc.exited() {
            Ok(Some(_)) => break,
            Err(error) => {
                unknown.insert(error);
            }
            Ok(None) => {}
        }
        assert!(
            std::time::Instant::now() < until,
            "native stop did not finish: {unknown:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!("intermediate native readback errors (never terminal): {unknown:?}");
    b.state.borrow_mut().fail_comments = 1;
    service.reap_owned();
    assert_eq!(
        service.kids.len(),
        1,
        "lost exit receipt must retain ownership"
    );
    assert_eq!(
        crate::service_run::kids_file(&cfg.install, &root)
            .unwrap()
            .len(),
        1
    );
    service.reap_owned();
    assert!(service.kids.is_empty());
    let x = crate::core::executions(task, &crate::board::entries(&b, task).unwrap()).remove(0);
    assert!(!x.accepted);
    assert_eq!(x.release, Some(crate::core::Release::Exit));
    assert!(
        crate::service_run::kids_file(&cfg.install, &root)
            .unwrap()
            .is_empty()
    );
    std::fs::write(
        root.join("verified"),
        "same native execution; acceptance unknown; exit read back; no replay",
    )
    .unwrap();
}

// Chain: prepared brief -> native stdin -> consumer or non-reading child, with no blocking producer.
#[test]
fn file_stdin_handoff_preserves_bytes_and_never_waits_for_a_reader() {
    let root = std::env::temp_dir().join(format!("taskq-input-handoff-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let text = format!("{}EOF-CANARY", "private-input-".repeat(12_000));
    for mode in ["blocked", "read"] {
        let go = root.join(mode);
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "runtime::tests::stdin_child", "--ignored"])
            .env("TASKQ_STDIN_MODE", mode)
            .env("TASKQ_STDIN_GO", &go)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let began = std::time::Instant::now();
        let mut child = start_command(&mut command, &root, &text).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            began.elapsed() < std::time::Duration::from_secs(3),
            "handoff waited for the child to read"
        );
        for entry in std::fs::read_dir(root.join(".taskq/inputs")).unwrap() {
            let path = entry.unwrap().path();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            std::fs::remove_file(path).unwrap();
        }
        if mode == "blocked" {
            let _pending = stop(child.as_mut());
        } else {
            std::fs::write(&go, "read now").unwrap();
        }
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.exited().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= until {
                let cleanup = stop(child.as_mut());
                panic!("input fixture did not finish; cleanup: {cleanup:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        if mode == "read" {
            assert!(
                status == 0,
                "child could not read the original bytes after cache unlink"
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "file stdin handoff fixture"]
fn stdin_child() {
    if std::env::var("TASKQ_STDIN_MODE").unwrap() == "blocked" {
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }
    let go = PathBuf::from(std::env::var_os("TASKQ_STDIN_GO").unwrap());
    while !go.exists() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).unwrap();
    assert_eq!(
        text,
        format!("{}EOF-CANARY", "private-input-".repeat(12_000))
    );
}

// Chain: native owned child / exec-adopted child -> stop request -> OS exit readback.
#[test]
fn native_stop_reaps_the_owned_process() {
    let spawn = || {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "runtime::tests::stop_child", "--ignored"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    let check = |proc: &mut dyn Proc| {
        let requested = stop(proc);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if proc.exited().unwrap().is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < until,
                "owned stop did not finish: {requested:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    check(&mut spawn());
    #[cfg(unix)]
    {
        let child = spawn();
        let pid = child.id();
        drop(child);
        check(&mut Adopted::new(
            coreq::process::identity(pid).unwrap().unwrap(),
        ));
        assert_eq!(liveness(pid), Ok(false));
    }
}

// Chain: production stdin launch -> launcher exits -> exact known descendant remains live.
// Risk: root exit releases the execution while an owned tool is still running.
#[cfg(windows)]
#[test]
#[ignore = "owned Windows process-tree qualification; no model or board"]
fn native_cli_owner_exit_does_not_release_live_descendant() {
    use crate::board::Board;
    for cold in [false, true] {
        let root = std::env::temp_dir().join(format!("taskq-native-tree-{}", uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let board = crate::board::Fake::default();
        let task = board
            .create(
                "native scope fixture",
                "",
                &[crate::service::TASK_LABEL.into()],
            )
            .unwrap();
        let attempt = board
            .comment(task, "taskq: spawn worker codex:fixture mac")
            .unwrap();
        let cfg = crate::service::Config {
            root: root.clone(),
            install: root.join("install"),
            ..crate::tests::cfg()
        };
        let log = attempt_log(&root, "worker", task, attempt);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, "").unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "runtime::tests::windows_tree_owner_child",
                "--ignored",
            ])
            .env("TASKQ_TREE_OWNER_ROOT", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = start_command(&mut command, &root, "").unwrap();
        let native = child.native_identity().unwrap();
        let mut turn = CliTurn {
            proc: child,
            log,
            pos: 0,
            start: 0,
            stopping: false,
            accepted: false,
            read_error: None,
            session: None,
            native: Some(native.clone()),
            recovery: None,
            rt: "codex".into(),
        };
        let launch = Launch {
            files: String::new(),
            execution: Some(attempt),
            task,
            title: String::new(),
            attempt,
            role: "worker".into(),
            rt: "codex".into(),
            session: "fixture".into(),
            url: String::new(),
            resume: None,
        };
        turn.checkpoint(&cfg, &launch, attempt).unwrap();
        let saved = crate::service_run::kids_file(&cfg.install, &root).unwrap();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].job.is_some());
        let mut legacy = saved[0].clone();
        legacy.job = None;
        assert!(execution_live(&legacy).unwrap_err().contains("legacy"));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !root.join("descendant.json").exists() {
            assert!(
                std::time::Instant::now() < until,
                "owned fixture did not announce its descendant"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let row: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("descendant.json")).unwrap()).unwrap();
        let descendant = coreq::process::Identity {
            pid: row["pid"].as_u64().unwrap() as u32,
            started: row["started"].as_str().unwrap().into(),
        };
        let mut unrelated = Command::new(std::env::current_exe().unwrap());
        unrelated
            .args(["--exact", "runtime::tests::stop_child", "--ignored"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut sentinel = start_command(&mut unrelated, &root, "").unwrap();
        assert!(
            windows_process::Execution::recover(native.clone(), &sentinel.job().unwrap())
                .err()
                .unwrap()
                .contains("does not belong")
        );
        assert_eq!(turn.exited(), Ok(None));
        std::fs::write(root.join("exit-owner"), "").unwrap();
        while coreq::process::identity(native.pid).unwrap().as_ref() == Some(&native) {
            assert!(
                std::time::Instant::now() < until,
                "fixture launcher did not exit"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            coreq::process::identity(descendant.pid).unwrap(),
            Some(descendant.clone())
        );
        assert!(
            execution_live(&saved[0]).unwrap(),
            "scope-aware readback cannot retire surviving descendants"
        );
        let mut service = crate::service::Service::new(
            &board,
            &cfg,
            "native-fixture",
            Box::new(|_| panic!("native reconstruction must not dispatch")),
        );
        let mut proc: Box<dyn Proc> = if cold {
            // #2266: the last handle closes with the launcher gone; kill-on-close ends the descendant.
            drop(turn);
            while coreq::process::identity(descendant.pid).unwrap().as_ref() == Some(&descendant) {
                assert!(
                    std::time::Instant::now() < until,
                    "kill-on-close did not end the descendant"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            crate::service_run::adopt(&mut service, &cfg.install, &root).unwrap();
            assert_eq!(service.kids.len(), 1);
            assert_eq!(service.kids[0].proc.exited(), Ok(Some(-1)));
            service.reap_owned();
            assert!(service.kids.is_empty());
            assert!(
                crate::service_run::kids_file(&cfg.install, &root)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                crate::core::executions(task, &crate::board::entries(&board, task).unwrap())[0]
                    .release,
                Some(crate::core::Release::Exit)
            );
            assert_eq!(
                sentinel.exited(),
                Ok(None),
                "another execution's job must remain live"
            );
            sentinel.kill().unwrap();
            while sentinel.exited().unwrap().is_none() {
                assert!(
                    std::time::Instant::now() < until,
                    "sentinel cleanup not confirmed"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            drop(service);
            drop(sentinel);
            std::fs::remove_dir_all(root).unwrap();
            println!(
                "cold=true: last job handle closed; kill-on-close ended the descendant; vanished scope adopted as ended; Exit released; record retired"
            );
            continue;
        } else {
            Box::new(turn)
        };
        assert_eq!(
            proc.exited(),
            Ok(None),
            "launcher exit cannot release a live descendant"
        );
        proc.kill().unwrap();
        while proc.exited().unwrap().is_none() {
            assert!(
                std::time::Instant::now() < until,
                "native job cancellation did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_ne!(
            coreq::process::identity(descendant.pid).unwrap(),
            Some(descendant.clone())
        );
        assert!(!execution_live(&saved[0]).unwrap());
        assert_eq!(
            sentinel.exited(),
            Ok(None),
            "another execution's job must remain live"
        );
        sentinel.kill().unwrap();
        while sentinel.exited().unwrap().is_none() {
            assert!(
                std::time::Instant::now() < until,
                "sentinel cleanup not confirmed"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        service.kids.push(crate::service::Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            task,
            attempt,
            execution: attempt,
            rt: "codex".into(),
            proc,
        });
        board.state.borrow_mut().fail_comments = 1;
        service.reap_owned();
        assert_eq!(
            service.kids.len(),
            1,
            "failed terminal receipt retains ownership"
        );
        service.reap_owned();
        assert!(service.kids.is_empty());
        assert!(
            crate::service_run::kids_file(&cfg.install, &root)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            crate::core::executions(task, &crate::board::entries(&board, task).unwrap())[0].release,
            Some(crate::core::Release::Exit)
        );
        drop(service);
        drop(sentinel);
        std::fs::remove_dir_all(root).unwrap();
        println!(
            "cold={cold}: launcher exit held while descendant lived; native job cancelled; unrelated job preserved; terminal receipt retried; protected record retired"
        );
    }
}

#[cfg(windows)]
#[test]
#[ignore = "owned Windows tree launcher fixture"]
fn windows_tree_owner_child() {
    let root = PathBuf::from(std::env::var_os("TASKQ_TREE_OWNER_ROOT").unwrap());
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime::tests::stop_child", "--ignored"])
        .env_remove("TASKQ_TREE_OWNER_ROOT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let identity = coreq::process::identity(child.id()).unwrap().unwrap();
    coreq::write_durable(
        &root.join("descendant.json"),
        &serde_json::json!({"pid":identity.pid,"started":identity.started}).to_string(),
    )
    .unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !root.join("exit-owner").exists() {
        assert!(
            std::time::Instant::now() < until,
            "controller did not release fixture launcher"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(child);
}

#[test]
#[ignore = "owned process stop fixture"]
fn stop_child() {
    std::thread::sleep(std::time::Duration::from_secs(30));
}

// Chain: terminal CLI log / cancelled app handshake -> stop -> observed exit, never a synthetic completion.
#[test]
fn stop_retains_terminal_cli_and_cancelled_handshake_until_exit() {
    use std::{cell::Cell, rc::Rc};
    struct Pending {
        exit: Rc<Cell<bool>>,
        kills: Rc<Cell<usize>>,
    }
    impl Proc for Pending {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(self.exit.get().then_some(-9))
        }
        fn kill(&mut self) -> Result<(), String> {
            self.kills.set(self.kills.get() + 1);
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    use tracing_subscriber::prelude::*;
    let dir = std::env::temp_dir().join(format!("taskq-stop-{}", uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = std::sync::Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
    let capture =
        coreq::telemetry::capture::Capture::new(store.clone(), "taskq", "fixture").unwrap();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
        let log = dir.join("turn.log");
        std::fs::write(&log, "{\"type\":\"turn.completed\"}\n").unwrap();
        let exit = Rc::new(Cell::new(false));
        let kills = Rc::new(Cell::new(0));
        let make = || {
            Box::new(Pending {
                exit: exit.clone(),
                kills: kills.clone(),
            }) as Box<dyn Proc>
        };
        let mut cli = CliTurn {
            proc: make(),
            log: log.clone(),
            pos: 0,
            start: 0,
            stopping: false,
            accepted: false,
            read_error: None,
            session: None,
            native: None,
            recovery: None,
            rt: "codex".into(),
        };
        assert_eq!(cli.exited(), Ok(None));
        assert_eq!(
            cli.exited(),
            Ok(None),
            "acknowledged stop awaiting exit is pending, not a failed observation"
        );
        assert_eq!(
            cli.accepted(),
            Ok(false),
            "terminal alone is not exact-input acceptance"
        );
        assert_eq!(kills.get(), 2, "terminal state must survive a pending stop");
        exit.set(true);
        assert_eq!(cli.exited(), Ok(Some(-9)));
        exit.set(false);
        let mut app = AppTurn {
            root: std::env::temp_dir(),
            history: AppHistory::default(),
            hand: Some(make()),
            log,
            text: String::new(),
            _input: None,
            binding: None,
            thread: String::new(),
            done: 0,
            rollout: None,
            running: || Ok(true),
            control: |_, _| Err("fixture native cancellation unavailable".into()),
            cancelled: false,
            queue_error: None,
            accepted: false,
        };
        assert!(stop(&mut app).is_err());
        assert!(app.hand.is_some());
        exit.set(true);
        assert_eq!(app.exited(), Ok(Some(-9)));
        assert!(
            app.thread.is_empty(),
            "cancelled handshake must not open or queue an app task"
        );
    });
    capture.flush().unwrap();
    let rows = store.export(0, 0, 100).unwrap();
    assert!(
        rows["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |r| r["observation"]["operation"] == "taskq.operation.runtime.stop"
                    && r["observation"]["error"] == "stop-unconfirmed"
            )
    );
    drop(capture);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

// Chain: § 4 step 1 `gone` reason, #1034. Risk: a worker's last message publishes local paths or many lines.
#[test]
fn last_message_is_one_public_line_without_local_paths() {
    for (root, text, want) in [
        (
            r"C:\w\taskq",
            "See [service.rs](/C:/w/taskq/.worktrees/taskq-1/src/service.rs#L5).",
            "See [service.rs](.worktrees/taskq-1/src/service.rs#L5).",
        ),
        (
            r"C:\w\taskq",
            r"Log: c:\W\taskq\.taskq\T1-2.log and C:\Users\me\AppData\x.txt",
            "Log: .taskq/T1-2.log and <path>",
        ),
        (
            "/home/me/taskq",
            "`/home/me/taskq/src/a.rs`, file:///tmp/x/y, ~/.codex/auth.json\n\n  next line",
            "`src/a.rs`, <path>, <path> next line",
        ),
        (
            "/home/me/taskq",
            "Kept: https://github.com/o/r/pull/1 and/or /help, root /home/me/taskq.",
            "Kept: https://github.com/o/r/pull/1 and/or /help, root ..",
        ),
    ] {
        assert_eq!(public_line(text, Path::new(root), 240), want);
    }
    let long = public_line(&"word ".repeat(100), Path::new("/r"), 224);
    assert_eq!((long.chars().count(), long.ends_with(" wor…")), (224, true));
}

// #2153 A1/A2: launcher/sandbox stderr before JSON events surfaces via public_line() in last_message(), and
// setup/readiness login probes verify the active runtime account home.
#[test]
fn launcher_stderr_in_last_message_and_active_account_probe() {
    let dir = std::env::temp_dir().join(format!("taskq-2153-runtime-{}", uuid()));
    let root = dir.join("project");
    let log = attempt_log(&root, "worker", 2153, 1);
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "{\"type\":\"taskq.rules\",\"phase\":\"start\"}\n{\"type\":\"taskq.account\",\"account\":\"work\"}\ntaskq: linux sandbox prerequisite /home/me/.codex/taskq-accounts/work/config.toml: No such file or directory (os error 2)\n").unwrap();
    assert_eq!(
        last_message(&log, "codex").as_deref(),
        Some("taskq: linux sandbox prerequisite <path>: No such file or directory (os error 2)")
    );
    std::fs::write(&log, "{\"type\":\"taskq.rules\",\"phase\":\"start\"}\n{\"type\":\"result\",\"result\":\"completed work\"}\ntrailing warning\n").unwrap();
    assert_eq!(
        last_message(&log, "claude").as_deref(),
        Some("completed work")
    );
    std::fs::write(&log, "{\"type\":\"taskq.rules\",\"phase\":\"start\"}\n{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n").unwrap();
    assert_eq!(last_message(&log, "codex"), None);

    let install = dir.join("install");
    let base = dir.join("codex-home");
    let work = base.join(crate::runtime::accounts::HOMES).join("work");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        install.join(crate::runtime::accounts::FILE),
        r#"{"codex":[{"label":"default","enabled":false},{"label":"work","enabled":true}]}"#,
    )
    .unwrap();
    std::fs::write(
        base.join("auth.json"),
        r#"{"auth_mode":"chatgpt","last_refresh":"2026-10-01T00:00:00Z"}"#,
    )
    .unwrap();
    let bin = if cfg!(windows) {
        let p = dir.join("codex.cmd");
        std::fs::write(&p, "@echo off\r\nif \"%1\"==\"--version\" (echo codex-cli 0.159.3\r\nexit /b 0)\r\nif exist \"%CODEX_HOME%\\auth.json\" (echo Logged in using ChatGPT) else (echo Not logged in)\r\n").unwrap();
        p
    } else {
        let p = dir.join("codex");
        std::fs::write(&p, "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"codex-cli 0.159.3\"; exit 0; fi\nif [ -f \"$CODEX_HOME/auth.json\" ]; then echo \"Logged in using ChatGPT\"; else echo \"Not logged in\"; fi\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        p
    };
    let row = agent_row_in(&install, "codex", &bin, Some(&base));
    assert_eq!(
        row["login"], "not logged in",
        "active work account has no auth.json yet: {row}"
    );
    std::fs::write(
        work.join("auth.json"),
        r#"{"auth_mode":"chatgpt","last_refresh":"2026-11-01T00:00:00Z"}"#,
    )
    .unwrap();
    let row = agent_row_in(&install, "codex", &bin, Some(&base));
    assert_eq!(
        (row["login"].as_str(), row["refreshed"].as_str()),
        (Some("logged in"), Some("2026-11-01T00:00:00Z"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Chain: §3 Runtime accounts / R12 (#2166 A2/A5/A6). Risk: readiness forces a native-default
// override, borrows default credentials for named homes, or disagrees with launch/resume.
#[test]
fn claude_account_environment_parity() {
    let root = std::env::temp_dir().join(format!("taskq-2166-{}", uuid()));
    for mode in [
        "implicit",
        "explicit-default",
        "custom",
        "nested",
        "named",
        "unreadable",
    ] {
        let dir = root.join(mode);
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "runtime::tests::claude_account_environment_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("TASKQ_ACCOUNT_FIXTURE", &dir)
        .env("TASKQ_ACCOUNT_MODE", mode)
        .env("TASKQ_INSTALL", dir.join("install"))
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME");
        c.env(
            "PATH",
            std::env::join_paths(std::iter::once(dir.clone()).chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )))
            .unwrap(),
        );
        if mode == "explicit-default" {
            c.env("CLAUDE_CONFIG_DIR", dir.join(".claude"));
        }
        if mode == "custom" || mode == "unreadable" {
            c.env("CLAUDE_CONFIG_DIR", dir.join("custom"));
        }
        if mode == "nested" {
            c.env("CLAUDE_CONFIG_DIR", dir.join(".claude/taskq-accounts/work"));
        }
        let started = std::time::Instant::now();
        let out = coreq::process::output(
            "qualification.account-env",
            &mut c,
            std::time::Duration::from_secs(if cfg!(windows) { 60 } else { 30 }),
        );
        eprintln!("account environment {mode}: {:?}", started.elapsed());
        assert!(out.is_ok(), "{mode}: {out:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "isolated account-environment subprocess"]
fn claude_account_environment_fixture() {
    let root = PathBuf::from(std::env::var_os("TASKQ_ACCOUNT_FIXTURE").unwrap());
    let mode = std::env::var("TASKQ_ACCOUNT_MODE").unwrap();
    let install = crate::platform::default_install();
    std::fs::create_dir_all(&install).unwrap();
    let base = crate::runtime::accounts::base("claude").unwrap();
    let work = base.join(crate::runtime::accounts::HOMES).join("work");
    std::fs::create_dir_all(&work).unwrap();
    let policy = install.join(crate::runtime::accounts::FILE);
    if mode == "named" {
        std::fs::write(
            &policy,
            r#"{"claude":[{"label":"default","enabled":false},{"label":"work","enabled":true}]}"#,
        )
        .unwrap();
    }
    let bin = root.join(if cfg!(windows) {
        "claude.cmd"
    } else {
        "claude"
    });
    // Invented status only: the fallback is logged in, every explicit home starts empty.
    let script = if cfg!(windows) {
        "@echo off\r\nif \"%1\"==\"agents\" (echo []\r\nexit /b 0)\r\nif \"%1\"==\"--version\" (echo 2.1.316\r\nexit /b 0)\r\nif not defined CLAUDE_CONFIG_DIR (echo {\"loggedIn\":true}\r\nexit /b 0)\r\nif exist \"%CLAUDE_CONFIG_DIR%\\fixture-status\" (type \"%CLAUDE_CONFIG_DIR%\\fixture-status\") else (echo {\"loggedIn\":false})\r\n"
    } else {
        "#!/bin/sh\nif [ \"$1\" = agents ]; then echo '[]'; exit; fi\nif [ \"$1\" = --version ]; then echo 2.1.316; exit; fi\nif [ \"${CLAUDE_CONFIG_DIR+x}\" != x ]; then echo '{\"loggedIn\":true}'; elif [ -f \"$CLAUDE_CONFIG_DIR/fixture-status\" ]; then cat \"$CLAUDE_CONFIG_DIR/fixture-status\"; else echo '{\"loggedIn\":false}'; fi\n"
    };
    std::fs::write(&bin, script).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let selected = if mode == "named" {
        work.clone()
    } else {
        claude_home().unwrap()
    };
    if mode == "unreadable" {
        std::fs::write(selected.join("fixture-status"), "unreadable").unwrap();
    }
    let expected = if mode == "implicit" {
        "logged in"
    } else if mode == "unreadable" {
        "unknown"
    } else {
        "not logged in"
    };
    let env = Env {
        context: "",
        root: &root,
        exe: &bin,
        install: &install,
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let mut l = Launch {
        files: String::new(),
        execution: Some(44),
        task: 2166,
        title: String::new(),
        attempt: 42,
        role: "worker".into(),
        rt: "claude".into(),
        session: "retained-session".into(),
        url: String::new(),
        resume: None,
    };
    let log = log_path(&root, &l);
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    crate::runtime::accounts::mark(&log, if mode == "named" { "work" } else { "default" }).unwrap();
    let pending = root.join("pending-input.txt");
    std::fs::write(&pending, "retained input").unwrap();
    bind_input(
        &log,
        &l,
        &pending,
        "retained input",
        serde_json::json!({"session":l.session}),
    )
    .unwrap();
    let retained = std::fs::read(&log).unwrap();
    let check = |expected: &str| {
        assert_eq!(
            login_state("claude", &bin),
            expected,
            "service readiness: {mode}"
        );
        assert_eq!(
            agent_row_in(&install, "claude", &bin, claude_home().as_deref())["login"],
            expected,
            "agents: {mode}"
        );
    };
    check(expected);
    for resume in [false, true] {
        l.resume = resume.then(|| "retained input".into());
        let turn = spec(&env, &l, root.clone(), vec![]);
        let mut c = agent("claude")
            .unwrap()
            .command(crate::platform::command(&bin), &turn)
            .unwrap();
        let out = coreq::process::output(
            "qualification.launch-env",
            &mut c,
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(
            login(
                "claude",
                Some((out.status.success(), String::from_utf8(out.stdout).unwrap()))
            )
            .0,
            expected,
            "launch/resume: {mode}"
        );
    }
    if mode == "named" || mode == "custom" || mode == "nested" {
        std::fs::write(selected.join("fixture-status"), r#"{"loggedIn":true}"#).unwrap();
        check("logged in");
    }
    if mode == "named" {
        let session = uuid();
        let default_dir = base.join("projects/default");
        let work_dir = work.join("projects/work");
        std::fs::create_dir_all(&default_dir).unwrap();
        std::fs::create_dir_all(&work_dir).unwrap();
        let default_path = default_dir.join(format!("{session}.jsonl"));
        let work_path = work_dir.join(format!("{session}.jsonl"));
        std::fs::write(
            &default_path,
            r#"{"sessionKind":"bg","permissionMode":"bypassPermissions"}"#,
        )
        .unwrap();
        assert!(
            !claude_background(&root, &session, Some(&work)).unwrap(),
            "missing named history cannot borrow default background state"
        );
        std::fs::write(&work_path, r#"{"permissionMode":"dontAsk"}"#).unwrap();
        assert_eq!(
            claude_transcript(&session, Some(&work)),
            Some(work_path.clone()),
            "saved permissions stay in their account"
        );
        assert!(
            !claude_background(&root, &session, Some(&work)).unwrap(),
            "duplicate session stays in its account"
        );
        std::fs::write(
            &work_path,
            r#"{"sessionKind":"bg","permissionMode":"dontAsk"}"#,
        )
        .unwrap();
        assert!(claude_background(&root, &session, Some(&work)).unwrap());
    }
    if mode == "named" {
        std::fs::write(
            &policy,
            r#"{"claude":[{"label":"default","enabled":true},{"label":"work","enabled":false}]}"#,
        )
        .unwrap();
        assert_eq!(
            spec(&env, &l, root.clone(), vec![]).home,
            Some(work),
            "policy changes never move a resumed attempt"
        );
    }
    assert_eq!(
        std::fs::read(log).unwrap(),
        retained,
        "probes preserve attempt and pending-input log bytes"
    );
    assert_eq!(std::fs::read_to_string(pending).unwrap(), "retained input");
    assert_eq!(l.session, "retained-session");
}

// Chain: § 4 service pass, `agents` rows (R12), #1114. Risk: a runtime CLI whose child keeps the pipes open blocks the pass.
#[test]
fn probe_gives_up_on_time_when_a_grandchild_keeps_the_pipes() {
    let dir = std::env::temp_dir().join(format!("taskq-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = |name: &str, body: &str| {
        let (p, text) = if cfg!(windows) {
            (
                dir.join(format!("{name}.cmd")),
                format!("@echo off\r\n{body}\r\n"),
            )
        } else {
            (dir.join(name), format!("#!/bin/sh\n{body}\n"))
        };
        std::fs::write(&p, text).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        p
    };
    let (hang, fork) = if cfg!(windows) {
        ("ping -n 30 127.0.0.1", "start /b ping -n 30 127.0.0.1")
    } else {
        ("sleep 30", "sleep 30 &")
    };
    // the child hangs, then a killed child leaves its grandchild holding the pipes; or the child exits and leaves it
    for (name, body, want) in [
        ("ok", "echo 1.2.3".to_string(), Some("1.2.3")),
        ("hang", format!("echo 1.2.3\n{hang}"), None),
        ("fork", format!("{fork}\necho 1.2.3"), None),
    ] {
        let t = std::time::Instant::now();
        let got = probe_within(
            "runtime.version",
            &script(
                name,
                &body.replace('\n', if cfg!(windows) { "\r\n" } else { "\n" }),
            ),
            &["--version"],
            std::time::Duration::from_secs(2),
        );
        assert_eq!(
            got.as_ref().map(|(ok, out)| (*ok, out.trim())),
            want.map(|v| (true, v)),
            "{name}"
        );
        assert!(
            t.elapsed() < std::time::Duration::from_secs(5),
            "{name}: {:?}",
            t.elapsed()
        );
    }
    let began = std::time::Instant::now();
    let outcome = observe_until(began + std::time::Duration::from_millis(100), || {
        probe_within(
            "runtime.version",
            &script("budget", hang),
            &[],
            std::time::Duration::from_secs(10),
        )
    });
    assert!(outcome.is_none());
    assert!(
        began.elapsed() < std::time::Duration::from_secs(2),
        "adapter ignored remaining pass time"
    );
    assert!(
        observation_deadline(std::time::Duration::from_secs(1))
            > std::time::Instant::now() + std::time::Duration::from_millis(500),
        "observation deadline leaked into later work"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Chain: incident runtime start. Risk: environment errors reported as taskq faults.
#[test]
fn incident_start_preserves_diagnostic_and_excludes_environment_errors() {
    use std::io::{Error, ErrorKind};
    for kind in [
        ErrorKind::NotFound,
        ErrorKind::PermissionDenied,
        ErrorKind::OutOfMemory,
        ErrorKind::WouldBlock,
        ErrorKind::TimedOut,
        ErrorKind::StorageFull,
        ErrorKind::ResourceBusy,
        ErrorKind::InvalidInput,
        ErrorKind::Unsupported,
    ] {
        let error = StartError::io(Error::new(kind, "unchanged diagnostic"), Some("claude"));
        assert!(!error.reportable, "{kind:?}");
        assert_eq!(error.to_string(), "claude: unchanged diagnostic");
        let expected = match kind {
            ErrorKind::PermissionDenied => "permission",
            ErrorKind::TimedOut => "timeout",
            ErrorKind::InvalidInput => "invalid",
            _ => "other",
        };
        crate::profile::tests::take_report();
        let result =
            crate::profile::measure_classified("spawn", || Err::<(), _>(error), StartError::class)
                .unwrap_err();
        assert_eq!(result.to_string(), "claude: unchanged diagnostic");
        assert!(crate::profile::tests::take_report().contains(&format!("{expected}:1")));
    }
    assert!(StartError::io(Error::other("internal launch fault"), None).reportable);
}

// Chain: § 5 taskq agents probe. Risk: unreadable output shown as a known state.
#[test]
fn agent_probes_read_version_and_login_or_say_unknown() {
    assert_eq!(
        version("2.1.296 (Claude Code)\n").as_deref(),
        Some("2.1.296")
    );
    assert_eq!(version("codex-cli 0.159.3").as_deref(), Some("0.159.3"));
    assert_eq!(version("error"), None);
    let ok = |s: &str| Some((true, s.to_string()));
    assert_eq!(
        login("claude", ok(r#"{"loggedIn": true, "email": "a@b.c"}"#)),
        ("logged in", Some("a@b.c".into()))
    );
    assert_eq!(
        login("claude", ok(r#"{"loggedIn": false}"#)),
        ("not logged in", None)
    );
    assert_eq!(
        login("claude", ok("Unknown command: auth")),
        ("unknown", None)
    );
    assert_eq!(
        login("codex", ok("Logged in using ChatGPT\n")),
        ("logged in", Some("ChatGPT".into()))
    );
    assert_eq!(
        login("codex", ok("Logged in using an API key - sk-ab***cd")),
        ("logged in", Some("an API key".into()))
    );
    assert_eq!(
        login("codex", Some((false, "Not logged in\n".into()))),
        ("not logged in", None)
    );
    assert_eq!(login("codex", None), ("unknown", None));
    let row = |l: &str, a: serde_json::Value| serde_json::json!({"runtime": "codex", "version": "0.1", "login": l, "account": a});
    assert_eq!(
        agent_text(&row("logged in", "ChatGPT".into())),
        "codex 0.1 · ChatGPT"
    );
    assert_eq!(
        agent_text(&row("not installed", serde_json::Value::Null)),
        "codex · not installed"
    );
    assert_eq!(
        agent_text(&row("unknown", serde_json::Value::Null)),
        "codex 0.1 · login unknown"
    );
    // #1429: kind and last refresh, never a token value; unreadable reads unknown
    assert_eq!(
        kind_refresh(
            "claude",
            Some(r#"{"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}"#),
            None
        ),
        (Some("claude.ai max".into()), None)
    );
    assert_eq!(
        kind_refresh(
            "codex",
            None,
            Some(
                r#"{"auth_mode": "chatgpt", "tokens": {"refresh_token": "invented"}, "last_refresh": "2026-10-19T18:14:41Z"}"#
            )
        ),
        (Some("chatgpt".into()), Some("2026-10-19T18:14:41Z".into()))
    );
    assert_eq!(kind_refresh("codex", None, Some("{")), (None, None));
    let rows = serde_json::json!([{"runtime": "claude", "version": "2.1", "login": "logged in", "account": "a@b.c", "kind": "claude.ai max", "refreshed": null},
            {"runtime": "codex", "version": "0.1", "login": "not logged in", "account": null, "kind": "chatgpt", "refreshed": "2026-10-19T18:14:41Z"}]);
    assert_eq!(
        login_lines(&rows),
        [
            "Login: claude 2.1 · a@b.c · kind claude.ai max · refreshed unknown",
            "Login: codex 0.1 · not logged in · kind chatgpt · refreshed 2026-10-19T18:14:41Z",
            "Next: codex admits no task; run `codex login --device-auth` with the PM (docs/relogin.md)"
        ]
    );
    assert_eq!(
        login_lines(&serde_json::Value::Null),
        ["Login: unknown (no runtime probe yet)"]
    );
    // `taskq agents` prints kind and refresh too; a missing value reads unknown
    assert_eq!(
        agents_line(
            &serde_json::json!({"runtime": "codex", "path": "/bin/codex", "version": "0.1", "login": "logged in", "account": "a@b.c", "kind": "chatgpt", "refreshed": "2026-10-19T18:14:41Z"})
        ),
        "codex  /bin/codex  0.1  logged in as a@b.c  kind chatgpt  refreshed 2026-10-19T18:14:41Z"
    );
    assert_eq!(
        agents_line(
            &serde_json::json!({"runtime": "claude", "path": null, "login": "not installed"})
        ),
        "claude  not installed  -  not installed  kind unknown  refreshed unknown"
    );
    // #1446 A2: a rejected login shows its reason
    let off = serde_json::json!({"runtime": "codex", "path": "/bin/codex", "version": "0.1", "login": "not logged in", "account": "a@b.c", "reason": "refresh token rejected"});
    assert_eq!(
        agent_text(&off),
        "codex 0.1 · not logged in (refresh token rejected)"
    );
    assert!(
        agents_line(&off).contains("  not logged in (refresh token rejected)  "),
        "{}",
        agents_line(&off)
    );
    // #1661 A2: Settings states stay distinct; a method is never shown as the account
    let view = |r: serde_json::Value| {
        let v = agent_view(&r);
        (
            v["state"].as_str().unwrap().to_string(),
            v["account"].clone(),
            v["method"].clone(),
        )
    };
    assert_eq!(
        view(off.clone()),
        ("rejected".into(), "a@b.c".into(), serde_json::Value::Null)
    );
    assert_eq!(
        view(row("logged in", "ChatGPT".into())),
        (
            "signed-in".into(),
            serde_json::Value::Null,
            "ChatGPT".into()
        )
    );
    assert_eq!(
        view(
            serde_json::json!({"login": "logged in", "account": "a@b.c", "kind": "claude.ai max", "expires": 1})
        ),
        ("expired".into(), "a@b.c".into(), "claude.ai max".into())
    );
    assert_eq!(
        view(serde_json::json!({"login": "logged in", "expires": now() + 3600})).0,
        "signed-in"
    );
    assert_eq!(
        view(serde_json::json!({"login": "not logged in"})).0,
        "sign-in-required"
    );
    assert_eq!(
        view(serde_json::json!({"login": "unknown", "path": "/bin/codex"})).0,
        "check-failed"
    );
    assert_eq!(
        view(serde_json::json!({"login": "not installed", "path": null})).0,
        "not-installed"
    );
    assert_eq!(view(serde_json::json!({})).0, "unknown");
    // an old last refresh is not an expiry; only a needed sign-in names the native route
    let fresh = agent_view(
        &serde_json::json!({"runtime": "codex", "login": "logged in", "refreshed": "2020-01-01T00:00:00Z"}),
    );
    assert_eq!(
        (fresh["state"].as_str(), fresh["next"].as_str()),
        (Some("signed-in"), None)
    );
    assert!(
        agent_view(&off)["next"]
            .as_str()
            .is_some_and(|n| n.contains("codex login"))
    );
}

// Chain: #1446 A1/A3. Risk: Codex's dead refresh token reads logged in and burns retries, worker output that
// quotes the error marks the login off, or the mark survives a re-login.
#[test]
fn rejected_codex_login_is_read_from_recorded_lines_until_relogin() {
    let dir = std::env::temp_dir().join(format!("taskq-auth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("T1-1.log");
    let check = |text: &str| {
        std::fs::write(&log, text).unwrap();
        auth_rejected(&log, "codex")
    };
    // recorded on hermes-fra1, T1420-6308286799.log (#1424)
    assert_eq!(check("2026-10-21T08:00:00.000000Z ERROR codex_login::auth::manager: Failed to refresh token: Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.\n").as_deref(), Some("refresh token rejected"));
    assert_eq!(check("2026-10-21T08:00:00.000000Z ERROR codex_login::auth::manager: Failed to refresh token status=401 Unauthorized detail=TokenErrorDetail { error_code: Some(\"refresh_token_reused\") }\n").as_deref(), Some("refresh token rejected"));
    assert_eq!(check("2026-10-21T08:00:01.000000Z ERROR codex_models_manager::manager: failed to refresh available models: unexpected status 401 Unauthorized: Provided authentication token is expired. Please try signing in again.\n").as_deref(), Some("401 Unauthorized"));
    assert_eq!(check("{\"type\":\"turn.failed\",\"error\":{\"message\":\"unexpected status 401 Unauthorized: Provided authentication token is expired. Please try signing in again., auth error: 401, auth error code: token_expired\"}}\n").as_deref(), Some("401 Unauthorized"));
    // worker output quoting the same text is not Codex's own error
    assert_eq!(
        check(
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"ERROR codex_login::auth::manager: Failed to refresh token: refresh token was already used\"}}\n"
        ),
        None
    );
    assert_eq!(
        check(
            "{\"type\":\"error\",\"message\":\"stream disconnected\"}\nworker said 401 Unauthorized\n"
        ),
        None
    );
    assert_eq!(auth_rejected(&dir.join("missing.log"), "codex"), None);
    // the mark holds until auth.json's last_refresh changes (a re-login)
    mark_login_off(&dir, "codex", "refresh token rejected").unwrap();
    let at = codex_refreshed(&dir);
    assert_eq!(
        login_off(&dir, "codex", at.as_deref()).as_deref(),
        Some("refresh token rejected")
    );
    assert_eq!(login_off(&dir, "codex", Some("2099-01-01T00:00:00Z")), None);
    assert_eq!(login_off(&dir, "claude", None), None);
    let _ = std::fs::remove_dir_all(&dir);
}

// Chain: CLI input boundary -> native stream -> acceptance. Risk: an old turn or a process exit consumes new input.
#[test]
fn cli_acceptance_uses_only_native_events_after_its_input_boundary() {
    struct Exited;
    impl Proc for Exited {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(Some(0))
        }
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let dir = std::env::temp_dir().join(format!("taskq-cli-accept-{}", uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("turn.log");
    let old = "{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n";
    for (rt, native) in [
        ("codex", ""),
        ("codex", "{\"type\":\"turn.started\"}\n"),
        (
            "claude",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"work\"}]}}\n",
        ),
    ] {
        std::fs::write(
            &path,
            format!("{old}{native}{{\"type\":\"turn.completed\"}}\n"),
        )
        .unwrap();
        let mut turn = CliTurn {
            proc: Box::new(Exited),
            log: path.clone(),
            pos: old.len() as u64,
            start: old.len() as u64,
            stopping: false,
            accepted: false,
            read_error: None,
            session: None,
            native: None,
            recovery: None,
            rt: rt.into(),
        };
        assert_eq!(turn.accepted(), Ok(false));
        assert_eq!(turn.exited(), Ok(Some(0)));
        assert_eq!(turn.accepted(), Ok(!native.is_empty()));
        assert_eq!(turn.exited(), Ok(Some(0)));
        assert_eq!(
            turn.accepted(),
            Ok(!native.is_empty()),
            "proof survives repeated polls"
        );
    }
    let mut truncated = CliTurn {
        proc: Box::new(Exited),
        log: path.clone(),
        pos: 1000,
        start: 1000,
        stopping: false,
        accepted: false,
        read_error: None,
        session: None,
        native: None,
        recovery: None,
        rt: "codex".into(),
    };
    assert_eq!(
        truncated.exited(),
        Ok(Some(0)),
        "native exit remains independently observable"
    );
    assert!(truncated.accepted().unwrap_err().contains("truncated"));
    assert!(
        truncated.input_offset().is_err(),
        "a lost native input boundary cannot be handed over"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

// Chain: § 8 codex-app turn end. Risk: a turn ends early or never ends.
#[test]
fn app_turn_keeps_unreadable_completion_unknown() {
    let p = std::env::temp_dir().join(format!(
        "taskq-rollout-unknown-{}.jsonl",
        std::process::id()
    ));
    let mut t = AppTurn {
        root: p.parent().unwrap().canonicalize().unwrap(),
        history: AppHistory::default(),
        hand: None,
        log: p.clone(),
        text: String::new(),
        _input: None,
        binding: None,
        thread: "th".into(),
        done: 1,
        rollout: Some(p.clone()),
        running: || Ok(true),
        control: |_, _| Err("fixture native cancellation unavailable".into()),
        cancelled: false,
        queue_error: None,
        accepted: false,
    };
    let _ = std::fs::remove_file(&p);
    assert!(
        t.exited().is_err(),
        "missing history must retain an explicit unknown reason"
    );
    std::fs::write(&p, "{\"type\":\"event_msg\"").unwrap();
    assert!(
        t.exited().is_err(),
        "incomplete history cannot prove the turn is still healthy"
    );
    std::fs::write(&p, "").unwrap();
    assert!(
        t.exited().is_err(),
        "history shorter than the baseline cannot reset the turn count"
    );
    t.running = || Err("probe timed out".into());
    assert_eq!(
        t.exited(),
        Err("probe timed out".into()),
        "failed app query cannot become an exit"
    );
    for windows in [false, true] {
        assert!(app_presence(b"unreadable", windows).is_err());
        assert_eq!(app_presence(b"", windows), Ok(false));
    }
    assert_eq!(app_presence(b"ASN:0x0-0x12-\"Codex\":\n", false), Ok(true));
    assert_eq!(
        app_presence(b"\"Codex.exe\",\"42\",\"OpenAI.Codex_1\"\n", true),
        Ok(true)
    );
    assert_eq!(
        app_presence(b"\"other.exe\",\"43\",\"Other\"\n", true),
        Ok(false)
    );
    assert!(app_presence(b"\"other.exe\",\"unknown\",\"Other\"\n", true).is_err());
    assert!(app_presence(b"ASN:0xbad:", false).is_err());
    assert_eq!(
        app_presence(
            b"\"other-\x82.exe\",\"43\",\"Other\"\n\"Codex.exe\",\"42\",\"OpenAI.Codex_1\"\n",
            true
        ),
        Ok(true)
    );
    assert_eq!(
        app_presence(b"\"other-\x82.exe\",\"43\",\"Other\"\n", true),
        Ok(false)
    );
    assert!(app_presence(b"\"other-\x82.exe\",\"unknown\",\"Other\"\n", true).is_err());
    assert!(app_presence(b"ASN:0x0-0x12-\"\xff\":\n", false).is_err());
    std::fs::remove_file(p).unwrap();
}

// Chain: § 8 codex-app turn end. Risk: a turn ends early or never ends.
// D2/#2159: a previous aborted identical input cannot acknowledge or finish a new owner submission.
#[test]
fn app_owner_input_floor_excludes_an_old_aborted_identical_reply() {
    let root = std::env::temp_dir().join(format!("taskq-app-floor-{}", uuid()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let path = root.join("native.jsonl");
    let events = [
        serde_json::json!({"type":"session_meta","payload":{"id":"thread","cwd":root}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"old"}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"same reply","turn_id":"old"}}),
        serde_json::json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"old"}}),
    ];
    let old = events.iter().map(|v| format!("{v}\n")).collect::<String>();
    std::fs::write(&path, &old).unwrap();
    let mut history = AppHistory {
        floor: old.len() as u64,
        ..AppHistory::default()
    };
    assert_eq!(
        history.read(&path, 0, "same reply", &root, "thread"),
        Ok((false, false))
    );
    let mut next = old;
    for payload in [
        serde_json::json!({"type":"task_started","turn_id":"new"}),
        serde_json::json!({"type":"user_message","message":"same reply","turn_id":"new"}),
        serde_json::json!({"type":"task_complete","turn_id":"new"}),
    ] {
        next += &format!(
            "{}\n",
            serde_json::json!({"type":"event_msg","payload":payload})
        );
    }
    std::fs::write(&path, next).unwrap();
    assert_eq!(
        history.read(&path, 0, "same reply", &root, "thread"),
        Ok((true, true))
    );
    assert_eq!(history.owned.as_deref(), Some("new"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn app_turn_requires_its_input_and_matching_native_completion() {
    let p = std::env::temp_dir().join(format!("taskq-rollout-{}.jsonl", std::process::id()));
    let event = |kind: &str, id: &str, message: &str| {
        serde_json::json!({"type":"event_msg", "payload":{"type":kind,"turn_id":id,"message":message}}).to_string() + "\n"
    };
    let meta = serde_json::json!({"type":"session_meta","payload":{"id":"th","cwd":p.parent().unwrap().canonicalize().unwrap()}});
    let mut history = meta.to_string() + "\n" + &event("task_complete", "old", "");
    std::fs::write(&p, &history).unwrap();
    let mut t = AppTurn {
        root: p.parent().unwrap().canonicalize().unwrap(),
        history: AppHistory::default(),
        hand: None,
        log: p.clone(),
        text: "owned input".into(),
        _input: None,
        binding: None,
        thread: "th".into(),
        done: 1,
        rollout: Some(p.clone()),
        running: || Ok(true),
        control: |_, _| Err("fixture native cancellation unavailable".into()),
        cancelled: false,
        queue_error: None,
        accepted: false,
    };
    assert_eq!(t.exited(), Ok(None));
    assert_eq!(t.accepted(), Ok(false));
    for (id, input, completion, expected) in [
        ("other", "unrelated input", "other", false),
        ("owned", "owned input", "wrong", false),
    ] {
        history += &event("task_started", id, "");
        history += &event("user_message", id, input);
        history += &event("task_complete", completion, "");
        std::fs::write(&p, &history).unwrap();
        assert_eq!(
            t.exited(),
            Ok(expected.then_some(0)),
            "unrelated completion cannot release ownership"
        );
        assert_eq!(
            t.accepted(),
            Ok(id == "owned"),
            "acceptance belongs to the exact input and native turn"
        );
    }
    t.done = 0;
    t.history = AppHistory::default(); // cold observer has no volatile completion baseline
    t.queue_error = Some("submission response lost".into());
    assert_eq!(
        t.exited(),
        Ok(None),
        "native input acceptance resolves the lost submission response"
    );
    assert!(
        stop(&mut t)
            .unwrap_err()
            .contains("native cancellation unavailable"),
        "a live App turn needs an explicit unsupported-control reason"
    );
    assert_eq!(
        t.exited(),
        Ok(None),
        "refused cancellation retains the exact execution observer"
    );
    t.control = |thread, turn| {
        assert_eq!((thread, turn), ("th", "owned"));
        Ok(())
    };
    assert_eq!(t.kill(), Ok(()));
    assert_eq!(
        t.exited(),
        Ok(None),
        "acknowledged interrupt is not native terminal evidence"
    );
    history += &event("task_complete", "owned", "");
    std::fs::write(&p, &history).unwrap();
    assert_eq!(
        t.exited(),
        Ok(Some(0)),
        "later exact proof resolves an uncertain submission without replay"
    );
    assert_eq!(
        stop(&mut t),
        Ok(0),
        "observed native completion resolves stop even when cancellation is unsupported"
    );
    t.running = || Ok(false);
    assert!(t.exited().unwrap_err().contains("termination unconfirmed"));
    t.running = || Ok(true);
    for change in ["root", "thread", "copied", "missing", "duplicate"] {
        let mut wrong = meta.clone();
        match change {
            "root" => wrong["payload"]["cwd"] = "/nonexistent/taskq".into(),
            "thread" => wrong["payload"]["id"] = "foreign".into(),
            "copied" => wrong["payload"]["forked_from_id"] = "original".into(),
            _ => {}
        }
        let (_, events) = history.split_once('\n').unwrap();
        let text = match change {
            "missing" => events.to_owned(),
            "duplicate" => meta.to_string() + "\n" + &history,
            _ => wrong.to_string() + "\n" + events,
        };
        std::fs::write(&p, text).unwrap();
        t.history = AppHistory::default();
        assert!(
            t.exited().is_err(),
            "{change} cannot prove native completion"
        );
    }
    // Installed desktop emits UserMessage items instead of legacy user_message events.
    let native = serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":"th","turn_id":"owned",
            "item":{"type":"UserMessage","id":"native-input","content":[{"type":"text","text":"owned input","text_elements":[]}]}}});
    for change in [
        "valid",
        "transport-lf",
        "extra-lf",
        "space",
        "aborted",
        "foreign-abort",
        "conflicting-terminal",
        "thread",
        "turn",
        "missing-turn",
        "content",
        "duplicate",
        "duplicate-lf",
    ] {
        let mut item = native.clone();
        match change {
            "thread" => item["payload"]["thread_id"] = "foreign".into(),
            "turn" => item["payload"]["turn_id"] = "foreign".into(),
            "missing-turn" => {
                item["payload"].as_object_mut().unwrap().remove("turn_id");
            }
            "content" => item["payload"]["item"]["content"][0]["text"] = "unrelated".into(),
            "transport-lf" | "duplicate-lf" => {
                item["payload"]["item"]["content"][0]["text"] = "owned input\n".into()
            }
            "extra-lf" => item["payload"]["item"]["content"][0]["text"] = "owned input\n\n".into(),
            "space" => item["payload"]["item"]["content"][0]["text"] = "owned input ".into(),
            _ => {}
        }
        let mut text = meta.to_string()
            + "\n"
            + &event("task_started", "owned", "")
            + &item.to_string()
            + "\n";
        if matches!(change, "duplicate" | "duplicate-lf") {
            text += &(native.to_string() + "\n");
        }
        if matches!(change, "aborted" | "foreign-abort" | "conflicting-terminal") {
            text += &event(
                "turn_aborted",
                if change == "foreign-abort" {
                    "foreign"
                } else {
                    "owned"
                },
                "",
            );
        }
        if change != "aborted" && change != "foreign-abort" {
            text += &event("task_complete", "owned", "");
        }
        std::fs::write(&p, text).unwrap();
        t.history = AppHistory::default();
        t.accepted = false;
        let outcome = t.exited();
        match change {
            "valid" | "transport-lf" => {
                assert_eq!(outcome, Ok(Some(0)));
                assert_eq!(t.accepted(), Ok(true));
            }
            "aborted" => {
                assert_eq!(outcome, Ok(Some(-1)));
                assert_eq!(t.accepted(), Ok(true));
            }
            "foreign-abort" => {
                assert_eq!(outcome, Ok(None));
                assert_eq!(t.accepted(), Ok(true));
            }
            "content" | "extra-lf" | "space" => {
                assert_eq!(outcome, Ok(None));
                assert_eq!(t.accepted(), Ok(false));
            }
            _ => assert!(
                outcome.is_err(),
                "{change} cannot establish input acceptance"
            ),
        }
    }
    t.thread = "x & calc".into();
    assert!(t.queue().is_err());
    std::fs::remove_file(p).unwrap();
}

// Chain: native queued input -> lost-response cold observer -> exact native completion.
// Read-only qualification of an already-completed dedicated probe; no dispatch or control.
#[test]
#[ignore = "owned native probe; TASKQ_APP_READBACK selects captured lifecycle evidence"]
fn native_app_lost_reply_readback() {
    let evidence = PathBuf::from(std::env::var("TASKQ_APP_READBACK").expect("TASKQ_APP_READBACK"));
    let read = |file: &str| {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(evidence.join(file)).unwrap(),
        )
        .unwrap()
    };
    let manifest = read("manifest.json");
    for (file, code) in [("queue-command.json", 0), ("interrupt-queue.json", -1)] {
        let queued = read(file);
        let root = PathBuf::from(manifest["root"].as_str().unwrap())
            .canonicalize()
            .unwrap();
        let thread = manifest["thread"].as_str().unwrap().to_owned();
        let text = queued["input"].as_str().unwrap().to_owned();
        let mut turn = AppTurn {
            root,
            history: AppHistory::default(),
            hand: None,
            log: evidence.join("handshake.jsonl"),
            text,
            _input: None,
            binding: None,
            rollout: rollout(&thread),
            thread,
            done: 0,
            running: app_running,
            control: app_cancel,
            cancelled: false,
            queue_error: Some("fixture: submission response lost".into()),
            accepted: false,
        };
        assert_eq!(turn.exited(), Ok(Some(code)));
        assert_eq!(turn.accepted(), Ok(true));
        assert!(turn.queue_error.is_none());
        assert_eq!(
            turn.exited(),
            Ok(Some(code)),
            "repeated observation cannot replay the submitted input"
        );
        println!(
            "native App exact input and terminal {code} recovered without submission: {}",
            turn.thread
        );
    }
}

// Chain: owned native input -> production IPC cancellation -> matching native abort.
#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "dedicated active native probe; TASKQ_APP_READBACK selects its evidence"]
fn native_app_cancels_exact_owned_turn() {
    let evidence = PathBuf::from(std::env::var("TASKQ_APP_READBACK").expect("TASKQ_APP_READBACK"));
    let read = |file: &str| {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(evidence.join(file)).unwrap(),
        )
        .unwrap()
    };
    let manifest = read("manifest.json");
    let queued = read("source-cancel-queue.json");
    let root = PathBuf::from(manifest["root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let thread = manifest["thread"].as_str().unwrap().to_owned();
    let text = queued["input"].as_str().unwrap().to_owned();
    let mut turn = AppTurn {
        root,
        history: AppHistory::default(),
        hand: None,
        log: evidence.join("handshake.jsonl"),
        text,
        _input: None,
        binding: None,
        rollout: rollout(&thread),
        thread,
        done: 0,
        running: app_running,
        control: app_cancel,
        cancelled: false,
        queue_error: Some("fixture: submission response lost".into()),
        accepted: false,
    };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !turn.accepted {
        let observed = turn.exited();
        assert!(
            !matches!(observed, Ok(Some(_))),
            "native probe must still be active before cancellation"
        );
        assert!(
            std::time::Instant::now() < until,
            "input acceptance unavailable: {observed:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let owned = turn.history.owned.clone().unwrap();
    turn.kill().expect("exact owner/turn interrupt request");
    std::fs::write(
        evidence.join("source-cancel-receipt.json"),
        serde_json::json!({"thread":turn.thread,"turn":owned,"request_acknowledged":true})
            .to_string(),
    )
    .unwrap();
    loop {
        let observed = turn.exited();
        if let Ok(Some(code)) = observed {
            assert_eq!(code, -1);
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "interrupt request is not terminal: {observed:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(turn.history.owned, Some(owned.clone()));
    assert_eq!(turn.accepted(), Ok(true));
    println!(
        "production App IPC cancelled exact native turn {owned}; native abort observed; no resubmission"
    );
}

// Chain: long native App history -> bounded cold scan -> exact completion -> appended observations.
// Risk: a total-size ceiling stalls long sessions, or a partial scan/rewritten history releases work early.
#[test]
fn app_history_progresses_in_bounded_passes_without_early_completion() {
    let root = std::env::temp_dir().join(format!("taskq-app-history-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let path = root.join("rollout.jsonl");
    let event = |kind: &str, message: &str| {
        serde_json::json!({"type":"event_msg","payload":{"type":kind,"turn_id":"owned","message":message}}).to_string()+"\n"
    };
    let header = serde_json::json!({"type":"session_meta","payload":{"id":"thread","cwd":root}})
        .to_string()
        + "\n";
    let prefix = header
        + &event("task_started", "")
        + &event("user_message", "exact input")
        + &event("task_complete", "");
    for duplicate in [false, true] {
        let mut file = File::create(&path).unwrap();
        file.write_all(prefix.as_bytes()).unwrap();
        let padding = serde_json::json!({"type":"response_item","payload":"x".repeat(1024*1024)})
            .to_string()
            + "\n";
        for _ in 0..18 {
            file.write_all(padding.as_bytes()).unwrap();
        }
        if duplicate {
            file.write_all(event("task_started", "").as_bytes())
                .unwrap();
            file.write_all(event("user_message", "exact input").as_bytes())
                .unwrap();
        }
        file.sync_all().unwrap();
        drop(file);
        let baseline_began = std::time::Instant::now();
        assert_eq!(completes(&path).unwrap(), 1);
        let baseline_time = baseline_began.elapsed();
        let mut history = AppHistory::default();
        let mut passes = 0;
        loop {
            passes += 1;
            let before = history.reader.offset;
            let result = history.read(&path, 0, "exact input", &root, "thread");
            assert!(history.reader.offset - before <= 8 * 1024 * 1024);
            match result {
                Err(e) if e.contains("scan pending") => {
                    assert!(history.reader.offset > before);
                    assert!(passes < 5);
                }
                Err(e) => {
                    assert!(duplicate && e.contains("multiple native messages"), "{e}");
                    break;
                }
                Ok(outcome) => {
                    assert!(!duplicate);
                    assert_eq!(outcome, (true, true));
                    assert!(passes >= 3);
                    break;
                }
            }
        }
        if duplicate {
            continue;
        }
        let end = history.reader.offset;
        assert_eq!(
            history.read(&path, 0, "exact input", &root, "thread"),
            Ok((true, true))
        );
        assert_eq!(history.reader.offset, end);
        let idle = std::time::Instant::now();
        for _ in 0..100 {
            assert_eq!(
                history.read(&path, 0, "exact input", &root, "thread"),
                Ok((true, true))
            );
        }
        eprintln!(
            "App native history: {} bytes, {passes} bounded passes; baseline {:?}; 100 unchanged reads {:?}",
            end,
            baseline_time,
            idle.elapsed()
        );
        let extra = event("agent_message", "later native diagnostic");
        let split = extra.len() / 2;
        File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&extra.as_bytes()[..split])
            .unwrap();
        assert!(
            history
                .read(&path, 0, "exact input", &root, "thread")
                .unwrap_err()
                .contains("incomplete")
        );
        assert_eq!(
            history.reader.offset, end,
            "a partial record is not consumed"
        );
        File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&extra.as_bytes()[split..])
            .unwrap();
        assert_eq!(
            history.read(&path, 0, "exact input", &root, "thread"),
            Ok((true, true))
        );
        assert_eq!(history.reader.offset, end + extra.len() as u64);
        // A replacement with the same bytes must not inherit the old cursor.
        let replacement = root.join("replacement.jsonl");
        std::fs::copy(&path, &replacement).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert!(
            history
                .read(&path, 0, "exact input", &root, "thread")
                .is_err()
        );
    }
    std::fs::write(&path, &prefix).unwrap();
    let mut history = AppHistory::default();
    assert_eq!(
        history.read(&path, 0, "exact input", &root, "thread"),
        Ok((true, true))
    );
    std::fs::write(&path, "{}\n").unwrap();
    assert!(
        history
            .read(&path, 0, "exact input", &root, "thread")
            .unwrap_err()
            .contains("truncated")
    );
    std::fs::write(&path, &prefix).unwrap();
    let mut history = AppHistory::default();
    history
        .read(&path, 0, "exact input", &root, "thread")
        .unwrap();
    let changed = prefix.replace("task_complete", "task_conflict")
        + &event("agent_message", "growth cannot hide a rewritten boundary");
    std::fs::write(&path, changed).unwrap();
    assert!(
        history
            .read(&path, 0, "exact input", &root, "thread")
            .unwrap_err()
            .contains("boundary changed")
    );
    std::fs::write(&path, "x".repeat(8 * 1024 * 1024 + 1)).unwrap();
    assert!(
        AppHistory::default()
            .read(&path, 0, "exact input", &root, "thread")
            .unwrap_err()
            .contains("exceeds 8 MiB")
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: live Claude worker -> transcript mtime bumped with no new bytes (#1588) -> history read.
// Risk: an mtime-only touch reports Run state unknown, or a same-length rewrite passes as unchanged.
#[test]
fn native_history_mtime_touch_keeps_observation_and_same_length_rewrite_fails() {
    let root = std::env::temp_dir().join(format!("taskq-history-touch-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("transcript.jsonl");
    let text = "{\"type\":\"user\",\"n\":1}\n{\"type\":\"assistant\",\"n\":2}\n";
    std::fs::write(&path, text).unwrap();
    let mut history = NativeHistory::default();
    let read = |h: &mut NativeHistory| h.read(&path, |_| Ok(()));
    assert_eq!(read(&mut history), Ok(true));
    let touch = |secs| {
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(secs))
            .unwrap()
    };
    touch(60);
    assert_eq!(
        read(&mut history),
        Ok(true),
        "an mtime-only change keeps the execution observed"
    );
    let mut file = File::options().write(true).open(&path).unwrap();
    file.write_all(b"{\"type\":\"system\"").unwrap();
    drop(file);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), text.len() as u64);
    touch(120);
    assert!(
        read(&mut history).unwrap_err().contains("rewritten"),
        "changed bytes of the same length fail closed"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: live Claude worker -> EnterWorktree moves its transcript to the worktree's project folder (#1627) -> history read twice.
// Risk: the moved transcript reads as another history, or a different file at the new path passes as the original.
#[test]
fn native_history_follows_a_moved_transcript_and_rejects_a_replacement() {
    let root = std::env::temp_dir().join(format!("taskq-history-move-{}", uuid()));
    for dir in ["root", "worktree"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let (old, new) = (
        root.join("root/transcript.jsonl"),
        root.join("worktree/transcript.jsonl"),
    );
    std::fs::write(&old, "{\"type\":\"user\",\"n\":1}\n").unwrap();
    let mut history = NativeHistory::default();
    let mut seen = Vec::new();
    assert_eq!(
        history.read(&old, |v| {
            seen.push(v["n"].as_u64());
            Ok(())
        }),
        Ok(true)
    );
    std::fs::rename(&old, &new).unwrap();
    File::options()
        .append(true)
        .open(&new)
        .unwrap()
        .write_all(b"{\"type\":\"assistant\",\"n\":2}\n")
        .unwrap();
    assert_eq!(
        history.read(&new, |v| {
            seen.push(v["n"].as_u64());
            Ok(())
        }),
        Ok(true),
        "a moved transcript keeps the execution observed"
    );
    assert_eq!(
        seen,
        [Some(1), Some(2)],
        "the cursor continues without replay"
    );
    // The replacement exists before the original goes, so no file system can reuse its identity.
    std::fs::write(
        &old,
        "{\"type\":\"user\",\"n\":1}\n{\"type\":\"assistant\",\"n\":2}\n",
    )
    .unwrap();
    std::fs::remove_file(&new).unwrap();
    std::fs::rename(&old, &new).unwrap();
    let replaced = history.read(&new, |_| Ok(())).unwrap_err();
    assert!(
        replaced.contains("replaced") || replaced.contains("identity changed"),
        "another file at the moved path fails closed: {replaced}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: board intent -> native input -> cold observer. Risk: restart substitutes input, replays work or drops its lock.
#[test]
fn cold_input_recovery_preserves_the_exact_execution() {
    let root = std::env::temp_dir().join(format!("taskq-cold-input-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    for rt in ["claude", "codex-app"] {
        let l = Launch {
            files: String::new(),
            execution: Some(44),
            task: 7,
            title: String::new(),
            attempt: 42,
            role: "worker".into(),
            rt: rt.into(),
            session: uuid(),
            url: String::new(),
            resume: Some("continue".into()),
        };
        let x = crate::core::Execution {
            id: 44,
            attempt: 42,
            through: 43,
            host: "mac".into(),
            runtime: rt.into(),
            accepted: false,
            release: None,
            usage_hold: None,
        };
        let log = log_path(&root, &l);
        let (path, lock) = turn_input(&root, "original task", Some(&l)).unwrap();
        assert_eq!(
            path,
            root.canonicalize()
                .unwrap()
                .join(".taskq/inputs/execution-7-42-44.txt")
        );
        assert!(turn_input(&root, "replacement task", Some(&l)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original task");
        let input = if rt == "claude" {
            claude_reference(&path)
        } else {
            app_reference(&path)
        }
        .unwrap();
        let binding = bind_input(
            &log,
            &l,
            &path,
            &input,
            serde_json::json!({"thread":"native-thread","session":l.session}),
        )
        .unwrap();
        drop(lock);
        let mut large_log = File::options().append(true).open(&log).unwrap();
        let row = serde_json::json!({"type":"fixture","padding":"x".repeat(1024*1024)}).to_string()
            + "\n";
        for _ in 0..10 {
            large_log.write_all(row.as_bytes()).unwrap();
        }
        drop(large_log);
        let mut observer = recover(&root, &root.join("install"), 7, &x).unwrap();
        assert_eq!(observer.id(), 0);
        assert!(
            !matches!(observer.accepted(), Ok(true)),
            "binding is not native acceptance"
        );
        let file = File::options().read(true).write(true).open(&path).unwrap();
        assert!(matches!(
            file.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(observer);
        file.try_lock().unwrap();
        file.unlock().unwrap();
        for case in [
            "foreign-task",
            "foreign-execution",
            "foreign-runtime",
            "input",
            "path",
            "legacy",
            "missing",
            "partial",
            "oversized",
            "ambiguous",
            "released",
        ] {
            let mut b = binding.clone();
            let mut execution = x.clone();
            let reviewer = attempt_log(&root, "reviewer", 7, 42);
            match case {
                "foreign-task" => b["task"] = 8.into(),
                "foreign-execution" => b["execution"] = 45.into(),
                "foreign-runtime" => b["runtime"] = "other".into(),
                "input" => b["input"] = "unrelated input".into(),
                "path" => b["path"] = "/unrelated/input.txt".into(),
                "legacy" => {
                    std::fs::rename(&path, root.join("legacy.txt")).unwrap();
                }
                "ambiguous" => std::fs::write(&reviewer, "{}").unwrap(),
                "released" => execution.release = Some(crate::core::Release::Exit),
                _ => {}
            }
            std::fs::write(
                &log,
                match case {
                    "missing" => String::new(),
                    "partial" => "{incomplete".into(),
                    "oversized" => "x".repeat(8 * 1024 * 1024 + 1),
                    _ => b.to_string() + "\n",
                },
            )
            .unwrap();
            assert!(
                recover(&root, &root.join("install"), 7, &execution).is_err(),
                "{rt}: {case}"
            );
            if case == "legacy" {
                std::fs::rename(root.join("legacy.txt"), &path).unwrap();
            }
            if case == "ambiguous" {
                std::fs::remove_file(reviewer).unwrap();
            }
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(log).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

// Risk (A5): the service read its trust wait from the error text.
#[test]
fn only_the_trust_error_marks_the_trust_wait() {
    let pending = StartError::trust_pending();
    assert!(pending.trust_pending && pending.no_start && pending.class() == "admission");
    assert!(
        !StartError::admission(pending.message).trust_pending,
        "same text, no trust wait"
    );
}

// A1 / R9: trust repair can retry a definite no-start, but never reuse uncertain input.
#[test]
fn trust_retry_reuses_only_a_definitely_refused_input() {
    let root = std::env::temp_dir().join(format!("taskq-trust-input-{}", uuid()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let l = Launch {
        files: String::new(),
        execution: Some(42),
        task: 7,
        title: String::new(),
        attempt: 42,
        role: "worker".into(),
        rt: "claude".into(),
        session: String::new(),
        url: String::new(),
        resume: None,
    };
    let (input, lock, path) = claude_input(&root, "original input", Some(&l)).unwrap();
    drop(lock);
    assert!(
        claude_input(&root, "changed input", Some(&l)).is_err(),
        "unobserved submission stays fenced"
    );
    let log = log_path(&root, &l);
    let binding = serde_json::json!({"type":"taskq.input","execution":42});
    let refusal = serde_json::json!({"type":"claude.launch","stdout":"","stderr":"Workspace not trusted","status":1});
    std::fs::write(&log, format!("{binding}\n{refusal}\n")).unwrap();
    let (reused, lock, same) = claude_input(&root, "changed input", Some(&l)).unwrap();
    assert_eq!(reused, input);
    assert_eq!(same, path);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original input");
    drop(lock);
    record_input(&log, &binding).unwrap();
    assert!(
        claude_input(&root, "changed input", Some(&l)).is_err(),
        "lost retry response never permits replay"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Risk: a refused launch blocks its task forever, or a doubtful record releases a started job (#1557).
#[test]
fn saved_launch_records_reconcile_only_a_refusal() {
    // #1557 A4: records written before the status field still reconcile; any doubt stays unknown.
    let root = std::env::temp_dir().join(format!("taskq-refused-{}", uuid()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let x = crate::core::Execution {
        id: 44,
        attempt: 42,
        through: 0,
        host: "mac".into(),
        runtime: "claude".into(),
        accepted: false,
        release: None,
        usage_hold: None,
    };
    let bind = |id: u64| serde_json::json!({"type":"taskq.input","execution":id}).to_string();
    let launch = |stdout: &str, stderr: &str, status: Option<serde_json::Value>| {
        let mut v = serde_json::json!({"type":"claude.launch","stdout":stdout,"stderr":stderr});
        if let Some(s) = status {
            v["status"] = s;
        }
        v.to_string()
    };
    let refused = launch("", "Workspace not trusted", None);
    for (case, lines, want) in [
        ("legacy refusal", vec![bind(44), refused.clone()], true),
        (
            "exit 1",
            vec![
                bind(44),
                launch("", "Workspace not trusted", Some(1.into())),
            ],
            true,
        ),
        (
            "signal",
            vec![
                bind(44),
                launch("", "Workspace not trusted", Some(serde_json::Value::Null)),
            ],
            false,
        ),
        (
            "exit 0",
            vec![bind(44), launch("", "warning", Some(0.into()))],
            false,
        ),
        (
            "id printed",
            vec![
                bind(44),
                launch("backgrounded · c37eb75a\n", "failed", Some(1.into())),
            ],
            false,
        ),
        ("empty stderr", vec![bind(44), launch("", "", None)], false),
        ("other execution", vec![bind(43), refused.clone()], false),
        (
            "later binding",
            vec![bind(44), bind(45), refused.clone()],
            false,
        ),
        (
            "retry response unknown",
            vec![bind(44), refused.clone(), bind(44)],
            false,
        ),
        (
            "retry accepted",
            vec![
                bind(44),
                refused.clone(),
                bind(44),
                launch("backgrounded \u{b7} c37eb75a\n", "", Some(0.into())),
            ],
            false,
        ),
        (
            "retry refused",
            vec![bind(44), refused.clone(), bind(44), refused.clone()],
            true,
        ),
        ("no record", vec![bind(44)], false),
    ] {
        std::fs::write(attempt_log(&root, "worker", 7, 42), lines.join("\n")).unwrap();
        assert_eq!(refused_launch(&root, 7, &x).is_some(), want, "{case}");
    }
    std::fs::write(
        attempt_log(&root, "worker", 7, 42),
        [bind(44), refused].join("\n"),
    )
    .unwrap();
    assert!(
        refused_launch(
            &root,
            7,
            &crate::core::Execution {
                accepted: true,
                ..x.clone()
            }
        )
        .is_none(),
        "accepted input never reads no-start"
    );
    assert!(
        refused_launch(
            &root,
            7,
            &crate::core::Execution {
                runtime: "codex-app".into(),
                ..x.clone()
            }
        )
        .is_none()
    );
    std::fs::write(attempt_log(&root, "reviewer", 7, 42), "").unwrap();
    assert!(
        refused_launch(&root, 7, &x).is_none(),
        "ambiguous log stays unknown"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: exact execution input binding -> uncertain native launch -> retained observation and input lock.
// Risk: a timeout or unreadable launch response drops the only recovery token and invites resubmission.
#[test]
fn uncertain_background_launch_retains_its_bound_input() {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    let root = std::env::temp_dir().join(format!("taskq-uncertain-input-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let candidate = "c37eb75a";
    // A2: a timeout, a signal-killed launcher or a printed ID may have started a job; A1: a refusal did not (#1557, #1555).
    let cases: &[&str] = if cfg!(unix) {
        &[
            "timeout",
            "response",
            "exit-id",
            "killed",
            "log",
            "copy",
            "no-start",
            "refused",
            "untrusted",
        ]
    } else {
        &[
            "timeout",
            "response",
            "exit-id",
            "log",
            "copy",
            "no-start",
            "refused",
            "untrusted",
        ]
    };
    for &case in cases {
        let log = root.join(format!("{case}.log"));
        let mut l = Launch {
            files: String::new(),
            execution: Some(44),
            task: 7,
            title: String::new(),
            attempt: 42,
            role: "worker".into(),
            rt: "claude".into(),
            session: uuid(),
            url: String::new(),
            resume: Some("continue".into()),
        };
        let (input, lock, path) = claude_input(&root, "private task body", None).unwrap();
        for execution in [None, Some(41)] {
            l.execution = execution;
            assert!(bind_input(&log, &l, &path, &input, serde_json::json!({})).is_err());
            assert!(
                !log.exists(),
                "invalid intent must refuse before native submission"
            );
        }
        l.execution = Some(44);
        assert!(
            bind_input(&root, &l, &path, &input, serde_json::json!({})).is_err(),
            "unwritable binding must refuse before native submission"
        );
        bind_input(
            &log,
            &l,
            &path,
            &input,
            serde_json::json!({"before":10,"session":l.session}),
        )
        .unwrap();
        let written = std::fs::read_to_string(&log).unwrap();
        let record: serde_json::Value = serde_json::from_str(written.trim()).unwrap();
        assert_eq!(
            (
                record["task"].as_u64(),
                record["attempt"].as_u64(),
                record["execution"].as_u64()
            ),
            (Some(7), Some(42), Some(44))
        );
        assert_eq!(record["input"], input);
        assert!(!written.contains("private task body"));
        let mut turn = ClaudeTurn {
            root: root.clone(),
            log: log.clone(),
            session: l.session.clone(),
            id: if case == "copy" {
                uuid()
            } else {
                String::new()
            },
            announced: false,
            accepted: false,
            history: ClaudeHistory::default(),
            before: 10,
            mode: "dontAsk".into(),
            input: input.clone(),
            _input_file: lock,
            pid: 0,
            native: None,
            recovery: None,
            done: false,
            failure: None,
            handed: false,
            released: false,
            launch_error: None,
            resumed: None,
            lost: None,
        };
        if case == "log" {
            std::fs::remove_file(&log).unwrap();
            std::fs::create_dir(&log).unwrap();
        }
        let output = match case {
            "timeout" => Err(coreq::process::Failure::Timeout),
            "no-start" => Err(coreq::process::Failure::Spawn(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "fixture missing launcher",
            ))),
            "killed" => Err(coreq::process::Failure::Exit(std::process::Output {
                status: std::process::ExitStatus::from_raw(9),
                stdout: vec![],
                stderr: b"native launch failed".to_vec(),
            })),
            "exit-id" => Err(coreq::process::Failure::Exit(std::process::Output {
                status: std::process::ExitStatus::from_raw(256),
                stdout: format!("backgrounded · {candidate}\n").into_bytes(),
                stderr: b"remote control unavailable".to_vec(),
            })),
            "refused" => Err(coreq::process::Failure::Exit(std::process::Output {
                status: std::process::ExitStatus::from_raw(256),
                stdout: vec![],
                stderr: b"Error: Background sessions are disabled.\nAsk your administrator.\n"
                    .to_vec(),
            })),
            // #1555: Claude's own refusal of an untrusted root
            "untrusted" => Err(coreq::process::Failure::Exit(std::process::Output {
                status: std::process::ExitStatus::from_raw(256),
                stdout: vec![],
                stderr: b"Error: Workspace not trusted. Run claude here first.".to_vec(),
            })),
            _ => Ok(std::process::Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: if case == "response" {
                    b"lost response".to_vec()
                } else {
                    format!("backgrounded · {candidate}\n").into_bytes()
                },
                stderr: vec![],
            }),
        };
        let result = turn.finish_launch(output);
        if case == "no-start" {
            assert!(result.unwrap_err().no_start);
        } else if case == "untrusted" {
            let e = result.unwrap_err();
            assert!(
                e.no_start
                    && e.class() == "trust-setup"
                    && e.message.contains("service retries automatically"),
                "{}",
                e.message
            );
            assert!(
                !e.message.contains(&root.display().to_string()),
                "board text never holds a local path"
            );
        } else if case == "refused" {
            let e = result.unwrap_err();
            assert!(
                e.no_start
                    && e.message.contains(
                        "refused: Error: Background sessions are disabled. Ask your administrator."
                    ),
                "{}",
                e.message
            );
            let x = crate::core::Execution {
                id: 44,
                attempt: 42,
                through: 0,
                host: "mac".into(),
                runtime: "claude".into(),
                accepted: false,
                release: None,
                usage_hold: None,
            };
            let attempt = attempt_log(&root, "worker", 7, 42);
            std::fs::copy(&log, &attempt).unwrap();
            assert_eq!(
                refused_launch(&root, 7, &x).as_deref(),
                Some("Error: Background sessions are disabled. Ask your administrator."),
                "saved refusal reconciles cold"
            );
            std::fs::remove_file(attempt).unwrap();
        } else {
            assert!(
                result.is_ok(),
                "{case}: uncertain native side effect must retain its handle"
            );
            assert!(turn.launch_error.is_some());
            if case == "exit-id" {
                assert_eq!(turn.id, candidate);
            }
            if case == "killed" {
                assert!(
                    turn.launch_error
                        .as_deref()
                        .unwrap()
                        .contains("native launch failed")
                );
                let reason = turn.recover_job(&[], Some(&root)).unwrap_err();
                assert!(
                    reason.contains("native launch failed")
                        && reason.contains("original input not found"),
                    "native refusal was masked: {reason}"
                );
            }
            assert_eq!(turn.input, input);
            assert!(turn.observe(&serde_json::json!({"id":turn.id,"sessionId":turn.session,"state":"failed","pid":null})).unwrap_err().contains("current input acceptance unconfirmed"),"old job status cannot settle the current submission");
            if matches!(case, "log" | "copy") {
                assert_eq!(
                    turn.id, candidate,
                    "returned native identity survives a subsequent failure"
                );
            }
            let probe = File::options().read(true).write(true).open(&path).unwrap();
            assert!(
                matches!(probe.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
                "uncertain turn keeps its original input"
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: lost launch reply -> exact native input lookup -> resumed observation without resubmission.
// Risk: a missing identity stays orphaned, or unrelated/ambiguous history is adopted as this execution.
// #1627 `native-cwd`: `claude agents` names the plain root, not the canonical (Windows `\\?\`) form.
// #2209 `resume-two-*`: a resumed session lists its earlier job too; only a unique input readback picks the job.
#[test]
fn claude_lost_reply_recovers_only_its_unique_input() {
    let root = std::env::temp_dir().join(format!("taskq-claude-lookup-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let session = uuid();
    let other = uuid();
    std::fs::create_dir_all(root.join("projects/a")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.join("missing-native-directory"),
        root.join("projects/missing"),
    )
    .unwrap();
    for id in ["c37eb75a", "eaff90e7"] {
        std::fs::create_dir_all(root.join("jobs").join(id)).unwrap();
    }
    for case in [
        "match",
        "native-cwd",
        "known",
        "blocks",
        "absent",
        "two-jobs",
        "two-inputs",
        "partial",
        "invalid",
        "oversize",
        "missing",
        "foreign",
        "resume",
        "old",
        "copied",
        "large-unrelated",
        "locator-only",
        "bad-locator",
        "foreign-locator",
        "large-locator",
        "long-history",
        "resume-two-jobs",
        "resume-two-inputs",
    ] {
        let (input, lock, path) = claude_input(&root, "fixture task body", None).unwrap();
        let event=serde_json::json!({"type":"user","message":{"content":if case=="blocks" {serde_json::json!([{"type":"text","text":input}])} else {serde_json::json!(input)}}}).to_string()+"\n";
        let a = root.join("projects/a").join(format!("{session}.jsonl"));
        let b = root.join("projects/a").join(format!("{other}.jsonl"));
        let content=match case {
                "absent"=>serde_json::json!({"type":"user","message":{"content":"unrelated input"}}).to_string()+"\n",
                "two-inputs"=>event.repeat(2),
                "partial"=>event.clone()+"{",
                "invalid"=>event.clone()+"{}\n",
                "oversize"=>"x".repeat(8*1024*1024+1),
                "long-history"=>(serde_json::json!({"type":"system","padding":"x".repeat(1024*1024)}).to_string()+"\n").repeat(18)+&event
                    +&serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"owned final"}]}}).to_string()+"\n"
                    +&serde_json::json!({"type":"system","subtype":"turn_duration"}).to_string()+"\n"
                    +&serde_json::json!({"type":"user","message":{"content":"another turn"}}).to_string()+"\n"
                    +&serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"foreign final"}]}}).to_string()+"\n",
                "resume"|"resume-two-jobs"|"resume-two-inputs"=>serde_json::json!({"type":"system"}).to_string()+"\n"+&event,
                _=>event.clone(),
            };
        std::fs::write(&a, content).unwrap();
        std::fs::write(&b, &event).unwrap();
        let shared = case.starts_with("resume-two");
        for (id, sid) in [
            ("c37eb75a", &session),
            ("eaff90e7", if shared { &session } else { &other }),
        ] {
            let intent = if case == "locator-only"
                || case == "resume"
                || (matches!(case, "large-unrelated" | "resume-two-jobs") && id == "eaff90e7")
            {
                "unrelated original input"
            } else {
                &input
            };
            let locator = serde_json::json!({"cwd":root,"sessionId":sid,"intent":intent,"providerEnv":{"FIXTURE":"not retained or printed"}});
            std::fs::write(
                root.join("jobs").join(id).join("state.json"),
                locator.to_string(),
            )
            .unwrap();
        }
        let locator = root.join("jobs/c37eb75a/state.json");
        if case == "bad-locator" {
            std::fs::write(&locator, "{").unwrap();
        }
        if case == "foreign-locator" {
            std::fs::write(
                &locator,
                serde_json::json!({"cwd":root,"sessionId":other,"intent":input}).to_string(),
            )
            .unwrap();
        }
        if case == "large-locator" {
            std::fs::write(&locator, "x".repeat(1024 * 1024 + 1)).unwrap();
        }
        if case == "large-unrelated" {
            std::fs::write(&b, "x".repeat(8 * 1024 * 1024 + 1)).unwrap();
        }
        let mut turn = ClaudeTurn {
            root: root.clone(),
            log: root.join("attempt.log"),
            session: if matches!(case, "resume" | "old") || shared {
                session.clone()
            } else if case == "copied" {
                other.clone()
            } else {
                String::new()
            },
            id: if matches!(case, "known" | "two-jobs" | "resume") {
                "c37eb75a".into()
            } else {
                String::new()
            },
            announced: false,
            accepted: false,
            history: ClaudeHistory::default(),
            before: usize::from(matches!(case, "resume" | "old") || shared),
            mode: "dontAsk".into(),
            input: input.clone(),
            _input_file: lock,
            pid: 0,
            native: None,
            recovery: None,
            done: false,
            failure: None,
            handed: false,
            released: false,
            launch_error: Some("launch response lost".into()),
            resumed: None,
            lost: None,
        };
        let plain = if cfg!(windows) {
            PathBuf::from(root.to_str().unwrap().trim_start_matches(r"\\?\"))
        } else {
            root.join("projects/..")
        };
        let cwd = match case {
            "foreign" => root.parent().unwrap(),
            "native-cwd" => plain.as_path(),
            _ => root.as_path(),
        };
        let mut rows = vec![
            serde_json::json!({"id":"c37eb75a","kind":"background","sessionId":session,"cwd":cwd,"state":"working"}),
        ];
        if matches!(case, "two-jobs" | "large-unrelated") {
            rows.push(serde_json::json!({"id":"eaff90e7","kind":"background","sessionId":other,"cwd":root,"state":"working"}));
        }
        // #2209: the resumed conversation's earlier job stays listed on the same session beside the job that took the input.
        if shared {
            rows.insert(0,serde_json::json!({"id":"eaff90e7","kind":"background","sessionId":session,"cwd":root,"state":"done"}));
        }
        rows.push(serde_json::json!({"id":"abcdef12","kind":"background","cwd":root.join("deleted-worktree"),"state":"done"}));
        if case == "missing" {
            std::fs::remove_file(&a).unwrap();
        }
        let original_id = turn.id.clone();
        let mut result = turn.recover_job(&rows, Some(&root));
        if case == "long-history" {
            let mut passes = 1;
            while result.as_ref().is_err_and(|e| e.contains("scan pending")) {
                assert!(!turn.accepted);
                assert_eq!(turn.id, original_id);
                assert!(passes < 5);
                result = turn.recover_job(&rows, Some(&root));
                passes += 1;
            }
            assert!(passes >= 3);
            assert!(turn.history.ended);
            assert_eq!(turn.history.answer.as_deref(), Some("owned final"));
            assert!(
                !std::fs::read_to_string(&turn.log)
                    .unwrap()
                    .contains("foreign final")
            );
            assert_eq!(
                claude_history(&a, 0, |_| {}).unwrap(),
                23,
                "pre-submission scan supports the same large history"
            );
        }
        if matches!(
            case,
            "match"
                | "native-cwd"
                | "known"
                | "blocks"
                | "resume"
                | "large-unrelated"
                | "long-history"
                | "resume-two-jobs"
        ) {
            result.unwrap();
            assert_eq!(turn.id, "c37eb75a");
            assert_eq!(turn.session, session);
            assert_eq!(turn.accepted(), Ok(true));
            assert!(!turn.done, "input acceptance is not native exit");
            turn.recover_job(&rows, None).unwrap();
        } else {
            assert!(result.is_err(), "{case} must remain unknown");
            assert_eq!(turn.id, original_id);
            assert!(!turn.accepted);
        }
        assert_eq!(turn.input, input);
        assert!(path.exists());
        let file = File::options().read(true).write(true).open(path).unwrap();
        assert!(
            matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "recovery retains the original input lock"
        );
    }
    let projects = root.join("projects");
    std::fs::create_dir_all(projects.join("a")).unwrap();
    std::fs::create_dir_all(projects.join("b")).unwrap();
    std::fs::write(projects.join("unrelated-file"), "").unwrap();
    let native = projects.join("a").join(format!("{session}.jsonl"));
    std::fs::write(&native, "{}\n").unwrap();
    assert_eq!(claude_transcript_in(&root, &session).unwrap(), native);
    std::fs::write(projects.join("b").join(format!("{session}.jsonl")), "{}\n").unwrap();
    assert!(
        claude_transcript_in(&root, &session)
            .unwrap_err()
            .contains("multiple transcript paths")
    );
    assert!(claude_transcript_in(&root, "../other").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: an owned native probe loses its launch reply -> candidate recovers input and observes exit.
// Reuses an already-stopped probe; never dispatches another model turn.
#[test]
#[ignore = "owned native probe; TASKQ_CLAUDE_READBACK selects its captured lifecycle evidence"]
fn native_claude_lost_reply_readback() {
    let evidence =
        PathBuf::from(std::env::var("TASKQ_CLAUDE_READBACK").expect("TASKQ_CLAUDE_READBACK"));
    let read = |file: &str| {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(evidence.join(file)).unwrap(),
        )
        .unwrap()
    };
    let manifest = read("manifest.json");
    let result = read("stop-result.json");
    let identity = read("identity-before.json");
    let pid = u32::try_from(identity["pid"].as_u64().unwrap()).unwrap();
    assert_eq!(
        coreq::process::identity(pid).unwrap(),
        None,
        "probe must already have exited"
    );
    assert_eq!(result["stop"]["exit_code"], 0);
    let root = PathBuf::from(manifest["native_root"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let input_path = evidence.join("input.txt");
    let input = std::fs::read_to_string(&input_path).unwrap();
    let log = evidence.join("candidate-readback.log");
    File::create(&log).unwrap();
    let mut turn = ClaudeTurn {
        root,
        log,
        session: String::new(),
        id: String::new(),
        announced: false,
        accepted: false,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "inherited-probe".into(),
        input,
        _input_file: coreq::file_lock::try_lock_shared_file(File::open(input_path).unwrap())
            .unwrap(),
        pid,
        native: Some(coreq::process::Identity {
            pid,
            started: identity["started"].as_str().unwrap().into(),
        }),
        recovery: None,
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: Some("fixture: launch reply lost".into()),
        resumed: None,
        lost: None,
    };
    assert_eq!(turn.exited(), Ok(Some(0)));
    assert_eq!(turn.accepted(), Ok(true));
    assert_eq!(turn.id, result["job"].as_str().unwrap());
    assert_eq!(turn.session, result["session"].as_str().unwrap());
    assert_eq!(coreq::process::identity(pid).unwrap(), None);
}

// Chain: § 8 Access adapter on the host OS (A3, A4). Risk: a Full grant keeps the runtime's own permission or
// sandbox flags on start or resume, or a host without enforcement starts a non-Full worker or reviewer.
#[test]
fn access_maps_full_and_refuses_unknown_enforcement_before_start() {
    let install = std::env::temp_dir().join(format!("taskq-access-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&install);
    std::fs::create_dir_all(&install).unwrap();
    let codex: Vec<String> = [
        "-s",
        "workspace-write",
        "-m",
        "gpt-5",
        "-c",
        "model_reasoning_effort=high",
    ]
    .map(String::from)
    .into();
    let root = std::env::temp_dir();
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("/i/taskq"),
        install: &install,
        permission_mode: "dontAsk",
        codex: &codex,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let args = |c: &Command| {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut l = Launch {
        files: String::new(),
        execution: None,
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "worker".into(),
        rt: "claude".into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    crate::policy::grant(&install, "claude", "full", None, "op", 1).unwrap();
    crate::policy::grant(&install, "codex", "full", None, "op", 1).unwrap();
    for resume in [None, Some("answer".to_string())] {
        l.resume = resume;
        l.rt = "claude".into();
        assert!(
            args(&command(&env, &l, &access(&env, "claude").unwrap()).unwrap())
                .contains("--permission-mode bypassPermissions")
        );
        l.rt = "codex".into();
        assert!(
            args(&command(&env, &l, &access(&env, "codex").unwrap()).unwrap()).starts_with(
                "exec --json -s danger-full-access -m gpt-5 -c model_reasoning_effort=high "
            )
        );
    }
    // the host without enforcement (every OS but macOS): no grant or a non-Full one refuses before any child or fetch
    if !cfg!(target_os = "macos") {
        for (rt, mode) in [
            ("claude", "none"),
            ("codex", "workspace"),
            ("claude", "restricted"),
        ] {
            crate::policy::grant(&install, rt, mode, None, "op", 2).unwrap();
            l.rt = rt.into();
            let e = spawn(&env, &l).err().expect("refused");
            assert!(
                e.message.starts_with("access refused: ")
                    && e.message.contains(std::env::consts::OS),
                "{}",
                e.message
            );
        }
    }
    let _ = std::fs::remove_dir_all(&install);
}

// Chain: #2221/#2188 resume with flags -> Claude continues in a copy job on a new conversation -> the copy hands in
// -> the service restarts and recovers the execution with no recorded process -> the copy's process exits.
// Defect: recovery refused the copy forever and exit needed a never-recorded identity, so the execution was never
// released and the requeued task never admitted. Protection kept: without this attempt's hand-in, the copy stays
// refused and a terminal row without a recorded process is no exit.
#[test]
fn handed_in_copy_job_is_adopted_and_exits_without_recorded_process() {
    let root = std::env::temp_dir().join(format!("taskq-claude-copy-{}", uuid()));
    std::fs::create_dir_all(root.join("projects/a")).unwrap();
    let root = root.canonicalize().unwrap();
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    let (saved, copy) = (uuid(), uuid());
    let (input, lock, _) = claude_input(&root, "resumed answer body", None).unwrap();
    let line = |v: serde_json::Value| v.to_string() + "\n";
    std::fs::write(
        root.join("projects/a").join(format!("{saved}.jsonl")),
        line(serde_json::json!({"type":"user","message":{"content":"earlier input"}})),
    )
    .unwrap();
    std::fs::write(root.join("projects/a").join(format!("{copy}.jsonl")),line(serde_json::json!({"type":"user","message":{"content":input}}))
            +&line(serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"handed in"}]}}))
            +&line(serde_json::json!({"type":"system","subtype":"turn_duration"}))).unwrap();
    for (id, session, intent) in [
        ("c3f6f318", &saved, "earlier input"),
        ("94f2ba2a", &copy, input.as_str()),
    ] {
        std::fs::create_dir_all(root.join("jobs").join(id)).unwrap();
        std::fs::write(
            root.join("jobs").join(id).join("state.json"),
            serde_json::json!({"cwd":root,"sessionId":session,"intent":intent}).to_string(),
        )
        .unwrap();
    }
    let done = serde_json::json!({"id":"94f2ba2a","kind":"background","sessionId":copy,"cwd":root,"state":"done"});
    let rows = [
        serde_json::json!({"id":"c3f6f318","kind":"background","sessionId":saved,"cwd":root,"state":"stopped"}),
        done.clone(),
    ];
    // The § 4 recovery shape: saved session from the binding, no job id, no recorded process.
    let mut t = ClaudeTurn {
        root: root.clone(),
        log: root.join("attempt.log"),
        session: saved.clone(),
        id: String::new(),
        announced: false,
        accepted: false,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "unknown: recovered observer".into(),
        input,
        _input_file: lock,
        pid: 0,
        native: None,
        recovery: Some(Recovery {
            root: root.clone(),
            install,
            task: 2188,
            attempt: 10,
            execution: 11,
            runtime: "claude".into(),
        }),
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: Some("recovered execution; original native input acceptance pending".into()),
        resumed: None,
        lost: None,
    };
    let refused = t.recover_job(&rows, Some(&root)).unwrap_err();
    assert!(
        refused.contains("copied job 94f2ba2a"),
        "no hand-in: the copy stays refused: {refused}"
    );
    assert!(t.id.is_empty() && !t.accepted && t.persist().is_err());
    assert!(
        claude_exit(&done, None, false).is_err(),
        "no hand-in: a terminal row alone is no exit"
    );
    // #2285: a board Exit release lets the copy stand in for its exit; acceptance still needs the transcript readback
    t.released();
    t.recover_job(&rows, Some(&root)).unwrap();
    assert_eq!(
        (t.id.as_str(), t.accepted, t.handed),
        ("94f2ba2a", t.history.ended, false),
        "a board Exit is no hand-in"
    );
    t.accept();
    t.recover_job(&rows, Some(&root)).unwrap();
    assert_eq!(
        (t.id.as_str(), t.session.as_str()),
        ("94f2ba2a", copy.as_str()),
        "the hand-in adopts the copy holding this exact input"
    );
    assert_eq!(t.accepted(), Ok(true));
    assert!(t.history.ended);
    assert_eq!(t.history.answer.as_deref(), Some("handed in"));
    t.capture_identity(&done).unwrap();
    t.persist().unwrap();
    assert!(
        t.native.is_none() && t.recovery.is_some(),
        "no process was observed, so none is recorded"
    );
    assert_eq!(
        claude_exit(&done, None, true),
        Ok(true),
        "handed-in terminal row without a process is the exit"
    );
    let mut live = done.clone();
    live["pid"] = std::process::id().into();
    assert_eq!(
        claude_exit(&live, None, true),
        Ok(false),
        "a listed process is never an exit"
    );
    for state in ["working", "idle"] {
        live["state"] = state.into();
        live["pid"] = serde_json::Value::Null;
        assert_eq!(
            claude_exit(&live, None, true),
            Ok(false),
            "{state} is no exit"
        );
    }
    let me = coreq::process::identity(std::process::id()).unwrap();
    assert_eq!(
        claude_exit(&done, me.as_ref(), true),
        Ok(false),
        "a recorded live process still decides"
    );
    let _ = std::fs::remove_dir_all(root);
}

// Chain: #2272 answer -> Claude resume launched -> no native process and no transcript input -> deadline -> lost exit.
// Risk: an answered resume that never started waits as "accepting input" forever; a live process or an accepted
// input is taken for a lost one.
#[test]
fn resumed_claude_job_without_process_or_input_is_lost_after_the_accept_deadline() {
    let root = std::env::temp_dir().join(format!("taskq-claude-lost-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let session = uuid();
    let (input, lock, _) = claude_input(&root, "answered resume body", None).unwrap();
    let mut t = ClaudeTurn {
        root: root.clone(),
        log: root.join("attempt.log"),
        session: session.clone(),
        id: "5d1e0b7a".into(),
        announced: false,
        accepted: false,
        history: ClaudeHistory::default(),
        before: 3,
        mode: "dontAsk".into(),
        input,
        _input_file: lock,
        pid: 0,
        native: None,
        recovery: None,
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: None,
        resumed: Some(std::time::Instant::now()),
        lost: None,
    };
    let mut row = serde_json::json!({"id":"5d1e0b7a","kind":"background","sessionId":session,"cwd":root,"state":"done"});
    let early = t.observe(&row).unwrap_err();
    assert!(
        early.starts_with(
            "no native process after resume: Claude job done input not accepted 0 s after launch"
        ),
        "the wait names its cause: {early}"
    );
    assert_eq!(
        crate::snapshot::wait_text(&early),
        crate::snapshot::WAIT_TEXTS[2]
    );
    assert!(
        !t.done && t.lost_input().is_none(),
        "before the deadline nothing ends"
    );
    row["pid"] = std::process::id().into();
    let late = CLAUDE_RESUME_ACCEPT + std::time::Duration::from_secs(1);
    assert_eq!(
        t.lost_resume(&row, late),
        Ok(None),
        "a listed live process is never lost"
    );
    row["pid"] = serde_json::Value::Null;
    t.native = coreq::process::identity(std::process::id()).unwrap();
    assert_eq!(
        t.lost_resume(&row, late),
        Ok(None),
        "a recorded live process is never lost"
    );
    t.native = None;
    t.resumed = std::time::Instant::now().checked_sub(late);
    assert_eq!(
        t.observe(&row),
        Ok(Some(-1)),
        "past the deadline the resume ends"
    );
    let lost = t.lost_input().unwrap();
    assert!(
        lost.starts_with("no native process after resume: Claude job done input not accepted 30"),
        "{lost}"
    );
    assert!(
        lost.ends_with("released and requeued after 300 s"),
        "{lost}"
    );
    assert_eq!(
        (t.done, t.exit_error(), t.exited()),
        (true, Some("lost-input"), Ok(Some(-1)))
    );
    let mut fresh = ClaudeTurn {
        resumed: None,
        lost: None,
        done: false,
        failure: None,
        ..t
    };
    fresh.handed = true;
    fresh.resumed = std::time::Instant::now().checked_sub(late);
    let handed = fresh.observe(&row);
    assert!(
        handed != Ok(Some(-1)) && fresh.lost_input().is_none() && !fresh.done,
        "a hand-in is acceptance, never a lost input: {handed:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

// Defect #2262: a recovered Claude execution whose worker entered `.worktrees/taskq-N` never matched its native
// job (cwd below the root), so acceptance stayed pending and status showed "progress source unavailable" until
// hand-in. Protection kept: a job whose cwd lies outside the project root is never adopted.
#[test]
fn recovered_worktree_claude_job_is_accepted_and_reports_progress() {
    let base = std::env::temp_dir().join(format!("taskq-claude-wt-{}", uuid()));
    let root = base.join("project");
    for d in ["projects/a", ".worktrees/taskq-7"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::create_dir_all(base.join("other")).unwrap();
    let (root, other) = (
        root.canonicalize().unwrap(),
        base.join("other").canonicalize().unwrap(),
    );
    let session = uuid();
    let (input, lock, _) = claude_input(&root, "worktree task body", None).unwrap();
    let line = |v: serde_json::Value| v.to_string() + "\n";
    std::fs::write(root.join("projects/a").join(format!("{session}.jsonl")),line(serde_json::json!({"type":"user","message":{"content":input}}))
            +&line(serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}))).unwrap();
    let mut t = ClaudeTurn {
        root: root.clone(),
        log: root.join("attempt.log"),
        session: session.clone(),
        id: String::new(),
        announced: false,
        accepted: false,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "unknown: recovered observer".into(),
        input,
        _input_file: lock,
        pid: 0,
        native: None,
        recovery: None,
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: Some("recovered execution; original native input acceptance pending".into()),
        resumed: None,
        lost: None,
    };
    let row = |cwd: &Path| serde_json::json!({"id":"5be0c1d2","kind":"background","sessionId":session,"cwd":cwd,"state":"working"});
    let refused = t.recover_job(&[row(&other)], Some(&root)).unwrap_err();
    assert!(
        refused.contains("original input not found") && !t.accepted,
        "outside the root stays unknown: {refused}"
    );
    assert!(t.progress_source().is_err());
    t.recover_job(&[row(&root.join(".worktrees/taskq-7"))], Some(&root))
        .unwrap();
    assert!(t.accepted && t.id == "5be0c1d2", "worktree job adopted");
    assert_eq!(
        t.progress_source().map(|(p, _)| p),
        Ok(root.join("attempt.log")),
        "native progress readable"
    );
    let _ = std::fs::remove_dir_all(base);
}

// Chain: native identity -> failed/retried protected checkpoint -> cold observer -> exact process exit.
// Risk: cold Claude recovery recaptures a reused PID, or failed persistence drops the only original identity.
#[test]
fn claude_cold_identity_survives_checkpoint_failure_and_process_exit() {
    let root = std::env::temp_dir().join(format!("taskq-claude-cold-{}", uuid()));
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    let l = Launch {
        files: String::new(),
        execution: Some(42),
        task: 7,
        title: String::new(),
        attempt: 42,
        role: "worker".into(),
        rt: "claude".into(),
        session: uuid(),
        url: String::new(),
        resume: None,
    };
    let log = log_path(&root, &l);
    let (input, lock, path) = claude_input(&root, "controlled process fixture", Some(&l)).unwrap();
    bind_input(
        &log,
        &l,
        &path,
        &input,
        serde_json::json!({"session":l.session}),
    )
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime::tests::stop_child", "--ignored"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut t = ClaudeTurn {
        root: root.clone(),
        log,
        session: l.session.clone(),
        id: "c37eb75a".into(),
        announced: false,
        accepted: true,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "fixture".into(),
        input,
        _input_file: lock,
        pid: 0,
        native: None,
        recovery: Some(Recovery {
            root: root.clone(),
            install: install.clone(),
            task: 7,
            attempt: 42,
            execution: 42,
            runtime: "claude".into(),
        }),
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: None,
        resumed: None,
        lost: None,
    };
    let row =
        serde_json::json!({"id":t.id,"sessionId":t.session,"pid":child.id(),"state":"working"});
    t.capture_identity(&row).unwrap();
    let original = t.native_identity().unwrap();
    std::fs::write(
        install.join("native"),
        "fixture: unavailable recovery directory",
    )
    .unwrap();
    assert!(t.persist().unwrap_err().contains("checkpoint failed"));
    assert_eq!(t.native_identity(), Ok(original.clone()));
    assert!(t.recovery.is_some());
    std::fs::remove_file(install.join("native")).unwrap();
    t.persist().unwrap();
    assert!(t.recovery.is_none());
    let saved = crate::service_run::kids_file(&install, &root).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].process, original);
    assert_eq!(saved[0].runtime, "claude");
    assert_eq!(saved[0].offset, 0);
    let mut conflicting = row.clone();
    conflicting["pid"] = std::process::id().into();
    assert!(t.capture_identity(&conflicting).is_err());
    assert_eq!(t.native_identity(), Ok(original.clone()));
    let x = crate::core::Execution {
        id: 42,
        attempt: 42,
        through: 42,
        host: "mac".into(),
        runtime: "claude".into(),
        accepted: true,
        release: None,
        usage_hold: None,
    };
    drop(t);
    let restored = recover(&root, &install, 7, &x).unwrap();
    assert_eq!(restored.native_identity(), Ok(original.clone()));
    assert_eq!(
        claude_stopped(&serde_json::json!({"state":"done"}), Some(&original)),
        Ok(false)
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        claude_stopped(&serde_json::json!({"state":"done"}), Some(&original)),
        Ok(true)
    );
    drop(restored);
    let after_exit = recover(&root, &install, 7, &x).unwrap();
    assert_eq!(
        after_exit.native_identity(),
        Ok(original),
        "cold recovery does not recapture an absent/reused PID"
    );
    let mut other = x.clone();
    other.id = 43;
    assert!(recover(&root, &install, 7, &other).is_err());
    let protected = crate::paths::kids_path(&install, &root).unwrap();
    std::fs::write(&protected, "corrupt").unwrap();
    assert!(recover(&root, &install, 7, &x).is_err());
    assert_eq!(std::fs::read_to_string(protected).unwrap(), "corrupt");
    drop(after_exit);
    std::fs::remove_dir_all(root).unwrap();
}

// #2057 Chain: image with an untracked helper and a worker child -> exec -> both exit -> host reap.
// Defect: the old image's helpers (curl, `open -W`, `log show`) stayed zombies, ~3 per re-exec. Risk: the reap
// misses them, or takes the adopted worker's exit status.
#[cfg(unix)]
#[test]
fn reexec_reaps_untracked_children_and_keeps_adopted_status() {
    let root = std::env::temp_dir().join(format!("taskq-inherit-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let result = coreq::process::output(
        "qualification.reexec-reap",
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime::tests::reexec_reap_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_PHASE", "before"),
        std::time::Duration::from_secs(30),
    );
    assert!(result.is_ok(), "re-exec reap fixture failed: {result:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("verified")).unwrap(),
        "helper reaped; worker exit 7"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "re-exec reap fixture"]
fn reexec_reap_child() {
    use std::os::unix::process::CommandExt;
    let root = PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
    if std::env::var("TASKQ_IMAGE_PHASE").unwrap() == "before" {
        // Handles drop at exec, as a dying update or link thread drops its curl.
        let helper = Command::new("sleep").arg("1").spawn().unwrap().id();
        let worker = Command::new("sh")
            .args(["-c", "sleep 1; exit 7"])
            .spawn()
            .unwrap()
            .id();
        std::fs::write(root.join("pids"), format!("{helper} {worker}")).unwrap();
        let error = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime::tests::reexec_reap_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_PHASE", "after")
            .exec();
        panic!("exec failed: {error}");
    }
    let pids: Vec<u32> = std::fs::read_to_string(root.join("pids"))
        .unwrap()
        .split(' ')
        .map(|p| p.parse().unwrap())
        .collect();
    let (helper, worker) = (pids[0], pids[1]);
    let mut inherited = inherit_children().unwrap();
    inherited.sort();
    let mut want = pids.clone();
    want.sort();
    assert_eq!(inherited, want, "the new image sees both old children");
    let mut adopted = Adopted::new(coreq::process::identity(worker).unwrap().unwrap());
    assert_eq!(adopted.exited(), Ok(None), "worker still runs");
    let exited = |pid: u32| {
        // exited, not yet reaped
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            return false;
        }
        #[cfg(target_os = "linux")]
        return unsafe { info.si_pid() } != 0;
        #[cfg(not(target_os = "linux"))]
        return info.si_pid != 0;
    };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !(exited(helper) && exited(worker)) {
        assert!(std::time::Instant::now() < until, "children did not exit");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // The host reap runs first and takes both statuses; neither stays a zombie.
    assert_eq!(reap_inherited(), 2);
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    for pid in [helper, worker] {
        assert_eq!(
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            },
            -1,
            "{pid} left a zombie"
        );
    }
    assert_eq!(
        adopted.exited(),
        Ok(Some(7)),
        "adopted worker keeps its exit status"
    );
    std::fs::write(root.join("verified"), "helper reaped; worker exit 7").unwrap();
}

// Chain: live Claude background turn -> protected checkpoint -> image re-exec -> recovered observer -> native exit -> board exit (#1407).
// Risk: re-exec resubmits the original input, loses the native answer, or releases the board before the native process exits.
#[cfg(unix)]
#[test]
fn claude_turn_survives_image_reexec() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("taskq-claude-image-{}", uuid()));
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let root = root.canonicalize().unwrap();
    // Stand-in native CLI: answers job readback only and records every call, so any submission or stop shows up.
    let fake = root.join("bin/claude");
    std::fs::write(&fake,"#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKQ_IMAGE_ROOT/claude-calls\"\n[ \"$1\" = agents ] && exec cat \"$TASKQ_IMAGE_ROOT/agents.json\"\nexit 9\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(root.join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let result = coreq::process::output(
        "qualification.claude-image-handover",
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime::tests::claude_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_PHASE", "before")
            .env("PATH", path)
            .env("CLAUDE_CONFIG_DIR", root.join("home")),
        std::time::Duration::from_secs(30),
    );
    assert!(result.is_ok(), "Claude image handover failed: {result:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("verified")).unwrap(),
        "no resubmission; kept answer; exit after native exit"
    );
    let calls = std::fs::read_to_string(root.join("claude-calls")).unwrap();
    assert!(
        calls
            .lines()
            .all(|c| c == format!("agents --json --all --cwd {}", root.display())),
        "handover may only read native jobs: {calls}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "Claude image handover fixture"]
fn claude_image_child() {
    use crate::board::Board;
    use std::os::unix::process::CommandExt;
    let root = PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
    let home = root.join("home");
    // The in-memory board is rebuilt identically in both images; only the native record crosses exec.
    let b = crate::board::Fake::default();
    let n = b
        .create("T", "", &[crate::service::TASK_LABEL.into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:session mac")
        .unwrap();
    let c = crate::service::Config {
        root: root.clone(),
        install: root.join("install"),
        ..crate::tests::cfg()
    };
    let mut s = crate::service::Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|_| panic!("handover must not submit new work")),
    );
    let session = "0b6c3f4e-8f2a-4c1d-9e7b-5a2d1c3b4e6f";
    let transcript = home.join("projects/p").join(format!("{session}.jsonl"));
    let agents = |state: &str, pid: Option<u32>| {
        std::fs::write(root.join("agents.json"),serde_json::json!([{"id":"c37eb75a","kind":"background","sessionId":session,"cwd":root,"state":state,"pid":pid}]).to_string()).unwrap()
    };
    if std::env::var("TASKQ_IMAGE_PHASE").unwrap() == "before" {
        let l = Launch {
            files: String::new(),
            execution: Some(a),
            task: n,
            title: String::new(),
            attempt: a,
            role: "worker".into(),
            rt: "claude".into(),
            session: "session".into(),
            url: String::new(),
            resume: None,
        };
        let log = log_path(&root, &l);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        let (input, lock, path) = claude_input(&root, "image handover fixture", Some(&l)).unwrap();
        bind_input(
            &log,
            &l,
            &path,
            &input,
            serde_json::json!({"session":l.session}),
        )
        .unwrap();
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::create_dir_all(home.join("jobs/c37eb75a")).unwrap();
        std::fs::write(
            &transcript,
            serde_json::json!({"type":"user","message":{"content":input}}).to_string() + "\n",
        )
        .unwrap();
        std::fs::write(
            home.join("jobs/c37eb75a/state.json"),
            serde_json::json!({"cwd":root,"sessionId":session,"intent":input}).to_string(),
        )
        .unwrap();
        // An orphaned sleep stands in for the native process: not this image's child, so its exit leaves no zombie.
        let out = Command::new("sh")
            .args(["-c", "sleep 60 >/dev/null 2>&1 & echo $!"])
            .output()
            .unwrap();
        let pid: u32 = String::from_utf8(out.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::fs::write(root.join("native-pid"), pid.to_string()).unwrap();
        agents("working", Some(pid));
        let turn = ClaudeTurn {
            root: root.clone(),
            log,
            session: String::new(),
            id: "c37eb75a".into(),
            announced: false,
            accepted: false,
            history: ClaudeHistory::default(),
            before: 0,
            mode: "dontAsk".into(),
            input,
            _input_file: lock,
            pid: 0,
            native: None,
            recovery: None,
            done: false,
            failure: None,
            handed: false,
            released: false,
            launch_error: None,
            resumed: None,
            lost: None,
        };
        s.kids.push(crate::service::Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution: a,
            task: n,
            attempt: a,
            rt: "claude".into(),
            proc: Box::new(turn),
        });
        assert!(
            !crate::service_run::lifecycle_waits(&s, false).contains(&format!("#{n}")),
            "#2297: a turn without native identity passes to the next image"
        );
        s.kids[0].proc.checkpoint(&c, &l, a).unwrap();
        assert!(
            !crate::service_run::lifecycle_waits(&s, false).contains(&format!("#{n}")),
            "saved native identity: re-exec passes over the live turn"
        );
        crate::service_run::save_kids(&s, &root).unwrap();
        coreq::telemetry::capture::before_exec();
        let error = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime::tests::claude_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_PHASE", "after")
            .exec();
        panic!("exec failed: {error}");
    }
    let pid: u32 = std::fs::read_to_string(root.join("native-pid"))
        .unwrap()
        .parse()
        .unwrap();
    crate::service_run::adopt(&mut s, &c.install, &root).unwrap();
    crate::service_run::adopt(&mut s, &c.install, &root).unwrap();
    assert_eq!(s.kids.len(), 1);
    assert_eq!(s.kids[0].execution, a);
    assert_eq!(s.kids[0].proc.id(), pid);
    let accepted = || {
        crate::core::executions(n, &crate::board::entries(&b, n).unwrap())
            .into_iter()
            .find(|x| x.id == a)
            .unwrap()
    };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !accepted().accepted {
        s.reap_owned();
        assert!(
            std::time::Instant::now() < until,
            "acceptance not observed: {:?}",
            s.kids.iter().map(|k| &k.unknown).collect::<Vec<_>>()
        );
    }
    let mut file = File::options().append(true).open(&transcript).unwrap();
    writeln!(file,"{}\n{}",serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"owned final"}]}}),serde_json::json!({"type":"system","subtype":"turn_duration"})).unwrap();
    s.reap_owned();
    assert_eq!(s.kids.len(), 1, "a finished answer is not native exit");
    assert_eq!(accepted().release, None);
    Command::new("kill").arg(pid.to_string()).status().unwrap();
    // An exiting orphan may briefly have no readable creation identity until init reaps it.
    while !matches!(coreq::process::identity(pid), Ok(None)) {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    agents("done", None);
    while !s.kids.is_empty() {
        s.reap_owned();
        assert!(
            std::time::Instant::now() < until,
            "native exit not observed: {:?}",
            s.kids.iter().map(|k| &k.unknown).collect::<Vec<_>>()
        );
    }
    assert_eq!(accepted().release, Some(crate::core::Release::Exit));
    let log = std::fs::read_to_string(attempt_log(&root, "worker", n, a)).unwrap();
    assert_eq!(
        log.matches("\"result\":\"owned final\"").count(),
        1,
        "answer kept once"
    );
    crate::service_run::save_kids(&s, &root).unwrap();
    assert!(
        crate::service_run::kids_file(&c.install, &root)
            .unwrap()
            .is_empty()
    );
    std::fs::write(
        root.join("verified"),
        "no resubmission; kept answer; exit after native exit",
    )
    .unwrap();
}

// #1561 Chain: two Claude jobs launched apart -> native rows -> recovery records.
// Defect: the later job's record held the earlier job's process; its exit then failed every handover pass.
// Retained: each record carries its own job's process; a process another task's record holds is refused.
#[cfg(unix)]
#[test]
fn claude_records_carry_their_own_process() {
    let root = std::env::temp_dir().join(format!("taskq-claude-own-{}", uuid()));
    let install = root.join("install");
    std::fs::create_dir_all(&root).unwrap();
    let mut first = Command::new("sleep").arg("30").spawn().unwrap();
    let mut second = Command::new("sleep").arg("30").spawn().unwrap();
    let turn = |task: u64, id: &str| ClaudeTurn {
        root: root.clone(),
        log: root.join(format!("{task}.log")),
        session: String::new(),
        id: id.into(),
        announced: false,
        accepted: true,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "dontAsk".into(),
        input: "input".into(),
        _input_file: coreq::file_lock::try_lock_shared_file(
            File::open(std::env::current_exe().unwrap()).unwrap(),
        )
        .unwrap(),
        pid: 0,
        native: None,
        recovery: Some(Recovery {
            install: install.clone(),
            root: root.clone(),
            task,
            attempt: task * 10,
            execution: task * 10,
            runtime: "claude".into(),
        }),
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: None,
        resumed: None,
        lost: None,
    };
    let row = |id: &str, pid: u32| serde_json::json!({"id":id,"state":"working","pid":pid});
    let mut a = turn(1557, "c2fc6503");
    a.capture_identity(&row("c2fc6503", first.id())).unwrap();
    a.persist().unwrap();
    let mut b = turn(1558, "90d3662a");
    let refused = b
        .capture_identity(&row("90d3662a", first.id()))
        .unwrap_err();
    assert!(
        refused.contains("#1557"),
        "the earlier job's process is not this job's identity: {refused}"
    );
    assert!(b.native.is_none(), "a refused readback keeps no identity");
    b.capture_identity(&row("90d3662a", second.id())).unwrap();
    b.persist().unwrap();
    let records: Vec<_> = crate::service_run::kids_file(&install, &root)
        .unwrap()
        .into_iter()
        .map(|k| (k.task, k.process.pid))
        .collect();
    assert_eq!(
        records,
        [(1557, first.id()), (1558, second.id())],
        "each record carries its own job's process"
    );
    let mut stolen = crate::service_run::kids_file(&install, &root).unwrap()[0].clone();
    stolen.task = 1559;
    assert!(
        crate::service_run::checkpoint_kid(&install, &root, stolen)
            .unwrap_err()
            .contains("#1557"),
        "no record takes another task's process"
    );
    for c in [&mut first, &mut second] {
        c.kill().unwrap();
        c.wait().unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

// #1638 Chain: recorded Claude process -> service restart -> native job reports a new process -> recovered readback.
// Defect: the recovered turn compared the new pid with the record and fenced the execution as unknown forever.
// Retained: a live recorded process, an unproved input or an unknown conversation still refuse the new process.
#[cfg(unix)]
#[test]
fn recovered_claude_job_on_a_new_process_resolves() {
    let root = std::env::temp_dir().join(format!("taskq-claude-moved-{}", uuid()));
    let install = root.join("install");
    std::fs::create_dir_all(&root).unwrap();
    let mut old = Command::new("sleep").arg("30").spawn().unwrap();
    let mut new = Command::new("sleep").arg("30").spawn().unwrap();
    let recorded = coreq::process::identity(old.id()).unwrap().unwrap();
    crate::service_run::checkpoint_kid(
        &install,
        &root,
        crate::data::KidLine {
            task: 1592,
            attempt: 10,
            runtime: "claude".into(),
            process: recorded.clone(),
            execution: 11,
            offset: 0,
            job: None,
        },
    )
    .unwrap();
    let turn = |accepted: bool, session: &str| ClaudeTurn {
        root: root.clone(),
        log: root.join("1592.log"),
        session: session.into(),
        id: "c37eb75a".into(),
        announced: false,
        accepted,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "dontAsk".into(),
        input: "input".into(),
        _input_file: coreq::file_lock::try_lock_shared_file(
            File::open(std::env::current_exe().unwrap()).unwrap(),
        )
        .unwrap(),
        pid: 0,
        native: Some(recorded.clone()),
        recovery: Some(Recovery {
            install: install.clone(),
            root: root.clone(),
            task: 1592,
            attempt: 10,
            execution: 11,
            runtime: "claude".into(),
        }),
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: None,
        resumed: None,
        lost: None,
    };
    let session = "0b6c3f4e-8f2a-4c1d-9e7b-5a2d1c3b4e6f";
    let row =
        serde_json::json!({"id":"c37eb75a","sessionId":session,"state":"working","pid":new.id()});
    let fenced = |mut t: ClaudeTurn| {
        let e = t.capture_identity(&row).unwrap_err();
        assert!(e.contains("process changed"), "{e}");
        assert_eq!(t.native.as_ref(), Some(&recorded));
    };
    fenced(turn(true, session));
    old.kill().unwrap();
    old.wait().unwrap();
    fenced(turn(false, session));
    fenced(turn(true, ""));
    let mut t = turn(true, session);
    t.capture_identity(&row).unwrap();
    t.persist().unwrap();
    assert_eq!(
        t.id(),
        new.id(),
        "the job's new process stands in for the exited one"
    );
    let records: Vec<_> = crate::service_run::kids_file(&install, &root)
        .unwrap()
        .into_iter()
        .map(|k| (k.task, k.execution, k.process.pid))
        .collect();
    assert_eq!(
        records,
        [(1592, 11, new.id())],
        "the record follows the same execution to its new process"
    );
    new.kill().unwrap();
    new.wait().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

// #1561 Chain: live Claude handle -> recorded process exits -> handover pass -> board exit -> handover.
// Defect: every pass failed with "child identity changed during handover" and the pending update never ran.
// Retained: before the board exit the record stays for the next image (#2297); after it the record resolves.
#[cfg(unix)]
#[test]
fn exited_process_resolves_from_board_exit_at_handover() {
    use crate::board::Board;
    let root = std::env::temp_dir().join(format!("taskq-exited-{}", uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let b = crate::board::Fake::default();
    let n = b
        .create("T", "", &[crate::service::TASK_LABEL.into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:session mac")
        .unwrap();
    let c = crate::service::Config {
        root: root.clone(),
        install: root.clone(),
        ..crate::tests::cfg()
    };
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let native = coreq::process::identity(child.id()).unwrap().unwrap();
    crate::service_run::checkpoint_kid(
        &root,
        &root,
        crate::data::KidLine {
            task: n,
            attempt: a,
            runtime: "claude".into(),
            process: native.clone(),
            execution: a,
            offset: 0,
            job: None,
        },
    )
    .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    let turn = ClaudeTurn {
        root: root.clone(),
        log: root.join("attempt.log"),
        session: String::new(),
        id: "c37eb75a".into(),
        announced: false,
        accepted: true,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "dontAsk".into(),
        input: "input".into(),
        _input_file: coreq::file_lock::try_lock_shared_file(
            File::open(std::env::current_exe().unwrap()).unwrap(),
        )
        .unwrap(),
        pid: 0,
        native: Some(native),
        recovery: None,
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: None,
        resumed: None,
        lost: None,
    };
    let mut s = crate::service::Service::new(&b, &c, "test", Box::new(|_| unreachable!()));
    s.kids.push(crate::service::Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: a,
        task: n,
        attempt: a,
        rt: "claude".into(),
        proc: Box::new(turn),
    });
    let path = crate::paths::kids_path(&root, &root).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    crate::service_run::save_kids(&s, &root).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        saved,
        "#2297: an unreleased record stays and the handover goes on"
    );
    b.comment(
        n,
        &format!("taskq: released {a} exit attempt {a}\nop-id: release-{a}"),
    )
    .unwrap();
    crate::service_run::save_kids(&s, &root).unwrap();
    assert!(
        crate::service_run::kids_file(&root, &root)
            .unwrap()
            .is_empty(),
        "board exit resolves the record; handover completes"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: native job schema -> observation -> release fence.
// #2254 defect: a worker that entered `.worktrees/taskq-N` left its native job cwd below the project root; every
// stop refused "background project differs", the kid stayed unknown and the Windows re-exec waited forever.
// Kept protection: a cwd outside the project root still refuses.
#[test]
fn claude_stop_binds_worktree_job_inside_project_only() {
    let base = std::env::temp_dir().join(format!("taskq-2254-{}", uuid()));
    let (root, tree, other) = (
        base.join("project"),
        base.join("project/.worktrees/taskq-2239"),
        base.join("other"),
    );
    for d in [&tree, &other] {
        std::fs::create_dir_all(d).unwrap();
    }
    let row = |cwd: &Path| serde_json::json!({"id":"c37eb75a","kind":"background","cwd":cwd,"sessionId":uuid(),"state":"idle","pid":4242});
    for cwd in [&root, &tree] {
        let (spec, job) = claude_stop_job(&root, &row(cwd), "c37eb75a", None).unwrap();
        assert_eq!(
            spec.root.canonicalize().unwrap(),
            cwd.canonicalize().unwrap()
        );
        assert_eq!((job.id.as_str(), job.pid), ("c37eb75a", Some(4242)));
    }
    assert_eq!(
        claude_stop_job(&root, &row(&other), "c37eb75a", None)
            .err()
            .as_deref(),
        Some("background project differs")
    );
    std::fs::remove_dir_all(base).unwrap();
}

// Risk: missing optional fields or a new state fabricate exit or hide observation failures.
#[test]
fn claude_native_state_requires_exit_evidence() {
    let root = std::env::temp_dir().join(format!("taskq-claude-state-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let log = root.join("attempt.log");
    File::create(&log).unwrap();
    let mut t = ClaudeTurn {
        root: root.clone(),
        log,
        session: uuid(),
        id: "c37eb75a".into(),
        announced: false,
        accepted: false,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "dontAsk".into(),
        input: "invented input".into(),
        _input_file: coreq::file_lock::try_lock_shared_file(
            File::open(std::env::current_exe().unwrap()).unwrap(),
        )
        .unwrap(),
        pid: 0,
        native: None,
        recovery: None,
        done: false,
        failure: None,
        handed: false,
        released: false,
        launch_error: None,
        resumed: None,
        lost: None,
    };
    assert!(native_job_id(&t.id));
    assert!(native_job_id(&uuid()));
    for id in ["--all", "../c37eb75a", "c37eb75a --all", "invented-job", ""] {
        assert!(!native_job_id(id));
    }
    for state in ["failed", "stopped", "crashed", "unexpected"] {
        assert!(
            t.observe(&serde_json::json!({"state":state})).is_err(),
            "{state} without process proof must not release"
        );
    }
    for state in ["done", "blocked", "failed", "crashed", "unexpected"] {
        assert_ne!(
            claude_stopped(&serde_json::json!({"state":state}), None),
            Ok(true),
            "{state} is not a process exit receipt"
        );
    }
    assert!(claude_stopped(&serde_json::json!({"state":"stopped"}), None).is_err());
    assert_eq!(
        claude_stopped(
            &serde_json::json!({"state":"stopped"}),
            coreq::process::identity(std::process::id())
                .unwrap()
                .as_ref()
        ),
        Ok(false)
    );
    assert_eq!(
        claude_stopped(
            &serde_json::json!({"state":"done"}),
            coreq::process::identity(std::process::id())
                .unwrap()
                .as_ref()
        ),
        Ok(false)
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime::tests::stop_child", "--ignored"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let identity = coreq::process::identity(child.id()).unwrap().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        claude_stopped(&serde_json::json!({"state":"done"}), Some(&identity)),
        Ok(true),
        "native stop can preserve done after observed process exit"
    );
    for pid in [
        serde_json::json!(0),
        serde_json::json!(u64::MAX),
        serde_json::json!("unknown"),
    ] {
        assert!(claude_stopped(&serde_json::json!({"state":"stopped","pid":pid}), None).is_err());
    }
    for row in [serde_json::json!({}), serde_json::json!({"state":null})] {
        assert!(t.observe(&row).is_err());
    }
    let row = serde_json::json!({"id":t.id,"state":"working","sessionId":t.session});
    assert!(
        t.observe(&row)
            .unwrap_err()
            .contains("transcript unavailable"),
        "no status field must not suppress the real observation error"
    );
    assert!(t.announced);
    // A completed input survives asynchronous stop; an API failure never becomes a valid final answer.
    for api_error in [false, true] {
        t.history = ClaudeHistory::default();
        t.done = false;
        t.failure = None;
        t.accepted = true;
        let path = root.join(format!("completed-{api_error}.jsonl"));
        let mut assistant = serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":if api_error {"Login expired; please run /login"} else {"owned final"}}]}});
        if api_error {
            assistant["error"] = serde_json::json!("authentication_failed");
            assistant["isApiErrorMessage"] = serde_json::json!(true);
        }
        let events = [
            serde_json::json!({"type":"user","message":{"content":t.input}}),
            assistant,
            serde_json::json!({"type":"system","subtype":"turn_duration"}),
        ];
        std::fs::write(
            &path,
            events.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        assert!(t.history.read(&path, 0, &t.input, &t.log).unwrap());
        assert!(t.history.ended);
        let row = serde_json::json!({"id":t.id,"sessionId":t.session,"state":"stopped","pid":null});
        t.native = coreq::process::identity(std::process::id()).unwrap();
        assert!(
            t.observe(&row).is_err(),
            "completed input and stop acknowledgment cannot replace process exit proof"
        );
        t.native = Some(identity.clone());
        assert_eq!(
            t.observe(&row),
            Ok(Some(if api_error { -1 } else { 0 })),
            "completed input outcome lost after stopped"
        );
        assert_eq!(
            t.exited(),
            Ok(Some(if api_error { -1 } else { 0 })),
            "terminal result must remain stable"
        );
        if api_error {
            assert_eq!(t.exit_error(), Some("auth"));
            assert!(
                t.history.answer.is_none(),
                "native API error cannot be a success answer"
            );
            assert!(
                t.history
                    .failure
                    .as_ref()
                    .unwrap()
                    .1
                    .contains("authentication_failed: Login expired; please run /login")
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

// #2330 Chain: hand-in -> service stops the turn still running it -> native row `stopped` with no process -> reap.
// Risk: the service's own stop logs a clean hand-in as exit failed/other, or the exemption hides a real failure.
#[test]
fn claude_handed_in_job_stopped_by_the_service_exits_ok_and_real_failures_stay_failed() {
    let root = std::env::temp_dir().join(format!("taskq-claude-handed-stop-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let log = root.join("attempt.log");
    File::create(&log).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime::tests::stop_child", "--ignored"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let gone = coreq::process::identity(child.id()).unwrap().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    let live = coreq::process::identity(std::process::id()).unwrap();
    let session = uuid();
    let turn = |handed: bool, native: Option<coreq::process::Identity>| ClaudeTurn {
        root: root.clone(),
        log: log.clone(),
        session: session.clone(),
        id: "c37eb75a".into(),
        announced: false,
        accepted: handed,
        history: ClaudeHistory::default(),
        before: 0,
        mode: "dontAsk".into(),
        input: "invented input".into(),
        _input_file: coreq::file_lock::try_lock_shared_file(
            File::open(std::env::current_exe().unwrap()).unwrap(),
        )
        .unwrap(),
        pid: 0,
        native,
        recovery: None,
        done: false,
        failure: None,
        handed,
        released: false,
        launch_error: None,
        resumed: None,
        lost: None,
    };
    let row = |state: &str, detail: Option<&str>| serde_json::json!({"id":"c37eb75a","sessionId":session,"state":state,"detail":detail,"pid":null});
    for native in [None, Some(gone.clone())] {
        let mut t = turn(true, native.clone());
        assert_eq!(
            t.observe(&row("stopped", Some("stopped"))),
            Ok(Some(0)),
            "stopped after hand-in, identity {native:?}"
        );
        assert_eq!(t.exit_error(), None, "a clean hand-in has no exit reason");
        assert_eq!(
            t.exited(),
            Ok(Some(0)),
            "terminal result must remain stable"
        );
        for (state, detail, class) in [
            ("failed", "permission denied", "permission"),
            (
                "crashed",
                "ENAMETOOLONG: name too long, uv_spawn",
                "argument-too-long",
            ),
            ("failed", "exit 4", "other"),
        ] {
            let mut t = turn(true, native.clone());
            assert_eq!(
                t.observe(&row(state, Some(detail))),
                Ok(Some(-1)),
                "native {state} after hand-in stays failed"
            );
            assert_eq!(t.exit_error(), Some(class), "{state} keeps its reason");
        }
    }
    // Without a hand-in a stopped job is still no clean exit; its attempt is released as an ordinary failure.
    let mut t = turn(false, Some(gone));
    assert_eq!(t.observe(&row("stopped", Some("stopped"))), Ok(Some(-1)));
    assert_eq!(t.exit_error(), Some("other"));
    // A hand-in never stands in for process exit: the recorded process still runs, so the stop stays pending.
    let mut t = turn(true, live);
    assert!(
        t.observe(&row("stopped", Some("stopped")))
            .unwrap_err()
            .contains("native process exit unconfirmed")
    );
    assert!(!t.done);
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: § 8 codex-app handshake -> open thread -> queue -> one new `task_complete` on this host's app.
// Paid/client-dependent qualification: deliberately excluded from the normal CI gate.
#[test]
#[ignore = "live runtime matrix; set TASKQ_APP_ROOT=<git dir> and run explicitly"]
fn live_app_turn_completes_once() {
    let root = PathBuf::from(std::env::var("TASKQ_APP_ROOT").expect("TASKQ_APP_ROOT"));
    let install = std::env::temp_dir().join(format!("taskq-app-{}", std::process::id()));
    std::fs::create_dir_all(&install).unwrap();
    crate::policy::grant(&install, "codex-app", "full", None, "op", 1).unwrap();
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("taskq"),
        install: &install,
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let l = Launch {
        files: String::new(),
        execution: None,
        task: 0,
        title: "live".into(),
        attempt: 0,
        role: "worker".into(),
        rt: "codex-app".into(),
        session: "-".into(),
        url: String::new(),
        resume: None,
    };
    let log = root.join("app-turn.log");
    let (text, input, _path) = app_input(
        &root,
        &format!(
            "{}Reply with the single word QUEUED and do nothing else.",
            "A taskq test line; ignore it.\n".repeat(700)
        ),
        None,
    )
    .unwrap();
    let mut t = AppTurn {
        root: root.canonicalize().unwrap(),
        history: AppHistory::default(),
        hand: None,
        log: log.clone(),
        text,
        _input: Some(input),
        binding: None,
        thread: String::new(),
        done: 0,
        rollout: None,
        running: app_running,
        control: app_cancel,
        cancelled: false,
        queue_error: None,
        accepted: false,
    };
    t.hand = Some(
        start(
            &env,
            &l,
            &log,
            HANDSHAKE,
            &access(&env, "codex-app").unwrap(),
        )
        .unwrap(),
    );
    let code = (0..600).find_map(|_| {
        std::thread::sleep(std::time::Duration::from_secs(1));
        t.exited().unwrap()
    });
    eprintln!(
        "thread {} rollout {:?} completes {} exit {code:?}",
        t.thread,
        t.rollout,
        t.rollout.as_ref().map_or(0, |p| completes(p).unwrap())
    );
    let _ = std::fs::remove_dir_all(&install);
    assert_eq!(code, Some(0));
    assert_eq!(
        completes(t.rollout.as_ref().unwrap()).unwrap(),
        t.done + 1,
        "exactly one queued turn completed"
    );
}

// Chain: § 8 Windows app brief -> PowerShell -> taskq. Risk: quoted paths break parsing or execute input.
#[cfg(windows)]
#[test]
fn app_brief_powershell_values_round_trip() {
    let root = Path::new("C:\\project's $cash; folder");
    let exe = Path::new("C:\\tool's $cash; folder\\taskq.exe");
    let env = Env {
        context: "",
        root,
        exe,
        install: Path::new("C:\\install"),
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let l = Launch {
        files: String::new(),
        execution: None,
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "worker's $cash; role".into(),
        rt: "codex-app".into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    let text = brief(&env, &l);
    let command = text
        .split("your inbox: `")
        .nth(1)
        .unwrap()
        .split('`')
        .next()
        .unwrap();
    let prefix = command.split("; & ").next().unwrap();
    let executable = command
        .split("; & ")
        .nth(1)
        .unwrap()
        .strip_suffix(" inbox 7")
        .unwrap();
    let script = format!(
        "{prefix}; ConvertTo-Json -Compress @($env:TASKQ_TASK, $env:TASKQ_ATTEMPT, $env:TASKQ_ROLE, $env:TASKQ_ROOT, {executable})"
    );
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let values: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        values,
        vec![
            "7",
            "42",
            &l.role,
            &root.display().to_string(),
            &exe.display().to_string()
        ]
    );
}

// Chain: § 7 sandbox-blocked host checks. Risk: a worker requeues a sandbox limit or claims a local PASS.
#[test]
fn worker_brief_keeps_sandbox_blocked_checks_at_the_publication_ci_gate() {
    assert!(SANDBOX_CHECKS.contains("sandbox-blocked") && SANDBOX_CHECKS.contains("Checks"));
    assert!(SANDBOX_CHECKS.contains("never as PASS") && SANDBOX_CHECKS.contains("do not requeue"));
    assert!(SANDBOX_CHECKS.contains("green CI on the GitHub PR merge result (merge_group)"));
    assert!(SANDBOX_CHECKS.contains("private fallback uses exact-SHA CI"));
}

fn rules_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("taskq-rules-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("docs/sub")).unwrap();
    std::fs::write(
        root.join("docs/guide.md"),
        "Intro line\n## Guide title\nBody.",
    )
    .unwrap();
    std::fs::write(root.join("docs/sub/plain.md"), "No heading here.").unwrap();
    std::fs::write(root.join("docs/shot.png"), [0u8, 159, 146]).unwrap();
    root
}

// Chain: section 8 rule delivery. Risk: native runtime argv contains a full brief or its file is pruned mid-turn.
#[test]
fn claude_large_input_stays_out_of_argv() {
    let root = rules_root("background input ' quoted");
    let text = format!("{}\nUTF-8: \u{2605}\n", "invented padding ".repeat(6400));
    assert!(text.len() > 100 * 1024);
    let mut inputs = Vec::new();
    for (turn, app) in [
        ("start", false),
        ("resume", false),
        ("concurrent operator", false),
        ("app start", true),
        ("app resume", true),
        ("app concurrent", true),
    ] {
        let brief = format!("{text}{turn}");
        let (input, _file, _path) = if app {
            app_input(&root, &brief, None)
        } else {
            claude_input(&root, &brief, None)
        }
        .unwrap();
        assert!(input.len() < 8192 && !input.contains("invented padding"));
        assert!(input.contains("in full"));
        if app {
            assert!(input.contains("do not proceed"));
        } else {
            assert!(input.contains("use Read"));
        }
        assert!(
            !inputs.contains(&input),
            "each turn has an immutable input file"
        );
        let prefix = if app {
            "Before any work, read "
        } else {
            "Before any work, use Read to read "
        };
        let quoted = input
            .strip_prefix(prefix)
            .unwrap()
            .split(" in full")
            .next()
            .unwrap();
        let path: String = serde_json::from_str(quoted).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), brief);
        let probe = File::options().read(true).write(true).open(&path).unwrap();
        assert!(matches!(
            probe.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(_file);
        probe.try_lock().unwrap();
        inputs.push(input);
    }
    assert_eq!(
        std::fs::read_dir(root.join(".taskq/inputs"))
            .unwrap()
            .count(),
        6
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn rule(url: &str) -> Result<String, String> {
    if url == SPECQ_REFS {
        return Ok(advertisement(SPECQ_REVISION));
    }
    if let Some(revision) = url
        .strip_prefix(SPECQ_TREES)
        .and_then(|s| s.strip_suffix("?recursive=1"))
    {
        return Ok(tree_fixture(revision).to_string());
    }
    Ok(format!("# Rule from {url}\nApply this rule."))
}

fn pinned(
    root: &Path,
    revision: &str,
    file: &str,
    fetch: &mut impl FnMut(&str) -> Result<String, String>,
) -> Result<String, StartError> {
    let body = rule(&format!(
        "https://raw.githubusercontent.com/taskqorg/specq/{revision}/{file}"
    ))
    .unwrap();
    super::pinned(root, revision, file, &git_blob_hash(&body), fetch)
}

fn tree_fixture(revision: &str) -> serde_json::Value {
    let entries=SPECQ_CORE.iter().chain(SPECQ_INDEX).map(|file| {
            let body=format!("# Rule from https://raw.githubusercontent.com/taskqorg/specq/{revision}/{file}\nApply this rule.");
            serde_json::json!({"path":file,"type":"blob","mode":"100644","sha":git_blob_hash(&body)})
        }).collect::<Vec<_>>();
    serde_json::json!({"sha":revision,"truncated":false,"tree":entries})
}

#[test]
fn newly_published_shared_file_is_indexed_and_cached_without_a_loader_change() {
    let root = rules_root("new-shared-file");
    let path = "engineering/new-shared-rule.md";
    let url = format!("https://raw.githubusercontent.com/taskqorg/specq/{SPECQ_REVISION}/{path}");
    let body = "# Newly published shared rule\nRead this rule on demand.\n";
    let mut fetched = Vec::new();
    let text=rules(&root,true,|request| {
            fetched.push(request.to_string());
            if request.starts_with(SPECQ_TREES) {
                let mut tree=tree_fixture(SPECQ_REVISION);
                for name in [path,"docs/excluded.md","vendor/excluded.md","README.md"] {
                    tree["tree"].as_array_mut().unwrap().push(serde_json::json!({"type":"blob","mode":"100644","path":name,"sha":git_blob_hash(body)}));
                }
                Ok(tree.to_string())
            } else if request==url {Ok(body.into())} else {rule(request)}
        }).unwrap();
    assert!(text.contains(&format!("- {path}: Newly published shared rule\n")));
    assert!(
        !text.contains("Read this rule on demand."),
        "noncore body remains demand-loaded"
    );
    assert!(!fetched.iter().any(|url| url.contains("excluded.md")));
    assert_eq!(
        std::fs::read_to_string(specq_dir(&root, SPECQ_REVISION).join(path)).unwrap(),
        body
    );
    assert_eq!(
        rules(&root, true, |_| Err(
            "must use fresh source/tree/body cache".into()
        ))
        .unwrap(),
        text
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn incomplete_unsafe_duplicate_or_nonregular_shared_trees_refuse_delivery() {
    let valid = tree_fixture(SPECQ_REVISION);
    assert_eq!(
        parse_shared_tree(&valid.to_string()).unwrap().len(),
        SPECQ_CORE.len() + SPECQ_INDEX.len()
    );
    let mut broken = Vec::new();
    let mut tree = valid.clone();
    tree["truncated"] = true.into();
    broken.push(tree);
    let mut tree = valid.clone();
    tree.as_object_mut().unwrap().remove("truncated");
    broken.push(tree);
    let mut tree = valid.clone();
    tree["tree"].as_array_mut().unwrap().remove(0);
    broken.push(tree);
    let mut tree = valid.clone();
    let duplicate = tree["tree"][0].clone();
    tree["tree"].as_array_mut().unwrap().push(duplicate);
    broken.push(tree);
    let mut tree = valid.clone();
    tree["tree"][0]["mode"] = "120000".into();
    broken.push(tree);
    let mut tree = valid.clone();
    tree["tree"][0]["sha"] = "invalid".into();
    broken.push(tree);
    let mut tree = valid.clone();
    tree["tree"][SPECQ_CORE.len()]["type"] = "unknown".into();
    broken.push(tree);
    let mut tree = valid.clone();
    tree["tree"] = serde_json::json!(vec![serde_json::json!({}); 1025]);
    broken.push(tree);
    for path in [
        "../escape.md",
        "/absolute.md",
        "C:/escape.md",
        "engineering/../escape.md",
        "engineering//empty.md",
        "engineering\\escape.md",
        "engineering/query?.md",
        "engineering/NUL.md",
        "engineering/COM1.md",
        "engineering./trailing.md",
        "ENGINEERING/PRINCIPLES.md",
    ] {
        let mut tree = valid.clone();
        let mut entry = tree["tree"][0].clone();
        entry["path"] = path.into();
        tree["tree"].as_array_mut().unwrap().push(entry);
        broken.push(tree);
    }
    for tree in broken {
        assert!(parse_shared_tree(&tree.to_string()).is_err());
    }
}

#[test]
fn source_validation_repairs_coherent_bad_caches_and_never_publishes_bad_fetches() {
    assert_eq!(
        git_blob_hash("abc"),
        "f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f"
    );
    let root = rules_root("source-validation");
    let revision = SPECQ_REVISION;
    let file = SPECQ_CORE[0];
    let expected = rule(&url(file)).unwrap();
    let hash = git_blob_hash(&expected);
    let path = specq_dir(&root, revision).join(file);
    assert!(
        super::pinned(&root, revision, file, &hash, &mut |_| Ok(
            "wrong source body".into()
        ))
        .is_err()
    );
    assert!(!path.exists());
    assert_eq!(
        super::pinned(&root, revision, file, &hash, &mut rule).unwrap(),
        expected
    );
    let bad = "# Corrupted coherent cache\n";
    std::fs::write(&path, bad).unwrap();
    std::fs::write(
        path.with_extension("cache.json"),
        rule_cache(revision, file, bad).to_string(),
    )
    .unwrap();
    assert!(super::pinned(&root, revision, file, &hash, &mut |_| Err("offline".into())).is_err());
    assert_eq!(
        super::pinned(&root, revision, file, &hash, &mut rule).unwrap(),
        expected
    );
    shared_files(&root, revision, &mut rule).unwrap();
    let tree = specq_dir(&root, revision).join("tree.json");
    std::fs::write(&tree, "{}").unwrap();
    std::fs::write(
        tree.with_extension("cache.json"),
        rule_cache(revision, "tree.json", "{}").to_string(),
    )
    .unwrap();
    assert!(shared_files(&root, revision, &mut |_| Ok("{}".into())).is_err());
    assert_eq!(std::fs::read_to_string(&tree).unwrap(), "{}");
    assert_eq!(
        shared_files(&root, revision, &mut rule).unwrap().len(),
        SPECQ_CORE.len() + SPECQ_INDEX.len()
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn advertisement(revision: &str) -> String {
    let line = format!("{revision} refs/heads/main\n");
    format!(
        "001e# service=git-upload-pack\n0000{:04x}{line}0000",
        line.len() + 4
    )
}

#[test]
fn git_advertisement_requires_complete_unambiguous_main_reference() {
    let valid = advertisement(SPECQ_REVISION);
    assert_eq!(advertised_revision(&valid).unwrap(), SPECQ_REVISION);
    for cut in 0..valid.len() {
        assert!(
            advertised_revision(&valid[..cut]).is_err(),
            "truncation {cut}"
        );
    }
    for bad in [
        valid.replace("refs/heads/main", "refs/heads/main-other"),
        valid.replace(SPECQ_REVISION, &"z".repeat(40)),
        valid.clone() + "0001",
        valid.clone() + &valid,
        valid.replace("003d", "ffff"),
        "0000".into(),
    ] {
        assert!(advertised_revision(&bad).is_err(), "{bad:?}");
    }
    let head = format!("{SPECQ_REVISION} HEAD\0multi_ack thin-pack\n");
    let with_head = format!(
        "001e# service=git-upload-pack\n0000{:04x}{head}{}",
        head.len() + 4,
        &valid[34..]
    );
    assert_eq!(advertised_revision(&with_head).unwrap(), SPECQ_REVISION);
    let other = format!("{SPECQ_REVISION} refs/heads/main-other\n");
    assert!(
        advertised_revision(&format!(
            "001e# service=git-upload-pack\n0000{:04x}{other}0000",
            other.len() + 4
        ))
        .is_err()
    );
}

#[test]
fn shared_revision_refreshes_at_bound_and_never_falls_back_after_failure() {
    let root = rules_root("source-freshness");
    let first = "a".repeat(40);
    let second = "b".repeat(40);
    assert_eq!(
        shared_revision_at(&root, 100, &mut |url| {
            assert_eq!(url, SPECQ_REFS);
            Ok(advertisement(&first))
        })
        .unwrap(),
        first
    );
    assert_eq!(
        shared_revision_at(&root, 159, &mut |_| Err("offline".into())).unwrap(),
        first
    );
    assert!(
        shared_revision_at(&root, 160, &mut |_| Err("offline".into()))
            .unwrap_err()
            .to_string()
            .contains("freshness")
    );
    assert_eq!(
        shared_revision_at(&root, 160, &mut |_| Ok(advertisement(&second))).unwrap(),
        second
    );
    assert!(shared_revision_at(&root, 159, &mut |_| Err("future cache".into())).is_err());
    assert!(shared_revision_at(&root, 220, &mut |_| Ok("invalid".into())).is_err());
    assert!(shared_revision_at(&root, 220, &mut |_| Ok("x".repeat(RULES_CAP + 1))).is_err());
    let path = root.join(".taskq/specq/source.json");
    for bad in [
        "{".to_string(),
        "{}".into(),
        serde_json::json!({"revision":"../escape","checked_at":100}).to_string(),
        "x".repeat(1025),
    ] {
        std::fs::write(&path, bad).unwrap();
        assert!(shared_revision_at(&root, 101, &mut |_| Err("offline".into())).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_moving_shared_head_cannot_mix_bodies_and_resume_gets_the_new_snapshot() {
    let root = rules_root("snapshot-consistency");
    let first = "a".repeat(40);
    let second = "b".repeat(40);
    let mut files = Vec::new();
    let initial = rules(&root, false, |url| {
        if url == SPECQ_REFS {
            return Ok(advertisement(&first));
        }
        files.push(url.to_string());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        std::fs::create_dir_all(root.join(".taskq/specq")).unwrap();
        std::fs::write(
            root.join(".taskq/specq/source.json"),
            serde_json::json!({"revision":second,"checked_at":now}).to_string(),
        )
        .unwrap();
        rule(url)
    })
    .unwrap();
    assert!(initial.starts_with(&format!("TaskQ rules: specq@{first} ")));
    assert_eq!(files.len(), 1 + SPECQ_CORE.len() + SPECQ_INDEX.len());
    assert!(files.iter().all(|url| url.contains(&format!("/{first}/"))
        || url == &format!("{SPECQ_TREES}{first}?recursive=1")));
    let resumed = rules(&root, true, rule).unwrap();
    assert!(resumed.starts_with(&format!("TaskQ rules: specq@{second} ")));
    assert!(!resumed.contains(&format!("/{first}/")));
    assert_eq!(resumed.matches("\n--- specq/").count(), SPECQ_CORE.len());
    std::fs::remove_dir_all(root).unwrap();
}

fn url(file: &str) -> String {
    format!("https://raw.githubusercontent.com/taskqorg/specq/{SPECQ_REVISION}/{file}")
}

// Chain: § 4 reviewer on a host with `default_runtime` codex (#1668). Risk: a codex reviewer gets other review rules,
// access check or verdict commands (close, requeue, ask) than a claude reviewer.
#[test]
fn codex_reviewer_brief_matches_the_claude_reviewer() {
    let root = rules_root("codex-review");
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("/i/taskq"),
        install: Path::new("/i"),
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let l = |rt: &str| Launch {
        files: String::new(),
        execution: None,
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "reviewer".into(),
        rt: rt.into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    // Only the command prefix differs: the native Claude session runs in its host's environment (§ 8).
    let host_env = format!(
        "env TASKQ_TASK=7 TASKQ_ATTEMPT=42 TASKQ_ROLE=reviewer TASKQ_ROOT={} ",
        root.display()
    );
    let (claude, codex) = (brief(&env, &l("claude")), brief(&env, &l("codex")));
    assert!(claude.contains(&host_env));
    assert_eq!(claude.replace(&host_env, ""), codex);
    for verdict in ["close 7", "requeue 7", "ask 7"] {
        assert!(codex.contains(verdict), "{verdict}");
    }
    std::fs::remove_dir_all(&root).ok();
}

// Chain: § 8 rule delivery, #1156. Risk: a new/resumed worker or reviewer misses the contract, a core rule or the index,
// or a continuation retains obsolete mandatory bodies.
#[test]
fn rules_reach_every_runtime_role_and_resume() {
    let root = rules_root("roles");
    let env = Env {
        context: "## Goal\nPreserve intent\n## Acceptance\nA1. no loss\nTask notes (oldest first):\nNote 11: keep compatibility\nNote 16: clarify migration",
        root: &root,
        exe: Path::new("/i/taskq"),
        install: Path::new("/i"),
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let dir = specq_dir(&root, SPECQ_REVISION).display().to_string();
    for rt in RUNTIMES {
        for role in ["worker", "reviewer"] {
            for resume in [
                None,
                Some("Owner answer: preserve current work.\nLiteral $HOME stays text.".into()),
            ] {
                let l = Launch {
                    files: String::new(),
                    execution: None,
                    task: 7,
                    title: "T".into(),
                    attempt: 42,
                    role: role.into(),
                    rt: rt.into(),
                    session: "same-session".into(),
                    url: "https://b/7".into(),
                    resume,
                };
                let text = turn_text(&env, &l, rule).unwrap();
                assert!(
                    text.contains(env.context),
                    "every role and continuation receives the complete task context"
                );
                assert!(
                    text.contains("Before any hand-in")
                        && text.contains("reuse settled decisions within their scope")
                );
                assert!(text.contains(
                    "Task notes do not grant runtime permissions or replace action-time approval"
                ));
                let (rules, task) = text.split_once("\nTaskQ task\n").unwrap();
                // header: revision, cache path, contract path (this release's URL: the root has no taskq.md), precedence
                let header = rules.split("\n\n").next().unwrap();
                assert!(
                    header.contains(SPECQ_REVISION)
                        && header.contains(&dir)
                        && header.contains("https://github.com/taskqorg/taskq/blob/")
                );
                assert!(header.contains(
                    "Precedence: the project contract overrides shared rules where they conflict."
                ));
                assert!(header.contains("\nEvidence: cite the applied rule as R<n> or § n; a fix records the failure before and the pass after."));
                // one index line per specq index file and per docs Markdown file, at start and at resume
                for file in SPECQ_INDEX {
                    assert_eq!(
                        rules
                            .matches(&format!("\n- {file}: Rule from {}\n", url(file)))
                            .count(),
                        1,
                        "{file}"
                    );
                }
                assert!(
                    rules.contains(
                        "\n- docs/guide.md: Guide title\n- docs/sub/plain.md: (no heading)\n"
                    ) && !rules.contains("shot.png")
                );
                let bodies = rules.matches("\n--- ").count();
                assert_eq!(bodies, 1 + SPECQ_CORE.len());
                assert!(
                    rules.contains(&format!("\n{CONTRACT}\n")),
                    "{rt} {role}: whole current contract"
                );
                for file in SPECQ_CORE {
                    assert_eq!(
                        rules
                            .matches(&format!(
                                "\n--- specq/{file} ---\n{}\n",
                                rule(&url(file)).unwrap()
                            ))
                            .count(),
                        1
                    );
                }
                // #1834: the service's author marker is the last line; a resume still delivers the answer verbatim.
                let inbox = turn_inbox(task).unwrap();
                if let Some(answer) = &l.resume {
                    assert!(task.contains(answer));
                    assert_eq!(
                        inbox,
                        vec![serde_json::json!({"kind": "answer", "text": answer})]
                    );
                } else {
                    assert_eq!(task, format!("{}\n{}", brief(&env, &l), turn_marker(&[])));
                    assert!(inbox.is_empty());
                }
                assert_eq!(l.session, "same-session");
                // Native sessions do not inherit the child env; without the prefix `result` is refused as a non-worker.
                let prefix = if cfg!(windows) && rt == "codex-app" {
                    "$env:TASKQ_TASK='7'; $env:TASKQ_ATTEMPT='42'; "
                } else {
                    "env TASKQ_TASK=7 TASKQ_ATTEMPT=42 "
                };
                assert_eq!(
                    brief(&env, &l).contains(prefix),
                    of(rt).has(Cap::RemoteControl) || of(rt).has(Cap::AppTurn),
                    "{rt} {role}: attempt identity"
                );
                assert!(task.contains(FILES) && task.contains(CI_WAIT));
                if role == "worker" {
                    assert!(
                        brief(&env, &l).contains(SANDBOX_CHECKS)
                            && brief(&env, &l).contains(DEV_LOOP)
                    );
                }
                // docs/testing.md: a reviewer reads exact-SHA CI and reruns only changed or unknown checks.
                assert_eq!(
                    brief(&env, &l).contains(REVIEW_CHECKS),
                    role == "reviewer",
                    "{rt} {role}: review checks"
                );
                // #1842: a worker publishes once; a moved main alone never sends it back to rebase or recheck.
                assert_eq!(
                    brief(&env, &l).contains(PUBLISH_ONCE),
                    role == "worker",
                    "{rt} {role}: publish once"
                );
                // cs#548 (#2078 A4): a read-only shared .git burned attempts on retries; the worker hands in one host gap
                assert_eq!(
                    brief(&env, &l).contains("do not retry: hand in once")
                        && brief(&env, &l).contains(
                            "Problem: host gap: the worker sandbox cannot write the shared .git"
                        ),
                    role == "worker",
                    "{rt} {role}: one exact host gap"
                );
                assert!(
                    brief(&env, &l).contains("later 7 --blocker #M --text")
                        && brief(&env, &l).contains("resumes when #M closes"),
                    "{rt} {role}: a park names its blocker task"
                );
                assert!(
                    brief(&env, &l).contains("later 7 --until <UTC> --text"),
                    "{rt} {role}: a wait for elapsed time parks until it, not requeue (#1823)"
                );
                assert!(
                    brief(&env, &l).contains("add `--stage A<k>` per finished item"),
                    "{rt} {role}: a stage park names its flag (#1880)"
                );
                // the attempt log's evidence line
                let log = root.join(format!(".taskq/{rt}-{role}.log"));
                std::fs::remove_file(&log).ok();
                rules_line(&log, "start", &text).unwrap();
                let v: serde_json::Value =
                    serde_json::from_str(std::fs::read_to_string(&log).unwrap().trim()).unwrap();
                assert_eq!(v["shared_revision"], SPECQ_REVISION);
                assert_eq!(
                    (
                        v["type"].as_str(),
                        v["header"].as_str(),
                        v["bodies"].as_u64()
                    ),
                    (Some("taskq.rules"), Some(header), Some(bodies as u64))
                );
                assert_eq!(v["index"].as_u64(), Some(SPECQ_INDEX.len() as u64 + 2));
            }
        }
    }
    std::fs::remove_dir_all(&root).ok();
}

// Chain: section 7 step 4 test repair guidance (#1245). Risk: repairs rejected or weakened assertions accepted without a rule change.
#[test]
fn briefs_distinguish_test_repairs_from_weakened_checks() {
    let env = Env {
        context: "",
        root: Path::new("/project"),
        exe: Path::new("taskq"),
        install: Path::new("/i"),
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let rule = "Test changes follow specq engineering/testing.md: name the defect and retained risk protection in --text; change a rule only when accepted behavior or protection changes (section 7 step 4).";
    for rt in RUNTIMES {
        for role in ["worker", "reviewer"] {
            let l = Launch {
                files: String::new(),
                execution: None,
                task: 7,
                title: "Repair".into(),
                attempt: 42,
                role: role.into(),
                rt: rt.into(),
                session: "s".into(),
                url: "https://b/7".into(),
                resume: None,
            };
            let text = brief(&env, &l);
            assert!(text.contains(rule), "{rt}: {role}");
            assert!(
                !text.contains("a changed or deleted existing test is a gap unless"),
                "{rt}: {role}: obsolete ban"
            );
        }
    }
}

// Chain: §7 research Reports handoff (#1826). Risk: finished research blocks the queue, loses its report,
// or a decision another task needs bypasses owner choice. The owner approved replacing the old ask gate.
#[test]
fn research_reports_close_without_answer_but_keep_blocking_decisions() {
    let env = Env {
        context: "",
        root: Path::new("/project"),
        exe: Path::new("taskq"),
        install: Path::new("/i"),
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    for rt in RUNTIMES {
        let mut l = Launch {
            files: String::new(),
            execution: None,
            task: 7,
            title: "Proposal".into(),
            attempt: 42,
            role: "worker".into(),
            rt: rt.into(),
            session: "s".into(),
            url: "https://b/7".into(),
            resume: None,
        };
        let worker = brief(&env, &l);
        assert!(worker.contains("Experiments (`research` or a `Hypothesis:` line, any type) deliver docs/research/7.md by PR"), "{rt}: report artifact");
        assert!(
            worker.contains("Ask only when another task waits on that decision"),
            "{rt}: blocking decision"
        );
        l.role = "reviewer".into();
        let reviewer = brief(&env, &l);
        assert!(reviewer.contains("need docs/research/<N>.md at the result SHA, not a prior owner answer, and a close line `Verdict: confirmed|refuted|inconclusive <evidence>`"), "{rt}: reviewer");
        assert!(
            reviewer.contains("Only a decision blocking another task needs ask before close"),
            "{rt}: blocking decision review"
        );
    }
}

// Chain: § 8 rule delivery after compaction, #1211. Risk: a compacted Claude worker loses mandatory rule bodies
// or task text again, or another runtime gets the hook.
#[test]
fn compaction_resends_header_and_index_only_to_claude() {
    let root = rules_root("compact");
    // fixture grant in a scoped install: hosts without enforcement refuse a missing grant (§ 8 Access)
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    for rt in ["claude", "codex"] {
        crate::policy::grant(&install, rt, "full", None, "op", 1).unwrap();
    }
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("/i/taskq"),
        install: &install,
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let launch = |rt: &str| Launch {
        files: String::new(),
        execution: None,
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "worker".into(),
        rt: rt.into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    let args = |rt: &str| {
        command(&env, &launch(rt), &access(&env, rt).unwrap())
            .unwrap()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    let l = launch("claude");
    let claude = args("claude");
    let settings: serde_json::Value =
        serde_json::from_str(&claude[claude.iter().position(|a| a == "--settings").unwrap() + 1])
            .unwrap();
    let hook = &settings["hooks"]["SessionStart"][0];
    assert_eq!(hook["matcher"], "compact");
    assert!(
        hook["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .ends_with(&format!(
                " rules compact '{}' 'worker' 7 42",
                root.display()
            ))
    );
    assert!(
        !args("codex").iter().any(|a| a == "--settings"),
        "codex gets no hook"
    );
    // #1531: the per-user Claude daemon spawns from its own env; settings carry this launch's values.
    // Risk: a worker reads another launch's TASKQ_* (install, task, attempt), or an operator session gets an attempt.
    let want = |task: &str, attempt: &str, role: &str| serde_json::json!({"TASKQ_INSTALL": install.to_string_lossy(), "TASKQ_ROOT": root.to_string_lossy(), "TASKQ_TASK": task, "TASKQ_ATTEMPT": attempt, "TASKQ_ROLE": role});
    let mut worker = want("7", "42", "worker");
    worker["AGENTQ_OPTIMIZATION"] = std::env::var("AGENTQ_OPTIMIZATION")
        .unwrap_or("inherit".into())
        .into();
    worker["AGENTQ_LAUNCHER"] = "taskq".into();
    if let Some(p) = build_cache_for("claude", &access(&env, "claude").unwrap(), &env) {
        worker["RUSTC_WRAPPER"] = p.to_string_lossy().into();
    }
    assert_eq!(
        settings["env"], worker,
        "a full-access Claude worker gets this host's build cache, if any"
    );
    let operator: serde_json::Value = serde_json::from_str(&compact_settings(
        &root,
        &install,
        &Launch {
            role: "operator".into(),
            ..launch("claude")
        },
        None,
        None,
    ))
    .unwrap();
    assert_eq!(
        operator["env"],
        want("", "", ""),
        "operator: empty means unset"
    );
    let cached: serde_json::Value = serde_json::from_str(&compact_settings(
        &root,
        &install,
        &launch("claude"),
        None,
        Some(Path::new("/b/sccache")),
    ))
    .unwrap();
    assert_eq!(
        cached["env"]["RUSTC_WRAPPER"], "/b/sccache",
        "the daemon's env is not the service's: settings carry the wrapper"
    );
    // The compact hook restores current mandatory bodies and index, without inventing a task.
    let start = turn_text(&env, &l, rule).unwrap();
    let text = compact(&root, "worker", 7, 42).unwrap();
    assert!(
        text.starts_with("TaskQ rules: specq@")
            && text.contains("\nEvidence: cite the applied rule")
    );
    assert!(text.contains(INDEX) && text.contains("\n- docs/guide.md: Guide title\n"));
    assert!(
        !text.contains("TaskQ task") && text.contains("Apply this rule."),
        "current mandatory bodies, no task"
    );
    assert_eq!(text.matches("\n--- ").count(), 1 + SPECQ_CORE.len());
    assert_eq!(text, rules(&root, true, rule).unwrap());
    assert!(start.len() > text.len());
    let log = std::fs::read_to_string(attempt_log(&root, "worker", 7, 42)).unwrap();
    let v: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(
        (
            v["phase"].as_str(),
            v["bodies"].as_u64(),
            v["bytes"].as_u64()
        ),
        (
            Some("compact"),
            Some((1 + SPECQ_CORE.len()) as u64),
            Some(text.len() as u64)
        )
    );
    std::fs::remove_dir_all(&root).ok();
}

// Chain: § 8 compile jobs, #1605. Risk: parallel workers each start one cargo job per CPU, thrash, and the load
// gate holds new launches; or a Claude --bg session misses the bound because its daemon's env wins.
#[test]
fn launches_carry_the_host_compile_jobs_share() {
    let p = crate::host::Policy::parse(
        serde_json::json!({"limits": {"claude": 10}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let mut h = crate::host::Host::new(Ok(p));
    h.sync(
        Path::new("/a"),
        vec![(1, 11, "claude".into()), (2, 0, "codex".into())],
    );
    h.sync(
        Path::new("/b"),
        vec![(3, 33, "claude".into()), (4, 44, "claude".into())],
    );
    assert_eq!(
        h.occupied(),
        4,
        "pending grants and children of every project"
    );
    for (cpus, occupied, jobs) in [(16, 4, 4), (8, 10, 1), (8, 3, 2), (8, 1, 8), (8, 0, 8)] {
        assert_eq!(
            compile_jobs(Some(cpus), occupied),
            Some(jobs),
            "{cpus} CPUs, {occupied} slots"
        );
    }
    assert_eq!(compile_jobs(None, 4), None, "unknown CPUs set nothing");
    let root = rules_root("jobs");
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    for rt in ["claude", "codex"] {
        crate::policy::grant(&install, rt, "full", None, "op", 1).unwrap();
    }
    let launch = |rt: &str| Launch {
        files: String::new(),
        execution: None,
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "worker".into(),
        rt: rt.into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    for jobs in [compile_jobs(Some(16), h.occupied()), None] {
        let env = Env {
            context: "",
            root: &root,
            exe: Path::new("/i/taskq"),
            install: &install,
            permission_mode: "dontAsk",
            codex: &[],
            delivery: crate::config::Delivery::AutoMerge,
            board_env: &[],
            jobs,
        };
        let want = jobs.map(|j| j.to_string());
        for rt in ["claude", "codex"] {
            let c = command(&env, &launch(rt), &access(&env, rt).unwrap()).unwrap();
            let got = c
                .get_envs()
                .find(|(k, _)| *k == "CARGO_BUILD_JOBS")
                .and_then(|(_, v)| v)
                .map(|v| v.to_string_lossy().into_owned());
            assert_eq!(got, want, "{rt} process env");
        }
        let settings: serde_json::Value = serde_json::from_str(&compact_settings(
            &root,
            &install,
            &launch("claude"),
            jobs,
            None,
        ))
        .unwrap();
        assert_eq!(
            settings["env"]["CARGO_BUILD_JOBS"]
                .as_str()
                .map(String::from),
            want,
            "Claude --bg settings env"
        );
    }
    std::fs::remove_dir_all(&root).ok();
}

// Chain: build cache (docs/testing.md#build-cache), #1604. Risk: a sandboxed child gets a wrapper whose server it
// cannot reach and its build fails, or a host without sccache or with the owner's own wrapper gets one.
#[test]
fn build_cache_only_where_the_child_reaches_it() {
    let dir = std::env::temp_dir().join(format!("taskq-build-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join(if cfg!(windows) {
        "sccache.exe"
    } else {
        "sccache"
    });
    assert_eq!(
        build_cache_in(false, dir.as_os_str()),
        None,
        "no sccache: children unchanged"
    );
    std::fs::write(&bin, "").unwrap();
    assert_eq!(build_cache_in(false, dir.as_os_str()), Some(bin.clone()));
    assert_eq!(
        build_cache_in(true, dir.as_os_str()),
        None,
        "an inherited wrapper stays the owner's"
    );
    let env = Env {
        context: "",
        root: &dir,
        exe: Path::new("taskq"),
        install: &dir,
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let seatbelt = crate::policy::Effective {
        runtime: String::new(),
        requested: String::new(),
        grant: String::new(),
        mode: "legacy".into(),
        enforcement: String::new(),
        profile: Some("(version 1)".into()),
        linux: false,
        windows: false,
        claude: None,
        codex: None,
        refused: None,
    };
    assert!(
        build_open("claude", &seatbelt, &env),
        "the seatbelt profile keeps user caches writable"
    );
    assert!(
        !build_open("codex", &seatbelt, &env),
        "Codex defaults: its own sandbox is unknown"
    );
    let ws = ["--sandbox".to_string(), "workspace-write".into()];
    assert!(!build_open("codex", &seatbelt, &Env { codex: &ws, ..env }));
    let full = crate::policy::Effective {
        codex: Some(vec!["-s".into(), "danger-full-access".into()]),
        ..seatbelt.clone()
    };
    assert!(build_open("codex", &full, &env));
    assert!(
        !build_open(
            "codex",
            &crate::policy::Effective {
                windows: true,
                ..full.clone()
            },
            &env
        ),
        "taskq Windows sandbox"
    );
    assert!(
        !build_open(
            "codex",
            &crate::policy::Effective {
                linux: true,
                ..full
            },
            &env
        ),
        "taskq Linux sandbox"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// Chain: § 8 rule delivery to a PM session, #1219. Risk: the PM hook gets other rules than a worker at the same pin,
// compaction loses mandatory bodies, or the committed hook names a local path or misses start/compact.
#[test]
fn pm_hook_gets_the_worker_start_rules() {
    let root = rules_root("pm");
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("/i/taskq"),
        install: Path::new("/i"),
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let l = Launch {
        files: String::new(),
        execution: None,
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "worker".into(),
        rt: "claude".into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    let start = turn_text(&env, &l, rule).unwrap();
    assert_eq!(
        pm_rules(&root, "start", rule).unwrap(),
        start.split("\nTaskQ task\n").next().unwrap()
    );
    assert_eq!(
        pm_rules(&root, "compact", rule).unwrap(),
        rules(&root, true, rule).unwrap()
    );
    // the committed project hook: start on startup and clear, compact on compaction, no local path or secret
    let settings: serde_json::Value =
        serde_json::from_str(include_str!("../.claude/settings.json")).unwrap();
    let hooks = settings["hooks"]["SessionStart"].as_array().unwrap();
    for (matcher, phase) in [("startup|clear", "start"), ("compact", "compact")] {
        let cmd = hooks.iter().find(|h| h["matcher"] == matcher).unwrap()["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(
            cmd.ends_with(&format!(" rules pm {phase}")) && cmd.contains("$CLAUDE_PROJECT_DIR"),
            "{cmd}"
        );
        assert!(!cmd.contains("/Users/") && !cmd.contains("token"), "{cmd}");
    }
    std::fs::remove_dir_all(&root).ok();
}

// Chain: § 8 rule delivery cache and refusals, #1156. Risk: a missing or unreadable pinned file, an unreadable doc or an
// oversized payload starts a worker without its rules; or every start refetches the pin.
#[test]
fn registered_project_contract_overrides_taskq_and_missing_contract_refuses() {
    let root = rules_root("project-contract");
    std::fs::write(
        root.join("taskq.json"),
        r#"{"repo":"group/nested/serverq"}"#,
    )
    .unwrap();
    std::fs::write(root.join("taskq.md"), "# Wrong primary contract\n").unwrap();
    std::fs::write(
        root.join("serverq.md"),
        "# ServerQ contract\nProject rule overrides shared rule.\n",
    )
    .unwrap();
    let text = rule_text(&root, "task", false, rule).unwrap();
    assert!(text.contains(&format!(
        "--- contract {} ---\n# ServerQ contract",
        root.join("serverq.md").display()
    )));
    assert!(!text.contains("Wrong primary contract"));
    for resume in [false, true] {
        let text = rule_text(&root, "task", resume, rule).unwrap();
        assert!(text.contains(&format!("contract `{}`", root.join("serverq.md").display())));
    }
    std::fs::write(
        root.join("serverq.md"),
        "# Updated ServerQ contract\nNew current rule.\n",
    )
    .unwrap();
    let resumed = rule_text(&root, "task", true, rule).unwrap();
    assert!(
        resumed.contains("New current rule.")
            && !resumed.contains("Project rule overrides shared rule.")
    );
    std::fs::remove_file(root.join("serverq.md")).unwrap();
    for resume in [false, true] {
        assert!(rule_text(&root, "task", resume, rule).is_err());
    }
    for config in [
        "{",
        r#"{"repo":"group/.."}"#,
        r#"{"repo":"group/"}"#,
        r#"{}"#,
    ] {
        std::fs::write(root.join("taskq.json"), config).unwrap();
        assert!(rule_text(&root, "task", false, rule).is_err());
    }
    std::fs::write(root.join("taskq.json"), r#"{"repo":"taskqorg/taskq"}"#).unwrap();
    assert!(
        rule_text(&root, "task", false, rule)
            .unwrap()
            .contains("Wrong primary contract")
    );
    std::fs::remove_file(root.join("taskq.md")).unwrap();
    assert!(
        rule_text(&root, "task", false, rule)
            .unwrap()
            .contains(CONTRACT)
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: § 7 rule 2 worker base -> § 8 rule delivery, #1479. Risk: a root behind origin or with a local edit hands the
// worker a contract other than the one its branch starts from.
#[test]
fn contract_comes_from_fetched_origin_main_not_the_root_tree() {
    let root = rules_root("base-commit");
    let up = root.with_file_name(format!(
        "{}-up",
        root.file_name().unwrap().to_str().unwrap()
    ));
    std::fs::remove_dir_all(&up).ok();
    let git = |dir: &Path, args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    std::fs::write(root.join("taskq.json"), r#"{"repo":"group/serverq"}"#).unwrap();
    std::fs::write(root.join("serverq.md"), "# Stale root contract\n").unwrap();
    std::fs::write(root.join("taskq.md"), "# Wrong root taskq contract\n").unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "old"]);
    std::fs::create_dir_all(&up).unwrap();
    git(&up, &["clone", "-q", root.to_str().unwrap(), "."]);
    std::fs::write(up.join("serverq.md"), "# Fresh origin contract\n").unwrap();
    git(&up, &["commit", "-q", "-am", "new"]);
    let fresh = git(&up, &["rev-parse", "HEAD"]);
    git(&root, &["remote", "add", "origin", up.to_str().unwrap()]);
    std::fs::write(root.join("serverq.md"), "# Local root edit\n").unwrap();
    for resume in [false, true] {
        let text = rule_text(&root, "task", resume, rule).unwrap();
        assert!(
            text.contains(&format!(
                "--- contract {} at origin/main {fresh} ---\n# Fresh origin contract",
                root.join("serverq.md").display()
            )),
            "{resume}"
        );
        assert!(text.contains(&format!(
            "contract `{} at origin/main {fresh}`",
            root.join("serverq.md").display()
        )));
        assert!(!text.contains("Stale root contract") && !text.contains("Local root edit"));
    }
    // #1946: `taskq rules size` checks the contract a branch proposes, before it reaches origin/main
    let tree = bundle(&root, true, true, rule).unwrap();
    assert!(tree.contains("# Local root edit") && !tree.contains("Fresh origin contract"));
    assert_eq!(rules_size_with(&root, rule).unwrap().0, tree.len());
    git(&up, &["rm", "-q", "serverq.md", "taskq.md"]);
    git(&up, &["commit", "-q", "-m", "drop"]);
    assert!(
        rule_text(&root, "task", false, rule).is_err(),
        "a registered contract absent at the base refuses"
    );
    std::fs::write(root.join("taskq.json"), r#"{"repo":"taskqorg/taskq"}"#).unwrap();
    let text = rule_text(&root, "task", false, rule).unwrap();
    assert!(
        text.contains(CONTRACT) && !text.contains("Wrong root taskq contract"),
        "absent at the base: this release's contract"
    );
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(up).unwrap();
}
#[test]
fn pinned_cache_rejects_changed_utf8_wrong_identity_and_partial_pairs() {
    assert_eq!(
        rule_cache(SPECQ_REVISION, "fixture", "abc")["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let root = rules_root("cache-integrity");
    let file = SPECQ_CORE[0];
    let path = specq_dir(&root, SPECQ_REVISION).join(file);
    let expected = pinned(&root, SPECQ_REVISION, file, &mut rule).unwrap();
    std::fs::write(&path, "# Changed valid UTF-8 rule\n").unwrap();
    assert!(
        pinned(&root, SPECQ_REVISION, file, &mut |_| Err("offline".into())).is_err(),
        "altered nonempty cache must not be accepted"
    );
    assert_eq!(
        pinned(&root, SPECQ_REVISION, file, &mut rule).unwrap(),
        expected
    );
    let metadata = path.with_extension("cache.json");
    for key in ["revision", "file", "sha256"] {
        let mut state = rule_cache(SPECQ_REVISION, file, &expected);
        state[key] = serde_json::json!("wrong");
        std::fs::write(&metadata, state.to_string()).unwrap();
        assert!(
            pinned(&root, SPECQ_REVISION, file, &mut |_| Err("offline".into())).is_err(),
            "{key}"
        );
        assert_eq!(
            pinned(&root, SPECQ_REVISION, file, &mut rule).unwrap(),
            expected
        );
    }
    for bad in [None, Some("{"), Some("")] {
        if let Some(text) = bad {
            std::fs::write(&metadata, text).unwrap();
        } else {
            std::fs::remove_file(&metadata).unwrap();
        }
        assert!(pinned(&root, SPECQ_REVISION, file, &mut |_| Err("offline".into())).is_err());
        assert_eq!(
            pinned(&root, SPECQ_REVISION, file, &mut rule).unwrap(),
            expected
        );
    }
    std::fs::write(&path, "x".repeat(RULES_CAP + 1)).unwrap();
    assert!(pinned(&root, SPECQ_REVISION, file, &mut |_| Err("offline".into())).is_err());
    assert!(
        pinned(&root, SPECQ_REVISION, file, &mut |_| Ok(
            "x".repeat(RULES_CAP + 1)
        ))
        .is_err()
    );
    assert_eq!(
        pinned(&root, SPECQ_REVISION, file, &mut rule).unwrap(),
        expected
    );
    assert_eq!(
        pinned(&root, SPECQ_REVISION, file, &mut |_| Err(
            "must use verified cache".into()
        ))
        .unwrap(),
        expected
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn pinned_cache_parallel_repairs_publish_complete_files_without_collisions() {
    let root = rules_root("cache-concurrent");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let file = SPECQ_CORE[0];
    std::thread::scope(|scope| {
        let threads = (0..8)
            .map(|_| {
                let root = &root;
                let barrier = barrier.clone();
                scope.spawn(move || {
                    pinned(root, SPECQ_REVISION, file, &mut |url| {
                        barrier.wait();
                        rule(url)
                    })
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            assert_eq!(thread.join().unwrap().unwrap(), rule(&url(file)).unwrap());
        }
    });
    assert_eq!(
        pinned(&root, SPECQ_REVISION, file, &mut |_| Err(
            "must use verified cache".into()
        ))
        .unwrap(),
        rule(&url(file)).unwrap()
    );
    assert_eq!(
        std::fs::read_dir(specq_dir(&root, SPECQ_REVISION).join("engineering"))
            .unwrap()
            .count(),
        2,
        "no staging files left"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn rules_come_from_the_pin_cache_and_refuse_when_unreadable() {
    let root = rules_root("cache");
    std::fs::write(root.join("taskq.md"), "# Project contract\nProject rule.\n").unwrap();
    let mut fetched = Vec::new();
    let text = rule_text(&root, "task", false, |u| {
        fetched.push(u.to_string());
        rule(u)
    })
    .unwrap();
    assert_eq!(
        fetched.len(),
        2 + SPECQ_CORE.len() + SPECQ_INDEX.len(),
        "source reference, tree and each pinned file once"
    );
    assert!(text.contains(&format!(
        "--- contract {} ---\n# Project contract\nProject rule.\n",
        root.join("taskq.md").display()
    )));
    for file in SPECQ_CORE.iter().chain(SPECQ_INDEX) {
        assert!(fetched.contains(&url(file)));
        assert_eq!(
            std::fs::read_to_string(specq_dir(&root, SPECQ_REVISION).join(file)).unwrap(),
            rule(&url(file)).unwrap()
        );
    }
    for resume in [false, true] {
        assert!(
            rule_text(&root, "task", resume, |u| Err(format!("refetched {u}"))).is_ok(),
            "the cache serves the pin"
        );
    }
    // a cached file that went bad is fetched again; a failed fetch refuses
    std::fs::write(
        specq_dir(&root, SPECQ_REVISION).join(SPECQ_CORE[3]),
        [0xffu8, 0xfe],
    )
    .unwrap();
    std::fs::write(specq_dir(&root, SPECQ_REVISION).join(SPECQ_INDEX[0]), "").unwrap();
    for resume in [false, true] {
        for bad in [Err("offline".to_string()), Ok(" \n".to_string())] {
            let e = rule_text(&root, "task", resume, |_| bad.clone())
                .unwrap_err()
                .to_string();
            assert!(e.contains(SPECQ_CORE[3]), "{e}");
        }
    }
    let mut again = Vec::new();
    rule_text(&root, "task", true, |u| {
        again.push(u.to_string());
        rule(u)
    })
    .unwrap();
    assert_eq!(again, [url(SPECQ_CORE[3]), url(SPECQ_INDEX[0])]);
    // an unreadable doc, contract or an over-cap payload refuses
    std::fs::write(root.join("docs/sub/bad.md"), [0xffu8]).unwrap();
    assert!(
        rule_text(&root, "task", true, rule)
            .unwrap_err()
            .to_string()
            .contains("docs/sub/bad.md")
    );
    std::fs::remove_file(root.join("docs/sub/bad.md")).unwrap();
    std::fs::write(root.join("taskq.md"), [0xffu8]).unwrap();
    assert!(rule_text(&root, "task", false, rule).is_err());
    std::fs::write(root.join("taskq.md"), "x".repeat(RULES_CAP)).unwrap();
    assert!(
        rule_text(&root, "task", false, rule)
            .unwrap_err()
            .to_string()
            .contains("cap")
    );
    assert!(
        rule_text(&root, "task", true, rule)
            .unwrap_err()
            .to_string()
            .contains("cap"),
        "resume also enforces the mandatory-body cap"
    );
    std::fs::remove_dir_all(&root).ok();
}

// Chain: § 8 rule delivery, #1946 (pilotq#169). Risk: a contract edit over the cap merges unnoticed and every worker
// start then fails until the owner trims it; the size check must fail first and the start refusal must name the file.
#[test]
fn rules_size_reports_headroom_and_the_cap_refusal_names_the_contract() {
    let root = rules_root("size");
    std::fs::write(root.join("taskq.json"), r#"{"repo":"group/pilotq"}"#).unwrap();
    let contract = "# Project contract\nProject rule.\n";
    std::fs::write(root.join("pilotq.md"), contract).unwrap();
    let (len, cap) = rules_size_with(&root, rule).unwrap();
    assert!(cap == RULES_CAP && len < cap && len > contract.len());
    let over = cap - len + 100;
    std::fs::write(
        root.join("pilotq.md"),
        format!("{contract}{}", "x".repeat(over)),
    )
    .unwrap();
    let e = rules_size_with(&root, rule).unwrap_err();
    assert_eq!(e.class(), "rules-cap");
    assert_eq!(
        e.to_string(),
        format!(
            "rules bundle {} bytes, 100 over the {cap}-byte cap\nNext: trim pilotq.md below the cap, then retry",
            cap + 100
        )
    );
    for resume in [false, true] {
        let e = rule_text(&root, "task", resume, rule).unwrap_err();
        assert!(
            e.class() == "rules-cap" && e.no_start && !e.reportable,
            "{resume}: {e}"
        );
        assert!(
            e.to_string().contains("trim pilotq.md")
                && !e.to_string().contains(&root.display().to_string()),
            "no local path (§ 7 rule 6): {e}"
        );
    }
    std::fs::remove_dir_all(&root).ok();
}

// Chain: § 8 Access, #2432 A1. Risk: the launch path reaches the shared daemon under another grant, or binds its input
// before it waits, so a held retry would submit a second binding of one launch.
#[test]
fn a_differing_daemon_starts_no_background_job() {
    let root = rules_root("daemon-gate");
    let env = Env {
        context: "",
        root: &root,
        exe: Path::new("taskq"),
        install: &root,
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let l = Launch {
        files: String::new(),
        execution: Some(42),
        task: 7,
        title: "T".into(),
        attempt: 42,
        role: "worker".into(),
        rt: "claude".into(),
        session: "s".into(),
        url: "https://b/7".into(),
        resume: None,
    };
    let access = crate::policy::Effective {
        runtime: "claude".into(),
        requested: String::new(),
        grant: String::new(),
        mode: "workspace".into(),
        enforcement: String::new(),
        profile: Some("(version 1)".into()),
        linux: false,
        windows: false,
        claude: None,
        codex: None,
        refused: None,
    };
    let log = root.join("T7-42.log");
    daemon_sandbox::fake(Some(daemon_sandbox::Fit::Differs {
        pid: 77,
        reason: "daemon runs under another Seatbelt profile",
    }));
    let refused = ClaudeTurn::start(&env, &l, &log, "brief", &access)
        .err()
        .expect("no job starts");
    daemon_sandbox::fake(None);
    assert_eq!(
        (refused.class(), refused.to_string().as_str()),
        (
            "admission",
            "live daemon pid 77: daemon runs under another Seatbelt profile"
        )
    );
    assert!(!log.exists(), "no input is bound");
    std::fs::remove_dir_all(&root).ok();
}

// R12 / #2448: an AgentQ page buffered past TaskQ's byte budget cannot conceal changed source bytes.
#[test]
fn claude_history_buffered_records_still_require_exact_source() {
    let root = std::env::temp_dir().join(format!("taskq-claude-buffer-{}", uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("transcript.jsonl");
    let rows: Vec<_> = (0..10)
        .map(|n| {
            serde_json::json!({"type":"system", "n":n,"padding":"x".repeat(1024 * 1024)})
                .to_string()
                + "\n"
        })
        .collect();
    std::fs::write(&path, rows.concat()).unwrap();
    let mut reader = ClaudeRecords::default();
    let mut seen = 0;
    assert!(
        !reader
            .read(&path, |_| {
                seen += 1;
                Ok(())
            })
            .unwrap()
    );
    assert!(seen > 0 && seen < rows.len());
    assert!(reader.pending.len() > 0);
    let mut changed = rows.clone();
    changed[seen] = changed[seen].replace(&format!("\"n\":{seen}"), "\"n\":99");
    changed.push("{\"type\":\"system\"}\n".into());
    std::fs::write(&path, changed.concat()).unwrap();
    assert!(
        reader
            .read(&path, |_| panic!("changed buffered record delivered"))
            .unwrap_err()
            .contains("changed during observation")
    );
    std::fs::remove_dir_all(root).unwrap();
}
