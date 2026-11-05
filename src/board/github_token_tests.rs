    use super::*;

    // #2352/R12: a lagging full list must not undo a confirmed own heartbeat and bill another PATCH each poll.
    // Keep periodic renewal and fresh newer board edits; this fixture substitutes HTTP, not native traffic.
    #[test]
    fn lagging_lists_do_not_repeat_confirmed_heartbeats() {
        use std::{cell::Cell, rc::Rc};
        use crate::board::{Reply, operation};
        let start = iso("2026-11-04T12:00:00Z").unwrap();
        let clock = Rc::new(Cell::new(start));
        let written = Rc::new(Cell::new(start - 30));
        let listing = Rc::new(Cell::new(0));
        let (now, beat, mode) = (clock.clone(), written.clone(), listing.clone());
        let mut api = api("github.com", "o/r", "invented-token");
        api.transport = Some(Box::new(move |method, url, _| {
            let updated = if method == "PATCH" { beat.set(now.get()); beat.get() }
                else if mode.get() == 1 { beat.get() } else if mode.get() == 2 { now.get() }
                else { beat.get().min(now.get() - 180).max(start - 30) };
            let text = if mode.get() == 0 { "beat" } else { "board edit" };
            let row = json!({"number":7,"title":"taskq host win","body":"beat","state":"open",
                "updated_at":stamp(updated),"created_at":stamp(start - 3600),"author_association":"OWNER",
                "labels":[{"name":"taskq-host"}]});
            let body = if method == "PATCH" { row } else if mode.get() == 3 {
                json!({"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false},"nodes":[]}}}})
            } else {
                assert!(url.ends_with("/graphql"), "{method} {url}");
                json!({"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},
                    "nodes":[{"number":7,"title":"taskq host win","body":text,"updatedAt":stamp(updated),
                        "createdAt":stamp(start - 3600),"authorAssociation":"OWNER","labels":{"nodes":[{"name":"taskq-host"}]}}]}}}})
            };
            Ok(Reply {status:200,headers:vec![],body:body.to_string(),operation:operation(method)})
        }));
        let h = GitHub {api,rules:Default::default(),open:Default::default(),heartbeat:Default::default(),evidence:Default::default(),ci:Default::default()};
        let mut writes = 0;
        for pass in 0..60 {
            clock.set(start + pass * 60);
            h.forget(); // a busy board or an expired kept lease forces a full list
            let list = h.issues(None).unwrap();
            if clock.get() - list[0].updated >= 100 { h.edit(7,"beat").unwrap(); writes += 1; }
            assert!(clock.get() - written.get() < 300, "renewal retains the lease");
        }
        assert!(writes <= 30, "confirmed writes must survive list lag: {writes} PATCHes/h");
        eprintln!("lagging full lists: {writes} PATCHes/h; lease renewal retained");
        for mode in [1, 2] {
            listing.set(mode); h.forget();
            assert_eq!(h.issues(None).unwrap()[0].body,"board edit","equal/newer edits win");
        }
        listing.set(3); h.forget();
        assert!(h.issues(None).unwrap().is_empty(),"absent heartbeats are never reinserted");
        listing.set(0);
        h.api.generation.set(h.api.generation.get()+1); h.forget();
        assert!(h.issues(None).unwrap()[0].updated < written.get(),"another credential cannot use the write");
        for close in [true, false] {
            h.edit(7,"beat").unwrap();
            if close { h.close(7).unwrap(); } else { h.reopen(7).unwrap(); }
            assert!(h.issues(None).unwrap()[0].updated < written.get(),"close/reopen invalidates the write");
        }
    }


    // #1840 A2: a known repository-access refusal has a plain cause; other failures retain their diagnostic.
    #[test]
    fn repository_access_failure_is_plain() {
        for (kind, expected) in [("NOT_FOUND", "no access to the repository"), ("FORBIDDEN", "GraphQL open issues:")] {
            let mut api = api("github.com", "o/r", "invented-token");
            api.transport = Some(Box::new(move |_, _, _| Ok(super::super::Reply {status:200, headers:vec![], operation:"read",
                body:json!({"errors":[{"type":kind,"message":"invented refusal"}]}).to_string()})));
            let board = GitHub {api, rules:Default::default(), open:Default::default(), heartbeat:Default::default(),evidence:Default::default(),ci:Default::default()};
            assert!(board.open_issues().unwrap_err().starts_with(expected));
        }
    }

    // #1840 A3 / section 9: one opened board uses the next credential on the wire, with no restart or login fallback.
    #[test]
    fn next_read_uses_replaced_host_token() {
        use std::{io::{Read, Write}, net::TcpListener, time::{Duration, Instant}};
        let dir = std::env::temp_dir().join(format!("taskq-rotate-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("token"), "invented-first-token").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for name in ["first", "second"] {
                let until = Instant::now() + Duration::from_secs(60);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < until, "board read never reached the local fixture");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
                let mut bytes = vec![];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut part = [0; 4096];
                    let n = socket.read(&mut part).unwrap();
                    assert!(n > 0 && bytes.len() + n <= 65536, "invalid fixture request");
                    bytes.extend_from_slice(&part[..n]);
                }
                assert!(String::from_utf8_lossy(&bytes).to_ascii_lowercase().contains(&format!("authorization: bearer invented-{name}-token")));
                let body = json!({"number":1,"title":name,"state":"open","author_association":"OWNER"}).to_string();
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        // Keep the production credential lookup, Kind::open and GitHub adapter; substitute only the HTTP endpoint.
        let kind = super::super::Kind { api: |root, repo, token| Api::new(root.into(),format!("{root}/repos/{repo}"),token), ..KIND };
        let board = kind.open(&root, "o/r", None, &crate::platform::tool(&dir)).unwrap();
        std::fs::write(crate::requests::user_file(&dir, &root, "invented-first-token"), "kept-login").unwrap();
        assert_eq!(board.me().as_deref(), Ok("kept-login"), "the kept login answers without GET /user");
        let first = board.issue(1).map(|issue| issue.title);
        let old_pause = crate::requests::pause_file(&dir, &root, "invented-first-token");
        std::fs::write(&old_pause, (super::super::wall() + 60).to_string()).unwrap();
        std::fs::write(dir.join("replacement"), "invented-second-token").unwrap();
        std::fs::remove_file(dir.join("token")).unwrap();
        std::fs::rename(dir.join("replacement"), dir.join("token")).unwrap();
        let second = board.issue(1).map(|issue| issue.title);
        let joined = server.join();
        std::fs::remove_file(dir.join("token")).unwrap();
        let missing = board.issue(1).err();
        assert!(old_pause.exists(), "rotation preserves the old credential's pause");
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(first.as_deref(), Ok("first"));
        assert_eq!(second.as_deref(), Ok("second"));
        assert!(joined.is_ok(), "replacement credential was not observed on the wire");
        assert_eq!(missing.as_deref(), Some(TOKEN_ERROR));
    }

    // Chain: host credential -> service board and worker environment. Risk: login fallback or mismatched errors.
    #[test]
    fn service_and_workers_share_the_host_credential_and_missing_token_error() {
        let dir = std::env::temp_dir().join(format!("taskq-token-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let tool = crate::platform::tool(&dir);
        for contents in [None, Some(" \n")] {
            if let Some(contents) = contents { std::fs::write(dir.join("token"), contents).unwrap(); }
            let service = KIND.open("github.com", "o/r", None, &tool).err().unwrap();
            assert_eq!(service, TOKEN_ERROR);
            assert_eq!(KIND.worker_env(&tool, "github.com").unwrap_err(), service);
        }
        std::fs::write(dir.join("token"), "existing-owner-token\n").unwrap();
        let env = KIND.worker_env(&tool, "github.com").unwrap();
        assert_eq!(env, vec![("GH_TOKEN".into(), "existing-owner-token".into()), ("GITHUB_TOKEN".into(), "existing-owner-token".into())]);
        assert!(KIND.open("github.com", "o/r", None, &tool).is_ok());
        std::fs::remove_file(dir.join("token")).unwrap();
        std::fs::create_dir(dir.join("token")).unwrap();
        assert_eq!(KIND.worker_env(&tool, "github.com").unwrap_err(), TOKEN_ERROR);
        std::fs::remove_dir_all(dir).unwrap();
    }
