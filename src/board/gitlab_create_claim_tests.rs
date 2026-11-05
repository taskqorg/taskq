    use super::*;
    use super::super::Reply;
    use std::{cell::RefCell, rc::Rc};

    /// A GitLab project whose issue 1 holds create claims; `store` turns a POSTed (title, description, labels) into
    /// what GitLab keeps.
    fn board(store: fn(&str, &str, Vec<String>) -> (String, String, Vec<String>)) -> Box<dyn Board> {
        let issues = Rc::new(RefCell::new(vec![(1u64, "Claims".to_string(), String::new(), vec![])]));
        let notes: Rc<RefCell<Vec<(u64, u64, String)>>> = Rc::default();
        let mut api = api("gitlab.example", "o/r", "test");
        *api.label_schema.borrow_mut() = Some(false);
        api.transport = Some(Box::new(move |method, url, body| {
            let at = "2026-10-30T00:00:00Z";
            let issue = |(n, title, body, labels): &(u64, String, String, Vec<String>)| json!({"iid": n, "title": title,
                "description": body, "labels": labels, "state": "opened", "author": {"id": 1}, "created_at": at, "updated_at": at});
            let path = url.split('?').next().unwrap().trim_start_matches("https://gitlab.example/api/v4");
            let rest = path.strip_prefix("/projects/o%2Fr/issues").map(|r| r.trim_start_matches('/'));
            let v = match (method, rest, rest.and_then(|r| r.split_once('/'))) {
                _ if path == "/user" => json!({"username": "owner"}),
                _ if path.starts_with("/projects/o%2Fr/members/all/") => json!({"access_level": 30}),
                ("GET", Some(""), _) => json!(issues.borrow().iter().map(issue).collect::<Vec<_>>()),
                ("POST", Some(""), _) => {
                    let body = body.unwrap();
                    let labels = body["labels"].as_str().unwrap().split(',').map(String::from).collect();
                    let (title, text, labels) = store(body["title"].as_str().unwrap(), body["description"].as_str().unwrap(), labels);
                    let n = issues.borrow().len() as u64 + 1;
                    issues.borrow_mut().push((n, title, text, labels));
                    json!({"iid": n})
                }
                ("GET", _, Some((n, "notes"))) => json!(notes.borrow().iter().filter(|(i, ..)| i.to_string() == n).map(|(_, id, body)|
                    json!({"id": id, "body": body, "system": false, "author": {"id": 1, "username": "owner"}, "created_at": at, "updated_at": at})).collect::<Vec<_>>()),
                ("POST", _, Some((n, "notes"))) => {
                    let id = 100 + notes.borrow().len() as u64;
                    notes.borrow_mut().push((n.parse().unwrap(), id, body.unwrap()["body"].as_str().unwrap().into()));
                    json!({"id": id})
                }
                ("GET", Some(n), None) => issue(issues.borrow().iter().find(|i| i.0.to_string() == n).unwrap()),
                _ => panic!("unexpected {method} {url}"),
            };
            Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: "read" })
        }));
        (KIND.board)(api)
    }

    // Chain: § 1 `add --op-id` on GitLab -> create readback and retry discovery (#1914). Defect: gradus/cyprusbz#540,
    // GitLab keeps a description without the trailing newline `add` sends, so the first add reported 'create readback
    // does not match the request' and every retry 'created issue conflicts with the immutable request'. Risk: a
    // different title, body or label set is accepted as the requested issue.
    #[test]
    fn gitlab_add_op_id_accepts_its_stripped_description_and_still_refuses_other_content() {
        let raw = json!({"create_claim_issue": 1});
        let op = crate::Op { id: Some("pilotq-1.p18-t1".into()), ..Default::default() };
        let body = "## Goal\nG\n\n## Acceptance\nA1. Check\n\nop-id: pilotq-1.p18-t1\n";
        let labels = vec!["q-held".to_string()];
        // GitLab strips the surrounding whitespace of a stored title and description
        let b = board(|title, text, labels| (title.trim().into(), text.trim().into(), labels));
        let n = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap();
        let r = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap_err();
        assert_eq!((r.outcome, r.task, r.state), ("accepted", n, Some("held")), "{}", r.reason);
        let cases: [(&str, fn(&str, &str, Vec<String>) -> (String, String, Vec<String>)); 3] = [
            ("title", |_, text, labels| ("Other".into(), text.into(), labels)),
            ("body", |title, text, labels| (title.into(), text.replace("A1. Check", "A1. Other"), labels)),
            ("labels", |title, text, labels| (title.into(), text.into(), [labels, vec!["bug".into()]].concat())),
        ];
        for (field, store) in cases {
            let b = board(store);
            let r = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap_err();
            assert_eq!((r.outcome, r.reason.as_str()), ("unknown", "create readback does not match the request"), "{field}");
            // labels move with the task state after start, so retry discovery identifies the issue by title and body
            let r = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap_err();
            let want = if field == "labels" { ("accepted", "") } else { ("unknown", "created issue conflicts with the immutable request") };
            assert_eq!((r.outcome, r.reason.as_str()), want, "{field}");
        }
    }

    /// A GitLab project holding one closed research report #540 at `sha`, its report file and nothing else: no claim
    /// issue and, in the caller's taskq.json, no `create_claim_issue`. Issues and notes are the shared JSON GitLab returns.
    fn report_board(sha: &str, report: &str) -> (Box<dyn Board>, Rc<RefCell<Vec<Value>>>) {
        let at = "2026-10-30T00:00:00Z";
        let issue = move |n: u64, title: &str, labels: &[&str], state: &str| json!({"iid": n, "title": title, "description": "",
            "labels": labels, "state": state, "author": {"id": 1}, "created_at": at, "updated_at": at});
        let issues = Rc::new(RefCell::new(vec![issue(540, "Research report", &["taskq", "research"], "closed")]));
        let notes: Rc<RefCell<Vec<(u64, u64, String)>>> = Rc::new(RefCell::new(vec![
            (540, 1, "taskq: spawn worker invented:session host".into()),
            (540, 2, format!("taskq: result {sha} attempt 1\nReport: docs/research/540.md")),
            (540, 3, "taskq: close".into()),
        ]));
        let file = (format!("/projects/o%2Fr/repository/files/docs%2Fresearch%2F540.md?ref={sha}"),
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, report));
        let list = issues.clone();
        let mut api = api("gitlab.example", "o/r", "test");
        *api.label_schema.borrow_mut() = Some(false);
        api.transport = Some(Box::new(move |method, url, body| {
            let full = url.trim_start_matches("https://gitlab.example/api/v4");
            let path = full.split('?').next().unwrap();
            let rest = path.strip_prefix("/projects/o%2Fr/issues").map(|r| r.trim_start_matches('/'));
            let find = |n: &str| list.borrow().iter().position(|i| i["iid"].to_string() == n).unwrap();
            let v = match (method, rest, rest.and_then(|r| r.split_once('/'))) {
                _ if path == "/user" => json!({"username": "owner"}),
                _ if path.starts_with("/projects/o%2Fr/members/all/") => json!({"access_level": 30}),
                ("GET", _, _) if full == file.0 => json!({"encoding": "base64", "content": file.1}),
                ("GET", Some(""), _) => json!(list.borrow().iter().filter(|i| !full.contains("state=opened") || i["state"] == "opened")
                    .filter(|i| !full.contains("labels=taskq") || i["labels"].to_string().contains("\"taskq\"")).collect::<Vec<_>>()),
                ("POST", Some(""), _) => {
                    let body = body.unwrap();
                    let labels: Vec<_> = body["labels"].as_str().unwrap().split(',').filter(|l| !l.is_empty()).collect();
                    let n = list.borrow().iter().filter_map(|i| i["iid"].as_u64()).max().unwrap() + 1;
                    // GitLab keeps the description without its trailing newline (#1914)
                    let mut created = issue(n, body["title"].as_str().unwrap(), &labels, "opened");
                    created["description"] = json!(body["description"].as_str().unwrap().trim());
                    list.borrow_mut().push(created);
                    json!({"iid": n})
                }
                ("GET", _, Some((n, "notes"))) => json!(notes.borrow().iter().filter(|(i, ..)| i.to_string() == n).map(|(_, id, body)|
                    json!({"id": id, "body": body, "system": false, "author": {"id": 1, "username": "owner"}, "created_at": at, "updated_at": at})).collect::<Vec<_>>()),
                ("POST", _, Some((n, "notes"))) => {
                    let id = 100 + notes.borrow().len() as u64;
                    notes.borrow_mut().push((n.parse().unwrap(), id, body.unwrap()["body"].as_str().unwrap().into()));
                    json!({"id": id})
                }
                ("GET", Some(n), None) => list.borrow()[find(n)].clone(),
                ("PUT", Some(n), None) => {
                    let (body, k) = (body.unwrap(), find(n));
                    let mut all = list.borrow_mut();
                    let csv = |key: &str| body[key].as_str().unwrap_or("").split(',').filter(|l| !l.is_empty()).map(String::from).collect::<Vec<_>>();
                    let mut labels: Vec<String> = serde_json::from_value(all[k]["labels"].clone()).unwrap();
                    labels.retain(|l| !csv("remove_labels").contains(l));
                    labels.extend(csv("add_labels").into_iter().filter(|l| !labels.contains(l)).collect::<Vec<_>>());
                    all[k]["labels"] = json!(labels);
                    all[k].clone()
                }
                _ => panic!("unexpected {method} {url}"),
            };
            Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: "read" })
        }));
        ((KIND.board)(api), issues)
    }

    // #2352/R12: any heartbeat changed the open list and `tasks`, so each pass of each project billed both. Before this fix
    // the ten passes below read the full list ten times and `tasks` asked for every issue. Keep: a task change and a task
    // write still read the list, and an own write shows at once. The fixture substitutes HTTP, not native traffic.
    #[test]
    fn heartbeats_do_not_bill_the_open_list_each_pass() {
        let row = |n: u64, label: &str, at: &str| json!({"iid": n, "title": format!("issue {n}"), "description": "old", "labels": [label],
            "state": "opened", "author": {"id": 1}, "created_at": "2026-11-04T00:00:00Z", "updated_at": at});
        let rows = Rc::new(RefCell::new(vec![row(1, "taskq-host", "2026-11-04T01:00:00Z"), row(2, "taskq", "2026-11-04T01:00:00Z")]));
        let urls: Rc<RefCell<Vec<String>>> = Rc::default();
        let (list, log) = (rows.clone(), urls.clone());
        let mut api = api("gitlab.example", "o/r", "test");
        api.transport = Some(Box::new(move |method, url, body| {
            let full = url.trim_start_matches("https://gitlab.example/api/v4/projects/o%2Fr").to_string();
            let v = if full.starts_with("/members/all/") {
                json!({"access_level": 30})
            } else if method == "GET" {
                log.borrow_mut().push(full.clone());
                json!(list.borrow().iter().filter(|i| !full.contains("state=opened") || i["state"] == "opened")
                    .filter(|i| !full.contains("labels=taskq") || i["labels"][0] == "taskq").collect::<Vec<_>>())
            } else {
                let (body, n) = (body.unwrap(), full.rsplit('/').next().unwrap().to_string());
                let mut all = list.borrow_mut();
                let at = all.iter().position(|i| i["iid"].to_string() == n).unwrap();
                if let Some(text) = body["description"].as_str() { all[at]["description"] = json!(text); }
                if body["state_event"] == "close" { all[at]["state"] = json!("closed"); }
                all[at]["updated_at"] = json!("2026-11-04T02:00:00Z");
                all[at].clone()
            };
            Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: "read" })
        }));
        let b = (KIND.board)(api);
        let full = |urls: &Rc<RefCell<Vec<String>>>| urls.borrow().iter().filter(|u| u.contains("state=opened")).count();
        assert_eq!(b.issues(None).unwrap().len(), 2);
        assert_eq!(b.kept(), None);
        for k in 0..10 {
            rows.borrow_mut()[0]["updated_at"] = json!(format!("2026-11-04T01:{:02}:00Z", 10 + k));
            assert_eq!(b.issues(None).unwrap().len(), 2);
            assert_eq!(b.tasks(0).unwrap().iter().map(|i| i.number).collect::<Vec<_>>(), [2], "`tasks` reads tasks only");
        }
        assert_eq!(full(&urls), 1, "one full list in ten passes: before this fix, eleven");
        assert!(b.kept().is_some(), "`views` learns the list is kept, so it relists a stale heartbeat");
        assert!(urls.borrow().iter().filter(|u| !u.contains("state=opened")).all(|u| u.contains("labels=taskq")), "{:?}", urls.borrow());
        b.edit(1, "mine").unwrap();
        assert_eq!(b.issues(None).unwrap()[0].body, "mine", "an own write patches the kept list");
        assert_eq!(full(&urls), 1);
        rows.borrow_mut()[1]["updated_at"] = json!("2026-11-04T03:00:00Z");
        b.issues(None).unwrap();
        assert_eq!(full(&urls), 2, "a task change reads the list");
        assert_eq!(b.kept(), None);
        b.issues(None).unwrap();
        b.close(2).unwrap();
        assert_eq!(b.issues(None).unwrap().iter().map(|i| i.number).collect::<Vec<_>>(), [1], "a task write forgets the kept list");
        assert_eq!(full(&urls), 3);
    }

    // Chain: § 7 Reports Make on GitLab -> claim issue setup -> § 1 create claims -> held drafts (#1945). Defect:
    // gradus/cyprusbz#540, a GitLab board without `create_claim_issue`, refused Make with 'configure create_claim_issue
    // before idempotent create', so its proposed task was filed by hand as #541. Risk: Make creates a task without a
    // claim, leaves the key unwritten, or a retry creates a second claim issue or task.
    #[test]
    fn gitlab_report_make_settles_the_claim_issue_and_creates_held_drafts_once() {
        let sha = "a".repeat(40);
        let report = "# Question\nQ\n# Findings\nEvidence\n# Options\nPros/cons\n# Recommendation\nR\n# Proposed tasks\n```taskq-tasks\n[{\"title\":\"Follow the report\",\"goal\":\"Do the recommended change.\",\"acceptance\":[\"The change is visible.\"]}]\n```\n";
        let (b, issues) = report_board(&sha, report);
        let root = std::env::temp_dir().join(format!("taskq-gitlab-make-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("taskq.json"), r#"{"board": "gitlab", "repo": "o/r"}"#).unwrap();
        let config = || -> Value { serde_json::from_str(&std::fs::read_to_string(root.join("taskq.json")).unwrap()).unwrap() };
        let made = crate::reports::owner_act_until(b.as_ref(), &root, &config(), 540, &sha, "make",None).unwrap();
        assert_eq!(made["report"]["decision"], "made", "{made}");
        assert_eq!(config()["create_claim_issue"], 541, "the claim issue is written to taskq.json and read back");
        assert_eq!(config()["repo"], "o/r", "other keys stay");
        let claim = b.issue(541).unwrap();
        assert_eq!(claim.title, super::super::CLAIM_TITLE);
        assert_eq!(made["report"]["tasks_made"], json!([542]));
        let task = b.issue(542).unwrap();
        assert!(task.open && task.labels.contains(&"q-held".into()) && !task.labels.contains(&"taskq".into()), "{:?}", task.labels);
        assert!(task.body.contains("Depends on #540"));
        assert_eq!(crate::core::fold(542, &super::super::entries(b.as_ref(), 542).unwrap(), false).state, crate::core::State::Held);
        assert!(b.comments(541).unwrap().iter().any(|c| c.body.starts_with(&format!("taskq: create-claim report.540.{sha}.1\n"))));
        assert!(b.comments(540).unwrap().iter().any(|c| c.body == format!("taskq: report-made {sha}\n[542]")));
        // a retry, with the written key or without it, creates nothing
        for raw in [config(), json!({"board": "gitlab", "repo": "o/r"})] {
            let again = crate::reports::owner_act_until(b.as_ref(), &root, &raw, 540, &sha, "make",None).unwrap();
            assert_eq!(again["report"]["tasks_made"], json!([542]));
            assert_eq!(issues.borrow().len(), 3);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
