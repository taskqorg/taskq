    use super::*;

    // M14: read/clear survives restart, never alters OS cursors or downgrades a cleared event; reject invalid IPC atomically.
    #[test]
    fn inbox_markers_are_exact_idempotent_and_persisted() {
        let id=json!(["https://example.org/issues","event",7,[100,"ask","9"]]).to_string();
        let mut n=json!({"cursor":{"/p":"prior"},"kinds":{"ask":"sound"}});
        assert!(inbox_set(&mut n,&json!({"action":"read","ids":[id]})));
        assert_eq!(n["inbox"][&id],"read");
        let mut saved:Value=serde_json::from_str(&n.to_string()).unwrap();
        assert!(inbox_set(&mut saved,&json!({"action":"clear","ids":[id]})));
        assert!(inbox_set(&mut saved,&json!({"action":"read","ids":[id]})));
        assert_eq!(saved["inbox"][&id],"cleared");
        let before=saved.clone();
        assert!(!inbox_set(&mut saved,&json!({"action":"read","ids":[id,"invalid"]})));
        assert_eq!(saved,before,"invalid batch changes nothing");
        assert_eq!(saved["cursor"],n["cursor"]);
        assert_eq!(saved["kinds"],n["kinds"]);
    }

    fn proj(events: Value) -> Value {
        json!({"root": "/p", "name": "p", "tasks": [{"n": 7, "title": "Fix it"}], "events": events})
    }

    // Defect #1845 A2: only Screen Recording had an owner notice; a stale Accessibility grant became a worker ask.
    // Risk: the notice repeats each tick or restart, or never returns for a new episode.
    #[test]
    fn grant_notices_once_per_episode() {
        let bad = |k: &str, s: &str| [json!({"gui": {"grants": {k: s}, "binary": "/i/taskq"}})];
        let mut n = json!({});
        assert!(grant_notices(&mut n, &bad("accessibility", "allowed")).is_empty());
        let post = grant_notices(&mut n, &bad("accessibility", "stale"));
        assert_eq!(post.len(), 1, "first stale read posts");
        assert_eq!(post[0].body, "System Settings > Privacy & Security > Accessibility: select taskq, remove it (-), then add /i/taskq with + and switch it on.");
        assert!(grant_notices(&mut n, &bad("accessibility", "missing")).is_empty(), "repeated within the episode");
        let saved = n.to_string();
        assert!(grant_notices(&mut serde_json::from_str(&saved).unwrap(), &bad("accessibility", "stale")).is_empty(), "repeated after a restart");
        assert!(grant_notices(&mut n, &bad("accessibility", "allowed")).is_empty());
        assert_eq!(grant_notices(&mut n, &bad("accessibility", "missing"))[0].body, "System Settings > Privacy & Security > Accessibility: add /i/taskq with + and switch it on.", "a new episode notifies again");
        let app = grant_notices(&mut n, &bad("app_data", "stale"));
        assert_eq!((app.len(), app[0].title.as_str()), (1, "taskq · App Data grant is stale"));
        assert!(grant_notices(&mut n, &bad("app_data", "unknown")).is_empty() && n["grants"].get("app_data").is_none(), "unknown ends the App Data episode");
    }

    // Defect #1803 A2: a cdhash-pinned grant re-prompted forever with no owner step. Risk: the notice repeats each
    // tick or restart, or a fixed and later re-broken grant never notifies again.
    #[test]
    fn stale_grant_notifies_once_per_episode() {
        let stale = [json!({"gui": {"listed": false, "stale_grant": true}}), json!({"gui": {"listed": false, "stale_grant": false}})];
        let mut n = json!({});
        assert_eq!(stale_grant(&mut n, &[json!({"gui": {"listed": false, "stale_grant": false}})]), None);
        let post = stale_grant(&mut n, &stale).expect("first stale read posts");
        assert_eq!(post.body, STALE_GRANT);
        assert_eq!(stale_grant(&mut n, &stale), None, "repeated on the next tick");
        let saved = serde_json::to_string(&n).unwrap();
        assert_eq!(stale_grant(&mut serde_json::from_str(&saved).unwrap(), &stale), None, "repeated after a restart");
        assert_eq!(stale_grant(&mut n, &[json!({"gui": {"listed": true, "stale_grant": false}})]), None);
        assert!(stale_grant(&mut n, &stale).is_some(), "a new episode after a passing probe notifies again");
        let mut t = json!({});
        let p = json!({"root": "/p", "name": "p", "tasks": [], "events": [], "gui": {"listed": false, "stale_grant": true}});
        assert_eq!(due(&mut t, &[p.clone()], 100, &mut Bursts::new()).len(), 1, "the tray tick posts it");
        assert!(due(&mut t, &[p], 200, &mut Bursts::new()).is_empty());
    }

    // Chain: M14 modes. Risk: a bad value or a missing override silences or floods a kind.
    #[test]
    fn modes_default_override_and_unknown() {
        let mut n = json!({"kinds": {"close": "loud", "failed": "silent"}});
        assert_eq!([host(&n, "ask"), host(&n, "close"), host(&n, "failed"), host(&n, "gone")], ["sound", "off", "silent", "banner"], "unknown reads as default");
        assert!(set(&mut n, &json!({"kind": "close", "value": "banner", "root": "/p"})));
        assert_eq!((mode(&n, "/p", "close"), mode(&n, "/q", "close")), ("banner", "off"));
        assert!(set(&mut n, &json!({"kind": "close", "value": "inherit", "root": "/p"})));
        assert_eq!(mode(&n, "/p", "close"), "off");
        assert!(!set(&mut n, &json!({"kind": "close", "value": "inherit"})), "host has no inherit");
        assert!(!set(&mut n, &json!({"kind": "blocked", "value": "banner"})));
        assert!(set(&mut n, &json!({"kind": "ask", "value": "off"})));
        assert!(!wants_banner(&json!({"kinds": {"ask": "off", "failed": "silent", "gone": "off"}}), &["/p".into()]));
        assert!(wants_banner(&json!({"kinds": {"ask": "off", "failed": "silent", "gone": "off"}, "projects": {"/p": {"close": "banner"}}}), &["/p".into()]));
    }

    // Chain: M14 once per event across restarts. Risk: a restart repeats or floods old events.
    #[test]
    fn cursor_once_per_event_across_restart() {
        let old = json!([[100, "ask", "5", 7]]);
        let mut n = json!(null);
        assert!(due(&mut n, &[proj(old.clone())], 150, &mut Bursts::new()).is_empty(), "first start: history never notifies");
        let ev = json!([[100, "ask", "5", 7], [200, "failed", "6", 7], [200, "close", "6", 8]]);
        let p = due(&mut n, &[proj(ev.clone())], 210, &mut Bursts::new());
        assert_eq!(p.len(), 1, "close is off by default");
        assert_eq!((p[0].title.as_str(), p[0].body.as_str(), p[0].n, p[0].sound), ("p · Failed", "p#7 Fix it", Some(7), false));
        // a restart reloads the saved value: nothing again
        let mut saved: Value = serde_json::from_str(&n.to_string()).unwrap();
        assert!(due(&mut saved, &[proj(ev)], 220, &mut Bursts::new()).is_empty());
        // the cursor line trimmed away: later times only
        let ev = json!([[200, "close", "6", 8], [300, "ask", "9", 7]]);
        let p = due(&mut saved, &[proj(ev)], 310, &mut Bursts::new());
        assert_eq!((p.len(), p[0].sound), (1, true));
        // silent and off still move the cursor
        set(&mut saved, &json!({"kind": "ask", "value": "silent"}));
        assert!(due(&mut saved, &[proj(json!([[300, "ask", "9", 7], [400, "ask", "10", 7]]))], 410, &mut Bursts::new()).is_empty());
        set(&mut saved, &json!({"kind": "ask", "value": "banner"}));
        assert!(due(&mut saved, &[proj(json!([[300, "ask", "9", 7], [400, "ask", "10", 7]]))], 420, &mut Bursts::new()).is_empty());
    }

    // Chain: M14 burst. Risk: a busy pass floods the screen.
    #[test]
    fn burst_becomes_one_summary() {
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev = json!([[10, "failed", "1", 1], [10, "failed", "2", 2], [10, "ask", "3", 3], [10, "gone", "4", 4], [10, "close", "5", 5]]);
        let p = due(&mut n, &[proj(ev)], 20, &mut Bursts::new());
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].title.as_str(), p[0].body.as_str(), p[0].n, p[0].sound), ("p · 4 events", "1 question, 2 failed, 1 attempt gone", None, true));
        let mut n = json!({"cursor": {"/p": "0"}});
        assert_eq!(due(&mut n, &[proj(json!([[10, "failed", "1", 1], [10, "failed", "2", 2], [10, "ask", "3", 3]]))], 20, &mut Bursts::new()).len(), 3, "three stay single");
        // spread over ticks: the 4th banner within 10 s turns the window into one summary, replaced as it grows
        let (mut n, mut b) = (json!({"cursor": {"/p": "0"}}), Bursts::new());
        let mut ev = vec![];
        let mut tick = |t: i64, e: Value| { ev.push(e); due(&mut n, &[proj(json!(ev))], t, &mut b) };
        for t in [100, 102, 104] {
            assert_eq!(tick(t, json!([t, "failed", t.to_string(), 7]))[0].n, Some(7));
        }
        let s1 = tick(106, json!([106, "gone", "106", 7]));
        assert_eq!((s1.len(), s1[0].title.as_str(), s1[0].body.as_str()), (1, "p · 4 events", "3 failed, 1 attempt gone"));
        let s2 = tick(109, json!([109, "ask", "109", 7]));
        assert_eq!((s2[0].title.as_str(), &s2[0].id), ("p · 5 events", &s1[0].id), "same identifier: the OS replaces it");
        let p = tick(118, json!([118, "failed", "118", 7]));
        assert_eq!((p[0].title.as_str(), &p[0].id), ("p · 6 events", &s1[0].id), "under 10 s apart: the summary grows");
        let p = tick(130, json!([130, "failed", "130", 7]));
        assert_eq!((p.len(), p[0].n), (1, Some(7)), "a quiet 10 s ends it");
        // sliding: 108..111 is a burst although 100 started an earlier window
        let (mut n, mut b) = (json!({"cursor": {"/p": "0"}}), Bursts::new());
        let mut ev = vec![];
        let mut tick = |t: i64| { ev.push(json!([t, "failed", t.to_string(), 7])); due(&mut n, &[proj(json!(ev))], t, &mut b) };
        let singles: Vec<usize> = [100, 108, 109, 110].into_iter().map(|t| tick(t).len()).collect();
        assert_eq!(singles, [1, 1, 1, 1]);
        let s = tick(111);
        assert_eq!((s.len(), s[0].title.as_str(), s[0].n), (1, "p · 4 events", None), "108, 109, 110, 111");
        // one tick: an early single, then a burst
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev = json!([[50, "failed", "1", 1], [70, "failed", "2", 2], [71, "failed", "3", 3], [72, "ask", "4", 4], [73, "gone", "5", 5]]);
        let p = due(&mut n, &[proj(ev)], 80, &mut Bursts::new());
        assert_eq!(p.iter().map(|p| (p.n, p.title.as_str())).collect::<Vec<_>>(), [(Some(1), "p · Failed"), (None, "p · 4 events")]);
        // one tick: a burst, then a single after a quiet 10 s; and two separated bursts
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev = json!([[100, "failed", "1", 1], [101, "failed", "2", 2], [102, "ask", "3", 3], [103, "gone", "4", 4], [120, "failed", "5", 5]]);
        let p = due(&mut n, &[proj(ev)], 130, &mut Bursts::new());
        assert_eq!(p.iter().map(|p| (p.n, p.title.as_str())).collect::<Vec<_>>(), [(Some(5), "p · Failed"), (None, "p · 4 events")]);
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev: Vec<Value> = [100, 101, 102, 103, 104, 200, 201, 202, 203].iter().map(|t| json!([t, "failed", t.to_string(), 7])).collect();
        let p = due(&mut n, &[proj(json!(ev))], 210, &mut Bursts::new());
        let ids: Vec<Value> = p.iter().map(|p| serde_json::from_str::<Value>(&p.id).unwrap()["sum"].clone()).collect();
        assert_eq!((p.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ids), (vec!["p · 5 events", "p · 4 events"], vec![json!(100), json!(200)]));
    }

    // Chain: M14 a closed task's banner and card name it. Risk: `status.json` drops closed tasks, so the title is lost,
    // also when the tray restarts between the last open sighting and the close.
    #[test]
    fn closed_task_keeps_its_title() {
        let mut n = json!({"kinds": {"close": "banner"}});
        due(&mut n, &[proj(json!([]))], 100, &mut Bursts::new());
        let mut saved: Value = serde_json::from_str(&n.to_string()).unwrap();
        let gone = json!({"root": "/p", "name": "p", "tasks": [], "events": [[200, "close", "6", 7], [201, "close", "8", 90, "replaced by #104 (ready)"]]});
        let p = due(&mut saved, &[gone], 210, &mut Bursts::new());
        assert_eq!(p[0].body, "p#7 Fix it");
        // #1800 (pilotq#90): a replaced close names its successor's state instead of a bare `Closed`
        assert_eq!(p[1].body, "p#90 closed: replaced by #104 (ready)");
        assert_eq!(serde_json::from_str::<Value>(&p[0].id).unwrap()["t"], "Fix it", "the click opens a card with its title");
        assert_eq!(saved["titles"]["/p"], json!({}), "only open tasks stay: bounded by the open tasks");
    }
