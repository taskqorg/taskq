//! §2.26–27: routing a suspended question never replaces its attempt or grants an answer.
use crate::{Res, board::{self, Board}, core::{self, State, Task}, dispatch::Op};
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn actor(actor: &str) -> bool { matches!(actor, "owner" | "pm") }

fn local(reference: &str) -> Res<u64> {
    let n=reference.strip_prefix('#').and_then(|n|n.parse::<u64>().ok()).filter(|n|*n>0);
    n.ok_or_else(|| "wait requires a local task reference #N".into())
}

/// Read the reachable graph before creating an edge. Unknown graph identity cannot prove acyclicity.
fn graph(b:&dyn Board,n:u64,path:&mut HashSet<u64>,done:&mut HashSet<u64>)->Res<()> {
    if path.contains(&n) { return Err(format!("dependency cycle at #{n}")); }
    if done.contains(&n) { return Ok(()); }
    path.insert(n);
    let issue=b.issue(n)?;
    if !issue.trusted || !issue.labels.iter().any(|l| matches!(l.as_str(),"taskq"|"q-held")) {
        return Err(format!("#{n} is not a trusted TaskQ task"));
    }
    if !issue.open {path.remove(&n);done.insert(n);return Ok(());}
    let task=core::fold(n,&board::entries(b,n)?,false);
    let mut refs=core::depends(&core::dep_body(&issue.body,&task));
    if let Some(w)=task.wait { refs.extend(core::depends(&format!("Depends on {}",w.blocker))); }
    for dep in refs {
        if dep.repo.is_some() { return Err("dependency graph completion unknown: foreign task log unavailable".into()); }
        graph(b,dep.n,path,done)?;
    }
    path.remove(&n);done.insert(n);Ok(())
}

pub fn acyclic(b:&dyn Board,n:u64)->Res<()> {graph(b,n,&mut HashSet::new(),&mut HashSet::new())}

/// Validate before the shared writer posts; the optional text is exact evidence forwarded onto this task.
pub fn validate(b:&dyn Board,n:u64,t:&Task,kind:&str,args:&[&str],op:&Op,child:bool)->Res<Option<String>> {
    if !matches!(kind,"classify"|"wait"|"wait-ready") { return Ok(None); }
    if child { return Err("only the owner or PM can reconcile a suspended question".into()); }
    if op.revision.is_none() || op.id.is_none() { return Err("reconciliation requires --revision and --op-id".into()); }
    if t.state!=State::Ask && !(kind=="classify"&&t.state==State::Review&&t.card.is_some()) { return Err("reconciliation requires the current question".into()); }
    match kind {
        "classify" if args.len()==1 && actor(args[0]) && t.wait.is_none()=>Ok(None),
        "wait" if args.len()==2 && matches!(args[0],"evidence"|"completion") && t.wait.is_none()=> {
            let m=local(args[1])?;
            let mut path=HashSet::from([n]);
            graph(b,m,&mut path,&mut HashSet::new())?;
            Ok(None)
        },
        "wait-ready"=> {
            let w=t.wait.as_ref().ok_or("no task wait to reconcile")?;
            if args.first().and_then(|id|id.parse::<u64>().ok())!=Some(w.id) { return Err("task wait changed; read the original question again".into()); }
            if w.kind=="completion" {
                if args.len()!=1 { return Err("completion wait does not take evidence in place of closure".into()); }
                let blockers=crate::service::blockers(b,&format!("Depends on {}",w.blocker),None,&mut Default::default());
                if !blockers.is_empty() { return Err(format!("completion is not verified: {}",blockers[0].reason)); }
                return Ok(None);
            }
            let m=local(&w.blocker)?;
            let (source,id)=args.get(1).and_then(|v|v.split_once('.')).ok_or("evidence wait needs --evidence N.COMMENT")?;
            if args.len()!=2 || source.parse::<u64>()!=Ok(m) { return Err("proof must come from the named blocker".into()); }
            let id=id.parse::<u64>().map_err(|_|"invalid evidence comment ID")?;
            let source=b.comments(m)?.into_iter().find(|c|c.id==id&&c.trusted&&!c.edited&&!c.body.trim().is_empty())
                .ok_or("evidence comment is missing, edited or untrusted; wait retained")?;
            let hash=ring::digest::digest(&ring::digest::SHA256,source.body.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>();
            Ok(Some(format!("Evidence source: {}\nEvidence sha256: {hash}\n{}",b.comment_url(m,id),source.body)))
        },
        _=>Err("invalid reconciliation or active task wait".into()),
    }
}

pub fn action(t:&Task,n:u64,revision:u64,blockers:&[crate::service::Blocker])->Value {
    let command=|name:&str|format!("taskq {name} {n} --revision {revision} --op-id <id>");
    if let Some(w)=&t.wait {
        let unknown=blockers.iter().any(|b|b.reason.starts_with("unknown"));
        let service=w.kind=="completion"&&t.publication.is_some();
        return json!({"actor":"dependency","status":if unknown {"unknown"} else if w.kind=="completion"&&blockers.is_empty(){"ready"}else{"blocked"},
            "blocker":w.blocker,"kind":w.kind,"id":w.id,"question_revision":revision,
            "next":if service {"service rechecks completion and resumes saved publication".into()}else if w.kind=="evidence" {format!("{} --evidence <blocker>.<comment>",command("wait-ready"))}else{command("wait-ready")},
            "responsible":if service{"service"}else{"pm"},"reason":if unknown {"blocker completion unknown"}else if w.kind=="evidence"{"PM forwards verified proof before the evidence task publishes"}else{"verify completed closure, then reconcile the saved question"}});
    }
    let actor=if t.state==State::Ask || t.state==State::Review&&t.card.is_some() {t.action_owner.as_deref().unwrap_or("unknown")}else if t.state==State::Waiting {"dependency"}else if t.state==State::Held {"pm"}else {"service"};
    json!({"actor":actor,"status":if actor=="unknown"||blockers.iter().any(|b|b.reason.starts_with("unknown")){"unknown"}else if t.state==State::Waiting&&!blockers.is_empty(){"blocked"}else{"ready"},"responsible":if actor=="unknown"{"pm"}else if actor=="dependency"{"service"}else{actor},
        "next":if t.state==State::Held{format!("taskq start {n}")}else{match actor {"unknown"=>format!("{} --actor owner|pm",command("classify")),"owner"|"pm"=>format!("taskq inbox {n}; answer the current question at revision {revision}"),"dependency"=>"service rechecks verified dependency completion".into(),_=>"follow the current task stage".into()}}})
}

/// R6: observation over the already-read snapshot; never a board write or execution action.
pub fn attention(rows:&mut [crate::snapshot::Row],now:i64,b:&dyn Board) {
    let open=rows.iter().map(|r|r.n).collect::<HashSet<_>>();
    let mut coverage=std::collections::HashMap::new();
    let edges=rows.iter().map(|r| {
        let mut deps=core::depends(&core::dep_body(&r.brief,&r.task));
        if let Some(w)=&r.task.wait {deps.extend(core::depends(&format!("Depends on {}",w.blocker)));}
        let observed=|d:&core::Dep|d.repo.is_none()&&open.contains(&d.n)||r.deps.iter().any(|active|active.dep==*d)||r.task.wait.as_ref().is_some_and(|w|w.blocker==d.name());
        coverage.insert(r.n,deps.iter().all(&observed));
        deps.retain(observed);
        (r.n,deps)
    }).collect::<std::collections::HashMap<_,_>>();
    let reach=|start:u64,reverse:bool| {
        let mut seen=HashSet::new();let mut active=HashSet::new();let mut pending=vec![(start,false)];let mut found=vec![];let mut foreign=vec![];let mut unknown=false;
        while let Some((n,exit))=pending.pop() {
            if exit {active.remove(&n);seen.insert(n);continue;}
            if active.contains(&n){unknown=true;continue;}
            if seen.contains(&n) {continue;}
            active.insert(n);pending.push((n,true));
            let next=if reverse {
                edges.iter().filter(|(_,deps)|deps.iter().any(|d|d.repo.is_none()&&d.n==n)).map(|(n,_)|*n).collect::<Vec<_>>()
            }else {
                let deps=edges.get(&n);unknown|=deps.is_none()||deps.is_some_and(|deps|deps.iter().any(|d|d.repo.is_some()));
                unknown|=coverage.get(&n)!=Some(&true);
                foreign.extend(deps.into_iter().flatten().filter(|d|d.repo.is_some()).cloned());
                deps.into_iter().flatten().filter(|d|d.repo.is_none()).map(|d|d.n).collect()
            };
            if reverse && n!=start || !reverse && n!=start && next.is_empty() {found.push(n);}
            pending.extend(next.into_iter().map(|n|(n,false)));
        }
        unknown|=!reverse&&found.is_empty()&&edges.get(&start).is_some_and(|deps|!deps.is_empty());
        found.sort_unstable();found.dedup();(found,foreign,unknown)
    };
    for r in rows {
        let elapsed=|key:&str|r.timing[key].as_i64().filter(|at|*at>0&&*at<=now).map(|at|now-at);
        let total=elapsed("first");let current=elapsed("current");let transitions=r.timing["changes"].as_u64();
        let flagged=if total.is_some_and(|age|age>3600)||transitions.is_some_and(|n|n>10){Some(true)}
            else if total.is_some()&&transitions.is_some(){Some(false)}else{None};
        let (roots,foreign,unknown)=reach(r.n,false);let (downstream,_,_)=reach(r.n,true);
        let links=|ns:Vec<u64>|ns.into_iter().map(|n|json!({"ref":format!("#{n}"),"url":b.url(n)})).collect::<Vec<_>>();
        let action=r.action();
        let mut roots=links(roots);
        for d in foreign {let root=json!({"ref":d.name(),"url":b.foreign_url(d.repo.as_deref().unwrap(),d.n)});if !roots.contains(&root){roots.push(root);}}
        let mut reasons=vec![];
        if total.is_none(){reasons.push("first accepted execution unknown");}if current.is_none(){reasons.push("current state boundary unknown");}
        if transitions.is_none(){reasons.push("transition count unknown");}if unknown{reasons.push("dependency graph coverage unknown");}
        r.attention=json!({"flagged":flagged,"total_s":total,"current_s":current,"transitions":transitions,
            "source":"accepted worker input receipt (board time)",
            "roots":roots,"downstream":links(downstream),"graph":if unknown{"unknown"}else{"known"},
            "responsible":action["responsible"],"next":action["next"],
            "reason":(!reasons.is_empty()).then(||reasons.join("; "))});
    }
}
