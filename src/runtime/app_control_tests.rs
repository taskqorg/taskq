#[cfg(windows)]
use super::windows_pipe::tests::Listener;
use super::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, net::UnixListener as Listener};
// Chain: installed desktop -> absent exact owner -> explicit refusal before any interruption.
#[cfg(any(target_os = "macos", windows))]
#[test]
#[ignore = "read-only installed desktop IPC qualification; no model, queue or existing turn control"]
fn native_absent_owner_never_sends_interrupt() {
    let thread = super::super::uuid();
    let turn = super::super::uuid();
    let began = Instant::now();
    let error = cancel(&thread, &turn).unwrap_err();
    assert!(
        error.contains("thread-owner-discovery refused: no-client-found"),
        "{error}"
    );
    assert!(began.elapsed() < Duration::from_secs(11));
    println!(
        "native desktop same-user connection and absent-owner refusal passed; no interrupt or submission: {error}"
    );
}

// Chain: untrusted frame sizes -> cumulative wire cap, independent of native transfer speed.
#[test]
fn response_budget_counts_prefixes_and_refuses_overflow() {
    let mut received = 0;
    for _ in 0..3 {
        received = frame_end(received, FRAME_LIMIT).unwrap();
    }
    assert_eq!(
        frame_end(received, FRAME_LIMIT - 16).unwrap(),
        RESPONSE_LIMIT
    );
    assert!(
        frame_end(received, FRAME_LIMIT - 12).is_err(),
        "payload-only accounting would admit an extra prefix"
    );
    for (received, size) in [
        (0, 0),
        (0, FRAME_LIMIT + 1),
        (RESPONSE_LIMIT, 1),
        (usize::MAX, 1),
    ] {
        assert!(frame_end(received, size).is_err());
    }
}

// Chain: §8, R2, R9, R11, R12, #2157 A1/A2 -> passive writer lock release verifies completed turn, absent desktop owner, coordination lock, unheld OS lock and unchanged inode; live/owned/active/unknown writers stay untouched.
#[test]
fn passive_writer_lock_releases_only_confirmed_idle_unheld_locks() {
    let write_rollout = |home: &Path, project: &Path, thread: &str, state: &str| {
        let sessions = home.join("sessions/2026/11/02");
        fs::create_dir_all(&sessions).unwrap();
        let rollout = sessions.join(format!("rollout-2026-11-02T00-00-00-{thread}.jsonl"));
        let mut body = format!(
            "{}\n",
            json!({"timestamp":"2026-11-02T00:00:00Z","type":"session_meta","payload":{"id":thread,"timestamp":"2026-11-02T00:00:00Z","cwd":project,"originator":"taskq-2157-fixture","cli_version":"0.159.3","source":"cli","model_provider":"openai","forked_from_id":if state=="forked" {json!("01a10000-0000-7000-8000-000000000000")} else {Value::Null}}})
        );
        if state != "empty" {
            body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}})));
            body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:02Z","type":"event_msg","payload":{"type":"user_message","turn_id":"turn-1","message":"fixture prompt"}})));
        }
        match state {
                "completed"=>body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}))),
                "aborted"=>body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-1"}}))),
                "trailing-item"=>{
                    body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}})));
                    body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:04Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"turn-1","item":{"type":"CommandExecution"}}})));
                }
                _=>{}
            }
        fs::write(&rollout, body).unwrap();
    };
    for case in [
        "absent",
        "released-no-ipc",
        "released-aborted",
        "released-no-client-found",
        "released-discovery-timeout",
        "retained-desktop-owner",
        "retained-live-flock",
        "retained-coord-busy",
        "retained-active-turn",
        "retained-empty-rollout",
        "retained-trailing-item",
        "retained-wrong-cwd",
        "retained-forked",
        "retained-ipc-malformed",
        "retained-ipc-refused",
        "retained-lock-symlink",
        "retained-lock-mode",
        "retained-replaced-inode",
    ] {
        let scratch = std::env::temp_dir().join(format!("qpw-{}", super::super::uuid()));
        let home = scratch.join("home");
        let project = scratch.join("project");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700)).unwrap();
        let thread = super::super::uuid();
        let lock_dir = home.join("thread-writer-locks");
        fs::create_dir_all(&lock_dir).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&lock_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let lock_path = lock_dir.join(format!("{thread}.lock"));
        if case != "absent" {
            fs::write(&lock_path, b"").unwrap();
            #[cfg(unix)]
            fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(windows)]
        if matches!(
            case,
            "retained-lock-symlink" | "retained-lock-mode" | "retained-replaced-inode"
        ) {
            fs::remove_dir_all(scratch).unwrap();
            continue;
        }
        #[cfg(unix)]
        if case == "retained-lock-symlink" {
            let target = scratch.join("target.lock");
            fs::write(&target, b"").unwrap();
            fs::remove_file(&lock_path).unwrap();
            std::os::unix::fs::symlink(&target, &lock_path).unwrap();
        }
        #[cfg(unix)]
        if case == "retained-lock-mode" {
            fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o666)).unwrap();
        }
        let rollout_state = match case {
            "released-aborted" => "aborted",
            "retained-active-turn" => "active",
            "retained-empty-rollout" => "empty",
            "retained-trailing-item" => "trailing-item",
            "retained-forked" => "forked",
            _ => "completed",
        };
        let rollout_project = if case == "retained-wrong-cwd" {
            &scratch
        } else {
            &project
        };
        write_rollout(&home, rollout_project, &thread, rollout_state);

        let _live_lock = if case == "retained-live-flock" {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)
                .unwrap();
            Some(coreq::file_lock::try_lock_file(file).unwrap())
        } else {
            None
        };
        let _coord_lock = if case == "retained-coord-busy" {
            let coord = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(lock_dir.join(".coordination.lock"))
                .unwrap();
            #[cfg(unix)]
            fs::set_permissions(
                lock_dir.join(".coordination.lock"),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
            Some(coreq::file_lock::try_lock_file(coord).unwrap())
        } else {
            None
        };

        #[cfg(unix)]
        let ipc_path = scratch.join("s");
        #[cfg(windows)]
        let ipc_path =
            std::path::PathBuf::from(format!(r"\\.\pipe\taskq-pw-{}", super::super::uuid()));

        let (finished, completion) = std::sync::mpsc::channel();
        let server = if matches!(
            case,
            "released-no-client-found"
                | "released-discovery-timeout"
                | "retained-desktop-owner"
                | "retained-ipc-malformed"
                | "retained-ipc-refused"
                | "retained-replaced-inode"
        ) {
            let listener = Listener::bind(&ipc_path).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&ipc_path, fs::Permissions::from_mode(0o600)).unwrap();
            let lock_for_race = lock_path.clone();
            let mode = case.to_string();
            let expected_thread = thread.clone();
            Some(std::thread::spawn(move || -> std::io::Result<()> {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let mut size = [0; 4];
                stream.read_exact(&mut size)?;
                let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
                stream.read_exact(&mut bytes)?;
                let init: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(init["method"], "initialize");
                let init_reply=serde_json::to_vec(&json!({"type":"response","requestId":init["requestId"],"method":"initialize","resultType":"success","handledByClientId":"cli-1","result":{"clientId":"cli-1"}})).unwrap();
                stream.write_all(&(init_reply.len() as u32).to_le_bytes())?;
                stream.write_all(&init_reply)?;

                let mut size = [0; 4];
                stream.read_exact(&mut size)?;
                let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
                stream.read_exact(&mut bytes)?;
                let disc: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(disc["method"], "thread-owner-discovery");
                assert_eq!(disc["params"]["conversationId"], expected_thread);

                let ask = serde_json::to_vec(
                    &json!({"type":"client-discovery-request","requestId":"disc-1","request":disc}),
                )
                .unwrap();
                stream.write_all(&(ask.len() as u32).to_le_bytes())?;
                stream.write_all(&ask)?;
                let mut size = [0; 4];
                stream.read_exact(&mut size)?;
                let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
                stream.read_exact(&mut bytes)?;
                let cr: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(cr["type"], "client-discovery-response");
                assert_eq!(cr["response"]["canHandle"], false);

                #[cfg(unix)]
                if mode == "retained-replaced-inode" {
                    let replacement = lock_for_race.with_extension("replacement");
                    fs::write(&replacement, b"new").unwrap();
                    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
                    fs::remove_file(&lock_for_race).unwrap();
                    fs::rename(&replacement, &lock_for_race).unwrap();
                }

                match mode.as_str() {
                    "released-no-client-found" | "retained-replaced-inode" => {
                        let reply=serde_json::to_vec(&json!({"type":"response","requestId":disc["requestId"],"method":"thread-owner-discovery","resultType":"error","error":"no-client-found"})).unwrap();
                        stream.write_all(&(reply.len() as u32).to_le_bytes())?;
                        stream.write_all(&reply)?;
                    }
                    "released-discovery-timeout" => {
                        let _ = completion.recv_timeout(Duration::from_secs(5));
                        return Ok(());
                    }
                    "retained-desktop-owner" => {
                        let reply=serde_json::to_vec(&json!({"type":"response","requestId":disc["requestId"],"method":"thread-owner-discovery","resultType":"success","handledByClientId":"desktop-owner","result":{}})).unwrap();
                        stream.write_all(&(reply.len() as u32).to_le_bytes())?;
                        stream.write_all(&reply)?;
                    }
                    "retained-ipc-refused" => {
                        let reply=serde_json::to_vec(&json!({"type":"response","requestId":disc["requestId"],"method":"thread-owner-discovery","resultType":"error","error":"request-version-mismatch"})).unwrap();
                        stream.write_all(&(reply.len() as u32).to_le_bytes())?;
                        stream.write_all(&reply)?;
                    }
                    "retained-ipc-malformed" => {
                        stream.write_all(&[1, 0, 0, 0, b'{'])?;
                        return Ok(());
                    }
                    _ => {}
                }
                let _ = stream.read(&mut [0]);
                Ok(())
            }))
        } else {
            None
        };

        let outcome = release_passive_writer_at(
            &home,
            &project,
            &thread,
            &ipc_path,
            Instant::now() + Duration::from_millis(200),
        );
        let _ = finished.send(());
        if let Some(server) = server {
            let _ = server.join().unwrap();
        }
        match case {
            "absent" => assert_eq!(outcome, PassiveWriterOutcome::Absent, "{case}"),
            c if c.starts_with("released-") => {
                assert_eq!(
                    outcome,
                    PassiveWriterOutcome::Released,
                    "{case}: {outcome:?}"
                );
                assert!(
                    !lock_path.exists(),
                    "{case}: passive lock file should be removed"
                );
            }
            _ => {
                assert!(
                    matches!(outcome, PassiveWriterOutcome::Retained(_)),
                    "{case}: expected Retained, got {outcome:?}"
                );
                assert!(
                    fs::symlink_metadata(&lock_path).is_ok(),
                    "{case}: active/owned/unknown lock must stay untouched"
                );
            }
        }
        drop(_live_lock);
        drop(_coord_lock);
        fs::remove_dir_all(scratch).unwrap();
    }
}

// Chain: exact input -> current owner -> addressed interrupt -> validated receipt or explicit unknown.
// Risk: stale/foreign responses, partial I/O or a silent peer release or stall an execution.
#[test]
fn cancellation_is_owner_bound_bounded_and_never_replays_work() {
    for case in [
        "ok",
        "client",
        "owner",
        "turn",
        "null",
        "method",
        "refused",
        "oversize",
        "broadcast",
        "malformed",
        "drop",
        "timeout",
        "timeout-before-init",
        "unrelated",
        "directory-mode",
        "socket-mode",
        "symlink",
    ] {
        let root = std::env::temp_dir().join(format!("qipc-{}", super::super::uuid()));
        fs::create_dir(&root).unwrap();
        // R12: fixture ownership must be explicit even when the host umask permits group writes.
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        #[cfg(unix)]
        let path = root.join("s");
        #[cfg(windows)]
        let path =
            std::path::PathBuf::from(format!(r"\\.\pipe\taskq-ipc-{}", super::super::uuid()));
        #[cfg(windows)]
        if matches!(case, "directory-mode" | "socket-mode" | "symlink") {
            fs::remove_dir_all(root).unwrap();
            continue;
        }
        let listener = Listener::bind(&path).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        #[cfg(unix)]
        if matches!(case, "directory-mode" | "socket-mode" | "symlink") {
            let checked = if case == "symlink" {
                let link = root.join("link");
                std::os::unix::fs::symlink(&path, &link).unwrap();
                link
            } else {
                fs::set_permissions(
                    if case == "directory-mode" {
                        &root
                    } else {
                        &path
                    },
                    fs::Permissions::from_mode(0o777),
                )
                .unwrap();
                path.clone()
            };
            assert!(
                cancel_at(
                    &checked,
                    "thread",
                    "turn",
                    Instant::now() + Duration::from_secs(1)
                )
                .unwrap_err()
                .contains("ownership/protection")
            );
            listener.set_nonblocking(true).unwrap();
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock,
                "untrusted endpoint must not receive a request"
            );
            fs::remove_dir_all(root).unwrap();
            continue;
        }
        let (finished, completion) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || -> std::io::Result<()> {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            if case == "timeout-before-init" {
                completion.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            for method in [
                "initialize",
                "thread-owner-discovery",
                "thread-follower-interrupt-turn",
            ] {
                let mut size = [0; 4];
                stream.read_exact(&mut size)?;
                let size = u32::from_le_bytes(size) as usize;
                assert!(size <= 4096);
                let mut bytes = vec![0; size];
                stream.read_exact(&mut bytes)?;
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    request["method"], method,
                    "cancellation never queues or resumes a turn"
                );
                let mut result = match method {
                    "initialize" => json!({"clientId":"client"}),
                    "thread-owner-discovery" => {
                        assert_eq!(request["params"]["conversationId"], "thread");
                        json!({})
                    }
                    _ => {
                        assert_eq!(request["targetClientId"], "owner");
                        assert_eq!(request["version"], 4);
                        assert_eq!(
                            request["params"],
                            json!({"conversationId":"thread","expectedTurnId":"turn","mode":"system"})
                        );
                        json!({"ok":true,"interruptedTurnId":"turn"})
                    }
                };
                if method == "initialize" && case == "client" {
                    result["clientId"] = "wrong".into();
                }
                let control = method == "thread-follower-interrupt-turn";
                let mut reply = json!({"type":"response","requestId":request["requestId"],"method":method,"resultType":"success","handledByClientId":if method=="initialize" {"client"} else {"owner"},"result":result});
                if control {
                    match case {
                        "owner" => reply["handledByClientId"] = "foreign".into(),
                        "turn" => reply["result"]["interruptedTurnId"] = "foreign".into(),
                        "null" => reply["result"]["interruptedTurnId"] = Value::Null,
                        "method" => reply["method"] = "foreign".into(),
                        "refused" => {
                            reply["resultType"] = "error".into();
                            reply["error"] = "request-version-mismatch".into();
                        }
                        "oversize" => {
                            stream.write_all(&(1024u32 * 1024 + 1).to_le_bytes())?;
                            return Ok(());
                        }
                        "broadcast" => {
                            let body =
                                serde_json::to_vec(&json!({"type":"broadcast","event":"changed"}))
                                    .unwrap();
                            stream.write_all(&(body.len() as u32).to_le_bytes())?;
                            stream.write_all(&body)?;
                        }
                        "malformed" => {
                            stream.write_all(&[1, 0, 0, 0, b'{'])?;
                            return Ok(());
                        }
                        "drop" => return Ok(()),
                        "timeout" => {
                            let _ = completion.recv_timeout(Duration::from_secs(5));
                            return Ok(());
                        }
                        "unrelated" => reply["requestId"] = "another-request".into(),
                        _ => {}
                    }
                }
                let body = serde_json::to_vec(&reply).unwrap();
                stream.write_all(&(body.len() as u32).to_le_bytes())?;
                // Fragmentation cannot turn one frame into another response.
                for chunk in body.chunks(7) {
                    stream.write_all(chunk)?;
                }
                if case == "client" {
                    return Ok(());
                }
            }
            let _ = stream.read(&mut [0]);
            Ok(())
        });
        let began = Instant::now();
        let budget = if case.starts_with("timeout") {
            Duration::from_millis(150)
        } else {
            Duration::from_secs(3)
        };
        let outcome = cancel_at(&path, "thread", "turn", began + budget);
        let _ = finished.send(());
        assert_eq!(
            outcome.is_ok(),
            matches!(case, "ok" | "broadcast"),
            "{case}: {outcome:?}"
        );
        assert!(began.elapsed() < budget + Duration::from_secs(1));
        if case == "refused" {
            assert!(
                outcome
                    .as_ref()
                    .unwrap_err()
                    .contains("request-version-mismatch")
            );
        }
        if let Err(e) = server.join().unwrap() {
            // A deadline may close the peer during any fixture I/O, before its interrupt request.
            assert!(
                case.starts_with("timeout")
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::TimedOut
                    ),
                "{case}: fixture I/O: {e}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
