    use super::*;
    // Risk: product flags target another collector, reports omit real failures, or expired selection keeps collecting.
    #[test]
    fn product_selection_failure_report_export_and_expiry() {
        use std::sync::Arc;
        use tracing_subscriber::prelude::*;
        let dir=std::env::temp_dir().join(format!("taskq-diagnose-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(dir.join("telemetry.log")).unwrap();
        let store=Arc::new(coreq::telemetry::Store::open(&dir.join("telemetry.log/universal.sqlite3")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()),|| {
            let args=["profile","missing","--diagnose","profile","--diagnose-ms","60000","--diagnose-limit","1"].map(String::from);
            assert!(crate::run(&args).unwrap_err().contains("unknown profile view"));
            assert_eq!(capture.health()["diagnostics"]["state"],"limited");
            assert_eq!(capture.health()["diagnostics"]["admitted"],1);
            let expiry=["profile","missing","--diagnose","service","--diagnose-ms","1","--diagnose-limit","256"].map(String::from);
            assert!(crate::run(&expiry).is_err());
            let deadline=std::time::Instant::now()+Duration::from_secs(2);
            while capture.health()["diagnostics"]["state"]=="active" {
                assert!(std::time::Instant::now()<deadline,"product selection never expired");std::thread::yield_now();
            }
            assert_eq!(capture.health()["diagnostics"]["state"],"expired");
            let admitted=capture.health()["diagnostics"]["admitted"].as_u64().unwrap();assert_eq!(admitted,0);
            crate::profile::view(&dir,"guide",None).unwrap();
            assert_eq!(capture.health()["diagnostics"]["admitted"],admitted,"expired product selection admitted more methods");
            let id=request(&dir,std::process::id(),"profile",5000,1).unwrap();
            assert!(request(&dir,std::process::id(),"profile",5000,1).unwrap_err().contains("active"));
            let mut seen=None;apply(&dir,&mut seen).unwrap();assert_eq!(seen.as_deref(),Some(id.as_str()));
            crate::profile::view(&dir,"guide",None).unwrap();assert_eq!(capture.health()["diagnostics"]["state"],"limited");
            apply(&dir,&mut seen).unwrap();assert_eq!(capture.health()["diagnostics"]["admitted"],1,"live request reset its volume cap");
            let mut stale=read(&dir).unwrap().unwrap();stale.id=crate::runtime::uuid().replace('-',"");stale.started+="-wrong";
            coreq::write_durable(&dir.join("telemetry.log/diagnostic.json"),&serde_json::to_string(&stale).unwrap()).unwrap();
            assert!(apply(&dir,&mut seen).unwrap_err().contains("identity-mismatch"));assert_eq!(seen.as_deref(),Some(id.as_str()));
            std::fs::write(dir.join("telemetry.log/diagnostic.json"),"{broken").unwrap();assert!(apply(&dir,&mut seen).is_err());
            assert_eq!(std::fs::read_to_string(dir.join("telemetry.log/diagnostic.json")).unwrap(),"{broken");
            let open=tracing::info_span!("owned-open-fixture");
            let failed=coreq::telemetry::capture::operation("diagnostic.fixture",||Err::<(),_>("private payload"),|_|"fixture-failure");assert!(failed.is_err());
            let local=health();assert_eq!(local["scope"],"this-process");assert_eq!(local["lost"],0);
            drop(coreq::telemetry::Store::open(&dir.join("store-span-fixture.sqlite3")).unwrap());
            let report=actions(&dir,None).unwrap();
            // #1517 A1: a store span row names its trigger and outcome and dates its slowest wait.
            let span=report.lines().find(|row|row.contains("taskq.operation.telemetry.open first-open->scan")).unwrap_or_else(||panic!("{report}"));
            assert!(span.split('|').nth(5).unwrap().trim().parse::<u64>().is_ok(),"{span}");
            assert!(report.contains("taskq.operation.diagnostic.fixture"),"{report}");assert!(report.contains("fixture-failure:1"));
            let row=report.lines().find(|row|row.contains("owned-open-fixture")).unwrap();assert!(row.contains("0/0/1"),"open span was not unknown: {row}");
            let exported=super::super::export(&dir,None).unwrap();assert!(!exported.contains("private payload"));
            let values=exported.lines().skip(1).map(|line|serde_json::from_str::<Value>(line).unwrap()["observation"].clone()).collect::<Vec<_>>();
            assert!(values.iter().any(|v|v["source"]=="diagnostic"&&v["kind"]=="method"));
            assert!(values.iter().any(|v|v["operation"]=="collector.diagnostic.state"&&v["source"]=="expired"));
            assert!(values.iter().any(|v|v["operation"]=="collector.health"));drop(open);
        });
        capture.flush().unwrap();drop(capture);drop(store);std::fs::remove_dir_all(dir).unwrap();
    }

    // #1297 A2: defect was one `collection` row per status write plus a materialized `service.loop` row each pass
    // (its counters bypass CoreQ folding). Retained protection: a simulated hour of 2 s passes over four 15-minute
    // windows keeps one health row per window, each with cumulative counters, and a lost-count change stays a row.
    #[test]
    fn steady_health_writes_one_row_per_window() {
        use std::sync::Arc;
        use tracing_subscriber::prelude::*;
        let dir=std::env::temp_dir().join(format!("taskq-health-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(dir.join("telemetry.log")).unwrap();
        let store=Arc::new(coreq::telemetry::Store::open(&dir.join("telemetry.log/universal.sqlite3")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
        let start=crate::profile::now_ms().unwrap()/HEALTH_WINDOW_MS*HEALTH_WINDOW_MS;
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()),|| for tick in 0..1800 {
            let _repeating=tracing::info_span!("service.loop",coreq_repeating=true,project="owner/repo").entered();
            assert_eq!(health_at(Some(start+tick*2000))["state"],"collecting");
        });
        capture.flush().unwrap();
        let rows=store.export(0,0,1024).unwrap()["events"].as_array().unwrap().iter().map(|r|r["observation"].clone()).collect::<Vec<_>>();
        let health:Vec<_>=rows.iter().filter(|r|r["operation"]=="collector.health").collect();
        assert_eq!(health.len(),4,"one health row per window: {}",health.len());
        assert!(health.iter().all(|r|r["counts"]["lost"]==0&&r["counts"]["accepted"].is_u64()));
        assert!(rows.iter().filter(|r|r["operation"]=="taskq.service.loop").count()<=8,"health materialized the pass root");
        drop(capture);drop(store);std::fs::remove_dir_all(dir).unwrap();
    }
