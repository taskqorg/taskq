//! I3 / UX30-21: a rebuildable projection of first queue admissions, sourced only from board label events.
use crate::board::Board;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MONTH: i64 = 30 * 86400;
const EVENT_READS: usize = 5;
/// #2083: the longest between admission scans of every issue (§ 4 budget).
const FULL_SCAN: i64 = 300;

/// Observation stays off the service's heartbeat/admission path, including a slow or unavailable board.
pub struct Reader {
    request: std::sync::mpsc::SyncSender<()>,
    response: std::sync::mpsc::Receiver<Value>,
    busy: bool,
    value: Value,
}
impl Reader {
    pub fn new(cfg: crate::service::Config, schema: Option<String>, identity: &str) -> Self {
        let value=crate::data::json(&cfg.root.join(".taskq/status.json")).ok().flatten()
            .map(|v|v["incoming"].clone()).filter(|v|valid(v,identity)).unwrap_or(Value::Null);
        let seed=value.clone();
        let identity=identity.to_string();
        let (request, requests)=std::sync::mpsc::sync_channel(1);
        let (responses, response)=std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let board=cfg.board.open(&cfg.web_host,&cfg.repo,schema.as_deref(),&crate::platform::tool(&cfg.install));
            let mut history=History::seed(&seed,&identity);
            while let Ok(())=requests.recv() {
                let value=match &board {
                    Ok(board)=>{
                        match board.now() {Ok(now)=>history.refresh(board.as_ref(),now),Err(reason)=>history.missing=vec![reason]}
                        history.json(&board.url(0))
                    },
                    Err(reason)=>{history.missing=vec![reason.clone()];history.json(&identity)},
                };
                if responses.send(value).is_err() {break;}
            }
        });
        Self {request,response,busy:false,value}
    }

    pub fn tick(&mut self) -> Value {
        match self.response.try_recv() {
            Ok(value)=>{self.value=value;self.busy=false;},
            Err(std::sync::mpsc::TryRecvError::Disconnected)=>{if self.value["schema"]==1 {self.value["missing"]=json!(["admission reader exited"]);}
                else {self.value=json!({"reason":"admission reader exited"});}},
            Err(std::sync::mpsc::TryRecvError::Empty)=>{},
        }
        if !self.busy && self.request.try_send(()).is_ok() {self.busy=true;}
        self.value.clone()
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct History {
    // Updated issue and its last verified first admission; no task state or launch authority.
    seen: BTreeMap<u64, (i64, Option<(u64, i64)>)>,
    // Issue update, current task eligibility, last event read (0: unvisited); all rebuildable observations.
    pending: BTreeMap<u64, (i64, bool, i64)>,
    // Issue update and reason of a complete history read that failed; read again only after the issue updates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    failed: BTreeMap<u64, (i64, String)>,
    listed: Option<i64>,
    // Newest non-heartbeat issue update a scan returned: the `taskq` scan asks from it, so an unchanged board repeats one
    // URL and answers 304 (#1985, #2083).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    newest: Option<i64>,
    // Board time of the last scan of every issue (#2083): closed, label-removed and never-admitted issues, at least every
    // FULL_SCAN; heartbeat writes change that reply, so each pass between asks for `taskq` issues only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full: Option<i64>,
    through: Option<i64>,
    start: Option<i64>,
    missing: Vec<String>,
}

impl History {
    fn seed(v: &Value, board: &str) -> Self {
        if !valid(v,board) {return Self::default();}
        if let Ok(h)=serde_json::from_value::<Self>(v["cache"].clone()) {
            if h.json(board)==*v && h.listed.is_some_and(|t|h.through.is_none_or(|end|end<=t))
                && h.seen.iter().all(|(n,(t,_))|*n>0 && *t>0)
                && h.pending.iter().all(|(n,(t,_,tried))|*n>0 && *t>0 && *tried>=0)
                && h.failed.iter().all(|(n,(t,_))|*n>0 && *t>0) {return h;}
        }
        // Older projections retain verified arrivals and coverage, but lack negative observations or a scan cursor.
        let mut h=Self {start:v["start"].as_i64(),through:v["end"].as_i64(),
            missing:v["missing"].as_array().unwrap().iter().map(|s|s.as_str().unwrap().to_string()).collect(),..Self::default()};
        for e in v["events"].as_array().unwrap() {
            let at=e[0].as_i64().unwrap();
            h.seen.insert(e[3].as_u64().unwrap(),(at,Some((e[2].as_str().unwrap().parse().unwrap(),at))));
        }
        h
    }

    pub fn refresh(&mut self, board: &dyn Board, now: i64) {
        let floor = now - MONTH;
        if self.listed.is_some_and(|t|t>now) {self.missing=vec!["admission scan clock moved backwards".into()];return;}
        // The newest seen update is never after the last scan, so asking from it still covers every later update.
        let after = |t: Option<i64>| t.map_or(floor, |t| (t - 30).max(floor));
        let full = self.full.is_none_or(|t| now - t >= FULL_SCAN || t > now);
        let scan = if full { board.issues(Some(after(self.full.or(self.newest).or(self.listed)))) }
            else { board.tasks(after(self.newest.or(self.listed))) };
        let issues = match scan {
            Ok(issues) => issues,
            Err(e) => { self.missing = vec![format!("admission scan: {e}")]; return; }
        };
        if full { self.full = Some(now); }
        self.newest = issues.iter().filter(|i| !crate::service::is_host(i)).map(|i| i.updated).filter(|t| *t <= now).chain(self.newest).max();
        for issue in issues.iter().filter(|i| i.trusted && !crate::service::is_host(i)) {
            if let Some((updated,event))=self.seen.get_mut(&issue.number) {
                // A settled earliest admission is immutable; comments and later label changes cannot create another.
                if event.is_some() || *updated==issue.updated {*updated=issue.updated;continue;}
            }
            if self.failed.get(&issue.number).is_some_and(|(updated,_)|*updated==issue.updated) {continue;}
            self.failed.remove(&issue.number);
            let pending=self.pending.entry(issue.number).or_insert((issue.updated,crate::service::is_task(issue),0));
            pending.0=issue.updated;
            pending.1=crate::service::is_task(issue);
        }
        self.listed=Some(now);
        self.seen.retain(|_, (updated, event)| *updated>=floor || event.is_some_and(|(_, at)| at>=floor));
        self.pending.retain(|_, (updated,_,_)|*updated>=floor);
        self.failed.retain(|_, (updated,_)|*updated>=floor);
        let mut pending:Vec<_>=self.pending.iter().filter(|(_, (updated,_,_))|now>=*updated+30).map(|(n,(_,_,tried))|(*tried,*n)).collect();
        pending.sort_unstable();
        let mut missing=vec![];
        for (_,n) in pending.into_iter().take(EVENT_READS) {
            let (updated,task,tried)=self.pending.get_mut(&n).unwrap();
            *tried=now;
            match admission(board,n,*task).and_then(|event| if event.is_some_and(|(_, at)| at > now) {Err("first-admission time is in the future".into())} else {Ok(event)}) {
                Ok(event)=>{self.seen.insert(n,(*updated,event));self.pending.remove(&n);},
                Err(e) if SETTLED.contains(&e.as_str())=>{self.failed.insert(n,(*updated,e));self.pending.remove(&n);},
                Err(e)=>missing.push(format!("#{n}: {e}")),
            }
        }
        // A failed source keeps coverage incomplete until its issue updates or leaves the month.
        missing.extend(self.failed.iter().map(|(n,(_,e))|format!("#{n}: {e}")));
        // An unresolved source must be retried, including after its issue leaves the update overlap.
        if self.pending.is_empty() && self.failed.is_empty() {
            self.start = Some(self.start.unwrap_or(floor).max(floor));
            // #2083: a scan of `taskq` issues proves nothing of other issues; coverage ends at the last scan of every issue
            self.through = self.full.map(|t| t.min(now));
        } else if !self.pending.is_empty() {
            missing.push(format!("{} admission sources pending",self.pending.len()));
        }
        self.missing = missing;
    }

    pub fn json(&self, board: &str) -> Value {
        let events: Vec<_> = self.seen.iter().filter_map(|(n, (_, event))| event.map(|(id, at)| json!([at, "new", id.to_string(), n]))).collect();
        json!({"schema":1, "board":board, "start":self.start, "end":self.through, "events":events, "missing":self.missing, "cache":self})
    }
}

/// A status cache with malformed provenance cannot establish a complete zero.
pub fn valid(v: &Value, board: &str) -> bool {
    v["schema"] == 1 && v["board"] == board
        && v["missing"].as_array().is_some_and(|m|m.iter().all(Value::is_string))
        && v["events"].as_array().is_some_and(|events|events.iter().all(|e| {
            e.as_array().is_some_and(|a|a.len()==4) && e[0].as_i64().is_some_and(|at|at>0) && e[1]=="new"
                && e[2].as_str().and_then(|id|id.parse::<u64>().ok()).is_some_and(|id|id>0) && e[3].as_u64().is_some_and(|n|n>0)
        }))
        && (v["start"].is_null() && v["end"].is_null() || v["start"].as_i64().zip(v["end"].as_i64()).is_some_and(|(a,b)|a<=b))
}

/// Malformed records a complete history read gives again until its issue updates (§ 4 admission records); other failures retry.
const SETTLED: [&str; 3] = ["admission ID unavailable", "admission time unavailable", "invalid first-admission provenance"];

fn admission(board: &dyn Board, n: u64, task: bool) -> Result<Option<(u64, i64)>, String> {
    let event = board.first_admission(n)?;
    if event.is_some_and(|(id, at)| id == 0 || at <= 0) { return Err("invalid first-admission provenance".into()); }
    if event.is_none() && task { return Err("first-admission history unavailable".into()); }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Fake;

    // § 4 budget / I3: a cold scan or one unavailable source must not replay a month of per-issue reads.
    #[test]
    fn collection_bounds_cold_restart_and_failed_source_requests() {
        let reads=std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let unavailable=std::rc::Rc::new(std::cell::Cell::new(true));
        let fail=unavailable.clone();
        let requests=reads.clone();
        let board=crate::board::replying(&crate::board::github::KIND, move |_,url| {
            requests.borrow_mut().push(url.to_string());
            if url.contains("/events?") {
                if url.contains("/issues/1/") && fail.get() {return Err("unavailable".into());}
                return Ok(if url.contains("/issues/2/") {json!([{"id":72,"event":"labeled","label":{"name":"taskq"},"created_at":crate::board::stamp(100)}])} else {json!([])});
            }
            assert!(url.contains("/issues?"), "unexpected request: {url}");
            Ok(if !url.ends_with(&crate::board::stamp(200-MONTH)) {json!([])} else {Value::Array((1..=1006).map(|n|json!({"number":n,"title":"draft","state":"closed",
                "author_association":"OWNER","updated_at":crate::board::stamp(100),"labels":[]})).collect())
            })
        });
        let mut h=History::default();
        h.refresh(board.as_ref(),200);
        assert_eq!(reads.borrow().iter().filter(|u|u.contains("/events?")).count(),5,
            "cold collection must bound event-source requests per pass");
        assert!(h.through.is_none(),"a bounded partial scan cannot prove a complete zero");
        assert_eq!(h.json(&board.url(0))["events"],json!([[100,"new","72",2]]));
        let mut now=300;
        // Simulate a restart after every bounded batch; negatives and unvisited sources survive too.
        while h.pending.len()>1 {
            let snapshot:Value=serde_json::from_str(&h.json(&board.url(0)).to_string()).unwrap();
            h=History::seed(&snapshot,&board.url(0));
            reads.borrow_mut().clear();
            h.refresh(board.as_ref(),now);
            let requests=reads.borrow();
            // #2083: between scans of every issue, which ask from the last one, the `taskq` scan repeats from the newest update
            let task=format!("/issues?state=all&labels=taskq&per_page=100&since={}",crate::board::stamp(70));
            let full=format!("/issues?state=all&per_page=100&since={}",crate::board::stamp(now-FULL_SCAN-30));
            assert!(requests[0].ends_with(&task) || requests[0].ends_with(&full),
                "an unresolved source cannot reset the list cursor, which repeats from the newest update (#1985): {}",requests[0]);
            assert!(requests.iter().filter(|u|u.contains("/events?")).count()<=5);
            assert!(!requests.iter().any(|u|u.contains("/issues/2/")),"a resolved source is not read again after restart");
            assert!(now<21000,"failures cannot starve unvisited sources");
            now+=100;
        }
        assert_eq!(h.seen.len(),1005);
        assert!(h.through.is_none());
        h=History::seed(&h.json(&board.url(0)),&board.url(0));
        reads.borrow_mut().clear();
        h.refresh(board.as_ref(),now);
        assert_eq!(reads.borrow().len(),2,"only the incremental list and failed source are retried");
        assert!(h.missing.iter().any(|s|s.contains("unavailable")));
        unavailable.set(false);
        h.refresh(board.as_ref(),now+100);
        assert!(h.through.is_some_and(|t|t>now+100-FULL_SCAN) && h.through==h.full,"coverage runs through the last scan of every issue");
        h=History::seed(&h.json(&board.url(0)),&board.url(0));
        reads.borrow_mut().clear();
        h.refresh(board.as_ref(),now+200);
        assert_eq!(reads.borrow().len(),1,"a settled restart reads no event histories");
        assert!(h.missing.is_empty());
    }

    // #1960 defect: a source whose complete history could not be used was read again on every pass, growing failed reads.
    // Chain: §4 admission records. Risk kept: a settled source proves zero, or a transient failure stops retrying.
    #[test]
    fn permanent_source_failure_settles_until_update_and_transient_retries() {
        let board=Fake::default();
        board.state.borrow_mut().clock=99;
        let bad=board.create("bad","",&["taskq".into()]).unwrap();
        let flaky=board.create("flaky","",&["taskq".into()]).unwrap();
        board.state.borrow_mut().admissions.insert(bad,Ok(Some((0,100))));
        board.state.borrow_mut().admissions.insert(flaky,Err("HTTP 502".into()));
        let mut h=History::default();
        h.refresh(&board,200);
        assert!(h.failed.contains_key(&bad));assert!(!h.pending.contains_key(&bad));
        assert!(h.pending.contains_key(&flaky));
        // A good answer is not read until the issue updates, and a restart keeps the settled state.
        board.state.borrow_mut().admissions.insert(bad,Ok(Some((71,100))));
        h=History::seed(&h.json(&board.url(0)),&board.url(0));
        for now in [300,400] {
            h.refresh(&board,now);
            assert_eq!(h.pending[&flaky].2,now,"a transient failure retries every pass");
            assert!(h.missing.iter().any(|s|s==&format!("#{bad}: invalid first-admission provenance")),"{:?}",h.missing);
            assert!(h.through.is_none(),"a failed source never proves zero");
        }
        assert!(h.json(&board.url(0))["events"].as_array().unwrap().is_empty());
        board.state.borrow_mut().admissions.insert(flaky,Ok(Some((72,100))));
        board.state.borrow_mut().clock=449;
        board.comment(bad,"edit").unwrap();
        h.refresh(&board,500);
        assert!(h.failed.is_empty()&&h.pending.is_empty()&&h.missing.is_empty(),"{:?}",h.missing);
        assert_eq!(h.through,Some(500));
        assert_eq!(h.json(&board.url(0))["events"].as_array().unwrap().len(),2);
    }

    // UX30-21: a stalled history API cannot delay the service heartbeat or admission.
    #[test]
    fn history_reader_tick_never_waits_for_collection() {
        let (request,requests)=std::sync::mpsc::sync_channel(1);
        let (responses,response)=std::sync::mpsc::channel();
        let mut reader=Reader {request,response,busy:false,value:Value::Null};
        let began=std::time::Instant::now();
        for _ in 0..1000 {assert_eq!(reader.tick(),Value::Null);}
        assert!(began.elapsed()<std::time::Duration::from_secs(1));
        requests.try_recv().unwrap();
        responses.send(json!({"schema":1,"end":100,"missing":[]})).unwrap();
        assert_eq!(reader.tick()["end"],100);
        assert!(reader.busy);
        drop(responses);
        assert_eq!(reader.tick()["missing"],json!(["admission reader exited"]));
        assert_eq!(reader.value["end"],100,"a dead collector retains bounded historical evidence");
    }

    #[test]
    fn corrupt_or_foreign_source_never_proves_zero() {
        let valid=json!({"schema":1,"board":"u/0","start":1,"end":2,"events":[],"missing":[]});
        assert!(super::valid(&valid,"u/0"));
        assert!(!super::valid(&valid,"other/0"));
        for (key,value) in [("schema",json!(2)),("events",Value::Null),("start",json!(3)),("missing",json!([1]))] {
            let mut bad=valid.clone();bad[key]=value;assert!(!super::valid(&bad,"u/0"));
        }
        let mut bad=valid.clone();bad["events"]=json!([[1,"new","",7]]);assert!(!super::valid(&bad,"u/0"));
    }

    // I3 / R12: cache compatibility must preserve board provenance and never skip unread history.
    #[test]
    fn cache_loss_corruption_and_clock_gaps_keep_coverage_unknown() {
        let board=Fake::default();
        board.state.borrow_mut().clock=99;
        let task=board.create("task","",&["taskq".into()]).unwrap();
        board.state.borrow_mut().admissions.insert(task,Ok(Some((71,100))));
        let mut h=History::default();
        h.refresh(&board,200);
        let snapshot=h.json(&board.url(0));
        let mut legacy=snapshot.clone();legacy.as_object_mut().unwrap().remove("cache");
        let mut corrupt=snapshot.clone();corrupt["cache"]["seen"]=json!({"0":[100,null]});
        for v in [legacy,corrupt] {
            let mut rebuilt=History::seed(&v,&board.url(0));
            assert!(rebuilt.listed.is_none(),"legacy or corrupt scan progress requires a rebuild");
            assert_eq!(rebuilt.json(&board.url(0))["events"],snapshot["events"],"independent valid event provenance survives");
            rebuilt.refresh(&board,300);
            assert!(rebuilt.missing.is_empty());
            assert_eq!(rebuilt.through,Some(300));
        }
        let foreign=History::seed(&snapshot,"other board");
        assert!(foreign.through.is_none());assert!(foreign.seen.is_empty());
        let mut restarted=History::seed(&snapshot,&board.url(0));
        restarted.refresh(&board,199);
        assert_eq!(restarted.through,Some(200));
        assert!(restarted.missing[0].contains("clock"));
        // Recent updates wait for settled event history, including after a cache round trip.
        board.state.borrow_mut().clock=489;
        let held=board.create("held","",&["q-held".into()]).unwrap();
        restarted.refresh(&board,500);
        assert_eq!(restarted.through,Some(200));
        restarted=History::seed(&restarted.json(&board.url(0)),&board.url(0));
        restarted.refresh(&board,600);
        // #2083: this pass scans `taskq` issues only, so coverage ends at the scan of every issue that saw the draft
        assert_eq!(restarted.through,Some(500));assert!(restarted.missing.is_empty());
        board.state.borrow_mut().clock=649;
        board.labels(held,&["taskq".into()],&["q-held".into()]).unwrap();
        restarted.refresh(&board,700);
        assert_eq!(restarted.through,Some(500));
        assert!(restarted.missing.iter().any(|s|s.contains("history unavailable")));
        restarted.refresh(&board,MONTH+700);
        assert!(restarted.pending.is_empty());assert!(restarted.seen.is_empty(),"the scan cache is month-bounded");
        assert_eq!(restarted.through,Some(MONTH+700));
    }

    // Chain: M6. UX30-21: first label admission survives restarts, requeues, removal and closure; absent history is unknown.
    #[test]
    fn first_admission_rebuilds_without_inventing_arrivals() {
        let board = Fake::default();
        let held = board.create("held draft", "", &["q-held".into()]).unwrap();
        let mut h = History::default();
        h.refresh(&board, 100);
        assert_eq!(h.json("board")["events"], json!([]));
        board.state.borrow_mut().clock=119;
        board.labels(held, &["taskq".into()], &["q-held".into()]).unwrap();
        board.state.borrow_mut().admissions.insert(held, Ok(Some((71, 120))));
        for kind in ["start", "spawn worker codex:x mac", "requeue", "spawn worker claude:y mac"] { board.comment(held, &format!("taskq: {kind}")).unwrap(); }
        h.refresh(&board, 200);
        assert_eq!(h.json("board")["events"], json!([[120,"new","71",held]]));
        board.state.borrow_mut().clock=249;
        board.close(held).unwrap();
        board.labels(held, &[], &["taskq".into()]).unwrap();
        let mut rebuilt = History::default();
        rebuilt.refresh(&board, 300);
        assert_eq!(rebuilt.json("board")["events"], h.json("board")["events"], "the board's original event survives cache loss");
        board.state.borrow_mut().clock=349;
        let legacy = board.create("legacy", "", &["taskq".into()]).unwrap();
        rebuilt.refresh(&board, 400);
        let partial = rebuilt.json("board");
        assert_eq!(partial["end"], 300);
        assert!(partial["missing"].to_string().contains(&format!("#{legacy}")));
        assert_eq!(partial["events"], h.json("board")["events"], "retain known lower bounds");
        board.state.borrow_mut().admissions.insert(legacy, Ok(Some((99, 350))));
        // #2083: coverage ends at the next scan of every issue, at most FULL_SCAN after the last
        rebuilt.refresh(&board, 300 + FULL_SCAN);
        assert_eq!(rebuilt.json("board")["end"], 300 + FULL_SCAN);
        assert_eq!(rebuilt.json("board")["events"].as_array().unwrap().len(), 2);
    }
}
