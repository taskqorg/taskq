    use super::*;

    /// Lines of `text` before its `#[cfg(test)] mod … {`: test processes may start children their own way.
    fn product(text: &str) -> Vec<&str> {
        let lines: Vec<&str> = text.lines().collect();
        let end = (0..lines.len()).find(|&i| lines[i].trim_start().starts_with("#[cfg(test)]") && {
            let next = lines[i + 1..].iter().map(|l| l.trim()).find(|l| !l.starts_with("#[")).unwrap_or("");
            next.trim_start_matches("pub(crate) ").starts_with("mod ") && next.ends_with('{')
        }).unwrap_or(lines.len());
        lines[..end].to_vec()
    }

    // Defect #2049 A2: tasklist, git, the launcher and codex started without CREATE_NO_WINDOW, so the windowless
    // service flashed a console window per start. Every product start goes through `command` (or the pass-through's
    // `console_command`); this enumerates the spawn sites and names any raw `Command::new` or a flag that drops the bit.
    #[test]
    fn spawn_sites() {
        let (mut sites, mut raw) = (vec![], vec![]);
        let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() { dirs.push(path); continue; }
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with(".rs") || name.ends_with("_tests.rs") { continue; }
                let text = fs::read_to_string(&path).unwrap();
                let file = path.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display().to_string();
                for (i, line) in product(&text).iter().enumerate() {
                    let at = format!("{file}:{}", i + 1);
                    if line.contains("platform::command(") || line.contains("platform::console_command(") { sites.push(at.clone()); }
                    let helper = name == "platform.rs" && line.trim() == "let mut c = std::process::Command::new(program);";
                    if line.contains("Command::new(") && !helper { raw.push(format!("{at}: {}", line.trim())); }
                    if let Some(flags) = line.split("creation_flags(0x").nth(1) {
                        let hex: String = flags.chars().take_while(|c| c.is_ascii_hexdigit() || *c == '_').filter(|c| *c != '_').collect();
                        if u32::from_str_radix(&hex, 16).map_or(true, |f| f & 0x0800_0000 == 0) { raw.push(format!("{at}: drops CREATE_NO_WINDOW: {}", line.trim())); }
                    }
                }
            }
        }
        assert!(raw.is_empty(), "start these through platform::command:\n{}", raw.join("\n"));
        // the scan found the starts: a moved src or a misread test boundary would pass with nothing checked
        for site in ["src/runtime/owner.rs", "src/board/github.rs", "src/service_run.rs", "src/windows_sandbox.rs", "src/runtime.rs"] {
            assert!(sites.iter().any(|s| s.replace('\\', "/").starts_with(site)), "no spawn site in {site}: {sites:?}");
        }
        assert!(sites.len() > 50, "{} spawn sites: {sites:?}", sites.len());
    }

    // Defect #1803 A4: a gui probe that captured without an allowed preflight raised a Screen Recording prompt.
    #[test]
    fn gui_capture_runs_only_after_an_allowed_preflight() {
        let mut ran = false;
        assert_eq!(gated(false, || { ran = true; Ok(()) }), Err(NOT_ALLOWED.to_string()));
        assert!(!ran, "capture ran after a refused preflight");
        assert_eq!(gated(true, || Err("screencapture wrote no PNG".into())), Err("screencapture wrote no PNG".to_string()));
    }

    // Defect #1845 A1: only Screen Recording was probed; a stale Accessibility or App Data grant surfaced as a worker
    // ask. Risk: a state misread lets a gui task run into the denial or blocks it on a healthy grant.
    #[test]
    fn grants_read_allowed_missing_stale_and_unknown() {
        let bin = Path::new("/i/taskq");
        let line = |s: &str| format!("tccd: Failed to match existing code requirement for subject /i/taskq and service {s}\n");
        let st = |t, l: Option<&str>| { let g = grant_states(t, l, bin); (g["accessibility"].clone(), g["app_data"].clone()) };
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(st(true, Some("")), s("allowed", "unknown"));
        assert_eq!(st(false, None), s("missing", "unknown"), "unreadable log: denial is missing, App Data unknown");
        assert_eq!(st(false, Some(&line("kTCCServiceAccessibility"))), s("stale", "unknown"));
        assert_eq!(st(true, Some(&line("kTCCServiceAccessibility"))), s("allowed", "unknown"), "a trusted process is never stale");
        assert_eq!(st(true, Some(&line("kTCCServiceSystemPolicyAppData"))), s("allowed", "stale"));
        assert_eq!(st(true, Some(&line("kTCCServiceSystemPolicyAppData").replace("/i/taskq", "/other/taskq"))), s("allowed", "unknown"), "another binary's line");
    }

    // Defect #1445: service and status.json were written in place without a flush; a power cut left zeros.
    #[test]
    fn synced_write_replaces_whole_record_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("taskq-synced-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("service");
        write_synced(&p, "old").unwrap();
        write_synced(&p, "new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "temp file removed");
        let me = coreq::process::identity(std::process::id()).unwrap().unwrap();
        assert!(!started_before_boot(&me.started).unwrap(), "this process started in this boot");
        for invalid in ["", "0", "0-0", "1-invalid", "invented-previous-incarnation"] {assert!(started_before_boot(invalid).is_err(),"malformed start cannot prove a later boot");}
        fs::remove_dir_all(dir).unwrap();
    }

    // #1381: project taskq's log is the host unit's stderr file; a line written to both landed twice.
    #[test]
    fn service_line_lands_once_when_project_log_is_stderr_target() {
        let dir = std::env::temp_dir().join(format!("taskq-log-once-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let (own, other) = (dir.join("taskq.log"), dir.join("demo.log"));
        let mut stderr = fs::OpenOptions::new().create(true).append(true).open(&own).unwrap();
        emit("a", Some(&own), &mut stderr, Some(&own));
        emit("b", Some(&own), &mut stderr, Some(&own));
        assert_eq!(fs::read_to_string(&own).unwrap(), "a\nb\n");
        emit("c", Some(&other), &mut stderr, Some(&own));
        assert_eq!((fs::read_to_string(&own).unwrap(), fs::read_to_string(&other).unwrap()), ("a\nb\nc\n".into(), "c\n".into()));
        emit("d", Some(&dir.join("missing/x.log")), &mut stderr, Some(&dir.join("missing/x.log")));
        assert!(fs::read_to_string(&own).unwrap().ends_with("c\nd\n"));
        fs::remove_dir_all(dir).unwrap();
    }

    /// Defect #2305: one Windows append outlasted CoreQ's 3 s flush wait, so `flush().unwrap()` failed on a healthy store.
    /// Retries only that unacknowledged wait, bounded at 30 s; any other flush error still fails. Returns the retries.
    fn flush_settled(mut unacknowledged: impl FnMut()) -> usize {
        let capture=tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().cloned()).unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(30);
        let mut retries=0;
        loop {
            match capture.flush() {
                Ok(())=>return retries,
                Err(e) if e.starts_with("telemetry flush unacknowledged (")&&std::time::Instant::now()<until=>{retries+=1;unacknowledged();}
                Err(e)=>panic!("{e}"),
            }
        }
    }

    // Chain: § 4 service log lines. Risk: a line without its UTC time, or one message split over lines.
    // Chain: short adapter -> silent/nonzero dependency -> bounded response and private failure evidence.
    // #2305: the store write lock is held until the first flush misses its deadline, as one slow Windows append did.
    #[test]
    fn short_dependency_outputs_are_bounded_and_preserve_nonzero_receipts() {
        crate::tests::with_test_capture(|store,dir| {
            let write=fs::OpenOptions::new().write(true).open(dir.join("db.write.lock")).unwrap();write.lock().unwrap();
            let mut write=Some(write);
            let fixture=|mode:&str| {
                let mut command=std::process::Command::new(std::env::current_exe().unwrap());
                command.args(["--exact","platform::tests::short_dependency_child","--ignored","--nocapture"]).env("TASKQ_SHORT_DEPENDENCY",mode);command
            };
            let began=std::time::Instant::now();
            let error=output("fixture.silent",&mut fixture("silent"),std::time::Duration::from_millis(100)).unwrap_err();
            assert!(error.contains("timeout")&&error.contains("unknown"));
            assert!(began.elapsed()<std::time::Duration::from_secs(2));
            let receipt=output("fixture.nonzero",&mut fixture("nonzero"),std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(receipt.status.code(),Some(7));
            assert!(String::from_utf8_lossy(&receipt.stderr).contains("private dependency receipt"));
            assert_eq!(flush_settled(||drop(write.take())),1,"the append held past the flush deadline");
            let export=store.export(0,0,1024).unwrap();let rows=export["events"].as_array().unwrap();
            for class in ["timeout","exit"] {assert!(rows.iter().any(|r|r["observation"]["error"]==class));}
            assert!(!export.to_string().contains("private dependency receipt"));
        });
    }

    #[test]
    #[ignore="owned silent/nonzero subprocess for the short dependency adapter"]
    fn short_dependency_child() {
        if std::env::var("TASKQ_SHORT_DEPENDENCY").unwrap()=="nonzero" {eprintln!("private dependency receipt");std::process::exit(7);}
        loop {std::thread::park();}
    }

    // Chain: #1625 § 4 service exit -> stderr redirected to taskq.log by the launcher -> process ends with the code.
    // Risk: a service exit leaves no reason in taskq.log, or the line is lost when Windows terminates the process.
    #[test]
    fn service_exit_logs_reason_before_process_ends() {
        let dir = std::env::temp_dir().join(format!("taskq-service-exit-{}", crate::runtime::uuid()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("taskq.log");
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::tests::service_exit_child", "--ignored", "--nocapture"])
            .env("TASKQ_SERVICE_EXIT", "1").stderr(fs::File::create(&log).unwrap()).output().unwrap();
        assert_eq!(o.status.code(), Some(75));
        let text = fs::read_to_string(&log).unwrap();
        assert_eq!(text.lines().filter(|l| l.ends_with(" service exits 75: restart to 9.9.9")).count(), 1, "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "owned subprocess that ends through service_exit"]
    fn service_exit_child() {
        if std::env::var_os("TASKQ_SERVICE_EXIT").is_none() { return; }
        service_log();
        service_exit(75, "restart to 9.9.9");
    }

    #[test]
    fn service_log_lines_start_with_utc_time_and_fold() {
        let at = crate::board::iso("2026-10-14T04:27:05Z").unwrap();
        assert_eq!(log_line(at, "pass: PATCH x: HTTP 502 <html>\n  <p>bad</p>\r\n\n</html>"),"2026-10-14T04:27:05Z pass: PATCH x: HTTP 502 <html> <p>bad</p> </html>");
    }

    // Chain: #1386 boot release. Risk: a wrong boot reading releases a live execution or never releases a dead one.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn boot_time_reads_on_this_host() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let boot = boot_time().unwrap();
        assert!(boot > 1_000_000_000 && boot <= now, "boot {boot} now {now}");
        assert!((boot_time().unwrap() - boot).abs() <= 2, "boot time is stable");
    }

    // Chain: § 3 Auto memory floor. Risk: the floor or pressure reading fails on a real host and Auto never admits.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn host_memory_reads_on_this_host() {
        let memory = host_memory();
        #[cfg(target_os = "macos")]
        if memory.is_none() {
            use std::io::Write;
            writeln!(std::io::stderr(), "sandbox-blocked: host_memory_reads_on_this_host: OS memory probe unavailable").unwrap();
            return;
        }
        let (gib, level) = memory.unwrap();
        assert!(gib > 0.0 && level <= 4, "{gib} GiB, level {level}");
    }

    // Chain: § 8 shared update across services. Risk: a second service downloads or misses the update.
    #[test]
    fn two_services_share_current_without_download_or_smoke() {
        struct FakeService { version: String, starts: usize }
        let install = std::env::temp_dir().join(format!("taskq-shared-update-{}", std::process::id()));
        fs::create_dir_all(&install).unwrap();
        write_atomic(&install.join("current"), VERSION).unwrap();
        let mut first = FakeService { version: VERSION.into(), starts: 1 };
        let mut second = FakeService { version: VERSION.into(), starts: 1 };
        // First service's successful update publishes current; the second still runs the old image.
        let next = "999.0.0";
        write_atomic(&install.join("current"), next).unwrap();
        first.version = next.into();
        first.starts += 1;
        assert!(coreq::update::newer(&tool(&install).current().unwrap(), &second.version));
        if let Some(v) = service_update(&install).unwrap() {
            second.version = v;
            second.starts += 1;
        }
        assert_eq!(second.starts, 2);
        assert_eq!(second.version, first.version);
        // No release executable exists to smoke, and even the download directory was never created.
        assert!(!install.join("releases").exists());
        assert!(!coreq::update::newer(next, &tool(&install).current().unwrap()));
        assert_eq!(fs::read_to_string(install.join("current")).unwrap().trim(), next);
        fs::remove_dir_all(&install).unwrap();
    }

    // Chain: § 8 item 1 dev install (#1388, on demand #2311). Risk: a requested dev build never installs the green main
    // SHA, installs one with red or pending CI, reinstalls the same or a failed SHA in a loop, starts a second build,
    // a live host builds at all, or a merge builds with no request (135-200 build min/day, spawns 12.9/h -> 4.5/h).
    #[test]
    fn dev_install_starts_a_requested_green_main_sha_once() {
        let (a,b)=("a".repeat(40),"b".repeat(40));
        let mut reads=vec![];
        let mut green=|sha:&str|{reads.push(sha.to_string());Ok(())};
        assert_eq!(dev_next("dev",false,Some(&b),Some(&a),None,false,&mut green),None,"no request: a new green SHA waits");
        assert_eq!(dev_next("dev",true,Some(&b),Some(&a),None,false,&mut green),Some(b.clone()),"requested green SHA installs");
        assert_eq!(dev_next("dev",true,Some(&a),Some(&a),None,false,&mut green),None,"same SHA: no-op");
        assert_eq!(dev_next("dev",true,Some(&b),Some(&a),Some(&b),false,&mut green),None,"started or failed SHA: not retried");
        assert_eq!(dev_next("dev",true,Some(&b),Some(&a),None,true,&mut green),None,"one build at a time");
        assert_eq!(dev_next("live",true,Some(&b),Some(&a),None,false,&mut green),None,"channel live: no-op");
        assert_eq!(dev_next("dev",true,None,Some(&a),None,false,&mut green),None,"unknown main SHA waits");
        assert_eq!(reads,vec![b.clone()],"CI is read only for a SHA that could start");
        for ci in ["merge_group CI not green on merge result","CI tests pending"] {
            assert_eq!(dev_next("dev",true,Some(&b),Some(&a),None,false,&mut |_|Err(ci.into())),None,"red or pending CI waits");
        }
        // the script's report folds once into installed/failed; a started SHA stays started, so it never restarts
        #[cfg(unix)] {
        let install=std::env::temp_dir().join(format!("taskq-dev-{}",std::process::id()));
        let _=fs::remove_dir_all(&install);
        fs::create_dir_all(install.join("dev")).unwrap();
        fs::write(install.join("dev/installed"),&a).unwrap();
        fs::write(install.join("dev/started"),&b).unwrap();
        fs::write(install.join("dev/result"),format!("{b} failed\n")).unwrap();
        let v=dev_tick(&install,&install,&mut |_|panic!("no request: no main SHA read, no CI read"));
        assert_eq!((v["installed"].as_str(),v["failed"].as_str(),v["building"].as_bool(),v["requested"].as_bool()),(Some(a.as_str()),Some(b.as_str()),Some(false),Some(false)));
        assert!(!install.join("dev/result").exists(),"report read once");
        fs::write(install.join("dev/result"),format!("{b} installed")).unwrap();
        let v=dev_tick(&install,&install,&mut |_|Ok(()));
        assert_eq!((v["installed"].as_str(),v["failed"].as_str()),(Some(b.as_str()),None),"success clears the failed SHA");
        fs::remove_dir_all(&install).unwrap();
        }
    }

    // Chain: § 8 item 1 dev request from a dependency (#2321). Risk: a ready task waits on a merge the dev host never
    // builds without a PM step, a dependency installed already requests a build, or the same close requests again
    // after its own merge commit (timed before the close) installs.
    #[cfg(unix)]
    #[test]
    fn dev_request_follows_a_dependency_closed_after_the_installed_commit() {
        let base=std::env::temp_dir().join(format!("taskq-dev-dep-{}",std::process::id()));
        let _=fs::remove_dir_all(&base);
        let (root,install)=(base.join("root"),base.join("install"));
        fs::create_dir_all(install.join("dev")).unwrap();
        fs::create_dir_all(&root).unwrap();
        let git=|args:&[&str]|{
            let o=std::process::Command::new("git").arg("-C").arg(&root).args(["-c","user.name=t","-c","user.email=t@example.invalid"]).args(args)
                .env("GIT_COMMITTER_DATE","@1000 +0000").env("GIT_AUTHOR_DATE","@1000 +0000").output().unwrap();
            assert!(o.status.success(),"git {args:?}: {}",String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&["init","-q"]);git(&["commit","-q","--allow-empty","-m","installed"]);
        let request=dev_request(&install);
        assert!(!dev_dep_request(&install,&root,Some(2000)),"no installed SHA: unknown, no request");
        fs::write(install.join("dev/installed"),"f".repeat(40)).unwrap();
        assert!(!dev_dep_request(&install,&root,Some(2000)),"an installed SHA the root lacks: no request");
        fs::write(install.join("dev/installed"),git(&["rev-parse","HEAD"])).unwrap();
        assert!(!dev_dep_request(&install,&root,None),"no ready task with a done dependency");
        assert!(!dev_dep_request(&install,&root,Some(1000))&&!dev_dep_request(&install,&root,Some(900)),"closed before the installed commit: installed");
        assert!(!request.exists());
        assert!(dev_dep_request(&install,&root,Some(1001))&&request.exists(),"closed after the installed commit: requested");
        fs::remove_file(&request).unwrap();
        assert!(!dev_dep_request(&install,&root,Some(1001))&&!request.exists(),"the same close requests once");
        assert!(dev_dep_request(&install,&root,Some(1500))&&request.exists(),"a newer close requests again");
        fs::remove_dir_all(&base).unwrap();
    }

    // Chain: § 8 item 1 dev install gate (#1406). Risk: a root checked out before scripts/dev-install.sh existed never
    // installs a green main merge, or the install runs the root's own copy or touches its working tree.
    #[cfg(unix)]
    #[test]
    fn dev_install_runs_the_fetched_script_from_a_root_without_one() {
        let base=std::env::temp_dir().join(format!("taskq-dev-gate-{}",std::process::id()));
        let _=fs::remove_dir_all(&base);
        let (origin,root,w,install)=(base.join("origin.git"),base.join("root"),base.join("w"),base.join("install"));
        fs::create_dir_all(install.join("dev")).unwrap();
        let git=|dir:&Path,args:&[&str]|{
            let o=std::process::Command::new("git").arg("-C").arg(dir).args(["-c","user.name=t","-c","user.email=t@example.invalid"]).args(args).output().unwrap();
            assert!(o.status.success(),"git {args:?}: {}",String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&base,&["init","-q","--bare","-b","main",origin.to_str().unwrap()]);
        git(&base,&["clone","-q",origin.to_str().unwrap(),w.to_str().unwrap()]);
        git(&w,&["checkout","-q","-b","main"]);
        fs::write(w.join("README"),"old main\n").unwrap();
        git(&w,&["add","README"]);git(&w,&["commit","-qm","old"]);git(&w,&["push","-q","origin","main"]);
        git(&base,&["clone","-q",origin.to_str().unwrap(),root.to_str().unwrap()]);
        // the new main adds the script; the root stays at the old commit without it
        let script="touch \"$0.done\"\n";
        fs::create_dir_all(w.join("scripts")).unwrap();
        fs::write(w.join("scripts/dev-install.sh"),script).unwrap();
        git(&w,&["add","scripts"]);git(&w,&["commit","-qm","new"]);git(&w,&["push","-q","origin","main"]);
        assert!(!root.join("scripts/dev-install.sh").exists());
        // #1633: another fetch in the root holds origin/main's ref lock; the install must not need that ref
        let tracking=git(&root,&["rev-parse","origin/main"]);
        fs::write(root.join(".git/refs/remotes/origin/main.lock"),"").unwrap();
        fs::write(install.join("host.json"),r#"{"channel":"dev"}"#).unwrap();
        assert!(dev_enabled(RELEASE_REPO,&install),"gate needs no script in the root");
        assert!(!dev_enabled("other/repo",&install)&&!dev_enabled(RELEASE_REPO,&base),"other repo or channel live: off");
        // the start line goes to this thread's project log
        service_log();
        project_log(base.join("project.log"));
        // #2311: a green merge with no request starts nothing
        dev_tick(&install,&root,&mut |_|panic!("no request: no CI read"));
        assert!(!install.join("dev/started").exists(),"no request: nothing starts");
        // a request takes the newest green main SHA and clears when the build starts
        fs::write(w.join("README"),"newest main\n").unwrap();
        git(&w,&["add","README"]);git(&w,&["commit","-qm","newest"]);git(&w,&["push","-q","origin","main"]);
        let sha=git(&w,&["rev-parse","HEAD"]);
        fs::write(dev_request(&install),"").unwrap();
        let mut read=None;
        let v=dev_tick(&install,&root,&mut |s|{read=Some(s.to_string());Ok(())});
        assert_eq!(read.as_deref(),Some(sha.as_str()),"CI read for the newest main SHA");
        assert!(!dev_request(&install).exists()&&v["requested"]==false,"request cleared on start");
        assert_eq!(fs::read_to_string(install.join("dev/started")).unwrap().trim(),sha);
        assert!(fs::read_to_string(base.join("project.log")).unwrap().contains(&format!("dev install: start {sha}")));
        let done=install.join("dev/install.sh.done");
        for _ in 0..100 { if done.exists() {break;} std::thread::sleep(std::time::Duration::from_millis(100)); }
        assert!(done.exists(),"fetched script ran: {}",fs::read_to_string(install.join(format!("dev/{sha}.install.log"))).unwrap_or_default());
        assert_eq!(fs::read_to_string(install.join("dev/install.sh")).unwrap(),script,"script taken from the fetched SHA");
        assert!(!root.join("scripts/dev-install.sh").exists()&&git(&root,&["status","--porcelain"]).is_empty(),"root working tree untouched");
        assert_eq!(git(&root,&["rev-parse","origin/main"]),tracking,"shared remote-tracking ref untouched");
        // #2208 A1: each failed SHA keeps its diagnostic output through the next build.
        for message in ["first install failed", "second install failed"] {
            fs::write(w.join("scripts/dev-install.sh"),format!("echo '{message}'\necho \"$1 failed\" > \"$(dirname \"$0\")/result\"\nexit 1\n")).unwrap();
            git(&w,&["add","scripts"]);git(&w,&["commit","-qm",message]);git(&w,&["push","-q","origin","main"]);
            let failed_sha=git(&w,&["rev-parse","HEAD"]);
            fs::write(dev_request(&install),"").unwrap();
            dev_tick(&install,&root,&mut |_|Ok(()));
            for _ in 0..100 { if install.join("dev/result").exists() {break;} std::thread::sleep(std::time::Duration::from_millis(100)); }
            assert!(install.join("dev/result").exists(),"failed build reports completion");
            dev_tick(&install,&root,&mut |_|Ok(()));
            let path=format!("dev/{failed_sha}.install.log");
            assert!(fs::read_to_string(install.join(&path)).unwrap().contains(message),"failed SHA retains its output");
            assert!(fs::read_to_string(base.join("project.log")).unwrap().contains(&format!("dev install: {failed_sha} failed; see {path}")),"failure names retained log");
            if message=="first install failed" {
                fs::write(base.join("first-sha"),&failed_sha).unwrap();
            }
        }
        let first=fs::read_to_string(base.join("first-sha")).unwrap();
        assert!(fs::read_to_string(install.join(format!("dev/{first}.install.log"))).unwrap().contains("first install failed"),"next SHA leaves first failure intact");
        // a request while main is already installed clears and starts nothing
        let main=git(&w,&["rev-parse","HEAD"]);
        fs::write(install.join("dev/installed"),&main).unwrap();
        fs::remove_file(install.join("dev/started")).unwrap();
        fs::write(dev_request(&install),"").unwrap();
        dev_tick(&install,&root,&mut |_|panic!("installed main: no CI read"));
        assert!(!dev_request(&install).exists()&&!install.join("dev/started").exists(),"installed main: request cleared, nothing started");

        fs::remove_dir_all(&base).unwrap();
    }

    // Chain: § 8 update -> doctor `version` (#2194). Risk: the doctor trusted any `latest.json` text or left its probe
    // behind. Protection: only a plain version is returned and the probe file is always removed.
    #[test]
    fn latest_release_reads_a_plain_version_and_leaves_no_probe() {
        let install=std::env::temp_dir().join(format!("taskq-latest-{}",crate::runtime::uuid()));
        for (body,want) in [(r#"{"version":"4.6.60"}"#,Ok("4.6.60")),(r#"{"version":"4.6.60; rm"}"#,Err(())),(r#"{}"#,Err(()))] {
            let fetch=|_:&str,to:&Path|fs::write(to,body).map_err(|e|e.to_string());
            assert_eq!(latest_with(&install,&fetch).as_deref().map_err(|_|()),want,"{body}");
            assert!(fs::read_dir(&install).unwrap().next().is_none(),"probe removed");
        }
        assert!(latest_with(&install,&|_,_|Err("offline".into())).is_err());
        fs::remove_dir_all(&install).unwrap();
    }

    // Chain: § 8 item 1 host channel dev -> live. Risk: a dev host installs a release, a live host stays on a -dev
    // build forever, or a missing channel stops releases.
    #[test]
    fn channel_dev_holds_releases_and_live_replaces_a_dev_build() {
        let install=std::env::temp_dir().join(format!("taskq-channel-{}",std::process::id()));
        let _=fs::remove_dir_all(&install);
        let t=tool(&install);
        let (dev,release)=(format!("{VERSION}-dev.1"),"0.9.0");
        // #2049 A1: a Windows update derives the GUI launcher from the release, so the fixture is a console PE image.
        let image=|v:&str|-> Vec<u8> {
            if !cfg!(windows) {return v.as_bytes().to_vec();}
            let mut b=vec![0u8;0x40+24+240];
            b[..2].copy_from_slice(b"MZ");b[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());b[0x40..0x44].copy_from_slice(b"PE\0\0");
            b[0x40+20..0x40+22].copy_from_slice(&240u16.to_le_bytes());b[0x40+24..0x40+26].copy_from_slice(&0x20bu16.to_le_bytes());
            b[0x40+24+68..0x40+24+70].copy_from_slice(&3u16.to_le_bytes());b.extend_from_slice(v.as_bytes());b
        };
        for v in [dev.as_str(),release] {fs::create_dir_all(t.release_exe(v).parent().unwrap()).unwrap();fs::write(t.release_exe(v),image(v)).unwrap();}
        write_atomic(&install.join("current"),&dev).unwrap();
        let latest=std::cell::RefCell::new(release.to_string());
        let own=std::cell::Cell::new(true);
        let fetch=|name:&str,to:&Path| -> Result<(),String> {
            if name!="latest.json" {return Err("no network in tests".into());}
            let v=latest.borrow().clone();
            let file=format!("taskq-{v}-{}{}",coreq::update::target(),std::env::consts::EXE_SUFFIX);
            let target=if own.get() {coreq::update::target()} else {"other-os".into()};
            fs::write(to,serde_json::json!({"version":v,"files":{target:file}}).to_string()).map_err(|e|e.to_string())
        };
        // dev: neither a newer nor an older release replaces the running version
        fs::write(install.join("host.json"),r#"{"channel":"dev"}"#).unwrap();
        assert_eq!(channel(&install).unwrap(),"dev");
        for v in ["999.0.0",release] {
            *latest.borrow_mut()=v.into();
            assert_eq!(channel_update(&install,&fetch).unwrap(),None);
            assert_eq!(t.current().unwrap(),dev);
        }
        // dev still re-execs into a newer local current written by dev-install
        write_atomic(&install.join("current"),"999.0.1-dev.2").unwrap();
        assert_eq!(channel_update(&install,&fetch).unwrap().as_deref(),Some("999.0.1-dev.2"));
        write_atomic(&install.join("current"),&dev).unwrap();
        // missing channel = live: the latest release replaces the higher -dev build at the next check
        fs::remove_file(install.join("host.json")).unwrap();
        assert_eq!(channel(&install).unwrap(),"live");
        // #2234: a release whose file for this OS is not published yet waits without an error
        own.set(false);
        assert_eq!(channel_update(&install,&fetch).unwrap(),None);
        assert_eq!(t.current().unwrap(),dev);
        own.set(true);
        assert_eq!(channel_update(&install,&fetch).unwrap().as_deref(),Some(release));
        assert_eq!((t.current().unwrap(),fs::read_to_string(install.join("previous")).unwrap()),(release.to_string(),dev.clone()));
        assert_eq!(fs::read(install.join(t.exe_name())).unwrap(),image(release));
        if cfg!(windows) {assert!(install.join(t.gui_exe_name()).is_file(),"the update places the GUI launcher the Scheduled Tasks start");}
        fs::write(install.join("host.json"),r#"{"channel":"beta"}"#).unwrap();
        assert!(channel(&install).is_err());
        fs::remove_dir_all(&install).unwrap();
    }

    // Chain: incident update.smoke. Risk: download failures reported as smoke failures.
    #[test]
    fn incident_smoke_detection_excludes_download_and_security_failures() {
        crate::incident::tests::take_detected();
        let smoke = Err("4.2.0 selftest failed: private diagnostic".into());
        assert_eq!(update_result(smoke.clone()), smoke);
        assert_eq!(crate::incident::tests::take_detected(), ["update.smoke"]);
        for e in ["download failed", "sha256 not in SHA256SUMS", "curl: timeout"] {
            let failure = Err(e.into());
            assert_eq!(update_result(failure.clone()), failure);
        }
        assert!(crate::incident::tests::take_detected().is_empty());
    }

    // Chain: § 3 load gate readings. Risk: load read wrong or missing on this OS.
    #[test]
    fn load_readings_parse_and_read_on_this_host() {
        assert_eq!(proc_stat("cpu  10 2 8 70 5 1 1 3 4 0\ncpu0 1 1 1 1 1 1 1 1 1 1\n"), Some((25, 100)), "idle + iowait; guest not added");
        assert_eq!(proc_stat("intr 1"), None);
        assert_eq!(meminfo("MemTotal:  1000 kB\nMemFree: 100 kB\nMemAvailable:   250 kB\n"), Some(25.0));
        assert_eq!(meminfo("MemTotal:  1000 kB\nMemFree: 100 kB\n"), None, "no MemAvailable: unknown");
        // R12: a native reading on this platform, plausible bounds only
        let (busy, total) = cpu_ticks().expect("cpu ticks");
        assert!(busy <= total && total > 0);
        let m = memory_available().expect("available memory");
        assert!(m > 0.0 && m < 100.0, "{m}");
    }

    // H-A5/#973: local signing and repeated preparation cannot rewrite the immutable release or active files.
    #[test]
    fn repeated_install_preparation_preserves_immutable_release() {
        let install=std::env::temp_dir().join(format!("taskq-install-self-{}",crate::runtime::uuid()));
        let t=tool(&install);
        let first=prepare_install(&t).unwrap();
        assert!(!install.join("current").exists()&&!install.join(t.exe_name()).exists());
        let bytes=fs::read(t.release_exe(VERSION)).unwrap();
        #[cfg(unix)] let inode={use std::os::unix::fs::MetadataExt;fs::metadata(t.release_exe(VERSION)).unwrap().ino()};
        assert_eq!(prepare_install(&t).unwrap(),first);
        assert_eq!(fs::read(t.release_exe(VERSION)).unwrap(),bytes);
        #[cfg(unix)] {use std::os::unix::fs::MetadataExt;assert_eq!(fs::metadata(t.release_exe(VERSION)).unwrap().ino(),inode);}
        activate_install(&t).unwrap();
        assert_eq!(fs::read(install.join(t.exe_name())).unwrap(),bytes);
        assert_eq!(fs::read_to_string(install.join("current")).unwrap(),VERSION);
        fs::write(t.release_exe(VERSION),"changed-same-version").unwrap();
        assert!(prepare_install(&t).unwrap_err().contains("differs"));
        assert_eq!(fs::read(t.release_exe(VERSION)).unwrap(),b"changed-same-version");
        fs::remove_dir_all(install).unwrap();
    }

    // Windows install layout: relocated executable/real launcher must find GNU's loader without Cargo's PATH.
    #[cfg(windows)]
    #[test]
    fn installed_windows_release_starts_without_build_paths() {
        let base=std::env::temp_dir().join(format!("taskq-windows-install-{}",crate::runtime::uuid()));
        let appdata=base.join("appdata");let install=appdata.join("taskq");let t=tool(&install);
        prepare_install(&t).unwrap();activate_install(&t).unwrap();
        let system32=PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        for exe in [t.release_exe(VERSION),install.join(t.exe_name())] {
            let out=coreq::process::output("test.installed-windows-binary",std::process::Command::new(&exe)
                .args(["--exact","platform::tests::installed_windows_binary_fixture","--ignored","--nocapture"])
                .current_dir(&base).env("PATH",&system32).env("LOCALAPPDATA",&appdata).env_remove("TASKQ_ROOT")
                .env("TASKQ_INSTALL_FIXTURE",&install).env("TASKQ_SELFTEST_OFFLINE","1").env("TASKQ_INCIDENT_REPORTS","off"),std::time::Duration::from_secs(30));
            assert!(out.is_ok(),"{}: {out:?}",exe.display());
            assert!(String::from_utf8_lossy(&out.unwrap().stdout).contains("golden traces PASS"));
        }
        #[cfg(target_env="gnu")] for folder in [&install,t.release_exe(VERSION).parent().unwrap()] {assert!(folder.join("WebView2Loader.dll").is_file());}
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore="owned relocated-binary fixture; requires TASKQ_INSTALL_FIXTURE"]
    fn installed_windows_binary_fixture() {
        let install=PathBuf::from(std::env::var_os("TASKQ_INSTALL_FIXTURE").expect("owned fixture installation"));
        if std::env::current_exe().unwrap().parent()==Some(install.as_path()) {
            coreq::launcher::launch(&tool(&install),&std::env::args().skip(1).collect::<Vec<_>>());
        }
        let _telemetry=coreq::telemetry::capture::Runtime::install_bounded(
            &install.join("telemetry.log/universal.sqlite3"),"taskq",VERSION,crate::telemetry::budget(&install,"telemetry.sqlite3").unwrap(),
        ).unwrap();
        crate::run(&["selftest".into()]).unwrap();
    }

    // Chain: § 8 step 5 macOS signing (#1089). Risk: an absent identity or a failed signature passes silently, or two
    // releases end up with requirements TCC does not match as one app.
    #[cfg(target_os = "macos")]
    #[test]
    fn local_identity_signs_releases_under_one_requirement() {
        let dir = std::env::temp_dir().join(format!("taskq-sign-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        fs::copy("/usr/bin/true", &a).unwrap();
        fs::copy("/usr/bin/false", &b).unwrap();
        let before = fs::read(&a).unwrap();
        assert_eq!(codesign("TaskQ No Such Identity", &a, "org.taskq.taskq"), Ok(false));
        assert_eq!(fs::read(&a).unwrap(), before, "an absent identity touched the file");
        let failed = std::process::Command::new("/bin/sh").args(["-c", "echo boom >&2; exit 1"]).output().unwrap();
        assert!(identity_hash(SIGNING_IDENTITY, &failed).unwrap_err().contains("boom"));
        // ponytail: CI has no identity; the rest runs on a host where `service install` signs
        let signed = codesign(SIGNING_IDENTITY, &a, "org.taskq.taskq");
        assert!(signed.is_ok(), "{signed:?}");
        if signed == Ok(true) {
            assert_eq!(codesign(SIGNING_IDENTITY, &b, "org.taskq.taskq"), Ok(true));
            let req = |p: &Path| String::from_utf8(std::process::Command::new("codesign").args(["-d", "-r-"]).arg(p).output().unwrap().stdout).unwrap();
            assert_eq!(req(&a), req(&b));
            assert!(req(&a).contains("identifier \"org.taskq.taskq\" and certificate leaf"), "{}", req(&a));
            assert!(codesign(SIGNING_IDENTITY, &dir.join("missing"), "org.taskq.taskq").is_err());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    // H-A5: all update entrypoints respect installation ownership, including the current-version fast path.
    #[test]
    fn installation_transition_fences_updates_before_release_selection_or_effects() {
        let install=std::env::temp_dir().join(format!("taskq-install-update-{}",crate::runtime::uuid()));
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("current"),"999.0.0").unwrap();
        let lock=update_lock(&install).unwrap();
        assert!(service_update(&install).unwrap_err().contains("busy"));
        for check in [true,false] {assert!(update(&install,check).unwrap_err().contains("busy"));}
        drop(lock);
        // An installer crash releases the OS lock but leaves its recovery/admission marker.
        let prepare=format!("{}.pending",crate::host::INSTALL_PENDING);
        for name in ["projects.pending",crate::host::INSTALL_PENDING,prepare.as_str()] {
            fs::write(install.join(name),"retained-install-binding").unwrap();
            assert!(service_update(&install).unwrap_err().contains(name));
            for check in [true,false] {assert!(update(&install,check).unwrap_err().contains(name));}
            assert_eq!(fs::read_to_string(install.join("current")).unwrap(),"999.0.0");
            assert!(!install.join("releases").exists(),"a held transition must not download or prune");
            fs::remove_file(install.join(name)).unwrap();
        }
        assert_eq!(service_update(&install).unwrap(),Some("999.0.0".into()));
        fs::remove_dir_all(install).unwrap();
    }

    // #1415 pattern: a dev install right after the host re-execs meets each project's update step holding the lock.
    #[test]
    fn installer_waits_a_bounded_time_for_a_brief_update_holder() {
        let install = std::env::temp_dir().join(format!("taskq-update-wait-{}", crate::runtime::uuid()));
        let lock = update_lock(&install).unwrap();
        let started = std::time::Instant::now();
        assert!(update_lock_within(&install, std::time::Duration::from_millis(300)).err().is_some_and(|e| e.contains("busy")));
        assert!(started.elapsed() >= std::time::Duration::from_millis(300));
        let holder = std::thread::spawn(move || {std::thread::sleep(std::time::Duration::from_millis(200)); drop(lock)});
        assert!(update_lock_within(&install, std::time::Duration::from_secs(5)).is_ok(), "a brief holder must not fail an install");
        holder.join().unwrap();
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: CLI/service update ownership -> cleanup -> the same metadata and activation lock stays exclusive.
    #[test]
    fn updates_share_one_lock_through_cleanup_and_release_it() {
        let install = std::env::temp_dir().join(format!("taskq-update-lock-{}", crate::runtime::uuid()));
        let lock = update_lock(&install).unwrap();
        assert!(update_lock(&install).is_err(), "a competing updater must return without blocking service heartbeats");
        sweep(&install);
        let other = fs::OpenOptions::new().write(true).open(install.join("update.lock")).unwrap();
        assert!(other.try_lock().is_err(), "cleanup must not unlink the live update lock");
        #[cfg(unix)]
        let child = {
            let mut pipe = [0; 2];
            assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                // Keep the inherited descriptor alive using only async-signal-safe calls.
                unsafe {
                    libc::close(pipe[1]);
                    let mut byte = 0u8;
                    libc::read(pipe[0], (&mut byte as *mut u8).cast(), 1);
                    libc::_exit(0);
                }
            }
            unsafe { libc::close(pipe[0]); }
            (pid, pipe[1])
        };
        drop(lock);
        let released = other.try_lock();
        #[cfg(unix)]
        unsafe {
            libc::write(child.1, b"x".as_ptr().cast(), 1);
            libc::close(child.1);
            let mut status = 0;
            assert_eq!(libc::waitpid(child.0, &mut status, 0), child.0);
            assert_eq!(status, 0);
        }
        released.unwrap();
        drop(other);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: § 8 install sweep. Risk: sweep deletes a live release or owned file.
    #[test]
    fn sweep_keeps_the_allowlist_and_live_releases() {
        let t = std::env::temp_dir().join(format!("taskq-sweep-{}", std::process::id()));
        let (install, root) = (t.join("install"), t.join("proj"));
        let _ = fs::remove_dir_all(&t);
        // the running version's release stays, whatever the build sets it to (release CI: the tag)
        let mine = format!("releases/{VERSION}");
        let dirs = ["releases/4.2.0", "releases/4.1.0", "releases/4.0.5", &mine, "releases/cc33ae6", "releases/4.3.0.tmp", "obsolete-service/x", "mac-pre-update-backup", "capacity", "resources", "resource-state", "windows-native/state", "failed-release-evidence"];
        for d in dirs {
            fs::create_dir_all(install.join(d)).unwrap();
        }
        fs::create_dir_all(root.join(".taskq")).unwrap();
        // a project service still on 4.0.5 with a live child: its release stays (A3)
        fs::write(root.join(".taskq/status.json"), r#"{"version": "4.0.5", "kids": [{"task": 1}]}"#).unwrap();
        // #2049 A1: the GUI launcher the Scheduled Tasks start stays; a renamed running one (`.old`) goes
        let files = ["current", "previous", "token", "taskq", "taskqw.exe", "taskqw.exe.old", "taskq.log", "taskq.log.tmp", "proj.log", "tray", "tray.json", "TaskQ-proj.xml", "worker-policy.json", "hold", "admission.json", "admission.lock", "update.lock", ".freshness.json", "current.json", "mac-scheduled-pass.py"];
        for f in files {
            fs::write(install.join(f), "").unwrap();
        }
        fs::write(install.join("hold"), "invented incident hold").unwrap();
        fs::write(install.join("current"), "4.2.0").unwrap();
        fs::write(install.join("previous"), "4.1.0").unwrap();
        fs::write(install.join("windows-native/state/cap_sid"), "native control state").unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        // H-A5: update keeps policy, interrupted writes, exact native recovery and the qualified rollback floor.
        // #1897: the GitLab board token (§ 9) survives byte-identical, like the GitHub `token`.
        let protected=[("host.json","{\"limits\":{\"codex\":2},\"owner_field\":\"keep\"}"),
            ("host.json.pending","interrupted policy; preserve"),("service","123"),("service.lock",""),("poke","wake"),
            ("restart","restart requested"),("update","update requested"),("projects.lock",""),("link-key","host link key; keep"),
            ("gitlab-token","invented-gitlab-token\n"),
            ("accounts-state.pending.json","pending quota observation; preserve"),("accounts-state.lock","quota writer lock; preserve")];
        for (name,text) in protected {fs::write(install.join(name),text).unwrap();}
        fs::create_dir_all(install.join("host-policy-migration")).unwrap();
        fs::write(install.join("host-policy-migration/original.json"),"original configuration").unwrap();
        fs::create_dir_all(install.join("link")).unwrap();
        let native=crate::paths::kids_path(&install,&root).unwrap();fs::create_dir_all(native.parent().unwrap()).unwrap();
        let owner=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let recovery=format!("pid {} {} executor-v1\n7 41 codex 123 child 43 40\n",owner.pid,owner.started);
        fs::write(&native,&recovery).unwrap();
        coreq::launcher::set_rollback_floor(&tool(&install),"4.2.0").unwrap();
        sweep(&install);
        let ls = |d: &Path| {
            let mut v: Vec<String> = fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
            v.sort();
            v
        };
        let mut want = vec![VERSION.to_string(), "4.0.5".into(), "4.1.0".into(), "4.2.0".into(), "4.3.0.tmp".into()];
        want.sort();
        want.dedup();
        assert_eq!(ls(&install.join("releases")), want);
        for (name,text) in protected {assert_eq!(fs::read_to_string(install.join(name)).ok().as_deref(),Some(text),"update lost {name}");}
        assert_eq!(fs::read_to_string(install.join("host-policy-migration/original.json")).unwrap(),"original configuration");
        assert_eq!(fs::read_to_string(&native).unwrap(),recovery);
        assert_eq!(crate::service_run::kids_file(&install,&root).unwrap()[0].execution,43,"native execution identity survives update");
        assert_eq!(fs::read_to_string(install.join("rollback-floor")).unwrap(),"4.2.0");
        let mut owned=vec!["TaskQ-proj.xml", "admission.json", "admission.lock", "capacity", "current", "hold", "previous", "proj.log", "projects", "releases", "resource-state", "resources", "taskq", "taskqw.exe", "taskq.log", "taskq.log.tmp", "token", "tray", "tray.json", "update.lock", "windows-native", "worker-policy.json", "native", "host-policy-migration", "rollback-floor", "link"];
        owned.extend(protected.iter().map(|(name,_)|*name));owned.sort();
        assert_eq!(ls(&install),owned);
        assert_eq!(crate::admission::hold(&install).as_deref(), Some("invented incident hold"), "update cleanup must never release an emergency hold");
        assert_eq!(fs::read_to_string(install.join("windows-native/state/cap_sid")).unwrap(), "native control state");
        // § 4 Data: a corrupt status.json leaves the project's version unknown, so every release stays and so does the file
        fs::write(root.join(".taskq/status.json"), "{\"version\": \"4.0").unwrap();
        fs::create_dir_all(install.join("releases/4.0.1")).unwrap();
        sweep(&install);
        assert!(install.join("releases/4.0.1").exists() && install.join("releases/4.0.5").exists());
        assert_eq!(fs::read_to_string(root.join(".taskq/status.json")).unwrap(), "{\"version\": \"4.0");
        crate::tests::with_test_capture(|store,_| {
            fs::write(root.join(".taskq/status.json"),r#"{"version":"4.0.5"}"#).unwrap();
            fs::write(install.join("projects"),b"\xff").unwrap();
            fs::create_dir_all(install.join("releases/4.0.2")).unwrap();sweep(&install);
            assert!(install.join("releases/4.0.2").exists(),"unreadable registry cannot authorize release deletion");
            fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(install.join("projects.pending"),"retained registry prepare").unwrap();
            sweep(&install);
            assert!(install.join("releases/4.0.2").exists(),"interrupted registry cannot authorize release deletion");
            assert_eq!(fs::read_to_string(install.join("projects.pending")).unwrap(),"retained registry prepare");
            fs::remove_file(install.join("projects.pending")).unwrap();
            fs::remove_file(root.join(".taskq/status.json")).unwrap();
            sweep(&install);
            assert!(install.join("releases/4.0.2").exists(),"missing project status cannot authorize release deletion");
            flush_settled(||{});
            let export=store.export(0,0,1024).unwrap();
            for class in ["registry-unreadable","version-unknown"] {
                assert!(export["events"].as_array().unwrap().iter().any(|row|row["observation"]["error"]==class),"missing cleanup failure {class}");
            }
            let private=serde_json::to_string(root.to_str().unwrap()).unwrap();
            assert!(!export.to_string().contains(private.trim_matches('"')),"private local paths entered telemetry");
        });
        let _ = fs::remove_dir_all(&t);
    }
