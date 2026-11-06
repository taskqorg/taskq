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
#[path = "service_versions_tests.rs"]
mod tests;
