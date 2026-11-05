    use super::*;

    // Chain: § 7 close outcome. Risk: a superseded, unchanged or partly unverified close reads as delivered work.
    #[test]
    fn close_outcome_names_successor_no_change_and_unverified_items() {
        // pilotq#90 verbatim shape: closed for #104 with all items moved
        let p90 = "Changed: none; owner chose option 2, #90 closes as superseded by #104, which delivers voice input\nChecks: Task fit: A1 unknown, A2 unknown, A3 unknown, A4 unknown; owner answer 2 moves all four to open #104\nOpen: #104 is held";
        assert_eq!(successor(p90), Some(104));
        assert_eq!(close_outcome(Some(p90), Some("ready")), "replaced by #104 (ready)");
        assert_eq!(close_outcome(Some("Changed: Replaced by #7"), None), "replaced by #7 (unknown)");
        assert_eq!(close_outcome(Some("Changed: none\nChecks: A1 ok"), None), "no change");
        let partly = "Changed: icon ships\nChecks: A1 ok, A2 gap\nChecks: A3 unknown; A4 unknown; A2 gap\nOpen: none";
        assert_eq!(close_outcome(Some(partly), None), "delivered, 3 items unverified");
        assert_eq!(close_outcome(Some("Changed: x\nChecks: A1 ok\nOpen: A2 unknown"), None), "delivered", "only Checks lines count");
        assert_eq!(close_outcome(None, None), "closed on the board");
        assert_eq!(successor("closes #12 after #13"), None);
    }

    fn project(name: &str, tasks: Value) -> Value {
        json!({"name": name, "issues": "https://github.com/o/r/issues/", "board_kind": "github", "complete": true, "focus": [], "saved": false, "tasks": tasks})
    }

    // Chain: menu.md M3 age. Risk: CLI and page print different units for one elapsed time.
    #[test]
    fn age_is_short_units() {
        assert_eq!([-5, 59, 300, 7200, 259200].map(age), ["<1m", "<1m", "5m", "2h", "3d"]);
    }

    // Chain: #1928 owner note / R6 / M3. Risk: an approved queued review looks like a stalled reviewer;
    // an unqueued publication or a different lifecycle state must never read as queued.
    #[test]
    fn queued_review_is_named_from_the_existing_queue_receipt() {
        let mut ps = [project("a", json!([
            {"n":1,"state":"review","enqueued":"invented-receipt"},
            {"n":2,"state":"review","publication":{"reason":"manual merge needed"}},
            {"n":3,"state":"doing","enqueued":"obsolete-receipt"}
        ]))];
        model(&mut ps, 1000);
        for t in ps[0]["tasks"].as_array().unwrap() {
            assert_eq!(t["state_name"], match t["n"].as_u64().unwrap() {1 => "merge queue · CI", 2 => "Review", _ => "Doing"});
        }
    }

    // Chain: K-A2/K-A3 deduplication. Risk: a task registered twice counts twice, or a conflict shows a number.
    #[test]
    fn one_task_per_board_and_conflicts_are_unknown() {
        let row = json!({"n": 1, "state": "doing", "revision": 4, "title": "T"});
        let mut ps = [project("a", json!([row])), project("b", json!([row, {"n": 2, "state": "closed"}]))];
        let m = model(&mut ps, 100);
        assert_eq!((m["all"]["counts"][0].clone(), m["all"]["open"].clone()), (json!({"state": "doing", "name": "Doing", "n": 1}), json!(1)));
        assert_eq!((ps[0]["tasks"][0]["dup"].clone(), ps[1]["tasks"][0]["dup"].clone()), (json!(false), json!(true)));
        let mut ps = [project("a", json!([row])), project("b", json!([{"n": 1, "state": "review", "revision": 5, "title": "T"}]))];
        let m = model(&mut ps, 100);
        assert_eq!((m["all"]["partial"].clone(), m["all"]["open"].clone(), m["all"]["counts"][0]["n"].clone()), (json!(["b"]), Value::Null, Value::Null));
        let mut ps = [json!({"name": "c", "issues": "x", "complete": false, "focus": ["p0"], "tasks": []})];
        let m = model(&mut ps, 100);
        assert_eq!((m["all"]["partial"].clone(), ps[0]["mode"].clone(), m["run"].clone()), (json!(["c"]), json!("run"), json!(true)));
    }

    // #1840 A1: a failed project must not hide loaded totals or count retained stale rows.
    #[test]
    fn loaded_projects_keep_counts_when_another_project_fails() {
        let mut good = project("loaded", json!([{"n":1,"state":"doing"},{"n":2,"state":"ready"}]));
        good["issues"] = json!("https://github.com/o/loaded/issues");
        let mut bad = project("broken", json!([{"n":99,"state":"ask"}]));
        bad["complete"] = json!(false);
        bad["error"] = json!("no access to the repository");
        let mut ps = [good, bad];
        let m = model(&mut ps, 100);
        assert_eq!(m["all"]["counts"][0]["n"], 1);
        assert_eq!(m["all"]["counts"][2]["n"], 0, "retained failed rows are not loaded counts");
        assert_eq!(m["all"]["open"], 2);
        assert!(ps[1]["counts"][0]["n"].is_null(), "failed project's own count stays unknown");
        assert_eq!(m["all"]["partial"], json!([]));
        ps[1]["issues"] = ps[0]["issues"].clone();
        ps[1]["tasks"][0]["n"] = json!(1);
        ps.swap(0, 1);
        let m = model(&mut ps, 100);
        assert_eq!(m["all"]["open"], 2, "a failed registration cannot shadow a loaded board");
        assert_eq!(ps[1]["tasks"][0]["dup"], false);
        assert_eq!(ps[0]["tasks"][0]["dup"], true);
        ps[1]["complete"] = json!(false);
        let m = model(&mut ps, 100);
        assert!(m["all"]["open"].is_null());
        assert_eq!(ps[0]["tasks"][0]["dup"], false);
        assert_eq!(ps[1]["tasks"][0]["dup"], true, "failed-only registrations still show each task once");
    }

    // Chain: menu.md Tasks list order and M3 age. Risk: rows leave strip/priority/number order or show an invented age.
    #[test]
    fn rows_sort_by_state_priority_number_and_carry_age_text() {
        let mut ps = [project("a", json!([{"n": 9, "state": "ready"}, {"n": 3, "state": "ready", "labels": ["priority-1"]},
            {"n": 7, "state": "doing", "since": 700, "created": 0, "steps": [{"start": 400, "end": 700}, {"start": 700, "end": null}, {"start": null, "end": null}]}]))];
        model(&mut ps, 1000);
        let t = ps[0]["tasks"].as_array().unwrap();
        assert_eq!(t.iter().map(|t| t["n"].as_u64().unwrap()).collect::<Vec<_>>(), [7, 3, 9]);
        assert_eq!(t.iter().map(|t| t["rank"].as_u64().unwrap()).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!((t[0]["age"].as_str(), t[0]["total_age"].as_str(), t[0]["state_name"].as_str()), (Some("5m"), None, Some("Doing")));
        assert_eq!(t[0]["steps"].as_array().unwrap().iter().map(|s| s["age"].clone()).collect::<Vec<_>>(), [json!("5m"), json!("5m"), Value::Null]);
        assert_eq!(ps[0]["mode"], "watch");
        // #1661 R17: the CPU/memory sample is as old as its offset plus the status file; no sample, no age
        let mut ps = [json!({"age": 30, "capacity": {"sample_age_seconds": 90}}), json!({"age": 30, "capacity": {}})];
        model(&mut ps, 1000);
        assert_eq!((ps[0]["sample_age_text"].as_str(), ps[1]["sample_age_text"].clone()), (Some("2m"), Value::Null));
    }
