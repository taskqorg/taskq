    use super::*;

    const QUESTION: &str = "Problem: Pick one\n1. Ship ★\n2. Wait";

    /// One project whose task 7 reads `state` at § 2 `revision`, its log carrying `ops`.
    fn projects(state: &str, revision: u64, ops: &[&str]) -> Value {
        let mut t = json!({"n": 7, "state": state, "revision": revision, "ops": ops});
        if state == "ask" { t["card"] = json!(QUESTION); t["question"] = crate::snapshot::question(QUESTION); }
        json!([{"root": "/p", "tasks": [t]}])
    }
    fn request(id: &str, action: Option<&str>) -> Value {
        let mut m = json!({"root": "/p", "op": {"id": id, "task": 7, "revision": 30, "choice": 1}});
        if let Some(a) = action { m["op"]["action"] = json!(a); m["op"].as_object_mut().unwrap().remove("choice"); }
        m
    }
    fn shown(p: &Pending, projects: &Value) -> Value {
        let mut ps = projects.as_array().unwrap().clone();
        p.apply(&mut ps);
        ps[0]["tasks"].get(0).cloned().unwrap_or(Value::Null)
    }

    // Chain: owner answer -> op-id readback -> one tray snapshot. Risk (#1918): § 2 revision drops after an answer
    // (ask entry 30 -> spawn 25), so a revision compare settles on the pre-answer status.json and every surface
    // shows the question open again.
    #[test]
    fn an_accepted_answer_holds_until_a_row_carries_its_op_id_though_the_revision_drops() {
        let mut p = Pending::default();
        let before = projects("ask", 30, &["ask-op"]);
        p.begin(&request("tray.1", None), &before);
        let row = shown(&p, &before);
        assert_eq!((row["state"].as_str(), row["sync"]["phase"].as_str()), (Some("ask"), Some("sending")), "no expected state ahead of the board");
        assert!(p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.1", "state": "accepted"}, "sync": {"state": "doing", "revision": 25}})));
        let now = Instant::now();
        assert!(!p.observe(before.as_array().unwrap(), now), "a stale snapshot never settles it, though its revision 30 >= 25");
        let row = shown(&p, &before);
        assert_eq!((row["state"].as_str(), row["revision"].as_u64()), (Some("doing"), Some(25)));
        assert_eq!((row["card"].clone(), row["question"].clone()), (Value::Null, Value::Null), "the question is closed on every surface");
        assert_eq!(row["history"], json!([{"question": before[0]["tasks"][0]["question"], "answered": {"by": "you", "text": "Ship"}}]), "the conversation keeps the question");
        assert_eq!(row["answered"], json!({"by": "you", "text": "Ship"}));
        assert!(row.get("sync").is_none(), "accepted is the board's readback, not a pending mark");
        assert_eq!(row["controls"], json!(crate::snapshot::controls("doing")));

        let after = projects("doing", 25, &["tray.1", "ask-op"]);
        assert!(p.observe(after.as_array().unwrap(), now), "the op id confirms it");
        assert_eq!(shown(&p, &after), after[0]["tasks"][0], "a row read after the write is the board's alone");
        assert_eq!(shown(&p, &before)["state"], "doing", "a lagging read after confirmation still shows the readback");
        assert!(p.observe(before.as_array().unwrap(), now + CONFIRMED));
        assert_eq!(p.json(), json!([]));
    }

    // Chain: second owner answer -> readback -> conversation history. Risk (#1918 A5): the closed question replaced the
    // earlier question and its answer instead of appending after them.
    #[test]
    fn an_accepted_answer_appends_its_question_after_earlier_ones() {
        let mut p = Pending::default();
        let earlier = json!({"question": {"text": "Problem: First?"}, "answered": {"by": "sam", "text": "Wait"}});
        let mut before = projects("ask", 30, &["ask-op"]);
        before[0]["tasks"][0]["history"] = json!([earlier]);
        p.begin(&request("tray.1", None), &before);
        assert!(p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.1", "state": "accepted"}, "sync": {"state": "doing", "revision": 25}})));
        let row = shown(&p, &before);
        assert_eq!(row["history"], json!([earlier, {"question": before[0]["tasks"][0]["question"], "answered": {"by": "you", "text": "Ship"}}]));
    }

    // Chain: control while offline -> failed -> one Retry. Risk (#1819): an outbox replays it later, or Retry
    // sends a new operation id.
    #[test]
    fn an_offline_write_fails_with_one_retry_of_the_same_operation() {
        let mut p = Pending::default();
        let m = request("tray.start.7.30", Some("start"));
        let held = projects("held", 30, &[]);
        p.begin(&m, &held);
        assert_eq!(p.retry("/p", "tray.start.7.30"), None, "only a failed operation retries");
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.start.7.30", "state": "unknown", "reason": "service unreachable"}}));
        let row = shown(&p, &held);
        assert_eq!((row["state"].as_str(), row["sync"]["phase"].as_str(), row["sync"]["reason"].as_str()), (Some("held"), Some("failed"), Some("service unreachable")));
        assert_eq!(p.retry("/p", "tray.start.7.30"), Some(m.clone()));
        p.begin(&m, &held);
        assert_eq!((p.json().as_array().unwrap().len(), p.json()[0]["phase"].as_str()), (1, Some("sending")));
    }

    // Chain: two actors answer the same question -> stale receipt. Risk (#1819): the tray asks again or shows the
    // owner's answer as the one that won.
    #[test]
    fn a_conflict_shows_the_real_state_and_a_reason() {
        let mut p = Pending::default();
        let before = projects("ask", 30, &[]);
        p.begin(&request("tray.2", None), &before);
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.2", "state": "stale", "answered": {"by": "alex", "text": "Wait"}}, "sync": {"state": "doing", "revision": 26}}));
        let row = shown(&p, &before);
        assert_eq!((row["state"].as_str(), row["sync"]["phase"].as_str(), row["sync"]["reason"].as_str()), (Some("doing"), Some("mismatch"), Some("Answered first by alex")));
        assert_eq!(row["answered"], json!({"by": "alex", "text": "Wait"}));
        let moved = projects("review", 40, &[]);
        assert_eq!(shown(&p, &moved)["state"], "review", "a newer snapshot is the board's");
    }

    // Chain: owner cancels -> later then close readback -> the task leaves the open snapshot. Risk: a stale row
    // keeps the cancelled task on the list until the next pass.
    #[test]
    fn a_closed_readback_removes_the_stale_row() {
        let mut p = Pending::default();
        let before = projects("doing", 25, &[]);
        p.begin(&request("tray.close.7.25", Some("close")), &before);
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.close.7.25", "state": "accepted"}, "sync": {"state": "closed", "revision": 41}}));
        assert_eq!(shown(&p, &before), Value::Null);
        assert!(p.observe(projects("doing", 25, &["tray.close.7.25.close"]).as_array().unwrap(), Instant::now()), "the close step's op id confirms the cancellation");
    }
    impl Pending {
        /// The store as test evidence: one entry per operation, without ages.
        pub fn json(&self) -> Value {
            json!(self.ops.iter().map(|o| json!({"root": o.root, "id": o.id, "task": o.task, "phase": o.phase, "reason": o.reason,
                "state": o.state, "confirmed": o.seen.is_some()})).collect::<Vec<_>>())
        }
    }
