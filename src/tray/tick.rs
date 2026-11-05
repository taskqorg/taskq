//! The periodic refresh, and the receipts the queues and pages send back.

use super::*;

impl TrayState {
    pub(super) fn tick(&mut self, flow: &mut ControlFlow) {
        // #1297: the 2 s refresh folds steady successful work into 15-minute aggregates.
        let _repeating = tracing::info_span!("tray.refresh", coreq_repeating = true).entered();
        // another version became current: exit, a service starts the current one (§ 4)
        // a removed install (a test's temp folder) has no current version: its tray exits too (M15)
        if !alone(&self.install)
            || crate::platform::tool(&self.install).current().as_deref()
                != Some(crate::platform::VERSION)
        {
            *flow = ControlFlow::Exit;
            return;
        }
        // M15: Quit ends the tray once the service has stopped; no heartbeat stays behind. Restart then frees the
        // host lock and starts a new tray, whose Open starts a new service generation.
        if let Some(phase) = self.quitting
            && crate::host::service_status(&self.install)["state"] != "running"
        {
            let _ = fs::remove_file(self.install.join("tray"));
            if phase == "Restarting" {
                drop(self.lock.take());
                if let Err(e) = relaunch(&self.install) {
                    eprintln!("taskq tray: Restart could not start TaskQ: {e}");
                }
            }
            *flow = ControlFlow::Exit;
            return;
        }
        if fs::remove_file(raise_request()).is_ok() {
            let _ = self.proxy.send_event(Ev::Raise);
        }
        let _ = fs::write(self.install.join("tray"), std::process::id().to_string());
        // a drag sends many moves: save the last one per tick
        if std::mem::take(&mut self.moved) {
            self.settings.save(&self.install);
        }
        let st = state(
            &self.install,
            &self.settings,
            self.dark,
            None,
            &self.adding,
            &mut self.sync,
        );
        self.seen_projects = st["projects"].clone();
        // #1819, #2352: while any window is visible and focused the service polls the board every 15 s (M8)
        let front = |w: &Window| w.is_visible() && w.is_focused();
        let visible = |o: &Option<(Window, WebView)>| o.as_ref().is_some_and(|(w, _)| front(w));
        if visible(&self.ui)
            || visible(&self.workspace)
            || visible(&self.settings_window)
            || visible(&self.views_window)
            || self.detached.values().any(|(w, _, _)| front(w))
        {
            crate::paths::touch(&self.install.join("menu-open"));
        }
        // #2365 A3/A5: faults retire a bundle, a clean start proves it, and each page moves when it holds nothing in flight.
        if let Some(store) = &mut self.ui_store {
            let mut pages: Vec<&mut WebCalls> = vec![&mut self.web];
            pages.extend(self.workspace_web.as_mut());
            pages.extend(self.settings_web.as_mut());
            pages.extend(self.views_web.as_mut());
            pages.extend(self.detached.values_mut().map(|(_, _, calls)| calls));
            for calls in &mut pages {
                let Some(reason) = calls.start_fault.take() else {
                    continue;
                };
                let bundle = calls.bundle.clone();
                if bundle != crate::ui_bundle::embedded().version {
                    eprintln!("taskq tray: interface bundle {bundle} failed to start ({reason})");
                    store.fail(&bundle, &format!("failed to start ({reason})"));
                    let _ = fs::remove_file(self.install.join("tray.fault"));
                    self.icon_sig.clear();
                }
            }
            let active = crate::ui_bundle::active();
            if store.on_trial()
                && pages.iter().any(|calls| {
                    calls.bundle == active.version
                        && !calls.failed
                        && calls.booted.is_some_and(|at| at.elapsed() >= START_PROOF)
                })
            {
                store.commit();
            }
            let now = Instant::now();
            // A page with a user on it decides for itself; the receive hook declines while an edit is open.
            let offer = |view: &WebView, calls: &mut WebCalls, surface: &str, eligible: bool| {
                if calls.bundle == active.version {
                    return;
                }
                if calls.failed {
                    let _ = self.proxy.send_event(Ev::AdoptPage(surface.into()));
                    return;
                }
                if !eligible
                    || !calls.idle()
                    || calls
                        .offered
                        .is_some_and(|at| now.saturating_duration_since(at) < ADOPT_RETRY)
                {
                    return;
                }
                calls.offered = Some(now);
                web_eval(
                    view,
                    "taskq.receive({bundleAdopt:true})",
                    "tray.web.receive",
                    &self.proxy,
                    calls,
                );
            };
            if let Some((window, view)) = &self.ui {
                offer(
                    view,
                    &mut self.web,
                    MENU_SURFACE,
                    !window.is_visible() || self.settings.pin.pinned,
                );
            }
            if let (Some((window, view)), Some(calls)) =
                (&self.settings_window, &mut self.settings_web)
            {
                offer(view, calls, SETTINGS_SURFACE, !window.is_visible());
            }
            if let (Some((window, view)), Some(calls)) = (&self.views_window, &mut self.views_web) {
                offer(view, calls, VIEWS_SURFACE, !window.is_visible());
            }
            if let (Some((_, view)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
                offer(view, calls, "main", true);
            }
            for (id, (_, view, calls)) in self.detached.iter_mut() {
                offer(view, calls, id, true);
            }
            if crate::ui_bundle::follows_channel() && !self.ui_checking && now >= self.next_ui_check
            {
                self.ui_checking = true;
                self.next_ui_check = now + UI_CHECK_EVERY;
                let (check, p) = (store.check(), self.proxy.clone());
                std::thread::spawn(move || {
                    let _ = p.send_event(Ev::BundleChecked(check.run(&crate::ui_bundle::fetch)));
                });
            }
        }
        if !self.web.failed {
            refresh_icon(
                &self.tray,
                &st,
                &self.settings,
                self.dark,
                &mut self.icon_sig,
                self.ui.as_ref().map(|(w, _)| w),
            );
        }
        notes(
            &self.install,
            &mut self.settings,
            &st,
            &mut self.asked,
            &mut self.bursts,
        );
        self.work_item.set_text(quit_label(&st, self.quitting));
        let mut comparable = st.clone();
        comparable
            .as_object_mut()
            .expect("tray snapshot object")
            .remove("now");
        let sig = comparable.to_string();
        if let (Some((_, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
            if sig != self.workspace_shown && !calls.failed && calls.idle() {
                self.workspace_shown = sig.clone();
                web_eval(
                    wv,
                    &format!("taskq.receive({})", workspace_snapshot(&st)),
                    TICK,
                    &self.proxy,
                    calls,
                );
            }
        }
        if let (Some((_, wv)), Some(calls)) = (&self.settings_window, &mut self.settings_web) {
            if sig != self.settings_shown && !calls.failed && calls.idle() {
                self.settings_shown = sig.clone();
                web_eval(
                    wv,
                    &format!("taskq.receive({st})"),
                    TICK,
                    &self.proxy,
                    calls,
                );
            }
        }
        if let (Some((_, wv)), Some(calls)) = (&self.views_window, &mut self.views_web) {
            if sig != self.views_shown && !calls.failed && calls.idle() {
                self.views_shown = sig.clone();
                web_eval(
                    wv,
                    &format!("taskq.receive({st})"),
                    TICK,
                    &self.proxy,
                    calls,
                );
            }
        }
        self.detached_shown
            .retain(|id, _| self.detached.contains_key(id));
        for (id, (_, wv, calls)) in self.detached.iter_mut() {
            if take_due(
                self.detached_shown.entry(id.clone()).or_default(),
                &sig,
                !calls.failed && calls.idle(),
            ) {
                web_eval(
                    wv,
                    &format!("taskq.receive({})", workspace_snapshot(&st)),
                    TICK,
                    &self.proxy,
                    calls,
                );
            }
        }
        if sig != self.shown && self.web.idle() {
            self.shown = sig;
            if let Some((_, wv)) = &self.ui
                && !self.web.failed
            {
                web_eval(
                    wv,
                    &format!("taskq.receive({st})"),
                    TICK,
                    &self.proxy,
                    &mut self.web,
                );
            }
        }
    }

    pub(super) fn answer(&mut self, receipt: Value) {
        let packet = if receipt["answerReceipt"].is_object() && self.sync.receipt(&receipt) {
            let snapshot = state(
                &self.install,
                &self.settings,
                self.dark,
                None,
                &self.adding,
                &mut self.sync,
            );
            self.seen_projects = snapshot["projects"].clone();
            let _ = self.proxy.send_event(Ev::Sync);
            json!({"updates":[snapshot,receipt]})
        } else {
            receipt.clone()
        };
        let cap = capmsg_refresh(
            &receipt,
            &mut self.shown,
            &mut self.workspace_shown,
            &mut self.settings_shown,
        );
        if cap {
            let _ = self.proxy.send_event(Ev::Sync);
        }
        push_state(&self.proxy, &mut self.web, &self.ui, &packet);
        if let (Some((_, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
            web_eval(
                wv,
                &format!("taskq.receive({receipt})"),
                "tray.web.receive",
                &self.proxy,
                calls,
            );
        }
        if cap
            && let (Some((_, wv)), Some(calls)) = (&self.settings_window, &mut self.settings_web)
            && !calls.failed
        {
            web_eval(
                wv,
                &format!("taskq.receive({receipt})"),
                "tray.web.receive",
                &self.proxy,
                calls,
            );
        }
        // #1819: a detached conversation sees the answer it may show too.
        if receipt["answerReceipt"].is_object() || receipt["reportReceipt"].is_object() {
            for (_, wv, calls) in self.detached.values_mut() {
                if !calls.failed {
                    web_eval(
                        wv,
                        &format!("taskq.receive({receipt})"),
                        "tray.web.receive",
                        &self.proxy,
                        calls,
                    );
                }
            }
        }
    }

    pub(super) fn evaluated(&mut self, generation: u64, id: u64, result: &'static str) {
        if self.web.generation != generation {
            return;
        }
        self.web.finish(id, result);
        if self.web.pending.is_empty()
            && let Some(next) = self.shot_wait.take()
        {
            let _ = self.proxy.send_event(next);
        }
    }

    pub(super) fn dialogue(&mut self, surface: String, mut receipt: Value) {
        let current = if surface == "main" {
            self.workspace_web.as_ref()
        } else {
            self.detached.get(&surface).map(|(_, _, c)| c)
        };
        if current.is_none_or(|c| receipt["readerGeneration"].as_u64() != Some(c.generation)) {
            return;
        }
        task_body_receipt(&self.sync, &mut receipt);
        let script = format!("taskq.receive({})", json!({"dialogueReceipt":receipt}));
        if surface == "main" {
            if let (Some((_, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
                web_eval(wv, &script, "tray.web.receive", &self.proxy, calls);
            }
        } else if let Some((_, wv, calls)) = self.detached.get_mut(&surface) {
            web_eval(wv, &script, "tray.web.receive", &self.proxy, calls);
        }
    }

    pub(super) fn conversation(&mut self, surface: String, receipt: Value) {
        let script = format!("taskq.receive({})", json!({"conversationReceipt":receipt}));
        if surface == "main" {
            if let (Some((_, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
                web_eval(wv, &script, "tray.web.receive", &self.proxy, calls);
            }
        } else if let Some((_, wv, calls)) = self.detached.get_mut(&surface) {
            web_eval(wv, &script, "tray.web.receive", &self.proxy, calls);
        }
        self.shown.clear();
        self.workspace_shown.clear();
        self.detached_shown.clear();
    }

    pub(super) fn added(&mut self, generation: u64, v: Value) {
        if matches!(
            self.adding["state"].as_str(),
            Some("picking dismissed" | "installing dismissed")
        ) && self.add_generation == generation.wrapping_add(1)
        {
            self.adding = Value::Null;
            push_state(
                &self.proxy,
                &mut self.web,
                &self.ui,
                &state(
                    &self.install,
                    &self.settings,
                    self.dark,
                    None,
                    &self.adding,
                    &mut self.sync,
                ),
            );
            return;
        }
        if !apply_added(&mut self.adding, self.add_generation, generation, v) {
            return;
        }
        push_state(
            &self.proxy,
            &mut self.web,
            &self.ui,
            &state(
                &self.install,
                &self.settings,
                self.dark,
                None,
                &self.adding,
                &mut self.sync,
            ),
        );
        if let (Some((_, wv)), Some(calls)) = (&self.workspace, &mut self.workspace_web) {
            web_eval(
                wv,
                &format!(
                    "taskq.receive({})",
                    workspace_snapshot(&state(
                        &self.install,
                        &self.settings,
                        self.dark,
                        None,
                        &self.adding,
                        &mut self.sync
                    ))
                ),
                "tray.web.receive",
                &self.proxy,
                calls,
            );
        }
        if !self.adding.is_null()
            && let Some((w, _)) = &self.workspace
        {
            w.set_visible(true);
            w.set_focus();
        }
    }
}
