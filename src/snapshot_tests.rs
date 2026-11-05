    use super::*;

    fn exec() -> crate::core::Execution {
        crate::core::Execution { id: 7, attempt: 5, through: 7, host: "win".into(), runtime: "codex".into(), accepted: true, release: None, usage_hold: None }
    }

    fn host(age: i64) -> board::Issue {
        let row = r#"execution {"task":1,"attempt":5,"id":7,"through":7,"state":"running","reason":"turn running"}"#;
        board::Issue { number: 9, title: "taskq host win".into(), body: format!("version 4\n{row}"), updated: 1000 - age, trusted: true, ..Default::default() }
    }

    // Chain: R6 Anomalies, § 4 Filter. Defect: a manager-only host claims no task, so its focus gates nothing, yet an
    // unread focus printed an anomaly the PM could not act on. Risk: a host that claims tasks, or one whose claim mode
    // is unread, admits nothing without a row.
    #[test]
    fn focus_unknown_is_an_anomaly_only_on_a_host_that_may_claim_tasks() {
        let at = 10_000;
        let beat = |name: &str, body: String| board::Issue { title: format!("taskq host {name}"), body, updated: at - 10, trusted: true, ..Default::default() };
        // The published schema 1 heartbeat shape: Balancing::read refuses a partial line, and an unread mode keeps the row.
        let runtime = r#"{"code":"unknown","reason":"see host diagnostics","full":false,"disabled":false,"quota":[]}"#;
        let mode = |claim: &str, focus: &str| format!("version 4\nbalancing {{\"schema\":1,\"mode\":\"{claim}\",\"default_runtime\":\"claude\",\"allowed_runtimes\":null,\"auto_reroute\":false,\"has\":[],\"focus\":null,\"routing\":[],\"runtimes\":{{\"antigravity\":{runtime},\"claude\":{runtime},\"codex\":{runtime},\"codex-app\":{runtime}}}}}\nfocus {focus}");
        let cfg = crate::tests::cfg();
        let snap = from_views("board", &crate::board::Fake::default(), &cfg, at, &[], &[
            beat("manager", mode("none", "unknown")),
            beat("primary", mode("any", "unknown")),
            beat("spill", mode("spillover", "unknown")),
            beat("pinned", mode("labelled", "unknown")),
            beat("legacy", "version 3\nfocus unknown".into()),
            beat("idle", mode("any", "off")),
            beat("unread", "version 4\nbalancing {\"mode\":\"none\"}\nfocus unknown".into()),
        ]);
        let text = snap.text(&cfg, &json!({})).unwrap();
        for name in ["primary", "spill", "pinned", "legacy", "unread"] {
            assert!(text.contains(&format!("- host {name} · focus unknown: it admits nothing")), "{name}: {text}");
        }
        for name in ["manager", "idle"] {
            assert!(!text.contains(&format!("- host {name} · focus")), "{name}: {text}");
        }
    }

    // O1/R1/R12: retain accepted board stages, including missing dates; ignored entries cannot invent a step.
    #[test]
    fn dashboard_stages_use_only_accepted_board_changes() {
        let log=vec![crate::core::Entry{id:1,body:"taskq: hold".into(),edited:false},
            crate::core::Entry{id:2,body:"taskq: start".into(),edited:false},
            crate::core::Entry{id:3,body:"taskq: spawn worker codex:s mac".into(),edited:false},
            crate::core::Entry{id:4,body:"taskq: result deadbeef attempt 999".into(),edited:false}];
        let comments=vec![board::Comment{id:1,body:log[0].body.clone(),created:100,trusted:true,edited:false,author:String::new(),read:None},
            board::Comment{id:2,body:log[1].body.clone(),created:150,trusted:true,edited:false,author:String::new(),read:None}];
        let v=View {issue:board::Issue{number:1,created:100,..Default::default()},projection:crate::core::project(1,&log,false),log,comments,blockers:vec![]};
        let steps=stage_history(&v);
        assert_eq!(steps.iter().map(|s|s["state"].as_str().unwrap()).collect::<Vec<_>>(),["ready","held","ready","doing"]);
        assert_eq!(steps[1],json!({"state":"held","start":100,"end":150,"complete":true}));
        assert!(steps[2]["start"].is_null() && steps[2]["end"].is_null(),"unknown boundary never becomes a duration");
        assert!(steps[3]["start"].is_null() && steps[3]["end"].is_null() && steps[3]["complete"]==false);
    }

    // O1/R1: total starts with accepted ownership, counts only real transitions, and survives rework.
    #[test]
    fn compact_timing_uses_first_worker_and_real_changes() {
        let bodies=["taskq: hold","taskq: start","taskq: spawn worker codex:invented win","taskq: accepted 3 attempt 3","taskq: steer\nInvented note","taskq: ask attempt 3\nProblem: invented","taskq: answer 1","taskq: result aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa attempt 3","taskq: spawn reviewer codex:review win","taskq: requeue attempt 9","taskq: spawn worker codex:again win"];
        let comments=bodies.iter().enumerate().map(|(i,body)|board::Comment{id:(i+1) as u64,body:body.to_string(),created:100+i as i64*60,trusted:true,edited:false,author:String::new(),read:None}).collect::<Vec<_>>();
        let log=comments.iter().map(|c|core::Entry{id:c.id,body:c.body.clone(),edited:false}).collect::<Vec<_>>();
        let project=core::project(1,&log,false);let timing=task_timing(&project,&comments,false,None);
        assert_eq!(timing,json!({"started":true,"first":280,"changes":5,"current":700,"completed":false,"closed":null}));
        let mut missing=comments.clone();missing[3].created=0;assert!(task_timing(&project,&missing,false,None)["first"].is_null());
        let unstarted=core::project(1,&log[..2],false);assert_eq!(task_timing(&unstarted,&comments,false,None)["started"],false);assert!(task_timing(&unstarted,&comments,false,None)["changes"].is_null());
        assert!(task_timing(&core::project(1,&log[..3],false),&comments,false,None)["started"].is_null());
        let mut waiting=core::project(1,&log,false);waiting.task.state=State::Waiting;
        let unknown=task_timing(&waiting,&comments,false,None);assert!(unknown["current"].is_null());assert!(unknown["changes"].is_null());
        let unknown=task_timing(&project,&comments,true,None);assert_eq!(unknown["completed"],true);assert!(unknown["closed"].is_null());
        let closed=task_timing(&project,&comments,true,Some(760));assert_eq!(closed["changes"],6);assert_eq!(closed["closed"],760);assert_eq!(closed["current"],760);
    }

    #[test]
    fn remote_codex_reads_starting_until_accepted() {
        let task = Task { attempt: Some(5), session: Some("codex:- win".into()), ..Default::default() };
        let mut r = Row { heartbeat: Value::Null, worker: None, workers: vec![], conversation: None, brief: String::new(), n: 1, title: String::new(), labels: vec![], url: String::new(), task, revision: 5, updated: None, execution_wait: None, missing_reviewer: None,
            executions: vec![], accepted: false, deps: vec![], failed: None, later: None, session_url: None, since: None, created: None, steps: vec![], timing:Value::Null, stuck: None, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None, verdict: None, unlinked: vec![] };
        let cfg = crate::tests::cfg();
        assert_eq!(session_link(&cfg, &json!({}), &r).1, "win: starting");
        r.accepted = true;
        assert_eq!(session_link(&cfg, &json!({}), &r).1, "win: running");
    }

    // Chain: § 5 late answer -> status row -> tray, DashQ, PilotQ (#1785). Risk: a closed question that only the
    // answering client's receipt names, so every other surface still offers it or shows a stale error.
    #[test]
    fn rows_name_the_answer_that_closed_the_question() {
        let mut r = Row { n: 1, ..Default::default() };
        assert!(r.json().get("answered").is_none(), "no answer, no field");
        r.answered = Some(crate::dispatch::Answered { by: "alex".into(), text: "use x".into(), ..Default::default() });
        assert_eq!(r.json()["answered"], json!({"by": "alex", "text": "use x"}));
    }

    // Chain: R6 3 session cell per runtime and host (#1565). Risk: a Codex thread link shown off its host, where it
    // cannot open, or the DashQ progress page replacing the owning host's thread link.
    #[test]
    fn codex_thread_links_only_on_its_host_else_dashq_progress() {
        use std::fs;
        let task = |host: &str| Task { attempt: Some(5), session: Some(format!("codex:thread-1 {host}")), ..Default::default() };
        let row = |host: &str| Row { heartbeat: Value::Null, worker: None, workers: vec![], conversation: None, brief: String::new(), n: 3, title: String::new(), labels: vec![], url: String::new(), task: task(host), revision: 5, updated: None, execution_wait: None, missing_reviewer: None,
            executions: vec![], accepted: true, deps: vec![], failed: None, later: None, session_url: None, since: None, created: None, steps: vec![], timing:Value::Null, stuck: None, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None, verdict: None, unlinked: vec![] };
        let mut cfg = crate::tests::cfg();
        cfg.me = "mac".into();
        let pages = json!({"pages": "https://p"});
        assert_eq!(session_link(&cfg, &pages, &row("mac")).1, "[mac](https://p/open.html#codex://threads/thread-1)");
        assert_eq!(session_link(&cfg, &pages, &row("win")).1, "win", "no connection: the host alone");
        cfg.install = std::env::temp_dir().join(format!("taskq-progress-page-{}", std::process::id()));
        let id = crate::enrollment::installation_id(&cfg.install).unwrap();
        let conn = json!({"destination": "https://dash.example/machine", "credential": "c"});
        fs::create_dir_all(crate::enrollment::dir(&cfg.install)).unwrap();
        fs::write(crate::enrollment::dir(&cfg.install).join("connection.json"), json!({"installation": id, "pending": null, "active": conn, "revoke": []}).to_string()).unwrap();
        assert_eq!(session_link(&cfg, &pages, &row("win")).1, "[win](https://dash.example/projects#board-o%2Fr-task-3)");
        fs::remove_dir_all(&cfg.install).unwrap();
    }

    // Chain: R6 stuck. Risk: a silent task never shows, or a moving or leased one is flagged.
    #[test]
    fn stuck_flags_each_state_from_two_hours_and_never_a_moving_task() {
        let t = |state, attempt| Task { state, attempt, ..Default::default() };
        let dep = |reason: &str| service::Blocker { dep: crate::core::depends("Depends on #2").remove(0), url: String::new(), reason: reason.into() };
        let beat = |age: i64| board::Issue { title: "taskq host mac".into(), body: "version 4\n#1 5".into(), updated: 10_000 - age, trusted: true, ..Default::default() };
        let at = |task: &Task, deps: &[service::Blocker], age: i64, hosts: &[board::Issue]| stuck(1, task, deps, 10_000 - age, hosts, 10_000, 300);
        for s in [State::Held, State::Ask, State::Later, State::Doing, State::Review] {
            assert_eq!(at(&t(s, Some(5)), &[], STUCK_AFTER - 1, &[]), None, "{s:?} under 2 h");
            assert_eq!(at(&t(s, Some(5)), &[], STUCK_AFTER, &[]), Some(STUCK_AFTER), "{s:?} at 2 h");
            assert_eq!(at(&t(s, Some(5)), &[], 60, &[]), None, "{s:?} moving");
        }
        // a doing attempt its fresh heartbeat lists is leased; a stale heartbeat or another attempt is not
        assert_eq!(at(&t(State::Doing, Some(5)), &[], 3 * STUCK_AFTER, &[beat(10)]), None);
        assert_eq!(at(&t(State::Doing, Some(5)), &[], 3 * STUCK_AFTER, &[beat(301)]), Some(3 * STUCK_AFTER)); // #1591: live through ttl, stale after it
        assert_eq!(at(&t(State::Doing, Some(6)), &[], 3 * STUCK_AFTER, &[beat(10)]), Some(3 * STUCK_AFTER));
        // #1752: a review with a leased reviewer is not stuck; one whose reviewer never starts is
        assert_eq!(at(&t(State::Review, Some(5)), &[], 3 * STUCK_AFTER, &[beat(10)]), None);
        assert_eq!(at(&t(State::Review, Some(6)), &[], 3 * STUCK_AFTER, &[beat(10)]), Some(3 * STUCK_AFTER));
        for s in [State::Ready, State::Failed] {
            assert_eq!(at(&t(s, None), &[], 3 * STUCK_AFTER, &[]), None, "{s:?} never");
        }
        // waiting: at once when every blocker is closed, never while one is open or unreadable
        let w = t(State::Waiting, None);
        assert_eq!(at(&w, &[dep("unknown: closed not planned")], 0, &[]), Some(0));
        assert_eq!(at(&w, &[dep("unknown: closed not planned"), dep("open")], 3 * STUCK_AFTER, &[]), None);
        assert_eq!(at(&w, &[dep("unknown: 502")], 3 * STUCK_AFTER, &[]), None);
    }

    #[test]
    fn stuck_row_prints_one_line_and_json_age() {
        let row = |n, stuck| Row { heartbeat: Value::Null, worker: None, workers: vec![], conversation: None, brief: String::new(), n, title: "T".into(), labels: vec![], url: format!("u{n}"), task: Task { state: State::Ask, ..Default::default() }, revision: 0, updated: None,
            execution_wait: None, missing_reviewer: None, executions: vec![], accepted: false, deps: vec![], failed: None, later: None, session_url: None, since: None, created: None, steps: vec![], timing:Value::Null, stuck, admission: None, answered: None, history: vec![], ops: vec![], publication: None, stage: None, churn: None, verdict: None, unlinked: vec![] };
        let snap = Snapshot { source: "board", observed: 0, reason: None, reason_code: None, board: json!({"url": "b"}), tasks: vec![row(1, Some(9000)), row(2, None)], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("| ") && l.contains("stuck")).collect::<Vec<_>>(), ["| [#1 T](u1) | owner · stuck 2h | answer the Owner row |"], "{text}");
        let rows = snap.json()["tasks"].clone();
        assert_eq!((rows[0]["stuck"].clone(), rows[0]["age_s"].clone()), (json!(true), json!(9000)));
        assert_eq!((rows[1]["stuck"].clone(), rows[1].get("age_s")), (json!(false), None));
    }

    // Chain: #2261 heartbeat cause -> status anomaly. Risk: a heartbeat cause breaks the summary markdown or adds a link,
    // or an untrusted host issue supplies the cause.
    #[test]
    fn session_link_cause_is_escaped_and_read_only_from_trusted_heartbeats() {
        let cause = "unknown: failed | x\n[open](https://evil.example)";
        let beat = |trusted| board::Issue { title: "taskq host win".into(), body: format!("session #1 7 {cause}"), trusted, ..Default::default() };
        assert!(session_urls(&[beat(false)]).is_empty(), "untrusted heartbeat");
        // a heartbeat line ends at the newline; the anomaly still escapes a cause that holds one
        assert_eq!(session_urls(&[beat(true)]).get(&("win".to_string(), 7)).map(String::as_str), Some("unknown: failed | x"));
        let task = Task { state: State::Doing, attempt: Some(7), session: Some("claude:s-1 win".into()), ..Default::default() };
        let row = Row { n: 1, title: "T".into(), url: "u1".into(), task, accepted: true, session_url: Some(cause.into()), ..Default::default() };
        let mut cfg = crate::tests::cfg();
        cfg.me = "mac".into();
        let snap = Snapshot { source: "board", observed: 0, reason: None, reason_code: None, board: json!({"url": "b"}), tasks: vec![row], hosts: vec![] };
        let text = snap.text(&cfg, &json!({})).unwrap();
        let line = text.lines().find(|l| l.contains("Remote Control URL")).unwrap_or_else(|| panic!("{text}"));
        assert!(line.contains(r"unknown: failed \| x \[open\](https://evil.example)") && !line.contains("[open]"), "{line}");
    }

    // Chain: #1625 R6 6 Hosts line. Risk: a host whose service stopped keeps reading as a plain age, never as down.
    #[test]
    fn hosts_line_marks_heartbeat_older_than_ttl_stale() {
        let ttl = crate::tests::cfg().ttl;
        let host = |name: &str, updated| Host { name: name.into(), version: "1.0".into(), updated, ..Default::default() };
        let snap = Snapshot { source: "board", observed: 10_000, reason: None, reason_code: None, board: json!({"url": "b"}), tasks: vec![],
            hosts: vec![host("mac", 10_000 - ttl), host("win", 10_000 - ttl - 1)] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let line = text.lines().find(|l| l.starts_with("Hosts: ")).unwrap();
        assert!(line.contains("mac v1.0 ") && !line.contains("stale"), "{line}");
        assert_eq!(text.lines().filter(|l| l.contains("stale")).collect::<Vec<_>>(), ["- host win · heartbeat stale 5m · Next: inspect the host service (`taskq doctor --hosts`)"], "R6 A3: a stale host is one anomaly row");
    }

    // Chain: §3 Runtime accounts (#2341 A5): heartbeat usage rows share the `limit ` prefix with a hold. Risk: status
    // and health read a usage row as a usage limit. Protection: only the hold line is a limit; unmet policy shows.
    #[test]
    fn usage_rows_are_no_limits_and_unmet_policy_shows() {
        let mut i = host(0);
        i.body = "version 4\nlimit codex 2026-01-01T00:00:00Z\nlimit codex work id abcd1234 window 10080m used 20 reset - at 2026-01-01T00:00:00Z\nlimit claude default id - unknown no login\naccount-policy unmet codex abcd1234\n".into();
        let h = super::host(&i, 1000, 300);
        assert_eq!(h.limits, ["codex 2026-01-01T00:00:00Z"]);
        assert_eq!(h.account_policy, ["unmet codex abcd1234"]);
        let snap = Snapshot { source: "board", observed: 1000, reason: None, reason_code: None, board: json!({"url": "b"}), tasks: vec![], hosts: vec![h] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("account-policy unmet codex abcd1234") && !text.contains("window") && !text.contains("no login"), "{text}");
    }

    // Chain: heartbeat `selected` line -> status Hosts line and JSON (#2247 A1, A2). Risk: during a handover status
    // showed only the running version, so a rollout read as done or stuck. Protection: equal versions show one value;
    // a pending handover shows running, selected and the attempts it waits for; an older host without the line falls
    // back to fresh diagnostics with its wait unknown, never to the running version alone.
    #[test]
    fn hosts_line_shows_selected_version_and_its_waits_during_handover() {
        let beat = |title: &str, body: &str| board::Issue { title: title.into(), body: body.into(), updated: 1000, trusted: true, ..Default::default() };
        let snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), 1000, &[], &[
            beat("taskq host mac", "version 4.6.62"),
            beat("taskq host win", "version 4.6.61\nselected 4.6.62 waits #12 attempt 41, #15 attempt 52")]);
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let line = text.lines().find(|l| l.starts_with("Hosts: ")).unwrap();
        assert!(line.contains("mac v4.6.62 ") && !line.split(", win").next().unwrap().contains("selected"), "{line}");
        assert!(line.contains("win v4.6.61 (selected 4.6.62 waits #12 attempt 41, #15 attempt 52) "), "{line}");
        let hosts = snap.json()["hosts"].clone();
        assert_eq!((hosts[0].get("selected"), hosts[1]["selected"].as_str()), (None, Some("4.6.62 waits #12 attempt 41, #15 attempt 52")));
        let old = Host { version: "4.6.61".into(), diagnostics: json!({"selected": {"value": "4.6.62", "state": "ok"}}), ..Default::default() };
        assert_eq!(old.version_text(), "4.6.61 (selected 4.6.62 waits unknown: older host)");
        let same = Host { version: "4.6.62".into(), diagnostics: json!({"selected": {"value": "4.6.62", "state": "ok"}}), ..Default::default() };
        assert_eq!(same.version_text(), "4.6.62");
        let stale = Host { version: "4.6.61".into(), diagnostics: json!({"selected": {"value": "4.6.62", "state": "stale"}}), ..Default::default() };
        assert_eq!(stale.version_text(), "4.6.61", "a stale selection is not reported as pending");
    }

    // Chain: #2256 A1 heartbeat `queue` counts -> installation anomaly row. Case: win serves desiign, a project
    // missing from this host's list, and its rows elsewhere are anonymous. Risk: that project drops out of the view.
    #[test]
    fn unseen_projects_are_an_anomaly_row() {
        let w = |n| ("win".to_string(), n);
        assert_eq!(super::unseen(&[vec![w(3)], vec![w(3), ("mac".into(), 1)]]),
            ["- host win · serves 1 project outside this view · Next: add it to `<install>/projects` here (R10) or run `taskq status` on win"]);
        assert!(super::unseen(&[vec![w(2)], vec![w(2)]]).is_empty(), "every served project on a read board: no row");
    }

    // Chain: #2255 heartbeat `queue` -> status `Queues:` rows. Case 2026-11-03: win hung on re-exec 00:42-01:51Z,
    // ready work in taskq and another project never spawned, status showed only `running unknown`. Risk: a host with
    // ready work and free capacity stalls unseen; a short wait, an empty queue or a full host reads as a stall.
    #[test]
    fn queue_rows_flag_the_2026_11_03_win_stall_on_every_project() {
        let at = |t: &str| crate::time::iso(&format!("2026-11-03T{t}:00Z")).unwrap();
        let body = |full: bool, ready: u64, waiting: &str| format!("version 4.6.61\nbalancing {}\nqueue {}",
            json!({"schema": 1, "mode": "any", "default_runtime": "claude", "allowed_runtimes": null, "auto_reroute": false, "has": [], "focus": ["all"], "routing": [],
                "runtimes": crate::runtime::RUNTIMES.iter().map(|rt| (rt.to_string(), json!({"code": "hold", "reason": format!("{rt}: hold; see host diagnostics"),
                    "full": full, "disabled": false, "quota": []}))).collect::<serde_json::Map<_, _>>()}),
            json!({"restart": at("00:42"), "projects": [
                {"project": "taskqorg/taskq", "ready": ready, "spawn": at("00:30"), "waiting": at(waiting), "reason": "host restart pending"},
                {"project": null, "ready": 1, "spawn": null, "waiting": at(waiting), "reason": "host restart pending"}]}));
        let snap = |observed: &str, full, ready, waiting: &str| {
            let beat = board::Issue { title: "taskq host win".into(), body: body(full, ready, waiting), updated: at(observed) - 30, trusted: true, ..Default::default() };
            from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), at(observed), &[], &[beat])
        };
        let stalled = |s: &Snapshot| s.json()["hosts"][0]["queue"].as_array().unwrap().iter().map(|r| r["stalled"] == true).collect::<Vec<_>>();
        let hang = snap("00:55", false, 2, "00:42");
        let text = hang.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("Queues:\n- win taskqorg/taskq ready 2 · last spawn 2026-11-03 00:30 UTC · restart pending 13m · STALLED 13m\n- win other project ready 1 · last spawn none · restart pending 13m · STALLED 13m"), "{text}");
        assert!(text.contains("Anomalies:\n- host win taskqorg/taskq · STALLED 13m: ready 2 without a spawn: host restart pending · Next: inspect the host service (`taskq doctor --hosts`)\n- host win other project · STALLED 13m: ready 1"), "A4: the win hang replay is an anomaly row: {text}");
        assert_eq!(stalled(&hang), [true, true]);
        assert_eq!(stalled(&snap("00:51", false, 2, "00:42")), [false, false], "9 min is not yet a stall");
        assert_eq!(stalled(&snap("00:55", true, 2, "00:42")), [false, false], "a full host is not stalled");
        assert_eq!(stalled(&snap("00:55", false, 0, "00:42")), [false, true], "no ready work in taskq, the other project still waits");
    }

    // Chain: #2290 each host's admission pass -> its heartbeat `queue` row `tasks` -> status Ready row per host. Case
    // 2026-11-03: taskq#2200 and agentq#210 sat ready on two hosts with `no host decision`. Risk: a host's refusal
    // (weight, quota, claim mode, focus scope, label or load gate) stays invisible, a stale host or another
    // project's row speaks for this board, or a long reason crowds out the other hosts.
    #[test]
    fn ready_rows_name_each_hosts_reason() {
        let at = crate::time::iso("2026-11-03T12:00:00Z").unwrap();
        let reasons = [
            ("weight", "capacity: every candidate runtime has weight 0; pin one with run-<runtime>"),
            ("quota", "usage-limit: claude: until 2026-11-03T15:00:00Z"),
            ("claim mode", "host: spillover waits: primary mac has effective claude capacity; grace 60 s"),
            ("scope", "focus: focus p0"),
            ("label", "labels: conflicting run-* labels"),
            ("load gate", "load-gate: host load gate or 30 s launch spacing"),
        ];
        let tasks = |shift: usize| json!(reasons.iter().enumerate().map(|(i, r)| ((i + 1).to_string(), json!(reasons[(i + shift) % reasons.len()].1))).collect::<serde_json::Map<_, _>>());
        let beat = |name: &str, project: &str, age: i64, shift| board::Issue { title: format!("taskq host {name}"), trusted: true, updated: at - age,
            body: format!("version 4.6.62\nqueue {}", json!({"restart": null, "projects": [{"project": project, "ready": 6, "spawn": null, "waiting": null, "reason": null, "tasks": tasks(shift)}]})), ..Default::default() };
        let mut snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), at, &[],
            &[beat("mac", "o/r", 30, 0), beat("win", "o/r", 30, 1), beat("old", "o/r", 3600, 2), beat("other", "o/other", 30, 2)]);
        snap.tasks = (1..=7).map(|n| Row { n, title: format!("T{n}"), url: format!("u{n}"), task: Task { state: State::Ready, ..Default::default() }, ..Default::default() }).collect();
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let ready: Vec<&str> = text.lines().filter(|l| l.starts_with("| [#")).collect();
        assert_eq!(ready.len(), 7, "A1: one line per unspawned ready task: {text}");
        for (i, (kind, reason)) in reasons.iter().enumerate() {
            let code = reason.split(':').next().unwrap();
            let line = ready[i];
            assert!(line.starts_with(&format!("| [#{} T{}](u{}) | mac: {code}", i + 1, i + 1, i + 1)), "A2 {kind}: {line}");
            assert!(line.contains("; win: "), "A2 {kind}: the second host keeps its share: {line}");
            assert!(!line.contains("old:") && !line.contains("other:"), "a stale host or another project never speaks: {line}");
        }
        assert!(ready[0].contains("mac: capacity: every candidate runtime has weight 0"), "{}", ready[0]);
        assert!(ready[6].contains("| no host decision |"), "a task no host reported keeps the fallback: {}", ready[6]);
    }

    // Chain: § 4 Host link heartbeat key. Risk: an untrusted `taskq host` issue publishes a key that hosts then serve.
    #[test]
    fn host_link_keys_come_only_from_trusted_heartbeats() {
        let beat = |title: &str, trusted| board::Issue { title: title.into(), body: "version 4
link aa bb".into(), updated: 1000, trusted, ..Default::default() };
        let snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), 1000, &[], &[beat("taskq host mac", true), beat("taskq host evil", false)]);
        let hosts = snap.json()["hosts"].clone();
        assert_eq!((hosts[0]["link"].clone(), hosts[1].get("link")), (json!("aa bb"), None));
    }

    // Chain: R6 timed park -> status `Later:` line and JSON row (#1823). Risk: a timed park reads as open-ended, or
    // the owner sees the raw UTC time instead of this host's clock.
    #[test]
    fn timed_park_shows_its_local_time_and_reason() {
        let at = crate::time::iso("2026-11-05T09:00:00Z").unwrap();
        let row = |n, until: Option<&str>| Row { n, title: format!("T{n}"), url: format!("u{n}"), later: Some("Reason: 168 qualified hours".into()),
            task: Task { state: State::Later, until: until.map(str::to_string), ..Default::default() }, ..Default::default() };
        let snap = Snapshot { source: "board", observed: at, reason: None, reason_code: None, board: json!({"url": "https://github.com/o/r/issues"}), hosts: vec![],
            tasks: vec![row(1, Some("2026-11-05T09:00:00Z")), row(2, None)] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains(&format!("| [#1 T1](u1) | owner now · until {}: 168 qualified hours | `taskq requeue 1` |\n| [#2 T2](u2) | owner | `taskq requeue 2` |", crate::time::local(at, at))), "{text}");
        let off = crate::platform::utc_offset(at);
        assert!(off.abs() <= 14 * 3600 && off % 900 == 0, "a real zone offset: {off}");
        let rows = snap.json()["tasks"].clone();
        assert_eq!((rows[0]["until"].as_str(), rows[0]["later"].as_str()), (Some("2026-11-05T09:00:00Z"), Some("Reason: 168 qualified hours")));
        assert!(rows[1].get("until").is_none(), "an untimed park adds no keys");
    }

    // Chain: § 2 Stages -> R6 status State cell, `Later:` line and JSON row (#1880). Risk: a multi-stage task hides
    // its progress, or a task without stage records gains stage text or keys.
    #[test]
    fn staged_task_shows_stage_k_of_n() {
        let at = crate::time::iso("2026-11-05T09:00:00Z").unwrap();
        let row = |n, state, stage| Row { n, title: format!("T{n}"), url: format!("u{n}"), stage,
            task: Task { state, stages: if stage.is_some() { vec!["A1".into()] } else { vec![] }, ..Default::default() }, ..Default::default() };
        let snap = Snapshot { source: "board", observed: at, reason: None, reason_code: None, board: json!({"url": "https://github.com/o/r/issues"}), hosts: vec![],
            tasks: vec![row(1, State::Later, Some((2, 4))), row(2, State::Ready, Some((3, 4))), row(3, State::Ready, None)] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("| [#1 T1](u1) · stage 2/4 | owner |") && text.contains("| [#2 T2](u2) · stage 3/4 | no host decision |"), "{text}");
        assert_eq!(text.matches("stage ").count(), 2, "{text}");
        let rows = snap.json()["tasks"].clone();
        assert_eq!(rows[0]["stage"], json!({"k": 2, "n": 4, "done": ["A1"]}));
        assert!(rows[2].get("stage").is_none(), "no stage record adds no key");
    }

    // Chain: #1592 A4 one display model. Risk: the CLI and the tray page disagree on counts, order, state names or age
    // text for one board. The golden fixture feeds the page tests (tests/display-model.test.cjs) the same model.
    #[test]
    fn cli_and_page_model_agree_on_one_board() {
        let now = 200_000;
        let row = |n, state, labels: &[&str], ago: i64| Row { n, title: format!("T{n}"), labels: labels.iter().map(|l| l.to_string()).collect(), url: format!("https://github.com/o/r/issues/{n}"),
            task: Task { state, ..Default::default() }, since: Some(now - ago), stuck: (state == State::Ask).then_some(ago), ..Default::default() };
        let snap = Snapshot { source: "board", observed: now, reason: None, reason_code: None, board: json!({"url": "https://github.com/o/r/issues"}), hosts: vec![], tasks: vec![
            row(3, State::Ready, &["p1"], 300), row(5, State::Ready, &[], 7200), row(7, State::Doing, &[], 60), row(8, State::Ready, &["p0"], 30),
            row(9, State::Ask, &[], 9000), row(2, State::Held, &[], 90_000), row(4, State::Later, &[], 600)] };
        // the `from_views` key: priority, then number
        let mut snap = snap;
        snap.tasks.sort_by_key(|r| (crate::display::priority(&r.labels), r.n));
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        let mut ps = [json!({"name": "a", "root": "a", "issues": "https://github.com/o/r/issues", "board_kind": "github", "complete": true, "focus": [], "saved": true,
            "tasks": snap.json()["tasks"]})];
        let display = crate::display::model(&mut ps, now);
        let n = |s: &str| display["all"]["counts"].as_array().unwrap().iter().find(|c| c["state"] == s).unwrap()["n"].as_u64().unwrap();
        assert_eq!(text.lines().nth(1).unwrap(), format!("In work {} · Waiting for answer {} · Ready {}", n("doing") + n("review"), n("ask"), n("ready")));
        let rows = ps[0]["tasks"].as_array().unwrap();
        let ready: Vec<String> = rows.iter().filter(|t| t["state"] == "ready").map(|t| format!("[#{} ", t["n"])).collect();
        let table: Vec<&str> = text.lines().filter(|l| l.contains("| no host decision |")).collect();
        assert_eq!(table.len(), ready.len(), "{text}");
        assert!(table.iter().zip(&ready).all(|(l, r)| l.starts_with(&format!("| {r}"))), "ready order {ready:?}\n{text}");
        for (t, reason) in rows.iter().filter_map(|t| match t["state"].as_str() { Some("held") => Some((t, "PM")), Some("later") => Some((t, "owner")), _ => None }) {
            assert!(text.lines().any(|l| l.starts_with(&format!("| [#{} ", t["n"])) && l.contains(&format!(") | {reason} |"))), "{text}");
        }
        let ask = rows.iter().find(|t| t["state"] == "ask").unwrap();
        assert!(text.contains(&format!("| owner · stuck {} |", ask["age"].as_str().unwrap())), "{text}");
        assert_eq!(ps[0]["mode"], "paused");
        // the first line names the per-test project directory
        let got = json!({"cli": text.lines().skip(1).collect::<Vec<_>>().join("\n"), "now": now, "display": display, "projects": ps});
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/display-model.json");
        if std::env::var_os("TASKQ_BLESS").is_some() { std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap(); }
        let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
    }

    // Chain: #1524 menu.md M3 Task row text. Risk: a raw execution, fence or wait diagnostic reaches a tray or status row.
    #[test]
    fn every_wait_producer_maps_to_a_spec_named_owner_text() {
        let spec = include_str!("../docs/design/menu.md");
        for t in WAIT_TEXTS { assert!(spec.contains(&format!("`{t}`")), "menu.md names {t}"); }
        // every raw producer of `execution_wait`: service waits, execution statuses, gui eligibility, snapshot fallbacks
        let cases = [
            ("launch held by admission gate", 0),
            ("GUI unavailable on mac: no session", 1), ("GUI runtime codex refused: sandbox", 1), ("GUI runtime claude has no effective Full grant", 1),
            ("native input acceptance pending", 2), ("execution 3 on mac: input acceptance unknown; reconcile native input", 2),
            ("input pending on win; await or restore the owning service", 2),
            ("execution 3 on mac: waiting: owner answer required", 3),
            ("execution 3 on mac: termination unconfirmed", 4), ("cancellation pending; awaiting native termination", 4),
            ("execution 3 on mac: running: native execution observed; termination pending", 4),
            ("execution 3 on mac: unknown: termination unconfirmed; status missing or stale; restore owning service", 5),
            ("native observation unavailable; restore owning runtime", 5), ("execution 3: native observation deferred; recovery budget exhausted", 5),
            ("execution 3 on mac: recovery unknown: io", 5), ("multiple unresolved executions; verify native ownership before recovery", 4),
            ("native execution evidence missing; reconcile the owning runtime", 5), ("session identity unknown; recover the original native session", 5),
            ("Claude returned no valid background ID; launch outcome unknown; reconcile the original input", 2),
            ("spawn 3 not started on mac: input not accepted; lost spawn released no-start 300 s after the spawn", 2),
            ("execution 3 on mac: termination unconfirmed; reboot mac to prove exit", 4),
            ("execution 3 on hermes: termination unconfirmed; start the taskq service on hermes once, after a reboot if the run had started", 4),
            ("execution 3 on mac: unknown: agent exited; input acceptance unknown; reconcile native input", 6),
        ];
        for (raw, k) in cases { assert_eq!(wait_text(raw), WAIT_TEXTS[k], "{raw}"); }
        // status text and row JSON: owner text only in the row, raw reason kept in --json
        let raw = cases[11].0;
        let row = Row { heartbeat: Value::Null, worker: None, workers: vec![], conversation: None, brief: String::new(), n: 1, title: "T".into(), url: "u1".into(), task: Task { state: State::Doing, ..Default::default() }, execution_wait: Some(raw.into()), ..Default::default() };
        let snap = Snapshot { source: "board", observed: 0, reason: None, reason_code: None, board: json!({"url": "b"}), tasks: vec![row], hosts: vec![] };
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap();
        assert!(text.contains("| doing · Run state unknown; see service log |") && !text.contains("termination") && !text.contains("execution 3"), "{text}");
        let j = &snap.json()["tasks"][0];
        assert_eq!((j["wait"].as_str(), j["execution_wait"].as_str()), (Some(WAIT_TEXTS[5]), Some(raw)));
        // #2257: a result with no reviewer has no live agent, so an input-class diagnostic reads as the handover
        let handover = Task { state: State::Review, result: Some("abc".into()), ..Default::default() };
        let row = Row { task: handover.clone(), execution_wait: Some(cases[4].0.into()), ..Default::default() };
        assert_eq!(row.wait().as_deref(), Some(WAIT_TEXTS[4]));
        let row = Row { task: Task { attempt: Some(9), ..handover }, execution_wait: Some(cases[4].0.into()), ..Default::default() };
        assert_eq!(row.wait().as_deref(), Some(WAIT_TEXTS[2]), "a live reviewer keeps the input wait");
    }

    // #2159 A4/R1: last accepted worker survives terminal state projection, without becoming current authority.
    #[test]
    fn retained_worker_identity_comes_only_from_accepted_board_projection() {
        let log=vec![core::Entry{id:9,body:"taskq: spawn worker codex:- mac".into(),edited:false},
            core::Entry{id:10,body:"taskq: later\nReason: checkpoint".into(),edited:false},
            core::Entry{id:11,body:"taskq: spawn worker codex:forged other".into(),edited:false}];
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let snap=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[]);
        let row=snap.tasks[0].json();
        assert!(row["attempt"].is_null() && row["session"].is_null(),"read identity cannot restore task authority");
        assert_eq!(row["worker"],json!({"attempt":9,"session":"codex:- mac","role":"worker"}),"ignored spawn cannot select retained history");
        assert_eq!(row["workers"].as_array().unwrap().iter().map(|w|w["attempt"].as_u64().unwrap()).collect::<Vec<_>>(),[9],"history enumerates accepted sources only");
    }

    // #1775 A3: a reviewer spawned in review leaves `changes` (state moves only). Risk: its execution row has no role,
    // the menu host strip reads the host as unknown and hides its Claude/Codex counts.
    #[test]
    fn reviewer_execution_row_carries_reviewer_role() {
        let sha = "a".repeat(40);
        let log = vec![crate::core::Entry{id:9,body:"taskq: spawn worker claude:s0 mac".into(),edited:false},
            crate::core::Entry{id:10,body:format!("taskq: result {sha} attempt 9\nChanged: x"),edited:false},
            crate::core::Entry{id:11,body:"taskq: spawn reviewer codex:- mac".into(),edited:false}];
        let v = View {issue:board::Issue{number:1,..Default::default()},projection:crate::core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let snap = from_views("board", &crate::board::Fake::default(), &crate::tests::cfg(), 1000, &[v], &[]);
        let roles: Vec<_> = snap.tasks[0].executions.iter().map(|x| (x["attempt"].as_u64(), x["role"].as_str())).collect();
        assert!(roles.contains(&(Some(11), Some("reviewer"))), "{roles:?}");
    }

    // R6/R12, #2173: a fresh lease must not hide missing execution progress.
    // R6/R12, #2173 note 6574454403: an accepted board answer is not a native executor.
    #[test]
    fn logical_doing_after_answer_waits_for_an_exact_execution() {
        let log=vec![core::Entry{id:5,body:"taskq: spawn worker codex:invented win".into(),edited:false},
            core::Entry{id:6,body:"taskq: accepted 5 attempt 5".into(),edited:false},
            core::Entry{id:7,body:"taskq: ask attempt 5\nDone: checked\nProblem: owner decision".into(),edited:false},
            core::Entry{id:8,body:"taskq: released 5 exit attempt 5".into(),edited:false},
            core::Entry{id:9,body:"taskq: answer\nContinue".into(),edited:false}];
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log:log.clone(),comments:vec![],blockers:vec![]};
        let host=board::Issue{trusted:true,title:"taskq host win".into(),updated:1000,body:"#1 5".into(),..Default::default()};
        let row=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[host]).tasks[0].json();
        assert_eq!(row["state"],"doing");assert!(row["executions"].is_null());
        assert_eq!(row["heartbeat"]["executor"]["state"],"awaiting execution");
        assert!(row["heartbeat"]["executor"]["reason"].as_str().unwrap().contains("input recorded"));
        assert_eq!(row["stuck"],false,"ordinary delivery wait is not a failed task");
        assert_eq!(core::executions(1,&log).len(),1,"projection cannot submit another execution");
        let mut log=log;
        log.push(core::Entry{id:10,body:"taskq: executing 9 attempt 5".into(),edited:false});
        log.push(core::Entry{id:11,body:"taskq: accepted 10 attempt 5".into(),edited:false});
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let host=board::Issue{trusted:true,title:"taskq host win".into(),updated:1000,
            body:format!("#1 5\nexecution {}",json!({"task":1,"attempt":5,"id":10,"through":9,"state":"running","reason":"native executor observed"})),..Default::default()};
        for (now,expected) in [(1000,"active"),(1301,"unknown")] {
            let row=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),now,std::slice::from_ref(&v),std::slice::from_ref(&host)).tasks[0].json();
            assert_eq!(row["heartbeat"]["executor"]["state"],expected,"a stale remote executor cannot read active");
        }
    }

    #[test]
    fn heartbeat_health_is_explicit_even_without_an_execution_row() {
        let log=vec![core::Entry{id:5,body:"taskq: spawn worker codex:- mac".into(),edited:false}];
        let v=View{issue:board::Issue{number:1,..Default::default()},projection:core::project(1,&log,false),log,comments:vec![],blockers:vec![]};
        let snap=from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[]);
        let row=snap.tasks[0].json();
        assert_eq!(row["heartbeat"]["freshness"],"unknown");
        assert!(row["heartbeat"]["reason"].as_str().unwrap().contains("missing"));
        assert!(row["heartbeat"]["warning"].is_string());
        assert_eq!(row["state"],"doing","health never changes board state");
    }

    // Chain: R6 manager status -> shared heartbeat warnings. Risk: health is hidden, duplicated or re-detected differently.
    #[test]
    fn stale_heartbeats_block_reuses_shared_health_and_clears() {
        let now=10_000;let mut cfg=crate::tests::cfg();cfg.root="invented-project".into();
        let quiet=json!({"source":"native execution events","clock":"native timestamp or append mtime lower bound","observed":9990,"status":"quiet","last_event":9990,"pending_tools":0,"reason":"no pending tool; quiet work is not failure"});
        let row=|n,host_age:Option<i64>,progress:Value,pending:bool| {
            let task=Task{state:State::Doing,attempt:Some(5),session:Some("codex:invented mac".into()),..Default::default()};
            let executions=if pending {vec![]} else {vec![json!({"id":10,"attempt":5,"host":"mac","observed":now,"state":"running","reason":"native executor observed","progress":progress})]};
            let hosts=host_age.map(|age|vec![board::Issue{trusted:true,title:"taskq host mac".into(),updated:now-age,body:format!("#{n} 5"),..Default::default()}]).unwrap_or_default();
            let mut health=heartbeat(&task,n,&hosts,&executions,now,cfg.ttl);
            health["executor"]=executor(&task,&executions,pending,now,cfg.ttl);refresh_heartbeat(&mut health,now,true);
            Row{n,title:format!("Invented task {n}"),url:format!("https://github.com/invented/project/issues/{n}"),task,executions,heartbeat:health,..Default::default()}
        };
        let mut error=crate::runtime::progress::unknown("native progress record invalid");error["observed"]=json!(9990);
        let overdue=json!({"source":"native execution events","observed":9990,"status":"overdue","last_event":8800,"tool_since":8000,"pending_tools":1,"overdue_since":8900,"reason":"tool progress overdue; preserve execution and inspect owning session"});
        let mut stale=quiet.clone();stale["observed"]=json!(now-cfg.ttl-1);stale["last_event"]=stale["observed"].clone();
        for progress in [&quiet,&stale,&overdue] {assert!(crate::runtime::progress::valid(progress),"fixture retains native future guards");}
        let mut unknown=row(6,Some(10),error.clone(),false);unknown.executions[0]["state"]=json!("unknown");
        unknown.executions[0]["reason"]=json!("native identity unavailable");
        unknown.heartbeat["executor"]=executor(&unknown.task,&unknown.executions,false,now,cfg.ttl);refresh_heartbeat(&mut unknown.heartbeat,now,true);
        let mut question=Row{n:9,title:"Owner question".into(),url:"q9".into(),task:Task{state:State::Ask,card:Some("Problem: owner choice\n1. keep ★\n2. change".into()),..Default::default()},stuck:Some(9000),..Default::default()};
        question.heartbeat=Value::Null;
        let mut snap=Snapshot{source:"board",observed:now,reason:None,reason_code:None,board:json!({"url":"https://github.com/invented/project/issues"}),hosts:vec![],
            tasks:vec![row(1,Some(10),quiet.clone(),false),row(2,Some(cfg.ttl+1),quiet.clone(),false),row(3,None,error.clone(),false),row(4,Some(10),error,false),row(5,Some(10),overdue,false),unknown,row(7,Some(10),Value::Null,true),row(8,Some(10),stale,false),question]};
        snap.tasks[5].task.state=State::Review;
        let decide=json!({"complete":true,"rows":[{"n":20,"title":"Completed result","url":"r20","verdict":"confirmed","age_s":600,"recommendation":"Discuss result"}]});
        let text=snap.text_with(&cfg,&json!({}),Some(&decide)).unwrap();
        assert!(text.contains("Anomalies:"),"manager status must expose health without opening a native task detail: {text}");
        assert!(text.find("Owner (").unwrap()<text.find("Decide:").unwrap() && text.find("Decide:").unwrap()<text.find("Anomalies:").unwrap() && text.find("Anomalies:").unwrap()<text.find("Running:").unwrap());
        assert!(text.contains("Completed result") && text.contains("Discuss result"),"completed results remain discoverable");
        assert_eq!(text.lines().filter(|l|l.contains("stuck 2h")).collect::<Vec<_>>(),["| [#9 Owner question](q9) | owner · stuck 2h | answer the Owner row |"],"health tasks must not duplicate the stuck row");
        let block=format!("Anomalies:{}\n",text.split_once("Anomalies:").unwrap().1.split("\n\n").next().unwrap());
        let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/stale-heartbeats.txt");
        if std::env::var_os("TASKQ_BLESS").as_deref()==Some(std::ffi::OsStr::new("1")) {std::fs::write(&path,&block).unwrap();}
        assert_eq!(block,std::fs::read_to_string(path).unwrap().replace("\r\n","\n"));
        assert!(!block.contains("Invented task 1"));
        let long="x".repeat(200);snap.tasks[5].heartbeat["progress"]["clock"]=json!(long);
        assert!(snap.text(&cfg,&json!({})).unwrap().contains(&format!("native execution events ([{}]({}))",cell(&compact(&long,160)),snap.tasks[5].url)),"long source metadata links to its full source");
        snap.tasks[1].heartbeat["observed"]=json!(now+60);refresh_heartbeat(&mut snap.tasks[1].heartbeat,now,true);
        assert!(snap.text(&cfg,&json!({})).unwrap().contains(" · <1m ago · "),"a future observation reads as the shortest age, never a negative one");
        let json_before=snap.json();
        for r in &mut snap.tasks[..8] {
            r.heartbeat["freshness"]=json!("fresh");r.heartbeat["observed"]=json!(now);r.heartbeat["executor"]=json!({"state":"active","observed":now});r.heartbeat["progress"]=quiet.clone();
            refresh_heartbeat(&mut r.heartbeat,now,true);
        }
        assert!(!snap.text(&cfg,&json!({})).unwrap().contains("Anomalies:"),"recovery omits an empty block");
        assert_eq!(json_before["tasks"][4]["heartbeat"]["progress"]["status"],"overdue","JSON/PM consumers retain the same typed projection");
    }

    #[test]
    fn execution_row_within_ttl_reads_running() {
        let v = execution_status(1, &exec(), &[host(90)], 1000, 300);
        assert_eq!((v["state"].as_str(), v["observed"].as_i64()), (Some("running"), Some(910)));
    }

    #[test]
    fn execution_row_older_than_ttl_reads_unknown() {
        let v = execution_status(1, &exec(), &[host(301)], 1000, 300);
        assert_eq!(v["state"], "unknown");
        assert_eq!(v["reason"], "termination unconfirmed; status missing or stale; restore owning service");
        assert!(v["observed"].is_null());
    }

    // #1594 A3: the owner's verdict, published in its heartbeat and read by another host or the tray, stays the same;
    // only a childless verdict other than `ended` holds a handover. Risk: a caller re-derives liveness and a fix lands
    // in one place only (#947 foreign or childless execution, #1036 exited child, #1386 boot proof, #1481/#1491
    // heartbeat ttl, #1551 cleanup after reboot).
    #[test]
    fn one_verdict_through_every_caller() {
        let (done, failed, stopping) = (Child { exited: true, ..Default::default() }, Child { unknown: Some("io"), ..Default::default() }, Child { stopping: true, ..Default::default() });
        let (rec, reused) = (coreq::process::Identity { pid: 9, started: "1-0".into() }, coreq::process::Identity { pid: 9, started: "2-0".into() });
        let cases: Vec<(&str, Signals, &str, &str)> = vec![
            ("#947 childless", Signals { local: true, ..Default::default() }, "unknown", "native observation unavailable; restore owning runtime"),
            ("#947 recovery wait", Signals { local: true, wait: Some("termination unconfirmed"), ..Default::default() }, "unknown", "termination unconfirmed"),
            ("#1036 exited", Signals { local: true, child: Some(done), ..Default::default() }, "ended", "native exit observed"),
            ("#1036 observation failed", Signals { local: true, child: Some(failed), ..Default::default() }, "unknown", "io"),
            ("stopping", Signals { local: true, child: Some(stopping), ..Default::default() }, "waiting", "cancellation pending; awaiting native termination"),
            ("running", Signals { local: true, child: Some(Child::default()), ..Default::default() }, "running", "native execution observed; termination pending"),
            ("input", Signals { local: true, child: Some(Child::default()), input_pending: true, ..Default::default() }, "waiting", "native input acceptance pending"),
            ("ask", Signals { local: true, child: Some(Child::default()), ask: true, ..Default::default() }, "waiting", "owner answer required"),
            ("held", Signals { local: true, held: true, ..Default::default() }, "waiting", "launch held by admission gate"),
            ("#1386/#1551 boot after intent", Signals { local: true, boot: Some((200, 100)), ..Default::default() }, "ended", "host booted 1970-01-01 00:03 UTC after intent 1970-01-01 00:01 UTC"),
            ("#1386 boot before intent", Signals { local: true, boot: Some((100, 200)), ..Default::default() }, "unknown", "native observation unavailable; restore owning runtime"),
            ("#1481 no heartbeat", Signals::default(), "unknown", STALE),
            ("#1561 recorded process exited", Signals { local: true, process: Some((&rec, Ok(None))), ..Default::default() }, "ended", "recorded native process exited"),
            ("#1561 recorded pid reused", Signals { local: true, process: Some((&rec, Ok(Some(reused.clone())))), ..Default::default() }, "ended", "recorded native process exited"),
            ("#1561 recorded process live", Signals { local: true, process: Some((&rec, Ok(Some(rec.clone())))), ..Default::default() }, "running", "recorded native process observed"),
            ("#1561 identity read failed", Signals { local: true, process: Some((&rec, Err("io".into()))), ..Default::default() }, "unknown", "recorded native process identity unknown: io"),
        ];
        for (name, s, state, reason) in cases {
            let (got, why) = verdict(&s);
            assert_eq!((got, why.as_str()), (state, reason), "{name}");
            if got == "ended" { continue; }
            // the owner publishes `execution_statuses` rows; the reader and the tray read them back through `execution_status`
            let row = ExecutionStatus { progress: None, task: 1, attempt: 5, id: 7, through: 7, state: got.into(), reason: why.clone() };
            let issue = board::Issue { body: format!("execution {}", serde_json::to_string(&row).unwrap()), ..host(10) };
            let v = execution_status(1, &exec(), &[issue.clone()], 1000, 300);
            assert_eq!((v["state"].as_str(), v["reason"].as_str()), (Some(got), Some(why.as_str())), "{name} via heartbeat");
            // #1491: the same row past the ttl reads unknown
            let v = execution_status(1, &exec(), &[issue], 1301, 300);
            assert_eq!((v["state"].as_str(), v["reason"].as_str()), (Some("unknown"), Some(STALE)), "{name} stale");
        }
    }
    // CS2/R12: poll, labels, edited and untrusted comments cannot manufacture conversation activity.
    #[test]
    fn sidebar_activity_projects_trusted_comment_time_not_observation() {
        let log=vec![];let mut v=View {issue:board::Issue{number:1,created:100,updated:999,..Default::default()},projection:crate::core::project(1,&log,false),log,comments:vec![
            board::Comment{id:1,created:150,trusted:true,edited:false,..Default::default()},
            board::Comment{id:2,created:500,trusted:false,edited:false,..Default::default()},
            board::Comment{id:3,created:600,trusted:true,edited:true,..Default::default()}],blockers:vec![]};
        let read=|v:View|from_views("board",&crate::board::Fake::default(),&crate::tests::cfg(),1000,&[v],&[]).json()["tasks"][0]["updated"].clone();
        assert_eq!(read(View{issue:v.issue.clone(),projection:crate::core::project(1,&[],false),log:vec![],comments:v.comments.clone(),blockers:vec![]}),150);
        // #1930 A4 explicitly removes the issue-creation fallback while retaining trusted/unedited filtering.
        v.comments.push(board::Comment{id:4,created:0,trusted:true,edited:false,..Default::default()});
        assert!(read(View{issue:v.issue.clone(),projection:crate::core::project(1,&[],false),log:vec![],comments:v.comments.clone(),blockers:vec![]}).is_null(),"a missing newest event cannot borrow an older comment time");
        v.comments.clear();assert!(read(v).is_null(),"issue creation is not conversation activity");
    }

    // Chain: A5 board failure -> status. Risk: an unread board reports only text, so a reader must match words to learn why.
    #[test]
    fn an_unread_board_reports_its_reason_code() {
        let b = crate::board::replying(&crate::board::github::KIND, |_, url| Err(board::Error::http(403, format!("GET {url}: HTTP 403 denied"))));
        let snap = read(&*b, &crate::tests::cfg(), &Value::Null, &[]);
        let wire = snap.json();
        assert_eq!((wire["complete"].clone(), wire["reason_code"].clone()), (json!(false), json!("forbidden")));
        assert!(wire["reason"].as_str().unwrap().contains("HTTP 403 denied"), "{wire}");
        let text = snap.text(&crate::tests::cfg(), &json!({})).unwrap_err();
        assert!(text.ends_with(" (reason: forbidden)"), "{text}");
    }

    // Chain: R6 Anomalies, #2432 A2. Risk: a live daemon with another project's sandbox holds jobs and status never says
    // so, or an untrusted heartbeat invents one, or a stale heartbeat keeps naming a daemon that has gone.
    #[test]
    fn a_daemon_that_differs_from_a_jobs_grant_is_an_anomaly() {
        let at = 10_000;
        let beat = |name: &str, age: i64, trusted: bool, line: &str| board::Issue { title: format!("taskq host {name}"), body: format!("version 4\n{line}"), updated: at - age, trusted, ..Default::default() };
        let differs = r#"daemon-sandbox {"bin":"claude","pid":77,"reason":"daemon runs under another Seatbelt profile","tasks":[5,6]}"#;
        let unknown = r#"daemon-sandbox {"bin":"claude","pid":null,"reason":"daemon status unreadable","tasks":[7]}"#;
        let cfg = crate::tests::cfg();
        let snap = from_views("board", &crate::board::Fake::default(), &cfg, at, &[], &[
            beat("mac", 10, true, differs), beat("next", 10, true, unknown), beat("forged", 10, false, differs), beat("old", cfg.ttl + 60, true, differs),
        ]);
        let text = snap.text(&cfg, &json!({})).unwrap();
        assert!(text.contains("- host mac · live claude daemon pid 77 sandbox differs from the grant of #5, #6: daemon runs under another Seatbelt profile · Next: jobs wait until the daemon exits; run `claude daemon status` on mac"), "{text}");
        assert!(text.contains("- host next · claude daemon sandbox unknown for #7: daemon status unreadable · Next:"), "{text}");
        assert!(!text.contains("- host forged · live") && !text.contains("- host old · live"), "{text}");
    }
