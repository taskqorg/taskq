    use super::*;
    fn snapshot(root: &Path, seen: &mut Option<String>) -> Res<Value> {
        super::snapshot(root,&mut None,seen).map(|v|v.command)
    }
    fn receipts(store: &coreq::telemetry::Store) -> Vec<Value> {
        assert!(crate::telemetry::settle(std::time::Duration::from_secs(30)),"receipts did not settle");
        store.export(0,0,1024).unwrap()["events"].as_array().unwrap().iter()
            .filter(|r|r["observation"]["source"]=="command").cloned().collect()
    }

    // Chain: local command -> service snapshot -> replacement -> fresh observer; no effect replay.
    #[test]
    fn service_observes_original_command_and_supersession_idempotently() {
        crate::tests::with_test_capture(|store,dir| {
            let root=dir.join("project");fs::create_dir_all(dot(&root)).unwrap();
            assert_eq!(snapshot(&root,&mut None).unwrap()["state"],"untracked");
            crate::cli::service_cmd::set_focus(&root,"area-private",Some("private owner words")).unwrap();
            let pointer=fs::read_to_string(dot(&root).join("focus.command")).unwrap();
            assert!(!pointer.contains("area-private")&&!pointer.contains("private owner words")&&!pointer.contains(root.to_str().unwrap()));
            let mut seen=None;
            let absent=tracing::dispatcher::with_default(&tracing::Dispatch::none(),||snapshot(&root,&mut seen));
            assert!(absent.unwrap_err().contains("collector unavailable"));assert!(seen.is_none());
            let first=snapshot(&root,&mut seen).unwrap();
            assert_eq!(first["state"],"observed");
            assert_eq!(snapshot(&root,&mut seen).unwrap(),first);
            assert_eq!(snapshot(&root,&mut None).unwrap(),first,"new observer changed the receipt identity");
            let rows=receipts(store);
            assert_eq!(rows.iter().filter(|r|r["id"]==first["observed_receipt"]).count(),1);
            let applied=rows.iter().find(|r|r["id"]==first["applied_receipt"]).unwrap();
            let observed=rows.iter().find(|r|r["id"]==first["observed_receipt"]).unwrap();
            for key in ["trace","span","operation","version"] {assert_eq!(applied["observation"][key],observed["observation"][key]);}
            crate::cli::service_cmd::set_focus(&root,"area-private",None).unwrap();
            let second=snapshot(&root,&mut seen).unwrap();
            assert_ne!(first["observed_receipt"],second["observed_receipt"],"identical focus reused an old command");
            crate::cli::service_cmd::pause(&root,false).unwrap();
            let paused=read(&root).unwrap().unwrap().receipt;
            crate::cli::service_cmd::resume(&root).unwrap();
            assert_eq!(snapshot(&root,&mut seen).unwrap()["state"],"observed");
            let rows=receipts(store);
            assert!(rows.iter().any(|r|r["id"]==paused.event_id("superseded").unwrap()));
            assert!(!rows.iter().any(|r|r["id"]==paused.event_id("observed").unwrap()),"coalesced Pause falsely acknowledged");
            assert_eq!(rows.iter().filter(|r|r["observation"]["kind"]=="command.superseded").count(),3);
            assert_eq!(data::focus(&root).unwrap(),["area-private"]);
        });
    }

    // Chain: interruption or partial mutation -> explicit unknown/failed -> new command replaces without replay.
    #[test]
    fn interrupted_and_failed_commands_never_acknowledge_an_equal_focus() {
        crate::tests::with_test_capture(|store,dir| {
            let root=dir.join("project");fs::create_dir_all(dot(&root)).unwrap();
            crate::cli::service_cmd::set_focus(&root,"all",None).unwrap();
            let interrupted={
                let _action=tracing::info_span!("fixture.interrupted").entered();
                let r=CommandReceipt::current().unwrap();r.checkpoint("received").unwrap();
                let _locked=crate::wake::wake_lock(&root).unwrap();
                let _pending=begin(&root,&r).unwrap();r
            };
            assert!(snapshot(&root,&mut None).unwrap_err().contains("pending"));
            assert_eq!(data::focus(&root).unwrap(),["all"]);
            crate::cli::service_cmd::set_focus(&root,"all",None).unwrap();
            assert_eq!(snapshot(&root,&mut None).unwrap()["state"],"observed");
            let rows=receipts(store);
            assert!(rows.iter().any(|r|r["id"]==interrupted.event_id("superseded").unwrap()));
            assert!(!rows.iter().any(|r|r["id"]==interrupted.event_id("observed").unwrap()));
            fs::create_dir(dot(&root).join("focus.request")).unwrap();
            assert!(crate::cli::service_cmd::set_focus(&root,"area-next",None).unwrap_err().contains("partially saved"));
            assert_eq!(data::focus(&root).unwrap(),["area-next"]);
            assert!(snapshot(&root,&mut None).unwrap_err().contains("failed"));
            fs::remove_dir(dot(&root).join("focus.request")).unwrap();
            crate::cli::service_cmd::set_focus(&root,"all",None).unwrap();
            assert_eq!(snapshot(&root,&mut None).unwrap()["state"],"observed");
        });
    }

    // Chain: corrupt/mismatched tracking -> refusal before Resume -> safe Pause retains evidence.
    #[test]
    fn bad_tracking_is_preserved_and_never_blocks_safe_restriction() {
        crate::tests::with_test_capture(|store,dir| {
            let root=dir.join("project");fs::create_dir_all(dot(&root)).unwrap();
            crate::cli::service_cmd::set_focus(&root,"all",None).unwrap();
            let path=dot(&root).join("focus.command");let valid=fs::read_to_string(&path).unwrap();
            let original=read(&root).unwrap().unwrap().receipt;
            fs::write(dot(&root).join("focus"),"off").unwrap();
            assert!(snapshot(&root,&mut None).unwrap_err().contains("differs"));
            assert!(!receipts(store).iter().any(|r|r["id"]==original.event_id("observed").unwrap()));
            fs::write(dot(&root).join("focus.paused"),"all").unwrap();
            let mut newer:Value=serde_json::from_str(&valid).unwrap();newer["schema"]=json!(2);
            for bad in ["{".to_owned(),newer.to_string(),"x".repeat(4097)] {
                fs::write(&path,&bad).unwrap();fs::write(dot(&root).join("focus"),"off").unwrap();
                assert!(crate::cli::service_cmd::resume(&root).unwrap_err().contains("tracking unavailable"));
                assert!(data::focus(&root).unwrap().is_empty());
                fs::write(dot(&root).join("focus"),"all").unwrap();
                assert!(crate::cli::service_cmd::pause(&root,false).unwrap_err().contains("tracking unknown"));
                assert!(data::focus(&root).unwrap().is_empty());assert_eq!(fs::read_to_string(&path).unwrap(),bad);
                assert!(snapshot(&root,&mut None).is_err());
            }
            fs::write(&path,&valid).unwrap();
            fs::write(dot(&root).join("focus.command.pending"),"interrupted bytes").unwrap();
            assert!(crate::cli::service_cmd::resume(&root).is_err());
            assert!(snapshot(&root,&mut None).unwrap_err().contains("corrupt"));
            assert_eq!(fs::read_to_string(dot(&root).join("focus.command.pending")).unwrap(),"interrupted bytes");
            assert_eq!(fs::read_to_string(path).unwrap(),valid);
            let other=dir.join("other");fs::create_dir_all(dot(&other)).unwrap();
            fs::write(dot(&other).join("focus.command"),&valid).unwrap();
            fs::write(dot(&other).join("focus"),"all").unwrap();
            assert!(snapshot(&other,&mut None).unwrap_err().contains("another project"));
            assert_eq!(fs::read_to_string(dot(&other).join("focus.command")).unwrap(),valid);
        });
    }

    // Chain: validated crash residue -> phase-aware metadata recovery -> same observation, never an effect replay.
    #[test]
    fn staged_writes_reconcile_without_replacing_current_with_unapplied_intent() {
        crate::tests::with_test_capture(|store,dir| {
            let root=dir.join("project");fs::create_dir_all(dot(&root)).unwrap();
            crate::cli::service_cmd::set_focus(&root,"all",None).unwrap();
            let path=dot(&root).join("focus.command");let stage=dot(&root).join("focus.command.pending");
            let current=fs::read_to_string(&path).unwrap();let old=read(&root).unwrap().unwrap().receipt;
            let never_applied={
                let _action=tracing::info_span!("fixture.staged_initial").entered();
                let r=CommandReceipt::current().unwrap();r.checkpoint("received").unwrap();r
            };
            let mut v:Value=serde_json::from_str(&current).unwrap();
            v["command"]=json!(never_applied.encode());v["phase"]=json!("pending");v["focus"]=Value::Null;v["previous"]=json!(old.encode());
            for _ in 0..2 {
                fs::write(&stage,v.to_string()).unwrap();
                assert_eq!(snapshot(&root,&mut None).unwrap()["applied_receipt"],old.event_id("applied").unwrap());
                assert_eq!(fs::read_to_string(&path).unwrap(),current);assert!(!stage.exists());
            }
            let rows=receipts(store);
            assert_eq!(rows.iter().filter(|r|r["id"]==never_applied.event_id("failed").unwrap()).count(),1);
            assert!(!rows.iter().any(|r|r["id"]==old.event_id("superseded").unwrap()),"uncommitted intent superseded the actual current command");
            for phase in ["applied","failed"] {
                let r={
                    let _action=tracing::info_span!("fixture.staged_terminal").entered();
                    let r=CommandReceipt::current().unwrap();r.checkpoint("received").unwrap();
                    let owner=crate::wake::wake_lock(&root).unwrap();let _pending=begin(&root,&r).unwrap();
                    if phase=="applied" {crate::platform::write_atomic(&dot(&root).join("focus"),"area-staged").unwrap();}
                    crate::wake::poke_locked(&owner).unwrap();r.checkpoint(phase).unwrap();
                    let mut v:Value=serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
                    v["phase"]=json!(phase);v["previous"]=Value::Null;
                    v["focus"]=if phase=="applied" {json!(fingerprint(&data::focus(&root).unwrap()))} else {Value::Null};
                    fs::write(&stage,v.to_string()).unwrap();r
                };
                let before=fs::read(dot(&root).join("focus")).unwrap();
                let observed=snapshot(&root,&mut None);
                if phase=="applied" {assert_eq!(observed.unwrap()["observed_receipt"],r.event_id("observed").unwrap());}
                else {assert!(observed.unwrap_err().contains("failed"));}
                assert!(!stage.exists());assert_eq!(fs::read(dot(&root).join("focus")).unwrap(),before);
                assert_eq!(read(&root).unwrap().unwrap().phase,phase);
            }
            crate::cli::service_cmd::set_focus(&root,"all",None).unwrap();
            assert_eq!(snapshot(&root,&mut None).unwrap()["state"],"observed");
            let mut wrong:Value=serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            wrong["command"]=json!(old.encode());
            fs::write(&stage,wrong.to_string()).unwrap();
            assert!(snapshot(&root,&mut None).unwrap_err().contains("identity mismatch"));
            assert_eq!(fs::read_to_string(stage).unwrap(),wrong.to_string());
        });
    }

    // Chain: service snapshot -> stalled observed commit -> observation and the next writer proceed (#1351); the receipt lands later.
    #[test]
    fn observation_never_waits_on_a_stalled_receipt_commit() {
        use std::{sync::Arc,time::{Duration,Instant}};
        use tracing_subscriber::prelude::*;
        let dir=std::env::temp_dir().join(format!("taskq-observation-lock-{}",std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store=Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
        let layer=capture.clone().with_filter(tracing_subscriber::filter::Targets::new()
            .with_target("taskq::observation_fixture",tracing::Level::INFO));
        let dispatch=tracing::Dispatch::new(tracing_subscriber::registry().with(layer));
        let root=dir.join("project");fs::create_dir_all(dot(&root)).unwrap();
        tracing::dispatcher::with_default(&dispatch,|| {
            let _command=tracing::info_span!(target:"taskq::observation_fixture","fixture.command").entered();
            crate::wake::change_focus(&root,false,false,|d|crate::platform::write_atomic(&d.join("focus"),"all")).unwrap();
        });
        capture.flush().unwrap();
        let db=rusqlite::Connection::open(dir.join("db")).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        let started=Instant::now();
        let observed=tracing::dispatcher::with_default(&dispatch,||snapshot(&root,&mut None)).unwrap();
        assert_eq!(observed["state"],"observed");
        assert!(started.elapsed()<Duration::from_secs(2),"observation waited {:?} on a stalled store",started.elapsed());
        drop(crate::wake::wake_lock(&root).expect("writer blocked behind a receipt commit"));
        db.execute_batch("ROLLBACK").unwrap();
        assert!(crate::telemetry::settle(Duration::from_secs(30)));capture.flush().unwrap();
        assert!(store.export(0,0,1024).unwrap()["events"].as_array().unwrap().iter().any(|r|r["id"]==observed["observed_receipt"]),"observed receipt never landed");
        capture.flush().unwrap();drop(db);drop(dispatch);drop(capture);drop(store);fs::remove_dir_all(dir).unwrap();
    }

    // Chain: actual writer death before/after mutation -> no observed receipt -> explicit replacement.
    #[test]
    fn killed_command_leaves_unknown_state_without_replaying_effects() {
        use std::{io::{BufRead,BufReader},process::{Command,Stdio},time::Duration};
        crate::tests::with_test_capture(|store,dir| {
            for phase in ["prepared","changed"] {
                let root=dir.join(phase);fs::create_dir_all(dot(&root)).unwrap();
                crate::cli::service_cmd::set_focus(&root,"area-before",None).unwrap();
                let mut child=Command::new(std::env::current_exe().unwrap())
                    .args(["--exact","control::tests::interrupted_command_writer","--ignored","--nocapture"])
                    .env("TASKQ_CONTROL_FIXTURE",&root).env("TASKQ_CONTROL_PHASE",phase)
                    .env_remove("COREQ_TRACE_CONTEXT").stdout(Stdio::piped()).spawn().unwrap();
                let stdout=child.stdout.take().unwrap();let (tx,rx)=std::sync::mpsc::channel();
                let reader=std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines() {if line.unwrap()=="ACK" {let _=tx.send(());break;}}
                });
                let acknowledged=rx.recv_timeout(Duration::from_secs(15));
                let _=child.kill();child.wait().unwrap();reader.join().unwrap();acknowledged.unwrap();
                let command=read(&root).unwrap().unwrap().receipt;
                let expected=if phase=="changed" {"area-partial"} else {"area-before"};
                assert_eq!(data::focus(&root).unwrap(),[expected]);
                assert!(snapshot(&root,&mut None).unwrap_err().contains("pending"));
                assert_eq!(data::focus(&root).unwrap(),[expected],"observer replayed an interrupted effect");
                crate::cli::service_cmd::set_focus(&root,"area-after",None).unwrap();
                assert_eq!(snapshot(&root,&mut None).unwrap()["state"],"observed");
                let rows=receipts(store);
                assert!(rows.iter().any(|r|r["id"]==command.event_id("superseded").unwrap()));
                for phase in ["applied","observed"] {assert!(!rows.iter().any(|r|r["id"]==command.event_id(phase).unwrap()),"interrupted command acquired an invented phase");}
            }
        });
    }

    #[test]
    #[ignore = "subprocess killed by killed_command_leaves_unknown_state_without_replaying_effects"]
    fn interrupted_command_writer() {
        use std::{io::Write,sync::Arc};
        use tracing_subscriber::prelude::*;
        let root=std::path::PathBuf::from(std::env::var_os("TASKQ_CONTROL_FIXTURE").unwrap());
        let store=Arc::new(coreq::telemetry::Store::open(&root.parent().unwrap().join("db")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store,"taskq","crash-fixture").unwrap();
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture),|| {
            let _action=tracing::info_span!("fixture.crash").entered();
            crate::wake::change_focus(&root,false,false,|d|->Res<()> {
                if std::env::var("TASKQ_CONTROL_PHASE").unwrap()=="changed" {crate::platform::write_atomic(&d.join("focus"),"area-partial")?;}
                println!("ACK");std::io::stdout().flush().unwrap();
                loop {std::thread::park();}
            }).unwrap();
        });
    }
