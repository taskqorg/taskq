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
fn version(s:&str)->bool {
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
    serde_json::to_value(d).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
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
