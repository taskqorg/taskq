use super::*;

fn facts(os: &str) -> Facts {
    Facts {
        os: os.into(),
        root: PathBuf::from("/p"),
        host: "h".into(),
        git: true,
        external: None,
        runtimes: vec![Runtime {
            name: "claude".into(),
            login: Some("logged in".into()),
            granted: true,
            ..Default::default()
        }],
        codex_dir: true,
        labelled: false,
        unlabelled: None,
        token: Some(Ok(())),
        delivery: Ok(crate::config::Delivery::AutoMerge),
        pr_route: Some(Probe::Ok),
        host_setup: None,
    }
}

fn gaps(f: &Facts) -> Vec<(String, String)> {
    items(f)
        .into_iter()
        .filter(|i| i.status == "gap")
        .map(|i| (i.id, i.fix.unwrap()))
        .collect()
}

// Chain: § 6 setup check (#1938 A2/A4). Risk: a ready project reports a gap, so owners learn to ignore it.
#[test]
fn a_ready_project_prints_one_ready_line() {
    let f = facts("macos");
    assert!(
        items(&f).iter().all(|i| i.status == "ok"),
        "{:?}",
        items(&f)
    );
    assert_eq!(lines(&items(&f)), ["Ready: setup check passed"]);
    assert_eq!(
        json("p", &items(&f))["items"].as_array().unwrap().len(),
        items(&f).len(),
        "--json keeps ok items too"
    );
}

// Defect (2026-10-30, A6): codex limit 4 without a grant held every Codex launch and the reviewer never spawned,
// silently. Protection: the refused runtime is a gap whose fix is the exact grant command.
#[test]
fn codex_in_use_without_a_grant_names_the_grant_command() {
    let mut f = facts("windows");
    f.runtimes.push(Runtime {
        name: "codex".into(),
        login: Some("logged in".into()),
        refused: Some(
            "no operator grant and no policy store protection for codex on windows; grant a preset"
                .into(),
        ),
        ..Default::default()
    });
    assert_eq!(
        gaps(&f),
        [(
            "access codex".to_string(),
            "`taskq service permissions --runtime codex --mode workspace --scope project`"
                .to_string()
        )]
    );
    let l = lines(&items(&f));
    assert!(
        l[0].starts_with("Gap: access refused for codex")
            && l[0].ends_with("its launches are held"),
        "{l:?}"
    );
    assert!(
        l[1].starts_with("Next: `taskq service permissions"),
        "{l:?}"
    );
    f.runtimes[0].refused = Some("full enforcement unknown".into());
    f.runtimes[0].granted = false;
    assert!(
        gaps(&f)
            .iter()
            .any(|(id, fix)| id == "access claude" && fix.contains("--mode full")),
        "Claude off macOS has only Full"
    );
}

// Defect (2026-10-30, A6): `publish: pr` beside `delivery.preset: direct` and a private repository whose rules read
// answers 403 left the project unable to publish. Protection: both are gaps with their owner fix.
#[test]
fn delivery_disagreement_and_a_refused_rules_read_are_gaps() {
    let mut f = facts("macos");
    f.delivery = Err("taskq.json: publish pr disagrees with delivery.preset direct".into());
    f.pr_route = Some(Probe::Gap(
        "the board refuses to read main branch rules (HTTP 403)".into(),
    ));
    let g = gaps(&f);
    assert_eq!(
        g.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["delivery", "pr-route"]
    );
    assert!(
        g[0].1.contains("remove `publish`") && g[1].1.contains("`direct`"),
        "{g:?}"
    );
    assert_eq!(
        items(&f).iter().find(|i| i.id == "delivery").unwrap().text,
        "publish pr disagrees with delivery.preset direct"
    );
    f.pr_route = Some(Probe::Unknown("HTTP 503".into()));
    assert_eq!(
        items(&f)
            .iter()
            .find(|i| i.id == "pr-route")
            .unwrap()
            .status,
        "unknown",
        "R12: unverified stays unknown"
    );
}

// Defect (2026-10-30, A6): on Linux a missing `<root>/.codex` refused Codex with a sandbox prerequisite error.
// Protection: one codex-dir gap with the mkdir fix, not a misleading grant step.
#[test]
fn missing_codex_dir_on_linux_is_one_gap_with_mkdir() {
    let mut f = facts("linux");
    f.codex_dir = false;
    f.runtimes.push(Runtime {
        name: "codex".into(),
        login: Some("logged in".into()),
        granted: true,
        refused: Some("linux sandbox prerequisite /p/.codex: No such file or directory".into()),
        ..Default::default()
    });
    assert_eq!(
        gaps(&f),
        [("codex-dir".to_string(), "`mkdir /p/.codex`".to_string())]
    );
    f.os = "macos".into();
    f.runtimes[1].refused = None;
    assert!(gaps(&f).is_empty(), "only Linux needs the project .codex");
}

// Defect cs#548 (#2078 A3): the shared pre-commit hook was never installed on pds18, a worker sandbox cannot write
// the shared .git, and attempts burned silently. Protection: a declared host setup that has not run is a gap whose
// fix runs it outside the sandbox; a changed command needs a new run.
#[test]
fn host_setup_not_run_is_a_gap_until_install_stamps_it() {
    let root = std::env::temp_dir().join(format!("taskq-host-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    let cmd = "scripts/install-git-hooks.sh";
    let mut f = facts("linux");
    f.host_setup = Some((cmd.into(), host_setup_done(&root, cmd)));
    assert_eq!(
        gaps(&f),
        [(
            "host-setup".to_string(),
            "run `taskq service install` in /p".to_string()
        )]
    );
    assert!(
        status_lines(&json("p", &items(&f)))[0]
            .starts_with("Setup gap: host setup `scripts/install-git-hooks.sh` has not run")
    );
    std::fs::write(host_setup_stamp(&root), format!("{cmd}\n")).unwrap();
    f.host_setup = Some((cmd.into(), host_setup_done(&root, cmd)));
    assert!(gaps(&f).is_empty(), "{:?}", gaps(&f));
    assert!(
        !host_setup_done(&root, "scripts/other.sh"),
        "a changed command runs again"
    );
    f.host_setup = None;
    assert!(
        items(&f).iter().all(|i| i.id != "host-setup"),
        "undeclared: no item"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// Chain: § 6 setup check host items (#1938 A2). Risk: a missing CLI, login, token, checkout or host label stalls silently.
#[test]
fn host_items_name_each_gap() {
    let mut f = facts("macos");
    f.git = false;
    f.external = Some(false);
    f.runtimes[0].login = Some("not logged in".into());
    f.runtimes.push(Runtime {
        name: "codex".into(),
        missing: Some("codex CLI not found on the service PATH".into()),
        ..Default::default()
    });
    f.labelled = true;
    f.unlabelled = Some(vec![4, 9]);
    f.token = Some(Err("file missing".into()));
    let ids: Vec<String> = gaps(&f).into_iter().map(|g| g.0).collect();
    assert_eq!(
        ids,
        [
            "checkout",
            "workspace",
            "login claude",
            "cli codex",
            "host-label",
            "token"
        ]
    );
    assert!(
        items(&f).iter().all(|i| i.id != "login codex"),
        "a missing CLI has no login to probe"
    );
    let label = items(&f)
        .into_iter()
        .find(|i| i.id == "host-label")
        .unwrap();
    assert!(label.text.contains("(#4, #9)") && label.fix.unwrap().contains("--host h --claim any"));
}

// Chain: § 6 status lines (#1938 A5). Risk: status hides a gap, or shows unknowns as gaps.
#[test]
fn status_lines_show_only_gaps() {
    let mut f = facts("macos");
    f.runtimes[0].refused = Some("policy unreadable (x)".into());
    f.pr_route = Some(Probe::Unknown("HTTP 503".into()));
    let l = status_lines(&json("p", &items(&f)));
    assert_eq!(l.len(), 2, "{l:?}");
    assert!(
        l[0].starts_with("Setup gap: access refused for claude") && l[1].starts_with("Next: "),
        "{l:?}"
    );
    assert!(status_lines(&Value::Null).is_empty());
}
