//! R12 advisory source evidence; never consulted by admission or build demand.
use crate::core::{self, Entry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub repository: String,
    pub pr: u64,
    pub base: Option<String>,
    pub head: String,
    pub integration: Option<String>,
    pub merged: Option<String>,
    pub parent: Option<String>,
    pub ci_kind: Option<String>,
    pub ci_sha: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema: u8, task: u64, url: String, result_entry: u64, attempt: u64, result_sha: String,
    frozen: Option<Publication>, landed: Option<Publication>,
}
pub fn hex(s: &str, size: usize) -> bool {s.len()==size && s.bytes().all(|b|b.is_ascii_hexdigit())}
fn identity(n: u64, url: &str, log: &[Entry]) -> Option<Evidence> {
    let p=core::project(n,log,false);
    let (id,before,after)=p.changes.iter().rev().find(|(id,_,_)|log.iter().any(|e|e.id==*id && core::kind(n,&e.body)=="result"))?;
    let sha=after.result.as_deref()?;
    if before.role.as_deref()!=Some("worker") || before.attempt.is_none()
        || p.task.result.as_deref()!=Some(sha) || !hex(sha,40) {return None;}
    Some(Evidence {schema:1,task:n,url:url.into(),result_entry:*id,attempt:before.attempt?,result_sha:sha.into(),frozen:None,landed:None})
}
fn retained(n: u64, log: &[Entry], current: &Evidence) -> Option<Evidence> {
    let p=core::project(n,log,false);
    log.iter().filter(|e|!e.edited && p.accepts(e.id))
        .filter(|e|matches!(core::kind(n,&e.body),"enqueued"|"close"))
        .filter_map(|entry| {
            let mut lines=entry.body.lines().filter_map(|l|l.strip_prefix("advisory "));
            let text=lines.next().filter(|s|s.len()<=4096)?;
            if lines.next().is_some() {return None;}
            let e=serde_json::from_str::<Evidence>(text).ok()?;
            (e.schema==1 && e.task==n && e.url==current.url && e.result_entry==current.result_entry
                && e.attempt==current.attempt && e.result_sha==current.result_sha).then_some((entry.id,e))
        }).max_by_key(|(id,_)|*id).map(|(_,e)|e)
}
/// Text travels inside ordinary publication receipts; old folds and verdicts keep their authority.
pub fn receipt(n: u64, url: &str, log: &[Entry], observations: Vec<Publication>) -> String {
    let Some(current)=identity(n,url,log) else {return String::new()};
    let mut e=retained(n,log,&current).unwrap_or(current);
    for o in observations {
        if o.merged.is_some() {e.landed=Some(o);}
        else if e.frozen.is_none() {e.frozen=Some(o);}
    }
    if e.frozen.is_none() && e.landed.is_none() {return String::new();}
    format!("advisory {}",serde_json::to_string(&e).unwrap())
}
pub fn source(n: u64, url: &str, log: &[Entry]) -> Value {
    let unknown=|reason|json!({"state":"unknown","reason":reason});
    let Some(current)=identity(n,url,log) else {return unknown("accepted result identity unavailable or replaced")};
    let Some(e)=retained(n,log,&current) else {return unknown("publication evidence not retained")};
    let reason=match (&e.frozen,&e.landed) {
        (Some(f),Some(l))=> {
            let repo=|s:&str|s.split_once('/').is_some_and(|(o,r)|[o,r].iter().all(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b))));
            if !repo(&f.repository)||f.repository!=l.repository||f.pr==0||f.pr!=l.pr {Some("publication identity mismatch")}
            else if f.head!=e.result_sha||l.head!=f.head {Some("head moved or rebase coverage unproved")}
            else if !f.base.as_deref().is_some_and(|s|hex(s,40))||l.parent!=f.base {Some("frozen base or historical integration coverage unknown")}
            else if !l.merged.as_deref().is_some_and(|s|hex(s,40))||l.integration!=l.merged {Some("integration identity unknown")}
            else if !match l.ci_kind.as_deref() {Some("merge_group")=>l.ci_sha==l.integration,Some("private_exact_sha")=>l.ci_sha.as_deref()==Some(f.head.as_str()),_=>false} {Some("exact-source CI unknown")}
            else {None}
        },
        _=>Some("frozen inputs or successful publication unknown"),
    };
    // Only bounded typed public identities survive; malformed producer values never reach consumers.
    let mut v=json!({"state":if reason.is_none(){"source_ready"}else{"unknown"},"reason":reason,
        "task":n,"result_entry":e.result_entry,"attempt":e.attempt,"result_sha":e.result_sha});
    if reason.is_none() {v["evidence"]=serde_json::to_value(e).unwrap();}
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::entry;
    fn log() -> Vec<Entry> {vec![Entry{id:10,edited:false,body:entry("spawn",&["worker","codex:x","invented"],None,"")},
        Entry{id:11,edited:false,body:entry("result",&[&"a".repeat(40)],Some(10),"Changed: invented\nOpen: none")}, Entry{id:12,edited:false,body:entry("publish",&[],None,"Changed: invented")} ]}
    fn proof() -> Publication {Publication {repository:"invented/repo".into(),pr:3,base:Some("b".repeat(40)),head:"a".repeat(40),
        integration:Some("c".repeat(40)),merged:Some("c".repeat(40)),parent:Some("b".repeat(40)),ci_kind:Some("merge_group".into()),ci_sha:Some("c".repeat(40))}}
    // A2/A3/A6: false readiness can become unsafe build authority in a later consumer.
    #[test]
    fn advisory_source_fences_inputs_and_never_infers_from_close_time() {
        let url="https://github.com/invented/repo/issues/7";
        for case in ["ok","attempt","edited","result","head","ci","base","parent","rebase","close-only","private","repository","pr","wrong-result-attempt","replacement"] {
            let mut log=log(); let mut landed=proof();let mut frozen=landed.clone();
            frozen.integration=None;frozen.merged=None;frozen.parent=None;frozen.ci_kind=None;frozen.ci_sha=None;
            match case {"head"=>landed.head="d".repeat(40),"ci"=>landed.ci_sha=Some("d".repeat(40)),"base"=>frozen.base=None,"repository"=>landed.repository="invented/other".into(),"pr"=>landed.pr=4,
                "parent"=>landed.parent=Some("d".repeat(40)),"private"=>{landed.ci_kind=Some("private_exact_sha".into());landed.ci_sha=Some("a".repeat(40));},_=>{}}
            let text=receipt(7,url,&log,vec![frozen]);
            log.push(Entry{id:13,edited:false,body:entry("enqueued",&["invented-queue"],None,&text)});
            let text=if case=="close-only" {String::new()} else {receipt(7,url,&log,vec![landed])};
            log.push(Entry{id:14,edited:false,body:entry("close",&[],None,&text)});
            let plain=log.iter().map(|e|Entry {body:e.body.lines().filter(|l|!l.starts_with("advisory ")).collect::<Vec<_>>().join("\n"),..e.clone()}).collect::<Vec<_>>();
            assert_eq!(core::fold(7,&log,false),core::fold(7,&plain,false),"advisory cannot change fold");
            match case {"attempt"=>log[0].edited=true,"edited"=>log[1].edited=true,
                "result"=>log[1].body=entry("result",&[&"d".repeat(40)],Some(10),"Changed: replaced"),
                "wrong-result-attempt"=>log[1].body=entry("result",&[&"a".repeat(40)],Some(9),"Changed: stale"),
                "replacement"=>{log.truncate(4);log.push(Entry{id:14,edited:false,body:entry("requeue",&[],None,"")});
                    log.push(Entry{id:15,edited:false,body:entry("spawn",&["worker","codex:y","invented"],None,"")});
                    log.push(Entry{id:16,edited:false,body:entry("result",&[&"a".repeat(40)],Some(15),"Changed: replaced")});},
                "rebase"=>{log.pop();log.push(Entry{id:14,edited:false,body:entry("rebased",&[&"d".repeat(40)],None,"")});},_=>{}}
            assert_eq!(source(7,url,&log)["state"],if matches!(case,"ok"|"private"){"source_ready"}else{"unknown"},"{case}");
        }
    }
}
