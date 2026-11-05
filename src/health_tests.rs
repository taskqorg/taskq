    use super::*;
    use crate::snapshot::Host;

    // Chain: § 6 host setup -> `taskq doctor` token checks (#2194 A1, A4). Risk: a fine-grained token without
    // Contents: Read let the board work while publication and reports failed with raw 403 errors and no named cause.
    // Protection: a 403 on the contents read is a `token-contents` gap naming the permission, the status and one fix.
    #[test]
    fn doctor_names_a_token_without_contents_read() {
        let items = token_items("o/r", false, &[("Issues", Probe::Ok), ("Contents", Probe::Gap("HTTP 403".into())), ("Pull requests", Probe::Ok)]);
        let c = items.iter().find(|i| i.id == "token-contents").unwrap();
        assert_eq!((c.status, c.text.as_str()), ("gap", "token lacks Contents: Read on o/r (HTTP 403)"));
        assert!(c.fix.as_deref().unwrap().contains("Contents: Read"), "{:?}", c.fix);
        assert!(items.iter().filter(|i| i.id != "token-contents").all(|i| i.status == "ok"));
        assert_eq!(line(7, &items), "doctor 7 token-contents:gap");
        assert_eq!(http_status("GET /commits?per_page=1: HTTP 403 Resource not accessible"), Some(403));
        assert!(token_items("o/r", false, &[]).iter().all(|i| i.status == "unknown"), "an unsupported board is unknown, never ok");
    }

    // Chain: § 6 GitLab board token -> `taskq doctor` token checks (#2246 A2). Risk: GitLab boards had no token
    // probe, so every token cell read unknown and a read-only token passed setup unnoticed. Protection: the token's own
    // scopes and the project role decide each permission; a missing scope or a low role is a gap with a GitLab fix,
    // an unread fact stays unknown.
    #[test]
    fn doctor_checks_gitlab_token_scopes_and_role() {
        let dev = Ok(json!({"permissions": {"project_access": {"access_level": 30}, "group_access": null}}));
        let full = gitlab_access(&Ok(json!({"active": true, "revoked": false, "scopes": ["api"]})), &dev);
        assert!(full.iter().all(|(_, p)| *p == Probe::Ok), "{full:?}");
        let read = gitlab_access(&Ok(json!({"active": true, "scopes": ["read_api"]})), &Ok(json!({"permissions": {"group_access": {"access_level": 20}}})));
        assert_eq!(read, vec![("Issues", Probe::Gap("scopes read_api lack api".into())), ("Contents", Probe::Ok), ("Pull requests", Probe::Gap("scopes read_api lack api".into()))]);
        let low = gitlab_access(&Ok(json!({"active": true, "scopes": ["api"]})), &Ok(json!({"permissions": {"project_access": {"access_level": 20}}})));
        assert_eq!(low[2].1, Probe::Gap("project role 20 below 30".into()));
        assert!(matches!(gitlab_access(&Ok(json!({"active": false, "scopes": ["api"]})), &dev)[0].1, Probe::Gap(_)));
        assert!(gitlab_access(&Err("HTTP 401".into()), &dev).iter().all(|(_, p)| matches!(p, Probe::Unknown(_))), "unread scopes: unknown, never ok");
        let items = token_items("g/p", true, &read);
        assert_eq!(items[0].fix.as_deref(), Some("owner: give the GitLab board token scope api and role Reporter or higher on g/p"));
        assert!(!items.iter().filter_map(|i| i.fix.as_deref()).any(|f| f.contains("fine-grained")), "no GitHub fix on GitLab");
    }

    // Chain: § 8 update -> `taskq doctor` version check (#2194 A1, A4). Risk: a host stayed on an old release with no
    // sign in doctor. Protection: behind the latest release is a gap with `taskq update`; an unread release is unknown.
    // #2246: a stale service status names restart, never `service install` for a service that exists.
    #[test]
    fn doctor_flags_an_outdated_version() {
        let v = version_item("4.6.58", &Ok("4.6.60".into()));
        assert_eq!((v.status, v.text.as_str(), v.fix.as_deref()), ("gap", "installed 4.6.58 is behind the latest release 4.6.60", Some("run `taskq update`")));
        assert_eq!(version_item("4.6.60", &Ok("4.6.60".into())).status, "ok");
        assert_eq!(version_item("4.7.0-dev.1", &Ok("4.6.60".into())).status, "ok", "a newer dev build is not behind");
        assert_eq!(version_item("4.6.58", &Err("no network".into())).status, "unknown");
        let r = reports_item(&json!({"complete": false, "reason": "Report collection is stale; waiting for the service"}), false);
        assert_eq!((r.status, r.text.as_str()), ("gap", "report collection incomplete: Report collection is stale; waiting for the service"));
        assert_eq!(reports_item(&json!({"complete": true}), false).status, "ok");
        let e = error_item(Ok(Some(json!({"error": "board unavailable: HTTP 502\ndetail"}))), Some(5), 120);
        assert_eq!((e.status, e.text.as_str()), ("gap", "last service pass failed: board unavailable: HTTP 502"));
        assert_eq!(error_item(Ok(Some(json!({"error": null}))), Some(5), 120).status, "ok");
        let stale = error_item(Ok(Some(json!({"error": null}))), Some(500), 120);
        assert_eq!(stale.status, "unknown", "a stale status is unknown");
        assert!(!stale.fix.unwrap().contains("install"), "an existing service is restarted, not installed");
        assert_eq!(error_item(Ok(None), None, 120).status, "unknown");
    }

    // Chain: reports collection (#2195) -> `taskq doctor` reports check -> heartbeat `doctor-reports` -> `--hosts`
    // (#2194 A1, #2246 A2). Risk: decided historical reports whose source went 404 kept a `reports` gap on every host
    // with no cause, and an undecided unreadable report read ok. Protection: only undecided report rows count; the
    // gap and the heartbeat name the cause and the reports; the `--hosts` fix follows the cause.
    #[test]
    fn doctor_reports_name_missing_reports_and_skip_decided_ones() {
        let ok = reports_item(&json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "task", "n": 5, "reason": "No accepted hand-in recorded; result details unknown"},
                {"kind": "report", "n": 6, "visible": false, "missing": true, "reason": "#6 source: HTTP 404"}]}), false);
        assert_eq!(ok.status, "ok", "a decided report with a gone source needs nothing");
        let rows = json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "report", "n": 7, "visible": true, "reason": "#7 source: GET /contents/x: HTTP 403 Resource not accessible"},
                {"kind": "report", "n": 8, "missing": true, "reason": "#8 source: HTTP 404"}]});
        assert_eq!(reports_detail(&rows).as_deref(), Some("denied #7 #8"));
        let bad = reports_item(&rows, false);
        assert_eq!((bad.status, bad.text.as_str()), ("gap", "2 undecided report(s) unreadable (denied: #7 #8), first: #7 source: GET /contents/x: HTTP 403 Resource not accessible"));
        assert_eq!(bad.fix.as_deref(), Some("reports #7 #8 unreadable (HTTP 401/403): owner: give the board token of this host Contents: Read"));
        let many: Vec<Value> = (1..=7).map(|n| json!({"kind": "report", "n": n, "missing": true, "reason": "source: HTTP 404"})).collect();
        let gone = json!({"complete": true, "reports": many});
        assert_eq!(reports_detail(&gone).as_deref(), Some("missing #1 #2 #3 #4 #5 +2"));
        assert_eq!(reports_detail(&json!({"complete": false, "reason": "Reports not collected yet"})).as_deref(), Some("uncollected"));
        assert_eq!(fix("reports", "gap", "o/r", "win", false, Some("missing #1 #2 +2")),
            "reports #1 #2 +2 have no source file at their result SHA: on win run `taskq results 1 --action followup` or `--action reopen`");
        assert_eq!(fix("reports", "gap", "o/r", "win", false, Some("evil text")), "run `taskq doctor` on win: its version names no report cause", "only known causes");
    }

    // Chain: § 4 step 3 heartbeat `doctor` line -> `taskq doctor --hosts` (#2194 A2, A4). Risk: a dead or old host
    // read as healthy, a hand-written heartbeat injected text, or (#2246 A1, A2) a heartbeat updated after the reader's
    // clock read `<1m old; host unreachable` with `service install`. Protection: a heartbeat newer than `now` is live; a
    // stale one names the silent service and restart; an old host reads unknown with its version; unknown ids refused.
    #[test]
    fn doctor_hosts_reads_stale_and_old_hosts_unknown_and_fresh_ones_live() {
        let host = |name: &str, updated, version: &str, doctor: Option<&str>| Host { name: name.into(), version: version.into(), updated, doctor: doctor.map(String::from), ..Default::default() };
        let now = 10_000;
        let gone = host_row(&host("mac", now - 1500, "4.6.60", Some("9000 ok")), "o/r", false, now, 120, None);
        assert_eq!((gone["state"].as_str(), gone["reason"].as_str()), (Some("unknown"), Some("heartbeat 25m old, read fresh; its service is not writing")));
        assert!(!gone["fix"].as_str().unwrap().contains("install"), "{gone}");
        let ahead = host_row(&host("pds", now + 30, "4.6.60", Some("9990 ok")), "o/r", false, now, 120, None);
        assert_eq!(ahead["state"], "ok", "a heartbeat written after the reader's clock is fresh: {ahead}");
        let old = host_row(&host("lin", now - 10, "4.6.50", None), "o/r", false, now, 120, None);
        assert_eq!((old["state"].as_str(), old["reason"].as_str()), (Some("unknown"), Some("version 4.6.50 runs no health checks")));
        let gap = host_row(&host("win", now - 10, "4.6.58", Some("9900 token-contents:gap version:gap")), "o/r", false, now, 120, None);
        assert_eq!(gap["state"], "gap");
        assert_eq!(gap["gaps"], json!(["token-contents", "version"]));
        assert_eq!(gap["fix"], "owner: grant the board token of win Contents: Read for o/r; run `taskq update` on win");
        let gl = host_row(&host("pds", now - 10, "4.6.62", Some("9900 token-pulls:gap")), "g/p", true, now, 120, None);
        assert_eq!(gl["fix"], "owner: give the GitLab board token of pds scope api and role Developer or higher on g/p");
        let unread = host_row(&host("pds", now - 10, "4.6.62", Some("9900 token-issues:unknown")), "g/p", true, now, 120, None);
        assert_eq!(unread["fix"], "run `taskq doctor` on pds: it names why the token read failed", "an unread token asks for no grant");
        assert_eq!(host_row(&host("ok", now - 10, "4.6.60", Some("9900 ok")), "o/r", false, now, 120, None)["state"], "ok");
        let lapsed = host_row(&host("ok", now - 10, "4.6.60", Some(&format!("{} ok", now - STALE - 1))), "o/r", false, now, 120, None);
        assert_eq!(lapsed["state"], "unknown", "a result the service stopped refreshing is unknown");
        assert_eq!(parse("9900 rm-rf:gap"), None, "only known ids");
        assert_eq!(parse("9900 version:fine"), None, "only known statuses");
        assert_eq!(parse("9900"), None);
        let tag = |mut v: Value, b: &str| { v["board"] = json!(b); v };
        let lines = hosts_lines(&[tag(gone, "o/r"), tag(gap, "o/r"), tag(gl, "g/p")],
            &[json!({"board": "g/p", "host": "win", "task": 7, "attempt": 42, "runtime": "claude", "alive": true, "stalled": false, "reason": null, "account": "ok (account work)"})]);
        assert!(lines[0].starts_with("board") && lines[0].contains("host"), "one host table with a board column: {lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("o/r") && l.contains("mac") && l.contains("not writing")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("g/p") && l.contains("pds")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("g/p ") && l.contains("#7") && l.contains("claude") && l.contains("yes")), "{lines:#?}");
        // #2247 A1: the version column shows running and, during a handover, selected with its waits; equal is one value
        let mut handover = host("win", now - 10, "4.6.61", Some("9900 ok"));
        handover.selected = Some("4.6.62 waits #7 attempt 42".into());
        let row = host_row(&handover, "o/r", false, now, 120, None);
        assert_eq!((row["version"].as_str(), row["running_selected"].as_str()), (Some("4.6.61"), Some("4.6.61 (selected 4.6.62 waits #7 attempt 42)")));
        let lines = hosts_lines(&[tag(row, "o/r"), tag(host_row(&host("lin", now - 10, "4.6.62", Some("9900 ok")), "o/r", false, now, 120, None), "o/r")], &[]);
        assert!(lines.iter().any(|l| l.starts_with("o/r") && l.contains(" win ") && l.contains("4.6.61 (selected 4.6.62 waits #7 attempt 42)")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("o/r") && l.contains(" lin ") && l.contains("4.6.62") && !l.contains("selected")), "{lines:#?}");
    }

    // Chain: `taskq doctor --hosts` request issue -> service pass -> heartbeat `doctor-answer` (#2194 A2, #2246 A1).
    // Risk: a lagging issue list showed a live host stale, every board waited 300s in turn (14 boards: 70 minutes),
    // and a silent host read `busy or unreachable`. Protection: a listed-stale heartbeat is read fresh before the
    // request; one wait covers every board and stops once all answer; a silent live host names its stuck service.
    #[test]
    fn doctor_hosts_requests_every_board_at_once_and_reads_a_silent_host_unknown() {
        use crate::board::{Board, Fake};
        let (b, c) = (Fake::default(), Fake::default());
        b.advance(10_000);
        c.advance(10_000);
        let beat = |b: &Fake, name: &str, body: &str| b.create(&format!("taskq host {name}"), body, &[crate::service::HOST_LABEL.into()]).unwrap();
        let (win, lin) = (beat(&b, "win", "version 4.6.61\ndoctor 9990 ok"), beat(&b, "lin", "version 4.6.61\ndoctor 9990 ok"));
        let mac = beat(&b, "mac", "version 4.6.61\ndoctor 9990 ok");
        beat(&b, "old", "version 4.6.50");
        let pds = beat(&c, "pds", "version 4.6.61\ndoctor 9990 ok");
        let ttl = 120;
        let read = |b: &Fake| b.issues(None).unwrap().iter().filter(|i| crate::service::is_host(i)).map(|i| crate::snapshot::host(i, b.now().unwrap(), ttl)).collect::<Vec<_>>();
        let mut hosts = read(&b);
        // the list lags: mac reads two hours old there while its heartbeat is fresh
        hosts.iter_mut().find(|h| h.name == "mac").unwrap().updated = 1_000;
        let mut gone = read(&c);
        let now = b.now().unwrap();
        let asked = request(&b, &mut hosts, now, ttl);
        assert!(hosts.iter().find(|h| h.name == "mac").unwrap().updated > 1_000, "a listed-stale heartbeat is read fresh");
        let other = request(&c, &mut gone, now, ttl);
        let id = asked.id.clone().unwrap();
        let mut asks = vec![Ask::new(&b, hosts, asked, now, ttl), Ask::new(&c, gone, other, now, ttl)];
        // win, mac and pds answer on their next pass; lin never answers
        let slept = std::cell::Cell::new(0);
        let answer = |_| {
            slept.set(slept.get() + 1);
            let id = request_of(&b.issues(None).unwrap()).expect("request on the board");
            b.edit(win, &format!("version 4.6.61\ndoctor {now} token-pulls:gap\n{}", answer_line(&id))).unwrap();
            b.edit(mac, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap();
            let id = request_of(&c.issues(None).unwrap()).expect("request on the second board");
            c.edit(pds, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap();
        };
        collect(&mut asks, ttl, 60, 20, &answer);
        assert_eq!((asks[0].asked.waited, asks[1].asked.waited, slept.get()), (60, 60, 3), "one wait for both boards, full for the silent host");
        let requests: Vec<_> = b.issues(None).unwrap().into_iter().filter(|i| i.title == REQUEST_TITLE).collect();
        assert_eq!(requests.len(), 1);
        assert!(!crate::service::is_host(&requests[0]), "an old host's heartbeat parser ignores the request");
        assert_eq!(request_of(&requests), Some(id.clone()));
        let rows: Vec<Value> = asks[0].hosts.iter().map(|h| host_row(h, "o/r", false, now + 60, ttl, Some(&asks[0].asked))).collect();
        let row = |name: &str| rows.iter().find(|r| r["host"] == name).unwrap().clone();
        assert_eq!((row("win")["state"].as_str(), &row("win")["gaps"]), (Some("gap"), &json!(["token-pulls"])), "the answer's fresh result");
        assert_eq!(row("mac")["state"], "ok", "a host the list showed stale answers");
        assert_eq!((row("lin")["state"].as_str(), row("lin")["reason"].as_str()), (Some("unknown"), Some("live heartbeat, no answer to the request within 60s: its service has not run a pass since")));
        assert_eq!(row("old")["reason"], "version 4.6.50 runs no health checks");
        assert_eq!(host_row(&asks[1].hosts[0], "g/p", true, now + 60, ttl, Some(&asks[1].asked))["state"], "ok");
        // the next request edits the same issue and stops waiting once every live host answered
        b.edit(lin, &format!("version 4.6.61\ndoctor {now} ok")).unwrap();
        let mut hosts = read(&b);
        let again = request(&b, &mut hosts, b.now().unwrap(), ttl);
        let mut asks = vec![Ask::new(&b, hosts, again, b.now().unwrap(), ttl)];
        let all = |_| {
            let id = request_of(&b.issues(None).unwrap()).unwrap();
            for n in [win, lin, mac] { b.edit(n, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap(); }
        };
        collect(&mut asks, ttl, 300, 20, &all);
        assert_eq!(asks[0].asked.waited, 20, "no wait once all answered");
        assert_eq!(b.issues(None).unwrap().iter().filter(|i| i.title == REQUEST_TITLE).count(), 1, "one reused request issue");
        assert_ne!(asks[0].asked.id.as_ref().unwrap(), &id, "every run is a new request");
        let mut none = vec![Host { name: "gone".into(), issue: 9999, updated: 1_000, doctor: Some("900 ok".into()), ..Default::default() }];
        assert!(request(&b, &mut none, now, ttl).id.is_err(), "no live host: no request");
    }

    // Chain: § 4 heartbeat lease, execution observation and account lines -> `taskq doctor --hosts` workers (#2194
    // A3). Risk: a held lease read as a live worker and a heartbeat with no account facts read as account ok, so a
    // PM trusted a worker whose runtime had died or logged out. Protection: alive needs the fresh lease and a fresh
    // executor; the account is ok only from an active unlimited account line or fresh runtime output on a live
    // heartbeat; `login off` and limits are named; anything else is unknown with its reason.
    #[test]
    fn doctor_hosts_lists_workers_with_health() {
        use crate::snapshot::{Row, Snapshot};
        let now = 10_000;
        let row = |n, rt: &str, host: &str, heartbeat: Value| Row { n, task: crate::core::Task { state: State::Doing, attempt: Some(n * 10),
            session: Some(format!("{rt}:s{n} {host}")), ..Default::default() }, heartbeat, ..Default::default() };
        let beat = |executor: &str, progress: &str, warning: Option<&str>| json!({"freshness": "fresh", "executor": {"state": executor, "reason": "observed"},
            "progress": {"status": progress}, "warning": warning});
        let idle = Row { n: 9, task: crate::core::Task { state: State::Ready, ..Default::default() }, ..Default::default() };
        let s = Snapshot { source: "board", observed: now, reason: None, board: Value::Null, hosts: vec![
            Host { name: "win".into(), updated: now - 5, logins: vec!["codex".into()], limits: vec!["claude until 2026-11-02T10:00:00Z".into()], ..Default::default() },
            Host { name: "lin".into(), updated: now - 5, accounts: vec!["claude work on active".into(), "codex home on".into()], ..Default::default() },
            Host { name: "fra".into(), updated: now - 5, ..Default::default() },
            Host { name: "old".into(), updated: now - 900, accounts: vec!["claude work on active".into()], ..Default::default() }],
            tasks: vec![row(1, "claude", "win", beat("active", "active", None)),
                row(2, "codex", "win", beat("active", "overdue", Some("execution progress observation missing or stale"))),
                row(3, "claude", "mac", json!({"freshness": "stale", "reason": "owning host heartbeat stale or time unknown"})),
                row(4, "claude", "lin", beat("unknown", "active", Some("current executor unknown; inspect execution health"))),
                row(5, "codex", "lin", beat("active", "quiet", None)),
                row(6, "claude", "fra", beat("active", "quiet", None)),
                row(7, "claude", "fra", beat("waiting", "active", None)),
                row(8, "claude", "old", beat("active", "active", None)), idle] };
        let w = worker_rows(&s, 120);
        assert_eq!(w.len(), 8, "only live attempts");
        let get = |n: u64| w.iter().find(|r| r["task"] == n).unwrap();
        assert_eq!((&get(1)["alive"], &get(1)["stalled"], &get(1)["account"]), (&json!(true), &json!(false), &json!("limit claude until 2026-11-02T10:00:00Z")));
        assert_eq!((&get(2)["stalled"], &get(2)["account"], &get(2)["runtime"]), (&json!(true), &json!("login codex off"), &json!("codex")));
        assert_eq!((&get(3)["alive"], &get(3)["alive_reason"], &get(3)["stalled"], &get(3)["account"]), (&Value::Null,
            &json!("owning host heartbeat stale or time unknown"), &Value::Null, &json!("unknown: host heartbeat missing")), "a stale lease is no live worker");
        assert_eq!((&get(4)["alive"], &get(4)["alive_reason"], &get(4)["account"]), (&Value::Null, &json!("executor unknown: observed"), &json!("ok (account work)")),
            "a fresh lease without a fresh executor is unknown, never alive");
        assert_eq!(get(5)["account"], "unknown: no fresh codex account facts", "an inactive account line is no evidence");
        assert_eq!(get(6)["account"], "unknown: no fresh claude account facts", "no account line and no output: unknown, never ok");
        assert_eq!((&get(7)["alive"], &get(7)["account"]), (&json!(true), &json!("ok (runtime output fresh)")));
        assert_eq!(get(8)["account"], "unknown: host heartbeat 15m old", "a stale heartbeat's account line is no evidence");
        let lines = hosts_lines(&[], &w);
        assert!(lines.iter().any(|l| l.contains("#3") && l.contains("unknown: owning host heartbeat stale")), "{lines:#?}");
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("taskq-health-{tag}-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // Chain: § 6 host setup -> `taskq doctor` claude-trust -> heartbeat -> `--hosts` (#2300 A1, assetq#17). Risk:
    // `doctor --hosts` read all ok while Claude refused a registered root, so the first Claude worker failed there.
    // Protection: an untrusted root is a `claude-trust` gap naming the folder with one fix; a trusted ancestor or no
    // Claude config is ok, an unreadable config unknown; `--hosts` names the gap with one fix.
    #[test]
    fn doctor_flags_a_root_claude_does_not_trust() {
        let home = scratch("trust");
        let root = home.join("work").join("assetq");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(trust_item(&home, &root, true).status, "ok", "no Claude config: Claude is not set up here");
        let config = home.join(crate::runtime::owner::CLAUDE_CONFIG);
        std::fs::write(&config, json!({"projects": {"/elsewhere": {"hasTrustDialogAccepted": true}}}).to_string()).unwrap();
        let gap = trust_item(&home, &root, true);
        let key = crate::runtime::owner::claude_key(&root);
        assert_eq!((gap.status, gap.text.as_str()), ("gap", format!("Claude does not trust {key}; Claude workers cannot start here").as_str()));
        assert!(gap.fix.as_deref().unwrap().contains(&key), "{:?}", gap.fix);
        assert_eq!(line(7, &[gap]), "doctor 7 claude-trust:gap");
        let parent = crate::runtime::owner::claude_key(&home.join("work"));
        std::fs::write(&config, json!({"projects": {parent: {"hasTrustDialogAccepted": true}}}).to_string()).unwrap();
        assert_eq!(trust_item(&home, &root, true).status, "ok", "a trusted parent covers the root");
        std::fs::write(&config, "{").unwrap();
        assert_eq!(trust_item(&home, &root, true).status, "unknown", "an unreadable config is unknown, never ok");
        let now = 10_000;
        let h = Host { name: "win".into(), version: "4.7.0".into(), updated: now - 10, doctor: Some("9900 claude-trust:gap".into()), ..Default::default() };
        let row = host_row(&h, "o/r", false, now, 120, None);
        assert_eq!((row["state"].as_str(), row["gaps"][0].as_str()), (Some("gap"), Some("claude-trust")), "{row}");
        assert!(row["fix"].as_str().unwrap().contains("accept its trust prompt"), "{row}");
        let _ = std::fs::remove_dir_all(&home);
    }

    // Chain: Claude config left on a host -> `taskq doctor` claude-trust -> heartbeat (#2417 A1). Risk: a Linux service
    // host without a claude binary read as a trust gap it could never act on, because a `~/.claude.json` was present.
    // Protection: no binary is ok whatever the config says; with a binary the #2300 readings stay.
    #[test]
    fn doctor_reports_no_trust_gap_without_a_claude_binary() {
        let home = scratch("no-claude");
        let root = home.join("work");
        std::fs::create_dir_all(&root).unwrap();
        let config = home.join(crate::runtime::owner::CLAUDE_CONFIG);
        std::fs::write(&config, json!({"projects": {}}).to_string()).unwrap();
        assert_eq!(trust_item(&home, &root, true).status, "gap", "a host with Claude keeps the gap");
        let item = trust_item(&home, &root, false);
        assert_eq!((item.status, item.fix.as_deref()), ("ok", None), "{item:?}");
        assert_eq!(line(7, &[item]), "doctor 7 ok");
        std::fs::write(&config, "{").unwrap();
        assert_eq!(trust_item(&home, &root, false).status, "ok", "an unreadable config cannot block a host without Claude");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// An install on release 1.2.0 whose launcher is the kept release 1.1.0, with a bare agentq hook: the #2300 gap.
    fn stale_install(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, Vec<std::path::PathBuf>) {
        let base = scratch(tag);
        let agentq = base.join("agentq");
        let exe = agentq_exe();
        for version in ["1.2.0", "1.1.0"] {
            std::fs::create_dir_all(agentq.join("releases").join(version)).unwrap();
            std::fs::write(agentq.join("releases").join(version).join(exe), format!("agentq {version}")).unwrap();
        }
        std::fs::write(agentq.join("current"), "1.2.0").unwrap();
        std::fs::write(agentq.join(exe), "agentq 1.1.0").unwrap();
        let hooks = base.join("settings.json");
        std::fs::write(&hooks, json!({"command": "agentq hook claude pre"}).to_string()).unwrap();
        (base, agentq, vec![hooks])
    }

    fn no_onboard(item: &Item, status: &str) {
        assert_eq!(item.status, status, "{item:?}");
        assert!(!item.fix.as_deref().unwrap_or("").contains("onboard"), "{item:?}");
        assert!(!line(7, std::slice::from_ref(item)).contains(":gap"), "`--hosts` turns a gap into onboard advice: {item:?}");
    }

    // Chain: `agentq disable` -> `data/integration.json` -> `taskq doctor` agentq-hook -> `--hosts` (#2417 A2). Risk: the
    // doctor told a host where AgentQ is off on purpose to run an onboard that undoes the choice or refuses, and AgentQ's
    // refusal then names `agentq enable`. Protection: host disable and a disabled hook client are ok, an unread state is
    // unknown, and none names onboard; an unrelated disabled client keeps the gap with a per-agent onboard, which AgentQ
    // accepts and which leaves that client off.
    #[test]
    fn doctor_advises_only_an_onboard_that_keeps_a_disable() {
        let (base, agentq, files) = stale_install("disabled");
        std::fs::create_dir_all(agentq.join("data")).unwrap();
        let state = agentq.join("data/integration.json");
        assert_eq!(hook_item(&agentq, &files).status, "gap", "no state file: no disable intent");
        std::fs::write(&state, json!({"schema": 1, "host_disabled": true, "profiles": {}}).to_string()).unwrap();
        no_onboard(&hook_item(&agentq, &files), "ok");
        std::fs::write(&state, json!({"schema": 1, "host_disabled": false, "profiles": {"claude": {}}}).to_string()).unwrap();
        no_onboard(&hook_item(&agentq, &files), "ok");
        std::fs::write(&state, "{").unwrap();
        no_onboard(&hook_item(&agentq, &files), "unknown");
        std::fs::write(&state, json!({"schema": 1, "host_disabled": false, "profiles": {"hermes": {}}}).to_string()).unwrap();
        let unrelated = hook_item(&agentq, &files);
        assert_eq!(unrelated.status, "gap", "{unrelated:?}");
        let fix = unrelated.fix.as_deref().unwrap();
        assert!(fix.starts_with("run `agentq onboard --yes claude`:"), "{fix}");
        assert!(!fix.contains("agentq enable"), "{fix}");
        std::fs::write(agentq.join(agentq_exe()), "agentq 1.2.0").unwrap();
        std::fs::remove_file(agentq.join("releases/1.2.0").join(agentq_exe())).unwrap();
        let unread = hook_item(&agentq, &files);
        assert_eq!(unread.status, "unknown", "{unread:?}");
        assert!(!unread.fix.as_deref().unwrap().contains("onboard"), "bare onboard refuses while a client is disabled: {unread:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    // Chain: dev launcher -> `taskq doctor` agentq-hook, agentq-data -> `--hosts` (#2417 note 6653818228). Risk: a hook
    // running a stale dev launcher blocks work like any other old hook, so reading it as ok or unknown hides a true
    // finding. Protection: bytes that differ from the release are a gap naming the binary and each stale client once, in
    // or outside the install; a checkout binary equal to the release is ok.
    #[test]
    fn doctor_keeps_the_gap_on_a_stale_dev_launcher() {
        let (base, agentq, mut files) = stale_install("dev");
        let dev = base.join("checkout").join(agentq_exe());
        std::fs::create_dir_all(dev.parent().unwrap()).unwrap();
        std::fs::write(&dev, "local build").unwrap();
        let command = |client: &str| json!({"command": format!("\"{}\" hook {client} pre", dev.display())}).to_string();
        std::fs::write(&files[0], command("claude")).unwrap();
        let outside = hook_item(&agentq, &files);
        assert_eq!(outside.status, "gap", "{outside:?}");
        assert!(outside.text.contains(&dev.display().to_string()), "{outside:?}");
        let codex = base.join("hooks.json");
        std::fs::write(&codex, command("codex")).unwrap();
        files.push(codex);
        let second = base.join("other.json");
        std::fs::write(&second, command("codex")).unwrap();
        files.push(second);
        assert!(hook_item(&agentq, &files).fix.as_deref().unwrap().starts_with("run `agentq onboard --yes claude codex`:"), "each stale client once");
        files.truncate(1);
        std::fs::write(&dev, "agentq 1.2.0").unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "ok", "a checkout binary equal to the release runs current hook code");
        std::fs::write(&files[0], json!({"command": "agentq hook claude pre"}).to_string()).unwrap();
        std::fs::write(agentq.join(agentq_exe()), "local build").unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "gap", "an install launcher equal to no kept release");
        assert_eq!(data_item(&agentq).status, "gap", "a missing data folder blocks every hook");
        let _ = std::fs::remove_dir_all(&base);
    }

    // Chain: agentq onboarding -> client hook files -> `taskq doctor` agentq-hook -> `--hosts` (#2300 A1, agentq#190).
    // Risk: a client hook ran an installed agentq launcher older than the current release, so its old hook code
    // refused worker calls while doctor read ok. Protection: a hook whose binary differs from the current release is
    // a gap naming the file and release with one fix; a matching launcher, no hook or no agentq install is ok.
    #[test]
    fn doctor_flags_an_agentq_hook_older_than_the_release() {
        let base = scratch("hook");
        let agentq = base.join("agentq");
        let exe = agentq_exe();
        std::fs::create_dir_all(agentq.join("releases/1.2.0")).unwrap();
        std::fs::create_dir_all(agentq.join("releases/1.1.0")).unwrap();
        std::fs::write(agentq.join("current"), "1.2.0\n").unwrap();
        std::fs::write(agentq.join("releases/1.2.0").join(exe), b"agentq 1.2.0").unwrap();
        std::fs::write(agentq.join("releases/1.1.0").join(exe), b"agentq 1.1.0").unwrap();
        std::fs::write(agentq.join(exe), b"agentq 1.1.0").unwrap();
        let claude = base.join("settings.json");
        let codex = base.join("hooks.json");
        let files = vec![claude.clone(), codex.clone(), base.join("absent.json")];
        assert_eq!(hook_item(&base.join("none"), &files).status, "ok", "no agentq install");
        assert_eq!(hook_item(&agentq, &files).status, "ok", "no hook file");
        std::fs::write(&claude, json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "other hook x"}, {"type": "command", "command": "agentq hook claude pre"}]}]}}).to_string()).unwrap();
        let gap = hook_item(&agentq, &files);
        assert_eq!(gap.status, "gap", "{gap:?}");
        assert!(gap.text.starts_with("agentq hook in settings.json runs ") && gap.text.ends_with("not the current release 1.2.0"), "{}", gap.text);
        assert!(gap.fix.as_deref().unwrap().contains("agentq onboard --yes"), "{:?}", gap.fix);
        std::fs::write(agentq.join(exe), b"agentq 1.2.0").unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "ok", "the launcher is the current release");
        let old = agentq.join("releases/1.1.0").join(exe);
        std::fs::write(&codex, json!({"agentq": [{"command": format!("\"{}\" hook antigravity PreToolUse", old.display())}]}).to_string()).unwrap();
        assert!(hook_item(&agentq, &files).text.contains("hooks.json"), "a quoted path to an old release is a gap");
        std::fs::write(&codex, "{").unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "unknown", "an unreadable hook file is unknown, never ok");
        assert_eq!(hook_binary("'/opt/a b/agentq' hook codex pre").as_deref(), Some("/opt/a b/agentq"));
        assert_eq!(hook_binary("env X=1 agentq.exe hook claude pre").as_deref(), Some("agentq.exe"));
        assert_eq!(hook_binary("notagentq hook claude pre"), None);
        let now = 10_000;
        let h = Host { name: "mac".into(), version: "4.7.0".into(), updated: now - 10, doctor: Some("9900 agentq-hook:gap".into()), ..Default::default() };
        let row = host_row(&h, "o/r", false, now, 120, None);
        assert_eq!((row["state"].as_str(), row["fix"].as_str()), (Some("gap"), Some("run `taskq doctor` on mac and the agentq command it names: its client hooks differ from the current agentq release")), "{row}");
        let _ = std::fs::remove_dir_all(&base);
    }

    // R12 / #2300 A1: failed filesystem observations must not claim an old hook or an absent install.
    // Keep the stale-byte regression above; these cases protect distinct metadata and binary-read failures.
    #[test]
    fn doctor_hook_read_failures_are_unknown() {
        let base = scratch("hook-unreadable");
        let agentq = base.join("agentq");
        let exe = agentq_exe();
        std::fs::create_dir_all(agentq.join("releases/1.2.0")).unwrap();
        std::fs::write(agentq.join("current"), "1.2.0").unwrap();
        std::fs::write(agentq.join("releases/1.2.0").join(exe), b"current").unwrap();
        let hook = base.join("hooks.json");
        std::fs::write(&hook, json!({"command": "agentq hook antigravity PreToolUse"}).to_string()).unwrap();
        let files = vec![hook];
        assert_eq!(hook_item(&agentq, &files).status, "unknown", "missing hook binary is unread, not proved old");
        std::fs::create_dir(agentq.join(exe)).unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "unknown", "a directory cannot be read as a hook binary");
        let blocked = base.join("file");
        std::fs::write(&blocked, "not a directory").unwrap();
        assert_eq!(hook_item(&blocked, &files).status, "unknown", "an install path that is a file is not an absent install");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(hook_item(&blocked.join("agentq"), &files).status, "unknown", "install metadata failure is not absence");
            let parent = base.join("denied");
            std::fs::create_dir_all(parent.join("agentq")).unwrap();
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();
            let denied = hook_item(&parent.join("agentq"), &files);
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(denied.status, "unknown", "inaccessible install root must not read as not installed");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    // Chain: agentq install -> `taskq doctor` agentq-data -> `--hosts` (#2300 A1, agentq#190). Risk: agentq could not
    // write its `data/lifecycle.lock` (EPERM), so every agentq hook failed while doctor read ok. Protection: a refused
    // write in the data folder or on its lock is a gap with one fix; the probe leaves no file and the lock unchanged.
    #[test]
    fn doctor_flags_an_unwritable_agentq_data_folder() {
        let base = scratch("data");
        let agentq = base.join("agentq");
        assert_eq!(data_item(&agentq).status, "ok", "no agentq install");
        std::fs::create_dir_all(&agentq).unwrap();
        assert_eq!(data_item(&agentq).status, "gap", "a missing data folder");
        let data = agentq.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let lock = data.join("lifecycle.lock");
        std::fs::write(&lock, "held").unwrap();
        assert_eq!(data_item(&agentq).status, "ok");
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 1, "the probe leaves no file");
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "held", "the lock is unchanged");
        let mut p = std::fs::metadata(&lock).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&lock, p.clone()).unwrap();
        let gap = data_item(&agentq);
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        std::fs::set_permissions(&lock, p).unwrap();
        assert_eq!(gap.status, "gap", "{gap:?}");
        assert!(gap.text.contains("not writable"), "{}", gap.text);
        assert_eq!(gap.fix.as_deref(), Some("give this OS user write access to the agentq data folder"));
        let now = 10_000;
        let h = Host { name: "mac".into(), version: "4.7.0".into(), updated: now - 10, doctor: Some("9900 agentq-data:gap agentq-hook:unknown".into()), ..Default::default() };
        let row = host_row(&h, "o/r", false, now, 120, None);
        assert_eq!(row["gaps"], json!(["agentq-data", "agentq-hook (unknown)"]), "{row}");
        assert!(row["fix"].as_str().unwrap().starts_with("on mac: give its OS user write access to the agentq data folder"), "{row}");
        let _ = std::fs::remove_dir_all(&base);
    }
