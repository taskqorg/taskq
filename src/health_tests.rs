use super::*;
use crate::snapshot::Host;

// Chain: § 6 host setup -> `taskq doctor` token checks (#2194 A1, A4). Risk: a fine-grained token without
// Contents: Read let the board work while publication and reports failed with raw 403 errors and no named cause.
// Protection: a 403 on the contents read is a `token-contents` gap naming the permission, the status and one fix.
#[test]
fn doctor_names_a_token_without_contents_read() {
    let items = token_items(
        "o/r",
        false,
        &[
            ("Issues", Probe::Ok),
            ("Contents", Probe::Gap("HTTP 403".into())),
            ("Pull requests", Probe::Ok),
        ],
    );
    let c = items.iter().find(|i| i.id == "token-contents").unwrap();
    assert_eq!(
        (c.status, c.text.as_str()),
        ("gap", "token lacks Contents: Read on o/r (HTTP 403)")
    );
    assert!(
        c.fix.as_deref().unwrap().contains("Contents: Read"),
        "{:?}",
        c.fix
    );
    assert!(
        items
            .iter()
            .filter(|i| i.id != "token-contents")
            .all(|i| i.status == "ok")
    );
    assert_eq!(line(7, &items), "doctor 7 token-contents:gap");
    assert!(
        token_items("o/r", false, &[])
            .iter()
            .all(|i| i.status == "unknown"),
        "an unsupported board is unknown, never ok"
    );
}

// Chain: § 6 GitLab board token -> `taskq doctor` token checks (#2246 A2). Risk: GitLab boards had no token
// probe, so every token cell read unknown and a read-only token passed setup unnoticed. Protection: the token's own
// scopes and the project role decide each permission; a missing scope or a low role is a gap with a GitLab fix,
// an unread fact stays unknown.
#[test]
fn doctor_checks_gitlab_token_scopes_and_role() {
    let dev =
        Ok(json!({"permissions": {"project_access": {"access_level": 30}, "group_access": null}}));
    let full = gitlab_access(
        &Ok(json!({"active": true, "revoked": false, "scopes": ["api"]})),
        &dev,
    );
    assert!(full.iter().all(|(_, p)| *p == Probe::Ok), "{full:?}");
    let read = gitlab_access(
        &Ok(json!({"active": true, "scopes": ["read_api"]})),
        &Ok(json!({"permissions": {"group_access": {"access_level": 20}}})),
    );
    assert_eq!(
        read,
        vec![
            ("Issues", Probe::Gap("scopes read_api lack api".into())),
            ("Contents", Probe::Ok),
            (
                "Pull requests",
                Probe::Gap("scopes read_api lack api".into())
            )
        ]
    );
    let low = gitlab_access(
        &Ok(json!({"active": true, "scopes": ["api"]})),
        &Ok(json!({"permissions": {"project_access": {"access_level": 20}}})),
    );
    assert_eq!(low[2].1, Probe::Gap("project role 20 below 30".into()));
    assert!(matches!(
        gitlab_access(&Ok(json!({"active": false, "scopes": ["api"]})), &dev)[0].1,
        Probe::Gap(_)
    ));
    assert!(
        gitlab_access(&Err(crate::board::Error::http(401, "HTTP 401")), &dev)
            .iter()
            .all(|(_, p)| matches!(p, Probe::Unknown(_))),
        "unread scopes: unknown, never ok"
    );
    let items = token_items("g/p", true, &read);
    assert_eq!(
        items[0].fix.as_deref(),
        Some("owner: give the GitLab board token scope api and role Reporter or higher on g/p")
    );
    assert!(
        !items
            .iter()
            .filter_map(|i| i.fix.as_deref())
            .any(|f| f.contains("fine-grained")),
        "no GitHub fix on GitLab"
    );
}

// Chain: § 8 update -> `taskq doctor` version check (#2194 A1, A4). Risk: a host stayed on an old release with no
// sign in doctor. Protection: behind the latest release is a gap with `taskq update`; an unread release is unknown.
// #2246: a stale service status names restart, never `service install` for a service that exists.
#[test]
fn doctor_flags_an_outdated_version() {
    let v = version_item("4.6.58", &Ok("4.6.60".into()));
    assert_eq!(
        (v.status, v.text.as_str(), v.fix.as_deref()),
        (
            "gap",
            "installed 4.6.58 is behind the latest release 4.6.60",
            Some("run `taskq update`")
        )
    );
    assert_eq!(version_item("4.6.60", &Ok("4.6.60".into())).status, "ok");
    assert_eq!(
        version_item("4.7.0-dev.1", &Ok("4.6.60".into())).status,
        "ok",
        "a newer dev build is not behind"
    );
    assert_eq!(
        version_item("4.6.58", &Err("no network".into())).status,
        "unknown"
    );
    let r = reports_item(
        &json!({"complete": false, "reason": "Report collection is stale; waiting for the service"}),
        false,
    );
    assert_eq!(
        (r.status, r.text.as_str()),
        (
            "gap",
            "report collection incomplete: Report collection is stale; waiting for the service"
        )
    );
    assert_eq!(reports_item(&json!({"complete": true}), false).status, "ok");
    let e = error_item(
        Ok(Some(
            json!({"error": "board unavailable: HTTP 502\ndetail"}),
        )),
        Some(5),
        120,
    );
    assert_eq!(
        (e.status, e.text.as_str()),
        (
            "gap",
            "last service pass failed: board unavailable: HTTP 502"
        )
    );
    assert_eq!(
        error_item(Ok(Some(json!({"error": null}))), Some(5), 120).status,
        "ok"
    );
    let stale = error_item(Ok(Some(json!({"error": null}))), Some(500), 120);
    assert_eq!(stale.status, "unknown", "a stale status is unknown");
    assert!(
        !stale.fix.unwrap().contains("install"),
        "an existing service is restarted, not installed"
    );
    assert_eq!(error_item(Ok(None), None, 120).status, "unknown");
}

// Chain: reports collection (#2195) -> `taskq doctor` reports check -> heartbeat `doctor-reports` -> `--hosts`
// (#2194 A1, #2246 A2). Risk: decided historical reports whose source went 404 kept a `reports` gap on every host
// with no cause, and an undecided unreadable report read ok. Protection: only undecided report rows count; the
// gap and the heartbeat name the cause and the reports; the `--hosts` fix follows the cause.
#[test]
fn doctor_reports_name_missing_reports_and_skip_decided_ones() {
    let ok = reports_item(
        &json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "task", "n": 5, "reason": "No accepted hand-in recorded; result details unknown"},
                {"kind": "report", "n": 6, "visible": false, "missing": true, "status": 404, "reason": "#6 source: HTTP 404"}]}),
        false,
    );
    assert_eq!(
        ok.status, "ok",
        "a decided report with a gone source needs nothing"
    );
    let rows = json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "report", "n": 7, "visible": true, "status": 403, "reason": "#7 source: GET /contents/x: HTTP 403 Resource not accessible"},
                {"kind": "report", "n": 8, "missing": true, "status": 404, "reason": "#8 source: HTTP 404"}]});
    assert_eq!(reports_detail(&rows).as_deref(), Some("denied #7 #8"));
    let bad = reports_item(&rows, false);
    assert_eq!(
        (bad.status, bad.text.as_str()),
        (
            "gap",
            "2 undecided report(s) unreadable (denied: #7 #8), first: #7 source: GET /contents/x: HTTP 403 Resource not accessible"
        )
    );
    assert_eq!(
        bad.fix.as_deref(),
        Some(
            "reports #7 #8 unreadable (HTTP 401/403): owner: give the board token of this host Contents: Read"
        )
    );
    let many: Vec<Value> = (1..=7).map(|n| json!({"kind": "report", "n": n, "missing": true, "status": 404, "reason": "source: HTTP 404"})).collect();
    let gone = json!({"complete": true, "reports": many});
    assert_eq!(
        reports_detail(&gone).as_deref(),
        Some("missing #1 #2 #3 #4 #5 +2")
    );
    assert_eq!(
        reports_detail(&json!({"complete": false, "reason": "Reports not collected yet"}))
            .as_deref(),
        Some("uncollected")
    );
    assert_eq!(
        fix(
            "reports",
            "gap",
            "o/r",
            "win",
            false,
            Some("missing #1 #2 +2")
        ),
        "reports #1 #2 +2 have no source file at their result SHA: on win run `taskq results 1 --action followup` or `--action reopen`"
    );
    assert_eq!(
        fix("reports", "gap", "o/r", "win", false, Some("evil text")),
        "run `taskq doctor` on win: its version names no report cause",
        "only known causes"
    );
}

// Chain: § 4 step 3 heartbeat `doctor` line -> `taskq doctor --hosts` (#2194 A2, A4). Risk: a dead or old host
// read as healthy, a hand-written heartbeat injected text, or (#2246 A1, A2) a heartbeat updated after the reader's
// clock read `<1m old; host unreachable` with `service install`. Protection: a heartbeat newer than `now` is live; a
// stale one names the silent service and restart; an old host reads unknown with its version; unknown ids refused.
#[test]
fn doctor_hosts_reads_stale_and_old_hosts_unknown_and_fresh_ones_live() {
    let host = |name: &str, updated, version: &str, doctor: Option<&str>| Host {
        name: name.into(),
        version: version.into(),
        updated,
        doctor: doctor.map(String::from),
        ..Default::default()
    };
    let now = 10_000;
    let gone = host_row(
        &host("mac", now - 1500, "4.6.60", Some("9000 ok")),
        "o/r",
        false,
        now,
        120,
        None,
    );
    assert_eq!(
        (gone["state"].as_str(), gone["reason"].as_str()),
        (
            Some("unknown"),
            Some("heartbeat 25m ago, read fresh; its service is not writing")
        )
    );
    assert!(!gone["fix"].as_str().unwrap().contains("install"), "{gone}");
    let ahead = host_row(
        &host("pds", now + 30, "4.6.60", Some("9990 ok")),
        "o/r",
        false,
        now,
        120,
        None,
    );
    assert_eq!(
        ahead["state"], "ok",
        "a heartbeat written after the reader's clock is fresh: {ahead}"
    );
    let old = host_row(
        &host("lin", now - 10, "4.6.50", None),
        "o/r",
        false,
        now,
        120,
        None,
    );
    assert_eq!(
        (old["state"].as_str(), old["reason"].as_str()),
        (
            Some("unknown"),
            Some("version 4.6.50 runs no health checks")
        )
    );
    let gap = host_row(
        &host(
            "win",
            now - 10,
            "4.6.58",
            Some("9900 token-contents:gap version:gap"),
        ),
        "o/r",
        false,
        now,
        120,
        None,
    );
    assert_eq!(gap["state"], "gap");
    assert_eq!(gap["gaps"], json!(["token-contents", "version"]));
    assert_eq!(
        gap["fix"],
        "owner: grant the board token of win Contents: Read for o/r; run `taskq update` on win"
    );
    let gl = host_row(
        &host("pds", now - 10, "4.6.62", Some("9900 token-pulls:gap")),
        "g/p",
        true,
        now,
        120,
        None,
    );
    assert_eq!(
        gl["fix"],
        "owner: give the GitLab board token of pds scope api and role Developer or higher on g/p"
    );
    let unread = host_row(
        &host("pds", now - 10, "4.6.62", Some("9900 token-issues:unknown")),
        "g/p",
        true,
        now,
        120,
        None,
    );
    assert_eq!(
        unread["fix"], "run `taskq doctor` on pds: it names why the token read failed",
        "an unread token asks for no grant"
    );
    assert_eq!(
        host_row(
            &host("ok", now - 10, "4.6.60", Some("9900 ok")),
            "o/r",
            false,
            now,
            120,
            None
        )["state"],
        "ok"
    );
    let lapsed = host_row(
        &host(
            "ok",
            now - 10,
            "4.6.60",
            Some(&format!("{} ok", now - STALE - 1)),
        ),
        "o/r",
        false,
        now,
        120,
        None,
    );
    assert_eq!(
        lapsed["state"], "unknown",
        "a result the service stopped refreshing is unknown"
    );
    assert_eq!(parse("9900 rm-rf:gap"), None, "only known ids");
    assert_eq!(parse("9900 version:fine"), None, "only known statuses");
    assert_eq!(parse("9900"), None);
    let tag = |mut v: Value, b: &str| {
        v["board"] = json!(b);
        v
    };
    let lines = hosts_lines(
        &[tag(gone, "o/r"), tag(gap, "o/r"), tag(gl, "g/p")],
        &[
            json!({"board": "g/p", "host": "win", "task": 7, "attempt": 42, "runtime": "claude", "alive": true, "stalled": false, "reason": null, "account": "ok (account work)"}),
        ],
        &[],
    );
    assert!(
        lines[0].starts_with("board") && lines[0].contains("host"),
        "one host table with a board column: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("o/r") && l.contains("mac") && l.contains("not writing")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("g/p") && l.contains("pds")),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("g/p ")
            && l.contains("#7")
            && l.contains("claude")
            && l.contains("yes")),
        "{lines:#?}"
    );
    // #2247 A1: the version column shows running and, during a handover, selected with its waits; equal is one value
    let mut handover = host("win", now - 10, "4.6.61", Some("9900 ok"));
    handover.selected = Some("4.6.62 waits #7 attempt 42".into());
    let row = host_row(&handover, "o/r", false, now, 120, None);
    assert_eq!(
        (row["version"].as_str(), row["running_selected"].as_str()),
        (
            Some("4.6.61"),
            Some("4.6.61 (selected 4.6.62 waits #7 attempt 42)")
        )
    );
    let lines = hosts_lines(
        &[
            tag(row, "o/r"),
            tag(
                host_row(
                    &host("lin", now - 10, "4.6.62", Some("9900 ok")),
                    "o/r",
                    false,
                    now,
                    120,
                    None,
                ),
                "o/r",
            ),
        ],
        &[],
        &[],
    );
    assert!(
        lines.iter().any(|l| l.starts_with("o/r")
            && l.contains(" win ")
            && l.contains("4.6.61 (selected 4.6.62 waits #7 attempt 42)")),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("o/r")
            && l.contains(" lin ")
            && l.contains("4.6.62")
            && !l.contains("selected")),
        "{lines:#?}"
    );
}

/// A `--fresh` or published run of one board on a virtual clock: `tick` runs at each sleep, like a host's pass.
fn fresh_run(
    b: &crate::board::Fake,
    hosts: Vec<Host>,
    wait: u64,
    tick: &dyn Fn(u64),
) -> (Vec<(Value, bool)>, u64, u64) {
    use crate::board::Board;
    let clock = std::cell::Cell::new(0);
    let (elapsed, sleep) = (
        || clock.get(),
        |t: u64| {
            clock.set(clock.get() + t);
            b.advance(t as i64);
            tick(clock.get());
        },
    );
    let pace = Pace {
        wait,
        poll: 20,
        elapsed: &elapsed,
        sleep: &sleep,
    };
    let at = Scope {
        repo: "o/r",
        gitlab: false,
        ttl: 120,
        now: b.now().unwrap(),
    };
    let issues = b.issues(None).unwrap();
    let before = b.state.borrow().reads;
    let mut events = vec![];
    ask(b, &issues, hosts, &at, &pace, &mut |e| {
        if let Event::Row(row, done) = e {
            events.push((row, done));
        }
    });
    (
        events,
        clock.get(),
        (b.state.borrow().reads - before) as u64,
    )
}

// Chain: § 6 `doctor --hosts --fresh` -> request issue -> service pass -> heartbeat `doctor-answer` (#2194 A2, #2246
// A1, #2450 A2, A3). Risk: each poll read every unanswered heartbeat of 13 boards while the quota was near its limit,
// the answers printed only at the end, and a silent host read `busy or unreachable`. Protection: at most one heartbeat
// read per poll in turn, an answer settles its host at once, a silent live host names its stuck service and the run
// stops at its wait.
#[test]
fn doctor_fresh_reads_one_heartbeat_per_poll_and_settles_each_answer() {
    use crate::board::{Board, Fake};
    let b = Fake::default();
    b.advance(10_000);
    let beat = |name: &str, body: &str| {
        b.create(
            &format!("taskq host {name}"),
            body,
            &[crate::service::HOST_LABEL.into()],
        )
        .unwrap()
    };
    let (win, _lin, mac) = (
        beat("win", "version 4.6.61\ndoctor 9990 ok"),
        beat("lin", "version 4.6.61\ndoctor 9990 ok"),
        beat("mac", "version 4.6.61\ndoctor 9990 ok"),
    );
    beat("old", "version 4.6.50");
    let now = b.now().unwrap();
    let hosts = listed(&b.issues(None).unwrap(), now, 120);
    assert_eq!(hosts.len(), 4);
    // win and mac answer on their next pass; lin stays silent, and every heartbeat keeps writing
    let answer = |_| {
        let id = request_of(&b.issues(None).unwrap())
            .expect("the request is on the board before any wait");
        b.edit(
            win,
            &format!(
                "version 4.6.61\ndoctor {now} token-pulls:gap\n{}",
                answer_line(&id)
            ),
        )
        .unwrap();
        b.edit(
            mac,
            &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id)),
        )
        .unwrap();
        b.edit(_lin, "version 4.6.61\ndoctor 9990 ok").unwrap();
    };
    let (events, waited, reads) = fresh_run(&b, hosts, 100, &answer);
    assert_eq!(
        waited, 100,
        "a silent live host holds the run to its wait, no longer"
    );
    assert!(
        reads <= waited / 20,
        "at most one heartbeat read per poll: {reads} reads in {waited}s"
    );
    let last = |name: &str| {
        events
            .iter()
            .rev()
            .find(|(r, _)| r["host"] == name)
            .unwrap()
            .clone()
    };
    let settled_at = |name: &str| {
        events
            .iter()
            .position(|(r, d)| r["host"] == name && *d)
            .unwrap()
    };
    assert!(
        settled_at("old") < settled_at("win"),
        "a host that cannot answer prints before any wait"
    );
    assert_eq!(
        (
            last("win").0["state"].as_str(),
            &last("win").0["gaps"],
            last("win").1
        ),
        (Some("gap"), &json!(["token-pulls"]), true),
        "the answer's fresh result"
    );
    assert_eq!(last("mac").0["state"], "ok");
    assert!(
        settled_at("mac") < settled_at("lin"),
        "an answer prints before the run ends"
    );
    let silent = last("lin").0;
    assert_eq!(
        (silent["state"].as_str(), silent["reason"].as_str()),
        (
            Some("unknown"),
            Some(
                "live heartbeat, no answer to the request within 100s: its service has not run a pass since"
            )
        )
    );
    assert_eq!(
        silent["last"], "ok",
        "the last published result stays visible"
    );
    assert_eq!(
        last("old").0["reason"],
        "version 4.6.50 runs no health checks"
    );
    let requests: Vec<_> = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(|i| i.title == REQUEST_TITLE)
        .collect();
    assert_eq!(requests.len(), 1);
    assert!(
        !crate::service::is_host(&requests[0]),
        "an old host's heartbeat parser ignores the request"
    );
    let first = request_of(&requests).unwrap();
    // the next run edits the same issue, is a new request and stops once all answered
    let (win_only, now) = (
        listed(&b.issues(None).unwrap(), b.now().unwrap(), 120)
            .into_iter()
            .filter(|h| h.name == "win")
            .collect::<Vec<_>>(),
        b.now().unwrap(),
    );
    let again = |_| {
        let id = request_of(&b.issues(None).unwrap()).unwrap();
        b.edit(
            win,
            &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id)),
        )
        .unwrap();
    };
    let (_, waited, _) = fresh_run(&b, win_only, 300, &again);
    assert_eq!(waited, 20, "no wait once all answered");
    assert_eq!(
        b.issues(None)
            .unwrap()
            .iter()
            .filter(|i| i.title == REQUEST_TITLE)
            .count(),
        1,
        "one reused request issue"
    );
    assert_ne!(
        request_of(&b.issues(None).unwrap()).unwrap(),
        first,
        "every run is a new request"
    );
}

// Chain: § 6 `doctor --hosts --fresh` -> list lag (#2246 A1, #2450 A3). Risk: a lagging list showed a live host stale
// and it read `not writing`, or every stale heartbeat was read at once before the first poll. Protection: a list with
// no live host costs one read at once, the first heartbeat read live posts the request and answers; a dead board gets
// no request and its hosts read stale from a fresh read.
#[test]
fn doctor_fresh_reads_listed_stale_hosts_in_turn_before_the_request() {
    use crate::board::{Board, Fake};
    let b = Fake::default();
    b.advance(10_000);
    let beat = |name: &str, body: &str| {
        b.create(
            &format!("taskq host {name}"),
            body,
            &[crate::service::HOST_LABEL.into()],
        )
        .unwrap()
    };
    let (mac, pds) = (
        beat("mac", "version 4.6.61\ndoctor 9990 ok"),
        beat("pds", "version 4.6.61\ndoctor 9990 ok"),
    );
    let now = b.now().unwrap();
    // the list lags: both read two hours old there while mac's heartbeat is fresh; pds really stopped
    let mut hosts = listed(&b.issues(None).unwrap(), now, 120);
    for h in &mut hosts {
        h.updated = 1_000;
    }
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == pds)
        .unwrap()
        .updated = 1_000;
    let answer = |_| {
        let Some(id) = request_of(&b.issues(None).unwrap()) else {
            return;
        };
        b.edit(
            mac,
            &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id)),
        )
        .unwrap();
    };
    let (events, _, reads) = fresh_run(&b, hosts, 100, &answer);
    let last = |name: &str| {
        events
            .iter()
            .rev()
            .find(|(r, _)| r["host"] == name)
            .unwrap()
            .0
            .clone()
    };
    assert_eq!(
        last("mac")["state"],
        "ok",
        "a host the list showed stale answers"
    );
    assert!(
        last("pds")["reason"]
            .as_str()
            .unwrap()
            .contains("read fresh; its service is not writing"),
        "{}",
        last("pds")
    );
    assert_eq!(reads, 3, "one at once, one per poll until both settled");
    // a board with only dead hosts: no write, no wait past their reads
    let dead = Fake::default();
    dead.advance(10_000);
    let gone = dead
        .create(
            "taskq host gone",
            "version 4.6.61\ndoctor 900 ok",
            &[crate::service::HOST_LABEL.into()],
        )
        .unwrap();
    dead.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == gone)
        .unwrap()
        .updated = 1_000;
    let hosts = listed(&dead.issues(None).unwrap(), dead.now().unwrap(), 120);
    let (events, waited, reads) = fresh_run(&dead, hosts, 100, &|_| {});
    assert_eq!((waited, reads), (0, 1), "one read at once and no sleep");
    assert!(
        events.last().unwrap().0["reason"]
            .as_str()
            .unwrap()
            .contains("not writing")
    );
    assert_eq!(
        request_of(&dead.issues(None).unwrap()),
        None,
        "no live host: no request"
    );
}

// Chain: § 6 `doctor --hosts` published result and age (#2450 A1). Risk: the default run waited up to 300s for
// answers while a manager needed the result now; a stale list read `not writing`. Protection: the default posts no
// request and never sleeps, every host shows its published result and age, and a stale listed heartbeat is read
// fresh once; a stale host keeps its last result visible beside the unknown state.
#[test]
fn doctor_hosts_prints_published_results_with_age_without_a_request_or_sleep() {
    use crate::board::{Board, Fake};
    let b = Fake::default();
    b.advance(10_000);
    let beat = |name: &str, body: &str| {
        b.create(
            &format!("taskq host {name}"),
            body,
            &[crate::service::HOST_LABEL.into()],
        )
        .unwrap()
    };
    let now = b.now().unwrap();
    beat(
        "win",
        &format!("version 4.6.61\ndoctor {} token-pulls:gap", now - 600),
    );
    beat("mac", &format!("version 4.6.61\ndoctor {} ok", now - 7200));
    let gone = beat(
        "gone",
        &format!("version 4.6.61\ndoctor {} ok", now - 90_000),
    );
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == gone)
        .unwrap()
        .updated = now - 90_000;
    let issues = b.issues(None).unwrap();
    let hosts = listed(&issues, now, 120);
    let sleep = |_: u64| panic!("the published view never sleeps");
    let elapsed = || 0;
    let pace = Pace {
        wait: 25,
        poll: 20,
        elapsed: &elapsed,
        sleep: &sleep,
    };
    let at = Scope {
        repo: "o/r",
        gitlab: false,
        ttl: 120,
        now,
    };
    let before = b.state.borrow().reads;
    let mut rows = std::collections::BTreeMap::new();
    published(&b, hosts, &at, &pace, &mut |e| {
        if let Event::Row(row, _) = e {
            rows.insert(row["host"].as_str().unwrap().to_string(), row);
        }
    });
    assert_eq!(
        b.state.borrow().reads - before,
        1,
        "only the listed-stale heartbeat is read"
    );
    assert_eq!(request_of(&b.issues(None).unwrap()), None, "no request");
    assert_eq!(
        (rows["win"]["state"].as_str(), rows["win"]["age"].as_str()),
        (Some("gap"), Some("10m"))
    );
    assert_eq!(
        (rows["mac"]["state"].as_str(), rows["mac"]["age"].as_str()),
        (Some("ok"), Some("2h"))
    );
    assert_eq!(
        (
            rows["gone"]["state"].as_str(),
            rows["gone"]["age"].as_str(),
            rows["gone"]["last"].as_str()
        ),
        (Some("unknown"), Some("1d"), Some("ok"))
    );
    let rows: Vec<Value> = rows
        .into_values()
        .map(|mut r| {
            r["board"] = json!("o/r");
            r
        })
        .collect();
    let lines = hosts_lines(&rows, &[], &[]);
    assert!(lines[0].contains("age"), "{lines:#?}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains(" mac ") && l.contains(" 2h ")),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("gone")
            && l.contains("not writing")
            && l.contains("last result: ok")),
        "{lines:#?}"
    );
}

// Chain: § 6 `doctor --hosts` deadline (#2450 A1, A2). Risk: a paused or hung board request held the run past its
// stated wait with nothing printed. Protection: the gather ends at the deadline with what each board said; a settled
// row reaches the printer as it arrives, a host's newer row replaces the older one, a silent board stays unread and
// unread workers never read `no running worker`.
#[test]
fn doctor_hosts_gather_ends_at_the_deadline_with_a_hung_board() {
    use std::time::{Duration, Instant};
    let (tx, rx) = std::sync::mpsc::channel();
    let row =
        |host: &str, state: &str| json!({"host": host, "state": state, "gaps": [], "reason": null});
    tx.send((0, Event::Listed)).unwrap();
    tx.send((0, Event::Row(row("win", "unknown"), false)))
        .unwrap();
    tx.send((0, Event::Row(row("win", "ok"), true))).unwrap();
    let mut boards = vec![Gathered::new("o/r"), Gathered::new("g/hung")];
    let mut printed = vec![];
    let began = Instant::now();
    // `tx` stays open: the second board's thread is blocked in a request
    gather(
        &rx,
        began + Duration::from_millis(100),
        &mut boards,
        &mut |r| printed.push(r["host"].as_str().unwrap().to_string()),
    );
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "the deadline ends the run"
    );
    assert_eq!(printed, ["win"]);
    assert_eq!(
        (
            boards[0].rows.len(),
            boards[0].rows[0]["state"].as_str(),
            boards[0].rows[0]["board"].as_str()
        ),
        (1, Some("ok"), Some("o/r"))
    );
    assert!(!boards[1].listed && boards[1].workers.is_none());
    let lines = hosts_lines(
        &boards[0].rows,
        &[],
        &["workers of g/hung: unknown: not read within 30s".into()],
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("workers of g/hung: unknown"))
            && !lines.iter().any(|l| l == "no running worker"),
        "{lines:#?}"
    );
    drop(tx);
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
    let row = |n, rt: &str, host: &str, heartbeat: Value| Row {
        n,
        task: crate::core::Task {
            state: State::Doing,
            attempt: Some(n * 10),
            session: Some(format!("{rt}:s{n} {host}")),
            ..Default::default()
        },
        heartbeat,
        ..Default::default()
    };
    let beat = |executor: &str, progress: &str, warning: Option<&str>| {
        json!({"freshness": "fresh", "executor": {"state": executor, "reason": "observed"},
            "progress": {"status": progress}, "warning": warning})
    };
    let idle = Row {
        n: 9,
        task: crate::core::Task {
            state: State::Ready,
            ..Default::default()
        },
        ..Default::default()
    };
    let s = Snapshot {
        source: "board",
        observed: now,
        reason: None,
        reason_code: None,
        board: Value::Null,
        hosts: vec![
            Host {
                name: "win".into(),
                updated: now - 5,
                logins: vec!["codex".into()],
                limits: vec!["claude until 2026-11-02T10:00:00Z".into()],
                ..Default::default()
            },
            Host {
                name: "lin".into(),
                updated: now - 5,
                accounts: vec!["claude work on active".into(), "codex home on".into()],
                ..Default::default()
            },
            Host {
                name: "fra".into(),
                updated: now - 5,
                ..Default::default()
            },
            Host {
                name: "old".into(),
                updated: now - 900,
                accounts: vec!["claude work on active".into()],
                ..Default::default()
            },
        ],
        tasks: vec![
            row(1, "claude", "win", beat("active", "active", None)),
            row(
                2,
                "codex",
                "win",
                beat(
                    "active",
                    "overdue",
                    Some("execution progress observation missing or stale"),
                ),
            ),
            row(
                3,
                "claude",
                "mac",
                json!({"freshness": "stale", "reason": "owning host heartbeat stale or time unknown"}),
            ),
            row(
                4,
                "claude",
                "lin",
                beat(
                    "unknown",
                    "active",
                    Some("current executor unknown; inspect execution health"),
                ),
            ),
            row(5, "codex", "lin", beat("active", "quiet", None)),
            row(6, "claude", "fra", beat("active", "quiet", None)),
            row(7, "claude", "fra", beat("waiting", "active", None)),
            row(8, "claude", "old", beat("active", "active", None)),
            idle,
        ],
    };
    let w = worker_rows(&s, 120);
    assert_eq!(w.len(), 8, "only live attempts");
    let get = |n: u64| w.iter().find(|r| r["task"] == n).unwrap();
    assert_eq!(
        (&get(1)["alive"], &get(1)["stalled"], &get(1)["account"]),
        (
            &json!(true),
            &json!(false),
            &json!("limit claude until 2026-11-02T10:00:00Z")
        )
    );
    assert_eq!(
        (&get(2)["stalled"], &get(2)["account"], &get(2)["runtime"]),
        (&json!(true), &json!("login codex off"), &json!("codex"))
    );
    assert_eq!(
        (
            &get(3)["alive"],
            &get(3)["alive_reason"],
            &get(3)["stalled"],
            &get(3)["account"]
        ),
        (
            &Value::Null,
            &json!("owning host heartbeat stale or time unknown"),
            &Value::Null,
            &json!("unknown: host heartbeat missing")
        ),
        "a stale lease is no live worker"
    );
    assert_eq!(
        (
            &get(4)["alive"],
            &get(4)["alive_reason"],
            &get(4)["account"]
        ),
        (
            &Value::Null,
            &json!("executor unknown: observed"),
            &json!("ok (account work)")
        ),
        "a fresh lease without a fresh executor is unknown, never alive"
    );
    assert_eq!(
        get(5)["account"],
        "unknown: no fresh codex account facts",
        "an inactive account line is no evidence"
    );
    assert_eq!(
        get(6)["account"],
        "unknown: no fresh claude account facts",
        "no account line and no output: unknown, never ok"
    );
    assert_eq!(
        (&get(7)["alive"], &get(7)["account"]),
        (&json!(true), &json!("ok (runtime output fresh)"))
    );
    assert_eq!(
        get(8)["account"],
        "unknown: host heartbeat 15m ago",
        "a stale heartbeat's account line is no evidence"
    );
    let lines = hosts_lines(&[], &w, &[]);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("#3") && l.contains("unknown: owning host heartbeat stale")),
        "{lines:#?}"
    );
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
    assert_eq!(
        trust_item(&home, &root, true).status,
        "ok",
        "no Claude config: Claude is not set up here"
    );
    let config = home.join(crate::runtime::CLAUDE_CONFIG);
    std::fs::write(
        &config,
        json!({"projects": {"/elsewhere": {"hasTrustDialogAccepted": true}}}).to_string(),
    )
    .unwrap();
    let gap = trust_item(&home, &root, true);
    let key = crate::runtime::owner::claude_key(&root);
    assert_eq!(
        (gap.status, gap.text.as_str()),
        (
            "gap",
            format!("Claude does not trust {key}; Claude workers cannot start here").as_str()
        )
    );
    assert!(gap.fix.as_deref().unwrap().contains(&key), "{:?}", gap.fix);
    assert_eq!(line(7, &[gap]), "doctor 7 claude-trust:gap");
    let parent = crate::runtime::owner::claude_key(&home.join("work"));
    std::fs::write(
        &config,
        json!({"projects": {parent: {"hasTrustDialogAccepted": true}}}).to_string(),
    )
    .unwrap();
    assert_eq!(
        trust_item(&home, &root, true).status,
        "ok",
        "a trusted parent covers the root"
    );
    std::fs::write(&config, "{").unwrap();
    assert_eq!(
        trust_item(&home, &root, true).status,
        "unknown",
        "an unreadable config is unknown, never ok"
    );
    let now = 10_000;
    let h = Host {
        name: "win".into(),
        version: "4.7.0".into(),
        updated: now - 10,
        doctor: Some("9900 claude-trust:gap".into()),
        ..Default::default()
    };
    let row = host_row(&h, "o/r", false, now, 120, None);
    assert_eq!(
        (row["state"].as_str(), row["gaps"][0].as_str()),
        (Some("gap"), Some("claude-trust")),
        "{row}"
    );
    assert!(
        row["fix"]
            .as_str()
            .unwrap()
            .contains("accept its trust prompt"),
        "{row}"
    );
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
    let config = home.join(crate::runtime::CLAUDE_CONFIG);
    std::fs::write(&config, json!({"projects": {}}).to_string()).unwrap();
    assert_eq!(
        trust_item(&home, &root, true).status,
        "gap",
        "a host with Claude keeps the gap"
    );
    let item = trust_item(&home, &root, false);
    assert_eq!((item.status, item.fix.as_deref()), ("ok", None), "{item:?}");
    assert_eq!(line(7, &[item]), "doctor 7 ok");
    std::fs::write(&config, "{").unwrap();
    assert_eq!(
        trust_item(&home, &root, false).status,
        "ok",
        "an unreadable config cannot block a host without Claude"
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// An install on release 1.2.0 whose launcher is the kept release 1.1.0, with a bare agentq hook: the #2300 gap.
fn stale_install(
    tag: &str,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    Vec<std::path::PathBuf>,
) {
    let base = scratch(tag);
    let agentq = base.join("agentq");
    let exe = agentq_exe();
    for version in ["1.2.0", "1.1.0"] {
        std::fs::create_dir_all(agentq.join("releases").join(version)).unwrap();
        std::fs::write(
            agentq.join("releases").join(version).join(exe),
            format!("agentq {version}"),
        )
        .unwrap();
    }
    std::fs::write(agentq.join("current"), "1.2.0").unwrap();
    std::fs::write(agentq.join(exe), "agentq 1.1.0").unwrap();
    let hooks = base.join("settings.json");
    std::fs::write(
        &hooks,
        json!({"command": "agentq hook claude pre"}).to_string(),
    )
    .unwrap();
    (base, agentq, vec![hooks])
}

fn no_onboard(item: &Item, status: &str) {
    assert_eq!(item.status, status, "{item:?}");
    assert!(
        !item.fix.as_deref().unwrap_or("").contains("onboard"),
        "{item:?}"
    );
    assert!(
        !line(7, std::slice::from_ref(item)).contains(":gap"),
        "`--hosts` turns a gap into onboard advice: {item:?}"
    );
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
    assert_eq!(
        hook_item(&agentq, &files).status,
        "gap",
        "no state file: no disable intent"
    );
    std::fs::write(
        &state,
        json!({"schema": 1, "host_disabled": true, "profiles": {}}).to_string(),
    )
    .unwrap();
    no_onboard(&hook_item(&agentq, &files), "ok");
    std::fs::write(
        &state,
        json!({"schema": 1, "host_disabled": false, "profiles": {"claude": {}}}).to_string(),
    )
    .unwrap();
    no_onboard(&hook_item(&agentq, &files), "ok");
    std::fs::write(&state, "{").unwrap();
    no_onboard(&hook_item(&agentq, &files), "unknown");
    std::fs::write(
        &state,
        json!({"schema": 1, "host_disabled": false, "profiles": {"hermes": {}}}).to_string(),
    )
    .unwrap();
    let unrelated = hook_item(&agentq, &files);
    assert_eq!(unrelated.status, "gap", "{unrelated:?}");
    let fix = unrelated.fix.as_deref().unwrap();
    assert!(
        fix.starts_with("run `agentq onboard --yes claude`:"),
        "{fix}"
    );
    assert!(!fix.contains("agentq enable"), "{fix}");
    std::fs::write(agentq.join(agentq_exe()), "agentq 1.2.0").unwrap();
    std::fs::remove_file(agentq.join("releases/1.2.0").join(agentq_exe())).unwrap();
    let unread = hook_item(&agentq, &files);
    assert_eq!(unread.status, "unknown", "{unread:?}");
    assert!(
        !unread.fix.as_deref().unwrap().contains("onboard"),
        "bare onboard refuses while a client is disabled: {unread:?}"
    );
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
    let command = |client: &str| {
        json!({"command": format!("\"{}\" hook {client} pre", dev.display())}).to_string()
    };
    std::fs::write(&files[0], command("claude")).unwrap();
    let outside = hook_item(&agentq, &files);
    assert_eq!(outside.status, "gap", "{outside:?}");
    assert!(
        outside.text.contains(&dev.display().to_string()),
        "{outside:?}"
    );
    let codex = base.join("hooks.json");
    std::fs::write(&codex, command("codex")).unwrap();
    files.push(codex);
    let second = base.join("other.json");
    std::fs::write(&second, command("codex")).unwrap();
    files.push(second);
    assert!(
        hook_item(&agentq, &files)
            .fix
            .as_deref()
            .unwrap()
            .starts_with("run `agentq onboard --yes claude codex`:"),
        "each stale client once"
    );
    files.truncate(1);
    std::fs::write(&dev, "agentq 1.2.0").unwrap();
    assert_eq!(
        hook_item(&agentq, &files).status,
        "ok",
        "a checkout binary equal to the release runs current hook code"
    );
    std::fs::write(
        &files[0],
        json!({"command": "agentq hook claude pre"}).to_string(),
    )
    .unwrap();
    std::fs::write(agentq.join(agentq_exe()), "local build").unwrap();
    assert_eq!(
        hook_item(&agentq, &files).status,
        "gap",
        "an install launcher equal to no kept release"
    );
    assert_eq!(
        data_item(&agentq).status,
        "gap",
        "a missing data folder blocks every hook"
    );
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
    assert_eq!(
        hook_item(&base.join("none"), &files).status,
        "ok",
        "no agentq install"
    );
    assert_eq!(hook_item(&agentq, &files).status, "ok", "no hook file");
    std::fs::write(&claude, json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "other hook x"}, {"type": "command", "command": "agentq hook claude pre"}]}]}}).to_string()).unwrap();
    let gap = hook_item(&agentq, &files);
    assert_eq!(gap.status, "gap", "{gap:?}");
    assert!(
        gap.text.starts_with("agentq hook in settings.json runs ")
            && gap.text.ends_with("not the current release 1.2.0"),
        "{}",
        gap.text
    );
    assert!(
        gap.fix.as_deref().unwrap().contains("agentq onboard --yes"),
        "{:?}",
        gap.fix
    );
    std::fs::write(agentq.join(exe), b"agentq 1.2.0").unwrap();
    assert_eq!(
        hook_item(&agentq, &files).status,
        "ok",
        "the launcher is the current release"
    );
    let old = agentq.join("releases/1.1.0").join(exe);
    std::fs::write(&codex, json!({"agentq": [{"command": format!("\"{}\" hook antigravity PreToolUse", old.display())}]}).to_string()).unwrap();
    assert!(
        hook_item(&agentq, &files).text.contains("hooks.json"),
        "a quoted path to an old release is a gap"
    );
    std::fs::write(&codex, "{").unwrap();
    assert_eq!(
        hook_item(&agentq, &files).status,
        "unknown",
        "an unreadable hook file is unknown, never ok"
    );
    assert_eq!(
        hook_binary("'/opt/a b/agentq' hook codex pre").as_deref(),
        Some("/opt/a b/agentq")
    );
    assert_eq!(
        hook_binary("env X=1 agentq.exe hook claude pre").as_deref(),
        Some("agentq.exe")
    );
    assert_eq!(hook_binary("notagentq hook claude pre"), None);
    let now = 10_000;
    let h = Host {
        name: "mac".into(),
        version: "4.7.0".into(),
        updated: now - 10,
        doctor: Some("9900 agentq-hook:gap".into()),
        ..Default::default()
    };
    let row = host_row(&h, "o/r", false, now, 120, None);
    assert_eq!(
        (row["state"].as_str(), row["fix"].as_str()),
        (
            Some("gap"),
            Some(
                "run `taskq doctor` on mac and the agentq command it names: its client hooks differ from the current agentq release"
            )
        ),
        "{row}"
    );
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
    std::fs::write(
        &hook,
        json!({"command": "agentq hook antigravity PreToolUse"}).to_string(),
    )
    .unwrap();
    let files = vec![hook];
    assert_eq!(
        hook_item(&agentq, &files).status,
        "unknown",
        "missing hook binary is unread, not proved old"
    );
    std::fs::create_dir(agentq.join(exe)).unwrap();
    assert_eq!(
        hook_item(&agentq, &files).status,
        "unknown",
        "a directory cannot be read as a hook binary"
    );
    let blocked = base.join("file");
    std::fs::write(&blocked, "not a directory").unwrap();
    assert_eq!(
        hook_item(&blocked, &files).status,
        "unknown",
        "an install path that is a file is not an absent install"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            hook_item(&blocked.join("agentq"), &files).status,
            "unknown",
            "install metadata failure is not absence"
        );
        let parent = base.join("denied");
        std::fs::create_dir_all(parent.join("agentq")).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = hook_item(&parent.join("agentq"), &files);
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            denied.status, "unknown",
            "inaccessible install root must not read as not installed"
        );
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
    assert_eq!(
        std::fs::read_dir(&data).unwrap().count(),
        1,
        "the probe leaves no file"
    );
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap(),
        "held",
        "the lock is unchanged"
    );
    let mut p = std::fs::metadata(&lock).unwrap().permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&lock, p.clone()).unwrap();
    let gap = data_item(&agentq);
    #[allow(clippy::permissions_set_readonly_false)]
    p.set_readonly(false);
    std::fs::set_permissions(&lock, p).unwrap();
    assert_eq!(gap.status, "gap", "{gap:?}");
    assert!(gap.text.contains("not writable"), "{}", gap.text);
    assert_eq!(
        gap.fix.as_deref(),
        Some("give this OS user write access to the agentq data folder")
    );
    let now = 10_000;
    let h = Host {
        name: "mac".into(),
        version: "4.7.0".into(),
        updated: now - 10,
        doctor: Some("9900 agentq-data:gap agentq-hook:unknown".into()),
        ..Default::default()
    };
    let row = host_row(&h, "o/r", false, now, 120, None);
    assert_eq!(
        row["gaps"],
        json!(["agentq-data", "agentq-hook (unknown)"]),
        "{row}"
    );
    assert!(
        row["fix"]
            .as_str()
            .unwrap()
            .starts_with("on mac: give its OS user write access to the agentq data folder"),
        "{row}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

// Chain: Mac Codex launch -> `taskq doctor` codex-git -> heartbeat -> `--hosts` (#2449 A3). Risk: doctor read ok while
// Git in linked worktrees failed. Protection: a named launch is a gap with one fix, and `--hosts` names it.
#[test]
fn doctor_names_a_mac_codex_launch_without_git_metadata() {
    assert_eq!(codex_git_item(None).status, "ok");
    let gap = codex_git_item(Some("Mac codex legacy launch cannot write /p/.git".into()));
    assert_eq!(
        (gap.status, gap.text.as_str()),
        ("gap", "Mac codex legacy launch cannot write /p/.git")
    );
    assert!(
        gap.fix.as_deref().unwrap().contains("--mode workspace"),
        "{:?}",
        gap.fix
    );
    assert_eq!(line(7, &[gap]), "doctor 7 codex-git:gap");
    let now = 10_000;
    let h = Host {
        name: "mac".into(),
        version: "4.7.0".into(),
        updated: now - 10,
        doctor: Some("9900 codex-git:gap".into()),
        ..Default::default()
    };
    let row = host_row(&h, "o/r", false, now, 120, None);
    assert_eq!(
        (row["state"].as_str(), row["gaps"][0].as_str()),
        (Some("gap"), Some("codex-git")),
        "{row}"
    );
    assert!(
        row["fix"].as_str().unwrap().contains("Git metadata"),
        "{row}"
    );
}

// Chain: § 3 Disk guard reading -> `taskq doctor` line -> `--hosts` fix (#2492 A2). Risk: a held host passes doctor or
// an unread volume reads ok. Protection: gap below the threshold, ok at it, unknown stays unknown; each open item has
// one fix, and no text carries a path.
#[test]
fn disk_item_is_gap_below_the_threshold_ok_at_it_and_unknown_when_unread() {
    use crate::disk::{Disk, MIN_FREE};
    let cases = [
        (Disk::Free(MIN_FREE - 1), "gap", true),
        (Disk::Free(MIN_FREE), "ok", false),
        (Disk::Unknown, "unknown", true),
    ];
    for (disk, status, fix) in cases {
        let item = disk_item(disk);
        assert_eq!((item.status, item.fix.is_some()), (status, fix), "{disk:?}");
        assert!(!item.text.contains(['/', '\\']), "{}", item.text);
    }
    assert!(
        IDS.contains(&"disk"),
        "a heartbeat `doctor` line may carry it"
    );
    let h = |doctor: &str| Host {
        name: "win".into(),
        version: crate::platform::VERSION.into(),
        updated: 10_000,
        doctor: Some(doctor.into()),
        ..Default::default()
    };
    for (line, fix) in [
        ("9990 disk:gap", "on win: free space on the project volume"),
        ("9990 disk:unknown", "on win: let the service user read"),
    ] {
        let row = host_row(&h(line), "o/r", false, 10_000, 120, None);
        assert!(row["fix"].as_str().unwrap().starts_with(fix), "{row}");
    }
}
