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
