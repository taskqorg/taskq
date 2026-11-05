    use super::*;

    // Chain: host stops a sandboxed worker -> SIGKILL to the private parent -> launcher ends; exit status passes through (#2071).
    // Risk: the parent hides the launcher, so a stopped worker keeps running, or spawn blocks on the parent's descriptors.
    #[test]
    fn private_parent_passes_status_and_stop() {
        use std::os::unix::process::ExitStatusExt;
        let status = |script: &str| { let mut c = Command::new("/bin/sh"); c.args(["-c", script]); own(&mut c); c.status().unwrap() };
        assert_eq!(status("exit 7").code(), Some(7));
        assert_eq!(status("kill -TERM $$").signal(), Some(libc::SIGTERM));
        let mut missing = Command::new("/nonexistent/taskq-sandbox"); own(&mut missing);
        assert!(missing.spawn().is_err(), "a launch failure stops the attempt");
        let file = std::env::temp_dir().join(format!("taskq-private-parent-{}", std::process::id()));
        let mut c = Command::new("/bin/sh"); c.args(["-c", "echo $$ > \"$0\"; exec sleep 30"]).arg(&file); own(&mut c);
        let start = std::time::Instant::now();
        let mut parent = c.spawn().unwrap();
        let launcher = loop {
            if let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse::<i32>().ok()) { break pid; }
            assert!(start.elapsed().as_secs() < 5, "launcher never started"); std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_ne!(launcher, parent.id() as i32, "the launcher runs below a private parent");
        parent.kill().unwrap(); parent.wait().unwrap();
        while std::fs::read_to_string(format!("/proc/{launcher}/stat")).is_ok_and(|s| !s.contains(") Z ")) {
            assert!(start.elapsed().as_secs() < 10, "stopping the parent left the launcher running"); std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn codex_login_state_stays_writable() {
        let ch = Path::new("/home/me/.codex");
        assert!(!read_only_entry(&ch.join("auth.json")), "a read-only auth.json burns the rotated refresh token (#1424)");
        for n in ["config.toml", "hooks.json", "AGENTS.md", "plugins", "skills", "packages"] { assert!(read_only_entry(&ch.join(n)), "{n}"); }
    }

    // #2153 A2: Linux sandbox readiness checks the active Codex account home's config.toml and refuses symlinks.
    #[test]
    fn prerequisites_check_active_account_home() {
        let dir = std::env::temp_dir().join(format!("taskq-2153-linux-{}", crate::runtime::uuid()));
        let root = dir.join("root");
        let install = dir.join("install");
        let base = dir.join("codex-home");
        let work = base.join(crate::runtime::accounts::HOMES).join("work");
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(base.join("config.toml"), "").unwrap();
        assert!(prerequisites(&root, &crate::runtime::accounts::active_in(&install, "codex", &base)).is_ok());

        std::fs::write(install.join(crate::runtime::accounts::FILE), r#"{"codex":[{"label":"default","enabled":false},{"label":"work","enabled":true}]}"#).unwrap();
        let active = crate::runtime::accounts::active_in(&install, "codex", &base);
        assert_eq!(active, work);
        assert!(prerequisites(&root, &active).unwrap_err().contains("linux sandbox prerequisite"));
        std::os::unix::fs::symlink(base.join("config.toml"), work.join("config.toml")).unwrap();
        assert!(prerequisites(&root, &active).unwrap_err().contains("linux sandbox refuses symlink"));
        std::fs::remove_file(work.join("config.toml")).unwrap();
        std::fs::write(work.join("config.toml"), "").unwrap();
        assert!(prerequisites(&root, &active).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    // Chain: workspace worker git fetch/commit/push (#1490). Risk: the git common dir stays read-only for linked worktrees.
    #[test]
    fn workspace_git_common_dir_stays_writable() {
        let root = std::env::temp_dir().join(format!("taskq-1490-{}", std::process::id()));
        let git = root.join(".git");
        std::fs::create_dir_all(git.join("hooks")).unwrap();
        let (ch, home) = (Path::new("/home/me/.codex"), Path::new("/home/me"));
        assert_eq!(git_dir("workspace", &root), None, "unmaskable config: git dir stays read-only");
        std::fs::write(git.join("config"), "").unwrap();
        assert_eq!(git_read_only(&git), [git.join("hooks"), git.join("config")], "host-side git must not run sandbox-written hooks or config");
        let g = git_dir("workspace", &root);
        assert_eq!(git_dir("restricted", &root), None, "restricted keeps only .taskq");
        let ws = write_mounts("workspace", &root, g.as_deref(), ch, home);
        assert_eq!(ws[..3], [root.clone(), git.clone(), ch.to_path_buf()]);
        let req: Vec<String> = ["exec", "-s", "workspace-write", "-c", "sandbox_workspace_write.writable_roots=[]"].map(String::from).into();
        let roots = format!("sandbox_workspace_write.writable_roots=[{:?}]", git.to_str().unwrap());
        assert_eq!(codex_args(g.as_deref(), &req)[4], roots, "native Codex keeps <cwd>/.git read-only without it");
        assert_eq!(codex_args(None, &req), req);
        std::fs::remove_file(git.join("config")).unwrap();
        std::os::unix::fs::symlink(home, git.join("config")).unwrap();
        assert_eq!(git_dir("workspace", &root), None, "a planted symlink never redirects a mount");
        std::fs::remove_dir_all(&root).ok();
    }

    // Paid chain: role start/resume, denied writes/network and nested-launcher bypass; not per-commit CI.
    #[test]
    #[ignore = "requires installed root-owned Linux helper and an explicit disposable project grant"]
    fn live_linux_boundary_matrix() {
        use crate::runtime::{Env, Launch};
        let root = PathBuf::from(std::env::var_os("TASKQ_QUALIFY_ROOT").expect("disposable project"));
        let install = PathBuf::from(std::env::var_os("TASKQ_INSTALL").expect("disposable installation"));
        let exe = PathBuf::from(std::env::var_os("TASKQ_QUALIFY_EXE").expect("candidate CLI"));
        let sandbox = std::env::var("TASKQ_QUALIFY_SANDBOX").unwrap_or("workspace-write".into());
        assert!(["workspace-write", "read-only"].contains(&sandbox.as_str()));
        let network = std::env::var("TASKQ_QUALIFY_NETWORK").unwrap_or("true".into());
        assert!(["true", "false"].contains(&network.as_str()));
        let codex = vec!["-s".into(), sandbox.clone(), "-c".into(), format!("sandbox_workspace_write.network_access={network}")];
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let receipts = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let observed = receipts.clone();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = stop.clone();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok((mut socket, _)) = listener.accept() {
                    socket.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                    let mut bytes = [0; 256];
                    if let Ok(n) = socket.read(&mut bytes) {
                        observed.lock().unwrap().push(String::from_utf8_lossy(&bytes[..n]).into_owned());
                        socket.write_all(b"OK").ok();
                    }
                } else { std::thread::sleep(std::time::Duration::from_millis(10)); }
            }
        });
        let env = Env { context: "", root: &root, install: &install, exe: &exe, codex: &codex, permission_mode: "dontAsk", publish: "pr", board_env: &[], jobs: None };
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let canaries = [root.parent().unwrap().join("outside.txt"), install.join("canary.txt"), root.join(".codex/canary.toml")];
        for p in &canaries { std::fs::write(p, "UNCHANGED\n").unwrap(); }
        std::fs::remove_file(root.join("outside-link")).ok();
        std::os::unix::fs::symlink(&canaries[0], root.join("outside-link")).unwrap();
        std::fs::write(root.join("fixture-codex-path"), crate::runtime::which("codex").to_str().unwrap()).unwrap();
        std::fs::write(root.join("fixture-install-path"), install.to_str().unwrap()).unwrap();
        std::fs::write(root.join("check.py"), include_str!("../tests/fixtures/linux-boundary.py")).unwrap();
        std::fs::write(root.join("taskq.md"), "# Owner-authorized isolated runtime qualification\nFor both worker and reviewer, run exactly `python3 check.py`, inspect its JSON stdout (proof.json is also written when workspace writes are allowed) and compare baseline, expected and candidate active/repeated/empty cases. Identify regressions even when the command exits 0. Report denied canaries and evidence limits. This fixture has no board task: do not run inbox, create worktrees, commit, publish or request elevation. These owner instructions override generic board workflow. Every continuation repeats the check after stage changes.\n").unwrap();
        std::fs::write(root.join("AGENTS.md"), "Read taskq.md; disposable qualification only.\n").unwrap();
        let attempt = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() * 2;
        for (i, role) in ["worker", "reviewer"].iter().enumerate() {
            let mut l = Launch { files: String::new(), execution: None, task: 1, title: "Owner-authorized isolated boundary qualification. Follow fixture taskq.md; no board or publication.".into(), attempt: attempt + i as u64, role: role.to_string(), rt: "codex".into(), session: crate::runtime::uuid(), url: "https://github.com/taskqorg/taskq".into(), resume: None };
            for turn in 0..2 {
                std::fs::write(root.join("stage"), turn.to_string()).unwrap();
                std::fs::remove_file(root.join("proof.json")).ok();
                let token = crate::runtime::uuid();
                std::fs::write(root.join("network-target.json"), serde_json::json!({"port":port,"token":token}).to_string()).unwrap();
                if turn == 1 { l.resume = Some("Owner: repeat the exact fixture check against the changed stage; retain this session. No other actions.".into()); }
                let mut child = crate::runtime::spawn(&env, &l).unwrap_or_else(|e| panic!("{e}"));
                let began = std::time::Instant::now();
                let code = loop {
                    if let Some(code) = child.exited().unwrap() { break code; }
                    if began.elapsed().as_secs() > 180 { child.kill(); panic!("owned qualification deadline"); }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                };
                let log = std::fs::read_to_string(crate::runtime::log_path(&root, &l)).unwrap();
                assert_eq!(code, 0, "{log}");
                let ids: Vec<String> = log.lines().filter_map(|s| serde_json::from_str::<serde_json::Value>(s).ok()).filter(|v| v["type"] == "thread.started").filter_map(|v| v["thread_id"].as_str().map(String::from)).collect();
                if turn == 0 { l.session = ids.first().expect("native session").clone(); }
                assert!(ids.iter().all(|id| *id == l.session), "same session");
                let proof: serde_json::Value = log.lines().filter_map(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .filter(|v| v["type"] == "item.completed" && v["item"]["type"] == "command_execution")
                    .filter_map(|v| serde_json::from_str::<serde_json::Value>(v["item"]["aggregated_output"].as_str()?).ok())
                    .filter(|v| v["stage"] == turn.to_string()).last().expect("real command proof");
                assert_eq!(proof["stage"], turn.to_string());
                assert_eq!(proof["allowed"], sandbox == "workspace-write");
                for p in &canaries { assert_eq!(std::fs::read_to_string(p).unwrap(), "UNCHANGED\n", "{}", p.display()); }
                for key in ["outside", "policy", "config", "symlink", "child"] { assert_eq!(proof[key], "denied", "{key}: {proof}"); }
                let permitted = sandbox == "workspace-write" && network == "true";
                for family in ["ipv4", "ipv6"] {
                    assert_eq!(proof[family]["result"], if permitted { "connected" } else { "denied" }, "{proof}");
                    assert_eq!(receipts.lock().unwrap().contains(&format!("{token}-{family}")), permitted, "independent server receipt");
                    if !permitted { assert!(matches!(proof[family]["errno"].as_i64(), Some(1 | 13)), "OS denial, not timeout: {proof}"); }
                }
                assert_eq!(proof["nested"]["escaped"], false, "nested launcher cannot regain denied access: {proof}");
                assert!(proof["nested"]["exit"].as_i64().is_some_and(|c| c != 0), "nested refusal must be observed: {proof}");
                assert!(proof["nested"]["error"].as_str().unwrap().contains("linux sandbox requires root-owned, non-writable, non-setid /usr/local/libexec/taskq-sandbox"), "known guard refusal, not a missing dependency: {proof}");
                println!("QUALIFIED Linux network={network} {sandbox} {role} turn={turn} session={} proof={proof}", l.session);
            }
            let store = install.join("worker-policy.json");
            let saved = std::fs::read(&store).unwrap();
            crate::policy::grant(&install, "codex", "none", Some(&root), "qualification revocation", 2).unwrap();
            let refused = crate::runtime::spawn(&env, &l).err();
            std::fs::write(&store, saved).unwrap();
            assert!(refused.is_some(), "revoked continuation must not launch");
            println!("QUALIFIED {role} revoked continuation refused before spawn");
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        server.join().unwrap();
    }
