//! The tray event loop: one state, one dispatch; each event's work lives in a sibling module.

use super::*;

impl TrayState {
    pub(super) fn new(
        lock: Option<fs::File>,
        install: PathBuf,
        snapshot: Option<PathBuf>,
        routes: Vec<String>,
        settings: Settings,
        proxy: tao::event_loop::EventLoopProxy<Ev>,
        queues: Queues,
        shots: [bool; 4],
    ) -> Self {
        let [opacity_shots, agentq_shots, views_shots, conversation_shots] = shots;
        let hidden_at = Instant::now() - Duration::from_secs(1);
        let ui_store = (snapshot.is_none() || crate::ui_bundle::snapshot_uses_store())
            .then(|| crate::ui_bundle::Store::open(&install));
        TrayState {
            lock,
            install,
            snapshot,
            routes,
            opacity_shots,
            agentq_shots,
            views_shots,
            conversation_shots,
            settings,
            proxy,
            queues,
            ui: None,
            workspace: None,
            workspace_web: None,
            detached: Default::default(),
            detached_shown: Default::default(),
            workspace_handoff: false,
            workspace_open: Value::Null,
            focused_workspace: String::from("main"),
            workspace_checkpoints: Default::default(),
            workspace_checkpoint_errors: Default::default(),
            settings_window: None,
            settings_web: None,
            settings_open: Value::Null,
            settings_shown: String::new(),
            views_window: None,
            views_web: None,
            views_shown: String::new(),
            tray: None,
            shown: String::new(),
            icon_sig: String::new(),
            dark: true,
            workspace_shown: String::new(),
            geometry_dirty: false,
            hidden_at,
            shown_at: hidden_at,
            anchor: None,
            step: 0,
            overflow: Vec::new(),
            capture_errors: Vec::new(),
            checkpoint_complete: false,
            checkpoint_deadline: None,
            adding: Value::Null,
            add_generation: 0,
            moved: false,
            asked: false,
            bursts: notify::Bursts::new(),
            shot_wait: None,
            ui_store,
            next_ui_check: Instant::now(),
            ui_checking: false,
            web: WebCalls::new(),
            sync: Default::default(),
            seen_projects: Value::Null,
            work_item: MenuItem::new("No active work", false, None),
            quitting: None,
        }
    }

    /// One native or user event; the arms keep their order, because the guards fall through.
    pub(super) fn handle(
        &mut self,
        event: Event<'_, Ev>,
        target: &tao::event_loop::EventLoopWindowTarget<Ev>,
        flow: &mut ControlFlow,
    ) {
        *flow = ControlFlow::Wait;
        self.before_event(&event);
        match event {
            Event::UserEvent(Ev::WorkspaceClose) => {
                if self.focused_workspace == "main" {
                    if let Some((w, _)) = &self.workspace {
                        w.set_visible(false);
                    }
                } else if let Some((w, _, _)) = self.detached.get(&self.focused_workspace) {
                    w.set_visible(false);
                }
            }
            Event::UserEvent(Ev::Page(surface, generation, body)) => {
                let current = match surface {
                    PageSurface::Menu => Some(&self.web),
                    PageSurface::Settings => self.settings_web.as_ref(),
                    PageSurface::Views => self.views_web.as_ref(),
                };
                if current.is_none_or(|calls| calls.generation != generation) {
                    return;
                }
                let _ = self.proxy.send_event(match surface {
                    PageSurface::Menu => Ev::Ipc(body),
                    PageSurface::Settings => Ev::Settings(body),
                    PageSurface::Views => Ev::Views(body),
                });
            }
            Event::UserEvent(Ev::BundleChecked(checked)) => {
                if let Some(store) = &mut self.ui_store {
                    store.apply(checked);
                }
                self.ui_checking = false;
                self.shown.clear();
                self.workspace_shown.clear();
                self.settings_shown.clear();
                self.views_shown.clear();
                self.detached_shown.clear();
            }
            // Same recreation as Reload; the checkpoint and the ready handoff carry the reader across.
            Event::UserEvent(Ev::AdoptPage(surface)) => self.adopt_page(surface),
            Event::UserEvent(Ev::RecoverWorkspace(surface, safe)) => {
                self.recover_workspace(surface, safe)
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } if self.detached.values().any(|(w, _, _)| w.id() == window_id) => {
                if let Some((w, _, _)) =
                    self.detached.values().find(|(w, _, _)| w.id() == window_id)
                {
                    w.set_visible(false);
                }
            }
            Event::WindowEvent { window_id, .. }
                if self.detached.values().any(|(w, _, _)| w.id() == window_id) => {}
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } if self
                .workspace
                .as_ref()
                .is_some_and(|(w, _)| w.id() == window_id) =>
            {
                if let (Some((w, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
                    w.set_visible(false);
                    web_eval(
                        wv,
                        "taskq.receive({workerPanelClosed:true})",
                        "tray.web.receive",
                        &self.proxy,
                        calls,
                    );
                }
            }
            Event::WindowEvent { window_id, .. }
                if self
                    .workspace
                    .as_ref()
                    .is_some_and(|(w, _)| w.id() == window_id) => {}
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } if self
                .settings_window
                .as_ref()
                .is_some_and(|(w, _)| w.id() == window_id) =>
            {
                if let Some((w, _)) = &self.settings_window {
                    w.set_visible(false);
                }
            }
            Event::WindowEvent { window_id, .. }
                if self
                    .settings_window
                    .as_ref()
                    .is_some_and(|(w, _)| w.id() == window_id) => {}
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } if self
                .views_window
                .as_ref()
                .is_some_and(|(w, _)| w.id() == window_id) =>
            {
                if let Some((w, _)) = &self.views_window {
                    w.set_visible(false);
                }
            }
            Event::WindowEvent { window_id, .. }
                if self
                    .views_window
                    .as_ref()
                    .is_some_and(|(w, _)| w.id() == window_id) => {}
            Event::NewEvents(StartCause::Init) => self.init(target, flow),
            Event::WindowEvent {
                window_id,
                event: WindowEvent::Moved(p),
                ..
            } if self.snapshot.is_none()
                && self.ui.as_ref().is_some_and(|(w, _)| {
                    w.id() == window_id && (self.settings.pin.pinned || native_header_drag(w))
                }) =>
            {
                self.menu_moved(p)
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some((w, _)) = &self.ui {
                    w.set_visible(false);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(false),
                ..
            } if self.snapshot.is_none() && !self.settings.pin.pinned => self.menu_blurred(),
            Event::WindowEvent {
                event: WindowEvent::ThemeChanged(_),
                ..
            } => self.theme_changed(),
            Event::UserEvent(Ev::Tick) if self.snapshot.is_none() => self.tick(flow),
            Event::UserEvent(ev @ (Ev::Quit | Ev::Restart))
                if self.snapshot.is_none() && self.quitting.is_none() =>
            {
                self.quit(ev)
            }
            Event::UserEvent(Ev::QuitFailed(e)) => self.quit_failed(e),
            Event::UserEvent(Ev::Raise) | Event::Reopen { .. } if self.snapshot.is_none() => {
                self.raise_menu()
            }
            Event::UserEvent(Ev::Click(rect)) => self.click(rect),
            Event::UserEvent(Ev::Answer(receipt)) => self.answer(receipt),
            // #1918: a store change rebuilds the one snapshot at once: every window, the icon and notifications read it
            Event::UserEvent(Ev::Sync) => {
                let _ = self.proxy.send_event(Ev::Tick);
            }
            // a notification click opens the menu (or raises the pinned window) on that card (M14)
            Event::UserEvent(Ev::Note(id)) => self.note(id),
            Event::UserEvent(Ev::Evaluated(generation, id, result)) => {
                self.evaluated(generation, id, result)
            }
            Event::UserEvent(Ev::Dialogue(surface, receipt)) => self.dialogue(surface, receipt),
            Event::UserEvent(Ev::Conversation(surface, receipt)) => {
                self.conversation(surface, receipt)
            }
            Event::UserEvent(Ev::WorkspaceEvaluated(generation, surface, id, result)) => {
                if surface == "main" {
                    if let Some(calls) = &mut self.workspace_web {
                        if calls.generation == generation {
                            calls.finish(id, result);
                        }
                    }
                } else if surface == SETTINGS_SURFACE {
                    if let Some(calls) = &mut self.settings_web {
                        if calls.generation == generation {
                            calls.finish(id, result);
                        }
                    }
                } else if surface == VIEWS_SURFACE {
                    if let Some(calls) = &mut self.views_web {
                        if calls.generation == generation {
                            calls.finish(id, result);
                        }
                    }
                } else if let Some((_, _, calls)) = self.detached.get_mut(&surface) {
                    if calls.generation == generation {
                        calls.finish(id, result);
                    }
                }
            }
            Event::UserEvent(Ev::Workspace(surface, message)) => {
                self.workspace_message(surface, message, target)
            }
            // R22: Settings is one undecorated window over the tray page; window chrome stays here and every other
            // action takes the menu's path, so the Window page still configures the menu.
            Event::UserEvent(Ev::Settings(message)) => self.settings_message(message),
            // P-W4: the View window edits the menu's dashboard model; the menu stays its only writer to tray.json.
            Event::UserEvent(Ev::Views(message)) => self.views_message(message),
            Event::UserEvent(Ev::Ipc(m)) => self.ipc(m, target),
            Event::UserEvent(Ev::Added(generation, v)) => self.added(generation, v),
            Event::UserEvent(Ev::Shot | Ev::Shot2(_)) if self.web.failed => {}
            // snapshot mode: per step push a state, wait for layout, capture, next
            Event::UserEvent(Ev::Shot) => self.shot(flow),
            Event::UserEvent(Ev::Shot2(step)) if self.step == step => self.shot2(),
            _ => {}
        }
    }

    /// The work every event shares: telemetry folding, focus, geometry and interface fault notices.
    fn before_event(&mut self, event: &Event<'_, Ev>) {
        crate::telemetry::diagnostic::poll(&self.install);
        if matches!(event, Event::UserEvent(Ev::Tick)) {
            TICK_QUEUED.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        let native = match event {
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => Some("tray.window.resize"),
            Event::WindowEvent {
                event: WindowEvent::Moved(_),
                ..
            } => Some("tray.window.move"),
            Event::WindowEvent {
                event: WindowEvent::Focused(_),
                ..
            } => Some("tray.window.focus"),
            Event::WindowEvent {
                event: WindowEvent::ThemeChanged(_),
                ..
            } => Some("tray.window.theme"),
            Event::WindowEvent {
                event: WindowEvent::Destroyed,
                ..
            } => Some("tray.window.destroy"),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => Some("tray.window.close"),
            Event::RedrawRequested(_) => Some("tray.window.redraw-request"),
            _ => None,
        };
        // #1548: a fit after each tick render resizes the window; unchanged native events fold per window.
        if let Some(name) = native {
            let _ = coreq::telemetry::capture::poll(name, 0, || Ok::<_, &str>(()), |_| "observed");
        }

        if let Event::WindowEvent {
            window_id,
            event: WindowEvent::Focused(true),
            ..
        } = event
        {
            if self
                .workspace
                .as_ref()
                .is_some_and(|(w, _)| w.id() == *window_id)
            {
                self.focused_workspace = "main".into();
            } else if let Some((id, _)) = self
                .detached
                .iter()
                .find(|(_, (w, _, _))| w.id() == *window_id)
            {
                self.focused_workspace = id.clone();
            }
        }
        if self.snapshot.is_none() {
            if let Event::WindowEvent {
                window_id,
                event: WindowEvent::Moved(_) | WindowEvent::Resized(_),
                ..
            } = event
            {
                if let Some((window, _)) = &self.workspace
                    && window.id() == *window_id
                    && let Some(value) = ConversationGeometry::capture(window)
                {
                    if self.settings.conversation_geometry.as_ref() != Some(&value) {
                        self.settings.conversation_geometry = Some(value);
                        self.geometry_dirty = true;
                    }
                }
            }
            if self.geometry_dirty
                && matches!(
                    event,
                    Event::UserEvent(Ev::Tick | Ev::WorkspaceClose)
                        | Event::WindowEvent {
                            event: WindowEvent::CloseRequested,
                            ..
                        }
                )
            {
                self.geometry_dirty = false;
                if !self.settings.save(&self.install)
                    || Settings::load(&self.install).conversation_geometry
                        != self.settings.conversation_geometry
                {
                    if let (Some((_, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web)
                    {
                        web_eval(
                            wv,
                            "taskq.receive({workspaceError:'Window geometry save failed; previous saved position may be retained'})",
                            "tray.web.notice",
                            &self.proxy,
                            calls,
                        );
                    }
                }
            }
        }
        self.web.expire(Instant::now());
        if let Some(calls) = &mut self.workspace_web {
            calls.expire(Instant::now());
            if calls.failed && !calls.notified {
                calls.notified = true;
                if let Some((_, wv)) = &self.workspace {
                    web_eval(
                        wv,
                        "window.taskqWebFault('native')",
                        "tray.web.notice",
                        &self.proxy,
                        calls,
                    );
                }
            }
        }
        if !self.web.pending.contains_key(&0)
            && let Some((_, wv)) = &self.ui
            && let Some(selection) = crate::telemetry::diagnostic::take_web_selection()
        {
            web_eval(
                wv,
                &format!("taskq.receive({})", json!({"diagnose":selection})),
                "tray.web.diagnose",
                &self.proxy,
                &mut self.web,
            );
        }
        if self.web.failed && !self.web.notified {
            self.web.notified = true;
            if let Some(t) = &self.tray {
                let _ = t.set_tooltip(Some(
                    "TaskQ: interface outcomes unknown; check results before retry",
                ));
            }
            if self.snapshot.is_none()
                && let Some((_, wv)) = &self.ui
            {
                web_eval(
                    wv,
                    "window.taskqWebFault('native')",
                    "tray.web.notice",
                    &self.proxy,
                    &mut self.web,
                );
            }
        }
        if self.web.failed && self.snapshot.is_some() && self.web.pending.is_empty() {
            crate::platform::exit(1);
        }
    }
}
