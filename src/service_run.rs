//! The `service run` loop of one project (§ 4), its native recovery records and its handover to a restart.

use crate::Res;
use crate::cli::host_cmd::setup_items;
use crate::cli::service_cmd::service_id;
#[cfg(target_os = "macos")]
use crate::cli::service_cmd::tray_app;
use crate::config::{Ctx, hostname};
use crate::paths::{dot, installed, kids_path, launcher, project_name, touch};
use crate::service::{Config, Kid, Service};
use crate::wake::{MENU_POLL, menu_open, poke_times, poll_wait, sleep_until_poke, take_reread};
use crate::{
    board, check, control, core, data, deploy, enrollment, host, incident, incoming, labels, link,
    platform, pm, policy, reports, requests, resources, runtime, snapshot, telemetry,
    telemetry_sync,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub(crate) fn kids_file(install: &Path, root: &Path) -> Res<Vec<data::KidLine>> {
    use std::io::Read;
    let path = kids_path(install, root)?;
    let file = match fs::File::open(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return match fs::symlink_metadata(dot(root).join("kids")) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
                _ => Err(
                    "legacy project kids record requires native reconciliation; file kept".into(),
                ),
            };
        }
        Err(e) => return Err(format!("kids: unreadable ({e}); file kept")),
        Ok(file) => file,
    };
    let mut text = String::new();
    file.take(data::KIDS_MAX + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("kids: unreadable ({e}); file kept"))?;
    let (owner, kids) = data::kids(&text)?;
    let current = coreq::process::identity(std::process::id())
        .map_err(|e| format!("service identity unknown: {e}"))?;
    if current.as_ref() != Some(&owner)
        && coreq::process::identity(owner.pid)
            .map_err(|e| format!("kids: previous service identity unknown: {e}; file kept"))?
            .as_ref()
            == Some(&owner)
    {
        return Err(
            "kids: previous service still alive; file kept, native reconciliation required".into(),
        );
    }
    Ok(kids)
}

/// A process that started before this boot cannot still run; its board attempt recovers like any lost heartbeat.
pub(crate) fn pre_boot(k: &data::KidLine) -> bool {
    platform::started_before_boot(&k.process.started).unwrap_or(false) && recorded(k).0 == "ended"
}

/// #1594 the verdict on a recorded native process: its identity read now against the record.
pub(crate) fn recorded(k: &data::KidLine) -> (&'static str, String) {
    let now = coreq::process::identity(k.process.pid).map_err(|e| e.to_string());
    snapshot::verdict(&snapshot::Signals {
        local: true,
        process: Some((&k.process, now)),
        ..Default::default()
    })
}

/// Power loss (#1445): drop a torn `.pending` and retire kids from a previous boot before adoption.
pub(crate) fn retire_pre_boot(install: &Path, root: &Path) -> Res<usize> {
    // kids_file first: it refuses while a previous service still owns the record.
    let mut kids = kids_file(install, root)?;
    let path = kids_path(install, root)?;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".pending");
    match fs::remove_file(path.with_file_name(name)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("kids: stale pending file kept ({e})"));
        }
        _ => {}
    }
    let before = kids.len();
    kids.retain(|k| !pre_boot(k));
    let retired = before - kids.len();
    if retired > 0 {
        write_kids(install, root, &kids)?;
    }
    Ok(retired)
}

pub(crate) fn adopt(s: &mut Service, install: &Path, root: &Path) -> Res<()> {
    let retired = retire_pre_boot(install, root)?;
    if retired > 0 {
        platform::log(&format!(
            "{}: retired {retired} kid(s) started before this boot",
            root.display()
        ));
    }
    let kids = kids_file(install, root)?;
    for k in kids {
        // #2266: one row that cannot be adopted keeps its record and board reservation; the project loop runs on.
        if let Err(e) = adopt_kid(s, install, root, k.clone()) {
            platform::log(&format!(
                "#{} execution {}: {e}; native recovery record kept, execution stays reserved",
                k.task, k.execution
            ));
        }
    }
    Ok(())
}

pub(crate) fn adopt_kid(s: &mut Service, install: &Path, root: &Path, k: data::KidLine) -> Res<()> {
    if let Some(owned) = s
        .kids
        .iter()
        .find(|owned| owned.task == k.task || owned.execution == k.execution)
    {
        if owned.task == k.task
            && owned.attempt == k.attempt
            && owned.execution == k.execution
            && owned.rt == k.runtime
            && owned.proc.native_identity()? == k.process
        {
            return Ok(());
        }
        return Err("native recovery differs from the retained handle; file kept".into());
    }
    let history = board::entries(s.board, k.task)?;
    let execution = core::executions(k.task, &history)
        .into_iter()
        .find(|x| {
            x.id == k.execution
                && x.attempt == k.attempt
                && x.host == s.cfg.me
                && x.runtime == k.runtime
        })
        .ok_or("native recovery identity does not match board ownership; file kept")?;
    match execution.release {
        Some(core::Release::Exit) => {
            if runtime::released_execution_live(&k)
                .map_err(|e| format!("released native scope unknown: {e}"))?
            {
                return Err(
                    "board exit contradicts a live native identity; file kept for reconciliation"
                        .into(),
                );
            }
            retire_kid(&s.cfg, k.task, k.attempt, k.execution)?;
            return Ok(());
        }
        Some(core::Release::NoStart) => {
            return Err("board no-start contradicts a recorded native process; file kept".into());
        }
        None => {}
    }
    let proc = runtime::adopt(root, install, &k, &execution)?;
    s.adopt(Kid {
        unknown: None,
        stopping: false,
        session_url: None,
        execution: k.execution,
        task: k.task,
        attempt: k.attempt,
        rt: k.runtime,
        proc,
    })?;
    Ok(())
}

// §4: only a confirmed logout holds admission; an unknown probe keeps its existing meaning.
pub(crate) fn login_holds(rows: &Value) -> std::collections::BTreeSet<String> {
    rows.as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["login"] == "not logged in")
        .filter_map(|r| r["runtime"].as_str().map(String::from))
        .collect()
}

pub(crate) fn service_run(c: &Ctx, host: Option<host::Shared>) -> Res<()> {
    let cfg = &c.cfg;
    let exe = launcher(&cfg.install);
    let board_env = cfg
        .board
        .worker_env(&platform::tool(&cfg.install), &cfg.web_host)?;
    let env = runtime::Env {
        context: "",
        board_env: &board_env,
        root: &cfg.root,
        exe: &exe,
        install: &cfg.install,
        permission_mode: &cfg.permission_mode,
        codex: &cfg.codex,
        delivery: cfg.delivery,
        jobs: None,
    };
    let start = |l: &runtime::Launch| {
        let issue = c
            .board
            .issue(l.task)
            .map_err(runtime::StartError::admission)?;
        let log =
            board::entries(c.board.as_ref(), l.task).map_err(runtime::StartError::admission)?;
        let context = core::task_context(l.task, &issue.body, &log);
        let env = runtime::Env {
            context: &context,
            ..env
        };
        if let Some(host) = &host {
            let h = host
                .lock()
                .map_err(|_| runtime::StartError::admission("host admission poisoned"))?;
            if let Some(reason) = &h.held {
                return Err(runtime::StartError::admission(reason.clone()));
            }
            let p = h.policy.clone().map_err(runtime::StartError::admission)?;
            let jobs = runtime::compile_jobs(
                std::thread::available_parallelism().ok().map(|n| n.get()),
                h.occupied(),
            );
            drop(h);
            let env = runtime::Env {
                permission_mode: &p.permission_mode,
                codex: &p.codex,
                jobs,
                ..env
            };
            runtime::spawn(&env, l)
        } else {
            runtime::spawn(&env, l)
        }
    };
    let mut s = Service::new(c.board.as_ref(), cfg, platform::VERSION, Box::new(start));
    s.recover = Some(Box::new(|task, execution| {
        runtime::recover(&cfg.root, &cfg.install, task, execution)
    }));
    s.host = host.clone();
    s.instance = enrollment::installation_id(&cfg.install).map(|id| service_id(&id))?;
    // § 4 machine line: agentq's `host.name` is the same `hostname` output; WSL as agentq detects it
    let wsl = cfg!(target_os = "linux")
        && (std::env::var_os("WSL_DISTRO_NAME").is_some()
            || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists());
    s.machine = Some(hostname())
        .filter(|h| !h.is_empty())
        .map(|h| if wsl { format!("{h} wsl") } else { h })
        .unwrap_or_default();
    fs::create_dir_all(dot(&cfg.root)).map_err(|e| e.to_string())?;
    // § 4 one service per root: wait, not exit, so the unit sees no crash loop
    loop {
        let other = if host.is_some() {
            host::project_service(&cfg.root)
        } else {
            Ok(other_service(&cfg.root))
        };
        match other {
            Ok(None) => break,
            Ok(Some(pid)) => log!("service pid {pid} already runs this project; waiting"),
            Err(e) => log!("{e}; waiting"),
        }
        std::thread::sleep(Duration::from_secs(cfg.poll.max(1)));
    }
    adopt(&mut s, &cfg.install, &cfg.root)?;
    let mut run = ProjectLoop::new(c, host.clone());
    loop {
        // #1297: steady successful work folds into 15-minute aggregates; failures, changes and command-linked passes stay rows.
        let _repeating = tracing::info_span!(
            "service.loop",
            coreq_repeating = true,
            project = cfg.repo.as_str(),
            actor = "service"
        )
        .entered();
        let observe = observes(host.as_ref(), &cfg.root)?;
        let pass_began = Instant::now();
        let poke_start = poke_times(&run.pokes);
        telemetry::diagnostic::poll(&cfg.install);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run.one_pass(&mut s, observe, pass_began, poke_start)
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(payload) => run.after_panic(&mut s, payload)?,
        }
    }
}

/// One project loop's inputs and the state it keeps between passes.
struct ProjectLoop<'a> {
    c: &'a Ctx,
    host: Option<host::Shared>,
    d: PathBuf,
    project: String,
    issues: String,
    log: Option<PathBuf>,
    is_installed: bool,
    began: Instant,
    incoming: incoming::Reader,
    checked: Option<Instant>,
    update: Option<String>,
    restart_empty: Option<u64>,
    waits_logged: String,
    tray: Option<std::process::Child>,
    sampled: Option<Instant>,
    agents: Option<(Instant, Value)>,
    pr_route: Option<(Instant, check::Probe)>,
    #[cfg(target_os = "macos")]
    dev_ci: Option<(String, Instant)>,
    focus_error: Option<String>,
    pruned: Option<Instant>,
    /// § 4 board budget: the pause logged last, and the cumulative counts at the last full hour
    since: u64,
    paused_logged: Option<i64>,
    /// the budget alarm is logged once per reset
    low_logged: Option<Option<i64>>,
    hour: (Instant, BTreeMap<String, u64>),
    last_hour: Option<BTreeMap<String, u64>>,
    dashq: enrollment::Sender,
    telemetry_sync: telemetry_sync::Sender,
    observation_shipper: telemetry_sync::Sender,
    policy_exchange: resources::Exchanger,
    pm_exchange: pm::Exchanger,
    reports: reports::Collector,
    link: link::Server,
    panics: host::Panics,
    pokes: [PathBuf; 2],
    observed_poke: Option<String>,
    observed_command: Option<String>,
    poked: bool,
}

/// What one pass learned, for the status it publishes.
struct PassFacts<'p> {
    r: Res<()>,
    command: Value,
    access: &'p [policy::Effective],
    requests: Value,
    dev: Value,
    setup: Value,
    kids: Vec<Value>,
    request: Option<String>,
    at: u64,
    quiet: bool,
}

impl<'a> ProjectLoop<'a> {
    fn new(c: &'a Ctx, host: Option<host::Shared>) -> Self {
        let cfg = &c.cfg;
        let d = dot(&cfg.root);
        let incoming = incoming::Reader::new(
            cfg.clone(),
            c.raw["label_schema"].as_str().map(str::to_string),
            &c.board.url(0),
        );
        let is_installed = installed(&cfg.install);
        let began = Instant::now();
        log!("{}", incident::disclosure(Some(&cfg.root)));
        log!("{}", runtime::build_cache_line());
        let _ = fs::remove_file(d.join("restart"));
        let project = project_name(&cfg.root);
        let issues = c
            .board
            .url(0)
            .strip_suffix("/0")
            .unwrap_or_default()
            .to_string();
        let log = coreq::service::log_path(&platform::tool(&cfg.install), &project);
        log!(
            "taskq {} service on {} for {}",
            platform::VERSION,
            cfg.me,
            cfg.repo
        );
        let since = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let pokes = [d.join("poke"), cfg.install.join("poke")];
        Self {
            c,
            host,
            d,
            project,
            issues,
            log,
            is_installed,
            began,
            incoming,
            since,
            pokes,
            checked: None,
            update: None,
            restart_empty: None,
            waits_logged: String::new(),
            tray: None,
            sampled: None,
            agents: None,
            pr_route: None,
            #[cfg(target_os = "macos")]
            dev_ci: None,
            focus_error: None,
            pruned: None,
            paused_logged: None,
            low_logged: None,
            hour: (Instant::now(), BTreeMap::new()),
            last_hour: None,
            dashq: Default::default(),
            telemetry_sync: Default::default(),
            observation_shipper: telemetry_sync::Sender::observations(),
            policy_exchange: Default::default(),
            pm_exchange: Default::default(),
            reports: Default::default(),
            link: Default::default(),
            panics: Default::default(),
            observed_poke: None,
            observed_command: None,
            poked: false,
        }
    }

    /// One pass: reread local state, run the board pass, publish status, act on update and handover, then wait.
    fn one_pass(
        &mut self,
        s: &mut Service<'_>,
        observe: bool,
        pass_began: Instant,
        poke_start: [Option<SystemTime>; 2],
    ) -> Res<()> {
        let c = self.c;
        let cfg = &c.cfg;
        let control = control::snapshot(
            &cfg.root,
            &mut self.observed_poke,
            &mut self.observed_command,
        );
        let command = match &control {
            Ok(snapshot) => snapshot.command.clone(),
            Err(e) => json!({"state":"unknown","reason":e}),
        };
        let wakeup = control
            .as_ref()
            .ok()
            .and_then(|s| s.link.as_ref())
            .map(|link| link.span().entered());
        let reduced =
            self.panics.quarantined() || self.began.elapsed().as_secs() < self.panics.until;
        // #1766: an observe pass reads and reports only; exchanges, probes and pruning wait for the next ordinary pass
        let quiet = reduced || observe;
        self.reload_capacity(s);
        let access = self.reread_access(s);
        if !quiet {
            if let Some(m) = policy::pin_rollback(&cfg.install, platform::VERSION) {
                log!("{m}");
            }
        }
        s.mono = self
            .host
            .as_ref()
            .and_then(|h| h.lock().ok().map(|h| h.elapsed()))
            .unwrap_or_else(|| self.began.elapsed().as_secs());
        s.wall = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.apply_focus(s, control);
        // #1819: a tray write rereads its task at once: no cached comments, no unchanged-board shortcut
        let reread = take_reread(&self.d);
        if !reread.is_empty() {
            s.board.forget();
            reread.into_iter().for_each(|t| s.reread(t));
        }
        if !quiet {
            self.probe_agents(s);
        }
        let r = if reduced {
            s.reduced()
                .and_then(|_| Err(self.panics.reason.clone().unwrap_or_default()))
        } else if observe {
            s.observe()
        } else {
            s.pass()
        };
        s.sync_host();
        for l in s.log.drain(..) {
            log!("{l}");
        }
        let requests = self.budget_report(&r);
        let at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let kids = kid_rows(s);
        let request = fs::read_to_string(self.d.join("focus.request")).ok();
        if !quiet {
            self.exchanges(s, at);
        }
        let dev = self.dev_status(s, quiet);
        let setup = self.setup_report(s, &access, quiet, at);
        let complete = r.is_ok();
        let mut status = self.status_report(
            s,
            PassFacts {
                r,
                command,
                access: &access,
                requests,
                dev,
                setup,
                kids,
                request,
                at,
                quiet,
            },
        );
        let published = publish_status(
            &platform::tool(&cfg.install),
            &cfg.root,
            &status,
            self.host.as_ref(),
        );
        drop(wakeup);
        if self.host.is_none()
            && self.is_installed
            && published
            && self.began.elapsed().as_secs() >= cfg.rollback_after
        {
            let h = cfg
                .install
                .join("releases")
                .join(platform::VERSION)
                .join("healthy");
            if !h.exists() {
                if let Err(e) = fs::write(h, "") {
                    log!("release health unknown: {e}");
                }
            }
        }
        self.check_update(s)?;
        self.restart_when_asked(s)?;
        // § 4 Data: the pruners run hourly
        if !quiet && self.pruned.is_none_or(|t| t.elapsed().as_secs() >= 3600) {
            self.pruned = Some(Instant::now());
            let live = s.retained_attempts();
            for l in data::prune(&cfg.root, &cfg.install, &self.project, &live) {
                log!("{l}");
            }
        }
        if self.host.is_none() && self.is_installed {
            keep_tray(&cfg.install, &mut self.tray);
        }
        self.handover(s, observe)?;
        // #1357: a failed handover retries at `poll`; its reduced pass may list the board for unknown ownership.
        let wait = if reduced {
            1
        } else {
            poll_wait(
                cfg.poll,
                complete,
                !s.idle(),
                self.poked,
                s.focus_off(),
                s.snapshot.as_ref(),
            )
        };
        self.wait_for_next(s, &mut status, wait, pass_began, poke_start);
        Ok(())
    }

    /// § 3: capacity reloads before each admission decision; children are untouched; § 7 delivery follows too (#1942)
    fn reload_capacity(&self, s: &mut Service<'_>) {
        let cfg = &self.c.cfg;
        let cap = s.reread_project().and_then(|v| {
            s.cfg.routing = labels::routing(&v)?;
            match &self.host {
                Some(h) => {
                    let h = h
                        .lock()
                        .map_err(|_| "host admission poisoned; launches held")?;
                    let p = h.policy.as_ref().map_err(Clone::clone)?;
                    p.apply(&mut s.cfg);
                    let cap = p.project(&cfg.root, &v)?;
                    s.cfg.limits = p.project_limits(&cfg.root, &v)?;
                    s.project_revision = p.project_revision(&cfg.root, &v).ok();
                    Ok(cap)
                }
                None => Err("no host policy; launches held".to_string()),
            }
        });
        if let Err(e) = &cap {
            log!("capacity: {e}; launches held");
        }
        s.reload(cap);
        if let Some(host) = &self.host {
            if let Ok(h) = host.lock() {
                s.gate = h.gate.clone();
            }
        }
    }

    /// § 8 Access: re-read every pass; a refused runtime is not claimed or resumed
    fn reread_access(&self, s: &mut Service<'_>) -> Vec<policy::Effective> {
        let cfg = &self.c.cfg;
        let access = policy::report(
            &cfg.install,
            &cfg.root,
            &s.cfg.permission_mode,
            &s.cfg.codex,
        );
        s.refused_access = access
            .iter()
            .filter_map(|e| Some((e.runtime.clone(), e.refused.clone()?)))
            .collect();
        // #1393: a runtime whose CLI does not resolve admits nothing instead of failing each spawn with os error 2
        for rt in runtime::agent_runtimes() {
            if let Some(r) = runtime::missing_cli(rt) {
                s.refused_access.entry(rt.to_string()).or_insert(r);
            }
        }
        s.full = access
            .iter()
            .filter(|e| e.mode == "full" && e.refused.is_none())
            .map(|e| e.runtime.clone())
            .collect();
        // One sandbox per daemon: the grant of each daemon-owned runtime, whose live daemon this pass reads back
        s.seatbelt = access
            .iter()
            .filter(|e| {
                cfg!(target_os = "macos")
                    && e.refused.is_none()
                    && runtime::of(&e.runtime).has(runtime::Cap::Background)
            })
            .map(|e| (e.runtime.clone(), e.profile.clone()))
            .collect();
        if s.gui_checked
            .is_none_or(|t: Instant| t.elapsed().as_secs() >= 300)
        {
            s.gui_checked = Some(Instant::now());
            s.gui_probe = platform::gui_probe();
            s.gui_stale = platform::stale_grant(&cfg.install, &s.gui_probe);
            s.grants = platform::grants(&cfg.install);
        }
        access
    }

    /// § 4 Data: an unreadable focus admits nothing and reads unknown with its reason
    fn apply_focus(&mut self, s: &mut Service<'_>, control: Res<control::Snapshot>) {
        let focus = control.map(|snapshot| snapshot.focus);
        if focus.as_ref().err() != self.focus_error.as_ref() {
            if let Err(e) = &focus {
                log!("{e}; nothing admitted");
            }
            self.focus_error = focus.as_ref().err().cloned();
        }
        s.focus = focus.unwrap_or_default();
        if let Some(host) = &self.host {
            if let Ok(h) = host.lock() {
                if h.draining.contains(&self.c.cfg.root) {
                    s.focus.clear();
                }
            }
        }
        s.focus_error = self.focus_error.clone();
    }

    /// § 4: runtime probes at most every 5 minutes
    fn probe_agents(&mut self, s: &mut Service<'_>) {
        if self
            .agents
            .as_ref()
            .is_some_and(|(t, _)| t.elapsed().as_secs() < 300)
        {
            return;
        }
        let cfg = &self.c.cfg;
        mark_checking(&platform::tool(&cfg.install), &cfg.root, s.wall as u64);
        self.agents = Some((
            Instant::now(),
            json!({"at": s.wall, "rows": runtime::agents(&cfg.install)}),
        ));
        // #1429: a logged-out runtime admits nothing until a probe reads it logged in
        s.logged_out = self
            .agents
            .as_ref()
            .map(|a| login_holds(&a.1["rows"]))
            .unwrap_or_default();
        s.clis = runtime::clis_versions(
            &self
                .agents
                .as_ref()
                .map_or(Value::Null, |a| a.1["rows"].clone()),
        );
    }

    /// § 4 board budget: logs a pause, a failed pass, a low budget and the hourly summary; returns the status field.
    fn budget_report(&mut self, r: &Res<()>) -> Value {
        let budget = self.c.board.requests();
        let requests_json = match &self.host {
            Some(h) => match h.lock() {
                Ok(h) => budget.json(h.request_since, h.last_hour.as_ref(), &h.hour_start),
                Err(_) => json!({"reason": "host request accounting unknown: admission poisoned"}),
            },
            None => budget.json(self.since, self.last_hour.as_ref(), &self.hour.1),
        };
        let paused = budget.paused_until();
        if paused != self.paused_logged {
            match paused {
                Some(t) => log!("{}", requests::paused_text(t)),
                None => log!("rate limit: board requests resumed"),
            }
            self.paused_logged = paused;
        }
        // while paused every pass fails the same way: the pause line above is the one report
        if let (Err(e), None) = (r, paused) {
            log!("pass: {e}");
        }
        if budget.low() && self.low_logged != Some(budget.reset) {
            let (top, n) = budget.top(&self.hour.1).unwrap_or(("none".into(), 0));
            log!(
                "board budget low: {}/{}{} until {}; top here: {top} {n}",
                budget.remaining.unwrap_or(0),
                budget.limit.unwrap_or(0),
                budget
                    .resource
                    .as_deref()
                    .map(|r| format!(" {r}"))
                    .unwrap_or_default(),
                budget
                    .reset
                    .map_or(crate::time::UNKNOWN.into(), crate::time::utc)
            );
            self.low_logged = Some(budget.reset);
        }
        if self.host.is_none() && self.hour.0.elapsed().as_secs() >= 3600 {
            let h = requests::diff(&budget.ops, &self.hour.1);
            log!("requests 1h: {}", requests::summary(&h, ""));
            requests::flush(requests::SERVICE);
            self.last_hour = Some(h);
            self.hour = (Instant::now(), budget.ops.clone());
        }
        requests_json
    }

    /// The exchanges of one ordinary pass; each runs on its own thread, so its failure never fails or delays the pass.
    fn exchanges(&mut self, s: &mut Service<'_>, at: u64) {
        let cfg = &self.c.cfg;
        // docs/design/enrollment.md: delivery runs on its own thread; its failure never fails or delays the pass
        let local = enrollment::Local {
            host: cfg.me.clone(),
            service: Some(s.instance.clone()),
            ttl: cfg.ttl.max(0) as u64,
            agents: self.agents.as_ref().map_or(Value::Null, |a| a.1.clone()),
            progress: s
                .snapshot
                .as_ref()
                .map(|x| enrollment::progress(x, &cfg.root, &cfg.me))
                .unwrap_or_default(),
        };
        if let Some(l) = self
            .dashq
            .tick(&cfg.install, s.snapshot.as_ref(), &local, at)
        {
            log!("{l}");
        }
        // docs/design/standalone-telemetry.md#remote-sync-1353: only under the telemetry grant, on its own thread
        if let Some(l) = self
            .telemetry_sync
            .tick(&cfg.install, s.snapshot.as_ref(), at)
        {
            log!("{l}");
        }
        if let Some(l) = self
            .observation_shipper
            .tick(&cfg.install, s.snapshot.as_ref(), at)
        {
            log!("{l}");
        }
        // docs/design/shared-resource-controls.md § Host adapter: the same speaker exchanges resource operations
        for l in self
            .policy_exchange
            .tick(&cfg.install, s.snapshot.as_ref(), at)
        {
            log!("{l}");
        }
        for l in self.pm_exchange.tick(&cfg.install, s.snapshot.as_ref(), at) {
            log!("{l}");
        }
        // docs/design/host-link.md: the same speaker listens on the host link and publishes its key
        s.link = self
            .link
            .tick(&cfg.install, s.snapshot.as_ref())
            .to_string();
    }

    /// § 8 item 1: only the Mac thread of the taskq checkout on channel dev, on request (#2311); merge_group CI read at most every 5 minutes per SHA
    #[cfg(target_os = "macos")]
    fn dev_status(&mut self, s: &Service<'_>, quiet: bool) -> Value {
        let c = self.c;
        let cfg = &c.cfg;
        if quiet || !self.is_installed || !platform::dev_enabled(&cfg.repo, &cfg.install) {
            return Value::Null;
        }
        // #2321: a ready task whose dependency closed after the installed commit requests the build itself
        platform::dev_dep_request(&cfg.install, &cfg.root, s.dep_closed);
        let dev_ci = &mut self.dev_ci;
        platform::dev_tick(&cfg.install, &cfg.root, &mut |sha| {
            if dev_ci
                .as_ref()
                .is_some_and(|(s, t)| s == sha && t.elapsed().as_secs() < 300)
            {
                return Err("checked".into());
            }
            *dev_ci = Some((sha.into(), Instant::now()));
            c.board.merge_ci_green(sha).map_err(|e| e.to_string())
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn dev_status(&mut self, _: &Service<'_>, _: bool) -> Value {
        Value::Null
    }

    /// § 6 setup check: local items each pass, the board's PR route at most hourly
    fn setup_report(
        &mut self,
        s: &Service<'_>,
        access: &[policy::Effective],
        quiet: bool,
        at: u64,
    ) -> Value {
        if !quiet
            && s.cfg.delivery.mode() == "pr"
            && self
                .pr_route
                .as_ref()
                .is_none_or(|(t, _)| t.elapsed().as_secs() >= 3600)
        {
            self.pr_route = Some((Instant::now(), self.c.board.pr_route()));
        }
        let limits = s
            .cap
            .as_ref()
            .map_or_else(|_| s.cfg.limits.clone(), |cap| cap.limits.clone());
        let open = s
            .snapshot
            .as_ref()
            .map(|snap| snap.tasks.iter().map(|r| (r.n, r.labels.clone())).collect());
        let setup_items = setup_items(
            &s.cfg,
            &limits,
            Ok(s.cfg.delivery),
            &self
                .agents
                .as_ref()
                .map_or(Value::Null, |a| a.1["rows"].clone()),
            access,
            open,
            self.pr_route.as_ref().map(|r| r.1.clone()),
        );
        let mut setup = check::json(&self.project, &setup_items);
        setup["checked"] = json!(at);
        setup
    }

    /// The status this pass publishes; before the first successful pass tasks are unknown, not an empty list (§ 4).
    fn status_report(&mut self, s: &Service<'_>, f: PassFacts<'_>) -> Value {
        let cfg = &self.c.cfg;
        let host = self.host.as_ref();
        let tool = platform::tool(&cfg.install);
        let error = f.r.as_ref().err().cloned();
        // #1845 A1: every TCC grant a gui worker needs, Screen Recording from the gui probe
        let mut grants = s.grants.clone();
        if !grants.is_empty() {
            let screen = if s.gui_stale {
                "stale"
            } else if s
                .gui_probe
                .as_ref()
                .err()
                .is_some_and(|e| e == platform::NOT_ALLOWED)
            {
                "missing"
            } else {
                "allowed"
            };
            grants.insert("screen_recording".into(), screen.into());
        }
        let mut status = coreq::status::common(
            &tool,
            &cfg.me,
            &self.project,
            f.at,
            f.r.err(),
            self.update.as_deref(),
        );
        // § 4 snapshot keys; before the first successful pass tasks are unknown, not an empty list
        match &s.snapshot {
            Some(snap) => {
                for (k, v) in snap.status_keys(error.as_deref()) {
                    status[k] = v;
                }
                // R6: the tray and PilotQ show the manager view's own rows
                if let Ok(t) = snap.text(&s.cfg, &Value::Null) {
                    status["report"] = json!(t);
                }
            }
            None => {
                status["complete"] = json!(false);
                status["reason"] = json!(error.as_deref().unwrap_or("no board read yet"));
                status["tasks"] = json!([]);
            }
        }
        let focus_error = &self.focus_error;
        for (k, v) in [
            (
                "focus",
                if focus_error.is_some() {
                    Value::Null
                } else {
                    json!(s.focus)
                },
            ),
            ("focus_error", json!(focus_error)),
            ("focus_command", f.command),
            ("kids", json!(f.kids)),
            ("issues", json!(self.issues)),
            ("log", json!(self.log)),
            (
                "tasks_age_seconds",
                json!(s.tasks_read.map(|t| t.elapsed().as_secs())),
            ),
            ("request", json!(f.request)),
            ("events_since", json!(s.events_since)),
            ("incoming", self.incoming.tick()),
            (
                "gui",
                json!({"listed": s.gui().is_ok(), "reason": s.gui().err(), "stale_grant": s.gui_stale, "grants": grants, "binary": cfg.install.join("taskq")}),
            ),
            (
                "waits",
                json!(
                    s.waits
                        .iter()
                        .map(|(n, reason)| json!({"task": n, "reason": reason}))
                        .collect::<Vec<_>>()
                ),
            ),
            ("execution_waits", json!(s.execution_waits)),
            ("capacity", s.capacity_json()),
            ("hold", json!(s.hold)),
            (
                "access",
                json!(f.access.iter().map(|e| e.json()).collect::<Vec<_>>()),
            ),
            ("agents", json!(self.agents.as_ref().map(|a| &a.1))),
            ("stop_progress", json!(s.stop_progress)),
            ("poll", json!(cfg.poll)),
            (
                "restart_overdue",
                json!(host.is_some_and(|h| h.lock().is_ok_and(|h| h.restart_overdue))),
            ),
            (
                "restart_waits",
                json!(
                    (self.d.join("restart").exists()
                        || host.is_some_and(|h| h.lock().is_ok_and(|h| h.restart)))
                    .then(|| restart_waits(
                        s,
                        host.is_some_and(|h| h.lock().is_ok_and(|h| h.stopping))
                    ))
                ),
            ),
            ("incident", incident::status(&cfg.root)),
            ("incident_notice", json!(incident::notice(&cfg.root))),
            ("requests", f.requests),
            ("channel", json!(platform::channel(&cfg.install).ok())),
            ("dev", f.dev),
            ("connection", json!(self.dashq.outcome)),
            ("telemetry_sync", self.telemetry_sync.status.clone()),
            (
                "observation_shipper",
                self.observation_shipper.status.clone(),
            ),
            ("telemetry", telemetry::diagnostic::health()),
            (
                "tidy",
                fs::read_to_string(self.d.join("tidy.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(Value::Null),
            ),
            ("setup", f.setup),
        ] {
            status[k] = v;
        }
        status["diagnostics"] = s.service_diagnostics();
        status["binary_evidence"] = s.binary_evidence();
        status["reports"] =
            reports::summary(&self.reports.tick(&cfg.root, cfg.poll, cfg.ttl, !f.quiet));
        status
    }

    /// § 4 step 7: a not installed service only reports a newer version; `<root>/.taskq/update` checks at once
    fn check_update(&mut self, s: &Service<'_>) -> Res<()> {
        let cfg = &self.c.cfg;
        let is_installed = self.is_installed;
        let asked = fs::remove_file(self.d.join("update")).is_ok();
        // § 8 step 3: a pending restart already goes to `current`; repeating the check would only repeat its log line
        let pending = is_installed && self.d.join("restart").exists();
        if self.host.is_some() && (asked || pending) {
            touch(&cfg.install.join(if asked { "update" } else { "restart" }));
        }
        if self.host.is_some()
            || pending
            || !(asked
                || self
                    .checked
                    .is_none_or(|t| t.elapsed().as_secs() >= cfg.update_every))
        {
            return Ok(());
        }
        self.checked = Some(Instant::now());
        if is_installed {
            match platform::service_update(&cfg.install) {
                Ok(Some(v)) => {
                    self.update = None;
                    // § 9 deploy: the release flow records the applied version before the re-exec
                    if let Ok(exe) = std::env::current_exe() {
                        for l in deploy::applied(
                            &exe,
                            std::slice::from_ref(&cfg.root),
                            platform::channel(&cfg.install).unwrap_or("live"),
                            platform::VERSION,
                            &v,
                            &hostname(),
                        ) {
                            log!("{l}");
                        }
                    }
                    reexec(s, cfg)?;
                }
                Ok(None) => self.update = None,
                Err(e) => log!("update: {e}"),
            }
        } else {
            match platform::update(&cfg.install, true) {
                Ok(found) => self.update = found,
                Err(e) => log!("update: {e}"),
            }
        }
        if asked {
            log!(
                "update requested: {}",
                self.update
                    .as_deref()
                    .map_or("up to date".into(), |v| format!("{v} found"))
            );
        }
        Ok(())
    }

    /// Windows re-exec waits for the live children; the request stays until it can go
    fn restart_when_asked(&mut self, s: &Service<'_>) -> Res<()> {
        let cfg = &self.c.cfg;
        let restart = self.d.join("restart");
        restart_watch(
            &mut self.restart_empty,
            cfg!(windows) && restart.exists() && s.kids.is_empty(),
            self.began.elapsed().as_secs(),
            cfg.update_every,
        );
        if self.host.is_none() && self.is_installed && restart.exists() {
            let waits = lifecycle_waits(s, false).join(", ");
            if waits.is_empty() {
                let _ = fs::remove_file(&restart);
                log!("restart requested");
            } else if waits != self.waits_logged {
                log!(
                    "restart to {} waits for {waits}",
                    platform::tool(&cfg.install).current().unwrap_or_default()
                );
            }
            self.waits_logged = waits;
            reexec(s, cfg)?;
        }
        Ok(())
    }

    /// An observe pass keeps the project's last handover report (#1402)
    fn handover(&self, s: &Service<'_>, observe: bool) -> Res<()> {
        let cfg = &self.c.cfg;
        if let Some(host) = self.host.as_ref().filter(|_| !observe) {
            if let Ok(mut h) = host.lock() {
                let registered = !h.draining.contains(&cfg.root);
                let drained = drained(s, &h);
                if !registered && drained {
                    h.sync(&cfg.root, vec![]);
                    h.ready(&cfg.root, false);
                    return Err("project drained".into());
                }
                if registered {
                    h.busy.insert(cfg.root.clone(), !drained);
                }
                if let Err(e) = report_handoff(s, &mut h, &cfg.root, drained) {
                    log!("handoff unknown: {e}");
                }
            }
        }
        Ok(())
    }

    /// Waits for the next pass or a poke, every 30 s a gate sample (when enabled) and a fresh capacity in status.json (§ 4)
    fn wait_for_next(
        &mut self,
        s: &mut Service<'_>,
        status: &mut Value,
        wait: u64,
        pass_began: Instant,
        poke_start: [Option<SystemTime>; 2],
    ) {
        let cfg = &self.c.cfg;
        let tool = platform::tool(&cfg.install);
        let (host, began, pokes) = (self.host.as_ref(), self.began, &self.pokes);
        let (reports, sampled) = (&mut self.reports, &mut self.sampled);
        let mut tick = || {
            let results_ready = reports.received();
            if results_ready {
                status["reports"] =
                    reports::summary(&reports.tick(&cfg.root, cfg.poll, cfg.ttl, false));
                status["tasks_age_seconds"] = json!(s.tasks_read.map(|t| t.elapsed().as_secs()));
                status["at"] = json!(
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs())
                );
            }
            if sampled.is_some_and(|t| t.elapsed().as_secs() < 30) {
                if results_ready {
                    publish_status(&tool, &cfg.root, status, host);
                }
                return;
            }
            *sampled = Some(Instant::now());
            s.mono = began.elapsed().as_secs();
            let wall = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Some(host) = host {
                if let Ok(h) = host.lock() {
                    s.gate = h.gate.clone();
                    s.mono = h.elapsed();
                }
            } else if s.gate.enabled() {
                s.gate.host_mem = platform::host_memory();
                s.gate.observe(
                    s.mono,
                    wall as i64,
                    platform::cpu_ticks(),
                    platform::memory_available(),
                );
            }
            s.wall = wall as i64;
            s.sync_host();
            status["tasks_age_seconds"] = json!(s.tasks_read.map(|t| t.elapsed().as_secs()));
            status["capacity"] = s.capacity_json();
            status["at"] = json!(wall);
            publish_status(&tool, &cfg.root, status, host);
        };
        tick();
        let until = pass_began + Duration::from_secs(wait);
        let short = pass_began + Duration::from_secs(MENU_POLL.min(wait));
        self.poked = loop {
            let end = if menu_open(&cfg.install) {
                short
            } else {
                until.min(Instant::now() + Duration::from_secs(MENU_POLL))
            };
            if sleep_until_poke(pokes, poke_start, end, &mut tick) {
                break true;
            }
            if Instant::now() >= until || menu_open(&cfg.install) {
                break false;
            }
        };
    }

    /// A panicked pass: an unhosted service dies; a hosted one records it, rebuilds, reports and retries, or after
    /// repeated panics stops its board loop.
    fn after_panic(
        &mut self,
        s: &mut Service<'_>,
        payload: Box<dyn std::any::Any + Send>,
    ) -> Res<()> {
        let cfg = &self.c.cfg;
        if self.host.is_none() {
            std::panic::resume_unwind(payload);
        }
        if self.panics.quarantined()
            && let Some(host) = &self.host
        {
            // Stop the board loop, retaining its handles solely for safe exit/update monitoring (R11).
            s.unready();
            loop {
                let monitored = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    stopped_handoff(&mut *s, host, cfg)
                }));
                if matches!(monitored, Ok(Ok(true))) {
                    return Err("project drained".into());
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        let msg = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        self.panics.record(self.began.elapsed().as_secs(), msg);
        s.rebuild();
        log!(
            "{}",
            self.panics.reason.as_deref().unwrap_or("project panic")
        );
        let mut status = coreq::status::common(
            &platform::tool(&cfg.install),
            &cfg.me,
            &self.project,
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            self.panics.reason.clone(),
            None,
        );
        status["complete"] = json!(false);
        status["kids"] = json!(s.kids.iter().map(|k| json!({"task": k.task, "attempt": k.attempt, "rt": k.rt, "pid": k.proc.id()})).collect::<Vec<_>>());
        publish_status(
            &platform::tool(&cfg.install),
            &cfg.root,
            &status,
            self.host.as_ref(),
        );
        Ok(())
    }
}

/// The status rows of this project's live children.
fn kid_rows(s: &Service<'_>) -> Vec<Value> {
    s.kids.iter().map(|k| {
        let unknown = k.unknown.as_ref();
        let child = snapshot::Child { unknown: unknown.map(String::as_str), ..Default::default() };
        let live = if snapshot::verdict(&snapshot::Signals { local: true, child: Some(child), ..Default::default() }).0 == "unknown" { "unknown" } else { "running" };
        json!({"task": k.task, "attempt": k.attempt, "rt": k.rt, "pid": k.proc.id(), "live": live, "reason": unknown})
    }).collect()
}

/// AS5 Checking (#1661): before a due probe the last published status names it in progress, its prior rows kept;
/// the pass's own status replaces the object without the mark. No prior status: nothing to mark.
pub(crate) fn mark_checking(tool: &coreq::Tool, root: &Path, at: u64) {
    let p = coreq::status::path(tool, root);
    let Some(mut s) = fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
    else {
        return;
    };
    if !s["agents"].is_object() {
        s["agents"] = json!({});
    }
    s["agents"]["checking"] = json!(at);
    if let Err(e) = platform::write_status(tool, root, &s) {
        log!("agents checking mark not written: {e}");
    }
}

pub(crate) fn publish_status(
    tool: &coreq::Tool,
    root: &Path,
    status: &Value,
    host: Option<&host::Shared>,
) -> bool {
    let mut status = status.clone();
    let complete =
        snapshot::complete(&status) && status["error"].is_null() && status["focus_error"].is_null();
    if let Some(host) = host {
        status["host_instance"] = host
            .lock()
            .map(|h| json!(h.instance))
            .unwrap_or(Value::Null);
        status["host_install"] = if complete {
            status["host_instance"]
                .as_str()
                .and_then(|instance| host::install_acknowledgement(&tool.install, root, instance))
                .map_or(Value::Null, |id| json!(id))
        } else {
            Value::Null
        };
    }
    let written = platform::write_status(tool, root, &status);
    if let Err(e) = &written {
        log!("status publication failed: {e}; observed state unknown");
    }
    let qualified = written.is_ok() && complete;
    if let Some(host) = host {
        if let Ok(mut h) = host.lock() {
            h.publication(root, qualified);
        }
    }
    qualified
}

pub(crate) fn write_kids(install: &Path, root: &Path, kids: &[data::KidLine]) -> Res<()> {
    let owner = coreq::process::identity(std::process::id())
        .map_err(|e| e.to_string())?
        .ok_or("service native identity missing")?;
    let mut text = format!("pid {} {} executor-v1\n", owner.pid, owner.started);
    for k in kids {
        text += &format!(
            "{} {} {} {} {} {} {}{}\n",
            k.task,
            k.attempt,
            k.runtime,
            k.process.pid,
            k.process.started,
            k.execution,
            k.offset,
            k.job
                .as_ref()
                .map(|scope| format!(" {scope}"))
                .unwrap_or_default()
        );
    }
    data::kids(&text)?;
    let path = kids_path(install, root)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    coreq::write_durable(&path, &text)
}

pub(crate) fn checkpoint_kid(install: &Path, root: &Path, kid: data::KidLine) -> Res<()> {
    let mut kids = kids_file(install, root)?;
    // #1561: one native process backs one execution; a record never takes another task's process.
    if let Some(other) = kids
        .iter()
        .find(|k| k.task != kid.task && k.process == kid.process)
    {
        return Err(format!(
            "native process already recorded for #{}; retry native readback",
            other.task
        ));
    }
    if let Some(old) = kids.iter_mut().find(|k| k.task == kid.task) {
        if *old == kid {
            return Ok(());
        }
        // #1638: the same execution may move to a new process only after its recorded one exited (checked below).
        let same = old.execution == kid.execution
            && old.attempt == kid.attempt
            && old.runtime == kid.runtime
            && old.job == kid.job;
        if old.execution > kid.execution || old.execution == kid.execution && !same {
            return Err(
                "execution already has different or newer native recovery identity; file kept"
                    .into(),
            );
        }
        if runtime::execution_live(old)? {
            return Err("previous native execution still present; recovery identity kept".into());
        }
        *old = kid;
    } else {
        kids.push(kid);
    }
    write_kids(install, root, &kids)
}

/// Called only after the exact execution's native Exit has been read back from the board.
pub(crate) fn retire_kid(cfg: &Config, task: u64, attempt: u64, execution: u64) -> Res<()> {
    let mut kids = kids_file(&cfg.install, &cfg.root)?;
    let before = kids.len();
    kids.retain(|k| !(k.task == task && k.attempt == attempt && k.execution == execution));
    if kids.len() == before {
        return Ok(());
    }
    write_kids(&cfg.install, &cfg.root, &kids)
}

pub(crate) fn save_kids(s: &Service, root: &Path) -> Res<()> {
    let exited = |task: u64, attempt: u64, execution: u64, runtime: &str| -> Res<bool> {
        let log = board::entries(s.board, task)?;
        Ok(core::executions(task, &log).iter().any(|x| {
            x.id == execution
                && x.attempt == attempt
                && x.host == s.cfg.me
                && x.runtime == runtime
                && x.release == Some(core::Release::Exit)
        }))
    };
    let mut kids = vec![];
    for k in &s.kids {
        // #2297: a child without native identity leaves no record; the next image rebuilds it from the board (§ 3 unknown)
        let Ok(process) = k.proc.native_identity() else {
            continue;
        };
        let line = data::KidLine {
            task: k.task,
            attempt: k.attempt,
            runtime: k.rt.clone(),
            process,
            execution: k.execution,
            offset: k.proc.input_offset()?,
            job: k.proc.job(),
        };
        // #1561: a recorded process that exited resolves from its board exit; any other record passes to the next image (#2297)
        if recorded(&line).0 == "ended" && exited(k.task, k.attempt, k.execution, &k.rt)? {
            continue;
        }
        kids.push(line);
    }
    let mut kept = vec![];
    for old in kids_file(&s.cfg.install, root)? {
        if kids.contains(&old) {
            continue;
        }
        // #1435, #2297: an unresolved record passes to the next image instead of holding the update on it
        if !exited(old.task, old.attempt, old.execution, &old.runtime)? {
            kept.push(old);
        }
    }
    kids.extend(kept);
    write_kids(&s.cfg.install, root, &kids)
}

/// #1766: a project the handover barrier keeps out of passes (#1402) still observes the board for its status.json.
pub(crate) fn observes(host: Option<&host::Shared>, root: &Path) -> Res<bool> {
    Ok(match host {
        Some(host) => !host
            .lock()
            .map_err(|_| "host handover unknown")?
            .begin_pass(root),
        None => false,
    })
}

/// #1769: a Stop drains running work only. A released attempt whose resume the Stop refused stays owned on the board
/// and resumes after the next start; waiting on it deadlocked the drain.
pub(crate) fn drained(s: &Service, h: &host::Host) -> bool {
    if h.restart && h.stopping {
        lifecycle_waits(s, true).is_empty()
    } else {
        s.idle()
    }
}

/// The end of a project pass under a pending restart: prepared once nothing it owns blocks the re-exec.
pub(crate) fn report_handoff(
    s: &Service,
    h: &mut host::Host,
    root: &Path,
    drained: bool,
) -> Res<()> {
    let ready = h.restart
        && (if h.stopping {
            drained
        } else {
            lifecycle_waits(s, false).is_empty()
        });
    let saved = if ready { save_kids(s, root) } else { Ok(()) };
    h.end_pass(root, ready && saved.is_ok());
    saved
}

/// A failed reduced loop retains its handles; an image switch also needs recovery evidence.
pub(crate) fn stopped_handoff(s: &mut Service, host: &host::Shared, cfg: &Config) -> Res<bool> {
    if !host
        .lock()
        .map_err(|_| "stopped project admission unknown")?
        .begin_pass(&cfg.root)
    {
        return Ok(false);
    }
    s.reap_owned();
    s.reconcile_ownership()?;
    s.sync_host();
    let mut h = host
        .lock()
        .map_err(|_| "stopped project admission unknown")?;
    let idle = drained(s, &h);
    h.busy.insert(cfg.root.clone(), !idle);
    report_handoff(s, &mut h, &cfg.root, idle)?;
    Ok(idle && h.draining.contains(&cfg.root))
}

/// Start the host's one `taskq tray` of the current release when its heartbeat `<install>/tray` is stale (§ 4); reap
/// the one this image started.
pub(crate) fn keep_tray(install: &Path, tray: &mut Option<std::process::Child>) {
    if !cfg!(any(target_os = "macos", windows)) {
        return;
    }
    if tray
        .as_mut()
        .is_some_and(|c| !matches!(c.try_wait(), Ok(None)))
    {
        *tray = None;
    }
    if tray.is_some() || coreq::status::age(&install.join("tray")).is_some_and(|a| a < 30) {
        return;
    }
    let t = platform::tool(install);
    let Some(v) = t.current() else { return };
    // M15: Windows keys the notification-area pin by exe path, so the tray runs as the launcher `<install>/taskq.exe`,
    // which every update rewrites with the current release
    let launcher = install.join(t.exe_name());
    let exe = if cfg!(windows) && launcher.is_file() {
        launcher
    } else {
        t.release_exe(&v)
    };
    let mut c = crate::platform::command(&exe);
    c.arg("tray");
    // through LaunchServices: a bundle exec'd by the service has the unbundled service as its responsible process,
    // and the OS refuses its authorization request (UNErrorDomain 1). `open` passes no args, env or stderr here: the
    // bundle with no args is the tray of its parent folder and writes its own tray.log (`bundled_tray`)
    #[cfg(target_os = "macos")]
    if let Some(app) = tray_app(install, &exe) {
        c = crate::platform::command("open");
        c.args(["-n", "-W", "-g"]).arg(app);
    }
    // #1755: appended, so a relaunch keeps the last tray's fault lines; it starts over past 1 MiB
    let log = install.join("tray.log");
    if fs::metadata(&log).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = fs::remove_file(&log);
    }
    let err = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map(std::process::Stdio::from)
        .unwrap_or_else(|_| std::process::Stdio::null());
    c.current_dir(install)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(err);
    // its own process group: stopping one project's service (launchctl bootout) leaves the host's tray alone
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    *tray = c.spawn().ok();
}

/// Another live process that wrote this root's fresh `status.json` (§ 4 one service per root).
pub(crate) fn other_service(root: &Path) -> Option<u32> {
    let p = dot(root).join("status.json");
    if !snapshot::status_live(&p, snapshot::ttl(root)) {
        return None;
    }
    let pid = data::json(&p).ok()??["pid"].as_u64()? as u32;
    (pid != std::process::id() && runtime::alive(pid)).then_some(pid)
}

#[cfg(unix)]
pub(crate) fn reexec(s: &Service, cfg: &Config) -> Res<()> {
    use std::os::unix::process::CommandExt;
    // App turns and Claude turns without a native identity have no adoptable handle; keep their lease until completion.
    if !lifecycle_waits(s, false).is_empty() {
        return fs::write(dot(&cfg.root).join("restart"), "").map_err(|e| e.to_string());
    }
    save_kids(s, &cfg.root)?;
    let mut command =
        crate::platform::command(cfg.install.join(platform::tool(&cfg.install).exe_name()));
    command.args(["service", "run"]);
    coreq::telemetry::capture::before_exec();
    let e = command.exec();
    incident::detect(
        "dispatch.reexec",
        concat!("src/service_run.rs:", line!()),
        "none",
        &[],
        &["reexec:failed"],
    );
    Err(format!("re-exec: {e}"))
}

#[cfg(not(unix))]
pub(crate) fn reexec(s: &Service, cfg: &Config) -> Res<()> {
    // ponytail: Windows cannot hand children to a new image; exit once no child is live and the unit restarts us.
    // The new `current` is already written, so the update waits only for the running attempts.
    if lifecycle_waits(s, false).is_empty() {
        crate::platform::service_exit(
            75,
            &format!(
                "restart to {}",
                platform::tool(&cfg.install).current().unwrap_or_default()
            ),
        );
    }
    // update checks pause while the restart request carries the re-exec until the children end
    fs::write(dot(&cfg.root).join("restart"), "").map_err(|e| e.to_string())
}

/// #2254: status names an unreadable child exit's reason, so a stuck re-exec reads as an incident, not "running unknown".
pub(crate) fn restart_waits(s: &Service, stopping: bool) -> Vec<String> {
    lifecycle_waits(s, stopping)
        .into_iter()
        .map(|w| {
            let reason = w
                .strip_prefix('#')
                .and_then(|n| n.parse::<u64>().ok())
                .and_then(|n| s.kids.iter().find(|k| k.task == n)?.unknown.as_ref());
            match reason {
                Some(e) => format!("{w} ({e})"),
                None => w,
            }
        })
        .collect()
}

/// § 8 step 3: what a pending re-exec waits for: codex-app turns only; every other child runs on (#2297).
pub(crate) fn lifecycle_waits(s: &Service, stopping: bool) -> Vec<String> {
    let mut w: Vec<String> = s
        .kids
        .iter()
        .filter(|k| stopping || runtime::handover_waits(&k.rt))
        .map(|k| format!("#{}", k.task))
        .collect();
    w.dedup();
    // a refused resume (#1601) or Stop (#1769) leaves a childless attempt to the next start
    w.extend(s.handoff_waits(true, cfg!(windows) && !stopping));
    w
}

/// True once per stuck period: `stuck` held for 2 × `interval` seconds of `elapsed`.
pub(crate) fn restart_watch(
    since: &mut Option<u64>,
    stuck: bool,
    elapsed: u64,
    interval: u64,
) -> bool {
    if !stuck {
        *since = None;
        return false;
    }
    let start = *since.get_or_insert(elapsed);
    if elapsed.saturating_sub(start) < interval.saturating_mul(2) {
        return false;
    }
    incident::detect(
        "dispatch.reexec",
        concat!("src/service_run.rs:", line!()),
        "none",
        &[],
        &["restart:stuck"],
    );
    *since = Some(u64::MAX);
    true
}
