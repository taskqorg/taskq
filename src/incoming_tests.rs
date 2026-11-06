    use super::*;
    use crate::board::Fake;

    // § 4 budget / I3: a cold scan or one unavailable source must not replay a month of per-issue reads.
    #[test]
    fn collection_bounds_cold_restart_and_failed_source_requests() {
        let reads=std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let unavailable=std::rc::Rc::new(std::cell::Cell::new(true));
        let fail=unavailable.clone();
        let requests=reads.clone();
        let board=crate::board::replying(&crate::board::github::KIND, move |_,url| {
            requests.borrow_mut().push(url.to_string());
            if url.contains("/events?") {
                if url.contains("/issues/1/") && fail.get() {return Err("unavailable".into());}
                return Ok(if url.contains("/issues/2/") {json!([{"id":72,"event":"labeled","label":{"name":"taskq"},"created_at":crate::time::stamp(100)}])} else {json!([])});
            }
            assert!(url.contains("/issues?"), "unexpected request: {url}");
            Ok(if !url.ends_with(&crate::time::stamp(200-MONTH)) {json!([])} else {Value::Array((1..=1006).map(|n|json!({"number":n,"title":"draft","state":"closed",
                "author_association":"OWNER","updated_at":crate::time::stamp(100),"labels":[]})).collect())
            })
        });
        let mut h=History::default();
        h.refresh(board.as_ref(),200);
        assert_eq!(reads.borrow().iter().filter(|u|u.contains("/events?")).count(),5,
            "cold collection must bound event-source requests per pass");
        assert!(h.through.is_none(),"a bounded partial scan cannot prove a complete zero");
        assert_eq!(h.json(&board.url(0))["events"],json!([[100,"new","72",2]]));
        let mut now=300;
        // Simulate a restart after every bounded batch; negatives and unvisited sources survive too.
        while h.pending.len()>1 {
            let snapshot:Value=serde_json::from_str(&h.json(&board.url(0)).to_string()).unwrap();
            h=History::seed(&snapshot,&board.url(0));
            reads.borrow_mut().clear();
            h.refresh(board.as_ref(),now);
            let requests=reads.borrow();
            // #2083: between scans of every issue, which ask from the last one, the `taskq` scan repeats from the newest update
            let task=format!("/issues?state=all&labels=taskq&per_page=100&since={}",crate::time::stamp(70));
            let full=format!("/issues?state=all&per_page=100&since={}",crate::time::stamp(now-FULL_SCAN-30));
            assert!(requests[0].ends_with(&task) || requests[0].ends_with(&full),
                "an unresolved source cannot reset the list cursor, which repeats from the newest update (#1985): {}",requests[0]);
            assert!(requests.iter().filter(|u|u.contains("/events?")).count()<=5);
            assert!(!requests.iter().any(|u|u.contains("/issues/2/")),"a resolved source is not read again after restart");
            assert!(now<21000,"failures cannot starve unvisited sources");
            now+=100;
        }
        assert_eq!(h.seen.len(),1005);
        assert!(h.through.is_none());
        h=History::seed(&h.json(&board.url(0)),&board.url(0));
        reads.borrow_mut().clear();
        h.refresh(board.as_ref(),now);
        assert_eq!(reads.borrow().len(),2,"only the incremental list and failed source are retried");
        assert!(h.missing.iter().any(|s|s.contains("unavailable")));
        unavailable.set(false);
        h.refresh(board.as_ref(),now+100);
        assert!(h.through.is_some_and(|t|t>now+100-FULL_SCAN) && h.through==h.full,"coverage runs through the last scan of every issue");
        h=History::seed(&h.json(&board.url(0)),&board.url(0));
        reads.borrow_mut().clear();
        h.refresh(board.as_ref(),now+200);
        assert_eq!(reads.borrow().len(),1,"a settled restart reads no event histories");
        assert!(h.missing.is_empty());
    }

    // #1960 defect: a source whose complete history could not be used was read again on every pass, growing failed reads.
    // Chain: §4 admission records. Risk kept: a settled source proves zero, or a transient failure stops retrying.
    #[test]
    fn permanent_source_failure_settles_until_update_and_transient_retries() {
        let board=Fake::default();
        board.state.borrow_mut().clock=99;
        let bad=board.create("bad","",&["taskq".into()]).unwrap();
        let flaky=board.create("flaky","",&["taskq".into()]).unwrap();
        board.state.borrow_mut().admissions.insert(bad,Ok(Some((0,100))));
        board.state.borrow_mut().admissions.insert(flaky,Err(crate::board::Error::http(502, "HTTP 502")));
        let mut h=History::default();
        h.refresh(&board,200);
        assert!(h.failed.contains_key(&bad));assert!(!h.pending.contains_key(&bad));
        assert!(h.pending.contains_key(&flaky));
        // A good answer is not read until the issue updates, and a restart keeps the settled state.
        board.state.borrow_mut().admissions.insert(bad,Ok(Some((71,100))));
        h=History::seed(&h.json(&board.url(0)),&board.url(0));
        for now in [300,400] {
            h.refresh(&board,now);
            assert_eq!(h.pending[&flaky].2,now,"a transient failure retries every pass");
            assert!(h.missing.iter().any(|s|s==&format!("#{bad}: invalid first-admission provenance")),"{:?}",h.missing);
            assert!(h.through.is_none(),"a failed source never proves zero");
        }
        assert!(h.json(&board.url(0))["events"].as_array().unwrap().is_empty());
        board.state.borrow_mut().admissions.insert(flaky,Ok(Some((72,100))));
        board.state.borrow_mut().clock=449;
        board.comment(bad,"edit").unwrap();
        h.refresh(&board,200+FULL_SCAN);
        assert!(h.failed.is_empty()&&h.pending.is_empty()&&h.missing.is_empty(),"{:?}",h.missing);
        assert_eq!(h.through,Some(200+FULL_SCAN));
        assert_eq!(h.json(&board.url(0))["events"].as_array().unwrap().len(),2);
    }

    // UX30-21: a stalled history API cannot delay the service heartbeat or admission.
    #[test]
    fn history_reader_tick_never_waits_for_collection() {
        let (request,requests)=std::sync::mpsc::sync_channel(1);
        let (responses,response)=std::sync::mpsc::channel();
        let mut reader=Reader {request,response,busy:false,value:Value::Null,every:std::time::Duration::ZERO,asked:None};
        let began=std::time::Instant::now();
        for _ in 0..1000 {assert_eq!(reader.tick(),Value::Null);}
        assert!(began.elapsed()<std::time::Duration::from_secs(1));
        requests.try_recv().unwrap();
        responses.send(json!({"schema":1,"end":100,"missing":[]})).unwrap();
        assert_eq!(reader.tick()["end"],100);
        assert!(reader.busy);
        drop(responses);
        assert_eq!(reader.tick()["missing"],json!(["admission reader exited"]));
        assert_eq!(reader.value["end"],100,"a dead collector retains bounded historical evidence");
    }

    // #2352: each pass of each project asked for another scan, so one task change billed this reader at every host.
    // Risk kept: the first scan runs at once, and an unresolved source still reads each pass.
    #[test]
    fn reader_scans_on_its_own_cadence_unless_a_source_is_unresolved() {
        let (request,requests)=std::sync::mpsc::sync_channel(1);
        let (responses,response)=std::sync::mpsc::channel();
        let mut reader=Reader {request,response,busy:false,value:Value::Null,every:READ_EVERY,asked:None};
        let mut scans=|reader:&mut Reader,reply:Value|{
            let mut n=0;
            for _ in 0..60 {
                reader.tick();
                if requests.try_recv().is_ok() {n+=1;responses.send(reply.clone()).unwrap();}
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            n
        };
        assert_eq!(scans(&mut reader,json!({"schema":1,"missing":[]})),1,"one scan in a burst of passes: before this fix, 60");
        reader.asked=None;
        assert!(scans(&mut reader,json!({"schema":1,"missing":["#1: pending"]}))>1,"an unresolved source reads each pass");
    }

    #[test]
    fn corrupt_or_foreign_source_never_proves_zero() {
        let valid=json!({"schema":1,"board":"u/0","start":1,"end":2,"events":[],"missing":[]});
        assert!(super::valid(&valid,"u/0"));
        assert!(!super::valid(&valid,"other/0"));
        for (key,value) in [("schema",json!(2)),("events",Value::Null),("start",json!(3)),("missing",json!([1]))] {
            let mut bad=valid.clone();bad[key]=value;assert!(!super::valid(&bad,"u/0"));
        }
        let mut bad=valid.clone();bad["events"]=json!([[1,"new","",7]]);assert!(!super::valid(&bad,"u/0"));
    }

    // I3 / R12: cache compatibility must preserve board provenance and never skip unread history.
    #[test]
    fn cache_loss_corruption_and_clock_gaps_keep_coverage_unknown() {
        let board=Fake::default();
        board.state.borrow_mut().clock=99;
        let task=board.create("task","",&["taskq".into()]).unwrap();
        board.state.borrow_mut().admissions.insert(task,Ok(Some((71,100))));
        let mut h=History::default();
        h.refresh(&board,200);
        let snapshot=h.json(&board.url(0));
        let mut legacy=snapshot.clone();legacy.as_object_mut().unwrap().remove("cache");
        let mut corrupt=snapshot.clone();corrupt["cache"]["seen"]=json!({"0":[100,null]});
        for v in [legacy,corrupt] {
            let mut rebuilt=History::seed(&v,&board.url(0));
            assert!(rebuilt.listed.is_none(),"legacy or corrupt scan progress requires a rebuild");
            assert_eq!(rebuilt.json(&board.url(0))["events"],snapshot["events"],"independent valid event provenance survives");
            rebuilt.refresh(&board,300);
            assert!(rebuilt.missing.is_empty());
            assert_eq!(rebuilt.through,Some(300));
        }
        let foreign=History::seed(&snapshot,"other board");
        assert!(foreign.through.is_none());assert!(foreign.seen.is_empty());
        let mut restarted=History::seed(&snapshot,&board.url(0));
        restarted.refresh(&board,199);
        assert_eq!(restarted.through,Some(200));
        assert!(restarted.missing[0].contains("clock"));
        // Recent updates wait for settled event history, including after a cache round trip.
        let scan=200+FULL_SCAN;
        board.state.borrow_mut().clock=scan-11;
        let held=board.create("held","",&["q-held".into()]).unwrap();
        restarted.refresh(&board,scan);
        assert_eq!(restarted.through,Some(200));
        restarted=History::seed(&restarted.json(&board.url(0)),&board.url(0));
        restarted.refresh(&board,scan+100);
        // #2083: this pass scans `taskq` issues only, so coverage ends at the scan of every issue that saw the draft
        assert_eq!(restarted.through,Some(scan));assert!(restarted.missing.is_empty());
        board.state.borrow_mut().clock=scan+149;
        board.labels(held,&["taskq".into()],&["q-held".into()]).unwrap();
        restarted.refresh(&board,scan+200);
        assert_eq!(restarted.through,Some(scan));
        assert!(restarted.missing.iter().any(|s|s.contains("history unavailable")));
        restarted.refresh(&board,MONTH+scan+200);
        assert!(restarted.pending.is_empty());assert!(restarted.seen.is_empty(),"the scan cache is month-bounded");
        assert_eq!(restarted.through,Some(MONTH+scan+200));
    }

    // Chain: M6. UX30-21: first label admission survives restarts, requeues, removal and closure; absent history is unknown.
    #[test]
    fn first_admission_rebuilds_without_inventing_arrivals() {
        let board = Fake::default();
        let held = board.create("held draft", "", &["q-held".into()]).unwrap();
        let mut h = History::default();
        h.refresh(&board, 100);
        assert_eq!(h.json("board")["events"], json!([]));
        board.state.borrow_mut().clock=119;
        board.labels(held, &["taskq".into()], &["q-held".into()]).unwrap();
        board.state.borrow_mut().admissions.insert(held, Ok(Some((71, 120))));
        for kind in ["start", "spawn worker codex:x mac", "requeue", "spawn worker claude:y mac"] { board.comment(held, &format!("taskq: {kind}")).unwrap(); }
        h.refresh(&board, 200);
        assert_eq!(h.json("board")["events"], json!([[120,"new","71",held]]));
        board.state.borrow_mut().clock=249;
        board.close(held).unwrap();
        board.labels(held, &[], &["taskq".into()]).unwrap();
        let mut rebuilt = History::default();
        rebuilt.refresh(&board, 300);
        assert_eq!(rebuilt.json("board")["events"], h.json("board")["events"], "the board's original event survives cache loss");
        board.state.borrow_mut().clock=349;
        let legacy = board.create("legacy", "", &["taskq".into()]).unwrap();
        rebuilt.refresh(&board, 400);
        let partial = rebuilt.json("board");
        assert_eq!(partial["end"], 300);
        assert!(partial["missing"].to_string().contains(&format!("#{legacy}")));
        assert_eq!(partial["events"], h.json("board")["events"], "retain known lower bounds");
        board.state.borrow_mut().admissions.insert(legacy, Ok(Some((99, 350))));
        // #2083: coverage ends at the next scan of every issue, at most FULL_SCAN after the last
        rebuilt.refresh(&board, 300 + FULL_SCAN);
        assert_eq!(rebuilt.json("board")["end"], 300 + FULL_SCAN);
        assert_eq!(rebuilt.json("board")["events"].as_array().unwrap().len(), 2);
    }
