    use super::*;

    // Chain: #1084 A1 CLI process -> source record -> `taskq profile` hour table. Risk: a source is merged with
    // another, a request outside the hour is counted, a missing or corrupt record reads as zero, or free text
    // from arguments reaches the log.
    // Chain: #1985 A4 `taskq status` quota line. Risk: the PM reads no used/remaining of the board user.
    #[test]
    fn quota_line_names_used_and_left() {
        let mut b = Budget::default();
        assert_eq!(quota(&b), "Board quota: unknown (no core budget headers)");
        let reply = |resource: &'static str, left: &'static str| move |h: &str| match h {
            "x-ratelimit-resource" if !resource.is_empty() => Some(resource.to_string()),
            "x-ratelimit-remaining" | "ratelimit-remaining" => Some(left.to_string()),
            "x-ratelimit-limit" | "ratelimit-limit" => Some("5000".to_string()),
            "x-ratelimit-reset" | "ratelimit-reset" => Some("0".to_string()),
            _ => None,
        };
        let now = Instant::now();
        // #2084 A2: a graphql reply alone leaves the quota unknown, and a lower graphql budget never replaces core
        b.observe(200, false, reply("graphql", "100"), 0, now);
        assert_eq!(quota(&b), "Board quota: unknown (no core budget headers)");
        b.observe(200, false, reply("core", "4000"), 0, now);
        b.observe(200, false, reply("graphql", "50"), 0, now);
        assert_eq!(b.resource.as_deref(), Some("graphql"), "the pause budget stays the lowest one");
        assert_eq!(quota(&b), "Board quota: 1000/5000 core used, 4000 left until 1970-01-01T00:00:00Z");
        let mut gitlab = Budget::default();
        gitlab.observe(200, false, reply("", "4500"), 0, now);
        assert_eq!(quota(&gitlab), "Board quota: 500/5000 used, 4500 left until 1970-01-01T00:00:00Z");
    }

    // Chain: #2084 A1 heartbeat requests per API host. Defect: a host with GitHub and GitLab boards sent one sum, so
    // the uncounted gap of the GitHub quota was unknown. Risk: a path, user or token reaches the heartbeat, the free
    // budget read or a 304 counts as billed, or the line older readers parse changes.
    #[test]
    fn heartbeat_splits_billed_requests_by_api_host() {
        assert_eq!(api_host("https://api.github.com"), "api.github.com");
        assert_eq!(api_host("https://GitLab.com/api/v4"), "gitlab.com");
        assert_eq!(api_host("https://user@ghe.example:8443/api/v3?x"), "ghe.example", "no user or port");
        assert_eq!(api_host("https://bad host/x"), "*");
        assert_eq!(api_host(""), "*");
        let hour = [("GET issues".to_string(), 5), ("GET issues 304".to_string(), 40), (FREE.to_string(), 2)].into();
        assert_eq!(billed(&hour), 5);
        assert_eq!(heartbeat(&hour, None), ["requests 5/h"]);
        let hosts = [("api.github.com".to_string(), 3), ("gitlab.com".to_string(), 2)].into();
        assert_eq!(heartbeat(&hour, Some(&hosts)), ["requests 5/h", "requests api.github.com 3/h", "requests gitlab.com 2/h"]);
    }

    #[test]
    fn requests_by_source_for_the_last_hour() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(source(&args(&["result", "1084"]), Some("worker")), "worker result");
        assert_eq!(source(&args(&["status"]), Some("operator")), "cli status");
        assert_eq!(source(&args(&["service", "status"]), None), "cli service status");
        assert_eq!(source(&args(&["service", "run"]), None), SERVICE, "the service keeps one source");
        assert_eq!(source(&args(&["C:\\secret path"]), None), "cli *");
        assert_eq!(source(&[], None), "cli *");
        assert_eq!(source(&args(&["--json", "status"]), None), "cli status", "a flag before the command");
        assert_eq!(source(&args(&["--json", "service", "run"]), None), SERVICE, "service run with a flag first");
        assert_eq!(source(&args(&["service", "--json", "run"]), None), SERVICE, "service run with a flag between");
        let line = |at: u64, src: &str, ops: &[(&str, u64)]| format!("{RECORD}{}\n", serde_json::to_string(&Usage { at_ms: at,
            source: src.into(), ops: ops.iter().map(|(k, v)| (k.to_string(), *v)).collect() }).unwrap());
        let now = 10_000_000;
        assert!(sources("", now).contains("unknown (no request records"));
        let text = [line(now - 3_700_000, "cli status", &[("GET issues", 50)]), "taskq-profile {}\nother line\n".into(),
            line(now - 60_000, "worker result", &[("POST issues/N/comments", 1), ("GET issues/N/comments", 2)]),
            line(now - 50_000, "worker result", &[("GET issues/N/comments", 2)]),
            line(now - 40_000, "service", &[("POST graphql", 60), ("PATCH issues/N", 30)])].concat();
        let report = sources(&text, now);
        assert!(report.starts_with("Board requests by source: 95 in 6400000..10000000 Unix ms (full hour)"), "{report}");
        assert!(report.contains("| service | 1 | 90 | POST graphql 60, PATCH issues/N 30 |\n| worker result | 2 | 5 | GET issues/N/comments 4, POST issues/N/comments 1 |"), "{report}");
        assert!(!report.contains("cli status"), "a record before the hour is not counted");
        assert!(sources(&text[line(now - 3_700_000, "cli status", &[("GET issues", 50)]).len()..], now).contains("(partial: records start 9940000"));
        assert!(sources(&format!("{text}{RECORD}{{\"at_ms\":1}}\n"), now).starts_with("Board requests by source: unknown (corrupt"));
        let dir = std::env::temp_dir().join(format!("taskq-sources-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        std::fs::write(dir.join("taskq.log"), line(wall - 60_000, "worker result", &[("GET issues/N/comments", 5)]).repeat(2)).unwrap();
        drop(crate::profile::log_lock(&dir.join("taskq.log")).unwrap());
        crate::telemetry::read(&dir,wall).unwrap();
        assert!(crate::profile::report(&dir).contains("| worker result | 2 | 10 |"), "`taskq profile` prints the table");
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: §4 Board request records -> `taskq profile` hour table. Defect #2367: the table assumed one service record
    // an hour. Risk kept: finer records sum wrongly per source and operation, or one older than the hour counts.
    #[test]
    fn hour_table_sums_the_finer_service_records() {
        let line = |at: u64, src: &str, ops: &[(&str, u64)]| format!("{RECORD}{}\n", serde_json::to_string(&Usage { at_ms: at,
            source: src.into(), ops: ops.iter().map(|(k, v)| (k.to_string(), *v)).collect() }).unwrap());
        let now = 10_000_000;
        // Sixteen records 4 minutes apart: the oldest lies exactly on the hour's start, outside it.
        let mut text: String = (0..16).map(|i| line(now - i * RECORD_SECONDS * 1000, SERVICE, &[("GET issues/N", 2), ("GET issues/N 304", 1)])).collect();
        text += &line(now - 1000, "cli status", &[("GET issues", 1)]);
        let report = sources(&text, now);
        assert!(report.starts_with("Board requests by source: 46 in 6400000..10000000 Unix ms (full hour)"), "{report}");
        assert!(report.contains("| service | 15 | 45 | GET issues/N 30, GET issues/N 304 15 |\n| cli status | 1 | 1 | GET issues 1 |"), "{report}");
        assert!(report.contains("a service every 4 min and at a clean end, so a killed service loses at most its last 5 min"), "{report}");
    }

    /// A child of this test binary with its own counts, over the telemetry store of `install`.
    fn records_child(install: &std::path::Path, generation: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "requests::tests::service_records_child", "--ignored", "--nocapture"])
            .env("TASKQ_REQUESTS_CHILD", install).env("TASKQ_REQUESTS_GENERATION", generation)
            .env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_TASK").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT")
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap()
    }

    // Chain: §4 Board request records -> flush -> telemetry -> `taskq profile`. Defect #2367: the service lost its counts
    // at a restart. Risk kept: a count is recorded twice or lost across records, a 304 answered around a record, a
    // refused submit, or two generations; recording changes the hourly counts or sends a request.
    #[test]
    fn clean_ends_record_each_count_once() {
        let install = std::env::temp_dir().join(format!("taskq-requests-clean-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&install).unwrap();
        for generation in ["first", "second"] {
            let output = records_child(&install, generation).wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(75), "{generation}: {}", String::from_utf8_lossy(&output.stdout));
        }
        let report = crate::profile::report(&install);
        std::fs::remove_dir_all(&install).unwrap();
        assert!(report.contains("| service | 5 | 9 | GET issues/N 4, PATCH issues/N 2, POST issues/N/comments 2 |"), "{report}");
    }

    // Chain: §4 Board request records -> Stop path: flush and drain before its controller ends the process. Risk kept:
    // counts submitted just before a process end are not durable when the controller terminates it.
    #[test]
    fn counts_drained_before_a_controller_kill_survive_it() {
        use std::io::BufRead;
        let install = std::env::temp_dir().join(format!("taskq-requests-stop-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&install).unwrap();
        let mut child = records_child(&install, "stop");
        let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        assert!(lines.any(|line| line.is_ok_and(|line| line == "drained")), "the child never drained");
        child.kill().unwrap();child.wait().unwrap();
        let report = crate::profile::report(&install);
        std::fs::remove_dir_all(&install).unwrap();
        assert!(report.contains("| service | 1 | 2 | PATCH issues/N 2 |"), "{report}");
    }

    #[test]
    #[ignore = "owned subprocess with its own board request counts"]
    fn service_records_child() {
        let Some(install) = std::env::var_os("TASKQ_REQUESTS_CHILD").map(std::path::PathBuf::from) else { return };
        let host = "api.github.com";
        let unrecorded = || UNRECORDED.lock().unwrap().clone();
        match std::env::var("TASKQ_REQUESTS_GENERATION").unwrap().as_str() {
            "first" => {
                tally("GET issues/N", host);tally("GET issues/N", host);tally("GET issues/N", host);
                flush(SERVICE);
                assert_eq!(unrecorded().get("GET issues/N"), Some(&3), "no collector admits the record, so the counts stay");
                let _runtime = crate::telemetry::install_capture(&install).unwrap();
                let (ops, hosts) = (totals(), host_totals());
                flush(SERVICE);
                assert!(unrecorded().is_empty());
                assert_eq!((totals(), host_totals()), (ops, hosts), "a record is no board request and keeps the hourly counts");
                tally("GET issues/N", host);untally("GET issues/N", host);tally("POST issues/N/comments", host);
                flush(SERVICE);
                flush(SERVICE);
                // The 304 of a request already recorded: it stays counted once, never a second time as a 304.
                tally("GET issues/N", host);
                flush(SERVICE);
                untally("GET issues/N", host);
                assert!(unrecorded().is_empty());
                tally("PATCH issues/N", host);tally("PATCH issues/N", host);
                crate::platform::service_exit(75, "restart to 9.9.9");
            }
            "second" => {
                let _runtime = crate::telemetry::install_capture(&install).unwrap();
                tally("POST issues/N/comments", host);
                crate::platform::service_exit(75, "restart to 9.9.9");
            }
            _ => {
                let _runtime = crate::telemetry::install_capture(&install).unwrap();
                tally("PATCH issues/N", host);tally("PATCH issues/N", host);
                flush(SERVICE);
                crate::platform::drain_telemetry();
                println!("\ndrained");
                std::thread::sleep(Duration::from_secs(120));
            }
        }
    }

    // Chain: § 4 board budget. Risk: an exhausted budget keeps sending requests, or a key leaks a branch or SHA.
    #[test]
    fn exhausted_budget_pauses_until_reset_and_resumes() {
        let t0 = Instant::now();
        let mut b = Budget::default();
        let h = |pairs: &'static [(&'static str, &'static str)]| move |n: &str| pairs.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string());
        assert_eq!(b.observe(200, false, h(&[("x-ratelimit-remaining", "12"), ("x-ratelimit-limit", "5000")]), 1000, t0), None);
        assert_eq!((b.remaining, b.limit, b.paused(t0)), (Some(12), Some(5000), None));
        // low budget paces: the time to the reset spread over the requests left, capped; no reset known, no pacing
        assert_eq!(b.pace(1000), None);
        b.observe(200, false, h(&[("x-ratelimit-remaining", "1000"), ("x-ratelimit-limit", "5000"), ("x-ratelimit-reset", "3000")]), 1000, t0);
        assert_eq!(b.pace(1000), Some(Duration::from_secs(2)));
        b.observe(200, false, h(&[("x-ratelimit-remaining", "10"), ("x-ratelimit-limit", "5000"), ("x-ratelimit-reset", "3000")]), 1000, t0);
        assert_eq!(b.pace(1000), Some(Duration::from_secs(5)));
        b.observe(200, false, h(&[("x-ratelimit-remaining", "1001"), ("x-ratelimit-limit", "5000"), ("x-ratelimit-reset", "3000")]), 1000, t0);
        assert_eq!(b.pace(1000), None, "above 20 % no pacing");
        b.observe(200, false, h(&[("x-ratelimit-remaining", "12"), ("x-ratelimit-limit", "5000")]), 1000, t0);
        // a 403 without budget headers is an ordinary refusal
        assert_eq!(b.observe(403, false, h(&[]), 1000, t0), None);
        // primary limit: wait for the reset header against board time, not the local clock
        assert_eq!(b.observe(403, false, h(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1840")]), 1000, t0), Some(1840));
        assert_eq!(b.paused(t0 + Duration::from_secs(839)), Some(1840));
        assert_eq!(b.paused(t0 + Duration::from_secs(840)), None, "resumes at the reset without a restart");
        // secondary limit: retry-after wins; a far reset is capped at one hour
        assert_eq!(b.observe(429, false, h(&[("retry-after", "30"), ("x-ratelimit-remaining", "0")]), 1000, t0), Some(1030));
        assert_eq!(b.observe(403, false, h(&[("ratelimit-remaining", "0"), ("ratelimit-reset", "999999")]), 1000, t0), Some(4600));
        assert_eq!(op("PATCH", "/issues/77"), "PATCH issues/N");
        assert_eq!(op("GET", "/issues/5/comments?per_page=100"), "GET issues/N/comments");
        assert_eq!(op("GET", "/git/ref/heads/taskq-1009"), "GET git/ref/heads");
        assert_eq!(op("GET", &format!("/commits/{}/check-runs", "a1".repeat(20))), "GET commits/*/check-runs");
        let mut later = BTreeMap::from([("GET issues".to_string(), 7), ("PATCH issues/N".to_string(), 60)]);
        let hour = diff(&later, &BTreeMap::from([("GET issues".to_string(), 7)]));
        assert_eq!(summary(&hour, ""), "60; PATCH issues/N 60");
        later.extend((0..9).map(|i| (format!("GET x{i}"), 1)));
        assert!(summary(&later, "").ends_with(", GET x5 1, other 3"));
    }

    // Chain: § 4 board budget to `service status` and R6. Risk: missing accounting reads as zero requests.
    #[test]
    fn request_lines_keep_unknown_and_name_the_pause() {
        assert_eq!(lines(&json!({})), ["Board requests: unknown (no request accounting in status)"]);
        let s = json!({"host": "mac", "requests": {"total": 3, "last_hour": null, "paused_until": "2026-10-14T04:41:00Z"}});
        assert_eq!(lines(&s), ["Board requests: unknown (no full hour yet)", "Rate limit: board requests paused until 2026-10-14T04:41:00Z (mac)"]);
        let s = json!({"requests": {"last_hour": {"PATCH issues/N": 60}, "paused_until": null}});
        assert_eq!(lines(&s), ["Board requests: 60/h; PATCH issues/N 60"]);
    }

    // Chain: § 4 budget alarm to `service status` and R6. Risk: a draining budget stays silent, or names no consumer.
    #[test]
    fn low_budget_alarm_names_the_top_consumer() {
        let h = |r: &'static str| move |n: &str| match n { "x-ratelimit-remaining" => Some(r.to_string()), "x-ratelimit-limit" => Some("5000".into()),
            "x-ratelimit-reset" => Some("1791964800".into()), _ => None };
        let mut b = Budget::default();
        b.observe(200, false, h("1001"), 0, Instant::now());
        assert!(!b.low(), "above 20 % is quiet");
        b.observe(200, false, h("1000"), 0, Instant::now());
        assert!(b.low());
        let start = BTreeMap::from([("PATCH issues/N".to_string(), 50)]);
        b.ops = BTreeMap::from([("PATCH issues/N".to_string(), 80), ("GET issues".to_string(), 20)]);
        assert_eq!(b.top(&start), Some(("PATCH issues/N".into(), 30)));
        let s = json!({"host": "mac", "requests": b.json(0, None, &start)});
        assert_eq!(warning(&s).unwrap(), "Board budget low: 1000/5000 until 2026-10-14T08:00:00Z (mac); top here: PATCH issues/N 30");
    }

    // Chain: § 4 per-resource budget. Risk: a drained GraphQL bucket hides behind a full REST one, or the reverse.
    #[test]
    fn budget_follows_the_lowest_resource() {
        let h = |res: &'static str, r: &'static str| move |n: &str| match n { "x-ratelimit-resource" => Some(res.to_string()),
            "x-ratelimit-remaining" => Some(r.to_string()), "x-ratelimit-limit" => Some("5000".into()), _ => None };
        let mut b = Budget::default();
        b.observe(200, false, h("core", "900"), 0, Instant::now());
        b.observe(200, false, h("graphql", "4000"), 0, Instant::now());
        assert_eq!((b.resource.as_deref(), b.remaining), (Some("core"), Some(900)), "a full bucket never hides a low one");
        b.observe(200, false, h("graphql", "500"), 0, Instant::now());
        b.observe(200, false, h("core", "4990"), 0, Instant::now());
        assert_eq!((b.resource.as_deref(), b.remaining, b.low()), (Some("graphql"), Some(500), true));
        let s = json!({"host": "mac", "requests": b.json(0, None, &BTreeMap::new())});
        assert_eq!(warning(&s).unwrap(), "Board budget low: 500/5000 graphql until unknown (mac); top here: none");
    }
