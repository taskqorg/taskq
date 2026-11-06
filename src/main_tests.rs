// Chain: §2.26/§2.27/R6: CLI routing must reach the writer, and a lost wait-ready reply must not forward proof twice.
#[test]
fn dependency_wait_2469_cli_routes_and_retries_without_replay() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let b = c.board.as_ref();
    let n = b.create("consumer", "", &["taskq".into()]).unwrap();
    let m = b.create("proof", "", &["taskq".into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:invented win")
        .unwrap();
    let question = b
        .comment(n, &format!("taskq: ask attempt {a}\nProblem: proof needed"))
        .unwrap();
    let run = |words: &[&str]| {
        let words = words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        command(&c, &words[0], &Args::parse(&words)?)
    };
    let n = n.to_string();
    let revision = question.to_string();
    let blocker = format!("#{m}");
    run(&[
        "classify",
        &n,
        "--actor",
        "pm",
        "--revision",
        &revision,
        "--op-id",
        "cli-classify",
    ])
    .unwrap();
    run(&[
        "wait",
        &n,
        "--kind",
        "evidence",
        "--blocker",
        &blocker,
        "--revision",
        &revision,
        "--op-id",
        "cli-wait",
    ])
    .unwrap();
    let proof = b
        .comment(m, "Invented proof with exact candidate identity")
        .unwrap();
    let evidence = format!("{m}.{proof}");
    let ready = [
        "wait-ready",
        &n,
        "--evidence",
        &evidence,
        "--revision",
        &revision,
        "--op-id",
        "cli-ready",
    ];
    run(&ready).unwrap();
    let count = b.comments(n.parse().unwrap()).unwrap().len();
    run(&ready).unwrap();
    assert_eq!(b.comments(n.parse().unwrap()).unwrap().len(), count);
    assert_eq!(
        core::revision(n.parse().unwrap(), &entries(b, n.parse().unwrap()).unwrap()),
        question
    );
    assert!(
        fold(
            n.parse().unwrap(),
            &entries(b, n.parse().unwrap()).unwrap(),
            false
        )
        .wait
        .is_none()
    );
    assert!(
        Args::parse(&["wait".into(), "--window".into(), "1".into()]).is_ok(),
        "bare wait retains observation syntax"
    );
}

// Chain: § 7 rule 3 worker park (#1486, #1823). Risk: a worker parks without a blocker or time (no automatic exit), an
// owner park starts needing one, or a malformed time reaches the board.
#[test]
fn worker_park_needs_one_blocker_or_time() {
    let w = || (Some("7".to_string()), Some("42".to_string()));
    let (t, a) = w();
    assert!(
        crate::cli::text::park(7, None, None, None, t, a)
            .unwrap_err()
            .contains("--blocker")
    );
    for bad in ["#7", "#x", "12", "#1 #2"] {
        let (t, a) = w();
        assert!(
            crate::cli::text::park(7, Some(bad), None, None, t, a).is_err(),
            "{bad}"
        );
    }
    let (t, a) = w();
    assert_eq!(
        crate::cli::text::park(7, Some("dashq#36"), None, None, t, a).unwrap(),
        ["dashq#36"]
    );
    assert!(
        crate::cli::text::park(7, None, None, None, None, None)
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::cli::text::park(7, None, None, None, Some("8".into()), Some("42".into()))
            .unwrap()
            .is_empty(),
        "another task's env is not this task's worker"
    );
    assert!(crate::cli::text::park(7, Some("#12"), None, None, None, None).is_err());
    let (t, a) = w();
    assert_eq!(
        crate::cli::text::park(7, None, Some("2026-11-05T09:00:00Z"), None, t, a).unwrap(),
        ["until", "2026-11-05T09:00:00Z"],
        "a worker waiting only for time parks with it"
    );
    assert_eq!(
        crate::cli::text::park(7, None, Some("2026-11-05T09:00:00Z"), None, None, None).unwrap(),
        ["until", "2026-11-05T09:00:00Z"]
    );
    assert!(
        crate::cli::text::park(7, None, Some("2026-11-05"), None, None, None)
            .unwrap_err()
            .contains("bad --until")
    );
    let (t, a) = w();
    assert!(
        crate::cli::text::park(7, Some("#12"), Some("2026-11-05T09:00:00Z"), None, t, a)
            .unwrap_err()
            .contains("exclude")
    );
}

// #2137 A3: reroute reads back ready pins, refuses active/unreleased work, and never rewrites other labels.
#[test]
fn balancing_reroute_preserves_live_attempts() {
    let b = board::Fake::default();
    let dir = std::env::temp_dir().join(format!("taskq-reroute-{}", runtime::uuid()));
    let labels = vec![
        "taskq".into(),
        "run-claude".into(),
        "prefer-antigravity".into(),
        "host-mac".into(),
    ];
    let ready = b.create("Ready", "", &labels).unwrap();
    let doing = b.create("Doing", "", &labels).unwrap();
    let spawn = b
        .comment(doing, "taskq: spawn worker claude:owned mac")
        .unwrap();
    let unknown = b.create("Unknown execution", "", &labels).unwrap();
    b.comment(unknown, "taskq: spawn worker claude:unknown mac")
        .unwrap();
    b.comment(unknown, "taskq: requeue").unwrap();
    let held = b.create("Held", "", &labels).unwrap();
    b.comment(held, "taskq: hold").unwrap();
    let rs = reroute(&b, &dir, "claude", "codex").unwrap();
    assert_eq!(
        rs.iter()
            .filter(|r| r.ok())
            .map(|r| r.task)
            .collect::<Vec<_>>(),
        [ready]
    );
    assert_eq!(rs.iter().filter(|r| !r.ok()).count(), 3);
    let got = b.issue(ready).unwrap().labels;
    assert!(got.contains(&"run-codex".into()) && !got.contains(&"run-claude".into()));
    for keep in ["taskq", "host-mac", "prefer-antigravity"] {
        assert!(got.contains(&keep.into()));
    }
    assert_eq!(current(&b, doing).unwrap().1.attempt, Some(spawn));
    assert!(
        b.issue(doing)
            .unwrap()
            .labels
            .contains(&"run-claude".into())
    );
    assert_eq!(
        b.comments(ready)
            .unwrap()
            .iter()
            .filter(|c| c.body.starts_with("taskq: reroute "))
            .count(),
        1
    );
    assert!(reroute(&b, &dir, "codex-app", "codex").is_err());
    assert!(reroute(&b, &dir, "codex", "codex").is_err());
    // A spawn winning before the routing fence must keep both its pin and its live identity.
    let race = b.create("Concurrent spawn", "", &labels).unwrap();
    b.state.borrow_mut().race = Some("taskq: spawn worker claude:concurrent mac".into());
    let rs = reroute(&b, &dir, "claude", "codex").unwrap();
    assert!(!rs.iter().find(|r| r.task == race).unwrap().ok());
    assert!(b.issue(race).unwrap().labels.contains(&"run-claude".into()));
    assert_eq!(current(&b, race).unwrap().1.state, State::Doing);
    // Lost label/completion readback retains a durable intent; retry returns fresh facts without a second fence.
    let interrupted = b.create("Interrupted reroute", "", &labels).unwrap();
    b.state.borrow_mut().fail_labels = true;
    let rs = reroute(&b, &dir, "claude", "codex").unwrap();
    assert_eq!(
        rs.iter().find(|r| r.task == interrupted).unwrap().outcome,
        "unknown"
    );
    assert!(
        core::project(interrupted, &entries(&b, interrupted).unwrap(), false)
            .reroute
            .is_some()
    );
    b.state.borrow_mut().fail_labels = false;
    b.labels(interrupted, &["run-codex".into()], &[]).unwrap();
    let rs = reroute(&b, &dir, "claude", "codex").unwrap();
    assert_eq!(
        rs.iter().find(|r| r.task == interrupted).unwrap().state,
        Some("ready")
    );
    assert_eq!(
        b.comments(interrupted)
            .unwrap()
            .iter()
            .filter(|c| c.body.starts_with("taskq: reroute "))
            .count(),
        1
    );
    let dependency = b.create("Dependency", "", &[]).unwrap();
    let waiting = b
        .create(
            "Dependency reopened during reroute",
            &format!("Depends on #{dependency}"),
            &labels,
        )
        .unwrap();
    b.comment(waiting, "taskq: reroute claude codex\nrouting-fence: 1")
        .unwrap();
    assert_eq!(dispatch::finish_reroute(&b, waiting).outcome, "unknown");
    assert!(
        b.issue(waiting)
            .unwrap()
            .labels
            .contains(&"run-claude".into()),
        "dependency blocking still binds during reconciliation"
    );
    let partial = Fake::default();
    let unreadable = partial.create("Unreadable", "", &labels).unwrap();
    let next = partial.create("Readable", "", &labels).unwrap();
    partial.state.borrow_mut().fail_comment_reads = 1;
    let rs = reroute(&partial, &dir, "claude", "codex").unwrap();
    assert_eq!(
        rs.iter().find(|r| r.task == unreadable).unwrap().outcome,
        "unknown"
    );
    assert!(
        rs.iter().find(|r| r.task == next).unwrap().ok(),
        "one read failure keeps every other receipt"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// Chain: § 2 Stages, § 5 `later --stage` (#1880). Risk: a worker cannot park after a finished stage, a stage outside the
// issue's Acceptance reaches the board, or the `Stages:` line breaks the § 5 `Reason:`/`Next:` text.
#[test]
fn stage_park_records_acceptance_items() {
    let (t, a) = (Some("7".to_string()), Some("42".to_string()));
    assert_eq!(
        crate::cli::text::park(7, None, None, Some("2026-11-03T10:00:00Z"), t, a).unwrap(),
        ["until", "2026-11-03T10:00:00Z"],
        "a worker stage park wakes at once (#2289)"
    );
    assert!(
        crate::cli::text::park(7, None, None, Some("2026-11-03T10:00:00Z"), None, None)
            .unwrap()
            .is_empty(),
        "an owner or PM stage park still holds"
    );
    let (t, a) = (Some("7".to_string()), Some("42".to_string()));
    assert_eq!(
        crate::cli::text::park(7, Some("#9"), None, Some("2026-11-03T10:00:00Z"), t, a).unwrap(),
        ["#9"],
        "a blocker still parks on it"
    );
    let body = "## Goal\ng\n\n## Acceptance\nA1. a\nA2. b\n- A3. c\n";
    assert_eq!(
        crate::cli::text::stage_text("Reason: soak\nNext: A3", &["A2".into(), "A1".into()], body)
            .unwrap(),
        "Reason: soak\nNext: A3\nStages: A1 A2"
    );
    assert_eq!(
        crate::cli::text::stage_text("", &["A3".into()], body).unwrap(),
        "Stages: A3"
    );
    assert!(
        crate::cli::text::stage_text("", &["A4".into()], body)
            .unwrap_err()
            .contains("bad --stage A4: one Acceptance item of this task (A1 A2 A3)")
    );
    assert!(
        crate::cli::text::stage_text("", &["A1".into()], "no items")
            .unwrap_err()
            .contains("(none)")
    );
    let text = crate::cli::text::stage_text("Reason: soak", &["A1".into()], body).unwrap();
    assert_eq!(
        crate::core::stages_line(&text),
        Some(vec!["A1".to_string()])
    );
}

use super::*;
use crate::Res;
use crate::board::{Board, entries};
use crate::cli::host_cmd::doctor_hosts_board_json;
#[cfg(unix)]
use crate::cli::host_cmd::host_setup;
use crate::cli::host_cmd::{
    CapEdit, add_host, cap_changes, channel_cmd, dev_build_cmd, setup_items,
};
use crate::cli::inbox::{
    closed_history, closed_pin, closed_row, closed_session, closed_transcript, inbox,
};
#[cfg(unix)]
use crate::cli::service_cmd::service_id;
#[cfg(target_os = "macos")]
use crate::cli::service_cmd::tray_app;
use crate::cli::service_cmd::{
    install_focus, lifecycle_scope, pause, resume, service, service_fields, service_project,
    set_focus, tray, tray_id,
};
use crate::cli::status_cmd::status_prints_json;
use crate::cli::status_cmd::{json_here_only, status_text};
use crate::cli::task_cmd::add_placement;
use crate::cli::task_cmd::{
    NewTask, add_task, claim_issue_setup, claimed_create, emit_finding, finding_submit, reroute,
    start_one, start_task, start_tasks, task_body,
};
use crate::cli::text::{
    all, ask_card, attachment_files, card, card_links, close_verdict, formats_selftest,
    held_blocker, media_links, one_line, option_lines, result_text, service_card, task_text,
    upload_attachments, upload_images, width, wrap_reply,
};
use crate::cli::{Args, PM_COMMANDS, USAGE, help, pm_instructions};
use crate::config::{Ctx, check_load, delivery, load, project_config_value, project_value};
use crate::core::{Entry, State, entry, fold};
use crate::dispatch::{Op, Receipt, act, answer_picks, current};
#[cfg(target_os = "linux")]
use crate::linux_sandbox;
use crate::paths::{PROJECT_ROOT, dot, kids_path, needs_checkout, root_for, touch};
use crate::service::Config;
#[cfg(unix)]
use crate::service::{Kid, Service};
#[cfg(unix)]
use crate::service_run::{adopt, lifecycle_waits, other_service, recorded, reexec, save_kids};
use crate::service_run::{kids_file, mark_checking, pre_boot, retire_pre_boot, write_kids};
use crate::snapshot::in_scope;
use crate::wake::{
    ask_reread, change_focus, menu_open, poke, poke_times, sleep_until_poke, take_reread,
    wake_lock, wake_read,
};
use crate::{
    board, core, data, dispatch, enrollment, host, platform, policy, profile, reply_files, reports,
    runtime, service, snapshot, telemetry,
};
use board::Fake;
use dispatch::close_one;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

// Defect #1445: kids from before a reboot held startup (Windows job recovery fails). Risk: a live kid retired.
#[test]
fn kids_from_a_previous_boot_are_retired_and_live_kids_kept() {
    let root = std::env::temp_dir().join(format!("taskq-preboot-{}", runtime::uuid()));
    let install = root.join("install");
    fs::create_dir_all(&install).unwrap();
    let mut gone = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
    } else {
        std::process::Command::new("true").spawn()
    }
    .unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    let old = if cfg!(target_os = "linux") {
        "00000000-0000-0000-0000-000000000000-1"
    } else if cfg!(windows) {
        "1"
    } else {
        "1-0"
    };
    let mut live = if cfg!(windows) {
        std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
    } else {
        std::process::Command::new("sleep").arg("30").spawn()
    }
    .unwrap();
    let me = coreq::process::identity(live.id()).unwrap().unwrap();
    let kid = |task, process| data::KidLine {
        task,
        attempt: 1,
        runtime: "codex".into(),
        process,
        execution: task,
        offset: 0,
        job: None,
    };
    let stale = kid(
        1,
        coreq::process::Identity {
            pid,
            started: old.into(),
        },
    );
    assert!(pre_boot(&stale));
    assert!(
        !pre_boot(&kid(2, me.clone())),
        "a process of this boot is kept"
    );
    write_kids(&install, &root, &[stale, kid(2, me.clone())]).unwrap();
    let mut pending = kids_path(&install, &root).unwrap().into_os_string();
    pending.push(".pending");
    fs::write(&pending, [0u8; 8]).unwrap();
    assert_eq!(retire_pre_boot(&install, &root).unwrap(), 1);
    assert_eq!(kids_file(&install, &root).unwrap(), vec![kid(2, me)]);
    assert!(
        !Path::new(&pending).exists(),
        "a torn pending file cannot block later checkpoints"
    );
    live.kill().unwrap();
    live.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn removed_custom_icon_flag_refuses_before_reading_file_or_opening_tray() {
    let error = tray(
        PathBuf::from("invented-unused-install"),
        None,
        false,
        &[],
        Some("invented-missing-icon.json"),
    )
    .unwrap_err();
    assert!(error.contains("Custom icons were removed"), "{error}");
}

// #1801: the release bundle id belongs to the OS default install only; every other install kind takes the test id.
#[test]
fn tray_bundle_id_per_install_kind() {
    assert_eq!(tray_id(true), "org.taskq.tray");
    assert_eq!(tray_id(false), "org.taskq.tray.test");
    let home = PathBuf::from("/invented/home/.local/share/taskq");
    for (install, want) in [
        (home.clone(), "org.taskq.tray"),
        (
            std::env::temp_dir().join("taskq-evidence"),
            "org.taskq.tray.test",
        ),
        (
            PathBuf::from("/invented/dev-install"),
            "org.taskq.tray.test",
        ),
    ] {
        assert_eq!(
            tray_id(platform::is_home(&install, Some(&home))),
            want,
            "{}",
            install.display()
        );
    }
}

// #1347: packaging must retain all native sizes and a valid signature under the existing identity.
#[cfg(target_os = "macos")]
#[test]
fn app_bundle_icon_resources_preserve_signature() {
    let dir = std::env::temp_dir().join(format!("taskq-app-icon-{}", runtime::uuid()));
    let app = tray_app(&dir, &std::env::current_exe().unwrap()).expect("signed tray bundle");
    let plist = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
    // #1801: a temp install never takes the release bundle id
    assert!(
        plist.contains("<string>org.taskq.tray.test</string>"),
        "{plist}"
    );
    let bytes = fs::read(app.join("Contents/Resources/TaskQ.icns")).unwrap();
    let mut sizes = Vec::new();
    let mut offset = 8;
    while offset < bytes.len() {
        let len = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes[offset + 8..offset + len]));
        let reader = decoder.read_info().unwrap();
        sizes.push(reader.info().width);
        assert_eq!(reader.info().width, reader.info().height);
        offset += len;
    }
    assert_eq!(sizes, vec![16, 32, 64, 128, 256, 512, 1024]);
    assert!(
        std::process::Command::new("codesign")
            .args(["--verify", "--strict"])
            .arg(&app)
            .status()
            .unwrap()
            .success()
    );
    fs::remove_dir_all(dir).unwrap();
}

// Chain: shared wakeup metadata -> bounded, locked read -> link or explicit unknown.
#[test]
fn wakeup_context_refuses_partial_or_busy_input() {
    let root = std::env::temp_dir().join(format!("qw-link-{}", std::process::id()));
    fs::create_dir_all(dot(&root)).unwrap();
    let path = dot(&root).join("poke");
    let mut seen = None;
    assert!(wake_context(&path, &mut seen).unwrap().is_none());
    for text in [Vec::new(), b".".to_vec()] {
        fs::write(&path, text).unwrap();
        assert!(wake_context(&path, &mut seen).unwrap().is_none());
    }
    for text in [b"1 incomplete".to_vec(), vec![b'x'; 129], vec![255]] {
        fs::write(&path, text).unwrap();
        assert!(wake_context(&path, &mut seen).is_err());
    }
    let context = format!("1 {}.1 {}.2", "a".repeat(32), "a".repeat(32));
    fs::write(&path, &context).unwrap();
    let writer = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    writer.try_lock().unwrap();
    assert_eq!(
        wake_context(&path, &mut seen).unwrap_err(),
        "wakeup context busy"
    );
    writer.unlock().unwrap();
    writer.try_lock_shared().unwrap();
    assert!(poke(&root).unwrap_err().contains("wake-lock"));
    assert!(
        !poke(&root).unwrap_err().contains("unsupported"),
        "a lock held by a live service stays an ordinary refusal"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        context,
        "busy reader allowed a partial overwrite"
    );
    assert_eq!(
        wake_context(&path, &mut seen).unwrap().unwrap().encode(),
        context
    );
    assert!(wake_context(&path, &mut seen).unwrap().is_none());
    // Parallel subprocess tests may inherit this fixture's descriptor before exec.
    writer.unlock().unwrap();
    drop(writer);
    poke(&root).unwrap();
    assert!(wake_context(&path, &mut seen).unwrap().is_none());
    fs::remove_dir_all(root).unwrap();
}

// Chain: § 5 stale TASKQ_ROOT (#1072). Risk: a reused session's command writes into another project.
#[test]
fn stale_taskq_root_is_refused() {
    let t = std::env::temp_dir().join(format!("taskq-stale-root-{}", std::process::id()));
    let (a, b, out) = (t.join("a"), t.join("b"), t.join("out"));
    for d in [&a, &b] {
        fs::create_dir_all(d.join(".worktrees/x")).unwrap();
        fs::write(d.join("taskq.json"), "{}").unwrap();
    }
    fs::create_dir_all(&out).unwrap();
    assert_eq!(
        root_for(Some(a.clone()), a.join(".worktrees/x")).unwrap(),
        a
    );
    assert_eq!(root_for(Some(a.clone()), out.clone()).unwrap(), a);
    let e = root_for(Some(b.clone()), a.join(".worktrees/x")).unwrap_err();
    assert!(
        e.starts_with("TASKQ_ROOT ") && e.contains("is not the project of the current directory"),
        "{e}"
    );
    assert_eq!(root_for(None, a.join(".worktrees/x")).unwrap(), a);
    fs::remove_dir_all(&t).unwrap();
}

// Chain: §5 Board-only root (#2093). Risk: a root with only taskq.json (PilotQ: code elsewhere) refuses a PM command,
// writes a checkout, or lets this host serve and spawn workers into a root without code.
#[test]
fn board_only_root_runs_pm_commands_and_refuses_checkout_work() {
    let base = std::env::temp_dir().join(format!("taskq-board-only-{}", crate::runtime::uuid()));
    let (root, install) = (base.join("root"), base.join("install"));
    for d in [&root, &install] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/code-elsewhere"}"#,
    )
    .unwrap();
    // invented board token (§ 9 `<install>/token`); the fake board reads none
    fs::write(install.join("token"), "ghp_aaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    assert_eq!(root_for(None, root.clone()).unwrap(), root);
    let (mut config, raw) = project_config(&root).unwrap();
    config.install = install;
    let c = Ctx {
        cfg: config,
        raw,
        board: Box::new(Fake::default()),
    };
    let run = |words: &[&str]| {
        let v: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        command(&c, &v[0], &Args::parse(&v).unwrap())
    };
    let state = |n: u64| fold(n, &entries(&*c.board, n).unwrap(), false).state;
    run(&[
        "add",
        "Board-only fixture",
        "--goal",
        "Code lives elsewhere",
        "--acceptance",
        "The fixture closes",
    ])
    .unwrap();
    let n = c
        .board
        .issues(None)
        .unwrap()
        .iter()
        .find(|i| i.title == "Board-only fixture")
        .unwrap()
        .number;
    run(&["start", &n.to_string()]).unwrap();
    assert_eq!(state(n), State::Ready);
    // a host whose service serves the project runs the attempt
    let attempt = c
        .board
        .comment(n, "taskq: spawn worker claude:fixture mac")
        .unwrap();
    run(&[
        "steer",
        &n.to_string(),
        "--text",
        "Read the board-only note",
    ])
    .unwrap();
    c.board
        .comment(
            n,
            &core::entry(
                "ask",
                &[],
                Some(attempt),
                "Done: not started\nProblem: which root?",
            ),
        )
        .unwrap();
    assert_eq!(state(n), State::Ask);
    run(&["answer", &n.to_string(), "--text", "The board-only one"]).unwrap();
    assert_eq!(state(n), State::Doing);
    run(&[
        "requeue",
        &n.to_string(),
        "--text",
        "Reason: retry on another host",
    ])
    .unwrap();
    assert_eq!(state(n), State::Ready);
    run(&[
        "later",
        &n.to_string(),
        "--text",
        "Reason: code host offline",
    ])
    .unwrap();
    assert_eq!(state(n), State::Later);
    for read in [vec!["status"], vec!["inbox", "N"], vec!["issue", "N"]] {
        let v: Vec<String> = read
            .iter()
            .map(|w| {
                if *w == "N" {
                    n.to_string()
                } else {
                    w.to_string()
                }
            })
            .collect();
        command(&c, &v[0], &Args::parse(&v).unwrap()).unwrap_or_else(|e| panic!("{v:?}: {e}"));
    }
    run(&[
        "close",
        &n.to_string(),
        "--text",
        "Changed: none\nChecks: A1 ok\nOpen: none",
    ])
    .unwrap();
    assert_eq!(state(n), State::Closed);
    let mut left: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert!(
        left.iter().all(|f| f == "taskq.json" || f == ".taskq"),
        "board commands write no checkout: {left:?}"
    );
    // A2: commands that need a checkout refuse with one line naming the reason
    let e = run(&["cleanup"]).unwrap_err();
    assert!(
        e.starts_with("cleanup: board-only root: no code checkout (.git) here\nNext: "),
        "{e}"
    );
    PROJECT_ROOT.set(Some(root.clone()));
    let e = service(&Args::parse(&["service".into(), "install".into()]).unwrap()).unwrap_err();
    PROJECT_ROOT.set(None);
    assert!(e.starts_with("service install: board-only root"), "{e}");
    assert_eq!(
        platform::error_text(&e, None).lines().count(),
        2,
        "Error line names the reason, then Next (§ 5)"
    );
    assert!(
        host::projects(&c.cfg.install)
            .unwrap_or_default()
            .is_empty(),
        "nothing registered"
    );
    fs::create_dir(root.join(".git")).unwrap();
    assert!(needs_checkout(&root, "cleanup").is_ok());
    fs::remove_dir_all(&base).unwrap();
}

// Defect (#1938 A6): `publish: pr` beside `delivery.preset: direct` refused every command, so the setup check could
// not say why. Protection: `check_load` keeps the project readable and reports the disagreement as the delivery item.
#[test]
fn check_load_reports_a_delivery_disagreement_as_an_item() {
    let t = std::env::temp_dir().join(format!("taskq-check-load-{}", crate::runtime::uuid()));
    fs::create_dir_all(&t).unwrap();
    fs::write(
        t.join("taskq.json"),
        r#"{"repo": "o/r", "publish": "pr", "delivery": {"preset": "direct", "revision": 1}}"#,
    )
    .unwrap();
    assert!(
        load(&t).err().is_some_and(|e| e.contains("disagrees")),
        "ordinary commands keep refusing"
    );
    let (cfg, _, delivery) = check_load(&t).unwrap();
    assert_eq!(
        (cfg.delivery.mode(), delivery.unwrap_err().as_str()),
        (
            "direct",
            "taskq.json: publish pr disagrees with delivery.preset direct"
        )
    );
    fs::write(
        t.join("taskq.json"),
        r#"{"repo": "o/r", "delivery": {"preset": "local", "revision": 1}}"#,
    )
    .unwrap();
    assert!(check_load(&t).is_err(), "only the disagreement is lenient");
    fs::remove_dir_all(&t).unwrap();
}

// Defect cs#548 (#2078 A3): the PM installed the shared pre-commit hook by hand on pds18. Protection: `service
// install` runs the declared host setup once, stamps it so the gap clears, and a failing run keeps the gap.
#[cfg(unix)]
#[test]
fn install_runs_the_host_setup_once_and_clears_its_gap() {
    let mut c = cfg();
    c.root = std::env::temp_dir().join(format!("taskq-host-setup-run-{}", crate::runtime::uuid()));
    fs::create_dir_all(c.root.join("scripts")).unwrap();
    let script = c.root.join("scripts/install-git-hooks.sh");
    // `cp` writes the executable from its own process (see tools tests: ETXTBSY under parallel forks)
    fs::write(c.root.join("src.sh"), "#!/bin/sh\necho run >> ran\n").unwrap();
    fs::set_permissions(
        c.root.join("src.sh"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    assert!(
        std::process::Command::new("cp")
            .arg(c.root.join("src.sh"))
            .arg(&script)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(host_setup(&c), None, "undeclared: nothing runs");
    c.host_setup = Some("scripts/install-git-hooks.sh".into());
    let gap = |c: &Config| {
        setup_items(
            c,
            &BTreeMap::new(),
            Ok(crate::config::Delivery::AutoMerge),
            &Value::Null,
            &[],
            None,
            None,
        )
        .into_iter()
        .find(|i| i.id == "host-setup")
        .unwrap()
        .status
    };
    assert_eq!(gap(&c), "gap");
    assert_eq!(
        host_setup(&c).as_deref(),
        Some("host setup: `scripts/install-git-hooks.sh` done")
    );
    assert_eq!(gap(&c), "ok");
    assert_eq!(host_setup(&c), None, "a done setup never reruns");
    assert_eq!(
        fs::read_to_string(c.root.join("ran")).unwrap(),
        "run\n",
        "ran once, in the project root"
    );
    c.host_setup = Some("scripts/missing.sh".into());
    assert!(
        host_setup(&c)
            .unwrap()
            .starts_with("host setup: `scripts/missing.sh` failed: ")
    );
    assert_eq!(gap(&c), "gap", "a failed run keeps the gap");
    // repo-controlled config never runs a script outside the root
    for bad in ["../x.sh", "/bin/sh"] {
        c.host_setup = Some(bad.into());
        assert!(
            host_setup(&c)
                .unwrap()
                .ends_with("refused: not a path inside the project root"),
            "{bad}"
        );
    }
    fs::remove_dir_all(&c.root).unwrap();
}

// Defect (#1938 A6): codex limit 4 with no grant held every Codex launch; with default_runtime codex the reviewer
// never spawned either. Protection: a runtime with a limit or the review default is checked; one at limit 0 is not.
#[test]
fn setup_items_check_runtimes_in_use_only() {
    let mut c = cfg();
    c.default_runtime = crate::runtime::RuntimeId::Codex;
    let refused = |rt: &str| policy::Effective {
        runtime: rt.into(),
        requested: String::new(),
        grant: "none (taskq.json is a request)".into(),
        mode: "legacy".into(),
        enforcement: String::new(),
        profile: None,
        linux: false,
        windows: false,
        claude: None,
        codex: None,
        refused: Some(format!(
            "no operator grant and no policy store protection for {rt} on linux; grant a preset"
        )),
    };
    let access = [refused("claude"), refused("codex")];
    let ids = |limits: &[(&str, u32)]| {
        let limits = limits.iter().map(|(k, n)| (k.to_string(), *n)).collect();
        setup_items(
            &c,
            &limits,
            Ok(crate::config::Delivery::AutoMerge),
            &Value::Null,
            &access,
            None,
            None,
        )
        .into_iter()
        .filter(|i| i.id.starts_with("access "))
        .map(|i| (i.id, i.status))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&[("codex", 4)]),
        [("access codex".to_string(), "gap")],
        "limit 4 and the review default"
    );
    assert_eq!(
        ids(&[]),
        [("access codex".to_string(), "gap")],
        "the review default alone is in use"
    );
    assert_eq!(
        ids(&[("codex", 0), ("claude", 2)]),
        [("access claude".to_string(), "gap")],
        "limit 0 is off"
    );
    let items = setup_items(
        &c,
        &BTreeMap::new(),
        Ok(crate::config::Delivery::AutoMerge),
        &Value::Null,
        &access,
        None,
        None,
    );
    assert_eq!(
        items.iter().find(|i| i.id == "pr-route").unwrap().status,
        "unknown",
        "an unread board stays unknown (R12)"
    );
    // #2118: a healthy fallback removes the mandatory Claude prerequisite, but never hides an explicit pin.
    c.default_runtime = crate::runtime::RuntimeId::Claude;
    let mut codex = refused("codex");
    codex.refused = None;
    codex.grant = "fixture owner grant".into();
    let agents = json!([{"runtime":"claude","login":"not installed"},{"runtime":"codex","login":"logged in"}]);
    for pin in [false, true] {
        let items = setup_items(
            &c,
            &BTreeMap::new(),
            Ok(crate::config::Delivery::AutoMerge),
            &agents,
            &[codex.clone()],
            Some(if pin {
                vec![(1, vec!["run-claude".into()])]
            } else {
                vec![]
            }),
            None,
        );
        assert_eq!(items.iter().any(|i| i.id == "cli claude"), pin);
        assert!(items.iter().any(|i| i.id == "access codex"));
    }
}

// #2138 A3/R8/§4: readiness must honor disabled-pin rerouting without hiding strict pins or access/login gaps.
#[test]
fn balancing_setup_readiness_uses_project_execution_policy() {
    let mut c = cfg();
    c.install = std::env::temp_dir().join(format!("taskq-setup-balancing-{}", runtime::uuid()));
    fs::create_dir_all(&c.install).unwrap();
    let grant = |rt: &str| policy::Effective {
        runtime: rt.into(),
        requested: String::new(),
        grant: "fixture owner grant".into(),
        mode: "full".into(),
        enforcement: String::new(),
        profile: None,
        linux: false,
        windows: false,
        claude: None,
        codex: None,
        refused: None,
    };
    let agents = json!([{"runtime":"claude","login":"not installed"},{"runtime":"codex","login":"logged in"}]);
    for (cause, reroute, expected_pin) in [
        ("limit", true, false),
        ("excluded", true, false),
        ("accounts", true, false),
        ("limit", false, true),
        ("excluded", false, true),
        ("accounts", false, true),
        ("login", true, true),
        ("access", true, true),
        ("quota", true, true),
    ] {
        c.default_runtime = crate::runtime::RuntimeId::Codex;
        c.auto_reroute = reroute;
        c.allowed_runtimes = (cause == "excluded").then(|| vec![crate::runtime::RuntimeId::Codex]);
        let limits = [
            ("claude".into(), if cause == "limit" { 0 } else { 2 }),
            ("codex".into(), 2),
        ]
        .into_iter()
        .collect();
        fs::write(
            c.install.join(runtime::accounts::FILE),
            if cause == "accounts" {
                r#"{"claude":[{"label":"default","enabled":false}]}"#
            } else {
                "{}"
            },
        )
        .unwrap();
        fs::write(
            c.install.join(runtime::accounts::STATE),
            if cause == "quota" {
                r#"{"claude":{"default":{"limited":1,"until":4102444800}}}"#
            } else {
                "{}"
            },
        )
        .unwrap();
        let mut claude = grant("claude");
        if cause == "access" {
            claude.refused = Some("fixture access refusal".into());
        }
        let items = setup_items(
            &c,
            &limits,
            Ok(crate::config::Delivery::AutoMerge),
            &agents,
            &[claude, grant("codex")],
            Some(vec![(1, vec!["run-claude".into()])]),
            None,
        );
        assert_eq!(
            items.iter().any(|i| i.id == "cli claude"),
            expected_pin,
            "{cause}, auto_reroute={reroute}: {items:?}"
        );
        assert!(items.iter().any(|i| i.id == "access codex"));
    }
    // Allowed runtimes also narrow unpinned readiness even with a positive host limit and a healthy excluded default.
    c.default_runtime = crate::runtime::RuntimeId::Claude;
    c.allowed_runtimes = Some(vec![crate::runtime::RuntimeId::Codex]);
    let limits = [("claude".into(), 2), ("codex".into(), 2)]
        .into_iter()
        .collect();
    let agents =
        json!([{"runtime":"claude","login":"logged in"},{"runtime":"codex","login":"logged in"}]);
    let items = setup_items(
        &c,
        &limits,
        Ok(crate::config::Delivery::AutoMerge),
        &agents,
        &[grant("claude"), grant("codex")],
        Some(vec![]),
        None,
    );
    assert!(
        !items.iter().any(|i| i.id.ends_with(" claude")),
        "{items:?}"
    );
    assert!(items.iter().any(|i| i.id == "access codex"));
    let agents = json!([{"runtime":"claude","login":"logged in"},{"runtime":"codex","login":"not logged in"}]);
    let items = setup_items(
        &c,
        &BTreeMap::new(),
        Ok(crate::config::Delivery::AutoMerge),
        &agents,
        &[grant("claude"), grant("codex")],
        Some(vec![]),
        None,
    );
    assert!(
        !items.iter().any(|i| i.id.ends_with(" claude")),
        "{items:?}"
    );
    assert!(
        items
            .iter()
            .any(|i| i.id.ends_with(" codex") && i.status == "gap"),
        "an unavailable allowed fallback must remain visible: {items:?}"
    );
    fs::remove_dir_all(&c.install).unwrap();
}

// Chain: #1661 A2 AS5 Checking. Risk: a probe in progress has no service signal, or the mark drops prior rows.
#[test]
fn a_due_probe_marks_the_last_status_checking() {
    let t = std::env::temp_dir().join(format!("taskq-checking-{}", crate::runtime::uuid()));
    let tool = platform::tool(&t.join("install"));
    let p = coreq::status::path(&tool, &t);
    mark_checking(&tool, &t, 30);
    assert!(!p.exists(), "no prior status: nothing to mark");
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(
        &p,
        json!({"at": 20, "agents": {"at": 20, "rows": [{"runtime": "claude"}]}}).to_string(),
    )
    .unwrap();
    mark_checking(&tool, &t, 30);
    let s: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(
        s["agents"],
        json!({"at": 20, "checking": 30, "rows": [{"runtime": "claude"}]})
    );
    fs::remove_dir_all(&t).unwrap();
}

// Chain: § 5 capacity edit -> the changes line. Risk: a mode or maximum change reads as no change.
#[test]
fn capacity_changes_name_mode_maximum_and_gate() {
    let get = |v: Value| service::capacity(&v).unwrap();
    let auto = get(json!({"admission": {"mode": "auto", "max": 6}, "limits": {"claude": 0}}));
    let none = get(json!({"admission": {"mode": "auto"}, "limits": {"claude": 0}}));
    let fixed = get(
        json!({"admission": {"mode": "fixed", "max": 6}, "load_gate": {"enabled": false}, "limits": {"claude": 0}}),
    );
    assert_eq!(cap_changes(&auto, &none), "Maximum 6 → none");
    assert_eq!(
        cap_changes(&auto, &fixed),
        "Mode Auto → Fixed, Load gate on (CPU 90 %, memory 10 %) → off"
    );
}

pub(crate) fn cfg() -> Config {
    Config {
        // Independent command fixtures must not contend for the same project wake lock.
        root: std::env::temp_dir().join(format!("taskq-main-test-{}", crate::runtime::uuid())),
        install: std::env::temp_dir().join(format!(
            "taskq-main-test-install-{}",
            crate::runtime::uuid()
        )),
        board: &board::github::KIND,
        web_host: String::new(),
        repo: "o/r".into(),
        delivery: crate::config::Delivery::AutoMerge,
        delivery_revision: 0,
        me: "mac".into(),
        limits: BTreeMap::new(),
        has: vec![],
        claim: host::ClaimMode::Any,
        spillover_grace: 300,
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

// Chain: § 9 `claim` (#1493, #1786). Risk: a task filed on a labelled host (worker follow-up or owner/PM plan)
// leaves it for another host, an explicit --host is lost, or an any-claim host starts routing its adds.
#[test]
fn add_on_a_labelled_host_keeps_that_host() {
    let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let mut c = cfg();
    c.claim = host::ClaimMode::Labelled;
    assert_eq!(
        add_host(None, &c).as_deref(),
        Some("mac"),
        "labelled host: add without --host gets host-mac"
    );
    assert_eq!(
        add_host(Some("win"), &c).as_deref(),
        Some("win"),
        "--host wins over the labelled default"
    );
    c.claim = host::ClaimMode::Any;
    assert_eq!(
        add_host(None, &c),
        None,
        "any-claim host: add stays unrouted"
    );
    assert_eq!(add_host(Some("win"), &c).as_deref(), Some("win"));
    let a = parse(&[
        "service", "capacity", "--host", "mac", "--claim", "labelled",
    ])
    .unwrap();
    assert_eq!(
        CapEdit::from_args(&a).unwrap().claim,
        Some(host::ClaimMode::Labelled)
    );
    let bad = parse(&["service", "capacity", "--claim", "some"]).unwrap();
    assert!(CapEdit::from_args(&bad).is_err());
}

// Chain: § 9 `claim` (#1912). Risk: a labelled host adding to a board it does not serve (PilotQ on pds18, a
// mac-only board) gets a refusal without its fix, or the add silently routes to another host.
#[test]
fn add_on_a_labelled_host_the_board_lacks_refuses_naming_service_install() {
    let mut cfg = cfg();
    cfg.claim = host::ClaimMode::Labelled;
    cfg.me = "pds18".into();
    let c = Ctx {
        cfg,
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let add = |extra: &[&str]| {
        let mut v = vec!["add", "T", "--goal", "G", "--acceptance", "observable"];
        v.extend_from_slice(extra);
        command(
            &c,
            "add",
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    let beat = |h: &str| {
        c.board
            .create(
                &format!("taskq host {h}"),
                "version 4.0.0",
                &[service::HOST_LABEL.into()],
            )
            .unwrap()
    };
    beat("mac");
    let refused = "--host pds18: no host named pds18; hosts: mac\nNext: run taskq service install for this project on host pds18 so it serves this board";
    assert_eq!(
        add(&[]).unwrap_err(),
        refused,
        "labelled default: refused, never routed to the board's other host"
    );
    assert_eq!(add(&["--host", "pds18"]).unwrap_err(), refused);
    assert_eq!(
        add(&["--host", "win"]).unwrap_err(),
        "--host win: no host named win; hosts: mac",
        "another host: today's refusal"
    );
    assert_eq!(
        c.board.issues(None).unwrap().len(),
        1,
        "a refused add files nothing"
    );
    add(&["--host", "mac"]).unwrap();
    beat("pds18");
    add(&[]).unwrap();
    assert!(
        c.board
            .issues(None)
            .unwrap()
            .pop()
            .unwrap()
            .labels
            .contains(&"host-pds18".into()),
        "a served board keeps this host"
    );
}

// Chain: § 5 `service capacity --default-runtime` -> host.json `default_runtime` (#1668). Risk: a typo or
// `codex-app` reaches host policy, or the flag alone reads as an empty edit and nothing is saved.
#[test]
fn capacity_default_runtime_accepts_cli_subscriptions_only() {
    let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    for rt in crate::runtime::CLI_RUNTIMES {
        let e = CapEdit::from_args(
            &parse(&[
                "service",
                "capacity",
                "--host",
                "fra",
                "--default-runtime",
                rt,
            ])
            .unwrap(),
        )
        .unwrap();
        assert_eq!(e.default_runtime.as_deref(), Some(rt));
        assert!(!e.is_empty(), "the flag alone is a write");
    }
    for bad in ["codex-app", "gpt"] {
        let a = parse(&[
            "service",
            "capacity",
            "--host",
            "fra",
            "--default-runtime",
            bad,
        ])
        .unwrap();
        assert_eq!(
            CapEdit::from_args(&a).err().unwrap(),
            format!("--default-runtime: claude or codex or antigravity, not {bad}")
        );
    }
}

// #2136 A1/A2: capacity flag and strict task pin must reach existing CLI validation, never a soft hint.
#[test]
fn antigravity_capacity_and_task_pin_are_accepted() {
    let parse =
        |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
    let edit = CapEdit::from_args(&parse(&[
        "service",
        "capacity",
        "--host",
        "invented",
        "--antigravity",
        "0",
        "--default-runtime",
        "antigravity",
    ]))
    .unwrap();
    assert_eq!(edit.limits["antigravity"], 0);
    assert_eq!(edit.default_runtime.as_deref(), Some("antigravity"));
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    command(
        &c,
        "add",
        &parse(&[
            "add",
            "Invented runtime pin",
            "--goal",
            "G",
            "--acceptance",
            "Observable result",
            "--runtime",
            "antigravity",
        ]),
    )
    .unwrap();
    assert!(
        c.board
            .issues(None)
            .unwrap()
            .iter()
            .any(|t| t.labels.contains(&"run-antigravity".into()))
    );
}

// Chain: R6 Anomalies. Defect: every GitLab status printed `board quota unread` though a GitLab instance may send no
// budget headers, so the row asked for an action nobody could take. Risk: a GitHub read without them is a client
// defect and loses its row, or an unknown quota prints as a normal `Board quota:` line.
#[test]
fn board_quota_anomaly_only_where_the_board_always_sends_its_budget() {
    for (kind, anomaly) in [(&board::github::KIND, true), (&board::gitlab::KIND, false)] {
        let mut config = cfg();
        config.board = kind;
        let c = Ctx {
            cfg: config,
            raw: json!({}),
            board: Box::new(Fake::default()),
        };
        let (text, _) = status_text(&c, &Args::default(), None, &mut vec![]).unwrap();
        assert_eq!(
            text.contains("board quota unread"),
            anomaly,
            "{}: {text}",
            kind.name
        );
        assert!(
            !text.contains("Board quota:"),
            "{}: an unknown quota is never a normal line: {text}",
            kind.name
        );
    }
}

// Chain: § 5 CLI arguments. Risk: stray words or a consumed scope/JSON flag change the requested command.
#[test]
fn unused_positionals_are_rejected() {
    let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(
        parse(&["add", "T", "--goal", "G", "--acceptance", "A", "B", "C"])
            .err()
            .unwrap(),
        "unused positional arguments: B C"
    );
    assert_eq!(
        parse(&["inbox", "1", "extra"]).err().unwrap(),
        "unused positional arguments: extra"
    );
    assert_eq!(
        parse(&["issue", "1", "extra"]).err().unwrap(),
        "unused positional arguments: extra"
    );
    assert_eq!(
        parse(&["selftest", "extra"]).err().unwrap(),
        "unused positional arguments: extra"
    );
    assert_eq!(
        parse(&["help", "extra"]).err().unwrap(),
        "unused positional arguments: extra"
    );
    assert!(parse(&["help"]).is_ok());
    assert_eq!(
        parse(&["service", "status", "extra"]).err().unwrap(),
        "unused positional arguments: extra"
    );
    assert_eq!(
        parse(&[
            "add",
            "T",
            "--goal",
            "G",
            "--acceptance",
            "A",
            "--acceptance",
            "B"
        ])
        .unwrap()
        .all("acceptance"),
        vec!["A", "B"]
    );
    assert_eq!(
        parse(&["service", "channel", "dev", "extra"])
            .err()
            .unwrap(),
        "unused positional arguments: extra"
    );
    assert_eq!(
        channel_cmd(&parse(&["service", "channel", "beta"]).unwrap())
            .err()
            .unwrap(),
        "channel must be dev or live"
    );
    // #2311: `service dev-build` writes the dev request only on channel dev
    let install = std::env::temp_dir().join(format!("taskq-dev-build-{}", std::process::id()));
    let _ = fs::remove_dir_all(&install);
    fs::create_dir_all(&install).unwrap();
    let build = parse(&["service", "dev-build"]).unwrap();
    assert!(
        dev_build_cmd(&build, &install)
            .unwrap_err()
            .starts_with("channel live:"),
        "live host: refused"
    );
    assert!(!platform::dev_request(&install).exists());
    fs::write(install.join("host.json"), r#"{"channel":"dev"}"#).unwrap();
    dev_build_cmd(&build, &install).unwrap();
    assert!(
        platform::dev_request(&install).exists(),
        "dev host: request written"
    );
    assert_eq!(
        parse(&["service", "dev-build", "x"]).err().unwrap(),
        "unused positional arguments: x"
    );
    fs::remove_dir_all(&install).unwrap();
    for v in [
        vec!["close", "1", "2"],
        vec!["answer", "1.1", "2.1"],
        vec!["service", "focus", "code", "docs"],
        vec!["service", "channel"],
        vec!["service", "channel", "dev"],
        vec!["service", "channel", "live"],
    ] {
        assert!(parse(&v).is_ok());
    }
    for flags in [
        vec!["--host"],
        vec!["--host", "--json"],
        vec!["--json", "--host", "--request", "original-id"],
    ] {
        let mut v = vec!["service", "uninstall"];
        v.extend_from_slice(&flags);
        let parsed = parse(&v).unwrap();
        assert_eq!(
            parsed.one("host"),
            Some(""),
            "host uninstall scope consumed another option"
        );
        assert_eq!(parsed.flags.contains_key("json"), flags.contains(&"--json"));
        if flags.contains(&"--request") {
            assert_eq!(parsed.one("request"), Some("original-id"));
        }
    }
    for v in [
        vec!["service", "capacity", "--host", "fixture-host"],
        vec!["add", "T", "--host", "fixture-host"],
    ] {
        assert_eq!(
            parse(&v).unwrap().one("host"),
            Some("fixture-host"),
            "host-name option lost its value"
        );
    }
}

// Chain: § 5 taskq add -> § 1 held -> start. Risk: an invalid task is written, or a new task is admitted without `start`.
// #1929 A1/R1: creation owns one immutable chat link; replay cannot assign twice or retarget it.
#[test]
fn conversation_1929_hold_is_trusted_immutable_and_creation_bound() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({"create_claim_issue":1}),
        board: Box::new(Fake::default()),
    };
    c.board.create("Claims", "", &[]).unwrap();
    let task = NewTask {
        title: "T".into(),
        goal: "G".into(),
        acceptance: vec!["Observable".into()],
        conversation: Some("owner-1929".into()),
        ..Default::default()
    };
    assert!(add_task(&*c.board, &c.cfg.root, &c.raw, &task, &Op::default()).is_err());
    let op = Op {
        id: Some("pm.owner-1929.1".into()),
        revision: None,
    };
    assert!(
        add_task(&*c.board, &c.cfg.root, &c.raw, &task, &op)
            .unwrap()
            .ok()
    );
    let log = entries(&*c.board, 2).unwrap();
    assert_eq!(core::conversation(2, &log).as_deref(), Some("owner-1929"));
    let replay = add_task(&*c.board, &c.cfg.root, &c.raw, &task, &op).unwrap();
    assert!(replay.ok(), "{replay:?}");
    assert_eq!(c.board.issues(None).unwrap().len(), 2);
    let changed = NewTask {
        conversation: Some("other-chat".into()),
        ..task
    };
    assert!(
        !add_task(&*c.board, &c.cfg.root, &c.raw, &changed, &op)
            .unwrap()
            .ok()
    );
    let mut hostile = log.clone();
    hostile.push(Entry {
        id: 999,
        body: "taskq: hold\nconversation: forged".into(),
        edited: false,
    });
    assert_eq!(
        core::conversation(2, &hostile).as_deref(),
        Some("owner-1929")
    );
    hostile[0].edited = true;
    assert!(core::conversation(2, &hostile).is_none_or(|id| id != "owner-1929"));
    c.board.comment(2, "taskq: start").unwrap();
    let attempt = c
        .board
        .comment(2, "taskq: spawn worker claude:fixture mac")
        .unwrap();
    c.board.comment(2,&format!("taskq: ask attempt {attempt}\nDone: Original work\nProblem: Keep compatibility?\nEvidence: [Original](https://example.org/evidence)\n1. Keep it ★\n2. Remove it")).unwrap();
    c.board.comment(2, "taskq: answer 1").unwrap();
    c.board
        .comment(
            2,
            &format!(
                "taskq: result {} attempt {attempt}\nChanged: Done\nOpen: none",
                "a".repeat(40)
            ),
        )
        .unwrap();
    c.board
        .comment(2, "taskq: close\nChanged: Done\nOpen: none")
        .unwrap();
    let mut issue = c.board.issue(2).unwrap();
    issue.open = false;
    issue.reason = Some("completed".into());
    let closed = closed_row(&*c.board, &issue, None, false);
    let questions: Vec<_> = closed["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| !r["question"].is_null())
        .collect();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0]["question"]["done"], "Original work");
    assert_eq!(
        questions[0]["question"]["evidence"][0]["url"],
        "https://example.org/evidence"
    );
    assert_eq!(closed["conversation"]["id"], "owner-1929");
}

#[test]
fn add_uses_canonical_names_defaults_and_validates_before_writing() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let args = |extra: &[&str]| {
        let mut v = vec!["add", "T", "--goal", "G", "--acceptance", "observable"];
        v.extend_from_slice(extra);
        Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    };
    let sorted = |n: u64| {
        let mut l = c.board.issue(n).unwrap().labels;
        l.sort();
        l
    };
    command(&c, "add", &args(&[])).unwrap();
    let i = c.board.issues(None).unwrap().pop().unwrap();
    // § 1: every add is held, `--hold` or not; no flag skips it
    assert_eq!(i.labels, vec!["q-held"]);
    assert_eq!(
        fold(i.number, &entries(&*c.board, i.number).unwrap(), false).state,
        State::Held
    );
    assert!(
        command(&c, "add", &args(&["--later"])).is_err_and(|e| e.contains("held")),
        "--later cannot skip hold"
    );
    command(
        &c,
        "add",
        &args(&[
            "--priority",
            "1",
            "--type",
            "code",
            "--deps",
            &i.number.to_string(),
        ]),
    )
    .unwrap();
    let j = c.board.issues(None).unwrap().pop().unwrap();
    assert_eq!(sorted(j.number), vec!["code", "p1", "q-held"]);
    command(
        &c,
        "start",
        &Args::parse(&["start".into(), j.number.to_string()]).unwrap(),
    )
    .unwrap();
    let j = c.board.issue(j.number).unwrap();
    assert_eq!(
        sorted(j.number),
        vec!["code", "p1", "q-blocked", "taskq"],
        "start: dependency still open"
    );
    assert!(in_scope(&j, &["q-waiting".into(), "priority-1".into()]));
    command(&c, "add", &args(&["--runtime", "codex-app"])).unwrap();
    assert!(
        c.board
            .issues(None)
            .unwrap()
            .pop()
            .unwrap()
            .labels
            .contains(&"run-codex-app".into())
    );
    command(&c, "add", &args(&["--priority", "0", "--hold"])).unwrap();
    assert_eq!(
        c.board.issues(None).unwrap().pop().unwrap().labels,
        vec!["q-held", "p0"]
    );
    assert!(
        command(&c, "add", &args(&["--prefer", "codex"]))
            .is_err_and(|e| e.contains("--runtime only for an owner pin")),
        "§ 9: --prefer refused with the --runtime hint"
    );
    for extra in [
        &["--priority", "3"][..],
        &["--runtime", "any"],
        &["--type", "code", "--type", "docs"],
        &["--host", "win,mac"],
        &["--prefer", "codex-app"],
        &["--prefer", "codex", "--prefer", "claude"],
    ] {
        assert!(command(&c, "add", &args(extra)).is_err());
    }
    // #1558: a repeated single-value flag is refused by name and files nothing, never kept as the last value
    for flag in [
        "--goal",
        "--type",
        "--runtime",
        "--priority",
        "--host",
        "--op-id",
        "--revision",
        "--text",
    ] {
        let e = command(&c, "add", &args(&[flag, "1", flag, "2"])).unwrap_err();
        assert!(
            e.contains(flag) && e.contains("more than once"),
            "{flag}: {e}"
        );
    }
    assert_eq!(c.board.issues(None).unwrap().len(), 4);
    command(
        &c,
        "add",
        &Args::parse(
            &[
                "add",
                "T",
                "--goal",
                "G1\nG2",
                "--acceptance",
                "a",
                "--acceptance",
                "b",
                "--scope",
                "a",
                "--scope",
                "b",
            ]
            .map(String::from),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        c.board
            .issues(None)
            .unwrap()
            .pop()
            .unwrap()
            .body
            .contains("## Goal\nG1\nG2\n"),
        "two-line goal through one value"
    );
    assert!(
        task_body(
            &*c.board,
            &NewTask {
                title: "T".into(),
                goal: "G".into(),
                acceptance: vec!["observable".into()],
                labels: vec!["prefer-codex".into(), "prefer-claude".into()],
                ..Default::default()
            },
            &Op::default()
        )
        .is_err_and(|e| e.contains("prefer"))
    );
}

// Chain: § 1 `add --host` -> heartbeat names (§ 4 step 3). Risk (#1721): a `host-host-mac` or misspelt host label
// files a task no host ever claims, and it sits ready with no reason shown.
#[test]
fn add_host_refuses_prefixed_and_unknown_names_before_writing() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    c.board
        .create(
            "taskq host mac",
            "version 4.0.0",
            &[service::HOST_LABEL.into()],
        )
        .unwrap();
    let add = |host: &str| {
        command(
            &c,
            "add",
            &Args::parse(
                &[
                    "add",
                    "T",
                    "--goal",
                    "G",
                    "--acceptance",
                    "observable",
                    "--host",
                    host,
                ]
                .map(String::from),
            )
            .unwrap(),
        )
    };
    assert_eq!(
        add("host-mac").unwrap_err(),
        "--host takes the host name without host-: --host mac"
    );
    assert_eq!(
        add("macbook").unwrap_err(),
        "--host macbook: no host named macbook; hosts: mac"
    );
    assert_eq!(
        c.board.issues(None).unwrap().len(),
        1,
        "a refused add files nothing"
    );
    add("mac").unwrap();
    assert!(
        c.board
            .issues(None)
            .unwrap()
            .pop()
            .unwrap()
            .labels
            .contains(&"host-mac".into())
    );
}

// Chain: § 1 Task text, #2241 unknown term -> add/start refusal; declared rules -> body `Rules:` line for the gate.
// Risk: vague task text reaches a worker and comes back as a technical ask, or a known term is refused.
#[test]
fn add_and_start_refuse_unknown_terms_and_record_declared_rules() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let run = |v: &[&str]| {
        command(
            &c,
            "add",
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    let e = run(&[
        "add",
        "T",
        "--goal",
        "Fix R99 and §2.999",
        "--acceptance",
        "`taskq frobnicate 1` prints x",
    ])
    .unwrap_err();
    assert!(
        e.contains("unknown term: R99, §2.999, taskq frobnicate"),
        "{e}"
    );
    assert!(
        c.board.issues(None).unwrap().is_empty(),
        "a refused add files nothing"
    );
    run(&[
        "add",
        "T",
        "--goal",
        "Amend R7, §2.3 and § 4; add new R14",
        "--acceptance",
        "`taskq status --json` and `taskq answer 1.2` work; see #12 and PR42",
        "--rules",
        "R7, §4 Profiling",
    ])
    .unwrap();
    let i = c.board.issues(None).unwrap().pop().unwrap();
    assert!(i.body.contains("\nRules: R7, §4 Profiling\n"), "{}", i.body);
    run(&[
        "add",
        "T2",
        "--goal",
        "G",
        "--acceptance",
        "a",
        "--rules",
        "all",
    ])
    .unwrap();
    for bad in ["R99", "x", ""] {
        assert!(
            run(&[
                "add",
                "T3",
                "--goal",
                "G",
                "--acceptance",
                "a",
                "--rules",
                bad
            ])
            .unwrap_err()
            .contains("--rules"),
            "{bad}"
        );
    }
    // a body edited after add is linted again at start, before any write
    let n = c
        .board
        .create(
            "U",
            "## Goal\nUse `taskq nope`\n\n## Acceptance\nA1. R0 holds\n\n## Scope\n- R99 path\n",
            &["q-held".into()],
        )
        .unwrap();
    let before = entries(&*c.board, n).unwrap().len();
    let e = start_tasks(&*c.board, &c.cfg.root, &[n.to_string()], &Op::default()).unwrap_err();
    assert_eq!(
        e,
        format!(
            "#{n}: unknown term: R0, taskq nope; fix the task text (taskq.md, `taskq help`) or write `new <ID>`; nothing started"
        )
    );
    assert_eq!(entries(&*c.board, n).unwrap().len(), before);
}

// Chain: F1/F2 local finding -> held issue -> replay. Risk: duplicate or wrong-board writes.
#[test]
fn finding_cli_held_replay_payload_and_board_boundaries() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({"create_claim_issue": 1}),
        board: Box::new(Fake::default()),
    };
    c.board.create("Claims", "", &[]).unwrap();
    let base = [
        "finding",
        "Investigate",
        "--board",
        "https://board",
        "--source",
        "install/rule/episode",
        "--goal",
        "G",
        "--acceptance",
        "a",
        "--evidence",
        "Depends on #999",
    ];
    let run = |v: &[&str]| {
        command(
            &c,
            "finding",
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    run(&base).unwrap();
    run(&base).unwrap();
    let i = c.board.issue(2).unwrap();
    assert_eq!(i.labels, ["q-held"]);
    assert!(i.body.contains("## Source\ninstall/rule/episode"));
    assert!(i.body.contains("## Evidence\n- Depends on #999"));
    assert!(
        core::depends(&i.body).is_empty(),
        "source evidence is data, never a dependency"
    );
    assert!(
        c.board.comments(2).unwrap().is_empty(),
        "default finding never starts"
    );
    assert_eq!(c.board.issues(Some(0)).unwrap().len(), 2);
    for (index, changed) in [
        (1, "Changed"),
        (3, "https://other"),
        (5, "../rule/episode"),
        (7, "Changed goal"),
        (9, "Changed acceptance"),
        (11, "Changed evidence"),
    ] {
        let mut v = base;
        v[index] = changed;
        assert!(run(&v).is_err(), "changed field {index}");
    }
    for extra in [
        ["--op-id", "different"],
        ["--repo", "other/repo"],
        ["--host", "win,mac"],
        ["--goal", "second"],
        ["--evidence", "line\nline"],
    ] {
        assert!(run(&[&base[..], &extra].concat()).is_err());
    }
    assert!(
        run(&[&base[..], &["--start"]].concat()).is_err(),
        "start choice is immutable; separate start remains available"
    );
    assert_eq!(
        c.board.issues(Some(0)).unwrap().len(),
        2,
        "denied input preserves the sole issue"
    );
    assert_eq!(
        c.board.comments(1).unwrap().len(),
        1,
        "denied/replayed requests never add another claim"
    );
    c.board.close(2).unwrap();
    run(&base).unwrap();
    assert_eq!(
        c.board.issues(Some(0)).unwrap().len(),
        2,
        "closed episode never recreates"
    );
    let mut another = base;
    another[5] = "install/rule/another";
    run(&another).unwrap();
    let stale = Op {
        id: Some("later-start".into()),
        revision: Some(1),
    };
    assert_eq!(
        start_one(c.board.as_ref(), &c.cfg.root, 3, &stale).outcome,
        "stale"
    );
    assert!(
        c.board.comments(3).unwrap().is_empty(),
        "stale start cannot initialize hold"
    );
    c.board.comment(3, "taskq: unsupported").unwrap();
    let fresh = Op {
        revision: Some(0),
        ..stale
    };
    assert!(
        start_one(c.board.as_ref(), &c.cfg.root, 3, &fresh).ok(),
        "held default supports standalone start"
    );
    let mut explicit = base;
    explicit[5] = "install/rule/explicit";
    let explicit = [&explicit[..], &["--start", "--json"]].concat();
    run(&explicit).unwrap();
    run(&explicit).unwrap();
    assert_eq!(c.board.issues(Some(0)).unwrap().len(), 4);
    assert!(service::is_task(&c.board.issue(4).unwrap()));
    assert_eq!(
        c.board.comments(4).unwrap().len(),
        2,
        "one hold and one start across retries"
    );
}

#[test]
fn finding_start_adapter_failures_preserve_create_and_replay() {
    // Chain: F2 create readback -> failed admission -> same-ID recovery; no issue or receipt is lost.
    for failure in [
        "initialize",
        "hold-read",
        "start-read",
        "readback",
        "labels",
    ] {
        let b = Fake::default();
        b.create("Claims", "", &[]).unwrap();
        let raw = json!({"create_claim_issue": 1});
        let op = Op {
            id: Some("finding.failure".into()),
            revision: None,
        };
        let root = cfg().root;
        let submit = || {
            finding_submit(
                &b,
                &raw,
                &root,
                "T",
                "G\nop-id: finding.failure\n",
                &["q-held".into()],
                "start:true",
                &op,
                true,
            )
        };
        {
            let mut st = b.state.borrow_mut();
            st.fail_issue_read = match failure {
                "initialize" => Some((2, 2)),
                "hold-read" => Some((2, 3)),
                "start-read" => Some((2, 4)),
                "readback" => Some((2, 5)),
                _ => None,
            };
            st.fail_labels = failure == "labels";
        }
        let (created, started) = submit();
        assert_eq!(
            (created.outcome, created.task),
            ("accepted", 2),
            "{failure}"
        );
        let started = started.unwrap();
        assert_eq!(
            (started.outcome, started.task, started.op_id.as_deref()),
            ("unknown", 2, Some("finding.failure.start")),
            "{failure}"
        );
        assert!(emit_finding(created, Some(started), true).is_err());
        let (created, started) = submit();
        assert!(created.ok() && started.unwrap().ok(), "{failure}");
        let count = b.comments(2).unwrap().len();
        assert_eq!(count, 2, "one hold and one start: {failure}");
        let (created, started) = submit();
        assert!(created.ok() && started.unwrap().ok());
        assert_eq!(b.comments(2).unwrap().len(), count);
        assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
        assert!(service::is_task(&b.issue(2).unwrap()));
        assert!(!service::is_held(&b.issue(2).unwrap()));
    }
}

#[test]
fn finding_separate_start_receipts_recover_without_recreate() {
    // Chain: F2 create success -> unknown start -> recovered start. Risk: losing the issue or admitting twice.
    let b = Fake::default();
    b.create("Claims", "", &[]).unwrap();
    let raw = json!({"create_claim_issue": 1});
    let op = Op {
        id: Some("finding.test".into()),
        revision: None,
    };
    let body = "G\nop-id: finding.test\n";
    let labels = ["q-held".into()];
    let root = cfg().root;
    let submit = || finding_submit(&b, &raw, &root, "T", body, &labels, "start:true", &op, true);
    b.state.borrow_mut().hide_latest_comment = true;
    let (created, started) = submit();
    assert_eq!(created.outcome, "unknown", "a hidden claim cannot create");
    assert!(started.is_none());
    b.state.borrow_mut().hide_latest_comment = false;
    // A claimant whose response was lost may only discover, never POST another issue.
    b.create("T", body, &labels).unwrap();
    b.state.borrow_mut().hide_latest_comment = true;
    let (created, started) = submit();
    assert_eq!((created.outcome, created.task), ("accepted", 2));
    assert_eq!(started.unwrap().outcome, "unknown");
    assert_eq!(b.issue(2).unwrap().labels, ["q-held"]);
    b.state.borrow_mut().hide_latest_comment = false;
    let (created, started) = submit();
    assert!(created.ok());
    assert!(started.unwrap().ok());
    assert_eq!(b.issue(2).unwrap().labels, ["q-ready", "taskq"]);
    let count = b.comments(2).unwrap().len();
    let (created, started) = submit();
    assert!(created.ok() && started.unwrap().ok());
    assert_eq!(b.comments(2).unwrap().len(), count);
    assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
    b.labels(2, &["q-held".into()], &["q-ready".into(), "taskq".into()])
        .unwrap();
    assert!(
        submit().1.unwrap().ok(),
        "retry repairs partial eligibility projection"
    );
    assert_eq!(b.issue(2).unwrap().labels, ["q-ready", "taskq"]);
    assert_eq!(
        b.comments(2).unwrap().len(),
        count,
        "projection recovery adds no log entries"
    );
    b.comment(2, &entry("spawn", &["worker", "codex:s", "win"], None, ""))
        .unwrap();
    assert_eq!(
        submit().1.unwrap().state,
        Some("doing"),
        "replay never restarts active work"
    );
    b.close(2).unwrap();
    assert_eq!(submit().0.state, Some("closed"));
    assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
    let rejected_op = Op {
        id: Some("finding.rejected".into()),
        revision: None,
    };
    let rejected_body = "G\nop-id: finding.rejected\n";
    let n = claimed_create(
        &b,
        &raw,
        "U",
        rejected_body,
        &labels,
        "start:true",
        &rejected_op,
    )
    .unwrap();
    b.close(n).unwrap();
    let (created, started) = finding_submit(
        &b,
        &raw,
        &root,
        "U",
        rejected_body,
        &labels,
        "start:true",
        &rejected_op,
        true,
    );
    assert_eq!((created.outcome, created.task), ("accepted", n));
    assert_eq!(started.unwrap().outcome, "rejected");
    assert_eq!(
        b.issues(Some(0)).unwrap().len(),
        3,
        "rejected start keeps its existing issue"
    );
}

// Chain: § 1 `taskq claim-issue` (#1907) -> taskq.json. Defect: the PM wrote create_claim_issue by hand on a new board.
// Risk: the key is not read back, other keys are lost, an existing key is replaced, or a rerun creates again.
#[test]
fn claim_issue_setup_writes_key_once_and_keeps_config() {
    let root = std::env::temp_dir().join(format!("taskq-claim-issue-{}", runtime::uuid()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("taskq.json");
    fs::write(
        &path,
        "{\n  \"repo\": \"o/r\",\n  \"limits\": {\"claude\": 2}\n}\n",
    )
    .unwrap();
    let b = Fake::default();
    let r = claim_issue_setup(&b, &root, None).unwrap();
    assert_eq!(
        (
            r["outcome"].as_str(),
            r["source"].as_str(),
            r["issue"].as_u64(),
            r["key"].as_str()
        ),
        (
            Some("accepted"),
            Some("created"),
            Some(1),
            Some("create_claim_issue")
        ),
        "{r}"
    );
    assert_eq!(r["url"], b.url(1));
    let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        v.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["repo", "limits", "create_claim_issue"]
    );
    assert_eq!(
        v,
        json!({"repo": "o/r", "limits": {"claude": 2}, "create_claim_issue": 1})
    );
    assert_eq!(
        claim_issue_setup(&b, &root, None).unwrap()["source"],
        "configured"
    );
    b.create("Other", "", &[]).unwrap();
    assert_eq!(
        claim_issue_setup(&b, &root, Some(2)).unwrap()["outcome"],
        "rejected"
    );
    assert_eq!(
        b.issues(Some(0)).unwrap().len(),
        2,
        "reruns create no claim issue"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap()).unwrap()["create_claim_issue"],
        1
    );
    fs::write(&path, r#"{"create_claim_issue": "x"}"#).unwrap();
    assert!(
        claim_issue_setup(&b, &root, None).is_err(),
        "a malformed key is not replaced"
    );
    fs::remove_dir_all(&root).ok();
}

// Chain: §1 claimed create -> readback/retry. Risk: duplicate issues after a race, restart or lost response.
#[test]
fn create_claim_race_and_unknown_post_reconcile_without_another_post() {
    let raw = json!({"create_claim_issue": 1});
    let op = Op {
        id: Some("finding.1".into()),
        ..Default::default()
    };
    let body = "## Goal\nG\n\nop-id: finding.1\n";
    let labels = vec!["q-held".into()];
    let b = Fake::default();
    b.create("Claims", "", &[]).unwrap();
    let n = claimed_create(&b, &raw, "T", body, &labels, "", &op).unwrap();
    let claim = b.comments(1).unwrap()[0].body.clone();
    assert_eq!(
        claimed_create(&b, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "accepted"
    );
    for (title, text, labels) in [
        ("Changed", "", labels.clone()),
        ("T", "changed", labels.clone()),
        ("T", "", vec!["taskq".into()]),
    ] {
        assert_eq!(
            claimed_create(&b, &raw, title, body, &labels, text, &op)
                .unwrap_err()
                .outcome,
            "rejected"
        );
    }
    b.close(n).unwrap();
    assert_eq!(
        claimed_create(&b, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .state,
        Some("closed")
    );
    assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
    b.create("T", body, &labels).unwrap();
    assert_eq!(
        claimed_create(&b, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "unknown",
        "duplicate evidence is never accepted"
    );

    let trusted = Fake::default();
    trusted.create("Claims", "", &[]).unwrap();
    trusted.raw(
        1,
        "taskq: create-claim finding.1\nrequest-sha256: forged",
        false,
        false,
    );
    trusted.raw(
        1,
        "taskq: create-claim finding.1\nrequest-sha256: edited",
        true,
        true,
    );
    assert!(
        claimed_create(&trusted, &raw, "T", body, &labels, "", &op).is_ok(),
        "untrusted and edited claims have no authority"
    );
    assert_eq!(
        claimed_create(&trusted, &raw, "T", "changed body", &labels, "", &op)
            .unwrap_err()
            .outcome,
        "rejected"
    );
    trusted.labels(1, &["taskq".into()], &[]).unwrap();
    assert_eq!(
        claimed_create(&trusted, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "rejected",
        "a task cannot be the claim issue"
    );

    let legacy = Fake::default();
    legacy.create("Claims", "", &[]).unwrap();
    legacy.create("T", body, &labels).unwrap();
    assert_eq!(
        claimed_create(&legacy, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "rejected"
    );
    assert!(
        legacy.comments(1).unwrap().is_empty(),
        "legacy reconciliation creates no claim"
    );

    // Two invocations read no claim. The competitor's claim lands just before ours;
    // it is paused before its issue POST, so our invocation must only discover.
    let race = Fake::default();
    race.create("Claims", "", &[]).unwrap();
    race.state.borrow_mut().race = Some(claim);
    let r = claimed_create(&race, &raw, "T", body, &labels, "", &op).unwrap_err();
    assert_eq!(r.outcome, "unknown");
    assert_eq!(
        race.comments(1).unwrap().len(),
        2,
        "both contenders posted claims"
    );
    assert_eq!(
        race.issues(None).unwrap().len(),
        1,
        "loser never POSTs an issue"
    );
    assert_eq!(
        claimed_create(&race, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "unknown"
    );
    // Winner's POST has an unknown response and is absent from discovery until later.
    let n = race.create("T", body, &labels).unwrap();
    race.advance(10000);
    let r = claimed_create(&race, &raw, "T", body, &labels, "", &op).unwrap_err();
    assert_eq!((r.outcome, r.task, r.state), ("accepted", n, Some("held")));
    assert_eq!(
        race.comments(1).unwrap().len(),
        2,
        "recovery writes no comments"
    );
    assert_eq!(
        race.issues(None).unwrap().len(),
        2,
        "recovery writes no issues"
    );

    let hidden = Fake::default();
    hidden.create("Claims", "", &[]).unwrap();
    hidden.state.borrow_mut().hide_latest_comment = true;
    assert_eq!(
        claimed_create(&hidden, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "unknown"
    );
    assert_eq!(
        hidden.issues(None).unwrap().len(),
        1,
        "missing claim readback cannot create"
    );
    hidden.state.borrow_mut().hide_latest_comment = false;
    assert_eq!(
        claimed_create(&hidden, &raw, "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "unknown"
    );
    assert_eq!(
        hidden.comments(1).unwrap().len(),
        1,
        "a crashed claimant is never replaced"
    );
    assert_eq!(
        claimed_create(&hidden, &json!({}), "T", body, &labels, "", &op)
            .unwrap_err()
            .outcome,
        "rejected"
    );
}

// Chain: §1 create POST -> lost response -> delayed readback. Risk: a retry duplicates the accepted POST.
#[test]
fn create_claim_adapter_lost_posts_and_interleaved_creates() {
    use std::cell::Cell;
    struct Writes {
        b: Fake,
        lose_create: Cell<bool>,
        lose_claim: Cell<bool>,
        hide_issue: Cell<bool>,
        race: Cell<bool>,
    }
    impl Board for Writes {
        fn issues(&self, since: Option<i64>) -> board::Result<Vec<board::Issue>> {
            Ok(self
                .b
                .issues(since)?
                .into_iter()
                .filter(|i| !self.hide_issue.get() || i.number == 1)
                .collect())
        }
        fn issue(&self, n: u64) -> board::Result<board::Issue> {
            self.b.issue(n)
        }
        fn comments(&self, n: u64) -> board::Result<Vec<board::Comment>> {
            self.b.comments(n)
        }
        fn comment(&self, n: u64, body: &str) -> board::Result<u64> {
            if self.race.replace(false) {
                let op = Op {
                    id: Some("lost.1".into()),
                    ..Default::default()
                };
                claimed_create(
                    &self.b,
                    &json!({"create_claim_issue": 1}),
                    "T",
                    "G\nop-id: lost.1\n",
                    &["q-held".into()],
                    "",
                    &op,
                )
                .unwrap();
            }
            let id = self.b.comment(n, body)?;
            if self.lose_claim.replace(false) {
                Err("claim POST response lost".into())
            } else {
                Ok(id)
            }
        }
        fn create(&self, title: &str, body: &str, labels: &[String]) -> board::Result<u64> {
            let n = self.b.create(title, body, labels)?;
            if self.lose_create.replace(false) {
                Err("create POST response lost".into())
            } else {
                Ok(n)
            }
        }
        fn me(&self) -> board::Result<String> {
            self.b.me()
        }
        fn edit_comment(&self, n: u64, id: u64, body: &str) -> board::Result<()> {
            self.b.edit_comment(n, id, body)
        }
        fn labels(&self, n: u64, add: &[String], remove: &[String]) -> board::Result<()> {
            self.b.labels(n, add, remove)
        }
        fn edit(&self, n: u64, body: &str) -> board::Result<()> {
            self.b.edit(n, body)
        }
        fn close(&self, n: u64) -> board::Result<()> {
            self.b.close(n)
        }
        fn now(&self) -> board::Result<i64> {
            self.b.now()
        }
        fn publish(
            &self,
            n: u64,
            sha: &str,
            mode: &str,
            title: &str,
            task_line: &str,
        ) -> board::Publish {
            self.b.publish(n, sha, mode, title, task_line)
        }
        fn url(&self, n: u64) -> String {
            self.b.url(n)
        }
        fn summaries(&self) -> bool {
            self.b.summaries()
        }
    }
    for mode in ["create", "claim", "race"] {
        let b = Writes {
            b: Fake::default(),
            lose_create: Cell::new(mode == "create"),
            lose_claim: Cell::new(mode == "claim"),
            hide_issue: Cell::new(false),
            race: Cell::new(mode == "race"),
        };
        b.b.create("Claims", "", &[]).unwrap();
        let op = Op {
            id: Some("lost.1".into()),
            ..Default::default()
        };
        let run = || {
            claimed_create(
                &b,
                &json!({"create_claim_issue": 1}),
                "T",
                "G\nop-id: lost.1\n",
                &["q-held".into()],
                "",
                &op,
            )
        };
        let r = run().unwrap_err();
        assert_eq!(
            r.outcome,
            if mode == "race" {
                "accepted"
            } else {
                "unknown"
            }
        );
        let count = b.b.issues(None).unwrap().len();
        assert_eq!(count, if mode == "claim" { 1 } else { 2 });
        b.hide_issue.set(true);
        assert_eq!(
            run().unwrap_err().outcome,
            "unknown",
            "empty discovery cannot authorize another POST"
        );
        b.hide_issue.set(false);
        let r = run().unwrap_err();
        assert_eq!(
            r.outcome,
            if mode == "claim" {
                "unknown"
            } else {
                "accepted"
            }
        );
        assert_eq!(b.b.issues(None).unwrap().len(), count);
        assert_eq!(
            b.comments(1).unwrap().len(),
            if mode == "race" { 2 } else { 1 }
        );
    }
}

// #2362: a logged start with failed labels must stay visible and repair without another admission entry.
#[test]
fn logged_start_repairs_eligibility_and_reports_mismatch() {
    for (dependency, eligibility_write) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let b = Fake::default();
        let cfg = cfg();
        let dep = b.create("Dependency", "", &[]).unwrap();
        let n = b
            .create(
                "Logged start",
                if dependency { "Depends on #1" } else { "" },
                &["q-held".into()],
            )
            .unwrap();
        b.comment(n, "taskq: hold").unwrap();
        if eligibility_write {
            b.state.borrow_mut().before_labels = Some(Box::new(|b, _| {
                b.state.borrow_mut().before_labels =
                    Some(Box::new(|b, _| b.state.borrow_mut().fail_labels = true));
            }));
        } else {
            b.state.borrow_mut().fail_labels = true;
        }
        let first = start_task(&b, &cfg.root, n, &Op::default()).unwrap();
        assert!(!first.ok());
        let failed_labels = if eligibility_write {
            if dependency { "q-blocked" } else { "q-ready" }
        } else {
            "q-held"
        };
        assert_eq!(b.issue(n).unwrap().labels, [failed_labels]);
        assert!(
            !service::is_task(&b.issue(n).unwrap()),
            "failed start is not eligible for admission"
        );
        let count = entries(&b, n).unwrap().len();
        assert_eq!(count, 2, "hold and accepted start survive label failure");
        let state = if dependency {
            State::Waiting
        } else {
            State::Ready
        };
        let snap = snapshot::read(&b, &cfg, &json!({}), &[]);
        let row = snap.tasks.iter().find(|r| r.n == n).unwrap();
        let stale = Op {
            revision: Some(0),
            ..Op::default()
        };
        assert!(!start_task(&b, &cfg.root, n, &stale).unwrap().ok());
        assert_eq!(b.issue(n).unwrap().labels, [failed_labels]);
        let retry = start_tasks(&b, &cfg.root, &[n.to_string()], &Op::default()).unwrap();
        assert!(
            retry[0].ok(),
            "retry must repair: {}; status before retry: {:?}",
            retry[0].text(),
            row.task.state
        );
        assert_eq!(
            row.task.state, state,
            "status must not conceal the logged start as held"
        );
        assert_eq!(row.json()["admission"]["code"], "labels");
        let text = snap.text(&cfg, &json!({})).unwrap();
        assert!(
            text.contains("start eligibility labels incomplete")
                && text.contains(&format!("taskq start {n}")),
            "{text}"
        );
        let issue = b.issue(n).unwrap();
        assert!(service::is_task(&issue) && !issue.labels.contains(&"q-held".into()));
        assert!(issue.labels.contains(&format!(
            "q-{}",
            if dependency { "blocked" } else { "ready" }
        )));
        assert_eq!(
            entries(&b, n).unwrap().len(),
            count,
            "repair must not duplicate the start log"
        );
        assert_eq!(
            snapshot::read(&b, &cfg, &json!({}), &[])
                .tasks
                .iter()
                .find(|r| r.n == n)
                .unwrap()
                .task
                .state,
            state
        );
        assert!(
            start_tasks(&b, &cfg.root, &[n.to_string()], &Op::default()).is_err(),
            "eligible work still refuses a fresh start"
        );
        b.close(dep).unwrap();
    }
}

// Chain: § 1 held creation -> explicit start. Risk: idempotent retries admit held work.
#[test]
fn held_create_is_idempotent_and_starts_only_on_start() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({"create_claim_issue": 1}),
        board: Box::new(Fake::default()),
    };
    let b = c.board.as_ref();
    b.create("Create claims", "", &[]).unwrap();
    let run = |v: &[&str]| {
        command(
            &c,
            v[0],
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    let item = [
        "add",
        "T",
        "--goal",
        "G",
        "--acceptance",
        "a",
        "--hold",
        "--op-id",
        "req.1.0",
    ];
    run(&item).unwrap();
    run(&item).unwrap();
    let issues: Vec<_> = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(service::is_held)
        .collect();
    assert_eq!(issues.len(), 1, "a repeated op id writes one issue");
    let n = issues[0].number;
    assert!(issues[0].body.ends_with("\nop-id: req.1.0\n"));
    // N-1: no `taskq` label, so 4.1.11 (same `is_task`) skips the issue; this version lists it as held
    assert_eq!(issues[0].labels, vec!["q-held"]);
    assert!(!service::is_task(&issues[0]) && service::is_held(&issues[0]));
    let (views, _) = service::views(b, &mut HashMap::new(), &mut HashMap::new(), 300).unwrap();
    assert_eq!(views[0].projection.task.state, State::Held);
    assert_eq!(fold(n, &entries(b, n).unwrap(), false).state, State::Held);
    run(&[
        "add",
        "U",
        "--goal",
        "G",
        "--acceptance",
        "a",
        "--op-id",
        "req.1.1",
    ])
    .unwrap();
    assert_eq!(
        b.issues(None).unwrap().len(),
        3,
        "each item has its own receipt, plus the claim issue"
    );
    run(&["start", &n.to_string(), "--op-id", "s1"]).unwrap();
    let i = b.issue(n).unwrap();
    assert!(
        service::is_task(&i)
            && i.labels.contains(&"q-ready".to_string())
            && !i.labels.contains(&"q-held".to_string()),
        "{:?}",
        i.labels
    );
    let log = entries(b, n).unwrap();
    assert_eq!(fold(n, &log, false).state, State::Ready);
    // N-1 reads `hold` and `start` as unknown kinds: the same `ready`
    let old: Vec<Entry> = log
        .iter()
        .filter(|e| !e.body.starts_with("taskq: hold") && !e.body.starts_with("taskq: start"))
        .cloned()
        .collect();
    assert_eq!(fold(n, &old, false), fold(n, &log, false));
    run(&["start", &n.to_string(), "--op-id", "s1"]).unwrap();
    assert_eq!(
        entries(b, n).unwrap().len(),
        log.len(),
        "a retried start writes nothing"
    );
    assert!(
        run(&["start", &n.to_string()]).is_err(),
        "a started task refuses another start"
    );
    // § 1 multi start: every number is checked before any write, then started in order
    let held: Vec<u64> = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(service::is_held)
        .map(|i| i.number)
        .collect();
    assert_eq!(held.len(), 1, "add without --hold is held too");
    let u = held[0];
    let before = entries(b, u).unwrap().len();
    assert!(
        run(&["start", &u.to_string(), &n.to_string()])
            .is_err_and(|e| e.contains(&format!("#{n}")))
    );
    assert_eq!(
        entries(b, u).unwrap().len(),
        before,
        "a bad number in the list writes nothing"
    );
    // #2288: a held body naming an open task without `Depends on` gets the deps hint; one with `--deps` does not
    run(&[
        "add",
        "V",
        "--goal",
        &format!("Runs after #{u}"),
        "--acceptance",
        "a",
    ])
    .unwrap();
    let v = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(service::is_held)
        .map(|i| i.number)
        .max()
        .unwrap();
    run(&[
        "add",
        "W",
        "--goal",
        &format!("Runs after #{u}"),
        "--acceptance",
        "a",
        "--deps",
        &u.to_string(),
    ])
    .unwrap();
    let w = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(service::is_held)
        .map(|i| i.number)
        .max()
        .unwrap();
    let snap = snapshot::read(b, &c.cfg, &c.raw, &[]);
    let text = snap.text(&c.cfg, &c.raw).unwrap();
    assert!(
        text.contains(&format!("| [#{u} U]("))
            && text.contains(&format!("| PM | `taskq start {u}` |"))
            && text.contains(&format!("[#{v} V]")),
        "{text}"
    );
    assert!(text.contains(&format!("| PM: names open #{u} without Depends on | add `Depends on #{u}`, then `taskq start {v}` |")), "{text}");
    assert!(
        text.contains(&format!("| PM | `taskq start {w}` |")),
        "{text}"
    );
    assert_eq!(
        snap.tasks.iter().find(|r| r.n == v).unwrap().json()["unlinked"],
        json!([u])
    );
    // #2288: a worker park on a held local blocker starts it; a started task or another repo's issue is not one
    assert_eq!(held_blocker(b, &format!("#{v}")), Some(v));
    assert_eq!(
        (
            held_blocker(b, &format!("#{n}")),
            held_blocker(b, "dashq#5"),
            held_blocker(b, "#999")
        ),
        (None, None, None)
    );
    b.close(w).unwrap();
    let receipts = start_tasks(
        b,
        &c.cfg.root,
        &[u.to_string(), format!("#{v}")],
        &Op::default(),
    )
    .unwrap();
    assert_eq!(
        receipts.iter().map(|r| r.task).collect::<Vec<_>>(),
        vec![u, v]
    );
    assert!(
        [u, v]
            .iter()
            .all(|&m| fold(m, &entries(b, m).unwrap(), false).state == State::Ready)
    );
}

// Chain: #2310 § 7 gate outside recipes. Risk: a task whose Acceptance forbids publishing closes (publishes)
// without owner approval, or a started task is gated after work began.
#[test]
fn add_gate_and_gate_n_require_owner_approval_before_close() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let b = c.board.as_ref();
    let run = |v: &[&str]| {
        command(
            &c,
            v[0],
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    run(&[
        "add",
        "Spec only",
        "--goal",
        "Decide",
        "--acceptance",
        "no main before owner check",
        "--gate",
    ])
    .unwrap();
    run(&[
        "add",
        "Later gated",
        "--goal",
        "Decide",
        "--acceptance",
        "a",
    ])
    .unwrap();
    let n: Vec<_> = b.issues(None).unwrap().iter().map(|i| i.number).collect();
    for &m in &n[..1] {
        assert!(
            b.issue(m)
                .unwrap()
                .labels
                .contains(&"needs-gate".to_string())
        );
        assert_eq!(
            fold(m, &entries(b, m).unwrap(), false).gate,
            Some(core::Gate::Required)
        );
    }
    assert_eq!(fold(n[1], &entries(b, n[1]).unwrap(), false).gate, None);
    run(&["gate", &n[1].to_string()]).unwrap();
    assert_eq!(
        fold(n[1], &entries(b, n[1]).unwrap(), false).gate,
        Some(core::Gate::Required)
    );
    assert!(
        b.issue(n[1])
            .unwrap()
            .labels
            .contains(&"needs-gate".to_string())
    );
    start_tasks(b, &c.cfg.root, &[n[0].to_string()], &Op::default()).unwrap();
    b.comment(
        n[0],
        &entry("spawn", &["worker", "claude:u", "mac"], None, ""),
    )
    .unwrap();
    let a = fold(n[0], &entries(b, n[0]).unwrap(), false)
        .attempt
        .unwrap();
    b.comment(
        n[0],
        &entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(a),
            "",
        ),
    )
    .unwrap();
    assert!(!close_one(b, &c.cfg, n[0], "ok", &Op::default()).ok() && b.issue(n[0]).unwrap().open);
}

// Chain: § 7 recipe run, held tasks, owner gate, close publication, dependants. Risk: a rerun duplicates tasks,
// a task starts before the whole recipe reads back, or a gated result publishes without approval of its SHA.
#[test]
fn a_recipe_holds_its_tasks_and_a_gate_blocks_close_until_the_owner_approves() {
    let raw = json!({"create_claim_issue": 1, "recipes": {"feature": {"version": 2, "tasks": [
        {"title": "Spec {goal}", "goal": "Decide {goal}", "acceptance": ["spec merged"], "gate": true},
        {"title": "Code {goal}", "goal": "Build {goal}", "acceptance": ["tests pass"], "deps": [1]}
    ]}}});
    let c = Ctx {
        cfg: cfg(),
        raw,
        board: Box::new(Fake::default()),
    };
    let b = c.board.as_ref();
    let run = |v: &[&str]| {
        command(
            &c,
            v[0],
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    b.create("create claims", "", &[]).unwrap();
    run(&["recipe", "feature", "--run", "r1", "--goal", "login"]).unwrap();
    let issues: Vec<_> = b
        .issues(None)
        .unwrap()
        .into_iter()
        .filter(|i| service::is_held(i))
        .collect();
    assert_eq!(issues.len(), 2);
    let (spec, code) = (issues[0].number, issues[1].number);
    assert_eq!(
        (issues[0].title.as_str(), issues[0].labels.clone()),
        (
            "Spec login",
            vec!["q-held".to_string(), "needs-gate".into()]
        )
    );
    assert!(issues[0].body.ends_with("\nop-id: recipe.feature.2.r1.1\n"));
    assert!(
        issues[1].body.contains(&format!("Depends on #{spec}\n")) && !service::is_task(&issues[1])
    );
    let t = fold(spec, &entries(b, spec).unwrap(), false);
    assert_eq!((t.state, t.gate), (State::Held, Some(core::Gate::Required)));
    // the rerun returns the first receipts, then starts both
    run(&[
        "recipe", "feature", "--run", "r1", "--goal", "login", "--start",
    ])
    .unwrap();
    assert_eq!(b.issues(None).unwrap().len(), 3, "a rerun adds nothing");
    assert_eq!(
        b.comments(1).unwrap().len(),
        2,
        "one immutable claim per recipe task"
    );
    assert_eq!(
        (
            current(b, spec).unwrap().1.state,
            current(b, code).unwrap().1.state
        ),
        (State::Ready, State::Waiting)
    );
    let sha = "0123456789abcdef0123456789abcdef01234567";
    b.comment(
        spec,
        &entry("spawn", &["worker", "claude:u", "mac"], None, ""),
    )
    .unwrap();
    let a = fold(spec, &entries(b, spec).unwrap(), false)
        .attempt
        .unwrap();
    b.comment(spec, &entry("result", &[sha], Some(a), ""))
        .unwrap();
    let r = close_one(b, &c.cfg, spec, "ok", &Op::default());
    assert!(
        !r.ok() && r.reason.contains("gate is required") && b.issue(spec).unwrap().open,
        "{}",
        r.text()
    );
    let approve = |sha: Option<&str>, agent: bool| {
        dispatch::approve(b, &c.cfg.root, spec, sha, &Op::default(), agent)
    };
    assert!(
        !approve(Some("89abcde"), false).ok(),
        "another SHA is not approved"
    );
    assert!(!approve(None, true).ok(), "an agent cannot approve");
    assert!(approve(Some(&sha[..7]), false).ok());
    assert_eq!(current(b, spec).unwrap().1.gate, Some(core::Gate::Approved));
    assert!(close_one(b, &c.cfg, spec, "ok", &Op::default()).ok());
    assert!(!b.issue(spec).unwrap().open);
    assert_eq!(
        current(b, code).unwrap().1.state,
        State::Ready,
        "the approved spec frees the coding task"
    );
}

// Chain: § 7 delivery preset -> owner approval of the exact SHA -> publication receipt. Risk: `publish: pr` turns
// into review-pr silently, a review-pr result publishes unapproved or on a stale approval, or the receipt omits the policy.
#[test]
fn review_pr_publishes_only_the_approved_sha_and_names_the_policy() {
    let preset = |v: Value| {
        delivery(&v)
            .map_err(String::from)
            .map(|(p, r)| format!("{} {p} {r}", p.mode()))
    };
    assert_eq!(preset(json!({})).unwrap(), "pr auto-merge 0");
    assert_eq!(
        preset(json!({"publish": "pr"})).unwrap(),
        "pr auto-merge 0",
        "no silent pr to review-pr"
    );
    assert_eq!(
        preset(json!({"publish": "direct"})).unwrap(),
        "direct direct 0"
    );
    assert_eq!(
        preset(json!({"delivery": {"preset": "review-pr", "revision": 3}})).unwrap(),
        "pr review-pr 3"
    );
    assert!(
        preset(json!({"delivery": {"preset": "local", "revision": 1}}))
            .unwrap_err()
            .contains("not available")
    );
    assert!(
        preset(json!({"delivery": {"preset": "review-pr"}}))
            .unwrap_err()
            .contains("revision")
    );
    assert!(
        preset(json!({"publish": "direct", "delivery": {"preset": "review-pr", "revision": 1}}))
            .unwrap_err()
            .contains("disagrees")
    );
    let c = Config {
        delivery: crate::config::Delivery::ReviewPr,
        delivery_revision: 3,
        ..cfg()
    };
    let b = Fake::default();
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    let (one, two) = (
        "0123456789abcdef0123456789abcdef01234567",
        "89abcdef0123456789abcdef0123456789abcdef",
    );
    let work = |sha: &str| {
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
            .unwrap();
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
        b.comment(n, &entry("result", &[sha], Some(a), "")).unwrap();
    };
    work(one);
    let r = close_one(&b, &c, n, "ok", &Op::default());
    assert!(
        !r.ok() && r.reason.contains("gate is required"),
        "{}",
        r.text()
    );
    assert!(dispatch::approve(&b, &c.root, n, Some(&one[..7]), &Op::default(), false).ok());
    // a new SHA invalidates the approval
    act(&b, &c.root, n, "requeue", &[], "redo", &Op::default());
    work(two);
    let r = close_one(&b, &c, n, "ok", &Op::default());
    assert!(
        !r.ok() && r.reason.contains("gate is stale") && b.issue(n).unwrap().open,
        "{}",
        r.text()
    );
    assert!(
        !dispatch::approve(&b, &c.root, n, Some(&one[..7]), &Op::default(), false).ok(),
        "the old SHA is not the candidate"
    );
    assert!(dispatch::approve(&b, &c.root, n, Some(&two[..7]), &Op::default(), false).ok());
    assert_eq!(
        close_one(&b, &c, n, "ok", &Op::default()).text(),
        format!("#{n} closed\ndelivery review-pr r3: merged {two}")
    );
}

#[test]
fn add_renders_the_task_body_and_rejects_over_limit_text() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let run = |v: &[&str]| {
        command(
            &c,
            "add",
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    c.board.create("dep", "", &[]).unwrap();
    run(&[
        "add",
        "T",
        "--goal",
        "G",
        "--acceptance",
        "a",
        "--acceptance",
        "b",
        "--scope",
        "s",
        "--deps",
        "1",
    ])
    .unwrap();
    let i = c.board.issues(None).unwrap().pop().unwrap();
    assert_eq!(
        i.body,
        "## Goal\nG\n\n## Acceptance\nA1. a\nA2. b\n\n## Scope\n- s\n\nDepends on #1\n"
    );
    let long = "x".repeat(237);
    assert!(
        run(&["add", "T", "--goal", "G", "--acceptance", &long])
            .unwrap_err()
            .contains("241 characters, limit 240")
    );
    assert!(
        run(&["add", &"t".repeat(81), "--goal", "G", "--acceptance", "a"])
            .unwrap_err()
            .contains("80")
    );
    assert!(
        run(&["add", "T", "--goal", "1\n2\n3", "--acceptance", "a"])
            .unwrap_err()
            .contains("Goal: 3 lines")
    );
    assert!(run(&["add", "T", "--goal", "G", "--acceptance", "a\nb"]).is_err());
    // #1696: a typed A<n>. label would render "A1. A1. ..."; a later mention is text
    assert!(
        run(&[
            "add",
            "T",
            "--goal",
            "G",
            "--acceptance",
            "a",
            "--acceptance",
            " A12. b"
        ])
        .unwrap_err()
        .contains("--acceptance 2: drop the A<n>. label")
    );
    run(&[
        "add",
        "T2",
        "--goal",
        "G",
        "--acceptance",
        "see A1. above",
        "--acceptance",
        "Abc. d",
    ])
    .unwrap();
    assert!(
        c.board
            .issues(None)
            .unwrap()
            .pop()
            .unwrap()
            .body
            .contains("A1. see A1. above\nA2. Abc. d\n")
    );
    let mut v = vec!["add", "T", "--goal", "G"];
    v.extend(["--acceptance", "a"].repeat(9));
    assert!(run(&v).unwrap_err().contains("split the task"));
    // three violations, one error (#1346)
    let title = "t".repeat(81);
    v[1] = &title;
    v[3] = "1\n2\n3";
    let e = run(&v).unwrap_err();
    assert!(
        e.contains("title: 81 characters, limit 80")
            && e.contains("Goal: 3 lines, allowed 1-2")
            && e.contains("Acceptance: 9 lines, allowed 1-8"),
        "{e}"
    );
    assert_eq!(
        c.board.issues(None).unwrap().len(),
        3,
        "rejected input writes nothing"
    );
}

// Chain: § 2 visual evidence (#1782). Defect: csgo#378 published image links the board could not render.
// Risk: an image that the board does not serve back is embedded, or a non-image file is uploaded.
#[test]
fn ask_images_embed_only_after_the_load_check() {
    let dir = std::env::temp_dir().join(format!("taskq-ask-images-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let png = dir.join("front view.png");
    fs::write(&png, b"\x89PNG\r\n\x1a\n0123456789").unwrap();
    let txt = dir.join("notes.png");
    fs::write(&txt, "not an image").unwrap();
    let b = Fake::default();
    let p = png.display().to_string();
    assert_eq!(
        upload_images(&b, 7, std::slice::from_ref(&p)).unwrap(),
        vec!["![front view](https://board.example/7/0-front-view.png)".to_string()]
    );
    assert!(
        upload_images(&b, 7, &[txt.display().to_string()])
            .unwrap_err()
            .contains("not a PNG")
    );
    assert!(
        upload_images(&b, 7, &vec![p.clone(); 5])
            .unwrap_err()
            .contains("at most four")
    );
    b.state.borrow_mut().bad_image = true;
    assert!(
        upload_images(&b, 7, &[p])
            .unwrap_err()
            .starts_with("image load check failed")
    );
    fs::remove_dir_all(&dir).unwrap();
}

// Chain: § 2 visual evidence videos (#1844). Defect: csgo#415's videos bypassed the load-checked upload (`--image`
// took images only). Risk: a video embeds as an image where the board cannot play it, a renamed or oversized file
// uploads, or visual-choice numbering breaks for a linked video.
#[test]
fn videos_upload_like_images_and_embed_by_board_player() {
    let dir = std::env::temp_dir().join(format!("taskq-ask-videos-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mp4 = dir.join("run.mp4");
    fs::write(&mp4, b"\0\0\0\x18ftypisom0123456789").unwrap();
    let webm = dir.join("clip.webm");
    fs::write(&webm, b"\x1a\x45\xdf\xa30123456789").unwrap();
    let renamed = dir.join("run.png");
    fs::write(&renamed, b"\0\0\0\x18ftypisom0123456789").unwrap();
    let big = dir.join("big.mp4");
    fs::write(
        &big,
        [b"\0\0\0\x18ftypisom".as_slice(), &vec![0; 10 << 20]].concat(),
    )
    .unwrap();
    let (m, w) = (mp4.display().to_string(), webm.display().to_string());
    let b = Fake::default();
    assert_eq!(
        upload_images(&b, 7, &[m.clone(), w.clone()]).unwrap(),
        [
            "[▶ run](https://board.example/7/0-run.mp4)",
            "[▶ clip](https://board.example/7/1-clip.webm)"
        ]
    );
    b.state.borrow_mut().video_inline = true;
    assert_eq!(
        upload_images(&b, 7, std::slice::from_ref(&m)).unwrap(),
        ["![run](https://board.example/7/2-run.mp4)"]
    );
    assert!(
        upload_images(&b, 7, &[renamed.display().to_string()])
            .unwrap_err()
            .contains("ends in .mp4")
    );
    assert!(
        upload_images(&b, 7, &[big.display().to_string()])
            .unwrap_err()
            .contains("at most 10 MB")
    );
    let opts = [
        "Left - + fast; - ugly".to_string(),
        "Right - + clean; - slow".into(),
    ];
    let links = [
        "[▶ a](https://x/a.mp4)".to_string(),
        "![b](https://x/b.png#upload=1)".into(),
    ];
    let card = ask_card(
        7,
        "Done: built\nProblem: which run",
        &opts,
        Some("1"),
        &links,
    )
    .unwrap();
    assert!(
        card.contains("Evidence: [▶ 1. a](https://x/a.mp4), ![2. b](https://x/b.png#upload=1)"),
        "{card}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

// Chain: § 2 evidence files (#2019). Defect: a worker had no way to hand in a zip, tar.gz, PDF or HTML file with
// its result. Risk: a file of another kind or over 25 MB uploads or writes an entry, a file links before the load
// check, files and links together exceed the § 2 cap of two links, or a result without a card refuses file links.
#[test]
fn result_and_ask_files_link_after_the_load_check_and_refuse_big_files_before_any_write() {
    let mut config = cfg();
    fs::create_dir(&config.root).unwrap();
    config.install = config.root.clone();
    let dir = config.root.clone();
    let file = |name: &str, bytes: &[u8]| {
        let p = dir.join(name);
        fs::write(&p, bytes).unwrap();
        p.display().to_string()
    };
    let zip = file("build (final).zip", b"PK\x03\x04 zip body");
    let tgz = file("logs.tar.gz", b"\x1f\x8b gzip body");
    let pdf = file("report.pdf", b"%PDF-1.7 body");
    let html = file("site.HTML", b"\xef\xbb\xbf\n  <!doctype html><p>x");
    let fake_pdf = file("fake.pdf", b"PK\x03\x04 a zip");
    let exe = file("tool.exe", b"MZ");
    let big = file(
        "big.zip",
        &[b"PK\x03\x04".as_slice(), &vec![0; 25 << 20]].concat(),
    );
    let exact = file(
        "exact.pdf",
        &[b"%PDF-".as_slice(), &vec![0; (25 << 20) - 5]].concat(),
    );
    assert_eq!(
        attachment_files(
            &[zip.clone(), tgz.clone(), pdf.clone(), html.clone(), exact],
            false
        )
        .unwrap(),
        [
            "⬇ build final.zip",
            "⬇ logs.tar.gz",
            "⬇ report.pdf",
            "⬇ site.HTML",
            "⬇ exact.pdf"
        ]
    );
    // a board that shows HTML as source gets the page zipped; other kinds keep their name, labels stay ≤ 32 characters
    let long = file("a very long page name for the release notes.htm", b"<p>x");
    assert_eq!(
        attachment_files(&[zip.clone(), html.clone(), long], true).unwrap(),
        [
            "⬇ build final.zip",
            "⬇ site.HTML.zip",
            "⬇ a very long page name.htm.zip"
        ]
    );
    assert!(
        attachment_files(&[fake_pdf], false)
            .unwrap_err()
            .contains("content is not a pdf file")
    );
    assert!(
        attachment_files(&[exe], false)
            .unwrap_err()
            .contains("not a .zip, .tar.gz, .tgz, .pdf, .html or .htm file")
    );
    let b = Fake::default();
    b.state.borrow_mut().bad_image = true;
    assert!(
        upload_attachments(&b, 7, std::slice::from_ref(&pdf))
            .unwrap_err()
            .starts_with("file load check failed")
    );
    let b = Fake::default();
    let n = b.create("Files", "", &["taskq".into()]).unwrap();
    b.comment(n, "taskq: spawn worker claude:fixture mac")
        .unwrap();
    let c = Ctx {
        cfg: config,
        raw: Value::Null,
        board: Box::new(b),
    };
    let run = |v: &[&str]| {
        command(
            &c,
            v[0],
            &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap(),
        )
    };
    let writes = || entries(c.board.as_ref(), n).unwrap().len();
    let before = writes();
    let (num, sha) = (n.to_string(), "a".repeat(40));
    let result = [
        "result",
        num.as_str(),
        "--sha",
        sha.as_str(),
        "--checks",
        "cargo test -- pass",
        "--text",
        "Changed: c\nOpen: none",
    ];
    let e = run(&[&result[..], &["--file", &big]].concat()).unwrap_err();
    assert!(e.contains("at most 25 MB"), "{e}");
    let e = run(&[
        &result[..],
        &["--file", &zip, "--file", &pdf, "--file", &tgz],
    ]
    .concat())
    .unwrap_err();
    assert!(e.contains("at most two --link and --file"), "{e}");
    assert_eq!(writes(), before, "a refused file writes no entry");
    let ask = [
        "ask",
        num.as_str(),
        "--text",
        "Done: built\nProblem: ship it?",
        "--option",
        "Ship - + done; - risk",
        "--option",
        "Hold - + safe; - slow",
        "--recommend",
        "1",
    ];
    let e = run(&[
        &ask[..],
        &[
            "--link",
            "https://example.org/1",
            "--file",
            &pdf,
            "--file",
            &tgz,
        ],
    ]
    .concat())
    .unwrap_err();
    assert!(e.contains("at most two --link and --file"), "{e}");
    assert!(
        run(&[
            &ask[..],
            &["--link", "[⬇ x.zip](https://example.org/x.zip)"]
        ]
        .concat())
        .unwrap_err()
        .contains("files through --file")
    );
    assert_eq!(writes(), before, "a refused file writes no entry");
    // the first upload has index 0: the refused files uploaded nothing
    let files = upload_attachments(c.board.as_ref(), n, &[zip, html.clone()]).unwrap();
    assert_eq!(
        files,
        [
            format!("[⬇ build final.zip](https://board.example/{n}/0-build--final-.zip)"),
            format!("[⬇ site.HTML](https://board.example/{n}/1-site.HTML)")
        ]
    );
    assert_eq!(
        c.board
            .file(&format!("https://board.example/{n}/1-site.HTML"))
            .unwrap(),
        fs::read(&html).unwrap()
    );
    // a result without a decision card takes file links; a plain --link still needs --option
    let body = result_text(
        n,
        "Changed: c\nOpen: none",
        &["cargo test -- pass".into()],
        &[],
        None,
        &files,
    )
    .unwrap();
    assert!(
        body.ends_with(&format!("\nEvidence: {}, {}", files[0], files[1])),
        "{body}"
    );
    assert!(
        result_text(
            n,
            "Changed: c\nOpen: none",
            &["t -- pass".into()],
            &[],
            None,
            &["https://example.org/1".into()]
        )
        .is_err()
    );
    let opts = [
        "Ship - + done; - risk".to_string(),
        "Hold - + safe; - slow".into(),
    ];
    let card = ask_card(
        n,
        "Done: built\nProblem: ship it?",
        &opts,
        Some("1"),
        &["https://example.org/1".into(), files[0].clone()],
    )
    .unwrap();
    assert!(
        card.contains(&format!(
            "\nEvidence: [#1](https://example.org/1), {}\n1. Ship",
            files[0]
        )),
        "{card}"
    );
    fs::remove_dir_all(&c.cfg.root).unwrap();
}

// Chain: § 2 visual evidence links (#1844). Defect: csgo#415 hand-posted host-less `/-/project/39/uploads/..mp4`
// links that GitLab rewrote into missing repository files. Risk: a relative, host-less or repository-path media link
// reaches a task, or an uploaded media URL or a plain link is refused.
#[test]
fn media_links_refuse_relative_host_less_and_repo_paths() {
    let bad = |v: &str| media_links(&[v]).unwrap_err();
    let e = bad("Changed: demo [run](/-/project/39/uploads/0f3a/run.mp4)");
    assert!(
        e.contains("/-/project/39/uploads/0f3a/run.mp4 is relative or host-less"),
        "{e}"
    );
    assert!(bad("see /-/project/39/uploads/0f3a/run.mp4.").contains("host-less"));
    assert!(bad("<video src=\"/uploads/0f3a/run.webm\">").contains("host-less"));
    assert!(bad("[shot](docs/shot.png)").contains("relative"));
    assert!(
        bad("[run](https://gitlab.example/alex/csgo/-/blob/main/run.mp4)")
            .contains("a repository path")
    );
    assert!(bad("[run](https://github.com/o/r/blob/main/run.mp4)").contains("a repository path"));
    assert!(bad("[run](https://github.com/o/r/raw/main/run.mp4)").contains("a repository path"));
    assert!(bad("https://raw.githubusercontent.com/o/r/main/a.gif").contains("a repository path"));
    let sha = "0123456789abcdef0123456789abcdef01234567";
    for ok in [
        "![run](https://gitlab.example/-/project/39/uploads/0f3a/run.mp4)".to_string(),
        "![b](https://gitlab.example/-/project/39/uploads/0f3a/b.png#upload=2)".into(),
        format!("[▶ run](https://cdn.jsdelivr.net/gh/o/r@{sha}/1844/run.mp4)"),
        format!("![a](https://github.com/o/r/raw/{sha}/1844/a.png)"),
        "Changed: fixed logo.png and src/main.rs; see [#12](https://github.com/o/r/issues/12)"
            .into(),
    ] {
        assert_eq!(media_links(&[&ok]), Ok(()), "{ok}");
    }
}

// Chain: § 2 Ask card. Risk: a card breaks the template or limits.
#[test]
fn ask_card_follows_section_2() {
    let card = ask_card(
        7,
        "Done: checked spec\nProblem: two ways\nboth slow",
        &[
            "Left - + fast; - ugly".into(),
            "Right - + clean; - slower".into(),
        ],
        Some("2"),
        &[
            "https://g/o/r/pull/12".into(),
            "[shot](https://x/y.png)".into(),
        ],
    )
    .unwrap();
    assert_eq!(
        card,
        "Done: checked spec\nProblem: two ways\nboth slow\nEvidence: [#12](https://g/o/r/pull/12), [shot](https://x/y.png)\n1. Left - + fast; - ugly\n2. Right - + clean; - slower ★\nTo answer: `taskq answer 7.K` or comment `K` or the option text."
    );
    let o = |n: usize| {
        (1..=n)
            .map(|i| format!("o{i} - + g; - c"))
            .collect::<Vec<_>>()
    };
    assert!(
        ask_card(7, "Problem: p", &o(2), Some("1"), &[])
            .unwrap_err()
            .contains("Done: 0 lines")
    );
    assert!(
        ask_card(7, "question?", &o(2), Some("1"), &[])
            .unwrap_err()
            .contains("starts with")
    );
    assert!(
        ask_card(7, "Done: d", &o(2), Some("1"), &[])
            .unwrap_err()
            .contains("Problem: 0 lines")
    );
    assert!(
        ask_card(7, "Problem: p\nDone: d", &o(2), Some("1"), &[])
            .unwrap_err()
            .contains("order")
    );
    assert!(
        ask_card(
            7,
            &format!("Done: {}", "d".repeat(155)),
            &o(2),
            Some("1"),
            &[]
        )
        .unwrap_err()
        .contains("161 characters, limit 160")
    );
    assert!(ask_card(7, "Done: d\nProblem: p", &o(1), Some("1"), &[]).is_err());
    assert!(ask_card(7, "Done: d\nProblem: p", &o(5), Some("1"), &[]).is_err());
    assert!(
        ask_card(7, "Done: d\nProblem: p", &o(2), None, &[])
            .unwrap_err()
            .contains("--recommend")
    );
    assert!(ask_card(7, "Done: d\nProblem: p", &o(2), Some("3"), &[]).is_err());
    assert!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &o(2),
            Some("1"),
            &vec!["u".to_string(); 3]
        )
        .is_err()
    );
    assert_eq!(
        ask_card(7, "Done: d\nProblem: add the token", &[], None, &[]).unwrap(),
        "Done: d\nProblem: add the token\nTo answer: `taskq answer 7 --text <done>`."
    );
    assert!(
        ask_card(7, "Done: d\nProblem: p", &[], Some("1"), &[]).is_err(),
        "--recommend needs options"
    );
    // #1457: one short labelled link per Problem/option line; the URL is not visible text; the error names the line
    let url = format!("https://github.com/o/r/issues/1443#{}", "x".repeat(200));
    assert!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &[
                format!(
                    "Merge later - + after [#1443]({url}) {}; - waits",
                    "w".repeat(100)
                ),
                "b - + g; - c".into()
            ],
            Some("1"),
            &[]
        )
        .is_ok()
    );
    assert_eq!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &["a - + g; - c".into(), format!("b - + see {url}; - c")],
            Some("1"),
            &[]
        )
        .unwrap_err(),
        "option line 2: bare URL; write [label](url)"
    );
    assert_eq!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &[
                "a - + [a](https://x); - [b](https://y)".into(),
                "b - + g; - c".into()
            ],
            Some("1"),
            &[]
        )
        .unwrap_err(),
        "option line 1: 2 links, at most one [label](url)"
    );
    assert_eq!(
        ask_card(
            7,
            &format!("Done: d\nProblem: p\n[{}](https://x)", "l".repeat(41)),
            &o(2),
            Some("1"),
            &[]
        )
        .unwrap_err(),
        "Problem line 2: link label at most 40 characters"
    );
    assert!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &[
                "a - + g; - c".into(),
                format!("b - + {}; - c", "c".repeat(147))
            ],
            Some("1"),
            &[]
        )
        .unwrap_err()
        .contains("option line 2: 161 characters, limit 160")
    );
    assert_eq!(
        width("see [#12](https://very/long/url) and `cargo test --locked` https://x/y"),
        15,
        "link targets and code are exempt; outside whitespace counts"
    );
    // #1787: an option names its meaning in a short unique label, then one plus and one minus; the error names the line
    let ok = |a: &str, b: &str| {
        ask_card(
            7,
            "Done: d\nProblem: p",
            &[a.into(), b.into()],
            Some("1"),
            &[],
        )
    };
    assert_eq!(
        ok("a - + g; - c", "- + g; - c").unwrap_err(),
        "option line 2: missing label; write `<label> - + <gain>; - <cost>`"
    );
    assert_eq!(
        ok("a - + g; - c", "b").unwrap_err(),
        "option line 2: detail must be `+ <gain>; - <cost>` after `b - `"
    );
    assert_eq!(
        ok("a - + g; - c", "b - only a gain").unwrap_err(),
        "option line 2: detail must be `+ <gain>; - <cost>` after `b - `"
    );
    assert_eq!(
        ok(&format!("{} - + g; - c", "l".repeat(33)), "b - + g; - c").unwrap_err(),
        "option line 1: label 33 characters, limit 32"
    );
    assert_eq!(
        ok("Keep - + g; - c", "keep - + h; - d").unwrap_err(),
        "option line 2: label repeats option line 1"
    );
    // A Russian card counts characters, not bytes: a 32-letter Cyrillic label fits, 33 do not
    let ru = ask_card(
        7,
        "Done: проверил оба пути\nProblem: выбрать способ сборки",
        &[
            "Оставить совместимость - + старые клиенты работают; - больше кода".into(),
            "Убрать старый API - + проще код; - клиенты сломаются".into(),
        ],
        Some("1"),
        &[],
    )
    .unwrap();
    assert!(ru.contains("1. Оставить совместимость - + старые клиенты работают; - больше кода ★\n2. Убрать старый API"), "{ru}");
    assert!(ok(&format!("{} - + да; - нет", "ж".repeat(32)), "b - + g; - c").is_ok());
    assert_eq!(
        ok(&format!("{} - + да; - нет", "ж".repeat(33)), "b - + g; - c").unwrap_err(),
        "option line 1: label 33 characters, limit 32"
    );
    // #1782: uploaded images embed inline after the links; a relative link would render broken on the board
    let img =
        "![front](https://gitlab.example/-/project/39/uploads/ab/front.png#upload=214)".to_string();
    assert_eq!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &o(2),
            Some("1"),
            &["[#1](https://x/1)".into(), img.clone()]
        )
        .unwrap()
        .lines()
        .nth(2),
        Some(format!("Evidence: [#1](https://x/1), {img}").as_str())
    );
    // #1787 visual choice: one image per option, image K named `K.` so every surface captions it by number
    let pair = [
        "![left](https://x/l.png)".to_string(),
        "![right](https://x/r.png)".to_string(),
    ];
    assert_eq!(
        ask_card(7, "Done: d\nProblem: p", &o(2), Some("1"), &pair)
            .unwrap()
            .lines()
            .nth(2),
        Some("Evidence: ![1. left](https://x/l.png), ![2. right](https://x/r.png)")
    );
    assert_eq!(
        ask_card(7, "Done: d\nProblem: p", &o(3), Some("1"), &pair)
            .unwrap()
            .lines()
            .nth(2),
        Some("Evidence: ![left](https://x/l.png), ![right](https://x/r.png)"),
        "fewer images than options: plain evidence"
    );
    assert_eq!(
        ask_card(
            7,
            "Done: d\nProblem: p",
            &o(2),
            Some("1"),
            &["[shot](/uploads/ab/front.png)".into()]
        )
        .unwrap_err(),
        "Evidence: [shot](/uploads/ab/front.png) needs an absolute http(s) URL"
    );
    assert_eq!(
        result_text(
            7,
            "Changed: c\nOpen: none",
            &["t -- ok".into()],
            &[],
            None,
            std::slice::from_ref(&img)
        )
        .unwrap(),
        format!("Changed: c\nChecks: t -- ok\nOpen: none\n\nEvidence: {img}")
    );
    assert!(
        result_text(
            7,
            "Changed: c\nOpen: none",
            &["t -- ok".into()],
            &[],
            None,
            &["https://x/1".into()]
        )
        .is_err(),
        "--link still needs --option"
    );
}

// H-A5: one registry owner covers preflight/effects/commit; a later remover reads the preceding writer's roots.
#[test]
fn registry_owner_preserves_parallel_changes_before_uninstall() {
    let base = std::env::temp_dir().join(format!("taskq-registry-owner-{}", std::process::id()));
    let install = base.join("install");
    let a = base.join("a");
    let b = base.join("b");
    let c = base.join("c");
    for path in [&install, &a, &b, &c] {
        fs::create_dir_all(path).unwrap();
    }
    let before = format!("{}\n{}\n", a.display(), b.display());
    fs::write(install.join("projects"), &before).unwrap();
    fs::write(install.join("fixture.unit"), "unit").unwrap();
    let held = coreq::file_lock::lock(&install.join("projects.lock")).unwrap();
    let run = || {
        coreq::process::output(
            "test.registry-owner",
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::registry_uninstall_fixture", "--ignored"])
                .env("TASKQ_REGISTRY_FIXTURE", &base),
            Duration::from_secs(5),
        )
        .unwrap()
    };
    run();
    let busy: Value = serde_json::from_slice(&fs::read(base.join("result.json")).unwrap()).unwrap();
    let unchanged = fs::read_to_string(install.join("projects")).unwrap() == before
        && install.join("fixture.unit").is_file();
    // This is the first owner's acknowledged change while the second one has been refused.
    fs::write(
        install.join("projects"),
        format!("{before}{}\n", c.display()),
    )
    .unwrap();
    drop(held);
    fs::write(install.join("fixture.unit"), "unit").unwrap();
    run();
    let done: Value = serde_json::from_slice(&fs::read(base.join("result.json")).unwrap()).unwrap();
    let after = fs::read_to_string(install.join("projects")).unwrap();
    fs::remove_dir_all(&base).unwrap();
    assert_eq!(
        busy["ok"], false,
        "uninstall crossed another registry owner's boundary: {busy}"
    );
    assert!(
        unchanged,
        "registry or unit changed while the preceding owner held projects.lock"
    );
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(
        after,
        format!("{}\n{}\n", b.display(), c.display()),
        "a stale snapshot lost the preceding writer's project"
    );
}

#[test]
#[ignore = "owned process for registry_owner_preserves_parallel_changes_before_uninstall"]
fn registry_uninstall_fixture() {
    let base = PathBuf::from(std::env::var_os("TASKQ_REGISTRY_FIXTURE").unwrap());
    let install = base.join("install");
    let result =
        host::ProjectEdit::new(&install, &base.join("a"), false).and_then(|edit| edit.commit());
    platform::write_atomic(
        &base.join("result.json"),
        &json!({"ok":result.is_ok(),"error":result.err()}).to_string(),
    )
    .unwrap();
}

// Chain: § 4 service uninstall. Risk: an unreadable registry loses the unit.
#[test]
fn unreadable_registry_preserves_unit_before_unload() {
    let root = std::env::temp_dir().join(format!("taskq-registry-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let projects = root.join("projects");
    let registered = b"other-project\n\xff";
    fs::write(&projects, registered).unwrap();
    let u = platform::Unit {
        path: root.join("unit"),
        text: String::new(),
        load: vec![],
        stop: vec![],
        unload: vec![vec!["taskq-must-not-run-unload".into()]],
    };
    fs::write(&u.path, "unit contents").unwrap();
    let error = host::ProjectEdit::new(&root, &root, false).err().unwrap();
    assert!(error.contains(&projects.display().to_string()));
    assert!(!error.contains("taskq-must-not-run-unload"));
    assert_eq!(fs::read_to_string(&u.path).unwrap(), "unit contents");
    assert_eq!(fs::read(&projects).unwrap(), registered);
    fs::remove_file(&projects).unwrap();
    assert!(
        host::ProjectEdit::new(&root, &root, false)
            .err()
            .unwrap()
            .contains(&projects.display().to_string())
    );
    assert!(u.path.is_file());
    assert!(!projects.exists());
    fs::remove_file(&u.path).unwrap();
    fs::remove_dir_all(&root).unwrap();
}

// Chain: § 7 close verdict. Risk: a padded verdict publishes.
#[test]
fn padded_close_verdict_is_rejected_before_publication() {
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let n = c
        .board
        .create("T", "A1. observable", &["taskq".into()])
        .unwrap();
    c.board
        .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let attempt = fold(n, &entries(c.board.as_ref(), n).unwrap(), false).attempt;
    let sha = "0123456789abcdef0123456789abcdef01234567";
    c.board
        .comment(n, &entry("result", &[sha], attempt, "result"))
        .unwrap();
    let before = c.board.comments(n).unwrap();
    for changed in [
        format!("{}Changed: c", " ".repeat(240)),
        format!("Changed: c{}", " ".repeat(240)),
    ] {
        let verdict = format!("{changed}\nChecks: A1 ok; CI green on {sha}\nOpen: none");
        let args = Args::parse(&["close".into(), n.to_string(), "--text".into(), verdict]).unwrap();
        assert!(command(&c, "close", &args).is_err());
        assert!(c.board.issue(n).unwrap().open);
        assert_eq!(c.board.comments(n).unwrap().len(), before.len());
        assert_eq!(
            fold(n, &entries(c.board.as_ref(), n).unwrap(), false).state,
            State::Review
        );
    }
}

// Chain: § 7 Result and close templates. Risk: result or close text breaks the template.
#[test]
fn result_and_close_follow_section_7() {
    let checks = ["cargo test --locked -- pass\ncargo run -- selftest -- pass".to_string()];
    assert_eq!(
        result_text(7, "Changed: faster\nOpen: none", &checks, &[], None, &[]).unwrap(),
        "Changed: faster\nChecks: cargo test --locked -- pass\nChecks: cargo run -- selftest -- pass\nOpen: none"
    );
    let r = result_text(
        7,
        "Changed: c\nOpen: none\nDone: shipped\nProblem: keep it?",
        &["cargo test --locked -- pass".into()],
        &[
            "close: ship - + done now; - no retry".into(),
            "Redo - + cleaner; - slower".into(),
        ],
        Some("1"),
        &[],
    )
    .unwrap();
    assert_eq!(
        r,
        "Changed: c\nChecks: cargo test --locked -- pass\nOpen: none\n\nDone: shipped\nProblem: keep it?\n1. close: ship - + done now; - no retry ★\n2. Redo - + cleaner; - slower\nTo answer: `taskq answer 7.K` or comment `K` or the option text."
    );
    assert!(
        result_text(
            7,
            "Changed: c\nOpen: none\nDone: d",
            &checks,
            &[],
            None,
            &[]
        )
        .unwrap_err()
        .contains("only with --option")
    );
    assert!(
        result_text(7, "Changed: c\nOpen: none", &[], &[], None, &[])
            .unwrap_err()
            .contains("Checks: 0 lines")
    );
    assert!(result_text(7, "summary", &checks, &[], None, &[]).is_err());
    let body = "## Acceptance\nA1. a\nA2. b\n";
    assert!(
        close_verdict(
            "Changed: c\nChecks: A1 ok, A2 gap; CI green on abc\nOpen: A2",
            body,
            false
        )
        .is_ok()
    );
    assert!(
        close_verdict(
            "Changed: c\nChecks: A1 ok; CI green\nOpen: none",
            body,
            false
        )
        .unwrap_err()
        .contains("A2")
    );
    assert!(
        close_verdict("Changed: c\nChecks: A1 ok, A2 ok\nOpen: none", body, false)
            .unwrap_err()
            .contains("CI")
    );
    assert!(
        close_verdict("A1 ok, A2 ok; c; open: none", body, false).is_err(),
        "the old one-line verdict is rejected"
    );
    assert_eq!(
        formats_selftest().unwrap(),
        "formats: ask card, result, close verdict PASS"
    );
}

// Chain: #1979 experiment close. Risk: an experiment closes with no verdict and its outcome is never decided.
#[test]
fn experiment_close_needs_a_verdict() {
    let body = "Hypothesis: x is faster\n## Acceptance\nA1. a\n";
    let plain = "Changed: c\nChecks: A1 ok; CI green on abc\nOpen: none";
    assert!(
        close_verdict(plain, body, true)
            .unwrap_err()
            .contains("Verdict"),
        "no verdict, no close"
    );
    assert!(
        close_verdict(plain, body, false).is_ok(),
        "other tasks close as before"
    );
    let good = "Changed: c\nVerdict: refuted p95 rose 12% in bench run 3\nChecks: A1 ok; CI green on abc\nOpen: none";
    assert!(close_verdict(good, body, true).is_ok());
    assert!(
        close_verdict(&good.replace("refuted", "maybe"), body, true).is_err(),
        "unknown verdict word"
    );
    assert!(
        close_verdict(&good.replace("refuted", "Confirmed."), body, false).is_ok(),
        "case and trailing punctuation"
    );
    assert!(reports::experiment(&board::Issue {
        body: body.into(),
        ..Default::default()
    }));
    assert!(reports::experiment(&board::Issue {
        labels: vec!["research".into()],
        ..Default::default()
    }));
    assert!(!reports::experiment(&board::Issue {
        body: "A1. no hypothesis here".into(),
        ..Default::default()
    }));
}

// Chain: § 2 card to hand-typed answer. Risk: a hand-typed answer to a rendered card is ignored.
#[test]
fn rendered_ask_card_resumes_the_worker_through_the_hand_typed_answer() {
    let b = Fake::default();
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let a = fold(n, &entries(&b, n).unwrap(), false).attempt;
    b.comment(
        n,
        &entry(
            "ask",
            &[],
            a,
            &ask_card(
                n,
                "Done: read spec\nProblem: which way",
                &[
                    "Left - + fast; - ugly".into(),
                    "Right - + clean; - slow".into(),
                ],
                Some("1"),
                &[],
            )
            .unwrap(),
        ),
    )
    .unwrap();
    let card = fold(n, &entries(&b, n).unwrap(), false).card.unwrap();
    assert!(
        card.ends_with(&format!(
            "To answer: `taskq answer {n}.K` or comment `K` or the option text."
        )),
        "{card}"
    );
    b.comment(n, "taskq answer 2").unwrap();
    let log = entries(&b, n).unwrap();
    assert_eq!(fold(n, &log, false).state, State::Doing);
    assert_eq!(
        core::delivery(n, &log, log.last().unwrap().id),
        "Right - + clean; - slow"
    );
}

// Chain: #1979 experiment close -> verdict label readback -> Decide. Risk: the board loses the verdict,
// a stale verdict label survives, or an unconfirmed label write still publishes.
#[test]
fn experiment_close_writes_one_verdict_label_before_publication() {
    let (b, c) = (Fake::default(), cfg());
    let n = b
        .create(
            "Invented experiment",
            "Hypothesis: a cache halves reads",
            &["taskq".into(), "code".into(), "verdict-confirmed".into()],
        )
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    let verdict = "Changed: c\nVerdict: refuted reads fell 3% only\nChecks: A1 ok; CI green on abc\nOpen: none";
    b.state.borrow_mut().fail_labels = true;
    let r = close_one(&b, &c, n, verdict, &Op::default());
    assert!(
        !r.ok() && b.issue(n).unwrap().open,
        "a failed label write refuses the close: {}",
        r.text()
    );
    assert!(close_one(&b, &c, n, verdict, &Op::default()).ok());
    let labels = b.issue(n).unwrap().labels;
    assert!(
        labels.contains(&"verdict-refuted".into()) && !labels.contains(&"verdict-confirmed".into()),
        "{labels:?}"
    );
    assert!(!b.issue(n).unwrap().open);
}

// Chain: § 5 commands to board entries. Risk: a rejected command writes; close does not publish.
// Chain: section 7 reviewer close saves one durable verdict; repeated close never closes before a merge.
#[test]
fn queued_close_records_intent_once_and_keeps_the_issue_open() {
    let (b, c) = (Fake::default(), cfg());
    let n = b
        .create("Invented queue task", "", &["taskq".into()])
        .unwrap();
    let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
    b.comment(
        n,
        &entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
    )
    .unwrap();
    b.state.borrow_mut().publish = "queue-admitted".into();
    let verdict = "Changed: reviewed\nChecks: A1 ok; merge-result CI pending\nOpen: none";
    assert!(close_one(&b, &c, n, verdict, &Op::default()).ok());
    assert!(close_one(&b, &c, n, "duplicate", &Op::default()).ok());
    let t = fold(n, &entries(&b, n).unwrap(), false);
    assert_eq!(t.publication.as_deref(), Some(verdict));
    assert_eq!(t.state, State::Review);
    assert!(b.issue(n).unwrap().open);
    assert_eq!(
        b.comments(n)
            .unwrap()
            .iter()
            .filter(|e| e.body.starts_with("taskq: publish"))
            .count(),
        1
    );
    b.state.borrow_mut().fail_publication_cancel = true;
    assert!(!act(&b, &c.root, n, "later", &[], "cancel", &Op::default()).ok());
    assert!(
        fold(n, &entries(&b, n).unwrap(), false)
            .publication
            .is_some()
    );
    b.state.borrow_mut().fail_publication_cancel = false;
    assert!(act(&b, &c.root, n, "later", &[], "cancel", &Op::default()).ok());
    assert_eq!(fold(n, &entries(&b, n).unwrap(), false).state, State::Later);
}

// Chain: §7 CLI close -> route-specific verdict -> publication. Adapter tests bypass the CLI validation.
// Risk: pending queue CI or a distinct merge SHA is rejected, or direct publication loses its exact-SHA check.
#[test]
fn cli_close_accepts_merge_result_ci_only_for_deferred_publication() {
    for ci in [
        "merge-result CI pending",
        "merge_group CI green on bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ] {
        for queued in [true, false] {
            let b = Fake::default();
            let n = b
                .create("Invented close", "A1. published", &["taskq".into()])
                .unwrap();
            let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
            b.comment(
                n,
                &entry("result", &[&"a".repeat(40)], Some(a), "candidate"),
            )
            .unwrap();
            b.state.borrow_mut().publish = if queued { "queue-admitted" } else { "" }.into();
            let c = Ctx {
                cfg: cfg(),
                raw: json!({}),
                board: Box::new(b),
            };
            let verdict = format!("Changed: reviewed\nChecks: A1 ok; {ci}\nOpen: none");
            let args =
                Args::parse(&["close".into(), n.to_string(), "--text".into(), verdict]).unwrap();
            assert_eq!(command(&c, "close", &args).is_ok(), queued);
            let t = fold(n, &entries(c.board.as_ref(), n).unwrap(), false);
            assert_eq!(t.publication.is_some(), queued);
            assert!(c.board.issue(n).unwrap().open);
            if !queued {
                let verdict = format!(
                    "Changed: reviewed\nChecks: A1 ok; CI green on {}\nOpen: none",
                    "a".repeat(40)
                );
                let args = Args::parse(&["close".into(), n.to_string(), "--text".into(), verdict])
                    .unwrap();
                command(&c, "close", &args).unwrap();
                assert!(!c.board.issue(n).unwrap().open);
            }
        }
    }
}

#[test]
fn commands_write_only_accepted_entries_and_close_publishes() {
    let (b, c) = (Fake::default(), cfg());
    let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
    let note = act(&b, &c.root, n, "steer", &[], "x", &Op::default());
    assert!(
        note.ok() && note.note == Some(true),
        "an owner steer saves context before work starts"
    );
    let log = entries(&b, n).unwrap();
    assert_eq!(
        fold(n, &log, false).state,
        State::Ready,
        "saving a note never starts work"
    );
    assert_eq!(
        core::task_context(n, "## Goal\ng", &log)
            .matches("Note ")
            .count(),
        1
    );
    let before = b.comments(n).unwrap().len();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
    b.comment(
        n,
        &entry(
            "result",
            &["0123456789abcdef0123456789abcdef01234567"],
            Some(a),
            "1. close: ship it ★\n2. redo the copy",
        ),
    )
    .unwrap();
    assert_eq!(b.comments(n).unwrap().len(), before + 2);
    b.state.borrow_mut().publish = "pending".into();
    assert!(
        close_one(&b, &c, n, "ok", &Op::default())
            .reason
            .contains("pending")
    );
    assert_eq!(
        fold(n, &entries(&b, n).unwrap(), false).state,
        State::Review,
        "pending CI changes nothing"
    );
    b.state.borrow_mut().publish = String::new();
    assert!(
        answer_picks(&b, &c, &[format!("{n}.3")], &Op::default()).is_err(),
        "picks are checked before writing"
    );
    assert_eq!(
        answer_picks(&b, &c, &[format!("{n}.1")], &Op::default())
            .unwrap()
            .iter()
            .map(Receipt::text)
            .collect::<Vec<_>>(),
        vec![format!(
            "#{n} closed\ndelivery auto-merge r0: merged 0123456789abcdef0123456789abcdef01234567"
        )]
    );
    let i = b.issue(n).unwrap();
    assert!(!i.open && !i.labels.iter().any(|l| l.starts_with("q-")));
}

// Chain: § 4 one service per root (#933). Risk: a second `service run` on one root drops the first one's children.
#[cfg(unix)]
#[test]
fn a_fresh_status_json_of_another_live_service_holds_service_run() {
    let root = std::env::temp_dir().join(format!("taskq-one-service-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(dot(&root)).unwrap();
    let st = dot(&root).join("status.json");
    assert_eq!(other_service(&root), None, "no status.json");
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    fs::write(&st, format!(r#"{{"pid": {}}}"#, other.id())).unwrap();
    assert_eq!(
        other_service(&root),
        Some(other.id()),
        "a live other service"
    );
    fs::write(&st, format!(r#"{{"pid": {}}}"#, std::process::id())).unwrap();
    assert_eq!(
        other_service(&root),
        None,
        "its own status.json after the update re-exec"
    );
    fs::write(&st, format!(r#"{{"pid": {}}}"#, other.id())).unwrap();
    other.kill().unwrap();
    other.wait().unwrap();
    assert_eq!(
        other_service(&root),
        None,
        "a dead writer, e.g. before a restart"
    );
    assert_eq!(service_id("abc").len(), 12);
    assert_ne!(service_id("abc"), "abc");
    let _ = fs::remove_dir_all(&root);
}

// Chain: native image replacement -> exact child/input recovery -> accepted input -> exit receipt.
// Risk: a pending acceptance loses its execution or an old turn accepts the new input.
#[cfg(unix)]
#[test]
fn native_image_handover_preserves_pending_input_and_terminal_receipts() {
    let root = std::env::temp_dir().join(format!("taskq-image-{}", runtime::uuid()));
    fs::create_dir_all(&root).unwrap();
    let result = coreq::process::output(
        "qualification.image-handover",
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::handover_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_PHASE", "before"),
        Duration::from_secs(20),
    );
    assert!(result.is_ok(), "image handover failed: {result:?}");
    assert_eq!(
        fs::read_to_string(root.join("verified")).unwrap(),
        "same process; exact execution; accepted input; native exit"
    );
    fs::remove_dir_all(root).unwrap();
}

// Chain: killed service -> cold restart with a live or already-exited native child -> exact receipt.
// Risk: restart loses ownership, invents no-start, or replays the pending input.
#[cfg(unix)]
#[test]
fn native_cold_restart_reconciles_the_original_execution() {
    let phases = if cfg!(any(target_os = "macos", target_os = "linux")) {
        &["live", "ended", "cancel"][..]
    } else {
        &["live", "ended"][..]
    };
    for &phase in phases {
        let root = std::env::temp_dir().join(format!("taskq-cold-{phase}-{}", runtime::uuid()));
        fs::create_dir_all(&root).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "tests::handover_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_COLD", phase)
            .stdin(std::process::Stdio::null());
        let mut old = command
            .env("TASKQ_IMAGE_PHASE", "before")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !root.join("ready").exists() {
            assert!(
                old.try_wait().unwrap().is_none(),
                "old service stopped before its checkpoint"
            );
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
        if phase == "ended" {
            fs::write(root.join("observe"), "").unwrap();
            fs::write(root.join("exit"), "").unwrap();
            while !root.join("native-exited").exists() {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        old.kill().unwrap();
        old.wait().unwrap();
        let output = coreq::process::output(
            "qualification.cold-restart",
            command
                .env("TASKQ_IMAGE_PHASE", "after")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped()),
            Duration::from_secs(8),
        );
        assert!(output.is_ok(), "{phase} cold recovery failed: {output:?}");
        assert_eq!(
            fs::read_to_string(root.join("verified")).unwrap(),
            "same process; exact execution; accepted input; native exit"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(unix)]
#[test]
#[ignore = "image handover fixture"]
fn handover_image_child() {
    use std::os::unix::process::CommandExt;
    let root = PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
    fs::create_dir_all(dot(&root)).unwrap();
    let b = Fake::default();
    let n = b.create("T", "", &[service::TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    b.comment(n, &format!("taskq: accepted {a} attempt {a}"))
        .unwrap();
    b.comment(n, &format!("taskq: released {a} exit attempt {a}"))
        .unwrap();
    let input = b.comment(n, "taskq: steer\ncontinue").unwrap();
    let execution = b
        .comment(n, &format!("taskq: executing {input} attempt {a}"))
        .unwrap();
    let c = Config {
        root: root.clone(),
        install: root.join("install"),
        ..cfg()
    };
    let mut s = Service::new(
        &b,
        &c,
        "fixture",
        Box::new(|_| panic!("handover must not submit new work")),
    );
    let log = runtime::attempt_log(&root, "worker", n, a);
    let cold = std::env::var("TASKQ_IMAGE_COLD").unwrap_or_default();
    if std::env::var("TASKQ_IMAGE_PHASE").unwrap() == "before" {
        let old = "{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n";
        fs::write(&log, old).unwrap();
        let output = fs::File::options().append(true).open(&log).unwrap();
        let sandbox = std::env::var_os("TASKQ_IMAGE_SANDBOX").is_some();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                if sandbox {
                    "tests::sandbox_helper_child"
                } else {
                    "tests::handover_native_child"
                },
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .stdin(std::process::Stdio::null())
            .stdout(output)
            .stderr(std::process::Stdio::null());
        // #1920: a host service spawns children on a project thread, which lives until the re-exec ends it.
        // #2071: through the private parent `runtime::executable` gives the launcher.
        #[cfg(target_os = "linux")]
        if sandbox {
            linux_sandbox::own(&mut command);
        }
        let child = if sandbox {
            let (sent, spawned) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                sent.send(command.spawn()).unwrap();
                loop {
                    std::thread::park();
                }
            });
            spawned.recv().unwrap().unwrap()
        } else {
            command.spawn().unwrap()
        };
        let native = coreq::process::identity(child.id()).unwrap();
        s.kids.push(Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution,
            task: n,
            attempt: a,
            rt: "codex".into(),
            proc: Box::new(runtime::CliTurn {
                proc: Box::new(child),
                log,
                pos: old.len() as u64,
                start: old.len() as u64,
                stopping: false,
                accepted: false,
                read_error: None,
                session: None,
                native,
                recovery: None,
                rt: "codex".into(),
            }),
        });
        let launch = runtime::Launch {
            files: String::new(),
            execution: None,
            task: n,
            title: String::new(),
            attempt: a,
            role: "worker".into(),
            rt: "codex".into(),
            session: "session".into(),
            url: String::new(),
            resume: Some("continue".into()),
        };
        s.kids[0].proc.checkpoint(&c, &launch, execution).unwrap();
        assert_eq!(
            kids_file(&c.install, &root)
                .unwrap()
                .first()
                .map(|k| k.execution),
            Some(execution),
            "successful launch must retain its exact execution before any image handover"
        );
        if !cold.is_empty() {
            fs::write(root.join("ready"), "").unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            if cold == "ended" {
                while s.kids[0].proc.exited().unwrap().is_none() {
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(10));
                }
                fs::write(root.join("native-exited"), "").unwrap();
            }
            loop {
                assert!(Instant::now() < until, "fixture service was not killed");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        save_kids(&s, &root).unwrap();
        coreq::telemetry::capture::before_exec();
        let error = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::handover_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_PHASE", "after")
            .exec();
        panic!("exec failed: {error}");
    }
    let checkpoint = fs::read_to_string(kids_path(&c.install, &root).unwrap()).unwrap();
    adopt(&mut s, &c.install, &root).unwrap();
    adopt(&mut s, &c.install, &root).unwrap();
    assert_eq!(
        s.kids.len(),
        1,
        "retry after partial recovery retains the same native handle"
    );
    if cold != "ended" {
        assert_eq!(
            s.kids[0].proc.exited(),
            Ok(None),
            "the re-exec killed the live child"
        );
    }
    assert_eq!(s.kids[0].execution, execution);
    assert_eq!(s.delivered(a), a);
    let pid = s.kids[0].proc.id();
    if cold == "live" && !cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(
            s.kids[0]
                .proc
                .kill()
                .unwrap_err()
                .contains("recovered process"),
            "non-child stop needs a stable native control handle"
        );
        assert!(
            coreq::process::identity(pid).unwrap().is_some(),
            "failed stop cannot signal a reused PID"
        );
    }
    s.reap_owned();
    if cold != "ended" {
        assert_eq!(
            s.delivered(a),
            a,
            "old log entries cannot accept this input"
        );
    }
    fs::write(root.join("observe"), "").unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while s.delivered(a) != input {
        s.reap_owned();
        assert!(
            Instant::now() < until,
            "acceptance not observed: {:?}",
            s.kids.iter().map(|k| &k.unknown).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    if cold == "cancel" {
        s.kids[0].proc.kill().unwrap();
    } else {
        fs::write(root.join("exit"), "").unwrap();
    }
    if !s.kids.is_empty() {
        while !s.kids[0].proc.exited().is_ok_and(|code| code.is_some()) {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(10));
        }
        let saved = fs::read_to_string(kids_path(&c.install, &root).unwrap()).unwrap();
        b.state.borrow_mut().fail_comments = 1;
        s.reap_owned();
        assert!(
            !s.kids.is_empty(),
            "failed release readback retains the terminal handle"
        );
        assert_eq!(
            fs::read_to_string(kids_path(&c.install, &root).unwrap()).unwrap(),
            saved,
            "recovery record survives a failed terminal receipt"
        );
    }
    while !s.kids.is_empty() {
        s.reap_owned();
        assert!(
            Instant::now() < until,
            "native exit not observed: {:?}",
            s.kids.iter().map(|k| &k.unknown).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(coreq::process::identity(pid).unwrap(), None);
    let entries = board::entries(&b, n).unwrap();
    assert!(
        !entries.iter().any(|e| e.body.starts_with("taskq: gone")),
        "a handed-over child never gets gone"
    );
    let records = core::executions(n, &entries);
    assert!(
        records
            .iter()
            .find(|x| x.id == execution)
            .is_some_and(|x| x.accepted && x.release == Some(core::Release::Exit))
    );
    if !cold.is_empty() {
        // Crash after the board receipt but before local cleanup: the stale identity must not block startup.
        fs::write(kids_path(&c.install, &root).unwrap(), checkpoint).unwrap();
        adopt(&mut s, &c.install, &root).unwrap();
        assert!(s.kids.is_empty());
    }
    save_kids(&s, &root).unwrap();
    assert!(kids_file(&c.install, &root).unwrap().is_empty());
    fs::write(
        root.join("verified"),
        "same process; exact execution; accepted input; native exit",
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "native process for image handover fixture"]
fn handover_native_child() {
    use std::io::Write;
    let root = PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
    // Stand-in for bwrap `--die-with-parent`: SIGKILL once the thread that forked this process ends.
    #[cfg(target_os = "linux")]
    if std::env::var_os("TASKQ_IMAGE_PDEATHSIG").is_some() {
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) },
            0
        );
    }
    let until = Instant::now() + Duration::from_secs(10);
    while !root.join("observe").exists() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("\n{{\"type\":\"turn.started\"}}");
    std::io::stdout().flush().unwrap();
    while !root.join("exit").exists() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Linux sandbox launcher for image handover fixture"]
fn sandbox_helper_child() {
    let mut bwrap = std::process::Command::new(std::env::current_exe().unwrap());
    bwrap
        .args([
            "--exact",
            "tests::handover_native_child",
            "--ignored",
            "--nocapture",
        ])
        .env("TASKQ_IMAGE_PDEATHSIG", "1");
    // A root-owned helper copy older than #1934 execs bwrap; self-updates never refresh it (#2071).
    if std::env::var_os("TASKQ_IMAGE_STALE").is_some() {
        panic!(
            "exec failed: {}",
            std::os::unix::process::CommandExt::exec(&mut bwrap)
        );
    }
    crate::platform::exit(linux_sandbox::supervise(bwrap).unwrap());
}

// Chain: Linux sandboxed Codex worker -> host re-exec from another thread -> adopted live child, exit receipt (#1920).
// Risk: bwrap `--die-with-parent` kills the worker when the re-exec ends the project thread that launched it.
#[cfg(target_os = "linux")]
#[test]
fn sandboxed_child_survives_the_reexec() {
    let root = std::env::temp_dir().join(format!("taskq-image-sandbox-{}", runtime::uuid()));
    fs::create_dir_all(&root).unwrap();
    let result = coreq::process::output(
        "qualification.image-handover",
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::handover_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_SANDBOX", "1")
            .env("TASKQ_IMAGE_PHASE", "before"),
        Duration::from_secs(20),
    );
    assert!(
        result.is_ok(),
        "sandboxed image handover failed: {result:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("verified")).unwrap(),
        "same process; exact execution; accepted input; native exit"
    );
    fs::remove_dir_all(root).unwrap();
}

// Chain: Linux service update with a helper copy older than #1934 -> launcher execs bwrap -> host re-exec ->
// adopted live child, exit receipt (#2071, pds18 4.6.55: live pilotq workers recorded gone, exit -1).
// Risk: the host relies on helper code that self-updates never refresh, so the re-exec kills the worker.
#[cfg(target_os = "linux")]
#[test]
fn stale_sandbox_helper_child_survives_the_reexec() {
    let root = std::env::temp_dir().join(format!("taskq-image-stale-{}", runtime::uuid()));
    fs::create_dir_all(&root).unwrap();
    let result = coreq::process::output(
        "qualification.image-handover",
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::handover_image_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_IMAGE_ROOT", &root)
            .env("TASKQ_IMAGE_SANDBOX", "1")
            .env("TASKQ_IMAGE_STALE", "1")
            .env("TASKQ_IMAGE_PHASE", "before"),
        Duration::from_secs(20),
    );
    assert!(
        result.is_ok(),
        "stale-helper image handover failed: {result:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("verified")).unwrap(),
        "same process; exact execution; accepted input; native exit"
    );
    fs::remove_dir_all(root).unwrap();
}

// § 8 re-exec preserves native CLI and Claude identity and defers for app ownership; no unit reload path remains.
#[cfg(unix)]
#[test]
fn live_child_survives_reexec_preparation_and_native_adoption() {
    use runtime::Proc;
    let root = std::env::temp_dir().join(format!("taskq-install-live-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(dot(&root)).unwrap();
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut adopted = runtime::Adopted::new(coreq::process::identity(pid).unwrap().unwrap());
    let mut wrong = coreq::process::identity(pid).unwrap().unwrap();
    wrong.started.push_str("-wrong");
    let mut wrong = runtime::Adopted::new(wrong);
    assert_eq!(
        wrong.exited(),
        Ok(Some(-1)),
        "a different native incarnation proves the recorded one is gone"
    );
    wrong.kill().unwrap(); // Terminal identity is a no-op; the current incarnation must remain untouched.
    assert_eq!(adopted.exited(), Ok(None), "the child still runs");
    // the re-exec writes the kids file; the new image adopts the child, not `gone`
    let owner = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let native = coreq::process::identity(pid).unwrap().unwrap();
    let path = kids_path(&root, &root).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "pid {} {} executor-v1\n7 41 codex {pid} {} 43 40\n",
            owner.pid, owner.started, native.started
        ),
    )
    .unwrap();
    assert_eq!(
        kids_file(&root, &root).unwrap(),
        vec![data::KidLine {
            task: 7,
            attempt: 41,
            runtime: "codex".into(),
            process: native,
            execution: 43,
            offset: 40,
            job: None
        }]
    );
    assert_eq!(adopted.exited(), Ok(None), "adopted live");
    // A real process stands in for an app turn: restart must stay pending and keep its lease.
    let b = Fake::default();
    let c = Config {
        root: root.clone(),
        ..cfg()
    };
    let mut s = Service::new(&b, &c, "test", Box::new(|_| unreachable!()));
    s.kids.push(Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: 41,
        task: 7,
        attempt: 41,
        rt: "codex-app".into(),
        proc: Box::new(runtime::Adopted::new(
            coreq::process::identity(pid).unwrap().unwrap(),
        )),
    });
    reexec(&s, &c).unwrap();
    assert!(dot(&root).join("restart").exists());
    assert_eq!(
        lifecycle_waits(&s, false),
        ["#7", "ownership unknown"],
        "status names the active task and missing ownership read"
    );
    assert_eq!(s.kids[0].attempt, 41);
    assert_eq!(adopted.exited(), Ok(None));
    s.kids.clear();
    coreq::process::terminate(&coreq::process::identity(pid).unwrap().unwrap()).unwrap();
    child.wait().unwrap();
    let _ = fs::remove_dir_all(&root);
}

// #1541: an adopted child that exits before the first poll is a zombie whose identity read fails on macOS.
// Risk: it is never reaped and every later handover pass fails in save_kids.
#[cfg(unix)]
#[test]
fn adopted_zombie_child_is_reaped_and_handover_completes() {
    let root = std::env::temp_dir().join(format!("taskq-zombie-{}", runtime::uuid()));
    fs::create_dir_all(dot(&root)).unwrap();
    let b = Fake::default();
    let n = b.create("T", "", &[service::TASK_LABEL.into()]).unwrap();
    let a = b
        .comment(n, "taskq: spawn worker codex:session mac")
        .unwrap();
    let c = Config {
        root: root.clone(),
        install: root.clone(),
        ..cfg()
    };
    let mut child = std::process::Command::new("sh")
        .args(["-c", "read x"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let pid = child.id();
    drop(child); // the re-exec keeps the pid, not the handle
    let owner = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let native = coreq::process::identity(pid).unwrap().unwrap();
    let path = kids_path(&root, &root).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            "pid {} {} executor-v1\n{n} {a} codex {pid} {} {a} 0\n",
            owner.pid, owner.started, native.started
        ),
    )
    .unwrap();
    fs::write(runtime::attempt_log(&root, "worker", n, a), "").unwrap();
    drop(stdin);
    let until = Instant::now() + Duration::from_secs(10);
    let zombie = || {
        // exited, not yet reaped
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            },
            0
        );
        #[cfg(target_os = "linux")]
        return unsafe { info.si_pid() } != 0;
        #[cfg(not(target_os = "linux"))]
        return info.si_pid != 0;
    };
    while !zombie() {
        assert!(Instant::now() < until, "child did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut s = Service::new(&b, &c, "test", Box::new(|_| unreachable!()));
    adopt(&mut s, &root, &root).unwrap();
    assert_eq!(s.kids.len(), 1);
    while !s.kids.is_empty() {
        s.reap_owned();
        assert!(
            Instant::now() < until,
            "zombie not reaped: {:?}",
            s.kids.iter().map(|k| &k.unknown).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        runtime::liveness(pid),
        Ok(false),
        "waitpid reaped the zombie"
    );
    assert!(
        core::executions(n, &board::entries(&b, n).unwrap())
            .iter()
            .any(|x| x.id == a && x.release == Some(core::Release::Exit))
    );
    save_kids(&s, &root).unwrap();
    assert!(
        kids_file(&root, &root).unwrap().is_empty(),
        "handover completes"
    );
    let _ = fs::remove_dir_all(&root);
}

// #2266 A2: one recovery row that cannot be adopted no longer ends the project loop.
// Risk: adopt(...)? stopped the project thread, so a dead execution beside it was never released and other hosts waited forever.
#[cfg(unix)]
#[test]
fn unadoptable_row_keeps_reservation_and_releases_dead_sibling() {
    let root = std::env::temp_dir().join(format!("taskq-adopt-row-{}", runtime::uuid()));
    fs::create_dir_all(dot(&root)).unwrap();
    let b = Fake::default();
    let (n, m) = (
        b.create("T", "", &[service::TASK_LABEL.into()]).unwrap(),
        b.create("U", "", &[service::TASK_LABEL.into()]).unwrap(),
    );
    let (kept, dead) = (
        b.comment(n, "taskq: spawn worker codex:a win").unwrap(),
        b.comment(m, "taskq: spawn worker codex:b mac").unwrap(),
    );
    let c = Config {
        root: root.clone(),
        install: root.clone(),
        ..cfg()
    };
    let ended = || {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let id = coreq::process::identity(child.id()).unwrap().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        id
    };
    let (win, mac) = (ended(), ended());
    let owner = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let path = kids_path(&root, &root).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("pid {} {} executor-v1\n{n} {kept} codex {} {} {kept} 0 job-kill:{}\n{m} {dead} codex {} {} {dead} 0\n",owner.pid,owner.started,win.pid,win.started,runtime::uuid(),mac.pid,mac.started)).unwrap();
    for (t, a) in [(n, kept), (m, dead)] {
        fs::write(runtime::attempt_log(&root, "worker", t, a), "").unwrap();
    }
    let mut s = Service::new(&b, &c, "test", Box::new(|_| unreachable!()));
    adopt(&mut s, &root, &root).unwrap();
    assert_eq!(s.kids.len(), 1);
    assert_eq!(s.kids[0].execution, dead);
    let until = Instant::now() + Duration::from_secs(10);
    while !s.kids.is_empty() {
        s.reap_owned();
        assert!(Instant::now() < until, "dead sibling not reaped");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        core::executions(m, &board::entries(&b, m).unwrap())[0].release,
        Some(core::Release::Exit)
    );
    assert_eq!(
        core::executions(n, &board::entries(&b, n).unwrap())[0].release,
        None,
        "unadoptable row stays reserved"
    );
    assert_eq!(
        kids_file(&root, &root)
            .unwrap()
            .iter()
            .map(|k| k.execution)
            .collect::<Vec<_>>(),
        vec![kept],
        "its native recovery record is kept"
    );
    let _ = fs::remove_dir_all(&root);
}

// #1594 A3: the recorded-process verdict reaches pre-boot retirement and the handover checkpoint unchanged.
// Risk: save_kids or pre_boot re-derive exit from identity and drift from snapshot::verdict (#1561, #1445).
#[cfg(unix)]
#[test]
fn recorded_process_verdict_through_pre_boot_and_handover() {
    let root = std::env::temp_dir().join(format!("taskq-verdict-{}", runtime::uuid()));
    fs::create_dir_all(dot(&root)).unwrap();
    let b = Fake::default();
    let (n, m) = (
        b.create("T", "", &[service::TASK_LABEL.into()]).unwrap(),
        b.create("U", "", &[service::TASK_LABEL.into()]).unwrap(),
    );
    let (live, ended) = (
        b.comment(n, "taskq: spawn worker codex:a mac").unwrap(),
        b.comment(m, "taskq: spawn worker codex:b mac").unwrap(),
    );
    let c = Config {
        root: root.clone(),
        install: root.clone(),
        ..cfg()
    };
    let spawn = || {
        std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap()
    };
    let kid = |task, attempt, mut child: std::process::Child, exit: bool| {
        let native = coreq::process::identity(child.id()).unwrap();
        if exit {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        Kid {
            unknown: None,
            stopping: false,
            session_url: None,
            execution: attempt,
            task,
            attempt,
            rt: "codex".into(),
            proc: Box::new(runtime::CliTurn {
                proc: Box::new(child),
                log: root.join("log"),
                pos: 0,
                start: 0,
                stopping: false,
                accepted: false,
                read_error: None,
                session: None,
                native,
                recovery: None,
                rt: "codex".into(),
            }),
        }
    };
    let mut s = Service::new(&b, &c, "test", Box::new(|_| unreachable!()));
    fs::write(root.join("log"), "").unwrap();
    s.kids = vec![kid(n, live, spawn(), false), kid(m, ended, spawn(), true)];
    let line = |k: &Kid| data::KidLine {
        task: k.task,
        attempt: k.attempt,
        runtime: "codex".into(),
        process: k.proc.native_identity().unwrap(),
        execution: k.execution,
        offset: 0,
        job: None,
    };
    let (a, e) = (line(&s.kids[0]), line(&s.kids[1]));
    assert_eq!(recorded(&a).0, "running");
    assert_eq!(
        recorded(&e),
        ("ended", "recorded native process exited".into())
    );
    assert!(
        !pre_boot(&a) && !pre_boot(&e),
        "a process of this boot is never retired as pre-boot"
    );
    save_kids(&s, &root).unwrap();
    assert_eq!(
        kids_file(&root, &root).unwrap(),
        vec![a.clone(), e],
        "#2297: the ended record passes to the next image until its board exit"
    );
    b.comment(m, &format!("taskq: released {ended} exit attempt {ended}"))
        .unwrap();
    save_kids(&s, &root).unwrap();
    assert_eq!(
        kids_file(&root, &root).unwrap(),
        vec![a],
        "the ended record resolves from its board exit; the live one is kept"
    );
    for k in &mut s.kids {
        let _ = runtime::stop(k.proc.as_mut());
    }
    let _ = fs::remove_dir_all(&root);
}

// § 8 recovery: corrupt/legacy identities stay unknown and preserved; no timestamp can discard them.
#[test]
fn native_recovery_preserves_corrupt_and_legacy_evidence() {
    let root = std::env::temp_dir().join(format!("taskq-handover-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(dot(&root)).unwrap();
    let st = dot(&root).join("status.json");
    let kids = kids_path(&root, &root).unwrap();
    fs::create_dir_all(kids.parent().unwrap()).unwrap();
    assert!(kids_file(&root, &root).unwrap().is_empty());
    let legacy = dot(&root).join("kids");
    fs::write(&legacy, "untrusted project recovery record").unwrap();
    assert!(kids_file(&root, &root).unwrap_err().contains("legacy"));
    assert_eq!(
        fs::read_to_string(&legacy).unwrap(),
        "untrusted project recovery record"
    );
    fs::write(&kids, "pid 1\n7 41 claude\n").unwrap();
    assert!(kids_file(&root, &root).is_err());
    assert_eq!(
        fs::read_to_string(&kids).unwrap(),
        "pid 1\n7 41 claude\n",
        "a malformed kids file stays"
    );
    let owner = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let launcher_only = format!(
        "pid {} {}\n7 41 codex 123 child 43 40\n",
        owner.pid, owner.started
    );
    fs::write(&kids, &launcher_only).unwrap();
    assert!(
        kids_file(&root, &root)
            .unwrap_err()
            .contains("legacy launcher")
    );
    assert_eq!(
        fs::read_to_string(&kids).unwrap(),
        launcher_only,
        "never upgrade old launcher identity implicitly"
    );
    let valid = format!(
        "pid {} {} executor-v1\n7 41 codex 123 child 43 40\n",
        owner.pid, owner.started
    );
    fs::write(&kids, &valid).unwrap();
    assert_eq!(
        kids_file(&root, &root).unwrap(),
        vec![data::KidLine {
            task: 7,
            attempt: 41,
            runtime: "codex".into(),
            process: coreq::process::Identity {
                pid: 123,
                started: "child".into()
            },
            execution: 43,
            offset: 40,
            job: None
        }]
    );
    assert_eq!(
        fs::read_to_string(&kids).unwrap(),
        valid,
        "readback never deletes recovery evidence"
    );
    assert!(
        legacy.exists(),
        "protected adoption does not delete the legacy artifact"
    );
    fs::write(
        &kids,
        format!("pid {} wrong-creation executor-v1\n", owner.pid),
    )
    .unwrap();
    assert!(
        kids_file(&root, &root).unwrap().is_empty(),
        "a different service incarnation permits reconciliation, never blind child control"
    );
    let mut foreign = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runtime::tests::stop_child", "--ignored"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let foreign_id = coreq::process::identity(foreign.id()).unwrap().unwrap();
    let foreign_record = format!(
        "pid {} {} executor-v1\n",
        foreign_id.pid, foreign_id.started
    );
    fs::write(&kids, &foreign_record).unwrap();
    assert!(kids_file(&root, &root).unwrap_err().contains("still alive"));
    foreign.kill().unwrap();
    foreign.wait().unwrap();
    assert!(kids_file(&root, &root).unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(&kids).unwrap(),
        foreign_record,
        "native reads never erase the prior owner's record"
    );
    fs::write(&kids, "pid 1\n7 41 claude 123 40\n").unwrap();
    assert!(
        kids_file(&root, &root).is_err() && kids.exists(),
        "previous service metadata survives for reconciliation"
    );
    // a corrupt focus pauses nothing away and names its reason
    fs::write(dot(&root).join("focus"), b"\xff").unwrap();
    assert!(pause(&root, false).unwrap_err().contains("not UTF-8"));
    assert_eq!(fs::read(dot(&root).join("focus")).unwrap(), b"\xff");
    assert!(service_fields(&root).contains("Focus: unknown (focus: not UTF-8 text; file kept)"));
    fs::write(&st, "{").unwrap();
    assert!(service_fields(&root).contains("Status: unknown (status.json: corrupt"));
    let _ = fs::remove_dir_all(&root);
}

// Chain: § 4 Focus default after install (#974). Risk: a fresh install admits all, or overwrites the owner's focus.
#[test]
fn install_writes_focus_off_only_when_missing() {
    let root = std::env::temp_dir().join(format!("taskq-install-focus-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    assert_eq!(install_focus(&root), Ok(true));
    assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "off");
    assert!(
        service_fields(&root).contains("Focus: off") && !service_fields(&root).contains("Paused")
    );
    // #1592 menu.md `Paused`: focus off with a saved focus prints the Paused line from the display model's mode
    fs::write(dot(&root).join("focus.paused"), "area-maps").unwrap();
    assert!(
        service_fields(&root).contains("Focus: off\nPaused"),
        "{}",
        service_fields(&root)
    );
    fs::write(dot(&root).join("focus"), "area-maps").unwrap();
    assert_eq!(install_focus(&root), Ok(false));
    assert_eq!(
        fs::read_to_string(dot(&root).join("focus")).unwrap(),
        "area-maps"
    );
    let _ = fs::remove_dir_all(&root);
}

// Chain: direct shared commands -> receipt refusal -> preserved focus, with safe Pause fallback.
#[test]
fn direct_focus_commands_cannot_bypass_required_receipts() {
    let root = std::env::temp_dir().join(format!("taskq-direct-receipt-{}", std::process::id()));
    fs::create_dir_all(dot(&root)).unwrap();
    fs::write(dot(&root).join("focus"), "off").unwrap();
    fs::write(dot(&root).join("focus.paused"), "area-maps").unwrap();
    assert!(resume(&root).unwrap_err().contains("receipt unavailable"));
    assert_eq!(data::focus(&root).unwrap(), Vec::<String>::new());
    assert!(
        set_focus(&root, "all", None)
            .unwrap_err()
            .contains("receipt unavailable")
    );
    assert_eq!(data::focus(&root).unwrap(), Vec::<String>::new());
    fs::write(dot(&root).join("focus"), "area-maps").unwrap();
    assert!(
        pause(&root, false)
            .unwrap_err()
            .contains("applied locally; durable receipt unknown")
    );
    assert_eq!(data::focus(&root).unwrap(), Vec::<String>::new());
    assert_eq!(
        fs::read_to_string(dot(&root).join("focus.paused")).unwrap(),
        "area-maps"
    );
    fs::remove_dir_all(root).unwrap();
}

// Chain: initialization refusal -> original error and durable command failure, before any setting write.
#[test]
fn focus_initialization_failures_keep_required_receipts() {
    crate::tests::with_test_capture(|store, dir| {
        let root = dir.join("project");
        fs::create_dir(&root).unwrap();
        fs::write(dot(&root), "preserve this obstacle").unwrap();
        assert!(
            set_focus(&root, "all", None)
                .unwrap_err()
                .contains("failure receipt")
        );
        assert!(pause(&root, true).unwrap_err().contains("failure receipt"));
        assert_eq!(
            fs::read_to_string(dot(&root)).unwrap(),
            "preserve this obstacle"
        );
        assert!(crate::telemetry::settle(Duration::from_secs(30)));
        let page = store.export(0, 0, 1024).unwrap();
        let receipts: Vec<_> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["observation"])
            .filter(|r| r["source"] == "command")
            .collect();
        assert_eq!(
            receipts
                .iter()
                .map(|r| r["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "command.received",
                "command.failed",
                "command.received",
                "command.failed"
            ]
        );
        assert_ne!(receipts[0]["span"], receipts[2]["span"]);
    });
}

// Chain: settings change -> stalled terminal commit -> command returns and frees the signal (#1351) -> receipts land later.
#[test]
fn focus_never_waits_on_a_stalled_terminal_receipt() {
    use std::sync::Arc;
    use tracing_subscriber::prelude::*;
    let dir = std::env::temp_dir().join(format!("taskq-commit-lock-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
    let capture =
        coreq::telemetry::capture::Capture::new(store.clone(), "taskq", "fixture").unwrap();
    // Only the action and required receipts enter this collector; its next accepted row is the terminal receipt.
    let layer = capture.clone().with_filter(
        tracing_subscriber::filter::Targets::new()
            .with_target("taskq::receipt_lock_fixture", tracing::Level::INFO),
    );
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
        let root = dir.join("project");
        fs::create_dir_all(dot(&root)).unwrap();
        fs::write(dot(&root).join("focus"), "off").unwrap();
        #[cfg(windows)]
        {
            let path = dir.join("raw-read");
            fs::write(&path, "signal").unwrap();
            let locked = coreq::file_lock::try_lock(&path).unwrap();
            assert!(
                fs::read_to_string(&path).is_err(),
                "raw reads bypassed the Windows byte-range lock"
            );
            drop(locked);
            assert_eq!(fs::read_to_string(path).unwrap(), "signal");
        }
        let db = rusqlite::Connection::open(dir.join("db")).unwrap();
        db.busy_timeout(Duration::from_secs(3)).unwrap();
        let _action =
            tracing::info_span!(target:"taskq::receipt_lock_fixture","fixture.focus").entered();
        let started = Instant::now();
        change_focus(&root, false, false, |d| {
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            platform::write_atomic(&d.join("focus"), "all")
        })
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "focus waited {:?} on a stalled store",
            started.elapsed()
        );
        drop(wake_lock(&root).expect("next command blocked behind a terminal receipt commit"));
        db.execute_batch("ROLLBACK").unwrap();
        assert!(crate::telemetry::settle(Duration::from_secs(30)));
        capture.flush().unwrap();
        let page = store.export(0, 0, 1024).unwrap();
        let receipts: Vec<_> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["observation"])
            .filter(|r| r["source"] == "command")
            .collect();
        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[1]["kind"], "command.applied");
        assert_eq!(receipts[0]["span"], receipts[1]["span"]);
        assert_eq!(data::focus(&root).unwrap(), ["all"]);
    });
    capture.flush().unwrap();
    drop(capture);
    drop(store);
    fs::remove_dir_all(dir).unwrap();
}

// Chain: § 4 pause and resume focus. Risk: resume widens focus to all.
#[test]
fn focus_commands_do_not_mutate_while_another_command_or_reader_owns_the_signal() {
    crate::tests::with_test_capture(|_, _| {
        let root = std::env::temp_dir().join(format!("taskq-focus-lock-{}", std::process::id()));
        fs::create_dir_all(dot(&root)).unwrap();
        let d = dot(&root);
        fs::write(d.join("focus"), "off").unwrap();
        fs::write(d.join("focus.paused"), "area-maps").unwrap();
        fs::write(d.join("focus.request"), "owner intent").unwrap();
        fs::write(d.join("poke"), ".").unwrap();
        for shared in [false, true] {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(d.join("poke"))
                .unwrap();
            let _owner = if shared {
                coreq::file_lock::try_lock_shared_file(file)
            } else {
                coreq::file_lock::try_lock_file(file)
            }
            .unwrap();
            assert!(resume(&root).is_err());
            assert_eq!(
                fs::read_to_string(d.join("focus")).unwrap(),
                "off",
                "Resume changed state before owning its command boundary"
            );
            assert!(set_focus(&root, "all", Some("new intent")).is_err());
            assert_eq!(
                fs::read_to_string(d.join("focus.request")).unwrap(),
                "owner intent"
            );
            assert_eq!(fs::read_to_string(d.join("focus")).unwrap(), "off");
            assert!(pause(&root, false).is_err());
            assert_eq!(
                fs::read_to_string(d.join("focus.paused")).unwrap(),
                "area-maps"
            );
            assert!(install_focus(&root).is_err());
        }
        assert_eq!(resume(&root).unwrap().0, "area-maps");
        pause(&root, false).unwrap();
        assert_eq!(resume(&root).unwrap().0, "area-maps");
        let context = format!("1 {}.1 {}.2", "a".repeat(32), "a".repeat(32));
        fs::write(d.join("poke"), &context).unwrap();
        let mut seen = None;
        let (link, focus) = wake_read(&d.join("poke"), &mut seen, || {
            assert!(
                pause(&root, false).is_err(),
                "writer entered a reader's snapshot"
            );
            data::focus(&root)
        })
        .unwrap();
        assert_eq!(link.unwrap().encode(), context);
        assert_eq!(focus, ["area-maps"]);
        seen = None;
        fs::write(d.join("focus"), b"\xff").unwrap();
        assert!(wake_read(&d.join("poke"), &mut seen, || data::focus(&root)).is_err());
        assert!(
            seen.is_none(),
            "failed state read consumed the command identity"
        );
        fs::write(d.join("focus"), "area-maps").unwrap();
        assert!(
            wake_read(&d.join("poke"), &mut seen, || data::focus(&root))
                .unwrap()
                .0
                .is_some()
        );
        fs::remove_file(d.join("focus.request")).unwrap();
        fs::create_dir(d.join("focus.request")).unwrap();
        assert!(
            set_focus(&root, "area-other", None)
                .unwrap_err()
                .contains("partially saved")
        );
        assert_eq!(data::focus(&root).unwrap(), ["area-other"]);
        let signal = fs::read_to_string(d.join("poke")).unwrap();
        assert_ne!(
            signal, context,
            "partial state retained the previous command's link"
        );
        assert!(coreq::telemetry::capture::TraceContext::decode(&signal).is_ok());
        fs::remove_dir_all(root).unwrap();
    });
}

// Chain: repeated Pause -> Resume preserves the owner's saved scope.
#[test]
fn resume_restores_the_saved_focus_and_never_defaults_to_all() {
    crate::tests::with_test_capture(|_, _| {
        let root = std::env::temp_dir().join(format!("taskq-resume-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(dot(&root)).unwrap();
        fs::write(
            dot(&root).join("status.json"),
            r#"{"tasks": [{"n": 1, "labels": ["taskq", "q-ready", "area-maps"]}]}"#,
        )
        .unwrap();
        fs::write(dot(&root).join("focus"), "off").unwrap();
        let e = resume(&root).unwrap_err();
        assert!(e.contains("labels of open tasks: area-maps"), "{e}");
        assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "off");
        fs::write(dot(&root).join("focus"), "area-maps").unwrap();
        pause(&root, false).unwrap();
        pause(&root, false).unwrap();
        assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "off");
        assert_eq!(
            resume(&root).unwrap().0,
            "area-maps",
            "a second pause keeps the saved focus"
        );
        let _ = fs::remove_dir_all(&root);
    });
}

// Chain: R6 human report. Risk: report breaks the R6 format.
#[test]
fn report_follows_r6() {
    let (b, c) = (Fake::default(), cfg());
    let n = b.create("Ship | it", "", &["taskq".into()]).unwrap();
    let q = b.create("Q", "", &["taskq".into()]).unwrap();
    b.comment(
        q,
        &entry("spawn", &["worker", "claude:abc", "mac"], None, ""),
    )
    .unwrap();
    let a = fold(q, &entries(&b, q).unwrap(), false).attempt.unwrap();
    b.comment(
        q,
        &entry(
            "ask",
            &["owner"],
            Some(a),
            "Which way?\nBecause.\n1. left\n2. right ★",
        ),
    )
    .unwrap();
    b.create(
        "taskq host mac",
        "version 4.0.0",
        &[service::HOST_LABEL.into()],
    )
    .unwrap();
    let r = report(&b, &c, &Value::Null, &[]).unwrap();
    assert!(
        r.contains("In work 0 · Waiting for answer 1 · Ready 1"),
        "{r}"
    );
    assert!(
        r.contains(&format!(
            "| [#{n} Ship \\| it](https://board/{n}) | no host decision |"
        )),
        "{r}"
    );
    assert!(
        r.contains(&format!("{q}.1 left · {q}.2 right ★ |")),
        "only the star without a pick: {r}"
    );
    assert!(r.contains("Hosts: mac v4.0.0"), "{r}");
    assert!(!inbox(&b, &c.root, q).unwrap().contains("Recommendation:"));
    // § 2: the PM pick and its reason sit beside the worker's star in status, status --json and inbox
    b.comment(q, &entry("pick", &["1"], None, "left | is cheaper"))
        .unwrap();
    let r = report(&b, &c, &Value::Null, &[]).unwrap();
    assert!(
        r.contains(&format!(
            "{q}.1 left · {q}.2 right ★ · PM {q}.1: left \\| is cheaper |"
        )),
        "{r}"
    );
    let rows = snapshot::read(&b, &c, &Value::Null, &[]).json()["tasks"].clone();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["n"] == q)
        .unwrap()
        .clone();
    assert_eq!(row["pick"], json!({"id": 1, "reason": "left | is cheaper"}));
    assert!(inbox(&b, &c.root, q).unwrap().contains(&format!(
        "Recommendation: worker ★ {q}.2 · PM {q}.1: left | is cheaper"
    )));
}

/// A board with one task per report part: ready, ask with a card, blocked, failed, plus a host heartbeat.
fn snapshot_board() -> Fake {
    let b = Fake::default();
    b.create("Ready one", "", &["taskq".into(), "p1".into()])
        .unwrap();
    let q = b.create("Question", "", &["taskq".into()]).unwrap();
    b.comment(
        q,
        &entry("spawn", &["worker", "claude:abc", "mac"], None, ""),
    )
    .unwrap();
    let a = fold(q, &entries(&b, q).unwrap(), false).attempt.unwrap();
    b.comment(
        q,
        &entry(
            "ask",
            &[],
            Some(a),
            "Done: read\nProblem: Which way?\n1. left\n2. right ★",
        ),
    )
    .unwrap();
    let dep = b.create("plain issue", "", &[]).unwrap();
    b.create("Blocked", &format!("Depends on #{dep}"), &["taskq".into()])
        .unwrap();
    let f = b.create("Broken", "", &["taskq".into()]).unwrap();
    b.comment(f, &entry("failed", &[], None, "Reason: no login"))
        .unwrap();
    b.create(
        "taskq host mac",
        "version 4.0.0",
        &[service::HOST_LABEL.into()],
    )
    .unwrap();
    // #2014: a review row carries the typed § 7 verdict of its result
    let r = b.create("Reviewed", "", &["taskq".into()]).unwrap();
    b.comment(
        r,
        &entry("spawn", &["worker", "claude:def", "mac"], None, ""),
    )
    .unwrap();
    let a = fold(r, &entries(&b, r).unwrap(), false).attempt.unwrap();
    b.comment(r, &core::with_fields(entry("result", &[&"c".repeat(40)], Some(a), "Changed: faster status\nfor every reader\nChecks: cargo test --locked -- pass\nChecks: cargo run -- selftest -- pass\nOpen: none"), Some("op-1"), None)).unwrap();
    b.advance(90);
    b
}

// Chain: § 5 status read model. Risk: status outputs disagree.
#[test]
fn text_json_and_service_status_count_from_one_snapshot() {
    let (b, c) = (snapshot_board(), cfg());
    let snap = snapshot::read(&b, &c, &Value::Null, &[]);
    let text = snap.text(&c, &Value::Null).unwrap();
    let j = snap.json();
    let t = &j["totals"];
    assert_eq!(
        (
            t["ready"].as_u64(),
            t["ask"].as_u64(),
            t["waiting"].as_u64(),
            t["failed"].as_u64()
        ),
        (Some(1), Some(1), Some(1), Some(1))
    );
    let line = format!(
        "In work {} · Waiting for answer {} · Ready {}",
        t["doing"].as_u64().unwrap() + t["review"].as_u64().unwrap(),
        t["ask"],
        t["ready"]
    );
    assert!(text.contains(&line), "{text}");
    // the service writes the same rows and totals into status.json; `service status` counts them back
    let root = std::env::temp_dir().join(format!("taskq-snap-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(dot(&root)).unwrap();
    fs::write(dot(&root).join("focus"), "all").unwrap();
    let mut status = json!({"error": null, "at": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), "tasks_age_seconds": 0});
    for (k, v) in snap.status_keys(None) {
        status[k] = v;
    }
    assert_eq!(status["totals"], j["totals"]);
    assert_eq!(status["tasks"], j["tasks"]);
    fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
    assert!(service_fields(&root).contains(&line));
    // a failed pass keeps the last rows for N-1 readers but its counts are unknown
    for (k, v) in snap.status_keys(Some("board down")) {
        status[k] = v;
    }
    assert_eq!(
        (
            status["complete"].clone(),
            status["totals"].clone(),
            status["reason"].clone()
        ),
        (json!(false), Value::Null, json!("board down"))
    );
    fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
    assert!(service_fields(&root).contains("Counts: unknown"));
    let _ = fs::remove_dir_all(&root);
}

// Chain: R12 status unknown. Risk: an unreadable board shows zero.
#[test]
fn unreadable_board_snapshot_is_incomplete_not_zero() {
    let (b, c) = (Fake::default(), cfg());
    let mut snap = snapshot::read(&b, &c, &Value::Null, &[]);
    snap.reason = Some("board unreadable: 502".into());
    let j = snap.json();
    assert_eq!(
        (
            j["complete"].clone(),
            j["reason"].clone(),
            j["totals"].clone()
        ),
        (json!(false), json!("board unreadable: 502"), Value::Null)
    );
    assert_eq!(
        snap.text(&c, &Value::Null).unwrap_err(),
        "board unreadable: 502"
    );
    assert!(
        !snapshot::complete(&json!({"error": "board failed", "tasks": []})),
        "N-1 status.json with an error"
    );
    assert!(
        snapshot::complete(&json!({"error": null, "tasks": []})),
        "N-1 status.json after a good pass"
    );
    assert!(!snapshot::complete(
        &json!({"error": null, "complete": false, "tasks": []})
    ));
}

// Chain: § 5 status --json schema. Risk: status JSON drifts from tests/fixtures.
#[test]
fn status_json_matches_the_golden_fixture() {
    let snap = snapshot::read(&snapshot_board(), &cfg(), &Value::Null, &[]);
    let got = serde_json::to_string_pretty(&snap.json()).unwrap() + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/status-snapshot.json");
    if std::env::var_os("TASKQ_BLESS").is_some() {
        fs::write(&path, &got).unwrap();
    }
    // Parsed compare: a CRLF checkout on Windows must not fail the golden.
    let want: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        snap.json(),
        want,
        "TASKQ_BLESS=1 rewrites {}",
        path.display()
    );
}

/// The JSON Schema subset the snapshot schema uses: type, const, enum, required, properties, additionalProperties, items.
fn schema_errors(schema: &Value, v: &Value, at: &str, out: &mut Vec<String>) {
    let kind = |v: &Value| match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    let types: Vec<&str> = match &schema["type"] {
        Value::String(t) => vec![t],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    if !types.is_empty() && !types.contains(&kind(v)) {
        return out.push(format!("{at}: {} is not {types:?}", kind(v)));
    }
    if schema.get("const").is_some_and(|c| c != v) {
        out.push(format!("{at}: not {}", schema["const"]))
    }
    if schema["enum"].as_array().is_some_and(|e| !e.contains(v)) {
        out.push(format!("{at}: {v} not in enum"))
    }
    if let Value::Object(o) = v {
        for k in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|k| !o.contains_key(*k))
        {
            out.push(format!("{at}: missing {k}"))
        }
        for (k, x) in o {
            let sub = schema["properties"]
                .get(k)
                .or_else(|| schema.get("additionalProperties").filter(|a| a.is_object()));
            if let Some(sub) = sub {
                schema_errors(sub, x, &format!("{at}.{k}"), out)
            }
        }
    }
    if let (Value::Array(a), Some(items)) = (v, schema.get("items")) {
        for (i, x) in a.iter().enumerate() {
            schema_errors(items, x, &format!("{at}[{i}]"), out)
        }
    }
}

// Release shots: each release opened one more draft PR and 13 piled up. Risk: the close step misses them, or reaches a
// PR that is not an earlier shots PR of this repo. Protection: the guards stay on the close step.
#[test]
fn shots_workflow_closes_only_earlier_shots_prs() {
    let shots = include_str!("../.github/workflows/shots.yml");
    let close = &shots[shots.find("gh pr list").expect("close step")..];
    for guard in [
        ".number < ${url##*/}",
        "isCrossRepository | not",
        r#"startswith(\"taskq-shots-\")"#,
        r#"startswith(\"Refresh README screenshots\")"#,
        "gh pr close \"$old\" --delete-branch",
    ] {
        assert!(close.contains(guard), "{guard}");
    }
}

// Chain: § 5 status --json schema -> released snapshot schema and fixtures (#2014). Risk: consumers read a contract the output no longer meets.
#[test]
fn status_json_meets_the_released_snapshot_schema() {
    let schema: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/status-snapshot.schema.json"
    ))
    .unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/fixtures/status-snapshot.json")).unwrap();
    let snap = snapshot::read(&snapshot_board(), &cfg(), &Value::Null, &[]).json();
    for (name, v) in [("status --json", &snap), ("fixture", &fixture)] {
        let mut errors = vec![];
        schema_errors(&schema, v, "$", &mut errors);
        assert!(errors.is_empty(), "{name}: {errors:?}");
    }
    // the schema's state enum is the snapshot vocabulary, and the fixture shows a typed verdict
    let ids: Vec<Value> = snap["vocabulary"]["states"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    assert_eq!(
        schema["properties"]["tasks"]["items"]["properties"]["state"]["enum"]
            .as_array()
            .unwrap(),
        &ids
    );
    assert!(snap["tasks"].as_array().unwrap().iter().any(|t| {
        t["verdict"]["checks"]
            .as_array()
            .is_some_and(|c| !c.is_empty())
    }));
    let mut bad = snap.clone();
    bad["tasks"][0]["verdict"] = json!({"changed": "x"});
    let mut errors = vec![];
    schema_errors(&schema, &bad, "$", &mut errors);
    assert_eq!(
        errors,
        [
            "$.tasks[0].verdict: missing checks",
            "$.tasks[0].verdict: missing open"
        ]
    );
    // A3: each release publishes both files, covered by SHA256SUMS
    let release = include_str!("../.github/workflows/release-build.yml");
    for (file, asset) in [
        ("status-snapshot.json", "status-snapshot-fixtures.json"),
        ("status-snapshot.schema.json", "status-snapshot-schema.json"),
    ] {
        assert!(
            release.contains(&format!(
                "cp tests/fixtures/{file} \"dist/taskq-$VER-{asset}\""
            )),
            "release misses {file}"
        );
    }
}

// Chain: § 2 card -> canonical parsed question -> shared answer card fixtures. Risk: a UI fixture drifts from TaskQ's parse.
#[test]
fn answer_card_fixtures_carry_the_canonical_parse() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/answer-card.json");
    let mut fx: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut want = fx.clone();
    for row in want["cases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|c| c["rows"].as_array_mut().unwrap())
    {
        row["question"] = row["card"].as_str().map_or(Value::Null, snapshot::question);
    }
    if std::env::var_os("TASKQ_BLESS").is_some() {
        fs::write(&path, serde_json::to_string_pretty(&want).unwrap() + "\n").unwrap();
        fx = want.clone();
    }
    assert_eq!(fx, want, "TASKQ_BLESS=1 rewrites {}", path.display());
    // #1787 A3: the Russian fixture card is what `ask` writes, so it passes the § 2 limits counted in characters
    let ru = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "labelled options in Russian")
        .unwrap()["rows"][0]["card"]
        .as_str()
        .unwrap();
    let opts: Vec<String> = core::options(ru).into_iter().map(|o| o.1).collect();
    let text = ru.lines().take(2).collect::<Vec<_>>().join("\n");
    assert_eq!(
        ask_card(
            880,
            &text,
            &opts,
            Some("1"),
            &["[#12](https://github.com/o/r/pull/12)".into()]
        )
        .unwrap(),
        ru
    );
    let q = snapshot::question(
        "Done: d\nProblem: p\nmore\nEvidence: [#1](https://x/1), [shot](https://x/s.png)\n1. close: ship ★\n2. redo\nTo answer: `taskq answer 7.K`",
    );
    assert_eq!(
        (
            q["parsed"].clone(),
            q["problem"].clone(),
            q["evidence"][1]["url"].clone(),
            q["choices"][0]["consequential"].clone()
        ),
        (
            json!(true),
            json!("p more"),
            json!("https://x/s.png"),
            json!(true)
        )
    );
    // #1787: the label shows on the button, the detail beside it; a card without ` - ` keeps its whole text as the label
    let c =
        snapshot::question("Problem: p\n1. Keep - + safe; - slow ★\n2. redo")["choices"].clone();
    assert_eq!(
        [
            &c[0]["label"],
            &c[0]["detail"],
            &c[1]["label"],
            &c[1]["detail"]
        ],
        [
            &json!("Keep"),
            &json!("+ safe; - slow"),
            &json!("redo"),
            &Value::Null
        ]
    );
    let img = snapshot::question(
        "Problem: p\nEvidence: [#1](https://x/1), ![front](https://x/f.png#upload=2)\n1. a ★\n2. b",
    )["evidence"]
        .clone();
    assert_eq!(
        img,
        json!([{"label": "#1", "url": "https://x/1", "image": false}, {"label": "front", "url": "https://x/f.png#upload=2", "image": true}]),
        "#1782 images"
    );
    let vid = snapshot::question("Problem: p\nEvidence: ![run](https://x/u/run.mp4), [▶ clip](https://x/clip.webm)\n1. a ★\n2. b")["evidence"].clone();
    assert_eq!(
        vid,
        json!([{"label": "run", "url": "https://x/u/run.mp4", "image": false}, {"label": "▶ clip", "url": "https://x/clip.webm", "image": false}]),
        "#1844 videos stay links in the apps"
    );
    assert_eq!(
        snapshot::question("1. a ★\n2. b")["parsed"],
        json!(false),
        "no Problem: read only"
    );
    assert_eq!(
        snapshot::question("Problem: p\n1. a\n2. b")["parsed"],
        json!(false),
        "no star: read only"
    );
    // #1662 A4: a repeated or skipped number names no single option, so the card stays read only
    for card in [
        "Problem: p\n1. a ★\n1. b",
        "Problem: p\n1. a ★\n3. b",
        "Problem: p\n2. a ★\n1. b",
    ] {
        assert_eq!(snapshot::question(card)["parsed"], json!(false), "{card}");
    }
    let owner = snapshot::question(
        "Problem: add the board token\nTo answer: `taskq answer 7 --text <done>`",
    );
    assert_eq!(
        (owner["parsed"].clone(), owner["choices"].clone()),
        (json!(true), json!([])),
        "an owner action takes a text answer"
    );
}

/// The card text written outside the § 2 writer in `src`: each string literal of production code (no test item, no
/// comment, no writer function) that holds a card marker, except where the code reads a card (`starts_with(` ...).
fn hand_cards(file: &str, src: &str) -> Vec<String> {
    const WRITER: [&str; 5] = [
        "fn numbered(",
        "fn ask_card(",
        "fn service_card(",
        "fn plain(",
        "fn card(",
    ];
    const READS: [&str; 7] = [
        "starts_with(",
        "strip_prefix(",
        "find(",
        "split_once(",
        "contains(",
        "field(",
        "trim_start_matches(",
    ];
    let lines: Vec<&str> = src.lines().collect();
    let (mut kept, mut i) = (String::new(), 0);
    while i < lines.len() {
        let t = lines[i].trim_start();
        let indent = &lines[i][..lines[i].len() - t.len()];
        let attr = t.starts_with("#[cfg(test)]") || t.starts_with("#[test]");
        if attr || file == "text.rs" && indent.is_empty() && WRITER.iter().any(|w| t.contains(w)) {
            let mut j = i + attr as usize;
            while j < lines.len() && lines[j].trim_start().starts_with("#[") {
                j += 1;
            }
            let one = lines
                .get(j)
                .map(|l| l.trim_end())
                .is_some_and(|l| l.ends_with(';') || l.contains('{') && l.ends_with('}'));
            if !one {
                while j < lines.len() && lines[j].trim_end() != format!("{indent}}}") {
                    j += 1;
                }
            }
            i = j + 1;
            continue;
        }
        kept.push_str(lines[i]);
        kept.push('\n');
        i += 1;
    }
    let c: Vec<char> = kept.chars().collect();
    let (mut out, mut i) = (vec![], 0);
    while i < c.len() {
        match c[i] {
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
            }
            '\'' if c.get(i + 2) == Some(&'\'') => i += 2,
            '\'' if c.get(i + 1) == Some(&'\\') && c.get(i + 3) == Some(&'\'') => i += 3,
            '"' => {
                let start = i;
                i += 1;
                while i < c.len() && c[i] != '"' {
                    i += if c[i] == '\\' { 2 } else { 1 };
                }
                let lit: String = c[start + 1..i.min(c.len())].iter().collect();
                let before: String = c[start.saturating_sub(48)..start].iter().collect();
                let read = READS.iter().any(|r| before.trim_end().ends_with(r))
                    || before.contains("ask_card(");
                if !read
                    && ["Problem: ", "Done: ", "To answer", "★"]
                        .iter()
                        .any(|m| lit.contains(m))
                {
                    out.push(lit);
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

// Chain: § 2 ask card -> every card TaskQ posts (#2012). Risk: a production card is written by hand, so a § 2 format
// change misses it; the scan reads every src file and only the writer, a card reader or this list may hold card text.
#[test]
fn every_production_card_goes_through_the_writer() {
    // worker prompts that teach the card form, and the § 2 star of a pick shown in the inbox
    const KNOWN: [(&str, &str); 4] = [
        ("runtime.rs", "ask {n} --actor owner --text"),
        ("runtime.rs", "Delivery review-pr:"),
        ("snapshot.rs", " ★"),
        ("inbox.rs", "Recommendation: worker ★"),
    ];
    let mut found = vec![];
    let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(d) = dirs.pop() {
        for e in fs::read_dir(&d).unwrap().map(Result::unwrap) {
            let (p, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
                continue;
            }
            for lit in hand_cards(&name, &fs::read_to_string(&p).unwrap()) {
                if !KNOWN.iter().any(|(f, s)| *f == name && lit.contains(s)) {
                    found.push(format!("{name}: {lit}"));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "card text outside the § 2 writer (use service_card or ask_card):\n{}",
        found.join("\n")
    );
    let hand = "fn f(n: u64) -> String {\n    format!(\"Problem: p\\n1. a ★\\n2. b\")\n}\n";
    assert_eq!(
        hand_cards("dispatch.rs", hand),
        ["Problem: p\\n1. a ★\\n2. b"],
        "the guard flags a hand-written card"
    );
    assert!(
        hand_cards(
            "dispatch.rs",
            &format!("#[cfg(test)]\nmod tests {{\n    #[test]\n{hand}}}\n")
        )
        .is_empty(),
        "test text is not production"
    );
    assert!(
        hand_cards(
            "dispatch.rs",
            "fn f(l: &str) -> bool { l.starts_with(\"Problem: \") }\n"
        )
        .is_empty(),
        "reading a card is not writing one"
    );
}

// Chain: § 2 option format -> the writer's check -> fixtures and service cards (#2012). Risk: a fixture or a service
// card shows an option the writer would refuse, so the apps are tested against cards TaskQ never posts.
#[test]
fn fixture_and_service_cards_pass_the_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/answer-card.json");
    let fx: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut seen = 0;
    for c in fx["cases"].as_array().unwrap() {
        for card in c["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["card"].as_str())
        {
            let opts: Vec<String> = core::options(card).into_iter().map(|o| o.1).collect();
            if opts.is_empty() {
                continue;
            }
            seen += 1;
            assert_eq!(
                all([option_lines(&opts), card_links("option", &opts)]),
                Ok(()),
                "{}: {card}",
                c["name"]
            );
        }
    }
    assert!(seen >= 10, "{seen} fixture cards with options");
    let t = core::Task {
        state: State::Doing,
        retries: 2,
        ..Default::default()
    };
    let cards = [
        core::limit_text(7, &t, "requeue", ""),
        dispatch::blocked_card(
            7,
            "CI failed on https://x/run/1 for [#9](https://x/9)",
            None,
        ),
        dispatch::blocked_card(7, "the PR was merged elsewhere", Some("#86 at ddddddd")),
        service::closed_card(7, "2026-10-31 09:00 UTC"),
        service::refused_card(7, "rules over the cap\nNext: trim the rules"),
        service::input_card(7, "worker input", 3),
    ];
    for got in &cards {
        assert_eq!(snapshot::question(got)["parsed"], json!(true), "{got}");
        assert!(!got.contains("card check failed"), "{got}");
        let opts: Vec<String> = core::options(got).into_iter().map(|o| o.1).collect();
        let k = core::options(got)
            .into_iter()
            .find(|o| o.2)
            .unwrap()
            .0
            .to_string();
        let text = got
            .lines()
            .filter(|l| l.starts_with("Done: ") || l.starts_with("Problem: "))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            card(7, &text, &opts, Some(&k), &[], true).as_deref(),
            Ok(got.as_str()),
            "the writer rebuilds the card"
        );
    }
    let bad = service_card(7, "d", "p", &["no detail".into(), "later".into()], 1);
    assert!(
        bad.contains("card check failed: option line 1")
            && snapshot::question(&bad)["parsed"] == json!(true),
        "{bad}"
    );
}

// Chain: canonical board read -> approved projection.
#[test]
fn enrollment_projects_only_approved_board_metadata() {
    use enrollment::{BoardScope, project};
    let mut snap = snapshot::read(&snapshot_board(), &cfg(), &Value::Null, &[]);
    snap.board["host"] = json!("github.com");
    snap.board["token"] = json!("forge-secret");
    snap.board["url"] = json!("https://user:forge-secret@github.com/o/r");
    let scope = BoardScope::new("github", "GITHUB.COM", "o/r").unwrap();
    let approved = [scope.clone()];
    let local = enrollment::Local {
        host: "mac".into(),
        service: None,
        ttl: 300,
        agents: Value::Null,
        progress: Default::default(),
    };
    let a = project(&snap, &"a".repeat(64), &approved, &local).unwrap();
    let got = serde_json::to_value(&a).unwrap();
    let want: Value =
        serde_json::from_str(include_str!("../tests/fixtures/enrollment-projection.json")).unwrap();
    assert_eq!(got, want);
    let text = got.to_string();
    for omitted in [
        "forge-secret",
        "session",
        "labels",
        "title",
        "Which way?",
        "no login",
        "faster status",
        "verdict",
        "url",
        "hosts",
    ] {
        assert!(!text.contains(omitted), "{omitted}");
    }
    assert!(project(&snap, &"a".repeat(64), &[], &local).is_err());
    assert_eq!(
        scope,
        BoardScope::new("github", "github.com", "O/R").unwrap()
    );
    for (host, repo) in [
        ("user:secret@github.com", "o/r"),
        ("github.com", "o/../r"),
        ("github.com", "o/r?token=x"),
    ] {
        assert!(BoardScope::new("github", host, repo).is_err());
    }
    snap.reason = Some("forge-secret and private path".into());
    let incomplete =
        serde_json::to_value(project(&snap, &"a".repeat(64), &approved, &local).unwrap()).unwrap();
    assert_eq!(
        (
            &incomplete["complete"],
            &incomplete["reason"],
            &incomplete["totals"]
        ),
        (&json!(false), &json!("source incomplete"), &Value::Null)
    );
    let other = BoardScope::new("gitlab", "gitlab.com", "o/r").unwrap();
    assert!(project(&snap, &"a".repeat(64), &[other], &local).is_err());
}

// Chain: §5 PilotQ-shaped CLI arguments -> shared dispatcher -> board manifest. Risk: file-only input still requires --text.
#[test]
fn reply_files_cli_accepts_downloaded_media_and_refuses_numbered_choices() {
    let mut config = cfg();
    fs::create_dir(&config.root).unwrap();
    config.install = config.root.clone();
    let file = config.root.join("downloaded-photo.png");
    fs::write(&file, b"invented photo").unwrap();
    let b = Fake::default();
    let n = b
        .create("PilotQ CLI fixture", "", &["taskq".into()])
        .unwrap();
    let a = b
        .comment(n, "taskq: spawn worker claude:fixture mac")
        .unwrap();
    b.comment(n, &entry("ask", &[], Some(a), "Problem: attach a file"))
        .unwrap();
    let c = Ctx {
        cfg: config,
        raw: Value::Null,
        board: Box::new(b),
    };
    let list = c.cfg.root.join("files.json");
    fs::write(
        &list,
        serde_json::to_vec(&vec![file.display().to_string()]).unwrap(),
    )
    .unwrap();
    for (kind, text) in [
        ("answer", ""),
        ("steer", "mixed reply"),
        ("requeue", "Reason: inspect the file"),
    ] {
        let revision = core::revision(n, &entries(c.board.as_ref(), n).unwrap());
        let (flag, input) = if kind == "answer" {
            ("--files-from", &list)
        } else {
            ("--file", &file)
        };
        let mut args = Args::parse(&[
            kind.into(),
            n.to_string(),
            flag.into(),
            input.display().to_string(),
            "--text-stdin".into(),
            "--op-id".into(),
            format!("pilotq-{kind}"),
            "--revision".into(),
            revision.to_string(),
        ])
        .unwrap();
        args.read_text(&mut std::io::Cursor::new(text.as_bytes()))
            .unwrap();
        command(&c, kind, &args).unwrap();
        let log = entries(c.board.as_ref(), n).unwrap();
        let files = reply_files::manifest(&log.last().unwrap().body).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "downloaded-photo.png");
    }
    for command in [
        vec!["answer", "7.1"],
        vec!["close", "7"],
        vec!["later", "7"],
        vec!["result", "7", "--files-from", &list.display().to_string()],
    ] {
        let mut command: Vec<String> = command.iter().map(|s| s.to_string()).collect();
        command.extend(["--file".into(), file.display().to_string()]);
        assert!(Args::parse(&command).is_err());
    }
    // #2019: a worker hands in files with result N and ask N
    for kind in ["result", "ask"] {
        assert!(
            Args::parse(&[
                kind.into(),
                "7".into(),
                "--file".into(),
                file.display().to_string()
            ])
            .is_ok()
        );
    }
    fs::remove_dir_all(c.cfg.root).unwrap();
}

// Chain: § 5 CLI text limits. Risk: invalid text is written.
#[test]
fn cli_audit_boundaries_and_no_write_on_invalid_text() {
    assert_eq!(width("a  b"), 4);
    assert_eq!(width("a`unclosed"), 10);
    assert_eq!(width("é🦀"), 2);
    assert!(one_line("option", &["a\rb".into()]).is_err());
    let text = "Changed: visible\nChecks: A1 ok\nChecks: A2 ok; CI green on abc\nOpen: none";
    close_verdict(text, "A1. a\nA2. b", false).unwrap();
    assert!(task_text("requeue", "unlabelled reason").is_err());
    assert!(task_text("requeue", &format!("Reason: ok{}", " ".repeat(240))).is_err());
    let e = task_text(
        "requeue",
        &format!("Reason: {}\nNext: a\nb", "r".repeat(240)),
    )
    .unwrap_err();
    assert!(
        e.contains("requeue: 3 lines, allowed 1-2")
            && e.contains("Reason line 1: 248 characters, limit 240")
            && e.contains("Next: 2 lines, allowed 0-1"),
        "{e}"
    );
    assert!(task_text("failed", "Reason: blocked\nNext: repair").is_ok());
    assert!(task_text("steer", "1\n2\n3\n4\n5").is_ok());
    for cmd in ["answer", "steer"] {
        assert!(task_text(cmd, &"x".repeat(267)).is_ok());
        assert!(task_text(cmd, &"word\n".repeat(400)).is_ok());
    }
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let n = c.board.create("T", "", &["taskq".into()]).unwrap();
    c.board
        .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let before = c.board.comments(n).unwrap();
    for (cmd, text) in [
        ("later", "bad"),
        ("failed", "bad"),
        ("answer", ""),
        ("steer", " \n "),
    ] {
        let a = Args::parse(&[cmd.into(), n.to_string(), "--text".into(), text.into()]).unwrap();
        assert!(command(&c, cmd, &a).is_err(), "{cmd}");
    }
    assert_eq!(c.board.comments(n).unwrap().len(), before.len());
    run(&["--help".into()]).unwrap();
    run(&["service".into(), "pause".into(), "--help".into()]).unwrap();
    run(&["close".into(), "12".into(), "--help".into()]).unwrap();
    assert!(USAGE.contains("--recommend K"));
    // Risk (#1910): the PM catalog drifts from the CLI, or help stops rendering it for PM surfaces.
    let catalog: Value = serde_json::from_str(PM_COMMANDS).unwrap();
    let text = help(false).unwrap();
    assert!(text.starts_with(USAGE));
    // #2288: help tells the PM to start dependents with `--deps` and hold only for read-back
    assert!(
        text.contains("gets `--deps N` and is started at once")
            && text.contains("hold is only for the read-back"),
        "{text}"
    );
    for c in catalog["commands"].as_array().unwrap() {
        let (name, usage) = (c["name"].as_str().unwrap(), c["use"].as_str().unwrap());
        let mut words = name.split(' ');
        let first = words.next().unwrap();
        assert!(
            (USAGE.contains(&format!("taskq {first}"))
                || USAGE.contains(&format!("taskq service {first}")))
                && words.all(|w| USAGE.contains(&format!("|{w}|"))),
            "{name} is not a taskq command"
        );
        assert!(
            text.contains(usage) && text.contains(c["purpose"].as_str().unwrap()),
            "{name}"
        );
    }
    assert!(
        text.contains("'taskq телеметрия': taskq profile watch --since 1h; procedure: ")
            && text.contains(profile::WATCH_DOC)
    );
    assert_eq!(
        serde_json::from_str::<Value>(&help(true).unwrap()).unwrap(),
        catalog
    );
    // Manager mode: exact on and off instructions, period and scope reach the timer prompt verbatim.
    let args =
        |v: &[&str]| Args::parse(&v.iter().map(|w| w.to_string()).collect::<Vec<_>>()).unwrap();
    let on = pm_instructions(&args(&["pm"])).unwrap();
    assert!(
        on.contains("`interval` 180")
            && on.contains(
                "Period: 3m. Prompt, verbatim: `Manager tick: run taskq pm --every 3m and do"
            )
            && on.contains("`Manager on · 3m · all`")
            && !on.contains("{{"),
        "{on}"
    );
    let some = pm_instructions(&args(&[
        "pm",
        "--every",
        "5m",
        "--project",
        "a",
        "--project",
        "b",
    ]))
    .unwrap();
    assert!(
        some.contains("`interval` 300")
            && some.contains("run taskq pm --every 5m --project a --project b and do")
            && some.contains("`Manager on · 5m · a, b`"),
        "{some}"
    );
    let off = pm_instructions(&args(&["pm", "stop"])).unwrap();
    assert!(
        on.contains("`pm_note_write` with `id` `manager-mode`")
            && off.contains("`pm_note_forget` with `id` `manager-mode`")
    );
    assert!(off.contains("`loop_stop`") && off.contains("`Manager off`"));
    assert!(
        pm_instructions(&args(&["pm", "--every", "0m"])).is_err()
            && pm_instructions(&args(&["pm", "--every", "90s"])).is_err()
            && pm_instructions(&args(&["pm", "go"])).is_err()
    );
    assert!(USAGE.contains("--snapshot DIR"));
    assert!(USAGE.contains("taskq service capacity [--host H] [--max N|none] [--claude N] [--codex N] [--antigravity N] [--codex-app N] [--load-gate on|off] [--cpu-max P] [--memory-min P] [--claim any|spillover|labelled|none] [--default-runtime claude|codex|antigravity]"));
    run(&["service".into(), "capacity".into(), "--help".into()]).unwrap();
    let e = platform::error_text("#7 is ready", None);
    assert!(e.starts_with("Error: #7 is ready\nNext: "));
    assert_eq!(e.lines().count(), 2);
    // #1894: an error carrying its own fitting `Next:` line keeps it instead of the generic one.
    assert_eq!(
        platform::error_text("no GitLab token: no f\nNext: write a token to f", None),
        "Error: no GitLab token: no f\nNext: write a token to f"
    );
    let dep = c.board.create("ordinary issue", "", &[]).unwrap();
    let blocked = c
        .board
        .create(
            "blocked",
            &format!("Depends on #{dep}"),
            &["taskq".into(), "selected".into()],
        )
        .unwrap();
    let report = report(c.board.as_ref(), &c.cfg, &c.raw, &["selected".into()]).unwrap();
    assert!(report.contains(&format!("#{blocked} blocked")));
    assert!(
        report.contains(&format!("dependency ([#{dep}](https://board/{dep}) open)")),
        "non-task dependency remains visible outside filter"
    );
}

// Chain: § 5 piped owner reply -> CLI parsing -> shared dispatcher. Risk: OS arguments cap or alter the text.
#[test]
fn human_reply_stdin_preserves_unbounded_text_and_validation() {
    let text = "Words and 🦀 from stdin.\n".repeat(6000);
    for kind in ["answer", "steer", "requeue"] {
        let mut a = Args::parse(&[kind.into(), "1".into(), "--text-stdin".into()]).unwrap();
        a.read_text(&mut text.as_bytes()).unwrap();
        assert_eq!(a.one("text"), Some(text.as_str()));
        assert!(a.one("text-stdin").is_none());
        assert!(
            Args::parse(&[
                kind.into(),
                "1".into(),
                "--text-stdin".into(),
                "--text".into(),
                "ambiguous".into()
            ])
            .is_err()
        );
    }
    assert!(Args::parse(&["answer".into(), "1.2".into(), "--text-stdin".into()]).is_err());
    assert!(Args::parse(&["result".into(), "1".into(), "--text-stdin".into()]).is_err());
    let mut a = Args::parse(&["answer".into(), "1".into(), "--text-stdin".into()]).unwrap();
    assert!(
        a.read_text(&mut &[0xff_u8][..]).is_err(),
        "invalid UTF-8 must never become a changed reply"
    );
}

// Chain: § 5 large stored answer -> next status/late receipt -> file JSON. Risk: a child stdout cap blocks the next Send.
#[test]
fn human_reply_json_files_preserve_large_status_and_receipts() {
    let dir = std::env::temp_dir().join(format!("taskq-reply-json-{}", runtime::uuid()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("receipt.json");
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    let b = &*c.board;
    let n = b
        .create("JSON reply fixture", "", &["taskq".into()])
        .unwrap();
    let attempt = b
        .comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    b.comment(n, &entry("ask", &[], Some(attempt), "Problem: reply"))
        .unwrap();
    let text = "word ".repeat(300_000);
    assert!(act(b, Path::new(""), n, "answer", &[], &text, &Op::default()).ok());
    let output = |cmd: &str| {
        Args::parse(&[
            cmd.into(),
            "--json".into(),
            "--json-file".into(),
            path.to_string_lossy().into_owned(),
        ])
        .unwrap()
    };
    command(&c, "status", &output("status")).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(
        bytes.len() > 1024 * 1024,
        "fixture crosses the bounded stdout buffer"
    );
    let status: Value = serde_json::from_slice(&bytes).unwrap();
    let expected = wrap_reply(&text);
    assert!(
        status["tasks"][0]["answered"]["text"].as_str() == Some(expected.trim()),
        "status lost or changed the wrapped reply"
    );
    let mut a = output("answer");
    a.pos.push(n.to_string());
    a.flags.insert("text".into(), vec!["late reply".into()]);
    assert!(
        command(&c, "answer", &a).is_ok(),
        "a late reply is saved as a note without reconfirmation"
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(
        receipt["answered"]["text"].as_str() == Some(expected.trim()),
        "receipt lost or changed the wrapped reply"
    );
    assert_eq!(receipt["outcome"], "accepted");
    assert_eq!(receipt["note"], true);
    let log = entries(b, n).unwrap();
    assert_eq!(
        core::delivery(
            n,
            &log,
            *core::project(n, &log, false).notes.last().unwrap()
        ),
        "late reply"
    );
    assert!(Args::parse(&["status".into(), "--json-file".into(), "out".into()]).is_err());
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
}

// Chain: R6 compact report. Risk: compaction drops choices, links or evidence.
#[test]
fn compact_report_keeps_choices_links_and_original_evidence() {
    let (b, c) = (Fake::default(), cfg());
    let title = "🦀".repeat(65);
    let n = b.create(&title, "", &["taskq".into()]).unwrap();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let reason = "r".repeat(170);
    let option = "x".repeat(125);
    let card = format!(
        "Done: inspected\nProblem: {reason}\nEvidence: [context](https://context)\n1. {option} ★\n2. keep\nTo answer: exact syntax"
    );
    let a = fold(n, &entries(&b, n).unwrap(), false).attempt;
    b.comment(n, &entry("ask", &["owner"], a, &card)).unwrap();
    let r = report(&b, &c, &Value::Null, &[]).unwrap();
    assert!(r.contains(&format!("{}…", "🦀".repeat(59))));
    assert!(r.contains(&format!("[{}…](https://board/{n})", "r".repeat(159))));
    assert!(r.contains(&format!(
        "{n}.1 [{}…](https://board/{n}) ★",
        "x".repeat(119)
    )));
    assert!(r.contains(&format!("{n}.2 keep")));
    assert!(!r.contains("Done: inspected"));
    assert!(!r.contains("To answer:"));
    assert_eq!(
        fold(n, &entries(&b, n).unwrap(), false).card.as_deref(),
        Some(card.as_str())
    );
    assert_eq!(b.issue(n).unwrap().title, title);
    assert_eq!(
        snapshot::brief("Old question?\n1. keep\n2. change"),
        "Old question?"
    );
}

// Chain: § 2 owner action (#1125): ask without options, R6 row, answer resumes the attempt. Risk: an action card is
// refused, shows no action in status, or its answer starts a new attempt.
#[test]
fn owner_action_waits_in_status_and_its_answer_resumes_the_attempt() {
    let (b, c) = (Fake::default(), cfg());
    let n = b.create("t", "", &["taskq".into()]).unwrap();
    b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, ""))
        .unwrap();
    let a = fold(n, &entries(&b, n).unwrap(), false).attempt;
    b.comment(
        n,
        &entry(
            "ask",
            &["owner"],
            a,
            &ask_card(
                n,
                "Done: tests pass\nProblem: add the board token",
                &[],
                None,
                &[],
            )
            .unwrap(),
        ),
    )
    .unwrap();
    let r = report(&b, &c, &Value::Null, &[]).unwrap();
    assert!(
        r.contains("| waiting for owner: add the board token |"),
        "{r}"
    );
    b.comment(n, &entry("answer", &[], None, "token added"))
        .unwrap();
    let t = fold(n, &entries(&b, n).unwrap(), false);
    assert_eq!((t.state, t.attempt, t.inbox.len()), (State::Doing, a, 1));
}

// Chain: R12 service status fields. Risk: missing data shown as known; failed pause reported ok.
#[test]
fn service_fields_keep_missing_data_unknown_and_pause_reports_failure() {
    let root = std::env::temp_dir().join(format!("taskq-fields-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let s = service_fields(&root);
    assert!(s.contains("Focus: unknown"));
    assert!(s.contains("Counts: unknown"));
    assert!(s.contains("Freshness: unknown"));
    assert!(!s.contains("Ready 0"));
    assert!(
        pause(&root, false).is_err(),
        "missing state folder cannot acknowledge pause"
    );
    fs::create_dir_all(dot(&root)).unwrap();
    fs::write(dot(&root).join("focus"), "one two").unwrap();
    let mut status = json!({"error": null, "at": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), "tasks_age_seconds": 0, "tasks": [{"state":"doing"},{"state":"review"},{"state":"ask"},{"state":"ready"}]});
    fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
    let s = service_fields(&root);
    assert!(s.contains("Focus: one\nFocus: two"));
    assert!(s.contains("In work 2 · Waiting for answer 1 · Ready 1"));
    assert!(s.contains("cached status"));
    assert!(
        !s.contains("Service:"),
        "project cache cannot claim native host health"
    );
    // Capacity-only publication keeps the file fresh without refreshing the board counts.
    status["tasks_age_seconds"] = json!(301);
    status["capacity"] = json!({"gate": "off"});
    fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
    assert!(coreq::status::age(&dot(&root).join("status.json")).unwrap() <= 1);
    assert!(service_fields(&root).contains("Counts: unknown"));
    status["tasks_age_seconds"] = Value::Null;
    fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
    assert!(
        service_fields(&root).contains("Counts: unknown"),
        "old snapshots lack board freshness"
    );
    fs::write(
        dot(&root).join("status.json"),
        r#"{"error":"board failed","tasks":[]}"#,
    )
    .unwrap();
    assert!(service_fields(&root).contains("Counts: unknown"));
    let _ = fs::remove_dir_all(&root);
}

// A4: receipt replay and status must distinguish one removal target from the reviewed registry.
#[test]
fn lifecycle_scope_names_only_the_project_being_uninstalled() {
    let mut receipt = json!({"action":"uninstall","project":"/fixture/remove","projects":["/fixture/remove","/fixture/keep"]});
    assert_eq!(
        lifecycle_scope(&receipt),
        "Scope: project uninstall; /fixture/remove\nReviewed registry (other projects stay registered):\nRegistered project: /fixture/remove\nRegistered project: /fixture/keep"
    );
    receipt.as_object_mut().unwrap().remove("project");
    assert!(
        lifecycle_scope(&receipt)
            .starts_with("Scope: host uninstall; all projects drain; registry kept\n")
    );
    for action in ["restart", "stop"] {
        receipt["action"] = json!(action);
        assert!(
            lifecycle_scope(&receipt).starts_with("Scope: host service; all registered projects\n")
        );
    }
}

// Chain: #1975 report rows -> reports.json; status.json and `service status --json` keep counts.
// Risk: every collected report embedded per board (2 MB on 7 boards) breaks PilotQ's 1 MiB tool cap.
#[test]
fn service_status_json_stays_bounded_with_many_reports_per_board() {
    let at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let rows: Vec<Value> = (0..600).map(|n| json!({"n":n,"kind":"report","sha":"a".repeat(40),"closed":at,"read":n%2==0,"visible":true,
            "title":"Invented report title","body":"Invented report body line.\n".repeat(80),"tasks":[]})).collect();
    let value =
        json!({"reports":rows,"history":rows[..25],"complete":true,"reason":null,"observed":at});
    // About 25 KB of other status per board, the largest measured without reports.
    let filler = json!({"tasks":(0..60).map(|n|json!({"n":n,"state":"ready","title":"Invented task title ".repeat(10)})).collect::<Vec<_>>()});
    let mut projects = Vec::new();
    for i in 0..7 {
        let root = std::env::temp_dir().join(format!("taskq-bound-{}-{i}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(dot(&root)).unwrap();
        fs::write(dot(&root).join("reports.json"), value.to_string()).unwrap();
        // Board 0 keeps a status written before #1975, rows inline.
        let reports = if i == 0 {
            value.clone()
        } else {
            reports::summary(&value)
        };
        let status = json!({"at":at,"tasks":filler["tasks"],"reports":reports});
        fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
        let read = data::json(&dot(&root).join("status.json"));
        projects.push(service_project(&root, "current", &read));
        assert_eq!(
            reports::cached_list(&root, 1, None).unwrap()["reports"][0]["n"],
            0,
            "results keep the rows"
        );
        fs::remove_dir_all(&root).unwrap();
    }
    assert_eq!(projects[1]["status"]["reports"]["count"], 600);
    assert_eq!(projects[0]["status"]["reports"]["unread"], 300);
    let out = json!({"schema":"taskq.service/1","projects":projects}).to_string();
    assert!(
        out.len() < 256 * 1024,
        "service status --json is {} bytes",
        out.len()
    );
}

// Chain: § 5 error diagnostics. Risk: a compact error loses its full diagnostic.
// Risk: adding a full poll after native work drifts heartbeat cadence; a pre-pass poke must still wake it.
#[test]
fn completed_pass_does_not_add_another_poll_interval() {
    let dir = std::env::temp_dir().join(format!("taskq-poll-{}", runtime::uuid()));
    fs::create_dir(&dir).unwrap();
    let paths = [dir.join("project"), dir.join("host")];
    let before = poke_times(&paths);
    let mut ticks = 0;
    let began = Instant::now();
    sleep_until_poke(&paths, before, began - Duration::from_secs(1), &mut || {
        ticks += 1
    });
    fs::write(&paths[0], "wake").unwrap();
    sleep_until_poke(&paths, before, began + Duration::from_secs(60), &mut || {
        ticks += 1
    });
    assert_eq!(ticks, 0);
    assert!(began.elapsed() < Duration::from_secs(1));
    fs::remove_dir_all(dir).unwrap();
}

// Chain: tray write -> reread request -> next pass; open window -> short poll (#1819). Risk: a concurrent append
// is lost, the reread repeats every pass, or a closed menu keeps the board polled every 15 s.
#[test]
fn tray_writes_reread_once_and_an_open_menu_polls_short() {
    let dir = std::env::temp_dir().join(format!("taskq-reread-{}", runtime::uuid()));
    fs::create_dir_all(dot(&dir)).unwrap();
    ask_reread(&dir, 7).unwrap();
    ask_reread(&dir, 9).unwrap();
    assert!(
        dot(&dir).join("poke").exists(),
        "the write wakes the service"
    );
    assert_eq!(take_reread(&dot(&dir)), [7, 9]);
    assert_eq!(take_reread(&dot(&dir)), Vec::<u64>::new(), "taken once");
    assert!(!menu_open(&dir));
    touch(&dir.join("menu-open"));
    assert!(menu_open(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compact_errors_retain_complete_diagnostics() {
    let root = std::env::temp_dir().join(format!("taskq-error-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let original = format!("#7 {}\nexact evidence", "é".repeat(200));
    let e = platform::error_text(&original, Some(&root));
    assert_eq!(e.lines().count(), 2);
    assert!(width(e.lines().next().unwrap()) <= 160);
    assert!(e.contains("cli-errors.log"));
    assert_eq!(
        fs::read_to_string(dot(&root).join("cli-errors.log")).unwrap(),
        format!("{original}\n")
    );
    let failed = platform::error_text(&original, None);
    assert!(
        failed.contains(&original),
        "a failed save never discards the original evidence"
    );
    // #1555: no bare `could not be saved`; it says why
    assert!(
        failed.ends_with("no cli-errors.log copy (outside a taskq project)."),
        "{failed}"
    );
    let _ = fs::remove_dir_all(&root);
}

// Chain: MP14 Completed. Risk: a page invents an outcome, loses board paging or hides the PR link.
#[test]
fn completed_reads_board_pages_with_sourced_outcome() {
    let b = Fake::default();
    for n in 1..=12u64 {
        b.create(&format!("t{n}"), "", &["taskq".into()]).unwrap();
        b.close(n).unwrap();
        let mut st = b.state.borrow_mut();
        let i = st.issues.iter_mut().find(|i| i.number == n).unwrap();
        (i.updated, i.reason) = (
            100 + n as i64,
            [Some("completed".into()), Some("not_planned".into()), None][n as usize % 3].clone(),
        );
    }
    b.comment(12, "Merged https://github.com/o/r/pull/7.")
        .unwrap();
    b.state
        .borrow_mut()
        .issues
        .iter_mut()
        .find(|i| i.number == 12)
        .unwrap()
        .updated = 112;
    let first = closed_history(&b, 1, None, false).unwrap();
    assert_eq!(
        (
            first["tasks"].as_array().unwrap().len(),
            first["more"].clone()
        ),
        (10, json!(true))
    );
    assert_eq!(
        (
            first["tasks"][0]["n"].clone(),
            first["tasks"][0]["outcome"].clone(),
            first["tasks"][0]["pr"].clone()
        ),
        (
            json!(12),
            json!("success"),
            json!("https://github.com/o/r/pull/7")
        )
    );
    assert_eq!(
        (
            first["tasks"][1]["outcome"].clone(),
            first["tasks"][2]["outcome"].clone()
        ),
        (json!("unknown"), json!("not_planned")),
        "GitHub without a reason stays unknown"
    );
    let last = closed_history(&b, 2, None, false).unwrap();
    assert_eq!(
        (
            last["tasks"].as_array().unwrap().len(),
            last["more"].clone()
        ),
        (2, json!(false))
    );
    assert_eq!(
        closed_history(&b, 1, None, true).unwrap()["tasks"][1]["outcome"],
        "success",
        "§ 1: a closed GitLab issue is done"
    );
    assert_eq!(
        closed_history(&b, 1, Some(110), false).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "period keeps closes at or after since"
    );
}

// Before: the tray's `status --json` got the all-project shape after 48 s and every control failed. After: one project.
#[test]
fn status_budget_warning_stays_out_of_json_stdout() {
    let prints_json = |words: &[&str]| {
        let v: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        status_prints_json(&Args::parse(&v).unwrap())
    };
    assert!(prints_json(&["status", "--json"]));
    assert!(prints_json(&["status", "--closed"]));
    assert!(
        !prints_json(&["status"]),
        "text status keeps the warning first on stdout (R6)"
    );
}

#[test]
fn status_json_inside_a_project_reads_only_that_project() {
    assert!(json_here_only(true, &[], true));
    assert!(
        !json_here_only(false, &[], true),
        "text status still reports every listed project (R10)"
    );
    assert!(
        !json_here_only(true, &["other".into()], true),
        "--project narrows by name"
    );
    assert!(
        !json_here_only(true, &[], false),
        "outside a project JSON stays all-project"
    );
}

// #1829 A1/A2: a pin outside page one reads its exact board outcome; native reads cannot resume work or cross hosts.
/// A1/A4 read-only live board instrument; no labels, comments, source bodies or credentials are printed.
#[test]
#[ignore = "requires explicit TASKQ_SOURCE_COST_ROOT and closed TASKQ_SOURCE_COST_TASK"]
fn conversations_board_cost_checkpoint() {
    let root =
        PathBuf::from(std::env::var_os("TASKQ_SOURCE_COST_ROOT").expect("explicit project root"));
    let n = std::env::var("TASKQ_SOURCE_COST_TASK")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let (cfg, raw) = load(&root).unwrap();
    let b = cfg
        .board
        .open(
            &cfg.web_host,
            &cfg.repo,
            raw["label_schema"].as_str(),
            &platform::tool(&cfg.install),
        )
        .unwrap();
    let diff = |before: &std::collections::BTreeMap<String, u64>,
                after: &std::collections::BTreeMap<String, u64>| {
        after
            .iter()
            .filter_map(|(k, v)| {
                let n = v.saturating_sub(before.get(k).copied().unwrap_or(0));
                (n > 0).then(|| (k.clone(), n))
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    for baseline in [true, false] {
        let cache = reports::Selected::default();
        let mut revision = None;
        for page in 0..3 {
            let before = b.requests().ops;
            let started = Instant::now();
            let value = if baseline {
                closed_pin(&*b, n, !cfg.board.closed_reason)
            } else {
                cache.read(b.url(n), revision, Instant::now(), || {
                    closed_pin(&*b, n, !cfg.board.closed_reason)
                })
            }
            .unwrap();
            assert_eq!(
                value["tasks"][0]["state"], "closed",
                "the instrument is closed-task read-only"
            );
            assert!(value["tasks"][0]["error"].is_null());
            revision = value["tasks"][0]["revision"].as_u64();
            let ops = diff(&before, &b.requests().ops);
            let http = ops.values().sum::<u64>();
            let not_modified = ops
                .iter()
                .filter(|(k, _)| k.ends_with(" 304"))
                .map(|(_, n)| n)
                .sum::<u64>();
            println!(
                "BOARD_COST {}",
                json!({"task":n,"baseline":baseline,"page":page,"logical_selected_reads":1,"http":http,"not_modified_304":not_modified,"billed":requests::billed(&ops),"operations":ops,"elapsed_us":started.elapsed().as_micros(),"projection_bytes":value.to_string().len(),"history_rows":value["tasks"][0]["history"].as_array().map(Vec::len)})
            );
            if !baseline && page > 0 {
                assert_eq!(http, 0, "warm paging must not revalidate any HTTP page");
            }
        }
    }
}

#[test]
fn closed_thread_keeps_attempt_roles_and_source_times() {
    // A5: closed threads must interleave every accepted attempt with board records, just like live threads.
    let b = Fake::default();
    let n = b.create("Thread fixture", "", &["taskq".into()]).unwrap();
    let first = b.comment(n, "taskq: spawn worker codex:first mac").unwrap();
    b.comment(n, "taskq: spawn worker codex:ignored mac")
        .unwrap();
    b.comment(n, &format!("taskq: gone attempt {first}"))
        .unwrap();
    let second = b
        .comment(n, "taskq: spawn worker codex:second mac")
        .unwrap();
    b.comment(
        n,
        &format!("taskq: result {} attempt {second}", "a".repeat(40)),
    )
    .unwrap();
    let reviewer = b
        .comment(n, "taskq: spawn reviewer codex:review mac")
        .unwrap();
    let live = snapshot::read(&b, &cfg(), &Value::Null, &[]).json()["tasks"][0].clone();
    b.comment(n, &format!("taskq: close attempt {reviewer}"))
        .unwrap();
    b.close(n).unwrap();
    let comments = b.comments(n).unwrap();
    let started = |id| comments.iter().find(|c| c.id == id).unwrap().created;
    let row = closed_pin(&b, n, false).unwrap()["tasks"][0].clone();
    assert_eq!(
        row["workers"],
        json!([
            {"attempt":first,"role":"worker","session":"codex:first mac","started":started(first)},
            {"attempt":second,"role":"worker","session":"codex:second mac","started":started(second)}
        ])
    );
    assert_eq!(
        row["reviewers"],
        json!([
            {"attempt":reviewer,"role":"reviewer","session":"codex:review mac","started":started(reviewer)}
        ])
    );
    assert_eq!(live["workers"], row["workers"]);
    assert_eq!(live["reviewers"], row["reviewers"]);
    assert_eq!(closed_history(&b, 1, None, false).unwrap()["tasks"][0], row);
    b.state.borrow_mut().fail_comment_reads = 1;
    let unknown = closed_pin(&b, n, false).unwrap()["tasks"][0].clone();
    assert!(
        !unknown["error"].is_null(),
        "unreadable history stays unknown"
    );
    assert!(unknown["workers"].is_null());
    assert!(unknown["reviewers"].is_null());
}

#[test]
fn closed_pin_reads_exact_issue_and_fences_transcript() {
    let b = Fake::default();
    for n in 1..=12 {
        b.create(&format!("pin {n}"), "", &["taskq".into()])
            .unwrap();
        b.close(n).unwrap();
    }
    assert!(
        closed_history(&b, 1, None, false).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["n"] != 1)
    );
    let closed = b.issue(1).unwrap().closed;
    b.comment(1, "A later comment must not reset the closure time")
        .unwrap();
    let pin = closed_pin(&b, 1, false).unwrap()["tasks"][0].clone();
    assert_eq!(
        (
            pin["n"].clone(),
            pin["state"].clone(),
            pin["outcome"].clone(),
            pin["closed"].clone()
        ),
        (json!(1), json!("closed"), json!("success"), json!(closed))
    );
    b.state.borrow_mut().issues[0].reason = None;
    assert_eq!(
        closed_pin(&b, 1, false).unwrap()["tasks"][0]["outcome"],
        "unknown"
    );
    assert_eq!(
        closed_pin(&b, 1, true).unwrap()["tasks"][0]["outcome"],
        "success",
        "§ 1 GitLab has no closure reason"
    );
    b.state.borrow_mut().issues[0].open = true;
    assert_eq!(
        closed_pin(&b, 1, true).unwrap()["tasks"][0]["state"],
        "open"
    );
    let local = std::env::temp_dir().join(format!("taskq-closed-transcript-{}", runtime::uuid()));
    let unreached =
        |_: &str, _: u64| -> Res<Value> { panic!("a local or refused read crossed the host link") };
    assert!(
        closed_transcript(&b, &local, 1, "mac", None, unreached)
            .unwrap_err()
            .contains("closed trusted task")
    );
    let spawn = b
        .comment(1, "taskq: spawn worker codex:invented1829 remote")
        .unwrap();
    b.comment(
        1,
        &format!("taskq: result {} attempt {spawn}", "a".repeat(40)),
    )
    .unwrap();
    b.close(1).unwrap();
    // Before: "Transcript is on host remote; open it there". After: the read goes to the worker's host.
    let page = closed_transcript(&b, &local, 1, "mac", Some(9), |host, attempt| {
        Ok(json!({"host":host,"attempt":attempt}))
    })
    .unwrap();
    assert_eq!(page, json!({"host":"remote","attempt":spawn}));
    // Before: a remote-control worker's board job id named no transcript. After: the attempt log names the session.
    let job = b
        .comment(
            2,
            "taskq: spawn worker claude:88b43179-06e0-4c61-bf19-5cd156f22dd3 mac",
        )
        .unwrap();
    b.comment(
        2,
        &format!("taskq: result {} attempt {job}", "b".repeat(40)),
    )
    .unwrap();
    fs::create_dir_all(dot(&local)).unwrap();
    fs::write(runtime::attempt_log(&local,"worker",2,job),"{\"type\":\"thread.started\",\"session_id\":\"3aae983f-ee51-4fa1-af25-7252766bec4b\",\"runtime\":\"claude-bg\"}\n{\"type\":\"system\",\"sessionId\":\"3aae983f-ee51-4fa1-af25-7252766bec4b\"}\n").unwrap();
    let value = reports::selected(&local, &b, 2, false, None).unwrap();
    let (rt, id, owner, attempt) = reports::worker(&value, None).unwrap();
    assert_eq!((owner.as_str(), attempt), ("mac", job));
    assert_eq!(
        closed_session(&local, &rt, &id, 2, attempt).unwrap(),
        "3aae983f-ee51-4fa1-af25-7252766bec4b"
    );
    b.state.borrow_mut().issues[0].trusted = false;
    assert!(
        closed_pin(&b, 1, true)
            .unwrap_err()
            .contains("trusted task")
    );
    assert!(closed_pin(&b, 999, false).is_err());
}

// Chain: #1679 A1 § 5 `status --closed [--page N]`. Risk: `--closed` eats `--page` as its value (before: `unused
// positional arguments: 1`), so the tray Completed view never loads.
#[test]
fn status_closed_is_a_flag_and_pages() {
    let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    for v in [
        &["status", "--closed", "--page", "1"][..],
        &["status", "--closed"],
        &["status", "--page", "2", "--closed", "--since", "5"],
    ] {
        let a = parse(v).unwrap();
        assert!(
            a.flags.contains_key("closed") && a.pos == ["status"],
            "{v:?}"
        );
    }
    assert_eq!(
        parse(&["status", "--closed", "--page", "2"])
            .unwrap()
            .one("page"),
        Some("2")
    );
    let c = Ctx {
        cfg: cfg(),
        raw: json!({}),
        board: Box::new(Fake::default()),
    };
    for v in [
        &["status", "--closed", "--page", "1"][..],
        &["status", "--closed"],
    ] {
        command(&c, "status", &parse(v).unwrap()).unwrap();
    }
}

// Chain: #1679 A3/A4 MP14 Merged. Risk: a closed issue with a PR link, a direct fast-forward or an edited close
// reads as a confirmed merge; only a PR/MR merge receipt on a `close` entry is evidence.
#[test]
fn completed_merge_evidence_comes_only_from_a_close_receipt() {
    let b = Fake::default();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let bodies = [
        format!(
            "taskq: close attempt 9
Changed: x
delivery auto-merge r0: merged #7 as {sha}; merge-result CI green"
        ),
        "taskq: close
delivery auto-merge r0: merged !3"
            .to_string(),
        "taskq: result abc attempt 9
Changed: https://github.com/o/r/pull/7"
            .to_string(),
        format!(
            "taskq: close
delivery direct r0: main fast-forwarded to {sha}"
        ),
        "taskq: requeue
delivery auto-merge r0: merged #7 as x"
            .to_string(),
    ];
    for (k, body) in bodies.iter().enumerate() {
        let n = k as u64 + 1;
        b.create(&format!("t{n}"), "", &["taskq".into()]).unwrap();
        b.comment(n, body).unwrap();
        b.close(n).unwrap();
    }
    b.create("t6", "", &["taskq".into()]).unwrap();
    b.raw(6, &bodies[0], true, true);
    b.close(6).unwrap();
    let page = closed_history(&b, 1, None, false).unwrap();
    let merged = |n: u64| {
        page["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["n"] == n)
            .unwrap()["merged"]
            .clone()
    };
    assert_eq!(
        merged(1),
        json!(format!("merged #7 as {sha}; merge-result CI green"))
    );
    assert_eq!(merged(2), json!("merged !3"));
    for n in 3..=6 {
        assert_eq!(
            merged(n),
            Value::Null,
            "#{n}: no merge evidence stays unknown"
        );
    }
}

// Chain: #1800 § 7 close outcome -> `status --closed` `close`. Risk: a close for a successor or with unknown
// acceptance items reads as delivered work (pilotq#90 closed for #104 while #104 stayed held).
#[test]
fn completed_names_the_close_outcome() {
    let b = Fake::default();
    let succ = b.create("successor", "", &["q-held".into()]).unwrap();
    b.comment(succ, &entry("hold", &[], None, "")).unwrap();
    let bodies = [
        format!(
            "taskq: close\nChanged: none; closes as superseded by #{succ}\nChecks: A1 unknown, A2 unknown"
        ),
        "taskq: close attempt 9\nChanged: x\nChecks: A1 ok, A2 unknown, A3 gap, A4 unknown".into(),
        "taskq: close\nChanged: none; duplicate".into(),
    ];
    let ns: Vec<u64> = bodies
        .iter()
        .map(|body| {
            let n = b.create("t", "", &["taskq".into()]).unwrap();
            b.comment(n, body).unwrap();
            b.close(n).unwrap();
            n
        })
        .collect();
    let bare = b.create("t", "", &["taskq".into()]).unwrap();
    b.close(bare).unwrap();
    let page = closed_history(&b, 1, None, false).unwrap();
    let close = |n: u64| {
        page["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["n"] == n)
            .unwrap()["close"]
            .clone()
    };
    assert_eq!(
        [close(ns[0]), close(ns[1]), close(ns[2]), close(bare)],
        [
            json!(format!("replaced by #{succ} (held)")),
            json!("delivered, 3 items unverified"),
            json!("no change"),
            json!("closed on the board")
        ]
    );
}
fn project_config(root: &Path) -> Res<(Config, Value)> {
    project_config_value(root, project_value(root)?)
}
pub(crate) fn wake_context(
    path: &Path,
    seen: &mut Option<String>,
) -> Res<Option<coreq::telemetry::capture::TraceContext>> {
    wake_read(path, seen, || Ok(())).map(|(link, ())| link)
}
/// Test reader of a [`zip_one`] archive: the one file's name and bytes, after checking its CRC and the end record.
pub(crate) fn unzip_one(zip: &[u8]) -> (String, Vec<u8>) {
    let u16_at = |i: usize| u16::from_le_bytes([zip[i], zip[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes(zip[i..i + 4].try_into().unwrap());
    assert_eq!(
        (u32_at(0), u16_at(8)),
        (0x0403_4b50, 8),
        "local header, deflate"
    );
    let (packed, name_len) = (u32_at(18) as usize, u16_at(26));
    let name = String::from_utf8(zip[30..30 + name_len].to_vec()).unwrap();
    let mut data = vec![];
    std::io::Read::read_to_end(
        &mut flate2::read::DeflateDecoder::new(&zip[30 + name_len..30 + name_len + packed]),
        &mut data,
    )
    .unwrap();
    let mut crc = flate2::Crc::new();
    crc.update(&data);
    assert_eq!(
        (crc.sum(), u32_at(22) as usize),
        (u32_at(14), data.len()),
        "CRC and size"
    );
    let end = zip.len() - 22;
    assert_eq!(
        (u32_at(end), u16_at(end + 10), u32_at(end + 16) as usize),
        (0x0605_4b50, 1, 30 + name_len + packed),
        "end record"
    );
    assert_eq!(u32_at(30 + name_len + packed), 0x0201_4b50, "central entry");
    (name, data)
}
fn report(b: &dyn Board, cfg: &Config, raw: &Value, only: &[String]) -> Res<String> {
    snapshot::read(b, cfg, raw, only).text(cfg, raw)
}
pub(crate) fn with_test_capture<T>(run: impl FnOnce(&coreq::telemetry::Store, &Path) -> T) -> T {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use tracing_subscriber::prelude::*;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "taskq-command-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
    let capture =
        coreq::telemetry::capture::Capture::new(store.clone(), "taskq", "fixture").unwrap();
    let result = tracing::subscriber::with_default(
        tracing_subscriber::registry().with(capture.clone()),
        || run(&store, &dir),
    );
    telemetry::settle(Duration::from_secs(30));
    capture.flush().unwrap();
    drop(capture);
    drop(store);
    // #1640: Windows can keep a just-closed store file busy (os error 32) for a moment; a lasting hold still fails.
    // #1939: Linux CI saw a store file appear during removal (DirectoryNotEmpty); retry it the same way.
    for left in (0..50).rev() {
        match fs::remove_dir_all(&dir) {
            Err(e)
                if left > 0
                    && (e.raw_os_error() == Some(32)
                        || e.kind() == std::io::ErrorKind::DirectoryNotEmpty) =>
            {
                std::thread::sleep(Duration::from_millis(100))
            }
            other => {
                other.unwrap();
                break;
            }
        }
    }
    result
}

// Chain: § 6 fresh request -> refusal -> JSON (#2450). Risk: cached healthy rows hide a failed refresh.
// Keep the published result and age, alongside the exact refusal that prevented a fresh check (R12).
#[test]
fn doctor_fresh_json_keeps_request_failure_beside_published_result() {
    use super::*;
    let b = board::replying_to(&board::github::KIND, |method, _, _| {
        assert_eq!(method, "PATCH");
        Err("HTTP 403 request refused".into())
    });
    let host = snapshot::Host {
        name: "mac".into(),
        version: "4.6.87".into(),
        updated: 9990,
        doctor: Some("9990 ok".into()),
        ..Default::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let at = health::Scope {
        repo: "o/r",
        gitlab: false,
        ttl: 300,
        now: 10_000,
    };
    let pace = health::Pace {
        wait: 300,
        poll: 20,
        elapsed: &|| 0,
        sleep: &|_| panic!("failed request must not wait"),
    };
    let request = board::Issue {
        number: 42,
        title: health::REQUEST_TITLE.into(),
        trusted: true,
        ..Default::default()
    };
    health::ask(b.as_ref(), &[request], vec![host], &at, &pace, &mut |e| {
        tx.send((0, e)).unwrap();
    });
    drop(tx);
    let mut gathered = vec![health::Gathered::new("o/r")];
    health::gather(
        &rx,
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        &mut gathered,
        &mut |_| {},
    );
    let out = doctor_hosts_board_json(&gathered[0], None, &Ok(vec![]));
    assert_eq!(out["hosts"][0]["state"], "ok", "retain the published check");
    assert_eq!(out["hosts"][0]["age"], "<1m");
    assert_eq!(
        out["notes"],
        json!(["request not posted: HTTP 403 request refused"]),
        "fresh request failure must reach JSON"
    );
}

// §5/A3: visible defaults must never be reported as a caller placement request.
#[test]
fn add_placement_explains_defaults_and_requested_pins() {
    for (args, host, source, runtime) in [
        (
            vec!["add", "T"],
            Some("mac"),
            "host default (claim labelled)",
            None,
        ),
        (vec!["add", "T"], None, "unpinned", None),
        (
            vec!["add", "T", "--host", "win", "--runtime", "codex"],
            Some("win"),
            "requested",
            Some("codex"),
        ),
    ] {
        let a = Args::parse(&args.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
        let t = NewTask {
            host: host.map(String::from),
            runtime: runtime.map(String::from),
            ..Default::default()
        };
        let p = add_placement(&a, &t);
        assert_eq!(p["host_source"], source);
        assert_eq!(
            p["runtime_source"],
            if runtime.is_some() {
                "requested"
            } else {
                "admission default"
            }
        );
        assert_eq!(p["host"], json!(host));
        assert_eq!(p["runtime"], json!(runtime));
    }
}
