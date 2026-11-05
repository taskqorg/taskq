    use super::*;
    use crate::runtime::owner::{claude_key, codex_thread, trust, trust_gap, trust_step};

    fn ok(_: &Path, _: &Request) -> Launched { Launched::Job { job: "abcd1234".into(), session: None, url: None } }
    fn unknown(_: &Path, _: &Request) -> Launched { Launched::Unknown("timeout".into()) }
    fn never(_: &Path, _: &Request) -> Launched { panic!("setup launched again") }

    // R9/C4/A5: project Codex uses one immutable request; a readback cannot create a second launch.
    #[test]
    fn codex_project_request_keeps_its_explicit_runtime_and_identity() {
        let install=tmp();
        let request=open_id(&install,&install,"literal PM request","owner-2159","codex",false);
        if cfg!(any(target_os="macos",windows)) {
            let request=request.expect("explicit project Codex request is supported");
            assert_eq!(request.runtime,"codex");assert_eq!(request.root.as_deref(),install.canonicalize().unwrap().to_str());
            assert_eq!(open_id(&install,&install,"literal PM request","owner-2159","codex",true).unwrap(),request);
            assert!(open_id(&install,&install,"literal PM request","owner-2159","claude",false).is_err());
            step(&install,ok).unwrap();step(&install,never).unwrap();assert_eq!(all(&install).unwrap().0.len(),1);
        } else {assert!(request.is_err());assert!(all(&install).unwrap().0.is_empty());}
        fs::remove_dir_all(install).unwrap();
    }

    // #2240 A2: owner weights never touch a visible session that names its runtime, weight 0 included.
    #[test]
    fn owner_session_keeps_zero_weight_runtime() {
        let install=tmp();
        fs::write(install.join("host.json"),r#"{"weights":{"claude":0}}"#).unwrap();
        let r=open_id(&install,&install,"zero weight","owner-2240","claude",false).unwrap();
        assert_eq!(r.runtime,"claude");
        step(&install,ok).unwrap();
        assert_eq!(open_id(&install,&install,"zero weight","owner-2240","claude",true).unwrap().state,"launched");
        fs::remove_dir_all(install).unwrap();
    }

    // #1464 A5: a duplicate/crossed request never creates a second session, even after service launch.
    #[test]
    fn owner_request_retries_read_the_same_record_and_refuse_changed_input() {
        let install=std::env::temp_dir().join(format!("taskq-request-id-{}",crate::runtime::uuid()));
        fs::create_dir_all(&install).unwrap();
        let first=open_id(&install,&install,"literal request","owner-1464","claude",false).unwrap();
        assert_eq!(open_id(&install,&install,"literal request","owner-1464","claude",true).unwrap(),first);
        step(&install,ok).unwrap();
        let observed=open_id(&install,&install,"literal request","owner-1464","claude",true).unwrap();
        assert_eq!(observed.state,"launched");
        assert_eq!(all(&install).unwrap().0.len(),1);
        for (text,id,rt) in [("changed","owner-1464","claude"),(" literal request","owner-1464","claude"),("literal request","../escape","claude"),("literal request","owner-1464","codex")] {
            assert!(open_id(&install,&install,text,id,rt,false).is_err());
        }
        assert_eq!(all(&install).unwrap().0.len(),1);
        fs::write(file(&install,"owner-1464"),"corrupt record").unwrap();
        assert!(open_id(&install,&install,"literal request","owner-1464","claude",true).is_err());
        assert_eq!(fs::read_to_string(file(&install,"owner-1464")).unwrap(),"corrupt record");
        fs::remove_file(file(&install,"owner-1464")).unwrap();
        assert!(open_id(&install,&install,"literal request","owner-1464","claude",true).is_err(),"missing readback must never recreate a request");
        assert!(all(&install).unwrap().0.is_empty());
        fs::remove_dir_all(install).unwrap();
    }

    // #1409: a Mac launch recorded `launched` with null session and url; Remote Control publishes the link later.
    // Risk: no Open session, a link written to another job, or rereads that never stop.
    #[test]
    fn later_passes_fill_a_missing_link_within_the_window() {
        fn none(_: &Path, _: &str) -> (Option<String>, Option<String>) { (None, None) }
        fn found(_: &Path, job: &str) -> (Option<String>, Option<String>) {
            assert_eq!(job, "abcd1234");
            (Some("abcd1234-0000-0000-0000-000000000000".into()), Some("https://claude.ai/code/session_x".into()))
        }
        fn never(_: &Path, _: &str) -> (Option<String>, Option<String>) { panic!("read outside the window") }
        let t = tmp();
        let r = submit(&t, "claude", None, &[]).unwrap();
        step(&t, ok).unwrap();
        link(&t, r.created, none).unwrap();
        assert_eq!(read(&file(&t, &r.id)).unwrap().url, None);
        link(&t, r.created + 1000, found).unwrap();
        let cur = read(&file(&t, &r.id)).unwrap();
        assert_eq!((cur.session.as_deref(), cur.url.as_deref()), (Some("abcd1234-0000-0000-0000-000000000000"), Some("https://claude.ai/code/session_x")));
        assert_eq!(view(&cur)["kind"], "launched");
        link(&t, r.created + 2000, never).unwrap();
        let late = submit(&t, "claude", None, &[]).unwrap();
        step(&t, ok).unwrap();
        link(&t, late.created + LINK_WINDOW_MS, never).unwrap();
        fs::remove_dir_all(t).unwrap();
    }

    // #1391 A3: transport retry is one request; a new OK is another session; every brief retains selected scope.
    #[test]
    fn agentq_clicks_reuse_shared_setup_without_replaying_installation() {
        let i=tmp();let targets=vec![i.join("agentq").display().to_string(),i.join(".claude").display().to_string()];
        let a=agentq(&i,"click-one","claude",targets.clone()).unwrap();
        assert_eq!(agentq(&i,"click-one","claude",targets.clone()).unwrap(),a);
        assert!(a.brief.contains("Consent already names these exact displayed effects"));
        assert!(a.brief.contains("Do not kill or restart existing agent sessions"));
        assert!(a.brief.contains("Selected client: Claude Code"));
        assert_eq!(a.targets,targets);assert!(a.root.is_none());
        agentq(&i,"click-two","claude",a.targets.clone()).unwrap();
        assert_eq!(all(&i).unwrap().0.len(),2);
        assert!(agentq(&i,"click-one","claude",vec![i.display().to_string(),i.display().to_string()]).is_err());
        fs::remove_dir_all(i).unwrap();
    }

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-setup-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }

    // Risk (PM run on FRA): an Access line without its folder reads as a host refusal while projects hold grants;
    // Windows ssh reads Claude missing with no hint to use a Scheduled Task.
    #[test]
    fn onboard_lines_name_their_folder_and_the_windows_ssh_case() {
        let (install, root) = (tmp(), tmp());
        let refused = crate::policy::resolve(&Err("x".into()), &crate::policy::Host::here(&install, &root, &install), "codex", "", &[]);
        let ok = crate::policy::Effective { refused: None, ..refused.clone() };
        let r = root.display().to_string();
        // A2 (#1555): a working runtime prints nothing
        let lines = onboard_access(&r, &[refused.clone(), ok]);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with(&format!("Access {r}: ")), "{lines:?}");
        assert!(lines[1].contains("--scope project` in ") && lines[1].contains(&r), "{lines:?}");
        let setup = onboard_access("setup", &[refused]);
        assert!(setup[0].starts_with("Access setup: ") && !setup[1].contains("--scope project"), "{setup:?}");
        assert!(onboard_skipped("claude", true).contains("Scheduled Task"));
        assert!(!onboard_skipped("claude", false).contains("Scheduled Task"));
    }

    #[test]
    fn onboarding_trusts_forward_slash_keys_once_with_no_claude_running() {
        let (install, home) = (tmp(), tmp());
        fs::write(install.join("projects"), "C:\\Users\\a\\p\n/srv/q\n").unwrap();
        // the hand-made backslash key never matched on Windows; it stays as it was
        let old = r#"{"numStartups": 3, "projects": {"C:\\Users\\a\\p": {"hasTrustDialogAccepted": true}, "/srv/q": {"allowedTools": ["x"]}}}"#;
        fs::write(home.join(".claude.json"), old).unwrap();
        #[cfg(unix)]
        fs::set_permissions(home.join(".claude.json"), std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        let setup = claude_key(&dir(&install));
        assert!(!setup.contains('\\'));
        // #1555: a running Claude leaves the file; each untrusted key names the step that needs no Claude to close
        let gaps = trust(&install, &home, || Ok(vec!["42".into()])).unwrap();
        let want: Vec<String> = [setup.as_str(), "C:/Users/a/p", "/srv/q"].iter()
            .flat_map(|k| [format!("Trust: {k} not trusted; Claude runs (pid 42), so onboard leaves the file"), trust_step(k)]).collect();
        assert_eq!(gaps, want);
        assert!(trust(&install, &home, || Err("no tasklist".into())).unwrap_err().contains("unknown"));
        assert_eq!(fs::read_to_string(home.join(".claude.json")).unwrap(), old);

        let lines = trust(&install, &home, || Ok(vec![])).unwrap();
        assert!(lines.iter().any(|l| l.starts_with("Backup: ")), "{lines:?}");
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(home.join(".claude.json")).unwrap()).unwrap();
        for k in [setup.as_str(), "C:/Users/a/p", "/srv/q"] { assert_eq!(v["projects"][k]["hasTrustDialogAccepted"], true, "{k}"); }
        assert_eq!(v["projects"]["/srv/q"]["allowedTools"][0], "x");
        assert_eq!(v["numStartups"], 3);
        let backup = fs::read_dir(&home).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with(".claude.json.bak-onboard-")).unwrap();
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), old);
        // the config may hold account data: neither file opens to other users
        #[cfg(unix)]
        for p in [home.join(".claude.json"), backup.path()] { assert_eq!(std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&p).unwrap().permissions()) & 0o077, 0, "{}", p.display()); }

        // idempotent: nothing to change reads no processes and writes nothing
        // A2 (#1555): a working host prints no trust line
        assert!(trust(&install, &home, || panic!("process list read with nothing to change")).unwrap().is_empty());
    }

    // A1 / R9: a fresh registered root must be trusted without manual onboarding, preserving config data.
    #[test]
    fn registered_root_trust_repairs_both_windows_spellings() {
        let (install, home, root) = (tmp(), tmp(), tmp());
        fs::write(install.join("projects"), format!("{}\n", root.canonicalize().unwrap().display())).unwrap();
        let path = home.join(".claude.json");
        let old = r#"{"numStartups":3,"projects":{}}"#;
        fs::write(&path, old).unwrap();
        assert!(crate::runtime::owner::registered_trust_with(&install, &root, Some(&home), || Ok(vec!["42".into()])).unwrap_err().contains("service retries"));
        assert!(crate::runtime::owner::registered_trust_with(&install, &root, Some(&home), || Err("inventory unavailable".into())).unwrap_err().contains("unknown"));
        assert_eq!(fs::read_to_string(&path).unwrap(), old, "live or unknown Claude never loses config data");
        let key = claude_key(&root.canonicalize().unwrap());
        let mut keys = vec![key.clone()];
        if cfg!(windows) { keys.push(key.replace('/', "\\")); }
        let result = crate::runtime::owner::registered_trust_with(&install, &root, Some(&home), || Ok(vec![]));
        assert!(result.is_ok(), "{result:?}");
        let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for key in keys { assert_eq!(value["projects"][key]["hasTrustDialogAccepted"], true); }
        assert_eq!(value["numStartups"], 3);
        let backup = fs::read_dir(&home).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with(".claude.json.bak-onboard-")).unwrap();
        assert_eq!(fs::read_to_string(backup.path()).unwrap(), old);
        assert!(crate::runtime::owner::registered_trust_with(&install, &root, Some(&home), || Ok(vec![])).unwrap().is_empty());
        let unregistered = tmp();
        let before = fs::read(&path).unwrap();
        assert!(crate::runtime::owner::registered_trust_with(&install, &unregistered, Some(&home), || Ok(vec![])).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, "{").unwrap();
        assert!(crate::runtime::owner::registered_trust_with(&install, &root, Some(&home), || Ok(vec![])).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{", "invalid config is preserved");
        fs::remove_file(&path).unwrap();
        assert!(crate::runtime::owner::registered_trust_with(&install, &root, Some(&home), || Ok(vec![])).is_ok(), "fresh config is created");
        for p in [install, home, root, unregistered] { fs::remove_dir_all(p).unwrap(); }
    }

    // Risk (#1555, pilotq#3): `service install` registered a root Claude did not trust and said nothing.
    #[test]
    fn install_reports_an_untrusted_root_and_honours_a_trusted_parent() {
        let home = tmp();
        let root = Path::new("/srv/p/new");
        assert!(trust_gap(&home, root).is_empty(), "no Claude config: Claude is not used here");
        fs::write(home.join(".claude.json"), r#"{"projects": {"/srv/q": {"hasTrustDialogAccepted": true}}}"#).unwrap();
        assert_eq!(trust_gap(&home, root), ["Trust: /srv/p/new not trusted; Claude workers cannot start here".to_string(), trust_step("/srv/p/new")]);
        fs::write(home.join(".claude.json"), r#"{"projects": {"/srv/p": {"hasTrustDialogAccepted": true}}}"#).unwrap();
        assert!(trust_gap(&home, root).is_empty(), "a trusted parent covers the root");
        fs::write(home.join(".claude.json"), "{").unwrap();
        assert!(trust_gap(&home, root)[0].contains("unknown"), "unreadable config is unknown, never trusted");
    }

    // Risk (#1858): `service install` passes the canonical `\\?\C:\` root; its `//?/C:/` key missed Claude's `C:/` trust.
    #[cfg(windows)]
    #[test]
    fn install_trust_reads_the_plain_key_of_a_verbatim_root() {
        let home = tmp();
        fs::write(home.join(".claude.json"), r#"{"projects": {"C:/Users/a/p": {"hasTrustDialogAccepted": true}, "D:/w": {"hasTrustDialogAccepted": true}}}"#).unwrap();
        assert_eq!(claude_key(Path::new(r"\\?\C:\Users\a\p")), "C:/Users/a/p");
        assert_eq!(claude_key(Path::new(r"\\?\UNC\srv\share\p")), "//srv/share/p");
        assert!(trust_gap(&home, Path::new(r"\\?\C:\Users\a\p")).is_empty(), "a trusted verbatim root reads trusted");
        assert!(trust_gap(&home, Path::new(r"\\?\D:\w\child")).is_empty(), "a trusted parent of a verbatim root covers it");
        assert_eq!(trust_gap(&home, Path::new(r"\\?\C:\Users\a\q"))[0], "Trust: C:/Users/a/q not trusted; Claude workers cannot start here");
    }
    fn states(i: &Path) -> Vec<String> { all(i).unwrap().0.into_iter().map(|r| r.state).collect() }

    // Chain: service setup to host launch (A2). Risk: a repeat returns an old session or a request launches twice.
    #[test]
    fn each_request_launches_once() {
        let t = tmp();
        let i = t.as_path();
        let first = submit(i, "claude", None, &[]).unwrap();
        let second = submit(i, "claude", None, &[]).unwrap();
        assert_ne!(first.id, second.id, "every call writes a new request");
        assert!(submit(i, "gemini", None, &[]).is_err(), "unqualified runtime refused");
        // #1522: Codex setup is a Codex app thread, so only native macOS and Windows take it.
        let codex = submit(i, "codex", None, &[]);
        assert_eq!(codex.is_ok(), cfg!(any(target_os = "macos", windows)));
        if let Ok(c) = codex { assert_eq!((c.client.as_str(), first.client.as_str()), ("Codex", "Claude Code")); fs::remove_file(file(i, &c.id)).unwrap(); }
        assert!(first.brief.contains("full PM chat") && first.brief.contains(&first.host));
        step(i, ok).unwrap();
        assert_eq!(all(i).unwrap().0[0].id, first.id, "oldest first");
        assert_eq!(states(i), ["launched", "pending"]);
        step(i, ok).unwrap();
        assert_eq!(states(i), ["launched", "launched"]);
        // A restarted service never relaunches a launched request.
        recover(i).unwrap();
        step(i, never).unwrap();
        assert_eq!(last(i, false)["kind"], "launched");
    }

    // Chain: crash boundary between `launching` and the native answer (A2). Risk: an uncertain launch is replayed.
    #[test]
    fn uncertain_launch_is_interrupted() {
        let t = tmp();
        let i = t.as_path();
        submit(i, "claude", None, &[]).unwrap();
        step(i, unknown).unwrap();
        assert_eq!(states(i), ["interrupted"]);
        // Crash after `launching` was written: recovery reads interrupted, the next pass launches nothing.
        let mut r = all(i).unwrap().0.remove(0);
        r.state = "launching".into();
        write(i, &r).unwrap();
        recover(i).unwrap();
        assert_eq!(states(i), ["interrupted"]);
        step(i, never).unwrap();
        // The pre-#1426 slot is kept unread; another unreadable record is unknown, kept and reported.
        fs::write(dir(i).join("request.json"), r#"{"schema":1}"#).unwrap();
        step(i, never).unwrap();
        fs::write(dir(i).join("x.json"), r#"{"schema":3}"#).unwrap();
        assert!(step(i, never).is_err());
        assert_eq!(last(i, false)["kind"], "interrupted", "the newest readable request still shows");
        fs::remove_file(file(i, &r.id)).unwrap();
        assert_eq!(last(i, false)["kind"], "unknown");
    }

    // Chain: host policy and holds to launch (#1426 A5, #1427 A4). Risk: a session the owner opened (setup or AI request)
    // waits on queue limits or holds. `step` takes no host state, so Claude capacity 0, the emergency hold, Stop and restart cannot reach it.
    #[test]
    fn limits_and_holds_do_not_wait() {
        let t = tmp();
        let i = t.as_path();
        fs::write(i.join("host.json"), r#"{"limits":{"claude":0},"admission":{"max":0}}"#).unwrap();
        fs::write(i.join("hold"), "emergency").unwrap();
        fs::write(i.join("restart"), "").unwrap();
        submit(i, "claude", None, &[]).unwrap();
        let ai = open(i, i, "  Why is nothing starting?  ").unwrap();
        step(i, ok).unwrap();
        step(i, ok).unwrap();
        assert_eq!(states(i), ["launched", "launched"]);
        assert_eq!(last(i, true)["id"], ai.id.as_str(), "the AI request row shows the project request, not setup");
        assert!(ai.brief.contains("\nWhy is nothing starting?\n") && ai.root.is_some());
        assert!(open(i, i, " ").is_err(), "empty request writes nothing");
        assert_eq!(states(i).len(), 2);
    }

    // Chain: launch outcome to tray view (A4). Risk: missing runtime, login and access read as one generic failure.
    #[test]
    fn view_names_each_actionable_state() {
        let mut r = Request { schema: 2, id: "a".into(), created: 0, host: "h".into(), platform: "p".into(), client: "c".into(), runtime: "claude".into(),
            targets: vec![], brief: String::new(), state: "failed".into(), job: None, session: None, url: None, reason: None, root: None, pm:false,choice:None };
        let kind = |r: &Request| view(r)["kind"].as_str().unwrap().to_string();
        let mut kinds = vec![];
        for reason in [format!("{MISSING} x"), format!("{LOGIN} x"), format!("{ACCESS} x"), "spawn".into()] { r.reason = Some(reason); kinds.push(kind(&r)); }
        r.state = "interrupted".into(); kinds.push(kind(&r));
        assert_eq!(kinds, ["runtime", "login", "access", "failed", "interrupted"]);
        r.state = "launched".into(); r.reason = None; r.url = Some("https://claude.ai/code/session_x".into());
        assert_eq!((kind(&r).as_str(), view(&r)["url"].as_str()), ("launched", Some("https://claude.ai/code/session_x")));
        assert!(view(&r)["next"].as_str().unwrap().contains("Open the session"));
        // #1522: a Codex request names Codex steps, never Claude ones.
        r.runtime = "codex".into(); r.state = "failed".into(); r.url = None;
        for (reason, step) in [(MISSING, "Install Codex"), (LOGIN, "`codex login`")] {
            r.reason = Some(format!("{reason} x"));
            assert!(view(&r)["next"].as_str().unwrap().contains(step));
        }
    }

    // #1522 A4: the Codex handshake answer to the record. Risk: a login prompt reads as a generic failure, a thread whose
    // brief was not queued reads as launched or is retried, or an unsafe id reaches the app URL.
    #[test]
    fn codex_outcomes_stay_distinct() {
        let started = r#"{"type":"thread.started","thread_id":"019a-b2"}"#;
        let queued = |t: &str| { assert_eq!(t, "019a-b2"); Ok(()) };
        let refused = |_: &str| -> Res<()> { Err("codex queue failed for thread 019a-b2".into()) };
        let never = |_: &str| -> Res<()> { panic!("no thread to submit") };
        let Launched::Job { job, session, url } = codex_thread("codex-app", started, "", Some(0), queued) else { panic!("thread not launched") };
        assert_eq!((job.as_str(), session.as_deref(), url.as_deref()), ("019a-b2", Some("019a-b2"), Some("codex://threads/019a-b2")));
        assert!(matches!(codex_thread("codex-app", "", "Error: 401 Unauthorized", Some(1), never), Launched::NoStart(r) if r.starts_with(LOGIN)));
        assert!(matches!(codex_thread("codex-app", "", "model overloaded", Some(1), never), Launched::NoStart(r) if !r.starts_with(LOGIN)));
        assert!(matches!(codex_thread("codex-app", started, "", Some(0), refused), Launched::Unknown(r) if r.contains("019a-b2")));
        assert!(matches!(codex_thread("codex-app", r#"{"type":"thread.started","thread_id":"x&calc"}"#, "", Some(0), never), Launched::Unknown(_)));
    }

    // #2324: the login cause comes from the AgentQ adapter for both runtimes. Risk: taskq matches runtime text itself,
    // so a login prompt reads as a generic failure or an untrusted workspace as a sign-in.
    #[test]
    fn launch_cause_comes_from_the_adapter() {
        use crate::runtime::launch_cause as refusal;
        assert_eq!(refusal("claude", "Not logged in · Please run /login"), Some("login-required"));
        assert_eq!(refusal("claude", "Error: Workspace not trusted"), Some("workspace-untrusted"));
        assert_eq!(refusal("codex", "Error: 401 Unauthorized"), Some("login-required"));
        assert_eq!(refusal("codex-app", "please log in again"), Some("login-required"));
        assert_eq!(refusal("codex", "model overloaded"), None);
    }
