//! Messages from the Conversations workspace and its detached windows.

use super::*;

impl TrayState {
    /// Raises, detaches or returns one conversation window (#1699); every path ends the message.
    pub(super) fn conversation_window(&mut self, surface:&str, input:&Value, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        let id=input["conversation"].as_str().unwrap_or("");
        if !crate::resources::id(id){return;}
        let action=input["windowAction"].as_str().or_else(||input["action"].as_str()).unwrap_or("");
        if action=="raise" {if let Some((window,_,calls))=self.detached.get(id){if calls.failed {let _=self.proxy.send_event(Ev::RecoverWorkspace(id.into(),false));}else{window.set_visible(true);window.set_focus();}}return;}
        if action=="detach" && surface=="main" && !self.detached.contains_key(id) {
            let saved=crate::conversations::summary(&self.install);
            let valid=saved["items"].as_array().is_some_and(|rows|rows.iter().any(|c|c["id"]==id&&c["revision"]==input["revision"]));
            if !valid {return;}
            let p=self.proxy.clone();let source=id.to_string();let mut reader=WebCalls::new();reader.workspace=true;reader.surface=source.clone();
            let built=WindowBuilder::new().with_title("TaskQ Conversations").with_decorations(true).with_inner_size(LogicalSize::new(640.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window|{
                workspace_view(&window,p,source,&reader).map(|wv|(window,wv))
            });
            match built {
                Ok((window,wv))=>{window.set_visible(true);window.set_focus();self.detached.insert(id.into(),(window,wv,reader));}
                Err(_)=>{if let (Some((_,wv)),Some(calls))=(&self.workspace,&mut self.workspace_web){let r=json!({"conversationReceipt":{"id":input["id"],"state":"unknown","reason":"Detached window unavailable; main retains ownership"}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&self.proxy,calls);}return;}
            }
        }else if action=="return" && surface==id {self.detached.remove(id);self.workspace_checkpoints.remove(id);if let Some((window,_))=&self.workspace{window.set_visible(true);window.set_focus();}}
        else {return;}
        if let (Some((_,wv)),Some(calls))=(&self.workspace,&mut self.workspace_web){
            let owner=json!({"workspaceOwner":{"id":"main","detached":self.detached.keys().collect::<Vec<_>>()}});
            web_eval(wv,&format!("taskq.receive({owner})"),"tray.web.receive",&self.proxy,calls);
            if action=="return" {web_eval(wv,&format!("taskq.receive({});taskq.receive({})",workspace_snapshot(&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync)),json!({"open":{"page":"conversations"}})),"tray.web.receive",&self.proxy,calls);let select=json!({"workspaceOwner":{"id":"main","select":id,"detached":self.detached.keys().collect::<Vec<_>>()}});web_eval(wv,&format!("taskq.receive({select})"),"tray.web.receive",&self.proxy,calls);}
            else {let r=json!({"conversationReceipt":{"id":input["id"],"state":"accepted","snapshot":crate::conversations::summary(&self.install)}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&self.proxy,calls);}
        }
        return;
    }

    pub(super) fn workspace_message(&mut self, surface:String, message:String, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        let Ok(input)=serde_json::from_str::<Value>(&message) else {return};
        let current=if surface=="main" {self.workspace_web.as_ref()}else{self.detached.get(&surface).map(|(_,_,c)|c)};
        if current.is_none_or(|c|input["readerGeneration"].as_u64()!=Some(c.generation)){return;}
        if input["a"]=="workspace-origin" && surface=="main" && input["origin"]["surface"]=="settings" {
            // TW1: Settings keeps its DOM, route, scroll and focus while the shared review is open.
            if let Some((window,_))=&self.settings_window {window.set_visible(true);window.set_focus();if let Some((window,_))=&self.workspace {window.set_visible(false);}}
            return;
        }
        if input["a"]=="conversation-window" {self.conversation_window(&surface,&input,target);return;}
        let detached_ids:Vec<String>=self.detached.keys().cloned().collect();
        let (w,wv,calls)=if surface=="main" {let (Some((w,wv)),Some(calls))=(&self.workspace,&mut self.workspace_web) else{return};(w,wv,calls)}else{let Some((w,wv,calls))=self.detached.get_mut(&surface) else{return};(&*w,&*wv,calls)};
        let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
        if m["a"]=="web-error" {calls.errors(&m);return;}
        if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
        if m["a"]=="route-error" {calls.route(&m);return;}
        let context=match m["context"].as_str(){Some(text)=>match coreq::telemetry::capture::TraceContext::decode(text){Ok(context)=>Some(context),Err(_)=>{calls.fault("invalid-web-context");return;}},None=>None};
        let _continuation=context.as_ref().map(|context|context.span().entered());
        if m["a"]=="workspace-recovered" {if !calls.pending.contains_key(&0){calls.recovery_ready=true;}return;}
        if m["a"]=="workspace-checkpoint" {
            match workspace_checkpoint(calls,&mut self.workspace_checkpoints,&surface,&m["value"]) {
                Ok(true)=>{self.workspace_checkpoint_errors.remove(&surface);}
                Ok(false)=>{}
                Err(reason)=>{self.workspace_checkpoint_errors.insert(surface.clone());web_eval(wv,&format!("taskq.receive({})",json!({"workspaceLocalError":reason})),"tray.web.receive",&self.proxy,calls);}
            }
            return;
        }
        if m["a"]=="workspace-reload" {let _=self.proxy.send_event(if m["adopt"]==true {Ev::AdoptPage(surface.clone())} else {Ev::RecoverWorkspace(surface.clone(),m["safe"]==true)});return;}
        if m["a"]=="update"||m["a"]=="ui-check" {self.next_ui_check=Instant::now();}
        if m["a"]=="ui-check" {return;}
        if m["a"]=="hide" {w.set_visible(false);return;}
        if calls.failed {return;}
        match m["a"].as_str() {
            Some("reports")=>{let result=if m["local"].is_object(){answer_local(&self.install,m["local_key"].as_str().unwrap_or(""),&m["local"])}else{Ok(())}.and_then(|_|{let mut request=m.clone();request.as_object_mut().map(|v|{v.remove("local");v.remove("local_key");});self.queues.report_actions.submit(&request)});if let Err(reason)=result {web_eval(wv,&format!("taskq.receive({})",json!({"reportReceipt":{"id":m["id"],"root":m["root"],"reason":reason}})),"tray.web.receive",&self.proxy,calls);}}
            Some("sync-retry") => {
                if let Some(request)=self.sync.retry(m["root"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or("")) {let _=send_answer(&self.queues.answer_actions,&mut self.sync,&request,&self.seen_projects,&self.proxy);}
            }
            Some("ready") | Some("refresh") => {
                let _=self.proxy.send_event(Ev::Sync);
                calls.finish(0,"ok");
                let mut messages=vec![workspace_snapshot(&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync))];
                if m["a"]=="ready" && let Some(value)=self.workspace_checkpoints.get(&surface) {messages.push(json!({"workspaceRecovery":value}));}
                if surface=="main" && !self.workspace_open.is_null() && !self.workspace_open["page"].is_null(){messages.push(json!({"open":self.workspace_open.take()}));}
                let own=if surface=="main" {json!({"id":"main","detached":detached_ids})}else{json!({"id":surface,"conversation":surface,"detached":[]})};
                messages.push(json!({"workspacePin":w.is_always_on_top(),"workspaceMaximized":w.is_maximized()}));
                if surface!="main" && m["a"]=="ready" {messages.push(json!({"open":{"page":"conversations","thread":if calls.safe {json!("recovery")} else {Value::Null}}}));}
                messages.push(json!({"workspaceOwner":own}));
                if m["a"]=="ready" {messages.push(json!({"workspaceRecoveryReady":true}));}
                // One evaluation: a failed restore cannot acknowledge or replace its previous checkpoint.
                let script=messages.iter().map(|v|format!("taskq.receive({v})")).collect::<Vec<_>>().join(";");
                web_eval(wv,&script,"tray.web.receive",&self.proxy,calls);
            }
            Some("task-pin")=>{let st=state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync);let receipt=match task_pin(&mut self.settings,&self.install,&st,&m){Ok(dashboard)=>{push_state(&self.proxy,&mut self.web,&self.ui,&json!({"dashboard":dashboard}));json!({"dashboard":dashboard})},Err(reason)=>json!({"taskPinError":reason})};web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&self.proxy,calls);}
            Some("hide") => w.set_visible(false),
            Some("workspace-window")=>{
                match m["action"].as_str().unwrap_or("") {
                    "pin"=>{let pin=!w.is_always_on_top();w.set_always_on_top(pin);if surface=="main" {if let Ok(mut p)=requests_file_checked(&self.install){p["workspace_pin"]=json!(pin);let _=crate::platform::write_atomic(&self.install.join("tray-requests.json"),&p.to_string());}}web_eval(wv,&format!("taskq.receive({})",json!({"workspacePin":pin})),"tray.web.receive",&self.proxy,calls);}
                    "drag"=>{let _=w.drag_window();}
                    "enlarge"=>{let enlarged=!w.is_maximized();w.set_maximized(enlarged);web_eval(wv,&format!("taskq.receive({})",json!({"workspaceMaximized":enlarged})),"tray.web.receive",&self.proxy,calls);},
                    "size"=>{if let (Some(width),Some(height))=(m["width"].as_f64(),m["height"].as_f64()){if width.is_finite()&&height.is_finite(){let area=w.current_monitor().map(|m|work_area(&m));let scale=w.scale_factor();let max_w=area.map_or(2000.0,|(_,_,w,_)|f64::from(w)/scale).max(320.0);let max_h=area.map_or(1600.0,|(_,_,_,h)|f64::from(h)/scale).max(240.0);w.set_inner_size(LogicalSize::new(width.clamp(320.0,max_w),height.clamp(240.0,max_h)));}}}
                    _=>{}
                }
            }
            Some("conversation-navigation") if m["sidebar"].is_object()=>{let receipt=crate::conversations::sidebar(&self.install,&m);web_eval(wv,&format!("taskq.receive({})",json!({"conversationSidebarReceipt":receipt})),"tray.web.receive",&self.proxy,calls);}
            Some("conversation-navigation")=>{let error=if surface=="main" {crate::conversations::navigation(&self.install,&m["navigation"]).err()}else{None};web_eval(wv,&format!("taskq.receive({})",json!({"conversationNavigationError":error})),"tray.web.receive",&self.proxy,calls);}
            Some("conversation-action")=>{
                let id=m["conversation"].as_str().unwrap_or("");
                // CS5: local archive/restore may organize a detached chat; draft edits stay with its owner.
                let allowed=matches!(m["action"].as_str(),Some("archive"|"restore"))||if surface=="main" {!detached_ids.iter().any(|s|s==id)}else{surface==id};
                if allowed&&matches!(m["action"].as_str(),Some("reply"|"reply-check"|"watch"|"unwatch")) {
                    let mut request=m.clone();request["root"]=json!(surface);
                    if let Err(reason)=self.queues.conversation_actions.submit(&request){web_eval(wv,&format!("taskq.receive({})",json!({"conversationReceipt":{"id":m["id"],"state":"rejected","reason":reason}})),"tray.web.receive",&self.proxy,calls);}return;
                }
                let receipt=if allowed {crate::conversations::act(&self.install,&m,&roots(&self.install))}else{json!({"id":m["id"],"state":"unknown","reason":"Draft ownership moved; edit only in its owner window"})};
                web_eval(wv,&format!("taskq.receive({})",json!({"conversationReceipt":receipt})),"tray.web.receive",&self.proxy,calls);
                self.shown.clear();self.workspace_shown.clear();self.detached_shown.clear();
            }
            Some("conversation-dialogue")=>{
                if !w.is_visible() && m["conversation"].as_str().is_some_and(|id|id.starts_with("task:")) {
                    let receipt=json!({"id":m["id"],"conversation":m["conversation"],"action":m["action"],"state":"accepted","value":{"ended":true,"items":[]}});
                    web_eval(wv,&format!("taskq.receive({})",json!({"dialogueReceipt":receipt})),"tray.web.receive",&self.proxy,calls);return;
                }
                // Reads only, off the event loop: the D3 holder probe starts the runtime CLI.
                let (p,install,surface,m)=(self.proxy.clone(),self.install.clone(),surface.clone(),m.clone());
                std::thread::spawn(move||{let mut receipt=crate::conversations::dialogue(&install,&m);receipt["readerGeneration"]=m["readerGeneration"].clone();let _=coreq::telemetry::capture::operation("tray.dialogue.report",||p.send_event(Ev::Dialogue(surface,receipt)),|_|"ui-closed");});
            }
            Some("message-action") => {
                let ps:Vec<Value>=roots(&self.install).iter().map(|r|project(&self.install,r)).collect();
                let keys:Vec<String>=m["keys"].as_array().into_iter().flatten().filter_map(|v|v.as_str().map(String::from)).collect();
                let mut receipt=crate::messages::act(&self.install,&ps,m["action"].as_str().unwrap_or(""),&keys);receipt["id"]=m["id"].clone();
                web_eval(wv,&format!("taskq.receive({})",json!({"messageReceipt":receipt})),"tray.web.receive",&self.proxy,calls);
                web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync))),"tray.web.receive",&self.proxy,calls);
                push_state(&self.proxy,&mut self.web,&self.ui,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));
            }
            Some("workspace-set") => {
                let reason=save_workspace_navigation(&mut self.settings,&self.install,&m["navigation"]);
                web_eval(wv,&format!("taskq.receive({})",json!({"viewStateError":reason})),"tray.web.receive",&self.proxy,calls);
            }
            Some("answer-local") => {
                let result=answer_local(&self.install,m["key"].as_str().unwrap_or(""),&m["local"]);
                web_eval(wv,&format!("taskq.receive({})",json!({"workspaceLocalError":result.err()})),"tray.web.receive",&self.proxy,calls);
            }
            Some("reply-file") => {
                let result=crate::reply_files::store_file(&self.install,&m);
                let receipt=match result{Ok(file)=>json!({"id":m["id"],"state":"accepted","file":file}),Err(e)=>{tracing::info!(coreq_refusal=crate::reply_files::refusal_kind(&e));json!({"id":m["id"],"state":"rejected","kind":"storage","reason":e})}};
                web_eval(wv,&format!("taskq.receive({})",json!({"replyFileReceipt":receipt})),"tray.web.receive",&self.proxy,calls);
            }
            Some("answer") => {
                let key=format!("{}#{}",m["board"].as_str().unwrap_or(""),m["op"]["task"]);
                let result=if m["local"].is_object(){answer_local(&self.install,&key,&m["local"])}else{Ok(())}.and_then(|_|send_answer(&self.queues.answer_actions,&mut self.sync,&answer_request(&m),&self.seen_projects,&self.proxy));
                if let Err(error)=result {let r=json!({"answerReceipt":{"root":m["root"],"id":m["op"]["id"],"state":"failed","reason":error,"next":"Nothing reached the board; send again"}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&self.proxy,calls);}
            }
            Some("open") => {if let Some(url)=m["url"].as_str().filter(|u|openable(u)){open(url);}}
            Some(a) if matches!(a,"ai-send"|"ai-draft"|"ai-clear") => {
                let result=ai(&self.install,a,&m);
                let receipt=if a=="ai-send" {json!({"aiReceipt":{"id":m["id"],"state":if result.is_ok(){"accepted"}else{"unknown"},"reason":result.err().unwrap_or_else(||"Request saved; native launch is separate".into())}})}else{json!({"workspaceLocalError":result.err()})};
                web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&self.proxy,calls);
                if a=="ai-send" {web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync))),"tray.web.receive",&self.proxy,calls);}
            }
            // Workspace reviews use the same queues, native picker and receipt adapters as the tray.
            Some(a) if workspace_operation(a) => {if let Some(message)=workspace_forward(&m) {let _=self.proxy.send_event(Ev::Ipc(message));}}
            _ => calls.fault("unsupported-workspace-action"),
        }
    }
}
