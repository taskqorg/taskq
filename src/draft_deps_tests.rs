use crate::{
    board::{Board, Fake},
    core, dispatch,
};
#[test]
fn pending_amendment_fences_start_and_other_mutations() {
    let b = Fake::default();
    let n = b.create("draft", "## Goal\nG", &["q-held".into()]).unwrap();
    let hold = b.comment(n, "taskq: hold").unwrap();
    b.comment(
        n,
        &format!("taskq: deps {hold}\n{{}}\nop-id: amend\nrevision: {hold}"),
    )
    .unwrap();
    b.comment(n, "taskq: start").unwrap();
    b.comment(n, "taskq: requeue").unwrap();
    assert_eq!(
        core::fold(n, &crate::board::entries(&b, n).unwrap(), false).state,
        core::State::Held
    );
    assert!(
        !dispatch::act(
            &b,
            std::path::Path::new("/tmp"),
            n,
            "start",
            &[],
            "",
            &Default::default()
        )
        .ok()
    );
}
fn draft(b: &Fake) -> (u64, dispatch::Op) {
    let n = b
        .create(
            "draft",
            "## Goal\r\nKeep me\r\n\r\nDepends on #99\r\n## Scope\nunchanged",
            &["q-held".into(), "p1".into()],
        )
        .unwrap();
    let revision = b.comment(n, "taskq: hold").unwrap();
    (
        n,
        dispatch::Op {
            id: Some("amend-1".into()),
            revision: Some(revision),
        },
    )
}
#[test]
fn amendment_receipt_preserves_body_and_blocks_until_verified_completion() {
    let b = Fake::default();
    let (n, op) = draft(&b);
    let blocker = b.create("blocker", "", &[]).unwrap();
    let url = b.url(0);
    let url = url.trim_end_matches("/0");
    let original = b.issue(n).unwrap();
    let root = std::env::temp_dir().join(format!("taskq-deps-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let r = super::amend(&b, &root, n, url, &format!("#{blocker}"), &op, true);
    assert!(r.ok(), "{}", r.text());
    let got = b.issue(n).unwrap();
    assert_eq!(
        got.body,
        format!("## Goal\r\nKeep me\r\n\r\nDepends on #{blocker}\n## Scope\nunchanged")
    );
    assert_eq!(got.labels, original.labels);
    assert_eq!(got.title, original.title);
    assert_eq!(got.assignees, original.assignees);
    let count = b.state.borrow().comments.len();
    let repeat = super::amend(&b, &root, n, url, &format!("#{blocker}"), &op, true);
    assert!(repeat.ok());
    assert_eq!(repeat.revision, r.revision);
    assert_eq!(count, b.state.borrow().comments.len());
    let log = crate::board::entries(&b, n).unwrap();
    assert!(dispatch::deps_open(&b, n, &got.body, &log));
    assert!(dispatch::act(&b, &root, n, "start", &[], "", &Default::default()).ok());
    assert_eq!(
        dispatch::current(&b, n).unwrap().1.state,
        core::State::Waiting
    );
    b.close(blocker).unwrap();
    assert_eq!(
        dispatch::current(&b, n).unwrap().1.state,
        core::State::Ready
    );
    let count = b.state.borrow().comments.len();
    assert!(super::amend(&b, &root, n, url, &format!("#{blocker}"), &op, true).ok());
    assert_eq!(count, b.state.borrow().comments.len());
    let new = dispatch::Op {
        id: Some("new".into()),
        revision: r.revision,
    };
    assert!(!super::amend(&b, &root, n, url, "", &new, true).ok());
    assert_eq!(count, b.state.borrow().comments.len());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn amendment_refusals_have_no_effects_and_unknown_dependency_waits() {
    let b = Fake::default();
    let (n, op) = draft(&b);
    let url = b.url(0);
    let url = url.trim_end_matches("/0");
    let original = b.issue(n).unwrap().body;
    let count = b.state.borrow().comments.len();
    for (board, list, request, authorized) in [
        (url, "#987", op.clone(), false),
        ("https://wrong.invalid", "#987", op.clone(), true),
        (
            url,
            "#987",
            dispatch::Op {
                revision: Some(0),
                ..op.clone()
            },
            true,
        ),
        (url, "bad#x", op.clone(), true),
    ] {
        assert!(
            !super::amend(
                &b,
                std::path::Path::new("/tmp"),
                n,
                board,
                list,
                &request,
                authorized
            )
            .ok()
        );
        assert_eq!(b.issue(n).unwrap().body, original);
        assert_eq!(count, b.state.borrow().comments.len());
    }
    let r = super::amend(&b, std::path::Path::new("/tmp"), n, url, "#987", &op, true);
    assert!(r.ok(), "{}", r.text());
    let count = b.state.borrow().comments.len();
    assert!(!super::amend(&b, std::path::Path::new("/tmp"), n, url, "", &op, true).ok());
    assert_eq!(count, b.state.borrow().comments.len());
    b.comment(n, "taskq: start").unwrap();
    assert_eq!(
        dispatch::current(&b, n).unwrap().1.state,
        core::State::Waiting,
        "unreadable is never done"
    );
}

#[test]
fn lost_receipt_reconciles_and_raced_start_prevents_the_edit() {
    let b = Fake::default();
    let (n, op) = draft(&b);
    let url = b.url(0);
    let url = url.trim_end_matches("/0");
    b.state.borrow_mut().lose_comment_response = true;
    let r = super::amend(
        &b,
        std::path::Path::new("/tmp"),
        n,
        url,
        "foreign/repo#7",
        &op,
        true,
    );
    assert_eq!(r.outcome, "unknown");
    let r = super::amend(
        &b,
        std::path::Path::new("/tmp"),
        n,
        url,
        "foreign/repo#7",
        &op,
        true,
    );
    assert!(r.ok(), "{}", r.text());
    assert_eq!(
        core::depends(&b.issue(n).unwrap().body)[0].name(),
        "foreign/repo#7"
    );
    let clear = dispatch::Op {
        id: Some("clear".into()),
        revision: r.revision,
    };
    assert!(super::amend(&b, std::path::Path::new("/tmp"), n, url, "", &clear, true).ok());
    assert!(core::depends(&b.issue(n).unwrap().body).is_empty());
    let (n, op) = draft(&b);
    let before = b.issue(n).unwrap().body;
    b.state.borrow_mut().race = Some("taskq: start".into());
    assert!(!super::amend(&b, std::path::Path::new("/tmp"), n, url, "#987", &op, true).ok());
    assert_eq!(
        before,
        b.issue(n).unwrap().body,
        "start won; no edit may follow"
    );
}

#[test]
fn cli_clear_keeps_an_explicit_empty_list_and_refuses_missing_or_repeated_flags() {
    let b = Fake::default();
    let (n, op) = draft(&b);
    let url = b.url(0);
    let url = url.trim_end_matches("/0");
    let parse = |flags: &[&str]| {
        crate::Args::parse(
            &[
                vec![
                    "deps".to_string(),
                    n.to_string(),
                    "--board".into(),
                    url.into(),
                ],
                flags.iter().map(|s| s.to_string()).collect(),
            ]
            .concat(),
        )
        .unwrap()
    };
    let before = b.issue(n).unwrap().body;
    assert!(crate::deps_command(&b, std::path::Path::new("/tmp"), &parse(&[]), &op, true).is_err());
    assert!(
        crate::deps_command(
            &b,
            std::path::Path::new("/tmp"),
            &parse(&["--deps", "", "--deps", "#3"]),
            &op,
            true
        )
        .is_err()
    );
    assert_eq!(before, b.issue(n).unwrap().body);
    let r = crate::deps_command(
        &b,
        std::path::Path::new("/tmp"),
        &parse(&["--deps", ""]),
        &op,
        true,
    )
    .unwrap();
    assert!(r.ok(), "{}", r.text());
    assert!(core::depends(&b.issue(n).unwrap().body).is_empty());
}
