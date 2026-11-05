    use super::*;
    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("taskq-sqlite-{}-{}", std::process::id(), crate::runtime::uuid()));
        std::fs::create_dir(&d).unwrap(); d
    }
    // R12/#1465: settled receipts must release their collector before its installation can be removed.
    #[test]
    fn receipt_settlement_waits_for_dispatch_release() {
        if std::env::var_os("TASKQ_RECEIPT_TEARDOWN_TEST").is_none() {
            let output=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","telemetry::tests::receipt_settlement_waits_for_dispatch_release","--nocapture"])
                .env("TASKQ_RECEIPT_TEARDOWN_TEST","1").output().unwrap();
            assert!(output.status.success(),"{}{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));return;
        }
        use std::sync::{Arc,Mutex,mpsc};
        use std::time::Duration;
        use tracing_subscriber::prelude::*;
        struct Release {entered:mpsc::Sender<()>,resume:Mutex<mpsc::Receiver<()>>}
        impl<S:tracing::Subscriber> tracing_subscriber::Layer<S> for Release {}
        impl Drop for Release {
            fn drop(&mut self) {
                self.entered.send(()).unwrap();
                self.resume.get_mut().unwrap().recv_timeout(Duration::from_secs(30)).unwrap();
            }
        }
        let d=dir();
        let store=Arc::new(coreq::telemetry::Store::open(&d.join("universal.sqlite3")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store,"taskq","test").unwrap();
        let (entered,dropping)=mpsc::channel();let (resume,wait)=mpsc::channel();
        let subscriber=tracing_subscriber::registry().with(capture).with(Release {entered,resume:Mutex::new(wait)});
        let (command,dispatch)=tracing::subscriber::with_default(subscriber,|| {
            let _span=tracing::info_span!("host.install").entered();
            (coreq::telemetry::capture::CommandReceipt::current().unwrap(),tracing::dispatcher::get_default(Clone::clone))
        });
        let worker=receipt_worker();
        *RECEIPTS.0.lock().unwrap()+=1;
        worker.send((command,"observed".into(),dispatch)).unwrap();drop(worker);
        dropping.recv_timeout(Duration::from_secs(30)).unwrap();
        let premature=settle(Duration::from_millis(25));
        resume.send(()).unwrap();
        assert!(settle(Duration::from_secs(30)),"receipt worker did not finish dispatcher teardown");
        assert!(!premature,"receipt settled while its dispatcher still owned the collector");
        std::fs::remove_dir_all(d).unwrap();
    }
    // #2329: both fixture processes acknowledge on a slow disk (as CoreQ's fixtures do), and the parent's waits outlast
    // that deadline. Production keeps the 3 s default; the fixture proves handover, not append speed.
    const SLOW_DISK:std::time::Duration=std::time::Duration::from_secs(60);
    const HANDOVER_BUDGET:std::time::Duration=std::time::Duration::from_secs(150);
    // A2/#1309: an older tray owns an incompatible IPC writer when the new host starts.
    // The heartbeat is the existing tray exit protocol; history and a new durable receipt must survive.
    #[test]
    fn older_tray_writer_hands_over_to_new_host() {
        use std::process::{Command,Stdio};
        for fault in ["wire", "unresponsive", "stale", "late"] {
            let d=dir();
            struct Child(std::process::Child);
            impl Drop for Child {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
            let spawn=|mode|Child(Command::new(std::env::current_exe().unwrap())
                .args(["--exact","telemetry::tests::tray_handover_process","--ignored","--nocapture"])
                .env("TASKQ_HANDOVER_FIXTURE",&d).env("TASKQ_HANDOVER_MODE",mode).env("TASKQ_HANDOVER_FAULT",fault).stdin(Stdio::null()).spawn().unwrap());
            let mut tray=spawn("tray");
            let until=std::time::Instant::now()+HANDOVER_BUDGET;
            while !d.join("ready").exists() {
                assert!(tray.0.try_wait().unwrap().is_none(),"old tray failed before readiness");
                assert!(std::time::Instant::now()<until,"old tray did not start");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let age=|age|std::fs::File::options().write(true).open(d.join("tray")).unwrap()
                .set_modified(std::time::SystemTime::now()-std::time::Duration::from_secs(age)).unwrap();
            if fault=="stale" || fault=="late" {age(20);}
            let mut host=spawn("host");
            // #1336: the tray heartbeat turns fresh after the host started without a collector.
            if fault=="late" {std::thread::sleep(std::time::Duration::from_secs(2));age(0);}
            loop {
                if let Some(status)=host.0.try_wait().unwrap() {assert!(status.success(),"new host could not record its receipt");break;}
                assert!(std::time::Instant::now()<until,"handover did not finish");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if fault=="stale" {
                assert_eq!(std::fs::read_to_string(d.join("tray")).unwrap(),tray.0.id().to_string(),"unknown owner was displaced");
                assert!(tray.0.try_wait().unwrap().is_none(),"unknown owner was stopped");
                std::fs::write(d.join("tray"),"fixture finished").unwrap();
            }
            assert!(tray.0.wait().unwrap().success());
            let reader=coreq::telemetry::Reader::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
            let rows=reader.export(0,0,1024).unwrap().to_string();
            assert!(rows.contains("old.tray"),"handover lost earlier history");
            assert_eq!(rows.contains("new.host.receipt"),fault!="stale","host receipt durability or refusal changed");
            drop(reader);std::fs::remove_dir_all(d).unwrap();
        }
    }

    // #1755 A3: lost batches log one unknown line and the next commit logs recovery; a quiet collector logs nothing.
    // Risk: a sticky "Telemetry: unknown" line reads as a dead collector while commits resumed.
    #[test]
    fn collector_state_reports_loss_then_recovery_once() {
        let (lost,ok)=("Telemetry: unknown (observations lost); collector retries","Telemetry: collecting");
        assert_eq!(capture_state((0,5),(0,5),ok),ok);
        assert_eq!(capture_state((0,5),(3,5),ok),lost);
        assert_eq!(capture_state((3,5),(3,5),lost),lost);
        assert_eq!(capture_state((3,5),(4,9),lost),lost,"a commit alongside a new loss is not recovery");
        assert_eq!(capture_state((4,9),(4,10),lost),ok);
    }

    #[test]
    #[ignore="owned older-tray/new-host process fixture"]
    fn tray_handover_process() {
        let d=std::path::PathBuf::from(std::env::var_os("TASKQ_HANDOVER_FIXTURE").unwrap());
        if std::env::var("TASKQ_HANDOVER_MODE").unwrap()=="tray" {
            let runtime=install_capture(&d).unwrap();runtime.acknowledge_within(SLOW_DISK).unwrap();
            let span=tracing::info_span!("old.tray");{let _entered=span.enter();}
            drop(span);coreq::telemetry::capture::qualify().unwrap();
            let endpoint=d.join("telemetry.log/telemetry-ipc/endpoint");
            let mut value:serde_json::Value=serde_json::from_slice(&std::fs::read(&endpoint).unwrap()).unwrap();
            if std::env::var("TASKQ_HANDOVER_FAULT").unwrap()!="unresponsive" {value["wire"]=serde_json::json!(1);}
            else {value["key"]=serde_json::json!(vec![0u8;32]);} // Keep the writer alive but refuse the handshake.
            std::fs::write(endpoint,serde_json::to_vec(&value).unwrap()).unwrap();
            let pid=std::process::id().to_string();
            std::fs::write(d.join("tray"),&pid).unwrap();std::fs::write(d.join("ready"),"").unwrap();
            let until=std::time::Instant::now()+HANDOVER_BUDGET;
            while std::fs::read_to_string(d.join("tray")).unwrap()==pid {
                assert!(std::time::Instant::now()<until,"old tray never received the handover");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        } else {
            if std::env::var("TASKQ_HANDOVER_FAULT").unwrap()=="stale" {
                let Err(error)=host_capture(&d) else {panic!("stale tray recovery succeeded")};
                assert!(error.contains("tray owner not fresh"),"{error}");return;
            }
            let mut capture=HostCapture::default();
            if std::env::var("TASKQ_HANDOVER_FAULT").unwrap()=="late" {
                let first=capture.pass(&d).unwrap();
                assert!(first.contains("tray owner not fresh"),"{first}");
                let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
                while capture.runtime.is_none() {
                    assert!(std::time::Instant::now()<until,"host never retried the fresh tray");
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if let Some(line)=capture.pass(&d) {assert_eq!(line,"Telemetry: collecting","one line per state change");}
                }
            } else {assert_eq!(capture.pass(&d).as_deref(),Some("Telemetry: collecting"),"new host collector unavailable");}
            capture.runtime.as_ref().unwrap().acknowledge_within(SLOW_DISK).unwrap();
            let span=tracing::info_span!("new.host.receipt");let _entered=span.enter();
            coreq::telemetry::capture::CommandReceipt::current().unwrap().checkpoint("observed").unwrap();
            coreq::telemetry::capture::qualify().unwrap();
        }
    }

    // R12: a mixed snapshot must obtain the budget lock before pinning SQLite pages.
    // Otherwise an owner checkpoint can wait for this reader while this reader waits for its lock.
    #[test]
    fn mixed_snapshot_waits_for_budget_before_opening_sqlite() {
        let d=dir();
        drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        drop(open(&d).unwrap());
        assert!(legacy_rows(&d,None,None).unwrap_err().contains("appeared during snapshot"));
        let path=d.join("telemetry.log/universal.sqlite3");
        std::fs::write(&path,b"unavailable until the owner finishes").unwrap();
        let held=crate::profile::log_lock(&d.join("taskq.log")).unwrap();
        let result=export(&d,None).unwrap_err();
        drop(held);
        assert!(result.contains("legacy snapshot lock"),"read entered SQLite before acquiring the shared budget: {result}");
        assert_eq!(std::fs::read(&path).unwrap(),b"unavailable until the owner finishes");
        std::fs::remove_file(&path).unwrap();
        drop(coreq::telemetry::Store::open_bounded(&path,budget(&d,"telemetry.sqlite3").unwrap()).unwrap());
        assert!(export(&d,None).unwrap().contains("committed-import-cursor-snapshot"));
        drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        std::fs::remove_dir_all(d).unwrap();
    }
    // Chain: product migration retry -> mixed reports -> original clocks, archive and rollback reader.
    #[test]
    fn maintenance_migration_retries_without_duplicate_reports_or_retention_reset() {
        let d=dir();drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        let now=std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;
        let received=now-60_000;
        let line=r#"taskq-profile {"op":"spawn","us":7,"error":"none","at_ms":1}"#;
        let mut archive=open(&d).unwrap();
        {let tx=archive.transaction().unwrap();for i in 0..1100 {
            let stage=if i==1023 {Some(r#"taskq-stage {"project":"o/r","host":"mac","stage":"build","task":1,"attempt":1,"rt":"claude","open":true,"outcome":"unknown","error":"none","at_ms":1,"unknown":"open-stage"}"#)}
                else if i==1024 {Some(r#"taskq-stage {"project":"o/r","host":"mac","stage":"build","task":1,"attempt":1,"rt":"claude","open":false,"outcome":"ok","error":"none","at_ms":1}"#)} else {None};
            insert(&tx,stage.unwrap_or(line),received).unwrap();
        }tx.commit().unwrap();}
        checkpoint(&archive).unwrap();drop(archive);
        let expected=crate::profile::legacy_reports(&report_input(&d).unwrap(),now).unwrap();
        let path=d.join("telemetry.log/universal.sqlite3");drop(coreq::telemetry::Store::open(&path).unwrap());
        let inject=Connection::open(&path).unwrap();
        inject.execute_batch("CREATE TABLE imports(source TEXT PRIMARY KEY,cursor TEXT NOT NULL,receipt TEXT NOT NULL); CREATE TRIGGER refuse_second BEFORE UPDATE ON imports BEGIN SELECT RAISE(ABORT,'injected second batch failure'); END").unwrap();
        assert!(migrate(&d).unwrap_err().contains("injected second batch failure"));
        assert_eq!(crate::profile::legacy_reports(&report_input(&d).unwrap(),now).unwrap(),expected,"partly committed import double-counted history");
        let cursor=coreq::telemetry::Reader::open(&path).unwrap().import_cursor("taskq.legacy").unwrap().unwrap();
        assert!(cursor.ends_with(".0.1024"));
        inject.execute_batch("DROP TRIGGER refuse_second").unwrap();drop(inject);
        assert_eq!(migrate(&d).unwrap(),76);assert_eq!(migrate(&d).unwrap(),0);
        assert_eq!(crate::profile::legacy_reports(&report_input(&d).unwrap(),now).unwrap(),expected);
        let exported=export(&d,None).unwrap();let rows=exported.lines().map(|s|serde_json::from_str::<serde_json::Value>(s).unwrap()).collect::<Vec<_>>();
        assert_eq!(rows.len(),1101);assert_eq!(rows[0]["sources"]["legacy"]["samples"],0);
        assert!(rows[1..].iter().all(|r|r["received_ms"]==received));
        let old=open(&d).unwrap();insert(&old,line,received+1).unwrap();checkpoint(&old).unwrap();
        assert_eq!(report_input(&d).unwrap().lines().count(),1101,"old writer append vanished");
        drop(old);assert_eq!(migrate(&d).unwrap(),1);
        assert_eq!(read(&d,now).unwrap().lines().count(),1101,"compatible rollback reader lost history");
        // Expired target records stay pruned even though the preserved archive still holds them.
        let old=open(&d).unwrap();let seq:i64=old.query_row("SELECT max(seq) FROM records",[],|r|r.get(0)).unwrap();
        insert(&old,line,1).unwrap();checkpoint(&old).unwrap();drop(old);
        assert_eq!(migrate(&d).unwrap(),1);
        assert_eq!(report_input(&d).unwrap().lines().count(),1101);
        assert_eq!(migrate(&d).unwrap(),0);
        assert_eq!(read(&d,now).unwrap().lines().count(),(seq+1) as usize);
        prune(&d,now+WINDOW_MS+1).unwrap();
        assert_eq!(read(&d,now).unwrap().lines().count(),(seq+1) as usize,"service retired the unaccepted migration source");
        std::fs::remove_dir_all(d).unwrap();
    }

    // Chain: retained semantic history + automatic observations -> read-only schema2 agent export.
    #[test]
    fn live_capacity_counts_retained_archive_without_deleting_it() {
        let d=dir();drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        let path=d.join("telemetry.log/universal.sqlite3");std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let live=coreq::telemetry::Store::open_bounded(&path,budget(&d,"telemetry.sqlite3").unwrap()).unwrap();
        let before=live.storage().unwrap()["database_limit"].as_i64().unwrap();
        let archive_count;
        {
            let _lock=crate::profile::log_lock(&d.join("taskq.log")).unwrap();let old=open(&d).unwrap();
            old.execute("INSERT INTO records(retained_ms,line) VALUES(1,?1)",[r#"taskq-profile {"op":"spawn","us":1,"error":"none","at_ms":1}"#]).unwrap();
            for _ in 0..17 {checkpoint(&old).unwrap();old.execute("INSERT INTO records(retained_ms,line) SELECT retained_ms,line FROM records",[]).unwrap();}
            checkpoint(&old).unwrap();archive_count=old.query_row("SELECT count(*) FROM records",[],|r|r.get::<_,i64>(0)).unwrap();
        }
        let event=coreq::telemetry::Event::new(semantic::observation(r#"taskq-profile {"op":"spawn","us":2,"error":"none","at_ms":2}"#).unwrap()).unwrap();
        live.append(&[event]).unwrap();let storage=live.storage().unwrap();
        // A6: peer growth never shrinks the 384 MiB store cap; accounting stays inside the 512 MiB working share.
        assert_eq!((before,storage["database_limit"].as_i64().unwrap()),(384<<20,384<<20));
        assert!(storage["installation_accounted_bytes"].as_u64().unwrap()<=coreq::telemetry::INSTALLATION_BYTES/2);
        let _lock=crate::profile::log_lock(&d.join("taskq.log")).unwrap();let old=open(&d).unwrap();
        assert_eq!(old.query_row("SELECT count(*) FROM records",[],|r|r.get::<_,i64>(0)).unwrap(),archive_count);
        drop(old);drop(_lock);drop(live);std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn live_semantics_use_the_installed_capture_and_leave_no_legacy_sink() {
        use tracing_subscriber::prelude::*;
        let d=dir();std::fs::create_dir(d.join("telemetry.log")).unwrap();
        let store=std::sync::Arc::new(coreq::telemetry::Store::open_bounded(&d.join("telemetry.log/universal.sqlite3"),budget(&d,"telemetry.sqlite3").unwrap()).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","test").unwrap();
        let ops=(0..1200).map(|mut i|{let mut route=String::from("route");for _ in 0..4 {route.push((b'a'+(i%26) as u8) as char);i/=26;}(format!("GET issues/N/{route}"),1)}).collect::<std::collections::BTreeMap<_,u64>>();
        let bytes=serde_json::to_vec(&ops).unwrap().len();assert!(bytes>16<<10&&bytes<LINE_MAX-1024);
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()),|| {
            let _action=tracing::info_span!("fixture.command").entered();
            record(Sample::legacy(r#"taskq-stage {"stage":"claim","task":1,"attempt":2,"rt":"other","us":1,"outcome":"ok","error":"none","at_ms":1,"unknown":"no-model"}"#).unwrap()).unwrap();
            for line in [
                r#"taskq-profile {"op":"spawn","us":1,"error":"none","at_ms":1}"#,
                r#"taskq-board {"op":"close","us":12,"outcome":"failed","error":"http4xx","remaining":null,"at_ms":1}"#,
                r#"taskq-transition {"project":"other","task":1,"entry":2,"from":"doing","to":"ask","prev_s":null,"rt":null,"host":null,"at_ms":1}"#,
            ] {record(Sample::legacy(line).unwrap()).unwrap();}
            crate::requests::record("worker service status",&ops);
        });
        capture.flush().unwrap();
        assert!(!d.join("telemetry.log/telemetry.sqlite3").exists(),"live semantic events must use the installed writer");
        let rows=store.export_kinds(&["stage"],0,0,1024).unwrap();
        let event=&rows["events"][0]["observation"];
        assert_eq!(event["version"],"test");assert!(event["trace"].is_string());
        let report=report_samples(&d,semantic::KINDS).unwrap();
        assert_eq!(report.len(),5);
        for kind in semantic::KINDS {
            assert!(report.iter().any(|sample|sample.kind()==*kind),"{kind}");
        }
        let requests=report.iter().find(|sample|sample.kind()=="requests").unwrap().canonical().unwrap();
        assert_eq!(requests["ops"],serde_json::to_value(&ops).unwrap());
        assert!(crate::requests::sources_samples(&report,crate::profile::now_ms().unwrap()).contains("worker service status"));
        let parts=store.export_kinds(&["requests"],0,0,1024).unwrap();let parts=parts["events"].as_array().unwrap();assert!(parts.len()>1);
        assert!(parts.iter().all(|part|part["observation"]["group_id"].is_string()));
        drop(capture);
        let db=Connection::open(d.join("telemetry.log/universal.sqlite3")).unwrap();
        assert_eq!(db.execute("DELETE FROM events WHERE id=?1",[parts.last().unwrap()["id"].as_str().unwrap()]).unwrap(),1);drop(db);
        assert!(report_samples(&d,&["requests"]).err().unwrap().contains("incomplete count summary"));
        assert!(crate::profile::report(&d).contains("Profile: unknown (telemetry incomplete count summary)"));
        drop(store);std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn export_preserves_history_unknowns_and_refuses_unimported_tail() {
        let d=dir();
        let old="taskq-profile {\"op\":\"spawn\",\"us\":7,\"error\":\"none\",\"at_ms\":1}\n";
        let unknown="taskq-profile {\"op\":\"spawn\",\"us\":8,\"error\":\"none\",\"at_ms\":null}\n";
        drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        append(&d,old,1).unwrap();
        append(&d,unknown,1).unwrap();
        let store=coreq::telemetry::Store::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
        store.append(&[coreq::telemetry::Event::new(serde_json::json!({"product":"taskq","version":"1","operation":"resume","kind":"method","trace":"t","span":"s","started_ms":2000,"outcome":"failed","error":"returned-error"})).unwrap()]).unwrap();
        drop(store);
        let archive=d.join("telemetry.log/telemetry.sqlite3");let before=std::fs::read(&archive).unwrap();
        let output=export(&d,Some(1000)).unwrap();
        let rows:Vec<serde_json::Value>=output.lines().map(|s|serde_json::from_str(s).unwrap()).collect();
        assert_eq!(rows[0]["sources"]["universal"]["samples"],1);assert_eq!(rows[0]["sources"]["legacy"]["samples"],1);
        assert_eq!(rows[1]["observation"]["operation"],"resume");
        assert_eq!(rows[2]["source"],"legacy");assert_eq!(rows[2]["id"],serde_json::Value::Null);
        assert_eq!(rows[2]["observation"]["started_ms"],serde_json::Value::Null);
        assert_eq!(rows[2]["observation"]["unknown"]["started_ms"],"not-recorded");
        assert_eq!(std::fs::read(&archive).unwrap(),before);
        std::fs::write(d.join("taskq.log"),old).unwrap();
        assert!(export(&d,None).unwrap_err().contains("awaits service import"));
        assert_eq!(std::fs::read(&archive).unwrap(),before);assert_eq!(std::fs::read_to_string(d.join("taskq.log")).unwrap(),old);
        std::fs::remove_dir_all(d).unwrap();
    }
    // R12/#2354: settled exports must accept native Windows paths without changing archive bytes or creating sidecars.
    #[cfg(windows)]
    #[test]
    fn export_reads_settled_archive_through_plain_and_verbatim_paths() {
        let root=dir();let d=root.join("archive %23 # Unicode é");std::fs::create_dir(&d).unwrap();
        drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        let line=r#"taskq-profile {"op":"spawn","us":7,"error":"none","at_ms":1}"#;
        append(&d,line,1).unwrap();
        let store=coreq::telemetry::Store::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
        let archive=d.join("telemetry.log/telemetry.sqlite3");let before=std::fs::read(&archive).unwrap();
        let verbatim=d.canonicalize().unwrap();assert!(verbatim.to_str().unwrap().starts_with(r"\\?\"));
        let plain=crate::platform::plain_path(&verbatim);
        let original=std::fs::metadata(&archive).unwrap().permissions();let mut readonly=original.clone();readonly.set_readonly(true);
        std::fs::set_permissions(&archive,readonly).unwrap();
        let output=[export(&plain,None),export(&verbatim,None)];
        let report=report_input(&verbatim);
        std::fs::set_permissions(&archive,original).unwrap();
        let output=output.map(Result::unwrap);assert_eq!(output[0],output[1]);
        let rows:Vec<serde_json::Value>=output[0].lines().map(|s|serde_json::from_str(s).unwrap()).collect();
        assert_eq!(rows.len(),2);assert_eq!(rows[0]["sources"]["legacy"]["samples"],1);
        assert_eq!(rows[1]["observation"]["operation"],"spawn");assert_eq!(report.unwrap(),format!("{line}\n"));
        assert_eq!(std::fs::read(&archive).unwrap(),before);
        for suffix in ["-wal","-shm"] {assert!(!std::path::PathBuf::from(format!("{}{suffix}",archive.display())).exists());}
        drop(store);std::fs::remove_dir_all(root).unwrap();
    }
    // R12/#2354: immutable reads must never hide committed WAL frames; bad or missing imported archives stay unknown.
    #[test]
    fn export_preserves_wal_frames_and_refuses_missing_or_corrupt_archive() {
        let d=dir();drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        let line=r#"taskq-profile {"op":"spawn","us":7,"error":"none","at_ms":1}"#;
        append(&d,line,1).unwrap();
        let store=coreq::telemetry::Store::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
        let archive=d.join("telemetry.log/telemetry.sqlite3");let db=open(&d).unwrap();
        insert(&db,line,2).unwrap();
        let wal=d.join("telemetry.log/telemetry.sqlite3-wal");
        let before=std::fs::read(&archive).unwrap();let frames=std::fs::read(&wal).unwrap();assert!(frames.len()>32);
        let output=export(&d,None).unwrap();let rows:Vec<serde_json::Value>=output.lines().map(|s|serde_json::from_str(s).unwrap()).collect();
        assert_eq!(rows.len(),3);assert_eq!(rows[0]["sources"]["legacy"]["samples"],2);
        assert_eq!(rows[2]["received_ms"],2);assert_eq!(std::fs::read(&archive).unwrap(),before);assert_eq!(std::fs::read(&wal).unwrap(),frames);
        drop(db);drop(store);
        assert_eq!(migrate(&d).unwrap(),2);
        let saved=std::fs::read(&archive).unwrap();std::fs::remove_file(&archive).unwrap();
        assert!(export(&d,None).unwrap_err().contains("imported archive missing; coverage unknown"));assert!(!archive.exists());
        std::fs::write(&archive,b"corrupt archive").unwrap();
        assert!(export(&d,None).is_err());assert_eq!(std::fs::read(&archive).unwrap(),b"corrupt archive");
        std::fs::write(&archive,saved).unwrap();assert!(export(&d,None).is_ok());
        std::fs::remove_dir_all(d).unwrap();
    }
    // Chain: #2306 profile export in a worker sandbox. Risk: a reader that cannot write <install> fails on a WAL
    // archive with "unable to open database file" because SQLite must create -shm.
    #[cfg(unix)]
    #[test]
    fn export_reads_wal_archive_from_a_read_only_install() {
        use std::os::unix::fs::PermissionsExt;
        let d=dir();
        drop(crate::profile::log_lock(&d.join("taskq.log")).unwrap());
        append(&d,"taskq-profile {\"op\":\"spawn\",\"us\":7,\"error\":\"none\",\"at_ms\":2000000}\n",1).unwrap();
        // the service keeps the universal store open, so its -wal/-shm exist; the closed legacy archive has neither
        let store=coreq::telemetry::Store::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
        let logs=d.join("telemetry.log");
        assert!(!logs.join("telemetry.sqlite3-wal").exists()&&!logs.join("telemetry.sqlite3-shm").exists());
        let before=std::fs::read(logs.join("telemetry.sqlite3")).unwrap();
        std::fs::set_permissions(&logs,std::fs::Permissions::from_mode(0o555)).unwrap();
        let out=export(&d,Some(1000));
        std::fs::set_permissions(&logs,std::fs::Permissions::from_mode(0o755)).unwrap();
        let out=out.unwrap();
        let head:serde_json::Value=serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(head["sources"]["legacy"]["samples"],1,"{out}");
        assert_eq!(std::fs::read(logs.join("telemetry.sqlite3")).unwrap(),before);
        drop(store);std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn legacy_retry_append_rotation_and_identical_events() {
        let d = dir(); let p = d.join("taskq.log");
        let original = "taskq-profile same\ntaskq-profile same\ntaskq-profile last\n";
        std::fs::write(&p, original).unwrap();
        assert_eq!(read(&d, 100).unwrap(), original);
        assert_eq!(read(&d, 100).unwrap(), original);
        let rotated = "taskq-profile same\ntaskq-profile last\ntaskq-profile new\n";
        std::fs::write(&p, rotated).unwrap();
        assert_eq!(read(&d, 100).unwrap(), format!("{original}taskq-profile new\n"));
        append(&d, "same\nsame\n", 100).unwrap();
        assert_eq!(read(&d, 100).unwrap().lines().count(), 6);
        std::fs::write(&p, "unrelated\n").unwrap();
        assert_eq!(read(&d, 100).unwrap().lines().count(), 6);
        let db = open(&d).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM records", [], |r| r.get::<_,i64>(0)).unwrap(), 6);
        let state: serde_json::Value = serde_json::from_str(&db.query_row("SELECT text FROM legacy WHERE id=1", [], |r| r.get::<_,String>(0)).unwrap()).unwrap();
        assert_eq!(state["gaps"], 1);
        assert_eq!(state["gap"]["rebaselined_bytes"], "unrelated\n".len());
        drop(db); std::fs::remove_dir_all(d).unwrap();
    }
    // A1/#2273: a log that outgrew the migration bound while imports failed still imports, rotates and exports.
    // Rotation keeps its newest half, every telemetry record reaches the archive and the re-baseline is a recorded gap.
    #[test]
    fn oversized_legacy_log_imports_rotates_and_exports() {
        let d = dir(); let p = d.join("taskq.log");
        let record = |n: u64| format!("taskq-profile {{\"op\":\"spawn\",\"us\":{n},\"error\":\"none\",\"at_ms\":{n}}}\n");
        std::fs::write(&p, record(1)).unwrap();
        crate::profile::prune(&d).unwrap();
        // A crash rewrote the head, so imports failed and rotation never ran while diagnostics kept arriving.
        let mut big = format!("\0\0\0\n{}", record(2));
        while big.len() as u64 <= LEGACY_MAX + (1 << 20) { big.push_str("service diagnostic line\n"); }
        big.push_str(&"x".repeat(2 * LINE_MAX)); big.push('\n');
        big.push_str(&record(3));
        std::fs::write(&p, &big).unwrap();
        drop(coreq::telemetry::Store::open(&d.join("telemetry.log/universal.sqlite3")).unwrap());
        // Read-only export never re-baselines: coverage stays unknown until the owner's pass.
        let e = export(&d, None).unwrap_err(); assert!(e.contains("rewritten or lost"), "{e}");
        crate::profile::prune(&d).unwrap();
        let kept = std::fs::read(&p).unwrap();
        assert!(kept.len() as u64 <= crate::data::LOG_MAX / 2 && big.as_bytes().ends_with(&kept));
        assert_eq!(read(&d, 200).unwrap(), format!("{}{}{}", record(1), record(2), record(3)));
        let db = open(&d).unwrap();
        let state: serde_json::Value = serde_json::from_str(&db.query_row("SELECT text FROM legacy WHERE id=1", [], |r| r.get::<_,String>(0)).unwrap()).unwrap();
        assert_eq!(state["gaps"], 1, "the rotated file continues the checkpoint");
        assert_eq!(state["gap"]["rebaselined_bytes"], big.len());
        drop(db);
        let rows: Vec<serde_json::Value> = export(&d, None).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
        assert_eq!(rows[0]["sources"]["legacy"]["samples"], 3);
        assert_eq!(rows[0]["sources"]["legacy"]["import_gaps"], 1);
        assert_eq!(rows[0]["sources"]["legacy"]["last_import_gap"], state["gap"]);
        assert!(report_samples(&d,semantic::KINDS).err().unwrap().contains("1 recorded recovery gap"));
        // Moving all source rows to the shared store must not erase source coverage metadata.
        assert_eq!(migrate(&d).unwrap(), 3);
        let header:serde_json::Value=serde_json::from_str(export(&d,Some(0)).unwrap().lines().next().unwrap()).unwrap();
        assert_eq!(header["sources"]["legacy"]["samples"],0);
        assert_eq!(header["sources"]["legacy"]["import_gaps"],1);
        assert_eq!(header["sources"]["universal"]["samples"],3);
        std::fs::write(d.join("projects"), "").unwrap();
        assert!(crate::profile::watch(&d,Some("1h")).unwrap().contains("Board: unknown (legacy import coverage unknown: 1 recorded recovery gap(s))"));
        assert!(report_samples(&d,semantic::KINDS).err().unwrap().contains("1 recorded recovery gap"));
        std::fs::remove_dir_all(d).unwrap();
    }
    // #2273: streaming keeps the import limits; refused logs stay byte for byte.
    #[test]
    fn legacy_telemetry_over_import_limits_is_refused_and_kept() {
        let d = dir(); let p = d.join("taskq.log");
        let line = format!("taskq-profile {}\n", "x".repeat(1000));
        let text = line.repeat(LEGACY_MAX as usize / line.len() + 1);
        std::fs::write(&p, &text).unwrap();
        assert!(read(&d, 100).unwrap_err().contains("migration bound; file kept"));
        assert!(crate::profile::prune(&d).unwrap_err().contains("migration bound; file kept"));
        assert_eq!(std::fs::read(&p).unwrap(), text.as_bytes());
        let long = format!("taskq-profile {}\n", "x".repeat(LINE_MAX));
        std::fs::write(&p, &long).unwrap();
        assert!(read(&d, 100).unwrap_err().contains("exceeds 64 KiB; file kept"));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), long);
        std::fs::remove_dir_all(d).unwrap();
    }
    // #2273: offsets, digests and the rotation tail cover complete lines; diagnostics may hold any bytes.
    #[test]
    fn legacy_scan_covers_complete_lines_only() {
        let d = dir();
        let complete = b"a\xff\r\ntaskq-stage s\r\n";
        std::fs::write(d.join("taskq.log"), [&complete[..], b"partial diagnostic"].concat()).unwrap();
        let log = legacy_log(&d, &[0, 3, 4, complete.len()]).unwrap();
        let digest = |b: &[u8]| hex(ring::digest::digest(&ring::digest::SHA256, b));
        assert_eq!((log.bytes, log.sha256.as_str()), (complete.len(), digest(complete).as_str()));
        assert_eq!(log.digest(0), Some(hash("").as_str()));
        assert_eq!(log.digest(4), Some(digest(&complete[..4]).as_str()));
        assert_eq!(log.digest(3), None, "a checkpoint inside a line never matches");
        assert_eq!(log.digest(complete.len()), Some(log.sha256.as_str()));
        assert_eq!(log.telemetry, vec![(4, "taskq-stage s".to_string())]);
        assert_eq!((log.tail_bytes, log.tail_sha256.as_str()), (complete.len() - 4, digest(&complete[4..]).as_str()));
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn rewritten_legacy_log_rebaselines_with_recorded_gap_logs_once_and_resumes_import() {
        let d = dir(); let p = d.join("taskq.log");
        let first = "taskq-profile {\"op\":\"spawn\",\"us\":5,\"error\":\"none\",\"at_ms\":10}\n";
        std::fs::write(&p, first).unwrap();
        prune(&d, 100).unwrap();
        assert_eq!(read(&d, 100).unwrap(), first);
        // Simulate preserved migration state (as on Windows) and an external taskq.log rewrite.
        let mut db = open(&d).unwrap();
        source::prepare(&d).unwrap();
        assert!(source::preserved(&d).unwrap());
        drop(db);
        let rewritten = "service diagnostic line\ntaskq-profile {\"op\":\"spawn\",\"us\":9,\"error\":\"none\",\"at_ms\":20}\n";
        std::fs::write(&p, rewritten).unwrap();
        // Cycle 1 after rewrite: prune re-baselines with a recorded gap and imports the new telemetry line.
        prune(&d, 200).unwrap();
        db = open(&d).unwrap();
        let state1: serde_json::Value = serde_json::from_str(&db.query_row("SELECT text FROM legacy WHERE id=1", [], |r| r.get::<_,String>(0)).unwrap()).unwrap();
        assert_eq!(state1["gaps"], 1);
        assert_eq!(state1["gap"]["at_ms"], 200);
        assert_eq!(state1["gap"]["previous_bytes"], first.len());
        assert_eq!(state1["gap"]["rebaselined_bytes"], rewritten.len());
        drop(db);
        // Cycle 2 without further rewrite: no error, no second gap, import resumes on appended lines.
        prune(&d, 300).unwrap();
        let appended = format!("{rewritten}taskq-profile {{\"op\":\"spawn\",\"us\":11,\"error\":\"none\",\"at_ms\":30}}\n");
        std::fs::write(&p, &appended).unwrap();
        prune(&d, 400).unwrap();
        db = open(&d).unwrap();
        let state2: serde_json::Value = serde_json::from_str(&db.query_row("SELECT text FROM legacy WHERE id=1", [], |r| r.get::<_,String>(0)).unwrap()).unwrap();
        assert_eq!(state2["gaps"], 1, "subsequent cycles must not record additional gaps");
        assert_eq!(state2["gap"]["at_ms"], 200);
        drop(db);
        assert_eq!(read(&d, 400).unwrap().lines().count(), 3, "retained history kept and new lines imported after re-baseline");
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn incomplete_legacy_record_is_unknown_but_new_collection_continues() {
        let d = dir(); let p = d.join("taskq.log");
        std::fs::write(&p,"taskq-profile {truncated").unwrap();
        assert!(read(&d,100).unwrap_err().contains("truncated"));
        assert!(append(&d,"new record\n",100).unwrap_err().contains("truncated"));
        let db = open(&d).unwrap();
        assert_eq!(db.query_row("SELECT line FROM records", [], |r| r.get::<_,String>(0)).unwrap(),"new record");
        assert_eq!(std::fs::read_to_string(&p).unwrap(),"taskq-profile {truncated");
        drop(db); std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn failed_transaction_foreign_corrupt_and_newer_store_are_preserved() {
        let d = dir();
        assert_eq!(read(&d,100).unwrap(), "");
        let db = open(&d).unwrap();
        db.execute_batch("CREATE TRIGGER reject_test BEFORE INSERT ON records WHEN new.line='reject' BEGIN SELECT RAISE(ABORT,'invented write failure'); END;").unwrap();
        drop(db);
        assert!(append(&d,"ok\nreject\n",100).is_err());
        assert_eq!(read(&d,100).unwrap(), "", "the first row rolls back with the failed second row");
        assert!(append(&d,&"x".repeat(LINE_MAX+1),100).is_err());
        let db = open(&d).unwrap(); db.pragma_update(None,"user_version",2).unwrap(); drop(db);
        assert!(read(&d,100).unwrap_err().contains("unsupported"));
        let p = d.join("telemetry.log/telemetry.sqlite3"); std::fs::write(&p,b"foreign bytes").unwrap();
        assert!(read(&d,100).is_err()); assert_eq!(std::fs::read(&p).unwrap(),b"foreign bytes");
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn acknowledged_write_survives_process_kill() {
        if let Some(path) = std::env::var_os("TASKQ_SQLITE_CRASH_TEST") {
            let d = Path::new(&path);
            append(d, "acknowledged\n", 100).unwrap();
            // Keep the connection and its uncommitted write alive across the forced process exit.
            let db = open(d).unwrap();
            db.execute_batch("BEGIN IMMEDIATE; INSERT INTO records(retained_ms,line) VALUES(100,'uncommitted');").unwrap();
            std::fs::write(d.join("ack"), b"ready").unwrap();
            std::thread::sleep(std::time::Duration::from_secs(30));
            panic!("parent did not kill child");
        }
        let d = dir();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "telemetry::tests::acknowledged_write_survives_process_kill", "--nocapture"])
            .env("TASKQ_SQLITE_CRASH_TEST", &d).stdout(std::process::Stdio::null()).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !d.join("ack").exists() {
            assert!(child.try_wait().unwrap().is_none(), "writer exited before acknowledgement");
            if std::time::Instant::now() >= deadline { child.kill().unwrap(); child.wait().unwrap(); panic!("writer did not acknowledge"); }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap(); child.wait().unwrap();
        assert_eq!(read(&d,100).unwrap(), "acknowledged\n");
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn legacy_sweep_preserves_store_and_rollback_history() {
        let d = dir();
        append(&d,"new-version\n",100).unwrap();
        crate::platform::sweep(&d, &d.join("unused-shim"));
        std::fs::write(d.join("taskq.log"),"taskq-profile old-version\n").unwrap();
        assert_eq!(read(&d,100).unwrap(),"new-version\ntaskq-profile old-version\n");
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn archive_pressure_refuses_growth_without_deleting_history() {
        // Bulk-load the first rows in one transaction; per-append open and checkpoint made 60 s on Windows (#1376).
        let (d,filled)=(dir(),440);
        {let mut db=open(&d).unwrap();let tx=db.transaction().unwrap();
        for n in 0..filled {insert(&tx,&format!("{n}:{}","x".repeat(60<<10)),100).unwrap();}
        tx.commit().unwrap();checkpoint(&db).unwrap();}
        let mut accepted=filled;
        for n in filled..600 {
            match append(&d,&format!("{n}:{}\n","x".repeat(60<<10)),100) {
                Ok(())=>accepted+=1,
                Err(error)=>{assert!(error.contains("source archive preserved"),"append {n} at {}: {error}",d.display());break;}
            }
            let bytes:u64=std::fs::read_dir(d.join("telemetry.log")).unwrap().map(|e|e.unwrap().metadata().unwrap().len()).sum();
            assert!(bytes<64<<20);
        }
        assert!(accepted>filled&&accepted<600,"{accepted}");
        let text=read(&d,100).unwrap();assert_eq!(text.lines().count(),accepted);assert!(text.starts_with("0:"));
        std::fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn retention_and_pinned_reader_refuse_writes_without_loss() {
        let d = dir(); append(&d,"old\n",1).unwrap();
        append(&d,"fresh\n",WINDOW_MS+2).unwrap(); prune(&d,WINDOW_MS+2).unwrap();
        assert_eq!(read(&d,WINDOW_MS+2).unwrap(),"fresh\n");
        let reader = open(&d).unwrap(); reader.execute_batch("BEGIN; SELECT * FROM records;").unwrap();
        let writer = open(&d).unwrap(); writer.execute("INSERT INTO records(retained_ms,line) VALUES(1,'committed')",[]).unwrap();
        assert!(append(&d,"refused\n",100).unwrap_err().contains("checkpoint busy"));
        reader.execute_batch("ROLLBACK").unwrap(); drop(reader); drop(writer);
        assert_eq!(read(&d,100).unwrap(),"fresh\ncommitted\n");
        std::fs::remove_dir_all(d).unwrap();
    }
