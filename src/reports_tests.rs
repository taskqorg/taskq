use super::*;
fn fixture() -> (Issue, Vec<Comment>) {
    let i = Issue {
        number: 12,
        trusted: true,
        open: false,
        reason: Some("completed".into()),
        labels: vec!["taskq".into(), "research".into()],
        ..Default::default()
    };
    let c = |id, body: String| Comment {
        id,
        body,
        trusted: true,
        ..Default::default()
    };
    (
        i,
        vec![
            c(1, "taskq: spawn worker invented:session host".into()),
            c(2, format!("taskq: result {} attempt 1", "a".repeat(40))),
            c(3, "taskq: close".into()),
        ],
    )
}
// Chain: §7 -> closed issue + trusted markers -> Reports projection. Risk: reports alter the queue, stale
// or untrusted decisions hide a new report, or reading changes close-time ordering.
#[test]
fn decisions_are_separate_from_queue_and_bound_to_the_result() {
    let (i, mut comments) = fixture();
    let sha = "a".repeat(40);
    let row = |cs: &[Comment]| project(&i, 45, cs, "invented".into()).unwrap();
    assert_eq!(row(&comments)["decision"], "none");
    assert_eq!(row(&comments)["read"], false);
    for (id, kind, decision, visible) in [
        (4, "read", "none", true),
        (5, "later", "later", true),
        (6, "drop", "drop", false),
        (7, "made", "drop", false),
    ] {
        comments.push(Comment {
            id,
            body: format!("taskq: report-{kind} {sha}"),
            trusted: true,
            ..Default::default()
        });
        let r = row(&comments);
        assert_eq!(r["decision"], decision);
        assert_eq!(r["visible"], visible);
        assert_eq!(r["closed"], 45);
        assert_eq!(
            crate::core::project(12, &log(&comments), false).task.state,
            crate::core::State::Closed
        );
    }
    let (i, mut cs) = fixture();
    for (id, trusted, edited, s) in [
        (4, false, false, sha.clone()),
        (5, true, true, sha.clone()),
        (6, true, false, "b".repeat(40)),
    ] {
        cs.push(Comment {
            id,
            body: format!("taskq: report-drop {s}"),
            trusted,
            edited,
            ..Default::default()
        });
    }
    assert_eq!(
        project(&i, 45, &cs, "invented".into()).unwrap()["read"],
        false
    );
    cs.push(Comment {
        id: 7,
        body: format!("taskq: report-made {sha}\n[20,21]"),
        trusted: true,
        ..Default::default()
    });
    let r = project(&i, 45, &cs, "invented".into()).unwrap();
    assert_eq!(r["visible"], false);
    assert_eq!(r["tasks_made"], json!([20, 21]));
}
// Chain: §7 -> report JSON -> §1 task validation. Risk: an invalid proposal partially creates tasks or grants host policy.
#[test]
fn report_requires_reviewable_findings_and_data_only_proposals() {
    let body = "# Question\nQ\n# Findings\nEvidence\n# Options\nPros/cons\n# Recommendation\nR\n# Proposed tasks\n```taskq-tasks\n[]\n```";
    assert!(proposals(body).unwrap().is_empty());
    assert!(
        proposals(&body.replace(
            "[]",
            "[{\"title\":\"X\",\"goal\":\"G\",\"acceptance\":[\"A\"],\"permission_mode\":\"full\"}]"
        ))
        .is_err()
    );
    assert!(proposals(&body.replace("# Findings", "# Missing")).is_err());
}

fn board() -> (crate::board::Fake, u64, u64, String) {
    let b = crate::board::Fake::default();
    let claim = b.create("Invented create claims", "", &[]).unwrap();
    let n = b
        .create("Retry research", "", &["taskq".into(), "docs".into()])
        .unwrap();
    let spawn = b.raw(n, "taskq: spawn worker invented:session host", true, false);
    let sha = "a".repeat(40);
    b.raw(
        n,
        &format!("taskq: result {sha} attempt {spawn}\nReport: docs/research/{n}.md"),
        true,
        false,
    );
    b.raw(n, "taskq: close", true, false);
    b.close(n).unwrap();
    let body = "# Question\nQ\n# Findings\nEvidence\n# Options\nPros/cons\n# Recommendation\nR\n# Proposed tasks\n```taskq-tasks\n[{\"title\":\"Document limits\",\"goal\":\"Show retry limits.\",\"acceptance\":[\"Help names retry limits.\"],\"type\":\"docs\"},{\"title\":\"Protect retries\",\"goal\":\"Keep retries bounded.\",\"acceptance\":[\"A failed request stops at its retry limit.\"]}]\n```";
    b.state.borrow_mut().report_sources.push((
        format!("docs/research/{n}.md"),
        sha.clone(),
        body.into(),
    ));
    b.state.borrow_mut().report_closed.insert(n, 45);
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == n)
        .unwrap()
        .closed = Some(45);
    (b, claim, n, sha)
}

// Chain: Product / RC4–RC7 -> report decisions and claims. Risk: Accept hides partial drafts,
// decided content disappears, or a report correction reopens a completed dependency.
#[test]
fn conversations_1977_decisions_keep_history_and_creation_fences() {
    let (b, claim, n, sha) = board();
    let raw = json!({"create_claim_issue":claim});
    let root = std::env::temp_dir().join(format!("taskq-1977-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let accepted = owner_act_until(&b, &root, &raw, n, &sha, "accept", None).unwrap();
    assert_eq!(accepted["report"]["decision"], "accepted");
    assert_eq!(accepted["report"]["read"], true);
    let count = b.comments(n).unwrap().len();
    owner_act_until(&b, &root, &raw, n, &sha, "accept", None).unwrap();
    assert_eq!(b.comments(n).unwrap().len(), count);
    assert_eq!(
        owner_act_until(&b, &root, &raw, n, &sha, "make", None).unwrap()["report"]["decision"],
        "accepted"
    );
    let corrected = successor(
        &b,
        &root,
        &raw,
        n,
        &sha,
        "reopen",
        "Keep the original receipt",
    )
    .unwrap();
    let child = corrected["successor"].as_u64().unwrap();
    assert!(b.issue(child).unwrap().labels.contains(&"q-held".into()));
    assert!(!b.issue(n).unwrap().open);
    let mut cache = Cache::default();
    cache.collect(&b);
    assert!(cache.value["reports"].as_array().unwrap().is_empty());
    assert!(
        cache.value["history"][0]["body"].is_string(),
        "decided report content stays readable"
    );
    std::fs::write(
        crate::paths::dot(&root).join("reports.json"),
        cache.value.to_string(),
    )
    .unwrap();
    assert!(cached_open(&root, n, &sha).unwrap()["report"]["body"].is_string());
    let (other, oc, on, os) = board();
    mark(&other, on, &os, "making", &[]).unwrap();
    let pending = owner_act_until(
        &other,
        &root,
        &json!({"create_claim_issue":oc}),
        on,
        &os,
        "accept",
        None,
    )
    .unwrap();
    assert!(pending["reason"].is_string());
    assert_eq!(pending["report"]["pending_decision"], "make");
    other.raw(on, &format!("taskq: report-accept {os}"), true, false);
    assert_eq!(
        current(&other, on, &os).unwrap()["pending_decision"],
        "make",
        "a raced acknowledgement cannot win against a make reservation"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: §7 Results -> trusted completion/seen markers -> held successor with immutable create claim.
// Risk: acknowledgement reopens a dependency, old markers hide a new SHA, or retry duplicates correction work.
#[test]
fn ordinary_results_acknowledge_and_reopen_without_reopening_dependencies() {
    let (b, claim, n, sha) = board();
    {
        let mut st = b.state.borrow_mut();
        st.issues.iter_mut().find(|i| i.number == n).unwrap().labels =
            vec!["taskq".into(), "code".into()];
        let (_, result) = st
            .comments
            .iter_mut()
            .find(|(task, c)| *task == n && c.body.starts_with("taskq: result "))
            .unwrap();
        result.body = result.body.split_once('\n').unwrap().0.to_string()
            + "\nChanged: Requests recover once.\nChecks: replay -- passed\nOpen: none\nhttps://example.com/invented-evidence";
    }
    let root = std::env::temp_dir().join(format!("taskq-results-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let raw = json!({"create_claim_issue":claim});
    let row = current(&b, n, &sha).unwrap();
    assert_eq!(row["kind"], "task");
    assert!(row["body"].as_str().unwrap().contains("Checks: replay"));
    for (trusted, edited, s) in [
        (false, false, sha.clone()),
        (true, true, sha.clone()),
        (true, false, "b".repeat(40)),
    ] {
        b.raw(n, &format!("taskq: result-read {s}"), trusted, edited);
    }
    assert_eq!(current(&b, n, &sha).unwrap()["read"], false);
    assert_eq!(
        owner_act_until(&b, &root, &raw, n, &sha, "read", None).unwrap()["report"]["decision"],
        "none"
    );
    assert_eq!(
        owner_act_until(&b, &root, &raw, n, &sha, "accept", None).unwrap()["report"]["decision"],
        "accepted"
    );
    let opened = successor(
        &b,
        &root,
        &raw,
        n,
        &sha,
        "reopen",
        "Preserve the retry receipt",
    )
    .unwrap();
    assert!(opened["reason"].is_null(), "{opened}");
    let child = opened["successor"].as_u64().unwrap();
    let issue = b.issue(child).unwrap();
    assert!(issue.body.contains("Reason: Preserve the retry receipt"));
    assert!(issue.body.contains(&format!("Depends on #{n}")));
    assert!(issue.labels.contains(&"q-held".into()));
    assert!(!issue.labels.contains(&"taskq".into()));
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == n)
        .unwrap()
        .title = "Renamed after creation".into();
    let count = b.state.borrow().issues.len();
    assert_eq!(
        successor(
            &b,
            &root,
            &raw,
            n,
            &sha,
            "reopen",
            "Preserve the retry receipt"
        )
        .unwrap()["successor"],
        child
    );
    assert_eq!(b.state.borrow().issues.len(), count);
    assert!(!b.issue(n).unwrap().open);
    assert_eq!(
        crate::core::fold(n, &crate::board::entries(&b, n).unwrap(), false).state,
        crate::core::State::Closed
    );
    assert!(successor(&b, &root, &raw, n, &sha, "reopen", "").is_err());
    let mut cache = Cache::default();
    cache.collect(&b);
    assert_eq!(cache.value["reports"][0]["kind"], "task");
    assert_eq!(cache.value["reports"][0]["read"], true);
    std::fs::remove_dir_all(root).unwrap();
}

// A5/A6: paging must not skip racing updates; a restart may retain data but never assert a failed read is healthy.
#[test]
fn collection_cursor_precedes_paging_and_restart_retains_failed_rows() {
    let (b, _, n, sha) = board();
    let before = b.now().unwrap();
    b.state.borrow_mut().closed_page_advance = 10;
    let mut cache = Cache::default();
    cache.collect(&b);
    assert_eq!(cache.verified, Some(before));
    b.raw(n, &format!("taskq: report-later {sha}"), true, false);
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == n)
        .unwrap()
        .updated = before + 2;
    cache.collect(&b);
    assert_eq!(
        cache.value["reports"][0]["decision"], "later",
        "update inside paging window must be revisited"
    );
    let mut restarted = Cache::retaining(cache.value.clone());
    b.state.borrow_mut().fail_comment_reads = 1;
    b.advance(2);
    let moved = b.now().unwrap();
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == n)
        .unwrap()
        .updated = moved;
    restarted.collect(&b);
    assert_eq!(restarted.value["complete"], false);
    assert_eq!(restarted.value["reports"][0]["n"], n);
    assert!(
        restarted.value["reason"]
            .as_str()
            .unwrap()
            .contains("comment read failed")
    );
}

// §7/R12: ignored close comments cannot manufacture delivery evidence or replace the accepted verdict.
#[test]
fn result_verdict_uses_only_the_accepted_close() {
    let (mut i, mut comments) = fixture();
    i.labels = vec!["taskq".into(), "code".into()];
    comments[2].body.push_str(
        "\nChanged: Reviewed result\nChecks: A1 ok\ndelivery auto-merge r0: merged #20 as SHA",
    );
    comments.push(Comment {
        id: 4,
        body:
            "taskq: close attempt 999\nChanged: Ignored\ndelivery auto-merge r0: merged #999 as SHA"
                .into(),
        trusted: true,
        ..Default::default()
    });
    let row = project(&i, 45, &comments, "invented".into()).unwrap();
    assert!(row["verdict"].as_str().unwrap().contains("Reviewed result"));
    assert!(!row["verdict"].as_str().unwrap().contains("Ignored"));
    assert!(row["merged"].as_str().unwrap().contains("#20"));
    assert!(!row["merged"].as_str().unwrap().contains("#999"));
}

// R6/§7: legacy answered research must not regain attention or disappear from Completed history.
#[test]
fn legacy_answered_reports_remain_in_history_without_new_attention() {
    let (b, _, n, _) = board();
    {
        let mut st = b.state.borrow_mut();
        st.issues.iter_mut().find(|i| i.number == n).unwrap().labels =
            vec!["taskq".into(), "research".into()];
        let (_, c) = st
            .comments
            .iter_mut()
            .find(|(task, c)| *task == n && c.body.starts_with("taskq: result "))
            .unwrap();
        c.body = c.body.lines().next().unwrap().into();
    }
    b.raw(n, "taskq: answer 1", true, false);
    let mut cache = Cache::default();
    cache.collect(&b);
    assert!(cache.value["reports"].as_array().unwrap().is_empty());
    assert_eq!(cache.value["history"][0]["n"], n);
    assert_eq!(cache.value["history"][0]["read"], true);
}

// #1940 A5/A6/R12: a background collection must retain SHA content, observe new decisions/closures,
// and preserve useful rows as unknown on failure. Cached reads have no Board argument or token lookup.
#[test]
fn collected_reports_refresh_without_losing_unknown_or_sha_identity() {
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    let first = cache.value.clone();
    assert_eq!(first["complete"], true);
    assert_eq!(first["reports"][0]["sha"], sha);
    assert!(first["reports"][0]["body"].is_string());
    // The source is immutable; an unchanged pass and a read marker need no source fetch.
    b.state.borrow_mut().report_sources.clear();
    cache.collect(&b);
    assert_eq!(cache.value["reports"], first["reports"]);
    mark(&b, n, &sha, "read", &[]).unwrap();
    cache.collect(&b);
    assert_eq!(cache.value["reports"][0]["read"], true);
    assert!(cache.value["reports"][0]["body"].is_string());
    // Replacing a result never borrows the old SHA's content or its read marker.
    let replacement = "b".repeat(40);
    let mut st = b.state.borrow_mut();
    let result = st
        .comments
        .iter_mut()
        .find(|(task, c)| *task == n && c.body.starts_with("taskq: result "))
        .unwrap();
    result.1.body = result.1.body.replace(&sha, &replacement);
    st.issues
        .iter_mut()
        .find(|i| i.number == n)
        .unwrap()
        .updated += 1;
    drop(st);
    cache.collect(&b);
    assert_eq!(cache.value["reports"][0]["read"], false);
    assert!(cache.value["reports"][0]["body"].is_null());
    assert!(
        cache.value["reports"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("unavailable")
    );
    b.state.borrow_mut().fail_comment_reads = 1;
    cache.collect(&b);
    assert_eq!(cache.value["complete"], false);
    assert_eq!(cache.value["reports"][0]["n"], n);
    assert!(cache.value["reason"].is_string());
    let (new, _, other, _) = board();
    // A cold collection discovers an already closed report without any UI request.
    let mut fresh = Cache::default();
    fresh.collect(&new);
    assert_eq!(fresh.value["reports"][0]["n"], other);
}

// A5/A6: same-second markers must become visible; settled historical issues must not cost another comment read.
#[test]
fn collection_rechecks_same_second_updates_then_reuses_settled_projections() {
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    let updated = b.issue(n).unwrap().updated;
    b.raw(n, &format!("taskq: report-later {sha}"), true, false);
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == n)
        .unwrap()
        .updated = updated;
    b.advance(2);
    cache.collect(&b);
    assert_eq!(cache.value["reports"][0]["decision"], "later");
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    let reads = b.state.borrow().comment_reads;
    cache.collect(&b);
    assert_eq!(
        b.state.borrow().comment_reads,
        reads,
        "settled unchanged projections reuse comments"
    );
    b.raw(n, &format!("taskq: report-drop {sha}"), true, false);
    cache.collect(&b);
    assert!(cache.value["reports"].as_array().unwrap().is_empty());
}

// #1985: dev installs restart the service every few minutes; a restart must not re-read every closed task's comments.
#[test]
fn restart_reuses_settled_rows_without_comment_reads() {
    let (b, _, _, _) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    let reads = b.state.borrow().comment_reads;
    let mut restarted = Cache::retaining(cache.value.clone());
    b.advance(2);
    restarted.collect(&b);
    assert_eq!(
        b.state.borrow().comment_reads,
        reads,
        "restart resumes the cursor"
    );
    let mut legacy = cache.value.clone();
    legacy["verified"] = Value::Null;
    let mut restarted = Cache::retaining(legacy);
    b.advance(2);
    restarted.collect(&b);
    assert_eq!(
        b.state.borrow().comment_reads,
        reads,
        "a full listing still reuses rows with their read proof"
    );
    assert_eq!(restarted.value["complete"], true);
    assert!(restarted.value["reports"][0]["body"].is_string());
}

// #2087 A1: a decision whose issue update a lagging list left out must still leave Decide, before and after a restart,
// and a cache stored before the list-lag overlap must heal once. Risk: a dropped report stays in Decide for good.
#[test]
fn lagging_list_never_hides_a_decision_for_good() {
    // #2352: the full read a second after the decision lags it; the gap between full reads is longer than the lag.
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(FULL_SCAN - 1);
    cache.collect(&b);
    let list = b.state.borrow().issues.clone();
    b.raw(n, &format!("taskq: report-drop {sha}"), true, false);
    b.state.borrow_mut().stale_list = Some(list);
    b.advance(1);
    cache.collect(&b);
    assert_eq!(
        cache.value["reports"][0]["n"], n,
        "the lagging reply still shows the report"
    );
    b.advance(FULL_SCAN);
    cache.collect(&b);
    assert!(
        cache.value["reports"].as_array().unwrap().is_empty(),
        "the next full read overlaps the lag"
    );
    assert_eq!(cache.value["history"][0]["decision"], "drop");
    // The same miss just before a restart heals in the retained cache's first pass.
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(FULL_SCAN - 1);
    cache.collect(&b);
    let list = b.state.borrow().issues.clone();
    b.raw(n, &format!("taskq: report-drop {sha}"), true, false);
    b.state.borrow_mut().stale_list = Some(list);
    b.advance(1);
    cache.collect(&b);
    assert_eq!(cache.value["reports"][0]["n"], n);
    let mut restarted = Cache::retaining(cache.value.clone());
    b.advance(60);
    restarted.collect(&b);
    assert!(restarted.value["reports"].as_array().unwrap().is_empty());
    assert_eq!(restarted.value["history"][0]["decision"], "drop");
    // A cache stored before the overlap (no `overlap` key) relists once in full and drops the stale row.
    let mut old = cache.value.clone();
    old.as_object_mut().unwrap().remove("overlap");
    old["verified"] = json!(b.now().unwrap());
    let mut healed = Cache::retaining(old);
    healed.collect(&b);
    assert!(healed.value["reports"].as_array().unwrap().is_empty());
    assert_eq!(healed.value["overlap"], LIST_LAG);
}

// #2091 A2: a comment read soon after an accept can lag it; the cache then held the report at the issue's current update
// with a fresh-looking proof, so no later pass re-read it. Risk: an accepted report stays in Decide for good.
#[test]
fn lagging_comment_read_never_keeps_an_accepted_report() {
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    b.raw(n, &format!("taskq: report-accept {sha}"), true, false);
    let at = b.issue(n).unwrap().updated;
    // The stored cache as the PDS host (4.6.57) wrote it: the row carries the accept's update, read 66 s later without it.
    let mut stale = cache.value.clone();
    stale.as_object_mut().unwrap().remove("settle");
    stale["reports"][0]["updated"] = json!(at);
    stale["reports"][0]["proof"] = json!(at + 66);
    b.advance(600);
    stale["verified"] = json!(b.now().unwrap() - 60);
    let mut healed = Cache::retaining(stale);
    healed.collect(&b);
    assert!(
        healed.value["reports"].as_array().unwrap().is_empty(),
        "the stored lagging read heals without a reset"
    );
    assert_eq!(healed.value["history"][0]["decision"], "accepted");
    // A live read inside the lag window stays unsettled: the next pass that lists the issue reads its comments again.
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    b.raw(n, &format!("taskq: report-accept {sha}"), true, false);
    let at = b.issue(n).unwrap().updated;
    {
        let e = cache.entries.get_mut(&n).unwrap();
        e.0 = at;
        e.1 = at + 66;
    }
    b.advance(126);
    cache.collect(&b);
    assert!(
        cache.value["reports"].as_array().unwrap().is_empty(),
        "a read 60 s after the update is not settled proof"
    );
}

// #1973 A1: open tasks and host heartbeats move every minute but never project; they must cost no comment read.
#[test]
fn open_and_heartbeat_updates_skip_comment_reads() {
    // The second pass settles the closed fixture's same-second read before counting.
    let (b, _, _, _) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    let open = b
        .create("Live task", "", &["taskq".into(), "code".into()])
        .unwrap();
    let beat = b
        .create("Host heartbeat", "", &["taskq-host".into()])
        .unwrap();
    b.close(beat).unwrap();
    let reads = b.state.borrow().comment_reads;
    for _ in 0..3 {
        b.advance(2);
        let at = b.now().unwrap();
        for n in [open, beat] {
            b.state
                .borrow_mut()
                .issues
                .iter_mut()
                .find(|i| i.number == n)
                .unwrap()
                .updated = at;
        }
        cache.collect(&b);
        assert_eq!(cache.value["complete"], true);
    }
    assert_eq!(
        b.state.borrow().comment_reads,
        reads,
        "no comment read for issues that cannot project"
    );
}

// #1968 A4: a source missing at its result SHA is settled; it must not block the cursor or repeat reads.
#[test]
fn missing_report_source_settles_without_repeating_reads() {
    let (b, _, n, _) = board();
    let source = b.state.borrow_mut().report_sources.pop().unwrap();
    let mut cache = Cache::default();
    cache.collect(&b);
    assert_eq!(cache.value["complete"], true);
    assert!(
        cache.verified.is_some(),
        "a settled 404 advances the cursor"
    );
    assert!(
        cache.value["reports"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("HTTP 404")
    );
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    let reads = b.state.borrow().comment_reads;
    // A source read would now succeed; staying settled proves no pass fetches it again.
    b.state.borrow_mut().report_sources.push(source);
    b.advance(2);
    cache.collect(&b);
    assert_eq!(
        b.state.borrow().comment_reads,
        reads,
        "settled rows reuse comments"
    );
    assert_eq!(cache.value["reports"][0]["n"], n);
    assert!(cache.value["reports"][0]["body"].is_null());
}

// Chain: #2193 A1/A2 -> collect. Risk: one 403 report marks every result incomplete or shows the raw reply body.
#[test]
fn unreadable_report_source_keeps_its_row_and_others_complete() {
    let (b, _, n, sha) = board();
    let other = b.create("Plain task", "", &["taskq".into()]).unwrap();
    let spawn = b.raw(
        other,
        "taskq: spawn worker invented:session host",
        true,
        false,
    );
    b.raw(
        other,
        &format!("taskq: result {} attempt {spawn}", "c".repeat(40)),
        true,
        false,
    );
    b.raw(other, "taskq: close", true, false);
    b.close(other).unwrap();
    let body = r#"GET /repos/o/r/contents/docs/research/1.md?ref=x: HTTP 403 {"message":"Resource not accessible by personal access token","documentation_url":"https://docs.github.com/rest","status":"403"}"#;
    b.state.borrow_mut().source_error = Some(board::Error::http(403, body));
    let mut cache = Cache::default();
    cache.collect(&b);
    assert_eq!(cache.value["complete"], true, "{}", cache.value["reason"]);
    assert!(cache.value["reason"].is_null());
    assert!(
        cache.verified.is_none(),
        "the cursor holds so the read retries"
    );
    let rows = cache.value["reports"].as_array().unwrap();
    let row = rows.iter().find(|r| r["n"] == n).unwrap();
    let reason = row["reason"].as_str().unwrap();
    assert!(reason.contains("Contents read"), "{reason}");
    assert!(
        !reason.contains('{') && !reason.contains("personal access token"),
        "{reason}"
    );
    assert!(
        rows.iter()
            .any(|r| r["n"] == other && r["reason"].is_null()),
        "other rows stay complete"
    );
    // The token gains Contents read: the next pass heals the row.
    b.state.borrow_mut().source_error = None;
    b.advance(2);
    cache.collect(&b);
    let row = cache.value["reports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["n"] == n)
        .unwrap()
        .clone();
    assert!(row["reason"].is_null() && row["body"].is_string() && row["sha"] == sha.as_str());
    assert!(cache.verified.is_some());
    assert_eq!(
        source_reason(&board::Error::http(
            502,
            "GET x: HTTP 502 <html>bad gateway</html>"
        )),
        "Board read failed (HTTP 502); details in the service log"
    );
    assert_eq!(
        source_reason(&board::Error::from("report missing Question heading")),
        "report missing Question heading"
    );
    assert_eq!(
        source_reason(&board::Error::http(
            403,
            "GET x: HTTP 403 {\"message\":\"API rate limit exceeded\"}"
        )),
        "Board read failed (HTTP 403); details in the service log"
    );
}

#[test]
fn close_event_invalidates_collection_before_the_next_due_pass() {
    let (b, _, _, _) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    let root = std::env::temp_dir().join(format!("taskq-report-close-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(cache).unwrap();
    let mut collector = Collector {
        pending: Some(rx),
        started: Some(std::time::Instant::now()),
        ..Default::default()
    };
    assert_eq!(collector.tick(&root, 300, 300, false)["complete"], true);
    let rows = stored(&root, &Value::Null);
    assert!(
        rows["reports"][0]["body"].is_string(),
        "#1975: rows land in reports.json"
    );
    assert_eq!(summary(&rows)["count"], 1);
    assert!(
        summary(&rows).get("reports").is_none(),
        "status keeps counts, never rows"
    );
    std::fs::write(
        crate::paths::dot(&root).join("events"),
        "invented #12 close 3\n",
    )
    .unwrap();
    collector.tick(&root, 300, 300, false);
    assert!(
        collector.invalidated,
        "observe-only passes retain the requested refresh"
    );
    collector.tick(&root, 300, 300, true);
    assert!(
        collector.pending.is_some(),
        "close schedules without waiting for poll"
    );
    // No project config: the read-only worker must return unknown, never create or launch anything.
    let failed = collector
        .pending
        .take()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(failed.value["complete"], false);
    assert!(failed.value["reason"].is_string());
    assert!(
        !crate::paths::dot(&root).join("poke").exists(),
        "completion must not force a new board read"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cache_fresh_stale_missing_and_corrupt_are_distinct() {
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    let root = std::env::temp_dir().join(format!("taskq-report-cache-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    assert_eq!(cached(&root)["complete"], false);
    std::fs::write(crate::paths::dot(&root).join("status.json"), "corrupt").unwrap();
    assert!(cached(&root)["reason"].is_string());
    let at = cache.value["observed"].as_i64().unwrap();
    assert_eq!(
        freshness(cache.value.clone(), at + 300, 300, None)["complete"],
        true
    );
    let stale = freshness(cache.value.clone(), at + 301, 300, None);
    assert_eq!(stale["complete"], false);
    assert_eq!(stale["reports"][0]["n"], n);
    let failed = freshness(cache.value.clone(), at, 300, Some("Board unavailable"));
    assert_eq!(failed["reason"], "Board unavailable");
    std::fs::write(
        crate::paths::dot(&root).join("status.json"),
        json!({"reports":cache.value}).to_string(),
    )
    .unwrap();
    assert_eq!(cached_list(&root, 1, None).unwrap()["reports"][0]["n"], n);
    let history = cached_history(&root, &json!({"task":n})).unwrap();
    assert_eq!(history["source"], "service");
    assert_eq!(history["tasks"][0]["n"], n);
    assert!(cached_open(&root, n, &sha).unwrap()["report"]["body"].is_string());
    assert_eq!(
        cached_open(&root, n, &"c".repeat(40)).unwrap()["complete"],
        false
    );
    assert_eq!(
        open_latest(&root, n, None).unwrap()["report"]["sha"],
        sha,
        "steer #2256: no --sha opens the latest result"
    );
    let mut old = cached(&root);
    old["observed"] = json!(0);
    std::fs::write(
        crate::paths::dot(&root).join("status.json"),
        json!({"reports":old}).to_string(),
    )
    .unwrap();
    let stale_history = cached_history(&root, &json!({"task":n})).unwrap();
    assert_eq!(stale_history["complete"], false);
    assert_eq!(stale_history["tasks"][0]["n"], n);
    assert!(stale_history["reason"].is_string());
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: §7 Reports -> exact source -> §1 claims/held drafts -> readback. Risk: one click admits tasks,
// duplicate or uncertain writes duplicate drafts, failed creation discards a report, or Later changes order.
#[test]
fn report_actions_keep_unknowns_and_create_held_drafts_once() {
    let (b, claim, n, sha) = board();
    let raw = json!({"create_claim_issue":claim});
    let root = std::env::temp_dir().join(format!("taskq-reports-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let run = |action| owner_act_until(&b, &root, &raw, n, &sha, action, None).unwrap();
    assert_eq!(list(&b, 1).unwrap()["reports"][0]["read"], false);
    assert!(owner_act_until(&b, &root, &raw, n, &"b".repeat(40), "drop", None).is_err());
    let opened = run("open");
    assert_eq!(opened["report"]["read"], true);
    assert_eq!(opened["report"]["tasks"].as_array().unwrap().len(), 2);
    let count = b.comments(n).unwrap().len();
    run("open");
    assert_eq!(
        b.comments(n).unwrap().len(),
        count,
        "repeat open is read-only"
    );
    // #1979: a Later without a future revisit date is no decision and writes nothing.
    let count = b.comments(n).unwrap().len();
    let now = b.now().unwrap();
    assert!(
        owner_act_until(&b, &root, &raw, n, &sha, "later", None)
            .unwrap_err()
            .contains("--until")
    );
    assert!(
        owner_act_until(
            &b,
            &root,
            &raw,
            n,
            &sha,
            "later",
            Some(&crate::time::stamp(now))
        )
        .unwrap_err()
        .contains("future")
    );
    assert!(
        owner_act_until(
            &b,
            &root,
            &raw,
            n,
            &sha,
            "drop",
            Some(&crate::time::stamp(now + 60))
        )
        .is_err(),
        "--until only with Later"
    );
    assert_eq!(b.comments(n).unwrap().len(), count);
    let until = crate::time::stamp(now + 3600);
    assert_eq!(
        owner_act_until(&b, &root, &raw, n, &sha, "later", Some(&until)).unwrap()["report"]["later_until"],
        until.as_str()
    );
    assert_eq!(list(&b, 1).unwrap()["reports"][0]["closed"], 45);
    let missing = owner_act_until(&b, &root, &json!({}), n, &sha, "make", None).unwrap();
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("create_claim_issue")
    );
    assert_eq!(list(&b, 1).unwrap()["reports"].as_array().unwrap().len(), 1);
    let result = run("make");
    let made = result["report"]["tasks_made"].as_array().unwrap();
    assert_eq!(made.len(), 2);
    for child in made {
        let i = b.issue(child.as_u64().unwrap()).unwrap();
        assert!(i.open);
        assert!(i.labels.contains(&"q-held".into()));
        assert!(!i.labels.contains(&"taskq".into()));
        assert!(i.body.contains(&format!("Depends on #{n}")));
        assert_eq!(
            crate::core::fold(
                i.number,
                &crate::board::entries(&b, i.number).unwrap(),
                false
            )
            .state,
            crate::core::State::Held
        );
    }
    let issues = b.state.borrow().issues.len();
    run("make");
    assert_eq!(b.state.borrow().issues.len(), issues);
    assert!(
        list(&b, 1).unwrap()["reports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!b.issue(n).unwrap().open);
    assert_eq!(
        crate::core::fold(n, &crate::board::entries(&b, n).unwrap(), false).state,
        crate::core::State::Closed
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: #1979 R6 Decide -> collected reports -> status rows. Risk: an undecided experiment outcome leaves focus,
// a Later without a date or a past revisit date hides it for good, or a decided report stays listed.
#[test]
fn decide_lists_undecided_reports_oldest_first_and_returns_later_at_its_date() {
    let (i, comments) = fixture();
    let sha = "a".repeat(40);
    let marker = |id, text: String| Comment {
        id,
        body: text,
        trusted: true,
        ..Default::default()
    };
    let body = "# Question\nQ\n# Recommendation\n\nShip the cache behind a flag.\nMore detail.\n";
    let row = |n: u64, closed: i64, labels: &[&str], extra: &[Comment]| {
        let mut i = i.clone();
        i.number = n;
        i.labels.extend(labels.iter().map(|l| l.to_string()));
        let mut cs = comments.clone();
        cs.extend_from_slice(extra);
        let mut r = project(&i, closed, &cs, format!("invented/{n}")).unwrap();
        r["body"] = json!(body);
        r
    };
    let now = 10_000;
    let at = |t: i64| crate::time::stamp(t);
    let rows = vec![
        row(5, 300, &["verdict-refuted"], &[]),
        row(
            6,
            100,
            &[],
            &[marker(4, format!("taskq: report-read {sha}"))],
        ),
        row(
            7,
            50,
            &[],
            &[marker(4, format!("taskq: report-later {sha}"))],
        ),
        row(
            8,
            60,
            &[],
            &[marker(
                4,
                format!("taskq: report-later {sha}\nuntil {}", at(now + 60)),
            )],
        ),
        row(
            9,
            70,
            &[],
            &[marker(
                4,
                format!("taskq: report-later {sha}\nuntil {}", at(now - 60)),
            )],
        ),
        row(
            10,
            80,
            &[],
            &[marker(4, format!("taskq: report-drop {sha}"))],
        ),
        row(
            11,
            90,
            &[],
            &[marker(4, format!("taskq: report-making {sha}"))],
        ),
    ];
    let value = json!({"reports":rows.iter().filter(|r|r["visible"]==true).collect::<Vec<_>>(),"complete":true,"reason":null});
    let d = decide(&value, now);
    let ns: Vec<u64> = d["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["n"].as_u64().unwrap())
        .collect();
    assert_eq!(
        ns,
        [7, 9, 6, 5],
        "undated and past Later return; future Later, Drop and Make tasks decide; oldest close first"
    );
    assert_eq!(
        d["rows"][3]["verdict"], "refuted",
        "the board label reads the verdict back"
    );
    assert_eq!(
        d["rows"][0]["verdict"], "unknown",
        "legacy reports have no verdict"
    );
    assert_eq!(d["rows"][3]["age_s"], now - 300);
    assert_eq!(
        d["rows"][0]["recommendation"],
        "Ship the cache behind a flag."
    );
    let close = marker(
        3,
        "taskq: close\nChanged: c\nVerdict: inconclusive need a second host".into(),
    );
    let mut cs = comments.clone();
    cs[2] = close;
    assert_eq!(
        project(&i, 45, &cs, "invented".into()).unwrap()["experiment_verdict"],
        "inconclusive",
        "close line covers a label still in flight"
    );
    let stale = decide(
        &json!({"reports":value["reports"],"complete":false,"reason":"Report collection is stale; waiting for the service"}),
        now,
    );
    assert_eq!(stale["complete"], false);
    assert_eq!(
        crate::snapshot::decide_text(&stale).unwrap(),
        "Decide: unknown (Report collection is stale; waiting for the service)"
    );
    let text = crate::snapshot::decide_text(&d).unwrap();
    assert!(text.starts_with("Decide:\n| Report | Verdict | Age | Recommendation |\n|---|---|---|---|\n| [#7 ](invented/7) | unknown |"),"{text}");
    assert_eq!(
        crate::snapshot::decide_text(&decide(&json!({"reports":[],"complete":true}), now)),
        None,
        "nothing undecided prints nothing"
    );
}

// §1/R1: known creation refusal must leave the report undecided and Drop available.
#[test]
fn rejected_creation_configuration_never_reserves_the_report() {
    for invalid in [json!({}), json!({"create_claim_issue":2})] {
        let (b, _, n, sha) = board();
        let count = b.comments(n).unwrap().len();
        // a root without taskq.json: the missing key cannot be settled and written back
        let root = std::env::temp_dir().join(format!("taskq-reports-{}", crate::runtime::uuid()));
        let result = owner_act_until(&b, &root, &invalid, n, &sha, "make", None).unwrap();
        assert_eq!(result["receipt"]["outcome"], "rejected");
        assert_eq!(
            b.comments(n).unwrap().len(),
            count,
            "known refusal writes no make-intent"
        );
        assert_eq!(
            owner_act_until(&b, Path::new("."), &invalid, n, &sha, "drop", None).unwrap()["report"]
                ["decision"],
            "drop"
        );
    }
}

#[test]
fn unread_source_and_lost_writes_never_claim_success_or_repeat_create() {
    let (b, claim, n, sha) = board();
    let root = std::env::temp_dir().join(format!("taskq-reports-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let raw = json!({"create_claim_issue":claim});
    let source = b.state.borrow_mut().report_sources.pop().unwrap();
    assert!(owner_act_until(&b, &root, &raw, n, &sha, "open", None).is_err());
    assert_eq!(list(&b, 1).unwrap()["reports"][0]["read"], false);
    b.state.borrow_mut().report_sources.push(source);
    mark(&b, n, &sha, "making", &[]).unwrap();
    b.state.borrow_mut().lose_comment_response = true;
    let lost = owner_act_until(&b, &root, &raw, n, &sha, "make", None).unwrap();
    assert_eq!(lost["receipt"]["outcome"], "unknown");
    let retry = owner_act_until(&b, &root, &raw, n, &sha, "make", None).unwrap();
    assert_eq!(retry["receipt"]["outcome"], "unknown");
    assert_eq!(
        b.state.borrow().issues.len(),
        2,
        "an uncertain create claim never permits another create POST"
    );
    assert_eq!(
        list(&b, 1).unwrap()["reports"][0]["pending_decision"],
        "make"
    );
    let blocked = owner_act_until(&b, &root, &raw, n, &sha, "drop", None).unwrap();
    assert!(blocked["reason"].is_string());
    assert_eq!(list(&b, 1).unwrap()["reports"].as_array().unwrap().len(), 1);
    let (other, _, on, os) = board();
    other.state.borrow_mut().lose_comment_response = true;
    assert!(owner_act_until(&other, &root, &raw, on, &os, "drop", None).is_err());
    assert!(
        list(&other, 1).unwrap()["reports"]
            .as_array()
            .unwrap()
            .is_empty(),
        "refresh reconciles the committed drop"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: §7 -> source-bound intent -> partial creation/decision -> replay. Risk: concurrent Drop hides
// drafts, a failed final marker loses receipts, or a retry creates them twice.
#[test]
fn make_intent_fences_drop_and_preserves_receipts_after_final_marker_failure() {
    let (b, claim, n, sha) = board();
    let raw = json!({"create_claim_issue":claim});
    let root = std::env::temp_dir().join(format!("taskq-reports-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let prior = mark(&b, n, &sha, "making", &[]).unwrap();
    b.raw(n, &format!("taskq: report-drop {sha}"), true, false);
    assert_eq!(
        current(&b, n, &sha).unwrap()["visible"],
        true,
        "later Drop cannot win against the make intent"
    );
    let source = b.source(&format!("docs/research/{n}.md"), &sha).unwrap();
    let tasks = proposals(&source).unwrap();
    let mut made = vec![];
    for (k, t) in tasks.iter().enumerate() {
        let r = crate::cli::task_cmd::add_task(
            &b,
            &root,
            &raw,
            &t.task(n),
            &crate::Op {
                id: Some(format!("report.{n}.{sha}.{}", k + 1)),
                revision: None,
            },
        )
        .unwrap();
        assert!(r.ok());
        made.push(r.task);
    }
    b.state.borrow_mut().fail_comments = 1;
    let failed = finish(&b, n, &sha, &prior, &made);
    assert!(failed["reason"].is_string());
    assert_eq!(failed["receipts"], json!(made));
    assert_eq!(failed["report"]["visible"], true);
    let count = b.state.borrow().issues.len();
    let retried = owner_act_until(&b, &root, &raw, n, &sha, "make", None).unwrap();
    assert_eq!(retried["report"]["tasks_made"], json!(made));
    assert_eq!(b.state.borrow().issues.len(), count);
    let (other, _, on, os) = board();
    other.raw(on, &format!("taskq: report-drop {os}"), true, false);
    let result = owner_act_until(&other, &root, &raw, on, &os, "make", None).unwrap();
    assert_eq!(result["report"]["decision"], "drop");
    assert_eq!(
        other.state.borrow().issues.len(),
        2,
        "Drop first prevents creation"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: §7 -> stable close-time cursor -> decisions -> More. Risk: removing a page-one row skips an unread report.
#[test]
fn pagination_and_docs_discovery_survive_decisions_without_auxiliary_labels() {
    let (b, _, n, sha) = board();
    assert_eq!(
        list(&b, 1).unwrap()["reports"][0]["n"],
        n,
        "docs report lives in its accepted result, not a later label"
    );
    let mut expected = vec![n];
    for k in 0..11 {
        let next = b
            .create(
                "Invented research",
                "",
                &["taskq".into(), "research".into()],
            )
            .unwrap();
        let spawn = b.raw(next, "taskq: spawn worker invented:s host", true, false);
        b.raw(
            next,
            &format!("taskq: result {sha} attempt {spawn}"),
            true,
            false,
        );
        b.raw(next, "taskq: close", true, false);
        b.close(next).unwrap();
        b.state.borrow_mut().report_closed.insert(next, 100 + k);
        expected.push(next);
    }
    let first = list(&b, 1).unwrap();
    assert_eq!(first["more"], true);
    let rows = first["reports"].as_array().unwrap();
    let dropped = rows[0]["n"].as_u64().unwrap();
    mark(&b, dropped, &sha, "drop", &[]).unwrap();
    expected.retain(|n| *n != dropped);
    let cursor = first["next"].as_str().unwrap().split_once(':').unwrap();
    let rest = list_before(
        &b,
        2,
        Some((cursor.0.parse().unwrap(), cursor.1.parse().unwrap())),
    )
    .unwrap();
    let mut seen: Vec<_> = rows
        .iter()
        .filter_map(|r| r["n"].as_u64())
        .filter(|n| *n != dropped)
        .chain(
            rest["reports"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|r| r["n"].as_u64()),
        )
        .collect();
    seen.sort();
    expected.sort();
    assert_eq!(seen, expected);
}
fn list_before(b: &crate::board::Fake, page: usize, before: Option<(i64, u64)>) -> Res<Value> {
    let mut rows = Vec::new();
    for (i, closed) in b.research_tasks()? {
        if let Some(row) = project(&i, closed, &b.comments(i.number)?, b.url(i.number)) {
            let position = (
                row["closed"].as_i64().unwrap_or(0),
                row["n"].as_u64().unwrap_or(0),
            );
            if row["visible"] == true && before.is_none_or(|b| position < b) {
                rows.push(row);
            }
        }
    }
    rows.sort_by_key(|r| {
        (
            std::cmp::Reverse(r["closed"].as_i64().unwrap_or(0)),
            std::cmp::Reverse(r["n"].as_u64().unwrap_or(0)),
        )
    });
    let start = if before.is_some() {
        0
    } else {
        page.saturating_sub(1)
            .saturating_mul(crate::board::CLOSED_PAGE)
    };
    let selected: Vec<_> = rows
        .iter()
        .skip(start)
        .take(crate::board::CLOSED_PAGE)
        .collect();
    let next = selected
        .last()
        .map(|r| format!("{}:{}", r["closed"], r["n"]));
    Ok(
        json!({"reports":selected,"more":rows.len()>start.saturating_add(crate::board::CLOSED_PAGE),"next":next,"page":page}),
    )
}
fn list(b: &crate::board::Fake, page: usize) -> Res<Value> {
    list_before(b, page, None)
}

// R1/R6, #2439: task-list lag plus a newer task must not strand an accepted SHA until a full scan.
// Existing decision guards stay intact; this checks discovery through the collector and Decide.
#[test]
fn incremental_lag_reconciles_accepted_report_before_full_scan() {
    for restart in [false, true] {
        let (b, _, n, sha) = board();
        let mut cache = Cache::default();
        cache.collect(&b);
        b.advance(LIST_LAG + 1);
        cache.collect(&b); // settled initial row and full-scan clock
        b.advance(1);
        b.raw(n, &format!("taskq: report-accept {sha}"), true, false);
        let accepted = b.now().unwrap();
        b.advance(61);
        let newer = b.create("Newer task", "", &["taskq".into()]).unwrap();
        // A lagging list contains the newer task but omits the decision update entirely.
        let list = vec![b.issue(newer).unwrap()];
        b.state.borrow_mut().stale_list = Some(list);
        cache.collect(&b);
        assert_eq!(cache.value["reports"][0]["decision"], "none");
        assert_eq!(decide(&cache.value, b.now().unwrap())["rows"][0]["n"], n);
        if restart {
            cache = Cache::retaining(cache.value.clone());
        }
        b.advance(LIST_LAG);
        cache.collect(&b);
        assert!(b.now().unwrap() - accepted < FULL_SCAN);
        assert!(
            cache.value["reports"].as_array().unwrap().is_empty(),
            "accepted report stranded by incremental cursor (restart={restart})"
        );
        assert!(
            decide(&cache.value, b.now().unwrap())["rows"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(cache.value["history"][0]["sha"], sha);
        assert_eq!(cache.value["history"][0]["decision"], "accepted");
    }
}

// R1/R12: caches from before the incremental overlap must heal even after the decision left the window.
#[test]
fn pre_overlap_cache_rebuilds_an_already_stranded_accept() {
    let (b, _, n, sha) = board();
    let mut cache = Cache::default();
    cache.collect(&b);
    b.advance(LIST_LAG + 1);
    cache.collect(&b);
    b.raw(n, &format!("taskq: report-accept {sha}"), true, false);
    let writes = b.state.borrow().comments.len();
    b.advance(601);
    let mut stale = cache.value.clone();
    stale["verified"] = json!(b.now().unwrap());
    stale.as_object_mut().unwrap().remove("incremental_overlap");
    let mut restarted = Cache::retaining(stale);
    restarted.collect(&b);
    assert!(
        decide(&restarted.value, b.now().unwrap())["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(restarted.value["reports"].as_array().unwrap().is_empty());
    assert_eq!(restarted.value["history"][0]["decision"], "accepted");
    assert_eq!(
        b.state.borrow().comments.len(),
        writes,
        "healing never replays decisions"
    );
}
