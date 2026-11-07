//! Owner sessions (§ 8): how a setup or project request launches, links and gets trusted per runtime. Only this
//! adapter knows the native argv, output and refusals; `setup` keeps the request state machine.
use crate::runtime::{CLAUDE_CONFIG, Cap};
use crate::setup::{Request, dir};
use std::fs;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

pub enum Launched {
    Job {
        job: String,
        session: Option<String>,
        url: Option<String>,
    },
    NoStart(String),
    Unknown(String),
}

pub(crate) fn supported(runtime: &str) -> bool {
    let a = super::of(runtime);
    super::owner_stream::handles(runtime)
        || a.has(Cap::RemoteControl)
        || a.has(Cap::SandboxArgs) && a.has(Cap::Cli) && cfg!(any(target_os = "macos", windows))
}

pub(super) fn request_access(
    env: &super::Env,
    runtime: &str,
    r: &Request,
) -> Res<crate::policy::Effective> {
    match &r.choice {
        Some(choice) => {
            let mut choice = choice.clone();
            if r.state == "launched" {
                choice.grant_revision = Some(crate::policy::load(env.install)?.revision);
            } else {
                choice.validate(runtime)?;
            }
            choice.access(
                env.install,
                env.root,
                runtime,
                env.permission_mode,
                env.codex,
            )
        }
        None => super::access(env, runtime).map_err(|e| e.message),
    }
}

/// The request's runtime adapter (§ 8 table).
pub fn launch(own: &Path, r: &Request) -> Launched {
    if super::owner_stream::handles(&r.runtime) {
        return super::owner_stream::launch(own, r);
    }
    if super::of(&r.runtime).has(Cap::ThreadLink) {
        codex_launch(own, r)
    } else {
        claude_launch(own, r)
    }
}

/// The brief is a file in the session folder; the first message only points at it.
pub(crate) fn read_brief(path: &Path) -> String {
    format!(
        "Before any work, use Read to read {} in full. It is this session's brief; follow it.",
        serde_json::to_string(&path.to_string_lossy()).unwrap_or_default()
    )
}

/// The visible Codex app owns the thread after the handshake; its existing app grant bounds launch and resume (R9).
fn codex_launch(own: &Path, r: &Request) -> Launched {
    let a = super::of(&r.runtime);
    let bin = super::which(a.bin);
    if !bin.is_absolute() {
        return Launched::NoStart(format!(
            "{MISSING} {} is not on the host service PATH",
            a.bin
        ));
    }
    if super::login_state(a.bin, &bin) == "not logged in" {
        return Launched::NoStart(format!(
            "{LOGIN} {} is not logged in on this host",
            a.client
        ));
    }
    let path = own.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) {
        return Launched::NoStart(format!("setup brief: {e}"));
    }
    let mut request = match request(own, r, super::HANDSHAKE) {
        Ok(request) => request,
        Err(e) => return Launched::NoStart(e),
    };
    if r.root.is_none() {
        request.host_args.push("--skip-git-repo-check".into());
    }
    request.log = own.join("launch.log");
    request.deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
    let adapter = match super::agent(&r.runtime) {
        Ok(adapter) => adapter,
        Err(e) => return Launched::NoStart(e),
    };
    let mut run = match adapter.launch(&request) {
        Ok(run) => run,
        Err(error) => return match error.stage {
            agentq_runtime::agent::InputStage::BeforeInput => Launched::NoStart(error.detail),
            agentq_runtime::agent::InputStage::Unknown => Launched::Unknown(error.detail),
        },
    };
    loop {
        match adapter.poll(&mut run) {
            Ok(agentq_runtime::agent::State::Running) => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            Ok(_) => break,
            Err(e) => return Launched::Unknown(format!("Codex launch outcome unknown: {e}")),
        }
    }
    let report = match adapter.report(&run) {
        Ok(report) => report,
        Err(e) => return Launched::Unknown(e),
    };
    let stdout = fs::read_to_string(&request.log).unwrap_or_default();
    codex_thread(&r.runtime, &stdout, "", report.exit, |thread| {
        super::app_submit(thread, &read_brief(&path))
    })
}

/// The handshake's answer: no thread is a definite no-start; a thread whose brief is not queued is unknown.
pub(crate) fn codex_thread(
    rt: &str,
    stdout: &str,
    stderr: &str,
    code: Option<i32>,
    submit: impl FnOnce(&str) -> Res<()>,
) -> Launched {
    let thread = super::classify(rt, stdout).and_then(|r| r.session);
    let Some(thread) = thread else {
        if super::launch_cause(rt, &format!("{stdout}{stderr}")) == Some("login-required") {
            return Launched::NoStart(format!("{LOGIN} Codex asked for login"));
        }
        return Launched::NoStart(format!(
            "Codex handshake started no thread (exit {})",
            code.map_or("unknown".into(), |c| c.to_string())
        ));
    };
    // the id reaches the app URL and, on Windows, a shell: only a plain id
    if !super::plain_thread(&thread) {
        return Launched::Unknown(format!("Codex returned thread {thread:?}; outcome unknown"));
    }
    match submit(&thread) {
        Ok(()) => Launched::Job {
            job: thread.clone(),
            session: Some(thread.clone()),
            url: Some(super::thread_url(&thread)),
        },
        Err(e) => Launched::Unknown(format!(
            "{e}; brief delivery unknown, inspect Codex thread {thread}"
        )),
    }
}

/// Claude background session with Remote Control (§ 8 table); access and permission mode come from § 8 Access.
fn claude_launch(own: &Path, r: &Request) -> Launched {
    let path = own.join("brief.txt");
    if let Err(error) = fs::write(&path, &r.brief) {
        return Launched::NoStart(format!("setup brief: {error}"));
    }
    let mut request = match request(own, r, &read_brief(&path)) {
        Ok(request) => request,
        Err(error) => return Launched::NoStart(error),
    };
    request.log = own.join("launch.log");
    request.mode = agentq_runtime::agent::Mode::Background;
    request.title = Some(
        if r.root.is_some() {
            "taskq request"
        } else {
            "taskq setup"
        }
        .into(),
    );
    let adapter = match super::agent(&r.runtime) {
        Ok(adapter) => adapter,
        Err(error) => return Launched::NoStart(error),
    };
    let install = own.parent().and_then(Path::parent).unwrap_or(own);
    let root = r.root.as_deref().map_or(own, Path::new);
    let policy = match crate::host::Policy::read_for_hostname(install, crate::config::hostname) {
        Ok(policy) => policy,
        Err(error) => return Launched::NoStart(error),
    };
    let exe = std::env::current_exe().unwrap_or_default();
    let env = super::Env {
        context: "",
        root,
        exe: &exe,
        install,
        permission_mode: &policy.permission_mode,
        codex: &policy.codex,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let access = match request_access(&env, &r.runtime, r) {
        Ok(access) => access,
        Err(error) => return Launched::NoStart(error),
    };
    let home = super::accounts::default_home(&r.runtime);
    let _daemon = match super::daemon_sandbox::enter(
        &r.runtime,
        home.as_deref(),
        access.profile.as_deref(),
        false,
    ) {
        Ok(gate) => gate,
        Err(error) => return Launched::NoStart(format!("{ACCESS} {error}")),
    };
    let mut run = match adapter.launch(&request) {
        Ok(run) => run,
        Err(error) => {
            return Launched::Unknown(format!("Native background launch: {}; no replay", error.detail));
        }
    };
    if let Ok(output) = adapter.launcher_output(&mut run) {
        if let Ok(output) | Err(coreq::process::Failure::Exit(output)) = output {
            let _ = fs::write(
                &request.log,
                format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
            );
        }
    }
    if let Err(error) = adapter.poll(&mut run) {
        return Launched::Unknown(error);
    }
    let job = match adapter.background_job(&run) {
        Ok(job) => job,
        Err(cause) => {
            return Launched::Unknown(format!(
                "Native background identity unverified: {cause}; original input retained, no replay"
            ));
        }
    };
    let url = adapter.session_url(&run).ok();
    Launched::Job {
        job: job.id,
        session: Some(job.session),
        url,
    }
}

/// A launched job's native session and Remote Control link, each unknown until Claude publishes it.
pub fn claude_link(workdir: &Path, job: &str) -> (Option<String>, Option<String>) {
    let session = super::claude_agents(workdir).ok().and_then(|rows| {
        rows.iter()
            .find(|v| v.id() == Some(job))
            .map(|v| v.session().to_owned())
    });
    let url = session.as_deref().and_then(super::claude_remote);
    (session, url)
}
/// The existing owner grant becomes the same typed launch policy as worker/operator turns.
pub(super) fn request(own: &Path, r: &Request, text: &str) -> Res<agentq_runtime::agent::Launch> {
    let install = own
        .parent()
        .and_then(Path::parent)
        .ok_or("Owner installation unknown")?;
    let policy = crate::host::Policy::read_for_hostname(install, crate::config::hostname)?;
    let root = r.root.as_deref().map_or(own, Path::new);
    let exe = std::env::current_exe().map_err(|_| "Owner executable unknown")?;
    let env = super::Env {
        context: "",
        root,
        exe: &exe,
        install,
        permission_mode: &policy.permission_mode,
        codex: &policy.codex,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    let a = super::of(&r.runtime);
    let access = request_access(
        &env,
        if a.has(Cap::SandboxArgs) {
            a.setup
        } else {
            &r.runtime
        },
        r,
    )?;
    if a.has(Cap::StreamInput) && !matches!(access.mode.as_str(), "workspace" | "full") {
        return Err("Antigravity owner sessions require a Workspace or Full operator grant".into());
    }
    let launch = super::Launch {
        files: String::new(),
        execution: None,
        task: 0,
        title: String::new(),
        attempt: 0,
        role: "operator".into(),
        rt: r.runtime.clone(),
        session: r.session.clone().unwrap_or_default(),
        url: String::new(),
        resume: r.session.as_ref().map(|_| "continue".into()),
    };
    let log = if a.has(Cap::StreamInput) {
        super::owner_stream::log(own)
    } else {
        own.join("turn.out")
    };
    let mut request = super::launch::request(&env, &launch, &log, text, &access)?;
    request.title = None;
    request.compact_hook = None;
    if a.has(Cap::RemoteControl) {
        request.settings_env = super::claude_env(install, None, None)
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().into()))
            .collect();
    }
    if let Some(choice) = &r.choice {
        request.model = (!choice.model.is_empty()).then(|| choice.model.clone());
        request.effort = (!choice.effort.is_empty()).then(|| choice.effort.clone());
    }
    Ok(request)
}

/// D2: prepare one typed turn into an existing owner session, without submitting input.
pub fn turn(own: &Path, r: &Request) -> Res<agentq_runtime::agent::Launch> {
    let session = r
        .session
        .as_deref()
        .filter(|s| super::plain_thread(s))
        .ok_or("native session unknown; open it in its runtime")?;
    let a = super::of(&r.runtime);
    if a.has(Cap::ThreadLink) {
        return Err("App-owned Codex input must use its native queue; CLI resume refused".into());
    }
    if a.has(Cap::StreamInput) {
        if super::owner_stream::holder(own, r)? {
            return Err("Antigravity native turn still owns this session; no input sent".into());
        }
        super::owner_stream::native_source(session)?;
    } else {
        release_idle(own, r)?;
    }
    request(own, r, "")
}

/// D2/D5: reuse native App readback, binding canonical project, thread, exact input and terminal turn id.
fn app_history(
    own: &Path,
    r: &Request,
    input: &str,
    baseline: usize,
    after: u64,
) -> Res<super::AppHistory> {
    let session = r
        .session
        .as_deref()
        .filter(|s| super::plain_thread(s))
        .ok_or("native App session unknown")?;
    let root = r
        .root
        .as_deref()
        .map_or(own, Path::new)
        .canonicalize()
        .map_err(|_| "native App project unavailable")?;
    let path = transcript(&r.runtime, session)?;
    let mut history = super::AppHistory {
        floor: after,
        ..super::AppHistory::default()
    };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match history.read(&path, baseline, input, &root, session) {
            Ok(_) if history.reader.offset >= after => return Ok(history),
            Ok(_) => return Err("native App source shrank before the saved input boundary".into()),
            Err(reason)
                if reason
                    == "Codex native history scan pending; retry observation without queueing"
                    && std::time::Instant::now() < until => {}
            Err(reason) => return Err(reason),
        }
    }
}

pub(crate) fn app_baseline(own: &Path, r: &Request) -> Res<(usize, u64)> {
    let history = app_history(own, r, &read_brief(&own.join("brief.txt")), 0, 0)?;
    if history.current.is_some() {
        return Err("native App is working; input not queued".into());
    }
    Ok((history.count, history.reader.offset))
}

pub(crate) fn app_submit(own: &Path, r: &Request, input: &str) -> Res<()> {
    let session = r
        .session
        .as_deref()
        .filter(|s| super::plain_thread(s))
        .ok_or("native App session unknown")?;
    let install = own
        .parent()
        .and_then(Path::parent)
        .ok_or("owner installation unknown")?;
    let root = r.root.as_deref().map_or(own, Path::new);
    let exe = std::env::current_exe().map_err(|_| "owner executable unknown")?;
    let policy = crate::host::Policy::read_for_hostname(install, crate::config::hostname)?;
    let env = super::Env {
        context: "",
        root,
        exe: &exe,
        install,
        permission_mode: &policy.permission_mode,
        codex: &policy.codex,
        delivery: crate::config::Delivery::AutoMerge,
        board_env: &[],
        jobs: None,
    };
    request_access(&env, super::of(&r.runtime).setup, r)?;
    super::app_queue(session, input)
}

pub(crate) fn app_observe(
    own: &Path,
    r: &Request,
    t: &crate::dialogue::Turn,
) -> Res<crate::dialogue::AppState> {
    let history = app_history(
        own,
        r,
        &t.input,
        t.app_baseline.ok_or("native App baseline unknown")?,
        t.app_after.ok_or("native App input boundary unknown")?,
    )?;
    Ok(crate::dialogue::AppState {
        accepted: history.owned.is_some(),
        active: history.current.is_some(),
        outcome: history.ended.then(|| {
            if history.aborted {
                "failed: native App turn aborted".into()
            } else {
                "success".into()
            }
        }),
    })
}

/// D3: Claude lists a session someone has open; Codex refuses the second writer itself, so its check is the turn.
pub fn holder(_own: &Path, r: &Request) -> Res<bool> {
    if super::owner_stream::handles(&r.runtime) {
        return super::owner_stream::holder(_own, r);
    }
    if super::of(&r.runtime).has(Cap::ThreadLink) {
        return Ok(false);
    }
    Ok(holds(&claude_rows(_own, r)?, r) == Hold::Other)
}

#[derive(Debug, PartialEq)]
enum Hold {
    Free,
    Own,
    Other,
}

/// The job TaskQ launched stays open after its reply; idle, it is no other writer. Interactive, busy or foreign rows hold.
/// Only `status` tells a turn in progress: `claude agents --json` keeps `state` `working` on an idle launched job (#1762).
fn holds(rows: &[agentq_runtime::agent::BackgroundCandidate], r: &Request) -> Hold {
    let mut hold = Hold::Free;
    for v in rows.iter().filter(|v| Some(v.session()) == r.session.as_deref()) {
        let own = v.kind() == agentq_runtime::agent::BackgroundKind::Background
            && v.id() == r.job.as_deref();
        if !own || v.activity() != agentq_runtime::agent::BackgroundActivity::Idle {
            return Hold::Other;
        }
        hold = Hold::Own;
    }
    hold
}

fn claude_rows(own: &Path, r: &Request) -> Res<Vec<agentq_runtime::agent::BackgroundCandidate>> {
    let root = r.root.as_deref().map_or(own, Path::new);
    let home = super::claude_home();
    super::agent(super::CLAUDE)?.background_candidates(
        crate::platform::command(super::which(super::of(super::CLAUDE).bin)),
        &super::claude_spec(root, home.as_deref()),
        false,
        std::time::Duration::from_secs(10),
    )
}

/// D3: Claude resumes only a stopped session, so the turn stops the idle launched job first and reads its release back.
fn release_idle(own: &Path, r: &Request) -> Res<()> {
    let job = match holds(&claude_rows(own, r)?, r) {
        Hold::Free => return Ok(()),
        Hold::Other => return Err(open_in(&r.runtime)),
        Hold::Own => r.job.as_deref().unwrap_or_default(),
    };
    let (ok, out) = super::probe(
        "runtime.stop",
        &super::which(super::of(super::CLAUDE).bin),
        &["stop", job],
    )
    .ok_or("Claude stop unavailable or timed out; state unknown")?;
    if !ok {
        return Err(format!("claude stop: {}", out.trim()));
    }
    if holds(&claude_rows(own, r)?, r) == Hold::Free {
        Ok(())
    } else {
        Err("Claude session still open after stop; state unknown".into())
    }
}

/// D3 refusal text.
pub fn open_in(runtime: &str) -> String {
    format!("open in {runtime}; close it there first")
}

/// D5: the outcome only from AgentQ's report of the runtime's end record; exit alone is unknown.
pub fn outcome(runtime: &str, out: &str, err: &str) -> String {
    if super::owner_stream::handles(runtime) {
        return super::owner_stream::outcome(runtime, out);
    }
    let report = super::classify(runtime, &format!("{out}\n{err}"));
    if report.as_ref().is_some_and(|r| r.error == "session-busy") {
        return open_in(runtime);
    }
    match report {
        Some(r) if r.outcome == "ok" => "success".into(),
        Some(r) if r.outcome != "unknown" => {
            format!("failed: {}", r.detail.as_deref().unwrap_or(r.error))
        }
        _ => "unknown: the turn ended without a result record".into(),
    }
}

/// The session's transcript (D1): Claude `projects/*/<id>.jsonl`, Codex `sessions/**/rollout-*-<id>.jsonl`.
pub fn transcript_for(install: &Path, r: &Request) -> Res<PathBuf> {
    if super::owner_stream::handles(&r.runtime) {
        return super::owner_stream::transcript(&dir(install).join(&r.id), r);
    }
    transcript(
        &r.runtime,
        r.session
            .as_deref()
            .ok_or("Native session identity unknown")?,
    )
}

pub fn transcript(runtime: &str, session: &str) -> Res<PathBuf> {
    let a = super::of(runtime);
    if !a.has(Cap::ThreadLink) && !a.has(Cap::RemoteControl) {
        return Err(format!(
            "runtime {runtime:?}: no retained native transcript file; view unknown"
        ));
    }
    if a.has(Cap::ThreadLink) {
        codex_transcript_in(
            &super::codex_home().ok_or("transcript unknown: no home folder")?,
            session,
        )
    } else {
        super::claude_transcript_in(
            &super::claude_home().ok_or("transcript unknown: no home folder")?,
            session,
        )
    }
}

#[derive(Default)]
struct TranscriptPaths {
    home: PathBuf,
    dirs: Vec<(PathBuf, std::time::SystemTime)>,
    paths: std::collections::BTreeMap<String, (PathBuf, bool)>,
}
static TRANSCRIPT_PATHS: std::sync::Mutex<Vec<TranscriptPaths>> = std::sync::Mutex::new(Vec::new());
// A directory change in the clock tick of its last change keeps the stamp (NTFS: seen on 26 of 100 immediate changes).
// Only a stamp this much older than the scan start proves that a later change will show; 2 s is the coarsest tick (FAT).
pub(crate) const STAMP_SETTLE: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) fn codex_transcript_in(home: &Path, session: &str) -> Res<PathBuf> {
    crate::profile::source_read(|| codex_path(home, session))
}
fn codex_path(home: &Path, session: &str) -> Res<PathBuf> {
    if !super::plain_thread(session) {
        return Err("Codex thread identity invalid".into());
    }
    let mut caches = TRANSCRIPT_PATHS
        .lock()
        .map_err(|_| "Codex transcript path cache unavailable")?;
    let index = if let Some(i) = caches.iter().position(|c| c.home == home) {
        i
    } else {
        if caches.len() >= 4 {
            caches.remove(0);
        }
        caches.push(TranscriptPaths {
            home: home.into(),
            ..Default::default()
        });
        caches.len() - 1
    };
    let cache = &mut caches[index];
    let prior = cache.paths.get(session).cloned();
    // A vanished accepted path refuses; finding another matching name must never silently replace it.
    if prior.as_ref().is_some_and(|(p, _)| !p.is_file()) {
        return Err("Codex transcript path lost; native source unavailable".into());
    }
    let reuse = true;
    #[cfg(test)]
    let reuse = !crate::profile::SOURCE_BASELINE.get();
    if reuse
        && let Some((path, true)) = &prior
        && !cache.dirs.is_empty()
        && cache.dirs.iter().all(|(p, at)| {
            crate::profile::source_count("path_directory_checks", 1);
            fs::metadata(p).and_then(|m| m.modified()).ok().as_ref() == Some(at)
        })
    {
        crate::profile::source_count("path_cache_hits", 1);
        return Ok(path.clone());
    }
    let suffix = format!("-{session}.jsonl");
    let started = std::time::SystemTime::now();
    let (mut found, mut dirs, mut seen, mut stamps) =
        (vec![], vec![(home.join("sessions"), 0)], 0, vec![]);
    while let Some((dir, depth)) = dirs.pop() {
        crate::profile::source_count("path_directories_scanned", 1);
        let at = fs::metadata(&dir)
            .and_then(|m| m.modified())
            .map_err(|e| format!("Codex transcript unavailable: {e}"))?;
        for entry in fs::read_dir(&dir).map_err(|e| format!("Codex transcript unavailable: {e}"))? {
            seen += 1;
            crate::profile::source_count("path_entries_scanned", 1);
            if seen > 200_000 {
                return Err("Codex transcript scan exceeds 200000 entries".into());
            }
            let p = entry.map_err(|e| e.to_string())?.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            if depth < 3 && p.is_dir() {
                dirs.push((p.clone(), depth + 1));
            } else if name.starts_with("rollout-") && name.ends_with(&suffix) && p.is_file() {
                found.push(p);
            }
        }
        if fs::metadata(&dir).and_then(|m| m.modified()).ok() != Some(at) {
            return Err(
                "Codex transcript directory changed during resolution; retry exact source".into(),
            );
        }
        stamps.push((dir, at));
    }
    let path = match found.len() {
        1 => found.remove(0),
        0 => return Err("Codex transcript unavailable".into()),
        _ => {
            return Err(
                "Codex session has multiple transcript paths; inspect native history".into(),
            );
        }
    };
    if prior.as_ref().is_some_and(|(p, _)| p != &path) {
        return Err("Codex transcript path changed; native source unavailable".into());
    }
    // Directory stamps detect tree changes, not transcript integrity; AgentQ still verifies the file every read.
    // shortcut: cache at most 4096 directories / 64 sessions per account; larger trees keep the bounded full scan.
    // Invalidate other sessions' scan proof while retaining their accepted path fences.
    // A stamp not yet settled (or ahead of this clock) keeps the path fence but proves nothing: the next read scans again.
    let settled = stamps.iter().all(|(_, at)| {
        started
            .duration_since(*at)
            .is_ok_and(|age| age >= STAMP_SETTLE)
    });
    if stamps.len() <= 4096 {
        if cache.dirs != stamps {
            for (_, verified) in cache.paths.values_mut() {
                *verified = false;
            }
        }
        cache.dirs = stamps;
        if cache.paths.len() >= 64 && !cache.paths.contains_key(session) {
            cache.paths.pop_first();
        }
        cache.paths.insert(session.into(), (path.clone(), settled));
    }
    Ok(path)
}

/// Claude's project key: forward slashes on every platform; a Windows backslash key never matches (#1546),
/// nor a verbatim `//?/C:/` one (#1858).
pub fn claude_key(p: &Path) -> String {
    crate::platform::plain_path(p)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// The owner step for an untrusted folder that works while other Claude sessions run (#1555): Claude saves its own trust.
pub fn trust_step(folder: &str) -> String {
    format!(
        "Next: run `claude` in {folder} and accept its trust prompt; other Claude sessions keep running"
    )
}

/// Whether this host has a claude binary a worker launch would find (#2417).
pub fn claude_installed() -> bool {
    super::which(super::of(super::CLAUDE).bin).is_absolute()
}

/// Whether Claude trusts `root` per its config file in `<home>`, read only: `None` without a Claude config (#1555, #2300).
pub fn trust_state(home: &Path, root: &Path) -> Result<Option<bool>, String> {
    let v: serde_json::Value = match fs::read_to_string(home.join(CLAUDE_CONFIG)) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    // Claude walks parent folders: a trusted parent covers the root
    Ok(Some(root.ancestors().any(|p| {
        v["projects"][claude_key(p)]["hasTrustDialogAccepted"] == true
    })))
}

/// Onboarding trust (#1546): `<install>/setup` and every registered root get `hasTrustDialogAccepted` in
/// Claude's config file in `<home>`. Nothing trusted yet changes only while no Claude runs, after a backup; gap lines only.
pub fn trust(
    install: &Path,
    home: &Path,
    claude_pids: impl FnOnce() -> Res<Vec<String>>,
) -> Res<Vec<String>> {
    let roots = fs::read_to_string(install.join("projects")).or_else(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Ok(String::new())
        } else {
            Err(format!("project registry unreadable: {e}"))
        }
    })?;
    let keys: Vec<String> = std::iter::once(dir(install))
        .chain(
            roots
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(PathBuf::from),
        )
        .map(|p| claude_key(&p))
        .collect();
    trust_keys(home, &keys, claude_pids, false)
}

// Serialize TaskQ's project threads; Claude itself still requires the process guard below.
static TRUST_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// R9: registration authorizes only this exact root. Missing trust is repaired, never an owner prompt.
pub(crate) fn registered_trust(
    install: &Path,
    root: &Path,
    config: Option<&Path>,
) -> Res<Vec<String>> {
    registered_trust_with(install, root, config, claude_pids)
}

pub(crate) fn registered_trust_with(
    install: &Path,
    root: &Path,
    config: Option<&Path>,
    pids: impl FnOnce() -> Res<Vec<String>>,
) -> Res<Vec<String>> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !crate::host::projects(install)?.contains(&root) {
        return Err("trust setup: root is not registered".into());
    }
    let home = match config {
        Some(p) => p.to_path_buf(),
        None => crate::platform::user_home().ok_or("trust setup: host home unknown")?,
    };
    // A selected CLAUDE_CONFIG_DIR stores the config file inside that directory.
    let key = claude_key(&root);
    let mut keys = vec![key.clone()];
    if cfg!(windows) {
        keys.push(key.replace('/', "\\"));
    }
    trust_keys(&home, &keys, pids, true)
}

/// Host registration/startup prepares the default config; selected account configs are checked at launch.
pub(crate) fn registered_host_trust(install: &Path, root: &Path) -> Res<Vec<String>> {
    if !super::which(super::of(super::CLAUDE).bin).is_file() {
        return Ok(vec![]);
    }
    let config = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    registered_trust(install, root, config.as_deref())
}

fn trust_keys(
    home: &Path,
    keys: &[String],
    claude_pids: impl FnOnce() -> Res<Vec<String>>,
    automatic: bool,
) -> Res<Vec<String>> {
    let _guard = TRUST_WRITE
        .lock()
        .map_err(|_| "Claude trust writer unavailable")?;
    let _file_lock = coreq::file_lock::try_lock(&home.join(format!("{CLAUDE_CONFIG}.taskq-lock")))
        .map_err(|e| format!("Claude trust writer busy: {e}"))?;
    let path = home.join(CLAUDE_CONFIG);
    let text = match fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Claude config unreadable: {e}")),
    };
    let (next, changed) = trusted(text.as_deref(), &keys)?;
    let mut lines = vec![];
    if changed.is_empty() {
        return Ok(lines);
    }
    // Claude rewrites the whole file: a running one would drop the change, so the owner lets Claude write it (#1555)
    let pids =
        claude_pids().map_err(|e| format!("Claude processes unknown ({e}); trust unchanged"))?;
    if !pids.is_empty() {
        if automatic {
            return Err(
                "Claude trust setup pending: Claude runs; service retries when it exits".into(),
            );
        }
        return Ok(changed
            .iter()
            .flat_map(|k| {
                [
                    format!(
                        "Trust: {k} not trusted; Claude runs (pid {}), so onboard leaves the file",
                        pids.join(", ")
                    ),
                    trust_step(k),
                ]
            })
            .collect());
    }
    if let Some(t) = &text {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let backup = home.join(format!("{CLAUDE_CONFIG}.bak-onboard-{ms}"));
        // copy keeps the config's owner-only mode; a plain write would leave it world-readable
        fs::copy(&path, &backup)
            .map_err(|e| format!("Claude config backup: {e}; trust unchanged"))?;
        lines.push(format!("Backup: {}", backup.display()));
    }
    let tmp = home.join(format!("{CLAUDE_CONFIG}.onboard-tmp"));
    let mut open = fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    if text.is_some() {
        fs::copy(&path, &tmp).map_err(|e| format!("Claude config write: {e}"))?;
    }
    std::io::Write::write_all(
        &mut open
            .open(&tmp)
            .map_err(|e| format!("Claude config write: {e}"))?,
        next.as_bytes(),
    )
    .map_err(|e| format!("Claude config write: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Claude config write: {e}"))?;
    let (_, left) = trusted(fs::read_to_string(&path).ok().as_deref(), &keys)?;
    if !left.is_empty() {
        return Err(format!(
            "Claude config readback: {} not trusted",
            left.join(", ")
        ));
    }
    lines.extend(changed.iter().map(|k| format!("Trust: {k} trusted")));
    Ok(lines)
}

/// The config with every key trusted, and the keys that were not.
pub(crate) fn trusted(text: Option<&str>, keys: &[String]) -> Res<(String, Vec<String>)> {
    let mut v: serde_json::Value = match text {
        Some(t) => serde_json::from_str(t).map_err(|e| format!("Claude config unreadable: {e}"))?,
        None => serde_json::json!({}),
    };
    let projects = v
        .as_object_mut()
        .ok_or("Claude config is not an object")?
        .entry("projects")
        .or_insert_with(|| serde_json::json!({}));
    let projects = projects
        .as_object_mut()
        .ok_or("Claude config projects is not an object")?;
    let mut changed = vec![];
    for k in keys {
        let p = projects
            .entry(k.clone())
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or(format!("Claude config project {k} is not an object"))?;
        if p.get("hasTrustDialogAccepted") != Some(&serde_json::Value::Bool(true)) {
            p.insert("hasTrustDialogAccepted".into(), true.into());
            changed.push(k.clone());
        }
    }
    Ok((
        serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
        changed,
    ))
}

/// Running Claude processes of this user's machine; an unreadable list is an error, never none (R12).
pub fn claude_pids() -> Res<Vec<String>> {
    let out = if cfg!(windows) {
        crate::platform::command("tasklist")
            .args([
                "/FI",
                format!("IMAGENAME eq {}", super::CLAUDE_EXE).as_str(),
                "/FO",
                "CSV",
                "/NH",
            ])
            .output()
    } else {
        // ponytail: process name `claude` (native installs); an npm `node` Claude is missed, match the command line if one shows up
        crate::platform::command("pgrep")
            .args(["-x", super::of(super::OWNER).bin])
            .output()
    }
    .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    if cfg!(windows) {
        if !out.status.success() {
            return Err(format!("tasklist exit {:?}", out.status.code()));
        }
        return Ok(text
            .lines()
            .filter_map(|l| {
                l.split("\",\"")
                    .nth(1)
                    .filter(|_| l.starts_with(&format!("\"{}\"", super::CLAUDE_EXE)))
                    .map(String::from)
            })
            .collect());
    }
    match out.status.code() {
        Some(0) => Ok(text.split_whitespace().map(String::from).collect()),
        Some(1) => Ok(vec![]),
        c => Err(format!("pgrep exit {c:?}")),
    }
}

pub(crate) const MISSING: &str = "runtime missing:";
pub(crate) const LOGIN: &str = "sign-in needed:";
pub(crate) const ACCESS: &str = "access refused:";

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
