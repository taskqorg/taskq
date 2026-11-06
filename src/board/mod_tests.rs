    use super::*;
    use std::process::Command;

    const HTTP_FIXTURE_SECONDS: u64 = if cfg!(windows) { 60 } else { 15 };

    // Chain: real curl/config stdin -> local HTTP refusal -> request accounting and secret-free execution trace.
    #[test]
    fn native_http_preserves_refusal_without_exporting_request_data() {
        let dir = std::env::temp_dir().join(format!("taskq-native-http-{}-{}", std::process::id(),crate::runtime::uuid()));
        std::fs::create_dir(&dir).unwrap();
        let out = coreq::process::output("test.http", Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "board::tests::native_http_child", "--ignored", "--nocapture"])
            .env("TASKQ_NATIVE_HTTP_TEST_ROOT", &dir), std::time::Duration::from_secs(HTTP_FIXTURE_SECONDS));
        let phases = std::fs::read_to_string(dir.join("phases.log")).unwrap_or_else(|_|"child entry not observed".into());
        assert!(out.is_ok(), "native HTTP chain failed: {out:?}; phases: {phases}");
        println!("native HTTP phases: {phases}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "native curl and collector fixture; local loopback only"]
    fn native_http_child() {
        use std::{io::{Read, Write}, net::TcpListener, time::{Duration, Instant}};
        let dir = std::path::PathBuf::from(std::env::var_os("TASKQ_NATIVE_HTTP_TEST_ROOT").unwrap());
        let began = Instant::now();
        let phase=|name:&str|writeln!(std::fs::OpenOptions::new().create(true).append(true).open(dir.join("phases.log")).unwrap(),"{name} {}ms",began.elapsed().as_millis()).unwrap();
        phase("capture.start");
        let _runtime = coreq::telemetry::capture::Runtime::install(&dir.join("db"), "fixtureq", "1").unwrap();
        phase("capture.ready");
        let installed = began.elapsed();
        crate::profile::tests::take_report();
        // The HTTP deadline starts after collector setup, which has its own outer bound.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let body = json!({"text": "private-body".repeat(4000)});
        let expected = body.clone();
        let server = std::thread::spawn(move || {
            // Collector/process startup may precede curl; the parent bounds the whole fixture.
            let until = Instant::now() + Duration::from_secs(if cfg!(windows) { HTTP_FIXTURE_SECONDS } else { 5 });
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until, "native curl did not reach the local fixture");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut part = [0; 4096];
                let n = socket.read(&mut part).unwrap();
                assert_ne!(n, 0, "incomplete request"); bytes.extend_from_slice(&part[..n]);
                assert!(bytes.len() <= 1024 * 1024);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                    if bytes.len() < end + 4 + length { continue; }
                    assert!(head.contains("authorization: bearer private-token"));
                    assert_eq!(serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length]).unwrap(), expected);
                    break;
                }
            }
            let reply = "private-response";
            write!(socket, "HTTP/1.1 503 Unavailable\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
        });
        phase("http.start");
        let result = Api::new(base.clone(), base, "private-token").call("POST", "/request", Some(body));
        phase("http.returned");
        let joined = server.join();
        phase("http.joined");
        assert!(joined.is_ok(), "HTTP fixture failed; runtime install={installed:?}, through request={:?}, client error={:?}", began.elapsed(), result.as_ref().err());
        assert!(result.err().unwrap().text().contains("HTTP 503"));
        assert!(crate::profile::tests::take_report().contains("http5xx:1"));
        phase("capture.qualify");
        let health = coreq::telemetry::capture::qualify().unwrap();
        phase("capture.qualified");
        let store = coreq::telemetry::Reader::open(&dir.join("db")).unwrap();
        let exported = store.export(0, 0, 1024).unwrap();
        assert!(exported["events"].as_array().unwrap().iter().any(|r| r["observation"]["operation"] == "fixtureq.operation.board.http"
            && r["observation"]["kind"] == "method" && r["observation"]["outcome"] == "ok"));
        assert!(!exported.to_string().contains("private-"));
        assert_eq!(health["lost"], 0);
        phase("capture.drop");drop(_runtime);phase("complete");
    }


    fn http(kind: &Kind, schema: Option<bool>, transport: Transport) -> Box<dyn Board> {
        let mut api = (kind.api)(if kind.name == "github" { "github.com" } else { "gitlab.example" }, "o/r", "test");
        api.transport = Some(transport);
        *api.label_schema.borrow_mut() = schema;
        (kind.board)(api)
    }

    // Chain: § 2 evidence files (#2019) -> GitHub and GitLab adapters. Defect: neither board stored a worker's zip,
    // tar.gz, PDF or HTML file. Risk: GitHub commits files to a branch that default clones download, links a moving ref
    // instead of the commit, or GitLab links a relative upload; on both boards a file links before its load check.
    #[test]
    fn evidence_files_commit_under_a_non_branch_ref_on_github_and_upload_on_gitlab() {
        use base64::Engine;
        let dir = std::env::temp_dir().join(format!("taskq-evidence-files-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("build.zip");
        std::fs::write(&zip, b"PK\x03\x04 zip body").unwrap();
        let paths = [zip.display().to_string()];
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let reply = |v: Value, method: &str| Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: operation(method) });
        let calls = std::rc::Rc::new(RefCell::new(Vec::<String>::new()));
        let seen = calls.clone();
        let html = dir.join("site.html");
        std::fs::write(&html, b"<!doctype html><p>page</p>").unwrap();
        // the blob GitHub stores is the blob it serves back
        let blob = std::rc::Rc::new(RefCell::new(String::new()));
        let stored = blob.clone();
        let github = http(&github::KIND, None, Box::new(move |method, url, body| {
            seen.borrow_mut().push(format!("{method} {url} {}", body.as_ref().map(|b| b.to_string()).unwrap_or_default()));
            let path = url.strip_prefix("https://api.github.com/repos/o/r").unwrap_or(url);
            match (method, path) {
                ("GET", "/git/ref/taskq/evidence") => Err(Error::http(404, "HTTP 404 Not Found")),
                ("POST", "/git/blobs") => { *stored.borrow_mut() = body.as_ref().map(|b| s(b, "content")).unwrap_or_default(); reply(json!({"sha": "b1"}), method) }
                ("POST", "/git/trees") => reply(json!({"sha": "t1"}), method),
                ("POST", "/git/commits") => reply(json!({"sha": sha}), method),
                ("POST", "/git/refs") => reply(json!({}), method),
                ("GET", p) if p.starts_with("/contents/7/") => reply(json!({"sha": "b1"}), method),
                ("GET", "/git/blobs/b1") => reply(json!({"content": stored.borrow().clone()}), method),
                _ => Err(format!("unexpected {method} {url}").into()),
            }
        }));
        let links = crate::cli::text::upload_attachments(github.as_ref(), 7, &paths).unwrap();
        let digest: String = ring::digest::digest(&ring::digest::SHA256, &std::fs::read(&zip).unwrap()).as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(links, [format!("[⬇ build.zip](https://github.com/o/r/raw/{sha}/7/{digest}-build.zip)")]);
        // Defect: GitHub raw serves HTML as text/plain, so one click showed the page source; the page goes up zipped
        let links = crate::cli::text::upload_attachments(github.as_ref(), 7, &[html.display().to_string()]).unwrap();
        let packed = base64::engine::general_purpose::STANDARD.decode(blob.borrow().as_str()).unwrap();
        let digest: String = ring::digest::digest(&ring::digest::SHA256, &packed).as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(links, [format!("[⬇ site.html.zip](https://github.com/o/r/raw/{sha}/7/{digest}-site.html.zip)")]);
        assert_eq!(crate::tests::unzip_one(&packed), ("site.html".to_string(), std::fs::read(&html).unwrap()));
        let calls = calls.borrow();
        assert!(calls.iter().any(|c| c.starts_with("POST https://api.github.com/repos/o/r/git/refs ") && c.contains(r#""ref":"refs/taskq/evidence""#)), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains("heads")), "no branch: default clones skip the file; {calls:?}");

        let uploads = std::rc::Rc::new(RefCell::new(Vec::<String>::new()));
        let seen = uploads.clone();
        let gitlab = http(&gitlab::KIND, None, Box::new(move |method, url, body| {
            let path = url.strip_prefix("https://gitlab.example/api/v4/projects/o%2Fr").unwrap_or(url);
            match (method, path) {
                ("GET", "") => reply(json!({"id": 39}), method),
                ("POST", "/uploads") => {
                    let name = body.and_then(|b| b["file"].as_str().map(String::from)).unwrap_or_default();
                    seen.borrow_mut().push(name.clone());
                    reply(json!({"id": 5, "url": format!("/uploads/cef859da/{name}")}), method)
                }
                ("GET", "/uploads/5") => Ok(Reply { status: 200, headers: vec![], body: "PK\x03\x04 zip body".into(), operation: "read" }),
                _ => Err(format!("unexpected {method} {url}").into()),
            }
        }));
        let links = crate::cli::text::upload_attachments(gitlab.as_ref(), 7, &paths).unwrap();
        let name = uploads.borrow()[0].clone();
        assert!(name.ends_with("-build.zip"), "{name}");
        assert_eq!(links, [format!("[⬇ build.zip](https://gitlab.example/-/project/39/uploads/cef859da/{name}#upload=5)")]);
        assert!(gitlab.html_downloads() && !github.html_downloads(), "GitLab keeps an HTML upload as is; GitHub zips it");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Chain: § 5 `taskq issue` (#1892) -> GitHub/GitLab adapters. Defect: PilotQ owner actions on any issue refused,
    // TaskQ had no read/comment/label command. Risk: a field is missing, a write is not read back, or a repeated op id
    // posts a second comment or label.
    #[test]
    fn issue_command_reads_and_writes_once_per_op_id_on_both_boards() {
        use std::{cell::RefCell, rc::Rc};
        for kind in BOARDS {
            let state = Rc::new(RefCell::new((vec!["bug".to_string()], vec![(1u64, "outside text".to_string())], 0usize)));
            let st = state.clone();
            let gl = kind.name == "gitlab";
            let b = http(kind, Some(false), Box::new(move |method, url, body| {
                let mut st = st.borrow_mut();
                let note = |id: u64, body: &str| json!({"id": id, "body": body, "created_at": "2026-10-30T00:00:00Z", "updated_at": "2026-10-30T00:00:00Z",
                    "author_association": "OWNER", "user": {"login": "owner"}, "author": {"id": 1, "username": "owner"}, "system": false});
                let path = url.split('?').next().unwrap();
                let v = match (method, path.rsplit_once("/issues/7").map(|x| x.1)) {
                    _ if url.contains("/members/") => json!({"access_level": 30}),
                    ("GET", Some("")) => json!({"number": 7, "iid": 7, "title": "Any issue", "body": "text", "description": "text", "state": if gl { "opened" } else { "open" },
                        "labels": st.0, "author_association": "OWNER", "author": {"id": 1}}),
                    ("GET", Some("/comments" | "/notes")) => json!(st.1.iter().map(|(id, b)| note(*id, b)).collect::<Vec<_>>()),
                    ("POST", Some("/comments" | "/notes")) => {
                        st.2 += 1;
                        let id = 100 + st.2 as u64;
                        st.1.push((id, body.unwrap()["body"].as_str().unwrap().into()));
                        json!({"id": id})
                    }
                    ("POST", Some("/labels")) => { st.0.extend(names(&body.unwrap()["labels"], "name")); st.2 += 1; json!([]) }
                    ("PUT", Some("")) => { st.0.extend(body.unwrap()["add_labels"].as_str().unwrap().split(',').map(String::from)); st.2 += 1; json!({}) }
                    _ => panic!("{} unexpected {method} {url}", kind.name),
                };
                Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: "read" })
            }));
            let read = issue_json(b.as_ref(), 7).unwrap();
            assert_eq!(read["url"], b.url(7));
            assert!(read["url"].as_str().unwrap().starts_with(if gl { "https://gitlab.example/o/r/" } else { "https://github.com/o/r/" }), "{read}");
            assert_eq!((read["title"].as_str(), read["state"].as_str(), read["body"].as_str()), (Some("Any issue"), Some("open"), Some("text")));
            assert_eq!(read["labels"], json!(["bug"]));
            assert_eq!(read["comments"][0]["body"], "outside text");
            assert_eq!(read["comments"][0]["author"], "owner");
            for _ in 0..2 {
                let r = issue_write(b.as_ref(), 7, IssueWrite::Comment("hello"), Some("pilot.1"));
                assert_eq!((r["outcome"].as_str(), r["comment"].as_u64(), r["op_id"].as_str()), (Some("accepted"), Some(101), Some("pilot.1")), "{r}");
                let r = issue_write(b.as_ref(), 7, IssueWrite::Label("infra"), Some("pilot.2"));
                assert_eq!((r["outcome"].as_str(), r["label"].as_str()), (Some("accepted"), Some("infra")), "{r}");
            }
            assert_eq!(state.borrow().2, 2, "{}: one comment and one label write", kind.name);
            assert_eq!(state.borrow().1[1].1, "hello\nop-id: pilot.1");
            let r = issue_write(b.as_ref(), 7, IssueWrite::Comment("other"), Some("pilot.1"));
            assert_eq!((r["outcome"].as_str(), r["comment"].as_u64()), (Some("rejected"), Some(101)));
            for w in [IssueWrite::Comment("taskq: close"), IssueWrite::Comment(" "), IssueWrite::Label("q-ready"), IssueWrite::Label("taskq"), IssueWrite::Label("a,b")] {
                assert_eq!(issue_write(b.as_ref(), 7, w, None)["outcome"], "rejected");
            }
            assert_eq!(state.borrow().2, 2, "refusals write nothing");
        }
    }

    // Chain: § 1 create claim, `taskq claim-issue` (#1907) -> GitHub/GitLab adapters. Defect: no command set up a board's
    // claim issue; the PM created and configured it by hand. Risk: a rerun creates a second claim issue, or a foreign,
    // task or host issue holds create claims.
    #[test]
    fn claim_issue_creates_once_reuses_and_refuses_foreign_task_or_host_on_both_boards() {
        use std::{cell::RefCell, rc::Rc};
        for kind in BOARDS {
            // (number, title, labels, trusted author), creates
            let seed = vec![(1u64, "Outside bug", vec![], false), (2, CLAIM_TITLE, vec!["taskq".to_string()], true),
                (3, "host mac", vec!["taskq-host".to_string()], true), (4, "Board notes", vec![], true)];
            let state = Rc::new(RefCell::new((seed, 0usize)));
            let st = state.clone();
            let gl = kind.name == "gitlab";
            let b = http(kind, Some(false), Box::new(move |method, url, body| {
                let mut st = st.borrow_mut();
                let issue = |(n, title, labels, trusted): &(u64, &str, Vec<String>, bool)| json!({"number": n, "iid": n, "title": title,
                    "body": "", "description": "", "state": if gl { "opened" } else { "open" }, "labels": labels,
                    "author_association": if *trusted { "OWNER" } else { "NONE" }, "author": {"id": if *trusted { 1 } else { 2 }}});
                let path = url.split('?').next().unwrap();
                let v = match (method, path.rsplit_once("/issues").map(|x| x.1)) {
                    _ if url.contains("/members/all/") => json!({"access_level": if url.ends_with("/1") { 30 } else { 10 }}),
                    ("GET", Some("")) => json!(st.0.iter().map(issue).collect::<Vec<_>>()),
                    ("GET", Some(n)) => { let n: u64 = n[1..].parse().unwrap(); issue(st.0.iter().find(|i| i.0 == n).unwrap()) }
                    ("POST", Some("")) => {
                        let body = body.unwrap();
                        assert_eq!(body["title"], CLAIM_TITLE);
                        let n = st.0.len() as u64 + 1;
                        st.0.push((n, CLAIM_TITLE, vec![], true));
                        st.1 += 1;
                        json!({"number": n, "iid": n})
                    }
                    _ => panic!("{} unexpected {method} {url}", kind.name),
                };
                Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: "read" })
            }));
            let b = b.as_ref();
            let r = claim_issue(b, None, None);
            assert_eq!((r["outcome"].as_str(), r["source"].as_str(), r["issue"].as_u64()), (Some("accepted"), Some("created"), Some(5)), "{}: {r}", kind.name);
            assert_eq!(r["url"], b.url(5));
            let r = claim_issue(b, None, None);
            assert_eq!((r["outcome"].as_str(), r["source"].as_str(), r["issue"].as_u64()), (Some("accepted"), Some("found"), Some(5)), "lost key: {r}");
            for adopt in [None, Some(5)] {
                assert_eq!(claim_issue(b, Some(5), adopt)["source"], "configured");
                assert_eq!(claim_issue(b, Some(5), adopt)["outcome"], "accepted");
            }
            assert_eq!(claim_issue(b, Some(5), Some(4))["outcome"], "rejected", "a configured issue is never replaced");
            for (n, why) in [(1, "not trusted"), (2, "a task"), (3, "host heartbeat")] {
                let r = claim_issue(b, None, Some(n));
                assert_eq!(r["outcome"], "rejected", "{r}");
                assert!(r["reason"].as_str().unwrap().contains(why), "{r}");
            }
            let r = claim_issue(b, None, Some(4));
            assert_eq!((r["outcome"].as_str(), r["source"].as_str()), (Some("accepted"), Some("adopted")));
            state.borrow_mut().0.push((6, CLAIM_TITLE, vec![], true));
            let r = claim_issue(b, None, None);
            assert_eq!((r["outcome"].as_str(), r["reason"].as_str()), (Some("rejected"), Some("several claim issues: #5, #6")), "{r}");
            assert_eq!(state.borrow().1, 1, "{}: one create in all", kind.name);
        }
    }

    // #1892 A3: a comment whose response was lost is found by its op id on retry, never posted twice.
    #[test]
    fn issue_comment_lost_response_replays_by_op_id() {
        let b = Fake::default();
        b.create("t", "", &[]).unwrap();
        b.state.borrow_mut().lose_comment_response = true;
        let first = issue_write(&b, 1, IssueWrite::Comment("note"), Some("x"));
        assert_eq!(first["outcome"], "accepted", "readback finds the committed comment: {first}");
        let again = issue_write(&b, 1, IssueWrite::Comment("note"), Some("x"));
        assert_eq!(again["comment"], first["comment"]);
        assert_eq!(b.comments(1).unwrap().len(), 1);
        b.state.borrow_mut().fail_comments = 1;
        let failed = issue_write(&b, 1, IssueWrite::Comment("next"), Some("y"));
        assert_eq!((failed["outcome"].as_str(), failed["next"].as_str()), (Some("unknown"), Some("Retry with --op-id y.")));
        assert_eq!(issue_write(&b, 1, IssueWrite::Comment("next"), Some("y"))["outcome"], "accepted");
        assert_eq!(b.comments(1).unwrap().len(), 2);
    }

    // #1829 A1/A3: exact GitHub/GitLab issue reads retain closure time independently of later edits.
    #[test]
    fn closed_pin_adapters_retain_board_closure() {
        for kind in BOARDS {
            let b=http(kind,Some(true),Box::new(|method,url,_| {
                assert_eq!(method,"GET");
                let v=if url.contains("/members/") { json!({"access_level":20}) } else {
                    assert!(url.ends_with("/issues/7"),"exact pin read: {url}");
                    json!({"number":7,"iid":7,"state":"closed","state_reason":"completed","author_association":"OWNER","author":{"id":1},
                        "labels":["taskq"],"closed_at":"2026-10-28T00:00:00Z","updated_at":"2026-10-29T00:00:00Z"})
                };
                Ok(Reply {status:200,headers:vec![],body:v.to_string(),operation:"read"})
            }));
            let i=b.issue(7).unwrap();
            assert_eq!((i.number,i.open,i.closed),(7,false,iso("2026-10-28T00:00:00Z")));
            assert!(i.trusted && i.closed.unwrap()<i.updated);
        }
    }
    // Chain: §5 `inbox N --json` -> board adapters -> answer writer, board time and question revision, attempt progress.
    // Risk: a client sees an answer without who wrote it or which question it answered, or the read costs more requests than `inbox`.
    #[test]
    fn inbox_json_lists_answers_with_writer_time_and_question_revision() {
        use crate::core::entry;
        let root = std::env::temp_dir().join(format!("taskq-inbox-json-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        std::fs::write(crate::runtime::attempt_log(&root, "worker", 7, 1), "progress").unwrap();
        let log = [(1, "worker", entry("spawn", &["worker", "claude:abc", "mac"], None, "")),
            (2, "worker", entry("ask", &[], Some(1), "Which way?\nBecause.\n1. left\n2. right ★")),
            (3, "owner", entry("answer", &["2"], None, "")),
            (4, "owner", entry("steer", &["answer", "2"], None, "left after all")),
            (5, "worker", entry("ask", &[], Some(1), "Second?\n1. up\n2. down")),
            (6, "pm", "down".to_string())];
        for kind in BOARDS {
            let board = |calls: std::rc::Rc<RefCell<usize>>, log: Vec<(u64, &'static str, String)>| http(kind, Some(true), Box::new(move |method, url, _| {
                assert_eq!(method, "GET", "a read only: {url}");
                *calls.borrow_mut() += 1;
                let at = |id: u64| format!("2026-10-30T00:00:0{id}Z");
                let v = if url.contains("/members/") { json!({"access_level": 30}) }
                    else if url.contains("/comments") || url.contains("/notes") {
                        json!(log.iter().map(|(id, by, body)| json!({"id": id, "body": body, "created_at": at(*id), "updated_at": at(*id),
                            "author_association": "OWNER", "user": {"login": by}, "author": {"id": 1, "username": by}, "system": false})).collect::<Vec<_>>())
                    } else {
                        assert!(url.ends_with("/issues/7"), "{url}");
                        json!({"number": 7, "iid": 7, "title": "Pick", "body": "Goal: g", "state": if kind.name == "github" { "open" } else { "opened" }, "author_association": "OWNER",
                            "author": {"id": 1}, "labels": ["taskq"], "created_at": at(0), "updated_at": at(6)})
                    };
                Ok(Reply {status: 200, headers: vec![], body: v.to_string(), operation: "read"})
            }));
            let (json_calls, human_calls) = (std::rc::Rc::new(RefCell::new(0)), std::rc::Rc::new(RefCell::new(0)));
            let v = crate::cli::inbox::inbox_json(&*board(json_calls.clone(), log.to_vec()), &root, "mac", 7).unwrap();
            let human = crate::cli::inbox::inbox(&*board(human_calls.clone(), log.to_vec()), &root, 7).unwrap();
            assert_eq!(*json_calls.borrow(), *human_calls.borrow(), "{}: same reads as the human inbox", kind.name);
            assert!(human.starts_with("#7 Pick\n") && human.contains("\nLog:\n"), "human inbox unchanged: {human}");
            let at = |id: u64| iso(&format!("2026-10-30T00:00:0{id}Z")).unwrap();
            assert_eq!(v["answers"], json!([
                {"id": 3, "by": "owner", "at": at(3), "revision": 2, "option": 2, "text": "right", "late": false, "read": null},
                {"id": 4, "by": "owner", "at": at(4), "revision": 2, "option": null, "text": "left after all", "late": true, "read": null},
                {"id": 6, "by": "pm", "at": at(6), "revision": 5, "option": 2, "text": "down", "late": false, "read": null}]), "{}: {v}", kind.name);
            let attempt = &v["attempt"];
            assert_eq!((v["open"].clone(), v["state"].clone(), attempt["id"].clone(), attempt["host"].clone(), attempt["runtime"].clone(), attempt["started"].clone()),
                (json!(true), json!("doing"), json!(1), json!("mac"), json!("claude"), json!(at(1))), "{}: {v}", kind.name);
            assert!(attempt["log_updated"].is_u64(), "the attempt's own host reads its log time: {v}");
            assert_eq!(v["last"], json!({"id": 6, "at": at(6)}));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
    // Chain: §7 Reports -> board adapters -> immutable source and close time. Risk: opening reads moving main,
    // or read/decision comments change the report order through updated_at.
    #[test]
    fn research_reports_use_exact_source_and_native_close_time() {
        use base64::{Engine,engine::general_purpose::STANDARD};
        for kind in [&github::KIND,&gitlab::KIND] {
            let sha="a".repeat(40);
            let b=http(kind,Some(true),Box::new(move|method,url,_| {
                assert_eq!(method,"GET");
                let value=if url.contains("/members/all/") {json!({"access_level":20})} else {
                    assert!(url.contains("/contents/")||url.contains("/repository/files/"));
                    assert!(url.ends_with(&format!("?ref={}","a".repeat(40))));assert!(url.contains("docs%2Fresearch%2F12.md"));
                    json!({"encoding":"base64","content":STANDARD.encode("# Question\nInvented source")})
                };
                Ok(Reply {status:200,headers:vec![],body:value.to_string(),operation:operation(method)})
            }));
            assert_eq!(b.source("docs/research/12.md",&sha).unwrap(),"# Question\nInvented source");
            
        }
    }
    /// A recorded GitHub response; tests/fixtures/github-merge-queue/README.md says how to re-record.
    fn recorded(name: &str) -> Value {
        macro_rules! files { ($($n:literal),*) => { match name {
            $($n => include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/github-merge-queue/", $n, ".json")),)*
            _ => panic!("no recording {name}"),
        } } }
        serde_json::from_str(files!("queue-open", "queue-enqueued", "queue-merged", "removal-merge-conflict", "removal-failed-checks", "removal-manual",
            "enqueue-admitted", "enqueue-conflict", "enqueue-already-queued", "readback-enqueued", "readback-none", "ready-for-review",
            "cancel-queued", "cancel-open", "cancel-merged", "dequeue-done", "node-not-found", "rules-merge-queue", "rules-none", "repo-public",
            "pulls-open", "runs-green", "runs-failed", "runs-checks-queued", "runs-none", "checks-green", "checks-failed", "checks-queued", "status-none")).unwrap()
    }
    fn publication_board(kind: &'static Kind, mode: &str, first_merged: bool, second_merged: Option<bool>, ancestry: Option<bool>) -> (Box<dyn Board>, std::rc::Rc<RefCell<Vec<(String, String)>>>) {
        let calls = std::rc::Rc::new(RefCell::new(vec![]));
        let captured = calls.clone();
        let reads = RefCell::new(0);
        let put = RefCell::new(false);
        let comparisons = RefCell::new(0);
        let direct = mode == "direct";
        let h = http(kind, None, Box::new(move |method, url, _| {
            captured.borrow_mut().push((method.into(), url.into()));
            let result = "a".repeat(40);
            let merged = "b".repeat(40);
            let value = if url.contains("/compare/") || url.contains("/merge_base?") {
                *comparisons.borrow_mut() += 1;
                let initial = *comparisons.borrow() == 1;
                if !initial {
                    assert!(url.ends_with(if direct { &result } else { &merged }), "ancestry must use the published SHA");
                    if ancestry.is_none() { return Err(Error::http(503, "HTTP 503")); }
                }
                let on_main = !initial && ancestry == Some(true);
                if kind.name == "github" { json!({"status": if on_main { "identical" } else { "ahead" }}) }
                else { json!({"id": if on_main { if direct { result } else { merged } } else { "c".repeat(40) }}) }
            } else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
            else if url.ends_with("/status") { json!({"total_count": 0}) }
            else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
            else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": "c".repeat(40)}}) }
            else if url.ends_with("/repos/o/r") { json!({"private": true}) }
            else if url.ends_with("/rules/branches/main") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) }
            else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
            else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": "ff", "autoclose_referenced_issues": false}) }
            else if url.contains("/git/ref/heads/taskq-") { json!({"object": {"sha": result}}) }
            else if url.ends_with("/git/refs/heads/main") { json!({"object": {"sha": result}}) }
            else if url.contains("/pulls?") { json!([{"number": 7, "head": {"sha": result}}]) }
            else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result}]) }
            else if url.ends_with("/merge") { *put.borrow_mut() = true; json!({"sha": merged, "squash_commit_sha": merged}) }
            else if url.ends_with("/merge_requests/7") && !*put.borrow() { json!({"state": "opened", "detailed_merge_status": "mergeable"}) }
            else if url.ends_with("/pulls/7") || url.ends_with("/merge_requests/7") {
                *reads.borrow_mut() += 1;
                let yes = if *reads.borrow() == 1 { first_merged } else { second_merged.ok_or("HTTP 503")? };
                json!({"merged": yes, "state": if yes { "merged" } else { "opened" }, "merge_commit_sha": merged})
            } else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        (h, calls)
    }

    // Defect (#1938 A6): a private repository whose plan has no branch rules answered the rules read with 403, and
    // `publish: pr` never merged. Protection: the setup check names that a gap and follows the `try_publish` route.
    #[test]
    fn pr_route_follows_the_publication_route() {
        use crate::check::Probe;
        for (scenario, want) in [("403", "gap"), ("503", "unknown"), ("queue", "ok"), ("public", "gap"), ("strict", "ok"), ("loose", "gap"), ("other", "unknown")] {
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                assert_eq!(method, "GET", "the check only reads");
                let value = if url.ends_with("/rules/branches/main") {
                    match scenario {
                        "403" => return Err(Error::http(403, "GET /rules/branches/main: HTTP 403 Upgrade to GitHub Pro")),
                        "503" => return Err(Error::http(503, "GET /rules/branches/main: HTTP 503")),
                        "queue" => json!([{"type": "merge_queue"}]),
                        "strict" => json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]),
                        _ => json!([]),
                    }
                } else if url.ends_with("/repos/o/r") { match scenario { "public" => json!({"private": false}), "other" => json!({}), _ => json!({"private": true}) } }
                else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
                else if url.ends_with("/branches/main/protection") { return Err(Error::http(404, "HTTP 404")) }
                else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let got = match h.pr_route() { Probe::Ok => "ok", Probe::Gap(_) => "gap", Probe::Unknown(_) => "unknown" };
            assert_eq!(got, want, "{scenario}");
        }
    }

    // Chain: § 6 `taskq doctor` token checks (#2194 A1, A4). Risk: a token without Contents: Read on a private board
    // read issues fine, so doctor called it healthy while commits, reports and publication failed with raw 403s; a
    // read-only Issues or Pull requests grant passed the reads and failed every comment or PR. Protection: each
    // permission is probed on its own; 401/403/404 is a gap naming read or write and the status, an empty repository's
    // 409 still proves Contents: Read, the write probe's 422 proves write without creating anything, and any other
    // failure stays unknown.
    #[test]
    fn token_access_probes_each_permission_and_names_a_403_contents_gap() {
        use crate::check::Probe;
        let show = |p: &Probe| match p { Probe::Ok => "ok".to_string(), Probe::Gap(g) => format!("gap {g}"), Probe::Unknown(_) => "unknown".into() };
        for (scenario, want) in [("403", "gap read HTTP 403"), ("409", "ok"), ("502", "unknown"), ("ok", "ok")] {
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                if method == "POST" {
                    assert_eq!(body, Some(json!({})), "a write probe sends nothing it could create");
                    return Err(Error::http(422, format!("POST {url}: HTTP 422 Validation Failed")));
                }
                if url.contains("/commits?per_page=1") && scenario != "ok" {
                    return Err(Error::http(scenario.parse().unwrap(), format!("GET /commits?per_page=1: HTTP {scenario} Resource not accessible by personal access token")));
                }
                assert!(url.contains("/issues?labels=taskq&per_page=1") || url.contains("/pulls?per_page=1") || url.contains("/commits?per_page=1"), "unexpected {url}");
                Ok(Reply { status: 200, headers: vec![], body: "[]".into(), operation: operation(method) })
            }));
            let access = h.access();
            assert_eq!(access.iter().map(|(n, _)| *n).collect::<Vec<_>>(), ["Issues", "Contents", "Pull requests"]);
            assert_eq!(show(&access[1].1), want, "{scenario}");
            assert!(matches!((&access[0].1, &access[2].1), (Probe::Ok, Probe::Ok)), "{scenario}");
        }
        // a read-only Issues and Pull requests grant: both reads pass, both writes are refused
        let posts = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let seen = posts.clone();
        let h = http(&github::KIND, None, Box::new(move |method, url, _| {
            if method == "POST" {
                seen.borrow_mut().push(url.rsplit('/').next().unwrap_or_default().to_string());
                return Err(Error::http(403, format!("POST {url}: HTTP 403 Resource not accessible by personal access token")));
            }
            Ok(Reply { status: 200, headers: vec![], body: "[]".into(), operation: operation(method) })
        }));
        let access = h.access();
        assert_eq!(access.iter().map(|(_, p)| show(p)).collect::<Vec<_>>(), ["gap write HTTP 403", "ok", "gap write HTTP 403"]);
        assert_eq!(*posts.borrow(), ["issues", "pulls"], "only Issues and Pull requests are write-probed");
    }

    // Chain: § 7 publication on both boards. Risk: publication uses the wrong SHA or writes twice.
    #[test]
    fn incident_publication_uses_merge_sha_and_keeps_outcomes() {
        for kind in BOARDS {
            for mode in ["direct", "pr"] {
                for ancestry in [Some(true), Some(false), None] {
                    crate::incident::tests::take_detected();
                    let (h, calls) = publication_board(kind, mode, true, Some(true), ancestry);
                    let out = if mode == "direct" { h.publish(1, &"a".repeat(40), mode, "title", "Task: fake") }
                        else { h.resume_publication(1, &"a".repeat(40), "pr", "title", "Task: fake", std::path::Path::new("unused"), false) };
                    assert!(matches!(out, Publish::Done(_)));
                    assert_eq!(crate::incident::tests::take_detected(), if ancestry == Some(false) { vec!["publication.not_on_main"] } else { vec![] });
                    assert_eq!(calls.borrow().iter().filter(|(method, _)| method != "GET").count(), 1);
                }
            }
        }
    }

    // Chain: § 7 merged, not closed, on both boards. Risk: a close retried after a merge requeues or merges twice;
    // a PR/MR merged into a release branch (#1039) never closes its task.
    #[test]
    fn a_merged_pr_at_the_result_sha_reconciles_to_one_publication() {
        for (kind, base) in BOARDS.iter().flat_map(|k| [(k, "main"), (k, "release/4.1")]) {
            let calls = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = calls.clone();
            let h = http(kind, None, Box::new(move |method, url, _| {
                captured.borrow_mut().push(method.to_string());
                assert!(!url.contains("main&state=closed") && !url.contains("main&state=merged"), "merged lookup must not filter the base: {url}");
                let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                    else if url.ends_with("/rules/branches/main") { json!([]) }
                    else if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                    else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                    else if url.ends_with("/status") { json!({"total_count": 0}) }
                    else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                    else if url.contains("state=open") { json!([]) }
                    else if url.contains("state=closed") || url.contains("state=merged") {
                        json!([{"number": 6, "iid": 6, "head": {"sha": "d".repeat(40)}, "sha": "d".repeat(40), "merged_at": "x"},
                               {"number": 7, "iid": 7, "head": {"sha": "a".repeat(40)}, "sha": "a".repeat(40), "merged_at": "x", "merge_commit_sha": "b".repeat(40),
                                "base": {"ref": base}, "target_branch": base}])
                    } else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let Publish::Done(m) = h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake") else { panic!("not reconciled") };
            let into = if base == "main" { "" } else { " into release/4.1" };
            assert!(m.starts_with(&if kind.name == "github" { format!("merged #7{into} as bbbb") } else { format!("merged !7{into}") }), "{m}");
            assert!(calls.borrow().iter().all(|m| m == "GET"), "no second merge");
        }
    }

    // Chain: § 7 publication on a private GitHub Free board -> rules 403 probed once -> manual-merge wait with the PR link
    // -> a person merges -> close as today; `selftest` names the plan gap (#1935).
    // Defect (Kirsalina/desiign): the uncached 403 was read every pass and the task sat in review with no reason.
    // Retained: one rules probe per credential, a waiting reason naming the PR, the merged-PR reconcile.
    #[test]
    fn a_free_plan_private_board_probes_rules_once_and_waits_for_a_manual_merge() {
        let (rules, merged) = (std::rc::Rc::new(RefCell::new(0)), std::rc::Rc::new(RefCell::new(false)));
        let (ruled, done) = (rules.clone(), merged.clone());
        let pr = json!({"number": 7, "head": {"sha": "a".repeat(40)}, "html_url": "https://github.com/o/r/pull/7"});
        let h = http(&github::KIND, None, Box::new(move |method, url, _| {
            assert_eq!(method, "GET", "no merge on a manual-merge board: {url}");
            if url.ends_with("/rules/branches/main") {
                *ruled.borrow_mut() += 1;
                let body = json!({"message": "Upgrade to GitHub Pro or make this repository public to enable this feature.", "status": "403"}).to_string();
                return Ok(Reply { status: 403, headers: vec![], body, operation: operation(method) });
            }
            let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                else if url.contains("state=open") { if *done.borrow() { json!([]) } else { json!([pr]) } }
                else if url.contains("state=closed") { json!([{"number": 7, "head": {"sha": "a".repeat(40)}, "merged_at": "x", "merge_commit_sha": "b".repeat(40), "base": {"ref": "main"}}]) }
                else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                else if url.ends_with("/status") { json!({"total_count": 0}) }
                else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        for _ in 0..3 {
            let got = h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake");
            assert!(matches!(&got, Publish::Pending(m) if m.starts_with(&format!("PR #7 {MANUAL_MERGE}: ")) && m.ends_with("merge it: https://github.com/o/r/pull/7")), "{got:?}");
        }
        assert!(h.delivery_gap("pr").unwrap().is_some_and(|g| g.contains("HTTP 403")));
        assert_eq!(h.delivery_gap("direct").unwrap(), None);
        assert_eq!(*rules.borrow(), 1, "one capability probe per board, not one per poll");
        *merged.borrow_mut() = true;
        let got = h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake");
        assert!(matches!(&got, Publish::Done(m) if m.starts_with("merged #7 as bbbb")), "{got:?}");
    }

    // Chain: § 7 publication with no open task PR and merged PRs only at other heads (#1777, pilotq#84).
    // Risk: `Failed` makes `publication-failed` cancel a PR that is gone, its write is refused and publish retries each pass.
    #[test]
    fn a_result_superseded_by_merged_prs_asks_once() {
        let calls = std::rc::Rc::new(RefCell::new(vec![]));
        let captured = calls.clone();
        let h = http(&github::KIND, None, Box::new(move |method, url, _| {
            captured.borrow_mut().push(method.to_string());
            let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                else if url.ends_with("/rules/branches/main") { json!([]) }
                else if url.contains("state=open") { json!([]) }
                else if url.contains("state=closed") {
                    json!([{"number": 86, "head": {"sha": "d".repeat(40)}, "merged_at": "x"},
                           {"number": 87, "head": {"sha": "a".repeat(40)}, "merged_at": null},
                           {"number": 88, "head": {"sha": "e".repeat(40)}, "merged_at": "x"}])
                } else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        let sha = "a".repeat(40);
        let got = h.resume_publication(1, &sha, "pr", "title", "Task: fake", std::path::Path::new("missing-worktree"), false);
        let Publish::Superseded { reason, merged } = got else { panic!("{got:?}") };
        assert_eq!(reason, format!("no open taskq-1 PR at result {sha}; merged taskq-1 PRs: #86 at ddddddd, #88 at eeeeeee"));
        assert_eq!(merged, "#86 at ddddddd, #88 at eeeeeee");
        assert!(calls.borrow().iter().all(|m| m == "GET"), "no publication write");
    }

    // Chain: §7 Result and close verdicts: a task PR merged only at another head closes when it carries the result's patch-id.
    // #1820 (pilotq #106, PR #114): a rebase in review moved the head, the PR merged there and the owner was asked to close it.
    // Retained: a patch that differs (a conflict resolved in the rebase) or an omitted file patch still asks the owner.
    #[test]
    fn a_pr_merged_at_a_rebased_head_with_the_result_patch_closes() {
        let patch = |sp: &str| json!({"files": [{"filename": "src/a.rs", "status": "modified", "patch": format!("@@ -1,2 +1,2 @@
 fn a() {{
-{sp}old();
+{sp}new();")}]});
        for case in ["same", "other", "binary"] {
            let calls = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = calls.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                captured.borrow_mut().push(method.to_string());
                let value = if url.contains("/compare/main...") { json!({"status": "diverged"}) }
                    else if url.ends_with("/rules/branches/main") { json!([]) }
                    else if url.contains("state=open") { json!([]) }
                    else if url.contains("state=closed") {
                        json!([{"number": 114, "head": {"sha": "d".repeat(40)}, "merged_at": "x", "merge_commit_sha": "b".repeat(40), "base": {"ref": "main"}}])
                    } else if url.ends_with(&format!("/commits/{}", "b".repeat(40))) { json!({"parents": [{"sha": "c".repeat(40)}]}) }
                    // the rebased head: other line numbers and indentation, same change
                    else if url.contains(&format!("...{}", "d".repeat(40))) { patch("    ").to_string().replace("-1,2 +1,2", "-40,2 +40,2").parse().unwrap() }
                    else if url.contains(&format!("...{}", "a".repeat(40))) {
                        match case { "same" => patch(""), "other" => patch("").to_string().replace("new()", "merged()").parse().unwrap(),
                            _ => json!({"files": [{"filename": "src/a.rs", "status": "modified"}]}) }
                    }
                    else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                    else if url.ends_with("/status") { json!({"total_count": 0}) }
                    else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let sha = "a".repeat(40);
            let got = h.resume_publication(1, &sha, "pr", "title", "Task: fake", std::path::Path::new("missing-worktree"), false);
            match case {
                "same" => assert_eq!(got, Publish::Done(format!("merged #114 as {} (head ddddddd, the result's patch-id); exact-SHA CI green on {}", "b".repeat(40), "d".repeat(40)))),
                _ => assert!(matches!(&got, Publish::Superseded { merged, .. } if merged == "#114 at ddddddd"), "{case}: {got:?}"),
            }
            assert!(calls.borrow().iter().all(|m| m == "GET"), "no publication write");
        }
    }

    // Chain: section 7 intent -> expected-head admission -> merge_group CI -> observed merge receipt.
    // Risks: divergent heads spend retries, lost mutations count as admission, or head CI closes an unmerged task.
    // #1820 (pilotq #106): a PR rebased in review, a conflict after a sibling merged, or a refused enqueue left the task in review,
    // silent, for 50 minutes. Retained: each such pass ends requeued with the PR, files or check named, re-enqueued, or waiting with a reason.
    #[test]
    fn github_queue_qualifies_the_merge_result_and_preserves_unknowns() {
        for scenario in ["new", "draft", "waiting", "merged", "conflict", "failed", "removed", "unknown", "readback", "public", "graphql", "rules-unknown", "merge-ci-missing", "merge-ci-failed", "merge-ci-pending", "merge-push-pending", "merge-push-failed", "merge-check-failed", "enqueue-conflict", "enqueue-race", "merged-flag-lag",
            "head-moved", "conflicting", "conflict-cleared", "conflict-unknown", "enqueue-checks-running", "enqueue-checks-failed"] {
            // #1595: every body is a recorded GitHub response; invented removal reasons once hid GitHub's lowercase codes.
            let merged = scenario == "merged" || scenario.starts_with("merge-");
            let mut state = recorded(if scenario == "waiting" { "queue-enqueued" } else if merged { "queue-merged" } else { "queue-open" });
            // Overlay: GitHub keeps no unmerged PR after a removal, so the open PR takes a recorded removal timeline.
            let removal = match scenario { "conflict" | "conflict-cleared" | "conflict-unknown" => Some(recorded("removal-merge-conflict")), "failed" => Some(recorded("removal-failed-checks")),
                "removed" => Some(recorded("removal-manual")), "merged-flag-lag" => Some(recorded("queue-merged")), _ => None };
            if let Some(r) = removal { state["data"]["node"]["timelineItems"] = r["data"]["node"]["timelineItems"].clone(); }
            // Overlay: GitHub's mergeability after a sibling PR merged (CONFLICTING) or the conflicting entry ahead left (MERGEABLE).
            match scenario { "conflict" | "conflicting" => state["data"]["node"]["mergeable"] = "CONFLICTING".into(), "conflict-cleared" => state["data"]["node"]["mergeable"] = "MERGEABLE".into(), _ => {} }
            // Overlay: no recording holds a non-conflict refusal; only its UNPROCESSABLE type matters, never its wording.
            let mut refusal = recorded("enqueue-already-queued");
            refusal["errors"][0]["message"] = "Required status check build is in progress".into();
            let (runs, mut checks) = match scenario {
                "merge-ci-missing" => (recorded("runs-none"), Value::Null),
                "merge-ci-failed" | "enqueue-checks-failed" => (recorded("runs-failed"), recorded("checks-failed")),
                "merge-ci-pending" | "enqueue-checks-running" => (recorded("runs-checks-queued"), recorded("checks-queued")),
                _ => (recorded("runs-green"), recorded("checks-green")),
            };
            // Overlay: the merge result is the commit the CI recording ran on.
            if let (true, Some(sha)) = (merged, runs["workflow_runs"][0]["head_sha"].as_str()) { state["data"]["node"]["mergeCommit"]["oid"] = sha.into(); }
            // Overlay: #1343 the push suite still running after the merge; #1413 a failed push retest never holds close; a failed merge_group check does.
            let flip = match scenario { "merge-push-pending" => Some(("build", "in_progress", Value::Null)), "merge-push-failed" => Some(("build", "completed", json!("failure"))),
                "merge-check-failed" => Some(("plan", "completed", json!("failure"))), _ => None };
            if let Some((name, status, conclusion)) = flip {
                let run = checks["check_runs"].as_array_mut().unwrap().iter_mut().find(|c| c["name"] == name).unwrap();
                run["status"] = status.into();
                run["conclusion"] = conclusion;
            }
            let head = state["data"]["node"]["headRefOid"].as_str().unwrap().to_string();
            let merge = state["data"]["node"]["mergeCommit"]["oid"].as_str().unwrap_or_default().to_string();
            // Overlay: the PR list names the recorded PR state's head.
            let mut pulls = recorded("pulls-open");
            pulls[0]["head"]["sha"] = head.clone().into();
            // Overlay: #106 the reviewer pushed a rebased head after the result.
            let moved = "f".repeat(40);
            if scenario == "head-moved" { pulls[0]["head"]["sha"] = moved.clone().into(); }
            pulls[0]["draft"] = (scenario == "draft").into();
            let pr = pulls[0]["node_id"].clone();
            let writes = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = writes.clone();
            let rules = std::rc::Rc::new(RefCell::new(0));
            let ruled = rules.clone();
            let sent = head.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                if method != "GET" { captured.borrow_mut().push(body.clone().unwrap_or_default()); }
                let value = if url.contains("/compare/") {
                    // Overlay: compare bodies with a merge base and changed files; the PR and main both changed src/service.rs.
                    let base = "c".repeat(40);
                    let files = if url.contains(&format!("/compare/{base}...main")) { json!([{"filename": "src/service.rs"}, {"filename": "docs/x.md"}]) }
                        else { json!([{"filename": "README.md"}, {"filename": "src/service.rs"}]) };
                    json!({"status": if !merge.is_empty() && url.ends_with(&merge) { "identical" } else { "diverged" }, "merge_base_commit": {"sha": base}, "files": files})
                }
                    else if url.ends_with("/rules/branches/main") { *ruled.borrow_mut() += 1; if scenario == "rules-unknown" { return Err(Error::http(403, "HTTP 403")); } recorded(if scenario == "public" { "rules-none" } else { "rules-merge-queue" }) }
                    else if url.ends_with("/repos/o/r") { assert_eq!(scenario, "public"); recorded("repo-public") }
                    else if url.contains("/pulls?") { pulls.clone() }
                    else if url.ends_with("/graphql") {
                        let body = body.unwrap();
                        let query = body["query"].as_str().unwrap();
                        assert_eq!(body["variables"]["id"], pr);
                        if scenario == "graphql" { recorded("node-not-found") }
                        else if query.contains("enqueuePullRequest") {
                            assert_eq!(body["variables"]["sha"], head);
                            match scenario { "enqueue-checks-running" | "enqueue-checks-failed" => refusal.clone(),
                                _ => recorded(match scenario { "enqueue-conflict" => "enqueue-conflict", "enqueue-race" => "enqueue-already-queued", _ => "enqueue-admitted" }) }
                        } else if query.contains("markPullRequestReadyForReview") { recorded("ready-for-review") }
                        else if !query.contains("timelineItems") { recorded(if scenario == "readback" { "readback-none" } else { "readback-enqueued" }) }
                        else { state.clone() }
                    }
                    else if url.contains("/actions/runs?") { assert!(url.contains(&merge)); runs.clone() }
                    else if url.contains("/check-runs") { assert!(url.contains(&merge)); checks.clone() }
                    else if url.ends_with("/status") { assert!(url.contains(&merge)); recorded("status-none") }
                    else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let prior = !matches!(scenario, "new" | "draft" | "readback" | "public" | "graphql" | "enqueue-conflict" | "enqueue-race" | "head-moved" | "conflicting" | "enqueue-checks-running" | "enqueue-checks-failed");
            let got = h.resume_publication(1, &sent, "pr", "title", "Task: invented", std::path::Path::new("missing-worktree"), prior);
            if scenario=="waiting" {assert!(h.take_publication_evidence().is_empty(),"a recovered queue cannot reconstruct pre-admission inputs");}
            match scenario {
                "new" | "draft" | "waiting" | "conflict-cleared" => assert!(matches!(got, Publish::Enqueued(_)), "{scenario}: {got:?}"),
                "conflict" | "enqueue-conflict" | "conflicting" => assert_eq!(got, Publish::MainMoved("PR #1590 conflicts with main; conflict files: src/service.rs".into()), "{scenario}"),
                "head-moved" => assert_eq!(got, Publish::MainMoved(format!("PR #1590 head {moved:.7} is not the result {sent:.7}; deliver the PR head as a new result"))),
                "enqueue-checks-running" => assert!(matches!(&got, Publish::Pending(m) if m.starts_with("merge queue did not admit PR #1590: Required status check build is in progress; CI ") && m.contains(" pending on ")), "{got:?}"),
                "conflict-unknown" => assert_eq!(got, Publish::Pending("merge queue removed PR #1590 as merge_conflict; mergeability not computed yet".into())),
                "enqueue-checks-failed" => assert!(matches!(&got, Publish::Failed(m) if m.starts_with("merge queue did not admit PR #1590: ")), "{got:?}"),
                "merged" | "merge-push-pending" => assert!(matches!(&got, Publish::Done(m) if !m.contains("push CI")), "{scenario}: {got:?}"),
                "merge-push-failed" => assert!(matches!(&got, Publish::Done(m) if m.ends_with("merge-result CI green; push CI build failure ignored")), "{got:?}"),
                // #1413: merged with failed merge-result CI or removed from the queue: only the owner can unblock it
                "merge-ci-failed" | "merge-check-failed" | "removed" => assert!(matches!(got, Publish::Blocked(_)), "{scenario}: {got:?}"),
                "failed" => assert!(matches!(got, Publish::Failed(_)), "{got:?}"),
                // #1489: reason merged before the merged flag rereads next pass instead of asking; an enqueue race rereads too
                _ => assert!(matches!(got, Publish::Pending(_)), "{scenario}: {got:?}"),
            }
            if scenario == "waiting" {
                // #1359: a waiting publication polls the queue each pass; main rules stay read once per process.
                assert!(matches!(h.resume_publication(1, &sent, "pr", "title", "Task: invented", std::path::Path::new("missing-worktree"), prior), Publish::Enqueued(_)));
                assert_eq!(*rules.borrow(), 1, "main rules reread");
            }
            assert!(!writes.borrow().iter().any(|b| b.get("merge_method").is_some()), "queue never calls REST merge");
            assert_eq!(writes.borrow().iter().filter(|b| b["query"].as_str().is_some_and(|q| q.contains("enqueuePullRequest"))).count(), usize::from(matches!(scenario, "new" | "draft" | "readback" | "enqueue-conflict" | "enqueue-race" | "conflict-cleared" | "enqueue-checks-running" | "enqueue-checks-failed")), "{scenario}: one enqueue call");
        }
    }

    // Chain: section 7 rule 4 publish once -> main moved after the push, no conflict -> enqueue at the result SHA -> merge_group CI on the merge result -> close (#1842).
    // #1842: workers rebased and rechecked up to 5 times per attempt because main moved. Retained: a clean PR behind a moved main
    // is enqueued at the unchanged result, never rebased or pushed, and closes on the merge result's CI.
    #[test]
    fn a_moved_main_without_conflict_publishes_the_result_unrebased() {
        let state = std::rc::Rc::new(RefCell::new(recorded("queue-open")));
        let head = state.borrow()["data"]["node"]["headRefOid"].as_str().unwrap().to_string();
        state.borrow_mut()["data"]["node"]["mergeable"] = "MERGEABLE".into();
        let mut runs = recorded("runs-green");
        runs["workflow_runs"][0]["id"]=123.into();runs["workflow_runs"][0]["run_attempt"]=2.into();
        let merge = runs["workflow_runs"][0]["head_sha"].as_str().unwrap().to_string();
        let mut pulls = recorded("pulls-open");
        pulls[0]["head"]["sha"] = head.clone().into();
        pulls[0]["base"]["sha"] = "b".repeat(40).into();
        let writes = std::rc::Rc::new(RefCell::new(vec![]));
        let (captured, read, sent, merged) = (writes.clone(), state.clone(), head.clone(), merge.clone());
        let h = http(&github::KIND, None, Box::new(move |method, url, body| {
            // GraphQL reads are POSTs too; only mutations and REST writes change the PR or its branch.
            let read_only = body.as_ref().is_some_and(|b| b["query"].as_str().is_some_and(|q| q.starts_with("query")));
            if method != "GET" && !read_only { captured.borrow_mut().push(format!("{method} {url} {}", body.clone().unwrap_or_default())); }
            // Overlay: main moved after the push; the result is neither on main nor its ancestor.
            let value = if url.contains("/compare/") { json!({"status": if url.ends_with(&merged) { "identical" } else { "diverged" }}) }
                else if url.ends_with("/rules/branches/main") { recorded("rules-merge-queue") }
                else if url.contains("/pulls?") { pulls.clone() }
                else if url.ends_with("/graphql") {
                    let body = body.unwrap();
                    let query = body["query"].as_str().unwrap();
                    if query.contains("enqueuePullRequest") { assert_eq!(body["variables"]["sha"], sent, "enqueued at the result SHA"); recorded("enqueue-admitted") }
                    else if !query.contains("timelineItems") { recorded("readback-enqueued") }
                    else { read.borrow().clone() }
                }
                else if url.ends_with(&format!("/commits/{merged}")) { json!({"sha":merged,"parents":[{"sha":"b".repeat(40)}]}) }
                else if url.contains("/actions/runs?") { assert!(url.contains(&merged), "CI read on the merge result: {url}"); runs.clone() }
                else if url.contains("/check-runs") { assert!(url.contains(&merged), "CI read on the merge result: {url}"); recorded("checks-green") }
                else if url.ends_with("/status") { recorded("status-none") }
                else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        // A rebase would need the task worktree; this host has none, so any rebase attempt fails the pass.
        let root = std::path::Path::new("missing-worktree");
        assert!(matches!(h.resume_publication(1, &head, "pr", "title", "Task: invented", root, false), Publish::Enqueued(_)));
        let frozen=h.take_publication_evidence();
        assert_eq!(frozen.len(),1);assert_eq!(frozen[0].base.as_deref(),Some("b".repeat(40).as_str()));
        assert!(frozen[0].merged.is_none());
        assert!(writes.borrow().iter().all(|w| w.contains("enqueuePullRequest")), "only the enqueue writes: {:?}", writes.borrow());
        // Overlay: the merge queue merged the PR at the result head; the merge result is the commit the CI recording ran on.
        let mut done = recorded("queue-merged");
        done["data"]["node"]["headRefOid"] = head.clone().into();
        done["data"]["node"]["mergeCommit"]["oid"] = merge.clone().into();
        *state.borrow_mut() = done;
        let got = h.resume_publication(1, &head, "pr", "title", "Task: invented", root, true);
        assert!(matches!(&got, Publish::Done(m) if m.contains(&format!("as {merge}; merge-result CI green"))), "{got:?}");
        let landed=h.take_publication_evidence();
        assert_eq!(landed.len(),1);assert_eq!(landed[0].parent,frozen[0].base);
        assert_eq!(landed[0].ci_sha.as_deref(),Some(merge.as_str()));
        assert_eq!(landed[0].ci_run_id,Some(123));assert_eq!(landed[0].ci_run_attempt,Some(2));
        assert!(h.take_publication_evidence().is_empty(),"publication observations are consumed once");
        assert_eq!(writes.borrow().len(), 1, "one enqueue, no rebase or push: {:?}", writes.borrow());
    }

    /// A GitHub board `o/r` whose task 1 result SHA lives in repository `o/x`: the board repository's compare is 404,
    /// the trusted log links `o/x` PR 1590, and `foreign` answers every `o/x` read (#2047).
    fn foreign_pr_board(foreign: impl Fn(&str, &str, Option<Value>) -> Value + 'static) -> Box<dyn Board> {
        use crate::core::entry;
        let head = recorded("pulls-open")[0]["head"]["sha"].as_str().unwrap().to_string();
        let log = [
            entry("result", &["1111111111111111111111111111111111111111"], Some(1), "Changed: older attempt [o/y#1](https://github.com/o/y/pull/1)"),
            entry("requeue", &[], None, "worker again"),
            entry("result", &[&head], Some(2), "Changed: [o/x#1590](https://github.com/o/x/pull/1590), board change https://github.com/o/r/pull/3."),
            entry("publish", &[], None, "Changed: [o/x#1590](https://github.com/o/x/pull/1590)"),
        ];
        http(&github::KIND, None, Box::new(move |method, url, body| {
            let reply = |status: u16, v: Value| Ok(Reply { status, headers: vec![], body: v.to_string(), operation: operation(method) });
            if let Some(rest) = url.strip_prefix("https://api.github.com/repos/o/r") {
                return if rest.starts_with("/compare/") { reply(404, json!({"message": "Not Found"})) }
                    else if rest.starts_with("/pulls?") { reply(200, json!([])) }
                    else if rest.starts_with("/issues/1/comments") {
                        reply(200, json!(log.iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "created_at": "2026-10-30T00:00:00Z",
                            "updated_at": "2026-10-30T00:00:00Z", "author_association": "OWNER", "user": {"login": "owner"}})).collect::<Vec<_>>()))
                    } else { panic!("board repository read {method} {url}") };
            }
            let v = foreign(method, url, body);
            if v["status"] == 404 { reply(404, json!({"message": "Not Found"})) } else { reply(200, v) }
        }))
    }

    // #2047 defect (#2009, #2010, #2015): a task PR in another repository looped on the board repository's
    // `compare main...<sha>` HTTP 404; no stage, requeue refused. Chain: section 7 Publication routes, foreign PR ->
    // that repository's merge queue, merge state and CI -> close. Risk kept: a foreign PR is read in the board
    // repository, merges unqualified, or an unreadable PR retries silently.
    #[test]
    fn a_task_pr_in_another_repository_publishes_there_and_blocks_by_name() {
        let state = std::rc::Rc::new(RefCell::new(recorded("queue-open")));
        let head = recorded("pulls-open")[0]["head"]["sha"].as_str().unwrap().to_string();
        state.borrow_mut()["data"]["node"]["headRefOid"] = head.clone().into();
        state.borrow_mut()["data"]["node"]["mergeable"] = "MERGEABLE".into();
        let runs = recorded("runs-green");
        let merge = runs["workflow_runs"][0]["head_sha"].as_str().unwrap().to_string();
        let (read, merged, sent) = (state.clone(), merge.clone(), head.clone());
        let h = foreign_pr_board(move |method, url, body| {
            if url.ends_with("/graphql") {
                let body = body.unwrap();
                let query = body["query"].as_str().unwrap();
                return if query.contains("enqueuePullRequest") { assert_eq!(body["variables"]["sha"], sent); recorded("enqueue-admitted") }
                    else if !query.contains("timelineItems") { recorded("readback-enqueued") }
                    else { read.borrow().clone() };
            }
            let rest = url.strip_prefix("https://api.github.com/repos/o/x").unwrap_or_else(|| panic!("read outside o/x: {method} {url}"));
            if rest.starts_with("/compare/") { json!({"status": "diverged"}) }
            else if rest == "/pulls/1590" { recorded("pulls-open")[0].clone() }
            else if rest == "/rules/branches/main" { recorded("rules-merge-queue") }
            else if rest.starts_with("/actions/runs?") { assert!(rest.contains(&merged)); runs.clone() }
            else if rest.contains("/check-runs") { recorded("checks-green") }
            else if rest.ends_with("/status") { recorded("status-none") }
            else { panic!("unexpected {method} {url}") }
        });
        let root = std::path::Path::new("missing-worktree");
        let got = h.resume_publication(1, &head, "pr", "title", "Task: invented", root, false);
        assert!(matches!(got, Publish::Enqueued(_)), "A1/A2: enqueued in o/x, so status shows the merge queue stage: {got:?}");
        let frozen=h.take_publication_evidence();
        assert_eq!(frozen[0].repository,"o/x","advisory follows the actual publication repository");
        let mut done = recorded("queue-merged");
        done["data"]["node"]["headRefOid"] = head.clone().into();
        done["data"]["node"]["mergeCommit"]["oid"] = merge.clone().into();
        *state.borrow_mut() = done;
        let got = h.resume_publication(1, &head, "pr", "title", "Task: invented", root, true);
        assert!(matches!(&got, Publish::Done(m) if m.starts_with(&format!("merged #1590 in o/x as {merge}; merge-result CI green"))), "A1: {got:?}");
        assert_eq!(h.take_publication_evidence()[0].repository,"o/x");
        assert!(matches!(h.resume_publication(1, &head, "direct", "title", "Task: invented", root, false), Publish::Blocked(e) if e.starts_with("o/x#1590: ")));

        // A3: an unreadable PR is a named blocker at once, not a pending retry.
        let h = foreign_pr_board(|method, url, _| match url.strip_prefix("https://api.github.com/repos/o/x") {
            Some("/pulls/1590") => json!({"status": 404}),
            _ => panic!("unexpected {method} {url}"),
        });
        let got = h.resume_publication(1, &head, "pr", "title", "Task: invented", root, false);
        assert!(matches!(&got, Publish::Blocked(e) if e.starts_with("publication PR o/x#1590 unreachable: GET ")), "A3: {got:?}");
        assert_eq!(got.outcome().0, "ask");
        let mut closed = recorded("pulls-open")[0].clone();
        closed["state"] = "closed".into();
        let h = foreign_pr_board(move |method, url, _| match url.strip_prefix("https://api.github.com/repos/o/x") {
            Some(r) if r.starts_with("/compare/") => json!({"status": "diverged"}),
            Some("/pulls/1590") => closed.clone(),
            Some("/rules/branches/main") => recorded("rules-merge-queue"),
            _ => panic!("unexpected {method} {url}"),
        });
        assert!(matches!(h.publish(1, &head, "pr", "title", "Task: invented"), Publish::Blocked(e) if e.starts_with("o/x#1590: publication PR closed unmerged")));
    }

    // #2047 A3 defect: requeue of a task whose PR lives in another repository was refused as 'publication PR absent or
    // ambiguous'. Chain: section 7 cancel dequeues -> the foreign PR's queue entry. Risk kept: an unreadable or merged
    // foreign PR never claims cancellation.
    #[test]
    fn requeue_cancels_a_task_pr_in_another_repository() {
        let head = recorded("pulls-open")[0]["head"]["sha"].as_str().unwrap().to_string();
        for scenario in ["queued", "merged", "unreachable"] {
            let removed = std::rc::Rc::new(RefCell::new(false));
            let done = removed.clone();
            let h = foreign_pr_board(move |method, url, body| {
                if url.ends_with("/graphql") {
                    return if body.unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest") { *done.borrow_mut() = true; recorded("dequeue-done") }
                        else { recorded(match scenario { "merged" => "cancel-merged", _ if *done.borrow() => "cancel-open", _ => "cancel-queued" }) };
                }
                match url.strip_prefix("https://api.github.com/repos/o/x") {
                    Some("/pulls/1590") if scenario == "unreachable" => json!({"status": 404}),
                    Some("/pulls/1590") => recorded("pulls-open")[0].clone(),
                    _ => panic!("unexpected {method} {url}"),
                }
            });
            let result = h.cancel_publication(1, &head);
            assert_eq!(result.is_ok(), scenario == "queued", "{scenario}: {result:?}");
            assert_eq!(*removed.borrow(), scenario == "queued", "{scenario}");
            if let Err(e) = result { assert!(e.text().contains("o/x#1590"), "{scenario}: the blocker names the PR: {e}"); }
        }
    }

    // #2061 defect (#2010 pilotq#200, #2015 dashq#241): a result naming its PR as `owner/name#P` compared in the board
    // repository (HTTP 404 every pass), and a PR merged at a rebased head whose diff GitHub omitted a file patch of, or
    // whose hunks a rebase reordered, asked the owner. Chain: publication (agent-behavior.md, publication.md) -> the
    // named PR's repository only -> merged head carries the result's changes -> close with the merge commit.
    // Retained: a mention that is not the task's PR is skipped; a merged head with other changes still asks the owner.
    #[test]
    fn a_task_pr_named_by_reference_publishes_in_its_repository_and_closes_its_rebased_merge() {
        use crate::core::entry;
        let (sha, head, merge, base) = ("a".repeat(40), "d".repeat(40), "b".repeat(40), "c".repeat(40));
        let views = |patch: &str| json!({"filename": "docs/views.md", "status": "modified", "sha": "1".repeat(40), "patch": patch});
        let blob = |b: &str| json!({"filename": "assets/app.js", "status": "modified", "sha": b.repeat(40)});
        // the merged head: main inserted context next to the change and the diff moved the removed line after the added ones
        let merged = json!({"files": [views("@@ -41,3 +41,4 @@ ## Views\n sibling row\n+row new\n+row two\n-row old\n context"), blob("e")]});
        for case in ["same", "binary", "lines"] {
            let result = json!({"files": [
                views(if case == "lines" { "@@ -38,3 +38,4 @@\n before\n-row old\n+row new\n+row three\n after" } else { "@@ -38,3 +38,4 @@\n before\n-row old\n+  row new\n+row two\n after" }),
                blob(if case == "binary" { "f" } else { "e" })]});
            // the last named PR is read first: o/y#7 is only a mention
            let log = [entry("result", &[&sha], Some(1), "Changed: delivered in taskq-style o/x#241, follows o/y#7.\nChecks: A1 ok\nOpen: none")];
            let reads = std::rc::Rc::new(RefCell::new(vec![]));
            let seen = reads.clone();
            let h = {
                let (sha, head, merge, base, merged) = (sha.clone(), head.clone(), merge.clone(), base.clone(), merged.clone());
                http(&github::KIND, None, Box::new(move |method, url, _| {
                    assert_eq!(method, "GET", "no publication write: {url}");
                    seen.borrow_mut().push(url.to_string());
                    let value = if let Some(rest) = url.strip_prefix("https://api.github.com/repos/o/r") {
                        // A1: the board repository serves only the task log; any compare or PR read there panics
                        assert!(rest.starts_with("/issues/1/comments"), "board repository read {url}");
                        json!(log.iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "created_at": "2026-10-30T00:00:00Z",
                            "updated_at": "2026-10-30T00:00:00Z", "author_association": "OWNER", "user": {"login": "owner"}})).collect::<Vec<_>>())
                    } else if url == "https://api.github.com/repos/o/y/pulls/7" {
                        json!({"number": 7, "state": "open", "head": {"ref": "feature", "sha": "9".repeat(40)}})
                    } else {
                        let rest = url.strip_prefix("https://api.github.com/repos/o/x").unwrap_or_else(|| panic!("read outside o/x: {url}"));
                        if rest == "/pulls/241" {
                            json!({"number": 241, "state": "closed", "merged_at": "x", "merge_commit_sha": merge, "head": {"ref": "taskq-1", "sha": head}, "base": {"ref": "main"}})
                        } else if rest == format!("/compare/main...{sha}") { json!({"status": "diverged"}) }
                        else if rest == "/rules/branches/main" { json!([]) }
                        else if rest == format!("/commits/{merge}") { json!({"parents": [{"sha": base}]}) }
                        else if rest == format!("/compare/{base}...{head}") { merged.clone() }
                        else if rest == format!("/compare/{base}...{sha}") { result.clone() }
                        else if rest.contains("/check-runs") { assert!(rest.contains(&head)); json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                        else if rest.ends_with("/status") { json!({"total_count": 0}) }
                        else { panic!("unexpected {method} {url}") }
                    };
                    Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
                }))
            };
            let got = h.resume_publication(1, &sha, "pr", "title", "Task: invented", std::path::Path::new("missing-worktree"), false);
            match case {
                // A2: the rebased merge carries the reviewed changes; it closes with its merge commit, no ask
                "same" => assert_eq!(got, Publish::Done(format!("merged #241 in o/x as {merge} (head ddddddd, the result's changes); exact-SHA CI green on {head}"))),
                // A3: another blob or other changed lines keep the publication-blocked ask, named by the PR
                _ => {
                    assert!(matches!(&got, Publish::Superseded { reason, merged } if reason.starts_with("o/x#241: ") && merged == "#241 at ddddddd"), "{case}: {got:?}");
                    assert_eq!(got.outcome().0, "ask", "{case}");
                }
            }
            // A1: the mention o/y#7 is read once and skipped; every other read is in o/x
            assert_eq!(reads.borrow().iter().filter(|u| u.contains("/repos/o/y/")).count(), 1, "{case}: {:?}", reads.borrow());
        }
    }

    // #2061 A4 defect (#2010): the board repository's compare HTTP 404 was the only reason shown for a result the board
    // repository does not hold. Chain: publication -> no task PR of another repository named -> one stable reason
    // naming the repository compared, so the service logs it once. Retained: the publication waits, no ask.
    #[test]
    fn a_result_missing_from_the_board_repository_waits_with_one_reason_naming_it() {
        let sha = "a".repeat(40);
        let log = [crate::core::entry("result", &[&sha], Some(1), "Changed: x, see #12\nChecks: A1 ok\nOpen: none")];
        let h = http(&github::KIND, None, Box::new(move |method, url, _| {
            let rest = url.strip_prefix("https://api.github.com/repos/o/r").unwrap_or_else(|| panic!("read outside o/r: {method} {url}"));
            let reply = |status: u16, v: Value| Ok(Reply { status, headers: vec![], body: v.to_string(), operation: operation(method) });
            if rest.starts_with("/compare/") { reply(404, json!({"message": "Not Found"})) }
            else if rest.starts_with("/issues/1/comments") {
                reply(200, json!(log.iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "created_at": "2026-10-30T00:00:00Z",
                    "updated_at": "2026-10-30T00:00:00Z", "author_association": "OWNER", "user": {"login": "owner"}})).collect::<Vec<_>>()))
            } else { panic!("unexpected {method} {url}") }
        }));
        let want = Publish::Pending(format!("result {sha} not found in o/r (compare HTTP 404); the result names no task PR of another repository"));
        for pass in 0..2 { assert_eq!(h.publish(1, &sha, "pr", "title", "Task: invented"), want, "pass {pass}"); }
    }

    // #2308 defect (#2302 agentq#206): a worker merged the PR of another repository itself and handed in the merge
    // commit; that PR was read as a mention (head neither `taskq-<N>` nor the result), the board repository's compare
    // was HTTP 404 and the publication waited every pass with no path to close. Chain: publication -> the named PR
    // merged as the result -> its merge-result CI -> close naming that repository. Retained: a merged PR at another
    // merge commit stays a mention; a failed merge-result CI blocks, never closes unqualified.
    #[test]
    fn a_result_merged_as_another_repositorys_merge_commit_closes_there() {
        use crate::core::entry;
        let (sha, head) = ("a".repeat(40), "d".repeat(40));
        for case in ["queue", "queue-red", "no-queue", "direct", "other-merge"] {
            let log = [entry("result", &[&sha], Some(1), "Changed: o/x PR o/x#206 merged.\nChecks: A1 ok\nOpen: none")];
            let (sha2, head2) = (sha.clone(), head.clone());
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                assert_eq!(method, "GET", "no publication write: {url}");
                let reply = |status: u16, v: Value| Ok(Reply { status, headers: vec![], body: v.to_string(), operation: operation(method) });
                if let Some(rest) = url.strip_prefix("https://api.github.com/repos/o/r") {
                    return if rest.starts_with("/compare/") && case == "other-merge" { reply(404, json!({"message": "Not Found"})) }
                        else if rest.starts_with("/issues/1/comments") {
                            reply(200, json!(log.iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "created_at": "2026-10-30T00:00:00Z",
                                "updated_at": "2026-10-30T00:00:00Z", "author_association": "OWNER", "user": {"login": "owner"}})).collect::<Vec<_>>()))
                        } else { panic!("board repository read {url}") };
                }
                let rest = url.strip_prefix("https://api.github.com/repos/o/x").unwrap_or_else(|| panic!("read outside o/x: {url}"));
                let merge = if case == "other-merge" { "b".repeat(40) } else { sha2.clone() };
                let suite = |ok: &str| json!({"workflow_runs": [{"head_sha": sha2, "event": "merge_group", "status": "completed", "conclusion": ok, "name": "Test", "check_suite_id": 5}]});
                let checks = json!({"check_runs": [{"app": {"slug": "github-actions"}, "check_suite": {"id": 5}, "status": "completed", "conclusion": "success", "name": "Test"}]});
                reply(200, if rest == "/pulls/206" {
                    json!({"number": 206, "state": "closed", "merged_at": "x", "merge_commit_sha": merge, "head": {"ref": "coreq-pin", "sha": head2}, "base": {"ref": "main"}})
                } else if rest == format!("/compare/main...{sha2}") { json!({"status": "identical"}) }
                else if rest == "/rules/branches/main" { if case == "no-queue" { json!([]) } else { recorded("rules-merge-queue") } }
                else if rest.starts_with("/actions/runs?") { assert!(rest.contains(&sha2)); suite(if case == "queue-red" { "failure" } else { "success" }) }
                else if rest.contains("/check-runs") { assert!(rest.contains(if case == "no-queue" { &head2 } else { &sha2 }), "{case}: {rest}"); checks }
                else if rest.ends_with("/status") { json!({"total_count": 0}) }
                else { panic!("{case}: unexpected {method} {url}") })
            }));
            let mode = if case == "direct" { "direct" } else { "pr" };
            let got = h.resume_publication(1, &sha, mode, "title", "Task: invented", std::path::Path::new("missing-worktree"), false);
            match case {
                // A1: the merge in o/x is the publication; it closes with its merge-result CI, no taskq PR
                "queue" | "direct" => assert_eq!(got, Publish::Done(format!("merged #206 in o/x as {sha}; merge-result CI green")), "{case}"),
                "no-queue" => assert_eq!(got, Publish::Done(format!("merged #206 in o/x as {sha}; exact-SHA CI green on {head}")), "{case}"),
                "queue-red" => assert!(matches!(&got, Publish::Blocked(e) if e.starts_with("o/x#206: PR #206 merged as ") && e.contains("failure")), "{case}: {got:?}"),
                _ => assert!(matches!(&got, Publish::Pending(e) if e.contains("names no task PR of another repository")), "{case}: {got:?}"),
            }
        }
    }

    // Chain: section 7 private fallback real Git fetch/rebase/leased push, or conflict/dirty preservation.
    // An HTTP-only fixture cannot prove preservation of the worktree or the pushed branch.
    #[test]
    fn private_fallback_rebases_its_owned_worktree_and_preserves_conflicts() {
        for mode in ["clean", "conflict", "dirty"] {
            let dir = std::env::temp_dir().join(format!("taskq-fallback-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&dir).unwrap();
            let root = dir.join("project");
            let remote = dir.join("remote.git");
            let git = |at: &std::path::Path, args: &[&str]| {
                let out = std::process::Command::new("git").current_dir(at).args(args).env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
                assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
                String::from_utf8(out.stdout).unwrap().trim().to_string()
            };
            git(&dir, &["init", "--bare", remote.to_str().unwrap()]);
            git(&dir, &["init", "-b", "main", root.to_str().unwrap()]);
            for (key, value) in [("user.name", "Invented fixture"), ("user.email", "fixture@example.invalid"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] { git(&root, &["config", key, value]); }
            std::fs::write(root.join("base"), "base\n").unwrap();
            git(&root, &["add", "."]); git(&root, &["commit", "-m", "base"]);
            git(&root, &["remote", "add", "origin", remote.to_str().unwrap()]);
            git(&root, &["push", "origin", "main"]);
            git(&root, &["worktree", "add", "-b", "taskq-1", ".worktrees/taskq-1", "origin/main"]);
            let wt = root.join(".worktrees/taskq-1");
            std::fs::write(wt.join(if mode == "conflict" { "base" } else { "worker" }), "worker\n").unwrap();
            git(&wt, &["add", "."]); git(&wt, &["commit", "-m", "worker"]);
            // The invented token must reach GitHub credentials only, including inside Git's rebase subprocess.
            std::fs::write(root.join(".git/hooks/pre-rebase"), r#"#!/bin/sh
board=$(printf 'protocol=https\nhost=github.com\n\n' | git credential fill) || exit 1
case "$board" in *password=test*) ;; *) exit 1;; esac
other=$(printf 'protocol=https\nhost=elsewhere.invalid\n\n' | git credential fill 2>/dev/null)
case "$other" in *password=test*) exit 1;; esac
exit 0
"#).unwrap();
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(root.join(".git/hooks/pre-rebase"), std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let sha = git(&wt, &["rev-parse", "HEAD"]);
            git(&wt, &["push", "origin", "taskq-1"]);
            std::fs::write(root.join(if mode == "conflict" { "base" } else { "main" }), "main\n").unwrap();
            git(&root, &["add", if mode == "conflict" { "base" } else { "main" }]); git(&root, &["commit", "-m", "main"]);
            git(&root, &["push", "origin", "main"]);
            // Invented board URL, rewritten to this test's local bare remote; no network or credentials.
            git(&root, &["config", "remote.origin.url", "https://github.com/o/r.git"]);
            git(&root, &["config", &format!("url.{}.insteadOf", remote.to_str().unwrap().replace('\\', "/")), "https://github.com/o/r.git"]);
            if mode == "dirty" { std::fs::write(wt.join("untracked"), "keep me").unwrap(); }
            let original = sha.clone();
            let bare = remote.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                    else if url.contains("/pulls?") { json!([{"number": 7, "head": {"sha": original}}]) }
                    else if url.ends_with("/rules/branches/main") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) }
                    else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
                    else if url.ends_with("/repos/o/r") { json!({"private": true}) }
                    else if url.ends_with("/git/ref/heads/taskq-1") {
                        let out = std::process::Command::new("git").arg("--git-dir").arg(&bare).args(["rev-parse", "refs/heads/taskq-1"]).output().unwrap();
                        json!({"object": {"sha": String::from_utf8(out.stdout).unwrap().trim()}})
                    } else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let out = h.resume_publication(1, &sha, "pr", "title", "Task: invented", &root, false);
            match mode {
                "clean" => {
                    let Publish::Rebased(head) = out else { panic!("{out:?}") };
                    assert_ne!(head, sha);
                    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), head);
                    assert_eq!(git(&root, &["ls-remote", "origin", "refs/heads/taskq-1"]).split_whitespace().next(), Some(head.as_str()));
                    git(&wt, &["merge-base", "--is-ancestor", "origin/main", "HEAD"]);
                }
                "conflict" => {
                    assert!(matches!(out, Publish::MainMoved(_)), "{out:?}");
                    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), sha);
                    assert!(git(&wt, &["status", "--porcelain"]).is_empty());
                }
                _ => {
                    assert!(matches!(out, Publish::Pending(_)), "{out:?}");
                    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), sha);
                    assert_eq!(std::fs::read_to_string(wt.join("untracked")).unwrap(), "keep me");
                }
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    // Chain: section 7 queue cancellation -> dequeue readback; unknown/merged state never claims cancellation.
    #[test]
    fn github_queue_cancellation_reads_back_before_releasing_the_task() {
        for scenario in ["cancel", "absent", "merged", "unknown", "lost-readback"] {
            let removed = std::rc::Rc::new(RefCell::new(false));
            let done = removed.clone();
            let pulls = recorded("pulls-open");
            let head = pulls[0]["head"]["sha"].as_str().unwrap().to_string();
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                let value = if url.contains("/pulls?") { pulls.clone() }
                    else if url.ends_with("/graphql") {
                        if body.as_ref().unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest") {
                            // #1472: GitHub's DequeuePullRequestInput names the PR `id`; `pullRequestId` fails missingRequiredInputObjectAttribute.
                            assert!(body.as_ref().unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest(input: {id: $id})"), "{body:?}");
                            assert_eq!(body.as_ref().unwrap()["variables"]["id"], pulls[0]["node_id"]);
                            *done.borrow_mut() = true;
                            recorded("dequeue-done")
                        } else if scenario == "unknown" { recorded("node-not-found") }
                        else { recorded(match scenario { "merged" => "cancel-merged", "absent" => "cancel-open", _ if *done.borrow() && scenario != "lost-readback" => "cancel-open", _ => "cancel-queued" }) }
                    } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let result = h.cancel_publication(1, &head);
            assert_eq!(result.is_ok(), matches!(scenario, "cancel" | "absent"), "{scenario}: {result:?}");
            assert_eq!(*removed.borrow(), matches!(scenario, "cancel" | "lost-readback"));
        }
    }

    // #2347 defect: a proof task's result had no PR at all, so `later` was refused and the blocked task could never leave.
    // Retained: a merged task PR still refuses, a failed read still refuses.
    #[test]
    fn cancelling_a_publication_with_no_task_pr_has_nothing_to_dequeue() {
        for (closed, ok) in [(json!([]), true), (json!([{"merged_at": null}]), true), (json!([{"merged_at": "2026-11-04T00:00:00Z"}]), false), (json!({"message": "boom"}), false)] {
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                let value = if url.contains("state=closed") { closed.clone() } else if url.contains("/pulls?") || url.contains("/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let result = h.cancel_publication(1, &"a".repeat(40));
            assert_eq!(result.is_ok(), ok, "{result:?}");
        }
    }

    // Chain: section 7 close without merge -> dequeue, one comment, unmerged close; merged/unknown PRs stay untouched.
    #[test]
    fn github_task_close_closes_its_open_pr_once_and_keeps_the_branch() {
        // scenario -> (result ok, PRs closed, comments posted, dequeued)
        for (scenario, ok, closed, posted, dequeued) in [("open", true, 1, 1, false), ("queued", true, 1, 1, true), ("commented", true, 1, 0, false),
            ("closed", true, 0, 0, false), ("merged", false, 0, 0, false), ("unknown", false, 0, 0, false)] {
            let calls = std::rc::Rc::new(RefCell::new(vec![]));
            let seen = calls.clone();
            let queued = std::rc::Rc::new(RefCell::new(scenario == "queued"));
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                seen.borrow_mut().push(format!("{method} {}", url.rsplit_once("/o/r").map_or(url, |x| x.1)));
                let value = if url.contains("/pulls?") {
                    assert!(url.contains("head=o:taskq-1&state=open") && !url.contains("base="), "{url}");
                    if scenario == "closed" { json!([]) } else { recorded("pulls-open") }
                } else if url.ends_with("/graphql") {
                    if body.as_ref().unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest") {
                        *queued.borrow_mut() = false;
                        recorded("dequeue-done")
                    } else if scenario == "unknown" { recorded("node-not-found") }
                    else { recorded(if scenario == "merged" { "cancel-merged" } else if *queued.borrow() { "cancel-queued" } else { "cancel-open" }) }
                } else if url.contains("/issues/1590/comments") && method == "GET" {
                    if scenario == "commented" { json!([{"id": 1, "body": "Task #1 closed without merging this PR: earlier"}]) } else { json!([]) }
                } else if url.contains("/issues/1590/comments") {
                    assert!(body.as_ref().unwrap()["body"].as_str().unwrap().starts_with("Task #1 closed without merging this PR: superseded\nBranch `taskq-1` kept. Task: https://github.com/o/r/issues/1"));
                    json!({"id": 2})
                } else if url.ends_with("/pulls/1590") && method == "PATCH" {
                    assert_eq!(body.as_ref().unwrap(), &json!({"state": "closed"}));
                    json!({"state": "closed", "merged": false})
                } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let result = h.close_task_prs(1, "superseded");
            let calls = calls.borrow();
            let count = |p: &str| calls.iter().filter(|c| c.starts_with(p)).count();
            assert_eq!(result.is_ok(), ok, "{scenario}: {result:?}");
            let graphql = count("POST") - count("POST /issues");
            assert_eq!((count("PATCH /pulls/1590"), count("POST /issues/1590/comments"), graphql), (closed, posted, match (scenario, dequeued) { ("closed", _) => 0, (_, true) => 3, _ => 1 }), "{scenario}: {calls:?}");
            assert!(!calls.iter().any(|c| c.starts_with("DELETE")), "{scenario}: branch never deleted");
        }
    }

    // Chain: section 7 unchanged GitLab stale-main/merge fence; GitHub has queue coverage above.
    #[test]
    fn gitlab_pr_merges_only_onto_the_main_its_ci_built() {
        // (rebased, how main refuses stale merges, main moves before the merge) -> outcome
        let cases = [
            (false, "ruleset:never", false, "Failed"),
            (true, "none", false, "Pending"),
            (true, "ruleset:always", false, "Pending"),
            (true, "ruleset:pull_requests_only", false, "Pending"),
            (true, "classic:admins exempt", false, "Pending"),
            (true, "ruleset:never", true, "Failed"),
            (true, "classic:enforced", true, "Failed"),
        ];
        for (rebased, guard, moved, want) in cases {
            for kind in [&gitlab::KIND] {
                let calls = std::rc::Rc::new(RefCell::new(vec![]));
                let captured = calls.clone();
                let enforced = matches!(guard, "ruleset:never" | "classic:enforced");
                let h = http(kind, None, Box::new(move |method, url, _| {
                    captured.borrow_mut().push(method.to_string());
                    let result = "a".repeat(40);
                    let value = if url.contains("/compare/") { json!({"status": if rebased { "ahead" } else { "diverged" }}) }
                        else if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                        else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": if rebased { "c" } else { "d" }.repeat(40)}}) }
                        else if url.ends_with("/rules/branches/main") {
                            if guard.starts_with("ruleset:") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) } else { json!([]) }
                        }
                        else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": guard.trim_start_matches("ruleset:")}) }
                        else if url.ends_with("/branches/main/protection") {
                            if !guard.starts_with("classic:") { return Err(Error::http(403, "HTTP 403 Resource not accessible by personal access token")) }
                            json!({"required_status_checks": {"strict": true}, "enforce_admins": {"enabled": guard == "classic:enforced"}})
                        }
                        else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": if enforced { "ff" } else { "merge" }, "autoclose_referenced_issues": false}) }
                        else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                        else if url.ends_with("/status") { json!({"total_count": 0}) }
                        else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                        else if url.contains("/pulls?") { json!([{"number": 7, "head": {"sha": result}}]) }
                        else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result}]) }
                        else if url.ends_with("/merge_requests/7") { json!({"detailed_merge_status": "mergeable"}) }
                        else if url.ends_with("/merge") && moved { return Err(Error::http(405, "HTTP 405 Head branch is out of date")) }
                        else { panic!("unexpected {method} {url}") };
                    Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
                }));
                let got = match h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake") {
                    Publish::Failed(m) => { assert!(m.contains(if moved { "HTTP 405" } else { "not rebased on current main" }), "{m}"); "Failed" }
                    Publish::Pending(_) => "Pending",
                    Publish::Done(m) => panic!("merged: {m}"),
                    other => panic!("unexpected {other:?}"),
                };
                assert_eq!(got, want, "{} rebased={rebased} guard={guard} moved={moved}", kind.name);
                assert_eq!(calls.borrow().iter().filter(|m| *m != "GET").count(), usize::from(moved), "merge only behind the fence");
            }
        }
    }

    // Chain: § 7 GitLab `pr` close vs asynchronous mergeability (#1687). Risk: a merge sent while GitLab rechecks the MR
    // gets 422 'Branch cannot be merged' and requeues a reviewed result; a real blocker merges or loses its reason.
    #[test]
    fn gitlab_merge_waits_for_mergeability() {
        // (merge statuses read in order, PUT merge refusal) -> outcome, merge writes
        let cases: [(&[&str], Option<&str>, &str, usize); 9] = [
            (&["unchecked", "checking", "mergeable"], None, "Done", 1),
            (&["checking"], None, "Pending", 0),
            (&["conflict"], None, "Failed:conflict", 0),
            (&["discussions_not_resolved"], None, "Failed:discussions_not_resolved", 0),
            (&["ci_must_pass"], None, "Failed:ci_must_pass", 0),
            (&["mergeable", "checking"], Some("HTTP 422 Branch cannot be merged"), "Pending", 1),
            (&["mergeable", "conflict"], Some("HTTP 422 Branch cannot be merged"), "Failed:conflict", 1),
            // #1939: the MR pipeline runs, so the service merges it later
            (&["ci_still_running"], None, "Waiting", 0),
            (&["mergeable", "ci_still_running"], Some("HTTP 422 Branch cannot be merged"), "Waiting", 1),
        ];
        for (statuses, refusal, want, writes) in cases {
            let statuses: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
            let refusal = refusal.map(String::from);
            let reads = std::rc::Rc::new(RefCell::new(0usize));
            let seen = reads.clone();
            let calls = std::rc::Rc::new(RefCell::new(0usize));
            let captured = calls.clone();
            let merged = std::rc::Rc::new(RefCell::new(false));
            let h = http(&gitlab::KIND, None, Box::new(move |method, url, _| {
                let result = "a".repeat(40);
                let value = if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                    else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": "c".repeat(40)}}) }
                    else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": "ff", "autoclose_referenced_issues": false}) }
                    else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                    else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result}]) }
                    else if url.ends_with("/merge") {
                        *captured.borrow_mut() += 1;
                        if let Some(e) = &refusal { return Err(Error::http(422, e.clone())) }
                        *merged.borrow_mut() = true;
                        json!({"squash_commit_sha": "b".repeat(40)})
                    }
                    else if url.ends_with("/merge_requests/7") {
                        if *merged.borrow() { json!({"state": "merged"}) } else {
                            let i = *seen.borrow();
                            *seen.borrow_mut() += 1;
                            json!({"state": "opened", "detailed_merge_status": statuses[i.min(statuses.len() - 1)]})
                        }
                    }
                    else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let got = match h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake") {
                Publish::Done(_) => "Done".to_string(),
                Publish::Pending(_) => "Pending".to_string(),
                Publish::Waiting(_) => "Waiting".to_string(),
                Publish::Failed(m) => format!("Failed:{}", m.split(": ").nth(1).unwrap_or("")),
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(got, want, "{want}");
            assert_eq!(*calls.borrow(), writes, "{want}: merge writes");
            if want == "Pending" && writes == 0 { assert_eq!(*reads.borrow(), gitlab::MERGE_STATUS_READS as usize, "bounded wait"); }
        }
    }

    // Chain: § 7 GitLab close vs a pipeline that has not ended (#1939). Defect (cyprusbz #540, MR !1196): a running pipeline
    // refused close like missing CI, so the reviewer ended as `gone`. Risk: a running pipeline is not a wait, a red one hides
    // its link, a stuck one polls forever, or a missing board clock trips the deadline.
    #[test]
    fn gitlab_running_pipeline_waits_until_it_ends_or_its_deadline() {
        let link = "https://gitlab.example/o/r/-/pipelines/9";
        // (commit pipeline status, its start, board Date header) -> outcome
        let cases = [
            ("running", "2026-10-30T11:00:00Z", Some("Fri, 30 Oct 2026 12:00:00 GMT"), "Waiting"),
            ("pending", "2026-10-30T11:00:00Z", Some("Fri, 30 Oct 2026 12:00:00 GMT"), "Waiting"),
            ("running", "2026-10-29T11:00:00Z", Some("Fri, 30 Oct 2026 12:00:00 GMT"), "Blocked"),
            ("running", "2026-10-29T11:00:00Z", None, "Waiting"),
            ("failed", "2026-10-30T11:00:00Z", None, "Failed"),
            ("canceled", "2026-10-30T11:00:00Z", None, "Failed"),
            ("manual", "2026-10-30T11:00:00Z", None, "Pending"),
        ];
        for (status, created, date, want) in cases {
            let h = http(&gitlab::KIND, None, Box::new(move |method, url, _| {
                let value = if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                    else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": status, "created_at": created, "web_url": link}}) }
                    else { panic!("unexpected {method} {url}") };
                let headers = date.map(|d| vec![("Date".to_string(), d.to_string())]).unwrap_or_default();
                Ok(Reply { status: 200, headers, body: value.to_string(), operation: operation(method) })
            }));
            let got = match h.publish(1, &"a".repeat(40), "direct", "title", "Task: fake") {
                Publish::Waiting(m) => { assert!(m.contains(link) && m.contains(status), "{m}"); "Waiting" }
                Publish::Blocked(m) => { assert!(m.contains(link) && m.contains("after 24 h"), "{m}"); "Blocked" }
                Publish::Failed(m) => { assert!(m.ends_with(&format!("CI {status} on {} {link}", "a".repeat(40))), "{m}"); "Failed" }
                Publish::Pending(m) => { assert!(m.contains(status), "{m}"); "Pending" }
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(got, want, "{status} {created} {date:?}");
        }
    }

    // Chain: § 7 GitLab merge vs auto-close of referenced issues (csgo#428, #1909). Risk: MR text such as 'Do not close #N'
    // matches GitLab's closing pattern, so the merge closes the task before its acceptance passes and its worker stops.
    #[test]
    fn gitlab_merge_never_closes_the_task_by_mr_text() {
        // project auto-close -> outcome, task closed by the merge
        for (autoclose, want) in [(Some(true), "Pending"), (None, "Pending"), (Some(false), "Done")] {
            for mode in ["direct", "pr"] {
                let closed = std::rc::Rc::new(RefCell::new(false));
                let seen = closed.clone();
                let merged = std::rc::Rc::new(RefCell::new(false));
                let h = http(&gitlab::KIND, None, Box::new(move |method, url, _| {
                    let result = "a".repeat(40);
                    let value = if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                        else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": "c".repeat(40)}}) }
                        else if url.ends_with("/projects/o%2Fr") {
                            let mut p = json!({"merge_method": "ff"});
                            if let Some(on) = autoclose { p["autoclose_referenced_issues"] = json!(on) }
                            p
                        }
                        else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                        else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result, "description": "Do not close #1 before the live check."}]) }
                        else if url.ends_with("/merge") {
                            // GitLab closes issues named after a closing keyword unless the project turned auto-close off
                            *merged.borrow_mut() = true;
                            *seen.borrow_mut() = autoclose != Some(false);
                            json!({"squash_commit_sha": "b".repeat(40)})
                        }
                        else if url.ends_with("/merge_requests/7") { json!({"state": if *merged.borrow() { "merged" } else { "opened" }, "detailed_merge_status": "mergeable"}) }
                        else { panic!("unexpected {method} {url}") };
                    Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
                }));
                let got = match h.publish(1, &"a".repeat(40), mode, "title", "Task: fake") {
                    Publish::Done(_) => "Done",
                    Publish::Pending(m) => { assert!(m.contains("autoclose_referenced_issues"), "{m}"); "Pending" }
                    other => panic!("unexpected {other:?}"),
                };
                assert_eq!(got, want, "{mode} autoclose={autoclose:?}");
                assert!(!*closed.borrow(), "{mode} autoclose={autoclose:?}: merge closed the task");
            }
        }
    }

    // Chain: § 7 `pr` publication of a draft PR. Risk: GitHub refuses to merge a draft (HTTP 405), so close never publishes.
    #[test]
    fn private_fallback_readies_a_draft_then_merges_the_checked_head() {
        for undrafted in [true, false] {
            let writes = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = writes.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                let (result, merged) = ("a".repeat(40), "b".repeat(40));
                if method != "GET" { captured.borrow_mut().push(url.rsplit_once(".com").map_or(url, |x| x.1).to_string()); }
                let value = if url.contains("/compare/") { json!({"status": "ahead"}) }
                    else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                    else if url.ends_with("/status") { json!({"total_count": 0}) }
                    else if url.ends_with("/repos/o/r") { json!({"private": true}) }
            else if url.ends_with("/rules/branches/main") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) }
                    else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
                    else if url.contains("/pulls?") { json!([{"number": 7, "node_id": "PR_7", "draft": true, "head": {"sha": result}}]) }
                    else if url == "https://api.github.com/graphql" {
                        assert_eq!(body.unwrap()["variables"]["id"], "PR_7");
                        if undrafted { json!({"data": {"markPullRequestReadyForReview": {"pullRequest": {"isDraft": false}}}}) }
                        else { json!({"data": null, "errors": [{"message": "forbidden"}]}) }
                    }
                    else if url.ends_with("/pulls/7/merge") { assert_eq!(body.unwrap()["sha"], result); json!({"sha": merged}) }
                    else if url.ends_with("/pulls/7") { json!({"merged": true, "merge_commit_sha": merged}) }
                    else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let out = h.resume_publication(1, &"a".repeat(40), "pr", "title", "Task: fake", std::path::Path::new("unused"), false);
            if undrafted {
                assert!(matches!(&out, Publish::Done(m) if m.starts_with("merged #7 as bbbb")));
                assert_eq!(*writes.borrow(), ["/graphql", "/repos/o/r/pulls/7/merge"], "ready before merge");
            } else {
                assert!(matches!(&out, Publish::Pending(m) if m.contains("still a draft")));
                assert_eq!(*writes.borrow(), ["/graphql"], "no merge of a draft");
            }
        }
    }

    // Chain: § 7 publication readback. Risk: one flaky read reports a lost publication.
    #[test]
    fn incident_publication_readback_requires_two_negative_reads() {
        for kind in BOARDS {
            for second in [Some(false), Some(true), None] {
                crate::incident::tests::take_detected();
                let (h, calls) = publication_board(kind, "pr", false, second, Some(true));
                assert!(matches!(h.resume_publication(1, &"a".repeat(40), "pr", "title", "Task: fake", std::path::Path::new("unused"), false), Publish::Pending(_)), "reporting preserves first read outcome");
                assert_eq!(crate::incident::tests::take_detected(), if second == Some(false) { vec!["publication.readback"] } else { vec![] });
                let readback = calls.borrow().iter().skip_while(|(m, _)| m == "GET").filter(|(m, u)| m == "GET" && (u.ends_with("/pulls/7") || u.ends_with("/merge_requests/7"))).count();
                assert_eq!(readback, 2);
                assert_eq!(calls.borrow().iter().filter(|(method, _)| method != "GET").count(), 1);
            }
        }
    }

    // Chain: incident adapter.bad_json. Risk: HTTP failures reported as bad JSON.
    #[test]
    fn incident_bad_json_is_only_a_successful_response() {
        for fail in [false, true] {
            crate::incident::tests::take_detected();
            let mut h = (github::KIND.api)("github.com", "o/r", "fake");
            h.transport = Some(Box::new(move |_, _, _| if fail { Err(Error::http(503, "HTTP 503 malformed response")) }
                else { Ok(Reply { status: 200, headers: vec![], body: "private invalid JSON".into(), operation: "read" }) }));
            assert!(h.call("GET", "/issues", None).and_then(|r| r.json()).is_err());
            assert_eq!(crate::incident::tests::take_detected(), if fail { vec![] } else { vec!["adapter.bad_json"] });
        }
    }

    // Chain: § 1 `Depends on <repo>#N` adapters (#1126). Risk: a wrong repository path or a lost close reason makes a
    // dependency of another repository never or wrongly done.
    #[test]
    fn foreign_dependency_reads_sibling_repositories() {
        for (kind, body, api, web, reason) in [
            (&github::KIND, r#"{"number":36,"state":"closed","state_reason":"not_planned"}"#, "https://api.github.com/repos/o/dashq/issues/36",
                "https://github.com/o/dashq/issues/36", Some("not_planned")),
            (&gitlab::KIND, r#"{"iid":36,"state":"closed"}"#, "https://gitlab.example/api/v4/projects/o%2Fdashq/issues/36",
                "https://gitlab.example/o/dashq/-/issues/36", None),
        ] {
            let urls = std::rc::Rc::new(RefCell::new(vec![]));
            let seen = urls.clone();
            let b = http(kind, Some(true), Box::new(move |_, url, _| {
                seen.borrow_mut().push(url.to_string());
                Ok(Reply { status: 200, headers: vec![], body: body.into(), operation: "read" })
            }));
            let i = b.foreign("dashq", 36).unwrap();
            assert_eq!((i.number, i.open, i.reason.as_deref()), (36, false, reason));
            assert_eq!((urls.borrow()[0].as_str(), b.foreign_url("dashq", 36)), (api, web.to_string()));
            assert_eq!(b.foreign_url("x/y", 1).replace("x/y", "o/r"), b.url(1), "a full path is used as written");
        }
        // R12: a reply that is not that issue in a known state stays unknown, never done
        for (kind, body) in [(&github::KIND, "{}"), (&github::KIND, r#"{"number":37,"state":"closed"}"#), (&github::KIND, r#"{"number":36,"state":"gone"}"#),
            (&gitlab::KIND, "{}"), (&gitlab::KIND, r#"{"iid":36}"#), (&gitlab::KIND, r#"{"iid":36,"state":"open"}"#)] {
            let b = http(kind, Some(true), Box::new(move |_, _, _| Ok(Reply { status: 200, headers: vec![], body: body.into(), operation: "read" })));
            assert_eq!(b.foreign("dashq", 36).unwrap_err().text(), "malformed reply for issue 36", "{body}");
        }
    }

    // Chain: A3 forge response -> profile report. Risk: HTTP/GraphQL failures count as successes or pauses send again.
    #[test]
    fn forge_outcomes_reach_profile_report() {
        crate::profile::tests::take_report();
        for (status, body, limited) in [(200, "{}", false), (204, "", false), (404, "private response", false),
            (503, "private response HTTP 404", false), (200, "{\"errors\":[{\"type\":\"RATE_LIMITED\"}]}", true)] {
            let mut api = (github::KIND.api)("github.com", "o/r", "private token");
            api.transport = Some(Box::new(move |_, _, _| Ok(Reply { status, body: body.into(), operation: "read",
                headers: if limited { vec![("retry-after".into(), "60".into())] } else { vec![] } })));
            assert_eq!(api.call("POST", "https://api.github.com/graphql", None).is_ok(), status < 300 && !limited);
            if limited {
                assert!(api.call("POST", "https://api.github.com/graphql", None).err().unwrap().text().contains("paused until"));
                assert_eq!(api.credentials.borrow().budget.borrow().total(), 1);
            }
        }
        let report = crate::profile::tests::take_report();
        assert!(report.contains("| forge.call | 5 |"), "{report}");
        assert!(report.contains("| 2/3 | 60.00 |"), "{report}");
        assert!(report.contains("http4xx:2, http5xx:1"), "{report}");
        assert!(!report.contains("private"));
    }

    // Chain: § 4 board budget, #1356. Risk: a GraphQL RATE_LIMIT reply without budget headers starts no pause, so every
    // pass of every project sends again until the reset.
    #[test]
    fn graphql_rate_limit_reply_pauses_board_requests() {
        // the exact reply body of 2026-10-20 08:40Z
        const BODY: &str = r#"{"errors":[{"type":"RATE_LIMIT","code":"graphql_rate_limit","message":"API rate limit already exceeded for user ID 1640869."}]}"#;
        for status in [200, 403] {
            let sent = std::rc::Rc::new(RefCell::new(0));
            let count = sent.clone();
            let mut api = (github::KIND.api)("github.com", "o/r", "private token");
            api.transport = Some(Box::new(move |_, _, _| {
                *count.borrow_mut() += 1;
                Ok(Reply { status, body: BODY.into(), operation: "read", headers: vec![] })
            }));
            let first = api.call("POST", "https://api.github.com/graphql", None).err().unwrap();
            assert!(first.text().contains("HTTP 403 rate limit: board requests paused until"), "{status}: {first}");
            assert!(api.call("POST", "https://api.github.com/graphql", None).err().unwrap().text().contains("paused until"));
            assert!(api.credentials.borrow().budget.borrow().paused_until().is_some());
            assert_eq!(*sent.borrow(), 1, "{status}: no GraphQL request is sent while paused");
        }
    }

    // Chain: § 4 board budget. Risk: a rate-limit reply pauses only the process that got it; PM, worker and DashQ
    // CLI processes of the same install keep sending until the reset and stretch the board's block.
    #[test]
    fn rate_limit_pause_stops_every_process_of_the_install() {
        let install = std::env::temp_dir().join(format!("taskq-pause-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&install).unwrap();
        let sent = std::rc::Rc::new(RefCell::new(0));
        let process = |status: u16| {
            let count = sent.clone();
            let mut api = (github::KIND.api)("github.com", "o/r", "private token");
            api.credentials.get_mut().pause = Some(crate::requests::pause_file(&install, &api.root, "private token"));
            api.transport = Some(Box::new(move |_, _, _| {
                *count.borrow_mut() += 1;
                Ok(Reply { status, body: "{}".into(), operation: "read", headers: vec![("retry-after".into(), "60".into())] })
            }));
            api
        };
        assert!(process(429).call("GET", "/issues", None).err().unwrap().text().contains("paused until"));
        let other = process(200);
        assert!(other.call("GET", "/issues", None).err().unwrap().text().contains("paused until"));
        assert!(other.credentials.borrow().budget.borrow().paused_until().is_some(), "service status shows the install's pause");
        assert_eq!(*sent.borrow(), 1, "no process of the install sends while paused");
        let mut elsewhere = process(200);
        elsewhere.credentials.get_mut().pause = Some(crate::requests::pause_file(&install, &elsewhere.root, "other token"));
        assert!(elsewhere.call("GET", "/issues", None).is_ok(), "another credential keeps its own budget");
        std::fs::write(crate::requests::pause_file(&install, "https://api.github.com", "private token"), "1").unwrap();
        assert!(process(200).call("GET", "/issues", None).is_ok(), "a past reset holds nothing");
        let _ = std::fs::remove_dir_all(&install);
    }

    // Chain: board list/readback -> malformed successful HTTP page -> explicit failure, never an empty queue/catalog.
    #[test]
    fn malformed_list_is_not_an_empty_board() {
        for kind in [&github::KIND, &gitlab::KIND] {
            for body in ["{}", "null", "\"private-error\""] {
                let board = http(kind, None, Box::new(move |_, _, _| Ok(Reply {
                    status: 200, headers: vec![], body: body.into(), operation: "read",
                })));
                let expected = Some("invalid board list: expected array");
                assert_eq!(board.issues(Some(0)).err().as_ref().map(Error::text), expected, "{} issue list: {body}", kind.name);
                assert_eq!(board.comments(1).err().as_ref().map(Error::text), expected, "{} comments: {body}", kind.name);
                assert_eq!(board.canonical_labels().err().as_ref().map(Error::text), expected, "{} catalog: {body}", kind.name);
            }
        }
    }

    fn label_board(kind: &'static Kind, fail: bool, mismatch: bool) -> (Box<dyn Board>, std::rc::Rc<RefCell<Vec<Value>>>, std::rc::Rc<RefCell<usize>>) {
        let catalog = std::rc::Rc::new(RefCell::new(vec![json!({"id": 99, "name": "bug", "color": "FF0000", "description": "user owned"})]));
        let writes = std::rc::Rc::new(RefCell::new(0));
        let (data, count) = (catalog.clone(), writes.clone());
        let h = http(kind, None, Box::new(move |method, url, body| {
            assert!(url.contains("/labels"));
            if method == "GET" { return Ok(Reply { status: 200, headers: vec![], operation: "read", body: json!(*data.borrow()).to_string() }); }
            assert!(matches!(method, "POST" | "PATCH" | "PUT"));
            if method == "POST" { assert!(url.ends_with("/labels")); }
            else if kind.name == "gitlab" { assert_eq!(method, "PUT"); assert!(url.rsplit('/').next().unwrap().parse::<u64>().is_ok()); }
            else { assert_eq!(method, "PATCH"); }
            assert_eq!(body.as_ref().unwrap()["color"].as_str().unwrap().starts_with('#'), kind.name == "gitlab");
            *count.borrow_mut() += 1;
            if fail && *count.borrow() == 2 { return Err(Error::http(403, "HTTP 403 denied")); }
            let mut v = body.unwrap();
            if mismatch { v["color"] = json!("000000"); }
            let mut all = data.borrow_mut();
            if let Some(old) = all.iter_mut().find(|old| old["name"] == v["name"]) {
                v["id"] = old["id"].clone(); *old = v;
            } else { v["id"] = json!(all.len() + 100); all.push(v); }
            Ok(Reply { status: 200, headers: vec![], operation: "read", body: "{}".into() })
        }));
        (h, catalog, writes)
    }

    // Chain: § 6 label setup on both boards. Risk: repeat writes or user label metadata overwritten.
    #[test]
    fn label_adapters_readback_idempotence_and_preservation() {
        for kind in BOARDS {
            let (h, catalog, writes) = label_board(kind, false, false);
            let mut names = crate::labels::FIXED.iter().chain(["host-win", "needs-gpu", "area-ui"].iter()).map(|s| s.to_string()).collect::<Vec<_>>();
            names.push("taskq".into());
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), names.len() - 1);
            let reads = h.requests().total();
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), names.len() - 1, "repeat makes no writes");
            assert_eq!(h.requests().total(), reads, "repeat sends no catalog read");
            assert_eq!(catalog.borrow()[0]["description"], "user owned");
            let i = catalog.borrow().iter().position(|v| v["name"] == "q-ask").unwrap();
            catalog.borrow_mut()[i]["color"] = json!("000000");
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), names.len() - 1, "verified once per process (§ 4 budget)");
            let (fresh, fresh_catalog, fresh_writes) = label_board(kind, false, false);
            *fresh_catalog.borrow_mut() = catalog.borrow().clone();
            fresh.ensure_labels(&names).unwrap();
            assert_eq!(*fresh_writes.borrow(), 1, "a new process repairs existing metadata");
        }
    }

    // Chain: § 9 label cutover gate. Risk: partial label writes or legacy board relabelled early.
    #[test]
    fn label_adapter_failures_and_legacy_gate() {
        for kind in BOARDS {
            let names = vec!["q-doing".into(), "q-ask".into()];
            let (h, _, writes) = label_board(kind, true, false);
            assert!(h.ensure_labels(&names).unwrap_err().text().contains("403"));
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), 3, "safe retry only writes missing label");
            let (h, _, _) = label_board(kind, false, true);
            assert!(h.ensure_labels(&names).unwrap_err().text().contains("readback differs"));
            let (h, catalog, writes) = label_board(kind, false, false);
            catalog.borrow_mut().push(json!({"name":"q-waiting"}));
            assert!(!h.canonical_labels().unwrap());
            assert_eq!(h.state_label("waiting").unwrap(), "q-waiting");
            assert!(h.ensure_labels(&names).unwrap_err().text().contains("all-host gate"));
            assert_eq!(*writes.borrow(), 0);
            let h = http(kind, Some(true), Box::new(|_, url, _| Err(format!("unexpected {url}").into())));
            assert_eq!(h.state_label("waiting").unwrap(), "q-blocked");
            assert!(label_schema(Some("typo")).is_err());
        }
    }

    // Chain: § 4 board budget on both boards. Risk: an exhausted budget keeps sending the heartbeat every pass, or a
    // multi-line error body splits a log line.
    #[test]
    fn exhausted_budget_sends_nothing_until_reset() {
        for kind in BOARDS {
            let sent = std::rc::Rc::new(RefCell::new(0));
            let count = sent.clone();
            let reset = iso("2026-10-14T04:41:00Z").unwrap().to_string();
            let h = http(kind, Some(true), Box::new(move |_, url, _| {
                *count.borrow_mut() += 1;
                let date = ("Date".to_string(), "Wed, 14 Oct 2026 04:27:00 GMT".to_string());
                if url.ends_with("/issues/6") {
                    return Ok(Reply { status: 500, headers: vec![date], body: "<html>\n  <p>down</p>\n</html>".into(), operation: "update" });
                }
                let headers = vec![date, ("X-RateLimit-Remaining".into(), "0".into()), ("X-RateLimit-Reset".into(), reset.clone())];
                Ok(Reply { status: 403, headers, body: "{\"message\": \"API rate limit exceeded\"}".into(), operation: "update" })
            }));
            assert!(h.edit(6, "beat").unwrap_err().text().ends_with("HTTP 500 <html> <p>down</p> </html>"));
            let first = h.edit(5, "beat").unwrap_err();
            assert!(first.text().contains("HTTP 403 rate limit: board requests paused until 2026-10-14T04:41:00Z"), "{first}");
            let again = h.edit(5, "beat").unwrap_err();
            assert!(again.text().ends_with("rate limit: board requests paused until 2026-10-14T04:41:00Z"), "{again}");
            assert_eq!(*sent.borrow(), 2, "nothing is sent while paused");
            let b = h.requests();
            assert_eq!((b.total(), b.remaining, b.paused_until()), (2, Some(0), iso("2026-10-14T04:41:00Z")));
        }
    }

    // Chain: § 4 two project adapters -> shared host/token pause. Risk: another project keeps sending after 429,
    // or a different credential inherits that pause. Fake transport proves it without board writes.
    #[test]
    fn host_project_adapters_share_budget_and_pause_by_credential() {
        std::thread::spawn(|| {
            crate::host::enter(std::path::Path::new("invented-root"), std::path::Path::new("invented-install"));
            let sent = std::rc::Rc::new(RefCell::new(0));
            let make = |token: &str, status: u16| {
                let mut api = (github::KIND.api)("github.com", "invented/project", token);
                let sent = sent.clone();
                api.transport = Some(Box::new(move |_, _, _| {
                    *sent.borrow_mut() += 1;
                    Ok(Reply { status, headers: vec![("retry-after".into(), "60".into())], body: "{}".into(), operation: "read" })
                })); api
            };
            let a = make("invented-host-budget-token", 429);
            let b = make("invented-host-budget-token", 200);
            let different = make("invented-other-budget-token", 200);
            assert!(a.call("GET", "/issues", None).err().unwrap().text().contains("paused"));
            assert!(b.call("GET", "/issues", None).err().unwrap().text().contains("paused"));
            assert_eq!(*sent.borrow(), 1); assert_eq!(b.credentials.borrow().budget.borrow().total(), 1);
            assert!(different.call("GET", "/issues", None).is_ok()); assert_eq!(*sent.borrow(), 2);
        }).join().unwrap();
    }

    // Chain: § 4 board budget, GitHub open list over GraphQL. Risk: the list loses a page, a field or trust, falls back
    // to core REST, keeps sending after a GraphQL RATE_LIMITED reply, or relists an unchanged board (#1819).
    #[test]
    fn github_open_list_is_one_graphql_request_per_page() {
        let kind = BOARDS.iter().find(|k| k.name == "github").unwrap();
        let limited = std::rc::Rc::new(RefCell::new(false));
        let flag = limited.clone();
        let probes = std::rc::Rc::new(RefCell::new(0));
        let probed = probes.clone();
        // the changed-since probe: a set reply is a 200 with that body, an unset one 304 Not Modified
        let changes = std::rc::Rc::new(RefCell::new(Some("[]".to_string())));
        let changed = changes.clone();
        let h = http(kind, Some(true), Box::new(move |method, url, body| {
            if method == "GET" {
                assert!(url.starts_with("https://api.github.com/repos/o/r/issues?state=all&labels=taskq&sort=updated&direction=desc&per_page=100&since=2026-10-14T04:27"), "{url}");
                *probed.borrow_mut() += 1;
                let reply = changed.borrow_mut().take();
                return Ok(Reply { status: if reply.is_some() { 200 } else { 304 }, headers: vec![("ETag".into(), "\"e1\"".into())],
                    body: reply.unwrap_or_default(), operation: "read" });
            }
            assert_eq!((method, url), ("POST", "https://api.github.com/graphql"));
            let body = body.unwrap();
            // labels are the only nested connection: assignees would double the points (#1043)
            assert!(!body["query"].as_str().unwrap().contains("assignees"));
            assert_eq!((&body["variables"]["owner"], &body["variables"]["name"]), (&json!("o"), &json!("r")));
            let left = if *flag.borrow() { "0" } else { "4990" };
            let headers = vec![("X-RateLimit-Resource".into(), "graphql".into()), ("X-RateLimit-Remaining".into(), left.into()),
                ("X-RateLimit-Limit".into(), "5000".into()), ("X-RateLimit-Reset".into(), "1791964800".into())];
            let value = if *flag.borrow() { json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]}) }
                else if body["variables"]["after"].is_null() { json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": true, "endCursor": "c1"},
                    "nodes": [{"number": 5, "title": "t", "body": "b", "updatedAt": "2026-10-14T04:27:00Z", "createdAt": "2026-10-14T04:20:00Z", "authorAssociation": "OWNER",
                        "labels": {"nodes": [{"name": "taskq"}, {"name": "q-ready"}]}}]}}}}) }
                else { json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": false, "endCursor": null},
                    "nodes": [{"number": 6, "title": "u", "body": null, "updatedAt": "2026-10-14T04:28:00Z", "authorAssociation": "NONE",
                        "labels": {"nodes": []}}]}}}}) };
            Ok(Reply { status: 200, headers, body: value.to_string(), operation: operation(method) })
        }));
        let list = h.issues(None).unwrap();
        assert_eq!(list.iter().map(|i| (i.number, i.open, i.trusted)).collect::<Vec<_>>(), [(5, true, true), (6, true, false)]);
        assert_eq!((list[0].labels.clone(), list[0].assignees.clone(), list[0].updated, list[0].created), (vec!["taskq".to_string(), "q-ready".to_string()], vec![], iso("2026-10-14T04:27:00Z").unwrap(), iso("2026-10-14T04:20:00Z").unwrap()));
        let b = h.requests();
        assert_eq!((b.ops.get("POST graphql"), b.resource.clone(), b.remaining), (Some(&2), Some("graphql".into()), Some(4990)));
        assert_eq!(h.issues(None).unwrap(), list, "an empty probe keeps the last list");
        assert_eq!(h.issues(None).unwrap(), list, "a 304 probe keeps the last list");
        assert_eq!((h.requests().ops.get("POST graphql"), *probes.borrow()), (Some(&2), 2));
        // #2083: the probe asks for `taskq` issues only, so heartbeat writes never reach it; a pull request update keeps the list
        let pull = json!({"number": 8, "title": "PR", "state": "open", "updated_at": "2026-10-14T04:29:30Z", "pull_request": {}, "labels": [{"name": "taskq"}]});
        *changes.borrow_mut() = Some(json!([pull]).to_string());
        assert_eq!(h.issues(None).unwrap(), list, "a pull request update keeps the list");
        assert_eq!(h.issues(None).unwrap(), list, "a 304 keeps the list");
        assert_eq!(h.requests().ops.get("POST graphql"), Some(&2), "only a task change relists");
        // a task change relists
        *changes.borrow_mut() = Some(json!([{"number": 5, "title": "t", "state": "open", "updated_at": "2026-10-14T04:30:00Z", "labels": []}]).to_string());
        h.issues(None).unwrap();
        assert_eq!(h.requests().ops.get("POST graphql"), Some(&4), "a task change relists");
        h.forget();
        assert_eq!(h.issues(None).unwrap(), list);
        assert_eq!(h.requests().ops.get("POST graphql"), Some(&6), "a forgotten list is read in full");
        h.forget();
        *limited.borrow_mut() = true;
        assert!(h.issues(None).unwrap_err().text().contains("HTTP 403 rate limit: board requests paused until"));
        assert!(h.issues(None).unwrap_err().text().contains("paused"));
        assert_eq!(h.requests().total(), 12, "nothing is sent while paused");
    }

    // Chain: #2084 A2 `Board quota:` shows the core budget. Defect: a status that read only GraphQL showed the graphql
    // budget as the board user's quota. Risk: the quota line names graphql, or the free budget read costs a request.
    #[test]
    fn github_quota_reads_core_for_free_after_graphql_only() {
        let kind = BOARDS.iter().find(|k| k.name == "github").unwrap();
        let sent = std::rc::Rc::new(RefCell::new(Vec::<String>::new()));
        let log = sent.clone();
        let mut api = (kind.api)("github.com", "o/r", "test");
        api.transport = Some(Box::new(move |method, url, _| {
            log.borrow_mut().push(format!("{method} {url}"));
            let (resource, left) = if url.ends_with("/graphql") { ("graphql", "4990") } else { ("core", "3000") };
            let headers = vec![("X-RateLimit-Resource".into(), resource.into()), ("X-RateLimit-Remaining".into(), left.into()),
                ("X-RateLimit-Limit".into(), "5000".into()), ("X-RateLimit-Reset".into(), "0".into())];
            Ok(Reply { status: 200, headers, body: "{}".into(), operation: operation(method) })
        }));
        api.call("POST", "https://api.github.com/graphql", Some(json!({}))).unwrap();
        let h = (kind.board)(api);
        assert_eq!(crate::requests::quota(&h.requests()), "Board quota: unknown (no core budget headers)");
        assert_eq!(crate::requests::quota(&h.quota()), "Board quota: 2000/5000 core used, 3000 left until 1970-01-01 00:00 UTC");
        assert_eq!(sent.borrow().last().map(String::as_str), Some("GET https://api.github.com/rate_limit"));
        h.quota();
        assert_eq!(sent.borrow().len(), 2, "a seen core budget needs no second read");
        let ops = h.requests().ops;
        assert_eq!((ops.get("GET rate_limit"), crate::requests::billed(&ops)), (Some(&1), 1), "GitHub never bills the budget read");
    }

    // Chain: § 4 budget (#2083) -> § 3 lease. Defect: every host's heartbeat write changed the reply of the open-issue
    // probe and of the admission and report scans, so an unchanged board billed three `GET issues` a pass on every host.
    // Protection kept: a live host never reads stale from the kept list, a stopped one reads stale at the first pass
    // after its last beat + ttl, and this host's own write patches the kept list instead of writing again.
    #[test]
    fn heartbeats_leave_issue_reads_unbilled_and_stale_leases_show() {
        use std::{cell::Cell, collections::HashMap, rc::Rc};
        const TTL: i64 = 300;
        let start = iso("2026-10-14T04:00:00Z").unwrap();
        let clock = Rc::new(Cell::new(start));
        // (number, title, labels, updated): one ready task and three hosts that beat
        type Row = (u64, &'static str, Vec<&'static str>, i64);
        let state: Rc<RefCell<Vec<Row>>> = Rc::new(RefCell::new(vec![(5, "task", vec!["taskq", "q-ready"], start - 3600),
            (7, "taskq host win", vec!["taskq-host"], start - 30), (8, "taskq host mac", vec!["taskq-host"], start - 30),
            (9, "taskq host fra", vec!["taskq-host"], start - 30)]));
        let (now, issues) = (clock.clone(), state.clone());
        let last = RefCell::new(HashMap::<String, String>::new());
        let h = http(&github::KIND, Some(true), Box::new(move |method, url, _| {
            let t = now.get();
            let rest = |i: &Row| json!({"number": i.0, "title": i.1, "body": "beat", "state": "open", "updated_at": stamp(i.3),
                "created_at": stamp(start - 7200), "author_association": "OWNER", "labels": i.2.iter().map(|l| json!({"name": l})).collect::<Vec<_>>()});
            let path = url.strip_prefix("https://api.github.com/repos/o/r").unwrap_or(url);
            let value = match (method, path) {
                ("POST", _) => json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": false, "endCursor": null},
                    "nodes": issues.borrow().iter().map(|i| json!({"number": i.0, "title": i.1, "body": "beat", "updatedAt": stamp(i.3),
                        "createdAt": stamp(start - 7200), "authorAssociation": "OWNER",
                        "labels": {"nodes": i.2.iter().map(|l| json!({"name": l})).collect::<Vec<_>>()}})).collect::<Vec<_>>()}}}}),
                ("PATCH", p) => {
                    let n: u64 = p.strip_prefix("/issues/").unwrap().parse().unwrap();
                    let mut rows = issues.borrow_mut();
                    let i = rows.iter_mut().find(|i| i.0 == n).unwrap();
                    i.3 = t;
                    rest(i)
                }
                ("GET", p) if p.starts_with("/issues?state=closed") => json!([]),
                ("GET", p) if p.starts_with("/issues?") => {
                    let since = p.split('&').find_map(|q| q.strip_prefix("since=")).and_then(iso).unwrap_or(0);
                    let mut rows: Vec<Row> = issues.borrow().iter().filter(|i| i.3 >= since && (!p.contains("labels=taskq") || i.2.contains(&"taskq"))).cloned().collect();
                    rows.sort_by_key(|i| -i.3);
                    json!(rows.iter().map(rest).collect::<Vec<_>>())
                }
                ("GET", "/issues/5/events?per_page=100") => json!([{"id": 1, "event": "labeled", "label": {"name": "taskq"}, "created_at": stamp(start - 3600)}]),
                ("GET", _) => json!([]),
                _ => panic!("{method} {url}"),
            };
            let body = value.to_string();
            // GitHub answers a repeated GET whose reply did not change with an unbilled 304 (ETag: see the test below)
            let same = method == "GET" && last.borrow_mut().insert(url.to_string(), body.clone()).as_deref() == Some(body.as_str());
            let s = stamp(t);
            let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][s[5..7].parse::<usize>().unwrap() - 1];
            let date = format!("Wed, {} {month} {} {} GMT", &s[8..10], &s[0..4], &s[11..19]);
            Ok(Reply { status: if same { 304 } else { 200 }, headers: vec![("ETag".into(), format!("\"{}\"", body.len())), ("Date".into(), date)],
                body: if same { String::new() } else { body }, operation: operation(method) })
        }));
        let (mut cache, mut deps) = (HashMap::new(), HashMap::new());
        let (mut history, mut reports) = (crate::incoming::History::default(), crate::reports::Cache::default());
        let stopped = start + 30 * 60;
        // one hour of 60 s passes: mac beats throughout, fra stops at the half hour, this host (win) writes every ttl/3
        for pass in 0..60 {
            let t = start + pass * 60;
            clock.set(t);
            for n in [8, 9].into_iter().filter(|n| *n == 8 || t <= stopped) {
                state.borrow_mut().iter_mut().find(|i| i.0 == n).unwrap().3 = t;
            }
            let (_, hosts) = crate::service::views(h.as_ref(), &mut cache, &mut deps, TTL).unwrap();
            let read = |n| t - hosts.iter().find(|i| i.number == n).unwrap().updated;
            assert!(read(8) <= TTL && read(7) <= TTL, "a live host never reads stale (pass {pass})");
            assert_eq!(read(9) > TTL, t > stopped + TTL, "a stopped host reads stale at the first pass after its beat + ttl (pass {pass})");
            if read(7) >= TTL / 3 { h.edit(7, "beat").unwrap(); }
            history.refresh(h.as_ref(), t);
            reports.collect(h.as_ref());
        }
        let ops = h.requests().ops;
        let count = |op: &str| ops.get(op).copied().unwrap_or(0);
        assert!(count("GET issues 304") > count("GET issues"), "{ops:?}");
        assert!(count("PATCH issues/N") <= 30, "the patched list keeps this host's write interval: {ops:?}");
        assert!(count("POST graphql") <= 14, "a kept heartbeat relists about once per ttl: {ops:?}");
    }

    // Chain: § 4 budget (#1819). Risk: a repeated GET is counted by GitHub, or a 304 loses the body.
    // #1985 A1: an unbilled 304 counts apart, so it does not inflate the host's billed rate.
    #[test]
    fn repeated_get_reuses_its_body_on_304() {
        let mut api = (BOARDS[0].api)("github.com", "o/r", "test");
        let calls = std::rc::Rc::new(RefCell::new(0));
        let seen = calls.clone();
        api.transport = Some(Box::new(move |_, _, _| {
            *seen.borrow_mut() += 1;
            let n = *seen.borrow();
            Ok(match n {
                1 => Reply { status: 200, headers: vec![("ETag".into(), "W/\"a\"".into())], body: "[1]".into(), operation: "read" },
                2 => Reply { status: 304, headers: vec![], body: String::new(), operation: "read" },
                _ => Reply { status: 200, headers: vec![], body: "[2]".into(), operation: "read" },
            })
        }));
        assert_eq!((api.call("GET", "/x", None).unwrap().body, api.unchanged.get()), ("[1]".to_string(), false));
        assert_eq!((api.call("GET", "/x", None).unwrap().body, api.unchanged.get()), ("[1]".to_string(), true));
        assert_eq!((api.call("GET", "/x", None).unwrap().body, api.unchanged.get()), ("[2]".to_string(), false));
        assert!(api.etags.borrow().is_empty(), "a reply without ETag drops the cached one");
        let ops = api.credentials.borrow().budget.borrow().ops.clone();
        assert_eq!(ops.get("GET x 304"), Some(&1), "{ops:?}");
        assert_eq!((ops.get("GET x"), crate::requests::billed(&ops)), (Some(&2), 2), "{ops:?}");
    }

    // Chain: #1985 four hosts share one board user. Risk: every CLI process or restarted service rereads unchanged issues,
    // comments and events in full, or a 304 counts against the budget GitHub never billed.
    #[test]
    fn issue_reads_carry_across_processes_and_304_is_unbilled() {
        let dir = std::env::temp_dir().join(format!("taskq-board-cache-{}", crate::runtime::uuid()));
        let open = |replies: Vec<(&'static str, Reply)>| {
            let mut api = (BOARDS[0].api)("github.com", "o/r", "test");
            api.credentials.get_mut().store = Some(dir.clone());
            let replies = RefCell::new(replies);
            api.transport = Some(Box::new(move |_, url, _| {
                let (path, reply) = replies.borrow_mut().remove(0);
                assert!(url.ends_with(path), "{url} for {path}");
                Ok(reply)
            }));
            api
        };
        let tagged = |status, body: &str| Reply { status, headers: vec![("ETag".into(), "W/\"e5\"".into())], body: body.into(), operation: "read" };
        let first = open(vec![("/issues/5/comments", tagged(200, "[1]")), ("/pulls/5", tagged(200, "{}"))]);
        assert_eq!(first.call("GET", "/issues/5/comments", None).unwrap().body, "[1]");
        first.call("GET", "/pulls/5", None).unwrap();
        // a second process: the stored ETag turns the unchanged comments into a free 304 with the stored body;
        // reads outside the shared set stay in their own process
        let second = open(vec![("/issues/5/comments", tagged(304, "")), ("/pulls/5", tagged(200, "{}"))]);
        assert_eq!((second.call("GET", "/issues/5/comments", None).unwrap().body, second.unchanged.get()), ("[1]".to_string(), true));
        second.call("GET", "/pulls/5", None).unwrap();
        assert!(!second.unchanged.get(), "only issue, comment and event reads are shared");
        let ops = second.credentials.borrow().budget.borrow().ops.clone();
        assert_eq!((ops.get("GET issues/N/comments 304"), crate::requests::billed(&ops)), (Some(&1), 1), "GitHub does not bill a 304: {ops:?}");
        // a changed issue replaces the stored reply for the next process
        let third = open(vec![("/issues/5/comments", Reply { status: 200, headers: vec![("ETag".into(), "W/\"e6\"".into())], body: "[1,2]".into(), operation: "read" })]);
        assert_eq!(third.call("GET", "/issues/5/comments", None).unwrap().body, "[1,2]");
        assert_eq!(store::get(&dir, &format!("{}/issues/5/comments", third.base)).map(|(e, r)| (e, r.body)), Some(("W/\"e6\"".to_string(), "[1,2]".to_string())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: #1903 claim readback on an issue past 100 notes. Risk: a 304 on page 1 replays the Link cached while the
    // issue had one page, so `all` never reads page 2 and a new spawn note is never seen.
    #[test]
    fn unchanged_full_page_rereads_for_a_new_next_page() {
        use std::cell::Cell;
        let mut api = (BOARDS[0].api)("github.com", "o/r", "test");
        // (notes on the issue, page-1 replies still answered 304, requests sent)
        let state = std::rc::Rc::new((Cell::new(2u64), Cell::new(0u32), Cell::new(0u32)));
        let st = state.clone();
        api.transport = Some(Box::new(move |_, url, _| {
            st.2.set(st.2.get() + 1);
            let n = st.0.get();
            if url.contains("&page=2") {
                return Ok(Reply { status: 200, headers: vec![], body: json!((3..=n).collect::<Vec<_>>()).to_string(), operation: "read" });
            }
            if st.1.get() > 0 {
                st.1.set(st.1.get() - 1);
                return Ok(Reply { status: 304, headers: vec![], body: String::new(), operation: "read" });
            }
            let mut headers = vec![("ETag".to_string(), "W/\"p1\"".to_string())];
            if n > 2 { headers.push(("Link".into(), "<https://api.github.com/repos/o/r/x?per_page=2&page=2>; rel=\"next\"".into())); }
            Ok(Reply { status: 200, headers, body: "[1,2]".into(), operation: "read" })
        }));
        assert_eq!(api.all("/x?per_page=2").unwrap().len(), 2);
        // a third note lands on page 2; page 1's body and ETag stay the same, so the server answers 304
        state.0.set(3);
        state.1.set(1);
        assert_eq!(api.all("/x?per_page=2").unwrap(), vec![json!(1), json!(2), json!(3)], "the new note on page 2 is read");
        // the reread cached page 1 with its next Link: a later 304 reaches page 2 without another reread
        state.1.set(1);
        state.2.set(0);
        assert_eq!(api.all("/x?per_page=2").unwrap().len(), 3);
        assert_eq!(state.2.get(), 2, "page 1 (304) and page 2 only");
    }

    // Chain: § 2 state labels on both boards. Risk: a state change removes user labels.
    #[test]
    fn issue_label_adapters_preserve_unrelated_assignments() {
        for kind in BOARDS {
            let labels = std::rc::Rc::new(RefCell::new(vec!["bug".to_string(), "q-ready".to_string()]));
            let data = labels.clone();
            let h = http(kind, Some(false), Box::new(move |method, url, body| {
                let reply = |body: Value| Ok(Reply { status: 200, headers: vec![], operation: "read", body: body.to_string() });
                if url.contains("/members/") { return reply(json!({"access_level": 20})); }
                if method == "GET" {
                    assert!(url.ends_with("/issues/7"));
                    return reply(json!({"number": 7, "iid": 7, "state": "open", "author_association": "OWNER", "labels": *data.borrow()}));
                }
                if method == "POST" && url.ends_with("/issues") {
                    let body = body.unwrap();
                    *data.borrow_mut() = if kind.name == "gitlab" { body["labels"].as_str().unwrap().split(',').map(String::from).collect() }
                        else { names(&body["labels"], "name") };
                    return reply(json!({"number": 7, "iid": 7}));
                }
                if kind.name == "gitlab" {
                    assert_eq!(method, "PUT"); assert!(url.ends_with("/issues/7"));
                    let body = body.unwrap();
                    let remove = body["remove_labels"].as_str().unwrap();
                    data.borrow_mut().retain(|l| !remove.split(',').any(|r| r == l));
                    for label in body["add_labels"].as_str().unwrap().split(',').filter(|s| !s.is_empty()) { data.borrow_mut().push(label.into()); }
                } else if method == "POST" {
                    assert!(url.ends_with("/issues/7/labels"));
                    data.borrow_mut().extend(names(&body.unwrap()["labels"], "name"));
                } else {
                    assert_eq!(method, "DELETE"); assert!(url.ends_with("/issues/7/labels/q-ready"));
                    data.borrow_mut().retain(|l| l != "q-ready");
                }
                reply(json!({}))
            }));
            h.labels(7, &["q-doing".into()], &["q-ready".into()]).unwrap();
            assert_eq!(*labels.borrow(), vec!["bug", "q-doing"]);
            h.create("new", "", &["taskq".into(), "q-ready".into()]).unwrap();
            assert_eq!(*labels.borrow(), vec!["taskq", "q-ready"]);
        }
    }

    // Chain: R12 label readback. Risk: a partial label change reported as applied.
    #[test]
    fn assignment_readback_rejects_partial_application() {
        let b = Fake::default();
        let n = b.create("t", "", &["bug".into(), "q-ready".into()]).unwrap();
        assert!(verify_assignment(&b, n, &["q-doing".into()], &["q-ready".into()]).is_err());
        b.labels(n, &["q-doing".into()], &["q-ready".into()]).unwrap();
        verify_assignment(&b, n, &["q-doing".into()], &["q-ready".into()]).unwrap();
        assert!(b.issue(n).unwrap().labels.contains(&"bug".into()));
    }

    // Chain: § 3 lease time from board stamps. Risk: board dates parsed wrong, leases expire wrong.
    #[test]
    fn clocks() {
        assert_eq!(iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso("2026-10-12T10:20:30.123Z"), Some(1791800430));
        assert_eq!(http_date("Mon, 12 Oct 2026 10:20:30 GMT"), Some(1791800430));
        assert_eq!(stamp(1791800430), "2026-10-12T10:20:30Z");
    }

    // Chain: R1 board entries to fold. Risk: untrusted or edited comments move the queue.
    #[test]
    fn untrusted_and_edited_entries_never_reach_fold() {
        let b = Fake::default();
        let n = b.create("t", "", &[]).unwrap();
        b.raw(n, "taskq: spawn worker claude:x evil", false, false);
        b.raw(n, "1", false, false);
        b.raw(n, "taskq: later", true, true);
        let e = entries(&b, n).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(crate::core::fold(0, &e, false).state, crate::core::State::Ready);
    }

    // Chain: § 9 GitLab token lookup (coreq v0.14.8 `<install>/gitlab-token`) -> board read. Risk: a GitLab host with only
    // the install token file finds no token, or the GitHub `token` file goes to GitLab.
    #[test]
    fn gitlab_board_reads_install_token_file() {
        if std::env::var_os("GITLAB_TOKEN").is_some() { return; }
        let install = std::env::temp_dir().join(format!("taskq-gitlab-token-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("token"), "invented-github-token\n").unwrap();
        std::fs::write(install.join("gitlab-token"), "invented-gitlab-token\n").unwrap();
        let tool = coreq::Tool { install: install.clone(), ..coreq::Tool::new("taskqgitlabtokentest", "TaskQ", "fixture/unused", "1.2.3") };
        let token = (gitlab::KIND.token)(&tool, "gitlab.example").unwrap();
        assert_eq!(token, "invented-gitlab-token");
        assert_eq!(gitlab::KIND.worker_env(&tool, "gitlab.example").unwrap(), vec![("GITLAB_TOKEN".into(), token.clone())]);
        let mut api = (gitlab::KIND.api)("gitlab.example", "o/r", &token);
        assert_eq!(api.credentials.borrow().auth, "Authorization: Bearer invented-gitlab-token");
        api.transport = Some(Box::new(|_, url, _| {
            assert!(url.starts_with("https://gitlab.example/api/v4/projects/o%2Fr/"), "{url}");
            let body = if url.ends_with("/issues/7") { r#"{"iid":7,"title":"t","state":"opened"}"# } else { r#"{"access_level":30}"# };
            Ok(Reply { status: 200, headers: vec![], body: body.into(), operation: "read" })
        }));
        assert_eq!((gitlab::KIND.board)(api).issue(7).unwrap().number, 7);
        std::fs::remove_dir_all(install).unwrap();
    }

    // Chain: board token boundary. Risk: the token is sent to another host.
    #[test]
    fn token_goes_only_to_the_board_api() {
        let h = (github::KIND.api)("github.com", "o/r", "t");
        for url in ["https://evil.example/x", "https://api.github.com.evil.example/x"] {
            assert!(h.call("GET", url, None).err().is_some_and(|e| e.text().starts_with("refused")), "{url}");
        }
    }

    // Chain: §5 real curl request/response -> native file transport. Risk: a 20 MiB attachment hits a process-pipe cap.
    #[test]
    fn reply_files_cross_real_http_without_process_buffer_caps() {
        use std::{io::{BufRead,Read,Write},net::TcpListener,time::{Duration,Instant}};
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();
        let root=format!("http://{}",listener.local_addr().unwrap());
        let payload=serde_json::json!({"data":"x".repeat(20*1024*1024)});
        let expected=payload.to_string().into_bytes();let reply=expected.clone();
        let server=std::thread::spawn(move||{
            for request in 0..2 {
                let until=Instant::now()+Duration::from_secs(20);
                let mut peer=loop {match listener.accept(){Ok((s,_))=>break s,Err(e) if e.kind()==std::io::ErrorKind::WouldBlock&&Instant::now()<until=>std::thread::sleep(Duration::from_millis(10)),Err(e)=>panic!("HTTP fixture accept: {e}")}};
                peer.set_nonblocking(false).unwrap();
                peer.set_read_timeout(Some(Duration::from_secs(20))).unwrap();peer.set_write_timeout(Some(Duration::from_secs(20))).unwrap();
                let mut reader=std::io::BufReader::new(peer.try_clone().unwrap());let mut length=0;
                loop{let mut line=String::new();reader.read_line(&mut line).unwrap();if line=="\r\n"||line.is_empty(){break;}if let Some((key,value))=line.split_once(':'){if key.eq_ignore_ascii_case("content-length"){length=value.trim().parse::<usize>().unwrap();}}}
                let mut data=vec![0;length];reader.read_exact(&mut data).unwrap();
                if request==0{assert!(data==expected,"POST changed or truncated the large JSON body");}
                write!(peer,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",reply.len()).unwrap();peer.write_all(&reply).unwrap();
                // Finish the response before dropping both Windows socket handles; keep the peer's close observable.
                peer.shutdown(std::net::Shutdown::Write).unwrap();
                let mut trailing=Vec::new();reader.read_to_end(&mut trailing).unwrap();assert!(trailing.is_empty(),"unexpected bytes after the HTTP request");
            }
        });
        let got=curl("POST",&format!("{root}/upload"),"Authorization: Bearer invented",Some(&payload),None).unwrap();
        assert_eq!(got.status,200);assert!(got.body.as_bytes()==payload.to_string().as_bytes(),"HTTP reply was truncated");
        let mut api=(github::KIND.api)("invented.example","o/r","invented");api.root=root.clone();api.base=root;
        let got=api.raw("/download",None).unwrap();assert!(got==payload.to_string().as_bytes(),"binary download was truncated");
        server.join().unwrap();
    }
