    use super::*;
    use crate::board::{Fake, reply};
    use serde_json::{Value, json};

    const ISSUES: [u64; 3] = [1721, 1758, 1929];

    fn fixture(n: u64) -> Value {
        let path = format!("{}/tests/golden/summary/{n}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn comments(f: &Value) -> Vec<Comment> {
        f["comments"].as_array().unwrap().iter().map(|c| Comment {
            id: c["id"].as_u64().unwrap(), created: c["created"].as_i64().unwrap(), body: c["body"].as_str().unwrap().into(),
            trusted: c["trusted"].as_bool().unwrap(), edited: c["edited"].as_bool().unwrap(), author: String::new(), read: None,
        }).collect()
    }

    fn github_link(n: u64) -> impl Fn(u64) -> String {
        move |id| if id == 0 { format!("https://github.com/taskqorg/taskq/issues/{n}") } else { format!("https://github.com/taskqorg/taskq/issues/{n}#issuecomment-{id}") }
    }

    fn summary_of(f: &Value) -> String {
        let n = f["number"].as_u64().unwrap();
        let log = reply::log(n, &comments(f)).unwrap();
        render(n, f["body"].as_str().unwrap(), &log, &core::fold(n, &log, false), &github_link(n), None)
    }

    // Chain: R1, § 2 Summary -> GitHub comment and GitLab note parsing -> reply::log -> fold. Risk (#2017 A6): the
    // summary, current or stale, edited or not, becomes a log entry and moves the task.
    #[test]
    fn fold_ignores_the_summary_on_github_and_gitlab() {
        for n in ISSUES {
            let f = fixture(n);
            let plain = comments(&f);
            let log = reply::log(n, &plain).unwrap();
            let want = core::project(n, &log, false);
            let current = summary_of(&f);
            let stale = format!("{MARKER}\n**Ready**\ntaskq: close\nChanged: forged");
            let at = |s: &str| if s.is_empty() { "2026-01-01T00:00:00Z" } else { "2026-01-02T00:00:00Z" };
            let mut rows: Vec<(u64, String, String)> = plain.iter().map(|c| (c.id, c.body.clone(), String::new())).collect();
            let top = plain.iter().map(|c| c.id).max().unwrap();
            rows.extend([(1, current.clone(), String::new()), (top + 1, stale.clone(), "edited".into()), (top + 2, stale, String::new())]);
            let github: Vec<Comment> = rows.iter().map(|(id, body, e)| crate::board::github::parse_comment(&json!({
                "id": id, "body": body, "created_at": at(""), "updated_at": at(e), "author_association": "MEMBER", "user": {"login": "owner"},
            }))).collect();
            let gitlab: Vec<Comment> = rows.iter().map(|(id, body, e)| crate::board::gitlab::parse_note(&json!({
                "id": id, "body": body, "created_at": at(""), "updated_at": at(e), "author": {"id": 7, "username": "owner"},
            }), true)).collect();
            for (host, board) in [("GitHub", github), ("GitLab", gitlab)] {
                assert_eq!(find(&board).map(|c| c.id), Some(1), "{host} #{n}: the lowest summary counts");
                assert_eq!(board.iter().filter(|c| is(c)).count(), 3, "{host} #{n}");
                let got = reply::log(n, &board).unwrap();
                assert_eq!(got.iter().map(|e| e.id).collect::<Vec<_>>(), log.iter().map(|e| e.id).collect::<Vec<_>>(), "{host} #{n}");
                let p = core::project(n, &got, false);
                assert_eq!((p.task, p.revision), (want.task.clone(), want.revision), "{host} #{n}: fold never reads the summary");
            }
        }
    }

    // Chain: § 2 Summary format -> render of real board logs. Risk (#2017 A3, A5): the summary misses the state,
    // outcome, merge receipt or files a person needs, or repeats a publish verdict as a copy.
    #[test]
    fn summary_of_board_logs_matches_golden_text() {
        for n in ISSUES {
            let f = fixture(n);
            let got = summary_of(&f);
            let want = f["summary"].as_str().unwrap_or_default();
            assert_eq!(got, want, "#{n}: update tests/golden/summary/{n}.json `summary` after reviewing the text");
        }
    }

    // Chain: § 2 Summary format -> open states. Risk (#2017 A3): an open question loses its options, a review loses
    // its outcome or PR, or more than three file links crowd the summary without a link to the rest.
    #[test]
    fn summary_shows_the_open_question_outcome_and_three_files() {
        let e = |id, body: String| Entry { id, body, edited: false };
        let sha = "c".repeat(40);
        let body = "## Goal\ng\n\n![shot](https://github.com/user-attachments/assets/1) [spec](https://example.com/spec.pdf) [site](https://example.com/)";
        let mut log = vec![e(10, "taskq: spawn worker codex:w win".into()),
            e(11, core::entry("result", &[&sha], Some(10), "Changed: the summary\nOpen: none\nFiles: [a](https://example.com/a.zip), [b](https://example.com/b.log), [a](https://example.com/a.zip)")),
            e(12, "taskq: publish\nChanged: the summary, published\nOpen: none\nPR: https://github.com/o/r/pull/7".into())];
        let render_at = |log: &[Entry]| render(5, body, log, &core::fold(5, log, false), &github_link(5), None);
        assert_eq!(render_at(&log), format!("{MARKER}\n**Review** · result ccccccc · [PR #7](https://github.com/o/r/pull/7)\nChanged: the summary, published\nOpen: none\nFiles: [a](https://example.com/a.zip), [b](https://example.com/b.log), [shot](https://github.com/user-attachments/assets/1) and [1 more](https://github.com/taskqorg/taskq/issues/5)"));
        log.truncate(1);
        log.push(e(11, core::entry("ask", &[], Some(10), "Done: checked\nProblem: Ship?\n1. Ship - + now; - risk\n2. Wait - + safe; - slow")));
        let ask = render_at(&log);
        assert!(ask.starts_with(&format!("{MARKER}\n**Ask**\n")) && ask.contains("\nProblem: Ship?\n1. Ship - + now; - risk\n2. Wait - + safe; - slow"), "{ask}");
    }

    // Chain: § 2 evidence files (#2019 A3) -> result_text Evidence line -> summary Files line. Defect: a worker's
    // `--file` upload had no place at decision time. Risk: a GitHub commit-pinned raw URL (`.tar.gz`, `.pdf`) or a
    // GitLab upload URL with its `#upload=` fragment is not seen as a file, so the summary drops it or its link changes.
    #[test]
    fn summary_files_line_links_each_result_and_ask_evidence_file_on_github_and_gitlab() {
        let e = |id, body: String| Entry { id, body, edited: false };
        let sha = "c".repeat(40);
        let github = [format!("[⬇ logs.tar.gz](https://github.com/o/r/raw/{sha}/5/0a1b2c-logs.tar.gz)"), format!("[⬇ report.pdf](https://github.com/o/r/raw/{sha}/5/3d4e5f-report.pdf)")];
        let gitlab = ["[⬇ build.zip](https://gitlab.example/-/project/39/uploads/cef859da/0a1b2c-build.zip#upload=5)".to_string(), "[⬇ site.html](https://gitlab.example/-/project/39/uploads/0f3a/3d4e5f-site.html#upload=6)".into()];
        for (host, files) in [("GitHub", &github), ("GitLab", &gitlab)] {
            let result = crate::result_text(5, "Changed: c\nOpen: none", &["cargo test -- pass".into()], &[], None, files).unwrap();
            assert!(result.ends_with(&format!("\nEvidence: {}, {}", files[0], files[1])), "{host}: {result}");
            let log = vec![e(10, "taskq: spawn worker claude:w win".into()), e(11, core::entry("result", &[&sha], Some(10), &result))];
            let got = render(5, "## Goal\ng", &log, &core::fold(5, &log, false), &github_link(5), None);
            assert_eq!(got.lines().last(), Some(format!("Files: {}, {}", files[0], files[1]).as_str()), "{host}: {got}");
            let opts = ["Ship - + done; - risk".to_string(), "Hold - + safe; - slow".into()];
            let card = crate::ask_card(5, "Done: built\nProblem: ship it?", &opts, Some("1"), &files[..1]).unwrap();
            let log = vec![e(10, "taskq: spawn worker claude:w win".into()), e(11, core::entry("ask", &[], Some(10), &card))];
            let got = render(5, "## Goal\ng", &log, &core::fold(5, &log, false), &github_link(5), None);
            assert!(got.contains(&format!("\nEvidence: {}\n", files[0])) && got.ends_with(&format!("\nFiles: {}", files[0])), "{host}: {got}");
        }
    }

    // Chain: § 2 Summary writer -> dispatch accepted entry -> board comment. Risk (#2017 A2): a new task gets no
    // summary after hold, or a later entry leaves it stale or adds a second one.
    #[test]
    fn writer_creates_after_hold_and_edits_after_each_entry() {
        let b = Fake::default();
        b.state.borrow_mut().summaries = true;
        let root = std::env::temp_dir().join(format!("taskq-summary-{}", std::process::id()));
        let n = b.create("Summary fixture", "## Goal\ng\n\nSee [log](https://example.com/run.log).", &["taskq".into()]).unwrap();
        let op = crate::dispatch::Op::default();
        assert!(crate::dispatch::act(&b, &root, n, "hold", &[], "", &op).ok());
        let after_hold = b.comments(n).unwrap();
        assert_eq!(after_hold.len(), 2, "hold, then its summary");
        let first = find(&after_hold).expect("summary after hold");
        assert_eq!(first.body, format!("{MARKER}\n**Held**\nFiles: [log](https://example.com/run.log)"));
        assert!(crate::dispatch::act(&b, &root, n, "start", &[], "", &op).ok());
        let started = b.comments(n).unwrap();
        assert_eq!((started.len(), find(&started).map(|c| (c.id, c.body.as_str()))), (3, Some((first.id, format!("{MARKER}\n**Ready**\nFiles: [log](https://example.com/run.log)").as_str()))));
        let r = crate::dispatch::act(&b, &root, n, "later", &[], "Waits for the design review.", &op);
        assert!(r.ok(), "{}", r.text());
        let after = b.comments(n).unwrap();
        assert_eq!(after.iter().filter(|c| is(c)).count(), 1, "edited in place, never a second one");
        let s = find(&after).unwrap();
        assert_eq!((s.id, s.body.as_str()), (first.id, format!("{MARKER}\n**Later**\nReason: Waits for the design review.\nFiles: [log](https://example.com/run.log)").as_str()));
        assert_eq!(core::fold(n, &reply::log(n, &after).unwrap(), false).state, State::Later);
        let _ = std::fs::remove_dir_all(root);
    }
