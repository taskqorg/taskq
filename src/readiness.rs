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
    #[serde(default)] pub ci_run_id: Option<u64>,
    #[serde(default)] pub ci_run_attempt: Option<u64>,
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
        || p.task.result.is_none() || !hex(sha,40) {return None;}
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
pub fn unavailable(n:u64,url:&str,reason:&str)->Value {
    json!({"schema":1,"state":"unknown","reason":reason,"accepted_result_state":"unknown",
        "task":n,"url":url,"result_entry":null,"attempt":null,"result_sha":null,"current_result_sha":null,"evidence":null})
}
pub fn source(n: u64, url: &str, log: &[Entry]) -> Value {
    let current_sha=core::fold(n,log,false).result;
    let unknown=|reason| {let mut v=unavailable(n,url,reason);v["current_result_sha"]=json!(current_sha);v};
    let Some(current)=identity(n,url,log) else {return unknown("accepted result identity unavailable or replaced")};
    let e=retained(n,log,&current).unwrap_or(current);
    let reason=if current_sha.as_deref()!=Some(e.result_sha.as_str()) {Some("head moved or rebase coverage unproved")}else{match (&e.frozen,&e.landed) {
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
    }};
    // Only bounded typed public identities survive; malformed producer values never reach consumers.
    let mut v=json!({"schema":1,"accepted_result_state":"proven","url":url,"current_result_sha":current_sha,"state":if reason.is_none(){"source_ready"}else{"unknown"},"reason":reason,
        "task":n,"result_entry":e.result_entry,"attempt":e.attempt,"result_sha":e.result_sha});
    let safe=|p:&Publication|p.pr>0&&p.repository.split_once('/').is_some_and(|(a,b)|[a,b].iter().all(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_.".contains(&b))))
        &&hex(&p.head,40)&&[&p.base,&p.integration,&p.merged,&p.parent,&p.ci_sha].iter().all(|v|v.as_deref().is_none_or(|v|hex(v,40)))
        &&p.ci_kind.as_deref().is_none_or(|k|matches!(k,"merge_group"|"private_exact_sha"|"exact_sha"))
        &&p.ci_run_id.is_none_or(|n|n>0)&&p.ci_run_attempt.is_none_or(|n|n>0);
    let valid=e.frozen.as_ref().is_none_or(safe)&&e.landed.as_ref().is_none_or(safe);
    if !valid {v["state"]=json!("unknown");v["reason"]=json!("publication identity invalid");}
    v["evidence"]=if valid {serde_json::to_value(e).unwrap()}else{Value::Null};
    v
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
