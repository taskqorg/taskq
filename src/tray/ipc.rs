//! Messages from the menu page and the Settings and View windows.

use super::*;

/// Sends one reply to the menu page, and the shared parts of it to the workspace and Settings pages.
macro_rules! reply {
    ($s:expr,$value:expr) => {{
        let value=$value;
        push_state(&$s.proxy,&mut $s.web,&$s.ui,value);
        if value["projects"].is_array() || ["capmsg","focus_receipt","action_error","resources_receipt","aiError","agentqError","answerReceipt"].iter().any(|key|value.get(*key).is_some()) {
            if let (Some((_,wv)),Some(calls))=(&$s.workspace,&mut $s.workspace_web) {web_eval(wv,&format!("taskq.receive({value})"),"tray.web.receive",&$s.proxy,calls);}
            if let (Some((_,wv)),Some(calls))=(&$s.settings_window,&mut $s.settings_web) && !calls.failed {web_eval(wv,&format!("taskq.receive({value})"),"tray.web.receive",&$s.proxy,calls);}
        }
    }};
}

impl TrayState {
    // R22: Settings is one undecorated window over the tray page; window chrome stays here and every other
    // action takes the menu's path, so the Window page still configures the menu.
    pub(super) fn settings_message(&mut self, message:String) {
        let (Some((sw,swv)),Some(calls))=(&self.settings_window,&mut self.settings_web) else {return};
        let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
        if m["a"]=="web-error" {calls.errors(&m);return;}
        if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
        if calls.failed {return;}
        let chrome=|w:&Window|json!({"settingsWindow":{"top":w.is_always_on_top(),"maximized":w.is_maximized()}});
        match m["a"].as_str().unwrap_or("") {
            "bundle-reload" => {let _=self.proxy.send_event(Ev::AdoptPage(SETTINGS_SURFACE.into()));}
            "ready" => {
                calls.finish(0,"ok");
                web_eval(swv,&format!("taskq.receive({})",state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync)),"tray.web.receive",&self.proxy,calls);
                web_eval(swv,&format!("taskq.receive({})",chrome(sw)),"tray.web.receive",&self.proxy,calls);
                if !self.settings_open.is_null() {web_eval(swv,&format!("taskq.receive({})",json!({"settingsOpen":self.settings_open.take()})),"tray.web.receive",&self.proxy,calls);}
            }
            "hide" => sw.set_visible(false),
            "drag" => {let _=sw.drag_window();}
            "settings-top" => {
                let top=!sw.is_always_on_top();sw.set_always_on_top(top);
                if let Ok(mut r)=requests_file_checked(&self.install){r["settings_top"]=json!(top);let _=crate::platform::write_atomic(&self.install.join("tray-requests.json"),&r.to_string());}
                web_eval(swv,&format!("taskq.receive({})",chrome(sw)),"tray.web.receive",&self.proxy,calls);
            }
            "settings-enlarge" => {sw.set_maximized(!sw.is_maximized());web_eval(swv,&format!("taskq.receive({})",chrome(sw)),"tray.web.receive",&self.proxy,calls);}
            "resize" => {if let (Some(width),Some(height))=(m["w"].as_f64(),m["h"].as_f64()){if width.is_finite()&&height.is_finite(){let area=sw.current_monitor().map(|m|work_area(&m));let scale=sw.scale_factor();let max_w=area.map_or(2000.0,|(_,_,w,_)|f64::from(w)/scale).max(320.0);let max_h=area.map_or(1600.0,|(_,_,_,h)|f64::from(h)/scale).max(240.0);sw.set_inner_size(LogicalSize::new(width.clamp(320.0,max_w),height.clamp(240.0,max_h)));}}}
            // the menu's own geometry and navigation are not this window's
            "size" | "resize-end" | "overflow" | "pin" => {}
            "set" if m.get("navigation").is_some() => {}
            _ => {let _=self.proxy.send_event(Ev::Ipc(message));}
        }
    }

    // P-W4: the View window edits the menu's dashboard model; the menu stays its only writer to tray.json.
    pub(super) fn views_message(&mut self, message:String) {
        let (Some((vw,vwv)),Some(calls))=(&self.views_window,&mut self.views_web) else {return};
        let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
        if m["a"]=="web-error" {calls.errors(&m);return;}
        if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
        if calls.failed {return;}
        match m["a"].as_str().unwrap_or("") {
            "bundle-reload" => {let _=self.proxy.send_event(Ev::AdoptPage(VIEWS_SURFACE.into()));}
            "ready" => {
                calls.finish(0,"ok");
                let st=state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync);
                web_eval(vwv,&format!("taskq.receive({st})"),"tray.web.receive",&self.proxy,calls);
                if st["navigation"]["dashboard"].is_object() {web_eval(vwv,&format!("taskq.receive({})",json!({"dashboard":st["navigation"]["dashboard"],"initial":true})),"tray.web.receive",&self.proxy,calls);}
            }
            "hide" => vw.set_visible(false),
            "drag" => {let _=vw.drag_window();}
            "workspace" | "task-pin" => {let _=self.proxy.send_event(Ev::Ipc(message));}
            "views-dashboard" if m["dashboard"].is_object() && m["dashboard"].to_string().len()<=65536 => push_state(&self.proxy,&mut self.web,&self.ui,&json!({"dashboard":m["dashboard"]})),
            // ponytail: the View window owns no other action; add arms when it gains one
            _ => {}
        }
    }

    /// One menu message: envelope checks and its trace context, then the ordered commands.
    pub(super) fn ipc(&mut self, m:String, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        let Ok(m) = serde_json::from_str::<Value>(&m) else {self.web.fault("invalid-ipc");return;};
        if m["a"]=="bundle-reload" {let _=self.proxy.send_event(Ev::AdoptPage(MENU_SURFACE.into()));return;}
        if m["a"]=="ui-check" {self.next_ui_check=Instant::now();return;}
        if m["a"]=="update" {self.next_ui_check=Instant::now();}
        if self.ui.is_none() {return;}
        if m["a"]=="web-error" {self.web.errors(&m);return;}
        if m["a"]=="web-observation" {
            if self.web.metadata(&m["row"])&&let Some(tray)=&self.tray {
                let _=tray.set_tooltip(Some("TaskQ: activity history incomplete; check each command confirmation"));
            }
            return;
        }
        let context=match m["context"].as_str(){Some(text)=>match coreq::telemetry::capture::TraceContext::decode(text){Ok(context)=>Some(context),Err(_)=>{self.web.fault("invalid-web-context");return;}},None=>None};
        let _continuation=context.as_ref().map(|context|context.span().entered());
        if self.web.failed {return;}
        self.ipc_surfaces(&m,target);
    }

    /// Opens the other surfaces and answers snapshot reports.
    fn ipc_surfaces(&mut self, m:&Value, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        let Some((w, _)) = &self.ui else { return };
        match m["a"].as_str().unwrap_or("") {
            "workspace" if self.snapshot.is_none() => {
                self.workspace_open=m["open"].clone();
                if self.workspace_web.as_ref().is_some_and(|c|c.failed) {let _=self.proxy.send_event(Ev::RecoverWorkspace("main".into(),false));return;}
                // #1699: the header entry (it names a thread) opens the main workspace, never a detached conversation
                if self.workspace_open["page"]=="conversations" && self.workspace_open["thread"].is_null() && self.workspace_open["task"].is_null() {if let Some(id)=requests_file(&self.install)["conversation_navigation"]["selected"].as_str(){if let Some((window,_,_))=self.detached.get(id){window.set_visible(true);window.set_focus();return;}}}
                if self.workspace.is_none() {
                    let p=self.proxy.clone();let mut reader=WebCalls::new();reader.workspace=true;
                    let built=conversations_window(target,w,self.settings.conversation_geometry.as_ref(),requests_file(&self.install)["workspace_pin"]==true).and_then(|window| {
                        workspace_view(&window,p,"main".into(),&reader).map(|wv|(window,wv))
                    });
                    match built {
                        Ok(window)=>{self.workspace=Some(window);self.workspace_web=Some(reader);}
                        Err(error)=>{reply!(self,&json!({"workspaceError":format!("Workspace unavailable: {error}")}));return;}
                    }
                }
                if let (Some((_,wv)),Some(calls))=(&self.workspace,&mut self.workspace_web) {
                    if !calls.pending.contains_key(&0) && !self.workspace_open.is_null() && !self.workspace_open["page"].is_null(){web_eval(wv,&format!("taskq.receive({})",json!({"open":self.workspace_open.take()})),"tray.web.receive",&self.proxy,calls);}
                }
                self.workspace_handoff=w.is_visible() && !self.settings.pin.pinned;
                if let Some((window,_))=&self.workspace {window.set_visible(true);window.set_focus();}
                reply!(self,&json!({"workspaceError":Value::Null}));
            }
            "settings-open" if self.snapshot.is_none() => {
                self.settings_open=if m["page"].is_string() {json!({"page":m["page"]})} else {Value::Null};
                if self.settings_window.is_none() {
                    let p=self.proxy.clone();
                    let mut calls=WebCalls::new();calls.workspace=true;calls.surface=SETTINGS_SURFACE.into();
                    let built=WindowBuilder::new().with_title("TaskQ Settings").with_decorations(true).with_always_on_top(requests_file(&self.install)["settings_top"]==true).with_inner_size(LogicalSize::new(420.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                        surface_view(&window,p,&crate::ui_bundle::active().tray,"window.taskqSurface='settings';",PageSurface::Settings,&calls).map(|wv|(window,wv))
                    });
                    match built {
                        Ok(window)=>{self.settings_window=Some(window);self.settings_web=Some(calls);self.settings_shown.clear();}
                        Err(error)=>{reply!(self,&json!({"workspaceError":format!("Settings unavailable: {error}")}));return;}
                    }
                }
                if let (Some((_,wv)),Some(calls))=(&self.settings_window,&mut self.settings_web) && !calls.pending.contains_key(&0) && !self.settings_open.is_null() {web_eval(wv,&format!("taskq.receive({})",json!({"settingsOpen":self.settings_open.take()})),"tray.web.receive",&self.proxy,calls);}
                self.workspace_handoff=w.is_visible() && !self.settings.pin.pinned;
                if let Some((window,_))=&self.settings_window {window.set_visible(true);window.set_focus();}
                reply!(self,&json!({"workspaceError":Value::Null}));
            }
            "views-open" if self.snapshot.is_none() => {
                if self.views_window.is_none() {
                    let p=self.proxy.clone();
                    let mut calls=WebCalls::new();calls.workspace=true;calls.surface=VIEWS_SURFACE.into();
                    let built=WindowBuilder::new().with_title("TaskQ View").with_decorations(true).with_inner_size(LogicalSize::new(480.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                        surface_view(&window,p,&crate::ui_bundle::active().tray,"window.taskqSurface='views';",PageSurface::Views,&calls).map(|wv|(window,wv))
                    });
                    match built {
                        Ok(window)=>{self.views_window=Some(window);self.views_web=Some(calls);self.views_shown.clear();}
                        Err(error)=>{reply!(self,&json!({"workspaceError":format!("View window unavailable: {error}")}));return;}
                    }
                }
                self.workspace_handoff=w.is_visible() && !self.settings.pin.pinned;
                if let Some((window,_))=&self.views_window {window.set_visible(true);window.set_focus();}
                reply!(self,&json!({"workspaceError":Value::Null}));
            }
            "answer" if self.snapshot.is_none() && crate::snapshot::CONTROLS.iter().any(|c| m["op"]["action"]==c.0) => {
                if let Err(error)=send_answer(&self.queues.answer_actions,&mut self.sync,&answer_request(&m),&self.seen_projects,&self.proxy){reply!(self,&json!({"answerReceipt":{"root":m["root"],"id":m["op"]["id"],"state":"failed","reason":error,"next":"Nothing reached the board; send again"}}));}
            }
            "sync-retry" if self.snapshot.is_none() => {
                if let Some(request)=self.sync.retry(m["root"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or("")) {let _=send_answer(&self.queues.answer_actions,&mut self.sync,&request,&self.seen_projects,&self.proxy);}
            }
            "snapshot-checkpoint" if self.snapshot.is_some() && self.routes.iter().any(|r|r=="conversations-checkpoint") && m["state"]=="passed" => {self.checkpoint_complete=true;println!("PASS native Conversations checkpoint: {}",m["value"]);}
            "overflow" if self.snapshot.is_some() => self.overflow.push(format!("{} by {} px", m["shot"].as_str().unwrap_or("?"), m["x"])),
            "ready" => {
                let _=self.proxy.send_event(Ev::Sync);
                self.web.finish(0,"ok");
                if self.snapshot.is_some() {
                    let _ = self.proxy.send_event(Ev::Shot);
                    return;
                }
                // M3.26: restore navigation and native window controls before the first fit escapes.
                reply!(self,&json!({"updates":[state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync),{"settingsWindow":true,"viewsWindow":true}]}));
            }
            _ => self.ipc_layout(m,target),
        }
    }

    /// The menu window: size, position and pin.
    fn ipc_layout(&mut self, m:&Value, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        let Some((w, _)) = &self.ui else { return };
        match m["a"].as_str().unwrap_or("") {
            "size" | "resize" => {
                let Some((width, height)) = m["w"].as_f64().zip(m["h"].as_f64()).filter(|(w, h)| w.is_finite() && h.is_finite()) else { return };
                let Some(scale) = m["d"].as_f64().filter(|k| k.is_finite() && (0.5..=8.0).contains(k)) else { return };
                let (max_w, max_h) = w.current_monitor().or_else(|| w.primary_monitor()).map_or((1920.0, 800.0), |monitor| {
                    let area = work_area(&monitor);
                    (f64::from(area.2) / scale, f64::from(area.3) / scale)
                });
                let outer=w.outer_size();let inner=client_size(w);
                let frame=(outer.width.saturating_sub(inner.width),outer.height.saturating_sub(inner.height));
                let max_w=(max_w-f64::from(frame.0)/scale).max(240.0);let max_h=(max_h-f64::from(frame.1)/scale).max(120.0);
                let size = (width.max(240.0).min(max_w), height.max(120.0).min(max_h));
                let measurement = json!({"w": size.0, "h": if m["a"] == "resize" { size.1 } else { height.max(120.0) }, "d": scale, "g": m["g"]});
                let (width, height) = popover_size(&measurement, max_h * scale / w.scale_factor(), w.scale_factor());
                let old_size = w.outer_size();
                let old_position = w.outer_position().ok();
                w.set_inner_size(PhysicalSize::new(width.min((max_w * scale) as u32), height));
                if m["a"] == "size" && self.settings.pin.size_anchor != "fixed" {
                    if let (Some(p), Some(monitor)) = (old_position, w.current_monitor()) {
                        let p = anchored_position((p.x,p.y),old_size.height as i32,((width+frame.0) as i32,(height+frame.1) as i32),work_area(&monitor),if self.settings.pin.size_anchor=="auto" { &self.settings.pin.auto_anchor } else { &self.settings.pin.size_anchor });
                        w.set_outer_position(PhysicalPosition::new(p.0,p.1));
                    }
                }
                if m["a"] == "resize" {
                    self.settings.pin.size = Some(size);
                    self.moved = true;
                }
                // pinned: the window stays where it was put; a move then keeps it in the work area
                if m["a"] == "size" && self.settings.pin.size_anchor == "fixed" && let (Some(r), false) = (self.anchor, self.settings.pin.pinned) {
                    self.settings.pin.attached_edge = place(w, r, &mut self.settings.pin).into();
                }
            }
            "size-anchor" => {
                self.settings.pin.size_anchor = if let Some(value)=m["value"].as_str().filter(|v| ["auto","top","bottom","fixed"].contains(v)) {value.into()} else {return};
                if self.settings.pin.size_anchor == "fixed" { let size=client_size(w).to_logical::<f64>(w.scale_factor()); self.settings.pin.size=Some((size.width,size.height)); }
                self.settings.save(&self.install);
                reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));
            }
            "resize-end" => { self.settings.save(&self.install); }
            "hide" => w.set_visible(false),
            "top" => {
                self.settings.pin.top = !self.settings.pin.top;
                w.set_always_on_top(self.settings.pin.top);
                self.settings.save(&self.install);
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
            }
            "pin" | "drag" => {
                self.settings.pin.pinned = m["a"]=="drag" || !self.settings.pin.pinned;
                #[cfg(windows)]
                { use tao::platform::windows::WindowExtWindows; w.set_skip_taskbar(!self.settings.pin.pinned); }
                if let (true, Ok(p), Some(m)) = (self.settings.pin.pinned, w.outer_position(), w.current_monitor()) {
                    self.settings.pin.display = key(&m);
                    self.settings.pin.pos.insert(self.settings.pin.display.clone(), (p.x, p.y));
                }
                // unpin: back to the popover at the tray icon
                if let (false, Some(r)) = (self.settings.pin.pinned, self.anchor.or_else(|| self.tray.as_ref().and_then(TrayIcon::rect))) {
                    self.settings.pin.attached_edge = place(w, r, &mut self.settings.pin).into();
                }
                self.settings.save(&self.install);
                w.set_focus();
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
                if m["a"]=="drag" { let _=w.drag_window(); }
            }
            _ => self.ipc_reads(m,target),
        }
    }

    /// Queued reads, links and notification settings.
    fn ipc_reads(&mut self, m:&Value, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        match m["a"].as_str().unwrap_or("") {
            "completed" => {
                let page=if self.snapshot.is_some() {Some(json!({"tasks":[],"more":false,"reason":"snapshot: no board read"}))} else {self.queues.history_reads.submit(&m).err().map(|reason|json!({"tasks":[],"more":false,"reason":reason}))};
                if let Some(mut page)=page {page["root"]=m["root"].clone();page["id"]=m["id"].clone();reply!(self,&json!({"completed":page}));}
            }
            "reports" => {
                if let Err(reason)=self.queues.report_actions.submit(&m) {reply!(self,&json!({"reportReceipt":{"id":m["id"],"root":m["root"],"reason":reason}}));}
            }
            "resources" => {
                let id=format!("resources-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_nanos()));
                let request=json!({"a":"resources","root":self.install,"id":id});
                if let Err(error)=self.queues.resources_actions.submit(&request) {reply!(self,&json!({"resources_receipt":{"state":"rejected","reason":error}}));}
            }
            "open" => {
                if let Some(u) = m["url"].as_str().filter(|u| openable(u)) {
                    open(u);
                }
            }
            "notification-state" => {
                let previous=self.settings.notify.clone();
                let reason=if !notify::inbox_set(&mut self.settings.notify,&m) {Some("Notification update rejected: invalid event identities")}
                    else if !self.settings.save(&self.install) {self.settings.notify=previous;Some("Notification state was not saved; previous state retained")}
                    else {None};
                reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));
                reply!(self,&json!({"notificationReceipt":{"reason":reason}}));
            }
            "notify" => {
                if notify::set(&mut self.settings.notify, &m) {
                    self.settings.save(&self.install);
                    notes(&self.install, &mut self.settings, &Value::Null, &mut self.asked, &mut self.bursts);
                }
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
            }
            "notify-test" if self.snapshot.is_none() => {
                if let Some(k) = m["kind"].as_str().filter(|k| notify::KINDS.contains(k)) {
                    notify::post(&notify::sample(&self.settings.notify, k, now()));
                }
            }
            "task-pin" => {
                let st=state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync);
                let receipt=match task_pin(&mut self.settings,&self.install,&st,&m){Ok(dashboard)=>json!({"dashboard":dashboard}),Err(reason)=>json!({"viewStateError":reason})};
                reply!(self,&receipt);
            }
            "notify-os" => notify::open_settings(),
            _ => self.ipc_set(m,target),
        }
    }

    /// One saved preference; the state goes back to every surface.
    fn ipc_set(&mut self, m:&Value, target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        let Some((w, _)) = &self.ui else { return };
        match m["a"].as_str().unwrap_or("") {
            "set" => {
                if let Err(error)=appearance_input(&m) {
                    self.settings.opacity_error=Some(error);
                    reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));return;
                }
                let appearance=["opacity","opacity_mode","background_opacity"].iter().any(|key|m.get(*key).is_some());
                let previous=self.settings.clone();
                self.settings.set(&m);
                if appearance {
                    self.settings.opacity_error=match window_opacity(w,self.settings.native_opacity()) {
                        Ok(())=>None,
                        Err(error)=>{
                            self.settings=previous;
                            let rollback=window_opacity(w,self.settings.native_opacity()).err().map(|e|format!("; restore unknown: {e}")).unwrap_or_default();
                            Some(format!("Menu opacity was not applied: {error}{rollback}"))
                        }
                    };
                }
                let saved = (m.get("navigation").is_none() || self.settings.navigation == m["navigation"]) && self.settings.save(&self.install);
                if !saved && appearance {self.settings.opacity_error=Some("Opacity save failed; restart may restore the previous value".into());}
                if m.get("navigation").is_some() {
                    let readback=saved&&crate::data::json(&self.install.join("tray.json")).ok().flatten().is_some_and(|v|v["navigation"]==self.settings.navigation);
                    let error=if !readback {Some("tray.json: save/readback failed; view state persistence unknown")}else{None};
                    if error.is_none() {if let (Some((_,wv)),Some(calls))=(&self.views_window,&mut self.views_web) && !calls.failed && !calls.pending.contains_key(&0) && m["navigation"]["dashboard"].is_object() {web_eval(wv,&format!("taskq.receive({})",json!({"dashboard":m["navigation"]["dashboard"]})),"tray.web.receive",&self.proxy,calls);}}
                    push_state(&self.proxy,&mut self.web, &self.ui, &json!({"viewStateError":error}));
                    return;
                }
        let st = state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync);
                refresh_icon(&self.tray, &st, &self.settings, self.dark, &mut self.icon_sig, self.ui.as_ref().map(|(w,_)|w));
                reply!(self,&st);
            }
            _ => self.ipc_project(m,target),
        }
    }

    /// Project commands and Add project.
    fn ipc_project(&mut self, m:&Value, _target:&tao::event_loop::EventLoopWindowTarget<Ev>) {
        match m["a"].as_str().unwrap_or("") {
            // `Edit limits` after the page's confirm: the same write as `service capacity` (§ 5), this host only
            "capacity" | "restart" | "stop" | "uninstall" | "recovery-read" | "recovery-apply" => {
                let root = m["root"].as_str().unwrap_or("");
                if self.snapshot.is_some() && m["a"]=="recovery-read" {
                    let mut reply:Value=serde_json::from_str(include_str!("../../tests/fixtures/recovery-review.json")).expect("invented recovery fixture");
                    reply["root"]=m["root"].clone();reply["id"]=m["id"].clone();
                    reply!(self,&json!({"capmsg":reply}));
                } else if let Err(error)=self.queues.capacity_actions.submit(&m) {
                    reply!(self,&json!({"capmsg": {"root": root, "id": m["id"], "state":"rejected","reason":error}}));
                }
            }
            // Add project (§ 4): the folder comes only from the OS picker, never from the page
            "add" if self.adding.is_null() => {
                self.add_generation=self.add_generation.wrapping_add(1);
                let generation=self.add_generation;
                self.adding = json!({"state": "picking", "generation":generation});
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
                if let Err(error)=self.queues.add_actions.submit(generation,None) {self.adding=json!({"state":"add failed","reason":error});reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));}
            }
            "agentq-setup" | "agentq-dismiss" if self.snapshot.is_none() => {
                let error=crate::agentq::action(&self.install,m["a"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or(""),m["runtime"].as_str().unwrap_or("")).err();
                reply!(self,&json!({"agentqError":error}));
                reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));
            }
            "agentq-check" => {reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));}
            "add-cancel" if self.adding["state"]=="picking" => {
                self.add_generation=self.add_generation.wrapping_add(1);
                self.adding = json!({"state":"picking dismissed","generation":self.add_generation});
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
            }
            "add-cancel" if self.adding["state"]=="installing" => {
                self.add_generation=self.add_generation.wrapping_add(1);
                self.adding = json!({"state":"installing dismissed","generation":self.add_generation});
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
            }
            "add-cancel" => {self.adding=Value::Null;reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));}
            "add-install" if m["generation"].is_u64() && m["generation"]==self.adding["generation"] && self.adding["kind"] == "taskq" && self.adding["state"] == "picked" => {
                self.adding["state"] = json!("installing");
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
                let generation=self.adding["generation"].as_u64().unwrap_or(0);
                if let Err(error)=self.queues.add_actions.submit(generation,Some(self.adding.clone())) {self.adding["state"]=json!("install failed");self.adding["reason"]=json!(error);reply!(self,&state(&self.install,&self.settings,self.dark,None,&self.adding,&mut self.sync));}
            }
            "add-setup" if m["generation"].is_u64() && m["generation"]==self.adding["generation"] && matches!(self.adding["kind"].as_str(), Some("new" | "plain")) => {
                if let Err(reason) = crate::setup::submit(&self.install, crate::runtime::OWNER, None, &[self.adding["path"].as_str().unwrap_or_default().to_string()]) {reply!(self,&json!({"aiError":reason}));}
                self.adding = Value::Null;
                reply!(self,&state(&self.install, &self.settings, self.dark, None, &self.adding, &mut self.sync));
            }
            "add-install" | "add-setup" => reply!(self,&json!({"aiError":"Setup selection changed or was already submitted; read the current selection before confirming."})),
            a => {
                // only a registered project (§ 4): the page never names a path of its own
                let root = m["root"].as_str().unwrap_or("");
                if roots(&self.install).iter().any(|r| r == root) {
                    let result=if matches!(a,"pause"|"resume"|"focus") {
                        self.queues.focus_actions.submit(&m)
                    } else {act(a,Path::new(root),m["label"].as_str()).map(|_|())};
                    if let Err(error) = result {
                        let key=if matches!(a,"pause"|"resume"|"focus") {"focus_receipt"} else {"action_error"};
                        reply!(self,&json!({key:{"root":root,"id":m["id"],"text":error}}));
                    }
                } else if matches!(a,"pause"|"resume"|"focus") {
                    reply!(self,&json!({"focus_receipt":{"root":root,"id":m["id"],"text":"Project is no longer registered; command not submitted"}}));
                }
            }
        }
    }
}
