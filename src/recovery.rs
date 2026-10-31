//! E3: owner-reviewed recovery; board truth stays on the board, selected scope stays in the Stop interlock.
use crate::{Res, board::{Board, entries}, core::{self, Release, State}, dispatch, service::Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub task:u64, pub execution:u64, pub revision:u64, pub workspace:String, pub evidence:String,
    pub effects_known:bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request { pub stop:String, pub board:String, pub rows:Vec<Selection> }

fn read_hold(install:&Path)->Res<Value> {
    let value:Value=serde_json::from_str(&fs::read_to_string(install.join("hold")).map_err(|e|format!("Stop hold unreadable: {e}"))?).map_err(|_|"Stop hold corrupt")?;
    if crate::lifecycle::stop_hold_status(install)["state"]!="held" {return Err("reviewed Stop hold required".into());}
    if value.get("recovery").is_some_and(|v|!v.is_object()) {return Err("recovery scope corrupt; held".into());}
    Ok(value)
}

/// A narrow admission exception, not a replacement task queue. Any corrupt record fails closed.
pub fn selected(install:&Path,root:&Path,task:u64)->bool {
    if !cfg!(target_os="macos") {return false;}
    let Ok(h)=read_hold(install) else {return false};
    if !h["previous_hold"].is_null() || !h["taskq_stop"]["roots"].as_array().is_some_and(|r|r.contains(&json!(root))) {return false;}
    h["recovery"].as_object().is_some_and(|ops|ops.values().any(|op| {
        op["state"]=="accepted" && op["root"]==json!(root) && op["selection"]["task"]==task
            && op["stop"]==h["taskq_stop"]["id"] && op["receipt"]["outcome"]=="accepted"
            && op["receipt"]["task"]==task && op["receipt"]["op_id"]==op["op_id"]
            && serde_json::from_value::<Selection>(op["selection"].clone()).is_ok_and(|s|valid(&s).is_ok())
    }))
}

pub fn allowed(install:&Path,root:&Path,task:u64)->bool {
    crate::data::focus(root).is_ok_and(|f|!f.is_empty()) && selected(install,root,task)
}

fn valid(s:&Selection)->Res<()> {
    if s.task==0||s.execution==0||s.revision==0||!s.effects_known||s.evidence.trim().is_empty()
        ||s.evidence.contains(['\n','\r'])||s.evidence.chars().count()>160||s.workspace.len()!=64 {
        return Err("name execution/revision, workspace and known effect outcomes with 1-160 characters of owner-reviewed evidence".into());
    }
    Ok(())
}
fn workspace(root:&Path,n:u64)->Res<Value> {
    let path=root.join(".worktrees").join(format!("taskq-{n}"));
    if !path.is_dir()||fs::symlink_metadata(&path).map_err(|e|e.to_string())?.file_type().is_symlink() {return Err("preserved worktree missing or symlink; inspect work before recovery".into());}
    let git=|args:&[&str]|->Res<Vec<u8>> {
        let o=crate::platform::output("recovery.workspace",crate::platform::command("git").arg("--no-optional-locks").args(args).current_dir(&path),Duration::from_secs(10))?;
        if !o.status.success() {return Err("preserved Git work unreadable; inspect before recovery".into());}Ok(o.stdout)
    };
    let top=git(&["rev-parse","--show-toplevel"])?;
    if Path::new(String::from_utf8_lossy(&top).trim()).canonicalize().map_err(|e|e.to_string())?!=path.canonicalize().map_err(|e|e.to_string())? {return Err("preserved worktree ownership differs".into());}
    let head=git(&["rev-parse","HEAD"])?;
    let status=git(&["status","--porcelain=v1","-z","--untracked-files=all"])?;
    let diff=git(&["diff","--binary","HEAD"])?;
    let mut digest=ring::digest::Context::new(&ring::digest::SHA256);
    for bytes in [&head,&status,&diff] {digest.update(&(bytes.len() as u64).to_le_bytes());digest.update(bytes);}
    let fingerprint=digest.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();
    Ok(json!({"fingerprint":fingerprint,"state":if status.is_empty(){"clean; preserved"}else{"dirty/untracked work retained"}}))
}

/// Pure eligibility boundary: native release and acceptance are independent of owner evidence.
fn blocker(p:&core::Projection,me:&str)->Option<&'static str> {
    let last=p.executions.last()?;
    if last.host!=me {return Some("last execution belongs to another host");}
    if p.executions.iter().any(|x|x.release.is_none()) {return Some("native release unknown; reconcile exact execution");}
    if p.executions.iter().any(|x|!x.accepted && x.release!=Some(Release::NoStart)) {return Some("input acceptance unknown; reconcile before replay");}
    if p.task.publication.is_some() {return Some("publication in progress; reconcile its receipt");}
    if !matches!(p.task.state,State::Ready|State::Doing|State::Review|State::Later|State::Failed) {return Some("task state requires its existing owner decision");}
    None
}
fn row(b:&dyn Board,c:&Config,n:u64)->Res<Value> {
    let issue=b.issue(n)?;
    if !issue.open||!crate::service::is_task(&issue) {return Err("task closed or no longer managed".into());}
    let log=entries(b,n)?;
    let p=core::project(n,&log,dispatch::deps_open(b,n,&issue.body,&log));
    let last=p.executions.last().ok_or("no execution evidence")?;
    let work=workspace(&c.root,n);
    let focus=crate::data::focus(&c.root);
    let reason=blocker(&p,&c.me).map(String::from)
        .or_else(||focus.as_ref().err().cloned())
        .or_else(||focus.as_ref().ok().filter(|f|f.is_empty()).map(|_|"project paused; recovery does not change focus".into()))
        .or_else(||work.as_ref().err().cloned());
    Ok(json!({"task":n,"title":issue.title,"url":b.url(n),"state":p.task.state.name(),"host":last.host,"runtime":last.runtime,
        "execution":last.id,"revision":p.revision,"native_exit":match last.release {Some(Release::Exit)=>"released exit",Some(Release::NoStart)=>"confirmed no-start",None=>"unknown"},
        "workspace":work.unwrap_or_else(|e|json!({"state":"unknown","reason":e})),"blocker":reason,"eligible":reason.is_none(),"selected":allowed(&c.install,&c.root,n)}))
}
fn scope(c:&Config,h:&Value)->Res<()> {
    if !cfg!(target_os="macos") {return Err("reviewed recovery is Mac only".into());}
    if !crate::host::project_paths(&c.install)?.contains(&c.root) || !h["taskq_stop"]["roots"].as_array().is_some_and(|r|r.contains(&json!(c.root))) {return Err("project is outside the reviewed Stop scope".into());}
    if !h["previous_hold"].is_null() {return Err("an earlier emergency hold remains; no recovery admission permitted".into());}
    Ok(())
}
pub fn review(b:&dyn Board,c:&Config)->Res<Value> {
    let h=read_hold(&c.install)?;scope(c,&h)?;
    let pending:Vec<Value>=h["recovery"].as_object().into_iter().flat_map(|ops|ops.values()).filter(|op|op["root"]==json!(c.root)&&op["state"]!="accepted").map(|op|json!({"stop":op["stop"],"board":op["board"],"rows":[op["selection"]]})).collect();
    let mut rows=vec![];
    for i in b.issues(None)?.into_iter().filter(crate::service::is_task) {
        let p=core::project(i.number,&entries(b,i.number)?,false);
        if p.executions.last().is_none_or(|x|x.host!=c.me) {continue;}
        rows.push(row(b,c,i.number).unwrap_or_else(|e|json!({"task":i.number,"title":i.title,"url":b.url(i.number),"eligible":false,"blocker":e})));
    }
    Ok(json!({"state":"review","stop":h["taskq_stop"]["id"],"board":b.url(0),"host":c.me,"rows":rows,"pending":pending,"focus":crate::data::focus(&c.root)?,"mode":"Mixed: unselected work stays held","service":crate::host::service_status(&c.install),"limits":crate::host::Policy::read(&c.install).map(|p|p.cap.json()).unwrap_or_else(|e|json!({"unknown":e})),"access":crate::policy::report(&c.install,&c.root,&c.permission_mode,&c.codex).iter().map(|e|e.json()).collect::<Vec<_>>()}))
}
fn op_id(stop:&str,s:&Selection)->String {format!("recovery.{stop}.{}.{}.{}",s.task,s.execution,s.revision)}
fn save(c:&Config,h:&Value)->Res<()> {
    crate::platform::write_atomic(&c.install.join("hold"),&h.to_string())?;
    if read_hold(&c.install)?!=*h {return Err("recovery hold readback differs; admission unknown".into());}Ok(())
}
/// Reconcile exactly this immutable board operation; never infer application from task state alone.
fn checked_entry(b:&dyn Board,s:&Selection,id:&str,kind:&str,text:&str,revision:u64)->Res<Option<u64>> {
    let log=entries(b,s.task)?;
    let Some(at)=log.iter().position(|e|!e.edited&&core::op_id(s.task,&e.body)==Some(id)) else {return Ok(None)};
    let expected=core::with_fields(core::entry(kind,&[],None,text),Some(id),Some(revision));
    if log[at].body!=expected || core::project(s.task,&log[..at],false).revision!=revision
        || core::project(s.task,&log[..=at],false).task.state!=if kind=="later" {State::Later}else{State::Ready} {
        return Err("recovery operation conflicts with board history; keep held".into());
    }
    Ok(Some(log[at].id))
}
pub fn apply(b:&dyn Board,c:&Config,request:&Request)->Res<Value> {
    if !cfg!(test) && std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("worker cannot authorize recovery".into());}
    if serde_json::to_vec(request).map_err(|e|e.to_string())?.len()>16<<10 {return Err("recovery request exceeds 16 KiB".into());}
    if request.rows.is_empty()||request.rows.len()>8 {return Err("select 1-8 tasks per review".into());}
    let _lock=coreq::file_lock::try_lock(&c.install.join("admission.lock")).map_err(|e|format!("recovery busy: {e}"))?;
    let mut h=read_hold(&c.install)?;scope(c,&h)?;
    if request.stop!=h["taskq_stop"]["id"] || request.board!=b.url(0) {return Err("Stop or board changed; review again".into());}
    if crate::data::focus(&c.root)?.is_empty() {return Err("project paused; focus preserved".into());}
    let mut seen=std::collections::BTreeSet::new();
    for s in &request.rows {valid(s)?;if !seen.insert(s.task) {return Err("duplicate selected task".into());}}
    let mut receipts=vec![];
    for s in &request.rows {
        let id=op_id(&request.stop,s);
        let text=format!("Reason: Reviewed recovery of execution {}: {}",s.execution,s.evidence);
        let bound=json!({"stop":request.stop,"board":request.board,"root":c.root,"selection":s,"op_id":id});
        let prior=h["recovery"][&id].clone();
        if prior.is_null() && h["recovery"].as_object().is_some_and(|ops|ops.values().any(|o|o["root"]==json!(c.root)&&o["selection"]["task"]==s.task&&o["state"]!="accepted")) {
            receipts.push(json!({"task":s.task,"outcome":"unknown","reason":"earlier recovery outcome unknown; reconcile its original selection first"}));continue;
        }
        if !prior.is_null() && ["stop","board","root","selection","op_id"].iter().any(|k|prior[*k]!=bound[*k]) {return Err("recovery operation reused with changed evidence; keep original payload".into());}
        if prior["state"]=="accepted" {receipts.push(prior["receipt"].clone());continue;}
        let outcome=(||->Res<Value>{
            let park_id=format!("{id}.park");
            let parked=checked_entry(b,s,&park_id,"later",&text,s.revision)?;
            let revision=parked.unwrap_or(s.revision);
            let mut found=checked_entry(b,s,&id,"requeue",&text,revision)?;
            let current=row(b,c,s.task)?;
            if current["eligible"]!=true {return Err(current["blocker"].as_str().unwrap_or("recovery eligibility unknown").into());}
            if current["execution"]!=s.execution||current["workspace"]["fingerprint"]!=s.workspace {return Err("execution or preserved work changed; review again".into());}
            if found.is_none() {
                if !prior.is_null() && !(prior["state"]=="park_pending" && parked.is_some()) {return Err("prior write outcome unknown; inspect the board before retry; no replay".into());}
                if current["revision"]!=revision {return Err("board revision changed; review again".into());}
                let mut pending=bound.clone();
                if h.get("recovery").is_none() {h["recovery"]=json!({});}
                if h["recovery"].as_object().is_none_or(|ops|ops.len()>=4096) {return Err("recovery scope full; no operation submitted".into());}
                let revision=if current["state"]=="ready" {
                    pending["state"]=json!("park_pending");h["recovery"][&id]=pending.clone();save(c,&h)?;
                    let r=dispatch::act(b,&c.root,s.task,"later",&[],&text,&dispatch::Op{id:Some(park_id.clone()),revision:Some(s.revision)});
                    if !r.ok() {return Err(format!("{}: {}",r.outcome,r.reason));}
                    checked_entry(b,s,&park_id,"later",&text,s.revision)?.ok_or("parking receipt missing")?
                }else{revision};
                pending["state"]=json!("requeue_pending");h["recovery"][&id]=pending;save(c,&h)?;
                let r=dispatch::act(b,&c.root,s.task,"requeue",&[],&text,&dispatch::Op{id:Some(id.clone()),revision:Some(revision)});
                if !r.ok() {return Err(format!("{}: {}",r.outcome,r.reason));}
                found=checked_entry(b,s,&id,"requeue",&text,revision)?;
                if found.is_none() {return Err("board receipt missing; keep held".into());}
            }
            let current=row(b,c,s.task)?;
            if current["execution"]!=s.execution||current["eligible"]!=true||current["revision"].as_u64()!=found {return Err("execution or board revision changed during recovery; keep held".into());}
            let receipt=json!({"task":s.task,"op_id":id,"outcome":"accepted","state":current["state"],"revision":current["revision"]});
            let mut accepted=bound.clone();accepted["state"]=json!("accepted");accepted["receipt"]=receipt.clone();h["recovery"][&id]=accepted;save(c,&h)?;
            Ok(receipt)
        })();
        receipts.push(outcome.unwrap_or_else(|reason|json!({"task":s.task,"op_id":id,"outcome":"unknown","reason":reason})));
    }
    crate::poke(&c.root)?;
    Ok(json!({"state":"mixed","stop":request.stop,"receipts":receipts,"reason":"Only confirmed selected tasks may run; all other work stays held"}))
}

/// Restart supervision once after the original Stop's exact host exit; never treat submission as running.
pub fn resume_service(c:&Config,stop:&str)->Value {
    let service=crate::host::service_status(&c.install);
    let receipt=crate::lifecycle::status(&c.install,Some(stop));
    resume_service_with(c,stop,&service,&receipt,||crate::host::install_project(&c.install,&c.root))
        .unwrap_or_else(|reason|json!({"state":"unknown","reason":reason}))
}
fn resume_service_with(c:&Config,stop:&str,service:&Value,receipt:&Value,install:impl FnOnce()->Res<String>)->Res<Value> {
    if service["state"]=="running" && service["fresh"]==true {return Ok(service.clone());}
    if service["state"]!="exited"||receipt["id"]!=stop||receipt["state"]!="observed"||receipt["native_exit"]!=true {
        return Err("host exit or original Stop receipt unknown; supervision not restarted".into());
    }
    let lock=coreq::file_lock::try_lock(&c.install.join("admission.lock")).map_err(|e|e.to_string())?;
    let mut h=read_hold(&c.install)?;scope(c,&h)?;
    if h["taskq_stop"]["id"]!=stop {return Err("Stop changed; supervision not restarted".into());}
    if !h["recovery"].as_object().is_some_and(|ops|ops.values().any(|op|op["root"]==json!(c.root)&&op["selection"]["task"].as_u64().is_some_and(|n|allowed(&c.install,&c.root,n)))) {return Err("no confirmed selection; supervision not restarted".into());}
    if h.get("recovery_start").is_some() {return Err("supervision start already requested; inspect its installation receipt, never submit another start".into());}
    h["recovery_start"]=json!({"state":"requested","stop":stop});save(c,&h)?;drop(lock);
    let receipt=install()?;
    Ok(json!({"state":"submitted","reason":receipt}))
}

#[cfg(test)]
mod tests {
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
}
