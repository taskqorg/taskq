//! The menu window: start, position, focus, Quit and interface recovery.

use super::*;

impl TrayState {
    // Same recreation as Reload; the checkpoint and the ready handoff carry the reader across.
    pub(super) fn adopt_page(&mut self, surface: String) {
        if self.workspace_checkpoint_errors.contains(&surface) {
            return;
        }
        let bundle = crate::ui_bundle::active();
        let (mut failure, mut retired) = (None, None);
        match surface.as_str() {
            MENU_SURFACE => {
                if let Some((window, view)) = &mut self.ui {
                    self.web.background = true;
                    self.shown.clear();
                    if let Err(error) =
                        recreate_workspace(window, view, &mut self.web, false, |window, reader| {
                            surface_view(
                                window,
                                self.proxy.clone(),
                                &bundle.tray,
                                "",
                                PageSurface::Menu,
                                reader,
                            )
                        })
                    {
                        failure = Some(error);
                        retired = Some(&mut self.web);
                    }
                }
            }
            SETTINGS_SURFACE => {
                if let (Some((window, view)), Some(calls)) =
                    (&mut self.settings_window, &mut self.settings_web)
                {
                    calls.background = true;
                    self.settings_shown.clear();
                    if let Err(error) =
                        recreate_workspace(window, view, calls, false, |window, reader| {
                            surface_view(
                                window,
                                self.proxy.clone(),
                                &bundle.tray,
                                "window.taskqSurface='settings';",
                                PageSurface::Settings,
                                reader,
                            )
                        })
                    {
                        failure = Some(error);
                        retired = Some(calls);
                    }
                }
            }
            VIEWS_SURFACE => {
                if let (Some((window, view)), Some(calls)) =
                    (&mut self.views_window, &mut self.views_web)
                {
                    calls.background = true;
                    self.views_shown.clear();
                    if let Err(error) =
                        recreate_workspace(window, view, calls, false, |window, reader| {
                            surface_view(
                                window,
                                self.proxy.clone(),
                                &bundle.tray,
                                "window.taskqSurface='views';",
                                PageSurface::Views,
                                reader,
                            )
                        })
                    {
                        failure = Some(error);
                        retired = Some(calls);
                    }
                }
            }
            "main" => {
                if let (Some((window, view)), Some(calls)) =
                    (&mut self.workspace, &mut self.workspace_web)
                {
                    calls.background = true;
                    self.workspace_shown.clear();
                    let safe = calls.safe;
                    if let Err(error) =
                        recreate_workspace(window, view, calls, safe, |window, reader| {
                            workspace_view(window, self.proxy.clone(), "main".into(), reader)
                        })
                    {
                        failure = Some(error);
                        retired = Some(calls);
                    }
                }
            }
            id => {
                if let Some((window, view, calls)) = self.detached.get_mut(id) {
                    calls.background = true;
                    self.detached_shown.remove(id);
                    let safe = calls.safe;
                    if let Err(error) =
                        recreate_workspace(window, view, calls, safe, |window, reader| {
                            workspace_view(window, self.proxy.clone(), surface.clone(), reader)
                        })
                    {
                        failure = Some(error);
                        retired = Some(calls);
                    }
                }
            }
        }
        // The old page stays; it is not asked again until a newer bundle, and Reload remains its way out.
        if let (Some(error), Some(calls)) = (failure, retired) {
            calls.log(format_args!("interface bundle not adopted: {error}"));
            calls.bundle = bundle.version.clone();
        }
    }

    pub(super) fn recover_workspace(&mut self, surface: String, safe: bool) {
        let surface = if surface.is_empty() {
            self.focused_workspace.clone()
        } else {
            surface
        };
        if self.workspace_checkpoint_errors.contains(&surface) {
            eprintln!(
                "taskq tray: Conversations recovery checkpoint exceeds 32 MiB; reader retained, reload refused"
            );
            if let Some(tray) = &self.tray {
                let _ = tray.set_tooltip(Some(
                    "Conversations recovery unknown: checkpoint exceeds 32 MiB; reader retained",
                ));
            }
            return;
        }
        if surface != "main" {
            if let Some((window, view, calls)) = self.detached.get_mut(&surface) {
                match recreate_workspace(window, view, calls, safe, |window, reader| {
                    workspace_view(window, self.proxy.clone(), surface.clone(), reader)
                }) {
                    Ok(()) => {
                        window.set_visible(true);
                        window.set_focus();
                    }
                    Err(reason) => {
                        eprintln!("taskq tray: detached Conversations reload unavailable: {reason}")
                    }
                }
            }
            return;
        }
        if let (Some((window, view)), Some(calls)) = (&mut self.workspace, &mut self.workspace_web)
        {
            match recreate_workspace(window, view, calls, safe, |window, reader| {
                workspace_view(window, self.proxy.clone(), "main".into(), reader)
            }) {
                Ok(()) => {
                    self.workspace_shown.clear();
                    if safe {
                        self.workspace_open = json!({"page":"conversations","thread":"recovery"});
                    }
                    window.set_visible(true);
                    window.set_focus();
                }
                Err(reason) => {
                    eprintln!(
                        "taskq tray: Conversations reload unavailable: {reason}; existing window retained"
                    );
                }
            }
        }
    }

    pub(super) fn init(
        &mut self,
        target: &tao::event_loop::EventLoopWindowTarget<Ev>,
        flow: &mut ControlFlow,
    ) {
        let b = WindowBuilder::new()
            .with_window_icon(Some(window_icon(&compact(true, false, 256.0 / 22.0))))
            .with_title("TaskQ")
            .with_transparent(true)
            .with_decorations(true)
            .with_resizable(false)
            .with_always_on_top(self.settings.pin.top)
            .with_visible(self.snapshot.is_some())
            .with_inner_size(if self.routes.iter().any(|r| r.starts_with("health-")) {
                if self.conversation_shots {
                    LogicalSize::new(1100.0, 900.0)
                } else {
                    LogicalSize::new(420.0, 900.0)
                }
            } else if self.views_shots {
                LogicalSize::new(480.0, 600.0)
            } else if self.conversation_shots && self.routes.iter().any(|r| r != "conversations") {
                LogicalSize::new(1280.0, 760.0)
            } else if self.conversation_shots {
                LogicalSize::new(800.0, 600.0)
            } else {
                LogicalSize::new(320.0, 400.0)
            })
            .with_position(tao::dpi::LogicalPosition::new(40.0, 40.0));
        // ordinary snapshots render off-screen; opacity evidence must remain visible and unoccluded (R12)
        let b = if self.snapshot.is_some() {
            b.with_position(tao::dpi::LogicalPosition::new(
                if self.agentq_shots || self.opacity_shots {
                    40.0
                } else {
                    -20000.0
                },
                if self.agentq_shots || self.opacity_shots {
                    40.0
                } else {
                    -20000.0
                },
            ))
            .with_always_on_top(self.opacity_shots)
            .with_focused(false)
        } else {
            b
        };
        #[cfg(windows)]
        let b = tao::platform::windows::WindowBuilderExtWindows::with_skip_taskbar(
            b,
            !self.settings.pin.pinned,
        );
        let p = self.proxy.clone();
        let built = b.build(target).map_err(|e| e.to_string()).and_then(|w| {
            let bundle = crate::ui_bundle::active();
            surface_view(
                &w,
                p,
                if self.conversation_shots {
                    &bundle.workspace
                } else {
                    &bundle.tray
                },
                if self.views_shots {
                    "window.taskqSurface='views';"
                } else {
                    ""
                },
                PageSurface::Menu,
                &self.web,
            )
            .map(|v| (w, v))
        });
        match built {
            Ok(x) => {
                if let Err(error) = native_titlebar(&x.0) {
                    eprintln!("taskq tray: titlebar unchanged: {error}");
                }
                if !self.views_shots
                    && !self.conversation_shots
                    && (self.settings.opacity_mode == "background" || self.settings.opacity < 100)
                    && let Err(error) = window_opacity(&x.0, self.settings.native_opacity())
                {
                    self.settings.opacity_error =
                        Some(format!("Menu opacity unavailable: {error}; Off shown"));
                    self.settings.opacity = 100;
                    self.settings.opacity_mode = "whole".into();
                    if let Err(error) = window_opacity(&x.0, 100) {
                        self.settings.opacity_error =
                            Some(format!("Menu opacity unknown: {error}"));
                    }
                }
                self.dark = is_dark(&x.0);
                self.ui = Some(x);
            }
            Err(e) => {
                eprintln!("taskq tray: {e}");
                *flow = ControlFlow::Exit;
                return;
            }
        }
        if self.snapshot.is_some() {
            return;
        }
        // macOS needs a running event loop before the icon exists
        // M15: Open, Restart TaskQ and Quit TaskQ in the tray menu (right click), naming the work they wait for
        let menu = Menu::with_items(&[
            &self.work_item,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("open", "Open", true, None),
            &MenuItem::with_id(
                "conversations-reload",
                "Reload Conversations safely",
                true,
                None,
            ),
            &MenuItem::with_id("conversations-back", "Conversations safe view", true, None),
            &MenuItem::with_id("conversations-close", "Close Conversations", true, None),
            &MenuItem::with_id("restart", "Restart TaskQ", true, None),
            &MenuItem::with_id("quit", "Quit TaskQ", true, None),
        ]);
        match TrayIconBuilder::new()
            .with_icon(rgba_icon(&compact(false, self.dark, 2.0)))
            .with_tooltip("taskq")
            .with_menu_on_left_click(false)
            .with_menu(Box::new(menu.unwrap_or_default()))
            .build()
        {
            Ok(t) => self.tray = Some(t),
            Err(e) => {
                eprintln!("taskq tray: {e}");
                *flow = ControlFlow::Exit;
            }
        }
        let p = self.proxy.clone();
        notify::watch(move |id| {
            let _ = p.send_event(Ev::Note(id));
        });
        // a pinned window survives a tray restart at its last position; off-screen: at the tray icon (M13)
        if let (Some((w, _)), true) = (&self.ui, self.settings.pin.pinned) {
            let saved = w
                .available_monitors()
                .find(|m| key(m) == self.settings.pin.display)
                .zip(self.settings.pin.pos.get(&self.settings.pin.display))
                .filter(|(m, p)| {
                    on_screen(
                        **p,
                        (
                            m.position().x,
                            m.position().y,
                            m.size().width as i32,
                            m.size().height as i32,
                        ),
                    )
                });
            match (saved, self.tray.as_ref().and_then(TrayIcon::rect)) {
                (Some((_, p)), _) => w.set_outer_position(PhysicalPosition::new(p.0, p.1)),
                (None, Some(r)) => {
                    self.settings.pin.attached_edge = place(w, r, &mut self.settings.pin).into();
                }
                _ => {}
            }
            w.set_visible(true);
        }
    }

    pub(super) fn menu_moved(&mut self, p: PhysicalPosition<i32>) {
        let Some((w, _)) = &self.ui else { return };
        if !self.settings.pin.pinned {
            self.settings.pin.pinned = true;
            #[cfg(windows)]
            {
                use tao::platform::windows::WindowExtWindows;
                w.set_skip_taskbar(false);
            }
            self.settings.save(&self.install);
            let _ = self.proxy.send_event(Ev::Sync);
        }
        let Some(m) = w.current_monitor() else { return };
        let s = w.outer_size();
        let c = if cfg!(target_os = "macos") {
            clamp((p.x, p.y), (s.width as i32, s.height as i32), work_area(&m))
        } else {
            (p.x, p.y)
        };
        if c != (p.x, p.y) {
            w.set_outer_position(PhysicalPosition::new(c.0, c.1));
        }
        self.settings.pin.display = key(&m);
        self.settings
            .pin
            .pos
            .insert(self.settings.pin.display.clone(), c);
        self.moved = true;
    }

    pub(super) fn menu_blurred(&mut self) {
        if let Some((w, _)) = &self.ui {
            if blur_refocuses(
                self.shown_at.elapsed(),
                std::mem::take(&mut self.workspace_handoff),
            ) {
                w.set_focus();
                return;
            }
            w.set_visible(false);
            self.hidden_at = Instant::now();
        }
    }

    pub(super) fn theme_changed(&mut self) {
        if let Some((w, _)) = &self.ui {
            self.dark = is_dark(w);
            self.icon_sig.clear();
            self.shown.clear();
        }
    }

    pub(super) fn quit(&mut self, ev: Ev) {
        // M15: Restart drain-stops exactly as Quit does
        self.quitting = Some(if matches!(ev, Ev::Restart) {
            "Restarting"
        } else {
            "Quitting"
        });
        let (install, p) = (self.install.clone(), self.proxy.clone());
        std::thread::spawn(move || {
            if let Err(e) = crate::lifecycle::quit(&install) {
                eprintln!("taskq tray: Quit refused: {e}");
                let _ = p.send_event(Ev::QuitFailed(e));
            }
        });
    }

    pub(super) fn quit_failed(&mut self, e: String) {
        let what = if self.quitting == Some("Restarting") {
            "Restart"
        } else {
            "Quit"
        };
        self.quitting = None;
        self.work_item
            .set_text(format!("{what} refused; see tray.log"));
        if let Some(t) = &self.tray {
            let _ = t.set_tooltip(Some(
                format!("TaskQ: {what} refused: {e}")
                    .chars()
                    .take(160)
                    .collect::<String>(),
            ));
        }
    }

    pub(super) fn raise_menu(&mut self) {
        let Some((w, _)) = &self.ui else { return };
        if !self.settings.pin.pinned
            && let Some(r) = self.tray.as_ref().and_then(TrayIcon::rect)
        {
            self.settings.pin.attached_edge = place(w, r, &mut self.settings.pin).into();
        }
        w.set_visible(true);
        w.set_focus();
        self.shown_at = Instant::now();
    }

    pub(super) fn click(&mut self, rect: Rect) {
        self.workspace_handoff = false;
        let Some((w, _)) = &self.ui else { return };
        self.anchor = Some(rect);
        // pinned: the click raises the window, never closes it (M13)
        if self.settings.pin.pinned {
            w.set_visible(true);
            w.set_focus();
            return;
        }
        // a click on the icon while open first blurs the popover: that click only closes it
        if w.is_visible() || self.hidden_at.elapsed() < Duration::from_millis(300) {
            w.set_visible(false);
            return;
        }
        self.settings.pin.attached_edge = place(w, rect, &mut self.settings.pin).into();
        w.set_visible(true);
        w.set_focus();
        self.shown_at = Instant::now();
    }

    // a notification click opens the menu (or raises the pinned window) on that card (M14)
    pub(super) fn note(&mut self, id: String) {
        let Ok(id) = serde_json::from_str::<Value>(&id) else {
            return;
        };
        let Some((w, _)) = &self.ui else { return };
        if !self.settings.pin.pinned
            && let Some(r) = self
                .anchor
                .or_else(|| self.tray.as_ref().and_then(TrayIcon::rect))
        {
            self.settings.pin.attached_edge = place(w, r, &mut self.settings.pin).into();
        }
        w.set_visible(true);
        w.set_focus();
        if id["root"].is_string() {
            push_state(
                &self.proxy,
                &mut self.web,
                &self.ui,
                &json!({"open": {"root": id["root"], "n": id["n"], "kind": id["k"], "title": id["t"], "event":id["e"]}}),
            );
        }
    }
}
