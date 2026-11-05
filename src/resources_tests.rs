    use super::*;

    const INST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn fix() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/resource-policy-v1.json")).unwrap()
    }

    /// DashQ stand-in: queued replies (none: offline) and the exchange bodies it was sent; `at`: the host clock after
    /// the POST; `cut`: a connection file the host replaces while the POST is in flight.
    struct Dash { replies: Vec<(u16, Value)>, posts: Vec<Value>, at: u64, cut: Option<(PathBuf, String)> }
    impl Wire for Dash {
        fn post(&mut self, dest: &str, route: &str, auth: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
            assert_eq!((dest, route, auth), ("https://x/machine", EXCHANGE, Some("c")));
            self.posts.push(body.clone());
            if let Some((path, text)) = self.cut.take() { fs::write(path, text).unwrap() }
            if self.replies.is_empty() { return Err("offline".into()) }
            Ok(self.replies.remove(0))
        }
        fn now(&self) -> u64 { self.at }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    struct T { base: PathBuf, install: PathBuf, root: PathBuf, me: String }

    thread_local! {
        /// The project revision of this test's `setup`; `submit` expects it.
        static REVISION: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }

    /// One registered project with ID `project-1` under a host policy, and the fixture's grant with project limits.
    fn setup(name: &str) -> T {
        let base = std::env::temp_dir().join(format!("taskq-resources-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (install, root) = (base.join("install"), base.join("proj"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(root.join("taskq.json"), "{\n  \"repo\": \"o/r\",\n  \"limits\": {\"codex\": 2},\n  \"custom\": {\"keep\": true}\n}\n").unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        fs::write(install.join("host.json"), "{\"limits\":{\"codex\":5}}").unwrap();
        with(&install, |s| {
            sync(s, &install, 0)?;
            let (_, p) = s.projects.pop_first().unwrap();
            REVISION.with_borrow_mut(|r| r.clone_from(&p.revision));
            s.projects.insert("project-1".into(), p);
            let mut g = fix()["grant"].clone();
            g["project_limits"] = json!(true);
            g.as_object_mut().unwrap().remove("grant_id");
            g["by"] = json!("test");
            g["at"] = json!(0);
            s.grants.insert("grant-1".into(), serde_json::from_value(g).unwrap());
            Ok(())
        }).unwrap();
        let me = crate::host::Policy::read(&install).unwrap().me;
        fs::create_dir_all(enrollment::dir(&install)).unwrap();
        fs::write(conn_file(&install), conn(Some(active())).to_string()).unwrap();
        T { base, install, root, me }
    }

    fn active() -> Value {
        json!({"installation": INST, "generation": 1, "destination": "https://x/machine", "credential": "c"})
    }

    // #1395: the store never makes `<install>` a macOS bundle and an old `resources` store keeps its state.
    #[test]
    fn store_leaves_install_no_bundle() {
        let base=std::env::temp_dir().join(format!("taskq-res-dir-{}",std::process::id()));let _=fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("resources")).unwrap();fs::write(base.join("resources").join(FILE),"{}").unwrap();
        let d=dir(&base);
        assert!(!d.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("resources"));
        assert_eq!(fs::read_to_string(d.join(FILE)).unwrap(),"{}");assert!(!base.join("resources").exists());
        fs::remove_dir_all(base).unwrap();
    }

    // #1395: an older host recreated `resources` after the move; the owning service retires it and keeps both stores.
    #[test]
    fn owner_retires_recreated_store() {
        let base=std::env::temp_dir().join(format!("taskq-res-both-{}",std::process::id()));let _=fs::remove_dir_all(&base);
        for (d,t) in [("resource-state","real"),("resources","fresh")] {fs::create_dir_all(base.join(d)).unwrap();fs::write(base.join(d).join(FILE),t).unwrap();}
        assert_eq!(dir(&base),base.join("resource-state"),"both present: the real store stays the store");
        assert!(retire_superseded(&base).unwrap().is_some());
        assert!(!base.join("resources").exists(),"<install> is no bundle");
        assert_eq!(fs::read_to_string(base.join("resource-state").join(FILE)).unwrap(),"real");
        assert_eq!(fs::read_to_string(base.join("resource-state/superseded").join(FILE)).unwrap(),"fresh");
        fs::create_dir_all(base.join("resources")).unwrap();fs::write(base.join("resources").join(FILE),"again").unwrap();
        assert!(retire_superseded(&base).unwrap().is_some());
        assert_eq!(fs::read_to_string(base.join("resource-state/superseded").join(FILE)).unwrap(),"again");
        assert_eq!(retire_superseded(&base).unwrap(),None,"nothing to retire");
        fs::remove_dir_all(base).unwrap();
    }

    // H-A2: a retired or local-only schema never writes, and a broken host policy never falls back to project settings.
    #[test]
    fn host_policy_refuses_other_remote_schemas() {
        let t=setup("host-profile");let before=project_config(&t);
        let host=json!({"limits":{"codex":2},"owner_field":"preserve"}).to_string();
        fs::write(t.install.join("host.json"),&host).unwrap();
        let fence=Fence {clock:&||100,live:&||true};
        with(&t.install,|s|prepare(s,&t.install,100)).unwrap();
        let observed=read(&t.install).unwrap().projects["project-1"].clone();
        for schema in ["taskq.capacity/1","taskq.admission/1","taskq.host-capacity/1"] {
            let mut remote=submit(schema.replace(['.','/'],"-").as_str(),json!({"runtime_limit":0}));
            remote["expected_revision"]=json!(observed.revision);
            remote["policy_schema"]=json!(schema);
            with(&t.install,|s|decide(&t.install,s,&remote,(INST,1),true,100,&fence)).unwrap();
            assert_eq!(receipt_of(&t,remote["op_id"].as_str().unwrap())["reason"],"unsupported","{schema}");
        }
        assert_eq!(project_config(&t),before);assert_eq!(config(&t),host);
        let authority=crate::host::Policy::read(&t.install).unwrap();
        let mut want=authority.cap.json();want["project_limits"]=json!({"codex":2});
        assert_eq!(observed.policy,want);
        assert_eq!(observed.revision,authority.project_revision(&t.root, &crate::load(&t.root).unwrap().1).unwrap());
        assert!(report(&t.install,101).unwrap().iter().any(|s|s.contains("remote writes: taskq.project-limits/1 under a --project-limits grant")));
        fs::write(t.install.join("host.json"),"{\"limits\":{\"codex\":5}}").unwrap();
        with(&t.install,|s|prepare(s,&t.install,102)).unwrap();
        let changed=read(&t.install).unwrap().projects["project-1"].clone();
        assert_ne!(changed.revision,observed.revision);
        assert_eq!(changed.revision,crate::host::Policy::read(&t.install).unwrap().project_revision(&t.root,&crate::load(&t.root).unwrap().1).unwrap());
        for (file,body) in [("host.json","broken"),("host.json.pending","retained")] {
            fs::write(t.install.join(file),body).unwrap();
            with(&t.install,|s|prepare(s,&t.install,103)).unwrap();
            assert!(read(&t.install).unwrap().projects["project-1"].policy.is_null(),"no project fallback from {file}");
        }
        assert_eq!(project_config(&t),before);
        fs::remove_dir_all(t.base).unwrap();
    }

    // H-A2: reusing the bounded store does not export local receipts or grant remote control over their IDs.
    #[test]
    fn local_capacity_receipts_never_enter_the_remote_exchange() {
        let t=setup("local-isolation");
        fs::write(t.install.join("host.json"),"{\"limits\":{\"codex\":2}}").unwrap();
        let (capture,guard)=crate::profile::tests::collecting(&t.install);
        let p=crate::host::Policy::read(&t.install).unwrap();let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let request=json!({"id":"local-op","issued_at":now,"expires_at":now+300,"ops":[{"target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":"codex"},
            "policy_schema":"taskq.host-capacity/1","expected_revision":p.revision().unwrap(),"changes":{"runtime_limit":3}}]});
        assert_eq!(crate::host::local_capacity(&t.install,&p.me,&request).unwrap()["state"],"saved");
        // Only the local save needs a collector; settle its receipts before testing exchange isolation.
        drop(guard);capture.flush().unwrap();drop(capture);
        let before=read(&t.install).unwrap().ops["local-op"].clone();let policy=fs::read(t.install.join("host.json")).unwrap();
        let mut cancel=fix()["cancel"].clone();cancel["op_id"]=json!("local-op");
        let (sent,log)=go(&t,&[submit("local-op",json!({"runtime_limit":0}))],&[cancel],100);
        assert!(sent.is_empty(),"local receipt leaked into DashQ: {sent:?}");
        assert_eq!(read(&t.install).unwrap().ops["local-op"],before);
        assert_eq!(fs::read(t.install.join("host.json")).unwrap(),policy);
        assert_eq!(log.len(),2,"remote cancel and ID reuse must both be explicitly refused");
        for (key,id) in [("local_restart","local-restart"),("local_stop","local-stop"),("local_uninstall","local-uninstall"),("local_install","local-install")] {
            with(&t.install,|state| {
                state.ops.insert(id.into(),Op {bound:json!({(key):{"request":{"id":id}}}),
                    receipt:json!({"id":id,"state":"queued"}),expires_at:0,saved_at:Some(0),writing:false,sent:true});Ok(())
            }).unwrap();
            let original=read(&t.install).unwrap().ops[id].clone();
            let mut cancel=fix()["cancel"].clone();cancel["op_id"]=json!(id);
            let (sent,log)=go(&t,&[submit(id,json!({"runtime_limit":0}))],&[cancel],100);
            assert!(sent.is_empty());assert_eq!(log.len(),2);assert_eq!(read(&t.install).unwrap().ops[id],original);
        }
        fs::remove_dir_all(t.base).unwrap();
    }

    fn conn_file(install: &Path) -> PathBuf {
        enrollment::dir(install).join("connection.json")
    }

    fn conn(active: Option<Value>) -> Value {
        json!({"installation": INST, "pending": null, "active": active, "revoke": []})
    }

    fn submit(op: &str, changes: Value) -> Value {
        let mut v = fix()["submit"].clone();
        v["op_id"] = json!(op);
        v["policy_schema"] = json!(OVERLAY);
        v["expected_revision"] = json!(REVISION.with_borrow(Clone::clone));
        v["changes"] = changes;
        v
    }

    /// One exchange at `now` whose reply delivers `ops` and `cancels`: the receipts sent and the log.
    fn go(t: &T, ops: &[Value], cancels: &[Value], now: u64) -> (Vec<Value>, Vec<String>) {
        let mut d = Dash { replies: vec![(200, json!({"schema": SCHEMA, "server_at": now, "operations": ops, "cancellations": cancels}))], posts: vec![], at: 0, cut: None };
        let (log, _) = exchange(&t.install, &active(), &mut d, now).unwrap();
        (d.posts.first().map(|p| p["receipts"].as_array().unwrap().clone()).unwrap_or_default(), log)
    }

    fn receipt_of(t: &T, op: &str) -> Value {
        read(&t.install).unwrap().ops.get(op).unwrap_or_else(|| panic!("{op} not recorded")).receipt.clone()
    }

    /// The one file a remote write may change.
    fn config(t: &T) -> String {
        fs::read_to_string(t.install.join("host.json")).unwrap()
    }

    fn project_config(t: &T) -> String {
        fs::read_to_string(t.root.join("taskq.json")).unwrap()
    }

    // Chain: DashQ submit -> exchange -> host grant check -> host.json overlay write -> service status readback ->
    // receipt. Risk: a saved write reported applied before the service runs it.
    #[test]
    fn saved_turns_applied_only_on_a_fresh_matching_readback() {
        let t = setup("apply");
        let first = read(&t.install).unwrap().projects["project-1"].clone();
        let (sent, log) = go(&t, &[submit("op-1", json!({"runtime_limit": 0}))], &[], 100);
        assert_eq!((sent.len(), log), (0, vec!["resources: op-1 saved".to_string()]));
        assert!(t.install.join("poke").exists(), "the service reloads");
        let s = read(&t.install).unwrap();
        let p = s.projects["project-1"].clone();
        let r = receipt_of(&t, "op-1");
        assert_ne!(p.revision, first.revision);
        assert_eq!((&r["state"], &r["saved_revision"], &r["effective_revision"]), (&json!("saved"), &json!(p.revision), &Value::Null));
        fs::create_dir_all(crate::dot(&t.root)).unwrap();
        let status = |at: u64, host: &str, revision: &str, limits: &Value| fs::write(crate::dot(&t.root).join("status.json"), json!({"at": at, "host": host, "capacity": {"project_revision": revision, "project_limits": limits}}).to_string()).unwrap();
        let (sent, _) = go(&t, &[], &[], 101);
        assert_eq!((sent.len(), &sent[0]["state"]), (1, &json!("saved")), "no status yet: saved");
        let limits = &p.policy["project_limits"];
        for (at, host, revision, seen, now) in [(90, t.me.as_str(), &p.revision, limits, 110), (110, t.me.as_str(), &first.revision, limits, 110), (110, t.me.as_str(), &p.revision, &first.policy["project_limits"], 110), (110, "other", &p.revision, limits, 110), (110, t.me.as_str(), &p.revision, limits, 171)] {
            status(at, host, revision, seen);
            go(&t, &[], &[], now);
            assert_eq!(receipt_of(&t, "op-1")["state"], "saved", "stale, old, other host or old readback at {now}");
        }
        status(110, &t.me, &p.revision, limits);
        let (sent, _) = go(&t, &[], &[], 111);
        let r = receipt_of(&t, "op-1");
        assert_eq!((&r["state"], &r["effective_revision"], &r["observed_at"]), (&json!("applied"), &json!(p.revision), &json!(110)));
        assert_eq!(sent, [r], "the applied receipt goes out in the same exchange");
        assert!(go(&t, &[], &[], 112).0.is_empty(), "and only once");
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: hand edit or duplicate/lost reply or reused op ID -> exchange -> decide. Risk: an operation writing over
    // a newer config, writing twice, or a reused ID rebinding to another payload.
    #[test]
    fn conflicts_duplicates_and_mismatches_never_write_again() {
        let t = setup("dedup");
        fs::write(t.root.join("taskq.json"), project_config(&t).replace("\"codex\": 2", "\"codex\": 3")).unwrap();
        let edited = config(&t);
        go(&t, &[submit("op-0", json!({"runtime_limit": 0}))], &[], 100);
        assert_eq!((receipt_of(&t, "op-0")["reason"].as_str(), config(&t)), (Some("conflict"), edited), "a hand edit gets a new revision");
        let mut op = submit("op-1", json!({"runtime_limit": 1}));
        op["expected_revision"] = json!(read(&t.install).unwrap().projects["project-1"].revision);
        go(&t, std::slice::from_ref(&op), &[], 100);
        let (saved, after) = (receipt_of(&t, "op-1"), config(&t));
        assert_eq!(saved["state"], "saved");
        // the reply was lost: the next exchange resends the receipt, DashQ redelivers the same submit
        let mut off = Dash { replies: vec![], posts: vec![], at: 0, cut: None };
        assert!(exchange(&t.install, &active(), &mut off, 101).is_err());
        let (sent, _) = go(&t, &[op.clone(), op.clone()], &[], 102);
        assert!(sent.contains(&saved), "unsent receipts go again");
        let (sent, _) = go(&t, &[], &[], 103);
        assert_eq!((sent, receipt_of(&t, "op-1"), config(&t)), (vec![saved.clone()], saved.clone(), after.clone()), "one write, the same receipt");
        let mut other = op.clone();
        other["changes"] = json!({"runtime_limit": 0});
        let (_, log) = go(&t, &[other], &[], 104);
        assert_eq!(log, ["resources: op-1 reused with another payload; original kept"]);
        assert_eq!((receipt_of(&t, "op-1"), config(&t)), (saved, after));
        assert!(go(&t, &[], &[], 105).0.is_empty(), "a mismatch sends nothing");
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: #2138/R9: the limits-only remote profile must preserve local execution policy, including without limits.
    #[test]
    fn balancing_overlay_preserves_execution_policy() {
        let t=setup("execution-overlay");
        let execution=json!({"default_runtime":"codex","allowed_runtimes":["codex"],"auto_reroute":true});
        fs::write(t.install.join("host.json"),json!({"limits":{"codex":5},"projects":{t.root.display().to_string():execution}}).to_string()).unwrap();
        go(&t,&[],&[],100);
        with(&t.install,|s|{s.grants.get_mut("grant-1").unwrap().project_limits=true;Ok(())}).unwrap();
        for (id,limit) in [("lower",json!(1)),("restore",Value::Null)] {
            let revision=read(&t.install).unwrap().projects["project-1"].revision.clone();
            let mut op=submit(id,json!({"runtime_limit":limit}));op["policy_schema"]=json!(OVERLAY);op["expected_revision"]=json!(revision);
            go(&t,&[op],&[],101);
            assert_eq!(receipt_of(&t,id)["state"],"saved","{:?}",receipt_of(&t,id));
            let saved=crate::host::Policy::read(&t.install).unwrap();
            let (_,entry)=saved.overlay(&t.root).unwrap();
            for key in ["default_runtime","allowed_runtimes","auto_reroute"] {assert_eq!(entry[key],execution[key]);}
        }
        fs::remove_dir_all(t.base).unwrap();
    }

    // Chain #1282: DashQ overlay submit -> exchange -> project grant -> host.json projects writer -> service
    // status readback -> receipt. Risk: a project grant read as host-wide, a write losing other host keys, a stale
    // or reverted revision applying, a duplicate writing twice, or saved reported applied without the exact revision.
    #[test]
    fn project_overlay_writes_only_its_scope_and_applies_on_exact_readback() {
        let t = setup("overlay");
        let host = json!({"limits": {"codex": 5, "claude": 3}, "projects": {"/elsewhere": {"limits": {"codex": 1}}}, "owner_field": "keep"});
        fs::write(t.install.join("host.json"), host.to_string()).unwrap();
        let project = project_config(&t);
        with(&t.install, |s| { s.grants.get_mut("grant-1").unwrap().project_limits = false; Ok(()) }).unwrap();
        let rev = |t: &T| read(&t.install).unwrap().projects["project-1"].revision.clone();
        let op = |id: &str, limit: Value, expected: &str| {
            let mut v = submit(id, json!({"runtime_limit": limit}));
            v["policy_schema"] = json!(OVERLAY);
            v["expected_revision"] = json!(expected);
            v
        };
        go(&t, &[], &[], 100);
        let r0 = rev(&t);
        assert_eq!(r0, crate::host::Policy::read(&t.install).unwrap().project_revision(&t.root, &crate::load(&t.root).unwrap().1).unwrap());
        go(&t, &[op("no-flag", json!(1), &r0)], &[], 100);
        assert_eq!(receipt_of(&t, "no-flag")["reason"], "scope", "a grant without --project-limits never writes the overlay");
        assert_eq!(fs::read_to_string(t.install.join("host.json")).unwrap(), host.to_string());
        with(&t.install, |s| { s.grants.get_mut("grant-1").unwrap().project_limits = true; Ok(()) }).unwrap();
        let mut host_scope = op("host-scope", json!(1), &r0);
        host_scope["target"]["project_id"] = Value::Null;
        host_scope["target"]["runtime_id"] = Value::Null;
        let mut other_runtime = op("claude", json!(1), &r0);
        other_runtime["target"]["runtime_id"] = json!("claude");
        let mut legacy = op("legacy", json!(1), &r0);
        legacy["policy_schema"] = json!("taskq.admission/1");
        let mut extra = op("extra", json!(1), &r0);
        extra["changes"]["maximum"] = json!(2);
        for (v, want) in [(host_scope, "invalid"), (other_runtime, "scope"), (legacy, "unsupported"), (extra, "invalid"), (op("stale", json!(1), "r-old"), "conflict"), (op("raise", json!(6), &r0), "invalid")] {
            let id = v["op_id"].as_str().unwrap().to_string();
            go(&t, &[v], &[], 100);
            assert_eq!(receipt_of(&t, &id)["reason"], want, "{id}");
        }
        assert_eq!(fs::read_to_string(t.install.join("host.json")).unwrap(), host.to_string(), "refusals write nothing");
        let lower = op("lower", json!(1), &r0);
        let (_, log) = go(&t, std::slice::from_ref(&lower), &[], 101);
        assert_eq!(log, ["resources: lower saved"]);
        let saved: Value = serde_json::from_str(&fs::read_to_string(t.install.join("host.json")).unwrap()).unwrap();
        let key = t.root.display().to_string();
        assert_eq!(saved["projects"][&key], json!({"limits": {"codex": 1}, "op": "lower"}));
        assert_eq!((&saved["limits"], &saved["owner_field"], &saved["projects"]["/elsewhere"]), (&host["limits"], &host["owner_field"], &host["projects"]["/elsewhere"]), "other keys, projects and runtimes kept");
        assert_eq!(project_config(&t), project, "the project's taskq.json is never written");
        assert!(t.install.join("poke").exists(), "the host reloads");
        let r1 = rev(&t);
        let policy = crate::host::Policy::read(&t.install).unwrap();
        assert_eq!(r1, policy.project_revision(&t.root, &crate::load(&t.root).unwrap().1).unwrap());
        assert_eq!(policy.project(&t.root, &serde_json::from_str(&project).unwrap()).unwrap().limits["codex"], 1);
        let r = receipt_of(&t, "lower");
        assert_eq!((&r["state"], &r["saved_revision"]), (&json!("saved"), &json!(r1)));
        // a lost reply: the same submit again resends the receipt and never writes twice; another payload is refused
        let bytes = fs::read(t.install.join("host.json")).unwrap();
        go(&t, &[lower.clone()], &[], 102);
        let (_, log) = go(&t, &[op("lower", json!(0), &r1)], &[], 102);
        assert_eq!(log, ["resources: lower reused with another payload; original kept"]);
        assert_eq!((receipt_of(&t, "lower"), fs::read(t.install.join("host.json")).unwrap()), (r.clone(), bytes));
        fs::create_dir_all(crate::dot(&t.root)).unwrap();
        let status = |at: u64, revision: &str, limits: Value| fs::write(crate::dot(&t.root).join("status.json"),
            json!({"at": at, "host": policy.me, "capacity": {"project_revision": revision, "project_limits": limits}}).to_string()).unwrap();
        for (at, revision, limits, now) in [(110, r0.as_str(), json!({"codex": 2}), 110), (110, r1.as_str(), json!({"codex": 2}), 110), (90, r1.as_str(), json!({"codex": 1}), 110), (110, r1.as_str(), json!({"codex": 1}), 171)] {
            status(at, revision, limits);
            go(&t, &[], &[], now);
            assert_eq!(receipt_of(&t, "lower")["state"], "saved", "old revision, old limits, stale or late readback at {now}");
        }
        // a taskq.json limits edit after the save is a new effective policy: the old receipt never applies on it
        fs::write(t.root.join("taskq.json"), project.replace("\"codex\": 2", "\"codex\": 0")).unwrap();
        status(110, &r1, json!({"codex": 0}));
        go(&t, &[], &[], 111);
        assert_ne!(rev(&t), r1);
        assert_eq!(receipt_of(&t, "lower")["state"], "saved", "a project policy edit supersedes the saved revision");
        fs::write(t.root.join("taskq.json"), &project).unwrap();
        status(110, &r1, json!({"codex": 1}));
        go(&t, &[], &[], 111);
        let r = receipt_of(&t, "lower");
        assert_eq!((&r["state"], &r["effective_revision"], &r["observed_at"]), (&json!("applied"), &json!(r1), &json!(110)));
        // the projection: allowlisted keys, this root's saved overlay at r1, effective only from the fresh status
        let pj = projection(&t.install, 111, true).unwrap();
        let codex = pj["targets"].as_array().unwrap().iter().find(|x| x["target"]["runtime_id"] == "codex").unwrap();
        let keys: Vec<&String> = codex.as_object().unwrap().keys().collect();
        assert_eq!(keys, fix()["overlay_projection"]["targets"][0].as_object().unwrap().keys().collect::<Vec<_>>());
        assert_eq!((&codex["target"]["project_id"], &codex["supported"][0]["policy_schema"]), (&json!("project-1"), &json!(OVERLAY)));
        assert_eq!((&codex["current_revision"], &codex["effective_revision"], &codex["current_policy"], &codex["effective_policy"]),
            (&json!(r1), &json!(r1), &json!({"runtime_limit": 1}), &json!({"runtime_limit": 1})));
        assert_eq!(projection(&t.install, 200, true).unwrap()["targets"][0]["effective_revision"], Value::Null, "a stale status is unknown");
        // the readback keeps its own age: observed_at is the status time, so 61 s later a consumer reads it stale
        assert_eq!((&pj["observed_at"], &projection(&t.install, 200, true).unwrap()["observed_at"]), (&json!(110), &json!(110)));
        // status reasons survive as blockers; an unknown one makes the target partial, never complete
        fs::write(crate::dot(&t.root).join("status.json"), json!({"at": 110, "host": policy.me, "capacity": {"project_revision": r1, "project_limits": {"codex": 1},
            "gate": "held", "reasons": [{"code": "cpu", "text": "busy"}, {"code": "future-reason", "text": "?"}]}}).to_string()).unwrap();
        let codex = projection(&t.install, 111, true).unwrap()["targets"].as_array().unwrap().iter().find(|x| x["target"]["runtime_id"] == "codex").unwrap().clone();
        assert_eq!((codex["blockers"].as_array().unwrap().iter().map(|b| (b["code"].as_str().unwrap(), b["knowledge"].as_str().unwrap())).collect::<Vec<_>>(), &codex["completeness"]),
            (vec![("cpu", "known"), ("unknown", "unknown")], &json!("partial")));
        // 61 s later the same-host status is stale: last-known blockers stay, an unknown sample is added
        let codex = projection(&t.install, 171, true).unwrap()["targets"].as_array().unwrap().iter().find(|x| x["target"]["runtime_id"] == "codex").unwrap().clone();
        assert_eq!((codex["blockers"].as_array().unwrap().iter().map(|b| (b["code"].as_str().unwrap(), b["knowledge"].as_str().unwrap())).collect::<Vec<_>>(), &codex["completeness"], &codex["effective_revision"], &codex["effective_policy"]["runtime_limit"]),
            (vec![("cpu", "known"), ("unknown", "unknown"), ("sample", "unknown")], &json!("unknown"), &Value::Null, &Value::Null));
        // an unreadable taskq.json keeps the project listed with a config blocker and nothing effective, fresh or stale
        fs::write(t.root.join("taskq.json"), "{").unwrap();
        let codex = projection(&t.install, 111, true).unwrap()["targets"].as_array().unwrap().iter().find(|x| x["target"]["runtime_id"] == "codex").unwrap().clone();
        assert_eq!((&codex["blockers"][0]["code"], &codex["completeness"], &codex["effective_revision"], &codex["effective_policy"]["runtime_limit"]),
            (&json!("config"), &json!("unknown"), &Value::Null, &Value::Null));
        let codex = projection(&t.install, 171, true).unwrap()["targets"].as_array().unwrap().iter().find(|x| x["target"]["runtime_id"] == "codex").unwrap().clone();
        assert_eq!(codex["blockers"].as_array().unwrap().iter().map(|b| (b["code"].as_str().unwrap(), b["knowledge"].as_str().unwrap())).collect::<Vec<_>>(),
            vec![("config", "known"), ("cpu", "known"), ("unknown", "unknown"), ("sample", "unknown")]);
        fs::write(t.root.join("taskq.json"), &project).unwrap();
        status(110, &r1, json!({"codex": 1}));
        // restore: null removes the entry; a stale operation naming the first revision cannot apply after the revert
        go(&t, &[op("restore", Value::Null, &r1)], &[], 120);
        assert_eq!(receipt_of(&t, "restore")["state"], "saved");
        let restored = crate::host::Policy::read(&t.install).unwrap();
        assert_eq!(restored.project(&t.root, &serde_json::from_str(&project).unwrap()).unwrap().limits["codex"], 2);
        assert!(rev(&t) != r0 && rev(&t) != r1, "edit-and-revert is a new revision");
        go(&t, &[op("replay-r0", json!(1), &r0)], &[], 121);
        assert_eq!(receipt_of(&t, "replay-r0")["reason"], "conflict");
        // revoke and expiry refuse before the write
        with(&t.install, |s| { s.grants.get_mut("grant-1").unwrap().revoked = true; Ok(()) }).unwrap();
        let current = rev(&t);
        go(&t, &[op("revoked", json!(1), &current)], &[], 122);
        assert_eq!(receipt_of(&t, "revoked")["reason"], "revoked");
        // a restart reads the same durable receipts; without host.json the overlay profile is unsupported
        let before = read(&t.install).unwrap().ops;
        assert_eq!(read(&t.install).unwrap().ops, before);
        with(&t.install, |s| { s.grants.get_mut("grant-1").unwrap().revoked = false; Ok(()) }).unwrap();
        fs::remove_file(t.install.join("host.json")).unwrap();
        go(&t, &[], &[], 123);
        go(&t, &[op("no-policy", json!(1), &rev(&t))], &[], 123);
        assert_eq!(receipt_of(&t, "no-policy")["reason"], "unsupported");
        assert_eq!(project_config(&t), project);
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: DashQ-approved submit outside the host grant -> exchange -> authorize. Risk: DashQ's grant alone, another
    // host or generation, a revoked or expired grant, or a path or privilege change reaching host.json.
    #[test]
    fn grant_scope_typing_and_lifetime_refuse_before_any_write() {
        let t = setup("scope");
        with(&t.install, |s| {
            let mut g = s.grants["grant-1"].clone();
            g.revoked = true;
            s.grants.insert("grant-r".into(), g.clone());
            // keeps the exchange going after grant-1 expires
            s.grants.insert("grant-2".into(), Grant { revoked: false, expires_at: 10_000, ..g });
            Ok(())
        }).unwrap();
        let before = config(&t);
        let rt = |c: Value| submit("x", json!({"runtime_limit": c}));
        let set = |mut v: Value, path: &[&str], x: Value| {
            let last = path.len() - 1;
            path[..last].iter().fold(&mut v, |v, k| &mut v[*k])[path[last]] = x;
            v
        };
        let cases = [
            (set(rt(json!(0)), &["grant_id"], json!("grant-x")), 100, "unauthorized"),
            (set(rt(json!(0)), &["actor_id"], json!("operator-2")), 100, "unauthorized"),
            (set(rt(json!(0)), &["grant_id"], json!("grant-r")), 100, "revoked"),
            (set(set(rt(json!(0)), &["issued_at"], json!(950)), &["expires_at"], json!(1200)), 1000, "revoked"),
            (set(rt(json!(0)), &["target", "generation"], json!(2)), 100, "scope"),
            (set(rt(json!(0)), &["target", "installation_id"], json!("b".repeat(64))), 100, "scope"),
            (set(rt(json!(0)), &["target", "project_id"], json!("project-2")), 100, "scope"),
            (set(rt(json!(0)), &["target", "runtime_id"], json!("claude")), 100, "scope"),
            (set(set(submit("x", json!({"maximum": 2})), &["target", "project_id"], Value::Null), &["target", "runtime_id"], Value::Null), 100, "invalid"),
            (submit("x", json!({"runtime_limit": 0, "path": "/etc"})), 100, "invalid"),
            (submit("x", json!({"privileged": true})), 100, "invalid"),
            (set(rt(json!(0)), &["policy_schema"], json!("taskq.capacity/2")), 100, "unsupported"),
            (set(rt(json!(0)), &["expected_revision"], json!("r0")), 100, "conflict"),
        ];
        for (i, (mut op, now, want)) in cases.into_iter().enumerate() {
            op["op_id"] = json!(format!("op-{i}"));
            go(&t, &[op], &[], now);
            assert_eq!((receipt_of(&t, &format!("op-{i}"))["reason"].as_str(), config(&t)), (Some(want), before.clone()), "case {i}");
        }
        // a runtime without a project is no valid target: nothing to answer, nothing recorded (legacy_runtime_scope)
        let (_, log) = go(&t, &[set(submit("op-t", json!({"runtime_limit": 0})), &["target", "project_id"], Value::Null)], &[], 100);
        assert_eq!((log, config(&t)), (vec!["resources: dropped an operation without a valid ID or target".to_string()], before.clone()));
        assert!(!read(&t.install).unwrap().ops.contains_key("op-t"));
        // clock: DashQ's time 100 s away; the deadline on either clock
        let mut d = Dash { replies: vec![(200, json!({"schema": SCHEMA, "server_at": 200, "operations": [submit("op-c", json!({"runtime_limit": 0}))], "cancellations": []}))], posts: vec![], at: 0, cut: None };
        exchange(&t.install, &active(), &mut d, 100).unwrap();
        assert_eq!(receipt_of(&t, "op-c")["reason"], "clock_unknown");
        go(&t, &[submit("op-e", json!({"runtime_limit": 0}))], &[], 400);
        go(&t, &[submit("op-e", json!({"runtime_limit": 0}))], &[], 401);
        assert_eq!((receipt_of(&t, "op-e")["state"].as_str(), config(&t)), (Some("expired"), before.clone()), "redelivered after expiry");
        // a host-policy grant still has no host-wide remote write; a retired schema never writes
        with(&t.install, |s| { s.grants.get_mut("grant-1").unwrap().host_policy = true; Ok(()) }).unwrap();
        go(&t, &[set(set(submit("op-h", json!({"maximum": 2})), &["target", "project_id"], Value::Null), &["target", "runtime_id"], Value::Null)], &[], 100);
        assert_eq!(receipt_of(&t, "op-h")["reason"], "invalid");
        go(&t, &[set(submit("op-f", json!({"runtime_limit": 0})), &["policy_schema"], json!("taskq.capacity/1"))], &[], 100);
        assert_eq!((receipt_of(&t, "op-f")["reason"].as_str(), config(&t)), (Some("unsupported"), before));
        // the project leaves the registry: its ID goes for good
        fs::write(t.install.join("projects"), "").unwrap();
        go(&t, &[submit("op-u", json!({"runtime_limit": 0}))], &[], 100);
        assert_eq!(receipt_of(&t, "op-u")["reason"], "scope");
        assert!(read(&t.install).unwrap().projects.is_empty());
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: revoke -> exchange. Risk: a revoked host still exchanging, or a pending operation applying after revoke.
    #[test]
    fn revoked_grant_stops_the_exchange_and_pending_writes() {
        let t = setup("revoke");
        revoke(&t.install, "grant-1").unwrap();
        let mut d = Dash { replies: vec![], posts: vec![], at: 0, cut: None };
        assert_eq!(exchange(&t.install, &active(), &mut d, 100).unwrap(), (vec![], false));
        assert!(d.posts.is_empty(), "no active grant, nothing unsent: no exchange");
        with(&t.install, |s| {
            let mut g = s.grants["grant-1"].clone();
            g.revoked = false;
            s.grants.insert("grant-2".into(), g);
            Ok(())
        }).unwrap();
        let before = config(&t);
        go(&t, &[submit("op-1", json!({"runtime_limit": 0}))], &[], 100);
        assert_eq!((receipt_of(&t, "op-1")["reason"].as_str(), config(&t)), (Some("revoked"), before));
        assert!(revoke(&t.install, "grant-9").is_err());
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: cancellation and submit racing through the exchange; crash between marker and write. Risk: a cancelled
    // operation writing later, a completed write rolled back, or an interrupted write repeated after restart.
    #[test]
    fn cancel_races_and_interrupted_writes_settle_once() {
        let t = setup("cancel");
        let before = config(&t);
        let mut cancel = fix()["cancel"].clone();
        go(&t, &[submit("op-1", json!({"runtime_limit": 0}))], &[cancel.clone()], 100);
        assert_eq!((receipt_of(&t, "op-1")["state"].as_str(), config(&t)), (Some("cancelled"), before.clone()), "cancel wins");
        go(&t, &[submit("op-1", json!({"runtime_limit": 0}))], &[], 101);
        assert_eq!((receipt_of(&t, "op-1")["state"].as_str(), config(&t)), (Some("cancelled"), before.clone()), "late delivery cannot write");
        go(&t, &[submit("op-2", json!({"runtime_limit": 0}))], &[], 102);
        cancel["op_id"] = json!("op-2");
        go(&t, &[], &[cancel], 103);
        assert_eq!(receipt_of(&t, "op-2")["state"], "saved", "apply wins: never rolled back");
        // the marker was durable, the write may or may not have happened: unknown, and never written again
        let after = config(&t);
        with(&t.install, |s| { s.ops.get_mut("op-2").unwrap().writing = true; Ok(()) }).unwrap();
        let (sent, _) = go(&t, &[submit("op-2", json!({"runtime_limit": 0}))], &[], 104);
        assert_eq!((sent.len(), &sent[0]["state"]), (1, &json!("unknown")));
        assert_eq!((receipt_of(&t, "op-2")["state"].as_str(), config(&t)), (Some("unknown"), after));
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: exchange POST in flight -> disconnect, new generation or clock past a deadline -> decide -> write.
    // Risk: an operation applying on a connection that is no longer active, or after its or its grant's deadline
    // because the time was read before the POST, the lock wait or the marker save.
    #[test]
    fn races_during_the_exchange_never_write() {
        let t = setup("race");
        let before = config(&t);
        let reply = |op: &str| (200, json!({"schema": SCHEMA, "server_at": 399, "operations": [submit(op, json!({"runtime_limit": 0}))], "cancellations": []}));
        let mut gen2 = active();
        gen2["generation"] = json!(2);
        for (op, cut) in [("op-d", conn(None)), ("op-g", conn(Some(gen2)))] {
            let mut d = Dash { replies: vec![reply(op)], posts: vec![], at: 0, cut: Some((conn_file(&t.install), cut.to_string())) };
            let (log, _) = exchange(&t.install, &active(), &mut d, 399).unwrap();
            assert_eq!(log, ["resources: connection changed during the exchange; 1 operations not taken"]);
            assert!(!read(&t.install).unwrap().ops.contains_key(op) && config(&t) == before, "{op}");
            fs::write(conn_file(&t.install), conn(Some(active())).to_string()).unwrap();
        }
        // started at 399, the operation expires at 400, the reply lands at 401
        let mut d = Dash { replies: vec![reply("op-e")], posts: vec![], at: 401, cut: None };
        exchange(&t.install, &active(), &mut d, 399).unwrap();
        assert_eq!((receipt_of(&t, "op-e")["state"].as_str(), config(&t)), (Some("expired"), before.clone()));
        // the clock passes the operation's or the grant's deadline, or the connection goes, during the marker save
        let mut late = submit("op-l", json!({"runtime_limit": 0}));
        late["issued_at"] = json!(950);
        late["expires_at"] = json!(1200);
        let cases = [(submit("op-x", json!({"runtime_limit": 0})), 399, 400, true, "expired"), (late, 990, 1000, true, "revoked"), (submit("op-c", json!({"runtime_limit": 0})), 100, 100, false, "scope")];
        for (v, now, then, up, want) in cases {
            let op = v["op_id"].as_str().unwrap().to_string();
            let fence = Fence { clock: &|| then, live: &|| up };
            with(&t.install, |s| decide(&t.install, s, &v, (INST, 1), true, now, &fence)).unwrap();
            let r = receipt_of(&t, &op);
            assert_eq!((r["reason"].as_str().or(r["state"].as_str()), config(&t)), (Some(want), before.clone()), "{op}");
        }
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: exchange POST in flight -> `service uninstall` -> decide -> write. Risk: a removed project still taking
    // a write because the registration was read before the POST or the lock wait, or its ID keeping authority.
    #[test]
    fn unregister_during_the_exchange_never_writes() {
        let t = setup("unregister");
        let before = config(&t);
        let reply = |op: &str| (200, json!({"schema": SCHEMA, "server_at": 399, "operations": [submit(op, json!({"runtime_limit": 0}))], "cancellations": []}));
        let mut d = Dash { replies: vec![reply("op-u")], posts: vec![], at: 0, cut: Some((t.install.join("projects"), String::new())) };
        exchange(&t.install, &active(), &mut d, 399).unwrap();
        assert_eq!((receipt_of(&t, "op-u")["reason"].as_str(), config(&t)), (Some("scope"), before.clone()));
        // the locked path drops the ID with the registration; registering the root again never revives it
        fs::write(t.install.join("projects"), format!("{}\n", t.root.display())).unwrap();
        unregister(&t.install, std::slice::from_ref(&t.root), || fs::write(t.install.join("projects"), "").map_err(|e| e.to_string())).unwrap();
        assert!(read(&t.install).unwrap().projects.is_empty());
        fs::write(t.install.join("projects"), format!("{}\n", t.root.display())).unwrap();
        go(&t, &[submit("op-v", json!({"runtime_limit": 0}))], &[], 399);
        assert_eq!((receipt_of(&t, "op-v")["reason"].as_str(), config(&t)), (Some("scope"), before));
        assert!(!read(&t.install).unwrap().projects.contains_key("project-1"));
        fs::remove_dir_all(&t.base).unwrap();
    }

    // H-A5: a failed registry commit cannot revive a retired scope on the next exchange.
    #[test]
    fn failed_registry_commit_keeps_scope_retired() {
        let t=setup("unregister-failed");
        let before=config(&t);
        let result=unregister(&t.install,std::slice::from_ref(&t.root),||Err("fixture registry write failed".into()));
        assert!(result.unwrap_err().contains("scope retired"));
        assert!(!read(&t.install).unwrap().projects.contains_key("project-1"));
        go(&t,&[submit("old-scope",json!({"runtime_limit":0}))],&[],100);
        assert_eq!(receipt_of(&t,"old-scope")["reason"],"scope");
        assert_eq!(config(&t),before);
        let state=read(&t.install).unwrap();
        assert!(!state.projects.contains_key("project-1"));
        assert!(state.grants.contains_key("grant-1") && state.ops.contains_key("old-scope"));
        fs::remove_dir_all(t.base).unwrap();
    }

    // H-A5: process death after scope retirement and before registry replacement cannot revive old grants.
    #[test]
    fn retired_scope_survives_process_death_before_registry_commit() {
        let t=setup("registry-crash");
        let registry=fs::read(t.install.join("projects")).unwrap();
        let signal=t.base.join("retired");
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","resources::tests::retire_fixture","--ignored"])
            .env("TASKQ_RESOURCE_RETIRE_FIXTURE",&t.base)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while !signal.exists() && child.try_wait().unwrap().is_none() && std::time::Instant::now()<until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let retired=fs::read_to_string(signal).unwrap_or_default();
        let _=child.kill();child.wait().unwrap();
        assert_eq!(retired,"retired","fixture did not reach the crash boundary");
        assert_eq!(fs::read(t.install.join("projects")).unwrap(),registry);
        assert!(!read(&t.install).unwrap().projects.contains_key("project-1"));
        go(&t,&[submit("after-crash",json!({"runtime_limit":0}))],&[],100);
        assert_eq!(receipt_of(&t,"after-crash")["reason"],"scope");
        fs::remove_dir_all(t.base).unwrap();
    }

    #[test]
    #[ignore="owned subprocess for retired_scope_survives_process_death_before_registry_commit"]
    fn retire_fixture() {
        let base=PathBuf::from(std::env::var_os("TASKQ_RESOURCE_RETIRE_FIXTURE").unwrap());
        unregister(&base.join("install"),&[base.join("proj")],|| {
            crate::platform::write_atomic(&base.join("retired"),"retired")?;
            loop {std::thread::park();}
        }).unwrap();
    }

    // H-A5: unknown membership is not an empty authoritative list and must not erase cached scopes.
    #[test]
    fn unknown_registry_preserves_resource_authority() {
        let t=setup("registry-unknown");
        let registry=fs::read(t.install.join("projects")).unwrap();
        let state=fs::read(dir(&t.install).join(FILE)).unwrap();
        for fault in ["missing","invalid","pending"] {
            match fault {
                "missing"=>fs::remove_file(t.install.join("projects")).unwrap(),
                "invalid"=>fs::write(t.install.join("projects"),b"\xff").unwrap(),
                _=>fs::write(t.install.join("projects.pending"),"interrupted").unwrap(),
            }
            assert!(report(&t.install,100).is_err(),"{fault} membership silently accepted");
            let mut wire=Dash {replies:vec![],posts:vec![],at:0,cut:None};
            assert!(exchange(&t.install,&active(),&mut wire,100).is_err());
            assert!(wire.posts.is_empty());
            assert_eq!(fs::read(dir(&t.install).join(FILE)).unwrap(),state,"{fault} destroyed cached authority");
            fs::write(t.install.join("projects"),&registry).unwrap();
            if fault=="pending" {fs::remove_file(t.install.join("projects.pending")).unwrap();}
        }
        fs::write(dir(&t.install).join("state.json.pending"),"interrupted retirement").unwrap();
        let mut wrote=false;
        assert!(unregister(&t.install,std::slice::from_ref(&t.root),|| {wrote=true;Ok(())}).is_err());
        assert!(!wrote,"unknown resource retirement cannot commit registry removal");
        assert_eq!(fs::read(dir(&t.install).join(FILE)).unwrap(),state);
        assert_eq!(fs::read(dir(&t.install).join("state.json.pending")).unwrap(),b"interrupted retirement");
        fs::remove_dir_all(t.base).unwrap();
    }

    /// DashQ stand-in for connection calls: pairing activates generation 2, a revoke succeeds, ingest answers 401.
    struct Peer;
    impl Wire for Peer {
        fn post(&mut self, _: &str, route: &str, _: Option<&str>, _: &Value) -> Result<(u16, Value), String> {
            Ok(match route {
                "/connect/device" => (200, json!({"protocol": 1, "device_code": "d", "user_code": "U-1", "verification_uri": "https://x/v", "expires_in": 600, "interval": 1})),
                "/connect/token" => (200, json!({"protocol": 1, "installation": INST, "product": "taskq", "credential": "c2", "generation": 2})),
                "/connect/ack" => (200, json!({"protocol": 1, "state": "active", "generation": 2})),
                "/ingest" => (401, json!({"protocol": 1, "error": "revoked"})),
                _ => (200, json!({"protocol": 1})),
            })
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    // Chain: decide's final connection check -> disconnect, pairing activation or DashQ's 401 revoke -> write.
    // Risk: the connection going between the final check and the write, so an operation writes after it went.
    #[test]
    fn connection_changes_wait_for_the_write() {
        let t = setup("final");
        let before = config(&t);
        let changes: [(&str, fn(&Path)); 3] = [
            ("op-d", |i| { enrollment::disconnect_with(i, &mut Peer).unwrap(); }),
            ("op-p", |i| {
                let req = coreq::enroll::Request { installation: Some(INST), product: "taskq", host_label: "h", schemas: &[enrollment::SCHEMA], scope: None, capabilities: &[] };
                coreq::enroll::pair(&enrollment::dir(i), "https://x/machine", &req, &mut Fenced::new(i, Peer)).unwrap();
            }),
            ("op-r", |i| {
                let rec = coreq::enroll::stamp(&enrollment::dir(i), enrollment::SCHEMA, "e-1", 0, json!({})).unwrap();
                assert!(coreq::enroll::deliver(&enrollment::dir(i), &[rec], &mut Default::default(), &mut Fenced::new(i, Peer)).is_err());
            }),
        ];
        for (op, change) in changes {
            fs::write(t.install.join("host.json"), &before).unwrap();
            fs::write(conn_file(&t.install), conn(Some(active())).to_string()).unwrap();
            with(&t.install, |s| sync(s, &t.install, 0)).unwrap();
            let seen = std::cell::RefCell::new(None);
            // the final read sees the operation's connection; the change starts right after it
            let live = || {
                let up = enrollment::active(&t.install).unwrap() == Some(active());
                let install = t.install.clone();
                seen.replace(Some(std::thread::spawn(move || { change(&install); fs::read_to_string(install.join("host.json")).unwrap() })));
                std::thread::sleep(std::time::Duration::from_millis(500));
                up
            };
            let fence = Fence { clock: &|| 100, live: &live };
            with(&t.install, |s| decide(&t.install, s, &submit(op, json!({"runtime_limit": 0})), (INST, 1), true, 100, &fence)).unwrap();
            let after = seen.take().expect("final check ran").join().unwrap();
            assert_eq!(receipt_of(&t, op)["state"], "saved", "{op}");
            assert_ne!(after, before, "{op}: the connection changed before the write landed");
            assert_ne!(enrollment::active(&t.install).unwrap(), Some(active()), "{op}: the change applied after the write");
        }
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: exchange reply -> bounds -> decide. Risk: an oversized, malformed or throttled reply applying anything,
    // or a host sending unbounded receipts.
    #[test]
    fn exchange_holds_v1_bounds() {
        let t = setup("bounds");
        let ops: Vec<Value> = (0..9).map(|i| submit(&format!("op-{i}"), json!({"runtime_limit": 0}))).collect();
        let reply = |ops: &[Value], extra: bool| {
            let mut r = json!({"schema": SCHEMA, "server_at": 100, "operations": ops, "cancellations": []});
            if extra { r["debug"] = json!(1) }
            r
        };
        for r in [(200, reply(&ops, false)), (200, reply(&ops[..1], true)), (200, json!({"schema": SCHEMA}))] {
            let mut d = Dash { replies: vec![r], posts: vec![], at: 0, cut: None };
            assert_eq!(exchange(&t.install, &active(), &mut d, 100).unwrap_err(), "exchange reply invalid");
        }
        let mut d = Dash { replies: vec![(429, json!({})), (403, json!({"reason": "revoked"}))], posts: vec![], at: 0, cut: None };
        assert_eq!(exchange(&t.install, &active(), &mut d, 100).unwrap(), (vec![], true), "busy: come back after the spacing");
        assert_eq!(exchange(&t.install, &active(), &mut d, 100).unwrap_err(), "exchange refused: http 403 revoked");
        assert!(read(&t.install).unwrap().ops.is_empty(), "nothing taken from a refused reply");
        let bad: Vec<Value> = (0..10).map(|i| { let mut o = submit(&format!("u-{i}"), json!({"runtime_limit": 0})); o["grant_id"] = json!("grant-x"); o }).collect();
        go(&t, &bad[..8], &[], 100);
        assert_eq!(go(&t, &bad[8..], &[], 100).0.len(), ITEMS, "at most 8 receipts per exchange");
        assert_eq!(go(&t, &[], &[], 101).0.len(), 2);
        fs::remove_dir_all(&t.base).unwrap();
    }

    // Chain: service resources --grant/--revoke -> report; sweep and pruning.
    // Risk: a grant naming an unknown or ambiguous project, an unbounded grant, or the store lost to cleanup.
    #[test]
    fn host_grants_resolve_projects_and_survive_cleanup() {
        let t = setup("grant");
        let fresh = t.base.join("fresh");
        let mut d = Dash { replies: vec![], posts: vec![], at: 0, cut: None };
        assert_eq!(exchange(&fresh, &active(), &mut d, 0).unwrap(), (vec![], false));
        assert!(d.posts.is_empty() && !dir(&fresh).exists(), "no grant ever made: no exchange, no store");
        let scope =|projects: &[&str], runtimes: &[&str], hours| Scope { host_policy: false, project_limits: false, projects: projects.iter().map(|x| x.to_string()).collect(), runtimes: runtimes.iter().map(|x| x.to_string()).collect(), hours };
        fs::write(conn_file(&t.install), conn(None).to_string()).unwrap();
        assert!(grant(&t.install, "g", "op", &scope(&["proj"], &[], 1), "me", 0).unwrap_err().contains("no active DashQ connection"));
        fs::write(conn_file(&t.install), conn(Some(active())).to_string()).unwrap();
        for (id, s, want) in [("g", scope(&[], &[], 1), "name a --project"), ("g", scope(&["proj"], &[], 0), "--hours"), ("g", scope(&["proj"], &[], MAX_HOURS + 1), "--hours"),
            ("g", scope(&["proj"], &["vim"], 1), "unknown runtime"), ("g", scope(&["nope"], &[], 1), "unknown project"), ("g/1", scope(&["proj"], &[], 1), "grant and actor IDs")] {
            assert!(grant(&t.install, id, "op", &s, "me", 0).unwrap_err().contains(want), "{want}");
        }
        let g = grant(&t.install, "g", "op", &scope(&["proj", "project-1"], &["codex"], 2), "me", 10).unwrap();
        assert_eq!((g.project_ids, g.runtime_ids, g.installation_id.as_str(), g.generation, g.expires_at), (vec!["project-1".to_string()], vec!["codex".to_string()], INST, 1, 10 + 7200));
        let r = report(&t.install, 20).unwrap();
        let revision = REVISION.with_borrow(Clone::clone);
        assert!(r.iter().any(|l| *l == format!("Project proj: project-1 revision {revision}")), "{r:?}");
        assert!(r.iter().any(|l| l.starts_with("Grant g: actor op generation 1, projects project-1, runtimes codex; expires")), "{r:?}");
        revoke(&t.install, "g").unwrap();
        assert!(report(&t.install, 20).unwrap().iter().any(|l| l == "Grant g: actor op generation 1, projects project-1, runtimes codex; revoked"));
        crate::platform::sweep(&t.install);
        crate::data::prune(&t.root, &t.install, "proj", &[]);
        assert!(read(&t.install).unwrap().grants.contains_key("g"), "the store survives sweep and pruning");
        // a corrupt store is kept and refuses everything
        fs::write(dir(&t.install).join(FILE), "{").unwrap();
        assert!(report(&t.install, 40).unwrap_err().contains("unreadable"));
        assert!(exchange(&t.install, &active(), &mut Dash { replies: vec![], posts: vec![], at: 0, cut: None }, 40).is_err());
        assert_eq!(fs::read_to_string(dir(&t.install).join(FILE)).unwrap(), "{");
        fs::remove_dir_all(&t.base).unwrap();
    }

    /// DashQ ingest stand-in: the C4 payload allowlist of dashq src/enroll.rs `metadata` and `field`, one receipt per
    /// record; `status` 401 is a revoke.
    struct Ingest { status: u16, bodies: Vec<Value> }
    impl Wire for Ingest {
        fn post(&mut self, dest: &str, route: &str, auth: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
            assert_eq!((dest, route, auth), ("https://x/machine", "/ingest", Some("c")));
            self.bodies.push(body.clone());
            if self.status != 200 { return Ok((self.status, json!({"protocol": 1, "error": "revoked"}))) }
            let receipts: Vec<Value> = body["records"].as_array().unwrap().iter().map(|r| match dashq_metadata(&r["payload"], 0) {
                true => json!({"event_id": r["event_id"], "state": "accepted"}),
                false => json!({"event_id": r["event_id"], "state": "rejected", "reason": "metadata"}),
            }).collect();
            Ok((200, json!({"protocol": 1, "receipts": receipts})))
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    fn dashq_metadata(v: &Value, depth: usize) -> bool {
        const DENY: [&str; 18] = ["prompt", "command", "cmd", "args", "stdout", "stderr", "output", "input", "content", "text", "message", "path", "cwd", "env", "token", "secret", "password", "credential"];
        let field = |k: &str| k.len() <= 64 && !DENY.iter().any(|d| k == *d || k.ends_with(&format!("_{d}")));
        match v {
            Value::Null | Value::Bool(_) | Value::Number(_) => depth > 0,
            Value::String(s) => depth > 0 && s.chars().count() <= 256 && !s.chars().any(char::is_control) && !s.starts_with(['/', '~', '\\']) && !s.get(1..3).is_some_and(|x| x == ":\\" || x == ":/"),
            Value::Array(a) => depth > 0 && depth < 6 && a.len() <= 256 && a.iter().all(|x| dashq_metadata(x, depth + 1)),
            Value::Object(o) => depth < 5 && o.len() <= 256 && o.iter().all(|(k, x)| field(k) && dashq_metadata(x, depth + 1)),
        }
    }

    // Chain #1299: host overlay + service status -> projection -> enrollment record -> coreq deliver -> DashQ ingest.
    // Risk: a record DashQ's allowlist refuses (blocker scope nests too deep), a path or credential in the payload, a
    // grant without the schema sending it, stale or unreadable readback shown as effective, or delivery after a revoke.
    #[test]
    fn enrollment_delivers_the_projection_dashq_accepts() {
        let t = setup("enrolled");
        fs::write(t.install.join("host.json"), json!({"limits": {"codex": 5}, "projects": {t.root.display().to_string(): {"limits": {"codex": 1}}}}).to_string()).unwrap();
        assert!(enrollment::resources_record(&t.install, &active(), 111).unwrap_err().contains("rerun taskq enroll"), "an old grant lacks the schema");
        let mut a = active();
        a["schemas"] = json!([enrollment::SCHEMA, enrollment::RESOURCES]);
        fs::write(conn_file(&t.install), conn(Some(a.clone())).to_string()).unwrap();
        let rev = projection(&t.install, 100, true).unwrap()["targets"][0]["current_revision"].clone();
        fs::create_dir_all(crate::dot(&t.root)).unwrap();
        fs::write(crate::dot(&t.root).join("status.json"), json!({"at": 110, "host": crate::host::Policy::read(&t.install).unwrap().me, "capacity": {"project_revision": rev, "project_limits": {"codex": 1}}}).to_string()).unwrap();
        let codex = |r: &Value| r["payload"]["targets"].as_array().unwrap().iter().find(|x| x["target"]["runtime_id"] == "codex").unwrap().clone();
        let deliver = |recs: &[Value], wire: &mut Ingest| coreq::enroll::deliver(&enrollment::dir(&t.install), recs, &mut Default::default(), wire);
        locked(&t.install, || Ok(assert!(enrollment::resources_record(&t.install, &a, 111).unwrap_err().contains("lock unavailable"), "the sender waits only a bounded time behind a writer"))).unwrap();
        // fresh: the effective revision and limit come from this host's status
        let (_, fresh) = enrollment::resources_record(&t.install, &a, 111).unwrap();
        assert_eq!((&codex(&fresh)["effective_revision"], &codex(&fresh)["effective_policy"]), (&rev, &json!({"runtime_limit": 1})));
        // stale: the exact fixture shape, effective unknown with the sample reason
        let (_, stale) = enrollment::resources_record(&t.install, &a, 171).unwrap();
        let mut want = fix()["enrollment_record"].clone();
        assert!(stale["event_id"].as_str().unwrap().starts_with("171-"));
        want["event_id"] = stale["event_id"].clone();
        want["payload"]["targets"][0]["current_revision"] = rev.clone();
        want["payload"]["targets"][0]["service_version"] = json!(crate::platform::VERSION);
        let mut got = stale.clone();
        got.as_object_mut().unwrap().remove("grant");
        got["payload"]["targets"] = json!([codex(&stale)]);
        assert_eq!(got, want);
        // unreadable taskq.json: a config blocker, nothing effective
        fs::write(t.root.join("taskq.json"), "{").unwrap();
        let (_, broken) = enrollment::resources_record(&t.install, &a, 111).unwrap();
        assert_eq!((&codex(&broken)["blockers"][0], &codex(&broken)["effective_revision"]), (&json!({"code": "config", "knowledge": "known"}), &Value::Null));
        let recs = [fresh, stale, broken];
        let mut dash = Ingest { status: 200, bodies: vec![] };
        let receipts = deliver(&recs, &mut dash).unwrap();
        assert!(receipts.iter().all(|r| r["state"] == "accepted"), "{receipts:?}");
        let sent = dash.bodies[0].to_string();
        assert!(!sent.contains(&t.base.display().to_string()) && !sent.contains("\"c\"") && !sent.contains("grant"), "no path, credential or grant leaves");
        // the projection as printed, blocker scope included, is what DashQ would refuse
        let mut raw = recs[1].clone();
        raw["payload"] = projection(&t.install, 171, true).unwrap();
        assert!(!dashq_metadata(&raw["payload"], 0));
        // revoked: coreq ends the connection; the next try is disconnected and sends nothing
        let mut revoked = Ingest { status: 401, bodies: vec![] };
        assert_eq!(deliver(&recs, &mut revoked), Err(coreq::enroll::Delivery::Stopped("revoked".into())));
        assert!(enrollment::active(&t.install).unwrap().is_none());
        assert_eq!(deliver(&recs, &mut revoked), Err(coreq::enroll::Delivery::Stopped("disconnected".into())));
        assert_eq!(revoked.bodies.len(), 1);
        assert!(enrollment::resources_record(&t.install, &a, 172).is_err(), "no record without a connection");
        fs::remove_dir_all(&t.base).unwrap();
    }
