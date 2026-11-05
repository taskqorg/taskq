    use super::*;

    /// Settles this process's queued command receipts (#1351) before the fixture collector is released.
    pub(crate) struct Collecting(#[allow(dead_code)] tracing::subscriber::DefaultGuard);
    impl Drop for Collecting {fn drop(&mut self) {crate::telemetry::settle(std::time::Duration::from_secs(30));}}
    pub(crate) fn collecting(dir:&Path)->(coreq::telemetry::capture::Capture,Collecting) {
        use tracing_subscriber::prelude::*;
        fs::create_dir_all(dir.join("telemetry.log")).unwrap();
        let store=std::sync::Arc::new(coreq::telemetry::Store::open_bounded(&dir.join("telemetry.log/universal.sqlite3"),crate::telemetry::budget(dir,"telemetry.sqlite3").unwrap()).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store,"taskq","test").unwrap();
        let guard=tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        (capture,Collecting(guard))
    }
    // #1960 defect: `history` (#1947) was missing from BOARD_OPS, so every first-admission read failed its telemetry append.
    // Risk kept: a board adapter op kind that telemetry rejects.
    #[test]
    fn every_profiled_board_op_appends() {
        let dir=std::env::temp_dir().join(format!("taskq-board-ops-{}",std::process::id()));
        let _=fs::remove_dir_all(&dir);
        let (_capture,_guard)=collecting(&dir);
        let kinds:Vec<&str>=include_str!("board/mod.rs").split("self.op(\"").skip(1).filter_map(|s|s.split_once("\", |b|").map(|p|p.0)).collect();
        assert!(kinds.contains(&"history"),"{kinds:?}");
        for op in kinds.into_iter().chain(["claim","comment","publish"]) {
            let sample=crate::telemetry::Sample::Board(BoardOp {op:op.into(),us:1,outcome:"failed".into(),error:error("HTTP 502").into(),remaining:None,at_ms:Some(1)});
            assert!(crate::telemetry::record(sample).is_ok(),"board op {op} must append");
        }
        let _=fs::remove_dir_all(&dir);
    }
    #[test]
    fn guide_needs_no_store() {
        let missing = Path::new("/nonexistent/taskq-profile-guide");
        let guide = view(missing, "guide", None).unwrap();
        assert!(guide.contains("taskq profile export --since 7d"));
        assert!(guide.contains("Never add"));
        assert!(guide.contains("cost unknown"));
        assert!(view(missing, "guide", Some("24h")).is_err());
    }


    // Chain: § 4 simultaneous project stages -> one host log. Risk: first-project identity leaks or writes collide.
    #[test]
    fn project_threads_keep_stage_identity_and_serialize_appends() {
        let dir = std::env::temp_dir().join(format!("taskq-profile-threads-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap(); let (_capture,_guard)=collecting(&dir); let path = dir.join("taskq.log"); let _ = fs::remove_file(&path);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let dispatch=tracing::dispatcher::get_default(Clone::clone);
        let joins: Vec<_> = ["invented/one", "invented/two"].into_iter().map(|repo| {
            let (path, barrier, dispatch) = (path.clone(), barrier.clone(), dispatch.clone());
            std::thread::spawn(move || tracing::dispatcher::with_default(&dispatch,|| {
                project(repo); host("invented-host"); barrier.wait();
                stage_at(Some(&path), "start", 1, 7, "codex", Some(std::time::Duration::ZERO), "ok", "none", Usage::NoModel);
            }))
        }).collect();
        for j in joins { j.join().unwrap(); }
        let text = read_log(&dir).unwrap();
        let projects: std::collections::BTreeSet<_> = text.lines().map(|l| {
            let r: Stage = serde_json::from_str(l.strip_prefix(STAGE_PREFIX).unwrap()).unwrap();
            assert_eq!(r.host.as_deref(), Some("invented-host")); r.project.unwrap()
        }).collect();
        assert_eq!(projects, ["invented/one".to_string(), "invented/two".to_string()].into_iter().collect());
        drop(dispatch);drop(_guard);drop(_capture);fs::remove_dir_all(dir).unwrap();
    }
    // Chain: §4 terminal report; completed owner waits must not erase hand-in evidence,
    // while a new build/review lifetime must expose a later vanished turn.
    #[test]
    fn terminal_regression_owner_wait_and_reopened_turns() {
        let row = |stage, outcome, open| format!("taskq-stage {}\n", serde_json::json!({
            "project":"o/r", "host":"mac", "stage":stage, "task":7, "attempt":41,
            "rt":"codex", "us":null, "outcome":outcome, "error":"none", "at_ms":1, "open":open
        }));
        let mut text = row("build", "ask", false);
        text += &row("ask", "ok", false);
        assert_eq!(defects(&text, &vec![], None, 400_000, 300).unwrap().1, 0);
        text += &row("build", "unknown", true);
        assert_eq!(defects(&text, &vec![], None, 400_000, 300).unwrap().1, 1);
        text += &row("build", "cancelled", false);
        text += &row("ask", "ok", false);
        assert_eq!(unresolved(&text).unwrap().len(), 0, "owner requeue preserves cancellation");
        text += &row("review", "unknown", true);
        assert_eq!(unresolved(&text).unwrap().len(), 1);
        text += &row("review", "ok", false);
        assert!(unresolved(&text).unwrap().is_empty());
    }

    // Chain: §4 cached report; a fresh task row is not proof of a live local child.
    #[test]
    fn terminal_regression_snapshot_liveness() {
        let dir = std::env::temp_dir().join(format!("taskq-profile-live-{}", std::process::id()));
        fs::create_dir_all(dir.join(".taskq")).unwrap();
        fs::write(dir.join("projects"), dir.display().to_string()).unwrap();
        let mut v = serde_json::json!({"complete":true,"board":{"repo":"o/r"},
            "tasks":[{"attempt":41}],"kids":[],"at":now_ms().unwrap()/1000,
            "pid":std::process::id(),"version":crate::platform::VERSION});
        fs::write(dir.join(".taskq/status.json"), v.to_string()).unwrap();
        assert!(local_live(&dir).is_empty(), "dead-host task must remain reportable");
        v["kids"] = serde_json::json!([{"attempt":42,"live":"running"},{"attempt":43,"live":"unknown"}]);
        fs::write(dir.join(".taskq/status.json"), v.to_string()).unwrap();
        assert_eq!(local_live(&dir), vec![("o/r".into(),42)]);
        v["complete"] = serde_json::json!(false);
        fs::write(dir.join(".taskq/status.json"), v.to_string()).unwrap();
        assert!(local_live(&dir).is_empty(), "incomplete snapshots exclude nothing");
        v["complete"] = serde_json::json!(true);
        fs::write(dir.join(".taskq/status.json"), v.to_string()).unwrap();
        OpenOptions::new().write(true).open(dir.join(".taskq/status.json")).unwrap().set_times(
            fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1))).unwrap();
        assert!(local_live(&dir).is_empty(), "stale snapshots exclude nothing");
        fs::remove_dir_all(dir).unwrap();
    }

    // #1345 A1-A4: only a fresh status of a live service speaks for its health; stale, dead or missing reads unknown, never zero.
    #[test]
    fn profile_health_shows_service_scope_or_unknown_reason() {
        let now=1_800_000_000;
        let tel=serde_json::json!({"state":"collecting","scope":"this-process","lost":0,"failed_batches":0,"append_us":900,"append_batches":3,"max_append_us":700});
        let st=|at:u64|Ok(Some(serde_json::json!({"at":at,"pid":42,"telemetry":tel})));
        let lines=|statuses:&[Result<Option<serde_json::Value>,String>],alive:bool|health_lines("Service telemetry",&service_health(statuses,now,|_|alive));
        let fresh=lines(&[Ok(None),st(now-90),st(now-5)],true);
        assert_eq!(fresh,"Service telemetry: collecting · scope this-process · pid 42 · at 2027-01-15 07:59 UTC\n\
            Service telemetry counts: lost 0 · failed batches 0 · timeouts unknown (not counted) · append avg 300 us · max 700 us\n");
        assert!(fresh.lines().all(|l|l.chars().count()<=160),"{fresh}");
        assert_eq!(lines(&[st(now-90)],true),"Service telemetry: unknown (service status 1m old)\n");
        assert_eq!(lines(&[st(now-5)],false),"Service telemetry: unknown (service pid 42 not running)\n");
        assert_eq!(lines(&[Ok(None)],true),"Service telemetry: unknown (service status.json missing)\n");
        assert_eq!(lines(&[],true),"Service telemetry: unknown (no registered project)\n");
        assert_eq!(lines(&[Ok(Some(serde_json::json!({"at":now,"pid":42})))],true),"Service telemetry: unknown (service status has no telemetry snapshot)\n");
        let lost=serde_json::json!({"state":"unknown","scope":"this-process","lost":2,"failed_batches":1});
        assert!(health_lines("CLI telemetry",&Ok((lost,7,now))).contains("unknown (collector lost records or failed batches)"));
        let missing=serde_json::json!({"state":"unknown","scope":"this-process","reason":"collector-snapshot-unavailable"});
        assert_eq!(health_lines("CLI telemetry",&Ok((missing,7,now))),"CLI telemetry: unknown (collector-snapshot-unavailable) · scope this-process · pid 7 · at 2027-01-15 08:00 UTC\n");
    }

    // #1766 A3-A4: a drain froze every idle project's status.json and health showed no age, so nothing flagged it.
    // Retained protection: health reads each registered project's status age from the file alone and flags one older
    // than two refreshes, with no recorded operation needed; a missing status reads unknown, never fresh.
    #[test]
    fn profile_health_flags_project_status_older_than_two_refreshes() {
        let dir=std::env::temp_dir().join(format!("taskq-status-age-{}",crate::runtime::uuid()));
        let root=dir.join("frozen");
        fs::create_dir_all(root.join(".taskq")).unwrap();
        fs::write(dir.join("projects"),format!("{}\n",root.display())).unwrap();
        let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let line=|at:u64|{
            fs::write(root.join(".taskq/status.json"),serde_json::json!({"at":at,"poll":60}).to_string()).unwrap();
            status_line("frozen",&crate::data::json(&root.join(".taskq/status.json")),now)
        };
        assert_eq!(line(now-30),"frozen: status <1m old · refresh 60s");
        assert!(health(&dir).contains("frozen: status <1m old · refresh 60s"));
        assert_eq!(line(now-120),"frozen: status 2m old · refresh 60s","two refreshes are not yet stale");
        assert_eq!(line(now-121),"frozen: status 2m old · refresh 60s · stale");
        assert_eq!(line(now-1500),"frozen: status 25m old · refresh 60s · stale");
        assert!(health(&dir).contains("frozen: status 25m old · refresh 60s · stale"));
        // § 4 Churn (#1905): a flagged row prints its counts; an unflagged one prints nothing
        fs::write(root.join(".taskq/status.json"),serde_json::json!({"at":now,"tasks":[{"n":7,"churn":{"notes":144,"spawns":64,"accepted":17}},{"n":8}]}).to_string()).unwrap();
        let h=health(&dir);
        assert!(h.contains("frozen: #7 churn: notes 144 · spawns 64 · accepted 17\n"),"{h}");
        assert!(!h.contains("#8 churn"),"{h}");
        assert_eq!(status_line("old",&Ok(Some(serde_json::json!({"at":now-200}))),now),"old: status 3m old · refresh 60s · stale","pre-#1766 status: § 9 default poll");
        fs::remove_file(root.join(".taskq/status.json")).unwrap();
        assert!(health(&dir).contains("frozen: status unknown (status.json missing)"));
        assert_eq!(status_line("p",&Ok(Some(serde_json::json!({"poll":60}))),now),"p: status unknown (no time)");
        fs::remove_dir_all(dir).unwrap();
    }

    // #1640 A2-A3: the store line showed WAL file size only, so a high-water file read as a full WAL. Retained protection:
    // live WAL bytes come from a read-only snapshot taken while the writer lock is held, and the read checkpoints nothing.
    #[test]
    fn profile_health_shows_live_wal_without_writer_lock_or_checkpoint() {
        let dir=std::env::temp_dir().join(format!("taskq-store-line-{}",std::process::id()));
        let _=fs::remove_dir_all(&dir);
        let (capture,_guard)=collecting(&dir);
        append_line(&dir.join("taskq.log"),r#"taskq-profile {"op":"spawn","us":1,"error":"none","at_ms":1}"#).unwrap();
        capture.flush().unwrap();
        let db=dir.join("telemetry.log/universal.sqlite3");
        let storage=||coreq::telemetry::Reader::open(&db).unwrap().storage().unwrap();
        let before=storage();let live=before["wal_live_bytes"].as_u64().unwrap();
        assert!(live>0,"{before}");
        let write=OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("telemetry.log/universal.sqlite3.write.lock")).unwrap();write.lock().unwrap();
        let line=store_line(&dir);
        drop(write);
        assert!(line.contains(&format!(" (live {:.1} MiB) of ",live as f64/1048576.0)),"{line}");
        assert!(line.starts_with("Telemetry store: database ")&&line.ends_with("budget within\n"),"{line}");
        assert!(line.chars().count()<=160,"{line}");
        assert_eq!(storage(),before,"profile health checkpointed or changed the store");
        drop(_guard);drop(capture);fs::remove_dir_all(dir).unwrap();
    }

    // Chain: producers enqueue without file I/O; the shared budget lock serializes the eventual commit.
    #[test]
    fn concurrent_appenders_keep_records() {
        let dir = std::env::temp_dir().join(format!("taskq-append-{}", std::process::id()));
        let _=fs::remove_dir_all(&dir);
        let (capture,_guard)=collecting(&dir);let path=dir.join("taskq.log");
        let held=log_lock(&path).unwrap();
        let dispatch=tracing::dispatcher::get_default(Clone::clone);
        std::thread::scope(|scope| {
            for _ in 0..2 {let dispatch=&dispatch;let path=&path;scope.spawn(move ||tracing::dispatcher::with_default(dispatch,|| {
                for _ in 0..100 {append_line(path,r#"taskq-profile {"op":"spawn","us":1,"error":"none","at_ms":1}"#).unwrap();}
            }));}
        });
        drop(held);capture.flush().unwrap();
        assert_eq!(read_log(&dir).unwrap().lines().count(),200);
        assert!(!dir.join("telemetry.log/telemetry.sqlite3").exists());
        assert!(append_line(&path,"not metadata").is_err());
        // #1297 A3: admission is not commit, and an occupied budget lock delays the batch instead of losing it
        // (was: lost 1, failed_batches 1). Retained protection: flush stays unacknowledged while held, then commits.
        // #1333: CoreQ's 10 s retry window starts at the writer's commit and its flush waits 10 s; a test stall over
        // ~1 s after admission let the window expire first (lost 1). The commit now waits in the blocking store write
        // lock after the budget lock: it outlasts the flush and commits on release, whatever the scheduling.
        let held=log_lock(&path).unwrap();
        append_line(&path,r#"taskq-profile {"op":"spawn","us":1,"error":"none","at_ms":1}"#).unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(5);
        while capture.health()["inflight"]!=1 {assert!(Instant::now()<until,"commit never met the held budget lock");std::thread::sleep(std::time::Duration::from_millis(5));}
        let write=OpenOptions::new().write(true).open(dir.join("telemetry.log/universal.sqlite3.write.lock")).unwrap();write.lock().unwrap();
        drop(held);
        assert!(capture.flush().unwrap_err().starts_with("telemetry flush unacknowledged ("));
        drop(write);
        capture.flush().unwrap();
        assert_eq!(capture.health()["lost"],0);assert_eq!(capture.health()["failed_batches"],0);
        let reader=coreq::telemetry::Reader::open(&dir.join("telemetry.log/universal.sqlite3")).unwrap();
        assert_eq!(reader.export_kinds(&["operation"],0,0,1024).unwrap()["events"].as_array().unwrap().len(),201);
        drop(reader);
        drop(dispatch);drop(_guard);drop(capture);fs::remove_dir_all(dir).unwrap();
    }

    // #1359 A3: a closed task's stage row without a host that another host owns was reread (issue and comments)
    // every pass, forever. Retained protection: the first pass still reads and leaves the row unknown, and an open
    // task's row is never repaired.
    #[test]
    fn repair_ended_reads_an_unowned_closed_row_once() {
        let dir = std::env::temp_dir().join(format!("taskq-repair-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (capture, _guard) = collecting(&dir);
        use crate::board::Board;
        let b = crate::board::Fake::default();
        let closed = b.create("Invented closed", "", &["taskq".into()]).unwrap();
        let a = b.comment(closed, "taskq: spawn worker codex:w other").unwrap();
        b.close(closed).unwrap();
        let open = b.create("Invented open", "", &["taskq".into()]).unwrap();
        let o = b.comment(open, "taskq: spawn worker codex:w mac").unwrap();
        for (task, attempt) in [(closed, a), (open, o)] {
            write_stage(&dir.join("taskq.log"), &Stage { project: Some("o/r".into()), host: None, open: true, stage: "build".into(), task, attempt,
                rt: "codex".into(), us: None, outcome: "unknown".into(), error: "none".into(), at_ms: Some(1), calls: None, tokens: None, model: None, output_tokens: None, input_tokens: None, cache_read_tokens: None, cache_write_tokens: None, unknown: Some("open-stage".into()) });
        }
        capture.flush().unwrap();
        let (listed, mut unowned) = (std::collections::HashSet::from([open]), std::collections::HashSet::new());
        let reads = || (b.state.borrow().reads, b.state.borrow().comment_reads);
        let mut counts = vec![];
        for _ in 0..2 {
            let before = reads();
            repair_ended(&dir, &dir, "o/r", "mac", &b, &listed, &mut unowned);
            counts.push((reads().0 - before.0, reads().1 - before.1));
        }
        assert_eq!(counts, [(1, 1), (0, 0)], "issue and comment reads per pass");
        assert_eq!(unresolved_samples(&crate::telemetry::report_samples(&dir, &["stage"]).unwrap()).unwrap().len(), 2, "nothing repaired");
        drop(_guard); drop(capture); fs::remove_dir_all(dir).unwrap();
    }

    // Chain: A2/A3 operation -> bounded log -> ranked report. Risk: lost failures, wrong ranks/quantiles,
    // unsafe input persisted, corrupt/missing data called healthy, or telemetry outgrows existing retention.
    #[test]
    fn retained_profile_report() {
        let dir = std::env::temp_dir().join(format!("taskq-profile-{}", std::process::id()));
        // A failed run leaves its directory; a reused process id must not inherit that log.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap(); let (_capture,_guard)=collecting(&dir);
        let path = dir.join("taskq.log");
        assert!(report(&dir).contains("unknown (no observations)"));
        fs::write(&path, "normal taskq diagnostic\n").unwrap();
        for (op, times, err) in [("spawn", vec![1000, 3000], "none"), ("forge.call", vec![2000, 2000], "http5xx")] {
            for us in times { append(&path, &Record { op: op.into(), us, error: err.into(), at_ms: Some(us), trace: None, counts: None }).unwrap(); }
        }
        // board operations (#1173): ranked by p95; failures keep only their class; no remaining reads unknown
        for (op, us, err, remaining) in [("list", 4000, None, Some(4900)), ("list", 1000, Some("HTTP 502 secret /host/path"), Some(4800)), ("claim", 2000, None, None)] {
            append_line(&path, &format!("{BOARD_PREFIX}{}\n", serde_json::to_string(&BoardOp { op: op.into(), us, outcome: if err.is_some() { "failed" } else { "ok" }.into(),
                error: err.map_or("none", error).into(), remaining, at_ms: Some(1) }).unwrap())).unwrap();
        }
        // #1572 defect: CI pending or an owner blocker counted as a failed publish; only the CI failure counts
        for p in [crate::board::Publish::Pending("CI pending".into()), crate::board::Publish::Blocked("merged, CI HTTP 404".into()), crate::board::Publish::Failed("CI failed".into())] {
            let (outcome, err) = p.outcome();
            append_line(&path, &format!("{BOARD_PREFIX}{}\n", serde_json::to_string(&BoardOp { op: "publish".into(), us: 1000, outcome: outcome.into(),
                error: err.map_or("none", error).into(), remaining: None, at_ms: Some(1) }).unwrap())).unwrap();
        }
        let text = report(&dir);
        assert!(text.contains("| publish | 3 | 1.000/1.000/1.000 | 1 | other:1 |"), "{text}");
        assert!(text.contains("| list | 2 | 1.000/4.000/4.000 | 1 | http5xx:1 | 4800 | unknown |\n| claim | 1 | 2.000/2.000/2.000 | 0 | none | unknown (not reported) |"), "{text}");
        assert!(text.contains("Slow (p95): spawn > forge.call"));
        assert!(text.contains("Failing (rate): forge.call > spawn"));
        assert!(text.contains("Unstable (variance): spawn > forge.call"));
        assert!(text.contains("1.000/3.000/3.000 | 2/0 | 0.00 | 1.000000"));
        assert!(text.contains("http5xx:2"));
        assert!(text.contains("1000..3000 Unix ms") && text.contains("switch off for this run"));
        assert_eq!(error("HTTP 500 private token /host/path"), "http5xx");
        assert_eq!(measure_at(Some(&path), true, "spawn", || Err::<(), _>("private token /host/path")), Err("private token /host/path"));
        let log = read_log(&dir).unwrap();
        assert!(!log.contains("private") && !log.contains("/host/path"));
        let r: Record = serde_json::from_str(log.lines().last().unwrap().strip_prefix(PREFIX).unwrap()).unwrap();
        assert_eq!(r.error, "other");
        assert_eq!(r.trace.unwrap().parent, 0);
        measure_at(Some(&path), true, "dispatch", || measure_at(Some(&path), true, "spawn", || Ok::<_, String>(()))).unwrap();
        let log = read_log(&dir).unwrap();
        let r: Vec<Record> = log.lines().filter_map(|l| l.strip_prefix(PREFIX)).map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(r[r.len()-2].trace.as_ref().unwrap().parent, r.last().unwrap().trace.as_ref().unwrap().span);
        assert!(std::panic::catch_unwind(|| measure_at(Some(&path), true, "spawn", || -> Result<(), String> { panic!("private panic") })).is_err());
        assert!(report(&dir).contains("panic:1"));
        assert_eq!(PARENT.get(), 0);
        assert_eq!(measure_at(Some(&path.join("unwritable")), false, "spawn", || Ok::<_, String>(7)).unwrap(), 7);
        for corrupt in ["{", "{\"op\":\"spawn\",\"us\":2,\"error\":\"secret\"}"] {
            let bad_dir = dir.join(format!("bad-{}", corrupt.len()));
            fs::create_dir_all(&bad_dir).unwrap();
            let bad = format!("{PREFIX}{corrupt}\n");
            fs::write(bad_dir.join("taskq.log"), &bad).unwrap();
            assert!(report(&bad_dir).starts_with("Profile: unknown"));
            assert_eq!(fs::read_to_string(bad_dir.join("taskq.log")).unwrap(), bad);
        }
        let held = log_lock(&path).unwrap();
        assert!(log_lock(&path).is_err());
        assert_eq!(measure_at(Some(&path), false, "spawn", || Ok::<_, String>(7)).unwrap(), 7);
        drop(held);
        let before = read_log(&dir).unwrap().lines().filter(|l| l.starts_with(PREFIX)).count();
        // Concurrent producer admission is covered by concurrent_appenders_keep_records; rotation shares the budget lock.
        OpenOptions::new().append(true).open(&path).unwrap().write_all("normal log\n".repeat(crate::data::LOG_MAX as usize / 11 + 1).as_bytes()).unwrap();
        assert!(crate::data::trim_tail(&path, crate::data::LOG_MAX));
        append(&path, &Record { op: "spawn".into(), us: 1, error: "none".into(), at_ms: Some(1), trace: None, counts: None }).unwrap();
        let records = read_log(&dir).unwrap().lines().filter(|l| l.starts_with(PREFIX)).count();
        assert_eq!(records, before + 1);
        assert!(!report(&dir).starts_with("Profile: unknown"));
        drop(_guard);drop(_capture);fs::remove_dir_all(dir).unwrap();
    }

    // Chain: #1147 attempt stage -> runtime usage from the attempt log -> bounded log -> per-runtime report.
    // Risk: tokens double-counted or invented, failure text or paths persisted, unknown usage shown as zero.
    #[test]
    fn attempt_stage_trace() {
        let dir = std::env::temp_dir().join(format!("taskq-stage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap(); let (_capture,_guard)=collecting(&dir);
        let (path, attempt) = (dir.join("taskq.log"), dir.join("attempt.log"));
        assert!(report(&dir).contains("Attempt stages: unknown (no observations)"));
        let claude = r#"{"type":"assistant","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5,"output_tokens":3}}}"#;
        let codex = r#"{"type":"turn.completed","usage":{"input_tokens":700,"cached_input_tokens":600,"output_tokens":9}}"#;
        // one Claude message spans several lines with one id; a second id is a second call; lines before launch are skipped
        let before = format!("{}\n", claude.replace("m1", "m0"));
        fs::write(&attempt, format!("{before}{claude}\n{claude}\n{}\n", claude.replace("m1", "m2"))).unwrap();
        // gap G3: model of most calls and output tokens; Codex reports output tokens but no model; a path-like
        // model id or a call without output tokens stays unknown, never a guess or a smaller sum
        let got = |log: &Path, pos, rt| match crate::runtime::usage(log, pos, rt) { Usage::Known { calls, tokens, model, output, .. } => (calls, tokens, model, output), _ => panic!("{rt}") };
        // #1647: context tokens split into uncached input, cache read and cache write, which sum to `tokens`;
        // Codex's input includes its cached tokens and Codex reports no cache writes
        let split = |log: &Path, pos, rt| match crate::runtime::usage(log, pos, rt) { Usage::Known { input, cache_read, cache_write, .. } => (input, cache_read, cache_write), _ => panic!("{rt}") };
        assert_eq!(got(&attempt, before.len() as u64, "claude"), (Some(2), Some(230), Some("claude-opus-5-5".into()), Some(6)));
        assert_eq!(split(&attempt, before.len() as u64, "claude"), (Some(20), Some(200), Some(10)));
        let odd = dir.join("odd.log");
        let bad = claude.replace("claude-opus-5-5", "/home/x/model");
        fs::write(&odd, format!("{bad}\n{}\n", bad.replace("m1", "m2").replace(r#","output_tokens":3"#, ""))).unwrap();
        assert_eq!(got(&odd, 0, "claude"), (Some(2), Some(230), None, None));
        fs::write(&odd, format!("{codex}\n")).unwrap();
        assert_eq!(got(&odd, 0, "codex"), (None, Some(700), None, Some(9)));
        assert_eq!(split(&odd, 0, "codex"), (Some(100), Some(600), Some(0)));
        // a Codex count whose cached part exceeds its input is inconsistent: input unknown, never wrapped
        fs::write(&odd, format!("{}
", codex.replace("700", "500"))).unwrap();
        assert_eq!(split(&odd, 0, "codex"), (None, Some(600), Some(0)));
        let ms = |m| Some(std::time::Duration::from_millis(m));
        stage_at(Some(&path), "build", 7, 1, "claude", ms(9), "ok", "none", crate::runtime::usage(&attempt, before.len() as u64, "claude"));
        fs::write(&attempt, format!("{codex}\n{codex}\n")).unwrap();
        stage_at(Some(&path), "build", 8, 2, "codex", ms(5), "ask", "none", crate::runtime::usage(&attempt, 0, "codex"));
        stage_at(Some(&path), "build", 9, 3, "codex", ms(1), "failed", "exit", crate::runtime::usage(&dir.join("missing"), 0, "codex"));
        stage_at(Some(&path), "review", 7, 4, "codex-app", ms(2), "ok", "none", crate::runtime::usage(&attempt, 0, "codex-app"));
        stage_at(Some(&path), "claim", 7, 1, "private-rt", ms(3), "failed", class("HTTP 502 secret /host/path"), Usage::NoModel);
        // a Claude call with empty usage keeps the call count and makes tokens unknown, never 0; an adopted child is untimed
        fs::write(&attempt, format!("{}\n", claude.replace(r#""usage":{"input_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5,"output_tokens":3}"#, r#""usage":{}"#))).unwrap();
        assert_eq!(split(&attempt, 0, "claude"), (None, None, None));
        stage_at(Some(&path), "review", 7, 5, "claude", ms(4), "ok", "none", crate::runtime::usage(&attempt, 0, "claude"));
        stage_at(Some(&path), "review", 7, 6, "claude", None, "ok", "none", Usage::Unknown("adopted"));
        // a child stopped after its result or requeue keeps that outcome; a live child exiting without one failed;
        // a stop without hand-in (owner requeue) is a cancellation; a hand-in `fold` ignored (invalid SHA, a repeated
        // op id, a result in `review`) is none
        let e = |id, body: &str| crate::core::Entry { id, body: body.into(), edited: false };
        let spawn = e(10, "taskq: spawn worker claude:s1 mac");
        let sha = "a".repeat(40);
        let hand = |kind: &str, arg: &str| vec![spawn.clone(), e(11, &format!("taskq: {kind} {arg}attempt 10
Changed: x"))];
        let (result, requeue, owner) = (hand("result", &format!("{sha} ")), hand("requeue", ""), vec![spawn.clone(), e(11, "taskq: requeue
Reason: owner")]);
        let invalid = hand("result", "abc ");
        let op = |id, body: &str| e(id, &format!("{body}
op-id: x"));
        let repeated = vec![spawn.clone(), op(11, "taskq: ask attempt 10
Done: x"), e(12, "taskq: answer 1"), op(13, &format!("taskq: result {sha} attempt 10
Changed: x"))];
        let review = vec![e(9, "taskq: spawn worker claude:s0 mac"), e(10, &format!("taskq: result {sha} attempt 9
Changed: x")),
            e(11, "taskq: spawn reviewer codex:- win"), e(12, &format!("taskq: result {sha} attempt 11
Changed: x"))];
        assert_eq!(crate::core::stage_outcome(7, Ok((review.as_slice(), Some(true))), 11, 11), ("failed", "exit"));
        // an issue that left the board read is read back: the reviewer's accepted close is ok; a label removal or an
        // owner close (no reviewer hand-in) and an unreadable read-back are unknown with reason, never ok
        let closed = |by: &str| [&review[..3], &[e(13, &format!("taskq: close {by}\nlooks fine"))]].concat();
        let (own, other, removed) = (closed("attempt 11"), closed(""), review[..3].to_vec());
        for (log, want) in [(&own, ("ok", "none")), (&other, ("unknown", "left-board")), (&removed, ("unknown", "left-board"))] {
            assert_eq!(crate::core::stage_outcome(7, Ok((log.as_slice(), None)), 11, 11), want, "{:?}", log.last().map(|e| &e.body));
        }
        assert_eq!(crate::core::stage_outcome(7, Err("unreadable"), 11, 11), ("unknown", "unreadable"));
        for (log, live, want) in [(&result, false, ("ok", "none")), (&requeue, false, ("retried", "none")), (&owner, false, ("cancelled", "none")),
            (&owner, true, ("failed", "exit")), (&invalid, true, ("failed", "exit")), (&repeated, true, ("ask", "none"))] {
            assert_eq!(crate::core::stage_outcome(7, Ok((log.as_slice(), Some(live))), 10, 10), want, "{:?}", log.last().map(|e| &e.body));
        }
        stage_at(Some(&path), "review", 7, 11, "codex", ms(6), "unknown", "left-board", Usage::Unknown("log-unreadable"));
        // `close` times the whole command from entry: an unreadable task, a gate refusal, a stale revision, a CLI verdict refusal, a non-publication close
        // and a publication each record one `close` stage (the publication also `checks`)
        use crate::board::Board;
        let staged = || STAGED.with(|v| std::mem::take(&mut *v.borrow_mut())).join("; ");
        let (b, c) = (crate::board::Fake::default(), crate::tests::cfg());
        let gated = crate::Config { delivery: "review-pr".into(), ..crate::tests::cfg() };
        let t = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        let close = |c: &crate::Config, op: &crate::dispatch::Op| crate::dispatch::close_one(&b, c, t, "ok", op).ok();
        assert!(!close(&c, &Default::default()));
        b.comment(t, &crate::core::entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = crate::core::fold(t, &crate::board::entries(&b, t).unwrap(), false).attempt.unwrap();
        b.comment(t, &crate::core::entry("result", &[&sha], Some(a), "")).unwrap();
        b.comment(t, &crate::core::entry("spawn", &["reviewer", "codex:-", "mac"], None, "")).unwrap();
        let a = crate::core::fold(t, &crate::board::entries(&b, t).unwrap(), false).attempt.unwrap();
        assert!(!close(&gated, &Default::default()));
        assert!(!close(&c, &crate::dispatch::Op { revision: Some(1), ..Default::default() }));
        // the CLI's verdict refusal and unreadable task are inside the stage too
        let cli = |n, text: &str| crate::dispatch::close_checked(&b, &c, n, text, &Default::default(), &|i, t| crate::close_check(text, i, t, b.defer_publication(&c.publish))).ok();
        assert!(!cli(t, "no verdict"));
        assert!(!cli(999, ""));
        assert!(close(&c, &Default::default()));
        assert!(!crate::dispatch::close_one(&b, &c, 999, "ok", &Default::default()).ok());
        assert_eq!(staged(), format!("close {t} 0 other failed other; close {t} {a} codex failed other; close {t} {a} codex failed other; \
            close {t} {a} codex failed other; close 999 0 other failed other; \
            checks {t} {a} codex ok none; close {t} {a} codex ok none; close 999 0 other failed other"));
        let log = read_log(&dir).unwrap();
        assert!(!log.contains("secret") && !log.contains("/host/path") && !log.contains("private-rt"));
        let text = report(&dir);
        assert!(text.contains("Slow stages (p95): build/claude > review/codex > build/codex > review/claude > claim/other > review/codex-app"), "{text}");
        assert!(text.contains("Failing stages (rate): claim/other > build/codex"), "{text}");
        assert!(text.contains("| build | claude | 1 | 9.000/9.000/9.000 | ok:1 | 2 | 230 | unknown |"), "{text}");
        assert!(text.contains("| build | codex | 2 | 1.000/5.000/5.000 | ask:1, failed exit:1 | unknown (log-unreadable, not-reported) | 1400 (1 of 2 known; log-unreadable) |"), "{text}");
        assert!(text.contains("| review | codex-app | 1 | 2.000/2.000/2.000 | ok:1 | unknown (app-turn) | unknown (app-turn) |"), "{text}");
        assert!(text.contains("| review | claude | 2 | 4.000/4.000/4.000 (1 of 2 timed; adopted) | ok:2 | 1 (1 of 2 known; adopted) | unknown (adopted, no-usage) |"), "{text}");
        assert!(text.contains("| claim | other | 1 | 3.000/3.000/3.000 | failed http5xx:1 | none (no model) | none (no model) |"), "{text}");
        assert!(text.contains("| review | codex | 1 | 6.000/6.000/6.000 | unknown left-board:1 |"), "{text}");
        fs::write(&path, format!("{log}{STAGE_PREFIX}{{\"stage\":\"lunch\"}}\n")).unwrap();
        assert!(report(&dir).contains("legacy telemetry awaits service import"));
        assert_eq!(fs::read_to_string(&path).unwrap(),format!("{log}{STAGE_PREFIX}{{\"stage\":\"lunch\"}}\n"));
        drop(_guard);drop(_capture);fs::remove_dir_all(dir).unwrap();
    }
    // Chain: #1174 retained records -> `profile export` (header, one row per observation) -> `profile tasks` and `profile board`.
    // Risk: a record lost or invented, columns drift for DashQ, one task number merged across projects, unknown shown as a number.
    #[test]
    fn export_then_reports() {
        let t = |p: Option<&str>, task, from: &str, to: &str, prev_s, rt: Option<&str>, host: Option<&str>, at| format!("{TRANSITION_PREFIX}{}\n", serde_json::to_string(&Transition {
            project: p.map(Into::into), task, entry: at, from: from.into(), to: to.into(), prev_s, rt: rt.map(Into::into), host: host.map(Into::into), at_ms: Some(at), counts: None }).unwrap());
        let b = |op: &str, us, err: Option<&str>, remaining, at| format!("{BOARD_PREFIX}{}\n", serde_json::to_string(&BoardOp { op: op.into(), us,
            outcome: if err.is_some() { "failed" } else { "ok" }.into(), error: err.unwrap_or("none").into(), remaining, at_ms: Some(at) }).unwrap());
        let log = [
            "normal taskq diagnostic\n".to_string(),
            t(Some("o/a"), 5, "ready", "doing", Some(10), Some("claude"), Some("mac"), 100),
            t(Some("o/a"), 5, "doing", "review", Some(100), Some("claude"), Some("mac"), 110),
            t(Some("o/b"), 5, "doing", "review", Some(300), Some("codex"), Some("win"), 120),
            t(None, 6, "doing", "ask", None, Some("claude"), Some("mac"), 130),
            t(Some("o/a"), 7, "ready", "doing", Some(999), None, None, 1),
            t(Some("o/a"), 8, "doing", "review", Some(7), Some("claude"), Some("lin"), 131),
            t(Some("o/a"), 8, "doing", "review", None, Some("claude"), Some("lin"), 132),
            t(Some("o/b"), 9, "ask", "ready", None, Some("codex"), Some("win"), 133),
            // duplicates by project, task and entry count once, a known time winning; the same task and entry in another project is its own move
            t(Some("o/a"), 5, "doing", "review", Some(100), Some("claude"), Some("mac"), 110),
            t(Some("o/a"), 10, "ready", "doing", None, Some("codex"), Some("lin"), 134),
            t(Some("o/a"), 10, "ready", "doing", Some(50), Some("codex"), Some("lin"), 134),
            t(Some("o/b"), 5, "ready", "doing", Some(20), Some("codex"), Some("win"), 110),
            b("list", 4000, None, Some(4900), 100), b("list", 2000, Some("http5xx"), Some(4100), 101), b("claim", 1000, None, None, 102),
            format!("{BOARD_PREFIX}{{\"op\":\"lunch\"}}\n"),
            format!("{PREFIX}{}\n", serde_json::to_string(&Record { op: "spawn".into(), us: 7, error: "none".into(), at_ms: Some(140), trace: None, counts: None }).unwrap()),
            format!("{STAGE_PREFIX}{}\n", serde_json::to_string(&Stage { host: None, open: false, project: Some("o/a".into()), stage: "build".into(), task: 5, attempt: 9, rt: "codex".into(),
                us: Some(5), outcome: "ok".into(), error: "none".into(), at_ms: Some(150), calls: None, tokens: Some(3), input_tokens: Some(1), cache_read_tokens: Some(2), cache_write_tokens: Some(0), model: None, output_tokens: Some(2), unknown: None }).unwrap()),
        ].concat();
        let text = export(&log, Some(100));
        let mut lines = text.lines();
        assert_eq!(lines.next().unwrap(), format!(r#"{{"schema":3,"columns":{},"since_ms":100,"window_ms":[100,150],"samples":16,"corrupt":1}}"#, serde_json::to_string(&COLUMNS).unwrap()));
        let first: serde_json::Map<String, serde_json::Value> = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(first.keys().collect::<Vec<_>>(), COLUMNS.iter().collect::<Vec<_>>());
        assert_eq!(serde_json::Value::Object(first), serde_json::json!({"kind":"transition","at_ms":100,"project":"o/a","task":5,"attempt":null,"entry":100,"name":null,
            "from":"ready","to":"doing","runtime":"claude","host":"mac","us":null,"prev_s":10,"outcome":null,"error":null,"remaining":null,"calls":null,"tokens":null,"input_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"model":null,"output_tokens":null,"unknown":null}));
        assert!(text.contains(r#""kind":"stage","at_ms":150,"project":"o/a","task":5,"attempt":9,"entry":null,"name":"build""#), "{text}");
        assert!(text.contains(r#""tokens":3,"input_tokens":1,"cache_read_tokens":2,"cache_write_tokens":0,"model":null,"output_tokens":2"#), "{text}");
        assert!(!text.contains("999"), "a record before the window is not exported");
        let tasks = render_tasks(&text).unwrap();
        assert!(tasks.contains("Window: 100..150 Unix ms (since 100); 16 samples, 1 corrupt skipped; earlier/pruned history unknown
Transitions: 9 unique of 11 rows"), "{tasks}");
        assert!(tasks.contains("| doing | codex | win | 1 | 300.000/300.000/300.000 |
| doing | claude | mac | 2 | 100.000/100.000/100.000 (1 of 2 known; no entry time) |
| ready | codex | lin | 1 | 50.000/50.000/50.000 |
| ready | codex | win | 1 | 20.000/20.000/20.000 |
| ready | claude | mac | 1 | 10.000/10.000/10.000 |"), "{tasks}");
        // the duplicate o/a #5 entry counts once (not 2, 200 s); o/a #10 keeps its known time; o/b #5 entry 110 stays its own
        assert!(tasks.contains("| o/b | #5 | doing | codex | win | 1 | 300 | 300.000/300.000/300.000 |
| o/a | #5 | doing | claude | mac | 1 | 100 | 100.000/100.000/100.000 |
| o/a | #10 | ready | codex | lin | 1 | 50 | 50.000/50.000/50.000 |
| o/b | #5 | ready | codex | win | 1 | 20 | 20.000/20.000/20.000 |
"), "{tasks}");
        assert!(tasks.contains("| o/a | #5 | ready | claude | mac | 1 | 10 | 10.000/10.000/10.000 |
| o/a | #8 | doing | claude | lin | 2 | 7 partial (1 of 2 known) | 7.000/7.000/7.000 (1 of 2 known; no entry time) |
| o/b | #9 | ask | codex | win | 1 | unknown (no entry time) | unknown (no entry time) |
| unknown (not recorded) | #6 | doing | claude | mac | 1 | unknown (no entry time) | unknown (no entry time) |"), "{tasks}");
        assert!(tasks.contains("| ready | claude | mac | 1 | 10.000/10.000/10.000 |
| doing | claude | lin | 2 | 7.000/7.000/7.000 (1 of 2 known; no entry time) |
| ask | codex | win | 1 | unknown (no entry time) |"), "unknown-only state ranks last: {tasks}");
        assert!(!tasks.contains("| 0 |"), "unknown never reads zero: {tasks}");
        let board = render_board(&text).unwrap();
        assert!(board.contains("Slow (p95): list > claim\nFailing (rate): list > claim\nRate-limit use (lowest remaining): list\n"), "{board}");
        assert!(board.contains("| claim | 1 | 1.000/1.000/1.000 | 0.00 | none | unknown (not reported) |\n| list | 2 | 2.000/4.000/4.000 | 50.00 | http5xx:1 | 4100 |"), "{board}");
        let empty = export("", None);
        assert!(render_tasks(&empty).unwrap().contains("Window: unknown (no timestamps) (all retained); 0 samples") && render_board(&empty).unwrap().contains("unknown (no observations)"));
        assert_eq!(since_ms("2h", 10_000_000), Ok(2_800_000));
        assert!(since_ms("2w", 1).is_err() && since_ms("h", 1).is_err() && since_ms("1日", 1).is_err() && since_ms("", 1).is_err());
        assert!(view(Path::new("/nonexistent-taskq"), "lunch", None).unwrap_err().contains("unknown profile view"));
    }

    #[test]
    fn watch_flags_the_pm_thresholds() {
        // Risk (#1910): a PM telemetry pass that misses a stall, a growing error or a board failure, or flags a healthy row as one.
        let now=1_800_000_000;
        let row=|v:serde_json::Value|stall(&v,now);
        assert_eq!(row(serde_json::json!({"n":1,"state":"doing","updated":now-3601})).as_deref(),Some("#1 doing, no board event for 1h"));
        assert_eq!(row(serde_json::json!({"n":2,"state":"doing","updated":now-3600})),None);
        assert_eq!(row(serde_json::json!({"n":3,"state":"ready","since":now-901,"admission":{"code":"capacity"}})).as_deref(),Some("#3 ready for 15m (admission capacity)"));
        assert_eq!(row(serde_json::json!({"n":4,"state":"ask","since":now-1801})).as_deref(),Some("#4 ask unanswered for 30m"));
        assert_eq!(row(serde_json::json!({"n":5,"state":"review","publication":{"since":now-1801}})).as_deref(),Some("#5 merge queue for 30m"));
        // A5: healthy execution observations must not hide a separate publication stall.
        let mut review=serde_json::json!({"n":5,"state":"review","attempt":1,"updated":now-7200,
            "heartbeat":{"observed":now,"ttl_s":300,"freshness":"fresh","executor":{"state":"active","observed":now},"progress":{"observed":now,"status":"quiet"}},
            "publication":{"since":now-1800}});
        assert_eq!(row(review.clone()),None);
        review["publication"]["since"]=serde_json::json!(now-1801);
        assert_eq!(row(review.clone()).as_deref(),Some("#5 merge queue for 30m"));
        review["heartbeat"]["progress"]["status"]=serde_json::json!("overdue");
        review["heartbeat"]["progress"]["reason"]=serde_json::json!("tool progress overdue");
        assert_eq!(row(review.clone()).as_deref(),Some("#5 tool progress overdue"),"task health keeps priority over queue age");
        review["state"]=serde_json::json!("doing");review["heartbeat"]["progress"]["status"]=serde_json::json!("quiet");
        assert_eq!(row(review),None,"quiet work with a heartbeat does not fall back to board inactivity");
        assert_eq!(row(serde_json::json!({"n":6,"state":"later","until":"2000-01-01T00:00:00Z"})).as_deref(),Some("#6 later past its until 2000-01-01T00:00:00Z"));
        assert_eq!(row(serde_json::json!({"n":7,"state":"later","until":"2999-01-01T00:00:00Z"})),None);
        assert_eq!(row(serde_json::json!({"n":8,"state":"held","stuck":true,"age_s":7200})).as_deref(),Some("#8 stuck in held for 2h"));
        let counts=BTreeMap::from([(("a".into(),"exit".into()),[0,2]),(("b".into(),"timeout".into()),[3,4]),(("c".into(),"exit".into()),[5,5]),(("d".into(),"exit".into()),[2,0])]);
        assert_eq!(action_lines(&counts),["a exit: 2 (new)","b timeout: 4 (previous 3)"]);
        let events=format!("{} #1 close a
{} #2 close b
{} #3 failed c exit
{} #4 gone d
not an event
",now-7200,now-60,now-30,now-10);
        let tasks=[serde_json::json!({"created":now-100}),serde_json::json!({"created":now-100}),serde_json::json!({"created":now-9000})];
        let (line,health)=cadence("o/a",&events,&tasks,now-3600);
        assert_eq!(line,"o/a: closed 1 · new 2 · net +1 · open 3");
        assert_eq!(health,["o/a #3 failed","o/a #4 gone"]);
        let call=|name:&str,outcome:&str,remaining:Option<u64>|Row {kind:"board".into(),name:Some(name.into()),outcome:Some(outcome.into()),remaining,..Default::default()};
        let mut rows:Vec<Row>=(0..99).map(|_|call("list","ok",Some(4000))).collect();
        rows.push(call("list","failed",Some(450)));
        rows.push(call("claim","failed",None));
        let board=board_watch(&rows);
        assert_eq!(board,"Board: 101 calls · failed 2 (1.98%) · lowest remaining 450
- claim failed 1 of 1 (100.00%)
- rate limit: remaining under 500
","list at 1.00% stays under the 1% flag: {board}");
        assert!(board_watch(&[]).contains("unknown"));
    }

