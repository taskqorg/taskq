//! §7 Reports: a separate projection of closed board objects. Never a queue state or local store (R1).
use crate::{board::{Board, Comment, Issue}, core::Entry, Res};
use serde::Deserialize;
use serde_json::{Value,json};
use std::path::Path;
pub(crate) fn now()->i64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs() as i64)}

/// Selected-task read-through on the existing board projection, never a task-state authority.
/// The per-source lock coalesces concurrent metadata/history/worker reads, without serializing other sources.
#[derive(Default)]
pub(crate) struct Selected {
    entries:std::sync::Mutex<std::collections::VecDeque<(String,std::sync::Arc<std::sync::Mutex<Option<(std::time::Instant,u64,Option<u64>,Res<Value>)>>>,usize)>>,
}
impl Selected {
    pub(crate) fn read(&self,key:String,revision:Option<u64>,now:std::time::Instant,load:impl FnOnce()->Res<Value>)->Res<Value> {
        let source={
            let mut entries=self.entries.lock().map_err(|_|"Selected history cache unavailable")?;
            let (source,bytes)=entries.iter().position(|(k,_,_)|*k==key).and_then(|i|entries.remove(i)).map(|(_,v,bytes)|(v,bytes))
                .unwrap_or_else(||(std::sync::Arc::new(std::sync::Mutex::new(None)),0));
            if entries.len()>=8 {
                let index=entries.iter().position(|(_,v,_)|std::sync::Arc::strong_count(v)==1).ok_or("Selected history reads busy; retry")?;
                entries.remove(index);
            }
            entries.push_back((key,source.clone(),bytes));source
        };
        let mut entry=source.lock().map_err(|_|"Selected history source unavailable")?;
        let fresh=entry.as_ref().is_some_and(|(at,ttl,observed,value)|
            now.checked_duration_since(*at).is_some_and(|age|age<std::time::Duration::from_secs(*ttl))
                &&revision.is_none_or(|wanted|observed.map_or(value.is_err(),|actual|wanted==actual)));
        let (value,bytes)=if fresh {(entry.as_ref().unwrap().3.clone(),None)}else{
            let value=load();let ttl=if value.is_ok(){60}else{2};let observed=value.as_ref().ok().and_then(|v|v["tasks"][0]["revision"].as_u64());
            if value.as_ref().map_or(true,|v|v["tasks"][0]["state"]=="closed") {*entry=Some((now,ttl,observed,value.clone()));}
            else {*entry=None;}
            let bytes=entry.as_ref().and_then(|(_,_,_,v)|v.as_ref().ok()).map_or(0,|v|v.to_string().len());
            (value,Some(bytes))
        };
        let mut entries=self.entries.lock().map_err(|_|"Selected history cache unavailable")?;
        if let Some(bytes)=bytes {if let Some((_,_,size))=entries.iter_mut().find(|(_,v,_)|std::sync::Arc::ptr_eq(v,&source)){*size=bytes;}}
        drop(entry);
        drop(source);
        // Keep the selected source and in-flight readers; the budget evicts idle sources, never denies history.
        while entries.len()>1&&entries.iter().map(|(_,_,bytes)|bytes).sum::<usize>()>64*1024*1024 {
            let Some(index)=entries.iter().take(entries.len()-1).position(|(_,v,_)|std::sync::Arc::strong_count(v)==1) else {break};
            entries.remove(index);
        }
        value
    }
}

pub(crate) fn selected(root:&Path,b:&dyn Board,n:u64,gitlab:bool,revision:Option<u64>)->Res<Value> {
    static CACHE:std::sync::OnceLock<Selected>=std::sync::OnceLock::new();
    let key=format!("{}|{}|{gitlab}",root.display(),b.url(n));
    CACHE.get_or_init(Selected::default).read(key,revision,std::time::Instant::now(),||{let value=crate::cli::inbox::closed_pin(b,n,gitlab)?;if !value["tasks"][0]["error"].is_null(){return Err("Selected task source incomplete; refresh or open its board".into());}Ok(value)})
}

pub(crate) fn worker(value:&Value,wanted:Option<u64>)->Res<(String,String,String,u64)> {
    let row=&value["tasks"][0];
    if row["state"]!="closed"||!row["error"].is_null(){return Err("Transcript requires a closed trusted task; refresh the project".into());}
    let source=row["workers"].as_array().into_iter().flatten().rev().find(|w|wanted.is_none_or(|a|w["attempt"]==a)).ok_or("No recorded worker transcript for this attempt")?;
    let (native,owner)=source["session"].as_str().and_then(|s|s.rsplit_once(' ')).ok_or("Worker identity unavailable")?;
    let (runtime,session)=native.split_once(':').ok_or("Worker session unavailable")?;
    Ok((runtime.into(),session.into(),owner.into(),source["attempt"].as_u64().ok_or("Worker attempt unavailable")?))
}

#[cfg(test)]
#[path = "reports_selected_tests.rs"]
mod selected_tests;

/// R1/R12: disposable board projection. Negative discoveries are cached too; immutable content is SHA-bound.
#[derive(Default)]
pub struct Cache {
    entries: std::collections::BTreeMap<u64,(i64,i64,Option<Value>)>,
    value: Value,
    verified: Option<i64>,
    /// #2083: board time of the last read of every issue updated since the one before (label-removed tasks included).
    full: Option<i64>,
    /// #2083: `since` of the next read of `taskq` issues only: the newest task update read, so an unchanged board repeats
    /// one URL and answers 304, where heartbeat writes change every reply of all issues.
    cursor: Option<i64>,
}

/// #2083: the longest between report reads of every issue.
const FULL_SCAN: i64 = 1800;
/// #2087: github.com serves issue lists up to 5 min old (§ 4), so a read of every issue starts this far before the
/// previous one: an update the lagging reply left out falls inside the next window instead of before it.
const LIST_LAG: i64 = 300;
impl Cache {
    fn retaining(value:Value)->Self {
        let entries=value["reports"].as_array().into_iter().flatten().chain(value["history"].as_array().into_iter().flatten())
            .filter_map(|r|r["n"].as_u64().map(|n|(n,(r["updated"].as_i64().unwrap_or(-1),r["proof"].as_i64().unwrap_or(0),Some(r.clone()))))).collect();
        // A restart resumes the stored cursor; rows keep their read proof, so only issues that moved re-read comments.
        // #2087: a cache written without the list-lag overlap may have missed an update for good; list it once in full.
        // #2091: rows read before a settled proof was required may hold a lagging comment read for good; same once-only relist.
        let verified=value["verified"].as_i64().filter(|_|value["overlap"]==LIST_LAG&&value["settle"]==LIST_LAG);
        Self{entries,value,verified,full:None,cursor:None}
    }
    pub fn collect(&mut self,b:&dyn Board) {
        let result=coreq::telemetry::capture::operation("reports.collect",|| -> Res<Value> {
            // Start before paging: updates racing a later page must remain inside the next overlap.
            let verified=b.now()?;
            let full=self.cursor.is_none() || self.full.is_none_or(|t|verified-t>=FULL_SCAN||t>verified);
            let since=self.verified.map(|v|if full {self.full.unwrap_or(v).min(v)-LIST_LAG-1} else {self.cursor.unwrap_or(v-1).min(v-1)});
            let issues=coreq::telemetry::capture::operation("reports.collect.list",|| -> Res<Vec<(Issue,i64)>> {
                if let Some(since)=since {
                    let issues=if full {b.issues(Some(since))?} else {b.tasks(since)?};
                    return Ok(issues.into_iter().map(|i|{let closed=i.closed.or_else(||self.entries.get(&i.number).and_then(|(_,_,r)|r.as_ref()).and_then(|r|r["closed"].as_i64())).unwrap_or(i.updated);(i,closed)}).collect());
                }
                let mut all=Vec::new();let mut page=1;
                loop {let (rows,more)=b.closed_tasks(page,None)?;all.extend(rows);if !more {break;}page+=1;}
                Ok(all)
            },|_|"board-read")?;
            let newest=issues.iter().filter(|(i,_)|i.labels.iter().any(|l|l==crate::service::TASK_LABEL)).map(|(i,_)|i.updated).max();
            let mut next=if self.verified.is_none(){Default::default()}else{self.entries.clone()};let mut errors=Vec::new();let mut unreadable=false;
            for (i,closed) in issues {
                let old=self.entries.get(&i.number);
                // Board timestamps have second precision, and #2091: a comment list read soon after the update may still lag
                // it (a decision read 66 s after it was missing). Only a read LIST_LAG after the update settles the row.
                let reusable=i.updated>0 && old.is_some_and(|(at,proof,row)|*at==i.updated && *proof>i.updated+LIST_LAG && row.as_ref().is_none_or(|r|r["kind"]=="task"||r["reason"].is_null()||r["missing"]==true));
                // #1973: open, untrusted or non-task issues (heartbeats, live tasks) never project; reading their comments
                // each time they move cost most of the board budget.
                let row=if reusable {
                    old.unwrap().2.clone()
                } else if !projectable(&i) {
                    None
                } else {
                    match coreq::telemetry::capture::operation("reports.collect.comments",||b.comments(i.number),|_|"board-read") {
                        Ok(comments)=>project(&i,closed,&comments,b.url(i.number)),
                        Err(e)=>{errors.push(format!("#{}: {}",i.number,shown(&format!("#{} comments",i.number),&e.to_string(),brief(&e.to_string()))));if let Some((_,_,row))=old {next.insert(i.number,(-1,0,row.clone()));}continue;}
                    }
                };
                let row=row.map(|mut row| {
                    if row["kind"]=="report" {
                        let source=old.and_then(|(_,_,r)|r.as_ref()).filter(|r|r["sha"]==row["sha"] && (r["body"].is_string()||r["missing"]==true));
                        let content=match source {Some(r) if r["missing"]==true=>Err(r["reason"].as_str().unwrap_or_default().to_string()),Some(r)=>Ok((r["body"].clone(),r["tasks"].clone())),None=>{
                            coreq::telemetry::capture::operation("reports.collect.source",|| -> Res<(Value,Value)> {
                                let body=b.source(&format!("docs/research/{}.md",i.number),row["sha"].as_str().ok_or("report SHA missing")?)?.replace("\r\n","\n");
                                let tasks=proposals(&body)?;
                                for t in &tasks {crate::cli::task_cmd::task_body(b,&t.task(i.number),&crate::Op::default())?;}
                                Ok((json!(body),json!(tasks)))
                            },|_|"report-source")
                        }};
                        match content {Ok((body,tasks))=>{row["body"]=body;row["tasks"]=tasks;},Err(reason)=>{
                            // #1968: a 404 at an immutable SHA never heals; settle it instead of re-reading every pass.
                            if reason.contains("HTTP 404") {row["missing"]=json!(true);}
                            row["reason"]=json!(shown(&format!("#{} source",i.number),&reason,source_reason(&reason)))}}
                    }row
                });
                // #2193: one unreadable report keeps its row and reason; the list stays complete, but the cursor
                // holds so the next pass retries it.
                if row.as_ref().is_some_and(|r|r["kind"]=="report"&&r["missing"]!=true&&r["reason"].is_string()) {unreadable=true;}
                next.insert(i.number,(i.updated,if reusable{old.unwrap().1}else{verified},row));
            }
            self.entries=next;
            if errors.is_empty()&&!unreadable {
                if let Some(since)=since {
                    if full {self.full=Some(verified);}
                    self.cursor=newest.map(|t|t-30).filter(|t|*t<=verified).into_iter().chain(self.cursor).max().or(Some(since));
                }
                self.verified=Some(verified);
            }
            let mut rows:Vec<Value>=self.entries.values().filter_map(|(at,proof,r)|r.clone().map(|mut r|{r["updated"]=json!(at);r["proof"]=json!(proof);r})).collect();
            rows.sort_by_key(|r|(std::cmp::Reverse(r["closed"].as_i64().unwrap_or(0)),std::cmp::Reverse(r["n"].as_u64().unwrap_or(0))));
            let visible:Vec<_>=rows.iter().filter(|r|r["visible"]==true).collect();let archived:Vec<_>=rows.iter().filter(|r|r["visible"]!=true).collect();
            Ok(json!({"reports":visible,"history":archived,"complete":errors.is_empty(),"reason":(!errors.is_empty()).then(||errors.join("; ")),"observed":now(),"verified":self.verified,"overlap":LIST_LAG,"settle":LIST_LAG}))
        },|_|"board-read");
        match result {Ok(value)=>self.value=value,Err(reason)=>{self.value["complete"]=json!(false);self.value["reason"]=json!(shown("collect",&reason.to_string(),brief(&reason.to_string())));}}
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
        let closes=std::fs::read_to_string(crate::paths::dot(root).join("events")).unwrap_or_default().lines().filter(|l|l.contains(" close ")).collect::<Vec<_>>().join("\n");
        if closes!=self.closes {self.closes=closes;self.invalidated=true;}
        // #2352: one collection every three polls, under `ttl` (freshness below); a close asks at once.
        let every=(poll.max(1)*3).min(u64::try_from(ttl/2).unwrap_or(0).max(1));
        if enabled && self.pending.is_none() && (self.invalidated || self.started.is_none_or(|t|t.elapsed().as_secs()>=every)) {
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
fn file(root:&Path)->std::path::PathBuf {crate::paths::dot(root).join("reports.json")}
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
                match crate::config::load(&root).and_then(|(cfg,raw)|cfg.board.open(&cfg.web_host,&cfg.repo,raw["label_schema"].as_str(),&crate::platform::tool(&cfg.install))) {
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
    let result=crate::data::json(&crate::paths::dot(root).join("status.json"));
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

pub fn cached_open(root:&Path,n:u64,sha:&str)->Res<Value> {open_latest(root,n,Some(sha))}

/// `sha: None` opens the task's latest collected result, so `taskq results N` needs no SHA digging.
pub fn open_latest(root:&Path,n:u64,sha:Option<&str>)->Res<Value> {
    coreq::telemetry::capture::operation("reports.open.cached",|| -> Res<Value> {
        if sha.is_some_and(|s|!identity(s)) {return Err("result identity must be full SHA or collected close identity".into());}
        let value=cached(root);
        let row=value["reports"].as_array().into_iter().flatten().chain(value["history"].as_array().into_iter().flatten()).find(|r|r["n"]==n && sha.is_none_or(|s|r["sha"]==s));
        match row {Some(row)=>Ok(json!({"report":row,"complete":value["complete"],"reason":value["reason"]})),None=>Ok(json!({"reason":value["reason"].as_str().unwrap_or("Report not in the collected snapshot; waiting for the service"),"complete":false}))}
    },|_|"cache-read")
}

/// Reads bypass board construction and token lookup, so offline navigation cannot become a login/API probe.
pub fn read_cli(root:&Path,a:&crate::cli::Args)->Res<Option<Value>> {
    if a.flags.iter().any(|(k,v)|!["action","sha","page","before","json","text","until"].contains(&k.as_str())||v.len()!=1) {return Err("results: use --action, --sha, --page, --before, --text, --until and --json once each".into());}
    if a.one("text").is_some()&&!matches!(a.one("action"),Some("followup"|"reopen")) {return Err("--text supplies the reason for followup or reopen".into());}
    if a.one("until").is_some()&&a.one("action")!=Some("later") {return Err("--until sets the revisit date of --action later".into());}
    if a.pos.len()==1 {
        if a.one("action").is_some()||a.one("sha").is_some() {return Err("report action needs a task number".into());}
        let page=a.one("page").unwrap_or("1").parse::<usize>().ok().filter(|p|*p>0).ok_or("report page must be positive")?;
        let before=a.one("before").map(|s|s.split_once(':').and_then(|(t,n)|Some((t.parse::<i64>().ok()?,n.parse::<u64>().ok()?))).ok_or("report cursor must be TIME:N")).transpose()?;
        return cached_list(root,page,before).map(Some);
    }
    if a.one("page").is_some()||a.one("before").is_some() {return Err("--page/--before list reports without a task number".into());}
    if a.one("action").unwrap_or("open")=="open" {return open_latest(root,a.num(1)?,a.one("sha")).map(Some);}
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
    fn task(&self,n:u64)->crate::cli::task_cmd::NewTask {
        crate::cli::task_cmd::NewTask {conversation:None,title:self.title.clone(),goal:self.goal.clone(),acceptance:self.acceptance.clone(),scope:self.scope.clone(),
            deps:vec![n],kind:self.kind.clone(),runtime:self.runtime.clone(),priority:self.priority.clone(),host:None,labels:vec![],rules:None}
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
    let (mut read,mut decision,mut made,mut making,mut until)=(legacy_answered,if legacy_answered{"answered"}else{"none"},json!([]),false,None);
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
                // #1979: the newest Later counts; one without a valid revisit date is not a decision.
                if kind=="later" {until=c.body.split_once('\n').and_then(|(_,s)|s.trim().strip_prefix("until ")).filter(|u|crate::core::utc(u)).map(String::from);}
                read=true;if kind=="making" {making=true;} else if kind!="read" {decision=value;}
            }
        }
    }
    let text=|c:Option<&Comment>|c.and_then(|c|c.body.split_once('\n').map(|(_,text)|text)).unwrap_or("").to_string();
    let accepted_close:Vec<_>=closing.into_iter().map(|c|Entry{id:c.id,body:c.body.clone(),edited:false}).collect();
    let mut row=json!({"n":i.number,"title":i.title,"url":url,"sha":sha,"closed":closed,"read":read,"kind":if report{"report"}else{"task"},"state":"closed","outcome":if completed{"success"}else if i.reason.as_deref()==Some("not_planned"){"not_planned"}else{"unknown"},"merged":crate::cli::inbox::merge_receipt(&accepted_close),
        "comments_through":comments.iter().filter(|c|c.trusted&&!c.edited).map(|c|c.id).max().unwrap_or(0),
        "conversation":crate::core::conversation(i.number,&log(comments)),
        "decision":decision,"visible":!legacy_answered&&!matches!(decision,"drop"|"made")&&!(report&&decision=="accepted"),"pending_decision":if making&&!matches!(decision,"made"|"drop"|"accepted"){Some("make")}else{None},"tasks_made":made});
    let details=text(handin);let verdict=crate::core::verdict(i.number,&log(comments),&text(closing));
    row["result"]=json!(details.lines().find_map(|l|l.strip_prefix("Changed: ")));
    row["pr"]=json!(format!("{details}\n{verdict}").split(|c:char|c.is_whitespace()||matches!(c,'('|')'|'['|']'|'<'|'>')).filter(|u|u.starts_with("https://")&&(u.contains("/pull/")||u.contains("/merge_requests/"))).last().map(|u|u.trim_end_matches(|c:char|!c.is_ascii_alphanumeric()).to_string()));
    row["close"]=json!(crate::display::close_outcome(Some(&verdict),None));
    // #1979: the board label reads the verdict back; the accepted close line covers a label write still in flight.
    row["experiment_verdict"]=json!(i.labels.iter().find_map(|l|l.strip_prefix("verdict-").and_then(|v|outcome_verdict(&format!("Verdict: {v}")))).or_else(||outcome_verdict(&verdict)));
    row["later_until"]=json!(until);
    if !report {row["body"]=json!(details);row["verdict"]=json!(verdict);if handin.is_none() {row["reason"]=json!("No accepted hand-in recorded; result details unknown");}}
    Some(row)
}

/// #2193: the row reason for a failed report source read. A board reply body (JSON) never reaches Conversations or
/// the menu; the service log keeps the full text. Own validation messages are already short and stay.
fn source_reason(e:&str)->String {
    match http_status(e).as_deref() {
        Some("404")=>"Report source unavailable at the result SHA (HTTP 404)".into(),
        Some("403") if !e.to_ascii_lowercase().contains("rate limit")=>"Report source unreadable: board token lacks Contents read permission (HTTP 403)".into(),
        _=>brief(e),
    }
}

/// A board error without its reply body; the caller logs the full text.
fn brief(e:&str)->String {
    match http_status(e) {Some(status)=>format!("Board read failed (HTTP {status}); details in the service log"),None=>e.to_string()}
}

fn http_status(e:&str)->Option<String> {
    let at=e.find("HTTP ")?;Some(e[at+5..].chars().take_while(char::is_ascii_digit).collect())
}

/// Shortens a board error for the projection and logs the full text when it differs.
fn shown(context:&str,e:&str,short:String)->String {
    if short!=e {crate::platform::log(&format!("reports: {context}: {e}"));}
    short
}

pub const VERDICTS:[&str;3]=["confirmed","refuted","inconclusive"];

/// #1979: the experiment verdict of the first `Verdict:` line, its word before any evidence.
pub fn outcome_verdict(text:&str)->Option<&'static str> {
    let line=text.lines().find_map(|l|l.trim().strip_prefix("Verdict:"))?;
    let word=line.split_whitespace().next()?.trim_end_matches(|c:char|!c.is_ascii_alphabetic()).to_ascii_lowercase();
    VERDICTS.into_iter().find(|v|*v==word)
}

/// #1979: an experiment task (specq experiment tasks), whatever its type: `research`, or a `Hypothesis:` body line.
pub fn experiment(i:&Issue)->bool {
    i.labels.iter().any(|l|l=="research")||i.body.lines().any(|l|l.trim_start_matches(['-','*',' ']).starts_with("Hypothesis:"))
}

/// The first prose line of the report's Recommendation section.
fn recommendation(body:&str)->Option<String> {
    let mut lines=body.lines().skip_while(|l|!(l.starts_with('#')&&l.trim_start_matches('#').trim().eq_ignore_ascii_case("Recommendation"))).skip(1);
    lines.find(|l|!l.trim().is_empty()).filter(|l|!l.starts_with('#')).map(|l|crate::cli::text::compact(l.trim(),120))
}

/// R6 Decide: undecided reports, oldest close first. Make tasks (made or reserved), Drop and a Later still before
/// its revisit date decide; legacy answered research is out. Unknown collection never reads as an empty list (R12).
pub fn decide(value:&Value,now:i64)->Value {
    let mut rows:Vec<&Value>=value["reports"].as_array().into_iter().flatten().filter(|r|r["kind"]!="task"&&r["visible"]==true&&r["pending_decision"].is_null()
        &&matches!(r["decision"].as_str(),Some("none"|"later"))
        &&!(r["decision"]=="later"&&r["later_until"].as_str().and_then(crate::board::iso).is_some_and(|at|at>now))).collect();
    rows.sort_by_key(|r|(r["closed"].as_i64().unwrap_or(0),r["n"].as_u64().unwrap_or(0)));
    let rows:Vec<Value>=rows.into_iter().map(|r|json!({"n":r["n"],"title":r["title"],"url":r["url"],"sha":r["sha"],"verdict":r["experiment_verdict"].as_str().unwrap_or("unknown"),
        "closed":r["closed"],"age_s":(now-r["closed"].as_i64().unwrap_or(now)).max(0),"recommendation":r["body"].as_str().and_then(recommendation),"later_until":r["later_until"]})).collect();
    json!({"complete":value["complete"]==true,"reason":value["reason"],"rows":rows})
}

fn identity_sha(sha:&str)->bool {sha.len()==40&&sha.bytes().all(|b|b.is_ascii_hexdigit())}
fn identity(sha:&str)->bool {identity_sha(sha)||sha.strip_prefix("close-").is_some_and(|id|!id.is_empty()&&id.bytes().all(|b|b.is_ascii_digit()))}

fn current(b:&dyn Board,n:u64,sha:&str)->Res<Value> {
    if !identity(sha) {return Err("result identity invalid".into());}
    let i=b.issue(n)?;
    if i.number!=n {return Err("result issue identity mismatch".into());}
    let row=project(&i,0,&b.comments(n)?,b.url(n)).ok_or("not a completed research report")?;
    if row["sha"]!=sha {return Err("report changed; refresh before deciding".into());} Ok(row)
}

fn mark(b:&dyn Board,n:u64,sha:&str,kind:&str,made:&[u64])->Res<Value> {mark_until(b,n,sha,kind,made,None)}
fn mark_until(b:&dyn Board,n:u64,sha:&str,kind:&str,made:&[u64],until:Option<&str>)->Res<Value> {
    let row=current(b,n,sha)?;
    if row["visible"]==false||matches!(row["decision"].as_str(),Some("accepted"|"drop"|"made"|"answered"))||kind=="read"&&row["read"]==true||kind=="accept"&&row["decision"]=="accepted"||kind=="later"&&row["decision"]=="later"&&row["later_until"].as_str()==until||kind=="making"&&row["pending_decision"]=="make" {return Ok(row);}
    let prefix=if row["kind"]=="task" {"result"}else{"report"};
    let tail=match (kind,until) {("made",_)=>format!("\n{}",json!(made)),("later",Some(u))=>format!("\nuntil {u}"),_=>String::new()};
    let text=format!("taskq: {prefix}-{kind} {sha}{tail}");
    let id=b.comment(n,&text)?;
    let comments=b.comments(n)?;
    if !comments.iter().any(|c|c.id==id&&c.trusted&&!c.edited&&c.body==text) {return Err("report write readback unknown; refresh before retrying".into());}
    current(b,n,sha)
}

/// `until`: a report Later's revisit time (#1979).
pub fn act_with_reason(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str,reason:&str,until:Option<&str>)->Res<Value> {
    if action=="open" {return cached_open(root,n,sha);}
    if crate::dispatch::agent() {return Err("report decisions require the owner CLI, outside an attempt".into());}
    let result=if matches!(action,"followup"|"reopen") {successor(b,root,raw,n,sha,action,reason)}else {owner_act_until(b,root,raw,n,sha,action,until)};
    let _=crate::wake::poke(root);result
}

fn successor(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str,reason:&str)->Res<Value> {
    let row=current(b,n,sha)?;
    if row["pending_decision"]=="make" {return Err("Make tasks already started; reconcile its held drafts before another decision.".into());}
    let reason=reason.trim();
    if reason.is_empty()||reason.chars().count()>180||reason.contains(['\r','\n']) {return Err("Give a one-line reason, at most 180 characters".into());}
    let label=if action=="reopen" {"Reopen"}else{"Follow-up"};
    let task=crate::cli::task_cmd::NewTask{conversation:None,title:format!("{label} #{n}"),
        goal:format!("{label} #{n}: {reason}"),acceptance:vec![format!("The operator's reason is addressed: {reason}")],
        scope:vec![format!("Source: {}",b.url(n)),format!("Reason: {reason}")],deps:vec![n],kind:None,runtime:None,priority:None,host:None,labels:vec![],rules:None};
    crate::cli::task_cmd::task_body(b,&task,&crate::Op::default())?;
    let hash=ring::digest::digest(&ring::digest::SHA256,reason.as_bytes());
    let hash:String=hash.as_ref().iter().map(|b|format!("{b:02x}")).collect();
    let op=crate::Op{id:Some(format!("result.{n}.{sha}.{action}.{hash}")),revision:None};
    let receipt=crate::cli::task_cmd::add_task(b,root,raw,&task,&op)?;
    Ok(json!({"report":row,"successor":receipt.task,"successor_url":(receipt.task>0).then(||b.url(receipt.task)),"receipt":receipt,"reason":(receipt.outcome!="accepted").then(||receipt.reason.clone())}))
}

pub(crate) fn owner_act_until(b:&dyn Board,root:&Path,raw:&Value,n:u64,sha:&str,action:&str,until:Option<&str>)->Res<Value> {
    if !["open","read","accept","later","drop","make"].contains(&action) {return Err("result action must be open, read, accept, followup, reopen, later, drop or make".into());}
    if until.is_some()&&action!="later" {return Err("--until sets a report Later's revisit date".into());}
    let prior=current(b,n,sha)?;
    if prior["kind"]=="task" {
        if !matches!(action,"read"|"accept"|"later"|"drop") {return Err("completed task actions are read, accept, later, drop, followup or reopen".into());}
        return Ok(json!({"report":mark(b,n,sha,action,&[])?}));
    }
    if prior["visible"]==false {return Ok(json!({"report":prior}));}
    // #1979 Decide: a Later without a revisit date is not a decision.
    if action=="later" {
        let at=until.filter(|u|crate::core::utc(u)).and_then(crate::board::iso).ok_or("Later needs --until YYYY-MM-DDTHH:MM:SSZ, the revisit date")?;
        if at<=b.now()? {return Err("Later's revisit date must be in the future".into());}
    }
    if matches!(action,"read"|"accept"|"later"|"drop") {
        let row=if matches!(action,"accept"|"drop")&&prior["pending_decision"]=="make" {prior} else {mark_until(b,n,sha,action,&[],until)?};
        return Ok(if matches!(action,"accept"|"drop")&&row["pending_decision"]=="make" {json!({"report":row,"reason":"Make tasks already started; reconcile its held drafts before another decision."})} else {json!({"report":row})});
    }
    let body=b.source(&format!("docs/research/{n}.md"),sha)?.replace("\r\n","\n");
    let tasks=proposals(&body)?;
    // Validate all proposals before the first external write; task_body retains §1/routing/limit guards.
    for t in &tasks {crate::cli::task_cmd::task_body(b,&t.task(n),&crate::Op::default())?;}
    if action=="open" {
        let mut row=mark(b,n,sha,"read",&[])?;row["body"]=json!(body);row["tasks"]=json!(tasks);
        return Ok(json!({"report":row}));
    }
    if tasks.is_empty() {return Err("report proposes no follow-up tasks".into());}
    // #1945: a board set up without the key (cyprusbz#540) refused every Make; the owner's Make settles it like claim-issue
    let settled;
    let raw=if raw["create_claim_issue"].is_null() {
        let r=crate::cli::task_cmd::claim_issue_setup(b,root,None).unwrap_or_else(|e|json!({"outcome":"rejected","reason":e,"key":"create_claim_issue"}));
        if r["outcome"]!="accepted" {
            return Ok(json!({"report":prior,"reason":format!("create_claim_issue: {}",r["reason"].as_str().unwrap_or("unknown")),"receipt":r}));
        }
        let mut v=raw.clone();v["create_claim_issue"]=r["issue"].clone();settled=v;&settled
    } else {raw};
    if let Err(receipt)=crate::cli::task_cmd::create_claim_issue(b,raw,&crate::Op::default()) {
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
        let receipt=match crate::cli::task_cmd::add_task(b,root,raw,&t.task(n),&op) {Ok(r)=>r,Err(e)=>return Ok(unknown(b,n,sha,&reserved,&made,e))};
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
    let out=crate::platform::output("reports.candidate",crate::platform::command("git").current_dir(root).args(["cat-file","-e",&spec]),std::time::Duration::from_secs(15))?;
    if !research&&!out.status.success() {return Ok(false);}
    let body=b.source(&format!("docs/research/{n}.md"),sha)?.replace("\r\n","\n");
    for task in proposals(&body)? {crate::cli::task_cmd::task_body(b,&task.task(n),&crate::Op::default())?;}
    Ok(true)
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;
