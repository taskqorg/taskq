use super::*;
// Chain: §8 #2161 A1/A2 — launchd preserves child optimization intent in the hashed job.
// Risk: an explicit off silently becomes inherit at the supervisor boundary.
#[test]
fn protected_scope_preserves_optimization_in_hashed_job() {
    if std::env::var_os("TASKQ_SCOPE_ENV_TEST").is_none() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact","runtime::protected_app_server::scope::tests::protected_scope_preserves_optimization_in_hashed_job","--nocapture"])
                .env("TASKQ_SCOPE_ENV_TEST","1").env("AGENTQ_OPTIMIZATION","off");
        let out = coreq::process::output(
            "test.scope-environment",
            &mut command,
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let mut environment = environment(Path::new("/invented/home"));
    assert_eq!(environment["AGENTQ_OPTIMIZATION"], "off");
    let job = plist(
        "invented-label",
        "/invented/adapter",
        "slot-invented",
        "/invented/project",
        &environment,
    )
    .unwrap();
    assert!(job.contains("<key>AGENTQ_OPTIMIZATION</key><string>off</string>"));
    let job_sha256 = digest(job.as_bytes());
    for intent in [Some("on"), None] {
        if let Some(value) = intent {
            environment["AGENTQ_OPTIMIZATION"] = json!(value);
        } else {
            environment
                .as_object_mut()
                .unwrap()
                .remove("AGENTQ_OPTIMIZATION");
        }
        let changed = plist(
            "invented-label",
            "/invented/adapter",
            "slot-invented",
            "/invented/project",
            &environment,
        )
        .unwrap();
        assert_ne!(job_sha256, digest(changed.as_bytes()));
    }
}
// Chain: R8 R9 R12 R13 #2151 A3/A4/A5 — job registration has no replay/restart trigger or executable chosen by native text.
// Risk: launchd restarts a consumed generation or a path changes the plist surface.
#[test]
fn protected_scope_plan_has_only_one_explicit_dispatch() {
    let plan = plist(
        "invented-label",
        "/invented/adapter & image",
        "slot-invented",
        "/invented/project",
        &json!({"HOME":"/invented/home"}),
    )
    .unwrap();
    for fact in [
        "<key>RunAtLoad</key><false/>",
        "<key>KeepAlive</key><false/>",
        "<key>LaunchOnlyOnce</key><true/>",
        "<key>AbandonProcessGroup</key><true/>",
        "<string>scope-run</string>",
        "adapter &amp; image",
    ] {
        assert!(plan.contains(fact), "{fact}");
    }
    for forbidden in [
        "StartInterval",
        "StartCalendarInterval",
        "WatchPaths",
        "QueueDirectories",
        "MachServices",
        "Sockets",
    ] {
        assert!(!plan.contains(forbidden), "{forbidden}");
    }
    assert!(
        plist(
            "invented",
            "invented",
            "invented",
            "invented",
            &json!({"HOME":null})
        )
        .is_err()
    );
}
// Chain: R8 R9 R12 #2151 A2/A5 — stdio peers and request framing bind native identity, not caller-supplied PID text.
// Risk: cross-generation connection, incomplete input or buffered bytes become a replayed request.
#[test]
fn protected_scope_transport_binds_peer_and_preserves_following_bytes() {
    let (mut writer, reader) = UnixStream::pair().unwrap();
    socket_deadlines(&reader).unwrap();
    assert_eq!(peer(&reader).unwrap(), identity().unwrap());
    writer
        .write_all(b"{\"invented\":true}\nnext native input")
        .unwrap();
    writer.shutdown(Shutdown::Write).unwrap();
    let mut reader = BufReader::new(reader);
    assert_eq!(line(&mut reader, 64).unwrap(), "{\"invented\":true}\n");
    let mut rest = String::new();
    reader.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "next native input");
    let (mut writer, reader) = UnixStream::pair().unwrap();
    writer.write_all(b"incomplete").unwrap();
    writer.shutdown(Shutdown::Write).unwrap();
    assert!(line(&mut BufReader::new(reader), 32).is_err());
    let (mut writer, reader) = UnixStream::pair().unwrap();
    writer.write_all(b"oversize\n").unwrap();
    assert!(line(&mut BufReader::new(reader), 4).is_err());
}
// Chain: R12 #2179 A1 — root cause of the d9ea8cef and native15f6 refusals: macOS accept() inherits the listener's O_NONBLOCK.
// Risk: the frontend's first stdout read before native output returns EAGAIN (errno 35), the frontend exits and the supervisor then sees EINVAL (errno 22).
#[test]
fn protected_scope_accepted_stream_blocks_until_native_output() {
    let dir = std::env::temp_dir().join(format!("tq-accept-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(dir.join("io")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let supervisor = UnixStream::connect(dir.join("io")).unwrap();
    let mut frontend = accept(&listener, Instant::now() + WAIT).unwrap();
    let flags = unsafe { libc::fcntl(frontend.as_raw_fd(), libc::F_GETFL) };
    assert!(
        flags >= 0 && flags & libc::O_NONBLOCK == 0,
        "accepted scope stream must block"
    );
    frontend.set_read_timeout(None).unwrap();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        (&supervisor).write_all(b"late").unwrap();
        supervisor
    });
    let mut bytes = [0u8; 4];
    io::Read::read_exact(&mut frontend, &mut bytes).unwrap();
    assert_eq!(
        &bytes, b"late",
        "no native output yet is a wait, never EAGAIN"
    );
    drop(writer.join().unwrap());
    assert_eq!(io::Read::read(&mut frontend, &mut bytes).unwrap(), 0);
    let _ = fs::remove_dir_all(&dir);
}
// Chain: R12 #2175 A1 — the d9ea8cef refusal: macOS rejects SO_RCVTIMEO with EINVAL once the frontend closed its socket.
// Risk: the supervisor reports frontend loss as an unavailable input bound after native spawn instead of input EOF.
#[test]
fn protected_scope_input_bound_precedes_frontend_loss() {
    let (frontend, supervisor) = UnixStream::pair().unwrap();
    drop(frontend);
    assert_eq!(
        supervisor
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EINVAL)
    );
    let (frontend, mut supervisor) = UnixStream::pair().unwrap();
    supervisor
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    drop(frontend);
    assert_eq!(
        io::Read::read(&mut supervisor, &mut [0u8; 8]).unwrap(),
        0,
        "bound set before spawn turns frontend loss into input EOF"
    );
}
