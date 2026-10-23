//! Opt-in owner-request exchange, never task authority or a conversation/telemetry channel.
use crate::{Res, enrollment, resources, setup};
use coreq::enroll::{Https, Wire};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io::Read, path::Path, thread::JoinHandle};

pub const SCHEMA: &str = "taskq.pm/1";
pub const EXCHANGE: &str = "/pm/v1/exchange";
const BODY: usize = 1 << 20;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target { pub installation_id:String, pub generation:u64, pub project_id:String, pub runtime_id:String }
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema:String, pub actor_id:String, pub op_id:String, pub grant_id:String, pub revision:String,
    pub target:Target, pub issued_at:u64, pub expires_at:u64, pub text:String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    actor_id:String, installation_id:String, generation:u64, projects:Vec<String>, runtimes:Vec<String>,
    expires_at:u64, revoked:bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Op { hash:String, request_id:String, actor_id:String, op_id:String, revision:String, grant_id:String,
    installation_id:String, generation:u64, project_id:String, runtime_id:String, expires_at:u64, receipt:Value, sent:bool }
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State { grants:BTreeMap<String,Grant>, ops:BTreeMap<String,Op> }

fn digest(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn folder(install:&Path)->std::path::PathBuf {setup::dir(install).join("pm")}
fn save(install:&Path,s:&State)->Res<()> {
    let text=serde_json::to_string(s).map_err(|_|"PM state unavailable")?;
    if text.len()>8<<20 {return Err("PM state full; existing identities kept".into());}
    let path=folder(install).join("state");
    coreq::write_durable(&path,&text).map_err(|_|"PM state write unknown; reconcile only")?;
    if fs::read_to_string(path).ok().as_deref()!=Some(text.as_str()){return Err("PM state readback unknown; reconcile only".into());}Ok(())
}
fn with<T>(install:&Path,f:impl FnOnce(&mut State)->Res<T>)->Res<T> {
    crate::messages::plain(&setup::dir(install))?;crate::messages::plain(&folder(install))?;
    fs::create_dir_all(folder(install)).map_err(|_|"PM storage unavailable")?;
    let _lock=coreq::file_lock::try_lock(&folder(install).join("lock")).map_err(|_|"PM storage busy; retry readback")?;
    match fs::symlink_metadata(folder(install).join("state.pending")) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        _=>return Err("PM interrupted durable write; files kept for reconciliation".into()),
    }
    let path=folder(install).join("state");crate::messages::plain(&path)?;
    let mut s=if path.exists() {
        let mut text=String::new();fs::File::open(&path).map_err(|_|"PM storage unavailable")?.take((8<<20)+1).read_to_string(&mut text).map_err(|_|"PM storage unreadable; kept")?;
        if text.len()>8<<20 {return Err("PM storage oversized; kept".into());}
        serde_json::from_str(&text).map_err(|_|"PM storage corrupt; kept")?
    }else{State::default()};
    let valid_hash=|v:&str|v.len()==64&&v.bytes().all(|c|c.is_ascii_digit()||(b'a'..=b'f').contains(&c));
    if s.grants.len()>64||s.ops.len()>4096
        ||s.grants.iter().any(|(id,g)|![id,&g.actor_id,&g.installation_id].iter().all(|id|resources::id(id))||g.generation==0
            ||g.projects.iter().any(|id|!resources::id(id))||g.runtimes.iter().any(|r|!crate::runtime::setup_runtimes().any(|rt|rt==r)||!crate::runtime::of(r).remote))
        ||s.ops.iter().any(|(key,o)|!valid_hash(key)||o.request_id!=*key||!valid_hash(&o.hash)
        ||[&o.actor_id,&o.op_id,&o.revision,&o.grant_id,&o.installation_id,&o.project_id,&o.runtime_id].iter().any(|id|!resources::id(id))) {
        return Err("PM storage identity invalid; kept".into());
    }
    f(&mut s)
}
pub fn projects(install:&Path,now:u64)->Res<BTreeMap<String,resources::Project>> {
    resources::with(install,|s|{resources::prepare(s,install,now)?;Ok(s.projects.clone())})
}
pub fn grant(install:&Path,id:&str,actor:&str,projects:&[String],runtimes:&[String],hours:u64,now:u64)->Res<()> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some(){return Err("a worker cannot change PM grants".into());}
    if !resources::id(id)||!resources::id(actor)||!(1..=720).contains(&hours)||projects.is_empty()||runtimes.is_empty()
        ||runtimes.iter().any(|r|!crate::runtime::setup_runtimes().any(|rt|rt==r)||!crate::runtime::of(r).remote) {return Err("PM grant needs valid IDs, projects, owner runtimes and 1-720 hours".into());}
    resources::with(install,|registry| {
    resources::prepare(registry,install,now)?;
    let registered=&registry.projects;
    if projects.iter().any(|id|!registered.contains_key(id)){return Err("Unknown PM project; read service pm --json".into());}
    let a=enrollment::active(install)?.ok_or("No active enrolled connection")?;
    let (Some(inst),Some(generation))=(a["installation"].as_str(),a["generation"].as_u64()) else {return Err("Connection identity unknown".into());};
    with(install,|s| {
        if s.grants.len()>=64&&!s.grants.contains_key(id){return Err("PM grant store full; kept".into());}
        s.grants.insert(id.into(),Grant{actor_id:actor.into(),installation_id:inst.into(),generation,projects:projects.to_vec(),runtimes:runtimes.to_vec(),expires_at:now.saturating_add(hours*3600),revoked:false});save(install,s)
    })
    })
}
pub fn revoke(install:&Path,id:&str)->Res<()> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some(){return Err("a worker cannot change PM grants".into());}
    with(install,|s|{s.grants.get_mut(id).ok_or("Unknown PM grant")?.revoked=true;save(install,s)})
}
fn authorized(g:&Grant,r:&Request,active:&Value,now:u64)->bool {
    !g.revoked&&now<g.expires_at&&g.actor_id==r.actor_id&&g.installation_id==r.target.installation_id&&g.generation==r.target.generation
        &&active["installation"]==r.target.installation_id&&active["generation"]==r.target.generation
        &&g.projects.contains(&r.target.project_id)&&g.runtimes.contains(&r.target.runtime_id)
}
fn typed(r:&Request)->bool {
    r.schema==SCHEMA&&[&r.actor_id,&r.op_id,&r.grant_id,&r.revision,&r.target.project_id,&r.target.installation_id,&r.target.runtime_id].iter().all(|s|resources::id(s))
        &&r.target.generation>0&&!r.text.trim().is_empty()&&r.text.len()<=64<<10
        &&r.expires_at>r.issued_at&&r.expires_at-r.issued_at<=300
}
fn receipt(r:&Request,outcome:&str,reason:&str)->Value {
    json!({"schema":SCHEMA,"actor_id":r.actor_id,"op_id":r.op_id,"revision":r.revision,"outcome":outcome,"reason":reason})
}
fn observe(install:&Path,o:&Op)->Value {
    let outcome=o.receipt["outcome"].as_str().filter(|s|["accepted","rejected","stale","unknown"].contains(s)).unwrap_or("unknown");
    let mut out=json!({"schema":SCHEMA,"actor_id":o.actor_id,"op_id":o.op_id,"revision":o.revision,"outcome":outcome,
        "reason":match outcome {"rejected"=>"PM request denied or expired","stale"=>"PM project revision changed",_=>"PM dispatch intent; check the same request"}});
    // An intent without a readable setup record can never authorize another create.
    if out["outcome"]=="unknown"||out["outcome"]=="accepted" {
        match setup::read(&setup::dir(install).join(format!("{}.json",o.request_id))) {
            Ok(r) if r.runtime==o.runtime_id=>{
                out["outcome"]=json!("accepted");out["reason"]=Value::Null;out["request_id"]=json!(o.request_id);
                let url=r.url.filter(|u|crate::runtime::link_ok(u));
                out["native"]=json!({"state":r.state,"host":r.host,"runtime":r.runtime,"session":r.session,"url":url});
            },
            _=>{out["outcome"]=json!("unknown");out["reason"]=json!("Native request readback unavailable; never resend with a new ID");}
        }
    }
    out
}
fn take(install:&Path,s:&mut State,v:&Value,active:&Value,registered:&BTreeMap<String,resources::Project>,now:u64)->Res<()> {
    let r:Request=serde_json::from_value(v.clone()).map_err(|_|"Invalid PM request")?;
    if !typed(&r){return Err("Invalid PM request".into());}
    let key=digest(format!("{}\0{}\0{}",r.target.installation_id,r.actor_id,r.op_id).as_bytes());
    let hash=digest(&serde_json::to_vec(&r).map_err(|_|"Invalid PM request")?);
    if let Some(old)=s.ops.get_mut(&key) {
        // Changed input never replaces the original identity or its receipt.
        if old.hash!=hash {return Err("PM op-id already binds different input".into());}
        old.sent=false;return save(install,s);
    }
    if s.ops.len()>=4096 {return Err("PM identity store full; no request taken".into());}
    let verdict=if s.grants.get(&r.grant_id).is_none_or(|g|!authorized(g,&r,active,now)){("rejected","PM grant denied")}
        else if now.abs_diff(r.issued_at)>300||r.issued_at>now.saturating_add(30)||now>=r.expires_at{("rejected","PM request expired or clock unknown")}
        else if registered.get(&r.target.project_id).is_none(){("rejected","PM project unavailable")}
        else if registered[&r.target.project_id].revision!=r.revision{("stale","PM project revision changed")}
        else {("unknown","PM dispatch intent; check the same request")};
    let out=receipt(&r,verdict.0,verdict.1);
    s.ops.insert(key.clone(),Op{hash,request_id:key.clone(),actor_id:r.actor_id.clone(),op_id:r.op_id.clone(),revision:r.revision.clone(),grant_id:r.grant_id.clone(),installation_id:r.target.installation_id.clone(),generation:r.target.generation,project_id:r.target.project_id.clone(),runtime_id:r.target.runtime_id.clone(),expires_at:r.expires_at,receipt:out,sent:false});
    save(install,s)?;
    if verdict.0=="unknown" {
        // Revalidate the connection after disk/lock waits. Failure leaves an intent, never replayable.
        if enrollment::active(install)?.as_ref()!=Some(active){return Err("PM connection changed; outcome unknown".into());}
        let root=&registered[&r.target.project_id].root;
        // setup::open_id persists once; every later exchange only calls observe.
        let _=setup::open_pm(install,Path::new(root),&r.text,&key,&r.target.runtime_id);
        let observed=observe(install,&s.ops[&key]);s.ops.get_mut(&key).unwrap().receipt=observed;save(install,s)?;
    }
    Ok(())
}
fn targets(s:&State,active:&Value,registered:&BTreeMap<String,resources::Project>,now:u64)->Vec<Value> {
    s.grants.iter().filter(|(_,g)|!g.revoked&&now<g.expires_at&&active["installation"]==g.installation_id&&active["generation"]==g.generation).flat_map(|(id,g)|g.projects.iter().filter_map(move |pid|registered.get(pid).map(|p|json!({"project_id":pid,"revision":p.revision,"runtimes":g.runtimes,"grant_id":id,"actor_id":g.actor_id,"expires_at":g.expires_at})))).collect()
}
pub fn report(install:&Path,now:u64)->Res<Value> {
    let registered=projects(install,now)?;let active=enrollment::active(install)?.unwrap_or(Value::Null);
    with(install,|s|Ok(json!({"schema":SCHEMA,"projects":registered.iter().map(|(id,p)|json!({"project_id":id,"revision":p.revision,"root":p.root})).collect::<Vec<_>>(),"targets":targets(s,&active,&registered,now),"grants":s.grants,"receipts":s.ops.values().map(|o|observe(install,o)).collect::<Vec<_>>()})))
}

/// Serialize grant revocation against the pending -> launching boundary, in PM -> setup lock order.
pub fn launch_guard<T>(install:&Path,r:&setup::Request,f:impl FnOnce()->Res<T>)->Res<T> {
    if !r.pm{return f();}
    resources::with(install,|registry| {
      let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"PM clock unknown")?.as_secs();
      resources::prepare(registry,install,now)?;
      let active=enrollment::active(install)?.unwrap_or(Value::Null);
      let registered=&registry.projects;
      with(install,|s| {
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"PM clock unknown")?.as_secs();
        let o=s.ops.get(&r.id).ok_or("PM launch identity missing; request kept")?;
        let allowed=active["installation"]==o.installation_id&&active["generation"]==o.generation&&s.grants.get(&o.grant_id).is_some_and(|g|!g.revoked&&now<g.expires_at&&g.actor_id==o.actor_id&&active["installation"]==g.installation_id&&active["generation"]==g.generation&&g.projects.contains(&o.project_id)&&g.runtimes.contains(&o.runtime_id))
            &&now<o.expires_at&&registered.get(&o.project_id).is_some_and(|p|r.root.as_deref()==Some(p.root.as_str())&&p.revision==o.revision)&&r.runtime==o.runtime_id;
        if !allowed {return Err("PM launch authority expired, revoked or stale; request kept".into());}
        f()
      })
    })
}
pub fn exchange(install:&Path,active:&Value,w:&mut dyn Wire,now:u64)->Res<bool> {
    if !folder(install).join("state").exists()&&!folder(install).join("state.pending").exists(){return Ok(false);}
    // shortcut: one bounded HTTPS exchange holds the existing connection fence; split leases if contention is measured.
    resources::with(install,|registry| {
      resources::prepare(registry,install,now)?;
      if enrollment::active(install)?.as_ref()!=Some(active){return Err("PM connection changed; no disclosure".into());}
      with(install,|s| {
        let targets=targets(s,active,&registry.projects,now);
        let mut receipts=vec![];
        for (key,o) in &mut s.ops {
            if active["installation"]!=o.installation_id||active["generation"]!=o.generation {continue;}
            // A revoked actor no longer receives native session identities.
            let denial=["rejected","stale"].iter().any(|v|o.receipt["outcome"]==*v);
            if !denial&&!s.grants.get(&o.grant_id).is_some_and(|g|!g.revoked&&now<g.expires_at&&g.actor_id==o.actor_id&&active["installation"]==g.installation_id&&active["generation"]==g.generation&&g.projects.contains(&o.project_id)&&g.runtimes.contains(&o.runtime_id)){continue;}
            let fresh=observe(install,o);if fresh!=o.receipt{o.receipt=fresh;o.sent=false;}
            if !o.sent&&receipts.len()<8 {receipts.push((key.clone(),o.receipt.clone()));}
        }
        save(install,s)?;
        if targets.is_empty()&&receipts.is_empty(){return Ok(false);}
        let send=json!({"schema":SCHEMA,"installation_id":active["installation"],"generation":active["generation"],"targets":targets,"receipts":receipts.iter().map(|(_,r)|r).collect::<Vec<_>>()});
    let (code,reply)=w.post(active["destination"].as_str().ok_or("PM destination unknown")?,EXCHANGE,active["credential"].as_str(),&send).map_err(|_|"PM exchange unavailable")?;
    if code!=200 {return Err("PM exchange denied or unavailable".into());}
    if serde_json::to_vec(&reply).map_or(usize::MAX,|v|v.len())>BODY||!resources::keys(&reply,&["schema","server_at","requests"])||reply["schema"]!=SCHEMA {return Err("Invalid PM exchange reply".into());}
    let requests=reply["requests"].as_array().filter(|a|a.len()<=8).ok_or("Invalid PM request batch")?;
    let server=reply["server_at"].as_u64().ok_or("PM server clock unknown")?;
    let now=now.max(w.now());if now.abs_diff(server)>30{return Err("PM clock uncertainty; no dispatch".into());}
    if enrollment::active(install)?.as_ref()!=Some(active){return Err("PM connection changed; no dispatch".into());}
    resources::prepare(registry,install,now)?;
        for (key,r) in receipts {if let Some(o)=s.ops.get_mut(&key).filter(|o|o.receipt==r){o.sent=true;}}
        save(install,s)?;
        for v in requests {take(install,s,v,active,&registry.projects,now.max(w.now()))?;}
        Ok(!requests.is_empty()||!send["receipts"].as_array().unwrap().is_empty())
      })
    })
}
#[derive(Default)]
pub struct Exchanger {job:Option<JoinHandle<Res<bool>>>,next:u64}
impl Exchanger {
    pub fn tick(&mut self,install:&Path,snap:Option<&crate::snapshot::Snapshot>,now:u64)->Vec<String> {
        let mut log=vec![];
        if self.job.as_ref().is_some_and(|j|j.is_finished()) {
            let result=self.job.take().unwrap().join().unwrap_or_else(|_|Err("PM exchange failed".into()));
            self.next=now+match result {Ok(true)=>5,Ok(false)=>30,Err(_)=>60};if let Err(e)=result {log.push(e);}
        }
        if self.job.is_some()||now<self.next{return log;}
        self.next=now+30;
        let Some(active)=enrollment::active(install).ok().flatten().filter(|a|enrollment::speaks(a,snap)) else{return log;};
        if !folder(install).join("state").exists(){return log;}
        let install=install.to_path_buf();
        self.job=Some(std::thread::spawn(move ||{let mut w=Https{browser:false};let now=w.now();exchange(&install,&active,&mut w,now)}));log
    }
}

#[cfg(test)]
mod tests {
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
    #[test]
    fn captured_exchange_never_exports_owner_text() {
        crate::with_test_capture(|store,_| {
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
}
