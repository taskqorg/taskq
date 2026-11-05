    // Chain: native observations -> replay -> continuation. Risk: late, duplicate or contradictory evidence consumes/replays input.
    #[test]
    fn execution_observations_keep_acceptance_and_exit_independent() {
        use super::*;
        let mut log = vec![Entry { id:10, body:"taskq: spawn worker codex:s mac".into(), edited:false }];
        for (id, body, edited, delivered_cursor, release) in [
            (11,"taskq: accepted 10 attempt 9",false,None,None),
            (12,"taskq: accepted 10 attempt 10",true,None,None),
            (13,"taskq: released 10 exit attempt 10\nop-id: release-10",false,None,Some(Release::Exit)),
            (14,"taskq: released 10 no-start attempt 10",false,None,Some(Release::Exit)),
            (15,"taskq: accepted 99 attempt 10",false,None,Some(Release::Exit)),
            (16,"taskq: accepted 10 attempt 10\nop-id: accept-10",false,Some(10),Some(Release::Exit)),
            (17,"taskq: accepted 10 attempt 10\nop-id: accept-10",false,Some(10),Some(Release::Exit)),
        ] {
            log.push(Entry {id,body:body.into(),edited});
            assert_eq!(delivered(7,&log,10),delivered_cursor,"entry {id}");
            assert_eq!(executions(7,&log)[0].release,release,"entry {id}");
            assert_eq!(revision(7,&log),10);
            let mut reversed=log.clone(); reversed.reverse();
            assert_eq!(executions(7,&reversed),executions(7,&log),"transport order is not board order");
        }
        log.extend([
            Entry {id:18,body:"taskq: steer\nnext".into(),edited:false},
            Entry {id:19,body:"taskq: executing 18 attempt 10".into(),edited:false},
            Entry {id:20,body:"taskq: released 19 no-start attempt 10".into(),edited:false},
            Entry {id:21,body:"taskq: accepted 19 attempt 10".into(),edited:false},
            Entry {id:22,body:"taskq: released 19 exit attempt 10".into(),edited:false},
        ]);
        assert_eq!(delivered(7,&log,10),Some(10),"no-start cannot later become acceptance");
        assert_eq!(executions(7,&log)[1].release,Some(Release::NoStart));
        log.extend([
            Entry {id:23,body:"taskq: executing 18 attempt 10\nop-id: retry".into(),edited:false},
            Entry {id:24,body:"taskq: executing 18 attempt 10\nop-id: retry".into(),edited:false},
            Entry {id:25,body:"taskq: accepted 23 attempt 10".into(),edited:false},
            Entry {id:26,body:"taskq: released 23 no-start attempt 10".into(),edited:false},
        ]);
        assert_eq!(executions(7,&log).len(),3);
        assert_eq!(executions(7,&log)[2].release,None,"accepted input cannot be declared never started");
        assert_eq!(delivered(7,&log,10),Some(18));
        log.extend([
            Entry {id:27,body:"taskq: requeue".into(),edited:false},
            Entry {id:28,body:"taskq: released 23 exit attempt 10".into(),edited:false},
            Entry {id:29,body:"taskq: spawn worker claude:new win".into(),edited:false},
            Entry {id:30,body:"taskq: accepted 23 attempt 10".into(),edited:false},
            Entry {id:31,body:"taskq: released 29 exit attempt 10".into(),edited:false},
        ]);
        assert_eq!(delivered(7,&log,29),None,"old evidence cannot accept a replacement");
        assert_eq!(executions(7,&log).last().unwrap().release,None);
    }

    // Chain: continuation refusal -> restart -> same input retry. Risk: intent consumes undelivered input.
    #[test]
    fn no_start_preserves_the_continuation_cursor() {
        use super::*;
        let bodies = [
            "taskq: spawn worker claude:s mac",
            "taskq: accepted 10 attempt 10",
            "taskq: released 10 exit attempt 10",
            "taskq: steer\ncontinue",
            "taskq: executing 13 attempt 10\nop-id: execute-10-13-10",
            "taskq: released 14 no-start attempt 10",
            "taskq: executing 13 attempt 10\nop-id: execute-10-13-14",
        ];
        let log: Vec<_> = bodies.iter().enumerate().map(|(i, body)| Entry { id: 10+i as u64, body: (*body).into(), edited:false }).collect();
        assert_eq!(executions(7,&log).iter().map(|x|x.id).collect::<Vec<_>>(), [10,14,16]);
        assert_eq!(revision(7,&log),10);
    }

    // Chain: submission -> task mutation/continuation -> exact release. Risk: task state is mistaken for execution exit.
    #[test]
    fn execution_release_survives_replacement_and_distinguishes_continuations() {
        use super::*;
        let mut log=vec![];
        for (id,body,edited,pending) in [
            (10,"taskq: spawn worker claude:s mac",false,vec![10]),
            (11,"taskq: requeue",false,vec![10]),
            (12,"taskq: released 10 exit attempt 999",false,vec![10]),
            (13,"taskq: released 10 exit attempt 10",true,vec![10]),
            (14,"taskq: released 10 exit attempt 10",false,vec![]),
            (15,"taskq: released 16 exit attempt 16",false,vec![]),
            (16,"taskq: spawn worker codex:s win",false,vec![16]),
            (17,"taskq: released 16 timeout attempt 16",false,vec![16]),
            (18,"taskq: released 16 no-start attempt 16\nop-id: release.16",false,vec![]),
            (19,"taskq: released 16 no-start attempt 16\nop-id: release.16",false,vec![]),
        ] {
            log.push(Entry {id,body:body.into(),edited});
            assert_eq!(executions(7,&log).iter().filter(|x|x.release.is_none()).map(|x|x.id).collect::<Vec<_>>(),pending,"entry{id}");
        }
        assert_eq!(revision(7,&log),16,"execution evidence does not change task revision");
        log.clear();
        for (id,body,pending) in [
            (8,"taskq: spawn worker claude:s mac",vec![8]),
            (9,"taskq: accepted 8 attempt 8",vec![8]),
            (11,"taskq: ask attempt 8\nWhich?\n1. x ★",vec![8]),
            (12,"taskq: released 8 exit attempt 8",vec![]),
            (13,"taskq: answer 1",vec![]),
            (14,"taskq: executing 13 attempt 8",vec![14]),
            (15,"taskq: released 8 exit attempt 8",vec![14]),
            (16,"taskq: released 14 exit attempt 8",vec![]),
            (17,"taskq: executing 13 attempt 8",vec![]),
            (18,"taskq: steer\nnew input",vec![]),
            (19,"taskq: accepted 14 attempt 8",vec![]),
            (20,"taskq: executing 18 attempt 8",vec![20]),
            (21,"taskq: released 20 exit attempt 9",vec![20]),
            (22,"taskq: released 20 exit attempt 8",vec![]),
        ] {
            log.push(Entry {id,body:body.into(),edited:false});
            let executions=executions(7,&log);
            assert_eq!(executions.iter().filter(|x|x.release.is_none()).map(|x|x.id).collect::<Vec<_>>(),pending,"entry{id}");
            assert!(executions.iter().all(|x|x.attempt==8 && x.host=="mac" && x.runtime=="claude"));
        }
        assert_eq!(executions(7,&log).iter().map(|x|x.through).collect::<Vec<_>>(),[8,13,18]);
    }

    // §8/§9: old readers ignore the new kind, so this is a cutover, never a mixed-reader rollout.
    #[test]
    fn usage_limit_requires_cutover() {
        let entries = |kind: &str| vec![
            super::Entry {id:1,body:"taskq: spawn worker claude:s mac".into(),edited:false},
            super::Entry {id:2,body:"taskq: gone attempt 1".into(),edited:false},
            super::Entry {id:3,body:"taskq: spawn worker claude:s mac".into(),edited:false},
            super::Entry {id:4,body:format!("taskq: {kind} attempt 3"),edited:false},
        ];
        let current = super::fold(1,&entries("usage-limit"),false);
        // Unknown kinds model the N-1 parser; legacy gone still retains its retry protection.
        let old = super::fold(1,&entries("unknown-to-reader"),false);
        let gone = super::fold(1,&entries("gone usage-limit"),false);
        assert_eq!((current.state,current.retries,current.attempt),(super::State::Ready,1,None));
        assert_eq!((old.state,old.retries,old.attempt),(super::State::Doing,1,Some(3)));
        assert_eq!((gone.state,gone.retries),(super::State::Ask,2));
    }

    // Chain: § 2 fold over tests/golden. Risk: fold drifts from the traces; a wrong expectation still passes.
    #[test]
    fn golden_traces() {
        assert!(super::golden(super::GOLDEN).unwrap() > 20);
        let wrong = super::GOLDEN.replacen("\"retries\": 2}", "\"retries\": 1}", 1);
        assert!(super::golden(&wrong).is_err(), "a wrong expectation must fail");
    }

    // Chain: § 2 Stages -> R6 `stage k/n` and the § 7 brief (#1880). Risk: a stage park loses the first open stage,
    // counts a non-Acceptance line, or a resumed attempt restarts at A1 outside its worktree.
    #[test]
    fn stages_name_the_first_open_acceptance_item() {
        use super::*;
        let body = "## Acceptance\n- A1. spec\nA2. code\n* A3. tests\nA04. not an id\nA2. duplicate\nNote A9 is prose";
        assert_eq!(acceptance(body), ["A1", "A2", "A3"]);
        assert_eq!(stages_line("Reason: x\nStages: A3 A1 A1"), Some(vec!["A1".into(), "A3".into()]));
        assert_eq!(stages_line("Stages: none"), Some(vec![]));
        for bad in ["Stages: A1 B2", "Stages:", "Reason: x"] { assert_eq!(stages_line(bad), None, "{bad}"); }
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!((stage(body, &[]), stage(body, &s(&["A1", "A3"])), stage(body, &s(&["A1", "A2", "A3"]))), (None, Some((2, 3)), Some((3, 3))));
        let log = |text: &str| vec![
            Entry { id: 10, body: "taskq: spawn worker claude:s1 mac".into(), edited: false },
            Entry { id: 11, body: format!("taskq: later until 2026-11-05T09:00:00Z attempt 10\nReason: wait\n{text}"), edited: false },
            Entry { id: 12, body: "taskq: requeue\nop-id: wake-11".into(), edited: false },
        ];
        assert!(task_context(7, body, &log("Stages: A1")).contains("Stages done: A1; continue at A2 (stage 2/3) in the existing worktree"));
        assert!(task_context(7, body, &log("Stages: A1 A2 A3")).contains("all 3 stages are done"));
        assert!(!task_context(7, body, &log("")).contains("Stages done"), "a task without stage records reads as before");
    }

    // Chain: historical prefix semantics, including ignored entries; timings are observations, not a gate.
    #[test]
    fn projection_preserves_prefix_transitions() {
        use super::*;
        let check = |n, entries: &[Entry]| {
            let mut log: Vec<_> = entries.iter().filter(|e| !e.edited).cloned().collect();
            log.sort_by_key(|e| e.id);
            let (mut expected, mut before) = (vec![], Task::default());
            for i in 0..log.len() {
                let after = fold(n, &log[..=i], false);
                if before.state != after.state { expected.push((log[i].id, before, after.clone())); }
                before = after;
            }
            assert_eq!(changes(n, entries), expected);
        };
        for tr in serde_json::from_str::<Vec<Trace>>(GOLDEN).unwrap() {
            check(tr.task, &tr.entries);
            let mut log = tr.entries.clone();
            log.reverse();
            check(tr.task, &log);
            log.extend(tr.entries.iter().cloned());
            check(tr.task, &log);
        }
        for cycles in [4, 128] {
            let mut log = vec![];
            for i in 0..cycles {
                let a = i * 4 + 1;
                for (id, body) in [(a, entry("spawn", &["worker", "codex:s", "mac"], None, "")),
                    (a+1, entry("ask", &[], Some(a), "Problem: detail\n1. continue ★\n2. later")),
                    (a+2, entry("answer", &["1"], None, "")),
                    (a+3, entry("requeue", &[], None, "fresh attempt"))] {
                    log.push(Entry {id, body, edited:false});
                }
            }
            check(7, &log);
            let began = std::time::Instant::now();
            for _ in 0..3 { std::hint::black_box(changes(7, &log)); }
            eprintln!("changes entries={} mean_us={}", log.len(), began.elapsed().as_micros()/3);
        }
    }

    // Chain: § 2 limit card (#1123). Risk: a card below the limit, a card added twice, or fix lines read as options.
    #[test]
    fn limit_text_adds_the_card_only_at_the_limit() {
        let t = |state, retries| super::Task { state, retries, ..Default::default() };
        let card = "Done: d\nProblem: p\n1. a ★\n2. b\nTo answer: x";
        assert_eq!(super::limit_text(7, &t(super::State::Review, 1), "requeue", "1. a"), "1. a");
        assert_eq!(super::limit_text(7, &t(super::State::Review, 2), "requeue", card), card);
        assert_eq!(super::limit_text(7, &t(super::State::Ask, 2), "requeue", "r"), "r");
        assert_eq!(super::limit_text(7, &t(super::State::Doing, 2), "steer", "r"), "r");
        let got = super::limit_text(7, &t(super::State::Doing, 1), "gone", "exit 1\n  2. b");
        assert_eq!(got.lines().take(4).collect::<Vec<_>>(), ["exit 1", "  2) b", "Done: the service stopped retrying; no new attempt starts until the owner answers.", "Problem: retry limit 2 reached by this gone; the task waits for the owner."]);
        assert_eq!(super::options(&got).iter().map(|o| (o.0, o.2)).collect::<Vec<_>>(), [(1, true), (2, false)]);
        assert!(super::limit_text(7, &t(super::State::Doing, 2), "requeue", "").starts_with("Done: "));
    }

    /// R8 made mechanical: every (state, entry) pair of the § 2 table is accepted by some golden trace.
    // Chain: R8 § 2 table to golden traces; spec size. Risk: a § 2 row without a trace, taskq.md over its declared contract-budget, or dated.
    #[test]
    fn every_table_row_has_a_golden_trace() {
        let spec = include_str!("../taskq.md");
        let mut seen = std::collections::HashSet::new();
        for tr in serde_json::from_str::<Vec<super::Trace>>(super::GOLDEN).unwrap() {
            let mut log = tr.entries.clone();
            log.sort_by_key(|e| e.id);
            for i in 0..log.len() {
                let (before, after) = (super::fold(tr.task, &log[..i], tr.deps_open), super::fold(tr.task, &log[..=i], tr.deps_open));
                if let (Some(l), true) = (super::parse(tr.task, &log[i].body), before != after) {
                    seen.insert((before.state.name().to_string(), l.kind.to_string()));
                }
            }
        }
        let ticks = |cell: &str| -> Vec<String> {
            cell.split('`').skip(1).step_by(2).map(|s| s.split(' ').next().unwrap().to_string()).collect()
        };
        let rows = spec.lines().skip_while(|l| !l.starts_with("| State | Entry |")).skip(2).take_while(|l| l.starts_with('|'));
        let mut missing = vec![];
        for row in rows {
            let cells: Vec<&str> = row.split('|').collect();
            let states = if cells[1].contains("any open") {
                ["ready", "waiting", "doing", "review", "ask"].map(String::from).to_vec()
            } else {
                ticks(cells[1])
            };
            for s in &states {
                for k in ticks(cells[2]) {
                    if !seen.contains(&(s.clone(), k.clone())) {
                        missing.push(format!("{s} + {k}"));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "§ 2 rows without a golden trace: {missing:?}");
        // specq contracts.md#size-budgets: the cap is the declared `contract-budget` comment.
        let cap = |key: &str| -> usize {
            let budget = spec.lines().find_map(|line| line.trim().strip_prefix("<!-- contract-budget:")).expect("taskq.md declares contract-budget");
            budget.split_whitespace().find_map(|pair| pair.strip_prefix(key)?.strip_prefix('=')?.parse().ok()).expect("contract-budget bytes and lines")
        };
        assert!(spec.lines().count() <= cap("lines"), "taskq.md over its contract-budget lines");
        assert!(spec.replace("\r\n", "\n").len() <= cap("bytes"), "taskq.md over its contract-budget bytes");
        let dated = |w: &[u8]| w.len() == 10 && w[4] == b'-' && w[7] == b'-' && w.iter().filter(|b| b.is_ascii_digit()).count() == 8;
        assert!(!spec.as_bytes().windows(10).any(dated), "taskq.md holds a date: history goes to git and issues");
    }

    // Chain: § 2 ask answer to resumed worker. Risk: the worker resumes with the wrong answer text.
    #[test]
    fn delivery_falls_back_to_option_then_first_line() {
        let e = |id, body: &str| super::Entry { id, body: body.into(), edited: false };
        let mut log = vec![e(10, "taskq: spawn worker claude:s1 mac"), e(11, "taskq: ask attempt 10\nWhich?\n1. x\n2. y ★")];
        log.push(e(12, "taskq answer 7.2"));
        assert_eq!(super::delivery(7, &log, 12), "y");
        log[2] = e(12, "taskq: answer 2\nuse y, keep x");
        assert_eq!(super::delivery(7, &log, 12), "use y, keep x");
        log[2] = e(12, "2");
        assert_eq!(super::delivery(7, &log, 12), "y");
        log[2] = e(12, " 2. Y ★");
        assert_eq!(super::delivery(7, &log, 12), "y");
        log[2] = e(12, "taskq answer 3");
        assert_eq!(super::delivery(7, &log, 12), "taskq answer 3");
        // § 2: a PM pick is never the answer; the worker receives the owner's option
        log[2] = e(12, "taskq: pick 1\nx is cheaper");
        log.push(e(13, "taskq answer 7.2"));
        assert_eq!(super::delivery(7, &log, 13), "y");
    }

    // Chain: accepted answer -> replacement attempt/reviewer context. Risk: a short reply loses its question,
    // or a late answer is mistaken for resolving the current question or granting runtime authority.
    #[test]
    fn task_context_keeps_answers_with_their_original_questions() {
        let e = |id, body: &str| super::Entry { id, body: body.into(), edited: false };
        let body = "## Goal\nContinue work\n## Acceptance\nA3. Verify the released service";
        let mut log = vec![
            e(10, "taskq: spawn worker codex:first mac"),
            e(11, "taskq: ask attempt 10\nProblem: Move released-service proof for A3 to DashQ #255?\n1. Move proof ★\n2. Keep proof"),
            e(12, "taskq: answer 1\nyes"),
            e(13, "taskq: requeue"),
            e(20, "taskq: spawn worker codex:second mac"),
            e(21, "taskq: ask attempt 20\nProblem: A new unrelated decision\n1. Continue ★\n2. Stop"),
            e(22, "taskq: steer answer 11\nKeep the original plan"),
            e(23, "taskq: steer\nPreserve this additional context"),
            e(24, "taskq: pick 2\nPM recommendation only"),
        ];
        let check = |log: &[super::Entry]| {
            let context = super::task_context(7, body, log);
            assert!(context.contains(body), "acceptance is never rewritten by a note");
            assert!(context.contains("Answer to question revision 11, option 1 (accepted):\nyes"), "{context}");
            assert_eq!(context.matches("Problem: Move released-service proof for A3 to DashQ #255?").count(), 2,
                "both accepted and late answers retain the original question");
            assert!(context.contains("Late answer saved as a note for question revision 11; it did not answer the current question:\nKeep the original plan"), "{context}");
            assert!(context.find("Note 12:").unwrap() < context.find("Note 22:").unwrap());
            assert!(context.find("Note 22:").unwrap() < context.find("Note 23:").unwrap());
            assert!(context.contains("Preserve this additional context"));
            assert!(!context.contains("PM recommendation only"));
            assert!(!context.contains("Problem: A new unrelated decision"), "a late reply must not borrow the current question");
        };
        check(&log);
        assert_eq!(super::project(7, &log, false).revision, 21);
        log.extend([
            e(25, "taskq: answer 1"),
            e(26, "taskq: result 1111111111111111111111111111111111111111 attempt 20"),
            e(30, "taskq: spawn reviewer codex:review mac"),
        ]);
        // The later answer legitimately carries the newer question; the old answer still carries its own.
        let context = super::task_context(7, body, &log);
        assert!(context.contains("Answer to question revision 21, option 1 (accepted):\nContinue"));
        assert!(context.contains("Answer to question revision 11, option 1 (accepted):\nyes"));
        assert_eq!(super::fold(7, &log, false).role.as_deref(), Some("reviewer"));
        log.push(e(31, "taskq: steer answer 999\nUnknown old source"));
        let context = super::task_context(7, body, &log);
        assert!(context.contains("Historical question revision 999 unavailable"));
        assert!(context.contains("Unknown old source"), "missing provenance must not discard the note");
        log.push(e(999, "taskq: ask attempt 30\nProblem: Future source must not attach to the old note\n1. Continue ★\n2. Stop"));
        let context = super::task_context(7, body, &log);
        assert!(context.contains("Historical question revision 999 unavailable"));
        assert!(!context.contains("Future source must not attach to the old note"));
        log[1].edited = true;
        let context = super::task_context(7, body, &log);
        assert!(context.contains("Historical question revision 11 unavailable"));
        assert!(context.contains("Keep the original plan"));
        assert!(!context.contains("Move released-service proof"), "an edited source is never question evidence");
        assert!(!context.contains("Note 12:"), "the answer to the ignored ask was never accepted");
    }

    // Chain: § 2 timed park time. Risk: a malformed or unreal time keeps a task parked forever or wakes it at once.
    #[test]
    fn timed_park_time_is_one_real_utc_instant() {
        for ok in ["2026-11-05T09:00:00Z", "2028-02-29T23:59:59Z", "2000-02-29T00:00:00Z"] { assert!(super::utc(ok), "{ok}"); }
        for bad in ["2026-11-05T09:00:00", "2026-11-05T09:00:00+00:00", "2026-11-05T09:00:00.5Z", "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z", "2026-13-01T00:00:00Z", "2026-04-31T00:00:00Z", "2026-11-05T24:00:00Z", "2026-11-05 09:00:00Z",
            "2026-11-0aT09:00:00Z", "+026-11-05T09:00:00Z", "", "tomorrow"] {
            assert!(!super::utc(bad), "{bad}");
        }
    }
