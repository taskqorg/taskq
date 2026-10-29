//! The `taskq` command line (taskq.md § 5). Every write is one `taskq:` comment that `fold` accepts.

/// One service log line (§ 4): stamped and folded in `service run`, plain on the CLI.
macro_rules! log {
    ($($t:tt)*) => { $crate::platform::log(&format!($($t)*)) };
}

mod admission;
#[cfg(any(target_os="macos",windows,test))]
mod agentq;
mod host;
mod host_control;
mod board;
mod core;
mod control;
mod data;
mod dispatch;
mod display;
mod enrollment;
mod incident;
mod labels;
mod recipe;
mod requests;
mod platform;
mod service_versions;
mod policy;
mod resources;
mod pm;
mod lifecycle;
mod recovery;
#[cfg(target_os = "linux")]
mod linux_sandbox;
#[cfg(windows)]
mod windows_sandbox;
mod profile;
mod telemetry;
mod telemetry_sync;
mod tidy;
mod runtime;
mod service;
mod setup;
mod dialogue;
mod transcript;
mod messages;
mod conversations;
mod snapshot;
#[cfg(any(target_os = "macos", windows))]
mod icon;
#[cfg(any(target_os = "macos", windows))]
mod notify;
#[cfg(any(target_os = "macos", windows))]
mod tray;
#[cfg(any(target_os = "macos", windows, test))]
mod pending;

use board::{Board, entries};
use core::{Entry, State, Task, entry, fold};
use dispatch::{Op, Receipt, act, answer_picks, close_checked, current, deps_open};
use serde_json::{Value, json};
use service::{Config, Service};
use service::Kid;
use snapshot::in_scope;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub type Res<T> = Result<T, String>;

const USAGE: &str = "taskq agents
 taskq labels
 taskq add <title> --goal G --acceptance A [--acceptance B..] [--scope P..] [--deps N..] [--type T] [--runtime R] [--priority 0|1|2] [--host H]
 taskq finding <title> --board URL --source installation/rule/episode --goal G --acceptance A.. [--evidence E..] [--start] [--json]
 taskq start N [M..]
 taskq ask N --text Q --option O.. --recommend K [--link URL..]
 taskq answer N --text A | taskq answer N.K [M.K..] | taskq pick N.K --text R
 taskq result N --sha SHA --checks C.. --text T [--option O.. --recommend K --link URL..]
 taskq requeue N [--text T] | taskq later N [--text T] [--blocker #M|--until UTC]
 taskq steer N --text T | taskq failed N --text T
 taskq close N [M..] [--text VERDICT] | taskq approve N [--sha SHA]
 taskq recipe NAME --run ID [--goal G] [--start]
 taskq status [--only L,..] [--json] | taskq status --closed [--page N] [--since T] [--task N [--transcript [--before B]]] | taskq inbox N
 taskq profile [guide|export|tasks|board|actions|health|migrate] [--since 24h] | taskq retro
 taskq wait [--window MIN] [--only L,..]
 taskq cleanup [--force] | taskq update [--check]
 taskq enroll <https-url> [--headless] [--telemetry] | taskq connection status | taskq disconnect
 taskq service install|run|pause|resume|status|restart|stop|uninstall|list
 taskq service setup [--runtime claude|codex] [--client C] [--target PATH..] [--json]
 taskq service onboard
 taskq service stop [--mode drain|full] (macOS)
 taskq service focus [all|off|<label..>] [--request TEXT]
 taskq service capacity [--host H] [--max N|none] [--claude N] [--codex N] [--codex-app N] [--load-gate on|off] [--cpu-max P] [--memory-min P] [--claim any|labelled] [--default-runtime claude|codex]
 taskq service permissions [--runtime R --mode restricted|workspace|full|none [--scope host|project]]
 taskq service resources [--grant ID --actor ID --hours N [--project P].. [--runtime R].. [--host-policy] [--project-limits] | --revoke ID]
 taskq service pm [--grant ID --actor ID --hours N --project P.. --runtime R.. | --revoke ID] [--json]
 taskq tray [--open | --snapshot DIR [--route R,..]] | taskq selftest
Writes to a task take [--op-id ID] [--revision R] [--json] (taskq.md section 5).
Text templates and limits: taskq.md sections 1, 2, 5, 7.";

// ---------- arguments ----------

#[derive(Default)]
struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

impl Args {
    fn parse(v: &[String]) -> Res<Args> {
        let mut a = Args::default();
        let mut it = v.iter();
        while let Some(x) = it.next() {
            match x.strip_prefix("--") {
                Some(k) => {
                    let val = if matches!(k, "later" | "hold" | "start" | "check" | "force" | "help" | "json" | "apply" | "headless" | "telemetry" | "host-policy" | "project-limits" | "closed" | "open" | "transcript") || k=="host"&&a.pos==["service","uninstall"] { String::new() } else { it.next().cloned().ok_or(format!("--{k} needs a value"))? };
                    a.flags.entry(k.to_string()).or_default().push(val);
                }
                None => a.pos.push(x.clone()),
            }
        }
        let used = match a.pos.first().map(String::as_str) {
            Some("close" | "start") => a.pos.len(),
            Some("answer") if a.pos.get(1).is_some_and(|p| p.contains('.')) => a.pos.len(),
            Some("service") if a.pos.get(1).is_some_and(|p| p == "focus") => a.pos.len(),
            Some("service") if a.pos.get(1).is_some_and(|p| p == "channel") => 3,
            Some("rules") => 6,
            Some("service" | "profile" | "add" | "finding" | "approve" | "recipe" | "ask" | "answer" | "pick" | "result" | "requeue" | "later" | "failed" | "steer" | "inbox" | "enroll" | "connection") => 2,
            _ => 1,
        };
        if a.pos.len() > used {
            return Err(format!("unused positional arguments: {}", a.pos[used..].join(" ")));
        }
        Ok(a)
    }
    fn one(&self, k: &str) -> Option<&str> {
        self.flags.get(k).and_then(|v| v.last()).map(String::as_str)
    }
    fn need(&self, k: &str) -> Res<&str> {
        self.one(k).filter(|s| !s.trim().is_empty()).ok_or(format!("--{k} is required"))
    }
    fn all(&self, k: &str) -> Vec<String> {
        self.flags.get(k).cloned().unwrap_or_default()
    }
    fn num(&self, i: usize) -> Res<u64> {
        let s = self.pos.get(i).ok_or("task number missing")?;
        s.trim_start_matches('#').parse().map_err(|_| format!("not a task number: {s}"))
    }
}

// ---------- config ----------

struct Ctx {
    cfg: Config,
    raw: Value,
    board: Box<dyn Board>,
}

thread_local!(static PROJECT_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) });

fn find_root() -> Res<PathBuf> {
    if let Some(root) = PROJECT_ROOT.with_borrow(Clone::clone) { return Ok(root); }
    root_for(std::env::var_os("TASKQ_ROOT").map(PathBuf::from), std::env::current_dir().map_err(|e| e.to_string())?)
}

/// `TASKQ_ROOT` wins only when it contains `cwd` or `cwd` is in no project: a reused session's stale value is refused (§ 5).
fn root_for(env: Option<PathBuf>, cwd: PathBuf) -> Res<PathBuf> {
    if let Some(r) = env {
        let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let (rc, dc) = (canon(&r), canon(&cwd));
        if dc.starts_with(&rc) || dc.ancestors().all(|a| !a.join("taskq.json").is_file()) {
            return Ok(r);
        }
        return Err(format!("TASKQ_ROOT {} is not the project of the current directory {}", r.display(), cwd.display()));
    }
    let mut d = cwd;
    loop {
        let in_worktree = d.parent().and_then(Path::file_name).is_some_and(|n| n == ".worktrees");
        if d.join("taskq.json").is_file() && !in_worktree {
            return Ok(d);
        }
        if !d.pop() {
            return Err("no taskq.json here or above".into());
        }
    }
}

fn hostname() -> String {
    // ponytail: first nonempty name per process (#1297: was one `hostname` child per call); a renamed host needs a service restart.
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(name) = NAME.get() { return name.clone(); }
    let name = coreq::process::output("host.identity", &mut std::process::Command::new("hostname"), std::time::Duration::from_secs(10)).ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if !name.is_empty() { let _ = NAME.set(name.clone()); }
    name
}

fn load(root: &Path) -> Res<(Config, Value)> {
    let (mut cfg,raw)=project_config(root)?;
    if let Ok(policy)=host::Policy::read(&cfg.install) {policy.apply(&mut cfg);}
    Ok((cfg,raw))
}

fn project_config(root:&Path)->Res<(Config,Value)> {
    let p = root.join("taskq.json");
    let v: Value = serde_json::from_str(&fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?;
    let str_or = |k: &str, d: &str| v[k].as_str().unwrap_or(d).to_string();
    let secs = |k: &str, d: u64| v[k].as_u64().unwrap_or(d);
    let h = hostname();
    let me = std::env::var("TASKQ_HOST").ok().filter(|s| !s.is_empty()).or_else(|| v["hosts"][&h].as_str().map(String::from)).unwrap_or(h);
    let has = v["has"][&me].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
    let limits: BTreeMap<String, u32> = v["limits"].as_object().into_iter().flatten().map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0) as u32)).collect();
    let codex = match v[runtime::CODEX_ARGS].as_array() {
        Some(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        None => vec!["-s".into(), "danger-full-access".into()],
    };
    let board = board::kind(&str_or("board", "github"))?;
    let (publish, delivery, delivery_revision) = delivery(&v)?;
    let cfg = Config {
        root: root.to_path_buf(),
        install: platform::default_install(),
        board,
        web_host: str_or("host", board.default_host),
        repo: str_or("repo", ""),
        publish,
        delivery,
        delivery_revision,
        me,
        limits,
        has,
        labelled: false,
        default_runtime: crate::runtime::OWNER.into(),
        permission_mode: str_or("permission_mode", "dontAsk"),
        codex,
        ttl: secs("ttl", TTL as u64) as i64,
        poll: secs("poll", 60),
        update_every: secs("update_every", 60),
        rollback_after: secs("rollback_after", 600),
        external: v["workspace"].as_str() == Some("external"),
    };
    if cfg.repo.is_empty() {
        return Err("taskq.json: repo is required".into());
    }
    profile::project(&cfg.repo);
    profile::host(&cfg.me);
    Ok((cfg, v))
}

/// § 7 delivery: `(publish mode, preset, revision)`. Absent, `publish` names the preset at revision 0; a preset
/// never changes by itself, so `publish: pr` stays `auto-merge`.
fn delivery(v: &Value) -> Res<(String, String, u64)> {
    let publish = v["publish"].as_str();
    if v["delivery"].is_null() {
        return Ok(match publish.unwrap_or("pr") {
            "direct" => ("direct".into(), "direct".into(), 0),
            _ => ("pr".into(), "auto-merge".into(), 0),
        });
    }
    let d = &v["delivery"];
    let revision = d["revision"].as_u64().ok_or("taskq.json: delivery.revision must be a whole number")?;
    let preset = d["preset"].as_str().unwrap_or_default();
    let mode = match preset {
        "auto-merge" | "review-pr" => "pr",
        "direct" => "direct",
        // ponytail: B3 pending-handoff states come first; until then these presets cannot finish a task
        "local" | "push-branch" => return Err(format!("taskq.json: delivery.preset {preset} is not available yet: a result needs a published SHA")),
        _ => return Err(format!("taskq.json: delivery.preset {preset:?}: use auto-merge, review-pr or direct")),
    };
    if publish.is_some_and(|p| p != mode) {
        return Err(format!("taskq.json: publish {} disagrees with delivery.preset {preset}", publish.unwrap_or_default()));
    }
    Ok((mode.into(), preset.into(), revision))
}

fn ctx() -> Res<Ctx> {
    let (cfg, raw) = load(&find_root()?)?;
    let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
    Ok(Ctx { cfg, raw, board })
}

fn dot(root: &Path) -> PathBuf {
    root.join(".taskq")
}

fn poke(root: &Path) -> Res<()> {
    poke_locked(&wake_lock(root)?)
}

fn wake_lock(root: &Path) -> Res<coreq::file_lock::Lock> {
    coreq::telemetry::capture::operation("wake.lock", || {
        fs::create_dir_all(dot(root)).map_err(|error| ("wake-directory", error))?;
        let file = fs::OpenOptions::new().write(true).create(true).truncate(false).open(dot(root).join("poke")).map_err(|error| ("wake-open", error))?;
        coreq::file_lock::try_lock_file(file).map_err(|error| ("wake-lock", std::io::Error::from(error)))
    }, |(class, _)| *class).map_err(|(class, error)| format!("service wake failed ({class}: {error}); check service status"))
}

#[tracing::instrument(name="poke",skip_all)]
fn poke_locked(locked: &coreq::file_lock::Lock) -> Res<()> {
    use std::io::Write;
    coreq::telemetry::capture::operation("wake.write", || {
        let context = coreq::telemetry::capture::TraceContext::current().map(|c| c.encode()).unwrap_or_else(|| ".".into());
        let mut file = locked.file();
        file.set_len(0).map_err(|error| ("wake-truncate", error))?;
        file.write_all(context.as_bytes()).map_err(|error| ("wake-write", error))
    }, |(class, _)| *class).map_err(|(class, error)| format!("service wake failed ({class}: {error}); state may already be saved; check service status"))
}

#[tracing::instrument(name="control.read",skip_all)]
fn wake_read<T>(path: &Path, seen: &mut Option<String>, read: impl FnOnce() -> Res<T>) -> Res<(Option<coreq::telemetry::capture::TraceContext>, T)> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => { *seen = None; return read().map(|state| (None, state)); }
        Err(_) => return Err("wakeup context unreadable".into()),
    };
    let locked = coreq::file_lock::try_lock_shared_file(file).map_err(|_| "wakeup context busy")?;
    let mut text = String::new();
    locked.file().take(129).read_to_string(&mut text).map_err(|_| "wakeup context unreadable")?;
    let link = if seen.as_ref() == Some(&text) || matches!(text.as_str(), "" | ".") { None } else { Some(coreq::telemetry::capture::TraceContext::decode(&text)?) };
    let state = read()?;
    *seen = Some(text);
    Ok((link, state))
}

#[cfg(test)]
fn wake_context(path: &Path, seen: &mut Option<String>) -> Res<Option<coreq::telemetry::capture::TraceContext>> {
    wake_read(path, seen, || Ok(())).map(|(link, ())| link)
}

fn change_focus<T>(root: &Path, restrictive: bool, initialize: bool, change: impl FnOnce(&Path) -> Res<T>) -> Res<(T,String)> {
    let receipt = coreq::telemetry::capture::CommandReceipt::current();
    let received = receipt.as_ref().map_err(Clone::clone).and_then(|r| telemetry::receipt(r, "received"));
    if !restrictive { if let Err(error) = &received { return Err(format!("command not applied: receipt unavailable ({error}); check service status")); } }
    // Retain the settings lock through terminal commit, including partial failures.
    let mut locked = None;
    let mut tracking = None;
    let mut tracking_error = None;
    let result = (|| {
        if initialize { fs::create_dir_all(dot(root)).map_err(|e| e.to_string())?; }
        fs::metadata(dot(root)).map_err(|e| format!("focus unchanged: state directory unavailable: {e}"))?;
        locked = Some(wake_lock(root)?);
        let lock = locked.as_ref().unwrap();
        let prepared=received.as_ref().map_err(Clone::clone)
            .and_then(|_|receipt.as_ref().map_err(Clone::clone)).and_then(|r|control::begin(root,r));
        match prepared {
            Ok(pending)=>tracking=Some(pending),
            Err(error) if restrictive=>tracking_error=Some(error),
            Err(error)=>return Err(format!("command not applied: service tracking unavailable ({error})")),
        }
        // A failed mutation must never leave its new state attributed to the preceding signal.
        lock.file().set_len(0).map_err(|e| format!("focus unchanged: cannot invalidate previous signal: {e}"))?;
        let result = change(&dot(root));
        let notified = poke_locked(lock);
        match (result, notified) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(format!("{error}; focus may be partially saved; check service status")),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(wake)) => Err(format!("{error}; {wake}")),
        }
    })();
    let terminal = receipt.and_then(|r| telemetry::receipt(&r, if result.is_ok() { "applied" } else { "failed" }));
    if terminal.is_ok() {if let Some(pending)=tracking {
        if let Err(error)=control::finish(root,pending,result.is_ok()) {tracking_error=Some(error);}
    }}
    let uncertain = received.err().or_else(|| terminal.as_ref().err().cloned()).map(|e|format!("durable receipt unknown ({e})"))
        .into_iter().chain(tracking_error.map(|e|format!("service tracking unknown ({e})"))).collect::<Vec<_>>().join("; ");
    let uncertain=(!uncertain.is_empty()).then_some(uncertain);
    match (result, uncertain) {
        (Ok(value), None) => {
            let applied=terminal.unwrap();
            println!("Command applied locally; service observation pending; receipt {applied}");
            Ok((value,applied))
        }
        (Ok(_), Some(error)) => Err(format!("command applied locally; {error}; check service status before retry")),
        (Err(error), None) => Err(format!("{error}; failure receipt {}", terminal.unwrap())),
        (Err(error), Some(receipt)) => Err(format!("{error}; {receipt}; check service status before retry")),
    }
}

/// Bump `p`'s mtime. Not `fs::write(p, "")`: on Windows rewriting an empty file keeps its mtime.
fn touch(p: &Path) {
    let _ = fs::write(p, ".");
}

fn project_name(root: &Path) -> String {
    root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "taskq".into())
}

use coreq::status::age as file_age;
/// Default project ttl (§ 9): heartbeat lease and the one freshness bound (#1591).
const TTL: i64 = 300;

// ---------- writes ----------

fn numbered(opts: &[String], recommend: Option<usize>) -> String {
    opts.iter().enumerate().map(|(i, o)| format!("{}. {o}{}", i + 1, if recommend == Some(i + 1) { " ★" } else { "" })).collect::<Vec<_>>().join("\n")
}

fn join(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.trim().is_empty()).copied().collect::<Vec<_>>().join("\n\n")
}

// ---------- message formats (R7, § 1, § 2 ask card, § 7 verdicts) ----------

/// R7: Unicode characters of a line; link targets and `code` spans (exact commands, SHAs, paths) are exempt.
fn width(l: &str) -> usize {
    let mut s = String::new();
    let mut rest = l;
    while let Some(a) = rest.find('`') {
        s.push_str(&rest[..a]);
        match rest[a + 1..].find('`') {
            Some(b) => rest = &rest[a + b + 2..],
            None => { rest = &rest[a..]; break; }
        }
    }
    s.push_str(rest);
    while let Some(a) = s.find("](") {
        let b = s[a..].find(')').map_or(s.len(), |b| a + b + 1);
        s.replace_range(a + 1..b, "");
    }
    s.split_inclusive(char::is_whitespace).filter(|w| !w.starts_with("http://") && !w.starts_with("https://")).map(|w| w.chars().count()).sum()
}

/// Rejects, with a one-line hint, a field outside `min..=max` lines or with a line over `chars` (R7: never cut).
fn limit(what: &str, lines: &[String], min: usize, max: usize, chars: usize) -> Res<()> {
    match lines.len() {
        n if n > 0 && max == 0 => Err(format!("{what}: not allowed here (only with --option)")),
        n if n < min || n > max => Err(format!("{what}: {n} lines, allowed {min}-{max}; shorten or split")),
        _ => match lines.iter().map(|l| width(l)).enumerate().find(|(_, w)| *w > chars) {
            Some((i, w)) => Err(format!("{what} line {}: {w} characters, limit {chars}; shorten it", i + 1)),
            None => Ok(()),
        },
    }
}

/// All violations of `checks` in one error, `; `-joined, so one retry fixes them (#1346).
fn all(checks: impl IntoIterator<Item = Res<()>>) -> Res<()> {
    let errs: Vec<String> = checks.into_iter().filter_map(Result::err).collect();
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

fn one_line(what: &str, items: &[String]) -> Res<()> {
    if items.iter().any(|s| s.contains(['\r', '\n']) || s.trim().is_empty()) { Err(format!("{what}: one nonempty line each")) } else { Ok(()) }
}

/// § 1: `add` numbers Acceptance itself; a typed `A<n>.` label would double (#1696).
fn labelled_acceptance(items: &[String]) -> Res<()> {
    let bad = |s: &str| s.trim_start().strip_prefix('A').and_then(|r| r.split_once('.')).is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    match items.iter().position(|s| bad(s)) {
        Some(i) => Err(format!("--acceptance {}: drop the A<n>. label; add numbers the lines", i + 1)),
        None => Ok(()),
    }
}

/// The `later` arguments (§ 7 rule 3): a worker park names one blocker issue or, when only time is left, a UTC time;
/// only the owner or PM parks without either.
fn park<'a>(n: u64, blocker: Option<&'a str>, until: Option<&'a str>, task: Option<String>, attempt: Option<String>) -> Res<Vec<&'a str>> {
    let worker = attempt.is_some() && task.is_some_and(|t| t.parse() == Ok(n));
    match (blocker, until) {
        (Some(_), Some(_)) => return Err("--blocker and --until exclude each other: a park waits for one of them".into()),
        (None, Some(u)) if !core::utc(u) => return Err(format!("bad --until {u}: one UTC time like 2026-11-05T09:00:00Z")),
        (None, Some(u)) => return Ok(vec!["until", u]),
        _ => {}
    }
    match blocker {
        None if worker => Err("a worker park needs --blocker #M (file the blocker as a held task first) or, when only time is left, --until <UTC> (§ 7)".into()),
        None => Ok(vec![]),
        Some(_) if !worker => Err("--blocker is for a worker park; the owner or PM adds `Depends on #M` or parks with later".into()),
        Some(r) if !core::blocker_ref(r) || r == format!("#{n}") => Err(format!("bad --blocker {r}: one #M or <repo>#M, not this task")),
        Some(r) => Ok(vec![r]),
    }
}

fn task_text(cmd: &str, text: &str) -> Res<()> {
    if matches!(cmd, "requeue" | "later" | "failed") {
        if text.is_empty() && cmd != "failed" { return Ok(()); }
        let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
        all([limit(cmd, &lines, 1, 2, 240), fields(text, &[("Reason", 1, 1, 240), ("Next", 0, 1, 240)]).map(drop)])?;
    } else {
        let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
        limit(cmd, &lines, 1, snapshot::ANSWER_LINES, snapshot::ANSWER_CHARS)?;
    }
    Ok(())
}

/// Labelled `--text` lines (`Label: ..`; an unlabelled line continues the field above) in `spec` order:
/// (label, min lines, max lines, characters).
fn fields(text: &str, spec: &[(&str, usize, usize, usize)]) -> Res<Vec<Vec<String>>> {
    let names = spec.iter().map(|s| format!("`{}:`", s.0)).collect::<Vec<_>>().join(", ");
    let mut out = vec![vec![]; spec.len()];
    let mut cur: Option<usize> = None;
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match spec.iter().position(|s| l.strip_prefix(s.0).is_some_and(|r| r.starts_with(": "))) {
            Some(i) if cur.is_none_or(|c| i > c || (i == c && spec[i].0 == "Checks")) => cur = Some(i),
            Some(_) => return Err(format!("--text fields go once each in the order {names}")),
            None if cur.is_none() => return Err(format!("--text starts with one of {names}")),
            None => {}
        }
        out[cur.unwrap()].push(l.to_string());
    }
    all(out.iter().zip(spec).map(|(f, s)| limit(s.0, f, s.1, s.2, s.3)))?;
    Ok(out)
}

/// `--link`: `[label](url)` as given, else the URL labelled `#N` by its trailing number, else `link`.
fn evidence(l: &str) -> String {
    if l.starts_with('[') && l.contains("](") || l.starts_with("![") {
        return l.to_string();
    }
    let tail = l.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    format!("[{}]({l})", if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) { format!("#{tail}") } else { "link".into() })
}

/// § 2 ask card: a Problem or option line holds at most one `[label](url)` with a label of at most 40 characters, no bare URL.
fn card_links(what: &str, lines: &[String]) -> Res<()> {
    all(lines.iter().enumerate().map(|(i, l)| {
        let n = l.matches("](").count();
        let long = l.split("](").take(n).any(|a| a.rsplit('[').next().is_none_or(|t| t.chars().count() > 40));
        let bare = l.split_whitespace().any(|w| w.starts_with("http://") || w.starts_with("https://"));
        match () {
            _ if n > 1 => Err(format!("{what} line {}: {n} links, at most one [label](url)", i + 1)),
            _ if long => Err(format!("{what} line {}: link label at most 40 characters", i + 1)),
            _ if bare => Err(format!("{what} line {}: bare URL; write [label](url)", i + 1)),
            _ => Ok(()),
        }
    }))
}

/// § 2 ask card: an option is `<label> - + <gain>; - <cost>`; the label names the meaning in at most 32 characters, unique.
fn option_lines(opts: &[String]) -> Res<()> {
    let label = |o: &str| o.trim().strip_prefix("- ").map_or_else(|| o.split_once(" - ").map_or(o.trim(), |p| p.0.trim()).to_string(), |_| String::new());
    all(opts.iter().enumerate().map(|(i, o)| {
        let (k, l) = (i + 1, label(o));
        let detail = o.trim().get(l.len()..).unwrap_or_default().trim_start().strip_prefix("- ").unwrap_or_default();
        let pair = detail.strip_prefix("+ ").and_then(|d| d.split_once("; - ")).is_some_and(|(g, c)| !g.trim().is_empty() && !c.trim().is_empty());
        let twin = opts[..i].iter().position(|p| label(p).to_lowercase() == l.to_lowercase());
        if l.is_empty() {
            Err(format!("option line {k}: missing label; write `<label> - + <gain>; - <cost>`"))
        } else if width(&l) > 32 {
            Err(format!("option line {k}: label {} characters, limit 32", width(&l)))
        } else if let Some(j) = twin {
            Err(format!("option line {k}: label repeats option line {}", j + 1))
        } else if !pair {
            Err(format!("option line {k}: detail must be `+ <gain>; - <cost>` after `{l} - `"))
        } else {
            Ok(())
        }
    }))
}

/// § 2 visual evidence: `--image` files, at most four images of at most 10 MB each.
fn image_files(paths: &[String]) -> Res<()> {
    if paths.len() > 4 { return Err("Evidence: at most four --image".into()); }
    all(paths.iter().map(|p| {
        let head = std::fs::File::open(p).and_then(|f| { let mut b = vec![]; std::io::Read::read_to_end(&mut std::io::Read::take(f, 16), &mut b).map(|_| b) }).map_err(|e| format!("--image {p}: {e}"))?;
        board::image_kind(&head).ok_or(format!("--image {p}: not a PNG, JPEG, GIF or WebP file"))?;
        let size = std::fs::metadata(p).map_err(|e| format!("--image {p}: {e}"))?.len();
        if size > 10 << 20 { return Err(format!("--image {p}: {size} bytes, at most 10 MB")); }
        Ok(())
    }))
}

/// § 2 visual evidence: uploads each `--image` to the board and embeds it as `![name](absolute URL)` only after the board
/// serves the same bytes back (the load check); a failed check publishes nothing.
fn upload_images(b: &dyn board::Board, n: u64, paths: &[String]) -> Res<Vec<String>> {
    image_files(paths)?;
    paths.iter().map(|p| {
        let url = b.upload_image(n, Path::new(p))?;
        let sent = std::fs::read(p).map_err(|e| format!("--image {p}: {e}"))?;
        let back = b.image(&url).map_err(|e| format!("image load check failed for {url}: {e}"))?;
        if back != sent { return Err(format!("image load check failed: {url} serves {} bytes, {} uploaded", back.len(), sent.len())); }
        let name: String = Path::new(p).file_stem().map(|s| s.to_string_lossy().replace([']', '[', ',', ')'], " ")).unwrap_or_default().chars().take(30).collect();
        Ok(format!("![{}]({url})", if name.trim().is_empty() { "image".into() } else { name }))
    }).collect()
}

/// § 2 ask card: Done, Problem, optional Evidence, 2-4 options with one ★ or none (an owner action), the answer line.
fn ask_card(n: u64, text: &str, opts: &[String], recommend: Option<&str>, links: &[String]) -> Res<String> {
    let f = fields(text, &[("Done", 1, 1, 160), ("Problem", 1, 2, 240)])?;
    all([card_links("Problem", &f[1]), card_links("option", opts), option_lines(opts)])?;
    let mut lines = f.concat();
    one_line("--option", opts)?;
    let action = opts.is_empty() && recommend.is_none();
    if !action && !(2..=4).contains(&opts.len()) || opts.iter().any(|o| o.contains('★')) {
        return Err(format!("{} options, allowed 2-4 without ★ (none: an owner action); --recommend adds it", opts.len()));
    }
    let k = if action { 0 } else {
        recommend.and_then(|k| k.parse().ok()).filter(|k| (1..=opts.len()).contains(k)).ok_or(format!("--recommend needs one option number 1-{}", opts.len()))?
    };
    one_line("--link", links)?;
    if links.iter().filter(|l| !l.starts_with("![")).count() > 2 {
        return Err("Evidence: at most two --link".into());
    }
    let ev: Vec<String> = links.iter().filter(|l| !l.starts_with("![")).map(|l| evidence(l)).collect();
    if ev.iter().any(|e| e.split_once("](").is_some_and(|(l, _)| l.chars().count() > 41)) {
        return Err("Evidence: link labels at most 40 characters".into());
    }
    // § 2: a link must open from the board and every app; a relative path renders as a broken repository link (#1782)
    if let Some(e) = ev.iter().find(|e| !e.split_once("](").is_some_and(|(_, u)| u.starts_with("https://") || u.starts_with("http://"))) {
        return Err(format!("Evidence: {e} needs an absolute http(s) URL"));
    }
    // visual choice (answer card § Question types): one image per option names image K `K. <name>`
    let images: Vec<&String> = links.iter().filter(|l| l.starts_with("![")).collect();
    let each = images.len() >= 2 && images.len() == opts.len();
    let ev = [ev, images.iter().enumerate().map(|(i, l)| if each { format!("![{}. {}", i + 1, &l[2..]) } else { l.to_string() }).collect()].concat();
    if !ev.is_empty() {
        lines.push(format!("Evidence: {}", ev.join(", ")));
    }
    if action {
        lines.push(format!("To answer: `taskq answer {n} --text <done>`."));
        return Ok(lines.join("\n"));
    }
    let opts: Vec<String> = numbered(opts, Some(k)).lines().map(String::from).collect();
    limit("option", &opts, 2, 4, 160)?;
    lines.extend(opts);
    lines.push(format!("To answer: `taskq answer {n}.K` or comment `K` or the option text."));
    Ok(lines.join("\n"))
}

/// § 7 result: Changed, one `Checks:` line per check, Open; a decision appends the § 2 card (Done/Problem in `--text`).
fn result_text(n: u64, text: &str, checks: &[String], opts: &[String], recommend: Option<&str>, links: &[String]) -> Res<String> {
    let d = !opts.is_empty() as usize;
    if d == 0 && (recommend.is_some() || links.iter().any(|l| !l.starts_with("!["))) {
        return Err("--recommend and --link need --option".into());
    }
    let f = fields(text, &[("Changed", 1, 2, 240), ("Open", 1, 2, 240), ("Done", d, d, 160), ("Problem", d, 2 * d, 240)])?;
    let checks: Vec<String> = checks.iter().flat_map(|c| c.lines()).map(str::trim).filter(|c| !c.is_empty()).map(|c| format!("Checks: {}", c.trim_start_matches("Checks: "))).collect();
    limit("Checks", &checks, 1, 8, 240)?;
    let verdict = [f[0].clone(), checks, f[1].clone()].concat();
    limit("result", &verdict, 3, 12, 240)?;
    let card = if d == 1 { ask_card(n, &[f[2].clone(), f[3].clone()].concat().join("\n"), opts, recommend, links)? } else if links.is_empty() { String::new() } else { format!("Evidence: {}", links.join(", ")) };
    Ok(join(&[&verdict.join("\n"), &card]))
}

/// § 7 rule 4 shots gate: a result whose diff from `origin/main` changes a `uiSources` file of `docs/shots/manifest.json`
/// needs the manifest to record that file's blob at the SHA. Only a readable candidate without the manifest passes unchecked;
/// any other Git or manifest failure reads unknown and refuses (R12).
fn shots_gate(git_bin: &str, root: &Path, sha: &str) -> Res<()> {
    let unknown = |e: String| format!("shots check unknown: {e}");
    let git = |args: &[&str]| {
        let o = match coreq::process::output("shots.git", std::process::Command::new(git_bin).arg("-C").arg(root).args(args), std::time::Duration::from_secs(30)) {
            Ok(o) | Err(coreq::process::Failure::Exit(o)) => o,
            Err(e) => return Err(unknown(format!("{git_bin}: {e}"))),
        };
        if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()) } else { Err(unknown(String::from_utf8_lossy(&o.stderr).trim().to_string())) }
    };
    // `ls-tree` succeeds with empty output for an absent path and fails for an unreadable tree-ish.
    let blob = |f: &str| git(&["ls-tree", "--object-only", &format!("{sha}^{{commit}}"), "--", f]);
    if blob("docs/shots/manifest.json")?.is_empty() { return Ok(()) }
    let m: Value = serde_json::from_str(&git(&["show", &format!("{sha}:docs/shots/manifest.json")])?).map_err(|e| unknown(format!("docs/shots/manifest.json: {e}")))?;
    let ui = m["uiSources"].as_object().ok_or_else(|| unknown("docs/shots/manifest.json lacks uiSources".into()))?;
    let mut stale = vec![];
    // `--no-renames` lists a moved source under its old path too, so a pure rename still reads stale.
    for f in git(&["diff", "--no-renames", "--name-only", &format!("origin/main...{sha}")])?.lines() {
        if let Some(b) = ui.get(f) && blob(f)? != b.as_str().unwrap_or_default() { stale.push(f.to_string()) }
    }
    if stale.is_empty() { Ok(()) } else { Err(format!("stale README shots: {} changed without a manifest update; run docs/shots/update.sh and commit docs/shots/", stale.join(", "))) }
}

/// § 7 close verdict: Changed, Checks naming every Acceptance item `A<k> ok|gap|unknown` and CI, Open.
/// § 7: deferred publication accepts merge-result CI; synchronous routes name the result SHA.
fn close_check(text: &str, issue: &board::Issue, t: &Task, deferred: bool) -> Res<()> {
    if t.state != State::Review || text.is_empty() {
        return Ok(());
    }
    close_verdict(text, &issue.body)?;
    if deferred && text.lines().any(|l| l.starts_with("Checks:") && l.contains("CI") && (l.contains("merge-result") || l.contains("merge_group"))) {
        return Ok(());
    }
    if t.result.as_deref().is_some_and(|sha| text.lines().any(|l| l.starts_with("Checks:") && l.contains(sha))) { Ok(()) } else { Err("Checks: name the exact result SHA with the CI outcome".into()) }
}

fn close_verdict(text: &str, body: &str) -> Res<()> {
    let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
    limit("verdict", &lines, 3, 12, 240)?;
    let f = fields(text, &[("Changed", 1, 2, 240), ("Checks", 1, 8, 240), ("Open", 1, 2, 240)])?;
    limit("verdict", &f.concat(), 3, 12, 240)?;
    let checks = f[1].join(" ");
    for k in body.lines().filter_map(|l| l.trim().strip_prefix('A')?.split_once('.')?.0.parse::<u32>().ok()) {
        if !["ok", "gap", "unknown"].iter().any(|v| checks.contains(&format!("A{k} {v}"))) {
            return Err(format!("Checks: name A{k} ok|gap|unknown"));
        }
    }
    if checks.contains("CI") { Ok(()) } else { Err("Checks: name the route's CI outcome".into()) }
}

/// Selftest of the rendered forms: a result, a close verdict, and an ask card answered by the hand-typed form (§ 2).
fn formats_selftest() -> Res<String> {
    let checks = ["cargo test --locked -- pass".to_string()];
    result_text(1, "Changed: x\nOpen: none", &checks, &[], None, &[])?;
    close_verdict("Changed: x\nChecks: A1 ok; CI green on the SHA\nOpen: none", "A1. y")?;
    let card = ask_card(1, "Done: not started\nProblem: pick one", &["Left - + fast; - ugly".into(), "Right - + clean; - slow".into()], Some("2"), &[])?;
    let log: Vec<Entry> = [entry("spawn", &["worker", &format!("{}:s", runtime::OWNER), "mac"], None, ""), entry("ask", &[], Some(1), &card), "taskq answer 2".into()]
        .into_iter().enumerate().map(|(i, body)| Entry { id: i as u64 + 1, body, edited: false }).collect();
    match (fold(1, &log, false).state, core::delivery(1, &log, 3)) {
        (State::Doing, d) if d == "Right - + clean; - slow" => Ok("formats: ask card, result, close verdict PASS".into()),
        (s, d) => Err(format!("formats: FAIL hand-typed answer gave {} and {d:?}", s.name())),
    }
}

// ---------- report ----------

pub fn cell(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|").replace('[', "\\[").replace(']', "\\]").replace(['\r', '\n'], " ")
}

/// R6: compact display only; the issue link retains the complete stored text.
pub fn compact(s: &str, max: usize) -> String {
    let s = s.replace(['\r', '\n'], " ");
    if s.chars().count() > max { s.chars().take(max - 1).chain(['…']).collect() } else { s }
}

pub use display::age;

#[cfg(test)]
fn report(b: &dyn Board, cfg: &Config, raw: &Value, only: &[String]) -> Res<String> {
    snapshot::read(b, cfg, raw, only).text(cfg, raw)
}

fn inbox(b: &dyn Board, n: u64) -> Res<String> {
    let issue = b.issue(n)?;
    let log = entries(b, n)?;
    let projection = core::project(n, &log, deps_open(b, n, &issue.body, &log));
    let t = &projection.task;
    let mut out = vec![format!("#{n} {}\n{}", issue.title, b.url(n)), issue.body.trim().to_string(), format!("Folded: {}", serde_json::to_string(&t).unwrap_or_default())];
    out.push(format!("Revision: {}", projection.revision));
    if let Some((k, why)) = &t.pick {
        // § 2: the worker's star and the PM's pick, side by side
        let star = core::options(t.card.as_deref().unwrap_or("")).into_iter().find(|o| o.2).map_or("none".into(), |o| format!("{n}.{}", o.0));
        out.push(format!("Recommendation: worker ★ {star} · PM {n}.{k}: {why}"));
    }
    out.push("Log:".into());
    for e in log {
        out.push(format!("[{}{}] {}", e.id, if e.edited { " edited, ignored" } else { "" }, e.body.trim()));
    }
    Ok(out.join("\n\n"))
}

/// MP14 Completed: one board page of closed tasks with their sourced outcome, result line, last PR link and merge receipt; no store.
fn closed_history(b: &dyn Board, page: u32, since: Option<i64>, gitlab: bool) -> Res<Value> {
    let (issues, more) = b.closed_tasks(page, since)?;
    let tasks: Vec<Value> = issues.iter().filter(|(_, closed)| since.is_none_or(|t| *closed >= t))
        .map(|(i, closed)| closed_row(b, i, Some(*closed), gitlab)).collect();
    Ok(json!({"schema": 1, "source": "board", "page": page.max(1), "more": more, "tasks": tasks}))
}

/// P-W1: exact pin reads share Completed's outcome projection, never depend on its loaded page.
fn closed_row(b: &dyn Board, i: &board::Issue, closed: Option<i64>, gitlab: bool) -> Value {
    let log = entries(b, i.number);
    // § 1: GitHub records why an issue closed; GitLab records no reason, so a closed GitLab issue is done
    let outcome = match (i.reason.as_deref(), gitlab) { (Some("completed"), _) | (None, true) => "success", (Some("not_planned"), _) => "not_planned", _ => "unknown" };
    let (result, pr) = log.as_ref().map_or((Value::Null, Value::Null), |log| {
        let t = fold(i.number, log, false);
        let pr = log.iter().flat_map(|e| e.body.split_whitespace()).filter(|w| w.starts_with("https://") && (w.contains("/pull/") || w.contains("/merge_requests/"))).last();
        (json!(t.publication.as_deref().and_then(|p| p.lines().next()).map(|l| l.trim_start_matches("Changed:").trim())), json!(pr.map(|u| u.trim_end_matches(|c: char| !c.is_ascii_alphanumeric()))))
    });
    let merged = log.as_ref().ok().and_then(|log| merge_receipt(log));
    // § 7 close outcome (#1800): delivered, no change or replaced by #M with its current state, plus unverified items
    let verdict = log.as_ref().ok().and_then(|log| log.iter().rev().find(|e| !e.edited && e.body.starts_with("taskq: close")).map(|e| e.body.split_once('\n').map_or("", |x| x.1)));
    let next = verdict.and_then(display::successor).map(|m| dispatch::current(b, m).map_or("unknown", |(i, t, _)| if i.open { t.state.name() } else { "closed" }));
    let close = log.is_ok().then(|| display::close_outcome(verdict, next));
    json!({"n": i.number, "title": i.title, "url": b.url(i.number), "state": if i.open { "open" } else { "closed" }, "closed": closed, "outcome": outcome, "close": close, "result": result, "pr": pr, "merged": merged, "error": log.err()})
}

fn closed_pin(b: &dyn Board, n: u64, gitlab: bool) -> Res<Value> {
    let i = b.issue(n)?;
    if i.number != n || !service::is_task(&i) { return Err("Pinned issue is not the requested trusted task".into()); }
    Ok(json!({"schema":1,"source":"board","more":false,"tasks":[closed_row(b,&i,i.closed,gitlab)]}))
}

/// Read only: use the last accepted worker's native identity, only on its recorded host.
fn closed_transcript(b: &dyn Board, n: u64, host: &str, before: Option<u64>) -> Res<Value> {
    let i = b.issue(n)?;
    if i.number != n || i.open || !service::is_task(&i) { return Err("Transcript requires a closed trusted task; refresh the project".into()); }
    let log = entries(b,n)?;
    let projection = core::project(n,&log,false);
    let t = projection.changes.iter().rev().map(|(_,_,t)|t).find(|t| t.role.as_deref()==Some("worker") && t.session.is_some()).ok_or("No recorded worker transcript")?;
    let (native,owner) = t.session.as_deref().unwrap_or_default().rsplit_once(' ').ok_or("Worker identity unavailable")?;
    if owner != host { return Err(format!("Transcript is on host {owner}; open it there")); }
    let (runtime,session) = native.split_once(':').ok_or("Worker session unavailable")?;
    let path = runtime::owner::transcript(runtime,session)?;
    let page = transcript::before(runtime::of(runtime).codex,&path,before)?;
    Ok(json!({"items":page.items,"start":page.start,"end":page.end,"size":page.size}))
}

/// #1679: the PR/MR merge `close` read back (§ 7 `delivery <preset> r<R>: merged #P ..`, GitLab `merged !P`); none stays unknown.
/// Issue closure, a PR link or a direct fast-forward is no merge evidence.
fn merge_receipt(log: &[Entry]) -> Option<String> {
    log.iter().rev().filter(|e| !e.edited && e.body.lines().next().is_some_and(|h| h.trim() == "taskq: close" || h.trim().starts_with("taskq: close attempt ")))
        .flat_map(|e| e.body.lines().filter_map(|l| l.trim().strip_prefix("delivery ")?.split_once(": ").map(|(_, r)| r.trim())))
        .find(|r| r.starts_with("merged #") || r.starts_with("merged !")).map(String::from)
}

// ---------- wait ----------

/// PM wait (§ 5): the cursor `<root>/.taskq/pm-cursor` holds the board time of the last read and the last seen state
/// per task. Only moves into `ask`, `review` with options, closed or `failed` are printed.
fn wait(b: &dyn Board, cfg: &Config, window_min: u64, only: &[String]) -> Res<Vec<String>> {
    let path = dot(&cfg.root).join("pm-cursor");
    let mut since = None;
    let mut seen: HashMap<u64, String> = HashMap::new();
    if let Ok(text) = fs::read_to_string(&path) {
        let mut lines = text.lines();
        since = lines.next().and_then(|l| l.parse::<i64>().ok());
        seen = lines.filter_map(|l| l.split_once(' ').and_then(|(n, s)| Some((n.parse().ok()?, s.to_string())))).collect();
    }
    let deadline = Instant::now() + Duration::from_secs(window_min * 60);
    loop {
        let now = b.now()?;
        let issues = b.issues(since.map(|s| s - 1).or(Some(now)))?;
        let open: Vec<u64> = b.issues(None)?.iter().map(|i| i.number).collect();
        let mut moved = vec![];
        for i in issues.iter().filter(|i| service::is_task(i) && in_scope(i, only)) {
            let t = fold(i.number, &entries(b, i.number)?, core::depends(&i.body).iter().any(|d| d.repo.is_none() && open.contains(&d.n)));
            let state = if !i.open { "closed".to_string() } else if t.state == State::Review && t.card.is_some() { "review*".into() } else { t.state.name().into() };
            if seen.get(&i.number) != Some(&state) && matches!(state.as_str(), "ask" | "review*" | "closed" | "failed") {
                moved.push(format!("{} #{}", state.trim_end_matches('*'), i.number));
            }
            seen.insert(i.number, state);
        }
        since = Some(now);
        let mut text = format!("{now}\n");
        let mut keep: Vec<_> = seen.iter().filter(|(_, s)| *s != "closed").collect();
        keep.sort();
        for (n, s) in keep {
            text += &format!("{n} {s}\n");
        }
        fs::create_dir_all(dot(&cfg.root)).map_err(|e| e.to_string())?;
        platform::write_atomic(&path, &text)?;
        if !moved.is_empty() {
            return Ok(moved);
        }
        if Instant::now() >= deadline {
            return Ok(vec!["tick".into()]);
        }
        std::thread::sleep(Duration::from_secs(cfg.poll.clamp(5, 60)).min(deadline.saturating_duration_since(Instant::now()) + Duration::from_secs(1)));
    }
}

// ---------- service ----------

fn installed(install: &Path) -> bool {
    std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).zip(install.join("releases").canonicalize().ok()).is_some_and(|(e, r)| e.starts_with(r))
}

fn launcher(install: &Path) -> PathBuf {
    let l = install.join(platform::tool(install).exe_name());
    if l.is_file() { l } else { std::env::current_exe().unwrap_or_else(|_| PathBuf::from("taskq")) }
}

fn poke_times(paths: &[PathBuf; 2]) -> [Option<SystemTime>; 2] {
    paths.each_ref().map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Compare with the pre-pass snapshot: a command arriving during work must not become the sleep baseline.
/// Returns true when a poke (any local command) ended the wait.
fn sleep_until_poke(paths: &[PathBuf; 2], start: [Option<SystemTime>; 2], until: Instant, tick: &mut dyn FnMut()) -> bool {
    loop {
        let left=until.saturating_duration_since(Instant::now());
        if poke_times(paths) != start {
            return true;
        }
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1).min(left));
        tick();
    }
}

/// § 4 step 8 (#1819): the board is read at most this many seconds apart while a tray window is open.
const MENU_POLL: u64 = 15;

/// A tray window touches `<install>/menu-open` every tick while open.
fn menu_open(install: &Path) -> bool {
    file_age(&install.join("menu-open")).is_some_and(|a| a < 30)
}

/// The tray asks the next pass to reread `task` after its write (#1819), then wakes the service.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn ask_reread(root: &Path, task: u64) -> Res<()> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(dot(root).join("reread")).map_err(|e| format!("reread: {e}"))?;
    writeln!(f, "{task}").map_err(|e| format!("reread: {e}"))?;
    poke(root)
}

/// Tasks the tray wrote to (`<root>/.taskq/reread`, one number per line), taken once: renamed first so a concurrent append is kept for the next pass.
fn take_reread(dot: &Path) -> Vec<u64> {
    let taken = dot.join("reread.taken");
    if fs::rename(dot.join("reread"), &taken).is_err() { return vec![]; }
    let tasks = fs::read_to_string(&taken).unwrap_or_default().lines().filter_map(|l| l.trim().parse().ok()).collect();
    let _ = fs::remove_file(&taken);
    tasks
}

/// § 4 step 8 (#1431): an idle pass waits 3×`poll`; board-only changes may show up to 2×`poll` later.
fn poll_wait(poll: u64, complete: bool, children: bool, poked: bool, snapshot: Option<&snapshot::Snapshot>) -> u64 {
    let active = snapshot.is_none_or(|s| s.reason.is_some() || s.tasks.iter().any(|r| matches!(r.task.state, State::Ready | State::Doing | State::Review)));
    if !complete || children || poked || active { poll } else { poll * 3 }
}

/// Read-only recovery evidence; failed identity reads and a still-live foreign owner prevent takeover.
fn kids_path(install:&Path,root:&Path)->Res<PathBuf> {
    let root=root.canonicalize().map_err(|e|format!("native recovery root unavailable: {e}"))?;
    let root=root.to_str().ok_or("native recovery root is not UTF-8")?;
    let hash:String=ring::digest::digest(&ring::digest::SHA256,root.as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect();
    Ok(install.join("native").join(format!("{hash}.kids")))
}

fn kids_file(install:&Path,root: &Path) -> Res<Vec<data::KidLine>> {
    use std::io::Read;
    let path = kids_path(install,root)?;
    let file = match fs::File::open(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return match fs::symlink_metadata(dot(root).join("kids")) {
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(vec![]),
                _=>Err("legacy project kids record requires native reconciliation; file kept".into()),
            };
        },
        Err(e) => return Err(format!("kids: unreadable ({e}); file kept")),
        Ok(file) => file,
    };
    let mut text=String::new();
    file.take(data::KIDS_MAX+1).read_to_string(&mut text).map_err(|e|format!("kids: unreadable ({e}); file kept"))?;
    let (owner,kids)=data::kids(&text)?;
    let current=coreq::process::identity(std::process::id()).map_err(|e|format!("service identity unknown: {e}"))?;
    if current.as_ref()!=Some(&owner) && coreq::process::identity(owner.pid).map_err(|e|format!("kids: previous service identity unknown: {e}; file kept"))?.as_ref()==Some(&owner) {
        return Err("kids: previous service still alive; file kept, native reconciliation required".into());
    }
    Ok(kids)
}

/// A process that started before this boot cannot still run; its board attempt recovers like any lost heartbeat.
fn pre_boot(k:&data::KidLine)->bool {
    platform::started_before_boot(&k.process.started).unwrap_or(false) && recorded(k).0=="ended"
}

/// #1594 the verdict on a recorded native process: its identity read now against the record.
fn recorded(k:&data::KidLine)->(&'static str,String) {
    let now=coreq::process::identity(k.process.pid).map_err(|e|e.to_string());
    snapshot::verdict(&snapshot::Signals {local:true,process:Some((&k.process,now)),..Default::default()})
}

/// Power loss (#1445): drop a torn `.pending` and retire kids from a previous boot before adoption.
fn retire_pre_boot(install:&Path,root:&Path)->Res<usize> {
    // kids_file first: it refuses while a previous service still owns the record.
    let mut kids=kids_file(install,root)?;
    let path=kids_path(install,root)?;
    let mut name=path.file_name().unwrap_or_default().to_os_string();
    name.push(".pending");
    match fs::remove_file(path.with_file_name(name)) {
        Err(e) if e.kind()!=std::io::ErrorKind::NotFound=>return Err(format!("kids: stale pending file kept ({e})")),
        _=>{},
    }
    let before=kids.len();
    kids.retain(|k|!pre_boot(k));
    let retired=before-kids.len();
    if retired>0 { write_kids(install,root,&kids)?; }
    Ok(retired)
}

fn adopt(s: &mut Service, install:&Path, root: &Path) -> Res<()> {
    let retired=retire_pre_boot(install,root)?;
    if retired>0 { platform::log(&format!("{}: retired {retired} kid(s) started before this boot", root.display())); }
    let kids = kids_file(install,root)?;
    for k in kids {
        if let Some(owned)=s.kids.iter().find(|owned|owned.task==k.task||owned.execution==k.execution) {
            if owned.task==k.task&&owned.attempt==k.attempt&&owned.execution==k.execution&&owned.rt==k.runtime
                &&owned.proc.native_identity()?==k.process {continue;}
            return Err("native recovery differs from the retained handle; file kept".into());
        }
        let history=board::entries(s.board,k.task)?;
        let execution=core::executions(k.task,&history).into_iter().find(|x|x.id==k.execution && x.attempt==k.attempt && x.host==s.cfg.me && x.runtime==k.runtime)
            .ok_or("native recovery identity does not match board ownership; file kept")?;
        match execution.release {
            Some(core::Release::Exit)=>{
                if runtime::released_execution_live(&k).map_err(|e|format!("released native scope unknown: {e}"))? {
                    return Err("board exit contradicts a live native identity; file kept for reconciliation".into());
                }
                retire_kid(&s.cfg,k.task,k.attempt,k.execution)?;
                continue;
            },
            Some(core::Release::NoStart)=>return Err("board no-start contradicts a recorded native process; file kept".into()),
            None=>{},
        }
        let proc=runtime::adopt(root,install,&k,&execution)?;
        s.adopt(Kid { unknown:None, stopping:false, session_url:None, execution:k.execution,task:k.task,attempt:k.attempt,rt:k.runtime,proc })?;
    }
    Ok(())
}

fn service_run(c: &Ctx, host: Option<host::Shared>) -> Res<()> {
    let cfg = &c.cfg;
    let exe = launcher(&cfg.install);
    let board_env = cfg.board.worker_env(&platform::tool(&cfg.install), &cfg.web_host)?;
    let env = runtime::Env { board_env: &board_env, root: &cfg.root, exe: &exe, install: &cfg.install, permission_mode: &cfg.permission_mode, codex: &cfg.codex, publish: &cfg.delivery, jobs: None };
    let start = |l: &runtime::Launch| {
        if let Some(host) = &host {
            let h = host.lock().map_err(|_| runtime::StartError::admission("host admission poisoned"))?;
            if let Some(reason)=&h.held {return Err(runtime::StartError::admission(reason.clone()));}
            let p = h.policy.clone().map_err(runtime::StartError::admission)?;
            let jobs = runtime::compile_jobs(std::thread::available_parallelism().ok().map(|n| n.get()), h.occupied());
            drop(h);
            let env = runtime::Env { permission_mode: &p.permission_mode, codex: &p.codex, jobs, ..env };
            runtime::spawn(&env, l)
        } else { runtime::spawn(&env, l) }
    };
    let mut s = Service::new(c.board.as_ref(), cfg, platform::VERSION, Box::new(start));
    s.recover = Some(Box::new(|task, execution| runtime::recover(&cfg.root, &cfg.install, task, execution)));
    s.host = host.clone();
    s.instance = enrollment::installation_id(&cfg.install).map(|id| service_id(&id))?;
    // § 4 machine line: agentq's `host.name` is the same `hostname` output; WSL as agentq detects it
    let wsl = cfg!(target_os = "linux") && (std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists());
    s.machine = Some(hostname()).filter(|h| !h.is_empty()).map(|h| if wsl { format!("{h} wsl") } else { h }).unwrap_or_default();
    let d = dot(&cfg.root);
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    // § 4 one service per root: wait, not exit, so the unit sees no crash loop
    loop {
        let other = if host.is_some() { host::project_service(&cfg.root) } else { Ok(other_service(&cfg.root)) };
        match other {
            Ok(None) => break,
            Ok(Some(pid)) => log!("service pid {pid} already runs this project; waiting"),
            Err(e) => log!("{e}; waiting"),
        }
        std::thread::sleep(Duration::from_secs(cfg.poll.max(1)));
    }
    adopt(&mut s, &cfg.install, &cfg.root)?;
    let is_installed = installed(&cfg.install);
    let began = Instant::now();
    log!("{}", incident::disclosure(Some(&cfg.root)));
    log!("{}", runtime::build_cache_line());
    let mut checked: Option<Instant> = None;
    let mut update: Option<String> = None;
    let mut restart_empty: Option<u64> = None;
    let mut waits_logged = String::new();
    let _ = fs::remove_file(d.join("restart"));
    let mut tray = None;
    let project = project_name(&cfg.root);
    let issues = c.board.url(0).strip_suffix("/0").unwrap_or_default().to_string();
    let log = coreq::service::log_path(&platform::tool(&cfg.install), &project);
    log!("taskq {} service on {} for {}", platform::VERSION, cfg.me, cfg.repo);
    let mut sampled: Option<Instant> = None;
    let mut agents: Option<(Instant, Value)> = None;
    let mut dev_ci: Option<(String, Instant)> = None;
    let mut focus_error: Option<String> = None;
    let mut pruned: Option<Instant> = None;
    // § 4 board budget: the pause logged last, and the cumulative counts at the last full hour
    let since = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut paused_logged: Option<i64> = None;
    // the budget alarm is logged once per reset
    let mut low_logged: Option<Option<i64>> = None;
    let mut hour: (Instant, BTreeMap<String, u64>) = (Instant::now(), BTreeMap::new());
    let mut last_hour: Option<BTreeMap<String, u64>> = None;
    let mut dashq = enrollment::Sender::default();
    let mut telemetry_sync = telemetry_sync::Sender::default();
    let mut policy_exchange = resources::Exchanger::default();
    let mut pm_exchange = pm::Exchanger::default();
    let mut panics = host::Panics::default();
    let pokes = [d.join("poke"), cfg.install.join("poke")];
    let mut observed_poke = None;
    let mut observed_command = None;
    let mut poked = false;
    loop {
        // #1297: steady successful work folds into 15-minute aggregates; failures, changes and command-linked passes stay rows.
        let _repeating = tracing::info_span!("service.loop", coreq_repeating = true, project = cfg.repo.as_str()).entered();
        let observe=observes(host.as_ref(),&cfg.root)?;
        let pass_began=Instant::now();
        let poke_start = poke_times(&pokes);
        telemetry::diagnostic::poll(&cfg.install);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Res<()> {
        let control = control::snapshot(&cfg.root,&mut observed_poke,&mut observed_command);
        let command=match &control {Ok(snapshot)=>snapshot.command.clone(),Err(e)=>json!({"state":"unknown","reason":e})};
        let wakeup = control.as_ref().ok().and_then(|s|s.link.as_ref()).map(|link| link.span().entered());
        let reduced = panics.quarantined() || began.elapsed().as_secs() < panics.until;
        // #1766: an observe pass reads and reports only; exchanges, probes and pruning wait for the next ordinary pass
        let quiet = reduced || observe;
        // § 3: capacity reloads before each admission decision; children are untouched
        let cap = fs::read_to_string(cfg.root.join("taskq.json")).map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| format!("taskq.json: {e}")))
            .and_then(|v| match &host {
                Some(h) => {
                    let h = h.lock().map_err(|_| "host admission poisoned; launches held")?;
                    let p = h.policy.as_ref().map_err(Clone::clone)?;
                    p.apply(&mut s.cfg);
                    let cap=p.project(&cfg.root,&v)?;
                    s.cfg.limits=p.project_limits(&cfg.root,&v)?;
                    s.project_revision=p.project_revision(&cfg.root,&v).ok();
                    Ok(cap)
                }
                None => service::capacity(&v, &cfg.me),
            });
        if let Err(e) = &cap {
            log!("capacity: {e}; launches held");
        }
        s.reload(cap);
        if let Some(host) = &host { if let Ok(h) = host.lock() { s.gate = h.gate.clone(); } }
        // § 8 Access: re-read every pass; a refused runtime is not claimed or resumed
        let access = policy::report(&cfg.install, &cfg.root, &s.cfg.permission_mode, &s.cfg.codex);
        s.refused_access = access.iter().filter_map(|e| Some((e.runtime.clone(), e.refused.clone()?))).collect();
        // #1393: a runtime whose CLI does not resolve admits nothing instead of failing each spawn with os error 2
        for rt in runtime::agent_runtimes() {
            if let Some(r) = runtime::missing_cli(rt) { s.refused_access.entry(rt.to_string()).or_insert(r); }
        }
        s.full = access.iter().filter(|e| e.mode == "full" && e.refused.is_none()).map(|e| e.runtime.clone()).collect();
        if s.gui_checked.is_none_or(|t: Instant| t.elapsed().as_secs() >= 300) {
            s.gui_checked = Some(Instant::now());
            s.gui_probe = platform::gui_probe();
            s.gui_stale = platform::stale_grant(&cfg.install, &s.gui_probe);
        }
        if !quiet { if let Some(m) = policy::pin_rollback(&cfg.install, platform::VERSION) { log!("{m}"); } }
        s.mono = host.as_ref().and_then(|h| h.lock().ok().map(|h| h.elapsed())).unwrap_or_else(|| began.elapsed().as_secs());
        s.wall = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        // § 4 Data: an unreadable focus admits nothing and reads unknown with its reason
        let focus = control.map(|snapshot|snapshot.focus);
        if focus.as_ref().err() != focus_error.as_ref() {
            if let Err(e) = &focus { log!("{e}; nothing admitted"); }
            focus_error = focus.as_ref().err().cloned();
        }
        s.focus = focus.unwrap_or_default();
        if let Some(host) = &host {
            if let Ok(h) = host.lock() { if h.draining.contains(&cfg.root) { s.focus.clear(); } }
        }
        s.focus_error = focus_error.clone();
        // #1819: a tray write rereads its task at once: no cached comments, no unchanged-board shortcut
        let reread = take_reread(&d);
        if !reread.is_empty() { s.board.forget(); reread.into_iter().for_each(|t| s.reread(t)); }
        let r = if reduced { s.reduced().and_then(|_| Err(panics.reason.clone().unwrap_or_default())) } else if observe { s.observe() } else { s.pass() };
        s.sync_host();
        for l in s.log.drain(..) {
            log!("{l}");
        }
        let budget = c.board.requests();
        let requests_json = match &host {
            Some(h) => match h.lock() {
                Ok(h) => budget.json(h.request_since, h.last_hour.as_ref(), &h.hour_start),
                Err(_) => json!({"reason": "host request accounting unknown: admission poisoned"}),
            },
            None => budget.json(since, last_hour.as_ref(), &hour.1),
        };
        let paused = budget.paused_until();
        if paused != paused_logged {
            match paused {
                Some(t) => log!("{}", requests::paused_text(t)),
                None => log!("rate limit: board requests resumed"),
            }
            paused_logged = paused;
        }
        // while paused every pass fails the same way: the pause line above is the one report
        if let (Err(e), None) = (&r, paused) {
            log!("pass: {e}");
        }
        if budget.low() && low_logged != Some(budget.reset) {
            let (top, n) = budget.top(&hour.1).unwrap_or(("none".into(), 0));
            log!("board budget low: {}/{}{} until {}; top here: {top} {n}", budget.remaining.unwrap_or(0), budget.limit.unwrap_or(0),
                budget.resource.as_deref().map(|r| format!(" {r}")).unwrap_or_default(), budget.reset.map_or("unknown".into(), board::stamp));
            low_logged = Some(budget.reset);
        }
        if host.is_none() && hour.0.elapsed().as_secs() >= 3600 {
            let h = requests::diff(&budget.ops, &hour.1);
            log!("requests 1h: {}", requests::summary(&h, ""));
            requests::record("service", &h);
            last_hour = Some(h);
            hour = (Instant::now(), budget.ops.clone());
        }
        let at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let kids: Vec<Value> = s.kids.iter().map(|k| {
            let unknown = k.unknown.as_ref();
            let child = snapshot::Child { unknown: unknown.map(String::as_str), ..Default::default() };
            let live = if snapshot::verdict(&snapshot::Signals { local: true, child: Some(child), ..Default::default() }).0 == "unknown" { "unknown" } else { "running" };
            json!({"task": k.task, "attempt": k.attempt, "rt": k.rt, "pid": k.proc.id(), "live": live, "reason": unknown})
        }).collect();
        let request = fs::read_to_string(d.join("focus.request")).ok();
        // § 4: runtime probes at most every 5 minutes
        if !quiet && agents.as_ref().is_none_or(|(t, _)| t.elapsed().as_secs() >= 300) {
            mark_checking(&platform::tool(&cfg.install), &cfg.root, at);
            agents = Some((Instant::now(), json!({"at": at, "rows": runtime::agents(&cfg.install)})));
            // #1429: a logged-out runtime admits nothing until a probe reads it logged in
            s.logged_out = agents.as_ref().and_then(|a| a.1["rows"].as_array().cloned()).unwrap_or_default().iter()
                .filter(|r| r["login"] == "not logged in").filter_map(|r| r["runtime"].as_str().map(String::from)).collect();
        }
        // docs/design/enrollment.md: delivery runs on its own thread; its failure never fails or delays the pass
        let local = enrollment::Local { host: cfg.me.clone(), service: Some(s.instance.clone()), ttl: cfg.ttl.max(0) as u64, agents: agents.as_ref().map_or(Value::Null, |a| a.1.clone()),
            progress: s.snapshot.as_ref().map(|x| enrollment::progress(x, &cfg.root, &cfg.me)).unwrap_or_default() };
        if !quiet { if let Some(l) = dashq.tick(&cfg.install, s.snapshot.as_ref(), &local, at) { log!("{l}"); } }
        // docs/design/standalone-telemetry.md#remote-sync-1353: only under the telemetry grant, on its own thread
        if !quiet { if let Some(l) = telemetry_sync.tick(&cfg.install, s.snapshot.as_ref(), at) { log!("{l}"); } }
        // docs/design/shared-resource-controls.md § Host adapter: the same speaker exchanges resource operations
        if !quiet { for l in policy_exchange.tick(&cfg.install, s.snapshot.as_ref(), at) { log!("{l}"); } }
        if !quiet { for l in pm_exchange.tick(&cfg.install, s.snapshot.as_ref(), at) { log!("{l}"); } }
        // § 8 item 1: only the Mac thread of the taskq checkout on channel dev; merge_group CI read at most every 5 minutes per SHA
        let mut dev = Value::Null;
        #[cfg(target_os = "macos")]
        if !quiet && is_installed && platform::dev_enabled(&cfg.repo, &cfg.install) {
            dev = platform::dev_tick(&cfg.install, &cfg.root, &mut |sha| {
                if dev_ci.as_ref().is_some_and(|(s, t)| s == sha && t.elapsed().as_secs() < 300) { return Err("checked".into()); }
                dev_ci = Some((sha.into(), Instant::now()));
                c.board.merge_ci_green(sha)
            });
        }
        let tool = platform::tool(&cfg.install);
        let error = r.as_ref().err().cloned();
        let mut status = coreq::status::common(&tool, &cfg.me, &project, at, r.err(), update.as_deref());
        // § 4 snapshot keys; before the first successful pass tasks are unknown, not an empty list
        match &s.snapshot {
            Some(snap) => for (k, v) in snap.status_keys(error.as_deref()) { status[k] = v; },
            None => { status["complete"] = json!(false); status["reason"] = json!(error.as_deref().unwrap_or("no board read yet")); status["tasks"] = json!([]); }
        }
        for (k, v) in [("focus", if focus_error.is_some() { Value::Null } else { json!(s.focus) }), ("focus_error", json!(focus_error)), ("focus_command",command), ("kids", json!(kids)), ("issues", json!(issues)), ("log", json!(log)),
            ("tasks_age_seconds", json!(s.tasks_read.map(|t| t.elapsed().as_secs()))), ("request", json!(request)), ("events_since", json!(s.events_since)),
            ("gui", json!({"listed": s.gui().is_ok(), "reason": s.gui().err(), "stale_grant": s.gui_stale})), ("waits", json!(s.waits.iter().map(|(n, reason)| json!({"task": n, "reason": reason})).collect::<Vec<_>>())),
            ("execution_waits", json!(s.execution_waits)), ("capacity", s.capacity_json()), ("hold", json!(s.hold)), ("access", json!(access.iter().map(|e| e.json()).collect::<Vec<_>>())), ("agents", json!(agents.as_ref().map(|a| &a.1))),
            ("stop_progress",json!(s.stop_progress)), ("poll", json!(cfg.poll)),
            ("restart_waits", json!((d.join("restart").exists()||host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.restart))).then(|| lifecycle_waits(&s, host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.stopping)))))),
            ("incident", incident::status(&cfg.root)), ("incident_notice", json!(incident::notice(&cfg.root))),
            ("requests", requests_json), ("channel", json!(platform::channel(&cfg.install).ok())), ("dev", dev), ("connection", json!(dashq.outcome)), ("telemetry_sync", telemetry_sync.status.clone()), ("telemetry",telemetry::diagnostic::health()),
            ("tidy", fs::read_to_string(d.join("tidy.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null))]
        {
            status[k] = v;
        }
        status["diagnostics"] = s.service_diagnostics();
        let published = publish_status(&tool, &cfg.root, &status, host.as_ref());
        drop(wakeup);
        if host.is_none() && is_installed && published && began.elapsed().as_secs() >= cfg.rollback_after {
            let h = cfg.install.join("releases").join(platform::VERSION).join("healthy");
            if !h.exists() {
                if let Err(e) = fs::write(h, "") { log!("release health unknown: {e}"); }
            }
        }
        // § 4 step 7: a not installed service only reports a newer version; `<root>/.taskq/update` checks at once
        let asked = fs::remove_file(d.join("update")).is_ok();
        // § 8 step 3: a pending restart already goes to `current`; repeating the check would only repeat its log line
        let pending = is_installed && d.join("restart").exists();
        if host.is_some() && (asked || pending) { touch(&cfg.install.join(if asked { "update" } else { "restart" })); }
        if host.is_none() && !pending && (asked || checked.is_none_or(|t| t.elapsed().as_secs() >= cfg.update_every)) {
            checked = Some(Instant::now());
            if is_installed {
                match platform::service_update(&cfg.install) {
                    Ok(Some(_)) => {
                        update = None;
                        reexec(&s, cfg)?;
                    }
                    Ok(None) => update = None,
                    Err(e) => log!("update: {e}"),
                }
            } else {
                match platform::update(&cfg.install, true) {
                    Ok(found) => update = found,
                    Err(e) => log!("update: {e}"),
                }
            }
            if asked {
                log!("update requested: {}", update.as_deref().map_or("up to date".into(), |v| format!("{v} found")));
            }
        }
        restart_watch(&mut restart_empty, cfg!(windows) && d.join("restart").exists() && s.kids.is_empty(), began.elapsed().as_secs(), cfg.update_every);
        if host.is_none() && is_installed && d.join("restart").exists() {
            // Windows re-exec waits for the live children; the request stays until it can go
            let waits = lifecycle_waits(&s, false).join(", ");
            if waits.is_empty() {
                let _ = fs::remove_file(d.join("restart"));
                log!("restart requested");
            } else if waits != waits_logged {
                log!("restart to {} waits for {waits}", platform::tool(&cfg.install).current().unwrap_or_default());
            }
            waits_logged = waits;
            reexec(&s, cfg)?;
        }
        // § 4 Data: the pruners run hourly
        if !quiet && pruned.is_none_or(|t| t.elapsed().as_secs() >= 3600) {
            pruned = Some(Instant::now());
            let live = s.retained_attempts();
            for l in data::prune(&cfg.root, &cfg.install, &project, &live) {
                log!("{l}");
            }
        }
        if host.is_none() && is_installed {
            keep_tray(&cfg.install, &mut tray);
        }
        // an observe pass keeps the project's last handover report (#1402)
        if let Some(host) = host.as_ref().filter(|_| !observe) {
            if let Ok(mut h) = host.lock() {
            let registered = !h.draining.contains(&cfg.root);
            let drained = drained(&s, &h);
            if !registered && drained { h.sync(&cfg.root, vec![]); h.ready(&cfg.root, false); return Err("project drained".into()); }
            if registered { h.busy.insert(cfg.root.clone(), !drained); }
            if let Err(e) = report_handoff(&s, &mut h, &cfg.root, drained) { log!("handoff unknown: {e}"); }
            }
        }
        // #1357: a failed handover retries at `poll`; its reduced pass may list the board for unknown ownership.
        let wait = if reduced {1} else {poll_wait(cfg.poll, error.is_none(), !s.idle(), poked, s.snapshot.as_ref())};
        // every 30 s, also between board polls: a gate sample (when enabled) and a fresh capacity in status.json (§ 4)
        let mut tick = || {
            if sampled.is_some_and(|t| t.elapsed().as_secs() < 30) {
                return;
            }
            sampled = Some(Instant::now());
            s.mono = began.elapsed().as_secs();
            let wall = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            if let Some(host) = &host {
                if let Ok(h) = host.lock() { s.gate = h.gate.clone(); s.mono = h.elapsed(); }
            } else if s.gate.enabled() {
                s.gate.host_mem = platform::host_memory();
                s.gate.observe(s.mono, wall as i64, platform::cpu_ticks(), platform::memory_available());
            }
            s.wall = wall as i64;
            s.sync_host();
            status["tasks_age_seconds"] = json!(s.tasks_read.map(|t| t.elapsed().as_secs()));
            status["capacity"] = s.capacity_json();
            status["at"] = json!(wall);
            publish_status(&tool, &cfg.root, &status, host.as_ref());
        };
        tick();
        let until = pass_began+Duration::from_secs(wait);
        let short = pass_began+Duration::from_secs(MENU_POLL.min(wait));
        poked = loop {
            let end = if menu_open(&cfg.install) { short } else { until.min(Instant::now()+Duration::from_secs(MENU_POLL)) };
            if sleep_until_poke(&pokes, poke_start, end, &mut tick) { break true; }
            if Instant::now() >= until || menu_open(&cfg.install) { break false; }
        };
        Ok(())
        }));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(payload) => {
                if host.is_none() { std::panic::resume_unwind(payload); }
                if panics.quarantined()&&let Some(host)=&host {
                    // Stop the board loop, retaining its handles solely for safe exit/update monitoring (R11).
                    s.unready();
                    loop {
                        let monitored = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stopped_handoff(&mut s, host, cfg)));
                        if matches!(monitored, Ok(Ok(true))) { return Err("project drained".into()); }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                let msg = payload.downcast_ref::<String>().map(String::as_str).or_else(|| payload.downcast_ref::<&str>().copied()).unwrap_or("unknown panic");
                panics.record(began.elapsed().as_secs(), msg);
                s.rebuild();
                log!("{}", panics.reason.as_deref().unwrap_or("project panic"));
                let mut status = coreq::status::common(&platform::tool(&cfg.install), &cfg.me, &project,
                    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()), panics.reason.clone(), None);
                status["complete"] = json!(false);
                status["kids"] = json!(s.kids.iter().map(|k| json!({"task": k.task, "attempt": k.attempt, "rt": k.rt, "pid": k.proc.id()})).collect::<Vec<_>>());
                publish_status(&platform::tool(&cfg.install), &cfg.root, &status, host.as_ref());
            }
        }
    }
}

/// AS5 Checking (#1661): before a due probe the last published status names it in progress, its prior rows kept;
/// the pass's own status replaces the object without the mark. No prior status: nothing to mark.
fn mark_checking(tool: &coreq::Tool, root: &Path, at: u64) {
    let p = coreq::status::path(tool, root);
    let Some(mut s) = fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object) else { return };
    if !s["agents"].is_object() { s["agents"] = json!({}); }
    s["agents"]["checking"] = json!(at);
    if let Err(e) = platform::write_status(tool, root, &s) { log!("agents checking mark not written: {e}"); }
}

fn publish_status(tool: &coreq::Tool, root: &Path, status: &Value, host: Option<&host::Shared>) -> bool {
    let mut status=status.clone();
    if let Some(host)=host {
        status["host_instance"]=host.lock().map(|h|json!(h.instance)).unwrap_or(Value::Null);
        status["host_install"]=status["host_instance"].as_str().and_then(|instance|host::install_acknowledgement(&tool.install,root,instance)).map_or(Value::Null,|id|json!(id));
    }
    let written = platform::write_status(tool, root, &status);
    if let Err(e) = &written { log!("status publication failed: {e}; observed state unknown"); }
    let qualified = written.is_ok() && snapshot::complete(&status) && status["error"].is_null() && status["focus_error"].is_null();
    if let Some(host) = host { if let Ok(mut h) = host.lock() { h.publication(root, qualified); } }
    qualified
}

fn write_kids(install:&Path,root:&Path,kids:&[data::KidLine])->Res<()> {
    let owner=coreq::process::identity(std::process::id()).map_err(|e|e.to_string())?.ok_or("service native identity missing")?;
    let mut text=format!("pid {} {} executor-v1\n",owner.pid,owner.started);
    for k in kids {
        text+=&format!("{} {} {} {} {} {} {}{}\n",k.task,k.attempt,k.runtime,k.process.pid,k.process.started,k.execution,k.offset,k.job.as_ref().map(|key|format!(" job-global:{key}")).unwrap_or_default());
    }
    data::kids(&text)?;
    let path=kids_path(install,root)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;
    coreq::write_durable(&path, &text)
}

fn checkpoint_kid(install:&Path,root:&Path,kid:data::KidLine)->Res<()> {
    let mut kids=kids_file(install,root)?;
    // #1561: one native process backs one execution; a record never takes another task's process.
    if let Some(other)=kids.iter().find(|k|k.task!=kid.task&&k.process==kid.process) {
        return Err(format!("native process already recorded for #{}; retry native readback",other.task));
    }
    if let Some(old)=kids.iter_mut().find(|k|k.task==kid.task) {
        if *old==kid { return Ok(()) }
        // #1638: the same execution may move to a new process only after its recorded one exited (checked below).
        let same=old.execution==kid.execution && old.attempt==kid.attempt && old.runtime==kid.runtime && old.job==kid.job;
        if old.execution>kid.execution || old.execution==kid.execution && !same { return Err("execution already has different or newer native recovery identity; file kept".into()); }
        if runtime::execution_live(old)? {
            return Err("previous native execution still present; recovery identity kept".into());
        }
        *old=kid;
    } else { kids.push(kid); }
    write_kids(install,root,&kids)
}

/// Called only after the exact execution's native Exit has been read back from the board.
fn retire_kid(cfg:&Config,task:u64,attempt:u64,execution:u64)->Res<()> {
    let mut kids=kids_file(&cfg.install,&cfg.root)?;
    let before=kids.len();
    kids.retain(|k|!(k.task==task && k.attempt==attempt && k.execution==execution));
    if kids.len()==before { return Ok(()) }
    write_kids(&cfg.install,&cfg.root,&kids)
}

fn save_kids(s: &Service, root: &Path) -> Res<()> {
    let exited=|task:u64,attempt:u64,execution:u64,runtime:&str|->Res<bool> {
        let log=board::entries(s.board,task)?;
        Ok(core::executions(task,&log).iter().any(|x|x.id==execution && x.attempt==attempt && x.host==s.cfg.me && x.runtime==runtime && x.release==Some(core::Release::Exit)))
    };
    let mut kids=vec![];
    for k in &s.kids {
        let line=data::KidLine {task:k.task,attempt:k.attempt,runtime:k.rt.clone(),process:k.proc.native_identity()?,execution:k.execution,offset:k.proc.input_offset()?,job:k.proc.job()};
        match recorded(&line) {
            ("running",_)=>{},
            // #1561: a recorded process that exited resolves from its board exit; until then one error names the wait.
            ("ended",_) if exited(k.task,k.attempt,k.execution,&k.rt)? => continue,
            (_,why)=>return Err(format!("#{} execution {}: {why}; handover waits for its board release",k.task,k.execution)),
        }
        kids.push(line);
    }
    let mut kept=vec![];
    for old in kids_file(&s.cfg.install,root)? {
        if kids.contains(&old) { continue; }
        if !exited(old.task,old.attempt,old.execution,&old.runtime)? {
            // #1435: a Windows exit keeps the record for the next image instead of holding the update on it
            if !cfg!(windows) { return Err("unresolved native recovery record cannot be discarded during handover".into()); }
            kept.push(old);
        }
    }
    kids.extend(kept);
    write_kids(&s.cfg.install,root,&kids)
}

/// #1766: a project the handover barrier keeps out of passes (#1402) still observes the board for its status.json.
fn observes(host: Option<&host::Shared>, root: &Path) -> Res<bool> {
    Ok(match host {Some(host)=>!host.lock().map_err(|_|"host handover unknown")?.begin_pass(root),None=>false})
}

/// #1769: a Stop drains running work only. A released attempt whose resume the Stop refused stays owned on the board
/// and resumes after the next start; waiting on it deadlocked the drain.
fn drained(s: &Service, h: &host::Host) -> bool {
    if h.restart && h.stopping { lifecycle_waits(s, true).is_empty() } else { s.idle() }
}

/// The end of a project pass under a pending restart: prepared once nothing it owns blocks the re-exec.
fn report_handoff(s: &Service, h: &mut host::Host, root: &Path, drained: bool) -> Res<()> {
    let ready = h.restart && (if h.stopping {drained}else{lifecycle_waits(s, false).is_empty()});
    let saved = if ready { save_kids(s, root) } else { Ok(()) };
    h.end_pass(root, ready && saved.is_ok());
    saved
}

/// A failed reduced loop retains its handles; an image switch also needs recovery evidence.
fn stopped_handoff(s: &mut Service, host: &host::Shared, cfg: &Config) -> Res<bool> {
    if !host.lock().map_err(|_|"stopped project admission unknown")?.begin_pass(&cfg.root) {return Ok(false);}
    s.reap_owned();
    s.reconcile_ownership()?;
    s.sync_host();
    let mut h = host.lock().map_err(|_| "stopped project admission unknown")?;
    let idle = drained(s, &h);
    h.busy.insert(cfg.root.clone(), !idle);
    report_handoff(s, &mut h, &cfg.root, idle)?;
    Ok(idle && h.draining.contains(&cfg.root))
}

// ---------- capacity (§ 3, § 5 `service capacity`, § 9) ----------

/// One owner edit of a host's capacity: runtime limits and load gate fields; `None` keeps a field.
#[derive(Default, Clone)]
struct CapEdit {
    /// `admission.mode` auto (true) or fixed (false), § 9
    mode: Option<bool>,
    /// `admission.max`: `Some(None)` removes it
    max: Option<Option<u32>>,
    limits: BTreeMap<String, u32>,
    /// runtimes whose `capacity_hosts` limit override goes (resource policy `runtime_limit: null`)
    unset: std::collections::BTreeSet<String>,
    gate: Option<bool>,
    cpu_max: Option<u32>,
    memory_min: Option<u32>,
    /// host.json `claim`: `labelled` (true) or `any` (false), § 9
    claim: Option<bool>,
    /// host.json `default_runtime`, § 9
    default_runtime: Option<String>,
}

impl CapEdit {
    fn from_args(a: &Args) -> Res<CapEdit> {
        let num = |k: &str| a.one(k).map(|x| x.parse::<u32>().map_err(|_| format!("--{k}: not an integer 0..{}", u32::MAX))).transpose();
        let mut e = CapEdit { cpu_max: num("cpu-max")?, memory_min: num("memory-min")?, ..Default::default() };
        for rt in RUNTIMES {
            if let Some(n) = num(rt)? {
                e.limits.insert(rt.into(), n);
            }
        }
        if a.one("mode").is_some() {return Err("host policy has no mode selector; use optional limits and the load gate".into());}
        e.max = match a.one("max") {
            None => None,
            Some("none") => Some(None),
            Some(_) => Some(num("max")?),
        };
        e.gate = match a.one("load-gate") {
            None => None,
            Some("on") => Some(true),
            Some("off") => Some(false),
            Some(x) => return Err(format!("--load-gate: on or off, not {x}")),
        };
        e.claim = match a.one("claim") {
            None => None,
            Some("any") => Some(false),
            Some("labelled") => Some(true),
            Some(x) => return Err(format!("--claim: any or labelled, not {x}")),
        };
        e.default_runtime = match a.one("default-runtime") {
            None => None,
            Some(rt) if host::DEFAULT_RUNTIMES.contains(&rt) => Some(rt.into()),
            Some(x) => return Err(format!("--default-runtime: {}, not {x}", host::DEFAULT_RUNTIMES.join(" or "))),
        };
        Ok(e)
    }

    fn is_empty(&self) -> bool {
        self.mode.is_none() && self.max.is_none() && self.limits.is_empty() && self.unset.is_empty() && self.gate.is_none() && self.cpu_max.is_none() && self.memory_min.is_none() && self.claim.is_none()
            && self.default_runtime.is_none()
    }
}

use service::RUNTIMES;

/// `taskq.json` text with `e` applied to `host` (§ 9): only that host's `capacity_hosts` entry changes, other keys
/// and their order stay. `expect`: the capacity the owner saw; a different current one is a concurrent edit.
fn edit_capacity(text: &str, host: &str, me: &str, e: &CapEdit, expect: Option<&Value>) -> Res<String> {
    let mut v: Value = serde_json::from_str(text).map_err(|e| format!("taskq.json: {e}"))?;
    let known = host == me || v["hosts"].as_object().is_some_and(|h| h.values().any(|n| n == host));
    if !known {
        return Err(format!("unknown host {host}: use this host ({me}) or a name in hosts"));
    }
    let old = service::capacity(&v, host)?;
    if expect.is_some_and(|x| *x != old.json()) {
        return Err("capacity changed since it was shown; nothing written".into());
    }
    if e.is_empty() {
        return Err("nothing to change: give --mode, --max, --claude, --codex, --codex-app, --load-gate, --cpu-max or --memory-min".into());
    }
    if let Some(rt) = e.limits.keys().chain(&e.unset).find(|k| !RUNTIMES.contains(&k.as_str())) {
        return Err(format!("unknown runtime {rt}"));
    }
    if !v["capacity_hosts"].is_object() {
        v["capacity_hosts"] = json!({});
    }
    let h = &mut v["capacity_hosts"][host];
    for (rt, n) in &e.limits {
        h["limits"][rt] = json!(n);
    }
    for rt in &e.unset {
        if let Some(l) = h["limits"].as_object_mut() {
            l.remove(rt);
        }
    }
    if e.mode.is_some() || e.max.is_some() {
        // the host's object replaces the top-level one (§ 9): carry the unedited field over
        h["admission"] = json!({"mode": if e.mode.unwrap_or(old.auto) { "auto" } else { "fixed" }});
        if let Some(m) = e.max.unwrap_or(old.max) {
            h["admission"]["max"] = json!(m);
        }
    }
    if e.gate.is_some() || e.cpu_max.is_some() || e.memory_min.is_some() {
        let g = old.gate;
        h["load_gate"] = json!({"enabled": e.gate.unwrap_or(g.enabled), "cpu_max": e.cpu_max.unwrap_or(g.cpu_max), "memory_min": e.memory_min.unwrap_or(g.memory_min)});
    }
    service::capacity(&v, host)?;
    // keep the file's indent
    let indent = text.lines().nth(1).map(|l| l.len() - l.trim_start().len()).filter(|n| *n > 0).unwrap_or(2);
    let (mut out, pad) = (vec![], " ".repeat(indent));
    let mut ser = serde_json::Serializer::with_formatter(&mut out, serde_json::ser::PrettyFormatter::with_indent(pad.as_bytes()));
    serde::Serialize::serialize(&v, &mut ser).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out).into_owned() + if text.ends_with('\n') { "\n" } else { "" })
}

fn cap_changes(old: &service::Capacity, new: &service::Capacity) -> String {
    let mut out = vec![];
    let mode = |a: bool| if a { "Auto" } else { "Fixed" };
    if old.auto != new.auto {
        out.push(format!("Mode {} → {}", mode(old.auto), mode(new.auto)));
    }
    if old.max != new.max {
        let m = |x: Option<u32>| x.map_or("none".into(), |x| x.to_string());
        out.push(format!("Maximum {} → {}", m(old.max), m(new.max)));
    }
    for rt in old.limits.keys().chain(new.limits.keys()).collect::<std::collections::BTreeSet<_>>() {
        let (a, b) = (old.limits.get(rt), new.limits.get(rt));
        if a != b {
            let n = |x: Option<&u32>| x.map_or("none".into(), |x| x.to_string());
            out.push(format!("{} {} → {}", runtime_name(rt), n(a), n(b)));
        }
    }
    if old.gate != new.gate {
        let g = |g: service::GateCfg| if g.enabled { format!("on (CPU {} %, memory {} %)", g.cpu_max, g.memory_min) } else { "off".into() };
        out.push(format!("Load gate {} → {}", g(old.gate), g(new.gate)));
    }
    if out.is_empty() { "no change".into() } else { out.join(", ") }
}

/// § 9: a task added on a `claim` `labelled` host keeps that host unless `--host` names one.
fn add_host(given: Option<&str>, cfg: &Config) -> Option<String> {
    given.map(String::from).or_else(|| cfg.labelled.then(|| cfg.me.clone()))
}

/// host.json `claim` value (§ 9).
fn claim_name(labelled: bool) -> &'static str { if labelled { "labelled" } else { "any" } }

fn runtime_name(rt: &str) -> String {
    let mut c = rt.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The gate line of a `status.json` capacity (§ 4 tray text).
fn gate_text(c: &Value) -> String {
    let reasons = |codes: &[&str]| -> Vec<&Value> { c["reasons"].as_array().into_iter().flatten().filter(|r| codes.iter().any(|x| r["code"] == *x)).collect() };
    match c["gate"].as_str() {
        Some("off") => "Off".into(),
        Some("open") => "Ready".into(),
        Some("held") => format!("Waiting: {}", reasons(&["cpu", "memory", "cooldown", "max", "spacing", "version"]).into_iter().map(|r| match r["code"].as_str() { Some("cpu") => "CPU", Some("memory") => "memory", Some("max") => "host maximum", Some("spacing") => "host spacing", Some("version") => "old service", _ => "recovery" }).collect::<Vec<_>>().join(", ")),
        Some(_) => format!("Unknown: {}", reasons(&["sample", "config", "host"]).into_iter().filter_map(|r| r["text"].as_str()).collect::<Vec<_>>().join("; ")),
        None => "Unknown: service has no capacity status".into(),
    }
}

/// The admission line of a `status.json` capacity (§ 3): mode, host ceiling and use, own reservations.
fn admission_text(c: &Value) -> String {
    let n = |v: &Value| v.as_u64().map_or("unknown".into(), |n| n.to_string());
    let a = &c["admission"];
    let mode = match a["mode"].as_str() {
        Some("auto") => format!("Auto · host {} / {} held", n(&c["host_used"]), a["max"].as_u64().map_or("no max".into(), |m| m.to_string())),
        Some(_) => "Fixed".into(),
        None => "unknown (service older than this release)".into(),
    };
    let res: Vec<String> = c["reservations"].as_array().into_iter().flatten().map(|r| format!("#{} {}", r["task"], r["state"].as_str().unwrap_or("?"))).collect();
    format!("Admission: {mode}{}", if res.is_empty() { String::new() } else { format!(" · {}", res.join(", ")) })
}

/// `taskq service permissions` (§ 5, § 8 Access): requested vs effective access per runtime; a write sets one grant.
fn permissions_cmd(root: &Path, a: &Args) -> Res<()> {
    let (cfg, _) = load(root)?;
    if let Some(rt) = a.one("runtime") {
        if std::env::var_os("TASKQ_ATTEMPT").is_some() {
            // hygiene only: the OS boundary of § 8 Access is what keeps a worker out
            return Err("a worker cannot change the policy that governs it".into());
        }
        let mode = a.need("mode")?;
        let project = match a.one("scope").unwrap_or("host") {
            "host" => None,
            "project" => Some(cfg.root.as_path()),
            s => return Err(format!("--scope {s}: host or project")),
        };
        let at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let by = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "unknown".into());
        let p = policy::grant(&cfg.install, rt, mode, project, &by, at)?;
        println!("Saved: {rt} {mode} ({}) revision {}; new attempts only", if project.is_some() { "project" } else { "host" }, p.revision);
        if let Some(m) = policy::pin_rollback(&cfg.install, platform::VERSION) {
            println!("{m}");
        }
    } else {
        match policy::load(&cfg.install) {
            Ok(p) if p.grants.is_empty() => println!("Policy: no grants"),
            Ok(p) => println!("Policy: revision {}, {} grants", p.revision, p.grants.len()),
            Err(e) => println!("Policy: unknown: {e}"),
        }
    }
    for e in policy::report(&cfg.install, &cfg.root, &cfg.permission_mode, &cfg.codex) {
        println!("{}", e.line());
    }
    Ok(())
}

/// `taskq service resources` (§ 5): the host operator's resource grants for DashQ operations, project IDs and revisions.
fn resources_cmd(root: &Path, a: &Args) -> Res<()> {
    let (cfg, _) = load(root)?;
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if a.flags.contains_key("json") {
        println!("{}", resources::projection(&cfg.install, now, true)?);
        return Ok(());
    }
    let change = a.one("grant").or(a.one("revoke"));
    if change.is_some() && std::env::var_os("TASKQ_ATTEMPT").is_some() {
        return Err("a worker cannot change resource grants".into());
    }
    if let Some(id) = a.one("revoke") {
        resources::revoke(&cfg.install, id)?;
        println!("Revoked {id}; operations not yet written cannot write");
    } else if let Some(id) = a.one("grant") {
        let hours = a.need("hours")?.parse().map_err(|_| "--hours: a number of hours".to_string())?;
        let scope = resources::Scope { host_policy: a.flags.contains_key("host-policy"), project_limits: a.flags.contains_key("project-limits"), projects: a.all("project"), runtimes: a.all("runtime"), hours };
        let by = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "unknown".into());
        let g = resources::grant(&cfg.install, id, a.need("actor")?, &scope, &by, now)?;
        println!("Granted {id} to {} until {}", g.actor_id, board::stamp(g.expires_at as i64));
    }
    for l in resources::report(&cfg.install, now)? {
        println!("{l}");
    }
    Ok(())
}

fn pm_cmd(root:&Path,a:&Args)->Res<()> {
    if a.pos.len()!=2||a.flags.keys().any(|k|!["grant","actor","hours","project","runtime","revoke","json"].contains(&k.as_str()))
        ||["grant","actor","hours","revoke","json"].iter().any(|k|a.all(k).len()>1)
        ||a.one("grant").is_none()&&["actor","hours","project","runtime"].iter().any(|k|a.flags.contains_key(*k))
        ||a.one("grant").is_some()&&a.one("revoke").is_some(){return Err("Usage: taskq service pm [--grant ID --actor ID --hours N --project P.. --runtime R.. | --revoke ID] [--json]".into());}
    let (cfg,_)=load(root)?;
    let now=SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|_|"PM clock unknown")?.as_secs();
    if let Some(id)=a.one("grant") {pm::grant(&cfg.install,id,a.need("actor")?,&a.all("project"),&a.all("runtime"),a.need("hours")?.parse().map_err(|_|"PM hours invalid")?,now)?;}
    if let Some(id)=a.one("revoke") {pm::revoke(&cfg.install,id)?;}
    let report=pm::report(&cfg.install,now)?;
    if a.flags.contains_key("json"){println!("{report}");}else {
        let action=if let Some(id)=a.one("grant"){format!("PM grant {id} saved")}else if let Some(id)=a.one("revoke"){format!("PM grant {id} revoked")}else{format!("PM requests: {} authorized targets; {} recorded operations",report["targets"].as_array().map_or(0,Vec::len),report["receipts"].as_array().map_or(0,Vec::len))};
        println!("{action} · taskq {}",platform::VERSION);
    }
    Ok(())
}

/// `taskq service capacity` (§ 5).
fn capacity_cmd(a: &Args) -> Res<()> {
    let install=platform::default_install();
    let policy=host::Policy::read(&install);
    let me=policy.as_ref().map(|p|p.me.clone()).unwrap_or_else(|_|std::env::var("TASKQ_HOST").unwrap_or_else(|_|hostname()));
    let host=a.one("host").unwrap_or(&me).to_string();
    let roots=host::projects(&install)?;
    let status=|| -> Option<Value> {
        let now=SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
        roots.iter().find_map(|root| {
            let path=dot(root).join("status.json");
            let st:Value=serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            let ttl=snapshot::ttl(root);
            (st["host"]==me && st["at"].as_i64().is_some_and(|at|snapshot::live(now as i64-at,ttl))
                && snapshot::status_live(&path,ttl)).then_some(st)
        })
    };
    let e = CapEdit::from_args(a)?;
    if e.is_empty() {
        let own=policy.as_ref().ok().filter(|p|p.me==host).map(|p|(p.labelled,p.default_runtime.clone()));
        let saved = policy.and_then(|policy| {
            if policy.me!=host {return Err("no local policy for remote host".into());}
            Ok(policy.cap)
        });
        if let Err(reason)=&saved {println!("Saved host policy: unknown ({reason})");}
        println!("Capacity on {host}");
        if let Some((l,rt))=own {println!("Claim: {}",claim_name(l));println!("Default runtime: {rt}");}
        let c = status().and_then(|st|service::host_capacity(&st["capacity"])).filter(|_|host==me);
        let limits = c.as_ref().map(|c| c["limits"].clone()).or_else(|| saved.as_ref().ok().map(|s| s.json()["limits"].clone())).unwrap_or_default();
        let mut rts: std::collections::BTreeSet<String> = RUNTIMES.iter().map(|rt|rt.to_string()).collect();
        rts.extend(limits.as_object().into_iter().flatten().map(|(k,_)|k.clone()));
        if let Some(c) = &c {
            rts.extend(c["used"].as_object().into_iter().flatten().map(|(k,_)|k.clone()));
        }
        for rt in rts {
            let used = c.as_ref().and_then(|c| c["used"][&rt].as_u64()).map_or("?".into(), |n| n.to_string());
            let limit=limits[&rt].as_u64().map(|n|n.to_string()).unwrap_or_else(||if c.is_some()||saved.is_ok(){"no cap".into()}else{"?".into()});
            println!("{} {used}/{limit}",runtime_name(&rt));
        }
        match (&c, &saved) {
            (Some(c), _) => {
                println!("Load gate: {}", gate_text(c));
                println!("{}", admission_text(c));
            }
            (None, Err(e)) => println!("Unknown: {e}"),
            (None, Ok(_)) if host != me => println!("Unknown: no local snapshot of {host}"),
            (None, Ok(_)) => println!("Unknown: service has no fresh capacity status"),
        }
        if let (Some(c), Ok(s)) = (&c, &saved)
            && (c["limits"] != s.json()["limits"] || c["load_gate"] != s.json()["load_gate"] || c["admission"] != s.json()["admission"])
        {
            println!("Saved config differs: {}", cap_changes(&service::Capacity { limits: serde_json::from_value(c["limits"].clone()).unwrap_or_default(), ..s.clone() }, s));
        }
        return Ok(());
    }
    // a write names its host explicitly (§ 5)
    let host = a.need("host")?.to_string();
    let (old,saved,command)=host::write_capacity(&install,&host,&e,None)?;
    println!("Receipt: {}",command.event_id("observed")?);
    let mut changes=cap_changes(&old,&saved);
    let mut add=|c:String| changes=if changes=="no change" {c} else {format!("{changes}, {c}")};
    if let Some(l)=e.claim {add(format!("Claim {}",claim_name(l)));}
    if let Some(rt)=&e.default_runtime {println!("Default runtime: {rt}");add(format!("Default runtime {rt}"));}
    let mut unknown=None;
    for _ in 0..20 {
        match host::capacity_observed(&install,&command) {
            Ok(true)=> {println!("Applied on {host}: {changes}; running tasks continue");return Ok(());}
            Ok(false)=>{},
            Err(e)=> {unknown=Some(e);break;}
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("Saved; waiting for host service on {host}: {changes}; running tasks continue");
    if let Some(reason)=unknown {eprintln!("Capacity observation unknown: {reason}");}
    Ok(())
}

/// § 8 item 1: print the host channel, or write it to host.json and wait for the service to read it back.
/// § 8 item 1 `service status` line of the dev install; SHAs shortened to 7.
fn dev_line(dev: &Value) -> String {
    let sha = |k: &str| dev[k].as_str().map(|s| s.chars().take(7).collect::<String>());
    let mut line = format!("Dev: installed {}", sha("installed").unwrap_or("unknown".into()));
    if let Some(p) = sha("pending") { line += &format!(" · pending {p}{}", if dev["building"] == true { " (building)" } else { "" }); }
    if let Some(f) = sha("failed") { line += &format!(" · failed {f}"); }
    line
}

fn channel_cmd(a: &Args) -> Res<()> {
    let install=platform::default_install();
    let me=host::Policy::read(&install).map(|p|p.me).unwrap_or_else(|_|std::env::var("TASKQ_HOST").unwrap_or_else(|_|hostname()));
    let observed=|since:u64| -> Option<String> {
        let now=SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
        host::projects(&install).ok()?.iter().find_map(|root| {
            let st:Value=serde_json::from_slice(&fs::read(dot(root).join("status.json")).ok()?).ok()?;
            (st["host"]==me.as_str() && st["at"].as_u64().is_some_and(|at|at>=since && snapshot::live(now as i64-at as i64,snapshot::ttl(root)))).then(||st["channel"].as_str().map(String::from)).flatten()
        })
    };
    let Some(want)=a.pos.get(2) else {
        println!("Channel on {me}: {}",platform::channel(&install).map(String::from).unwrap_or_else(|e|format!("unknown ({e})")));
        println!("Service: {}",observed(0).unwrap_or_else(||"unknown (no fresh status)".into()));
        return Ok(());
    };
    if !matches!(want.as_str(),"dev"|"live") {return Err("channel must be dev or live".into());}
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot change host policy".into());}
    let since=SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|e|e.to_string())?.as_secs();
    resources::locked(&install,|| {
        let path=install.join("host.json");
        let mut v:Value=serde_json::from_str(&fs::read_to_string(&path).map_err(|e|format!("host.json unknown: {e}"))?).map_err(|e|format!("host.json unknown: {e}"))?;
        v["channel"]=json!(want);
        host::Policy::parse(v.clone(),&hostname())?;
        platform::write_atomic(&path,&(serde_json::to_string_pretty(&v).map_err(|e|e.to_string())?+"\n"))
    })?;
    touch(&install.join("poke"));
    for _ in 0..20 {
        if observed(since).as_deref()==Some(want.as_str()) {println!("Applied on {me}: channel {want}");return Ok(());}
        std::thread::sleep(Duration::from_secs(1));
    }
    println!("Saved; waiting for host service on {me}: channel {want}");
    Ok(())
}

/// Start the host's one `taskq tray` of the current release when its heartbeat `<install>/tray` is stale (§ 4); reap
/// the one this image started.
fn keep_tray(install: &Path, tray: &mut Option<std::process::Child>) {
    if !cfg!(any(target_os = "macos", windows)) {
        return;
    }
    if tray.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None))) {
        *tray = None;
    }
    if tray.is_some() || file_age(&install.join("tray")).is_some_and(|a| a < 30) {
        return;
    }
    let t = platform::tool(install);
    let Some(v) = t.current() else { return };
    // M15: Windows keys the notification-area pin by exe path, so the tray runs as the launcher `<install>/taskq.exe`,
    // which every update rewrites with the current release
    let launcher = install.join(t.exe_name());
    let exe = if cfg!(windows) && launcher.is_file() { launcher } else { t.release_exe(&v) };
    let mut c = std::process::Command::new(&exe);
    c.arg("tray");
    // through LaunchServices: a bundle exec'd by the service has the unbundled service as its responsible process,
    // and the OS refuses its authorization request (UNErrorDomain 1). `open` passes no args, env or stderr here: the
    // bundle with no args is the tray of its parent folder and writes its own tray.log (`bundled_tray`)
    #[cfg(target_os = "macos")]
    if let Some(app) = tray_app(install, &exe) {
        c = std::process::Command::new("open");
        c.args(["-n", "-W", "-g"]).arg(app);
    }
    // #1755: appended, so a relaunch keeps the last tray's fault lines; it starts over past 1 MiB
    let log = install.join("tray.log");
    if fs::metadata(&log).is_ok_and(|m| m.len() > 1 << 20) { let _ = fs::remove_file(&log); }
    let err = fs::OpenOptions::new().create(true).append(true).open(&log).map(std::process::Stdio::from).unwrap_or_else(|_| std::process::Stdio::null());
    c.current_dir(install).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(err);
    // its own process group: stopping one project's service (launchctl bootout) leaves the host's tray alone
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    *tray = platform::quiet(&mut c).spawn().ok();
}

/// #1801: only the OS default install's bundle is `org.taskq.tray`; temp, test and evidence installs take their own id,
/// so their iconless copies never win the notification icon in LaunchServices.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn tray_id(home: bool) -> &'static str {
    if home { "org.taskq.tray" } else { "org.taskq.tray.test" }
}

/// macOS notifications need a bundle identity (§ 4): `<install>/TaskQ.app`, signed as § 8 step 5 (else ad-hoc),
/// holding the current release; the default install's bundle is `~/Applications/TaskQ.app`, which Finder, Spotlight
/// and the Dock open (M15). None: the tray runs unbundled and notifications read unsupported.
#[cfg(target_os = "macos")]
fn tray_app(install: &Path, exe: &Path) -> Option<PathBuf> {
    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{id}</string>
<key>CFBundleName</key><string>TaskQ</string>
<key>CFBundleExecutable</key><string>taskq</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>TaskQ.icns</string>
</dict></plist>
"#;
    // M15: a regular app (Dock, Cmd+Tab), so no LSUIElement; only the OS default install writes the user's launcher
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|_| platform::is_home(install, platform::home_install().as_deref()));
    let app = home.as_ref().map_or_else(|| install.join("TaskQ.app"), |h| h.join("Applications/TaskQ.app"));
    // one bundle per identity: the default install's older `<install>/TaskQ.app` would shadow it in LaunchServices
    if home.is_some() { let _ = fs::remove_dir_all(install.join("TaskQ.app")); }
    let bin = app.join("Contents/MacOS/taskq");
    fs::create_dir_all(bin.parent()?).ok()?;
    let id = tray_id(home.is_some());
    fs::write(app.join("Contents/Info.plist"), PLIST.replace("{id}", id)).ok()?;
    let resources=app.join("Contents/Resources");
    fs::create_dir_all(&resources).ok()?;
    fs::write(resources.join("TaskQ.icns"),include_bytes!("../assets/TaskQ.icns")).ok()?;
    let tmp = bin.with_extension("tmp");
    fs::copy(exe, &tmp).and_then(|_| fs::rename(&tmp, &bin)).ok()?;
    let signed = match platform::codesign(platform::SIGNING_IDENTITY, &app, id) {
        Ok(true) => true,
        r => {
            if let Err(e) = r { platform::log(&format!("signing failed, TaskQ.app stays ad-hoc: {e}")) }
            coreq::process::output("signing.adhoc", std::process::Command::new("codesign").args(["-f", "-s", "-"]).arg(&app), std::time::Duration::from_secs(30)).is_ok()
        }
    };
    // #1801: every tray launch (install, and update: the tray quits for a newer release) rewrites the bundle; force
    // LaunchServices to re-read it so the release icon wins over a cached or stale registration
    if signed && home.is_some() {
        const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        if let Err(e) = coreq::process::output("lsregister", std::process::Command::new(LSREGISTER).arg("-f").arg(&app), std::time::Duration::from_secs(30)) {
            platform::log(&format!("lsregister -f failed, the notification icon may stay stale: {e}"));
        }
    }
    signed.then_some(app)
}

/// The tray `open` started (`keep_tray`, Finder, Spotlight, Dock): `TaskQ.app/Contents/MacOS/taskq` with no args runs
/// the tray of `<install>`, its stderr appended to `<install>/tray.log`.
#[cfg(target_os = "macos")]
fn bundled_tray(exe: &Path, install:&Path) -> bool {
    use std::os::fd::AsRawFd;
    if !exe.ends_with("TaskQ.app/Contents/MacOS/taskq") {return false;}
    if let Ok(f) = fs::OpenOptions::new().create(true).append(true).open(install.join("tray.log")) {
        // SAFETY: both descriptors are open; fd 2 now refers to the log for the life of the process
        unsafe { libc::dup2(f.as_raw_fd(), 2) };
    }
    true
}

/// Labels of the open tasks in the last `status.json`, for a resume that has no focus to restore (§ 4).
fn status_labels(root: &Path) -> Vec<String> {
    let st: Value = fs::read_to_string(dot(root).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut l: Vec<String> = st["tasks"].as_array().into_iter().flatten().flat_map(|t| t["labels"].as_array().cloned().unwrap_or_default())
        .filter_map(|x| x.as_str().map(String::from)).filter(|x| !x.starts_with("q-") && x != service::TASK_LABEL).collect();
    l.sort();
    l.dedup();
    l
}

/// Focus (§ 4): `f` admits; `request` keeps the owner's words, none removes them.
#[tracing::instrument(name="set_focus",skip_all)]
fn set_focus(root: &Path, f: &str, request: Option<&str>) -> Res<String> {
    change_focus(root, f.trim() == "off", true, |d| {
        platform::write_atomic(&d.join("focus"), f)?;
        match request {
            Some(r) => platform::write_atomic(&d.join("focus.request"), r)?,
            None => {
                fs::remove_file(d.join("focus.request")).or_else(|e| if e.kind()==std::io::ErrorKind::NotFound {Ok(())} else {Err(e)}).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }).map(|(_,receipt)|receipt)
}

/// Install (§ 4 Focus): a missing focus becomes the explicit default `off`; any other stays. True when it reads off.
fn install_focus(root: &Path) -> Res<bool> {
    let _signal = wake_lock(root)?;
    let d = dot(root);
    if !d.join("focus").exists() {
        platform::write_atomic(&d.join("focus"), "off")?;
    }
    Ok(data::focus(root).is_ok_and(|f| f.is_empty()))
}

/// Pause (§ 4): keep the focus in `focus.paused`, admit nothing.
#[tracing::instrument(name="pause",skip_all)]
fn pause(root: &Path, initialize: bool) -> Res<String> {
    change_focus(root, true, initialize, |d| {
        let f = data::focus(root)?.join(" ");
        if !f.is_empty() { platform::write_atomic(&d.join("focus.paused"), &f)?; }
        platform::write_atomic(&d.join("focus"), "off")
    }).map(|(_,receipt)|receipt)
}

/// Resume (§ 4): restore the saved focus; never `all` by default.
#[tracing::instrument(name="resume",skip_all)]
fn resume(root: &Path) -> Res<(String,String)> {
    change_focus(root, false, false, |d| {
        let f = fs::read_to_string(d.join("focus.paused")).or_else(|e| if e.kind()==std::io::ErrorKind::NotFound {Ok(String::new())} else {Err(e)})
            .map_err(|e| format!("saved focus unreadable: {e}"))?;
        if matches!(f.trim(), "" | "off") {
            return Err(format!("no saved focus: choose with `taskq service focus all|<label..>`; labels of open tasks: {}", status_labels(root).join(", ")));
        }
        platform::write_atomic(&d.join("focus"), &f)?;
        Ok(f.trim().to_string())
    })
}

/// The heartbeat's `service <id>` (§ 4): public, so a hash of the installation id, never the id itself.
fn service_id(installation: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, installation.as_bytes());
    d.as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// Another live process that wrote this root's fresh `status.json` (§ 4 one service per root).
fn other_service(root: &Path) -> Option<u32> {
    let p = dot(root).join("status.json");
    if !snapshot::status_live(&p, snapshot::ttl(root)) {
        return None;
    }
    let pid = data::json(&p).ok()??["pid"].as_u64()? as u32;
    (pid != std::process::id() && runtime::alive(pid)).then_some(pid)
}

#[cfg(unix)]
fn reexec(s: &Service, cfg: &Config) -> Res<()> {
    use std::os::unix::process::CommandExt;
    // App turns and Claude turns without a native identity have no adoptable handle; keep their lease until completion.
    if !lifecycle_waits(s, false).is_empty() {
        return fs::write(dot(&cfg.root).join("restart"), "").map_err(|e| e.to_string());
    }
    save_kids(s, &cfg.root)?;
    let mut command = std::process::Command::new(cfg.install.join(platform::tool(&cfg.install).exe_name()));
    command.args(["service", "run"]);
    coreq::telemetry::capture::before_exec();
    let e = command.exec();
    incident::detect("dispatch.reexec", concat!("src/main.rs:", line!()), "none", &[], &["reexec:failed"]);
    Err(format!("re-exec: {e}"))
}

#[cfg(not(unix))]
fn reexec(s: &Service, cfg: &Config) -> Res<()> {
    // ponytail: Windows cannot hand children to a new image; exit once no child is live and the unit restarts us.
    // The new `current` is already written, so the update waits only for the running attempts.
    if lifecycle_waits(s, false).is_empty() {
        crate::platform::service_exit(75, &format!("restart to {}", platform::tool(&cfg.install).current().unwrap_or_default()));
    }
    // update checks pause while the restart request carries the re-exec until the children end
    fs::write(dot(&cfg.root).join("restart"), "").map_err(|e| e.to_string())
}

/// § 8 step 3: what a pending re-exec waits for. Windows waits for every live child; elsewhere CLI children and
/// Claude background turns with a saved native identity pass over (#1407); codex-app turns wait.
fn lifecycle_waits(s: &Service, stopping:bool) -> Vec<String> {
    let mut w: Vec<String> = s.kids.iter().filter(|k| stopping || cfg!(windows) || runtime::handover_waits(&k.rt,k.proc.as_ref())).map(|k| format!("#{}", k.task)).collect();
    w.dedup();
    // a refused resume (#1601) or Stop (#1769) leaves a childless attempt to the next start
    w.extend(s.handoff_waits(true, cfg!(windows) && !stopping));
    w
}

fn service_fields(root: &Path) -> String {
    service_project_fields(root,data::json(&dot(root).join("status.json")))
}

fn service_project_fields(root:&Path,status:Res<Option<Value>>)->String {
    let d = dot(root);
    let mut out = vec![format!("Project: `{}`", root.display())];
    match data::focus(root) {
        _ if !d.join("focus").exists() => out.push("Focus: unknown (focus file unavailable)".into()),
        // menu.md:250 `Paused`: focus off with a saved focus (display::mode)
        Ok(f) if f.is_empty() => out.extend(["Focus: off".to_string()].into_iter().chain((display::mode(false, display::saved(&d)) == "paused").then(|| "Paused".to_string()))),
        Ok(f) => out.extend(f.iter().map(|l| format!("Focus: {l}"))),
        Err(e) => out.push(format!("Focus: unknown ({e})")),
    }
    match host::Policy::read(&platform::default_install()) {
        Ok(p) => out.extend([format!("Claim: {} (host {})", claim_name(p.labelled), p.me), format!("Default runtime: {} (host {})", p.default_runtime, p.me)]),
        Err(e) => out.extend([format!("Claim: unknown ({e})"), "Default runtime: unknown".into()]),
    }
    if let Err(e) = &status {
        out.push(format!("Status: unknown ({e})"));
    }
    let status = status.ok().flatten();
    if let Some(w) = status.as_ref().and_then(|s| s["restart_waits"].as_array()).filter(|w| !w.is_empty()) {
        out.push(format!("Restart: waits for {}", w.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")));
    }
    // § 4 board budget: only a live status speaks for the running service
    let ttl = snapshot::ttl(root);
    let requests = match status.as_ref().filter(|_| snapshot::status_live(&d.join("status.json"), ttl)) {
        Some(s) => requests::lines(s),
        None => vec!["Board requests: unknown (status missing or stale)".into()],
    };
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs());
    let fresh = status.as_ref().and_then(|s| {
        s["tasks_age_seconds"].as_u64()?.checked_add(now?.checked_sub(s["at"].as_u64()?)?)
    }).filter(|a| snapshot::live(*a as i64, ttl));
    match (status, fresh) {
        (Some(s), Some(a)) if snapshot::complete(&s) => {
            out.push(snapshot::counts_line(&snapshot::status_totals(&s).unwrap()));
            out.push(format!("Freshness: {} (cached status)", age(a as i64)));
        }
        _ => out.push("Snapshot: unknown (status missing, stale or failed)\nFreshness: unknown (no fresh successful status)\nCounts: unknown (no fresh successful read)".into()),
    }
    out.extend(requests);
    out.join("\n")
}

fn service_report(a:&Args)->Res<()> {
    let install=platform::default_install();
    let service=host::service_status(&install);
    let registry=host::project_paths(&install);
    let rows:Vec<_>=registry.as_ref().ok().into_iter().flatten().map(|root|(root,data::json(&dot(root).join("status.json")))).collect();
    let binding=|status:&Res<Option<Value>>| {
        let Some(s)=status.as_ref().ok().and_then(Option::as_ref) else {return "unknown";};
        if service["state"]!="running"||s["pid"].as_u64().is_none_or(|pid|pid==0||pid>i32::MAX as u64)
            ||s["host_instance"].as_str().is_none_or(|id|!resources::id(id)) {return "unknown";}
        if s["pid"]==service["pid"]&&s["host_instance"]==service["instance"] {"current"}else{"other"}
    };
    let installation=host::installation_status(&install,a.one("request"));
    let lifecycle=if a.one("request").is_some()&&installation["id"].is_string() {installation.clone()}else{lifecycle::status(&install,a.one("request"))};
    if a.flags.contains_key("json") {
        let projects:Vec<_>=rows.into_iter().map(|(root,status)|json!({"root":root,"host_binding":binding(&status),
            "status":status.as_ref().ok().and_then(Option::as_ref),"error":status.as_ref().err()})).collect();
        println!("{}",json!({"schema":"taskq.service/1","service":service,"projects":projects,"registry_error":registry.as_ref().err(),"restart":if lifecycle["action"].as_str().is_some_and(|action|action!="restart") {Value::Null}else{lifecycle.clone()},"lifecycle":lifecycle,"installation":installation}));
    } else {
        println!("{}",host::service_line(&service));
        if lifecycle["telemetry"]=="unknown" {println!("Telemetry receipts: unknown; lifecycle state is recorded separately");}
        let restart=lifecycle;let title=match restart["action"].as_str() {Some("stop")=>"Stop",Some("uninstall")=>"Uninstall",Some("install")=>"Install",_=>"Restart"};
        if let Some(id)=restart["id"].as_str() {println!("{title}: {id} {}",restart["state"].as_str().unwrap_or("unknown"));}
        if let Some(reason)=restart["reason"].as_str() {println!("{title}: unknown ({reason})");}
        if a.one("request").is_none()&&let Some(id)=installation["id"].as_str() {println!("Install: {id} {}",installation["state"].as_str().unwrap_or("unknown"));}
        if a.one("request").is_none()&&let Some(reason)=installation["reason"].as_str() {println!("Install: unknown ({reason})");}
        if let Err(error)=&registry {println!("Projects: unknown ({error})");}
        for (root,status) in rows {
            let owner=binding(&status);
            let notice=status.as_ref().ok().and_then(Option::as_ref).is_some_and(|s|s["incident_notice"]==true);
            let update=status.as_ref().ok().and_then(Option::as_ref).and_then(|s|s["update"].as_str()).unwrap_or("unknown");
            println!("Update: {update}");
            println!("Channel: {}",status.as_ref().ok().and_then(Option::as_ref).and_then(|s|s["channel"].as_str()).unwrap_or("unknown"));
            for l in runtime::login_lines(&status.as_ref().ok().and_then(Option::as_ref).map_or(Value::Null,|s|s["agents"]["rows"].clone())) {println!("{l}");}
            if let Some(dev)=status.as_ref().ok().and_then(Option::as_ref).map(|s|&s["dev"]).filter(|d|d.is_object()) {println!("{}",dev_line(dev));}
            println!("{}\nProject owner: {owner}",service_project_fields(root,status));
            println!("{}",incident::line(&incident::status(root)));
            if notice||incident::notice(root) {println!("Incident reports: notice");}
        }
    }
    Ok(())
}

#[tracing::instrument(name="service.command",skip_all,fields(coreq_operation=match args.pos.get(1).map(String::as_str).unwrap_or("") {
    sub @ ("install"|"run"|"onboard"|"focus"|"pause"|"resume"|"capacity"|"channel"|"permissions"|"resources"|"pm"|"hold"|"release"|"recover"|"status"|"restart"|"stop"|"complete-stop"|"uninstall"|"list")=>sub,
    _=>"invalid",
}))]
fn service(args: &Args) -> Res<()> {
    let sub = args.pos.get(1).map(String::as_str).unwrap_or("");
    if sub == "run" {
        // before root and config load: their errors reach the unit log stamped too (§ 4)
        platform::service_log();
    }
    if matches!(sub,"status"|"list") {return service_report(args);}
    if sub == "capacity" {return capacity_cmd(args);}
    if sub == "setup" {
        if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot request the host setup session".into());}
        if args.pos.len()!=2||args.all("runtime").len()>1||args.all("client").len()>1||args.flags.keys().any(|key|!["runtime","client","target","json"].contains(&key.as_str())) {
            return Err("usage: taskq service setup [--runtime claude|codex] [--client C] [--target PATH..] [--json]".into());
        }
        let r=setup::submit(&platform::default_install(),args.one("runtime").unwrap_or(runtime::OWNER),args.one("client"),&args.all("target"))?;
        if args.flags.contains_key("json") {println!("{}",serde_json::to_string(&r).map_err(|e|e.to_string())?);}
        else {println!("Setup: {} {}{}",r.id,r.state,r.url.as_deref().map(|u|format!(" {u}")).unwrap_or_default());if let Some(reason)=&r.reason {println!("Reason: {reason}");}}
        return Ok(());
    }
    if sub == "onboard" {
        if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot onboard the host".into());}
        if args.pos.len()!=2||!args.flags.is_empty() {return Err("usage: taskq service onboard".into());}
        return onboard_cmd(&platform::default_install());
    }
    if sub == "channel" {return channel_cmd(args);}
    if sub=="complete-stop" {
        platform::service_log();
        if args.pos.len()!=2||args.all("request").len()!=1||args.flags.keys().any(|key|!["request","json"].contains(&key.as_str())) {return Err("usage: taskq service complete-stop --request ID [--json]".into());}
        let result=lifecycle::complete_stop(&platform::default_install(),args.need("request")?)?;
        if args.flags.contains_key("json") {println!("{result}");}return Ok(());
    }
    if matches!(sub,"restart"|"stop"|"uninstall") {
        if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot restart the host service or request Stop".into());}
        if args.pos.len()!=2||args.all("request").len()>1||args.flags.keys().any(|key|!["request","json","host","mode","diagnose","diagnose-ms","diagnose-limit"].contains(&key.as_str()))||sub!="uninstall"&&args.flags.contains_key("host") {
            return Err("usage: taskq service restart|stop|uninstall [--host] [--mode drain|full] [--request ID] [--json]".into());
        }
        if args.flags.contains_key("mode") && !cfg!(target_os="macos") {return Err("Stop modes are available only on macOS".into());}
        if args.all("mode").len()>1 || args.one("mode").is_some_and(|m|sub!="stop" || !matches!(m,"drain"|"full")) {return Err("Stop mode must be drain or full".into());}
        let install=platform::default_install();
        let id=args.one("request").map(str::to_owned).unwrap_or_else(runtime::uuid);
        let mut result=if let Some(receipt)=lifecycle::receipt(&install,&id)? {receipt} else {
            let reviewed=match sub {
                "stop"=>if args.one("mode")==Some("full") {lifecycle::full_stop_review(&install,&id)?}else{lifecycle::stop_review(&install,&id)?},
                "uninstall"=>{let root=if args.flags.contains_key("host") {None}else{Some(find_root()?)};lifecycle::uninstall_review(&install,&id,root.as_deref())?},
                _=>lifecycle::review(&install,&id)?,
            };
            if !args.flags.contains_key("json") {
                println!("Scope: host service; all registered projects");
                if sub=="stop" && cfg!(target_os="macos") {println!("Stop mode: {}; durable hold retained; automatic rollback to older releases disabled",args.one("mode").unwrap_or("drain"));}
                for root in reviewed["roots"].as_array().into_iter().flatten() {println!("Project: {}",root.as_str().unwrap_or("unknown"));}
            }
            lifecycle::submit(&install,&reviewed)?
        };
        if args.one("mode").is_some_and(|mode|result["mode"]!=mode) {return Err("request ID belongs to another Stop mode".into());}
        if result["action"].as_str().unwrap_or("restart")!=sub {return Err("request ID belongs to another lifecycle action".into());}
        if args.flags.contains_key("host")&&result["scope"]!="host" {return Err("request ID belongs to a project Uninstall; payload unchanged".into());}
        if matches!(sub,"stop"|"uninstall")&&result["state"]=="applied"&&let Some(p)=lifecycle::pending(&install)?.filter(|p|p.stops()) {
            if p.id()!=id {return Err("Stop replay binding changed; reconcile service status".into());}
            host_control::dispatch(&install,&p)?.wait()?;
            result=lifecycle::receipt(&install,&id)?.ok_or("Stop result no longer retained; outcome unknown")?;
        }
        if !args.flags.contains_key("json") && result["telemetry"]=="unknown" {println!("Telemetry receipts: unknown; lifecycle state is recorded separately");}
        if args.flags.contains_key("json") {println!("{result}");}else {println!("{}: {id} {}",match sub {"stop"=>lifecycle::Action::Stop,"uninstall"=>lifecycle::Action::Uninstall,_=>lifecycle::Action::Restart}.title(),result["state"].as_str().unwrap_or("unknown"));if let Some(condition)=result["restart_condition"].as_str() {println!("Restart condition: {condition}");}}
        return Ok(());
    }
    if sub == "run" {
        if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot run the host service".into());}
        let install=platform::default_install();
        let in_install=std::env::current_dir().and_then(|p|p.canonicalize()).ok()
            .zip(install.canonicalize().ok()).is_some_and(|(cwd,install)|cwd==install);
        if !in_install {
            match find_root() {
                Ok(root)=>{
                    let root=root.canonicalize().map_err(|e|format!("service root unknown: {e}"))?;
                    if !host::projects(&install)?.contains(&root) {return Err("service root is not registered; install it explicitly before running".into());}
                },
                Err(error) if std::env::var_os("TASKQ_ROOT").is_some()=>return Err(error),
                Err(_)=>{},
            }
        }
        #[cfg(windows)] runtime::windows_process::exit_with_launcher(&install.join(platform::tool(&install).exe_name()));
        return host::run(&install);
    }
    let root = find_root()?;
    match sub {
        "focus" => {
            let f = args.pos[2..].join(" ");
            if f.is_empty() {
                // the contract an agent maps the owner's words to (§ 4)
                let c = ctx()?;
                let mut n: BTreeMap<String, usize> = BTreeMap::new();
                for i in c.board.issues(None)?.iter().filter(|i| service::is_task(i)) {
                    for l in i.labels.iter().filter(|l| !l.starts_with("q-") && *l != service::TASK_LABEL) {
                        *n.entry(l.clone()).or_default() += 1;
                    }
                }
                println!("{}", service_fields(&root));
                for h in &c.cfg.has { println!("Has: {h}"); }
                for (l, k) in n { println!("Label: {l} ({k})"); }
                return Ok(());
            }
            set_focus(&root, &f, args.one("request"))?;
            println!("{}", service_fields(&root));
            Ok(())
        }
        "pause" => {
            pause(&root,true)?;
            println!("Paused {}", project_name(&root));
            Ok(())
        }
        "resume" => {
            resume(&root)?;
            println!("{}", service_fields(&root));
            Ok(())
        }
        "permissions" => permissions_cmd(&root, args),
        "resources" => resources_cmd(&root, args),
        "pm" => pm_cmd(&root,args),
        "hold" | "release" => hold_cmd(sub == "hold", args.one("text")),
        "recover" => {
            if args.flags.contains_key("apply") && lifecycle::stop_hold_status(&platform::default_install()).is_object() {return Err("reviewed Stop requires a selected recovery request; bulk apply refused".into());}
            if let Some(text)=args.one("request") {
                let c=ctx()?;
                let request:recovery::Request=serde_json::from_str(text).map_err(|_|"invalid recovery request")?;
                let mut result=recovery::apply(&*c.board,&c.cfg,&request)?;
                if result["receipts"].as_array().is_some_and(|rows|rows.iter().any(|r|r["outcome"]=="accepted")) {result["service"]=recovery::resume_service(&c.cfg,&request.stop);}
                println!("{result}");Ok(())
            }else if args.flags.contains_key("json") && lifecycle::stop_hold_status(&platform::default_install()).is_object() {
                let c=ctx()?;println!("{}",recovery::review(&*c.board,&c.cfg)?);Ok(())
            }else{recover_cmd(&root,args.flags.contains_key("apply"),args.flags.contains_key("json"))}
        },
        "install" => {
            let (cfg, _) = load(&root)?;
            println!("{}",host::install_project(&cfg.install,&root)?);
            // #1555: a root Claude does not trust starts no Claude worker; say so and how, without writing
            if let Some(home)=std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
                for l in runtime::owner::trust_gap(Path::new(&home),&root.canonicalize().map_err(|e|e.to_string())?) {println!("{l}");}
            }
            Ok(())
        }
        _ => Err("usage: taskq service install|run|onboard|focus|pause|resume|capacity|channel|permissions|hold|release|recover|status|restart|stop|uninstall|list".into()),
    }
}

/// Host onboarding (§ 4, #1546): runtime logins and § 8 Access are read and named as owner steps; Claude trust is the
/// only write. Prints gaps only (#1555); idempotent: a ready host prints one `Ready:` line.
fn onboard_cmd(install: &Path) -> Res<()> {
    let mut claude = true;
    let mut gaps = vec![];
    for r in runtime::agents(install).as_array().into_iter().flatten() {
        let rt = r["runtime"].as_str().unwrap_or("?");
        match r["login"].as_str() {
            Some("logged in") => {}
            // a codex-only host is valid: a missing runtime is skipped, not a failure
            Some("not installed") => { claude &= !runtime::of(rt).remote; gaps.push(setup::onboard_skipped(rt, cfg!(windows) && std::env::var_os("SSH_CONNECTION").is_some())) }
            _ => gaps.extend([format!("Login: {}", runtime::agent_text(r)), format!("Next: {}", runtime::relogin(rt))]),
        }
    }
    let p = host::Policy::read_for_hostname(install, hostname);
    let (mode, codex, has) = p.map_or(("dontAsk".to_string(), vec![], vec![]), |p| (p.permission_mode, p.codex, p.has));
    // codex-app is opt-in (§ 9): its refusal is a gap only where `has` lists it
    let opted = |es: Vec<policy::Effective>| es.into_iter().filter(|e| !runtime::of(&e.runtime).app || has.contains(&e.runtime)).collect::<Vec<_>>();
    // Access is per folder: the setup folder for owner sessions, each registered root for its workers
    gaps.extend(setup::onboard_access("setup", &opted(policy::report(install, &setup::dir(install), &mode, &[]))));
    let roots = match host::projects(install) {
        Ok(roots) => roots,
        Err(e) => { gaps.push(format!("Access projects: unknown ({e})")); vec![] }
    };
    for root in &roots { gaps.extend(setup::onboard_access(&root.display().to_string(), &opted(policy::report(install, root, &mode, &codex)))); }
    if claude {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).ok_or("home folder unknown")?;
        gaps.extend(runtime::owner::trust(install, &home, runtime::owner::claude_pids)?);
    }
    if gaps.is_empty() { gaps.push(format!("Ready: {} project(s) on {}; logins, access and trust in place", roots.len(), hostname())); }
    for l in gaps {println!("{l}");}
    Ok(())
}

/// § 4 emergency hold: write or remove `<install>/hold`, then poke every registered project so its service reads it.
#[tracing::instrument(name="hold",skip_all,fields(coreq_operation=if on {"set"} else {"release"}))]
fn hold_cmd(on: bool, reason: Option<&str>) -> Res<()> {
    let install = platform::default_install();
    let path = install.join("hold");
    let _hold=coreq::file_lock::try_lock(&install.join("admission.lock")).map_err(|e|format!("hold busy: {e}"))?;
    lifecycle::release_guard(&install)?;
    if on {
        let reason = reason.unwrap_or("emergency hold");
        one_line("--text", &[reason.to_string()])?;
        platform::write_atomic(&path, &format!("{reason}\n"))?;
    } else if let Err(e) = fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }) {
        return Err(format!("hold file: {e}; still held"));
    }
    let projects = coreq::telemetry::capture::operation("wake.projects", || {
        fs::read_to_string(install.join("projects")).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(String::new()) } else { Err(e) })
    }, |_| "registry-unreadable").map_err(|_| "Hold state saved; project registry unreadable; service notification unknown")?;
    let mut failed = 0;
    for r in projects.lines().filter(|l| !l.is_empty()) {
        if poke(Path::new(r)).is_err() { failed += 1; }
    }
    let notification = if failed == 0 { Ok(()) } else { Err(format!("Hold state saved; {failed} service wake(s) failed; check service status")) };
    if !on {
        println!("Hold released on {}; launches follow each project's focus", hostname());
        return notification;
    }
    println!("Hold on {}: new launches refused until `taskq service release`; live children continue", hostname());
    for o in admission::old_services(&install) {
        println!("Not enforced: {o} is older than {}", admission::MIN_SERVICE);
    }
    notification
}

/// § 4 recovery: list this host's interrupted attempts and their fencing ops; `--apply` writes them through the
/// dispatcher, one receipt each. Only under a hold.
fn recover_cmd(root: &Path, apply: bool, json: bool) -> Res<()> {
    let c = ctx()?;
    let hold = admission::hold(&c.cfg.install).ok_or("no emergency hold on this host; run `taskq service hold` first")?;
    // the service's children, from a live status.json (§ 4); else every row is unknown
    let p = dot(root).join("status.json");
    let kids = match (data::json(&p), file_age(&p)) {
        (Ok(Some(s)), Some(a)) if snapshot::live(a as i64, c.cfg.ttl) => Ok(s["kids"].as_array().cloned().unwrap_or_default()),
        (Err(e), _) => Err(e),
        (Ok(Some(_)), Some(a)) => Err(format!("status.json {a} s old")),
        _ => Err("no service status".to_string()),
    };
    let rows = dispatch::proposal(&*c.board, &c.cfg.me, &kids)?;
    println!("Hold: {hold}");
    if rows.is_empty() {
        println!("No interrupted attempts on {}", c.cfg.me);
    }
    for r in &rows {
        println!("{}", r.text());
    }
    if !apply {
        return Ok(());
    }
    emit(&dispatch::recover(&*c.board, root, &rows), json)
}

#[cfg(any(target_os = "macos", windows))]
fn tray(install: PathBuf, snapshot: Option<&str>, open: bool, routes: &[String], icon: Option<&str>) -> Res<()> {
    if icon.is_some() { return Err("Custom icons were removed; choose Icon or Summary in Tray icon settings".into()); }
    if snapshot.is_none() && !routes.is_empty() {
        return Err("--route needs --snapshot DIR".into());
    }
    tray::run(install, snapshot.map(PathBuf::from), routes.to_vec(), open)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn tray(_install: PathBuf, _snapshot: Option<&str>, _open: bool, _routes: &[String], icon: Option<&str>) -> Res<()> {
    if icon.is_some() { return Err("Custom icons were removed; choose Icon or Summary in Tray icon settings".into()); }
    Err("taskq tray: macOS and Windows only".into())
}

// ---------- commands ----------

/// The fields of one new task (§ 1), from `add` or a recipe (§ 7).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewTask {
    pub title: String,
    pub goal: String,
    pub acceptance: Vec<String>,
    pub scope: Vec<String>,
    pub deps: Vec<u64>,
    pub kind: Option<String>,
    pub runtime: Option<String>,
    pub priority: Option<String>,
    pub host: Option<String>,
    /// Extra labels, e.g. `needs-gate`.
    pub labels: Vec<String>,
}

/// Task body and create claim shared by direct adds and recipes; limits checked before any write.
fn task_body(b: &dyn Board, t: &NewTask, op: &Op) -> Res<(String, Vec<String>)> {
    let title = t.title.chars().count();
    let goal: Vec<String> = t.goal.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let acc: Vec<String> = t.acceptance.iter().enumerate().map(|(i, s)| format!("A{}. {s}", i + 1)).collect();
    let scope: Vec<String> = t.scope.iter().map(|s| format!("- {s}")).collect();
    all([
        if t.title.trim().is_empty() || t.title.contains(['\r', '\n']) { Err("title: one nonempty line".into()) } else { Ok(()) },
        if title > 80 { Err(format!("title: {title} characters, limit 80; shorten it")) } else { Ok(()) },
        limit("Goal", &goal, 1, 2, 240),
        one_line("--acceptance", &t.acceptance),
        labelled_acceptance(&t.acceptance),
        one_line("--scope", &t.scope),
        limit("Acceptance", &acc, 1, 8, 240).map_err(|e| if acc.len() > 8 { format!("{e} (split the task, R2)") } else { e }),
        limit("Scope", &scope, 0, 4, 240),
    ])?;
    let mut body = format!("## Goal\n{}\n\n## Acceptance\n{}\n", goal.join("\n"), acc.join("\n"));
    if !scope.is_empty() {
        body += &format!("\n## Scope\n{}\n", scope.join("\n"));
    }
    if !t.deps.is_empty() {
        body += "\n";
        body += &t.deps.iter().map(|d| format!("Depends on #{d}\n")).collect::<String>();
    }
    for (flag, v, allowed) in [("type", &t.kind, &["code", "docs", "research", "asset"][..]), ("runtime", &t.runtime, &runtime::RUNTIMES[..]), ("priority", &t.priority, &["0", "1", "2"][..])] {
        if v.as_deref().is_some_and(|v| !allowed.contains(&v)) {
            return Err(format!("--{flag} requires one of {}", allowed.join("|")));
        }
    }
    if t.host.as_deref().is_some_and(|v| v.is_empty() || v.contains(',') || v.chars().any(char::is_whitespace)) {
        return Err("--host requires one nonempty host name".into());
    }
    // § 1: a host label no heartbeat names waits forever (#1721: `host-host-mac` sat ready 30 min)
    if let Some(h) = t.host.as_deref() {
        if let Some(name) = h.strip_prefix("host-") {
            return Err(format!("--host takes the host name without host-: --host {name}"));
        }
        let issues = b.issues(None)?;
        let known: Vec<&str> = issues.iter().filter(|i| i.trusted).filter_map(service::host_name).collect();
        if !known.contains(&h) {
            return Err(format!("--host {h}: no host named {h}; hosts: {}", if known.is_empty() { "none".into() } else { known.join(", ") }));
        }
    }
    if t.priority.as_deref() == Some("0") && !b.canonical_labels()? {
        return Err("--priority 0 needs the canonical label schema (§ 9)".into());
    }
    if let Some(id) = &op.id {
        body += &format!("\nop-id: {id}\n");
    }
    // § 1: every new task is held: `q-held` instead of `taskq`, so no service of any version admits it before `start`
    let mut labels = vec![b.state_label("held")?];
    for (v, prefix) in [(&t.kind, ""), (&t.runtime, "run-"), (&t.priority, if b.canonical_labels()? { "p" } else { "priority-" }), (&t.host, "host-")] {
        if let Some(v) = v {
            labels.push(format!("{prefix}{v}"));
        }
    }
    labels.extend(t.labels.iter().cloned());
    labels::validate(&labels)?;
    // the service ignores both `prefer-*` labels (§ 4 Filter); add refuses them
    if labels.iter().filter(|l| l.starts_with("prefer-")).count() > 1 {
        return Err("conflicting prefer-* labels".into());
    }
    Ok((body, labels))
}

fn add_task(b: &dyn Board, root: &Path, raw: &Value, t: &NewTask, op: &Op) -> Res<Receipt> {
    let (body, labels) = task_body(b, t, op)?;
    let n = if op.id.is_some() {
        match claimed_create(b, raw, &t.title, &body, &labels, "", op) {
            Ok(n) => n,
            Err(r) => return Ok(r),
        }
    } else { b.create(&t.title, &body, &labels)? };
    let r = act(b, root, n, "hold", &[], "", &Op::default());
    Ok(Receipt { op_id: op.id.clone(), ..r }.wake(root))
}

/// § 7 recipe run: every task is added held with op id `recipe.<name>.<version>.<run>.<k>` and read back; only then,
/// with `start`, do they start. A rerun returns the first receipts.
fn run_recipe(b: &dyn Board, root: &Path, raw: &Value, name: &str, run: &str, goal: Option<&str>, start: bool) -> Res<Vec<Receipt>> {
    let (version, items) = recipe::load(raw, name, run, goal)?;
    let base = |k: usize| format!("recipe.{name}.{version}.{run}.{}", k + 1);
    let (mut out, mut made) = (vec![], vec![]);
    for (k, item) in items.into_iter().enumerate() {
        let new = NewTask { deps: item.deps.iter().map(|d| made[d - 1]).collect(), ..item.task };
        let r = add_task(b, root, raw, &new, &Op { id: Some(base(k)), revision: None })?;
        let r = if r.ok() && item.gate { act(b, root, r.task, "gate", &[], "", &Op { id: Some(format!("{}.gate", base(k))), revision: None }) } else { r };
        let ok = r.ok();
        made.push(r.task);
        out.push(r);
        if !ok {
            return Ok(out);
        }
    }
    if start {
        out = made.iter().enumerate().map(|(k, n)| start_task(b, root, *n, &Op { id: Some(format!("{}.start", base(k))), revision: None })).collect::<Res<_>>()?;
    }
    Ok(out)
}

/// § 1 `start N [M..]`: every task is read and checked held before the first write, then started in order;
/// with several tasks an op id becomes `<id>.<N>` per task.
fn start_tasks(b: &dyn Board, root: &Path, nums: &[String], op: &Op) -> Res<Vec<Receipt>> {
    let ns: Vec<u64> = nums.iter().map(|s| s.trim_start_matches('#').parse().map_err(|_| format!("not a task number: {s}"))).collect::<Res<_>>()?;
    if ns.is_empty() { return Err("task number missing".into()) }
    if ns.len() > 1 && op.revision.is_some() { return Err("--revision names one task; start several without it".into()) }
    for &n in &ns {
        let issue = b.issue(n)?;
        let held = service::is_held(&issue) || fold(n, &entries(b, n)?, false).state == State::Held;
        // a retried op id may find its task already started; `act` returns the first receipt
        if !issue.trusted || !issue.open || !(held || op.id.is_some()) {
            return Err(format!("#{n} is not a held task; nothing started"));
        }
    }
    ns.iter().map(|&n| {
        let op = if ns.len() > 1 { Op { id: op.id.as_ref().map(|id| format!("{id}.{n}")), revision: None } } else { op.clone() };
        start_task(b, root, n, &op)
    }).collect()
}

/// § 1 `start`: `taskq` goes on after the `start` entry; a retry adds the label a failed write left off.
fn start_task(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Res<Receipt> {
    Ok(start_one(b, root, n, op))
}

/// § 5 receipts: JSON lines with `--json`, else the text lines; any receipt not accepted fails after all are shown.
fn emit(rs: &[Receipt], json: bool) -> Res<()> {
    for r in rs {
        match (json, r.ok()) {
            (true, _) => println!("{}", r.json()),
            (false, true) => println!("{}", r.text()),
            (false, false) => eprintln!("{}", r.text()),
        }
    }
    if rs.iter().all(Receipt::ok) { Ok(()) } else { Err(String::new()) } // already shown: exit 1 quietly
}

fn golden_selftest(traces: &str, published: bool) -> Res<usize> {
    let result = core::golden(traces);
    if published && result.is_err() {
        incident::detect("invariant.golden", concat!("src/main.rs:", line!()), "none", &["fold.json"], &["golden:mismatch"]);
    }
    result
}

/// True once per stuck period: `stuck` held for 2 × `interval` seconds of `elapsed`.
fn restart_watch(since: &mut Option<u64>, stuck: bool, elapsed: u64, interval: u64) -> bool {
    if !stuck { *since = None; return false; }
    let start = *since.get_or_insert(elapsed);
    if elapsed.saturating_sub(start) < interval.saturating_mul(2) { return false; }
    incident::detect("dispatch.reexec", concat!("src/main.rs:", line!()), "none", &[], &["restart:stuck"]);
    *since = Some(u64::MAX);
    true
}

#[tracing::instrument(name="run",skip_all)]
fn run(argv: &[String]) -> Res<()> {
    let a = Args::parse(argv)?;
    telemetry::diagnostic::poll(&platform::default_install());
    if let Some(selector)=a.one("diagnose") {
        let milliseconds=a.one("diagnose-ms").unwrap_or("60000").parse::<u64>().map_err(|_|"invalid --diagnose-ms")?;
        let limit=a.one("diagnose-limit").unwrap_or("256").parse::<u64>().map_err(|_|"invalid --diagnose-limit")?;
        telemetry::diagnostic::select(selector,Duration::from_millis(milliseconds),limit)?;
    } else if a.one("diagnose-ms").is_some() || a.one("diagnose-limit").is_some() {return Err("diagnostic limits require --diagnose".into());}
    if a.pos.is_empty() || a.pos.first().is_some_and(|s| s == "help") || a.pos.iter().any(|s| s == "-h") || a.flags.contains_key("help") {
        println!("{USAGE}");
        return Ok(());
    }
    let cmd = a.pos.first().map(String::as_str).unwrap_or("");
    if std::env::var_os("TASKQ_ROOT").is_some() {
        find_root()?; // § 5: a stale TASKQ_ROOT refuses every command, not only those that read the project
    }
    match cmd {
        "profile" => match a.pos.get(1) {
            Some(v) if v=="migrate" => {
                let count=telemetry::migrate(&platform::default_install())?;
                println!("Migrated {count} legacy records; original receipt clocks and source archive preserved");Ok(())
            }
            Some(v) if v=="diagnose" => {
                let pid=a.need("pid")?.parse().map_err(|_|"invalid --pid")?;
                let period=a.one("duration-ms").unwrap_or("60000").parse().map_err(|_|"invalid --duration-ms")?;
                let limit=a.one("limit").unwrap_or("256").parse().map_err(|_|"invalid --limit")?;
                let request=telemetry::diagnostic::request(&platform::default_install(),pid,a.need("selector")?,period,limit)?;
                println!("Diagnostic selection requested: {request}; verify target status/export; no application acknowledgment yet");Ok(())
            }
            None if a.one("since").is_some() => Err("--since needs a view: taskq profile export|tasks|board --since 24h".into()),
            None => { print!("{}", profile::report(&platform::default_install())); Ok(()) }
            Some(v) => { print!("{}", profile::view(&platform::default_install(), v, a.one("since"))?); Ok(()) }
        },
        "rules" => match &a.pos[1..] {
            [phase, root, role, task, attempt] if phase == "compact" => {
                let n = |s: &String| s.parse::<u64>().map_err(|_| format!("rules compact: {s} is not a number"));
                print!("{}", runtime::compact(Path::new(root), role, n(task)?, n(attempt)?).map_err(|e| e.to_string())?);
                Ok(())
            }
            // a worker or reviewer under the project hook: its rules come with its turn (§ 8)
            [pm, _] if pm == "pm" && std::env::var_os("TASKQ_ROLE").is_some() => Ok(()),
            [pm, phase] if pm == "pm" && (phase == "start" || phase == "compact") => {
                print!("{}", runtime::pm(&find_root()?, phase).map_err(|e| e.to_string())?);
                Ok(())
            }
            _ => Err("usage: taskq rules compact ROOT ROLE TASK ATTEMPT | taskq rules pm start|compact".into()),
        },
        "selftest" => {
            let n = golden_selftest(core::GOLDEN, !platform::VERSION.contains('-')).map_err(|e| format!("fold: FAIL {e}"))?;
            println!("fold: {n} golden traces PASS");
            println!("{}", formats_selftest()?);
            if std::env::var_os("TASKQ_SELFTEST_OFFLINE").is_some() {
                println!("board: unknown (TASKQ_SELFTEST_OFFLINE set; no board read)");
            } else if let Ok(root) = find_root() {
                let c = ctx()?;
                let (views, hosts) = service::views(c.board.as_ref(), &mut Default::default(), &mut Default::default())?;
                let now = c.board.now()?;
                let live = views.iter().filter_map(|v| {
                    let a = v.projection.task.attempt?;
                    let h = service::host_of(&v.projection.task)?;
                    (hosts.iter().any(|hb| hb.title == format!("taskq host {h}")
                        && now - hb.updated <= c.cfg.ttl && hb.body.lines().any(|l| l == format!("#{} {a}", v.issue.number))))
                        .then(|| (c.cfg.repo.clone(), a))
                }).collect();
                println!("{}", profile::selftest(&c.cfg.install, &c.cfg.repo, &live, c.cfg.ttl.max(0) as u64)?);
                let open = views.len();
                println!("board: {} as {} on {}", c.cfg.repo, c.board.me()?, c.cfg.web_host);
                println!("open issues: {open}");
                println!("host: {}", c.cfg.me);
                println!("root: `{}`", root.display());
            } else {
                println!("board: unknown (no project config found; no board read)");
            }
            let capture=coreq::telemetry::capture::qualify().map_err(|e|format!("telemetry: FAIL {e}"))?;
            // #1419: entry.rs gave the offline (staged release) selftest a private store.
            if std::env::var_os("TASKQ_SELFTEST_OFFLINE").is_some() {println!("telemetry: staged capture committed {} observations (private store; running writer not used)",capture["committed"]);}
            else {println!("telemetry: local capture committed {} observations",capture["committed"]);}
            Ok(())
        }
        "update" => {
            let install = platform::default_install();
            let check = a.flags.contains_key("check");
            println!("{}", incident::disclosure(find_root().ok().as_deref()));
            match platform::update(&install, check)? {
                Some(v) if check => println!("update available: {} -> {v}", platform::tool(&install).current().unwrap_or_else(|| platform::VERSION.into())),
                Some(v) => println!("updated to {v}; the service restarts on its next update check"),
                None => println!("up to date ({})", platform::tool(&install).current().unwrap_or_else(|| platform::VERSION.into())),
            }
            Ok(())
        }
        // § 5: read-only probes on this host, no board
        "agents" => {
            for r in runtime::agents(&platform::default_install()).as_array().into_iter().flatten() { println!("{}", runtime::agents_line(r)); }
            Ok(())
        }
        "service" => service(&a),
        // § 5: an owner session (§ 4) in this project; no board
        "retro" => {
            let (cfg, _) = load(&find_root()?)?;
            let r = setup::open(&cfg.install, &cfg.root, setup::RETRO)?;
            println!("retro: request {} pending; the host service opens a new session in {}", r.id, cfg.root.display());
            Ok(())
        }
        "tray" => tray(platform::default_install(), a.one("snapshot"), a.one("open").is_some(), &a.all("route").iter().flat_map(|r| r.split(',')).map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect::<Vec<_>>(), a.one("icon")),
        "" => Err("command missing; use `taskq help`".into()),
        _ => command(&ctx()?, cmd, &a),
    }
}

/// §1 create claim: only the invocation that reads back its winning claim may POST an issue.
fn claimed_create(b: &dyn Board, raw: &Value, title: &str, body: &str, labels: &[String], later_text: &str, op: &Op) -> Result<u64, Receipt> {
    let id = op.id.as_deref().expect("claimed create needs an op id");
    let unknown = |n, reason: String| Receipt {
        outcome: "unknown", next: "Retry the identical add with the same --op-id for read-only discovery.".into(),
        ..Receipt::rejected(n, op, reason)
    };
    let read = |e| unknown(0, e);
    let claim_issue = raw["create_claim_issue"].as_u64().filter(|n| *n > 0)
        .ok_or_else(|| Receipt::rejected(0, op, "configure create_claim_issue before idempotent create"))?;
    let issue = b.issue(claim_issue).map_err(read)?;
    if !issue.trusted || service::is_task(&issue) || service::is_held(&issue) || issue.labels.iter().any(|l| l == "taskq-host") {
        return Err(Receipt::rejected(0, op, "create_claim_issue must be trusted and outside the queue/hosts"));
    }
    let request = json!({"actor": b.me().map_err(read)?, "title": title, "body": body, "labels": labels, "later_text": later_text}).to_string();
    let hash: String = ring::digest::digest(&ring::digest::SHA256, request.as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let header = format!("taskq: create-claim {id}");
    let claim = format!("{header}\nrequest-sha256: {hash}");
    let first = || -> Res<Option<board::Comment>> {
        Ok(b.comments(claim_issue)?.into_iter().filter(|c| c.trusted && !c.edited && c.body.lines().next() == Some(header.as_str())).min_by_key(|c| c.id))
    };
    let discover = || -> Result<Receipt, Receipt> {
        let marker = format!("\nop-id: {id}");
        let matches: Vec<_> = b.issues(Some(0)).map_err(read)?.into_iter().filter(|i| i.body.trim_end().ends_with(&marker)).collect();
        if matches.len() != 1 {
            return Err(unknown(0, "create claim exists; issue absent or duplicated, no further POST allowed".into()));
        }
        let i = b.issue(matches[0].number).map_err(read)?;
        if !i.trusted || i.title != title || i.body != body {
            return Err(unknown(i.number, "created issue conflicts with the immutable request".into()));
        }
        let log = entries(b, i.number).map_err(read)?;
        let projection = core::project(i.number, &log, deps_open(b, i.number, &i.body, &log));
        let state = if !i.open { State::Closed } else if service::is_held(&i) { State::Held } else { projection.task.state };
        Ok(Receipt::accepted(i.number, op, state, projection.revision))
    };
    if let Some(first) = first().map_err(read)? {
        if first.body != claim {
            return Err(Receipt::rejected(0, op, "op id already binds a different request or actor"));
        }
        return Err(discover().unwrap_or_else(|r| r));
    }
    // Legacy op ids cannot be upgraded by blindly creating another issue.
    let marker = format!("\nop-id: {id}");
    if b.issues(Some(0)).map_err(read)?.iter().any(|i| i.body.trim_end().ends_with(&marker)) {
        return Err(Receipt::rejected(0, op, "existing issue has no create claim; owner reconciliation required"));
    }
    let own = b.comment(claim_issue, &claim).map_err(read)?;
    let winner = first().map_err(read)?.ok_or_else(|| unknown(0, "claim missing on readback; no create POST allowed".into()))?;
    if winner.body != claim {
        return Err(Receipt::rejected(0, op, "op id already binds a different request or actor"));
    }
    if winner.id != own {
        return Err(discover().unwrap_or_else(|r| r));
    }
    let n = b.create(title, body, labels).map_err(read)?;
    let issue = b.issue(n).map_err(|e| unknown(n, e))?;
    if !issue.trusted || issue.title != title || issue.body != body || issue.labels.len() != labels.len() || labels.iter().any(|l| !issue.labels.contains(l)) {
        return Err(unknown(n, "create readback does not match the request".into()));
    }
    Ok(n)
}

/// Section 1 finding -> existing claim/create -> separate admission; drafts remain caller-owned.
fn finding_submit(b: &dyn Board, raw: &Value, root: &Path, title: &str, body: &str, labels: &[String], request: &str, op: &Op, start: bool) -> (Receipt, Option<Receipt>) {
    let create = match claimed_create(b, raw, title, body, labels, request, op) {
        Ok(n) => Receipt::accepted(n, op, State::Held, 0),
        Err(r) if r.outcome == "unknown" => Receipt { next: "Retry the identical finding command for read-only create discovery; optional start has its own receipt.".into(), ..r },
        Err(r) => r,
    };
    if !create.ok() || !start { return (create, None) }
    let n = create.task;
    let admission = Op { id: Some(format!("{}.start", op.id.as_deref().unwrap())), revision: None };
    let r = start_one(b, root, n, &admission);
    (create, Some(r))
}

/// Section 1 start log -> eligibility label -> authoritative readback, shared by start and finding.
fn start_one(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Receipt {
    let mut start_op = op.clone();
    // A finding's q-held projection prevents admission even before its hold log is initialized.
    let initialize = || -> Res<Option<Receipt>> {
        let issue = b.issue(n)?;
        let log = entries(b, n)?;
        let revision = core::project(n, &log, false).revision;
        if op.revision.is_some_and(|r| r != revision) { return Ok(None) }
        if issue.open && service::is_held(&issue) && revision == 0 {
            let hold = Op { id: op.id.as_ref().map(|id| format!("{id}.hold")), revision: Some(revision) };
            let r = act(b, root, n, "hold", &[], "", &hold);
            return Ok(Some(Receipt { op_id: op.id.clone(), ..r }));
        }
        Ok(None)
    };
    match initialize() {
        Ok(Some(r)) if !r.ok() => return r,
        Ok(Some(r)) => { if start_op.revision.is_some() { start_op.revision = r.revision; } }
        Err(e) => return Receipt { outcome: "unknown", ..Receipt::rejected(n, op, e) },
        Ok(None) => {}
    }
    let r = act(b, root, n, "start", &[], "", &start_op);
    if !r.ok() { return r }
    let readback = || -> Res<Receipt> {
        let (issue, t, _) = current(b, n)?;
        let log = entries(b, n)?;
        let started = log.iter().any(|e| !e.edited && e.body.starts_with("taskq: start"));
        if started && issue.open { service::project(b, &issue, &t)?; }
        if started && issue.open && !service::is_task(&issue) {
            b.labels(n, &[service::TASK_LABEL.into()], &[])?;
        }
        let (issue, t, _) = current(b, n)?;
        if issue.open && (!service::is_task(&issue) || service::is_held(&issue)) {
            return Err("start eligibility labels not read back".into());
        }
        Ok(Receipt::accepted(n, op, if issue.open { t.state } else { State::Closed }, core::revision(n, &entries(b, n)?)))
    };
    readback().unwrap_or_else(|e| Receipt { outcome: "unknown", next: format!("Read taskq inbox {n}; retry the same start operation."), ..Receipt::rejected(n, op, e) })
}

fn emit_finding(create: Receipt, start: Option<Receipt>, json: bool) -> Res<()> {
    let ok = create.ok() && start.as_ref().is_none_or(Receipt::ok);
    if json {
        println!("{}", json!({"create": create, "start": start}));
    } else {
        println!("Create: {}", create.text());
        if let Some(r) = start { println!("Start: {}", r.text()); }
    }
    if ok { Ok(()) } else { Err(String::new()) }
}

fn command(c: &Ctx, cmd: &str, a: &Args) -> Res<()> {
    let (b, cfg, root) = (c.board.as_ref(), &c.cfg, c.cfg.root.as_path());
    let text = a.one("text").unwrap_or("");
    if matches!(cmd, "requeue" | "later" | "failed" | "steer") || (cmd == "answer" && !a.pos.get(1).is_some_and(|p| p.contains('.'))) {
        task_text(cmd, text)?;
    }
    if cmd == "add" && a.flags.contains_key("later") { task_text("later", text)?; }
    let revision = a.one("revision").map(|r| r.parse::<u64>().map_err(|_| format!("bad --revision {r}"))).transpose()?;
    let mut op = Op { id: a.one("op-id").map(String::from), revision };
    if op.id.as_deref().is_some_and(|i| i.is_empty() || i.contains(char::is_whitespace)) {
        return Err("--op-id: one word".into());
    }
    let json = a.flags.contains_key("json");
    if revision.is_some() && (cmd == "close" && a.pos.len() > 2 || cmd == "answer" && a.pos.len() > 2) {
        return Err("--revision names one task".into());
    }
    match cmd {
        "labels" => {
            let mut names = labels::FIXED.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            names.extend(b.issues(None)?.into_iter().flat_map(|i| i.labels));
            b.ensure_labels(&names)?;
            println!("Label catalog verified for {}", project_name(root));
            Ok(())
        }
        "add" | "finding" => {
            let finding = cmd == "finding";
            // § 1 task body: Goal, Acceptance, Scope, dependencies; limits checked before any write
            let title = a.pos.get(1).filter(|t| !t.trim().is_empty()).ok_or("title missing")?;
            let deps: Vec<u64> = a.all("deps").iter().flat_map(|d| d.split([',', ' '])).filter(|d| !d.is_empty()).map(|d| d.trim_start_matches('#').parse().map_err(|_| format!("bad --deps {d}"))).collect::<Res<_>>()?;
            // § 9: work waits for its runtime; a soft preference is retired
            if a.flags.contains_key("later") {
                return Err("--later is retired: every add is held (§ 1); park it with `taskq later N`".into());
            }
            if a.flags.contains_key("prefer") {
                return Err("--prefer is retired; use --runtime codex for design, UI, visual and image work (none: claude)".into());
            }
            // a repeated single-value flag is refused before any write; `one` would keep only the last value
            let mut repeated: Vec<_> = a.flags.iter().filter(|(f, v)| v.len() > 1 && !["acceptance", "scope", "deps", "evidence"].contains(&f.as_str())).map(|(f, _)| format!("--{f}")).collect();
            repeated.sort();
            if !repeated.is_empty() {
                return Err(format!("{} given more than once; pass each once (a two-line --goal uses one value with a newline)", repeated.join(", ")));
            }
            let new = NewTask {
                title: title.clone(),
                goal: a.need("goal")?.to_string(),
                acceptance: a.all("acceptance"),
                scope: a.all("scope"),
                deps,
                kind: a.one("type").map(String::from),
                runtime: a.one("runtime").map(String::from),
                priority: a.one("priority").map(String::from),
                host: add_host(a.one("host"), cfg),
                labels: vec![],
            };
            if finding {
                let (mut body, labels) = task_body(b, &new, &Op::default())?;
                let finding_request;
                for (flag, values) in &a.flags {
                    if !["board", "source", "goal", "acceptance", "scope", "deps", "type", "runtime", "priority", "host", "evidence", "start", "json"].contains(&flag.as_str()) {
                        return Err(format!("finding does not accept --{flag}"));
                    }
                    if !["acceptance", "scope", "deps", "evidence"].contains(&flag.as_str()) && values.len() != 1 {
                        return Err(format!("finding --{flag} requires one value"));
                    }
                }
                let board_url = b.url(0);
                let board_url = board_url.trim_end_matches("/0");
                if a.all("board").len() != 1 || a.need("board")? != board_url {
                    return Err("finding --board must match the configured board URL from taskq status --json".into());
                }
                let source = a.need("source")?;
                let parts: Vec<_> = source.split('/').collect();
                if a.all("source").len() != 1 || parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.bytes().all(|c| c == b'.') || p.len() > 64 || !p.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))) {
                    return Err("--source: installation/rule/episode; each part 1-64 ASCII letters, digits, dot, underscore or hyphen".into());
                }
                let evidence: Vec<_> = a.all("evidence").into_iter().map(|e| format!("- {e}")).collect();
                one_line("--evidence", &evidence)?;
                limit("Evidence", &evidence, 0, 4, 240)?;
                let identity = json!({"kind": cfg.board.name, "host": cfg.web_host, "repo": cfg.repo, "url": board_url, "source": source});
                let hash: String = ring::digest::digest(&ring::digest::SHA256, identity.to_string().as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
                op.id = Some(format!("finding.{hash}"));
                body += &format!("\n## Source\n{source}\n");
                if !evidence.is_empty() { body += &format!("\n## Evidence\n{}\n", evidence.join("\n")); }
                finding_request = json!({"board": identity, "start": a.flags.contains_key("start")}).to_string();
                body += &format!("\nop-id: {}\n", op.id.as_deref().unwrap());
                let (create, start) = finding_submit(b, &c.raw, root, title, &body, &labels, &finding_request, &op, a.flags.contains_key("start"));
                let create = if create.ok() { create.wake(root) } else { create };
                return emit_finding(create, start, json);
            } else if ["start", "source", "board", "evidence"].iter().any(|f| a.flags.contains_key(*f)) {
                return Err("--start/--source/--board/--evidence require finding".into());
            }
            emit(&[add_task(b, root, &c.raw, &new, &op)?], json)
        }
        "start" => emit(&start_tasks(b, root, &a.pos[1..], &op)?, json),
        "approve" => emit(&[dispatch::approve(b, root, a.num(1)?, a.one("sha"), &op, dispatch::agent())], json),
        "recipe" => {
            let name = a.pos.get(1).ok_or("recipe name missing")?;
            emit(&run_recipe(b, root, &c.raw, name, a.need("run")?, a.one("goal"), a.flags.contains_key("start"))?, json)
        }
        "ask" => {
            let n = a.num(1)?;
            let (links, images) = (a.all("link"), a.all("image"));
            if links.iter().any(|l| l.starts_with('!')) { return Err("--link takes links; images go through --image".into()); }
            // validate the card and the files before any upload
            ask_card(n, a.need("text")?, &a.all("option"), a.one("recommend"), &links)?;
            image_files(&images)?;
            let body = ask_card(n, a.need("text")?, &a.all("option"), a.one("recommend"), &[links, upload_images(b, n, &images)?].concat())?;
            emit(&[act(b, root, n, "ask", &[], &body, &op)], json)
        }
        "answer" if a.pos.get(1).is_some_and(|p| p.contains('.')) => {
            emit(&answer_picks(b, cfg, &a.pos[1..], &op)?, json)
        }
        "pick" => emit(&[dispatch::pm_pick(b, root, a.pos.get(1).ok_or("N.K missing")?, text, &op)?], json),
        "answer" => {
            emit(&dispatch::answer_text(b, cfg, a.num(1)?, a.need("text")?, &op)?, json)
        }
        "result" => {
            let n = a.num(1)?;
            let sha = a.need("sha")?;
            let (links, images) = (a.all("link"), a.all("image"));
            if links.iter().any(|l| l.starts_with('!')) { return Err("--link takes links; images go through --image".into()); }
            result_text(n, text, &a.all("checks"), &a.all("option"), a.one("recommend"), &links)?;
            image_files(&images)?;
            shots_gate("git", root, sha)?;
            let body = result_text(n, text, &a.all("checks"), &a.all("option"), a.one("recommend"), &[links, upload_images(b, n, &images)?].concat())?;
            emit(&[act(b, root, n, "result", &[sha], &body, &op)], json)
        }
        "requeue" => {
            let n = a.num(1)?;
            emit(&[act(b, root, n, cmd, &[], text, &op)], json)
        }
        "later" => {
            let n = a.num(1)?;
            emit(&[act(b, root, n, cmd, &park(n, a.one("blocker"), a.one("until"), std::env::var("TASKQ_TASK").ok(), std::env::var("TASKQ_ATTEMPT").ok())?, text, &op)], json)
        }
        "steer" | "failed" => {
            let n = a.num(1)?;
            emit(&[act(b, root, n, cmd, &[], a.need("text")?, &op)], json)
        }
        "close" => {
            let mut out = vec![];
            for i in 1..a.pos.len() {
                let n = a.num(i)?;
                out.push(close_checked(b, cfg, n, text, &op, &|issue, t| close_check(text, issue, t, b.defer_publication(&cfg.publish))));
            }
            emit(&out, json)
        }
        // docs/design/enrollment.md: observation only, no task answers or commands; the board stays the truth
        "enroll" => {
            let url = a.pos.get(1).ok_or("enroll needs the DashQ https URL")?;
            let board = enrollment::BoardScope::new(cfg.board.name, &cfg.web_host, &cfg.repo)?;
            let dest = enrollment::destination(url)?;
            if let Some(old) = enrollment::active(&cfg.install)?.filter(|o| o["destination"] != dest.as_str() || enrollment::approved(o).as_ref() != Some(&board)) {
                println!("switch: {} {} -> {dest} {}", old["destination"].as_str().unwrap_or("?"), enrollment::approved(&old).map_or("unknown".into(), |b| b.value()), board.value());
            }
            println!("board scope: {} {}", enrollment::SCHEMA, board.value());
            println!("grant: observation only; no task answers or commands");
            let telemetry = a.flags.contains_key("telemetry");
            if telemetry { println!("telemetry: {} (local telemetry rows; revoke with taskq enroll without --telemetry or taskq disconnect)", telemetry_sync::SCHEMA); }
            let conn = enrollment::pair(&cfg.install, url, &cfg.me, &board, !a.flags.contains_key("headless"), telemetry).map_err(|e| format!("pairing: {e}\nNext: rerun taskq enroll {url} and approve the new code"))?;
            // the CLI runs no runtime probe: login reads unknown until the service's next send
            let local = enrollment::Local { host: cfg.me.clone(), service: enrollment::installation_id(&cfg.install).ok().map(|i| service_id(&i)), ttl: cfg.ttl.max(0) as u64, agents: Value::Null, progress: Default::default() };
            let (_, rec) = enrollment::record(&cfg.install, &snapshot::read(b, cfg, &c.raw, &[]), &conn, &local)?;
            match enrollment::first_receipt(&cfg.install, &rec, 15)? {
                "projected" => println!("connected: {dest} workspace {} generation {}; first snapshot projected", conn["workspace"].as_str().unwrap_or("?"), conn["generation"]),
                s => return Err(format!("approved, first snapshot {s}, not yet projected\nNext: taskq connection status")),
            }
            Ok(())
        }
        "connection" if a.pos.get(1).is_some_and(|p| p == "status") => {
            let st = enrollment::state(&cfg.install)?;
            let st = st.as_ref().unwrap_or(&Value::Null);
            let (act, pend) = (&st["active"], &st["pending"]);
            match (act.is_object(), pend.is_object()) {
                (true, _) => println!("connection: active"),
                (false, true) => println!("connection: pending approval"),
                _ => println!("connection: none"),
            }
            let c = if act.is_object() { act } else { pend };
            if c.is_object() {
                println!("destination: {}", c["destination"].as_str().unwrap_or("?"));
                println!("board scope: {} {}", c["scope"]["name"].as_str().unwrap_or("none"), c["scope"]["value"].as_str().unwrap_or(""));
            }
            if act.is_object() {
                println!("workspace: {}  generation: {}", act["workspace"].as_str().unwrap_or("?"), act["generation"]);
                println!("grant: observation only; no task answers or commands");
            }
            if let Some(id) = st["installation"].as_str() { println!("installation: {id}"); }
            let n = st["revoke"].as_array().map_or(0, Vec::len);
            if n > 0 { println!("revokes pending: {n}"); }
            let local = dot(root).join("status.json");
            let report = data::json(&local).ok().flatten().filter(|_| snapshot::status_live(&local, cfg.ttl));
            match report.as_ref().map(|r| &r["connection"]) {
                Some(Value::String(o)) => println!("delivery: {o}"),
                Some(_) => println!("delivery: unknown (the service has not tried yet)"),
                None => println!("delivery: unknown (no fresh service report for this project)"),
            }
            if act.is_object() { println!("telemetry: {}", telemetry_line(act, report.as_ref().map(|r| &r["telemetry_sync"]))); }
            Ok(())
        }
        "disconnect" => {
            println!("{}", enrollment::disconnect(&cfg.install)?.as_deref().unwrap_or("connection: none"));
            Ok(())
        }
        "status" => {
            // R6: a board rate-limit pause the local service reports comes first, also when this read fails
            let local = dot(&cfg.root).join("status.json");
            if let Some(w) = data::json(&local).ok().flatten().filter(|_| snapshot::status_live(&local, cfg.ttl)).as_ref().and_then(requests::warning) {
                println!("{w}");
            }
            if a.flags.contains_key("closed") {
                let page = a.one("page").map_or(Ok(1), |p| p.parse::<u32>().map_err(|_| "bad --page"))?;
                let since = a.one("since").map(|t| t.parse::<i64>().map_err(|_| "bad --since: board seconds")).transpose()?;
                let value = if let Some(n) = a.one("task") {
                    let n = n.parse::<u64>().ok().filter(|n|*n>0).ok_or("bad --task")?;
                    if a.flags.contains_key("transcript") {
                        let before = a.one("before").map(|v|v.parse::<u64>().map_err(|_|"bad --before")).transpose()?;
                        json!({"transcript":closed_transcript(b,n,&cfg.me,before).map_or_else(|reason|json!({"reason":reason}),|v|v)})
                    } else { closed_pin(b,n,cfg.board.name=="gitlab")? }
                } else { closed_history(b,page,since,cfg.board.name=="gitlab")? };
                println!("{value}");
                return Ok(());
            }
            let snap = snapshot::read(b, cfg, &c.raw, &a.all("only").iter().flat_map(|x| x.split(',')).map(String::from).collect::<Vec<_>>());
            if a.flags.contains_key("json") {
                println!("{}", serde_json::to_string_pretty(&snap.json()).map_err(|e| e.to_string())?);
            } else {
                println!("{}", snap.text(cfg, &c.raw)?);
            }
            Ok(())
        }
        "cleanup" => {
            // a child the local service still runs keeps its worktree, whatever the board says
            let st: Value = fs::read_to_string(dot(root).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let live: Vec<u64> = st["kids"].as_array().into_iter().flatten().filter_map(|k| k["task"].as_u64()).collect();
            let t = tidy::Tidy { board: b, root, me: &cfg.me, boot: platform::boot_time, external: cfg.external, force: a.flags.contains_key("force") };
            let log = t.run(&mut |n| live.contains(&n), &|_| None).log;
            println!("{}", if log.is_empty() { "nothing to clean".into() } else { log.join("\n") });
            Ok(())
        }
        "inbox" => {
            println!("{}", inbox(b, a.num(1)?)?);
            Ok(())
        }
        "wait" => {
            let w = a.one("window").map(|w| w.parse().map_err(|_| "bad --window")).transpose()?.unwrap_or(10);
            for l in wait(b, cfg, w, &a.all("only").iter().flat_map(|x| x.split(',')).map(String::from).collect::<Vec<_>>())? {
                println!("{l}");
            }
            Ok(())
        }
        _ => Err(format!("unknown command `{cmd}`; use `taskq help`")),
    }
}

#[tracing::instrument(name="application",skip_all)]
fn application() {
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("__windows-security-restore") {
        if let Err(e) = windows_sandbox::restore(&std::env::args().skip(2).collect::<Vec<_>>()) { eprintln!("{e}"); crate::platform::exit(1); }
        return;
    }
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("__windows-codex") {
        if let Err(e) = windows_sandbox::run(&std::env::args().skip(2).collect::<Vec<_>>()) { eprintln!("{e}"); crate::platform::exit(1); }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::current_exe().ok().is_some_and(|p| p == Path::new(linux_sandbox::HELPER)) {
        if let Err(e) = linux_sandbox::run(&std::env::args().skip(1).collect::<Vec<_>>()) { eprintln!("{e}"); crate::platform::exit(1); }
        return;
    }
    incident::install_panic_hook();
    #[allow(unused_mut)]
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `<install>/taskq` is the launcher: it runs the current release (coreq.md § Launcher).
    let install = platform::default_install();
    let exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());
    let t = platform::tool(&install);
    let launcher = install.join(t.exe_name()).canonicalize().ok();
    if exe.is_some() && exe == launcher && t.current().is_some() {
        // #1566: CoreQ's Windows launcher ends a CLI call through `ExitProcess`, the TLS-teardown hang `platform::exit` avoids.
        // M15: the launcher is the Windows app. No arguments on its own console (Explorer, Start menu, taskbar pin) opens
        // TaskQ without a console window; the tray runs in this process, so its exe path, and the pins keyed by it,
        // stay the same across updates. ponytail: a launcher an update could not rewrite runs its older tray until the
        // tray's version check ends it.
        #[cfg(windows)]
        if args.is_empty() && platform::own_console() { args = vec!["tray".into(), "--open".into()]; }
        #[cfg(windows)]
        if args.first().map(String::as_str) != Some("tray") && !(args.first().map(String::as_str) == Some("service") && args.get(1).map(String::as_str) == Some("run")) {
            let v = coreq::launcher::pick(&t, false).unwrap_or_else(|e| platform::die(&e));
            let status = std::process::Command::new(t.release_exe(&v)).args(&args).env(t.env("INSTALL"), &t.install).status();
            platform::exit(status.map_or_else(|e| platform::die(&format!("start {v}: {e}")), |s| s.code().unwrap_or(1)));
        }
        #[cfg(not(windows))]
        coreq::launcher::launch(&t, &args);
        #[cfg(windows)]
        if args.first().map(String::as_str) != Some("tray") { coreq::launcher::launch(&t, &args); }
    }
    #[cfg(target_os = "macos")]
    if args.is_empty()&&exe.as_deref().is_some_and(|exe|bundled_tray(exe,&install)) {
        profile::init(&install);
        if let Err(e) = tray(install, None, false, &[], None) {
            platform::die(&e);
        }
        return;
    }
    profile::init(&install);
    let r = run(&args);
    // #1351: the command already took effect; hand queued receipts to the writer before the exit flush, which has the same bound.
    telemetry::settle(Duration::from_secs(10));
    // § 4 budget: this process's board requests by source, for `taskq profile`
    if let Some(source) = requests::source(&args, std::env::var("TASKQ_ROLE").ok().as_deref()) {
        requests::flush(&source);
    }
    if let Err(e) = r {
        if e.is_empty() {
            crate::platform::exit(1);
        }
        platform::die(&e);
    }
}

#[cfg(test)]
fn with_test_capture<T>(run: impl FnOnce(&coreq::telemetry::Store, &Path) -> T) -> T {
    use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
    use tracing_subscriber::prelude::*;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir=std::env::temp_dir().join(format!("taskq-command-test-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    fs::create_dir_all(&dir).unwrap();
    let store=Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
    let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
    let result=tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()),||run(&store, &dir));
    telemetry::settle(Duration::from_secs(30));capture.flush().unwrap();
    drop(capture);drop(store);
    // #1640: Windows can keep a just-closed store file busy (os error 32) for a moment; a lasting hold still fails.
    for left in (0..50).rev() {match fs::remove_dir_all(&dir) {Err(e) if left>0&&e.raw_os_error()==Some(32)=>std::thread::sleep(Duration::from_millis(100)),other=>{other.unwrap();break}}}
    result
}

/// `taskq connection status` telemetry line (docs/design/standalone-telemetry.md#status-a5); unknown stays unknown.
fn telemetry_line(active: &Value, report: Option<&Value>) -> String {
    if !telemetry_sync::granted(active) { return format!("off (grant lacks {}; taskq enroll <url> --telemetry)", telemetry_sync::SCHEMA) }
    let Some(r) = report.filter(|r| r["state"].is_string()) else { return "unknown (no fresh service report for this project)".into() };
    let n = |k: &str| r[k].as_u64().map_or("unknown".into(), |v| v.to_string());
    let mut out = r["state"].as_str().unwrap_or("unknown").to_string();
    if let Some(e) = r["reason"].as_str().or(r["last_error"].as_str()) { out += &format!(" ({e})") }
    if r["state"] != "off" {
        out += &format!("; last synced {}; backlog {} rows, {} bytes; gap {} rows", r["last_synced"].as_u64().map_or("never".into(), |t| t.to_string()), n("backlog_rows"), n("backlog_bytes"), n("gap_rows"));
    }
    out
}

#[cfg(test)]
mod tests {
    // Chain: § 7 rule 3 worker park (#1486, #1823). Risk: a worker parks without a blocker or time (no automatic exit), an
    // owner park starts needing one, or a malformed time reaches the board.
    #[test]
    fn worker_park_needs_one_blocker_or_time() {
        let w = || (Some("7".to_string()), Some("42".to_string()));
        let (t, a) = w();
        assert!(super::park(7, None, None, t, a).unwrap_err().contains("--blocker"));
        for bad in ["#7", "#x", "12", "#1 #2"] { let (t, a) = w(); assert!(super::park(7, Some(bad), None, t, a).is_err(), "{bad}"); }
        let (t, a) = w();
        assert_eq!(super::park(7, Some("dashq#36"), None, t, a).unwrap(), ["dashq#36"]);
        assert!(super::park(7, None, None, None, None).unwrap().is_empty());
        assert!(super::park(7, None, None, Some("8".into()), Some("42".into())).unwrap().is_empty(), "another task's env is not this task's worker");
        assert!(super::park(7, Some("#12"), None, None, None).is_err());
        let (t, a) = w();
        assert_eq!(super::park(7, None, Some("2026-11-05T09:00:00Z"), t, a).unwrap(), ["until", "2026-11-05T09:00:00Z"], "a worker waiting only for time parks with it");
        assert_eq!(super::park(7, None, Some("2026-11-05T09:00:00Z"), None, None).unwrap(), ["until", "2026-11-05T09:00:00Z"]);
        assert!(super::park(7, None, Some("2026-11-05"), None, None).unwrap_err().contains("bad --until"));
        let (t, a) = w();
        assert!(super::park(7, Some("#12"), Some("2026-11-05T09:00:00Z"), t, a).unwrap_err().contains("exclude"));
    }

    use super::*;
    use board::Fake;
    use dispatch::close_one;

    // Defect #1445: kids from before a reboot held startup (Windows job recovery fails). Risk: a live kid retired.
    #[test]
    fn kids_from_a_previous_boot_are_retired_and_live_kids_kept() {
        let root = std::env::temp_dir().join(format!("taskq-preboot-{}", runtime::uuid()));
        let install = root.join("install");
        fs::create_dir_all(&install).unwrap();
        let mut gone = if cfg!(windows) { std::process::Command::new("cmd").args(["/c", "exit"]).spawn() } else { std::process::Command::new("true").spawn() }.unwrap();
        let pid = gone.id(); gone.wait().unwrap();
        let old = if cfg!(target_os = "linux") { "00000000-0000-0000-0000-000000000000-1" } else if cfg!(windows) { "1" } else { "1-0" };
        let mut live = if cfg!(windows) { std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn() } else { std::process::Command::new("sleep").arg("30").spawn() }.unwrap();
        let me = coreq::process::identity(live.id()).unwrap().unwrap();
        let kid = |task, process| data::KidLine { task, attempt: 1, runtime: "codex".into(), process, execution: task, offset: 0, job: None };
        let stale = kid(1, coreq::process::Identity { pid, started: old.into() });
        assert!(pre_boot(&stale));
        assert!(!pre_boot(&kid(2, me.clone())), "a process of this boot is kept");
        write_kids(&install, &root, &[stale, kid(2, me.clone())]).unwrap();
        let mut pending = kids_path(&install, &root).unwrap().into_os_string(); pending.push(".pending");
        fs::write(&pending, [0u8; 8]).unwrap();
        assert_eq!(retire_pre_boot(&install, &root).unwrap(), 1);
        assert_eq!(kids_file(&install, &root).unwrap(), vec![kid(2, me)]);
        assert!(!Path::new(&pending).exists(), "a torn pending file cannot block later checkpoints");
        live.kill().unwrap(); live.wait().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removed_custom_icon_flag_refuses_before_reading_file_or_opening_tray() {
        let error=tray(PathBuf::from("invented-unused-install"),None,false,&[],Some("invented-missing-icon.json")).unwrap_err();
        assert!(error.contains("Custom icons were removed"),"{error}");
    }

    // #1801: the release bundle id belongs to the OS default install only; every other install kind takes the test id.
    #[test]
    fn tray_bundle_id_per_install_kind() {
        assert_eq!(tray_id(true),"org.taskq.tray");
        assert_eq!(tray_id(false),"org.taskq.tray.test");
        let home=PathBuf::from("/invented/home/.local/share/taskq");
        for (install,want) in [(home.clone(),"org.taskq.tray"),(std::env::temp_dir().join("taskq-evidence"),"org.taskq.tray.test"),(PathBuf::from("/invented/dev-install"),"org.taskq.tray.test")] {
            assert_eq!(tray_id(platform::is_home(&install,Some(&home))),want,"{}",install.display());
        }
    }

    // #1347: packaging must retain all native sizes and a valid signature under the existing identity.
    #[cfg(target_os = "macos")]
    #[test]
    fn app_bundle_icon_resources_preserve_signature() {
        let dir=std::env::temp_dir().join(format!("taskq-app-icon-{}",runtime::uuid()));
        let app=tray_app(&dir,&std::env::current_exe().unwrap()).expect("signed tray bundle");
        let plist=fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        // #1801: a temp install never takes the release bundle id
        assert!(plist.contains("<string>org.taskq.tray.test</string>"),"{plist}");
        let bytes=fs::read(app.join("Contents/Resources/TaskQ.icns")).unwrap();
        let mut sizes=Vec::new(); let mut offset=8;
        while offset<bytes.len() {
            let len=u32::from_be_bytes(bytes[offset+4..offset+8].try_into().unwrap()) as usize;
            let decoder=png::Decoder::new(std::io::Cursor::new(&bytes[offset+8..offset+len]));
            let reader=decoder.read_info().unwrap();
            sizes.push(reader.info().width);
            assert_eq!(reader.info().width,reader.info().height);
            offset+=len;
        }
        assert_eq!(sizes,vec![16,32,64,128,256,512,1024]);
        assert!(std::process::Command::new("codesign").args(["--verify","--strict"]).arg(&app).status().unwrap().success());
        fs::remove_dir_all(dir).unwrap();
    }

    // Chain: shared wakeup metadata -> bounded, locked read -> link or explicit unknown.
    #[test]
    fn wakeup_context_refuses_partial_or_busy_input() {
        let root = std::env::temp_dir().join(format!("qw-link-{}", std::process::id()));
        fs::create_dir_all(dot(&root)).unwrap();
        let path = dot(&root).join("poke"); let mut seen = None;
        assert!(wake_context(&path, &mut seen).unwrap().is_none());
        for text in [Vec::new(), b".".to_vec()] {
            fs::write(&path, text).unwrap();
            assert!(wake_context(&path, &mut seen).unwrap().is_none());
        }
        for text in [b"1 incomplete".to_vec(), vec![b'x'; 129], vec![255]] {
            fs::write(&path, text).unwrap();
            assert!(wake_context(&path, &mut seen).is_err());
        }
        let context = format!("1 {}.1 {}.2", "a".repeat(32), "a".repeat(32));
        fs::write(&path, &context).unwrap();
        let writer = fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        writer.try_lock().unwrap();
        assert_eq!(wake_context(&path, &mut seen).unwrap_err(), "wakeup context busy");
        writer.unlock().unwrap();
        writer.try_lock_shared().unwrap();
        assert!(poke(&root).unwrap_err().contains("wake-lock"));
        assert_eq!(fs::read_to_string(&path).unwrap(), context, "busy reader allowed a partial overwrite");
        assert_eq!(wake_context(&path, &mut seen).unwrap().unwrap().encode(), context);
        assert!(wake_context(&path, &mut seen).unwrap().is_none());
        // Parallel subprocess tests may inherit this fixture's descriptor before exec.
        writer.unlock().unwrap();
        drop(writer);
        poke(&root).unwrap();
        assert!(wake_context(&path, &mut seen).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    // Chain: § 5 stale TASKQ_ROOT (#1072). Risk: a reused session's command writes into another project.
    #[test]
    fn stale_taskq_root_is_refused() {
        let t = std::env::temp_dir().join(format!("taskq-stale-root-{}", std::process::id()));
        let (a, b, out) = (t.join("a"), t.join("b"), t.join("out"));
        for d in [&a, &b] {
            fs::create_dir_all(d.join(".worktrees/x")).unwrap();
            fs::write(d.join("taskq.json"), "{}").unwrap();
        }
        fs::create_dir_all(&out).unwrap();
        assert_eq!(root_for(Some(a.clone()), a.join(".worktrees/x")).unwrap(), a);
        assert_eq!(root_for(Some(a.clone()), out.clone()).unwrap(), a);
        let e = root_for(Some(b.clone()), a.join(".worktrees/x")).unwrap_err();
        assert!(e.starts_with("TASKQ_ROOT ") && e.contains("is not the project of the current directory"), "{e}");
        assert_eq!(root_for(None, a.join(".worktrees/x")).unwrap(), a);
        fs::remove_dir_all(&t).unwrap();
    }

    // Legacy migration input: test project timers independently of an installed host policy.
    #[test]
    fn update_every_defaults_to_60_and_keeps_override() {
        let t = std::env::temp_dir().join(format!("taskq-update-every-{}", crate::runtime::uuid()));
        fs::create_dir_all(&t).unwrap();
        fs::write(t.join("taskq.json"), r#"{"repo": "o/r"}"#).unwrap();
        assert_eq!(project_config(&t).unwrap().0.update_every, 60);
        fs::write(t.join("taskq.json"), r#"{"repo": "o/r", "update_every": 900}"#).unwrap();
        assert_eq!(project_config(&t).unwrap().0.update_every, 900);
        fs::remove_dir_all(&t).unwrap();
    }

    // Chain: #1661 A2 AS5 Checking. Risk: a probe in progress has no service signal, or the mark drops prior rows.
    #[test]
    fn a_due_probe_marks_the_last_status_checking() {
        let t = std::env::temp_dir().join(format!("taskq-checking-{}", crate::runtime::uuid()));
        let tool = platform::tool(&t.join("install"));
        let p = coreq::status::path(&tool, &t);
        mark_checking(&tool, &t, 30);
        assert!(!p.exists(), "no prior status: nothing to mark");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, json!({"at": 20, "agents": {"at": 20, "rows": [{"runtime": "claude"}]}}).to_string()).unwrap();
        mark_checking(&tool, &t, 30);
        let s: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(s["agents"], json!({"at": 20, "checking": 30, "rows": [{"runtime": "claude"}]}));
        fs::remove_dir_all(&t).unwrap();
    }

    // Chain: § 5 capacity edit. Risk: an invalid edit replaces the file.
    #[test]
    fn capacity_edit_keeps_the_file_and_validates() {
        let text = "{\n    \"repo\": \"o/r\",\n    \"limits\": {\"claude\": 4, \"codex\": 4},\n    \"hosts\": {\"a\": \"win\"},\n    \"zeta\": 1\n}\n";
        let e = CapEdit { limits: BTreeMap::from([("claude".into(), 2)]), gate: Some(true), ..Default::default() };
        let out = edit_capacity(text, "win", "mac", &e, None).unwrap();
        assert!(out.starts_with("{\n    \"repo\"") && out.ends_with("}\n"), "indent, order and newline kept:\n{out}");
        assert!(out.find("\"zeta\"").unwrap() < out.find("\"capacity_hosts\"").unwrap());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["limits"], json!({"claude": 4, "codex": 4}), "top level untouched");
        assert_eq!(v["capacity_hosts"]["win"], json!({"limits": {"claude": 2}, "load_gate": {"enabled": true, "cpu_max": 85, "memory_min": 15}}));
        assert_eq!(service::capacity(&v, "mac").unwrap().limits["claude"], 4, "other hosts keep theirs");
        let old = service::capacity(&serde_json::from_str(text).unwrap(), "win").unwrap().json();
        assert!(edit_capacity(text, "win", "mac", &e, Some(&old)).is_ok());
        let changed = edit_capacity(text, "win", "mac", &e, None).unwrap();
        assert!(edit_capacity(&changed, "win", "mac", &e, Some(&old)).is_err(), "lost receipt retries cannot overwrite a concurrent config change");
        for (host, e, expect, err) in [
            ("nas", e.clone(), None, "unknown host"),
            ("mac", CapEdit::default(), None, "nothing to change"),
            ("mac", CapEdit { cpu_max: Some(5), ..Default::default() }, None, "cpu_max"),
            ("mac", CapEdit { memory_min: Some(96), ..Default::default() }, None, "memory_min"),
            ("mac", CapEdit { limits: BTreeMap::from([("gpt".into(), 1)]), ..Default::default() }, None, "unknown runtime"),
            ("win", e.clone(), Some(json!({})), "nothing written"),
        ] {
            let r = edit_capacity(text, host, "mac", &e, expect.as_ref());
            assert!(r.as_ref().is_err_and(|x| x.contains(err)), "{host} {err}: {r:?}");
        }
    }

    // Chain: § 5 capacity --mode/--max -> § 9 admission. Risk: a mode edit drops the inherited maximum, `none`
    // leaves it, or Auto saves with the gate off.
    #[test]
    fn capacity_edit_writes_mode_and_maximum() {
        let text = r#"{"admission": {"mode": "auto", "max": 6}, "limits": {"claude": 0}, "hosts": {"a": "win"}}"#;
        let get = |out: &str, h: &str| service::capacity(&serde_json::from_str(out).unwrap(), h).unwrap();
        let fixed = edit_capacity(text, "mac", "mac", &CapEdit { mode: Some(false), ..Default::default() }, None).unwrap();
        let (c, v): (_, Value) = (get(&fixed, "mac"), serde_json::from_str(&fixed).unwrap());
        assert!(!c.auto && c.max == Some(6), "inherited maximum carried over");
        assert_eq!(v["admission"], json!({"mode": "auto", "max": 6}), "top level untouched");
        assert!(get(&fixed, "win").auto, "other hosts keep theirs");
        assert_eq!(c.limits["claude"], 0, "explicit zero kept");
        let none = edit_capacity(text, "mac", "mac", &CapEdit { max: Some(None), ..Default::default() }, None).unwrap();
        assert!(get(&none, "mac").auto && get(&none, "mac").max.is_none());
        assert_eq!(cap_changes(&get(text, "mac"), &get(&none, "mac")), "Maximum 6 → none");
        assert_eq!(cap_changes(&get(text, "mac"), &c), "Mode Auto → Fixed, Load gate on (CPU 90 %, memory 10 %) → off");
        let off = r#"{"load_gate": {"enabled": false}}"#;
        let r = edit_capacity(off, "mac", "mac", &CapEdit { mode: Some(true), ..Default::default() }, None);
        assert!(r.is_err_and(|x| x.contains("needs the load gate")), "Auto with the gate off is refused");
    }

    pub(crate) fn cfg() -> Config {
        Config {
            // Independent command fixtures must not contend for the same project wake lock.
            root: std::env::temp_dir().join(format!("taskq-main-test-{}",crate::runtime::uuid())),
            install: PathBuf::from("/nonexistent"),
            board: &board::github::KIND,
            web_host: String::new(),
            repo: "o/r".into(),
            publish: "pr".into(),
            delivery: "auto-merge".into(),
            delivery_revision: 0,
            me: "mac".into(),
            limits: BTreeMap::new(),
            has: vec![],
            labelled: false,
            default_runtime: "claude".into(),
            permission_mode: "dontAsk".into(),
            codex: vec![],
            ttl: 300,
            poll: 60,
            update_every: 900,
            rollback_after: 600,
            external: false,
        }
    }

    // Chain: § 9 `claim` (#1493, #1786). Risk: a task filed on a labelled host (worker follow-up or owner/PM plan)
    // leaves it for another host, an explicit --host is lost, or an any-claim host starts routing its adds.
    #[test]
    fn add_on_a_labelled_host_keeps_that_host() {
        let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let mut c = cfg();
        c.labelled = true;
        assert_eq!(add_host(None, &c).as_deref(), Some("mac"), "labelled host: add without --host gets host-mac");
        assert_eq!(add_host(Some("win"), &c).as_deref(), Some("win"), "--host wins over the labelled default");
        c.labelled = false;
        assert_eq!(add_host(None, &c), None, "any-claim host: add stays unrouted");
        assert_eq!(add_host(Some("win"), &c).as_deref(), Some("win"));
        let a = parse(&["service", "capacity", "--host", "mac", "--claim", "labelled"]).unwrap();
        assert_eq!(CapEdit::from_args(&a).unwrap().claim, Some(true));
        let bad = parse(&["service", "capacity", "--claim", "some"]).unwrap();
        assert!(CapEdit::from_args(&bad).is_err());
    }

    // Chain: § 5 `service capacity --default-runtime` -> host.json `default_runtime` (#1668). Risk: a typo or
    // `codex-app` reaches host policy, or the flag alone reads as an empty edit and nothing is saved.
    #[test]
    fn capacity_default_runtime_accepts_claude_or_codex_only() {
        let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        for rt in ["claude", "codex"] {
            let e = CapEdit::from_args(&parse(&["service", "capacity", "--host", "fra", "--default-runtime", rt]).unwrap()).unwrap();
            assert_eq!(e.default_runtime.as_deref(), Some(rt));
            assert!(!e.is_empty(), "the flag alone is a write");
        }
        for bad in ["codex-app", "gpt"] {
            let a = parse(&["service", "capacity", "--host", "fra", "--default-runtime", bad]).unwrap();
            assert_eq!(CapEdit::from_args(&a).err().unwrap(), format!("--default-runtime: claude or codex, not {bad}"));
        }
    }

    // Chain: § 5 CLI arguments. Risk: stray words or a consumed scope/JSON flag change the requested command.
    #[test]
    fn unused_positionals_are_rejected() {
        let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(parse(&["add", "T", "--goal", "G", "--acceptance", "A", "B", "C"]).err().unwrap(), "unused positional arguments: B C");
        assert_eq!(parse(&["inbox", "1", "extra"]).err().unwrap(), "unused positional arguments: extra");
        assert_eq!(parse(&["selftest", "extra"]).err().unwrap(), "unused positional arguments: extra");
        assert_eq!(parse(&["help", "extra"]).err().unwrap(), "unused positional arguments: extra");
        assert!(parse(&["help"]).is_ok());
        assert_eq!(parse(&["service", "status", "extra"]).err().unwrap(), "unused positional arguments: extra");
        assert_eq!(parse(&["add", "T", "--goal", "G", "--acceptance", "A", "--acceptance", "B"]).unwrap().all("acceptance"), vec!["A", "B"]);
        assert_eq!(parse(&["service", "channel", "dev", "extra"]).err().unwrap(), "unused positional arguments: extra");
        assert_eq!(channel_cmd(&parse(&["service", "channel", "beta"]).unwrap()).err().unwrap(), "channel must be dev or live");
        for v in [vec!["close", "1", "2"], vec!["answer", "1.1", "2.1"], vec!["service", "focus", "code", "docs"], vec!["service", "channel"], vec!["service", "channel", "dev"], vec!["service", "channel", "live"]] {
            assert!(parse(&v).is_ok());
        }
        for flags in [vec!["--host"],vec!["--host","--json"],vec!["--json","--host","--request","original-id"]] {
            let mut v=vec!["service","uninstall"];v.extend_from_slice(&flags);
            let parsed=parse(&v).unwrap();
            assert_eq!(parsed.one("host"),Some(""),"host uninstall scope consumed another option");
            assert_eq!(parsed.flags.contains_key("json"),flags.contains(&"--json"));
            if flags.contains(&"--request") {assert_eq!(parsed.one("request"),Some("original-id"));}
        }
        for v in [vec!["service","capacity","--host","fixture-host"],vec!["add","T","--host","fixture-host"]] {
            assert_eq!(parse(&v).unwrap().one("host"),Some("fixture-host"),"host-name option lost its value");
        }
    }

    // Chain: § 5 taskq add -> § 1 held -> start. Risk: an invalid task is written, or a new task is admitted without `start`.
    #[test]
    fn add_uses_canonical_names_defaults_and_validates_before_writing() {
        let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(Fake::default()) };
        let args = |extra: &[&str]| {
            let mut v = vec!["add", "T", "--goal", "G", "--acceptance", "observable"];
            v.extend_from_slice(extra); Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
        };
        let sorted = |n: u64| { let mut l = c.board.issue(n).unwrap().labels; l.sort(); l };
        command(&c, "add", &args(&[])).unwrap();
        let i = c.board.issues(None).unwrap().pop().unwrap();
        // § 1: every add is held, `--hold` or not; no flag skips it
        assert_eq!(i.labels, vec!["q-held"]);
        assert_eq!(fold(i.number, &entries(&*c.board, i.number).unwrap(), false).state, State::Held);
        assert!(command(&c, "add", &args(&["--later"])).is_err_and(|e| e.contains("held")), "--later cannot skip hold");
        command(&c, "add", &args(&["--priority", "1", "--type", "code", "--deps", &i.number.to_string()])).unwrap();
        let j = c.board.issues(None).unwrap().pop().unwrap();
        assert_eq!(sorted(j.number), vec!["code", "p1", "q-held"]);
        command(&c, "start", &Args::parse(&["start".into(), j.number.to_string()]).unwrap()).unwrap();
        let j = c.board.issue(j.number).unwrap();
        assert_eq!(sorted(j.number), vec!["code", "p1", "q-blocked", "taskq"], "start: dependency still open");
        assert!(in_scope(&j, &["q-waiting".into(), "priority-1".into()]));
        command(&c, "add", &args(&["--runtime", "codex-app"])).unwrap();
        assert!(c.board.issues(None).unwrap().pop().unwrap().labels.contains(&"run-codex-app".into()));
        command(&c, "add", &args(&["--priority", "0", "--hold"])).unwrap();
        assert_eq!(c.board.issues(None).unwrap().pop().unwrap().labels, vec!["q-held", "p0"]);
        assert!(command(&c, "add", &args(&["--prefer", "codex"])).is_err_and(|e| e.contains("--runtime codex")), "§ 9: --prefer refused with the --runtime hint");
        for extra in [&["--priority", "3"][..], &["--runtime", "any"], &["--type", "code", "--type", "docs"], &["--host", "win,mac"], &["--prefer", "codex-app"], &["--prefer", "codex", "--prefer", "claude"]] {
            assert!(command(&c, "add", &args(extra)).is_err());
        }
        // #1558: a repeated single-value flag is refused by name and files nothing, never kept as the last value
        for flag in ["--goal", "--type", "--runtime", "--priority", "--host", "--op-id", "--revision", "--text"] {
            let e = command(&c, "add", &args(&[flag, "1", flag, "2"])).unwrap_err();
            assert!(e.contains(flag) && e.contains("more than once"), "{flag}: {e}");
        }
        assert_eq!(c.board.issues(None).unwrap().len(), 4);
        command(&c, "add", &Args::parse(&["add", "T", "--goal", "G1\nG2", "--acceptance", "a", "--acceptance", "b", "--scope", "a", "--scope", "b"].map(String::from)).unwrap()).unwrap();
        assert!(c.board.issues(None).unwrap().pop().unwrap().body.contains("## Goal\nG1\nG2\n"), "two-line goal through one value");
        assert!(task_body(&*c.board, &NewTask { title: "T".into(), goal: "G".into(), acceptance: vec!["observable".into()], labels: vec!["prefer-codex".into(), "prefer-claude".into()], ..Default::default() }, &Op::default()).is_err_and(|e| e.contains("prefer")));
    }

    // Chain: § 1 `add --host` -> heartbeat names (§ 4 step 3). Risk (#1721): a `host-host-mac` or misspelt host label
    // files a task no host ever claims, and it sits ready with no reason shown.
    #[test]
    fn add_host_refuses_prefixed_and_unknown_names_before_writing() {
        let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(Fake::default()) };
        c.board.create("taskq host mac", "version 4.0.0", &[service::HOST_LABEL.into()]).unwrap();
        let add = |host: &str| command(&c, "add", &Args::parse(&["add", "T", "--goal", "G", "--acceptance", "observable", "--host", host].map(String::from)).unwrap());
        assert_eq!(add("host-mac").unwrap_err(), "--host takes the host name without host-: --host mac");
        assert_eq!(add("macbook").unwrap_err(), "--host macbook: no host named macbook; hosts: mac");
        assert_eq!(c.board.issues(None).unwrap().len(), 1, "a refused add files nothing");
        add("mac").unwrap();
        assert!(c.board.issues(None).unwrap().pop().unwrap().labels.contains(&"host-mac".into()));
    }

    // Chain: F1/F2 local finding -> held issue -> replay. Risk: duplicate or wrong-board writes.
    #[test]
    fn finding_cli_held_replay_payload_and_board_boundaries() {
        let c = Ctx { cfg: cfg(), raw: json!({"create_claim_issue": 1}), board: Box::new(Fake::default()) };
        c.board.create("Claims", "", &[]).unwrap();
        let base = ["finding", "Investigate", "--board", "https://board", "--source", "install/rule/episode", "--goal", "G", "--acceptance", "a", "--evidence", "Depends on #999"];
        let run = |v: &[&str]| command(&c, "finding", &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap());
        run(&base).unwrap();
        run(&base).unwrap();
        let i = c.board.issue(2).unwrap();
        assert_eq!(i.labels, ["q-held"]);
        assert!(i.body.contains("## Source\ninstall/rule/episode"));
        assert!(i.body.contains("## Evidence\n- Depends on #999"));
        assert!(core::depends(&i.body).is_empty(), "source evidence is data, never a dependency");
        assert!(c.board.comments(2).unwrap().is_empty(), "default finding never starts");
        assert_eq!(c.board.issues(Some(0)).unwrap().len(), 2);
        for (index, changed) in [(1, "Changed"), (3, "https://other"), (5, "../rule/episode"), (7, "Changed goal"), (9, "Changed acceptance"), (11, "Changed evidence")] {
            let mut v = base;
            v[index] = changed;
            assert!(run(&v).is_err(), "changed field {index}");
        }
        for extra in [["--op-id", "different"], ["--repo", "other/repo"], ["--host", "win,mac"], ["--goal", "second"], ["--evidence", "line\nline"]] {
            assert!(run(&[&base[..], &extra].concat()).is_err());
        }
        assert!(run(&[&base[..], &["--start"]].concat()).is_err(), "start choice is immutable; separate start remains available");
        assert_eq!(c.board.issues(Some(0)).unwrap().len(), 2, "denied input preserves the sole issue");
        assert_eq!(c.board.comments(1).unwrap().len(), 1, "denied/replayed requests never add another claim");
        c.board.close(2).unwrap();
        run(&base).unwrap();
        assert_eq!(c.board.issues(Some(0)).unwrap().len(), 2, "closed episode never recreates");
        let mut another = base;
        another[5] = "install/rule/another";
        run(&another).unwrap();
        let stale = Op { id: Some("later-start".into()), revision: Some(1) };
        assert_eq!(start_one(c.board.as_ref(), &c.cfg.root, 3, &stale).outcome, "stale");
        assert!(c.board.comments(3).unwrap().is_empty(), "stale start cannot initialize hold");
        c.board.comment(3, "taskq: unsupported").unwrap();
        let fresh = Op { revision: Some(0), ..stale };
        assert!(start_one(c.board.as_ref(), &c.cfg.root, 3, &fresh).ok(), "held default supports standalone start");
        let mut explicit = base;
        explicit[5] = "install/rule/explicit";
        let explicit = [&explicit[..], &["--start", "--json"]].concat();
        run(&explicit).unwrap();
        run(&explicit).unwrap();
        assert_eq!(c.board.issues(Some(0)).unwrap().len(), 4);
        assert!(service::is_task(&c.board.issue(4).unwrap()));
        assert_eq!(c.board.comments(4).unwrap().len(), 2, "one hold and one start across retries");
    }

    #[test]
    fn finding_start_adapter_failures_preserve_create_and_replay() {
        // Chain: F2 create readback -> failed admission -> same-ID recovery; no issue or receipt is lost.
        for failure in ["initialize", "hold-read", "start-read", "readback", "labels"] {
            let b = Fake::default();
            b.create("Claims", "", &[]).unwrap();
            let raw = json!({"create_claim_issue": 1});
            let op = Op { id: Some("finding.failure".into()), revision: None };
            let root = cfg().root;
            let submit = || finding_submit(&b, &raw, &root, "T", "G\nop-id: finding.failure\n", &["q-held".into()], "start:true", &op, true);
            {
                let mut st = b.state.borrow_mut();
                st.fail_issue_read = match failure { "initialize" => Some((2, 2)), "hold-read" => Some((2, 3)), "start-read" => Some((2, 4)), "readback" => Some((2, 5)), _ => None };
                st.fail_labels = failure == "labels";
            }
            let (created, started) = submit();
            assert_eq!((created.outcome, created.task), ("accepted", 2), "{failure}");
            let started = started.unwrap();
            assert_eq!((started.outcome, started.task, started.op_id.as_deref()), ("unknown", 2, Some("finding.failure.start")), "{failure}");
            assert!(emit_finding(created, Some(started), true).is_err());
            let (created, started) = submit();
            assert!(created.ok() && started.unwrap().ok(), "{failure}");
            let count = b.comments(2).unwrap().len();
            assert_eq!(count, 2, "one hold and one start: {failure}");
            let (created, started) = submit();
            assert!(created.ok() && started.unwrap().ok());
            assert_eq!(b.comments(2).unwrap().len(), count);
            assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
            assert!(service::is_task(&b.issue(2).unwrap()));
            assert!(!service::is_held(&b.issue(2).unwrap()));
        }
    }

    #[test]
    fn finding_separate_start_receipts_recover_without_recreate() {
        // Chain: F2 create success -> unknown start -> recovered start. Risk: losing the issue or admitting twice.
        let b = Fake::default();
        b.create("Claims", "", &[]).unwrap();
        let raw = json!({"create_claim_issue": 1});
        let op = Op { id: Some("finding.test".into()), revision: None };
        let body = "G\nop-id: finding.test\n";
        let labels = ["q-held".into()];
        let root = cfg().root;
        let submit = || finding_submit(&b, &raw, &root, "T", body, &labels, "start:true", &op, true);
        b.state.borrow_mut().hide_latest_comment = true;
        let (created, started) = submit();
        assert_eq!(created.outcome, "unknown", "a hidden claim cannot create");
        assert!(started.is_none());
        b.state.borrow_mut().hide_latest_comment = false;
        // A claimant whose response was lost may only discover, never POST another issue.
        b.create("T", body, &labels).unwrap();
        b.state.borrow_mut().hide_latest_comment = true;
        let (created, started) = submit();
        assert_eq!((created.outcome, created.task), ("accepted", 2));
        assert_eq!(started.unwrap().outcome, "unknown");
        assert_eq!(b.issue(2).unwrap().labels, ["q-held"]);
        b.state.borrow_mut().hide_latest_comment = false;
        let (created, started) = submit();
        assert!(created.ok());
        assert!(started.unwrap().ok());
        assert_eq!(b.issue(2).unwrap().labels, ["q-ready", "taskq"]);
        let count = b.comments(2).unwrap().len();
        let (created, started) = submit();
        assert!(created.ok() && started.unwrap().ok());
        assert_eq!(b.comments(2).unwrap().len(), count);
        assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
        b.labels(2, &["q-held".into()], &["q-ready".into(), "taskq".into()]).unwrap();
        assert!(submit().1.unwrap().ok(), "retry repairs partial eligibility projection");
        assert_eq!(b.issue(2).unwrap().labels, ["q-ready", "taskq"]);
        assert_eq!(b.comments(2).unwrap().len(), count, "projection recovery adds no log entries");
        b.comment(2, &entry("spawn", &["worker", "codex:s", "win"], None, "")).unwrap();
        assert_eq!(submit().1.unwrap().state, Some("doing"), "replay never restarts active work");
        b.close(2).unwrap();
        assert_eq!(submit().0.state, Some("closed"));
        assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
        let rejected_op = Op { id: Some("finding.rejected".into()), revision: None };
        let rejected_body = "G\nop-id: finding.rejected\n";
        let n = claimed_create(&b, &raw, "U", rejected_body, &labels, "start:true", &rejected_op).unwrap();
        b.close(n).unwrap();
        let (created, started) = finding_submit(&b, &raw, &root, "U", rejected_body, &labels, "start:true", &rejected_op, true);
        assert_eq!((created.outcome, created.task), ("accepted", n));
        assert_eq!(started.unwrap().outcome, "rejected");
        assert_eq!(b.issues(Some(0)).unwrap().len(), 3, "rejected start keeps its existing issue");
    }

    // Chain: §1 claimed create -> readback/retry. Risk: duplicate issues after a race, restart or lost response.
    #[test]
    fn create_claim_race_and_unknown_post_reconcile_without_another_post() {
        let raw = json!({"create_claim_issue": 1});
        let op = Op { id: Some("finding.1".into()), ..Default::default() };
        let body = "## Goal\nG\n\nop-id: finding.1\n";
        let labels = vec!["q-held".into()];
        let b = Fake::default();
        b.create("Claims", "", &[]).unwrap();
        let n = claimed_create(&b, &raw, "T", body, &labels, "", &op).unwrap();
        let claim = b.comments(1).unwrap()[0].body.clone();
        assert_eq!(claimed_create(&b, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "accepted");
        for (title, text, labels) in [("Changed", "", labels.clone()), ("T", "changed", labels.clone()), ("T", "", vec!["taskq".into()])] {
            assert_eq!(claimed_create(&b, &raw, title, body, &labels, text, &op).unwrap_err().outcome, "rejected");
        }
        b.close(n).unwrap();
        assert_eq!(claimed_create(&b, &raw, "T", body, &labels, "", &op).unwrap_err().state, Some("closed"));
        assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
        b.create("T", body, &labels).unwrap();
        assert_eq!(claimed_create(&b, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "unknown", "duplicate evidence is never accepted");

        let trusted = Fake::default();
        trusted.create("Claims", "", &[]).unwrap();
        trusted.raw(1, "taskq: create-claim finding.1\nrequest-sha256: forged", false, false);
        trusted.raw(1, "taskq: create-claim finding.1\nrequest-sha256: edited", true, true);
        assert!(claimed_create(&trusted, &raw, "T", body, &labels, "", &op).is_ok(), "untrusted and edited claims have no authority");
        assert_eq!(claimed_create(&trusted, &raw, "T", "changed body", &labels, "", &op).unwrap_err().outcome, "rejected");
        trusted.labels(1, &["taskq".into()], &[]).unwrap();
        assert_eq!(claimed_create(&trusted, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "rejected", "a task cannot be the claim issue");

        let legacy = Fake::default();
        legacy.create("Claims", "", &[]).unwrap();
        legacy.create("T", body, &labels).unwrap();
        assert_eq!(claimed_create(&legacy, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "rejected");
        assert!(legacy.comments(1).unwrap().is_empty(), "legacy reconciliation creates no claim");

        // Two invocations read no claim. The competitor's claim lands just before ours;
        // it is paused before its issue POST, so our invocation must only discover.
        let race = Fake::default();
        race.create("Claims", "", &[]).unwrap();
        race.state.borrow_mut().race = Some(claim);
        let r = claimed_create(&race, &raw, "T", body, &labels, "", &op).unwrap_err();
        assert_eq!(r.outcome, "unknown");
        assert_eq!(race.comments(1).unwrap().len(), 2, "both contenders posted claims");
        assert_eq!(race.issues(None).unwrap().len(), 1, "loser never POSTs an issue");
        assert_eq!(claimed_create(&race, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "unknown");
        // Winner's POST has an unknown response and is absent from discovery until later.
        let n = race.create("T", body, &labels).unwrap();
        race.advance(10000);
        let r = claimed_create(&race, &raw, "T", body, &labels, "", &op).unwrap_err();
        assert_eq!((r.outcome, r.task, r.state), ("accepted", n, Some("held")));
        assert_eq!(race.comments(1).unwrap().len(), 2, "recovery writes no comments");
        assert_eq!(race.issues(None).unwrap().len(), 2, "recovery writes no issues");

        let hidden = Fake::default();
        hidden.create("Claims", "", &[]).unwrap();
        hidden.state.borrow_mut().hide_latest_comment = true;
        assert_eq!(claimed_create(&hidden, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "unknown");
        assert_eq!(hidden.issues(None).unwrap().len(), 1, "missing claim readback cannot create");
        hidden.state.borrow_mut().hide_latest_comment = false;
        assert_eq!(claimed_create(&hidden, &raw, "T", body, &labels, "", &op).unwrap_err().outcome, "unknown");
        assert_eq!(hidden.comments(1).unwrap().len(), 1, "a crashed claimant is never replaced");
        assert_eq!(claimed_create(&hidden, &json!({}), "T", body, &labels, "", &op).unwrap_err().outcome, "rejected");
    }

    // Chain: §1 create POST -> lost response -> delayed readback. Risk: a retry duplicates the accepted POST.
    #[test]
    fn create_claim_adapter_lost_posts_and_interleaved_creates() {
        use std::cell::Cell;
        struct Writes {
            b: Fake,
            lose_create: Cell<bool>,
            lose_claim: Cell<bool>,
            hide_issue: Cell<bool>,
            race: Cell<bool>,
        }
        impl Board for Writes {
            fn issues(&self, since: Option<i64>) -> Res<Vec<board::Issue>> { Ok(self.b.issues(since)?.into_iter().filter(|i| !self.hide_issue.get() || i.number == 1).collect()) }
            fn issue(&self, n: u64) -> Res<board::Issue> { self.b.issue(n) }
            fn comments(&self, n: u64) -> Res<Vec<board::Comment>> { self.b.comments(n) }
            fn comment(&self, n: u64, body: &str) -> Res<u64> {
                if self.race.replace(false) {
                    let op = Op { id: Some("lost.1".into()), ..Default::default() };
                    claimed_create(&self.b, &json!({"create_claim_issue": 1}), "T", "G\nop-id: lost.1\n", &["q-held".into()], "", &op).unwrap();
                }
                let id = self.b.comment(n, body)?;
                if self.lose_claim.replace(false) { Err("claim POST response lost".into()) } else { Ok(id) }
            }
            fn create(&self, title: &str, body: &str, labels: &[String]) -> Res<u64> {
                let n = self.b.create(title, body, labels)?;
                if self.lose_create.replace(false) { Err("create POST response lost".into()) } else { Ok(n) }
            }
            fn me(&self) -> Res<String> { self.b.me() }
            fn edit_comment(&self, n: u64, id: u64, body: &str) -> Res<()> { self.b.edit_comment(n, id, body) }
            fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Res<()> { self.b.labels(n, add, remove) }
            fn edit(&self, n: u64, body: &str) -> Res<()> { self.b.edit(n, body) }
            fn close(&self, n: u64) -> Res<()> { self.b.close(n) }
            fn now(&self) -> Res<i64> { self.b.now() }
            fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> board::Publish { self.b.publish(n, sha, mode, title, task_line) }
            fn url(&self, n: u64) -> String { self.b.url(n) }
        }
        for mode in ["create", "claim", "race"] {
            let b = Writes { b: Fake::default(), lose_create: Cell::new(mode == "create"), lose_claim: Cell::new(mode == "claim"), hide_issue: Cell::new(false), race: Cell::new(mode == "race") };
            b.b.create("Claims", "", &[]).unwrap();
            let op = Op { id: Some("lost.1".into()), ..Default::default() };
            let run = || claimed_create(&b, &json!({"create_claim_issue": 1}), "T", "G\nop-id: lost.1\n", &["q-held".into()], "", &op);
            let r = run().unwrap_err();
            assert_eq!(r.outcome, if mode == "race" { "accepted" } else { "unknown" });
            let count = b.b.issues(None).unwrap().len();
            assert_eq!(count, if mode == "claim" { 1 } else { 2 });
            b.hide_issue.set(true);
            assert_eq!(run().unwrap_err().outcome, "unknown", "empty discovery cannot authorize another POST");
            b.hide_issue.set(false);
            let r = run().unwrap_err();
            assert_eq!(r.outcome, if mode == "claim" { "unknown" } else { "accepted" });
            assert_eq!(b.b.issues(None).unwrap().len(), count);
            assert_eq!(b.comments(1).unwrap().len(), if mode == "race" { 2 } else { 1 });
        }
    }

    // Chain: § 1 held creation -> explicit start. Risk: idempotent retries admit held work.
    #[test]
    fn held_create_is_idempotent_and_starts_only_on_start() {
        let c = Ctx { cfg: cfg(), raw: json!({"create_claim_issue": 1}), board: Box::new(Fake::default()) };
        let b = c.board.as_ref();
        b.create("Create claims", "", &[]).unwrap();
        let run = |v: &[&str]| command(&c, v[0], &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap());
        let item = ["add", "T", "--goal", "G", "--acceptance", "a", "--hold", "--op-id", "req.1.0"];
        run(&item).unwrap();
        run(&item).unwrap();
        let issues: Vec<_> = b.issues(None).unwrap().into_iter().filter(service::is_held).collect();
        assert_eq!(issues.len(), 1, "a repeated op id writes one issue");
        let n = issues[0].number;
        assert!(issues[0].body.ends_with("\nop-id: req.1.0\n"));
        // N-1: no `taskq` label, so 4.1.11 (same `is_task`) skips the issue; this version lists it as held
        assert_eq!(issues[0].labels, vec!["q-held"]);
        assert!(!service::is_task(&issues[0]) && service::is_held(&issues[0]));
        let (views, _) = service::views(b, &mut HashMap::new(), &mut HashMap::new()).unwrap();
        assert_eq!(views[0].projection.task.state, State::Held);
        assert_eq!(fold(n, &entries(b, n).unwrap(), false).state, State::Held);
        run(&["add", "U", "--goal", "G", "--acceptance", "a", "--op-id", "req.1.1"]).unwrap();
        assert_eq!(b.issues(None).unwrap().len(), 3, "each item has its own receipt, plus the claim issue");
        run(&["start", &n.to_string(), "--op-id", "s1"]).unwrap();
        let i = b.issue(n).unwrap();
        assert!(service::is_task(&i) && i.labels.contains(&"q-ready".to_string()) && !i.labels.contains(&"q-held".to_string()), "{:?}", i.labels);
        let log = entries(b, n).unwrap();
        assert_eq!(fold(n, &log, false).state, State::Ready);
        // N-1 reads `hold` and `start` as unknown kinds: the same `ready`
        let old: Vec<Entry> = log.iter().filter(|e| !e.body.starts_with("taskq: hold") && !e.body.starts_with("taskq: start")).cloned().collect();
        assert_eq!(fold(n, &old, false), fold(n, &log, false));
        run(&["start", &n.to_string(), "--op-id", "s1"]).unwrap();
        assert_eq!(entries(b, n).unwrap().len(), log.len(), "a retried start writes nothing");
        assert!(run(&["start", &n.to_string()]).is_err(), "a started task refuses another start");
        // § 1 multi start: every number is checked before any write, then started in order
        let held: Vec<u64> = b.issues(None).unwrap().into_iter().filter(service::is_held).map(|i| i.number).collect();
        assert_eq!(held.len(), 1, "add without --hold is held too");
        let u = held[0];
        let before = entries(b, u).unwrap().len();
        assert!(run(&["start", &u.to_string(), &n.to_string()]).is_err_and(|e| e.contains(&format!("#{n}"))));
        assert_eq!(entries(b, u).unwrap().len(), before, "a bad number in the list writes nothing");
        run(&["add", "V", "--goal", "G", "--acceptance", "a"]).unwrap();
        let v = b.issues(None).unwrap().into_iter().filter(service::is_held).map(|i| i.number).max().unwrap();
        let text = snapshot::read(b, &c.cfg, &c.raw, &[]).text(&c.cfg, &c.raw).unwrap();
        assert!(text.contains(&format!("Held: [#{u} U]")) && text.contains(&format!("[#{v} V]")), "{text}");
        let receipts = start_tasks(b, Path::new("/nonexistent"), &[u.to_string(), format!("#{v}")], &Op::default()).unwrap();
        assert_eq!(receipts.iter().map(|r| r.task).collect::<Vec<_>>(), vec![u, v]);
        assert!([u, v].iter().all(|&m| fold(m, &entries(b, m).unwrap(), false).state == State::Ready));
    }

    // Chain: § 7 recipe run, held tasks, owner gate, close publication, dependants. Risk: a rerun duplicates tasks,
    // a task starts before the whole recipe reads back, or a gated result publishes without approval of its SHA.
    #[test]
    fn a_recipe_holds_its_tasks_and_a_gate_blocks_close_until_the_owner_approves() {
        let raw = json!({"create_claim_issue": 1, "recipes": {"feature": {"version": 2, "tasks": [
            {"title": "Spec {goal}", "goal": "Decide {goal}", "acceptance": ["spec merged"], "gate": true},
            {"title": "Code {goal}", "goal": "Build {goal}", "acceptance": ["tests pass"], "deps": [1]}
        ]}}});
        let c = Ctx { cfg: cfg(), raw, board: Box::new(Fake::default()) };
        let b = c.board.as_ref();
        let run = |v: &[&str]| command(&c, v[0], &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap());
        b.create("create claims", "", &[]).unwrap();
        run(&["recipe", "feature", "--run", "r1", "--goal", "login"]).unwrap();
        let issues: Vec<_> = b.issues(None).unwrap().into_iter().filter(|i| service::is_held(i)).collect();
        assert_eq!(issues.len(), 2);
        let (spec, code) = (issues[0].number, issues[1].number);
        assert_eq!((issues[0].title.as_str(), issues[0].labels.clone()), ("Spec login", vec!["q-held".to_string(), "needs-gate".into()]));
        assert!(issues[0].body.ends_with("\nop-id: recipe.feature.2.r1.1\n"));
        assert!(issues[1].body.contains(&format!("Depends on #{spec}\n")) && !service::is_task(&issues[1]));
        let t = fold(spec, &entries(b, spec).unwrap(), false);
        assert_eq!((t.state, t.gate), (State::Held, Some(core::Gate::Required)));
        // the rerun returns the first receipts, then starts both
        run(&["recipe", "feature", "--run", "r1", "--goal", "login", "--start"]).unwrap();
        assert_eq!(b.issues(None).unwrap().len(), 3, "a rerun adds nothing");
        assert_eq!(b.comments(1).unwrap().len(), 2, "one immutable claim per recipe task");
        assert_eq!((current(b, spec).unwrap().1.state, current(b, code).unwrap().1.state), (State::Ready, State::Waiting));
        let sha = "0123456789abcdef0123456789abcdef01234567";
        b.comment(spec, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = fold(spec, &entries(b, spec).unwrap(), false).attempt.unwrap();
        b.comment(spec, &entry("result", &[sha], Some(a), "")).unwrap();
        let r = close_one(b, &c.cfg, spec, "ok", &Op::default());
        assert!(!r.ok() && r.reason.contains("gate is required") && b.issue(spec).unwrap().open, "{}", r.text());
        let approve = |sha: Option<&str>, agent: bool| dispatch::approve(b, &c.cfg.root, spec, sha, &Op::default(), agent);
        assert!(!approve(Some("89abcde"), false).ok(), "another SHA is not approved");
        assert!(!approve(None, true).ok(), "an agent cannot approve");
        assert!(approve(Some(&sha[..7]), false).ok());
        assert_eq!(current(b, spec).unwrap().1.gate, Some(core::Gate::Approved));
        assert!(close_one(b, &c.cfg, spec, "ok", &Op::default()).ok());
        assert!(!b.issue(spec).unwrap().open);
        assert_eq!(current(b, code).unwrap().1.state, State::Ready, "the approved spec frees the coding task");
    }

    // Chain: § 7 delivery preset -> owner approval of the exact SHA -> publication receipt. Risk: `publish: pr` turns
    // into review-pr silently, a review-pr result publishes unapproved or on a stale approval, or the receipt omits the policy.
    #[test]
    fn review_pr_publishes_only_the_approved_sha_and_names_the_policy() {
        let preset = |v: Value| delivery(&v).map(|(m, p, r)| format!("{m} {p} {r}"));
        assert_eq!(preset(json!({})).unwrap(), "pr auto-merge 0");
        assert_eq!(preset(json!({"publish": "pr"})).unwrap(), "pr auto-merge 0", "no silent pr to review-pr");
        assert_eq!(preset(json!({"publish": "direct"})).unwrap(), "direct direct 0");
        assert_eq!(preset(json!({"delivery": {"preset": "review-pr", "revision": 3}})).unwrap(), "pr review-pr 3");
        assert!(preset(json!({"delivery": {"preset": "local", "revision": 1}})).unwrap_err().contains("not available"));
        assert!(preset(json!({"delivery": {"preset": "review-pr"}})).unwrap_err().contains("revision"));
        assert!(preset(json!({"publish": "direct", "delivery": {"preset": "review-pr", "revision": 1}})).unwrap_err().contains("disagrees"));
        let c = Config { delivery: "review-pr".into(), delivery_revision: 3, ..cfg() };
        let b = Fake::default();
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        let (one, two) = ("0123456789abcdef0123456789abcdef01234567", "89abcdef0123456789abcdef0123456789abcdef");
        let work = |sha: &str| {
            b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
            let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
            b.comment(n, &entry("result", &[sha], Some(a), "")).unwrap();
        };
        work(one);
        let r = close_one(&b, &c, n, "ok", &Op::default());
        assert!(!r.ok() && r.reason.contains("gate is required"), "{}", r.text());
        assert!(dispatch::approve(&b, &c.root, n, Some(&one[..7]), &Op::default(), false).ok());
        // a new SHA invalidates the approval
        act(&b, &c.root, n, "requeue", &[], "redo", &Op::default());
        work(two);
        let r = close_one(&b, &c, n, "ok", &Op::default());
        assert!(!r.ok() && r.reason.contains("gate is stale") && b.issue(n).unwrap().open, "{}", r.text());
        assert!(!dispatch::approve(&b, &c.root, n, Some(&one[..7]), &Op::default(), false).ok(), "the old SHA is not the candidate");
        assert!(dispatch::approve(&b, &c.root, n, Some(&two[..7]), &Op::default(), false).ok());
        assert_eq!(close_one(&b, &c, n, "ok", &Op::default()).text(), format!("#{n} closed\ndelivery review-pr r3: merged {two}"));
    }

    #[test]
    fn add_renders_the_task_body_and_rejects_over_limit_text() {
        let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(Fake::default()) };
        let run = |v: &[&str]| command(&c, "add", &Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap());
        c.board.create("dep", "", &[]).unwrap();
        run(&["add", "T", "--goal", "G", "--acceptance", "a", "--acceptance", "b", "--scope", "s", "--deps", "1"]).unwrap();
        let i = c.board.issues(None).unwrap().pop().unwrap();
        assert_eq!(i.body, "## Goal\nG\n\n## Acceptance\nA1. a\nA2. b\n\n## Scope\n- s\n\nDepends on #1\n");
        let long = "x".repeat(237);
        assert!(run(&["add", "T", "--goal", "G", "--acceptance", &long]).unwrap_err().contains("241 characters, limit 240"));
        assert!(run(&["add", &"t".repeat(81), "--goal", "G", "--acceptance", "a"]).unwrap_err().contains("80"));
        assert!(run(&["add", "T", "--goal", "1\n2\n3", "--acceptance", "a"]).unwrap_err().contains("Goal: 3 lines"));
        assert!(run(&["add", "T", "--goal", "G", "--acceptance", "a\nb"]).is_err());
        // #1696: a typed A<n>. label would render "A1. A1. ..."; a later mention is text
        assert!(run(&["add", "T", "--goal", "G", "--acceptance", "a", "--acceptance", " A12. b"]).unwrap_err().contains("--acceptance 2: drop the A<n>. label"));
        run(&["add", "T2", "--goal", "G", "--acceptance", "see A1. above", "--acceptance", "Abc. d"]).unwrap();
        assert!(c.board.issues(None).unwrap().pop().unwrap().body.contains("A1. see A1. above\nA2. Abc. d\n"));
        let mut v = vec!["add", "T", "--goal", "G"];
        v.extend(["--acceptance", "a"].repeat(9));
        assert!(run(&v).unwrap_err().contains("split the task"));
        // three violations, one error (#1346)
        let title = "t".repeat(81);
        v[1] = &title;
        v[3] = "1\n2\n3";
        let e = run(&v).unwrap_err();
        assert!(e.contains("title: 81 characters, limit 80") && e.contains("Goal: 3 lines, allowed 1-2") && e.contains("Acceptance: 9 lines, allowed 1-8"), "{e}");
        assert_eq!(c.board.issues(None).unwrap().len(), 3, "rejected input writes nothing");
    }

    // Chain: § 2 visual evidence (#1782). Defect: csgo#378 published image links the board could not render.
    // Risk: an image that the board does not serve back is embedded, or a non-image file is uploaded.
    #[test]
    fn ask_images_embed_only_after_the_load_check() {
        let dir = std::env::temp_dir().join(format!("taskq-ask-images-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let png = dir.join("front view.png");
        fs::write(&png, b"\x89PNG\r\n\x1a\n0123456789").unwrap();
        let txt = dir.join("notes.png");
        fs::write(&txt, "not an image").unwrap();
        let b = Fake::default();
        let p = png.display().to_string();
        assert_eq!(upload_images(&b, 7, std::slice::from_ref(&p)).unwrap(), vec!["![front view](https://board.example/7/0-front-view.png)".to_string()]);
        assert!(upload_images(&b, 7, &[txt.display().to_string()]).unwrap_err().contains("not a PNG"));
        assert!(upload_images(&b, 7, &vec![p.clone(); 5]).unwrap_err().contains("at most four"));
        b.state.borrow_mut().bad_image = true;
        assert!(upload_images(&b, 7, &[p]).unwrap_err().starts_with("image load check failed"));
        fs::remove_dir_all(&dir).unwrap();
    }

    // Chain: § 2 Ask card. Risk: a card breaks the template or limits.
    #[test]
    fn ask_card_follows_section_2() {
        let card = ask_card(7, "Done: checked spec\nProblem: two ways\nboth slow", &["Left - + fast; - ugly".into(), "Right - + clean; - slower".into()], Some("2"), &["https://g/o/r/pull/12".into(), "[shot](https://x/y.png)".into()]).unwrap();
        assert_eq!(card, "Done: checked spec\nProblem: two ways\nboth slow\nEvidence: [#12](https://g/o/r/pull/12), [shot](https://x/y.png)\n1. Left - + fast; - ugly\n2. Right - + clean; - slower ★\nTo answer: `taskq answer 7.K` or comment `K` or the option text.");
        let o = |n: usize| (1..=n).map(|i| format!("o{i} - + g; - c")).collect::<Vec<_>>();
        assert!(ask_card(7, "Problem: p", &o(2), Some("1"), &[]).unwrap_err().contains("Done: 0 lines"));
        assert!(ask_card(7, "question?", &o(2), Some("1"), &[]).unwrap_err().contains("starts with"));
        assert!(ask_card(7, "Done: d", &o(2), Some("1"), &[]).unwrap_err().contains("Problem: 0 lines"));
        assert!(ask_card(7, "Problem: p\nDone: d", &o(2), Some("1"), &[]).unwrap_err().contains("order"));
        assert!(ask_card(7, &format!("Done: {}", "d".repeat(155)), &o(2), Some("1"), &[]).unwrap_err().contains("161 characters, limit 160"));
        assert!(ask_card(7, "Done: d\nProblem: p", &o(1), Some("1"), &[]).is_err());
        assert!(ask_card(7, "Done: d\nProblem: p", &o(5), Some("1"), &[]).is_err());
        assert!(ask_card(7, "Done: d\nProblem: p", &o(2), None, &[]).unwrap_err().contains("--recommend"));
        assert!(ask_card(7, "Done: d\nProblem: p", &o(2), Some("3"), &[]).is_err());
        assert!(ask_card(7, "Done: d\nProblem: p", &o(2), Some("1"), &vec!["u".to_string(); 3]).is_err());
        assert_eq!(ask_card(7, "Done: d\nProblem: add the token", &[], None, &[]).unwrap(), "Done: d\nProblem: add the token\nTo answer: `taskq answer 7 --text <done>`.");
        assert!(ask_card(7, "Done: d\nProblem: p", &[], Some("1"), &[]).is_err(), "--recommend needs options");
        // #1457: one short labelled link per Problem/option line; the URL is not visible text; the error names the line
        let url = format!("https://github.com/o/r/issues/1443#{}", "x".repeat(200));
        assert!(ask_card(7, "Done: d\nProblem: p", &[format!("Merge later - + after [#1443]({url}) {}; - waits", "w".repeat(100)), "b - + g; - c".into()], Some("1"), &[]).is_ok());
        assert_eq!(ask_card(7, "Done: d\nProblem: p", &["a - + g; - c".into(), format!("b - + see {url}; - c")], Some("1"), &[]).unwrap_err(), "option line 2: bare URL; write [label](url)");
        assert_eq!(ask_card(7, "Done: d\nProblem: p", &["a - + [a](https://x); - [b](https://y)".into(), "b - + g; - c".into()], Some("1"), &[]).unwrap_err(), "option line 1: 2 links, at most one [label](url)");
        assert_eq!(ask_card(7, &format!("Done: d\nProblem: p\n[{}](https://x)", "l".repeat(41)), &o(2), Some("1"), &[]).unwrap_err(), "Problem line 2: link label at most 40 characters");
        assert!(ask_card(7, "Done: d\nProblem: p", &["a - + g; - c".into(), format!("b - + {}; - c", "c".repeat(147))], Some("1"), &[]).unwrap_err().contains("option line 2: 161 characters, limit 160"));
        assert_eq!(width("see [#12](https://very/long/url) and `cargo test --locked` https://x/y"), 15, "link targets and code are exempt; outside whitespace counts");
        // #1787: an option names its meaning in a short unique label, then one plus and one minus; the error names the line
        let ok = |a: &str, b: &str| ask_card(7, "Done: d\nProblem: p", &[a.into(), b.into()], Some("1"), &[]);
        assert_eq!(ok("a - + g; - c", "- + g; - c").unwrap_err(), "option line 2: missing label; write `<label> - + <gain>; - <cost>`");
        assert_eq!(ok("a - + g; - c", "b").unwrap_err(), "option line 2: detail must be `+ <gain>; - <cost>` after `b - `");
        assert_eq!(ok("a - + g; - c", "b - only a gain").unwrap_err(), "option line 2: detail must be `+ <gain>; - <cost>` after `b - `");
        assert_eq!(ok(&format!("{} - + g; - c", "l".repeat(33)), "b - + g; - c").unwrap_err(), "option line 1: label 33 characters, limit 32");
        assert_eq!(ok("Keep - + g; - c", "keep - + h; - d").unwrap_err(), "option line 2: label repeats option line 1");
        // A Russian card counts characters, not bytes: a 32-letter Cyrillic label fits, 33 do not
        let ru = ask_card(7, "Done: проверил оба пути\nProblem: выбрать способ сборки", &["Оставить совместимость - + старые клиенты работают; - больше кода".into(), "Убрать старый API - + проще код; - клиенты сломаются".into()], Some("1"), &[]).unwrap();
        assert!(ru.contains("1. Оставить совместимость - + старые клиенты работают; - больше кода ★\n2. Убрать старый API"), "{ru}");
        assert!(ok(&format!("{} - + да; - нет", "ж".repeat(32)), "b - + g; - c").is_ok());
        assert_eq!(ok(&format!("{} - + да; - нет", "ж".repeat(33)), "b - + g; - c").unwrap_err(), "option line 1: label 33 characters, limit 32");
        // #1782: uploaded images embed inline after the links; a relative link would render broken on the board
        let img = "![front](https://gitlab.example/-/project/39/uploads/ab/front.png#upload=214)".to_string();
        assert_eq!(ask_card(7, "Done: d\nProblem: p", &o(2), Some("1"), &["[#1](https://x/1)".into(), img.clone()]).unwrap().lines().nth(2), Some(format!("Evidence: [#1](https://x/1), {img}").as_str()));
        // #1787 visual choice: one image per option, image K named `K.` so every surface captions it by number
        let pair = ["![left](https://x/l.png)".to_string(), "![right](https://x/r.png)".to_string()];
        assert_eq!(ask_card(7, "Done: d\nProblem: p", &o(2), Some("1"), &pair).unwrap().lines().nth(2), Some("Evidence: ![1. left](https://x/l.png), ![2. right](https://x/r.png)"));
        assert_eq!(ask_card(7, "Done: d\nProblem: p", &o(3), Some("1"), &pair).unwrap().lines().nth(2), Some("Evidence: ![left](https://x/l.png), ![right](https://x/r.png)"), "fewer images than options: plain evidence");
        assert_eq!(ask_card(7, "Done: d\nProblem: p", &o(2), Some("1"), &["[shot](/uploads/ab/front.png)".into()]).unwrap_err(), "Evidence: [shot](/uploads/ab/front.png) needs an absolute http(s) URL");
        assert_eq!(result_text(7, "Changed: c\nOpen: none", &["t -- ok".into()], &[], None, std::slice::from_ref(&img)).unwrap(), format!("Changed: c\nChecks: t -- ok\nOpen: none\n\nEvidence: {img}"));
        assert!(result_text(7, "Changed: c\nOpen: none", &["t -- ok".into()], &[], None, &["https://x/1".into()]).is_err(), "--link still needs --option");
    }

    // H-A5: one registry owner covers preflight/effects/commit; a later remover reads the preceding writer's roots.
    #[test]
    fn registry_owner_preserves_parallel_changes_before_uninstall() {
        let base=std::env::temp_dir().join(format!("taskq-registry-owner-{}",std::process::id()));
        let install=base.join("install");let a=base.join("a");let b=base.join("b");let c=base.join("c");
        for path in [&install,&a,&b,&c] {fs::create_dir_all(path).unwrap();}
        let before=format!("{}\n{}\n",a.display(),b.display());
        fs::write(install.join("projects"),&before).unwrap();fs::write(install.join("fixture.unit"),"unit").unwrap();
        let held=coreq::file_lock::lock(&install.join("projects.lock")).unwrap();
        let run=||coreq::process::output("test.registry-owner",std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","tests::registry_uninstall_fixture","--ignored"]).env("TASKQ_REGISTRY_FIXTURE",&base),Duration::from_secs(5)).unwrap();
        run();
        let busy:Value=serde_json::from_slice(&fs::read(base.join("result.json")).unwrap()).unwrap();
        let unchanged=fs::read_to_string(install.join("projects")).unwrap()==before && install.join("fixture.unit").is_file();
        // This is the first owner's acknowledged change while the second one has been refused.
        fs::write(install.join("projects"),format!("{before}{}\n",c.display())).unwrap();
        drop(held);fs::write(install.join("fixture.unit"),"unit").unwrap();run();
        let done:Value=serde_json::from_slice(&fs::read(base.join("result.json")).unwrap()).unwrap();
        let after=fs::read_to_string(install.join("projects")).unwrap();
        fs::remove_dir_all(&base).unwrap();
        assert_eq!(busy["ok"],false,"uninstall crossed another registry owner's boundary: {busy}");
        assert!(unchanged,"registry or unit changed while the preceding owner held projects.lock");
        assert_eq!(done["ok"],true,"{done}");
        assert_eq!(after,format!("{}\n{}\n",b.display(),c.display()),"a stale snapshot lost the preceding writer's project");
    }

    #[test]
    #[ignore="owned process for registry_owner_preserves_parallel_changes_before_uninstall"]
    fn registry_uninstall_fixture() {
        let base=PathBuf::from(std::env::var_os("TASKQ_REGISTRY_FIXTURE").unwrap());let install=base.join("install");
        let result=host::ProjectEdit::new(&install,&base.join("a"),false).and_then(|edit|edit.commit());
        platform::write_atomic(&base.join("result.json"),&json!({"ok":result.is_ok(),"error":result.err()}).to_string()).unwrap();
    }

    // Chain: § 4 service uninstall. Risk: an unreadable registry loses the unit.
    #[test]
    fn unreadable_registry_preserves_unit_before_unload() {
        let root = std::env::temp_dir().join(format!("taskq-registry-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let projects = root.join("projects");
        let registered = b"other-project\n\xff";
        fs::write(&projects, registered).unwrap();
        let u = platform::Unit {
            path: root.join("unit"), text: String::new(), load: vec![], stop: vec![],
            unload: vec![vec!["taskq-must-not-run-unload".into()]],
        };
        fs::write(&u.path, "unit contents").unwrap();
        let error = host::ProjectEdit::new(&root,&root,false).err().unwrap();
        assert!(error.contains(&projects.display().to_string()));
        assert!(!error.contains("taskq-must-not-run-unload"));
        assert_eq!(fs::read_to_string(&u.path).unwrap(), "unit contents");
        assert_eq!(fs::read(&projects).unwrap(), registered);
        fs::remove_file(&projects).unwrap();
        assert!(host::ProjectEdit::new(&root,&root,false).err().unwrap().contains(&projects.display().to_string()));
        assert!(u.path.is_file());
        assert!(!projects.exists());
        fs::remove_file(&u.path).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    // Chain: § 7 close verdict. Risk: a padded verdict publishes.
    #[test]
    fn padded_close_verdict_is_rejected_before_publication() {
        let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(Fake::default()) };
        let n = c.board.create("T", "A1. observable", &["taskq".into()]).unwrap();
        c.board.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let attempt = fold(n, &entries(c.board.as_ref(), n).unwrap(), false).attempt;
        let sha = "0123456789abcdef0123456789abcdef01234567";
        c.board.comment(n, &entry("result", &[sha], attempt, "result")).unwrap();
        let before = c.board.comments(n).unwrap();
        for changed in [format!("{}Changed: c", " ".repeat(240)), format!("Changed: c{}", " ".repeat(240))] {
            let verdict = format!("{changed}\nChecks: A1 ok; CI green on {sha}\nOpen: none");
            let args = Args::parse(&["close".into(), n.to_string(), "--text".into(), verdict]).unwrap();
            assert!(command(&c, "close", &args).is_err());
            assert!(c.board.issue(n).unwrap().open);
            assert_eq!(c.board.comments(n).unwrap().len(), before.len());
            assert_eq!(fold(n, &entries(c.board.as_ref(), n).unwrap(), false).state, State::Review);
        }
    }

    // Chain: § 7 rule 4 shots gate (#1127). Risk: a result with stale README shots reaches review.
    #[test]
    fn result_refuses_stale_shots() {
        let root = std::env::temp_dir().join(format!("taskq-shots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs/shots")).unwrap();
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git").arg("-C").arg(&root).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
            assert!(o.status.success(), "{args:?}");
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        let commit = |tray: &str, fresh: bool| {
            fs::write(root.join("src/tray.rs"), tray).unwrap();
            git(&["add", "-A"]);
            let blob = if fresh { git(&["hash-object", "src/tray.rs"]) } else { "0".repeat(40) };
            let old: Value = fs::read_to_string(root.join("docs/shots/manifest.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let blob = if fresh || old.is_null() { blob } else { old["uiSources"]["src/tray.rs"].as_str().unwrap().to_string() };
            fs::write(root.join("docs/shots/manifest.json"), json!({"uiSources": {"src/tray.rs": blob}}).to_string()).unwrap();
            git(&["add", "-A"]);
            git(&["commit", "-qm", tray]);
            git(&["rev-parse", "HEAD"])
        };
        git(&["init", "-q"]);
        let base = commit("v1", true);
        git(&["update-ref", "refs/remotes/origin/main", &base]);
        assert!(shots_gate("git", &root, &base).is_ok(), "an unchanged diff passes");
        let stale = commit("v2", false);
        let e = shots_gate("git", &root, &stale).unwrap_err();
        assert!(e.contains("src/tray.rs") && e.contains("docs/shots/update.sh"), "{e}");
        let fresh = commit("v3", true);
        assert!(shots_gate("git", &root, &fresh).is_ok(), "a fresh manifest passes");
        git(&["update-ref", "refs/remotes/origin/main", &fresh]);
        git(&["mv", "src/tray.rs", "src/tray_menu.rs"]);
        git(&["commit", "-qm", "pure rename"]);
        let renamed = git(&["rev-parse", "HEAD"]);
        let e = shots_gate("git", &root, &renamed).unwrap_err();
        assert!(e.contains("src/tray.rs") && e.contains("docs/shots/update.sh"), "a pure rename leaves the manifest stale: {e}");
        git(&["reset", "-q", "--hard", &fresh]);
        fs::remove_file(root.join("docs/shots/manifest.json")).unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "no shots"]);
        let bare = git(&["rev-parse", "HEAD"]);
        assert!(shots_gate("git", &root, &bare).is_ok(), "a readable candidate without the manifest passes");
        for (bin, sha, why) in [("taskq-no-such-git", fresh.as_str(), "Git unavailable"), ("git", &"f".repeat(40), "candidate unavailable"), ("git", "", "empty SHA")] {
            let e = shots_gate(bin, &root, sha).unwrap_err();
            assert!(e.starts_with("shots check unknown"), "{why}: {e}");
        }
        assert!(shots_gate("git", &root.join("nowhere"), &bare).unwrap_err().starts_with("shots check unknown"), "no repository");
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: § 7 Result and close templates. Risk: result or close text breaks the template.
    #[test]
    fn result_and_close_follow_section_7() {
        let checks = ["cargo test --locked -- pass\ncargo run -- selftest -- pass".to_string()];
        assert_eq!(result_text(7, "Changed: faster\nOpen: none", &checks, &[], None, &[]).unwrap(), "Changed: faster\nChecks: cargo test --locked -- pass\nChecks: cargo run -- selftest -- pass\nOpen: none");
        let r = result_text(7, "Changed: c\nOpen: none\nDone: shipped\nProblem: keep it?", &["cargo test --locked -- pass".into()], &["close: ship - + done now; - no retry".into(), "Redo - + cleaner; - slower".into()], Some("1"), &[]).unwrap();
        assert_eq!(r, "Changed: c\nChecks: cargo test --locked -- pass\nOpen: none\n\nDone: shipped\nProblem: keep it?\n1. close: ship - + done now; - no retry ★\n2. Redo - + cleaner; - slower\nTo answer: `taskq answer 7.K` or comment `K` or the option text.");
        assert!(result_text(7, "Changed: c\nOpen: none\nDone: d", &checks, &[], None, &[]).unwrap_err().contains("only with --option"));
        assert!(result_text(7, "Changed: c\nOpen: none", &[], &[], None, &[]).unwrap_err().contains("Checks: 0 lines"));
        assert!(result_text(7, "summary", &checks, &[], None, &[]).is_err());
        let body = "## Acceptance\nA1. a\nA2. b\n";
        assert!(close_verdict("Changed: c\nChecks: A1 ok, A2 gap; CI green on abc\nOpen: A2", body).is_ok());
        assert!(close_verdict("Changed: c\nChecks: A1 ok; CI green\nOpen: none", body).unwrap_err().contains("A2"));
        assert!(close_verdict("Changed: c\nChecks: A1 ok, A2 ok\nOpen: none", body).unwrap_err().contains("CI"));
        assert!(close_verdict("A1 ok, A2 ok; c; open: none", body).is_err(), "the old one-line verdict is rejected");
        assert_eq!(formats_selftest().unwrap(), "formats: ask card, result, close verdict PASS");
    }

    // Chain: § 2 card to hand-typed answer. Risk: a hand-typed answer to a rendered card is ignored.
    #[test]
    fn rendered_ask_card_resumes_the_worker_through_the_hand_typed_answer() {
        let b = Fake::default();
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt;
        b.comment(n, &entry("ask", &[], a, &ask_card(n, "Done: read spec\nProblem: which way", &["Left - + fast; - ugly".into(), "Right - + clean; - slow".into()], Some("1"), &[]).unwrap())).unwrap();
        let card = fold(n, &entries(&b, n).unwrap(), false).card.unwrap();
        assert!(card.ends_with(&format!("To answer: `taskq answer {n}.K` or comment `K` or the option text.")), "{card}");
        b.comment(n, "taskq answer 2").unwrap();
        let log = entries(&b, n).unwrap();
        assert_eq!(fold(n, &log, false).state, State::Doing);
        assert_eq!(core::delivery(n, &log, log.last().unwrap().id), "Right - + clean; - slow");
    }

    // Chain: § 5 commands to board entries. Risk: a rejected command writes; close does not publish.
    // Chain: section 7 reviewer close saves one durable verdict; repeated close never closes before a merge.
    #[test]
    fn queued_close_records_intent_once_and_keeps_the_issue_open() {
        let (b, c) = (Fake::default(), cfg());
        let n = b.create("Invented queue task", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        b.state.borrow_mut().publish = "queue-admitted".into();
        let verdict = "Changed: reviewed\nChecks: A1 ok; merge-result CI pending\nOpen: none";
        assert!(close_one(&b, &c, n, verdict, &Op::default()).ok());
        assert!(close_one(&b, &c, n, "duplicate", &Op::default()).ok());
        let t = fold(n, &entries(&b, n).unwrap(), false);
        assert_eq!(t.publication.as_deref(), Some(verdict));
        assert_eq!(t.state, State::Review);
        assert!(b.issue(n).unwrap().open);
        assert_eq!(b.comments(n).unwrap().iter().filter(|e| e.body.starts_with("taskq: publish")).count(), 1);
        b.state.borrow_mut().fail_publication_cancel = true;
        assert!(!act(&b, &c.root, n, "later", &[], "cancel", &Op::default()).ok());
        assert!(fold(n, &entries(&b, n).unwrap(), false).publication.is_some());
        b.state.borrow_mut().fail_publication_cancel = false;
        assert!(act(&b, &c.root, n, "later", &[], "cancel", &Op::default()).ok());
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).state, State::Later);
    }

    // Chain: §7 CLI close -> route-specific verdict -> publication. Adapter tests bypass the CLI validation.
    // Risk: pending queue CI or a distinct merge SHA is rejected, or direct publication loses its exact-SHA check.
    #[test]
    fn cli_close_accepts_merge_result_ci_only_for_deferred_publication() {
        for ci in ["merge-result CI pending", "merge_group CI green on bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"] {
            for queued in [true, false] {
                let b = Fake::default();
                let n = b.create("Invented close", "A1. published", &["taskq".into()]).unwrap();
                let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
                b.comment(n, &entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
                b.state.borrow_mut().publish = if queued { "queue-admitted" } else { "" }.into();
                let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(b) };
                let verdict = format!("Changed: reviewed\nChecks: A1 ok; {ci}\nOpen: none");
                let args = Args::parse(&["close".into(), n.to_string(), "--text".into(), verdict]).unwrap();
                assert_eq!(command(&c, "close", &args).is_ok(), queued);
                let t = fold(n, &entries(c.board.as_ref(), n).unwrap(), false);
                assert_eq!(t.publication.is_some(), queued);
                assert!(c.board.issue(n).unwrap().open);
                if !queued {
                    let verdict = format!("Changed: reviewed\nChecks: A1 ok; CI green on {}\nOpen: none", "a".repeat(40));
                    let args = Args::parse(&["close".into(), n.to_string(), "--text".into(), verdict]).unwrap();
                    command(&c, "close", &args).unwrap();
                    assert!(!c.board.issue(n).unwrap().open);
                }
            }
        }
    }

    #[test]
    fn commands_write_only_accepted_entries_and_close_publishes() {
        let (b, c) = (Fake::default(), cfg());
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        assert!(act(&b, &c.root, n, "steer", &[], "x", &Op::default()).reason.contains("ready"), "steer needs a live worker");
        let before = b.comments(n).unwrap().len();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
        b.comment(n, &entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(a), "1. close: ship it ★\n2. redo the copy")).unwrap();
        assert_eq!(b.comments(n).unwrap().len(), before + 2);
        b.state.borrow_mut().publish = "pending".into();
        assert!(close_one(&b, &c, n, "ok", &Op::default()).reason.contains("pending"));
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).state, State::Review, "pending CI changes nothing");
        b.state.borrow_mut().publish = String::new();
        assert!(answer_picks(&b, &c, &[format!("{n}.3")], &Op::default()).is_err(), "picks are checked before writing");
        assert_eq!(answer_picks(&b, &c, &[format!("{n}.1")], &Op::default()).unwrap().iter().map(Receipt::text).collect::<Vec<_>>(), vec![format!("#{n} closed\ndelivery auto-merge r0: merged 0123456789abcdef0123456789abcdef01234567")]);
        let i = b.issue(n).unwrap();
        assert!(!i.open && !i.labels.iter().any(|l| l.starts_with("q-")));
    }

    // Chain: § 4 one service per root (#933). Risk: a second `service run` on one root drops the first one's children.
    #[cfg(unix)]
    #[test]
    fn a_fresh_status_json_of_another_live_service_holds_service_run() {
        let root = std::env::temp_dir().join(format!("taskq-one-service-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(dot(&root)).unwrap();
        let st = dot(&root).join("status.json");
        assert_eq!(other_service(&root), None, "no status.json");
        let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        fs::write(&st, format!(r#"{{"pid": {}}}"#, other.id())).unwrap();
        assert_eq!(other_service(&root), Some(other.id()), "a live other service");
        fs::write(&st, format!(r#"{{"pid": {}}}"#, std::process::id())).unwrap();
        assert_eq!(other_service(&root), None, "its own status.json after the update re-exec");
        fs::write(&st, format!(r#"{{"pid": {}}}"#, other.id())).unwrap();
        other.kill().unwrap();
        other.wait().unwrap();
        assert_eq!(other_service(&root), None, "a dead writer, e.g. before a restart");
        assert_eq!(service_id("abc").len(), 12);
        assert_ne!(service_id("abc"), "abc");
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: native image replacement -> exact child/input recovery -> accepted input -> exit receipt.
    // Risk: a pending acceptance loses its execution or an old turn accepts the new input.
    #[cfg(unix)]
    #[test]
    fn native_image_handover_preserves_pending_input_and_terminal_receipts() {
        let root=std::env::temp_dir().join(format!("taskq-image-{}",runtime::uuid()));
        fs::create_dir_all(&root).unwrap();
        let result=coreq::process::output("qualification.image-handover",std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","tests::handover_image_child","--ignored","--nocapture"])
            .env("TASKQ_IMAGE_ROOT",&root).env("TASKQ_IMAGE_PHASE","before"),Duration::from_secs(20));
        assert!(result.is_ok(),"image handover failed: {result:?}");
        assert_eq!(fs::read_to_string(root.join("verified")).unwrap(),"same process; exact execution; accepted input; native exit");
        fs::remove_dir_all(root).unwrap();
    }

    // Chain: killed service -> cold restart with a live or already-exited native child -> exact receipt.
    // Risk: restart loses ownership, invents no-start, or replays the pending input.
    #[cfg(unix)]
    #[test]
    fn native_cold_restart_reconciles_the_original_execution() {
        let phases=if cfg!(any(target_os="macos",target_os="linux")) { & ["live","ended","cancel"][..] } else { & ["live","ended"][..] };
        for &phase in phases {
            let root=std::env::temp_dir().join(format!("taskq-cold-{phase}-{}",runtime::uuid()));
            fs::create_dir_all(&root).unwrap();
            let mut command=std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact","tests::handover_image_child","--ignored","--nocapture"])
                .env("TASKQ_IMAGE_ROOT",&root).env("TASKQ_IMAGE_COLD",phase)
                .stdin(std::process::Stdio::null());
            let mut old=command.env("TASKQ_IMAGE_PHASE","before")
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
            let until=Instant::now()+Duration::from_secs(5);
            while !root.join("ready").exists() {
                assert!(old.try_wait().unwrap().is_none(),"old service stopped before its checkpoint");
                assert!(Instant::now()<until); std::thread::sleep(Duration::from_millis(10));
            }
            if phase=="ended" {
                fs::write(root.join("observe"),"").unwrap(); fs::write(root.join("exit"),"").unwrap();
                while !root.join("native-exited").exists() { assert!(Instant::now()<until); std::thread::sleep(Duration::from_millis(10)); }
            }
            old.kill().unwrap();old.wait().unwrap();
            let output=coreq::process::output("qualification.cold-restart",command
                .env("TASKQ_IMAGE_PHASE","after").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()),Duration::from_secs(8));
            assert!(output.is_ok(),"{phase} cold recovery failed: {output:?}");
            assert_eq!(fs::read_to_string(root.join("verified")).unwrap(),"same process; exact execution; accepted input; native exit");
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore="image handover fixture"]
    fn handover_image_child() {
        use std::os::unix::process::CommandExt;
        let root=PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
        fs::create_dir_all(dot(&root)).unwrap();
        let b=Fake::default(); let n=b.create("T","",&[service::TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:session mac").unwrap();
        b.comment(n,&format!("taskq: accepted {a} attempt {a}")).unwrap();
        b.comment(n,&format!("taskq: released {a} exit attempt {a}")).unwrap();
        let input=b.comment(n,"taskq: steer\ncontinue").unwrap();
        let execution=b.comment(n,&format!("taskq: executing {input} attempt {a}")).unwrap();
        let c=Config {root:root.clone(),install:root.join("install"),..cfg()};
        let mut s=Service::new(&b,&c,"fixture",Box::new(|_|panic!("handover must not submit new work")));
        let log=runtime::attempt_log(&root,"worker",n,a);
        let cold=std::env::var("TASKQ_IMAGE_COLD").unwrap_or_default();
        if std::env::var("TASKQ_IMAGE_PHASE").unwrap()=="before" {
            let old="{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\"}\n";
            fs::write(&log,old).unwrap();
            let output=fs::File::options().append(true).open(&log).unwrap();
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","tests::handover_native_child","--ignored","--nocapture"])
                .env("TASKQ_IMAGE_ROOT",&root).stdin(std::process::Stdio::null()).stdout(output).stderr(std::process::Stdio::null()).spawn().unwrap();
            let native=coreq::process::identity(child.id()).unwrap();
            s.kids.push(Kid { unknown:None, stopping:false, session_url:None,execution,task:n,attempt:a,rt:"codex".into(),proc:Box::new(runtime::CliTurn {
                proc:Box::new(child),log,pos:old.len() as u64,start:old.len() as u64,stopping:false,accepted:false,read_error:None,native,recovery:None
            })});
            let launch=runtime::Launch {execution:None,task:n,title:String::new(),attempt:a,role:"worker".into(),rt:"codex".into(),session:"session".into(),url:String::new(),resume:Some("continue".into())};
            s.kids[0].proc.checkpoint(&c,&launch,execution).unwrap();
            assert_eq!(kids_file(&c.install,&root).unwrap().first().map(|k|k.execution),Some(execution),"successful launch must retain its exact execution before any image handover");
            if !cold.is_empty() {
                fs::write(root.join("ready"),"").unwrap();
                let until=Instant::now()+Duration::from_secs(10);
                if cold=="ended" {
                    while s.kids[0].proc.exited().unwrap().is_none() { assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10)); }
                    fs::write(root.join("native-exited"),"").unwrap();
                }
                loop { assert!(Instant::now()<until,"fixture service was not killed");std::thread::sleep(Duration::from_millis(10)); }
            }
            save_kids(&s,&root).unwrap();
            coreq::telemetry::capture::before_exec();
            let error=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","tests::handover_image_child","--ignored","--nocapture"])
                .env("TASKQ_IMAGE_ROOT",&root).env("TASKQ_IMAGE_PHASE","after").exec();
            panic!("exec failed: {error}");
        }
        let checkpoint=fs::read_to_string(kids_path(&c.install,&root).unwrap()).unwrap();
        adopt(&mut s,&c.install,&root).unwrap();
        adopt(&mut s,&c.install,&root).unwrap();
        assert_eq!(s.kids.len(),1,"retry after partial recovery retains the same native handle");
        assert_eq!(s.kids[0].execution,execution); assert_eq!(s.delivered(a),a);
        let pid=s.kids[0].proc.id();
        if cold=="live" && !cfg!(any(target_os="macos",target_os="linux")) {
            assert!(s.kids[0].proc.kill().unwrap_err().contains("recovered process"),"non-child stop needs a stable native control handle");
            assert!(coreq::process::identity(pid).unwrap().is_some(),"failed stop cannot signal a reused PID");
        }
        s.reap_owned(); if cold!="ended" { assert_eq!(s.delivered(a),a,"old log entries cannot accept this input"); }
        fs::write(root.join("observe"),"").unwrap();
        let until=Instant::now()+Duration::from_secs(5);
        while s.delivered(a)!=input {
            s.reap_owned(); assert!(Instant::now()<until,"acceptance not observed: {:?}",s.kids.iter().map(|k|&k.unknown).collect::<Vec<_>>());
            std::thread::sleep(Duration::from_millis(10));
        }
        if cold=="cancel" { s.kids[0].proc.kill().unwrap(); }
        else { fs::write(root.join("exit"),"").unwrap(); }
        if !s.kids.is_empty() {
            while !s.kids[0].proc.exited().is_ok_and(|code|code.is_some()) { assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10)); }
            let saved=fs::read_to_string(kids_path(&c.install,&root).unwrap()).unwrap();
            b.state.borrow_mut().fail_comments=1;
            s.reap_owned();
            assert!(!s.kids.is_empty(),"failed release readback retains the terminal handle");
            assert_eq!(fs::read_to_string(kids_path(&c.install,&root).unwrap()).unwrap(),saved,"recovery record survives a failed terminal receipt");
        }
        while !s.kids.is_empty() {
            s.reap_owned(); assert!(Instant::now()<until,"native exit not observed: {:?}",s.kids.iter().map(|k|&k.unknown).collect::<Vec<_>>());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(coreq::process::identity(pid).unwrap(),None);
        let records=core::executions(n,&board::entries(&b,n).unwrap());
        assert!(records.iter().find(|x|x.id==execution).is_some_and(|x|x.accepted && x.release==Some(core::Release::Exit)));
        if !cold.is_empty() {
            // Crash after the board receipt but before local cleanup: the stale identity must not block startup.
            fs::write(kids_path(&c.install,&root).unwrap(),checkpoint).unwrap();
            adopt(&mut s,&c.install,&root).unwrap();
            assert!(s.kids.is_empty());
        }
        save_kids(&s,&root).unwrap(); assert!(kids_file(&c.install,&root).unwrap().is_empty());
        fs::write(root.join("verified"),"same process; exact execution; accepted input; native exit").unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[ignore="native process for image handover fixture"]
    fn handover_native_child() {
        use std::io::Write;
        let root=PathBuf::from(std::env::var_os("TASKQ_IMAGE_ROOT").unwrap());
        let until=Instant::now()+Duration::from_secs(10);
        while !root.join("observe").exists() { assert!(Instant::now()<until); std::thread::sleep(Duration::from_millis(10)); }
        println!("\n{{\"type\":\"turn.started\"}}"); std::io::stdout().flush().unwrap();
        while !root.join("exit").exists() { assert!(Instant::now()<until); std::thread::sleep(Duration::from_millis(10)); }
    }

    // § 8 re-exec preserves native CLI and Claude identity and defers for app ownership; no unit reload path remains.
    #[cfg(unix)]
    #[test]
    fn live_child_survives_reexec_preparation_and_native_adoption() {
        use runtime::Proc;
        let root = std::env::temp_dir().join(format!("taskq-install-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(dot(&root)).unwrap();
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let mut adopted = runtime::Adopted::new(coreq::process::identity(pid).unwrap().unwrap());
        let mut wrong=coreq::process::identity(pid).unwrap().unwrap(); wrong.started.push_str("-wrong");
        let mut wrong=runtime::Adopted::new(wrong);
        assert_eq!(wrong.exited(),Ok(Some(-1)),"a different native incarnation proves the recorded one is gone");
        wrong.kill().unwrap(); // Terminal identity is a no-op; the current incarnation must remain untouched.
        assert_eq!(adopted.exited(), Ok(None), "the child still runs");
        // the re-exec writes the kids file; the new image adopts the child, not `gone`
        let owner=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let native=coreq::process::identity(pid).unwrap().unwrap();
        let path=kids_path(&root,&root).unwrap();fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("pid {} {} executor-v1\n7 41 codex {pid} {} 43 40\n",owner.pid,owner.started,native.started)).unwrap();
        assert_eq!(kids_file(&root,&root).unwrap(), vec![data::KidLine {task:7,attempt:41,runtime:"codex".into(),process:native,execution:43,offset:40,job:None}]);
        assert_eq!(adopted.exited(), Ok(None), "adopted live");
        // A real process stands in for an app turn: restart must stay pending and keep its lease.
        let b = Fake::default();
        let c = Config { root: root.clone(), ..cfg() };
        let mut s = Service::new(&b, &c, "test", Box::new(|_| unreachable!()));
        s.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution: 41, task: 7, attempt: 41, rt: "codex-app".into(), proc: Box::new(runtime::Adopted::new(coreq::process::identity(pid).unwrap().unwrap())) });
        reexec(&s, &c).unwrap();
        assert!(dot(&root).join("restart").exists());
        assert_eq!(lifecycle_waits(&s, false), ["#7","ownership unknown"], "status names the active task and missing ownership read");
        assert_eq!(s.kids[0].attempt, 41);
        assert_eq!(adopted.exited(), Ok(None));
        s.kids.clear();
        coreq::process::terminate(&coreq::process::identity(pid).unwrap().unwrap()).unwrap();
        child.wait().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    // #1541: an adopted child that exits before the first poll is a zombie whose identity read fails on macOS.
    // Risk: it is never reaped and every later handover pass fails in save_kids.
    #[cfg(unix)]
    #[test]
    fn adopted_zombie_child_is_reaped_and_handover_completes() {
        let root = std::env::temp_dir().join(format!("taskq-zombie-{}", runtime::uuid()));
        fs::create_dir_all(dot(&root)).unwrap();
        let b=Fake::default(); let n=b.create("T","",&[service::TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:session mac").unwrap();
        let c=Config {root:root.clone(),install:root.clone(),..cfg()};
        let mut child=std::process::Command::new("sh").args(["-c","read x"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
        let stdin=child.stdin.take(); let pid=child.id(); drop(child); // the re-exec keeps the pid, not the handle
        let owner=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let native=coreq::process::identity(pid).unwrap().unwrap();
        let path=kids_path(&root,&root).unwrap(); fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("pid {} {} executor-v1\n{n} {a} codex {pid} {} {a} 0\n",owner.pid,owner.started,native.started)).unwrap();
        fs::write(runtime::attempt_log(&root,"worker",n,a),"").unwrap();
        drop(stdin);
        let until=Instant::now()+Duration::from_secs(10);
        let zombie=||{ // exited, not yet reaped
            let mut info:libc::siginfo_t=unsafe{std::mem::zeroed()};
            assert_eq!(unsafe{libc::waitid(libc::P_PID,pid,&mut info,libc::WEXITED|libc::WNOHANG|libc::WNOWAIT)},0);
            #[cfg(target_os="linux")] return unsafe{info.si_pid()}!=0;
            #[cfg(not(target_os="linux"))] return info.si_pid!=0;
        };
        while !zombie() { assert!(Instant::now()<until,"child did not exit"); std::thread::sleep(Duration::from_millis(10)); }
        let mut s=Service::new(&b,&c,"test",Box::new(|_|unreachable!()));
        adopt(&mut s,&root,&root).unwrap();
        assert_eq!(s.kids.len(),1);
        while !s.kids.is_empty() {
            s.reap_owned(); assert!(Instant::now()<until,"zombie not reaped: {:?}",s.kids.iter().map(|k|&k.unknown).collect::<Vec<_>>());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(runtime::liveness(pid),Ok(false),"waitpid reaped the zombie");
        assert!(core::executions(n,&board::entries(&b,n).unwrap()).iter().any(|x|x.id==a && x.release==Some(core::Release::Exit)));
        save_kids(&s,&root).unwrap(); assert!(kids_file(&root,&root).unwrap().is_empty(),"handover completes");
        let _ = fs::remove_dir_all(&root);
    }

    // #1594 A3: the recorded-process verdict reaches pre-boot retirement and the handover checkpoint unchanged.
    // Risk: save_kids or pre_boot re-derive exit from identity and drift from snapshot::verdict (#1561, #1445).
    #[cfg(unix)]
    #[test]
    fn recorded_process_verdict_through_pre_boot_and_handover() {
        let root = std::env::temp_dir().join(format!("taskq-verdict-{}", runtime::uuid()));
        fs::create_dir_all(dot(&root)).unwrap();
        let b=Fake::default(); let (n,m)=(b.create("T","",&[service::TASK_LABEL.into()]).unwrap(),b.create("U","",&[service::TASK_LABEL.into()]).unwrap());
        let (live,ended)=(b.comment(n,"taskq: spawn worker codex:a mac").unwrap(),b.comment(m,"taskq: spawn worker codex:b mac").unwrap());
        let c=Config {root:root.clone(),install:root.clone(),..cfg()};
        let spawn=||std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let kid=|task,attempt,mut child:std::process::Child,exit:bool| {
            let native=coreq::process::identity(child.id()).unwrap();
            if exit {child.kill().unwrap();child.wait().unwrap();}
            Kid {unknown:None,stopping:false,session_url:None,execution:attempt,task,attempt,rt:"codex".into(),proc:Box::new(runtime::CliTurn {
                proc:Box::new(child),log:root.join("log"),pos:0,start:0,stopping:false,accepted:false,read_error:None,native,recovery:None})}
        };
        let mut s=Service::new(&b,&c,"test",Box::new(|_|unreachable!()));
        fs::write(root.join("log"),"").unwrap();
        s.kids=vec![kid(n,live,spawn(),false),kid(m,ended,spawn(),true)];
        let line=|k:&Kid|data::KidLine {task:k.task,attempt:k.attempt,runtime:"codex".into(),process:k.proc.native_identity().unwrap(),execution:k.execution,offset:0,job:None};
        let (a,e)=(line(&s.kids[0]),line(&s.kids[1]));
        assert_eq!(recorded(&a).0,"running");
        assert_eq!(recorded(&e),("ended","recorded native process exited".into()));
        assert!(!pre_boot(&a) && !pre_boot(&e),"a process of this boot is never retired as pre-boot");
        let held=save_kids(&s,&root).unwrap_err();
        assert!(held.contains("recorded native process exited; handover waits"),"{held}");
        b.comment(m,&format!("taskq: released {ended} exit attempt {ended}")).unwrap();
        save_kids(&s,&root).unwrap();
        assert_eq!(kids_file(&root,&root).unwrap(),vec![a],"the ended record resolves from its board exit; the live one is kept");
        for k in &mut s.kids {let _=runtime::stop(k.proc.as_mut());}
        let _ = fs::remove_dir_all(&root);
    }

    // § 8 recovery: corrupt/legacy identities stay unknown and preserved; no timestamp can discard them.
    #[test]
    fn native_recovery_preserves_corrupt_and_legacy_evidence() {
        let root = std::env::temp_dir().join(format!("taskq-handover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(dot(&root)).unwrap();
        let st = dot(&root).join("status.json");
        let kids = kids_path(&root,&root).unwrap(); fs::create_dir_all(kids.parent().unwrap()).unwrap();
        assert!(kids_file(&root,&root).unwrap().is_empty());
        let legacy=dot(&root).join("kids"); fs::write(&legacy,"untrusted project recovery record").unwrap();
        assert!(kids_file(&root,&root).unwrap_err().contains("legacy"));
        assert_eq!(fs::read_to_string(&legacy).unwrap(),"untrusted project recovery record");
        fs::write(&kids, "pid 1\n7 41 claude\n").unwrap();
        assert!(kids_file(&root,&root).is_err());
        assert_eq!(fs::read_to_string(&kids).unwrap(), "pid 1\n7 41 claude\n", "a malformed kids file stays");
        let owner=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let launcher_only=format!("pid {} {}\n7 41 codex 123 child 43 40\n",owner.pid,owner.started);
        fs::write(&kids,&launcher_only).unwrap();
        assert!(kids_file(&root,&root).unwrap_err().contains("legacy launcher"));
        assert_eq!(fs::read_to_string(&kids).unwrap(),launcher_only,"never upgrade old launcher identity implicitly");
        let valid=format!("pid {} {} executor-v1\n7 41 codex 123 child 43 40\n",owner.pid,owner.started);
        fs::write(&kids,&valid).unwrap();
        assert_eq!(kids_file(&root,&root).unwrap(),vec![data::KidLine {task:7,attempt:41,runtime:"codex".into(),process:coreq::process::Identity {pid:123,started:"child".into()},execution:43,offset:40,job:None}]);
        assert_eq!(fs::read_to_string(&kids).unwrap(),valid,"readback never deletes recovery evidence");
        assert!(legacy.exists(),"protected adoption does not delete the legacy artifact");
        fs::write(&kids,format!("pid {} wrong-creation executor-v1\n",owner.pid)).unwrap();
        assert!(kids_file(&root,&root).unwrap().is_empty(),"a different service incarnation permits reconciliation, never blind child control");
        let mut foreign=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","runtime::tests::stop_child","--ignored"])
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let foreign_id=coreq::process::identity(foreign.id()).unwrap().unwrap();
        let foreign_record=format!("pid {} {} executor-v1\n",foreign_id.pid,foreign_id.started);
        fs::write(&kids,&foreign_record).unwrap();
        assert!(kids_file(&root,&root).unwrap_err().contains("still alive"));
        foreign.kill().unwrap();foreign.wait().unwrap();
        assert!(kids_file(&root,&root).unwrap().is_empty());
        assert_eq!(fs::read_to_string(&kids).unwrap(),foreign_record,"native reads never erase the prior owner's record");
        fs::write(&kids, "pid 1\n7 41 claude 123 40\n").unwrap();
        assert!(kids_file(&root,&root).is_err() && kids.exists(), "previous service metadata survives for reconciliation");
        // a corrupt focus pauses nothing away and names its reason
        fs::write(dot(&root).join("focus"), b"\xff").unwrap();
        assert!(pause(&root,false).unwrap_err().contains("not UTF-8"));
        assert_eq!(fs::read(dot(&root).join("focus")).unwrap(), b"\xff");
        assert!(service_fields(&root).contains("Focus: unknown (focus: not UTF-8 text; file kept)"));
        fs::write(&st, "{").unwrap();
        assert!(service_fields(&root).contains("Status: unknown (status.json: corrupt"));
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: § 4 Focus default after install (#974). Risk: a fresh install admits all, or overwrites the owner's focus.
    #[test]
    fn install_writes_focus_off_only_when_missing() {
        let root = std::env::temp_dir().join(format!("taskq-install-focus-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(install_focus(&root), Ok(true));
        assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "off");
        assert!(service_fields(&root).contains("Focus: off") && !service_fields(&root).contains("Paused"));
        // #1592 menu.md `Paused`: focus off with a saved focus prints the Paused line from the display model's mode
        fs::write(dot(&root).join("focus.paused"), "area-maps").unwrap();
        assert!(service_fields(&root).contains("Focus: off\nPaused"), "{}", service_fields(&root));
        fs::write(dot(&root).join("focus"), "area-maps").unwrap();
        assert_eq!(install_focus(&root), Ok(false));
        assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "area-maps");
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: direct shared commands -> receipt refusal -> preserved focus, with safe Pause fallback.
    #[test]
    fn direct_focus_commands_cannot_bypass_required_receipts() {
        let root=std::env::temp_dir().join(format!("taskq-direct-receipt-{}",std::process::id()));
        fs::create_dir_all(dot(&root)).unwrap();
        fs::write(dot(&root).join("focus"),"off").unwrap();
        fs::write(dot(&root).join("focus.paused"),"area-maps").unwrap();
        assert!(resume(&root).unwrap_err().contains("receipt unavailable"));
        assert_eq!(data::focus(&root).unwrap(),Vec::<String>::new());
        assert!(set_focus(&root,"all",None).unwrap_err().contains("receipt unavailable"));
        assert_eq!(data::focus(&root).unwrap(),Vec::<String>::new());
        fs::write(dot(&root).join("focus"),"area-maps").unwrap();
        assert!(pause(&root,false).unwrap_err().contains("applied locally; durable receipt unknown"));
        assert_eq!(data::focus(&root).unwrap(),Vec::<String>::new());
        assert_eq!(fs::read_to_string(dot(&root).join("focus.paused")).unwrap(),"area-maps");
        fs::remove_dir_all(root).unwrap();
    }

    // Chain: initialization refusal -> original error and durable command failure, before any setting write.
    #[test]
    fn focus_initialization_failures_keep_required_receipts() {
        crate::with_test_capture(|store,dir| {
            let root=dir.join("project");fs::create_dir(&root).unwrap();
            fs::write(dot(&root),"preserve this obstacle").unwrap();
            assert!(set_focus(&root,"all",None).unwrap_err().contains("failure receipt"));
            assert!(pause(&root,true).unwrap_err().contains("failure receipt"));
            assert_eq!(fs::read_to_string(dot(&root)).unwrap(),"preserve this obstacle");
            assert!(crate::telemetry::settle(Duration::from_secs(30)));
            let page=store.export(0,0,1024).unwrap();
            let receipts:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"])
                .filter(|r|r["source"]=="command").collect();
            assert_eq!(receipts.iter().map(|r|r["kind"].as_str().unwrap()).collect::<Vec<_>>(),
                ["command.received","command.failed","command.received","command.failed"]);
            assert_ne!(receipts[0]["span"],receipts[2]["span"]);
        });
    }

    // Chain: settings change -> stalled terminal commit -> command returns and frees the signal (#1351) -> receipts land later.
    #[test]
    fn focus_never_waits_on_a_stalled_terminal_receipt() {
        use std::sync::Arc;
        use tracing_subscriber::prelude::*;
        let dir=std::env::temp_dir().join(format!("taskq-commit-lock-{}",std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store=Arc::new(coreq::telemetry::Store::open(&dir.join("db")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
        // Only the action and required receipts enter this collector; its next accepted row is the terminal receipt.
        let layer=capture.clone().with_filter(tracing_subscriber::filter::Targets::new()
            .with_target("taskq::receipt_lock_fixture",tracing::Level::INFO));
        tracing::subscriber::with_default(tracing_subscriber::registry().with(layer),|| {
            let root=dir.join("project");fs::create_dir_all(dot(&root)).unwrap();
            fs::write(dot(&root).join("focus"),"off").unwrap();
            #[cfg(windows)]
            {
                let path=dir.join("raw-read");fs::write(&path,"signal").unwrap();
                let locked=coreq::file_lock::try_lock(&path).unwrap();
                assert!(fs::read_to_string(&path).is_err(),"raw reads bypassed the Windows byte-range lock");
                drop(locked);assert_eq!(fs::read_to_string(path).unwrap(),"signal");
            }
            let db=rusqlite::Connection::open(dir.join("db")).unwrap();
            db.busy_timeout(Duration::from_secs(3)).unwrap();
            let _action=tracing::info_span!(target:"taskq::receipt_lock_fixture","fixture.focus").entered();
            let started=Instant::now();
            change_focus(&root,false,false,|d| {
                db.execute_batch("BEGIN IMMEDIATE").unwrap();
                platform::write_atomic(&d.join("focus"),"all")
            }).unwrap();
            assert!(started.elapsed()<Duration::from_secs(2),"focus waited {:?} on a stalled store",started.elapsed());
            drop(wake_lock(&root).expect("next command blocked behind a terminal receipt commit"));
            db.execute_batch("ROLLBACK").unwrap();
            assert!(crate::telemetry::settle(Duration::from_secs(30)));capture.flush().unwrap();
            let page=store.export(0,0,1024).unwrap();
            let receipts:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"])
                .filter(|r|r["source"]=="command").collect();
            assert_eq!(receipts.len(),2);assert_eq!(receipts[1]["kind"],"command.applied");
            assert_eq!(receipts[0]["span"],receipts[1]["span"]);
            assert_eq!(data::focus(&root).unwrap(),["all"]);
        });
        capture.flush().unwrap();drop(capture);drop(store);fs::remove_dir_all(dir).unwrap();
    }

    // Chain: § 4 pause and resume focus. Risk: resume widens focus to all.
    #[test]
    fn focus_commands_do_not_mutate_while_another_command_or_reader_owns_the_signal() {
        crate::with_test_capture(|_, _| {
            let root=std::env::temp_dir().join(format!("taskq-focus-lock-{}",std::process::id()));
            fs::create_dir_all(dot(&root)).unwrap();
            let d=dot(&root);
            fs::write(d.join("focus"),"off").unwrap();
            fs::write(d.join("focus.paused"),"area-maps").unwrap();
            fs::write(d.join("focus.request"),"owner intent").unwrap();
            fs::write(d.join("poke"),".").unwrap();
            for shared in [false,true] {
                let file=fs::OpenOptions::new().read(true).write(true).open(d.join("poke")).unwrap();
                let _owner=if shared {coreq::file_lock::try_lock_shared_file(file)} else {coreq::file_lock::try_lock_file(file)}.unwrap();
                assert!(resume(&root).is_err());
                assert_eq!(fs::read_to_string(d.join("focus")).unwrap(),"off","Resume changed state before owning its command boundary");
                assert!(set_focus(&root,"all",Some("new intent")).is_err());
                assert_eq!(fs::read_to_string(d.join("focus.request")).unwrap(),"owner intent");
                assert_eq!(fs::read_to_string(d.join("focus")).unwrap(),"off");
                assert!(pause(&root,false).is_err());
                assert_eq!(fs::read_to_string(d.join("focus.paused")).unwrap(),"area-maps");
                assert!(install_focus(&root).is_err());
            }
            assert_eq!(resume(&root).unwrap().0,"area-maps");
            pause(&root,false).unwrap();
            assert_eq!(resume(&root).unwrap().0,"area-maps");
            let context=format!("1 {}.1 {}.2","a".repeat(32),"a".repeat(32));
            fs::write(d.join("poke"),&context).unwrap();
            let mut seen=None;
            let (link,focus)=wake_read(&d.join("poke"),&mut seen,|| {
                assert!(pause(&root,false).is_err(),"writer entered a reader's snapshot");
                data::focus(&root)
            }).unwrap();
            assert_eq!(link.unwrap().encode(),context);assert_eq!(focus,["area-maps"]);
            seen=None;
            fs::write(d.join("focus"),b"\xff").unwrap();
            assert!(wake_read(&d.join("poke"),&mut seen,||data::focus(&root)).is_err());
            assert!(seen.is_none(),"failed state read consumed the command identity");
            fs::write(d.join("focus"),"area-maps").unwrap();
            assert!(wake_read(&d.join("poke"),&mut seen,||data::focus(&root)).unwrap().0.is_some());
            fs::remove_file(d.join("focus.request")).unwrap();fs::create_dir(d.join("focus.request")).unwrap();
            assert!(set_focus(&root,"area-other",None).unwrap_err().contains("partially saved"));
            assert_eq!(data::focus(&root).unwrap(),["area-other"]);
            let signal=fs::read_to_string(d.join("poke")).unwrap();
            assert_ne!(signal,context,"partial state retained the previous command's link");
            assert!(coreq::telemetry::capture::TraceContext::decode(&signal).is_ok());
            fs::remove_dir_all(root).unwrap();
        });
    }

    // Chain: repeated Pause -> Resume preserves the owner's saved scope.
    #[test]
    fn resume_restores_the_saved_focus_and_never_defaults_to_all() {
        crate::with_test_capture(|_, _| {
            let root = std::env::temp_dir().join(format!("taskq-resume-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(dot(&root)).unwrap();
            fs::write(dot(&root).join("status.json"), r#"{"tasks": [{"n": 1, "labels": ["taskq", "q-ready", "area-maps"]}]}"#).unwrap();
            fs::write(dot(&root).join("focus"), "off").unwrap();
            let e = resume(&root).unwrap_err();
            assert!(e.contains("labels of open tasks: area-maps"), "{e}");
            assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "off");
            fs::write(dot(&root).join("focus"), "area-maps").unwrap();
            pause(&root,false).unwrap();
            pause(&root,false).unwrap();
            assert_eq!(fs::read_to_string(dot(&root).join("focus")).unwrap(), "off");
            assert_eq!(resume(&root).unwrap().0, "area-maps", "a second pause keeps the saved focus");
            let _ = fs::remove_dir_all(&root);
        });
    }

    // Chain: R6 human report. Risk: report breaks the R6 format.
    #[test]
    fn report_follows_r6() {
        let (b, c) = (Fake::default(), cfg());
        let n = b.create("Ship | it", "", &["taskq".into()]).unwrap();
        let q = b.create("Q", "", &["taskq".into()]).unwrap();
        b.comment(q, &entry("spawn", &["worker", "claude:abc", "mac"], None, "")).unwrap();
        let a = fold(q, &entries(&b, q).unwrap(), false).attempt.unwrap();
        b.comment(q, &entry("ask", &[], Some(a), "Which way?\nBecause.\n1. left\n2. right ★")).unwrap();
        b.create("taskq host mac", "version 4.0.0", &[service::HOST_LABEL.into()]).unwrap();
        let r = report(&b, &c, &Value::Null, &[]).unwrap();
        assert!(r.contains("In work 0 · Waiting for answer 1 · Ready 1"), "{r}");
        assert!(r.contains(&format!("| [#{n} Ship \\| it](https://board/{n}) | ready |")), "{r}");
        assert!(r.contains(&format!("{q}.1 left · {q}.2 right ★ |")), "only the star without a pick: {r}");
        assert!(r.contains("Hosts: mac v4.0.0"), "{r}");
        assert!(!inbox(&b, q).unwrap().contains("Recommendation:"));
        // § 2: the PM pick and its reason sit beside the worker's star in status, status --json and inbox
        b.comment(q, &entry("pick", &["1"], None, "left | is cheaper")).unwrap();
        let r = report(&b, &c, &Value::Null, &[]).unwrap();
        assert!(r.contains(&format!("{q}.1 left · {q}.2 right ★ · PM {q}.1: left \\| is cheaper |")), "{r}");
        let rows = snapshot::read(&b, &c, &Value::Null, &[]).json()["tasks"].clone();
        let row = rows.as_array().unwrap().iter().find(|t| t["n"] == q).unwrap().clone();
        assert_eq!(row["pick"], json!({"id": 1, "reason": "left | is cheaper"}));
        assert!(inbox(&b, q).unwrap().contains(&format!("Recommendation: worker ★ {q}.2 · PM {q}.1: left | is cheaper")));
    }

    /// A board with one task per report part: ready, ask with a card, blocked, failed, plus a host heartbeat.
    fn snapshot_board() -> Fake {
        let b = Fake::default();
        b.create("Ready one", "", &["taskq".into(), "p1".into()]).unwrap();
        let q = b.create("Question", "", &["taskq".into()]).unwrap();
        b.comment(q, &entry("spawn", &["worker", "claude:abc", "mac"], None, "")).unwrap();
        let a = fold(q, &entries(&b, q).unwrap(), false).attempt.unwrap();
        b.comment(q, &entry("ask", &[], Some(a), "Done: read\nProblem: Which way?\n1. left\n2. right ★")).unwrap();
        let dep = b.create("plain issue", "", &[]).unwrap();
        b.create("Blocked", &format!("Depends on #{dep}"), &["taskq".into()]).unwrap();
        let f = b.create("Broken", "", &["taskq".into()]).unwrap();
        b.comment(f, &entry("failed", &[], None, "Reason: no login")).unwrap();
        b.create("taskq host mac", "version 4.0.0", &[service::HOST_LABEL.into()]).unwrap();
        b.advance(90);
        b
    }

    // Chain: § 5 status read model. Risk: status outputs disagree.
    #[test]
    fn text_json_and_service_status_count_from_one_snapshot() {
        let (b, c) = (snapshot_board(), cfg());
        let snap = snapshot::read(&b, &c, &Value::Null, &[]);
        let text = snap.text(&c, &Value::Null).unwrap();
        let j = snap.json();
        let t = &j["totals"];
        assert_eq!((t["ready"].as_u64(), t["ask"].as_u64(), t["waiting"].as_u64(), t["failed"].as_u64()), (Some(1), Some(1), Some(1), Some(1)));
        let line = format!("In work {} · Waiting for answer {} · Ready {}", t["doing"].as_u64().unwrap() + t["review"].as_u64().unwrap(), t["ask"], t["ready"]);
        assert!(text.contains(&line), "{text}");
        // the service writes the same rows and totals into status.json; `service status` counts them back
        let root = std::env::temp_dir().join(format!("taskq-snap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(dot(&root)).unwrap();
        fs::write(dot(&root).join("focus"), "all").unwrap();
        let mut status = json!({"error": null, "at": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), "tasks_age_seconds": 0});
        for (k, v) in snap.status_keys(None) { status[k] = v; }
        assert_eq!(status["totals"], j["totals"]);
        assert_eq!(status["tasks"], j["tasks"]);
        fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
        assert!(service_fields(&root).contains(&line));
        // a failed pass keeps the last rows for N-1 readers but its counts are unknown
        for (k, v) in snap.status_keys(Some("board down")) { status[k] = v; }
        assert_eq!((status["complete"].clone(), status["totals"].clone(), status["reason"].clone()), (json!(false), Value::Null, json!("board down")));
        fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
        assert!(service_fields(&root).contains("Counts: unknown"));
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: R12 status unknown. Risk: an unreadable board shows zero.
    #[test]
    fn unreadable_board_snapshot_is_incomplete_not_zero() {
        let (b, c) = (Fake::default(), cfg());
        let mut snap = snapshot::read(&b, &c, &Value::Null, &[]);
        snap.reason = Some("board unreadable: 502".into());
        let j = snap.json();
        assert_eq!((j["complete"].clone(), j["reason"].clone(), j["totals"].clone()), (json!(false), json!("board unreadable: 502"), Value::Null));
        assert_eq!(snap.text(&c, &Value::Null).unwrap_err(), "board unreadable: 502");
        assert!(!snapshot::complete(&json!({"error": "board failed", "tasks": []})), "N-1 status.json with an error");
        assert!(snapshot::complete(&json!({"error": null, "tasks": []})), "N-1 status.json after a good pass");
        assert!(!snapshot::complete(&json!({"error": null, "complete": false, "tasks": []})));
    }

    // Chain: § 5 status --json schema. Risk: status JSON drifts from tests/fixtures.
    #[test]
    fn status_json_matches_the_golden_fixture() {
        let snap = snapshot::read(&snapshot_board(), &cfg(), &Value::Null, &[]);
        let got = serde_json::to_string_pretty(&snap.json()).unwrap() + "\n";
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/status-snapshot.json");
        if std::env::var_os("TASKQ_BLESS").is_some() { fs::write(&path, &got).unwrap(); }
        // Parsed compare: a CRLF checkout on Windows must not fail the golden.
        let want: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(snap.json(), want, "TASKQ_BLESS=1 rewrites {}", path.display());
    }

    // Chain: § 2 card -> canonical parsed question -> shared answer card fixtures. Risk: a UI fixture drifts from TaskQ's parse.
    #[test]
    fn answer_card_fixtures_carry_the_canonical_parse() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/answer-card.json");
        let mut fx: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let mut want = fx.clone();
        for row in want["cases"].as_array_mut().unwrap().iter_mut().flat_map(|c| c["rows"].as_array_mut().unwrap()) {
            row["question"] = row["card"].as_str().map_or(Value::Null, snapshot::question);
        }
        if std::env::var_os("TASKQ_BLESS").is_some() {
            fs::write(&path, serde_json::to_string_pretty(&want).unwrap() + "\n").unwrap();
            fx = want.clone();
        }
        assert_eq!(fx, want, "TASKQ_BLESS=1 rewrites {}", path.display());
        // #1787 A3: the Russian fixture card is what `ask` writes, so it passes the § 2 limits counted in characters
        let ru = fx["cases"].as_array().unwrap().iter().find(|c| c["name"] == "labelled options in Russian").unwrap()["rows"][0]["card"].as_str().unwrap();
        let opts: Vec<String> = core::options(ru).into_iter().map(|o| o.1).collect();
        let text = ru.lines().take(2).collect::<Vec<_>>().join("\n");
        assert_eq!(ask_card(880, &text, &opts, Some("1"), &["[#12](https://github.com/o/r/pull/12)".into()]).unwrap(), ru);
        let q = snapshot::question("Done: d\nProblem: p\nmore\nEvidence: [#1](https://x/1), [shot](https://x/s.png)\n1. close: ship ★\n2. redo\nTo answer: `taskq answer 7.K`");
        assert_eq!((q["parsed"].clone(), q["problem"].clone(), q["evidence"][1]["url"].clone(), q["choices"][0]["consequential"].clone()), (json!(true), json!("p more"), json!("https://x/s.png"), json!(true)));
        // #1787: the label shows on the button, the detail beside it; a card without ` - ` keeps its whole text as the label
        let c = snapshot::question("Problem: p\n1. Keep - + safe; - slow ★\n2. redo")["choices"].clone();
        assert_eq!([&c[0]["label"], &c[0]["detail"], &c[1]["label"], &c[1]["detail"]], [&json!("Keep"), &json!("+ safe; - slow"), &json!("redo"), &Value::Null]);
        let img = snapshot::question("Problem: p\nEvidence: [#1](https://x/1), ![front](https://x/f.png#upload=2)\n1. a ★\n2. b")["evidence"].clone();
        assert_eq!(img, json!([{"label": "#1", "url": "https://x/1", "image": false}, {"label": "front", "url": "https://x/f.png#upload=2", "image": true}]), "#1782 images");
        assert_eq!(snapshot::question("1. a ★\n2. b")["parsed"], json!(false), "no Problem: read only");
        assert_eq!(snapshot::question("Problem: p\n1. a\n2. b")["parsed"], json!(false), "no star: read only");
        // #1662 A4: a repeated or skipped number names no single option, so the card stays read only
        for card in ["Problem: p\n1. a ★\n1. b", "Problem: p\n1. a ★\n3. b", "Problem: p\n2. a ★\n1. b"] {
            assert_eq!(snapshot::question(card)["parsed"], json!(false), "{card}");
        }
        let owner = snapshot::question("Problem: add the board token\nTo answer: `taskq answer 7 --text <done>`");
        assert_eq!((owner["parsed"].clone(), owner["choices"].clone()), (json!(true), json!([])), "an owner action takes a text answer");
    }

    // Chain: canonical board read -> approved projection.
    #[test]
    fn enrollment_projects_only_approved_board_metadata() {
        use enrollment::{BoardScope, project};
        let mut snap = snapshot::read(&snapshot_board(), &cfg(), &Value::Null, &[]);
        snap.board["host"] = json!("github.com");
        snap.board["token"] = json!("forge-secret");
        snap.board["url"] = json!("https://user:forge-secret@github.com/o/r");
        let scope = BoardScope::new("github", "GITHUB.COM", "o/r").unwrap();
        let approved = [scope.clone()];
        let local = enrollment::Local { host: "mac".into(), service: None, ttl: 300, agents: Value::Null, progress: Default::default() };
        let a = project(&snap, &"a".repeat(64), &approved, &local).unwrap();
        let got = serde_json::to_value(&a).unwrap();
        let want: Value = serde_json::from_str(include_str!("../tests/fixtures/enrollment-projection.json")).unwrap();
        assert_eq!(got, want);
        let text = got.to_string();
        for omitted in ["forge-secret", "session", "labels", "title", "Which way?", "no login", "url", "hosts"] {
            assert!(!text.contains(omitted), "{omitted}");
        }
        assert!(project(&snap, &"a".repeat(64), &[], &local).is_err());
        assert_eq!(scope, BoardScope::new("github", "github.com", "O/R").unwrap());
        for (host, repo) in [("user:secret@github.com", "o/r"), ("github.com", "o/../r"), ("github.com", "o/r?token=x")] {
            assert!(BoardScope::new("github", host, repo).is_err());
        }
        snap.reason = Some("forge-secret and private path".into());
        let incomplete = serde_json::to_value(project(&snap, &"a".repeat(64), &approved, &local).unwrap()).unwrap();
        assert_eq!((&incomplete["complete"], &incomplete["reason"], &incomplete["totals"]), (&json!(false), &json!("source incomplete"), &Value::Null));
        let other = BoardScope::new("gitlab", "gitlab.com", "o/r").unwrap();
        assert!(project(&snap, &"a".repeat(64), &[other], &local).is_err());
    }

    // Chain: § 5 CLI text limits. Risk: invalid text is written.
    #[test]
    fn cli_audit_boundaries_and_no_write_on_invalid_text() {
        assert_eq!(width("a  b"), 4);
        assert_eq!(width("a`unclosed"), 10);
        assert_eq!(width("é🦀"), 2);
        assert!(one_line("option", &["a\rb".into()]).is_err());
        let text = "Changed: visible\nChecks: A1 ok\nChecks: A2 ok; CI green on abc\nOpen: none";
        close_verdict(text, "A1. a\nA2. b").unwrap();
        assert!(task_text("requeue", "unlabelled reason").is_err());
        assert!(task_text("requeue", &format!("Reason: ok{}", " ".repeat(240))).is_err());
        let e = task_text("requeue", &format!("Reason: {}\nNext: a\nb", "r".repeat(240))).unwrap_err();
        assert!(e.contains("requeue: 3 lines, allowed 1-2") && e.contains("Reason line 1: 248 characters, limit 240") && e.contains("Next: 2 lines, allowed 0-1"), "{e}");
        assert!(task_text("failed", "Reason: blocked\nNext: repair").is_ok());
        assert!(task_text("steer", "1\n2\n3\n4\n5").is_err());
        let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(Fake::default()) };
        let n = c.board.create("T", "", &["taskq".into()]).unwrap();
        c.board.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let before = c.board.comments(n).unwrap();
        for (cmd, text) in [("requeue", "bad"), ("later", "bad"), ("failed", "bad"), ("answer", "1\n2\n3\n4\n5"), ("steer", "1\n2\n3\n4\n5")] {
            let a = Args::parse(&[cmd.into(), n.to_string(), "--text".into(), text.into()]).unwrap();
            assert!(command(&c, cmd, &a).is_err(), "{cmd}");
        }
        assert_eq!(c.board.comments(n).unwrap().len(), before.len());
        run(&["--help".into()]).unwrap();
        run(&["service".into(), "pause".into(), "--help".into()]).unwrap();
        run(&["close".into(), "12".into(), "--help".into()]).unwrap();
        assert!(USAGE.contains("--recommend K"));
        assert!(USAGE.contains("--snapshot DIR"));
        assert!(USAGE.contains("taskq service capacity [--host H] [--max N|none] [--claude N] [--codex N] [--codex-app N] [--load-gate on|off] [--cpu-max P] [--memory-min P] [--claim any|labelled] [--default-runtime claude|codex]"));
        run(&["service".into(), "capacity".into(), "--help".into()]).unwrap();
        let e = platform::error_text("#7 is ready", None);
        assert!(e.starts_with("Error: #7 is ready\nNext: "));
        assert_eq!(e.lines().count(), 2);
        let dep = c.board.create("ordinary issue", "", &[]).unwrap();
        let blocked = c.board.create("blocked", &format!("Depends on #{dep}"), &["taskq".into(), "selected".into()]).unwrap();
        let report = report(c.board.as_ref(), &c.cfg, &c.raw, &["selected".into()]).unwrap();
        assert!(report.contains(&format!("#{blocked} blocked")));
        assert!(report.contains(&format!("blocked ([#{dep}](https://board/{dep}) open)")), "non-task dependency remains visible outside filter");
    }

    // Chain: R6 compact report. Risk: compaction drops choices, links or evidence.
    #[test]
    fn compact_report_keeps_choices_links_and_original_evidence() {
        let (b, c) = (Fake::default(), cfg());
        let title = "🦀".repeat(65);
        let n = b.create(&title, "", &["taskq".into()]).unwrap();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let reason = "r".repeat(170);
        let option = "x".repeat(125);
        let card = format!("Done: inspected\nProblem: {reason}\nEvidence: [context](https://context)\n1. {option} ★\n2. keep\nTo answer: exact syntax");
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt;
        b.comment(n, &entry("ask", &[], a, &card)).unwrap();
        let r = report(&b, &c, &Value::Null, &[]).unwrap();
        assert!(r.contains(&format!("{}…", "🦀".repeat(59))));
        assert!(r.contains(&format!("[{}…](https://board/{n})", "r".repeat(159))));
        assert!(r.contains(&format!("{n}.1 [{}…](https://board/{n}) ★", "x".repeat(119))));
        assert!(r.contains(&format!("{n}.2 keep")));
        assert!(!r.contains("Done: inspected"));
        assert!(!r.contains("To answer:"));
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).card.as_deref(), Some(card.as_str()));
        assert_eq!(b.issue(n).unwrap().title, title);
        assert_eq!(snapshot::brief("Old question?\n1. keep\n2. change"), "Old question?");
    }

    // Chain: § 2 owner action (#1125): ask without options, R6 row, answer resumes the attempt. Risk: an action card is
    // refused, shows no action in status, or its answer starts a new attempt.
    #[test]
    fn owner_action_waits_in_status_and_its_answer_resumes_the_attempt() {
        let (b, c) = (Fake::default(), cfg());
        let n = b.create("t", "", &["taskq".into()]).unwrap();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt;
        b.comment(n, &entry("ask", &[], a, &ask_card(n, "Done: tests pass\nProblem: add the board token", &[], None, &[]).unwrap())).unwrap();
        let r = report(&b, &c, &Value::Null, &[]).unwrap();
        assert!(r.contains("| waiting for owner: add the board token |"), "{r}");
        b.comment(n, &entry("answer", &[], None, "token added")).unwrap();
        let t = fold(n, &entries(&b, n).unwrap(), false);
        assert_eq!((t.state, t.attempt, t.inbox.len()), (State::Doing, a, 1));
    }

    // Chain: R12 service status fields. Risk: missing data shown as known; failed pause reported ok.
    #[test]
    fn service_fields_keep_missing_data_unknown_and_pause_reports_failure() {
        let root = std::env::temp_dir().join(format!("taskq-fields-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let s = service_fields(&root);
        assert!(s.contains("Focus: unknown"));
        assert!(s.contains("Counts: unknown"));
        assert!(s.contains("Freshness: unknown"));
        assert!(!s.contains("Ready 0"));
        assert!(pause(&root,false).is_err(), "missing state folder cannot acknowledge pause");
        fs::create_dir_all(dot(&root)).unwrap();
        fs::write(dot(&root).join("focus"), "one two").unwrap();
        let mut status = json!({"error": null, "at": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), "tasks_age_seconds": 0, "tasks": [{"state":"doing"},{"state":"review"},{"state":"ask"},{"state":"ready"}]});
        fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
        let s = service_fields(&root);
        assert!(s.contains("Focus: one\nFocus: two"));
        assert!(s.contains("In work 2 · Waiting for answer 1 · Ready 1"));
        assert!(s.contains("cached status"));
        assert!(!s.contains("Service:"),"project cache cannot claim native host health");
        // Capacity-only publication keeps the file fresh without refreshing the board counts.
        status["tasks_age_seconds"] = json!(301);
        status["capacity"] = json!({"gate": "off"});
        fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
        assert!(file_age(&dot(&root).join("status.json")).unwrap() <= 1);
        assert!(service_fields(&root).contains("Counts: unknown"));
        status["tasks_age_seconds"] = Value::Null;
        fs::write(dot(&root).join("status.json"), status.to_string()).unwrap();
        assert!(service_fields(&root).contains("Counts: unknown"), "old snapshots lack board freshness");
        fs::write(dot(&root).join("status.json"), r#"{"error":"board failed","tasks":[]}"#).unwrap();
        assert!(service_fields(&root).contains("Counts: unknown"));
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: § 5 error diagnostics. Risk: a compact error loses its full diagnostic.
    // Risk: adding a full poll after native work drifts heartbeat cadence; a pre-pass poke must still wake it.
    #[test]
    fn completed_pass_does_not_add_another_poll_interval() {
        let dir=std::env::temp_dir().join(format!("taskq-poll-{}",runtime::uuid()));fs::create_dir(&dir).unwrap();
        let paths=[dir.join("project"),dir.join("host")];let before=poke_times(&paths);
        let mut ticks=0;let began=Instant::now();
        sleep_until_poke(&paths,before,began-Duration::from_secs(1),&mut ||ticks+=1);
        fs::write(&paths[0],"wake").unwrap();
        sleep_until_poke(&paths,before,began+Duration::from_secs(60),&mut ||ticks+=1);
        assert_eq!(ticks,0);assert!(began.elapsed()<Duration::from_secs(1));fs::remove_dir_all(dir).unwrap();
    }

    // Chain: tray write -> reread request -> next pass; open window -> short poll (#1819). Risk: a concurrent append
    // is lost, the reread repeats every pass, or a closed menu keeps the board polled every 15 s.
    #[test]
    fn tray_writes_reread_once_and_an_open_menu_polls_short() {
        let dir=std::env::temp_dir().join(format!("taskq-reread-{}",runtime::uuid()));fs::create_dir_all(dot(&dir)).unwrap();
        ask_reread(&dir,7).unwrap();ask_reread(&dir,9).unwrap();
        assert!(dot(&dir).join("poke").exists(),"the write wakes the service");
        assert_eq!(take_reread(&dot(&dir)),[7,9]);
        assert_eq!(take_reread(&dot(&dir)),Vec::<u64>::new(),"taken once");
        assert!(!menu_open(&dir));
        touch(&dir.join("menu-open"));
        assert!(menu_open(&dir));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn compact_errors_retain_complete_diagnostics() {
        let root = std::env::temp_dir().join(format!("taskq-error-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let original = format!("#7 {}\nexact evidence", "é".repeat(200));
        let e = platform::error_text(&original, Some(&root));
        assert_eq!(e.lines().count(), 2);
        assert!(width(e.lines().next().unwrap()) <= 160);
        assert!(e.contains("cli-errors.log"));
        assert_eq!(fs::read_to_string(dot(&root).join("cli-errors.log")).unwrap(), format!("{original}\n"));
        let failed = platform::error_text(&original, None);
        assert!(failed.contains(&original), "a failed save never discards the original evidence");
        // #1555: no bare `could not be saved`; it says why
        assert!(failed.ends_with("no cli-errors.log copy (outside a taskq project)."), "{failed}");
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: MP14 Completed. Risk: a page invents an outcome, loses board paging or hides the PR link.
    #[test]
    fn completed_reads_board_pages_with_sourced_outcome() {
        let b = Fake::default();
        for n in 1..=12u64 {
            b.create(&format!("t{n}"), "", &["taskq".into()]).unwrap();
            b.close(n).unwrap();
            let mut st = b.state.borrow_mut();
            let i = st.issues.iter_mut().find(|i| i.number == n).unwrap();
            (i.updated, i.reason) = (100 + n as i64, [Some("completed".into()), Some("not_planned".into()), None][n as usize % 3].clone());
        }
        b.comment(12, "Merged https://github.com/o/r/pull/7.").unwrap();
        b.state.borrow_mut().issues.iter_mut().find(|i| i.number == 12).unwrap().updated = 112;
        let first = closed_history(&b, 1, None, false).unwrap();
        assert_eq!((first["tasks"].as_array().unwrap().len(), first["more"].clone()), (10, json!(true)));
        assert_eq!((first["tasks"][0]["n"].clone(), first["tasks"][0]["outcome"].clone(), first["tasks"][0]["pr"].clone()), (json!(12), json!("success"), json!("https://github.com/o/r/pull/7")));
        assert_eq!((first["tasks"][1]["outcome"].clone(), first["tasks"][2]["outcome"].clone()), (json!("unknown"), json!("not_planned")), "GitHub without a reason stays unknown");
        let last = closed_history(&b, 2, None, false).unwrap();
        assert_eq!((last["tasks"].as_array().unwrap().len(), last["more"].clone()), (2, json!(false)));
        assert_eq!(closed_history(&b, 1, None, true).unwrap()["tasks"][1]["outcome"], "success", "§ 1: a closed GitLab issue is done");
        assert_eq!(closed_history(&b, 1, Some(110), false).unwrap()["tasks"].as_array().unwrap().len(), 3, "period keeps closes at or after since");
    }

    // #1829 A1/A2: a pin outside page one reads its exact board outcome; native reads cannot resume work or cross hosts.
    #[test]
    fn closed_pin_reads_exact_issue_and_fences_transcript() {
        let b = Fake::default();
        for n in 1..=12 { b.create(&format!("pin {n}"), "", &["taskq".into()]).unwrap(); b.close(n).unwrap(); }
        assert!(closed_history(&b,1,None,false).unwrap()["tasks"].as_array().unwrap().iter().all(|t|t["n"]!=1));
        let closed = b.issue(1).unwrap().closed;
        b.comment(1,"A later comment must not reset the closure time").unwrap();
        let pin = closed_pin(&b,1,false).unwrap()["tasks"][0].clone();
        assert_eq!((pin["n"].clone(),pin["state"].clone(),pin["outcome"].clone(),pin["closed"].clone()),(json!(1),json!("closed"),json!("success"),json!(closed)));
        b.state.borrow_mut().issues[0].reason=None;
        assert_eq!(closed_pin(&b,1,false).unwrap()["tasks"][0]["outcome"],"unknown");
        assert_eq!(closed_pin(&b,1,true).unwrap()["tasks"][0]["outcome"],"success","§ 1 GitLab has no closure reason");
        b.state.borrow_mut().issues[0].open=true;
        assert_eq!(closed_pin(&b,1,true).unwrap()["tasks"][0]["state"],"open");
        assert!(closed_transcript(&b,1,"mac",None).unwrap_err().contains("closed trusted task"));
        let spawn=b.comment(1,"taskq: spawn worker codex:invented1829 remote").unwrap();
        b.comment(1,&format!("taskq: result {} attempt {spawn}","a".repeat(40))).unwrap();
        b.close(1).unwrap();
        assert!(closed_transcript(&b,1,"mac",None).unwrap_err().contains("on host remote"));
        b.state.borrow_mut().issues[0].trusted=false;
        assert!(closed_pin(&b,1,true).unwrap_err().contains("trusted task"));
        assert!(closed_pin(&b,999,false).is_err());
    }

    // Chain: #1679 A1 § 5 `status --closed [--page N]`. Risk: `--closed` eats `--page` as its value (before: `unused
    // positional arguments: 1`), so the tray Completed view never loads.
    #[test]
    fn status_closed_is_a_flag_and_pages() {
        let parse = |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        for v in [&["status", "--closed", "--page", "1"][..], &["status", "--closed"], &["status", "--page", "2", "--closed", "--since", "5"]] {
            let a = parse(v).unwrap();
            assert!(a.flags.contains_key("closed") && a.pos == ["status"], "{v:?}");
        }
        assert_eq!(parse(&["status", "--closed", "--page", "2"]).unwrap().one("page"), Some("2"));
        let c = Ctx { cfg: cfg(), raw: json!({}), board: Box::new(Fake::default()) };
        for v in [&["status", "--closed", "--page", "1"][..], &["status", "--closed"]] {
            command(&c, "status", &parse(v).unwrap()).unwrap();
        }
    }

    // Chain: #1679 A3/A4 MP14 Merged. Risk: a closed issue with a PR link, a direct fast-forward or an edited close
    // reads as a confirmed merge; only a PR/MR merge receipt on a `close` entry is evidence.
    #[test]
    fn completed_merge_evidence_comes_only_from_a_close_receipt() {
        let b = Fake::default();
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let bodies = [
            format!("taskq: close attempt 9
Changed: x
delivery auto-merge r0: merged #7 as {sha}; merge-result CI green"),
            "taskq: close
delivery auto-merge r0: merged !3".to_string(),
            "taskq: result abc attempt 9
Changed: https://github.com/o/r/pull/7".to_string(),
            format!("taskq: close
delivery direct r0: main fast-forwarded to {sha}"),
            "taskq: requeue
delivery auto-merge r0: merged #7 as x".to_string(),
        ];
        for (k, body) in bodies.iter().enumerate() {
            let n = k as u64 + 1;
            b.create(&format!("t{n}"), "", &["taskq".into()]).unwrap();
            b.comment(n, body).unwrap();
            b.close(n).unwrap();
        }
        b.create("t6", "", &["taskq".into()]).unwrap();
        b.raw(6, &bodies[0], true, true);
        b.close(6).unwrap();
        let page = closed_history(&b, 1, None, false).unwrap();
        let merged = |n: u64| page["tasks"].as_array().unwrap().iter().find(|t| t["n"] == n).unwrap()["merged"].clone();
        assert_eq!(merged(1), json!(format!("merged #7 as {sha}; merge-result CI green")));
        assert_eq!(merged(2), json!("merged !3"));
        for n in 3..=6 { assert_eq!(merged(n), Value::Null, "#{n}: no merge evidence stays unknown"); }
    }

    // Chain: #1800 § 7 close outcome -> `status --closed` `close`. Risk: a close for a successor or with unknown
    // acceptance items reads as delivered work (pilotq#90 closed for #104 while #104 stayed held).
    #[test]
    fn completed_names_the_close_outcome() {
        let b = Fake::default();
        let succ = b.create("successor", "", &["q-held".into()]).unwrap();
        b.comment(succ, &entry("hold", &[], None, "")).unwrap();
        let bodies = [
            format!("taskq: close\nChanged: none; closes as superseded by #{succ}\nChecks: A1 unknown, A2 unknown"),
            "taskq: close attempt 9\nChanged: x\nChecks: A1 ok, A2 unknown, A3 gap, A4 unknown".into(),
            "taskq: close\nChanged: none; duplicate".into(),
        ];
        let ns: Vec<u64> = bodies.iter().map(|body| {
            let n = b.create("t", "", &["taskq".into()]).unwrap();
            b.comment(n, body).unwrap();
            b.close(n).unwrap();
            n
        }).collect();
        let bare = b.create("t", "", &["taskq".into()]).unwrap();
        b.close(bare).unwrap();
        let page = closed_history(&b, 1, None, false).unwrap();
        let close = |n: u64| page["tasks"].as_array().unwrap().iter().find(|t| t["n"] == n).unwrap()["close"].clone();
        assert_eq!([close(ns[0]), close(ns[1]), close(ns[2]), close(bare)],
            [json!(format!("replaced by #{succ} (held)")), json!("delivered, 3 items unverified"), json!("no change"), json!("closed on the board")]);
    }
}
