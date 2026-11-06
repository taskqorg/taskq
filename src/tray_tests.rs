use super::*;

// Queue handoffs wait on a worker thread; a loaded instrumented Windows runner took over 3 s (merge_group run 41538580169).
// Only the failure path waits this long.
const WAIT: Duration = Duration::from_secs(30);

// M13 / #1966 A2: a new window must not consume the first control click just to activate the app.
// P-W3/C2/R12: recreation failure cannot destroy the singleton or retire its active generation.
#[test]
fn workspace_recreation_retains_identity_and_reader_on_failure() {
    use std::{cell::Cell, rc::Rc};
    struct View(Rc<Cell<usize>>);
    impl Drop for View {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for surface in ["main", "invented-detached"] {
        let window = 42;
        let dropped = Rc::new(Cell::new(0));
        let mut view = View(dropped.clone());
        let mut calls = WebCalls::new();
        calls.surface = surface.into();
        calls.workspace = true;
        calls.failed = true;
        let generation = calls.generation;
        assert!(
            recreate_workspace(&window, &mut view, &mut calls, true, |w, next| {
                assert_eq!(*w, 42);
                assert_ne!(next.generation, generation);
                Err("Invented native creation refusal".into())
            })
            .is_err()
        );
        assert_eq!(window, 42);
        assert_eq!(
            dropped.get(),
            0,
            "failed recreation must retain the old view"
        );
        assert_eq!(calls.generation, generation);
        assert!(calls.failed);
        recreate_workspace(&window, &mut view, &mut calls, true, |w, next| {
            assert_eq!(*w, 42);
            assert!(next.safe);
            assert_eq!(next.surface, surface);
            Ok(View(dropped.clone()))
        })
        .unwrap();
        assert_eq!(window, 42);
        assert_eq!(dropped.get(), 1);
        assert_ne!(calls.generation, generation);
        assert!(!calls.failed);
        assert!(
            !calls.recovery_ready,
            "new reader still needs its ordered handoff"
        );
    }
}

// R9/R12: one unavailable runtime must not hide another; stale readiness never becomes available.
#[test]
fn owner_choices_keep_separate_fresh_readiness_without_fallback() {
    let projects = vec![
        json!({"agents":{"at":now(),"rows":[{"runtime":"claude","login":"not logged in"},{"runtime":"codex","login":"logged in"}]}}),
    ];
    let offered = owner_runtime(&projects);
    assert_eq!(offered["id"], crate::runtime::OWNER);
    assert_eq!(
        offered["available"], false,
        "default never silently switches"
    );
    let codex = offered["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "codex")
        .unwrap();
    assert_eq!(codex["available"], cfg!(any(target_os = "macos", windows)));
    for at in [now() - 361, now() + 10] {
        let unknown = owner_runtime(&[
            json!({"agents":{"at":at,"rows":[{"runtime":"codex","login":"logged in"}]}}),
        ]);
        assert!(
            unknown["choices"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["available"] == false && !r["reason"].as_str().unwrap().is_empty())
        );
    }
}

#[test]
fn every_webview_accepts_first_mouse() {
    fn check(dir: &Path) -> usize {
        let mut count = 0;
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                count += check(&path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let source: String = fs::read_to_string(&path)
                    .unwrap()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                for builder in source.split(concat!("WebView", "Builder::")).skip(1) {
                    let chain = builder.split(".build(").next().unwrap();
                    assert!(
                        chain.contains(concat!(".with_accept", "_first_mouse(true)")),
                        "{}: WebView must accept the first mouse click",
                        path.display()
                    );
                    count += 1;
                }
            }
        }
        count
    }
    assert!(check(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) > 0);
}

// M15: Quit and Restart name the doing and review work they wait for within the 32-character menu label, and app
// text never says service; a second tray on the host never takes the lock. Risk: Quit or Restart hides active
// work, the app shows itself as a service, or two trays run across install folders.
#[test]
fn quit_names_active_work_and_host_lock_is_single() {
    let st = |ns: &[(u64, &str)]| json!({"projects": [{"name": "p", "tasks": ns.iter().map(|(n, s)| json!({"n": n, "state": s})).collect::<Vec<_>>()}]});
    assert_eq!(quit_label(&st(&[]), None), "No active work");
    assert_eq!(
        quit_label(&st(&[(12, "doing"), (13, "ready"), (14, "review")]), None),
        "Quit waits for p#12 p#14"
    );
    let many = quit_label(
        &st(&[
            (1001, "doing"),
            (1002, "doing"),
            (1003, "review"),
            (1004, "doing"),
        ]),
        Some("Quitting"),
    );
    assert_eq!(many, "Quitting after p#1001 +3");
    assert!(many.len() <= 32);
    let restart = quit_label(
        &st(&[
            (1001, "doing"),
            (1002, "doing"),
            (1003, "review"),
            (1004, "doing"),
        ]),
        Some("Restarting"),
    );
    assert_eq!(restart, "Restarting after p#1001 +3");
    for phase in ["Quitting", "Restarting"] {
        assert_eq!(quit_label(&st(&[]), Some(phase)), format!("{phase} TaskQ"));
    }
    if let Some(held) = host_lock() {
        assert!(
            host_lock().is_none(),
            "a second tray on this host is refused"
        );
        drop(held);
    }
}

// A1/C1: `|_|false` blocked the inline page itself and left the window blank.
// #1391 A2/A4: every icon style keeps all original counters/warnings; the absence mark clears independently.
#[test]
fn agentq_badge_preserves_icon_pixels_and_snapshot_states() {
    let image = compact(true, true, 2.0);
    let original = image.px.clone();
    let width = image.w;
    assert_eq!(agentq_mark(compact(true, true, 2.0), false).px, original);
    let marked = agentq_mark(image, true);
    for y in 0..marked.h as usize {
        assert_eq!(
            &marked.px[y * marked.w as usize * 4..y * marked.w as usize * 4 + width as usize * 4],
            &original[y * width as usize * 4..(y + 1) * width as usize * 4]
        );
    }
    for (route, kind, warning) in [
        ("agentq-shown", "shown", true),
        ("agentq-setting-up", "setting-up", true),
        ("agentq-running", "running", false),
        ("agentq-unknown", "unknown", true),
    ] {
        let f = agentq_fixture(route).unwrap();
        assert_eq!(f["kind"], kind);
        assert_eq!(f["warning"], warning);
    }
}

// #1679 A2: the exact Completed read parses as `status --closed` with its page; before, `--closed` ate `--page`.
#[test]
fn history_read_is_a_parsable_status_closed_page() {
    for (m, page, since) in [
        (json!({}), "1", None),
        (json!({"page":0}), "1", None),
        (json!({"page":3,"since":42}), "3", Some("42")),
    ] {
        let a = crate::cli::Args::parse(&history_args(&m)).unwrap();
        assert_eq!(
            (
                a.pos.as_slice(),
                a.flags.contains_key("closed"),
                a.one("page"),
                a.one("since")
            ),
            (&["status".to_string()][..], true, Some(page), since),
            "{m}"
        );
    }
    let a = crate::cli::Args::parse(&history_args(
        &json!({"task":7,"transcript":true,"before":80}),
    ))
    .unwrap();
    assert_eq!(
        (
            a.one("task"),
            a.flags.contains_key("transcript"),
            a.one("before")
        ),
        (Some("7"), true, Some("80")),
        "#1829: exact native read retains task and byte cursor; no session-open command"
    );
    // A closed thread's background read names the board's exact task and asks for no worker log.
    let a = crate::cli::Args::parse(&history_args(&json!({"task":7,"board":true}))).unwrap();
    assert_eq!(
        (
            a.pos.as_slice(),
            a.flags.contains_key("closed"),
            a.one("task"),
            a.flags.contains_key("transcript")
        ),
        (&["status".to_string()][..], true, Some("7"), false)
    );
}

#[test]
fn page_fence_loads_one_inline_document_and_bridges_only_it() {
    use std::sync::atomic::AtomicBool;
    // #2365 A6: the first inline load passes on either platform's form of it.
    for first in ["about:blank", "data:text/html;base64,AA"] {
        let armed = AtomicBool::new(true);
        assert!(navigation_allowed(&armed, first));
        // Nothing after it: not a remote page, a file, or another inline document.
        for later in [
            "about:blank",
            "data:text/html,x",
            "https://example.com",
            "http://example.com",
            "file:///C:/x.html",
            "HTTPS://example.com",
        ] {
            assert!(!navigation_allowed(&armed, later), "{later}");
        }
    }
    for first in [
        "https://example.com",
        "http://example.com",
        "file:///C:/x.html",
        "taskq://x",
    ] {
        let armed = AtomicBool::new(true);
        assert!(!navigation_allowed(&armed, first), "{first}");
        assert!(
            navigation_allowed(&armed, "about:blank"),
            "a refused page does not consume the inline load"
        );
    }
    let message = |uri: &str| {
        bridge_message(
            wry::http::Request::builder()
                .uri(uri)
                .body("x".to_string())
                .unwrap(),
        )
    };
    assert_eq!(message("about:blank").as_deref(), Some("x"));
    for source in [
        "https://example.com/",
        "http://example.com/",
        "file://host/x.html",
        "HTTPS://example.com/",
    ] {
        assert_eq!(message(source), None, "{source}");
    }
    // wry drops a source it cannot parse before the handler, which covers a local file document.
    assert!(
        wry::http::Request::builder()
            .uri("file:///C:/x.html")
            .body(String::new())
            .is_err()
    );
}

// #2365 A3/A5: boot and the next 5 s decide whether a bundle started; a page behind a live window does not fault early.
#[test]
fn start_fault_covers_boot_and_proof_time_only() {
    let mut calls = WebCalls::new();
    calls.fault("script-error");
    assert_eq!(
        calls.start_fault,
        Some("script-error"),
        "a fault before boot is a failed start"
    );
    let mut calls = WebCalls::new();
    calls.finish(0, "ok");
    assert!(calls.booted.is_some());
    calls.fault("page-error");
    assert_eq!(
        calls.start_fault,
        Some("page-error"),
        "a fault right after boot is a failed start"
    );
    let mut calls = WebCalls::new();
    calls.finish(0, "ok");
    calls.booted = Some(Instant::now() - START_PROOF);
    calls.fault("page-error");
    assert_eq!(
        calls.start_fault, None,
        "a proven page keeps the ordinary fault path"
    );
    let mut calls = WebCalls::new();
    calls.fault("page-not-ready");
    assert_eq!(
        calls.start_fault, None,
        "a push before boot is the tray's mistake, not the bundle's"
    );
}

// #1464 A4/A5/A7: native drafts survive restart/corruption and never replay owner/task effects.
#[test]
fn workspace_local_preserves_drafts_and_unknown_identity_without_replay() {
    let base =
        std::env::temp_dir().join(format!("taskq-workspace-local-{}", crate::runtime::uuid()));
    fs::create_dir_all(&base).unwrap();
    let local = json!({"draft":"kept literal answer","bound":12,"op":{"id":"same-op","task":42,"revision":12,"text":"kept literal answer"},"receipt":{"state":"unknown"},"caret":[2,4]});
    answer_local(&base, "https://example.org/issues#42", &local).unwrap();
    ai(&base,"ai-draft",&json!({"text":"native request","root":"registered","local":{"text":"native request","caret":[1,5]}})).unwrap();
    let observed = requests_file_checked(&base).unwrap();
    assert_eq!(observed["answers"]["https://example.org/issues#42"], local);
    assert_eq!(observed["local"]["caret"], json!([1, 5]));
    assert!(
        crate::setup::all(&base).unwrap().0.is_empty(),
        "view persistence must not replay effects"
    );
    let large = json!({"draft":"word ".repeat(1_700_000),"op":{"text":"word ".repeat(1_700_000)}});
    answer_local(&base, "large-draft", &large).unwrap();
    crate::conversations::navigation(&base, &json!({"search":"after a large reply"})).unwrap();
    answer_local(&base, "another-draft", &local).unwrap();
    assert_eq!(
        requests_file_checked(&base).unwrap()["answers"]["large-draft"],
        large,
        "§ 5: a large reply is saved without a length cap"
    );
    let bytes = fs::read(base.join("tray-requests.json")).unwrap();
    assert!(
        answer_local(&base, "x", &json!({"draft":7})).is_err(),
        "invalid draft types are still refused"
    );
    assert!(
        answer_local(
            &base,
            "x",
            &json!({"draft":"ok","extra":"x".repeat(8*1024*1024)})
        )
        .is_err(),
        "non-reply metadata retains its 8 MiB budget"
    );
    assert_eq!(fs::read(base.join("tray-requests.json")).unwrap(), bytes);
    fs::write(base.join("tray-requests.json"), "broken kept bytes").unwrap();
    assert!(ai(&base, "ai-draft", &json!({"text":"replacement"})).is_err());
    assert!(answer_local(&base, "x", &local).is_err());
    assert_eq!(
        fs::read_to_string(base.join("tray-requests.json")).unwrap(),
        "broken kept bytes"
    );
    assert!(
        requests_file(&base)["error"]
            .as_str()
            .unwrap()
            .contains("existing data kept")
    );
    fs::remove_dir_all(base).unwrap();
}

// A2/M7: Send opens one owner session in the chosen registered project. Risk: an unregistered path or empty
// text launches a session, or a refused send drops the draft.
#[test]
fn ai_send_opens_one_session_in_a_registered_project() {
    let base = std::env::temp_dir().join(format!("taskq-tray-ai-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let install = base.join("install");
    let root = base.join("project");
    let other = base.join("other");
    for d in [&install, &root, &other] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
    let r = root.to_str().unwrap();
    ai(&install, "ai-draft", &json!({"text":"draft"})).unwrap();
    assert!(
        ai(
            &install,
            "ai-send",
            &json!({"root":other.to_str().unwrap(),"text":"why"})
        )
        .is_err()
    );
    assert!(ai(&install, "ai-send", &json!({"root":r,"text":"  "})).is_err());
    assert_eq!(
        requests_file(&install)["draft"],
        "draft",
        "refused send lost its draft"
    );
    assert!(crate::setup::last(&install, true).is_null());
    ai(
        &install,
        "ai-send",
        &json!({"id":"owner-request-1464","runtime":"claude","root":r,"text":" why "}),
    )
    .unwrap();
    ai(
        &install,
        "ai-send",
        &json!({"id":"owner-request-1464","runtime":"claude","root":r,"text":" why "}),
    )
    .unwrap();
    assert_eq!(
        crate::setup::all(&install).unwrap().0.len(),
        1,
        "same ID retry must not launch twice"
    );
    assert!(
        ai(
            &install,
            "ai-send",
            &json!({"id":"owner-request-1464","runtime":"claude","root":r,"text":"changed"})
        )
        .is_err()
    );
    let req = crate::setup::last(&install, true);
    assert_eq!(req["state"], "pending");
    assert_eq!(
        crate::setup::all(&install).unwrap().0[0].root.as_deref(),
        Some(root.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        (
            requests_file(&install)["draft"].clone(),
            requests_file(&install)["history"].clone()
        ),
        (json!(""), json!(["why"]))
    );
    let _ = fs::remove_dir_all(base);
}

// Chain: native delivery -> exact JS outcome or unknown -> bounded safe shared observations.
#[test]
fn web_execution_requires_acknowledgment_and_bounds_error_reports() {
    crate::tests::with_test_capture(|store, _| {
        let now = Instant::now();
        let mut calls = WebCalls::new();
        calls.finish(0, "ok");
        for (name, result) in [
            ("tray.web.success", "ok"),
            ("tray.web.exception", "script-error"),
            ("tray.web.malformed", "invalid-ack"),
        ] {
            let id = calls.begin(name, now).unwrap();
            calls.finish(id, result);
            calls.finish(id, "ok");
        }
        let expired = calls.begin("tray.web.expired", now).unwrap();
        calls.seen = now;
        for s in 1..=29 {
            calls.expire(now + Duration::from_secs(s));
        }
        assert!(calls.pending.contains_key(&expired));
        calls.expire(now + Duration::from_secs(30));
        assert!(calls.pending.is_empty());
        calls.finish(expired, "ok");
        for _ in 0..64 {
            assert!(calls.begin("tray.web.pending", now).is_some());
        }
        assert!(calls.begin("tray.web.overflow", now).is_none());
        assert_eq!(calls.pending.len(), 64);
        for counts in [
            json!([1, 0, 0, 0]),
            json!([101, 1, 1, 1]),
            json!([101, 1, 1, 1]),
        ] {
            calls.errors(&json!({"counts":counts,"saturated":false}));
        }
        for counts in [
            json!([1, 1, 1, 1]),
            json!([-1, 0, 0, 0]),
            json!([4294967296_u64, 1, 1, 1]),
            json!([101]),
            json!(null),
        ] {
            calls.errors(&json!({"counts":counts,"saturated":false}));
            assert_eq!(calls.counts, [101, 1, 1, 1]);
        }
        calls.errors(&json!({"counts":[101,1,1,1],"saturated":true}));
        calls.errors(&json!({"counts":[101,1,1,1],"saturated":true}));
        assert!(calls.failed && calls.saturated);
        drop(calls);
        tracing::dispatcher::get_default(|d| {
            d.downcast_ref::<coreq::telemetry::capture::Capture>()
                .unwrap()
                .flush()
                .unwrap()
        });
        let page = store.export(0, 0, 1024).unwrap();
        let rows: Vec<_> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["observation"])
            .filter(|r| r["kind"] == "method")
            .collect();
        for (name, result) in [
            ("success", "ok"),
            ("exception", "failed"),
            ("malformed", "unknown"),
            ("expired", "unknown"),
        ] {
            let found: Vec<_> = rows
                .iter()
                .filter(|r| r["operation"] == format!("taskq.operation.tray.web.{name}"))
                .collect();
            assert_eq!(found.len(), 1, "duplicate/late callback changed {name}");
            assert_eq!(found[0]["outcome"], result);
        }
        assert_eq!(
            rows.iter()
                .filter(|r| r["operation"] == "taskq.operation.tray.web.pending"
                    && r["outcome"] == "unknown")
                .count(),
            64
        );
        let errors: Vec<_> = rows
            .iter()
            .filter(|r| r["operation"] == "taskq.operation.tray.web.error")
            .collect();
        assert_eq!(
            rows.iter()
                .filter(
                    |r| r["operation"] == "taskq.operation.tray.web.error-counts"
                        && r["error"] == "count-saturated"
                )
                .count(),
            1
        );
        assert_eq!(errors.len(), 5);
        assert_eq!(
            errors
                .iter()
                .map(|r| r["calls"].as_u64().unwrap())
                .sum::<u64>(),
            104
        );
        assert!(errors.iter().all(|r| r["outcome"] == "failed"));
        assert_eq!(web_result("true"), "invalid-ack");
        assert_eq!(web_result("\"ok\""), "ok");
    });
}

// #1755 A1: a slow refresh stalls the native loop and WebView2 callbacks queue behind it; the stall is not the page's
// ack-timeout, and a tick push waits for the previous one. Risk: a 28 s refresh under load fences the whole window
// as Interface error, or queued ticks stack evaluations to the pending limit.
#[test]
fn loop_stall_is_not_an_ack_timeout_and_tick_pushes_do_not_stack() {
    let mut calls = WebCalls::new();
    calls.finish(0, "ok");
    let now = calls.seen;
    assert!(calls.idle());
    let tick = calls.begin(TICK, now).unwrap();
    calls.tick = Some(tick);
    assert!(!calls.idle(), "a tick push waits for the previous one");
    calls.expire(now + Duration::from_secs(2));
    calls.expire(now + Duration::from_secs(60));
    calls.expire(now + Duration::from_secs(61));
    assert!(
        calls.pending.contains_key(&tick) && !calls.failed,
        "the loop stall counted against the page"
    );
    calls.finish(tick, "ok");
    assert!(calls.idle() && !calls.failed);
    let lost = calls.begin(TICK, now + Duration::from_secs(61)).unwrap();
    for s in 62..=91 {
        calls.expire(now + Duration::from_secs(s));
    }
    assert!(
        !calls.pending.contains_key(&lost) && calls.failed,
        "a responsive loop still times out a lost callback"
    );
}

// #1755 A3: a tray whose startup writer election failed retries off the UI loop; once the writer is free, a page
// error report commits as tray.web.error. Risk: one failed election leaves tray faults out of the store all lifetime.
#[test]
fn tray_collector_recovers_after_failed_election() {
    use std::process::{Command, Stdio};
    let d = std::env::temp_dir().join(format!("taskq-tray-capture-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let spawn = |mode| {
        Child(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tray::tests::tray_capture_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("TASKQ_CAPTURE_FIXTURE", &d)
                .env("TASKQ_CAPTURE_MODE", mode)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        )
    };
    let mut writer = spawn("writer");
    let until = Instant::now() + Duration::from_secs(30);
    while !d.join("ready").exists() {
        assert!(
            writer.0.try_wait().unwrap().is_none(),
            "refusing writer failed before readiness"
        );
        assert!(Instant::now() < until, "refusing writer did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut tray = spawn("tray");
    let status = loop {
        if let Some(status) = tray.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < until, "tray collector did not recover");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "tray collector did not retry and commit");
    assert!(writer.0.wait().unwrap().success());
    let reader =
        coreq::telemetry::Reader::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
    let rows = reader.export(0, 0, 1024).unwrap().to_string();
    assert!(
        rows.contains("taskq.operation.tray.web.error"),
        "page error report missing after recovery"
    );
    drop(reader);
    fs::remove_dir_all(d).unwrap();
}

#[test]
#[ignore = "owned refusing-writer/late-tray process fixture"]
fn tray_capture_process() {
    let d = PathBuf::from(std::env::var_os("TASKQ_CAPTURE_FIXTURE").unwrap());
    if std::env::var("TASKQ_CAPTURE_MODE").unwrap() == "writer" {
        let _runtime = crate::telemetry::install_capture(&d).unwrap();
        // Keep the writer alive but refuse every other election, as an unanswering owner does.
        let endpoint = d.join("telemetry.log/telemetry-ipc/endpoint");
        let mut value: Value = serde_json::from_slice(&fs::read(&endpoint).unwrap()).unwrap();
        value["wire"] = json!(1);
        fs::write(endpoint, serde_json::to_vec(&value).unwrap()).unwrap();
        fs::write(d.join("ready"), "").unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        while !d.join("release").exists() {
            assert!(Instant::now() < until, "tray never failed its election");
            std::thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    let (send, lines) = std::sync::mpsc::channel::<String>();
    crate::telemetry::keep_capture(d.clone(), Duration::from_millis(200), move |line| {
        let _ = send.send(line.to_string());
    });
    let first = lines.recv_timeout(Duration::from_secs(15)).unwrap();
    assert!(
        first.starts_with("Telemetry: unknown (") && first.ends_with("; retrying every 1s"),
        "{first}"
    );
    fs::write(d.join("release"), "").unwrap();
    assert_eq!(
        lines.recv_timeout(Duration::from_secs(20)).as_deref(),
        Ok("Telemetry: collecting"),
        "one line per state change"
    );
    WebCalls::new().errors(&json!({"counts":[1,0,0,0],"saturated":false}));
    coreq::telemetry::capture::qualify().unwrap();
}

// #1680 A3: route and page errors log an identifier-only class and site; text, URLs or paths are dropped and a
// failing route leaves the window usable. Risk: stderr carries page content, or a route error trips the fence.
#[test]
fn web_error_site_is_identifiers_only_and_route_errors_keep_the_window() {
    assert_eq!(
        web_site(&json!({"error":"TypeError","site":"title:1:7034"})),
        Some(("TypeError", "title:1:7034"))
    );
    assert_eq!(
        web_site(&json!({"error":"TypeError","site":"anonymous"})),
        Some(("TypeError", "anonymous"))
    );
    for bad in [
        json!({"error":"TypeError: secret","site":"x:1:2"}),
        json!({"error":"TypeError","site":"/private/x.js:1:2"}),
        json!({"error":"TypeError","site":"x:1"}),
        json!({"error":"TypeError","site":"x:a:2"}),
        json!({"site":"x:1:2"}),
        json!({"error":"E","site":""}),
    ] {
        assert_eq!(web_site(&bad), None, "{bad}");
    }
    let mut calls = WebCalls::new();
    calls.route(&json!({"a":"route-error","route":"conversations","error":"TypeError","site":"title:1:7034"}));
    calls.route(
        &json!({"a":"route-error","route":"../x","error":"TypeError","site":"title:1:7034"}),
    );
    assert!(!calls.failed, "a failing route must not fault the window");
    for _ in 0..20 {
        calls.route(&json!({"route":"messages","error":"TypeError","site":"x"}));
    }
    assert_eq!(calls.logged, 8, "stderr lines stay bounded");
    calls.errors(
        &json!({"counts":[1,0,0,0],"saturated":false,"error":"TypeError","site":"title:1:2"}),
    );
    assert!(
        calls.failed,
        "page-wide errors keep the unknown-outcome fence"
    );
}

// Chain: bounded UI submission -> owned worker -> same-trace receipt -> stale registration refusal.
#[test]
fn queued_focus_actions_keep_context_and_refuse_overflow_without_blocking() {
    crate::tests::with_test_capture(|store, dir| {
        let root = dir.join("project");
        let install = dir.join("install");
        fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(crate::paths::dot(&root).join("focus"), "area-maps").unwrap();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let queue = ActionQueue::start(
            install.clone(),
            &["pause", "resume", "focus"],
            true,
            queued_focus,
            move |root, id, result| {
                result_tx.send((root, id, result)).unwrap();
                release_rx.recv_timeout(WAIT).unwrap();
            },
        )
        .unwrap();
        let message = |action: &str, id: &str| json!({"a":action,"root":root,"id":id});
        let _click = tracing::info_span!("fixture.click").entered();
        for invalid in [String::new(), "../outside".into(), "x".repeat(65)] {
            assert!(
                queue
                    .submit(&message("pause", &invalid))
                    .unwrap_err()
                    .contains("correlation id")
            );
            assert_eq!(crate::data::focus(&root).unwrap(), ["area-maps"]);
        }
        queue.submit(&message("pause", "tray.1")).unwrap();
        let (reported, id, result) = result_rx.recv_timeout(WAIT).unwrap();
        assert_eq!(reported, root);
        assert_eq!(id, "tray.1");
        let applied = result.unwrap()["applied_receipt"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            queue.submit(&message("pause", "tray.1")).is_ok(),
            "the same pending operation coalesces"
        );
        assert!(
            queue
                .submit(&message("resume", "tray.1"))
                .unwrap_err()
                .contains("different request"),
            "an id cannot be repurposed while pending"
        );
        let snapshot = crate::control::snapshot(&root, &mut None, &mut None).unwrap();
        assert_eq!(snapshot.command["applied_receipt"], applied);
        let status = json!({"focus_command":snapshot.command,"focus":snapshot.focus,"tasks":[],"complete":true});
        fs::write(
            crate::paths::dot(&root).join("status.json"),
            status.to_string(),
        )
        .unwrap();
        assert_eq!(
            project(&install, root.to_str().unwrap())["focus_command"],
            status["focus_command"]
        );
        for n in 0..16 {
            queue
                .submit(&message("resume", &format!("queued-{n}")))
                .unwrap();
        }
        assert!(
            queue
                .submit(&message("resume", "overflow"))
                .unwrap_err()
                .contains("queue full")
        );
        assert!(crate::data::focus(&root).unwrap().is_empty());
        // Queued intent cannot act on a project removed after submission.
        fs::write(install.join("projects"), "").unwrap();
        for n in 0..16 {
            release_tx.send(()).unwrap();
            let (reported, id, result) = result_rx.recv_timeout(WAIT).unwrap();
            assert_eq!(reported, root);
            assert_eq!(id, format!("queued-{n}"));
            assert!(result.unwrap_err().contains("no longer registered"));
        }
        release_tx.send(()).unwrap();
        let ActionQueue { sender, worker, .. } = queue;
        drop(sender);
        worker.join().unwrap();
        assert!(crate::data::focus(&root).unwrap().is_empty());
        assert!(crate::telemetry::settle(Duration::from_secs(30)));
        let page = store.export(0, 0, 1024).unwrap();
        let rows: Vec<_> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["observation"])
            .collect();
        let receipts: Vec<_> = rows
            .iter()
            .copied()
            .filter(|r| r["source"] == "command")
            .collect();
        assert_eq!(
            receipts.len(),
            3,
            "refused queued actions mutated the project"
        );
        let click = rows
            .iter()
            .find(|r| r["operation"] == "taskq.fixture.click")
            .unwrap();
        assert_eq!(receipts[0]["trace"], click["trace"]);
        assert_eq!(receipts[1]["trace"], click["trace"]);
        assert_eq!(receipts[1]["kind"], "command.applied");
        assert_eq!(receipts[2]["kind"], "command.observed");
    });
}

// #2167: a reviewed Apply from the shared Workspace review carries the native readerGeneration stamp.
// Risk: the stamp reaches the strict capacity payload and every valid Apply reads "Unsupported host capacity payload".
#[test]
fn workspace_capacity_apply_drops_reader_stamp_and_keeps_strict_payload() {
    crate::tests::with_test_capture(|_store, dir| {
        let root = dir.join("project");
        let install = dir.join("install");
        fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        fs::create_dir_all(install.join("resources")).unwrap();
        fs::write(root.join("taskq.json"), "{\"repo\":\"fixture/project\"}").unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(
            install.join("host.json"),
            json!({"hosts":{crate::config::hostname():"capacity-host"},"limits":{"codex":2}})
                .to_string(),
        )
        .unwrap();
        let policy = crate::host::Policy::read(&install).unwrap();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let page = json!({"a":"capacity","root":root,"id":"ws-apply","issued_at":now,"expires_at":now+300,"context":"1 a b","ops":[{
                "target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":"codex"},
                "policy_schema":"taskq.host-capacity/1","expected_revision":policy.revision().unwrap(),"changes":{"runtime_limit":3}}]});
        // As the Workspace webview delivers it (workspace_view): the same page message plus its reader stamp.
        let mut stamped = page.clone();
        stamped["readerGeneration"] = json!(7);
        let before = capacity(&install, &root, &stamped).unwrap();
        assert_eq!(
            before["reason"],
            "Unsupported host capacity payload; refresh and review current settings",
            "before: raw stamp reaches the strict check"
        );
        let forwarded: Value = serde_json::from_str(&workspace_forward(&stamped).unwrap()).unwrap();
        assert!(forwarded.get("readerGeneration").is_none());
        assert_eq!(
            forwarded, page,
            "only the native stamp is dropped; reviewed id, ops and times stay exact"
        );
        // The tray queue and its report, as the native capacity worker runs them: receipt keeps the operation id.
        let (tx, rx) = std::sync::mpsc::channel();
        let queue = ActionQueue::start(
            install.clone(),
            &["capacity"],
            true,
            capacity,
            move |root, id, result| {
                let mut reply = result.unwrap();
                reply["root"] = json!(root);
                reply["id"] = json!(id);
                tx.send(reply).unwrap();
            },
        )
        .unwrap();
        queue.submit(&forwarded).unwrap();
        let saved = rx.recv_timeout(WAIT).unwrap();
        assert_eq!(saved["state"], "saved", "{saved}");
        assert_eq!(saved["id"], "ws-apply", "{saved}");
        let fresh = crate::host::Policy::read(&install).unwrap();
        assert_eq!(fresh.raw["limits"]["codex"], 3, "fresh readback");
        assert_eq!(saved["saved_revision"], json!(fresh.revision().unwrap()));
        // Lost receipt: the same reviewed operation reconciles without a second write.
        queue.submit(&forwarded).unwrap();
        let again = rx.recv_timeout(WAIT).unwrap();
        assert_eq!(again["id"], "ws-apply", "{again}");
        assert_ne!(again["state"], "rejected", "{again}");
        assert_eq!(
            crate::host::Policy::read(&install)
                .unwrap()
                .revision()
                .unwrap(),
            fresh.revision().unwrap(),
            "no duplicate write"
        );
        let ActionQueue { sender, worker, .. } = queue;
        drop(sender);
        worker.join().unwrap();
        // Unknown fields from the page, foreign actions and stale reviews still refuse.
        let mut extra = stamped.clone();
        extra["evil"] = json!(1);
        assert_eq!(
            capacity(
                &install,
                &root,
                &serde_json::from_str(&workspace_forward(&extra).unwrap()).unwrap()
            )
            .unwrap()["state"],
            "rejected"
        );
        assert!(workspace_forward(&json!({"a":"settings-top","readerGeneration":7})).is_none());
        let mut stale = forwarded.clone();
        stale["id"] = json!("ws-stale");
        assert_eq!(
            capacity(&install, &root, &stale).unwrap()["state"],
            "rejected",
            "stale expected revision refuses"
        );
    });
}

// H-A2: a blocked host-policy write cannot delay the independent restrictive Pause command.
#[test]
fn blocked_capacity_does_not_block_pause() {
    crate::tests::with_test_capture(|_store, dir| {
        let root = dir.join("project");
        let install = dir.join("install");
        fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        fs::create_dir_all(install.join("resources")).unwrap();
        fs::write(root.join("taskq.json"), "{\"repo\":\"fixture/project\"}").unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(
            install.join("host.json"),
            json!({"hosts":{crate::config::hostname():"capacity-host"},"limits":{"codex":2}})
                .to_string(),
        )
        .unwrap();
        fs::write(crate::paths::dot(&root).join("focus"), "all").unwrap();
        let policy = crate::host::Policy::read(&install).unwrap();
        assert_eq!(
            project(&install, root.to_str().unwrap())["cap_host"],
            "capacity-host"
        );
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let message = json!({"a":"capacity","root":root,"id":"blocked","issued_at":now,"expires_at":now+300,"ops":[{
                "target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":"codex"},
                "policy_schema":"taskq.host-capacity/1","expected_revision":policy.revision().unwrap(),"changes":{"runtime_limit":3}}]});
        let lock = coreq::file_lock::lock(&install.join("resources/lock")).unwrap();
        let (capacity_tx, capacity_rx) = std::sync::mpsc::channel();
        let blocked = ActionQueue::start(
            install.clone(),
            &["capacity"],
            true,
            |install, root, message| {
                crate::platform::write_atomic(&install.join("capacity-entered"), "")?;
                capacity(install, root, message)
            },
            move |_, _, result| capacity_tx.send(result).unwrap(),
        )
        .unwrap();
        blocked.submit(&message).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !install.join("capacity-entered").exists() && Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(install.join("capacity-entered").exists());
        assert!(capacity_rx.try_recv().is_err());
        let (pause_tx, pause_rx) = std::sync::mpsc::channel();
        let focus = ActionQueue::start(
            install.clone(),
            &["pause", "resume", "focus"],
            true,
            queued_focus,
            move |_, _, result| pause_tx.send(result).unwrap(),
        )
        .unwrap();
        focus
            .submit(&json!({"a":"pause","root":root,"id":"pause"}))
            .unwrap();
        assert!(pause_rx.recv_timeout(WAIT).unwrap().unwrap()["applied_receipt"].is_string());
        assert!(crate::data::focus(&root).unwrap().is_empty());
        assert!(capacity_rx.try_recv().is_err());
        drop(lock);
        assert_eq!(
            capacity_rx.recv_timeout(WAIT).unwrap().unwrap()["state"],
            "saved"
        );
        for queue in [blocked, focus] {
            let ActionQueue { sender, worker, .. } = queue;
            drop(sender);
            worker.join().unwrap();
        }
    });
}

#[test]
fn blocked_answer_does_not_block_pause() {
    crate::tests::with_test_capture(|_store, dir| {
        let root = dir.join("project");
        let install = dir.join("install");
        fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(crate::paths::dot(&root).join("focus"), "all").unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let answer = ActionQueue::start(
            install.clone(),
            &["answer"],
            true,
            move |_, _, _| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(WAIT).unwrap();
                Ok(json!({"answerReceipt":{"state":"accepted"}}))
            },
            |_, _, _| {},
        )
        .unwrap();
        answer
            .submit(&json!({"a":"answer","root":root,"id":"tray.1","op":{"id":"tray.1"}}))
            .unwrap();
        entered_rx.recv_timeout(WAIT).unwrap();
        let (pause_tx, pause_rx) = std::sync::mpsc::channel();
        let focus = ActionQueue::start(
            install.clone(),
            &["pause", "resume", "focus"],
            true,
            queued_focus,
            move |_, _, result| pause_tx.send(result).unwrap(),
        )
        .unwrap();
        focus
            .submit(&json!({"a":"pause","root":root,"id":"pause"}))
            .unwrap();
        assert!(pause_rx.recv_timeout(WAIT).unwrap().unwrap()["applied_receipt"].is_string());
        assert!(crate::data::focus(&root).unwrap().is_empty());
        release_tx.send(()).unwrap();
        for queue in [answer, focus] {
            let ActionQueue { sender, worker, .. } = queue;
            drop(sender);
            worker.join().unwrap();
        }
    });
}

// Chain: workspace answer send -> readback resend while the first is still queued. Defect (#1662 A1): the resend
// carried no draft side channel, so the same op read as id-mismatch and the pending answer as rejected. Risk kept:
// a different payload under a pending op id is still refused.
#[test]
fn answer_resend_keeps_its_pending_correlation() {
    crate::tests::with_test_capture(|_store, dir| {
        // § 5: 30 KiB crosses this queue's old 20 KiB cap; file/storage checks cover the larger boundaries.
        for op in [
            json!({"id":"ws.1","task":7,"revision":20,"choice":2}),
            json!({"id":"ws.1","task":7,"revision":20,"text":"word 🦀\n".repeat(3_000)}),
        ] {
            let root = dir.join("project");
            let install = dir.join("install");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&install).unwrap();
            fs::write(
                install.join("projects"),
                format!(
                    "{}
",
                    root.display()
                ),
            )
            .unwrap();
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let queue = ActionQueue::start(
                install.clone(),
                &["answer"],
                true,
                move |_, _, _| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(WAIT).unwrap();
                    Ok(json!({}))
                },
                |_, _, _| {},
            )
            .unwrap();
            let first = json!({"a":"answer","root":root,"board":"https://github.com/o/r/issues","op":op,"local":{"draft":"","receipt":{"state":"sending"}}});
            queue.submit(&answer_request(&first)).unwrap();
            entered_rx.recv_timeout(WAIT).unwrap();
            let readback =
                json!({"a":"answer","root":root,"board":"https://github.com/o/r/issues","op":op});
            let check = json!({"a":"answer","root":root,"board":"https://github.com/o/r/issues","op":op,"local":{"draft":"","receipt":{"state":"unknown"}}});
            // the cause: the prior bridge queued the message as sent, draft included
            let raw = |m: &Value| {
                let mut m = m.clone();
                m["id"] = json!("ws.0");
                m["op"]["id"] = json!("ws.0");
                m
            };
            queue.submit(&raw(&first)).unwrap();
            assert!(
                queue
                    .submit(&raw(&readback))
                    .unwrap_err()
                    .contains("already pending"),
                "the unnormalized resend reproduces id-mismatch"
            );
            queue.submit(&answer_request(&readback)).unwrap();
            queue.submit(&answer_request(&check)).unwrap();
            let mut other = readback.clone();
            other["op"]["choice"] = json!(1);
            assert!(
                queue
                    .submit(&answer_request(&other))
                    .unwrap_err()
                    .contains("already pending"),
                "the correlation guard stays"
            );
            let mut oversized = readback.clone();
            oversized["extra"] = json!("x".repeat(21 << 10));
            assert!(
                queue
                    .submit(&answer_request(&oversized))
                    .unwrap_err()
                    .contains("payload too large"),
                "non-reply envelope fields retain their bound"
            );
            release_tx.send(()).unwrap();
            entered_rx.recv_timeout(WAIT).unwrap(); // the deliberately unnormalized ws.0 job
            release_tx.send(()).unwrap();
            let ActionQueue { sender, worker, .. } = queue;
            drop(sender);
            worker.join().unwrap();
        }
    });
}

// Chain: native Resume -> shared command -> failure receipt; retries keep the selected focus.
#[test]
fn action_failures_reach_the_tray_caller() {
    crate::tests::with_test_capture(|store, _| {
        let root =
            std::env::temp_dir().join(format!("taskq-action-receipt-{}", std::process::id()));
        fs::create_dir_all(crate::paths::dot(&root)).unwrap();
        fs::write(crate::paths::dot(&root).join("focus"), "off").unwrap();
        assert!(
            act("resume", &root, None)
                .unwrap_err()
                .contains("no saved focus")
        );
        assert_eq!(
            fs::read_to_string(crate::paths::dot(&root).join("focus")).unwrap(),
            "off"
        );
        fs::write(
            crate::paths::dot(&root).join("focus.paused"),
            "telemetry-fixture",
        )
        .unwrap();
        act("resume", &root, None).unwrap();
        assert_eq!(
            fs::read_to_string(crate::paths::dot(&root).join("focus")).unwrap(),
            "telemetry-fixture"
        );
        assert!(act("focus", &root, Some("stale-label")).is_err());
        fs::remove_dir_all(&root).unwrap();
        assert!(act("pause", &root, None).is_err());
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
            receipts.len(),
            6,
            "native dispatch bypassed the shared receipt boundary"
        );
        assert_eq!(
            receipts
                .iter()
                .map(|r| r["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "command.received",
                "command.failed",
                "command.received",
                "command.applied",
                "command.received",
                "command.failed"
            ]
        );
        assert_ne!(
            receipts[0]["span"], receipts[2]["span"],
            "separate clicks reused an identity"
        );
        for pair in receipts.chunks_exact(2) {
            assert_eq!(pair[0]["span"], pair[1]["span"]);
        }
    });
}

// Chain: R12 tray evidence -> `tray --snapshot --route`. Risk: a filtered run captures the full matrix or drops a route.
#[test]
fn snapshot_routes_capture_only_named_pages() {
    let install = std::env::temp_dir().join(format!("taskq-tray-routes-{}", std::process::id()));
    fs::create_dir_all(&install).unwrap();
    let root = std::env::temp_dir().join("taskq-tray-routes-project");
    fs::write(install.join("projects"), root.to_str().unwrap()).unwrap();
    let r = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let names = |v: &[&str]| {
        (0..shots(&install, &r(v)).len())
            .map(|i| shot_name(&install, &r(v), i))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&["attention"]),
        [
            "dark-attention-16",
            "dark-attention-20",
            "light-attention-16",
            "light-attention-20"
        ]
    );
    assert_eq!(
        names(&["actions", "project"]),
        [
            "dark-home-actions-16",
            "dark-project-16",
            "light-home-actions-16",
            "light-project-16"
        ]
    );
    assert_eq!(
        names(&["scroll"])[..3],
        [
            "dark-home-scroll-start-16",
            "dark-home-scroll-middle-16",
            "dark-home-scroll-end-16"
        ]
    );
    assert_eq!(
        names(&["scroll"]).len(),
        12,
        "start, middle and end at 16 and 20 px in both themes"
    );
    assert_eq!(
        names(&["answers"])[..2],
        ["dark-home-answers-menu-16", "dark-home-answers-details-16"]
    );
    assert_eq!(
        names(&["answers"]).len(),
        8,
        "menu question and its details at 16 and 20 px in both themes"
    );
    assert!(
        names(&["icon"]).is_empty(),
        "icon alone captures no popover page"
    );
    assert_eq!(
        names(&["views"]),
        [
            "dark-views-top-16",
            "dark-views-states-16",
            "dark-views-top-20",
            "dark-views-states-20",
            "light-views-top-16",
            "light-views-states-16",
            "light-views-top-20",
            "light-views-states-20"
        ],
        "P-W4: the View window in both themes and sizes"
    );
    assert_eq!(
        names(&["conversations"]),
        [
            "dark-conversations-badge-18",
            "dark-conversations-question-18",
            "dark-conversations-owner-18",
            "dark-conversations-chat-18",
            "dark-conversations-badge-20",
            "dark-conversations-question-20",
            "dark-conversations-owner-20",
            "dark-conversations-chat-20",
            "light-conversations-badge-18",
            "light-conversations-chat-18"
        ],
        "#1699 A8: at most ten screens"
    );
    assert_eq!(
        names(&["opacity"]),
        [
            "dark-whole-100-16",
            "dark-whole-90-16",
            "dark-whole-60-16",
            "dark-background-0-16",
            "dark-background-50-16",
            "light-whole-100-16",
            "light-whole-90-16",
            "light-whole-60-16",
            "light-background-0-16",
            "light-background-50-16"
        ],
        "M1.1/M1.2: ten explicit native-alpha and background cases"
    );
    let all = names(&[]);
    assert_eq!(
        all.len(),
        2 * (4 + 2
            * (ROUTES
                .iter()
                .filter(|r| !r.starts_with("agentq-")
                    && **r != "views"
                    && **r != "conversations"
                    && **r != "opacity")
                .count()
                - 5)),
        "no filter keeps the menu matrix without scroll or answers"
    );
    assert!(
        all.iter().all(|name| !name.contains("agentq-")
            && !name.contains("-views-")
            && !name.contains("-conversations-")
            && !name.contains("-opacity-")),
        "invented agentq, View and opacity evidence is opt-in only"
    );
    for route in ROUTES {
        if route.starts_with("agentq-") {
            assert_eq!(names(&[route]).len(), 4, "one state, both themes and sizes");
        }
        assert!(!names(&[route]).is_empty(), "{route} names a captured page");
    }
    fs::remove_dir_all(&install).ok();
}

// Chain: §7 Reports -> native snapshot routes. Risk: reports lack a native capture or change the default matrix.
#[test]
fn reports_snapshots_are_opt_in_and_cover_both_surfaces_and_themes() {
    let install = std::env::temp_dir();
    for route in [
        "reports",
        "reports-conversations",
        "results",
        "results-conversations",
    ] {
        let selected = vec![route.to_string()];
        let cases = shots(&install, &selected);
        assert_eq!(cases.len(), 2);
        assert_eq!(
            shot_name(&install, &selected, 0),
            format!("dark-{route}-16")
        );
        assert_eq!(
            shot_name(&install, &selected, 1),
            format!("light-{route}-16")
        );
    }
    assert!(
        shots(&install, &[])
            .iter()
            .all(|(_, p, _, js)| !route_of(p, js).starts_with("reports"))
    );
}

// Chain: owner tray answer -> canonical CLI. Risk: IPC selects another project or inherits worker fencing.
#[test]
fn answer_bridge_restricts_targets_and_preserves_literal_owner_input() {
    let install = std::env::temp_dir().join(format!("taskq-tray-answer-{}", std::process::id()));
    let root = install.join("project");
    fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    fs::write(install.join("projects"), root.to_str().unwrap()).unwrap();
    let mut m = json!({"root": root, "board": "https://github.com/o/r/issues", "op": {"id": "tray.1", "task": 7, "revision": 20, "text": "2; literal $(text)"}});
    let args = answer_args(&install, &m).unwrap();
    assert_eq!(
        args,
        [
            "answer",
            "7",
            "--text",
            "2; literal $(text)",
            "--json",
            "--op-id",
            "tray.1",
            "--revision",
            "20"
        ]
    );
    let command = owner_command(&root).unwrap();
    for key in ["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE"] {
        assert!(
            command.get_envs().any(|(k, v)| k == key && v.is_none()),
            "{key}"
        );
    }
    m["op"]["choice"] = json!(2);
    assert!(
        answer_args(&install, &m).is_err(),
        "choice and text are exclusive"
    );
    m["op"].as_object_mut().unwrap().remove("text");
    assert_eq!(answer_args(&install, &m).unwrap()[1], "7.2");
    m["root"] = json!(install.join("unregistered"));
    assert!(
        answer_args(&install, &m)
            .unwrap_err()
            .contains("not registered")
    );
    m["root"] = json!(root);
    m["op"]["revision"] = Value::Null;
    assert!(answer_args(&install, &m).is_err());
    m["op"]["revision"] = json!(20);
    m["op"].as_object_mut().unwrap().remove("choice");
    m["op"]["text"] = json!("word ".repeat(53) + "ok");
    assert_eq!(
        answer_args(&install, &m).unwrap()[3],
        "word ".repeat(53) + "ok"
    );
    m["op"]["action"] = json!("requeue");
    m["op"]["text"] = json!("word\n".repeat(400));
    assert_eq!(
        answer_args(&install, &m).unwrap()[3],
        format!("Reason: {}", "word\n".repeat(400))
    );
    for action in ["answer", "steer", "requeue"] {
        if action == "answer" {
            m["op"].as_object_mut().unwrap().remove("action");
        } else {
            m["op"]["action"] = json!(action);
        }
        let text = "word 🦀\n".repeat(200_000);
        m["op"]["text"] = json!(text);
        let args = answer_args(&install, &m).unwrap();
        let path = install.join("reply-input");
        let (argv, input) = answer_input(&args, &path);
        let input = input.unwrap();
        let expected = if action == "requeue" {
            format!("Reason: {text}")
        } else {
            text
        };
        assert_eq!(
            input, expected,
            "§ 5: the entire reply uses file transport, above the old 1 MiB exchange limit"
        );
        assert!(
            argv.contains(&"--text-file".into())
                && argv.iter().map(String::len).sum::<usize>() < 512
        );
        fs::write(&path, input.as_bytes()).unwrap();
        let mut parsed = crate::cli::Args::parse(&argv).unwrap();
        parsed.read_text(&mut std::io::empty()).unwrap();
        assert_eq!(parsed.one("text"), Some(expected.as_str()));
    }
    for cmd in ["later", "close"] {
        let original = vec![
            cmd.into(),
            "7".into(),
            "--text".into(),
            "Reason: cancelled".into(),
        ];
        assert_eq!(
            answer_input(&original, Path::new("unused")),
            (original, None),
            "§ 5: cancellation commands retain literal text"
        );
    }
    fs::remove_dir_all(install).unwrap();
}

// Chain: live board snapshot -> tray answer. Risk: forged IPC bypasses read-only/close controls.
#[test]
fn answer_bridge_checks_board_and_canonical_question() {
    let card = "Done: checked\nProblem: decide\n1. close: ship ★\n2. redo";
    let row =
        json!({"n": 7, "revision": 20, "card": card, "question": crate::snapshot::question(card)});
    let mut s = json!({"complete": true, "board": {"url": "https://github.com/o/r/issues"}, "tasks": [row]});
    let mut m = json!({"board": "https://github.com/o/r/issues", "op": {"id": "tray.1", "task": 7, "revision": 20, "choice": 1}});
    assert!(
        answer_snapshot(&s, &m)
            .unwrap_err()
            .contains("close/publication")
    );
    m["op"]["choice"] = json!(2);
    assert!(answer_snapshot(&s, &m).is_ok());
    s["tasks"][0]["question"]["parsed"] = json!(false);
    assert!(answer_snapshot(&s, &m).unwrap_err().contains("read-only"));
    s["tasks"][0]["revision"] = json!(21);
    assert!(
        answer_snapshot(&s, &m).is_ok(),
        "dispatch owns stale and idempotent retry receipts"
    );
    let rows = s["tasks"].take();
    s["tasks"] = json!([]);
    assert!(
        answer_snapshot(&s, &m).is_err(),
        "an omitted row never bypasses the consequential-choice guard"
    );
    s["tasks"] = rows;
    s["board"]["url"] = json!("https://github.com/o/other/issues");
    assert!(answer_snapshot(&s, &m).unwrap_err().contains("identity"));
    s["complete"] = json!(false);
    s["reason"] = json!("permission denied");
    assert_eq!(answer_snapshot(&s, &m).unwrap_err(), "permission denied");
}

// Chain: lost answer race -> CLI stale receipt -> tray receipt -> steer. Risk (#1785): the tray showed revision
// ids and CLI advice instead of who answered, and offered no way to pass the losing answer on.
#[test]
fn answer_bridge_stale_receipt_names_the_winner_and_steers() {
    let m =
        json!({"root": "/p", "op": {"id": "tray.1", "task": 7, "revision": 20, "text": "use y"}});
    let cli = json!({"task": 7, "op_id": "tray.1", "outcome": "stale", "revision": 22, "reason": "revision 20 is stale; current is 22",
            "next": "Read `taskq inbox 7`; retry with --revision 22", "answered": {"by": "alex", "text": "use x"}});
    let r = answer_receipt(&m, &cli)["answerReceipt"].clone();
    assert_eq!(
        (
            r["state"].as_str(),
            r["answered"]["by"].as_str(),
            r["answered"]["text"].as_str()
        ),
        (Some("stale"), Some("alex"), Some("use x"))
    );
    let mut lost = cli.clone();
    lost.as_object_mut().unwrap().remove("answered");
    // A late answer the fold rejects (task already doing) names the winner too, not the CLI refusal text.
    let rejected = json!({"task": 7, "op_id": "tray.1", "outcome": "rejected", "reason": "#7 already answered by alex: use x",
            "next": "To add to it: `taskq steer 7 --text T`.", "answered": {"by": "alex", "text": "use x"}});
    let late = answer_receipt(&m, &rejected)["answerReceipt"].clone();
    assert_eq!(
        (
            late["state"].as_str(),
            late["answered"]["by"].as_str(),
            late["reason"].as_str()
        ),
        (Some("rejected"), Some("alex"), Some(""))
    );
    for r in [r, answer_receipt(&m, &lost)["answerReceipt"].clone(), late] {
        let words = r.to_string();
        assert!(
            !words.contains("revision") && !words.contains("taskq") && !words.contains("22"),
            "{words}"
        );
    }
    let install = std::env::temp_dir().join(format!("taskq-tray-steer-{}", std::process::id()));
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("projects"), "/p").unwrap();
    let mut m = json!({"root": "/p", "board": "https://github.com/o/r/issues", "op": {"id": "tray.2", "task": 7, "revision": 22, "action": "steer", "text": "use y"}});
    assert_eq!(
        answer_args(&install, &m).unwrap(),
        [
            "steer",
            "7",
            "--text",
            "use y",
            "--json",
            "--op-id",
            "tray.2",
            "--revision",
            "22"
        ]
    );
    let mut s = json!({"complete": true, "board": {"url": m["board"]}, "tasks": [{"n": 7, "state": "doing", "revision": 22}]});
    assert!(answer_snapshot(&s, &m).is_ok());
    s["tasks"][0]["state"] = json!("review");
    assert!(
        answer_snapshot(&s, &m).is_ok(),
        "steer remains a durable note outside doing"
    );
    s["tasks"][0]["state"] = json!("ask");
    s["tasks"][0]["revision"] = json!(23);
    assert!(
        answer_snapshot(&s, &m).is_ok(),
        "a new question cannot drop a steer"
    );
    s["tasks"][0]["state"] = json!("closed");
    assert!(
        answer_snapshot(&s, &m).is_err(),
        "notes never reopen closed tasks"
    );
    m["op"]["text"] = json!(
        "1
2
3
4
5"
    );
    assert!(
        answer_args(&install, &m).is_ok(),
        "§ 5: steer accepts multiline human text"
    );
    fs::remove_dir_all(install).unwrap();
}

// Chain: no-choice ask -> tray IPC -> CLI dispatch -> board readback. Risk: cancellation publishes a result,
// stale input acts on another question, or retry/partial failure duplicates a write or skips parking.
#[test]
fn answer_bridge_fallback_cancellation_chain() {
    use crate::board::{Board, Fake};
    let install = std::env::temp_dir().join(format!("taskq-tray-cancel-{}", std::process::id()));
    let root = install.join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(install.join("projects"), root.to_str().unwrap()).unwrap();
    fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
    let (cfg, _) = crate::config::load(&root).unwrap();
    let b = Fake::default();
    b.state.borrow_mut().publish = "pending".into(); // a publication attempt must not succeed
    let n = b
        .create("Needs a decision", "## Goal\ng", &["taskq".into()])
        .unwrap();
    let attempt = b.comment(n, "taskq: spawn worker codex:u mac").unwrap();
    b.comment(
        n,
        &format!("taskq: result {} attempt {attempt}", "a".repeat(40)),
    )
    .unwrap();
    let reviewer = b.comment(n, "taskq: spawn reviewer codex:v mac").unwrap();
    let rev = b
        .comment(
            n,
            &format!("taskq: ask attempt {reviewer}\nReviewer cannot continue."),
        )
        .unwrap();
    let mut m = json!({"root": root, "board": "https://github.com/o/r/issues", "op": {"id":"cancel.1", "task":n, "revision":rev, "action":"close", "confirmed":true}});
    let mut snapshot = json!({"complete":true,"board":{"url":m["board"]},"tasks":[{"n":n,"state":"ask","revision":rev,"question":crate::snapshot::question("Reviewer cannot continue.")}]});
    assert!(answer_snapshot(&snapshot, &m).is_ok());
    for st in ["doing", "review", "ready", "waiting"] {
        snapshot["tasks"][0]["state"] = json!(st);
        assert!(answer_snapshot(&snapshot, &m).is_err());
    }
    snapshot["tasks"][0]["state"] = json!("ask");
    snapshot["tasks"][0]["revision"] = json!(rev + 1);
    assert!(answer_snapshot(&snapshot, &m).is_err());
    snapshot["tasks"][0]["revision"] = json!(rev);
    snapshot["tasks"][0]["question"]["choices"] = json!([{"id":1}]);
    assert!(answer_snapshot(&snapshot, &m).is_err());
    snapshot["tasks"][0]["question"]["choices"] = json!([]);
    m["op"].as_object_mut().unwrap().remove("confirmed");
    assert!(
        answer_args(&install, &m).is_ok(),
        "one explicit cancellation action needs no confirmation flag"
    );
    let args = answer_args(&install, &m).unwrap();
    assert_eq!(args[0], "later");
    assert_eq!(args[args.len() - 3], "cancel.1.later");
    for outcome in ["rejected", "stale", "unknown"] {
        let mut calls = 0;
        let r = answer_dispatch(&m, &args, |_| {
            calls += 1;
            json!({"task":n,"op_id":"cancel.1.later","outcome":outcome})
        });
        assert_eq!((calls, r["outcome"].as_str().unwrap()), (1, outcome));
    }
    let mut calls = Vec::new();
    let execute = |args: &[String]| {
        let op = crate::dispatch::Op {
            id: Some(args[args.len() - 3].clone()),
            revision: Some(args.last().unwrap().parse().unwrap()),
        };
        let receipt = if args[0] == "close" {
            crate::dispatch::close_one(&b, &cfg, n, &args[3], &op)
        } else {
            crate::dispatch::act(&b, &root, n, &args[0], &[], &args[3], &op)
        };
        serde_json::to_value(receipt).unwrap()
    };
    let partial = answer_dispatch(&m, &args, |a| {
        calls.push(a.to_vec());
        if a[0] == "close" {
            json!({"task":n,"op_id":"cancel.1.close","outcome":"unknown","reason":"response lost"})
        } else {
            execute(a)
        }
    });
    assert!(
        partial["reason"]
            .as_str()
            .unwrap()
            .contains("Parked; close not verified")
    );
    assert_eq!(
        partial["outcome"], "partial",
        "a parked task whose close failed reads as partly applied"
    );
    assert_eq!(
        crate::dispatch::current(&b, n).unwrap().1.state,
        crate::core::State::Later
    );
    snapshot["tasks"][0]["state"] = json!("later");
    assert!(
        answer_snapshot(&snapshot, &m).is_ok(),
        "retry can reconcile its parking receipt"
    );
    let r = answer_dispatch(&m, &args, |a| {
        calls.push(a.to_vec());
        execute(a)
    });
    assert_eq!(r["state"], "closed");
    assert_eq!(
        r["outcome"], "accepted",
        "must cancel without publishing the stored result"
    );
    assert_eq!(calls[0], calls[2]);
    assert_eq!(calls[1], calls[3]);
    assert_eq!(calls[1].last().unwrap(), &rev.to_string());
    let count = b.comments(n).unwrap().len();
    snapshot["tasks"] = json!([]);
    assert!(
        answer_snapshot(&snapshot, &m).is_err(),
        "an omitted row never grants cancellation authority"
    );
    assert_eq!(answer_dispatch(&m, &args, execute)["state"], "closed");
    assert_eq!(b.comments(n).unwrap().len(), count);
    for action in ["requeue", "later"] {
        m["op"] = json!({"id":"action.1","task":n,"revision":rev,"action":action});
        if action == "requeue" {
            m["op"]["text"] = json!("Retry with literal $(text)");
        }
        let a = answer_args(&install, &m).unwrap();
        assert_eq!(a[0], action);
        assert_eq!(a[a.len() - 3], "action.1");
        let mut seen = 0;
        assert_eq!(
            answer_dispatch(&m, &a, |_| {
                seen += 1;
                json!({"task":n,"op_id":"action.1","outcome":"accepted","state":"ready"})
            })["outcome"],
            "accepted"
        );
        assert_eq!(seen, 1);
    }
    // #1476 Start: held drafts only; accepted counts only when read back ready or waiting.
    m["op"] = json!({"id":"start.1","task":n,"revision":rev,"action":"start"});
    let a = answer_args(&install, &m).unwrap();
    assert_eq!(
        a,
        [
            "start",
            &n.to_string(),
            "--json",
            "--op-id",
            "start.1",
            "--revision",
            &rev.to_string()
        ]
    );
    for (st, ok) in [
        ("held", true),
        ("ready", true),
        ("waiting", true),
        ("ask", false),
        ("doing", false),
    ] {
        snapshot["tasks"] = json!([{"n":n,"state":st,"revision":rev}]);
        assert_eq!(answer_snapshot(&snapshot, &m).is_ok(), ok, "{st}");
    }
    for (st, outcome) in [
        ("ready", "accepted"),
        ("waiting", "accepted"),
        ("held", "unknown"),
    ] {
        assert_eq!(
            answer_dispatch(
                &m,
                &a,
                |_| json!({"task":n,"op_id":"start.1","outcome":"accepted","state":st})
            )["outcome"],
            outcome,
            "{st}"
        );
    }
    m["op"]["text"] = json!("x");
    assert!(answer_args(&install, &m).is_err());
    m["op"] = json!({"id":"action.1","task":n,"revision":rev,"action":"requeue"});
    m["op"]["text"] = json!("x".repeat(233));
    assert!(
        answer_args(&install, &m).is_ok(),
        "§ 5: owner requeue is wrapped by dispatch"
    );
    m["op"]["text"] = json!("a\nb");
    assert!(
        answer_args(&install, &m).is_ok(),
        "§ 5: owner requeue accepts multiple lines"
    );
    // #1506 published controls: the DashQ fixture argv and state table are what the tray dispatches and offers.
    let f: Value =
        serde_json::from_str(include_str!("../tests/fixtures/task-controls.json")).unwrap();
    assert_eq!(f["version"], "taskq.task-controls/1");
    let table: Vec<Value> = crate::snapshot::CONTROLS
        .iter()
        .map(|(a, c, t, s)| json!([a, c, t, s]))
        .collect();
    let fixture: Vec<Value> = f["controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| json!([c["action"], c["command"], c["text"], c["states"]]))
        .collect();
    assert_eq!(table, fixture);
    for (st, offered) in f["states"].as_object().unwrap() {
        assert_eq!(json!(crate::snapshot::controls(st)), *offered, "{st}");
    }
    for c in f["cases"].as_array().unwrap() {
        m["op"] = c["op"].clone();
        let args = answer_args(&install, &m).unwrap();
        assert_eq!(json!(args), c["argv"]);
        let mut file_args = args;
        file_args.extend(["--json-file".into(), "receipt.json".into()]);
        assert!(
            crate::cli::Args::parse(&file_args).is_ok(),
            "§ 5: every published control supports file receipts: {}",
            c["action"]
        );
        snapshot["tasks"] = json!([c["row"]]);
        assert!(answer_snapshot(&snapshot, &m).is_ok());
        snapshot["tasks"][0]["state"] = json!("closed");
        assert!(
            answer_snapshot(&snapshot, &m).is_err(),
            "closed offers no control"
        );
        snapshot["tasks"][0]["revision"] = json!(99);
        // start reconciles by state (#1476); the others let an old revision reach the CLI, which replays or refuses stale
        assert_eq!(
            answer_snapshot(&snapshot, &m).is_ok(),
            c["op"]["action"] != "start"
        );
    }
    m["op"] = json!({"id":"x.1","task":n,"revision":rev,"action":"stop","text":"x"});
    assert!(answer_args(&install, &m).is_err());
    fs::remove_dir_all(install).unwrap();
}

// Chain: menu `size` message -> popover window size. Risk: a scrolling menu clipped on the right behind a horizontal
// scrollbar (#1073), a short menu given an empty scrollbar strip, or Windows Text size shrinking the CSS viewport.
#[test]
fn scrolling_menu_window_adds_the_scrollbar_width() {
    assert_eq!(
        popover_size(&json!({"w": 320, "h": 500, "g": 8, "d": 2}), 600.0, 2.0),
        (640, 1000)
    );
    assert_eq!(
        popover_size(&json!({"w": 360, "h": 900, "g": 8, "d": 1}), 600.0, 1.0),
        (368, 600)
    );
    assert_eq!(
        popover_size(&json!({"w": 320, "h": 900}), 600.0, 1.0),
        (320, 600),
        "overlay scrollbars (macOS) take no width"
    );
    // Text size 150 % at 100 % display scale: 320 CSS px need 480 window px; 400 CSS px tall fit, 500 scroll
    assert_eq!(
        popover_size(&json!({"w": 320, "h": 400, "g": 10, "d": 1.5}), 600.0, 1.0),
        (480, 600)
    );
    assert_eq!(
        popover_size(&json!({"w": 320, "h": 300, "g": 10, "d": 1.5}), 600.0, 1.0),
        (480, 450)
    );
    assert_eq!(
        popover_size(&json!({"w": 320, "h": 500, "g": 10, "d": 1.5}), 600.0, 1.0),
        (495, 600)
    );
    // Text size 140 % at 125 % display scale: 331 CSS px are 579.25 device px, never 579
    assert_eq!(
        popover_size(
            &json!({"w": 320, "h": 900, "g": 11, "d": 1.75}),
            600.0,
            1.25
        )
        .0,
        580
    );
}

// Chain: icon click -> popover shown -> blur. Risk: opened from the Windows 11 hidden-icons flyout, the popover
// closes 15 ms later because the flyout retakes the foreground; or a later blur no longer closes it.
#[test]
fn blur_right_after_opening_keeps_the_popover() {
    assert!(blur_refocuses(Duration::from_millis(15), false));
    assert!(
        !blur_refocuses(Duration::from_millis(300), false)
            && !blur_refocuses(Duration::from_secs(5), false)
    );
    assert!(
        !blur_refocuses(Duration::from_millis(15), true),
        "an intentional workspace handoff must not pull focus back to the tray"
    );
}

// Chain: M8 one store feeds every window. Risk: a detached conversation compares against the menu's shown
// snapshot, so it misses a change the menu already took while the detached window was busy.
#[test]
fn each_window_takes_a_snapshot_it_has_not_received() {
    let (mut menu, mut detached) = (String::new(), String::new());
    assert!(take_due(&mut menu, "a", true));
    assert!(!take_due(&mut detached, "a", false), "a busy window waits");
    assert!(
        take_due(&mut detached, "a", true),
        "the menu having it does not skip the detached window"
    );
    assert!(!take_due(&mut detached, "a", true) && !take_due(&mut menu, "a", true));
    assert!(take_due(&mut detached, "b", true));
}

// H-A2: host policy/readback cannot be confused with a project's lower ceiling or legacy writable scope.
#[test]
fn capacity_projects_to_resource_targets() {
    let saved = json!({"limits":{"claude":0,"codex":3},"load_gate":{"enabled":true,"cpu_max":85,"memory_min":10},"admission":{"mode":"auto","max":null}});
    let cap = json!({"host_policy":saved,"host_runtime_used":{"claude":1,"codex":2},"project_limits":{"codex":1},
            "limits":{"codex":1},"used":{"codex":1},"load_gate":{"enabled":true,"cpu_max":85,"memory_min":10},
            "sample_at":95,"sample_age_seconds":3,"cpu_percent":12.5,"memory_available_percent":40.0,
            "reasons":[{"code":"hold"},{"code":"future"},{"code":"cpu"}]});
    let group = resource_group(
        &cap,
        &saved,
        &json!("command-current"),
        &json!({"codex":1}),
        Some(100),
        None,
        "taskq",
    );
    let t = group["targets"].as_array().unwrap();
    assert_eq!(
        t[0]["runtime_usage"][1],
        json!({"runtime_id":"codex","used":2,"limit":3,"limit_kind":"ceiling"})
    );
    assert_eq!(t[1]["current_policy"]["limits"]["codex"], 1);
    assert!(
        t[0]["effective_revision"].is_null(),
        "equal settings without the host's exact receipt cannot look applied"
    );
    assert_eq!(
        t[0]["supported"][0]["fields"],
        json!(["maximum", "load_gate"])
    );
    assert_eq!(
        t[1]["supported"][0]["fields"],
        json!([]),
        "a project ceiling has no host write authority"
    );
    assert!(
        t[3]["target"]["project_id"].is_null(),
        "runtime ceiling belongs to the host"
    );
    assert!(t[0]["current_policy"]["mode"].is_null());
    let codes: Vec<_> = t[0]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["host", "unknown", "cpu"]);
    assert_eq!(t[0]["sample"]["age_seconds"], 3);
    let mut stale = cap.clone();
    stale.as_object_mut().unwrap().remove("host_policy");
    let unknown = resource_group(
        &stale,
        &saved,
        &json!("command-current"),
        &json!({"codex":1}),
        Some(100),
        None,
        "taskq",
    );
    assert!(unknown["targets"][0]["effective_revision"].is_null());
    assert!(
        unknown["targets"][0]["runtime_usage"][1]["used"].is_null(),
        "legacy project snapshot cannot prove host use"
    );
}

// H-A2: a stale page cannot repurpose a legacy project operation as an edit to every project on the host.
#[test]
fn capacity_bridge_refuses_unqualified_host_edits() {
    for set in [
        json!({"mode":"auto"}),
        json!({"limits":{"codex":4}}),
        json!({"maximum":null}),
    ] {
        assert_eq!(
            capacity(
                Path::new("/nonexistent-taskq-954"),
                Path::new("/nonexistent-taskq-954"),
                &json!({"set":set})
            )
            .unwrap()["state"],
            "rejected"
        );
    }
    let dir = std::env::temp_dir().join(format!("taskq-tray-cap-telemetry-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = dir.join("project");
    let install = dir.join("install");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
    fs::write(install.join("host.json"), "{\"limits\":{\"codex\":2}}").unwrap();
    let (capture, guard) = crate::profile::tests::collecting(&install);
    let (tx, rx) = std::sync::mpsc::channel();
    let queue = ActionQueue::start(
        install.clone(),
        &["capacity"],
        true,
        |install, root, message| {
            if message["id"] == "unknown-op" {
                return Ok(json!({"state":"unknown","reason":"prior write interrupted"}));
            }
            capacity(install, root, message)
        },
        move |_, _, result| tx.send(result).unwrap(),
    )
    .unwrap();
    queue
        .submit(&json!({"a":"capacity","root":root,"id":"rejected-op","ops":[]}))
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap().unwrap()["state"], "rejected");
    queue
        .submit(&json!({"a":"capacity","root":root,"id":"unknown-op","ops":[]}))
        .unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap().unwrap()["state"], "unknown");
    let ActionQueue { sender, worker, .. } = queue;
    drop(sender);
    worker.join().unwrap();
    capture.flush().unwrap();
    let actions = crate::telemetry::diagnostic::actions(&install, None).unwrap();
    assert!(
        actions.contains("tray.capacity.execute") && actions.contains("1/1/0"),
        "{actions}"
    );
    assert!(
        actions.contains("capacity-rejected:1, capacity-unknown:1"),
        "{actions}"
    );
    drop(guard);
    drop(capture);
    let _ = fs::remove_dir_all(dir);
}

// Chain: § 4 tray opens an incident report. Risk: tray opens a file outside .taskq/incidents.
#[test]
fn incident_report_only_opens_json_inside_its_project() {
    let root = std::env::temp_dir().join(format!("taskq-tray-incident-{}", std::process::id()));
    let dir = crate::paths::dot(&root).join("incidents");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("report.json"), "{}").unwrap();
    fs::write(root.join("private.json"), "{}").unwrap();
    fs::write(dir.join("program.exe"), "{}").unwrap();
    assert_eq!(
        incident_path(&root, &json!({"path": ".taskq/incidents/report.json"})),
        Some(dir.join("report.json"))
    );
    for path in [
        "private.json",
        ".taskq/incidents/../../private.json",
        ".taskq/incidents/program.exe",
        ".taskq/incidents/missing.json",
    ] {
        assert!(
            incident_path(&root, &json!({"path": path})).is_none(),
            "{path}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

// Chain: Scroll position (taskq.md), docs/design/ai-workspace.md. Risk: the page boundary refuses a drift or
// click-to-text fact, so `taskq profile actions` never reads it, or takes a name outside the closed lists.
#[test]
fn reading_facts_cross_the_web_boundary_as_actions() {
    let id = format!("{}.1", "a".repeat(32));
    let row = |operation: &str, outcome: &str, error: &str| json!({"kind":"ui","operation":operation,"trace":id,"span":id,"parent":null,"started_ms":1,"duration_us":7,"outcome":outcome,"error":error});
    for operation in [
        "tasks",
        "history",
        "conversations",
        "bounded",
        "answer-card",
        "finding-composer",
        "workspace",
    ]
    .iter()
    .flat_map(|surface| {
        [
            "insert-above",
            "append",
            "removal",
            "late-image",
            "resize",
            "refresh",
        ]
        .map(|cause| format!("js.scroll.drift.{surface}.{cause}"))
    }) {
        let accepted = web_observation(&row(&operation, "failed", "scroll-drift")).unwrap();
        assert_eq!(
            (&accepted["kind"], &accepted["source"]),
            (&json!("method"), &json!("action")),
            "{operation}"
        );
    }
    for operation in ["cold.snapshot", "warm.cache", "cold.board", "warm.unknown"] {
        let accepted = web_observation(&row(
            &format!("js.conversation.open.{operation}"),
            "ok",
            "none",
        ))
        .unwrap();
        assert_eq!(
            accepted["duration_us"], 7,
            "click-to-text carries its elapsed time"
        );
        assert!(
            accepted["trace"].is_string() && accepted["span"].is_string(),
            "the action reader requires both"
        );
    }
    for operation in [
        "js.scroll.drift.private.refresh",
        "js.scroll.drift.tasks.private",
        "js.conversation.open.cold.private",
    ] {
        assert!(
            web_observation(&row(operation, "ok", "none")).is_err(),
            "{operation}"
        );
    }
}

// Risk: page metadata smuggles content or forges a method outside the bundled inventory.
#[test]
fn web_metadata_is_allowlisted_and_aggregate_identity_is_explicitly_unknown() {
    let row = json!({"operation":"js.tray.create.mode","kind":"method.aggregate","started_ms":1,"counts":{"calls":2,"finished":2,"failed":0,"unknown":0},"outcome":"ok","error":"none"});
    let accepted = web_observation(&row).unwrap();
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        let mut calls = WebCalls::new();
        assert!(calls.metadata(&row), "missing collector must be visible");
        assert!(calls.metadata_unknown);
        assert!(!calls.failed, "optional metadata disabled command controls");
        assert!(
            !calls.metadata(&row),
            "same outage repeated its notification"
        );
    });
    assert_eq!(accepted["unknown"]["trace"], "aggregate-unlinked");
    for (key, value) in [
        ("message", json!("private")),
        ("operation", json!("js.private-data")),
        ("trace", json!("/private/path")),
        ("counts", json!({"private":1})),
        ("error", json!("raw-error")),
    ] {
        let mut invalid = row.clone();
        invalid[key] = value;
        assert!(web_observation(&invalid).is_err(), "{key}");
    }
}

// #1789 A1/A3: macOS can widen its slot; Windows retains a square notification-area image.
#[test]
fn two_digit_summary_uses_the_native_canvas() {
    let sum = Summary {
        counts: [12, 3, 15, 0],
        blocked: 0,
        known: true,
        run: true,
        ..summary(&[], (0, 1))
    };
    let image = draw(&sum, false, 1.0, 2.0).unwrap();
    assert_eq!(image.h, 44);
    assert_eq!(image.w > 44, cfg!(target_os = "macos"));
    assert_eq!(
        draw_svg(&icon::svg(&icon_data(&sum), false), 1.0, 2.0)
            .unwrap()
            .px,
        image.px,
        "published/native paths agree"
    );
}

// A7: removed Custom resolves to Summary; unrelated settings survive a subsequent save.
#[test]
fn saved_custom_resolves_to_summary_without_losing_settings() {
    let install = std::env::temp_dir().join(format!("taskq-icon-{}", std::process::id()));
    fs::create_dir_all(&install).unwrap();
    let p = install.join("tray.json");
    for icon in [json!({"version":1}), json!({"version":99}), Value::Null] {
        fs::write(&p,json!({"view":"custom","font":18,"period":"custom","start":5,"end":20,"tz":"UTC","icon":icon,"unrelated":{"keep":true}}).to_string()).unwrap();
        let settings = Settings::load(&install);
        assert_eq!(settings.view, "full");
        assert_eq!(settings.font, 18);
        assert_eq!(settings.range(50), (5, 20));
        assert!(settings.save(&install));
        let saved: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        assert_eq!(saved["unrelated"], json!({"keep":true}));
        assert!(saved.get("icon").is_none());
    }
    fs::remove_dir_all(install).unwrap();
}

// Chain: R6, § 4, #1967 A4: closing a heartbeat removes it from status, claim hosts and the tray strip input;
// an open stale host stays unknown, and retirement on one board does not retire it on another.
#[test]
fn retired_host_leaves_status_and_strip() {
    use crate::board::Board;
    let a = crate::board::Fake::default();
    let b = crate::board::Fake::default();
    let labels = [crate::service::HOST_LABEL.into()];
    let retired = a
        .create("taskq host retired", "version 4", &labels)
        .unwrap();
    a.create("taskq host stale", "version 4", &labels).unwrap();
    let other = b
        .create("taskq host retired", "version 4", &labels)
        .unwrap();
    a.advance(301);
    let cfg = crate::tests::cfg();
    let read = |board: &crate::board::Fake| crate::snapshot::read(board, &cfg, &json!({}), &[]);
    assert_eq!(read(&a).hosts.len(), 2);
    a.close(retired).unwrap();
    let snap = read(&a);
    assert_eq!(
        snap.hosts
            .iter()
            .map(|h| h.name.as_str())
            .collect::<Vec<_>>(),
        ["stale"]
    );
    let text = snap.text(&cfg, &json!({})).unwrap();
    assert!(!text.contains("retired"));
    assert!(
        text.contains("stale v4") && text.contains(" stale"),
        "{text}"
    );
    let (_, hosts) =
        crate::service::views(&a, &mut Default::default(), &mut Default::default(), 300).unwrap();
    assert_eq!(
        hosts
            .iter()
            .filter_map(crate::service::host_name)
            .collect::<Vec<_>>(),
        ["stale"]
    );
    let status = snap.json();
    let (strip, _) = liveness(&status, Some(0), cfg.ttl, &mut vec![]);
    assert_eq!(strip.as_array().unwrap().len(), 1);
    assert_eq!(
        (strip[0]["name"].clone(), strip[0]["live"].clone()),
        (json!("stale"), json!(false))
    );
    assert_eq!(read(&b).hosts[0].name, "retired");
    b.close(other).unwrap();
    assert!(read(&b).hosts.is_empty());
}

// #1591 A3: an observation 61-299 s old reads live in the tray, as in the CLI (`snapshot::live`); older than ttl stale.
// Risk: a 60 s literal in the tray marks a live service unknown between idle passes.
#[test]
fn tray_and_cli_share_the_ttl_freshness_rule() {
    let s = json!({"observed": 1000, "tasks_age_seconds": 0, "hosts": [{"name": "mac", "updated": 1000}, "bad"],
            "tasks": [{"n": 1, "executions": [{"id": 7, "observed": 1000}]}]});
    for (age, live) in [
        (0, true),
        (61, true),
        (299, true),
        (300, true),
        (301, false),
    ] {
        let mut tasks = s["tasks"].as_array().cloned().unwrap();
        let (hosts, tasks_live) = liveness(&s, Some(age), 300, &mut tasks);
        assert_eq!(
            (
                tasks_live,
                hosts[0]["live"].clone(),
                tasks[0]["executions"][0]["live"].clone()
            ),
            (live, json!(live), json!(live)),
            "{age} s"
        );
        assert_eq!(crate::snapshot::live(age as i64, 300), live, "CLI {age} s");
        assert_eq!(hosts[1], "bad");
    }
    let mut tasks = vec![];
    assert!(
        !liveness(&json!({"observed": 1000}), Some(1), 300, &mut tasks).1,
        "missing task age is stale"
    );
    assert!(
        !liveness(&s, None, 300, &mut tasks).1,
        "missing status file is stale"
    );
    assert!(
        !crate::snapshot::live(-1, 300),
        "a future observation is stale"
    );
}

// Chain: tray project page and settings. Risk: corrupt status.json or tray.json reads as zero/default and is reset
#[test]
fn corrupt_status_and_tray_json_read_unknown_and_stay() {
    let install = std::env::temp_dir().join(format!("taskq-tray-corrupt-{}", std::process::id()));
    let root = install.join("project");
    fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    // R6/M3: the native adapter preserves the same observation used by status and capacity.
    let status = json!({"observed":99,"tasks_age_seconds":3,"hosts":[{"name":"mac","updated":98}],
            "capacity":{"host_runtime_used":{"codex":2},"reservations":[]},
            "tasks":[{"n":1,"executions":[{"id":7,"role":"worker","runtime":"codex","state":"running"}]}]});
    fs::write(
        crate::paths::dot(&root).join("status.json"),
        status.to_string(),
    )
    .unwrap();
    let projected = project(&root, root.to_str().unwrap());
    for key in ["observed", "tasks_age_seconds", "capacity"] {
        assert_eq!(projected[key], status[key], "{key}");
    }
    // #1591/#1592: rows keep every observed field and gain only the computed `live`, `age` and `age_text`
    let strip = |v: &Value| {
        let mut v = v.clone();
        for r in v.as_array_mut().into_iter().flatten() {
            for k in ["live", "age", "age_text"] {
                r.as_object_mut().unwrap().remove(k);
            }
        }
        v
    };
    assert_eq!(strip(&projected["hosts"]), status["hosts"]);
    assert_eq!(
        strip(&projected["tasks"][0]["executions"]),
        status["tasks"][0]["executions"]
    );
    assert_eq!(projected["tasks"][0]["local_session"], false);
    fs::write(
        crate::paths::dot(&root).join("status.json"),
        "{\"tasks\": [{\"n\": 1",
    )
    .unwrap();
    let p = project(&root, root.to_str().unwrap());
    assert!(
        p["complete"] == false
            && p["tasks"] == json!([])
            && p["error"]
                .as_str()
                .unwrap()
                .starts_with("status.json: corrupt"),
        "{p}"
    );
    assert!(!summary(&[p], (0, 1)).known, "counts unknown, never zero");
    assert_eq!(
        fs::read_to_string(crate::paths::dot(&root).join("status.json")).unwrap(),
        "{\"tasks\": [{\"n\": 1"
    );
    // a corrupt focus in a fresh status.json names its reason
    fs::write(
        crate::paths::dot(&root).join("status.json"),
        r#"{"focus": null, "focus_error": "focus: not UTF-8 text; file kept"}"#,
    )
    .unwrap();
    assert_eq!(
        project(&root, root.to_str().unwrap())["error"],
        "focus: not UTF-8 text; file kept"
    );
    fs::write(install.join("tray.json"), "{\"view\": \"fu").unwrap();
    let s = Settings::load(&install);
    assert!(s.view == "icon" && s.font == 16, "defaults shown");
    assert_eq!(
        fs::read_to_string(install.join("tray.json")).unwrap(),
        "{\"view\": \"fu",
        "a read never resets it"
    );
    let reason = state(
        &install,
        &s,
        false,
        None,
        &Value::Null,
        &mut crate::pending::Pending::default(),
    )["settings_error"]
        .clone();
    assert!(
        reason.as_str().unwrap().starts_with("tray.json: corrupt"),
        "the popover names the reason: {reason}"
    );
    // tests/errors.browser.cjs verifies the corruption reason in the shared header, not a page-text literal.
    assert!(s.save(&install));
    assert_eq!(
        state(
            &install,
            &s,
            false,
            None,
            &Value::Null,
            &mut crate::pending::Pending::default()
        )["settings_error"],
        Value::Null,
        "a save clears it"
    );
    assert_eq!(
        fs::read_to_string(install.join("tray.json.corrupt")).unwrap(),
        "{\"view\": \"fu",
        "a save keeps a copy"
    );
    assert_eq!(Settings::load(&install).font, 16);
    fs::remove_dir_all(install).unwrap();
}

// Chain: owner answer -> accepted op-id readback -> stale status.json refresh -> every tray surface. Risk (#1918):
// the list, dot, badge, icon, notification or conversation reads a stale pre-answer snapshot and shows the
// question open again after the board accepted the answer.
#[test]
fn an_accepted_answer_stays_closed_on_every_surface_across_a_stale_refresh() {
    let base = std::env::temp_dir().join(format!("taskq-tray-one-channel-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let (install, root) = (base.join("install"), base.join("project"));
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
    let r = root.to_str().unwrap();
    let card = "Problem: Pick one\n1. Ship\n2. Wait";
    let status = |state: &str, revision: u64, ops: &[&str]| {
        let mut t = json!({"n": 7, "title": "T", "state": state, "revision": revision, "ops": ops});
        if state == "ask" {
            t["card"] = json!(card);
            t["question"] = crate::snapshot::question(card);
        }
        let s = json!({"complete": true, "issues": "https://github.com/o/r/issues", "observed": now(), "tasks_age_seconds": 0, "tasks": [t]});
        fs::write(crate::paths::dot(&root).join("status.json"), s.to_string()).unwrap();
    };
    // the ask was notified once, before the answer
    fs::write(
        crate::paths::dot(&root).join("events"),
        format!("since 1\n{} #7 ask 1\n", now() - 5),
    )
    .unwrap();
    status("ask", 30, &["ask-op"]);
    let s = Settings::load(&install);
    let mut sync = crate::pending::Pending::default();
    let (mut n, mut bursts) = (
        json!({"host": {"ask": "banner"}}),
        notify::Bursts::default(),
    );
    let before = state(&install, &s, false, None, &Value::Null, &mut sync);
    notify::due(
        &mut n,
        before["projects"].as_array().unwrap(),
        now(),
        &mut bursts,
    );
    let ask = |st: &Value| {
        let ps = st["projects"].as_array().unwrap();
        let t = &ps[0]["tasks"][0];
        let dot = ps[0]["counts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["state"] == "ask")
            .unwrap()["n"]
            .clone();
        let badge = st["messages"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "question" && e["pending"] == true);
        (
            t["state"].clone(),
            dot,
            badge,
            summary_at(ps, (0, 1), now()).counts[1],
        )
    };
    assert_eq!(
        ask(&before),
        (json!("ask"), json!(1), true, 1),
        "the question is open before the answer"
    );

    let request =
        json!({"root": r, "op": {"id": "tray.1", "task": 7, "revision": 30, "choice": 1}});
    sync.begin(&request, &before["projects"]);
    assert!(sync.receipt(&json!({"answerReceipt": {"root": r, "id": "tray.1", "state": "accepted"}, "sync": {"state": "doing", "revision": 25}})));
    // the service has not read the board again: status.json still holds the question, twice
    for _ in 0..2 {
        status("ask", 30, &["ask-op"]);
        let st = state(&install, &s, false, None, &Value::Null, &mut sync);
        assert_eq!(
            ask(&st),
            (json!("doing"), json!(0), false, 0),
            "list, dot, badge and icon keep the readback"
        );
        assert!(
            notify::due(
                &mut n,
                st["projects"].as_array().unwrap(),
                now(),
                &mut bursts
            )
            .is_empty(),
            "no notification asks again"
        );
        let t = &st["projects"][0]["tasks"][0];
        assert_eq!(
            (
                t["history"][0]["question"]["text"].as_str(),
                t["history"][0]["answered"].clone(),
                t["card"].clone()
            ),
            (
                Some(card),
                json!({"by": "you", "text": "Ship"}),
                Value::Null
            ),
            "the conversation keeps the question, then the answer"
        );
    }
    // the board's readback carries the op id; a lagging read after it still never reopens the question
    status("doing", 25, &["tray.1", "ask-op"]);
    assert_eq!(
        ask(&state(&install, &s, false, None, &Value::Null, &mut sync)).0,
        json!("doing")
    );
    status("ask", 30, &["ask-op"]);
    assert_eq!(
        ask(&state(&install, &s, false, None, &Value::Null, &mut sync)),
        (json!("doing"), json!(0), false, 0)
    );
    fs::remove_dir_all(base).unwrap();
}

// Chain: § 4 tray counts across projects. Risk: double counting or unknown shown as known.
#[test]
fn summary_counts_one_task_per_board_and_issue() {
    let p = |issues: &str, state: &str, focus: Value| json!({"issues": issues, "focus": focus, "tasks": [{"n": 1, "state": state}], "events": [[95, "close", "7"], [-250, "failed", "8"]]});
    let ps = [
        p("https://github.com/o/a/issues", "ask", json!([])),
        p("https://github.com/o/a/issues", "ask", json!([])),
        p("https://github.com/o/b/issues", "doing", json!(["all"])),
    ];
    let s = summary(&ps, (-200, 100));
    assert_eq!(s.counts, [1, 1, 0, 0]);
    assert!(s.run && !summary(&ps[..2], (-200, 100)).run);
    assert_eq!(
        s.bins.iter().map(|b| b.counts[0]).sum::<u32>(),
        2,
        "same attempt on two boards counts twice, on one board once"
    );
    assert_eq!(
        s.bins.iter().map(|b| b.counts[1]).sum::<u32>(),
        0,
        "outside the period"
    );
    assert!(
        !s.known,
        "a project without a complete snapshot makes the counts unknown"
    );
    let mut ok = ps.clone();
    for p in &mut ok {
        p["complete"] = json!(true);
    }
    assert!(summary(&ok, (-200, 100)).known);
    ok[0]["tasks"][0]["state"] = json!("waiting");
    ok[1]["tasks"][0]["state"] = json!("waiting");
    let blocked = summary(&ok, (-200, 100));
    assert_eq!(
        blocked.blocked, 1,
        "blocked tasks deduplicate by board and issue"
    );
    assert_eq!(
        blocked.counts[2], 0,
        "ready count stays separate from blocked"
    );
}

// Chain: M6/D11 rolling history grid and U-A6 icon. Risk: bars drift on every refresh, or a boundary loses or moves events.
#[test]
fn history_grid_moves_one_bin_at_a_boundary() {
    let b = 14400 * 150_000;
    let mut ev: Vec<Value> = (1..6)
        .map(|i| json!([b - i * 14400, KINDS[(i % 4) as usize], format!("e{i}")]))
        .collect();
    ev.extend([
        json!([b - 3700, "close", "late"]),
        json!([b - 3601, "ask", "last"]),
        json!([b + 30, "failed", "new"]),
    ]);
    let at = |now: i64| {
        let seen: Vec<&Value> = ev
            .iter()
            .filter(|e| e[0].as_i64().unwrap() <= now)
            .collect();
        summary_at(
            &[
                json!({"issues":"u","tasks":[],"focus":["all"],"events":seen,"since":0,"complete":true}),
            ],
            day(now),
            now,
        )
    };
    let icons = |s: &Summary| {
        [(1.0, 2.0), (16.0 / 22.0, 1.0)].map(|(k, dpr)| draw(s, true, k, dpr).unwrap().png())
    };
    let frames = [b - 3600, b - 1, b, b + 31, b + 3600];
    let sums: Vec<Summary> = frames.iter().map(|&t| at(t)).collect();
    let pngs: Vec<_> = sums.iter().map(icons).collect();
    for (i, j) in [(0, 1), (3, 4)] {
        assert_eq!(sums[i].bins, sums[j].bins);
        assert_eq!(pngs[i], pngs[j]);
    }
    let counts = |s: &Summary| s.bins.iter().map(|b| b.counts).collect::<Vec<_>>();
    let before = counts(&sums[1]);
    let after = counts(&sums[2]);
    assert_eq!(after[..5], before[1..], "whole grid advances one bin");
    assert_eq!((after[5], sums[3].bins[5].counts), ([0; 4], [0, 1, 0, 0]));
    assert_ne!(pngs[1], pngs[2]);
    if let Ok(dir) = std::env::var("TASKQ_FRAMES") {
        // ponytail: evidence frames on demand, `TASKQ_FRAMES=DIR cargo test history_grid`
        for (t, p) in frames.iter().zip(&pngs) {
            for (size, png) in ["22pt@2x", "16px@1x"].iter().zip(p) {
                fs::write(
                    Path::new(&dir).join(format!("icon-summary-{:+}s-{size}.png", t - b)),
                    png,
                )
                .unwrap();
            }
        }
    }
}

// Chain: § 4 tray settings input. Risk: webview input stores unknown values.
#[test]
fn settings_keep_only_known_values() {
    let mut s = Settings {
        view: "icon".into(),
        font: 16,
        opacity: 100,
        opacity_mode: "whole".into(),
        background_opacity: 100,
        opacity_error: None,
        period: "1d".into(),
        start: 0,
        end: 0,
        tz: "local".into(),
        pin: Pin::default(),
        notify: Value::Null,
        navigation: Value::Null,
        workspace_navigation: Value::Null,
        conversation_geometry: None,
    };
    s.set(&json!({"view": "full", "font": 18, "period": "1w"}));
    s.set(&json!({"view": "x", "font": 15, "period": "2y"}));
    s.set(&json!({"period": "custom", "start": 50, "end": 50, "tz": "UTC"}));
    assert!(
        s == Settings {
            view: "full".into(),
            font: 18,
            opacity: 100,
            opacity_mode: "whole".into(),
            background_opacity: 100,
            opacity_error: None,
            period: "1w".into(),
            start: 0,
            end: 0,
            tz: "local".into(),
            pin: Pin::default(),
            notify: Value::Null,
            navigation: Value::Null,
            workspace_navigation: Value::Null,
            conversation_geometry: None
        }
    );
    assert_eq!(
        s.range(1000),
        (100800 - 604800, 100800),
        "whole 28 h bins, the newest holds now"
    );
    s.set(&json!({"period": "custom", "start": 40, "end": 160, "tz": "UTC"}));
    assert_eq!((s.range(1000), s.tz.as_str()), ((40, 160), "UTC"));
}

// Chain: M1.1 / #1812 A2. Risk: restart loses opacity, invalid IPC bypasses the opacity range, or save loses other preferences.
#[test]
fn opacity_validation_and_restart_preserve_other_preferences() {
    let install = std::env::temp_dir().join(format!("taskq-opacity-{}", std::process::id()));
    fs::create_dir_all(&install).unwrap();
    fs::write(
        install.join("tray.json"),
        r#"{"font":20,"future":{"keep":true}}"#,
    )
    .unwrap();
    let mut settings = Settings::load(&install);
    assert_eq!(settings.opacity, 100, "existing installs default to Off");
    for value in [60, 61, 89, 90, 95, 100] {
        assert!(appearance_input(&json!({"opacity":value})).is_ok());
        settings.set(&json!({"opacity":value}));
        assert!(settings.save(&install));
        settings = Settings::load(&install);
        assert_eq!((settings.opacity, settings.font), (value, 20));
    }
    settings.set(&json!({"opacity":95}));
    for value in [
        json!(null),
        json!(59),
        json!(101),
        json!(-1),
        json!(90.5),
        json!("95"),
    ] {
        assert!(appearance_input(&json!({"opacity":value})).is_err());
        settings.set(&json!({"opacity":value}));
        assert_eq!(settings.opacity, 95, "refuse invalid opacity");
    }
    assert!(settings.save(&install));
    assert_eq!(
        state(
            &install,
            &settings,
            false,
            None,
            &Value::Null,
            &mut crate::pending::Pending::default()
        )["opacity"],
        95,
        "native readback reaches every menu route"
    );
    let saved: Value =
        serde_json::from_str(&fs::read_to_string(install.join("tray.json")).unwrap()).unwrap();
    assert_eq!(saved["future"]["keep"], true);
    fs::remove_dir_all(install).unwrap();
}

// Chain: M1.2 / #1876 A2–A3. Risk: legacy backing blocks transparency, restart loses either mode, or invalid IPC applies.
#[test]
fn background_opacity_validation_and_reload() {
    let install = std::env::temp_dir().join(format!("taskq-background-{}", std::process::id()));
    fs::create_dir_all(&install).unwrap();
    fs::write(
        install.join("tray.json"),
        r#"{"opacity":60,"font":20,"backing_strength":98,"future":true}"#,
    )
    .unwrap();
    let mut s = Settings::load(&install);
    assert_eq!(
        (s.opacity_mode.as_str(), s.native_opacity()),
        ("whole", 60),
        "legacy backing does not block new alpha"
    );
    for background in [0, 50, 100] {
        let v = json!({"opacity_mode":"background","background_opacity":background});
        assert!(appearance_input(&v).is_ok());
        s.set(&v);
        assert!(s.save(&install));
        s = Settings::load(&install);
        assert_eq!(
            (s.opacity, s.native_opacity(), s.background_opacity, s.font),
            (60, 100, background, 20)
        );
    }
    for v in [
        json!({"opacity_mode":null}),
        json!({"opacity_mode":"other"}),
        json!({"background_opacity":-1}),
        json!({"background_opacity":101}),
        json!({"background_opacity":0.5}),
        json!({"opacity":59}),
    ] {
        assert!(
            appearance_input(&v).is_err(),
            "reject {v} before native application"
        );
    }
    let st = state(
        &install,
        &s,
        false,
        None,
        &Value::Null,
        &mut crate::pending::Pending::default(),
    );
    assert_eq!(
        (
            st["opacity_mode"].as_str(),
            st["background_opacity"].as_u64()
        ),
        (Some("background"), Some(100))
    );
    assert!(
        st.get("backing_strength").is_none(),
        "retired backing never reaches the renderer"
    );
    s.set(&json!({"opacity_mode":"whole"}));
    assert_eq!(
        s.native_opacity(),
        60,
        "switch restores the independent whole-window value"
    );
    let saved: Value =
        serde_json::from_str(&fs::read_to_string(install.join("tray.json")).unwrap()).unwrap();
    assert_eq!(
        saved["backing_strength"], 98,
        "legacy field stays inert without destroying preferences"
    );
    assert_eq!(saved["future"], true);
    fs::remove_dir_all(install).unwrap();
}

// Chain: M1.1 / #1812 A3. Risk: native alpha washes primary, secondary or semantic body text below AA on a busy backdrop.
#[test]
fn opacity_text_palette_meets_aa_after_adverse_composition() {
    let rgb = |hex: &str| -> Vec<f64> {
        (1..7)
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64)
            .collect()
    };
    let luminance = |rgb: Vec<f64>| -> f64 {
        rgb.iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(v, w)| {
                let c = v / 255.0;
                w * if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            })
            .sum()
    };
    for dark in [false, true] {
        assert_eq!(
            menu_palette(dark, 100),
            icon::css(dark),
            "Off retains the accepted palette"
        );
        for percent in 90..100 {
            let palette = menu_palette(dark, percent);
            let colors = std::iter::once(if dark { "#FFFFFF" } else { "#12181C" })
                .chain(palette.split(';').map(|p| p.split_once(':').unwrap().1));
            let surfaces = if dark {
                ["#252B32", "#303840", "#37424C", "#354A40", "#323B45"]
            } else {
                ["#FAFBFC", "#EDF0F3", "#E9EDF1", "#DCE7E1", "#FFFFFF"]
            };
            for color in colors {
                for surface in surfaces {
                    for alpha in [
                        percent as f64 / 100.0,
                        ((percent * 255 + 50) / 100) as f64 / 255.0,
                    ] {
                        let fg = luminance(
                            rgb(color)
                                .iter()
                                .map(|v| alpha * v + (1.0 - alpha) * if dark { 0.0 } else { 255.0 })
                                .collect(),
                        );
                        let bg = luminance(
                            rgb(surface)
                                .iter()
                                .map(|v| alpha * v + (1.0 - alpha) * if dark { 255.0 } else { 0.0 })
                                .collect(),
                        );
                        let contrast = (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05);
                        assert!(
                            contrast >= 4.5,
                            "{dark} {percent}% {color} on {surface}: {contrast}"
                        );
                    }
                }
            }
        }
    }
}

// A7: logical geometry survives DPI changes and missing displays cannot strand the titlebar.
#[test]
fn conversations_geometry_clamps_and_roundtrips_additively() {
    let geometry = ConversationGeometry {
        display: "removed invented display".into(),
        position: [9000.0, -300.0],
        size: [800.0, 600.0],
    };
    let (position, size) = geometry.restore((100, 40, 1600, 900), 1.5, (16, 40));
    assert_eq!(size, (1200, 860));
    assert_eq!(position, (484, 40));
    let dir = std::env::temp_dir().join(format!("taskq-geometry-{}", crate::runtime::uuid()));
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join("tray.json"),
        json!({"unknown_owner_field":{"kept":true},"font":20}).to_string(),
    )
    .unwrap();
    let mut settings = Settings::load(&dir);
    settings.conversation_geometry = Some(geometry.clone());
    assert!(settings.save(&dir));
    let restored = Settings::load(&dir);
    assert!(restored.conversation_geometry == Some(geometry));
    assert_eq!(restored.font, 20);
    assert_eq!(
        crate::data::json(&dir.join("tray.json")).unwrap().unwrap()["unknown_owner_field"]["kept"],
        true
    );
    fs::remove_dir_all(dir).unwrap();
}

// Isolated native windows and temporary settings; never starts a tray/service or takes its host lock.
#[cfg(target_os = "windows")]
#[test]
#[ignore = "native Windows geometry instrument"]
fn conversations_geometry_native_reopen() {
    use tao::platform::{run_return::EventLoopExtRunReturn, windows::EventLoopBuilderExtWindows};
    let Ok(dir) = std::env::var("TASKQ_GEOMETRY_FIXTURE") else {
        let dir =
            std::env::temp_dir().join(format!("taskq-native-geometry-{}", crate::runtime::uuid()));
        fs::create_dir(&dir).unwrap();
        for phase in ["save", "restore"] {
            let mut child = crate::platform::command(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "tray::tests::conversations_geometry_native_reopen",
                    "--nocapture",
                ])
                .env("TASKQ_GEOMETRY_FIXTURE", &dir)
                .env("TASKQ_GEOMETRY_PHASE", phase)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            while child.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    let output = child.wait_with_output().unwrap();
                    panic!(
                        "native geometry fixture timed out: {}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            println!("{}", String::from_utf8_lossy(&output.stdout));
        }
        fs::remove_dir_all(dir).unwrap();
        return;
    };
    let dir = PathBuf::from(dir);
    let mut events = EventLoopBuilder::<Ev>::with_user_event()
        .with_any_thread(true)
        .build();
    let mut settings = Settings::load(&dir);
    let save = std::env::var("TASKQ_GEOMETRY_PHASE").unwrap() == "save";
    let check = |actual: &ConversationGeometry, expected: &ConversationGeometry| {
        assert_eq!(actual.display, expected.display);
        for (a, b) in actual
            .position
            .iter()
            .chain(&actual.size)
            .zip(expected.position.iter().chain(&expected.size))
        {
            assert!((a - b).abs() <= 2.0, "geometry drift {a} vs {b}");
        }
    };
    let (mut reference, mut window, mut captured, mut step) = (None, None, None, 0);
    let mut deadline = Instant::now() + Duration::from_millis(300);
    events.run_return(|event,target,flow|{
            *flow=ControlFlow::WaitUntil(deadline);
            if matches!(event,Event::NewEvents(StartCause::Init)) {
                reference=Some(WindowBuilder::new().with_visible(false).build(target).unwrap());
                let w=conversations_window(target,reference.as_ref().unwrap(),settings.conversation_geometry.as_ref(),false).unwrap();w.set_visible(true);
                if save {let area=work_area(&w.current_monitor().unwrap());w.set_outer_position(PhysicalPosition::new(area.0+100,area.1+80));w.set_inner_size(LogicalSize::new(760.0,500.0));}window=Some(w);
            }
            if !matches!(event,Event::NewEvents(_))||Instant::now()<deadline {return;}
            let w=window.as_ref().unwrap();
            match step {
                0=>{let value=ConversationGeometry::capture(w).expect("visible native geometry");if save {settings.conversation_geometry=Some(value.clone());assert!(settings.save(&dir));}check(&value,settings.conversation_geometry.as_ref().unwrap());captured=Some(value);window.take();let w=conversations_window(target,reference.as_ref().unwrap(),Settings::load(&dir).conversation_geometry.as_ref(),false).unwrap();w.set_visible(true);window=Some(w);}
                1=>{check(&ConversationGeometry::capture(w).unwrap(),captured.as_ref().unwrap());let mut missing=captured.clone().unwrap();missing.display="removed invented display".into();missing.position=[9000.0,-9000.0];missing.size=[9000.0,9000.0];missing.apply(w);}
                _=>{let p=w.outer_position().unwrap();let size=w.outer_size();let area=work_area(&w.current_monitor().unwrap());assert!(p.x>=area.0&&p.y>=area.1&&i64::from(p.x)+i64::from(size.width)<=i64::from(area.0)+i64::from(area.2)+1&&i64::from(p.y)+i64::from(size.height)<=i64::from(area.1)+i64::from(area.3)+1,"native fallback remains reachable");let value=captured.as_ref().unwrap();println!("PASS native {}: logical position {:?}, client size {:?}, scale {}; same-process reopen, fresh-process settings and missing-display fallback",if save {"save"}else{"restore"},value.position,value.size,w.scale_factor());
                    // This child owns only fixture windows; process exit avoids Tao's off-main-thread teardown hang.
                    std::process::exit(0);
                }
            }
            step+=1;deadline=Instant::now()+Duration::from_millis(300);
        });
}

#[test]
fn window_anchor_preserves_edge_and_clamps_to_work_area() {
    let area = (0, 0, 1000, 800);
    for (edge, y, expected) in [
        ("top", 20.0, "top"),
        ("bottom", 780.0, "bottom"),
        ("left", 700.0, "bottom"),
        ("right", 100.0, "top"),
    ] {
        assert_eq!(growth_anchor("auto", edge, Some(y), area), expected);
        assert_eq!(
            growth_anchor("top", edge, Some(y), area),
            "top",
            "explicit Down survives placement"
        );
        assert_eq!(
            growth_anchor("bottom", edge, Some(y), area),
            "bottom",
            "explicit Up survives placement"
        );
    }

    assert_eq!(
        anchored_position((30, 300), 200, (320, 400), area, "bottom"),
        (30, 100)
    );
    assert_eq!(
        anchored_position((30, 300), 400, (320, 200), area, "bottom"),
        (30, 500)
    );
    assert_eq!(
        anchored_position((30, 300), 200, (320, 400), area, "top"),
        (30, 300)
    );
    assert_eq!(
        anchored_position((30, 700), 200, (320, 400), area, "top"),
        (30, 400)
    );
    assert_eq!(
        anchored_position((30, 10), 200, (320, 400), area, "bottom"),
        (30, 0)
    );
}

// Chain: § 4 pinned tray window (menu.md M13). Risk: pin or position lost on restart; window placed off-screen.
#[test]
fn pin_and_position_per_display_survive_restart() {
    let install = std::env::temp_dir().join(format!("taskq-pin-{}", std::process::id()));
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("tray.json"), r#"{"font":18}"#).unwrap();
    let mut s = Settings::load(&install);
    assert!(
        !s.pin.pinned && s.pin.top && s.pin.size.is_none() && s.pin.size_anchor == "auto",
        "new growth defaults to actual-anchor Auto"
    );
    for mode in ["top", "bottom", "fixed"] {
        fs::write(
            install.join("tray.json"),
            json!({"size_anchor":mode,"font":20}).to_string(),
        )
        .unwrap();
        let legacy = Settings::load(&install);
        assert_eq!(
            legacy.pin.size_anchor, mode,
            "existing explicit growth is never migrated to Auto"
        );
        assert_eq!(legacy.font, 20);
    }
    fs::write(install.join("tray.json"), r#"{"size":[-1,0],"top":false}"#).unwrap();
    let invalid = Settings::load(&install);
    assert!(
        invalid.pin.size.is_none() && !invalid.pin.top,
        "invalid size cannot hide the header or resize grip"
    );
    s.pin = Pin {
        size_anchor: "bottom".into(),
        attached_edge: "top".into(),
        auto_anchor: "bottom".into(),
        top: false,
        size: Some((480.0, 600.0)),
        pinned: true,
        display: "B".into(),
        pos: [("A".into(), (10, 20)), ("B".into(), (-1500, 300))].into(),
    };
    // M8/UI02: navigation shares the existing settings save, independent of release version.
    s.set(&json!({"view":"full","period":"custom","start":40,"end":160,"tz":"UTC",
            "navigation":{"schema":1,"current":{"page":"projects","taskFilter":["doing"],"scroll":120,"open":{"scope":true}},"trail":[]},
            "workspace_navigation":{"schema":1,"current":{"page":"sessions","scroll":32},"trail":[{"page":"messages"}]}}));
    assert!(s.save(&install));
    let r = Settings::load(&install);
    assert!(
        r == s,
        "restart must retain navigation and every existing appearance/window setting"
    );
    assert_eq!(r.navigation["current"]["scroll"], 120);
    assert_eq!(
        r.workspace_navigation["current"]["page"], "sessions",
        "workspace and tray keep independent roots"
    );
    let mut invalid_navigation = r.clone();
    invalid_navigation.set(&json!({"navigation":{"schema":2}}));
    assert!(
        invalid_navigation == r,
        "unsupported navigation cannot overwrite known settings"
    );
    invalid_navigation
        .set(&json!({"navigation":{"schema":1,"current":{"page":"x".repeat(65536)}}}));
    assert!(
        invalid_navigation == r,
        "oversized IPC data cannot replace saved settings"
    );
    invalid_navigation.set(&json!({"workspace_navigation":{"schema":2}}));
    assert!(
        invalid_navigation == r,
        "unsupported workspace state cannot overwrite either window"
    );
    invalid_navigation
        .set(&json!({"workspace_navigation":{"schema":1,"current":{"page":"x".repeat(65536)}}}));
    assert!(
        invalid_navigation == r,
        "oversized workspace data cannot replace saved settings"
    );
    assert!(r.pin == s.pin && r.font == 18);
    // M14: modes and cursors survive a restart as saved, unknown values included
    let mut s = r;
    s.notify = json!({"kinds": {"ask": "loud"}, "cursor": {"/p": "100 #7 ask 5"}});
    let event = json!(["https://example.org/issues", "event", 7, [100, "ask", "5"]]).to_string();
    assert!(notify::inbox_set(
        &mut s.notify,
        &json!({"action":"read","ids":[event]})
    ));
    let st = json!({"projects": [{"root": "/p", "name": "p", "tasks": [], "events": [[100, "ask", "5", 7], [200, "failed", "6", 7]]}]});
    let mut asked = true;
    notes(
        &install,
        &mut s,
        &st,
        &mut asked,
        &mut notify::Bursts::new(),
    );
    let r = Settings::load(&install);
    assert_eq!(
        (
            r.notify["cursor"]["/p"].as_str(),
            r.notify["kinds"]["ask"].as_str()
        ),
        (Some("200 #7 failed 6"), Some("loud")),
        "cursor saved before any post"
    );
    assert_eq!(
        r.notify["inbox"][&event], "read",
        "M14: event read state survives disk save and tray restart"
    );
    assert_eq!(
        notify_state(&r.notify, &[])["inbox"],
        r.notify["inbox"],
        "M14: every native renderer receives saved markers"
    );
    let area = (0, 25, 1512, 900);
    assert!(
        on_screen((10, 30), area) && !on_screen((-1500, 300), area) && !on_screen((1512, 30), area)
    );
    assert_eq!(
        clamp((1400, 0), (320, 400), area),
        (1192, 25),
        "kept under the menu bar and inside the right edge"
    );
    assert_eq!(
        clamp((100, 800), (320, 2000), area),
        (100, 25),
        "a too tall window keeps its top in"
    );
    fs::remove_dir_all(install).unwrap();
}

// Chain: § 4 tray history graph. Risk: events land in the wrong bin or period.
#[test]
fn history_bins_are_equal_oldest_left_on_one_scale() {
    // M-A5/M-A6 fixture: 3 of each kind in [0, 120); the first and last bins are not empty; unknown kinds drop
    let ev = json!([
        [0, "close", "1"],
        [5, "close", "2"],
        [119, "close", "3"],
        [0, "failed", "4"],
        [60, "failed", "5"],
        [119, "failed", "6"],
        [120, "failed", "7"],
        [-1, "close", "8"],
        [0, "ask", "9"],
        [30, "ask", "10"],
        [119, "ask", "11"],
        [10, "blocked", "12"],
        [60, "blocked", "13"],
        [119, "blocked", "14"],
        [50, "later", "15"]
    ]);
    let p = json!([{"issues": "u", "tasks": [], "focus": ["all"], "events": ev, "since": -10}]);
    let s = summary(p.as_array().unwrap(), (0, 120));
    assert_eq!(s.bins.len(), 6);
    assert_eq!(
        (s.bins[0].counts, s.bins[3].counts, s.bins[5].counts),
        ([2, 1, 1, 1], [0, 1, 0, 1], [1, 1, 1, 1]),
        "[start, end): 120 and -1 are out"
    );
    assert_eq!(
        (0..4)
            .map(|k| s.bins.iter().map(|b| b.counts[k]).sum::<u32>())
            .collect::<Vec<_>>(),
        [3, 3, 3, 3]
    );
    assert_eq!(s.since, Some(-10));
    let ps = json!([{"issues": "u", "events": []}, {"issues": "v", "events": [], "since": 5}]);
    assert_eq!(
        summary(&ps.as_array().unwrap()[..1], (0, 1)).since,
        None,
        "no events cache: no since"
    );
    assert_eq!(summary(ps.as_array().unwrap(), (0, 1)).since, Some(5));
}

// #2110 C2/CS5: thinning before the accepted-receipt overlay must not hide or lose answered history.
#[test]
fn history_paging_keeps_the_receipt_tail_across_source_lag() {
    let old = json!({"question":{"text":"Earlier"},"answered":{"text":"Kept"}});
    let source = json!([{"root":"invented","tasks":[{"n":7,"state":"ask","revision":30,"ops":[],"question":{"text":"Current"},"history":[old.clone(),old]}]}]);
    let mut pending = crate::pending::Pending::default();
    pending.begin(
        &json!({"root":"invented","op":{"id":"answer","task":7,"revision":30,"text":"Ship"}}),
        &source,
    );
    assert!(pending.receipt(&json!({"answerReceipt":{"root":"invented","id":"answer","state":"accepted"},"sync":{"state":"doing","revision":31,"answered":{"by":"owner","text":"Ship"}}})));
    let mut before = source.clone();
    before[0]["tasks"][0]["history_count"] = json!(2);
    before[0]["tasks"][0]
        .as_object_mut()
        .unwrap()
        .remove("history");
    pending.apply(before.as_array_mut().unwrap());
    assert_ne!(
        before[0]["tasks"][0]["history_count"], 3,
        "before: the overlay's answered record was not in the projected total"
    );
    let mut after = source;
    after[0]["tasks"][0]["history_source_count"] = json!(2);
    after[0]["tasks"][0]["history_source_revision"] = json!(30);
    pending.apply(after.as_array_mut().unwrap());
    history_header(&mut after[0]["tasks"][0]);
    let row = &after[0]["tasks"][0];
    assert_eq!(row["history_count"], 3);
    assert_eq!(row["history_source_revision"], 30);
    assert_eq!(row["history_source_count"], 2);
    assert_eq!(row["history_tail"], true);
    assert_eq!(row["history"].as_array().unwrap().len(), 1);
    assert_eq!(row["history"][0]["question"]["text"], "Current");
    assert_eq!(row["history"][0]["answered"]["text"], "Ship");
    let thin = json!({"n":7,"state":"ask","revision":30,"question":{"text":"Current"},"history_source_revision":30,"history_source_count":2,"history_thin":true});
    let mut receipt = json!({"action":"task-body","state":"accepted","value":{"root":"invented","expected_revision":31,"tasks":[thin]}});
    task_body_receipt(&pending, &mut receipt);
    assert_eq!(receipt["value"]["history_count"], 3);
    assert_eq!(receipt["value"]["history_source_revision"], 30);
    assert_eq!(receipt["value"]["history"].as_array().unwrap().len(), 1);
    assert_eq!(
        receipt["value"]["history"][0]["question"]["text"],
        "Current"
    );
    assert_eq!(receipt["value"]["history"][0]["answered"]["text"], "Ship");
}

#[test]
fn conversation_task_headers_keep_counts_and_pending_source_without_eager_bodies() {
    let card = "Problem: Invented question\n1. Ship ★\n2. Wait";
    let rows=(1..=10_000).map(|n|json!({"n":n,"title":format!("Task {n}"),"state":"ask","revision":30,"card":card,"question":crate::snapshot::question(card),"conversation":{"id":"invented-owner","brief":"Invented body ".repeat(500),"verdict":"Earlier result"},"history":[]})).collect::<Vec<_>>();
    let source = json!({"projects":[{"root":"invented","tasks":rows,"complete":true}],"workspace_local":{"unrelated":{"draft":"Invented draft ".repeat(1000)}},"display":{"open":10000}});
    let before = source.to_string().len();
    let after = workspace_snapshot(&source);
    assert!(
        before > 60_000_000,
        "before: the old workspace envelope transfers every brief"
    );
    assert!(
        after.to_string().len() < 3_000_000,
        "after: lightweight metadata, independent of brief length"
    );
    assert_eq!(after["display"], source["display"]);
    assert_eq!(
        after["projects"][0]["tasks"].as_array().unwrap().len(),
        10_000
    );
    assert!(after["workspace_local"].is_null());
    assert!(after["projects"][0]["tasks"][0]["question"].is_null());
    assert_eq!(
        source["projects"][0]["tasks"][0]["question"]["text"], card,
        "canonical Pending/Home/OS source is not thinned"
    );
    let oversized = json!({"projects":[{"root":"invented","name":"Invented","tasks":[{"n":1,"title":"x".repeat(17*1024*1024)}]}],"display":{"open":1}});
    let refused = workspace_snapshot(&oversized);
    assert!(refused.to_string().len() < 16 * 1024 * 1024);
    assert_eq!(refused["conversations"]["complete"], false);
    assert_eq!(refused["conversations"]["absence_complete"], false);
    assert_eq!(refused["display"], oversized["display"]);
    let mut pending = crate::pending::Pending::default();
    pending.begin(
        &json!({"root":"invented","op":{"id":"answer","task":1,"revision":30,"text":"Ship"}}),
        &source["projects"],
    );
    assert!(pending.receipt(&json!({"answerReceipt":{"root":"invented","id":"answer","state":"accepted"},"sync":{"state":"doing","revision":31,"answered":{"by":"owner","text":"Ship"}}})));
    let body = source["projects"][0]["tasks"][0].clone();
    let mut receipt = json!({"action":"task-body","state":"accepted","value":{"root":"invented","expected_revision":31,"tasks":[body],"workspace_local":{"invented#1":{"draft":"Kept"}}}});
    task_body_receipt(&pending, &mut receipt);
    assert_eq!(receipt["value"]["revision"], 31);
    assert_eq!(receipt["value"]["state"], "doing");
    assert!(receipt["value"]["question"].is_null());
    assert_eq!(receipt["value"]["history"][0]["question"]["text"], card);
    assert_eq!(receipt["workspace_local"]["invented#1"]["draft"], "Kept");
}

// Chain: #1712 events file -> project() -> Hour summary. Risk: a second ask of one attempt or a close in the
// window is not counted, or a missing source reads as zero without a reason.
#[test]
fn hour_window_counts_each_ask_and_close_and_names_missing_sources() {
    let root = std::env::temp_dir().join(format!("taskq-1712-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(crate::paths::dot(&root)).unwrap();
    let install = root.join("install");
    let r = root.to_str().unwrap();
    assert_eq!(
        (
            project(&install, r)["since"].clone(),
            project(&install, r)["events_error"].clone()
        ),
        (Value::Null, json!("no events file"))
    );
    fs::write(crate::paths::dot(&root).join("events"), "1000 #9 ask 3\n").unwrap();
    assert_eq!(
        project(&install, r)["events_error"],
        "events file has no since line"
    );
    // the first ask before the window; an answer, then a second ask and a close of the same task inside it
    fs::write(
        crate::paths::dot(&root).join("events"),
        "since 0\n1000 #9 ask 11\n5000 #9 ask 22\n6000 #9 close 33 replaced by #10 (ready)\n",
    )
    .unwrap();
    let p = project(&install, r);
    // #1800: a close carries its § 7 outcome as a 5th item; other events keep 4
    assert_eq!(
        (
            p["events"][2].clone(),
            p["events"][1].as_array().unwrap().len()
        ),
        (
            json!([6000, "close", "33", 9, "replaced by #10 (ready)"]),
            4
        )
    );
    assert_eq!(
        (p["since"].clone(), p["events_error"].clone()),
        (json!(0), Value::Null)
    );
    let s = summary_at(&[p], (3600, 7200), 7200);
    assert_eq!(
        (0..4)
            .map(|k| s.bins.iter().map(|b| b.counts[k]).sum::<u32>())
            .collect::<Vec<_>>(),
        [1, 0, 1, 0],
        "closed, failed, ask, blocked"
    );
    assert!(s.bins.iter().all(|b| b.missing.is_empty()));
    let _ = fs::remove_dir_all(&root);
}

// Chain: M6. UX30-21: source identity, complete zero, lower bounds, half-open boundaries and multi-host deduplication.
#[test]
fn incoming_coverage_keeps_legacy_and_exact_event_sources_distinct() {
    let legacy = json!({"name":"legacy","issues":"u","tasks":[],"since":0,"events":[[1,"close","1",7],[2,"ask","2",7],[3,"new","unproven",8]]});
    let old = summary_at(&[legacy.clone()], (1, 7), 7);
    assert!(old.bins.iter().all(|b| !b.incoming_complete()));
    assert_eq!(
        old.bins.iter().map(|b| b.incoming).sum::<u32>(),
        0,
        "unversioned lines never prove incoming"
    );
    assert_eq!(
        icon::peak(&old.bins),
        1,
        "known completed/question stacks remain"
    );
    let vector = icon::svg(&icon_data(&old), false);
    assert!(vector.contains("data-mark=\"completed\"") && vector.contains("data-mark=\"ask\""));
    let mut p = legacy.clone();
    p["incoming"] = json!({"schema":1,"board":"u/0","start":1,"end":7,"missing":[],"events":[[1,"new","10",7],[2,"new","11",8],[7,"new","12",9]]});
    let fresh = summary_at(&[p.clone(), p.clone(), legacy.clone()], (1, 7), 7);
    assert!(
        fresh.bins.iter().all(icon::Bin::incoming_complete),
        "another host's legacy cache cannot erase verified board coverage"
    );
    assert_eq!(
        fresh.bins.iter().map(|b| b.incoming).collect::<Vec<_>>(),
        [1, 1, 0, 0, 0, 0],
        "exact [start,end) and one event across hosts"
    );
    assert_eq!(icon::peak(&fresh.bins), 2);
    assert!(
        fresh.bins[2..]
            .iter()
            .all(|b| b.incoming == 0 && b.incoming_complete()),
        "complete zero differs from unknown"
    );
    p["incoming"]["start"] = json!(3);
    let partial = summary_at(&[p.clone()], (1, 7), 7);
    assert!(!partial.bins[0].incoming_complete() && partial.bins[2].incoming_complete());
    assert!(
        !icon::svg(&icon_data(&partial), false)
            .split("id=\"bin-0\"")
            .nth(1)
            .unwrap()
            .split("</g>")
            .next()
            .unwrap()
            .contains("data-mark=\"completed\""),
        "an interval gap stays unknown, retaining bounds in detail"
    );
    p["incoming"]["board"] = json!("foreign/0");
    assert_eq!(
        summary_at(&[p], (1, 7), 7)
            .bins
            .iter()
            .map(|b| b.incoming)
            .sum::<u32>(),
        0
    );
}

// A8: real event identity, half-open bounds and incomplete sources feed the same renderer.
#[test]
fn approved_real_history_fixtures_use_one_six_bin_svg() {
    let fixture: Value =
        serde_json::from_str(include_str!("../tests/fixtures/tray/real-data-day.json")).unwrap();
    let ps:Vec<Value>=fixture["projects"].as_array().unwrap().iter().map(|p|json!({"name":p["name"],"issues":p["board_issues_url"],"since":p["history_since"],"complete":true,"focus":["all"],"tasks":p["current_tasks"].as_array().unwrap().iter().map(|t|{let mut row=t.clone();row["n"]=t["number"].clone();row}).collect::<Vec<_>>(),"events":p["window_events"].as_array().unwrap().iter().map(|e|json!([e["at"],e["kind"],e["id"],e["task"]])).collect::<Vec<_>>()})).collect();
    let captured = fixture["captured_at"].as_i64().unwrap();
    let range = (
        fixture["window"]["start"].as_i64().unwrap(),
        fixture["window"]["end"].as_i64().unwrap(),
    );
    let taskq = ps.iter().find(|p| p["name"] == "taskq").unwrap();
    let day = summary_at(&[taskq.clone()], range, captured);
    assert_eq!(
        day.counts,
        [1, 0, 1, 0],
        "frozen current counters keep distinct task identities"
    );
    assert_eq!(
        day.bins.iter().map(|b| b.counts[0]).collect::<Vec<_>>(),
        [5, 4, 7, 12, 7, 4]
    );
    assert!(day.bins.iter().all(icon::Bin::complete));
    assert!(day.bins[5].ongoing);
    let hour: Value =
        serde_json::from_str(include_str!("../tests/fixtures/tray/real-data-hour.json")).unwrap();
    let hourly = summary_at(
        &[taskq.clone()],
        (
            hour["window_start"].as_i64().unwrap(),
            hour["window_end"].as_i64().unwrap(),
        ),
        captured,
    );
    assert_eq!(
        hourly.bins.iter().map(|b| b.counts[0]).collect::<Vec<_>>(),
        [1, 0, 0, 1, 0, 0]
    );
    let aggregate = summary_at(&ps, range, captured);
    assert!(aggregate.bins[..5].iter().all(|b| b.missing == ["pilotq"]));
    assert!(aggregate.bins[5].complete());
    #[cfg(target_os = "macos")]
    let font = unsafe {
        objc2_app_kit::NSFont::systemFontOfSize_weight(9.0, objc2_app_kit::NSFontWeightMedium)
    }
    .fontName()
    .to_string();
    #[cfg(not(target_os = "macos"))]
    let font = "Segoe UI (requested; native qualification pending)".to_string();
    let mut paused = day.clone();
    paused.run = false;
    let mut zero = day.clone();
    zero.counts = [0; 4];
    zero.blocked = 0;
    for b in &mut zero.bins {
        b.counts = [0; 4];
        b.incoming = 0;
        b.incoming_missing.clear();
    }
    let mut failed = zero.clone();
    failed.bins[2].counts[1] = 1;
    let mut unknown = zero.clone();
    unknown.bins[0].missing = vec!["synthetic missing source".into()];
    unknown.bins[0].counts[0] = 999;
    let mut ask = zero.clone();
    ask.bins[0].counts[2] = 3;
    ask.bins[2].counts = [2, 1, 1, 0];
    ask.bins[4].missing = vec!["synthetic missing source".into()];
    ask.bins[4].counts = [999, 0, 2, 0];
    let mut stack = zero.clone();
    stack.counts = [12, 3, 15, 0];
    for (i, b) in stack.bins.iter_mut().enumerate() {
        b.counts = [(i + 1) as u32, 1, 2, 7];
        b.incoming = (6 - i) as u32;
    }
    let mut partial = stack.clone();
    partial.bins[2].incoming_missing = vec!["synthetic incoming gap".into()];
    let mut rare = zero.clone();
    rare.bins[0].counts = [10000, 1, 1, 0];
    rare.bins[0].incoming = 1;
    let mut overflow = stack.clone();
    overflow.counts = [100, 99, 12, 0];
    for (name, s) in [
        ("day", &day),
        ("hour", &hourly),
        ("incomplete", &aggregate),
        ("paused", &paused),
        ("synthetic-zero", &zero),
        ("synthetic-failed", &failed),
        ("synthetic-unknown", &unknown),
        ("synthetic-ask", &ask),
        ("synthetic-stack", &stack),
        ("synthetic-partial-new", &partial),
        ("synthetic-rare", &rare),
        ("synthetic-overflow", &overflow),
    ] {
        for dark in [false, true] {
            let vector = icon::svg(&icon_data(s), dark);
            let history_vector = icon::svg(
                &icon::Data {
                    paused: false,
                    ..icon_data(s)
                },
                dark,
            );
            for n in [18, 22, 33, 44, 66, 256] {
                assert_eq!(icon::raster(&vector, n).unwrap().w, n);
            }
            if std::env::var_os("TASKQ_ICON_EVIDENCE").is_none() {
                let theme = if dark { "dark" } else { "light" };
                let saved: Value = serde_json::from_slice(
                    &fs::read(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join(format!("tests/fixtures/icon-history/{name}-{theme}.json")),
                    )
                    .expect("canonical browser fixture"),
                )
                .unwrap();
                assert_eq!(
                    saved["bins"],
                    json!(s.bins),
                    "browser/native bins and exact counts"
                );
                assert_eq!(saved["peak"], icon::peak(&s.bins));
                assert_eq!(saved["palette"], icon::css(dark));
                let chart = |svg: &str| {
                    svg.split(r#"<g id="chart""#)
                        .nth(1)
                        .unwrap()
                        .split(r#"</g><g id="counters">"#)
                        .next()
                        .unwrap()
                        .to_string()
                };
                assert_eq!(
                    chart(saved["svg"].as_str().unwrap()),
                    chart(&history_vector),
                    "History retains canonical heights, colors and marks"
                );
            }
            if let Ok(dir) = std::env::var("TASKQ_ICON_EVIDENCE") {
                let dir = Path::new(&dir);
                fs::create_dir_all(dir).unwrap();
                let theme = if dark { "dark" } else { "light" };
                fs::write(dir.join(format!("{name}-{theme}.svg")), &vector).unwrap();
                for n in [18, 44, 66, 256] {
                    fs::write(
                        dir.join(format!("{name}-{theme}-{n}.png")),
                        icon::raster(&vector, n).unwrap().png(),
                    )
                    .unwrap();
                }
                fs::write(dir.join(format!("{name}-{theme}.json")),json!({"bins":s.bins,"peak":icon::peak(&s.bins),"svg":history_vector,"icon_svg":vector,"start":s.bins[0].start,"end":s.bins[5].end,"tz":"local","since":s.since,"palette":icon::css(dark),"captured":captured,"font":font,"compact":format!("data:image/png;base64,{}",base64(&compact(s.run,dark,2.0).png()))}).to_string()).unwrap();
            }
        }
    }
    let mut duplicate = ps.clone();
    duplicate.push(taskq.clone());
    assert_eq!(
        summary_at(&duplicate, range, captured).bins,
        aggregate.bins,
        "same board cache never doubles events"
    );
    let custom = summary_at(&[taskq.clone()], (1, 8), captured);
    assert_eq!(custom.bins[0].start, 1.0);
    assert_eq!(custom.bins[5].end, 8.0);
    assert!(!custom.bins.iter().any(|b| b.ongoing));
}

// Chain: R12 tray agents row. Risk: stale probe shown or missing probe shown as known.
#[test]
fn agents_come_from_the_freshest_probe_or_are_unknown() {
    let row = |v: &str| json!({"runtime": "claude", "path": "/c", "version": v, "login": "not logged in", "account": null});
    let ps = [
        json!({"agents": {"at": 10, "rows": [row("1")]}}),
        json!({"agents": {"at": 20, "rows": [row("2")]}}),
        json!({}),
    ];
    assert_eq!(
        agents(&ps),
        json!({"at": 20, "checking": null, "rows": [{"runtime": "claude", "version": "2", "state": "sign-in-required", "account": null, "method": null, "expires": null, "refreshed": null, "reason": null, "next": "run `claude auth login` with the PM (docs/relogin.md)"}]})
    );
    assert_eq!(agents(&ps[2..]), Value::Null);
    // #1661 A2: a probe in progress reads Checking over its prior rows; a probe read after the mark ends it
    let ps = [
        json!({"agents": {"at": 20, "checking": 30, "rows": [row("2")]}}),
        json!({"agents": {"at": 25, "rows": [row("3")]}}),
    ];
    assert_eq!(
        (
            agents(&ps)["checking"].clone(),
            agents(&ps)["rows"][0]["version"].clone()
        ),
        (json!(30), json!("3"))
    );
    let ps = [
        json!({"agents": {"at": 20, "checking": 30, "rows": []}}),
        json!({"agents": {"at": 40, "rows": []}}),
    ];
    assert_eq!(agents(&ps)["checking"], Value::Null);
    assert_eq!(
        agents(&[json!({"agents": {"checking": 30}})]),
        json!({"at": null, "checking": 30, "rows": []})
    );
}

// Chain: § 6 Add project from the tray. Risk: a folder is classified wrong and set up wrong.
#[test]
fn add_project_has_four_cases() {
    let d = std::env::temp_dir().join(format!("taskq-add-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let kind = |st: &dyn Fn(&Path) -> Result<(), String>| {
        let v = classify(&d, st);
        (
            v["kind"].as_str().unwrap().to_string(),
            v["reason"].as_str().unwrap().to_string(),
        )
    };
    let pass = |_: &Path| Ok(());
    assert_eq!(kind(&pass).0, "new", "empty folder");
    fs::write(d.join("notes.txt"), "x").unwrap();
    assert_eq!(
        kind(&pass).0,
        "plain",
        "files without Git: not a repository"
    );
    fs::create_dir(d.join(".git")).unwrap();
    assert_eq!(kind(&pass).0, "new", "a Git root without taskq.json");
    fs::write(d.join("taskq.json"), "{}").unwrap();
    assert_eq!(kind(&pass).0, "taskq");
    assert_eq!(
        kind(&|_: &Path| Err("taskq.json: repo is required".into())),
        ("old".into(), "taskq.json: repo is required".into()),
        "the verified reason"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn add_generation_ignores_late_picker_or_install_results_after_cancel() {
    let mut adding = json!({"state":"picking","generation":4});
    assert!(apply_added(
        &mut adding,
        4,
        4,
        json!({"state":"picked","generation":4,"path":"/picked"})
    ));
    assert_eq!(adding["state"], "picked");
    adding = Value::Null;
    assert!(
        !apply_added(&mut adding, 5, 4, json!({"state":"picked","generation":4})),
        "cancelled picker cannot restore a selection"
    );
    adding = json!({"state":"installing","generation":6});
    assert!(
        !apply_added(
            &mut adding,
            6,
            5,
            json!({"state":"installed","generation":5})
        ),
        "another generation cannot claim this install"
    );
    assert!(apply_added(
        &mut adding,
        6,
        6,
        json!({"state":"installed","generation":6})
    ));
    assert_eq!(adding["state"], "installed");
    assert!(apply_added(
        &mut adding,
        6,
        6,
        json!({"state":"cancelled","generation":6})
    ));
    assert!(
        adding.is_null(),
        "a matching picker cancel clears its presentation"
    );
}

#[test]
fn add_owner_keeps_picker_admission_after_cancel_until_its_result() {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(ADD_IDLE));
    let job = |generation| {
        AddJob::Pick(QueuedAction {
            id: format!("add-{generation}"),
            root: PathBuf::new(),
            message: json!({"a":"add","generation":generation}),
            context: None,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        })
    };
    add_submit(&sender, &state, job(4)).unwrap();
    let mut adding = json!({"state":"picking dismissed","generation":5});
    assert!(
        !apply_added(&mut adding, 5, 4, json!({"state":"picked","generation":4})),
        "cancelled presentation cannot take a late picker result"
    );
    assert!(
        add_submit(&sender, &state, job(6))
            .unwrap_err()
            .contains("already in progress"),
        "cancel did not release the owned picker"
    );
    assert!(matches!(receiver.recv().unwrap(), AddJob::Pick(_)));
    state.store(ADD_IDLE, std::sync::atomic::Ordering::Release);
    add_submit(&sender, &state, job(6)).unwrap();
}

#[test]
fn owned_effect_reports_keep_their_observable_outcomes() {
    let answer = answer_reply(
        PathBuf::from("registered"),
        "tray.1".into(),
        Err("registration changed".into()),
    );
    assert_eq!(answer["answerReceipt"]["root"], "registered");
    assert_eq!(answer["answerReceipt"]["id"], "tray.1");
    assert_eq!(answer["answerReceipt"]["state"], "unknown");
    assert_eq!(
        resources_reply(Ok(json!({}))),
        json!({"resources_receipt":{"state":"submitted","reason":"Resources terminal submitted"}})
    );
    assert_eq!(
        resources_reply(Err("launcher unavailable".into()))["resources_receipt"]["state"],
        "unknown"
    );
    assert_eq!(
        resources_reply(Err(
            "tray.resources.submit: timed out; outcome unknown".into()
        ))["resources_receipt"]["state"],
        "unknown"
    );
    let (mut shown, mut workspace_shown, mut settings_shown) =
        (String::from("ui"), String::from("ws"), String::from("st"));
    assert!(!capmsg_refresh(
        &answer,
        &mut shown,
        &mut workspace_shown,
        &mut settings_shown
    ));
    assert_eq!(
        (
            shown.as_str(),
            workspace_shown.as_str(),
            settings_shown.as_str()
        ),
        ("ui", "ws", "st")
    );
    assert!(capmsg_refresh(
        &json!({"capmsg":{"root":"registered","id":"op-1","state":"saved","saved_revision":"4".repeat(32)}}),
        &mut shown,
        &mut workspace_shown,
        &mut settings_shown
    ));
    assert!(shown.is_empty() && workspace_shown.is_empty() && settings_shown.is_empty());
}

// A caught effect panic is visible and cannot silently reopen native admission.
#[test]
fn add_outcomes_capture_failures_and_hold_unknown_ownership() {
    crate::tests::with_test_capture(|store, _| {
        for state in ["picked", "cancelled", "pick failed", "install failed"] {
            let (value, owner) = add_outcome("tray.add.fixture", || json!({"state":state}));
            assert_eq!(value["state"], state);
            assert_eq!(owner, ADD_IDLE);
        }
        let (value, owner) = add_outcome("tray.add.fixture", || panic!("owned fixture panic"));
        assert_eq!(value["state"], "add failed");
        assert_eq!(owner, ADD_UNAVAILABLE);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(owner));
        let job = AddJob::Pick(QueuedAction {
            id: "after-panic".into(),
            root: PathBuf::new(),
            message: json!({"generation":2}),
            context: None,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        });
        assert!(
            add_submit(&sender, &state, job)
                .unwrap_err()
                .contains("unavailable")
        );
        assert!(receiver.try_recv().is_err());
        let worker = std::thread::spawn(|| {});
        let until = Instant::now() + Duration::from_secs(3);
        while !worker.is_finished() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        let owner = AddOwner {
            sender,
            state,
            _worker: worker,
        };
        assert!(
            owner
                .submit(3, None)
                .unwrap_err()
                .contains("worker stopped")
        );
        tracing::dispatcher::get_default(|d| {
            d.downcast_ref::<coreq::telemetry::capture::Capture>()
                .unwrap()
                .flush()
                .unwrap()
        });
        let page = store.export(0, 0, 1024).unwrap();
        let terminals: Vec<_> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["observation"])
            .filter(|r| {
                r["operation"] == "taskq.operation.tray.add.fixture" && r["kind"] == "method"
            })
            .collect();
        assert_eq!(terminals.len(), 5);
        assert_eq!(
            terminals
                .iter()
                .filter(|r| r["outcome"] == "failed")
                .count(),
            3
        );
        assert_eq!(terminals.last().unwrap()["error"], "add-panic");
        assert!(
            page["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(
                    |r| r["observation"]["operation"] == "taskq.operation.tray.add.dispatch"
                        && r["observation"]["error"] == "add-admission"
                )
        );
    });
}

#[cfg(unix)]
#[test]
fn picker_result_keeps_roots_and_distinguishes_native_cancel_from_error() {
    use std::os::unix::process::ExitStatusExt;
    let output = |code, stdout: &[u8], stderr: &[u8]| std::process::Output {
        status: std::process::ExitStatus::from_raw(code),
        stdout: stdout.into(),
        stderr: stderr.into(),
    };
    assert_eq!(
        pick_result(output(0, b"/\n", b"")),
        Ok(Some(PathBuf::from("/")))
    );
    assert_eq!(
        pick_result(output(0, b" /picked \r\n", b"")),
        Ok(Some(PathBuf::from(" /picked "))),
        "only the protocol newline is removed"
    );
    assert!(
        pick_result(output(0, &[0xff, b'\n'], b""))
            .unwrap_err()
            .contains("non-UTF-8")
    );
    assert_eq!(
        pick_result(output(
            1 << 8,
            b"",
            b"execution error: User canceled. (-128)\n"
        )),
        Ok(None)
    );
    assert!(pick_result(output(1 << 8, b"", b"execution error: denied. (-10004)\n")).is_err());
}
// Chain: canonical session -> native row -> shared task link (#1204, #1565). Risk: a Codex thread opens off its host
// instead of its DashQ progress page, a remote Claude row loses its account-scoped link, or a starting observation opens anything.
#[test]
fn project_row_links_claude_anywhere_codex_only_locally() {
    let url = "https://claude.ai/code/session_014abc";
    let mut row = json!({"n": 938, "session": "codex:valid-thread win"});
    task_session(Path::new("."), "win", None, &mut row);
    assert_eq!(
        (row["session_url"].clone(), row["runtime"].clone()),
        (json!("codex://threads/valid-thread"), json!("codex")),
        "local codex: deep link"
    );
    assert_eq!(row["native_session_url"], row["session_url"]);
    assert!(row["progress_url"].is_null());
    let mut codex = json!({"session": "codex:valid-thread mac"});
    task_session(Path::new("."), "win", None, &mut codex);
    assert!(codex["session_url"].is_null());
    assert_eq!(
        codex["session_reason"], "Codex session opens only on mac",
        "remote codex: no link, reason"
    );
    let mut paged = json!({"n": 7, "session": "codex:valid-thread mac"});
    task_session(
        Path::new("."),
        "win",
        Some("https://d/projects#board-o%2Fr-task-"),
        &mut paged,
    );
    assert_eq!(
        paged["session_url"], "https://d/projects#board-o%2Fr-task-7",
        "remote codex: DashQ progress page, never codex://"
    );
    assert_eq!(paged["progress_url"], paged["session_url"]);
    assert!(paged["native_session_url"].is_null());
    assert_eq!(
        paged["native_session_reason"],
        "Codex session opens only on mac"
    );
    for runtime in ["codex", "codex-app"] {
        let mut app = json!({"n": 7, "session": format!("{runtime}:valid-thread win")});
        task_session(Path::new("."), "win", None, &mut app);
        assert_eq!(app["native_session_url"], "codex://threads/valid-thread");
        task_session(
            Path::new("."),
            "mac",
            Some("http://relay.example/progress/"),
            &mut app,
        );
        assert_eq!(app["progress_url"], "http://relay.example/progress/7");
        assert!(app["native_session_url"].is_null());
    }
    let mut remote = json!({"session": "claude:valid-thread mac", "session_url": url});
    task_session(Path::new("."), "win", None, &mut remote);
    assert_eq!(
        (
            remote["local_session"].clone(),
            remote["session_url"].clone()
        ),
        (json!(false), json!(url)),
        "remote claude: link"
    );
    assert_eq!(remote["native_session_url"], url);
    let mut both = json!({"n": 7, "session": "claude:valid-thread mac", "session_url": url});
    task_session(
        Path::new("."),
        "win",
        Some("https://relay.example/progress/"),
        &mut both,
    );
    assert_eq!(both["native_session_url"], url);
    assert_eq!(both["progress_url"], "https://relay.example/progress/7");
    let mut forged = json!({"session": "claude:valid-thread mac", "session_url": "https://evil.example/session_1"});
    task_session(Path::new("."), "win", None, &mut forged);
    assert!(
        forged["session_url"].is_null(),
        "only a canonical claude.ai URL"
    );
    let mut unpublished = json!({"session": "claude:valid-thread mac"});
    task_session(Path::new("."), "win", None, &mut unpublished);
    assert!(unpublished["session_url"].is_null() && unpublished["session_reason"].is_null());
    let mut invalid = json!({"session": "claude:bad/path win", "session_url": url});
    task_session(Path::new("."), "win", None, &mut invalid);
    assert!(invalid["session_url"].is_null());
    let mut local = json!({"session": "claude:valid-thread win"});
    task_session(Path::new("."), "win", None, &mut local);
    assert!(
        local["session_url"].is_null(),
        "a local UUID does not prove remote registration"
    );
    let mut starting = json!({"session": "claude:- mac", "session_url": url});
    task_session(Path::new("."), "win", None, &mut starting);
    assert!(starting["session_url"].is_null(), "starting: no link");
    task_session(Path::new("."), "win", None, &mut Value::Null);
}
#[test]
fn task_pin_keeps_shared_views_and_failed_storage() {
    let dir = std::env::temp_dir().join(format!("taskq-pin-{}", crate::runtime::uuid()));
    let root = dir.join("project");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/alpha"}"#,
    )
    .unwrap();
    let mut settings = Settings::load(&dir);
    let st = json!({"projects":[{"root":root,"tasks":[{"n":7,"title":"Keep source","url":"https://example.org/7"}]}]});
    let mut m = json!({"root":root,"task":7,"pinned":true});
    assert_eq!(
        task_pin(&mut settings, &dir, &st, &m).unwrap()["pins"][0]["ref"],
        "github://github.com/invented/alpha#7"
    );
    assert_eq!(
        Settings::load(&dir).navigation,
        settings.navigation,
        "pin survives readback without a prior dashboard"
    );
    let reference = "github://github.com/invented/alpha#7";
    let observed = json!({"ref":reference,"pinned":true,"closedSeen":1});
    let first =
        task_pin(&mut settings, &dir, &st, &observed).unwrap()["pins"][0]["closedSeen"].clone();
    assert!(
        first.as_i64().unwrap() > 1,
        "first verified observation uses the host clock"
    );
    assert_eq!(
        task_pin(&mut settings, &dir, &st, &observed).unwrap()["pins"][0]["closedSeen"],
        first,
        "refresh cannot renew the 24h lifetime"
    );
    assert_eq!(
        Settings::load(&dir).navigation,
        settings.navigation,
        "first observation survives restart"
    );
    assert!(
        task_pin(
            &mut settings,
            &dir,
            &st,
            &json!({"ref":reference,"pinned":true,"closedSeen":0})
        )
        .unwrap()["pins"][0]["closedSeen"]
            .is_null(),
        "reopened live task clears its old observation without unpinning"
    );
    assert!(
        task_pin(
            &mut settings,
            &dir,
            &st,
            &json!({"ref":"invented-missing","pinned":true,"closedSeen":1})
        )
        .is_err(),
        "observation never creates another pin"
    );
    settings.navigation["dashboard"]["pins"]
        .as_array_mut()
        .unwrap()
        .push(json!({"ref":"invented-saved","title":"Unavailable"}));
    let removed = task_pin(
        &mut settings,
        &dir,
        &st,
        &json!({"ref":"invented-saved","pinned":false}),
    )
    .expect("F19: unavailable saved pin can be removed by exact reference");
    assert!(
        !removed["pins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["ref"] == "invented-saved")
    );
    let mut finished = st.clone();
    finished["projects"][0]["reports"] =
        json!({"reports":[{"n":9,"title":"Finished task","url":"https://example.org/9"}]});
    assert!(
        task_pin(
            &mut settings,
            &dir,
            &finished,
            &json!({"root":root,"task":9,"pinned":true})
        )
        .unwrap()["pins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pin| pin["ref"] == "github://github.com/invented/alpha#9"),
        "Finished header can pin a source-backed closed task"
    );
    let before = settings.navigation.clone();
    m["task"] = json!(8);
    assert!(task_pin(&mut settings, &dir, &st, &m).is_err());
    assert_eq!(settings.navigation, before);
    fs::remove_file(dir.join("tray.json")).unwrap();
    fs::create_dir(dir.join("tray.json")).unwrap();
    m["task"] = json!(7);
    m["pinned"] = json!(false);
    assert!(
        task_pin(&mut settings, &dir, &st, &m)
            .unwrap_err()
            .contains("save/readback")
    );
    assert_eq!(
        settings.navigation, before,
        "unknown persistence retains prior pins in memory"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn conversations_recovery_storage_and_generation() {
    let dir = std::env::temp_dir().join(format!("taskq-2140-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut settings = Settings::load(&dir);
    let nav = json!({"schema":1,"current":{"page":"conversations","answer":{"root":"invented","task":112,"revision":71}},"trail":[]});
    assert_eq!(save_workspace_navigation(&mut settings, &dir, &nav), None);
    assert_eq!(Settings::load(&dir).workspace_navigation, nav);
    assert!(
        save_workspace_navigation(
            &mut settings,
            &dir,
            &json!({"schema":1,"text":"界".repeat(22000)})
        )
        .unwrap()
        .contains("64 KiB")
    );
    assert_eq!(settings.workspace_navigation, nav);
    fs::remove_file(dir.join("tray.json")).unwrap();
    fs::create_dir(dir.join("tray.json")).unwrap();
    assert!(
        save_workspace_navigation(&mut settings, &dir, &json!({"schema":1,"current":{}}))
            .unwrap()
            .contains("storage")
    );
    assert_eq!(
        settings.workspace_navigation, nav,
        "failed save retains previous navigation"
    );
    assert!(
        workspace_operation("completed"),
        "reader transcript reads reach the existing adapter"
    );
    assert!(
        !workspace_operation("answer"),
        "writes retain the workspace authority boundary"
    );
    let old = WebCalls::new();
    let fresh = WebCalls::new();
    assert_ne!(
        old.generation, fresh.generation,
        "retired reader IPC/callbacks are fenced"
    );
    let prior = json!({"conversation":{"drafts":[{"text":"Unsaved local edits","caret":[2,4]}]}});
    for surface in ["main", "invented-detached"] {
        let mut checkpoints = std::collections::BTreeMap::from([(surface.into(), prior.clone())]);
        let mut reader = WebCalls::new();
        reader.safe = true;
        assert!(!workspace_checkpoint(&reader, &mut checkpoints, surface, &json!({})).unwrap());
        reader.finish(0, "ok");
        assert!(
            !workspace_checkpoint(
                &reader,
                &mut checkpoints,
                surface,
                &json!({"snapshot":"unrestored"})
            )
            .unwrap()
        );
        assert_eq!(
            checkpoints[surface], prior,
            "boot and snapshot cannot overwrite recovery edits"
        );
        reader.recovery_ready = true;
        let restored = json!({"conversation":{"drafts":[{"text":"Restored edits","caret":[2,4]}]}});
        assert!(workspace_checkpoint(&reader, &mut checkpoints, surface, &restored).unwrap());
        assert_eq!(checkpoints[surface], restored);
        let next = WebCalls::new();
        assert!(!workspace_checkpoint(&next, &mut checkpoints, surface, &json!({})).unwrap());
        assert_eq!(
            checkpoints[surface], restored,
            "next generation must hydrate before replacing edits"
        );
    }

    fs::remove_dir_all(dir).unwrap();
}
/// #1548 fixture: one steady tray tick's own rows (push, its JS render summary, the fit resize); the host hour test owns the gate.
pub(crate) fn steady_tick() -> impl FnMut() {
    let mut calls = WebCalls::new();
    calls.pending.clear();
    move || {
        let id = calls.begin(TICK, Instant::now()).expect("tick push");
        calls.finish(id, "ok");
        let render = json!({"operation":"js.ui.render.tick","kind":"method.aggregate","started_ms":crate::profile::now_ms().unwrap(),"counts":{"calls":1,"finished":1,"failed":0,"unknown":0},"outcome":"ok","error":"none"});
        assert!(!calls.metadata(&render) && !calls.failed);
        let _ = coreq::telemetry::capture::poll(
            "tray.window.resize",
            0,
            || Ok::<_, &str>(()),
            |_| "observed",
        );
    }
}

// The 2 s refresh reads a local session's files at most once per TTL; a changed row or an expired entry reads again.
#[test]
fn session_link_reads_files_once_per_ttl_across_refreshes() {
    let mut links = SessionLinks::new();
    let start = Instant::now();
    let reads = std::cell::Cell::new(0);
    let read = |url: &str| {
        reads.set(reads.get() + 1);
        Some(url.to_string())
    };
    for tick in 0..15u64 {
        let now = start + Duration::from_secs(tick * 2);
        let url = remembered_link(&mut links, "a".into(), now, || read("one"));
        assert_eq!(url.as_deref(), Some("one"));
    }
    assert_eq!(reads.get(), 1, "a 30 s window of 2 s ticks reads once");
    remembered_link(&mut links, "b".into(), start, || read("two"));
    assert_eq!(reads.get(), 2, "another attempt has its own entry");
    let later = start + SESSION_LINK_TTL;
    let url = remembered_link(&mut links, "a".into(), later, || read("new"));
    assert_eq!(url.as_deref(), Some("new"), "an expired link reads again");
    assert_eq!(links.len(), 1, "expired entries leave the map");
}

// #tray CPU: a refresh inside the window serves the remembered link, without rereading the attempt log.
#[test]
fn local_session_row_does_not_reread_its_log_each_refresh() {
    let root = std::env::temp_dir().join(format!("taskq-link-{}", crate::runtime::uuid()));
    let log = crate::runtime::log_path(
        &root,
        &crate::runtime::Launch {
            files: String::new(),
            execution: None,
            task: 7,
            title: String::new(),
            attempt: 1,
            role: String::new(),
            rt: "codex".into(),
            session: "-".into(),
            url: String::new(),
            resume: None,
        },
    );
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    let write = |thread: &str| {
        let line = format!("{{\"type\":\"thread.started\",\"thread_id\":\"{thread}\"}}\n");
        std::fs::write(&log, line).unwrap();
    };
    let refresh = || {
        let mut row = json!({"n": 7, "attempt": 1, "session": "codex:- mac"});
        task_session(&root, "mac", None, &mut row);
        row["session_url"].clone()
    };
    write("first-thread");
    assert_eq!(refresh(), "codex://threads/first-thread");
    write("second-thread");
    assert_eq!(
        refresh(),
        "codex://threads/first-thread",
        "no reread inside the window"
    );
    std::fs::remove_dir_all(root).unwrap();
}
