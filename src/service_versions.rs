//! Read-only D18 producer projection. Never exports raw installation or lifecycle data.
use std::{fs, path::Path, time::{SystemTime, UNIX_EPOCH}};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all="snake_case")]
enum State { Ok, Unknown, LastKnown, Stale }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all="snake_case")]
enum Source { Binary, Current, Healthy, ReleaseCheck, Consumer, CargoLock }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field { value: Option<String>, at: Option<u64>, source: Source, state: State, reason: Option<String> }
impl Field {
    fn unknown(source:Source, reason:&str)->Self { Self {value:None,at:None,source,state:State::Unknown,reason:Some(reason.into())} }
    fn known(source:Source, value:&str, at:u64)->Self {Self {value:Some(value.into()),at:Some(at),source,state:State::Ok,reason:None}}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    schema:u64, service:String, observed:u64,
    running:Field, selected:Field, available:Field, health:Field, lifecycle:Field, coreq:Field,
}
fn clock()->u64 {SystemTime::now().duration_since(UNIX_EPOCH).map_or(0,|d|d.as_secs())}
fn modified(path:&Path)->Option<u64> {fs::symlink_metadata(path).ok().filter(|m|m.is_file())?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d|d.as_secs())}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct BinaryEvidence {
    schema:u8, service:String, observed:u64, revision:Option<String>, digest:Option<String>,
    pid:Option<u32>, started:Option<String>, generation:Option<String>, native_observed:Option<u64>, state:String, reason:Option<String>,
}
/// Windows locks executing files; Linux opens the native loaded image, not a replaceable pathname.
#[cfg(any(windows,target_os="linux"))]
fn artifact()->Option<&'static (std::path::PathBuf,String)> {
    static ARTIFACT:std::sync::OnceLock<Option<(std::path::PathBuf,String)>>=std::sync::OnceLock::new();
    ARTIFACT.get_or_init(|| {
        use std::io::Read;
        let path=std::env::current_exe().ok()?.canonicalize().ok()?;
        let loaded=if cfg!(target_os="linux") {Path::new("/proc/self/exe")} else {path.as_path()};
        let mut file=fs::File::open(loaded).ok()?;
        let mut hash=ring::digest::Context::new(&ring::digest::SHA256);let mut bytes=[0;65536];
        loop {let n=file.read(&mut bytes).ok()?;if n==0 {break;}hash.update(&bytes[..n]);}
        Some((path,hash.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect()))
    }).as_ref()
}
// Signing can replace a running macOS pathname; no digest without loaded-vnode proof.
#[cfg(not(any(windows,target_os="linux")))]
fn artifact()->Option<&'static (std::path::PathBuf,String)> {None}
pub fn binary(install:&Path,service:&str,generation:Option<&str>)->Value {
    let owner=crate::host::service_status(install);
    let bound=owner["state"]=="running"&&owner["fresh"]==true&&owner["pid"]==std::process::id()
        &&generation.is_some_and(|g|owner["instance"]==g);
    let artifact=if bound&&!cfg!(debug_assertions) {artifact()}else{None};
    let immutable=artifact.is_some_and(|(path,_)|
        crate::platform::tool(install).release_exe(crate::platform::VERSION).canonicalize().ok().as_ref()==Some(path));
    let revision=env!("TASKQ_REVISION");
    let proven=bound&&immutable&&crate::readiness::hex(revision,40);
    serde_json::to_value(BinaryEvidence {schema:1,service:service.into(),observed:clock(),
        revision:(bound&&!cfg!(debug_assertions)&&crate::readiness::hex(revision,40)).then(||revision.into()),digest:proven.then(||artifact.unwrap().1.clone()),
        pid:bound.then_some(std::process::id()),started:bound.then(||owner["started"].as_str().unwrap_or_default().into()),
        generation:bound.then(||generation.unwrap().into()),native_observed:bound.then(||owner["at"].as_u64().unwrap_or(0)),state:if proven {"binary_ready"}else{"unknown"}.into(),
        reason:(!proven).then(||"running artifact or native generation unproved".into())}).unwrap()
}
fn read_binary(body:&str,service:&str,updated:i64,now:i64,ttl:i64,failed:bool)->Value {
    let unknown=|reason|json!({"state":"unknown","reason":reason});
    let mut lines=body.lines().filter_map(|l|l.strip_prefix("binary-evidence "));
    let Some(text)=lines.next().filter(|s|s.len()<=2048) else {return unknown("running binary evidence missing")};
    if lines.next().is_some() {return unknown("running binary evidence invalid")}
    let Ok(e)=serde_json::from_str::<BinaryEvidence>(text) else {return unknown("running binary evidence invalid")};
    if e.schema!=1||e.service!=service||e.observed==0||e.observed>i64::MAX as u64 {return unknown("running binary identity invalid")}
    if failed||!crate::snapshot::live(now.saturating_sub(updated),ttl)
        ||!crate::snapshot::live(now.saturating_sub(e.observed as i64),ttl) {return unknown("running binary observation stale or unavailable")}
    let token=|s:&str|!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-');
    if e.revision.as_deref().is_some_and(|s|!crate::readiness::hex(s,40))
        ||e.digest.as_deref().is_some_and(|s|!crate::readiness::hex(s,64))||e.pid==Some(0)
        ||e.started.as_deref().is_some_and(|s|!token(s))||e.generation.as_deref().is_some_and(|s|!token(s))
        ||e.native_observed.is_some_and(|at|at==0||at>e.observed||!crate::snapshot::live(now.saturating_sub(at as i64),ttl)) {
        return unknown("running binary evidence invalid");
    }
    let complete=e.revision.is_some()&&e.digest.is_some()&&e.pid.is_some()&&e.started.is_some()&&e.generation.is_some()&&e.native_observed.is_some();
    if !match e.state.as_str() {
        "binary_ready"=>complete&&e.reason.is_none(),
        "unknown"=>e.reason.as_deref()==Some("running artifact or native generation unproved"),
        _=>false,
    } {return unknown("running artifact or native generation unproved");}
    serde_json::to_value(e).unwrap()
}
pub fn version(s:&str)->bool {
    let base=match s.split_once('-') {Some((base,pre)) if !pre.is_empty()=>base,Some(_)=>return false,None=>s};
    base.split('.').count()==3 && base.split('.').all(|p|!p.is_empty()&&p.bytes().all(|c|c.is_ascii_digit()))
        && s.len()<=80 && s.bytes().all(|c|c.is_ascii_alphanumeric()||b".-".contains(&c))
}
/// Called only by the publishing service. Source clocks are not replaced by board/ingestion time.
pub fn collect(install:&Path, running:&str, service:&str, started:u64, waiting:bool)->Value {
    collect_at(install,running,service,started,waiting,clock())
}
fn collect_at(install:&Path,running:&str,service:&str,started:u64,waiting:bool,now:u64)->Value {
    let tool=crate::platform::tool(install);
    let selected=match modified(&install.join("current")).map(|at|(tool.current().filter(|v|version(v)),at)) {
        Some((Some(v),at)) if at<=now=>Field::known(Source::Current,&v,at),
        _=>Field::unknown(Source::Current,"selection unavailable or invalid"),
    };
    let mut health=Field::unknown(Source::Healthy,"no current consumer health receipt");
    if version(running) {
        if let Some(at)=modified(&install.join("releases").join(running).join("healthy")).filter(|at|*at<=now) {
            health=Field::known(Source::Healthy,"healthy",at);
            if at<=started {health.state=State::LastKnown;health.reason=Some("receipt predates or cannot order process start".into());}
        }
    }
    let lifecycle=if waiting {Field::known(Source::Consumer,"waiting",now)}
        else if health.state==State::Ok {Field::known(Source::Consumer,"healthy",health.at.unwrap())}
        else if selected.value.as_deref()==Some(running) {Field::known(Source::Consumer,"starting",now)}
        else {Field::unknown(Source::Consumer,"no supported lifecycle evidence")};
    serde_json::to_value(Diagnostics {schema:1,service:service.into(),observed:now,
        running:if version(running) {Field::known(Source::Binary,running,now)}else{Field::unknown(Source::Binary,"running version invalid")},
        selected,available:Field::unknown(Source::ReleaseCheck,"release check receipt not retained"),health,lifecycle,
        coreq:Field::known(Source::CargoLock,env!("TASKQ_COREQ_VERSION"),now),
    }).unwrap()
}
/// Parse only the bounded allowlist. Old heartbeats remain valid and report diagnostics unknown.
pub fn read(body:&str,updated:i64,now:i64,ttl:i64,failed:bool)->Value {
    let unknown=||json!({"state":"unknown","reason":"service diagnostics missing or invalid"});
    let mut lines=body.lines().filter_map(|l|l.strip_prefix("diagnostics "));
    let Some(text)=lines.next().filter(|t|t.len()<=4096) else {return unknown()};
    if lines.next().is_some() {return unknown()}
    let Ok(mut d)=serde_json::from_str::<Diagnostics>(text) else {return unknown()};
    let service=body.lines().filter_map(|l|l.strip_prefix("service ")).collect::<Vec<_>>();
    if d.schema!=1 || d.service.len()!=12 || !d.service.bytes().all(|c|c.is_ascii_hexdigit()) || service!=[d.service.as_str()]
        || d.observed==0 || d.observed>i64::MAX as u64 {return unknown()}
    // Never forward arbitrary producer strings, even from hand-authored heartbeat JSON.
    for field in [&d.running,&d.selected,&d.available,&d.coreq] {
        if field.value.as_deref().is_some_and(|v|!version(v)) {return unknown()}
    }
    if d.health.value.as_deref().is_some_and(|v|v!="healthy")
        || d.lifecycle.value.as_deref().is_some_and(|v|!matches!(v,"waiting"|"starting"|"healthy")) {return unknown()}
    for (field,source) in [(&d.running,Source::Binary),(&d.selected,Source::Current),(&d.available,Source::ReleaseCheck),(&d.health,Source::Healthy),(&d.lifecycle,Source::Consumer),(&d.coreq,Source::CargoLock)] {
        if field.source!=source {return unknown()}
        if field.reason.as_deref().is_some_and(|s|!matches!(s,
            "selection unavailable or invalid"|"no current consumer health receipt"|"receipt predates or cannot order process start"|
            "no supported lifecycle evidence"|"running version invalid"|"release check receipt not retained")) {return unknown()}
        if field.at.is_some_and(|at|at>d.observed) || (field.state==State::Ok&&(field.value.is_none()||field.at.is_none())) {return unknown()}
    }
    let age=now.checked_sub(updated);
    // Producer timestamps have a separate clock: a future producer clock is unknown, never fresh.
    let producer_age=now.checked_sub(d.observed as i64);
    let invalid=failed||age.is_none_or(|a|a<0)||producer_age.is_none_or(|a|a<0);
    let stale=age.is_some_and(|a|a>ttl)||producer_age.is_some_and(|a|a>ttl);
    if invalid||stale {
        for field in [&mut d.running,&mut d.selected,&mut d.available,&mut d.health,&mut d.lifecycle,&mut d.coreq] {
            field.state=if invalid {State::Unknown}else{State::Stale};
            field.reason=Some(if failed {"board read failed"}else if invalid {"observation clock unknown"}else{"host observation stale"}.into());
        }
        // Historical values remain, but no current health assertion survives loss of source freshness.
        d.health.value=None;
    }
    let binary=read_binary(body,&d.service,updated,now,ttl,failed);
    let mut value=serde_json::to_value(d).unwrap();value["binary_evidence"]=binary;value
}

#[cfg(test)]
mod tests {
    use super::*;
    // A4/A6: installed/selected health and fresh board writes cannot upgrade an old running observation.
    #[test]
    fn advisory_binary_requires_artifact_native_identity_and_both_clocks() {
        let service="abcdef012345";
        let e=BinaryEvidence {schema:1,service:service.into(),observed:1000,revision:Some("a".repeat(40)),digest:Some("b".repeat(64)),
            pid:Some(123),started:Some("invented-start".into()),generation:Some("invented-generation".into()),native_observed:Some(1000),state:"binary_ready".into(),reason:None};
        let body=format!("binary-evidence {}",serde_json::to_string(&e).unwrap());
        assert_eq!(read_binary(&body,service,1000,1000,300,false)["state"],"binary_ready");
        for (board,now,failed) in [(1000,1301,false),(1301,1301,false),(1000,999,false),(1001,1000,false),(1000,1000,true)] {
            assert_eq!(read_binary(&body,service,board,now,300,failed)["state"],"unknown");
        }
        let mut partial=serde_json::to_value(&e).unwrap();partial["digest"]=Value::Null;partial["state"]=json!("unknown");
        partial["reason"]=json!("running artifact or native generation unproved");
        let partial=read_binary(&format!("binary-evidence {partial}"),service,1000,1000,300,false);
        assert_eq!(partial["state"],"unknown");assert_eq!(partial["revision"],"a".repeat(40));assert!(partial["digest"].is_null());
        for key in ["revision","digest","pid","started","generation","native_observed"] {
            let mut bad=serde_json::to_value(&e).unwrap();bad[key]=Value::Null;
            assert_eq!(read_binary(&format!("binary-evidence {bad}"),service,1000,1000,300,false)["state"],"unknown","{key}");
        }
        assert_eq!(read_binary(&body,"000000000000",1000,1000,300,false)["state"],"unknown");
        assert_eq!(read_binary(&format!("{body}\n{body}"),service,1000,1000,300,false)["state"],"unknown");
        assert_eq!(read_binary("version 9.9.9\nselected 9.9.9",service,1000,1000,300,false)["state"],"unknown");
        let root=std::env::temp_dir().join(format!("taskq-advisory-binary-{}",crate::runtime::uuid()));
        fs::create_dir_all(root.join("releases/9.9.9")).unwrap();
        fs::write(root.join("current"),"9.9.9").unwrap();fs::write(root.join("releases/9.9.9/healthy"),"").unwrap();
        assert_eq!(binary(&root,service,None)["state"],"unknown","installed markers cannot prove this running service");
        fs::remove_dir_all(root).unwrap();
    }
    // D18: selection/health sources and clocks, unavailable sources, malformed input and stale health.
    #[test]
    fn projection_preserves_source_clocks_and_never_invents_health() {
        let root=std::env::temp_dir().join(format!("taskq-versions-{}",std::process::id()));
        let _=fs::remove_dir_all(&root);fs::create_dir_all(root.join("releases/4.6.19")).unwrap();
        let id="abcdef012345";let now=clock();
        let missing=collect_at(&root,"4.6.19",id,now,false,now);
        assert_eq!(missing["selected"]["state"],"unknown");assert!(missing["health"]["value"].is_null());
        fs::write(root.join("current"),"4.6.19").unwrap();fs::write(root.join("releases/4.6.19/healthy"),"").unwrap();
        let now=clock();let value=collect_at(&root,"4.6.19",id,now.saturating_sub(1),false,now);
        let body=format!("version 4.6.19\nservice {id}\ndiagnostics {value}");
        let fresh=read(&body,now as i64,now as i64,300,false);
        assert_eq!(fresh["selected"]["value"],"4.6.19");assert_eq!(fresh["health"]["state"],"ok");
        assert_eq!(fresh["coreq"]["value"],env!("TASKQ_COREQ_VERSION"));assert!(fresh["available"]["value"].is_null());
        for (at,failed) in [(now+301,false),(now,true)] {
            let stale=read(&body,now as i64,at as i64,300,failed);
            assert!(stale["health"]["value"].is_null());assert_eq!(stale["selected"]["at"],fresh["selected"]["at"]);
        }
        assert_eq!(collect_at(&root,"4.6.19",id,now,false,now)["health"]["state"],"last_known");
        let old=collect_at(&root,"4.6.19",id,now+1,false,now+1);
        assert_eq!(old["health"]["state"],"last_known");assert_eq!(old["lifecycle"]["value"],"starting");
        assert_eq!(collect_at(&root,"4.6.18",id,now,true,now)["lifecycle"]["value"],"waiting");
        for body in ["", "diagnostics {}", &format!("{body}\ndiagnostics {{}}"), &body.replace(id,"bad")] {
            assert_eq!(read(body,now as i64,now as i64,300,false)["state"],"unknown");
        }
        for (key,replacement) in [("schema",json!(2)),("service",json!("000000000000"))] {
            let mut bad=value.clone();bad[key]=replacement;
            assert_eq!(read(&format!("service {id}\ndiagnostics {bad}"),now as i64,now as i64,300,false)["state"],"unknown");
        }
        assert_eq!(read(&body,now as i64,now as i64-1,300,false)["running"]["state"],"unknown");
        let mut bad=value.clone();bad["selected"]["reason"]=json!("private metadata");
        assert_eq!(read(&format!("service {id}\ndiagnostics {bad}"),now as i64,now as i64,300,false)["state"],"unknown");
        fs::write(root.join("current"),"../../secret").unwrap();
        assert!(collect_at(&root,"4.6.19",id,now,false,now)["selected"]["value"].is_null());
        fs::remove_dir_all(root).unwrap();
    }
}
