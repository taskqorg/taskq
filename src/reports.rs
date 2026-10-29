//! §7 Reports: a separate projection of closed board objects. Never a queue state or local store (R1).
use crate::{board::{Board, Comment, Issue}, core::Entry, Res};
use serde::Deserialize;
use serde_json::{Value,json};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    title:String, goal:String, acceptance:Vec<String>,
    #[serde(default)] scope:Vec<String>,
    #[serde(rename="type")] kind:Option<String>,
    runtime:Option<String>, priority:Option<String>,
}
impl Proposal {
    fn task(&self,n:u64)->crate::NewTask {
        crate::NewTask {title:self.title.clone(),goal:self.goal.clone(),acceptance:self.acceptance.clone(),scope:self.scope.clone(),
            deps:vec![n],kind:self.kind.clone(),runtime:self.runtime.clone(),priority:self.priority.clone(),host:None,labels:vec![]}
    }
}

/// The document is data. No HTML, runtime instructions, paths or authority are executed from it.
pub fn proposals(body:&str)->Res<Vec<Proposal>> {
    if body.len()>256*1024 {return Err("report exceeds 256 KiB".into());}
    for heading in ["Question","Findings","Options","Recommendation","Proposed tasks"] {
        if !body.lines().any(|l|l.trim_start_matches('#').trim().eq_ignore_ascii_case(heading)) {return Err(format!("report missing {heading} heading"));}
    }
    let mut blocks=body.split("```taskq-tasks\n");blocks.next();
    let rest=blocks.next().ok_or("report missing taskq-tasks JSON array")?;
    if blocks.next().is_some() {return Err("report has multiple taskq-tasks arrays".into());}
    let (data,_)=rest.split_once("```").ok_or("report taskq-tasks fence is not closed")?;
    let tasks:Vec<Proposal>=serde_json::from_str(data).map_err(|e|format!("report proposals invalid: {e}"))?;
    if tasks.len()>8 {return Err("report proposes more than eight tasks".into());} Ok(tasks)
}

fn log(comments:&[Comment])->Vec<Entry> {
    comments.iter().filter(|c|c.trusted).map(|c|Entry{id:c.id,body:c.body.clone(),edited:c.edited}).collect()
}

/// Markers never alter fold or release a dependency. The first terminal decision for this SHA wins.
pub fn project(i:&Issue,closed:i64,comments:&[Comment],url:String)->Option<Value> {
    if i.open||!i.trusted||i.reason.as_deref().is_some_and(|r|r!="completed")
        ||!i.labels.iter().any(|l|l=="taskq")||!i.labels.iter().any(|l|l=="research"||l=="report") {return None;}
    let t=crate::core::project(i.number,&log(comments),false).task;
    let sha=t.result?;
    // Legacy research with an already answered decision must not reappear as unread work.
    if !i.labels.iter().any(|l|l=="report") && comments.iter().any(|c|c.trusted&&!c.edited&&c.body.lines().next().is_some_and(|l|l=="taskq: answer"||l.starts_with("taskq: answer "))) {return None;}
    let mut sorted:Vec<_>=comments.iter().filter(|c|c.trusted&&!c.edited).collect();sorted.sort_by_key(|c|c.id);
    let (mut read,mut decision,mut made)=(false,"none",json!([]));
    for c in sorted {
        if matches!(decision,"drop"|"made") {break;}
        let line=c.body.lines().next().unwrap_or("");
        for (kind,value) in [("read","none"),("later","later"),("drop","drop"),("made","made")] {
            if line==format!("taskq: report-{kind} {sha}") {
                if kind=="made" {
                    let Some(tasks)=c.body.split_once('\n').and_then(|(_,s)|serde_json::from_str::<Vec<u64>>(s).ok()).filter(|v|!v.is_empty()&&v.len()<=8&&v.iter().all(|n|*n>0)) else {continue;};
                    made=json!(tasks);
                }
                read=true;if kind!="read" {decision=value;}
            }
        }
    }
    Some(json!({"n":i.number,"title":i.title,"url":url,"sha":sha,"closed":closed,"read":read,
        "decision":decision,"visible":!matches!(decision,"drop"|"made"),"tasks_made":made}))
}

pub fn list(b:&dyn Board,page:usize)->Res<Value> {
    let mut rows=Vec::new();
    for (i,closed) in b.research_tasks()? {
        if let Some(row)=project(&i,closed,&b.comments(i.number)?,b.url(i.number)) {
            if row["visible"]==true {rows.push(row);}
        }
    }
    rows.sort_by_key(|r|(std::cmp::Reverse(r["closed"].as_i64().unwrap_or(0)),std::cmp::Reverse(r["n"].as_u64().unwrap_or(0))));
    let start=page.saturating_sub(1).saturating_mul(crate::board::CLOSED_PAGE);
    Ok(json!({"reports":rows.iter().skip(start).take(crate::board::CLOSED_PAGE).collect::<Vec<_>>(),"more":rows.len()>start.saturating_add(crate::board::CLOSED_PAGE),"page":page}))
}

fn current(b:&dyn Board,n:u64,sha:&str)->Res<Value> {
    if sha.len()!=40||!sha.bytes().all(|b|b.is_ascii_hexdigit()) {return Err("report SHA must be full 40-hex".into());}
    let i=b.issue(n)?;
    let row=project(&i,0,&b.comments(n)?,b.url(n)).ok_or("not a completed research report")?;
    if row["sha"]!=sha {return Err("report changed; refresh before deciding".into());} Ok(row)
}

fn mark(b:&dyn Board,n:u64,sha:&str,kind:&str,made:&[u64])->Res<Value> {
    let row=current(b,n,sha)?;
    if row["visible"]==false||kind=="read"&&row["read"]==true||kind=="later"&&row["decision"]=="later" {return Ok(row);}
    let text=format!("taskq: report-{kind} {sha}{}",if kind=="made" {format!("\n{}",json!(made))} else {String::new()});
    let id=b.comment(n,&text)?;
    let comments=b.comments(n)?;
    if !comments.iter().any(|c|c.id==id&&c.trusted&&!c.edited&&c.body==text) {return Err("report write readback unknown; refresh before retrying".into());}
    current(b,n,sha)
}

pub fn act(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str)->Res<Value> {
    if crate::dispatch::agent() {return Err("report decisions require the owner CLI, outside an attempt".into());}
    owner_act(b,root,raw,n,sha,action)
}

fn owner_act(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str)->Res<Value> {
    if !["open","read","later","drop","make"].contains(&action) {return Err("report action must be open, read, later, drop or make".into());}
    let prior=current(b,n,sha)?;
    if prior["visible"]==false {return Ok(json!({"report":prior}));}
    if matches!(action,"read"|"later"|"drop") {
        return Ok(json!({"report":mark(b,n,sha,action,&[])?}));
    }
    let body=b.source(&format!("docs/research/{n}.md"),sha)?.replace("\r\n","\n");
    let tasks=proposals(&body)?;
    // Validate all proposals before the first external write; task_body retains §1/routing/limit guards.
    for t in &tasks {crate::task_body(b,&t.task(n),&crate::Op::default())?;}
    if action=="open" {
        let mut row=mark(b,n,sha,"read",&[])?;row["body"]=json!(body);row["tasks"]=json!(tasks);
        return Ok(json!({"report":row}));
    }
    if tasks.is_empty() {return Err("report proposes no follow-up tasks".into());}
    let mut made=vec![];
    for (k,t) in tasks.iter().enumerate() {
        // §1 immutable claims fence duplicate clicks, uncertain POSTs and restart; no receipt file.
        let op=crate::Op{id:Some(format!("report.{n}.{sha}.{}",k+1)),revision:None};
        let receipt=crate::add_task(b,root,raw,&t.task(n),&op)?;
        if !receipt.ok() {return Ok(json!({"report":current(b,n,sha)?,"reason":receipt.reason,"receipts":made,"receipt":receipt}));}
        made.push(receipt.task);
    }
    Ok(json!({"report":mark(b,n,sha,"made",&made)?}))
}

/// A result with a committed report is discoverable after close; a label is only a search hint (R1).
pub fn prepare(b:&dyn Board,root:&Path,n:u64,sha:&str)->Res<bool> {
    let research=b.issue(n)?.labels.iter().any(|l|l=="research");
    let spec=format!("{sha}:docs/research/{n}.md");
    let out=crate::platform::output("reports.candidate",std::process::Command::new("git").current_dir(root).args(["cat-file","-e",&spec]),std::time::Duration::from_secs(15))?;
    if !research&&!out.status.success() {return Ok(false);}
    let body=b.source(&format!("docs/research/{n}.md"),sha)?.replace("\r\n","\n");
    for task in proposals(&body)? {crate::task_body(b,&task.task(n),&crate::Op::default())?;}
    Ok(true)
}

/// Runs only after result fencing/readback succeeded. A stale worker must never change discovery labels.
pub fn discover(b:&dyn Board,n:u64)->Res<()> {
    b.labels(n,&["report".into()],&[])?;
    if !b.issue(n)?.labels.iter().any(|l|l=="report") {return Err("report discovery label readback unknown".into());} Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->(Issue,Vec<Comment>) {
        let i=Issue{number:12,trusted:true,open:false,reason:Some("completed".into()),labels:vec!["taskq".into(),"research".into()],..Default::default()};
        let c=|id,body:String|Comment{id,body,trusted:true,..Default::default()};
        (i,vec![c(1,"taskq: spawn worker invented:session host".into()),c(2,format!("taskq: result {} attempt 1","a".repeat(40))),c(3,"taskq: close attempt 1".into())])
    }
    // Chain: §7 -> closed issue + trusted markers -> Reports projection. Risk: reports alter the queue, stale
    // or untrusted decisions hide a new report, or reading changes close-time ordering.
    #[test] fn decisions_are_separate_from_queue_and_bound_to_the_result() {
        let (i,mut comments)=fixture();let sha="a".repeat(40);
        let row=|cs:&[Comment]|project(&i,45,cs,"invented".into()).unwrap();
        assert_eq!(row(&comments)["decision"],"none");assert_eq!(row(&comments)["read"],false);
        for (id,kind,decision,visible) in [(4,"read","none",true),(5,"later","later",true),(6,"drop","drop",false),(7,"made","drop",false)] {
            comments.push(Comment{id,body:format!("taskq: report-{kind} {sha}"),trusted:true,..Default::default()});
            let r=row(&comments);assert_eq!(r["decision"],decision);assert_eq!(r["visible"],visible);assert_eq!(r["closed"],45);
            assert_eq!(crate::core::project(12,&log(&comments),false).task.state,crate::core::State::Closed);
        }
        let (i,mut cs)=fixture();
        for (id,trusted,edited,s) in [(4,false,false,sha.clone()),(5,true,true,sha.clone()),(6,true,false,"b".repeat(40))] {
            cs.push(Comment{id,body:format!("taskq: report-drop {s}"),trusted,edited,..Default::default()});
        }
        assert_eq!(project(&i,45,&cs,"invented".into()).unwrap()["read"],false);
        cs.push(Comment{id:7,body:format!("taskq: report-made {sha}\n[20,21]"),trusted:true,..Default::default()});
        let r=project(&i,45,&cs,"invented".into()).unwrap();assert_eq!(r["visible"],false);assert_eq!(r["tasks_made"],json!([20,21]));
    }
    // Chain: §7 -> report JSON -> §1 task validation. Risk: an invalid proposal partially creates tasks or grants host policy.
    #[test] fn report_requires_reviewable_findings_and_data_only_proposals() {
        let body="# Question\nQ\n# Findings\nEvidence\n# Options\nPros/cons\n# Recommendation\nR\n# Proposed tasks\n```taskq-tasks\n[]\n```";
        assert!(proposals(body).unwrap().is_empty());
        assert!(proposals(&body.replace("[]","[{\"title\":\"X\",\"goal\":\"G\",\"acceptance\":[\"A\"],\"permission_mode\":\"full\"}]")).is_err());
        assert!(proposals(&body.replace("# Findings","# Missing")).is_err());
    }

    fn board()->(crate::board::Fake,u64,u64,String) {
        let b=crate::board::Fake::default();
        let claim=b.create("Invented create claims","",&[]).unwrap();
        let n=b.create("Retry research","",&["taskq".into(),"research".into(),"report".into()]).unwrap();
        let spawn=b.raw(n,"taskq: spawn worker invented:session host",true,false);
        let sha="a".repeat(40);
        b.raw(n,&format!("taskq: result {sha} attempt {spawn}"),true,false);
        b.raw(n,&format!("taskq: close attempt {spawn}"),true,false);b.close(n).unwrap();
        let body="# Question\nQ\n# Findings\nEvidence\n# Options\nPros/cons\n# Recommendation\nR\n# Proposed tasks\n```taskq-tasks\n[{\"title\":\"Document limits\",\"goal\":\"Show retry limits.\",\"acceptance\":[\"Help names retry limits.\"],\"type\":\"docs\"},{\"title\":\"Protect retries\",\"goal\":\"Keep retries bounded.\",\"acceptance\":[\"A failed request stops at its retry limit.\"]}]\n```";
        b.state.borrow_mut().report_sources.push((format!("docs/research/{n}.md"),sha.clone(),body.into()));
        b.state.borrow_mut().report_closed.insert(n,45);(b,claim,n,sha)
    }

    // Chain: §7 Reports -> exact source -> §1 claims/held drafts -> readback. Risk: one click admits tasks,
    // duplicate or uncertain writes duplicate drafts, failed creation discards a report, or Later changes order.
    #[test] fn report_actions_keep_unknowns_and_create_held_drafts_once() {
        let (b,claim,n,sha)=board();let raw=json!({"create_claim_issue":claim});
        let root=std::env::temp_dir().join(format!("taskq-reports-{}",crate::runtime::uuid()));std::fs::create_dir_all(&root).unwrap();
        let run=|action|owner_act(&b,&root,&raw,n,&sha,action).unwrap();
        assert_eq!(list(&b,1).unwrap()["reports"][0]["read"],false);
        assert!(owner_act(&b,&root,&raw,n,&"b".repeat(40),"drop").is_err());
        let opened=run("open");assert_eq!(opened["report"]["read"],true);assert_eq!(opened["report"]["tasks"].as_array().unwrap().len(),2);
        let count=b.comments(n).unwrap().len();run("open");assert_eq!(b.comments(n).unwrap().len(),count,"repeat open is read-only");
        run("later");assert_eq!(list(&b,1).unwrap()["reports"][0]["closed"],45);
        let missing=owner_act(&b,&root,&json!({}),n,&sha,"make").unwrap();assert!(missing["reason"].as_str().unwrap().contains("create_claim_issue"));
        assert_eq!(list(&b,1).unwrap()["reports"].as_array().unwrap().len(),1);
        let result=run("make");let made=result["report"]["tasks_made"].as_array().unwrap();assert_eq!(made.len(),2);
        for child in made {
            let i=b.issue(child.as_u64().unwrap()).unwrap();assert!(i.open);assert!(i.labels.contains(&"q-held".into()));assert!(!i.labels.contains(&"taskq".into()));
            assert!(i.body.contains(&format!("Depends on #{n}")));
            assert_eq!(crate::core::fold(i.number,&crate::board::entries(&b,i.number).unwrap(),false).state,crate::core::State::Held);
        }
        let issues=b.state.borrow().issues.len();run("make");assert_eq!(b.state.borrow().issues.len(),issues);assert!(list(&b,1).unwrap()["reports"].as_array().unwrap().is_empty());
        assert!(!b.issue(n).unwrap().open);assert_eq!(crate::core::fold(n,&crate::board::entries(&b,n).unwrap(),false).state,crate::core::State::Closed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test] fn unread_source_and_lost_writes_never_claim_success_or_repeat_create() {
        let (b,claim,n,sha)=board();let root=std::env::temp_dir().join(format!("taskq-reports-{}",crate::runtime::uuid()));std::fs::create_dir_all(&root).unwrap();
        let raw=json!({"create_claim_issue":claim});
        let source=b.state.borrow_mut().report_sources.pop().unwrap();
        assert!(owner_act(&b,&root,&raw,n,&sha,"open").is_err());assert_eq!(list(&b,1).unwrap()["reports"][0]["read"],false);
        b.state.borrow_mut().report_sources.push(source);
        b.state.borrow_mut().lose_comment_response=true;
        let lost=owner_act(&b,&root,&raw,n,&sha,"make").unwrap();assert_eq!(lost["receipt"]["outcome"],"unknown");
        let retry=owner_act(&b,&root,&raw,n,&sha,"make").unwrap();assert_eq!(retry["receipt"]["outcome"],"unknown");
        assert_eq!(b.state.borrow().issues.len(),2,"an uncertain create claim never permits another create POST");
        assert_eq!(list(&b,1).unwrap()["reports"][0]["read"],false);
        b.state.borrow_mut().lose_comment_response=true;
        assert!(owner_act(&b,&root,&raw,n,&sha,"drop").is_err());assert!(list(&b,1).unwrap()["reports"].as_array().unwrap().is_empty(),"refresh reconciles the committed drop");
        std::fs::remove_dir_all(root).unwrap();
    }
}
