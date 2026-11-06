//! The `tray --snapshot` capture sequence.

use super::*;

impl TrayState {
    // snapshot mode: per step push a state, wait for layout, capture, next
    pub(super) fn shot(&mut self, flow: &mut ControlFlow) {
        let dir = self.snapshot.clone().unwrap_or_default();
        let steps = shots(&self.install, &self.routes);
        if self.step == 0 {
            let _ = fs::create_dir_all(&dir);
        }
        if self.step == 0 && self.agentq_shots {
            let warning = self
                .routes
                .iter()
                .find_map(|r| agentq_fixture(r))
                .is_some_and(|f| f["warning"] == true);
            let empty = summary(&[], day(now()));
            for (name, full) in [("icon", false), ("summary", true)] {
                let image = full
                    .then(|| draw(&empty, true, 1.0, 2.0).ok())
                    .flatten()
                    .unwrap_or_else(|| compact(false, true, 2.0));
                fs::write(
                    dir.join(format!("agentq-{name}.png")),
                    agentq_mark(image, warning).png(),
                )
                .expect("write fixture icon");
            }
        }
        if self.step == 0 && (self.routes.is_empty() || self.routes.iter().any(|r| r == "icon")) {
            let ps: Vec<Value> = roots(&self.install)
                .iter()
                .map(|r| project(&self.install, r))
                .collect();
            let s = summary(&ps, self.settings.range(now()));
            // this Mac's menu-bar height and backing scale (A5); the names record them
            let (k, dpr) = (menu_bar_scale(), backing_scale());
            let size = format!("{}pt@{dpr}x", 22.0 * k);
            for (theme, d) in [("dark", true), ("light", false)] {
                for (mode, run) in [("run", true), ("pause", false)] {
                    for n in [16u32, 32, 64, 128, 256, 512, 1024] {
                        let _ = fs::write(
                            dir.join(format!("app-compact-{mode}-{theme}-{n}.png")),
                            compact(run, d, n as f32 / 22.0).png(),
                        );
                    }
                    let s = Summary { run, ..s.clone() };
                    let _ = fs::write(
                        dir.join(format!("icon-compact-{mode}-{theme}-{size}.png")),
                        compact(run, d, k * dpr).png(),
                    );
                    for name in ["summary"] {
                        if let Ok(img) = draw(&s, d, k, dpr) {
                            let _ = fs::write(
                                dir.join(format!("icon-{name}-{mode}-{theme}-{size}.png")),
                                img.png(),
                            );
                        }
                    }
                }
            }
        }
        let Some((theme, page, font, script)) = steps.get(self.step).cloned() else {
            if !self.web.pending.is_empty() {
                self.shot_wait = Some(Ev::Shot);
                return;
            }
            println!("snapshots in {}", dir.display());
            if !self.overflow.is_empty() {
                eprintln!("horizontal overflow: {}", self.overflow.join(", "));
                crate::platform::exit(1);
            }
            if !self.capture_errors.is_empty() {
                eprintln!(
                    "native capture unavailable: {}",
                    self.capture_errors.join("; ")
                );
                crate::platform::exit(1);
            }
            *flow = ControlFlow::Exit;
            return;
        };
        let mut s = self.settings.clone();
        s.font = font;
        if self.opacity_shots {
            let (_, mode, value) = OPACITY_SHOTS
                .iter()
                .find(|(js, _, _)| *js == script)
                .expect("opacity snapshot");
            s.opacity_mode = (*mode).into();
            if *mode == "whole" {
                s.opacity = *value;
            } else {
                s.background_opacity = *value;
            }
            if let Some((w, _)) = &self.ui {
                if let Err(error) = window_opacity(w, s.native_opacity()) {
                    self.capture_errors.push(error);
                }
            }
        }
        let mut st = state(
            &self.install,
            &s,
            theme == "dark",
            Some((theme, &page)),
            &Value::Null,
            &mut crate::pending::Pending::default(),
        );
        // the Conversations and question fixtures merge invented tasks into the real pushed state (palette, font, theme)
        self.checkpoint_complete = false;
        self.checkpoint_deadline = None;
        st["checkpoint"] = json!(self.step);
        let merged = self.conversation_shots
            || ANSWERS.iter().any(|(_, s)| *s == script)
            || REPORT_SHOTS.iter().any(|(_, s)| *s == script);
        if merged {
            if let Some((_, wv)) = &self.ui {
                web_eval(
                    wv,
                    &format!("{script}({st})"),
                    "tray.web.fixture",
                    &self.proxy,
                    &mut self.web,
                );
            }
        } else {
            push_state(&self.proxy, &mut self.web, &self.ui, &st);
        }
        if let (Some((_, wv)), false) = (&self.ui, script.is_empty() || merged) {
            web_eval(wv, script, "tray.web.fixture", &self.proxy, &mut self.web);
        }
        let p = self.proxy.clone();
        let step = self.step;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            let _ = p.send_event(Ev::Shot2(step));
        });
    }

    pub(super) fn shot2(&mut self) {
        let Some((window, wv)) = &self.ui else { return };
        let dir = self.snapshot.clone().unwrap_or_default();
        let p = self.proxy.clone();
        let name = format!(
            "popover-{}.png",
            shot_name(&self.install, &self.routes, self.step)
        );
        if !self.web.pending.is_empty() {
            self.shot_wait = Some(Ev::Shot2(self.step));
            return;
        }
        // the workspace page has no menu to measure
        if self.routes.iter().any(|r| r == "conversations-checkpoint") && !self.checkpoint_complete
        {
            if self.checkpoint_deadline.is_none() {
                self.checkpoint_deadline = Some(Instant::now() + Duration::from_secs(5));
                let proxy = self.proxy.clone();
                let step = self.step;
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    let _ = proxy.send_event(Ev::Shot2(step));
                });
            }
            if self
                .checkpoint_deadline
                .is_some_and(|deadline| Instant::now() < deadline)
            {
                return;
            }
            self.capture_errors
                .push(format!("{name}: native checkpoint did not complete"));
        }
        if !self.conversation_shots {
            web_eval(
                wv,
                &format!("taskq.measure({})", json!(&name)),
                "tray.web.measure",
                &self.proxy,
                &mut self.web,
            );
        }
        #[cfg(windows)]
        if self.opacity_shots {
            self.capture_errors.push(format!(
                "{name}: native composition capture is unavailable on this snapshot adapter"
            ));
        }
        #[cfg(target_os = "macos")]
        if (self.agentq_shots || self.opacity_shots)
            && let Err(error) = capture_screen(window, &dir.join(format!("screen-{name}")))
        {
            self.capture_errors.push(format!("{name}: {error}"));
        }
        self.step += 1;
        capture(wv, dir.join(name), move || {
            let _ = p.send_event(Ev::Shot);
        });
    }
}
