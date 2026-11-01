//! Local manager slots use the existing operation store and host admission authority (§3).
use crate::{Res,host,resources};
use coreq::process::Identity;
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use std::{path::{Path,PathBuf},collections::BTreeSet};

const SCHEMA:&str="taskq.slots/1";
const RETAIN:u64=24*3600;
#[derive(Clone,Debug,Deserialize,Serialize,PartialEq)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema:String,caller:String,op_id:String,root:PathBuf,runtime:String,pid:u32,started:String,boot:i64,expires_at:u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {binding:Binding,key:String}
fn now()->Res<u64> {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs()).map_err(|_|"slot clock unknown".into())}
fn digest(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn id(b:&Binding)->String {format!("slot-{}",digest(format!("{}\0{}",b.caller,b.op_id).as_bytes()))}
fn bound(op:&resources::Op)->Res<Binding> {
    serde_json::from_value(op.bound["local_slot"]["binding"].clone()).map_err(|_|"slot binding corrupt; occupancy unknown".into())
}
fn active(op:&resources::Op)->bool {op.receipt["occupied"]==true}
fn outcome(op:&mut resources::Op,state:&str,reason:&str,instance:&str,at:u64) {
    if op.receipt["state"]==state&&op.receipt["reason"]==reason&&op.receipt["instance"]==instance
        &&op.receipt["at"].as_u64().is_some_and(|old|at>=old&&at-old<30) {return;}
    op.receipt["state"]=json!(state);
    if state=="reserved" {op.receipt["occupied"]=json!(true);}
    if matches!(state,"released"|"denied") {op.receipt["occupied"]=json!(false);}
    op.receipt["reason"]=json!(reason);op.receipt["instance"]=json!(instance);op.receipt["at"]=json!(at);
    if matches!(state,"released"|"denied") {op.expires_at=at;}
}
fn validate(b:&Binding)->Res<()> {
    if b.schema!=SCHEMA||!resources::id(&b.caller)||!resources::id(&b.op_id)||!b.root.is_absolute()
        ||!crate::runtime::RUNTIMES.contains(&b.runtime.as_str())||b.pid==0||b.pid>i32::MAX as u32
        ||b.started.is_empty()||b.started.len()>128||!b.started.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-')
        ||b.boot<=0 {return Err("invalid local slot identity or release key".into());}
    Ok(())
}

fn exchange(install:&Path,action:&str,value:&Value)->Res<Value> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot reserve or release external slots".into());}
    if !matches!(action,"reserve"|"observe"|"release") {return Err("slot action must be reserve, observe or release".into());}
    let r:Request=serde_json::from_value(value.clone()).map_err(|_|"invalid local slot request")?;validate(&r.binding)?;
    if r.key.len()!=64||!r.key.bytes().all(|c|c.is_ascii_hexdigit()) {return Err("invalid local slot release key".into());}
    let key=id(&r.binding);let hash=digest(r.key.as_bytes());let at=now()?;
    let service=host::service_status(install);
    let result=resources::within(install,resources::WAIT,|s| {
        if let Some(op)=s.ops.get_mut(&key) {
            if bound(op)?!=r.binding||op.bound["local_slot"]["key_hash"]!=hash {return Err("slot belongs to another caller, payload or release key".into());}
            if action=="release"&&!matches!(op.receipt["state"].as_str(),Some("released"|"denied")) {
                // Cancellation is durable before acknowledgment; even queued release cannot later admit a turn.
                op.bound["local_slot"]["release"]=json!(true);
                op.receipt["state"]=json!(if active(op) {"releasing"}else{"cancelling"});
            }
        } else {
            if action!="reserve" {return Err("slot identity absent or no longer retained; never start from this observation".into());}
            if service["state"]!="running"||service["fresh"]!=true||service["slots_protocol"]!=1 {return Err("slot authority unknown: a fresh taskq.slots/1 host is required".into());}
            if at>=r.binding.expires_at||r.binding.expires_at-at>300 {return Err("slot admission deadline must be within 300 seconds".into());}
            if s.ops.len()>=resources::MAX_OPS {return Err("operation store full; no slot requested".into());}
            s.ops.insert(key.clone(),resources::Op {bound:json!({"local_slot":{"binding":r.binding,"key_hash":hash,"release":false}}),
                receipt:json!({"schema":SCHEMA,"caller":r.binding.caller,"op_id":r.binding.op_id,"state":"queued","occupied":false,"reason":"waiting for host admission","at":at}),
                expires_at:r.binding.expires_at,saved_at:None,writing:false,sent:true});
        }
        // within() durably saves and verifies the whole state before returning this receipt.
        Ok(s.ops[&key].receipt.clone())
    })?;
    let mut result=result;
    let service=host::service_status(install);
    let at=now()?;
    if service["state"]!="running"||service["fresh"]!=true||service["slots_protocol"]!=1
        ||result["instance"]!=service["instance"]||result["at"].as_u64().is_none_or(|t|at<t||at-t>60) {
        result["stored_state"]=result["state"].clone();result["state"]=json!("unknown");result["reason"]=json!("fresh matching host readback required; reconcile the same identity");
    }
    Ok(result)
}

pub fn command(args:&crate::Args)->Res<()> {
    if args.pos.len()!=2||args.flags.iter().any(|(k,v)|!["identity","action","request","json"].contains(&k.as_str())||v.len()!=1) {return Err("usage: service slots --identity PID | --action reserve|observe|release --request JSON [--json]".into());}
    let result=if let Some(pid)=args.one("identity") {
        if args.one("request").is_some()||args.one("action").is_some() {return Err("slot identity cannot include an action".into());}
        let pid=pid.parse::<u32>().ok().filter(|p|*p>0&&*p<=i32::MAX as u32).ok_or("invalid slot owner pid")?;
        let owner=coreq::process::identity(pid).map_err(|e|e.to_string())?.ok_or("slot caller is not live")?;
        json!({"schema":SCHEMA,"pid":owner.pid,"started":owner.started,"boot":crate::platform::boot_time()?})
    } else {
        let text=args.one("request").filter(|t|t.len()<=8192).ok_or("slot request required, at most 8192 bytes")?;
        exchange(&crate::platform::default_install(),args.one("action").ok_or("slot action required")?,&serde_json::from_str(text).map_err(|_|"invalid slot request JSON")?)?
    };
    if args.flags.contains_key("json")||args.one("identity").is_some() {println!("{result}");}
    else {println!("Slot: {}",result["state"].as_str().unwrap_or("unknown"));if let Some(reason)=result["reason"].as_str() {println!("Reason: {reason}");}}
    Ok(())
}

/// The host mutex stays held across store read, admission, durable receipt and slot synchronization.
/// Store errors fence every new host launch; they never discard an in-memory reservation.
pub fn tick(install:&Path,h:&mut host::Host)->Res<()> {
    let at=now()?;let boot=crate::platform::boot_time();
    let mut seen=std::collections::BTreeMap::new();
    reconcile(install,h,at,boot,|owner|seen.entry((owner.pid,owner.started.clone())).or_insert_with(||
        coreq::process::identity(owner.pid).map(|p|p.as_ref()==Some(owner)).map_err(|e|e.to_string())).clone())
}
fn reconcile(install:&Path,h:&mut host::Host,at:u64,boot:Res<i64>,mut live:impl FnMut(&Identity)->Res<bool>)->Res<()> {
    resources::try_with(install,|s| {
        let ids:Vec<_>=s.ops.iter().filter(|(_,op)|op.bound.get("local_slot").is_some()).map(|(id,_)|id.clone()).collect();
        let mut known=BTreeSet::new();
        // Restore every previous grant before attempting any new one, including across service restart.
        for key in &ids {
            let op=s.ops.get_mut(key).unwrap();let b=bound(op)?;validate(&b)?;
            if id(&b)!=*key||!matches!(op.receipt["state"].as_str(),Some("queued"|"busy"|"cancelling"|"reserved"|"releasing"|"unknown"|"released"|"denied")) {return Err("slot receipt corrupt; occupancy unknown".into());}
            if h.external_held(key)&&!active(op)&&!matches!(op.receipt["state"].as_str(),Some("released"|"denied")) {
                op.receipt["occupied"]=json!(true);
                op.receipt["state"]=json!("unknown");
                op.receipt["granted_at"]=json!(at);
            }
            if op.receipt["schema"]!=SCHEMA||op.receipt["caller"]!=b.caller||op.receipt["op_id"]!=b.op_id
                ||!op.receipt["occupied"].is_boolean()||!op.bound["local_slot"]["release"].is_boolean()
                ||op.bound["local_slot"]["key_hash"].as_str().is_none_or(|hash|hash.len()!=64||!hash.bytes().all(|c|c.is_ascii_hexdigit()))
                ||active(op)&&!op.receipt["granted_at"].is_u64()
                ||matches!(op.receipt["state"].as_str(),Some("released"|"denied"))&&active(op)
                ||op.receipt["state"]=="reserved"&&!active(op) {return Err("slot receipt identity invalid; occupancy unknown".into());}
            known.insert(key.clone());
            if active(op) {h.external_sync(key,&b.root,&b.runtime,true);}
        }
        if !h.external_known(&known) {return Err("previous slot disappeared from storage; occupancy retained".into());}
        // Rebuild eligible external demand under the host mutex; task passes own only task readiness.
        h.external_reset_ready();
        for key in ids {
            let op=s.ops.get_mut(&key).unwrap();let b=bound(op)?;
            let state=op.receipt["state"].as_str().unwrap().to_owned();
            if matches!(state.as_str(),"released"|"denied") {
                h.external_sync(&key,&b.root,&b.runtime,false);
                if at>op.expires_at.saturating_add(RETAIN) {s.ops.remove(&key);}else{
                    let expires_at=op.expires_at;
                    let reason=op.receipt["reason"].as_str().unwrap_or("terminal slot retained").to_owned();
                    outcome(op,&state,&reason,&h.instance,at);
                    op.expires_at=expires_at;
                }
                continue;
            }
            if op.bound["local_slot"]["release"]==true {
                // Only the exact immutable binding and release key can persist this completion attestation.
                outcome(op,"released","consumer confirmed entire turn ended",&h.instance,at);
                resources::save(install,s)?;h.external_sync(&key,&b.root,&b.runtime,false);continue;
            }
            if active(op) {
                if boot.as_ref().is_ok_and(|boot|*boot>op.receipt["granted_at"].as_i64().unwrap_or(i64::MAX)) {
                    outcome(op,"released","verified later host boot ended prior execution",&h.instance,at);
                    resources::save(install,s)?;h.external_sync(&key,&b.root,&b.runtime,false);continue;
                }
                let (state,reason)=match (&boot,live(&Identity{pid:b.pid,started:b.started.clone()})) {
                    (Ok(current),Ok(true)) if current.abs_diff(b.boot)<=2=>("reserved","slot retained for caller".to_owned()),
                    (_,Err(e))=>("unknown",format!("caller liveness unknown: {e}; slot retained")),
                    _=>("unknown","caller absent or boot uncertain; entire turn exit unproved; slot retained".into()),
                };
                outcome(op,state,&reason,&h.instance,at);continue;
            }
            let decision=(||->Res<()> {
                if at>=b.expires_at {return Err("denied: admission deadline expired".into());}
                if boot.as_ref().is_err()||!boot.as_ref().is_ok_and(|boot|boot.abs_diff(b.boot)<=2) {return Err("unknown: caller boot cannot be verified".into());}
                match live(&Identity{pid:b.pid,started:b.started.clone()}) {Ok(true)=>{},Ok(false)=>return Err("denied: caller exited before admission".into()),Err(e)=>return Err(format!("unknown: caller liveness: {e}"))}
                let roots=host::projects(install).map_err(|e|format!("unknown: {e}"))?;
                if !roots.contains(&b.root)||b.root.canonicalize().ok().as_ref()!=Some(&b.root) {return Err("denied: caller root is not registered canonically".into());}
                let v=crate::data::json(&b.root.join("taskq.json")).map_err(|e|format!("unknown: {e}"))?.ok_or("unknown: project configuration absent")?;
                // The host loop's guarded policy may forbid admission despite a valid host.json.
                let policy=h.policy.as_ref().map_err(|e|format!("unknown: {e}"))?;
                let cap=policy.project(&b.root,&v).map_err(|e|format!("unknown: {e}"))?;
                h.external_reserve(&key,&b.root,&b.runtime,&cap,h.elapsed())
            })();
            match decision {
                Ok(())=>{outcome(op,"reserved","host slot granted",&h.instance,at);op.receipt["granted_at"]=json!(at);},
                Err(reason)=>outcome(op,if reason.starts_with("denied:") {"denied"}else if reason.contains("unknown")||h.rebuilding {"unknown"}else{"busy"},&reason,&h.instance,at),
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs,sync::{Arc,Mutex,Barrier}};
    struct Fixture {dir:PathBuf,root:PathBuf,h:host::Host,request:Value,at:u64}
    impl Drop for Fixture {fn drop(&mut self) {let _=fs::remove_dir_all(&self.dir);}}
    impl Fixture {
        fn new()->Self {
            let dir=std::env::temp_dir().join(format!("taskq-slots-{}",crate::runtime::uuid()));fs::create_dir_all(&dir).unwrap();
            let root=dir.join("project");fs::create_dir(&root).unwrap();let root=root.canonicalize().unwrap();
            fs::write(root.join("taskq.json"),"{}").unwrap();fs::write(dir.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(dir.join("host.json"),json!({"limits":{"codex":1},"admission":{"max":1},"load_gate":{"enabled":false}}).to_string()).unwrap();
            fs::create_dir(root.join(".taskq")).unwrap();fs::write(root.join(".taskq/focus"),"off").unwrap();
            let h=host::Host::new(Ok(host::Policy::read(&dir).unwrap()));let at=now().unwrap();
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            fs::write(dir.join("service"),json!({"schema":1,"pid":native.pid,"started":native.started,"instance":h.instance,"at":at,"version":crate::platform::VERSION,"slots_protocol":1}).to_string()).unwrap();
            let request=json!({"binding":{"schema":SCHEMA,"caller":"pilotq","op_id":"turn-1","root":root,"runtime":"codex","pid":native.pid,"started":native.started,"boot":10,"expires_at":at+300},"key":"a".repeat(64)});
            Self{dir,root,h,request,at}
        }
        fn call(&self,action:&str)->Value {exchange(&self.dir,action,&self.request).unwrap()}
        fn tick(&mut self,live:Res<bool>) {reconcile(&self.dir,&mut self.h,self.at,Ok(10),|_|live.clone()).unwrap();}
        fn stored(&self)->Value {resources::try_with(&self.dir,|s|Ok(s.ops.values().find(|o|o.bound.get("local_slot").is_some()).unwrap().receipt.clone())).unwrap()}
    }
    // Chain: R1 R3 R9 R12 §3 — PilotQ and a worker contend for the same last slot; sync cannot erase a grant.
    // Risk: a second capacity pool or unsynchronized start oversubscribes, changes policy or changes focus.
    #[test]
    fn pilotq_worker_share_one_authority() {
        for pilotq_first in [false,true] {
            let f=Fixture::new();let policy=fs::read(f.dir.join("host.json")).unwrap();f.call("reserve");
            let shared=Arc::new(Mutex::new(host::Host::new(Ok(host::Policy::read(&f.dir).unwrap()))));
            let barrier=Arc::new(Barrier::new(3));let mut joins=vec![];
            for external in [pilotq_first,!pilotq_first] {
                let (shared,barrier,root,dir,at)=(shared.clone(),barrier.clone(),f.root.clone(),f.dir.clone(),f.at);
                joins.push(std::thread::spawn(move || {barrier.wait();let mut h=shared.lock().unwrap();
                    if external {reconcile(&dir,&mut h,at,Ok(10),|_|Ok(true)).unwrap();}
                    else {let cap=h.policy.as_ref().unwrap().cap.clone();let _=h.reserve(&root,1,77,"codex",&cap,false,30);}
                }));
            }
            barrier.wait();for j in joins {j.join().unwrap();}
            let mut h=shared.lock().unwrap();assert_eq!(h.occupied(),1);assert_eq!(h.used()["codex"],1);
            let r=f.stored();let external=r["state"]=="reserved";
            h.sync(&f.root,if external {vec![]}else{vec![(1,77,"codex".into())]});assert_eq!(h.occupied(),1);
            let cap=h.policy.as_ref().unwrap().cap.clone();assert!(h.reserve(&f.root,2,78,"codex",&cap,false,60).is_err());
            assert_eq!(fs::read(f.dir.join("host.json")).unwrap(),policy);assert_eq!(fs::read_to_string(f.root.join(".taskq/focus")).unwrap(),"off");
        }
    }
    // Chain: R9 R12 §3 A3 — a queued manager gets a round-robin turn despite a worker backlog.
    // Risk: project readiness updates erase external demand and starve PilotQ until its deadline.
    #[test]
    fn queued_slot_is_admitted_with_a_ready_worker_project() {
        let mut f=Fixture::new();
        let worker=f.root.join("worker");
        f.h.ready(&worker,true);
        f.call("reserve");
        f.h.ready(&f.root,false);
        f.tick(Ok(true));
        let out=f.stored();
        assert_eq!(out["state"],"reserved","queued external demand must join round-robin: {out}");
        assert_eq!(f.h.occupied(),1);
        let cap=f.h.policy.as_ref().unwrap().cap.clone();
        assert!(f.h.reserve(&worker,1,77,"codex",&cap,true,30).unwrap_err().contains("capacity full"));
    }
    // Chain: R9 R12 §3 — a valid host.json cannot clear the host loop's legacy/startup barrier.
    // Risk: reconciling PilotQ admission also enables workers later in the same host pass.
    #[test]
    fn external_admission_preserves_guarded_policy() {
        let mut f=Fixture::new();
        let cap=f.h.policy.as_ref().unwrap().cap.clone();
        let barrier="legacy/startup barrier".to_owned();
        f.h.reload(Err(barrier.clone()));
        f.call("reserve");
        for _ in 0..2 {
            f.tick(Ok(true));
            assert_eq!(f.h.policy,Err(barrier.clone()));
            let out=f.stored();
            assert_eq!(out["state"],"unknown");assert_eq!(out["occupied"],false);
            assert!(out["reason"].as_str().unwrap().contains(&barrier));
            assert_eq!(f.h.reserve(&f.root,1,77,"codex",&cap,false,30),Err(barrier.clone()));
            assert_eq!(f.h.occupied(),0);
        }
    }
    // Chain: R9 R12 §3 — existing policy gates external callers without changing policy or fabricating healthy reads.
    #[test]
    fn external_admission_keeps_each_existing_barrier() {
        for case in ["runtime","project","aggregate","load","hold","recovery","config","project-config"] {
            let mut f=Fixture::new();
            let mut policy=json!({"limits":{"codex":2},"admission":{"max":2},"load_gate":{"enabled":false}});
            match case {
                "runtime"=>policy["limits"]["codex"]=json!(0),
                "project"=>fs::write(f.root.join("taskq.json"),r#"{"limits":{"codex":0}}"#).unwrap(),
                "aggregate"=>policy["admission"]["max"]=json!(1),
                "load"=>policy["load_gate"]["enabled"]=json!(true),
                "project-config"=>fs::write(f.root.join("taskq.json"),"[]").unwrap(),
                "hold"=>f.h.held=Some("operator emergency hold".into()),
                "recovery"=>f.h.rebuilding=true,
                _=>{},
            }
            fs::write(f.dir.join("host.json"),if case=="config" {"{".into()}else{policy.to_string()}).unwrap();
            f.h.reload(host::Policy::read(&f.dir));
            f.call("reserve");
            if case=="aggregate" {
                // A worker occupies a different runtime; the host aggregate still includes it.
                f.h.sync(&f.root,vec![(1,77,"claude".into())]);
            }
            f.tick(Ok(true));let out=f.stored();assert_eq!(out["occupied"],false,"{case}: {out}");
            assert_eq!(out["state"],if ["load","recovery","config","project-config"].contains(&case) {"unknown"}else{"busy"},"{case}: {out}");
            let reason=out["reason"].as_str().unwrap();assert!(reason.contains(match case {"runtime"=>"host capacity","project"=>"project capacity","aggregate"=>"aggregate capacity","load"=>"sample","hold"=>"emergency hold","recovery"=>"recovery","project-config"=>"taskq.json",_=>"host.json"}),"{case}: {out}");
        }
    }
    // Chain: §3 R12 — duplicate/lost responses, foreign release, cancellation and restart reconcile one identity.
    // Risk: retries launch twice, cancellation races admission, or a vanished manager frees surviving tools.
    #[test]
    fn retries_release_and_recovery_keep_ownership() {
        let mut f=Fixture::new();f.call("reserve");f.call("reserve");f.tick(Ok(true));assert_eq!(f.call("observe")["state"],"reserved");
        assert_eq!(f.h.occupied(),1);assert_eq!(f.call("reserve")["state"],"reserved");
        for field in ["key","caller","runtime","op_id","started"] {
            let mut other=f.request.clone();if field=="key" {other[field]=json!("b".repeat(64));}else{other["binding"][field]=json!("foreign");}
            assert!(exchange(&f.dir,"release",&other).is_err(),"foreign {field}");assert_eq!(f.h.occupied(),1);
        }
        let mut recovered=host::Host::new(Ok(host::Policy::read(&f.dir).unwrap()));
        reconcile(&f.dir,&mut recovered,f.at,Err("boot unavailable".into()),|_|Err("native read unavailable".into())).unwrap();
        assert_eq!(recovered.occupied(),1);assert_eq!(f.stored()["state"],"unknown");
        f.h=recovered;f.tick(Ok(false));assert_eq!(f.h.occupied(),1);assert_eq!(f.stored()["state"],"unknown");
        assert_eq!(f.call("release")["state"],"unknown","old host generation cannot acknowledge a release");
        assert_eq!(f.stored()["state"],"releasing");
        let mut record=crate::data::json(&f.dir.join("service")).unwrap().unwrap();record["instance"]=json!(f.h.instance);
        fs::write(f.dir.join("service"),record.to_string()).unwrap();
        assert_eq!(f.call("release")["state"],"releasing");f.tick(Ok(false));assert_eq!(f.h.occupied(),0);
        assert_eq!(f.call("release")["state"],"released");assert_eq!(f.call("reserve")["state"],"released");
        let retained_until=resources::try_with(&f.dir,|s|Ok(s.ops.values().next().unwrap().expires_at)).unwrap();
        f.h=host::Host::new(Ok(host::Policy::read(&f.dir).unwrap()));f.tick(Ok(false));
        assert_eq!(f.stored()["instance"],f.h.instance,"a restarted host must reacknowledge a terminal slot");
        assert_eq!(resources::try_with(&f.dir,|s|Ok(s.ops.values().next().unwrap().expires_at)).unwrap(),retained_until,"readback cannot extend terminal retention");
        f.at+=RETAIN+1;f.tick(Ok(false));assert!(resources::try_with(&f.dir,|s|Ok(s.ops.is_empty())).unwrap());
    }
    // Chain: §3 R12 — never-started calls expire/cancel; admitted calls survive expiry until release or proven reboot.
    #[test]
    fn abandoned_calls_and_store_failures_stay_fenced() {
        let mut f=Fixture::new();f.call("reserve");f.call("release");f.tick(Ok(true));assert_eq!(f.stored()["state"],"released");assert_eq!(f.h.occupied(),0);
        let mut f=Fixture::new();f.call("reserve");f.at+=301;f.tick(Ok(true));assert_eq!(f.stored()["state"],"denied");assert_eq!(f.h.occupied(),0);
        let mut f=Fixture::new();f.call("reserve");f.tick(Ok(false));assert_eq!(f.stored()["state"],"denied");
        let mut f=Fixture::new();f.call("reserve");f.tick(Err("native unsupported".into()));assert_eq!(f.stored()["state"],"unknown");assert_eq!(f.h.occupied(),0);
        f.tick(Ok(true));assert_eq!(f.h.occupied(),1);f.at+=301;f.tick(Ok(false));assert_eq!(f.h.occupied(),1);
        let path=resources::dir(&f.dir).join("state.json");let original=fs::read(&path).unwrap();
        fs::write(resources::dir(&f.dir).join("state.json.pending"),"interrupted").unwrap();assert!(tick(&f.dir,&mut f.h).is_err());assert_eq!(f.h.occupied(),1);
        fs::remove_file(resources::dir(&f.dir).join("state.json.pending")).unwrap();fs::remove_file(&path).unwrap();
        assert!(reconcile(&f.dir,&mut f.h,f.at,Ok(10),|_|Ok(false)).unwrap_err().contains("disappeared"));assert_eq!(f.h.occupied(),1);
        fs::write(&path,&original).unwrap();
        reconcile(&f.dir,&mut f.h,f.at,Ok(f.at as i64),|_|Ok(false)).unwrap();assert_eq!(f.h.occupied(),0);assert_eq!(f.stored()["state"],"released");
    }
}
