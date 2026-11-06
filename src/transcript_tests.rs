    use super::*;
    use std::{io::Write, path::PathBuf};

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-transcript-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }
    // A5: the saved AgentQ checkpoint removes only TaskQ's synthesis; native prefix verification remains mandatory.
    #[test]
    fn checkpoint_reuse_refuses_deep_rewrite_and_replacement() {
        let dir=tmp();let path=dir.join("native.jsonl");
        let row=json!({"type":"assistant","message":{"content":"invented preserved source"}}).to_string()+"\n";
        fs::write(&path,row.repeat(200)).unwrap();
        let tail=before("claude-native","-",&path,None).unwrap();
        after("claude-native","-",&path,tail.end).unwrap();
        after("claude-native","-",&path,tail.end).unwrap();
        assert_eq!(crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().get("checkpoint_hits").copied()),Some(1));
        let mut changed=fs::read(&path).unwrap();changed[row.len()*150+35]=b'X';fs::write(&path,&changed).unwrap();
        assert!(after("claude-native","-",&path,tail.end).unwrap_err().contains("replaced"),"deep prefix rewrite must be rejected even on a cache hit");
        let replacement=dir.join("replacement");fs::write(&replacement,row.repeat(200)).unwrap();fs::rename(&path,dir.join("old")).unwrap();fs::rename(replacement,&path).unwrap();
        assert!(after("claude-native","-",&path,tail.end).unwrap_err().contains("replaced"));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A1/A5 instrument: real local files and pinned AgentQ; no board/provider/session launch.
    #[test]
    #[ignore = "source cost checkpoint; run alone with --nocapture --test-threads=1"]
    fn conversations_source_cost_checkpoint() {
        #[cfg(windows)]
        fn io()->Option<[u64;2]> {
            #[repr(C)] #[derive(Default)] struct Counters {read:u64,write:u64,other:u64,read_bytes:u64,write_bytes:u64,other_bytes:u64}
            unsafe extern "system" {fn GetCurrentProcess()->isize;fn GetProcessIoCounters(process:isize,counters:*mut Counters)->i32;}
            let mut counters=Counters::default();
            unsafe { (GetProcessIoCounters(GetCurrentProcess(),&mut counters)!=0).then_some([counters.read,counters.read_bytes]) }
        }
        #[cfg(not(windows))] fn io()->Option<[u64;2]>{None}
        for count in [12,4096] {
            let dir=tmp();let session=crate::runtime::uuid();let sessions=dir.join("sessions/2026/11/05");fs::create_dir_all(&sessions).unwrap();
            let path=sessions.join(format!("rollout-invented-{session}.jsonl"));
            let row=json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"invented ".repeat(128)}]}}).to_string()+"\n";
            fs::write(&path,row.repeat(count)).unwrap();for n in 0..1000 {fs::write(sessions.join(format!("irrelevant-{n}")),"").unwrap();}
            std::thread::sleep(crate::runtime::owner::STAMP_SETTLE+std::time::Duration::from_millis(100));
            for baseline in [true,false] {
                crate::profile::SOURCE_BASELINE.set(baseline);
                let resolve=std::time::Instant::now();assert_eq!(crate::runtime::owner::codex_transcript_in(&dir,&session).unwrap(),path);
                let cold_path=crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().clone());let cold_resolve_us=resolve.elapsed().as_micros();
                let render=std::time::Instant::now();let tail=before("codex-rollout",&session,&path,None).unwrap();let cold_us=render.elapsed().as_micros();
                let cold_work=crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().clone());assert!(!tail.items.is_empty());
                // Prime the end checkpoint, then measure an empty poll without replacing either AgentQ verification.
                after("codex-rollout",&session,&path,tail.end).unwrap();
                let previous=io();let start=std::time::Instant::now();
                assert_eq!(crate::runtime::owner::codex_transcript_in(&dir,&session).unwrap(),path);
                let warm_path=crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().clone());
                let page=after("codex-rollout",&session,&path,tail.end).unwrap();assert!(page.items.is_empty());
                let elapsed=start.elapsed().as_micros();let transfer=previous.zip(io()).map(|(a,b)|[b[0]-a[0],b[1]-a[1]]);
                let warm_work=crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().clone());
                println!("SOURCE_COST {}",json!({"count":count,"baseline":baseline,"file_bytes":fs::metadata(&path).unwrap().len(),"cold_native_us":cold_us,"cold_work":cold_work,"cold_path_us":cold_resolve_us,"cold_path":cold_path,"warm_path":warm_path,"warm_native_and_path_us":elapsed,"warm_work":warm_work,"process_read_operations_and_bytes":transfer,"agentq_hash_bytes":"not instrumented; both native prefix verifications retained"}));
                if !baseline {assert_eq!(warm_work.get("checkpoint_hits"),Some(&1));assert!(!warm_work.contains_key("taskq_prefix_read_bytes"));assert_eq!(warm_path.get("path_cache_hits"),Some(&1));}
            }
            crate::profile::SOURCE_BASELINE.set(false);fs::remove_dir_all(dir).unwrap();
        }
    }
    // F16: actual model changes belong to their source blocks; missing context never borrows a current task model.
    #[test]
    fn historical_blocks_keep_recorded_models() {
        let source=[("invented-model-one","first"),("invented-model-two","second")].map(|(model,text)|json!({"type":"assistant","message":{"role":"assistant","model":model,"content":[{"type":"text","text":text}]}}).to_string()).join("\n")+"\n";
        let items=read(false,&source);
        assert_eq!(items.iter().filter(|i|i["role"]=="assistant").map(|i|i["model"].as_str().unwrap()).collect::<Vec<_>>(),["invented-model-one","invented-model-two"]);
        assert!(read(false,"{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"unattributed\"}}\n").iter().all(|i|i["model"].is_null()));
    }
    /// The view text of items, without their record identity.
    fn text(items: &[Value]) -> Vec<Value> { items.iter().map(|i| json!({"role": i["role"], "text": i["text"]})).collect() }
    fn key(i: &Value) -> (u64, u64) { (i["at"].as_u64().unwrap(), i["i"].as_u64().unwrap()) }
    /// The view items of `source` read from a file.
    fn read(codex: bool, source: &str) -> Vec<Value> { let d = tmp(); let p = d.join("r.jsonl"); fs::write(&p, source).unwrap(); let v = before(fm(codex), "-", &p, None).unwrap().items; fs::remove_dir_all(d).unwrap(); v }
    fn fm(codex: bool) -> &'static str { if codex { "codex-rollout" } else { "claude-native" } }
    /// AgentQ's page bound: at most 128 source records.
    fn bounded(p: &Page) -> bool { p.items.iter().map(|i| i["at"].as_u64()).collect::<std::collections::HashSet<_>>().len() <= 128 }

    // D4/#2159: recorded image bytes, not a text path, are the only preview authority.
    #[test]
    fn recorded_images_keep_source_and_bound_preview_without_fetching_paths() {
        let png="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";
        let v=json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Review the image"},{"type":"input_image","image_url":format!("data:image/png;base64,{png}")} ]}});
        let items=&read(true,&format!("{v}\n"));
        assert_eq!(items.len(),2,"recorded image must remain a separate inspectable item");
        assert_eq!(items[1]["role"],"image","a provider user-image wrapper is not proof of an owner/board author");assert_eq!(items[1]["native_role"],"user");
        assert_eq!(items[1]["image"]["width"],1);assert!(items[1]["image"]["src"].as_str().unwrap().starts_with("data:image/png;"));
        let v=json!({"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":png}}]},{"type":"image","source":{"type":"url","url":"file:///unrelated/private.png"}}]}});
        let rows=read(false,&format!("{v}\n"));assert_eq!(rows.len(),2);
        assert!(rows[0]["image"]["src"].is_string());assert!(rows[1]["image"]["reason"].is_string());assert!(rows[1]["image"]["src"].is_null());
        let dir=tmp();let path=dir.join("images.jsonl");
        let recorded=json!({"type":"user","message":{"content":[{"type":"image","source":{"type":"base64","data":png}}]}});
        fs::write(&path,format!("{recorded}\n").repeat(8)).unwrap();let p=before("claude-native","-",&path,None).unwrap();
        assert!(p.generation.starts_with("history-v1:"),"AgentQ item ordinals require a parser-generation fence");
        assert_eq!(p.items.iter().filter(|i|i["image"]["src"].is_string()).count(),4,"page has at most four inline image sources");
        assert!(p.items[7]["image"]["reason"].as_str().unwrap().contains("limit"));
        let mut malformed=recorded.clone();malformed["message"]["content"][0]["source"]["data"]=json!("broken");
        let rows=read(false,&format!("{malformed}\n"));assert!(rows[0]["image"]["reason"].is_string());
        fs::remove_dir_all(dir).unwrap();
    }

    // D-A3: a 10 MiB transcript opens in one chunk read, scrolls up to its first record and appends while open
    // #1929 A4: byte offsets alone must not merge replacement sources; appending keeps generation.
    #[test]
    fn conversation_1929_generation_survives_append_and_detects_replacement() {
        let dir=std::env::temp_dir().join(format!("taskq-generation-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();let p=dir.join("log");
        let line=b"{\"type\":\"ignored\"}\n";fs::write(&p,line).unwrap();let first=before("claude-native","-",&p,None).unwrap().generation;
        use std::io::Write;fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(line).unwrap();
        assert_eq!(before("claude-native","-",&p,None).unwrap().generation,first);
        let replacement=dir.join("new");fs::write(&replacement,line).unwrap();
        #[cfg(windows)] {
            use std::os::windows::fs::FileTimesExt;
            let created=fs::metadata(&p).unwrap().created().unwrap();
            fs::OpenOptions::new().write(true).open(&replacement).unwrap().set_times(fs::FileTimes::new().set_created(created)).unwrap();
            assert_eq!(fs::metadata(&replacement).unwrap().created().unwrap(),created);
        }
        fs::remove_file(&p).unwrap();fs::rename(replacement,&p).unwrap();
        assert_ne!(before("claude-native","-",&p,None).unwrap().generation,first);fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ten_mib_claude_transcript_pages_by_cursor() {
        let dir = tmp();
        let id = "0b4eaf42-5e87-4d57-9d31-c37c62bfbc2c";
        fs::create_dir(dir.join("projects")).unwrap();
        fs::create_dir(dir.join("projects/-p")).unwrap();
        let path = dir.join(format!("projects/-p/{id}.jsonl"));
        assert_eq!(crate::runtime::claude_transcript_in(&dir, id).unwrap_err(), "Claude transcript unavailable; completion unknown");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "{}", json!({"type":"user","message":{"role":"user","content":"first"}})).unwrap();
        let filler = "x".repeat(1000);
        let mut n = 1;
        while f.metadata().unwrap().len() < 10 * 1024 * 1024 {
            writeln!(f, "{}", json!({"type":"assistant","message":{"content":[{"type":"text","text":format!("{n} {filler}")},{"type":"thinking","thinking":"hidden"}]}})).unwrap();
            writeln!(f, "{}", json!({"type":"cost-state","n":n})).unwrap();
            n += 1;
        }
        writeln!(f, "{}", json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"cargo  test\n--locked"}}]}})).unwrap();
        write!(f, "{{\"type\":\"user\",\"mess").unwrap();
        drop(f);
        assert_eq!(crate::runtime::claude_transcript_in(&dir, id).unwrap(), path);
        let open = before("claude-native","-",&path, None).unwrap();
        assert!(open.start > 0 && bounded(&open), "one bounded page");
        assert_eq!(text(&open.items).last().unwrap(), &json!({"role":"tool","text":"Bash cargo test --locked"}));
        assert!(open.items.iter().all(|i| i["role"] != "user"), "unknown and partial records are skipped");
        let (mut cursor, mut pages, mut first) = (open.start, 1, None);
        while cursor > 0 {
            let page = before("claude-native","-",&path, Some(cursor)).unwrap();
            assert!(page.end == cursor && page.start < cursor);
            cursor = page.start;
            first = text(&page.items).first().cloned();
            pages += 1;
        }
        assert_eq!(first, Some(json!({"role":"user","text":"first"})));
        assert!(pages >= 40);
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "age\":{{\"content\":\"next\"}}}}\n").unwrap();
        let new = after("claude-native","-",&path, open.end).unwrap();
        assert_eq!(text(&new.items), vec![json!({"role":"user","text":"next"})]);
        assert_eq!(new.end, fs::metadata(&path).unwrap().len());
        assert_eq!(last_line("claude-native","-",&path).unwrap(), Some(format!("{} {filler}", n - 1)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_rollout_is_found_and_read() {
        let dir = tmp();
        let id = "01a1624d-88a8-76f0-a9df-9e47cd23b786";
        assert!(crate::runtime::owner::codex_transcript_in(&dir, id).unwrap_err().starts_with("Codex transcript unavailable"), "missing is unknown, never empty");
        fs::create_dir_all(dir.join("sessions/2026/10/22")).unwrap();
        let path = dir.join(format!("sessions/2026/10/22/rollout-2026-10-22T13-30-11-{id}.jsonl"));
        let lines = [
            json!({"type":"session_meta","payload":{"id":id}}),
            json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"rules"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done\nall good"}]}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
        ];
        fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        assert_eq!(crate::runtime::owner::codex_transcript_in(&dir, id).unwrap(), path);
        assert_eq!(text(&before("codex-rollout",id,&path, None).unwrap().items), vec![json!({"role":"user","text":"hello"}), json!({"role":"tool","text":"exec_command ls"}), json!({"role":"assistant","text":"done\nall good"}), json!({"role":"event","text":"turn complete"})]);
        assert_eq!(last_line("codex-rollout",id,&path).unwrap().as_deref(), Some("all good"));
        fs::remove_dir_all(dir).unwrap();
    }

    // #2176 native Codex evidence: a call and its output in different backward pages gave a result-less call one way
    // and the call with its body the other, so the view depended on page boundaries. Risk kept: every result is one
    // item at its own record, identical from either end; output text is masked like arguments.
    #[test]
    fn tool_results_are_page_independent() {
        let dir = tmp();
        let path = dir.join("r.jsonl");
        let mut src = String::new();
        for n in 0..150 {
            src += &format!("{}\n", json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":format!("c{n}"),"arguments":"{\"cmd\":\"ls\"}"}}));
            src += &format!("{}\n", json!({"type":"response_item","payload":{"type":"function_call_output","call_id":format!("c{n}"),"output":format!("out {n} GH_TOKEN=ghp_{}", "a".repeat(36))}}));
        }
        fs::write(&path, src).unwrap();
        let size = fs::metadata(&path).unwrap().len();
        let mut pages = vec![before("codex-rollout", "-", &path, None).unwrap()];
        while pages.last().unwrap().start > 0 { pages.push(before("codex-rollout", "-", &path, Some(pages.last().unwrap().start)).unwrap()); }
        assert!(pages.len() > 1, "the fixture splits pages");
        let backward: Vec<Value> = pages.iter().rev().flat_map(|p| p.items.clone()).collect();
        let (mut forward, mut from) = (vec![], 0);
        while from < size { let p = after("codex-rollout", "-", &path, from).unwrap(); forward.extend(p.items); from = p.end; }
        assert_eq!(forward, backward);
        let results: Vec<&Value> = backward.iter().filter(|i| i["result"].is_string()).collect();
        assert_eq!(results.len(), 150, "every result kept once");
        assert!(results.iter().all(|r| r["result"].as_str().unwrap().ends_with("GH_TOKEN=[redacted]")), "output masked");
        fs::remove_dir_all(dir).unwrap();
    }

    // #2159 D7: status previews prose, while metadata, partial text and exact source remain inspectable.
    #[test]
    fn last_assistant_preview_excludes_complete_citation_suffix_only() {
        let dir=tmp();let path=dir.join("preview.jsonl");
        for codex in [false,true] {
            for (text,expected) in [
                ("Reviewed result.\n<oai-mem-citation>\nMEMORY.md:1-2\n</oai-mem-citation>\n",Some("Reviewed result.")),
                ("Ответ принят.\n\n<oai-mem-citation>\nfirst\n</oai-mem-citation>\n<oai-mem-citation>\nsecond\n</oai-mem-citation>",Some("Ответ принят.")),
                ("<oai-mem-citation>\nmetadata only\n</oai-mem-citation>",None),
                ("Reviewed.\n<oai-mem-citation>\npartial metadata",Some("partial metadata")),
                ("Reviewed.\n</oai-mem-citation>",Some("</oai-mem-citation>")),
                ("Literal example:\n```xml\n<oai-mem-citation>\nexample\n</oai-mem-citation>\n```",Some("```")),
                ("Reviewed.\n<oai-mem-citation>\nmetadata\n</oai-mem-citation>\nNext action.",Some("Next action.")),
                ("",None),
            ] {
                let record=if codex {json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}})}
                    else {json!({"type":"assistant","message":{"content":[{"type":"text","text":text}]}})};
                let source=format!("{record}\n");fs::write(&path,&source).unwrap();
                assert_eq!(last_line(fm(codex),"-",&path).unwrap().as_deref(),expected,"codex={codex}, text={text}");
                assert_eq!(fs::read_to_string(&path).unwrap(),source,"preview never rewrites native history");
                if !text.is_empty(){assert_eq!(before(fm(codex),"-",&path,None).unwrap().items[0]["text"],text,"exact source remains complete");}
            }
            let text="<oai-mem-citation>\nmetadata only\n</oai-mem-citation>";
            let records=["Prior reviewed prose.",text].map(|text|if codex {json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}})}else {json!({"type":"assistant","message":{"content":[{"type":"text","text":text}]}})});
            let source=records.iter().map(|r|format!("{r}\n")).collect::<String>();fs::write(&path,&source).unwrap();
            assert_eq!(last_line(fm(codex),"-",&path).unwrap().as_deref(),Some("Prior reviewed prose."),"metadata-only records cannot hide prose in the same chunk");
            assert_eq!(fs::read_to_string(&path).unwrap(),source);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // #1825 A4: a token-shaped word or a credential assignment in a tool call never reaches the view
    #[test]
    fn tool_lines_redact_credentials() {
        let token = format!("ghp_{}", "a".repeat(36));
        let input = json!({"command": format!("GH_TOKEN={token} gh api -H \"Authorization: token {token}\" x; DB_PASSWORD='p' run; sk-short; KEY=v")});
        assert_eq!(tool("Bash", &input)["text"], "Bash GH_TOKEN=[redacted] gh api -H \"Authorization: token [redacted] x; DB_PASSWORD=[redacted] run; sk-short; KEY=v");
    }

    // #1833 A2/A3: bearer tokens, Authorization headers and credential-named JSON fields never reach the view;
    // other words and fields stay readable. Values are invented.
    #[test]
    fn tool_lines_mask_headers_bearer_and_json_fields() {
        let line = |cmd: &str| tool("Bash", &json!({"command": cmd}))["text"].as_str().unwrap().to_string();
        assert_eq!(line("curl -H 'Authorization: Bearer abc.def' u"), "Bash curl -H 'Authorization: Bearer [redacted] u");
        assert_eq!(line("curl -H \"authorization: s3cr3t\" -H 'Accept: json' u"), "Bash curl -H \"authorization: [redacted] -H 'Accept: json' u");
        assert_eq!(line("http u Bearer xyz next"), "Bash http u Bearer [redacted] next");
        assert_eq!(line("requests.get(u, headers={'Authorization': 'Basic dXNlcg=='})"), "Bash requests.get(u, headers={'Authorization': 'Basic [redacted]");
        assert_eq!(line(r#"curl -d '{"access_token":"t1","name":"n","Password": "p w","max_tokens":5,"client_secret":"s"}' u"#),
            r#"Bash curl -d '{"access_token":"[redacted]","name":"n","Password": "[redacted]","max_tokens":5,"client_secret":"[redacted]"}' u"#);
        assert_eq!(line(r#"curl -d "{\"api_key\":\"k\",\"q\":\"x\"}" u"#), r#"Bash curl -d "{\"api_key\":\"[redacted]\",\"q\":\"x\"}" u"#);
        assert_eq!(line(r#"x '{"Authorization":"Bearer z","a":"b \" c"}'"#), r#"Bash x '{"Authorization":"[redacted]","a":"b \" c"}'"#);
        // a tool input without a known command field is shown as JSON and masked the same way
        assert_eq!(tool("mcp", &json!({"token": "t2", "repo": "r"}))["text"], r#"mcp {"token":"[redacted]","repo":"r"}"#);
    }

    // #1833 A1/A3: a Codex custom tool call shows the commands its script runs, never the script; other tools show
    // their name and target. Records follow the native rollout shape; paths and values are invented.
    #[test]
    fn codex_custom_tool_calls_read_as_commands() {
        let dir = tmp();
        let path = dir.join("rollout.jsonl");
        let call = |name: &str, input: &str| json!({"type":"response_item","payload":{"type":"custom_tool_call","status":"completed","call_id":"c","name":name,"input":input}});
        let lines = [
            call("exec", "text(await tools.exec_command({cmd:\"echo two\",max_output_tokens:2000}));\n"),
            call("exec", "// @exec: {\"yield_time_ms\": 1000}\nconst r=await Promise.allSettled([\ntools.exec_command({cmd:'git log -1 --format=\\'%H\\'', workdir:w}),\ntools.exec_command({\"cmd\": `rg -n \"tools.exec_command(\" src`}),\ntools.write_stdin({session_id:7,chars:\"secret\\n\"})\n]); r.forEach((v,i)=>text({i,...v}));\n"),
            call("exec", "text(await tools.apply_patch(\"*** Begin Patch\\n*** Update File: src/a.rs\\n@@\\n-x\\n+y\\n*** Add File: docs/b.md\\n+z\\n*** End Patch\\n\"));"),
            call("exec", "const r=await tools.view_image({path:\"shots/a.png\"});image(r.image_url);"),
            call("exec", "text(await tools.exec_command({cmd:\"GH_TOKEN=ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa gh api -H 'Authorization: Bearer q' x\"}));"),
            call("apply_patch", "*** Begin Patch\n*** Delete File: old.txt\n*** End Patch\n"),
            call("exec", "let x = 1;"),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}"}}),
        ];
        fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        let tools: Vec<Value> = text(&before("codex-rollout","-",&path, None).unwrap().items).into_iter().map(|i| i["text"].clone()).collect();
        assert_eq!(tools, vec![
            json!("exec echo two"),
            json!("exec git log -1 --format='%H' · rg -n \"tools.exec_command(\" src · write_stdin"),
            json!("exec apply_patch src/a.rs docs/b.md"),
            json!("exec view_image shots/a.png"),
            json!("exec GH_TOKEN=[redacted] gh api -H 'Authorization: Bearer [redacted] x"),
            json!("apply_patch old.txt"),
            json!("exec let x = 1;"),
            json!("exec_command ls"),
        ]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// #1833 review: a quote that never closes (a regex literal, a cut script) read past the script and panicked.
    #[test]
    fn codex_scripts_with_open_quotes_and_comments_read_without_panic() {
        assert_eq!(codex_script("text(r.replace(/'/g, ''))"), None);
        assert_eq!(codex_script("text('abc"), None);
        assert_eq!(codex_script("text(await tools.exec_command({cmd:\"echo a"), Some("echo a".into()));
        assert_eq!(codex_script("/* tools.exec_command({cmd:\"x\"}) 'it's */ text(await tools.exec_command({cmd:\"y\"}))"), Some("y".into()));
        assert_eq!(codex_script("/* open"), None);
        assert_eq!(codex_script("/*/ tools.exec_command({cmd:\"x\"})"), None);
        // every cut of a real script reads: Codex records may end mid-literal
        let js = "/* c */ const r=await tools.exec_command({cmd:'a \\'b\\' \\u00e9',\"workdir\": `w`}); // x\ntools.view_image({path:\"p\"})";
        for (k, _) in js.char_indices() { codex_script(&js[..k]); }
    }

    /// Writes invented records in the native format and returns the view text they must produce, in file order.
    /// `ignored` bytes of records without view items come before every `every`th record.
    fn transcript(codex: bool, path: &Path, records: usize, long: usize, every: usize, ignored: usize) -> Vec<Value> {
        let mut f = std::io::BufWriter::new(fs::File::create(path).unwrap());
        let (mut seed, mut want) = (7u64, vec![]);
        let mut line = |v: Value, f: &mut std::io::BufWriter<fs::File>| writeln!(f, "{v}").unwrap();
        for n in 0..records {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // variable height: 1 to 40 lines, multibyte text, now and then one very long message
            let body = if n % 997 == 500 { "ü".repeat(long) } else { vec![format!("ä {n} ✓"); 1 + (seed >> 33) as usize % 40].join("\n") };
            let text = format!("{n} {body}");
            let role = if n % 2 == 0 { "user" } else { "assistant" };
            if n % every == every / 2 {
                let mut left = ignored as i64;
                while left > 0 {
                    let v = if codex { json!({"type":"event_msg","payload":{"type":"token_count","pad":"x".repeat(900)}}) } else { json!({"type":"cost-state","pad":"x".repeat(900)}) };
                    left -= v.to_string().len() as i64 + 1;
                    line(v, &mut f);
                }
            }
            match (codex, role) {
                (false, "user") => line(json!({"type":"user","message":{"role":"user","content":text}}), &mut f),
                // two blocks in one record: the block ordinal tells them apart
                (false, _) => {
                    line(json!({"type":"assistant","message":{"content":[{"type":"text","text":text},{"type":"thinking","thinking":"hidden"},{"type":"tool_use","name":"Bash","input":{"command":format!("echo {n}")}}]}}), &mut f);
                    want.push(json!({"role":role,"text":text}));
                    want.push(json!({"role":"tool","text":format!("Bash echo {n}")}));
                    continue;
                }
                (true, _) => line(json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":"input_text","text":text}]}}), &mut f),
            }
            want.push(json!({"role":role,"text":text}));
        }
        want
    }

    /// Backward pages from the file end: each page's view items in file order.
    fn back(codex: bool, path: &Path) -> Vec<Page> {
        let mut pages = vec![before(fm(codex),"-",path, None).unwrap()];
        while pages.last().unwrap().start > 0 { pages.push(before(fm(codex),"-",path, Some(pages.last().unwrap().start)).unwrap()); }
        pages
    }

    // #2110 A5 / Q-A5: 10,000 variable-height visible records of both native renderers (#2176: was 100,000; each AgentQ
    // page re-hashes the prefix, so the gate keeps the same shapes at a tenth of the pages). Defect: a bounded or out-of-order
    // read omits or repeats a record, splits one, or stops at a page of only ignored records. Risk kept: every record
    // is reached from either end with the same identity, at most 128 records per page.
    #[test]
    fn ten_thousand_records_traverse_both_ways_in_any_order() {
        let dir = tmp();
        for codex in [false, true] {
            let path = dir.join(format!("{codex}.jsonl"));
            let want = transcript(codex, &path, 10_000, 300_000, 1500, 600 * 1024);
            let pages = back(codex, &path);
            assert!(pages.iter().all(bounded), "bounded pages");
            assert!(pages.iter().any(|p| p.items.is_empty() && p.start > 0), "a page of ignored records still leads earlier");
            for w in pages.windows(2) { assert_eq!(w[1].end, w[0].start, "pages meet at record boundaries"); }
            let backward: Vec<Value> = pages.iter().rev().flat_map(|p| p.items.clone()).collect();
            assert_eq!(text(&backward), want, "codex {codex}: backward reaches the first record without omission or duplicate");
            let keys: Vec<_> = backward.iter().map(key).collect();
            assert!(keys.windows(2).all(|k| k[0] < k[1]), "identity is unique and in file order");
            // forward from the start, as after an eviction, gives the same records and identities
            let (mut forward, mut from, size) = (vec![], 0, fs::metadata(&path).unwrap().len());
            while from < size { let p = after(fm(codex),"-",&path, from).unwrap(); assert!(bounded(&p) && p.end > from && p.size == size); forward.extend(p.items); from = p.end; }
            assert_eq!(forward, backward, "codex {codex}: forward and backward reads agree");
            // the same cursors read in another order give the same pages: a reply has no hidden state
            let mut order: Vec<usize> = (0..pages.len()).collect();
            order.sort_by_key(|i| (i * 7919) % pages.len());
            for i in order { let p = before(fm(codex),"-",&path, Some(pages[i].end)).unwrap(); assert_eq!((p.start, &p.items), (pages[i].start, &pages[i].items)); }
            // An ignored prefix can exceed CHUNK: the saved anchor is the visible record offset, not the page boundary.
            for item in pages.iter().skip(1).filter_map(|p|p.items.first()) {let a=after(fm(codex),"-",&path,item["at"].as_u64().unwrap()).unwrap();assert_eq!(a.items.first(),Some(item),"a saved record cursor reloads its range");}
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // #1742 Q-A5: a large source (#2176: 28 MiB, was 108 MiB; each AgentQ page re-hashes the prefix) with long single messages still reaches its first record; a record over 16 MiB,
    // a shrunk file and a partial last line stay explicit. Risk kept: no silent truncation or empty view.
    #[test]
    fn large_source_and_oversized_record_stay_explicit() {
        let dir = tmp();
        let path = dir.join("stress.jsonl");
        let started = std::time::Instant::now();
        let want = transcript(false, &path, 2_000, 2 * 1024 * 1024, 200, 2 * 1024 * 1024);
        let size = fs::metadata(&path).unwrap().len();
        assert!(size >= 20 * 1024 * 1024, "{size}");
        let pages = back(false, &path);
        assert_eq!(text(&pages.iter().rev().flat_map(|p| p.items.clone()).collect::<Vec<_>>()), want);
        eprintln!("large source stress: {} MiB in {} backward pages, {:?} with writing", size >> 20, pages.len(), started.elapsed());
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "{{\"type\":\"user\",\"mess").unwrap();
        assert!(after("claude-native","-",&path, size).unwrap().items.is_empty(), "a partial last line waits for its newline");
        assert_eq!(after("claude-native","-",&path, size).unwrap().end, size);
        writeln!(f, "age\":{{\"content\":\"{}\"}}}}", "y".repeat(17 * 1024 * 1024)).unwrap();
        drop(f);
        assert_eq!(before("claude-native","-",&path, None).unwrap_err(), "transcript record exceeds 16 MiB; view unknown");
        assert_eq!(after("claude-native","-",&path, size).unwrap_err(), "transcript record exceeds 16 MiB; view unknown");
        assert_eq!(text(&before("claude-native","-",&path, Some(size)).unwrap().items), text(&pages[0].items), "earlier records stay readable");
        fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(size / 2).unwrap();
        assert_eq!(before("claude-native","-",&path, Some(size)).unwrap_err(), "transcript shrank; view unknown");
        assert_eq!(after("claude-native","-",&path, size).unwrap_err(), "transcript shrank; view unknown");
        fs::remove_dir_all(dir).unwrap();
    }
    // #1930 A3/A4/A7: sanitized actual Claude/Codex metadata, never invented times or cursor-to-date inference.
    #[test]
    fn conversation_1930_observed_native_times_survive_pages_and_gaps() {
        let fixture:Value=serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/conversation-time-provenance.json"))).unwrap();
        let dir=tmp();let path=dir.join("native.jsonl");
        for sample in fixture["records"].as_array().unwrap() {
            let codex=sample["provider"]=="codex";let record=&sample["record"];
            fs::write(&path,format!("{record}\n")).unwrap();
            let page=before(fm(codex),"-",&path,None).unwrap();assert!(!page.items.is_empty());
            for item in &page.items {assert_eq!(item["updated"],record["timestamp"]);assert_eq!(item["at"],0);}
            assert_eq!(after(fm(codex),"-",&path,0).unwrap().items,page.items,"both read directions preserve source time and byte identity");
            let mut missing=record.clone();missing.as_object_mut().unwrap().remove("timestamp");
            fs::write(&path,format!("{record}\n{missing}\n")).unwrap();
            let page=before(fm(codex),"-",&path,None).unwrap();assert!(page.items.last().unwrap()["updated"].is_null());
            assert!(last_update(fm(codex),"-",&path).unwrap().is_null(),"a source gap cannot borrow the earlier record time");
        }
        for event in fixture["board"].as_array().unwrap() {assert_eq!(crate::time::iso(event["created_at"].as_str().unwrap()),event["seconds"].as_i64());}
        fs::remove_dir_all(dir).unwrap();
    }

    // CS2/R12: native record time, not read time or trailing progress/partial writes, orders conversations.
    #[test]
    fn sidebar_activity_uses_the_newest_visible_native_record() {
        let dir=std::env::temp_dir().join(format!("taskq-activity-{}",crate::runtime::uuid()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("transcript.jsonl");
        for codex in [false,true] {
            let record=|text:&str,time:&str|if codex {json!({"type":"response_item","timestamp":time,"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}})}else{json!({"type":"assistant","timestamp":time,"message":{"content":[{"type":"text","text":text}]}})};
            let a=record("First","2026-10-29T01:00:00.123Z");let b=record("Second","2026-10-29T01:01:00.456Z");
            fs::write(&path,format!("{a}\n{b}\n{{\"type\":\"progress\",\"timestamp\":\"2026-10-29T03:00:00Z\"}}\npartial")).unwrap();
            assert_eq!(last_update(fm(codex),"-",&path).unwrap(),"2026-10-29T01:01:00.456Z");assert_eq!(last_update(fm(codex),"-",&path).unwrap(),"2026-10-29T01:01:00.456Z","reading again never changes activity");
            let mut missing=b.clone();missing.as_object_mut().unwrap().remove("timestamp");fs::write(&path,format!("{a}\n{missing}\n")).unwrap();assert!(last_update(fm(codex),"-",&path).unwrap().is_null(),"do not substitute an older known timestamp");
        }
        fs::remove_file(&path).unwrap();assert!(last_update("claude-native","-",&path).is_err());fs::remove_dir_all(dir).unwrap();
    }

    // #2176 A5: every runtime's page names its AgentQ status, reason, CLI version slot and native qualification.
    // Defect: a supported or version-less capability was hidden. Risk kept: the view can always say what it relies on.
    #[test]
    fn capability_names_status_reason_version_and_native_state_per_runtime() {
        for (rt, format) in [("claude","claude-native"),("codex","codex-rollout"),("antigravity","agy-stream")] {
            let c = capability(super::format(rt));
            assert_eq!(c["format"], format);
            assert!(c.as_object().unwrap().contains_key("version"), "{rt}: version key present, null when unknown");
            assert!(["supported","limited"].contains(&c["history"]["status"].as_str().unwrap()) && c["history"]["reason"].is_string(), "{rt}: {c}");
            assert!(c["native"]["status"].is_string(), "{rt}: native qualification named");
        }
        assert_eq!(capability("agy-stream")["history"]["status"], "limited");
        assert_eq!(capability("nope")["history"]["status"], "unsupported");
    }

    /// #2176 A5 native evidence instrument, not a gate: `TASKQ_NATIVE_SOURCES` lists `format=session=path` lines of
    /// retained native transcripts. Reads each fully backward and forward through the product reader, read-only, and
    /// prints counts only (no text, no paths): `cargo test --bin taskq native_transcripts -- --ignored --nocapture`.
    // A2: lifecycle events render as one named line per runtime; usage counters stay source-only
    #[test]
    fn lifecycle_events_render_as_lines() {
        let dir = tmp();
        for (format, rows, want) in [
            ("codex-rollout", vec![json!({"type":"event_msg","payload":{"type":"task_started"}}), json!({"type":"event_msg","payload":{"type":"token_count"}}), json!({"type":"event_msg","payload":{"type":"task_complete"}})], vec!["turn started", "turn complete"]),
            ("claude-native", vec![json!({"type":"system","subtype":"compact_boundary"}), json!({"type":"rate_limit_event"})], vec!["system compact boundary"]),
            ("agy-stream", vec![json!({"event":"init"}), json!({"event":"step_update","step_update":{"step_index":1,"step_type":"checkpoint","state":"DONE"}})], vec!["session started", "checkpoint"]),
        ] {
            let path = dir.join(format!("{format}.jsonl"));
            fs::write(&path, rows.iter().map(|r| format!("{r}\n")).collect::<String>()).unwrap();
            let page = before(format, "-", &path, None).unwrap();
            let got: Vec<_> = page.items.iter().filter(|i| i["role"] == "event").map(|i| i["text"].as_str().unwrap()).collect();
            assert_eq!(got, want, "{format}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore]
    fn native_transcripts_read_both_ways() {
        let list = std::env::var("TASKQ_NATIVE_SOURCES").expect("TASKQ_NATIVE_SOURCES");
        for (n, entry) in list.lines().filter(|l| !l.trim().is_empty()).enumerate() {
            let mut it = entry.splitn(3, '=');
            let (format, session, path) = (it.next().unwrap(), it.next().unwrap(), Path::new(it.next().unwrap()));
            let size = fs::metadata(path).unwrap().len();
            let mut pages = vec![before(format, session, path, Some(size)).unwrap()];
            while pages.last().unwrap().start > 0 { pages.push(before(format, session, path, Some(pages.last().unwrap().start)).unwrap()); }
            let backward: Vec<Value> = pages.iter().rev().flat_map(|p| p.items.clone()).collect();
            let keys: Vec<_> = backward.iter().map(key).collect();
            assert!(keys.windows(2).all(|k| k[0] < k[1]), "source {n}: unique ordered identities");
            let (mut forward, mut from) = (vec![], 0);
            while from < size { let p = after(format, session, path, from).unwrap(); assert!(p.end > from); forward.extend(p.items); from = p.end; }
            if let Some(d) = (0..forward.len().max(backward.len())).find(|&i| forward.get(i) != backward.get(i)) {
                let brief = |v: Option<&Value>| v.map(|i| { let mut i = i.clone(); i.as_object_mut().unwrap().remove("text"); let r = i.as_object_mut().unwrap().remove("result").map(|r| r.as_str().map_or(0, str::len)); i["result_len"] = json!(r); i["text_len"] = json!(i.get("text").and_then(Value::as_str).map(str::len)); i });
                panic!("source {n}: forward and backward differ at item {d} of {}/{}: forward {:?} backward {:?}", forward.len(), backward.len(), brief(forward.get(d)), brief(backward.get(d)));
            }
            let mut roles = std::collections::BTreeMap::<String, usize>::new();
            for i in &backward { *roles.entry(i["role"].as_str().unwrap_or("?").into()).or_default() += 1; }
            let results = backward.iter().filter(|i| i["result"].is_string()).count();
            let c = &pages[0].capability;
            println!("source {n}: {format} {} bytes, {} pages, {} items {roles:?}, {results} tool results; history {} ({}), version {}, native {}",
                size, pages.len(), backward.len(), c["history"]["status"], c["history"]["reason"], c["version"], c["native"]["status"]);
        }
    }
