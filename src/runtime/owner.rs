//! Owner sessions (§ 8): how a setup or project request launches, links and gets trusted per runtime. Only this
//! adapter knows the native argv, output and refusals; `setup` keeps the request state machine.
use crate::setup::{dir, Request};
use std::fs;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

pub enum Launched { Job { job: String, session: Option<String>, url: Option<String> }, NoStart(String), Unknown(String) }

/// The request's runtime adapter (§ 8 table).
pub fn launch(own: &Path, r: &Request) -> Launched {
    if super::of(&r.runtime).codex { codex_launch(own, r) } else { claude_launch(own, r) }
}

/// The brief is a file in the session folder; the first message only points at it.
fn read_brief(path: &Path) -> String {
    format!("Before any work, use Read to read {} in full. It is this session's brief; follow it.", serde_json::to_string(&path.to_string_lossy()).unwrap_or_default())
}

/// Codex app thread (§ 8 Codex app, #1522): a `codex exec` handshake turn in the session folder creates the thread, then
/// the app shows it and gets the brief. Access is the `codex-app` grant, as for app workers; nothing falls back (R9).
fn codex_launch(own: &Path, r: &Request) -> Launched {
    let a = super::of(&r.runtime);
    let bin = super::which(a.bin);
    if !bin.is_absolute() { return Launched::NoStart(format!("{MISSING} {} is not on the host service PATH", a.bin)); }
    if super::login_state(a.bin, &bin) == "not logged in" { return Launched::NoStart(format!("{LOGIN} {} is not logged in on this host", a.client)); }
    let install = own.parent().and_then(Path::parent).unwrap_or(own);
    let codex = crate::host::Policy::read_for_hostname(install, crate::hostname).map_or(vec![], |p| p.codex);
    let exe = std::env::current_exe().unwrap_or_default();
    let env = super::Env { root: own, exe: &exe, install, permission_mode: "", codex: &codex, publish: "", board_env: &[], jobs: None };
    let access = match super::access(&env, a.setup) { Ok(a) => a, Err(e) => return Launched::NoStart(e.message) };
    let path = own.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) { return Launched::NoStart(format!("setup brief: {e}")); }
    let mut c = match super::cli(&r.runtime, &access, &env) { Ok(c) => c, Err(e) => return Launched::NoStart(format!("Codex launch: {e}")) };
    crate::platform::quiet(&mut c);
    c.current_dir(own).env_remove("CLAUDE_CODE_SESSION_ID").env_remove("CODEX_THREAD_ID")
        .env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT")
        // the session folder is no repository; skipping that check trusts nothing and leaves sandbox and approvals as granted
        .args(["exec", "--json", "--skip-git-repo-check"]).args(access.codex.as_deref().unwrap_or(&codex)).arg("-C").arg(own).arg("-");
    use coreq::process::Failure;
    let out = match coreq::process::exchange("setup.codex-launch", &mut c, super::HANDSHAKE.as_bytes(), std::time::Duration::from_secs(60)) {
        Ok(out) | Err(Failure::Exit(out)) => out,
        Err(e @ (Failure::InvalidDeadline | Failure::Capacity | Failure::Owner(_) | Failure::Spawn(_) | Failure::InputLimit)) => return Launched::NoStart(format!("Codex launch: {e}")),
        Err(e) => return Launched::Unknown(format!("Codex launch outcome unknown: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::write(own.join("launch.log"), format!("{stdout}\n{stderr}"));
    codex_thread(&stdout, &stderr, out.status.code(), |thread| super::app_submit(thread, &read_brief(&path)))
}

/// The handshake's answer: no thread is a definite no-start; a thread whose brief is not queued is unknown.
pub(crate) fn codex_thread(stdout: &str, stderr: &str, code: Option<i32>, submit: impl FnOnce(&str) -> Res<()>) -> Launched {
    let thread = stdout.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["type"] == "thread.started").and_then(|v| v["thread_id"].as_str().map(String::from));
    let Some(thread) = thread else {
        let text = format!("{stdout}{stderr}").to_lowercase();
        if text.contains("401") || text.contains("login") || text.contains("log in") { return Launched::NoStart(format!("{LOGIN} Codex asked for login")); }
        return Launched::NoStart(format!("Codex handshake started no thread (exit {})", code.map_or("unknown".into(), |c| c.to_string())));
    };
    // the id reaches the app URL and, on Windows, a shell: only a plain id
    if !super::plain_thread(&thread) { return Launched::Unknown(format!("Codex returned thread {thread:?}; outcome unknown")); }
    match submit(&thread) {
        Ok(()) => Launched::Job { job: thread.clone(), session: Some(thread.clone()), url: Some(super::thread_url(&thread)) },
        Err(e) => Launched::Unknown(format!("{e}; brief delivery unknown, inspect Codex thread {thread}")),
    }
}

/// Claude background session with Remote Control (§ 8 table); access and permission mode come from § 8 Access.
fn claude_launch(own: &Path, r: &Request) -> Launched {
    let workdir = r.root.as_deref().map_or(own, Path::new);
    let a = super::of(&r.runtime);
    let bin = super::which(a.bin);
    if !bin.is_absolute() { return Launched::NoStart(format!("{MISSING} {} is not on the host service PATH", a.bin)); }
    if super::login_state(a.bin, &bin) == "not logged in" { return Launched::NoStart(format!("{LOGIN} {} is not logged in on this host", a.client)); }
    let install = own.parent().and_then(Path::parent).unwrap_or(own);
    let policy = crate::host::Policy::read_for_hostname(install, crate::hostname).ok();
    let mode = policy.as_ref().map_or("dontAsk".to_string(), |p| p.permission_mode.clone());
    let exe = std::env::current_exe().unwrap_or_default();
    let env = super::Env { root: workdir, exe: &exe, install, permission_mode: &mode, codex: &[], publish: "", board_env: &[], jobs: None };
    let access = match super::access(&env, &r.runtime) { Ok(a) => a, Err(e) => return Launched::NoStart(e.message) };
    let path = own.join("brief.txt");
    if let Err(e) = fs::write(&path, &r.brief) { return Launched::NoStart(format!("setup brief: {e}")); }
    let mut c = match super::cli(&r.runtime, &access, &env) { Ok(c) => c, Err(e) => return Launched::NoStart(e) };
    c.current_dir(workdir).env_remove("CLAUDE_CODE_SESSION_ID").env_remove("CODEX_THREAD_ID")
        .env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT")
        .args(["--bg", "--settings", &serde_json::json!({"env": super::claude_env(install, None, None)}).to_string(), "--remote-control", if r.root.is_some() { "taskq request" } else { "taskq setup" }, "--permission-mode", access.claude.as_deref().unwrap_or(&mode)]);
    let input = read_brief(&path);
    use coreq::process::Failure;
    let out = match coreq::process::exchange("setup.claude-launch", &mut c, input.as_bytes(), std::time::Duration::from_secs(30)) {
        Ok(out) | Err(Failure::Exit(out)) => out,
        Err(e @ (Failure::InvalidDeadline | Failure::Capacity | Failure::Owner(_) | Failure::Spawn(_) | Failure::InputLimit)) => return Launched::NoStart(format!("Claude launch: {e}")),
        Err(e) => return Launched::Unknown(format!("Claude launch outcome unknown: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::write(own.join("launch.log"), format!("{stdout}\n{stderr}"));
    // Claude refuses before it starts anything: a definite no-start, never `interrupted`.
    if !out.status.success() && stderr.contains("Workspace not trusted") { return Launched::NoStart(format!("{ACCESS} Claude workspace not trusted: {}", workdir.display())); }
    if !out.status.success() && (stderr.contains("/login") || stderr.contains("Not logged in")) { return Launched::NoStart(format!("{LOGIN} Claude Code asked for login")); }
    match stdout.lines().find_map(|l| l.strip_prefix("backgrounded · ").and_then(|s| s.split_whitespace().next())) {
        // One link read at launch, never after: the Claude session manager owns the session from here.
        Some(id) if out.status.success() && super::native_job_id(id) => {
            let (session, url) = claude_link(workdir, id);
            Launched::Job { job: id.into(), session, url }
        }
        _ => Launched::Unknown("Claude returned no background job; outcome unknown, inspect native sessions".into()),
    }
}

/// A launched job's native session and Remote Control link, each unknown until Claude publishes it.
pub fn claude_link(workdir: &Path, job: &str) -> (Option<String>, Option<String>) {
    let session = super::claude_agents(workdir).ok()
        .and_then(|rows| rows.iter().find(|v| v["id"] == job).and_then(|v| v["sessionId"].as_str().map(String::from)));
    let url = session.as_deref().and_then(super::claude_remote);
    (session, url)
}
/// D2: one owner turn into a launched session, text on stdin; the caller spawns it. Never `--bg --resume`: that copies the session.
pub fn turn(own: &Path, r: &Request) -> Res<std::process::Command> {
    let a = super::of(&r.runtime);
    let session = r.session.as_deref().filter(|s| super::plain_thread(s)).ok_or("native session unknown; open it in its runtime")?;
    if !super::which(a.bin).is_absolute() { return Err(format!("{MISSING} {} is not on the host service PATH", a.bin)); }
    let install = own.parent().and_then(Path::parent).unwrap_or(own);
    let policy = crate::host::Policy::read_for_hostname(install, crate::hostname).ok();
    let mode = policy.as_ref().map_or("dontAsk".to_string(), |p| p.permission_mode.clone());
    let codex = policy.map_or(vec![], |p| p.codex);
    // the same folder and grant as the launch, so the turn resumes where the session started
    let workdir = if a.codex { own } else { r.root.as_deref().map_or(own, Path::new) };
    let exe = std::env::current_exe().unwrap_or_default();
    let env = super::Env { root: workdir, exe: &exe, install, permission_mode: &mode, codex: &codex, publish: "", board_env: &[], jobs: None };
    let access = super::access(&env, if a.codex { a.setup } else { &r.runtime }).map_err(|e| e.message)?;
    let mut c = super::cli(&r.runtime, &access, &env)?;
    crate::platform::quiet(&mut c);
    c.current_dir(workdir).env_remove("CLAUDE_CODE_SESSION_ID").env_remove("CODEX_THREAD_ID")
        .env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE").env_remove("TASKQ_ROOT");
    if a.codex {
        c.args(["exec", "--json", "--skip-git-repo-check"]).args(access.codex.as_deref().unwrap_or(&codex)).args(["resume", session, "-"]);
    } else {
        // print mode streams JSON only with --verbose
        c.args(["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", access.claude.as_deref().unwrap_or(&mode),
            "--settings", &serde_json::json!({"env": super::claude_env(install, None, None)}).to_string(), "--resume", session]);
    }
    Ok(c)
}

/// D3: Claude lists a session someone has open; Codex refuses the second writer itself, so its check is the turn.
pub fn holder(_own: &Path, r: &Request) -> Res<bool> {
    if super::of(&r.runtime).codex { return Ok(false); }
    let (ok, out) = super::probe("runtime.agents", &super::which("claude"), &["agents", "--json"]).ok_or("Claude agents unavailable or timed out")?;
    if !ok { return Err(format!("claude agents: {}", out.trim())); }
    let rows: Vec<serde_json::Value> = serde_json::from_str(&out).map_err(|e| format!("claude agents: {e}"))?;
    Ok(rows.iter().any(|v| v["sessionId"].as_str().is_some_and(|s| Some(s) == r.session.as_deref())))
}

/// D3 refusal text.
pub fn open_in(runtime: &str) -> String { format!("open in {runtime}; close it there first") }

/// D5: the outcome only from the runtime's end record (Claude `result`, Codex `turn.completed`/`turn.failed`); exit alone is unknown.
pub fn outcome(runtime: &str, out: &str, err: &str) -> String {
    let codex = super::of(runtime).codex;
    if codex && format!("{out}{err}").contains("already has an active writer") { return open_in(runtime); }
    for v in out.lines().rev().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()) {
        match (codex, v["type"].as_str()) {
            (false, Some("result")) if v["is_error"] != true && v["subtype"] == "success" => return "success".into(),
            (false, Some("result")) => return format!("failed: {}", v["subtype"].as_str().unwrap_or("error")),
            (true, Some("turn.completed")) => return "success".into(),
            (true, Some("turn.failed")) => return format!("failed: {}", v["error"]["message"].as_str().unwrap_or("error")),
            _ => {}
        }
    }
    "unknown: the turn ended without a result record".into()
}

/// The session's transcript (D1): Claude `projects/*/<id>.jsonl`, Codex `sessions/**/rollout-*-<id>.jsonl`.
pub fn transcript(runtime: &str, session: &str) -> Res<PathBuf> {
    // ponytail: no Windows probe qualified these homes (R12); unknown there until one does
    if cfg!(windows) { return Err("transcript unknown: Windows transcript paths are not qualified".into()); }
    let home = |var: &str, dir: &str| std::env::var_os(var).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(dir))).ok_or("transcript unknown: no home folder".to_string());
    if super::of(runtime).codex { codex_transcript_in(&home("CODEX_HOME", ".codex")?, session) } else { super::claude_transcript_in(&home("CLAUDE_CONFIG_DIR", ".claude")?, session) }
}

pub(crate) fn codex_transcript_in(home: &Path, session: &str) -> Res<PathBuf> {
    if !super::plain_thread(session) { return Err("Codex thread identity invalid".into()); }
    let suffix = format!("-{session}.jsonl");
    let (mut found, mut dirs, mut seen) = (vec![], vec![(home.join("sessions"), 0)], 0);
    // ponytail: one scan of sessions/YYYY/MM/DD per read; remember the path per session if this shows up in telemetry
    while let Some((dir, depth)) = dirs.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| format!("Codex transcript unavailable: {e}"))? {
            seen += 1;
            if seen > 200_000 { return Err("Codex transcript scan exceeds 200000 entries".into()); }
            let p = entry.map_err(|e| e.to_string())?.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            if depth < 3 && p.is_dir() { dirs.push((p.clone(), depth + 1)); } else if name.starts_with("rollout-") && name.ends_with(&suffix) && p.is_file() { found.push(p); }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err("Codex transcript unavailable".into()),
        _ => Err("Codex session has multiple transcript paths; inspect native history".into()),
    }
}

/// Claude's project key: forward slashes on every platform; a Windows backslash key never matches (#1546).
pub fn claude_key(p: &Path) -> String { p.display().to_string().replace('\\', "/") }

/// The owner step for an untrusted folder that works while other Claude sessions run (#1555): Claude saves its own trust.
pub fn trust_step(folder: &str) -> String { format!("Next: run `claude` in {folder} and accept its trust prompt; other Claude sessions keep running") }

/// `service install` (#1555): the new root's trust, read only; nothing when trusted or Claude has no config here.
pub fn trust_gap(home: &Path, root: &Path) -> Vec<String> {
    let key = claude_key(root);
    let v: serde_json::Value = match fs::read_to_string(home.join(CLAUDE_CONFIG)) {
        Ok(t) => match serde_json::from_str(&t) { Ok(v) => v, Err(e) => return vec![format!("Trust: {key} unknown (Claude config unreadable: {e})")] },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
        Err(e) => return vec![format!("Trust: {key} unknown (Claude config unreadable: {e})")],
    };
    // Claude walks parent folders: a trusted parent covers the root
    if root.ancestors().any(|p| v["projects"][claude_key(p)]["hasTrustDialogAccepted"] == true) { return vec![]; }
    vec![format!("Trust: {key} not trusted; Claude workers cannot start here"), trust_step(&key)]
}

/// Onboarding trust (#1546): `<install>/setup` and every registered root get `hasTrustDialogAccepted` in
/// `<home>/.claude.json`. Nothing trusted yet changes only while no Claude runs, after a backup; gap lines only.
pub fn trust(install: &Path, home: &Path, claude_pids: impl FnOnce() -> Res<Vec<String>>) -> Res<Vec<String>> {
    let roots = fs::read_to_string(install.join("projects")).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(String::new()) } else { Err(format!("project registry unreadable: {e}")) })?;
    let keys: Vec<String> = std::iter::once(dir(install)).chain(roots.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from)).map(|p| claude_key(&p)).collect();
    let path = home.join(CLAUDE_CONFIG);
    let text = match fs::read_to_string(&path) { Ok(t) => Some(t), Err(e) if e.kind() == std::io::ErrorKind::NotFound => None, Err(e) => return Err(format!("Claude config unreadable: {e}")) };
    let (next, changed) = trusted(text.as_deref(), &keys)?;
    let mut lines = vec![];
    if changed.is_empty() { return Ok(lines); }
    // Claude rewrites the whole file: a running one would drop the change, so the owner lets Claude write it (#1555)
    let pids = claude_pids().map_err(|e| format!("Claude processes unknown ({e}); trust unchanged"))?;
    if !pids.is_empty() {
        return Ok(changed.iter().flat_map(|k| [format!("Trust: {k} not trusted; Claude runs (pid {}), so onboard leaves the file", pids.join(", ")), trust_step(k)]).collect());
    }
    if let Some(t) = &text {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let backup = home.join(format!(".claude.json.bak-onboard-{ms}"));
        // copy keeps the config's owner-only mode; a plain write would leave it world-readable
        fs::copy(&path, &backup).map_err(|e| format!("Claude config backup: {e}; trust unchanged"))?;
        lines.push(format!("Backup: {}", backup.display()));
    }
    let tmp = home.join(".claude.json.onboard-tmp");
    let mut open = fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    if text.is_some() { fs::copy(&path, &tmp).map_err(|e| format!("Claude config write: {e}"))?; }
    std::io::Write::write_all(&mut open.open(&tmp).map_err(|e| format!("Claude config write: {e}"))?, next.as_bytes()).map_err(|e| format!("Claude config write: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Claude config write: {e}"))?;
    let (_, left) = trusted(fs::read_to_string(&path).ok().as_deref(), &keys)?;
    if !left.is_empty() { return Err(format!("Claude config readback: {} not trusted", left.join(", "))); }
    lines.extend(changed.iter().map(|k| format!("Trust: {k} trusted")));
    Ok(lines)
}

/// The config with every key trusted, and the keys that were not.
pub(crate) fn trusted(text: Option<&str>, keys: &[String]) -> Res<(String, Vec<String>)> {
    let mut v: serde_json::Value = match text { Some(t) => serde_json::from_str(t).map_err(|e| format!("Claude config unreadable: {e}"))?, None => serde_json::json!({}) };
    let projects = v.as_object_mut().ok_or("Claude config is not an object")?.entry("projects").or_insert_with(|| serde_json::json!({}));
    let projects = projects.as_object_mut().ok_or("Claude config projects is not an object")?;
    let mut changed = vec![];
    for k in keys {
        let p = projects.entry(k.clone()).or_insert_with(|| serde_json::json!({})).as_object_mut().ok_or(format!("Claude config project {k} is not an object"))?;
        if p.get("hasTrustDialogAccepted") != Some(&serde_json::Value::Bool(true)) {
            p.insert("hasTrustDialogAccepted".into(), true.into());
            changed.push(k.clone());
        }
    }
    Ok((serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?, changed))
}

/// Running Claude processes of this user's machine; an unreadable list is an error, never none (R12).
pub fn claude_pids() -> Res<Vec<String>> {
    let out = if cfg!(windows) {
        std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq claude.exe", "/FO", "CSV", "/NH"]).output()
    } else {
        // ponytail: process name `claude` (native installs); an npm `node` Claude is missed, match the command line if one shows up
        std::process::Command::new("pgrep").args(["-x", super::of(super::OWNER).bin]).output()
    }.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    if cfg!(windows) {
        if !out.status.success() { return Err(format!("tasklist exit {:?}", out.status.code())); }
        return Ok(text.lines().filter_map(|l| l.split("\",\"").nth(1).filter(|_| l.starts_with("\"claude.exe\"")).map(String::from)).collect());
    }
    match out.status.code() { Some(0) => Ok(text.split_whitespace().map(String::from).collect()), Some(1) => Ok(vec![]), c => Err(format!("pgrep exit {c:?}")) }
}

/// Claude's config file in the user's home: project trust lives there.
pub const CLAUDE_CONFIG: &str = ".claude.json";
pub(crate) const MISSING: &str = "runtime missing:";
pub(crate) const LOGIN: &str = "sign-in needed:";
pub(crate) const ACCESS: &str = "access refused:";
