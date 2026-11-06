use super::*;
use crate::runtime::accounts::{Host, claim, mark, of_log};

const NOW: i64 = 1_800_000_000;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("taskq-usage-{name}-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

// Chain: #2448 account usage -> credential read; a missing CLI on PATH cannot hide the adapter's actual result.
#[test]
fn usage_2448_does_not_require_a_cli_on_path() {
    let home = dir("no-cli");
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "runtime::usage::tests::usage_2448_no_cli_child",
            "--ignored",
            "--nocapture",
        ])
        .env("PATH", "")
        .env("CLAUDE_CONFIG_DIR", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
#[ignore = "private credential-free subprocess of usage_2448_does_not_require_a_cli_on_path"]
fn usage_2448_no_cli_child() {
    assert_eq!(std::env::var_os("PATH").unwrap(), "");
    assert_eq!(
        fetch("claude", "default", &mut Ids::default())
            .unknown
            .as_deref(),
        Some("credential-unreadable")
    );
}

fn list(install: &Path, labels: &[(&str, bool)]) {
    let rows: Vec<Value> = labels
        .iter()
        .map(|(l, e)| serde_json::json!({"label": l, "enabled": e}))
        .collect();
    std::fs::write(
        install.join(accounts::FILE),
        serde_json::json!({"codex": rows}).to_string(),
    )
    .unwrap();
}

/// `used` per label of the codex list; a label left out fails its poll.
fn poll(install: &Path, now: i64, used: &[(&str, u8)]) {
    poll_with(install, now, |rt, label| {
        if rt != "codex" {
            return Poll::failed("no login");
        }
        match used.iter().find(|(l, _)| *l == label) {
            Some((l, u)) => Poll {
                id: Some(fingerprint(&format!("{l}@example.com"))),
                windows: vec![Window {
                    name: "10080m".into(),
                    used: *u,
                    reset: Some(now + 3600),
                }],
                ..Default::default()
            },
            None => Poll {
                id: Some(fingerprint(&format!("{label}@example.com"))),
                ..Poll::failed("token expired")
            },
        }
    })
    .unwrap();
}

fn id(label: &str) -> String {
    fingerprint(&format!("{label}@example.com"))
}

fn agent_window(
    name: Option<&str>,
    used: Option<f64>,
    unknown: Option<&str>,
) -> agentq_runtime::agent::Window {
    agentq_runtime::agent::Window {
        window: name.map(String::from),
        used,
        resets_at: used.map(|_| (NOW + 3600) as u64),
        event_id: None,
        exhausted: None,
        unknown: unknown.map(String::from),
    }
}

/// What AgentQ reports for a Claude account with a weekly reading and a Fable scope.
fn claude_poll() -> Poll {
    poll_of(vec![
        agent_window(Some("seven_day"), Some(0.43), None),
        agent_window(Some("seven_day_fable"), Some(0.04), None),
    ])
}

/// The same account when AgentQ cannot read the Fable scope.
fn claude_partial() -> Poll {
    poll_of(vec![
        agent_window(Some("seven_day"), Some(0.43), None),
        agent_window(Some("seven_day_fable"), None, Some("evidence-unreadable")),
    ])
}

// Chain: capacity.md § Account usage and fleet policy. Replaces the native-shape tests of this file: AgentQ `limits.rs`
// owns those shapes. Risk: TaskQ loses a sibling reading, invents a percent, or reads a reason as a window. Protection:
// each kind of AgentQ window maps to one Poll field.
#[test]
fn poll_of_keeps_siblings_and_reasons() {
    let p = poll_of(vec![
        agent_window(Some("seven_day"), Some(0.43), None),
        agent_window(Some("seven_day_fable"), None, Some("evidence-unreadable")),
        agent_window(None, None, Some("limit-group-unreadable")),
    ]);
    assert_eq!(
        p.windows
            .iter()
            .map(|w| (w.name.as_str(), w.used))
            .collect::<Vec<_>>(),
        [("seven_day", 43)]
    );
    assert_eq!(
        p.unreadable,
        [
            "seven_day_fable: evidence-unreadable",
            "limit-group-unreadable"
        ]
    );
    assert_eq!(
        p.unknown, None,
        "a sibling failure is not a whole-response failure"
    );
    for (windows, reason) in [
        (vec![], "not-reported"),
        (
            vec![agent_window(None, None, Some("keychain-would-prompt"))],
            "keychain-would-prompt",
        ),
    ] {
        let p = poll_of(windows);
        assert_eq!(p.unknown.as_deref(), Some(reason));
        assert!(p.windows.is_empty() && p.unreadable.is_empty());
    }
    let p = poll_of(vec![
        agent_window(Some("300m"), Some(1.0), None),
        agent_window(Some("300m"), Some(0.1), None),
    ]);
    assert_eq!(
        p.windows
            .iter()
            .map(|w| (w.name.as_str(), w.used))
            .collect::<Vec<_>>(),
        [("300m", 100), ("300m-2", 10)],
        "a repeated length keeps both readings"
    );
}

// Chain: R12, #2359 A1/A2: structured Claude caps and consumer Gemini quotas reach every public heartbeat row.
// Risk: a partial native reply looks healthy while silently dropping a model's more restrictive window.
#[test]
fn model_windows_survive_poll_to_heartbeat() {
    let d = dir("model-windows");
    let p = poll_of(vec![
        agent_window(Some("gemini-5h"), Some(0.0), None),
        agent_window(Some("gemini-weekly"), Some(0.74), None),
    ]);
    poll_with(&d, NOW, |rt, _| match rt {
        "claude" => poll_of(vec![
            agent_window(Some("seven_day"), Some(0.34), None),
            agent_window(Some("seven_day_fable"), Some(0.02), None),
        ]),
        "antigravity" => p.clone(),
        _ => Poll::failed("no login"),
    })
    .unwrap();
    let rows: Vec<_> = Host::read(&d)
        .usage_lines(NOW)
        .iter()
        .filter_map(|l| Row::parse(l))
        .filter_map(|r| match r.usage {
            Usage::Window { window, at } => {
                Some((r.rt, window.name, window.used, window.reset, at))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "claude".into(),
                "seven_day".into(),
                34,
                Some(NOW + 3600),
                NOW
            ),
            (
                "claude".into(),
                "seven_day_fable".into(),
                2,
                Some(NOW + 3600),
                NOW
            ),
            (
                "antigravity".into(),
                "gemini-5h".into(),
                0,
                Some(NOW + 3600),
                NOW
            ),
            (
                "antigravity".into(),
                "gemini-weekly".into(),
                74,
                Some(NOW + 3600),
                NOW
            ),
        ]
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: R12: the native null surface must not hide readable weekly/Fable quotas.
#[test]
fn claude_native_null_surface_preserves_windows() {
    let now = NOW;
    let parsed = claude_poll();
    assert!(parsed.unreadable.is_empty());
    assert_eq!(
        parsed
            .windows
            .iter()
            .map(|w| (w.name.as_str(), w.used))
            .collect::<Vec<_>>(),
        [("seven_day", 43), ("seven_day_fable", 4)]
    );
    assert!(
        parsed.windows.iter().all(|w| w.reset.is_some()),
        "native resets survive"
    );
    let d = dir("claude-native");
    poll_with(&d, now, |rt, _| {
        if rt == "claude" {
            parsed.clone()
        } else {
            Poll::failed("no login")
        }
    })
    .unwrap();
    let lines = View::read(&d).lines("claude", "default", true, now);
    assert_eq!(lines.len(), 2);
    for (line, window) in lines.iter().zip(&parsed.windows) {
        assert_eq!(
            Row::parse(line).unwrap().usage,
            Usage::Window {
                window: window.clone(),
                at: now
            }
        );
        println!("{line}");
    }
    assert_eq!(View::read(&d).remaining("claude", "default", now), Some(57));
    assert!(
        (super::super::headroom::remaining(&d, "claude", now)
            .unwrap()
            .unwrap()
            - 0.57)
            .abs()
            < 1e-12
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: R12: mixed quota validity preserves readable rows without turning incomplete headroom healthy.
#[test]
fn claude_partial_windows_keep_unknown_through_publication_and_headroom() {
    let now = NOW;
    let d = dir("claude-partial");
    let mut parsed = claude_partial();
    parsed.id = Some(id("default"));
    assert_eq!(
        parsed
            .windows
            .iter()
            .map(|w| (w.name.as_str(), w.used))
            .collect::<Vec<_>>(),
        [("seven_day", 43)],
        "an unreadable structured quota invalidates the same legacy window"
    );
    assert_eq!(parsed.unreadable, ["seven_day_fable: evidence-unreadable"]);
    let save = |p: &Poll| {
        poll_with(&d, now, |rt, _| {
            if rt == "claude" {
                p.clone()
            } else {
                Poll::failed("no login")
            }
        })
        .unwrap()
    };
    save(&parsed);
    let view = View::read(&d);
    let lines = view.lines("claude", "default", true, now);
    assert_eq!(
        lines.len(),
        2,
        "both readable and unknown rows survive persistence"
    );
    assert!(lines[0].contains("window seven_day used 43"));
    assert!(lines[1].ends_with("unknown quota unreadable seven_day_fable: evidence-unreadable"));
    for line in &lines {
        assert_eq!(Row::parse(line).unwrap().line(), *line);
    }
    assert_eq!(view.remaining("claude", "default", now), None);
    assert_eq!(
        super::super::headroom::remaining(&d, "claude", now).unwrap(),
        None
    );
    assert!(super::super::headroom::cause(&d, "claude", now).contains("unreadable quota windows"));

    // No readable window left still masks a previous saved observation; no zero is synthesized.
    let empty = poll_of(vec![agent_window(
        Some("seven_day"),
        None,
        Some("evidence-unreadable"),
    )]);
    save(&empty);
    assert_eq!(
        View::read(&d).lines("claude", "default", true, now).len(),
        1
    );
    assert_eq!(
        super::super::headroom::remaining(&d, "claude", now).unwrap(),
        None
    );

    let mut complete = claude_poll();
    complete.id = parsed.id.clone();
    save(&complete);
    assert_eq!(
        View::read(&d).remaining("claude", "default", now),
        Some(57),
        "a complete poll recovers headroom"
    );
    fleet(&d, now, Policy::new(), vec![lines[1].clone()], None);
    assert_eq!(
        View::read(&d).remaining("claude", "default", now),
        None,
        "a matching fleet account's unknown also survives"
    );
    assert_eq!(
        super::super::headroom::remaining(&d, "claude", now).unwrap(),
        None
    );
    fleet(
        &d,
        now,
        Policy::new(),
        vec![lines[1].replace(&id("default"), &id("other"))],
        None,
    );
    assert_eq!(
        View::read(&d).remaining("claude", "default", now),
        Some(57),
        "unknown never crosses account identity"
    );
    fleet(
        &d,
        now - FLEET_TTL - 1,
        Policy::new(),
        vec![lines[1].clone()],
        None,
    );
    assert_eq!(
        View::read(&d).remaining("claude", "default", now),
        Some(57),
        "stale fleet evidence expires"
    );

    for reason in [
        "network failed",
        "call refused 401",
        "token expired",
        "usage shape changed",
    ] {
        save(&Poll::failed(reason));
        assert_eq!(
            View::read(&d).lines("claude", "default", true, now),
            [Row {
                rt: "claude".into(),
                label: "default".into(),
                id: parsed.id.clone(),
                usage: Usage::Unknown(reason.into())
            }
            .line()]
        );
    }
    std::fs::remove_dir_all(d).unwrap();
}

/// The account reader of v4.6.79 and earlier: its `Row::parse` grammar and `View::remaining` fleet aggregation, kept
/// here because a released binary cannot change. It ignores `unknown` rows.
fn n1_remaining(rows: &[String], want: &str, now: i64) -> Option<u8> {
    let mut windows: BTreeMap<String, (i64, u8)> = BTreeMap::new();
    for line in rows {
        let Some(w) = line.strip_prefix("limit ") else {
            continue;
        };
        let w: Vec<&str> = w.split(' ').collect();
        let [
            rt,
            _,
            "id",
            row_id,
            "window",
            name,
            "used",
            used,
            "reset",
            reset,
            "at",
            at,
        ] = w.as_slice()
        else {
            continue;
        };
        let (Some(at), Ok(used)) = (crate::time::iso(at), used.parse::<u8>()) else {
            continue;
        };
        let reset = if *reset == "-" {
            None
        } else {
            crate::time::iso(reset)
        };
        if *rt == "claude"
            && *row_id == want
            && fresh(at, reset, now)
            && windows.get(*name).is_none_or(|(old, _)| *old < at)
        {
            windows.insert(name.to_string(), (at, 100 - used));
        }
    }
    windows.values().map(|(_, left)| *left).min()
}

/// The rows a v4.6.79 binary rolled back onto this install publishes from `account-usage.json`: it never read fields
/// added later, so it only knows `windows` and `unknown`.
fn n1_rollback_rows(install: &Path, id: &str, at: i64) -> Vec<String> {
    #[derive(Deserialize)]
    struct N1Record {
        windows: Vec<Window>,
        unknown: Option<String>,
    }
    let store: BTreeMap<String, BTreeMap<String, N1Record>> =
        serde_json::from_str(&std::fs::read_to_string(install.join(FILE)).unwrap()).unwrap();
    let r = &store["claude"]["default"];
    match &r.unknown {
        Some(reason) => vec![format!("limit claude default id {id} unknown {reason}")],
        None => r
            .windows
            .iter()
            .map(|w| {
                Row {
                    rt: "claude".into(),
                    label: "default".into(),
                    id: Some(id.into()),
                    usage: Usage::Window {
                        window: w.clone(),
                        at,
                    },
                }
                .line()
            })
            .collect(),
    }
}

// Chain: R12, capacity.md Heartbeat: a released reader ignores `unknown` rows, so weekly 43 beside an unreadable Fable
// would read as headroom 57 on a mixed fleet or after a rollback; only a complete poll may reach it.
#[test]
fn claude_partial_poll_is_invisible_to_released_readers() {
    let now = NOW;
    let d = dir("claude-n1");
    let account = id("default");
    let mut partial = claude_partial();
    partial.id = Some(account.clone());
    assert!(
        !partial.unreadable.is_empty() && !partial.windows.is_empty(),
        "the fixture is a partial poll"
    );
    let save = |p: &Poll| {
        poll_with(&d, now, |rt, _| {
            if rt == "claude" {
                p.clone()
            } else {
                Poll::failed("no login")
            }
        })
        .unwrap()
    };
    save(&partial);
    let lines = View::read(&d).lines("claude", "default", true, now);
    assert_eq!(
        n1_remaining(&lines, &account, now),
        None,
        "mixed fleet: an old reader cannot derive headroom from a partial poll"
    );
    assert!(
        lines
            .iter()
            .all(|l| l.contains(" unknown ") || !l.starts_with("limit ")),
        "an old status shows no garbled hold row: {lines:?}"
    );
    assert_eq!(
        n1_remaining(&n1_rollback_rows(&d, &account, now), &account, now),
        None,
        "rollback: the old binary republishes nothing from the partial store"
    );
    assert!(
        View::read(&d).observed.get("claude").is_none(),
        "rollback: no partial observation is left for the old binary's headroom"
    );
    let row = Row::parse(&lines[0]).unwrap();
    assert_eq!(
        (row.id.as_deref(), &row.usage),
        (
            Some(account.as_str()),
            &Usage::Partial {
                window: partial.windows[0].clone(),
                at: now
            }
        ),
        "this reader keeps the window, its identity and its time"
    );
    assert_eq!(
        row.text(),
        format!(
            "seven_day used 43% reset {}",
            crate::time::utc(partial.windows[0].reset.unwrap())
        )
    );
    assert_eq!(row.line(), lines[0]);
    assert_eq!(
        View::read(&d)
            .lines("claude", "default", true, now + TTL + 1)
            .len(),
        1,
        "a partial row expires with its poll"
    );

    let mut complete = claude_poll();
    complete.id = Some(account.clone());
    save(&complete);
    let lines = View::read(&d).lines("claude", "default", true, now);
    assert_eq!(
        n1_remaining(&lines, &account, now),
        Some(57),
        "a complete poll keeps the released row grammar"
    );
    assert_eq!(
        n1_remaining(&lines, &id("other"), now),
        None,
        "identity still separates accounts"
    );
    assert_eq!(
        n1_remaining(&lines, &account, now + TTL + 1),
        None,
        "an old reader's expiry still applies"
    );
    assert_eq!(View::read(&d).remaining("claude", "default", now), Some(57));
    let shared = Row {
        rt: "claude".into(),
        label: "x".into(),
        id: Some(account.clone()),
        usage: Usage::Partial {
            window: complete.windows[0].clone(),
            at: now,
        },
    }
    .line();
    fleet(&d, now, Policy::new(), vec![shared.clone()], None);
    assert_eq!(
        View::read(&d).remaining("claude", "default", now),
        None,
        "a partial row alone marks the shared account incomplete"
    );
    fleet(
        &d,
        now,
        Policy::new(),
        vec![shared.replace(&account, &id("other"))],
        None,
    );
    assert_eq!(
        View::read(&d).remaining("claude", "default", now),
        Some(57),
        "another account's partial row changes nothing here"
    );
    for not_usage in [
        format!("limit-partial claude default id {account} unknown quota unreadable x"),
        "limit-partial claude until 2026-01-01T00:00:00Z".to_string(),
    ] {
        assert_eq!(Row::parse(&not_usage), None, "{not_usage}");
    }
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: R12: transport failures cannot turn a previous partial poll into healthy aggregate headroom.
#[test]
fn claude_failed_polls_preserve_incomplete_origin_until_expiry() {
    // Before the log file time: an observation takes the older of the clock and the file time.
    let now = 1_700_000_000;
    let d = dir("claude-failed-partial");
    let partial = claude_partial();
    poll_with(&d, now, |rt, _| {
        if rt == "claude" {
            partial.clone()
        } else {
            Poll::failed("no login")
        }
    })
    .unwrap();
    for (index, reason) in [
        "network failed",
        "call refused 401",
        "token expired",
        "usage shape changed",
    ]
    .iter()
    .enumerate()
    {
        let at = now + 120 + index as i64;
        poll_with(&d, at, |rt, _| {
            Poll::failed(if rt == "claude" { reason } else { "no login" })
        })
        .unwrap();
        assert_eq!(
            View::read(&d).remaining("claude", "default", at),
            None,
            "{reason} cannot restore partial headroom"
        );
        assert_eq!(
            super::super::headroom::remaining(&d, "claude", at).unwrap(),
            None
        );
        let lines = View::read(&d).lines("claude", "default", true, at);
        assert_eq!(
            lines.len(),
            2,
            "whole-response failure keeps the still-fresh window unknown for fleet readers"
        );
        assert!(lines.iter().any(|l| l.ends_with(reason)));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("unknown quota unreadable seven_day_fable"))
        );
    }
    let log = d.join("turn.jsonl");
    std::fs::write(&log, "{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"rateLimitType\":\"five_hour\",\"utilization\":0.25}}\n").unwrap();
    let at = now + TTL + 1;
    super::super::headroom::observe(&d, "claude", &log, at).unwrap();
    assert_eq!(
        View::read(&d).remaining("claude", "default", at),
        Some(75),
        "failures do not renew the original incomplete observation's expiry"
    );
    assert_eq!(
        super::super::headroom::remaining(&d, "claude", at).unwrap(),
        Some(0.75)
    );
    assert_eq!(
        View::read(&d).lines("claude", "default", true, at).len(),
        1,
        "expired per-window unknowns stop travelling to the fleet"
    );
    let complete = claude_poll();
    poll_with(&d, at + 1, |rt, _| {
        if rt == "claude" {
            complete.clone()
        } else {
            Poll::failed("no login")
        }
    })
    .unwrap();
    assert_eq!(
        View::read(&d).remaining("claude", "default", at + 1),
        Some(57),
        "a complete poll recovers all quotas"
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: capacity.md: Antigravity keeps one default account per host until a per-account `agy` home is qualified. Risk:
// a second label or a missing native status reaches the launch path, or a poll row lists a token. Protection: every
// entry point keeps `default`, a fleet `use` of another id is unmet, and rows show only the polled facts.
#[test]
fn antigravity_has_one_default_account_with_rows_and_policy() {
    let rt = "antigravity";
    let d = dir("one-account");
    assert!(
        accounts::add(&d, rt, "work")
            .unwrap_err()
            .contains("one `default` account"),
        "add refuses before any login"
    );
    std::fs::write(d.join(accounts::FILE), serde_json::json!({rt: [{"label": "default", "enabled": true}, {"label": "work", "enabled": true}]}).to_string()).unwrap();
    assert!(
        accounts::read_policy(&d)
            .unwrap_err()
            .contains("one `default` account"),
        "a hand-edited list stays refused"
    );
    std::fs::remove_file(d.join(accounts::FILE)).unwrap();
    assert_eq!(
        accounts::environment(rt, Path::new("elsewhere")),
        None,
        "no empty variable name reaches a child"
    );
    assert_eq!(
        super::super::account_email(rt, accounts::DEFAULT),
        None,
        "the bare binary is never run as a status probe"
    );
    assert_eq!(accounts::home(rt, accounts::DEFAULT), None);

    let at = |line: &str| line.starts_with(&format!("limit {rt} "));
    let poll = |id: Option<String>, used: u8| {
        poll_with(&d, NOW, |r, label| {
            if r != rt {
                return Poll::failed("no login");
            }
            assert_eq!(label, accounts::DEFAULT);
            Poll {
                id: id.clone(),
                windows: vec![Window {
                    name: "gemini-5h".into(),
                    used,
                    reset: None,
                }],
                ..Default::default()
            }
        })
        .unwrap()
    };
    poll(Some(fingerprint("project-1")), 30);
    let lines = Host::read(&d).usage_lines(NOW);
    let row = format!(
        "limit {rt} default id {} window gemini-5h used 30 reset - at {}",
        fingerprint("project-1"),
        crate::time::stamp(NOW)
    );
    assert_eq!(
        lines.iter().filter(|l| at(l)).cloned().collect::<Vec<_>>(),
        [row]
    );
    assert_eq!(
        Host::read(&d).active(rt, NOW).as_deref(),
        Some(accounts::DEFAULT)
    );
    let rows = Host::read(&d).rows("h", NOW, |_, _| None);
    let own = rows
        .iter()
        .find(|r| r.runtime == rt && r.label == accounts::DEFAULT)
        .unwrap();
    assert_eq!(
        own.id.as_deref(),
        Some(fingerprint("project-1").as_str()),
        "`taskq accounts` shows the polled id"
    );

    let policy = |preferred: Option<String>, avoid: &[String]| {
        BTreeMap::from([(
            rt.to_string(),
            RuntimePolicy {
                preferred,
                avoid: avoid.iter().cloned().collect(),
            },
        )])
    };
    fleet(&d, NOW, policy(Some(id("elsewhere")), &[]), vec![], None);
    assert!(
        Host::read(&d)
            .usage_lines(NOW)
            .contains(&format!("account-policy unmet {rt} {}", id("elsewhere"))),
        "another account has no home here"
    );
    assert_eq!(
        Host::read(&d).active(rt, NOW).as_deref(),
        Some(accounts::DEFAULT),
        "unmet keeps the default"
    );
    fleet(
        &d,
        NOW,
        policy(None, &[fingerprint("project-1")]),
        vec![],
        None,
    );
    assert!(
        Host::read(&d).off(rt, NOW),
        "an avoided only account starts no new work"
    );
}

fn fleet(install: &Path, at: i64, policy: Policy, rows: Vec<String>, error: Option<&str>) {
    std::fs::write(
        install.join(FLEET),
        serde_json::to_string(&Fleet {
            at,
            policy,
            rows,
            error: error.map(String::from),
        })
        .unwrap(),
    )
    .unwrap();
}

fn comment(id: u64, body: &str, trusted: bool, edited: bool) -> Comment {
    Comment {
        id,
        created: 0,
        body: body.into(),
        trusted,
        edited,
        author: String::new(),
        read: None,
    }
}

// Chain: §3 Runtime accounts (#2341 A2): rows carry a fingerprint and fixed reasons, never an email or a token; a
// poll never clears a hold. Risk: a public heartbeat leaks a login, or a good poll restarts a limited account.
#[test]
fn rows_carry_no_secret_and_a_poll_never_clears_a_hold() {
    let d = dir("rows");
    list(&d, &[("default", true), ("work", true), ("old", false)]);
    record_limit(&d, "codex", "work", NOW - 10, Some(NOW + 9000)).unwrap();
    let held = std::fs::read(d.join(accounts::STATE)).unwrap();
    poll(&d, NOW, &[("work", 1)]);
    assert_eq!(
        std::fs::read(d.join(accounts::STATE)).unwrap(),
        held,
        "a poll writes no quota record"
    );
    let h = Host::read(&d);
    assert_eq!(h.limit_end("codex", "work"), Some(NOW + 9000));
    assert_ne!(
        h.active("codex", NOW).as_deref(),
        Some("work"),
        "the best usage cannot lift a hold"
    );

    let lines = h.usage_lines(NOW);
    let text = lines.join("\n");
    assert!(
        !text.contains('@') && !text.contains("example.com") && !text.contains("sk-"),
        "{text}"
    );
    let stamp = crate::time::stamp;
    assert!(
        lines.contains(&format!(
            "limit codex work id {} window 10080m used 1 reset {} at {}",
            id("work"),
            stamp(NOW + 3600),
            stamp(NOW)
        )),
        "{text}"
    );
    assert!(
        lines.contains(&format!(
            "limit codex default id {} unknown token expired",
            id("default")
        )),
        "{text}"
    );
    assert!(
        lines.contains(&"limit codex old id - unknown disabled".to_string()),
        "{text}"
    );
    assert!(
        lines.contains(&"limit claude default id - unknown no login".to_string()),
        "{text}"
    );
    for l in lines.iter().filter(|l| l.starts_with("limit ")) {
        assert_eq!(Row::parse(l).unwrap().line(), *l, "a row round-trips");
    }
    assert_eq!(
        Row::parse("limit codex until 2026-11-04T10:00:00Z"),
        None,
        "the hold line is no usage row"
    );
    assert_eq!(Row::parse("limit codex probe 2026-11-04T10:00:00Z"), None);
    assert!(
        Host::read(&d)
            .usage_lines(NOW + TTL + 1)
            .iter()
            .any(|l| l.ends_with("unknown poll stale")),
        "an old poll is unknown, never healthy"
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: §3 Runtime accounts (#2341 A3): usage ranks new starts, the floor orders and never holds, and a resume keeps
// its log's account. Risk: a low account refuses work, or a resume changes login mid-attempt.
#[test]
fn ranking_groups_floor_and_kept_accounts() {
    let d = dir("rank");
    list(&d, &[("default", true), ("work", true), ("spare", true)]);
    let active = |now: i64| Host::read(&d).active("codex", now);
    assert_eq!(
        active(NOW).as_deref(),
        Some("default"),
        "no usage: list order"
    );

    poll(&d, NOW, &[("default", 90), ("work", 40), ("spare", 40)]);
    assert_eq!(
        active(NOW).as_deref(),
        Some("work"),
        "greatest remaining above the floor; a tie keeps list order"
    );

    poll(&d, NOW, &[("default", 90), ("work", 90)]);
    assert_eq!(
        Host::read(&d).usage.remaining("codex", "spare", NOW),
        Some(60),
        "a failed poll keeps the reading until it expires"
    );
    std::fs::remove_file(d.join(super::super::headroom::FILE)).unwrap();
    poll(&d, NOW, &[("default", 90), ("work", 90)]);
    assert_eq!(
        active(NOW).as_deref(),
        Some("spare"),
        "an unknown account beats one below the floor"
    );

    poll(&d, NOW, &[("default", 95), ("work", 90), ("spare", 100)]);
    assert_eq!(
        active(NOW).as_deref(),
        Some("work"),
        "every account below the floor still takes work: greatest first"
    );
    std::fs::write(d.join("host.json"), r#"{"account_floor":5}"#).unwrap();
    assert_eq!(active(NOW).as_deref(), Some("work"));
    std::fs::write(d.join("host.json"), r#"{"account_floor":12}"#).unwrap();
    assert_eq!(
        Host::read(&d).usage.remaining("codex", "work", NOW),
        Some(10)
    );
    assert_eq!(
        active(NOW).as_deref(),
        Some("work"),
        "the floor from host.json orders, it never refuses"
    );
    assert_eq!(
        active(NOW + TTL + 1).as_deref(),
        Some("default"),
        "expired usage is unknown: list order"
    );

    // A new start takes the ranked account, a resume keeps its log's account even when ranked last.
    let log = d.join("T1-2.log");
    std::fs::write(&log, "{\"type\":\"taskq.rules\"}\n").unwrap();
    let start = claim(&d, "codex", None, NOW, &log).unwrap();
    assert_eq!(start, "work");
    mark(&log, "spare").unwrap();
    assert_eq!(
        claim(&d, "codex", Some(of_log(&log).as_str()), NOW, &log).unwrap(),
        "spare",
        "resume and recovery keep the log's account"
    );
    std::fs::write(
        d.join(FLEET),
        serde_json::to_string(&Fleet {
            at: NOW,
            policy: BTreeMap::from([(
                "codex".to_string(),
                RuntimePolicy {
                    preferred: None,
                    avoid: BTreeSet::from([id("spare")]),
                },
            )]),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        claim(&d, "codex", Some("spare"), NOW, &log).unwrap(),
        "spare",
        "policy moves new starts only, never a running child"
    );

    // A limited account is skipped whatever its usage; with every account limited the hold still stops the runtime.
    list(&d, &[("work", true)]);
    std::fs::remove_file(d.join(FLEET)).unwrap();
    record_limit(&d, "codex", "work", NOW, Some(NOW + 99)).unwrap();
    assert_eq!(active(NOW), None);
    assert_eq!(Host::read(&d).hold("codex", NOW), Some((NOW + 99, true)));
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: §3 Runtime accounts (#2341 A3, A5): a trusted heartbeat row with the same id ranks an account down on the
// other hosts; no id, no merge. Risk: a label match merges two different logins.
#[test]
fn other_hosts_rows_merge_by_id_only() {
    let d = dir("merge");
    list(&d, &[("default", true), ("work", true)]);
    poll(&d, NOW, &[("default", 10), ("work", 30)]);
    let other = |who: &str, used: u8, at: i64| {
        Row {
            rt: "codex".into(),
            label: "x".into(),
            id: Some(id(who)),
            usage: Usage::Window {
                window: Window {
                    name: "300m".into(),
                    used,
                    reset: None,
                },
                at,
            },
        }
        .line()
    };
    fleet(
        &d,
        NOW,
        Policy::new(),
        vec![other("default", 95, NOW - 10), other("nobody", 99, NOW)],
        None,
    );
    let h = Host::read(&d);
    assert_eq!(
        h.usage.remaining("codex", "default", NOW),
        Some(5),
        "the same id on another host counts"
    );
    assert_eq!(
        h.usage.remaining("codex", "work", NOW),
        Some(70),
        "another id does not"
    );
    assert_eq!(h.active("codex", NOW).as_deref(), Some("work"));
    fleet(
        &d,
        NOW,
        Policy::new(),
        vec![other("default", 95, NOW - TTL - 1)],
        None,
    );
    assert_eq!(
        Host::read(&d).active("codex", NOW).as_deref(),
        Some("default"),
        "an old row counts for nothing"
    );
    fleet(
        &d,
        NOW - FLEET_TTL - 1,
        Policy::new(),
        vec![other("default", 95, NOW)],
        None,
    );
    assert_eq!(
        Host::read(&d).active("codex", NOW).as_deref(),
        Some("default"),
        "a stale file counts for nothing"
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: §3 Runtime accounts (#2341 A4): policy folds from trusted unedited comments in id order and every write is
// refused under TASKQ_ROLE. Risk: a task comment or a worker redirects every host's login.
#[test]
fn fleet_fold_write_and_refusal() {
    let (a, b) = ("aaaaaaaa", "bbbbbbbb");
    let p = fold(&[
        comment(5, &format!("taskq: account use codex {b}"), true, false),
        comment(1, &format!("taskq: account use codex {a}"), true, false),
        comment(2, &format!("taskq: account avoid codex {a}"), true, false),
        comment(3, &format!("taskq: account avoid claude {b}"), false, false),
        comment(4, &format!("taskq: account avoid claude {a}"), true, true),
        comment(6, "taskq: account avoid codex nothex!!", true, false),
        comment(7, &format!("taskq: account use codex-app {a}"), true, false),
    ]);
    assert_eq!(
        p["codex"].preferred.as_deref(),
        Some(b),
        "the later use replaces the earlier one"
    );
    assert_eq!(p["codex"].avoid, BTreeSet::from([a.to_string()]));
    assert_eq!(
        p.keys().collect::<Vec<_>>(),
        ["codex"],
        "untrusted, edited, malformed and account-less rows fold to nothing"
    );
    let cleared = fold(&[
        comment(1, &format!("taskq: account avoid codex {a}"), true, false),
        comment(2, &format!("taskq: account clear codex {a}"), true, false),
    ]);
    assert!(cleared.is_empty(), "clear removes either");

    let role = std::ffi::OsString::from("worker");
    assert!(
        refusal(Some(&role), None).is_some()
            && refusal(None, Some(&role)).is_some()
            && refusal(None, None).is_none()
    );
    let pm = std::ffi::OsString::from("pm");
    assert!(
        refusal(Some(&pm), None).is_some(),
        "every role, not only a worker"
    );

    let board = crate::board::Fake::default();
    assert!(
        fleet_write(&board, "use", "codex-app", a).is_err()
            && fleet_write(&board, "use", "codex", "UPPER").is_err()
            && fleet_write(&board, "drop", "codex", a).is_err()
    );
    assert!(
        board.issues(None).unwrap().is_empty(),
        "a refused write creates nothing"
    );
    fleet_write(&board, "use", "codex", a).unwrap();
    fleet_write(&board, "avoid", "codex", b).unwrap();
    let issues = board.issues(None).unwrap();
    assert_eq!(issues.len(), 1, "one policy issue");
    assert!(
        issues[0].labels.iter().any(|l| l == POLICY_LABEL) && !crate::service::is_task(&issues[0])
    );
    let comments = board.comments(issues[0].number).unwrap();
    assert_eq!(
        comments.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(),
        [
            format!("taskq: account use codex {a}"),
            format!("taskq: account avoid codex {b}")
        ]
    );

    // The refresh reads the issue, this board's other hosts, and one comment read per change.
    let d = dir("fleet");
    let mut cache = FleetCache::default();
    cache.refresh(&board, &d, &[], "me", NOW).unwrap();
    assert!(!cache.due(NOW + EVERY - 1) && cache.due(NOW + EVERY));
    let v = View::read(&d);
    assert_eq!(v.local_reason(NOW), None);
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: §3 Runtime accounts (#2341 A4, A5): a host applies policy to its own homes, reports what it cannot, and falls
// back to local ranking out loud. Risk: a `use` with no login here silently runs elsewhere, or a dead link holds work.
#[test]
fn policy_applies_to_own_homes_and_says_when_it_cannot() {
    let d = dir("policy");
    list(&d, &[("default", true), ("work", true)]);
    poll(&d, NOW, &[("default", 10), ("work", 90)]);
    let lines = |d: &Path| {
        Host::read(d)
            .usage_lines(NOW)
            .into_iter()
            .filter(|l| l.starts_with("account-policy "))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lines(&d),
        ["account-policy local no connection"],
        "no connection keeps local ranking and says so"
    );
    let policy = |preferred: Option<String>, avoid: &[String]| {
        BTreeMap::from([(
            "codex".to_string(),
            RuntimePolicy {
                preferred,
                avoid: avoid.iter().cloned().collect(),
            },
        )])
    };

    fleet(&d, NOW, policy(Some(id("work")), &[]), vec![], None);
    assert_eq!(
        Host::read(&d).active("codex", NOW).as_deref(),
        Some("work"),
        "`use` beats better usage"
    );
    assert_eq!(
        lines(&d),
        [format!("account-policy use codex {}", id("work"))]
    );

    fleet(&d, NOW, policy(Some(id("elsewhere")), &[]), vec![], None);
    assert_eq!(
        lines(&d),
        [format!("account-policy unmet codex {}", id("elsewhere"))],
        "no home for the id: unmet, never a copied login"
    );
    assert_eq!(
        Host::read(&d).active("codex", NOW).as_deref(),
        Some("default"),
        "unmet keeps ranking"
    );

    fleet(&d, NOW, policy(None, &[id("default")]), vec![], None);
    assert_eq!(
        Host::read(&d).active("codex", NOW).as_deref(),
        Some("work"),
        "avoid moves new starts"
    );
    let h = Host::read(&d);
    assert!(!h.off("codex", NOW));

    fleet(
        &d,
        NOW,
        policy(None, &[id("default"), id("work")]),
        vec![],
        None,
    );
    let h = Host::read(&d);
    assert_eq!(
        (h.active("codex", NOW), h.off("codex", NOW)),
        (None, true),
        "avoid of every account admits nothing"
    );
    assert_eq!(
        h.off_reason("codex"),
        "every account disabled or avoided on this host"
    );

    fleet(
        &d,
        NOW,
        policy(None, &[id("default"), id("work")]),
        vec![],
        Some("policy issue unreadable"),
    );
    let h = Host::read(&d);
    assert_eq!(
        h.active("codex", NOW).as_deref(),
        Some("default"),
        "an unreadable issue keeps local ranking"
    );
    assert_eq!(lines(&d), ["account-policy local policy issue unreadable"]);

    fleet(
        &d,
        NOW - FLEET_TTL - 1,
        policy(None, &[id("default")]),
        vec![],
        None,
    );
    assert_eq!(
        lines(&d),
        ["account-policy local policy stale"],
        "a dead speaker cannot pin old policy"
    );
    assert_eq!(
        Host::read(&d).active("codex", NOW).as_deref(),
        Some("default")
    );
    std::fs::write(d.join(FLEET), "corrupt").unwrap();
    assert_eq!(lines(&d), ["account-policy local policy file unreadable"]);
    assert!(
        Host::read(&d).error.is_none(),
        "policy loss never turns into unknown availability"
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: §3 Runtime accounts (#2341 A3, A5): a poll moves a new start with reason `usage`, policy with `fleet`, and
// `taskq accounts` reads id, usage and policy locally and from a heartbeat. Risk: the owner cannot see why starts moved.
#[test]
fn usage_and_fleet_switches_and_rows() {
    let d = dir("switches");
    list(&d, &[("default", true), ("work", true)]);
    poll(&d, NOW, &[("default", 50), ("work", 50)]);
    let mut seen = BTreeMap::new();
    accounts::switches(&Host::read(&d), NOW, &mut seen);
    poll(&d, NOW, &[("default", 95), ("work", 20)]);
    let lines = |h: &Host, seen: &mut BTreeMap<String, Option<String>>| {
        accounts::switches(h, NOW, seen)
            .iter()
            .map(|s| s.line())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lines(&Host::read(&d), &mut seen),
        ["account codex default -> work (usage)"]
    );
    assert!(
        Host::read(&d).usable("codex", "default", NOW),
        "a prepared launch keeps its account when ranking moves"
    );
    assert!(!Host::read(&d).usable("codex", "absent", NOW));

    let policy = |preferred: Option<String>, avoid: &[String]| {
        BTreeMap::from([(
            "codex".to_string(),
            RuntimePolicy {
                preferred,
                avoid: avoid.iter().cloned().collect(),
            },
        )])
    };
    fleet(&d, NOW, policy(Some(id("default")), &[]), vec![], None);
    assert_eq!(
        lines(&Host::read(&d), &mut seen),
        ["account codex work -> default (fleet)"]
    );
    fleet(&d, NOW, policy(None, &[id("default")]), vec![], None);
    assert_eq!(
        lines(&Host::read(&d), &mut seen),
        ["account codex default -> work (fleet)"]
    );

    fleet(&d, NOW, policy(Some(id("work")), &[]), vec![], None);
    let h = Host::read(&d);
    let rows = h.rows("mac", NOW, |_, _| None);
    let work = rows
        .iter()
        .find(|r| r.runtime == "codex" && r.label == "work")
        .unwrap();
    assert_eq!(
        (work.id.as_deref(), work.policy.as_deref()),
        (Some(id("work").as_str()), Some("use"))
    );
    assert_eq!(work.usage.len(), 1);
    assert!(work.usage[0].contains("used 20%"), "{:?}", work.usage);
    assert!(
        work.text().contains(&format!("id {}", id("work"))) && work.text().contains("policy use"),
        "{}",
        work.text()
    );
    assert_eq!(work.json()["id"], id("work"));
    assert_eq!(work.json()["policy"], "use");
    assert!(
        rows.iter()
            .find(|r| r.label == "default" && r.runtime == "codex")
            .unwrap()
            .policy
            .is_none()
    );

    // Another host's heartbeat: `account` lines stay as they were; usage rows and policy join them; a row with no
    // `account` line still shows, and a hold line is no usage row.
    let body = format!(
        "account codex work on 2026-01-01T00:00:00Z\nlimit codex 2026-01-01T00:00:00Z\nlimit codex work id {} window 10080m used 20 reset - at 2026-01-01T00:00:00Z\nlimit claude default id - unknown no login\naccount-policy use codex {}\naccount-policy unmet codex {}\n",
        id("work"),
        id("work"),
        id("elsewhere")
    );
    let rows = accounts::heartbeat_rows("win", &body);
    let work = rows
        .iter()
        .find(|r| r.runtime == "codex" && r.label == "work")
        .unwrap();
    assert_eq!(
        (work.id.as_deref(), work.usage.len(), work.policy.as_deref()),
        (Some(id("work").as_str()), 1, Some("use"))
    );
    let claude = rows.iter().find(|r| r.runtime == "claude").unwrap();
    assert_eq!(
        (claude.label.as_str(), claude.usage.as_slice()),
        ("default", ["unknown no login".to_string()].as_slice())
    );
    let unmet = rows.iter().find(|r| r.label == "-").unwrap();
    assert_eq!(
        (unmet.id.as_deref(), unmet.policy.as_deref(), unmet.enabled),
        (Some(id("elsewhere").as_str()), Some("unmet"), false)
    );

    std::fs::remove_file(d.join(FLEET)).unwrap();
    let h = Host::read(&d);
    assert!(
        h.rows("mac", NOW, |_, _| None)
            .iter()
            .all(|r| r.policy.as_deref() == Some("local no connection")),
        "no policy says why on every row"
    );
    std::fs::remove_dir_all(d).unwrap();
}

// Chain: §3 Runtime accounts (#2341 A3): `account_floor` is owner config with one shape. Risk: a typo silently changes ranking.
#[test]
fn account_floor_shape() {
    assert_eq!(floor_of(&serde_json::json!({})), Ok(None));
    assert_eq!(
        floor_of(&serde_json::json!({"account_floor": 0})),
        Ok(Some(0))
    );
    assert_eq!(
        floor_of(&serde_json::json!({"account_floor": 100})),
        Ok(Some(100))
    );
    for bad in [
        serde_json::json!({"account_floor": 101}),
        serde_json::json!({"account_floor": -1}),
        serde_json::json!({"account_floor": 1.5}),
        serde_json::json!({"account_floor": "15"}),
    ] {
        assert!(floor_of(&bad).is_err(), "{bad}");
    }
}

// #2341 A1 instrument: a real read-only poll on this host, run by hand on each host from a checkout:
// `cargo test --locked --bin taskq native_poll_qualifies -- --ignored --nocapture`. It prints rows only: fingerprints,
// percents and fixed reasons, and fails when a login file changes or a row carries an email.
#[test]
#[ignore = "reads this host's real logins and calls the providers"]
fn native_poll_qualifies() {
    let install = crate::platform::default_install();
    let policy = accounts::read_policy(&install).unwrap();
    let mut ids = Ids::default();
    for rt in super::super::CLI_RUNTIMES
        .iter()
        .filter(|rt| accounts::applies(rt))
    {
        let labels = policy
            .get(*rt)
            .map_or(vec![accounts::DEFAULT.to_string()], |l| {
                l.iter()
                    .filter(|e| e.enabled)
                    .map(|e| e.label.clone())
                    .collect()
            });
        for label in labels {
            let home = accounts::home(rt, &label)
                .or_else(|| accounts::base(rt))
                .unwrap();
            let files: Vec<PathBuf> = ["auth.json", ".credentials.json"]
                .iter()
                .map(|f| home.join(f))
                .collect();
            let sign = || {
                files
                    .iter()
                    .map(|f| std::fs::read(f).ok().map(|b| sha256(&b)))
                    .collect::<Vec<_>>()
            };
            let before = sign();
            let started = std::time::Instant::now();
            let p = fetch(rt, &label, &mut ids);
            assert_eq!(sign(), before, "{rt} {label}: a poll changed a login file");
            let at = accounts::now();
            let id = p.id.clone();
            let row = |usage| {
                Row {
                    rt: rt.to_string(),
                    label: label.clone(),
                    id: id.clone(),
                    usage,
                }
                .line()
            };
            let rows: Vec<String> = match &p.unknown {
                Some(reason) => vec![row(Usage::Unknown(reason.clone()))],
                None => {
                    p.windows
                        .iter()
                        .map(|w| {
                            row(if p.unreadable.is_empty() {
                                Usage::Window {
                                    window: w.clone(),
                                    at,
                                }
                            } else {
                                Usage::Partial {
                                    window: w.clone(),
                                    at,
                                }
                            })
                        })
                        .chain(p.unreadable.iter().map(|reason| {
                            row(Usage::Unknown(format!("quota unreadable {reason}")))
                        }))
                        .collect()
                }
            };
            for r in rows {
                assert!(!r.contains('@'), "a row carries an email");
                println!(
                    "{r}  ({} ms, login files unchanged)",
                    started.elapsed().as_millis()
                );
            }
        }
    }
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
