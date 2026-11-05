    use super::*;
    use std::{collections::BTreeMap, io::Read as _};

    /// DashQ stand-in: stores records by event id (a replay returns the stored receipt), or fails as told.
    #[derive(Default)]
    struct Dash { fail: Option<u16>, lose: bool, sent: Vec<String>, stored: BTreeMap<String, Value> }
    impl Wire for Dash {
        fn post(&mut self, _: &str, route: &str, _: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
            assert_eq!(route, "/ingest");
            let r = &body["records"][0];
            self.sent.push(r["event_id"].as_str().unwrap().into());
            if let Some(code) = self.fail { return Ok((code, json!({"protocol": 1, "error": "revoked", "retry_after": 7}))) }
            let kept = self.stored.entry(r["event_id"].as_str().unwrap().into()).or_insert(r["payload"].clone());
            let receipt = if *kept == r["payload"] { json!({"event_id": r["event_id"], "state": "accepted"}) }
                else { json!({"event_id": r["event_id"], "state": "rejected", "reason": "replay_mismatch"}) };
            if std::mem::take(&mut self.lose) { return Err("reply lost".into()) }
            Ok((200, json!({"protocol": 1, "receipts": [receipt]})))
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    fn seqs(d: &Dash) -> Vec<i64> {
        let mut out = vec![];
        for p in d.stored.values() {
            let gz = base64::engine::general_purpose::STANDARD.decode(p["gzip"].as_str().unwrap()).unwrap();
            let mut text = String::new();
            flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut text).unwrap();
            out.extend(text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["seq"].as_i64().unwrap()));
        }
        out.sort();
        out
    }

    fn setup(name: &str, schemas: Value) -> (PathBuf, coreq::telemetry::Store) {
        let install = std::env::temp_dir().join(format!("taskq-tsync-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&install);
        fs::create_dir_all(enrollment::dir(&install)).unwrap();
        let conn = json!({"destination": "https://d/machine", "credential": "c", "generation": 1, "installation": "i",
            "scope": {"name": enrollment::SCHEMA, "value": "github:github.com/o/a"}, "schemas": schemas});
        fs::write(enrollment::dir(&install).join("connection.json"), json!({"installation": "i", "pending": null, "active": conn, "revoke": []}).to_string()).unwrap();
        fs::create_dir_all(install.join("telemetry.log")).unwrap();
        let store = coreq::telemetry::Store::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
        (install, store)
    }

    fn append(store: &coreq::telemetry::Store, n: usize) {
        let e = || coreq::telemetry::Event::new(json!({"product": "taskq", "version": "1.0.0", "operation": "op", "kind": "operation",
            "trace": "t", "span": "s", "started_ms": 1000, "duration_us": 10, "outcome": "ok", "error": "none"})).unwrap();
        for c in (0..n).collect::<Vec<_>>().chunks(1000) { store.append(&c.iter().map(|_| e()).collect::<Vec<_>>()).unwrap(); }
    }

    fn go(install: &Path, d: &mut Dash) -> (Value, u64) {
        run(install, d, &mut Backoff::default(), Instant::now() + Duration::from_secs(60))
    }

    // Chain: grant -> blocks after the cursor -> receipts -> restart -> lost reply -> 429/5xx -> revoke.
    // Risk: duplicate or missing rows across restarts and retries, a cursor advanced without a receipt, or sending
    // without the telemetry grant (A2-A4).
    #[test]
    fn sync_resumes_from_the_cursor_without_duplicates_or_gaps() {
        let (install, store) = setup("nogrant", json!([enrollment::SCHEMA]));
        append(&store, 3000);
        let mut d = Dash::default();
        assert_eq!(go(&install, &mut d).0, json!({"state": "off"}), "no grant: off");
        assert!(d.sent.is_empty() && !cursor_path(&install).exists(), "no grant: no /ingest call, no cursor");
        drop(store);
        fs::remove_dir_all(install).unwrap();
        let (install, store) = setup("resume", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 3000);
        let (s, wait) = go(&install, &mut d);
        assert_eq!((s["state"].as_str(), s["backlog_rows"].as_i64(), s["backlog_bytes"].as_u64(), s["gap_rows"].as_u64(), wait), (Some("idle"), Some(0), Some(0), Some(0), IDLE));
        assert_eq!(seqs(&d), (1..=3000).collect::<Vec<_>>());
        // restart: a new job reads the cursor file; only new rows go, the last block's reply is lost
        append(&store, 500);
        d.lose = true;
        let (s, _) = go(&install, &mut d);
        assert_eq!((s["state"].as_str(), s["backlog_rows"].as_i64()), (Some("retrying"), Some(500)), "lost reply: cursor waits");
        let lost = d.sent.last().cloned().unwrap();
        append(&store, 10);
        // the retry repeats the lost block's status although the backlog grew: other content is replay_mismatch
        assert_eq!(go(&install, &mut d).0["state"], "idle");
        assert_eq!(d.sent.iter().filter(|e| **e == lost).count(), 2, "the retry repeats the pending block's event id");
        assert_eq!(seqs(&d), (1..=3510).collect::<Vec<_>>(), "no duplicate or missing row");
        // 429 and 5xx back off with the cursor kept; 401 stops and ends the grant: nothing more is sent
        append(&store, 5);
        for (code, want) in [(429, "retrying"), (503, "retrying"), (401, "stopped")] {
            d.fail = Some(code);
            let (s, wait) = go(&install, &mut d);
            assert_eq!((s["state"].as_str(), s["backlog_rows"].as_i64()), (Some(want), Some(5)), "{code}");
            if code == 429 { assert_eq!(wait, 7, "Retry-After wins") }
        }
        let n = d.sent.len();
        assert_eq!(go(&install, &mut d).0["state"], "off", "revoked: off");
        assert_eq!(d.sent.len(), n);
        drop(store);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: retention or capacity pruning before upload -> next job. Risk: pruned rows resent, invented or silently
    // lost; sync deleting rows itself (S4).
    #[test]
    fn rows_pruned_before_upload_count_as_gap() {
        let (install, store) = setup("gap", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 100);
        let mut d = Dash::default();
        go(&install, &mut d);
        append(&store, 300);
        drop(store);
        let db = rusqlite::Connection::open(install.join("telemetry.log/universal.sqlite3")).unwrap();
        db.execute("DELETE FROM events WHERE seq <= 250", []).unwrap();
        drop(db);
        let (s, _) = go(&install, &mut d);
        assert_eq!((s["state"].as_str(), s["gap_rows"].as_u64()), (Some("idle"), Some(150)));
        assert_eq!(seqs(&d), (1..=100).chain(251..=400).collect::<Vec<_>>());
        let r = coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
        assert_eq!(r.export(0, 0, 1).unwrap()["retained_seq"], json!([251, 400]), "sync deletes nothing");
        drop(r);
        // an unreadable cursor is unknown and kept
        fs::write(cursor_path(&install), "x").unwrap();
        assert_eq!(go(&install, &mut d).0, json!({"state": "unknown", "last_error": "cursor-unreadable"}));
        assert_eq!(fs::read_to_string(cursor_path(&install)).unwrap(), "x");
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: DashQ rejects block 1 as `expired`, accepts block 2 in the same job. Risk: an expired backlog stopping
    // sync forever (#1796), its rows lost uncounted or resent (S4).
    #[test]
    fn expired_block_counts_as_gap_and_sync_continues() {
        let (install, store) = setup("expired", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 8000);
        let mut c = Capture { replies: vec![("rejected", Some("expired"))], ..Default::default() };
        let s = go_with(&install, &mut c);
        assert!(c.records.len() >= 2, "two blocks or more: {}", c.records.len());
        let ids: std::collections::BTreeSet<_> = c.records.iter().map(|r| r["event_id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), c.records.len(), "no duplicate event id");
        let gap = c.records[0]["payload"]["rows"].as_u64().unwrap();
        assert_eq!((s["state"].as_str(), s["gap_rows"].as_u64(), s["backlog_rows"].as_i64()), (Some("idle"), Some(gap), Some(0)));
        assert_eq!(load(&install, "https://d/machine").unwrap()["seq"], 8000, "cursor past both blocks");
        drop(store);
        fs::remove_dir_all(install).unwrap();
    }

    /// The published wire fixture (A2): DashQ D11 conformance input, released as `taskq-<v>-telemetry-sync-fixtures.json`.
    const FIXTURE: &str = include_str!("../tests/fixtures/telemetry-sync.json");

    /// DashQ stand-in that answers each record with the next queued receipt state and reason, `accepted` when none.
    #[derive(Default)]
    struct Capture { records: Vec<Value>, replies: Vec<(&'static str, Option<&'static str>)>, lose: bool }
    impl Wire for Capture {
        fn post(&mut self, _: &str, route: &str, _: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
            assert_eq!(route, "/ingest");
            let r = body["records"][0].clone();
            self.records.push(r.clone());
            if std::mem::take(&mut self.lose) { return Err("reply lost".into()) }
            let (state, reason) = if self.replies.is_empty() { ("accepted", None) } else { self.replies.remove(0) };
            Ok((200, json!({"protocol": 1, "receipts": [{"event_id": r["event_id"], "state": state, "reason": reason}]})))
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    /// Fixture store rows: fixed ids and receipt clocks, operation and stage observations (a Claude stage with model,
    /// context split and output tokens, a Codex stage whose runtime reports no model and no cache writes).
    fn rows() -> Vec<Value> {
        (1..=11).map(|seq: i64| {
            let base = json!({"product": "taskq", "version": "4.6.31", "trace": null, "span": null, "started_ms": 1_792_000_000_000i64 + seq * 1000,
                "outcome": "ok", "error": "none", "unknown": {"version": "not-recorded", "trace": "not-recorded", "span": "not-recorded"}});
            let mut o = match seq % 3 {
                1 => json!({"kind": "operation", "operation": "service.pass", "duration_us": 1200}),
                2 => json!({"kind": "stage", "operation": "build", "task": 1614, "attempt": seq, "runtime": "claude", "duration_us": 9_000_000,
                    "calls": 2, "tokens": 230, "input_tokens": 20, "cache_read_tokens": 200, "cache_write_tokens": 10,
                    "model": "claude-opus-5-5", "output_tokens": 6}),
                _ => json!({"kind": "stage", "operation": "build", "task": 1614, "attempt": seq, "runtime": "codex", "duration_us": 5_000_000,
                    "tokens": 700, "input_tokens": 100, "cache_read_tokens": 600, "cache_write_tokens": 0, "model": null, "output_tokens": 9}),
            };
            for (k, v) in base.as_object().unwrap() { o[k] = v.clone() }
            if o["runtime"] == "codex" { o["unknown"]["model"] = json!("not-reported") }
            let id = coreq::telemetry::Event::identified("taskq.fixture", &seq.to_string(), o.clone()).unwrap().id().to_string();
            json!({"seq": seq, "id": id, "received_ms": 1_792_000_000_000i64 + seq * 1000, "observation": o})
        }).collect()
    }

    // Chain: store rows -> run() blocks with status -> DashQ receipts (accepted, replay after a lost reply,
    // replay_mismatch, payload reject). Risk: the published fixture drifting from what the sender posts, a retry
    // changing content under one event id, a refused block advancing the cursor (#1614 A1, A2, A5).
    // `TASKQ_TELEMETRY_FIXTURE=1` rewrites the fixture after an intended wire change.
    #[test]
    fn sender_posts_the_published_fixture() {
        let (install, store) = setup("fixture", json!([enrollment::SCHEMA, SCHEMA]));
        let rows = rows();
        let import = |from: usize, to: usize, prev: Option<&str>, next: &str| {
            let r = &rows[from - 1..to];
            let events: Vec<_> = r.iter().map(|r| coreq::telemetry::Event::identified("taskq.fixture", &r["seq"].to_string(), r["observation"].clone()).unwrap()).collect();
            store.import_history("fixture", prev, next, &events, &r.iter().map(|r| r["received_ms"].as_i64().unwrap()).collect::<Vec<_>>()).unwrap();
        };
        // last_synced is the sender's clock: the fixture leaves it out
        let mut c = Capture::default();
        let mut go = |c: &mut Capture| { let mut s = go_with(&install, c); s.as_object_mut().unwrap().remove("last_synced"); s };
        // blocks: seq 1..3, 4..5, then retention prunes through 7 (6..7 before upload: gap 2), 8..9
        import(1, 3, None, "p1");
        let s1 = go(&mut c);
        import(4, 5, Some("p1"), "p2");
        let s2 = go(&mut c);
        import(6, 9, Some("p2"), "p3");
        let db = rusqlite::Connection::open(install.join("telemetry.log/universal.sqlite3")).unwrap();
        db.execute("DELETE FROM events WHERE seq <= 7", []).unwrap();
        drop(db);
        let s3 = go(&mut c);
        let records = std::mem::take(&mut c.records);
        // replay: the reply is lost, the retry posts the same record, DashQ returns the stored receipt
        import(10, 10, Some("p3"), "p4");
        c.lose = true;
        let lost = go(&mut c);
        let replay_after = go(&mut c);
        assert_eq!(c.records[0], c.records[1], "a retry repeats event id and content");
        let replay = json!({"case": "replay", "record": c.records[0], "receipts": ["lost", {"state": "accepted"}], "status_after": [lost, replay_after]});
        // mismatch: DashQ holds other content under the next block's event id; reject: DashQ refuses the payload.
        // Either stops the sender with the cursor before the block, and the retry repeats it unchanged.
        import(11, 11, Some("p4"), "p5");
        c.records.clear();
        c.replies = vec![("rejected", Some("replay_mismatch")), ("rejected", Some("payload"))];
        let (mismatch_after, reject_after) = (go(&mut c), go(&mut c));
        assert_eq!(c.records[0], c.records[1]);
        // expired: DashQ refuses the same block as past its window; it becomes a gap and the cursor moves on
        c.replies = vec![("rejected", Some("expired"))];
        let expired_after = go(&mut c);
        assert_eq!(c.records[1], c.records[2]);
        let mut stored = c.records[0]["payload"].clone();
        stored.as_object_mut().unwrap().remove("status");
        let cases = json!([replay,
            {"case": "mismatch", "record": c.records[0], "stored": stored, "receipts": [{"state": "rejected", "reason": "replay_mismatch"}], "status_after": [mismatch_after]},
            {"case": "reject", "record": c.records[1], "receipts": [{"state": "rejected", "reason": "payload"}], "status_after": [reject_after]},
            {"case": "expired", "record": c.records[2], "receipts": [{"state": "rejected", "reason": "expired"}], "status_after": [expired_after]}]);
        let got = json!({"schema": SCHEMA, "installation": "i", "source": "src/telemetry_sync.rs sender_posts_the_published_fixture",
            "rows": rows, "pruned_through": 7, "gap": [6, 7], "records": records, "status_after": [s1, s2, s3], "cases": cases});
        if std::env::var("TASKQ_TELEMETRY_FIXTURE").is_ok_and(|v| v == "1") {
            fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/telemetry-sync.json"), serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        }
        assert_eq!(got, serde_json::from_str::<Value>(FIXTURE).unwrap(), "sender drifted from the published fixture; rerun with TASKQ_TELEMETRY_FIXTURE=1 after an intended change");
        // the fixture's own invariants, as DashQ D11 reads them
        for r in got["records"].as_array().unwrap().iter().chain([&got["cases"][0]["record"], &got["cases"][2]["record"]]) {
            let p = &r["payload"];
            let id = ring::digest::digest(&ring::digest::SHA256, format!("i\0{}\0{}", p["first"]["id"].as_str().unwrap(), p["last"]["id"].as_str().unwrap()).as_bytes());
            assert_eq!(r["event_id"], format!("t-{}", id.as_ref()[..16].iter().map(|x| format!("{x:02x}")).collect::<String>()));
            let gz = base64::engine::general_purpose::STANDARD.decode(p["gzip"].as_str().unwrap()).unwrap();
            let mut text = String::new();
            flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut text).unwrap();
            let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
            assert_eq!((p["rows"].as_u64(), p["bytes"].as_u64()), (Some(lines.len() as u64), Some(text.len() as u64)));
            assert_eq!((&lines[0]["seq"], &lines[0]["id"], &lines[lines.len() - 1]["seq"]), (&p["first"]["seq"], &p["first"]["id"], &p["last"]["seq"]));
            let st = p["status"].as_object().unwrap();
            assert!(st.len() == 3 && ["backlog_rows", "backlog_bytes", "gap_rows"].iter().all(|k| st[*k].is_u64() || st[*k].is_i64() || st[*k].is_null()), "{st:?}");
        }
        assert_eq!(got["records"][2]["payload"]["status"]["gap_rows"], 2, "pruned rows count as gap");
        drop(store);
        fs::remove_dir_all(install).unwrap();
    }

    fn go_with(install: &Path, w: &mut dyn Wire) -> Value {
        run(install, w, &mut Backoff::default(), Instant::now() + Duration::from_secs(60)).0
    }

    struct Slow;
    impl Wire for Slow {
        fn post(&mut self, _: &str, _: &str, _: Option<&str>, _: &Value) -> Result<(u16, Value), String> {
            std::thread::sleep(Duration::from_secs(3));
            Err("timeout".into())
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    fn snap(repo: &str) -> Snapshot {
        Snapshot { source: "host", observed: 1_700_000_000, reason: None, board: json!({"kind": "github", "host": "github.com", "repo": repo}), tasks: vec![], hosts: vec![] }
    }

    // Chain: service pass -> tick -> sender thread on a slow DashQ. Risk: a slow network delaying the pass that
    // launches workers (A4), or another project's service sending.
    #[test]
    fn tick_never_waits_on_the_network() {
        let (install, store) = setup("tick", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 10);
        let mut s = Sender::default();
        s.tick_with(&install, Some(&snap("o/b")), 1, |_| Box::new(Slow));
        assert!(s.job.is_none() && s.status["state"] == "off", "another board: off");
        let t = Instant::now();
        s.tick_with(&install, Some(&snap("o/a")), 1, |_| Box::new(Slow));
        assert!(s.job.is_some() && t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        s.tick_with(&install, Some(&snap("o/a")), 2, |_| Box::new(Slow));
        assert!(t.elapsed() < Duration::from_secs(1) && s.status["state"] == "syncing");
        while !s.job.as_ref().unwrap().is_finished() { std::thread::sleep(Duration::from_millis(50)) }
        assert_eq!(s.tick_with(&install, Some(&snap("o/a")), 3, |_| Box::new(Slow)).as_deref(), Some("telemetry sync: retrying (unreachable)"));
        assert_eq!(s.status["backlog_rows"], 10);
        drop(store);
        fs::remove_dir_all(install).unwrap();
    }
