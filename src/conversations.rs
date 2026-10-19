//! Request-backed AI workspace (M7/C1/C2/C4/C7). No agent launches or task state here.
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, collections::BTreeSet};
use crate::operator::{read, dir, ids, answer_path};

fn registered(install: &Path, root: &str) -> Result<PathBuf, String> {
    crate::operator::projects(install).into_iter().find(|(_,p)|p.to_str()==Some(root)).map(|(_,p)|p).ok_or("Project is no longer registered".into())
}
fn valid(id: &str) -> bool { !id.is_empty() && id.bytes().all(|c|c.is_ascii_digit()) }
fn preferences(install: &Path) -> Result<Value, String> {
    let value=read(&install.join("tray-requests.json"));
    if value["state"]=="unknown" {return Err("Conversation preferences unreadable; original kept".into());}
    Ok(if value.is_null(){json!({"draft":"","history":[],"drafts":{}})}else{value})
}
fn save(install: &Path, value: &Value) -> Result<(),String> {
    crate::platform::write_atomic(&install.join("tray-requests.json"), &value.to_string()).map_err(|e|e.to_string())
}
pub fn chain(root: &Path, id: &str) -> Result<Vec<Value>,String> {
    let mut seen=BTreeSet::new();let mut rows=Vec::new();let mut current=id.to_string();
    loop {
        if !valid(&current)||!seen.insert(current.clone()){return Err("Conversation chain invalid; delivery unknown".into());}
        let request=read(&dir(root).join(format!("{current}.json")));
        if !request["text"].is_string(){return Err("Conversation request unavailable; delivery unknown".into());}
        rows.push(json!({"id":current,"text":request["text"],"answer":read(&answer_path(root,&current))}));
        match request["after"].as_str().filter(|s|!s.is_empty()) {Some(after)=>current=after.into(),None=>break}
    }
    rows.reverse();Ok(rows)
}
fn latest(root: &Path, id: &str) -> bool {
    !ids(root).iter().any(|other|read(&dir(root).join(format!("{other}.json")))["after"]==id)
}
fn key(root: &str, first: &str) -> String {format!("{root}#{first}")}
pub fn select(install: &Path, root: &str, id: &str) -> Result<(),String> {
    let path=registered(install,root)?;let rows=chain(&path,id)?;
    // Retain the existing queue records, including earlier replies; opening never queues work.
    for row in &rows {fs::write(dir(&path).join(format!("{}.keep",row["id"].as_str().unwrap())),b"").map_err(|e|e.to_string())?;}
    let mut prefs=preferences(install)?;prefs["selected"]=json!({"root":root,"id":id});save(install,&prefs)
}
pub fn snapshot(install: &Path) -> Value {
    let prefs=match preferences(install){Ok(v)=>v,Err(reason)=>return json!({"error":reason,"conversations":[],"projects":[]})};
    let mut conversations=Vec::new();let projects=crate::operator::projects(install);
    for (name,root) in &projects {
        let all=ids(root);
        let parents:BTreeSet<String>=all.iter().filter_map(|id|read(&dir(root).join(format!("{id}.json")))["after"].as_str().map(String::from)).collect();
        for id in all.iter().rev().filter(|id|!parents.contains(*id)) {
            match chain(root,id) {
                Ok(rows)=>{let first=rows[0]["id"].as_str().unwrap();let draft=prefs["drafts"][key(&root.to_string_lossy(),first)].as_str().unwrap_or("");
                    conversations.push(json!({"root":root,"project":name,"id":id,"first":first,"draft":draft,"chain":rows}));}
                Err(reason)=>conversations.push(json!({"root":root,"project":name,"id":id,"error":reason,"chain":[]})),
            }
        }
    }
    json!({"conversations":conversations,"selected":prefs["selected"],"history":prefs["history"],"draft":prefs["draft"],"projects":projects.iter().map(|(name,root)|json!({"name":name,"root":root})).collect::<Vec<_>>()})
}
pub fn draft(install: &Path, root: &str, id: &str, text: &str) -> Result<(),String> {
    let mut prefs=preferences(install)?;
    if id.is_empty(){prefs["draft"]=json!(text);}else{
        let rows=chain(&registered(install,root)?,id)?;
        if !prefs["drafts"].is_object(){prefs["drafts"]=json!({});}
        prefs["drafts"][key(root,rows[0]["id"].as_str().unwrap())]=json!(text);
    }
    save(install,&prefs)
}
pub fn submit(install: &Path, root: &str, after: &str, text: &str) -> Result<String,String> {
    let root_path=registered(install,root)?;
    if text.trim().is_empty(){return Err("Write a message before sending".into());}
    let mut prefs=preferences(install)?;
    let first=if after.is_empty(){None}else{
        let rows=chain(&root_path,after)?;let answer=&rows.last().unwrap()["answer"];
        if !latest(&root_path,after)||!matches!(answer["state"].as_str(),Some("result"|"refuse"|"clarify"|"applied"))||answer["native_state"]=="unknown" {
            return Err("Request is active or unresolved; refresh and reconcile before sending".into());
        }
        if dir(&root_path).join(format!("{after}.confirm")).exists(){return Err("Confirmation is pending; refresh before sending".into());}
        Some(rows[0]["id"].as_str().unwrap().to_string())
    };
    fs::create_dir_all(dir(&root_path)).map_err(|e|e.to_string())?;
    let mut n=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_millis();
    while dir(&root_path).join(format!("{n}.json")).exists(){n+=1;}
    let id=n.to_string();
    // Marker precedes publication so service retention cannot race an accepted workspace request.
    fs::write(dir(&root_path).join(format!("{id}.keep")),b"").map_err(|e|e.to_string())?;
    let request=json!({"text":text,"after":if after.is_empty(){None}else{Some(after)}});
    crate::platform::write_atomic(&dir(&root_path).join(format!("{id}.json")),&request.to_string()).map_err(|e|e.to_string())?;
    let _=crate::poke(&root_path);
    prefs["selected"]=json!({"root":root,"id":id});
    let history=prefs["history"].as_array().cloned().unwrap_or_default();
    prefs["history"]=json!(std::iter::once(json!(text)).chain(history.into_iter().filter(|v|v.as_str()!=Some(text))).take(crate::operator::KEEP).collect::<Vec<_>>());
    if let Some(first)=first {if !prefs["drafts"].is_object(){prefs["drafts"]=json!({});}prefs["drafts"][key(root,&first)]=json!("");}else{prefs["draft"]=json!("");}
    // A preferences failure after queue publication must never invite automatic resubmission.
    save(install,&prefs).map_err(|_|format!("Request {id} accepted; preferences save failed. Reopen history; do not resend."))?;
    Ok(id)
}
pub fn confirm(install: &Path, root: &str, id: &str, rev: u64) -> Result<(),String> {
    let path=registered(install,root)?;chain(&path,id)?;
    let answer=read(&answer_path(&path,id));
    if !latest(&path,id)||answer["state"]!="proposal"||answer["rev"].as_u64()!=Some(rev)||rev==0{return Err("Proposal is stale or no longer confirmable; refresh the exchange".into());}
    let file=dir(&path).join(format!("{id}.confirm"));
    use std::io::Write;
    let mut output=fs::OpenOptions::new().write(true).create_new(true).open(file).map_err(|_|"Confirmation already pending or unavailable; refresh, do not retry".to_string())?;
    write!(output,"{rev}").map_err(|e|e.to_string())?;
    let _=crate::poke(&path);Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // A1-A3/C-A1,2,5,6,7: isolation, reopen without work, retention and fenced apply.
    #[test]
    fn exchange_identity_drafts_and_confirmation() {
        let base=std::env::temp_dir().join(format!("taskq-conversations-{}",std::process::id()));let _=fs::remove_dir_all(&base);
        let root=base.join("project");let install=base.join("install");fs::create_dir_all(&install).unwrap();fs::create_dir_all(dir(&root)).unwrap();
        fs::write(install.join("projects"),root.to_str().unwrap()).unwrap();let r=root.to_str().unwrap();
        assert!(submit(&install,"unregistered","","no permission").is_err());
        let a=submit(&install,r,"","first").unwrap();let b=submit(&install,r,"","second").unwrap();
        draft(&install,r,&a,"draft A").unwrap();draft(&install,r,&b,"draft B").unwrap();
        let before=ids(&root);select(&install,r,&a).unwrap();select(&install,r,&a).unwrap();assert_eq!(ids(&root),before);
        assert!(submit(&install,r,&a,"duplicate").is_err());
        fs::write(answer_path(&root,&a),json!({"state":"result","text":"Full reply\nEND","rev":1}).to_string()).unwrap();
        let next=submit(&install,r,&a,"follow up").unwrap();assert_eq!(chain(&root,&next).unwrap().len(),2);
        assert!(submit(&install,r,&a,"stale view").is_err());
        fs::write(answer_path(&root,&next),json!({"state":"proposal","rev":2,"ops":[]}).to_string()).unwrap();
        assert!(confirm(&install,r,&next,1).is_err());confirm(&install,r,&next,2).unwrap();assert!(confirm(&install,r,&next,2).is_err());
        fs::write(answer_path(&root,&next),json!({"state":"apply failed","rev":3,"readback":["one applied","two unknown"]}).to_string()).unwrap();
        assert!(submit(&install,r,&next,"retry").is_err());assert!(confirm(&install,r,&next,3).is_err());
        let snapshot=snapshot(&install);assert_eq!(snapshot["conversations"].as_array().unwrap().len(),2);
        assert!(snapshot.to_string().contains("draft B"));assert!(snapshot.to_string().contains("Full reply"));
        for n in 0..25 {submit(&install,r,"",&format!("message {n}")).unwrap();}
        crate::operator::step(&root,&install,Path::new("unused"),None);
        assert_eq!(chain(&root,&next).unwrap().len(),2,"retained exchange survives command history limit");
        assert_eq!(preferences(&install).unwrap()["history"].as_array().unwrap().len(),20);
        let _=fs::remove_dir_all(base);
    }
}
