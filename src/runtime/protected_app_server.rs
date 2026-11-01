//! R8/R9: one protected native app-server launch on an existing external slot, never a grant authority.
use crate::{Res, policy, resources, runtime};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, io::Read, path::{Path, PathBuf}};

const SCHEMA:&str="taskq.app-server/1";
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Request {schema:String,slot:Value,policy_revision:u64,account:String,#[serde(default)] preparation:String}
impl Request {
    fn parse(text:&str)->Res<Self> {
        if text.len()>16384 {return Err("app-server request exceeds 16384 bytes".into());}
        let r:Self=serde_json::from_str(text).map_err(|_|"invalid protected app-server request")?;
        if r.schema!=SCHEMA||r.policy_revision==0||!runtime::accounts::valid_label(&r.account)
            ||r.slot["binding"]["runtime"]!="codex" {return Err("invalid protected app-server identity".into());}
        Ok(r)
    }
    fn root(&self)->Res<PathBuf> {
        let root=PathBuf::from(self.slot["binding"]["root"].as_str().ok_or("canonical slot project required")?);
        if !root.is_absolute()||canonical(&root)?!=root {return Err("canonical slot project required".into());}Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Chain: R8 R9 R12 R13 #2151 A1/A2 — a launch receipt cannot create its own expected authority.
    // Risk: a missing independent preparation is accepted before native dispatch.
    #[test]
    fn protected_unprepared_launch_refuses_before_intent() {
        let r=Request{schema:SCHEMA.into(),slot:json!({"binding":{"runtime":"codex"}}),policy_revision:29,account:"default".into(),preparation:String::new()};
        let id=coreq::process::Identity{pid:42,started:"invented-creation".into()};
        let mut s=resources::State::default();
        s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{}}),receipt:json!({"occupied":true,"instance":"invented-host"}),expires_at:1,saved_at:None,writing:false,sent:true});
        let before=s.clone();
        assert!(consume(&mut s,"slot",&r,&json!({"invented":"authority"}),&id).is_err(),"missing independent preparation must refuse before launch intent");
        assert_eq!(s,before);
    }
    fn model()->(resources::State,Request,Value,coreq::process::Identity) {
        let authority=json!({"project":"invented-project","saved":{"revision":29,"grant":{"by":"invented-operator","mode":"workspace"}},"account":"default","account_home":"invented-home","executable_sha256":"invented-native","profile_sha256":"invented-profile","adapter":"invented-adapter"});
        let mut r=Request{schema:SCHEMA.into(),slot:json!({"binding":{"caller":"invented-consumer","op_id":"invented-effect","runtime":"codex","pid":17,"started":"invented-consumer-start","boot":10}}),policy_revision:29,account:"default".into(),preparation:String::new()};
        let pre=preparation(&r,&authority,&json!("invented-host"));r.preparation=digest(pre.to_string().as_bytes());
        let mut s=resources::State::default();
        s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{"binding":r.slot["binding"],"key_hash":"invented-private-hash","prepared":pre}}),receipt:json!({"occupied":true,"instance":"invented-host"}),expires_at:1,saved_at:None,writing:false,sent:true});
        (s,r,authority,coreq::process::Identity{pid:42,started:"invented-supervisor-start".into()})
    }
    // Chain: R8 R9 R12 R13 #2151 A1/A2/A5 — preparation is independent, immutable and bound to caller/artifacts/grant.
    // Risk: prepare/launch drift or a copied launch receipt supplies its own authority.
    #[test]
    fn protected_preparation_model_drift_and_recovery() {
        let (s,r,authority,id)=model();
        for field in ["project","saved","account","account_home","executable_sha256","profile_sha256","adapter"] {
            let mut changed=authority.clone();changed[field]=json!("changed-or-revoked");let mut trial=s.clone();let before=trial.clone();
            assert!(consume(&mut trial,"slot",&r,&changed,&id).is_err(),"{field}");assert_eq!(trial,before);
        }
        for field in ["pid","started","boot","op_id"] {
            let mut changed=r.clone();changed.slot["binding"][field]=json!("changed-incarnation");
            assert!(consume(&mut s.clone(),"slot",&changed,&authority,&id).is_err(),"{field}");
        }
        let mut changed=s.clone();changed.ops.get_mut("slot").unwrap().receipt["instance"]=json!("new-host");
        assert!(consume(&mut changed,"slot",&r,&authority,&id).is_err());
        let mut started=s.clone();consume(&mut started,"slot",&r,&authority,&id).unwrap();
        let restored:resources::State=serde_json::from_str(&serde_json::to_string(&started).unwrap()).unwrap();
        for state in ["unknown","verified-effective","cancelled","completed"] {
            let mut trial=restored.clone();trial.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["state"]=json!(state);let before=trial.clone();
            assert!(consume(&mut trial,"slot",&r,&authority,&id).is_err());assert_eq!(trial,before);
            assert!(release_ready(&trial.ops["slot"]).is_err());
        }
    }
    // Chain: R8 R9 R12 R13 #2151 A3/A4/A5 — cancellation/root exit is not descendant completion or slot release.
    // Risk: crashes, PID reuse, reordered observations or a cleanup intent release uncertain work.
    #[test]
    fn protected_completion_model_survivors_crashes_and_no_replay() {
        let (mut s,r,a,id)=model();consume(&mut s,"slot",&r,&a,&id).unwrap();
        assert!(release_ready(&s.ops["slot"]).is_err());
        cancel(&mut s,"slot",&r).unwrap();assert!(release_ready(&s.ops["slot"]).is_err());
        let binding=json!({"boot":10,"coalition":101});
        let launch=&mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"];
        launch["native"]=json!({"pid":43,"started":"invented-native-start"});
        launch["input_closed"]=json!(true);launch["scope"]=json!({"binding":binding,"dispatch_ack":true,"cleanup_intent":false,"cleanup_verified":false});
        // Native root exited; two surviving detached descendants plus the sealed sentinel remain.
        assert!(!certify(&mut s,"slot",&r,&id,&binding,[9,6]).unwrap());assert!(release_ready(&s.ops["slot"]).is_err());
        let before=s.clone();
        for (supervisor,binding,counts) in [(coreq::process::Identity{pid:42,started:"reused-pid".into()},binding.clone(),[9,8]),(id.clone(),json!({"boot":11,"coalition":101}),[9,8]),(id.clone(),binding.clone(),[8,8]),(id.clone(),binding.clone(),[9,10])] {
            assert!(certify(&mut s,"slot",&r,&supervisor,&binding,counts).is_err());assert_eq!(s,before);
        }
        s=serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(certify(&mut s,"slot",&r,&id,&binding,[9,8]).unwrap());
        let mut late_input=s.clone();late_input.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["cancel_requested"]=json!(false);
        assert!(input_gate(&late_input,"slot",&r).is_err(),"a sealed input cannot reopen after cancellation changes");
        let certified=s.clone();assert!(certify(&mut s,"slot",&r,&id,&binding,[9,8]).unwrap());assert_eq!(s,certified);
        assert!(certify(&mut s,"slot",&r,&id,&binding,[9,7]).is_err());assert_eq!(s,certified);
        assert!(certify(&mut s,"slot",&r,&id,&binding,[10,9]).is_err());assert_eq!(s,certified);
        assert!(release_ready(&s.ops["slot"]).is_err(),"native certificate alone is not cleanup acknowledgment");
        s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["cleanup_intent"]=json!(true);
        assert!(release_ready(&s.ops["slot"]).is_err(),"crash before/after bootout without acknowledgment stays occupied");
        s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["bootout_ack"]=json!(true);
        assert!(release_ready(&s.ops["slot"]).is_err(),"acknowledged bootout still needs exact supervisor exit");
        s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["scope"]["cleanup_verified"]=json!(true);
        release_ready(&s.ops["slot"]).unwrap();
        assert_eq!(s.ops["slot"].bound["local_slot"]["binding"],r.slot["binding"]);
        assert_eq!(s.ops["slot"].bound["local_slot"]["key_hash"],"invented-private-hash");
        assert!(consume(&mut s,"slot",&r,&a,&id).is_err());
        for field in ["native","supervisor","input_closed","scope"] {
            let mut corrupt=s.clone();corrupt.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"][field]=Value::Null;
            assert!(release_ready(&corrupt.ops["slot"]).is_err(),"{field}");
        }
    }
    // Chain: R8 R9 R12 #2151 A2/A5 — output descriptors cannot read input before authority validation.
    // Risk: a duplex native stdout bypasses the supervisor's input gate.
    #[cfg(unix)]
    #[test]
    fn protected_native_output_cannot_read_ungated_input() {
        use std::{io::Write,os::{fd::AsRawFd,unix::net::UnixStream},time::Duration};
        let (mut consumer,mut supervisor)=UnixStream::pair().unwrap();
        consumer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        supervisor.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        consumer.write_all(b"ungated native input").unwrap();
        let (output,pump)=native_output(supervisor.try_clone().unwrap()).unwrap();
        let mut output=fs::File::from(output);
        let mut bytes=[0u8;20];
        let read=unsafe{libc::read(output.as_raw_fd(),bytes.as_mut_ptr().cast(),bytes.len())};
        assert_eq!(read,-1,"native output must not read queued, ungated input");
        assert_eq!(std::io::Error::last_os_error().raw_os_error(),Some(libc::EBADF));
        supervisor.read_exact(&mut bytes).unwrap();assert_eq!(&bytes,b"ungated native input");
        output.write_all(b"native output").unwrap();drop(output);
        let mut received=[0u8;13];consumer.read_exact(&mut received).unwrap();assert_eq!(&received,b"native output");
        assert_eq!(pump.join().unwrap().unwrap(),13);
    }
    struct Fixture {dir:PathBuf,root:PathBuf,home:PathBuf,bin:PathBuf,r:Request,p:policy::Policy}
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.dir);}}
    impl Fixture {
        fn new()->Self {
            let dir=std::env::temp_dir().join(format!("taskq-protected-{}",runtime::uuid()));fs::create_dir(&dir).unwrap();let dir=canonical(&dir).unwrap();
            let root=dir.join("project");let home=dir.join("home");let bin=dir.join("bin/codex");
            fs::create_dir(bin.parent().unwrap()).unwrap();
            fs::create_dir(&root).unwrap();fs::create_dir_all(home.join(".codex")).unwrap();fs::write(&bin,b"invented native fixture").unwrap();
            fs::write(dir.join("taskq"),b"invented launcher").unwrap();
            fs::write(home.join(".codex/config.toml"),b"# invented config").unwrap();
            fs::write(dir.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(dir.join("host.json"),json!({"codex":["-s","danger-full-access","-m","invented-model","-c","model_reasoning_effort=high"]}).to_string()).unwrap();
            let p=policy::Policy{revision:29,grants:vec![policy::Grant{runtime:"codex".into(),mode:"workspace".into(),project:Some(root.to_str().unwrap().into()),by:"invented-operator".into(),at:1}]};
            let r=Request{schema:SCHEMA.into(),slot:json!({"binding":{"root":root,"runtime":"codex","caller":"invented-consumer","op_id":"invented-operation"}}),policy_revision:29,account:"default".into(),preparation:String::new()};
            let f=Self{dir,root,home,bin,r,p};f.save();f
        }
        fn save(&self){fs::write(self.dir.join(policy::FILE),serde_json::to_vec(&self.p).unwrap()).unwrap();}
        fn plan(&self)->Res<Plan>{resolve(&self.dir,&self.home,&self.r,"macos",&self.bin)}
    }
    // Chain: R8 R9 A1-A3: a request is a match requirement; missing, host-wide, widened or stale authority cannot launch.
    // Risk: accepting a copied receipt, task text or default grant as protected project authority.
    #[test]
    fn protected_authority_refuses_missing_tampered_stale_and_unknown() {
        let f=Fixture::new();let p=f.plan().unwrap();
        assert_eq!(p.authority["saved"]["grant"]["by"],"invented-operator");
        assert_eq!(p.authority["account_home"],json!(f.home.join(".codex")));
        assert!(p.args.contains(&"model=\"invented-model\"".to_string()));assert!(p.args.contains(&"model_reasoning_effort=high".to_string()));
        assert!(p.access.profile.as_ref().unwrap().contains("deny file-write"));
        for case in ["missing","corrupt","full","host","revision","approver","duplicate","account","quota","quota-shape","path","windows","linux"] {
            let mut f=Fixture::new();let os=match case {"windows"=>"windows","linux"=>"linux",_=>"macos"};
            match case {
                "missing"=>f.p.grants.clear(),"full"=>f.p.grants[0].mode="full".into(),"host"=>f.p.grants[0].project=None,
                "revision"=>f.r.policy_revision+=1,"approver"=>f.p.grants[0].by="unknown".into(),
                "duplicate"=>f.p.grants.push(f.p.grants[0].clone()),"account"=>f.r.account="foreign".into(),
                "path"=>f.r.slot["binding"]["root"]=json!(f.root.join("missing")),
                "quota"=>fs::write(f.dir.join(runtime::accounts::STATE),"unreadable limits").unwrap(),
                "quota-shape"=>fs::write(f.dir.join(runtime::accounts::STATE),"[]").unwrap(),_=>{},
            }
            f.save();if case=="corrupt" {fs::write(f.dir.join(policy::FILE),"receipt text is not a grant").unwrap();}
            assert!(resolve(&f.dir,&f.home,&f.r,os,&f.bin).is_err(),"{case}");
        }
        let mut f=Fixture::new();fs::write(f.dir.join("accounts.json"),"unreadable authority").unwrap();assert!(f.plan().is_err());
        fs::remove_file(f.dir.join("accounts.json")).unwrap();f.p.grants.clear();f.save();assert!(f.plan().is_err());
        let text=json!({"schema":SCHEMA,"slot":{"binding":{"runtime":"codex"}},"policy_revision":29,"account":"default","grant":"workspace"}).to_string();
        assert!(Request::parse(&text).is_err());assert!(Request::parse(&"x".repeat(16385)).is_err());
    }
    // Chain: R8 R9 R13 #2135 A1/A2: the existing Workspace/network request resolves without widening authority.
    // Risk: rejecting approved host config, losing model/effort/account, or forwarding an unsupported override.
    #[test]
    fn protected_workspace_network_host_request_retains_authority_and_config() {
        let f=Fixture::new();
        let args=["-s","workspace-write","-c","sandbox_workspace_write.network_access=true"];
        fs::write(f.dir.join("host.json"),json!({"codex":args}).to_string()).unwrap();
        let p=f.plan().unwrap_or_else(|e|panic!("existing host Workspace/network request must resolve: {e}"));
        assert_eq!(p.args,["-c","sandbox_mode=\"danger-full-access\"","-c","approval_policy=\"never\"","-c","sandbox_workspace_write.network_access=true"]);
        assert_eq!(p.access.mode,"workspace");assert!(p.access.profile.as_ref().unwrap().contains("deny file-write"));
        assert_eq!(p.authority["requested"],args.join(" "));
        assert_eq!(p.authority["account"],"default");assert_eq!(p.authority["account_home"],json!(f.home.join(".codex")));
        let profile=p.authority["profile_sha256"].clone();
        fs::write(f.dir.join("host.json"),json!({"codex":["--sandbox","workspace-write","--config","sandbox_workspace_write.network_access=true","--model","invented-model","--config","model_reasoning_effort=high"]}).to_string()).unwrap();
        let p=f.plan().unwrap();
        assert_eq!(p.authority["profile_sha256"],profile);
        assert_eq!(p.args,["-c","sandbox_mode=\"danger-full-access\"","-c","approval_policy=\"never\"","--config","sandbox_workspace_write.network_access=true","-c","model=\"invented-model\"","--config","model_reasoning_effort=high"]);
        for config in ["sandbox_workspace_write.network_access=false","sandbox_workspace_write.network_access=true.extra","sandbox_workspace_write.network_access=\"true\"","sandbox_workspace_write.writable_roots=[]","sandbox_mode=\"danger-full-access\"","approval_policy=\"never\"","unknown=true"] {
            fs::write(f.dir.join("host.json"),json!({"codex":["-s","workspace-write","-c",config]}).to_string()).unwrap();
            assert!(f.plan().is_err(),"unsupported config must refuse: {config}");
        }
        for args in [vec!["-s","read-only"],vec!["-c"],vec!["--unknown"],vec!["--add-dir","/invented"]] {
            fs::write(f.dir.join("host.json"),json!({"codex":args}).to_string()).unwrap();assert!(f.plan().is_err(),"{args:?}");
        }
    }
    // Chain: R8 R12 #2135 A2: query errors and permission mismatches remain closed and attributable without private paths.
    // Risk: treating unknown enforcement as allowed or hiding the native query needed to diagnose containment.
    #[test]
    fn protected_sandbox_refusal_names_query_and_result_without_paths() {
        for (result,errno,denied,observed,expected) in [
            (-1,Some(22),true,"unknown","denied"),
            (0,None,true,"allowed","denied"),
            (1,None,false,"denied","allowed"),
        ] {
            let error=sandbox_result("file-write-create","account-config",result,errno,denied).unwrap_err();
            assert_eq!(error,format!("native sandbox readback: operation=file-write-create category=account-config expected={expected} observed={observed} return={result} errno={}; input refused",errno.map_or("none".into(),|v|v.to_string())));
            assert!(!error.contains('/')&&!error.contains('\\'),"private paths must not reach diagnostics");
        }
        assert!(sandbox_result("file-write-data","workspace-probe",0,None,false).is_ok());
        assert!(sandbox_result("file-write-unlink","policy-store",1,None,true).is_ok());
        assert!(sandbox_result("file-write-unlink","policy-store",2,None,true).is_ok());
        assert!(sandbox_result("file-write-data","workspace-probe",-1,Some(3),false).is_err());
    }
    // Chain: R8 #2135 A2: absent paths can report denial even when creation is allowed on macOS.
    // Risk: rejecting the workspace or accepting nonexistent outside markers as containment evidence.
    #[test]
    fn protected_sandbox_queries_use_existing_trusted_objects() {
        let f=Fixture::new();let p=f.plan().unwrap();
        let paths=query_paths(&f.dir,&p);
        assert_eq!(paths.len(),6);
        assert_eq!(paths[0],("workspace-probe",f.root.clone(),false));
        assert_eq!(paths[5],("outside-workspace",f.home.clone(),true));
        for (category,path,_) in &paths {assert!(path.exists(),"{category} must already exist");}
        assert_eq!(p.authority["sandbox_paths"],json!(p.queries));
        let mut calls=vec![];
        for q in &p.queries {for &op in if q.denied {&["file-write-data","file-write-create","file-write-unlink"][..]}else{&["file-write-data"][..]} {
            q.check(op,|path| {assert!(path.exists());calls.push((q.category,op,q.denied));Ok((i32::from(q.denied),None))}).unwrap();
        }}
        assert_eq!(calls.len(),16);assert_eq!(calls[0],("workspace-probe","file-write-data",false));
        assert!(calls[1..].iter().all(|(_,_,denied)|*denied));
        let account=f.home.join(".codex").join(runtime::accounts::HOMES).join("invented-account");
        fs::create_dir_all(&account).unwrap();fs::write(account.join("config.toml"),b"# invented account config").unwrap();
        fs::write(f.dir.join("accounts.json"),r#"{"codex":[{"label":"invented-account","enabled":true}]}"#).unwrap();
        let mut r=f.r.clone();r.account="invented-account".into();
        let p=resolve(&f.dir,&f.home,&r,"macos",&f.bin).unwrap();
        assert_eq!(p.queries[4].path,account.join("config.toml"));
    }
    // Chain: R8 R9 #2135 A2: unknown/missing/replaced query objects close both allowed and denied observations.
    // Risk: treating ENOENT as enforcement or accepting a changed path before input delivery.
    #[test]
    fn protected_sandbox_query_identities_refuse_missing_changed_and_racing_paths() {
        for index in 1..5 {
            let f=Fixture::new();let p=f.plan().unwrap();let q=&p.queries[index];
            fs::remove_file(&q.path).unwrap();fs::create_dir(&q.path).unwrap();
            assert!(f.plan().is_err(),"a directory is not a required control file: {}",q.category);
        }
        for index in 0..6 {
            let f=Fixture::new();let p=f.plan().unwrap();let q=&p.queries[index];
            if q.path.is_dir() {fs::remove_dir_all(&q.path).unwrap();}else{fs::remove_file(&q.path).unwrap();}
            let error=q.check("file-write-data",|_|panic!("a missing object must not reach the kernel query")).unwrap_err();
            assert!(error.contains(q.category)&&error.contains("unknown-or-changed"));
            assert!(!error.contains('/')&&!error.contains('\\'));
            assert!(f.plan().is_err(),"a missing object must also refuse before launch");
        }
        for result in [0,1] {
            let f=Fixture::new();let p=f.plan().unwrap();let q=&p.queries[4];
            assert!(q.check("file-write-data",|path|{fs::write(path,b"# changed invented config").unwrap();Ok((result,None))}).is_err());
            assert_ne!(f.plan().unwrap().authority,p.authority,"config changes must invalidate original launch authority");
        }
        let f=Fixture::new();let p=f.plan().unwrap();let q=&p.queries[0];
        fs::write(f.root.join("ordinary-workspace-output"),b"invented output").unwrap();
        q.check("file-write-data",|_|Ok((0,None))).unwrap();
        assert_eq!(f.plan().unwrap().authority,p.authority,"directory content changes preserve identity");
        #[cfg(unix)] {
            let original=f.dir.join("original-project");fs::rename(&f.root,&original).unwrap();fs::create_dir(&f.root).unwrap();
            assert!(q.check("file-write-data",|_|panic!("replacement must refuse")).is_err());
            fs::remove_dir(&f.root).unwrap();std::os::unix::fs::symlink(&original,&f.root).unwrap();
            assert!(q.check("file-write-data",|_|panic!("retargeted alias must refuse")).is_err());
        }
    }
    // R9/R12 A2/A7: intent precedes spawn; interrupted/unknown/terminal records all consume the same launch.
    // Risk: restarting a supervisor replays an AssetQ operation or a stale PID is treated as live authority.
    #[test]
    fn protected_interruption_never_replays_and_unknown_identity_refuses() {
        let mut f=Fixture::new();let p=f.plan().unwrap();let id=coreq::process::Identity{pid:42,started:"invented-creation".into()};
        let mut s=resources::State::default();s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{}}),receipt:json!({"occupied":true,"instance":"invented-host"}),expires_at:1,saved_at:None,writing:false,sent:true});
        let pre=preparation(&f.r,&p.authority,&s.ops["slot"].receipt["instance"]);
        f.r.preparation=digest(pre.to_string().as_bytes());s.ops.get_mut("slot").unwrap().bound["local_slot"]["prepared"]=pre;
        consume(&mut s,"slot",&f.r,&p.authority,&id).unwrap();
        input_gate(&s,"slot",&f.r).unwrap();
        let original=s.clone();
        let effective=json!({"state":"verified-effective","scope":"invented-native-observation"});
        assert!(bound_gate(&s,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).is_err());
        let launch=&mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"];
        launch["native"]=json!({"pid":id.pid,"started":id.started});launch["state"]=json!("verified-effective");launch["verified_effective"]=effective.clone();
        bound_gate(&s,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).unwrap();
        let verified=s.clone();
        for field in ["authority","native","supervisor","state","verified_effective","cancel_requested","host_instance"] {
            let mut changed=verified.clone();changed.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"][field]=json!("tampered-or-unknown");
            assert!(bound_gate(&changed,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).is_err(),"{field}");
        }
        for field in ["executable_sha256","adapter"] {
            let mut changed=verified.clone();changed.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["authority"][field]=json!("tampered-or-unknown");
            assert!(bound_gate(&changed,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).is_err(),"{field}");
        }
        s=original.clone();
        for state in ["unknown","verified-effective","exited","cancelled"] {
            s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["state"]=json!(state);
            let before=s.clone();assert!(consume(&mut s,"slot",&f.r,&p.authority,&id).is_err());assert_eq!(s,before);
        }
        assert_eq!(original.ops["slot"].bound["local_slot"]["launch"]["state"],"unknown");
        assert!(exact(&id,Err("native observation denied".into())).is_err());assert!(exact(&id,Ok(None)).is_err());
        assert!(exact(&id,Ok(Some(coreq::process::Identity{pid:42,started:"reused-pid".into()}))).is_err());assert!(exact(&id,Ok(Some(id.clone()))).is_ok());
        assert!(native(&json!({"pid":u64::MAX,"started":"invented"})).is_err());
        let mut changed=f.r.clone();changed.policy_revision+=1;assert!(launch_record(&s,"slot",&changed).is_err());
        s=original.clone();cancel(&mut s,"slot",&f.r).unwrap();assert!(input_gate(&s,"slot",&f.r).is_err());
        assert!(native(&s.ops["slot"].bound["local_slot"]["launch"]["native"]).is_err(),"pre-spawn cancellation remains durable without a guessed child");
        assert!(consume(&mut s,"slot",&f.r,&p.authority,&id).is_err());
        s=original.clone();s.ops.get_mut("slot").unwrap().receipt["instance"]=json!("restarted-host");assert!(input_gate(&s,"slot",&f.r).is_err());
        resources::within(&f.dir,resources::WAIT,|stored|{*stored=s.clone();Ok(())}).unwrap();
        resources::within(&f.dir,resources::WAIT,|stored|{assert!(consume(stored,"slot",&f.r,&p.authority,&id).is_err());Ok(())}).unwrap();
        fs::write(resources::dir(&f.dir).join("state.json.pending"),"interrupted save").unwrap();
        assert!(resources::within(&f.dir,resources::WAIT,|_|Ok(())).is_err());
    }
    // R9 A3: the canonical grant writer and launch input gate cannot race a revocation.
    #[test]
    fn protected_grant_writer_shares_the_input_authority_lock() {
        let f=Fixture::new();let held=resources::acquire(&f.dir,resources::WAIT).unwrap();let before=fs::read(f.dir.join(policy::FILE)).unwrap();
        assert!(policy::grant(&f.dir,"codex","none",Some(&f.root),"invented-operator",2).is_err());
        assert_eq!(fs::read(f.dir.join(policy::FILE)).unwrap(),before);drop(held);
        policy::grant(&f.dir,"codex","none",Some(&f.root),"invented-operator",2).unwrap();assert!(f.plan().is_err());
    }
    // Chain: R11 §8 — update retention includes the external adapter until whole-turn release, even if launch is unknown.
    #[test]
    fn protected_unknown_launch_retains_its_release_until_slot_release() {
        let f=Fixture::new();fs::create_dir(f.root.join(".taskq")).unwrap();
        fs::write(f.root.join(".taskq/status.json"),json!({"version":crate::platform::VERSION}).to_string()).unwrap();
        resources::within(&f.dir,resources::WAIT,|s| {
            s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{"launch":{"authority":{"adapter":{"version":"invented-retained"}}}}}),
                receipt:json!({"occupied":true}),expires_at:1,saved_at:None,writing:false,sent:true});Ok(())
        }).unwrap();
        assert!(crate::platform::releases_in_use(&f.dir).unwrap().contains(&"invented-retained".into()));
        resources::within(&f.dir,resources::WAIT,|s|{s.ops.get_mut("slot").unwrap().receipt["occupied"]=json!(false);Ok(())}).unwrap();
        assert!(!crate::platform::releases_in_use(&f.dir).unwrap().contains(&"invented-retained".into()));
        fs::write(resources::dir(&f.dir).join("state.json.pending"),"interrupted save").unwrap();
        assert!(crate::platform::releases_in_use(&f.dir).is_none(),"uncertain ownership retains every release");
    }
}
fn canonical(p:&Path)->Res<PathBuf> {p.canonicalize().map_err(|_|"protected launch path identity unknown".into())}
fn digest(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn slot_id(r:&Request)->Res<String> {
    let b=&r.slot["binding"];
    Ok(format!("slot-{}",digest(format!("{}\0{}",b["caller"].as_str().ok_or("slot caller unknown")?,b["op_id"].as_str().ok_or("slot operation unknown")?).as_bytes())))
}
fn scope_directory(install:&Path,r:&Request)->Res<PathBuf> {
    // A short socket name under the existing protected install; collisions refuse creation, never replace a scope.
    Ok(install.join("native-scopes").join(&slot_id(r)?[5..37]))
}
fn preparation(r:&Request,authority:&Value,instance:&Value)->Value {
    json!({"schema":"taskq.app-server-preparation/1","binding":r.slot["binding"],"authority":authority,"host_instance":instance})
}
fn prepared(s:&resources::State,key:&str,r:&Request,authority:&Value)->Res<()> {
    let op=s.ops.get(key).ok_or("slot absent; preparation unknown")?;
    let expected=preparation(r,authority,&op.receipt["instance"]);
    if r.preparation.is_empty()||op.bound["local_slot"]["prepared"]!=expected
        ||r.preparation!=digest(expected.to_string().as_bytes()) {
        return Err("independent preparation missing, stale or changed; no native dispatch/input".into());
    }Ok(())
}

fn certified(launch:&Value)->Res<()> {
    let scope=&launch["scope"];let cert=&scope["certificate"];
    if launch["input_closed"]!=true||scope["dispatch_ack"]!=true
        ||cert["schema"]!="taskq.native-completion/1"||cert["scope"]!=scope["binding"]
        ||cert["native"]!=launch["native"]||cert["supervisor"]!=launch["supervisor"]
        ||cert["counts"][0].as_u64().zip(cert["counts"][1].as_u64()).is_none_or(|(a,b)|a.checked_sub(b)!=Some(1))
        ||native(&cert["supervisor"]).is_err()||native(&cert["native"]).is_err()
        ||scope["binding"]["coalition"].as_u64().is_none_or(|v|v==0)||scope["binding"]["boot"].as_i64().is_none_or(|v|v<=0) {
        return Err("protected descendant completion unproved; retain original slot".into());
    }Ok(())
}
/// R9/R12: an authenticated consumer still cannot release unproved protected descendants.
pub(crate) fn release_ready(op:&resources::Op)->Res<()> {
    let launch=&op.bound["local_slot"]["launch"];
    if launch.is_null() {return Ok(());}
    certified(launch)?;
    if op.bound["local_slot"]["binding"]["boot"].as_i64().zip(launch["scope"]["binding"]["boot"].as_i64()).is_none_or(|(a,b)|a<=0||a.abs_diff(b)>2) {
        return Err("protected slot/scope boot binding unknown; retain original slot".into());
    }
    if launch["scope"]["cleanup_intent"]!=true||launch["scope"]["bootout_ack"]!=true||launch["scope"]["cleanup_verified"]!=true {
        return Err("protected scope cleanup unproved; retain original slot".into());
    }Ok(())
}
fn certify(s:&mut resources::State,key:&str,r:&Request,supervisor:&coreq::process::Identity,binding:&Value,counts:[u64;2])->Res<bool> {
    launch_record(s,key,r)?;
    let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
    let mine=json!({"pid":supervisor.pid,"started":supervisor.started});
    if launch["supervisor"]!=mine||launch["scope"]["binding"]!=*binding||launch["input_closed"]!=true
        ||launch["scope"]["dispatch_ack"]!=true||counts[0].checked_sub(counts[1]).is_none() {
        return Err("scope identity/input seal unknown; completion refused".into());
    }
    let old=&launch["scope"]["last_counts"];
    if !old.is_null()&&(old[0].as_u64().is_none_or(|n|n>counts[0])||old[1].as_u64().is_none_or(|n|n>counts[1])) {
        return Err("scope counters regressed; completion unknown".into());
    }
    if !launch["scope"]["certificate"].is_null() {
        if launch["scope"]["certificate"]["counts"]!=json!(counts) {return Err("sealed scope changed after certification; retain original receipt".into());}
        return Ok(true);
    }
    if counts[0]-counts[1]==1 {native(&launch["native"])?;}
    launch["scope"]["last_counts"]=json!(counts);
    if counts[0]-counts[1]!=1 {return Ok(false);}
    launch["scope"]["certificate"]=json!({"schema":"taskq.native-completion/1","scope":binding,"supervisor":mine,"native":launch["native"],"counts":counts});
    Ok(true)
}
fn file_digest(path:&Path)->Res<String> {
    let mut file=fs::File::open(path).map_err(|_|"protected executable unreadable")?;
    let mut hash=ring::digest::Context::new(&ring::digest::SHA256);let mut bytes=[0;65536];
    loop {let n=file.read(&mut bytes).map_err(|_|"protected executable read unknown")?;if n==0 {break;}hash.update(&bytes[..n]);}
    Ok(hash.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect())
}
fn stamp(path:&Path)->Res<Value> {
    let m=fs::metadata(path).map_err(|_|"protected executable identity unknown")?;
    if !m.is_file() {return Err("protected executable is not a file".into());}
    let modified=m.modified().map_err(|_|"protected executable clock unknown")?;
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;return Ok(json!([m.dev(),m.ino(),m.ctime(),m.ctime_nsec(),m.len(),format!("{modified:?}")]));}
    #[cfg(not(unix))] {Ok(json!([m.len(),format!("{modified:?}"),format!("{:?}",m.created().map_err(|_|"protected executable creation unknown")?)]))}
}

struct Plan {authority:Value,access:policy::Effective,args:Vec<String>,root:PathBuf,home:PathBuf,bin:PathBuf,queries:Vec<SandboxQuery>}
fn query_paths(install:&Path,p:&Plan)->[(&'static str,PathBuf,bool);6] {
    [("workspace-probe",p.root.clone(),false),
        ("policy-store",install.join(policy::FILE),true),("taskq-launcher",install.join("taskq"),true),
        ("native-executable",p.bin.clone(),true),("account-config",Path::new(p.authority["account_home"].as_str().unwrap()).join("config.toml"),true),
        ("outside-workspace",p.home.clone(),true)]
}
#[derive(serde::Serialize)]
struct SandboxQuery {category:&'static str,path:PathBuf,denied:bool,identity:Value}
fn query_identity(path:&Path)->Res<Value> {
    let real=canonical(path)?;let m=fs::metadata(path).map_err(|_|"sandbox path identity unknown")?;
    if !m.is_file()&&!m.is_dir() {return Err("sandbox path is not a file or directory".into());}
    #[cfg(unix)] let object={use std::os::unix::fs::MetadataExt;json!([m.dev(),m.ino()])};
    // Windows is refused by resolve; this branch only supports portable fixtures.
    #[cfg(not(unix))] let object=json!(format!("{:?}",m.created().map_err(|_|"sandbox path creation unknown")?));
    Ok(json!({"canonical":real,"directory":m.is_dir(),"object":object,"file_stamp":if m.is_file(){stamp(path)?}else{Value::Null}}))
}
impl SandboxQuery {
    fn path_error(&self,operation:&str)->String {
        format!("native sandbox path readback: operation={operation} category={} observed=unknown-or-changed; input refused",self.category)
    }
    fn unchanged(&self,operation:&str)->Res<()> {
        if query_identity(&self.path).ok().as_ref()!=Some(&self.identity) {return Err(self.path_error(operation));}Ok(())
    }
    #[cfg(any(test,target_os="macos"))]
    fn check(&self,operation:&str,query:impl FnOnce(&Path)->Res<(i32,Option<i32>)>)->Res<()> {
        self.unchanged(operation)?;
        let (result,errno)=query(&self.path)?;
        self.unchanged(operation)?;
        sandbox_result(operation,self.category,result,errno,self.denied)
    }
}
// Resolve only host inputs. The request's revision/account are equality requirements, never grants.
fn resolve(install:&Path,home:&Path,r:&Request,os:&str,bin:&Path)->Res<Plan> {
    if os!="macos" {return Err("protected app-server supports qualified macOS Codex only; no Full fallback".into());}
    let root=r.root()?;let install=canonical(install)?;let home=canonical(home)?;let bin=canonical(bin)?;
    if !crate::host::projects(&install)?.contains(&root) {return Err("protected project is not registered".into());}
    let p=policy::load(&install)?;
    let grants:Vec<_>=p.grants.iter().filter(|g|g.runtime=="codex"&&g.project.as_deref()==root.to_str()).collect();
    let g=grants.first().filter(|_|grants.len()==1).ok_or("explicit project Codex Workspace grant required")?;
    if p.revision!=r.policy_revision||g.mode!="workspace"||g.by.trim().is_empty()||g.by=="unknown"||g.at==0 {
        return Err("project Workspace authority missing, stale or mismatched".into());
    }
    let host=crate::host::Policy::read(&install)?;
    let accounts=runtime::accounts::Host::read(&install);
    if let Some(state)=crate::data::json(&install.join(runtime::accounts::STATE))? {
        for limits in state.as_object().ok_or("host account limits unknown")?.values() {
            for limit in limits.as_object().ok_or("host account limits unknown")?.values() {
                if limit["limited"].as_i64().is_none()||(!limit["until"].is_null()&&limit["until"].as_i64().is_none()) {
                    return Err("host account limits unknown; no account fallback".into());
                }
            }
        }
    }
    if accounts.error.is_some()||accounts.active("codex",runtime::accounts::now()).as_deref()!=Some(&r.account) {
        return Err("selected host account unavailable or changed; no account fallback".into());
    }
    let base=home.join(".codex");
    let account=canonical(&if r.account==runtime::accounts::DEFAULT {base.clone()}else{base.join(runtime::accounts::HOMES).join(&r.account)})?;
    let mut access=policy::resolve(&Ok(p.clone()),&policy::Host{os,install:&install,root:&root,home:&home},"codex",&host.permission_mode,&host.codex);
    if access.refused.is_some()||access.mode!="workspace"||access.profile.is_none() {return Err("protected enforcement unknown; no native input".into());}
    let adapter=canonical(&std::env::current_exe().map_err(|_|"adapter executable unknown")?)?;
    // Staged qualification binaries need the same write protection as an installed launcher.
    for path in [&adapter,&bin] {
        let parent=path.parent().ok_or("protected executable parent absent")?;
        if root.starts_with(parent) {return Err("protected executable directory overlaps the writable project".into());}
        access.profile.as_mut().unwrap().push_str(&format!("(deny file-write* (subpath {}))\n",serde_json::to_string(&parent.to_string_lossy()).map_err(|_|"invalid executable path")?));
    }
    for path in [home.join(".local/bin"),home.join(".cargo/bin"),PathBuf::from("/opt/homebrew/bin"),PathBuf::from("/usr/local/bin")] {
        if path.is_dir() {access.profile.as_mut().unwrap().push_str(&format!("(deny file-write* (subpath {}))\n",serde_json::to_string(&canonical(&path)?.to_string_lossy()).map_err(|_|"invalid host executable directory")?));}
    }
    let control=scope_directory(&install,r)?;
    access.profile.as_mut().unwrap().push_str(&format!("(deny file* (subpath {}))\n",serde_json::to_string(&control).map_err(|_|"invalid scope control path")?));
    // App-server takes config options, not exec's sandbox/model switches. Preserve only known host requests.
    let mut args=vec!["-c".into(),"sandbox_mode=\"danger-full-access\"".into(),"-c".into(),"approval_policy=\"never\"".into()];
    let mut it=host.codex.iter();
    while let Some(a)=it.next() {match a.as_str() {
        "-s"|"--sandbox"=>{let v=it.next().ok_or("missing host sandbox request")?;if v!="danger-full-access"&&v!="workspace-write" {return Err("host sandbox narrowing is not qualified for app-server".into());}},
        "-m"|"--model"=>{let v=it.next().ok_or("missing host model")?;args.extend(["-c".into(),format!("model={}",serde_json::to_string(v).map_err(|_|"host model invalid")?)]);},
        "-c"|"--config"=>{let v=it.next().ok_or("missing host config request")?;
            if !v.starts_with("model_reasoning_effort=")&&v!="sandbox_workspace_write.network_access=true" {return Err("unsupported host app-server config; existing paths unchanged".into());}
            args.extend([a.clone(),v.clone()]);},
        _=>return Err("unsupported host app-server arguments; existing paths unchanged".into()),
    }}
    let authority=json!({"project":root,"runtime":"codex","profile_sha256":digest(access.profile.as_ref().unwrap().as_bytes()),
        "requested":access.requested,"saved":{"revision":p.revision,"grant":g},"resolved":access.json(),
        "executable":bin,"executable_identity":stamp(&bin)?,"executable_sha256":file_digest(&bin)?,
        "adapter":{"executable":adapter,"identity":stamp(&adapter)?,"sha256":file_digest(&adapter)?,"version":crate::platform::VERSION},
        "account":r.account,"account_home":account,"args":args,"scope_directory":control});
    let mut plan=Plan{authority,access,args,root,home,bin,queries:vec![]};
    for (category,path,denied) in query_paths(&install,&plan) {
        let mut q=SandboxQuery{category,path,denied,identity:Value::Null};
        q.identity=query_identity(&q.path).map_err(|_|q.path_error("bind"))?;
        if q.identity["directory"]!=json!(category=="workspace-probe"||category=="outside-workspace") {return Err(q.path_error("bind"));}
        plan.queries.push(q);
    }
    plan.authority["sandbox_paths"]=json!(plan.queries);
    Ok(plan)
}

fn launch_record<'a>(s:&'a resources::State,key:&str,r:&Request)->Res<&'a Value> {
    let launch=&s.ops.get(key).ok_or("slot absent; execution unknown")?.bound["local_slot"]["launch"];
    if launch["schema"]!=SCHEMA||launch["policy_revision"]!=r.policy_revision||launch["account"]!=r.account {
        return Err("launch identity absent, corrupt or mismatched; never retry launch".into());
    }Ok(launch)
}
fn consume(s:&mut resources::State,key:&str,r:&Request,authority:&Value,supervisor:&coreq::process::Identity)->Res<()> {
    prepared(s,key,r,authority)?;
    let op=s.ops.get_mut(key).ok_or("slot absent; no launch")?;
    if !op.bound["local_slot"]["launch"].is_null() {return Err("slot launch already consumed; observe the same operation, never replay".into());}
    op.bound["local_slot"]["launch"]=json!({"schema":SCHEMA,"policy_revision":r.policy_revision,"account":r.account,
        "authority":authority,"supervisor":{"pid":supervisor.pid,"started":supervisor.started},"state":"unknown",
        "host_instance":op.receipt["instance"],"cancel_requested":false,"input_closed":false,
        "reason":"launch intent consumed; native execution not yet verified"});
    Ok(())
}
fn input_gate(s:&resources::State,key:&str,r:&Request)->Res<()> {
    let launch=launch_record(s,key,r)?;
    if launch["cancel_requested"]!=false||launch["input_closed"]!=false||launch["host_instance"].as_str().filter(|v|!v.is_empty()).is_none()
        ||launch["host_instance"]!=s.ops[key].receipt["instance"] {
        return Err("launch cancelled or host generation changed; input refused, retain slot".into());
    }Ok(())
}
fn bound_gate(s:&resources::State,key:&str,r:&Request,authority:&Value,id:&coreq::process::Identity,supervisor:&coreq::process::Identity,effective:Option<&Value>)->Res<()> {
    input_gate(s,key,r)?;let record=launch_record(s,key,r)?;
    if record["authority"]!=*authority||native(&record["native"])?!=*id||native(&record["supervisor"])?!=*supervisor
        ||effective.is_some_and(|e|record["state"]!="verified-effective"||record["verified_effective"]!=*e) {
        return Err("durable launch binding changed or uncommitted; input refused".into());
    }Ok(())
}
fn cancel(s:&mut resources::State,key:&str,r:&Request)->Res<Value> {
    launch_record(s,key,r)?;
    let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
    launch["cancel_requested"]=json!(true);Ok(launch.clone())
}
fn native(value:&Value)->Res<coreq::process::Identity> {
    Ok(coreq::process::Identity{pid:value["pid"].as_u64().and_then(|v|u32::try_from(v).ok()).filter(|v|*v>0).ok_or("native identity unknown")?,
        started:value["started"].as_str().filter(|s|!s.is_empty()).ok_or("native identity unknown")?.into()})
}
fn exact(expected:&coreq::process::Identity,seen:Res<Option<coreq::process::Identity>>)->Res<()> {
    if seen?.as_ref()!=Some(expected) {return Err("native incarnation absent or changed; retain slot and reconcile".into());}Ok(())
}

#[cfg(any(test,target_os="macos"))]
fn sandbox_result(operation:&str,category:&str,result:i32,errno:Option<i32>,denied:bool)->Res<()> {
    if result<0||(result!=0)!=denied {
        let expected=if denied {"denied"}else{"allowed"};
        let observed=if result<0 {"unknown"}else if result==0 {"allowed"}else{"denied"};
        let errno=errno.map_or("none".into(),|v|v.to_string());
        return Err(format!("native sandbox readback: operation={operation} category={category} expected={expected} observed={observed} return={result} errno={errno}; input refused"));
    }Ok(())
}

#[cfg(any(target_os="macos",all(test,unix)))]
fn native_output(mut socket:std::os::unix::net::UnixStream)->Res<(std::os::fd::OwnedFd,std::thread::JoinHandle<std::io::Result<u64>>)> {
    let (mut reader,writer)=std::io::pipe().map_err(|_|"native output pipe unavailable; retain scope")?;
    // Native code must never inherit the duplex socket that carries input before validation.
    let pump=std::thread::Builder::new().spawn(move||std::io::copy(&mut reader,&mut socket))
        .map_err(|_|"native output forwarding unavailable; retain scope")?;
    Ok((writer.into(),pump))
}

pub(crate) fn versions_in_use(install:&Path)->Res<Vec<String>> {
    resources::try_with(install,|s| {
        s.ops.values().filter(|op|!op.bound["local_slot"]["launch"].is_null()&&op.receipt["occupied"]!=false)
            .map(|op|op.bound["local_slot"]["launch"]["authority"]["adapter"]["version"].as_str().filter(|v|!v.is_empty())
                .map(String::from).ok_or_else(||"external adapter release identity unknown".into())).collect()
    })
}

pub fn command(args:&crate::Args)->Res<()> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot launch or control external app servers".into());}
    if args.pos.len()!=2||args.flags.iter().any(|(k,v)|!["action","request"].contains(&k.as_str())||v.len()!=1) {
        return Err("usage: service app-server --action prepare|launch|observe|cancel|reconcile --request JSON".into());
    }
    #[cfg(target_os="macos")] {mac::dispatch(args.need("action")?,args.need("request")?)}
    #[cfg(not(target_os="macos"))] {Err("protected app-server supports qualified macOS Codex only; no Full fallback".into())}
}

#[cfg(target_os="macos")]
#[path="protected_scope.rs"]
mod scope;

#[cfg(target_os="macos")]
mod mac {
    use super::*;
    use crate::external_slots;
    use std::{ffi::{CStr,CString},io::{self,Write},os::{fd::AsRawFd,unix::ffi::OsStrExt},process::Stdio,time::{Duration,Instant}};

    // Sandbox SPI declarations match WebKit's Source/WTF/wtf/spi/darwin/SandboxSPI.h.
    #[link(name="sandbox")]
    unsafe extern "C" {fn sandbox_check(pid:i32,operation:*const libc::c_char,filter:i32,...)->i32;static SANDBOX_CHECK_NO_REPORT:i32;}

    pub(super) fn host_paths()->Res<(PathBuf,PathBuf)> {
        let mut pwd=std::mem::MaybeUninit::<libc::passwd>::uninit();let mut result=std::ptr::null_mut();let mut buf=vec![0u8;65536];
        let status=unsafe{libc::getpwuid_r(libc::geteuid(),pwd.as_mut_ptr(),buf.as_mut_ptr().cast(),buf.len(),&mut result)};
        if status!=0||result.is_null() {return Err("host account identity unknown".into());}
        let home=canonical(Path::new(std::ffi::OsStr::from_bytes(unsafe{CStr::from_ptr((*result).pw_dir)}.to_bytes())))?;
        let install=canonical(&home.join(".local/share/taskq"))?;
        for (name,expected) in [("HOME",&home),("TASKQ_INSTALL",&install),("CODEX_HOME",&home.join(".codex"))] {
            if let Some(value)=std::env::var_os(name) {if canonical(Path::new(&value))?!=canonical(expected)? {
                return Err("ambient host/account override cannot select authority".into());
            }}
        }
        Ok((install,home))
    }
    pub(super) fn executable(home:&Path)->Res<PathBuf> {
        // shortcut: standard host CLI locations; use host-owned runtime resolution if custom installations need support.
        let defaults=std::ffi::OsStr::new("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
        let path=runtime::user_bin_path(defaults,Some(home.as_os_str())).unwrap_or_else(||defaults.to_os_string());
        runtime::codex_native(runtime::which_in("codex",&path))
    }
    pub(super) fn live(id:&coreq::process::Identity)->Res<()> {exact(id,coreq::process::identity(id.pid).map_err(|_|"native liveness unknown".into()))}
    fn child_of(id:&coreq::process::Identity,supervisor:&coreq::process::Identity)->Res<()> {
        live(id)?;live(supervisor)?;
        let mut info=std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();let size=std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n=unsafe{libc::proc_pidinfo(id.pid as i32,libc::PROC_PIDTBSDINFO,0,info.as_mut_ptr().cast(),size)};
        if n!=size||unsafe{info.assume_init()}.pbi_ppid!=supervisor.pid {return Err("native child ownership unknown or mismatched; slot retained".into());}
        live(id)?;live(supervisor)
    }
    pub(super) fn image(id:&coreq::process::Identity,path:&Path)->Res<()> {
        live(id)?;let mut buf=[0u8;4096];
        let n=unsafe{libc::proc_pidpath(id.pid as i32,buf.as_mut_ptr().cast(),buf.len() as u32)};
        if n<=0 {return Err("native executable readback unknown".into());}
        let end=buf.iter().position(|b|*b==0).ok_or("native executable path incomplete")?;
        if canonical(Path::new(std::ffi::OsStr::from_bytes(&buf[..end])))?!=path {return Err("native executable has not matched the selected image".into());}
        live(id)
    }
    fn check(pid:u32,operation:&str,q:&SandboxQuery)->Res<()> {
        q.check(operation,|path| {
            let op=CString::new(operation).map_err(|_|"invalid sandbox operation")?;
            let path=CString::new(path.as_os_str().as_bytes()).map_err(|_|"invalid sandbox path")?;
            let result=unsafe{*libc::__error()=0;sandbox_check(pid as i32,op.as_ptr(),1|SANDBOX_CHECK_NO_REPORT,path.as_ptr())};
            let errno=if result<0 {io::Error::last_os_error().raw_os_error()}else{None};
            Ok((result,errno))
        })
    }
    fn verified(id:&coreq::process::Identity,p:&Plan,scope:&Value)->Res<Value> {
        image(id,&p.bin)?;
        for q in &p.queries {for &op in if q.denied {&["file-write-data","file-write-create","file-write-unlink"][..]}else{&["file-write-data"][..]} {check(id.pid,op,q)?;}}
        let control=SandboxQuery{category:"scope-control",path:PathBuf::from(p.authority["scope_directory"].as_str().ok_or("scope control unknown")?),denied:true,identity:scope["directory_identity"].clone()};
        for op in ["file-read-data","file-write-data","file-write-create","file-write-unlink"] {check(id.pid,op,&control)?;}
        scope::membership(id,&scope["binding"])?;
        image(id,&p.bin)?;
        let protected:Vec<_>=p.queries.iter().filter(|q|q.denied).map(|q|&q.path).collect();
        Ok(json!({"state":"verified-effective","scope":"live native image and named effective sandbox queries; native workload qualification separate",
            "workspace_write":true,"protected_write_create_unlink_denied":protected,"reads":"existing profile","network":"existing profile"}))
    }
    pub(super) fn refresh(install:&Path,home:&Path,r:&Request,original:&Value)->Res<Plan> {
        let bin=executable(home)?;let p=resolve(install,home,r,"macos",&bin)?;
        if p.authority!=*original {return Err("bound project/profile/account/executable authority changed; native input refused".into());}Ok(p)
    }
    fn key(install:&Path,r:&Request)->Res<String> {
        resources::within(install,resources::WAIT,|s|external_slots::reserved(install,s,&r.slot,"codex"))
    }
    pub(super) fn dispatch(action:&str,text:&str)->Res<()> {
        if action=="scope-run" {return scope::run(text);}
        if action=="prepare" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Prepare {schema:String,slot:Value}
            if text.len()>16384 {return Err("preparation request exceeds 16384 bytes".into());}
            let input:Prepare=serde_json::from_str(text).map_err(|_|"invalid preparation request")?;
            if input.schema!=SCHEMA {return Err("unsupported preparation schema".into());}
            let (install,home)=host_paths()?;
            let result=resources::within(&install,resources::WAIT,|s| {
                let account=runtime::accounts::Host::read(&install).active("codex",runtime::accounts::now()).ok_or("active account unknown")?;
                let r=Request{schema:input.schema.clone(),slot:input.slot.clone(),policy_revision:policy::load(&install)?.revision,account,preparation:String::new()};
                let key=external_slots::reserved(&install,s,&r.slot,"codex")?;
                let p=resolve(&install,&home,&r,"macos",&executable(&home)?)?;
                let op=s.ops.get_mut(&key).ok_or("slot absent")?;
                if !op.bound["local_slot"]["launch"].is_null() {return Err("launch already consumed; never derive expected authority from a launch receipt".into());}
                let value=preparation(&r,&p.authority,&op.receipt["instance"]);
                let old=&op.bound["local_slot"]["prepared"];
                if !old.is_null()&&old!=&value {return Err("original preparation changed; retain operation, no native launch".into());}
                op.bound["local_slot"]["prepared"]=value.clone();
                Ok(json!({"schema":SCHEMA,"state":"prepared","preparation":digest(value.to_string().as_bytes()),"policy_revision":r.policy_revision,"account":r.account,"prepared":value}))
            })?;println!("{result}");return Ok(());
        }
        command(action,&Request::parse(text)?)
    }
    pub(super) fn command(action:&str,r:&Request)->Res<()> {
        if !["launch","observe","cancel","reconcile"].contains(&action) {return Err("app-server action must be prepare, launch, observe, cancel or reconcile".into());}
        let (install,home)=host_paths()?;
        // Existing slot authentication, including exact payload/key, occurs before any native action.
        let observed=external_slots::exchange(&install,"observe",&r.slot)?;
        let slot_key=slot_id(r)?;let _=observed;
        if action=="launch" {return scope::launch(&install,&home,r,&slot_key);}
        if action=="reconcile" {let result=scope::reconcile(&install,r,&slot_key)?;println!("{result}");return Ok(());}
        let (record,completed)=resources::within(&install,resources::WAIT,|s| {
            let record=if action=="cancel" {cancel(s,&slot_key,r)?}else{launch_record(s,&slot_key,r)?.clone()};
            Ok((record,release_ready(s.ops.get(&slot_key).ok_or("slot absent")?).is_ok()))
        })?;
        let mut receipt=json!({"schema":SCHEMA,"operation":r.slot["binding"]["op_id"],"caller":r.slot["binding"]["caller"],"launch":record,"state":"unknown"});
        if completed {receipt["state"]=json!("completed");println!("{receipt}");return Ok(());}
        let id=match native(&record["native"]) {
            Ok(id)=>id,Err(reason)=>{receipt["reason"]=json!(if action=="cancel" {"cancellation saved before native identity; input remains closed".into()}else{reason});println!("{receipt}");return Ok(());}
        };
        if action=="cancel" {
            // Revocation closes input, but cannot remove the original owner's exact-process stop authority.
            child_of(&id,&native(&record["supervisor"])?)?;
            image(&id,Path::new(record["authority"]["executable"].as_str().ok_or("bound executable unknown")?))?;
            coreq::process::terminate(&id).map_err(|_|"native cancellation unconfirmed; retain slot and reconcile")?;
            receipt["reason"]=json!("exact native termination requested; turn/descendant completion unproved; slot retained");
        } else {
            let result=(||->Res<()> {
                key(&install,r)?;let p=refresh(&install,&home,r,&record["authority"])?;
                let supervisor=native(&record["supervisor"])?;live(&supervisor)?;
                child_of(&id,&supervisor)?;
                let effective=verified(&id,&p,&record["scope"])?;
                resources::within(&install,resources::WAIT,|s|bound_gate(s,&slot_key,r,&p.authority,&id,&supervisor,Some(&effective)))?;
                receipt["verified_effective"]=effective;
                if record["state"]!="verified-effective" {return Err("launch receipt uncommitted; execution unknown".into());}Ok(())})();
            match result {Ok(())=>receipt["state"]=json!("verified-effective"),Err(reason)=>receipt["reason"]=json!(reason)}
        }
        println!("{receipt}");Ok(())
    }
    pub(super) fn launch_native(install:&Path,home:&Path,r:&Request,key:&str,mut transport:std::io::BufReader<std::os::unix::net::UnixStream>,stdout:Stdio,stderr:Stdio)->Res<()> {
        let original=resources::within(install,resources::WAIT,|s|Ok(launch_record(s,key,r)?.clone()))?;
        let p=refresh(install,home,r,&original["authority"])?;
        let supervisor=coreq::process::identity(std::process::id()).map_err(|_|"supervisor identity unknown")?.ok_or("supervisor absent")?;
        resources::within(install,resources::WAIT,|s| {prepared(s,key,r,&p.authority)?;input_gate(s,key,r)})?;
        let exe=std::env::current_exe().map_err(|_|"adapter executable unknown")?;
        let env=runtime::Env{context:"",root:&p.root,exe:&exe,install,permission_mode:"",codex:&p.args,publish:"",board_env:&[],jobs:None};
        let mut cmd=runtime::executable(p.bin.clone(),&p.access,&env)?;
        // Neither authority secrets nor an ambient environment choose native account, configuration or executable.
        cmd.env_clear();for name in ["PATH","LANG","LC_ALL","TMPDIR","TERM"] {if let Some(v)=std::env::var_os(name) {cmd.env(name,v);}}
        cmd.env("HOME",home);
        let spec=runtime::optimized(agentq_runtime::agent::Spec{client:agentq_runtime::agent::Client::Codex,root:p.root.clone(),session:"-".into(),
            resume:false,args:p.args.iter().map(Into::into).collect(),home:Some(PathBuf::from(p.authority["account_home"].as_str().ok_or("account home unknown")?)),env:vec![]})?;
        let mut cmd=agentq_runtime::agent::app_server_command(cmd,&spec)?;
        cmd.stdin(Stdio::piped()).stdout(stdout).stderr(stderr);
        resources::within(install,resources::WAIT,|s|{external_slots::reserved(install,s,&r.slot,"codex")?;input_gate(s,key,r)})?;
        let mut child=cmd.spawn().map_err(|_|"native spawn outcome unknown; intent retained, never replay")?;
        let id=coreq::process::identity(child.id()).map_err(|_|"spawned native identity unknown; retain intent")?.ok_or("spawned native absent; retain intent")?;
        resources::within(install,resources::WAIT,|s| {
            launch_record(s,key,r)?;scope::membership(&id,&original["scope"]["binding"])?;
            let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
            launch["native"]=json!({"pid":id.pid,"started":id.started});Ok(())
        })?;
        let mut input=child.stdin.take().ok_or("native stdin unavailable; intent retained")?;
        let gate=(||->Res<()> {
            let until=Instant::now()+Duration::from_secs(5);
            loop {
                match image(&id,&p.bin) {Ok(())=>break,Err(e)=>{if Instant::now()>=until||child.try_wait().map_err(|_|"native liveness unknown")?.is_some() {return Err(e);}std::thread::sleep(Duration::from_millis(10));}}
            }
            resources::within(install,resources::WAIT,|s| {
                external_slots::reserved(install,s,&r.slot,"codex")?;refresh(install,home,r,&p.authority)?;
                bound_gate(s,key,r,&p.authority,&id,&supervisor,None)?;child_of(&id,&supervisor)?;
                let effective=verified(&id,&p,&original["scope"])?;
                let launch=&mut s.ops.get_mut(key).ok_or("slot disappeared")?.bound["local_slot"]["launch"];
                launch["verified_effective"]=effective;launch["state"]=json!("verified-effective");launch["reason"]=Value::Null;Ok(())
            })
        })();
        if let Err(e)=gate {drop(input);let _=coreq::process::terminate(&id);return Err(e);}
        let flags=unsafe{libc::fcntl(input.as_raw_fd(),libc::F_GETFL)};
        if flags<0||unsafe{libc::fcntl(input.as_raw_fd(),libc::F_SETFL,flags|libc::O_NONBLOCK)}<0 {return Err("bounded native input unavailable; intent retained".into());}
        transport.get_ref().set_read_timeout(Some(Duration::from_millis(250))).map_err(|_|"bounded scope input unavailable")?;
        loop {
            if let Some(status)=child.try_wait().map_err(|_|"native liveness unknown; slot retained")? {
                return if status.success(){Ok(())}else{Err("native app-server exited; turn outcome unproved, slot retained".into())};
            }
            resources::within(install,resources::WAIT,|s| {external_slots::reserved(install,s,&r.slot,"codex")?;input_gate(s,key,r)})?;
            let mut bytes=vec![0;65536];
            let count=match transport.read(&mut bytes) {
                Ok(n)=>n,Err(e) if matches!(e.kind(),io::ErrorKind::WouldBlock|io::ErrorKind::TimedOut|io::ErrorKind::Interrupted)=>continue,
                Err(_)=>return Err("consumer input interrupted; retain native scope".into()),
            };
            if count==0 {drop(input);return Err("consumer input ended; retain native scope until descendants finish".into());}
            bytes.truncate(count);
            resources::within(install,resources::WAIT,|s| {
                external_slots::reserved(install,s,&r.slot,"codex")?;refresh(install,home,r,&p.authority)?;let effective=verified(&id,&p,&original["scope"])?;
                bound_gate(s,key,r,&p.authority,&id,&supervisor,Some(&effective))?;child_of(&id,&supervisor)?;scope::membership(&id,&original["scope"]["binding"])?;
                let until=Instant::now()+Duration::from_secs(1);let mut left=bytes.as_slice();
                while !left.is_empty() {match input.write(left) {
                    Ok(0)=>return Err("native input disconnected; delivery unknown, never replay".into()),Ok(n)=>left=&left[n..],
                    Err(e) if e.kind()==io::ErrorKind::Interrupted=>{},
                    Err(e) if e.kind()==io::ErrorKind::WouldBlock&&Instant::now()<until=>{std::thread::sleep(Duration::from_millis(5));},
                    Err(_)=>return Err("native input deadline/error; delivery unknown, never replay".into()),
                }}Ok(())
            })?;
        }
    }
}
