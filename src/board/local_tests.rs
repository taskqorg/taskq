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
            let issue = json!({"number":5,"title":"fixture","body":"","state":if closed.get(){"closed"}else{"open"},"created_at":"2026-11-06T00:00:00Z","updated_at":"2026-11-06T00:00:00Z","labels":[{"name":"taskq"}],"author_association":"MEMBER"});
            let body = if url.ends_with("/graphql") {
                json!({"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false},"nodes":[{"number":5,"title":"fixture","body":"","createdAt":"2026-11-06T00:00:00Z","updatedAt":"2026-11-06T00:00:00Z","authorAssociation":"MEMBER","labels":{"nodes":[{"name":"taskq"}]}}]}}}})
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
    shared::forget(&first.api);
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
        json!({"request":text,"proof":sign(&board.api,text.as_bytes()).as_ref()})
    };
    let write = wire(
        json!({"kind":"request","method":"POST","url":"/issues/5/comments","body":{"body":"must never write"}}),
    );
    assert_eq!(
        answer(&board, &board.api, write).err().unwrap().reason(),
        Reason::Refused
    );
    let foreign = wire(
        json!({"kind":"request","method":"GET","url":"https://foreign.invalid/private","body":null}),
    );
    assert_eq!(
        answer(&board, &board.api, foreign).err().unwrap().reason(),
        Reason::OutsideApi
    );
    let mut forged = wire(json!({"kind":"request","method":"GET","url":"/issues/5"}));
    forged["proof"] = json!(vec![0; 32]);
    assert_eq!(
        answer(&board, &board.api, forged).err().unwrap().reason(),
        Reason::Unauthorized
    );
}

// Chain: #2431 A3/A4, R1/R9/R12: real loopback transport serves fresh reads without a CLI HTTP call;
// a stale owner, missing live endpoint or changed credential refuses instead of falling back.
#[test]
fn shared_2431_transport_reads_service_and_fails_closed() {
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
    let record = json!({"schema":1,"pid":me.pid,"started":me.started,"board":base,"address":listener.local_addr().unwrap().to_string()});
    std::fs::write(&path, record.to_string()).unwrap();
    let server = std::thread::spawn(move || {
        let mut api = Api::new(
            "https://fixture.invalid".into(),
            base.into(),
            "invented-token",
        );
        let calls = std::cell::Cell::new(0);
        api.transport = Some(Box::new(move |_, _, _| {
            calls.set(calls.get() + 1);
            Ok(Reply {
                status: 200,
                headers: vec![("date".into(), "Fri, 06 Nov 2026 00:01:00 GMT".into())],
                body: json!({"revision":calls.get()}).to_string(),
                operation: "read",
            })
        }));
        let board = github::GitHub::new(Api::new(
            "https://fixture.invalid".into(),
            base.into(),
            "invented-token",
        ));
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            connection(&stream).unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let result = answer(&board, &api, serde_json::from_str(&line).unwrap())
                .unwrap_or_else(|error| json!({"error":error}));
            writeln!(stream, "{result}").unwrap();
        }
    });
    let mut client = Api::new(
        "https://fixture.invalid".into(),
        base.into(),
        "invented-token",
    );
    client.install = Some(install.clone());
    client.transport = Some(Box::new(|_, _, _| {
        panic!("live-service reads must not make a CLI HTTP call")
    }));
    prefer(true);
    for revision in 1..=2 {
        assert_eq!(
            client
                .call("GET", "/issues/5", None)
                .unwrap()
                .json()
                .unwrap()["revision"],
            revision,
            "reads remain fresh for revision guards"
        );
    }
    client.credentials.borrow_mut().auth = "Authorization: Bearer other-token".into();
    assert_eq!(
        client
            .call("GET", "/issues/5", None)
            .err()
            .unwrap()
            .reason(),
        Reason::Unauthorized
    );
    server.join().unwrap();
    let mut stale = record.clone();
    stale["pid"] = json!(me.pid + 1);
    std::fs::write(&path, stale.to_string()).unwrap();
    assert!(
        client
            .call("GET", "/issues/5", None)
            .err()
            .unwrap()
            .text()
            .contains("owner differs")
    );
    std::fs::remove_file(&path).unwrap();
    assert!(
        client
            .call("GET", "/issues/5", None)
            .err()
            .unwrap()
            .text()
            .contains("live service has no local board reader")
    );
    std::fs::write(install.join("service"), "invalid").unwrap();
    assert!(
        client
            .call("GET", "/issues/5", None)
            .err()
            .unwrap()
            .text()
            .contains("ownership unknown")
    );
    std::fs::remove_file(install.join("service")).unwrap();
    assert!(
        raw(&client, "GET", "/issues/5", None).is_none(),
        "only confirmed absence falls back directly"
    );
    prefer(false);
    std::fs::remove_dir_all(install).unwrap();
}
