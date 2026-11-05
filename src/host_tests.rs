    use super::*;

    /// The real host loop of an empty fixture install in a child of this test binary. Release transport is refused at
    /// a dead proxy and worker identity is removed, as in `tests/host_restart.rs`; no native unit exists there.
    struct RecordsHost {base:PathBuf,install:PathBuf,log:PathBuf,child:std::process::Child}
    impl RecordsHost {
        fn start(name:&str,end:&str)->Self {
            let base=std::env::temp_dir().join(format!("taskq-{name}-{}",crate::runtime::uuid()));
            let install=base.join("install");fs::create_dir_all(&install).unwrap();
            fs::write(install.join("projects"),"").unwrap();fs::write(install.join("host.json"),"{}").unwrap();
            // Unix re-execs the install launcher: this one ends the new generation as Windows' restart exit does.
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                let launcher=install.join(crate::platform::tool(&install).exe_name());
                fs::write(&launcher,"#!/bin/sh\nexit 75\n").unwrap();fs::set_permissions(&launcher,fs::Permissions::from_mode(0o700)).unwrap();
            }
            let log=base.join("service.log");
            let mut command=std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact","host::tests::board_records_child","--ignored","--nocapture"])
                .env("TASKQ_RECORDS_CHILD",&install).env("TASKQ_RECORDS_END",end).current_dir(&install)
                .env("TASKQ_INCIDENT_REPORTS","off").env("CURL_HOME",&base).env("NO_PROXY","").env("no_proxy","")
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(fs::File::create(&log).unwrap());
            for key in ["TASKQ_ROOT","TASKQ_ATTEMPT","TASKQ_TASK","TASKQ_ROLE","COREQ_TRACE_CONTEXT"] {command.env_remove(key);}
            for key in ["HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","http_proxy","https_proxy","all_proxy"] {command.env(key,"http://127.0.0.1:9");}
            let child=command.spawn().unwrap();
            Self {base,install,log,child}
        }
        fn service_row(&self)->String {
            crate::profile::report(&self.install).lines().find(|line|line.starts_with("| service |")).unwrap_or_default().to_string()
        }
        fn log(&self)->String {fs::read_to_string(&self.log).unwrap_or_default()}
    }
    impl Drop for RecordsHost {
        fn drop(&mut self) {
            let _=self.child.kill();let _=self.child.wait();
            let _=fs::remove_dir_all(&self.base);
        }
    }

    // Chain: §4 Board request records -> host loop restart -> telemetry -> `taskq profile`. Defect #2367: the service
    // recorded its counts only each hour, so a restart lost every count of the hour and the table showed none. Risk
    // kept: a handover loses counts, or records them twice (exact sums).
    #[test]
    fn restart_keeps_the_board_requests_counted_before_it() {
        let mut host=RecordsHost::start("records-restart","restart");
        let until=Instant::now()+std::time::Duration::from_secs(60);
        let status=loop {
            if let Some(status)=host.child.try_wait().unwrap() {break status;}
            assert!(Instant::now()<until,"restart deadline: {}",host.log());
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(status.code(),Some(75),"{}",host.log());
        let row=host.service_row();
        assert!(row.starts_with("| service | ")&&row.contains(" | 5 | GET issues/N 3, GET issues 2 |"),"{row:?}: {}",host.log());
    }

    // Chain: §4 Board request records -> periodic record -> kill -> `taskq profile`. Defect #2367: a killed service lost
    // every count since its last hour. Risk kept: the host loop stops recording on its cadence, so a kill loses more
    // than the bound. `RECORD_EVERY` is short under test; its release value is checked in `requests`.
    #[test]
    fn killed_service_keeps_the_board_requests_of_its_last_record() {
        let mut host=RecordsHost::start("records-kill","kill");
        let until=Instant::now()+std::time::Duration::from_secs(60);
        while !host.service_row().contains(" | 3 | GET issues/N 3 |") {
            assert!(host.child.try_wait().unwrap().is_none(),"child ended: {}",host.log());
            assert!(Instant::now()<until,"no periodic record: {:?}: {}",host.service_row(),host.log());
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        host.child.kill().unwrap();host.child.wait().unwrap();
        assert!(host.service_row().contains(" | 3 | GET issues/N 3 |"),"{:?}",host.service_row());
    }

    #[test]
    #[ignore = "owned subprocess that runs the host loop"]
    fn board_records_child() {
        let Some(install)=std::env::var_os("TASKQ_RECORDS_CHILD").map(PathBuf::from) else {return};
        let end=std::env::var("TASKQ_RECORDS_END").unwrap();
        let worker=install.clone();
        std::thread::spawn(move || {
            for _ in 0..3 {crate::requests::tally("GET issues/N","api.github.com");}
            if end!="restart" {return;}
            for _ in 0..2 {crate::requests::tally("GET issues","api.github.com");}
            // The published service answers the review; a real `service restart` submits the same request.
            let until=Instant::now()+std::time::Duration::from_secs(30);
            let reviewed=loop {
                match crate::lifecycle::review(&worker,"records-restart") {
                    Ok(reviewed)=>break reviewed,
                    Err(e)=>assert!(Instant::now()<until,"{e}"),
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            };
            crate::lifecycle::submit(&worker,&reviewed).unwrap();
        });
        run(&install).unwrap();
    }

    // #1416: project `taskq` logs into the service's stderr file; a host line reaches every log once.
    #[test]
    fn host_line_written_once_per_log() {
        let install=std::env::temp_dir().join(format!("taskq-log-once-{}",crate::runtime::uuid()));fs::create_dir(&install).unwrap();
        let roots=[install.join("p/taskq"),install.join("p/other")];
        let stderr=install.join("taskq.log");
        fs::write(&stderr,"x update: up to date\n").unwrap();
        mirror(&install,&roots,"update: up to date",Some(&stderr));
        for f in ["taskq.log","other.log"] {assert_eq!(fs::read_to_string(install.join(f)).unwrap().matches("update: up to date").count(),1,"{f}");}
        mirror(&install,&roots,"next",None);
        assert_eq!(fs::read_to_string(&stderr).unwrap().matches("next").count(),1);
        fs::remove_dir_all(&install).unwrap();
    }

    // Defect #1419: the failed 4.6.26 selftest line was logged every pass.
    #[test]
    fn failed_selftest_logs_once_per_version() {
        let mut last=String::new();
        let fail=|v:&str,n:u8|format!("maintenance: {v} selftest failed: Error: telemetry: FAIL telemetry flush unacknowledged (accepted {n} inflight {n})");
        let passes=[fail("4.6.26",3),fail("4.6.26",4),fail("4.6.27",3),"update: up to date".into(),"update: up to date".into()];
        let logged:Vec<_>=passes.iter().filter(|text|maintenance_once(&mut last,text)).map(String::as_str).collect();
        assert_eq!(logged,[fail("4.6.26",3).as_str(),&fail("4.6.27",3),"update: up to date"]);
    }

    // A stable owner identity must not cache policy changes or hide an interrupted write.
    #[test]
    fn owner_identity_keeps_policy_readback_fresh() {
        let root=std::env::temp_dir().join(format!("taskq-policy-reload-{}",crate::runtime::uuid()));fs::create_dir(&root).unwrap();
        let value=json!({"hosts":{"owner-identity":"mapped-owner"},"limits":{"codex":2}});
        fs::write(root.join("host.json"),value.to_string()).unwrap();
        let first=Policy::read_for_hostname(&root,||"owner-identity".into()).unwrap();
        if std::env::var("TASKQ_HOST").ok().filter(|s|!s.is_empty()).is_none() {assert_eq!(first.me,"mapped-owner");}
        let mut changed=value;changed["limits"]["codex"]=json!(0);fs::write(root.join("host.json"),changed.to_string()).unwrap();
        assert_eq!(Policy::read_for_hostname(&root,||"owner-identity".into()).unwrap().cap.limits["codex"],0);
        fs::write(root.join("host.json.pending"),"retained").unwrap();
        assert!(Policy::read_for_hostname(&root,||panic!("identity ran before pending-write refusal")).unwrap_err().contains("interrupted"));
        fs::remove_file(root.join("host.json.pending")).unwrap();
        fs::write(root.join("host.json"),"{").unwrap();assert!(Policy::read_for_hostname(&root,||"owner-identity".into()).is_err());
        fs::remove_file(root.join("host.json")).unwrap();assert!(Policy::read_for_hostname(&root,||"owner-identity".into()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    // A5: a released admission hold must wake a project sleeping on its previous blocked pass, without editing focus.
    #[test]
    fn released_admission_hold_wakes_pending_work() {
        let root=temp("inventory-wake");let roots=vec![root.clone()];
        fs::create_dir_all(crate::dot(&root)).unwrap();
        fs::write(crate::dot(&root).join("focus"),"approved-label").unwrap();
        let paths=[crate::dot(&root).join("poke"),root.join("host-poke")];
        let baseline=crate::poke_times(&paths);
        let p=policy();let mut h=Host::new(Err("inventory pending".into()));
        for barrier in [Err::<(),String>("handoff: legacy service still running".into()),Err("legacy liveness unknown".into())] {
            h.reload_admission(barrier.clone().map(|_|p.clone()),barrier.err(),&roots);
            assert!(h.reserve(&root,1,1,"codex",&p.cap,true,0).is_err());
            assert_eq!(crate::poke_times(&paths),baseline,"blocked refresh cannot wake admission");
        }
        h.reload_admission(Ok(p.clone()),None,&roots);
        assert_ne!(crate::poke_times(&paths),baseline,"released hold must wake pending review/start");
        crate::sleep_until_poke(&paths,baseline,Instant::now()+std::time::Duration::from_secs(60),&mut ||panic!("wake was lost"));
        assert!(h.reserve(&root,1,1,"codex",&p.cap,true,0).is_ok());
        assert_eq!(fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"approved-label");
        fs::remove_dir_all(root).unwrap();
    }

    // An admission hold keeps valid maintenance intervals; unknown configuration keeps safe defaults.
    #[test]
    fn admission_hold_keeps_maintenance_intervals() {
        let mut configured=policy();configured.update_every=86400;configured.rollback_after=1200;
        let (blocked,timers)=guarded_policy(Ok(configured),&Err("handoff: legacy service still running".into()));
        assert!(blocked.unwrap_err().contains("still running"),"a live root owner still blocks admission");
        assert_eq!(timers,(86400,1200),"valid maintenance intervals survive an admission hold");
        let (invalid,timers)=guarded_policy(Err("host.json invalid".into()),&Ok(()));
        assert!(invalid.is_err());assert_eq!(timers,(60,600),"unknown configuration retains safe maintenance defaults");
    }

    // H-A4/H-A5: first launch waits for every project's occupancy, including unknown native ownership.
    #[test]
    fn cold_host_accounts_every_project_before_admission() {
        let p=policy();let mut h=Host::new(Ok(p.clone()));h.gate.configure(p.cap.gate);
        let a=PathBuf::from("a");let b=PathBuf::from("b");h.busy.insert(a.clone(),true);h.busy.insert(b.clone(),true);h.rebuilding=true;
        assert!(h.reserve(&a,1,0,"codex",&p.cap,true,0).is_err());
        assert!(!h.rebuild_slots(&a,vec![(1,7,"codex".into())]));
        assert_eq!(h.remaining(&a,"codex",&p.cap),0);assert!(h.reserve(&a,2,0,"codex",&p.cap,true,0).is_err());
        assert!(h.rebuild_slots(&b,vec![(2,8,"codex".into())]));assert!(!h.rebuilding);
        assert_eq!(h.used()["codex"],2);assert!(h.reserve(&a,3,0,"codex",&p.cap,true,0).is_err());
        h.sync(&b,vec![]);assert!(h.reserve(&a,3,0,"codex",&p.cap,true,0).is_ok());

        h.sync(&a,vec![(1,7,"codex".into())]);
        let c=PathBuf::from("later");h.attach(&c);
        assert_eq!(h.reserve(&a,4,0,"codex",&p.cap,true,30).unwrap_err(),"host occupancy recovery pending","registration may carry unresolved prior occupancy");
        assert!(!h.rebuild_slots(&a,vec![(1,7,"codex".into())]));
        assert!(h.rebuild_slots(&c,vec![(4,9,"codex".into())]));
        assert_eq!(h.used()["codex"],2);
        h.sync(&c,vec![]);h.busy.remove(&c);h.attach(&c);
        assert!(!h.rebuild_slots(&a,vec![(1,7,"codex".into())]),"re-registration cannot reuse the earlier root's recovery acknowledgment");
        assert!(h.rebuild_slots(&c,vec![]));assert!(!h.rebuilding);
    }

    // H-A5: the final handover barrier covers orphaned slots and closes before re-entry.
    #[test]
    fn handover_preparation_latches_only_after_every_owned_project() {
        let a=PathBuf::from("a");let b=PathBuf::from("b");
        let mut h=Host::new(Err("fixture".into()));
        assert!(h.begin_pass(&a));h.restart=true;h.end_pass(&a,true);
        h.sync(&b,vec![(1,7,"codex".into())]);
        assert!(!h.handoff_ready(),"an unrepresented slot is not an idle project");
        assert!(h.begin_pass(&a));assert!(!h.handoff_ready(),"a pass in flight holds the handover");
        h.end_pass(&a,true);assert!(h.begin_pass(&b));
        h.end_pass(&b,true);
        h.held=Some("registry unknown".into());assert!(!h.handoff_ready());
        h.held=None;assert!(h.handoff_ready());
        for _ in 0..3 {
            assert!(!h.begin_pass(&a));assert!(!h.begin_pass(&b));assert!(h.handoff_ready());
        }
        h.prepared.remove(&b);assert!(!h.handoff_ready(),"failed ownership cannot keep an old acknowledgment");
        assert!(h.begin_pass(&a));assert!(!h.handoff_ready());
        h.end_pass(&a,true);h.end_pass(&b,true);
        let (send,receive)=std::sync::mpsc::channel();
        h.updating=Some(std::thread::spawn(move || {receive.recv().map_err(|e|e.to_string())?;Ok(Maintenance::Update(None))}));
        assert!(!h.handoff_ready(),"an update owner must finish before image replacement");
        assert!(h.begin_pass(&a),"projects keep heartbeat/native monitoring while the update waits");
        h.end_pass(&a,true);send.send(()).unwrap();h.updating.take().unwrap().join().unwrap().unwrap();assert!(h.handoff_ready());
        h.stopping=true;
        assert!(!h.handoff_ready(),"Stop cannot reuse a CLI handover acknowledgment while a native slot remains");
        h.sync(&b,vec![]);assert!(!h.handoff_ready(),"Stop still waits for active project/operator ownership");
        for busy in h.busy.values_mut() {*busy=false;}
        assert!(h.handoff_ready(),"only the prepared host with no occupied work may stop");

    }
    // #1402: six projects whose passes overlap. Revoking preparation at each pass start left no moment with all
    // six prepared, so the host kept the old image. Risk: the handover waits on passes forever, or re-execs mid-pass.
    #[test]
    fn overlapping_passes_reach_the_handover() {
        let roots:Vec<_>=(0..6).map(|n|PathBuf::from(format!("project-{n}"))).collect();
        let mut h=Host::new(Err("fixture".into()));h.restart=true;
        for root in &roots {h.busy.insert(root.clone(),true);}
        for root in &roots {assert!(h.begin_pass(root));}
        // Staggered: each project ends a pass prepared and, before the others end, would start its next one.
        for (n,root) in roots.iter().enumerate() {
            h.end_pass(root,true);
            assert!(!h.handoff_ready(),"no re-exec while a pass is in flight");
            if n+1<roots.len() {assert!(h.begin_pass(root),"a project keeps passing while another has not reported");}
        }
        for root in &roots[..5] {
            assert_eq!(h.handoff_wait(),Some(crate::lifecycle::HandoffWait::Project));
            assert!(!h.begin_pass(&roots[5]),"every report prepared: no new pass starts");
            h.end_pass(root,true);assert!(!h.begin_pass(root));
        }
        assert!(h.handoff_ready());
        let mut h=Host::new(Err("fixture".into()));h.restart=true;
        for root in &roots {h.busy.insert(root.clone(),true);}
        for root in &roots {assert!(h.begin_pass(root));h.end_pass(root,true);}
        assert!(h.handoff_ready());
        h.prepared.remove(&roots[1]);assert!(h.begin_pass(&roots[0]),"an unprepared report lifts the barrier");
        assert!(h.begin_pass(&roots[0]),"a pass that never reported");assert!(!h.prepared.contains(&roots[0]),"leaves no acknowledgment");
        h.end_pass(&roots[0],true);assert!(h.begin_pass(&roots[1]));h.end_pass(&roots[1],false);
        assert!(!h.handoff_ready(),"an unprepared pass result keeps the old image");
    }

    // H-A3: idle projects must not revoke Stop preparation while another project is still draining.
    #[test]
    fn six_project_stop_retains_idle_preparation() {
        let roots:Vec<_>=(0..6).map(|n|PathBuf::from(format!("project-{n}"))).collect();
        let mut h=Host::new(Err("fixture".into()));h.restart=true;h.stopping=true;
        for root in &roots {h.busy.insert(root.clone(),true);}
        h.prepared.insert(roots[0].clone());h.sync(&roots[0],vec![(1,7,"codex".into())]);h.busy.insert(roots[0].clone(),false);
        assert!(h.begin_pass(&roots[0]),"an owned execution must continue to be monitored");
        h.sync(&roots[0],vec![]);h.end_pass(&roots[0],true);
        assert!(h.begin_pass(&roots[0]),"an active operator/project cannot freeze");
        for (n,root) in roots.iter().enumerate() {
            assert!(h.begin_pass(root));h.busy.insert(root.clone(),false);h.end_pass(root,true);
            if n+1<roots.len() {
                assert!(!h.handoff_ready());
                for ready in &roots[..=n] {assert!(!h.begin_pass(ready),"a drained project cannot reopen the Stop barrier");}
            }
        }
        assert!(h.handoff_ready());
    }

    // #1357 A4: live work elsewhere defers the handover. The host loop poked every project each second, so every
    // project listed the board each second (about 20k requests/h for six projects). The list stays at one per poll.
    #[test]
    fn deferred_handover_lists_the_board_once_per_poll() {
        use std::sync::atomic::{AtomicUsize,Ordering};
        let base=std::env::temp_dir().join(format!("taskq-deferred-handover-{}",crate::runtime::uuid()));
        let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(&root).unwrap();
        fs::write(install.join("token"),"owned-fixture-not-a-credential").unwrap();
        let root=root.canonicalize().unwrap();
        let shared=Arc::new(Mutex::new(Host::new(Err("fixture".into()))));
        {let mut h=shared.lock().unwrap();h.attach(&root);h.restart=true;h.sync(&base.join("busy"),vec![(1,7,"codex".into())]);}
        let lists=Arc::new(AtomicUsize::new(0));
        let project={let (root,install,host,lists)=(root.clone(),install.clone(),shared.clone(),lists.clone());std::thread::spawn(move || {
            enter(&root,&install);
            let mut cfg=crate::tests::cfg();cfg.root=root;cfg.install=install;cfg.poll=2;
            let board=crate::board::Fake::default();board.state.borrow_mut().lists=Some(lists);
            crate::board::Board::create(&board,"Waits for the handover","",&["taskq".into()])?;
            crate::service_run(&crate::Ctx {cfg,raw:json!({}),board:Box::new(board)},Some(host))
        })};
        // #1871: the first pass may run slow before its list (macOS gui probe: `log show`, `security`), so the 5 s
        // window starts at the first list. Retained protection: more than one list per poll, or a ready handover, fails.
        let mut until=None;
        let first=Instant::now()+std::time::Duration::from_secs(120);
        while until.is_none_or(|u|Instant::now()<u) {
            if shared.lock().unwrap().restart_wake() {crate::poke(&root).unwrap();}
            assert!(!shared.lock().unwrap().handoff_ready(),"the unrepresented slot defers the handover");
            if project.is_finished() {panic!("project loop ended: {:?}",project.join().unwrap());}
            if until.is_none() && lists.load(Ordering::SeqCst)>0 {until=Some(Instant::now()+std::time::Duration::from_secs(5));}
            assert!(until.is_some() || Instant::now()<first,"no board list within 120 s");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let listed=lists.load(Ordering::SeqCst);
        assert!((1..=4).contains(&listed),"5 s after the first list at poll 2 allows one wake and two polls; listed {listed}");
        shared.lock().unwrap().draining.insert(root.clone());crate::poke(&root).unwrap();
        assert_eq!(project.join().unwrap().unwrap_err(),"project drained");
        let _=fs::remove_dir_all(base);
    }

    // #1297 A1/A2: the defect was one detailed row per identity and status poll (63 MB/h live against 0.56 MB/h).
    // #1548: the tray tick's push, render and fit resize still stored two rows each (7 MB/h live); the tray tick fixture adds them.
    // Retained protection: six projects' hour of those polls through the bounded store stays under 384 MiB/720
    // inside repeating roots, the unmarked control fails the same gate, and no event is lost.
    // Span rows use the real clock, so the hour spans one or two of its four 15-minute windows: the gate scales the
    // measured growth to four windows. Health rows take the simulated clock (#1297 review: they materialized service.loop).
    // ponytail: root names copy main/tray.
    #[test]
    fn six_project_poll_hour_fits_store_budget() {
        use tracing_subscriber::prelude::*;
        let me=coreq::process::identity(std::process::id()).unwrap().unwrap();
        for folded in [false,true] {
            let install=temp(&format!("poll-hour-{folded}"));let _=fs::remove_dir_all(&install);fs::create_dir_all(install.join("telemetry.log")).unwrap();
            publish_service(&install,&me,"fixture").unwrap();
            let path=install.join("telemetry.log/universal.sqlite3");let budget=crate::telemetry::budget(&install,"telemetry.sqlite3").unwrap();
            let store=std::sync::Arc::new(coreq::telemetry::Store::open_bounded(&path,budget.clone()).unwrap());
            store.reclaim().unwrap();let empty=store.storage().unwrap()["total_bytes"].as_u64().unwrap();
            let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","test").unwrap();
            let dispatch=tracing::Dispatch::new(tracing_subscriber::registry().with(capture.clone()));
            let window=crate::telemetry::diagnostic::HEALTH_WINDOW_MS;
            let (first,simulated)=(crate::profile::now_ms().unwrap(),crate::profile::now_ms().unwrap()/window*window);
            // The tray module builds only on macOS and Windows; Linux hosts run no tray, so their hour has no tick rows.
            #[cfg(any(target_os = "macos", windows))]
            let mut tray=crate::tray::tests::steady_tick();
            #[cfg(not(any(target_os = "macos", windows)))]
            let mut tray=||();
            tracing::dispatcher::with_default(&dispatch,|| for tick in 0..1800 {
                if tick%30==0 {
                    // The hour runs in seconds: let the writer catch up each simulated minute, as real pacing would, so a
                    // loaded runner never meets CoreQ's 8192-event queue cap (merge_group flake on Windows).
                    let until=std::time::Instant::now()+std::time::Duration::from_secs(60);
                    while capture.health()["queued_events"].as_u64().unwrap()>1024 {
                        assert!(std::time::Instant::now()<until,"writer stalled: {}",capture.health());std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    for project in 0..6 {
                        let project=format!("owner/project{project}");
                        let _repeating=tracing::info_span!("service.loop",coreq_repeating=folded,project=project.as_str()).entered();
                        assert!(!crate::hostname().is_empty());assert_eq!(service_status(&install)["state"],"running");
                        assert_eq!(crate::telemetry::diagnostic::health_at(Some(simulated+tick*2000))["lost"],0);
                    }
                }
                let _repeating=tracing::info_span!("tray.refresh",coreq_repeating=folded).entered();
                assert!(!crate::hostname().is_empty());assert_eq!(service_status(&install)["state"],"running");
                tray();
            });
            // #1339: folded aggregates persist only through finish() or the last Capture drop; another thread's
            // tracing interest rebuild can briefly own a Dispatch clone, so `late` makes that drop order deterministic.
            let late=dispatch.clone();
            capture.finish().unwrap();
            let health=capture.health();
            assert_eq!((&health["lost"],&health["failed_batches"]),(&json!(0),&json!(0)),"{health}");
            drop(dispatch);drop(capture);drop(store);
            let store=coreq::telemetry::Store::open_bounded(&path,budget).unwrap();store.reclaim().unwrap();
            let windows=crate::profile::now_ms().unwrap()/window-first/window+1;
            let bytes=(store.storage().unwrap()["total_bytes"].as_u64().unwrap()-empty)*4/windows;
            let limit=384*1024*1024/720;
            eprintln!("poll hour folded={folded}: {bytes} bytes over four windows; limit={limit}");
            assert_eq!(bytes<=limit,folded,"hourly growth gate must reject unfolded polling");
            if folded {
                let page=store.export(0,0,1024).unwrap();let rows=page["events"].as_array().unwrap();
                let rows:Vec<_>=rows.iter().map(|r|&r["observation"]).collect();
                // Per window: one health row and the one service.loop pass it materializes (start and end).
                let health=rows.iter().filter(|r|r["operation"]=="collector.health").count();
                let passes=rows.iter().filter(|r|r["operation"]=="taskq.service.loop"&&r["kind"]=="method").count() as u64;
                assert!((1..=4).contains(&health)&&passes==health as u64,"{page}");
                assert_eq!(rows.iter().filter(|r|r["kind"]!="method.aggregate").count(),health*3,"{page}");
                let calls=|op:&str|rows.iter().filter(|r|r["operation"]==op).filter_map(|r|r["calls"].as_u64()).sum::<u64>();
                assert_eq!((calls("taskq.tray.refresh"),calls("taskq.service.loop")+passes),(1800,360),"aggregates keep every poll");
                let counts=|op:&str|rows.iter().filter(|r|r["operation"]==op).filter_map(|r|r["counts"]["calls"].as_u64()).sum::<u64>();
                if cfg!(any(target_os = "macos", windows)) {assert_eq!((calls("taskq.operation.tray.web.tick"),counts("js.ui.render.tick")),(1800,1800),"{page}");}
            }
            drop(late);drop(store);fs::remove_dir_all(&install).unwrap();
        }
    }

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("taskq-host-{name}-{}", std::process::id()));
        fs::create_dir_all(&p).unwrap(); p
    }
    // Chain: § 9 `claim` (#1493). Risk: a typo silently makes a labelled host claim everything.
    #[test]
    fn claim_parses_any_or_labelled_only() {
        assert_eq!(policy().claim, ClaimMode::Any, "missing claim: any");
        assert_eq!(Policy::parse(json!({"claim": "labelled"}), "invented").unwrap().claim, ClaimMode::Labelled);
        assert_eq!(Policy::parse(json!({"claim": "any"}), "invented").unwrap().claim, ClaimMode::Any);
        assert!(Policy::parse(json!({"claim": "label"}), "invented").is_err());
    }
    // #2138 A2/A3/R13: invalid new policy must hold launches; defaults and lower-only overlays remain intact.
    #[test]
    fn balancing_policy_validates_modes_and_project_execution() {
        for (alias,mode) in [("any",ClaimMode::Any),("spillover",ClaimMode::Spillover),("labelled",ClaimMode::Labelled),("none",ClaimMode::None)] {
            assert_eq!(Policy::parse(json!({"claim":alias,"spillover_grace":0}),"invented").unwrap().claim,mode);
        }
        assert_eq!(policy().spillover_grace,300);
        for invalid in [json!({"spillover_grace":-1}),json!({"spillover_grace":"5"}),json!({"claim":true}),json!({"claim":"primary"}),json!({"claim":"pinned-only"}),json!({"claim":"manager-only"})] {assert!(Policy::parse(invalid,"invented").is_err());}
        for entry in [json!({"default_runtime":"unknown"}),json!({"default_runtime":"codex-app"}),json!({"auto_reroute":1}),json!({"allowed_runtimes":null}),json!({"allowed_runtimes":["codex","codex"]}),json!({"allowed_runtimes":["unknown"]}),json!({"limits":null}),json!({"auto_reroute":false,"unexpected":true})] {
            assert!(Policy::parse(json!({"projects":{"/invented":entry}}),"invented").is_err());
        }
        for entry in [json!({"default_runtime":"codex"}),json!({"allowed_runtimes":[]}),json!({"auto_reroute":true})] {
            let p=Policy::parse(json!({"projects":{"/invented":entry}}),"invented").unwrap();
            assert!(p.overlay(Path::new("/other")).is_none());
            assert_ne!(p.project_revision(Path::new("/invented"),&json!({})).unwrap(),policy().project_revision(Path::new("/invented"),&json!({})).unwrap());
        }
    }
    // Chain: § 9 `default_runtime` (#1668). Risk: a host without the key stops running reviews on claude,
    // or a typo or `codex-app` silently becomes the runtime of every review on the host.
    #[test]
    fn default_runtime_parses_cli_subscriptions_only() {
        assert_eq!(policy().default_runtime, "claude", "missing key: claude");
        for rt in crate::runtime::CLI_RUNTIMES {
            assert_eq!(Policy::parse(json!({"default_runtime": rt}), "invented").unwrap().default_runtime, rt);
        }
        for bad in [json!("codex-app"), json!("Codex"), json!(""), json!(1)] {
            assert_eq!(Policy::parse(json!({"default_runtime": bad}), "invented").unwrap_err(), "host.json: default_runtime must be claude or codex or antigravity");
        }
    }
    fn policy() -> Policy {
        Policy::parse(json!({"limits": {"codex": 2}, "admission": {"max": 2}, "load_gate": {"enabled": false}}), "invented").unwrap()
    }

    // H-A5: native owner + exact host generation acknowledge install; project PID or freshness alone cannot.
    #[test]
    fn installation_observed_requires_current_native_owner_and_project_generation() {
        let service=json!({"state":"running","fresh":true,"version":crate::platform::VERSION,"pid":123,"instance":"owned-host"});
        let project=json!({"pid":123,"host_instance":"owned-host","complete":true,"tasks":[]});
        assert!(installation_observed(&service,Some(&project),crate::platform::VERSION));
        for (key,value) in [("state",json!("unknown")),("fresh",json!(false)),("version",json!("older")),("pid",json!(0)),("instance",json!(""))] {
            let mut other=service.clone();other[key]=value;assert!(!installation_observed(&other,Some(&project),crate::platform::VERSION),"{key}");
        }
        for other in [None,Some(json!({"pid":124,"host_instance":"owned-host"})),Some(json!({"pid":123,"host_instance":"prior-host"}))] {
            assert!(!installation_observed(&service,other.as_ref(),crate::platform::VERSION));
        }
    }

    // H-A5: refusal precedes native installation, focus changes, release replacement and registry publication.
    #[test]
    fn host_install_refuses_another_live_root_owner_without_effects() {
        let base=temp("install-legacy-owner");let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(crate::dot(&root)).unwrap();
        fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
        fs::write(crate::dot(&root).join("focus"),"retained-focus").unwrap();
        let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::stop_child","--ignored"])
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        fs::write(crate::dot(&root).join("status.json"),json!({"pid":child.id(),"kids":[{"task":7}]}).to_string()).unwrap();
        let before=fs::read(install.join("projects")).unwrap();
        let result=installation::install_bound(&install,&root,None);
        child.kill().unwrap();child.wait().unwrap();
        assert!(result.unwrap_err().contains("another live service owns a registered root"));
        assert_eq!(fs::read(install.join("projects")).unwrap(),before);
        assert_eq!(fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"retained-focus");
        assert!(!install.join("releases").exists()&&!install.join("poke").exists());
        fs::write(install.join("service"),"{broken").unwrap();
        assert!(installation::install_bound(&install,&root,None).unwrap_err().contains("malformed"));
        assert_eq!(fs::read(install.join("projects")).unwrap(),before);
        fs::remove_dir_all(base).unwrap();
    }

    // H-A5: one editor preserves unrelated roots/permissions, rejects interrupted state and removes canonical aliases.
    #[test]
    fn registry_editor_preserves_roots_and_refuses_unknown() {
        let base=temp("registry-editor");let install=base.join("install");let a=base.join("a");let b=base.join("b");
        for path in [&install,&a,&b] {fs::create_dir_all(path).unwrap();}
        assert!(ProjectEdit::new(&install,&a,false).is_err(),"only registration initializes a missing registry");
        ProjectEdit::new(&install,&a,true).unwrap().commit().unwrap();
        let path=install.join("projects");
        let before=fs::read_to_string(&path).unwrap();
        assert_eq!(before,format!("{}\n",crate::platform::plain_path(&a.canonicalize().unwrap()).display()));
        assert!(install.join("poke").exists());
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&path,fs::Permissions::from_mode(0o640)).unwrap();}
        let owner=ownership(&path).unwrap();
        ProjectEdit::new(&install,&b,true).unwrap().commit().unwrap();
        assert_eq!(ownership(&path).unwrap(),owner);
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;assert_eq!(fs::metadata(&path).unwrap().permissions().mode()&0o777,0o640);}
        let saved=fs::read(&path).unwrap();
        let mut registry=Registry::default();registry.refresh(&install,0);
        let roots=registry.roots.clone();
        fs::write(install.join("projects.pending"),"interrupted").unwrap();
        registry.refresh(&install,60);
        assert!(registry.error.is_some() && registry.roots==roots);
        assert!(ProjectEdit::new(&install,&a,false).is_err());
        assert_eq!(fs::read(&path).unwrap(),saved);
        fs::remove_file(install.join("projects.pending")).unwrap();
        let alias=a.join(".");
        fs::write(&path,format!("{}{}\n{}\n",String::from_utf8(saved).unwrap(),alias.display(),b.display())).unwrap();
        ProjectEdit::new(&install,&a,false).unwrap().commit().unwrap();
        assert_eq!(projects(&install).unwrap(),vec![b.canonicalize().unwrap()]);
        assert_eq!(project_paths(&install).unwrap().len(),2,"unrelated configured spelling is retained");
        fs::write(&path,b"\xff").unwrap();
        assert!(ProjectEdit::new(&install,&a,true).is_err());
        assert_eq!(fs::read(&path).unwrap(),b"\xff");
        fs::remove_dir_all(base).unwrap();
    }

    // Risk (#1858): Windows registration stored `\\?\C:\...` beside older plain entries, so path and trust checks missed.
    #[cfg(windows)]
    #[test]
    fn windows_registry_stores_plain_roots_and_matches_verbatim_entries() {
        let base=temp("registry-plain");let install=base.join("install");let a=base.join("a");let b=base.join("b");
        for path in [&install,&a,&b] {fs::create_dir_all(path).unwrap();}
        let verbatim=|p:&Path|{let c=p.canonicalize().unwrap();assert!(c.to_str().unwrap().starts_with(r"\\?\"));c};
        let plain=|p:&Path|crate::platform::plain_path(&verbatim(p));
        ProjectEdit::new(&install,&verbatim(&a),true).unwrap().commit().unwrap();
        let path=install.join("projects");
        assert_eq!(fs::read_to_string(&path).unwrap(),format!("{}\n",plain(&a).display()),"stored without the verbatim prefix");
        // an older prefixed entry and a plain one of the same root resolve to one project, and re-adding writes nothing
        fs::write(&path,format!("{}\n{}\n{}\n",verbatim(&a).display(),plain(&a).display(),plain(&b).display())).unwrap();
        assert_eq!(projects(&install).unwrap(),vec![verbatim(&a),verbatim(&b)]);
        let before=fs::read(&path).unwrap();
        ProjectEdit::new(&install,&plain(&a),true).unwrap().commit().unwrap();
        assert_eq!(fs::read(&path).unwrap(),before);
        ProjectEdit::new(&install,&plain(&a),false).unwrap().commit().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(),format!("{}\n",plain(&b).display()),"removal drops both spellings");
        fs::remove_dir_all(base).unwrap();
    }

    // H-A1: an owner before PID publication excludes a second process, survives sweep, and releases on exit.
    #[test]
    fn one_host_owner_before_heartbeat_publication() {
        let base=temp("host-owner");let install=base.join("install");fs::create_dir(&install).unwrap();
        let (first,second)=(base.join("first"),base.join("second"));
        let mut holder=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::owner_probe","--ignored"])
            .env("TASKQ_OWNER_INSTALL",&install).env("TASKQ_OWNER_MARKER",&first).env("TASKQ_OWNER_HOLD","1")
            .stdout(std::process::Stdio::null()).stderr(fs::File::create(base.join("owner.stderr")).unwrap()).spawn().unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(5);
        while !first.is_file() && holder.try_wait().unwrap().is_none() && Instant::now()<until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let entered=fs::read_to_string(&first).unwrap_or_default();
        crate::platform::sweep(&install);
        let probe=coreq::process::output("test.host-owner",std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::owner_probe","--ignored"])
            .env("TASKQ_OWNER_INSTALL",&install).env("TASKQ_OWNER_MARKER",&second).env_remove("TASKQ_OWNER_HOLD"),
            std::time::Duration::from_secs(5));
        if holder.try_wait().unwrap().is_none() {holder.kill().unwrap();holder.wait().unwrap();}
        let observed=fs::read_to_string(&second).unwrap_or_default();
        let released=owner(&install).unwrap();let freed=released.is_some();
        drop(released);
        let diagnostic=fs::read_to_string(base.join("owner.stderr")).unwrap_or_default();
        fs::remove_dir_all(base).unwrap();
        assert_eq!(entered,"claimed","first owner never reached the controlled startup boundary: {diagnostic}");
        probe.unwrap();
        assert_eq!(observed,"blocked","two processes claimed the host before its PID was published");
        assert!(freed,"native owner exit did not release ownership");
    }

    // H-A1/H-A5: exact native incarnation, launch generation and observation age have independent meanings.
    #[test]
    fn host_owner_projection_separates_identity_generation_and_freshness() {
        let install=temp("owner-observation");
        let marker=install.join("published");
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::owner_probe","--ignored"])
            .env("TASKQ_OWNER_INSTALL",&install).env("TASKQ_OWNER_MARKER",&marker).env("TASKQ_OWNER_PUBLISH","first")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(5);
        while !marker.exists()&&Instant::now()<until&&child.try_wait().unwrap().is_none() {std::thread::yield_now();}
        assert_eq!(fs::read_to_string(marker).unwrap(),"claimed");
        let native=coreq::process::identity(child.id()).unwrap().unwrap();
        let status=service_status(&install);
        assert_eq!(status["state"],"running");assert_eq!(status["fresh"],true);assert_eq!(status["instance"],"first");
        assert!(service_line(&status).contains("running (host, pid"));
        assert_eq!(other(&install).unwrap(),Some(child.id()));
        let original=fs::read_to_string(install.join("service")).unwrap();
        let mut record:Value=serde_json::from_str(&original).unwrap();
        for at in [wall().unwrap()-120,wall().unwrap()+120] {
            record["at"]=json!(at);fs::write(install.join("service"),record.to_string()).unwrap();
            let stale=service_status(&install);assert_eq!(stale["state"],"running");assert_eq!(stale["fresh"],false);
            assert!(service_line(&stale).contains("observation stale or unknown"));
            assert_eq!(other(&install).unwrap(),Some(child.id()),"stale/future projection released a live owner");
        }
        publish_service(&install,&native,"second").unwrap();
        assert_eq!(service_status(&install)["instance"],"second","same native PID may have a new host launch");
        record=serde_json::from_str(&original).unwrap();record["started"]=json!(format!("{}-older",native.started));
        fs::write(install.join("service"),record.to_string()).unwrap();
        assert_eq!(service_status(&install)["state"],"exited","another incarnation must not look like the recorded daemon");
        assert_eq!(other(&install).unwrap(),None);assert!(coreq::process::identity(child.id()).unwrap().is_some());
        assert!(owner(&install).unwrap().is_none(),"record mismatch bypassed the native ownership lock");
        record["schema"]=json!(2);
        fs::write(install.join("service"),record.to_string()).unwrap();
        assert_eq!(service_status(&install)["state"],"unknown");assert_eq!(other(&install).unwrap(),None,"another incarnation's unreadable record kept the host waiting");
        record["started"]=json!(native.started);
        for text in [record.to_string(),"{".into(),"x".repeat(1025)] {
            fs::write(install.join("service"),&text).unwrap();
            assert_eq!(service_status(&install)["state"],"unknown");assert!(other(&install).is_err());
            assert_eq!(fs::read_to_string(install.join("service")).unwrap(),text);
        }
        fs::write(install.join("service"),child.id().to_string()).unwrap();
        assert_eq!(service_status(&install)["state"],"unknown","a PID-only record cannot certify a native host owner");
        assert!(other(&install).is_err(),"a retired PID-only record reads as an unknown owner");
        publish_service(&install,&native,"second").unwrap();
        child.kill().unwrap();child.wait().unwrap();
        assert_eq!(service_status(&install)["state"],"exited");assert_eq!(other(&install).unwrap(),None);
        // #1310: a newer release's record whose process exited no longer holds an older host forever.
        let mut newer:Value=serde_json::from_str(&fs::read_to_string(install.join("service")).unwrap()).unwrap();
        newer["stop_protocol"]=json!(3);let mut extra=newer.clone();extra["future_protocol"]=json!(1);
        for text in [newer.to_string(),extra.to_string()] {
            fs::write(install.join("service"),&text).unwrap();
            assert_eq!(service_status(&install)["state"],"unknown");assert_eq!(other(&install).unwrap(),None,"exited unreadable record kept the host waiting");
            assert_eq!(fs::read_to_string(install.join("service")).unwrap(),text);
        }
        // R12: corrupt identities cannot prove exit, even when their PID has exited.
        for started in ["invalid start time!".to_owned(), "x".repeat(129), String::new()] {
            newer["started"]=json!(started);
            let text=newer.to_string();
            fs::write(install.join("service"),&text).unwrap();
            assert!(other(&install).is_err(),"corrupt identity released ownership: {started:?}");
            assert_eq!(fs::read_to_string(install.join("service")).unwrap(),text);
        }
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    #[ignore="owned native process for one_host_owner_before_heartbeat_publication"]
    fn owner_probe() {
        let install=PathBuf::from(std::env::var_os("TASKQ_OWNER_INSTALL").unwrap());
        let claim=owner(&install).unwrap();
        let marker=PathBuf::from(std::env::var_os("TASKQ_OWNER_MARKER").unwrap());
        if claim.is_some()&&let Ok(instance)=std::env::var("TASKQ_OWNER_PUBLISH") {
            use std::io::Read;
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            publish_service(&install,&native,&instance).unwrap();
            crate::platform::write_atomic(&marker,"claimed").unwrap();
            let _=std::io::stdin().read(&mut [0]); // Parent pipe closure also retires a failed fixture.
            return;
        }
        crate::platform::write_atomic(&marker,if claim.is_some(){"claimed"}else{"blocked"}).unwrap();
        if std::env::var_os("TASKQ_OWNER_HOLD").is_some() && claim.is_some() {loop {std::thread::park();}}
    }

    // H-A3: committed policy and its original command survive writer death, even before the applied receipt.
    #[test]
    fn acknowledged_policy_survives_writer_termination() {
        for phase in ["committed","before-applied"] {
        let root=temp(&format!("policy-commit-{phase}"));
        fs::write(root.join("host.json"),json!({"limits":{"codex":2},"owner_field":"preserve"}).to_string()).unwrap();
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::policy_writer","--ignored"])
            .env("TASKQ_HOST_POLICY_FIXTURE",&root).env("TASKQ_HOST_POLICY_PHASE",phase)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(10);
        while !root.join("ack").is_file() && child.try_wait().unwrap().is_none() && Instant::now()<until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let acknowledged=fs::read_to_string(root.join("ack")).unwrap_or_default();
        if child.try_wait().unwrap().is_none() {child.kill().unwrap();child.wait().unwrap();}
        assert_eq!(acknowledged,if phase=="committed" {"committed"} else {"policy-saved"},"writer did not reach {phase}");
        let read=Policy::read(&root).unwrap();assert_eq!(read.cap.limits["codex"],3);assert_eq!(read.raw["owner_field"],"preserve");
        let receipt=read.command().unwrap().unwrap();
        let before=fs::read(root.join("host.json")).unwrap();
        let (capture,guard)=crate::profile::tests::collecting(&root);
        assert!(!capacity_observed(&root,&receipt).unwrap());
        assert_eq!(observe_capacity(&root,&read).unwrap(),Some(receipt.event_id("observed").unwrap()));
        assert!(observed_soon(&root,&receipt));
        assert_eq!(fs::read(root.join("host.json")).unwrap(),before,"recovery replayed a settings mutation");
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    #[ignore="owned subprocess used by acknowledged_policy_survives_writer_termination"]
    fn policy_writer() {
        let root=PathBuf::from(std::env::var_os("TASKQ_HOST_POLICY_FIXTURE").unwrap());
        let (_capture,_guard)=crate::profile::tests::collecting(&root);
        let owner=Policy::read(&root).unwrap().me;
        if let Ok(request)=std::env::var("TASKQ_HOST_POLICY_REQUEST") {
            local_capacity(&root,&owner,&serde_json::from_str(&request).unwrap()).unwrap();
        } else {write_capacity(&root,&owner,&crate::CapEdit {limits:BTreeMap::from([("codex".into(),3)]),..Default::default()},None).unwrap();}
        crate::platform::write_atomic(&root.join("ack"),"committed").unwrap();
        loop {std::thread::park();}
    }

    fn local_fixture(name:&str)->(PathBuf,PathBuf,String,Value) {
        let base=temp(name);let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(&root).unwrap();
        fs::write(install.join("host.json"),"{\"limits\":{\"codex\":2}}").unwrap();
        fs::write(root.join("taskq.json"),"{\"repo\":\"fixture/project\"}").unwrap();
        fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
        let p=Policy::read(&install).unwrap();let now=wall().unwrap();
        let request=json!({"id":"local-capacity","issued_at":now,"expires_at":now+300,"ops":[{
            "target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":"codex"},
            "policy_schema":"taskq.host-capacity/1","expected_revision":p.revision().unwrap(),"changes":{"runtime_limit":3}}]});
        (base,install,p.me,request)
    }

    // Receipt commits run beside the operation (#1351): a reader polls within coreq's append deadline.
    fn observed_soon(root:&Path,receipt:&CommandReceipt)->bool {
        crate::telemetry::settle(std::time::Duration::from_secs(30));
        let until=Instant::now()+std::time::Duration::from_secs(15);
        while Instant::now()<until {if capacity_observed(root,receipt).unwrap() {return true;} std::thread::sleep(std::time::Duration::from_millis(20));}
        false
    }

    // #1351: a stalled telemetry writer (budget lock held) cannot delay or fail a limits save; a refused save keeps the old limits.
    #[test]
    fn local_capacity_saves_while_telemetry_is_stalled() {
        let (base,install,owner,request)=local_fixture("local-stalled");
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let budget=coreq::file_lock::lock(&install.join("taskq-profile.lock")).unwrap();
        let started=Instant::now();
        let saved=local_capacity(&install,&owner,&request).unwrap();
        assert_eq!(saved["state"],"saved","{saved}");
        assert!(started.elapsed()<std::time::Duration::from_secs(3),"save waited {:?} on telemetry",started.elapsed());
        assert_eq!(Policy::read(&install).unwrap().cap.limits["codex"],3);
        let bytes=fs::read(install.join("host.json")).unwrap();
        let mut stale=request.clone();stale["id"]=json!("stale");stale["ops"][0]["changes"]["runtime_limit"]=json!(5);
        let started=Instant::now();
        assert_eq!(local_capacity(&install,&owner,&stale).unwrap()["state"],"rejected");
        assert!(started.elapsed()<std::time::Duration::from_secs(3));
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"refused save changed policy");
        drop(budget);capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
    }

    // #1351 A3: a held host lock turns the save into a bounded "Not saved" with the old limits kept; the same request saves after release.
    #[test]
    fn local_capacity_refuses_within_bound_while_host_lock_is_held() {
        let (base,install,owner,request)=local_fixture("local-locked");
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let bytes=fs::read(install.join("host.json")).unwrap();
        fs::create_dir_all(crate::resources::dir(&install)).unwrap();
        let held=coreq::file_lock::lock(&crate::resources::dir(&install).join("lock")).unwrap();
        let started=Instant::now();
        let refused=local_capacity(&install,&owner,&request).unwrap();
        assert_eq!(refused["state"],"rejected","{refused}");
        assert!(refused["reason"].as_str().unwrap().contains("old limits kept"),"{refused}");
        assert!(started.elapsed()<LOCAL_LOCK_WAIT+std::time::Duration::from_secs(2),"save waited {:?} on the host lock",started.elapsed());
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"refused save changed policy");
        drop(held);
        assert_eq!(local_capacity(&install,&owner,&request).unwrap()["state"],"saved");
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
    }

    // H-A2: immutable same-ID replay, current revision and typed local scope hold before policy effects.
    #[test]
    fn local_capacity_request_replays_without_mutation_and_refuses_foreign_scope() {
        let (base,install,owner,request)=local_fixture("local-request");
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let saved=local_capacity(&install,&owner,&request).unwrap();assert_eq!(saved["state"],"saved");
        let bytes=fs::read(install.join("host.json")).unwrap();
        assert_eq!(local_capacity(&install,&owner,&request).unwrap(),saved);
        for change in ["payload","stale","expired","installation","generation","project","runtime","schema","type","future","lifetime","empty","too-many","extra","large"] {
            let mut other=request.clone();
            if change!="payload" {other["id"]=json!(change);}
            match change {
                "payload"=>other["ops"][0]["changes"]["runtime_limit"]=json!(4),
                "expired"=>{let now=wall().unwrap();other["issued_at"]=json!(now-301);other["expires_at"]=json!(now-1);}
                "installation"=>other["ops"][0]["target"]["installation_id"]=json!("a".repeat(64)),
                "generation"=>other["ops"][0]["target"]["generation"]=json!(2),
                "project"=>other["ops"][0]["target"]["project_id"]=json!("project"),
                "runtime"=>other["ops"][0]["target"]["runtime_id"]=json!("unknown"),
                "schema"=>other["ops"][0]["policy_schema"]=json!("taskq.capacity/1"),
                "type"=>other["ops"][0]["changes"]["runtime_limit"]=json!(true),
                "future"=>{let now=wall().unwrap();other["issued_at"]=json!(now+60);other["expires_at"]=json!(now+300);}
                "lifetime"=>other["expires_at"]=json!(request["issued_at"].as_u64().unwrap()+301),
                "empty"=>other["ops"]=json!([]),
                "too-many"=>other["ops"]=json!(vec![other["ops"][0].clone();service::RUNTIMES.len()+2]),
                "extra"=>other["set"]=json!({"codex":4}),
                "large"=>other["ops"][0]["changes"]["runtime_limit"]=json!("x".repeat(16<<10)),
                _=>{},
            }
            let result=local_capacity(&install,&owner,&other);
            assert!(result.is_err()||result.unwrap()["state"]=="rejected","{change} accepted");
            assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"{change} changed policy");
        }
        let current=Policy::read(&install).unwrap();observe_capacity(&install,&current).unwrap();
        assert_eq!(local_capacity(&install,&owner,&request).unwrap()["saved_revision"],current.revision().unwrap());
        let rev=current.revision().unwrap();
        let mut all_targets:Vec<Value>=service::RUNTIMES.iter().enumerate().map(|(i,rt)|json!({"target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":rt},"policy_schema":"taskq.host-capacity/1","expected_revision":rev,"changes":{"runtime_limit":(i+1) as u64}})).collect();
        all_targets.push(json!({"target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":null},"policy_schema":"taskq.host-capacity/1","expected_revision":rev,"changes":{"maximum":10,"load_gate":{"enabled":true,"cpu_max":85,"memory_min":15}}}));
        let mut newer=request.clone();newer["id"]=json!("newer");newer["ops"]=json!(all_targets);
        let next=local_capacity(&install,&owner,&newer).unwrap();assert_eq!(next["state"],"saved");assert_ne!(next["saved_revision"],saved["saved_revision"]);
        let bytes=fs::read(install.join("host.json")).unwrap();
        assert_eq!(local_capacity(&install,&owner,&request).unwrap(),saved,"a later command must not change the original receipt");
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes);
        crate::resources::with(&install,|state| {
            let op=state.ops["local-capacity"].clone();
            for n in state.ops.len()..crate::resources::MAX_OPS {state.ops.insert(format!("retained-{n}"),op.clone());}
            Ok(())
        }).unwrap();
        newer=request.clone();newer["id"]=json!("store-full");newer["ops"][0]["expected_revision"]=json!(Policy::read(&install).unwrap().revision().unwrap());
        assert_eq!(local_capacity(&install,&owner,&newer).unwrap()["state"],"rejected");
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes);
        fs::write(install.join("resource-state/state.json"),"{").unwrap();
        assert!(local_capacity(&install,&owner,&newer).unwrap_err().contains("file kept"));
        assert_eq!(fs::read_to_string(install.join("resource-state/state.json")).unwrap(),"{");
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes);
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
    }

    // H-A2/H-A3: crash before effect remains unknown; crash after replacement recovers the same receipt read-only.
    #[test]
    fn local_capacity_crashes_never_replay_an_uncertain_write() {
        for phase in ["before-write","before-applied"] {
            let (base,install,owner,request)=local_fixture(&format!("local-crash-{phase}"));
            let mut child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","host::tests::policy_writer","--ignored"]).env("TASKQ_HOST_POLICY_FIXTURE",&install)
                .env("TASKQ_HOST_POLICY_REQUEST",request.to_string()).env("TASKQ_HOST_POLICY_PHASE",phase)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
            let until=Instant::now()+std::time::Duration::from_secs(10);
            while !install.join("ack").exists() && child.try_wait().unwrap().is_none() && Instant::now()<until {std::thread::sleep(std::time::Duration::from_millis(5));}
            let signal=fs::read_to_string(install.join("ack")).unwrap_or_default();let _=child.kill();child.wait().unwrap();
            assert_eq!(signal,if phase=="before-write" {"intent-saved"}else{"policy-saved"});
            let bytes=fs::read(install.join("host.json")).unwrap();
            let (capture,guard)=crate::profile::tests::collecting(&install);
            let result=local_capacity(&install,&owner,&request).unwrap();
            assert_eq!(result["state"],if phase=="before-write" {"unknown"}else{"saved"},"{phase}: {result}");
            assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"recovery rewrote policy at {phase}");
            if phase=="before-applied" {
                let p=Policy::read(&install).unwrap();assert_eq!(observe_capacity(&install,&p).unwrap(),result["saved_revision"].as_str().map(String::from));
            }
            capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
        }
    }

    // H-A2: identical settings have different command identities; stale/manual readback cannot confirm a new edit.
    #[test]
    fn capacity_receipts_follow_the_loaded_command_not_equal_values() {
        let root=temp("capacity-receipt");
        let proj=root.join("proj");fs::create_dir_all(crate::dot(&proj)).unwrap();
        fs::write(root.join("projects"),format!("{}\n",proj.display())).unwrap();
        let poke_paths=[crate::dot(&proj).join("poke"),root.join("host-poke")];
        fs::write(root.join("host.json"),"{\"limits\":{\"codex\":2}}").unwrap();
        let initial=Policy::read(&root).unwrap();
        let owner=initial.me.clone();
        let mut h=Host::new(Ok(initial));
        let edit=crate::CapEdit {limits:BTreeMap::from([("codex".into(),3)]),gate:Some(false),..Default::default()};
        assert!(write_capacity(&root,&owner,&edit,None).is_err(),"missing required receipts must refuse before effect");
        assert_eq!(Policy::read(&root).unwrap().cap.limits["codex"],2);
        let (capture,guard)=crate::profile::tests::collecting(&root);
        let (_,_,first)=write_capacity(&root,&owner,&edit,None).unwrap();
        let loaded=Policy::read(&root).unwrap();
        let before_reload=crate::poke_times(&poke_paths);
        h.reload_admission(Ok(loaded.clone()),None,std::slice::from_ref(&proj));
        assert_ne!(crate::poke_times(&poke_paths),before_reload,"changed host Policy must wake registered project roots");
        assert!(!capacity_observed(&root,&first).unwrap());
        let (_,_,second)=write_capacity(&root,&owner,&edit,None).unwrap();
        let current=Policy::read(&root).unwrap();
        assert_eq!(loaded.cap,current.cap);assert_ne!(first.encode(),second.encode());
        assert!(observe_capacity(&root,&loaded).is_err(),"late old loader confirmed a different command");
        assert!(!capacity_observed(&root,&second).unwrap());
        let before_observe=crate::poke_times(&poke_paths);
        assert_eq!(observe_capacity(&root,&current).unwrap(),Some(second.event_id("observed").unwrap()));
        assert_ne!(crate::poke_times(&poke_paths),before_observe,"observed capacity command must wake registered project roots");
        assert!(observed_soon(&root,&second));
        assert!(capacity_observed(&root,&first).is_err());
        let mut edited=current.raw.clone();edited["limits"]["codex"]=json!(4);
        fs::write(root.join("host.json"),edited.to_string()).unwrap();
        assert!(observe_capacity(&root,&Policy::read(&root).unwrap()).is_err(),"manual edit borrowed old receipt");
        for invalid in [json!({"schema":2}),json!({"schema":1,"host":owner,"policy":current.cap.json(),"receipt":"{}"})] {
            edited[CAPACITY_COMMAND]=invalid;
            let bytes=edited.to_string();fs::write(root.join("host.json"),&bytes).unwrap();
            assert!(Policy::read(&root).is_err());assert!(write_capacity(&root,&owner,&edit,None).is_err());
            assert_eq!(fs::read_to_string(root.join("host.json")).unwrap(),bytes);
        }
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(root).unwrap();
    }

    // Chain: published project passes -> host release qualification; failure keeps owned work and recovery can qualify.
    #[test]
    fn release_health_requires_current_successful_publications() {
        let root=std::env::temp_dir().join(format!("taskq-publication-{}",std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let tool=crate::platform::tool(&root.join("install"));
        let host=Arc::new(Mutex::new(Host::new(Ok(policy()))));
        let roots=vec![root.clone()];
        host.lock().unwrap().sync(&root,vec![(1,99,"codex".into())]);
        let mut status=coreq::status::common(&tool,"fixture","project",1,None,None);
        status["complete"]=json!(true);status["tasks"]=json!([]);
        assert!(!host.lock().unwrap().publications_ready(&roots));
        assert!(crate::publish_status(&tool,&root,&status,Some(&host)));
        assert!(host.lock().unwrap().publications_ready(&roots));
        let path=coreq::status::path(&tool,&root);
        assert_eq!(crate::data::json(&path).unwrap().unwrap()["host_instance"],host.lock().unwrap().instance);
        fs::remove_file(&path).unwrap();fs::create_dir(&path).unwrap();
        assert!(!crate::publish_status(&tool,&root,&status,Some(&host)));
        assert!(!host.lock().unwrap().publications_ready(&roots));
        assert_eq!(host.lock().unwrap().slots.len(),1,"publication must not lose child ownership");
        fs::remove_dir(&path).unwrap();
        assert!(crate::publish_status(&tool,&root,&status,Some(&host)));
        for failed in [json!({"complete":false,"tasks":[]}),json!({"complete":true,"tasks":[],"error":"board unavailable"}),
            json!({"complete":true,"tasks":[],"focus_error":"focus command pending"})] {
            assert!(!crate::publish_status(&tool,&root,&failed,Some(&host)));
            assert_eq!(crate::data::json(&path).unwrap().unwrap()["host_instance"],host.lock().unwrap().instance,"failed status lost its owner generation");
            assert!(!host.lock().unwrap().publications_ready(&roots));
        }
        assert!(crate::publish_status(&tool,&root,&status,Some(&host)));
        let mut h=host.lock().unwrap();
        h.published.insert(root.clone(),Instant::now()-std::time::Duration::from_secs(61));
        assert!(!h.publications_ready(&roots),"old evidence cannot qualify a blocked thread");
        h.publication(&root,true);
        assert!(!h.publications_ready(&[root.clone(),root.join("new-project")]));
        h.held=Some("registry unreadable".into());assert!(!h.publications_ready(&roots));
        h.held=None;assert!(h.publications_ready(&roots));
        h.publication(&root,false);assert!(!h.publications_ready(&roots),"thread exit invalidates evidence");
        drop(h);fs::remove_dir_all(root).unwrap();
    }

    // Chain A3: grant -> accepted attempt -> pending/running union -> verified exit -> new grant.
    // Risk: double-counting retained grants, or cross-project equal numbers colliding and exceeding host ceilings.
    #[test]
    fn host_slots_survive_pending_running_and_project_identity() {
        let p = policy(); let cap = p.project(Path::new("/p"), &json!({"limits": {"codex": 2}})).unwrap();
        let mut h = Host::new(Ok(p));
        let (a, b) = (Path::new("a"), Path::new("b"));
        assert_eq!(h.reserve(a, 1, 0, "codex", &cap, false, 0), Ok(1));
        h.claimed(a, 1, 77);
        assert_eq!(h.sync(a, vec![(1, 77, "codex".into()), (1, 77, "codex".into())]), 1);
        assert_eq!(h.reserve(b, 1, 77, "codex", &cap, false, 30), Ok(2));
        assert_eq!(h.sync(b, vec![(1, 77, "codex".into()), (1, 77, "codex".into())]), 2);
        assert!(h.reserve(a, 2, 78, "codex", &cap, false, 60).is_err());
        assert_eq!(h.sync(a, vec![]), 1); // only a verified exit releases the attempt
        assert_eq!(h.reserve(a, 2, 78, "codex", &cap, false, 60), Ok(2));
        h.sync(a, vec![]); h.sync(b, vec![]);
        let lower = h.policy.as_ref().unwrap().project(Path::new("/p"), &json!({"limits": {"codex": 1}})).unwrap();
        assert!(h.reserve(a, 1, 80, "codex", &lower, false, 90).is_ok());
        assert!(h.reserve(a, 2, 81, "codex", &lower, false, 120).is_err());
        assert!(h.reserve(b, 2, 81, "codex", &cap, false, 120).is_ok());
    }

    // Chain A3: simultaneous project requests serialize through the same authority.
    // Risk: both threads observe the final free slot and grant it; failed contenders must not reserve anything.
    #[test]
    fn concurrent_grants_never_exceed_host_limit_and_round_robin_rotates() {
        let h = Arc::new(Mutex::new(Host::new(Ok(policy()))));
        let cap = policy().cap;
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let joins: Vec<_> = ["a", "b"].into_iter().map(|root| {
            let (h, cap, barrier) = (h.clone(), cap.clone(), barrier.clone());
            std::thread::spawn(move || { barrier.wait(); h.lock().unwrap().reserve(Path::new(root), 1, 1, "codex", &cap, false, 0).is_ok() })
        }).collect();
        barrier.wait();
        assert_eq!(joins.into_iter().map(|j| usize::from(j.join().unwrap())).sum::<usize>(), 1, "host spacing is serialized too");
        let mut h = h.lock().unwrap(); h.slots.clear(); h.last = None;
        h.ready(Path::new("a"), true); h.ready(Path::new("b"), true);
        assert!(h.reserve(Path::new("b"), 1, 1, "codex", &cap, true, 0).is_err());
        assert!(h.reserve(Path::new("a"), 1, 1, "codex", &cap, true, 0).is_ok());
        h.sync(Path::new("a"), vec![]);
        assert!(h.reserve(Path::new("a"), 2, 2, "codex", &cap, true, 30).is_err());
        assert!(h.reserve(Path::new("b"), 1, 1, "codex", &cap, true, 30).is_ok());
    }

    // Chain: R9 R12 §3 A3 — external demand survives task passes and rotates with a worker backlog.
    // Risk: PilotQ starves, overrides worker fairness or leaves stale demand after admission/cancellation.
    #[test]
    fn external_demand_rotates_without_task_pass_ownership() {
        for (manager,worker) in [("a","b"),("b","a"),("a","a")] {
            let (manager,worker)=(Path::new(manager),Path::new(worker));
            let p=policy();let cap=p.cap.clone();let mut h=Host::new(Ok(p));
            h.ready(worker,true);
            let first=h.external_reserve("turn-1",manager,"codex",&cap,0);
            if manager>worker {
                assert!(first.unwrap_err().contains("round-robin"));
                // A project pass with no task demand cannot remove the external request.
                h.ready(manager,false);
                assert!(h.reserve(worker,1,77,"codex",&cap,true,0).is_ok());
                h.sync(worker,vec![]);
                assert!(h.reserve(worker,2,78,"codex",&cap,true,30).unwrap_err().contains("round-robin"));
                assert!(h.external_reserve("turn-1",manager,"codex",&cap,30).is_ok());
            } else {assert!(first.is_ok());}
            assert_eq!(h.occupied(),1);
            h.external_sync("turn-1",manager,"codex",false);
            assert!(h.external_ready.is_empty(),"admission removes only its own demand");
            assert!(h.reserve(worker,3,79,"codex",&cap,true,60).is_ok());
            h.sync(worker,vec![]);
            // The next host reconciliation rebuilds demand from surviving eligible requests only.
            assert!(h.external_reserve("turn-2",manager,"codex",&cap,60).is_err());
            h.external_reset_ready();
            assert!(h.reserve(worker,4,80,"codex",&cap,true,90).is_ok());
        }
    }

    // Chain § 9: project text -> host policy. Risk: a stale per-project copy reaches admission or the host identity.
    #[test]
    fn project_copies_never_reach_the_host_policy() {
        let install=temp("project-copies");
        fs::write(install.join("host.json"),json!({"hosts":{"this":"named"},"has":{"named":["gpu"]},"limits":{"codex":3}}).to_string()).unwrap();
        let policy=Policy::parse(serde_json::from_str(&fs::read_to_string(install.join("host.json")).unwrap()).unwrap(),"this").unwrap();
        let project=json!({"repo":"invented/project","hosts":{"this":"copy"},"has":{"copy":["other"]},"permission_mode":"bypass","capacity_hosts":{"named":{"limits":{"codex":9}}},"admission":{"max":9},"load_gate":{"enabled":false}});
        let cap=policy.project(&install,&project).unwrap();
        assert_eq!((cap.limits.get("codex"),cap.max,cap.gate.enabled),(Some(&3),None,true));
        let (mut config,_)=crate::project_config_value(&install,project).unwrap();
        assert_eq!((config.has.len(),config.permission_mode.as_str()),(0,"dontAsk"));
        policy.apply(&mut config);
        assert_eq!((config.has,config.me),(vec!["gpu".to_string()],if std::env::var("TASKQ_HOST").ok().filter(|s|!s.is_empty()).is_none() {"named".to_string()} else {std::env::var("TASKQ_HOST").unwrap()}));
        fs::remove_dir_all(install).unwrap();
    }

    // Chain § 9: `service install` -> first policy. Risk: a host stays held for ever, or a later install replaces owner policy.
    #[test]
    fn install_seeds_a_missing_policy_and_keeps_an_existing_one() {
        let install=temp("policy-seed");
        assert!(Policy::read(&install).is_err(),"no policy reads unknown and holds launches");
        seed_policy(&install).unwrap();
        let seeded=Policy::read(&install).unwrap();
        assert_eq!((seeded.claim,seeded.update_every,seeded.rollback_after,seeded.cap.limits.len(),seeded.cap.auto),(ClaimMode::Any,60,600,0,true));
        let owner=json!({"limits":{"codex":2},"owner_field":"kept"}).to_string();
        fs::write(install.join("host.json"),&owner).unwrap();
        seed_policy(&install).unwrap();
        assert_eq!(fs::read_to_string(install.join("host.json")).unwrap(),owner);
        fs::remove_dir_all(install).unwrap();
    }

    #[cfg(windows)]
    fn modify_only_windows_acl(path:&Path) {
        // CI failed to load the Security module; use the framework ACL API directly.
        let code=r#"$ErrorActionPreference='Stop';$p=$env:TASKQ_ACL_FIXTURE;$a=[IO.Directory]::GetAccessControl($p);$a.SetAccessRuleProtection($true,$false);$s=[Security.Principal.WindowsIdentity]::GetCurrent().User;$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'TakeOwnership','ContainerInherit,ObjectInherit','None','Deny'));$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'Modify','ContainerInherit,ObjectInherit','None','Allow'));[IO.Directory]::SetAccessControl($p,$a)"#;
        let out=crate::platform::command("powershell").args(["-NoProfile","-NonInteractive","-Command",code]).env("TASKQ_ACL_FIXTURE",path).output().unwrap();
        assert!(out.status.success(),"ACL fixture: {}",String::from_utf8_lossy(&out.stderr));
    }

    // R9/section 9: an ordinary owner can replace data and DACLs without WRITE_OWNER.
    // Risk: unconditional owner/group copy rejects a CLI-written registry under a limited service token.
    #[cfg(windows)]
    #[test]
    fn registry_editor_preserves_modify_only_windows_acl() {
        for protected in [false,true] {
            let base=temp("registry-modify-acl");let install=base.join("install");
            let a=base.join("a");let b=base.join("b");
            for p in [&install,&a,&b] {fs::create_dir_all(p).unwrap();}
            modify_only_windows_acl(&install);
            ProjectEdit::new(&install,&a,true).unwrap().commit().unwrap();
            let path=install.join("projects");
            if protected {
                let code=r#"$ErrorActionPreference='Stop';$p=$env:TASKQ_ACL_FIXTURE;$a=[IO.File]::GetAccessControl($p);$a.SetAccessRuleProtection($true,$true);[IO.File]::SetAccessControl($p,$a)"#;
                let out=crate::platform::command("powershell").args(["-NoProfile","-NonInteractive","-Command",code]).env("TASKQ_ACL_FIXTURE",&path).output().unwrap();
                assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            }
            let expected=ownership(&path).unwrap();
            ProjectEdit::new(&install,&b,true).unwrap().commit().unwrap();
            assert_eq!(ownership(&path).unwrap(),expected,"registration preserves owner/group/DACL");
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            publish_service(&install,&native,"registry-fixture").unwrap();
            let (capture,guard)=crate::profile::tests::collecting(&install);
            let request=crate::lifecycle::uninstall_review(&install,"registry-uninstall",Some(&a)).unwrap();
            let received=crate::lifecycle::submit(&install,&request).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap_err().contains("project drain not observed"));
            let mode=fs::metadata(&path).unwrap().permissions();let mut readonly=mode.clone();readonly.set_readonly(true);
            fs::set_permissions(&path,readonly).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",true).is_err());
            let applied=crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap();
            let retained_applied=crate::resources::try_with(&install,|state|Ok(state.ops["registry-uninstall"].receipt.clone())).unwrap();
            assert_eq!(applied["state"],"applied");assert_eq!(applied["project_drained"],true);
            assert!(applied.get("observed_receipt").is_none());
            assert_eq!(projects(&install).unwrap().len(),2,"refused write retains both roots");
            assert_eq!(ownership(&path).unwrap(),expected);
            fs::set_permissions(&path,mode).unwrap();
            crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",true).unwrap();
            assert_eq!(projects(&install).unwrap(),vec![b.canonicalize().unwrap()]);
            assert_eq!(ownership(&path).unwrap(),expected,"removal preserves owner/group/DACL");
            let observed=crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap();
            assert_eq!(observed["state"],"observed");assert_eq!(observed["unregistered"],true);
            assert_eq!(observed["received_receipt"],received["received_receipt"]);
            assert_eq!(crate::lifecycle::submit(&install,&request).unwrap(),observed);
            // Resume the crash cut after registry commit but before the terminal receipt was saved.
            crate::resources::with(&install,|state| {
                state.ops.get_mut("registry-uninstall").unwrap().receipt=retained_applied.clone();Ok(())
            }).unwrap();
            let mut changed=expected.clone();changed[3]=json!(changed[3].as_u64().unwrap()^0x10);
            stage_owner(&path,&changed).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap_err().contains("ownership differs"));
            assert_eq!(crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap(),applied,"changed ACL cannot invent observation after registry commit");
            stage_owner(&path,&expected).unwrap();
            crate::resources::with(&install,|state| {state.ops.get_mut("registry-uninstall").unwrap().receipt.as_object_mut().unwrap().remove("registry_owner");Ok(())}).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap_err().contains("original registry ownership unknown"));
            crate::resources::with(&install,|state| {state.ops.get_mut("registry-uninstall").unwrap().receipt["registry_owner"]=expected.clone();Ok(())}).unwrap();
            crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap();
            assert_eq!(crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap(),observed);
            assert!(crate::lifecycle::pending(&install).unwrap().is_none(),"completed project removal releases the lifecycle barrier");
            capture.flush().unwrap();drop(guard);drop(capture);
            let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
            let events=reader.export_kinds(&["command.received","command.applied","command.observed"],0,0,1024).unwrap();
            for field in ["received_receipt","applied_receipt","observed_receipt"] {
                assert!(events["events"].as_array().unwrap().iter().any(|e|e["id"]==observed[field]),"missing native {field}");
            }
            println!("Windows registry protected={protected}: native owner {} {} receipt {observed}",native.pid,native.started);
            drop(reader);
            let mut foreign=expected.as_array().unwrap().clone();let offset=(foreign.len() as u32).to_le_bytes();
            for (i,byte) in offset.into_iter().enumerate() {foreign[4+i]=json!(byte);}
            foreign.extend([1,1,0,0,0,0,0,5,18,0,0,0].map(|byte|json!(byte))); // LocalSystem, not the ordinary owner.
            assert!(stage_owner(&path,&json!(foreign)).is_err(),"a differing owner still requires ownership rights");
            for i in 4..8 {foreign[i]=json!(255);}
            assert!(stage_owner(&path,&json!(foreign)).unwrap_err().contains("SID corrupt"));
            assert_eq!(ownership(&path).unwrap(),expected,"refused or corrupt ownership changes keep protection");
            fs::remove_dir_all(base).unwrap();
        }
    }

    // Chain A4: unknown or corrupt host policy -> refused launch, never fallback to project copies.
    #[test]
    fn unknown_policy_holds_without_discarding_occupied_slots() {
        let install = temp("unknown"); let _ = fs::remove_file(install.join("host.json"));
        let mut h = Host::new(Ok(policy())); let cap = policy().cap;
        h.reserve(Path::new("a"), 1, 7, "codex", &cap, false, 0).unwrap();
        h.reload(Policy::read(&install));
        assert!(h.policy.as_ref().unwrap_err().contains("unknown"));
        assert!(h.reserve(Path::new("b"), 1, 7, "codex", &cap, false, 30).is_err());
        assert_eq!(h.slots.len(), 1);
        for v in [json!({"admission": "bad"}), json!({"codex": [false]}), json!({"load_gate": {"enabled": "yes"}}), json!({"update_every": 0})] {
            assert!(Policy::parse(v, "invented").is_err());
        }
        assert_eq!(policy().project(Path::new("/p"), &json!({"limits": {"codex": 3}})).unwrap().limits["codex"], 2, "a project count above the host clamps, never holds");
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: host.json projects overlay -> project capacity and revision (#1282). Risk: an overlay raising the host
    // ceiling, reaching another root, or an edit-and-revert reusing a revision a stale operation names.
    #[test]
    fn project_overlay_only_lowers_its_own_root() {
        let host = json!({"limits": {"codex": 5, "claude": 5}});
        let base = Policy::parse(host.clone(), "invented").unwrap();
        let mut v = host.clone(); v["projects"] = json!({"/a": {"limits": {"codex": 4}, "op": "op-1"}});
        let p = Policy::parse(v.clone(), "invented").unwrap();
        assert_eq!(p.project(Path::new("/a"), &json!({})).unwrap().limits["codex"], 4);
        assert_eq!(p.project(Path::new("/a"), &json!({"limits": {"codex": 3}})).unwrap().limits["codex"], 3, "the lower of both");
        assert_eq!(p.project(Path::new("/b"), &json!({})).unwrap().limits["codex"], 5, "another root untouched");
        assert_eq!(p.project(Path::new("/b"), &json!({"limits": {"codex": 9}})).unwrap().limits["codex"], 5, "a taskq.json count above the host ceiling clamps, never holds");
        let off = Policy::parse(json!({"limits": {"codex": 5, "claude": 0}}), "invented").unwrap();
        assert_eq!(off.project(Path::new("/b"), &json!({"limits": {"claude": 4}})).unwrap().limits["claude"], 0, "a host 0 stays disabled");
        assert_eq!(p.project(Path::new("/a"), &json!({})).unwrap().limits["claude"], 5, "another runtime untouched");
        assert_eq!(p.project_limits(Path::new("/a"), &json!({})).unwrap(), BTreeMap::from([("codex".to_string(), 4)]));
        let e = json!({});
        assert_eq!(base.project_revision(Path::new("/a"), &e).unwrap(), base.project_revision(Path::new("/b"), &e).unwrap());
        assert_eq!(p.project_revision(Path::new("/b"), &e).unwrap(), base.project_revision(Path::new("/b"), &e).unwrap());
        let changed = p.project_revision(Path::new("/a"), &e).unwrap();
        assert_ne!(changed, base.project_revision(Path::new("/a"), &e).unwrap());
        assert_ne!(p.project_revision(Path::new("/a"), &json!({"limits": {"codex": 1}})).unwrap(), changed, "a taskq.json limits edit is a new revision");
        v["projects"]["/a"] = json!({"limits": {}, "op": "op-2"});
        let restored = Policy::parse(v.clone(), "invented").unwrap();
        assert_eq!(restored.project(Path::new("/a"), &json!({})).unwrap().limits["codex"], 5);
        let reverted = restored.project_revision(Path::new("/a"), &e).unwrap();
        assert!(reverted != changed && reverted != base.project_revision(Path::new("/a"), &e).unwrap(), "edit-and-revert is a new revision");
        for bad in [json!({"/a": {"limits": {"codex": 6}}}), json!({"/a": {"limits": {"other": 1}}}), json!({"/a": {"limits": {"codex": -1}}}),
            json!({"/a": {"limits": {}, "admission": {}}}), json!({"/a": {"op": "x"}}), json!({"/a": {"limits": {}, "op": "bad id"}}), json!(["/a"])] {
            let mut v = host.clone(); v["projects"] = bad.clone();
            assert!(Policy::parse(v, "invented").is_err(), "{bad} must hold launches");
        }
    }

    // Chain A5: host pid ownership and registry poke -> exact-root reload. Risk: duplicate host or discovery.
    #[test]
    fn power_loss_zeroed_records_are_set_aside_only_without_an_owner() {
        // Defect #1445: a zeroed service or status.json held startup forever after a power cut.
        let install = temp("zeroed");
        let zeroed = |p: &Path| fs::write(p, [0u8; 64]).unwrap();
        let aside = |dir: &Path, prefix: &str| fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(prefix)).count();
        zeroed(&install.join("service"));
        {
            let _owner = coreq::file_lock::try_lock(&install.join("service.lock")).unwrap();
            assert!(other(&install).is_err(), "a live native owner keeps the zeroed record");
            assert_eq!(fs::read(install.join("service")).unwrap(), [0u8; 64]);
        }
        assert_eq!(other(&install).unwrap(), None);
        assert!(!install.join("service").exists());
        assert_eq!(aside(&install, "service.zeroed-"), 1);
        fs::create_dir_all(crate::dot(&install)).unwrap();
        zeroed(&crate::dot(&install).join("status.json"));
        assert!(project_service(&install).is_err());
        assert!(crate::data::set_aside_zeroed(&crate::dot(&install).join("status.json")).unwrap());
        assert_eq!(project_service(&install).unwrap(), None);
        assert_eq!(aside(&crate::dot(&install), "status.json.zeroed-"), 1);
        fs::write(install.join("service"), "bad").unwrap();
        assert!(other(&install).is_err(), "a nonzero unreadable record is never moved");
    }

    #[test]
    fn ended_project_thread_logs_its_error() {
        // Defect #1445: an ended project thread logged no reason.
        assert_eq!(thread_error(&Ok(Err("kids: unreadable".into()))), "project thread error: kids: unreadable");
        let panic = std::thread::spawn(|| -> Res<()> { panic!("boom") }).join();
        assert_eq!(thread_error(&panic), "project thread error: panic: boom");
        let install = temp("thread-error");
        let root = install.join("proj");
        fs::create_dir_all(&root).unwrap();
        log_all(&install, std::slice::from_ref(&root), &thread_error(&Ok(Err("adopt failed".into()))));
        assert!(fs::read_to_string(project_log(&install, &root)).unwrap().contains("project thread error: adopt failed"));
    }

    #[test]
    fn pid_check_and_poke_use_host_paths_and_unknown_is_held() {
        let install = temp("pid");
        let child = if cfg!(windows) { std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn() }
            else { std::process::Command::new("sleep").arg("30").spawn() }.unwrap();
        let mut child = child;
        fs::create_dir_all(crate::dot(&install)).unwrap();
        let legacy_status = format!("{{\"pid\":{}}}", child.id());
        fs::write(crate::dot(&install).join("status.json"), &legacy_status).unwrap();
        assert!(startup_status(&install, &install, "invented malformed config","fixture").is_err());
        assert_eq!(fs::read_to_string(crate::dot(&install).join("status.json")).unwrap(), legacy_status, "startup failure cannot overwrite a live legacy pid");
        assert!(legacy(&install,&[install.clone()]).is_err());
        // A bare PID is no owner record: unknown holds, whatever the PID.
        fs::write(install.join("service"), child.id().to_string()).unwrap(); assert!(other(&install).unwrap_err().contains("unknown"));
        publish_service(&install,&coreq::process::identity(child.id()).unwrap().unwrap(),"invented-live-owner").unwrap(); assert_eq!(other(&install).unwrap(), Some(child.id()));
        let stale=std::time::SystemTime::now()-std::time::Duration::from_secs(120);
        fs::OpenOptions::new().write(true).open(install.join("service")).unwrap()
            .set_times(fs::FileTimes::new().set_modified(stale)).unwrap();
        assert_eq!(other(&install).unwrap(),Some(child.id()),"stale heartbeat cannot release a live service owner");

        publish_service(&install,&coreq::process::identity(std::process::id()).unwrap().unwrap(),"invented-own-owner").unwrap(); assert_eq!(other(&install).unwrap(), None);
        fs::write(install.join("service"), "bad").unwrap(); assert!(other(&install).unwrap_err().contains("unknown"));
        fs::write(install.join("projects"), format!("{}\n", install.display())).unwrap();
        assert_eq!(projects(&install).unwrap(), vec![install.canonicalize().unwrap()]);
        let _ = fs::remove_file(install.join("poke"));
        let mut registry = Registry::default(); registry.refresh(&install, 0);
        fs::write(install.join("projects"), "").unwrap(); registry.refresh(&install, 1);
        assert_eq!(registry.roots.len(), 1, "no folder discovery or unnecessary registry reads");
        crate::touch(&install.join("poke")); registry.refresh(&install, 2);
        assert!(registry.roots.is_empty(), "host poke loads the changed exact-root list before the minute poll");
        fs::write(install.join("projects"), "relative\n").unwrap(); registry.refresh(&install, 62);
        assert!(registry.error.as_ref().unwrap().contains("unknown")); assert!(registry.roots.is_empty(), "malformed registry retains the last good list");
        child.kill().unwrap(); let _ = child.wait(); fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn never_registered_host_has_zero_projects_but_a_lost_registry_is_unknown() {
        let install = std::env::temp_dir().join(format!("taskq-registry-{}", crate::runtime::uuid())); fs::create_dir(&install).unwrap();
        let mut registry = Registry::default(); registry.refresh(&install, 0);
        assert!(registry.error.is_none() && registry.roots.is_empty(), "fresh host: setup must not wait on a registry");
        fs::write(install.join("projects"), format!("{}\n", install.display())).unwrap(); registry.refresh(&install, 60);
        assert_eq!(registry.roots.len(), 1);
        fs::remove_file(install.join("projects")).unwrap(); registry.refresh(&install, 120);
        assert!(registry.error.is_some() && registry.roots.len() == 1, "a vanished registry keeps the last roots and reads unknown");
        fs::remove_dir_all(install).unwrap();
    }
