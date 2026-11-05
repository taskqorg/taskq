    use super::*;
    use std::process::Command;
    use crate::board::Fake;
    use std::path::PathBuf;
    struct Fixture {dir:PathBuf,c:Config,b:Fake,n:u64}
    impl Drop for Fixture {fn drop(&mut self) {let _=fs::remove_dir_all(&self.dir);}}
    fn fixture()->Fixture {
        let dir=std::env::temp_dir().join(format!("taskq-recovery-{}",crate::runtime::uuid()));
        let root=dir.join("project");let install=dir.join("install");fs::create_dir_all(&root).unwrap();fs::create_dir_all(&install).unwrap();
        let root=root.canonicalize().unwrap();let install=install.canonicalize().unwrap();
        fs::write(root.join("taskq.json"),r#"{"repo":"invented/project"}"#).unwrap();
        fs::create_dir_all(crate::dot(&root)).unwrap();fs::write(crate::dot(&root).join("focus"),"all").unwrap();
        fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
        fs::write(install.join("hold"),json!({"taskq_stop":{"id":"invented-stop","instance":"invented-owner","roots":[root],"issued_at":1,"expires_at":301,"action":"stop","mode":"full"},"previous_hold":null}).to_string()).unwrap();
        let (mut c,_)=crate::load(&root).unwrap();c.install=install;c.me="mac".into();
        let b=Fake::default();let n=b.create("Invented recovery task","",&["taskq".into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:invented mac").unwrap();
        b.comment(n,&format!("taskq: accepted {a} attempt {a}")).unwrap();
        b.comment(n,&format!("taskq: released {a} exit attempt {a}")).unwrap();
        let path=root.join(".worktrees").join(format!("taskq-{n}"));fs::create_dir_all(&path).unwrap();
        for args in [vec!["init","-q"],vec!["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-q","--allow-empty","-m","Invented fixture"]] {
            assert!(Command::new("git").args(args).current_dir(&path).status().unwrap().success());
        }
        fs::write(path.join("untracked.txt"),"preserve invented untracked work").unwrap();
        Fixture{dir,c,b,n}
    }
    fn request(f:&Fixture)->Request {
        let r=row(&f.b,&f.c,f.n).unwrap();
        Request{stop:"invented-stop".into(),board:f.b.url(0),rows:vec![Selection{task:f.n,execution:r["execution"].as_u64().unwrap(),revision:r["revision"].as_u64().unwrap(),workspace:r["workspace"]["fingerprint"].as_str().unwrap().into(),effects_known:true,evidence:"Invented target readback confirms no remaining unknown effects".into()}]}
    }
    #[test]
    fn recovery_model_never_substitutes_owner_evidence_for_native_release() {
        let mut log=vec![core::Entry{id:1,body:"taskq: spawn worker codex:invented mac".into(),edited:false}];
        assert!(blocker(&core::project(1,&log,false),"mac").unwrap().contains("release"));
        let boot=core::Entry{id:2,body:"taskq: released 1 exit attempt 1\nhost booted 2026-10-19T08:00:00Z after intent 2026-10-13T10:00:00Z\nop-id: release-1".into(),edited:false};
        assert!(blocker(&core::project(1,&[log.clone(),vec![boot]].concat(),false),"mac").unwrap().contains("acceptance"),"#1386 boot release keeps acceptance unknown");
        log.push(core::Entry{id:2,body:"taskq: released 1 exit attempt 1".into(),edited:false});
        assert!(blocker(&core::project(1,&log,false),"mac").unwrap().contains("acceptance"));
        log.push(core::Entry{id:3,body:"taskq: accepted 1 attempt 1".into(),edited:false});
        assert_eq!(blocker(&core::project(1,&log,false),"mac"),None);
        assert!(blocker(&core::project(1,&log,false),"other").unwrap().contains("another host"));
        log.push(core::Entry{id:4,body:"taskq: ask attempt 1\nOwner decision".into(),edited:false});
        assert!(blocker(&core::project(1,&log,false),"mac").unwrap().contains("owner decision"));
    }
    #[test]
    #[cfg(target_os="macos")]
    fn recovery_selected_receipt_preserves_unselected_work_and_pause() {
        let f=fixture();let other=f.b.create("Unselected","",&["taskq".into()]).unwrap();
        let request=request(&f);
        assert!(!allowed(&f.c.install,&f.c.root,f.n));
        let result=apply(&f.b,&f.c,&request).unwrap();assert_eq!(result["receipts"][0]["outcome"],"accepted","{result}");
        let before=f.b.state.borrow().comments.len();
        assert!(allowed(&f.c.install,&f.c.root,f.n));assert!(!allowed(&f.c.install,&f.c.root,other));
        assert_eq!(apply(&f.b,&f.c,&request).unwrap()["receipts"],result["receipts"]);
        assert_eq!(f.b.state.borrow().comments.len(),before,"duplicate confirmation writes nothing");
        assert!(entries(&f.b,other).unwrap().is_empty());
        assert_eq!(fs::read_to_string(f.c.root.join(format!(".worktrees/taskq-{}/untracked.txt",f.n))).unwrap(),"preserve invented untracked work");
        let mut changed=request.clone();changed.rows[0].evidence="Changed payload".into();assert!(apply(&f.b,&f.c,&changed).is_err());
        fs::write(crate::dot(&f.c.root).join("focus"),"off").unwrap();
        assert!(!allowed(&f.c.install,&f.c.root,f.n),"paused projects must remain blocked after recovery");
    }
    #[test]
    #[cfg(target_os="macos")]
    fn recovery_service_admits_only_selected_and_keeps_pause_distinct_from_stop() {
        use std::{rc::Rc,cell::Cell};
        struct Proc(Rc<Cell<bool>>);
        impl crate::runtime::Proc for Proc {
            fn accepted(&mut self)->Res<bool>{Ok(true)}
            fn exited(&mut self)->Res<Option<i32>>{Ok(self.0.get().then_some(-9))}
            fn kill(&mut self)->Res<()>{self.0.set(true);Ok(())}
            fn id(&self)->u32{0}
            fn checkpoint(&mut self,_:&Config,_:&crate::runtime::Launch,_:u64)->Res<()>{Ok(())}
        }
        let mut f=fixture();f.c.limits=std::collections::BTreeMap::from([("claude".into(),2)]);
        let other=f.b.create("Unselected","",&["taskq".into()]).unwrap();let request=request(&f);
        assert_eq!(apply(&f.b,&f.c,&request).unwrap()["receipts"][0]["outcome"],"accepted");
        let stopped=Rc::new(Cell::new(false));let flag=stopped.clone();
        let mut service=crate::service::Service::new(&f.b,&f.c,"fixture",Box::new(move |_|Ok(Box::new(Proc(flag.clone())))));
        service.focus=vec!["all".into()];service.pass().unwrap();
        assert_eq!(service.kids.iter().map(|k|k.task).collect::<Vec<_>>(),vec![f.n]);
        assert!(entries(&f.b,other).unwrap().is_empty(),"unselected task has no spawn or mutation");
        fs::write(crate::dot(&f.c.root).join("focus"),"off").unwrap();service.full_stop_owned();
        assert!(!stopped.get(),"Pause must not reapply the earlier Full stop to recovered work");
        let mut hold=read_hold(&f.c.install).unwrap();hold.as_object_mut().unwrap().remove("recovery");save(&f.c,&hold).unwrap();
        service.full_stop_owned();assert!(stopped.get(),"new Stop revokes selected admission and cancels owned work");
        assert!(service.kids.len()==1,"native cancellation alone does not release board ownership");
    }

    #[test]
    #[cfg(target_os="macos")]
    fn recovery_supervision_requires_stop_exit_and_never_replays_start() {
        let f=fixture();let request=request(&f);
        assert_eq!(apply(&f.b,&f.c,&request).unwrap()["receipts"][0]["outcome"],"accepted");
        let calls=std::cell::Cell::new(0);
        let install=||{calls.set(calls.get()+1);Err("invented lost install response".into())};
        let receipt=json!({"id":"invented-stop","state":"observed","native_exit":true});
        assert!(resume_service_with(&f.c,"invented-stop",&json!({"state":"unknown"}),&receipt,install).is_err());
        assert_eq!(calls.get(),0);
        assert!(resume_service_with(&f.c,"invented-stop",&json!({"state":"exited"}),&receipt,install).is_err());
        assert_eq!(calls.get(),1);
        assert!(resume_service_with(&f.c,"invented-stop",&json!({"state":"exited"}),&receipt,install).is_err());
        assert_eq!(calls.get(),1,"unknown external installation must not be replayed");
        assert_eq!(resume_service_with(&f.c,"invented-stop",&json!({"state":"running","fresh":true}),&receipt,install).unwrap()["state"],"running");
        assert_eq!(calls.get(),1);
    }

    #[test]
    #[cfg(target_os="macos")]
    fn recovery_ready_and_lost_response_reconcile_without_duplicate_writes() {
        let f=fixture();f.b.comment(f.n,"taskq: requeue").unwrap();let request=request(&f);
        f.b.state.borrow_mut().lose_comment_response=true;
        let result=apply(&f.b,&f.c,&request).unwrap();assert_eq!(result["receipts"][0]["outcome"],"unknown");assert!(!allowed(&f.c.install,&f.c.root,f.n));
        f.b.state.borrow_mut().lose_comment_response=false;
        let result=apply(&f.b,&f.c,&request).unwrap();assert_eq!(result["receipts"][0]["outcome"],"accepted","{result}");
        let log=entries(&f.b,f.n).unwrap();assert_eq!(log.iter().filter(|e|e.body.starts_with("taskq: later")).count(),1);
        assert_eq!(log.iter().filter(|e|core::op_id(f.n,&e.body)==Some(op_id(&request.stop,&request.rows[0]).as_str())).count(),1);
    }
    #[test]
    #[cfg(target_os="macos")]
    fn recovery_lost_requeue_never_overrides_later_owner_intent() {
        let f=fixture();let request=request(&f);
        f.b.state.borrow_mut().lose_comment_response=true;
        assert_eq!(apply(&f.b,&f.c,&request).unwrap()["receipts"][0]["outcome"],"unknown");
        f.b.state.borrow_mut().lose_comment_response=false;
        f.b.comment(f.n,"taskq: later\nReason: Owner parked after recovery").unwrap();
        let count=f.b.state.borrow().comments.len();
        assert_eq!(apply(&f.b,&f.c,&request).unwrap()["receipts"][0]["outcome"],"unknown");
        assert!(!allowed(&f.c.install,&f.c.root,f.n));
        assert_eq!(f.b.state.borrow().comments.len(),count,"later owner intent cannot be replayed away");
    }
    #[test]
    #[cfg(target_os="macos")]
    fn recovery_refuses_unknown_changed_and_paused_without_writing() {
        for case in ["unknown","revision","work","pause","release"] {
            let f=fixture();let mut request=request(&f);
            match case {
                "unknown"=>request.rows[0].effects_known=false,
                "revision"=>{f.b.comment(f.n,"taskq: later\nReason: Changed owner intent").unwrap();},
                "work"=>{fs::write(f.c.root.join(format!(".worktrees/taskq-{}/new.txt",f.n)),"new").unwrap();},
                "pause"=>{fs::write(crate::dot(&f.c.root).join("focus"),"off").unwrap();},
                _=>{f.b.state.borrow_mut().comments.retain(|(_,e)|!e.body.starts_with("taskq: released"));},
            }
            let count=f.b.state.borrow().comments.len();
            let result=apply(&f.b,&f.c,&request);assert!(result.as_ref().map_or(true,|r|r["receipts"][0]["outcome"]!="accepted"),"{case}");
            assert!(!allowed(&f.c.install,&f.c.root,f.n));assert_eq!(f.b.state.borrow().comments.len(),count,"{case}");
        }
    }
