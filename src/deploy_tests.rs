use super::*;
use crate::board::Fake;

fn deploy() -> Deploy {
    Deploy {
        project: "taskq".into(),
        envs: vec!["dev".into(), "live".into()],
    }
}

fn entry(id: u64, body: &str) -> Entry {
    Entry {
        id,
        body: body.into(),
        edited: false,
    }
}

/// The entries after `spawn` (id `attempt`) of a task closed from review with `changed` as its Changed line.
fn tail(changed: &str, attempt: u64) -> Vec<String> {
    let sha = "a".repeat(40);
    vec![
        format!(
            "taskq: result {sha} attempt {attempt}
Changed: {changed}
Open: none"
        ),
        format!(
            "taskq: close
Changed: {changed}
Open: none
delivery auto-merge r0: merged #7"
        ),
    ]
}

fn closed(changed: &str) -> Vec<Entry> {
    std::iter::once("taskq: spawn worker claude:s1 mac".to_string())
        .chain(tail(changed, 1))
        .enumerate()
        .map(|(k, b)| entry(k as u64 + 1, &b))
        .collect()
}

fn board() -> Fake {
    let b = Fake::default();
    b.state.borrow_mut().summaries = true;
    b
}

fn add_closed(b: &Fake, title: &str, changed: &str) -> u64 {
    let n = b.create(title, "", &["taskq".into()]).unwrap();
    let a = b.comment(n, "taskq: spawn worker claude:s1 mac").unwrap();
    for body in tail(changed, a) {
        b.comment(n, &body).unwrap();
    }
    b.close(n).unwrap();
    n
}

fn rec(id: u64, env: &str, version: &str, at: &str, host: Option<&str>) -> Entry {
    let mut body = format!("taskq: deployed {env} {version}\nat {at}");
    if let Some(h) = host {
        body.push_str(&format!("\nhost {h}"));
    }
    entry(
        id,
        &core::with_fields(body, Some(&op_id(env, version, host)), None),
    )
}

// Chain: R1, § 2 Deploy -> fold ignores `deployed` (N-1 too) -> a closed task stays closed with the same fold.
// Risk: a deploy record reopens a task or changes its fold, so its dependents or labels move.
#[test]
fn deployed_entries_never_change_a_closed_fold() {
    let log = closed("the menu shows deploys");
    let before = core::fold(9, &log, false);
    assert_eq!(before.state, State::Closed);
    let mut after = log.clone();
    after.push(rec(4, "live", "4.2.0", "2026-10-31T12:00:00Z", Some("mac")));
    after.push(entry(
        5,
        "taskq: deployed dev 4.2.0 attempt 1\nat 2026-10-31T12:00:00Z",
    ));
    assert_eq!(core::fold(9, &after, false), before);
    let p = core::project(9, &after, false);
    assert!(
        !p.accepts(4) && !p.accepts(5),
        "a deployed entry is never an accepted state entry"
    );
}

// Chain: § 2 Deploy summary -> waiting line, then one line per environment in configured order, first entry wins.
// Risk: lines in board order instead of deploy order, a repeat shown twice, or a no-change close waiting forever.
#[test]
fn summary_lines_follow_the_configured_environment_order() {
    let d = deploy();
    let mut log = closed("the menu shows deploys");
    assert_eq!(lines(9, &log, &d), vec![WAITING.to_string()]);
    assert!(waiting(9, &log, &d));
    log.push(rec(4, "live", "4.2.0", "2026-10-31T12:00:00Z", Some("mac")));
    log.push(rec(5, "dev", "4.2.0-dev.3", "2026-10-30T09:00:00Z", None));
    log.push(rec(6, "live", "4.2.1", "2026-11-01T08:00:00Z", Some("win")));
    log.push(rec(7, "staging", "4.2.1", "2026-11-01T08:00:00Z", None));
    assert_eq!(
        lines(9, &log, &d),
        vec![
            "taskq 4.2.0-dev.3 · dev · 2026-10-30T09:00:00Z".to_string(),
            "taskq 4.2.0 · live · 2026-10-31T12:00:00Z · mac".to_string(),
        ]
    );
    assert!(!waiting(9, &log, &d));
    assert!(
        lines(9, &closed("none: already on main"), &d).is_empty(),
        "a no-change close shows no deploy line"
    );
    let open = &closed("x")[..2];
    assert!(lines(9, open, &d).is_empty(), "an open task shows none");
}

#[test]
fn deploy_setting_reads_words_in_order_or_nothing() {
    assert_eq!(
        parse(&json!({"repo": "o/taskq", "deploy": ["dev", "live"]})),
        Some(deploy())
    );
    for bad in [
        json!({"repo": "o/taskq"}),
        json!({"repo": "o/taskq", "deploy": []}),
        json!({"repo": "o/taskq", "deploy": ["dev", "dev"]}),
        json!({"repo": "o/taskq", "deploy": ["two words"]}),
        json!({"repo": "o/taskq", "deploy": "live"}),
    ] {
        assert_eq!(parse(&bad), None, "{bad}");
    }
    assert!(
        range("v4.1.0..v4.2.0")
            && !range("v4.2.0")
            && !range("..v4")
            && !range("-x..y")
            && !range("a...b")
    );
}

// A6: the release flow records only in a release root whose `deploy` names the channel. Risk: the tracked
// taskq.json loses `deploy` or `live`, and a live host update records nothing.
#[test]
fn release_root_records_live_deploys() {
    let v: Value = serde_json::from_str(include_str!("../taskq.json")).unwrap();
    assert_eq!(v["repo"].as_str(), Some(crate::platform::RELEASE_REPO));
    assert!(
        parse(&v).is_some_and(|d| d.envs.iter().any(|e| e == "live")),
        "taskq.json deploy must name live"
    );
}

// Chain: A2 `taskq deployed` -> one entry per trailer task of this board, a rerun is a no-op by op id, the label
// and summary follow. Risk: a rerun posts twice, another board's task is written, or the waiting label stays.
#[test]
fn record_is_idempotent_and_skips_other_boards() {
    let b = board();
    let d = deploy();
    let n = add_closed(&b, "Show deploys", "the menu shows deploys");
    let m = add_closed(&b, "No change", "none: nothing to do");
    let open = b.create("Still open", "", &["taskq".into()]).unwrap();
    let other = "https://github.com/other/repo/issues/5";
    let msgs = format!(
        "fix: x\n\nTask: {}\n\nTask: {}\nTask: {other}\nTask: {}\n",
        b.url(n),
        b.url(m),
        b.url(open)
    );
    let (mine, skip) = tasks(&b, &msgs);
    assert_eq!(mine.iter().copied().collect::<Vec<_>>(), vec![n, m, open]);
    b.labels(n, &[LABEL.into()], &[]).unwrap();
    let first = record(&b, &d, &mine, skip.clone(), "4.2.0", "live", Some("mac")).unwrap();
    assert_eq!(
        first["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["outcome"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["accepted", "accepted"]
    );
    let skipped: Vec<&str> = first["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["url"].as_str().unwrap())
        .collect();
    assert_eq!(skipped, [other.to_string(), b.url(open)]);
    let count = |n: u64| {
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|c| c.body.starts_with("taskq: deployed"))
            .count()
    };
    assert_eq!((count(n), count(m), count(open)), (1, 1, 0));
    assert!(
        !b.issue(n).unwrap().labels.iter().any(|l| l == LABEL),
        "the first record removes the waiting label"
    );
    let summary = b
        .comments(n)
        .unwrap()
        .into_iter()
        .find(crate::summary::is)
        .unwrap()
        .body;
    assert!(
        summary.contains("\ntaskq 4.2.0 · live · ") && summary.ends_with(" · mac"),
        "{summary}"
    );
    let again = record(&b, &d, &mine, skip, "4.2.0", "live", Some("mac")).unwrap();
    assert!(
        again["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["outcome"] == "repeat"),
        "{again}"
    );
    assert_eq!(
        (count(n), count(m)),
        (1, 1),
        "a rerun with the same version and environment writes nothing"
    );
}

// Chain: A4 status -> closed tasks labelled deploy-waiting, oldest first, each linked.
#[test]
fn status_lists_waiting_tasks_oldest_first() {
    let b = board();
    let d = deploy();
    assert!(status(&b, Some(&d)).unwrap().is_empty());
    let n = add_closed(&b, "Older", "x");
    let m = add_closed(&b, "Newer", "y");
    for k in [m, n] {
        b.labels(k, &[LABEL.into()], &[]).unwrap();
    }
    b.advance(4 * 86400);
    let now = b.now().unwrap();
    b.state
        .borrow_mut()
        .report_closed
        .extend([(n, now - 3 * 86400), (m, now - 7200)]);
    let lines = status(&b, Some(&d)).unwrap();
    assert_eq!(lines[0], "Waiting for deploy: 2 tasks, oldest 3d (taskq)");
    assert!(
        lines[1].starts_with(&format!("  #{n} Older · closed 3d ago · {}", b.url(n))),
        "{lines:?}"
    );
    assert!(lines[2].starts_with(&format!("  #{m} Newer")), "{lines:?}");
    assert!(
        status(&b, None).unwrap().is_empty(),
        "no deploy setting, no line"
    );
}
