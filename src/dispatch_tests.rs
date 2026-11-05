use super::*;
use crate::board::Fake;

#[test]
fn closing_review_records_its_result_event() {
    let dir = std::env::temp_dir().join(format!("taskq-2283-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let (_capture, _guard) = crate::profile::tests::collecting(&dir);
    let mut c = crate::tests::cfg();
    c.install = dir.clone();
    let review = |env: ChildEnv| {
        review_result(&c, 7, 42, "codex", &env);
        crate::telemetry::worker_events(&dir)
            .into_iter()
            .filter(|e| e.event == "result")
            .count()
    };
    assert_eq!(
        review((Some("7".into()), Some("41".into()))),
        0,
        "another attempt's child records nothing"
    );
    assert_eq!(review((None, None)), 0, "an owner close records nothing");
    assert_eq!(review((Some("7".into()), Some("42".into()))), 1);
    let ev = crate::telemetry::worker_events(&dir)
        .into_iter()
        .find(|e| e.event == "result")
        .unwrap();
    assert_eq!(
        (ev.task, ev.attempt, ev.role.as_str(), ev.runtime.as_str()),
        (7, 42, "review", "codex")
    );
}

// Chain: §5, #2137 A3. Recovery must not steal a live/foreign writer, and must finish a crashed partial label write.
#[test]
fn balancing_reroute_writer_recovery_preserves_ownership() {
    let b = Fake::default();
    let n = b
        .create("Reroute", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    let intent = b
        .comment(n, "taskq: reroute claude codex\nrouting-fence: 1")
        .unwrap();
    let native = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let machine = crate::config::hostname();
    let writer = |machine: &str, started: &str| {
        serde_json::to_string(&RoutingWriter {
            machine: machine.into(),
            pid: native.pid,
            started: started.into(),
        })
        .unwrap()
    };
    for identity in [
        writer("invented-remote-host", &native.started),
        writer(&machine, &native.started),
        "invalid invented identity".into(),
    ] {
        let claim = b
            .comment(
                n,
                &format!("taskq: routing-write {intent}\nwriter: {identity}"),
            )
            .unwrap();
        let before = b.comments(n).unwrap().len();
        let r = finish_reroute(&b, n);
        assert_eq!(r.outcome, "unknown");
        assert!(r.reason.contains("writer"));
        assert_eq!(
            b.comments(n).unwrap().len(),
            before,
            "unknown or live writer cannot be released"
        );
        assert_eq!(b.issue(n).unwrap().labels, ["taskq", "run-claude"]);
        b.comment(n, &format!("taskq: routing-release {claim}"))
            .unwrap();
    }
    // Caller absence alone cannot release an orphaned HTTP child.
    let dead = writer(&machine, "invented-previous-incarnation");
    let claim = b
        .comment(n, &format!("taskq: routing-write {intent}\nwriter: {dead}"))
        .unwrap();
    assert_eq!(finish_reroute(&b, n).outcome, "unknown");
    b.comment(n, &format!("taskq: routing-release {claim}"))
        .unwrap();
    // Invented prior-boot identity: only this boundary proves the whole request tree cannot write again.
    #[cfg(target_os = "macos")]
    let before_boot = "1-0";
    #[cfg(target_os = "linux")]
    let before_boot = "00000000-0000-0000-0000-000000000000-1";
    #[cfg(windows)]
    let before_boot = "1";
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let before_boot = "unsupported";
    let dead = writer(&machine, before_boot);
    b.comment(n, &format!("taskq: routing-write {intent}\nwriter: {dead}"))
        .unwrap();
    b.labels(n, &["run-codex".into()], &[]).unwrap();
    assert!(finish_reroute(&b, n).ok());
    assert_eq!(b.issue(n).unwrap().labels, ["taskq", "run-codex"]);
    let p = core::project(n, &entries(&b, n).unwrap(), false);
    assert!(p.reroute.is_none() && p.routing_writer.is_none());
    assert_eq!(p.task.retries, 0);
}

// Chain: § 2 ask/answer log -> snapshot history -> conversation. Risk (#1918 A5): a second question replaced the
// first question and its answer, so the conversation lost earlier decisions.
#[test]
fn history_keeps_every_answered_question_in_board_order() {
    let ask = |q: &str| {
        format!(
            "taskq: ask attempt 10\nDone: checked\nProblem: {q}\n1. Ship - + now; - risk\n2. Wait - + safe; - slow"
        )
    };
    let log: Vec<Entry> = [
        (10, "taskq: spawn worker claude:s win".to_string()),
        (11, ask("First?")),
        (12, "taskq: answer 1".into()),
        (13, ask("Second?")),
        (14, "taskq: answer\nnot yet".into()),
        (15, ask("Third?")),
    ]
    .into_iter()
    .map(|(id, body)| Entry {
        id,
        body,
        edited: false,
    })
    .collect();
    let comment = |id, author: &str| board::Comment {
        id,
        created: 0,
        body: String::new(),
        trusted: true,
        edited: false,
        author: author.into(),
        read: None,
    };
    let comments = [comment(12, "sam"), comment(14, "alex")];
    let p = core::project(7, &log, false);
    assert_eq!(p.task.state, State::Ask, "the third question is open");
    let h = history(7, &log, &p, &comments);
    let shown: Vec<_> = h
        .iter()
        .map(|(c, a)| {
            (
                c.as_deref()
                    .and_then(|c| c.lines().find(|l| l.starts_with("Problem: "))),
                a.by.as_str(),
                a.text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (Some("Problem: First?"), "sam", "Ship - + now; - risk"),
            (Some("Problem: Second?"), "alex", "not yet")
        ]
    );
    assert_eq!(
        h[0].1.source.as_ref().unwrap()["question"],
        11,
        "question jumps bind the original board revision, never an array index"
    );
    assert_eq!(
        h[1].1.source.as_ref().unwrap()["id"],
        14,
        "answers retain their immutable event identity"
    );
    let all = timeline(7, &log, &p, &comments);
    assert_eq!(
        all.iter()
            .filter_map(|(_, a)| a.source.as_ref().and_then(|s| s["id"].as_u64()))
            .collect::<Vec<_>>(),
        [10, 12, 14],
        "F16: accepted spawn joins the questions, current question stays single"
    );
    assert_eq!(
        all[0].1.text, log[0].body,
        "board source text remains exact"
    );
    assert_eq!(
        answered_in(7, &log, &p, &comments),
        None,
        "the open question has no answer yet"
    );
    let mut review = log[..3].to_vec();
    review.extend(
        [
            (
                13,
                "taskq: result aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa attempt 10",
            ),
            (14, "taskq: spawn reviewer codex:r win"),
            (
                15,
                "taskq: ask attempt 14\nDone: reviewed\nProblem: Reviewer choice",
            ),
            (16, "taskq: answer\nContinue"),
        ]
        .into_iter()
        .map(|(id, body)| Entry {
            id,
            body: body.into(),
            edited: false,
        }),
    );
    let projected = core::project(7, &review, false);
    let history = history(7, &review, &projected, &[]);
    let source = history.last().unwrap().1.source.as_ref().unwrap();
    assert_eq!(
        source["role"], "reviewer",
        "F16: an answered review question retains its original role"
    );
    assert_eq!(source["attempt"], 14);
    assert_eq!(source["session"], "codex:r win");
}

// Chain: § 2 owner note -> live attempt inbox read -> `eyes` reaction -> Conversations history. Risk (#2188 A3): an
// owner message lived only in a worker channel, or the thread could not tell whether the worker had read it.
#[test]
fn owner_notes_show_in_history_and_the_live_attempt_marks_them_read() {
    let b = Fake::default();
    let n = b.create("Read receipts", "", &["taskq".into()]).unwrap();
    let a = b
        .comment(n, &entry("spawn", &["worker", "claude:s", "mac"], None, ""))
        .unwrap();
    let file = crate::reply_files::File {
        id: "f1".into(),
        name: "shot.png".into(),
        mime: "image/png".into(),
        hash: "a".repeat(64),
        bytes: 1,
        url: "https://example.test/shot.png".into(),
    };
    let owner = b
        .comment(
            n,
            &format!(
                "taskq: steer\n{}",
                crate::reply_files::append("look at this", std::slice::from_ref(&file))
            ),
        )
        .unwrap();
    let worker = b
        .comment(n, &format!("taskq: steer attempt {a}\ncontinue"))
        .unwrap();
    let notes = |b: &Fake| {
        let c = b.comments(n).unwrap();
        let log = board::reply::log(n, &c).unwrap();
        history(n, &log, &core::project(n, &log, false), &c)
    };
    let h = notes(&b);
    assert_eq!(
        h.len(),
        1,
        "only the owner note shows; the attempt-bound steer is the worker's own"
    );
    let (q, row) = &h[0];
    assert!(q.is_none());
    assert_eq!(
        (row.text.as_str(), row.read, row.files.len()),
        ("look at this", Some(false), 1),
        "unread, files split from text"
    );
    assert_eq!(row.files[0]["url"], file.url);
    let c = b.comments(n).unwrap();
    let log = board::reply::log(n, &c).unwrap();
    assert!(
        mark_read(&b, n, &log, &c, a + 1).is_empty(),
        "a stale attempt sets no receipt"
    );
    assert_eq!(mark_read(&b, n, &log, &c, a), [owner]);
    assert_eq!(
        notes(&b)[0].1.read,
        Some(true),
        "read once the live attempt read the inbox"
    );
    assert_eq!(
        b.comments(n)
            .unwrap()
            .iter()
            .find(|c| c.id == worker)
            .unwrap()
            .read,
        Some(false)
    );
    let c = b.comments(n).unwrap();
    assert!(
        mark_read(&b, n, &log, &c, a).is_empty(),
        "a read note is not reacted twice"
    );
    assert_eq!(
        core::project(n, &board::reply::log(n, &c).unwrap(), false).task,
        core::project(n, &log, false).task,
        "the reaction leaves the fold unchanged"
    );
}

// Chain: § 5 all clients -> shared dispatcher -> stored board text. Risk: length rejects or loses a human reply.
#[test]
fn oversized_human_reply_is_complete_before_the_task_moves() {
    let text = "A reply with words and 🦀.\n".repeat(6000);
    for kind in ["answer", "steer", "requeue"] {
        let b = Fake::default();
        let n = b
            .create("Large reply fixture", "", &["taskq".into()])
            .unwrap();
        let a = b
            .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
            .unwrap();
        if kind == "answer" {
            b.comment(n, &entry("ask", &[], Some(a), "Problem: reply"))
                .unwrap();
        }
        let before = b.comments(n).unwrap().len();
        let op = Op {
            id: Some("large-reply-once".into()),
            revision: Some(core::revision(n, &entries(&b, n).unwrap())),
        };
        let r = act(&b, Path::new(""), n, kind, &[], &text, &op);
        assert!(r.ok(), "{kind}: {}", r.text());
        let raw = b.comments(n).unwrap();
        assert!(
            raw.iter().all(|c| c.body.len() < 65536),
            "{kind}: one oversized comment still reaches the board"
        );
        assert!(
            raw.len() > before + 2,
            "reply needs ordered parts plus one command"
        );
        let log = entries(&b, n).unwrap();
        assert_eq!(
            core::delivery(n, &log, raw.last().unwrap().id)
                .split_whitespace()
                .collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
        let legacy: Vec<_> = raw
            .iter()
            .map(|c| Entry {
                id: c.id,
                body: c.body.clone(),
                edited: c.edited,
            })
            .collect();
        assert_eq!(
            fold(n, &legacy, false),
            fold(n, &log, false),
            "N-1 folds the same task"
        );
        for prefix in before..raw.len() - 1 {
            assert_eq!(
                fold(n, &legacy[..prefix], false),
                fold(n, &legacy[..before], false),
                "parts never move the task"
            );
        }
        assert!(act(&b, Path::new(""), n, kind, &[], &text, &op).ok());
        assert_eq!(
            b.comments(n).unwrap().len(),
            raw.len(),
            "retry cannot duplicate parts or the command"
        );
        b.edit_comment(n, raw[before].id, "changed part").unwrap();
        assert!(
            entries(&b, n).unwrap_err().text().contains("reply"),
            "corruption is unknown, never a partial delivery"
        );
    }
}

// Chain: § 5 interrupted multipart POST -> board readback -> same operation. Risk: partial text resumes work or retry duplicates it.
#[test]
fn oversized_human_reply_recovers_a_lost_part_response() {
    let b = Fake::default();
    let n = b
        .create("Interrupted reply", "", &["taskq".into()])
        .unwrap();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let before = entries(&b, n).unwrap();
    let op = Op {
        id: Some("resume-reply".into()),
        revision: Some(core::revision(n, &before)),
    };
    let text = "Words before interruption.\n".repeat(6000);
    b.state.borrow_mut().lose_comment_response = true;
    let r = act(&b, Path::new(""), n, "steer", &[], &text, &op);
    assert_eq!(r.outcome, "unknown");
    assert_eq!(
        fold(n, &entries(&b, n).unwrap(), false),
        fold(n, &before, false)
    );
    let first = b.comments(n).unwrap().last().unwrap().body.clone();
    assert!(first.starts_with("taskq: reply-part "));
    assert!(act(&b, Path::new(""), n, "steer", &[], &text, &op).ok());
    let comments = b.comments(n).unwrap();
    assert_eq!(comments.iter().filter(|c| c.body == first).count(), 1);
    let log = entries(&b, n).unwrap();
    assert_eq!(
        core::delivery(n, &log, comments.last().unwrap().id).trim(),
        text.trim()
    );
    for damage in [
        "missing",
        "untrusted",
        "edited",
        "forged-fields",
        "out-of-order",
    ] {
        let mut bad = comments.clone();
        let part = bad.iter().position(|c| c.body == first).unwrap();
        match damage {
            "missing" => {
                bad.remove(part);
            }
            "untrusted" => bad[part].trusted = false,
            "edited" => bad[part].edited = true,
            "forged-fields" => bad
                .last_mut()
                .unwrap()
                .body
                .push_str("\nextra command text"),
            _ => bad[part].id = u64::MAX,
        }
        assert!(
            board::reply::log(n, &bad).is_err(),
            "{damage} must be unknown"
        );
    }
    for body in [
        "A documented marker:\n<!-- taskq-reply example -->",
        "taskq: ask attempt 1\nProblem: inspect this syntax\n<!-- taskq-reply example -->",
        "taskq: answer\nA literal marker example\n<!-- taskq-reply example -->",
    ] {
        let comments = [board::Comment {
            id: 1,
            body: body.into(),
            trusted: true,
            ..Default::default()
        }];
        assert_eq!(
            board::reply::log(n, &comments).unwrap()[0].body,
            body,
            "historical comments stay verbatim"
        );
    }
}

// Chain: § 5 multipart write -> concurrent card -> bounded stale cleanup. Risk: an oversized edit leaves a losing command live.
#[test]
fn oversized_human_reply_race_edits_only_its_stored_reference() {
    let b = Fake::default();
    let n = b.create("Raced reply", "", &["taskq".into()]).unwrap();
    let a = b
        .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    b.state.borrow_mut().race = Some(entry("ask", &[], Some(a), "Problem: a new decision"));
    let op = Op {
        id: Some("raced-reply".into()),
        revision: Some(a),
    };
    let r = act(
        &b,
        Path::new(""),
        n,
        "requeue",
        &[],
        &"word ".repeat(30_000),
        &op,
    );
    assert_eq!(r.outcome, "stale", "{}", r.text());
    let raw = b.comments(n).unwrap();
    assert!(raw.last().unwrap().edited);
    assert!(
        raw.iter().all(|c| c.body.len() < 65536),
        "cleanup must edit the bounded stored reference"
    );
    assert_eq!(
        fold(n, &entries(&b, n).unwrap(), false).card.as_deref(),
        Some("Problem: a new decision")
    );
}

#[test]
fn human_replies_wrap_without_losing_words() {
    let short = format!("{}ok", "word ".repeat(53));
    let many = "word\n".repeat(400);
    assert_eq!((short.chars().count(), many.chars().count()), (267, 2000));
    for kind in ["answer", "steer", "requeue"] {
        for text in [
            &short,
            &many,
            &"🦀".repeat(481),
            &format!("{}\n\nlast paragraph", "word ".repeat(80)),
        ] {
            let b = Fake::default();
            let n = b.create("Reply fixture", "", &["taskq".into()]).unwrap();
            let attempt = b
                .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
                .unwrap();
            if kind == "answer" {
                b.comment(n, &entry("ask", &[], Some(attempt), "Problem: reply"))
                    .unwrap();
            }
            let before = b.comments(n).unwrap().len();
            let op = Op {
                id: Some("reply-once".into()),
                revision: None,
            };
            let receipt = act(&b, Path::new(""), n, kind, &[], text, &op);
            assert!(receipt.ok(), "{kind}: {}", receipt.text());
            let log = entries(&b, n).unwrap();
            let stored = log
                .last()
                .unwrap()
                .body
                .split_once('\n')
                .unwrap()
                .1
                .rsplit_once("\nop-id:")
                .unwrap()
                .0
                .trim();
            assert!(
                stored.lines().all(|l| l.chars().count() <= 240),
                "{kind}: {stored}"
            );
            assert_eq!(
                stored.split_whitespace().collect::<String>(),
                text.split_whitespace().collect::<String>()
            );
            if text.contains(' ') || text.contains("word") {
                assert_eq!(
                    stored.split_whitespace().collect::<Vec<_>>(),
                    text.split_whitespace().collect::<Vec<_>>()
                );
            }
            if text.contains("\n\n") {
                assert!(stored.contains("\n\nlast paragraph"));
            }
            assert!(act(&b, Path::new(""), n, kind, &[], text, &op).ok());
            assert_eq!(
                b.comments(n).unwrap().len(),
                before + 1,
                "same operation never writes twice"
            );
        }
    }
    for text in ["", " \n "] {
        for kind in ["answer", "steer"] {
            assert!(
                crate::cli::text::task_text(kind, text)
                    .unwrap_err()
                    .contains("type a reply")
            );
        }
    }
    let b = Fake::default();
    let n = b.create("Reserved suffix", "", &["taskq".into()]).unwrap();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let text = format!("{} op-id: forged", "x".repeat(239));
    assert!(
        !act(&b, Path::new(""), n, "steer", &[], &text, &Op::default()).ok(),
        "wrapping cannot inject metadata"
    );
    assert_eq!(b.comments(n).unwrap().len(), 1);
}

// Chain: committed board mutation -> failed wake -> same op retry; never duplicate the board write.
#[test]
fn wake_failure_preserves_acceptance_and_retry_only_notifies() {
    let b = Fake::default();
    let root = std::env::temp_dir().join(format!("taskq-wake-receipt-{}", std::process::id()));
    let path = crate::paths::dot(&root).join("poke");
    std::fs::create_dir_all(&path).unwrap();
    let n = b
        .create("Wake fixture", "## Goal\ng", &["taskq".into()])
        .unwrap();
    let op = Op {
        id: Some("hold-once".into()),
        revision: None,
    };
    let first = act(&b, &root, n, "hold", &[], "", &op);
    assert!(first.ok(), "{}", first.text());
    assert!(first.text().contains("service wake failed"));
    assert!(first.json().contains("wake-open"));
    let before = entries(&b, n).unwrap();
    assert_eq!(first.state, Some("held"));
    std::fs::remove_dir(&path).unwrap();
    let retry = act(&b, &root, n, "hold", &[], "", &op);
    assert!(retry.ok());
    assert!(retry.reason.is_empty());
    assert_eq!(retry.revision, first.revision);
    assert_eq!(entries(&b, n).unwrap().len(), before.len());
    assert!(path.is_file());
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: § 5 stale child environment (#1072). Risk: a reused session's TASKQ_TASK/TASKQ_ATTEMPT writes unfenced or ignored.
#[test]
fn stale_child_environment_is_refused() {
    let s = |k: &str, task: Option<&str>, attempt: Option<&str>| {
        stale_child(
            7,
            k,
            Some(42),
            &(task.map(Into::into), attempt.map(Into::into)),
        )
    };
    assert_eq!(s("requeue", Some("7"), Some("42")), None);
    assert_eq!(s("requeue", None, None), None); // the owner, not a child
    assert_eq!(s("hold", Some("9"), Some("1")), None); // a child may add and hold another task
    assert_eq!(
        s("requeue", Some("9"), Some("42")).unwrap(),
        "TASKQ_TASK \"9\" is not #7: stale session environment"
    );
    assert_eq!(
        s("result", None, Some("42")).unwrap(),
        "TASKQ_TASK \"\" is not #7: stale session environment"
    );
    assert_eq!(
        s("ask", Some("7"), Some("41")).unwrap(),
        "TASKQ_ATTEMPT 41 is not the live attempt 42: stale session environment"
    );
    assert!(
        stale_child(7, "gone", None, &(Some("7".into()), Some("42".into())))
            .unwrap()
            .contains("live attempt none")
    );
}

// Chain: § 5 op-id replay over § 3 fencing (#1072). Risk: retrying an accepted requeue, whose write cleared the
// attempt, is refused as stale instead of returning the first receipt.
#[test]
fn retried_child_op_id_returns_the_first_receipt_after_its_attempt_clears() {
    let (b, root) = (
        Fake::default(),
        std::env::temp_dir().join(format!("taskq-replay-{}", std::process::id())),
    );
    let n = b
        .create(
            "T",
            "## Goal
g",
            &["taskq".into()],
        )
        .unwrap();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
    let env = (Some(n.to_string()), Some(a.to_string()));
    let op = |id: &str| Op {
        id: Some(id.into()),
        revision: None,
    };
    let count = || b.comments(n).unwrap().len();
    let first = act_child(
        &b,
        &root,
        n,
        "requeue",
        &[],
        "blocked",
        &op("rq"),
        env.clone(),
    );
    assert_eq!(
        (first.outcome, first.state),
        ("accepted", Some("ready")),
        "{}",
        first.text()
    );
    assert_eq!(fold(n, &entries(&b, n).unwrap(), false).attempt, None);
    let before = count();
    let again = act_child(
        &b,
        &root,
        n,
        "requeue",
        &[],
        "blocked",
        &op("rq"),
        env.clone(),
    );
    assert_eq!(
        (again.outcome, again.state, again.revision, count()),
        ("accepted", Some("ready"), first.revision, before)
    );
    // a new op from the same, now stale, environment is still refused by name
    let other = act_child(&b, &root, n, "requeue", &[], "blocked", &op("rq2"), env);
    assert_eq!((other.outcome, count()), ("rejected", before));
    assert!(
        other.text().contains("is not the live attempt none"),
        "{}",
        other.text()
    );
}

// Chain: § 7 superseded close (pilotq#90). Risk: #90 closed for #104 while #104 stayed held with nobody to start it.
#[test]
fn superseded_close_starts_a_held_successor_or_refuses() {
    let (b, root) = (
        Fake::default(),
        std::env::temp_dir().join(format!("taskq-successor-{}", std::process::id())),
    );
    let held = || {
        let m = b
            .create("Successor", "## Goal\ng", &["q-held".into()])
            .unwrap();
        b.comment(m, &entry("hold", &[], None, "")).unwrap();
        m
    };
    let (old, m) = (
        b.create("Old", "## Goal\ng", &["taskq".into()]).unwrap(),
        held(),
    );
    b.comment(old, &entry("later", &[], None, "parked"))
        .unwrap();
    let verdict = format!("Changed: none; #{old} closes as superseded by #{m}\nChecks: A1 unknown");
    // an agent never starts work: it asks instead, and nothing is written to either task
    let before = (b.comments(old).unwrap().len(), b.comments(m).unwrap().len());
    let r = successor_moving(&b, &root, old, &verdict, &Op::default(), true).unwrap_err();
    assert!(
        r.reason.contains("is held") && r.next.contains(&format!("start #{m}")),
        "{}",
        r.text()
    );
    assert_eq!(
        (b.comments(old).unwrap().len(), b.comments(m).unwrap().len()),
        before
    );
    // the owner's or PM's close starts it first, then closes
    successor_moving(&b, &root, old, &verdict, &Op::default(), false).unwrap();
    let (issue, t, _) = current(&b, m).unwrap();
    assert_eq!((t.state, service::is_task(&issue)), (State::Ready, true));
    assert_eq!(
        act(&b, &root, old, "close", &[], &verdict, &Op::default()).state,
        Some("closed")
    );
    assert_eq!(
        crate::display::close_outcome(Some(&verdict), Some(t.state.name())),
        format!("replaced by #{m} (ready)")
    );
    // a moving successor needs nothing; one closed without completing refuses
    successor_moving(&b, &root, old, &verdict, &Op::default(), true).unwrap();
    b.close(m).unwrap();
    successor_moving(&b, &root, old, &verdict, &Op::default(), true).unwrap();
    assert!(
        successor_moving(
            &b,
            &root,
            old,
            "Changed: replaced by #9999",
            &Op::default(),
            false
        )
        .unwrap_err()
        .reason
        .contains("unreadable")
    );
}

// Chain: § 1 admission over § 2 `requeue` (#1735). Risk: requeue of a legacy `q-later` issue without `taskq`
// prints `ready`, yet status hides it and no service spawns it.
#[test]
fn requeue_without_task_label_is_refused_by_name() {
    let (b, root) = (
        Fake::default(),
        std::env::temp_dir().join(format!("taskq-unlabelled-{}", std::process::id())),
    );
    let n = b
        .create("Legacy", "## Goal\ng", &["q-later".into()])
        .unwrap();
    b.comment(n, &entry("later", &[], None, "parked")).unwrap();
    let before = b.comments(n).unwrap().len();
    let r = act(&b, &root, n, "requeue", &[], "", &Op::default());
    assert_eq!(
        (r.outcome, r.state, b.comments(n).unwrap().len()),
        ("rejected", Some("later"), before),
        "{}",
        r.text()
    );
    assert!(r.text().contains("lacks the taskq label"), "{}", r.text());
    assert_eq!(b.issue(n).unwrap().labels, ["q-later"]);
    b.labels(n, &["taskq".into()], &[]).unwrap();
    let r = act(&b, &root, n, "requeue", &[], "", &Op::default());
    assert_eq!(
        (r.outcome, r.state),
        ("accepted", Some("ready")),
        "{}",
        r.text()
    );
    assert!(service::is_task(&b.issue(n).unwrap()));
}

// Chain: § 2 retry limit to the owner's answer (#913, #1123). Risk: a reviewer's 3rd requeue after a lost double
// claim parks the task in `ask` on a card without options, so no answer moves it.
#[test]
fn retry_limit_requeue_asks_with_requeue_and_later_options() {
    let (b, root) = (
        Fake::default(),
        std::env::temp_dir().join(format!("taskq-limit-{}", std::process::id())),
    );
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
    let (cfg, _) = crate::config::load(&root).unwrap();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let at_limit = |reviewers: &[&str]| {
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        for _ in 0..2 {
            let a = b
                .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
                .unwrap();
            let r = act_child(
                &b,
                &root,
                n,
                "requeue",
                &[],
                "blocked",
                &Op::default(),
                (Some(n.to_string()), Some(a.to_string())),
            );
            assert_eq!(r.state, Some("ready"), "{}", r.text());
        }
        let t = fold(n, &entries(&b, n).unwrap(), false);
        assert_eq!(
            (t.retries, t.card),
            (2, None),
            "a requeue below the limit carries no card"
        );
        let a = b
            .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
            .unwrap();
        b.comment(n, &entry("result", &[sha], Some(a), "ok"))
            .unwrap();
        // both hosts spawn a reviewer for this result; the first wins, the second is ignored
        let ids: Vec<u64> = reviewers
            .iter()
            .map(|h| {
                b.comment(n, &entry("spawn", &["reviewer", "codex:v", h], None, ""))
                    .unwrap()
            })
            .collect();
        assert_eq!(
            fold(n, &entries(&b, n).unwrap(), false).attempt,
            Some(ids[0])
        );
        let r = act_child(
            &b,
            &root,
            n,
            "requeue",
            &[],
            "Fix:
1. x",
            &Op::default(),
            (Some(n.to_string()), Some(ids[0].to_string())),
        );
        assert_eq!(r.state, Some("ask"), "{}", r.text());
        n
    };
    let n = at_limit(&["win", "mac"]);
    let t = fold(n, &entries(&b, n).unwrap(), false);
    let card = t.card.unwrap();
    assert_eq!(
        (t.retries, card.lines().take(4).collect::<Vec<_>>()),
        (
            3,
            vec![
                "Fix:",
                "1) x",
                "Done: the service stopped retrying; no new attempt starts until the owner answers.",
                "Problem: retry limit 3 reached by this requeue; the task waits for the owner."
            ]
        )
    );
    assert_eq!(
        options(&card)
            .into_iter()
            .map(|o| (o.0, o.1.split(':').next().unwrap().to_string()))
            .collect::<Vec<_>>(),
        [(1, "requeue".into()), (2, "later".into())]
    );
    assert!(b.issue(n).unwrap().labels.contains(&"q-ask".to_string()));
    assert_eq!(card.lines().last(), Some(format!("To answer: `taskq answer {n}.K`; a bare comment `K` is a plain answer, never `close` or `later`.").as_str()));
    // `taskq answer N.2` parks the task; a bare board comment `2` is a plain answer to `ready` (§ 2)
    let r = answer_picks(&b, &cfg, &[format!("{n}.2")], &Op::default())
        .unwrap()
        .remove(0);
    assert_eq!(
        (r.outcome, r.state),
        ("accepted", Some("later")),
        "{}",
        r.text()
    );
    let n = at_limit(&["win", "mac"]);
    b.comment(n, "2").unwrap();
    assert_eq!(fold(n, &entries(&b, n).unwrap(), false).state, State::Ready);
    // `requeue` retries it with a fresh attempt
    let n = at_limit(&["mac", "win"]);
    let r = answer_picks(&b, &cfg, &[format!("{n}.1")], &Op::default())
        .unwrap()
        .remove(0);
    assert_eq!(
        (r.outcome, r.state),
        ("accepted", Some("ready")),
        "{}",
        r.text()
    );
    assert_eq!(fold(n, &entries(&b, n).unwrap(), false).retries, 0);
}

// Chain: § 2 PM pick. Risk: a pick lands on a closed card, names no option, or becomes the answer.
#[test]
fn pm_pick_records_a_view_and_refuses_bad_input() {
    let (b, root) = (
        Fake::default(),
        std::env::temp_dir().join(format!("taskq-pick-{}", std::process::id())),
    );
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    let pick = |k: &str, r: &str| pm_pick(&b, &root, &format!("{n}.{k}"), r, &Op::default());
    assert!(
        pick("1", "cheaper").is_err_and(|e| e.contains("without an open question")),
        "no card yet"
    );
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
    b.comment(n, &entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
        .unwrap();
    let count = || b.comments(n).unwrap().len();
    let before = count();
    assert!(pick("3", "r").is_err_and(|e| e.contains("no option 3")));
    assert!(
        pick("1", " ").is_err()
            && pick("1", "a\nb").is_err()
            && pick("1", &"x".repeat(161)).is_err()
    );
    assert_eq!(count(), before, "refusals write nothing");
    let r = pick("1", "x is cheaper").unwrap();
    assert_eq!(
        (r.outcome, r.state),
        ("accepted", Some("ask")),
        "{}",
        r.text()
    );
    assert_eq!(
        fold(n, &entries(&b, n).unwrap(), false).pick,
        Some((1, "x is cheaper".into()))
    );
    b.close(n).unwrap();
    assert!(pick("1", "x").is_err_and(|e| e.contains("closed")));
}

// Chain: § 2 § 5 owner send -> board readback -> durable note -> attempt inbox.
// Risk: a race drops intent, reuses a new option number, bypasses other revisions or duplicates delivery.
#[test]
fn late_answers_preserve_meaning_and_reconcile_once() {
    let b = Fake::default();
    let root = std::env::temp_dir().join(format!("taskq-notes-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
    let (cfg, _) = crate::config::load(&root).unwrap();
    let n = b
        .create(
            "T",
            "## Goal\nkeep safe\n\n## Acceptance\nA1. no loss",
            &["taskq".into()],
        )
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:s win").unwrap();
    let card = b
        .comment(
            n,
            &entry("ask", &[], Some(a), "Problem: Which?\n1. old x\n2. old y"),
        )
        .unwrap();
    let op = |id: &str, r| Op {
        id: Some(id.into()),
        revision: Some(r),
    };
    let count = || b.comments(n).unwrap().len();
    let before = count();
    assert_eq!(
        act(
            &b,
            &root,
            n,
            "requeue",
            &[],
            "Reason: retry",
            &op("stale-action", a)
        )
        .outcome,
        "stale"
    );
    assert_eq!(
        count(),
        before,
        "non-answer stale actions still write nothing"
    );
    assert_eq!(
        act(&b, &root, n, "answer", &["1"], "", &op("plain", card)).outcome,
        "accepted"
    );
    for (_, c) in b
        .state
        .borrow_mut()
        .comments
        .iter_mut()
        .filter(|(i, _)| *i == n)
    {
        c.author = "alex".into();
    }
    let late = act(&b, &root, n, "answer", &["2"], "", &op("late", card));
    assert_eq!((late.outcome, late.state), ("accepted", Some("doing")));
    assert_eq!(late.answered.as_ref().unwrap().text, "old x");
    assert!(
        late.reason.contains("task note") && !late.reason.contains("old x"),
        "{}",
        late.reason
    );
    assert_eq!(
        late.text().matches("old x").count(),
        1,
        "#1861: the receipt names the winner once
{}",
        late.text()
    );
    assert!(late.text().contains("already answered by alex: old x"));
    let written = count();
    for (id, args, text) in [("same", &["1"][..], ""), ("same-text", &[][..], " old  x ")] {
        let same = act(&b, &root, n, "answer", args, text, &op(id, card));
        assert_eq!(
            (same.outcome, same.state, same.reason.as_str()),
            ("accepted", Some("doing"), "Same answer; nothing saved"),
            "{id}"
        );
        assert_eq!(
            same.text().matches("old x").count(),
            1,
            "{id}: {}",
            same.text()
        );
        assert_eq!(same.answered.as_ref().map(|a| a.by.as_str()), Some("alex"));
    }
    assert_eq!(
        count(),
        written,
        "#1861: a repeat of the winning answer saves no note and steers nothing"
    );
    let log = entries(&b, n).unwrap();
    assert_eq!(
        core::delivery(n, &log, *fold(n, &log, false).inbox.last().unwrap()),
        "old y"
    );
    let written = count();
    for id in ["plain", "late"] {
        assert!(act(&b, &root, n, "answer", &["2"], "", &op(id, card)).ok());
    }
    assert_eq!(
        count(),
        written,
        "lost receipts reconcile the same ID without another note"
    );
    let newer = b
        .comment(
            n,
            &entry(
                "ask",
                &[],
                Some(a),
                "Problem: New question\n1. new x\n2. new y",
            ),
        )
        .unwrap();
    let r = answer_picks(&b, &cfg, &[format!("{n}.2")], &op("old-choice", card))
        .unwrap()
        .remove(0);
    assert_eq!(
        (r.outcome, r.state, r.revision),
        ("accepted", Some("ask"), Some(newer))
    );
    assert!(
        r.answered.is_none(),
        "a newer unanswered question has no invented winner"
    );
    let log = entries(&b, n).unwrap();
    assert_eq!(
        core::delivery(
            n,
            &log,
            *core::project(n, &log, false).notes.last().unwrap()
        ),
        "old y"
    );
    assert!(fold(n, &log, false).card.unwrap().contains("New question"));
    assert_eq!(
        act(
            &b,
            &root,
            n,
            "answer",
            &["9"],
            "",
            &op("invalid-choice", card)
        )
        .outcome,
        "rejected"
    );
    assert_eq!(
        act(
            &b,
            &root,
            n,
            "answer",
            &["1"],
            "",
            &op("unknown-question", a)
        )
        .outcome,
        "rejected"
    );
    assert_eq!(
        act(
            &b,
            &root,
            n,
            "steer",
            &[],
            "text\nop-id: spoof",
            &Op::default()
        )
        .outcome,
        "rejected"
    );
    assert_eq!(
        act(&b, &root, n, "result", &["abc"], "", &Op::default()).outcome,
        "rejected"
    );
    b.state.borrow_mut().race = Some(core::with_fields(
        entry("answer", &["1"], None, "new x"),
        Some("winner"),
        Some(newer),
    ));
    let r = act(&b, &root, n, "answer", &["2"], "", &op("raced", newer));
    assert_eq!((r.outcome, r.state), ("accepted", Some("doing")));
    let log = entries(&b, n).unwrap();
    assert!(
        log.iter().any(|e| e.edited),
        "the losing answer is fenced before its note is written"
    );
    assert_eq!(
        core::delivery(n, &log, *fold(n, &log, false).inbox.last().unwrap()),
        "new y"
    );
    let written = count();
    assert!(act(&b, &root, n, "answer", &["2"], "", &op("raced", newer)).ok());
    assert_eq!(count(), written);
    let context = core::task_context(n, &b.issue(n).unwrap().body, &log);
    for text in [
        "A1. no loss",
        "old x",
        "old y",
        "new x",
        "new y",
        "reuse settled decisions within their scope",
        "Before hand-in",
    ] {
        assert!(context.contains(text), "{text}");
    }
    let v: serde_json::Value = serde_json::from_str(&r.json()).unwrap();
    assert_eq!(
        (v["task"].as_u64(), v["op_id"].as_str()),
        (Some(n), Some("raced"))
    );
    let third = b
        .comment(
            n,
            &entry(
                "ask",
                &[],
                Some(a),
                "Problem: Third
1. z
2. w",
            ),
        )
        .unwrap();
    b.state.borrow_mut().race = Some(core::with_fields(
        entry("answer", &["1"], None, "z"),
        Some("winner-z"),
        Some(third),
    ));
    let notes = |b: &Fake| {
        entries(b, n)
            .unwrap()
            .iter()
            .filter(|e| core::kind(n, &e.body) == "steer")
            .count()
    };
    let noted = notes(&b);
    let r = act(&b, &root, n, "answer", &["1"], "", &op("raced-same", third));
    assert_eq!(
        (r.outcome, r.reason.as_str()),
        ("accepted", "Same answer; nothing saved"),
        "{}",
        r.text()
    );
    assert_eq!(
        notes(&b),
        noted,
        "#1861: losing the race to the same answer writes no note"
    );
    b.close(n).unwrap();
    let written = count();
    assert_eq!(
        act(
            &b,
            &root,
            n,
            "answer",
            &["1"],
            "",
            &op("closed-same", third)
        )
        .outcome,
        "accepted"
    );
    let other = act(
        &b,
        &root,
        n,
        "answer",
        &["2"],
        "",
        &op("closed-other", third),
    );
    assert_eq!(
        (other.outcome, other.reason.as_str()),
        ("rejected", format!("#{n} is closed").as_str())
    );
    assert_eq!(count(), written, "a closed task takes no note");
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: owner answer plan -> concurrent board write -> note receipt -> no consequential continuation.
// Risk: a note authorizes close/publication, reuses a new question number, or disappears on a steer race.
#[test]
fn review_note_never_closes_or_publishes() {
    for action in ["close", "later"] {
        for pending in [false, true] {
            let b = Fake::default();
            let root =
                std::env::temp_dir().join(format!("taskq-note-close-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
            let (cfg, _) = crate::config::load(&root).unwrap();
            let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
            let a = b.comment(n, "taskq: spawn worker codex:s win").unwrap();
            b.comment(n, &entry("result", &[&"a".repeat(40)], Some(a), ""))
                .unwrap();
            let text = format!("Problem: Publish?\n1. {action}: ship\n2. keep reviewing");
            let card = if pending {
                b.comment(
                    n,
                    "taskq: publish\nChanged: accepted\nChecks: pending\nOpen: none",
                )
                .unwrap();
                b.comment(n, &entry("publication-blocked", &[], None, &text))
                    .unwrap()
            } else {
                let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
                b.comment(n, &entry("ask", &[], Some(r), &text)).unwrap()
            };
            b.state.borrow_mut().race = Some(entry("answer", &["2"], None, "keep reviewing"));
            let r = answer_picks(
                &b,
                &cfg,
                &[format!("{n}.1")],
                &Op {
                    id: Some("close-race".into()),
                    revision: Some(card),
                },
            )
            .unwrap()
            .remove(0);
            assert_eq!(r.note, Some(true));
            assert_eq!(
                fold(n, &entries(&b, n).unwrap(), false).state,
                State::Review,
                "a saved note cannot close"
            );
            assert_eq!(
                b.state.borrow().publications,
                0,
                "a saved note cannot authorize publication"
            );
            assert!(b.issue(n).unwrap().open);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn review_note_keeps_a_numbered_answers_original_snapshot() {
    let b = Fake::default();
    let root = std::env::temp_dir().join(format!("taskq-note-read-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
    let (cfg, _) = crate::config::load(&root).unwrap();
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:s win").unwrap();
    b.comment(
        n,
        &entry(
            "ask",
            &[],
            Some(a),
            "Problem: First?\n1. original meaning\n2. other",
        ),
    )
    .unwrap();
    let read = b.state.borrow().comment_reads + 1;
    b.state.borrow_mut().after_comment_read = Some((
        n,
        read,
        vec![
            entry("answer", &["2"], None, "other"),
            entry(
                "ask",
                &[],
                Some(a),
                "Problem: New question\n1. different meaning\n2. new other",
            ),
        ],
    ));
    let r = answer_picks(
        &b,
        &cfg,
        &[format!("{n}.1")],
        &Op {
            id: Some("read-race".into()),
            revision: None,
        },
    )
    .unwrap()
    .remove(0);
    assert_eq!(r.note, Some(true));
    let log = entries(&b, n).unwrap();
    let p = core::project(n, &log, false);
    assert_eq!(p.task.state, State::Ask);
    assert!(p.task.card.unwrap().contains("New question"));
    assert_eq!(
        core::delivery(n, &log, *p.notes.last().unwrap()),
        "original meaning"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn review_note_preserves_steers_across_question_races() {
    for during_write in [false, true] {
        let b = Fake::default();
        let root =
            std::env::temp_dir().join(format!("taskq-note-steer-{}", crate::runtime::uuid()));
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:s win").unwrap();
        let ask = entry("ask", &[], Some(a), "Problem: Still need a decision");
        if during_write {
            b.state.borrow_mut().race = Some(ask);
        } else {
            b.comment(n, &ask).unwrap();
        }
        let op = Op {
            id: Some("steer-race".into()),
            revision: Some(a),
        };
        let r = act(&b, &root, n, "steer", &[], "Keep acceptance in scope", &op);
        assert!(r.ok(), "{}", r.text());
        let log = entries(&b, n).unwrap();
        let p = core::project(n, &log, false);
        assert_eq!(p.task.state, State::Ask);
        assert_eq!(
            core::delivery(n, &log, *p.notes.last().unwrap()),
            "Keep acceptance in scope"
        );
        let count = log.len();
        assert!(act(&b, &root, n, "steer", &[], "Keep acceptance in scope", &op).ok());
        assert_eq!(entries(&b, n).unwrap().len(), count);
        assert!(
            !act_as(
                &b,
                &root,
                n,
                "steer",
                &[],
                "wrong worker",
                &Op::default(),
                Some(a + 99),
                None
            )
            .ok()
        );
    }
}

// §5/R2: host removal must not replace active ownership; stale/worker/replay paths must never mutate it.
#[test]
fn host_unpin_preserves_ready_and_active_attempts_and_fences_writes() {
    use crate::board::{Board, Fake};
    for active in [false, true] {
        let b = Fake::default();
        let n = b
            .create(
                "T",
                "unchanged body",
                &[
                    "taskq".into(),
                    "host-invented".into(),
                    "run-codex".into(),
                    "code".into(),
                ],
            )
            .unwrap();
        if active {
            let attempt = b
                .comment(n, "taskq: spawn worker codex:invented-session invented")
                .unwrap();
            b.comment(
                n,
                &format!("taskq: accepted {attempt} attempt {attempt}\nop-id: accept-{attempt}"),
            )
            .unwrap();
        }
        let log = super::entries(&b, n).unwrap();
        let rev = crate::core::revision(n, &log);
        let op = super::Op {
            id: Some("invented-unpin".into()),
            revision: Some(rev),
        };
        let before = super::current(&b, n).unwrap();
        let count = b.comments(n).unwrap().len();
        assert_eq!(
            super::unpin(&b, n, "invented", &op, true).outcome,
            "rejected"
        );
        assert_eq!(
            super::unpin(
                &b,
                n,
                "invented",
                &super::Op {
                    revision: Some(rev + 1),
                    ..op.clone()
                },
                false
            )
            .outcome,
            "stale"
        );
        assert_eq!(
            b.comments(n).unwrap().len(),
            count,
            "refusals write nothing"
        );
        assert!(super::unpin(&b, n, "invented", &op, false).ok());
        let (got, task, _) = super::current(&b, n).unwrap();
        assert_eq!(got.body, before.0.body);
        assert_eq!(got.labels, vec!["taskq", "run-codex", "code"]);
        assert_eq!(
            serde_json::to_value(task).unwrap(),
            serde_json::to_value(before.1).unwrap()
        );
        let count = b.comments(n).unwrap().len();
        assert!(super::unpin(&b, n, "invented", &op, false).ok());
        assert_eq!(b.comments(n).unwrap().len(), count, "repeat writes nothing");
        assert_eq!(super::unpin(&b, n, "other", &op, false).outcome, "rejected");
        assert_eq!(b.comments(n).unwrap().len(), count);
        let fresh = super::Op {
            id: Some("incomplete".into()),
            revision: Some(rev),
        };
        let input = format!(
            "taskq: host-unpin\n{}\nhost-unpin-op: incomplete",
            serde_json::json!({"task":n,"url":b.url(n),"host":"invented","revision":rev})
        );
        b.comment(n, &input).unwrap();
        let count = b.comments(n).unwrap().len();
        assert_eq!(
            super::unpin(&b, n, "invented", &fresh, false).outcome,
            "unknown"
        );
        assert_eq!(
            b.comments(n).unwrap().len(),
            count,
            "interrupted intent never replays"
        );
    }
}
