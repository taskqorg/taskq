// Chain: §3 (#2499): native refusal -> recovery card. Risk: access/corruption errors are mistaken for absent history.
#[test]
fn recovery_2499_transcript_absence_is_specific() {
    for reason in [
        "No such file or directory (os error 2)",
        "The system cannot find the path specified. (os error 3)",
        "Codex transcript unavailable",
    ] {
        assert!(
            missing_transcript(&format!("native baseline unavailable: {reason}")),
            "{reason}"
        );
    }
    for reason in [
        "Permission denied (os error 13)",
        "Access is denied. (os error 5)",
        "invalid JSON",
        "native read timed out",
    ] {
        let text = format!("native baseline unavailable: {reason}");
        assert!(!missing_transcript(&text), "{reason}");
        assert!(input_card(7, &text, 2).contains("retry the input"));
    }
}

// Chain: §2.10/§3 (#2499): prior failure -> confirmed reboot -> gone -> automatic replacement.
// Risk: the second gone parks rebooted work, or a replacement duplicates a native execution.
#[test]
fn recovery_2499_boot_gone_restarts_without_spending_retry() {
    let (b, c) = (Fake::default(), cfg("mac"));
    let n = b.create("Rebooted work", "", &[TASK_LABEL.into()]).unwrap();
    let old = b.comment(n, "taskq: spawn worker codex:old mac").unwrap();
    release_execution(&b, n, old, old, "no-start").unwrap();
    b.comment(n, &core::entry("gone", &[], Some(old), "ordinary failure"))
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    release_execution_text(
        &b,
        n,
        a,
        a,
        "exit",
        "host booted 2033-05-18 03:33 UTC after intent 1970-01-12 13:46 UTC",
    )
    .unwrap();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Doing);
    assert_eq!(task(&b, n).retries, 1);
    assert_ne!(task(&b, n).attempt, Some(a));
    assert_eq!(s.kids.len(), 1);
    let log = entries(&b, n).unwrap();
    let gone = log
        .iter()
        .find(|e| e.body.starts_with(&format!("taskq: gone attempt {a}")))
        .unwrap();
    assert!(!gone.body.contains("retry limit"));
    assert!(task(&b, n).card.is_none());
}

// Chain: §3/§7.2 (#2499): refused resume -> recovery choice -> new attempt, same dirty branch and notes.
// Risk: missing native history causes endless input retries or loses accepted owner context.
#[test]
fn recovery_2499_missing_transcript_offers_fresh_attempt() {
    for (answer, saved, quota) in [
        ("1", false, false),
        ("taskq: answer 1", false, true),
        ("1", true, false),
    ] {
        let (b, mut c) = (Fake::default(), cfg("mac"));
        let n = b
            .create("Lost transcript", "", &[TASK_LABEL.into()])
            .unwrap();
        let a = b
            .comment(n, "taskq: spawn worker codex:session mac")
            .unwrap();
        c.root = started_root("2499", n, a);
        let branch = c.root.join(format!(".worktrees/taskq-{n}"));
        std::fs::create_dir_all(&branch).unwrap();
        let dirty = branch.join("untracked.txt");
        std::fs::write(&dirty, "preserve work").unwrap();
        accept_execution(&b, n, a, a, None).unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        b.comment(
            n,
            &core::entry("ask", &[], Some(a), "Problem: choose\n1. keep the branch ★"),
        )
        .unwrap();
        let accepted = b.comment(n, "taskq: answer 1").unwrap();
        let note = b
            .comment(
                n,
                &core::entry(
                    "steer",
                    if quota { &["quota"] } else { &[] },
                    quota.then_some(a),
                    "keep this note",
                ),
            )
            .unwrap();
        if saved {
            let execution = b
                .comment(
                    n,
                    &core::entry("executing", &[&note.to_string()], Some(a), ""),
                )
                .unwrap();
            release_execution_text(&b, n, a, execution, "no-start", LOST_TRANSCRIPT).unwrap();
        }
        let calls = Rc::new(RefCell::new(0));
        let seen = calls.clone();
        let mut s = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(move |l| {
                assert!(!saved, "saved no-start must ask without launching");
                *seen.borrow_mut() += 1;
                assert!(l.resume.is_some());
                Err("native baseline unavailable: No such file or directory (os error 2)".into())
            }),
        );
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Ask);
        let card = task(&b, n).card.unwrap();
        let recommended = core::options(&card).into_iter().find(|o| o.2).unwrap().1;
        assert!(recommended.starts_with("requeue:"), "{card}");
        assert!(recommended.contains("existing branch"), "{card}");
        assert!(!card.contains("retry the input"), "{card}");
        let attempts = *calls.borrow();
        s.pass().unwrap();
        assert_eq!(*calls.borrow(), attempts, "no launch while ask waits");
        drop(s);
        b.comment(n, answer).unwrap();
        let mut fresh = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(|l| {
                assert!(
                    l.resume.is_none(),
                    "fresh attempt must not replay old input"
                );
                Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))))
            }),
        );
        fresh.focus = vec!["all".into()];
        fresh.pass().unwrap();
        fresh.pass().unwrap();
        assert_eq!(fresh.kids.len(), 1);
        assert_ne!(task(&b, n).attempt, Some(a));
        assert_eq!(std::fs::read_to_string(&dirty).unwrap(), "preserve work");
        let p = core::project(n, &entries(&b, n).unwrap(), false);
        assert!(p.notes.contains(&accepted) && p.notes.contains(&note));
        let context = core::task_context(n, "## Goal\nPreserve work", &entries(&b, n).unwrap());
        assert!(context.contains("keep this note") && context.contains("keep the branch"));
        assert!(
            p.executions
                .iter()
                .filter(|x| x.attempt == a)
                .all(|x| x.release.is_some())
        );
        drop(fresh);
        std::fs::remove_dir_all(c.root).unwrap();
    }
}

use super::*;
use crate::board::{Board, Fake, entries};
use std::cell::RefCell;
use std::rc::Rc;

// Chain: §2.27/R6: dependency completion -> restart reconciliation -> saved publication, without an answer or launch.
#[test]
fn dependency_wait_2469_service_resumes_publication_after_verified_close() {
    let (b, c) = (Fake::default(), cfg("win"));
    let m = b
        .create("2468 prerequisite", "", &["taskq".into()])
        .unwrap();
    let n = b
        .create(
            "2462 saved publication",
            &format!("Depends on #{m}"),
            &["taskq".into()],
        )
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:invented win")
        .unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    b.comment(
        n,
        "taskq: publish\nChanged: proof retained\nChecks: A1 ok\nOpen: none",
    )
    .unwrap();
    {
        let mut s = Service::new(
            &b,
            &c,
            "test",
            Box::new(|_| panic!("dependency wait cannot launch")),
        );
        s.pass().unwrap();
    }
    assert_eq!(task(&b, n).state, State::Ask);
    assert!(task(&b, n).wait.is_some());
    assert_eq!(b.state.borrow().publications, 0);
    b.close(m).unwrap();
    b.advance(100);
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("saved publication cannot launch")),
    );
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Review);
    assert!(task(&b, n).publication.is_some());
    b.advance(100);
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Closed);
    let comments = b.comments(n).unwrap();
    assert_eq!(
        comments
            .iter()
            .filter(|e| core::kind(n, &e.body) == "spawn")
            .count(),
        1
    );
    assert!(
        !comments
            .iter()
            .any(|e| matches!(core::kind(n, &e.body), "answer" | "gone" | "requeue"))
    );
    assert_eq!(b.state.borrow().publications, 1);
    assert!(b.state.borrow().cancelled_publications.is_empty());
}

struct FakeProc(Rc<RefCell<Option<i32>>>);
impl Proc for FakeProc {
    fn daemon(&self) -> bool {
        true
    }
    fn accepted(&mut self) -> Result<bool, String> {
        Ok(true)
    }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        // -999: a failed read
        match *self.0.borrow() {
            Some(-999) => Err("read failed".into()),
            c => Ok(c),
        }
    }
    fn kill(&mut self) -> Result<(), String> {
        *self.0.borrow_mut() = Some(-9);
        Ok(())
    }
    // a live owned child: nonzero like any OS process (#2228: pid 0 is a turn with no observed process)
    fn id(&self) -> u32 {
        1
    }
}

struct UnknownExit;
impl Proc for UnknownExit {
    fn accepted(&mut self) -> Result<bool, String> {
        Ok(true)
    }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        Err("native exit code unknown".into())
    }
    fn termination(&mut self) -> Result<Option<crate::runtime::Exit>, String> {
        Ok(Some(crate::runtime::Exit { code: None }))
    }
    fn refused(&self, _: u64, _: u64, _: u64, _: i32) -> Option<String> {
        panic!("unknown numeric exit cannot prove pre-submission refusal")
    }
    fn kill(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn id(&self) -> u32 {
        1
    }
}

// Chain: § 2 visual evidence -> service copy -> tray inline (#1782). Risk: the app, which has no board token, never
// gets the image of an open ask, or keeps copies of answered asks.
#[test]
fn open_ask_images_are_copied_for_the_app_and_dropped_after() {
    let root = std::env::temp_dir().join(format!("taskq-1782-images-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let b = Fake::default();
    let mut c = cfg("mac");
    c.root = root.clone();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    let png = b"\x89PNG\r\n\x1a\nimage".to_vec();
    let url = "https://board.example/1/0-front.png".to_string();
    b.state.borrow_mut().images.push((url.clone(), png.clone()));
    b.comment(n, &format!("taskq: ask attempt {a}\nDone: d\nProblem: p\nEvidence: ![front]({url}), ![gone](https://board.example/1/9-x.png)\nTo answer: `taskq answer {n} --text <done>`.")).unwrap();
    let mut s = Service::new(&b, &c, "fixture", Box::new(|_| panic!("no launch")));
    s.pass().unwrap();
    assert_eq!(
        std::fs::read(crate::snapshot::image_copy(&root, &url)).unwrap(),
        png
    );
    assert_eq!(
        std::fs::read(crate::snapshot::image_copy(
            &root,
            "https://board.example/1/9-x.png"
        ))
        .unwrap(),
        b"",
        "a failed read leaves the link"
    );
    #[cfg(any(target_os = "macos", windows))]
    {
        let mut task = json!({"question": crate::snapshot::question(&format!("Done: d\nProblem: p\nEvidence: ![front]({url})"))});
        crate::tray::inline_images(&root, &mut task);
        assert_eq!(
            task["question"]["evidence"][0]["src"],
            "data:image/png;base64,iVBORw0KGgppbWFnZQ=="
        );
    }
    b.comment(n, "taskq: answer\nok").unwrap();
    s.pass().unwrap();
    assert!(
        !crate::snapshot::image_copy(&root, &url).exists(),
        "answered ask: copy removed"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

// Chain: #1386 boot release. Risk: a dead pre-boot execution holds capacity forever, or a live one is released.
#[test]
fn boot_after_intent_releases_only_this_hosts_older_executions() {
    let after: fn() -> Result<i64, String> = || Ok(2_000_000_000);
    let before: fn() -> Result<i64, String> = || Ok(500_000);
    let failed: fn() -> Result<i64, String> = || Err("sysctl kern.boottime failed".into());
    for (case, boot, host) in [
        ("after", after, "mac"),
        ("before", before, "mac"),
        ("failed", failed, "mac"),
        ("foreign", after, "win"),
    ] {
        let b = Fake::default();
        b.advance(1_000_000);
        let mut c = cfg("mac");
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let a = b
            .comment(n, &format!("taskq: spawn worker codex:session {host}"))
            .unwrap();
        c.root = started_root("boot", n, a);
        let mut s = Service::new(&b, &c, "fixture", Box::new(|_| panic!("no launch")));
        s.boot = boot;
        s.pass().unwrap();
        s.pass().unwrap();
        let log = entries(&b, n).unwrap();
        let releases: Vec<_> = log
            .iter()
            .filter(|e| e.body.starts_with("taskq: released"))
            .collect();
        if case == "after" {
            assert_eq!(releases.len(), 1, "one release over two passes");
            assert_eq!(
                releases[0].body,
                format!(
                    "taskq: released {a} exit attempt {a}\nhost booted 2033-05-18 03:33 UTC after intent 1970-01-12 13:46 UTC\nop-id: release-{a}"
                )
            );
            assert_eq!(
                core::executions(n, &log)[0].release,
                Some(core::Release::Exit)
            );
        } else {
            assert!(releases.is_empty(), "{case}");
            assert!(
                s.execution_waits[&n].contains("termination unconfirmed"),
                "{case}"
            );
        }
    }
}

// Chain: saved refused launch -> no-start release -> admission. Risk: a refused Claude launch blocks its task forever (#1557).
#[test]
fn historical_claude_diagnostics_never_release_or_replay() {
    for case in ["requeued", "doing", "review"] {
        let root = std::env::temp_dir().join(format!("taskq-1557-{case}-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let b = Fake::default();
        let mut c = cfg("mac");
        c.root = root.clone();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let worker = b
            .comment(n, "taskq: spawn worker claude:session mac")
            .unwrap();
        let (a, role) = if case == "review" {
            b.comment(
                n,
                &format!("taskq: result {} attempt {worker}", "a".repeat(40)),
            )
            .unwrap();
            release_execution(&b, n, worker, worker, "exit").unwrap();
            (
                b.comment(n, "taskq: spawn reviewer claude:session mac")
                    .unwrap(),
                "reviewer",
            )
        } else {
            (worker, "worker")
        };
        if case == "requeued" {
            b.comment(n, "taskq: requeue\ntrust set").unwrap();
        }
        std::fs::write(
            crate::runtime::attempt_log(&root, role, n, a),
            format!(
                "{}\n{}\n",
                json!({"type":"taskq.input","execution":a}),
                json!({"type":"claude.launch","stdout":"","stderr":"Workspace not trusted"})
            ),
        )
        .unwrap();
        let launched = Rc::new(RefCell::new(0));
        let count = launched.clone();
        let mut s = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(move |_| {
                *count.borrow_mut() += 1;
                if case != "requeued" && *count.borrow() <= 2 {
                    Err(crate::runtime::StartError::trust_setup())
                } else {
                    Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))))
                }
            }),
        );
        s.recover = Some(Box::new(|_, _| {
            Err("canonical native proof unavailable; input remains unknown".into())
        }));
        s.focus = vec!["all".into()];
        for _ in 0..4 {
            let _ = s.pass();
            b.advance(60);
        }
        let log = entries(&b, n).unwrap();
        assert_eq!(
            *launched.borrow(),
            0,
            "{case}: historical text cannot authorize new work"
        );
        assert_eq!(
            core::executions(n, &log).last().unwrap().release,
            None,
            "{case}: no-start needs qualified canonical proof"
        );
        assert!(
            !log.iter()
                .any(|entry| entry.body.starts_with("taskq: gone")),
            "historical diagnostics cannot retire an uncertain execution"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

// Chain: cold service -> board ownership -> observer -> terminal receipt. Risk: recovery replays an uncertain submit.
#[test]
fn cold_native_observers_never_use_the_launch_path() {
    for rt in ["claude", "codex-app"] {
        for case in [
            "live",
            "unknown",
            "missing",
            "foreign",
            "released",
            "ambiguous",
        ] {
            let b = Fake::default();
            let mut c = cfg("mac");
            let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
            let host = if case == "foreign" { "other" } else { "mac" };
            let a = b
                .comment(n, &format!("taskq: spawn worker {rt}:session {host}"))
                .unwrap();
            c.root = started_root("cold", n, a);
            if case == "released" {
                release_execution(&b, n, a, a, "exit").unwrap();
            }
            if case == "ambiguous" {
                b.comment(
                    n,
                    &core::entry("gone", &[], Some(a), "legacy gone without native exit"),
                )
                .unwrap();
                b.comment(n, &format!("taskq: spawn worker {rt}:second mac"))
                    .unwrap();
                assert_eq!(
                    core::executions(n, &entries(&b, n).unwrap())
                        .iter()
                        .filter(|x| x.release.is_none())
                        .count(),
                    2
                );
            }
            let seen = Rc::new(RefCell::new(Vec::new()));
            let calls = seen.clone();
            let exit = Rc::new(RefCell::new(if case == "unknown" {
                Some(-999)
            } else {
                None
            }));
            let flag = exit.clone();
            let mut s = Service::new(
                &b,
                &c,
                "fixture",
                Box::new(|_| panic!("recovery must never launch")),
            );
            s.recover = Some(Box::new(move |task, x| {
                calls
                    .borrow_mut()
                    .push((task, x.attempt, x.id, x.runtime.clone()));
                if case == "missing" {
                    Err("original input unavailable; inspect native submission".into())
                } else {
                    Ok(Box::new(FakeProc(flag.clone())))
                }
            }));
            s.pass().unwrap();
            s.pass().unwrap();
            if matches!(case, "foreign" | "released" | "ambiguous") {
                assert!(seen.borrow().is_empty(), "{rt}: {case}");
                if case == "ambiguous" {
                    assert!(s.execution_waits[&n].contains("multiple unresolved"));
                }
            } else if case == "missing" {
                assert_eq!(seen.borrow().len(), 2, "only observation is retried");
                assert!(s.kids.is_empty());
                assert!(s.reserved.iter().any(|r| r.0 == n && r.1 == a));
                assert!(s.execution_waits[&n].contains("original input unavailable"));
                assert!(
                    s.snapshot
                        .as_ref()
                        .unwrap()
                        .tasks
                        .iter()
                        .find(|r| r.n == n)
                        .unwrap()
                        .execution_wait
                        .as_ref()
                        .unwrap()
                        .contains("original input unavailable")
                );
            } else {
                assert_eq!(*seen.borrow(), vec![(n, a, a, rt.to_owned())]);
                assert_eq!(s.kids.len(), 1);
                assert!(
                    core::executions(n, &entries(&b, n).unwrap())[0]
                        .release
                        .is_none()
                );
                if case == "unknown" {
                    assert_eq!(
                        s.kids
                            .iter()
                            .find(|k| k.attempt == a)
                            .unwrap()
                            .unknown
                            .as_deref()
                            .unwrap(),
                        "read failed"
                    );
                }
                *exit.borrow_mut() = Some(0);
                s.pass().unwrap();
                assert!(s.kids.is_empty());
                assert_eq!(
                    core::executions(n, &entries(&b, n).unwrap())[0].release,
                    Some(core::Release::Exit)
                );
            }
        }
    }
}

// Chain: re-exec during pending acceptance -> exact execution. Risk: the old delivered cursor adopts execution zero.
#[test]
fn handover_uses_execution_identity_before_input_acceptance() {
    let b = Fake::default();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let c = cfg("mac");
    let a = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    let input = b.comment(n, "taskq: steer\ncontinue").unwrap();
    let execution = b
        .comment(
            n,
            &core::entry("executing", &[&input.to_string()], Some(a), ""),
        )
        .unwrap();
    let mut s = service(&b, &c, Default::default());
    s.adopt(Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution,
        task: n,
        attempt: a,
        rt: "codex".into(),
        proc: Box::new(FakeProc(Rc::new(RefCell::new(None)))),
    })
    .unwrap();
    assert_eq!(
        s.kids[0].execution, execution,
        "pending acceptance does not change native ownership"
    );
    assert_eq!(
        s.delivered(a),
        a,
        "handover does not invent input acceptance"
    );
}

// Chain: #2221 resumed Claude turn hands in, its transcript readback never matches, PM requeues.
// Risk: the stopping handle waits on input acceptance forever and the ready task is never admitted again; with the
// execution already accepted on the board, the handle still needs the hand-in before its exit observation.
#[test]
fn requeued_task_after_resumed_hand_in_is_admitted_next_pass() {
    struct Resumed {
        accepted: bool,
        killed: bool,
    }
    impl Proc for Resumed {
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(self.accepted)
        }
        fn accept(&mut self) {
            self.accepted = true;
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            if !self.killed {
                Ok(None)
            } else if self.accepted {
                Ok(Some(0))
            } else {
                Err("current input acceptance unconfirmed".into())
            }
        }
        fn kill(&mut self) -> Result<(), String> {
            self.killed = true;
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    for board_accepted in [false, true] {
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let c = cfg("mac");
        let a = b
            .comment(n, "taskq: spawn worker claude:session mac")
            .unwrap();
        accept_execution(&b, n, a, a, None).unwrap();
        b.comment(
            n,
            &core::entry(
                "ask",
                &[],
                Some(a),
                "Done: checked\nProblem: pick one\n1. Go ★",
            ),
        )
        .unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        let input = b.comment(n, "taskq: answer 1").unwrap();
        let execution = b
            .comment(
                n,
                &core::entry("executing", &[&input.to_string()], Some(a), ""),
            )
            .unwrap();
        if board_accepted {
            accept_execution(&b, n, a, execution, None).unwrap();
        }
        b.comment(
            n,
            &core::entry(
                "result",
                &[&"a".repeat(40)],
                Some(a),
                "Changed: x\nChecks: y -- PASS\nOpen: none",
            ),
        )
        .unwrap();
        b.comment(n, "taskq: requeue\nPM: no reviewer").unwrap();
        assert_eq!(task(&b, n).state, State::Ready);
        let starts = Rc::new(std::cell::Cell::new(0));
        let calls = starts.clone();
        let mut s = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(move |_| {
                calls.set(calls.get() + 1);
                Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))))
            }),
        );
        s.focus = vec!["all".into()];
        s.adopt(Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution,
            task: n,
            attempt: a,
            rt: "claude".into(),
            proc: Box::new(Resumed {
                accepted: false,
                killed: false,
            }),
        })
        .unwrap();
        s.pass().unwrap();
        let x = core::executions(n, &entries(&b, n).unwrap())
            .into_iter()
            .find(|x| x.id == execution)
            .unwrap();
        assert!(
            x.accepted && x.release == Some(core::Release::Exit),
            "hand-in accepts and the stop releases ({board_accepted}): {x:?}"
        );
        assert_eq!(
            starts.get(),
            1,
            "the ready task is admitted on the same pass ({board_accepted})"
        );
        assert_ne!(task(&b, n).attempt, Some(a));
    }
}

// Chain: #2272 answered Claude resume -> no native process or transcript input past the accept deadline -> lost exit.
// Risk: the task waits "for the agent to accept input" forever; an unaccepted exit the adapter did not prove lost
// loses the R6 input fence and is requeued.
#[test]
fn lost_resumed_input_releases_the_execution_and_requeues_the_task() {
    struct Lost(bool);
    impl Proc for Lost {
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(false)
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(Some(-1))
        }
        fn exit_error(&self) -> Option<&'static str> {
            self.0.then_some("lost-input")
        }
        fn lost_input(&self) -> Option<String> {
            self.0.then(|| "no native process after resume: Claude job done input not accepted 301 s after launch; released and requeued after 300 s".into())
        }
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    for lost in [true, false] {
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let c = cfg("mac");
        let a = b
            .comment(n, "taskq: spawn worker claude:session mac")
            .unwrap();
        accept_execution(&b, n, a, a, None).unwrap();
        b.comment(
            n,
            &core::entry(
                "ask",
                &[],
                Some(a),
                "Done: checked
Problem: pick one
1. Go ★",
            ),
        )
        .unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        let input = b.comment(n, "taskq: answer 1").unwrap();
        let execution = b
            .comment(
                n,
                &core::entry("executing", &[&input.to_string()], Some(a), ""),
            )
            .unwrap();
        let mut s = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(|_| panic!("no start in this pass")),
        );
        s.adopt(Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution,
            task: n,
            attempt: a,
            rt: "claude".into(),
            proc: Box::new(Lost(lost)),
        })
        .unwrap();
        s.pass().unwrap();
        s.pass().unwrap();
        let log = entries(&b, n).unwrap();
        let x = core::executions(n, &log)
            .into_iter()
            .find(|x| x.id == execution)
            .unwrap();
        assert!(
            !x.accepted && x.release == Some(core::Release::Exit),
            "the exit releases the exact execution ({lost}): {x:?}"
        );
        let gone = log
            .iter()
            .find(|e| e.body.starts_with(&format!("taskq: gone attempt {a}")));
        if lost {
            let gone = gone.expect("a lost input posts gone");
            assert!(
                gone.body.contains("no native process after resume"),
                "gone names the cause: {}",
                gone.body
            );
            assert!(
                log.iter().any(|e| e.body.starts_with("taskq: released")
                    && e.body.contains("no native process after resume")),
                "the release names the cause"
            );
            let t = task(&b, n);
            assert_eq!(
                (t.state, t.retries),
                (State::Ready, 1),
                "the task returns to ready with one retry"
            );
        } else {
            assert!(
                gone.is_none(),
                "an unaccepted exit not proven lost keeps the R6 input fence"
            );
            assert_eq!(
                (task(&b, n).state, task(&b, n).attempt),
                (State::Doing, Some(a))
            );
            assert!(
                s.execution_waits
                    .get(&n)
                    .is_some_and(|w| w.contains("agent exited;")),
                "{:?}",
                s.execution_waits
            );
        }
    }
}

// Chain: R6, §3, §4, #2143: a renewed assignment lease must not hide historical unaccepted Exit from PM observers.
#[test]
fn historical_unaccepted_exit_reports_blocked_despite_renewed_lease() {
    let b = Fake::default();
    let mut c = cfg("mac");
    let n = b
        .create("Historical writer refusal", "", &[TASK_LABEL.into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:original-thread mac")
        .unwrap();
    c.root = started_root("historical-writer", n, a);
    let dirty = c.root.join("dirty.txt");
    std::fs::write(&dirty, "keep original work").unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    b.comment(
        n,
        &core::entry(
            "ask",
            &[],
            Some(a),
            "Done: checked\nProblem: owner action needed",
        ),
    )
    .unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    let input = b
        .comment(n, "taskq: answer\noriginal pending input")
        .unwrap();
    let x = b
        .comment(
            n,
            &core::entry("executing", &[&input.to_string()], Some(a), ""),
        )
        .unwrap();
    release_execution(&b, n, a, x, "exit").unwrap();
    let quiet = b
        .create("Quiet running task", "", &[TASK_LABEL.into()])
        .unwrap();
    let qa = b
        .comment(quiet, "taskq: spawn worker codex:quiet mac")
        .unwrap();
    let lie = json!({"task":n,"attempt":a,"id":x,"through":input,"state":"running","reason":"invented stale running claim"});
    let host = b
        .create(
            "taskq host mac",
            &format!("version test\n#{n} {a}\n#{quiet} {qa}\nexecution {lie}"),
            &[HOST_LABEL.into()],
        )
        .unwrap();
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|_| panic!("historical input cannot be resubmitted")),
    );
    s.pending.insert((n, a));
    s.kids.push(Kid {
        task: quiet,
        attempt: qa,
        execution: qa,
        rt: "codex".into(),
        proc: Box::new(FakeProc(Rc::new(RefCell::new(None)))),
        unknown: None,
        stopping: false,
        session_url: None,
    });
    for age in [0, 60, 3 * crate::snapshot::STUCK_AFTER] {
        b.advance(age);
        if age > 0 {
            b.edit(
                host,
                &format!("{}\nat {}", s.heartbeat_body(), b.now().unwrap()),
            )
            .unwrap();
        }
        s.pass().unwrap();
        let snapshot = s.snapshot.as_ref().unwrap();
        let row = snapshot.tasks.iter().find(|r| r.n == n).unwrap();
        let data = row.json();
        assert_eq!(
            data["stuck"], true,
            "a fresh lease cannot mask known blocked input, even before one hour"
        );
        assert_eq!(data["wait"], "Agent exited; input recovery required");
        assert_eq!(data["executions"][0]["state"], "unknown");
        assert_eq!(data["executions"][0]["release"], "exit");
        assert!(
            data["execution_wait"]
                .as_str()
                .unwrap()
                .contains("inspect the pending input in Codex")
        );
        let report = snapshot.text(&c, &json!({})).unwrap();
        assert!(
            report.contains("Anomalies:")
                && report.contains("Agent exited; input recovery required")
        );
        assert!(
            !report.contains("Stuck:"),
            "the actionable health block replaces the duplicate list entry (R6)"
        );
        assert!(
            snapshot
                .tasks
                .iter()
                .find(|r| r.n == quiet)
                .unwrap()
                .stuck
                .is_none(),
            "a quiet live worker is not stuck"
        );
        let beat = s.heartbeat_body();
        assert!(
            beat.lines().any(|l| l == format!("#{n} {a}")),
            "assignment ownership stays leased"
        );
        let status = beat
            .lines()
            .filter_map(|l| l.strip_prefix("execution "))
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|r| r["id"] == x)
            .unwrap();
        assert_eq!(
            status["state"], "unknown",
            "heartbeat must expose blocked input, not a live worker"
        );
        let (views, hosts) =
            views(&b, &mut Default::default(), &mut Default::default(), c.ttl).unwrap();
        let remote = crate::snapshot::from_views("board", &b, &c, b.now().unwrap(), &views, &hosts);
        assert_eq!(
            remote.tasks.iter().find(|r| r.n == n).unwrap().json()["stuck"],
            true,
            "the PM's board read agrees with local status"
        );
        assert_eq!(task(&b, n).attempt, Some(a));
        assert_eq!(
            task(&b, n).session.as_deref(),
            Some("codex:original-thread mac")
        );
        assert_eq!(
            core::project(n, &entries(&b, n).unwrap(), false).delivered(a),
            Some(a)
        );
        assert_eq!(
            core::executions(n, &entries(&b, n).unwrap())
                .iter()
                .find(|e| e.id == x)
                .unwrap()
                .release,
            Some(core::Release::Exit)
        );
    }
    assert_eq!(
        s.log
            .iter()
            .filter(|l| l.starts_with(&format!("#{n} event stuck doing ")))
            .count(),
        1,
        "one actionable stuck episode, no renewed-lease silence or repeated alert"
    );
    assert_eq!(
        std::fs::read_to_string(&dirty).unwrap(),
        "keep original work"
    );
    drop(s);
    std::fs::remove_dir_all(c.root).unwrap();
}

// R2/R11/R12, #2143: native pre-submit writer refusal must retain input, never replay unknown submission or stop the app.
#[test]
fn codex_writer_refusal_keeps_unknown_input_fenced_and_bounds_no_start_retry() {
    for case in [
        "refused",
        "reap",
        "unknown",
        "active",
        "unbound",
        "duplicate",
        "foreign",
        "signal",
    ] {
        let b = Fake::default();
        let mut c = cfg("mac");
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let thread = "11111111-1111-4111-8111-111111111111";
        let a = b
            .comment(n, &format!("taskq: spawn worker codex:{thread} mac"))
            .unwrap();
        c.root = started_root(case, n, a);
        let dirty = c.root.join("dirty.txt");
        std::fs::write(&dirty, "keep dirty work").unwrap();
        accept_execution(&b, n, a, a, None).unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        b.comment(
            n,
            &core::entry(
                "ask",
                &[],
                Some(a),
                "Done: checked\nProblem: owner action needed",
            ),
        )
        .unwrap();
        let input = b.comment(n, "taskq: answer\nretained input").unwrap();
        let calls = Rc::new(RefCell::new(0));
        let seen = calls.clone();
        let root = c.root.clone();
        let mut s = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(move |l| {
                *seen.borrow_mut() += 1;
                assert!(l.resume.as_ref().unwrap().contains("retained input"));
                let log = crate::runtime::attempt_log(&root, "worker", n, a);
                let binding = json!({"type":"taskq.input","runtime":"codex","task":n,"attempt":a,"execution":l.execution.unwrap()+u64::from(case=="foreign"),
                    "path":"invented-input.txt","input":"retained input","native":{"session":thread}});
                let mut stream = if case == "unbound" {
                    String::new()
                } else {
                    format!("{binding}\n")
                };
                if case == "duplicate" {
                    stream += &format!("{binding}\n");
                }
                if case == "active" {
                    stream += "{\"type\":\"thread.started\",\"thread_id\":\"owned\"}\n{\"type\":\"turn.started\"}\n";
                }
                if case == "unknown" {
                    stream += "unexpected submission response\n";
                }
                stream += &format!(
                    "2026-11-01T17:17:45Z ERROR codex_core::session: Failed to create session: thread-store conflict: thread {thread} already has an active writer\nError: thread/resume: thread/resume failed: thread {thread} already has an active writer (code -32600)\n"
                );
                let start = std::fs::metadata(&log).map_or(0, |m| m.len());
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&log)
                    .unwrap()
                    .write_all(stream.as_bytes())
                    .unwrap();
                Ok(Box::new(crate::runtime::CliTurn {
                    proc: Box::new(FakeProc(Rc::new(RefCell::new(Some(if case == "signal" {
                        -1
                    } else {
                        1
                    }))))),
                    log,
                    pos: start,
                    start,
                    stopping: false,
                    accepted: false,
                    read_error: None,
                    session: None,
                    native: None,
                    recovery: None,
                    rt: "codex".into(),
                }))
            }),
        );
        s.pass().unwrap();
        if case == "reap" {
            s.reap_owned();
        } else {
            s.pass().unwrap();
        }
        let x = core::executions(n, &entries(&b, n).unwrap()).pop().unwrap();
        assert_eq!(x.accepted, case == "active");
        assert_eq!(
            x.release,
            Some(if matches!(case, "refused" | "reap") {
                core::Release::NoStart
            } else {
                core::Release::Exit
            }),
            "{case}"
        );
        if matches!(case, "refused" | "reap") {
            s.pass().unwrap();
            s.pass().unwrap();
            s.pass().unwrap();
            assert_eq!(*calls.borrow(), 2, "no-start retry is bounded");
            assert_eq!(task(&b, n).state, State::Ask);
            assert!(
                task(&b, n)
                    .card
                    .unwrap()
                    .contains("close that thread in Codex")
            );
            assert_eq!(
                core::project(n, &entries(&b, n).unwrap(), false).delivered(a),
                Some(a)
            );
        } else if case != "active" {
            s.pass().unwrap();
            assert_eq!(*calls.borrow(), 1, "unknown submission must never replay");
            assert!(s.execution_waits[&n].contains("inspect the pending input in Codex"));
            assert_eq!(
                core::project(n, &entries(&b, n).unwrap(), false).delivered(a),
                Some(a)
            );
        } else {
            assert!(
                core::project(n, &entries(&b, n).unwrap(), false)
                    .delivered(a)
                    .unwrap()
                    >= input
            );
        }
        assert_eq!(std::fs::read_to_string(&dirty).unwrap(), "keep dirty work");
        drop(s);
        std::fs::remove_dir_all(c.root).unwrap();
    }
}

// Chain: undelivered continuation -> no-start -> restart -> retry. Risk: an intent cursor loses input or retries without a bound.
#[test]
fn continuation_no_start_retries_retained_input_across_restart() {
    for refused in [1, 2] {
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let a = b
            .comment(n, "taskq: spawn worker codex:session mac")
            .unwrap();
        accept_execution(&b, n, a, a, None).unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        let cursor = b.comment(n, "taskq: steer\nretained user input").unwrap();
        let c = cfg("mac");
        let calls = Rc::new(RefCell::new(Vec::new()));
        for count in 0..refused {
            let seen = calls.clone();
            let mut s = Service::new(
                &b,
                &c,
                "fixture",
                Box::new(move |l| {
                    assert!(l.execution.is_some_and(|id| id > l.attempt));
                    seen.borrow_mut()
                        .push((l.resume.clone().unwrap(), l.execution.unwrap()));
                    Err(crate::runtime::StartError::io(
                        std::io::Error::other("pre-submit refusal"),
                        None,
                    ))
                }),
            );
            let result = s.pass();
            if count + 1 < core::GONE_MAX {
                assert!(result.unwrap_err().contains("input retained"));
            } else {
                result.unwrap();
            }
            assert_eq!(
                core::project(n, &entries(&b, n).unwrap(), false).delivered(a),
                Some(a)
            );
            assert_eq!(task(&b, n).attempt, Some(a));
        }
        if refused == 2 {
            assert_eq!(task(&b, n).state, State::Ask);
            let mut parked = service(&b, &c, Default::default());
            parked.pass().unwrap();
            assert!(parked.kids.is_empty());
            b.comment(n, "taskq: answer 1").unwrap();
        }
        let seen = calls.clone();
        let mut recovered = Service::new(
            &b,
            &c,
            "fixture",
            Box::new(move |l| {
                assert!(l.execution.is_some_and(|id| id > l.attempt));
                seen.borrow_mut()
                    .push((l.resume.clone().unwrap(), l.execution.unwrap()));
                Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))))
            }),
        );
        recovered.pass().unwrap();
        assert_eq!(calls.borrow().len(), refused as usize + 1);
        assert!(
            calls
                .borrow()
                .iter()
                .all(|(input, _)| input.contains("retained user input"))
        );
        // #1834: the input's marker names the delivered steer as the owner's, for the live worker view.
        let inbox = crate::runtime::turn_inbox(&calls.borrow().last().unwrap().0).unwrap();
        assert_eq!(
            inbox[0],
            serde_json::json!({"kind":"steer","text":"retained user input"})
        );
        assert!(
            core::project(n, &entries(&b, n).unwrap(), false)
                .delivered(a)
                .unwrap()
                >= cursor
        );
        let execution = recovered.kids[0].execution;
        assert_eq!(
            calls.borrow().last().unwrap().1,
            execution,
            "adapter receives the exact continuation intent"
        );
        recovered.pass().unwrap();
        assert_eq!(recovered.kids[0].execution, execution);
        drop(recovered);
        let mut restarted = service(&b, &c, Default::default());
        restarted.pass().unwrap();
        assert!(restarted.kids.is_empty());
        assert!(restarted.execution_waits[&n].contains("termination unconfirmed"));
        assert!(
            restarted.retained_attempts().contains(&a),
            "lost handle retains its input evidence after restart"
        );
    }
}

// Chain: native acceptance/exit -> failed or lost board response -> durable readback. Risk: evidence retry replays work or drops its handle.
#[test]
fn acceptance_readback_retries_evidence_and_keeps_the_native_handle() {
    struct Deferred {
        accepted: Rc<std::cell::Cell<bool>>,
        exit: Rc<std::cell::Cell<bool>>,
    }
    impl Proc for Deferred {
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(self.accepted.get())
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(self.exit.get().then_some(0))
        }
        fn kill(&mut self) -> Result<(), String> {
            self.exit.set(true);
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let b = Fake::default();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let c = cfg("mac");
    let accepted = Rc::new(std::cell::Cell::new(false));
    let exit = Rc::new(std::cell::Cell::new(false));
    let starts = Rc::new(std::cell::Cell::new(0));
    let (ac, ex, calls) = (accepted.clone(), exit.clone(), starts.clone());
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(move |_| {
            calls.set(calls.get() + 1);
            Ok(Box::new(Deferred {
                accepted: ac.clone(),
                exit: ex.clone(),
            }))
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    assert_eq!(s.delivered(a), 0, "a created handle is not acceptance");
    assert_eq!(
        core::project(n, &entries(&b, n).unwrap(), false).delivered(a),
        None
    );
    b.comment(
        n,
        &core::entry("ask", &[], Some(a), "Which?\n1. Continue ★"),
    )
    .unwrap();
    accepted.set(true);
    exit.set(true);
    b.state.borrow_mut().fail_comments = 1;
    s.pass().unwrap();
    assert_eq!(
        s.kids.len(),
        1,
        "retain native proof until its receipt is durable"
    );
    assert_eq!(s.delivered(a), 0);
    assert_eq!(starts.get(), 1);
    assert!(
        s.kids
            .iter()
            .find(|k| k.attempt == a)
            .unwrap()
            .unknown
            .as_deref()
            .unwrap()
            .contains("acceptance unconfirmed")
    );
    s.kids[0].stopping = true;
    s.kids[0].session_url = Some(Ok("https://claude.ai/code/session_014abc".into()));
    s.rebuild();
    assert!(s.kids[0].stopping && s.kids[0].unknown.is_some());
    assert_eq!(
        s.kids[0].session_url,
        Some(Ok("https://claude.ai/code/session_014abc".into()))
    );
    b.state.borrow_mut().lose_comment_response = true;
    s.pass().unwrap();
    assert_eq!(s.delivered(a), a);
    assert!(s.kids.is_empty());
    assert_eq!(starts.get(), 1);
    assert!(
        !s.heartbeat_body().contains("session #"),
        "retired execution cannot leave a session URL behind"
    );
    let log = entries(&b, n).unwrap();
    assert_eq!(
        log.iter()
            .filter(|e| e.body.starts_with("taskq: accepted "))
            .count(),
        1
    );
    assert_eq!(
        core::executions(n, &log)[0].release,
        Some(core::Release::Exit)
    );
    assert!(
        release_execution(&b, n, a, a, "no-start")
            .unwrap_err()
            .contains("contradictory")
    );
    let mut restarted = service(&b, &c, Default::default());
    restarted.pass().unwrap();
    assert!(restarted.kids.is_empty());
    assert_eq!(task(&b, n).state, State::Ask);
}

// Chain: pending input -> reduced exit with unreadable acceptance; release cannot authorize resubmission.
#[test]
fn terminal_with_unreadable_acceptance_never_resubmits() {
    struct Terminal;
    impl Proc for Terminal {
        fn accepted(&mut self) -> Result<bool, String> {
            Err("acceptance unavailable".into())
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(Some(0))
        }
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let b = Fake::default();
    let c = cfg("mac");
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:fixture mac")
        .unwrap();
    let input = b.comment(n, "taskq: steer\nkeep pending input").unwrap();
    let mut s = service(&b, &c, Default::default());
    s.adopt(Kid {
        task: n,
        attempt: a,
        execution: a,
        rt: "codex".into(),
        proc: Box::new(Terminal),
        unknown: None,
        stopping: false,
        session_url: None,
    })
    .unwrap();
    s.reap_owned();
    assert!(s.kids.is_empty());
    let x = core::executions(n, &entries(&b, n).unwrap()).remove(0);
    assert_eq!(x.release, Some(core::Release::Exit));
    assert!(!x.accepted);
    let mut restarted = service(&b, &c, Default::default());
    restarted.pass().unwrap();
    assert!(restarted.kids.is_empty());
    assert_eq!(restarted.delivered(a), 0);
    assert!(restarted.execution_waits[&n].contains("acceptance unknown"));
    assert_eq!(task(&b, n).attempt, Some(a));
    assert_eq!(task(&b, n).inbox, [input]);
}

// Chain: terminal old turn -> pending owner input -> unavailable owner/session. Risk: recovery drops input by fabricating gone.
#[test]
fn unavailable_owner_or_session_preserves_pending_input() {
    for (runtime, host) in [("codex:session", "other"), ("codex:-", "mac")] {
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let c = cfg("mac");
        let a = b
            .comment(
                n,
                &core::entry("spawn", &["worker", runtime, host], None, ""),
            )
            .unwrap();
        accept_execution(&b, n, a, a, None).unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        let input = b.comment(n, "taskq: steer\nkeep this input").unwrap();
        b.advance(3600);
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a));
        assert_eq!(task(&b, n).inbox, [input]);
        assert!(s.kids.is_empty());
        assert!(
            !b.comments(n)
                .unwrap()
                .iter()
                .any(|c| c.body.starts_with("taskq: gone"))
        );
        if host == "mac" {
            assert!(s.execution_waits[&n].contains("session identity unknown"));
            assert!(
                s.snapshot
                    .as_ref()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|r| r.n == n)
                    .unwrap()
                    .execution_wait
                    .as_ref()
                    .unwrap()
                    .contains("session identity unknown")
            );
        } else {
            assert!(s.execution_waits[&n].contains("input pending on other"));
        }
    }
}

// Chain: uncertain native submit -> service restart -> result/requeue -> another host.
// Risk: task state or a lost process handle permits duplicate external execution.
#[test]
fn uncertain_submission_stays_fenced_through_restart_and_task_moves() {
    for result in [true, false] {
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let (ca, cb) = (cfg("mac"), cfg("win"));
        let starts = Rc::new(std::cell::Cell::new(0));
        let called = starts.clone();
        let mut a = Service::new(
            &b,
            &ca,
            "fixture",
            Box::new(move |_| {
                called.set(called.get() + 1);
                let mut error = crate::runtime::StartError::io(
                    std::io::Error::other("response lost after submission"),
                    None,
                );
                error.no_start = false;
                Err(error)
            }),
        );
        a.focus = vec!["all".into()];
        assert!(a.pass().unwrap_err().contains("launch outcome unknown"));
        let attempt = task(&b, n).attempt.unwrap();
        assert_eq!(
            a.entry().held.len(),
            1,
            "unknown submission retains capacity"
        );
        drop(a);
        let exits: Exits = Default::default();
        let mut restored = service(&b, &ca, exits.clone());
        restored.pass().unwrap();
        assert_eq!(
            restored.entry().held.len(),
            1,
            "restart rebuilds unresolved ownership"
        );
        assert!(exits.borrow().is_empty());
        b.comment(
            n,
            &if result {
                core::entry(
                    "result",
                    &[&"a".repeat(40)],
                    Some(attempt),
                    "ready for review",
                )
            } else {
                core::entry("requeue", &[], Some(attempt), "retry")
            },
        )
        .unwrap();
        b.advance(900);
        let mut other = service(&b, &cb, exits.clone());
        other.pass().unwrap();
        restored.pass().unwrap();
        assert_eq!(starts.get(), 1);
        assert!(exits.borrow().is_empty());
        assert!(other.execution_waits.contains_key(&n));
        release_execution(&b, n, attempt, attempt, "exit").unwrap();
        other.pass().unwrap();
        assert_eq!(
            exits.borrow().len(),
            1,
            "fresh native exit evidence permits the next role"
        );
        restored.pass().unwrap();
        assert!(
            restored.entry().held.is_empty(),
            "another host's new attempt cannot retain the old reservation"
        );
        assert_eq!(
            task(&b, n).role.as_deref(),
            Some(if result { "reviewer" } else { "worker" })
        );
    }
}

// Chain: requeue -> stop request -> uncertain exit -> occupied slot -> confirmed exit -> replacement.
#[test]
fn cancellation_retains_an_unconfirmed_child_and_blocks_replacement() {
    struct SlowStop(Rc<std::cell::Cell<u8>>);
    impl Proc for SlowStop {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            if self.0.get() == 2 {
                Err("invented read failure".into())
            } else {
                Ok((self.0.get() == 3).then_some(-9))
            }
        }
        fn kill(&mut self) -> Result<(), String> {
            if self.0.get() == 1 {
                Err("invented stop refusal".into())
            } else {
                Ok(())
            }
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let b = Fake::default();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".into(), 3)]);
    let exited = Rc::new(std::cell::Cell::new(0));
    let starts = Rc::new(std::cell::Cell::new(0));
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| {
            starts.set(starts.get() + 1);
            Ok(Box::new(SlowStop(exited.clone())))
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let attempt = s.kids[0].attempt;
    b.comment(n, "taskq: requeue").unwrap();
    for mode in [1, 0, 2, 0] {
        exited.set(mode);
        s.pass().unwrap();
        assert_eq!(
            starts.get(),
            1,
            "an unconfirmed old process cannot overlap its replacement even with spare capacity"
        );
        assert_eq!(s.kids[0].attempt, attempt);
        assert!(
            s.kids
                .iter()
                .any(|k| k.attempt == attempt && k.unknown.is_some())
        );
        assert_eq!(task(&b, n).state, State::Ready);
        s.reap_owned();
        assert!(
            s.kids
                .iter()
                .any(|k| k.attempt == attempt && k.unknown.is_some()),
            "reduced monitoring must retain the pending-stop reason"
        );
    }
    exited.set(3);
    s.pass().unwrap();
    assert_eq!(starts.get(), 2);
    assert!(s.kids.iter().all(|k| k.attempt != attempt));
}

// Chain: §4 log retention -> service pass -> collection/selftest. Risk: the hourly timer leaves a full log stuck.
#[test]
fn service_pass_restores_bounded_log_collection() {
    let root = std::env::temp_dir().join(format!("taskq-service-retention-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("taskq.log");
    let c = Config {
        install: root.clone(),
        ..cfg("win")
    };
    let b = Fake::default();
    let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no tasks")));
    for size in [crate::data::LOG_MAX - 256, crate::data::LOG_MAX + 1024] {
        let mut text = "old diagnostic\n".repeat(size as usize / 15);
        let newest = "taskq-profile {\"op\":\"spawn\",\"us\":1,\"error\":\"none\",\"at_ms\":1}\n";
        text.push_str(newest);
        let terminal = format!(
            "taskq-stage {}\n",
            serde_json::json!({"project":"o/r", "host":"win",
                "stage":"build", "task":7, "attempt":41, "rt":"codex", "us":null,
                "outcome":"cancelled", "error":"none", "at_ms":1, "unknown":"adopted"})
        );
        text.push_str(&terminal);
        std::fs::write(&path, &text).unwrap();
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        let lock = crate::profile::log_lock(&path).unwrap();
        s.pass().unwrap();
        assert!(
            std::fs::read_to_string(&path).unwrap() == text,
            "busy lock preserves evidence"
        );
        drop(lock);
        let (_capture, _guard) = crate::profile::tests::collecting(&root);
        s.pass().unwrap();
        let retained = std::fs::read_to_string(&path).unwrap();
        assert!(retained.len() as u64 <= crate::data::LOG_MAX / 2);
        assert!(retained.starts_with("old diagnostic\n") && retained.ends_with(&terminal));
        use std::io::Write;
        writer.write_all(b"still open diagnostic\n").unwrap();
        crate::profile::tests::append_line(&path, newest).unwrap();
        crate::profile::tests::append_line(&path, &terminal).unwrap();
        assert!(
            crate::profile::selftest(&root, "o/r", &vec![], 300)
                .unwrap()
                .contains("no overdue")
        );
        assert!(!crate::profile::report(&root).contains("collection stopped"));
        drop(writer);
        drop(_guard);
        drop(_capture);
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::create_dir_all(&root).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn cfg(me: &str) -> Config {
    // Per-fixture temporary install: a shared path (C:\nonexistent on Windows) leaked quota holds into later runs.
    // ponytail: never created or removed here; tests that write leave one small directory in the OS temp dir.
    let base = std::env::temp_dir().join(format!("taskq-service-test-{}", crate::runtime::uuid()));
    Config {
        root: base.clone(),
        install: base,
        board: &crate::board::github::KIND,
        web_host: String::new(),
        repo: "o/r".into(),
        delivery: crate::config::Delivery::AutoMerge,
        delivery_revision: 0,
        me: me.into(),
        limits: BTreeMap::from([("claude".to_string(), 1)]),
        has: vec![],
        claim: crate::host::ClaimMode::Any,
        weights: BTreeMap::new(),
        allowed_runtimes: None,
        auto_reroute: false,
        default_runtime: crate::runtime::RuntimeId::Claude,
        routing: vec![],
        permission_mode: "dontAsk".into(),
        codex: vec![],
        ttl: 300,
        poll: 60,
        update_every: 900,
        rollback_after: 600,
        external: false,
        host_setup: None,
    }
}

// #2196: a fixture rooted at the shared /nonexistent path persisted quota holds across runs on Windows.
#[test]
fn fixtures_use_private_temporary_installs() {
    let (a, b) = (cfg("mac"), crate::tests::cfg());
    for c in [&a, &b] {
        assert!(
            c.install.starts_with(std::env::temp_dir()) && c.root.starts_with(std::env::temp_dir()),
            "{:?}",
            c.install
        );
    }
    assert_ne!(
        a.install,
        cfg("mac").install,
        "each fixture gets its own install"
    );
    assert_ne!(
        b.install,
        crate::tests::cfg().install,
        "each fixture gets its own install"
    );
    let shared = format!("\"/{}\"", "nonexistent");
    let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            if p.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            for (i, line) in text.lines().enumerate() {
                let l = line.trim_start();
                assert!(
                    l.starts_with("//") || !l.contains(&shared),
                    "{}:{} fixture uses the shared /nonexistent path",
                    p.display(),
                    i + 1
                );
            }
        }
    }
}

// Chain: Windows native reviewer launch -> service pass -> gone, on a disposable board only.
#[cfg(windows)]
#[test]
#[ignore = "live Claude reviewer; TASKQ_CLAUDE_ROOT selects the worktree"]
fn live_claude_reviewer_exit_writes_gone() {
    use crate::runtime::{ClaudeTurn, Env, access, log_path};
    let mut c = cfg("win");
    c.root = PathBuf::from(std::env::var("TASKQ_CLAUDE_ROOT").expect("TASKQ_CLAUDE_ROOT"));
    c.install = std::env::temp_dir().join(format!("taskq-claude-{}", std::process::id()));
    std::fs::create_dir_all(&c.install).unwrap();
    crate::policy::grant(&c.install, "claude", "full", None, "op", 1).unwrap();
    let env = Env {
        context: "",
        root: &c.root,
        exe: std::path::Path::new("taskq"),
        install: &c.install,
        permission_mode: "dontAsk",
        codex: &[],
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let b = Fake::default();
    let n = b
        .create("Exit qualification", "", &["taskq".into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:invented win")
        .unwrap();
    b.comment(
        n,
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(a),
            "invented candidate",
        ),
    )
    .unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    let mut s = Service::new(
        &b,
        &c,
        "4.4.0",
        Box::new(|l| {
            assert_eq!(l.role, "reviewer");
            let text = format!(
                "{}\nReply EXIT_CHECK and do nothing else.",
                "Qualification input only.\n".repeat(1800)
            );
            ClaudeTurn::start(
                &env,
                l,
                &log_path(env.root, l),
                &text,
                &access(&env, "claude").unwrap(),
            )
            .map(|t| Box::new(t) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let r = task(&b, n).attempt.unwrap();
    assert_ne!(r, a);
    eprintln!("win reviewer start attempt {r}");
    // End only this disposable native job; do not wait through Claude's spawn-retry backoff.
    crate::runtime::stop(s.kids[0].proc.as_mut()).unwrap();
    s.pass().unwrap();
    let log = b.comments(n).unwrap();
    for comment in &log {
        eprintln!("{}", comment.body);
    }
    for kid in &mut s.kids {
        crate::runtime::stop(kid.proc.as_mut()).unwrap();
    }
    assert!(
        log.iter()
            .any(|e| e.body.contains(&format!("taskq: gone attempt {r}"))),
        "next pass must reap the native terminal job"
    );
    assert!(task(&b, n).result.is_some());
    std::fs::remove_dir_all(&c.install).unwrap();
}

// Chain: section 7 publication intent -> admission -> service restart -> merge receipt/close, focus off.
// Risk: a restart launches another reviewer, expires the ended attempt, or forgets the approved verdict.
#[test]
fn pending_publication_survives_restart_without_a_reviewer() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented publication", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    let verdict =
        "Changed: tested publication\nChecks: Task fit A1 ok; merge-result CI pending\nOpen: none";
    b.comment(n, &core::entry("publish", &[], Some(r), verdict))
        .unwrap();
    let frozen = crate::readiness::Publication {
        repository: "invented/repo".into(),
        pr: 3,
        base: Some("b".repeat(40)),
        head: "a".repeat(40),
        integration: None,
        merged: None,
        parent: None,
        ci_kind: None,
        ci_sha: None,
        ci_run_id: None,
        ci_run_attempt: None,
    };
    b.state.borrow_mut().evidence = vec![frozen.clone()];
    b.state.borrow_mut().publish = "queue-admitted".into();
    {
        let mut s = Service::new(
            &b,
            &c,
            "test",
            Box::new(|_| panic!("no reviewer for pending publication")),
        );
        s.focus = vec!["all".into()];
        s.pass().unwrap();
    }
    assert!(task(&b, n).enqueued.is_some());
    b.state.borrow_mut().publish = "queue-waiting".into();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process while queued")),
    );
    s.pass().unwrap();
    assert_eq!(task(&b, n).publication.as_deref(), Some(verdict));
    assert_eq!(task(&b, n).retries, 0);
    assert!(b.issue(n).unwrap().open);
    b.state.borrow_mut().evidence = vec![crate::readiness::Publication {
        integration: Some("c".repeat(40)),
        merged: Some("c".repeat(40)),
        parent: Some("b".repeat(40)),
        ci_kind: Some("merge_group".into()),
        ci_sha: Some("c".repeat(40)),
        ..frozen
    }];
    b.state.borrow_mut().publish = String::new();
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Closed);
    assert!(!b.issue(n).unwrap().open);
    let log = entries(&b, n).unwrap();
    let reads = b.state.borrow().comment_reads;
    assert_eq!(
        crate::readiness::source(n, &b.url(n), &log)["state"],
        "source_ready",
        "publication retains the original result binding and frozen inputs"
    );
    assert_eq!(
        b.state.borrow().comment_reads,
        reads,
        "source projection makes no board or CI reads"
    );
    let comments = b.comments(n).unwrap();
    let close = comments
        .iter()
        .find(|e| e.body.starts_with("taskq: close"))
        .unwrap();
    // § 2 Summary: the close names the publish it repeats; readers resolve it to the verdict
    assert!(
        close.body.contains(core::REPEATS) && !close.body.contains(verdict),
        "{}",
        close.body
    );
    assert!(
        core::verdict(
            n,
            &entries(&b, n).unwrap(),
            core::entry_text(n, &close.body)
        )
        .contains(verdict)
    );
    assert_eq!(
        comments
            .iter()
            .filter(|e| e.body.starts_with("taskq: close"))
            .count(),
        1
    );
}

// Chain: § 7 GitLab `direct` close -> MR created -> merge refused `ci_still_running` -> `publish` -> service pass -> merge
// or `publication-failed` (#1939). Defect (cyprusbz #540, MR !1196): close refused while the MR pipeline ran, the
// reviewer exited, the service wrote `gone` and a second reviewer burned a retry. Retained: the running pipeline is a
// visible wait with its link; green merges and closes once, red or cancelled fails once with the link and stops polling.
#[test]
fn gitlab_close_waits_for_the_mr_pipeline_then_merges_or_fails_once() {
    for end in ["success", "failed", "canceled"] {
        let (b, mut c) = (Fake::default(), cfg("win"));
        c.delivery = crate::config::Delivery::Direct;
        let n = b
            .create("Invented GitLab close", "", &["taskq".into()])
            .unwrap();
        let sha = "a".repeat(40);
        let pipeline = Rc::new(RefCell::new("running"));
        // created MRs, merges
        let writes = Rc::new(RefCell::new((0, 0)));
        let (p, w, result) = (pipeline.clone(), writes.clone(), sha.clone());
        let link = "https://gitlab.example/o/r/-/pipelines/9";
        b.state.borrow_mut().publisher = Some(crate::board::replying(
            &crate::board::gitlab::KIND,
            move |method, url| {
                let (created, merged) = *w.borrow();
                let mr = json!({"iid": 1196, "sha": result});
                Ok(if url.contains("/merge_base?") {
                    json!({"id": "c".repeat(40)})
                }
                // the branch pipeline passed; the MR pipeline on the same SHA starts once the MR exists
                else if url.contains("/repository/commits/") {
                    json!({"last_pipeline": {"status": if created == 0 { "success" } else { *p.borrow() }, "web_url": link}})
                } else if url.contains("state=opened") {
                    if created == 0 { json!([]) } else { json!([mr]) }
                } else if url.contains("state=merged") {
                    json!([])
                } else if method == "POST" && url.ends_with("/merge_requests") {
                    w.borrow_mut().0 += 1;
                    mr
                } else if url.ends_with("/projects/o%2Fr") {
                    json!({"merge_method": "ff", "autoclose_referenced_issues": false})
                } else if method == "PUT" && url.ends_with("/merge_requests/1196/merge") {
                    w.borrow_mut().1 += 1;
                    json!({})
                } else if url.ends_with("/merge_requests/1196") {
                    let status = match *p.borrow() {
                        "running" => "ci_still_running",
                        "success" => "mergeable",
                        _ => "ci_must_pass",
                    };
                    json!({"state": if merged > 0 { "merged" } else { "opened" }, "detailed_merge_status": status, "head_pipeline": {"status": *p.borrow(), "web_url": link}})
                } else {
                    panic!("unexpected {method} {url}")
                })
            },
        ));
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&sha], Some(a), "candidate"))
            .unwrap();
        b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        let verdict = format!("Changed: x\nChecks: A1 ok; CI green on {sha}\nOpen: none");
        let op = crate::dispatch::Op::default();
        let got = crate::dispatch::close_one(&b, &c, n, &verdict, &op);
        assert!(got.ok() && got.extra.contains(link), "{end}: {got:?}");
        assert_eq!(
            (task(&b, n).state, task(&b, n).publication.as_deref()),
            (State::Review, Some(verdict.as_str())),
            "{end}"
        );
        assert!(
            crate::dispatch::close_one(&b, &c, n, &verdict, &op).ok(),
            "{end}: a repeated close keeps the saved verdict"
        );
        let mut s = Service::new(
            &b,
            &c,
            "test",
            Box::new(|_| panic!("no reviewer while the MR pipeline runs")),
        );
        for _ in 0..2 {
            s.pass().unwrap();
        }
        let row = s
            .snapshot
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|r| r.n == n)
            .unwrap()
            .json();
        assert!(
            row["publication"]["reason"]
                .as_str()
                .unwrap()
                .contains(link),
            "{end}: {row}"
        );
        *pipeline.borrow_mut() = end;
        s.pass().unwrap();
        let (t, log) = (task(&b, n), b.comments(n).unwrap());
        let count = |kind: &str| {
            log.iter()
                .filter(|e| e.body.starts_with(&format!("taskq: {kind}")))
                .count()
        };
        assert_eq!(
            (
                count("gone"),
                count("spawn reviewer"),
                count("publish"),
                *writes.borrow()
            ),
            (0, 1, 1, (1, usize::from(end == "success"))),
            "{end}"
        );
        if end == "success" {
            assert_eq!(
                (t.state, t.retries, count("close")),
                (State::Closed, 0, 1),
                "{end}"
            );
        } else {
            assert_eq!(
                (
                    t.state,
                    t.retries,
                    count("publication-failed"),
                    t.publication
                ),
                (State::Ready, 1, 1, None),
                "{end}"
            );
            assert!(
                log.iter()
                    .any(|e| e.body.starts_with("taskq: publication-failed")
                        && e.body.contains(&format!("CI {end} on {sha} {link}"))),
                "{end}"
            );
            let calls = b.state.borrow().publications;
            s.pass().unwrap();
            assert_eq!(
                b.state.borrow().publications,
                calls,
                "{end}: no publication polls after it failed"
            );
        }
    }
}

// Chain: section 7 publication -> refused enqueue -> status row reason and age, one `checks` record -> conflict requeue (#1820).
// Defect (pilotq #106): a publication the pass could not move was only logged, so the task sat in review 50 minutes with no
// board, menu or telemetry signal. Retained: every pass names the wait in the status row; a conflict requeues in the same pass.
#[test]
fn publication_stall_shows_its_reason_and_age_then_requeues_on_conflict() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented stalled publication", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    b.state.borrow_mut().publish = "queue-refused".into();
    crate::profile::STAGED.with(|v| v.borrow_mut().clear());
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a waiting publication")),
    );
    for _ in 0..2 {
        s.pass().unwrap();
    }
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    let reason = "merge queue did not admit PR #7: Required status check build is in progress; CI build pending";
    assert_eq!(
        (&row["state"], &row["publication"]["reason"]),
        (&json!("review"), &json!(reason))
    );
    assert!(
        row["publication"]["since"].is_i64() && row["publication"]["age_s"].is_i64(),
        "{row}"
    );
    assert!(
        row["wait"]
            .as_str()
            .unwrap()
            .starts_with(crate::snapshot::PUBLICATION_WAIT)
            && row["wait"].as_str().unwrap().ends_with(reason),
        "{row}"
    );
    let checks = crate::profile::STAGED.with(|v| {
        v.borrow()
            .iter()
            .filter(|l| l.starts_with(&format!("checks {n} ")))
            .cloned()
            .collect::<Vec<_>>()
    });
    assert_eq!(
        checks.len(),
        1,
        "one record per reason, not per pass: {checks:?}"
    );
    assert!(checks[0].ends_with(" retried other"), "{checks:?}");
    b.state.borrow_mut().publish = "queue-conflict".into();
    s.pass().unwrap();
    let t = task(&b, n);
    assert_eq!(
        (t.state, t.retries),
        (State::Ready, 0),
        "main-moved keeps retries"
    );
    assert!(
        b.comments(n)
            .unwrap()
            .iter()
            .any(|e| e.body.starts_with("taskq: requeue main-moved")
                && e.body.contains("queue merge conflict"))
    );
    assert!(s.publication_waits.is_empty());
}

// Chain: section 7 publication -> manual-merge wait -> status/menu row with the PR link -> one board comment (#1935).
// Defect (Kirsalina/desiign): review tasks waited up to 75 minutes for a person to merge with no reason on any surface.
// Retained: the row names the wait and PR; the board gets one plain comment, not one per pass and not a log entry.
#[test]
fn manual_merge_wait_is_shown_with_its_pr_and_commented_once() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented manual merge", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    b.state.borrow_mut().publish = "manual-merge".into();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a waiting publication")),
    );
    for _ in 0..3 {
        s.pass().unwrap();
    }
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert!(
        row["wait"]
            .as_str()
            .unwrap()
            .contains(crate::board::MANUAL_MERGE)
            && row["wait"]
                .as_str()
                .unwrap()
                .contains("https://github.com/o/r/pull/7"),
        "{row}"
    );
    let notes: Vec<_> = b
        .comments(n)
        .unwrap()
        .into_iter()
        .filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE))
        .collect();
    assert_eq!(notes.len(), 1, "one board comment, not one per pass");
    assert!(notes[0].body.contains("https://github.com/o/r/pull/7"));
    assert_eq!(task(&b, n).state, State::Review, "the note is no log entry");
    s.publication_waits.clear();
    s.pass().unwrap();
    assert_eq!(
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE))
            .count(),
        1,
        "a restart reads the board note back (R1)"
    );
    // #1942: a new PR head (a later `publish` entry) gets its own note
    b.state.borrow_mut().publish = "queue-conflict".into();
    s.pass().unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"b".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    b.state.borrow_mut().publish = "manual-merge".into();
    for _ in 0..2 {
        s.pass().unwrap();
    }
    assert_eq!(
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE))
            .count(),
        2,
        "one note per PR head"
    );
}

// Chain: § 7 Result and close verdicts -> `publish` under `pr` -> manual-merge wait -> preset switched to `direct`
// -> next service pass fast-forwards main to the PR head -> one close (#1942). Defect (desiign #31, #39, #44): PRs
// published before a switch to `direct` stayed in review forever, `needs a manual merge` logged every pass.
// Retained: a pending publication takes the current preset with its exact-SHA CI; the wait is noted once.
#[test]
fn a_pr_published_before_a_switch_to_direct_is_delivered_directly() {
    let (b, mut c) = (Fake::default(), cfg("win"));
    c.root = std::env::temp_dir().join(format!("taskq-1942-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&c.root).unwrap();
    let preset = |p: &str, r: u64| {
        std::fs::write(
            c.root.join("taskq.json"),
            json!({"repo": "o/r", "delivery": {"preset": p, "revision": r}}).to_string(),
        )
        .unwrap()
    };
    preset("auto-merge", 2);
    let n = b
        .create("Invented preset switch", "", &["taskq".into()])
        .unwrap();
    let sha = "a".repeat(40);
    // main fast-forwards
    let forwards = Rc::new(RefCell::new(0));
    let (f, head) = (forwards.clone(), sha.clone());
    b.state.borrow_mut().publisher = Some(crate::board::replying(
        &crate::board::github::KIND,
        move |method, url| {
            Ok(if url.contains("/compare/main...") {
                json!({"status": if *f.borrow() == 0 { "ahead" } else { "identical" }})
            } else if url.ends_with("/rules/branches/main") {
                return Err(crate::board::Error::http(
                    403,
                    "HTTP 403: Upgrade to GitHub Pro or make this repository public to enable this feature.",
                ));
            } else if url.contains("/pulls?head=o:taskq-1&base=main&state=open") {
                json!([{"number": 37, "head": {"sha": head}, "html_url": "https://github.com/o/r/pull/37"}])
            } else if url.contains("/check-runs") {
                json!({"check_runs": [{"name": "ci", "status": "completed", "conclusion": "success"}]})
            } else if url.ends_with("/status") {
                json!({"total_count": 0})
            } else if url.ends_with("/git/ref/heads/taskq-1") {
                json!({"object": {"sha": head}})
            } else if method == "PATCH" && url.ends_with("/git/refs/heads/main") {
                *f.borrow_mut() += 1;
                json!({"object": {"sha": head}})
            } else if url.contains("/issues/1/comments") {
                json!([])
            } else {
                panic!("unexpected {method} {url}")
            })
        },
    ));
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(n, &core::entry("result", &[&sha], Some(a), "candidate"))
        .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a waiting publication")),
    );
    // the service loop's pass: reread taskq.json, then pass
    let pass = |s: &mut Service| {
        s.reread_project().unwrap();
        s.pass().unwrap()
    };
    for _ in 0..3 {
        pass(&mut s);
    }
    let notes = |b: &Fake| {
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE))
            .count()
    };
    assert_eq!(
        (
            task(&b, n).state,
            notes(&b),
            *forwards.borrow(),
            s.cfg.delivery.mode()
        ),
        (State::Review, 1, 0, "pr"),
        "the `pr` wait is noted once"
    );
    // the owner edits taskq.json; the running service is not restarted
    preset("direct", 3);
    pass(&mut s);
    let log = b.comments(n).unwrap();
    let closes: Vec<_> = log
        .iter()
        .filter(|e| e.body.starts_with("taskq: close"))
        .collect();
    assert_eq!(
        (
            task(&b, n).state,
            closes.len(),
            *forwards.borrow(),
            notes(&b)
        ),
        (State::Closed, 1, 1, 1)
    );
    assert!(
        closes[0]
            .body
            .contains(&format!("delivery direct r3: main fast-forwarded to {sha}")),
        "{}",
        closes[0].body
    );
    pass(&mut s);
    assert_eq!(
        (b.comments(n).unwrap().len(), *forwards.borrow()),
        (log.len(), 1),
        "closed once"
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&c.root);
}

// Chain: section 7 publication -> blocked outcome -> ask in one pass -> answer -> retry -> close (#1413).
// Risk: a publication only the owner can unblock polls for hours with no board signal.
#[test]
fn blocked_publication_asks_the_owner_in_one_pass() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented blocked publication", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    b.state.borrow_mut().publish = "queue-blocked".into();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a blocked publication")),
    );
    s.pass().unwrap();
    let t = task(&b, n);
    assert_eq!(t.state, State::Ask);
    let card = t.card.unwrap();
    assert!(
        card.contains("Problem: PR #7 merged, merge_group CI tests failure")
            && card.contains("1. retry:"),
        "{card}"
    );
    let calls = b.state.borrow().publications;
    s.pass().unwrap();
    assert_eq!(
        b.state.borrow().publications,
        calls,
        "no publication polls while the owner decides"
    );
    b.comment(n, "taskq: answer 1").unwrap();
    b.state.borrow_mut().publish = String::new();
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Closed);
}

// Chain: section 7 Publication routes, task PR in another repository (#2047) -> service pass enqueues it in that
// repository -> status row shows the merge queue stage -> merged with green merge_group CI -> one close naming it.
// Defect (#2009 dashq#240, #2010 pilotq#200, #2015 dashq#241): publication looped on the board repository's compare
// HTTP 404 with no stage shown. Retained: the foreign PR moves through the same stages and receipt as a local one.
#[test]
fn a_task_pr_in_another_repository_shows_its_stage_and_closes_after_its_merge() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented foreign PR", "", &["taskq".into()])
        .unwrap();
    let (head, merge) = ("a".repeat(40), "b".repeat(40));
    let text = "Changed: [o/x#240](https://github.com/o/x/pull/240)\nChecks: A1 ok\nOpen: none";
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    let result = core::entry("result", &[&head], Some(a), text);
    b.comment(n, &result).unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    let publish = core::entry("publish", &[], Some(r), text);
    b.comment(n, &publish).unwrap();
    // 0 open, 1 queued, 2 merged
    let phase = Rc::new(std::cell::Cell::new(0));
    let (p, h, m) = (phase.clone(), head.clone(), merge.clone());
    b.state.borrow_mut().publish = "queue-foreign".into();
    b.state.borrow_mut().publisher = Some(crate::board::replying_to(
        &crate::board::github::KIND,
        move |method, url, body| {
            if let Some(rest) = url.strip_prefix("https://api.github.com/repos/o/r") {
                return if rest.starts_with("/compare/") {
                    Err(crate::board::Error::http(
                        404,
                        format!("GET {url}: HTTP 404 Not Found"),
                    ))
                } else if rest.starts_with(&format!("/issues/{n}/comments")) {
                    Ok(json!([result.clone(), publish.clone()].iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "author_association": "OWNER",
                            "created_at": "2026-10-30T00:00:00Z", "updated_at": "2026-10-30T00:00:00Z", "user": {"login": "owner"}})).collect::<Vec<_>>()))
                } else {
                    panic!("board repository read {method} {url}")
                };
            }
            if url.ends_with("/graphql") {
                let query = body.unwrap()["query"].as_str().unwrap().to_string();
                if query.contains("enqueuePullRequest") {
                    p.set(1);
                    return Ok(
                        json!({"data": {"enqueuePullRequest": {"mergeQueueEntry": {"id": "MQE_1"}}}}),
                    );
                }
                let entry = if p.get() == 1 {
                    json!({"id": "MQE_1"})
                } else {
                    Value::Null
                };
                return Ok(
                    json!({"data": {"node": {"headRefOid": h, "mergeable": "MERGEABLE", "merged": p.get() == 2, "mergeCommit": if p.get() == 2 { json!({"oid": m}) } else { Value::Null },
                    "mergeQueueEntry": entry, "timelineItems": {"nodes": []}}}}),
                );
            }
            let rest = url
                .strip_prefix("https://api.github.com/repos/o/x")
                .unwrap_or_else(|| panic!("read outside o/x: {method} {url}"));
            Ok(if rest.starts_with("/compare/") {
                json!({"status": if rest.ends_with(&m) { "identical" } else { "diverged" }})
            } else if rest == "/pulls/240" {
                json!({"number": 240, "node_id": "PR_240", "state": "open", "draft": false, "head": {"sha": h}, "merged_at": null})
            } else if rest == "/rules/branches/main" {
                json!([{"type": "merge_queue"}])
            } else if rest.starts_with("/actions/runs?") {
                json!({"workflow_runs": [{"head_sha": m, "event": "merge_group", "status": "completed", "conclusion": "success", "name": "ci", "check_suite_id": 5}]})
            } else if rest.contains("/check-runs") {
                json!({"check_runs": [{"name": "ci", "status": "completed", "conclusion": "success", "app": {"slug": "github-actions"}, "check_suite": {"id": 5}}]})
            } else if rest.ends_with("/status") {
                json!({"total_count": 0})
            } else {
                panic!("unexpected {method} {url}")
            })
        },
    ));
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a waiting publication")),
    );
    for _ in 0..3 {
        s.pass().unwrap();
    }
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert_eq!(
        (&row["state"], &row["enqueued"]),
        (&json!("review"), &json!("MQE_1")),
        "A2: the merge queue stage, as for a local PR: {row}"
    );
    phase.set(2);
    s.pass().unwrap();
    let closes: Vec<_> = b
        .comments(n)
        .unwrap()
        .into_iter()
        .filter(|e| e.body.starts_with("taskq: close"))
        .collect();
    assert_eq!((task(&b, n).state, closes.len()), (State::Closed, 1));
    assert!(
        closes[0].body.contains(&format!(
            "merged #240 in o/x as {merge}; merge-result CI green"
        )),
        "A1: {}",
        closes[0].body
    );
}

// Chain: section 7 Publication routes, the result is the merge commit of a PR of another repository (#2308) ->
// status names that PR while its merge-result CI runs -> green -> one close naming it, no board-repository PR.
// Defect (#2302 agentq#206): the PR read as a mention, the row showed the board repository's compare HTTP 404 and an
// empty reviewer slot, and close had no path. Retained: the close waits for the merge result's merge_group CI.
#[test]
fn a_result_merged_in_another_repository_names_its_pr_and_closes() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented merged foreign PR", "", &["taskq".into()])
        .unwrap();
    let (head, merge) = ("a".repeat(40), "b".repeat(40));
    let text = "Changed: o/x#206 merged as bbbbbbb\nChecks: A1 ok\nOpen: none";
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    let result = core::entry("result", &[&merge], Some(a), text);
    b.comment(n, &result).unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    let publish = core::entry("publish", &[], Some(r), text);
    b.comment(n, &publish).unwrap();
    let green = Rc::new(std::cell::Cell::new(false));
    let (g, h, m) = (green.clone(), head.clone(), merge.clone());
    b.state.borrow_mut().publish = "queue-foreign".into();
    b.state.borrow_mut().publisher = Some(crate::board::replying_to(
        &crate::board::github::KIND,
        move |method, url, _| {
            assert_eq!(method, "GET", "no publication write: {url}");
            if let Some(rest) = url.strip_prefix("https://api.github.com/repos/o/r") {
                return if rest.starts_with(&format!("/issues/{n}/comments")) {
                    Ok(json!([result.clone(), publish.clone()].iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "author_association": "OWNER",
                        "created_at": "2026-10-30T00:00:00Z", "updated_at": "2026-10-30T00:00:00Z", "user": {"login": "owner"}})).collect::<Vec<_>>()))
                } else {
                    panic!("board repository read {method} {url}")
                };
            }
            let rest = url
                .strip_prefix("https://api.github.com/repos/o/x")
                .unwrap_or_else(|| panic!("read outside o/x: {method} {url}"));
            let runs = if g.get() {
                json!([{"head_sha": m, "event": "merge_group", "status": "completed", "conclusion": "success", "name": "ci", "check_suite_id": 5}])
            } else {
                json!([])
            };
            Ok(if rest == format!("/compare/main...{m}") {
                json!({"status": "identical"})
            } else if rest == "/pulls/206" {
                json!({"number": 206, "state": "closed", "merged_at": "x", "merge_commit_sha": m, "head": {"ref": "pin", "sha": h}, "base": {"ref": "main"}})
            } else if rest == "/rules/branches/main" {
                json!([{"type": "merge_queue"}])
            } else if rest.starts_with("/actions/runs?") {
                json!({"workflow_runs": runs})
            } else if rest.contains("/check-runs") {
                json!({"check_runs": [{"name": "ci", "status": "completed", "conclusion": "success", "app": {"slug": "github-actions"}, "check_suite": {"id": 5}}]})
            } else if rest.ends_with("/status") {
                json!({"total_count": 0})
            } else {
                panic!("unexpected {method} {url}")
            })
        },
    ));
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a waiting publication")),
    );
    s.pass().unwrap();
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    let reason = row["publication"]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.starts_with("o/x#206: merge_group CI not green on merge result"),
        "A2: the row names the PR of o/x: {row}"
    );
    green.set(true);
    s.pass().unwrap();
    let closes: Vec<_> = b
        .comments(n)
        .unwrap()
        .into_iter()
        .filter(|e| e.body.starts_with("taskq: close"))
        .collect();
    assert_eq!((task(&b, n).state, closes.len()), (State::Closed, 1));
    assert!(
        closes[0].body.contains(&format!(
            "merged #206 in o/x as {merge}; merge-result CI green"
        )),
        "A1: {}",
        closes[0].body
    );
}

// Chain: § 7 rule 9 -> worker closes its own task -> § 1 dependency unknown -> § 4 step 5 reopen + one ask (#1950).
// Risk (desiign#50): issue text said "write done and close"; the worker commented done and closed the issue, so the
// close read as done, review was skipped and q-doing stayed on a closed issue.
#[test]
fn worker_close_during_attempt_reopens_with_one_ask() {
    let (b, c) = (Fake::default(), cfg("mac"));
    let n = b
        .create(
            "Invented: write done and close",
            "When finished, write done and close this issue.",
            &[TASK_LABEL.into()],
        )
        .unwrap();
    let exit = Rc::new(RefCell::new(None));
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| Ok(Box::new(FakeProc(exit.clone())))),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    let p = b
        .create("Invented: closed by hand", "", &[TASK_LABEL.into()])
        .unwrap();
    s.pass().unwrap();
    assert!(
        b.issue(n).unwrap().labels.contains(&"q-doing".to_string())
            && b.issue(p).unwrap().labels.contains(&"q-ready".to_string())
    );
    // the worker's hand-in by board writes: a plain comment and a close, labels untouched
    b.raw(n, "done", true, false);
    b.close(n).unwrap();
    b.close(p).unwrap();
    let dep = blockers(&b, &format!("Depends on #{n}"), None, &mut DepCache::new());
    assert_eq!(
        dep.iter().map(|d| d.reason.as_str()).collect::<Vec<_>>(),
        ["unknown: closed during a worker attempt"],
        "a dependent never reads it done"
    );
    s.pass().unwrap();
    let (issue, t) = (b.issue(n).unwrap(), task(&b, n));
    assert!(
        issue.open && t.state == State::Ask && t.attempt == Some(a),
        "{t:?}"
    );
    assert!(
        t.card.as_deref().unwrap().contains("closed at")
            && t.card.as_deref().unwrap().contains("1. redo:"),
        "{:?}",
        t.card
    );
    assert!(
        issue.labels.contains(&"q-ask".to_string())
            && !issue.labels.contains(&"q-doing".to_string()),
        "{:?}",
        issue.labels
    );
    assert_eq!(s.kids.len(), 1, "the live attempt is not stopped as gone");
    let p_issue = b.issue(p).unwrap();
    assert!(
        !p_issue.open && !p_issue.labels.iter().any(|l| l.starts_with("q-")),
        "a closed task keeps no lifecycle label: {:?}",
        p_issue.labels
    );
    s.pass().unwrap();
    let asks = b
        .comments(n)
        .unwrap()
        .iter()
        .filter(|c| c.body.starts_with("taskq: ask"))
        .count();
    assert_eq!(asks, 1, "one ask per close");
    b.comment(n, "taskq: answer 1").unwrap();
    s.pass().unwrap();
    assert_eq!(
        task(&b, n).state,
        State::Doing,
        "the answer resumes the attempt"
    );
}

// Chain: section 7 publication -> superseded by merged PRs -> ask with close -> `answer N.3` closes, no merge (#1779, pilotq#84).
// Risk: the card says close by hand, `close` is refused in `ask` and retry only asks again, so the task never ends.
#[test]
fn superseded_publication_closes_from_its_answer_without_a_merge() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented superseded publication", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    b.state.borrow_mut().publish = "superseded".into();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("no process for a superseded publication")),
    );
    s.pass().unwrap();
    let t = task(&b, n);
    assert_eq!(t.state, State::Ask);
    let card = t.card.unwrap();
    assert!(card.contains("1. retry:") && card.contains("2. later:") && card.contains("3. close: superseded, no merge - + merged #86 at ddddddd carries the work; - this result is not merged ★"), "{card}");
    let calls = b.state.borrow().publications;
    let got =
        crate::dispatch::answer_picks(&b, &c, &[format!("{n}.3")], &crate::dispatch::Op::default())
            .unwrap()
            .remove(0);
    assert!(got.ok(), "{got:?}");
    assert_eq!(task(&b, n).state, State::Closed);
    assert!(!b.issue(n).unwrap().open);
    assert_eq!(b.state.borrow().publications, calls, "no merge");
    s.pass().unwrap();
    assert_eq!(
        b.state.borrow().publications,
        calls,
        "no publication after close"
    );
}

// Chain: section 7 publication-blocked ask -> `answer N --text "close: .."` is option close (#1820, pilotq #106).
// Risk: the owner's `close:` text reads as free text, publication resumes and the same block asks again.
#[test]
fn blocked_publication_answer_text_with_an_option_key_applies_it_or_refuses() {
    let (b, c) = (Fake::default(), cfg("win"));
    let blocked = |kind: &str| {
        let n = b
            .create("Invented blocked publication", "", &["taskq".into()])
            .unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(
            n,
            &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
        )
        .unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(
            n,
            &core::entry(
                "publish",
                &[],
                Some(r),
                "Changed: x
Checks: A1 ok
Open: none",
            ),
        )
        .unwrap();
        b.state.borrow_mut().publish = kind.into();
        Service::new(
            &b,
            &c,
            "test",
            Box::new(|_| panic!("no process for a blocked publication")),
        )
        .pass()
        .unwrap();
        assert_eq!(task(&b, n).state, State::Ask, "{kind}");
        b.state.borrow_mut().publish = String::new();
        n
    };
    let op = crate::dispatch::Op::default();
    let n = blocked("superseded");
    let calls = b.state.borrow().publications;
    let got = crate::dispatch::answer_text(&b, &c, n, "close: superseded by #86, no merge", &op)
        .unwrap()
        .remove(0);
    assert!(got.ok(), "{got:?}");
    assert_eq!(task(&b, n).state, State::Closed);
    assert_eq!(b.state.borrow().publications, calls, "no merge");
    let n = blocked("queue-blocked");
    let err = crate::dispatch::answer_text(&b, &c, n, "Close: drop it", &op).unwrap_err();
    assert!(
        err.contains(&format!("#{n} has no `close` option"))
            && err.contains(&format!("`taskq answer {n}.1` (retry)")),
        "{err}"
    );
    assert_eq!(task(&b, n).state, State::Ask, "a refusal writes nothing");
    let got = crate::dispatch::answer_text(&b, &c, n, "later: after the freeze", &op)
        .unwrap()
        .remove(0);
    assert!(got.ok(), "{got:?}");
    assert_eq!(task(&b, n).state, State::Later);
    let n = blocked("queue-blocked");
    let got = crate::dispatch::answer_text(&b, &c, n, "token renewed, publish again", &op)
        .unwrap()
        .remove(0);
    assert_eq!(
        got.state,
        Some("review"),
        "free text still resumes publication: {got:?}"
    );
}

// Chain: R1 hand-typed cancellation -> queue readback -> allow replacement; unknown never starts a new worker.
#[test]
fn hand_typed_cancellation_dequeues_before_another_attempt() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented cancellation", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    b.comment(n, "taskq: publish\naccepted").unwrap();
    b.comment(n, "taskq: enqueued MQ_1").unwrap();
    b.comment(n, "taskq: requeue main-moved").unwrap();
    b.state.borrow_mut().fail_publication_cancel = true;
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("replacement before cancellation readback")),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    assert!(task(&b, n).attempt.is_none());
    b.state.borrow_mut().fail_publication_cancel = false;
    s.focus.clear();
    s.pass().unwrap();
    let before = b.state.borrow().cancelled_publications.len();
    s.pass().unwrap();
    assert_eq!(
        b.state.borrow().cancelled_publications.len(),
        before,
        "verified cancellation cached by its entry"
    );
    assert_eq!(
        b.state.borrow().cancelled_publications.last(),
        Some(&(n, "a".repeat(40)))
    );
    b.comment(n, "taskq: spawn worker codex:w2 other").unwrap();
    s.pass().unwrap();
    assert_eq!(
        b.state.borrow().cancelled_publications.len(),
        before,
        "a later revision does not repeat the cancellation"
    );
}

// Defect #1535: a stale view of a re-produced SHA dequeued the newer publication (PR #1529 on #1470).
#[test]
fn cancellation_never_dequeues_a_newer_publication_of_the_same_sha() {
    let (b, c) = (Fake::default(), cfg("win"));
    let n = b
        .create("Invented republication", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    b.comment(n, "taskq: publish\naccepted").unwrap();
    b.comment(n, "taskq: requeue main-moved").unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w2 other").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "same SHA"),
    )
    .unwrap();
    // a restarted service reads the task in review before the new publish entry
    let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process")));
    s.pass().unwrap();
    b.comment(n, "taskq: publish\naccepted").unwrap();
    b.comment(n, "taskq: enqueued MQ_2").unwrap();
    s.pass().unwrap();
    assert!(
        b.state.borrow().cancelled_publications.is_empty(),
        "the newer publication stays queued"
    );
}

// Chain: R1, § 2 Summary -> resumed publication close -> pass repair. Risk (#2017 A2, A5): the close copies the
// publish verdict instead of naming it, the summary loses the verdict, or a stale or deleted summary stays wrong.
#[test]
fn close_names_its_publish_and_the_pass_repairs_the_summary() {
    let (b, c) = (Fake::default(), cfg("win"));
    b.state.borrow_mut().summaries = true;
    let n = b.create("Summary repair", "", &["taskq".into()]).unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &["a".repeat(40).as_str()], Some(a), "Changed: x"),
    )
    .unwrap();
    let publish = b
        .comment(
            n,
            "taskq: publish\nChanged: the menu shows the summary\nOpen: none",
        )
        .unwrap();
    let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process")));
    s.pass().unwrap();
    let log = entries(&b, n).unwrap();
    let close = log
        .iter()
        .rfind(|e| core::kind(n, &e.body) == "close")
        .expect("publication resumed and closed");
    let text = core::entry_text(n, &close.body);
    assert!(
        text.starts_with(&format!("{}{publish}\n", core::REPEATS)),
        "{text}"
    );
    assert!(
        !text.contains("Changed:"),
        "the close names the publish instead of copying it"
    );
    assert!(
        core::verdict(n, &log, text)
            .starts_with("Changed: the menu shows the summary\nOpen: none\n"),
        "readers show the publish text"
    );
    let closed = |b: &Fake| {
        b.comments(n)
            .unwrap()
            .into_iter()
            .filter(crate::summary::is)
            .collect::<Vec<_>>()
    };
    s.pass().unwrap();
    let want = crate::summary::text(&b, n, "", &log, &core::fold(n, &log, false), None);
    assert!(
        want.starts_with(&format!("{}\n**Closed**", crate::summary::MARKER))
            && want.contains("\nChanged: the menu shows the summary\nOpen: none"),
        "{want}"
    );
    assert_eq!(
        closed(&b)
            .iter()
            .map(|c| c.body.as_str())
            .collect::<Vec<_>>(),
        [want.as_str()],
        "the state change created it"
    );
    // an open task: a stale summary is rewritten from the log, a deleted one recreated; an older task without one
    // keeps none until its state changes
    let summary = |n: u64| {
        b.comments(n)
            .unwrap()
            .into_iter()
            .filter(crate::summary::is)
            .collect::<Vec<_>>()
    };
    let (m, old) = (
        b.create("Parked", "", &["taskq".into()]).unwrap(),
        b.create("Older", "", &["taskq".into()]).unwrap(),
    );
    for k in [m, old] {
        b.comment(k, "taskq: later\nReason: Waits for the owner.")
            .unwrap();
    }
    let log = entries(&b, m).unwrap();
    crate::summary::write(
        &b,
        m,
        "",
        &b.comments(m).unwrap(),
        &log,
        &core::fold(m, &log, false),
        None,
    )
    .unwrap();
    let want = summary(m)[0].body.clone();
    assert_eq!(
        want,
        format!(
            "{}\n**Later**\nReason: Waits for the owner.",
            crate::summary::MARKER
        )
    );
    s.pass().unwrap();
    let id = summary(m)[0].id;
    b.state
        .borrow_mut()
        .comments
        .iter_mut()
        .find(|(_, c)| c.id == id)
        .unwrap()
        .1
        .body = format!("{}\n**Ready**", crate::summary::MARKER);
    s.pass().unwrap();
    assert_eq!(
        summary(m)
            .iter()
            .map(|c| (c.id, c.body.as_str()))
            .collect::<Vec<_>>(),
        [(id, want.as_str())],
        "a stale summary is rewritten"
    );
    b.state.borrow_mut().comments.retain(|(_, c)| c.id != id);
    s.pass().unwrap();
    assert_eq!(
        summary(m)
            .iter()
            .map(|c| c.body.as_str())
            .collect::<Vec<_>>(),
        [want.as_str()],
        "a deleted summary is recreated"
    );
    assert!(
        summary(old).is_empty(),
        "an older task gets its summary at its next state change"
    );
    assert_eq!(
        core::fold(m, &entries(&b, m).unwrap(), false).state,
        State::Later
    );
}

type Exits = Rc<RefCell<Vec<(u64, Rc<RefCell<Option<i32>>>)>>>;

// H-A5: a childless service can still own an uncertain native execution.
#[test]
fn project_drain_retains_unresolved_execution_until_release() {
    use std::fs;
    let b = Fake::default();
    let mut c = cfg("mac");
    c.root = std::env::temp_dir().join(format!("taskq-drain-{}", crate::runtime::uuid()));
    c.install = c.root.join("install");
    fs::create_dir_all(crate::paths::dot(&c.root)).unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
        "fixture holds admission".into(),
    ))));
    host.lock().unwrap().draining.insert(c.root.clone());
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|_| panic!("drain must never submit work")),
    );
    s.host = Some(host.clone());
    b.state.borrow_mut().fail_comment_reads = 1;
    assert!(
        crate::service_run::stopped_handoff(&mut s, &host, &c).is_err(),
        "no board observation is not an idle proof"
    );
    s.pass().unwrap();
    assert!(s.kids.is_empty());
    assert!(s.retained_attempts().contains(&a));
    assert!(
        !crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap(),
        "missing handle must retain the project and its slot"
    );
    assert_eq!(host.lock().unwrap().busy.get(&c.root), Some(&true));
    b.labels(n, &[], &[TASK_LABEL.into()]).unwrap();
    s.pass().unwrap();
    assert!(
        s.retained_attempts().contains(&a),
        "label removal is not native termination"
    );
    b.state
        .borrow_mut()
        .comments
        .iter_mut()
        .find(|(_, comment)| comment.id == a)
        .unwrap()
        .1
        .edited = true;
    s.pass().unwrap();
    assert!(
        !s.idle(),
        "missing execution history cannot release an observed execution"
    );
    b.state
        .borrow_mut()
        .comments
        .iter_mut()
        .find(|(_, comment)| comment.id == a)
        .unwrap()
        .1
        .edited = false;
    b.close(n).unwrap();
    b.state.borrow_mut().fail_comment_reads = 1;
    assert!(s.pass().is_err());
    assert!(s.retained_attempts().contains(&a));
    assert!(
        !crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap(),
        "failed ownership read cannot certify idle"
    );
    s.pass().unwrap();
    assert!(
        s.retained_attempts().contains(&a),
        "leaving the open-issue list is not native termination"
    );
    assert!(!crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap());
    release_execution(&b, n, a, a, "exit").unwrap();
    s.tasks_read = Some(std::time::Instant::now() - std::time::Duration::from_secs(c.poll.max(1)));
    assert!(
        crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap(),
        "read-only monitor must observe terminal delivery and complete drain"
    );
    let n = b.create("Recovered", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:recovered mac")
        .unwrap();
    s.pass().unwrap();
    assert!(s.retained_attempts().contains(&a));
    let exit = Rc::new(RefCell::new(None));
    s.adopt(Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: a,
        task: n,
        attempt: a,
        rt: "codex".into(),
        proc: Box::new(FakeProc(exit.clone())),
    })
    .unwrap();
    assert!(!crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap());
    *exit.borrow_mut() = Some(0);
    assert!(
        crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap(),
        "reduced monitoring must retire the recovered reservation after exact release"
    );
    b.state.borrow_mut().fail_comments = 1;
    s.ownership_known = false;
    assert!(
        crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap(),
        "read-only recovery can prepare without resuming ordinary board writes"
    );
    assert_eq!(
        b.state.borrow().fail_comments,
        1,
        "recovery must not try an ordinary task write"
    );
    drop(s);
    fs::remove_dir_all(&c.root).unwrap();
}

fn service<'a>(b: &'a Fake, c: &'a Config, exits: Exits) -> Service<'a> {
    let mut s = Service::new(
        b,
        c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            exits.borrow_mut().push((l.task, flag.clone()));
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    s
}

fn task(b: &Fake, n: u64) -> Task {
    core::fold(n, &entries(b, n).unwrap(), false)
}

/// A project root holding the attempt log a start writes before its native call: a started execution's evidence (#1879).
fn started_root(tag: &str, n: u64, a: u64) -> PathBuf {
    let root = std::env::temp_dir().join(format!("taskq-{tag}-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    std::fs::write(crate::runtime::attempt_log(&root, "worker", n, a), "").unwrap();
    root
}

// #2362: status exposes a logged start, but only read-back eligibility admits it; retries cannot replace active work.
#[test]
fn logged_start_waits_for_eligibility_before_admission() {
    let b = Fake::default();
    let c = cfg("mac");
    for (body, edited) in [
        ("taskq: requeue", false),
        ("taskq: start-invalid", false),
        ("taskq: start", true),
    ] {
        let held = b.create("Still held", "", &["q-held".into()]).unwrap();
        b.comment(held, "taskq: hold").unwrap();
        let id = b.comment(held, body).unwrap();
        if edited {
            b.edit_comment(held, id, &format!("{body}\nedited"))
                .unwrap();
        }
        let snap = crate::snapshot::read(&b, &c, &json!({}), &[]);
        assert_eq!(
            snap.tasks.iter().find(|r| r.n == held).unwrap().task.state,
            State::Held,
            "only an accepted start overrides held labels"
        );
        assert!(
            !crate::cli::task_cmd::start_one(
                &b,
                &c.root,
                held,
                &crate::Op {
                    revision: Some(0),
                    ..Default::default()
                }
            )
            .ok()
        );
    }
    let n = b.create("Logged start", "", &["q-held".into()]).unwrap();
    b.comment(n, "taskq: hold").unwrap();
    b.comment(n, "taskq: start").unwrap();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert!(
        s.kids.is_empty(),
        "a logged start without taskq is never admitted"
    );
    assert_eq!(
        b.issue(n).unwrap().labels,
        ["q-held"],
        "the service preserves discoverability for repair"
    );
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap();
    assert_eq!(row.task.state, State::Ready);
    assert_eq!(row.json()["admission"]["code"], "labels");
    assert_eq!(entries(&b, n).unwrap().len(), 2);
    assert!(
        crate::cli::task_cmd::start_tasks(&b, &c.root, &[n.to_string()], &crate::Op::default())
            .unwrap()[0]
            .ok()
    );
    s.pass().unwrap();
    assert_eq!(s.kids.len(), 1, "the repaired task is admitted");
    let attempt = task(&b, n).attempt;
    b.labels(n, &["q-held".into()], &["q-doing".into(), "taskq".into()])
        .unwrap();
    assert!(
        !crate::cli::task_cmd::start_tasks(&b, &c.root, &[n.to_string()], &crate::Op::default())
            .unwrap()[0]
            .ok()
    );
    assert_eq!(
        task(&b, n).attempt,
        attempt,
        "a retry never replaces active work"
    );
    assert_eq!(s.kids.len(), 1);
}

// Chain: § 1 held, a person adds `taskq` on the board. Risk: the label path leaves a held log under a `taskq` label,
// so the task never starts, or a plain labelled issue gets a stray `start`.
#[test]
fn a_person_adding_taskq_to_a_held_issue_starts_it() {
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits.clear();
    let held = b.create("H", "", &["q-held".into()]).unwrap();
    b.comment(held, "taskq: hold").unwrap();
    let plain = b.create("P", "", &[TASK_LABEL.into()]).unwrap();
    b.labels(held, &[TASK_LABEL.into()], &[]).unwrap();
    assert_eq!(task(&b, held).state, State::Held);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, held).state, State::Ready);
    assert_eq!(
        entries(&b, held)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with("taskq: start"))
            .count(),
        1,
        "one start over two passes"
    );
    assert!(!b.issue(held).unwrap().labels.contains(&"q-held".into()));
    assert!(
        entries(&b, plain)
            .unwrap()
            .iter()
            .all(|e| !e.body.starts_with("taskq: start"))
    );
    assert_eq!(task(&b, plain).state, State::Ready);
}

// #2247 A1, A2: after a live push win selected 4.6.62 but kept running 4.6.61 for its live workers, and the
// heartbeat named only the running version. Protection: equal versions write no `selected` line; a pending
// handover writes the selected version and the live attempts it waits for, else the pending restart.
#[test]
fn heartbeat_names_selected_version_and_waiting_attempts() {
    let b = Fake::default();
    let c = cfg("mac");
    let n = b.create("T", "## Goal\ng", &[TASK_LABEL.into()]).unwrap();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    let attempt = s.kids[0].attempt;
    std::fs::create_dir_all(&c.install).unwrap();
    std::fs::write(c.install.join("current"), &s.version).unwrap();
    assert_eq!(s.selected_line(), None, "equal versions: one value");
    assert!(
        !s.heartbeat_body()
            .lines()
            .any(|l| l.starts_with("selected "))
    );
    std::fs::write(c.install.join("current"), "9.9.9").unwrap();
    assert_eq!(
        s.selected_line().as_deref(),
        Some(
            if crate::service_run::lifecycle_waits(&s, false).is_empty() {
                "selected 9.9.9 waits restart pending".to_string()
            } else {
                format!("selected 9.9.9 waits #{n} attempt {attempt}")
            }
            .as_str()
        )
    );
    s.kids[0].stopping = false;
    s.observations.clear();
    let waits = crate::service_run::lifecycle_waits(&s, true);
    assert!(waits.contains(&format!("#{n}")), "{waits:?}");
    std::fs::write(c.install.join("current"), "../x").unwrap();
    assert_eq!(
        s.selected_line(),
        None,
        "an invalid selection is never published"
    );
}

// #1435: a stale childless execution held the Windows image exit forever. Risk kept: handover and stop still wait on it.
#[test]
fn windows_exit_passes_childless_unresolved_execution() {
    let b = Fake::default();
    let c = cfg("mac");
    let n = b.create("T", "## Goal\ng", &[TASK_LABEL.into()]).unwrap();
    let mut old = service(&b, &c, Default::default());
    old.pass().unwrap();
    assert_eq!(old.kids.len(), 1);
    drop(old);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert!(s.kids.is_empty());
    assert!(s.execution_waits[&n].contains("termination unconfirmed"));
    // #1594 A3 (#947): the published heartbeat row and the handover wait read the same childless verdict
    let row = &s.execution_statuses()[0];
    assert_eq!(
        (row.state.as_str(), row.reason.as_str()),
        ("unknown", s.execution_waits[&n].as_str())
    );
    assert!(
        s.handoff_waits(true, false)
            .iter()
            .any(|w| w.contains("unresolved execution")),
        "handover keeps waiting"
    );
    assert!(
        s.handoff_waits(true, true).is_empty(),
        "image exit leaves the record to the next image: {:?}",
        s.handoff_waits(true, true)
    );
    s.pending.insert((n, 1));
    assert!(
        s.handoff_waits(true, true).is_empty(),
        "#1601: a childless pending attempt passes to the next image"
    );
    assert!(
        !s.handoff_waits(false, false).is_empty(),
        "outside a handover it still waits"
    );
    s.pending.remove(&(n, 1));
    if cfg!(windows) {
        assert!(crate::service_run::lifecycle_waits(&s, false).is_empty());
    }
    assert!(
        !crate::service_run::lifecycle_waits(&s, true).is_empty(),
        "stop still waits"
    );
}

// #2228: on win a handed-in Claude resume whose native job stayed ambiguous (pid 0) held the 4.6.59 image exit.
// #2297: the 4.6.64 win upgrade then waited over 600 s: a live codex child deferred the exit, and every pass failed
// "handoff unknown: Claude original process identity unavailable" on the pid-0 turn, so no project was prepared.
// Retained: app turns and Stop wait; the live child runs on and the next image rebuilds both from the board.
#[test]
fn upgrade_passes_live_child_and_turn_without_observed_process() {
    struct NoPid;
    impl Proc for NoPid {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Err("Claude original input matches multiple native candidates; inspect them before release".into())
        }
        fn kill(&mut self) -> Result<(), String> {
            Err("Claude stop identity unavailable".into())
        }
        fn id(&self) -> u32 {
            0
        }
        fn native_identity(&self) -> Result<coreq::process::Identity, String> {
            Err("Claude original process identity unavailable".into())
        }
    }
    let b = Fake::default();
    let c = cfg("mac");
    std::fs::create_dir_all(crate::paths::dot(&c.root)).unwrap();
    let mut s = service(&b, &c, Default::default());
    s.ownership_known = true;
    let kid = |task, rt: &str, proc: Box<dyn Proc>| Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: task,
        task,
        attempt: task,
        rt: rt.into(),
        proc,
    };
    s.kids.push(kid(1, "claude", Box::new(NoPid)));
    s.kids.push(kid(
        2,
        "codex",
        Box::new(FakeProc(Rc::new(RefCell::new(None)))),
    ));
    assert!(
        crate::runtime::handover_waits("codex-app"),
        "an app turn still defers the exit"
    );
    assert!(
        crate::service_run::lifecycle_waits(&s, false).is_empty(),
        "{:?}",
        crate::service_run::lifecycle_waits(&s, false)
    );
    assert_eq!(
        crate::service_run::lifecycle_waits(&s, true),
        ["#1", "#2"],
        "stop still waits"
    );
    let mut h = crate::host::Host::new(Err("fixture".into()));
    assert!(h.begin_pass(&c.root));
    h.restart = true;
    crate::service_run::report_handoff(&s, &mut h, &c.root, false).unwrap();
    assert!(
        h.handoff_ready(),
        "one pass prepares the re-exec: {:?}",
        h.handoff_wait()
    );
    let _ = std::fs::remove_dir_all(&c.root);
}

// Chain: #1359 § 3 claim -> launch -> acceptance, one pass. Risk: a claim rereads the task log the pass just read.
#[test]
fn one_claim_reads_its_task_log_once() {
    let (b, c) = (Fake::default(), cfg("mac"));
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
    s.pass().unwrap();
    assert_eq!(s.kids.len(), 1);
    // was 6: views, pre-spawn, spawn readback, launch, acceptance before and after; the two readbacks stay (§ 3)
    assert_eq!(
        b.state.borrow().comment_reads,
        3,
        "views, spawn readback, acceptance readback"
    );
    assert_eq!(task(&b, n).state, State::Doing);
}

// Chain: #1359 § 7 waiting publication -> service pass on the owning and another host. Risk: every pass rereads
// the issue and log that `views` just read, and every host polls the same publication.
#[test]
fn a_waiting_publication_reads_nothing_twice_and_only_its_host_polls() {
    let b = Fake::default();
    let n = b
        .create("Invented publication", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
    b.comment(
        n,
        &core::entry(
            "publish",
            &[],
            Some(r),
            "Changed: x\nChecks: A1 ok\nOpen: none",
        ),
    )
    .unwrap();
    b.state.borrow_mut().publish = "queue-waiting".into();
    let (mut win, mut mac) = (cfg("win"), cfg("mac"));
    // A shared /nonexistent can contain other fixtures' profile rows, which add unrelated board reads.
    let install =
        std::env::temp_dir().join(format!("taskq-publication-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&install).unwrap();
    win.install = install.clone();
    mac.install = install.clone();
    win.root = install.join("win");
    mac.root = install.join("mac");
    let mut owner = Service::new(
        &b,
        &win,
        "test",
        Box::new(|_| panic!("no process while queued")),
    );
    let mut other = Service::new(
        &b,
        &mac,
        "test",
        Box::new(|_| panic!("no process while queued")),
    );
    let count = |b: &Fake| {
        let s = b.state.borrow();
        (s.reads, s.comment_reads, s.publications)
    };
    let mut passes = vec![];
    for round in 0..3 {
        // a log read within SETTLE of its last write is read once more on a later pass (#1718)
        if round > 0 {
            b.advance(SETTLE);
        }
        for s in [&mut owner, &mut other] {
            let before = count(&b);
            s.pass().unwrap();
            let after = count(&b);
            passes.push((after.0 - before.0, after.1 - before.1, after.2 - before.2));
        }
    }
    // (issue reads, comment reads, publication polls) per pass, owner then other; was (1, 3, 1) on both hosts;
    // the second round still reads the log its first round read within SETTLE, the settled third reads none
    assert_eq!(
        passes,
        [
            (0, 1, 1),
            (0, 1, 0),
            (0, 1, 1),
            (0, 1, 0),
            (0, 0, 1),
            (0, 0, 0)
        ]
    );
    // a stale owner heartbeat hands the poll to any host
    b.state.borrow_mut().clock += win.ttl + 1;
    let before = count(&b).2;
    other.pass().unwrap();
    assert_eq!(
        count(&b).2 - before,
        1,
        "no host polls a publication whose owner is gone"
    );
    std::fs::remove_dir_all(install).unwrap();
    assert_eq!(task(&b, n).state, State::Review);
}

// Chain: §5/R12 board reply files -> verified copies -> native start. Risk: missing files acknowledge the inbox or launch a partial reply.
#[test]
fn reply_files_hold_submission_until_every_attachment_is_readable() {
    let root = std::env::temp_dir().join(format!("taskq-reply-launch-{}", crate::runtime::uuid()));
    std::fs::create_dir(&root).unwrap();
    let mut c = cfg("mac");
    c.root = root.clone();
    c.install = root.clone();
    let b = Fake::default();
    let n = b
        .create("Attached reply", "", &[TASK_LABEL.into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:session mac")
        .unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    let original = root.join("document.bin");
    std::fs::write(&original, b"invented file bytes").unwrap();
    let r = crate::dispatch::act_files(
        &b,
        &root,
        n,
        "steer",
        "Read the file",
        &crate::dispatch::Op::default(),
        &[original.display().to_string()],
    );
    assert!(r.ok(), "{}", r.text());
    let log = entries(&b, n).unwrap();
    let through = log.last().unwrap().id;
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = calls.clone();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(move |l| {
            seen.borrow_mut().push(l.files.clone());
            Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))) as Box<dyn Proc>)
        }),
    );
    let l = Launch {
        files: String::new(),
        execution: None,
        task: n,
        title: String::new(),
        attempt: a,
        role: "worker".into(),
        rt: "claude".into(),
        session: "session".into(),
        url: b.url(n),
        resume: Some("Read the file".into()),
    };
    b.state.borrow_mut().bad_image = true;
    let before = core::executions(n, &log);
    s.launch(l, through, log).unwrap();
    assert!(calls.borrow().is_empty());
    assert_eq!(s.delivered(a), 0);
    assert_eq!(s.held.len(), 1);
    assert_eq!(
        core::executions(n, &entries(&b, n).unwrap()),
        before,
        "failed file delivery cannot create execution intent"
    );
    b.state.borrow_mut().bad_image = false;
    let l = s.held.remove(0);
    s.launch(l, through, entries(&b, n).unwrap()).unwrap();
    assert_eq!(calls.borrow().len(), 1);
    assert!(calls.borrow()[0].contains("untrusted data"));
    assert!(calls.borrow()[0].contains("document.bin"));
    assert_eq!(s.delivered(a), through);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

// R12 / §3: a last-moment admission hold is not a failed native submission.
#[test]
fn admission_wait_preserves_worker_reviewer_and_resume_execution() {
    for role in ["worker", "reviewer", "resume"] {
        let b = Fake::default();
        let c = cfg("mac");
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let w = b
            .comment(n, "taskq: spawn worker codex:session mac")
            .unwrap();
        let a = if role == "reviewer" {
            accept_execution(&b, n, w, w, None).unwrap();
            release_execution(&b, n, w, w, "exit").unwrap();
            b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40)))
                .unwrap();
            b.comment(n, "taskq: spawn reviewer codex:session mac")
                .unwrap()
        } else {
            w
        };
        let through = if role == "resume" {
            accept_execution(&b, n, a, a, None).unwrap();
            release_execution(&b, n, a, a, "exit").unwrap();
            b.comment(n, "taskq: steer\ncontinue").unwrap()
        } else {
            a
        };
        let calls = Rc::new(RefCell::new(0));
        let called = calls.clone();
        let mut s = Service::new(
            &b,
            &c,
            "test",
            Box::new(move |_| {
                *called.borrow_mut() += 1;
                if *called.borrow() == 1 {
                    Err(crate::runtime::StartError::admission(
                        "native inventory pending",
                    ))
                } else {
                    Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))) as Box<dyn Proc>)
                }
            }),
        );
        let l = Launch {
            files: String::new(),
            execution: None,
            task: n,
            title: "T".into(),
            attempt: a,
            role: if role == "reviewer" {
                "reviewer"
            } else {
                "worker"
            }
            .into(),
            rt: "codex".into(),
            session: "session".into(),
            url: b.url(n),
            resume: (role == "resume").then(|| "continue".into()),
        };
        s.launch(l, through, entries(&b, n).unwrap()).unwrap();
        assert_eq!(
            task(&b, n).attempt,
            Some(a),
            "{role}: admission consumed the attempt"
        );
        assert_eq!(task(&b, n).retries, 0);
        assert_eq!(s.held.len(), 1, "{role}: pending launch lost");
        assert!(s.kids.is_empty());
        assert_eq!(s.delivered(a), 0);
        let before = core::executions(n, &entries(&b, n).unwrap());
        assert_eq!(before.last().unwrap().release, None);
        let held = s.held.remove(0);
        s.launch(held, a, entries(&b, n).unwrap()).unwrap();
        let after = core::executions(n, &entries(&b, n).unwrap());
        assert_eq!(
            after.len(),
            before.len(),
            "admission retry duplicated execution intent"
        );
        assert_eq!(s.kids.len(), 1);
        assert_eq!(s.delivered(a), through);
        assert_eq!(*calls.borrow(), 2);
        assert_eq!(task(&b, n).retries, 0);
    }
}

// Chain: Resume process -> project wakeup -> separate scheduler process -> durable admission evidence.
// Board and process adapters are fakes; real command functions, scheduler and shared collector run unchanged.
#[test]
fn resume_handoff_links_the_service_decision() {
    let dir = std::env::temp_dir().join(format!("qh-{}", std::process::id()));
    for held in [true, false] {
        std::fs::create_dir_all(dir.join(".taskq")).unwrap();
        std::fs::create_dir_all(dir.join("i")).unwrap();
        std::fs::write(dir.join(".taskq/focus"), "off").unwrap();
        std::fs::write(dir.join(".taskq/focus.paused"), "all").unwrap();
        if held {
            std::fs::write(dir.join("i/hold"), "private owner reason").unwrap();
        }
        for mode in ["resume", "service"] {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "service::tests::resume_handoff_child",
                    "--ignored",
                ])
                .env("TASKQ_HANDOFF_ROOT", &dir)
                .env("TASKQ_HANDOFF_MODE", mode)
                .env_remove("COREQ_TRACE_CONTEXT")
                .env("TASKQ_INSTALL", dir.join("i"))
                .env("TASKQ_ROOT", &dir)
                .env("TASKQ_INCIDENT_REPORTS", "off")
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
        }
        let reader =
            coreq::telemetry::Reader::open(&dir.join("i/telemetry.log/universal.sqlite3")).unwrap();
        let mut observations = Vec::new();
        let mut after = 0;
        loop {
            let page = reader.export(0, after, 1024).unwrap();
            let rows = page["events"].as_array().unwrap();
            if rows.is_empty() {
                break;
            }
            let next = rows.last().unwrap()["seq"].as_i64().unwrap();
            assert!(next > after, "export cursor did not advance");
            after = next;
            observations.extend(rows.iter().map(|r| r["observation"].clone()));
        }
        let terminal = |name: &str| {
            observations
                .iter()
                .find(|r| r["operation"] == name && r["kind"] == "method")
                .unwrap()
        };
        let command = terminal("taskq.resume");
        let poke = terminal("taskq.poke");
        let write = terminal("taskq.operation.wake.write");
        let wake = terminal("taskq.continuation");
        assert_eq!(poke["trace"], command["trace"]);
        assert_eq!(write["trace"], command["trace"]);
        assert_eq!(write["parent"], poke["span"]);
        assert_eq!(write["outcome"], "ok");
        assert_eq!(wake["trace"], command["trace"]);
        assert_eq!(wake["parent"], write["span"]);
        assert_eq!(
            terminal("taskq.service.Service.pass")["trace"],
            command["trace"]
        );
        if held {
            assert!(observations.iter().any(|r| r["operation"]
                == "taskq.service.Service.may_launch"
                && r["kind"] == "method"
                && r["trace"] == command["trace"]
                && r["outcome"] == "refused"
                && r["error"] == "hold"));
            assert!(
                !observations
                    .iter()
                    .any(|r| r["operation"] == "taskq.service.Service.launch")
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("i/hold")).unwrap(),
                "private owner reason"
            );
        } else {
            let launch = terminal("taskq.service.Service.launch");
            assert_eq!(launch["trace"], command["trace"]);
            assert_eq!(launch["outcome"], "ok");
        }
        if !held {
            assert!(observations.iter().any(|r| r["kind"] == "stage"
                && r["operation"] == "start"
                && r["trace"] == command["trace"]));
            assert!(
                observations.iter().any(|r| r["kind"] == "stage"
                    && r["open"] == true
                    && r["version"] == "fixture")
            );
        }
        assert!(
            !dir.join("i/telemetry.log/telemetry.sqlite3").exists(),
            "scheduler must not open a second live sink"
        );
        let text = serde_json::to_string(&observations).unwrap();
        assert!(!text.contains("private owner reason") && !text.contains(dir.to_str().unwrap()));
        drop(reader);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
#[ignore = "subprocess helper exercised by resume_handoff_links_the_service_decision"]
fn resume_handoff_child() {
    let dir = PathBuf::from(std::env::var_os("TASKQ_HANDOFF_ROOT").unwrap());
    crate::profile::init(&dir.join("i"));
    let _runtime = coreq::telemetry::capture::Runtime::install_bounded(
        &dir.join("i/telemetry.log/universal.sqlite3"),
        "taskq",
        "fixture",
        crate::telemetry::budget(&dir.join("i"), "telemetry.sqlite3").unwrap(),
    )
    .unwrap();
    if std::env::var("TASKQ_HANDOFF_MODE").unwrap() == "resume" {
        crate::cli::service_cmd::resume(&dir).unwrap();
    } else {
        let mut seen = None;
        let link = crate::tests::wake_context(&dir.join(".taskq/poke"), &mut seen)
            .unwrap()
            .unwrap();
        assert!(
            crate::tests::wake_context(&dir.join(".taskq/poke"), &mut seen)
                .unwrap()
                .is_none()
        );
        let _wakeup = link.span().entered();
        let mut c = cfg("fixture");
        (c.root, c.install) = (dir.clone(), dir.join("i"));
        let b = Fake::default();
        let n = b.create("Wake fixture", "", &["taskq".into()]).unwrap();
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.focus = crate::data::focus(&dir).unwrap();
        s.pass().unwrap();
        assert_eq!(
            task(&b, n).state,
            if c.install.join("hold").exists() {
                State::Ready
            } else {
                State::Doing
            }
        );
        assert_eq!(
            e.borrow().len(),
            usize::from(!c.install.join("hold").exists())
        );
    }
}

// Chain: Resume during a pass -> wake -> fresh admission; a host hold still refuses the launch.
// Risk: a poke received during work becomes the sleep baseline, delaying Resume by another poll.
#[test]
fn resume_during_pass_wakes_the_next_admission_check() {
    crate::tests::with_test_capture(|_, _| {
        let dir = std::env::temp_dir().join(format!("qr-wake-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".taskq")).unwrap();
        let mut c = cfg("fixture");
        (c.root, c.install) = (dir.clone(), dir.join("i"));
        std::fs::create_dir_all(&c.install).unwrap();
        let b = Fake::default();
        let n = b.create("Wake fixture", "", &["taskq".into()]).unwrap();
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        std::fs::write(c.install.join("hold"), "owner-held fixture").unwrap();
        for held in [true, false] {
            std::fs::write(dir.join(".taskq/focus"), "all").unwrap();
            crate::cli::service_cmd::pause(&dir, false).unwrap();
            std::fs::remove_file(dir.join(".taskq/poke")).unwrap();
            let pokes = [dir.join(".taskq/poke"), c.install.join("poke")];
            let poke_start = crate::wake::poke_times(&pokes);
            s.focus = crate::data::focus(&dir).unwrap();
            s.pass().unwrap();
            assert_eq!(task(&b, n).state, State::Ready);
            crate::cli::service_cmd::resume(&dir).unwrap();
            let mut ticks = 0;
            crate::wake::sleep_until_poke(
                &pokes,
                poke_start,
                std::time::Instant::now() + std::time::Duration::from_secs(1),
                &mut || ticks += 1,
            );
            assert_eq!(
                ticks, 0,
                "Resume during the pass must skip the next poll wait"
            );
            s.focus = crate::data::focus(&dir).unwrap();
            s.pass().unwrap();
            if held {
                assert_eq!(task(&b, n).state, State::Ready);
                assert!(e.borrow().is_empty());
                assert_eq!(s.capacity_json()["reasons"][0]["code"], "hold");
                assert_eq!(
                    std::fs::read_to_string(c.install.join("hold")).unwrap(),
                    "owner-held fixture"
                );
                std::fs::remove_file(c.install.join("hold")).unwrap();
            } else {
                assert_eq!(task(&b, n).state, State::Doing);
                assert_eq!(e.borrow().len(), 1);
            }
        }
        let pokes = [dir.join(".taskq/poke"), c.install.join("poke")];
        let poke_start = crate::wake::poke_times(&pokes);
        crate::paths::touch(&pokes[1]);
        assert!(
            crate::wake::sleep_until_poke(
                &pokes,
                poke_start,
                std::time::Instant::now() + std::time::Duration::from_secs(1),
                &mut || panic!("host poke during a pass was lost")
            ),
            "a poke reports itself"
        );
        assert!(
            !crate::wake::sleep_until_poke(
                &pokes,
                crate::wake::poke_times(&pokes),
                std::time::Instant::now(),
                &mut || {}
            ),
            "a timeout is not a poke"
        );
        // A poke during sleep is observed after the current tick, without waiting another second.
        let poke_start = crate::wake::poke_times(&pokes);
        let mut ticks = 0;
        crate::wake::sleep_until_poke(
            &pokes,
            poke_start,
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            &mut || {
                ticks += 1;
                std::fs::remove_file(&pokes[1]).unwrap();
            },
        );
        assert_eq!(ticks, 1);
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    });
}

// Chain #1431 A2: board snapshot, children and pokes -> § 4 step 8 wait.
// Risk: an idle board polls at full rate, or a ready/live task or a local command waits 3×poll.
#[test]
fn idle_passes_wait_three_polls_until_work_or_a_poke() {
    let c = cfg("mac");
    let b = Fake::default();
    let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
    assert_eq!(
        crate::wake::poll_wait(60, true, false, false, false, None),
        60,
        "no board read yet is not idle"
    );
    assert_eq!(
        crate::wake::poll_wait(60, true, false, false, true, None),
        60,
        "no board read yet is not idle, focus off too"
    );
    s.pass().unwrap();
    let idle = s.snapshot.as_ref();
    assert_eq!(
        crate::wake::poll_wait(60, true, false, false, false, idle),
        180
    );
    assert_eq!(
        crate::wake::poll_wait(60, false, false, false, false, idle),
        60,
        "a failed pass keeps the normal interval"
    );
    assert_eq!(
        crate::wake::poll_wait(60, true, true, false, false, idle),
        60,
        "a running child keeps the normal interval"
    );
    assert_eq!(
        crate::wake::poll_wait(60, true, false, true, false, idle),
        60,
        "a poke restores the normal interval"
    );
    let n = b.create("Pending", "", &[TASK_LABEL.into()]).unwrap();
    s.pass().unwrap();
    let snap = s.snapshot.as_mut().unwrap();
    assert!(snap.tasks.iter().any(|r| r.n == n));
    for state in [State::Ready, State::Doing, State::Review, State::Ask] {
        snap.tasks.iter_mut().for_each(|r| r.task.state = state);
        assert_eq!(
            crate::wake::poll_wait(60, true, false, false, false, Some(&*snap)),
            if state == State::Ask { 180 } else { 60 },
            "{state:?} task"
        );
        // #2352: focus off admits nothing, so a board task elsewhere keeps no 60 s pass here
        assert_eq!(
            crate::wake::poll_wait(60, true, false, false, true, Some(&*snap)),
            180,
            "{state:?} task, focus off"
        );
        assert_eq!(
            crate::wake::poll_wait(60, false, false, false, true, Some(&*snap)),
            60,
            "{state:?} task, focus off, failed pass"
        );
        assert_eq!(
            crate::wake::poll_wait(60, true, true, false, true, Some(&*snap)),
            60,
            "{state:?} task, focus off, running child"
        );
        assert_eq!(
            crate::wake::poll_wait(60, true, false, true, true, Some(&*snap)),
            60,
            "{state:?} task, focus off, poke"
        );
    }
}

// Chain A2: project-thread unwind -> rebuilt board cache -> quarantine with live lease, beside healthy admission.
// Risk: a panic drops a child/cursor, stops the other project or collides on equal task/attempt numbers.
#[test]
fn host_project_panic_preserves_children_and_other_project_admission() {
    use std::sync::{Arc, Barrier, Mutex};
    let policy = crate::host::Policy::parse(
        json!({"limits": {"claude": 2}, "admission": {"max": 2}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let host = Arc::new(Mutex::new(crate::host::Host::new(Ok(policy.clone()))));
    let barrier = Arc::new(Barrier::new(2));
    let joins: Vec<_> = [true, false]
        .into_iter()
        .map(|panicking| {
            let (host, barrier, policy) = (host.clone(), barrier.clone(), policy.clone());
            std::thread::spawn(move || {
                let b = Fake::default();
                let n = b.create("Same number", "", &["taskq".into()]).unwrap();
                let mut c = cfg("invented");
                c.root = PathBuf::from(if panicking {
                    "/invented/panic"
                } else {
                    "/invented/healthy"
                });
                let exits: Exits = Default::default();
                let mut s = service(&b, &c, exits.clone());
                // Seed the panicking project's existing child without consuming a new host launch interval.
                if panicking {
                    s.pass().unwrap();
                    s.delivered.insert(s.kids[0].attempt, 99);
                }
                s.host = Some(host.clone());
                s.reload(Ok(policy.cap.clone()));
                s.sync_host();
                barrier.wait();
                if !panicking {
                    s.pass().unwrap();
                }
                assert_eq!(s.kids.len(), 1);
                let a = s.kids[0].attempt;
                let mut panics = crate::host::Panics::default();
                for now in [0, 1, 11] {
                    if panicking {
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            s.pass().unwrap();
                            panic!("invented pass panic");
                        }));
                        assert!(r.is_err());
                        panics.record(now, "invented pass panic");
                        s.rebuild();
                        s.reduced().unwrap();
                        assert_eq!(
                            s.delivered(a),
                            99,
                            "the inbox cursor survives every rebuild"
                        );
                    } else {
                        s.pass().unwrap();
                    }
                    assert_eq!(s.kids.len(), 1);
                    assert_eq!(*exits.borrow()[0].1.borrow(), None);
                }
                if panicking {
                    assert!(panics.quarantined());
                    assert_eq!(panics.reason.as_deref(), Some("quarantined: panic:other"));
                    let before = b.comments(n).unwrap().len();
                    b.advance(900);
                    s.mono = 900;
                    s.reduced().unwrap();
                    assert_eq!(
                        b.comments(n).unwrap().len(),
                        before,
                        "quarantine writes no task transitions"
                    );
                }
                let hb = b.issues(None).unwrap().into_iter().find(is_host).unwrap();
                assert!(
                    hb.body.lines().any(|l| l == format!("#{n} {a}")),
                    "child lease remains listed"
                );
                assert!(
                    b.now().unwrap() - hb.updated < c.ttl,
                    "quarantine refreshes the lease beyond ttl using board time"
                );
                (n, a, s.kids.len())
            })
        })
        .collect();
    let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(
        results[0], results[1],
        "equal issue and attempt numbers across projects stay distinct"
    );
    assert_eq!(
        host.lock()
            .unwrap()
            .sync(std::path::Path::new("unused"), vec![]),
        2
    );
}

// A2: scoped Uninstall drains one project while another retains work and admits its next job.
#[test]
fn project_uninstall_preserves_other_project_work_and_admission() {
    let base = std::env::temp_dir().join(format!(
        "taskq-project-uninstall-{}",
        crate::runtime::uuid()
    ));
    let policy = crate::host::Policy::parse(
        json!({"limits":{"claude":2},"load_gate":{"enabled":false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(
        policy.clone()
    ))));
    let (ba, bb) = (Fake::default(), Fake::default());
    let exits = Exits::default();
    let mut ca = cfg("invented");
    ca.root = base.join("a");
    ca.install = base.clone();
    let mut cb = ca.clone();
    cb.root = base.join("b");
    for root in [&ca.root, &cb.root] {
        std::fs::create_dir_all(crate::paths::dot(root)).unwrap();
    }
    let (mut sa, mut sb) = (
        service(&ba, &ca, exits.clone()),
        service(&bb, &cb, exits.clone()),
    );
    bb.create("Already running", "", &[TASK_LABEL.into()])
        .unwrap();
    sb.pass().unwrap();
    let attempt = sb.kids[0].attempt;
    for s in [&mut sa, &mut sb] {
        s.host = Some(host.clone());
        s.reload(Ok(policy.cap.clone()));
        s.sync_host();
    }
    host.lock().unwrap().draining.insert(ca.root.clone());
    let held = ba.create("Keep queued", "", &[TASK_LABEL.into()]).unwrap();
    sa.pass().unwrap();
    sa.sync_host();
    assert!(sa.kids.is_empty() && crate::service_run::drained(&sa, &host.lock().unwrap()));
    host.lock().unwrap().ready(&ca.root, false);
    let next = bb.create("Next job", "", &[TASK_LABEL.into()]).unwrap();
    sb.pass().unwrap();
    sb.sync_host();
    assert_eq!(
        sb.kids.len(),
        2,
        "unrelated admission stays open: {:?}",
        sb.waits
    );
    assert!(
        sb.kids.iter().any(|kid| kid.attempt == attempt),
        "the existing job keeps its attempt"
    );
    assert!(
        exits
            .borrow()
            .iter()
            .all(|(_, exit)| exit.borrow().is_none()),
        "no job was stopped"
    );
    assert!(
        entries(&ba, held)
            .unwrap()
            .iter()
            .all(|entry| !entry.body.starts_with("taskq: spawn"))
    );
    assert!(
        entries(&bb, next)
            .unwrap()
            .iter()
            .any(|entry| entry.body.starts_with("taskq: spawn"))
    );
    drop((sa, sb));
    std::fs::remove_dir_all(base).unwrap();
}

// Chain: #2255 restart pending -> each project's queue row -> one heartbeat `queue` line for every project. Case
// 2026-11-03: win hung on re-exec, ready work in two projects never spawned. Risk: the heartbeat hides the
// waiting queue, names another (private) project on this board, or a spawn leaves the wait running.
#[test]
fn heartbeat_queue_lists_every_project_waiting_on_a_pending_restart() {
    let base = std::env::temp_dir().join(format!("taskq-queue-{}", crate::runtime::uuid()));
    let policy = crate::host::Policy::parse(
        json!({"limits":{"claude":2},"load_gate":{"enabled":false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(
        policy.clone()
    ))));
    let (ba, bb) = (Fake::default(), Fake::default());
    let mut ca = cfg("invented");
    ca.root = base.join("a");
    ca.install = base.clone();
    let mut cb = ca.clone();
    cb.root = base.join("b");
    cb.repo = "o/private".into();
    for root in [&ca.root, &cb.root] {
        std::fs::create_dir_all(crate::paths::dot(root)).unwrap();
    }
    let (mut sa, mut sb) = (
        service(&ba, &ca, Default::default()),
        service(&bb, &cb, Default::default()),
    );
    for s in [&mut sa, &mut sb] {
        s.host = Some(host.clone());
        s.reload(Ok(policy.cap.clone()));
        s.sync_host();
    }
    host.lock().unwrap().restart = true;
    ba.create("Queued", "", &[TASK_LABEL.into()]).unwrap();
    bb.create("Queued too", "", &[TASK_LABEL.into()]).unwrap();
    sb.pass().unwrap();
    sa.pass().unwrap();
    assert!(
        sa.kids.is_empty() && sb.kids.is_empty(),
        "a pending restart admits nothing"
    );
    let queue = |s: &Service| {
        serde_json::from_str::<crate::host::Queues>(
            s.heartbeat_body()
                .lines()
                .find_map(|l| l.strip_prefix("queue "))
                .unwrap(),
        )
        .unwrap()
    };
    let q = queue(&sa);
    assert!(q.restart.is_some(), "{q:?}");
    let rows: Vec<_> = q
        .projects
        .iter()
        .map(|p| {
            (
                p.project.clone(),
                p.ready,
                p.spawn,
                p.waiting.is_some(),
                p.reason.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                Some(ca.repo.clone()),
                1,
                None,
                true,
                Some("host restart pending")
            ),
            (None, 1, None, true, Some("host restart pending"))
        ]
    );
    assert!(
        !sa.heartbeat_body().contains("o/private"),
        "another project stays anonymous on this board"
    );
    // #2290: this board's row names the task's refusal; the anonymous row never lists another project's tasks
    assert_eq!(
        (
            q.projects[0].tasks.keys().copied().collect::<Vec<_>>(),
            q.projects[1].tasks.is_empty()
        ),
        (vec![1], true),
        "{q:?}"
    );
    assert!(q.projects[0].tasks[&1].starts_with("hold: "), "{q:?}");
    host.lock().unwrap().restart = false;
    sa.pass().unwrap();
    let q = queue(&sa);
    assert_eq!(
        (
            sa.kids.len(),
            q.restart,
            q.projects[0].spawn.is_some(),
            q.projects[0].ready,
            q.projects[0].waiting
        ),
        (1, None, true, 0, None),
        "{q:?}"
    );
    assert!(
        q.projects[0].tasks.is_empty(),
        "an admitted task leaves the list: {q:?}"
    );
    drop((sa, sb));
    std::fs::remove_dir_all(base).unwrap();
}

// Chain: § 3 poisoned authority -> each project's retained handles -> verified slot rebuild.
// Risk: clearing poison early drops another project's occupancy and allows a launch above the host limit.
#[test]
fn host_poison_holds_until_every_project_reconciles_owned_handles() {
    let install = std::env::temp_dir().join(format!("taskq-host-poison-{}", std::process::id()));
    std::fs::create_dir_all(&install).unwrap();
    let raw =
        json!({"limits": {"claude": 2}, "admission": {"max": 2}, "load_gate": {"enabled": false}});
    std::fs::write(install.join("host.json"), raw.to_string()).unwrap();
    let p = crate::host::Policy::parse(raw, "invented").unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
    let (a, b) = (Fake::default(), Fake::default());
    let mut ca = cfg("invented");
    ca.root = install.join("a");
    ca.install = install.clone();
    let mut cb = ca.clone();
    cb.root = install.join("b");
    let mut sa = service(&a, &ca, Default::default());
    let mut sb = service(&b, &cb, Default::default());
    for s in [&mut sa, &mut sb] {
        s.host = Some(host.clone());
        s.reload(Ok(p.cap.clone()));
        s.kids.push(Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution: 7,
            task: 1,
            attempt: 7,
            rt: "claude".into(),
            proc: Box::new(FakeProc(Default::default())),
        });
        host.lock().unwrap().busy.insert(s.cfg.root.clone(), true);
        s.sync_host();
    }
    let poisoned = host.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("invented admission panic");
        })
        .join()
        .is_err()
    );
    sa.sync_host();
    assert!(host.is_poisoned());
    assert!(!sa.reserve(2, 8, "claude"));
    sb.sync_host();
    assert!(!host.is_poisoned());
    assert_eq!(
        host.lock()
            .unwrap()
            .sync(std::path::Path::new("unused"), vec![]),
        2
    );
    assert_eq!((sa.kids.len(), sb.kids.len()), (1, 1));
    std::fs::remove_dir_all(install).unwrap();
}

// Chain A2: unavailable project leaves round-robin -> multiple healthy grants; failed reduced loop -> safe update.
// Risk: stale readiness starves later work, or a stopped project can never satisfy the host handoff barrier.
#[test]
fn host_quarantine_does_not_starve_and_stopped_handles_can_prepare_restart() {
    let root = std::env::temp_dir().join(format!("taskq-host-stopped-{}", std::process::id()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let p = crate::host::Policy::parse(
        json!({"limits": {"claude": 2}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
    let b = Fake::default();
    let mut c = cfg("invented");
    c.root = root.clone();
    c.install = root.clone();
    let mut s = service(&b, &c, Default::default());
    s.host = Some(host.clone());
    let healthy = root.join("healthy");
    {
        let mut h = host.lock().unwrap();
        h.ready(&root, true);
        h.ready(&healthy, true);
    }
    s.rebuild();
    s.reduced().unwrap();
    for n in 1..=3 {
        let mut h = host.lock().unwrap();
        h.reserve(&healthy, n, n, "claude", &p.cap, true, (n - 1) * 30)
            .unwrap();
        h.sync(&healthy, vec![]);
    }
    let n = b.create("Owned", "", &[TASK_LABEL.into()]).unwrap();
    // #2297: a codex-app turn is the one child a re-exec still waits for
    let a = b
        .comment(n, "taskq: spawn worker codex-app:fixture invented")
        .unwrap();
    let exit = Rc::new(RefCell::new(None));
    s.kids.push(Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: a,
        task: n,
        attempt: a,
        rt: "codex-app".into(),
        proc: Box::new(FakeProc(exit.clone())),
    });
    s.pass().unwrap();
    s.rebuild();
    {
        let mut h = host.lock().unwrap();
        h.busy.insert(root.clone(), true);
        h.restart = true;
    }
    // This is the monitor entered after a reduced-loop panic: no board pass and no lease renewal.
    assert!(!crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap());
    assert!(!host.lock().unwrap().prepared.contains(&root));
    assert_eq!(s.kids.len(), 1);
    *exit.borrow_mut() = Some(0);
    assert!(!crate::service_run::stopped_handoff(&mut s, &host, &c).unwrap());
    let h = host.lock().unwrap();
    assert!(
        h.busy.keys().all(|p| h.prepared.contains(p)),
        "coordinator can restart after verified exit"
    );
    assert!(
        core::executions(n, &entries(&b, n).unwrap())
            .iter()
            .all(|x| x.release.is_some()),
        "only confirmed release permits handoff"
    );
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

// #1402 A1: restart pending, a live codex-app turn defers the handover (#2297: the only child kind); once it exits and
// is released, the next pass reports prepared. Risk: a released worker keeps the host on the old image.
#[test]
fn released_kid_lets_the_next_pass_prepare_the_restart() {
    let root = std::env::temp_dir().join(format!("taskq-host-released-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let p = crate::host::Policy::parse(
        json!({"limits": {"claude": 2}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p))));
    let b = Fake::default();
    let mut c = cfg("invented");
    c.root = root.clone();
    c.install = root.clone();
    let mut s = service(&b, &c, Default::default());
    s.host = Some(host.clone());
    let n = b.create("Owned", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex-app:fixture invented")
        .unwrap();
    let exit = Rc::new(RefCell::new(None));
    s.kids.push(Kid {
        unknown: Some("native observation deferred; pass budget exhausted".into()),
        stopping: false,
        session_url: None,
        execution: a,
        task: n,
        attempt: a,
        rt: "codex-app".into(),
        proc: Box::new(FakeProc(exit.clone())),
    });
    {
        let mut h = host.lock().unwrap();
        h.busy.insert(root.clone(), true);
        h.restart = true;
    }
    let project_pass = |s: &mut Service| {
        assert!(host.lock().unwrap().begin_pass(&root));
        s.pass().unwrap();
        s.sync_host();
        let mut h = host.lock().unwrap();
        let drained = crate::service_run::drained(s, &h);
        h.busy.insert(root.clone(), !drained);
        crate::service_run::report_handoff(s, &mut h, &root, drained).unwrap();
        h.handoff_ready()
    };
    assert!(!project_pass(&mut s), "the live worker defers the handover");
    assert_eq!(
        crate::service_run::lifecycle_waits(&s, false),
        vec![format!("#{n}")]
    );
    *exit.borrow_mut() = Some(0);
    assert!(
        project_pass(&mut s),
        "the released worker no longer holds the restart"
    );
    assert!(s.kids.is_empty() && host.lock().unwrap().prepared.contains(&root));
    assert!(
        core::executions(n, &entries(&b, n).unwrap())
            .iter()
            .all(|x| x.release.is_some())
    );
    std::fs::remove_dir_all(root).unwrap();
}

// #1766 A1-A2: a queued Stop with a live worker in project A froze idle project B: the barrier (#1402) skipped
// B's loop, so its status.json kept stale counters for the whole drain. Retained protection: the barrier still holds
// B out of ordinary passes and keeps its prepared report, A keeps monitoring its worker, and neither claims.
#[test]
fn drain_with_live_worker_keeps_idle_project_status_fresh_and_claims_nothing() {
    let base = std::env::temp_dir().join(format!("taskq-drain-observe-{}", crate::runtime::uuid()));
    let p = crate::host::Policy::parse(
        json!({"limits": {"claude": 2}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
    let (ba, bb) = (Fake::default(), Fake::default());
    let mut ca = cfg("invented");
    ca.root = base.join("a");
    ca.install = base.clone();
    let mut cb = ca.clone();
    cb.root = base.join("b");
    for root in [&ca.root, &cb.root] {
        std::fs::create_dir_all(crate::paths::dot(root)).unwrap();
    }
    let exits = Exits::default();
    let (mut sa, mut sb) = (
        service(&ba, &ca, exits.clone()),
        service(&bb, &cb, exits.clone()),
    );
    for s in [&mut sa, &mut sb] {
        s.host = Some(host.clone());
        s.reload(Ok(p.cap.clone()));
    }
    let live = ba.create("Live", "", &[TASK_LABEL.into()]).unwrap();
    let a = ba
        .comment(live, "taskq: spawn worker claude:fixture invented")
        .unwrap();
    sa.kids.push(Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: a,
        task: live,
        attempt: a,
        rt: "claude".into(),
        proc: Box::new(FakeProc(Default::default())),
    });
    let ready = [
        (
            &ba,
            ba.create("Ready A", "## Goal\ng", &[TASK_LABEL.into()])
                .unwrap(),
        ),
        (
            &bb,
            bb.create("Ready B", "## Goal\ng", &[TASK_LABEL.into()])
                .unwrap(),
        ),
    ];
    {
        let mut h = host.lock().unwrap();
        h.restart = true;
        h.stopping = true;
        for root in [&ca.root, &cb.root] {
            h.busy.insert(root.clone(), true);
        }
    }
    // One service-loop pass: the barrier's observe pass leaves the handover report untouched.
    let loop_pass = |s: &mut Service| -> bool {
        let observe = crate::service_run::observes(Some(&host), &s.cfg.root).unwrap();
        if observe {
            s.observe().unwrap();
            return true;
        }
        s.pass().unwrap();
        s.sync_host();
        let mut h = host.lock().unwrap();
        h.busy.insert(s.cfg.root.clone(), !s.idle());
        crate::service_run::report_handoff(s, &mut h, &s.cfg.root, s.idle()).unwrap();
        false
    };
    assert!(
        !loop_pass(&mut sa) && !loop_pass(&mut sb),
        "the first pass of each project reports its handover"
    );
    assert!(
        host.lock().unwrap().prepared.contains(&cb.root) && !host.lock().unwrap().handoff_ready(),
        "A's live worker holds the Stop"
    );
    for round in 0..3 {
        let arrived = bb
            .create(&format!("Arrived {round}"), "", &[TASK_LABEL.into()])
            .unwrap();
        sb.tasks_read = None;
        assert!(
            loop_pass(&mut sb),
            "the barrier keeps idle B out of ordinary passes"
        );
        assert!(
            sb.tasks_read.is_some()
                && sb
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .tasks
                    .iter()
                    .any(|r| r.n == arrived),
            "B's status reads the board it observes now"
        );
        assert!(!loop_pass(&mut sa), "A keeps monitoring its live worker");
        assert_eq!(sa.kids.len(), 1);
        assert!(
            host.lock().unwrap().prepared.contains(&cb.root),
            "an observe pass keeps B's prepared report"
        );
    }
    assert!(
        exits.borrow().is_empty(),
        "the drain admits nothing in either project"
    );
    for (b, n) in ready {
        assert!(
            entries(b, n)
                .unwrap()
                .iter()
                .all(|e| !e.body.starts_with("taskq: spawn")),
            "#{n} stays unclaimed"
        );
    }
    {
        let mut h = host.lock().unwrap();
        h.restart = false;
        h.stopping = false;
    }
    for _ in 0..2 {
        assert!(
            !loop_pass(&mut sa) && !loop_pass(&mut sb),
            "a cancelled Stop lifts the barrier"
        );
    }
    assert!(
        !exits.borrow().is_empty(),
        "the fixture claims once admission reopens, so the drain's refusal was real: {:?} {:?}",
        sa.waits,
        sb.waits
    );
    drop((sa, sb));
    std::fs::remove_dir_all(base).unwrap();
}

// #1601 A4: a pending restart refused the resume of an answered task, and the handover waited on that refused
// attempt ('#N unresolved ownership') forever. Risk kept: the resume is not started under the restart; the next
// image still finds it on the board with the inbox undelivered.
#[test]
fn refused_resume_lets_the_restart_hand_over() {
    let root = std::env::temp_dir().join(format!("taskq-host-refused-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let p = crate::host::Policy::parse(
        json!({"limits": {"codex": 2}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p))));
    let b = Fake::default();
    let mut c = cfg("invented");
    c.root = root.clone();
    c.install = root.clone();
    let n = b.create("Answered", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session invented")
        .unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    b.comment(n, "taskq: steer\ncontinue").unwrap();
    let mut s = service(&b, &c, Default::default());
    s.host = Some(host.clone());
    {
        let mut h = host.lock().unwrap();
        h.busy.insert(root.clone(), true);
        h.restart = true;
    }
    assert!(
        handover_pass(&mut s, &host, &root),
        "one pass reaches the re-exec: {:?}",
        crate::service_run::lifecycle_waits(&s, false)
    );
    assert!(
        s.kids.is_empty() && s.pending.contains(&(n, a)),
        "the restart refuses the resume"
    );
    let t = task(&b, n);
    assert_eq!(
        (t.state, t.attempt, s.delivered(a)),
        (State::Doing, Some(a), 0),
        "the inbox waits for the next image"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// #1769 A1: a drain Stop refused the resume of an answered task (its worker had asked and exited), then waited on
// that refused attempt ('#N unresolved ownership') forever. Risk kept: nothing starts under the Stop; the task
// stays Doing with its inbox undelivered, and the next generation resumes it.
#[test]
fn stop_drain_passes_a_released_attempt_to_the_next_start() {
    let root = std::env::temp_dir().join(format!("taskq-host-stop-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let p = crate::host::Policy::parse(
        json!({"limits": {"codex": 2}, "load_gate": {"enabled": false}}),
        "invented",
    )
    .unwrap();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
    let b = Fake::default();
    let mut c = cfg("invented");
    c.root = root.clone();
    c.install = root.clone();
    let n = b.create("Answered", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session invented")
        .unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    b.comment(n, "taskq: steer\ncontinue").unwrap();
    let mut s = service(&b, &c, Default::default());
    s.host = Some(host.clone());
    {
        let mut h = host.lock().unwrap();
        h.busy.insert(root.clone(), true);
        h.restart = true;
        h.stopping = true;
    }
    assert!(
        handover_pass(&mut s, &host, &root),
        "the drain completes: {:?}",
        crate::service_run::lifecycle_waits(&s, true)
    );
    assert!(
        s.kids.is_empty() && s.pending.contains(&(n, a)),
        "the Stop refuses the resume"
    );
    assert_eq!(
        (task(&b, n).state, s.delivered(a)),
        (State::Doing, 0),
        "the inbox waits for the next start"
    );
    drop(s);
    let mut next = service(&b, &c, Default::default());
    next.host = Some(std::sync::Arc::new(std::sync::Mutex::new(
        crate::host::Host::new(Ok(p)),
    )));
    next.pass().unwrap();
    assert_eq!(
        next.kids.iter().map(|k| k.task).collect::<Vec<_>>(),
        vec![n],
        "the next generation resumes it"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// #2285: replay #2252's closed history with the old Mac execution still observed.
// Retain the fence until exact terminal releases are read; closure alone is never exit.
#[test]
fn closed_2252_releases_reach_reexec_in_one_pass() {
    let root = std::env::temp_dir().join(format!("taskq-closed-reexec-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let b = Fake::default();
    let n = b
        .create("Closed history", "", &[TASK_LABEL.into()])
        .unwrap();
    let history: Vec<core::Entry> = serde_json::from_str(
        include_str!("../tests/fixtures/reexec-2252.json").trim_start_matches('\u{feff}'),
    )
    .unwrap();
    let mut c = cfg("mac");
    c.root = root.clone();
    c.install = root.clone();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
        "fixture holds launches".into(),
    ))));
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|_| panic!("restart must not launch")),
    );
    s.host = Some(host.clone());
    for e in &history {
        b.state.borrow_mut().next_id = e.id - 1;
        b.comment(n, &e.body).unwrap();
        if e.id == 6595390316 {
            s.observe_ownership(
                &views(&b, &mut Default::default(), &mut Default::default(), c.ttl)
                    .unwrap()
                    .0,
            )
            .unwrap();
            assert!(s.observations.contains_key(&(n, e.id)));
        }
    }
    b.close(n).unwrap();
    assert_eq!(task(&b, n).state, State::Closed);
    let key = (n, 6595390316);
    let original = s.observations[&key].clone();
    for reason in ["attempt mismatch", "host mismatch", "runtime mismatch"] {
        let mut mismatch = original.clone();
        match reason {
            "attempt mismatch" => mismatch.attempt += 1,
            "host mismatch" => mismatch.host = "other".into(),
            _ => mismatch.runtime = "other".into(),
        }
        s.observations.insert(key, mismatch);
        s.observe_ownership(&[]).unwrap();
        let waits = s.handoff_waits(true, false); // Unix handover; Windows exits without these observations.
        assert!(
            waits.iter().any(|w| w.contains(reason)),
            "an overdue wait must explain the unmatched release: {waits:?}"
        );
        let line = crate::cli::service_cmd::restart_line(
            &json!({"restart_waits": waits, "restart_overdue": true}),
        )
        .unwrap();
        assert!(line.contains("pending over 600 s") && line.contains(reason));
        assert!(
            s.observations.contains_key(&key),
            "mismatch must retain ownership"
        );
    }
    s.observations.insert(key, original);
    {
        let mut h = host.lock().unwrap();
        h.busy.insert(root.clone(), true);
        h.restart = true;
    }
    assert!(
        handover_pass(&mut s, &host, &root),
        "released history must reach re-exec: {:?}",
        s.handoff_waits(true, false)
    );
    assert!(s.observations.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

// #2285 Mac repro: #2252 closed while this service still held the handle of its resumed execution 6595390316,
// which Claude ran as a copied job. Stop needs the input in the saved conversation, so it failed every pass and
// the re-exec waited for an owner kickstart although the board held that execution's exact Exit release.
// #2297: the live handle no longer holds the re-exec; it is never killed, retired or released twice.
#[test]
fn closed_2252_board_exit_retires_the_copied_job_handle() {
    struct Copied {
        live: Rc<std::cell::Cell<bool>>,
        released: bool,
    }
    impl Proc for Copied {
        fn released(&mut self) {
            self.released = true;
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            if !self.released {
                return Err(
                    "Claude original input found in copied job; inspect native state".into(),
                );
            }
            Ok((!self.live.get()).then_some(0))
        }
        fn kill(&mut self) -> Result<(), String> {
            Err("Claude original input found in copied job; inspect native state".into())
        }
        // An observed live process, like FakeProc: pid 0 means childless on Windows (#2228),
        // which would attempt to save this unsupported fake handle instead of waiting for exit.
        fn id(&self) -> u32 {
            1
        }
    }
    let root = std::env::temp_dir().join(format!("taskq-closed-exit-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let b = Fake::default();
    let n = b
        .create("Closed history", "", &[TASK_LABEL.into()])
        .unwrap();
    let history: Vec<core::Entry> = serde_json::from_str(
        include_str!("../tests/fixtures/reexec-2252.json").trim_start_matches('\u{feff}'),
    )
    .unwrap();
    for e in &history {
        b.state.borrow_mut().next_id = e.id - 1;
        b.comment(n, &e.body).unwrap();
    }
    b.close(n).unwrap();
    let mut c = cfg("mac");
    c.root = root.clone();
    c.install = root.clone();
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
        "fixture holds launches".into(),
    ))));
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|_| panic!("restart must not launch")),
    );
    s.host = Some(host.clone());
    let live = Rc::new(std::cell::Cell::new(true));
    s.kids.push(Kid {
        task: n,
        attempt: 6595327803,
        execution: 6595390316,
        rt: "claude".into(),
        proc: Box::new(Copied {
            live: live.clone(),
            released: false,
        }),
        unknown: None,
        stopping: false,
        session_url: None,
    });
    {
        let mut h = host.lock().unwrap();
        h.busy.insert(root.clone(), true);
        h.restart = true;
    }
    let releases = || {
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with("taskq: released 6595390316"))
            .count()
    };
    let before = releases();
    // A live process contradicting the board Exit is never killed or retired, and runs on past the re-exec.
    assert!(
        handover_pass(&mut s, &host, &root),
        "{:?}",
        crate::service_run::lifecycle_waits(&s, false)
    );
    assert_eq!(s.kids.len(), 1);
    assert_eq!(
        releases(),
        before,
        "the existing release stands; no second release is written"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// One project pass under the host lock protocol; true when the host may re-exec.
fn handover_pass(s: &mut Service, host: &crate::host::Shared, root: &PathBuf) -> bool {
    assert!(host.lock().unwrap().begin_pass(root));
    s.pass().unwrap();
    s.sync_host();
    let mut h = host.lock().unwrap();
    let drained = crate::service_run::drained(s, &h);
    h.busy.insert(root.clone(), !drained);
    crate::service_run::report_handoff(s, &mut h, root, drained).unwrap();
    h.handoff_ready()
}

// #1601 class guard: a pending restart refuses every reservation (`Host::reserve`), so no handover wait may need
// one. Each attempt kind the refusal leaves childless is set up, the restart is requested, and one pass must
// reach the re-exec. Kinds: a ready claim, an answered resume, a held claim, a held resume, a held reservation.
// Live children, ownership reads and release writes resolve without a reservation and keep their own tests.
#[test]
fn every_handover_wait_resolves_without_a_reservation() {
    for kind in ["claim", "resume", "held claim", "held resume", "reserved"] {
        let root =
            std::env::temp_dir().join(format!("taskq-host-class-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        let p = crate::host::Policy::parse(
            json!({"limits": {"codex": 2}, "load_gate": {"enabled": false}}),
            "invented",
        )
        .unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p))));
        let b = Fake::default();
        let mut c = cfg("invented");
        c.root = root.clone();
        c.install = root.clone();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        if kind.ends_with("resume") {
            let a = b
                .comment(n, "taskq: spawn worker codex:session invented")
                .unwrap();
            accept_execution(&b, n, a, a, None).unwrap();
            release_execution(&b, n, a, a, "exit").unwrap();
            b.comment(n, "taskq: steer\ncontinue").unwrap();
        }
        let mut s = Service::new(
            &b,
            &c,
            "test",
            Box::new(|_| {
                Err(crate::runtime::StartError::admission(
                    "native inventory pending",
                ))
            }),
        );
        s.focus = vec!["all".into()];
        s.host = Some(host.clone());
        host.lock().unwrap().busy.insert(root.clone(), true);
        if kind.starts_with("held") {
            handover_pass(&mut s, &host, &root);
            assert_eq!(s.held.len(), 1, "{kind}: setup holds the launch");
        }
        if kind == "reserved" {
            s.pass().unwrap();
            s.reserved.push((n, 99, "codex".into()));
        }
        host.lock().unwrap().restart = true;
        assert!(
            handover_pass(&mut s, &host, &root),
            "{kind}: one pass reaches the re-exec: {:?}",
            crate::service_run::lifecycle_waits(&s, false)
        );
        assert!(
            s.kids.is_empty() && s.held.is_empty(),
            "{kind}: nothing starts under the restart"
        );
        assert!(
            core::executions(n, &entries(&b, n).unwrap())
                .iter()
                .all(|x| x.release.is_some()),
            "{kind}: no execution left open"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

// Chain: cross-host recovery. Risk: inbox age or an unlisted lease is mistaken for exit.
#[test]
fn cross_host_recovery_needs_exit_regardless_of_inbox_grace() {
    for (entry, trusted, edited, pending_input) in [
        ("taskq: answer 1", true, false, true),
        ("taskq: steer\ncontinue", true, false, true),
        ("taskq: steer attempt 999999\ncontinue", true, false, false),
        ("taskq: steer\ncontinue", false, false, false),
        ("taskq: steer\ncontinue", true, true, false),
        ("unrelated comment", true, false, false),
    ] {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let a = b
            .comment(
                n,
                &core::entry("spawn", &["worker", "claude:session", "win"], None, ""),
            )
            .unwrap();
        if entry.starts_with("taskq: answer") {
            b.comment(
                n,
                &core::entry("ask", &[], Some(a), "Which?\n1. yes\n2. no"),
            )
            .unwrap();
        }
        b.advance(400);
        b.raw(n, entry, trusted, edited);
        let hb = b
            .create("taskq host win", "version 4.0.0", &[HOST_LABEL.into()])
            .unwrap();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.focus.clear();
        s.pass().unwrap();
        assert_eq!(
            task(&b, n).attempt,
            Some(a),
            "{entry}, trusted={trusted}, edited={edited}"
        );
        b.advance(301);
        b.edit(hb, "version 4.0.0\nat fresh").unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a));
        release_execution(&b, n, a, a, "exit").unwrap();
        s.pass().unwrap();
        assert_eq!(
            task(&b, n).attempt,
            pending_input.then_some(a),
            "confirmed exit permits recovery but cannot discard pending input"
        );
    }
}

// Chain: § 3 answered attempt across hosts. Risk: another host steals an answered attempt.
#[test]
fn answered_attempt_survives_other_host_until_owner_resumes() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let (win, mac) = (cfg("win"), cfg("mac"));
    let exits: Exits = Default::default();
    let mut owner = service(&b, &win, exits.clone());
    let mut other = service(&b, &mac, Default::default());
    owner.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    b.comment(
        n,
        &core::entry("ask", &[], Some(a), "Which?\n1. yes\n2. no"),
    )
    .unwrap();
    *exits.borrow()[0].1.borrow_mut() = Some(0);
    b.advance(400);
    owner.pass().unwrap();
    b.comment(n, &core::entry("answer", &["1"], None, "yes"))
        .unwrap();
    other.pass().unwrap();
    assert_eq!(task(&b, n).attempt, Some(a));
    owner.pass().unwrap();
    assert_eq!((task(&b, n).attempt, exits.borrow().len()), (Some(a), 2));
    b.advance(301);
    other.pass().unwrap();
    assert_eq!(
        task(&b, n).attempt,
        Some(a),
        "the continuation remains owned after heartbeat expiry"
    );
    assert_eq!(
        core::executions(n, &entries(&b, n).unwrap())
            .iter()
            .filter(|x| x.release.is_none())
            .count(),
        1
    );
}

// Chain: § 4 step 1 turn end, A1/A2 of #1034. Risk: a worker whose `taskq result` was rate-limited finishes its
// turn, its process lingers, and the task shows `doing` for hours; or the `gone` hides the worker's last message.
#[test]
fn finished_turn_without_result_is_gone_with_last_message() {
    for paused in [false, true] {
        turn_end(paused);
    }
}

// Chain: service launch -> failed native checkpoint -> repair -> observation -> terminal receipt.
// Risk: persistence failure loses a live handle or resubmits the task instead of saving its identity.
#[cfg(unix)]
#[test]
fn native_checkpoint_failure_retains_execution_and_retries_only_storage() {
    let root = std::env::temp_dir().join(format!("taskq-checkpoint-{}", crate::runtime::uuid()));
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("native"), "blocked directory").unwrap();
    let b = Fake::default();
    let n = b
        .create("T", "", &[TASK_LABEL.into(), "run-codex".into()])
        .unwrap();
    let c = Config {
        root: root.clone(),
        install: install.clone(),
        limits: BTreeMap::from([("codex".into(), 1)]),
        ..cfg("mac")
    };
    let count = Rc::new(RefCell::new(0));
    let launched = count.clone();
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|l| {
            *launched.borrow_mut() += 1;
            let log = crate::runtime::log_path(&root, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "").unwrap();
            let child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let native = coreq::process::identity(child.id()).unwrap();
            Ok(Box::new(crate::runtime::CliTurn {
                proc: Box::new(child),
                log,
                pos: 0,
                start: 0,
                stopping: false,
                accepted: false,
                read_error: None,
                session: None,
                native,
                recovery: None,
                rt: "codex".into(),
            }))
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    assert_eq!(s.kids.len(), 1);
    s.reap_owned();
    assert!(
        s.kids
            .iter()
            .find(|k| k.attempt == a)
            .unwrap()
            .unknown
            .as_deref()
            .unwrap()
            .contains("checkpoint failed")
    );
    let pid = s.kids[0].proc.id();
    assert!(coreq::process::identity(pid).unwrap().is_some());
    std::fs::remove_file(install.join("native")).unwrap();
    s.reap_owned();
    let rows = crate::service_run::kids_file(&install, &root).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task,
            rows[0].attempt,
            rows[0].execution,
            rows[0].process.pid
        ),
        (n, a, a, pid)
    );
    assert_eq!(*count.borrow(), 1, "retry storage, never work");
    assert!(!s.kids.iter().any(|k| k.attempt == a && k.unknown.is_some()));
    let held = std::mem::take(&mut s.kids);
    let discarded = crate::service_run::save_kids(&s, &root);
    s.kids = held;
    discarded.unwrap();
    assert_eq!(
        crate::service_run::kids_file(&install, &root).unwrap(),
        rows,
        "a missing handle cannot erase recovery evidence"
    );
    // A second failed checkpoint must not prevent emergency cancellation.
    let l = Launch {
        files: String::new(),
        execution: None,
        task: n,
        title: String::new(),
        attempt: a,
        role: "worker".into(),
        rt: "codex".into(),
        session: "-".into(),
        url: String::new(),
        resume: None,
    };
    let path = crate::paths::kids_path(&install, &root).unwrap();
    std::fs::write(&path, "corrupt").unwrap();
    assert!(s.kids[0].proc.checkpoint(&c, &l, a).is_err());
    s.kids[0].proc.kill().unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !s.kids.is_empty() {
        s.reap_owned();
        assert!(
            std::time::Instant::now() < until,
            "stop must remain available during storage failure"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(*count.borrow(), 1);
    assert_eq!(
        core::executions(n, &crate::board::entries(&b, n).unwrap())[0].release,
        Some(core::Release::Exit)
    );
    let events = crate::telemetry::worker_events(&install);
    let release = events
        .iter()
        .find(|e| e.task == n && e.attempt == a && e.event == "slot-release")
        .unwrap();
    assert_eq!(
        release.pid, pid,
        "R12: reduced reap retains the exited executor PID"
    );

    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "corrupt",
        "failed recovery evidence is preserved"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

fn turn_end(paused: bool) {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("taskq-turn-end-{paused}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let b = Fake::default();
    let n = b
        .create("T", "", &[TASK_LABEL.into(), "run-codex".into()])
        .unwrap();
    let c = Config {
        root: root.clone(),
        limits: BTreeMap::from([("codex".into(), 1)]),
        ..cfg("win")
    };
    let flags: Exits = Default::default();
    let (seen, r) = (flags.clone(), root.clone());
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n").unwrap();
            Ok(Box::new(crate::runtime::CliTurn {
                proc: Box::new(FakeProc(flag)),
                log,
                pos: 0,
                start: 0,
                stopping: false,
                accepted: false,
                read_error: None,
                session: None,
                native: None,
                recovery: None,
                rt: "codex".into(),
            }) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, n).attempt, Some(a), "a running turn stays doing");

    // the worker's result is rejected by the board rate limit, then its turn ends and its process lingers
    b.state.borrow_mut().fail_comments = 1;
    assert!(
        b.comment(
            n,
            &core::entry(
                "result",
                &["d3e3479077ac2df8db8071b5bdf12cdacaf10ab4"],
                Some(a),
                "Changed: x"
            )
        )
        .is_err()
    );
    let log = crate::runtime::attempt_log(&root, "worker", n, a);
    let mut f = std::fs::File::options().append(true).open(&log).unwrap();
    let text = format!(
        "Task handoff is incomplete: GitHub rate limits rejected `taskq result`.\n\nSee [log]({}/.taskq/x.log).",
        root.display()
    );
    writeln!(
        f,
        "{}",
        json!({"type": "item.completed", "item": {"type": "agent_message", "text": text}})
    )
    .unwrap();
    writeln!(f, "{{\"type\":\"turn.completed\"}}").unwrap();
    if paused {
        // A failed release keeps the exited handle and retries only its evidence write.
        b.state.borrow_mut().fail_comments = 1;
        s.pass().unwrap();
        assert_eq!(s.kids.len(), 1);
        assert!(s.kids.iter().any(|k| k.attempt == a && k.unknown.is_some()));
    }
    s.pass().unwrap();
    assert_eq!(
        *flags.borrow()[0].1.borrow(),
        Some(-9),
        "the lingering process tree is stopped"
    );
    let t = task(&b, n);
    let gone = b
        .comments(n)
        .unwrap()
        .into_iter()
        .rfind(|c| c.body.starts_with("taskq: gone"))
        .unwrap()
        .body;
    assert_eq!(
        (t.state, t.attempt, t.retries),
        (State::Ready, None, 1),
        "paused={paused}"
    );
    let reason = "exit -9";
    assert!(gone.ends_with(&format!("\n{reason}\nLast message: Task handoff is incomplete: GitHub rate limits rejected `taskq result`. See [log](.taskq/x.log).")), "{gone}");
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: incident worker start. Risk: missing runtime or login reported as a fault.
#[test]
fn incident_start_requires_installed_logged_in_runtime_and_non_environment_failure() {
    use std::io::{Error, ErrorKind};
    for (path, login, kind, reports) in [
        (Some("fake"), "logged in", ErrorKind::Other, true),
        (None, "not installed", ErrorKind::Other, false),
        (Some("fake"), "not logged in", ErrorKind::Other, false),
        (Some("fake"), "unknown", ErrorKind::Other, false),
        (Some("fake"), "logged in", ErrorKind::OutOfMemory, false),
        (
            Some("fake"),
            "logged in",
            ErrorKind::PermissionDenied,
            false,
        ),
    ] {
        crate::incident::tests::take_detected();
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
        let c = cfg("local");
        let mut s = Service::new(
            &b,
            &c,
            "4.0.0",
            Box::new(move |_| {
                Err(crate::runtime::StartError::io(
                    Error::new(kind, "private failure text"),
                    None,
                ))
            }),
        );
        s.agent_status = json!([{"runtime": "claude", "path": path, "login": login}]);
        let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300)
            .unwrap()
            .0
            .remove(0);
        s.claim(&v, "worker", "claude", "").unwrap();
        assert_eq!(task(&b, n).state, State::Ready);
        assert!(
            b.comments(n)
                .unwrap()
                .last()
                .unwrap()
                .body
                .contains("start failed: private failure text")
        );
        assert_eq!(
            crate::incident::tests::take_detected(),
            if reports {
                vec!["dispatch.start"]
            } else {
                vec![]
            }
        );
    }
}

// Chain: § 8 rule delivery cap -> § 2 ask, #1946 (pilotq#170). Risk: a deterministic cap refusal burns two gone
// retries before the owner hears of it, or the answer resumes a session that never started.
#[test]
fn a_rules_cap_refusal_asks_once_and_its_answer_starts_a_fresh_attempt() {
    for role in ["worker", "reviewer"] {
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
        let result = "a".repeat(40);
        if role == "reviewer" {
            let w = b.comment(n, "taskq: spawn worker claude:w other").unwrap();
            b.comment(
                n,
                &core::entry("result", &[&result], Some(w), "Changed: x\nOpen: none"),
            )
            .unwrap();
            release_execution(&b, n, w, w, "exit").unwrap();
        }
        let c = cfg("local");
        let starts = Rc::new(RefCell::new(0));
        let counted = starts.clone();
        let start_root = c.root.clone();
        let mut s = Service::new(
            &b,
            &c,
            "4.0.0",
            Box::new(move |_| {
                *counted.borrow_mut() += 1;
                Err(crate::runtime::over_cap(&start_root, 128 * 1024 + 5))
            }),
        );
        let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300)
            .unwrap()
            .0
            .remove(0);
        s.claim(&v, role, "claude", "").unwrap();
        let t = task(&b, n);
        let a = t.attempt.unwrap();
        assert_eq!(
            (t.state, t.retries, *starts.borrow()),
            (State::Ask, 0, 1),
            "{role}: one start, no gone retry"
        );
        let card = t.card.unwrap();
        assert!(card.lines().nth(1).unwrap().starts_with("Problem: start refused: rules bundle 131077 bytes, 5 over the 131072-byte cap; trim taskq.md below the cap"), "{card}");
        assert!(card.contains("1. retry after trimming - + a fresh attempt starts; - it is refused again until the rules fit the cap ★") && card.contains("2. later:"), "{card}");
        assert!(
            !b.comments(n)
                .unwrap()
                .iter()
                .any(|c| c.body.starts_with("taskq: gone")),
            "{role}"
        );
        b.comment(n, "taskq: answer 1").unwrap();
        assert_eq!(
            task(&b, n).attempt,
            Some(a),
            "{role}: the answer returns to the never-started attempt"
        );
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(
            *starts.borrow(),
            1,
            "{role}: no resume of a session that never started"
        );
        if role == "worker" {
            assert_eq!(
                (t.state, t.attempt),
                (State::Ready, None),
                "the reply starts a fresh attempt"
            );
        } else {
            assert_eq!(
                (t.state, t.attempt, t.result.as_deref()),
                (State::Review, None, Some(result.as_str())),
                "a fresh reviewer keeps the result"
            );
        }
    }
}

// Chain: § 4 budget, github.com list without assignees -> `assignee-only` admission and report filter. Risk: an
// assigned task never starts, or a listed assignee costs a second read.
#[test]
fn assignees_missing_from_the_list_are_read_per_issue() {
    let b = Fake::default();
    let n = b.create("t", "", &["assignee-only".into()]).unwrap();
    let listed = b.issue(n).unwrap();
    b.state.borrow_mut().issues[0].assignees = vec!["me".into()];
    assert_eq!(
        (assigned(&b, &listed, "me"), assigned(&b, &listed, "you")),
        (Ok(true), Ok(false))
    );
    b.state.borrow_mut().fail_issue_read = Some((n, 1));
    assert!(assigned(&b, &listed, "me").is_err());
    let full = b.issue(n).unwrap();
    b.state.borrow_mut().fail_issue_read = Some((n, 1));
    assert_eq!(
        assigned(&b, &full, "me"),
        Ok(true),
        "a listed assignee needs no read"
    );
}

// Chain: incident lease heartbeat (#1140). Risk: a replaced attempt still listed by its host's heartbeat until
// that host's next pass reported as a second live attempt (#1132, #1137), or a launched lost claim missed.
#[test]
fn incident_heartbeats_flag_only_leases_newer_than_the_attempt() {
    for (lease, age, expected) in [
        ("replaced", 0, false),
        ("current", 0, false),
        ("lost", 0, true),
        ("lost", 301, false),
    ] {
        crate::incident::tests::take_detected();
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
        let replaced = b.comment(n, "taskq: spawn worker claude:a other").unwrap();
        b.comment(n, &format!("taskq: requeue attempt {replaced}"))
            .unwrap();
        let current = b.comment(n, "taskq: spawn worker claude:b first").unwrap();
        let lost = b.comment(n, "taskq: spawn worker claude:c other").unwrap();
        let a = match lease {
            "replaced" => replaced,
            "current" => current,
            _ => lost,
        };
        b.create(
            "taskq host other",
            &format!("#{n} {a}"),
            &[HOST_LABEL.into()],
        )
        .unwrap();
        b.advance(age);
        b.create(
            "taskq host first",
            &format!("#{n} {current}"),
            &[HOST_LABEL.into()],
        )
        .unwrap();
        let c = cfg("local");
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        s.pass().unwrap();
        assert_eq!(
            crate::incident::tests::take_detected(),
            if expected {
                vec!["invariant.double_attempt"]
            } else {
                vec![]
            },
            "{lease} lease, age {age}"
        );
    }
}

// Chain: incident rejected child. Risk: a replaced child reported as rejected.
#[test]
fn incident_rejected_child_excludes_replaced_accepted_child() {
    for rejected in [false, true] {
        crate::incident::tests::take_detected();
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
        let accepted = b
            .comment(n, "taskq: spawn worker claude:first local")
            .unwrap();
        let lost = b
            .comment(n, "taskq: spawn worker claude:second local")
            .unwrap();
        b.comment(n, "taskq: requeue").unwrap();
        let c = cfg("local");
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        s.focus.clear();
        let flag = Rc::new(RefCell::new(None));
        s.kids.push(Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution: if rejected { lost } else { accepted },
            task: n,
            attempt: if rejected { lost } else { accepted },
            rt: "claude".into(),
            proc: Box::new(FakeProc(flag.clone())),
        });
        s.pass().unwrap();
        assert_eq!(*flag.borrow(), Some(-9));
        assert_eq!(
            crate::incident::tests::take_detected(),
            if rejected {
                vec!["invariant.double_attempt"]
            } else {
                vec![]
            }
        );
    }
}

// Chain: incident claim readback. Risk: normal contention reported as a fault.
#[test]
fn incident_claim_missing_readback_excludes_normal_contention() {
    for missing in [false, true] {
        crate::incident::tests::take_detected();
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
        let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300)
            .unwrap()
            .0
            .remove(0);
        if !missing {
            b.comment(n, "taskq: spawn worker claude:first other")
                .unwrap();
        }
        b.state.borrow_mut().hide_latest_comment = missing;
        let c = cfg("local");
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        s.claim(&v, "worker", "claude", "").unwrap();
        assert!(s.kids.is_empty());
        assert_eq!(
            crate::incident::tests::take_detected(),
            if missing {
                vec!["adapter.read_after_write"]
            } else {
                vec![]
            }
        );
    }
}

// Chain: incident label loop (#1550). Risk: two hosts folding one task differently (N-1 `later #N attempt`:
// `later` vs `waiting`) rewrite its label every pass; one relabel reported as a loop.
#[test]
fn label_fix_backs_off_when_another_fold_reverts_it() {
    crate::incident::tests::take_detected();
    let b = Fake::default();
    let n = b
        .create("task", "", &[TASK_LABEL.into(), "q-doing".into()])
        .unwrap();
    let c = cfg("local");
    let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
    s.focus.clear();
    let has = |l: &str| b.issue(n).unwrap().labels.contains(&l.to_string());
    // the other host's fold (`doing`) undoes this host's `ready` before every pass
    for pass in 1..=61 {
        b.labels(n, &["q-doing".into()], &["q-ready".into()])
            .unwrap();
        s.pass().unwrap();
        assert_eq!(has("q-ready"), pass == 1 || pass == 61, "pass {pass}");
        assert_eq!(
            crate::incident::tests::take_detected(),
            if pass == 2 {
                vec!["invariant.label_loop"]
            } else {
                vec![]
            },
            "pass {pass}"
        );
    }
    s.pass().unwrap(); // no fix breaks the streak
    b.labels(n, &["q-doing".into()], &["q-ready".into()])
        .unwrap();
    s.pass().unwrap();
    assert!(has("q-ready") && crate::incident::tests::take_detected().is_empty());
    b.labels(n, &["q-ask".into()], &["q-ready".into()]).unwrap();
    s.pass().unwrap(); // different fix also breaks the streak
    assert!(has("q-ready") && crate::incident::tests::take_detected().is_empty());
}

// Chain: incident label loop (#1873). Risk: another host's normal spawn lands its comment seconds before its label
// move; this host fixes the label, then the next pass reads a list from before the fix (or the peer mid-move) and
// files a loop for a task that moves on normally (2026-10-29: #1813, #1840).
#[test]
fn label_fix_after_another_hosts_spawn_is_no_loop() {
    crate::incident::tests::take_detected();
    let b = Fake::default();
    let n = b
        .create("task", "", &[TASK_LABEL.into(), "q-ready".into()])
        .unwrap();
    b.comment(n, "taskq: spawn worker claude:peer win").unwrap();
    let c = cfg("local");
    let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
    s.focus.clear();
    let before = b.issues(None).unwrap();
    s.pass().unwrap(); // fold `doing`, label `q-ready`: this host fixes it
    assert!(b.issue(n).unwrap().labels.contains(&"q-doing".to_string()));
    b.state.borrow_mut().stale_list = Some(before);
    s.pass().unwrap(); // the list predates the fix
    b.labels(n, &["q-ready".into(), "q-doing".into()], &[])
        .unwrap();
    s.pass().unwrap(); // a read while the peer moves the same way shows both labels
    b.labels(n, &[], &["q-ready".into()]).unwrap();
    s.pass().unwrap();
    assert!(crate::incident::tests::take_detected().is_empty());
    assert!(s.label_fixes.is_empty());
}

// Chain: § 4 one service per host name (#933). Risk: a second install under the same host name posts `gone` for
// the first one's live child and respawns, so two workers run one task.
#[test]
fn a_second_service_under_one_host_name_writes_nothing_and_the_live_child_stays() {
    let b = Fake::default();
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    let c = cfg("win");
    let (ea, eb): (Exits, Exits) = Default::default();
    let mut a = service(&b, &c, ea.clone());
    let mut w = service(&b, &c, eb.clone());
    (a.instance, w.instance) = ("native".into(), "wsl".into());
    a.pass().unwrap();
    let comments = b.comments(n).unwrap().len();
    for _ in 0..3 {
        b.advance(70);
        assert!(
            w.pass()
                .unwrap_err()
                .contains("another service native runs as host win")
        );
        a.pass().unwrap();
    }
    let t = task(&b, n);
    assert_eq!(
        (
            t.state,
            t.retries,
            a.kids.len(),
            w.kids.len(),
            eb.borrow().len()
        ),
        (State::Doing, 0, 1, 0, 0)
    );
    assert_eq!(
        b.comments(n).unwrap().len(),
        comments,
        "no gone, no second spawn"
    );
    // a restart of the same install reads its own id and carries on; the first one stopped, the heartbeat goes
    // stale and the other install takes over the host name
    let mut again = service(&b, &c, Default::default());
    again.instance = "native".into();
    again.pass().unwrap();
    b.advance(301);
    assert!(w.pass().is_ok());
}

// D18: every installed service reports metadata without changing legacy heartbeat lines.
#[test]
fn heartbeat_reports_service_diagnostics() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.instance = "abcdef012345".into();
    let body = s.heartbeat_body();
    assert!(body.lines().any(|l| l.starts_with("diagnostics ")));
    let diagnostics: Value = serde_json::from_str(
        body.lines()
            .find_map(|l| l.strip_prefix("diagnostics "))
            .unwrap(),
    )
    .unwrap();
    let at = diagnostics["observed"].as_i64().unwrap();
    let issue = crate::board::Issue {
        number: 1,
        title: "taskq host mac".into(),
        body,
        labels: vec![],
        assignees: vec![],
        updated: at,
        created: at,
        closed: None,
        open: true,
        trusted: true,
        reason: None,
    };
    let snapshot = crate::snapshot::from_views("board", &b, &c, at, &[], &[issue]);
    assert_eq!(
        snapshot.json()["hosts"][0]["diagnostics"]["running"]["value"],
        "4.0.0"
    );
    let stale = crate::snapshot::from_views(
        "board",
        &b,
        &c,
        at + c.ttl as i64 + 1,
        &[],
        &[crate::board::Issue {
            number: 1,
            title: "taskq host mac".into(),
            body: s.heartbeat_body(),
            labels: vec![],
            assignees: vec![],
            updated: at,
            created: at,
            closed: None,
            open: true,
            trusted: true,
            reason: None,
        }],
    );
    assert_eq!(
        stale.json()["hosts"][0]["diagnostics"]["running"]["state"],
        "stale"
    );
}

// Chain: #1985 A4 host hourly requests -> heartbeat `requests <n>/h` -> status Hosts line. Risk: the PM cannot see
// which host spends the board user's quota, or unbilled 304s count toward it.
#[test]
fn host_hourly_requests_reach_hosts_line() {
    let b = Fake::default();
    let c = cfg("win");
    let mut s = service(&b, &c, Default::default());
    assert!(
        !s.heartbeat_body()
            .lines()
            .any(|l| l.starts_with("requests ")),
        "no full hour, no line"
    );
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
        "none".into()
    ))));
    host.lock().unwrap().last_hour = Some(
        [
            ("GET issues".to_string(), 5),
            ("GET issues 304".to_string(), 40),
        ]
        .into(),
    );
    s.host = Some(host);
    let body = s.heartbeat_body();
    assert!(body.lines().any(|l| l == "requests 5/h"), "{body}");
    let issue = crate::board::Issue {
        number: 1,
        title: "taskq host win".into(),
        body,
        labels: vec![],
        assignees: vec![],
        updated: 100,
        created: 100,
        closed: None,
        open: true,
        trusted: true,
        reason: None,
    };
    let text = crate::snapshot::from_views("board", &b, &c, 100, &[], &[issue])
        .text(&c, &json!({}))
        .unwrap();
    assert!(
        text.lines()
            .any(|l| l.starts_with("Hosts: win v4.0.0 ") && l.ends_with(" 5 req/h")),
        "{text}"
    );
}

// Chain: #2084 A1/A3 host hourly requests per API host -> heartbeat `requests <api host> <n>/h` -> Hosts line
// github.com share. Defect: a host with GitHub and GitLab boards showed one sum. Risk: the share is lost, or an
// older heartbeat with only `requests <n>/h` no longer reads.
#[test]
fn host_github_share_reaches_hosts_line() {
    let b = Fake::default();
    let c = cfg("win");
    let mut s = service(&b, &c, Default::default());
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
        "none".into()
    ))));
    host.lock().unwrap().last_hour = Some([("GET issues".to_string(), 5)].into());
    host.lock().unwrap().hosts_last_hour = Some(
        [
            ("api.github.com".to_string(), 3),
            ("gitlab.com".to_string(), 2),
        ]
        .into(),
    );
    s.host = Some(host);
    let body = s.heartbeat_body();
    for line in [
        "requests 5/h",
        "requests api.github.com 3/h",
        "requests gitlab.com 2/h",
    ] {
        assert!(body.lines().any(|l| l == line), "{line}: {body}");
    }
    let hosts = |body: String| {
        let issue = crate::board::Issue {
            number: 1,
            title: "taskq host win".into(),
            body,
            labels: vec![],
            assignees: vec![],
            updated: 100,
            created: 100,
            closed: None,
            open: true,
            trusted: true,
            reason: None,
        };
        let text = crate::snapshot::from_views("board", &b, &c, 100, &[], &[issue])
            .text(&c, &json!({}))
            .unwrap();
        text.lines()
            .find(|l| l.starts_with("Hosts: win v4.0.0 "))
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        hosts(body.clone()).ends_with(" 5 req/h (github.com 3)"),
        "{body}"
    );
    let old = body
        .lines()
        .filter(|l| !l.starts_with("requests api.") && !l.starts_with("requests gitlab."))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        hosts(old).ends_with(" 5 req/h"),
        "an older heartbeat still reads"
    );
}

// Chain: #1755 A4 tray fault file -> heartbeat `tray <code>` -> status Hosts line. Risk: a tray fault stays in a
// host-local log the PM cannot read, or a hand-written file forwards arbitrary text to the public board.
#[test]
fn tray_fault_reaches_heartbeat_and_hosts_line() {
    let dir = std::env::temp_dir().join(format!("taskq-tray-fault-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let b = Fake::default();
    let c = Config {
        install: dir.clone(),
        ..cfg("win")
    };
    let s = service(&b, &c, Default::default());
    assert!(
        !s.heartbeat_body().lines().any(|l| l.starts_with("tray ")),
        "no fault, no line"
    );
    std::fs::write(dir.join("tray.fault"), "ack-timeout").unwrap();
    let body = s.heartbeat_body();
    assert!(body.lines().any(|l| l == "tray ack-timeout"), "{body}");
    let issue = crate::board::Issue {
        number: 1,
        title: "taskq host win".into(),
        body,
        labels: vec![],
        assignees: vec![],
        updated: 100,
        created: 100,
        closed: None,
        open: true,
        trusted: true,
        reason: None,
    };
    let text = crate::snapshot::from_views("board", &b, &c, 100, &[], &[issue])
        .text(&c, &json!({}))
        .unwrap();
    assert!(
        text.lines()
            .any(|l| l.starts_with("Hosts: win v4.0.0 ") && l.ends_with(" tray ack-timeout")),
        "{text}"
    );
    std::fs::write(dir.join("tray.fault"), "C:\\Users\\x secret").unwrap();
    assert!(
        !s.heartbeat_body().lines().any(|l| l.starts_with("tray ")),
        "only a fixed code crosses to the board"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// Chain: board entry, fold changes, snapshot row `since` (§ 4 Tray age). Risk: the age restarts on a comment that
// changes no state, or invents a time for a task with no logged state change.
#[test]
fn row_since_is_the_last_logged_state_change() {
    let b = Fake::default();
    let c = cfg("mac");
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let since =
        || crate::snapshot::read(&b, &c, &Value::Null, &[]).tasks[0].json()["since"].clone();
    assert_eq!(since(), Value::Null, "no logged state change: unknown");
    b.advance(100);
    let w = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    let at = b
        .comments(n)
        .unwrap()
        .into_iter()
        .find(|x| x.id == w)
        .unwrap()
        .created;
    assert_eq!(since(), at);
    b.advance(100);
    b.raw(n, "a plain note", true, false);
    assert_eq!(since(), at, "a comment that changes no state keeps the age");
}

// Chain: `taskq doctor --hosts` request issue -> service pass -> heartbeat `doctor-answer` (#2194 A2). Risk: a host
// refreshed its checks only every 10 minutes, so `doctor --hosts` showed results older than a token fix and could
// not tell a checked host from a silent one. Protection: a new trusted request runs the checks in the next pass and
// the heartbeat names its id, once per id; an untrusted request runs nothing.
#[test]
fn service_answers_a_doctor_request_in_its_heartbeat() {
    let b = Fake::default();
    let c = cfg("win");
    let mut s = Service::new(&b, &c, "fixture", Box::new(|_| panic!("no launch")));
    let heartbeat = |b: &Fake| {
        b.issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host win")
            .unwrap()
            .body
    };
    s.pass().unwrap();
    assert!(
        !heartbeat(&b).contains("doctor"),
        "no request, no installed service: no checks"
    );
    let r = b
        .create(
            crate::health::REQUEST_TITLE,
            &crate::health::request_body("abc123"),
            &[HOST_LABEL.into()],
        )
        .unwrap();
    s.pass().unwrap();
    let body = heartbeat(&b);
    assert!(body.lines().any(|l| l == "doctor-answer abc123"), "{body}");
    assert!(
        body.lines().any(|l| l
            .strip_prefix("doctor ")
            .and_then(crate::health::parse)
            .is_some()),
        "{body}"
    );
    let ran = s.health.as_ref().unwrap().0;
    s.pass().unwrap();
    assert_eq!(s.health.as_ref().unwrap().0, ran, "one run per request");
    b.edit(r, &crate::health::request_body("evil1")).unwrap();
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == r)
        .unwrap()
        .trusted = false;
    s.pass().unwrap();
    assert!(
        !heartbeat(&b).contains("evil1"),
        "an untrusted request runs nothing"
    );
    assert_eq!(s.health.as_ref().unwrap().0, ran);
}

// Chain: § 4 heartbeat body, DashQ host join (dashq docs/design/hosts.md H2). Risk: a changed or missing line
// splits one machine into several Hosts rows or breaks the one-service-per-host check.
#[test]
fn heartbeat_body_lists_version_service_machine_focus() {
    let legacy = |s: &Service| {
        s.heartbeat_body()
            .lines()
            .filter(|l| {
                !l.starts_with("diagnostics ")
                    && !l.starts_with("binary-evidence ")
                    && !l.starts_with("balancing ")
                    && !l.starts_with("queue ")
                    && !l.starts_with("account-policy ")
                    && crate::runtime::usage::Row::parse(l).is_none()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let b = Fake::default();
    let c = cfg("win");
    let mut s = service(&b, &c, Default::default());
    s.instance = "abc123".into();
    s.machine = "DESKTOP wsl".into();
    s.focus = vec![];
    assert_eq!(
        legacy(&s),
        "version 4.0.0\nservice abc123\nmachine DESKTOP wsl\nfocus off"
    );
    // #1832: the host link key goes with the heartbeat the board trusts
    s.link = "link aa bb".into();
    assert_eq!(
        legacy(&s),
        "version 4.0.0\nservice abc123\nmachine DESKTOP wsl\nlink aa bb\nfocus off"
    );
    s.link = String::new();
    s.machine = String::new();
    s.focus = vec!["all".into()];
    assert_eq!(legacy(&s), "version 4.0.0\nservice abc123");
    // #2194: the last health check goes with the heartbeat `taskq doctor --hosts` reads
    s.health = Some((
        std::time::Instant::now(),
        "doctor 5 token-contents:gap".into(),
    ));
    assert_eq!(
        legacy(&s),
        "version 4.0.0\nservice abc123\ndoctor 5 token-contents:gap"
    );
    s.health = None;
    // #1204: a live Claude child publishes its account-scoped URL; every host's snapshot reads it back by host and attempt
    s.pending.insert((7, 42));
    s.kids.push(Kid {
        unknown: None,
        stopping: false,
        session_url: Some(Ok("https://claude.ai/code/session_014abc".into())),
        task: 7,
        attempt: 42,
        execution: 42,
        rt: "claude".into(),
        proc: Box::new(FakeProc(Default::default())),
    });
    assert_eq!(
        legacy(&s),
        "version 4.0.0\nservice abc123\n#7 42\nsession #7 42 https://claude.ai/code/session_014abc"
    );
    let host = |body: String| crate::board::Issue {
        number: 1,
        title: "taskq host win".into(),
        body,
        labels: vec![],
        assignees: vec![],
        updated: 0,
        created: 0,
        closed: None,
        open: true,
        trusted: true,
        reason: None,
    };
    let urls = crate::snapshot::session_urls(&[
        host(s.heartbeat_body()),
        host("session #8 43 https://evil.example/x".into()),
    ]);
    assert_eq!(
        urls.into_iter().collect::<Vec<_>>(),
        vec![(
            ("win".to_string(), 42),
            "https://claude.ai/code/session_014abc".to_string()
        )]
    );
    // #2261: with no link yet the heartbeat names the runtime's cause, which every host reads back and never links
    s.kids[0].session_url = Some(Err(crate::runtime::absence(
        "failed",
        Some("bridge refused\nsecond line"),
    )));
    assert!(legacy(&s).ends_with("\nsession #7 42 unknown: failed: bridge refused"));
    let urls = crate::snapshot::session_urls(&[host(s.heartbeat_body())]);
    assert_eq!(
        urls.get(&("win".to_string(), 42)).map(String::as_str),
        Some("unknown: failed: bridge refused")
    );
    assert!(!crate::runtime::link_ok(&urls[&("win".to_string(), 42)]));
}

// Chain: §4 native observation -> local status row -> heartbeat row. Risk (#2427 A7): a live process reads running
// locally and unknown to every other reader; only the host-local process_id and observation times may differ.
#[test]
fn a_live_process_reads_the_same_in_status_and_heartbeat() {
    struct Live(u32);
    impl Proc for Live {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(None)
        }
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn id(&self) -> u32 {
            self.0
        }
    }
    let b = Fake::default();
    let c = cfg("mac");
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let pid = std::process::id();
    let mut s = Service::new(&b, &c, "test", Box::new(move |_| Ok(Box::new(Live(pid)))));
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    s.pass().unwrap();
    // A read failure leaves the verdict unknown while the process lives.
    s.kids[0].unknown = Some("native read failed".into());
    s.pass().unwrap();
    s.beat().unwrap();
    let mut local = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json()["executions"]
        .clone();
    let mut published = crate::snapshot::read(&b, &cfg("win"), &json!({}), &[])
        .tasks
        .into_iter()
        .find(|r| r.n == n)
        .unwrap()
        .json()["executions"]
        .clone();
    assert_eq!(
        local[0]["process_id"], pid,
        "the local row names its process"
    );
    assert_eq!(
        published[0].get("process_id"),
        None,
        "a host-local pid stays off the heartbeat"
    );
    assert_eq!(local[0]["state"], "running");
    for row in [&mut local, &mut published] {
        for key in ["observed", "process_id"] {
            row[0].as_object_mut().unwrap().remove(key);
        }
        row[0]["progress"] = Value::Null;
    }
    assert_eq!(local, published);
}

// Chain: native observation -> aggregate heartbeat -> remote read -> durable terminal after lost response.
#[test]
fn execution_heartbeat_is_fenced_visible_and_never_reopens_terminal() {
    for (rt, unknown_exit) in crate::runtime::RUNTIMES
        .iter()
        .flat_map(|rt| [false, true].map(|unknown| (*rt, unknown)))
    {
        if crate::runtime::of(rt).has(crate::runtime::Cap::AppTurn)
            && !cfg!(any(target_os = "macos", windows))
        {
            continue;
        }
        let b = Fake::default();
        let mut c = cfg("mac");
        let remote = cfg("win");
        c.default_runtime = crate::runtime::RuntimeId::parse(rt).unwrap();
        c.limits = BTreeMap::from([(rt.into(), 1)]);
        c.has = vec![rt.into()];
        let n = b
            .create("T", "", &[TASK_LABEL.into(), format!("run-{rt}")])
            .unwrap();
        let exits: Exits = Default::default();
        let mut s = service(&b, &c, exits.clone());
        for pass in 1..=2 {
            s.pass().unwrap();
            s.beat().unwrap();
            let mut local = s
                .snapshot
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .find(|r| r.n == n)
                .unwrap()
                .json()["executions"]
                .clone();
            let mut published = crate::snapshot::read(&b, &remote, &json!({}), &[])
                .tasks
                .into_iter()
                .find(|r| r.n == n)
                .unwrap()
                .json()["executions"]
                .clone();
            assert_eq!(
                local.as_array().map(Vec::len),
                Some(1),
                "new execution is visible on pass {pass} for {rt}"
            );
            for rows in [&mut local, &mut published] {
                for row in rows.as_array_mut().unwrap() {
                    row.as_object_mut().unwrap().remove("observed");
                    row.as_object_mut().unwrap().remove("process_id");
                    if let Some(progress) = row.get_mut("progress").and_then(Value::as_object_mut) {
                        progress.remove("observed");
                    }
                }
            }
            assert_eq!(
                local, published,
                "status/heartbeat parity on pass {pass} for {rt}"
            );
        }
        let a = task(&b, n).attempt.unwrap();
        let read = || {
            crate::snapshot::read(&b, &remote, &json!({}), &[])
                .tasks
                .into_iter()
                .find(|r| r.n == n)
                .unwrap()
                .json()
        };
        let host = b
            .issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host mac")
            .unwrap();
        let running = host.body.clone();
        assert_eq!(read()["executions"][0]["state"], "running");
        assert_eq!(read()["executions"][0]["runtime"], rt);
        assert_eq!(read()["executions"][0]["role"], "worker");
        let snapshot = s.snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot.tasks[0].executions[0]["runtime"], rt,
            "local overlay keeps runtime"
        );
        assert_eq!(
            snapshot.tasks[0].executions[0]["role"], "worker",
            "local overlay keeps role"
        );
        let mut local = snapshot.tasks[0].json()["executions"].clone();
        let mut published = read()["executions"].clone();
        // Heartbeat publication advances the fixture board clock after the local snapshot.
        let local_at = local[0]
            .as_object_mut()
            .unwrap()
            .remove("observed")
            .unwrap()
            .as_i64()
            .unwrap();
        let published_at = published[0]
            .as_object_mut()
            .unwrap()
            .remove("observed")
            .unwrap()
            .as_i64()
            .unwrap();
        assert!(
            published_at >= local_at,
            "publication cannot predate its local observation"
        );
        assert_eq!(
            local, published,
            "local status and heartbeat readback agree for {rt}"
        );
        assert_eq!(
            snapshot
                .status_keys(None)
                .into_iter()
                .find(|(k, _)| *k == "hosts")
                .unwrap()
                .1,
            snapshot.json()["hosts"]
        );
        for fault in [
            "foreign",
            "execution",
            "input",
            "duplicate",
            "malformed",
            "partial-duplicate",
            "invalid-state",
            "untrusted",
            "stale",
            "future",
        ] {
            let mut body = running.clone();
            if fault == "foreign" {
                body = body.replace("execution ", "ignored ");
            }
            if matches!(fault, "execution" | "input" | "invalid-state") {
                let mut row: Value = serde_json::from_str(
                    running
                        .lines()
                        .find_map(|l| l.strip_prefix("execution "))
                        .unwrap(),
                )
                .unwrap();
                match fault {
                    "execution" => row["id"] = json!(a + 999),
                    "input" => row["through"] = json!(a + 999),
                    _ => row["state"] = json!("finished"),
                };
                body = running
                    .lines()
                    .filter(|l| !l.starts_with("execution "))
                    .chain(std::iter::once(""))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + &format!("execution {row}");
            }
            if fault == "duplicate" {
                body += &format!(
                    "\n{}",
                    running
                        .lines()
                        .find(|l| l.starts_with("execution "))
                        .unwrap()
                );
            }
            if fault == "partial-duplicate" {
                body += "\nexecution {";
            }
            if fault == "malformed" {
                body = running
                    .lines()
                    .filter(|l| !l.starts_with("execution "))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\nexecution {";
            }
            b.edit(host.number, &body).unwrap();
            {
                let mut st = b.state.borrow_mut();
                let now = st.clock;
                let h = st
                    .issues
                    .iter_mut()
                    .find(|i| i.number == host.number)
                    .unwrap();
                h.trusted = fault != "untrusted";
                if fault == "stale" {
                    h.updated = now - remote.ttl as i64 - 1;
                }
                if fault == "future" {
                    h.updated = now + 60;
                }
            }
            let row = read();
            assert_eq!(row["executions"][0]["state"], "unknown", "{fault}: {row}");
            assert!(row["execution_wait"].is_string(), "{fault}");
            b.state
                .borrow_mut()
                .issues
                .iter_mut()
                .find(|i| i.number == host.number)
                .unwrap()
                .trusted = true;
        }
        b.edit(host.number, &running).unwrap();
        s.kids.iter_mut().find(|k| k.attempt == a).unwrap().stopping = true;
        s.beat().unwrap();
        assert_eq!(read()["executions"][0]["state"], "waiting");
        s.kids.iter_mut().find(|k| k.attempt == a).unwrap().stopping = false;
        *exits
            .borrow()
            .iter()
            .find(|(id, _)| *id == n)
            .unwrap()
            .1
            .borrow_mut() = Some(-999);
        s.pass().unwrap();
        assert!(
            read()["executions"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("read failed")
        );
        assert!(
            s.snapshot
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .find(|r| r.n == n)
                .unwrap()
                .json()["execution_wait"]
                .as_str()
                .unwrap()
                .contains("read failed")
        );
        *exits
            .borrow()
            .iter()
            .find(|(id, _)| *id == n)
            .unwrap()
            .1
            .borrow_mut() = Some(0);
        if unknown_exit {
            s.kids.iter_mut().find(|k| k.attempt == a).unwrap().proc = Box::new(UnknownExit);
        }
        b.state.borrow_mut().fail_comments = 1;
        s.reap_owned();
        s.beat().unwrap();
        assert_eq!(s.kids.len(), 1);
        assert_eq!(
            read()["executions"][0]["state"],
            "unknown",
            "failed final delivery retains ownership"
        );
        b.state.borrow_mut().lose_comment_response = true;
        s.reap_owned();
        assert!(s.kids.is_empty());
        let events = crate::telemetry::worker_events(&c.install);
        let exit = events
            .iter()
            .find(|e| e.task == n && e.attempt == a && e.event == "exit")
            .unwrap();
        assert_eq!(exit.outcome, if unknown_exit { "unknown" } else { "ok" });
        assert_eq!(
            exit.reason,
            if unknown_exit {
                "exit-unknown"
            } else {
                "exit-0"
            }
        );
        b.state.borrow_mut().lose_comment_response = false;
        let log = entries(&b, n).unwrap();
        assert_eq!(
            core::executions(n, &log)[0].release,
            Some(core::Release::Exit)
        );
        assert_eq!(
            log.iter()
                .filter(|e| e.body.starts_with("taskq: released"))
                .count(),
            1
        );
        b.edit(host.number, &running).unwrap();
        let row = read();
        assert!(
            row.get("executions").is_none(),
            "late heartbeat cannot reopen terminal"
        );
        let mut fresh = service(&b, &c, Default::default());
        fresh.focus.clear();
        fresh.pass().unwrap();
        assert!(
            !fresh
                .heartbeat_body()
                .lines()
                .any(|l| l.starts_with("execution ")),
            "restart reads durable terminal"
        );
        println!(
            "{rt}: local status/heartbeat running, unknown, waiting and ended projections agree; fixture board/process only"
        );
    }
}

// Chain: slow native children -> bounded observation round -> honest heartbeat -> fair next round.
#[test]
fn slow_observations_defer_without_releasing_and_rotate_fairly() {
    // File reads and scheduling can consume 10ms before the first fixture poll on a loaded host.
    const POLL_MS: u64 = 100;
    struct Slow {
        task: u64,
        calls: Rc<RefCell<Vec<u64>>>,
    }
    impl Proc for Slow {
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(true)
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            self.calls.borrow_mut().push(self.task);
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS * 5 / 2));
            Ok(None)
        }
        fn kill(&mut self) -> Result<(), String> {
            Err("fixture must stay owned".into())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 3);
    for _ in 0..3 {
        b.create("slow fixture", "", &[TASK_LABEL.into()]).unwrap();
    }
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(s.kids.len(), 3);
    let calls = Rc::new(RefCell::new(vec![]));
    for kid in &mut s.kids {
        kid.proc = Box::new(Slow {
            task: kid.task,
            calls: calls.clone(),
        });
    }
    for round in 1..=3 {
        s.pass_inner(std::time::Duration::from_millis(POLL_MS))
            .unwrap();
        assert_eq!(
            calls.borrow().len(),
            round,
            "a slow poll must not delay status behind all children"
        );
        assert_eq!(s.kids.len(), 3);
        assert_eq!(
            s.kids.iter().filter(|k| k.unknown.is_some()).count(),
            2,
            "deferred observations must be unknown, not freshly healthy"
        );
        let heartbeat = b.issue(s.host_issue.unwrap()).unwrap();
        assert!(heartbeat.body.contains("native observation deferred"));
    }
    assert_eq!(
        calls
            .borrow()
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3,
        "slow first child starved another execution"
    );
    calls.borrow_mut().clear();
    for round in 1..=3 {
        s.reap_owned_within(std::time::Duration::from_millis(POLL_MS));
        assert_eq!(calls.borrow().len(), round);
        assert_eq!(s.kids.len(), 3);
        assert_eq!(s.kids.iter().filter(|k| k.unknown.is_some()).count(), 2);
    }
    assert_eq!(
        calls
            .borrow()
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3,
        "reduced pass starved an execution"
    );
    let tasks = s.kids.iter().map(|k| k.task).collect::<Vec<_>>();
    s.kids.clear();
    calls.borrow_mut().clear();
    let recovery_calls = calls.clone();
    s.recover = Some(Box::new(move |task, _| {
        recovery_calls.borrow_mut().push(task);
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS * 5 / 2));
        Err("fixture: native recovery unknown".into())
    }));
    for round in 1..=3 {
        s.pass_inner(std::time::Duration::from_millis(POLL_MS))
            .unwrap();
        assert_eq!(calls.borrow().len(), round);
        assert_eq!(s.execution_waits.len(), 3);
        assert!(s.kids.is_empty(), "unknown recovery cannot replay work");
    }
    assert_eq!(
        calls
            .borrow()
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3,
        "cold recovery starved an execution"
    );
    for task in tasks {
        assert!(
            core::executions(task, &entries(&b, task).unwrap())
                .iter()
                .all(|x| x.release.is_none())
        );
    }
}

// Chain: § 4 board budget, heartbeat. Risk: a PATCH every pass burns the shared budget (#1358: the diagnostics
// observation time changed the body every pass), or a sparse heartbeat lets another host expire a live lease.
#[test]
fn idle_heartbeat_is_batched_at_half_ttl_and_stays_fresh() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.instance = "abcdef012345".into();
    let beat = |b: &Fake| {
        b.issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host mac")
            .map(|i| i.updated)
            .unwrap()
    };
    s.pass().unwrap();
    let (mut writes, mut last) = (0, beat(&b));
    for _ in 0..360 {
        b.advance(10);
        assert!(
            b.now().unwrap() - beat(&b) < c.ttl,
            "lease stays fresh between passes"
        );
        // a live producer clock: every pass reports new observation times
        let mut d = s.service_diagnostics();
        d["observed"] = json!(b.now().unwrap());
        d["running"]["at"] = json!(b.now().unwrap());
        *s.diagnostics.borrow_mut() = Some((std::time::Instant::now(), d));
        s.pass().unwrap();
        if beat(&b) != last {
            writes += 1;
            last = beat(&b);
        }
    }
    assert_eq!(
        writes,
        3600 / (c.ttl / 2),
        "one periodic write per ttl/2 without an attempt, not one per fast pass"
    );
    assert_eq!(
        renewal("version 1\n#7 9", c.ttl),
        c.ttl / 3,
        "a heartbeat holding a lease keeps the wider margin"
    );
    assert_eq!(renewal("version 1", c.ttl), c.ttl / 2);
    let body = b
        .issues(None)
        .unwrap()
        .into_iter()
        .find(|i| i.title == "taskq host mac")
        .unwrap()
        .body;
    let at = body.lines().find_map(|l| l.strip_prefix("at ")).unwrap();
    assert!(
        body.contains(&format!("\"observed\":{at},")),
        "a write carries the current diagnostics"
    );
    let line = body
        .lines()
        .find(|l| l.starts_with("diagnostics "))
        .unwrap();
    let mut d: Value = serde_json::from_str(&line["diagnostics ".len()..]).unwrap();
    d["running"]["value"] = json!("9.9.9");
    assert_ne!(
        stable(&body),
        stable(&body.replace(line, &format!("diagnostics {d}"))),
        "a changed diagnostics value is written at once"
    );
    // § 3 Runtime accounts: a changed usage row travels with the periodic write; a hold or unmet policy is state.
    let usage = |used: u8, policy: &str| {
        format!(
            "host x\nlimit codex 2026-01-01T00:00:00Z\nlimit codex work id - window 10080m used {used} reset - at 2026-01-01T00:00:00Z\naccount-policy {policy}\n"
        )
    };
    assert_eq!(
        stable(&usage(10, "local no connection")),
        stable(&usage(90, "local no connection")),
        "a usage change forces no write"
    );
    assert!(
        stable(&usage(10, "local no connection"))
            .iter()
            .any(|l| l == "limit codex 2026-01-01T00:00:00Z"),
        "a hold line stays compared"
    );
    assert_ne!(
        stable(&usage(10, "local no connection")),
        stable(&usage(10, "unmet codex abcd1234")),
        "a policy change is written at once"
    );
}

// Chain: § 8 step 3, board budget. Risk: a reduced or observe pass (Stop barrier, quarantine) rewrote the
// heartbeat every ttl / 5 because live diagnostics times changed its raw lines; one PATCH a minute per project.
#[test]
fn reduced_heartbeat_ignores_diagnostics_times_and_writes_at_a_third_of_ttl() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.instance = "abcdef012345".into();
    let beat = |b: &Fake| {
        b.issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host mac")
            .map(|i| i.updated)
            .unwrap()
    };
    s.pass().unwrap();
    let (mut writes, mut last) = (0, beat(&b));
    for t in 1..=360 {
        b.advance(10);
        s.mono = t * 10;
        assert!(
            b.now().unwrap() - beat(&b) < c.ttl,
            "lease stays fresh between reduced passes"
        );
        let mut d = s.service_diagnostics();
        d["observed"] = json!(b.now().unwrap());
        d["running"]["at"] = json!(b.now().unwrap());
        *s.diagnostics.borrow_mut() = Some((std::time::Instant::now(), d));
        s.reduced().unwrap();
        if beat(&b) != last {
            writes += 1;
            last = beat(&b);
        }
    }
    assert!(
        writes <= 3600 / (c.ttl / 3),
        "{writes} reduced writes in an hour, want at most one per ttl / 3"
    );
}

// Chain: § 4 step 3, board budget (#2352). Risk: a value that is the same for the whole host changed and every
// project wrote its board at once, so 13 projects spent 13 writes on one change; focus off stopped none of them.
#[test]
fn host_wide_values_wait_for_the_renewal() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
        "fixture holds admission".into(),
    ))));
    s.host = Some(host.clone());
    let beat = |b: &Fake| {
        b.issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host mac")
            .map(|i| i.updated)
            .unwrap()
    };
    s.pass().unwrap();
    let (mut writes, mut last) = (0, beat(&b));
    for t in 1..=60u64 {
        b.advance(60);
        s.health = Some((std::time::Instant::now(), format!("doctor {t} ok")));
        {
            let mut h = host.lock().unwrap();
            h.last_hour = Some(BTreeMap::from([("GET issues".to_string(), t)]));
            h.queues.insert(
                "other".into(),
                crate::host::Queue {
                    ready: t,
                    ..Default::default()
                },
            );
        }
        s.pass().unwrap();
        if beat(&b) != last {
            writes += 1;
            last = beat(&b);
        }
        assert!(b.now().unwrap() - last < c.ttl, "lease stays fresh");
    }
    assert_eq!(
        writes, 20,
        "one renewal per ttl / 2 at 60 s passes, not one write per changed value"
    );
}

// Chain: § 4 step 3 (#2352). Risk: the host-wide cut hides a change another host or the owner must see at once.
#[test]
fn stable_lists_what_forces_a_heartbeat_write() {
    let binary = r#"binary-evidence {"observed":100,"native_observed":99,"revision":"a","state":"binary_ready"}"#;
    assert_eq!(
        stable(binary),
        stable(&binary.replace(":100", ":200").replace(":99", ":199")),
        "advisory sample clocks retain the heartbeat budget"
    );
    assert_ne!(
        stable(binary),
        stable(&binary.replace("binary_ready", "unknown")),
        "lost proof is a meaningful change"
    );
    let bal = |code: &str, full: bool, last: u64| {
        format!(
            r#"balancing {{"schema":2,"last_spawn":{last},"mode":"any","has":["gui"],"focus":["all"],"runtimes":{{"codex":{{"code":"{code}","reason":"r{code}","full":{full},"disabled":false,"quota":[],"load":{{"free":1}}}}}}}}"#
        )
    };
    let base = format!(
        "version 1\n{}\nqueue {{\"restart\":null,\"projects\":[{{\"ready\":1}}]}}\ndoctor 100 ok\nservice ab",
        bal("admitted", false, 5)
    );
    for (from, to) in [
        (bal("admitted", false, 5), bal("spacing", false, 5)),
        ("\"ready\":1".to_string(), "\"ready\":2".to_string()),
        ("doctor 100 ok".to_string(), "doctor 200 ok".to_string()),
        (
            "service ab".to_string(),
            "service ab\nhas gui\nheadroom codex none\nrequests 9/h".to_string(),
        ),
    ] {
        assert_eq!(
            stable(&base),
            stable(&base.replace(&from, &to)),
            "host-wide: {to}"
        );
    }
    for (from, to) in [
        ("version 1".to_string(), "version 2".to_string()),
        ("service ab".to_string(), "service cd".to_string()),
        (bal("admitted", false, 5), bal("admitted", true, 5)),
        (bal("admitted", false, 5), bal("admitted", false, 6)),
        (
            "doctor 100 ok".to_string(),
            "doctor 100 token:gap".to_string(),
        ),
        ("\"restart\":null".to_string(), "\"restart\":7".to_string()),
        ("service ab".to_string(), "service ab\n#7 9".to_string()),
        (
            "service ab".to_string(),
            "service ab\nsession #7 9 u".to_string(),
        ),
        (
            "service ab".to_string(),
            "service ab\nexecution {}".to_string(),
        ),
        (
            "service ab".to_string(),
            "service ab\nfocus off".to_string(),
        ),
        (
            "service ab".to_string(),
            "service ab\nlimit codex probe x".to_string(),
        ),
        (
            "service ab".to_string(),
            "service ab\nlogin codex off".to_string(),
        ),
        (
            "service ab".to_string(),
            "service ab\nmachine m".to_string(),
        ),
        (
            "service ab".to_string(),
            "service ab\ndoctor-answer 4".to_string(),
        ),
    ] {
        assert_ne!(
            stable(&base),
            stable(&base.replace(&from, &to)),
            "written at once: {to}"
        );
    }
}

// Chain: § 4 heartbeat, status.json hosts (#1729 A3). Risk: after a sleep the pass reads this host's heartbeat
// before rewriting it, so status.json shows this host stale for a whole pass interval while the board is fresh.
#[test]
fn own_heartbeat_row_is_fresh_after_a_wake() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.instance = "abcdef012345".into();
    s.pass().unwrap();
    b.advance(1920);
    s.pass().unwrap();
    let snapshot = s.snapshot.as_ref().unwrap();
    let row = snapshot.hosts.iter().find(|h| h.name == "mac").unwrap();
    assert_eq!(
        snapshot.observed - row.updated,
        0,
        "the pass that rewrote the heartbeat reports its write"
    );
}

// Chain: § 4 heartbeat, host rename (#1483). Risk: a renamed host leaves its old-name heartbeat open in every
// Hosts line, closes another service's heartbeat, or never replaces its own closed one.
#[test]
fn renamed_host_closes_only_its_own_old_heartbeat() {
    let b = Fake::default();
    let c = cfg("mac");
    let old = b
        .create(
            "taskq host macbook-m2.local",
            "version 4.0.0\nservice abcdef012345",
            &[HOST_LABEL.into()],
        )
        .unwrap();
    let other = b
        .create(
            "taskq host win",
            "version 4.0.0\nservice 999999999999",
            &[HOST_LABEL.into()],
        )
        .unwrap();
    b.advance(10 * c.ttl);
    let mut s = service(&b, &c, Default::default());
    s.instance = "abcdef012345".into();
    s.pass().unwrap();
    let old_issue = b.issue(old).unwrap();
    assert!(
        !old_issue.open,
        "A1: the old-name heartbeat of this service closes"
    );
    assert!(
        b.comments(old)
            .unwrap()
            .iter()
            .any(|c| c.body.contains("host mac")),
        "A1: the close names the new host"
    );
    assert!(
        b.issue(other).unwrap().open,
        "A3: a stale heartbeat of another service stays open"
    );
    assert!(b.comments(other).unwrap().is_empty());
    // A2: the host's own closed heartbeat is replaced on the next pass; the old name stays closed
    let mine = s.host_issue.unwrap();
    b.close(mine).unwrap();
    b.advance(c.ttl);
    s.pass().unwrap();
    let open: Vec<_> = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(is_host)
        .map(|i| i.title)
        .collect();
    assert_eq!(open, ["taskq host win", "taskq host mac"]);
    assert!(!b.issue(old).unwrap().open && s.host_issue != Some(mine));
}

// Chain: R2 two hosts -> lost heartbeat -> still-live executor -> confirmed exit -> replacement.
#[test]
fn two_hosts_wait_for_confirmed_exit_before_replacement() {
    let b = Fake::default();
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    let (ca, cb) = (cfg("mac"), cfg("win"));
    let (ea, eb): (Exits, Exits) = Default::default();
    let mut a = service(&b, &ca, ea.clone());
    let mut w = service(&b, &cb, eb.clone());
    a.pass().unwrap();
    w.pass().unwrap();
    let t = task(&b, n);
    assert_eq!((t.state, t.role.as_deref()), (State::Doing, Some("worker")));
    assert_eq!((a.kids.len(), w.kids.len()), (1, 0));
    assert!(b.issue(n).unwrap().labels.contains(&"q-doing".to_string()));

    // a live mac outlasts the ttl: its unchanged heartbeat stays fresh, win leaves the task alone
    for _ in 0..3 {
        b.advance(200);
        a.pass().unwrap();
        w.pass().unwrap();
    }
    assert_eq!(host_of(&task(&b, n)), Some("mac"));
    // #1491: a fresh owner heartbeat listing the execution as running is no wait on the other host
    assert!(
        !w.execution_waits.contains_key(&n),
        "{:?}",
        w.execution_waits
    );
    let row = w
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert!(row["execution_wait"].is_null(), "{row}");
    assert_eq!(
        (
            row["executions"][0]["state"].as_str(),
            row["executions"][0]["host"].as_str()
        ),
        (Some("running"), Some("mac"))
    );
    // a fresh owner heartbeat without the execution row still waits
    let hb = b
        .issues(None)
        .unwrap()
        .into_iter()
        .find(|i| i.title == "taskq host mac")
        .unwrap();
    b.edit(
        hb.number,
        &hb.body
            .lines()
            .filter(|l| !l.starts_with("execution "))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    w.pass().unwrap();
    assert!(w.execution_waits[&n].contains("termination unconfirmed"));
    // A missing heartbeat does not kill the old executor.
    b.advance(301);
    w.pass().unwrap();
    assert_eq!(host_of(&task(&b, n)), Some("mac"));
    assert_eq!(
        ea.borrow()
            .iter()
            .chain(eb.borrow().iter())
            .filter(|(_, p)| p.borrow().is_none())
            .count(),
        1
    );
    assert!(w.execution_waits[&n].contains("termination unconfirmed"));
    let snapshot = crate::snapshot::read(&b, &cb, &json!({}), &[]);
    assert!(
        snapshot.tasks.iter().find(|r| r.n == n).unwrap().json()["execution_wait"]
            .as_str()
            .unwrap()
            .contains("termination unconfirmed")
    );
    let text = snapshot.text(&cb, &json!({})).unwrap();
    assert!(
        text.contains("Run state unknown; see service log")
            && !text.contains("termination unconfirmed"),
        "#1524 owner text only: {text}"
    );
    assert!(eb.borrow().is_empty());
    *ea.borrow()[0].1.borrow_mut() = Some(0);
    a.focus.clear();
    a.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Ready);
    w.pass().unwrap();
    let t = task(&b, n);
    assert_eq!((t.state, host_of(&t)), (State::Doing, Some("win")));
    assert_eq!(t.retries, 1);
}

// Chain: #1226 open stage -> vanished child -> normal gone -> terminal record and freed lease.
// Existing exit checks do not cross the retained profile store or selftest boundary.
#[test]
fn missing_terminal_record_repaired_only_by_normal_recovery() {
    let dir = std::env::temp_dir().join(format!("taskq-terminal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (_capture, _guard) = crate::profile::tests::collecting(&dir);
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let a = b
        .comment(
            n,
            &core::entry("spawn", &["worker", "claude:s", "mac"], None, ""),
        )
        .unwrap();
    let mut c = cfg("mac");
    c.install = dir.clone();
    c.root = dir.clone();
    let record = json!({"project":c.repo,"host":"mac","stage":"build","open":true,"task":n,"attempt":a,"rt":"claude",
            "outcome":"unknown","error":"none","at_ms":1,"unknown":"open-stage"});
    crate::profile::tests::append_line(&dir.join("taskq.log"), &format!("taskq-stage {record}"))
        .unwrap();
    // #1389: an old gap warns once with a stable reason and passes; unavailable capture still fails (tests/telemetry_process.rs).
    let warn = crate::profile::selftest(&dir, &c.repo, &vec![], 300).unwrap();
    assert!(
        warn.starts_with("attempt terminal records: WARN (1 older than ttl 300s; not blocking)"),
        "{warn}"
    );
    assert_eq!(
        warn.matches(&format!("| o/r | #{n} | {a} | claude | mac |"))
            .count(),
        1,
        "{warn}"
    );
    assert!(warn.contains("| missing terminal record |"), "{warn}");
    // A live attempt stays exempt even after ttl; the check never mutates board state.
    assert!(
        crate::profile::selftest(&dir, &c.repo, &vec![(c.repo.clone(), a)], 300)
            .unwrap()
            .contains("no overdue")
    );
    assert_eq!(task(&b, n).attempt, Some(a));
    let report = crate::profile::report(&dir);
    assert!(
        report.contains(&format!("| o/r | #{n} | {a} | claude | mac |")),
        "{report}"
    );
    let mut s = service(&b, &c, Default::default());
    s.focus.clear();
    s.pass().unwrap();
    assert_eq!(
        task(&b, n).attempt,
        Some(a),
        "missing child is unknown, not exited"
    );
    assert!(s.execution_waits.contains_key(&n));
    release_execution(&b, n, a, a, "exit").unwrap();
    s.pass().unwrap();
    assert!(
        b.comments(n)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with(&format!("taskq: gone attempt {a}")))
    );
    assert_eq!(task(&b, n).attempt, None);
    assert!(!s.heartbeat_body().lines().any(|l| l == format!("#{n} {a}")));
    let repaired = crate::profile::tests::read_log(&dir).unwrap();
    assert!(
        repaired.contains("\"outcome\":\"failed\",\"error\":\"exit\""),
        "{repaired}"
    );
    assert!(
        crate::profile::selftest(&dir, &c.repo, &vec![], 300)
            .unwrap()
            .contains("no overdue")
    );
    s.pass().unwrap();
    assert_eq!(
        crate::profile::tests::read_log(&dir).unwrap(),
        repaired,
        "repair is idempotent"
    );

    drop(_guard);
    drop(_capture);
    std::fs::remove_dir_all(dir).unwrap();
}

// Chain: §4 ended review -> left-board -> close -> next-pass repair -> selftest.
// Risk: historical attempts absent from open views permanently block unrelated workers.
#[test]
fn closed_or_left_board_attempt_gets_terminal_record() {
    for closed in [true, false] {
        let dir = std::env::temp_dir().join(format!("taskq-ended-{}-{closed}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (_capture, _guard) = crate::profile::tests::collecting(&dir);
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
        b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40)))
            .unwrap();
        let a = b.comment(n, "taskq: spawn reviewer codex:r mac").unwrap();
        assert_eq!(task(&b, n).attempt, Some(a));
        let mut c = cfg("mac");
        c.install = dir.clone();
        c.root = dir.clone();
        let path = dir.join("taskq.log");
        let record = json!({"project": c.repo, "host":"mac", "stage":"review", "task":n,
                "attempt":a, "rt":"codex", "outcome":"unknown", "error":"left-board", "at_ms":1});
        crate::profile::tests::append_line(&path, &format!("taskq-stage {record}")).unwrap();
        if closed {
            b.comment(n, "taskq: close").unwrap();
            b.close(n).unwrap();
        } else {
            b.labels(n, &[], &["taskq".into()]).unwrap();
        }
        assert!(
            crate::profile::selftest(&dir, &c.repo, &vec![], 300)
                .unwrap()
                .contains("WARN"),
            "an ended attempt without a terminal record warns"
        );
        let mut s = service(&b, &c, Default::default());
        s.focus.clear();
        s.pass().unwrap();
        assert!(
            crate::profile::selftest(&dir, &c.repo, &vec![], 300)
                .unwrap()
                .contains("no overdue"),
            "ended attempt must have committed terminal evidence"
        );
        let repaired = crate::profile::tests::read_log(&dir).unwrap();
        s.pass().unwrap();
        assert_eq!(crate::profile::tests::read_log(&dir).unwrap(), repaired);
        assert!(
            !b.comments(n)
                .unwrap()
                .iter()
                .any(|c| c.body.starts_with("taskq: gone"))
        );
        drop(_guard);
        drop(_capture);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

// Chain: #1341 per-pass repair -> full-store Reader snapshot -> pinned WAL -> failed telemetry checkpoint.
// Risk: every project pass reopens the store, appends wait seconds and lose batches; a stale cache fabricates a repair.
#[test]
fn ended_attempt_repair_reads_store_once_per_interval() {
    let dir = std::env::temp_dir().join(format!("taskq-repair-read-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (_capture, _guard) = crate::profile::tests::collecting(&dir);
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
    let mut c = cfg("mac");
    c.install = dir.clone();
    c.root = dir.clone();
    crate::profile::open(&dir, &c.repo, "mac", n, a, "codex", false);
    let reads = || crate::profile::STORE_READS.with(|n| n.get());
    let before = reads();
    let mut s = service(&b, &c, Default::default());
    s.focus.clear();
    for _ in 0..3 {
        s.pass().unwrap();
    }
    assert_eq!(
        reads() - before,
        1,
        "open attempt passes reuse one store read"
    );
    // #1950: a close in `doing` is reopened; the attempt ends through a result and the reviewer's close
    b.comment(
        n,
        &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    b.comment(n, "taskq: close").unwrap();
    b.close(n).unwrap();
    s.pass().unwrap();
    assert_eq!(
        reads() - before,
        2,
        "a repair confirms the cached row with a fresh read"
    );
    crate::profile::selftest(&dir, &c.repo, &vec![], 300).expect("closed attempt repaired");
    let repaired = crate::profile::tests::read_log(&dir).unwrap();
    s.pass().unwrap();
    assert_eq!(reads() - before, 2);
    assert_eq!(
        crate::profile::tests::read_log(&dir).unwrap(),
        repaired,
        "repair is not repeated from the cache"
    );
    drop(_guard);
    drop(_capture);
    std::fs::remove_dir_all(dir).unwrap();
}

// Chain: §4 historical no-host review -> fresh ended-board proof -> one-pass repair -> selftest.
// Risk: #1252 stays blocked, or another project/host receives fabricated terminal evidence.
#[test]
fn historical_no_host_terminal_repair_requires_local_ownership() {
    let root = std::env::temp_dir().join(format!("taskq-no-host-{}", crate::runtime::uuid()));
    std::fs::create_dir(&root).unwrap();
    let child = coreq::process::output(
        "test.no-host-repair",
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "service::tests::historical_no_host_repair_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_NO_HOST_REPAIR_ROOT", &root)
            .env_remove("COREQ_TRACE_CONTEXT"),
        // #2515: waits for the child's exit on a stalled Windows runner; the limit only names a hang inside the 900 s
        // gate and outlasts the child's acknowledgment wait, so a lost acknowledgment reports itself.
        std::time::Duration::from_secs(300),
    )
    .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    // Native process exit releases every fixture handle before Windows removes the stores.
    std::fs::remove_dir_all(&root).unwrap();
    assert!(!root.exists(), "fixture cleanup must complete");
}

#[test]
#[ignore = "subprocess helper exercised by historical_no_host_terminal_repair_requires_local_ownership"]
fn historical_no_host_repair_child() {
    let root = PathBuf::from(std::env::var_os("TASKQ_NO_HOST_REPAIR_ROOT").unwrap());
    for closed in [true, false] {
        for evidence in [
            "local",
            "foreign-project",
            "foreign-host",
            "missing-log",
            "empty-log",
            "unreadable-log",
            "ignored-spawn",
            "untrusted-spawn",
            "runtime-mismatch",
            "unreadable-board",
            "unreadable-comments",
        ] {
            let dir = root.join(format!("{closed}-{evidence}"));
            std::fs::create_dir_all(dir.join(".taskq")).unwrap();
            let b = Fake::default();
            let n = b.create("T", "", &["taskq".into()]).unwrap();
            let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
            b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40)))
                .unwrap();
            let host = if evidence == "foreign-host" {
                "win"
            } else {
                "mac"
            };
            let spawn = format!("taskq: spawn reviewer codex:r {host}");
            let a = b.raw(
                n,
                &spawn,
                evidence != "untrusted-spawn",
                evidence == "ignored-spawn",
            );
            let mut c = cfg("mac");
            c.install = dir.clone();
            c.root = dir.clone();
            let path = dir.join("taskq.log");
            let (_capture, _guard) = crate::profile::tests::collecting(&dir);
            // This fixture proves repair ownership, not the collector's disk latency. #2478: one commit outlasted the
            // 3 s default; a flush returns on its acknowledgment, so this limit only names a store that never commits.
            #[cfg(windows)]
            _capture
                .acknowledge_within(std::time::Duration::from_secs(240))
                .unwrap();
            let project = if evidence == "foreign-project" {
                "other/project"
            } else {
                &c.repo
            };
            let rt = if evidence == "runtime-mismatch" {
                "claude"
            } else {
                "codex"
            };
            for (stage, outcome, error) in [
                ("claim", "ok", "none"),
                ("start", "ok", "none"),
                ("checks", "ok", "none"),
                ("close", "failed", "other"),
                ("review", "unknown", "left-board"),
            ] {
                let record = json!({"project":project, "stage":stage, "task":n, "attempt":a,
                        "rt":rt, "outcome":outcome, "error":error, "at_ms":1, "unknown":"no-usage"});
                crate::profile::tests::append_line(&path, &format!("taskq-stage {record}"))
                    .unwrap();
            }
            let attempt_log = crate::runtime::attempt_log(&dir, "reviewer", n, a);
            if evidence == "unreadable-log" {
                std::fs::create_dir(&attempt_log).unwrap();
            } else if evidence != "missing-log" {
                std::fs::write(
                    &attempt_log,
                    if evidence == "empty-log" {
                        ""
                    } else {
                        "original reviewer output\n"
                    },
                )
                .unwrap();
            }
            if closed {
                b.comment(n, &format!("taskq: close attempt {a}")).unwrap();
                b.close(n).unwrap();
            } else {
                b.labels(n, &[], &["taskq".into()]).unwrap();
            }
            if evidence == "unreadable-board" {
                b.state.borrow_mut().fail_issue_read = Some((n, 1));
            }
            if evidence == "unreadable-comments" {
                b.state.borrow_mut().fail_comment_reads = 1;
            }
            let history = crate::profile::tests::read_log(&dir).unwrap();
            assert!(
                crate::profile::selftest(&dir, project, &vec![], 300)
                    .unwrap()
                    .contains("WARN")
            );
            let mut s = service(&b, &c, Default::default());
            s.focus.clear();
            s.pass().unwrap();
            let repaired = crate::profile::tests::read_log(&dir).unwrap();
            assert!(
                repaired.starts_with(&history),
                "original history must survive"
            );
            if evidence == "local" {
                assert!(
                    crate::profile::selftest(&dir, project, &vec![], 300)
                        .unwrap()
                        .contains("no overdue"),
                    "{closed}: {repaired}"
                );
                let record: serde_json::Value = serde_json::from_str(
                    repaired
                        .lines()
                        .last()
                        .unwrap()
                        .strip_prefix("taskq-stage ")
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(record["host"], "mac");
                assert_eq!(record["stage"], "review");
                assert_eq!(record["outcome"], if closed { "ok" } else { "cancelled" });
                assert!(
                    record["us"].is_null()
                        && record["calls"].is_null()
                        && record["tokens"].is_null()
                );
                s.pass().unwrap();
                assert_eq!(
                    crate::profile::tests::read_log(&dir).unwrap(),
                    repaired,
                    "idempotent repair"
                );
            } else {
                assert_eq!(repaired, history, "{closed}: {evidence} must stay unknown");
                assert!(
                    crate::profile::selftest(&dir, project, &vec![], 300)
                        .unwrap()
                        .contains("WARN")
                );
            }
            if attempt_log.is_file() {
                assert_eq!(
                    std::fs::read_to_string(attempt_log).unwrap(),
                    if evidence == "empty-log" {
                        ""
                    } else {
                        "original reviewer output\n"
                    }
                );
            }
            assert!(
                !b.comments(n)
                    .unwrap()
                    .iter()
                    .any(|c| c.body.starts_with("taskq: gone"))
            );
            drop(s);
            drop(_guard);
            drop(_capture);
        }
    }
}

// Chain: § 7 worker exit to review. Risk: a silent exit is not gone; a result gets no reviewer.
#[test]
fn exit_without_result_is_gone_and_result_brings_a_reviewer() {
    for unknown_exit in [false, true] {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let c = cfg("mac");
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        b.comment(
            n,
            &core::entry(
                "result",
                &["0123456789abcdef0123456789abcdef01234567"],
                Some(a),
                "ok",
            ),
        )
        .unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(
            (t.state, t.role.as_deref(), t.retries),
            (State::Review, Some("reviewer"), 0)
        );
        let r = t.attempt.unwrap();
        *e.borrow()[1].1.borrow_mut() = Some(1);
        if unknown_exit {
            s.kids[0].proc = Box::new(UnknownExit);
        }
        s.pass().unwrap(); // reviewer exits without a verdict: gone keeps the result
        let t = task(&b, n);
        assert_eq!(
            (t.state, t.retries, t.result.is_some()),
            (State::Review, 1, true)
        );
        assert_ne!(t.attempt, Some(r));
        if unknown_exit {
            let log = entries(&b, n).unwrap();
            assert!(
                log.iter().any(|entry| core::kind(n, &entry.body) == "gone"
                    && entry.body.contains("exit unknown"))
            );
            assert!(
                core::executions(n, &log)
                    .iter()
                    .find(|x| x.id == r)
                    .unwrap()
                    .release
                    == Some(core::Release::Exit)
            );
        }
    }
}

// Chain: native terminal reviewer -> same-pass gone -> retained result and classified stage.
// Risk: native failure detail is lost between adapter and stage record.
#[test]
fn terminal_reviewer_records_native_error_and_releases_lease() {
    struct Terminal {
        root: PathBuf,
    }
    impl Proc for Terminal {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(Some(-1))
        }
        fn exit_error(&self) -> Option<&'static str> {
            let rows = crate::runtime::background_rows(
                &[serde_json::json!({"id":"abcdef12", "state":"failed"})],
                &self.root,
                &self.root,
            )
            .ok()?;
            crate::runtime::claude_failure(rows.first()?).ok()
        }
        fn kill(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:invented win")
        .unwrap();
    b.comment(
        n,
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(a),
            "ok",
        ),
    )
    .unwrap();
    release_execution(&b, n, a, a, "exit").unwrap();
    let c = cfg("win");
    let root =
        std::env::temp_dir().join(format!("taskq-native-failure-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(root.join("jobs/abcdef12")).unwrap();
    std::fs::write(
        root.join("jobs/abcdef12/state.json"),
        serde_json::json!({
            "cwd": root, "sessionId":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "state":"failed", "detail":"ENAMETOOLONG: name too long, uv_spawn"
        })
        .to_string(),
    )
    .unwrap();
    let mut s = Service::new(
        &b,
        &c,
        "4.4.0",
        Box::new(|_| Ok(Box::new(Terminal { root: root.clone() }))),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let r = task(&b, n).attempt.unwrap();
    crate::profile::STAGED.with(|v| v.borrow_mut().clear());
    s.focus.clear();
    s.pass().unwrap();
    assert!(s.kids.is_empty());
    assert!(
        b.comments(n)
            .unwrap()
            .iter()
            .any(|e| e.body.contains(&format!("taskq: gone attempt {r}")))
    );
    assert_eq!(
        (task(&b, n).state, task(&b, n).result.is_some()),
        (State::Review, true)
    );
    crate::profile::STAGED.with(|v| {
        assert!(
            v.borrow()
                .contains(&format!("review {n} {r} claude failed argument-too-long"))
        )
    });
    let failure = |state: &str, id: &str, project: &std::path::Path| {
        let rows = crate::runtime::background_rows(
            &[serde_json::json!({"id":id,"state":state,"cwd":project})],
            project,
            &root,
        )?;
        crate::runtime::claude_failure(&rows[0])
    };
    std::fs::write(
        root.join("jobs/abcdef12/state.json"),
        serde_json::json!({
        "cwd":root,"sessionId":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        "state":"failed","detail":"permission denied"})
        .to_string(),
    )
    .unwrap();
    assert_eq!(failure("failed", "abcdef12", &root), Ok("permission"));
    for detail in [None, Some(serde_json::Value::Null)] {
        let mut state = serde_json::json!({
            "cwd":root,"sessionId":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","state":"failed"
        });
        if let Some(detail) = detail {
            state["detail"] = detail;
        }
        std::fs::write(root.join("jobs/abcdef12/state.json"), state.to_string()).unwrap();
        assert!(
            failure("failed", "abcdef12", &root).is_err(),
            "absent native detail remains unknown"
        );
    }
    assert!(
        failure("stopped", "abcdef12", &root).is_err(),
        "mismatched native state remains unknown"
    );
    assert!(
        failure("failed", "../abcdef12", &root).is_err(),
        "invalid identity cannot read a locator"
    );
    assert!(
        failure("failed", "abcdef12", &root.join("jobs")).is_err(),
        "foreign cwd remains unknown"
    );
    std::fs::remove_file(root.join("jobs/abcdef12/state.json")).unwrap();
    assert!(
        failure("failed", "abcdef12", &root).is_err(),
        "missing failure evidence cannot become a generic native failure"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

// Chain: § 3 claim across hosts to § 2 retry limit (#913, #1123). Risk: two hosts claim one result's review, the
// loser writes more than its spawn, two reviewers run, or the limit parks the task on a card without options.
#[test]
fn lost_reviewer_claim_writes_nothing_more_and_the_gone_limit_asks_with_options() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let w = b
        .comment(
            n,
            &core::entry("spawn", &["worker", "claude:s", "linux"], None, ""),
        )
        .unwrap();
    b.comment(
        n,
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(w),
            "ok",
        ),
    )
    .unwrap();
    release_execution(&b, n, w, w, "exit").unwrap();
    let (cm, cw) = (cfg("mac"), cfg("win"));
    let (em, ew): (Exits, Exits) = Default::default();
    let (mut mac, mut win) = (service(&b, &cm, em.clone()), service(&b, &cw, ew.clone()));
    // mac reads the task in review before win's spawn lands: both hosts post a reviewer spawn for one result
    let stale = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300)
        .unwrap()
        .0
        .into_iter()
        .find(|v| v.issue.number == n)
        .unwrap();
    win.pass().unwrap();
    let r = task(&b, n).attempt.unwrap();
    let (labels, comments) = (b.issue(n).unwrap().labels, b.comments(n).unwrap().len());
    mac.claim(&stale, "reviewer", "claude", "").unwrap();
    // Fresh ownership read prevents even a losing spawn for the stale view.
    assert_eq!(
        (
            b.issue(n).unwrap().labels,
            b.comments(n).unwrap().len(),
            mac.kids.len(),
            em.borrow().len()
        ),
        (labels, comments, 0, 0)
    );
    assert_eq!(
        (task(&b, n).attempt, task(&b, n).state),
        (Some(r), State::Review)
    );
    // A3: later passes of both hosts start no second reviewer for this result
    mac.pass().unwrap();
    win.pass().unwrap();
    assert_eq!(
        (em.borrow().len(), ew.borrow().len(), task(&b, n).attempt),
        (0, 1, Some(r))
    );
    // the reviewer exits twice without a verdict: the 2nd gone asks with the limit card
    *ew.borrow()[0].1.borrow_mut() = Some(1);
    win.pass().unwrap();
    win.pass().unwrap(); // the gone task is left alone for the rest of its pass
    assert_eq!(
        (task(&b, n).state, task(&b, n).retries, ew.borrow().len()),
        (State::Review, 1, 2)
    );
    *ew.borrow()[1].1.borrow_mut() = Some(1);
    win.pass().unwrap();
    let t = task(&b, n);
    let card = t.card.clone().unwrap();
    assert_eq!((t.state, t.retries, t.attempt), (State::Ask, 2, None));
    assert!(
        card.starts_with("exit 1") && card.contains("Problem: retry limit 2 reached by this gone"),
        "{card}"
    );
    assert_eq!(
        core::options(&card)
            .into_iter()
            .map(|o| o.1.split(':').next().unwrap().to_string())
            .collect::<Vec<_>>(),
        ["requeue", "later"]
    );
    win.pass().unwrap(); // labels follow the fold on the next pass (§ 2)
    assert!(
        b.issue(n).unwrap().labels.contains(&"q-ask".to_string()),
        "{:?}",
        b.issue(n).unwrap().labels
    );
}

// Chain: § 2 retry limit after a failed start (#1123). Risk: a start failure whose comment read fails posts a
// gone without a fresh fold, so the limit parks the task on a card without options.
#[test]
fn start_failure_with_failed_read_posts_nothing_and_the_next_pass_asks_with_the_card() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let w = b
        .comment(
            n,
            &core::entry("spawn", &["worker", "claude:s", "linux"], None, ""),
        )
        .unwrap();
    b.comment(n, &core::entry("gone", &[], Some(w), "exit 1"))
        .unwrap();
    release_execution(&b, n, w, w, "exit").unwrap();
    assert_eq!((task(&b, n).state, task(&b, n).retries), (State::Ready, 1));
    let c = cfg("mac");
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(|_: &Launch| {
            b.state.borrow_mut().fail_comment_reads = 1; // the read after the failed start fails
            Err(crate::runtime::StartError::io(
                std::io::Error::other("boom"),
                Some("claude"),
            ))
        }),
    );
    s.focus = vec!["all".into()];
    let comments = b.comments(n).unwrap().len();
    let e = s.pass().unwrap_err();
    assert!(e.contains("comment read failed"), "{e}");
    assert_eq!(
        (b.comments(n).unwrap().len(), task(&b, n).state),
        (comments + 1, State::Doing),
        "only the spawn"
    );
    s.pass().unwrap();
    let t = task(&b, n);
    let card = t.card.clone().unwrap();
    assert_eq!((t.state, t.retries), (State::Ask, 2));
    assert!(
        card.contains("confirmed NoStart")
            && card.contains("Problem: retry limit 2 reached by this gone"),
        "{card}"
    );
    assert_eq!(core::options(&card).len(), 2);
}

// Chain: #1903 claim readback keeps missing the spawn (cs#528). Risk: the service posts a new spawn on every pass.
#[test]
fn a_spawn_the_readback_misses_is_not_reposted_each_pass() {
    let c = cfg("mac");
    let b = Fake::default();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
    let spawns = || {
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|c| c.body.starts_with("taskq: spawn"))
            .count()
    };
    b.state.borrow_mut().hide_latest_comment = true;
    for _ in 0..3 {
        s.pass().unwrap();
    }
    b.state.borrow_mut().hide_latest_comment = false;
    assert_eq!(spawns(), 1, "{:?}", s.log);
    // once the readback shows it, the spawn is the attempt
    s.pass().unwrap();
    assert_eq!((spawns(), task(&b, n).state), (1, State::Doing));
}

// Chain: #1879 (pilotq#133) claim readback lacks the won spawn -> no start, child or attempt log -> named wait for one
// ttl -> released no-start -> gone -> ready; the repeat reaches the retry-limit card. Risk: the spawn stays doing /
// starting with `Run state unknown` until the PM requeues it, or a spawn whose start was reached is released.
#[test]
fn lost_spawn_waits_one_ttl_then_ends_gone_and_a_reached_start_stays_unknown() {
    let dir = std::env::temp_dir().join(format!("taskq-lost-spawn-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(dir.join(".taskq")).unwrap();
    let mut c = cfg("mac");
    c.root = dir.clone();
    let b = Fake::default();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let other = b.create("note", "", &[]).unwrap();
    let exits = Rc::new(RefCell::new(vec![]));
    let mut s = service(&b, &c, exits.clone());
    let wait = |s: &Service| {
        s.snapshot.as_ref().unwrap().json()["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["n"] == n)
            .map(|r| r["wait"].as_str().unwrap_or_default().to_string())
            .unwrap()
    };
    for round in 1..=2 {
        // the lag hides only the newest comment: the spawn, never the previous round's gone
        b.comment(other, "unrelated").unwrap();
        b.state.borrow_mut().hide_latest_comment = true;
        s.pass().unwrap();
        b.state.borrow_mut().hide_latest_comment = false;
        let a = task(&b, n)
            .attempt
            .unwrap_or_else(|| panic!("round {round}: no spawn won"));
        assert_eq!(
            (task(&b, n).state, exits.borrow().len(), s.kids.len()),
            (State::Doing, 0, 0),
            "round {round}: start never called"
        );
        s.pass().unwrap();
        assert!(
            s.execution_waits[&n].contains(&format!("spawn {a} not started on mac")),
            "round {round}: {:?}",
            s.execution_waits
        );
        assert_eq!(
            wait(&s),
            crate::snapshot::WAIT_TEXTS[2],
            "round {round}: named wait, not Run state unknown"
        );
        b.advance(c.ttl);
        s.pass().unwrap();
        let log = entries(&b, n).unwrap();
        assert_eq!(
            core::executions(n, &log)
                .iter()
                .find(|x| x.id == a)
                .unwrap()
                .release,
            Some(core::Release::NoStart),
            "round {round}"
        );
        let gone = b
            .comments(n)
            .unwrap()
            .into_iter()
            .find(|c| c.body.starts_with(&format!("taskq: gone attempt {a}")))
            .expect("gone");
        assert!(
            gone.body.contains(&format!("lost spawn {a}")),
            "round {round}: {}",
            gone.body
        );
        assert!(
            s.log
                .iter()
                .any(|l| l.starts_with(&format!("#{n} lost spawn {a}: no attempt log on mac"))),
            "round {round}: {:?}",
            s.log
        );
        let t = task(&b, n);
        if round == 1 {
            assert_eq!((t.state, t.retries, t.attempt), (State::Ready, 1, None));
        } else {
            assert_eq!((t.state, t.retries), (State::Ask, 2));
            assert!(
                t.card
                    .unwrap()
                    .contains("Problem: retry limit 2 reached by this gone")
            );
        }
    }
    // A start that reached its native call (attempt log written) stays unknown after ttl: never released as no-start.
    let b = Fake::default();
    let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(
            n,
            &core::entry("spawn", &["worker", "codex:-", "mac"], None, ""),
        )
        .unwrap();
    std::fs::write(crate::runtime::attempt_log(&dir, "worker", n, a), "").unwrap();
    let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
    b.advance(c.ttl);
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n).state, task(&b, n).attempt),
        (State::Doing, Some(a))
    );
    assert_eq!(
        core::executions(n, &entries(&b, n).unwrap())[0].release,
        None
    );
    assert!(
        s.execution_waits[&n].contains("termination unconfirmed"),
        "{:?}",
        s.execution_waits
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Chain: #1886 (pilotq#133) lost spawn on a retired host, then the PM requeues -> no host claims while it is unreleased;
// one run of that host releases it no-start once and another host claims. Risk: the task stays unclaimable until a
// retired host is restored by hand, a repeated run posts a second release, or a reached start is released.
#[test]
fn retired_host_run_releases_a_requeued_lost_spawn_and_a_reached_start_names_the_host_step() {
    let dir = std::env::temp_dir().join(format!("taskq-retired-spawn-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(dir.join(".taskq")).unwrap();
    let (mut mac, mut hermes) = (cfg("mac"), cfg("hermes"));
    (mac.root, hermes.root) = (dir.clone(), dir.clone());
    let b = Fake::default();
    let labels = [TASK_LABEL.to_string(), "host-mac".into()];
    let lost = b.create("lost", "", &labels).unwrap();
    let reached = b.create("reached", "", &labels).unwrap();
    let handed = b.create("handed", "", &labels).unwrap();
    let mut spawns = vec![];
    for n in [lost, reached, handed] {
        let a = b
            .comment(
                n,
                &core::entry("spawn", &["worker", "codex:-", "hermes"], None, ""),
            )
            .unwrap();
        // the child's own requeue proves its start like an attempt log does
        b.comment(
            n,
            &core::entry(
                "requeue",
                &[],
                (n == handed).then_some(a),
                "Reason: hermes retired",
            ),
        )
        .unwrap();
        spawns.push(a);
    }
    std::fs::write(
        crate::runtime::attempt_log(&dir, "worker", reached, spawns[1]),
        "",
    )
    .unwrap();
    let releases = |n| {
        entries(&b, n)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with("taskq: released"))
            .count()
    };
    let mut m = service(&b, &mac, Rc::new(RefCell::new(vec![])));
    m.pass().unwrap();
    for n in [lost, reached] {
        assert_eq!(
            (task(&b, n).state, task(&b, n).attempt),
            (State::Ready, None),
            "#{n}: unreleased execution blocks the claim"
        );
        assert!(m.execution_waits[&n].ends_with("termination unconfirmed; start the taskq service on hermes once, after a reboot if the run had started"), "{:?}", m.execution_waits);
    }
    let mut h = service(&b, &hermes, Rc::new(RefCell::new(vec![])));
    for _ in 0..2 {
        h.pass().unwrap();
        assert_eq!(
            core::executions(lost, &entries(&b, lost).unwrap())[0].release,
            Some(core::Release::NoStart)
        );
        assert_eq!(
            [lost, reached, handed].map(releases),
            [1, 0, 0],
            "one fenced release; a reached start stays unknown (R12)"
        );
    }
    assert!(
        h.log.iter().any(|l| l.starts_with(&format!(
            "#{lost} lost spawn {}: no attempt log on hermes",
            spawns[0]
        ))),
        "{:?}",
        h.log
    );
    assert!(
        h.execution_waits[&reached]
            .ends_with("termination unconfirmed; reboot hermes to prove exit"),
        "{:?}",
        h.execution_waits
    );
    m.pass().unwrap();
    let t = task(&b, lost);
    assert_eq!(
        (t.state, host_of(&t)),
        (State::Doing, Some("mac")),
        "claimable elsewhere"
    );
    assert_eq!(
        (task(&b, reached).attempt, task(&b, handed).attempt),
        (None, None)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Chain: § 4 Filter, worker runtime admission over tests/golden/admit.json. Risk: unpinned work stalls with free Codex, or fallback overrides an owner pin.
#[test]
fn any_runtime_admission_follows_golden_traces() {
    let traces: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../tests/golden/admit.json")).unwrap();
    let rts = |v: &serde_json::Value| -> Vec<(String, u32)> {
        v.as_object()
            .unwrap()
            .iter()
            .map(|(k, n)| (k.clone(), n.as_u64().unwrap() as u32))
            .collect()
    };
    let mut first_free_fails = false;
    for tr in &traces {
        let (limits, running, tasks) = (
            rts(&tr["limits"]),
            rts(&tr["running"]),
            tr["tasks"].as_u64().unwrap(),
        );
        let want: Vec<Option<String>> = serde_json::from_value(tr["expect"].clone()).unwrap();
        let b = Fake::default();
        // Running children hold doing tasks of this host.
        let held: Vec<(u64, u64, String)> = running
            .iter()
            .flat_map(|(rt, n)| (0..*n).map(move |_| rt.clone()))
            .map(|rt| {
                let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
                (
                    n,
                    b.comment(n, &format!("taskq: spawn worker {rt}:s mac"))
                        .unwrap(),
                    rt,
                )
            })
            .collect();
        let mut labels = vec![TASK_LABEL.to_string()];
        labels.extend(
            serde_json::from_value::<Vec<String>>(tr.get("labels").cloned().unwrap_or(json!([])))
                .unwrap(),
        );
        let ns: Vec<u64> = (0..tasks)
            .map(|_| b.create("T", "", &labels).unwrap())
            .collect();
        let mut c = cfg("mac");
        c.limits = limits.iter().cloned().collect();
        c.default_runtime = tr["default"]
            .as_str()
            .and_then(crate::runtime::RuntimeId::parse)
            .unwrap_or(crate::runtime::RuntimeId::Claude);
        let mut s = service(&b, &c, Default::default());
        for (task, attempt, rt) in held {
            s.kids.push(Kid {
                unknown: None,
                stopping: false,
                session_url: None,
                execution: attempt,
                task,
                attempt,
                rt,
                proc: Box::new(FakeProc(Rc::new(RefCell::new(None)))),
            });
        }
        s.pass().unwrap();
        let got: Vec<Option<String>> = ns
            .iter()
            .map(|&n| rt_of(&task(&b, n)).map(|(rt, _)| rt.to_string()))
            .collect();
        assert_eq!(got, want, "{}", tr["name"]);
        let ignored = s
            .log
            .iter()
            .filter(|l| l.contains("conflicting prefer-*"))
            .count();
        assert_eq!(
            ignored,
            tr["ignored"].as_u64().unwrap_or(0) as usize,
            "{}",
            tr["name"]
        );
        // The old rule: every task takes the first listed runtime with a free slot.
        let mut used: BTreeMap<String, u32> = running.iter().cloned().collect();
        let old: Vec<Option<String>> = (0..tasks)
            .map(|_| {
                limits
                    .iter()
                    .find(|(rt, n)| used.get(rt).copied().unwrap_or(0) < *n)
                    .map(|(rt, _)| {
                        *used.entry(rt.clone()).or_default() += 1;
                        rt.clone()
                    })
            })
            .collect();
        first_free_fails |= old != want;
    }
    assert!(
        first_free_fails,
        "the traces must reject the first-free rule"
    );
}

// Chain: §4 worker result to unpinned review. Risk: reviews stall on unavailable Claude or override explicit pins.
#[test]
fn reviewer_prefers_claude_and_falls_back_to_codex() {
    for (worker, claude, expected) in [
        ("codex", 1, Some("claude")),
        ("codex", 0, Some("codex")),
        ("claude", 1, Some("claude")),
        ("codex-app", 1, Some("claude")),
        ("codex-app", 0, Some("codex")),
    ] {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        // The worker preference is separate from the subsequent unpinned review preference.
        c.limits = BTreeMap::from([
            (worker.into(), 1),
            ("codex".into(), 1),
            ("claude".into(), claude.max(u32::from(worker == "claude"))),
        ]);
        c.has = vec!["codex-app".into()];
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.cfg.default_runtime = crate::runtime::RuntimeId::parse(worker).unwrap();
        s.pass().unwrap();
        let t = task(&b, n);
        if worker == "codex-app" && !cfg!(any(target_os = "macos", windows)) {
            assert_eq!(
                t.state,
                State::Ready,
                "§ 9: app workers are macOS and Windows only"
            );
            assert!(
                t.attempt.is_none() && e.borrow().is_empty(),
                "no app worker claimed or started"
            );
            continue;
        }
        assert_eq!(rt_of(&t).unwrap().0, worker);
        s.cfg.default_runtime = crate::runtime::RuntimeId::Claude;
        // A losing spawn must not change which runtime actually did the work.
        b.comment(
            n,
            &core::entry(
                "spawn",
                &[
                    "worker",
                    if worker == "claude" {
                        "codex:ignored"
                    } else {
                        "claude:ignored"
                    },
                    "mac",
                ],
                None,
                "",
            ),
        )
        .unwrap();
        b.comment(
            n,
            &core::entry(
                "result",
                &["0123456789abcdef0123456789abcdef01234567"],
                t.attempt,
                "ok",
            ),
        )
        .unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        for retry in 0..2 {
            s.pass().unwrap();
            let t = task(&b, n);
            assert_eq!(t.state, State::Review);
            assert_eq!(
                (t.role.as_deref() == Some("reviewer")).then(|| rt_of(&t).unwrap().0),
                expected,
                "{worker}, claude {claude}, retry {retry}"
            );
            if retry == 0 && expected.is_some() {
                *e.borrow().last().unwrap().1.borrow_mut() = Some(1);
                s.pass().unwrap(); // gone ends the reviewer; the next pass claims its replacement
                assert_eq!(task(&b, n).attempt, None);
            }
        }
    }
}

// A2-A4, R2/R9/R12: worker/reviewer admission, quota fallback and same-session resume; never edit capacity or override pins/grants.
#[test]
fn antigravity_admission_and_resume_keep_ownership() {
    for reviewer in [false, true] {
        for why in ["quota", "pin", "disabled", "grant"] {
            let b = Fake::default();
            let root =
                std::env::temp_dir().join(format!("taskq-antigravity-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(root.join(".taskq")).unwrap();
            let mut c = cfg("mac");
            c.root = root.clone();
            c.install = root.join("install");
            c.limits = BTreeMap::from([
                ("claude".into(), 1),
                ("codex".into(), 0),
                ("antigravity".into(), 1),
            ]);
            let mut labels = vec!["taskq".into()];
            if why == "pin" {
                labels.push("run-claude".into());
            }
            let n = b
                .create("Invented Antigravity admission", "", &labels)
                .unwrap();
            if reviewer {
                let a = b
                    .comment(n, "taskq: spawn worker codex:fixture mac")
                    .unwrap();
                b.comment(
                    n,
                    &core::entry("result", &[&"a".repeat(40)], Some(a), "fixture candidate"),
                )
                .unwrap();
                release_execution(&b, n, a, a, "exit").unwrap();
            }
            let launches = Rc::new(RefCell::new(vec![]));
            let calls = launches.clone();
            let exits: Exits = Default::default();
            let running = exits.clone();
            let mut s = Service::new(
                &b,
                &c,
                "fixture",
                Box::new(move |l| {
                    calls
                        .borrow_mut()
                        .push((l.rt.clone(), l.session.clone(), l.resume.is_some()));
                    let flag = Rc::new(RefCell::new(None));
                    running.borrow_mut().push((l.task, flag.clone()));
                    Ok(Box::new(FakeProc(flag)))
                }),
            );
            s.focus = vec!["all".into()];
            s.limited.insert("claude".into(), (i64::MAX, true));
            if why == "disabled" {
                s.cap
                    .as_mut()
                    .unwrap()
                    .limits
                    .insert("antigravity".into(), 0);
            }
            if why == "grant" {
                s.refused_access
                    .insert("antigravity".into(), "no operator grant".into());
            }
            let capacity = s.cap.as_ref().unwrap().limits.clone();
            for _ in 0..2 {
                s.pass().unwrap();
            }
            assert_eq!(
                s.cap.as_ref().unwrap().limits,
                capacity,
                "quota cannot rewrite operator capacity"
            );
            if why != "quota" {
                assert!(launches.borrow().is_empty(), "{reviewer} {why}");
                let row = s
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|r| r.n == n)
                    .unwrap()
                    .json();
                assert!(
                    row["admission"]["reason"]
                        .as_str()
                        .unwrap()
                        .contains(if why == "pin" {
                            "claude"
                        } else {
                            "antigravity"
                        })
                );
            } else {
                assert_eq!(
                    *launches.borrow(),
                    [("antigravity".into(), "-".into(), false)]
                );
                let t = task(&b, n);
                let a = t.attempt.unwrap();
                assert_eq!(
                    t.role.as_deref(),
                    Some(if reviewer { "reviewer" } else { "worker" })
                );
                let path = crate::runtime::attempt_log(&root, t.role.as_deref().unwrap(), n, a);
                std::fs::write(path, "{\"conversationId\":\"agy-owned\"}\n").unwrap();
                b.comment(n, &core::entry("ask", &[], Some(a), "fixture question"))
                    .unwrap();
                *exits.borrow()[0].1.borrow_mut() = Some(0);
                s.pass().unwrap();
                b.comment(n, "taskq: answer\ncontinue").unwrap();
                s.pass().unwrap();
                assert_eq!(
                    launches.borrow().last().unwrap(),
                    &("antigravity".into(), "agy-owned".into(), true)
                );
                assert_eq!(
                    launches.borrow().len(),
                    2,
                    "only one new turn for the answer"
                );
                assert_eq!(task(&b, n).attempt, Some(a));
            }
            drop(s);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

// Defect #2118 (R8/R12/R13, §4/§9): unavailable Claude stalls new work/review despite eligible Codex.
// Protection: use the actual admission/claim/snapshot chain; pins, grants, common gates and live ownership bind.
#[test]
fn fallback_2118_isolated_admission() {
    for reviewer in [false, true] {
        for why in [
            "disabled",
            "full",
            "quota",
            "login",
            "missing",
            "unknown-login",
            "both",
            "pin",
            "grant",
            "focus",
            "stop",
            "load",
            "gui",
            "gui-grant",
            "gui-ok",
            "codex-unknown",
            "unresolved",
        ] {
            let b = Fake::default();
            let root = std::env::temp_dir().join(format!("taskq-2118-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            let mut c = cfg("mac");
            c.root = root.clone();
            c.install = root.join("install");
            c.limits = BTreeMap::from([("claude".into(), 0), ("codex".into(), 1)]);
            let mut labels = vec!["taskq".into()];
            if why == "pin" {
                labels.push("run-claude".into());
            }
            if why.starts_with("gui") {
                labels.push("needs-gui".into());
            }
            let n = b
                .create("Invented isolated admission", "", &labels)
                .unwrap();
            if reviewer {
                let a = b
                    .comment(n, "taskq: spawn worker claude:invented mac")
                    .unwrap();
                b.comment(
                    n,
                    &core::entry("result", &[&"a".repeat(40)], Some(a), "fixture candidate"),
                )
                .unwrap();
                release_execution(&b, n, a, a, "exit").unwrap();
            }
            let exits: Exits = Default::default();
            let mut s = service(&b, &c, exits.clone());
            let mut probes = json!([{"runtime":"claude","login":"logged in"},{"runtime":"codex","login":"logged in"}]);
            match why {
                "quota" => {
                    s.limited.insert("claude".into(), (i64::MAX, true));
                }
                "login" => {
                    s.logged_out.insert("claude".into());
                }
                "unknown-login" => {
                    probes[0]["login"] = json!("unknown");
                    s.cap.as_mut().unwrap().limits.insert("claude".into(), 1);
                }
                "missing" => {
                    s.refused_access.insert(
                        "claude".into(),
                        "claude CLI not found on the service PATH".into(),
                    );
                }
                "both" => {
                    s.logged_out.insert("codex".into());
                }
                "grant" => {
                    s.refused_access
                        .insert("codex".into(), "no operator grant".into());
                }
                "focus" => s.focus.clear(),
                "codex-unknown" => {
                    probes[1]["login"] = json!("unknown");
                }
                "stop" => {
                    std::fs::create_dir_all(&c.install).unwrap();
                    std::fs::write(c.install.join("hold"), "fixture stop").unwrap();
                }
                "load" => {
                    s.reload(Ok(Capacity::fixed(c.limits.clone(), on())));
                }
                "gui-grant" | "gui-ok" => {
                    s.gui_probe = Ok(());
                    s.gui_checked = Some(std::time::Instant::now());
                    s.full
                        .insert(if why == "gui-ok" { "codex" } else { "claude" }.into());
                }
                "unresolved" => {
                    let a = b
                        .comment(
                            n,
                            &format!(
                                "taskq: spawn {} claude:invented elsewhere",
                                if reviewer { "reviewer" } else { "worker" }
                            ),
                        )
                        .unwrap();
                    b.comment(
                        n,
                        &core::entry("requeue", &[], None, "fixture unknown owner"),
                    )
                    .unwrap();
                    assert!(a > 0);
                }
                "full" => {
                    s.cap.as_mut().unwrap().limits.insert("claude".into(), 1);
                    let busy = b
                        .create("Invented active work", "", &["taskq".into()])
                        .unwrap();
                    let a = b
                        .comment(busy, "taskq: spawn worker claude:busy mac")
                        .unwrap();
                    s.kids.push(Kid {
                        unknown: None,
                        stopping: false,
                        session_url: None,
                        execution: a,
                        task: busy,
                        attempt: a,
                        rt: "claude".into(),
                        proc: Box::new(FakeProc(Rc::new(RefCell::new(None)))),
                    });
                }
                _ => {}
            }
            s.logged_out
                .extend(crate::service_run::login_holds(&probes));
            s.pass().unwrap();
            let blocked = [
                "both",
                "pin",
                "grant",
                "focus",
                "stop",
                "load",
                "gui",
                "gui-grant",
                "unresolved",
            ]
            .contains(&why);
            assert_eq!(
                exits.borrow().len(),
                usize::from(!blocked),
                "{reviewer} {why}"
            );
            if !blocked {
                let t = task(&b, n);
                assert_eq!(
                    rt_of(&t).unwrap().0,
                    if why == "unknown-login" {
                        "claude"
                    } else {
                        "codex"
                    },
                    "{reviewer} {why}"
                );
                assert_eq!(
                    t.role.as_deref(),
                    Some(if reviewer { "reviewer" } else { "worker" })
                );
                let spawn = b
                    .comments(n)
                    .unwrap()
                    .into_iter()
                    .find(|e| Some(e.id) == t.attempt)
                    .unwrap();
                if why == "unknown-login" {
                    assert!(
                        !spawn.body.contains("fallback:"),
                        "unknown login alone must not change runtime"
                    );
                } else {
                    assert!(
                        spawn.body.contains("fallback:") && spawn.body.contains("selected codex"),
                        "{}",
                        spawn.body
                    );
                }
                if why == "unknown-login" || why == "codex-unknown" {
                    assert!(
                        crate::runtime::login_lines(&probes)
                            .iter()
                            .any(|line| line.contains("login unknown")),
                        "probe must stay unknown"
                    );
                }
                let row = s
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|r| r.n == n)
                    .unwrap()
                    .json();
                assert_eq!(row["admission"]["code"], "admitted");
                assert!(row["admission"]["reason"].as_str().unwrap().contains(
                    if why == "unknown-login" {
                        "claimed on claude"
                    } else {
                        "selected codex"
                    }
                ));
                s.pass().unwrap();
                assert_eq!(task(&b, n).attempt, t.attempt, "no duplicate or migration");
                assert_eq!(exits.borrow().len(), 1);
                if why == "unknown-login" || why == "codex-unknown" {
                    if why == "codex-unknown" {
                        let path = crate::runtime::attempt_log(
                            &c.root,
                            t.role.as_deref().unwrap(),
                            n,
                            t.attempt.unwrap(),
                        );
                        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                        std::fs::write(path, "{\"type\":\"thread.started\",\"thread_id\":\"fixture-native-session\"}\n").unwrap();
                    }
                    b.comment(n, &core::entry("ask", &[], t.attempt, "fixture pause"))
                        .unwrap();
                    *exits.borrow()[0].1.borrow_mut() = Some(0);
                    s.pass().unwrap();
                    b.comment(n, &core::entry("answer", &[], None, "continue"))
                        .unwrap();
                    s.pass().unwrap();
                    assert_eq!(
                        task(&b, n).attempt,
                        t.attempt,
                        "unknown login must preserve the attempt on resume"
                    );
                    assert_eq!(
                        task(&b, n).session,
                        t.session,
                        "unknown login must preserve the session/runtime on resume"
                    );
                    assert_eq!(exits.borrow().len(), 2, "the ended turn resumes once");
                }
            } else if why == "both" || why == "grant" {
                let row = s
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|r| r.n == n)
                    .unwrap()
                    .json();
                let reason = row["admission"]["reason"].as_str().unwrap();
                assert!(
                    reason.contains("claude") && reason.contains("codex"),
                    "{reason}"
                );
                s.logged_out.clear();
                s.refused_access.clear();
                s.pass().unwrap();
                assert_eq!(
                    rt_of(&task(&b, n)).unwrap().0,
                    "codex",
                    "automatic re-evaluation"
                );
            }
            drop(s);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

// A7/R12: real service admission -> installed Codex command -> native identity/exit, without a model turn.
// Board and capacity are isolated; read existing grants/options/login, never write host policy or submit a prompt.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "installed Codex admission qualification; no prompt/model or live board"]
fn fallback_2118_native_admission_without_model() {
    use std::process::Stdio;
    let install = crate::platform::default_install();
    let host = crate::host::Policy::read(&install).unwrap();
    let agents = crate::runtime::agents(&install);
    assert!(
        agents
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["runtime"] == "codex" && r["login"] == "logged in"),
        "native proof requires a confirmed Codex login"
    );
    for reviewer in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "taskq-qualification-fallback-{}",
            crate::runtime::uuid()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut c = cfg("qualification");
        host.apply(&mut c); // preserve actual model/effort/permission options
        c.root = root.clone();
        c.install = root.join("install");
        c.external = true;
        c.claim = crate::host::ClaimMode::Any;
        c.default_runtime = crate::runtime::RuntimeId::Claude; // staged preference, not a host policy edit
        c.limits = BTreeMap::from([("claude".into(), 0), ("codex".into(), 1)]);
        let env = crate::runtime::Env {
            context: "",
            root: &root,
            exe: std::path::Path::new("taskq"),
            install: &install,
            permission_mode: &c.permission_mode,
            codex: &c.codex,
            delivery: crate::config::Delivery::AutoMerge,
            board_env: &[],
            jobs: None,
        };
        let b = Fake::default();
        let n = b
            .create("TaskQ Qualification fallback", "", &["taskq".into()])
            .unwrap();
        if reviewer {
            let a = b
                .comment(n, "taskq: spawn worker claude:qualification qualification")
                .unwrap();
            b.comment(
                n,
                &core::entry("result", &[&"a".repeat(40)], Some(a), "isolated candidate"),
            )
            .unwrap();
            release_execution(&b, n, a, a, "exit").unwrap();
        }
        let launches = std::cell::Cell::new(0);
        let mut s = Service::new(
            &b,
            &c,
            "qualification",
            Box::new(|l| {
                launches.set(launches.get() + 1);
                assert_eq!(l.rt, "codex");
                let access = crate::runtime::access(&env, &l.rt)?;
                let mut command = crate::runtime::command(&env, l, &access)?;
                let output = std::fs::File::create(root.join("native.log")).unwrap();
                command
                    .stdin(Stdio::piped())
                    .stdout(output)
                    .stderr(std::fs::File::create(root.join("native.stderr")).unwrap());
                // Keep stdin open in Child: launch the actual executor, stop before input/model submission.
                command
                    .spawn()
                    .map(|p| Box::new(p) as Box<dyn Proc>)
                    .map_err(|e| crate::runtime::StartError::io(e, Some("codex")))
            }),
        );
        s.focus = vec!["all".into()];
        s.logged_out = crate::service_run::login_holds(&agents);
        s.refused_access = crate::policy::report(&install, &root, &c.permission_mode, &c.codex)
            .into_iter()
            .filter_map(|e| e.refused.map(|r| (e.runtime, r)))
            .collect();
        for rt in crate::runtime::agent_runtimes() {
            if let Some(r) = crate::runtime::missing_cli(rt) {
                s.refused_access.insert(rt.to_string(), r);
            }
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.pass().unwrap();
            assert_eq!(
                s.kids.len(),
                1,
                "{:?}",
                s.snapshot
                    .as_ref()
                    .map(|s| s.tasks.iter().map(|r| r.json()).collect::<Vec<_>>())
            );
            let identity = s.kids[0].proc.native_identity().unwrap();
            assert_eq!(s.kids[0].proc.termination().unwrap(), None);
            let attempt = task(&b, n).attempt;
            s.pass().unwrap();
            assert_eq!(
                launches.get(),
                1,
                "live execution cannot migrate or duplicate"
            );
            assert_eq!(task(&b, n).attempt, attempt);
            let spawn = b
                .comments(n)
                .unwrap()
                .into_iter()
                .find(|e| Some(e.id) == attempt)
                .unwrap();
            assert!(spawn.body.contains("fallback:") && spawn.body.contains("selected codex"));
            let codex = agents
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["runtime"] == "codex")
                .unwrap();
            println!(
                "role={} version={} pid={} creation={:?} grant={} launches=1; {}",
                if reviewer { "reviewer" } else { "worker" },
                codex["version"],
                identity.pid,
                identity,
                crate::runtime::access(&env, "codex").unwrap().line(),
                spawn.body
            );
        }));
        // End this fixture's attempt first (R11), then stop only its owned native process, even after failure.
        b.comment(n, "taskq: later\nReason: qualification complete")
            .unwrap();
        for kid in &mut s.kids {
            let identity = kid.proc.native_identity().ok();
            kid.proc.kill().unwrap();
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while kid.proc.termination().unwrap().is_none() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                kid.proc.termination().unwrap().is_some(),
                "native exit unconfirmed"
            );
            if let Some(identity) = identity {
                assert_eq!(coreq::process::identity(identity.pid).unwrap(), None);
            }
            println!(
                "pid={} native exit confirmed; no prompt/model submitted",
                kid.proc.id()
            );
        }
        drop(s);
        assert!(
            std::fs::read(root.join("native.log"))
                .unwrap_or_default()
                .is_empty(),
            "no turn without input"
        );
        std::fs::remove_dir_all(root).unwrap();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}

// Chain: § 4 Filter, host.json `default_runtime` codex (#1668) -> worker and reviewer claims -> verdict -> close.
// Risk: a host without claude routes its reviews or unlabelled work to claude and waits forever, a full codex
// review moves to a free claude slot, or a codex review takes another path to close.
#[test]
fn codex_default_host_runs_work_and_review_on_codex() {
    for (labels, codex, worker, expected) in [
        (vec![], 1, "codex", Some("codex")),
        (vec!["run-claude".to_string()], 1, "claude", Some("claude")),
        (vec!["run-claude".to_string()], 0, "claude", Some("claude")),
    ] {
        let b = Fake::default();
        let n = b
            .create(
                "T",
                "",
                &[vec!["taskq".to_string()], labels.clone()].concat(),
            )
            .unwrap();
        let mut c = Config {
            default_runtime: crate::runtime::RuntimeId::Codex,
            ..cfg("fra")
        };
        c.limits = BTreeMap::from([("claude".into(), 1), ("codex".into(), codex)]);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(
            rt_of(&t).unwrap().0,
            worker,
            "{labels:?}: run-* wins, else the host default"
        );
        b.comment(
            n,
            &core::entry(
                "result",
                &["0123456789abcdef0123456789abcdef01234567"],
                t.attempt,
                "ok",
            ),
        )
        .unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(t.state, State::Review);
        assert_eq!(
            (t.role.as_deref() == Some("reviewer")).then(|| rt_of(&t).unwrap().0),
            expected,
            "{labels:?}, codex {codex}: claude stays free"
        );
        if expected.is_none() {
            continue;
        }
        let verdict = "Changed: codex review
Checks: Task fit A1 ok; merge-result CI ok
Open: none";
        b.comment(n, &core::entry("publish", &[], t.attempt, verdict))
            .unwrap();
        *e.borrow().last().unwrap().1.borrow_mut() = Some(0);
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Closed, "{labels:?}");
        let log = entries(&b, n).unwrap();
        assert!(log.iter().any(|e| core::kind(n, &e.body) == "close"
            && core::verdict(n, &log, core::entry_text(n, &e.body)).contains(verdict)));
    }
}

// Chain: § 2 answer and requeue on a live child. Risk: a new session on answer; requeue leaves the child running.
#[test]
fn answer_resumes_the_same_session_and_requeue_stops_the_child() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.root = std::env::temp_dir().join(format!("taskq-session-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(c.root.join(".taskq")).unwrap();
    let e: Exits = Default::default();
    let sessions = Rc::new(RefCell::new(Vec::new()));
    let seen = sessions.clone();
    let exits = e.clone();
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l| {
            seen.borrow_mut().push(l.session.clone());
            let flag = Rc::new(RefCell::new(None));
            exits.borrow_mut().push((l.task, flag.clone()));
            Ok(Box::new(FakeProc(flag)))
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    std::fs::write(
        c.root.join(format!(".taskq/T{n}-{a}.log")),
        "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"native-session\"}\n",
    )
    .unwrap();
    b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Ask);
    // free comments, the owner's or not, move nothing (§ 2)
    b.raw(n, "prefer z", true, false);
    b.raw(n, "do x", false, false);
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n).state, e.borrow().len()),
        (State::Ask, 1),
        "a free comment moves nothing"
    );
    b.comment(n, &core::entry("answer", &["2"], None, "y"))
        .unwrap();
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), 2, "resumed once");
    assert_eq!(
        sessions.borrow()[1],
        "native-session",
        "resume resolves the pre-launch alias"
    );
    let t = task(&b, n);
    assert_eq!((t.state, t.attempt), (State::Doing, Some(a)));
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), 2, "an answer is delivered once");
    b.comment(n, &core::entry("requeue", &[], None, "redo"))
        .unwrap();
    s.focus = vec![];
    s.pass().unwrap();
    assert_eq!(*e.borrow()[1].1.borrow(), Some(-9));
    assert!(s.kids.is_empty());
    std::fs::remove_dir_all(&c.root).unwrap();
}

// Chain: § 8 Access refusal. Risk: a refused runtime is claimed (phantom attempt, retries) or resumed, or its
// reason is missing from status.
#[test]
fn refused_access_claims_and_resumes_nothing() {
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 2)]);
    let n1 = b.create("A", "", &["taskq".into()]).unwrap();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let a1 = task(&b, n1).attempt.unwrap();
    b.comment(
        n1,
        &core::entry("ask", &[], Some(a1), "Which?\n1. x\n2. y ★"),
    )
    .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    s.refused_access.insert(
        "claude".into(),
        "workspace enforcement unknown for claude on windows".into(),
    );
    b.comment(n1, &core::entry("answer", &["1"], None, "x"))
        .unwrap();
    let n2 = b.create("B", "", &["taskq".into()]).unwrap();
    s.pass().unwrap();
    assert_eq!(
        (
            task(&b, n2).state,
            task(&b, n2).attempt,
            task(&b, n1).state,
            e.borrow().len()
        ),
        (State::Ready, None, State::Doing, 1)
    );
    assert!(
        !b.comments(n2)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with("taskq: spawn")),
        "no phantom attempt"
    );
    let cap = s.capacity_json();
    assert_eq!(
        (
            cap["reasons"][0]["code"].as_str(),
            cap["reasons"][0]["text"].as_str()
        ),
        (
            Some("access"),
            Some("claude: workspace enforcement unknown for claude on windows")
        )
    );
    s.refused_access.clear();
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n2).state, e.borrow().len()),
        (State::Doing, 3),
        "granted: resume and claim"
    );
}

// Chain: usage limit before work -> `usage-limit` -> held runtime -> reset -> admission (#1279). Risk: a
// limit spends the gone budget (#1276), another eligible runtime waits, the hold outlives its reset or hides
// from status, or a limit after work destroys its attempt/session instead of waiting for quota.
#[test]
fn usage_limit_holds_the_runtime_until_reset_and_keeps_retries() {
    let root = std::env::temp_dir().join(format!("taskq-usage-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let b = Fake::default();
    let mut c = Config {
        root: root.clone(),
        install: root.join("install"),
        ..cfg("mac")
    };
    c.limits = BTreeMap::from([("claude".to_string(), 1), ("codex".to_string(), 1)]);
    let reset = 1000;
    // the next launch's log: before-work limit, after-work limit, or plain work
    let next: Rc<RefCell<&str>> = Rc::new(RefCell::new("limit"));
    let e: Exits = Default::default();
    let (seen, r, kind) = (e.clone(), root.clone(), next.clone());
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            let limit = format!(
                "{{\"type\":\"assistant\",\"apiError\":\"usage_limit_reached\",\"quotaLimits\":{{\"status\":\"rejected\",\"resetsAt\":{reset}}}}}\n"
            );
            let work =
                "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\"}]}}\n";
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(
                &log,
                match *kind.borrow() {
                    "limit" => limit,
                    "late" => format!("{work}{limit}"),
                    _ => work.into(),
                },
            )
            .unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    let any = b.create("A", "", &["taskq".into()]).unwrap();
    s.pass().unwrap();
    assert!(
        task(&b, any).session.unwrap().starts_with("claude:"),
        "unlabeled work runs on claude"
    );
    let only = b
        .create("B", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    *next.borrow_mut() = "work";
    *e.borrow()[0].1.borrow_mut() = Some(1);
    s.pass().unwrap();
    let gone = b
        .comments(any)
        .unwrap()
        .into_iter()
        .rfind(|c| c.body.starts_with("taskq: usage-limit"))
        .unwrap()
        .body;
    assert!(
        gone.starts_with("taskq: usage-limit attempt ")
            && gone.contains("\nUsage limit: claude until 1970-01-01 00:16 UTC"),
        "{gone}"
    );
    assert_eq!(
        (task(&b, any).state, task(&b, any).retries),
        (State::Ready, 0),
        "A2: the gone budget stays"
    );
    assert_eq!(
        core::executions(any, &entries(&b, any).unwrap())
            .last()
            .unwrap()
            .usage_hold,
        Some((reset, true)),
        "release persists the deadline before outcome publication"
    );
    s.pass().unwrap();
    assert_eq!(
        (task(&b, any).state, e.borrow().len()),
        (State::Doing, 2),
        "§4: released unpinned work automatically falls back to Codex"
    );
    assert!(
        task(&b, any).session.unwrap().starts_with("codex:"),
        "A4: work for another runtime starts"
    );
    assert_eq!(
        (task(&b, only).state, e.borrow().len()),
        (State::Ready, 2),
        "a claude-only task waits for the reset"
    );
    let cap = s.capacity_json();
    assert!(
        cap["reasons"].as_array().unwrap().iter().any(
            |r| r["code"] == "usage-limit" && r["text"] == "claude: until 1970-01-01 00:16 UTC"
        ),
        "{cap}"
    );
    assert!(
        s.heartbeat_body()
            .contains("\nlimit claude until 1970-01-01T00:16:40Z")
    );
    let text = s.snapshot.as_ref().unwrap().text(&c, &Value::Null).unwrap();
    assert!(
        text.contains(" claude limit until 1970-01-01 00:16 UTC"),
        "A3: {text}"
    );

    b.state.borrow_mut().clock = reset;
    *next.borrow_mut() = "late";
    s.pass().unwrap();
    assert_eq!(
        (task(&b, only).state, e.borrow().len()),
        (State::Doing, 3),
        "A5: the reset admits the held work"
    );
    assert!(s.limited.is_empty());
    *e.borrow()[2].1.borrow_mut() = Some(1);
    s.pass().unwrap();
    let kept = task(&b, only);
    assert_eq!(
        (kept.state, kept.retries),
        (State::Doing, 0),
        "#2137 A7: mid-work quota keeps work and its attempt"
    );
    assert!(
        b.comments(only)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with("taskq: steer quota attempt")
                && c.body.contains("same task and native session"))
    );
    assert!(
        !b.comments(only)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with("taskq: gone")),
        "quota is not a code defect"
    );
    assert!(
        crate::runtime::accounts::Host::read(&c.install)
            .active("claude", reset)
            .is_none(),
        "expired observed reset uses a bounded unknown-reset probe"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: §3 Runtime accounts (#2043 A4, A6): usage limit before work on the active account -> `usage-limit` with
// `next account` -> respawn on the next enabled account, retries kept -> every account limited -> the § 4 hold until
// the earliest end -> reset returns the first account -> all disabled admits nothing. Risk: a spare account waits
// behind a hold, a failover spends a retry, a switch leaves no log line, or a disabled list still starts work.
#[test]
fn usage_limit_fails_over_to_the_next_account_then_holds() {
    let root = std::env::temp_dir().join(format!("taskq-account-failover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    let list = |a: bool, w: bool| {
        std::fs::write(install.join(crate::runtime::accounts::FILE),
            format!("{{\"claude\":[{{\"label\":\"default\",\"enabled\":{a}}},{{\"label\":\"work\",\"enabled\":{w}}}]}}")).unwrap()
    };
    list(true, true);
    let b = Fake::default();
    let mut c = Config {
        root: root.clone(),
        install: install.clone(),
        ..cfg("mac")
    };
    c.limits = BTreeMap::from([("claude".to_string(), 1)]);
    let reset = 1000;
    let e: Exits = Default::default();
    let (seen, r, inst) = (e.clone(), root.clone(), install.clone());
    let clock = &b;
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            // the runtime adapter's start line (spawn_inner) before the native output: the account a new start takes
            let account = crate::runtime::accounts::Host::read(&inst)
                .active("claude", clock.now().unwrap())
                .unwrap();
            let limit = format!(
                "{{\"type\":\"assistant\",\"apiError\":\"usage_limit_reached\",\"quotaLimits\":{{\"status\":\"rejected\",\"resetsAt\":{reset}}}}}\n"
            );
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(
                &log,
                format!("{{\"type\":\"taskq.account\",\"account\":\"{account}\"}}\n{limit}"),
            )
            .unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    let n = b
        .create("A", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), 1);
    *e.borrow()[0].1.borrow_mut() = Some(1);
    s.pass().unwrap();
    let gone = b
        .comments(n)
        .unwrap()
        .into_iter()
        .rfind(|c| c.body.starts_with("taskq: usage-limit"))
        .unwrap()
        .body;
    assert!(
        gone.contains("\nUsage limit: claude until 1970-01-01 00:16 UTC; next account work"),
        "{gone}"
    );
    assert!(s.limited.is_empty(), "a spare account holds nothing");
    if e.borrow().len() < 2 {
        s.pass().unwrap();
    }
    assert_eq!(
        (e.borrow().len(), task(&b, n).state, task(&b, n).retries),
        (2, State::Doing, 0),
        "respawn on the next account, retries kept"
    );
    let log = |i: u64| crate::runtime::attempt_log(&root, "worker", n, i);
    let attempts: Vec<u64> = core::executions(n, &entries(&b, n).unwrap())
        .iter()
        .map(|x| x.attempt)
        .collect();
    assert_eq!(
        crate::runtime::accounts::of_log(&log(*attempts.last().unwrap())),
        "work"
    );
    assert!(
        s.log
            .iter()
            .any(|l| l == "account claude default -> work (limit)"),
        "{:?}",
        s.log
    );
    assert!(
        s.heartbeat_body()
            .contains("\naccount claude default on limited "),
        "{}",
        s.heartbeat_body()
    );
    assert!(
        s.heartbeat_body()
            .contains("\naccount claude work on active\n")
    );

    *e.borrow()[1].1.borrow_mut() = Some(1);
    s.pass().unwrap();
    let gone = b
        .comments(n)
        .unwrap()
        .into_iter()
        .rfind(|c| c.body.starts_with("taskq: usage-limit"))
        .unwrap()
        .body;
    assert!(
        gone.ends_with("Usage limit: claude until 1970-01-01 00:16 UTC")
            || gone.contains("until 1970-01-01 00:16 UTC\n"),
        "none left: {gone}"
    );
    assert!(!gone.contains("next account"), "{gone}");
    assert_eq!(
        s.limited.get("claude"),
        Some(&(reset, true)),
        "every account limited: the usage-limit hold"
    );
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n).state, e.borrow().len(), task(&b, n).retries),
        (State::Ready, 2, 0),
        "the hold keeps the task ready"
    );
    assert!(
        s.log
            .iter()
            .any(|l| l == "account claude work -> none (limit)"),
        "{:?}",
        s.log
    );

    b.state.borrow_mut().clock = reset;
    s.pass().unwrap();
    assert!(
        s.log
            .iter()
            .any(|l| l == "account claude none -> default (reset)"),
        "{:?}",
        s.log
    );
    assert_eq!(e.borrow().len(), 3, "the reset admits the work again");
    assert!(s.limited.is_empty());

    list(false, false);
    let cap = s.capacity_json();
    assert!(
        cap["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "capability"
                && r["text"] == "claude: every account disabled on this host"),
        "{cap}"
    );
    assert!(
        s.admit_runtime("claude", false)
            .is_err_and(|e| e.0 == "capability")
    );
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: #1446 A1/A2 Codex exit with a rejected refresh token -> `usage-limit` release -> login off at once.
// Risk: a dead login reads logged in, each admitted task burns its gone budget, or claude is held too.
#[test]
fn rejected_codex_login_releases_without_retry_and_holds_codex() {
    let root = std::env::temp_dir().join(format!("taskq-login-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let b = Fake::default();
    let mut c = Config {
        root: root.clone(),
        install: root.join("install"),
        ..cfg("mac")
    };
    c.limits = BTreeMap::from([("claude".to_string(), 1), ("codex".to_string(), 1)]);
    let e: Exits = Default::default();
    let (seen, r) = (e.clone(), root.clone());
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n2026-10-21T08:00:00.000000Z ERROR codex_login::auth::manager: Failed to refresh token: Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.\n").unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    let n = b
        .create("A", "", &["taskq".into(), "run-codex".into()])
        .unwrap();
    s.pass().unwrap();
    assert!(task(&b, n).session.unwrap().starts_with("codex:"));
    *e.borrow()[0].1.borrow_mut() = Some(1);
    s.pass().unwrap();
    let gone = b
        .comments(n)
        .unwrap()
        .into_iter()
        .rfind(|c| c.body.starts_with("taskq: usage-limit"))
        .unwrap()
        .body;
    assert!(
        gone.contains("\nLogin off: codex refresh token rejected; run `codex login --device-auth`"),
        "{gone}"
    );
    assert_eq!(
        (task(&b, n).state, task(&b, n).retries, e.borrow().len()),
        (State::Ready, 0, 1),
        "A2: released, no retry, not readmitted"
    );
    assert!(
        s.logged_out.contains("codex") && !s.logged_out.contains("claude"),
        "{:?}",
        s.logged_out
    );
    assert!(
        s.heartbeat_body().contains("\nlogin codex off"),
        "A1: at once, not at the next probe"
    );
    let rows = crate::runtime::agents(&c.install);
    if let Some(r) = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["runtime"] == "codex" && r["path"].is_string() && r["reason"].is_string())
    {
        assert_eq!(
            r["login"], "not logged in",
            "A2: taskq agents reads the mark"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: #1843 Codex worker exit 1 on "model is at capacity" mid-work -> continuing `steer` -> same session resumes (§ 4 step 1).
// Risk: a transient model failure loses the session as `gone`, or continuations loop past their bound without a retry.
#[test]
fn transient_model_failure_continues_the_same_session_twice() {
    let root = std::env::temp_dir().join(format!("taskq-transient-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let b = Fake::default();
    let mut c = Config {
        root: root.clone(),
        install: root.join("install"),
        ..cfg("mac")
    };
    c.limits = BTreeMap::from([("codex".to_string(), 1)]);
    let e: Exits = Default::default();
    let resumes: Rc<RefCell<Vec<(String, Option<String>)>>> = Default::default();
    let (seen, r, rs) = (e.clone(), root.clone(), resumes.clone());
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            rs.borrow_mut().push((l.session.clone(), l.resume.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"t1\"}
{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\"}}
{\"type\":\"turn.failed\",\"error\":{\"message\":\"Selected model is at capacity. Please try a different model.\"}}
").unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    let n = b
        .create("A", "", &["taskq".into(), "run-codex".into()])
        .unwrap();
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    for k in 0..2 {
        *e.borrow()[k].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        let steers: Vec<_> = b
            .comments(n)
            .unwrap()
            .into_iter()
            .filter(|c| {
                c.body.starts_with(&format!(
                    "taskq: steer attempt {a}
"
                ))
            })
            .collect();
        assert_eq!(steers.len(), k + 1, "one continuing steer per exit");
        assert!(
            steers[k].body.contains("Continue the task")
                && steers[k].body.contains("model is at capacity"),
            "{}",
            steers[k].body
        );
        assert_eq!(
            (task(&b, n).state, task(&b, n).attempt, task(&b, n).retries),
            (State::Doing, Some(a), 0),
            "the attempt stays live, no retry spent"
        );
        s.pass().unwrap();
        let (session, resume) = resumes.borrow()[k + 1].clone();
        assert_eq!(session, "t1", "the same Codex session resumes");
        assert!(
            resume.is_some_and(|r| r.contains("Continue the task")),
            "the steer reaches the session"
        );
    }
    *e.borrow()[2].1.borrow_mut() = Some(1);
    s.pass().unwrap();
    assert!(
        b.comments(n)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with(&format!(
                "taskq: gone attempt {a}
"
            ))),
        "a third failure ends the attempt"
    );
    assert_eq!((task(&b, n).state, task(&b, n).retries), (State::Ready, 1));
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: #2198 Claude worker exit 0 while its own `run_in_background` check runs -> continuing `steer` ->
// same session resumes (§ 4 step 1). Risk: the attempt goes `gone` as in #2176 and #2180, a retry is spent,
// or a worker that keeps backgrounding loops past the bound.
#[test]
fn claude_turn_ending_with_its_own_background_check_resumes_instead_of_gone() {
    let root = std::env::temp_dir().join(format!("taskq-background-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let b = Fake::default();
    let mut c = Config {
        root: root.clone(),
        install: root.join("install"),
        ..cfg("mac")
    };
    c.limits = BTreeMap::from([("claude".to_string(), 1)]);
    let e: Exits = Default::default();
    let resumes: Rc<RefCell<Vec<(String, Option<String>)>>> = Default::default();
    let (seen, r, rs) = (e.clone(), root.clone(), resumes.clone());
    let mut s = Service::new(
        &b,
        &c,
        "4.0.0",
        Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            rs.borrow_mut().push((l.session.clone(), l.resume.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, r#"{"type":"taskq.rules","phase":"start"}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo ghp_aaaaaaaaaaaaaaaaaaaaaaaa","description":"Full check","run_in_background":true}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"Command running in background with ID: b1. Output is being written to: out"}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"echo ghp_bbbbbbbbbbbbbbbbbbbbbbbb","run_in_background":true}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_2","content":"Command running in background with ID: b2. Output is being written to: out"}]}}
{"type":"result","subtype":"success","is_error":false}
"#).unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }),
    );
    s.focus = vec!["all".into()];
    let n = b
        .create("A", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    let first = resumes.borrow()[0].0.clone();
    for k in 0..3 {
        *e.borrow()[k].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let steers: Vec<_> = b
            .comments(n)
            .unwrap()
            .into_iter()
            .filter(|c| c.body.starts_with(&format!("taskq: steer attempt {a}\n")))
            .collect();
        assert_eq!(steers.len(), k + 1, "one continuing steer per exit");
        assert!(
            !steers[k].body.contains("ghp_"),
            "invented command tokens must never reach the board"
        );
        assert!(
            !steers[k].body.contains("echo"),
            "raw commands must never reach the board"
        );
        assert!(
            steers[k].body.contains(BACKGROUND_STEER)
                && steers[k].body.contains("Bash")
                && steers[k].body.contains("foreground"),
            "{}",
            steers[k].body
        );
        assert_eq!(
            (task(&b, n).state, task(&b, n).attempt, task(&b, n).retries),
            (State::Doing, Some(a), 0),
            "the attempt stays live, no retry spent"
        );
        s.pass().unwrap();
        let (session, resume) = resumes.borrow()[k + 1].clone();
        assert_eq!(session, first, "the same Claude session resumes");
        assert!(
            resume.is_some_and(|r| r.contains(BACKGROUND_STEER)),
            "the steer reaches the session"
        );
    }
    *e.borrow()[3].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    assert!(
        b.comments(n)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with(&format!("taskq: gone attempt {a}\n"))),
        "a fourth such exit ends the attempt"
    );
    assert_eq!((task(&b, n).state, task(&b, n).retries), (State::Ready, 1));
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: #1429 A2 runtime login. Risk: a logged-out runtime takes tasks that fail with 401, or its hold is silent.
#[test]
fn logged_out_runtime_admits_nothing_and_says_how_to_log_in() {
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 1), ("codex".to_string(), 1)]);
    let mut s = service(&b, &c, Default::default());
    s.logged_out = BTreeSet::from(["claude".to_string()]);
    let only = b
        .create("A", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    let other = b
        .create("B", "", &["taskq".into(), "run-codex".into()])
        .unwrap();
    s.pass().unwrap();
    assert_eq!(
        task(&b, only).state,
        State::Ready,
        "a claude-only task waits for the login"
    );
    assert!(
        task(&b, other).session.unwrap().starts_with("codex:"),
        "a logged-in runtime keeps working"
    );
    let cap = s.capacity_json();
    assert!(
        cap["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "login"
                && r["text"]
                    .as_str()
                    .unwrap()
                    .contains("claude: not logged in; run `claude auth login`")),
        "{cap}"
    );
    assert!(s.heartbeat_body().contains("\nlogin claude off"));
    s.logged_out.clear();
    s.pass().unwrap();
    assert_eq!(
        task(&b, only).state,
        State::Doing,
        "a probe that reads it logged in admits the task"
    );
    assert!(!s.heartbeat_body().contains("login claude"));
}

// Chain: #2094 A6 runtime probe rows -> heartbeat `cli` lines. Risk: a host's CLI versions stay invisible on the board.
#[test]
fn heartbeat_names_cli_versions() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.clis = crate::runtime::clis_versions(&json!([
        {"runtime": "claude", "path": "/bin/claude", "version": "2.1.300"},
        {"runtime": "codex", "path": "/bin/codex", "version": null},
        {"runtime": "codex-app", "path": "/bin/codex", "version": null},
    ]));
    let body = s.heartbeat_body();
    assert!(
        body.contains("\ncli claude 2.1.300\ncli codex unknown"),
        "{body}"
    );
    assert!(
        crate::runtime::clis_versions(
            &json!([{"runtime": "claude", "path": null, "version": null}])
        )
        .is_empty(),
        "not installed: no line"
    );
}

// A2/§4: a crash after release must not turn a quota refusal into a retry, locally or remotely.
#[test]
fn usage_limit_release_before_recovery() {
    for host in ["mac", "other"] {
        for role in ["worker", "reviewer"] {
            let b = Fake::default();
            // #2137: quota failures are host-wide; each simulated host needs its own writable store.
            let mut c = cfg("mac");
            c.install = std::env::temp_dir()
                .join(format!("taskq-recovered-quota-{}", crate::runtime::uuid()));
            let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
            let w = b
                .comment(n, &format!("taskq: spawn worker claude:s {host}"))
                .unwrap();
            let a = if role == "reviewer" {
                release_execution(&b, n, w, w, "exit").unwrap();
                b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40)))
                    .unwrap();
                b.comment(n, &format!("taskq: spawn reviewer claude:r {host}"))
                    .unwrap()
            } else {
                w
            };
            b.comment(
                n,
                &format!("taskq: released {a} exit attempt {a}\nusage-limit until 1000"),
            )
            .unwrap();
            let mut s = service(&b, &c, Default::default());
            s.focus = vec!["off".into()];
            s.pass().unwrap();
            let t = task(&b, n);
            assert_eq!(t.retries, 0, "{host}/{role}: recovery spent retries");
            assert_eq!(t.attempt, None);
            assert_eq!(
                t.state,
                if role == "reviewer" {
                    State::Review
                } else {
                    State::Ready
                }
            );
            assert_eq!(s.limited.contains_key("claude"), host == "mac");
            assert!(
                b.comments(n)
                    .unwrap()
                    .iter()
                    .any(|c| c.body.starts_with("taskq: usage-limit attempt"))
            );
            s.pass().unwrap();
            assert_eq!(
                b.comments(n)
                    .unwrap()
                    .iter()
                    .filter(|c| c.body.starts_with("taskq: usage-limit attempt"))
                    .count(),
                1
            );
            let _ = std::fs::remove_dir_all(&c.install);
        }
    }
}

// R11/E2: offline Full stop uses owned handles; Drain never cancels and exit must be observed.
#[test]
fn full_stop_cancels_owned_work_before_board_access() {
    let b = Fake::default();
    let dir = std::env::temp_dir().join(format!("taskq-full-stop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = cfg("mac");
    c.install = dir.clone();
    c.root = dir.clone();
    let n = b.create("owned", "", &["taskq".into()]).unwrap();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let attempt = task(&b, n).attempt.unwrap();
    let mut hold = json!({"taskq_stop":{"id":"stop1","instance":"owner1","roots":[dir],"issued_at":1,"expires_at":301,"action":"stop"}});
    std::fs::write(dir.join("hold"), hold.to_string()).unwrap();
    s.reduced().unwrap();
    assert_eq!(
        *e.borrow()[0].1.borrow(),
        None,
        "Drain preserves current work"
    );
    hold["taskq_stop"]["mode"] = json!("full");
    std::fs::write(dir.join("hold"), hold.to_string()).unwrap();
    b.state.borrow_mut().fail_comments = 100;
    let _ = s.reduced();
    assert_eq!(
        *e.borrow()[0].1.borrow(),
        Some(-9),
        "Full stop must cancel before a failing board write"
    );
    assert_eq!(s.kids.len(), 1, "offline release retains native ownership");
    assert_eq!(
        task(&b, n).attempt,
        Some(attempt),
        "no fabricated board retirement"
    );
    assert!(dir.join("hold").is_file());
    std::fs::remove_dir_all(dir).unwrap();
}

// Chain: § 3 emergency hold, § 4 reconciliation and recovery. Risk: a held host launches, an unknown read is
// marked gone, or a recovery op is applied twice or over a newer revision.
#[test]
fn emergency_hold_refuses_launches_and_recovery_fences_attempts() {
    use crate::dispatch::{proposal, recover};
    let b = Fake::default();
    let dir = std::env::temp_dir().join(format!("taskq-hold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = cfg("mac");
    (c.install, c.root, c.limits) = (
        dir.clone(),
        dir.clone(),
        BTreeMap::from([("claude".to_string(), 3)]),
    );
    let (n1, n2) = (
        b.create("A", "", &["taskq".into()]).unwrap(),
        b.create("B", "", &["taskq".into()]).unwrap(),
    );
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let (a1, a2) = (task(&b, n1).attempt.unwrap(), task(&b, n2).attempt.unwrap());
    // hold: a new task is not claimed, an answer is not resumed; children keep running
    std::fs::write(dir.join("hold"), "drill\n").unwrap();
    let n3 = b.create("C", "", &["taskq".into()]).unwrap();
    *e.borrow()[1].1.borrow_mut() = Some(0);
    b.comment(
        n2,
        &core::entry("ask", &[], Some(a2), "Which?\n1. x\n2. y ★"),
    )
    .unwrap();
    s.pass().unwrap();
    b.comment(n2, &core::entry("answer", &["1"], None, "x"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(-999); // n1's liveness read fails
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n3).state, task(&b, n2).state, e.borrow().len()),
        (State::Ready, State::Doing, 2),
        "held: nothing launched"
    );
    let cap = s.capacity_json();
    assert_eq!(
        (
            cap["gate"].as_str(),
            cap["reasons"][0]["code"].as_str(),
            cap["reasons"][0]["text"].as_str()
        ),
        (Some("held"), Some("hold"), Some("drill"))
    );
    // reconciliation: an unknown read keeps the child and the attempt; no gone, no op
    assert_eq!(
        (task(&b, n1).state, task(&b, n1).attempt, s.kids.len()),
        (State::Doing, Some(a1), 1)
    );
    let kids = |s: &Service| {
        Ok(s.kids.iter().map(|k| json!({"attempt": k.attempt, "pid": 1, "live": if k.unknown.is_some() { "unknown" } else { "running" }, "reason": k.unknown.as_ref()})).collect())
    };
    let rows = proposal(&b, "mac", &kids(&s)).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| (r.task, r.live.as_str(), r.revision))
            .collect::<Vec<_>>(),
        [
            (n1, "unknown (read failed)", None),
            (
                n2,
                "no child",
                Some(core::revision(n2, &entries(&b, n2).unwrap()))
            )
        ]
    );
    assert!(
        proposal(&b, "mac", &Err("status.json 1m ago".into()))
            .unwrap()
            .iter()
            .all(|r| r.revision.is_none()),
        "stale status: all unknown"
    );
    // stale recovery: the worker writes after the proposal; the op is refused and the board is unchanged
    let before = b.comments(n2).unwrap().len();
    b.comment(
        n2,
        &core::entry("ask", &[], Some(a2), "Again?\n1. x\n2. y ★"),
    )
    .unwrap();
    let r = recover(&b, &dir, &rows);
    assert_eq!(
        (r.len(), r[0].outcome, b.comments(n2).unwrap().len()),
        (1, "stale", before + 1)
    );
    // a fresh proposal once n1 reads running: one receipt per op, a repeated apply writes nothing more
    *e.borrow()[0].1.borrow_mut() = None;
    s.pass().unwrap();
    let rows = proposal(&b, "mac", &kids(&s)).unwrap();
    let r = recover(&b, &dir, &rows);
    assert_eq!(
        r.iter()
            .map(|r| (r.task, r.outcome, r.state))
            .collect::<Vec<_>>(),
        [(n1, "accepted", Some("ready"))],
        "n2 asks: not interrupted"
    );
    let count = b.comments(n1).unwrap().len();
    assert!(
        recover(&b, &dir, &rows).iter().all(|r| r.ok()) && b.comments(n1).unwrap().len() == count,
        "retried op ids"
    );
    // the fenced child is stopped (R11); still held, nothing relaunches
    s.pass().unwrap();
    assert_eq!(
        (*e.borrow()[0].1.borrow(), s.kids.len(), e.borrow().len()),
        (Some(-9), 0, 2)
    );
    // release: admission resumes
    std::fs::remove_file(dir.join("hold")).unwrap();
    s.pass().unwrap();
    assert!(task(&b, n1).attempt.is_some_and(|a| a != a1) && task(&b, n3).state == State::Doing);
    let _ = std::fs::remove_dir_all(&dir);
}

// A5: reopening host admission advances both pending start and review without a focus rewrite.
#[test]
fn admission_reopen_advances_pending_start_and_review() {
    for review in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "taskq-admission-progress-{}-{review}",
            std::process::id()
        ));
        std::fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        std::fs::write(crate::paths::dot(&root).join("focus"), "all").unwrap();
        let b = Fake::default();
        let n = b.create("Pending", "", &[TASK_LABEL.into()]).unwrap();
        if review {
            let a = b
                .comment(n, "taskq: spawn worker codex:fixture invented")
                .unwrap();
            b.comment(
                n,
                &core::entry(
                    "result",
                    &["0123456789abcdef0123456789abcdef01234567"],
                    Some(a),
                    "candidate",
                ),
            )
            .unwrap();
            release_execution(&b, n, a, a, "exit").unwrap();
        }
        let p = crate::host::Policy::parse(
            json!({"limits":{"claude":2},"load_gate":{"enabled":false}}),
            "invented",
        )
        .unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err(
            "handoff: legacy service still running".into(),
        ))));
        let mut c = cfg("invented");
        c.root = root.clone();
        c.install = root.clone();
        let exits: Exits = Default::default();
        let mut s = service(&b, &c, exits.clone());
        s.host = Some(host.clone());
        s.reload(Ok(p.cap.clone()));
        s.pass().unwrap();
        assert!(
            exits.borrow().is_empty(),
            "held admission cannot claim pending work"
        );
        let paths = [
            crate::paths::dot(&root).join("poke"),
            root.join("host-poke"),
        ];
        let baseline = crate::wake::poke_times(&paths);
        host.lock()
            .unwrap()
            .reload_admission(Ok(p), None, &[root.clone()]);
        crate::wake::sleep_until_poke(
            &paths,
            baseline,
            std::time::Instant::now() + std::time::Duration::from_secs(60),
            &mut || panic!("project was not awakened"),
        );
        s.pass().unwrap();
        assert_eq!(
            exits.borrow().len(),
            1,
            "reopened admission must advance pending start/review"
        );
        assert_eq!(
            task(&b, n).role.as_deref(),
            Some(if review { "reviewer" } else { "worker" })
        );
        assert_eq!(
            std::fs::read_to_string(crate::paths::dot(&root).join("focus")).unwrap(),
            "all"
        );
        drop(s);
        std::fs::remove_dir_all(root).unwrap();
    }
}

// Chain: § 2 early answer. Risk: an early answer marks the attempt gone.
#[test]
fn answer_before_the_asking_child_exits_resumes_without_gone() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let c = cfg("mac");
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
        .unwrap();
    b.comment(n, &core::entry("answer", &["2"], None, "y"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    let t = task(&b, n);
    assert_eq!(
        (t.state, t.attempt, t.retries, e.borrow().len()),
        (State::Doing, Some(a), 0, 2),
        "resumed, no gone"
    );
    // the resumed child exits without a new hand-in: that exit is gone
    *e.borrow()[1].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    assert_eq!(task(&b, n).retries, 1);
}

// Chain: § 2 answer resumes the asking attempt, every round (#1718). Risk: a second answer in one attempt stays
// undelivered while the task reads `doing` with no live worker: the pass that first saw the answer's `updated`
// read a comment list that lagged it, and every later pass reused that list for the unchanged `updated`.
#[test]
fn second_answer_in_one_attempt_resumes_within_one_pass() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let c = cfg("mac");
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    for round in 1..=2 {
        b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
            .unwrap();
        *e.borrow()[round - 1].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        assert_eq!(
            (task(&b, n).state, s.kids.len()),
            (State::Ask, 0),
            "round {round}: asked, child released"
        );
        let answer = b
            .comment(n, &core::entry("answer", &["2"], None, "y"))
            .unwrap();
        if round == 2 {
            b.state.borrow_mut().hide_latest_comment = true;
            s.pass().unwrap();
            b.state.borrow_mut().hide_latest_comment = false;
            assert_eq!(
                (s.kids.len(), e.borrow().len()),
                (0, 2),
                "a comment list lagging the answer resumes nothing yet"
            );
        }
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(
            (t.state, t.attempt, e.borrow().len(), s.kids.len()),
            (State::Doing, Some(a), round + 1, 1),
            "round {round}: resumed in one pass"
        );
        assert_eq!(
            s.delivered(a),
            answer,
            "round {round}: the answer is accepted input"
        );
    }
}

// Chain: § 3 admission filters. Risk: a waiting or filtered task starts.
#[test]
fn waiting_and_filters_hold_admission() {
    let b = Fake::default();
    let dep = b.create("dep", "", &["later".into()]).unwrap();
    b.comment(dep, &core::entry("later", &[], None, ""))
        .unwrap();
    let n = b
        .create("T", &format!("Depends on #{dep}"), &["taskq".into()])
        .unwrap();
    let other = b
        .create("O", "", &["taskq".into(), "host-win".into()])
        .unwrap();
    let user = b.create("U", "", &[]).unwrap();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert_eq!(task(&b, other).state, State::Ready);
    assert!(
        b.issue(n)
            .unwrap()
            .labels
            .contains(&"q-blocked".to_string())
    );
    b.close(dep).unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Doing);
    assert!(
        b.comments(user).unwrap().is_empty() && b.issue(user).unwrap().labels.is_empty(),
        "no taskq label: never a task"
    );
}

// Chain: § 2 worker park (#1486). Risk: a parked task starts while its blocker (a held draft too) is open, never
// resumes after the close, or hides the blocker link in status.
#[test]
fn worker_park_waits_on_its_blocker_until_it_closes() {
    let b = Fake::default();
    let blocker = b.create("fix shots", "", &["q-held".into()]).unwrap();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.root = std::env::temp_dir().join(format!("taskq-park-{}", crate::runtime::uuid()));
    c.install = c.root.clone();
    std::fs::create_dir_all(&c.root).unwrap();
    let raw = json!({});
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    b.comment(
        n,
        &core::entry("later", &[&format!("#{blocker}")], Some(a), "Reason: r"),
    )
    .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n).attempt, e.borrow().len()),
        (None, 1),
        "parked, no new spawn"
    );
    assert!(
        b.issue(n)
            .unwrap()
            .labels
            .contains(&"q-blocked".to_string())
    );
    let report = crate::snapshot::read(&b, &c, &raw, &[])
        .text(&c, &raw)
        .unwrap();
    assert!(
        report.contains(&format!(
            "dependency ([#{blocker}](https://board/{blocker}) open)"
        )),
        "{report}"
    );
    b.close(blocker).unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Doing);
    std::fs::remove_dir_all(&c.root).ok();
}

// #2189 A2-A7, §4 Balancing: connect owner policy to claims, projections and actual runtime.
#[test]
fn weighted_balancing_service_claim() {
    let b = Fake::default();
    let n = b.create("weighted", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    crate::host::Policy::parse(json!({"weights":{"claude":1,"codex":3}}), "mac")
        .unwrap()
        .apply(&mut c);
    c.limits.insert("codex".into(), 2);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert_eq!(rt_of(&task(&b, n)).unwrap().0, "codex");
    assert!(
        b.comments(n)
            .unwrap()
            .iter()
            .any(|c| c.body.contains("weighted pick codex"))
    );
}

// #2291 A2: a listing that repeats an issue (page shift) yields one view, so one Running row
#[test]
fn views_drop_repeated_listing_rows() {
    let b = Fake::default();
    let n = b.create("twice", "", &["taskq".into()]).unwrap();
    let again = b.state.borrow().issues[0].clone();
    b.state.borrow_mut().issues.push(again);
    let (v, _) = views(&b, &mut Default::default(), &mut Default::default(), 300).unwrap();
    assert_eq!(v.iter().filter(|v| v.issue.number == n).count(), 1);
}

// #2189 A3/A4/A7, §4 Balancing: count every project and use native per-account quota observations.
#[test]
fn weighted_runtime_hostwide_quota_selection() {
    let b = Fake::default();
    let n = b.create("weighted", "", &["taskq".into()]).unwrap();
    let install =
        std::env::temp_dir().join(format!("taskq-weighted-service-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&install).unwrap();
    let mut c = cfg("mac");
    c.install = install.clone();
    let policy=crate::host::Policy::parse(json!({"weights":{"claude":2,"codex":1},"limits":{"claude":4,"codex":4},"load_gate":{"enabled":false}}),"mac").unwrap();
    policy.apply(&mut c);
    let mut host = crate::host::Host::new(Ok(policy.clone()));
    host.sync(
        std::path::Path::new("/other-project"),
        vec![
            (1, 1, "claude".into()),
            (2, 2, "claude".into()),
            (3, 3, "codex".into()),
            (4, 4, "codex".into()),
        ],
    );
    let mut s = service(&b, &c, Default::default());
    s.reload(Ok(policy.cap.clone()));
    s.host = Some(std::sync::Arc::new(std::sync::Mutex::new(host)));
    let v = views(&b, &mut Default::default(), &mut Default::default(), 300)
        .unwrap()
        .0
        .remove(0);
    assert_eq!(s.runtime_load("claude").unwrap().running, 2);
    assert_eq!(s.select_runtime(&v).unwrap().0, "claude");
    let log = install.join("quota.log");
    std::fs::write(&log,"{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"rateLimitType\":\"five_hour\",\"utilization\":0.9}}\n").unwrap();
    crate::runtime::headroom::observe(&install, "claude", &log, b.now().unwrap()).unwrap();
    assert_eq!(s.select_runtime(&v).unwrap().0, "codex");
    crate::runtime::accounts::record_windows(
        &install,
        "codex",
        "default",
        b.now().unwrap(),
        &[("seven_day".into(), Some(7000))],
    )
    .unwrap();
    assert_eq!(s.select_runtime(&v).unwrap().0, "claude");
    b.labels(n, &["run-codex".into()], &[]).unwrap();
    let pinned = views(&b, &mut Default::default(), &mut Default::default(), 300)
        .unwrap()
        .0
        .remove(0);
    assert!(
        s.select_runtime(&pinned)
            .is_err_and(|e| e.0 == "usage-limit")
    );
    std::fs::remove_dir_all(install).unwrap();
}

// #2189 A5-A7, §4 Balancing: real host projections distribute claims and preserve pins/fences.
#[test]
fn weighted_balancing_host_claims_and_status() {
    let b = Fake::default();
    let mut c = cfg("mac");
    c.weights = BTreeMap::from([("claude".into(), 1), ("codex".into(), 1)]);
    c.limits = BTreeMap::from([("claude".into(), 2), ("codex".into(), 2)]);
    let w = Config {
        me: "win".into(),
        ..c.clone()
    };
    let mut mac = service(&b, &c, Default::default());
    let mut win = service(&b, &w, Default::default());
    mac.pass().unwrap();
    win.pass().unwrap();
    let first = b.create("first", "", &["taskq".into()]).unwrap();
    let second = b.create("second", "", &["taskq".into()]).unwrap();
    mac.pass().unwrap();
    assert_eq!(host_of(&task(&b, first)), Some("mac"));
    assert_eq!(task(&b, second).state, State::Ready);
    win.pass().unwrap();
    assert_eq!(host_of(&task(&b, second)), Some("win"));
    let pinned = b
        .create(
            "pinned",
            "",
            &["taskq".into(), "host-win".into(), "run-codex".into()],
        )
        .unwrap();
    mac.pass().unwrap();
    assert_eq!(task(&b, pinned).state, State::Ready);
    win.pass().unwrap();
    assert_eq!(host_of(&task(&b, pinned)), Some("win"));
    assert_eq!(rt_of(&task(&b, pinned)).unwrap().0, "codex");
    for n in [first, second, pinned] {
        assert_eq!(
            b.comments(n)
                .unwrap()
                .iter()
                .filter(|c| c.body.starts_with("taskq: spawn worker"))
                .count(),
            1,
            "claim fence"
        );
    }
    win.pass().unwrap();
    let (v, h) = views(&b, &mut Default::default(), &mut Default::default(), 300).unwrap();
    let snapshot = crate::snapshot::from_views("board", &b, &w, b.now().unwrap(), &v, &h);
    let json = snapshot.json();
    let row = json["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "win")
        .unwrap();
    assert_eq!(row["balancing"]["weights"]["codex"], 1);
    assert_eq!(row["balancing"]["runtimes"]["codex"]["load"]["running"], 1);
    let text = snapshot.text(&w, &json!({})).unwrap();
    assert!(text.contains("codex running 1 / weight 1"));
    // #2263 A1/A2: no observation names its cause in the heartbeat; status shows it and raises no anomaly row.
    assert!(text.contains("claude running 1 / weight 1 (headroom n/a: account default: Claude background jobs report no quota)"),"{text}");
    assert!(
        text.contains("(headroom n/a: account default: no quota observation yet)"),
        "{text}"
    );
    assert!(!text.contains("quota headroom not reported"), "{text}");
    assert!(row["balancing"]["runtimes"]["codex"]["load"]["headroom_reason"].is_string());
    let body = win.heartbeat_body();
    assert!(
        Balancing::read(&body).is_some(),
        "N-1 balancing shape unchanged"
    );
    assert!(Balancing::read(&body).unwrap().last_spawn.is_some());
    // #2263 review: an expired observation names a fixed time, so passes minutes apart keep an equal stable() body
    // (an age in the cause would edit the heartbeat every pass, § 4 step 3).
    std::fs::create_dir_all(&w.install).unwrap();
    let log = w.install.join("codex-turn.jsonl");
    std::fs::write(&log,"{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":20,\"window_minutes\":300}}}}\n").unwrap();
    let t = b.now().unwrap();
    crate::runtime::headroom::observe(&w.install, "codex", &log, t).unwrap();
    b.state.borrow_mut().clock = t + 400;
    let expired = win.heartbeat_body();
    assert!(
        expired.contains(&format!(
            "headroom codex account default: last quota observation {} expired",
            crate::time::utc(t)
        )),
        "{expired}"
    );
    b.state.borrow_mut().clock = t + 700;
    assert_eq!(stable(&expired), stable(&win.heartbeat_body()));
    let mut restarted = service(&b, &w, Default::default());
    let saved = Balancing::read(&b.issue(win.host_issue.unwrap()).unwrap().body)
        .unwrap()
        .last_spawn;
    restarted.pass().unwrap();
    assert_eq!(
        Balancing::read(&restarted.heartbeat_body())
            .unwrap()
            .last_spawn,
        saved
    );
}

// #2264 defect: unknown quota headroom dropped the whole load, so status showed "running count not reported" for a
// host with live workers. Review also found free slots under refusals. Kept: running/headroom survive,
// refusals report free 0, admitted capacity stays available, and peers never rank the refused runtime.
#[test]
fn weighted_heartbeat_reports_running_when_headroom_unknown() {
    let b = Fake::default();
    let install =
        std::env::temp_dir().join(format!("taskq-running-2264-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&install).unwrap();
    let mut c = cfg("win");
    c.install = install.clone();
    c.weights = BTreeMap::from([("claude".into(), 6), ("codex".into(), 2)]);
    c.limits = BTreeMap::from([("claude".into(), 2), ("codex".into(), 2)]);
    let n = b
        .create("work", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert_eq!(task(&b, n).state, State::Doing);
    std::fs::write(install.join(crate::runtime::headroom::FILE), "{\"bad\":1}").unwrap();
    let body = s.heartbeat_body();
    let read = Balancing::read(&body).unwrap();
    for (rt, running) in [("claude", 1), ("codex", 0)] {
        let r = &read.runtimes[rt];
        assert_eq!(
            (
                r.code.as_str(),
                r.load.as_ref().map(|l| (l.running, l.free, l.headroom))
            ),
            ("unknown", Some((running, 0, None))),
            "{rt}"
        );
    }
    assert!(
        s.runtime_load("claude").is_err(),
        "admission keeps refusing unknown headroom"
    );
    std::fs::remove_file(install.join(crate::runtime::headroom::FILE)).unwrap();
    for refusal in ["admitted", "focus", "quota"] {
        s.focus = if refusal == "focus" {
            vec![]
        } else {
            vec!["all".into()]
        };
        if refusal == "quota" {
            s.limited.insert("claude".into(), (i64::MAX, true));
        }
        let read = Balancing::read(&s.heartbeat_body()).unwrap();
        let r = &read.runtimes["claude"];
        assert_eq!(
            r.load.as_ref().map(|l| (l.running, l.free)),
            Some((1, if refusal == "admitted" { 1 } else { 0 })),
            "{refusal}"
        );
        assert_eq!(r.code == "admitted", refusal == "admitted");
    }

    let v = views(&b, &mut Default::default(), &mut Default::default(), 300)
        .unwrap()
        .0
        .remove(0);
    assert!(
        read.weighted_capacity(&v, false).is_err() || read.weighted_capacity(&v, false) == Ok(0.0)
    );
    std::fs::remove_dir_all(&install).ok();
}

// #2240 A1/A2, §4 Balancing: a weight-0 runtime takes no unpinned claim even when the others are full,
// a run-* pin still spawns on it, and status shows the zero weight.
#[test]
fn zero_weight_runtime_claims_only_pins() {
    let b = Fake::default();
    let mut c = cfg("mac");
    c.weights = BTreeMap::from([("claude".into(), 1), ("codex".into(), 0)]);
    c.limits = BTreeMap::from([("claude".into(), 1), ("codex".into(), 2)]);
    let mut mac = service(&b, &c, Default::default());
    let first = b.create("first", "", &["taskq".into()]).unwrap();
    let second = b.create("second", "", &["taskq".into()]).unwrap();
    mac.pass().unwrap();
    assert_eq!(rt_of(&task(&b, first)).unwrap().0, "claude");
    mac.pass().unwrap();
    assert_eq!(
        task(&b, second).state,
        State::Ready,
        "weight 0 is never a fallback"
    );
    let pinned = b
        .create("pinned", "", &["taskq".into(), "run-codex".into()])
        .unwrap();
    mac.pass().unwrap();
    assert_eq!(rt_of(&task(&b, pinned)).unwrap().0, "codex");
    assert_eq!(task(&b, second).state, State::Ready);
    let (v, h) = views(&b, &mut Default::default(), &mut Default::default(), 300).unwrap();
    let snapshot = crate::snapshot::from_views("board", &b, &c, b.now().unwrap(), &v, &h);
    assert!(
        snapshot
            .text(&c, &json!({}))
            .unwrap()
            .contains("codex running 1 / weight 0")
    );
}

// #2138 A2-A4/R13: mode and collaborator policy must reach real claims; preserve strict pins by default.
#[test]
fn balancing_modes_and_project_execution() {
    for (mode, own, unlabelled) in [
        ("any", true, true),
        ("labelled", true, false),
        ("none", false, false),
    ] {
        let b = Fake::default();
        let n = b.create("unlabelled", "", &["taskq".into()]).unwrap();
        let pin = b
            .create("pinned", "", &["taskq".into(), "host-mac".into()])
            .unwrap();
        let foreign = b
            .create("foreign", "", &["taskq".into(), "host-win".into()])
            .unwrap();
        let review = b
            .create("review", "", &["taskq".into(), "host-mac".into()])
            .unwrap();
        let a = b
            .comment(review, "taskq: spawn worker claude:previous win")
            .unwrap();
        b.comment(
            review,
            &core::entry(
                "result",
                &["0123456789abcdef0123456789abcdef01234567"],
                Some(a),
                "Changed: candidate\nOpen: none",
            ),
        )
        .unwrap();
        accept_execution(&b, review, a, a, None).unwrap();
        release_execution(&b, review, a, a, "exit").unwrap();
        let mut c = cfg("mac");
        crate::host::Policy::parse(
            json!({"hosts":{crate::config::hostname():"mac"},"claim":mode}),
            &crate::config::hostname(),
        )
        .unwrap()
        .apply(&mut c);
        c.limits.insert("claude".into(), 4);
        service(&b, &c, Default::default()).pass().unwrap();
        assert_eq!(task(&b, n).state == State::Doing, unlabelled, "{mode}");
        assert_eq!(task(&b, pin).state == State::Doing, own, "{mode}");
        assert_eq!(task(&b, foreign).state, State::Ready);
        assert_eq!(
            task(&b, review).role.as_deref() == Some("reviewer"),
            own,
            "review {mode}"
        );
    }
    for reroute in [false, true] {
        let b = Fake::default();
        let n = b
            .create("collaborator", "", &["taskq".into(), "run-claude".into()])
            .unwrap();
        let mut c = cfg("fleet");
        c.install =
            std::env::temp_dir().join(format!("taskq-collaborator-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&c.install).unwrap();
        let p=crate::host::Policy::parse(json!({"load_gate":{"enabled":false},"limits":{"claude":0,"codex":2},"projects":{c.root.to_string_lossy():{"default_runtime":"codex","allowed_runtimes":["codex"],"auto_reroute":reroute}}}),"fleet").unwrap();
        p.apply(&mut c);
        c.limits = p.project_limits(&c.root, &json!({})).unwrap();
        let mut s = service(&b, &c, Default::default());
        let mut host = crate::host::Host::new(Ok(p.clone()));
        host.reload(Ok(p.clone()));
        s.host = Some(std::sync::Arc::new(std::sync::Mutex::new(host)));
        s.reload(Ok(p.project(&c.root, &json!({})).unwrap()));
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(
            t.state,
            if reroute { State::Doing } else { State::Ready },
            "{:?}",
            s.snapshot
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .find(|r| r.n == n)
                .unwrap()
                .admission
        );
        if reroute {
            assert_eq!(rt_of(&t).unwrap().0, "codex");
        }
        assert!(
            b.issue(n).unwrap().labels.contains(&"run-claude".into()),
            "no board relabel"
        );
        std::fs::remove_dir_all(&c.install).unwrap();
    }
}

// #2138 A2/A4/R12: real heartbeat publication -> spillover claim, with quota, unknown and fencing intact.
#[test]
fn balancing_spillover_uses_effective_primary_availability() {
    for scenario in [
        "free",
        "full",
        "quota",
        "spare",
        "unknown",
        "unknown-focus",
        "unknown-load",
        "legacy",
        "corrupt",
        "stale",
        "none",
        "spacing",
        "pinned",
        "capability",
        "fenced",
        "one-free",
        "all-full",
    ] {
        let b = Fake::default();
        let root = std::env::temp_dir().join(format!("taskq-spillover-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&root).unwrap();
        let mut primary = cfg("win");
        primary.install = root.clone();
        primary.limits = BTreeMap::from([
            ("claude".into(), 1),
            ("codex".into(), if scenario == "spare" { 1 } else { 0 }),
            ("antigravity".into(), 0),
        ]);
        let mut p = service(&b, &primary, Default::default());
        if matches!(scenario, "quota" | "spare") {
            crate::runtime::accounts::record_windows(
                &root,
                "claude",
                "default",
                0,
                &[
                    ("five_hour".into(), Some(18000)),
                    ("seven_day".into(), Some(604800)),
                ],
            )
            .unwrap();
        }
        if scenario == "unknown" {
            p.cap = Err("unreadable policy".into());
        }
        if scenario == "unknown-focus" {
            p.focus.clear();
            p.focus_error = Some("invalid focus".into());
        }
        if scenario == "unknown-load" {
            p.gate.configure(GateCfg {
                enabled: true,
                ..GATE_DEFAULT
            });
        }
        if scenario == "spacing" {
            // A real open gate inside its 30 s launch interval, as the primary publishes it.
            p.gate.configure(GateCfg {
                enabled: true,
                ..GATE_DEFAULT
            });
            p.gate.last = Some((p.mono, 0, Some(1.0), Some(90.0)));
            p.gate.open = true;
            p.gate.launch(p.mono);
            assert!(p.heartbeat_body().contains("launch-interval"));
        }
        if matches!(scenario, "full" | "one-free" | "all-full") {
            p.reserved.push((99, 99, "claude".into()));
        }
        if scenario != "none" {
            let body = if scenario == "legacy" {
                "version old".into()
            } else if scenario == "corrupt" {
                "balancing {\"schema\":99}".into()
            } else {
                p.heartbeat_body()
            };
            let hb = b
                .create("taskq host win", &body, &[HOST_LABEL.into()])
                .unwrap();
            if scenario == "stale" {
                b.state
                    .borrow_mut()
                    .issues
                    .iter_mut()
                    .find(|i| i.number == hb)
                    .unwrap()
                    .updated = -1000;
            }
        }
        if matches!(scenario, "one-free" | "all-full") {
            let mut second = p.balancing();
            if scenario == "one-free" {
                let r = second.runtimes.get_mut("claude").unwrap();
                r.code = "admitted".into();
                r.reason.clear();
            }
            b.create(
                "taskq host linux",
                &format!("balancing {}", serde_json::to_string(&second).unwrap()),
                &[HOST_LABEL.into()],
            )
            .unwrap();
        }
        let labels = if scenario == "pinned" {
            vec!["taskq".into(), "host-mac".into()]
        } else if scenario == "capability" {
            vec!["taskq".into(), "needs-special".into()]
        } else {
            vec!["taskq".into()]
        };
        let n = b.create("work", "", &labels).unwrap();
        if scenario == "fenced" {
            let a = b.comment(n, "taskq: spawn worker claude:s win").unwrap();
            b.comment(
                n,
                &core::entry("requeue", &[], Some(a), "Reason: keep old execution fenced"),
            )
            .unwrap();
        }
        let mut c = cfg("mac");
        c.claim = crate::host::ClaimMode::Spillover;
        c.has.push("special".into());
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        let expected = matches!(
            scenario,
            "full"
                | "quota"
                | "unknown"
                | "unknown-focus"
                | "unknown-load"
                | "legacy"
                | "corrupt"
                | "stale"
                | "none"
                | "pinned"
                | "capability"
                | "all-full"
        );
        assert_eq!(
            task(&b, n).state == State::Doing,
            expected,
            "{scenario}: {:?}",
            s.log
        );
        if scenario == "quota" {
            let spawn = entries(&b, n)
                .unwrap()
                .into_iter()
                .find(|e| e.body.starts_with("taskq: spawn"))
                .unwrap();
            assert!(
                spawn.body.contains("seven_day"),
                "quota reset/probe evidence retained: {}",
                spawn.body
            );
            assert_eq!(
                p.cfg.limits, primary.limits,
                "quota never changes saved capacity"
            );
        }
        if !expected && scenario != "fenced" {
            let reason = &s
                .snapshot
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .find(|r| r.n == n)
                .unwrap()
                .admission
                .as_ref()
                .unwrap()
                .reason;
            assert!(
                reason.contains("spillover waits: primary "),
                "{scenario}: {reason}"
            );
            assert!(!reason.contains("grace"), "{scenario}: {reason}");
        }
        // A second pass must never duplicate a live claim.
        s.pass().unwrap();
        assert!(
            entries(&b, n)
                .unwrap()
                .iter()
                .filter(|e| e.body.starts_with("taskq: spawn"))
                .count()
                <= 1,
            "{scenario}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

// Every wording a primary publishes for temporary launch spacing is waiting; real holds stay exhaustion.
#[test]
fn balancing_launch_spacing_is_waiting_not_exhaustion() {
    for waiting in [
        "launch-interval: launch interval",
        "host load gate: 30 s launch spacing (12 s remaining)",
        "host launched 12 s ago",
        "no load sample since the last host launch",
    ] {
        assert!(launch_spacing(waiting), "{waiting}");
    }
    for exhausted in [
        "cpu: CPU 95 % >= 80 %",
        "host at its maximum of 2 sessions",
        "claude at capacity",
    ] {
        assert!(!launch_spacing(exhausted), "{exhausted}");
    }
}

// #2201 A1/A2: an untrusted author's fake primary heartbeat with free capacity never delays spillover;
// host_claim reads only trusted heartbeats, like weighted_host and the own-host balancing read.
#[test]
fn balancing_spillover_ignores_untrusted_heartbeat() {
    let b = Fake::default();
    let root = std::env::temp_dir().join(format!("taskq-spillover-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let mut primary = cfg("win");
    primary.install = root.clone();
    primary.limits = BTreeMap::from([
        ("claude".into(), 1),
        ("codex".into(), 0),
        ("antigravity".into(), 0),
    ]);
    let mut p = service(&b, &primary, Default::default());
    p.reserved.push((99, 99, "claude".into()));
    b.create("taskq host win", &p.heartbeat_body(), &[HOST_LABEL.into()])
        .unwrap();
    let mut free = p.balancing();
    let r = free.runtimes.get_mut("claude").unwrap();
    r.code = "admitted".into();
    r.reason.clear();
    b.create(
        "taskq host fake",
        &format!("balancing {}", serde_json::to_string(&free).unwrap()),
        &[HOST_LABEL.into()],
    )
    .unwrap();
    let n = b.create("work", "", &[TASK_LABEL.into()]).unwrap();
    let mut c = cfg("mac");
    c.claim = crate::host::ClaimMode::Spillover;
    let mut s = service(&b, &c, Default::default());
    let views = views(&b, &mut s.cache, &mut s.deps, 300).unwrap().0;
    let v = views.iter().find(|v| v.issue.number == n).unwrap();
    let mut hosts: Vec<Issue> = b
        .state
        .borrow()
        .issues
        .iter()
        .filter(|i| is_host(i))
        .cloned()
        .collect();
    let now = b.now().unwrap();
    assert!(
        s.host_claim(v, &hosts, now)
            .unwrap_err()
            .contains("primary fake"),
        "trusted fake still counts"
    );
    hosts
        .iter_mut()
        .find(|h| h.title == "taskq host fake")
        .unwrap()
        .trusted = false;
    let claim = s
        .host_claim(v, &hosts, now)
        .expect("untrusted heartbeat must not make spillover wait");
    assert!(
        claim
            .unwrap()
            .contains("zero effective primary availability")
    );
    std::fs::remove_dir_all(root).unwrap();
}

// #2138 A3/R9: the exception applies to operator disables only, for workers/reviewers, without enlarging access.
#[test]
fn balancing_project_reroute_keeps_other_pins_and_restrictions() {
    for reviewer in [false, true] {
        for scenario in [
            "zero",
            "excluded",
            "accounts-off",
            "quota",
            "full",
            "login",
            "access",
            "unknown",
            "app",
            "none-enabled",
            "fallback-access",
        ] {
            let b = Fake::default();
            let root = std::env::temp_dir()
                .join(format!("taskq-project-reroute-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            let mut c = cfg("fleet");
            c.install = root.clone();
            c.auto_reroute = true;
            c.default_runtime = crate::runtime::RuntimeId::Codex;
            c.limits = BTreeMap::from([
                (
                    "claude".into(),
                    if matches!(scenario, "zero" | "fallback-access") {
                        0
                    } else {
                        1
                    },
                ),
                ("codex".into(), 2),
            ]);
            if scenario == "excluded" {
                c.allowed_runtimes = Some(vec![crate::runtime::RuntimeId::Codex]);
            }
            if scenario == "none-enabled" {
                c.allowed_runtimes = Some(vec![]);
            }
            if scenario == "accounts-off" {
                std::fs::write(
                    root.join("accounts.json"),
                    r#"{"claude":[{"label":"default","enabled":false}]}"#,
                )
                .unwrap();
            }
            if scenario == "quota" {
                crate::runtime::accounts::record_windows(
                    &root,
                    "claude",
                    "default",
                    0,
                    &[("seven_day".into(), Some(604800))],
                )
                .unwrap();
            }
            let pin = if scenario == "app" {
                "run-codex-app"
            } else {
                "run-claude"
            };
            let n = b
                .create("collaborator", "", &["taskq".into(), pin.into()])
                .unwrap();
            let mut s = service(&b, &c, Default::default());
            if scenario == "full" {
                s.kids.push(Kid {
                    unknown: None,
                    stopping: false,
                    session_url: None,
                    execution: 99,
                    task: 99,
                    attempt: 99,
                    rt: "claude".into(),
                    proc: Box::new(FakeProc(Default::default())),
                });
            }
            if scenario == "login" {
                s.logged_out.insert("claude".into());
            }
            if scenario == "access" {
                s.refused_access
                    .insert("claude".into(), "missing grant".into());
            }
            if scenario == "fallback-access" {
                s.refused_access
                    .insert("codex".into(), "missing fallback grant".into());
            }
            if scenario == "unknown" {
                s.cap = Err("invalid policy".into());
            }
            let mut views = views(&b, &mut s.cache, &mut s.deps, 300).unwrap().0;
            let v = views.iter_mut().find(|v| v.issue.number == n).unwrap();
            if reviewer {
                v.projection.task.state = State::Review;
            }
            let selected = s.select_runtime(v);
            if matches!(scenario, "zero" | "excluded" | "accounts-off") {
                let (rt, reason) = selected.unwrap();
                assert_eq!(rt, "codex");
                assert!(reason.unwrap().contains("auto_reroute"));
            } else {
                assert!(selected.is_err(), "{scenario}: {selected:?}");
            }
            // Reuse exactly the published runtime policy on the other host.
            if !matches!(scenario, "unknown" | "app") {
                let balance = Balancing::read(&s.heartbeat_body()).unwrap();
                assert_eq!(
                    balance.available(v).is_ok(),
                    matches!(scenario, "zero" | "excluded" | "accounts-off"),
                    "remote {scenario}"
                );
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

// Chain: § 4 Filter, host.json `claim` (#1493). Risk: a labelled host claims unrouted work, or an any host stops
// taking it.
#[test]
fn labelled_host_claims_only_its_own_host_label() {
    let b = Fake::default();
    let open = b.create("U", "", &["taskq".into()]).unwrap();
    let own = b
        .create("F", "", &["taskq".into(), "host-fra".into()])
        .unwrap();
    let fra = Config {
        claim: crate::host::ClaimMode::Labelled,
        ..cfg("fra")
    };
    service(&b, &fra, Default::default()).pass().unwrap();
    assert_eq!(
        (task(&b, open).state, task(&b, own).state),
        (State::Ready, State::Doing),
        "labelled host skips the unlabelled task"
    );
    service(&b, &cfg("mac"), Default::default()).pass().unwrap();
    assert_eq!(
        task(&b, open).state,
        State::Doing,
        "an any host still takes unlabelled work"
    );
}

// Chain: § 1 `Depends on <repo>#N` (#1126). Risk: a task blocked by another repository never starts, starts on a
// close that did not complete, or reports no linked reason.
#[test]
fn cross_board_dependency_waits_with_link_until_completed() {
    let b = Fake::default();
    let dashq = |open: bool, reason: Option<&str>| {
        (
            "dashq".to_string(),
            Issue {
                number: 36,
                open,
                reason: reason.map(String::from),
                ..Default::default()
            },
        )
    };
    b.state.borrow_mut().foreign.push(dashq(true, None));
    let dep = b.create("dep", "", &[]).unwrap();
    let n = b
        .create(
            "T",
            "Depends on dashq#36\nDepends on owner/gone#5",
            &["taskq".into()],
        )
        .unwrap();
    let same = b
        .create("S", &format!("Depends on #{dep}"), &["taskq".into()])
        .unwrap();
    // § 8 cutover: N-1 reads only `Depends on #N`; a held task has no `taskq` label, so N-1 (same `is_task`) skips it
    assert!(
        core::depends("Depends on dashq#36")
            .iter()
            .all(|d| d.repo.is_some())
    );
    let held = b
        .create("H", "Depends on dashq#36", &["q-held".into()])
        .unwrap();
    assert!(!is_task(&b.issue(held).unwrap()) && is_held(&b.issue(held).unwrap()));
    let mut c = cfg("mac");
    // A shared absolute path may contain real profile records on the test host.
    c.root = std::env::temp_dir().join(format!("taskq-cross-board-{}", crate::runtime::uuid()));
    c.install = c.root.clone();
    std::fs::create_dir_all(&c.root).unwrap();
    let raw = json!({});
    let report = || {
        crate::snapshot::read(&b, &c, &raw, &[])
            .text(&c, &raw)
            .unwrap()
    };
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert!(
        b.issue(n)
            .unwrap()
            .labels
            .contains(&"q-blocked".to_string())
    );
    assert!(report().contains("dependency ([dashq#36](https://board/dashq/36) open, [owner/gone#5](https://board/owner/gone/5) unknown: HTTP 404 owner/gone#5)"), "{}", report());
    b.state.borrow_mut().foreign = vec![
        dashq(false, Some("not_planned")),
        (
            "owner/gone".into(),
            Issue {
                number: 5,
                ..Default::default()
            },
        ),
    ];
    b.state.borrow_mut().issues[dep as usize - 1].reason = Some("not_planned".into());
    b.state.borrow_mut().issues[dep as usize - 1].open = false;
    s.pass().unwrap();
    let blocked = |t: u64| {
        b.issue(t)
            .unwrap()
            .labels
            .contains(&"q-blocked".to_string())
    };
    assert!(
        blocked(n) && blocked(same),
        "not completed stays unknown, not done"
    );
    assert!(
        report().contains("[dashq#36](https://board/dashq/36) unknown: closed not planned)"),
        "{}",
        report()
    );
    b.state.borrow_mut().foreign[0] = dashq(false, Some("completed"));
    b.state.borrow_mut().reads = 0;
    s.pass().unwrap();
    assert_eq!(
        task(&b, n).state,
        State::Doing,
        "eligible within one pass after the blocker completed"
    );
    assert_eq!(
        b.state.borrow().reads,
        1,
        "closed dependencies already read are cached: only dashq#36 read again"
    );
    drop(s);
    std::fs::remove_dir_all(&c.root).unwrap();
}

// Chain: § 3 priority order. Risk: legacy priority labels order or filter wrong.
#[test]
fn canonical_and_legacy_priorities_filters_and_conflicts() {
    let b = Fake::default();
    let old = b
        .create("old", "", &["taskq".into(), "priority-1".into()])
        .unwrap();
    let new = b.create("new", "", &["taskq".into(), "p1".into()]).unwrap();
    let conflict = b
        .create("bad", "", &["taskq".into(), "p1".into(), "p2".into()])
        .unwrap();
    assert_eq!(
        priority(&b.issue(old).unwrap()),
        priority(&b.issue(new).unwrap())
    );
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 3);
    let mut s = service(&b, &c, Default::default());
    s.focus = vec!["p1".into()];
    s.pass().unwrap();
    assert_eq!(task(&b, old).state, State::Doing);
    assert_eq!(task(&b, new).state, State::Doing);
    assert_eq!(task(&b, conflict).state, State::Ready);
    assert!(
        b.comments(conflict).unwrap().is_empty(),
        "conflicting restriction never claimed"
    );
}

// Chain: § 4 step 6 order with one slot. Risk: p0 workers starve pending reviews (#1365), or a review jumps a resume.
#[test]
fn one_slot_goes_to_resume_then_review_then_ready() {
    for resume in [false, true] {
        let b = Fake::default();
        let live = b.create("L", "", &["taskq".into(), "p2".into()]).unwrap();
        let review = b.create("R", "", &["taskq".into(), "p1".into()]).unwrap();
        let ready = b.create("P0", "", &["taskq".into(), "p0".into()]).unwrap();
        let w = b
            .comment(review, "taskq: spawn worker codex:w mac")
            .unwrap();
        accept_execution(&b, review, w, w, None).unwrap();
        release_execution(&b, review, w, w, "exit").unwrap();
        b.comment(
            review,
            &format!("taskq: result {} attempt {w}", "a".repeat(40)),
        )
        .unwrap();
        if resume {
            let a = b
                .comment(live, "taskq: spawn worker claude:session mac")
                .unwrap();
            accept_execution(&b, live, a, a, None).unwrap();
            release_execution(&b, live, a, a, "exit").unwrap();
            b.comment(live, "taskq: steer\ncontinue").unwrap();
        } else {
            b.comment(live, "taskq: later\nparked").unwrap();
        }
        let c = cfg("mac");
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let launched: Vec<u64> = e.borrow().iter().map(|x| x.0).collect();
        assert_eq!(
            launched,
            vec![if resume { live } else { review }],
            "resume={resume}"
        );
        assert_eq!(task(&b, ready).state, State::Ready, "p0 waits for the slot");
        assert_eq!(
            task(&b, review).attempt.is_some(),
            !resume,
            "a resume keeps precedence over a review"
        );
    }
}

// Chain: § 3 p0 admission. Risk: p0 blocked by focus or passes pause.
#[test]
fn p0_passes_any_focus_but_not_pause_or_limits() {
    let b = Fake::default();
    let p1 = b
        .create("P1", "", &["taskq".into(), "p1".into(), "area-maps".into()])
        .unwrap();
    let p0 = b
        .create("P0", "", &["taskq".into(), "p0".into(), "area-ui".into()])
        .unwrap();
    let other = b
        .create("O", "", &["taskq".into(), "area-ui".into()])
        .unwrap();
    assert!(priority(&b.issue(p0).unwrap()) < priority(&b.issue(p1).unwrap()));
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.focus = vec![];
    s.pass().unwrap();
    assert_eq!(task(&b, p0).state, State::Ready, "pause: p0 waits too");
    s.focus = vec!["area-maps".into()];
    s.pass().unwrap();
    assert_eq!(
        (
            task(&b, p0).state,
            task(&b, p1).state,
            task(&b, other).state
        ),
        (State::Doing, State::Ready, State::Ready),
        "p0 first; limit 1 holds"
    );
}

// Chain: § 3 focus and needs. Risk: a task outside focus or needs starts.
#[test]
fn focus_and_needs_choose_tasks() {
    let b = Fake::default();
    let win = b
        .create("W", "", &["taskq".into(), "needs-windows".into()])
        .unwrap();
    let maps = b
        .create("M", "", &["taskq".into(), "area-maps".into()])
        .unwrap();
    let other = b.create("O", "", &["taskq".into()]).unwrap();
    let gated = b
        .create("G", "", &["taskq".into(), "needs-gate".into()])
        .unwrap();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.focus = vec![];
    s.pass().unwrap();
    assert_eq!(
        task(&b, other).state,
        State::Ready,
        "focus off: nothing starts"
    );
    s.focus = vec!["area-maps".into()];
    s.pass().unwrap();
    assert_eq!(
        (task(&b, maps).state, task(&b, other).state),
        (State::Doing, State::Ready)
    );
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 9);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    assert_eq!(
        (task(&b, win).state, task(&b, other).state),
        (State::Ready, State::Doing),
        "mac has no windows"
    );
    assert_eq!(
        task(&b, gated).state,
        State::Doing,
        "every host of this version has gates (§ 8)"
    );
    let w = Config {
        has: vec!["windows".into()],
        ..c.clone()
    };
    let mut s = service(&b, &w, Default::default());
    s.pass().unwrap();
    assert_eq!(task(&b, win).state, State::Doing);
}

// The shared TCC log has no result on the first pass. Risk: a grants read tied to the 300 s capture keeps that reading a
// whole period. Protection: grants refresh every pass while the capture waits its period.
#[test]
fn gui_grants_refresh_every_pass_between_captures() {
    let b = Fake::default();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    let at = std::time::Instant::now();
    s.gui_checked = Some(at);
    s.grants.insert("earlier".into(), "stale".into());
    crate::service_run::refresh_gui(&mut s, std::path::Path::new("/i"));
    assert_eq!(s.gui_checked, Some(at), "the capture waits its period");
    assert!(!s.grants.contains_key("earlier"));
}

// Chain: § 4 needs-gui routing (#1124). Risk: a gui task starts on a sandboxed runtime or an unprobed host, or waits silently.
#[test]
fn needs_gui_goes_only_to_a_probed_full_access_runtime() {
    let b = Fake::default();
    let t = b
        .create("G", "", &["taskq".into(), "needs-gui".into()])
        .unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 1);
    c.has = vec!["gui".into()]; // config never lists gui: only the probe does
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    let hb = |b: &Fake| {
        b.issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host mac")
            .unwrap()
            .body
    };
    s.pass().unwrap();
    assert_eq!(
        (task(&b, t).state, s.waits.contains_key(&t)),
        (State::Ready, true),
        "no probe: no gui host"
    );
    s.gui_checked = Some(std::time::Instant::now());
    s.gui_probe = Ok(());
    s.pass().unwrap();
    assert_eq!(
        (task(&b, t).state, s.waits.contains_key(&t)),
        (State::Ready, true),
        "every runtime sandboxed: no gui host"
    );
    assert!(!hb(&b).lines().any(|l| l == "has gui"));
    s.full = BTreeSet::from(["claude".to_string()]);
    s.gui_checked = Some(std::time::Instant::now() - std::time::Duration::from_secs(300));
    s.pass().unwrap();
    assert_eq!(
        task(&b, t).state,
        State::Ready,
        "stale capture cannot admit GUI work"
    );
    s.gui_checked = Some(std::time::Instant::now());
    b.labels(t, &["run-codex".into(), "prefer-claude".into()], &[])
        .unwrap();
    s.pass().unwrap();
    assert_eq!(
        task(&b, t).state,
        State::Ready,
        "hard runtime must not fall back to a Full runtime"
    );
    assert!(s.waits[&t].contains("codex has no effective Full grant"));
    b.labels(t, &[], &["run-codex".into(), "prefer-claude".into()])
        .unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, t).state, State::Doing);
    assert!(
        task(&b, t).session.unwrap().starts_with("claude:"),
        "only the Full access runtime"
    );
    assert!(hb(&b).lines().any(|l| l == "has gui") && s.waits.is_empty());
    // Full -> Workspace, then probe loss: the resume waits, inbox and lease kept; it runs once both hold again
    let a = task(&b, t).attempt.unwrap();
    b.comment(t, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    b.comment(t, &core::entry("answer", &["1"], None, "x"))
        .unwrap();
    for (probe, full) in [
        (Ok(()), vec![]),
        (
            Err("screencapture wrote no PNG".to_string()),
            vec!["claude".to_string()],
        ),
    ] {
        (s.gui_probe, s.full) = (probe, full.into_iter().collect());
        s.pass().unwrap();
        assert_eq!(
            (e.borrow().len(), task(&b, t).attempt),
            (1, Some(a)),
            "no sandboxed resume"
        );
        assert!(
            s.waits.contains_key(&t),
            "resume refusal must name a reason"
        );
        assert!(
            hb(&b).lines().any(|l| l == format!("#{t} {a}")),
            "the waiting resume keeps its lease"
        );
    }
    s.gui_probe = Ok(());
    s.gui_checked = Some(std::time::Instant::now() - std::time::Duration::from_secs(300));
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), 1, "stale evidence must not resume");
    assert!(s.waits[&t].contains("stale"));
    s.gui_checked = Some(std::time::Instant::now());
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), 2, "resumed on the Full runtime");
    let other = b
        .create("H", "", &["taskq".into(), "needs-gui".into()])
        .unwrap();
    let w = cfg("win");
    let mut s = service(&b, &w, Default::default());
    s.gui_probe = Err("no gui probe on this OS".into());
    s.pass().unwrap();
    assert!(s.waits[&other].contains("no gui probe on this OS"));
    assert_eq!(
        (task(&b, other).state, s.waits.contains_key(&other)),
        (State::Ready, true),
        "a remote heartbeat cannot hide local missing evidence"
    );
}

// § 4 GUI eligibility: won claims retain their lease/slot while evidence expires or access is revoked.
#[test]
fn needs_gui_held_launch_rechecks_evidence_and_grant() {
    // #1845 A3: a stale Accessibility grant holds the launch with a visible reason instead of a worker ask
    for case in ["missing", "unsupported", "stale", "revoked", "grant"] {
        let b = Fake::default();
        let n = b
            .create("GUI", "", &[TASK_LABEL.into(), "needs-gui".into()])
            .unwrap();
        let c = cfg("mac");
        let exits: Exits = Default::default();
        let mut s = service(&b, &c, exits.clone());
        s.gui_probe = Ok(());
        s.gui_checked = Some(std::time::Instant::now());
        s.full.insert("claude".into());
        s.hold = Some("fixture load hold".into());
        let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300)
            .unwrap()
            .0
            .remove(0);
        s.claim(&v, "worker", "claude", "").unwrap();
        let a = task(&b, n).attempt.unwrap();
        assert_eq!(s.held.len(), 1);
        s.hold = None;
        match case {
            "missing" => s.gui_checked = None,
            "unsupported" => s.gui_probe = Err("no gui probe on this OS".into()),
            "stale" => {
                s.gui_checked =
                    Some(std::time::Instant::now() - std::time::Duration::from_secs(300))
            }
            "grant" => {
                s.grants.insert("accessibility".into(), "stale".into());
            }
            _ => s.full.clear(),
        }
        s.pass().unwrap();
        assert!(exits.borrow().is_empty(), "{case}: no held launch");
        if case == "grant" {
            assert!(
                s.waits[&n].contains("Accessibility grant stale (TCC)"),
                "{}",
                s.waits[&n]
            );
        }
        s.grants.clear();
        assert_eq!(s.held.len(), 1);
        assert_eq!(task(&b, n).attempt, Some(a));
        assert!(s.waits.contains_key(&n), "{case}: visible reason");
        assert!(s.heartbeat_body().lines().any(|l| l == format!("#{n} {a}")));
        s.gui_probe = Ok(());
        s.gui_checked = Some(std::time::Instant::now());
        s.full.insert("claude".into());
        s.pass().unwrap();
        assert_eq!(
            exits.borrow().len(),
            1,
            "{case}: resume the same held claim once"
        );
        assert_eq!(task(&b, n).attempt, Some(a));
        assert!(s.held.is_empty());
    }
}

// Chain: § 4 empty focus (#974). Risk: a host with free slots admits nothing and says nowhere why.
#[test]
fn empty_focus_is_a_capacity_reason_and_shows_in_hosts() {
    let b = Fake::default();
    let t = b.create("T", "", &["taskq".into()]).unwrap();
    let c = cfg("mac");
    let mut s = service(&b, &c, Default::default());
    s.focus.clear();
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(task(&b, t).state, State::Ready);
    let focus = |s: &Service| {
        s.capacity_json()["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["code"] == "focus")
            .map(|r| r["text"].as_str().unwrap().to_string())
    };
    assert_eq!(focus(&s).as_deref(), Some(FOCUS_OFF));
    let text = s.snapshot.as_ref().unwrap().text(&c, &Value::Null).unwrap();
    assert!(
        text.contains("Hosts: mac v4.0.0 ") && text.ends_with(" focus off"),
        "{text}"
    );
    s.focus_error = Some("focus: not UTF-8 text; file kept".into());
    assert_eq!(
        focus(&s).as_deref(),
        s.focus_error.as_deref(),
        "an unreadable focus names its own reason"
    );
    s.focus_error = None;
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!((focus(&s), task(&b, t).state), (None, State::Doing));
    assert!(
        !s.snapshot
            .as_ref()
            .unwrap()
            .text(&c, &Value::Null)
            .unwrap()
            .contains("focus")
    );
}

// #2137 A7: a mid-work quota preserves ownership; restart waits, then resumes only the same native session.
#[test]
fn balancing_midwork_quota_resumes_same_session_after_restart() {
    for (reviewer, refuse) in [(false, false), (true, false), (false, true), (true, true)] {
        let b = Fake::default();
        let root =
            std::env::temp_dir().join(format!("taskq-midwork-quota-{}", crate::runtime::uuid()));
        let mut c = cfg("mac");
        c.root = root.clone();
        c.install = root.join("install");
        let n = b
            .create("Keep work", "", &["taskq".into(), "run-claude".into()])
            .unwrap();
        if reviewer {
            let a = b
                .comment(n, "taskq: spawn worker claude:worker mac")
                .unwrap();
            b.comment(n, &format!("taskq: result {} attempt {a}", "a".repeat(40)))
                .unwrap();
            b.comment(n, &format!("taskq: released {a} exit attempt {a}"))
                .unwrap();
        }
        let calls = Rc::new(RefCell::new(vec![]));
        let exits: Exits = Default::default();
        let starter = || {
            let (calls, exits, root) = (calls.clone(), exits.clone(), root.clone());
            Box::new(move |l: &Launch| {
                calls
                    .borrow_mut()
                    .push((l.attempt, l.session.clone(), l.resume.is_some()));
                let log = crate::runtime::log_path(&root, l);
                std::fs::create_dir_all(log.parent().unwrap()).unwrap();
                if l.resume.is_none() {
                    std::fs::write(&log,json!({"type":"system","subtype":"init","session_id":l.session}).to_string()+"\n"+concat!(
                    "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"Read\"}]}}\n",
                    "{\"type\":\"assistant\",\"apiError\":\"usage_limit_reached\",\"quotaLimits\":{\"status\":\"rejected\",\"resetsAt\":18000}}\n")).unwrap();
                } else {
                    if refuse {
                        return Err("native continuation unavailable; no replacement".into());
                    }
                    crate::runtime::rules_line(&log, "resume", "fixture").unwrap();
                }
                let flag = Rc::new(RefCell::new(None));
                exits.borrow_mut().push((l.task, flag.clone()));
                Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
            })
                as Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>, crate::runtime::StartError>>
        };
        let mut s = Service::new(&b, &c, "fixture", starter());
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        *exits.borrow()[0].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        assert_eq!(
            (task(&b, n).state, task(&b, n).attempt, task(&b, n).retries),
            (
                if reviewer {
                    State::Review
                } else {
                    State::Doing
                },
                Some(a),
                0
            )
        );
        drop(s);
        let mut s = Service::new(&b, &c, "fixture", starter());
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        assert_eq!(
            calls.borrow().len(),
            1,
            "restart cannot retry the exhausted account"
        );
        b.state.borrow_mut().clock = 18000;
        let _ = s.pass();
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(calls.borrow()[1], (a, calls.borrow()[0].1.clone(), true));
        assert_eq!(task(&b, n).attempt, Some(a));
        if refuse {
            for _ in 0..3 {
                let _ = s.pass();
            }
            assert_eq!(
                calls.borrow().len(),
                2,
                "unavailable continuation never replays or migrates"
            );
            assert_eq!((task(&b, n).attempt, task(&b, n).retries), (Some(a), 0));
            assert!(s.execution_waits[&n].contains("quota continuation unavailable"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

// #2137 A3: stale service selections and concurrent spawns cannot cross a board routing fence.
#[test]
fn balancing_reroute_fences_stale_service_claims() {
    for concurrent in [false, true] {
        let b = Fake::default();
        let root =
            std::env::temp_dir().join(format!("taskq-route-fence-{}", crate::runtime::uuid()));
        let mut c = cfg("mac");
        c.root = root.clone();
        c.install = root.join("install");
        c.limits.insert("codex".into(), 1);
        let n = b
            .create("Reroute", "", &["taskq".into(), "run-claude".into()])
            .unwrap();
        let v = views(&b, &mut Default::default(), &mut Default::default(), 300)
            .unwrap()
            .0
            .remove(0);
        let exits: Exits = Default::default();
        let mut s = service(&b, &c, exits.clone());
        if concurrent {
            b.state.borrow_mut().race =
                Some("taskq: reroute claude codex\nrouting-fence: 1".into());
        } else {
            b.comment(n, "taskq: reroute claude codex\nrouting-fence: 1")
                .unwrap();
            assert!(crate::dispatch::finish_reroute(&b, n).ok());
        }
        s.claim(&v, "worker", "claude", "").unwrap();
        assert!(exits.borrow().is_empty());
        assert_eq!(task(&b, n).state, State::Ready);
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!(
            rt_of(&task(&b, n)).unwrap().0,
            "codex",
            "automatic pending-intent reconciliation selects the new pin"
        );
        assert_eq!(exits.borrow().len(), 1);
        assert_eq!(task(&b, n).retries, 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}

// Chain: §5, #2137 A3. A paused finisher must not write pins after another service completes and admits work.
#[test]
fn balancing_reroute_stale_finisher_preserves_live_pins() {
    let b = Fake::default();
    let root =
        std::env::temp_dir().join(format!("taskq-stale-finisher-{}", crate::runtime::uuid()));
    let mut c = cfg("mac");
    c.root = root.clone();
    c.install = root.join("install");
    c.limits.insert("codex".into(), 1);
    c.limits.insert("antigravity".into(), 1);
    let n = b
        .create("Reroute", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    b.comment(n, "taskq: reroute claude codex\nrouting-fence: 1")
        .unwrap();
    let exits: Exits = Default::default();
    let (other, launched) = (c.clone(), exits.clone());
    b.state.borrow_mut().before_labels = Some(Box::new(move |b, n| {
        let mut second = service(b, &other, launched);
        second.focus = vec![];
        second.pass().unwrap();
        let pending = second
            .snapshot
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|r| r.n == n)
            .unwrap()
            .admission
            .as_ref()
            .unwrap();
        assert!(
            pending.reason.contains("reroute"),
            "pending writer must explain why work waits"
        );
        crate::cli::task_cmd::reroute(b, &other.root, "codex", "antigravity").unwrap();
        second.focus = vec!["all".into()];
        second.pass().unwrap();
    }));
    let receipt = crate::dispatch::finish_reroute(&b, n);
    let p = core::project(n, &crate::board::entries(&b, n).unwrap(), false);
    let pins: Vec<_> = b
        .issue(n)
        .unwrap()
        .labels
        .into_iter()
        .filter(|l| l.starts_with("run-"))
        .collect();
    assert_eq!(
        pins,
        [format!("run-{}", rt_of(&p.task).map_or("codex", |r| r.0))],
        "stale finisher corrupted the live pin"
    );
    assert!(
        receipt.ok(),
        "the sole writer can finish: {}",
        receipt.text()
    );
    assert!(
        exits.borrow().is_empty(),
        "the second service cannot admit while the first can still write"
    );
    crate::cli::task_cmd::reroute(&b, &c.root, "codex", "antigravity").unwrap();
    let mut second = service(&b, &c, exits.clone());
    second.focus = vec!["all".into()];
    second.pass().unwrap();
    assert_eq!(rt_of(&task(&b, n)).unwrap().0, "antigravity");
    assert_eq!(
        b.issue(n)
            .unwrap()
            .labels
            .iter()
            .filter(|l| l.starts_with("run-"))
            .cloned()
            .collect::<Vec<_>>(),
        ["run-antigravity"]
    );
    assert_eq!(exits.borrow().len(), 1);
    assert_eq!(task(&b, n).retries, 0);
    std::fs::remove_dir_all(root).unwrap();
}

// A5/A6: failed quota persistence must fence every project, not select the exhausted default as its own spare.
#[test]
fn balancing_quota_write_failure_fences_other_projects() {
    let root = std::env::temp_dir().join(format!("taskq-quota-save-{}", crate::runtime::uuid()));
    let install = root.join("install");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(
        install.join(crate::runtime::accounts::FILE),
        r#"{"claude":[{"label":"default","enabled":true},{"label":"work","enabled":true}]}"#,
    )
    .unwrap();
    // A directory at the atomic writer's temporary path fails the save after a valid state read.
    let failed = install.join(format!(
        "{}.{}.tmp",
        crate::runtime::accounts::STATE,
        std::process::id()
    ));
    std::fs::create_dir(&failed).unwrap();
    let b = Fake::default();
    let mut c = cfg("mac");
    c.root = root.join("a");
    c.install = install.clone();
    let n = b
        .create("Quota", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:owned mac")
        .unwrap();
    let t = task(&b, n);
    let log = crate::runtime::attempt_log(&c.root, "worker", n, a);
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(
        &log,
        "{\"type\":\"taskq.account\",\"account\":\"default\"}\n",
    )
    .unwrap();
    let mut s = service(&b, &c, Default::default());
    assert!(
        s.account_limit(n, &t, a, "claude", (18000, true), 10)
            .0
            .is_none()
    );
    // Once the failing aggregate path disappears, an unsaved observation must still be fenced.
    std::fs::remove_dir(failed).unwrap();
    let mut other = c.clone();
    other.root = root.join("b");
    let s2 = service(&b, &other, Default::default());
    assert!(
        s2.admit_runtime("claude", false).is_err(),
        "another project must not select the exhausted default"
    );
    assert!(
        crate::runtime::accounts::claim(&install, "claude", None, 10, &root.join("probe")).is_err()
    );
    drop(s);
    drop(s2);
    let mut restarted = service(&b, &other, Default::default());
    restarted.account_pass(10);
    let accounts = crate::runtime::accounts::Host::read(&install);
    assert_eq!(
        accounts.active("claude", 10).as_deref(),
        Some("work"),
        "recover the journal, preserving the exhausted default and spare account: {:?}; {:?}",
        accounts.error,
        restarted.log
    );
    assert_eq!(accounts.state["claude"]["default"].until, Some(18000));
    std::fs::remove_dir_all(root).unwrap();
}

// #2137 A2/A3/A8: real admission and status share routing, disabled pins and account quota evidence.
#[test]
fn balancing_service_routing_and_blockers() {
    let b = Fake::default();
    let root = std::env::temp_dir().join(format!(
        "taskq-balancing-service-{}",
        crate::runtime::uuid()
    ));
    std::fs::create_dir_all(root.join("install")).unwrap();
    let mut c = cfg("mac");
    c.root = root.clone();
    c.install = root.join("install");
    c.limits = BTreeMap::from([
        ("claude".into(), 0),
        ("codex".into(), 1),
        ("antigravity".into(), 1),
    ]);
    c.routing =
        crate::labels::routing(&json!({"routing":[{"labels":["ui"],"prefer":"claude"}]})).unwrap();
    let soft = b.create("UI", "", &["taskq".into(), "ui".into()]).unwrap();
    let pinned = b
        .create(
            "Disabled strict pin",
            "",
            &["taskq".into(), "run-claude".into()],
        )
        .unwrap();
    let quota = b
        .create(
            "Quota strict pin",
            "",
            &["taskq".into(), "run-antigravity".into()],
        )
        .unwrap();
    crate::runtime::accounts::record_windows(
        &c.install,
        "antigravity",
        "default",
        0,
        &[("seven_day".into(), Some(604800))],
    )
    .unwrap();
    let exits: Exits = Default::default();
    let mut s = service(&b, &c, exits.clone());
    s.focus = vec!["all".into()];
    for _ in 0..3 {
        s.pass().unwrap();
    }
    assert_eq!(rt_of(&task(&b, soft)).unwrap().0, "codex");
    assert_eq!(task(&b, pinned).state, State::Ready);
    assert_eq!(task(&b, quota).state, State::Ready);
    assert_eq!(
        exits.borrow().len(),
        1,
        "no repeated failed starts for exhausted accounts"
    );
    let snap = s.snapshot.as_ref().unwrap();
    for (n, code) in [(pinned, "capacity"), (quota, "usage-limit")] {
        assert_eq!(
            snap.tasks
                .iter()
                .find(|r| r.n == n)
                .unwrap()
                .admission
                .as_ref()
                .unwrap()
                .code,
            code
        );
    }
    let report = snap.text(&c, &Value::Null).unwrap();
    assert!(report.contains("claude disabled (limit 0)"), "{report}");
    assert!(
        report.contains("account default window seven_day"),
        "{report}"
    );
    assert_eq!(
        s.cfg.limits, c.limits,
        "quota observations do not edit owner capacity"
    );
    std::fs::remove_dir_all(root).unwrap();
}

// Chain: § 4 step 6 admission -> snapshot rows -> status.json and taskq-board/1 (#1616). Risk: DashQ or the tray
// guess why a ready task waits, or a host that did not decide reads as admitted or refused.
#[test]
fn ready_rows_carry_this_hosts_admission_decision() {
    let b = Fake::default();
    let first = b.create("A", "", &["taskq".into()]).unwrap();
    let second = b.create("B", "", &["taskq".into()]).unwrap();
    let other = b
        .create("H", "", &["taskq".into(), "host-win".into()])
        .unwrap();
    b.create("taskq host win", "version 4.0.0", &[HOST_LABEL.into()])
        .unwrap();
    let win = b
        .create("W", "", &["taskq".into(), "needs-windows".into()])
        .unwrap();
    let bad = b
        .create("X", "", &["taskq".into(), "p1".into(), "p2".into()])
        .unwrap();
    let maps = b
        .create("M", "", &["taskq".into(), "area-maps".into()])
        .unwrap();
    let dep = b
        .create("D", &format!("Depends on #{first}"), &["taskq".into()])
        .unwrap();
    let mut c = cfg("mac");
    c.install = std::env::temp_dir().join(format!("taskq-admission-rows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&c.install);
    std::fs::create_dir_all(&c.install).unwrap();
    let mut s = service(&b, &c, Default::default());
    let codes = |s: &Service| -> BTreeMap<u64, Value> {
        let snap = s.snapshot.as_ref().unwrap();
        snap.tasks
            .iter()
            .filter_map(|r| {
                r.json().get("admission").map(|a| {
                    if a["code"] != "dependency" {
                        assert_eq!(
                            (&a["host"], &a["observed"]),
                            (&json!("mac"), &json!(snap.observed)),
                            "#{} decided by this host on this read",
                            r.n
                        );
                    }
                    (r.n, a["code"].clone())
                })
            })
            .collect()
    };
    s.focus = vec![];
    s.pass().unwrap();
    let want = |pairs: &[(u64, &str)]| {
        pairs
            .iter()
            .map(|(n, c)| (*n, json!(c)))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        codes(&s),
        want(&[
            (first, "focus"),
            (second, "focus"),
            (other, "host"),
            (win, "capability"),
            (bad, "labels"),
            (maps, "focus"),
            (dep, "dependency")
        ]),
        "focus off"
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    assert_eq!(
        codes(&s),
        want(&[
            (first, "admitted"),
            (second, "capacity"),
            (other, "host"),
            (win, "capability"),
            (bad, "labels"),
            (maps, "capacity"),
            (dep, "dependency")
        ]),
        "one claude slot"
    );
    s.focus = vec!["area-maps".into()];
    s.logged_out.insert("claude".into());
    s.pass().unwrap();
    let got = codes(&s);
    assert_eq!(
        (&got[&second], &got[&maps], got.contains_key(&first)),
        (&json!("focus"), &json!("login"), false),
        "a doing task carries no admission"
    );
    std::fs::write(c.install.join("hold"), "fixture hold").unwrap();
    s.pass().unwrap();
    assert_eq!(
        (&codes(&s)[&second], &codes(&s)[&maps]),
        (&json!("hold"), &json!("hold"))
    );
    let _ = std::fs::remove_dir_all(&c.install);
    // the CLI read decides nothing: ready reads unknown with its reason, waiting keeps the fold's dependency
    let rows = crate::snapshot::read(&b, &c, &Value::Null, &[]).json()["tasks"].clone();
    let row = |n: u64| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["n"] == n)
            .unwrap()["admission"]
            .clone()
    };
    assert_eq!(
        (&row(maps)["code"], &row(maps)["reason"], &row(maps)["host"]),
        (&json!("unknown"), &json!("no host decision"), &Value::Null)
    );
    assert_eq!(row(dep)["code"], "dependency");
    assert!(
        row(dep)["reason"]
            .as_str()
            .unwrap()
            .contains(&format!("#{first} open"))
    );
}

// Chain: § 4 Filter, reviewer waits for its runtime's slot -> snapshot row -> status/tray wait text (#1685). Risk: a
// review refused on capacity sits silent in status and the tray, as three GitLab reviews did for two hours.
#[test]
fn review_rows_carry_this_hosts_admission_decision() {
    let b = Fake::default();
    let n = b.create("R", "", &["taskq".into()]).unwrap();
    let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
    accept_execution(&b, n, w, w, None).unwrap();
    release_execution(&b, n, w, w, "exit").unwrap();
    b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40)))
        .unwrap();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 0)]);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    assert!(e.borrow().is_empty(), "no reviewer slot");
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert_eq!(
        (
            &row["state"],
            &row["admission"]["code"],
            &row["admission"]["host"]
        ),
        (&json!("review"), &json!("capacity"), &json!("mac"))
    );
    assert_eq!(
        row["wait"],
        "Waiting for a free slot · claude disabled (limit 0); codex not in limits; antigravity not in limits"
    );
}

// Chain: `host-<name>` label -> heartbeat names -> board admission -> status wait text (R6). Risk (#1721): a task
// labelled `host-host-mac` sits ready for 30 min while status shows plain `ready` on every host.
#[test]
fn host_label_without_heartbeat_shows_its_wait_in_status() {
    let b = Fake::default();
    let orphan = b
        .create("O", "", &["taskq".into(), "host-host-mac".into()])
        .unwrap();
    let other = b
        .create("W", "", &["taskq".into(), "host-win".into()])
        .unwrap();
    b.create("taskq host win", "version 4.0.0", &[HOST_LABEL.into()])
        .unwrap();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 0)]);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    let rows = s.snapshot.as_ref().unwrap().json()["tasks"].clone();
    let row = |n: u64| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["n"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (
            &row(orphan)["admission"]["code"],
            &row(orphan)["admission"]["host"],
            &row(orphan)["wait"]
        ),
        (&json!("host"), &Value::Null, &json!("No host host-mac"))
    );
    assert_eq!(
        row(other)["admission"]["host"],
        "mac",
        "a host that beats keeps this host's own decision"
    );
    assert!(
        row(other)["wait"].is_null(),
        "another host's task is no wait of this host"
    );
    let cli = crate::snapshot::read(&b, &c, &Value::Null, &[]);
    assert_eq!(
        cli.json()["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["n"] == orphan)
            .unwrap()["wait"],
        "No host host-mac",
        "the CLI read decides it too"
    );
    assert!(
        cli.text(&c, &Value::Null)
            .unwrap()
            .contains("| No host host-mac | fix the labels or the host focus |")
    );
}

// Chain: host admission refusal text -> admission code (#1616). Risk: a host.rs or admission.rs refusal reads as
// the wrong code, or an unknown host state reads as a decision.
#[test]
fn host_refusals_map_to_admission_codes() {
    for (code, text, want) in [
        ("max", "host at its maximum of 2 sessions", "capacity"),
        ("spacing", "host launched 3 s ago", "load-gate"),
        (
            "version",
            "service p 4.1.11 is older than 4.2.0-dev",
            "hold",
        ),
        ("host", "admission file unreadable (x); rebuilt", "unknown"),
        ("host", "host occupancy recovery pending", "unknown"),
        ("host", "host admission poisoned; launches held", "unknown"),
        ("host", "project draining", "hold"),
        ("host", "host restart pending", "hold"),
        ("host", "runtime disabled", "capacity"),
        ("host", "round-robin: another project is ready", "capacity"),
        ("host", "host load gate or 30 s launch spacing", "load-gate"),
        ("host", "host or project capacity full", "capacity"),
    ] {
        let (got, reason) = refusal_admission(code, text);
        assert_eq!(got, want, "{code}: {text}");
        if got == "unknown" {
            assert!(crate::snapshot::ADMISSION_UNKNOWN.contains(&reason.as_str()));
        }
    }
}

// Chain: § 8 codex-app admission. Risk: codex-app runs an unlabelled task.
#[test]
fn codex_app_runs_only_labelled_tasks_on_opted_in_hosts() {
    let b = Fake::default();
    let app = b
        .create("A", "", &["taskq".into(), "run-codex-app".into()])
        .unwrap();
    let other = b.create("O", "", &["taskq".into()]).unwrap();
    let mut c = cfg("win");
    c.limits = BTreeMap::from([("codex-app".to_string(), 1)]);
    service(&b, &c, Default::default()).pass().unwrap();
    assert_eq!(
        (task(&b, app).state, task(&b, other).state),
        (State::Ready, State::Ready),
        "no opt-in: unchanged"
    );
    let m = Config {
        has: vec!["codex-app".into()],
        me: "mac".into(),
        ..c.clone()
    };
    service(&b, &m, Default::default()).pass().unwrap();
    let os = cfg!(any(target_os = "macos", windows));
    assert_eq!(
        (task(&b, app).state, task(&b, other).state),
        (if os { State::Doing } else { State::Ready }, State::Ready),
        "only macOS and Windows admit app turns; unlabelled tasks never get the app"
    );
    if os {
        assert!(
            task(&b, app)
                .session
                .unwrap()
                .starts_with("codex-app:- mac")
        );
    }
}

// Chain: § 3 capacity config. Risk: a gate field ignored, invalid limits accepted, or a per-host copy honoured.
#[test]
fn capacity_reads_one_policy_and_validates() {
    let v = json!({"limits": {"claude": 4, "codex": 4}, "load_gate": {"cpu_max": 90},
            "capacity_hosts": {"win": {"limits": {"codex": 2}, "load_gate": {"enabled": true}}}});
    let cap = capacity(&v).unwrap();
    assert_eq!(
        cap.limits,
        BTreeMap::from([("claude".into(), 4), ("codex".into(), 4)])
    );
    assert_eq!(
        cap.gate,
        GateCfg {
            enabled: false,
            cpu_max: 90,
            memory_min: 15,
            mean: false
        }
    );
    assert_eq!(
        capacity(&json!({"limits": {"claude": 1}})).unwrap(),
        Capacity::fixed(BTreeMap::from([("claude".into(), 1)]), GATE_DEFAULT),
        "absent keys: today"
    );
    for bad in [
        json!({"limits": {"claude": -1}}),
        json!({"limits": {"claude": 5_000_000_000u64}}),
        json!({"load_gate": {"cpu_max": 9}}),
        json!({"load_gate": {"memory_min": 96}}),
        json!({"load_gate": {"enabled": "yes"}}),
    ] {
        assert!(capacity(&bad).is_err(), "{bad}");
    }
}

fn on() -> GateCfg {
    GateCfg {
        enabled: true,
        ..GATE_DEFAULT
    }
}

/// Feed CPU busy percentages at 30 s steps from `t`; ticks advance by 100 per sample.
fn feed(g: &mut Gate, t: &mut u64, ticks: &mut (u64, u64), samples: &[(f64, f64)]) {
    for (cpu, mem) in samples {
        *ticks = (ticks.0 + *cpu as u64, ticks.1 + 100);
        *t += 30;
        g.host_mem = Some((8.0, 1));
        g.observe(*t, *t as i64, Some(*ticks), Some(*mem));
    }
}

// Chain: § 3 load gate. Risk: starts under high load or never reopens.
#[test]
fn gate_holds_over_thresholds_and_reopens_after_90_s_calm() {
    let mut g = Gate::default();
    g.configure(GATE_DEFAULT);
    assert!(g.may_launch(0) && g.state(0).0 == "off", "off by default");
    g.configure(on());
    assert_eq!(g.state(0).0, "unknown", "enabled: held until sampled");
    let (mut t, mut k) = (0, (0, 0));
    g.observe(0, 0, Some(k), Some(50.0));
    assert_eq!(
        g.state(0),
        ("unknown", vec![("sample", "no CPU reading yet".into())]),
        "first CPU reading is a baseline"
    );
    feed(&mut g, &mut t, &mut k, &[(50.0, 50.0), (50.0, 50.0)]);
    assert!(!g.may_launch(t) && g.state(t).0 == "held");
    feed(&mut g, &mut t, &mut k, &[(50.0, 50.0)]);
    assert!(g.may_launch(t), "three calm samples: 90 s");
    // exact thresholds hold; both reasons are kept
    feed(&mut g, &mut t, &mut k, &[(85.0, 15.0)]);
    assert_eq!(
        g.state(t).1.iter().map(|r| r.0).collect::<Vec<_>>(),
        ["cpu", "memory"]
    );
    // mid-band (CPU 80 > 85 - 10) resets recovery
    feed(
        &mut g,
        &mut t,
        &mut k,
        &[
            (50.0, 50.0),
            (50.0, 50.0),
            (80.0, 50.0),
            (50.0, 50.0),
            (50.0, 50.0),
        ],
    );
    assert_eq!(g.state(t).0, "held");
    feed(&mut g, &mut t, &mut k, &[(50.0, 50.0)]);
    assert_eq!(g.state(t).0, "open");
    // a stale sample holds; a failed reading holds and resets
    assert_eq!(g.state(t + 61).0, "unknown");
    g.observe(t + 30, 0, Some(k), None);
    assert!(!g.may_launch(t + 30));
    // a threshold change starts held
    let mut g2 = Gate::default();
    g2.configure(on());
    let (mut t2, mut k2) = (0, (0, 0));
    g2.observe(0, 0, Some(k2), Some(50.0));
    feed(&mut g2, &mut t2, &mut k2, &[(10.0, 50.0); 3]);
    assert!(g2.may_launch(t2));
    g2.configure(GateCfg {
        cpu_max: 90,
        ..on()
    });
    assert!(!g2.may_launch(t2));
    // Auto: the 60 s mean smooths one busy interval; a 100 % interval still vetoes
    let mut g3 = Gate::default();
    g3.configure(GATE_AUTO);
    let (mut t3, mut k3) = (0, (0, 0));
    g3.observe(0, 0, Some(k3), Some(50.0));
    feed(&mut g3, &mut t3, &mut k3, &[(70.0, 50.0), (95.0, 50.0)]);
    assert_eq!(g3.last.unwrap().2, Some(82.5));
    feed(&mut g3, &mut t3, &mut k3, &[(100.0, 50.0)]);
    assert_eq!(g3.state(t3).1[0].0, "cpu");
    // Auto memory floor: max(memory_min %, 2.5 GiB) and macOS pressure level 4 hold; no reading is unknown
    feed(&mut g3, &mut t3, &mut k3, &[(10.0, 50.0); 3]);
    assert_eq!(g3.state(t3).0, "open");
    for (m, code) in [
        (Some((2.4, 1)), "memory"),
        (Some((8.0, PRESSURE_CRITICAL)), "memory"),
        (None, "sample"),
    ] {
        g3.host_mem = m;
        g3.observe(t3, t3 as i64, Some(k3), Some(50.0));
        assert_eq!(g3.state(t3).1.last().unwrap().0, code, "{m:?}");
        assert!(!g3.may_launch(t3));
    }
    g2.host_mem = None;
    assert_eq!(g2.state(t2).0, "held", "Fixed ignores the floor reading");
}

// Chain: § 3 load gate spacing. Risk: launch burst or bad counters accepted.
#[test]
fn gate_spaces_launches_and_rejects_bad_counters() {
    let mut g = Gate::default();
    g.configure(on());
    let (mut t, mut k) = (0, (0, 0));
    g.observe(0, 0, Some(k), Some(50.0));
    feed(&mut g, &mut t, &mut k, &[(10.0, 50.0); 3]);
    g.launch(t);
    assert!(!g.may_launch(t + 40), "no sample after the launch");
    feed(&mut g, &mut t, &mut k, &[(10.0, 50.0)]);
    assert!(g.may_launch(t), "post-launch sample, 30 s later");
    g.observe(t + 30, 0, Some((0, 0)), Some(50.0)); // counters went backwards
    assert_eq!(g.state(t + 30).0, "unknown");
}

// Chain: § 3 load gate with live attempts. Risk: the gate drops leases or blocks resumes.
#[test]
fn load_gate_holds_claims_and_resumes_keeps_leases_and_recovers() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 2);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    let m = b
        .create("M", "", &["taskq".into(), "host-mac".into()])
        .unwrap();
    // gate on: held until three calm samples, the running child continues
    s.reload(Ok(Capacity::fixed(c.limits.clone(), on())));
    let (mut t, mut k) = (0, (0, 0));
    s.gate.observe(0, 0, Some(k), Some(50.0));
    feed(&mut s.gate, &mut t, &mut k, &[(90.0, 50.0)]);
    s.mono = t;
    b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    b.comment(n, &core::entry("answer", &["1"], None, "x"))
        .unwrap();
    s.pass().unwrap();
    assert_eq!(
        (e.borrow().len(), task(&b, m).state),
        (1, State::Ready),
        "no resume, no claim while held"
    );
    assert!(
        b.comments(m).unwrap().is_empty(),
        "no spawn written while held"
    );
    let hb = b.issues(None).unwrap().into_iter().find(is_host).unwrap();
    assert!(
        hb.body.lines().any(|l| l == format!("#{n} {a}")),
        "held resume keeps its lease: {}",
        hb.body
    );
    assert_eq!(s.capacity_json()["gate"], "held");
    // other hosts leave a listed lease alone
    let w = cfg("win");
    let mut other = service(&b, &w, Default::default());
    b.advance(400);
    s.pass().unwrap();
    other.pass().unwrap();
    assert_eq!(task(&b, n).attempt, Some(a));
    // recovery: the resume goes first, the claim waits for a post-launch sample and 30 s
    feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0); 3]);
    s.mono = t;
    s.pass().unwrap();
    assert_eq!(
        (e.borrow().len(), task(&b, m).state),
        (2, State::Ready),
        "one launch per 30 s"
    );
    feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0)]);
    s.mono = t;
    s.pass().unwrap();
    assert_eq!(task(&b, m).state, State::Doing);
    assert_eq!(s.capacity_json()["used"]["claude"], 2);
    // invalid config holds and reports, the children continue
    s.reload(Err("limits.claude: not an integer".into()));
    assert_eq!(s.capacity_json()["reasons"][0]["code"], "config");
    assert_eq!(s.kids.len(), 2);
}

// Chain: resource policy write (shared-resource-controls.md § Host adapter) -> lower limit -> service reload ->
// pass. Risk: a lowered limit stops or drops the lease of an attempt that is already running.
#[test]
fn lowered_limit_keeps_running_attempts() {
    let b = Fake::default();
    let n = b.create("T", "", &["taskq".into()]).unwrap();
    let m = b.create("M", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 2);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    let (a, am) = (task(&b, n).attempt, task(&b, m).attempt);
    let o = b.create("O", "", &["taskq".into()]).unwrap();
    s.reload(Ok(Capacity::fixed(
        BTreeMap::from([("claude".to_string(), 0)]),
        GATE_DEFAULT,
    )));
    b.advance(400);
    s.pass().unwrap();
    assert_eq!(
        (s.kids.len(), task(&b, n).state, task(&b, m).state),
        (2, State::Doing, State::Doing),
        "both attempts run on"
    );
    assert_eq!(
        (task(&b, n).attempt, task(&b, m).attempt),
        (a, am),
        "leases kept"
    );
    assert_eq!(
        task(&b, o).state,
        State::Ready,
        "no new claim under the lower limit"
    );
}

// Chain: § 9 Auto admission config. Risk: Auto silently off, or a cap or invalid mode accepted wrong.
#[test]
fn auto_admission_takes_d03_defaults_and_caps() {
    let a = capacity(&json!({"admission": {"mode": "auto", "max": 6}, "limits": {"codex": 2}}))
        .unwrap();
    assert_eq!((a.auto, a.max, a.gate), (true, Some(6), GATE_AUTO));
    let tuned =
        capacity(&json!({"admission": {"mode": "auto"}, "load_gate": {"cpu_max": 80}})).unwrap();
    assert_eq!(
        (
            tuned.gate.cpu_max,
            tuned.gate.memory_min,
            tuned.gate.enabled
        ),
        (80, 10, true),
        "a gate field overrides one default"
    );
    assert!(
        !capacity(&json!({})).unwrap().auto,
        "absent: Fixed, today's behavior"
    );
    for bad in [
        json!({"admission": "auto"}),
        json!({"admission": {"mode": "fast"}}),
        json!({"admission": {"max": -1}}),
        json!({"admission": {"mode": "auto"}, "load_gate": {"enabled": false}}),
    ] {
        assert!(capacity(&bad).is_err(), "{bad}");
    }
}

// Chain: § 3 host authority under Auto. Risk: a launch without a reservation, a cap overrun or admission on unknown data.
#[test]
fn auto_service_reserves_holds_at_cap_releases_and_refuses_unknown() {
    let install = std::env::temp_dir().join(format!("taskq-auto-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&install);
    std::fs::create_dir_all(&install).unwrap();
    let b = Fake::default();
    let ns: Vec<u64> = (0..3)
        .map(|i| b.create(&format!("T{i}"), "", &["taskq".into()]).unwrap())
        .collect();
    let mut c = cfg("mac");
    c.install = install.clone();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.reload(capacity(&json!({"admission": {"mode": "auto", "max": 1}})));
    // unknown: no sample yet, nothing claimed
    s.pass().unwrap();
    assert_eq!(
        (s.capacity_json()["gate"].as_str(), e.borrow().len()),
        (Some("unknown"), 0)
    );
    let (mut t, mut k) = (0, (0, 0));
    s.gate.observe(0, 0, Some(k), Some(50.0));
    feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0); 3]);
    (s.mono, s.wall) = (t, t as i64);
    // admit: one reservation, held as a child
    s.pass().unwrap();
    assert_eq!((e.borrow().len(), task(&b, ns[0]).state), (1, State::Doing));
    let cap = s.capacity_json();
    assert_eq!(
        (
            cap["host_used"].as_u64(),
            cap["reservations"][0]["state"].as_str()
        ),
        (Some(1), Some("held"))
    );
    // hold at the host cap of 1: requested with a reason, no spawn written
    feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0); 2]);
    (s.mono, s.wall) = (t, t as i64);
    s.pass().unwrap();
    assert_eq!((e.borrow().len(), task(&b, ns[1]).state), (1, State::Ready));
    let cap = s.capacity_json();
    assert_eq!(
        (cap["gate"].as_str(), cap["reasons"][0]["code"].as_str()),
        (Some("held"), Some("max"))
    );
    assert_eq!(cap["reservations"][1]["state"], "requested");
    // release: the child exits, the next pass admits the next task
    b.comment(
        ns[0],
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            task(&b, ns[0]).attempt,
            "ok",
        ),
    )
    .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    s.sync_host();
    feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0)]);
    (s.mono, s.wall) = (t, t as i64);
    s.pass().unwrap();
    assert_eq!(
        e.borrow().len(),
        2,
        "released slot admits one more (reviewer or next task)"
    );
    // unknown host data: an unreadable host file refuses and is rebuilt
    std::fs::write(install.join("admission.json"), "{").unwrap();
    *e.borrow()[1].1.borrow_mut() = Some(0);
    feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0)]);
    (s.mono, s.wall) = (t, t as i64);
    let before = e.borrow().len();
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), before, "refused on unknown");
    assert_eq!(s.capacity_json()["gate"], "unknown");
    let _ = std::fs::remove_dir_all(&install);
}

// Chain: § 4 history events file. Risk: duplicate events or unbounded growth.
#[test]
fn events_append_dedup_and_trim() {
    let p = std::env::temp_dir().join(format!("taskq-events-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let m = |n, k, a, o: &str| (n, k, a, o.to_string());
    assert_eq!(
        record(&p, 1000, &[m(1, "close", 7, "delivered")]).unwrap(),
        1000
    );
    // #1800: the § 7 outcome follows a close and does not defeat the dedup; other kinds keep 4 fields
    record(
        &p,
        2000,
        &[
            m(1, "close", 7, "delivered"),
            m(2, "failed", 7, ""),
            m(3, "close", 8, "replaced by #4 (ready)"),
        ],
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "since 1000\n1000 #1 close 7 delivered\n2000 #2 failed 7\n2000 #3 close 8 replaced by #4 (ready)\n",
        "same attempt and kind once"
    );
    let late = 1500 + MONTH;
    assert_eq!(record(&p, late, &[]).unwrap(), 1500);
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "since 1500\n2000 #2 failed 7\n2000 #3 close 8 replaced by #4 (ready)\n",
        "older than 30 days dropped"
    );
    let _ = std::fs::remove_file(&p);
}

// Chain: section 7 close without merge -> task PR closed with the close reason; requeue, later and failed keep it
// open; an unknown PR state is logged and left open.
#[test]
fn only_a_closed_task_closes_its_open_pr() {
    for fail in [false, true] {
        let (b, c) = (Fake::default(), cfg("win"));
        let ns: Vec<u64> = ["close", "requeue", "later", "failed"]
            .iter()
            .map(|t| {
                let n = b.create(t, "", &["taskq".into()]).unwrap();
                b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
                b.comment(n, "taskq: later\nparked").unwrap();
                n
            })
            .collect();
        b.state.borrow_mut().fail_task_prs = fail;
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process")));
        s.pass().unwrap();
        b.comment(ns[0], "taskq: close\nsuperseded by #9").unwrap();
        for (n, kind) in ns[1..].iter().zip(["requeue", "later", "failed"]) {
            b.comment(*n, &format!("taskq: {kind}\nwork may resume"))
                .unwrap();
        }
        s.pass().unwrap();
        s.pass().unwrap();
        assert!(!b.issue(ns[0]).unwrap().open);
        assert_eq!(
            b.state.borrow().closed_task_prs,
            [(ns[0], "superseded by #9".to_string())],
            "once, closed task only"
        );
        // #1800: the close event names the successor and its state (#9 is not on the board)
        assert!(
            s.log
                .iter()
                .any(|l| l.starts_with(&format!("#{} event close ", ns[0]))
                    && l.ends_with(" replaced by #9 (unknown)")),
            "{:?}",
            s.log
        );
        assert_eq!(
            s.log.iter().any(|l| l
                == &format!(
                    "#{} task PR left open, state unknown: PR state unknown",
                    ns[0]
                )),
            fail,
            "{:?}",
            s.log
        );
    }
}

// Chain: § 4 Profiling, #2241 worker ask -> `taskq-transition` record with kind and cause -> profile counts per day.
// Risk: an ask is not counted, counted under the wrong cause, or a non-ask move carries ask fields.
#[test]
fn a_worker_ask_records_its_kind_and_cause() {
    let b = Fake::default();
    let x = b.create("X", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 1);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    let _ = crate::profile::tests::take_moves();
    let ax = task(&b, x).attempt.unwrap();
    let card = "Done: ran the checks\nProblem: the rule gate does not name §4\n1. Name it - + passes; - edit ★\n2. Drop it - + small; - loses\nTo answer: taskq answer 1.1";
    b.comment(x, &core::entry("ask", &[], Some(ax), card))
        .unwrap();
    s.pass().unwrap();
    let (records, report) = crate::profile::tests::take_moves();
    let r: Vec<serde_json::Value> = records
        .lines()
        .map(|l| serde_json::from_str(l.strip_prefix("taskq-transition ").unwrap()).unwrap())
        .collect();
    let ask: Vec<_> = r.iter().filter(|v| v["to"] == "ask").collect();
    assert_eq!(ask.len(), 1, "{records}");
    assert_eq!(
        ask[0]["counts"],
        serde_json::json!({"ask.technical.rule-gate": 1}),
        "{records}"
    );
    assert!(
        r.iter()
            .filter(|v| v["to"] != "ask")
            .all(|v| v.get("counts").is_none()),
        "{records}"
    );
    assert!(
        report.contains("Worker asks: per UTC day")
            && report.contains("| technical | rule-gate | 1 |"),
        "{report}"
    );
    for (text, want) in [
        (
            "Problem: unknown term: R99\n1. a",
            ("technical", "unknown-term"),
        ),
        (
            "retry limit 3 reached by this requeue",
            ("owner", "retry-limit"),
        ),
        ("Problem: usage limit hit\n1. wait", ("technical", "budget")),
        (
            "Problem: the task conflicts with taskq.md R2\n1. a",
            ("owner", "spec-conflict"),
        ),
        ("Problem: log in to the board", ("owner", "other")),
    ] {
        assert_eq!(crate::profile::ask_of(text), want, "{text}");
    }
}

// Chain: #1173 logged state change -> pass replay -> `taskq-transition` record -> report. Risk: a transition
// missed, doubled across passes or a restart, recorded by another host's service, or a zero entry time.
#[test]
fn a_pass_records_each_logged_transition_once() {
    let b = Fake::default();
    let (x, y) = (
        b.create("X", "", &["taskq".into()]).unwrap(),
        b.create("Y", "", &["taskq".into()]).unwrap(),
    );
    let z = b.create("Z", "", &["taskq".into()]).unwrap();
    b.comment(z, &core::entry("hold", &[], None, "")).unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 1);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let _ = crate::profile::tests::take_moves();
    let ax = task(&b, x).attempt.unwrap();
    // another host claims y: its service records that move, not this one
    b.comment(
        y,
        &core::entry("spawn", &["worker", "codex:s", "win"], None, ""),
    )
    .unwrap();
    b.comment(
        x,
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(ax),
            "ok",
        ),
    )
    .unwrap();
    // the result and the requeue happen between two passes: both are recorded; the requeue left `review` without
    // an attempt, so every service records it without host
    b.comment(x, &core::entry("requeue", &[], None, "again"))
        .unwrap();
    // `held` is outside the queue: neither `hold` nor this `start` is a transition
    b.comment(z, &core::entry("start", &[], None, "")).unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    let created = |id: u64| {
        b.state
            .borrow()
            .comments
            .iter()
            .find(|(_, c)| c.id == id)
            .unwrap()
            .1
            .created
    };
    let ids: Vec<u64> = core::changes(x, &entries(&b, x).unwrap())
        .iter()
        .map(|e| e.0)
        .collect();
    let (records, report) = crate::profile::tests::take_moves();
    let r: Vec<serde_json::Value> = records
        .lines()
        .map(|l| serde_json::from_str(l.strip_prefix("taskq-transition ").unwrap()).unwrap())
        .collect();
    let row = |v: &serde_json::Value| {
        (
            v["task"].as_u64().unwrap(),
            v["from"].as_str().unwrap().to_string(),
            v["to"].as_str().unwrap().to_string(),
            v["prev_s"].as_u64(),
            v["host"].as_str().map(String::from),
        )
    };
    let mac = Some("mac".to_string());
    // the issue creation is the entry into ready: no new task shows `no entry time` (#1344)
    let opened = b.state.borrow().issues[x as usize - 1].created;
    assert_eq!(
        r.iter().map(row).collect::<Vec<_>>()[..3],
        [
            (
                x,
                "ready".into(),
                "doing".into(),
                Some((created(ids[0]) - opened) as u64),
                mac.clone()
            ),
            (
                x,
                "doing".into(),
                "review".into(),
                Some((created(ids[1]) - created(ids[0])) as u64),
                mac.clone()
            ),
            (
                x,
                "review".into(),
                "ready".into(),
                Some((created(ids[2]) - created(ids[1])) as u64),
                None
            )
        ],
        "{records}"
    );
    assert!(
        r.iter().all(|v| v["from"] != "held" && v["to"] != "held"),
        "{records}"
    );
    assert!(
        r.iter().all(|v| v["task"] != y)
            && r[..2].iter().all(|v| v["rt"] == "claude")
            && r[2]["rt"].is_null(),
        "{records}"
    );
    assert!(
        report.contains("| doing | review | claude | 1 |")
            && report.contains("| ready | doing | claude |"),
        "{report}"
    );
    // the next pass and a restarted service record none of them again
    s.pass().unwrap();
    let again = crate::profile::tests::take_moves().0;
    assert!(!again.contains("\"to\":\"review\""), "{again}");
    let mut restarted = service(&b, &c, Default::default());
    restarted.pass().unwrap();
    restarted.pass().unwrap();
    assert!(
        !crate::profile::tests::take_moves()
            .0
            .contains("\"to\":\"review\"")
    );
}

// Chain: #1344 ready -> doing -> review -> deferred publish -> merge queue -> close, replayed by a pass. Risk: review
// time absorbs the merge queue wait, the reviewer's runtime and host are lost, or a state has no entry time.
#[test]
fn a_pass_splits_review_from_the_merge_queue_wait() {
    let b = Fake::default();
    let x = b.create("X", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 1);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let _ = crate::profile::tests::take_moves();
    let aw = task(&b, x).attempt.unwrap();
    b.comment(
        x,
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(aw),
            "ok",
        ),
    )
    .unwrap();
    let ar = b
        .comment(
            x,
            &core::entry("spawn", &["reviewer", "codex:r", "mac"], None, ""),
        )
        .unwrap();
    b.comment(x, &core::entry("publish", &[], Some(ar), "PR 1"))
        .unwrap();
    b.comment(x, &core::entry("enqueued", &["MQ_1"], None, ""))
        .unwrap();
    b.comment(x, &core::entry("close", &[], None, "merged"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    let (records, report) = crate::profile::tests::take_moves();
    let r: Vec<serde_json::Value> = records
        .lines()
        .map(|l| serde_json::from_str(l.strip_prefix("taskq-transition ").unwrap()).unwrap())
        .collect();
    let row = |v: &serde_json::Value| {
        format!(
            "{} {} {} {} {}",
            v["from"].as_str().unwrap(),
            v["to"].as_str().unwrap(),
            v["rt"].as_str().unwrap_or("-"),
            v["host"].as_str().unwrap_or("-"),
            v["prev_s"].is_u64()
        )
    };
    assert_eq!(
        r.iter().map(row).collect::<Vec<_>>(),
        [
            "ready doing claude mac true",
            "doing review claude mac true",
            "review merge-queue codex mac true",
            "merge-queue closed - - true"
        ],
        "{records}"
    );
    assert!(
        report.contains("| review | merge-queue | codex |")
            && report.contains("| merge-queue | closed |"),
        "{report}"
    );
    assert!(!report.contains("no entry time"), "{report}");
}

// Chain: § 4 history across restart. Risk: a restart loses or doubles events.
#[test]
fn a_pass_records_close_and_failed_and_a_restart_keeps_them() {
    let root = std::env::temp_dir().join(format!("taskq-ev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let b = Fake::default();
    let (x, y) = (
        b.create("X", "", &["taskq".into()]).unwrap(),
        b.create("Y", "", &["taskq".into()]).unwrap(),
    );
    let mut c = cfg("mac");
    c.root = root.clone();
    c.limits.insert("claude".into(), 2);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.pass().unwrap();
    let (ax, ay) = (task(&b, x).attempt.unwrap(), task(&b, y).attempt.unwrap());
    b.comment(
        x,
        &core::entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(ax),
            "ok",
        ),
    )
    .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    let r = task(&b, x).attempt.unwrap();
    b.comment(x, &core::entry("close", &[], Some(r), "ok"))
        .unwrap();
    b.close(x).unwrap();
    b.comment(y, &core::entry("failed", &[], None, "no way"))
        .unwrap();
    s.pass().unwrap();
    let ev = std::fs::read_to_string(root.join(".taskq/events")).unwrap();
    let lines: Vec<&str> = ev
        .lines()
        .skip(1)
        .map(|l| l.split_once(' ').unwrap().1)
        .collect();
    // #1800: a close line ends with its § 7 outcome (verdict `ok`: delivered)
    assert_eq!(
        lines,
        [
            format!("#{x} close {r} delivered"),
            format!("#{y} failed {ay}")
        ],
        "{ev}"
    );
    let mut again = service(&b, &c, Default::default());
    again.pass().unwrap();
    again.pass().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".taskq/events")).unwrap(),
        ev,
        "a restart keeps history"
    );
    assert!(again.events_since.is_some());
    // a new task enters waiting (no comment: the issue number), a worker asks (its attempt)
    let z = b
        .create("Z", &format!("Depends on #{y}"), &["taskq".into()])
        .unwrap();
    let w = b.create("W", "", &["taskq".into()]).unwrap();
    again.pass().unwrap();
    let aw = task(&b, w).attempt.unwrap();
    let q1 = b
        .comment(
            w,
            &core::entry("ask", &[], Some(aw), "Which?\n1. x\n2. y ★"),
        )
        .unwrap();
    again.pass().unwrap();
    again.pass().unwrap();
    // #1712: the answer resumes the same attempt; its second ask is a second event, not a duplicate of the first
    b.comment(w, &core::entry("answer", &["1"], None, ""))
        .unwrap();
    again.pass().unwrap();
    let q2 = b
        .comment(
            w,
            &core::entry("ask", &[], Some(aw), "Where?\n1. x\n2. y ★"),
        )
        .unwrap();
    again.pass().unwrap();
    again.pass().unwrap();
    let ev = std::fs::read_to_string(root.join(".taskq/events")).unwrap();
    let lines: Vec<&str> = ev
        .lines()
        .skip(3)
        .map(|l| l.split_once(' ').unwrap().1)
        .collect();
    assert_eq!(
        lines,
        [
            format!("#{z} blocked {z}"),
            format!("#{w} ask {q1}"),
            format!("#{w} ask {q2}")
        ],
        "{ev}"
    );
    // a `gone` entry after the first pass is one `gone` event, once
    let v = b.create("V", "", &["taskq".into()]).unwrap();
    again.pass().unwrap();
    let av = task(&b, v).attempt.unwrap();
    b.comment(v, &core::entry("gone", &[], Some(av), "lost"))
        .unwrap();
    again.pass().unwrap();
    again.pass().unwrap();
    let ev = std::fs::read_to_string(root.join(".taskq/events")).unwrap();
    assert_eq!(
        ev.lines()
            .filter(|l| l.ends_with(&format!("#{v} gone {av}")))
            .count(),
        1,
        "{ev}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// A4/A5: real event-file observation -> host row -> remote snapshot -> committed telemetry.
// Risk: fresh leases hide an unbounded tool, notices repeat, or monitoring releases/replays work.
#[test]
fn progress_health_lifecycle_captures_once_recovers_and_keeps_execution_fenced() {
    use std::fs;
    struct Observed(PathBuf);
    impl Proc for Observed {
        fn progress_source(&self) -> Result<(PathBuf, u64), String> {
            Ok((self.0.clone(), 0))
        }
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(true)
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(None)
        }
        fn kill(&mut self) -> Result<(), String> {
            panic!("monitor cannot stop work")
        }
        fn id(&self) -> u32 {
            0
        }
    }
    let dir = std::env::temp_dir().join(format!("taskq-health-{}", crate::runtime::uuid()));
    fs::create_dir_all(&dir).unwrap();
    let (capture, guard) = crate::profile::tests::collecting(&dir);
    crate::profile::init(&dir);
    let b = Fake::default();
    b.state.borrow_mut().clock = 2_000_000_000;
    let n = b
        .create("Invented unbounded tool", "", &[TASK_LABEL.into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker antigravity:invented mac")
        .unwrap();
    accept_execution(&b, n, a, a, None).unwrap();
    let path = dir.join("native.log");
    let write = |text: &str| {
        fs::write(&path, text).unwrap();
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(b.now().unwrap() as u64);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(at))
            .unwrap();
    };
    let active = "{\"event\":\"step_update\",\"step_update\":{\"conversation_id\":\"invented\",\"step_index\":90,\"state\":\"ACTIVE\",\"step_type\":\"tool\"}}\n";
    write(active);
    let mut c = cfg("mac");
    c.root = dir.join("project");
    c.install = dir.clone();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("monitor cannot replay work")),
    );
    // Fake board time is deterministic; the wall budget is not under parallel suite load (#2279).
    s.progress_clock = |b| b.now().unwrap();
    s.progress_budget = std::time::Duration::from_secs(60);
    s.adopt(Kid {
        execution: a,
        task: n,
        attempt: a,
        rt: "antigravity".into(),
        proc: Box::new(Observed(path.clone())),
        unknown: None,
        stopping: false,
        session_url: None,
    })
    .unwrap();
    s.pass().unwrap();
    s.pass().unwrap();
    // R6/R12, #2180: a hot source can advance beyond the pass-start snapshot.
    let sampled = b.now().unwrap() + 5;
    s.progress_clock = |b| b.now().unwrap() + 5;
    use std::io::Write;
    let mut hot = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(hot,"{}",json!({"timestamp":crate::time::stamp(sampled),"event":"step_update","step_update":{"conversation_id":"invented","step_index":91,"state":"DONE","step_type":"agent_response"}})).unwrap();
    hot.set_times(
        fs::FileTimes::new()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(sampled as u64)),
    )
    .unwrap();
    // #2279: the 50 ms wall budget can cut the scan under full-suite load; a cut scan is unknown and resumes, never lost.
    s.progress_budget = std::time::Duration::ZERO;
    s.pass().unwrap();
    let progress = &s.progress[&(n, a, a)].1;
    assert_eq!(
        (&progress["status"], &progress["reason"]),
        (&json!("unknown"), &json!("native progress scan pending")),
        "{progress}"
    );
    s.progress_budget = std::time::Duration::from_secs(60);
    s.pass().unwrap();
    let progress = &s.progress[&(n, a, a)].1;
    assert_eq!(
        progress["status"], "active",
        "valid source appended after pass start is not future: {progress}"
    );
    assert_eq!(s.progress[&(n, a, a)].1["last_event"], sampled);
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert!(
        row["heartbeat"]["progress"]["observed"].as_i64().unwrap()
            > s.snapshot.as_ref().unwrap().observed
    );
    assert!(
        row["heartbeat"]["warning"].is_null(),
        "fresh local progress must not be re-aged against pass start"
    );
    b.advance(5);
    s.progress_clock = |b| b.now().unwrap();
    s.pass().unwrap();
    s.pass().unwrap();
    let beat_time = |b: &Fake| {
        b.issues(None)
            .unwrap()
            .into_iter()
            .find(|i| i.title == "taskq host mac")
            .unwrap()
            .updated
    };
    let before = beat_time(&b);
    b.advance(10);
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(
        beat_time(&b),
        before,
        "quiet progress observations do not burn a heartbeat write every pass"
    );
    b.advance(901);
    s.pass().unwrap();
    s.pass().unwrap();
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert_eq!(row["heartbeat"]["freshness"], "fresh");
    assert_eq!(row["heartbeat"]["progress"]["status"], "overdue");
    assert_eq!(row["stuck"], true);
    assert!(
        crate::profile::stall(&row, b.now().unwrap() as u64)
            .unwrap()
            .contains("tool progress overdue")
    );
    let (v, h) = views(&b, &mut CommentCache::new(), &mut DepCache::new(), c.ttl).unwrap();
    let remote = crate::snapshot::from_views("board", &b, &c, b.now().unwrap(), &v, &h)
        .tasks
        .into_iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert_eq!(
        remote["stuck"], true,
        "board consumer sees the same progress stall"
    );
    s.progress_clock = |b| b.now().unwrap() + 5;
    s.pass().unwrap();
    s.pass().unwrap();
    b.advance(5);
    s.progress_clock = |b| b.now().unwrap();
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(
        s.log
            .iter()
            .filter(|l| l.contains("event stuck doing"))
            .count(),
        1
    );
    let kids = std::mem::take(&mut s.kids);
    drop(s);
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("restart cannot replay work")),
    );
    s.progress_clock = |b| b.now().unwrap();
    s.progress_budget = std::time::Duration::from_secs(60);
    for kid in kids {
        s.adopt(kid).unwrap();
    }
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(
        s.log
            .iter()
            .filter(|l| l.contains("event stuck doing"))
            .count(),
        0,
        "published notice survives service restart"
    );
    // A collector/read failure cannot claim recovery or lose the original overdue episode.
    fs::rename(&path, dir.join("original")).unwrap();
    s.pass().unwrap();
    assert_eq!(s.progress[&(n, a, a)].1["status"], "unknown");
    fs::rename(dir.join("original"), &path).unwrap();
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(
        s.log
            .iter()
            .filter(|l| l.contains("event stuck doing"))
            .count(),
        0,
        "an unreadable source does not start a new overdue episode"
    );
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(f,"{{\"event\":\"step_update\",\"step_update\":{{\"conversation_id\":\"invented\",\"step_index\":90,\"state\":\"DONE\",\"step_type\":\"tool\"}}}}").unwrap();
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(b.now().unwrap() as u64);
    f.set_times(fs::FileTimes::new().set_modified(at)).unwrap();
    s.pass().unwrap();
    s.pass().unwrap();
    let row = s
        .snapshot
        .as_ref()
        .unwrap()
        .tasks
        .iter()
        .find(|r| r.n == n)
        .unwrap()
        .json();
    assert_eq!(row["stuck"], false);
    assert!(row["heartbeat"]["warning"].is_null());
    assert_eq!(row["heartbeat"]["progress"]["status"], "quiet");
    assert_eq!(crate::profile::stall(&row, b.now().unwrap() as u64), None);
    assert_eq!(s.kids.len(), 1);
    assert_eq!(task(&b, n).attempt, Some(a));
    assert!(
        core::executions(n, &entries(&b, n).unwrap())[0]
            .release
            .is_none()
    );
    capture.flush().unwrap();
    let samples = crate::telemetry::report_samples(&dir, &["operation"]).unwrap();
    for op in ["service.progress.overdue", "service.progress.recovered"] {
        let found: Vec<_> = samples
            .iter()
            .filter_map(|s| s.canonical().ok())
            .filter(|v| v["op"] == op)
            .collect();
        assert_eq!(found.len(), 1, "{op} captures one durable transition");
        assert_eq!(
            found[0]["counts"],
            json!({"task":n,"attempt":a,"execution":a})
        );
    }
    drop(s);
    drop(guard);
    drop(capture);
    fs::remove_dir_all(dir).unwrap();
}

// Chain: R6 stuck -> service log. Risk: a stuck task logs every pass, or never again after it moves.
#[test]
fn stuck_task_logs_once_until_it_moves() {
    let b = Fake::default();
    let c = cfg("mac");
    let n = b.create("Parked", "", &["taskq".into()]).unwrap();
    b.comment(n, &core::entry("later", &[], None, "parked"))
        .unwrap();
    let mut s = service(&b, &c, Default::default());
    let lines = |s: &Service| {
        s.log
            .iter()
            .filter(|l| l.starts_with(&format!("#{n} event stuck later ")))
            .count()
    };
    s.pass().unwrap();
    assert_eq!(lines(&s), 0, "a fresh park is not stuck");
    b.advance(crate::snapshot::STUCK_AFTER);
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(lines(&s), 1, "{:?}", s.log);
    assert_eq!(task(&b, n).state, State::Later, "observe only");
    b.comment(n, &core::entry("later", &[], None, "still parked"))
        .unwrap();
    s.pass().unwrap();
    b.advance(crate::snapshot::STUCK_AFTER);
    s.pass().unwrap();
    assert_eq!(lines(&s), 2, "a move rearms the line");
}

// Chain: § 4 Churn -> snapshot rows, status lines, service log (#1905). Risk: a looping task (taskq#1903: 144 notes,
// 64 spawns, 17 accepted) grows its notes unseen, or a normal task with an accepted spawn is flagged.
#[test]
fn churning_task_is_flagged_once_and_a_normal_task_not() {
    let b = Fake::default();
    let c = cfg("mac");
    let park = |title: &str| {
        let n = b.create(title, "", &["taskq".into()]).unwrap();
        b.comment(n, &core::entry("later", &[], None, "parked"))
            .unwrap();
        n
    };
    let (looping, noisy, normal) = (park("Looping"), park("Noisy"), park("Normal"));
    for _ in 0..6 {
        b.comment(looping, "taskq: spawn worker codex:- pds18")
            .unwrap();
    }
    for i in 0..100 {
        b.comment(noisy, &format!("note {i}")).unwrap();
    }
    let spawn = b
        .comment(normal, "taskq: spawn worker codex:- pds18")
        .unwrap();
    b.comment(normal, &format!("taskq: accepted {spawn} attempt {spawn}"))
        .unwrap();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    let lines = crate::snapshot::churn_lines(&s.snapshot.as_ref().unwrap().json());
    assert_eq!(
        lines,
        [
            format!("#{looping} churn: notes 7 · spawns 6 · accepted 0"),
            format!("#{noisy} churn: notes 101 · spawns 0 · accepted 0")
        ]
    );
    let events = |s: &Service| {
        s.log
            .iter()
            .filter(|l| l.contains(" event churn "))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        events(&s),
        [
            format!("#{looping} event churn notes 7 spawns 6 accepted 0"),
            format!("#{noisy} event churn notes 101 spawns 0 accepted 0")
        ]
    );
    assert_eq!(task(&b, looping).state, State::Later, "observe only");
    b.advance(3601);
    s.pass().unwrap();
    let lines = crate::snapshot::churn_lines(&s.snapshot.as_ref().unwrap().json());
    assert_eq!(
        lines,
        [format!("#{noisy} churn: notes 101 · spawns 0 · accepted 0")],
        "spawns older than an hour clear the flag"
    );
    assert_eq!(events(&s).len(), 2, "one line per flag");
}

// Chain: § 2 timed park -> service requeue at the board time (#1823). Risk: a time-only wait respawns at once and
// escalates through the retry limit, a restart loses the time, or a bad time wakes the task.
#[test]
fn timed_park_wakes_at_its_board_time_after_restart() {
    let b = Fake::default();
    b.state.borrow_mut().clock = crate::time::iso("2026-11-05T08:00:00Z").unwrap();
    let c = cfg("mac");
    let timed = b.create("Timed", "", &["taskq".into()]).unwrap();
    b.comment(
        timed,
        &core::entry(
            "later",
            &["until", "2026-11-05T09:00:00Z"],
            None,
            "Reason: 168 qualified hours",
        ),
    )
    .unwrap();
    let bad = b.create("Bad", "", &["taskq".into()]).unwrap();
    b.comment(
        bad,
        &core::entry(
            "later",
            &["until", "2026-02-30T09:00:00Z"],
            None,
            "Reason: bad time",
        ),
    )
    .unwrap();
    let owner = b.create("Owner", "", &["taskq".into()]).unwrap();
    b.comment(
        owner,
        &core::entry(
            "later",
            &["until", "2027-01-01T00:00:00Z"],
            None,
            "Reason: far",
        ),
    )
    .unwrap();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    assert_eq!(
        (task(&b, timed).state, task(&b, timed).attempt),
        (State::Later, None),
        "no spawn before the time"
    );
    assert_eq!(
        task(&b, timed).until.as_deref(),
        Some("2026-11-05T09:00:00Z")
    );
    b.comment(
        owner,
        &core::entry("requeue", &[], None, "Reason: owner wants it now"),
    )
    .unwrap();
    drop(s);
    b.advance(3600);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    s.pass().unwrap();
    s.pass().unwrap();
    let wakes = || {
        b.state
            .borrow()
            .comments
            .iter()
            .filter(|(i, c)| *i == timed && c.body.starts_with("taskq: requeue"))
            .count()
    };
    assert_eq!(wakes(), 1, "one wake after the restart");
    assert!(
        matches!(task(&b, timed).state, State::Ready | State::Doing)
            && task(&b, timed).retries == 0,
        "{:?}",
        task(&b, timed)
    );
    assert!(
        task(&b, owner).attempt.is_some(),
        "an owner requeue overrides the time"
    );
    assert_eq!(
        (task(&b, bad).state, task(&b, bad).until),
        (State::Later, None),
        "a bad time parks untimed"
    );
}

// Chain: § 2 Stages worker park -> timed wake -> next stage ready (#2289). Risk: a finished stage parks the task in
// `later` until a person requeues it, or the wake drops the recorded stages.
#[test]
fn stage_boundary_continues_without_a_human_step() {
    let b = Fake::default();
    b.state.borrow_mut().clock = crate::time::iso("2026-11-03T10:00:00Z").unwrap();
    let c = cfg("mac");
    let n = b
        .create("Staged", "## Acceptance\nA1. a\nA2. b\n", &["taskq".into()])
        .unwrap();
    let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
    // the entry `later --stage A1` writes for a worker without --until or --blocker
    b.comment(
        n,
        &core::entry(
            "later",
            &["until", "2026-11-03T10:00:00Z"],
            Some(w),
            "Reason: A1 done\nStages: A1",
        ),
    )
    .unwrap();
    assert_eq!(task(&b, n).state, State::Later);
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    let t = task(&b, n);
    assert!(matches!(t.state, State::Ready | State::Doing), "{t:?}");
    assert_eq!(
        (t.stages, t.retries),
        (vec!["A1".to_string()], 0),
        "the next attempt continues at A2"
    );
    assert!(
        !b.state.borrow().comments.iter().any(|(i, c)| *i == n
            && c.body.starts_with("taskq: requeue")
            && !c.body.contains("Timed park ended")),
        "only the service requeued"
    );
}

// Chain: R6 stuck review -> service log (#1752). Risk: a review no host can start sits silent for hours.
#[test]
fn review_without_reviewer_logs_stuck_once() {
    let b = Fake::default();
    let n = b.create("R", "", &["taskq".into()]).unwrap();
    let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
    accept_execution(&b, n, w, w, None).unwrap();
    release_execution(&b, n, w, w, "exit").unwrap();
    b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40)))
        .unwrap();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 0)]);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    let lines = |s: &Service| {
        s.log
            .iter()
            .filter(|l| l.starts_with(&format!("#{n} event stuck review ")))
            .count()
    };
    s.pass().unwrap();
    assert_eq!(lines(&s), 0, "a fresh review is not stuck");
    b.advance(crate::snapshot::STUCK_AFTER);
    s.pass().unwrap();
    s.pass().unwrap();
    assert!(e.borrow().is_empty(), "no reviewer slot");
    assert_eq!(lines(&s), 1, "{:?}", s.log);
    assert_eq!(task(&b, n).state, State::Review, "observe only");
}

#[test]
fn new_worktrees_get_a_private_target_seed() {
    // Chain: first worker start -> worktree + cloned target/debug. Risk: a seed that writes into or shares its source.
    let base = std::env::temp_dir().join(format!("taskq-seed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (origin, root) = (base.join("origin"), base.join("root"));
    let git = |dir: &std::path::Path, args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success(),
            "{args:?}"
        )
    };
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    git(&origin, &["commit", "-q", "--allow-empty", "-m", "init"]);
    git(&base, &["clone", "-q", origin.to_str().unwrap(), "root"]);
    // Defect cs#548 (#2078 A2): without a target/debug no worktree was made, and a worker whose sandbox cannot write
    // the shared .git burned its attempts. Protection: the service creates it, then continues the existing branch.
    assert_eq!(
        seed(&root, 6).as_deref(),
        Some("#6 worktree created"),
        "no target/debug: the worktree still comes first"
    );
    let wt6 = root.join(".worktrees/taskq-6");
    git(&wt6, &["commit", "-q", "--allow-empty", "-m", "wip"]);
    assert_eq!(
        seed(&root, 6),
        None,
        "an existing worktree is continued untouched"
    );
    std::fs::remove_dir_all(&wt6).unwrap();
    assert_eq!(
        seed(&root, 6).as_deref(),
        Some("#6 worktree created"),
        "a deleted folder's stale registration is pruned"
    );
    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&wt6)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "wip",
        "the existing taskq-6 branch is continued, not reset to origin/main"
    );
    std::fs::create_dir_all(root.join("target/debug/deps")).unwrap();
    std::fs::write(root.join("target/debug/deps/libx.rlib"), "warm").unwrap();
    let line = seed(&root, 7).unwrap();
    let copy = root.join(".worktrees/taskq-7/target/debug/deps/libx.rlib");
    assert!(root.join(".worktrees/taskq-7/.git").exists(), "{line}");
    if cfg!(target_os = "macos") {
        assert_eq!(line, "#7 seeded target/debug", "APFS temp dir clones");
    }
    // #2051: where the volume has no reflink (ext4, as on CI), the seed copies plainly instead of skipping.
    if cfg!(target_os = "linux") {
        let probe = std::process::Command::new("cp")
            .arg("--reflink=always")
            .arg(root.join("target/debug/deps/libx.rlib"))
            .arg(base.join("probe"))
            .output()
            .unwrap();
        eprintln!(
            "reflink here: {}",
            if probe.status.success() {
                "supported"
            } else {
                "unsupported, plain-copy path"
            }
        );
        assert_eq!(
            line, "#7 seeded target/debug",
            "a volume without reflink still seeds"
        );
    }
    if line == "#7 seeded target/debug" {
        std::fs::write(&copy, "edited").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("target/debug/deps/libx.rlib")).unwrap(),
            "warm",
            "the seed never writes into its source"
        );
    } else {
        assert!(
            line.starts_with("#7 seed skipped: ") && !copy.exists(),
            "{line}"
        );
    }
    assert_eq!(
        seed(&root, 7),
        None,
        "an existing worktree is continued, never reseeded"
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::create_dir_all(root.join(".worktrees/taskq-5/target/debug/newest")).unwrap();
    if seed(&root, 8).unwrap() == "#8 seeded target/debug" {
        assert!(
            root.join(".worktrees/taskq-8/target/debug/newest").exists(),
            "the newest target/debug is the source"
        );
    }
    // #1856: a failed fetch logs the command and Git's error, never a silent skip; the last fetched origin/main still
    // gives the worker its worktree (#2078)
    std::fs::remove_dir_all(&origin).unwrap();
    let failed = seed(&root, 9).unwrap();
    assert!(
        failed.contains(" (fetch failed: `git -C ")
            && failed.contains("fetch -q origin main` failed: "),
        "{failed}"
    );
    assert!(root.join(".worktrees/taskq-9/.git").exists(), "{failed}");
    let _ = std::fs::remove_dir_all(&base);
}

// Chain: closed task -> execution release -> worktree cleanup. Risk: deleting dirty or still-owned work.
// Defect #1549: other hosts' executions and pre-boot own ones held worktrees forever, logged every restart.
#[test]
fn closed_tasks_lose_clean_worktrees_only() {
    fn no_boot() -> Result<i64, String> {
        Err("boot time not read in tests".into())
    }
    fn booted() -> Result<i64, String> {
        Ok(2_000_000_000)
    }
    let root = std::env::temp_dir().join(format!("taskq-prune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success(),
            "{args:?}"
        )
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "init"]);
    for n in ["1", "2", "3", "4", "5"] {
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("taskq-{n}"),
            &format!(".worktrees/taskq-{n}"),
        ]);
    }
    std::fs::write(root.join(".worktrees/taskq-2/wip"), "x").unwrap();
    let b = Fake::default();
    for _ in 0..5 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    let execution = b
        .comment(4, "taskq: spawn worker claude:owned mac")
        .unwrap();
    b.comment(5, "taskq: spawn worker claude:remote win")
        .unwrap();
    for n in [1, 2, 4, 5] {
        b.close(n).unwrap();
    }
    let mut c = cfg("mac");
    c.root = root.clone();
    let mut s = service(&b, &c, Default::default());
    s.pass().unwrap();
    let wt = |n: &str| root.join(format!(".worktrees/taskq-{n}")).exists();
    assert_eq!(
        (wt("1"), wt("2"), wt("3"), wt("5")),
        (false, true, true, false),
        "clean closed removed; dirty and open stay; another host's execution never ran here"
    );
    let kept = |s: &Service| {
        s.log
            .iter()
            .filter(|l| l.starts_with("#4 worktree kept") && l.contains("reboot this host"))
            .count()
    };
    assert_eq!(
        kept(&s),
        1,
        "undecided own execution names the owner step: {:?}",
        s.log
    );
    let mut restarted = service(&b, &c, Default::default());
    restarted.pass().unwrap();
    assert_eq!(
        kept(&restarted),
        0,
        "a restart with the same outcome logs nothing: {:?}",
        restarted.log
    );
    let cleanup = |boot, force| {
        crate::tidy::Tidy {
            board: &b,
            root: &root,
            me: "mac",
            boot,
            external: false,
            force,
            dry: false,
        }
        .run(&mut |_| false, &|_| None)
        .log
    };
    cleanup(no_boot, true);
    assert_eq!(
        (wt("2"), wt("3")),
        (false, true),
        "force removes dirty closed; open stays"
    );
    assert!(
        wt("4"),
        "even forced cleanup preserves an unresolved execution's worktree"
    );
    let log = cleanup(booted, false);
    assert!(
        !wt("4"),
        "a boot after intent releases and permits cleanup: {log:?}"
    );
    assert!(
        core::executions(4, &crate::board::entries(&b, 4).unwrap())
            .iter()
            .all(|x| x.id != execution || x.release == Some(core::Release::Exit))
    );
    assert!(
        std::fs::read_to_string(root.join(".taskq/trash/taskq-2.patch"))
            .unwrap()
            .contains("wip"),
        "dirty work saved"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: tidy conditions -> `.taskq/findings.json` -> held finding (docs/design/worktrees.md Findings, #2060).
// Defect: hosts repeated the same dirty, stale and kept tidy lines for weeks and nothing filed a task. Kept
// protection: one finding per root and condition after a day, none while it is open, a new day once it is closed,
// and a board without a claim issue logs its reason once.
#[test]
fn tidy_conditions_file_one_finding_per_condition_after_a_day() {
    let root = std::env::temp_dir().join(format!("taskq-tidy-findings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let b = Fake::default();
    b.create("Claims", "", &[]).unwrap();
    let mut c = cfg("mac");
    c.root = root.clone();
    let dirty = BTreeMap::from([("dirty", vec!["#7 worktree dirty, untouched 3d".to_string()])]);
    let mut log = vec![];
    let mut run = |conds: &BTreeMap<&'static str, Vec<String>>, now: u64| {
        log.clear();
        tidy_findings(&b, &c, conds, now, &mut log);
        log.clone()
    };
    let t0 = 1_800_000_000;
    // no claim issue configured: nothing filed, the reason logs once
    assert!(run(&dirty, t0).is_empty());
    let unfiled = run(&dirty, t0 + 86_401);
    assert_eq!(
        unfiled,
        ["tidy finding not filed: configure create_claim_issue before idempotent create"]
    );
    assert!(
        run(&dirty, t0 + 86_402).is_empty(),
        "the same reason logs once"
    );
    std::fs::write(root.join("taskq.json"), r#"{"create_claim_issue": 1}"#).unwrap();
    assert_eq!(
        run(&dirty, t0 + 86_403),
        ["#2 finding filed: tidy dirty over a day"]
    );
    let issue = b.issue(2).unwrap();
    assert!(
        issue.title.starts_with("Task worktree dirty over a day: ") && is_held(&issue),
        "{}",
        issue.title
    );
    assert!(
        issue.body.contains(&format!(
            "## Source\nmac/tidy-dirty/{}-{t0}\n",
            crate::paths::project_name(&root)
        )),
        "{}",
        issue.body
    );
    assert!(
        issue
            .body
            .contains("## Evidence\n- #7 worktree dirty, untouched 3d\n"),
        "{}",
        issue.body
    );
    assert!(
        run(&dirty, t0 + 5 * 86_400).is_empty(),
        "no repeat while the finding is open"
    );
    assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
    // closed while the condition holds: a new day starts, then a second finding
    b.close(2).unwrap();
    let t1 = t0 + 6 * 86_400;
    assert!(run(&dirty, t1).is_empty());
    assert_eq!(
        run(&dirty, t1 + 86_401),
        ["#3 finding filed: tidy dirty over a day"]
    );
    // a cleared condition forgets its first-seen time
    assert!(run(&BTreeMap::new(), t1 + 86_402).is_empty());
    let cache: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".taskq/findings.json")).unwrap())
            .unwrap();
    assert!(cache.get("dirty").is_none(), "{cache}");
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: six-project service passes and CoreQ identity/status polls -> installed bounded runtime -> drained 15-minute intervals -> physical store bytes (A5).
// Risk: unchanged polls write per-poll rows and push 30 days past the 384 MiB universal store cap.
#[test]
fn simulated_six_project_day_fits_a_thirtieth_of_the_store_cap() {
    let dir = std::env::temp_dir().join(format!("taskq-poll-day-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let size = |name: &str| {
        let s = coreq::telemetry::Store::open_bounded(
            &dir.join(name).join("telemetry.log/universal.sqlite3"),
            crate::telemetry::budget(&dir.join(name), "telemetry.sqlite3").unwrap(),
        )
        .unwrap();
        s.reclaim().unwrap();
        s.storage().unwrap()["total_bytes"].as_u64().unwrap()
    };
    std::fs::create_dir_all(dir.join("empty/telemetry.log")).unwrap();
    let empty = size("empty");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "service::tests::simulated_poll_day_child",
            "--ignored",
            "--nocapture",
        ])
        .env("TASKQ_POLL_DAY_ROOT", &dir)
        .env_remove("COREQ_TRACE_CONTEXT")
        .env("TASKQ_INCIDENT_REPORTS", "off")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    // ponytail: CoreQ Reader::open validates the whole store, so a pass costs O(store); 3 of 96 intervals extrapolate within 3% of 6 at a quarter of the time (#1376).
    let day = (size("i") - empty) * 96 / INTERVALS;
    eprintln!(
        "six-project day: {day} stored bytes from {INTERVALS} intervals, limit {}",
        (384u64 << 20) / 30
    );
    assert!(
        day <= (384u64 << 20) / 30,
        "simulated six-project day stores {day} bytes"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

const INTERVALS: u64 = 3;

#[test]
#[ignore = "subprocess helper exercised by simulated_six_project_day_fits_a_thirtieth_of_the_store_cap"]
fn simulated_poll_day_child() {
    let dir = PathBuf::from(std::env::var_os("TASKQ_POLL_DAY_ROOT").unwrap());
    std::fs::create_dir_all(dir.join("i/telemetry.log")).unwrap();
    let _runtime = coreq::telemetry::capture::Runtime::install_bounded(
        &dir.join("i/telemetry.log/universal.sqlite3"),
        "taskq",
        "fixture",
        crate::telemetry::budget(&dir.join("i"), "telemetry.sqlite3").unwrap(),
    )
    .unwrap();
    let boards: Vec<Fake> = (0..6).map(|_| Fake::default()).collect();
    let configs: Vec<Config> = (0..6)
        .map(|p| {
            let mut c = cfg("mac");
            c.repo = format!("o/p{p}");
            (c.root, c.install) = (dir.join(format!("p{p}")), dir.join("i"));
            std::fs::create_dir_all(c.root.join(".taskq")).unwrap();
            c
        })
        .collect();
    for b in &boards {
        b.create("Idle", "", &["taskq".into()]).unwrap();
    }
    let mut services: Vec<Service> = boards
        .iter()
        .zip(&configs)
        .map(|(b, c)| {
            let mut s = service(b, c, Default::default());
            s.focus = vec![];
            s
        })
        .collect();
    let tool = crate::platform::tool(&dir.join("i"));
    // cfg polls every 60 s: a day is 96 intervals of 15 passes; each interval end drains CoreQ's poll windows as its timer does.
    for _ in 0..INTERVALS {
        for _ in 0..15 {
            for (s, c) in services.iter_mut().zip(&configs) {
                s.pass().unwrap();
                coreq::process::identity(std::process::id()).unwrap();
                coreq::status::write(
                    &tool,
                    &c.root,
                    &serde_json::json!({"project": c.repo, "state": "idle"}),
                )
                .unwrap();
            }
        }
        coreq::telemetry::capture::before_exec();
    }
}

// R12/#2337: recovery retains exact persisted executor identity, never a different execution or guessed PID.
#[test]
fn retirement_retains_recovery_pid_and_marks_missing_identity_unknown() {
    let dir = std::env::temp_dir().join(format!("taskq-2337-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let b = Fake::default();
    let mut c = cfg("mac");
    c.root = dir.join("project");
    c.install = dir.clone();
    std::fs::create_dir_all(c.root.join(".taskq")).unwrap();
    let mut s = service(&b, &c, Default::default());
    s.begun.insert(42, ("worker".into(), "codex".into(), None));
    let path = crate::paths::kids_path(&c.install, &c.root).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Invented historical process identity: no live OS process is required for retirement telemetry.
    std::fs::write(
        &path,
        "pid 12340 invented-owner executor-v1\n1 42 codex 12345 invented-start 43 0\n",
    )
    .unwrap();
    for (execution, expected) in [(43, 12345), (44, 0)] {
        s.retired(1, 42, execution, false, 0);
        let events = crate::telemetry::worker_events(&dir);
        let event = events.last().unwrap();
        assert_eq!(event.pid, expected);
        assert_eq!(event.outcome, if expected == 0 { "unknown" } else { "ok" });
        assert_eq!(
            event.reason,
            if expected == 0 {
                "execution-released-pid-unknown"
            } else {
                "execution-released"
            }
        );
    }
    std::fs::remove_file(path).unwrap();
    s.retired(1, 42, 43, false, 0);
    assert_eq!(
        crate::telemetry::worker_events(&dir)
            .last()
            .unwrap()
            .outcome,
        "unknown"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// #2237 Chain: worker lifecycle events -> telemetry universal store -> status readback and anomaly detection (A2, A3).
// Risk: status reports 'Run state unknown' while a worker pid lives, or alive-after-result / running-no-process go unrecorded.
#[test]
fn worker_lifecycle_telemetry_and_status_readback_cover_spawn_exit_result_kill_slots_heartbeat_and_anomalies()
 {
    struct LivePid {
        state: Rc<RefCell<Option<i32>>>,
        pid: u32,
    }
    impl Proc for LivePid {
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(true)
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(*self.state.borrow())
        }
        fn kill(&mut self) -> Result<(), String> {
            *self.state.borrow_mut() = Some(-9);
            Ok(())
        }
        fn id(&self) -> u32 {
            self.pid
        }
    }
    let dir = std::env::temp_dir().join(format!("taskq-2237-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let (_capture, _guard) = crate::profile::tests::collecting(&dir);
    crate::profile::init(&dir);
    let b = Fake::default();
    let mut c = cfg("mac");
    c.root = dir.join("project");
    c.install = dir.clone();
    std::fs::create_dir_all(c.root.join(".taskq")).unwrap();
    let live_pid = std::process::id();
    let proc_state: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let proc_state_spawn = proc_state.clone();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(move |_| {
            Ok(Box::new(LivePid {
                state: proc_state_spawn.clone(),
                pid: live_pid,
            }))
        }),
    );
    s.focus = vec!["all".into()];
    let n = b
        .create("Lifecycle telemetry", "## Goal\ng", &[TASK_LABEL.into()])
        .unwrap();
    // 1. Pass 1: claims and spawns worker -> records slot-hold, spawn, heartbeat.
    s.pass().unwrap();
    let attempt = task(&b, n).attempt.unwrap();
    let events = crate::telemetry::worker_events(&dir);
    for expected in ["slot-hold", "spawn", "heartbeat"] {
        let ev = events
            .iter()
            .find(|e| e.task == n && e.attempt == attempt && e.event == expected)
            .unwrap_or_else(|| panic!("missing {expected} in {events:?}"));
        assert_eq!(ev.host, "mac");
        assert_eq!(ev.runtime, "claude");
        assert_eq!(ev.pid, live_pid);
        assert!(!ev.reason.is_empty());
    }
    // 2. While live_pid lives, even with no host heartbeat (or stale heartbeat), status reads telemetry and never shows 'Run state unknown'.
    let (views_now, _) = views(&b, &mut HashMap::new(), &mut HashMap::new(), c.ttl).unwrap();
    let snap = crate::snapshot::from_views("cli", &b, &c, b.now().unwrap(), &views_now, &[]);
    let row_json = snap.tasks[0].json();
    assert!(
        !row_json.to_string().contains("Run state unknown"),
        "status must not report Run state unknown while worker pid lives: {row_json}"
    );
    assert!(
        snap.tasks[0].execution_wait.is_none(),
        "unexpected execution_wait: {:?}",
        snap.tasks[0].execution_wait
    );
    assert_eq!(snap.tasks[0].executions[0]["state"], "running");
    assert_eq!(snap.tasks[0].executions[0]["process_id"], live_pid);
    // 3. Worker posts result while process (live_pid) stays alive -> pass detects alive-after-result, then kills and releases slot.
    let sha = "0123456789abcdef0123456789abcdef01234567";
    b.comment(
        n,
        &core::entry("result", &[sha], Some(attempt), "Summary: ok"),
    )
    .unwrap();
    s.reread(n);
    s.pass().unwrap();
    let events = crate::telemetry::worker_events(&dir);
    for expected in [
        "result",
        "alive-after-result",
        "kill",
        "exit",
        "slot-release",
    ] {
        let ev = events
            .iter()
            .find(|e| e.task == n && e.attempt == attempt && e.event == expected)
            .unwrap_or_else(|| panic!("missing {expected} in {events:?}"));
        assert_eq!(ev.host, "mac");
        assert_eq!(ev.runtime, "claude");
        assert_eq!(
            ev.pid, live_pid,
            "{expected} must retain executor identity after release"
        );
        assert!(!ev.reason.is_empty());
    }
    assert!(
        events
            .iter()
            .filter(|e| e.task == n && e.attempt == attempt && e.event == "slot-release")
            .all(|e| e.pid == live_pid),
        "duplicate slot release must preserve identity"
    );
    let alive_after = events
        .iter()
        .find(|e| e.task == n && e.event == "alive-after-result")
        .unwrap();
    assert_eq!(alive_after.pid, live_pid);
    assert_eq!(alive_after.reason, "pid-alive-after-result");
    // 4. Anomaly 2: running counts with no process (mac codex running 2, none alive).
    s.reserved.push((101, 201, "codex".into()));
    s.reserved.push((102, 202, "codex".into()));
    s.pass().unwrap();
    let events = crate::telemetry::worker_events(&dir);
    let no_proc: Vec<_> = events
        .iter()
        .filter(|e| e.event == "running-no-process" && e.runtime == "codex")
        .collect();
    assert_eq!(
        no_proc.len(),
        2,
        "expected 2 running-no-process events for codex, got {no_proc:?}"
    );
    assert_eq!(no_proc[0].host, "mac");
    assert_eq!(no_proc[0].pid, 0);
    assert_eq!(no_proc[0].reason, "running-2-alive-0");
    // ponytail: best-effort cleanup; Windows keeps telemetry handles open (os error 32) until process exit.
    drop(s);
    drop(_guard);
    drop(_capture);
    let _ = std::fs::remove_dir_all(&dir);
}

// #2380 Chain: a Claude launch returns only a job ID -> a later agents readback reports the PID -> slot-hold, spawn and
// heartbeat telemetry. Defect: Mac #2374 attempt 6637484100 recorded them with pid 0, which scripts/telemetry.sh trace rejects.
// Risk: an event carries pid 0 again, or the held events are lost when the first PID readback is also the exit.
#[test]
fn worker_lifecycle_telemetry_waits_for_a_delayed_pid_readback() {
    struct LatePid {
        readbacks: u32,
        exit_with_pid: bool,
        pid: u32,
        state: Option<i32>,
    }
    impl Proc for LatePid {
        fn accepted(&mut self) -> Result<bool, String> {
            Ok(true)
        }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            self.readbacks = self.readbacks.saturating_sub(1);
            if self.readbacks == 0 && self.exit_with_pid {
                self.state = Some(0);
            }
            Ok(self.state)
        }
        fn kill(&mut self) -> Result<(), String> {
            self.state = Some(-9);
            Ok(())
        }
        fn id(&self) -> u32 {
            if self.readbacks == 0 { self.pid } else { 0 }
        }
    }
    let live_pid = std::process::id();
    let fixture = |name: &str, readbacks: u32, exit_with_pid: bool| {
        let dir =
            std::env::temp_dir().join(format!("taskq-2380-{name}-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = cfg("mac");
        c.root = dir.join("project");
        c.install = dir.clone();
        std::fs::create_dir_all(c.root.join(".taskq")).unwrap();
        let start: Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>, crate::runtime::StartError>> =
            Box::new(move |_| {
                Ok(Box::new(LatePid {
                    readbacks,
                    exit_with_pid,
                    pid: live_pid,
                    state: None,
                }))
            });
        (dir, c, start)
    };
    let of = |dir: &std::path::Path, n: u64, event: &str| -> Vec<crate::telemetry::WorkerEvent> {
        crate::telemetry::worker_events(dir)
            .into_iter()
            .filter(|e| e.task == n && e.event == event)
            .collect()
    };
    let all_real = |dir: &std::path::Path, n: u64| {
        let events = crate::telemetry::worker_events(dir);
        assert!(
            events
                .iter()
                .filter(|e| e.task == n)
                .all(|e| e.pid == live_pid),
            "every event carries the real PID: {events:?}"
        );
    };

    // 1. The PID arrives at the second readback; the worker then hands in and is stopped.
    let (dir, c, start) = fixture("late", 2, false);
    let (capture, guard) = crate::profile::tests::collecting(&dir);
    let b = Fake::default();
    let mut s = Service::new(&b, &c, "test", start);
    s.focus = vec!["all".into()];
    let n = b
        .create("Late PID", "## Goal\ng", &[TASK_LABEL.into()])
        .unwrap();
    s.pass().unwrap();
    let attempt = task(&b, n).attempt.unwrap();
    assert_eq!(s.kids.len(), 1, "the worker runs while its PID is unknown");
    assert_eq!(s.kids[0].proc.id(), 0);
    assert!(
        crate::telemetry::worker_events(&dir)
            .iter()
            .all(|e| e.task != n),
        "launch records nothing without a PID"
    );
    s.pass().unwrap();
    s.record_heartbeat_events();
    assert_eq!(s.kids[0].proc.id(), 0, "first readback still has no PID");
    let events = crate::telemetry::worker_events(&dir);
    assert!(
        events.iter().all(|e| e.task != n),
        "a renewal records nothing without a PID, and unknown is not running-no-process: {events:?}"
    );
    s.pass().unwrap();
    assert_eq!(s.kids[0].proc.id(), live_pid);
    for event in ["slot-hold", "spawn"] {
        assert_eq!(of(&dir, n, event).len(), 1, "{event} is recorded once");
    }
    assert!(
        !of(&dir, n, "heartbeat").is_empty(),
        "the held heartbeat is recorded with the PID"
    );
    all_real(&dir, n);
    let sha = "0123456789abcdef0123456789abcdef01234567";
    b.comment(
        n,
        &core::entry("result", &[sha], Some(attempt), "Summary: ok"),
    )
    .unwrap();
    s.reread(n);
    s.pass().unwrap();
    for event in [
        "slot-hold",
        "spawn",
        "heartbeat",
        "result",
        "exit",
        "slot-release",
    ] {
        assert!(!of(&dir, n, event).is_empty(), "the trace needs {event}");
    }
    assert_eq!(of(&dir, n, "spawn").len(), 1);
    all_real(&dir, n);
    drop(s);
    drop(guard);
    drop(capture);
    let _ = std::fs::remove_dir_all(&dir);

    // 2. The first readback that reports the PID also reports the exit: the kid leaves, the held events do not.
    let (dir, c, start) = fixture("exit", 1, true);
    let (capture, guard) = crate::profile::tests::collecting(&dir);
    let b = Fake::default();
    let mut s = Service::new(&b, &c, "test", start);
    s.focus = vec!["all".into()];
    let n = b
        .create("Exit with PID", "## Goal\ng", &[TASK_LABEL.into()])
        .unwrap();
    s.pass().unwrap();
    let attempt = task(&b, n).attempt.unwrap();
    s.pass().unwrap();
    for event in ["slot-hold", "spawn", "heartbeat", "exit", "slot-release"] {
        assert!(
            of(&dir, n, event).iter().any(|e| e.attempt == attempt),
            "{event} survives the exit"
        );
    }
    all_real(&dir, n);
    drop(s);
    drop(guard);
    drop(capture);
    let _ = std::fs::remove_dir_all(&dir);

    // 3. Unresolved ownership has no handle: only a saved process identity may name its slot.
    let (dir, c, start) = fixture("unresolved", 1, false);
    let (capture, guard) = crate::profile::tests::collecting(&dir);
    let b = Fake::default();
    let mut s = Service::new(&b, &c, "test", start);
    s.focus = vec!["all".into()];
    let n = b
        .create("Unresolved", "## Goal\ng", &[TASK_LABEL.into()])
        .unwrap();
    s.pass().unwrap();
    let attempt = task(&b, n).attempt.unwrap();
    let execution = s.kids.pop().unwrap().execution;
    s.pass().unwrap();
    assert!(
        s.reserved.iter().any(|r| r.0 == n && r.1 == attempt),
        "the slot stays held"
    );
    assert!(
        of(&dir, n, "slot-hold").is_empty(),
        "no slot-hold without a saved PID"
    );
    s.reserved.clear();
    let path = crate::paths::kids_path(&c.install, &c.root).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Invented historical process identity: no live OS process is required for the record.
    std::fs::write(&path, format!("pid 12340 invented-owner executor-v1\n{n} {attempt} claude 12345 invented-start {execution} 0\n")).unwrap();
    s.pass().unwrap();
    let held = of(&dir, n, "slot-hold");
    assert_eq!(held.len(), 1);
    assert_eq!(
        (held[0].pid, held[0].reason.as_str()),
        (12345, "unresolved-ownership")
    );
    drop(s);
    drop(guard);
    drop(capture);
    let _ = std::fs::remove_dir_all(&dir);
}

// Chain: #2257 replays #2239: win spawns the worker, it is accepted and hands in a result, then win hangs (#2254) with
// the worker execution unreleased; the PM requeues. Risk: mac shows 'Waiting for the agent to accept input' with no live
// agent and no next step, or no eligible host picks the task once win restarts and releases the execution.
#[test]
fn result_without_reviewer_names_the_release_step_and_any_host_takes_it_after_release() {
    for requeue in [false, true] {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let (win, mac) = (cfg("win"), cfg("mac"));
        let mut owner = service(&b, &win, Default::default());
        let mut other = service(&b, &mac, Default::default());
        owner.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        assert!(
            entries(&b, n)
                .unwrap()
                .iter()
                .any(|e| e.body.starts_with(&format!("taskq: accepted {a}"))),
            "accepted"
        );
        b.comment(
            n,
            &core::entry(
                "result",
                &["356b3e3f016c96ff237ca11071c611d9fd7222cd"],
                Some(a),
                "Changed: x\nChecks: y\nOpen: none",
            ),
        )
        .unwrap();
        drop(owner); // win hangs: no further pass releases the worker execution
        let row = |s: &Service| {
            s.snapshot.as_ref().unwrap().json()["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["n"] == n)
                .cloned()
                .unwrap()
        };
        other.pass().unwrap();
        let r = row(&other);
        assert_eq!(
            (r["wait"].as_str(), r["stuck"].as_bool()),
            (Some(crate::snapshot::WAIT_TEXTS[4]), Some(false)),
            "handover within ttl: {r}"
        );
        b.advance(mac.ttl);
        other.pass().unwrap();
        let r = row(&other);
        let step = "start the taskq service on win once, after a reboot if the run had started";
        assert_eq!(
            r["wait"].as_str(),
            Some(crate::snapshot::WAIT_TEXTS[4]),
            "{r}"
        );
        assert_eq!(
            (r["stuck"].as_bool(), r["missing_reviewer"].as_str()),
            (Some(true), Some(step)),
            "{r}"
        );
        let text = other
            .snapshot
            .as_ref()
            .unwrap()
            .text(&mac, &Value::Null)
            .unwrap();
        assert!(
            text.lines().any(|l| l.starts_with("- ")
                && l.contains(&format!(
                    "review stuck 5m; reviewer not started · Next: {step}"
                ))),
            "{text}"
        );
        assert!(!text.contains(crate::snapshot::WAIT_TEXTS[2]), "{text}");
        assert!(
            !entries(&b, n)
                .unwrap()
                .iter()
                .any(|e| e.body.starts_with("taskq: spawn reviewer")),
            "§ 3: reviewer needs the worker execution released"
        );
        if requeue {
            b.comment(
                n,
                &core::entry(
                    "requeue",
                    &[],
                    None,
                    "Review stalled after the win re-exec hang",
                ),
            )
            .unwrap();
            other.pass().unwrap();
            assert_eq!(
                (task(&b, n).state, task(&b, n).attempt),
                (State::Ready, None),
                "fenced until release"
            );
        }
        release_execution(&b, n, a, a, "exit").unwrap(); // win restarted and proved exit
        other.pass().unwrap();
        let t = task(&b, n);
        let spawn = entries(&b, n)
            .unwrap()
            .into_iter()
            .rev()
            .find(|e| e.body.starts_with("taskq: spawn"))
            .unwrap();
        let want = if requeue {
            "taskq: spawn worker claude:"
        } else {
            "taskq: spawn reviewer claude:"
        };
        assert!(
            spawn.body.starts_with(want) && spawn.body.lines().next().unwrap().ends_with(" mac"),
            "requeue={requeue}: {}",
            spawn.body
        );
        assert_eq!(
            (t.state, t.attempt),
            (
                if requeue { State::Doing } else { State::Review },
                Some(spawn.id)
            ),
            "one pass after release"
        );
        assert!(row(&other).get("missing_reviewer").is_none());
    }
}

fn daemon_differs() -> crate::runtime::daemon_sandbox::Fit {
    crate::runtime::daemon_sandbox::Fit::Differs {
        pid: 77,
        reason: "daemon runs under another Seatbelt profile",
    }
}

// Chain: § 4 selection, § 8 Access, #2432 A1 and A2. Risk: a job of another grant joins the live daemon, pinned work
// loses its reason, unpinned work stalls behind a daemon with free Codex beside it, or status never names the daemon.
#[test]
fn a_differing_daemon_holds_pinned_claude_work_and_unpinned_work_takes_another_runtime() {
    use crate::runtime::daemon_sandbox as ds;
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 2), ("codex".to_string(), 2)]);
    c.auto_reroute = true;
    let pinned = b
        .create("Pinned", "", &["taskq".into(), "run-claude".into()])
        .unwrap();
    let free = b.create("Free", "", &["taskq".into()]).unwrap();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.seatbelt.insert("claude".into(), Some("grant".into()));
    ds::fake(Some(daemon_differs()));
    s.pass().unwrap();
    assert_eq!(
        (task(&b, pinned).state, task(&b, pinned).attempt),
        (State::Ready, None),
        "no phantom attempt"
    );
    assert_eq!(task(&b, free).state, State::Doing);
    assert!(
        b.comments(free)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with("taskq: spawn worker codex:")),
        "unpinned took the next runtime"
    );
    let body = s.heartbeat_body();
    let line = body
        .lines()
        .find_map(|l| l.strip_prefix("daemon-sandbox "))
        .expect("A2: the heartbeat names the daemon");
    assert_eq!(
        serde_json::from_str::<Value>(line).unwrap(),
        json!({"bin": "claude", "pid": 77, "reason": "daemon runs under another Seatbelt profile", "tasks": [pinned], "moved": [free]})
    );
    let claude = &s.balancing().runtimes["claude"];
    assert_eq!(
        (claude.code.as_str(), claude.reason.as_str()),
        (
            "capacity",
            "claude: live daemon pid 77: daemon runs under another Seatbelt profile"
        )
    );
    // Unverified holds too, and still leaves other runtimes usable (R12).
    ds::fake(Some(ds::Fit::Unknown("daemon status unreadable".into())));
    s.pass().unwrap();
    assert_eq!(task(&b, pinned).state, State::Ready);
    assert!(s.heartbeat_body().contains(
        r#"daemon-sandbox {"bin":"claude","pid":null,"reason":"daemon status unreadable""#
    ));
    // The daemon is gone: the pinned task starts and the row clears.
    ds::fake(Some(ds::Fit::None));
    s.pass().unwrap();
    assert_eq!(task(&b, pinned).state, State::Doing);
    assert!(!s.heartbeat_body().contains("daemon-sandbox"));
    ds::fake(None);
}

// Chain: R6, § 8 Access, #2494 A1. Risk: a differing daemon moves all unpinned work to another runtime and status stays
// silent because nothing waits; or the row outlives the wait.
#[test]
fn work_moved_off_a_refusing_daemon_stays_named_until_the_wait_ends() {
    use crate::runtime::daemon_sandbox as ds;
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 2), ("codex".to_string(), 2)]);
    let moved = b.create("Moved", "", &["taskq".into()]).unwrap();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.seatbelt.insert("claude".into(), Some("grant".into()));
    ds::fake(Some(daemon_differs()));
    let row = json!({"bin": "claude", "pid": 77, "reason": "daemon runs under another Seatbelt profile", "tasks": [], "moved": [moved]});
    let line = |s: &Service| {
        s.heartbeat_body()
            .lines()
            .find_map(|l| l.strip_prefix("daemon-sandbox "))
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
    };
    s.pass().unwrap();
    assert_eq!(task(&b, moved).state, State::Doing);
    assert_eq!(line(&s).as_ref(), Some(&row), "the first pass names it");
    // Nothing is claimable now, and the wait still lasts.
    s.pass().unwrap();
    assert_eq!(line(&s).as_ref(), Some(&row), "a later pass still names it");
    ds::fake(Some(ds::Fit::None));
    s.pass().unwrap();
    assert_eq!(line(&s), None, "the wait ended");
    ds::fake(None);
}

// Chain: R6, #2494 A1. Risk: status says a daemon moved work that this host never claimed, because another host took
// the task or the claim did not land.
#[test]
fn work_this_host_did_not_claim_never_reads_as_moved_off_a_daemon() {
    use crate::runtime::daemon_sandbox as ds;
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 2), ("codex".to_string(), 2)]);
    let elsewhere = b
        .create("Elsewhere", "", &["taskq".into(), "host-win".into()])
        .unwrap();
    let unclaimed = b.create("Unclaimed", "", &["taskq".into()]).unwrap();
    let mut s = service(&b, &c, Default::default());
    s.seatbelt.insert("claude".into(), Some("grant".into()));
    s.unseen.insert(unclaimed, u64::MAX); // an earlier spawn is still unknown: no new claim
    ds::fake(Some(daemon_differs()));
    s.pass().unwrap();
    assert_eq!(task(&b, elsewhere).state, State::Ready);
    assert_eq!(task(&b, unclaimed).state, State::Ready);
    let body = s.heartbeat_body();
    assert!(
        !body.lines().any(|l| l.starts_with("daemon-sandbox ")),
        "{body}"
    );
    ds::fake(None);
}

// Chain: § 8 Access, #2432 A1. Risk: a held launch or a resume reaches the shared daemon under another grant, or
// waiting consumes the attempt, or a grant that waits never gets its turn.
#[test]
fn a_differing_daemon_keeps_held_launches_and_resumes_waiting() {
    use crate::runtime::daemon_sandbox as ds;
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 2)]);
    let n = b.create("Held", "", &["taskq".into()]).unwrap();
    let calls = Rc::new(RefCell::new(0));
    let called = calls.clone();
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(move |_| {
            *called.borrow_mut() += 1;
            if *called.borrow() == 1 {
                Err(crate::runtime::StartError::admission(
                    "native inventory pending",
                ))
            } else {
                Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))) as Box<dyn Proc>)
            }
        }),
    );
    s.focus = vec!["all".into()];
    s.seatbelt.insert("claude".into(), Some("grant".into()));
    s.pass().unwrap();
    assert_eq!(
        (*calls.borrow(), s.held.len()),
        (1, 1),
        "setup holds the launch"
    );
    let a = task(&b, n).attempt;
    ds::fake(Some(daemon_differs()));
    s.pass().unwrap();
    assert_eq!(
        (
            *calls.borrow(),
            s.held.len(),
            task(&b, n).attempt,
            task(&b, n).retries
        ),
        (1, 1, a, 0),
        "held: no launch, no new attempt"
    );
    assert!(
        s.heartbeat_body().contains(&format!(r#""tasks":[{n}]"#)),
        "A2 names the held job"
    );
    ds::fake(None);
    s.pass().unwrap();
    assert_eq!(
        (*calls.borrow(), s.held.len(), s.kids.len()),
        (2, 0, 1),
        "the same launch continues"
    );

    // A resume waits with its inbox undelivered.
    let b = Fake::default();
    let n = b.create("Resume", "", &["taskq".into()]).unwrap();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.seatbelt.insert("claude".into(), Some("grant".into()));
    s.pass().unwrap();
    let a = task(&b, n).attempt.unwrap();
    b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"))
        .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    s.pass().unwrap();
    ds::fake(Some(daemon_differs()));
    b.comment(n, &core::entry("answer", &["1"], None, "x"))
        .unwrap();
    s.pass().unwrap();
    assert_eq!(
        (task(&b, n).state, e.borrow().len()),
        (State::Doing, 1),
        "no job reaches the differing daemon"
    );
    ds::fake(None);
    s.pass().unwrap();
    assert_eq!(
        e.borrow().len(),
        2,
        "the resume continues once the daemon fits"
    );
}

// Chain: § 8 Access, #2432. Risk: new jobs of the fitting grant keep the daemon alive, so a waiting grant never runs.
#[test]
fn a_waiting_grant_holds_new_starts_of_the_fitting_one() {
    use crate::runtime::daemon_sandbox as ds;
    let b = Fake::default();
    let mut c = cfg("mac");
    c.limits = BTreeMap::from([("claude".to_string(), 2)]);
    let n = b.create("New", "", &["taskq".into()]).unwrap();
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.seatbelt.insert("claude".into(), Some("grant".into()));
    ds::fake(Some(ds::Fit::Fits));
    ds::wait(
        crate::runtime::accounts::default_home("claude").as_deref(),
        Some("other grant"),
    );
    s.pass().unwrap();
    assert_eq!((task(&b, n).state, e.borrow().len()), (State::Ready, 0));
    assert_eq!(
        s.balancing().runtimes["claude"].reason,
        "claude: another grant waits for this daemon to drain"
    );
    assert!(
        !s.heartbeat_body().contains("daemon-sandbox"),
        "the daemon itself fits this project"
    );
    ds::fake(None);
}

// Chain: §5/R12 audit read -> fresh placement -> claim. Risk: a stale pinned view launches after unpin.
#[test]
fn host_unpin_discards_cached_placement_before_admission() {
    let b = Fake::default();
    let c = cfg("mac");
    let n = b
        .create("T", "", &[TASK_LABEL.into(), "host-mac".into()])
        .unwrap();
    let v = views(&b, &mut Default::default(), &mut Default::default(), 300)
        .unwrap()
        .0
        .remove(0);
    let rev = core::revision(n, &entries(&b, n).unwrap());
    assert!(
        crate::dispatch::unpin(
            &b,
            n,
            "mac",
            &crate::dispatch::Op {
                id: Some("invented-ready-unpin".into()),
                revision: Some(rev)
            },
            false
        )
        .ok()
    );
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| panic!("cached placement cannot launch")),
    );
    s.claim(&v, "worker", c.default_runtime.name(), "").unwrap();
    assert_eq!(task(&b, n).state, State::Ready);
    assert!(s.kids.is_empty());
}

// Chain: §5/R2 host unpin -> service pass. Risk: a placement edit stops or replaces the live executor.
#[test]
fn host_unpin_keeps_the_running_executor_and_lease() {
    struct Kept;
    impl Proc for Kept {
        fn exited(&mut self) -> Result<Option<i32>, String> {
            Ok(None)
        }
        fn kill(&mut self) -> Result<(), String> {
            panic!("host unpin must never stop active execution")
        }
        fn id(&self) -> u32 {
            424242
        }
    }
    let b = Fake::default();
    let n = b
        .create("T", "", &[TASK_LABEL.into(), "host-mac".into()])
        .unwrap();
    let c = cfg("mac");
    let starts = Rc::new(std::cell::Cell::new(0));
    let mut s = Service::new(
        &b,
        &c,
        "test",
        Box::new(|_| {
            starts.set(starts.get() + 1);
            Ok(Box::new(Kept))
        }),
    );
    s.focus = vec!["all".into()];
    s.pass().unwrap();
    let attempt = s.kids[0].attempt;
    let execution = s.kids[0].execution;
    let session = task(&b, n).session;
    let rev = core::revision(n, &entries(&b, n).unwrap());
    let r = crate::dispatch::unpin(
        &b,
        n,
        "mac",
        &crate::dispatch::Op {
            id: Some("invented-live-unpin".into()),
            revision: Some(rev),
        },
        false,
    );
    assert!(r.ok(), "{}", r.reason);
    s.pass().unwrap();
    assert_eq!(starts.get(), 1);
    assert_eq!(s.kids.len(), 1);
    assert_eq!(
        (s.kids[0].attempt, s.kids[0].execution, s.kids[0].proc.id()),
        (attempt, execution, 424242)
    );
    assert_eq!(task(&b, n).session, session);
    assert!(
        s.entry()
            .held
            .iter()
            .any(|(task, id, _)| *task == n && *id == attempt)
    );
    assert!(!b.issue(n).unwrap().labels.contains(&"host-mac".into()));
}

// Chain: § 3 Disk guard -> claim, resume and reviewer launch -> status and heartbeat (#2492 A1, A2). Risk: full disks
// kill new reviewers and builds, a held launch loses its inbox, or the guard stops or deletes running work.
// Protection: below the threshold and on an unknown reading nothing new starts with the load gate off; the running
// child, the lease and every file stay; at the threshold everything held starts.
#[test]
fn disk_guard_holds_new_attempts_keeps_running_ones_and_recovers_at_the_threshold() {
    use crate::disk::{Disk, MIN_FREE};
    let b = Fake::default();
    let (asking, running) = (
        b.create("A", "", &["taskq".into()]).unwrap(),
        b.create("R", "", &["taskq".into()]).unwrap(),
    );
    let stays = b.create("S", "", &["taskq".into()]).unwrap();
    let mut c = cfg("mac");
    c.limits.insert("claude".into(), 9);
    let e: Exits = Default::default();
    let mut s = service(&b, &c, e.clone());
    s.disk = Some(Disk::Free(MIN_FREE));
    s.pass().unwrap();
    assert_eq!(e.borrow().len(), 3, "at the threshold all claims start");
    let a = task(&b, asking).attempt.unwrap();
    let r = task(&b, running).attempt.unwrap();
    let new = b.create("N", "", &["taskq".into()]).unwrap();
    b.comment(
        asking,
        &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★"),
    )
    .unwrap();
    *e.borrow()[0].1.borrow_mut() = Some(0);
    b.comment(
        running,
        &core::entry(
            "result",
            &["356b3e3f016c96ff237ca11071c611d9fd7222cd"],
            Some(r),
            "Changed: x\nChecks: y\nOpen: none",
        ),
    )
    .unwrap();
    *e.borrow()[1].1.borrow_mut() = Some(0);
    s.disk = Some(Disk::Free(MIN_FREE - 1));
    s.pass().unwrap();
    b.comment(asking, &core::entry("answer", &["1"], None, "x"))
        .unwrap();
    for (disk, gate) in [
        (Disk::Free(MIN_FREE - 1), "held"),
        (Disk::Unknown, "unknown"),
    ] {
        s.disk = Some(disk);
        s.pass().unwrap();
        assert_eq!(
            e.borrow().len(),
            3,
            "no resume, reviewer or claim: {disk:?}"
        );
        assert!(
            s.kids.iter().any(|k| k.task == stays) && e.borrow()[2].1.borrow().is_none(),
            "the running child continues"
        );
        assert_eq!(task(&b, new).state, State::Ready);
        assert!(b.comments(new).unwrap().is_empty(), "no spawn written");
        assert!(
            !b.comments(running)
                .unwrap()
                .iter()
                .any(|l| l.body.starts_with("taskq: spawn reviewer")),
            "no reviewer written"
        );
        let capacity = s.capacity_json();
        assert_eq!(capacity["gate"], gate);
        assert_eq!(capacity["reasons"][0]["code"], "disk", "{capacity}");
        let hb = b.issues(None).unwrap().into_iter().find(is_host).unwrap();
        assert!(
            hb.body.lines().any(|l| l == format!("#{asking} {a}")),
            "held resume keeps its lease: {}",
            hb.body
        );
    }
    s.disk = Some(Disk::Free(MIN_FREE));
    s.pass().unwrap();
    assert_eq!(s.capacity_json()["disk_free_bytes"], MIN_FREE);
    assert!(e.borrow().len() > 3, "space back: held launches start");
    for _ in 0..3 {
        s.pass().unwrap();
    }
    assert_ne!(task(&b, new).state, State::Ready, "the new claim starts");
    assert!(
        b.comments(running)
            .unwrap()
            .iter()
            .any(|l| l.body.starts_with("taskq: spawn reviewer")),
        "the reviewer starts"
    );
    let bytes = |n: u64| format!("disk {n}");
    assert_eq!(
        stable(&bytes(MIN_FREE)),
        stable(&bytes(MIN_FREE + 5)),
        "free bytes alone force no board write"
    );
    assert_ne!(stable(&bytes(MIN_FREE)), stable(&bytes(MIN_FREE - 1)));
    assert_ne!(stable(&bytes(MIN_FREE)), stable("disk unknown"));
}

// Chain: #2522 A1/A3, R6/R11/R12: runtime dependency disappears -> one pass -> heartbeat and local status.
// Risk: stale native jobs look running, or monitoring submits a pending steer again or releases ownership.
#[test]
fn runtime_2522_one_pass_reports_outages_without_replaying_input() {
    use crate::runtime::health::FAKE;
    let (b, c) = (Fake::default(), cfg("win"));
    std::fs::create_dir_all(&c.root).unwrap();
    let path = c.root.join("invented-runtime.exe");
    std::fs::write(&path, "invented executable fixture").unwrap();
    FAKE.with(|f| *f.borrow_mut() = Some((path.clone(), Ok(Some(77)))));
    let n = b.create("Working", "", &[TASK_LABEL.into()]).unwrap();
    let exits: Exits = Default::default();
    let mut s = service(&b, &c, exits.clone());
    s.pass().unwrap();
    let attempt = task(&b, n).attempt;
    b.comment(n, "taskq: steer\nkeep this pending input")
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    for reason in ["executable missing", "daemon not running"] {
        if reason == "daemon not running" {
            std::fs::write(&path, "invented restored fixture").unwrap();
            FAKE.with(|f| *f.borrow_mut() = Some((path.clone(), Ok(None))));
        }
        s.pass().unwrap();
        assert_eq!(task(&b, n).attempt, attempt);
        assert_eq!(exits.borrow().len(), 1, "no resubmission of worker input");
        let heartbeat = s.heartbeat_body();
        assert!(
            heartbeat.contains(&format!(
                "runtime-fault {}",
                json!({"runtime":"claude","path":path,"reason":reason,"tasks":[n]})
            )),
            "{heartbeat}"
        );
        let observation = s
            .execution_statuses()
            .into_iter()
            .find(|x| x.task == n)
            .unwrap();
        assert_eq!(observation.state, "unknown");
        assert_eq!(observation.reason, reason);
        let text = s.snapshot.as_ref().unwrap().text(&c, &json!({})).unwrap();
        assert!(
            text.contains(&format!("- host win · claude · {reason}")),
            "{text}"
        );
        assert!(
            text.contains(&crate::cli::text::cell(path.to_str().unwrap())),
            "{text}"
        );
        assert!(
            !b.comments(n)
                .unwrap()
                .iter()
                .any(|c| c.body.starts_with("taskq: gone")
                    || c.body.starts_with("taskq: released")
                    || c.body.starts_with("taskq: executing"))
        );
    }
    FAKE.with(|f| *f.borrow_mut() = Some((path.clone(), Ok(Some(77)))));
    s.pass().unwrap();
    assert!(s.runtime_health.faults.is_empty());
    FAKE.with(|f| *f.borrow_mut() = None);
    std::fs::remove_dir_all(&c.root).unwrap();
}
