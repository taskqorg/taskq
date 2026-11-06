use super::*;
// Chain: #2431 A2/A4, R1/R4/R12: independent readers share one poll, including close and every comment.
#[test]
fn shared_2431_inventory_keeps_changes_and_second_reader_adds_no_request() {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let closed = std::rc::Rc::new(std::cell::Cell::new(false));
    let make = || {
        let mut api = Api::new(
            "https://fixture.invalid".into(),
            format!(
                "https://fixture.invalid/repos/owner/{}",
                crate::runtime::uuid()
            ),
            "invented-token",
        );
        api.poll = Some(std::time::Duration::from_secs(60));
        let (calls, closed) = (calls.clone(), closed.clone());
        api.transport = Some(Box::new(move |_, url, _| {
            calls.set(calls.get() + 1);
            let issue = json!({"number":5,"title":"fixture","body":"","state":if closed.get(){"closed"}else{"open"},"created_at":"2026-11-06T00:00:00Z","updated_at":if closed.get(){"2026-11-06T00:00:30Z"}else{"2026-11-06T00:00:00Z"},"labels":[{"name":"taskq"}],"author_association":"MEMBER"});
            let body = if url.ends_with("/graphql") {
                json!({"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false},"nodes":[{"number":5,"title":"fixture","body":"","createdAt":"2026-11-06T00:00:00Z","updatedAt":"2026-11-06T00:00:00Z","authorAssociation":"MEMBER","labels":{"nodes":[{"name":"taskq"}]}}]}}}})
            } else if closed.get() && url.contains("state=open") {
                json!([])
            } else {
                json!([issue])
            };
            Ok(Reply {
                status: 200,
                headers: vec![("date".into(), "Fri, 06 Nov 2026 00:01:00 GMT".into())],
                body: body.to_string(),
                operation: "read",
            })
        }));
        github::GitHub::new(api)
    };
    let first = make();
    let mut second = make();
    second.api.base = first.api.base.clone();
    assert_eq!(shared::issues(&first, None, false).unwrap().len(), 1);
    let sent = calls.get();
    assert_eq!(shared::issues(&second, None, false).unwrap().len(), 1);
    assert_eq!(
        calls.get(),
        sent,
        "a second reader adds no issue-list request"
    );
    assert_eq!(shared::issues(&second, Some(0), true).unwrap().len(), 1);
    assert_eq!(
        calls.get(),
        sent,
        "a historical reader shares the same poll"
    );
    closed.set(true);
    shared::forget(&first.api, false);
    assert!(shared::issues(&first, None, false).unwrap().is_empty());
    let changes = shared::issues(&second, Some(0), true).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(
        !changes[0].open,
        "closed updates remain available to later readers"
    );
}

// Chain: #2431 A3/A4, R9/R12: no local board mutation, credential substitution or foreign API read.
#[test]
fn shared_2431_local_reader_authenticates_and_refuses_writes() {
    let api = Api::new(
        "https://fixture.invalid".into(),
        "https://fixture.invalid/repos/o/r".into(),
        "invented-token",
    );
    let board = github::GitHub::new(api);
    let wire = |v: Value| {
        let text = v.to_string();
        json!({"wire":WIRE,"request":text,"proof":sign(&board.api,text.as_bytes()).as_ref()})
    };
    let write = wire(
        json!({"kind":"request","method":"POST","url":"/issues/5/comments","body":{"body":"must never write"}}),
    );
    assert_eq!(
        answer(&board.api, write).err().unwrap().reason(),
        Reason::Refused
    );
    let foreign = wire(
        json!({"kind":"request","method":"GET","url":"https://foreign.invalid/private","body":null}),
    );
    assert_eq!(
        serde_json::from_value::<Error>(answer(&board.api, foreign).unwrap()["error"].clone())
            .unwrap()
            .reason(),
        Reason::OutsideApi
    );
    let mut forged = wire(json!({"kind":"request","method":"GET","url":"/issues/5"}));
    forged["proof"] = json!(vec![0; 32]);
    assert_eq!(
        answer(&board.api, forged).err().unwrap().reason(),
        Reason::Unauthorized
    );
}

// Chain: #2431 A3/A4, R1/R9/R12: fresh GET guards use the service; channel failure falls back with a cause.
#[test]
fn shared_2431_transport_reads_service_and_degrades() {
    let install =
        std::env::temp_dir().join(format!("taskq-local-board-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(&install).unwrap();
    let me = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let service = json!({"schema":1,"pid":me.pid,"started":me.started,"instance":"invented-2431","at":wall() as u64,"version":"fixture"});
    std::fs::write(install.join("service"), service.to_string()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = "https://fixture.invalid/repos/owner/repo";
    let path = endpoint(&install, base);
    let record = json!({"schema":1,"wire":WIRE,"pid":me.pid,"started":me.started,"board":base,"address":listener.local_addr().unwrap().to_string()});
    std::fs::write(&path, record.to_string()).unwrap();
    let server = std::thread::spawn(move || {
        let mut api = Api::new(
            "https://fixture.invalid".into(),
            base.into(),
            "invented-token",
        );
        let calls = std::cell::Cell::new(0);
        api.transport = Some(Box::new(move |_, url, _| {
            if url.ends_with("/blocked") {
                return Err(Error::http(503, "fixture HTTP 503"));
            }
            calls.set(calls.get() + 1);
            Ok(Reply {
                status: 200,
                headers: vec![("date".into(), "Fri, 06 Nov 2026 00:01:00 GMT".into())],
                body: json!({"revision":calls.get()}).to_string(),
                operation: "read",
            })
        }));
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            connection(&stream).unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let reply = answer(&api, serde_json::from_str(&line).unwrap())
                .unwrap_or_else(|error| json!({"wire":WIRE,"unavailable":error.text()}));
            writeln!(stream, "{reply}").unwrap();
        }
    });
    let direct = std::rc::Rc::new(std::cell::Cell::new(0));
    let sent = direct.clone();
    let mut client = Api::new(
        "https://fixture.invalid".into(),
        base.into(),
        "invented-token",
    );
    client.local_reader = true;
    client.install = Some(install.clone());
    client.transport = Some(Box::new(move |_, _, _| {
        sent.set(sent.get() + 1);
        Ok(Reply {
            status: 200,
            headers: vec![],
            body: json!({"revision":"direct"}).to_string(),
            operation: "read",
        })
    }));
    prefer(true);
    for revision in 1..=2 {
        assert_eq!(
            client
                .call("GET", "/issues/5", None)
                .unwrap()
                .json()
                .unwrap()["revision"],
            revision
        );
    }
    assert_eq!(
        direct.get(),
        0,
        "fresh service reads make no client HTTP call"
    );
    assert!(
        client
            .call("GET", "/blocked", None)
            .err()
            .unwrap()
            .text()
            .contains("503")
    );
    assert_eq!(direct.get(), 0, "a board refusal is not replayed directly");
    client.credentials.borrow_mut().auth = "Authorization: Bearer other-token".into();
    assert_eq!(
        client
            .call("GET", "/issues/5", None)
            .unwrap()
            .json()
            .unwrap()["revision"],
        "direct"
    );
    server.join().unwrap();
    let fallback = |record: Option<Value>, service: &Value| {
        std::fs::write(install.join("service"), service.to_string()).unwrap();
        if let Some(record) = record {
            std::fs::write(&path, record.to_string()).unwrap();
        } else {
            let _ = std::fs::remove_file(&path);
        }
        let before = direct.get();
        assert_eq!(
            client
                .call("GET", "/issues/5", None)
                .unwrap()
                .json()
                .unwrap()["revision"],
            "direct"
        );
        assert_eq!(direct.get(), before + 1);
    };
    let mut foreign = record.clone();
    foreign["pid"] = json!(me.pid + 1);
    fallback(Some(foreign), &service);
    let mut old = record.clone();
    old["wire"] = Value::Null;
    fallback(Some(old), &service);
    fallback(None, &service);
    let mut stale = service.clone();
    stale["at"] = json!(0);
    fallback(Some(record.clone()), &stale);
    fallback(Some(record), &service); // listener exited: connect refusal
    std::fs::write(install.join("service"), "invalid").unwrap();
    assert!(raw(&client, "GET", "/issues/5", None).is_none());
    std::fs::remove_file(install.join("service")).unwrap();
    assert!(raw(&client, "GET", "/issues/5", None).is_none());
    client.local_reader = false;
    assert!(
        raw(&client, "GET", "/issues/5", None).is_none(),
        "unsupported boards disclose direct reads"
    );
    prefer(false);
    std::fs::remove_dir_all(install).unwrap();
}

// Chain: #2431 A3/A4: endpoint failure cannot stop a board; a silent connection cannot serialize readers.
#[test]
fn shared_2431_reader_startup_and_connections_are_independent() {
    let root =
        std::env::temp_dir().join(format!("taskq-reader-isolation-{}", crate::runtime::uuid()));
    std::fs::create_dir_all(root.join("install")).unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "board::local::tests::shared_2431_reader_child",
            "--ignored",
            "--nocapture",
        ])
        .env("TASKQ_INSTALL", root.join("install"))
        .env("TASKQ_LOCAL_READER_ROOT", &root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "private local reader fixture; no board network or service unit"]
fn shared_2431_reader_child() {
    use std::io::Read;
    let root = std::path::PathBuf::from(std::env::var_os("TASKQ_LOCAL_READER_ROOT").unwrap());
    let install = root.join("install");
    let repo = format!("owner/fixture-{}", crate::runtime::uuid());
    std::fs::write(
        root.join("taskq.json"),
        json!({"repo":repo,"host":"fixture.invalid"}).to_string(),
    )
    .unwrap();
    std::fs::write(install.join("token"), "invented").unwrap();
    crate::host::enter(&root, &install);
    let mut api = (github::KIND.api)("fixture.invalid", &repo, "invented");
    api.poll = Some(std::time::Duration::from_secs(60));
    api.transport = Some(Box::new(|_, _, _| {
        Ok(Reply {
            status: 200,
            headers: vec![("Date".into(), "Fri, 06 Nov 2026 00:01:00 GMT".into())],
            body: "[]".into(),
            operation: "read",
        })
    }));
    let board = github::GitHub::new(api);
    assert!(shared::issues(&board, None, false).unwrap().is_empty());
    let path = endpoint(&install, &board.api.base);
    std::fs::create_dir(&path).unwrap(); // publication fails, after bind succeeds
    let tool = crate::platform::tool(&install);
    let opened = github::KIND
        .open("fixture.invalid", &repo, None, &tool)
        .unwrap();
    assert!(
        opened.issues(None).unwrap().is_empty(),
        "endpoint failure must not stop supervision"
    );
    std::fs::remove_dir(&path).unwrap();
    github::KIND
        .open("fixture.invalid", &repo, None, &tool)
        .unwrap();
    let record = crate::data::json(&path).unwrap().unwrap();
    assert_eq!(record["wire"], WIRE);
    let address = record["address"].as_str().unwrap();
    let mut silent = TcpStream::connect(address).unwrap();
    let mut reader = TcpStream::connect(address).unwrap();
    connection(&reader).unwrap();
    let payload = json!({"kind":"list","since":null,"tasks":false}).to_string();
    writeln!(
        reader,
        "{}",
        json!({"wire":WIRE,"request":payload,"proof":sign(&board.api,payload.as_bytes()).as_ref()})
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        reply["issues"],
        json!([]),
        "complete snapshot served: {reply}"
    );
    silent.set_nonblocking(true).unwrap();
    assert_eq!(
        silent.read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "the first connection remains pending when the second finishes"
    );
    drop(silent);
}
