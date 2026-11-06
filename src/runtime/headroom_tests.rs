use super::*;
// #2189 A3/A7: native observations affect the selected account, expire and never clear quota holds.
#[test]
fn weighted_headroom_native_observation() {
    let root = std::env::temp_dir().join(format!("taskq-headroom-{}", super::super::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let log = root.join("turn.jsonl");
    std::fs::write(&log,"{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":20,\"window_minutes\":300},\"secondary\":{\"used_percent\":90,\"window_minutes\":10080}}}}\n").unwrap();
    observe(&root, "codex", &log, 1000).unwrap();
    assert!((remaining(&root, "codex", 1000).unwrap().unwrap() - 0.1).abs() < 1e-9);
    let other = root.join("other.jsonl");
    std::fs::copy(&log, &other).unwrap();
    observe(&root, "codex", &other, 1001).unwrap();
    observe(&root, "codex", &log, 1200).unwrap();
    observe(&root, "codex", &other, 1201).unwrap();
    assert_eq!(
        remaining(&root, "codex", 1302).unwrap(),
        None,
        "same event must not refresh freshness"
    );
    assert_eq!(
        remaining(&root, "codex", 999).unwrap(),
        None,
        "future observations are unknown"
    );
    std::fs::write(&log,"{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"rateLimitType\":\"five_hour\",\"utilization\":0.75,\"resetsAt\":1400}}\n").unwrap();
    observe(&root, "claude", &log, 1300).unwrap();
    assert_eq!(remaining(&root, "claude", 1300).unwrap(), Some(0.25));
    assert_eq!(remaining(&root, "claude", 1400).unwrap(), None);
    super::super::accounts::record_windows(
        &root,
        "claude",
        "default",
        1300,
        &[("seven_day".into(), Some(7000))],
    )
    .unwrap();
    assert_eq!(
        remaining(&root, "claude", 1301).unwrap(),
        None,
        "a held account cannot supply headroom for a different launch account"
    );
    assert!(
        super::super::accounts::Host::read(&root)
            .active("claude", 1401)
            .is_none()
    );
    // #2263: a `codex exec --json` log has no rate limits; its thread rollout supplies them, else a named cause.
    let exec = root.join("exec.jsonl");
    let rollout = root.join("rollout-x-t1.jsonl");
    std::fs::write(
        &exec,
        "{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n",
    )
    .unwrap();
    std::fs::write(&rollout,"{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":68.0,\"window_minutes\":10080}}}}\n").unwrap();
    observe_in(&root, "codex", &exec, 1500, |_| None).unwrap();
    assert_eq!(remaining(&root, "codex", 1500).unwrap(), None);
    let expired = format!(
        "account default: last quota observation {} expired",
        crate::time::utc(1001)
    );
    assert_eq!(cause(&root, "codex", 1500), expired);
    assert_eq!(
        cause(&root, "codex", 1500 + 3600),
        expired,
        "the cause names a fixed time, so later passes keep an equal heartbeat body"
    );
    observe_in(&root, "codex", &exec, 1500, |t| {
        (t == "t1").then(|| rollout.clone())
    })
    .unwrap();
    assert!((remaining(&root, "codex", 1500).unwrap().unwrap() - 0.32).abs() < 1e-9);
    std::fs::write(root.join(FILE), "bad").unwrap();
    assert!(remaining(&root, "codex", 1400).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: taskq#2448 A1. Reading through AgentQ must keep the released .headroom marker: a changed source hash counts an
// old observation again. A window without a reading stays unknown; a repeated window length keeps both groups.
#[test]
fn agent_windows_keep_event_ids_unknowns_and_groups() {
    let root = std::env::temp_dir().join(format!("taskq-headroom-{}", super::super::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let log = root.join("turn.jsonl");
    std::fs::write(&log, "{\"type\":\"rate_limit_event\",\"uuid\":\"e1\",\"rate_limit_info\":{\"rateLimitType\":\"five_hour\",\"utilization\":0.5,\"resetsAt\":9000}}\n").unwrap();
    observe(&root, "claude", &log, 1000).unwrap();
    let seen: Value =
        serde_json::from_str(&std::fs::read_to_string(log.with_extension("headroom")).unwrap())
            .unwrap();
    assert_eq!(
        seen["five_hour"]["source"],
        "0d50ae30d7038ef2fdf8aa3bca59bcd498b0ab13b386c0752d95e726ad8f2d4d",
        "a released marker must keep matching"
    );
    std::fs::write(&log, "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"rateLimitType\":\"seven_day\",\"status\":\"allowed\"}}\n").unwrap();
    assert!(
        observations("claude", &super::super::events(&log, 0).unwrap()).is_empty(),
        "a window without a reading is unknown, never healthy"
    );
    std::fs::write(&log, "{\"rateLimitsByLimitId\":{\"a\":{\"primary\":{\"used_percent\":100,\"window_minutes\":300,\"resets_at\":9000}},\"b\":{\"primary\":{\"used_percent\":10,\"window_minutes\":300,\"resets_at\":9000}}}}\n").unwrap();
    let rows = observations("codex", &super::super::events(&log, 0).unwrap());
    assert_eq!(
        rows.keys().map(String::as_str).collect::<Vec<_>>(),
        ["300m", "300m-2"]
    );
    assert_eq!(
        rows.values().map(|r| r.1).fold(1.0, f64::min),
        0.0,
        "an exhausted group is not hidden by its sibling"
    );
    std::fs::remove_dir_all(root).unwrap();
}
