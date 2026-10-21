use std::{fs, process::Command};

// Chain: unavailable receipts -> Resume refuses before mutation, Pause remains usable, then durable recovery.
#[test]
fn focus_cli_requires_receipts_without_blocking_safe_pause() {
    let root=std::env::temp_dir().join(format!("qc-receipt-{}",std::process::id()));
    let install=root.join("i");fs::create_dir_all(&install).unwrap();fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(root.join(".taskq/focus"),"off").unwrap();fs::write(root.join(".taskq/focus.paused"),"all").unwrap();
    fs::write(install.join("hold"),"owner hold").unwrap();
    fs::write(install.join("telemetry.log"),"store unavailable").unwrap();
    let invoke=|command:&[&str]|Command::new(env!("CARGO_BIN_EXE_taskq")).arg("service").args(command)
        .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install).env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
    let refused=invoke(&["resume"]);
    assert!(!refused.status.success(),"Resume accepted without a committed receipt");
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("command not applied: receipt unavailable"));
    fs::write(root.join(".taskq/focus"),"all").unwrap();
    let paused=invoke(&["pause"]);
    assert!(!paused.status.success(),"unrecorded Pause must report uncertainty");
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off","receipt failure blocked safe Pause");
    assert!(String::from_utf8_lossy(&paused.stderr).contains("applied locally; durable receipt unknown"));
    assert!(!invoke(&["focus","all"]).status.success());
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off");
    fs::write(root.join(".taskq/focus"),"all").unwrap();
    let off=invoke(&["focus","off"]);assert!(!off.status.success());
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off");
    assert!(String::from_utf8_lossy(&off.stderr).contains("applied locally; durable receipt unknown"));
    fs::remove_file(install.join("telemetry.log")).unwrap();
    let resumed=invoke(&["resume"]);assert!(resumed.status.success(),"{}",String::from_utf8_lossy(&resumed.stderr));
    assert!(String::from_utf8_lossy(&resumed.stdout).contains("service observation pending"));
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"all");
    assert_eq!(fs::read_to_string(install.join("hold")).unwrap(),"owner hold");
    let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
    let page=reader.export(0,0,1024).unwrap();
    let output=String::from_utf8_lossy(&resumed.stdout);
    let printed=output.lines().find_map(|line|line.strip_prefix("Command applied locally; service observation pending; receipt ")).unwrap();
    assert!(page["events"].as_array().unwrap().iter().any(|row|row["id"]==printed&&row["observation"]["kind"]=="command.applied"));
    let receipts:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"]).filter(|r|r["source"]=="command").collect();
    assert_eq!(receipts.len(),2);assert_eq!(receipts[0]["kind"],"command.received");assert_eq!(receipts[1]["kind"],"command.applied");
    assert_eq!(receipts[0]["span"],receipts[1]["span"]);assert_eq!(receipts[0]["operation"],"taskq.resume");
    drop(reader);fs::remove_dir_all(root).unwrap();
}

// Chain: independent command handoff -> real CLI refusal -> causal history, with owner hold preserved.
#[test]
fn independent_cli_preserves_the_dispatch_trace_on_failure() {
    let root=std::env::temp_dir().join(format!("qc-link-{}",std::process::id()));
    let install=root.join("i");fs::create_dir_all(&install).unwrap();fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(install.join("hold"),"owner-held fixture").unwrap();
    fs::write(root.join(".taskq/focus"),"off").unwrap();
    let trace=format!("{}.1","a".repeat(32));let parent=format!("{}.2","b".repeat(32));
    let out=Command::new(env!("CARGO_BIN_EXE_taskq")).args(["service","resume"])
        .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install)
        .env("TASKQ_INCIDENT_REPORTS","off").env("COREQ_TRACE_CONTEXT",format!("1 {trace} {parent}"))
        .output().unwrap();
    assert!(!out.status.success());assert!(String::from_utf8_lossy(&out.stderr).contains("no saved focus"));
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off");
    assert_eq!(fs::read_to_string(install.join("hold")).unwrap(),"owner-held fixture");
    let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
    let exported=reader.export(0,0,1024).unwrap();
    let rows:Vec<_>=exported["events"].as_array().unwrap().iter().map(|r|&r["observation"]).collect();
    let terminal=rows.iter().find(|r|r["operation"]=="taskq.resume"&&r["kind"]=="method").unwrap();
    assert_eq!(terminal["trace"],trace,"CLI refusal lost its dispatch identity");
    assert_eq!(terminal["outcome"],"failed");
    assert!(rows.iter().any(|r|r["operation"]=="taskq.continuation"&&r["kind"]=="method.start"&&r["trace"]==trace&&r["parent"]==parent));
    drop(reader);fs::remove_dir_all(root).unwrap();
}

// Chain: emergency state -> all registered wakeups -> partial failure, without undoing safety state.
#[test]
fn emergency_state_survives_failed_wakeup_and_notifies_other_projects() {
    let root=std::env::temp_dir().join(format!("qh-{}",std::process::id()));
    let install=root.join("i"); let good=root.join("good"); let bad=root.join("bad");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(bad.join(".taskq/poke")).unwrap();
    fs::write(install.join("projects"),format!("{}\n{}\n",bad.display(),good.display())).unwrap();
    let invoke=|command|Command::new(env!("CARGO_BIN_EXE_taskq")).args(["service",command])
        .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install)
        .env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
    for command in ["hold","release"] {
        let out=invoke(command);
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("1 service wake(s) failed"));
        assert_eq!(install.join("hold").is_file(),command=="hold");
        assert!(good.join(".taskq/poke").is_file(),"first failure prevented notification of the next project");
        fs::remove_file(good.join(".taskq/poke")).unwrap();
    }
    fs::remove_file(install.join("projects")).unwrap();
    fs::create_dir(install.join("projects")).unwrap();
    let out=invoke("hold");
    assert!(!out.status.success()); assert!(install.join("hold").is_file());
    assert!(String::from_utf8_lossy(&out.stderr).contains("project registry unreadable"));
    let store=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
    let rows=store.export(0,0,1024).unwrap();
    assert!(rows["events"].as_array().unwrap().iter().any(|r|r["observation"]["error"]=="registry-unreadable"));
    drop(store); fs::remove_dir_all(root).unwrap();
}

// Chain: Resume refusal/success -> focus readback -> durable linked trace after process exit.
// Risk: a successful command is mistaken for admission, or a failed command disappears from history.
#[test]
fn resume_records_its_result_without_releasing_the_host_hold() {
    let root=std::env::temp_dir().join(format!("qr-{}",std::process::id()));
    fs::create_dir_all(root.join(".taskq")).unwrap();
    let install=root.join("i");fs::create_dir_all(&install).unwrap();
    fs::write(install.join("hold"),"owner-held fixture").unwrap();
    fs::write(root.join(".taskq/focus"),"off").unwrap();
    let invoke=||Command::new(env!("CARGO_BIN_EXE_taskq")).args(["service","resume"])
        .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install)
        // Independent owner commands do not inherit the test runner's continuation.
        .env_remove("COREQ_TRACE_CONTEXT").env("COREQ_TELEMETRY_STORE_TRACE","1")
        .env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
    let refused=invoke();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("no saved focus"));
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off");
    fs::write(root.join(".taskq/focus.paused"),"telemetry-fixture").unwrap();
    let resumed=invoke();
    assert!(resumed.status.success(),"{}\n{}",String::from_utf8_lossy(&resumed.stderr),
        fs::read_to_string(root.join(".taskq/cli-errors.log")).unwrap_or_default());
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"telemetry-fixture");
    assert_eq!(fs::read_to_string(install.join("hold")).unwrap(),"owner-held fixture");
    fs::write(root.join(".taskq/focus"),"off").unwrap();
    fs::remove_file(root.join(".taskq/poke")).unwrap();
    fs::create_dir(root.join(".taskq/poke")).unwrap();
    let undelivered=invoke();
    assert!(!undelivered.status.success(),"Resume reported success when its wake signal could not be written");
    assert!(String::from_utf8_lossy(&undelivered.stderr).contains("service wake failed"));
    assert!(String::from_utf8_lossy(&undelivered.stderr).contains("os error"),"caller lost the original filesystem error");
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"off","an unavailable command boundary must refuse before mutation");
    assert_eq!(fs::read_to_string(install.join("hold")).unwrap(),"owner-held fixture");
    let exported=Command::new(env!("CARGO_BIN_EXE_taskq")).args(["profile","export"])
        .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install)
        .env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
    assert!(exported.status.success(),"{}",String::from_utf8_lossy(&exported.stderr));
    let export=String::from_utf8(exported.stdout).unwrap();
    let rows:Vec<serde_json::Value>=export.lines().map(|line|serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows[0]["schema"],2);assert_eq!(rows[0]["coverage"],"committed-import-cursor-snapshot");
    let observations:Vec<_>=rows[1..].iter().map(|r|&r["observation"]).collect();
    let terminals:Vec<_>=observations.iter().filter(|r|r["operation"]=="taskq.resume"&&r["kind"]=="method").collect();
    assert_eq!(terminals.len(),3);
    assert_eq!(terminals[0]["outcome"],"failed");assert_eq!(terminals[1]["outcome"],"ok");
    assert_eq!(terminals[2]["outcome"],"failed");
    assert!(observations.iter().any(|r|r["operation"]=="taskq.operation.wake.lock"&&r["kind"]=="method"&&r["error"]=="wake-open"&&r["trace"]==terminals[2]["trace"]));
    assert_ne!(terminals[0]["trace"],terminals[1]["trace"]);
    for terminal in terminals {
        let phases:Vec<_>=observations.iter().filter(|r|r["source"]=="command"&&r["trace"]==terminal["trace"]).map(|r|r["kind"].as_str().unwrap()).collect();
        assert_eq!(phases,["command.received",if terminal["outcome"]=="ok" {"command.applied"} else {"command.failed"}]);
        assert!(terminal["duration_us"].as_u64().is_some());
        assert!(observations.iter().any(|r|r["kind"]=="method.start"&&r["span"]==terminal["span"]&&r["trace"]==terminal["trace"]));
        assert!(observations.iter().any(|r|r["span"]==terminal["parent"]&&r["trace"]==terminal["trace"]));
    }
    assert!(!export.contains("owner-held fixture"));
    assert!(!export.contains(root.to_str().unwrap()));
    #[cfg(target_os="macos")]
    {
        use std::io::BufRead;
        struct Owner(std::process::Child);
        impl Drop for Owner {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
        let mut owner=Owner(Command::new(std::env::current_exe().unwrap())
            .args(["--exact","telemetry_process::resident_analysis_owner","--ignored","--nocapture"])
            .env("TASKQ_ANALYSIS_OWNER",install.join("telemetry.log/universal.sqlite3"))
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap());
        let mut ready=std::io::BufReader::new(owner.0.stdout.take().unwrap());
        loop {let mut line=String::new();assert_ne!(ready.read_line(&mut line).unwrap(),0,"collector stopped before ready");if line.trim()=="ANALYSIS_READY" {break;}}
        let policy=format!("(version 1)(allow default)(deny file-write* (subpath {}))",serde_json::to_string(install.canonicalize().unwrap().to_str().unwrap()).unwrap());
        let denied=Command::new("/usr/bin/sandbox-exec").args(["-p",&policy,"/usr/bin/touch"])
            .arg(install.join("write-probe")).output().unwrap();
        assert!(!denied.status.success(),"test sandbox did not deny writes");
        let read=Command::new("/usr/bin/sandbox-exec").args(["-p",&policy,env!("CARGO_BIN_EXE_taskq"),"profile","export"])
            .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install)
            .env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
        assert!(read.status.success(),"{}",String::from_utf8_lossy(&read.stderr));
        assert!(String::from_utf8_lossy(&read.stdout).contains("taskq.resume"));
        assert!(!String::from_utf8_lossy(&read.stderr).contains("Telemetry: unknown"));
        assert!(!install.join("write-probe").exists());
        drop(owner.0.stdin.take());assert!(owner.0.wait().unwrap().success());
        let snapshot=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
        let page=snapshot.export(0,0,1024).unwrap();
        assert!(page["events"].as_array().unwrap().iter().filter(|r|r["observation"]["operation"]=="taskq.telemetry.export"&&r["observation"]["kind"]=="method"&&r["observation"]["outcome"]=="ok").count()>=2,"protected analysis did not leave its own committed terminal observation");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore="resident shared collector subprocess for the protected analysis chain"]
fn resident_analysis_owner() {
    use std::io::Write;
    let path=std::path::PathBuf::from(std::env::var_os("TASKQ_ANALYSIS_OWNER").unwrap());
    let dir=path.parent().unwrap();let install=dir.parent().unwrap();
    let budget=coreq::telemetry::Budget {lock:install.join("taskq-profile.lock"),
        peers:vec![dir.join("telemetry.sqlite3"),dir.join("telemetry.sqlite3-wal"),dir.join("telemetry.sqlite3-shm"),install.join("taskq.log")],
        total:coreq::telemetry::INSTALLATION_BYTES,reserved:coreq::telemetry::PEER_BYTES};
    // 4.6.19 published this budget; a different identity makes newer clients mismatch (#1294).
    let budget=if std::env::var_os("TASKQ_ANALYSIS_LEGACY_BUDGET").is_some() {coreq::telemetry::Budget {total:64<<20,reserved:(4<<20)+(64<<10)+(2<<20),..budget}} else {budget};
    let _runtime=coreq::telemetry::capture::Runtime::install_bounded(&path,"fixtureq","1",budget).unwrap();
    println!("ANALYSIS_READY");std::io::stdout().flush().unwrap();
    let mut line=String::new();std::io::stdin().read_line(&mut line).unwrap();
}

// Real binary -> shared bootstrap -> generated methods -> return/exit -> offline durable readback.
#[test]
fn commands_collect_without_configuration_and_flush_on_exit() {
    for (_name, args, success) in [("help", vec!["help"], true), ("failure", vec!["service", "run"], false)] {
        let root = std::env::temp_dir().join(format!("qt-{}-{}", u8::from(success), std::process::id()));
        if root.exists() { fs::remove_dir_all(&root).unwrap(); }
        fs::create_dir_all(&root).unwrap();
        let install = root.join("install");
        let out = Command::new(env!("CARGO_BIN_EXE_taskq")).args(args).current_dir(&root)
            .env("TASKQ_INSTALL", &install).env("TASKQ_ROOT", &root)
            .env("TASKQ_INCIDENT_REPORTS", "off").output().unwrap();
        assert_eq!(out.status.success(), success, "{}", String::from_utf8_lossy(&out.stderr));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("Telemetry: unknown"), "{}", String::from_utf8_lossy(&out.stderr));
        let store = coreq::telemetry::Store::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
        let export = store.export(0, 0, 1024).unwrap();
        let rows = export["events"].as_array().unwrap();
        assert!(rows.iter().any(|r| r["observation"]["operation"] == "taskq.application"
            && r["observation"]["kind"] == "method.start"));
        assert!(rows.iter().any(|r| r["observation"]["operation"] == "taskq.run"
            && r["observation"]["kind"] == "method"
            && r["observation"]["outcome"] == if success { "ok" } else { "failed" }));
        assert!(!export.to_string().contains(root.to_str().unwrap()), "host paths leaked");
        drop(store);
        if success {
            // Installed bootstrap must collect in the executable's root before launcher/bundle dispatch.
            let home=root.join("home");let local=root.join("local");
            fs::create_dir(&home).unwrap();fs::create_dir(&local).unwrap();
            let release=install.join("releases").join(env!("CARGO_PKG_VERSION")).join(format!("taskq{}",std::env::consts::EXE_SUFFIX));
            fs::create_dir_all(release.parent().unwrap()).unwrap();
            fs::write(install.join("current"),env!("CARGO_PKG_VERSION")).unwrap();
            let mut entrypoints=vec![install.join(format!("taskq{}",std::env::consts::EXE_SUFFIX)),release];
            #[cfg(target_os="macos")]
            entrypoints.push(install.join("TaskQ.app/Contents/MacOS/taskq"));
            for executable in &entrypoints {
                fs::create_dir_all(executable.parent().unwrap()).unwrap();fs::copy(env!("CARGO_BIN_EXE_taskq"),executable).unwrap();
            }
            fs::remove_dir_all(install.join("telemetry.log")).unwrap();
            for executable in entrypoints {
                let mut command=Command::new(executable);
                command.arg("help").current_dir(&root).env_remove("TASKQ_INSTALL").env_remove("COREQ_TRACE_CONTEXT")
                    .env("HOME",&home).env("LOCALAPPDATA",&local).env("TASKQ_ROOT",&root).env("TASKQ_INCIDENT_REPORTS","off");
                let out=coreq::process::output("test.installed-capture",&mut command,std::time::Duration::from_secs(15)).unwrap();
                assert!(String::from_utf8_lossy(&out.stdout).starts_with("taskq agents"));
                let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
                let history=reader.export(0,0,1024).unwrap();
                assert!(history["events"].as_array().unwrap().iter().any(|r|r["observation"]["operation"]=="taskq.run"&&r["observation"]["kind"]=="method"));
                drop(reader);fs::remove_dir_all(install.join("telemetry.log")).unwrap();
                assert!(!home.join(".local").exists()&&!local.join("taskq").exists(),"installed bootstrap created a second installation");
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}

// An unavailable collector must fail qualification while leaving ordinary command semantics intact.
#[test]
fn selftest_requires_committed_local_capture() {
    let root=std::env::temp_dir().join(format!("qc-{}",std::process::id()));
    if root.exists() {fs::remove_dir_all(&root).unwrap();}
    fs::create_dir_all(&root).unwrap();
    let blocked=root.join("blocked");fs::write(&blocked,"fixture").unwrap();
    // #1419: the offline (staged release) selftest qualifies a private store under the temp directory.
    let invoke=|install: &std::path::Path,command: &str| Command::new(env!("CARGO_BIN_EXE_taskq"))
        .arg(command).current_dir(&root).env("TASKQ_INSTALL",install).env("TASKQ_ROOT",&root)
        .env("TMPDIR",install).env("TMP",install).env("TEMP",install)
        .env("TASKQ_SELFTEST_OFFLINE","1").env("TASKQ_INCIDENT_REPORTS","off").env("COREQ_TELEMETRY_STORE_TRACE","1").output().unwrap();
    let help=invoke(&blocked,"help");assert!(help.status.success(),"ordinary help outcome changed");
    let failed=invoke(&blocked,"selftest");
    assert!(!failed.status.success(),"selftest accepted unavailable capture: {}",String::from_utf8_lossy(&failed.stdout));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("telemetry: FAIL telemetry collector unavailable"));
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("capture committed"));
    assert_eq!(fs::read_to_string(&blocked).unwrap(),"fixture");
    let good=root.join("good");fs::create_dir_all(&good).unwrap();let success=invoke(&good,"selftest");
    let detail=fs::read_to_string(root.join(".taskq/cli-errors.log")).unwrap_or_default();
    assert!(success.status.success(),"{}\n{}",String::from_utf8_lossy(&success.stderr),detail.chars().take(4096).collect::<String>());
    assert!(String::from_utf8_lossy(&success.stdout).contains("telemetry: staged capture committed"));
    let private=||fs::read_dir(&good).unwrap().filter(|e|e.as_ref().unwrap().file_name().to_string_lossy().starts_with("taskq-selftest-")).count();
    // Windows may fail to remove a still-open store at exit (#1514: coreq d04c6020 seals and often frees it);
    // the next staged selftest sweeps it, so at most one store remains.
    if cfg!(windows) {assert!(private()<=1,"private stores accumulated");assert!(invoke(&good,"selftest").status.success());}
    assert!(private()<=usize::from(cfg!(windows)),"private store left behind");
    assert!(!good.join("telemetry.log/universal.sqlite3").exists(),"staged selftest wrote the installation store");
    // An ordinary command still commits into the installation store.
    assert!(invoke(&good,"help").status.success());
    let store=coreq::telemetry::Store::open(&good.join("telemetry.log/universal.sqlite3")).unwrap();
    assert!(!store.export(0,0,1024).unwrap()["events"].as_array().unwrap().is_empty());
    drop(store);
    #[cfg(unix)] {
        use std::{io::Write,process::Stdio,time::{Duration,Instant}};
        fs::write(good.join("telemetry.log/diagnostic.json"),"{broken control").unwrap();
        let invalid_context=root.join("invalid-context");
        for (install,context) in [(&blocked,""),(&good,""),(&invalid_context,"invalid-process-context")] {
            // The shell waits until the reader is closed, then execs the actual product once.
            let mut command=Command::new("/bin/sh");
            command.args(["-c","read -r go || exit 2; exec \"$@\"","owned-capture-fixture",env!("CARGO_BIN_EXE_taskq"),"help"])
                .current_dir(&root).env("TASKQ_INSTALL",install).env("TASKQ_ROOT",&root).env("TASKQ_INCIDENT_REPORTS","off").env_remove("COREQ_TRACE_CONTEXT")
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            if !context.is_empty() {command.env("COREQ_TRACE_CONTEXT",context);}
            let mut child=command.spawn().unwrap();
            drop(child.stderr.take());child.stdin.take().unwrap().write_all(b"go\n").unwrap();
            let until=Instant::now()+Duration::from_secs(15);
            while child.try_wait().unwrap().is_none()&&Instant::now()<until {std::thread::sleep(Duration::from_millis(10));}
            let expired=child.try_wait().unwrap().is_none();if expired {child.kill().unwrap();}
            let result=child.wait_with_output().unwrap();
            assert!(!expired&&result.status.success(),"optional reporting changed ordinary help: {result:?}");
            assert!(String::from_utf8_lossy(&result.stdout).starts_with("taskq agents"));
        }
        assert_eq!(fs::read_to_string(good.join("telemetry.log/diagnostic.json")).unwrap(),"{broken control");
    }
    fs::remove_dir_all(root).unwrap();
}

// Chain (#1419): the old release's resident writer stops acknowledging (Windows 4.6.25: 1.7 s appends,
// half its events lost) -> the staged candidate's offline selftest still qualifies its own capture,
// and adds nothing to, and takes nothing from, the shared store.
#[cfg(unix)]
#[test]
fn staged_selftest_passes_behind_unresponsive_writer() {
    use std::io::BufRead;
    let root=std::env::temp_dir().join(format!("qs-{}",std::process::id()));
    if root.exists() {fs::remove_dir_all(&root).unwrap();}
    let install=root.join("i");fs::create_dir_all(&install).unwrap();
    let path=install.join("telemetry.log/universal.sqlite3");
    struct Owner(std::process::Child);
    impl Drop for Owner {fn drop(&mut self){unsafe {libc::kill(self.0.id() as i32,libc::SIGCONT);}let _=self.0.kill();let _=self.0.wait();}}
    let mut owner=Owner(Command::new(std::env::current_exe().unwrap())
        .args(["--exact","telemetry_process::resident_analysis_owner","--ignored","--nocapture"])
        .env("TASKQ_ANALYSIS_OWNER",&path).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap());
    let mut ready=std::io::BufReader::new(owner.0.stdout.take().unwrap());
    loop {let mut line=String::new();assert_ne!(ready.read_line(&mut line).unwrap(),0,"collector stopped before ready");if line.trim()=="ANALYSIS_READY" {break;}}
    assert_eq!(unsafe {libc::kill(owner.0.id() as i32,libc::SIGSTOP)},0);
    // The release updater's exact invocation (platform::channel_update).
    let out=coreq::process::output("test.staged-selftest",Command::new(env!("CARGO_BIN_EXE_taskq")).arg("selftest").current_dir(&root)
        .env("TASKQ_INSTALL",&install).env("TASKQ_SELFTEST_OFFLINE","1").env_remove("TASKQ_ROOT").env("TASKQ_INCIDENT_REPORTS","off").env_remove("COREQ_TRACE_CONTEXT"),
        std::time::Duration::from_secs(30));
    let out=match out {Ok(out)|Err(coreq::process::Failure::Exit(out))=>out,Err(e)=>panic!("staged selftest: {e}")};
    assert!(out.status.success(),"staged selftest failed behind an unresponsive writer: {}",String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("telemetry: staged capture committed"));
    assert_eq!(unsafe {libc::kill(owner.0.id() as i32,libc::SIGCONT)},0);
    drop(owner.0.stdin.take());assert!(owner.0.wait().unwrap().success());
    let events=coreq::telemetry::Reader::open(&path).unwrap().export(0,0,1024).unwrap()["events"].to_string();
    assert!(!events.contains("taskq.selftest")&&!events.contains("\"taskq\""),"staged selftest wrote the running writer's store: {events}");
    fs::remove_dir_all(root).unwrap();
}

// Chain: actual maintenance CLI bypasses collector bootstrap and refuses an existing live owner.
#[test]
fn migration_cli_uses_quiesced_owner_and_preserves_legacy_text() {
    let root=std::env::temp_dir().join(format!("qm-{}",std::process::id()));
    let install=root.join("i");fs::create_dir_all(&install).unwrap();
    let line=r#"taskq-profile {"op":"spawn","us":7,"error":"none","at_ms":1}"#;
    fs::write(install.join("taskq.log"),format!("{line}\n")).unwrap();
    let invoke=||Command::new(env!("CARGO_BIN_EXE_taskq")).args(["profile","migrate"])
        .current_dir(&root).env("TASKQ_INSTALL",&install).env_remove("TASKQ_ROOT")
        .env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
    let path=install.join("telemetry.log/universal.sqlite3");
    use std::io::BufRead;
    struct Owner(std::process::Child);
    impl Drop for Owner {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
    let mut owner=Owner(Command::new(std::env::current_exe().unwrap())
        .args(["--exact","telemetry_process::resident_analysis_owner","--ignored","--nocapture"])
        .env("TASKQ_ANALYSIS_OWNER",&path).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap());
    let mut ready=std::io::BufReader::new(owner.0.stdout.take().unwrap());
    loop {let mut line=String::new();assert_ne!(ready.read_line(&mut line).unwrap(),0,"collector stopped before ready");if line.trim()=="ANALYSIS_READY" {break;}}
    let blocked=invoke();assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("requires quiesced IPC writer"));
    assert!(!install.join("telemetry.log/telemetry.sqlite3").exists());
    drop(owner.0.stdin.take());assert!(owner.0.wait().unwrap().success());
    let result=invoke();assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stdout).contains("Migrated 1 legacy records"));
    let again=invoke();assert!(again.status.success(),"{}",String::from_utf8_lossy(&again.stderr));
    assert!(String::from_utf8_lossy(&again.stdout).contains("Migrated 0 legacy records"));
    assert_eq!(fs::read_to_string(install.join("taskq.log")).unwrap(),format!("{line}\n"));
    let reader=coreq::telemetry::Reader::open(&path).unwrap();assert!(reader.import_cursor("taskq.legacy").unwrap().is_some());
    let page=reader.export_kinds(&["operation"],0,0,1024).unwrap();assert_eq!(page["events"].as_array().unwrap().len(),1);
    drop(reader);fs::remove_dir_all(root).unwrap();
}

// Chain (#1294): a running 4.6.19 writer with another IPC budget -> new release selftest and
// receipt-bearing focus command still complete, so update and service install can hand over.
#[test]
fn incompatible_running_writer_hands_over_selftest_and_receipts() {
    use std::io::BufRead;
    let root=std::env::temp_dir().join(format!("qw-{}",std::process::id()));
    if root.exists() {fs::remove_dir_all(&root).unwrap();}
    let install=root.join("i");fs::create_dir_all(&install).unwrap();fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(root.join(".taskq/focus"),"off").unwrap();fs::write(root.join(".taskq/focus.paused"),"all").unwrap();
    fs::write(install.join("hold"),"owner hold").unwrap();
    struct Owner(std::process::Child);
    impl Drop for Owner {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
    let mut owner=Owner(Command::new(std::env::current_exe().unwrap())
        .args(["--exact","telemetry_process::resident_analysis_owner","--ignored","--nocapture"])
        .env("TASKQ_ANALYSIS_OWNER",install.join("telemetry.log/universal.sqlite3")).env("TASKQ_ANALYSIS_LEGACY_BUDGET","1")
        .env("COREQ_TELEMETRY_STORE_TRACE","1")
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap());
    let mut ready=std::io::BufReader::new(owner.0.stdout.take().unwrap());
    loop {let mut line=String::new();assert_ne!(ready.read_line(&mut line).unwrap(),0,"collector stopped before ready");if line.trim()=="ANALYSIS_READY" {break;}}
    let invoke=|command:&[&str]|Command::new(env!("CARGO_BIN_EXE_taskq")).args(command)
        .current_dir(&root).env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install)
        .env("COREQ_TELEMETRY_STORE_TRACE","1").env("TASKQ_SELFTEST_OFFLINE","1").env("TASKQ_INCIDENT_REPORTS","off").output().unwrap();
    let selftest=invoke(&["selftest"]);
    use std::io::Write;
    writeln!(std::io::stderr(),"handover selftest diagnostics: {}",String::from_utf8_lossy(&selftest.stderr)).unwrap();
    assert!(selftest.status.success(),"candidate selftest failed behind an incompatible writer: {}",String::from_utf8_lossy(&selftest.stderr));
    let resumed=invoke(&["service","resume"]);
    assert!(resumed.status.success(),"receipt unavailable behind an incompatible writer: {}",String::from_utf8_lossy(&resumed.stderr));
    assert_eq!(fs::read_to_string(root.join(".taskq/focus")).unwrap(),"all");
    assert_eq!(fs::read_to_string(install.join("hold")).unwrap(),"owner hold");
    // A1: exercise the real updater, checksum, candidate subprocess selftest and activation.
    // Only the release transport is substituted; no native service or production installation is touched.
    let assets=root.join("assets");let bin=root.join("bin");fs::create_dir_all(&assets).unwrap();fs::create_dir_all(&bin).unwrap();
    let helper=root.join("download.rs");
    fs::write(&helper,r#"fn main() {
        let args:Vec<_>=std::env::args().collect();
        assert_eq!(&args[1..5], &["-fsSL", "--max-time", "300", "-o"]);
        let name=args[6].rsplit('/').next().unwrap();
        assert!(!name.contains('\\') && name!=".." && !name.is_empty());
        let assets=std::path::PathBuf::from(std::env::var_os("TASKQ_FIXTURE_ASSETS").unwrap());
        std::fs::copy(assets.join(name),&args[5]).unwrap();
        std::fs::write(assets.join(format!("{name}.served")),b"fixture").unwrap();
    }"#).unwrap();
    let compiled=Command::new("rustc").arg(&helper).arg("-o").arg(bin.join(format!("curl{}",std::env::consts::EXE_SUFFIX))).output().unwrap();
    assert!(compiled.status.success(),"{}",String::from_utf8_lossy(&compiled.stderr));
    let version=env!("CARGO_PKG_VERSION");let target=coreq::update::target();
    let name=format!("taskq-{version}-{target}{}",std::env::consts::EXE_SUFFIX);
    let bytes=fs::read(env!("CARGO_BIN_EXE_taskq")).unwrap();fs::write(assets.join(&name),&bytes).unwrap();
    fs::write(assets.join("latest.json"),serde_json::json!({"version":version,"files":{target:name}}).to_string()).unwrap();
    fs::write(install.join("current"),"4.6.19").unwrap();
    // Windows searches the application directory before inherited PATH and can otherwise run system curl.
    // Keep the actual updater beside the owned transport, without changing product download resolution.
    let updater=bin.join(format!("taskq{}",std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_taskq"),&updater).unwrap();
    let path=std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let update=||Command::new(&updater).arg("update").current_dir(&root)
        .env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",&install).env("TASKQ_FIXTURE_ASSETS",&assets).env("PATH",&path)
        .env("TASKQ_INCIDENT_REPORTS","off").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_TASK").env_remove("COREQ_TRACE_CONTEXT").output().unwrap();
    fs::write(assets.join("SHA256SUMS"),format!("{}  {name}\n","0".repeat(64))).unwrap();
    let rejected=update();
    assert!(assets.join("SHA256SUMS.served").is_file()&&assets.join(format!("{name}.served")).is_file(),
        "update bypassed the fixture transport: {}",String::from_utf8_lossy(&rejected.stderr));
    assert!(!rejected.status.success(),"invalid checksum must not activate");
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("not in SHA256SUMS"),"wrong rejection: {}",String::from_utf8_lossy(&rejected.stderr));
    assert_eq!(fs::read_to_string(install.join("current")).unwrap(),"4.6.19");
    fs::write(assets.join("SHA256SUMS"),format!("{}  {name}\n",ring::digest::digest(&ring::digest::SHA256,&bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>())).unwrap();
    let updated=update();assert!(updated.status.success(),"update failed: {}",String::from_utf8_lossy(&updated.stderr));
    assert_eq!(fs::read_to_string(install.join("current")).unwrap().trim(),version);
    assert_eq!(fs::read_to_string(install.join("previous")).unwrap().trim(),"4.6.19");
    let activated=install.join("releases").join(version).join(format!("taskq{}",std::env::consts::EXE_SUFFIX));
    assert!(activated.is_file());
    // macOS may re-sign the verified candidate with the owner's local signing identity (§ 8).
    #[cfg(not(target_os="macos"))]
    assert!(fs::read(&activated).unwrap()==bytes,"activated a different artifact");
    assert!(!install.join("telemetry.log/handover").exists());
    drop(owner.0.stdin.take());assert!(owner.0.wait().unwrap().success());
    // Shared export must retain the actual command receipts; no side store may bypass its bound.
    assert!(coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap()
        .export(0,0,1024).unwrap()["events"].as_array().unwrap().iter().any(|r|r["observation"]["source"]=="command"));
    fs::remove_dir_all(root).unwrap();
}
