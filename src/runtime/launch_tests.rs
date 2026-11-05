use super::*;

// §8/R9/R12: trait launch and resume retain literal input, host flags, daemon env and native scope.
#[test]
fn trait_launch_preserves_input_policy_environment_and_exit() {
    let root = std::env::temp_dir().join(format!("taskq-launch-{}", uuid()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let python = which("python");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-launch.py");
    let args = ["-s", "danger-full-access", "-m", "invented-model", "-c", "model_reasoning_effort=high"].map(String::from);
    let vars = vec![("AGENTQ_OPTIMIZATION".into(), "off".into()), ("INVENTED_BOARD_SECRET".into(), "invented-secret".into())];
    let env = Env {context:"",root:&root,exe:Path::new("taskq"),install:&root,permission_mode:"dontAsk",codex:&args,delivery:crate::config::Delivery::AutoMerge,board_env:&vars,jobs:Some(2)};
    let input = format!("literal \"quote\", $HOME, `command`\n{}EOF-CANARY", "private-input-".repeat(12000));
    for rt in ["codex", "claude", "antigravity"] {
        crate::policy::grant(&root, rt, "full", None, "invented operator", 1).unwrap();
        let access = access(&env, rt).unwrap();
        for resume in [false, true] {
            let l = Launch {files:String::new(),execution:None,task:2427,title:"Invented fixture".into(),attempt:42,role:"worker".into(),rt:rt.into(),session:"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into(),url:String::new(),resume:resume.then(||"continue".into())};
            let record = root.join(format!("{rt}-{resume}.json"));
            let log = root.join(format!("{rt}-{resume}.log"));
            let mut req = request(&env, &l, &log, &input, &access).unwrap();
            req.command = vec![python.clone().into(), fixture.clone().into()];
            req.env.extend([("TASKQ_FIXTURE_RT".into(), rt.into()), ("TASKQ_FIXTURE_RECORD".into(), record.clone().into()), ("PYTHONIOENCODING".into(), "utf-8".into()), ("TASKQ_FIXTURE_LINGER".into(), "1".into())]);
            assert!(!format!("{:?}", req.host_args).contains("invented-secret"));
            let mut child = native(rt, resume.then_some(l.session.as_str()), &mut req).unwrap();
            let identity = child.native_identity().unwrap();
            let until = std::time::Instant::now()+std::time::Duration::from_secs(20);
            loop {
                if let Some(code) = child.exited().unwrap() {
                    assert_ne!(code, 0, "a completed turn stops the deliberately lingering client");
                    assert_ne!(coreq::process::identity(identity.pid).unwrap(), Some(identity));
                    assert_eq!(classify(rt,&std::fs::read_to_string(&log).unwrap()).unwrap().outcome,"ok");
                    break;
                }
                if std::time::Instant::now() >= until { let _=child.kill(); panic!("{rt} resume={resume}: no exit"); }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let row:serde_json::Value=serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
            if rt=="antigravity" { let wire:serde_json::Value=serde_json::from_str(row["input"].as_str().unwrap()).unwrap(); assert_eq!(wire["message"]["content"],input); }
            else { assert_eq!(row["input"],input); }
            assert_eq!(row["env"]["TASKQ_ATTEMPT"],"42");
            assert_eq!(row["env"]["CARGO_BUILD_JOBS"],"2");
            assert_eq!(row["env"]["INVENTED_BOARD_SECRET"],"invented-secret");
            assert_eq!(row["env"]["AGENTQ_OPTIMIZATION"],"off");
            assert_eq!(row["env"]["AGENTQ_LAUNCHER"],"taskq");
            let argv:Vec<_>=row["args"].as_array().unwrap().iter().map(|v|v.as_str().unwrap()).collect();
            if rt=="codex" { for arg in &args {assert!(argv.contains(&arg.as_str()),"host flag {arg}");} }
            if rt=="claude" {
                let settings:serde_json::Value=serde_json::from_str(argv[argv.iter().position(|s|*s=="--settings").unwrap()+1]).unwrap();
                assert_eq!(settings["env"]["TASKQ_ATTEMPT"],"42");
                assert_eq!(settings["env"]["CARGO_BUILD_JOBS"],"2");
                assert!(settings["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap().contains("rules compact"));
                assert!(!settings.to_string().contains("invented-secret"));
            }
            assert_eq!(native_session(rt,&log).as_deref(),Some(l.session.as_str()));
            #[cfg(windows)] assert!(child.job().unwrap().starts_with("job-kill:"));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

// §8/R11: a background resume carries no new flags; raw stderr/exit stays available to TaskQ's refusal guard.
#[test]
fn background_trait_retains_launcher_output_and_saved_session() {
    let root=std::env::temp_dir().join(format!("taskq-background-launch-{}",uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let session="aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let home=root.join("home");
    let plain=root.canonicalize().unwrap().to_string_lossy().strip_prefix(r"\\?\").unwrap_or(&root.to_string_lossy()).to_owned();
    let project:String=plain.chars().map(|c|if c.is_ascii_alphanumeric(){c}else{'-'}).collect();
    let transcript=home.join("projects").join(project).join(format!("{session}.jsonl"));
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript,"").unwrap();
    let adapter=agent("claude").unwrap();
    for resume in [false,true] {
        let record=root.join(format!("{resume}.json"));
        let mut request=Request::new(&root,root.join("launch.log"),"invented protected input reference","taskq");
        request.command=vec![which("python").into(),Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-launch.py").into()];
        request.mode=Mode::Background;request.home=Some(home.clone());
        request.policy.approval=Approval::Deny;request.title=Some("invented title".into());
        request.settings_env=vec![("TASKQ_ATTEMPT".into(),"42".into()),("CARGO_BUILD_JOBS".into(),"2".into())];
        request.env=vec![("TASKQ_FIXTURE_RT".into(),"claude".into()),("TASKQ_FIXTURE_RECORD".into(),record.clone().into()),("PYTHONIOENCODING".into(),"utf-8".into())];
        let mut run=submit("claude",resume.then_some(session),&request).unwrap();
        let output=adapter.launcher_output(&mut run).unwrap().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stderr).unwrap().trim(),"invented launcher diagnostic");
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(),"backgrounded · aaaaaaaa");
        assert!(adapter.launcher_output(&mut run).is_err(),"launcher receipt is handed over once");
        let row:serde_json::Value=serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
        assert_eq!(row["input"],request.input);
        if resume {assert_eq!(row["args"],serde_json::json!(["--bg","--resume",session]));}
        else {assert!(row["args"].as_array().unwrap().iter().any(|arg|arg=="--settings"));}
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
            let _=self.0.kill();let until=std::time::Instant::now()+std::time::Duration::from_secs(3);
            while !self.0.exited().is_ok_and(|exit|exit.is_some()) && std::time::Instant::now()<until {std::thread::sleep(std::time::Duration::from_millis(10));}
        }
    }
    let root=std::env::temp_dir().join(format!("taskq-trait-scope-{}",uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let tree=root.join("descendant");
    let request=|name:&str,tree:Option<&Path>| {
        let mut request=Request::new(&root,root.join(format!("{name}.log")),"invented input","taskq");
        request.command=vec![which("python").into(),Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-launch.py").into()];
        request.env=vec![("TASKQ_FIXTURE_RT".into(),"codex".into()),("TASKQ_FIXTURE_RECORD".into(),root.join(format!("{name}.json")).into()),("TASKQ_FIXTURE_LINGER".into(),"1".into())];
        if let Some(tree)=tree {request.env.push(("TASKQ_FIXTURE_TREE".into(),tree.into()));}
        request
    };
    let mut owned_child=Owned(native("codex",None,&mut request("tree",Some(&tree))).unwrap());
    let mut owned_sentinel=Owned(native("codex",None,&mut request("sentinel",None)).unwrap());
    let child=&mut owned_child.0;let sentinel=&mut owned_sentinel.0;
    let until=std::time::Instant::now()+std::time::Duration::from_secs(20);
    while !tree.exists() {assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
    let pid=std::fs::read_to_string(&tree).unwrap().parse().unwrap();
    let descendant=coreq::process::identity(pid).unwrap().unwrap();
    let launcher=child.native_identity().unwrap();
    std::fs::write(tree.with_extension("exit"),"").unwrap();
    while coreq::process::identity(launcher.pid).unwrap().as_ref()==Some(&launcher) {assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
    assert_eq!(child.exited(),Ok(None));
    assert_eq!(coreq::process::identity(pid).unwrap(),Some(descendant.clone()));
    child.kill().unwrap();
    while child.exited().unwrap().is_none() {assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
    assert_ne!(coreq::process::identity(pid).unwrap(),Some(descendant));
    assert_eq!(child.exited(),Ok(Some(-1)),"cancelled scope preserves its terminal verdict");
    assert_eq!(coreq::process::identity(sentinel.id()).unwrap(),Some(sentinel.native_identity().unwrap()));
    sentinel.kill().unwrap();
    while sentinel.exited().unwrap().is_none() {assert!(std::time::Instant::now()<until);std::thread::sleep(std::time::Duration::from_millis(10));}
    drop(owned_child);drop(owned_sentinel);std::fs::remove_dir_all(root).unwrap();
}

// A7/R9/R12: paid native start/resume and transcript evidence, under existing operator authority only.
#[test]
#[ignore = "paid native qualification; TASKQ_NATIVE_RUNTIME and TASKQ_NATIVE_INSTALL require owner approval"]
fn native_trait_start_resume_and_transcript() {
    let rt=std::env::var("TASKQ_NATIVE_RUNTIME").expect("TASKQ_NATIVE_RUNTIME");
    assert!(RuntimeId::parse(&rt).is_some() && of(&rt).has(Cap::Cli));
    let install=PathBuf::from(std::env::var_os("TASKQ_NATIVE_INSTALL").expect("TASKQ_NATIVE_INSTALL"));
    let host=crate::host::Policy::read_for_hostname(&install,crate::config::hostname).unwrap();
    assert!(host.cap.limits.get(&rt).is_none_or(|limit|*limit>0),"disabled runtime is not probe authority");
    let root=Path::new(env!("CARGO_MANIFEST_DIR"));
    let exe=std::env::current_exe().unwrap();
    let authority=Env{context:"",root,exe:&exe,install:&install,permission_mode:&host.permission_mode,codex:&host.codex,delivery:crate::config::Delivery::AutoMerge,board_env:&[],jobs:None};
    let access=access(&authority,&rt).unwrap();
    let evidence=std::env::temp_dir().join(format!("taskq-native-qualification-2427-{}",uuid()));
    let fixture=evidence.join("install");std::fs::create_dir_all(&fixture).unwrap();
    let env=Env{install:&fixture,..authority};
    let log=evidence.join("native.log");File::create(&log).unwrap();
    let mut session=String::from("-");let mut rows=vec![];
    for resume in [false,true] {
        let canary=format!("TaskQ-Qualification-2427-{}",uuid());
        let text=format!("This is an approved non-task TaskQ Qualification probe. Reply with exactly {canary}. Do no work, edit no files, run no commands and make no taskq or board writes. Read this turn's input file if the launcher refers to it; use no other tools.");
        let l=Launch{files:String::new(),execution:None,task:0,title:"TaskQ Qualification 2427".into(),attempt:0,role:"operator".into(),rt:rt.clone(),session:session.clone(),url:String::new(),resume:resume.then(||text.clone())};
        let pos=std::fs::metadata(&log).unwrap().len();
        let mut proc:Box<dyn Proc>=if of(&rt).has(Cap::Background) {
            Box::new(ClaudeTurn::start(&env,&l,&log,&text,&access).unwrap())
        } else {
            let child=start(&env,&l,&log,&text,&access).unwrap();let native=child.native_identity().ok();
            Box::new(CliTurn{proc:child,log:log.clone(),pos,start:pos,stopping:false,accepted:false,read_error:None,session:of(&rt).has(Cap::StreamInput).then(||l.session.clone()),native,recovery:None,rt:rt.clone()})
        };
        let until=std::time::Instant::now()+std::time::Duration::from_secs(180);
        let exit=loop {
            match proc.exited() {
                Ok(Some(code))=>break code,
                Ok(None)=>{},
                Err(reason)=>eprintln!("qualification observation pending: {reason}"),
            }
            if std::time::Instant::now()>=until {let cleanup=stop(proc.as_mut());panic!("native qualification timed out; cleanup={cleanup:?}; evidence={}",evidence.display());}
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        assert!(proc.accepted().unwrap(),"native input must be accepted independently of exit");
        let id=native_session(&rt,&log).expect("native session receipt");
        if resume {assert_eq!(id,session,"resume cannot create a replacement session");}
        session=id;
        let native=owner::transcript(&rt,&session).unwrap();
        assert!(std::fs::read_to_string(&native).unwrap().contains(&canary),"native transcript must contain this turn's fresh canary");
        let page=crate::dialogue::worker_transcript(&format!("{rt}:{session}"),&log,None,None).unwrap();
        assert!(page["items"].as_array().unwrap().iter().any(|item|item["role"]=="assistant" && item["text"].as_str().is_some_and(|text|text.contains(&canary))),"TaskQ transcript must show the same native reply");
        rows.push(serde_json::json!({"resume":resume,"session":session,"accepted":true,"exit":exit,"canary":canary,"access":access.mode}));
        drop(proc);
    }
    let manifest=serde_json::json!({"runtime":rt,"os":std::env::consts::OS,"source":env!("CARGO_MANIFEST_DIR"),"turns":rows,"scope":"native start/resume and transcript; no live board or installed host service"});
    std::fs::write(evidence.join("manifest.json"),serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    println!("{}\nevidence={}",manifest,evidence.display());
}
