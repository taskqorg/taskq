use super::*;

// Chain: identity creation -> concurrent project startup -> update/data pruning -> reread.
#[test]
fn identity_survives_pruning_and_concurrent_creation() {
    let root = std::env::temp_dir().join(format!("taskq-enrollment-{}", std::process::id()));
    let install = root.join("install");
    let _ = fs::remove_dir_all(&root);
    let ids = std::thread::scope(|s| {
        let threads: Vec<_> = (0..8)
            .map(|_| s.spawn(|| installation_id(&install).unwrap()))
            .collect();
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(ids.iter().all(|id| id == &ids[0]));
    crate::platform::sweep(&install);
    crate::data::prune(&root, &install, "p", &[]);
    assert_eq!(installation_id(&install).unwrap(), ids[0]);
    fs::write(install.join("installation-id"), "broken").unwrap();
    assert!(installation_id(&install).unwrap_err().contains("corrupt"));
    assert_eq!(
        fs::read_to_string(install.join("installation-id")).unwrap(),
        "broken"
    );
    // connection state survives the sweep and pruning too
    fs::create_dir_all(dir(&install)).unwrap();
    fs::write(dir(&install).join("connection.json"), "{}").unwrap();
    crate::platform::sweep(&install);
    assert!(dir(&install).join("connection.json").exists());
    fs::remove_file(install.join("installation-id")).unwrap();
    assert_ne!(installation_id(&install).unwrap(), ids[0]);
    fs::remove_dir_all(root).unwrap();
}

/// DashQ stand-in: answers revokes (or nothing, offline) and records the credentials it was asked to revoke.
struct Dash {
    online: bool,
    revoked: Vec<String>,
}
impl enroll::Wire for Dash {
    fn post(
        &mut self,
        _: &str,
        route: &str,
        auth: Option<&str>,
        _: &Value,
    ) -> Result<(u16, Value), String> {
        if !self.online {
            return Err("offline".into());
        }
        assert_eq!(route, "/connection/revoke", "disconnect only revokes");
        self.revoked.push(auth.unwrap_or_default().into());
        Ok((200, json!({"protocol": 1})))
    }
    fn now(&self) -> u64 {
        0
    }
    fn sleep(&mut self, _: u64) {}
    fn show(&mut self, _: &str, _: &str) {}
}

// Chain: enroll interrupted mid-pairing -> taskq disconnect -> enroll rerun. Risk: disconnect reporting the
// revoke confirmed while a pending pairing (or its staged, already granted credential) survives and resumes.
#[test]
fn disconnect_cancels_pending_pairing() {
    let install = std::env::temp_dir().join(format!("taskq-disconnect-{}", std::process::id()));
    let _ = fs::remove_dir_all(&install);
    let write = |pending: Value, active: Value, revoke: Value| {
        fs::create_dir_all(dir(&install)).unwrap();
        fs::write(
            dir(&install).join("connection.json"),
            json!({"installation": "i", "pending": pending, "active": active, "revoke": revoke})
                .to_string(),
        )
        .unwrap();
    };
    let conn = |c: &str| json!({"destination": "https://d/machine", "credential": c});
    let mut dash = Dash {
        online: true,
        revoked: vec![],
    };
    assert_eq!(
        disconnect_with(&install, &mut dash).unwrap(),
        None,
        "no state: none"
    );
    // code shown, not approved yet: nothing granted
    write(
        json!({"destination": "https://d/machine", "device_code": "x", "staged": null}),
        Value::Null,
        json!([]),
    );
    assert_eq!(
        disconnect_with(&install, &mut dash).unwrap().as_deref(),
        Some("disconnected: pairing cancelled; nothing granted to revoke")
    );
    assert!(
        state(&install).unwrap().unwrap()["pending"].is_null(),
        "enroll cannot resume the pairing"
    );
    assert_eq!(
        disconnect_with(&install, &mut dash).unwrap(),
        None,
        "second disconnect says none"
    );
    // approved and staged, ack not sent, beside an active connection: both credentials revoked
    write(
        json!({"destination": "https://d/machine", "device_code": "x", "staged": conn("staged")}),
        conn("active"),
        json!([]),
    );
    assert_eq!(
        disconnect_with(&install, &mut dash).unwrap().as_deref(),
        Some("disconnected: pairing cancelled; sending stopped; revoke confirmed")
    );
    assert_eq!(dash.revoked, ["staged", "active"]);
    // offline: the revoke stays queued for the service, reported as pending
    write(
        json!({"destination": "https://d/machine", "device_code": "x", "staged": conn("s2")}),
        Value::Null,
        json!([]),
    );
    let mut off = Dash {
        online: false,
        revoked: vec![],
    };
    assert_eq!(
        disconnect_with(&install, &mut off).unwrap().as_deref(),
        Some("disconnected: pairing cancelled; 1 revoke pending, the service retries it")
    );
    let s = state(&install).unwrap().unwrap();
    assert!(s["pending"].is_null() && s["revoke"][0]["credential"] == "s2");
    // repeated disconnect with only the queued revoke: retried and reported, never "connection: none"
    assert_eq!(
        disconnect_with(&install, &mut off).unwrap().as_deref(),
        Some("connection: none; 1 revoke pending, the service retries it")
    );
    let mut back = Dash {
        online: true,
        revoked: vec![],
    };
    assert_eq!(
        disconnect_with(&install, &mut back).unwrap().as_deref(),
        Some("connection: none; revoke confirmed")
    );
    assert_eq!(back.revoked, ["s2"]);
    assert_eq!(
        disconnect_with(&install, &mut back).unwrap(),
        None,
        "nothing left: none"
    );
    fs::remove_dir_all(install).unwrap();
}

// Chain: damaged connection.json -> connection status, disconnect, service pass. Risk: valid JSON of the wrong
// shape reading as "connection: none" (R12) or being overwritten, losing a credential DashQ still honours.
#[test]
fn malformed_connection_state_reads_unknown_and_is_kept() {
    let install = std::env::temp_dir().join(format!("taskq-malformed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&install);
    fs::create_dir_all(dir(&install)).unwrap();
    let ok = json!({"installation": "i", "pending": null, "active": {"destination": "https://d/machine", "credential": "c"}, "revoke": []});
    let mut bad = vec![json!({}), json!([]), json!(null), json!("x")];
    for (k, v) in [
        ("installation", json!(1)),
        ("pending", json!("x")),
        ("pending", json!({"staged": null})),
        ("active", json!({"destination": "https://d/machine"})),
        ("active", json!([])),
        ("revoke", json!(null)),
        ("revoke", json!([1])),
        ("revoke", json!([{"credential": "c"}])),
        ("revoke", json!([{"destination": "", "credential": "c"}])),
    ] {
        let mut s = ok.clone();
        s[k] = v;
        bad.push(s);
    }
    let path = dir(&install).join("connection.json");
    fs::write(&path, ok.to_string()).unwrap();
    assert!(
        state(&install).is_ok() && active(&install).unwrap().is_some(),
        "coreq's own shape reads"
    );
    for b in bad {
        fs::write(&path, b.to_string()).unwrap();
        let unknown =
            |r: Res<String>| assert!(r.unwrap_err().starts_with("connection: unknown"), "{b}");
        unknown(state(&install).map(|_| String::new()));
        unknown(active(&install).map(|_| String::new()));
        unknown(
            disconnect_with(
                &install,
                &mut Dash {
                    online: true,
                    revoked: vec![],
                },
            )
            .map(|_| String::new()),
        );
        let mut s = Sender::default();
        assert!(
            s.tick(&install, Some(&snap("o/a")), &local(), 1)
                .unwrap()
                .contains("unknown"),
            "{b}"
        );
        assert!(s.job.is_none());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            b.to_string(),
            "file kept"
        );
    }
    fs::remove_dir_all(install).unwrap();
}

// Chain: board heartbeats, task rows and the local runtime probe -> host observations -> DashQ reader expiry.
// Risk: DashQ infers online, logged in or an empty queue from silence, an old record or an incomplete read.
#[test]
fn host_observations_match_the_fixture() {
    use crate::{
        core::{State, Task},
        snapshot::{Host, Row},
    };
    let beat = |name: &str, updated, service: Option<&str>| Host {
        name: name.into(),
        updated,
        service: service.map(String::from),
        ..Default::default()
    };
    let row = |n, state, session: Option<&str>, labels: &[&str]| Row {
        n,
        labels: labels.iter().map(|l| l.to_string()).collect(),
        task: Task {
            state,
            attempt: session.map(|_| n),
            session: session.map(String::from),
            ..Default::default()
        },
        ..Default::default()
    };
    let base = || Snapshot {
        source: "host",
        observed: 1000,
        reason: None,
        reason_code: None,
        board: json!({"kind": "github", "host": "github.com", "repo": "o/r"}),
        tasks: vec![
            row(1, State::Doing, Some("claude:x mac"), &[]),
            row(2, State::Ready, None, &["host-mac"]),
            row(3, State::Waiting, None, &["host-win"]),
            row(4, State::Review, Some("codex:y win"), &[]),
            row(5, State::Ready, None, &[]),
        ],
        // mac online, win offline (heartbeat older than ttl), fra partial (no service line), wsl unknown (no heartbeat)
        hosts: vec![
            beat("mac", 950, Some("aaaaaaaaaaaa")),
            beat("win", 600, Some("bbbbbbbbbbbb")),
            beat("fra", 980, None),
        ],
    };
    let probe = |codex: &str| json!({"at": 990, "rows": [{"runtime": "claude", "login": "logged in"}, {"runtime": "codex", "login": codex}]});
    let local = |agents| Local {
        host: "wsl".into(),
        service: Some("cccccccccccc".into()),
        ttl: 300,
        agents,
        progress: BTreeMap::new(),
    };
    let scope = [BoardScope::new("github", "github.com", "o/r").unwrap()];
    let obs = |s: &Snapshot, agents| project(s, &"a".repeat(64), &scope, &local(agents)).unwrap();
    let mut incomplete = base();
    incomplete.reason = Some("board read failed".into());
    let mut over = base();
    over.tasks
        .extend((10..10 + MAX_TASKS as u64).map(|n| row(n, State::Ready, None, &[])));
    let mac = obs(&base(), probe("logged in"));
    let got = json!({
        "online offline unknown partial; queue busy idle; login logged in": mac.observations,
        "login not logged in": obs(&base(), probe("not logged in")).observations.last(),
        "login unknown: no probe": obs(&base(), Value::Null).observations.last(),
        "login partial: codex unknown": obs(&base(), probe("unknown")).observations.last(),
        "queue unknown: incomplete read": obs(&incomplete, Value::Null).observations.iter().filter(|o| o["kind"] == "queue").collect::<Vec<_>>(),
        "queue partial: task limit": obs(&over, Value::Null).observations.iter().filter(|o| o["kind"] == "queue").collect::<Vec<_>>(),
    });
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/host-observations.json");
    if std::env::var_os("TASKQ_BLESS").is_some() {
        fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
    assert!(
        !got.to_string().contains("x mac") && !got.to_string().contains("codex:y"),
        "no session ids"
    );
}

// Chain: snapshot admission -> taskq-board/1 task rows -> shipped fixture (#1616). Risk: a consumer meets a code
// outside the fixture, free reason text leaves the host, or a row nobody decided reads as decided.
#[test]
fn admission_codes_match_the_fixture() {
    use crate::{
        core::{State, Task},
        snapshot::{ADMISSION_CODES, ADMISSION_UNKNOWN, Admission, Row},
    };
    let row = |n, state, admission| Row {
        n,
        task: Task {
            state,
            ..Default::default()
        },
        admission,
        ..Default::default()
    };
    let decided = |code, reason: &str, host: Option<&str>| {
        Some(Admission {
            code,
            reason: reason.into(),
            observed: 1000,
            host: host.map(String::from),
        })
    };
    let mut tasks: Vec<Row> = ADMISSION_CODES
        .iter()
        .map(|c| c.0)
        .filter(|c| !matches!(*c, "dependency" | "unknown"))
        .enumerate()
        .map(|(i, c)| {
            row(
                i as u64 + 1,
                State::Ready,
                decided(c, "free text from the host", Some("mac")),
            )
        })
        .collect();
    tasks.push(row(
        20,
        State::Waiting,
        decided("dependency", "#1 open", None),
    ));
    tasks.extend(ADMISSION_UNKNOWN.iter().enumerate().map(|(i, r)| {
        row(
            30 + i as u64,
            State::Ready,
            decided("unknown", r, (i > 0).then_some("mac")),
        )
    }));
    tasks.push(row(40, State::Doing, None));
    let snap = Snapshot {
        source: "host",
        observed: 1000,
        reason: None,
        reason_code: None,
        board: json!({"kind": "github", "host": "github.com", "repo": "o/r"}),
        tasks,
        hosts: vec![],
    };
    let local = Local {
        host: "mac".into(),
        service: None,
        ttl: 300,
        agents: Value::Null,
        progress: BTreeMap::new(),
    };
    let scope = [BoardScope::new("github", "github.com", "o/r").unwrap()];
    let o = project(&snap, &"a".repeat(64), &scope, &local).unwrap();
    let got = json!({
        "codes": ADMISSION_CODES.iter().map(|(code, meaning)| json!({"code": code, "meaning": meaning})).collect::<Vec<_>>(),
        "unknown_reasons": ADMISSION_UNKNOWN,
        "tasks": o.tasks,
    });
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/admission-codes.json");
    if std::env::var_os("TASKQ_BLESS").is_some() {
        fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(got, want, "TASKQ_BLESS=1 rewrites {}", path.display());
    let rows = got["tasks"].as_array().unwrap();
    let mut seen: Vec<&str> = rows
        .iter()
        .filter_map(|t| t["admission"]["code"].as_str())
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        ADMISSION_CODES.len(),
        "the fixture shows every code"
    );
    assert!(
        !got["tasks"].to_string().contains("free text")
            && !got["tasks"].to_string().contains("#1 open"),
        "free reason text stays on the host"
    );
    assert!(
        rows.iter()
            .filter(|t| t["admission"]["code"] == "unknown")
            .all(|t| t["admission"]["reason"].is_string()),
        "unknown names its reason"
    );
    assert!(
        rows.iter()
            .find(|t| t["n"] == 40)
            .unwrap()
            .get("admission")
            .is_none(),
        "a doing row carries none"
    );
}

// Chain: live attempt log -> R6 3 session progress -> DashQ record allowlist (#1565, #1617). Risk: another host's or an
// idle task's progress, worker line text, the log itself, a session id or a path leaving the host.
#[test]
fn progress_carries_only_stage_and_age_of_this_hosts_live_attempts() {
    use crate::{
        core::{State, Task},
        snapshot::Row,
    };
    let root = std::env::temp_dir().join(format!("taskq-progress-{}", std::process::id()));
    fs::create_dir_all(root.join(".taskq")).unwrap();
    let row = |n, state, session: &str| Row {
        n,
        task: Task {
            state,
            attempt: Some(n * 10),
            role: Some("worker".into()),
            session: Some(session.into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let say = |n: u64, text: &str| {
        fs::write(
            crate::runtime::attempt_log(&root, "worker", n, n * 10),
            json!({"type": "item.completed", "item": {"type": "agent_message", "text": text}})
                .to_string()
                + "\n",
        )
        .unwrap()
    };
    say(1, &format!("Running tests in {}/src now", root.display()));
    say(2, "secret worker words");
    say(3, "other host");
    say(4, "done");
    let s = Snapshot {
        tasks: vec![
            row(1, State::Doing, "codex:thread-a mac"),
            row(2, State::Review, "codex:- mac"),
            row(3, State::Doing, "codex:b win"),
            row(4, State::Ready, "codex:c mac"),
        ],
        ..snap("o/r")
    };
    let p = progress(&s, &root, "mac");
    assert_eq!(
        p.keys().copied().collect::<Vec<_>>(),
        vec![1, 2],
        "only this host's doing or review attempts"
    );
    assert_eq!(
        (
            p[&1]["stage"].as_str(),
            p[&1]["runtime"].as_str(),
            p[&1]["host"].as_str()
        ),
        (Some("doing"), Some("codex"), Some("mac"))
    );
    assert!(p[&1]["at"].as_u64().is_some(), "update time");
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        keys(&p[&2]),
        ["host", "runtime", "stage", "at"],
        "host, runtime, stage and age only"
    );
    let mut l = local();
    l.progress = p;
    let o = project(
        &s,
        &"a".repeat(64),
        &[BoardScope::new("github", "github.com", "o/r").unwrap()],
        &l,
    )
    .unwrap();
    let text = serde_json::to_string(&o).unwrap();
    for leak in [
        "thread-a",
        &*root.to_string_lossy(),
        "Running tests",
        "secret worker words",
        "\"line\"",
    ] {
        assert!(!text.contains(leak), "{leak} left the host: {text}");
    }
    assert!(
        serde_json::to_value(&o).unwrap()["tasks"][2]
            .get("session")
            .is_none()
    );
    fs::remove_dir_all(root).unwrap();
}

fn local() -> Local {
    Local {
        host: "mac".into(),
        service: None,
        ttl: 300,
        agents: Value::Null,
        progress: BTreeMap::new(),
    }
}

fn snap(repo: &str) -> Snapshot {
    Snapshot {
        source: "host",
        observed: 1_700_000_000,
        reason: None,
        reason_code: None,
        board: json!({"kind": "github", "host": "github.com", "repo": repo}),
        tasks: vec![],
        hosts: vec![],
    }
}

// Chain: enrolled board scope -> service pass of each project -> delivery thread. Risk: another project's board
// leaking into the grant, or an unreachable DashQ stalling the pass that launches workers.
#[test]
fn sender_sends_only_the_approved_board_and_never_blocks() {
    let install = std::env::temp_dir().join(format!("taskq-sender-{}", std::process::id()));
    let _ = fs::remove_dir_all(&install);
    assert_eq!(
        Sender::default().tick(&install, Some(&snap("o/a")), &local(), 1),
        None,
        "no connection: nothing sent"
    );
    assert!(
        !dir(&install).exists(),
        "a status read creates no connection state"
    );
    let board = BoardScope::new("github", "GitHub.com", "O/A").unwrap();
    assert_eq!(board.value(), "github:github.com/o/a");
    let id = installation_id(&install).unwrap();
    let conn = json!({"destination": "https://127.0.0.1:9/machine", "credential": "c", "generation": 1, "installation": id, "scope": {"name": SCHEMA, "value": board.value()}});
    fs::create_dir_all(dir(&install)).unwrap();
    fs::write(
        dir(&install).join("connection.json"),
        json!({"installation": id, "pending": null, "active": conn, "revoke": []}).to_string(),
    )
    .unwrap();
    assert_eq!(approved(&conn), Some(board));
    assert_eq!(
        approved(&json!({"scope": {"name": SCHEMA, "value": "github:github.com/O/A"}})),
        None,
        "non-canonical scope"
    );
    let mut s = Sender::default();
    assert_eq!(
        s.tick(&install, Some(&snap("o/b")), &local(), 1),
        None,
        "another board: nothing sent"
    );
    assert!(s.job.is_none());
    let (_, rec) = record(&install, &snap("o/a"), &conn, &local()).unwrap();
    assert_eq!(
        (
            rec["schema"].as_str(),
            rec["payload"]["board"]["repo"].as_str()
        ),
        (Some(SCHEMA), Some("o/a"))
    );
    // #1328: each step's cost on this OS, printed on success and failure; only the state read and the payload
    // stay on the pass
    let timed = |f: &dyn Fn()| {
        let t = std::time::Instant::now();
        f();
        t.elapsed()
    };
    let steps = format!(
        "state read {:?}, coreq active read {:?}, record build {:?}, resources read {:?}",
        timed(&|| {
            state(&install).unwrap();
        }),
        timed(&|| {
            active(&install).unwrap();
        }),
        timed(&|| {
            record(&install, &snap("o/a"), &conn, &local()).unwrap();
        }),
        timed(&|| {
            let _ = resources_record(&install, &conn, 1);
        })
    );
    let t = std::time::Instant::now();
    assert_eq!(s.tick(&install, Some(&snap("o/a")), &local(), 1), None);
    let steps = format!("{steps}, tick with thread spawn {:?}", t.elapsed());
    eprintln!("sender steps: {steps}");
    assert!(
        s.job.is_some() && t.elapsed().as_secs() < 2,
        "delivery runs on its own thread: {steps}"
    );
    while !s.job.as_ref().unwrap().1.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(50))
    }
    assert_eq!(
        s.tick(&install, Some(&snap("o/a")), &local(), 2).as_deref(),
        Some("dashq connection: retry in 1 s"),
        "outage only waits"
    );
    fs::remove_dir_all(install).unwrap();
}
