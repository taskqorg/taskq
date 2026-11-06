    use super::*;

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-dialogue-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }

    fn request(install: &Path, id: &str, runtime: &str, root: Option<&Path>) -> Request {
        let r = Request { schema: 2, id: id.into(), created: 1, host: "h".into(), platform: "macos".into(), client: "c".into(), runtime: runtime.into(), targets: vec![], brief: "b".into(),
            state: "launched".into(), job: Some("j".into()), session: Some("0b4eaf42-5e87-4d57-9d31-c37c62bfbc2c".into()), url: None, reason: None, root: root.map(|p| p.display().to_string()), pm: false, choice:None };
        fs::create_dir_all(crate::setup::dir(install).join(id)).unwrap();
        fs::write(crate::setup::dir(install).join(format!("{id}.json")), serde_json::to_string(&r).unwrap()).unwrap();
        r
    }

    fn free(_: &Path, _: &Request) -> Res<bool> { Ok(false) }
    fn held(_: &Path, _: &Request) -> Res<bool> { Ok(true) }
    fn never(_: &Path, _: &Request) -> Res<std::process::Command> { panic!("no turn may start") }
    fn claude_ok(_: &Path, _: &Request) -> Res<std::process::Command> {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", r#"cat >/dev/null; echo '{"type":"system"}'; echo '{"type":"result","subtype":"success","is_error":false}'"#]);
        Ok(c)
    }
    fn exits(_: &Path, _: &Request) -> Res<std::process::Command> { let mut c = std::process::Command::new("sh"); c.args(["-c", "cat >/dev/null"]); Ok(c) }
    fn codex_writer(_: &Path, _: &Request) -> Res<std::process::Command> {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", "cat >/dev/null; echo 'Error: thread 0b4e already has an active writer' >&2; exit 1"]);
        Ok(c)
    }

    fn settle(install: &Path, id: &str, native: &Native) -> Turn {
        for _ in 0..200 {
            tick(install, now(), native).unwrap();
            let t = turn(install, id).unwrap().unwrap();
            if t.state == "done" { return t; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("turn never ended")
    }

    // D2/D5/#2159: App-owned replies never spawn a CLI writer; lost queue/restart only observe exact input.
    #[test]
    fn app_owner_reply_keeps_one_submission_and_native_receipts() {
        fn baseline(_: &Path,_:&Request)->Res<(usize,u64)>{Ok((9,100))}
        fn submit(own:&Path,_:&Request,input:&str)->Res<()>{
            let path=own.join("queued");assert!(!path.exists(),"no duplicate native App submission");fs::write(path,input).unwrap();Err("queue reply lost".into())
        }
        fn observe(own:&Path,_:&Request,t:&Turn)->Res<AppState>{
            let state=fs::read_to_string(own.join("observed")).unwrap_or_default();
            assert!(t.app_baseline.is_some());
            Ok(AppState {accepted:state!="",active:state=="active",outcome:(state=="done").then(||"success".into())})
        }
        let install=tmp();let r=request(&install,"owner","codex",None);let own=own(&install,"owner").unwrap();
        let native=Native {app:Some(AppNative{baseline,submit,observe}),turn:never,holder:free};
        send_op(&install,"owner","Exact owner reply",Some("original-op"),&native).unwrap();tick(&install,now(),&native).unwrap();
        assert!(!accepted(&install,"owner","original-op","Exact owner reply").unwrap(),"durable intent is not App input acceptance");
        let first=turn(&install,"owner").unwrap().unwrap();assert_eq!(first.app_baseline,Some(9));assert_eq!(first.input,"Exact owner reply");assert_eq!(first.state,"running");assert!(!first.app_accepted);
        let mut cold=first.clone();cold.state="dispatching".into();store(&own.join("turn.json"),&cold).unwrap();
        tick(&install,now(),&native).unwrap();assert_eq!(fs::read_to_string(own.join("queued")).unwrap(),first.input);
        fs::write(own.join("observed"),"active").unwrap();tick(&install,now(),&native).unwrap();
        assert!(turn(&install,"owner").unwrap().unwrap().app_accepted);assert!(accepted(&install,"owner","original-op","Exact owner reply").unwrap());assert_eq!(status(&install,&r,false,&native)["state"],"working");
        fs::write(own.join("observed"),"done").unwrap();tick(&install,now(),&native).unwrap();
        assert_eq!(turn(&install,"owner").unwrap().unwrap().outcome.as_deref(),Some("success"));
        fs::remove_file(own.join("turn.json")).unwrap();fs::write(own.join("observed"),"active").unwrap();
        // The initial brief is still executing tools; launch/queue alone cannot show Ready.
        assert_ne!(status(&install,&r,true,&native)["state"],"idle");
        let old=Turn {op_id:Some("refused-op".into()),input:"Keep refused input".into(),state:"done".into(),outcome:Some(crate::runtime::owner::open_in("codex")),pid:Some(std::process::id()),started:Some("prior-incarnation".into()),..Turn::default()};
        store(&own.join("turn.json"),&old).unwrap();
        assert!(retry_refused(&install,"owner","different-op",&native).is_err());
        retry_refused(&install,"owner","refused-op",&native).unwrap();let retried=turn(&install,"owner").unwrap().unwrap();
        assert_eq!(retried.input,old.input);assert_eq!(retried.op_id,old.op_id);assert_eq!(retried.prior_outcome,old.outcome);assert_eq!(retried.state,"pending");
        assert_eq!(fs::read_to_string(own.join("queued")).unwrap(),first.input,"explicit retry is admitted, not submitted by the UI call");
        fs::remove_dir_all(install).unwrap();
    }

    // A5/R12: late native success cannot certify a reply refused before stdin.
    #[test]
    fn antigravity_unsent_reply_keeps_its_unknown_fence() {
        let install=tmp();let r=request(&install,"owner","antigravity",None);let own=own(&install,"owner").unwrap();
        let t=Turn{op_id:Some("unsent-op".into()),input:"Unsent reply".into(),state:"running".into(),stream_offset:Some(0),outcome:Some("unknown: native startup unverified; input not sent".into()),..Default::default()};
        store(&own.join("turn.json"),&t).unwrap();
        fs::write(crate::runtime::owner_stream::log(&own),format!("{}\n{}\n{}\n",json!({"event":"init","conversation_id":r.session}),json!({"event":"step_update","step_update":{"conversation_id":r.session,"step_type":"user_input","state":"DONE"}}),json!({"event":"result","result":{"conversation_id":r.session,"status":"SUCCESS","response":"late unrelated result"}}))).unwrap();
        assert_eq!(ended(&own,&r,&t),t.outcome,"unsent reply cannot become success");
        assert!(!accepted(&install,"owner","unsent-op","Unsent reply").unwrap(),"no input write, no accepted reply");
        fs::remove_dir_all(install).unwrap();
    }

    // #1929 A6: same operation readback never starts or queues another native turn.
    #[test]
    fn conversation_1929_reply_readback_binds_input_and_operation() {
        let install=tmp();request(&install,"owner","claude",None);
        let native=Native{app:None,turn:never,holder:free};
        assert!(!accepted(&install,"owner","op-a","Hello").unwrap());
        send_op(&install,"owner","Hello",Some("op-a"),&native).unwrap();
        let first=turn(&install,"owner").unwrap();
        assert!(accepted(&install,"owner","op-a","Hello").unwrap());
        assert!(!accepted(&install,"owner","op-b","Hello").unwrap());
        assert!(!accepted(&install,"owner","op-a","Changed").unwrap());
        send_op(&install,"owner","Hello",Some("op-a"),&Native{app:None,turn:never,holder:held}).unwrap();
        assert_eq!(turn(&install,"owner").unwrap(),first);
        assert!(send_op(&install,"owner","Changed",Some("op-a"),&native).is_err());
        fs::remove_dir_all(install).unwrap();
    }

    // #1929 A6: a due watch cannot replace the only receipt while its reply acknowledgement is lost.
    #[test]
    fn conversation_1929_watch_waits_for_reply_acknowledgement() {
        let install=tmp();request(&install,"owner","claude",None);
        crate::conversations::act(&install,&json!({"conversation":"owner","action":"save","revision":0,"draft":{"title":"","root":"","text":"Keep reply","caret":[0,0],"scroll":0}}),&[]);
        let mut p=crate::conversations::preferences(&install).unwrap();
        p["conversations"]["owner"]["reply"]=json!({"id":"operation","input":"Immutable input","revision":1});
        crate::conversations::persist(&install,&p).unwrap();
        let native=Native{app:None,turn:never,holder:free};
        let path=own(&install,"owner").unwrap().join("turn.json");
        let t=Turn{op_id:Some("operation".into()),input:"Immutable input".into(),state:"dispatching".into(),..Default::default()};
        store(&path,&t).unwrap();set_watch(&install,"owner","Watch input",5,&[],0).unwrap();
        tick(&install,6*MINUTE,&native).unwrap();
        let finished=turn(&install,"owner").unwrap().unwrap();assert_eq!(finished.state,"done");
        tick(&install,6*MINUTE,&native).unwrap();
        assert_eq!(turn(&install,"owner").unwrap(),Some(finished),"watch must leave the receipt intact until acknowledgement");
        let check=crate::conversations::act(&install,&json!({"conversation":"owner","action":"reply-check","op_id":"operation"}),&[]);
        assert_eq!(check["state"],"accepted");
        let ack=crate::conversations::act(&install,&json!({"conversation":"owner","action":"save","revision":1,"draft":{"title":"","root":"","text":"New independent draft","caret":[0,0],"scroll":0,"reply_ack":"operation"}}),&[]);
        assert_eq!(ack["state"],"accepted");
        tick(&install,6*MINUTE,&native).unwrap();
        let next=turn(&install,"owner").unwrap().unwrap();assert!(next.watch);assert_eq!(next.input,"Watch input");
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn launch_brief_is_not_an_owner_message() {
        let brief = "You are a TaskQ session opened by the host service.\nProject: p\n\nWhy is nothing starting?\n";
        let mut items = vec![json!({"role":"user","text":brief}), json!({"role":"assistant","text":"ok"}), json!({"role":"user","text":"Why is nothing starting?"})];
        mark_brief(&mut items, brief, Path::new("/owned"));
        assert_eq!(items.iter().map(|i| i["role"].as_str().unwrap()).collect::<Vec<_>>(), ["brief", "assistant", "user"]);
        mark_brief(&mut items, "", Path::new("/owned"));
        assert_eq!(items[2]["role"], "user", "an empty brief marks nothing");
        let own=Path::new("/owned");let pointer=crate::runtime::owner::read_brief(&own.join("brief.txt"));
        let mut transport=vec![json!({"role":"user","text":crate::runtime::HANDSHAKE}),json!({"role":"assistant","text":"OK"}),json!({"role":"user","text":pointer}),json!({"role":"user","text":"Before any work, read a different file"})];
        mark_brief(&mut transport,brief,own);
        assert_eq!(transport.iter().map(|i|i["role"].as_str().unwrap()).collect::<Vec<_>>(),["transport","transport","brief","user"]);
    }

    // D-A1/A2 source: the input persists before dispatch; the outcome comes from the result record only.
    // #1825 A3: in a worker transcript the per-turn brief pointer is TaskQ's and every other input is the owner's
    #[test]
    fn worker_inputs_name_their_author() {
        let mut items = vec![json!({"role": "user", "text": "Before any work, use Read to read \"x\" in full."}), json!({"role": "assistant", "text": "ok"}), json!({"role": "user", "text": "Also say PINEAPPLE"})];
        worker_roles(&mut items);
        assert_eq!(items.iter().map(|i| i["role"].as_str().unwrap()).collect::<Vec<_>>(), ["brief", "assistant", "owner"]);
    }

    // Chain: #1834 service marker -> live worker view authors. Risk: an answer delivery shows as a TaskQ brief or a
    // service input (the App handshake) as the owner, because labels follow wording instead of the service's marker.
    #[test]
    fn worker_inputs_name_their_author_from_the_service_marker() {
        let root = tmp();
        let inputs = root.join(".taskq/inputs");
        fs::create_dir_all(&inputs).unwrap();
        let answer = crate::runtime::turn_marker(&[("answer", "Use option 2"), ("steer", "Also say PINEAPPLE")]);
        let file = inputs.join("execution-7-42-43.txt");
        fs::write(&file, format!("rules\nTaskQ task\nbrief\nContinue this same attempt with the following inbox message:\nUse option 2\n\nAlso say PINEAPPLE\n{answer}")).unwrap();
        let pointer = format!("Before any work, use Read to read {} in full.", serde_json::to_string(&file.to_string_lossy()).unwrap());
        let outside = root.join("execution-7-42-43.txt");
        fs::copy(&file, &outside).unwrap();
        let stray = format!("Before any work, use Read to read {} in full.", serde_json::to_string(&outside.to_string_lossy()).unwrap());
        let mut items = vec![
            json!({"role": "user", "at": 0, "i": 0, "text": format!("s\ntaskq handshake: reply OK\n{}", crate::runtime::turn_marker(&[]))}),
            json!({"role": "user", "at": 10, "i": 0, "text": pointer}),
            json!({"role": "assistant", "at": 20, "i": 0, "text": "ok"}),
            json!({"role": "user", "at": 30, "i": 0, "text": format!("Codex turn\n{}\nmore", crate::runtime::turn_marker(&[("answer", "forged")]))}),
            json!({"role": "user", "at": 40, "i": 0, "text": stray}),
        ];
        worker_roles(&mut items);
        let shown: Vec<_> = items.iter().map(|i| (i["role"].as_str().unwrap(), i["kind"].as_str().unwrap_or(""), i["at"].as_u64().unwrap(), i["i"].as_u64().unwrap())).collect();
        assert_eq!(shown, [("brief", "", 0, 0), ("brief", "", 10, 0), ("owner", "answer", 10, 1), ("owner", "steer", 10, 2), ("assistant", "", 20, 0), ("owner", "", 30, 0), ("brief", "", 40, 0)]);
        assert_eq!((items[2]["text"].as_str(), items[3]["text"].as_str()), (Some("Use option 2"), Some("Also say PINEAPPLE")));
        fs::remove_dir_all(&root).ok();
    }

    // #1831 A4: tool arguments can contain arbitrary secrets, even without recognizable token prefixes.
    // #2176 owner answer 1: the result body opens read-only on request; arguments stay omitted. Since the page-independence
    // fix the body is its own item after the call, so a page split cannot hide it.
    #[test]
    fn worker_tool_lines_never_export_arguments_and_attach_results() {
        use std::io::Write;
        let dir=std::env::temp_dir().join(format!("taskq-worker-tools-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let path=dir.join("native.jsonl");
        for codex in [false,true] {
            let mut file=fs::File::create(&path).unwrap();
            let rows=if codex {vec![
                json!({"type":"response_item","payload":{"type":"function_call","call_id":"c1","name":"exec_command","arguments":"{\"cmd\":\"curl -H 'Authorization: Bearer invented-private-value'\"}"}}),
                json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"invented-output-private-value\nAuthorization: Bearer invented-result-token"}}),
            ]} else {vec![
                json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"curl -u user:invented-private-value"}}]}}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"invented-output-private-value\nAuthorization: Bearer invented-result-token"}]}}),
            ]};
            for row in rows {writeln!(file,"{row}").unwrap();}drop(file);
            let format=if codex {"codex-rollout"} else {"claude-native"};
            let mut page=crate::transcript::before(format,"-",&path,None).unwrap();worker_roles(&mut page.items);
            assert_eq!(page.items.len(),2);assert_eq!(page.items[0]["role"],"tool");assert_eq!(page.items[1]["role"],"tool");
            assert_eq!(page.items[0]["text"],if codex {"exec_command"} else {"Bash"});assert!(page.items[0]["result"].is_null());
            let body=page.items[1]["result"].as_str().unwrap();assert!(body.starts_with("invented-output-private-value\n"),"result body follows its call");assert!(!body.contains("invented-result-token"),"a credential in the output is masked like arguments");
            assert!(!json!(page.items).to_string().contains("user:invented")&&!json!(page.items).to_string().contains("Bearer invented"),"arguments never leave the adapter");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // #1825 A2 live probe, run by hand: TASKQ_PROBE_SESSION=<rt>:<id> follows a session on this host with the task
    // thread's 1 s poll and prints each new item's arrival in Unix milliseconds, to compare with the native stream.
    #[test]
    #[ignore]
    fn live_worker_stream_probe() {
        let session = std::env::var("TASKQ_PROBE_SESSION").expect("TASKQ_PROBE_SESSION");
        let (mut end, until) = (None, std::time::Instant::now() + std::time::Duration::from_secs(60));
        while std::time::Instant::now() < until {
            match worker_transcript(&session, Path::new(&std::env::var("TASKQ_PROBE_LOG").unwrap_or_default()), None, end) {
                Ok(v) => {
                    for i in v["items"].as_array().unwrap() { println!("{} {} {}", now(), i["role"], i["text"].as_str().unwrap_or_default().chars().take(40).collect::<String>().replace('\n', " ")); }
                    end = v["end"].as_u64();
                }
                Err(e) => println!("{} unknown: {e}", now()),
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    #[test]
    fn send_dispatches_one_turn_and_reads_its_outcome() {
        let install = tmp();
        let r = request(&install, "c1", "claude", None);
        let native = Native { app:None,turn: claude_ok, holder: free };
        send(&install, "c1", "hello", &Native { app:None,turn: never, holder: free }).unwrap();
        assert_eq!(turn(&install, "c1").unwrap().unwrap().state, "pending", "input persisted before dispatch");
        assert_eq!(send(&install, "c1", "again", &native).unwrap_err(), "working");
        assert_eq!(status(&install, &r, false, &native)["state"], "working");
        let t = settle(&install, "c1", &native);
        assert_eq!((t.input.as_str(), t.outcome.as_deref()), ("hello", Some("success")));
        assert_eq!(status(&install, &r, false, &native)["state"], "idle");
        send(&install, "c1", "next", &native).unwrap();
        assert_eq!(settle(&install, "c1", &Native { app:None,turn: exits, holder: free }).outcome.as_deref(), Some("unknown: the turn ended without a result record"), "exit alone is never success");
        fs::remove_dir_all(install).unwrap();
    }

    // D-A2 source: a Claude holder refuses at Send and at dispatch; Codex's second-writer refusal reads the same text.
    #[test]
    fn native_holder_refuses_with_d3_text() {
        let install = tmp();
        let r = request(&install, "c2", "claude", None);
        assert_eq!(send(&install, "c2", "hi", &Native { app:None,turn: never, holder: held }).unwrap_err(), "open in claude; close it there first");
        assert_eq!(turn(&install, "c2").unwrap(), None, "a refused Send queues nothing");
        send(&install, "c2", "hi", &Native { app:None,turn: never, holder: free }).unwrap();
        let t = settle(&install, "c2", &Native { app:None,turn: never, holder: held });
        assert_eq!(t.outcome.as_deref(), Some("open in claude; close it there first"));
        assert_eq!(status(&install, &r, false, &NATIVE)["state"], "open in claude; close it there first");
        let r = request(&install, "x2", "codex", None);
        send(&install, "x2", "hi", &Native { app:None,turn: never, holder: free }).unwrap();
        assert_eq!(settle(&install, "x2", &Native { app:None,turn: codex_writer, holder: free }).outcome.as_deref(), Some("open in codex; close it there first"));
        assert_eq!(status(&install, &r, false, &NATIVE)["state"], "open in codex; close it there first");
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn outcome_only_from_end_records() {
        use crate::runtime::owner::outcome;
        assert_eq!(outcome("claude", "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}\n", ""), "success");
        assert_eq!(outcome("claude", "{\"type\":\"result\",\"subtype\":\"error_max_turns\",\"is_error\":true}\n", ""), "failed: error_max_turns");
        assert_eq!(outcome("codex", "{\"type\":\"turn.completed\"}\n", ""), "success");
        assert_eq!(outcome("codex", "{\"type\":\"turn.failed\",\"error\":{\"message\":\"quota\"}}\n", ""), "failed: quota");
        assert!(outcome("codex", "{\"type\":\"turn.started\"}\n", "").starts_with("unknown"));
        assert!(outcome("claude", "", "already has an active writer").starts_with("unknown"), "only Codex reports a writer");
    }

    // D-A4/D-A5 source: schedule, events at once, hold pauses, restart keeps the watch and an unknown outcome waits for idle.
    #[test]
    fn watch_fires_on_schedule_and_events_and_pauses_under_hold() {
        let install = tmp();
        let root = tmp();
        fs::create_dir(root.join(".taskq")).unwrap();
        fs::write(root.join(".taskq/events"), "since 100\n100 #7 ask 1\n").unwrap();
        let r = request(&install, "w1", "claude", Some(&root));
        assert!(set_watch(&install, "w1", "check", 4, &[], 0).unwrap_err().contains("at least 5"));
        assert!(set_watch(&install, "w1", "check", 5, &["late".into()], 0).unwrap_err().contains("Unknown watch event"));
        let w = set_watch(&install, "w1", "check the board", 5, &["ask".into()], 0).unwrap();
        assert_eq!((w.next_due, w.seen), (5 * MINUTE, 100), "existing events are not news");
        let quiet = Native { app:None,turn: never, holder: free };
        tick(&install, MINUTE, &quiet).unwrap();
        assert_eq!(turn(&install, "w1").unwrap(), None);
        // schedule
        tick(&install, 5 * MINUTE, &quiet).unwrap();
        let t = turn(&install, "w1").unwrap().unwrap();
        assert_eq!((t.input.as_str(), t.state.as_str(), t.watch), ("check the board", "pending", true));
        let t = settle(&install, "w1", &Native { app:None,turn: exits, holder: free });
        assert!(t.outcome.unwrap().starts_with("unknown"));
        let w = watch(&install, "w1").unwrap().unwrap();
        assert_eq!((w.last_turn, w.next_due), (Some(5 * MINUTE), 10 * MINUTE));
        assert!(w.last_outcome.unwrap().starts_with("unknown"), "unknown stays unknown");
        // a new ask line makes it due at once; other kinds only advance the cursor
        fs::write(root.join(".taskq/events"), "since 100\n100 #7 ask 1\n200 #8 close 2\n201 #9 ask 3\n").unwrap();
        fs::write(install.join("hold"), "owner hold\n").unwrap();
        tick(&install, 6 * MINUTE, &quiet).unwrap();
        assert!(turn(&install, "w1").unwrap().unwrap().state == "done", "hold pauses the watch");
        send(&install, "w1", "owner", &quiet).unwrap(); // owner Send still works under hold
        fs::write(crate::setup::dir(&install).join("w1/turn.json"), serde_json::to_string(&t_running()).unwrap()).unwrap();
        fs::remove_file(install.join("hold")).unwrap();
        // restart: a turn that was starting is unknown and the watch waits for that pass to end
        tick(&install, 6 * MINUTE, &quiet).unwrap();
        assert_eq!(turn(&install, "w1").unwrap().unwrap().outcome.as_deref(), Some("unknown: the service stopped while the turn started; inspect the transcript"));
        tick(&install, 6 * MINUTE, &quiet).unwrap();
        let t = turn(&install, "w1").unwrap().unwrap();
        assert_eq!((t.input.as_str(), t.state.as_str()), ("check the board\n\nNew TaskQ events:\n201 #9 ask 3", "pending"));
        let s = status(&install, &r, false, &quiet);
        assert_eq!((s["state"].as_str(), s["watch"]["every"].as_u64(), s["watch"]["seen"].as_i64(), s["watch"]["next_due"].as_u64()), (Some("working"), Some(5), Some(201), Some(11 * MINUTE as u64)));
        remove_watch(&install, "w1").unwrap();
        assert_eq!(status(&install, &r, false, &quiet)["watch"], "no watch");
        fs::remove_dir_all(install).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    fn t_running() -> Turn { Turn { input: "owner".into(), state: "dispatching".into(), at: 1, ..Turn::default() } }
    /// D2/D3 Send: `send_op` without an op id; a refusal is its reason.
    fn send(install: &Path, id: &str, input: &str, native: &Native) -> Res<()> { send_op(install,id,input,None,native).map_err(|e|e.reason) }
