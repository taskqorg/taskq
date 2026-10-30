//! I3 / UX30-21: a rebuildable projection of first queue admissions, sourced only from board label events.
use crate::board::{Board, Issue};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MONTH: i64 = 30 * 86400;

/// Observation stays off the service's heartbeat/admission path, including a slow or unavailable board.
pub struct Reader {
    request: std::sync::mpsc::SyncSender<()>,
    response: std::sync::mpsc::Receiver<Value>,
    busy: bool,
    value: Value,
}
impl Reader {
    pub fn new(cfg: crate::service::Config, schema: Option<String>) -> Self {
        let (request, requests)=std::sync::mpsc::sync_channel(1);
        let (responses, response)=std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let board=cfg.board.open(&cfg.web_host,&cfg.repo,schema.as_deref(),&crate::platform::tool(&cfg.install));
            let mut history=History::default();
            while let Ok(())=requests.recv() {
                let value=match &board {
                    Ok(board)=>{
                        match board.now() {Ok(now)=>history.refresh(board.as_ref(),now),Err(reason)=>history.missing=vec![reason]}
                        history.json(&board.url(0))
                    },
                    Err(reason)=>json!({"reason":reason}),
                };
                if responses.send(value).is_err() {break;}
            }
        });
        Self {request,response,busy:false,value:Value::Null}
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

#[derive(Default)]
pub struct History {
    // Updated issue and its last verified first admission; no task state or launch authority.
    seen: BTreeMap<u64, (i64, Option<(u64, i64)>)>,
    through: Option<i64>,
    start: Option<i64>,
    missing: Vec<String>,
}

impl History {
    pub fn refresh(&mut self, board: &dyn Board, now: i64) {
        let floor = now - MONTH;
        let after = self.through.map_or(floor, |t| (t - 30).max(floor));
        let issues = match board.issues(Some(after)) {
            Ok(issues) => issues,
            Err(e) => { self.missing = vec![format!("admission scan: {e}")]; return; }
        };
        let mut missing = vec![];
        for issue in issues.iter().filter(|i| i.trusted && !crate::service::is_host(i)) {
            // Leave recently modified issues uncached until the board's event list has settled.
            if self.seen.get(&issue.number).is_some_and(|(updated, _)| *updated == issue.updated && now >= issue.updated + 30) { continue; }
            match admission(board, issue).and_then(|event| if event.is_some_and(|(_, at)| at > now) {Err("first-admission time is in the future".into())} else {Ok(event)}) {
                Ok(event) => { self.seen.insert(issue.number, (if now >= issue.updated + 30 {issue.updated} else {-1}, event)); }
                Err(e) => { missing.push(format!("#{}: {e}", issue.number)); }
            }
        }
        // An unresolved source must be retried, including after its issue leaves the update overlap.
        if missing.is_empty() {
            self.start = Some(self.start.unwrap_or(floor).max(floor));
            self.through = Some(now);
        }
        self.missing = missing;
        self.seen.retain(|_, (updated, event)| *updated>=floor || event.is_some_and(|(_, at)| at>=floor));
    }

    pub fn json(&self, board: &str) -> Value {
        let events: Vec<_> = self.seen.iter().filter_map(|(n, (_, event))| event.map(|(id, at)| json!([at, "new", id.to_string(), n]))).collect();
        json!({"schema":1, "board":board, "start":self.start, "end":self.through, "events":events, "missing":self.missing})
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

fn admission(board: &dyn Board, issue: &Issue) -> Result<Option<(u64, i64)>, String> {
    let event = board.first_admission(issue.number)?;
    if event.is_some_and(|(id, at)| id == 0 || at <= 0) { return Err("invalid first-admission provenance".into()); }
    if event.is_none() && crate::service::is_task(issue) { return Err("first-admission history unavailable".into()); }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Fake;

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
        rebuilt.refresh(&board, 500);
        assert_eq!(rebuilt.json("board")["end"], 500);
        assert_eq!(rebuilt.json("board")["events"].as_array().unwrap().len(), 2);
    }
}
