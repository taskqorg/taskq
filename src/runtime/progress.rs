//! R6/R12: bounded advisory observation; never feeds execution release or admission.
use super::NativeHistory;
use serde_json::{Value,json};
use std::{collections::BTreeMap,path::Path};

#[derive(Default)]
pub struct Reader {
    history: NativeHistory,
    tools: BTreeMap<String,(i64,Option<i64>)>,
    last: Option<i64>,
    seen: bool,
    error: Option<&'static str>,
}

pub fn unknown(reason:&str)->Value {
    json!({"source":"native execution events","status":"unknown","last_event":null,"reason":reason})
}

/// Host metadata is optional observation, never an implicit healthy fallback for corrupt data.
pub fn valid(p:&Value)->bool {
    let observed=p["observed"].as_i64().filter(|t|*t>0);
    let time=|k:&str|p[k].is_null() || p[k].as_i64().zip(observed).is_some_and(|(t,now)|t>0 && t<=now);
    let known=matches!(p["status"].as_str(),Some("quiet"|"active"|"overdue"));
    p["source"]=="native execution events" && p["reason"].as_str().is_some_and(|s|!s.is_empty() && s.chars().count()<=160 && !s.chars().any(char::is_control))
        && (p["status"]=="unknown" || (known && observed.is_some() && time("last_event") && time("tool_since")
            && (p["status"]!="overdue" || (p["overdue_since"].is_i64() && time("overdue_since")))
            && p["pending_tools"].as_u64().is_some_and(|n|n<=128 && if p["status"]=="quiet" {n==0 && p["last_event"].is_i64()} else {n>0 && p["tool_since"].is_i64()})))
}

impl Reader {
    pub fn read(&mut self,path:&Path,start:u64,mut clock:impl FnMut()->i64,poll:u64)->Value {
        let modified=std::fs::metadata(path).and_then(|m|m.modified()).map_err(|_|"native progress source unreadable")
            .and_then(|at|at.duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs() as i64).map_err(|_|"native progress time invalid"));
        let mut now=clock();
        let result=(|| {
            let modified=modified?;
            if modified>now || modified<=0 {return Err("native progress time is future or unknown");}
            let mut at=self.history.offset;
            let mut events=Vec::new();
            let complete=self.history.lines(path,|line| {
                if at>=start && line.trim_start().starts_with('{') {
                    events.push(serde_json::from_str::<Value>(line).map_err(|_|"native progress record invalid".to_string())?);
                }
                at+=line.len() as u64;Ok(())
            });
            // A hot log may append across a second boundary during the bounded scan.
            now=clock();
            for v in events {
                let at=match v.get("timestamp") {
                    Some(Value::String(s))=>crate::board::iso(s).filter(|t|*t>0 && *t<=now),
                    None=>(modified<=now).then_some(modified),
                    _=>None,
                };
                let Some(at)=at else {self.error=Some("native progress timestamp invalid or future");continue;};
                self.event(&v,at);
            }
            if !complete.map_err(|_|"native progress read failed, changed or incomplete")? {return Err("native progress scan pending");}
            if let Some(reason)=self.error {return Err(reason);}
            if !self.seen {return Err("no supported native progress event");}
            Ok(())
        })();
        let mut value=match result {Err(reason)=>unknown(reason),Ok(())=>self.value(now,poll)};
        value["observed"]=json!(now);
        value
    }

    fn event(&mut self,v:&Value,at:i64) {
        let kind=v["type"].as_str().or(v["event"].as_str()).unwrap_or("");
        if matches!(kind,"taskq.input"|"turn.started"|"init") {self.tools.clear();self.last=None;self.seen=false;self.error=None;}
        let s=&v["step_update"];
        if kind=="step_update" {
            if s["step_type"]=="tool" {
                match (s["step_index"].as_u64(),s["state"].as_str()) {
                    (Some(id),Some(state @ ("ACTIVE"|"DONE"|"ERROR"|"FAILED")))=>self.tool(format!("{}:{id}",s["conversation_id"].as_str().unwrap_or("")),state=="ACTIVE",at,timeout(&s["tool_info"]["parameters"])),
                    _=>self.error=Some("native tool identity or state unsupported"),
                }
            } else if s["state"]=="DONE" {self.completed(at);}
        }
        if matches!(kind,"item.started"|"item.completed") {
            let item=&v["item"];
            if matches!(item["type"].as_str(),Some("command_execution"|"mcp_tool_call"|"web_search")) {
                if let Some(id)=item["id"].as_str() {self.tool(id.into(),kind=="item.started",at,timeout(item));}
            } else if kind=="item.completed" {self.completed(at);}
        }
        let p=if kind=="response_item" {&v["payload"]} else {v};
        match p["type"].as_str() {
            Some("function_call"|"custom_tool_call")=>if let Some(id)=p["call_id"].as_str() {
                let input=p["arguments"].as_str().and_then(|s|serde_json::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
                self.tool(id.into(),true,at,timeout(&input));
            },
            Some("function_call_output"|"custom_tool_call_output")=>if let Some(id)=p["call_id"].as_str() {self.tool(id.into(),false,at,None);},
            Some("assistant"|"user")=>{
                for block in p["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("tool_use")=>if let Some(id)=block["id"].as_str() {self.tool(id.into(),true,at,timeout(&block["input"]));},
                        Some("tool_result")=>if let Some(id)=block["tool_use_id"].as_str() {self.tool(id.into(),false,at,None);},
                        Some("text")=>self.completed(at),_=>{}
                    }
                }
            },
            Some("message"|"reasoning")=>self.completed(at),_=>{}
        }
        if (kind=="system" && v["subtype"]=="turn_duration") || matches!(kind,"turn.completed"|"turn.failed"|"result") || v["event"]=="result"
            || (kind=="event_msg" && matches!(v["payload"]["type"].as_str(),Some("task_complete"|"turn_aborted"))) {
            self.tools.clear();self.error=None;self.completed(at);
        }
    }

    fn completed(&mut self,at:i64) {self.last=Some(at);self.seen=true;}
    fn tool(&mut self,id:String,active:bool,at:i64,timeout:Option<i64>) {
        self.seen=true;
        if id.len()>256 || (active && self.tools.len()>=128 && !self.tools.contains_key(&id)) {
            self.error=Some("native pending tool identity bound exceeded");return;
        }
        if active {self.tools.entry(id).or_insert((at,timeout));}
        else {self.tools.remove(&id);self.completed(at);}
    }
    fn value(&self,now:i64,poll:u64)->Value {
        let overdue_since=self.tools.values().filter_map(|(at,timeout)| {
            let deadline=timeout.map_or_else(||(*at).max(self.last.unwrap_or(*at)).saturating_add(900),|s|at.saturating_add(s).saturating_add((poll.saturating_mul(2)).min(i64::MAX as u64) as i64));
            (now>deadline).then_some(deadline)
        }).min();
        let overdue=overdue_since.is_some();
        let status=if overdue {"overdue"} else if self.tools.is_empty() {"quiet"} else {"active"};
        json!({"source":"native execution events","clock":"native timestamp or append mtime lower bound","status":status,"last_event":self.last,
            "deadline":self.tools.values().filter_map(|(at,s)|s.map(|s|at.saturating_add(s))).min(),"overdue_since":overdue_since,"tool_since":self.tools.values().map(|(at,_)|at).min(),"pending_tools":self.tools.len(),
            "reason":if overdue {"tool progress overdue; preserve execution and inspect owning session"} else if self.tools.is_empty() {"no pending tool; quiet work is not failure"} else {"tool pending within observation deadline"}})
    }
}

fn timeout(v:&Value)->Option<i64> {
    [("timeout_ms",1000),("timeout",1000),("timeout_seconds",1)].into_iter().find_map(|(k,unit)|v[k].as_u64().filter(|n|*n>0)
        .and_then(|n|i64::try_from(n.div_ceil(unit)).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    // A4: reproduce the 6620 s Antigravity tool with no timeout; no board/process action exists here.
    #[test]
    fn unbounded_tool_stall_clears_only_on_progress_and_preserves_concurrent_tools() {
        let mut r=Reader::default();
        let event=|id,state|json!({"event":"step_update","step_update":{"conversation_id":"invented","step_index":id,"state":state,"step_type":"tool"}});
        r.event(&event(90,"ACTIVE"),1000);
        assert_eq!(r.value(1900,60)["status"],"active");
        assert_eq!(r.value(1901,60)["status"],"overdue");
        r.event(&event(91,"ACTIVE"),1902);
        assert_eq!(r.value(7620,60)["status"],"overdue");
        r.event(&event(90,"DONE"),7621);
        assert_eq!(r.value(7621,60)["pending_tools"],1);
        assert_eq!(r.value(7621,60)["status"],"active");
        r.event(&event(91,"DONE"),7622);
        assert_eq!(r.value(99999,60)["status"],"quiet","quiet reasoning never fails on age alone");
    }
    #[test]
    fn explicit_tool_deadline_keeps_its_budget_and_completed_events_do_not_extend_it() {
        assert_eq!(timeout(&json!({"timeout":120000})),Some(120),"Claude Bash uses milliseconds");
        let mut r=Reader::default();
        r.event(&json!({"type":"response_item","payload":{"type":"function_call","call_id":"c","arguments":"{\"timeout_ms\":3600000}"}}),1000);
        r.completed(4500);
        assert_eq!(r.value(4720,60)["status"],"active");
        assert_eq!(r.value(4721,60)["status"],"overdue");
        r.event(&json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"c"}}),4722);
        assert_eq!(r.value(99999,60)["status"],"quiet");
    }
    #[test]
    fn timestamp_fault_stays_unknown_until_a_valid_turn_boundary() {
        use std::{fs,io::Write};
        let dir=std::env::temp_dir().join(format!("taskq-progress-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let path=dir.join("events");
        let now=1000;
        // Keep filesystem time on the injected clock; wall-clock rollover must not invent future data.
        let pin_time=||fs::File::options().write(true).open(&path).unwrap().set_times(
            fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH+std::time::Duration::from_secs(now as u64))).unwrap();
        let content=|mut v:Value| {v.as_object_mut().unwrap().remove("observed");v};
        // R12/A4: consumed bad timestamps must not hide a pending tool on later incremental reads.
        for timestamp in [json!("invalid"),json!(crate::board::stamp(now+100)),json!(now),json!(false),Value::Null] {
            fs::write(&path,format!("{}\n",json!({"type":"item.completed","item":{"type":"agent_message"}}))).unwrap();pin_time();
            let mut r=Reader::default();
            assert_eq!(r.read(&path,0,||now,60)["status"],"quiet");
            let mut f=fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(f,"{}",json!({"timestamp":timestamp,"type":"item.started","item":{"id":"c","type":"command_execution"}})).unwrap();pin_time();
            let fault=r.read(&path,0,||now+1,60);
            assert_eq!(fault["status"],"unknown");
            assert_eq!(content(r.read(&path,0,||now+101,60)),content(fault.clone()),"time and an empty read cannot recover a consumed event");
            writeln!(f,"{}",json!({"type":"item.completed","item":{"type":"agent_message"}})).unwrap();pin_time();
            assert_eq!(content(r.read(&path,0,||now+102,60)),content(fault.clone()),"unrelated progress cannot prove the missing tool ended");
            writeln!(f,"{}",json!({"type":"turn.completed"})).unwrap();pin_time();
            assert_eq!(r.read(&path,0,||now+103,60)["status"],"quiet");
            writeln!(f,"{}\n{}",json!({"timestamp":"invalid","type":"item.started","item":{"id":"c","type":"command_execution"}}),json!({"type":"turn.completed"})).unwrap();pin_time();
            assert_eq!(r.read(&path,0,||now+104,60)["status"],"quiet","a valid boundary in the same batch is not discarded");
        }
        fs::remove_dir_all(dir).unwrap();
    }
    // R6/R12: sample after metadata and after the bounded read, without tolerating actual future data.
    #[test]
    fn hot_log_samples_time_after_read_and_keeps_future_metadata_fenced() {
        use std::{fs,io::Write,time::{UNIX_EPOCH,Duration}};
        let dir=std::env::temp_dir().join(format!("taskq-progress-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let path=dir.join("events");
        fs::write(&path,"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\"}}\n").unwrap();
        let mut f=fs::OpenOptions::new().append(true).open(&path).unwrap();
        let mtime=|at|fs::FileTimes::new().set_modified(UNIX_EPOCH+Duration::from_secs(at));
        f.set_times(mtime(1000)).unwrap();
        let mut r=Reader::default();let mut samples=0;
        let value=r.read(&path,0,|| {
            samples+=1;
            if samples==1 {
                writeln!(f,"{}",json!({"timestamp":crate::board::stamp(1001),"type":"item.completed","item":{"type":"agent_message"}})).unwrap();
                f.set_times(mtime(1001)).unwrap();1000
            } else {1001}
        },60);
        assert_eq!(value["status"],"quiet");assert_eq!(value["last_event"],1001);assert_eq!(value["observed"],1001);
        assert!(valid(&value));
        let offset=r.history.offset;
        writeln!(f,"{}",json!({"timestamp":crate::board::stamp(1002),"type":"item.started","item":{"id":"c","type":"command_execution"}})).unwrap();
        f.set_times(mtime(1003)).unwrap();
        let future=r.read(&path,0,||1002,60);
        assert_eq!(future["status"],"unknown");assert_eq!(future["reason"],"native progress time is future or unknown");
        assert_eq!(r.history.offset,offset,"future metadata must not consume events");
        f.set_times(mtime(1002)).unwrap();
        let recovered=r.read(&path,0,||1002,60);
        assert_eq!(recovered["status"],"active");assert_eq!(recovered["pending_tools"],1);assert!(valid(&recovered));
        fs::write(&path,"{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\"}}\n").unwrap();f.set_times(mtime(1002)).unwrap();
        let mut reversed=Reader::default();let mut clock=[1002,1001].into_iter();
        assert_eq!(reversed.read(&path,0,||clock.next().unwrap(),60)["status"],"unknown","a clock reversal cannot make a future mtime fallback healthy");
        drop(f);fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn incremental_source_does_not_turn_reads_partial_writes_or_replacement_into_health() {
        use std::{fs,io::Write};
        let dir=std::env::temp_dir().join(format!("taskq-progress-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let path=dir.join("events");fs::write(&path,"{\"type\":\"item.started\",\"item\":{\"id\":\"c\",\"type\":\"command_execution\"}}\n").unwrap();
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let mut r=Reader::default();let first=r.read(&path,0,||now,60);
        assert_eq!(first["status"],"active");
        assert_eq!(r.read(&path,0,||now+901,60)["status"],"overdue","read is not progress");
        let mut f=fs::OpenOptions::new().append(true).open(&path).unwrap();write!(f,"{{").unwrap();
        assert_eq!(r.read(&path,0,||now+902,60)["status"],"unknown");
        writeln!(f,"\"type\":\"item.completed\",\"item\":{{\"id\":\"c\",\"type\":\"command_execution\"}}}}").unwrap();
        assert_eq!(r.read(&path,0,||now+903,60)["status"],"quiet");
        fs::rename(&path,dir.join("old")).unwrap();fs::write(&path,"{}\n").unwrap();
        assert_eq!(r.read(&path,0,||now+904,60)["status"],"unknown");
        fs::remove_dir_all(dir).unwrap();
    }
}
