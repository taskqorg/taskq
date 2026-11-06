use super::*;

// #2137 A5/A7: journal replay survives loss of all process memory; a failed journal fences all project threads.
#[test]
fn balancing_quota_journal_restart_and_unwritable_storage() {
    let dir = std::env::temp_dir().join(format!("taskq-quota-journal-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let failed = dir.join(format!("{STATE}.{}.tmp", std::process::id()));
    std::fs::create_dir(&failed).unwrap();
    assert!(record_limit(&dir, "claude", "default", 10, Some(18000)).is_err());
    UNSAVED.lock().unwrap().remove(&dir); // Restart: only the on-disk journal survives.
    assert!(Host::read(&dir).error.is_some());
    std::fs::remove_dir(failed).unwrap();
    recover_pending(&dir).unwrap();
    assert_eq!(
        Host::read(&dir).state["claude"]["default"].until,
        Some(18000)
    );
    std::fs::create_dir(dir.join(PENDING)).unwrap();
    assert!(record_limit(&dir, "codex", "default", 10, Some(604800)).is_err());
    std::fs::remove_dir(dir.join(PENDING)).unwrap();
    assert!(
        Host::read(&dir).active("codex", 20).is_none(),
        "memory fence survives a journal write failure"
    );
    assert!(claim(&dir, "codex", None, 20, &dir.join("owner")).is_err());
    recover_pending(&dir).unwrap();
    assert_eq!(
        Host::read(&dir).state["codex"]["default"].until,
        Some(604800)
    );
    std::fs::remove_dir_all(dir).unwrap();
}

fn host(policy: &[(&str, &[(&str, bool)])], limits: &[(&str, &str, i64, Option<i64>)]) -> Host {
    let mut h = Host::default();
    for (rt, list) in policy {
        h.policy.insert(
            rt.to_string(),
            list.iter()
                .map(|(l, e)| Entry {
                    label: l.to_string(),
                    enabled: *e,
                })
                .collect(),
        );
    }
    for (rt, l, at, until) in limits {
        h.state.entry(rt.to_string()).or_default().insert(
            l.to_string(),
            Limit {
                at: *at,
                until: *until,
            },
        );
    }
    h
}

// #2137 A5-A8: one account probe across projects/restart, independent windows and unknown-state fencing.
#[test]
fn balancing_quota_windows_and_bounded_probes() {
    let dir = std::env::temp_dir().join(format!("taskq-quota-probes-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    record_windows(
        &dir,
        "claude",
        "default",
        0,
        &[
            ("five_hour".into(), Some(18000)),
            ("seven_day".into(), Some(604800)),
        ],
    )
    .unwrap();
    let h = Host::read(&dir);
    assert!(h.active("claude", 18001).is_none());
    assert!(
        h.heartbeat(18001)
            .iter()
            .any(|l| l.contains("account default window seven_day until"))
    );
    let (first, second) = (dir.join("attempt-one"), dir.join("attempt-two"));
    assert!(claim(&dir, "claude", None, 18001, &first).is_err());
    assert_eq!(
        claim(&dir, "claude", None, 604801, &first).unwrap(),
        "default"
    );
    assert!(
        claim(&dir, "claude", None, 604802, &second).is_err(),
        "restart must retain the claimed probe"
    );
    confirm_probe(&dir, "claude", "default", &second).unwrap();
    assert!(
        Host::read(&dir).active("claude", 604802).is_none(),
        "another execution cannot clear the probe"
    );
    confirm_probe(&dir, "claude", "default", &first).unwrap();
    assert_eq!(
        Host::read(&dir).active("claude", 604802).as_deref(),
        Some("default")
    );
    record_limit(&dir, "codex", "default", 100, None).unwrap();
    assert!(claim(&dir, "codex", None, 1899, &first).is_err());
    assert!(claim(&dir, "codex", None, 1900, &first).is_ok());
    assert!(claim(&dir, "codex", None, 1901, &second).is_err());
    std::fs::write(dir.join(STATE), "corrupt").unwrap();
    assert!(Host::read(&dir).error.is_some());
    assert!(
        claim(&dir, "codex", None, 999999, &first).is_err(),
        "unknown is never available"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

// #2137 A5/A7: a shorter quota window must not erase a longer hold across restart.
#[test]
fn balancing_overlapping_quota_survives_restart() {
    let dir = std::env::temp_dir().join(format!("taskq-quota-overlap-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    record_limit(&dir, "codex", "default", 0, Some(604800)).unwrap();
    record_limit(&dir, "codex", "default", 1, Some(18000)).unwrap();
    let h = Host::read(&dir);
    assert_eq!(
        h.active("codex", 18001),
        None,
        "a five-hour reset cannot clear a weekly window"
    );
    assert_eq!(h.hold("codex", 18001), Some((604800, true)));
    std::fs::remove_dir_all(dir).unwrap();
}

// Chain: §3 Runtime accounts (#2043 A3, A4): order and disable pick the start account; a limit moves to the next
// enabled account; with none left the hold ends at the earliest limit end. Risk: a disabled or limited account
// still takes new workers, or a host with a spare account holds the runtime.
#[test]
fn order_disable_failover_and_exhausted_hold() {
    let none = host(&[], &[]);
    assert_eq!(
        none.list("codex"),
        default_list(),
        "no list: one default account"
    );
    assert_eq!(
        (none.active("codex", 0), none.spare("codex", 0)),
        (Some("default".into()), None),
        "a lone account never fails over"
    );

    let h = host(
        &[(
            "codex",
            &[("default", true), ("work", true), ("spare", true)],
        )],
        &[],
    );
    assert_eq!(
        h.active("codex", 0).as_deref(),
        Some("default"),
        "list order is priority"
    );
    let h = host(
        &[(
            "codex",
            &[("default", false), ("work", true), ("spare", true)],
        )],
        &[],
    );
    assert_eq!(
        h.active("codex", 0).as_deref(),
        Some("work"),
        "a disabled account takes no start"
    );

    let h = host(
        &[(
            "codex",
            &[("default", true), ("work", true), ("spare", false)],
        )],
        &[("codex", "default", 100, None)],
    );
    assert_eq!(
        h.spare("codex", 200).as_deref(),
        Some("work"),
        "a limit moves to the next enabled account"
    );
    assert_eq!(h.hold("codex", 200), Some((100 + PROBE, false)));
    assert_eq!(
        h.spare("codex", 100 + PROBE).as_deref(),
        Some("default"),
        "the probe ends the limit and order returns"
    );

    let h = host(
        &[(
            "codex",
            &[("default", true), ("work", true), ("spare", false)],
        )],
        &[
            ("codex", "default", 100, Some(9000)),
            ("codex", "work", 150, None),
        ],
    );
    assert_eq!(
        h.spare("codex", 200),
        None,
        "every enabled account limited: no spare"
    );
    assert_eq!(
        h.hold("codex", 200),
        Some((150 + PROBE, false)),
        "the hold ends at the earliest enabled limit end"
    );
    assert_eq!(
        h.active("codex", 200),
        None,
        "an exhausted account cannot start"
    );
    assert!(!h.off("codex", 0));
    let h = host(&[("claude", &[("default", false)])], &[]);
    assert!(
        h.off("claude", 0) && h.active("claude", 0).is_none(),
        "all disabled: the runtime admits nothing"
    );
    assert!(
        !h.off("codex-app", 0) && h.spare("codex-app", 0).is_none(),
        "codex-app has no accounts"
    );
}

// Chain: §3 Runtime accounts (#2043 A2, A6): heartbeat lines carry labels and state, never an email; another host
// reads them back as rows; every change of the active account is one switch with its reason.
#[test]
fn heartbeat_rows_and_switches() {
    let h = host(
        &[("codex", &[("default", true), ("work", false)])],
        &[("codex", "default", 0, Some(60))],
    );
    let lines = h.heartbeat(10);
    assert_eq!(
        lines,
        [
            "account codex default on limited 1970-01-01T00:00:00Z until 1970-01-01T00:01:00Z",
            "account codex work off"
        ]
    );
    let rows = heartbeat_rows(
        "mac",
        &format!(
            "version 1\n{}\naccount codex work on active",
            lines.join("\n")
        ),
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(
        (
            rows[0].limited.as_deref(),
            rows[0].reset.as_deref(),
            rows[0].email.as_ref()
        ),
        (
            Some("1970-01-01T00:00:00Z"),
            Some("until 1970-01-01T00:01:00Z"),
            None
        )
    );
    assert!(rows[2].active && rows[2].enabled && !rows[1].enabled);
    assert!(
        heartbeat_rows("mac", "account codex-app default on\naccount codex Bad on").is_empty(),
        "foreign runtimes and labels are dropped"
    );

    let mut seen = BTreeMap::new();
    let mut h = host(&[("codex", &[("default", true), ("work", true)])], &[]);
    assert!(
        switches(&h, 0, &mut seen).is_empty(),
        "the first pass records without a switch"
    );
    h.state.entry("codex".into()).or_default().insert(
        "default".into(),
        Limit {
            at: 0,
            until: Some(100),
        },
    );
    let s = switches(&h, 10, &mut seen);
    assert_eq!(
        s.iter().map(Switch::line).collect::<Vec<_>>(),
        ["account codex default -> work (limit)"]
    );
    assert_eq!(
        s[0].counts("mac").keys().cloned().collect::<Vec<_>>(),
        [
            "from:default",
            "host:mac",
            "reason:limit",
            "runtime:codex",
            "to:work"
        ]
    );
    assert_eq!(
        switches(&h, 100, &mut seen)
            .iter()
            .map(Switch::line)
            .collect::<Vec<_>>(),
        ["account codex work -> default (reset)"]
    );
    h.policy.get_mut("codex").unwrap()[0].enabled = false;
    assert_eq!(
        switches(&h, 100, &mut seen)
            .iter()
            .map(Switch::line)
            .collect::<Vec<_>>(),
        ["account codex default -> work (policy)"]
    );
}

// Chain: §3 Runtime accounts (#2043 A3, A5): enable, disable and move edit the local list; add appends once; the
// attempt log names the account a resume keeps. Risk: a reorder drops an account or a resume changes login.
#[test]
fn list_writes_and_attempt_log_account() {
    let mut p = Policy::new();
    assert!(
        change(&mut p, "disable", "codex", "default", None)
            .unwrap()
            .contains("every account is disabled")
    );
    append(&mut p, "codex", "work").unwrap();
    assert!(
        append(&mut p, "codex", "work").is_err(),
        "a label is added once"
    );
    change(&mut p, "move", "codex", "work", Some(1)).unwrap();
    assert_eq!(
        p["codex"],
        [
            Entry {
                label: "work".into(),
                enabled: true
            },
            Entry {
                label: "default".into(),
                enabled: false
            }
        ]
    );
    assert!(
        change(&mut p, "move", "codex", "work", Some(3)).is_err()
            && change(&mut p, "enable", "codex", "gone", None).is_err()
    );
    assert_eq!(
        parse_policy(&policy_json(&p)).unwrap(),
        p,
        "the file round-trips"
    );
    assert!(parse_policy(&json!({"codex-app": [{"label": "default", "enabled": true}]})).is_err());
    assert!(
        parse_policy(
            &json!({"codex": [{"label": "a", "enabled": true}, {"label": "a", "enabled": false}]})
        )
        .is_err()
    );

    let dir = std::env::temp_dir().join(format!("taskq-accounts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("T1-2.log");
    std::fs::write(&log, "{\"type\":\"taskq.rules\"}\n").unwrap();
    assert_eq!(
        of_log(&log),
        DEFAULT,
        "an older log without the line uses default"
    );
    mark(&log, "work").unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"{\"type\":\"taskq.rules\",\"phase\":\"resume\"}\n")
        .unwrap();
    assert_eq!(of_log(&log), "work");
    assert!(home("codex", DEFAULT).is_none() && home("codex-app", "work").is_none());
    assert!(home("codex", "work").is_some_and(|h| h.ends_with(Path::new(HOMES).join("work"))));
    record_limit(&dir, "codex", "work", 5, None).unwrap();
    record_limit(&dir, "codex", "default", 6, Some(9)).unwrap();
    assert_eq!(
        Host::read(&dir).state["codex"]["work"],
        Limit { at: 5, until: None }
    );
    assert_eq!(Host::read(&dir).state["codex"]["default"].end(), 9);
    let _ = std::fs::remove_dir_all(&dir);
}

// #2153 A2: `accounts::add` for `codex` seeds `config.toml` into the new account home, and `active_in` resolves
// the active account home for setup/readiness checks.
#[test]
fn add_codex_seeds_config_toml_and_resolves_active_home() {
    let dir = std::env::temp_dir().join(format!("taskq-2153-accounts-{}", crate::runtime::uuid()));
    let install = dir.join("install");
    let base = dir.join("codex-home");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&base).unwrap();
    assert!(add_in(&install, "codex", "work", &base, |_, _| Ok(false)).is_err());
    assert!(
        !base.join(HOMES).join("work").join("config.toml").exists(),
        "failed login leaves home empty for retry"
    );

    let out = add_in(&install, "codex", "work", &base, |home, args| {
        assert_eq!(args, &["login", "--device-auth"]);
        std::fs::write(home.join("auth.json"), "{}").unwrap();
        Ok(true)
    })
    .unwrap();
    assert_eq!(out, "accounts: codex work added at 2 of 2, enabled");
    let work_cfg = base.join(HOMES).join("work").join("config.toml");
    assert!(
        std::fs::symlink_metadata(&work_cfg).is_ok_and(|m| m.is_file()),
        "codex add must seed a regular config.toml"
    );

    std::fs::write(base.join("config.toml"), "model_provider = \"agentq\"\n").unwrap();
    add_in(&install, "codex", "spare", &base, |home, _| {
        std::fs::write(home.join("auth.json"), "{}").unwrap();
        Ok(true)
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(base.join(HOMES).join("spare").join("config.toml")).unwrap(),
        "model_provider = \"agentq\"\n"
    );

    assert_eq!(
        active_in(&install, "codex", &base),
        base,
        "default is active first"
    );
    record_limit(&install, "codex", "default", 10, Some(9999999999)).unwrap();
    let work_home = base.join(HOMES).join("work");
    assert_eq!(
        active_in(&install, "codex", &base),
        work_home,
        "limit on default activates work home"
    );
    assert_eq!(
        active_in(&install, "codex", &work_home),
        work_home,
        "already-resolved account home is not nested"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
/// A usage limit with no named window, as a runtime reports it.
fn record_limit(
    install: &Path,
    rt: &str,
    label: &str,
    at: i64,
    until: Option<i64>,
) -> Result<(), String> {
    crate::runtime::accounts::record_windows(install, rt, label, at, &[("unknown".into(), until)])
}
impl Host {
    pub fn limit_end(&self, rt: &str, label: &str) -> Option<i64> {
        self.limit(rt, label).map(Limit::end)
    }
}
