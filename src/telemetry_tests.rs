use super::*;
fn dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "taskq-sqlite-{}-{}",
        std::process::id(),
        crate::runtime::uuid()
    ));
    std::fs::create_dir(&d).unwrap();
    d
}
// R12/#1465: settled receipts must release their collector before its installation can be removed.
#[test]
fn receipt_settlement_waits_for_dispatch_release() {
    if std::env::var_os("TASKQ_RECEIPT_TEARDOWN_TEST").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "telemetry::tests::receipt_settlement_waits_for_dispatch_release",
                "--nocapture",
            ])
            .env("TASKQ_RECEIPT_TEARDOWN_TEST", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;
    use tracing_subscriber::prelude::*;
    struct Release {
        entered: mpsc::Sender<()>,
        resume: Mutex<mpsc::Receiver<()>>,
    }
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Release {}
    impl Drop for Release {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.resume
                .get_mut()
                .unwrap()
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
        }
    }
    let d = dir();
    let store = Arc::new(coreq::telemetry::Store::open(&d.join("universal.sqlite3")).unwrap());
    let capture = coreq::telemetry::capture::Capture::new(store, "taskq", "test").unwrap();
    let (entered, dropping) = mpsc::channel();
    let (resume, wait) = mpsc::channel();
    let subscriber = tracing_subscriber::registry().with(capture).with(Release {
        entered,
        resume: Mutex::new(wait),
    });
    let (command, dispatch) = tracing::subscriber::with_default(subscriber, || {
        let _span = tracing::info_span!("host.install").entered();
        (
            coreq::telemetry::capture::CommandReceipt::current().unwrap(),
            tracing::dispatcher::get_default(Clone::clone),
        )
    });
    let worker = receipt_worker();
    *RECEIPTS.0.lock().unwrap() += 1;
    worker.send((command, "observed".into(), dispatch)).unwrap();
    drop(worker);
    dropping.recv_timeout(Duration::from_secs(30)).unwrap();
    let premature = settle(Duration::from_millis(25));
    resume.send(()).unwrap();
    assert!(
        settle(Duration::from_secs(30)),
        "receipt worker did not finish dispatcher teardown"
    );
    assert!(
        !premature,
        "receipt settled while its dispatcher still owned the collector"
    );
    std::fs::remove_dir_all(d).unwrap();
}
// #2329: both fixture processes acknowledge on a slow disk (as CoreQ's fixtures do), and the parent's waits outlast
// that deadline. Production keeps the 3 s default; the fixture proves handover, not append speed.
const SLOW_DISK: std::time::Duration = std::time::Duration::from_secs(60);
const HANDOVER_BUDGET: std::time::Duration = std::time::Duration::from_secs(150);
// A2/#1309: an older tray owns an incompatible IPC writer when the new host starts.
// The heartbeat is the existing tray exit protocol; history and a new durable receipt must survive.
#[test]
fn older_tray_writer_hands_over_to_new_host() {
    use std::process::{Command, Stdio};
    for fault in ["wire", "unresponsive", "stale", "late"] {
        let d = dir();
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
                        "telemetry::tests::tray_handover_process",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("TASKQ_HANDOVER_FIXTURE", &d)
                    .env("TASKQ_HANDOVER_MODE", mode)
                    .env("TASKQ_HANDOVER_FAULT", fault)
                    .stdin(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        };
        let mut tray = spawn("tray");
        let until = std::time::Instant::now() + HANDOVER_BUDGET;
        while !d.join("ready").exists() {
            assert!(
                tray.0.try_wait().unwrap().is_none(),
                "old tray failed before readiness"
            );
            assert!(std::time::Instant::now() < until, "old tray did not start");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let age = |age| {
            std::fs::File::options()
                .write(true)
                .open(d.join("tray"))
                .unwrap()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age))
                .unwrap()
        };
        if fault == "stale" || fault == "late" {
            age(20);
        }
        let mut host = spawn("host");
        // #1336: the tray heartbeat turns fresh after the host started without a collector.
        if fault == "late" {
            std::thread::sleep(std::time::Duration::from_secs(2));
            age(0);
        }
        loop {
            if let Some(status) = host.0.try_wait().unwrap() {
                assert!(status.success(), "new host could not record its receipt");
                break;
            }
            assert!(std::time::Instant::now() < until, "handover did not finish");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if fault == "stale" {
            assert_eq!(
                std::fs::read_to_string(d.join("tray")).unwrap(),
                tray.0.id().to_string(),
                "unknown owner was displaced"
            );
            assert!(
                tray.0.try_wait().unwrap().is_none(),
                "unknown owner was stopped"
            );
            std::fs::write(d.join("tray"), "fixture finished").unwrap();
        }
        assert!(tray.0.wait().unwrap().success());
        let reader =
            coreq::telemetry::Reader::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
        let rows = reader.export(0, 0, 1024).unwrap().to_string();
        assert!(rows.contains("old.tray"), "handover lost earlier history");
        assert_eq!(
            rows.contains("new.host.receipt"),
            fault != "stale",
            "host receipt durability or refusal changed"
        );
        drop(reader);
        std::fs::remove_dir_all(d).unwrap();
    }
}

// #1755 A3: lost batches log one unknown line and the next commit logs recovery; a quiet collector logs nothing.
// Risk: a sticky "Telemetry: unknown" line reads as a dead collector while commits resumed.
#[test]
fn collector_state_reports_loss_then_recovery_once() {
    let (lost, ok) = (
        "Telemetry: unknown (observations lost); collector retries",
        "Telemetry: collecting",
    );
    assert_eq!(capture_state((0, 5), (0, 5), ok), ok);
    assert_eq!(capture_state((0, 5), (3, 5), ok), lost);
    assert_eq!(capture_state((3, 5), (3, 5), lost), lost);
    assert_eq!(
        capture_state((3, 5), (4, 9), lost),
        lost,
        "a commit alongside a new loss is not recovery"
    );
    assert_eq!(capture_state((4, 9), (4, 10), lost), ok);
}

#[test]
#[ignore = "owned older-tray/new-host process fixture"]
fn tray_handover_process() {
    let d = std::path::PathBuf::from(std::env::var_os("TASKQ_HANDOVER_FIXTURE").unwrap());
    if std::env::var("TASKQ_HANDOVER_MODE").unwrap() == "tray" {
        let runtime = install_capture(&d).unwrap();
        runtime.acknowledge_within(SLOW_DISK).unwrap();
        let span = tracing::info_span!("old.tray");
        {
            let _entered = span.enter();
        }
        drop(span);
        coreq::telemetry::capture::qualify().unwrap();
        let endpoint = d.join("telemetry.log/telemetry-ipc/endpoint");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&endpoint).unwrap()).unwrap();
        if std::env::var("TASKQ_HANDOVER_FAULT").unwrap() != "unresponsive" {
            value["wire"] = serde_json::json!(1);
        } else {
            value["key"] = serde_json::json!(vec![0u8; 32]);
        } // Keep the writer alive but refuse the handshake.
        std::fs::write(endpoint, serde_json::to_vec(&value).unwrap()).unwrap();
        let pid = std::process::id().to_string();
        std::fs::write(d.join("tray"), &pid).unwrap();
        std::fs::write(d.join("ready"), "").unwrap();
        let until = std::time::Instant::now() + HANDOVER_BUDGET;
        while std::fs::read_to_string(d.join("tray")).unwrap() == pid {
            assert!(
                std::time::Instant::now() < until,
                "old tray never received the handover"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    } else {
        if std::env::var("TASKQ_HANDOVER_FAULT").unwrap() == "stale" {
            let Err(error) = host_capture(&d) else {
                panic!("stale tray recovery succeeded")
            };
            assert!(error.contains("tray owner not fresh"), "{error}");
            return;
        }
        let mut capture = HostCapture::default();
        if std::env::var("TASKQ_HANDOVER_FAULT").unwrap() == "late" {
            let first = capture.pass(&d).unwrap();
            assert!(first.contains("tray owner not fresh"), "{first}");
            let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while capture.runtime.is_none() {
                assert!(
                    std::time::Instant::now() < until,
                    "host never retried the fresh tray"
                );
                std::thread::sleep(std::time::Duration::from_millis(100));
                if let Some(line) = capture.pass(&d) {
                    assert_eq!(line, "Telemetry: collecting", "one line per state change");
                }
            }
        } else {
            assert_eq!(
                capture.pass(&d).as_deref(),
                Some("Telemetry: collecting"),
                "new host collector unavailable"
            );
        }
        capture
            .runtime
            .as_ref()
            .unwrap()
            .acknowledge_within(SLOW_DISK)
            .unwrap();
        let span = tracing::info_span!("new.host.receipt");
        let _entered = span.enter();
        coreq::telemetry::capture::CommandReceipt::current()
            .unwrap()
            .checkpoint("observed")
            .unwrap();
        coreq::telemetry::capture::qualify().unwrap();
    }
}

fn spawn_event(at_ms: Option<u64>) -> coreq::telemetry::Event {
    coreq::telemetry::Event::new(
        semantic::encode(
            "operation",
            serde_json::json!({"op":"spawn","us":1,"error":"none","at_ms":at_ms}),
        )
        .unwrap(),
    )
    .unwrap()
}
fn store_at(d: &Path) -> coreq::telemetry::Store {
    std::fs::create_dir_all(d.join("telemetry.log")).unwrap();
    coreq::telemetry::Store::open(&d.join("telemetry.log/universal.sqlite3")).unwrap()
}
// A6: a retired archive left on disk stays counted in the peer share and never shrinks the 384 MiB store cap.
#[test]
fn live_capacity_counts_a_retired_archive_without_deleting_it() {
    let d = dir();
    std::fs::create_dir(d.join("telemetry.log")).unwrap();
    let path = d.join("telemetry.log/universal.sqlite3");
    let live =
        coreq::telemetry::Store::open_bounded(&path, budget(&d, "telemetry.sqlite3").unwrap())
            .unwrap();
    let before = live.storage().unwrap()["database_limit"].as_i64().unwrap();
    let archive = d.join("telemetry.log/telemetry.sqlite3");
    std::fs::File::create(&archive)
        .unwrap()
        .set_len(8 << 20)
        .unwrap();
    live.append(&[spawn_event(Some(2))]).unwrap();
    let storage = live.storage().unwrap();
    assert_eq!(
        (before, storage["database_limit"].as_i64().unwrap()),
        (384 << 20, 384 << 20)
    );
    assert!(
        storage["installation_accounted_bytes"].as_u64().unwrap()
            <= coreq::telemetry::INSTALLATION_BYTES / 2
    );
    assert_eq!(std::fs::metadata(&archive).unwrap().len(), 8 << 20);
    drop(live);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn live_semantics_use_the_installed_capture_and_leave_no_second_sink() {
    use tracing_subscriber::prelude::*;
    let d = dir();
    std::fs::create_dir(d.join("telemetry.log")).unwrap();
    let store = std::sync::Arc::new(
        coreq::telemetry::Store::open_bounded(
            &d.join("telemetry.log/universal.sqlite3"),
            budget(&d, "telemetry.sqlite3").unwrap(),
        )
        .unwrap(),
    );
    let capture = coreq::telemetry::capture::Capture::new(store.clone(), "taskq", "test").unwrap();
    let ops = (0..1200)
        .map(|mut i| {
            let mut route = String::from("route");
            for _ in 0..4 {
                route.push((b'a' + (i % 26) as u8) as char);
                i /= 26;
            }
            (format!("GET issues/N/{route}"), 1)
        })
        .collect::<std::collections::BTreeMap<_, u64>>();
    let bytes = serde_json::to_vec(&ops).unwrap().len();
    assert!(bytes > 16 << 10 && bytes < LINE_MAX - 1024);
    tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()), || {
        let _action = tracing::info_span!("fixture.command").entered();
        record(Sample::fixture(r#"taskq-stage {"stage":"claim","task":1,"attempt":2,"rt":"other","us":1,"outcome":"ok","error":"none","at_ms":1,"unknown":"no-model"}"#).unwrap()).unwrap();
        for line in [
            r#"taskq-profile {"op":"spawn","us":1,"error":"none","at_ms":1}"#,
            r#"taskq-board {"op":"close","us":12,"outcome":"failed","error":"http4xx","remaining":null,"at_ms":1}"#,
            r#"taskq-transition {"project":"other","task":1,"entry":2,"from":"doing","to":"ask","prev_s":null,"rt":null,"host":null,"at_ms":1}"#,
        ] {
            record(Sample::fixture(line).unwrap()).unwrap();
        }
        crate::requests::record("worker service status", &ops);
    });
    capture.flush().unwrap();
    assert!(
        !d.join("telemetry.log/telemetry.sqlite3").exists(),
        "live semantic events must use the installed writer"
    );
    let rows = store.export_kinds(&["stage"], 0, 0, 1024).unwrap();
    let event = &rows["events"][0]["observation"];
    assert_eq!(event["version"], "test");
    assert!(event["trace"].is_string());
    let report = report_samples(&d, Sample::KINDS).unwrap();
    assert_eq!(report.len(), 5);
    for kind in Sample::KINDS {
        assert!(report.iter().any(|sample| sample.kind() == *kind), "{kind}");
    }
    let requests = report
        .iter()
        .find(|sample| sample.kind() == "requests")
        .unwrap()
        .canonical()
        .unwrap();
    assert_eq!(requests["ops"], serde_json::to_value(&ops).unwrap());
    assert!(
        crate::requests::sources_samples(&report, crate::profile::now_ms().unwrap())
            .contains("worker service status")
    );
    let parts = store.export_kinds(&["requests"], 0, 0, 1024).unwrap();
    let parts = parts["events"].as_array().unwrap();
    assert!(parts.len() > 1);
    assert!(
        parts
            .iter()
            .all(|part| part["observation"]["group_id"].is_string())
    );
    drop(capture);
    let db = rusqlite::Connection::open(d.join("telemetry.log/universal.sqlite3")).unwrap();
    assert_eq!(
        db.execute(
            "DELETE FROM events WHERE id=?1",
            [parts.last().unwrap()["id"].as_str().unwrap()]
        )
        .unwrap(),
        1
    );
    drop(db);
    assert!(
        report_samples(&d, &["requests"])
            .err()
            .unwrap()
            .contains("incomplete count summary")
    );
    assert!(
        crate::profile::report(&d)
            .contains("Profile: unknown (telemetry incomplete count summary)")
    );
    drop(store);
    std::fs::remove_dir_all(d).unwrap();
}

// Owner decision on #2427: retired archive rows leave reports and their files stay on disk, unread. Risk kept: export,
// a report or the service pass reads, rewrites or counts `telemetry.sqlite3` or telemetry text in taskq.log.
#[test]
fn retired_archive_and_log_records_stay_on_disk_unread() {
    let d = dir();
    let store = store_at(&d);
    store
        .append(&[spawn_event(Some(2000)), spawn_event(None)])
        .unwrap();
    drop(store);
    let archive = d.join("telemetry.log/telemetry.sqlite3");
    std::fs::write(&archive, b"retired archive").unwrap();
    let log = d.join("taskq.log");
    let old = "taskq-profile {\"op\":\"spawn\",\"us\":7,\"error\":\"none\",\"at_ms\":1}\ntaskq-profile {truncated";
    std::fs::write(&log, old).unwrap();
    let output = export(&d, None).unwrap();
    let rows: Vec<serde_json::Value> = output
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["coverage"], "committed-snapshot");
    assert_eq!(
        rows[0]["sources"],
        serde_json::json!({"universal":rows[0]["sources"]["universal"]})
    );
    assert_eq!(rows[0]["sources"]["universal"]["samples"], 2);
    assert!(rows[1..].iter().all(|r| r["source"] == "universal"));
    assert_eq!(
        rows[2]["observation"]["started_ms"],
        serde_json::Value::Null
    );
    assert_eq!(
        rows[2]["observation"]["unknown"]["started_ms"],
        "not-recorded"
    );
    assert_eq!(report_samples(&d, &["operation"]).unwrap().len(), 2);
    crate::profile::prune(&d).unwrap();
    crate::platform::sweep(&d);
    assert_eq!(std::fs::read(&archive).unwrap(), b"retired archive");
    assert_eq!(std::fs::read_to_string(&log).unwrap(), old);
    // Rotation keeps the newest half whatever the log holds.
    let big = "service diagnostic line\n".repeat(crate::data::LOG_MAX as usize / 24 + 1);
    std::fs::write(&log, format!("{old}\n{big}")).unwrap();
    crate::profile::prune(&d).unwrap();
    let kept = std::fs::read(&log).unwrap();
    assert!(kept.len() as u64 <= crate::data::LOG_MAX / 2 && big.as_bytes().ends_with(&kept));
    assert_eq!(std::fs::read(&archive).unwrap(), b"retired archive");
    std::fs::remove_dir_all(d).unwrap();
}
// Risk: a store row that does not decode reads as a healthy empty report.
#[test]
fn undecodable_store_row_makes_the_report_unknown() {
    let d = dir();
    let store = store_at(&d);
    let row = semantic::encode(
        "operation",
        serde_json::json!({"op":"spawn","us":1,"error":"secret","at_ms":2}),
    )
    .unwrap();
    store
        .append(&[coreq::telemetry::Event::new(row).unwrap()])
        .unwrap();
    drop(store);
    assert!(crate::profile::report(&d).starts_with("Profile: unknown"));
    std::fs::remove_dir_all(d).unwrap();
}
// R12/#2354: settled exports accept native Windows paths.
#[cfg(windows)]
#[test]
fn export_reads_plain_and_verbatim_paths() {
    let root = dir();
    let d = root.join("archive %23 # Unicode \u{e9}");
    std::fs::create_dir(&d).unwrap();
    let store = store_at(&d);
    store.append(&[spawn_event(Some(1))]).unwrap();
    let verbatim = d.canonicalize().unwrap();
    assert!(verbatim.to_str().unwrap().starts_with(r"\\?\"));
    let plain = crate::platform::plain_path(&verbatim);
    let output = [export(&plain, None), export(&verbatim, None)].map(Result::unwrap);
    assert_eq!(output[0], output[1]);
    let rows: Vec<serde_json::Value> = output[0]
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["observation"]["operation"], "spawn");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
// Chain: #2306 profile export in a worker sandbox. Risk: a reader that cannot write <install> fails on the store.
#[cfg(unix)]
#[test]
fn export_reads_the_store_from_a_read_only_install() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir();
    // the service keeps the universal store open, so its -wal/-shm exist
    let store = store_at(&d);
    store.append(&[spawn_event(Some(2000000))]).unwrap();
    let logs = d.join("telemetry.log");
    std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = export(&d, Some(1000));
    std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = out.unwrap();
    let head: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    assert_eq!(head["sources"]["universal"]["samples"], 1, "{out}");
    drop(store);
    std::fs::remove_dir_all(d).unwrap();
}
