//! The `taskq` command line (taskq.md § 5). Every write is one `taskq:` comment that `fold` accepts.

/// One service log line (§ 4): stamped and folded in `service run`, plain on the CLI.
macro_rules! log {
    ($($t:tt)*) => { $crate::platform::log(&format!($($t)*)) };
}

use runtime::accounts;
mod admission;
mod check;
#[cfg(any(target_os="macos",windows,test))]
mod agentq;
mod host;
mod host_control;
mod external_slots;
mod board;
mod core;
mod control;
mod data;
mod doctor;
mod health;
mod deploy;
mod dispatch;
mod display;
mod enrollment;
mod incident;
mod incoming;
mod labels;
mod balance;
mod link;
mod recipe;
mod requests;
mod reply_files;
mod platform;
mod service_versions;
mod readiness;
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
mod tools;
mod runtime;
mod service;
mod setup;
mod dialogue;
mod transcript;
mod messages;
mod conversations;
mod snapshot;
mod reports;
mod summary;
#[cfg(any(target_os = "macos", windows))]
mod icon;
#[cfg(any(target_os = "macos", windows))]
mod notify;
#[cfg(any(target_os = "macos", windows))]
mod tray;
#[cfg(any(target_os = "macos", windows))]
mod ui_bundle;
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
 taskq reroute --from <runtime> --to <runtime> [--json]
 taskq accounts [--json] | taskq accounts enable|disable|move <runtime> <label> [<position>] | taskq accounts add <runtime> <label> | taskq accounts fleet use|avoid|clear <runtime> <id>
 taskq labels
 taskq add <title> --goal G --acceptance A [--acceptance B..] [--scope P..] [--deps N..] [--type T] [--runtime R] [--priority 0|1|2] [--host H] [--rules IDs|all] [--gate] [--conversation ID --op-id ID]
 taskq finding <title> --board URL --source installation/rule/episode --goal G --acceptance A.. [--evidence E..] [--start] [--json]
 taskq start N [M..]
 taskq ask N --text Q --option O.. --recommend K [--link URL..]
 taskq answer N --text A | taskq answer N.K [M.K..] | taskq pick N.K --text R
 Human replies: answer, steer, requeue accept --text-stdin or --text-file PATH instead of --text; attach with --file PATH (repeat) or --files-from PATH (JSON array)
 JSON files: status, answer, steer, requeue, later, close, start, failed accept --json --json-file PATH
 taskq result N --sha SHA --checks C.. --text T [--option O.. --recommend K --link URL..]
 taskq requeue N [--text T] | taskq later N [--text T] [--blocker #M|--until UTC] [--stage A<k>..]
 taskq steer N --text T | taskq failed N --text T
 taskq close N [M..] [--text VERDICT] | taskq gate N | taskq approve N [--sha SHA]
 taskq recipe NAME --run ID [--goal G] [--start]
 taskq status [--project P..] [--only L,..] [--json] [--timing] | taskq status --closed [--page N] [--since T] [--task N [--transcript [--before B]]] | taskq inbox N [--json]
 taskq issue N | taskq issue N --comment T|--label L [--op-id ID] [--json]
 taskq claim-issue [N] [--json]
 taskq deployed --version V --env E --range A..B [--host H] [--json]
 taskq results [N] [--action open|read|accept|followup|reopen|later|drop|make] [--sha SHA] [--text REASON] [--until UTC] [--page N|--before TIME:N] --json
 taskq reports ... (alias of results)
 taskq profile [guide|export|tasks|board|actions|health|watch|migrate] [--since 24h] | taskq retro
 taskq wait [--window MIN] [--only L,..]
 taskq cleanup [--force] [--dry-run] [--all] | taskq doctor [--dry-run] [--hosts] [--json] | taskq update [--check]
 taskq enroll <https-url> [--headless] [--telemetry] | taskq connection status | taskq disconnect
 taskq service install|run|pause|resume|status|restart|stop|uninstall|list
 taskq service setup [--runtime claude|codex] [--client C] [--target PATH..] [--json]
 taskq service onboard
 taskq service dev-build (macOS channel dev)
 taskq service app-server --action prepare|launch|observe|cancel|reconcile --request JSON
 taskq service slots --identity PID | --action reserve|observe|release --request JSON [--json]
 taskq service stop [--mode drain|full] (macOS)
 taskq service focus [all|off|<label..>] [--request TEXT]
 taskq balance [claude=N codex=N antigravity=N | --clear]
 taskq service capacity [--host H] [--max N|none] [--claude N] [--codex N] [--antigravity N] [--codex-app N] [--load-gate on|off] [--cpu-max P] [--memory-min P] [--claim primary|any|spillover|pinned-only|labelled|manager-only|none] [--default-runtime claude|codex|antigravity]
 taskq service permissions [--runtime R --mode restricted|workspace|full|none [--scope host|project]]
 taskq service resources [--grant ID --actor ID --hours N [--project P].. [--runtime R].. [--host-policy] [--project-limits] | --revoke ID]
 taskq service pm [--grant ID --actor ID --hours N --project P.. --runtime R.. | --revoke ID] [--json]
 taskq tray [--open | --snapshot DIR [--route R,..]] | taskq selftest | taskq --version | taskq help [--json]
Writes to a task take [--op-id ID] [--revision R] [--json] (taskq.md section 5).
Text templates and limits: taskq.md sections 1, 2, 5, 7.";

/// #1910: the one PM command catalog; `taskq help` renders it, `taskq help --json` prints it for other PM surfaces.
const PM_COMMANDS: &str = include_str!("pm-commands.json");

fn help(json: bool) -> Res<String> {
    if json { return Ok(PM_COMMANDS.trim_end().into()); }
    let catalog: Value = serde_json::from_str(PM_COMMANDS).map_err(|e| format!("PM command catalog: {e}"))?;
    let mut out = format!("{USAGE}

PM commands (catalog: taskq help --json):
");
    for c in catalog["commands"].as_array().into_iter().flatten() {
        out += &format!("  {}
      {}
", c["use"].as_str().unwrap_or_default(), c["purpose"].as_str().unwrap_or_default());
    }
    out += "PM procedures:
";
    for p in catalog["procedures"].as_array().into_iter().flatten() {
        out += &format!("  '{}': {}; procedure: {}
", p["trigger"].as_str().unwrap_or_default(), p["command"].as_str().unwrap_or_default(), p["doc"].as_str().unwrap_or_default());
    }
    Ok(out.trim_end().into())
}

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
                    let val = if matches!(k, "text-stdin" | "later" | "hold" | "start" | "check" | "force" | "help" | "json" | "apply" | "headless" | "telemetry" | "host-policy" | "project-limits" | "closed" | "open" | "transcript" | "dry-run" | "all" | "clear" | "timing" | "gate") || k=="hosts"&&a.pos==["doctor"] || k=="host"&&a.pos==["service","uninstall"] { String::new() } else { it.next().cloned().ok_or(format!("--{k} needs a value"))? };
                    a.flags.entry(k.to_string()).or_default().push(val);
                }
                None => a.pos.push(x.clone()),
            }
        }
        let used = match a.pos.first().map(String::as_str) {
            Some("close" | "start" | "balance") => a.pos.len(),
            Some("answer") if a.pos.get(1).is_some_and(|p| p.contains('.')) => a.pos.len(),
            Some("service") if a.pos.get(1).is_some_and(|p| p == "focus") => a.pos.len(),
            Some("service") if a.pos.get(1).is_some_and(|p| p == "channel") => 3,
            Some("rules") => 6,
            Some("accounts") => 5,
            Some("service" | "profile" | "add" | "finding" | "approve" | "gate" | "recipe" | "ask" | "answer" | "pick" | "result" | "requeue" | "later" | "failed" | "steer" | "inbox" | "issue" | "claim-issue" | "reports" | "results" | "enroll" | "connection") => 2,
            _ => 1,
        };
        if a.pos.len() > used {
            return Err(format!("unused positional arguments: {}", a.pos[used..].join(" ")));
        }
        if a.flags.contains_key("text-stdin") || a.flags.contains_key("text-file") {
            if ["text", "text-stdin", "text-file"].iter().filter(|k| a.flags.contains_key(**k)).count() != 1
                || !matches!(a.pos.first().map(String::as_str), Some("answer" | "steer" | "requeue")) || a.pos.get(1).is_some_and(|n| n.contains('.')) {
                return Err("use one text source (--text, --text-stdin or --text-file) for answer, steer or requeue without a numbered choice".into());
            }
        }
        if a.flags.contains_key("json-file") && (!a.flags.contains_key("json") || a.flags.contains_key("closed")
            || !matches!(a.pos.first().map(String::as_str), Some("status" | "answer" | "steer" | "requeue" | "later" | "close" | "start" | "failed"))) {
            return Err("--json-file requires --json with status or a task control/reply command (not --closed)".into());
        }
        let worker_files = a.flags.contains_key("file") && !a.flags.contains_key("files-from") && matches!(a.pos.first().map(String::as_str), Some("result" | "ask"));
        if (a.flags.contains_key("file")||a.flags.contains_key("files-from")) && !worker_files && (!matches!(a.pos.first().map(String::as_str),Some("answer"|"steer"|"requeue"))||a.pos.get(1).is_some_and(|n|n.contains('.'))) {
            return Err("--file requires answer N, steer N, owner requeue N, result N or ask N; choose a text reply instead of a numbered option".into());
        }
        if a.all("files-from").len()>1{return Err("pass --files-from once with a JSON array of file paths".into());}
        Ok(a)
    }
    fn read_text(&mut self, input: &mut dyn std::io::Read) -> Res<()> {
        if let Some(paths)=self.flags.remove("files-from") {
            let data=fs::read(&paths[0]).map_err(|e|format!("Cannot read attachment list: {e}; supply a readable JSON file"))?;
            let files:Vec<String>=serde_json::from_slice(&data).map_err(|e|format!("Attachment list: {e}; use a JSON array of file paths"))?;
            self.flags.entry("file".into()).or_default().extend(files);
        }
        if self.flags.remove("text-stdin").is_some() {
            let mut text = String::new();
            input.read_to_string(&mut text).map_err(|e| format!("cannot read UTF-8 reply from stdin: {e}"))?;
            self.flags.insert("text".into(), vec![text]);
        } else if let Some(path) = self.flags.remove("text-file") {
            let text = fs::read_to_string(path.last().ok_or("--text-file needs a path")?).map_err(|e| format!("cannot read UTF-8 reply file: {e}"))?;
            self.flags.insert("text".into(), vec![text]);
        }
        Ok(())
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

// R10 A1: each status block's live hosts and their served-project counts, for the installation's anomaly rows
thread_local!(static SERVED: std::cell::RefCell<Vec<Vec<(String, usize)>>> = const { std::cell::RefCell::new(vec![]) });
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

/// § 5 Board-only root: a root without `.git` holds no code checkout, so commands that would spawn a worker or tidy
/// worktrees there refuse with one line naming the reason; the board commands run as in any root.
fn needs_checkout(root: &Path, what: &str) -> Res<()> {
    if root.join(".git").exists() { return Ok(()); }
    Err(format!("{what}: board-only root: no code checkout (.git) here\nNext: run it in a checkout of this project on a host that serves it"))
}

fn hostname() -> String {
    // ponytail: first nonempty name per process (#1297: was one `hostname` child per call); a renamed host needs a service restart.
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(name) = NAME.get() { return name.clone(); }
    let name = match coreq::process::output("host.identity", &mut crate::platform::command("hostname"), std::time::Duration::from_secs(10)) {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        // #1856: an empty name falls back to TASKQ_HOST or config; the log keeps the failed call
        Err(e) => { platform::log(&format!("host.identity: `hostname` failed: {e}")); String::new() }
    };
    if !name.is_empty() { let _ = NAME.set(name.clone()); }
    name
}

fn load(root: &Path) -> Res<(Config, Value)> {
    load_value(root, project_value(root)?)
}

fn load_value(root: &Path, v: Value) -> Res<(Config, Value)> {
    let (mut cfg,raw)=project_config_value(root, v)?;
    if let Ok(policy)=host::Policy::read(&cfg.install) {policy.apply(&mut cfg);}
    Ok((cfg,raw))
}

/// § 6 setup check: `load`, except that `publish` disagreeing with `delivery` is the `delivery` item, not an error.
fn check_load(root: &Path) -> Res<(Config, Value, Result<String, String>)> {
    let v = project_value(root)?;
    match delivery(&v) {
        Ok((_, preset, _)) => load_value(root, v).map(|(c, raw)| (c, raw, Ok(preset))),
        Err(e) if e.contains(" disagrees with ") => {
            let mut lenient = v;
            if let Some(o) = lenient.as_object_mut() { o.remove("publish"); }
            load_value(root, lenient).map(|(c, raw)| (c, raw, Err(e)))
        }
        Err(e) => Err(e),
    }
}

fn project_value(root: &Path) -> Res<Value> {
    let p = root.join("taskq.json");
    serde_json::from_str(&fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))
}

fn project_config_value(root:&Path, v: Value)->Res<(Config,Value)> {
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
        claim: host::ClaimMode::Any,
        spillover_grace: 300,
        weights: BTreeMap::new(),
        allowed_runtimes: None,
        auto_reroute: false,
        default_runtime: crate::runtime::OWNER.into(),
        routing: labels::routing(&v)?,
        permission_mode: str_or("permission_mode", "dontAsk"),
        codex,
        ttl: secs("ttl", TTL as u64) as i64,
        poll: secs("poll", 60),
        update_every: secs("update_every", 60),
        rollback_after: secs("rollback_after", 600),
        external: v["workspace"].as_str() == Some("external"),
        host_setup: v["host_setup"].as_str().filter(|s| !s.is_empty()).map(String::from),
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
    #[cfg(windows)] wake_filesystem(root)?;
    coreq::telemetry::capture::operation("wake.lock", || {
        fs::create_dir_all(dot(root)).map_err(|error| ("wake-directory", error))?;
        let file = fs::OpenOptions::new().write(true).create(true).truncate(false).open(dot(root).join("poke")).map_err(|error| ("wake-open", error))?;
        coreq::file_lock::try_lock_file(file).map_err(|error| ("wake-lock", std::io::Error::from(error)))
    }, |(class, _)| *class).map_err(|(class, error)| format!("service wake failed ({class}: {error}); check service status"))
}

#[cfg(windows)]
fn wake_filesystem(root:&Path)->Res<()> {
    use std::path::{Component,Prefix};
    let wsl=|path:&Path|match path.components().next() {
        Some(Component::Prefix(p))=>match p.kind() {
            Prefix::UNC(server,_)|Prefix::VerbatimUNC(server,_)=>{
                let server=server.to_string_lossy();server.eq_ignore_ascii_case("wsl.localhost")||server.eq_ignore_ascii_case("wsl$")
            },_=>false,
        },_=>false,
    };
    for path in [root.to_path_buf(),dot(root),dot(root).join("poke")] {
        if wsl(&path) {return Err("service wake unsupported on Windows WSL UNC; use a native Windows checkout".into());}
        match path.canonicalize() {
            Ok(actual) if wsl(&actual)=>return Err("service wake unsupported on Windows WSL UNC; use a native Windows checkout".into()),
            Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(format!("service wake filesystem unknown: {e}")),
        }
    }
    Ok(())
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
/// only the owner or PM parks without either. `staged` is the current UTC time of a `--stage` park.
fn park<'a>(n: u64, blocker: Option<&'a str>, until: Option<&'a str>, staged: Option<&'a str>, task: Option<String>, attempt: Option<String>) -> Res<Vec<&'a str>> {
    let worker = attempt.is_some() && task.is_some_and(|t| t.parse() == Ok(n));
    match (blocker, until) {
        (Some(_), Some(_)) => return Err("--blocker and --until exclude each other: a park waits for one of them".into()),
        (None, Some(u)) if !core::utc(u) => return Err(format!("bad --until {u}: one UTC time like 2026-11-05T09:00:00Z")),
        (None, Some(u)) => return Ok(vec!["until", u]),
        _ => {}
    }
    // § 2 Stages: a worker stage park alone wakes at once, so the next stage starts without a human step (#2289)
    match blocker {
        None if worker && let Some(now) = staged => Ok(vec!["until", now]),
        None if worker => Err("a worker park needs --blocker #M (file the blocker as a held task first) or, when only time is left, --until <UTC> (§ 7)".into()),
        None => Ok(vec![]),
        Some(_) if !worker => Err("--blocker is for a worker park; the owner or PM adds `Depends on #M` or parks with later".into()),
        Some(r) if !core::blocker_ref(r) || r == format!("#{n}") => Err(format!("bad --blocker {r}: one #M or <repo>#M, not this task")),
        Some(r) => Ok(vec![r]),
    }
}

/// § 7 rule 3 (#2288): the local `#M` of a worker park when it is an open held task, which the park starts.
fn held_blocker(b: &dyn Board, r: &str) -> Option<u64> {
    let m = r.strip_prefix('#')?.parse().ok()?;
    b.issue(m).is_ok_and(|i| i.open && service::is_held(&i)).then_some(m)
}

/// § 2 Stages: `text` with the `Stages:` line of `later --stage`; each stage is an Acceptance item of `body`.
fn stage_text(text: &str, staged: &[String], body: &str) -> Res<String> {
    let ids = core::acceptance(body);
    if let Some(bad) = staged.iter().find(|s| !ids.contains(s)) {
        return Err(format!("bad --stage {bad}: one Acceptance item of this task ({})", if ids.is_empty() { "none".into() } else { ids.join(" ") }));
    }
    let line = core::stages_line(&format!("Stages: {}", staged.join(" "))).unwrap_or_default().join(" ");
    Ok(if text.trim().is_empty() { format!("Stages: {line}") } else { format!("{}\nStages: {line}", text.trim_end()) })
}

fn task_text(cmd: &str, text: &str) -> Res<()> {
    if matches!(cmd, "answer" | "steer") {
        if text.trim().is_empty() { return Err(format!("{cmd}: empty text; type a reply, then send again")); }
    } else if matches!(cmd, "requeue" | "later" | "failed") {
        if text.is_empty() && cmd != "failed" { return Ok(()); }
        let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
        all([limit(cmd, &lines, 1, 2, 240), fields(text, &[("Reason", 1, 1, 240), ("Next", 0, 1, 240)]).map(drop)])?;
    }
    Ok(())
}

/// § 5: every human reply reaches the board through this Unicode-safe wrapper, without a line-count cap.
fn wrap_reply(text: &str) -> String {
    text.split('\n').map(|line| {
        let mut rest = line;
        let mut lines = Vec::new();
        while let Some((end, _)) = rest.char_indices().nth(snapshot::ANSWER_CHARS) {
            let split = rest[..end].char_indices().rev().find(|(i, c)| *i > 0 && c.is_whitespace());
            let (at, skip) = split.map_or((end, 0), |(i, c)| (i, c.len_utf8()));
            lines.push(&rest[..at]);
            rest = &rest[at + skip..];
        }
        lines.push(rest);
        lines.join("\n")
    }).collect::<Vec<_>>().join("\n")
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

/// § 2 visual evidence: `--image` files, at most four images or videos (#1844) of at most 10 MB each.
fn image_files(paths: &[String]) -> Res<()> {
    if paths.len() > 4 { return Err("Evidence: at most four --image".into()); }
    all(paths.iter().map(|p| {
        let head = std::fs::File::open(p).and_then(|f| { let mut b = vec![]; std::io::Read::read_to_end(&mut std::io::Read::take(f, 16), &mut b).map(|_| b) }).map_err(|e| format!("--image {p}: {e}"))?;
        let video = board::video_kind(&head);
        if board::image_kind(&head).is_none() && video.is_none() { return Err(format!("--image {p}: not a PNG, JPEG, GIF, WebP, MP4 or WebM file")); }
        // boards pick the player by the extension
        if let Some(k) = video.filter(|k| !p.to_ascii_lowercase().ends_with(&format!(".{k}"))) {
            return Err(format!("--image {p}: a {} video's file name ends in .{k}", k.to_uppercase()));
        }
        let size = std::fs::metadata(p).map_err(|e| format!("--image {p}: {e}"))?.len();
        if size > 10 << 20 { return Err(format!("--image {p}: {size} bytes, at most 10 MB")); }
        Ok(())
    }))
}

/// § 2 visual evidence: uploads each `--image` to the board and embeds it as `![name](absolute URL)` only after the board
/// serves the same bytes back (the load check); a failed check publishes nothing. A video the board does not play
/// inline (GitHub) is the link `[▶ name](URL)` that opens it playable (#1844).
fn upload_images(b: &dyn board::Board, n: u64, paths: &[String]) -> Res<Vec<String>> {
    image_files(paths)?;
    paths.iter().map(|p| {
        let url = b.upload_file(n, Path::new(p))?;
        let sent = std::fs::read(p).map_err(|e| format!("--image {p}: {e}"))?;
        let back = b.file(&url).map_err(|e| format!("image load check failed for {url}: {e}"))?;
        if back != sent { return Err(format!("image load check failed: {url} serves {} bytes, {} uploaded", back.len(), sent.len())); }
        let video = board::video_kind(&sent).is_some();
        let name: String = Path::new(p).file_stem().map(|s| s.to_string_lossy().replace([']', '[', ',', ')'], " ")).unwrap_or_default().chars().take(30).collect();
        let name = if !name.trim().is_empty() { name } else if video { "video".into() } else { "image".into() };
        Ok(if video && !b.video_inline() { format!("[▶ {name}]({url})") } else { format!("![{name}]({url})") })
    }).collect()
}

/// § 2 evidence files (#2019): the kinds `result --file` and `ask --file` take, by name ending and leading bytes.
const FILE_KINDS: [(&str, &[&[u8]]); 6] = [(".zip", &[b"PK\x03\x04", b"PK\x05\x06"]), (".tar.gz", &[b"\x1f\x8b"]), (".tgz", &[b"\x1f\x8b"]),
    (".pdf", &[b"%PDF-"]), (".html", &[b"<"]), (".htm", &[b"<"])];

/// § 2 evidence files (#2019): each `--file` of a result or ask is a zip, tar.gz, PDF or HTML file of at most 25 MB,
/// checked before any write; returns the Evidence label of each, `⬇ <name>` of at most 32 characters. `zip_html`
/// (a board that shows HTML as source) labels an HTML file `⬇ <name>.html.zip`, the zip it uploads instead.
fn attachment_files(paths: &[String], zip_html: bool) -> Res<Vec<String>> {
    paths.iter().map(|p| {
        let lower = p.to_ascii_lowercase();
        let (ext, heads) = FILE_KINDS.iter().find(|(e, _)| lower.ends_with(e)).ok_or(format!("--file {p}: not a .zip, .tar.gz, .tgz, .pdf, .html or .htm file"))?;
        let size = std::fs::metadata(p).map_err(|e| format!("--file {p}: {e}"))?.len();
        if size > 25 << 20 { return Err(format!("--file {p}: {size} bytes, at most 25 MB")); }
        let head = std::fs::File::open(p).and_then(|f| { let mut b = vec![]; std::io::Read::read_to_end(&mut std::io::Read::take(f, 1024), &mut b).map(|_| b) }).map_err(|e| format!("--file {p}: {e}"))?;
        // HTML may start with a byte order mark and blank lines before its first tag
        let html = *ext == ".html" || *ext == ".htm";
        let lead = if html { head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&head).trim_ascii_start() } else { &head[..] };
        if !heads.iter().any(|h| lead.starts_with(h)) { return Err(format!("--file {p}: content is not a {} file", &ext[1..])); }
        let name = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let zip = if html && zip_html { ".zip" } else { "" };
        let stem: String = name[..name.len() - ext.len()].replace(['[', ']', ',', '(', ')'], " ").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(30 - ext.len() - zip.len()).collect();
        Ok(format!("⬇ {}{}{zip}", if stem.trim().is_empty() { "file" } else { stem.trim() }, &name[name.len() - ext.len()..]))
    }).collect()
}

/// § 2 evidence files (#2019): uploads each `--file` to the board's file store and links it as `[⬇ name](absolute URL)`
/// in the Evidence line only after the board serves the same bytes back (the load check); a failed check publishes
/// nothing. On a board that shows HTML as source, an HTML file goes up inside `<name>.zip`, so one click downloads it.
fn upload_attachments(b: &dyn board::Board, n: u64, paths: &[String]) -> Res<Vec<String>> {
    let zip_html = !b.html_downloads();
    paths.iter().zip(attachment_files(paths, zip_html)?).map(|(p, label)| {
        let mut sent = std::fs::read(p).map_err(|e| format!("--file {p}: {e}"))?;
        let html = [".html", ".htm"].iter().any(|e| p.to_ascii_lowercase().ends_with(e));
        let url = if zip_html && html {
            let name = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            sent = zip_one(&name, &sent)?;
            let dir = std::env::temp_dir().join(format!("taskq-file-{}", runtime::uuid()));
            let zip = dir.join(format!("{name}.zip"));
            let up = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&zip, &sent)).map_err(|e| format!("--file {p}: {e}"))
                .and_then(|_| b.upload_attachment(n, &zip));
            let _ = std::fs::remove_dir_all(&dir);
            up?
        } else { b.upload_attachment(n, Path::new(p))? };
        let back = b.file(&url).map_err(|e| format!("file load check failed for {url}: {e}"))?;
        if back != sent { return Err(format!("file load check failed: {url} serves {} bytes, {} uploaded", back.len(), sent.len())); }
        Ok(format!("[{label}]({url})"))
    }).collect()
}

/// A zip archive holding the one file `name` with `data`, deflated, for a board that would show it as text (#2019).
fn zip_one(name: &str, data: &[u8]) -> Res<Vec<u8>> {
    use std::io::Write;
    let mut z = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
    let packed = z.write_all(data).and_then(|_| z.finish()).map_err(|e| format!("zip {name}: {e}"))?;
    let mut crc = flate2::Crc::new();
    crc.update(data);
    let sizes = [packed.len() as u32, data.len() as u32];
    // version 2.0, UTF-8 name flag, deflate, 1980-01-01 00:00; sizes fit u32 under the 25 MB cap
    let entry = |sig: u32| [&sig.to_le_bytes()[..], &20u16.to_le_bytes(), &0x0800u16.to_le_bytes(), &8u16.to_le_bytes(), &0u16.to_le_bytes(),
        &33u16.to_le_bytes(), &crc.sum().to_le_bytes(), &sizes[0].to_le_bytes(), &sizes[1].to_le_bytes(),
        &(name.len() as u16).to_le_bytes(), &0u16.to_le_bytes()].concat();
    let local = [entry(0x0403_4b50), name.as_bytes().to_vec(), packed].concat();
    // the central entry adds "made by" before the shared fields, then comment, disk, attributes and the local offset
    let shared = entry(0x0201_4b50);
    let central = [&shared[..4], &20u16.to_le_bytes(), &shared[4..], &[0u8; 14], name.as_bytes()].concat();
    let end = [&0x0605_4b50u32.to_le_bytes()[..], &[0u8; 4], &1u16.to_le_bytes(), &1u16.to_le_bytes(),
        &(central.len() as u32).to_le_bytes(), &(local.len() as u32).to_le_bytes(), &0u16.to_le_bytes()].concat();
    Ok([local, central, end].concat())
}

/// The Evidence links `--file` will add, with a placeholder URL: the card checks them before any upload.
fn file_stubs(b: &dyn board::Board, paths: &[String]) -> Res<Vec<String>> {
    Ok(attachment_files(paths, !b.html_downloads())?.into_iter().map(|l| format!("[{l}](https://board.invalid/file)")).collect())
}

/// § 2 evidence files (#2019): an uploaded `--file` link, `[⬇ name](url)`.
fn attachment(l: &str) -> bool {
    l.starts_with("[⬇ ")
}

/// § 2 visual evidence: where the name starts in an uploaded image or video, `![name](url)` or `[▶ name](url)`; none
/// for a link.
fn media(l: &str) -> Option<usize> {
    ["![", "[▶ "].into_iter().find(|p| l.starts_with(p)).map(str::len)
}

/// § 2 visual evidence (#1844): media reaches a task only through `--image`. A hand-written image or video link that is
/// relative, host-less (csgo#415: `/-/project/39/uploads/..`) or a repository path renders as a missing file.
fn media_links(values: &[&str]) -> Res<()> {
    const MEDIA: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "mp4", "webm", "mov", "m4v"];
    let bare = values.iter().flat_map(|v| v.split_whitespace()).map(|w| {
        let w = w.rsplit_once("src=").or_else(|| w.rsplit_once("href=")).map_or(w, |x| x.1);
        w.trim_matches(|c: char| "\"'<>()[],;.".contains(c))
    }).filter(|w| w.starts_with('/') || w.starts_with("http://") || w.starts_with("https://"));
    let marked = values.iter().flat_map(|v| v.split("](").skip(1)).filter_map(|r| r.split([')', ' ']).next());
    for link in marked.chain(bare) {
        let path = link.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
        if !path.rsplit_once('.').is_some_and(|(_, e)| MEDIA.contains(&e)) { continue; }
        let web = path.strip_prefix("https://").or_else(|| path.strip_prefix("http://"));
        // `/raw/<sha>/` is a commit-pinned taskq evidence file; `/raw/<branch>/` moves with the branch
        let pinned = |r: &str| r.split('/').next().is_some_and(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()));
        let repo = web.is_some_and(|w| w.starts_with("raw.githubusercontent.com/") || ["/blob/", "/-/blob/", "/-/raw/", "/-/tree/", "/tree/"].iter().any(|m| w.contains(m))
            || w.split_once("/raw/").is_some_and(|(_, r)| !pinned(r)));
        let why = if web.is_none() { "relative or host-less" } else if repo { "a repository path" } else { continue };
        return Err(format!("media link {link} is {why}; attach the file with --image (it uploads and embeds the absolute URL)"));
    }
    Ok(())
}

/// § 2 ask card: Done, Problem, optional Evidence, 2-4 options with one ★ or none (an owner action), the answer line.
fn ask_card(n: u64, text: &str, opts: &[String], recommend: Option<&str>, links: &[String]) -> Res<String> {
    card(n, text, opts, recommend, links, false)
}

/// § 2 ask card of a service decision (retry limit, blocked publication, closed task, refused start, failed input,
/// #2012): the writer of `ask`, so a format change reaches every card TaskQ posts. `done` and `problem` become one
/// plain line each, cut to the field limit; options keep their key (`later:`, `close:`), which runs only through
/// `taskq answer N.K` (§ 2). It never fails: a text the writer refuses is replaced by the refusal.
pub(crate) fn service_card(n: u64, done: &str, problem: &str, opts: &[String], recommend: usize) -> String {
    let text = |p: &str| format!("Done: {}\nProblem: {}", plain(done, 160), plain(p, 240));
    let k = recommend.to_string();
    card(n, &text(problem), opts, Some(&k), &[], true).unwrap_or_else(|e| {
        let opts = ["retry: retry the step - + runs it again; - may fail the same way".into(), "later: park the task - + nothing runs; - waits until the owner requeues it".into()];
        card(n, &text(&format!("card check failed: {e}")), &opts, Some("1"), &[], true).unwrap_or(e)
    })
}

/// One line of at most `max` characters for a service card field: no link (`](` breaks), each URL as `code`.
fn plain(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ").replace("](", "] (");
    let s = if s.is_empty() { "-".into() } else if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>()) } else { s };
    s.split(' ').map(|w| if w.starts_with("http://") || w.starts_with("https://") { format!("`{w}`") } else { w.into() }).collect::<Vec<_>>().join(" ")
}

/// The § 2 card writer of every card TaskQ posts. A `cli` card (a service decision) takes `taskq answer N.K` only:
/// its `close` and `later` options never run from a bare comment `K`.
fn card(n: u64, text: &str, opts: &[String], recommend: Option<&str>, links: &[String], cli: bool) -> Res<String> {
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
    if links.iter().filter(|l| media(l).is_none()).count() > 2 {
        return Err("Evidence: at most two --link and --file together".into());
    }
    let ev: Vec<String> = links.iter().filter(|l| media(l).is_none()).map(|l| evidence(l)).collect();
    if ev.iter().any(|e| e.split_once("](").is_some_and(|(l, _)| l.chars().count() > 41)) {
        return Err("Evidence: link labels at most 40 characters".into());
    }
    // § 2: a link must open from the board and every app; a relative path renders as a broken repository link (#1782)
    if let Some(e) = ev.iter().find(|e| !e.split_once("](").is_some_and(|(_, u)| u.starts_with("https://") || u.starts_with("http://"))) {
        return Err(format!("Evidence: {e} needs an absolute http(s) URL"));
    }
    // visual choice (answer card § Question types): one image per option names image K `K. <name>`
    let images: Vec<(&String, usize)> = links.iter().filter_map(|l| Some((l, media(l)?))).collect();
    let each = images.len() >= 2 && images.len() == opts.len();
    let ev = [ev, images.iter().enumerate().map(|(i, (l, k))| if each { format!("{}{}. {}", &l[..*k], i + 1, &l[*k..]) } else { l.to_string() }).collect()].concat();
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
    lines.push(if cli {
        format!("To answer: `taskq answer {n}.K`; a bare comment `K` is a plain answer, never `close` or `later`.")
    } else {
        format!("To answer: `taskq answer {n}.K` or comment `K` or the option text.")
    });
    Ok(lines.join("\n"))
}

/// § 7 result: Changed, one `Checks:` line per check, Open; a decision appends the § 2 card (Done/Problem in `--text`).
fn result_text(n: u64, text: &str, checks: &[String], opts: &[String], recommend: Option<&str>, links: &[String]) -> Res<String> {
    let d = !opts.is_empty() as usize;
    if d == 0 && (recommend.is_some() || links.iter().any(|l| media(l).is_none() && !attachment(l))) {
        return Err("--recommend and --link need --option".into());
    }
    let f = fields(text, &[("Changed", 1, 2, 240), ("Open", 1, 2, 240), ("Done", d, d, 160), ("Problem", d, 2 * d, 240)])?;
    let checks: Vec<String> = checks.iter().flat_map(|c| c.lines()).map(str::trim).filter(|c| !c.is_empty()).map(|c| format!("Checks: {}", c.trim_start_matches("Checks: "))).collect();
    limit("Checks", &checks, 1, 8, 240)?;
    let verdict = [f[0].clone(), checks, f[1].clone()].concat();
    limit("result", &verdict, 3, 12, 240)?;
    if links.iter().filter(|l| media(l).is_none()).count() > 2 {
        return Err("Evidence: at most two --link and --file together".into());
    }
    let card = if d == 1 { ask_card(n, &[f[2].clone(), f[3].clone()].concat().join("\n"), opts, recommend, links)? } else if links.is_empty() { String::new() } else { format!("Evidence: {}", links.join(", ")) };
    Ok(join(&[&verdict.join("\n"), &card]))
}

/// § 7 close verdict: Changed, Checks naming every Acceptance item `A<k> ok|gap|unknown` and CI, Open.
/// § 7: deferred publication accepts merge-result CI; synchronous routes name the result SHA.
fn close_check(text: &str, issue: &board::Issue, t: &Task, deferred: bool) -> Res<()> {
    if t.state != State::Review || text.is_empty() {
        return Ok(());
    }
    close_verdict(text, &issue.body, reports::experiment(issue))?;
    if deferred && text.lines().any(|l| l.starts_with("Checks:") && l.contains("CI") && (l.contains("merge-result") || l.contains("merge_group"))) {
        return Ok(());
    }
    if t.result.as_deref().is_some_and(|sha| text.lines().any(|l| l.starts_with("Checks:") && l.contains(sha))) { Ok(()) } else { Err("Checks: name the exact result SHA with the CI outcome".into()) }
}

/// #1979: an experiment's close carries one `Verdict: confirmed|refuted|inconclusive <evidence>` line after Changed.
fn close_verdict(text: &str, body: &str, experiment: bool) -> Res<()> {
    let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
    limit("verdict", &lines, 3, 12, 240)?;
    let f = fields(text, &[("Changed", 1, 2, 240), ("Verdict", 0, 1, 240), ("Checks", 1, 8, 240), ("Open", 1, 2, 240)])?;
    limit("verdict", &f.concat(), 3, 12, 240)?;
    if (experiment || !f[1].is_empty()) && reports::outcome_verdict(&f[1].join("\n")).is_none() {
        return Err("Verdict: an experiment closes with `Verdict: confirmed|refuted|inconclusive <evidence>` after Changed".into());
    }
    let checks = f[2].join(" ");
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
    close_verdict("Changed: x\nChecks: A1 ok; CI green on the SHA\nOpen: none", "A1. y", false)?;
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

fn inbox(b: &dyn Board, root: &Path, n: u64) -> Res<String> {
    let issue = b.issue(n)?;
    let comments = b.comments(n)?;
    let log = board::reply::log(n, &comments)?;
    let projection = core::project(n, &log, deps_open(b, n, &issue.body, &log));
    let t = &projection.task;
    let mut out = vec![format!("#{n} {}\n{}", issue.title, b.url(n)), core::task_context(n, issue.body.trim(), &log), format!("Folded: {}", serde_json::to_string(&t).unwrap_or_default())];
    out.push(format!("Revision: {}", projection.revision));
    if let Some((k, why)) = &t.pick {
        // § 2: the worker's star and the PM's pick, side by side
        let star = core::options(t.card.as_deref().unwrap_or("")).into_iter().find(|o| o.2).map_or("none".into(), |o| format!("{n}.{}", o.0));
        out.push(format!("Recommendation: worker ★ {star} · PM {n}.{k}: {why}"));
    }
    out.push("Log:".into());
    let files=reply_files::prepare(b,root,n,&log)?;
    if !files.is_empty(){out.push(files);}
    for e in &log {
        out.push(format!("[{}{}] {}", e.id, if e.edited { " edited, ignored" } else { "" }, e.body.trim()));
    }
    read_receipts(b, n, &log, &comments);
    Ok(out.join("\n\n"))
}

/// § 2 read receipt: the live attempt's inbox read reacts `eyes` to each owner note it has now read.
fn read_receipts(b: &dyn Board, n: u64, log: &[Entry], comments: &[board::Comment]) {
    if let Some(a) = std::env::var("TASKQ_ATTEMPT").ok().and_then(|s| s.parse().ok()) { dispatch::mark_read(b, n, log, comments, a); }
}

/// § 5 `inbox N --json`: the folded task, its current attempt's progress and each accepted answer with its writer, board
/// time and question revision, from the same issue, comment and dependency reads as the human `inbox` (#1885).
fn inbox_json(b: &dyn Board, root: &Path, me: &str, n: u64) -> Res<Value> {
    let issue = b.issue(n)?;
    let comments = b.comments(n)?;
    let log = board::reply::log(n, &comments)?;
    let p = core::project(n, &log, deps_open(b, n, &issue.body, &log));
    let t = &p.task;
    let at = |id: u64| comments.iter().find(|c| c.id == id);
    let answers: Vec<Value> = p.answers.iter().map(|a| json!({"id": a.id, "by": at(a.id).map(|c| c.author.as_str()), "at": at(a.id).map(|c| c.created),
        "revision": a.revision, "option": a.option, "text": core::delivery(n, &log, a.id), "late": a.late, "read": at(a.id).and_then(|c| c.read)})).collect();
    read_receipts(b, n, &log, &comments);
    let attempt = t.attempt.map(|a| {
        let session = t.session.as_deref().unwrap_or_default();
        let (native, host) = session.rsplit_once(' ').unwrap_or((session, ""));
        let role = t.role.as_deref().unwrap_or_default();
        // R6 3: the attempt log's update time is known only on the attempt's own host
        let updated = (host == me).then(|| fs::metadata(runtime::attempt_log(root, role, n, a)).and_then(|m| m.modified()).ok()).flatten()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
        json!({"id": a, "role": role, "runtime": native.split(':').next(), "host": host, "started": at(a).map(|c| c.created),
            "stage": t.state.name(), "stages": t.stages, "pending_notes": t.inbox.len(), "delivered": p.delivered(a), "log_updated": updated})
    });
    let last = log.iter().max_by_key(|e| e.id).map(|e| json!({"id": e.id, "at": at(e.id).map(|c| c.created)}));
    Ok(json!({"schema": 1, "task": n, "title": issue.title, "url": b.url(n), "open": issue.open, "state": t.state.name(), "revision": p.revision,
        "source_readiness": readiness::source(n,&b.url(n),&log), "card": t.card, "result": t.result, "attempt": attempt, "last": last, "answers": answers}))
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
    let comments=b.comments(i.number);
    let log=comments.as_ref().map_err(Clone::clone).and_then(|comments|board::reply::log(i.number,comments));
    // § 1: GitHub records why an issue closed; GitLab records no reason, so a closed GitLab issue is done
    let outcome = if !log.as_ref().is_ok_and(|log|fold(i.number,log,false).state!=State::Doing){"unknown"}else{match (i.reason.as_deref(), gitlab) { (Some("completed"), _) | (None, true) => "success", (Some("not_planned"), _) => "not_planned", _ => "unknown" }};
    let (result, pr) = log.as_ref().map_or((Value::Null, Value::Null), |log| {
        let t = fold(i.number, log, false);
        let pr = log.iter().flat_map(|e| e.body.split_whitespace()).filter(|w| w.starts_with("https://") && (w.contains("/pull/") || w.contains("/merge_requests/"))).last();
        (json!(t.publication.as_deref().and_then(|p| p.lines().next()).map(|l| l.trim_start_matches("Changed:").trim())), json!(pr.map(|u| u.trim_end_matches(|c: char| !c.is_ascii_alphanumeric()))))
    });
    let conversation=log.as_ref().ok().and_then(|log|core::conversation(i.number,log));
    let history=log.as_ref().ok().zip(comments.as_ref().ok()).map(|(log,comments)| {
        dispatch::timeline(i.number,log,&core::project(i.number,log,false),comments).iter().map(|(q,a)|json!({"question":q.as_deref().map(snapshot::question),"answered":a})).collect::<Vec<_>>()
    });
    let workers=log.as_ref().ok().map(|log|core::project(i.number,log,false).changes.into_iter().filter(|(id,_,t)|t.role.as_deref()==Some("worker")&&t.attempt==Some(*id)).map(|(id,_,t)|json!({"attempt":id,"role":"worker","session":t.session})).collect::<Vec<_>>());
    let timing=log.as_ref().ok().zip(comments.as_ref().ok()).map(|(log,comments)|snapshot::task_timing(&core::project(i.number,log,false),comments,!i.open,closed));
    let merged = log.as_ref().ok().and_then(|log| merge_receipt(log));
    // § 7 close outcome (#1800): delivered, no change or replaced by #M with its current state, plus unverified items
    let verdict = log.as_ref().ok().and_then(|log| log.iter().rev().find(|e| !e.edited && e.body.starts_with("taskq: close")).map(|e| core::verdict(i.number, log, e.body.split_once('\n').map_or("", |x| x.1))));
    let verdict = verdict.as_deref();
    let next = verdict.and_then(display::successor).map(|m| dispatch::current(b, m).map_or("unknown", |(i, t, _)| if i.open { t.state.name() } else { "closed" }));
    let close = log.is_ok().then(|| display::close_outcome(verdict, next));
    json!({"n": i.number,"revision":log.as_ref().ok().map(|log|core::revision(i.number,log)), "title": i.title, "url": b.url(i.number), "state": if i.open { "open" } else { "closed" }, "closed": closed, "outcome": outcome, "close": close, "result": result, "pr": pr, "merged": merged, "source_readiness": log.as_ref().map_or_else(|_|readiness::unavailable(i.number,&b.url(i.number),"task log unavailable"),|log|readiness::source(i.number,&b.url(i.number),log)), "timing":timing,"history":history,"workers":workers,"brief":i.body,"conversation": conversation.map(|id| json!({"id":id,"brief":i.body,"verdict":verdict})), "error": log.as_ref().err()})
}

fn closed_pin(b: &dyn Board, n: u64, gitlab: bool) -> Res<Value> {
    let i = b.issue(n)?;
    if i.number != n || !service::is_task(&i) { return Err("Pinned issue is not the requested trusted task".into()); }
    Ok(json!({"schema":1,"source":"board","more":false,"tasks":[closed_row(b,&i,i.closed,gitlab)]}))
}

/// The native session of `n`'s closed worker on this host: the board holds a job id for a remote-control launch, the
/// attempt log the session, as a live read resolves it.
fn closed_session(root: &Path, runtime: &str, id: &str, n: u64, attempt: u64) -> Res<String> {
    let session = runtime::session_id(root,runtime,id,n,attempt,"worker");
    if session=="-" || !runtime::plain_thread(&session) { return Err("Worker native session unknown".into()); }
    Ok(session)
}

/// A closed worker's page: from its transcript on this host, else through `remote` (the host link) on its host.
fn closed_transcript(b: &dyn Board, root: &Path, n: u64, host: &str, before: Option<u64>, remote: impl FnOnce(&str,u64)->Res<Value>) -> Res<Value> {
    let value=reports::selected(root,b,n,false,None)?;
    let (runtime,id,owner,attempt) = reports::worker(&value,None)?;
    if owner != host { return remote(&owner,attempt); }
    let session = closed_session(root,&runtime,&id,n,attempt)?;
    let path = runtime::owner::transcript(&runtime,&session)?;
    Ok(transcript::before(transcript::format(&runtime),&session,&path,before)?.json())
}

/// Host link principle 3: the peer finds a closed worker's session on the board and its own attempt log, never in the request.
pub(crate) fn closed_peer_session(root: &Path, local: &str, n: u64, attempt: u64) -> Res<String> {
    let (cfg, raw) = load(root)?;
    let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
    let value=reports::selected(root,&*board,n,!cfg.board.closed_reason,None)?;
    let (runtime,id,owner,last) = reports::worker(&value,Some(attempt))?;
    if owner != local || last != attempt { return Err("Worker transcript is available only on its host".into()); }
    Ok(format!("{runtime}:{}",closed_session(root,&runtime,&id,n,attempt)?))
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

/// A tray window touches `<install>/menu-open` every tick while visible and focused.
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

/// § 4 step 8 (#1431, #2352): an idle pass waits 3×`poll`; board-only changes may show up to 2×`poll` later.
fn poll_wait(poll: u64, complete: bool, children: bool, poked: bool, focus_off: bool, snapshot: Option<&snapshot::Snapshot>) -> u64 {
    let active = snapshot.is_none_or(|s| s.reason.is_some() || !focus_off && s.tasks.iter().any(|r| matches!(r.task.state, State::Ready | State::Doing | State::Review)));
    if !complete || children || poked || active { poll } else { poll * 3 }
}

/// Read-only recovery evidence; failed identity reads and a still-live foreign owner prevent takeover.
pub(crate) fn kids_path(install:&Path,root:&Path)->Res<PathBuf> {
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
        // #2266: one row that cannot be adopted keeps its record and board reservation; the project loop runs on.
        if let Err(e)=adopt_kid(s,install,root,k.clone()) {
            platform::log(&format!("#{} execution {}: {e}; native recovery record kept, execution stays reserved",k.task,k.execution));
        }
    }
    Ok(())
}

fn adopt_kid(s: &mut Service, install:&Path, root: &Path, k:data::KidLine) -> Res<()> {
    if let Some(owned)=s.kids.iter().find(|owned|owned.task==k.task||owned.execution==k.execution) {
        if owned.task==k.task&&owned.attempt==k.attempt&&owned.execution==k.execution&&owned.rt==k.runtime
            &&owned.proc.native_identity()?==k.process {return Ok(());}
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
            return Ok(());
        },
        Some(core::Release::NoStart)=>return Err("board no-start contradicts a recorded native process; file kept".into()),
        None=>{},
    }
    let proc=runtime::adopt(root,install,&k,&execution)?;
    s.adopt(Kid { unknown:None, stopping:false, session_url:None, execution:k.execution,task:k.task,attempt:k.attempt,rt:k.runtime,proc })?;
    Ok(())
}

// §4: only a confirmed logout holds admission; an unknown probe keeps its existing meaning.
fn login_holds(rows: &Value) -> std::collections::BTreeSet<String> {
    rows.as_array().into_iter().flatten().filter(|r| r["login"] == "not logged in")
        .filter_map(|r| r["runtime"].as_str().map(String::from)).collect()
}

fn service_run(c: &Ctx, host: Option<host::Shared>) -> Res<()> {
    let cfg = &c.cfg;
    let exe = launcher(&cfg.install);
    let board_env = cfg.board.worker_env(&platform::tool(&cfg.install), &cfg.web_host)?;
    let env = runtime::Env { context: "", board_env: &board_env, root: &cfg.root, exe: &exe, install: &cfg.install, permission_mode: &cfg.permission_mode, codex: &cfg.codex, publish: &cfg.delivery, jobs: None };
    let start = |l: &runtime::Launch| {
        let issue = c.board.issue(l.task).map_err(runtime::StartError::admission)?;
        let log = board::entries(c.board.as_ref(), l.task).map_err(runtime::StartError::admission)?;
        let context = core::task_context(l.task, &issue.body, &log);
        let env = runtime::Env { context: &context, ..env };
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
    let mut incoming = incoming::Reader::new(cfg.clone(), c.raw["label_schema"].as_str().map(str::to_string), &c.board.url(0));
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
    let mut pr_route: Option<(Instant, check::Probe)> = None;
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
    let mut reports = reports::Collector::default();
    let mut link = link::Server::default();
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
        // § 3: capacity reloads before each admission decision; children are untouched; § 7 delivery follows too (#1942)
        let cap = s.reread_project()
            .and_then(|v| {
                s.cfg.routing = labels::routing(&v)?;
                match &host {
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
            }});
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
            s.grants = platform::grants(&cfg.install);
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
        // § 4: runtime probes at most every 5 minutes
        if !quiet && agents.as_ref().is_none_or(|(t, _)| t.elapsed().as_secs() >= 300) {
            mark_checking(&platform::tool(&cfg.install), &cfg.root, s.wall as u64);
            agents = Some((Instant::now(), json!({"at": s.wall, "rows": runtime::agents(&cfg.install)})));
            // #1429: a logged-out runtime admits nothing until a probe reads it logged in
            s.logged_out = agents.as_ref().map(|a| login_holds(&a.1["rows"])).unwrap_or_default();
            s.clis = runtime::clis_versions(&agents.as_ref().map_or(Value::Null, |a| a.1["rows"].clone()));
        }
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
            requests::flush(requests::SERVICE);
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
        // docs/design/enrollment.md: delivery runs on its own thread; its failure never fails or delays the pass
        let local = enrollment::Local { host: cfg.me.clone(), service: Some(s.instance.clone()), ttl: cfg.ttl.max(0) as u64, agents: agents.as_ref().map_or(Value::Null, |a| a.1.clone()),
            progress: s.snapshot.as_ref().map(|x| enrollment::progress(x, &cfg.root, &cfg.me)).unwrap_or_default() };
        if !quiet { if let Some(l) = dashq.tick(&cfg.install, s.snapshot.as_ref(), &local, at) { log!("{l}"); } }
        // docs/design/standalone-telemetry.md#remote-sync-1353: only under the telemetry grant, on its own thread
        if !quiet { if let Some(l) = telemetry_sync.tick(&cfg.install, s.snapshot.as_ref(), at) { log!("{l}"); } }
        // docs/design/shared-resource-controls.md § Host adapter: the same speaker exchanges resource operations
        if !quiet { for l in policy_exchange.tick(&cfg.install, s.snapshot.as_ref(), at) { log!("{l}"); } }
        if !quiet { for l in pm_exchange.tick(&cfg.install, s.snapshot.as_ref(), at) { log!("{l}"); } }
        // docs/design/host-link.md: the same speaker listens on the host link and publishes its key
        if !quiet { s.link = link.tick(&cfg.install, s.snapshot.as_ref()).to_string(); }
        // § 8 item 1: only the Mac thread of the taskq checkout on channel dev, on request (#2311); merge_group CI read at most every 5 minutes per SHA
        let mut dev = Value::Null;
        #[cfg(target_os = "macos")]
        if !quiet && is_installed && platform::dev_enabled(&cfg.repo, &cfg.install) {
            // #2321: a ready task whose dependency closed after the installed commit requests the build itself
            platform::dev_dep_request(&cfg.install, &cfg.root, s.dep_closed);
            dev =platform::dev_tick(&cfg.install, &cfg.root, &mut |sha| {
                if dev_ci.as_ref().is_some_and(|(s, t)| s == sha && t.elapsed().as_secs() < 300) { return Err("checked".into()); }
                dev_ci = Some((sha.into(), Instant::now()));
                c.board.merge_ci_green(sha)
            });
        }
        // § 6 setup check: local items each pass, the board's PR route at most hourly
        if !quiet && s.cfg.publish == "pr" && pr_route.as_ref().is_none_or(|(t, _)| t.elapsed().as_secs() >= 3600) {
            pr_route = Some((Instant::now(), c.board.pr_route()));
        }
        let limits = s.cap.as_ref().map_or_else(|_| s.cfg.limits.clone(), |cap| cap.limits.clone());
        let open = s.snapshot.as_ref().map(|snap| snap.tasks.iter().map(|r| (r.n, r.labels.clone())).collect());
        let setup_items = setup_items(&s.cfg, &limits, Ok(s.cfg.delivery.clone()), &agents.as_ref().map_or(Value::Null, |a| a.1["rows"].clone()), &access,
            open, pr_route.as_ref().map(|r| r.1.clone()));
        let mut setup = check::json(&project, &setup_items);
        setup["checked"] = json!(at);
        let tool = platform::tool(&cfg.install);
        let error = r.as_ref().err().cloned();
        // #1845 A1: every TCC grant a gui worker needs, Screen Recording from the gui probe
        let mut grants = s.grants.clone();
        if !grants.is_empty() {
            let screen = if s.gui_stale { "stale" } else if s.gui_probe.as_ref().err().is_some_and(|e| e == platform::NOT_ALLOWED) { "missing" } else { "allowed" };
            grants.insert("screen_recording".into(), screen.into());
        }
        let mut status = coreq::status::common(&tool, &cfg.me, &project, at, r.err(), update.as_deref());
        // § 4 snapshot keys; before the first successful pass tasks are unknown, not an empty list
        match &s.snapshot {
            Some(snap) => {
                for (k, v) in snap.status_keys(error.as_deref()) { status[k] = v; }
                // R6: the tray and PilotQ show the manager view's own rows
                if let Ok(t) = snap.text(&s.cfg, &Value::Null) { status["report"] = json!(t); }
            }
            None => { status["complete"] = json!(false); status["reason"] = json!(error.as_deref().unwrap_or("no board read yet")); status["tasks"] = json!([]); }
        }
        for (k, v) in [("focus", if focus_error.is_some() { Value::Null } else { json!(s.focus) }), ("focus_error", json!(focus_error)), ("focus_command",command), ("kids", json!(kids)), ("issues", json!(issues)), ("log", json!(log)),
            ("tasks_age_seconds", json!(s.tasks_read.map(|t| t.elapsed().as_secs()))), ("request", json!(request)), ("events_since", json!(s.events_since)), ("incoming", incoming.tick()),
            ("gui", json!({"listed": s.gui().is_ok(), "reason": s.gui().err(), "stale_grant": s.gui_stale, "grants": grants, "binary": cfg.install.join("taskq")})), ("waits", json!(s.waits.iter().map(|(n, reason)| json!({"task": n, "reason": reason})).collect::<Vec<_>>())),
            ("execution_waits", json!(s.execution_waits)), ("capacity", s.capacity_json()), ("hold", json!(s.hold)), ("access", json!(access.iter().map(|e| e.json()).collect::<Vec<_>>())), ("agents", json!(agents.as_ref().map(|a| &a.1))),
            ("stop_progress",json!(s.stop_progress)), ("poll", json!(cfg.poll)),
            ("restart_overdue", json!(host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.restart_overdue)))),
            ("restart_waits", json!((d.join("restart").exists()||host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.restart))).then(|| restart_waits(&s, host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.stopping)))))),
            ("incident", incident::status(&cfg.root)), ("incident_notice", json!(incident::notice(&cfg.root))),
            ("requests", requests_json), ("channel", json!(platform::channel(&cfg.install).ok())), ("dev", dev), ("connection", json!(dashq.outcome)), ("telemetry_sync", telemetry_sync.status.clone()), ("telemetry",telemetry::diagnostic::health()),
            ("tidy", fs::read_to_string(d.join("tidy.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)), ("setup", setup)]
        {
            status[k] = v;
        }
        status["diagnostics"] = s.service_diagnostics();
        status["binary_evidence"] = s.binary_evidence();
        status["reports"] = reports::summary(&reports.tick(&cfg.root,cfg.poll,cfg.ttl,!quiet));
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
                    Ok(Some(v)) => {
                        update = None;
                        // § 9 deploy: the release flow records the applied version before the re-exec
                        if let Ok(exe) = std::env::current_exe() {
                            for l in deploy::applied(&exe, std::slice::from_ref(&cfg.root), platform::channel(&cfg.install).unwrap_or("live"), platform::VERSION, &v, &hostname()) { log!("{l}"); }
                        }
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
        let wait = if reduced {1} else {poll_wait(cfg.poll, error.is_none(), !s.idle(), poked, s.focus_off(), s.snapshot.as_ref())};
        // every 30 s, also between board polls: a gate sample (when enabled) and a fresh capacity in status.json (§ 4)
        let mut tick = || {
            let results_ready=reports.received();
            if results_ready {
                status["reports"]=reports::summary(&reports.tick(&cfg.root,cfg.poll,cfg.ttl,false));
                status["tasks_age_seconds"]=json!(s.tasks_read.map(|t|t.elapsed().as_secs()));
                status["at"]=json!(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0,|d|d.as_secs()));
            }
            if sampled.is_some_and(|t| t.elapsed().as_secs() < 30) {
                if results_ready {publish_status(&tool,&cfg.root,&status,host.as_ref());}
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
    let complete=snapshot::complete(&status)&&status["error"].is_null()&&status["focus_error"].is_null();
    if let Some(host)=host {
        status["host_instance"]=host.lock().map(|h|json!(h.instance)).unwrap_or(Value::Null);
        status["host_install"]=if complete {status["host_instance"].as_str().and_then(|instance|host::install_acknowledgement(&tool.install,root,instance)).map_or(Value::Null,|id|json!(id))}else{Value::Null};
    }
    let written = platform::write_status(tool, root, &status);
    if let Err(e) = &written { log!("status publication failed: {e}; observed state unknown"); }
    let qualified = written.is_ok() && complete;
    if let Some(host) = host { if let Ok(mut h) = host.lock() { h.publication(root, qualified); } }
    qualified
}

fn write_kids(install:&Path,root:&Path,kids:&[data::KidLine])->Res<()> {
    let owner=coreq::process::identity(std::process::id()).map_err(|e|e.to_string())?.ok_or("service native identity missing")?;
    let mut text=format!("pid {} {} executor-v1\n",owner.pid,owner.started);
    for k in kids {
        text+=&format!("{} {} {} {} {} {} {}{}\n",k.task,k.attempt,k.runtime,k.process.pid,k.process.started,k.execution,k.offset,k.job.as_ref().map(|scope|format!(" {scope}")).unwrap_or_default());
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
        // #2297: a child without native identity leaves no record; the next image rebuilds it from the board (§ 3 unknown)
        let Ok(process)=k.proc.native_identity() else {continue};
        let line=data::KidLine {task:k.task,attempt:k.attempt,runtime:k.rt.clone(),process,execution:k.execution,offset:k.proc.input_offset()?,job:k.proc.job()};
        // #1561: a recorded process that exited resolves from its board exit; any other record passes to the next image (#2297)
        if recorded(&line).0=="ended" && exited(k.task,k.attempt,k.execution,&k.rt)? {continue}
        kids.push(line);
    }
    let mut kept=vec![];
    for old in kids_file(&s.cfg.install,root)? {
        if kids.contains(&old) { continue; }
        // #1435, #2297: an unresolved record passes to the next image instead of holding the update on it
        if !exited(old.task,old.attempt,old.execution,&old.runtime)? { kept.push(old); }
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
    /// host.json task-claim mode, § 9
    claim: Option<host::ClaimMode>,
    /// host.json `default_runtime`, § 9
    default_runtime: Option<String>,
    weights: Option<balance::Weights>,
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
        e.claim = a.one("claim").map(host::ClaimMode::parse).transpose()?;
        e.default_runtime = match a.one("default-runtime") {
            None => None,
            Some(rt) if host::DEFAULT_RUNTIMES.contains(&rt) => Some(rt.into()),
            Some(x) => return Err(format!("--default-runtime: {}, not {x}", host::DEFAULT_RUNTIMES.join(" or "))),
        };
        Ok(e)
    }

    fn is_empty(&self) -> bool {
        self.mode.is_none() && self.max.is_none() && self.limits.is_empty() && self.unset.is_empty() && self.gate.is_none() && self.cpu_max.is_none() && self.memory_min.is_none() && self.claim.is_none()
            && self.default_runtime.is_none() && self.weights.is_none()
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
        return Err("nothing to change: give --mode, --max, --claude, --codex, --antigravity, --codex-app, --load-gate, --cpu-max or --memory-min".into());
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
    given.map(String::from).or_else(|| (cfg.claim == host::ClaimMode::Labelled).then(|| cfg.me.clone()))
}

/// § 9: a labelled host keeps its own host on every board; a board this host does not serve refuses the add,
/// and the refusal names the fix (#1912).
fn serve_hint(e: String, cfg: &Config) -> String {
    if e.starts_with(&format!("--host {0}: no host named {0};", cfg.me)) {
        format!("{e}\nNext: run taskq service install for this project on host {} so it serves this board", cfg.me)
    } else { e }
}

/// host.json `claim` value (§ 9).
fn claim_name(mode: host::ClaimMode) -> &'static str { mode.name() }

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

/// Local owner runtime weights (§4 Balancing).
fn balance_cli(a:&Args) -> Res<()> {
    if !a.flags.keys().all(|k|k=="clear") {return Err("balance accepts runtime=N or --clear".into());}
    let clear=a.flags.contains_key("clear");
    let args=&a.pos[1..];
    if clear && !args.is_empty() {return Err("balance --clear cannot include weights".into());}
    let install=platform::default_install();
    let policy=host::Policy::read(&install)?;
    if clear || !args.is_empty() {
        if dispatch::agent() {return Err("only the owner can change runtime weights".into());}
        let mut value=json!({"weights":{}});
        for arg in args {
            let (rt,n)=arg.split_once('=').ok_or("balance needs runtime=N")?;
            if value["weights"].get(rt).is_some() {return Err(format!("duplicate weight {rt}"));}
            value["weights"][rt]=json!(n.parse::<u32>().map_err(|_|"weight must be an integer 0..1000000")?);
        }
        let weights=balance::weights(&value)?;
        host::write_capacity(&install,&policy.me,&CapEdit {weights:Some(weights.clone()),..Default::default()},None)?;
        let saved=host::Policy::read(&install)?;
        if saved.weights!=weights {return Err("runtime weights save uncertain: readback differs".into());}
        println!("Saved runtime weights on {}: {}; new launches use them after the service reloads",saved.me,serde_json::to_string(&saved.weights).unwrap());
    } else {
        println!("Runtime weights on {}: {}",policy.me,serde_json::to_string(&policy.weights).unwrap());
        if policy.weights.is_empty() {println!("No weights: preference/default routing");}
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
        let own=policy.as_ref().ok().filter(|p|p.me==host).map(|p|(p.claim,p.default_runtime.clone()));
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
    if dev["requested"] == true { line += " · requested"; }
    line
}

/// § 8 item 1 (#2311): request one dev build; the service starts the newest green main SHA on its next pass.
fn dev_build_cmd(a: &Args, install: &Path) -> Res<()> {
    if a.pos.len() != 2 || !a.flags.is_empty() { return Err("usage: taskq service dev-build".into()); }
    let channel = platform::channel(install)?;
    if channel != "dev" { return Err(format!("channel {channel}: dev builds run only on channel dev (taskq service channel dev)")); }
    let request = platform::dev_request(install);
    fs::create_dir_all(request.parent().unwrap_or(install)).map_err(|e| e.to_string())?;
    coreq::write_durable(&request, "requested")?;
    println!("Dev build requested; the host service starts the newest green main SHA on its next pass");
    Ok(())
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
    if fs::metadata(&log).is_ok_and(|m| m.len() > 1 << 20) { let _ = fs::remove_file(&log); }
    let err = fs::OpenOptions::new().create(true).append(true).open(&log).map(std::process::Stdio::from).unwrap_or_else(|_| std::process::Stdio::null());
    c.current_dir(install).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(err);
    // its own process group: stopping one project's service (launchctl bootout) leaves the host's tray alone
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    *tray = c.spawn().ok();
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
            tools::run("signing.adhoc", crate::platform::command("codesign").args(["-f", "-s", "-"]).arg(&app), std::time::Duration::from_secs(30))
                .map_err(|e| platform::log(&format!("ad-hoc signing failed: {e}"))).is_ok()
        }
    };
    // #1801: every tray launch (install, and update: the tray quits for a newer release) rewrites the bundle; force
    // LaunchServices to re-read it so the release icon wins over a cached or stale registration
    if signed && home.is_some() {
        const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        if let Err(e) = coreq::process::output("lsregister", crate::platform::command(LSREGISTER).arg("-f").arg(&app), std::time::Duration::from_secs(30)) {
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
    let mut command = crate::platform::command(cfg.install.join(platform::tool(&cfg.install).exe_name()));
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

/// The status line of a pending re-exec; #2254: past its 600 s bound it reads as a host incident.
fn restart_line(status: &Value) -> Option<String> {
    let w = status["restart_waits"].as_array().filter(|w| !w.is_empty())?;
    let overdue = if status["restart_overdue"] == true { "host incident: pending over 600 s; " } else { "" };
    Some(format!("Restart: {overdue}waits for {}", w.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")))
}

/// #2254: status names an unreadable child exit's reason, so a stuck re-exec reads as an incident, not "running unknown".
fn restart_waits(s: &Service, stopping:bool) -> Vec<String> {
    lifecycle_waits(s, stopping).into_iter().map(|w| {
        let reason = w.strip_prefix('#').and_then(|n| n.parse::<u64>().ok()).and_then(|n| s.kids.iter().find(|k| k.task == n)?.unknown.as_ref());
        match reason { Some(e) => format!("{w} ({e})"), None => w }
    }).collect()
}

/// § 8 step 3: what a pending re-exec waits for: codex-app turns only; every other child runs on (#2297).
fn lifecycle_waits(s: &Service, stopping:bool) -> Vec<String> {
    let mut w: Vec<String> = s.kids.iter().filter(|k| stopping || runtime::handover_waits(&k.rt)).map(|k| format!("#{}", k.task)).collect();
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
        Ok(p) => {
            let overlay=p.overlay(root).map(|(_,e)|e);
            let default=overlay.and_then(|e|e["default_runtime"].as_str()).unwrap_or(&p.default_runtime);
            out.extend([format!("Claim: {} (host {})",claim_name(p.claim),p.me),format!("Default runtime: {default} (host {})",p.me)]);
            if let Some(e)=overlay {
                if let Some(allowed)=e["allowed_runtimes"].as_array() {out.push(format!("Allowed runtimes: {}",if allowed.is_empty() {"none".into()}else{allowed.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")}));}
                if e["auto_reroute"]==true {out.push("Auto reroute: disabled CLI pins may fall back".into());}
            }
        },
        Err(e) => out.extend([format!("Claim: unknown ({e})"), "Default runtime: unknown".into()]),
    }
    if let Err(e) = &status {
        out.push(format!("Status: unknown ({e})"));
    }
    let status = status.ok().flatten();
    out.extend(status.as_ref().and_then(restart_line));
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

/// #1975: one registered project in `service status --json`. Report rows stay in `taskq results`; a status written
/// before the split still carries them inline, so they are cut to counts here too.
fn service_project(root:&Path,binding:&str,read:&Res<Option<Value>>)->Value {
    let mut status=read.as_ref().ok().and_then(Option::as_ref).cloned();
    if let Some(s)=status.as_mut().filter(|s|s["reports"]["reports"].is_array()) {s["reports"]=reports::summary(&s["reports"]);}
    json!({"root":root,"host_binding":binding,"status":status,"error":read.as_ref().err()})
}
fn lifecycle_scope(value:&Value)->String {
    let mut lines=if let Some(root)=value["project"].as_str() {
        vec![format!("Scope: project uninstall; {root}"),"Reviewed registry (other projects stay registered):".into()]
    } else {
        vec![if value["action"]=="uninstall" {"Scope: host uninstall; all projects drain; registry kept"}else{"Scope: host service; all registered projects"}.into()]
    };
    for root in value["projects"].as_array().into_iter().flatten() {lines.push(format!("Registered project: {}",root.as_str().unwrap_or("unknown")));}
    lines.join("\n")
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
        let projects:Vec<_>=rows.into_iter().map(|(root,status)|service_project(root,binding(&status),&status)).collect();
        println!("{}",json!({"schema":"taskq.service/1","service":service,"projects":projects,"registry_error":registry.as_ref().err(),"restart":if lifecycle["action"].as_str().is_some_and(|action|action!="restart") {Value::Null}else{lifecycle.clone()},"lifecycle":lifecycle,"installation":installation}));
    } else {
        println!("{}",host::service_line(&service));
        if lifecycle["telemetry"]=="unknown" {println!("Telemetry receipts: unknown; lifecycle state is recorded separately");}
        let restart=lifecycle;let title=match restart["action"].as_str() {Some("stop")=>"Stop",Some("uninstall")=>"Uninstall",Some("install")=>"Install",_=>"Restart"};
        if let Some(id)=restart["id"].as_str() {
            println!("{title}: {id} {}",restart["state"].as_str().unwrap_or("unknown"));
            if matches!(restart["action"].as_str(),Some("restart"|"stop"|"uninstall")) {println!("{}",lifecycle_scope(&restart));}
        }
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
            let churn=status.as_ref().ok().and_then(Option::as_ref).map(snapshot::churn_lines).unwrap_or_default();
            // § 6: open setup gaps, access refusals that hold launches included
            let setup=check::status_lines(&status.as_ref().ok().and_then(Option::as_ref).map_or(Value::Null,|s|s["setup"].clone()));
            println!("{}\nProject owner: {owner}",service_project_fields(root,status));
            for l in churn.into_iter().chain(setup) {println!("{l}");}
            println!("{}",incident::line(&incident::status(root)));
            if notice||incident::notice(root) {println!("Incident reports: notice");}
        }
    }
    Ok(())
}

#[tracing::instrument(name="service.command",skip_all,fields(coreq_operation=match args.pos.get(1).map(String::as_str).unwrap_or("") {
    sub @ ("install"|"check"|"run"|"onboard"|"focus"|"pause"|"resume"|"capacity"|"slots"|"app-server"|"channel"|"dev-build"|"permissions"|"resources"|"pm"|"hold"|"release"|"recover"|"status"|"restart"|"stop"|"complete-stop"|"uninstall"|"list")=>sub,
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
    if sub == "slots" {return external_slots::command(args);}
    if sub == "app-server" {return runtime::protected_app_server::command(args);}
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
    if sub == "dev-build" {return dev_build_cmd(args, &platform::default_install());}
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
            lifecycle::submit(&install,&reviewed)?
        };
        if args.one("mode").is_some_and(|mode|result["mode"]!=mode) {return Err("request ID belongs to another Stop mode".into());}
        if result["action"].as_str().unwrap_or("restart")!=sub {return Err("request ID belongs to another lifecycle action".into());}
        if args.flags.contains_key("host")&&result["scope"]!="host" {return Err("request ID belongs to a project Uninstall; payload unchanged".into());}
        if !args.flags.contains_key("json") {
            println!("{}",lifecycle_scope(&result));
            if sub=="stop" && cfg!(target_os="macos") {println!("Stop mode: {}; durable hold retained; automatic rollback to older releases disabled",result["mode"].as_str().unwrap_or("unknown"));}
        }
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
        #[cfg(windows)] {let t=platform::tool(&install);runtime::windows_process::exit_with_launcher(&[install.join(t.gui_exe_name()),install.join(t.exe_name())]);}
        // #1856: a service start (install, update, logon) logs each host tool gap with its fix
        for l in tools::gaps(&install,None) {platform::log(&l);}
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
            if let Some(id)=args.one("cancel") {
                if args.pos.len()!=2||args.flags.len()!=1 {return Err("usage: taskq service install --cancel ID".into());}
                println!("{}",host::cancel_install(&platform::default_install(),&root,id)?);return Ok(());
            }
            // § 5 Board-only root: a served root gets workers, and a worker needs a checkout
            needs_checkout(&root, "service install")?;
            let (cfg, _, _) = check_load(&root)?;
            println!("{}",host::install_project(&cfg.install,&root)?);
            if let Some(l)=host_setup(&cfg) {println!("{l}");}
            for l in tools::gaps(&cfg.install,Some(&root)) {println!("{l}");}
            match runtime::owner::registered_host_trust(&cfg.install, &root) {
                Ok(lines) => for line in lines { println!("{line}"); },
                Err(e) => println!("Trust setup pending: {e}; service retries automatically"),
            }
            // § 6: the tray's Add project runs this install, so a new project ends with its setup check
            setup_check_cmd(&root, false)
        }
        "check" => {
            if args.pos.len()!=2||args.flags.keys().any(|k|k!="json") {return Err("usage: taskq service check [--json]".into());}
            setup_check_cmd(&root, args.flags.contains_key("json"))
        }
        _ => Err("usage: taskq service install|check|run|onboard|focus|pause|resume|capacity|channel|permissions|hold|release|recover|status|restart|stop|uninstall|list".into()),
    }
}

/// `taskq doctor [--dry-run] [--hosts] [--json]` ([host doctor](docs/design/host-service.md#host-doctor)): without
/// `--hosts`, one cleanup pass now (#2060), the host-setup gap (#2078) and the health checks of every registered board
/// (#2194); with `--hosts`, per board one table of every host's published health and one of the running workers, read
/// from the board alone. `--json` prints the same rows.
fn doctor_cmd(a: &Args) -> Res<()> {
    if a.pos.len() != 1 || a.flags.keys().any(|k| !["dry-run", "hosts", "json"].contains(&k.as_str())) {
        return Err("usage: taskq doctor [--dry-run] [--hosts] [--json]".into());
    }
    let (hosts, json_out) = (a.flags.contains_key("hosts"), a.flags.contains_key("json"));
    let install = platform::default_install();
    let (mut out, mut cleanup) = (vec![], Value::Null);
    if !hosts {
        let lines = doctor::cli(&install, a.flags.contains_key("dry-run"));
        cleanup = json!(lines);
        out.extend(lines);
        // #2078: a project's missing one-time host setup is a host gap the doctor names too
        if let Some(cfg) = find_root().ok().and_then(|r| load(&r).ok()).map(|(c, _)| c) {
            if let Some(cmd) = cfg.host_setup.as_deref().filter(|c| !check::host_setup_done(&cfg.root, c)) {
                out.extend(check::lines(&[check::host_setup_item(&cfg.root, cmd, false)]));
            }
        }
    }
    // every registered project of this host, else this project; one row set per board
    let roots = host::projects(&install).ok().filter(|r| !r.is_empty()).or_else(|| find_root().ok().map(|r| vec![r])).unwrap_or_default();
    let latest = if hosts { Err("not read".into()) } else { platform::latest_release(&install) };
    let (mut seen, mut boards) = (std::collections::BTreeSet::new(), vec![]);
    // #2246: `--hosts` reads every board first, asks all hosts at once and waits once for the whole run
    let mut opened: Vec<(Config, Box<dyn board::Board>, snapshot::Snapshot)> = vec![];
    let (mut host_rows, mut worker_rows) = (vec![], vec![]);
    let unread = |repo: &str, e: &str| json!({"board": repo, "host": "-", "version": "-", "state": "unknown", "gaps": [], "reason": format!("board unread: {e}"),
        "fix": "check the board token and network on this host, then `taskq doctor --hosts` again"});
    for root in roots {
        let (cfg, raw) = match load(&root) {
            Ok(v) => v,
            Err(e) => {
                if hosts { host_rows.push(unread(&project_name(&root), &e)); } else { out.push(format!("project {}: unknown: {e}", project_name(&root))); }
                boards.push(json!({"project": project_name(&root), "error": e}));
                continue;
            }
        };
        if !seen.insert(cfg.repo.clone()) { continue; }
        let gitlab = cfg.board.name == "gitlab";
        let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install));
        if !hosts {
            let items = match &board {
                Ok(b) => health::board_items(b.as_ref(), &cfg.repo, gitlab, &cfg.root, &cfg.install, &latest),
                Err(e) => health::unopened(&cfg.repo, gitlab, e),
            };
            out.extend(health::lines(&cfg.repo, &items));
            boards.push(json!({"board": cfg.repo, "items": items.iter().map(check::Item::json).collect::<Vec<_>>()}));
            continue;
        }
        let b = match board {
            Ok(b) => b,
            Err(e) => {
                host_rows.push(unread(&cfg.repo, &e));
                boards.push(json!({"board": cfg.repo, "error": e, "hosts": [], "workers": []}));
                continue;
            }
        };
        let s = snapshot::read(b.as_ref(), &cfg, &raw, &[]);
        if let Some(e) = &s.reason {
            host_rows.push(unread(&cfg.repo, e));
            boards.push(json!({"board": cfg.repo, "error": e, "hosts": [], "workers": []}));
            continue;
        }
        opened.push((cfg, b, s));
    }
    if hosts {
        let ttl = opened.iter().map(|(c, ..)| c.ttl).max().unwrap_or(TTL);
        let lists: Vec<Vec<snapshot::Host>> = opened.iter_mut().map(|(_, _, s)| std::mem::take(&mut s.hosts)).collect();
        let mut asks: Vec<health::Ask> = opened.iter().zip(lists).map(|((cfg, b, s), mut list)| {
            let asked = health::request(b.as_ref(), &mut list, s.observed, cfg.ttl);
            health::Ask::new(b.as_ref(), list, asked, b.now().unwrap_or(s.observed), cfg.ttl)
        }).collect();
        if !json_out { eprintln!("doctor: asked every host of {} board(s) to run its health checks (up to {}s in all)", asks.len(), health::ASK_WAIT); }
        health::collect(&mut asks, ttl, health::ASK_WAIT, health::ASK_POLL, &|t| std::thread::sleep(std::time::Duration::from_secs(t)));
        let answered: Vec<(Vec<snapshot::Host>, health::Asked)> = asks.into_iter().map(|a| (a.hosts, a.asked)).collect();
        for ((cfg, b, s), (list, asked)) in opened.iter_mut().zip(answered) {
            s.hosts = list;
            let now = b.now().unwrap_or(s.observed + asked.waited as i64);
            let tag = |mut v: Value| { v["board"] = json!(cfg.repo); v };
            let rows: Vec<Value> = s.hosts.iter().map(|h| tag(health::host_row(h, &cfg.repo, cfg.board.name == "gitlab", now, cfg.ttl, Some(&asked)))).collect();
            let workers: Vec<Value> = health::worker_rows(s, cfg.ttl).into_iter().map(tag).collect();
            if rows.is_empty() { host_rows.push(json!({"board": cfg.repo, "host": "-", "version": "-", "state": "unknown", "gaps": [], "reason": "no host heartbeat", "fix": "install a host service for this board: `taskq service install` in its project"})); }
            host_rows.extend(rows.iter().cloned());
            worker_rows.extend(workers.iter().cloned());
            boards.push(json!({"board": cfg.repo, "hosts": rows, "workers": workers}));
        }
        out.extend(health::hosts_lines(&host_rows, &worker_rows));
    }
    if json_out {
        let mut v = json!({"boards": boards});
        if !hosts { v["cleanup"] = cleanup; }
        println!("{v}");
    } else {
        if boards.is_empty() { out.push("doctor: no project registered on this host".into()); }
        for l in out { println!("{l}"); }
    }
    Ok(())
}

/// § 6 Shared project bootstrap (#2078): run the project's `host_setup` once per host, outside any worker sandbox, and
/// stamp it so the `host-setup` gap clears; a failure keeps the gap and says why. `None`: undeclared or already done.
fn host_setup(cfg: &Config) -> Option<String> {
    let cmd = cfg.host_setup.as_deref().filter(|c| !check::host_setup_done(&cfg.root, c))?;
    // only a script inside the root: no absolute path, no `..`
    if !Path::new(cmd).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Some(format!("host setup: `{cmd}` refused: not a path inside the project root"));
    }
    // #2239: the owner runs `service install` and can interrupt it; the script takes as long as the project needs
    let ran = platform::complete("host.setup", platform::command(cfg.root.join(cmd)).current_dir(&cfg.root))
        .and_then(|o| if o.status.success() {Ok(())} else {Err(format!("{}: {}", o.status, String::from_utf8_lossy(&o.stderr).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no error output")))})
        .and_then(|_| fs::create_dir_all(cfg.root.join(".taskq")).and_then(|_| fs::write(check::host_setup_stamp(&cfg.root), format!("{cmd}\n"))).map_err(|e| e.to_string()));
    Some(match ran { Ok(()) => format!("host setup: `{cmd}` done"), Err(e) => format!("host setup: `{cmd}` failed: {e}") })
}

/// § 6 setup check items of one project (#1938): read-only. The caller passes the effective runtime limits, the § 5
/// `agents` rows, the § 8 Access report, the open tasks' labels and the board's PR route (`None`: not read).
fn setup_items(cfg: &Config, limits: &BTreeMap<String, u32>, delivery: Result<String, String>, agents: &Value, access: &[policy::Effective],
    open: Option<Vec<(u64, Vec<String>)>>, pr_route: Option<check::Probe>) -> Vec<check::Item> {
    // A healthy fallback removes the preferred CLI prerequisite; project-disabled pins follow the same exception as admission.
    let disabled = |rt: &str| cfg.runtime_disabled(rt, Some(limits));
    let selected = labels::select_project(&[], false, &cfg.default_runtime, cfg.auto_reroute, disabled, |rt| {
        let login = agents.as_array().into_iter().flatten().find(|r| r["runtime"] == rt).and_then(|r| r["login"].as_str());
        let allowed = access.iter().any(|e| e.runtime == rt && e.refused.is_none() && !e.grant.starts_with("none") && e.grant != "unknown");
        if !disabled(rt) && login == Some("logged in") && allowed { Ok(rt.into()) }
        else { Err(("access", format!("{rt} not ready"))) }
    }).ok().map(|(rt, _)| rt);
    // If no runtime is ready, keep the first enabled candidate's setup gaps visible.
    let selected = selected.or_else(|| labels::candidates(&[], false, &cfg.default_runtime).into_iter().find(|rt| !disabled(rt)));
    let fallback = selected.as_ref().is_some_and(|rt| rt != &cfg.default_runtime);
    let pinned = |rt: &str| !(cfg.auto_reroute && runtime::CLI_RUNTIMES.contains(&rt) && disabled(rt))
        && open.as_ref().is_some_and(|tasks| tasks.iter().any(|(_, labels)| labels.iter().any(|l| l.strip_prefix("run-") == Some(rt))));
    let runtimes = runtime::agent_runtimes().iter().filter(|rt| {
        let in_use = !disabled(rt) && match limits.get(**rt) { Some(n) => *n > 0, None => selected.as_ref().unwrap_or(&cfg.default_runtime) == *rt };
        pinned(rt) || in_use && !(fallback && **rt == cfg.default_runtime)
    })
        .map(|rt| {
            let e = access.iter().find(|e| e.runtime == *rt);
            check::Runtime {
                name: rt.to_string(),
                missing: runtime::missing_cli(rt),
                login: agents.as_array().into_iter().flatten().find(|r| r["runtime"] == *rt).and_then(|r| r["login"].as_str()).map(String::from),
                refused: e.and_then(|e| e.refused.clone()),
                granted: e.is_some_and(|e| !e.grant.starts_with("none") && e.grant != "unknown"),
            }
        }).collect();
    check::items(&check::Facts {
        os: std::env::consts::OS.into(),
        root: cfg.root.clone(),
        host: cfg.me.clone(),
        git: cfg.root.join(".git").exists(),
        external: cfg.external.then(|| cfg.root.join("AGENTS.md").is_file()),
        runtimes,
        codex_dir: cfg.root.join(".codex").is_dir(),
        labelled: cfg.claim == host::ClaimMode::Labelled,
        unlabelled: open.map(|v| v.into_iter().filter(|(_, l)| !l.iter().any(|l| l.starts_with("host-"))).map(|(n, _)| n).collect()),
        token: (cfg.board.name == "github").then(|| board::github::token(&cfg.install).map(|_| ()).map_err(|_| "missing, empty or unreadable".to_string())),
        delivery,
        pr_route: (cfg.publish == "pr").then(|| pr_route.unwrap_or_else(|| check::Probe::Unknown("board not read".into()))),
        host_setup: cfg.host_setup.as_ref().map(|c| (c.clone(), check::host_setup_done(&cfg.root, c))),
    })
}

/// `taskq service check [--json]` (§ 6): reads only; the board answers open tasks and the PR route when it opens.
fn setup_check_cmd(root: &Path, json_out: bool) -> Res<()> {
    let (cfg, raw, delivery) = check_load(root)?;
    let limits = match host::Policy::read(&cfg.install) {
        Ok(p) => p.project(&cfg.root, &raw).map(|c| c.limits),
        Err(_) => service::capacity(&raw, &cfg.me).map(|c| c.limits),
    }.unwrap_or_else(|_| cfg.limits.clone());
    let access = policy::report(&cfg.install, &cfg.root, &cfg.permission_mode, &cfg.codex);
    let (open, route) = match cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install)) {
        Ok(b) => (b.issues(None).ok().map(|v| v.into_iter().filter(service::is_task).map(|i| (i.number, i.labels)).collect()),
            (cfg.publish == "pr").then(|| b.pr_route())),
        Err(e) => (None, Some(check::Probe::Unknown(e))),
    };
    let items = setup_items(&cfg, &limits, delivery, &runtime::agents(&cfg.install), &access, open, route);
    if json_out { println!("{}", check::json(&project_name(&cfg.root), &items)); } else { for l in check::lines(&items) { println!("{l}"); } }
    Ok(())
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
            Some("not installed") => { claude &= !runtime::of(rt).has("remote-control"); gaps.push(setup::onboard_skipped(rt, cfg!(windows) && std::env::var_os("SSH_CONNECTION").is_some())) }
            _ => gaps.extend([format!("Login: {}", runtime::agent_text(r)), format!("Next: {}", runtime::relogin(rt))]),
        }
    }
    let p = host::Policy::read_for_hostname(install, hostname);
    let (mode, codex, has) = p.map_or(("dontAsk".to_string(), vec![], vec![]), |p| (p.permission_mode, p.codex, p.has));
    // codex-app is opt-in (§ 9): its refusal is a gap only where `has` lists it
    let opted = |es: Vec<policy::Effective>| es.into_iter().filter(|e| !runtime::of(&e.runtime).has("app-turn") || has.contains(&e.runtime)).collect::<Vec<_>>();
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
    gaps.extend(tools::gaps(install, None));
    if gaps.is_empty() { gaps.push(format!("Ready: {} project(s) on {}; tools, logins, access and trust in place", roots.len(), hostname())); }
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
    pub conversation: Option<String>,
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
    /// § 1 Task text: the rule IDs this task changes, or `all`; the rule gate passes edits inside them.
    pub rules: Option<String>,
}

/// § 1 Task text: rule IDs (`R<n>`, `§<n>`, `§2.<k>`) and `taskq <command>` spans in goal or acceptance text that
/// neither taskq.md nor the command catalog defines; `new R14` names a rule the task adds.
fn unknown_terms(text: &str) -> Vec<String> {
    // ponytail: checks the command word only; subcommand words when a vague subcommand causes an ask
    let mut rules: Vec<String> = vec![];
    let mut in_log = false;
    for l in runtime::CONTRACT.lines() {
        if let Some(h) = l.strip_prefix("### R") { rules.push(format!("R{}", h.split('.').next().unwrap_or(""))); }
        if let Some(h) = l.strip_prefix("## ") {
            let n = h.split('.').next().unwrap_or("");
            in_log = n == "2";
            if h.contains('.') && n.parse::<u32>().is_ok() { rules.push(format!("§{n}")); }
        }
        if in_log && l.starts_with("| ") {
            if let Some(id) = l.trim_end_matches(['|', ' ']).rsplit('|').next().map(str::trim).filter(|id| id.parse::<u32>().is_ok()) { rules.push(format!("§2.{id}")); }
        }
    }
    let catalog = format!("{USAGE}\n{PM_COMMANDS}\n{}", runtime::CONTRACT);
    let commands: Vec<&str> = catalog.split("taskq ").skip(1).filter_map(|s| s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).next()).collect();
    let mut out: Vec<String> = vec![];
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        let rule = (c == 'R' && !(prev.is_alphanumeric() || ".#/-_".contains(prev))) || c == '§';
        if rule {
            let mut j = i + 1;
            if c == '§' && chars.get(j) == Some(&' ') { j += 1; }
            let start = j;
            while j < chars.len() && (chars[j].is_ascii_digit() || (chars[j] == '.' && chars.get(j + 1).is_some_and(char::is_ascii_digit) && c == '§')) { j += 1; }
            let next = chars.get(j).copied().unwrap_or(' ');
            if j > start && !next.is_alphanumeric() {
                let id = format!("{}{}", if c == 'R' { "R" } else { "§" }, chars[start..j].iter().collect::<String>());
                let fresh = chars[..i].iter().collect::<String>().to_lowercase().ends_with("new ");
                if !fresh && !rules.contains(&id) && !out.contains(&id) { out.push(id); }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    for (k, span) in text.split('`').enumerate() {
        if k % 2 == 1 {
            if let Some(cmd) = span.strip_prefix("taskq ").and_then(|s| s.split_whitespace().next()) {
                let term = format!("taskq {cmd}");
                if !cmd.starts_with('-') && !commands.contains(&cmd) && !out.contains(&term) { out.push(term); }
            }
        }
    }
    out
}

/// § 1 Task text: add and start refuse goal or acceptance terms no contract or catalog defines, naming each.
fn lint_terms(text: &str) -> Res<()> {
    let bad = unknown_terms(text);
    if bad.is_empty() { Ok(()) } else { Err(format!("unknown term: {}; fix the task text (taskq.md, `taskq help`) or write `new <ID>`", bad.join(", "))) }
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
        lint_terms(&format!("{}\n{}", t.goal, t.acceptance.join("\n"))),
    ])?;
    let mut body = format!("## Goal\n{}\n\n## Acceptance\n{}\n", goal.join("\n"), acc.join("\n"));
    if !scope.is_empty() {
        body += &format!("\n## Scope\n{}\n", scope.join("\n"));
    }
    if let Some(r) = &t.rules {
        let ids: Vec<&str> = r.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if ids != ["all"] && (ids.is_empty() || ids.iter().any(|id| !id.starts_with(['R', '§']) || !unknown_terms(id).is_empty())) {
            return Err(format!("--rules {r}: takes contract rule IDs (R<n>, §<n>, §2.<k>, §<n> <Title>) comma-separated, or all"));
        }
        body += &format!("\nRules: {}\n", ids.join(", "));
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
    if t.conversation.as_ref().is_some_and(|id| !setup::owner_id(id)) { return Err("Invalid conversation ID".into()); }
    if t.conversation.is_some() && op.id.is_none() { return Err("--conversation requires --op-id for same-request reconciliation".into()); }
    let origin = t.conversation.as_ref().map_or(String::new(), |id| format!("conversation: {id}"));
    let n = if op.id.is_some() {
        match claimed_create(b, raw, &t.title, &body, &labels, &origin, op) {
            Ok(n) => n,
            Err(r) => return Ok(r),
        }
    } else { b.create(&t.title, &body, &labels)? };
    let r = act(b, root, n, "hold", &[], &origin, &Op::default());
    Ok(Receipt { op_id: op.id.clone(), ..r }.wake(root))
}

/// § 7 owner gate on a held or ready task: label `needs-gate`, then the `gate` entry (op id `<op>.gate`).
fn gate_task(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Receipt {
    if let Err(e) = b.labels(n, &["needs-gate".into()], &[]) {
        return Receipt::rejected(n, op, e);
    }
    act(b, root, n, "gate", &[], "", &Op { id: op.id.as_ref().map(|i| format!("{i}.gate")), revision: None })
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
        let log = entries(b, n)?;
        let projection = core::project(n, &log, false);
        let held = service::is_held(&issue) || projection.task.state == State::Held;
        let repair = !service::is_task(&issue) && projection.task.state == State::Ready && service::start_logged(n, &log, &projection);
        // a retried op id may find its task already started; `act` returns the first receipt
        if !issue.trusted || !issue.open || !(held || repair || op.id.is_some()) {
            return Err(format!("#{n} is not a held task; nothing started"));
        }
        // § 1 Task text: only the Goal and Acceptance sections; Scope and links may name paths and issues
        let text: String = issue.body.split("\n## ").map(|s| s.trim_start_matches("## ")).filter(|s| s.starts_with("Goal") || s.starts_with("Acceptance")).collect::<Vec<_>>().join("\n");
        lint_terms(&text).map_err(|e| format!("#{n}: {e}; nothing started"))?;
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

fn json_output(text: &str, file: Option<&str>) -> Res<()> {
    if let Some(path) = file { platform::write_atomic(Path::new(path), text).map_err(|e| format!("JSON file write failed: {e}")) }
    else { println!("{text}"); Ok(()) }
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
    let mut a = Args::parse(argv)?;
    telemetry::diagnostic::poll(&platform::default_install());
    if let Some(selector)=a.one("diagnose") {
        let milliseconds=a.one("diagnose-ms").unwrap_or("60000").parse::<u64>().map_err(|_|"invalid --diagnose-ms")?;
        let limit=a.one("diagnose-limit").unwrap_or("256").parse::<u64>().map_err(|_|"invalid --diagnose-limit")?;
        telemetry::diagnostic::select(selector,Duration::from_millis(milliseconds),limit)?;
    } else if a.one("diagnose-ms").is_some() || a.one("diagnose-limit").is_some() {return Err("diagnostic limits require --diagnose".into());}
    if a.pos.is_empty() || a.pos.first().is_some_and(|s| s == "help") || a.pos.iter().any(|s| s == "-h") || a.flags.contains_key("help") {
        println!("{}", help(a.flags.contains_key("json"))?);
        return Ok(());
    }
    a.read_text(&mut std::io::stdin().lock())?;
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
            // #1946: project CI runs this so a contract edit over the cap fails before merge, not at worker start
            [size] if size == "size" => {
                let (len, cap) = runtime::rules_size(&find_root()?).map_err(|e| e.to_string())?;
                println!("Rules: {len} of {cap} bytes, headroom {}", cap - len);
                Ok(())
            }
            _ => Err("usage: taskq rules size | taskq rules compact ROOT ROLE TASK ATTEMPT | taskq rules pm start|compact".into()),
        },
        "selftest" => {
            let n = golden_selftest(core::GOLDEN, !platform::VERSION.contains('-')).map_err(|e| format!("fold: FAIL {e}"))?;
            println!("fold: {n} golden traces PASS");
            println!("{}", formats_selftest()?);
            if std::env::var_os("TASKQ_SELFTEST_OFFLINE").is_some() {
                println!("board: unknown (TASKQ_SELFTEST_OFFLINE set; no board read)");
            } else if let Ok(root) = find_root() {
                let c = ctx()?;
                let (views, hosts) = service::views(c.board.as_ref(), &mut Default::default(), &mut Default::default(), c.cfg.ttl)?;
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
                // #1935: a preset the repository's plan cannot finish, with the owner's fixes
                println!("delivery: {} r{}: {}", c.cfg.delivery, c.cfg.delivery_revision, match c.board.delivery_gap(&c.cfg.publish) {
                    Ok(None) => "no plan mismatch found".into(),
                    Ok(Some(gap)) => format!("MISMATCH {gap}; fix: make the repository public (merge queue), upgrade the plan, set delivery.preset direct, or merge each PR by hand"),
                    Err(e) => format!("unknown ({e})"),
                });
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
            // #1856: the release staging selftest is offline; a host gap must not hold the update that may fix it
            // the dev install and tray Add project check the build and the project, not this host's tools (TASKQ_SELFTEST_TOOLS=off)
            for var in ["TASKQ_SELFTEST_OFFLINE","TASKQ_SELFTEST_TOOLS"] {
                if std::env::var_os(var).is_some() {println!("tools: unknown ({var} set; no tool probe)");return Ok(());}
            }
            let rows=tools::rows(&platform::default_install(),find_root().ok().as_deref());
            for (line,failed) in &rows {println!("tool {}{line}",if *failed {"FAIL "} else {""});}
            let failed:Vec<_>=rows.iter().filter(|r|r.1).map(|r|r.0.split(':').next().unwrap_or("?")).collect();
            if failed.is_empty() {Ok(())} else {Err(format!("tools: FAIL {}; fix steps above (docs/design/host-service.md#host-tools)",failed.join(", ")))}
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
            if !check {for l in tools::gaps(&install, find_root().ok().as_deref()) {println!("{l}");}}
            Ok(())
        }
        // § 5: read-only probes on this host, no board
        "agents" => {
            for r in runtime::agents(&platform::default_install()).as_array().into_iter().flatten() { println!("{}", runtime::agents_line(r)); }
            Ok(())
        }
        // § 3 Runtime accounts: list reads every host; enable, disable, move and add write this host only
        "accounts" => accounts_cli(&a),
        "balance" => balance_cli(&a),
        "service" => service(&a),
        // #2060 (docs/design/worktrees.md#commands): the tidy of each registered project of this host
        "cleanup" if a.flags.contains_key("all") => {
            let roots = host::projects(&platform::default_install())?;
            if roots.is_empty() { println!("cleanup: no registered project on this host"); }
            for root in roots {
                println!("{}: {}", project_name(&root), root.display().to_string().trim_start_matches(r"\\?\"));
                let lines = needs_checkout(&root, "cleanup").and_then(|_| load(&root)).and_then(|(cfg, raw)| {
                    let b = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
                    Ok(cleanup_lines(&*b, &cfg, &a))
                });
                match lines {
                    Ok(lines) => for l in lines { println!("{l}"); },
                    Err(e) => println!("skipped: {e}"),
                }
            }
            Ok(())
        }
        // #2060 (docs/design/host-service.md#host-doctor): one host doctor pass now; no board
        "doctor" => doctor_cmd(&a),
        // § 5: an owner session (§ 4) in this project; no board
        "retro" => {
            let (cfg, _) = load(&find_root()?)?;
            let r = setup::open(&cfg.install, &cfg.root, setup::RETRO)?;
            println!("retro: request {} pending; the host service opens a new session in {}", r.id, cfg.root.display());
            Ok(())
        }
        "tray" => tray(platform::default_install(), a.one("snapshot"), a.one("open").is_some(), &a.all("route").iter().flat_map(|r| r.split(',')).map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect::<Vec<_>>(), a.one("icon")),
        "" => Err("command missing; use `taskq help`".into()),
        "status" if !a.flags.contains_key("closed") => status_all(&a),
        "reports"|"results" => match reports::read_cli(&find_root()?,&a)? {Some(value)=>{println!("{value}");Ok(())},None=>command(&ctx()?,cmd,&a)},
        _ => command(&ctx()?, cmd, &a),
    }
}

/// `taskq accounts` ([§ 3 Runtime accounts](../taskq.md#runtime-accounts)): this host's rows from its own files and a
/// read-only login probe per home (the email only here); other hosts' rows from their heartbeats, never an email.
fn accounts_cli(a: &Args) -> Res<()> {
    let install = platform::default_install();
    let w: Vec<&str> = a.pos.iter().skip(1).map(String::as_str).collect();
    let out = match w.as_slice() {
        [] => {
            let h = accounts::Host::read(&install);
            if let Some(e) = &h.error { eprintln!("accounts: {e}; new account starts remain unknown and held"); }
            let board = ctx();
            let me = board.as_ref().map_or_else(|_| "this-host".to_string(), |c| c.cfg.me.clone());
            let mut rows = h.rows(&me, accounts::now(), runtime::account_email);
            match &board {
                Ok(c) => for i in c.board.issues(None)?.iter().filter(|i| i.trusted) {
                    if let Some(host) = service::host_name(i).filter(|h| *h != me) { rows.extend(accounts::heartbeat_rows(host, &i.body)); }
                },
                Err(e) => eprintln!("accounts: other hosts unknown ({e}); this host only"),
            }
            if a.flags.contains_key("json") { serde_json::to_string(&rows.iter().map(accounts::Row::json).collect::<Vec<_>>()).map_err(|e| e.to_string())? }
            else { rows.iter().map(accounts::Row::text).collect::<Vec<_>>().join("\n") }
        }
        [op @ ("enable" | "disable"), rt, label] => accounts::write(&install, op, rt, label, None)?,
        ["move", rt, label, at] => accounts::write(&install, "move", rt, label, Some(at.parse().map_err(|_| "position must be a number")?))?,
        ["add", rt, label] => accounts::add(&install, rt, label)?,
        ["fleet", op, rt, id] => {
            runtime::usage::fleet_allowed()?;
            let c = ctx()?;
            runtime::usage::fleet_write(&*c.board, op, rt, id)?
        }
        _ => return Err("usage: taskq accounts [--json] | taskq accounts enable|disable <runtime> <label> | taskq accounts move <runtime> <label> <position> | taskq accounts add <runtime> <label> | taskq accounts fleet use|avoid|clear <runtime> <id>".into()),
    };
    println!("{out}");
    Ok(())
}

fn create_claim_issue(b: &dyn Board, raw: &Value, op: &Op) -> Result<u64, Receipt> {
    let read = |reason: String| Receipt {
        outcome: "unknown", next: "Retry the identical add with the same --op-id for read-only discovery.".into(),
        ..Receipt::rejected(0, op, reason)
    };
    let claim_issue = raw["create_claim_issue"].as_u64().filter(|n| *n > 0)
        .ok_or_else(|| Receipt::rejected(0, op, "configure create_claim_issue before idempotent create"))?;
    let issue = b.issue(claim_issue).map_err(read)?;
    if board::claim_issue_refusal(&issue).is_some() {
        return Err(Receipt::rejected(0, op, "create_claim_issue must be trusted and outside the queue/hosts"));
    }
    Ok(claim_issue)
}

/// § 1 `taskq claim-issue [N]`: settle the board's claim issue, then write `create_claim_issue` to taskq.json
/// and read it back. An existing key is never replaced; the receipt adds `key` to the board receipt.
fn claim_issue_setup(b: &dyn Board, root: &Path, adopt: Option<u64>) -> Res<Value> {
    let path = root.join("taskq.json");
    let read = || -> Res<Value> {
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).map_err(|e| format!("taskq.json unknown: {e}"))?).map_err(|e| format!("taskq.json unknown: {e}"))?;
        if v.is_object() { Ok(v) } else { Err("taskq.json unknown: not an object".into()) }
    };
    let mut v = read()?;
    let configured = match &v["create_claim_issue"] {
        Value::Null => None,
        k => Some(k.as_u64().filter(|n| *n > 0).ok_or("create_claim_issue in taskq.json is not an issue number")?),
    };
    let mut r = board::claim_issue(b, configured, adopt);
    r["key"] = json!("create_claim_issue");
    let Some(n) = r["issue"].as_u64().filter(|_| r["outcome"] == "accepted" && configured.is_none()) else { return Ok(r) };
    v["create_claim_issue"] = json!(n);
    let written = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
        .and_then(|text| platform::write_atomic(&path, &(text + "\n")))
        .and_then(|_| read())
        .and_then(|v| if v["create_claim_issue"].as_u64() == Some(n) { Ok(()) } else { Err("taskq.json read back without the new create_claim_issue".into()) });
    if let Err(e) = written {
        r["outcome"] = json!("unknown");
        r["reason"] = json!(e);
        r["next"] = json!("Retry the same taskq claim-issue: it reuses the issue and rewrites the key.");
    }
    Ok(r)
}

/// §1 create claim: only the invocation that reads back its winning claim may POST an issue.
fn claimed_create(b: &dyn Board, raw: &Value, title: &str, body: &str, labels: &[String], later_text: &str, op: &Op) -> Result<u64, Receipt> {
    let id = op.id.as_deref().expect("claimed create needs an op id");
    let unknown = |n, reason: String| Receipt {
        outcome: "unknown", next: "Retry the identical add with the same --op-id for read-only discovery.".into(),
        ..Receipt::rejected(n, op, reason)
    };
    let read = |e| unknown(0, e);
    let claim_issue = create_claim_issue(b, raw, op)?;
    let request = json!({"actor": b.me().map_err(read)?, "title": title, "body": body, "labels": labels, "later_text": later_text}).to_string();
    let hash: String = ring::digest::digest(&ring::digest::SHA256, request.as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let header = format!("taskq: create-claim {id}");
    let claim = format!("{header}\nrequest-sha256: {hash}");
    // GitLab keeps a title and description without surrounding whitespace, dropping the newline that ends `add` bodies (#1914)
    let same = |stored: &str, sent: &str| stored.trim() == sent.trim();
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
        if !i.trusted || !same(&i.title, title) || !same(&i.body, body) {
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
    if !issue.trusted || !same(&issue.title, title) || !same(&issue.body, body) || issue.labels.len() != labels.len() || labels.iter().any(|l| !issue.labels.contains(l)) {
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

/// One finding's body (Source, Evidence, op id) and its identity-derived op id, then its claimed create
/// (docs/design/finding-tasks.md); `taskq finding` and the service's tidy findings (docs/design/worktrees.md) share it.
/// `evidence` lines come without the `- ` bullet. Errors come only from the task body.
#[allow(clippy::too_many_arguments)]
fn submit_finding(b: &dyn Board, raw: &Value, root: &Path, board: &str, web_host: &str, repo: &str, new: &NewTask, source: &str, evidence: &[String], op: &mut Op, start: bool) -> Res<(Receipt, Option<Receipt>)> {
    let (mut body, labels) = task_body(b, new, &Op::default())?;
    let board_url = b.url(0);
    let identity = json!({"kind": board, "host": web_host, "repo": repo, "url": board_url.trim_end_matches("/0"), "source": source});
    let hash: String = ring::digest::digest(&ring::digest::SHA256, identity.to_string().as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    op.id = Some(format!("finding.{hash}"));
    body += &format!("\n## Source\n{source}\n");
    if !evidence.is_empty() { body += &format!("\n## Evidence\n{}\n", evidence.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n")); }
    let request = json!({"board": identity, "start": start}).to_string();
    body += &format!("\nop-id: {}\n", op.id.as_deref().unwrap());
    Ok(finding_submit(b, raw, root, &new.title, &body, &labels, &request, op, start))
}

/// Section 1 start log -> eligibility label -> authoritative readback, shared by start and finding.
fn start_one(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Receipt {
    let mut start_op = op.clone();
    let mut repair = false;
    // A finding's q-held projection prevents admission even before its hold log is initialized.
    let mut initialize = || -> Res<Option<Receipt>> {
        let issue = b.issue(n)?;
        let log = entries(b, n)?;
        let projection = core::project(n, &log, false);
        let revision = projection.revision;
        if op.revision.is_some_and(|r| r != revision) { return Ok(None) }
        repair = issue.trusted && issue.open && projection.task.state == State::Ready && service::start_logged(n, &log, &projection)
            && (!service::is_task(&issue) || issue.labels.iter().any(|l| l == "q-held"));
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
    if !repair {
        let r = act(b, root, n, "start", &[], "", &start_op);
        if !r.ok() { return r }
    }
    let readback = || -> Res<Receipt> {
        let (issue, t, _) = current(b, n)?;
        let log = entries(b, n)?;
        let started = service::start_logged(n, &log, &core::project(n, &log, false));
        if started && issue.open { service::project(b, &issue, &t)?; }
        if started && issue.open && !service::is_task(&issue) {
            b.labels(n, &[service::TASK_LABEL.into()], &[])?;
        }
        let (issue, t, _) = current(b, n)?;
        let want = b.state_label(t.state.name())?;
        if issue.open && (!service::is_task(&issue) || !issue.labels.contains(&want) || issue.labels.iter().any(|l| l.starts_with("q-") && l != &want)) {
            return Err("start eligibility labels not read back".into());
        }
        Ok(Receipt::accepted(n, op, if issue.open { t.state } else { State::Closed }, core::revision(n, &entries(b, n)?)))
    };
    readback().map(|r| if repair { r.wake(root) } else { r }).unwrap_or_else(|e| Receipt { outcome: "unknown", next: format!("Read taskq inbox {n}; retry the same start operation."), ..Receipt::rejected(n, op, e) })
}

/// §5: explicit owner request; labels change only on ready tasks without unreleased execution.
fn reroute(b: &dyn Board, root: &Path, from: &str, to: &str) -> Res<Vec<Receipt>> {
    if from == to || ![from, to].iter().all(|rt| runtime::CLI_RUNTIMES.contains(rt)) {
        return Err("reroute requires two different CLI runtimes: claude, codex or antigravity".into());
    }
    let (old, new) = (format!("run-{from}"), format!("run-{to}"));
    b.ensure_labels(std::slice::from_ref(&new))?;
    let mut out = vec![];
    for issue in b.issues(None)?.into_iter().filter(|i| i.open && service::is_task(i) && (i.labels.contains(&old) || i.labels.contains(&new))) {
        let n = issue.number;
        let pending=match entries(b,n) {
            Ok(log)=>core::project(n,&log,false).reroute,
            Err(e)=>{out.push(Receipt {outcome:"unknown",..Receipt::rejected(n,&Op::default(),e)}.wake(root));continue;}
        };
        if !issue.labels.contains(&old) && !pending.as_ref().is_some_and(|(_,source,target)| source==from && target==to) {continue;}
        let mut submitted=pending.is_some();
        let mut change = || -> Res<Receipt> {
            let (issue, task, _) = current(b, n)?;
            let log = entries(b, n)?;
            if !issue.open || task.state != State::Ready { return Err(format!("#{} is {}; reroute only changes open ready tasks", n, task.state.name())); }
            if pending.is_none() {labels::validate(&issue.labels)?;}
            if !issue.labels.contains(&old) && pending.is_none() { return Err("source runtime pin changed; read the task again".into()); }
            if core::project(n, &log, false).executions.iter().any(|x| x.release.is_none()) { return Err("execution unresolved; preserve its runtime and session".into()); }
            let projection=core::project(n,&log,false);
            if let Some((_,source,target))=&projection.reroute {
                if source!=from || target!=to {return Err("another reroute is pending; wait for its readback".into());}
            } else {
                submitted=true;
                let id=b.comment(n, &format!("taskq: reroute {from} {to}\nStrict runtime pin: {old} -> {new}\nrouting-fence: 1"))?;
                let fresh=core::project(n,&entries(b,n)?,false);
                if fresh.reroute.as_ref().map(|r|r.0)!=Some(id) {return Err("routing fence not acquired; no labels changed".into());}
            }
            Ok(dispatch::finish_reroute(b,n))
        };
        out.push(change().unwrap_or_else(|e| {
            let mut r=Receipt::rejected(n,&Op::default(),e);
            if submitted {r.outcome="unknown";r.next="The service reconciles pending routing intents; retry the same command for readback.".into();}
            r
        }).wake(root));
    }
    Ok(out)
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

/// `taskq cleanup [--force] [--dry-run]` of one root ([worktrees](docs/design/worktrees.md#commands)): every removal,
/// refusal, kept task worktree and root move with its reason, else `nothing to clean`.
fn cleanup_lines(b: &dyn Board, cfg: &Config, a: &Args) -> Vec<String> {
    // a child the local service still runs keeps its worktree, whatever the board says
    let st: Value = fs::read_to_string(dot(&cfg.root).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let live: Vec<u64> = st["kids"].as_array().into_iter().flatten().filter_map(|k| k["task"].as_u64()).collect();
    let t = tidy::Tidy { board: b, root: &cfg.root, me: &cfg.me, boot: platform::boot_time, external: cfg.external,
        force: a.flags.contains_key("force"), dry: a.flags.contains_key("dry-run") };
    let lines = t.run(&mut |n| live.contains(&n), &|_| None).lines;
    if lines.is_empty() { vec!["nothing to clean".into()] } else { lines }
}

/// R6 snapshot of one project: the board read, with ready rows the board alone leaves undecided taking this host's
/// live admission from `status.json` (A3), and #1979 Decide from the collected reports (no board call).
/// #2291: one comment read per open task dominates `status`; 8 board clients read them in parallel. Any failure
/// leaves the task out of the cache, so the snapshot read fetches it as before.
fn prefetch_comments(c: &Ctx) -> service::CommentCache {
    let Ok(issues) = c.board.issues(None) else { return Default::default() };
    let now = c.board.now().unwrap_or(0);
    let todo: Vec<(u64, i64)> = issues.iter().filter(|i| i.trusted && !service::is_host(i) && (service::is_task(i) || service::is_held(i))).map(|i| (i.number, i.updated)).collect();
    if todo.is_empty() { return Default::default(); }
    let (cfg, schema) = (&c.cfg, c.raw["label_schema"].as_str());
    std::thread::scope(|s| {
        let handles: Vec<_> = todo.chunks(todo.len().div_ceil(8)).map(|part| s.spawn(move || {
            let Ok(b) = cfg.board.open(&cfg.web_host, &cfg.repo, schema, &platform::tool(&cfg.install)) else { return vec![] };
            part.iter().filter_map(|&(n, updated)| b.comments(n).ok().map(|cs| (n, (updated, now, cs)))).collect::<Vec<_>>()
        })).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

fn status_snapshot(c: &Ctx, a: &Args) -> (snapshot::Snapshot, Value) {
    let only = a.all("only").iter().flat_map(|x| x.split(',')).map(String::from).collect::<Vec<_>>();
    let mut snap = snapshot::read_cached(&*c.board, &c.cfg, &c.raw, &only, prefetch_comments(c));
    let local = dot(&c.cfg.root).join("status.json");
    if let Some(s) = data::json(&local).ok().flatten().filter(|_| snapshot::status_live(&local, c.cfg.ttl)) { snap.overlay_admission(&s); }
    (snap, reports::decide(&reports::cached(&c.cfg.root), reports::now()))
}

/// One project's text report and the hosts it saw; `t` gets each section's wall time (#2291 `--timing`).
fn status_text(c: &Ctx, a: &Args, read: Option<(snapshot::Snapshot, Value)>, t: &mut Vec<(&'static str, f64)>) -> Res<(String, Vec<(String, usize)>)> {
    let (b, cfg) = (c.board.as_ref(), &c.cfg);
    let mut clock = std::time::Instant::now();
    let mut lap = |t: &mut Vec<(&'static str, f64)>, name| { t.push((name, clock.elapsed().as_secs_f64())); clock = std::time::Instant::now(); };
    let local = dot(&cfg.root).join("status.json");
    let mut out = vec![];
    // R6: a board rate-limit pause the local service reports comes first, also when this read fails
    if read.is_none() {
        if let Some(w) = data::json(&local).ok().flatten().filter(|_| snapshot::status_live(&local, cfg.ttl)).as_ref().and_then(requests::warning) { out.push(w); }
    }
    let (snap, decide) = match read { Some(r) => r, None => {
        let snap = status_snapshot(c, a).0; lap(t, "board read");
        let decide = reports::decide(&reports::cached(&cfg.root), reports::now()); lap(t, "decide (cached)");
        (snap, decide)
    } };
    // R6 A3: an unreadable deploy wait or board quota is an anomaly row, never an `unknown` line
    let mut extra = vec![];
    // § 2 Deploy: closed tasks still waiting for their first deploy, a live board read
    let deploys = deploy::status(b, deploy::config(&cfg.root).as_ref()).unwrap_or_else(|e| {
        extra.push(format!("- deploy wait unreadable: {} · Next: rerun `taskq status`; check the board token", compact(&e, 160)));
        vec![]
    });
    lap(t, "deploy wait");
    let quota = requests::quota(&b.quota());
    lap(t, "quota");
    if cfg.board.core_budget && quota.starts_with("Board quota: unknown") {
        extra.push("- board quota unread: no core budget headers on this read · Next: rerun `taskq status`; a lasting gap is a board client defect".into());
    }
    out.push(snap.text_extra(cfg, &c.raw, Some(&decide), extra)?);
    lap(t, "render");
    out.extend(deploys);
    if !quota.starts_with("Board quota: unknown") { out.push(quota); }
    // § 6: the live service's open setup gaps follow the report
    out.extend(data::json(&local).ok().flatten().filter(|_| snapshot::status_live(&local, cfg.ttl)).map(|s| check::status_lines(&s["setup"])).unwrap_or_default());
    lap(t, "setup gaps");
    Ok((out.join("\n"), snap.served()))
}

/// R10: `taskq status` covers every project of `<install>/projects` plus the current one; `--project P..` narrows to
/// folder names or `owner/repo`. One project keeps the single-project command; several print one block each.
fn json_here_only(json: bool, wanted: &[String], here: bool) -> bool { json && wanted.is_empty() && here }

fn status_all(a: &Args) -> Res<()> {
    let here = find_root().ok();
    let mut roots = host::projects(&platform::default_install()).unwrap_or_default();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if let Some(h) = &here { roots.retain(|r| canon(r) != canon(h)); roots.insert(0, h.clone()); }
    let wanted: Vec<String> = a.all("project").iter().flat_map(|x| x.split([',', ' '])).filter(|x| !x.is_empty()).map(String::from).collect();
    // A JSON consumer inside a project (tray, PilotQ) reads that project's schema 1 snapshot, not a slow all-project read.
    if json_here_only(a.flags.contains_key("json"),&wanted,here.is_some()) { roots.truncate(1); }
    if !wanted.is_empty() {
        roots.retain(|r| wanted.iter().any(|w| *w == project_name(r) || load(r).is_ok_and(|(cfg, _)| cfg.repo == *w)));
        if roots.is_empty() { return Err(format!("--project {}: no listed project matches", wanted.join(" "))); }
    }
    let at = |root: &PathBuf| -> Res<Ctx> {
        let (cfg, raw) = load(root)?;
        let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
        Ok(Ctx { cfg, raw, board })
    };
    // R10 A1: hosts serving projects outside this view close the report as anomaly rows
    let unseen = || {
        let rows = snapshot::unseen(&SERVED.take());
        if !rows.is_empty() { println!("\nAnomalies:\n{}", rows.join("\n")); }
    };
    if roots.len() <= 1 {
        let c = match roots.first() { Some(r) => at(r)?, None => ctx()? };
        command(&c, "status", a)?;
        if !a.flags.contains_key("json") { unseen(); }
        return Ok(());
    }
    if a.flags.contains_key("json") {
        let mut served = vec![];
        let projects: Vec<Value> = roots.iter().map(|r| match at(r) {
            Ok(c) => {
                let (snap, decide) = status_snapshot(&c, a);
                served.push(snap.served());
                let mut v = snap.json();
                if let Ok(t) = snap.text_with(&c.cfg, &c.raw, Some(&decide)) { v["report"] = json!(t); }
                v["project"] = json!(project_name(r)); v["decide"] = decide; v
            }
            Err(e) => json!({"project": project_name(r), "complete": false, "reason": e}),
        }).collect();
        return json_output(&serde_json::to_string_pretty(&json!({"projects": projects, "unseen": snapshot::unseen(&served)})).map_err(|e| e.to_string())?, a.one("json-file"));
    }
    // #2291: projects read in parallel; each block prints in list order
    let timing = a.flags.contains_key("timing");
    let started = std::time::Instant::now();
    let blocks: Vec<(String, Vec<(&'static str, f64)>)> = std::thread::scope(|s| {
        let handles: Vec<_> = roots.iter().map(|r| s.spawn(move || {
            PROJECT_ROOT.with_borrow_mut(|p| *p = Some(r.clone()));
            let clock = std::time::Instant::now();
            let mut t = vec![];
            let c = at(r);
            t.push(("open", clock.elapsed().as_secs_f64()));
            // a project that cannot be read is an anomaly line of its own block, never a silent gap
            let text = match c.and_then(|c| status_text(&c, a, None, &mut t)) {
                Ok((text, served)) => { SERVED.with_borrow_mut(|v| v.push(served)); (text, SERVED.take()) }
                Err(e) => (format!("{} · source unavailable: {e} · Next: `taskq status --project {}` after fixing its config or board token", project_name(r), project_name(r)), vec![]),
            };
            (text, t)
        })).collect();
        handles.into_iter().map(|h| {
            let ((text, served), t) = h.join().unwrap_or_else(|_| (("status thread panicked".into(), vec![]), vec![]));
            SERVED.with_borrow_mut(|v| v.extend(served));
            (text, t)
        }).collect()
    });
    for (i, (text, _)) in blocks.iter().enumerate() {
        if i > 0 { println!(); }
        println!("{text}");
    }
    if timing {
        eprintln!("| Project | {} | total |", blocks.iter().flat_map(|b| b.1.iter().map(|x| x.0)).fold(Vec::<&str>::new(), |mut v, n| { if !v.contains(&n) { v.push(n); } v }).join(" | "));
        for (r, (_, t)) in roots.iter().zip(&blocks) {
            eprintln!("| {} | {} | {:.2} |", project_name(r), t.iter().map(|(_, s)| format!("{s:.2}")).collect::<Vec<_>>().join(" | "), t.iter().map(|x| x.1).sum::<f64>());
        }
        eprintln!("wall {:.2} s", started.elapsed().as_secs_f64());
    }
    PROJECT_ROOT.with_borrow_mut(|p| *p = None);
    unseen();
    Ok(())
}

fn command(c: &Ctx, cmd: &str, a: &Args) -> Res<()> {
    let (b, cfg, root) = (c.board.as_ref(), &c.cfg, c.cfg.root.as_path());
    let text = a.one("text").unwrap_or("");
    if matches!(cmd, "later" | "failed" | "steer") || cmd == "requeue" && dispatch::agent() || (cmd == "answer" && !a.pos.get(1).is_some_and(|p| p.contains('.'))) {
        if a.all("file").is_empty() || !text.trim().is_empty() { task_text(cmd, text)?; }
    }
    if cmd == "add" && a.flags.contains_key("later") { task_text("later", text)?; }
    if matches!(cmd, "ask" | "result" | "requeue" | "later" | "failed" | "steer" | "answer" | "pick") {
        let given = [a.all("option"), a.all("checks"), a.all("link")].concat();
        media_links(&[vec![text], given.iter().map(String::as_str).collect()].concat())?;
    }
    let revision = a.one("revision").map(|r| r.parse::<u64>().map_err(|_| format!("bad --revision {r}"))).transpose()?;
    let mut op = Op { id: a.one("op-id").map(String::from), revision };
    if op.id.as_deref().is_some_and(|i| i.is_empty() || i.contains(char::is_whitespace)) {
        return Err("--op-id: one word".into());
    }
    let json = a.flags.contains_key("json");
    let emit = |rs: &[Receipt], json: bool| {
        if let Some(path) = a.one("json-file") {
            json_output(&rs.iter().map(Receipt::json).collect::<Vec<_>>().join("\n"), Some(path))?;
            if rs.iter().all(Receipt::ok) { Ok(()) } else { Err(String::new()) }
        } else { crate::emit(rs, json) }
    };
    if revision.is_some() && (cmd == "close" && a.pos.len() > 2 || cmd == "answer" && a.pos.len() > 2) {
        return Err("--revision names one task".into());
    }
    match cmd {
        "reports"|"results" => {
            let value=match reports::read_cli(root,a)? {
                Some(value)=>value,
                None=>reports::act_with_reason(b,root,&c.raw,a.num(1)?,a.need("sha")?,a.one("action").unwrap_or("open"),a.one("text").unwrap_or(""),a.one("until"))?
            };
            println!("{value}"); Ok(())
        }
        "reroute" => {
            if dispatch::agent() { return Err("only the owner or PM can reroute strict task pins".into()); }
            emit(&reroute(b, root, a.need("from")?, a.need("to")?)?, json)
        }
        "labels" => {
            let mut names = labels::FIXED.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            names.extend(b.issues(None)?.into_iter().flat_map(|i| i.labels));
            b.ensure_labels(&names)?;
            println!("Label catalog verified for {}", project_name(root));
            Ok(())
        }
        "add" | "finding" => {
            let finding = cmd == "finding";
            if finding && a.one("conversation").is_some() { return Err("--conversation requires add".into()); }
            // § 1 task body: Goal, Acceptance, Scope, dependencies; limits checked before any write
            let title = a.pos.get(1).filter(|t| !t.trim().is_empty()).ok_or("title missing")?;
            let deps: Vec<u64> = a.all("deps").iter().flat_map(|d| d.split([',', ' '])).filter(|d| !d.is_empty()).map(|d| d.trim_start_matches('#').parse().map_err(|_| format!("bad --deps {d}"))).collect::<Res<_>>()?;
            // §4: only an explicit --runtime adds a binding pin; type never adds one.
            if a.flags.contains_key("later") {
                return Err("--later is retired: every add is held (§ 1); park it with `taskq later N`".into());
            }
            if a.flags.contains_key("prefer") {
                return Err("--prefer is retired; use --runtime only for an owner pin (none: host preference with eligible fallback)".into());
            }
            // a repeated single-value flag is refused before any write; `one` would keep only the last value
            let mut repeated: Vec<_> = a.flags.iter().filter(|(f, v)| v.len() > 1 && !["acceptance", "scope", "deps", "evidence"].contains(&f.as_str())).map(|(f, _)| format!("--{f}")).collect();
            repeated.sort();
            if !repeated.is_empty() {
                return Err(format!("{} given more than once; pass each once (a two-line --goal uses one value with a newline)", repeated.join(", ")));
            }
            let new = NewTask {
                conversation: a.one("conversation").map(String::from),
                title: title.clone(),
                goal: a.need("goal")?.to_string(),
                acceptance: a.all("acceptance"),
                scope: a.all("scope"),
                deps,
                kind: a.one("type").map(String::from),
                runtime: a.one("runtime").map(String::from),
                priority: a.one("priority").map(String::from),
                host: add_host(a.one("host"), cfg),
                labels: if a.flags.contains_key("gate") { vec!["needs-gate".into()] } else { vec![] },
                rules: a.one("rules").map(String::from),
            };
            if finding {
                task_body(b, &new, &Op::default()).map_err(|e| serve_hint(e, cfg))?;
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
                let evidence: Vec<_> = a.all("evidence").into_iter().map(String::from).collect();
                let bullets: Vec<_> = evidence.iter().map(|e| format!("- {e}")).collect();
                one_line("--evidence", &bullets)?;
                limit("Evidence", &bullets, 0, 4, 240)?;
                let (create, start) = submit_finding(b, &c.raw, root, cfg.board.name, &cfg.web_host, &cfg.repo, &new, source, &evidence, &mut op, a.flags.contains_key("start"))
                    .map_err(|e| serve_hint(e, cfg))?;
                let create = if create.ok() { create.wake(root) } else { create };
                return emit_finding(create, start, json);
            } else if ["start", "source", "board", "evidence"].iter().any(|f| a.flags.contains_key(*f)) {
                return Err("--start/--source/--board/--evidence require finding".into());
            }
            let r = add_task(b, root, &c.raw, &new, &op).map_err(|e| serve_hint(e, cfg))?;
            let r = if r.ok() && a.flags.contains_key("gate") { gate_task(b, root, r.task, &op) } else { r };
            emit(&[r], json)
        }
        "gate" => emit(&[gate_task(b, root, a.num(1)?, &op)], json),
        "start" => emit(&start_tasks(b, root, &a.pos[1..], &op)?, json),
        "approve" => emit(&[dispatch::approve(b, root, a.num(1)?, a.one("sha"), &op, dispatch::agent())], json),
        "recipe" => {
            let name = a.pos.get(1).ok_or("recipe name missing")?;
            emit(&run_recipe(b, root, &c.raw, name, a.need("run")?, a.one("goal"), a.flags.contains_key("start"))?, json)
        }
        "ask" => {
            let n = a.num(1)?;
            let (links, images, files) = (a.all("link"), a.all("image"), a.all("file"));
            if links.iter().any(|l| l.starts_with('!') || media(l).is_some() || attachment(l)) { return Err("--link takes links; images and videos go through --image, files through --file".into()); }
            // validate the card and the files before any upload
            ask_card(n, a.need("text")?, &a.all("option"), a.one("recommend"), &[links.clone(), file_stubs(b, &files)?].concat())?;
            image_files(&images)?;
            let body = ask_card(n, a.need("text")?, &a.all("option"), a.one("recommend"), &[links, upload_attachments(b, n, &files)?, upload_images(b, n, &images)?].concat())?;
            emit(&[act(b, root, n, "ask", &[], &body, &op)], json)
        }
        "answer" if a.pos.get(1).is_some_and(|p| p.contains('.')) => {
            emit(&answer_picks(b, cfg, &a.pos[1..], &op)?, json)
        }
        "pick" => emit(&[dispatch::pm_pick(b, root, a.pos.get(1).ok_or("N.K missing")?, text, &op)?], json),
        "answer" => {
            if a.all("file").is_empty(){emit(&dispatch::answer_text(b, cfg, a.num(1)?, a.need("text")?, &op)?, json)}
            else{emit(&[dispatch::act_files(b,root,a.num(1)?,cmd,text,&op,&a.all("file"))],json)}
        }
        "result" => {
            let n = a.num(1)?;
            let sha = a.need("sha")?;
            let (links, images, files) = (a.all("link"), a.all("image"), a.all("file"));
            if links.iter().any(|l| l.starts_with('!') || media(l).is_some() || attachment(l)) { return Err("--link takes links; images and videos go through --image, files through --file".into()); }
            result_text(n, text, &a.all("checks"), &a.all("option"), a.one("recommend"), &[links.clone(), file_stubs(b, &files)?].concat())?;
            image_files(&images)?;
            let report = reports::prepare(b, root, n, sha)?;
            let mut body = result_text(n, text, &a.all("checks"), &a.all("option"), a.one("recommend"), &[links, upload_attachments(b, n, &files)?, upload_images(b, n, &images)?].concat())?;
            if report {body+=&format!("\nReport: docs/research/{n}.md");if body.lines().filter(|l|!l.trim().is_empty()).count()>12 {return Err("report result exceeds 12 lines; shorten the verdict".into());}}
            emit(&[act(b, root, n, "result", &[sha], &body, &op)], json)
        }
        "requeue" => {
            let n = a.num(1)?;
            emit(&[dispatch::act_files(b,root,n,cmd,text,&op,&a.all("file"))],json)
        }
        "later" => {
            let n = a.num(1)?;
            let staged = a.all("stage");
            let now = board::stamp(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64));
            let args = park(n, a.one("blocker"), a.one("until"), (!staged.is_empty()).then_some(now.as_str()), std::env::var("TASKQ_TASK").ok(), std::env::var("TASKQ_ATTEMPT").ok())?;
            let text = if staged.is_empty() { text.to_string() } else { stage_text(text, &staged, &b.issue(n)?.body)? };
            let mut out = vec![act(b, root, n, cmd, &args, &text, &op)];
            // § 7 rule 3 (#2288): the worker's held local blocker starts with its park, so neither waits for the PM
            if let Some(m) = a.one("blocker").filter(|_| out[0].ok()).and_then(|r| held_blocker(b, r)) {
                out.extend(start_tasks(b, root, &[m.to_string()], &Op::default())?);
            }
            emit(&out, json)
        }
        "steer" | "failed" => {
            let n = a.num(1)?;
            emit(&[dispatch::act_files(b,root,n,cmd,if a.all("file").is_empty(){a.need("text")?}else{text},&op,&a.all("file"))],json)
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
                        json!({"transcript":closed_transcript(b,&cfg.root,n,&cfg.me,before,|host,attempt|conversations::closed_remote(&cfg.install,&cfg.root,host,n,attempt,before)).map_or_else(|reason|json!({"reason":reason}),|v|v)})
                    } else { reports::selected(&cfg.root,b,n,!cfg.board.closed_reason,None)? }
                } else { closed_history(b,page,since,!cfg.board.closed_reason)? };
                println!("{value}");
                return Ok(());
            }
            let (snap, decide) = status_snapshot(c, a);
            if a.flags.contains_key("json") {
                let mut v = snap.json();
                if let Ok(t) = snap.text_with(cfg, &c.raw, Some(&decide)) { v["report"] = json!(t); }
                v["decide"] = decide;
                json_output(&serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?, a.one("json-file"))?;
            } else {
                let (text, served) = status_text(c, a, Some((snap, decide)), &mut vec![])?;
                SERVED.with_borrow_mut(|v| v.push(served));
                println!("{text}");
            }
            Ok(())
        }
        "cleanup" => {
            needs_checkout(root, "cleanup")?;
            for l in cleanup_lines(b, cfg, a) { println!("{l}"); }
            Ok(())
        }
        "issue" => {
            let n = a.num(1)?;
            if a.flags.iter().any(|(k, v)| !["comment", "label", "op-id", "json"].contains(&k.as_str()) || v.len() != 1) {
                return Err("issue: use --comment or --label, --op-id and --json once each".into());
            }
            let write = match (a.one("comment"), a.one("label")) {
                (None, None) if op.id.is_none() => { println!("{}", board::issue_json(b, n)?); return Ok(()); }
                (Some(t), None) => board::IssueWrite::Comment(t),
                (None, Some(l)) => board::IssueWrite::Label(l),
                _ => return Err("issue: pass one of --comment or --label to write".into()),
            };
            let r = board::issue_write(b, n, write, op.id.as_deref());
            let ok = r["outcome"] == "accepted";
            if json { println!("{r}"); }
            else if ok { println!("#{n} {}", if r["label"].is_string() { format!("label {}", r["label"].as_str().unwrap_or("")) } else { format!("comment {}", r["comment"]) }); }
            else { eprintln!("Error: #{n}: {}\nNext: {}", compact(r["reason"].as_str().unwrap_or(""), 153), compact(r["next"].as_str().unwrap_or(""), 154)); }
            if ok { Ok(()) } else { Err(String::new()) }
        }
        "deployed" => {
            if a.flags.iter().any(|(k, v)| !["version", "env", "range", "host", "json"].contains(&k.as_str()) || v.len() != 1) {
                return Err("deployed: use --version, --env, --range, optionally --host and --json, once each".into());
            }
            let need = |k: &str| a.one(k).ok_or(format!("deployed: --{k} is required"));
            let v = deploy::run(b, &cfg.root, need("version")?, need("env")?, need("range")?, a.one("host"))?;
            if a.flags.contains_key("json") { println!("{v}"); return Ok(()); }
            for r in v["tasks"].as_array().into_iter().flatten() {
                let reason = r["reason"].as_str().filter(|s| !s.is_empty()).map_or(String::new(), |s| format!(": {s}"));
                println!("#{} {}{reason}", r["issue"], r["outcome"].as_str().unwrap_or_default());
            }
            for r in v["skipped"].as_array().into_iter().flatten() {
                println!("skipped {}: {}", r["url"].as_str().unwrap_or_default(), r["reason"].as_str().unwrap_or_default());
            }
            if v["tasks"].as_array().is_none_or(|t| t.is_empty()) && v["skipped"].as_array().is_none_or(|t| t.is_empty()) { println!("no Task: trailers in the range"); }
            if v["tasks"].as_array().into_iter().flatten().any(|r| r["outcome"] == "unknown") { return Err("deployed: some entries are unknown; rerun the same command".into()); }
            Ok(())
        }
        "claim-issue" => {
            if a.flags.iter().any(|(k, v)| k != "json" || v.len() != 1) { return Err("claim-issue: only --json, once".into()); }
            if std::env::var_os("TASKQ_ATTEMPT").is_some() { return Err("a worker cannot change create_claim_issue".into()); }
            let adopt = if a.pos.len() > 1 { Some(a.num(1)?) } else { None };
            let r = claim_issue_setup(b, root, adopt)?;
            let ok = r["outcome"] == "accepted";
            if json { println!("{r}"); }
            else if ok { println!("#{} create_claim_issue {} ({})", r["issue"], r["url"].as_str().unwrap_or(""), r["source"].as_str().unwrap_or("")); }
            else { eprintln!("Error: {}\nNext: {}", compact(r["reason"].as_str().unwrap_or(""), 153), compact(r["next"].as_str().unwrap_or(""), 154)); }
            if ok { Ok(()) } else { Err(String::new()) }
        }
        "inbox" => {
            if a.flags.contains_key("json") { println!("{}", inbox_json(b, root, &cfg.me, a.num(1)?)?); } else { println!("{}", inbox(b, root, a.num(1)?)?); }
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
        match linux_sandbox::run(&std::env::args().skip(1).collect::<Vec<_>>()) {
            Ok(code) => crate::platform::exit(code),
            Err(e) => { eprintln!("{e}"); crate::platform::exit(1); }
        }
    }
    incident::install_panic_hook();
    #[allow(unused_mut)]
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `<install>/taskq` is the launcher: it runs the current release (coreq.md § Launcher).
    let install = platform::default_install();
    let exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());
    let t = platform::tool(&install);
    // #2049 A1: Windows tasks start the GUI launcher `<install>/taskqw.exe`; it runs every command windowless.
    let gui = exe.as_deref().and_then(Path::file_name).is_some_and(|f| f.eq_ignore_ascii_case(t.gui_exe_name()));
    if exe.as_deref().is_some_and(|e| t.is_launcher(e)) && t.current().is_some() {
        if gui { coreq::launcher::launch(&t, &args); }
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
            let status = platform::console_command(t.release_exe(&v)).args(&args).env(t.env("INSTALL"), &t.install).status();
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
    requests::flush(&requests::source(&args, std::env::var("TASKQ_ROLE").ok().as_deref()));
    if let Err(e) = r {
        if e.is_empty() {
            crate::platform::exit(1);
        }
        platform::die(&e);
    }
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
#[path = "main_tests.rs"]
pub(crate) mod tests;
