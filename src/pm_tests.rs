    use super::*;
    use std::path::PathBuf;
    struct Fixture {base:PathBuf,install:PathBuf,root:PathBuf,active:Value,r:Value,now:u64}
    impl Fixture {
        fn new()->Self {
            let base=std::env::temp_dir().join(format!("taskq-pm-{}",crate::runtime::uuid()));
            let (install,root)=(base.join("install"),base.join("project"));fs::create_dir_all(&root).unwrap();fs::create_dir_all(&install).unwrap();
            let root=root.canonicalize().unwrap();
            fs::write(root.join("taskq.json"),r#"{"repo":"invented/test-board","limits":{}}"#).unwrap();
            fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            let (pid,p)=projects(&install,now).unwrap().pop_first().unwrap();
            let mut r:Value=serde_json::from_str(include_str!("../tests/fixtures/pm-requests.json")).unwrap();r=r["request"].clone();
            r["target"]["project_id"]=json!(pid);r["revision"]=json!(p.revision);r["issued_at"]=json!(now);r["expires_at"]=json!(now+300);
            let active=json!({"installation":r["target"]["installation_id"],"generation":1,"destination":"https://invented.example/machine","credential":"invented"});
            fs::create_dir_all(enrollment::dir(&install)).unwrap();
            fs::write(enrollment::dir(&install).join("connection.json"),json!({"installation":active["installation"],"active":active,"pending":null,"revoke":[]}).to_string()).unwrap();
            with(&install,|s| {
                s.grants.insert("invented-grant".into(),Grant{actor_id:"invented-owner".into(),installation_id:"invented-installation".into(),generation:1,projects:vec![pid],runtimes:vec![crate::runtime::OWNER.into()],expires_at:now+3600,revoked:false});save(&install,s)
            }).unwrap();Self{base,install,root,active,r,now}
        }
        fn receipts(&self)->Vec<Value>{report(&self.install,self.now).unwrap()["receipts"].as_array().unwrap().clone()}
    }
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.base);}}
    struct Dash {requests:Vec<Value>,posts:Vec<Value>,at:u64,offline:bool,cut:Option<PathBuf>}
    impl Wire for Dash {
        fn post(&mut self,dest:&str,route:&str,auth:Option<&str>,body:&Value)->Res<(u16,Value)> {
            assert_eq!((dest,route,auth),("https://invented.example/machine",EXCHANGE,Some("invented")));
            self.posts.push(body.clone());
            if let Some(install)=self.cut.take(){assert!(resources::try_locked(&install,||Ok(())).is_err(),"connection writes must be fenced during disclosure");fs::write(enrollment::dir(&install).join("connection.json"),json!({"installation":"different","active":null,"pending":null,"revoke":[]}).to_string()).unwrap();}
            if self.offline{return Err("invented offline".into());}
            Ok((200,json!({"schema":SCHEMA,"server_at":self.at,"requests":self.requests})))
        }
        fn now(&self)->u64{self.at}
        fn sleep(&mut self,_:u64){}
        fn show(&mut self,_:&str,_:&str){}
    }
    fn native(_: &Path,_:&setup::Request)->setup::Launched {setup::Launched::Job{job:"invented-job".into(),session:Some("invented-session".into()),url:Some("https://claude.ai/code/session_invented".into())}}
    fn never(_: &Path,_:&setup::Request)->setup::Launched {panic!("PM launched twice or without authority")}
    // #1615 A2-A5: one fixture across request -> exchange -> setup -> native readback; no real native/board effects.
    #[test]
    fn fixtures_dispatch_once_and_keep_content_out_of_exchange() {
        let f:Value=serde_json::from_str(include_str!("../tests/fixtures/pm-requests.json")).unwrap();
        for case in f["cases"].as_array().unwrap() {
            let t=Fixture::new();let mut r=t.r.clone();
            match case["id"].as_str().unwrap(){"denied"=>r["grant_id"]=json!("missing"),"stale"=>r["revision"]=json!("stale"),"expired"=>{r["issued_at"]=json!(t.now-301);r["expires_at"]=json!(t.now-1);},_=>{}}
            let mut dash=Dash{requests:vec![r.clone()],posts:vec![],at:t.now,offline:false,cut:None};
            exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
            if case["outcome"]=="accepted" {
                setup::step(&t.install,native).unwrap();setup::step(&t.install,never).unwrap();
                exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
                let receipt=t.receipts().pop().unwrap();assert_eq!(receipt["native"]["session"],"invented-session");assert!(receipt["native"]["host"].is_string());
                let mut changed=r.clone();changed["text"]=json!("changed input");dash.requests=vec![changed];assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());
                with(&t.install,|s|{s.grants.get_mut("invented-grant").unwrap().projects.clear();save(&t.install,s)}).unwrap();
                let posts=dash.posts.len();dash.requests.clear();exchange(&t.install,&t.active,&mut dash,t.now).unwrap();assert_eq!(dash.posts.len(),posts,"narrowing a grant must withhold old native session identities");
            }else if case["id"]=="interrupted" {
                let id=t.receipts()[0]["request_id"].as_str().unwrap().to_owned();fs::remove_file(setup::dir(&t.install).join(format!("{id}.json"))).unwrap();
                exchange(&t.install,&t.active,&mut dash,t.now).unwrap();assert!(setup::all(&t.install).unwrap().0.is_empty());
            }else{assert!(setup::all(&t.install).unwrap().0.is_empty());}
            assert_eq!(t.receipts()[0]["outcome"],case["outcome"],"{}",case["id"]);
            if case["outcome"]=="rejected"||case["outcome"]=="stale" {
                dash.requests.clear();exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
                assert_eq!(dash.posts.last().unwrap()["receipts"][0]["outcome"],case["outcome"],"denial must reach its original connection");
            }
            let sent=serde_json::to_string(&dash.posts).unwrap();assert!(!sent.contains(r["text"].as_str().unwrap()));assert!(!sent.contains(&t.root.display().to_string()));
            assert!(!fs::read_to_string(folder(&t.install).join("state")).unwrap().contains(r["text"].as_str().unwrap()),"PM metadata cannot become a conversation store");
        }
    }
    // R9/R12: revoked/expired grants, corrupt state, disconnected and offline delivery never launch.
    #[test]
    fn outage_revocation_and_corrupt_state_preserve_identity() {
        let t=Fixture::new();let mut dash=Dash{requests:vec![t.r.clone()],posts:vec![],at:t.now,offline:true,cut:None};
        assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());assert!(setup::all(&t.install).unwrap().0.is_empty());
        dash.offline=false;exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
        with(&t.install,|s|{s.grants.get_mut("invented-grant").unwrap().revoked=true;save(&t.install,s)}).unwrap();
        setup::step(&t.install,never).unwrap();assert_eq!(setup::all(&t.install).unwrap().0[0].state,"failed");
        setup::open(&t.install,&t.root,"local request survives remote denial").unwrap();setup::step(&t.install,native).unwrap();
        let path=folder(&t.install).join("state");fs::write(&path,"corrupt").unwrap();assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());assert_eq!(fs::read_to_string(path).unwrap(),"corrupt");
        let t=Fixture::new();dash.cut=Some(t.install.clone());assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());assert!(setup::all(&t.install).unwrap().0.is_empty());
    }
    // #1615: input validation cannot poison later operations; connection renewal cannot inherit a pending intent.
    #[test]
    fn malformed_runtime_replacement_connection_and_corruption_stay_fenced() {
        let t=Fixture::new();let mut bad=t.r.clone();bad["target"]["runtime_id"]=json!("../invalid");
        let mut dash=Dash{requests:vec![bad],posts:vec![],at:t.now,offline:false,cut:None};
        assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());assert!(t.receipts().is_empty());
        dash.requests=vec![t.r.clone()];exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
        let mut active=t.active.clone();active["generation"]=json!(2);
        resources::locked(&t.install,||{
            fs::write(enrollment::dir(&t.install).join("connection.json"),json!({"installation":active["installation"],"active":active,"pending":null,"revoke":[]}).to_string()).unwrap();
            with(&t.install,|s|{s.grants.get_mut("invented-grant").unwrap().generation=2;save(&t.install,s)})
        }).unwrap();
        setup::step(&t.install,never).unwrap();assert_eq!(setup::all(&t.install).unwrap().0[0].state,"failed");
        let posted=dash.posts.len();assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());assert_eq!(dash.posts.len(),posted,"stale connection never discloses receipts");
        let t=Fixture::new();dash.requests=vec![t.r.clone()];exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
        fs::write(folder(&t.install).join("state"),"corrupt").unwrap();
        setup::open(&t.install,&t.root,"local request after corrupt PM metadata").unwrap();setup::step(&t.install,never).unwrap();
        assert_eq!(setup::all(&t.install).unwrap().0[0].state,"interrupted");setup::step(&t.install,native).unwrap();
        assert_eq!(setup::all(&t.install).unwrap().0[1].state,"launched");
        let t=Fixture::new();with(&t.install,|s|{s.grants.get_mut("invented-grant").unwrap().actor_id="invented private content".into();save(&t.install,s)}).unwrap();
        let posted=dash.posts.len();assert!(exchange(&t.install,&t.active,&mut dash,t.now).is_err());assert_eq!(dash.posts.len(),posted,"corrupt grant text must never enter exchange metadata");
    }
    // R12: interrupted durable prepares cannot authorize launch from older metadata or block unrelated local work.
    #[test]
    fn retained_prepares_fence_remote_launch_only() {
        for setup_prepare in [false,true] {
            let t=Fixture::new();let mut dash=Dash{requests:vec![t.r.clone()],posts:vec![],at:t.now,offline:false,cut:None};
            exchange(&t.install,&t.active,&mut dash,t.now).unwrap();
            let id=t.receipts()[0]["request_id"].as_str().unwrap().to_string();
            let pending=if setup_prepare {setup::dir(&t.install).join(format!("{id}.json.pending"))}else{folder(&t.install).join("state.pending")};
            fs::write(&pending,"interrupted prepare").unwrap();
            setup::open(&t.install,&t.root,"independent local request").unwrap();
            if !setup_prepare {setup::step(&t.install,never).unwrap();}
            setup::step(&t.install,native).unwrap();assert_eq!(fs::read_to_string(pending).unwrap(),"interrupted prepare");
            assert_eq!(setup::all(&t.install).unwrap().0.iter().find(|r|!r.pm).unwrap().state,"launched");
        }
    }
    // A4: observe real captured PM methods before asserting that no request/brief content entered telemetry.
    // #1659: a parallel test's first `pm.exchange` registration could store `never` after `diagnose` rebuilt
    // interest (tracing-core), so capture missed it on Windows CI; a warm-up call registers PM callsites first.
    #[test]
    fn captured_exchange_never_exports_owner_text() {
        crate::tests::with_test_capture(|store,_| {
            let warm=Fixture::new();exchange(&warm.install,&warm.active,&mut Dash{requests:vec![],posts:vec![],at:warm.now,offline:false,cut:None},warm.now).unwrap();
            let t=Fixture::new();let mut dash=Dash{requests:vec![t.r.clone()],posts:vec![],at:t.now,offline:false,cut:None};
            let capture=tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().unwrap().clone());
            capture.diagnose("pm.",std::time::Duration::from_secs(60),256).unwrap();
            exchange(&t.install,&t.active,&mut dash,t.now).unwrap();setup::step(&t.install,native).unwrap();
            assert!(crate::telemetry::settle(std::time::Duration::from_secs(30)));
            capture.flush().unwrap();
            let payload=store.export(0,0,1024).unwrap().to_string();
            assert!(payload.contains("pm.exchange"),"privacy proof needs observed PM capture");
            assert!(!payload.contains(t.r["text"].as_str().unwrap()),"owner text escaped governed setup storage");
        });
    }
