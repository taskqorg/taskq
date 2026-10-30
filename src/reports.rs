//! §7 Reports: a separate projection of closed board objects. Never a queue state or local store (R1).
use crate::{board::{Board, Comment, Issue}, core::Entry, Res};
use serde::Deserialize;
use serde_json::{Value,json};
use std::path::Path;
fn now()->i64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs() as i64)}

/// R1/R12: disposable board projection. Negative discoveries are cached too; immutable content is SHA-bound.
#[derive(Default)]
pub struct Cache {
    entries: std::collections::BTreeMap<u64,(i64,i64,Option<Value>)>,
    value: Value,
    verified: Option<i64>,
}
impl Cache {
    fn retaining(value:Value)->Self {
        let entries=value["reports"].as_array().into_iter().flatten().chain(value["history"].as_array().into_iter().flatten())
            .filter_map(|r|r["n"].as_u64().map(|n|(n,(-1,0,Some(r.clone()))))).collect();
        Self{entries,value,..Default::default()}
    }
    pub fn collect(&mut self,b:&dyn Board) {
        let result=coreq::telemetry::capture::operation("reports.collect",|| -> Res<Value> {
            // Start before paging: updates racing a later page must remain inside the next overlap.
            let verified=b.now()?;
            let issues=coreq::telemetry::capture::operation("reports.collect.list",|| -> Res<Vec<(Issue,i64)>> {
                if let Some(since)=self.verified {
                    return Ok(b.issues(Some(since-1))?.into_iter().map(|i|{let closed=i.closed.or_else(||self.entries.get(&i.number).and_then(|(_,_,r)|r.as_ref()).and_then(|r|r["closed"].as_i64())).unwrap_or(i.updated);(i,closed)}).collect());
                }
                let mut all=Vec::new();let mut page=1;
                loop {let (rows,more)=b.closed_tasks(page,None)?;all.extend(rows);if !more {break;}page+=1;}
                Ok(all)
            },|_|"board-read")?;
            let mut next=if self.verified.is_none(){Default::default()}else{self.entries.clone()};let mut errors=Vec::new();
            for (i,closed) in issues {
                let old=self.entries.get(&i.number);
                // Board timestamps have second precision. A read in that same second needs one settled re-read.
                let reusable=i.updated>0 && old.is_some_and(|(at,proof,row)|*at==i.updated && *proof>i.updated && row.as_ref().is_none_or(|r|r["kind"]=="task"||r["reason"].is_null()||r["missing"]==true));
                // #1973: open, untrusted or non-task issues (heartbeats, live tasks) never project; reading their comments
                // each time they move cost most of the board budget.
                let row=if reusable {
                    old.unwrap().2.clone()
                } else if !projectable(&i) {
                    None
                } else {
                    match coreq::telemetry::capture::operation("reports.collect.comments",||b.comments(i.number),|_|"board-read") {
                        Ok(comments)=>project(&i,closed,&comments,b.url(i.number)),
                        Err(e)=>{errors.push(format!("#{}: {e}",i.number));if let Some((_,_,row))=old {next.insert(i.number,(-1,0,row.clone()));}continue;}
                    }
                };
                let row=row.map(|mut row| {
                    if row["kind"]=="report" {
                        let source=old.and_then(|(_,_,r)|r.as_ref()).filter(|r|r["sha"]==row["sha"] && (r["body"].is_string()||r["missing"]==true));
                        let content=match source {Some(r) if r["missing"]==true=>Err(r["reason"].as_str().unwrap_or_default().to_string()),Some(r)=>Ok((r["body"].clone(),r["tasks"].clone())),None=>{
                            coreq::telemetry::capture::operation("reports.collect.source",|| -> Res<(Value,Value)> {
                                let body=b.source(&format!("docs/research/{}.md",i.number),row["sha"].as_str().ok_or("report SHA missing")?)?.replace("\r\n","\n");
                                let tasks=proposals(&body)?;
                                for t in &tasks {crate::task_body(b,&t.task(i.number),&crate::Op::default())?;}
                                Ok((json!(body),json!(tasks)))
                            },|_|"report-source")
                        }};
                        match content {Ok((body,tasks))=>{row["body"]=body;row["tasks"]=tasks;},Err(reason)=>{
                            // #1968: a 404 at an immutable SHA never heals; settle it instead of re-reading every pass.
                            if reason.contains("HTTP 404") {row["missing"]=json!(true);}
                            row["reason"]=json!(reason)}}
                    }row
                });
                if let Some(reason)=row.as_ref().filter(|r|r["kind"]=="report"&&r["missing"]!=true).and_then(|r|r["reason"].as_str()) {errors.push(format!("#{}: {reason}",i.number));}
                next.insert(i.number,(i.updated,if reusable{old.unwrap().1}else{verified},row));
            }
            self.entries=next;
            if errors.is_empty() {self.verified=Some(verified);}
            let mut rows:Vec<Value>=self.entries.values().filter_map(|(_,_,r)|r.clone()).collect();
            rows.sort_by_key(|r|(std::cmp::Reverse(r["closed"].as_i64().unwrap_or(0)),std::cmp::Reverse(r["n"].as_u64().unwrap_or(0))));
            let visible:Vec<_>=rows.iter().filter(|r|r["visible"]==true).collect();let archived:Vec<_>=rows.iter().filter(|r|r["visible"]!=true).collect();
            Ok(json!({"reports":visible,"history":archived,"complete":errors.is_empty(),"reason":(!errors.is_empty()).then(||errors.join("; ")),"observed":now()}))
        },|_|"board-read");
        match result {Ok(value)=>self.value=value,Err(reason)=>{self.value["complete"]=json!(false);self.value["reason"]=json!(reason);}}
    }
}

/// One background read per project; no board handle crosses threads and no collection blocks supervision.
/// #1973: the reader thread keeps its board, so `/user` and conditional replies survive between passes.
#[derive(Default)]
pub struct Collector {
    worker: Option<std::sync::mpsc::Sender<(Cache,std::sync::mpsc::Sender<Cache>)>>,
    cache: Option<Cache>,
    pending: Option<std::sync::mpsc::Receiver<Cache>>,
    started: Option<std::time::Instant>,
    closes: String,
    value: Value,
    invalidated: bool,
    written: Value,
}
impl Collector {
    /// Drain completion from the existing local service tick, without waking a board pass.
    pub fn received(&mut self)->bool {
        if let Some(rx)=&self.pending {
            match rx.try_recv() {
                Ok(cache)=>{self.value=cache.value.clone();self.cache=Some(cache);self.pending=None;return true;},
                Err(std::sync::mpsc::TryRecvError::Disconnected)=>{self.pending=None;self.value["complete"]=json!(false);self.value["reason"]=json!("report collector stopped before readback");return true;},
                Err(_)=>{}
            }
        }
        false
    }
    pub fn tick(&mut self,root:&Path,poll:u64,ttl:i64,enabled:bool)->Value {
        if self.value.is_null() {self.value=cached(root);}
        self.received();
        let closes=std::fs::read_to_string(crate::dot(root).join("events")).unwrap_or_default().lines().filter(|l|l.contains(" close ")).collect::<Vec<_>>().join("\n");
        if closes!=self.closes {self.closes=closes;self.invalidated=true;}
        if enabled && self.pending.is_none() && (self.invalidated || self.started.is_none_or(|t|t.elapsed().as_secs()>=poll.max(1))) {
            self.invalidated=false;self.started=Some(std::time::Instant::now());
            let (tx,rx)=std::sync::mpsc::channel();let job=(self.cache.take().unwrap_or_else(||Cache::retaining(self.value.clone())),tx);
            // A stopped reader hands the job back; one fresh reader replaces it.
            let job=match &self.worker {Some(w)=>w.send(job).err().map(|e|e.0),None=>Some(job)};
            match job {
                None=>self.pending=Some(rx),
                Some(job)=>match reader(root.to_path_buf()) {
                    Ok(worker)=>{let _=worker.send(job);self.worker=Some(worker);self.pending=Some(rx);},
                    Err(e)=>{self.worker=None;self.cache=Some(job.0);self.value["complete"]=json!(false);self.value["reason"]=json!(format!("report collector unavailable: {e}"));}
                }
            }
        }
        // #1975: rows live in their own file; a failed write retries on the next tick and status keeps only counts.
        if self.value!=self.written && crate::platform::write_atomic(&file(root),&self.value.to_string()).is_ok() {self.written=self.value.clone();}
        freshness(self.value.clone(),now(),ttl,None)
    }
}

/// #1975: `<root>/.taskq/reports.json` holds the rows; `status.json` (and so `service status --json`) holds this.
pub fn summary(value:&Value)->Value {
    let rows=value["reports"].as_array();
    json!({"complete":value["complete"],"reason":value["reason"],"observed":value["observed"],"count":rows.map(Vec::len),
        "unread":rows.map(|r|r.iter().filter(|r|r["read"]!=true).count()),"history":value["history"].as_array().map(Vec::len)})
}
fn file(root:&Path)->std::path::PathBuf {crate::dot(root).join("reports.json")}
/// The collected rows; a status written before #1975 still carries them inline until the service rewrites.
pub fn stored(root:&Path,status:&Value)->Value {
    match crate::data::json(&file(root)) {Ok(Some(v))=>v,_ if status["reports"]["reports"].is_array()=>status["reports"].clone(),_=>Value::Null}
}

/// Opens the board on the first job and retries a failed open on the next; config changes apply on service restart.
fn reader(root:std::path::PathBuf)->std::io::Result<std::sync::mpsc::Sender<(Cache,std::sync::mpsc::Sender<Cache>)>> {
    let (go,jobs)=std::sync::mpsc::channel::<(Cache,std::sync::mpsc::Sender<Cache>)>();
    std::thread::Builder::new().name("reports".into()).spawn(move || {
        let mut board:Option<Box<dyn Board>>=None;
        for (mut cache,tx) in jobs {
            if board.is_none() {
                match crate::load(&root).and_then(|(cfg,raw)|cfg.board.open(&cfg.web_host,&cfg.repo,raw["label_schema"].as_str(),&crate::platform::tool(&cfg.install))) {
                    Ok(b)=>board=Some(b),Err(reason)=>{cache.value["complete"]=json!(false);cache.value["reason"]=json!(reason);}
                }
            }
            if let Some(b)=&board {cache.collect(b.as_ref());}
            let _=tx.send(cache);
        }
    })?;
    Ok(go)
}

pub fn freshness(mut value:Value,now:i64,ttl:i64,unavailable:Option<&str>)->Value {
    if !value.is_object() {value=json!({"reports":[],"complete":false,"reason":"Reports not collected yet"});}
    if !value["reports"].is_array() || !value["complete"].is_boolean() {value["complete"]=json!(false);value["reason"]=json!("Report collection invalid; waiting for the service");}
    let reason=unavailable.map(String::from).or_else(|| match value["observed"].as_i64() {
        Some(at) if crate::snapshot::live(now-at,ttl)=>None,
        Some(_)=>Some("Report collection is stale; waiting for the service".into()),
        None=>Some(value["reason"].as_str().unwrap_or("Reports not collected yet").into())
    });
    if let Some(reason)=reason {value["complete"]=json!(false);value["reason"]=json!(reason);}
    value
}

pub fn cached(root:&Path)->Value {
    let result=crate::data::json(&crate::dot(root).join("status.json"));
    let (value,reason)=match result {Ok(Some(s))=>(stored(root,&s),s["error"].as_str().map(String::from)),Ok(None)=>(stored(root,&Value::Null),None),Err(e)=>(Value::Null,Some(e))};
    freshness(value,now(),crate::snapshot::ttl(root),reason.as_deref())
}

pub fn cached_list(root:&Path,page:usize,before:Option<(i64,u64)>)->Res<Value> {
    coreq::telemetry::capture::operation("reports.list.cached",|| -> Res<Value> {
        let mut value=cached(root);let rows=value["reports"].as_array().cloned().unwrap_or_default();
        let rows:Vec<_>=rows.into_iter().filter(|r|before.is_none_or(|p|(r["closed"].as_i64().unwrap_or(0),r["n"].as_u64().unwrap_or(0))<p)).collect();
        let start=if before.is_some(){0}else{page.saturating_sub(1).saturating_mul(crate::board::CLOSED_PAGE)};
        let selected:Vec<_>=rows.iter().skip(start).take(crate::board::CLOSED_PAGE).cloned().collect();
        value["more"]=json!(rows.len()>start.saturating_add(crate::board::CLOSED_PAGE));
        value["next"]=json!(selected.last().map(|r|format!("{}:{}",r["closed"],r["n"])));value["page"]=json!(page);value["reports"]=json!(selected);Ok(value)
    },|_|"cache-read")
}

/// Legacy Completed presentation reads the same projection; its IPC request is never a board request.
pub fn cached_history(root:&Path,m:&Value)->Res<Value> {
    let value=cached(root);let page=m["page"].as_u64().unwrap_or(1).max(1) as usize;
    let mut rows:Vec<Value>=value["reports"].as_array().into_iter().flatten().chain(value["history"].as_array().into_iter().flatten()).filter(|r|
        m["task"].as_u64().is_none_or(|n|r["n"]==n)&&m["since"].as_i64().is_none_or(|t|r["closed"].as_i64().unwrap_or(0)>=t)).cloned().collect();
    rows.sort_by_key(|r|(std::cmp::Reverse(r["closed"].as_i64().unwrap_or(0)),std::cmp::Reverse(r["n"].as_u64().unwrap_or(0))));
    let start=if m["task"].is_number(){0}else{(page-1)*crate::board::CLOSED_PAGE};
    Ok(json!({"source":"service","page":page,"tasks":rows.iter().skip(start).take(crate::board::CLOSED_PAGE).collect::<Vec<_>>(),"more":rows.len()>start+crate::board::CLOSED_PAGE,"reason":value["reason"],"complete":value["complete"]}))
}

pub fn cached_open(root:&Path,n:u64,sha:&str)->Res<Value> {
    coreq::telemetry::capture::operation("reports.open.cached",|| -> Res<Value> {
        if !identity(sha) {return Err("result identity must be full SHA or collected close identity".into());}
        let value=cached(root);
        let row=value["reports"].as_array().into_iter().flatten().chain(value["history"].as_array().into_iter().flatten()).find(|r|r["n"]==n && r["sha"]==sha);
        match row {Some(row)=>Ok(json!({"report":row,"complete":value["complete"],"reason":value["reason"]})),None=>Ok(json!({"reason":value["reason"].as_str().unwrap_or("Report not in the collected snapshot; waiting for the service"),"complete":false}))}
    },|_|"cache-read")
}

/// Reads bypass board construction and token lookup, so offline navigation cannot become a login/API probe.
pub fn read_cli(root:&Path,a:&crate::Args)->Res<Option<Value>> {
    if a.flags.iter().any(|(k,v)|!["action","sha","page","before","json","text"].contains(&k.as_str())||v.len()!=1) {return Err("results: use --action, --sha, --page, --before, --text and --json once each".into());}
    if a.one("text").is_some()&&!matches!(a.one("action"),Some("followup"|"reopen")) {return Err("--text supplies the reason for followup or reopen".into());}
    if a.pos.len()==1 {
        if a.one("action").is_some()||a.one("sha").is_some() {return Err("report action needs a task number".into());}
        let page=a.one("page").unwrap_or("1").parse::<usize>().ok().filter(|p|*p>0).ok_or("report page must be positive")?;
        let before=a.one("before").map(|s|s.split_once(':').and_then(|(t,n)|Some((t.parse::<i64>().ok()?,n.parse::<u64>().ok()?))).ok_or("report cursor must be TIME:N")).transpose()?;
        return cached_list(root,page,before).map(Some);
    }
    if a.one("page").is_some()||a.one("before").is_some() {return Err("--page/--before list reports without a task number".into());}
    if a.one("action").unwrap_or("open")=="open" {return cached_open(root,a.num(1)?,a.need("sha")?).map(Some);}
    Ok(None)
}

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
        crate::NewTask {conversation:None,title:self.title.clone(),goal:self.goal.clone(),acceptance:self.acceptance.clone(),scope:self.scope.clone(),
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

fn projectable(i:&Issue)->bool {!i.open&&i.trusted&&i.labels.iter().any(|l|l=="taskq")}

/// Markers never alter fold or release a dependency. The first terminal decision for this SHA wins.
pub fn project(i:&Issue,closed:i64,comments:&[Comment],url:String)->Option<Value> {
    if !projectable(i) {return None;}
    let p=crate::core::project(i.number,&log(comments),false);
    let t=p.task;
    let result=p.changes.iter().rev().find(|(_,before,after)|before.state==crate::core::State::Doing&&after.state==crate::core::State::Review);
    let handin=result.and_then(|(id,_,_)|comments.iter().find(|c|c.id==*id));
    let registered=handin.is_some_and(|c|c.body.lines().any(|l|l==format!("Report: docs/research/{}.md",i.number)));
    let completed=i.reason.as_deref().is_none_or(|r|r=="completed");
    let report=completed&&(i.labels.iter().any(|l|l=="research")||registered);
    let closing=p.changes.iter().rev().find(|(_,before,after)|before.state!=crate::core::State::Closed&&after.state==crate::core::State::Closed)
        .and_then(|(id,_,_)|comments.iter().find(|c|c.id==*id));
    let sha=t.result.unwrap_or_else(||format!("close-{}",closing.map_or(i.number,|c|c.id)));
    if report&&!identity_sha(&sha) {return None;}
    // Legacy research with an already answered decision must not reappear as unread work.
    let legacy_answered=report&&!registered && comments.iter().any(|c|c.trusted&&!c.edited&&c.body.lines().next().is_some_and(|l|l=="taskq: answer"||l.starts_with("taskq: answer ")));
    let mut sorted:Vec<_>=comments.iter().filter(|c|c.trusted&&!c.edited).collect();sorted.sort_by_key(|c|c.id);
    let (mut read,mut decision,mut made,mut making)=(legacy_answered,if legacy_answered{"answered"}else{"none"},json!([]),false);
    for c in sorted {
        if matches!(decision,"accepted"|"drop"|"made"|"answered") {break;}
        let line=c.body.lines().next().unwrap_or("");
        let prefix=if report{"report"}else{"result"};
        for (kind,value) in [("read","none"),("accept","accepted"),("later","later"),("making","none"),("drop","drop"),("made","made")] {
            if !report&&matches!(kind,"making"|"made") {continue;}
            if line==format!("taskq: {prefix}-{kind} {sha}") {
                if matches!(kind,"drop"|"accept")&&making {continue;}
                if kind=="made" {
                    let Some(tasks)=c.body.split_once('\n').and_then(|(_,s)|serde_json::from_str::<Vec<u64>>(s).ok()).filter(|v|!v.is_empty()&&v.len()<=8&&v.iter().all(|n|*n>0)) else {continue;};
                    made=json!(tasks);
                }
                read=true;if kind=="making" {making=true;} else if kind!="read" {decision=value;}
            }
        }
    }
    let text=|c:Option<&Comment>|c.and_then(|c|c.body.split_once('\n').map(|(_,text)|text)).unwrap_or("").to_string();
    let accepted_close:Vec<_>=closing.into_iter().map(|c|Entry{id:c.id,body:c.body.clone(),edited:false}).collect();
    let mut row=json!({"n":i.number,"title":i.title,"url":url,"sha":sha,"closed":closed,"read":read,"kind":if report{"report"}else{"task"},"state":"closed","outcome":if completed{"success"}else if i.reason.as_deref()==Some("not_planned"){"not_planned"}else{"unknown"},"merged":crate::merge_receipt(&accepted_close),
        "comments_through":comments.iter().filter(|c|c.trusted&&!c.edited).map(|c|c.id).max().unwrap_or(0),
        "conversation":crate::core::conversation(i.number,&log(comments)),
        "decision":decision,"visible":!legacy_answered&&!matches!(decision,"drop"|"made")&&!(report&&decision=="accepted"),"pending_decision":if making&&!matches!(decision,"made"|"drop"|"accepted"){Some("make")}else{None},"tasks_made":made});
    let details=text(handin);let verdict=text(closing);
    row["result"]=json!(details.lines().find_map(|l|l.strip_prefix("Changed: ")));
    row["pr"]=json!(format!("{details}\n{verdict}").split(|c:char|c.is_whitespace()||matches!(c,'('|')'|'['|']'|'<'|'>')).filter(|u|u.starts_with("https://")&&(u.contains("/pull/")||u.contains("/merge_requests/"))).last().map(|u|u.trim_end_matches(|c:char|!c.is_ascii_alphanumeric()).to_string()));
    row["close"]=json!(crate::display::close_outcome(Some(&verdict),None));
    if !report {row["body"]=json!(details);row["verdict"]=json!(verdict);if handin.is_none() {row["reason"]=json!("No accepted hand-in recorded; result details unknown");}}
    Some(row)
}

fn identity_sha(sha:&str)->bool {sha.len()==40&&sha.bytes().all(|b|b.is_ascii_hexdigit())}
fn identity(sha:&str)->bool {identity_sha(sha)||sha.strip_prefix("close-").is_some_and(|id|!id.is_empty()&&id.bytes().all(|b|b.is_ascii_digit()))}

#[cfg(test)]
pub fn list_before(b:&dyn Board,page:usize,before:Option<(i64,u64)>)->Res<Value> {
    let mut rows=Vec::new();
    for (i,closed) in b.research_tasks()? {
        if let Some(row)=project(&i,closed,&b.comments(i.number)?,b.url(i.number)) {
            let position=(row["closed"].as_i64().unwrap_or(0),row["n"].as_u64().unwrap_or(0));
            if row["visible"]==true&&before.is_none_or(|b|position<b) {rows.push(row);}
        }
    }
    rows.sort_by_key(|r|(std::cmp::Reverse(r["closed"].as_i64().unwrap_or(0)),std::cmp::Reverse(r["n"].as_u64().unwrap_or(0))));
    let start=if before.is_some(){0}else{page.saturating_sub(1).saturating_mul(crate::board::CLOSED_PAGE)};
    let selected:Vec<_>=rows.iter().skip(start).take(crate::board::CLOSED_PAGE).collect();
    let next=selected.last().map(|r|format!("{}:{}",r["closed"],r["n"]));
    Ok(json!({"reports":selected,"more":rows.len()>start.saturating_add(crate::board::CLOSED_PAGE),"next":next,"page":page}))
}
#[cfg(test)]
fn list(b:&dyn Board,page:usize)->Res<Value> {list_before(b,page,None)}

fn current(b:&dyn Board,n:u64,sha:&str)->Res<Value> {
    if !identity(sha) {return Err("result identity invalid".into());}
    let i=b.issue(n)?;
    if i.number!=n {return Err("result issue identity mismatch".into());}
    let row=project(&i,0,&b.comments(n)?,b.url(n)).ok_or("not a completed research report")?;
    if row["sha"]!=sha {return Err("report changed; refresh before deciding".into());} Ok(row)
}

fn mark(b:&dyn Board,n:u64,sha:&str,kind:&str,made:&[u64])->Res<Value> {
    let row=current(b,n,sha)?;
    if row["visible"]==false||matches!(row["decision"].as_str(),Some("accepted"|"drop"|"made"|"answered"))||kind=="read"&&row["read"]==true||kind=="later"&&row["decision"]=="later"||kind=="making"&&row["pending_decision"]=="make" {return Ok(row);}
    let prefix=if row["kind"]=="task" {"result"}else{"report"};
    let text=format!("taskq: {prefix}-{kind} {sha}{}",if kind=="made" {format!("\n{}",json!(made))} else {String::new()});
    let id=b.comment(n,&text)?;
    let comments=b.comments(n)?;
    if !comments.iter().any(|c|c.id==id&&c.trusted&&!c.edited&&c.body==text) {return Err("report write readback unknown; refresh before retrying".into());}
    current(b,n,sha)
}

pub fn act(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str)->Res<Value> {
    act_with_reason(b,root,raw,n,sha,action,"")
}
pub fn act_with_reason(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str,reason:&str)->Res<Value> {
    if action=="open" {return cached_open(root,n,sha);}
    if crate::dispatch::agent() {return Err("report decisions require the owner CLI, outside an attempt".into());}
    let result=if matches!(action,"followup"|"reopen") {successor(b,root,raw,n,sha,action,reason)}else {owner_act(b,root,raw,n,sha,action)};
    let _=crate::poke(root);result
}

fn successor(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str,reason:&str)->Res<Value> {
    let row=current(b,n,sha)?;
    if row["pending_decision"]=="make" {return Err("Make tasks already started; reconcile its held drafts before another decision.".into());}
    let reason=reason.trim();
    if reason.is_empty()||reason.chars().count()>180||reason.contains(['\r','\n']) {return Err("Give a one-line reason, at most 180 characters".into());}
    let label=if action=="reopen" {"Reopen"}else{"Follow-up"};
    let task=crate::NewTask{conversation:None,title:format!("{label} #{n}"),
        goal:format!("{label} #{n}: {reason}"),acceptance:vec![format!("The operator's reason is addressed: {reason}")],
        scope:vec![format!("Source: {}",b.url(n)),format!("Reason: {reason}")],deps:vec![n],kind:None,runtime:None,priority:None,host:None,labels:vec![]};
    crate::task_body(b,&task,&crate::Op::default())?;
    let hash=ring::digest::digest(&ring::digest::SHA256,reason.as_bytes());
    let hash:String=hash.as_ref().iter().map(|b|format!("{b:02x}")).collect();
    let op=crate::Op{id:Some(format!("result.{n}.{sha}.{action}.{hash}")),revision:None};
    let receipt=crate::add_task(b,root,raw,&task,&op)?;
    Ok(json!({"report":row,"successor":receipt.task,"successor_url":(receipt.task>0).then(||b.url(receipt.task)),"receipt":receipt,"reason":(receipt.outcome!="accepted").then(||receipt.reason.clone())}))
}

pub(crate) fn owner_act(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str)->Res<Value> {
    if !["open","read","accept","later","drop","make"].contains(&action) {return Err("result action must be open, read, accept, followup, reopen, later, drop or make".into());}
    let prior=current(b,n,sha)?;
    if prior["kind"]=="task" {
        if !matches!(action,"read"|"accept"|"later"|"drop") {return Err("completed task actions are read, accept, later, drop, followup or reopen".into());}
        return Ok(json!({"report":mark(b,n,sha,action,&[])?}));
    }
    if prior["visible"]==false {return Ok(json!({"report":prior}));}
    if matches!(action,"read"|"accept"|"later"|"drop") {
        let row=if matches!(action,"accept"|"drop")&&prior["pending_decision"]=="make" {prior} else {mark(b,n,sha,action,&[])?};
        return Ok(if matches!(action,"accept"|"drop")&&row["pending_decision"]=="make" {json!({"report":row,"reason":"Make tasks already started; reconcile its held drafts before another decision."})} else {json!({"report":row})});
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
    // #1945: a board set up without the key (cyprusbz#540) refused every Make; the owner's Make settles it like claim-issue
    let settled;
    let raw=if raw["create_claim_issue"].is_null() {
        let r=crate::claim_issue_setup(b,root,None).unwrap_or_else(|e|json!({"outcome":"rejected","reason":e,"key":"create_claim_issue"}));
        if r["outcome"]!="accepted" {
            return Ok(json!({"report":prior,"reason":format!("create_claim_issue: {}",r["reason"].as_str().unwrap_or("unknown")),"receipt":r}));
        }
        let mut v=raw.clone();v["create_claim_issue"]=r["issue"].clone();settled=v;&settled
    } else {raw};
    if let Err(receipt)=crate::create_claim_issue(b,raw,&crate::Op::default()) {
        return Ok(json!({"report":prior,"reason":receipt.reason,"receipt":receipt}));
    }
    // The first make-intent or Drop on this issue wins across hosts. Intent is not queue state; an unknown or
    // partial Make stays visible and retries reconcile each immutable item claim before completing the decision.
    let reserved=match mark(b,n,sha,"making",&[]) {Ok(r)=>r,Err(e)=>return Ok(unknown(b,n,sha,&prior,&[],e))};
    if reserved["visible"]==false {return Ok(json!({"report":reserved}));}
    let mut made=vec![];
    for (k,t) in tasks.iter().enumerate() {
        // §1 immutable claims fence duplicate clicks, uncertain POSTs and restart; no receipt file.
        let op=crate::Op{id:Some(format!("report.{n}.{sha}.{}",k+1)),revision:None};
        let receipt=match crate::add_task(b,root,raw,&t.task(n),&op) {Ok(r)=>r,Err(e)=>return Ok(unknown(b,n,sha,&reserved,&made,e))};
        if !receipt.ok() {let mut value=unknown(b,n,sha,&reserved,&made,receipt.reason.clone());value["receipt"]=json!(receipt);return Ok(value);}
        made.push(receipt.task);
    }
    Ok(finish(b,n,sha,&reserved,&made))
}

fn unknown(b:&dyn Board,n:u64,sha:&str,prior:&Value,made:&[u64],reason:String)->Value {
    json!({"report":current(b,n,sha).unwrap_or_else(|_|prior.clone()),"reason":reason,"receipts":made})
}
fn finish(b:&dyn Board,n:u64,sha:&str,prior:&Value,made:&[u64])->Value {
    match mark(b,n,sha,"made",made) {Ok(row)=>json!({"report":row}),Err(e)=>unknown(b,n,sha,prior,made,e)}
}

/// Validate the report before the single accepted result comment records its repository path (R1).
pub fn prepare(b:&dyn Board,root:&Path,n:u64,sha:&str)->Res<bool> {
    let research=b.issue(n)?.labels.iter().any(|l|l=="research");
    let spec=format!("{sha}:docs/research/{n}.md");
    let out=crate::platform::output("reports.candidate",std::process::Command::new("git").current_dir(root).args(["cat-file","-e",&spec]),std::time::Duration::from_secs(15))?;
    if !research&&!out.status.success() {return Ok(false);}
    let body=b.source(&format!("docs/research/{n}.md"),sha)?.replace("\r\n","\n");
    for task in proposals(&body)? {crate::task_body(b,&task.task(n),&crate::Op::default())?;}
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->(Issue,Vec<Comment>) {
        let i=Issue{number:12,trusted:true,open:false,reason:Some("completed".into()),labels:vec!["taskq".into(),"research".into()],..Default::default()};
        let c=|id,body:String|Comment{id,body,trusted:true,..Default::default()};
        (i,vec![c(1,"taskq: spawn worker invented:session host".into()),c(2,format!("taskq: result {} attempt 1","a".repeat(40))),c(3,"taskq: close".into())])
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
        let n=b.create("Retry research","",&["taskq".into(),"docs".into()]).unwrap();
        let spawn=b.raw(n,"taskq: spawn worker invented:session host",true,false);
        let sha="a".repeat(40);
        b.raw(n,&format!("taskq: result {sha} attempt {spawn}\nReport: docs/research/{n}.md"),true,false);
        b.raw(n,"taskq: close",true,false);b.close(n).unwrap();
        let body="# Question\nQ\n# Findings\nEvidence\n# Options\nPros/cons\n# Recommendation\nR\n# Proposed tasks\n```taskq-tasks\n[{\"title\":\"Document limits\",\"goal\":\"Show retry limits.\",\"acceptance\":[\"Help names retry limits.\"],\"type\":\"docs\"},{\"title\":\"Protect retries\",\"goal\":\"Keep retries bounded.\",\"acceptance\":[\"A failed request stops at its retry limit.\"]}]\n```";
        b.state.borrow_mut().report_sources.push((format!("docs/research/{n}.md"),sha.clone(),body.into()));
        b.state.borrow_mut().report_closed.insert(n,45);
        b.state.borrow_mut().issues.iter_mut().find(|i|i.number==n).unwrap().closed=Some(45);
        (b,claim,n,sha)
    }

    // Chain: Product / RC4–RC7 -> report decisions and claims. Risk: Accept hides partial drafts,
    // decided content disappears, or a report correction reopens a completed dependency.
    #[test] fn conversations_1977_decisions_keep_history_and_creation_fences() {
        let (b,claim,n,sha)=board();let raw=json!({"create_claim_issue":claim});
        let root=std::env::temp_dir().join(format!("taskq-1977-{}",crate::runtime::uuid()));std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let accepted=owner_act(&b,&root,&raw,n,&sha,"accept").unwrap();
        assert_eq!(accepted["report"]["decision"],"accepted");assert_eq!(accepted["report"]["read"],true);
        let count=b.comments(n).unwrap().len();owner_act(&b,&root,&raw,n,&sha,"accept").unwrap();assert_eq!(b.comments(n).unwrap().len(),count);
        assert_eq!(owner_act(&b,&root,&raw,n,&sha,"make").unwrap()["report"]["decision"],"accepted");
        let corrected=successor(&b,&root,&raw,n,&sha,"reopen","Keep the original receipt").unwrap();
        let child=corrected["successor"].as_u64().unwrap();assert!(b.issue(child).unwrap().labels.contains(&"q-held".into()));assert!(!b.issue(n).unwrap().open);
        let mut cache=Cache::default();cache.collect(&b);assert!(cache.value["reports"].as_array().unwrap().is_empty());
        assert!(cache.value["history"][0]["body"].is_string(),"decided report content stays readable");
        std::fs::write(crate::dot(&root).join("reports.json"),cache.value.to_string()).unwrap();
        assert!(cached_open(&root,n,&sha).unwrap()["report"]["body"].is_string());
        let (other,oc,on,os)=board();mark(&other,on,&os,"making",&[]).unwrap();
        let pending=owner_act(&other,&root,&json!({"create_claim_issue":oc}),on,&os,"accept").unwrap();
        assert!(pending["reason"].is_string());assert_eq!(pending["report"]["pending_decision"],"make");
        other.raw(on,&format!("taskq: report-accept {os}"),true,false);
        assert_eq!(current(&other,on,&os).unwrap()["pending_decision"],"make","a raced acknowledgement cannot win against a make reservation");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: §7 Results -> trusted completion/seen markers -> held successor with immutable create claim.
    // Risk: acknowledgement reopens a dependency, old markers hide a new SHA, or retry duplicates correction work.
    #[test] fn ordinary_results_acknowledge_and_reopen_without_reopening_dependencies() {
        let (b,claim,n,sha)=board();
        {let mut st=b.state.borrow_mut();
        st.issues.iter_mut().find(|i|i.number==n).unwrap().labels=vec!["taskq".into(),"code".into()];
        let (_,result)=st.comments.iter_mut().find(|(task,c)|*task==n&&c.body.starts_with("taskq: result ")).unwrap();
        result.body=result.body.split_once('\n').unwrap().0.to_string()+"\nChanged: Requests recover once.\nChecks: replay -- passed\nOpen: none\nhttps://example.com/invented-evidence";}
        let root=std::env::temp_dir().join(format!("taskq-results-{}",crate::runtime::uuid()));std::fs::create_dir_all(&root).unwrap();
        let raw=json!({"create_claim_issue":claim});
        let row=current(&b,n,&sha).unwrap();assert_eq!(row["kind"],"task");assert!(row["body"].as_str().unwrap().contains("Checks: replay"));
        for (trusted,edited,s) in [(false,false,sha.clone()),(true,true,sha.clone()),(true,false,"b".repeat(40))] {b.raw(n,&format!("taskq: result-read {s}"),trusted,edited);}
        assert_eq!(current(&b,n,&sha).unwrap()["read"],false);
        assert_eq!(owner_act(&b,&root,&raw,n,&sha,"read").unwrap()["report"]["decision"],"none");
        assert_eq!(owner_act(&b,&root,&raw,n,&sha,"accept").unwrap()["report"]["decision"],"accepted");
        let opened=successor(&b,&root,&raw,n,&sha,"reopen","Preserve the retry receipt").unwrap();
        assert!(opened["reason"].is_null(),"{opened}");let child=opened["successor"].as_u64().unwrap();
        let issue=b.issue(child).unwrap();assert!(issue.body.contains("Reason: Preserve the retry receipt"));assert!(issue.body.contains(&format!("Depends on #{n}")));
        assert!(issue.labels.contains(&"q-held".into()));assert!(!issue.labels.contains(&"taskq".into()));
        b.state.borrow_mut().issues.iter_mut().find(|i|i.number==n).unwrap().title="Renamed after creation".into();
        let count=b.state.borrow().issues.len();assert_eq!(successor(&b,&root,&raw,n,&sha,"reopen","Preserve the retry receipt").unwrap()["successor"],child);
        assert_eq!(b.state.borrow().issues.len(),count);assert!(!b.issue(n).unwrap().open);
        assert_eq!(crate::core::fold(n,&crate::board::entries(&b,n).unwrap(),false).state,crate::core::State::Closed);
        assert!(successor(&b,&root,&raw,n,&sha,"reopen","").is_err());
        let mut cache=Cache::default();cache.collect(&b);assert_eq!(cache.value["reports"][0]["kind"],"task");
        assert_eq!(cache.value["reports"][0]["read"],true);
        std::fs::remove_dir_all(root).unwrap();
    }

    // A5/A6: paging must not skip racing updates; a restart may retain data but never assert a failed read is healthy.
    #[test] fn collection_cursor_precedes_paging_and_restart_retains_failed_rows() {
        let (b,_,n,sha)=board();let before=b.now().unwrap();b.state.borrow_mut().closed_page_advance=10;
        let mut cache=Cache::default();cache.collect(&b);assert_eq!(cache.verified,Some(before));
        b.raw(n,&format!("taskq: report-later {sha}"),true,false);
        b.state.borrow_mut().issues.iter_mut().find(|i|i.number==n).unwrap().updated=before+2;
        cache.collect(&b);assert_eq!(cache.value["reports"][0]["decision"],"later","update inside paging window must be revisited");
        let mut restarted=Cache::retaining(cache.value.clone());b.state.borrow_mut().fail_comment_reads=1;
        restarted.collect(&b);assert_eq!(restarted.value["complete"],false);assert_eq!(restarted.value["reports"][0]["n"],n);
        assert!(restarted.value["reason"].as_str().unwrap().contains("comment read failed"));
    }

    // §7/R12: ignored close comments cannot manufacture delivery evidence or replace the accepted verdict.
    #[test] fn result_verdict_uses_only_the_accepted_close() {
        let (mut i,mut comments)=fixture();i.labels=vec!["taskq".into(),"code".into()];
        comments[2].body.push_str("\nChanged: Reviewed result\nChecks: A1 ok\ndelivery auto-merge r0: merged #20 as SHA");
        comments.push(Comment{id:4,body:"taskq: close attempt 999\nChanged: Ignored\ndelivery auto-merge r0: merged #999 as SHA".into(),trusted:true,..Default::default()});
        let row=project(&i,45,&comments,"invented".into()).unwrap();
        assert!(row["verdict"].as_str().unwrap().contains("Reviewed result"));assert!(!row["verdict"].as_str().unwrap().contains("Ignored"));
        assert!(row["merged"].as_str().unwrap().contains("#20"));assert!(!row["merged"].as_str().unwrap().contains("#999"));
    }

    // R6/§7: legacy answered research must not regain attention or disappear from Completed history.
    #[test] fn legacy_answered_reports_remain_in_history_without_new_attention() {
        let (b,_,n,_)=board();
        {let mut st=b.state.borrow_mut();st.issues.iter_mut().find(|i|i.number==n).unwrap().labels=vec!["taskq".into(),"research".into()];
        let (_,c)=st.comments.iter_mut().find(|(task,c)|*task==n&&c.body.starts_with("taskq: result ")).unwrap();c.body=c.body.lines().next().unwrap().into();}
        b.raw(n,"taskq: answer 1",true,false);
        let mut cache=Cache::default();cache.collect(&b);
        assert!(cache.value["reports"].as_array().unwrap().is_empty());assert_eq!(cache.value["history"][0]["n"],n);assert_eq!(cache.value["history"][0]["read"],true);
    }

    // #1940 A5/A6/R12: a background collection must retain SHA content, observe new decisions/closures,
    // and preserve useful rows as unknown on failure. Cached reads have no Board argument or token lookup.
    #[test] fn collected_reports_refresh_without_losing_unknown_or_sha_identity() {
        let (b,_,n,sha)=board();let mut cache=Cache::default();cache.collect(&b);
        let first=cache.value.clone();assert_eq!(first["complete"],true);assert_eq!(first["reports"][0]["sha"],sha);
        assert!(first["reports"][0]["body"].is_string());
        // The source is immutable; an unchanged pass and a read marker need no source fetch.
        b.state.borrow_mut().report_sources.clear();cache.collect(&b);assert_eq!(cache.value["reports"],first["reports"]);
        mark(&b,n,&sha,"read",&[]).unwrap();cache.collect(&b);
        assert_eq!(cache.value["reports"][0]["read"],true);assert!(cache.value["reports"][0]["body"].is_string());
        // Replacing a result never borrows the old SHA's content or its read marker.
        let replacement="b".repeat(40);let mut st=b.state.borrow_mut();
        let result=st.comments.iter_mut().find(|(task,c)|*task==n&&c.body.starts_with("taskq: result ")).unwrap();
        result.1.body=result.1.body.replace(&sha,&replacement);st.issues.iter_mut().find(|i|i.number==n).unwrap().updated+=1;drop(st);
        cache.collect(&b);assert_eq!(cache.value["reports"][0]["read"],false);assert!(cache.value["reports"][0]["body"].is_null());
        assert!(cache.value["reports"][0]["reason"].as_str().unwrap().contains("unavailable"));
        b.state.borrow_mut().fail_comment_reads=1;cache.collect(&b);
        assert_eq!(cache.value["complete"],false);assert_eq!(cache.value["reports"][0]["n"],n);
        assert!(cache.value["reason"].is_string());
        let (new,_,other,_) = board();
        // A cold collection discovers an already closed report without any UI request.
        let mut fresh=Cache::default();fresh.collect(&new);assert_eq!(fresh.value["reports"][0]["n"],other);
    }

    // A5/A6: same-second markers must become visible; settled historical issues must not cost another comment read.
    #[test] fn collection_rechecks_same_second_updates_then_reuses_settled_projections() {
        let (b,_,n,sha)=board();let mut cache=Cache::default();cache.collect(&b);
        let updated=b.issue(n).unwrap().updated;
        b.raw(n,&format!("taskq: report-later {sha}"),true,false);
        b.state.borrow_mut().issues.iter_mut().find(|i|i.number==n).unwrap().updated=updated;
        b.advance(2);cache.collect(&b);assert_eq!(cache.value["reports"][0]["decision"],"later");
        let reads=b.state.borrow().comment_reads;
        cache.collect(&b);assert_eq!(b.state.borrow().comment_reads,reads,"settled unchanged projections reuse comments");
        b.raw(n,&format!("taskq: report-drop {sha}"),true,false);cache.collect(&b);
        assert!(cache.value["reports"].as_array().unwrap().is_empty());
    }

    // #1973 A1: open tasks and host heartbeats move every minute but never project; they must cost no comment read.
    #[test] fn open_and_heartbeat_updates_skip_comment_reads() {
        // The second pass settles the closed fixture's same-second read before counting.
        let (b,_,_,_)=board();let mut cache=Cache::default();cache.collect(&b);b.advance(2);cache.collect(&b);
        let open=b.create("Live task","",&["taskq".into(),"code".into()]).unwrap();
        let beat=b.create("Host heartbeat","",&["taskq-host".into()]).unwrap();b.close(beat).unwrap();
        let reads=b.state.borrow().comment_reads;
        for _ in 0..3 {
            b.advance(2);let at=b.now().unwrap();
            for n in [open,beat] {b.state.borrow_mut().issues.iter_mut().find(|i|i.number==n).unwrap().updated=at;}
            cache.collect(&b);assert_eq!(cache.value["complete"],true);
        }
        assert_eq!(b.state.borrow().comment_reads,reads,"no comment read for issues that cannot project");
    }

    // #1968 A4: a source missing at its result SHA is settled; it must not block the cursor or repeat reads.
    #[test] fn missing_report_source_settles_without_repeating_reads() {
        let (b,_,n,_)=board();let source=b.state.borrow_mut().report_sources.pop().unwrap();
        let mut cache=Cache::default();cache.collect(&b);
        assert_eq!(cache.value["complete"],true);assert!(cache.verified.is_some(),"a settled 404 advances the cursor");
        assert!(cache.value["reports"][0]["reason"].as_str().unwrap().contains("HTTP 404"));
        b.advance(2);cache.collect(&b);let reads=b.state.borrow().comment_reads;
        // A source read would now succeed; staying settled proves no pass fetches it again.
        b.state.borrow_mut().report_sources.push(source);b.advance(2);cache.collect(&b);
        assert_eq!(b.state.borrow().comment_reads,reads,"settled rows reuse comments");
        assert_eq!(cache.value["reports"][0]["n"],n);assert!(cache.value["reports"][0]["body"].is_null());
    }

    #[test] fn close_event_invalidates_collection_before_the_next_due_pass() {
        let (b,_,_,_)=board();let mut cache=Cache::default();cache.collect(&b);
        let root=std::env::temp_dir().join(format!("taskq-report-close-{}",crate::runtime::uuid()));std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let (tx,rx)=std::sync::mpsc::channel();tx.send(cache).unwrap();
        let mut collector=Collector{pending:Some(rx),started:Some(std::time::Instant::now()),..Default::default()};
        assert_eq!(collector.tick(&root,300,300,false)["complete"],true);
        let rows=stored(&root,&Value::Null);assert!(rows["reports"][0]["body"].is_string(),"#1975: rows land in reports.json");
        assert_eq!(summary(&rows)["count"],1);assert!(summary(&rows).get("reports").is_none(),"status keeps counts, never rows");
        std::fs::write(crate::dot(&root).join("events"),"invented #12 close 3\n").unwrap();
        collector.tick(&root,300,300,false);assert!(collector.invalidated,"observe-only passes retain the requested refresh");
        collector.tick(&root,300,300,true);assert!(collector.pending.is_some(),"close schedules without waiting for poll");
        // No project config: the read-only worker must return unknown, never create or launch anything.
        let failed=collector.pending.take().unwrap().recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(failed.value["complete"],false);assert!(failed.value["reason"].is_string());
        assert!(!crate::dot(&root).join("poke").exists(),"completion must not force a new board read");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test] fn cache_fresh_stale_missing_and_corrupt_are_distinct() {
        let (b,_,n,sha)=board();let mut cache=Cache::default();cache.collect(&b);
        let root=std::env::temp_dir().join(format!("taskq-report-cache-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(crate::dot(&root)).unwrap();
        assert_eq!(cached(&root)["complete"],false);
        std::fs::write(crate::dot(&root).join("status.json"),"corrupt").unwrap();assert!(cached(&root)["reason"].is_string());
        let at=cache.value["observed"].as_i64().unwrap();
        assert_eq!(freshness(cache.value.clone(),at+300,300,None)["complete"],true);
        let stale=freshness(cache.value.clone(),at+301,300,None);assert_eq!(stale["complete"],false);assert_eq!(stale["reports"][0]["n"],n);
        let failed=freshness(cache.value.clone(),at,300,Some("Board unavailable"));assert_eq!(failed["reason"],"Board unavailable");
        std::fs::write(crate::dot(&root).join("status.json"),json!({"reports":cache.value}).to_string()).unwrap();
        assert_eq!(cached_list(&root,1,None).unwrap()["reports"][0]["n"],n);
        let history=cached_history(&root,&json!({"task":n})).unwrap();assert_eq!(history["source"],"service");assert_eq!(history["tasks"][0]["n"],n);
        assert!(cached_open(&root,n,&sha).unwrap()["report"]["body"].is_string());
        assert_eq!(cached_open(&root,n,&"c".repeat(40)).unwrap()["complete"],false);
        let mut old=cached(&root);old["observed"]=json!(0);std::fs::write(crate::dot(&root).join("status.json"),json!({"reports":old}).to_string()).unwrap();
        let stale_history=cached_history(&root,&json!({"task":n})).unwrap();assert_eq!(stale_history["complete"],false);assert_eq!(stale_history["tasks"][0]["n"],n);assert!(stale_history["reason"].is_string());
        std::fs::remove_dir_all(root).unwrap();
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

    // §1/R1: known creation refusal must leave the report undecided and Drop available.
    #[test] fn rejected_creation_configuration_never_reserves_the_report() {
        for invalid in [json!({}),json!({"create_claim_issue":2})] {
            let (b,_,n,sha)=board();let count=b.comments(n).unwrap().len();
            // a root without taskq.json: the missing key cannot be settled and written back
            let root=std::env::temp_dir().join(format!("taskq-reports-{}",crate::runtime::uuid()));
            let result=owner_act(&b,&root,&invalid,n,&sha,"make").unwrap();
            assert_eq!(result["receipt"]["outcome"],"rejected");
            assert_eq!(b.comments(n).unwrap().len(),count,"known refusal writes no make-intent");
            assert_eq!(owner_act(&b,Path::new("."),&invalid,n,&sha,"drop").unwrap()["report"]["decision"],"drop");
        }
    }

    #[test] fn unread_source_and_lost_writes_never_claim_success_or_repeat_create() {
        let (b,claim,n,sha)=board();let root=std::env::temp_dir().join(format!("taskq-reports-{}",crate::runtime::uuid()));std::fs::create_dir_all(&root).unwrap();
        let raw=json!({"create_claim_issue":claim});
        let source=b.state.borrow_mut().report_sources.pop().unwrap();
        assert!(owner_act(&b,&root,&raw,n,&sha,"open").is_err());assert_eq!(list(&b,1).unwrap()["reports"][0]["read"],false);
        b.state.borrow_mut().report_sources.push(source);
        mark(&b,n,&sha,"making",&[]).unwrap();
        b.state.borrow_mut().lose_comment_response=true;
        let lost=owner_act(&b,&root,&raw,n,&sha,"make").unwrap();assert_eq!(lost["receipt"]["outcome"],"unknown");
        let retry=owner_act(&b,&root,&raw,n,&sha,"make").unwrap();assert_eq!(retry["receipt"]["outcome"],"unknown");
        assert_eq!(b.state.borrow().issues.len(),2,"an uncertain create claim never permits another create POST");
        assert_eq!(list(&b,1).unwrap()["reports"][0]["pending_decision"],"make");
        let blocked=owner_act(&b,&root,&raw,n,&sha,"drop").unwrap();assert!(blocked["reason"].is_string());assert_eq!(list(&b,1).unwrap()["reports"].as_array().unwrap().len(),1);
        let (other,_,on,os)=board();other.state.borrow_mut().lose_comment_response=true;
        assert!(owner_act(&other,&root,&raw,on,&os,"drop").is_err());assert!(list(&other,1).unwrap()["reports"].as_array().unwrap().is_empty(),"refresh reconciles the committed drop");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: §7 -> source-bound intent -> partial creation/decision -> replay. Risk: concurrent Drop hides
    // drafts, a failed final marker loses receipts, or a retry creates them twice.
    #[test] fn make_intent_fences_drop_and_preserves_receipts_after_final_marker_failure() {
        let (b,claim,n,sha)=board();let raw=json!({"create_claim_issue":claim});
        let root=std::env::temp_dir().join(format!("taskq-reports-{}",crate::runtime::uuid()));std::fs::create_dir_all(&root).unwrap();
        let prior=mark(&b,n,&sha,"making",&[]).unwrap();
        b.raw(n,&format!("taskq: report-drop {sha}"),true,false);
        assert_eq!(current(&b,n,&sha).unwrap()["visible"],true,"later Drop cannot win against the make intent");
        let source=b.source(&format!("docs/research/{n}.md"),&sha).unwrap();let tasks=proposals(&source).unwrap();let mut made=vec![];
        for (k,t) in tasks.iter().enumerate() {let r=crate::add_task(&b,&root,&raw,&t.task(n),&crate::Op{id:Some(format!("report.{n}.{sha}.{}",k+1)),revision:None}).unwrap();assert!(r.ok());made.push(r.task);}
        b.state.borrow_mut().fail_comments=1;
        let failed=finish(&b,n,&sha,&prior,&made);assert!(failed["reason"].is_string());assert_eq!(failed["receipts"],json!(made));assert_eq!(failed["report"]["visible"],true);
        let count=b.state.borrow().issues.len();let retried=owner_act(&b,&root,&raw,n,&sha,"make").unwrap();assert_eq!(retried["report"]["tasks_made"],json!(made));assert_eq!(b.state.borrow().issues.len(),count);
        let (other,_,on,os)=board();other.raw(on,&format!("taskq: report-drop {os}"),true,false);
        let result=owner_act(&other,&root,&raw,on,&os,"make").unwrap();assert_eq!(result["report"]["decision"],"drop");assert_eq!(other.state.borrow().issues.len(),2,"Drop first prevents creation");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: §7 -> stable close-time cursor -> decisions -> More. Risk: removing a page-one row skips an unread report.
    #[test] fn pagination_and_docs_discovery_survive_decisions_without_auxiliary_labels() {
        let (b,_,n,sha)=board();assert_eq!(list(&b,1).unwrap()["reports"][0]["n"],n,"docs report lives in its accepted result, not a later label");
        let mut expected=vec![n];
        for k in 0..11 {
            let next=b.create("Invented research","",&["taskq".into(),"research".into()]).unwrap();let spawn=b.raw(next,"taskq: spawn worker invented:s host",true,false);
            b.raw(next,&format!("taskq: result {sha} attempt {spawn}"),true,false);b.raw(next,"taskq: close",true,false);b.close(next).unwrap();b.state.borrow_mut().report_closed.insert(next,100+k);expected.push(next);
        }
        let first=list(&b,1).unwrap();assert_eq!(first["more"],true);let rows=first["reports"].as_array().unwrap();let dropped=rows[0]["n"].as_u64().unwrap();
        mark(&b,dropped,&sha,"drop",&[]).unwrap();expected.retain(|n|*n!=dropped);
        let cursor=first["next"].as_str().unwrap().split_once(':').unwrap();let rest=list_before(&b,2,Some((cursor.0.parse().unwrap(),cursor.1.parse().unwrap()))).unwrap();
        let mut seen:Vec<_>=rows.iter().filter_map(|r|r["n"].as_u64()).filter(|n|*n!=dropped).chain(rest["reports"].as_array().unwrap().iter().filter_map(|r|r["n"].as_u64())).collect();
        seen.sort();expected.sort();assert_eq!(seen,expected);
    }
}
