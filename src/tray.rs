//! `taskq tray` (taskq.md § 4): the host's one menu-bar / notification-area icon over the `<root>/.taskq/status.json`
//! and `events` of every project in `<install>/projects`. A click opens `tray.html` in a wry webview; Rust pushes one
//! JSON state, the page sends actions back. No board access and no token: actions go through the same files and
//! commands as the CLI.

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Window, WindowBuilder};
use tray_icon::{Icon, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::{WebView, WebViewBuilder};

use crate::icon::{self, Rgba, compact};
use crate::notify;

const HTML: &str = include_str!(concat!(env!("OUT_DIR"),"/web/tray.html"));
const WORKSPACE_HTML: &str = include_str!(concat!(env!("OUT_DIR"),"/web/workspace.html"));
const FONTS: [u64; 4] = [14, 16, 18, 20];
const PERIODS: [(&str, i64); 5] = [("5m", 300), ("1h", 3600), ("1d", 86400), ("1w", 604800), ("1mo", 2592000)];

/// False when another tray wrote the heartbeat `<install>/tray` in the last 10 s (it ticks every 2 s): of two trays
/// started at once, the first to see the other's pid exits.
fn alone(install: &Path) -> bool {
    let hb = install.join("tray");
    let other = fs::read_to_string(&hb).ok().and_then(|p| p.trim().parse::<u32>().ok()).is_some_and(|p| p != std::process::id());
    !(other && crate::file_age(&hb).is_some_and(|a| a < 10))
}

enum Ev {
    Tick,
    Ipc(String),
    Workspace(String, String),
    WorkspaceEvaluated(String, u64, &'static str),
    Evaluated(u64, &'static str),
    Click(Rect),
    Shot,
    Shot2,
    /// Add project: the generation fences a late picker or install result.
    Added(u64, Value),
    Answer(Value),
    /// A notification click: its request identifier (M14)
    Note(String),
}

/// View, text size and period of `<install>/tray.json`; anything else falls back to the default. A
/// `custom` period is `[start, end)` in epoch seconds with the time zone `local` or `UTC` it was typed in. View
/// `full` is Summary; a legacy `custom` view resolves to Summary.
#[derive(Clone, PartialEq)]
struct Settings {
    view: String,
    font: u64,
    period: String,
    start: i64,
    end: i64,
    tz: String,
    pin: Pin,
    /// Notification modes and cursors (`notify.rs`), kept as saved
    notify: Value,
    navigation: Value,
    workspace_navigation: Value,
}

/// Pinned window (menu.md M13): pinned or not, the last display and the last top-left per display, physical pixels.
#[derive(Clone, PartialEq)]
struct Pin {
    pinned: bool,
    top: bool,
    size: Option<(f64, f64)>,
    size_anchor: String,
    attached_edge: String,
    auto_anchor: String,
    display: String,
    pos: std::collections::BTreeMap<String, (i32, i32)>,
}

impl Default for Pin {
    fn default() -> Self {
        Self { pinned: false, top: true, size: None, size_anchor: "auto".into(), attached_edge: "top".into(), auto_anchor: "top".into(), display: String::new(), pos: Default::default() }
    }
}

impl Settings {
    fn load(install: &Path) -> Settings {
        // § 4 Data: a corrupt tray.json shows the defaults with its reason; the file stays until a save keeps a copy
        let v = crate::data::json(&install.join("tray.json")).unwrap_or_else(|e| {
            eprintln!("{e}; defaults shown");
            None
        }).unwrap_or_default();
        let pos = v["pos"].as_object().into_iter().flatten().filter_map(|(k, p)| Some((k.clone(), (i32::try_from(p[0].as_i64()?).ok()?, i32::try_from(p[1].as_i64()?).ok()?)))).collect();
        let size = v["size"][0].as_f64().zip(v["size"][1].as_f64()).filter(|(w, h)| w.is_finite() && h.is_finite() && *w >= 240.0 && *h >= 120.0);
        let size_anchor = v["size_anchor"].as_str().filter(|v| ["auto","fixed","bottom","top"].contains(v)).unwrap_or(if size.is_some() {"fixed"} else {"auto"}).to_string();
        let pin = Pin { size_anchor, attached_edge: "top".into(), auto_anchor: v["auto_anchor"].as_str().filter(|v| ["top","bottom"].contains(v)).unwrap_or("top").into(), top: v["top"].as_bool().unwrap_or(true), size, pinned: v["pinned"] == true, display: v["display"].as_str().unwrap_or("").into(), pos };
        let mut s = Settings { view: "icon".into(), font: 16, period: "1d".into(), start: 0, end: 0, tz: "local".into(), pin,
            navigation: Value::Null, workspace_navigation: Value::Null, notify: if v["notify"].is_object() { v["notify"].clone() } else { Value::Null } };
        s.set(&v);
        s
    }

    fn set(&mut self, v: &Value) {
        // M8: the web module validates route keys against the current registered snapshot.
        if v["navigation"].is_object() && v["navigation"]["schema"] == 1 && v["navigation"].to_string().len() <= 65536 {
            self.navigation = v["navigation"].clone();
        }
        if v["workspace_navigation"].is_object() && v["workspace_navigation"]["schema"] == 1 && v["workspace_navigation"].to_string().len() <= 65536 {
            self.workspace_navigation = v["workspace_navigation"].clone();
        }
        if let Some(x) = v["view"].as_str().filter(|x| ["icon", "full", "custom"].contains(x)) {
            self.view = if x == "custom" { "full" } else { x }.into();
        }
        if let Some(x) = v["font"].as_u64().filter(|x| FONTS.contains(x)) {
            self.font = x;
        }
        if let Some(x) = v["period"].as_str().filter(|x| PERIODS.iter().any(|p| p.0 == *x)) {
            self.period = x.into();
        }
        // a custom range with start >= end is refused (menu.md M6)
        if let (Some("custom"), Some(a), Some(b)) = (v["period"].as_str(), v["start"].as_i64(), v["end"].as_i64())
            && a < b
        {
            (self.period, self.start, self.end) = ("custom".into(), a, b);
            self.tz = if v["tz"] == "UTC" { "UTC" } else { "local" }.into();
        }
    }

    /// `[start, end)` of the period at `now`.
    fn range(&self, now: i64) -> (i64, i64) {
        if self.period == "custom" {
            return (self.start, self.end);
        }
        rolling(now, PERIODS.iter().find(|p| p.0 == self.period).map_or(86400, |p| p.1))
    }

    fn save(&self, install: &Path) -> bool {
        let fields = json!({"view": self.view, "font": self.font, "period": self.period, "start": self.start, "end": self.end, "tz": self.tz,
            "pinned": self.pin.pinned, "top": self.pin.top, "size": self.pin.size, "size_anchor": self.pin.size_anchor, "auto_anchor": self.pin.auto_anchor, "display": self.pin.display, "pos": self.pin.pos, "notify": self.notify, "navigation": self.navigation, "workspace_navigation": self.workspace_navigation});
        let p = install.join("tray.json");
        let mut v = crate::data::json(&p).ok().flatten().filter(Value::is_object).unwrap_or_else(||json!({}));
        v.as_object_mut().unwrap().remove("icon");
        for (key,value) in fields.as_object().unwrap() { v[key]=value.clone(); }
        if crate::data::json(&p).is_err() && fs::copy(&p, install.join("tray.json.corrupt")).is_err() {
            return false;
        }
        crate::platform::write_atomic(&p, &v.to_string()).is_ok()
    }
}

/// Draft and history of AI requests (§ 4): `<install>/tray-requests.json`, apart from the view settings.
use crate::conversations::preferences as requests_file_checked;
fn requests_file(install: &Path) -> Value {
    requests_file_checked(install).unwrap_or_else(|e|json!({"error":e,"draft":"","history":[]}))
}

/// The last 20 distinct sent texts, newest first; the same text moves to the top (menu.md M7).
fn remember(history: &[Value], text: &str) -> Vec<Value> {
    std::iter::once(json!(text)).chain(history.iter().filter(|h| h.as_str() != Some(text)).cloned()).take(20).collect()
}

/// AI requests (§ 4): Send opens one new session in a registered project through the setup path.
fn ai(install: &Path, a: &str, m: &Value) -> Result<(), String> {
    let mut f = requests_file_checked(install)?;
    match a {
        "ai-draft" => {
            f["draft"] = json!(m["text"].as_str().unwrap_or(""));
            f["root"] = m["root"].clone();
            if m["local"].is_object() { f["local"] = m["local"].clone(); }
        },
        "ai-clear" => f["history"] = json!([]),
        "ai-send" => {
            // only a registered project: the page never names a path of its own
            let root = m["root"].as_str().filter(|r| roots(install).iter().any(|x| x == r)).ok_or("Choose a registered project")?;
            let text = m["text"].as_str().unwrap_or("");
            let id=m["id"].as_str().ok_or("Owner request ID missing")?;
            crate::setup::open_id(install, Path::new(root), text, id, m["runtime"].as_str().unwrap_or(crate::runtime::OWNER),m["reconcile"]==true)?;
            f["history"] = json!(remember(f["history"].as_array().map_or(&[][..], |v| v), text.trim()));
            f["draft"]=json!("");
        }
        _ => return Ok(()),
    }
    crate::platform::write_atomic(&install.join("tray-requests.json"), &f.to_string()).map_err(|e| e.to_string())
}

/// Workspace card drafts/operation identity use the existing native request preferences, never a browser outbox.
fn answer_local(install: &Path, key: &str, local: &Value) -> Result<(), String> {
    if key.len()>2048 || !local.is_object() || !local["draft"].is_string() || local.to_string().len()>65536 {
        return Err("Workspace draft is invalid or too large; not saved".into());
    }
    let mut f=requests_file_checked(install)?;
    if !f["answers"].is_object(){f["answers"]=json!({});}
    f["answers"][key]=local.clone();
    crate::platform::write_atomic(&install.join("tray-requests.json"),&f.to_string()).map_err(|e|format!("Workspace draft was not saved: {e}"))
}

fn owner_runtime(ps: &[Value]) -> Value {
    let probe=ps.iter().map(|p|&p["agents"]).filter(|a|a["at"].as_i64().is_some_and(|at|(0..=360).contains(&(now()-at)))).max_by_key(|a|a["at"].as_u64());
    let row=probe.and_then(|a|a["rows"].as_array()).and_then(|rows|rows.iter().find(|r|r["runtime"]==crate::runtime::OWNER));
    let reason=match row {
        Some(r) if r["login"]=="logged in" => "",
        Some(r) if r["login"]=="not logged in" => "Claude Code is not logged in; inspect Agents in the tray.",
        _ => "Claude Code owner-session readiness is unknown; refresh or inspect Agents in the tray.",
    };
    json!({"id":crate::runtime::OWNER,"name":crate::runtime::of(crate::runtime::OWNER).client,"available":reason.is_empty(),"reason":reason})
}

fn roots(install: &Path) -> Vec<String> {
    fs::read_to_string(install.join("projects")).unwrap_or_default().lines().filter(|l| !l.is_empty()).map(String::from).collect()
}

/// Owner answers and tray-only fallback actions cross this bridge; the CLI owns credentials, dispatch and board readback.
fn answer_args(install: &Path, m: &Value) -> Result<Vec<String>, String> {
    let root = m["root"].as_str().ok_or("project missing")?;
    if !roots(install).iter().any(|r| r == root) { return Err("project is not registered".into()); }
    if m["board"].as_str().is_none_or(str::is_empty) { return Err("board identity missing".into()); }
    let op = &m["op"];
    let id = op["id"].as_str().filter(|id| !id.is_empty() && !id.contains(char::is_whitespace)).ok_or("operation id must be one word")?;
    let n = op["task"].as_u64().filter(|n| *n > 0).ok_or("task number missing")?;
    let revision = op["revision"].as_u64().ok_or("question revision missing")?;
    let action = op.get("action").map(|v| v.as_str().ok_or("invalid action")).transpose()?;
    let control = crate::snapshot::CONTROLS.iter().find(|c| Some(c.0) == action);
    let mut args: Vec<String> = if let Some((name, command, text, _)) = control {
        if op.get("choice").is_some() || op.get("text").is_some() { return Err(format!("{name} takes no choice or text")); }
        let mut args = vec![command.to_string(), n.to_string()];
        if !text.is_empty() { args.extend(["--text".into(), text.to_string()]); }
        args
    } else if let Some(action) = action {
        if op.get("choice").is_some() { return Err("fallback actions cannot include a choice".into()); }
        let (command, text) = match action {
            "requeue" => {
                let reason = op["text"].as_str().filter(|s| !s.trim().is_empty() && !s.contains(['\r', '\n'])).ok_or("supply one requeue reason line")?;
                ("requeue", format!("Reason: {reason}"))
            }
            "close" if op["confirmed"] == true && op.get("text").is_none() => ("later", "Reason: Owner confirmed cancellation without publication from the tray.".into()),
            _ => return Err("unsupported or unconfirmed tray action".into()),
        };
        crate::task_text(command, &text)?;
        vec![command.into(), n.to_string(), "--text".into(), text]
    } else {
        let mut args = vec!["answer".into()];
        match (op.get("choice"), op.get("text")) {
            (Some(choice), None) => {
                let k = choice.as_u64().filter(|k| *k > 0).ok_or("invalid choice")?;
                args.push(format!("{n}.{k}"));
            }
            (None, Some(text)) => {
                let text = text.as_str().ok_or("invalid answer text")?;
                crate::task_text("answer", text)?;
                args.extend([n.to_string(), "--text".into(), text.into()]);
            }
            _ => return Err("supply one choice or literal text".into()),
        }
        args
    };
    let id = if action == Some("close") { format!("{id}.later") } else { id.into() };
    args.extend(["--json".into(), "--op-id".into(), id, "--revision".into(), revision.to_string()]);
    Ok(args)
}

fn owner_command(root: &Path) -> Result<std::process::Command, String> {
    let mut c = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    c.current_dir(root).env("TASKQ_ROOT", root).env_remove("TASKQ_TASK").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROLE");
    Ok(c)
}

fn answer_snapshot(s: &Value, m: &Value) -> Result<(), String> {
    if s["complete"] != true { return Err(s["reason"].as_str().unwrap_or("board read incomplete").into()); }
    if s["board"]["url"] != m["board"] { return Err("board identity changed; refresh the project".into()); }
    let op = &m["op"];
    let row = s["tasks"].as_array().into_iter().flatten().find(|r| r["n"] == op["task"]);
    let row = row.ok_or("task is not in the current board snapshot; open it on the board")?;
    // #1476: a started draft may already read ready/waiting; the CLI returns the op id's first receipt or refuses.
    if op["action"] == "start" {
        return if ["held", "ready", "waiting"].contains(&row["state"].as_str().unwrap_or("")) { Ok(()) } else { Err("task is not a held draft; check the board".into()) };
    }
    if let Some(action) = op["action"].as_str().filter(|a| crate::snapshot::CONTROLS.iter().any(|c| c.0 == *a)) {
        // An old revision still reaches dispatch: the CLI replays the op id's first receipt or refuses it as stale.
        if row["revision"] != op["revision"] || crate::snapshot::controls(row["state"].as_str().unwrap_or("")).contains(&action) { return Ok(()); }
        return Err(format!("{action} is not offered in this state; check the board"));
    }
    if op.get("action").is_some() {
        if row["revision"] != op["revision"] { return Err("question changed; refresh before acting".into()); }
        if op["action"] == "close" && row["state"] == "later" { return Ok(()); } // only a matching later op can succeed again
        if row["state"] != "ask" || row["question"]["choices"].as_array().is_some_and(|c| !c.is_empty()) {
            return Err("fallback actions require an ask without choices; check the board for partial cancellation".into());
        }
        return Ok(());
    }
    if !row["revision"].is_u64() { return Err("question revision unavailable".into()); }
    // An old revision still reaches dispatch so uncertain delivery can reconcile its op id.
    if row["revision"] == op["revision"] {
        if row["question"]["parsed"] != true { return Err("question is read-only; open it on the board".into()); }
        if let Some(choice) = op.get("choice") {
            let c = row["question"]["choices"].as_array().into_iter().flatten().find(|c| c["id"] == *choice).ok_or("choice not found")?;
            if c["consequential"] == true { return Err("close/publication requires review on the board".into()); }
        }
    }
    Ok(())
}

// Chain: one CLI receipt per step. Close never continues after unknown/failed parking or from another state.
fn answer_dispatch(m: &Value, args: &[String], mut run: impl FnMut(&[String]) -> Value) -> Value {
    let checked = |r: Value, id: &str| {
        if r["task"] == m["op"]["task"] && r["op_id"] == id
            && matches!(r["outcome"].as_str(), Some("accepted" | "stale" | "rejected" | "unknown")) { r }
        else { json!({"outcome": "unknown", "reason": "No matching CLI receipt", "next": "Check the board; retry the same operation id."}) }
    };
    let id = args[args.len() - 3].as_str();
    let mut r = checked(run(args), id);
    if m["op"]["action"] == "start" && r["outcome"] == "accepted" && !matches!(r["state"].as_str(), Some("ready" | "waiting")) {
        r["outcome"] = json!("unknown");
        r["reason"] = json!("Start did not read back ready or waiting");
        r["next"] = json!("Check the board; retry the same start to reconcile delivery.");
    }
    if m["op"]["action"] != "close" || r["outcome"] != "accepted" || r["state"] == "closed" { return r; }
    if r["state"] != "later" || !r["revision"].is_u64() {
        return json!({"outcome": "rejected", "reason": "Cancellation stopped: parking was not read back as later with a revision", "next": "Check the task on the board."});
    }
    let id = format!("{}.close", m["op"]["id"].as_str().unwrap());
    let close = vec!["close".into(), m["op"]["task"].to_string(), "--text".into(),
        "Changed: Cancelled by the owner without publication.\nChecks: Tray cancellation -- later read back before close.\nOpen: none".into(),
        "--json".into(), "--op-id".into(), id.clone(), "--revision".into(), r["revision"].to_string()];
    let mut r = checked(run(&close), &id);
    if r["outcome"] == "accepted" && r["state"] != "closed" {
        r["outcome"] = json!("unknown");
        r["reason"] = json!("Close receipt did not read back closed");
    }
    if r["outcome"] != "accepted" {
        r["reason"] = json!(format!("Parked; close not verified: {}", r["reason"].as_str().unwrap_or("unknown")));
        r["next"] = json!("Check the board; retry the same cancellation to reconcile delivery.");
    }
    r
}

fn answer(install: &Path, m: &Value) -> Value {
    let response = |state: &str, reason: &str, next: &str| json!({"answerReceipt": {"root": m["root"], "id": m["op"]["id"], "state": state, "reason": reason, "next": next}});
    let prepare = || -> Result<_, String> {
        let args = answer_args(install, m)?;
        let root = Path::new(m["root"].as_str().unwrap());
        let out = crate::platform::output("tray.answer-status",owner_command(root)?.args(["status", "--json"]),Duration::from_secs(30))?;
        let snapshot = serde_json::from_slice(&out.stdout).map_err(|_| String::from_utf8_lossy(&out.stderr).into_owned())?;
        answer_snapshot(&snapshot, m)?;
        Ok((root, args))
    };
    let (root, args) = match prepare() {
        Ok(c) => c,
        Err(e) => return response("rejected", &e, "Refresh the question or open it on the board."),
    };
    let r = answer_dispatch(m, &args, |args| {
        let out = owner_command(root).and_then(|mut c| crate::platform::output("tray.answer-command",c.args(args),Duration::from_secs(60)));
        match out {
            Ok(out) => serde_json::from_slice(&out.stdout).unwrap_or_else(|_| json!({"task": m["op"]["task"], "op_id": args[args.len()-3], "outcome": "unknown", "reason": String::from_utf8_lossy(&out.stderr), "next": "Check the board; retry the same operation id."})),
            Err(e) => json!({"task": m["op"]["task"], "op_id": args[args.len()-3], "outcome": "unknown", "reason": e, "next": "Check the board; retry the same operation id."}),
        }
    });
    let reason = r["reason"].as_str().filter(|s| !s.is_empty()).or_else(|| r["state"].as_str()).unwrap_or("");
    response(r["outcome"].as_str().unwrap_or("unknown"), reason, r["next"].as_str().unwrap_or(""))
}

/// One opaque revision of a policy: equal content, equal revision.
/// `ponytail:` a content hash, so an edit-and-revert reads as unchanged; the host adapter's revisions replace it.
fn revision(v: &Value) -> Value {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.to_string().hash(&mut h);
    json!(format!("r{:016x}", h.finish()))
}

/// Host policy and occupancy are distinct from this project's narrowing ceilings.
fn resource_group(cap: &Value, saved: &Value, saved_revision:&Value, project_limits: &Value, observed_at: Option<i64>, installation: Option<String>, project: &str) -> Value {
    let host = crate::service::host_capacity(cap).unwrap_or(Value::Null);
    let ok = host.is_object();
    let saved = if saved["error"].is_null() { saved } else { &Value::Null };
    let target = |p: bool, rt: Option<&str>| json!({"installation_id": installation, "generation": 1, "project_id": if p { json!(project) } else { Value::Null }, "runtime_id": rt});
    let entry = |t: Value, current: Value, effective: Value| {let project=!t["project_id"].is_null();let runtime=!t["runtime_id"].is_null();json!({"target": t, "service_version": crate::platform::VERSION,
        "supported": [{"policy_schema": "taskq.host-capacity/1", "modes": [], "fields": if project||saved.is_null() {json!([])}else if runtime {json!(["runtime_limit"])}else{json!(["maximum","load_gate"])}, "scopes": if project {json!([])}else if runtime {json!(["runtime"])}else{json!(["host"])} }],
        "current_revision": if saved.is_null() { Value::Null } else if project {revision(&current)}else{saved_revision.clone()},
        "effective_revision": if !ok {Value::Null}else if project {revision(&effective)}else{cap["host_command"].clone()},
        "current_policy": current, "effective_policy": effective,
        "sample": null, "runtime_usage": [], "blockers": [], "completeness": if ok { "complete" } else { "unknown" }})};
    let pol = |c: &Value| json!({"maximum": c["admission"]["max"], "load_gate": c["load_gate"]});
    let mut h = entry(target(false, None), pol(saved), pol(&host));
    let mut rts: Vec<String> = crate::service::RUNTIMES.iter().map(|rt| rt.to_string()).collect();
    for m in [&saved["limits"], &host["limits"], &host["used"]] {
        for k in m.as_object().into_iter().flatten().map(|(k, _)| k) { if !rts.contains(k) { rts.push(k.clone()); } }
    }
    h["runtime_usage"] = json!(rts.iter().map(|rt| {
        let l = if ok { host["limits"][rt].as_u64() } else { saved["limits"][rt].as_u64() };
        let kind = if !ok && saved.is_null() { "unknown" } else { match l { Some(0) => "disabled", Some(_) => "ceiling", None => "unbounded" } };
        json!({"runtime_id": rt, "used": host["used"][rt].as_u64(), "limit": l, "limit_kind": kind})
    }).collect::<Vec<_>>());
    if ok && host["sample_at"].is_i64() {
        h["sample"] = json!({"source": format!("taskq-admission-{}", if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" }),
            "sampled_at": host["sample_at"], "age_seconds": host["sample_age_seconds"], "cadence_seconds": 30, "window_seconds": 60,
            "cpu_busy_percent": host["cpu_percent"], "memory_available_percent": host["memory_available_percent"], "memory_available_gib": null, "pressure": null});
    }
    h["blockers"] = crate::resources::blockers(&host, |host_scope| target(!host_scope, None));
    let mut targets = vec![h, entry(target(true, None), json!({"limits": project_limits}), json!({"limits": cap["project_limits"]}))];
    for rt in &rts {
        let lim = |c: &Value| json!({"runtime_limit": c["limits"][rt]});
        targets.push(entry(target(false, Some(rt)), lim(saved), lim(&host)));
    }
    json!({"schema": "taskq.resources/1", "observed_at": observed_at, "targets": targets})
}

/// One project for the page: its `status.json`, saved focus, labels and `events` lines `[time, kind, attempt, task]`.
fn project(install: &Path, root: &str) -> Value {
    let r = Path::new(root);
    let d = crate::dot(r);
    // § 4 Data: a corrupt status.json is unknown with its reason, never zero
    let (s, bad) = match crate::data::json(&d.join("status.json")) {
        Ok(s) => (s.unwrap_or(Value::Null), None),
        Err(e) => (Value::Null, Some(e)),
    };
    let age = crate::file_age(&d.join("status.json"));
    // #1591: one freshness rule; the page gets `live` and `ttl`, never a literal
    let ttl = crate::snapshot::ttl(r);
    let live = age.is_some_and(|a| crate::snapshot::live(a as i64, ttl));
    let saved = fs::read_to_string(d.join("focus.paused")).is_ok_and(|f| !matches!(f.trim(), "" | "off"));
    let text = fs::read_to_string(d.join("events")).unwrap_or_default();
    let since = text.lines().next().and_then(|l| l.strip_prefix("since ")?.parse::<i64>().ok());
    let events: Vec<Value> = text.lines().skip(1).filter_map(|l| {
        let w: Vec<&str> = l.split(' ').collect();
        Some(json!([w.first()?.parse::<i64>().ok()?, w.get(2)?, w.get(3)?, w.get(1)?.strip_prefix('#')?.parse::<u64>().ok()?]))
    }).collect();
    // the local host's saved capacity, for `Edit limits` and its readback (§ 4 Capacity)
    let (cap_host, cap_saved, cap_revision,board_kind, project_limits) = match crate::load(r) {
        Ok((c, raw)) => {let policy=crate::host::Policy::read(install);let rev=policy.as_ref().ok().and_then(|p|p.revision().ok());let host=policy.as_ref().map(|p|json!(p.me)).unwrap_or(Value::Null);
            (host, policy.map(|p|p.cap.json()).unwrap_or_else(|e| json!({"error": e})),json!(rev), json!(c.board.name), if raw["limits"].is_null() {json!({})} else {raw["limits"].clone()})},
        Err(e) => (Value::Null, json!({"error": e}), Value::Null,Value::Null, Value::Null),
    };
    let mut tasks = s["tasks"].as_array().cloned().unwrap_or_default();
    let (hosts, tasks_live) = liveness(&s, age, ttl, &mut tasks);
    let page = crate::load(r).ok().and_then(|(c, _)| crate::enrollment::progress_page(install, &c.repo));
    for task in &mut tasks { task_session(r, cap_host.as_str().unwrap_or(""), page.as_deref(), task); }
    let installation = fs::read_to_string(install.join("installation-id")).ok().filter(|i| i.len() == 64 && i.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    let id: String = crate::project_name(r).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(64).collect();
    let observed_at=s["at"].as_i64().filter(|at|crate::snapshot::live(now()-*at,ttl)).and(age.map(|a|now()-a as i64));
    let resources = resource_group(&s["capacity"], &cap_saved,&cap_revision, &project_limits, observed_at, installation, &id);
    json!({"root": root, "name": s["project"].as_str().map(String::from).unwrap_or_else(|| crate::project_name(r)),
        "issues": s["issues"].as_str().unwrap_or(""), "board_kind": board_kind,
        "delivery": crate::load(r).ok().map(|(c,_)|json!({"preset":c.delivery,"revision":c.delivery_revision})),
        "log": s["log"].is_string(), "alive": live, "age": age, "ttl": ttl,
        "complete": crate::snapshot::complete(&s) && live,
        "error": bad.map(Value::from).or_else(|| [&s["error"], &s["focus_error"]].into_iter().find(|e| e.is_string()).cloned()).unwrap_or(Value::Null), "focus": s["focus"].as_array().cloned().unwrap_or_default(), "saved": saved, "labels": crate::status_labels(r),
        "focus_command": s["focus_command"],"service_pid":s["pid"],"host_instance":s["host_instance"],"restart_waits":s["restart_waits"],"stop_progress":s["stop_progress"],"uninstall_review":crate::lifecycle::uninstall_review(install,"review",Some(r)).unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})), "request": s["request"], "since": since, "events": events, "tasks": tasks, "host": s["host"], "version": s["version"], "update": s["update"],
        "hosts": hosts, "observed": s["observed"], "tasks_age_seconds": s["tasks_age_seconds"], "tasks_live": tasks_live, "capacity": s["capacity"], "resources": resources, "cap_host": cap_host, "cap_saved": cap_saved, "agents": s["agents"],
        "incident": s["incident"], "incident_line": s.get("incident").map(crate::incident::line), "incident_notice": s["incident_notice"],
        "incident_repo": crate::platform::RELEASE_REPO})
}

/// #1591: the page gets each observation's live state from the one Rust rule. Task, host and execution rows age by
/// the board read (`tasks_age_seconds`) plus the `status.json` file age `age`.
fn liveness(s: &Value, age: Option<u64>, ttl: i64, tasks: &mut [Value]) -> (Value, bool) {
    let elapsed = age.zip(s["tasks_age_seconds"].as_i64()).map(|(a, t)| a as i64 + t);
    let row_live = |row: &mut Value, at: &str| if let Some(o) = row.as_object_mut() {
        let l = elapsed.zip(s["observed"].as_i64()).zip(o.get(at).and_then(Value::as_i64)).is_some_and(|((e, obs), at)| crate::snapshot::live(obs - at + e, ttl));
        o.insert("live".into(), json!(l));
    };
    let mut hosts = s["hosts"].clone();
    for h in hosts.as_array_mut().into_iter().flatten() { row_live(h, "updated"); }
    for x in tasks.iter_mut().filter_map(|t| t.get_mut("executions").and_then(Value::as_array_mut)).flatten() { row_live(x, "observed"); }
    (hosts, elapsed.is_some_and(|e| crate::snapshot::live(e, ttl)))
}

fn now() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Native adapter (M3): a Claude URL is account-scoped, so any host links it (owning host: its transcript, else the
/// owner's heartbeat via the snapshot); a Codex thread opens only on its owning host, elsewhere its DashQ progress page
/// (`page` + task number, R6 3), else unavailable with a reason.
fn task_session(root: &Path, local: &str, page: Option<&str>, row: &mut Value) {
    let Some(object) = row.as_object_mut() else { return };
    let published = object.remove("session_url").and_then(|u| u.as_str().filter(|u| crate::runtime::remote_url(u)).map(String::from));
    object.remove("session_reason");
    object.insert("local_session".into(), json!(false));
    let session = row["session"].as_str().unwrap_or("").to_string();
    let words: Vec<&str> = session.split_whitespace().collect();
    let Some((rt, id)) = words.first().and_then(|s| s.split_once(':')) else { return };
    let host = words.get(1).copied().unwrap_or("");
    let own = !local.is_empty() && host == local;
    row["runtime"] = json!(rt);
    row["host"] = json!(host);
    row["local_session"] = json!(own);
    let a = crate::runtime::of(rt);
    if !a.remote && !own {
        if a.codex {
            match page {
                Some(p) => row["session_url"] = json!(format!("{p}{}", row["n"].as_u64().unwrap_or(0))),
                None => row["session_reason"] = json!(format!("Codex session opens only on {host}")),
            }
        }
        return;
    }
    let id = if own && ((id == "-" && a.codex && a.child) || a.remote) {
        let l = crate::runtime::Launch { execution: None, task: row["n"].as_u64().unwrap_or(0), title: String::new(), attempt: row["attempt"].as_u64().unwrap_or(0),
            role: row["role"].as_str().unwrap_or("").into(), rt: rt.into(), session: id.into(), url: String::new(), resume: None };
        crate::runtime::codex_thread(&crate::runtime::log_path(root, &l)).unwrap_or_else(|| id.into())
    } else { id.into() };
    if id == "-" || !crate::runtime::plain_thread(&id) { return }
    row["session_url"] = if a.codex { json!(crate::runtime::thread_url(&id)) }
        else if a.remote { json!(own.then(|| crate::runtime::claude_remote(&id)).flatten().or(published)) }
        else { Value::Null };
}

/// What the icon and the history show: in work, `ask`, ready, failed over all projects (one task per board and issue), whether
/// any project admits, six equal bins of `[start, end)` (oldest first) counting `KINDS` and the latest `since` of the
/// projects that have history (None: none has).
#[derive(Clone)]
struct Summary {
    /// work, ask, ready, failed
    counts: [usize; 4],
    blocked: usize,
    /// Every project's snapshot is complete and fresh; else the counts are unknown, never zero (§ 4).
    known: bool,
    run: bool,
    bins: Vec<icon::Bin>,
    since: Option<i64>,
}

/// Exact event count order (M6); only closed controls column height.
const KINDS: [&str; 4] = ["close", "failed", "ask", "blocked"];

fn summary(ps: &[Value], (start, end): (i64, i64)) -> Summary {
    summary_at(ps, (start, end), now())
}

fn summary_at(ps: &[Value], (start, end): (i64, i64), now:i64) -> Summary {
    let mut seen = std::collections::HashMap::new();
    for p in ps {
        for t in p["tasks"].as_array().into_iter().flatten() {
            seen.insert(format!("{}/{}", p["issues"].as_str().unwrap_or(""), t["n"]), t["state"].as_str().unwrap_or("").to_string());
        }
    }
    let mut bins:Vec<icon::Bin> = (0..6).map(|i| {
        let a=start as f64+(end as f64-start as f64)*i as f64/6.0;
        let b=start as f64+(end as f64-start as f64)*(i+1) as f64/6.0;
        let mut missing:Vec<String>=ps.iter().filter(|p|p["since"].as_i64().is_none_or(|t|t as f64>a)).map(|p|p["name"].as_str().or(p["issues"].as_str()).unwrap_or("unknown source").to_string()).collect();
        if a>now as f64 {missing.push("future interval".into());}
        icon::Bin {start:a,end:b,counts:[0;4],missing,ongoing:a<=now as f64&&(now as f64)<b}
    }).collect();
    let mut ev = std::collections::HashSet::new();
    for p in ps {
        for e in p["events"].as_array().into_iter().flatten() {
            let (t, kind) = (e[0].as_i64().unwrap_or(0), e[1].as_str().unwrap_or(""));
            let Some(k) = KINDS.iter().position(|x| *x == kind) else { continue };
            if end > start && t >= start && t < end && ev.insert(format!("{} {} {kind}", p["issues"], e[2])) {
                bins[(((t as i128 - start as i128) * 6 / (end as i128 - start as i128)) as usize).min(5)].counts[k] += 1;
            }
        }
    }
    let since = ps.iter().filter_map(|p| p["since"].as_i64()).max();
    let t = crate::snapshot::totals(seen.values().map(String::as_str));
    let known = ps.iter().all(|p| p["complete"] == true);
    Summary { blocked: t["waiting"], counts: [t["doing"] + t["review"], t["ask"], t["ready"], t["failed"]], known, run: ps.iter().any(|p| p["focus"].as_array().is_some_and(|f| !f.is_empty())), bins, since }
}

struct QueuedAction {
    id: String,
    root: PathBuf,
    message:Value,
    context: Option<coreq::telemetry::capture::TraceContext>,
    dispatch: tracing::Dispatch,
}
struct ActionQueue {
    sender: std::sync::mpsc::SyncSender<QueuedAction>,
    worker: std::thread::JoinHandle<()>,
    allowed:&'static [&'static str],
    pending: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
}
enum AddJob { Pick(QueuedAction), Install(QueuedAction, Value) }
const ADD_IDLE:u8=0;
const ADD_RUNNING:u8=1;
const ADD_UNAVAILABLE:u8=2;
struct AddOwner { sender:std::sync::mpsc::SyncSender<AddJob>,state:std::sync::Arc<std::sync::atomic::AtomicU8>,_worker:std::thread::JoinHandle<()> }
impl AddOwner {
    fn start(install:PathBuf,proxy:tao::event_loop::EventLoopProxy<Ev>)->crate::Res<Self> {
        let (sender,receiver)=std::sync::mpsc::sync_channel(1);let state=std::sync::Arc::new(std::sync::atomic::AtomicU8::new(ADD_IDLE));let released=state.clone();
        let worker=std::thread::Builder::new().name("tray.add.execute".into()).spawn(move ||while let Ok(job)=receiver.recv() {
            let (job,adding)=match job {AddJob::Pick(job)=>(job,None),AddJob::Install(job,adding)=>(job,Some(adding))};let generation=job.message["generation"].as_u64().unwrap_or(0);
            let operation=if adding.is_some() {"tray.install.execute"} else {"tray.picker.execute"};
            tracing::dispatcher::with_default(&job.dispatch,|| {let _context=job.context.as_ref().map(|c|c.span().entered());
                let (mut result,state)=add_outcome(operation,||match adding {
                    Some(adding)=>install_service(&install,adding),
                    None=>match pick() {Ok(Some(dir))=>classify(&dir,selftest),Ok(None)=>json!({"state":"cancelled"}),Err(reason)=>json!({"state":"pick failed","reason":reason})},
                });
                result["generation"]=json!(generation);
                released.store(state,std::sync::atomic::Ordering::Release);
                let _=coreq::telemetry::capture::operation("tray.add.report",||proxy.send_event(Ev::Added(generation,result)),|_|"ui-closed");
            });
        }).map_err(|e|format!("Cannot start add worker: {e}"))?;
        Ok(Self{sender,state,_worker:worker})
    }
    fn submit(&self,generation:u64,adding:Option<Value>)->crate::Res<()> {
        coreq::telemetry::capture::operation("tray.add.dispatch",|| {
        if self._worker.is_finished() {self.state.store(ADD_UNAVAILABLE,std::sync::atomic::Ordering::Release);return Err("Add project worker stopped; prior result unknown; do not retry".into());}
        let action=if adding.is_some() {"add-install"} else {"add"};
        let job=QueuedAction {id:format!("add-{generation}"),root:PathBuf::new(),message:json!({"a":action,"generation":generation}),context:coreq::telemetry::capture::TraceContext::current(),dispatch:tracing::dispatcher::get_default(Clone::clone)};
        add_submit(&self.sender,&self.state,if let Some(adding)=adding {AddJob::Install(job,adding)} else {AddJob::Pick(job)})
        },|_|"add-admission")
    }
}
fn add_outcome(operation:&'static str,work:impl FnOnce()->Value)->(Value,u8) {
    let result=coreq::telemetry::capture::operation(operation,||match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) if matches!(value["state"].as_str(),Some("pick failed"|"install failed"))=>Err((value,false)),
        Ok(value)=>Ok(value),
        Err(_)=>Err((json!({"state":"add failed","reason":"Add worker panicked; outcome unknown; verify the prior picker or installation before restarting the tray"}),true)),
    },|(_,panicked)|if *panicked {"add-panic"}else{"add-failed"});
    match result {Ok(value)=>(value,ADD_IDLE),Err((value,panicked))=>(value,if panicked {ADD_UNAVAILABLE}else{ADD_IDLE})}
}
fn add_submit(sender:&std::sync::mpsc::SyncSender<AddJob>,state:&std::sync::Arc<std::sync::atomic::AtomicU8>,job:AddJob)->crate::Res<()> {
    match state.compare_exchange(ADD_IDLE,ADD_RUNNING,std::sync::atomic::Ordering::AcqRel,std::sync::atomic::Ordering::Acquire) {Ok(_)=>{},Err(ADD_UNAVAILABLE)=>return Err("Add project worker unavailable; prior result unknown; do not retry".into()),Err(_)=>return Err("Add project already in progress; wait for the picker or install to finish".into())};
    sender.try_send(job).map_err(|e| {state.store(if matches!(e,std::sync::mpsc::TrySendError::Disconnected(_)) {ADD_UNAVAILABLE} else {ADD_IDLE},std::sync::atomic::Ordering::Release);match e {std::sync::mpsc::TrySendError::Full(_)=>"Add project queue full",std::sync::mpsc::TrySendError::Disconnected(_)=>"Add project worker unavailable; prior result unknown; do not retry"}.into()})
}
impl ActionQueue {
    fn start<E>(install: PathBuf,allowed:&'static [&'static str],registered:bool,execute:E,report: impl Fn(PathBuf, String, crate::Res<Value>) + Send + 'static) -> crate::Res<Self>
    where E: Fn(&Path,&Path,&Value)->crate::Res<Value> + Send + 'static {
        let (sender, receiver)=std::sync::mpsc::sync_channel::<QueuedAction>(16);
        let pending=std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
        let completed=pending.clone();
        let operation=if allowed.contains(&"capacity") {"tray.capacity.execute"}else{"tray.focus.execute"};
        let worker=std::thread::Builder::new().name(operation.into()).spawn(move || {
            while let Ok(job)=receiver.recv() {
                let operation=match job.message["a"].as_str() {Some("answer")=>"tray.answer.execute",Some("resources")=>"tray.resources.execute",Some("recovery-read")=>"tray.recovery.read",Some("recovery-apply")=>"tray.recovery.apply",Some("restart")=>"tray.restart.execute",Some("stop")=>"tray.stop.execute",Some("uninstall")=>"tray.uninstall.execute",_=>operation};
                tracing::dispatcher::with_default(&job.dispatch,|| {
                    let _context=job.context.as_ref().map(|c|c.span().entered());
                    let result=coreq::telemetry::capture::operation(operation,||std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if registered&&!crate::host::project_paths(&install)?.iter().any(|r|r==&job.root) {
                            return Err("Project is no longer registered; command not applied".into());
                        }
                        execute(&install,&job.root,&job.message)
                    })).unwrap_or_else(|_|Err("Command panicked; result unknown; check service status before retry".into())),|_|"command-failed");
                    report(job.root.clone(),job.id.clone(),result);
                    completed.lock().unwrap_or_else(|e|e.into_inner()).remove(&format!("{}#{}",job.root.display(),job.id));
                });
            }
        }).map_err(|e|format!("Cannot start command worker: {e}"))?;
        Ok(Self{sender,worker,allowed,pending})
    }
    fn submit(&self,message:&Value) -> crate::Res<()> {
        let action=message["a"].as_str().unwrap_or("");let id=message["id"].as_str().unwrap_or("");
        let name=match action {"answer"=>"tray.answer.dispatch","resources"=>"tray.resources.dispatch","recovery-read"|"recovery-apply"=>"tray.recovery.dispatch","pause"=>"tray.pause.dispatch","resume"=>"tray.resume.dispatch","focus"=>"tray.focus.dispatch","capacity"=>"tray.capacity.dispatch","restart"=>"tray.restart.dispatch","stop"=>"tray.stop.dispatch","uninstall"=>"tray.uninstall.dispatch",_=>"tray.invalid.dispatch"};
        coreq::telemetry::capture::operation(name,|| {
            if !self.allowed.contains(&action) {return Err(("invalid-command","Unsupported queued command"));}
            if id.is_empty() || id.len()>64 || !id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'.')) {
                return Err(("invalid-id","Invalid command correlation id; action not submitted"));
            }
            let mut frozen=message.clone();frozen.as_object_mut().map(|m|m.remove("context"));
            let bytes=serde_json::to_vec(&frozen).map_err(|_|("request-too-large","Command payload too large; action not submitted"))?;
            if bytes.len()>20<<10 {return Err(("request-too-large","Command payload too large; action not submitted"));}
            let root=message["root"].as_str().filter(|root|!root.is_empty()).ok_or(("invalid-root","Project root missing; action not submitted"))?;
            if self.worker.is_finished() {return Err(("worker-unavailable","Command worker unavailable; action not submitted"));}
            let key=format!("{root}#{id}");let mut pending=self.pending.lock().unwrap_or_else(|e|e.into_inner());
            if let Some(existing)=pending.get(&key) {
                return if existing==&bytes {Ok(())} else {Err(("id-mismatch","Correlation id is already pending for a different request"))};
            }
            pending.insert(key.clone(),bytes);
            if let Err(error)=self.sender.try_send(QueuedAction {id:id.into(),root:root.into(),message:message.clone(),
                context:coreq::telemetry::capture::TraceContext::current(),dispatch:tracing::dispatcher::get_default(Clone::clone)})
            {
                pending.remove(&key);
                return Err(match error {
                    std::sync::mpsc::TrySendError::Full(_)=>("queue-full","Command queue full; action not submitted"),
                    std::sync::mpsc::TrySendError::Disconnected(_)=>("worker-unavailable","Command worker unavailable; action not submitted"),
                });
            }
            Ok(())
        },|(code,_)|*code).map_err(|(_,message)|message.into())
    }
}

fn queued_focus(_install:&Path,root:&Path,message:&Value)->crate::Res<Value> {
    let receipt=act(message["a"].as_str().unwrap_or(""),root,message["label"].as_str())?.ok_or("Command confirmation unavailable; check service status")?;
    Ok(json!({"applied_receipt":receipt}))
}
fn queued_answer(install:&Path,_root:&Path,message:&Value)->crate::Res<Value> {Ok(answer(install,message))}
fn queued_resources(_install:&Path,_root:&Path,_message:&Value)->crate::Res<Value> {resources()}
fn answer_reply(root:PathBuf,id:String,result:crate::Res<Value>)->Value {result.unwrap_or_else(|error|json!({"answerReceipt":{"root":root,"id":id,"state":"unknown","reason":error,"next":"Check delivery with the same operation"}}))}
fn resources_reply(result:crate::Res<Value>)->Value {match result {Ok(_)=>json!({"resources_receipt":{"state":"submitted","reason":"Resources terminal submitted"}}),Err(reason)=>json!({"resources_receipt":{"state":"unknown","reason":reason}})}}

// The page owns asynchronous work; a submission result cannot acknowledge its execution.
struct WebCalls {
    workspace: bool,
    surface: String,
    next: u64,
    pending: std::collections::BTreeMap<u64, (Instant, tracing::Span)>,
    counts: [u64; 4],
    saturated: bool,
    failed: bool,
    notified: bool,
    metadata_unknown: bool,
}
impl WebCalls {
    fn new() -> Self {
        let mut calls=Self{workspace:false,surface:"main".into(),next:0,pending:Default::default(),counts:[0;4],saturated:false,failed:false,notified:false,metadata_unknown:false};
        calls.begin("tray.web.boot",Instant::now());
        calls
    }
    fn begin(&mut self, name: &'static str, now: Instant) -> Option<u64> {
        if self.pending.len()>=64 || self.next==u64::MAX {self.fault("pending-limit");return None;}
        let id=self.next;self.next+=1;
        // #1548: an unchanged tick push folds into one aggregate per window; a failed one keeps its rows.
        let span=if name==TICK {tracing::info_span!(parent: None,"operation",coreq_operation=name,coreq_repeating=true,coreq_result="unclassified",coreq_error=tracing::field::Empty)}
            else {coreq::telemetry::capture::detached_span(name)};
        self.pending.insert(id,(now,span));
        Some(id)
    }
    fn fault(&mut self, reason: &'static str) {
        if !self.failed {
            self.failed=true;
            eprintln!("taskq tray: interface outcomes unknown ({reason}); check service status before retry");
            let _=coreq::telemetry::capture::operation("tray.web.delivery",||Err::<(),_>(reason),|code|*code);
        }
    }
    fn finish(&mut self, id: u64, result: &'static str) {
        let Some((_,span))=self.pending.remove(&id) else {return};
        match result {
            "ok" => {span.record("coreq_result","observed");}
            "script-error"|"submit-error" => {
                span.record("coreq_result","observed");span.record("coreq_error",result);self.fault(result);
            }
            _ => self.fault(result), // no callback is not proof that the script failed or did not run
        }
    }
    fn expire(&mut self, now: Instant) {
        let expired:Vec<_>=self.pending.iter().filter(|(_, (start,_))|now.duration_since(*start)>=Duration::from_secs(30)).map(|(id,_)|*id).collect();
        for id in expired {self.finish(id,"ack-timeout");}
    }
    fn metadata(&mut self, row:&Value)->bool {
        let value=match web_observation(row){Ok(value)=>value,Err(_)=>{self.fault("invalid-web-observation");return false;}};
        // #1297: CoreQ folds JS summaries only inside a marked root; delivery is asynchronous to the refresh that caused it.
        let _repeating=tracing::info_span!("tray.web.ingest",coreq_repeating=true).entered();
        if let Err(error)=coreq::telemetry::capture::observe_linked(value)&&!self.metadata_unknown {
            self.metadata_unknown=true;
            eprintln!("taskq tray: activity history incomplete ({error}); command confirmations remain separate");
            return true;
        }
        false
    }
    fn errors(&mut self, message: &Value) {
        let Some(values)=message["counts"].as_array().filter(|v|v.len()==4) else {self.fault("invalid-error-report");return;};
        let mut counts=[0;4];
        for (i,value) in values.iter().enumerate() {
            let Some(n)=value.as_u64().filter(|n|*n>=self.counts[i] && *n<=u32::MAX as u64) else {self.fault("invalid-error-report");return;};
            counts[i]=n;
        }
        let Some(saturated)=message["saturated"].as_bool() else {self.fault("invalid-error-report");return;};
        for (i,class) in ["script-error","unhandled-rejection","resource-error","evaluation-error"].iter().enumerate() {
            let delta=counts[i]-self.counts[i];
            if delta>0 {
                // Page-wide counts have no reliable initiating click; do not invent a parent.
                let span=coreq::telemetry::capture::detached_span("tray.web.error");
                if !span.is_disabled() {tracing::info!(parent:&span,calls=delta);}
                span.record("coreq_result","observed");span.record("coreq_error",*class);
            }
        }
        self.counts=counts;
        if saturated && !self.saturated {
            self.saturated=true;
            let _=coreq::telemetry::capture::operation("tray.web.error-counts",||Err::<(),_>("count-saturated"),|code|*code);
            self.fault("error-count-saturated");
        }
        else if counts.iter().any(|n|*n>0) {self.fault("page-error");}
    }
}

// Accept only metadata named by the actual bundled modules; the WebView supplies no content fields.
fn web_observation(row: &Value) -> crate::Res<Value> {
    let object=row.as_object().ok_or("invalid-web-observation")?;
    if row.to_string().len()>4096||object.keys().any(|key|!matches!(key.as_str(),"kind"|"source"|"operation"|"trace"|"span"|"parent"|"started_ms"|"duration_us"|"outcome"|"error"|"counts")) {return Err("invalid-web-observation".into());}
    static INVENTORY:std::sync::OnceLock<Vec<String>>=std::sync::OnceLock::new();
    let inventory=INVENTORY.get_or_init(||serde_json::from_str(include_str!(concat!(env!("OUT_DIR"),"/web/operations.json"))).expect("build-owned web inventory"));
    if !inventory.iter().any(|name|row["operation"].as_str()==Some(name)) {return Err("unknown-web-operation".into());}
    let (kind,source)=match row["kind"].as_str() {
        Some("ui.start")=>("method.start","action"),Some("ui")=>("method","action"),
        Some("method")=>("method","diagnostic"),Some("method.aggregate")=>("method.aggregate","aggregate"),
        Some("effect.start")=>("method.start","effect"),Some("effect")=>("method","effect"),
        Some("effect.wait")=>("effect.wait","effect"),Some("collection")=>("collection","web"),
        Some("diagnostic")=>("diagnostic","web"),_=>return Err("invalid-web-kind".into()),
    };
    if !matches!(row["outcome"].as_str(),Some("ok"|"failed"|"unknown"))||!matches!(row["error"].as_str(),Some("none"|"javascript-error"|"unowned-async"|"completion-timeout"|"collection-incomplete")) {return Err("invalid-web-outcome".into());}
    if let Some(counts)=row.get("counts") {
        let counts=counts.as_object().ok_or("invalid-web-counts")?;
        if counts.len()>6||counts.iter().any(|(key,value)|!matches!(key.as_str(),"calls"|"finished"|"failed"|"unknown"|"lost"|"pending")||value.as_u64().is_none_or(|n|n>9007199254740991)) {return Err("invalid-web-counts".into());}
    }
    let mut value=row.clone();
    value["product"]=json!("taskq");value["version"]=json!(crate::platform::VERSION);value["process_id"]=json!(std::process::id());
    value["kind"]=json!(kind);value["source"]=json!(source);
    let mut unknown=serde_json::Map::new();
    for key in ["trace","span","parent"] {
        if value[key].is_null() {value[key]=Value::Null;unknown.insert(key.into(),json!("aggregate-unlinked"));}
        else if value[key].as_str().is_none_or(|id|coreq::telemetry::capture::TraceContext::decode(&format!("1 {id} {id}")).is_err()) {return Err("invalid-web-context".into());}
    }
    if !unknown.is_empty(){value["unknown"]=Value::Object(unknown);}
    coreq::telemetry::Event::new(value.clone())?;
    Ok(value)
}

fn web_result(value: &str) -> &'static str {
    match value {"\"ok\""=>"ok","\"script-error\""=>"script-error",_=>"invalid-ack"}
}
fn web_script(script: &str) -> String {
    // eval catches syntax errors too; the callback contains only a fixed result code.
    format!("(()=>{{try{{(0,eval)({});return 'ok';}}catch(_){{try{{window.taskqWebFault('evaluation');}}catch(_){{}}return 'script-error';}}}})()",json!(script))
}
const TICK: &str = "tray.web.tick";
/// #1548 fixture: one steady tray tick's own rows (push, its JS render summary, the fit resize); the host hour test owns the gate.
#[cfg(test)]
pub(crate) fn steady_tick() -> impl FnMut() {
    let mut calls=WebCalls::new();calls.pending.clear();
    move || {
        let id=calls.begin(TICK,Instant::now()).expect("tick push");calls.finish(id,"ok");
        let render=json!({"operation":"js.ui.render.tick","kind":"method.aggregate","started_ms":crate::profile::now_ms().unwrap(),"counts":{"calls":1,"finished":1,"failed":0,"unknown":0},"outcome":"ok","error":"none"});
        assert!(!calls.metadata(&render)&&!calls.failed);
        let _=coreq::telemetry::capture::poll("tray.window.resize",0,||Ok::<_,&str>(()),|_|"observed");
    }
}
fn web_eval(wv: &WebView, script: &str, name: &'static str, proxy: &tao::event_loop::EventLoopProxy<Ev>, calls: &mut WebCalls) {
    if calls.pending.contains_key(&0) {calls.fault("page-not-ready");return;}
    let Some(id)=calls.begin(name,Instant::now()) else {return};
    let p=proxy.clone();
    let workspace=calls.workspace;let surface=calls.surface.clone();
    if wv.evaluate_script_with_callback(&web_script(script),move |value| {let _=p.send_event(if workspace {Ev::WorkspaceEvaluated(surface.clone(),id,web_result(&value))} else {Ev::Evaluated(id,web_result(&value))});}).is_err() {
        calls.finish(id,"submit-error");
    }
}

pub fn run(install: PathBuf, snapshot: Option<PathBuf>, routes: Vec<String>) -> Result<(), String> {
    if snapshot.is_none() && !alone(&install) {
        return Err("taskq tray already runs on this host".into());
    }
    if let Some(r) = routes.iter().find(|r| r.as_str() != "icon" && !ROUTES.contains(&r.as_str())) {
        return Err(format!("unknown route {r}; known: icon, {}", ROUTES.join(", ")));
    }
    if snapshot.is_none(){crate::agentq::start(&install);}
    let agentq_shots=snapshot.is_some() && routes.iter().any(|r|r.starts_with("agentq-"));
    let mut settings = Settings::load(&install);
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // Accessory keeps the tray out of the Dock; AppKit menu accelerators enable native text editing.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(if settings.pin.pinned && snapshot.is_none() { ActivationPolicy::Regular } else { ActivationPolicy::Accessory });
        // a snapshot never takes the owner's focus (taskq.md R12)
        event_loop.set_activate_ignoring_other_apps(snapshot.is_none());
        if snapshot.is_none() {
            use objc2::{MainThreadOnly,sel};
            use objc2_foundation::{MainThreadMarker,NSString};
            use objc2_app_kit::{NSApplication,NSMenu,NSEventModifierFlags as Mod};
            let mtm=MainThreadMarker::new().ok_or("tray text editing requires the main thread")?;
            let app=NSApplication::sharedApplication(mtm);
            let menu=app.mainMenu().unwrap_or_else(||NSMenu::new(mtm));
            let edit=NSMenu::initWithTitle(NSMenu::alloc(mtm),&NSString::from_str("Edit"));
            for (title,action,key,shift) in [
                ("Undo",sel!(undo:),"z",false),("Redo",sel!(redo:),"z",true),
                ("Cut",sel!(cut:),"x",false),("Copy",sel!(copy:),"c",false),
                ("Paste",sel!(paste:),"v",false),("Select All",sel!(selectAll:),"a",false),
            ] {
                let item=unsafe {edit.addItemWithTitle_action_keyEquivalent(&NSString::from_str(title),Some(action),&NSString::from_str(key))};
                item.setKeyEquivalentModifierMask(Mod::Command|if shift {Mod::Shift}else{Mod::empty()});
            }
            let item=unsafe {menu.addItemWithTitle_action_keyEquivalent(&NSString::from_str("Edit"),None,&NSString::from_str(""))};
            item.setSubmenu(Some(&edit));app.setMainMenu(Some(&menu));
        }
    }
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { rect, button_state: MouseButtonState::Up, .. } = e {
            let _ = proxy.send_event(Ev::Click(rect));
        }
    }));
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        while proxy.send_event(Ev::Tick).is_ok() {
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    let proxy = event_loop.create_proxy();
    let replies=proxy.clone();
    let focus_actions=ActionQueue::start(install.clone(),&["pause","resume","focus"],true,queued_focus,move |root,id,result| {
        let mut reply=result.unwrap_or_else(|error|json!({"text":error}));reply["root"]=json!(root);reply["id"]=json!(id);
        let _=coreq::telemetry::capture::operation("tray.focus.report",||replies.send_event(Ev::Answer(json!({"focus_receipt":reply}))),|_|"ui-closed");
    })?;
    let replies=proxy.clone();
    let capacity_actions=ActionQueue::start(install.clone(),&["capacity","restart","stop","uninstall","recovery-read","recovery-apply"],true,capacity,move |root,id,result| {
        let mut reply=result.unwrap_or_else(|error|json!({"state":"unknown","reason":error}));reply["root"]=json!(root);reply["id"]=json!(id);
        let _=coreq::telemetry::capture::operation("tray.capacity.report",||replies.send_event(Ev::Answer(json!({"capmsg":reply}))),|_|"ui-closed");
    })?;
    let replies=proxy.clone();
    let answer_actions=ActionQueue::start(install.clone(),&["answer"],true,queued_answer,move |root,id,result| {
        let receipt=answer_reply(root,id,result);
        let _=coreq::telemetry::capture::operation("tray.answer.report",||replies.send_event(Ev::Answer(receipt)),|_|"ui-closed");
    })?;
    let replies=proxy.clone();
    let resources_actions=ActionQueue::start(install.clone(),&["resources"],false,queued_resources,move |_root,_id,result| {
        let receipt=resources_reply(result);
        let _=coreq::telemetry::capture::operation("tray.resources.report",||replies.send_event(Ev::Answer(receipt)),|_|"ui-closed");
    })?;
    let add_actions=AddOwner::start(install.clone(),proxy.clone())?;
    let mut ui: Option<(Window, WebView)> = None;
    let mut workspace: Option<(Window, WebView)> = None;
    let mut workspace_web: Option<WebCalls> = None;
    let mut detached:std::collections::BTreeMap<String,(Window,WebView,WebCalls)>=Default::default();
    let mut workspace_handoff=false;
    let mut workspace_open=Value::Null;
    let mut tray: Option<TrayIcon> = None;
    let (mut shown, mut icon_sig, mut dark) = (String::new(), String::new(), true);
    let mut workspace_shown=String::new();
    let mut hidden_at = Instant::now() - Duration::from_secs(1);
    let mut shown_at = hidden_at;
    let mut anchor: Option<Rect> = None;
    let mut step = 0usize;
    // snapshot pages wider than their window (#1073): `tray --snapshot` fails naming them
    let mut overflow: Vec<String> = Vec::new();
    let mut capture_errors:Vec<String>=Vec::new();
    let mut adding = Value::Null;
    let mut add_generation=0u64;
    let mut moved = false;
    let mut asked = false;
    let mut bursts = notify::Bursts::new();
    let mut shot_wait=None;
    let mut web=WebCalls::new();
    event_loop.run(move |event, target, flow| {
        crate::telemetry::diagnostic::poll(&install);
        *flow = ControlFlow::Wait;
        let native=match &event {
            Event::WindowEvent{event:WindowEvent::Resized(_),..}=>Some("tray.window.resize"),
            Event::WindowEvent{event:WindowEvent::Moved(_),..}=>Some("tray.window.move"),
            Event::WindowEvent{event:WindowEvent::Focused(_),..}=>Some("tray.window.focus"),
            Event::WindowEvent{event:WindowEvent::ThemeChanged(_),..}=>Some("tray.window.theme"),
            Event::WindowEvent{event:WindowEvent::Destroyed,..}=>Some("tray.window.destroy"),
            Event::WindowEvent{event:WindowEvent::CloseRequested,..}=>Some("tray.window.close"),
            Event::RedrawRequested(_)=>Some("tray.window.redraw-request"),_=>None,
        };
        // #1548: a fit after each tick render resizes the window; unchanged native events fold per window.
        if let Some(name)=native {let _=coreq::telemetry::capture::poll(name,0,||Ok::<_,&str>(()),|_|"observed");}

        web.expire(Instant::now());
        if let Some(calls)=&mut workspace_web {
            calls.expire(Instant::now());
            if calls.failed && !calls.notified {
                calls.notified=true;
                if let Some((_,wv))=&workspace {web_eval(wv,"window.taskqWebFault('native')","tray.web.notice",&proxy,calls);}
            }
        }
        if !web.pending.contains_key(&0)&&let Some((_,wv))=&ui&&let Some(selection)=crate::telemetry::diagnostic::take_web_selection(){
            web_eval(wv,&format!("taskq.receive({})",json!({"diagnose":selection})),"tray.web.diagnose",&proxy,&mut web);
        }
        if web.failed && !web.notified {
            web.notified=true;
            if let Some(t)=&tray {let _=t.set_tooltip(Some("TaskQ: interface outcomes unknown; check service status"));}
            if snapshot.is_none() && let Some((_,wv))=&ui {web_eval(wv,"window.taskqWebFault('native')","tray.web.notice",&proxy,&mut web);}
        }
        if web.failed && snapshot.is_some() && web.pending.is_empty() {crate::platform::exit(1);}
        let push = |web: &mut WebCalls, ui: &Option<(Window, WebView)>, st: &Value| {
            if let Some((_, wv)) = ui {
                if !web.failed {web_eval(wv,&format!("taskq.receive({st})"),"tray.web.receive",&proxy,web);}
            }
        };
        match event {
            Event::WindowEvent {window_id,event:WindowEvent::CloseRequested,..} if detached.values().any(|(w,_,_)|w.id()==window_id)=>{if let Some((w,_,_))=detached.values().find(|(w,_,_)|w.id()==window_id){w.set_visible(false);}}
            Event::WindowEvent {window_id,..} if detached.values().any(|(w,_,_)|w.id()==window_id)=>{}
            Event::WindowEvent {window_id,event:WindowEvent::CloseRequested,..} if workspace.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {
                if let Some((w,_))=&workspace {w.set_visible(false);}
            }
            Event::WindowEvent {window_id,..} if workspace.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {}
            Event::NewEvents(StartCause::Init) => {
                let b = WindowBuilder::new().with_window_icon(Some(window_icon(&compact(true,false,256.0/22.0)))).with_title("TaskQ").with_transparent(true).with_decorations(false).with_resizable(false).with_always_on_top(settings.pin.top)
                    .with_visible(snapshot.is_some()).with_inner_size(LogicalSize::new(320.0, 400.0)).with_position(tao::dpi::LogicalPosition::new(40.0, 40.0));
                // a snapshot renders off the visible screen (taskq.md R12)
                let b = if snapshot.is_some() { b.with_position(tao::dpi::LogicalPosition::new(if agentq_shots{40.0}else{-20000.0}, if agentq_shots{40.0}else{-20000.0})).with_always_on_top(false).with_focused(false) } else { b };
                #[cfg(windows)]
                let b = tao::platform::windows::WindowBuilderExtWindows::with_skip_taskbar(b, !settings.pin.pinned);
                let p = proxy.clone();
                let built = b.build(target).map_err(|e| e.to_string()).and_then(|w| {
                    // only our page: a link goes to the browser through the bridge
                    WebViewBuilder::new().with_transparent(true).with_accept_first_mouse(true).with_html(HTML).with_navigation_handler(own_page)
                        .with_ipc_handler(move |r| {
                            let _ = p.send_event(Ev::Ipc(r.into_body()));
                        })
                        .build(&w).map(|v| (w, v)).map_err(|e| e.to_string())
                });
                match built {
                    Ok(x) => {
                        dark = is_dark(&x.0);
                        ui = Some(x);
                    }
                    Err(e) => {
                        eprintln!("taskq tray: {e}");
                        *flow = ControlFlow::Exit;
                        return;
                    }
                }
                if snapshot.is_some() {
                    return;
                }
                // macOS needs a running event loop before the icon exists
                match TrayIconBuilder::new().with_icon(rgba_icon(&compact(false, dark, 2.0))).with_tooltip("taskq").build() {
                    Ok(t) => tray = Some(t),
                    Err(e) => {
                        eprintln!("taskq tray: {e}");
                        *flow = ControlFlow::Exit;
                    }
                }
                let p = proxy.clone();
                notify::watch(move |id| {
                    let _ = p.send_event(Ev::Note(id));
                });
                // a pinned window survives a tray restart at its last position; off-screen: at the tray icon (M13)
                if let (Some((w, _)), true) = (&ui, settings.pin.pinned) {
                    let saved = w.available_monitors().find(|m| key(m) == settings.pin.display).zip(settings.pin.pos.get(&settings.pin.display))
                        .filter(|(m, p)| on_screen(**p, (m.position().x, m.position().y, m.size().width as i32, m.size().height as i32)));
                    match (saved, tray.as_ref().and_then(TrayIcon::rect)) {
                        (Some((_, p)), _) => w.set_outer_position(PhysicalPosition::new(p.0, p.1)),
                        (None, Some(r)) => { settings.pin.attached_edge = place(w, r, &mut settings.pin).into(); },
                        _ => {}
                    }
                    w.set_visible(true);
                }
            }
            Event::WindowEvent { event: WindowEvent::Moved(p), .. } if settings.pin.pinned && snapshot.is_none() => {
                let Some((w, _)) = &ui else { return };
                let Some(m) = w.current_monitor() else { return };
                let s = w.outer_size();
                let c = if cfg!(target_os = "macos") { clamp((p.x, p.y), (s.width as i32, s.height as i32), work_area(&m)) } else { (p.x, p.y) };
                if c != (p.x, p.y) {
                    w.set_outer_position(PhysicalPosition::new(c.0, c.1));
                }
                settings.pin.display = key(&m);
                settings.pin.pos.insert(settings.pin.display.clone(), c);
                moved = true;
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => { if let Some((w,_))=&ui { w.set_visible(false); } }
            Event::WindowEvent { event: WindowEvent::Focused(false), .. } if snapshot.is_none() && !settings.pin.pinned => {
                if let Some((w, _)) = &ui {
                    if blur_refocuses(shown_at.elapsed(),std::mem::take(&mut workspace_handoff)) {
                        w.set_focus();
                        return;
                    }
                    w.set_visible(false);
                    hidden_at = Instant::now();
                }
            }
            Event::WindowEvent { event: WindowEvent::ThemeChanged(_), .. } => {
                if let Some((w, _)) = &ui {
                    dark = is_dark(w);
                    icon_sig.clear();
                    shown.clear();
                }
            }
            Event::UserEvent(Ev::Tick) if snapshot.is_none() => {
                // #1297: the 2 s refresh folds steady successful work into 15-minute aggregates.
                let _repeating = tracing::info_span!("tray.refresh", coreq_repeating = true).entered();
                // another version became current: exit, a service starts the current one (§ 4)
                if !alone(&install) || crate::platform::tool(&install).current().is_some_and(|v| v != crate::platform::VERSION) {
                    *flow = ControlFlow::Exit;
                    return;
                }
                let _ = fs::write(install.join("tray"), std::process::id().to_string());
                // a drag sends many moves: save the last one per tick
                if std::mem::take(&mut moved) {
                    settings.save(&install);
                }
                let st = state(&install, &settings, dark, None, &adding);
                if !web.failed {refresh_icon(&tray, &st, &settings, dark, &mut icon_sig, ui.as_ref().map(|(w,_)|w));}
                notes(&install, &mut settings, &st, &mut asked, &mut bursts);
                let mut comparable=st.clone();comparable.as_object_mut().expect("tray snapshot object").remove("now");
                let sig=comparable.to_string();
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {
                    if sig!=workspace_shown && !calls.failed && !calls.pending.contains_key(&0) {workspace_shown=sig.clone();web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}
                }
                for (_,wv,calls) in detached.values_mut(){if sig!=shown && !calls.failed && !calls.pending.contains_key(&0){web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}}
                if sig != shown && !web.pending.contains_key(&0) {
                    shown = sig;
                    if let Some((_,wv))=&ui && !web.failed {web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,&mut web);}
                }
            }
            Event::UserEvent(Ev::Click(rect)) => {
                workspace_handoff=false;
                let Some((w, _)) = &ui else { return };
                anchor = Some(rect);
                // pinned: the click raises the window, never closes it (M13)
                if settings.pin.pinned {
                    w.set_visible(true);
                    w.set_focus();
                    return;
                }
                // a click on the icon while open first blurs the popover: that click only closes it
                if w.is_visible() || hidden_at.elapsed() < Duration::from_millis(300) {
                    w.set_visible(false);
                    return;
                }
                settings.pin.attached_edge = place(w, rect, &mut settings.pin).into();
                w.set_visible(true);
                w.set_focus();
                shown_at = Instant::now();
            }
            Event::UserEvent(Ev::Answer(receipt)) => {
                push(&mut web,&ui,&receipt);
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);}
            },
            // a notification click opens the menu (or raises the pinned window) on that card (M14)
            Event::UserEvent(Ev::Note(id)) => {
                let Ok(id) = serde_json::from_str::<Value>(&id) else { return };
                let Some((w, _)) = &ui else { return };
                if !settings.pin.pinned && let Some(r) = anchor.or_else(|| tray.as_ref().and_then(TrayIcon::rect)) {
                    settings.pin.attached_edge = place(w, r, &mut settings.pin).into();
                }
                w.set_visible(true);
                w.set_focus();
                if id["root"].is_string() {
                    push(&mut web,&ui, &json!({"open": {"root": id["root"], "n": id["n"], "kind": id["k"], "title": id["t"]}}));
                }
            }
            Event::UserEvent(Ev::Evaluated(id,result)) => {
                web.finish(id,result);
                if web.pending.is_empty() && let Some(next)=shot_wait.take() {let _=proxy.send_event(next);}
            }
            Event::UserEvent(Ev::WorkspaceEvaluated(surface,id,result)) => {if surface=="main" {if let Some(calls)=&mut workspace_web {calls.finish(id,result);}}else if let Some((_,_,calls))=detached.get_mut(&surface){calls.finish(id,result);}}
            Event::UserEvent(Ev::Workspace(surface,message)) => {
                let Ok(input)=serde_json::from_str::<Value>(&message) else {return};
                if input["a"]=="conversation-window" {
                    let id=input["conversation"].as_str().unwrap_or("");
                    if !crate::resources::id(id){return;}
                    let action=input["windowAction"].as_str().or_else(||input["action"].as_str()).unwrap_or("");
                    if action=="raise" {if let Some((window,_,_))=detached.get(id){window.set_visible(true);window.set_focus();}return;}
                    if action=="detach" && surface=="main" && !detached.contains_key(id) {
                        let saved=crate::conversations::snapshot(&install);
                        let valid=saved["items"].as_array().is_some_and(|rows|rows.iter().any(|c|c["id"]==id&&c["revision"]==input["revision"]));
                        if !valid {return;}
                        let p=proxy.clone();let source=id.to_string();
                        let built=WindowBuilder::new().with_title("TaskQ Request").with_inner_size(LogicalSize::new(640.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window|{
                            WebViewBuilder::new().with_html(WORKSPACE_HTML).with_navigation_handler(own_page).with_ipc_handler(move |r|{let _=p.send_event(Ev::Workspace(source.clone(),r.into_body()));}).build(&window).map(|wv|(window,wv)).map_err(|e|e.to_string())
                        });
                        match built {
                            Ok((window,wv))=>{let mut calls=WebCalls::new();calls.workspace=true;calls.surface=id.into();window.set_visible(true);window.set_focus();detached.insert(id.into(),(window,wv,calls));}
                            Err(_)=>{if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web){let r=json!({"conversationReceipt":{"id":input["id"],"state":"unknown","reason":"Detached window unavailable; main retains ownership"}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}return;}
                        }
                    }else if action=="return" && surface==id {detached.remove(id);if let Some((window,_))=&workspace{window.set_visible(true);window.set_focus();}}
                    else {return;}
                    if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web){
                        let owner=json!({"workspaceOwner":{"id":"main","detached":detached.keys().collect::<Vec<_>>()}});
                        web_eval(wv,&format!("taskq.receive({owner})"),"tray.web.receive",&proxy,calls);
                        if action=="return" {web_eval(wv,&format!("taskq.receive({});taskq.receive({})",state(&install,&settings,dark,None,&adding),json!({"open":{"page":"conversations"}})),"tray.web.receive",&proxy,calls);let select=json!({"workspaceOwner":{"id":"main","select":id,"detached":detached.keys().collect::<Vec<_>>()}});web_eval(wv,&format!("taskq.receive({select})"),"tray.web.receive",&proxy,calls);}
                        else {let r=json!({"conversationReceipt":{"id":input["id"],"state":"accepted","snapshot":crate::conversations::snapshot(&install)}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}
                    }
                    return;
                }
                let detached_ids:Vec<String>=detached.keys().cloned().collect();
                let (w,wv,calls)=if surface=="main" {let (Some((w,wv)),Some(calls))=(&workspace,&mut workspace_web) else{return};(w,wv,calls)}else{let Some((w,wv,calls))=detached.get_mut(&surface) else{return};(&*w,&*wv,calls)};
                let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
                if m["a"]=="web-error" {calls.errors(&m);return;}
                if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
                let context=match m["context"].as_str(){Some(text)=>match coreq::telemetry::capture::TraceContext::decode(text){Ok(context)=>Some(context),Err(_)=>{calls.fault("invalid-web-context");return;}},None=>None};
                let _continuation=context.as_ref().map(|context|context.span().entered());
                if calls.failed {return;}
                match m["a"].as_str() {
                    Some("ready") | Some("refresh") => {
                        calls.finish(0,"ok");
                        web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding)),"tray.web.receive",&proxy,calls);
                        if surface=="main" && !workspace_open.is_null(){web_eval(wv,&format!("taskq.receive({})",json!({"open":workspace_open.take()})),"tray.web.receive",&proxy,calls);}
                        let own=if surface=="main" {json!({"id":"main","detached":detached_ids})}else{json!({"id":surface,"conversation":surface,"detached":[]})};
                        web_eval(wv,&format!("taskq.receive({})",json!({"workspacePin":w.is_always_on_top(),"workspaceMaximized":w.is_maximized()})),"tray.web.receive",&proxy,calls);
                        let script=if surface=="main" {format!("taskq.receive({})",json!({"workspaceOwner":own}))}else{format!("taskq.receive({});taskq.receive({})",json!({"open":{"page":"conversations"}}),json!({"workspaceOwner":own}))};
                        web_eval(wv,&script,"tray.web.receive",&proxy,calls);
                    }
                    Some("hide") => w.set_visible(false),
                    Some("workspace-window")=>{
                        match m["action"].as_str().unwrap_or("") {
                            "pin"=>{let pin=!w.is_always_on_top();w.set_always_on_top(pin);if surface=="main" {if let Ok(mut p)=requests_file_checked(&install){p["workspace_pin"]=json!(pin);let _=crate::platform::write_atomic(&install.join("tray-requests.json"),&p.to_string());}}web_eval(wv,&format!("taskq.receive({})",json!({"workspacePin":pin})),"tray.web.receive",&proxy,calls);}
                            "enlarge"=>{let enlarged=!w.is_maximized();w.set_maximized(enlarged);web_eval(wv,&format!("taskq.receive({})",json!({"workspaceMaximized":enlarged})),"tray.web.receive",&proxy,calls);},
                            "size"=>{if let (Some(width),Some(height))=(m["width"].as_f64(),m["height"].as_f64()){if width.is_finite()&&height.is_finite(){let area=w.current_monitor().map(|m|work_area(&m));let scale=w.scale_factor();let max_w=area.map_or(2000.0,|(_,_,w,_)|f64::from(w)/scale).max(320.0);let max_h=area.map_or(1600.0,|(_,_,_,h)|f64::from(h)/scale).max(240.0);w.set_inner_size(LogicalSize::new(width.clamp(320.0,max_w),height.clamp(240.0,max_h)));}}}
                            _=>{}
                        }
                    }
                    Some("conversation-navigation")=>{let error=if surface=="main" {crate::conversations::navigation(&install,&m["navigation"]).err()}else{None};web_eval(wv,&format!("taskq.receive({})",json!({"conversationNavigationError":error})),"tray.web.receive",&proxy,calls);}
                    Some("conversation-action")=>{
                        let id=m["conversation"].as_str().unwrap_or("");
                        let allowed=if surface=="main" {!detached_ids.iter().any(|s|s==id)}else{surface==id};
                        let receipt=if allowed {crate::conversations::act(&install,&m,&roots(&install))}else{json!({"id":m["id"],"state":"unknown","reason":"Draft ownership moved; edit only in its owner window"})};
                        web_eval(wv,&format!("taskq.receive({})",json!({"conversationReceipt":receipt})),"tray.web.receive",&proxy,calls);
                        shown.clear();workspace_shown.clear();
                    }
                    Some("message-action") => {
                        let ps:Vec<Value>=roots(&install).iter().map(|r|project(&install,r)).collect();
                        let keys:Vec<String>=m["keys"].as_array().into_iter().flatten().filter_map(|v|v.as_str().map(String::from)).collect();
                        let mut receipt=crate::messages::act(&install,&ps,m["action"].as_str().unwrap_or(""),&keys);receipt["id"]=m["id"].clone();
                        web_eval(wv,&format!("taskq.receive({})",json!({"messageReceipt":receipt})),"tray.web.receive",&proxy,calls);
                        web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding)),"tray.web.receive",&proxy,calls);
                        push(&mut web,&ui,&state(&install,&settings,dark,None,&adding));
                    }
                    Some("workspace-set") => {
                        settings.set(&json!({"workspace_navigation":m["navigation"]}));
                        let saved=m["navigation"].is_object() && m["navigation"]["schema"]==1 && settings.workspace_navigation==m["navigation"] && settings.save(&install);
                        web_eval(wv,&format!("taskq.receive({})",json!({"viewStateError":if saved {Value::Null}else{json!("Workspace view state was not saved; close and reopen may lose navigation")}})),"tray.web.receive",&proxy,calls);
                    }
                    Some("answer-local") => {
                        let result=answer_local(&install,m["key"].as_str().unwrap_or(""),&m["local"]);
                        web_eval(wv,&format!("taskq.receive({})",json!({"workspaceLocalError":result.err()})),"tray.web.receive",&proxy,calls);
                    }
                    Some("answer") => {
                        let key=format!("{}#{}",m["board"].as_str().unwrap_or(""),m["op"]["task"]);
                        let mut request=m.clone();request["id"]=m["op"]["id"].clone();
                        let result=if m["local"].is_object(){answer_local(&install,&key,&m["local"])}else{Ok(())}.and_then(|_|answer_actions.submit(&request));
                        if let Err(error)=result {let r=json!({"answerReceipt":{"root":m["root"],"id":m["op"]["id"],"state":"rejected","reason":error,"next":"Check delivery with the same operation"}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}
                    }
                    Some("open") => {if let Some(url)=m["url"].as_str().filter(|u|openable(u)){open(url);}}
                    Some(a) if matches!(a,"ai-send"|"ai-draft"|"ai-clear") => {
                        let result=ai(&install,a,&m);
                        let receipt=if a=="ai-send" {json!({"aiReceipt":{"id":m["id"],"state":if result.is_ok(){"accepted"}else{"unknown"},"reason":result.err().unwrap_or_else(||"Request saved; native launch is separate".into())}})}else{json!({"workspaceLocalError":result.err()})};
                        web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);
                        if a=="ai-send" {web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding)),"tray.web.receive",&proxy,calls);}
                    }
                    // Workspace reviews use the same queues, native picker and receipt adapters as the tray.
                    Some(a) if workspace_operation(a) => {let _=proxy.send_event(Ev::Ipc(message));}
                    _ => calls.fault("unsupported-workspace-action"),
                }
            }
            Event::UserEvent(Ev::Ipc(m)) => {
                let Ok(m) = serde_json::from_str::<Value>(&m) else {web.fault("invalid-ipc");return;};
                let Some((w, _)) = &ui else { return };
                if m["a"]=="web-error" {web.errors(&m);return;}
                if m["a"]=="web-observation" {
                    if web.metadata(&m["row"])&&let Some(tray)=&tray {
                        let _=tray.set_tooltip(Some("TaskQ: activity history incomplete; check each command confirmation"));
                    }
                    return;
                }
                let context=match m["context"].as_str(){Some(text)=>match coreq::telemetry::capture::TraceContext::decode(text){Ok(context)=>Some(context),Err(_)=>{web.fault("invalid-web-context");return;}},None=>None};
                let _continuation=context.as_ref().map(|context|context.span().entered());
                if web.failed {return;}
                macro_rules! reply {
                    ($value:expr) => {{
                        let value=$value;
                        push(&mut web,&ui,value);
                        if (value["projects"].is_array() || ["capmsg","focus_receipt","action_error","resources_receipt","aiError","agentqError","answerReceipt"].iter().any(|key|value.get(*key).is_some())) && let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({value})"),"tray.web.receive",&proxy,calls);}
                    }};
                }
                match m["a"].as_str().unwrap_or("") {
                    "workspace" if snapshot.is_none() => {
                        workspace_open=m["open"].clone();
                        if workspace_open["page"]=="conversations" {if let Some(id)=requests_file(&install)["conversation_navigation"]["selected"].as_str(){if let Some((window,_,_))=detached.get(id){window.set_visible(true);window.set_focus();return;}}}
                        if workspace.is_none() {
                            let p=proxy.clone();
                            let built=WindowBuilder::new().with_title("TaskQ Workspace").with_always_on_top(requests_file(&install)["workspace_pin"]==true).with_inner_size(LogicalSize::new(800.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                                WebViewBuilder::new().with_html(WORKSPACE_HTML).with_navigation_handler(own_page).with_ipc_handler(move |r|{let _=p.send_event(Ev::Workspace("main".into(),r.into_body()));}).build(&window).map(|wv|(window,wv)).map_err(|e|e.to_string())
                            });
                            match built {
                                Ok(window)=>{workspace=Some(window);let mut calls=WebCalls::new();calls.workspace=true;workspace_web=Some(calls);}
                                Err(error)=>{reply!(&json!({"workspaceError":format!("Workspace unavailable: {error}")}));return;}
                            }
                        }
                        if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {
                            if !calls.pending.contains_key(&0) && !workspace_open.is_null(){web_eval(wv,&format!("taskq.receive({})",json!({"open":workspace_open.take()})),"tray.web.receive",&proxy,calls);}
                        }
                        workspace_handoff=w.is_visible() && !settings.pin.pinned;
                        if let Some((window,_))=&workspace {window.set_visible(true);window.set_focus();}
                        reply!(&json!({"workspaceError":Value::Null}));
                    }
                    "answer" if snapshot.is_none() && crate::snapshot::CONTROLS.iter().any(|c| m["op"]["action"]==c.0) => {
                        let mut request=m.clone();request["id"]=m["op"]["id"].clone();
                        if let Err(error)=answer_actions.submit(&request){reply!(&json!({"answerReceipt":{"root":m["root"],"id":m["op"]["id"],"state":"rejected","reason":error,"next":"Check delivery with the same operation"}}));}
                    }
                    "overflow" if snapshot.is_some() => overflow.push(format!("{} by {} px", m["shot"].as_str().unwrap_or("?"), m["x"])),
                    "ready" => {
                        web.finish(0,"ok");
                        if snapshot.is_some() {
                            let _ = proxy.send_event(Ev::Shot);
                            return;
                        }
                        reply!(&state(&install, &settings, dark, None, &adding));
                    }
                    "size" | "resize" => {
                        let Some((width, height)) = m["w"].as_f64().zip(m["h"].as_f64()).filter(|(w, h)| w.is_finite() && h.is_finite()) else { return };
                        let Some(scale) = m["d"].as_f64().filter(|k| k.is_finite() && (0.5..=8.0).contains(k)) else { return };
                        let (max_w, max_h) = w.current_monitor().or_else(|| w.primary_monitor()).map_or((1920.0, 800.0), |monitor| {
                            let area = work_area(&monitor);
                            (f64::from(area.2) / scale, f64::from(area.3) / scale)
                        });
                        let size = (width.max(240.0).min(max_w), height.max(120.0).min(max_h));
                        let measurement = json!({"w": size.0, "h": if m["a"] == "resize" { size.1 } else { height.max(120.0) }, "d": scale, "g": m["g"]});
                        let (width, height) = popover_size(&measurement, max_h * scale / w.scale_factor(), w.scale_factor());
                        let old_size = w.outer_size();
                        let old_position = w.outer_position().ok();
                        w.set_inner_size(PhysicalSize::new(width.min((max_w * scale) as u32), height));
                        if m["a"] == "size" && settings.pin.size_anchor != "fixed" {
                            if let (Some(p), Some(monitor)) = (old_position, w.current_monitor()) {
                                let p = anchored_position((p.x,p.y),old_size.height as i32,(width as i32,height as i32),work_area(&monitor),if settings.pin.size_anchor=="auto" { &settings.pin.auto_anchor } else { &settings.pin.size_anchor });
                                w.set_outer_position(PhysicalPosition::new(p.0,p.1));
                            }
                        }
                        if m["a"] == "resize" {
                            settings.pin.size = Some(size);
                            moved = true;
                        }
                        // pinned: the window stays where it was put; a move then keeps it in the work area
                        if m["a"] == "size" && settings.pin.size_anchor == "fixed" && let (Some(r), false) = (anchor, settings.pin.pinned) {
                            settings.pin.attached_edge = place(w, r, &mut settings.pin).into();
                        }
                    }
                    "size-anchor" => {
                        settings.pin.size_anchor = if let Some(value)=m["value"].as_str().filter(|v| ["auto","top","bottom","fixed"].contains(v)) {value.into()} else {return};
                        if settings.pin.size_anchor == "fixed" { let size=w.inner_size().to_logical::<f64>(w.scale_factor()); settings.pin.size=Some((size.width,size.height)); }
                        settings.save(&install);
                        reply!(&state(&install,&settings,dark,None,&adding));
                    }
                    "resize-end" => { settings.save(&install); }
                    "hide" => w.set_visible(false),
                    "top" => {
                        settings.pin.top = !settings.pin.top;
                        w.set_always_on_top(settings.pin.top);
                        settings.save(&install);
                        reply!(&state(&install, &settings, dark, None, &adding));
                    }
                    "pin" | "drag" => {
                        settings.pin.pinned = m["a"]=="drag" || !settings.pin.pinned;
                        #[cfg(target_os = "macos")]
                        app_window(settings.pin.pinned);
                        #[cfg(windows)]
                        { use tao::platform::windows::WindowExtWindows; w.set_skip_taskbar(!settings.pin.pinned); }
                        if let (true, Ok(p), Some(m)) = (settings.pin.pinned, w.outer_position(), w.current_monitor()) {
                            settings.pin.display = key(&m);
                            settings.pin.pos.insert(settings.pin.display.clone(), (p.x, p.y));
                        }
                        // unpin: back to the popover at the tray icon
                        if let (false, Some(r)) = (settings.pin.pinned, anchor.or_else(|| tray.as_ref().and_then(TrayIcon::rect))) {
                            settings.pin.attached_edge = place(w, r, &mut settings.pin).into();
                        }
                        settings.save(&install);
                        w.set_focus();
                        reply!(&state(&install, &settings, dark, None, &adding));
                        if m["a"]=="drag" { let _=w.drag_window(); }
                    }
                    "resources" => {
                        let id=format!("resources-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_nanos()));
                        let request=json!({"a":"resources","root":install,"id":id});
                        if let Err(error)=resources_actions.submit(&request) {reply!(&json!({"resources_receipt":{"state":"rejected","reason":error}}));}
                    }
                    "open" => {
                        if let Some(u) = m["url"].as_str().filter(|u| openable(u)) {
                            open(u);
                        }
                    }
                    "notify" => {
                        if notify::set(&mut settings.notify, &m) {
                            settings.save(&install);
                            notes(&install, &mut settings, &Value::Null, &mut asked, &mut bursts);
                        }
                        reply!(&state(&install, &settings, dark, None, &adding));
                    }
                    "notify-test" if snapshot.is_none() => {
                        if let Some(k) = m["kind"].as_str().filter(|k| notify::KINDS.contains(k)) {
                            notify::post(&notify::sample(&settings.notify, k, now()));
                        }
                    }
                    "notify-os" => notify::open_settings(),
                    "set" => {
                        settings.set(&m);
                        let saved = (m.get("navigation").is_none() || settings.navigation == m["navigation"]) && settings.save(&install);
                        if m.get("navigation").is_some() {
                            push(&mut web, &ui, &json!({"viewStateError": if saved { Value::Null } else { json!("tray.json: save failed; view state is not persisted") }}));
                            return;
                        }
                let st = state(&install, &settings, dark, None, &adding);
                        refresh_icon(&tray, &st, &settings, dark, &mut icon_sig, ui.as_ref().map(|(w,_)|w));
                        reply!(&st);
                    }
                    // `Edit limits` after the page's confirm: the same write as `service capacity` (§ 5), this host only
                    "capacity" | "restart" | "stop" | "uninstall" | "recovery-read" | "recovery-apply" => {
                        let root = m["root"].as_str().unwrap_or("");
                        if snapshot.is_some() && m["a"]=="recovery-read" {
                            let mut reply:Value=serde_json::from_str(include_str!("../tests/fixtures/recovery-review.json")).expect("invented recovery fixture");
                            reply["root"]=m["root"].clone();reply["id"]=m["id"].clone();
                            reply!(&json!({"capmsg":reply}));
                        } else if let Err(error)=capacity_actions.submit(&m) {
                            reply!(&json!({"capmsg": {"root": root, "id": m["id"], "state":"rejected","reason":error}}));
                        }
                    }
                    // Add project (§ 4): the folder comes only from the OS picker, never from the page
                    "add" if adding.is_null() => {
                        add_generation=add_generation.wrapping_add(1);
                        let generation=add_generation;
                        adding = json!({"state": "picking", "generation":generation});
                        reply!(&state(&install, &settings, dark, None, &adding));
                        if let Err(error)=add_actions.submit(generation,None) {adding=json!({"state":"add failed","reason":error});reply!(&state(&install,&settings,dark,None,&adding));}
                    }
                    "agentq-setup" | "agentq-dismiss" if snapshot.is_none() => {
                        let error=crate::agentq::action(&install,m["a"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or(""),m["runtime"].as_str().unwrap_or("")).err();
                        reply!(&json!({"agentqError":error}));
                        reply!(&state(&install,&settings,dark,None,&adding));
                    }
                    "agentq-check" => {reply!(&state(&install,&settings,dark,None,&adding));}
                    "add-cancel" if adding["state"]=="picking" => {
                        add_generation=add_generation.wrapping_add(1);
                        adding = json!({"state":"picking dismissed","generation":add_generation});
                        reply!(&state(&install, &settings, dark, None, &adding));
                    }
                    "add-cancel" if adding["state"]=="installing" => {
                        add_generation=add_generation.wrapping_add(1);
                        adding = json!({"state":"installing dismissed","generation":add_generation});
                        reply!(&state(&install, &settings, dark, None, &adding));
                    }
                    "add-cancel" => {adding=Value::Null;reply!(&state(&install,&settings,dark,None,&adding));}
                    "add-install" if m["generation"].is_u64() && m["generation"]==adding["generation"] && adding["kind"] == "taskq" && adding["state"] == "picked" => {
                        adding["state"] = json!("installing");
                        reply!(&state(&install, &settings, dark, None, &adding));
                        let generation=adding["generation"].as_u64().unwrap_or(0);
                        if let Err(error)=add_actions.submit(generation,Some(adding.clone())) {adding["state"]=json!("install failed");adding["reason"]=json!(error);reply!(&state(&install,&settings,dark,None,&adding));}
                    }
                    "add-setup" if m["generation"].is_u64() && m["generation"]==adding["generation"] && matches!(adding["kind"].as_str(), Some("new" | "plain")) => {
                        if let Err(reason) = crate::setup::submit(&install, crate::runtime::OWNER, None, &[adding["path"].as_str().unwrap_or_default().to_string()]) {reply!(&json!({"aiError":reason}));}
                        adding = Value::Null;
                        reply!(&state(&install, &settings, dark, None, &adding));
                    }
                    "add-install" | "add-setup" => reply!(&json!({"aiError":"Setup selection changed or was already submitted; read the current selection before confirming."})),
                    a => {
                        // only a registered project (§ 4): the page never names a path of its own
                        let root = m["root"].as_str().unwrap_or("");
                        if roots(&install).iter().any(|r| r == root) {
                            let result=if matches!(a,"pause"|"resume"|"focus") {
                                focus_actions.submit(&m)
                            } else {act(a,Path::new(root),m["label"].as_str()).map(|_|())};
                            if let Err(error) = result {
                                let key=if matches!(a,"pause"|"resume"|"focus") {"focus_receipt"} else {"action_error"};
                                reply!(&json!({key:{"root":root,"id":m["id"],"text":error}}));
                            }
                        } else if matches!(a,"pause"|"resume"|"focus") {
                            reply!(&json!({"focus_receipt":{"root":root,"id":m["id"],"text":"Project is no longer registered; command not submitted"}}));
                        }
                    }
                }
            }
            Event::UserEvent(Ev::Added(generation,v)) => {
                if matches!(adding["state"].as_str(),Some("picking dismissed"|"installing dismissed"))&&add_generation==generation.wrapping_add(1) {adding=Value::Null;push(&mut web,&ui,&state(&install,&settings,dark,None,&adding));return;}
                if !apply_added(&mut adding,add_generation,generation,v) {return;}
                push(&mut web,&ui, &state(&install, &settings, dark, None, &adding));
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding)),"tray.web.receive",&proxy,calls);}
                if !adding.is_null() && let Some((w,_))=&workspace {
                    w.set_visible(true);
                    w.set_focus();
                }
            }
            Event::UserEvent(Ev::Shot | Ev::Shot2) if web.failed => {}
            // snapshot mode: per step push a state, wait for layout, capture, next
            Event::UserEvent(Ev::Shot) => {
                let dir = snapshot.clone().unwrap_or_default();
                let steps = shots(&install, &routes);
                if step == 0 {
                    let _ = fs::create_dir_all(&dir);
                }
                if step==0 && agentq_shots {
                    let warning=routes.iter().find_map(|r|agentq_fixture(r)).is_some_and(|f|f["warning"]==true);
                    let empty=summary(&[],day(now()));
                    for (name,full) in [("icon",false),("summary",true)] {
                        let image=full.then(||draw(&empty,true,1.0,2.0).ok()).flatten().unwrap_or_else(||compact(false,true,2.0));
                        fs::write(dir.join(format!("agentq-{name}.png")),agentq_mark(image,warning).png()).expect("write fixture icon");
                    }
                }
                if step == 0 && (routes.is_empty() || routes.iter().any(|r| r == "icon")) {
                    let ps: Vec<Value> = roots(&install).iter().map(|r| project(&install, r)).collect();
                    let s = summary(&ps, settings.range(now()));
                    // this Mac's menu-bar height and backing scale (A5); the names record them
                    let (k, dpr) = (menu_bar_scale(), backing_scale());
                    let size = format!("{}pt@{dpr}x", 22.0 * k);
                    for (theme, d) in [("dark", true), ("light", false)] {
                        for (mode, run) in [("run", true), ("pause", false)] {
                            for n in [16u32,32,64,128,256,512,1024] {
                                let _=fs::write(dir.join(format!("app-compact-{mode}-{theme}-{n}.png")),compact(run,d,n as f32/22.0).png());
                            }
                            let s = Summary { run, ..s.clone() };
                            let _ = fs::write(dir.join(format!("icon-compact-{mode}-{theme}-{size}.png")), compact(run, d, k * dpr).png());
                            for name in ["summary"] {
                                if let Ok(img) = draw(&s, d, k, dpr) {
                                    let _ = fs::write(dir.join(format!("icon-{name}-{mode}-{theme}-{size}.png")), img.png());
                                }
                            }
                        }
                    }
                }
                let Some((theme, page, font, script)) = steps.get(step).cloned() else {
                    if !web.pending.is_empty() {
                        shot_wait=Some(Ev::Shot);return;
                    }
                    println!("snapshots in {}", dir.display());
                    if !overflow.is_empty() {
                        eprintln!("horizontal overflow: {}", overflow.join(", "));
                        crate::platform::exit(1);
                    }
                    if !capture_errors.is_empty(){eprintln!("native capture unavailable: {}",capture_errors.join("; "));crate::platform::exit(1);}
                    *flow = ControlFlow::Exit;
                    return;
                };
                let mut s = settings.clone();
                s.font = font;
                push(&mut web,&ui, &state(&install, &s, theme == "dark", Some((theme, &page)), &Value::Null));
                if let (Some((_, wv)), false) = (&ui, script.is_empty()) {
                    web_eval(wv,script,"tray.web.fixture",&proxy,&mut web);
                }
                let p = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(800));
                    let _ = p.send_event(Ev::Shot2);
                });
            }
            Event::UserEvent(Ev::Shot2) => {
                let Some((window, wv)) = &ui else { return };
                let dir = snapshot.clone().unwrap_or_default();
                let p = proxy.clone();
                let name = format!("popover-{}.png", shot_name(&install, &routes, step));
                if !web.pending.is_empty() {
                    shot_wait=Some(Ev::Shot2);return;
                }
                web_eval(wv,&format!("taskq.measure({})",json!(&name)),"tray.web.measure",&proxy,&mut web);
                #[cfg(target_os="macos")]
                if agentq_shots && let Err(error)=capture_screen(window,&dir.join(format!("screen-{name}"))){capture_errors.push(format!("{name}: {error}"));}
                step += 1;
                capture(wv, dir.join(name), move || {
                    let _ = p.send_event(Ev::Shot);
                });
            }
            _ => {}
        }
    })
}

/// Physical inner size for a `size` message in CSS px; `max` is logical. The page renders at its own pixel ratio `d`:
/// Windows Text size scales WebView2 but not the window, so CSS px become logical px by `d / scale`. A menu taller than
/// `max` scrolls; the vertical scrollbar takes `g` of the width, so the window adds it and the menu keeps its full width
/// (#1073). The width rounds up: a fraction of a pixel short also scrolls horizontally.
fn popover_size(m: &Value, max: f64, scale: f64) -> (u32, u32) {
    let (w, h) = (m["w"].as_f64().unwrap_or(320.0), m["h"].as_f64().unwrap_or(400.0));
    let k = m["d"].as_f64().filter(|d| *d > 0.0).map_or(1.0, |d| d / scale);
    let w = if h * k > max { w + m["g"].as_f64().unwrap_or(0.0) } else { w };
    ((w * k * scale).ceil() as u32, ((h * k).min(max) * scale).round() as u32)
}

/// Reports a snapshot page that scrolls horizontally or clips content inside the menu (#1073), and a button whose label
/// is wider than the button, so the text crosses its border (#1049).

/// The `actions` snapshot: a refused custom range (menu.md M6), without applying a setting.
const ACTIONS: &str = "(()=>{const q=s=>document.querySelector(s),put=(s,v)=>{const i=q(s);i.value=v;i.dispatchEvent(new Event('input',{bubbles:true}));};
q('[data-route=settings]').click();q('[data-route=cadence]').click();q('[data-v=custom]').click();put('#hs','2026-10-13T10:00');put('#he','2026-10-13T09:00');q('[data-a=range]').click();})()";

/// The `scroll` snapshots: Home with each bounded AI block at its start, middle and end, realigned by the page's
/// `taskq.settle`, the step a reader's scroll runs once it settles (#1115). Called directly: an off-screen capture window may
/// deliver the scroll event after the capture.
const SCROLLS: [(&str, &str); 3] = [
    ("start", "document.querySelectorAll('[data-bound]').forEach(c=>{c.scrollTop=0;taskq.settle(c);})"),
    ("middle", "document.querySelectorAll('[data-bound]').forEach(c=>{c.scrollTop=(c.scrollHeight-c.clientHeight)/2;taskq.settle(c);})"),
    ("end", "document.querySelectorAll('[data-bound]').forEach(c=>{c.scrollTop=c.scrollHeight;taskq.settle(c);})"),
];

/// `tray --snapshot --route` names: `actions` is Home with the [`ACTIONS`] script, `scroll` Home with the [`SCROLLS`]
/// (only when named), `project` the first project's page.
const RECOVERY: &str = include_str!("../tests/fixtures/recovery-review.js");
/// The `answers` snapshots (#1148, menu.md M3): Attention after four Later answers, with the accepted toast, Answered
/// expanded, after dismissing, after the 6 s expiry; only when named.
const ANSWERS: [(&str, &str); 4] = [
    ("toast", concat!(include_str!("../tests/fixtures/answer-notices.js"), "taskqAnswerNotices('toast')")),
    ("expanded", concat!(include_str!("../tests/fixtures/answer-notices.js"), "taskqAnswerNotices('expanded')")),
    ("dismissed", concat!(include_str!("../tests/fixtures/answer-notices.js"), "taskqAnswerNotices('dismissed')")),
    ("expired", concat!(include_str!("../tests/fixtures/answer-notices.js"), "taskqAnswerNotices('expired')")),
];
const ROUTES: [&str; 23] = ["home", "actions", "scroll", "answers", "project", "projects", "attention", "settings", "appearance", "cadence", "notifications", "agents", "services", "about", "window", "tray-icon", "delivery", "stop", "recovery", "agentq-shown", "agentq-setting-up", "agentq-running", "agentq-unknown"];

/// Snapshot steps: theme, page (`home` or a project root), text size, a script run on the page; only `routes` unless empty.
fn shots(install: &Path, routes: &[String]) -> Vec<(&'static str, String, u64, &'static str)> {
    let first = roots(install).into_iter().next();
    ["dark", "light"].iter().flat_map(|t| {
        let mut v = vec![(*t, "home".to_string(), 16, ""), (*t, "home".to_string(), 20, ""), (*t, "home".to_string(), 16, ACTIONS)];
        for font in [16, 20] { v.extend(SCROLLS.iter().map(|(_, js)| (*t, "home".to_string(), font, *js))); }
        for font in [16, 20] { v.extend(ANSWERS.iter().map(|(_, js)| (*t, "attention".to_string(), font, *js))); }
        v.extend(first.clone().map(|r| (*t, r, 16, "")));
        for route in &ROUTES[5..] {
            if route.starts_with("agentq-") && !routes.iter().any(|r|r==route){continue;}
            for font in [16, 20] { v.push((*t, route.to_string(), font, if *route=="recovery" {RECOVERY}else{""})); }
        }
        v
    }).filter(|(_, p, _, js)| if routes.is_empty() { !matches!(route_of(p, js),"scroll"|"answers") } else { routes.iter().any(|r| r == route_of(p, js)) }).collect()
}

fn route_of<'a>(page: &'a str, script: &str) -> &'a str {
    if script == RECOVERY { "recovery" } else if script == ACTIONS { "actions" } else if ANSWERS.iter().any(|(_, s)| *s == script) { "answers" } else if !script.is_empty() { "scroll" } else if Path::new(page).is_absolute() { "project" } else { page }
}

fn shot_name(install: &Path, routes: &[String], step: usize) -> String {
    shots(install, routes).get(step).map_or(step.to_string(), |(t, p, f, js)| {
        let suffix = if js.is_empty() || *js==RECOVERY { String::new() } else { format!("-{}", SCROLLS.iter().chain(&ANSWERS).find(|(_, s)| s == js).map_or("actions", |(n, _)| n)) };
        let scroll = match route_of(p, js) { "scroll" => "-scroll", "answers" => "-answers", _ => "" };
        format!("{t}-{}{scroll}{suffix}-{f}", if Path::new(p).is_absolute() { "project" } else { p })
    })
}

fn state(install: &Path, s: &Settings, dark: bool, snap: Option<(&str, &str)>, add: &Value) -> Value {
    let ps: Vec<Value> = roots(install).iter().map(|r| project(install, r)).collect();
    let messages=crate::messages::snapshot(install,&ps);
    let onboarding=snap.and_then(|(_,route)|agentq_fixture(route)).unwrap_or_else(crate::agentq::snapshot);
    let observed=now();
    let (start, end) = s.range(observed);
    let sum = summary_at(&ps, (start, end), observed);
    let url = |rgba: &Rgba| format!("data:image/png;base64,{}", base64(&rgba.png()));
    let mut st = json!({"platform": if cfg!(windows) { "windows" } else { "macos" }, "version": crate::platform::VERSION,
        "agentq":onboarding,"messages":messages,"navigation": s.navigation, "workspace_navigation":s.workspace_navigation, "host": ps.iter().find_map(|p| p["host"].as_str()).unwrap_or("?"), "view": s.view, "font": s.font, "period": s.period, "pinned": s.pin.pinned, "top": s.pin.top, "size": s.pin.size, "size_anchor": s.pin.size_anchor, "attached_edge": s.pin.attached_edge, "auto_anchor":s.pin.auto_anchor,
        "now": observed, "palette":icon::css(dark), "service":crate::host::service_status(install),"lifecycle":crate::lifecycle::status(install,None),"restart_review":crate::lifecycle::review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"stop_review":crate::lifecycle::stop_review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"full_stop_review":crate::lifecycle::full_stop_review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"stop_hold":crate::lifecycle::stop_hold_status(install),"stop_modes":cfg!(target_os="macos"),"projects": ps, "notify": notify_state(&s.notify, &ps), "agents": agents(&ps), "workspace_local": requests_file(install)["answers"], "conversations":crate::conversations::snapshot(install), "ai": {"request": crate::setup::last(install, true), "runtime":owner_runtime(&ps), "root":requests_file(install)["root"], "local":requests_file(install)["local"], "error":requests_file(install)["error"], "draft": requests_file(install)["draft"], "history": requests_file(install)["history"]}, "add": add, "setup": crate::setup::last(install, false), "history": {"start": start, "end": end, "tz": s.tz, "bins": sum.bins, "peak":icon::peak(&sum.bins), "svg":icon::svg(&icon::Data {paused:false,..icon_data(&sum)},dark), "since": sum.since},
        // § 4 Data: a corrupt tray.json names its reason while the defaults show
        "settings_error": crate::data::json(&install.join("tray.json")).err()});
    {
        st["icons"] = json!({"icon": url(&agentq_mark(compact(sum.run,dark,2.0),st["agentq"]["warning"]==true)), "full":icon::svg(&icon_data(&sum),dark)});
    }
    if let Some((theme, page)) = snap {
        st["theme"] = json!(theme);
        st["page"] = json!(page);
    }
    st
}

/// Notifications for the page: OS status, host modes, per project override (null: inherit) and effective mode.
fn notify_state(n: &Value, ps: &[Value]) -> Value {
    let host: serde_json::Map<String, Value> = notify::KINDS.iter().map(|k| (k.to_string(), json!(notify::host(n, k)))).collect();
    let projects: serde_json::Map<String, Value> = ps.iter().filter_map(|p| p["root"].as_str()).map(|r| {
        (r.to_string(), notify::KINDS.iter().map(|k| (k.to_string(), json!([n["projects"][r][k].as_str().filter(|v| notify::MODES.contains(v)), notify::mode(n, r, k)]))).collect())
    }).collect();
    json!({"status": notify::status(), "failure": notify::failure(), "host": host, "projects": projects})
}

/// One tick of notifications (M14): the first banner setting asks the OS once; new events move the cursor, which is
/// saved before anything posts (a crash loses a notification, never repeats one); only an allowed OS posts.
fn notes(install: &Path, s: &mut Settings, st: &Value, asked: &mut bool, bursts: &mut notify::Bursts) {
    if !*asked && notify::status() == "not asked" && notify::wants_banner(&s.notify, &roots(install)) {
        *asked = true;
        notify::request();
    }
    let Some(ps) = st["projects"].as_array() else { return };
    let before = s.notify.clone();
    let posts = notify::due(&mut s.notify, ps, now(), bursts);
    if s.notify != before && !s.save(install) {
        s.notify = before;
        return;
    }
    if notify::status() == "allowed" {
        for p in &posts {
            notify::post(p);
        }
    }
}

/// Host-level `Agents` (§ 4): the rows of the freshest probe in any `status.json`, as menu lines; null when none has one.
fn agents(ps: &[Value]) -> Value {
    let Some(a) = ps.iter().map(|p| &p["agents"]).filter(|a| a["at"].is_u64()).max_by_key(|a| a["at"].as_u64()) else { return Value::Null };
    json!({"at": a["at"], "rows": a["rows"].as_array().into_iter().flatten().map(crate::runtime::agent_text).collect::<Vec<_>>()})
}

/// The default 24 h Summary period.
fn day(now: i64) -> (i64, i64) {
    rolling(now, 86400)
}

/// Six whole epoch-aligned bins of `period / 6`, the newest holding `now` (partial): the grid moves one bin at a boundary (M6).
fn rolling(now: i64, period: i64) -> (i64, i64) {
    let w = period / 6;
    let end = (now.div_euclid(w) + 1) * w;
    (end - period, end)
}

/// One yellow mark composed after the existing icon, preserving its counters and other warnings.
fn agentq_mark(image:Rgba,warning:bool)->Rgba {
    if !warning{return image;}
    let s=image.s;let gap=(6.0*s).ceil() as u32;let mut marked=Rgba::new(image.w+gap,image.h,s);
    for y in 0..image.h as usize {
        let from=y*image.w as usize*4;let to=y*marked.w as usize*4;
        marked.px[to..to+image.w as usize*4].copy_from_slice(&image.px[from..from+image.w as usize*4]);
    }
    let cx=image.w as f32/s+3.0;let cy=3.0;
    marked.fill([238,184,40,255],|x,y|(x-cx).powi(2)+(y-cy).powi(2)<=2.7f32.powi(2));
    marked.fill([32,30,20,255],|x,y|(x-cx).abs()<0.4 && ((y-cy>=-1.6&&y-cy<=0.4)||(y-cy>=1.0&&y-cy<=1.7)));
    marked
}
fn agentq_fixture(route:&str)->Option<Value> {
    let fixtures:Value=serde_json::from_str(include_str!("../tests/fixtures/agentq-onboarding.json")).expect("invented agentq fixtures");
    let patch=fixtures.get(route)?;let mut value=fixtures["base"].clone();
    for (key,value_patch) in patch.as_object()?{value[key]=value_patch.clone();} Some(value)
}

#[cfg(target_os="macos")]
fn capture_screen(window:&Window,path:&Path)->Result<(),String> {
    use tao::platform::macos::WindowExtMacOS;
    #[link(name="CoreGraphics",kind="framework")]
    unsafe extern "C" {fn CGPreflightScreenCaptureAccess()->bool;}
    // SAFETY: preflight never prompts; ns_window is this live window on its UI thread.
    let id:isize=unsafe {
        if !CGPreflightScreenCaptureAccess(){return Err("Screen Recording not allowed (TCC); no prompt requested".into());}
        objc2::msg_send![window.ns_window() as *mut objc2::runtime::AnyObject, windowNumber]
    };
    crate::platform::output("tray.agentq-screencapture",std::process::Command::new("/usr/sbin/screencapture").args(["-x","-o","-l",&id.to_string()]).arg(path),Duration::from_secs(5)).map_err(|e|e.to_string())?;
    if !fs::read(path).is_ok_and(|b|b.starts_with(b"\x89PNG\r\n\x1a\n")){return Err("screencapture wrote no PNG".into());} Ok(())
}

fn icon_data(s:&Summary)->icon::Data {
    icon::Data { blocked:s.known.then_some(s.blocked),counts:s.counts.map(|n|s.known.then_some(n)),bins:s.bins.clone(),paused:!s.run }
}
fn draw(s:&Summary,dark:bool,k:f32,dpr:f32)->Result<Rgba,String> {
    icon::raster(&icon::svg(&icon_data(s),dark),(22.0*k*dpr).round() as u32)
}

fn refresh_icon(tray: &Option<TrayIcon>, st: &Value, s: &Settings, dark: bool, sig: &mut String, window: Option<&Window>) {
    let Some(t) = tray else { return };
    let observed=st["now"].as_i64().unwrap_or_else(now);
    let sum = summary_at(st["projects"].as_array().map_or(&[][..], |v| v), s.range(observed), observed);
    let full=s.view=="full";
    let (k,dpr)=if full {(menu_bar_scale(),backing_scale())}else{(1.0,2.0)};
    // Reuse the published vector: a time-bin boundary cannot split native and web surfaces.
    let svg=full.then(||st["icons"]["full"].as_str().unwrap_or("").to_owned());
    let new=format!("{} {dark} {k} {dpr} {} {} {:?} {svg:?}",st["agentq"]["warning"],sum.run,sum.known,(sum.counts,sum.blocked));
    if *sig == new {
        return;
    }
    let applied = crate::profile::measure("tray.render", || {
        let rendered = svg.as_ref().map(|svg| icon::raster(svg,(22.0*k*dpr).round() as u32));
        let failed = rendered.as_ref().is_some_and(Result::is_err);
        let img = agentq_mark(rendered.and_then(Result::ok).unwrap_or_else(|| compact(sum.run, dark, k * dpr)),st["agentq"]["warning"]==true);
        let applied = t.set_icon(Some(rgba_icon(&img)));
        // Same configuration, state and fallback as the tray, rendered at native app-icon resolution.
        let app = agentq_mark(svg.as_ref().and_then(|svg| icon::raster(svg,256).ok())
            .unwrap_or_else(|| compact(sum.run, dark, 256.0/22.0)),st["agentq"]["warning"]==true);
        refresh_app_icon(&app, window)?;
        if k > 1.0 { fit_status_image(k); }
        if failed { Err("invalid icon render".to_string()) } else { applied.map_err(|e| e.to_string()) }
    });
    match applied { Ok(()) => *sig = new, Err(e) => eprintln!("taskq app icon: {e}") }
    let [w, a, r, _] = sum.counts;
    let counts = if sum.known { format!("in work {w} · waiting for answer {a} · ready + blocked {}",r + sum.blocked) } else { "counts unknown (service error or stale status)".into() };
    let _ = t.set_tooltip(Some(format!("taskq: {counts}{}{}", if sum.run { "" } else { " · paused" },if st["agentq"]["warning"]==true{" · agentq is not installed (last confirmed absence; check Agents)"}else{""})));
}

fn window_icon(img: &Rgba) -> tao::window::Icon {
    tao::window::Icon::from_rgba(img.px.clone(),img.w,img.h).expect("renderer RGBA dimensions")
}

/// Live app identity is in memory; never invalidate the signed bundle by rewriting its icon.
fn refresh_app_icon(img: &Rgba, window: Option<&Window>) -> Result<(),String> {
    if let Some(w)=window {w.set_window_icon(Some(window_icon(img)));}
    #[cfg(target_os = "macos")]
    {
        use objc2::AnyThread;
        use objc2_app_kit::{NSApplication,NSImage};
        use objc2_foundation::{NSData,MainThreadMarker};
        let mtm=MainThreadMarker::new().ok_or("app icon requires main thread")?;
        let image=NSImage::initWithData(NSImage::alloc(),&NSData::with_bytes(&img.png())).ok_or("app icon PNG decode failed")?;
        // SAFETY: main thread and a live decoded image retained by NSApplication.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)); }
    }
    Ok(())
}

/// M13: only a pinned tray participates in the normal macOS app switcher.
#[cfg(target_os = "macos")]
fn app_window(pinned: bool) {
    use objc2_app_kit::{NSApplication,NSApplicationActivationPolicy};
    if let Some(mtm)=objc2_foundation::MainThreadMarker::new() {
        let app=NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(if pinned {NSApplicationActivationPolicy::Regular} else {NSApplicationActivationPolicy::Accessory});
    }
}

fn workspace_operation(action:&str)->bool {
    matches!(action,"agentq-setup"|"agentq-dismiss"|"agentq-check"|"capacity"|"restart"|"stop"|"uninstall"|"recovery-read"|"recovery-apply"|"pause"|"resume"|"focus"|"update"|"add"|"add-cancel"|"add-install"|"add-setup"|"resources"|"log"|"folder")
}

/// A blur this soon after opening can be the Windows hidden-icons flyout retaking focus.
fn blur_refocuses(shown_for: Duration, workspace_handoff: bool) -> bool {
    !workspace_handoff && shown_for < Duration::from_millis(300)
}

/// Put the popover under a menu-bar icon or above a taskbar icon, inside the icon's monitor.
fn place(w: &Window, r: Rect, pin: &mut Pin) -> &'static str {
    let (ix, iy) = (r.position.x, r.position.y);
    let m = w.available_monitors().find(|m| {
        let (p, s) = (m.position(), m.size());
        ix >= p.x as f64 && ix < (p.x + s.width as i32) as f64 && iy >= p.y as f64 && iy < (p.y + s.height as i32) as f64
    });
    let area=m.as_ref().map(work_area);
    let (mp, ms) = m.map_or(((0, 0), (1920, 1080)), |m| ((m.position().x, m.position().y), (m.size().width as i32, m.size().height as i32)));
    let sz = w.outer_size();
    let (ww, wh) = (sz.width as i32, sz.height as i32);
    let edge=[("top",iy-mp.1 as f64),("bottom",(mp.1+ms.1) as f64-iy-r.size.height as f64),("left",ix-mp.0 as f64),("right",(mp.0+ms.0) as f64-ix-r.size.width as f64)].into_iter().min_by(|a,b|a.1.total_cmp(&b.1)).unwrap().0;
    let cx=ix as i32+r.size.width as i32/2-ww/2;
    let cy=iy as i32+r.size.height as i32/2-wh/2;
    let (x,y)=match edge {"bottom"=>(cx,iy as i32-wh),"left"=>(ix as i32+r.size.width as i32,cy),"right"=>(ix as i32-ww,cy),_=>(cx,iy as i32+r.size.height as i32)};
    let usable=area.unwrap_or((mp.0,mp.1,ms.0,ms.1));
    pin.auto_anchor=growth_anchor("auto",edge,Some(iy+f64::from(r.size.height)/2.0),usable).into();
    let p=clamp((x,y),(ww,wh),usable);
    w.set_outer_position(PhysicalPosition::new(p.0,p.1));
    edge
}

/// A display's name in `tray.json`; unnamed: its origin.
fn key(m: &tao::monitor::MonitorHandle) -> String {
    m.name().unwrap_or_else(|| format!("{},{}", m.position().x, m.position().y))
}

/// The top-left `p` lies on the area `(x, y, w, h)`.
fn on_screen(p: (i32, i32), (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    p.0 >= x && p.0 < x + w && p.1 >= y && p.1 < y + h
}

/// Top-left that keeps a window of `size` inside the area `(x, y, w, h)`; a too large window keeps its top-left edge in.
fn clamp(p: (i32, i32), size: (i32, i32), (x, y, w, h): (i32, i32, i32, i32)) -> (i32, i32) {
    (p.0.clamp(x, (x + w - size.0).max(x)), p.1.clamp(y, (y + h - size.1).max(y)))
}

/// M13 Auto uses the actual native anchor, including side taskbars; explicit settings never change on Pin/drag.
fn growth_anchor(mode:&str,edge:&str,icon_y:Option<f64>,area:(i32,i32,i32,i32))-> &'static str {
    if mode=="bottom" || mode=="auto" && (edge=="bottom" || matches!(edge,"left"|"right") && icon_y.is_some_and(|y| y > f64::from(area.1)+f64::from(area.3)/2.0)) {"bottom"} else {"top"}
}

/// M13: preserve the named edge unless the monitor's work area requires a clamp.
fn anchored_position(p:(i32,i32),old_height:i32,size:(i32,i32),area:(i32,i32,i32,i32),anchor:&str)->(i32,i32) {
    clamp((p.0,if anchor=="bottom" {p.1+old_height-size.1} else {p.1}),size,area)
}

/// The display without menu bar and Dock, physical pixels.
#[cfg(target_os = "macos")]
fn work_area(m: &tao::monitor::MonitorHandle) -> (i32, i32, i32, i32) {
    use tao::platform::macos::MonitorHandleExtMacOS;
    let (p, s) = (m.position(), m.size());
    let full = (p.x, p.y, s.width as i32, s.height as i32);
    let Some(ptr) = m.ns_screen().filter(|p| !p.is_null()) else { return full };
    // SAFETY: tao hands out a live NSScreen; read on the main thread
    let screen = unsafe { &*(ptr as *const objc2_app_kit::NSScreen) };
    let (f, v, k) = (screen.frame(), screen.visibleFrame(), m.scale_factor());
    // AppKit counts from the bottom: top inset = menu bar, bottom inset = Dock
    let top = ((f.origin.y + f.size.height) - (v.origin.y + v.size.height)) * k;
    let (left, bottom) = ((v.origin.x - f.origin.x) * k, (v.origin.y - f.origin.y) * k);
    (p.x + left as i32, p.y + top as i32, (v.size.width * k) as i32, s.height as i32 - top as i32 - bottom as i32)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn work_area(m: &tao::monitor::MonitorHandle) -> (i32, i32, i32, i32) {
    (m.position().x, m.position().y, m.size().width as i32, m.size().height as i32)
}

#[cfg(windows)]
fn work_area(m: &tao::monitor::MonitorHandle) -> (i32, i32, i32, i32) {
    use windows::Win32::{Foundation::POINT, Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST}};
    let p = m.position();
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: the monitor handle and writable MONITORINFO are valid for these synchronous reads.
    if unsafe { GetMonitorInfoW(MonitorFromPoint(POINT { x: p.x, y: p.y }, MONITOR_DEFAULTTONEAREST), &mut info) }.as_bool() {
        let r = info.rcWork;
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    } else {
        let s = m.size();
        (p.x, p.y, s.width as i32, s.height as i32)
    }
}

#[tracing::instrument(name="tray.act",skip_all,fields(coreq_operation=match action {
    "pause"|"resume"|"focus"|"update"|"restart"|"log"|"folder"|"incident-report"|"incident-manual"|"stop"|"uninstall"=>action,
    _=>"invalid",
}))]
fn act(action: &str, root: &Path, label: Option<&str>) -> crate::Res<Option<String>> {
    let d = crate::dot(root);
    match action {
        "pause" => return crate::pause(root,false).map(Some),
        "resume" => {
            return crate::resume(root).map(|(_,receipt)|Some(receipt));
        }
        // a label of the project's open tasks, never `all` (§ 4)
        "focus" => {
            if let Some(l) = label.filter(|l| crate::status_labels(root).iter().any(|x| x == l)) {
                return crate::set_focus(root, l, None).map(Some);
            } else { return Err("Label is no longer available; refresh and choose again".into()); }
        }
        // `Update now` (menu.md M9): the service checks at once (§ 4 step 7); the page waits for the version in status.json
        "update" => {
            fs::write(d.join("update"), "").map_err(|e| e.to_string())?;
            crate::poke(root)?;
        }
        "restart" | "stop" | "uninstall" => {
            return Err("Host lifecycle action needs a reviewed request; refresh the Services panel".into());
        }
        "log" => {
            let s: Value = fs::read_to_string(d.join("status.json")).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_default();
            if let Some(l) = s["log"].as_str() {
                open(l);
            }
        }
        "folder" => open(&root.display().to_string()),
        "incident-report" => {
            let s: Value = fs::read_to_string(d.join("status.json")).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_default();
            if let Some(path) = incident_path(root, &s["incident"]) {
                open(&path.display().to_string());
            }
        }
        "incident-manual" => open(&format!("https://github.com/{}/issues/new", crate::platform::RELEASE_REPO)),
        _ => {}
    }
    Ok(None)
}

fn incident_path(root: &Path, incident: &Value) -> Option<PathBuf> {
    let dir = crate::dot(root).join("incidents").canonicalize().ok()?;
    let path = root.join(incident["path"].as_str()?);
    let resolved = path.canonicalize().ok()?;
    (resolved.starts_with(dir) && resolved.is_file() && resolved.extension().is_some_and(|x| x == "json")).then_some(path)
}

/// `Resources` (menu.md M5): btop in its own OS terminal window, installed first with the OS package manager when
/// missing, its output in that window; no package manager: one line saying so.
const BTOP_SH: &str = r#"#!/bin/sh
printf '\033]0;Resources\007\033[8;30;110t'
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
if ! command -v btop >/dev/null; then
  if ! command -v brew >/dev/null; then echo "btop is not installed and Homebrew was not found: install btop, then try again."; read _; exit 1; fi
  echo "Installing btop: brew install btop"
  brew install btop || { echo "brew install btop failed (see above)."; read _; exit 1; }
fi
exec btop
"#;
const BTOP_PS1: &str = r#"$Host.UI.RawUI.WindowTitle = 'Resources'
if (-not (Get-Command btop -ErrorAction SilentlyContinue)) {
  if (-not (Get-Command winget -ErrorAction SilentlyContinue)) { 'btop4win is not installed and winget was not found: install btop4win, then try again.'; Read-Host; exit 1 }
  'Installing btop: winget install aristocratos.btop4win'
  winget install -e --id aristocratos.btop4win --accept-source-agreements --accept-package-agreements
  $env:Path = [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + [Environment]::GetEnvironmentVariable('Path', 'Machine')
  if (-not (Get-Command btop -ErrorAction SilentlyContinue)) { 'winget install aristocratos.btop4win failed (see above).'; Read-Host; exit 1 }
}
btop
"#;

fn resources() -> crate::Res<Value> {
    let file = std::env::temp_dir().join(if cfg!(windows) { "taskq-resources.ps1" } else { "taskq-resources.command" });
    fs::write(&file, if cfg!(windows) { BTOP_PS1 } else { BTOP_SH }).map_err(|e|e.to_string())?;
    // The terminal owns its human-driven lifetime. Submission does not claim that btop installed or appeared.
    let mut command=if cfg!(windows) {
        let path=file.display().to_string().replace('\'',"''");let mut c=std::process::Command::new("powershell");
        c.args(["-NoProfile","-Command",&format!("$args=@('powershell','-NoProfile','-ExecutionPolicy','Bypass','-File','\"{path}\"'); Start-Process -FilePath 'conhost.exe' -ArgumentList $args -ErrorAction Stop")]);c
    } else {
        #[cfg(unix)]
        fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).map_err(|e|e.to_string())?;
        let mut c=std::process::Command::new("open");c.args(["-a", "Terminal"]).arg(&file);c
    };
    let output=crate::platform::output("tray.resources.submit",&mut command,Duration::from_secs(30))?;
    if !output.status.success() {return Err(String::from_utf8_lossy(&output.stderr).lines().find(|line|!line.trim().is_empty()).unwrap_or("resource terminal submission failed").chars().take(160).collect());}
    Ok(json!({"state":"submitted"}))
}

/// The OS folder picker; None on cancel.
fn pick() -> Result<Option<PathBuf>,String> {
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-STA", "-Command", "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = 'Add project'; if ($d.ShowDialog() -eq 'OK') { [Console]::Out.Write($d.SelectedPath) }"]);
        c
    } else {
        let mut c = std::process::Command::new("osascript");
        c.args(["-e", "activate", "-e", "POSIX path of (choose folder with prompt \"Add project\")"]);
        c
    };
    coreq::telemetry::capture::operation("tray.picker",|| {
    let o = crate::platform::quiet(&mut c).output().map_err(|e|format!("folder picker did not start: {e}"))?;
    pick_result(o)
    },|_|"picker-error")
}
fn pick_result(o:std::process::Output)->Result<Option<PathBuf>,String> {
    if !o.status.success() {
        let error=String::from_utf8_lossy(&o.stderr);
        if !cfg!(windows)&&error.trim_end().ends_with("(-128)") {return Ok(None);}
        return Err(error.lines().find(|line|!line.trim().is_empty()).unwrap_or("folder picker failed").chars().take(160).collect());
    }
    let p=std::str::from_utf8(&o.stdout).map_err(|_|"folder picker returned a non-UTF-8 path; selection not applied")?;
    let p=p.strip_suffix("\r\n").or_else(||p.strip_suffix('\n')).unwrap_or(p);
    let p=p.to_string();
    Ok((!p.is_empty()).then(|| PathBuf::from(p)))
}

/// `taskq selftest` in `dir`: its first error line on failure.
fn selftest(dir: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let o = crate::platform::output("tray.selftest",std::process::Command::new(me).arg("selftest").current_dir(dir).env("TASKQ_ROOT", dir),Duration::from_secs(30))?;
    let text = String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout);
    if o.status.success() { Ok(()) } else { Err(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("selftest failed").chars().take(160).collect()) }
}

/// The four Add project cases (menu.md M10): `taskq.json` with a passing `selftest` (taskq), with a failing one (old,
/// owner's marker), else an empty folder or a Git root (new) or files without Git (plain: not a repository).
fn classify(dir: &Path, selftest: impl Fn(&Path) -> Result<(), String>) -> Value {
    let (kind, reason) = if dir.join("taskq.json").is_file() {
        selftest(dir).map_or_else(|e| ("old", e), |_| ("taskq", String::new()))
    } else if dir.join(".git").exists() || fs::read_dir(dir).is_ok_and(|mut d| d.next().is_none()) {
        ("new", String::new())
    } else {
        ("plain", String::new())
    };
    json!({"state": "picked", "path": dir, "name": crate::project_name(dir), "kind": kind, "reason": reason})
}

/// `service install` in the picked taskq project after the page's confirm; installed only when `<install>/projects`
/// lists it (R12).
fn install_service(install: &Path, mut a: Value) -> Value {
    let dir = PathBuf::from(a["path"].as_str().unwrap_or(""));
    let r = std::env::current_exe().map_err(|e| e.to_string()).and_then(|me| {
        let mut c = std::process::Command::new(me);
        c.args(["service", "install"]).current_dir(&dir).env("TASKQ_ROOT", &dir).stdin(std::process::Stdio::null());
        // a separate process group: launchctl must not take the tray with it
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        let o = crate::platform::output("tray.install", &mut c, Duration::from_secs(60))?;
        let text = String::from_utf8_lossy(&o.stderr).to_string() + &String::from_utf8_lossy(&o.stdout);
        if !o.status.success() {
            return Err(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("service install failed").to_string());
        }
        if roots(install).iter().any(|r| Path::new(r) == dir) { Ok(()) } else { Err("installed, but this host's projects list does not show it".into()) }
    });
    a["state"] = json!(if r.is_ok() { "installed" } else { "install failed" });
    a["reason"] = json!(r.err().unwrap_or_default());
    a
}

/// A picker/install result belongs only to the currently presented operation. Cancel invalidates its generation;
/// it cannot stop an install that was already submitted.
fn apply_added(adding:&mut Value,current:u64,generation:u64,value:Value)->bool {
    if adding.is_null()||current!=generation||value["generation"].as_u64()!=Some(generation) {return false;}
    if value["state"]=="cancelled" {*adding=Value::Null;return true;}
    *adding=value;true
}

/// Write the page's capacity edit for this host; the outcome line for the page.
fn capacity(install:&Path,root: &Path,m: &Value) -> crate::Res<Value> {
    if matches!(m["a"].as_str(),Some("recovery-read"|"recovery-apply")) {
        if !cfg!(target_os="macos") {return Err("Recovery is Mac only".into());}
        let mut command=owner_command(root)?;command.args(["service","recover","--json"]);
        if m["a"]=="recovery-apply" {
            let request:crate::recovery::Request=serde_json::from_value(m["request"].clone()).map_err(|_|"invalid recovery request")?;
            command.arg("--request").arg(serde_json::to_string(&request).map_err(|e|e.to_string())?);
        }
        let out=crate::platform::output("tray.recovery",&mut command,Duration::from_secs(60))?;
        if !out.status.success() {return Err("Recovery read or write unconfirmed; inspect the board and reconcile the same selection".into());}
        return serde_json::from_slice(&out.stdout).map_err(|_|"Recovery receipt unreadable; reconcile the same selection".into());
    }

    if matches!(m["a"].as_str(),Some("restart"|"stop"|"uninstall")) {
        let mut request=m.clone();let object=request.as_object_mut().ok_or("invalid restart message")?;
        for key in ["context","a","root"] {object.remove(key);}
        if request["action"].as_str().unwrap_or("restart")!=m["a"].as_str().unwrap_or("") {return Err("lifecycle action differs from its reviewed request".into());}
        if m["a"]=="uninstall"&&request["project"]!=json!(root.canonicalize().map_err(|e|e.to_string())?) {return Err("Uninstall root differs from its reviewed request".into());}
        return crate::lifecycle::submit(install,&request);
    }
    let mut input=m.clone();input.as_object_mut().ok_or("invalid capacity message")?.remove("context");
    if !crate::resources::keys(&input,&["a","root","id","ops","issued_at","expires_at"]) {
        return Ok(json!({"state":"rejected","reason":"Unsupported host capacity payload; refresh and review current settings"}));
    }
    let request=json!({"id":input["id"],"ops":input["ops"],"issued_at":input["issued_at"],"expires_at":input["expires_at"]});
    if let Err(error)=crate::host::capacity_request(&request) {return Ok(json!({"state":"rejected","reason":error}));}
    let policy=crate::host::Policy::read(install)?;
    crate::host::local_capacity(install,&policy.me,&request)
}

/// Web links and Codex app threads (setup sessions, #1522); the id reaches a shell on Windows, so only a plain id.
fn openable(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
        || crate::runtime::thread_of(u).is_some_and(crate::runtime::plain_thread)
}

fn open(arg: &str) {
    let opener = if cfg!(windows) { "explorer" } else { "open" };
    let _ = coreq::telemetry::capture::operation("tray.open-request",||crate::platform::quiet(std::process::Command::new(opener).arg(arg)).spawn().map(|_|()),|_|"spawn");
}

/// The menu bar's theme: macOS follows the app appearance; the Windows taskbar has its own setting.
fn is_dark(_w: &Window) -> bool {
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("reg");
        c.args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "/v", "SystemUsesLightTheme"]);
        let out = crate::platform::output("tray.theme", &mut c, Duration::from_secs(10)).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        !out.contains("0x1")
    }
    #[cfg(not(windows))]
    {
        _w.theme() == tao::window::Theme::Dark
    }
}

// ---------- icons: drawn by the shared renderer (src/icon.rs) ----------

fn rgba_icon(i: &Rgba) -> Icon {
    Icon::from_rgba(i.px.clone(), i.w, i.h).expect("rgba icon")
}

/// Call `f` with the status item's window and button (tray-icon keeps them private).
#[cfg(target_os = "macos")]
fn with_status_button(mut f: impl FnMut(&objc2_app_kit::NSWindow, &objc2_app_kit::NSButton)) {
    use objc2_app_kit::{NSApplication, NSButton, NSView, NSWindow};
    use objc2_foundation::MainThreadMarker;
    fn walk(w: &NSWindow, v: &NSView, f: &mut impl FnMut(&NSWindow, &NSButton)) -> bool {
        if let Some(b) = v.downcast_ref::<NSButton>() {
            f(w, b);
            return true;
        }
        v.subviews().iter().any(|s| walk(w, &s, f))
    }
    let Some(mtm) = MainThreadMarker::new() else { return };
    for w in NSApplication::sharedApplication(mtm).windows().iter() {
        if w.class().name().to_str() == Ok("NSStatusBarWindow") && w.contentView().is_some_and(|v| walk(&w, &v, &mut f)) {
            return;
        }
    }
}

/// Menu-bar height over the 22 pt the drawing assumes: 1.5 on notch Macs, 1 elsewhere.
#[cfg(target_os = "macos")]
fn menu_bar_scale() -> f32 {
    use objc2_app_kit::NSScreen;
    // no status item yet (`--snapshot`): the main screen's menu bar, its frame above the visible frame
    let mut bar = objc2_foundation::MainThreadMarker::new().and_then(NSScreen::mainScreen).map_or(22.0, |s| {
        let (f, v) = (s.frame(), s.visibleFrame());
        (f.origin.y + f.size.height - v.origin.y - v.size.height) as f32
    });
    // 0 until the item is placed: the next tick re-renders (`k` is in the icon signature)
    with_status_button(|w, _| bar = w.frame().size.height as f32);
    if (22.0..=44.0).contains(&bar) { bar / 22.0 } else { 1.0 }
}

/// Pixels per point of the main screen (2 on Retina).
#[cfg(target_os = "macos")]
fn backing_scale() -> f32 {
    objc2_foundation::MainThreadMarker::new().and_then(objc2_app_kit::NSScreen::mainScreen).map_or(2.0, |s| s.backingScaleFactor() as f32)
}

#[cfg(not(target_os = "macos"))]
fn menu_bar_scale() -> f32 {
    1.0
}

// shortcut: Windows uses a 2x icon raster; qualify per-monitor native sizes before changing calibration
#[cfg(not(target_os = "macos"))]
fn backing_scale() -> f32 {
    2.0
}

#[cfg(not(target_os = "macos"))]
fn fit_status_image(_: f32) {}

/// tray-icon caps the status-item image at 22 pt; grow it back to the full menu-bar height.
#[cfg(target_os = "macos")]
fn fit_status_image(k: f32) {
    use objc2_foundation::NSSize;
    with_status_button(|_, b| {
        let Some(img) = b.image() else { return };
        let sz = img.size();
        if (sz.height - 22.0).abs() < 0.5 {
            img.setSize(NSSize::new(sz.width * k as f64, 22.0 * k as f64));
            // the button takes its height from a newly set image, not from a resized one
            b.setImage(None);
            b.setImage(Some(&img));
        }
    });
}

pub(crate) fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            s.push(if k <= c.len() { A[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

// ---------- popover snapshot (`taskq tray --snapshot`): the webview renders its own page ----------

#[cfg(target_os = "macos")]
fn capture(wv: &WebView, path: PathBuf, done: impl Fn() + 'static) {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use wry::WebViewExtMacOS;
    let block = block2::RcBlock::new(move |img: *mut NSImage, _: *mut NSError| {
        // SAFETY: WebKit passes a valid image or null
        if let Some(img) = unsafe { img.as_ref() } {
            let png = img.TIFFRepresentation().and_then(|t| NSBitmapImageRep::imageRepWithData(&t))
                .and_then(|r| unsafe { r.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) });
            if let Some(p) = png {
                let _ = fs::write(&path, p.to_vec());
            }
        }
        done();
    });
    // SAFETY: main thread, a live webview
    unsafe { wv.webview().takeSnapshotWithConfiguration_completionHandler(None, &block) };
}

#[cfg(windows)]
fn capture(wv: &WebView, path: PathBuf, done: impl Fn() + 'static) {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::System::Com::{STGM_CREATE, STGM_WRITE};
    use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
    use wry::WebViewExtWindows;
    let p = windows::core::HSTRING::from(path.as_os_str());
    // SAFETY: a file stream handed to WebView2, released by the handler after the capture
    let Ok(stream) = (unsafe { SHCreateStreamOnFileEx(&p, (STGM_CREATE | STGM_WRITE).0, FILE_ATTRIBUTE_NORMAL.0, true, None) }) else {
        return done();
    };
    let done = std::rc::Rc::new(done);
    let (mut keep, d) = (Some(stream.clone()), done.clone());
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |_| {
        keep.take();
        d();
        Ok(())
    }));
    // SAFETY: UI thread, a live webview
    if unsafe { wv.webview().CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler) }.is_err() {
        done();
    }
}

/// Navigation filter of every tray webview: the inline page loads (wry navigates to it), web links do not.
fn own_page(u: String) -> bool { !u.starts_with("http") }

#[cfg(test)]
mod tests {
    use super::*;

    // A1/C1: `|_|false` blocked the inline page itself and left the window blank.
    // #1391 A2/A4: every icon style keeps all original counters/warnings; the absence mark clears independently.
    #[test]
    fn agentq_badge_preserves_icon_pixels_and_snapshot_states() {
        let image=compact(true,true,2.0);let original=image.px.clone();let width=image.w;
        assert_eq!(agentq_mark(compact(true,true,2.0),false).px,original);
        let marked=agentq_mark(image,true);
        for y in 0..marked.h as usize {assert_eq!(&marked.px[y*marked.w as usize*4..y*marked.w as usize*4+width as usize*4],&original[y*width as usize*4..(y+1)*width as usize*4]);}
        for (route,kind,warning) in [("agentq-shown","shown",true),("agentq-setting-up","setting-up",true),("agentq-running","running",false),("agentq-unknown","unknown",true)] {
            let f=agentq_fixture(route).unwrap();assert_eq!(f["kind"],kind);assert_eq!(f["warning"],warning);
        }
    }

    #[test]
    fn own_page_loads_inline_html_only() {
        assert!(own_page("about:blank".into()));
        assert!(own_page("data:text/html,x".into()));
        assert!(!own_page("https://example.com".into()));
        assert!(!own_page("http://example.com".into()));
    }

    // #1464 A4/A5/A7: native drafts survive restart/corruption and never replay owner/task effects.
    #[test]
    fn workspace_local_preserves_drafts_and_unknown_identity_without_replay() {
        let base=std::env::temp_dir().join(format!("taskq-workspace-local-{}",crate::runtime::uuid()));
        fs::create_dir_all(&base).unwrap();
        let local=json!({"draft":"kept literal answer","bound":12,"op":{"id":"same-op","task":42,"revision":12,"text":"kept literal answer"},"receipt":{"state":"unknown"},"caret":[2,4]});
        answer_local(&base,"https://example.org/issues#42",&local).unwrap();
        ai(&base,"ai-draft",&json!({"text":"native request","root":"registered","local":{"text":"native request","caret":[1,5]}})).unwrap();
        let observed=requests_file_checked(&base).unwrap();
        assert_eq!(observed["answers"]["https://example.org/issues#42"],local);
        assert_eq!(observed["local"]["caret"],json!([1,5]));
        assert!(crate::setup::all(&base).unwrap().0.is_empty(),"view persistence must not replay effects");
        let bytes=fs::read(base.join("tray-requests.json")).unwrap();
        assert!(answer_local(&base,"x",&json!({"draft":"x".repeat(65537)})).is_err());
        assert_eq!(fs::read(base.join("tray-requests.json")).unwrap(),bytes);
        fs::write(base.join("tray-requests.json"),"broken kept bytes").unwrap();
        assert!(ai(&base,"ai-draft",&json!({"text":"replacement"})).is_err());
        assert!(answer_local(&base,"x",&local).is_err());
        assert_eq!(fs::read_to_string(base.join("tray-requests.json")).unwrap(),"broken kept bytes");
        assert!(requests_file(&base)["error"].as_str().unwrap().contains("existing data kept"));
        fs::remove_dir_all(base).unwrap();
    }

    // A2/M7: Send opens one owner session in the chosen registered project. Risk: an unregistered path or empty
    // text launches a session, or a refused send drops the draft.
    #[test]
    fn ai_send_opens_one_session_in_a_registered_project() {
        let base=std::env::temp_dir().join(format!("taskq-tray-ai-{}",std::process::id()));
        let _=fs::remove_dir_all(&base);
        let install=base.join("install");let root=base.join("project");let other=base.join("other");
        for d in [&install,&root,&other] {fs::create_dir_all(d).unwrap();}
        fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
        let r=root.to_str().unwrap();
        ai(&install,"ai-draft",&json!({"text":"draft"})).unwrap();
        assert!(ai(&install,"ai-send",&json!({"root":other.to_str().unwrap(),"text":"why"})).is_err());
        assert!(ai(&install,"ai-send",&json!({"root":r,"text":"  "})).is_err());
        assert_eq!(requests_file(&install)["draft"],"draft","refused send lost its draft");
        assert!(crate::setup::last(&install,true).is_null());
        ai(&install,"ai-send",&json!({"id":"owner-request-1464","runtime":"claude","root":r,"text":" why "})).unwrap();
        ai(&install,"ai-send",&json!({"id":"owner-request-1464","runtime":"claude","root":r,"text":" why "})).unwrap();
        assert_eq!(crate::setup::all(&install).unwrap().0.len(),1,"same ID retry must not launch twice");
        assert!(ai(&install,"ai-send",&json!({"id":"owner-request-1464","runtime":"claude","root":r,"text":"changed"})).is_err());
        let req=crate::setup::last(&install,true);
        assert_eq!(req["state"],"pending");
        assert_eq!(crate::setup::all(&install).unwrap().0[0].root.as_deref(),Some(root.canonicalize().unwrap().to_str().unwrap()));
        assert_eq!((requests_file(&install)["draft"].clone(),requests_file(&install)["history"].clone()),(json!(""),json!(["why"])));
        let _=fs::remove_dir_all(base);
    }

    // Chain: native delivery -> exact JS outcome or unknown -> bounded safe shared observations.
    #[test]
    fn web_execution_requires_acknowledgment_and_bounds_error_reports() {
        crate::with_test_capture(|store,_| {
            let now=Instant::now();
            let mut calls=WebCalls::new();calls.finish(0,"ok");
            for (name,result) in [("tray.web.success","ok"),("tray.web.exception","script-error"),("tray.web.malformed","invalid-ack")] {
                let id=calls.begin(name,now).unwrap();calls.finish(id,result);calls.finish(id,"ok");
            }
            let expired=calls.begin("tray.web.expired",now).unwrap();
            calls.expire(now+Duration::from_secs(29));assert!(calls.pending.contains_key(&expired));
            calls.expire(now+Duration::from_secs(30));assert!(calls.pending.is_empty());calls.finish(expired,"ok");
            for _ in 0..64 {assert!(calls.begin("tray.web.pending",now).is_some());}
            assert!(calls.begin("tray.web.overflow",now).is_none());assert_eq!(calls.pending.len(),64);
            for counts in [json!([1,0,0,0]),json!([101,1,1,1]),json!([101,1,1,1])] {
                calls.errors(&json!({"counts":counts,"saturated":false}));
            }
            for counts in [json!([1,1,1,1]),json!([-1,0,0,0]),json!([4294967296_u64,1,1,1]),json!([101]),json!(null)] {
                calls.errors(&json!({"counts":counts,"saturated":false}));assert_eq!(calls.counts,[101,1,1,1]);
            }
            calls.errors(&json!({"counts":[101,1,1,1],"saturated":true}));
            calls.errors(&json!({"counts":[101,1,1,1],"saturated":true}));
            assert!(calls.failed && calls.saturated);drop(calls);
            tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().unwrap().flush().unwrap());
            let page=store.export(0,0,1024).unwrap();
            let rows:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"]).filter(|r|r["kind"]=="method").collect();
            for (name,result) in [("success","ok"),("exception","failed"),("malformed","unknown"),("expired","unknown")] {
                let found:Vec<_>=rows.iter().filter(|r|r["operation"]==format!("taskq.operation.tray.web.{name}")).collect();
                assert_eq!(found.len(),1,"duplicate/late callback changed {name}");assert_eq!(found[0]["outcome"],result);
            }
            assert_eq!(rows.iter().filter(|r|r["operation"]=="taskq.operation.tray.web.pending"&&r["outcome"]=="unknown").count(),64);
            let errors:Vec<_>=rows.iter().filter(|r|r["operation"]=="taskq.operation.tray.web.error").collect();
            assert_eq!(rows.iter().filter(|r|r["operation"]=="taskq.operation.tray.web.error-counts"&&r["error"]=="count-saturated").count(),1);
            assert_eq!(errors.len(),5);assert_eq!(errors.iter().map(|r|r["calls"].as_u64().unwrap()).sum::<u64>(),104);
            assert!(errors.iter().all(|r|r["outcome"]=="failed"));
            assert_eq!(web_result("true"),"invalid-ack");assert_eq!(web_result("\"ok\""),"ok");
        });
    }

    // Chain: bounded UI submission -> owned worker -> same-trace receipt -> stale registration refusal.
    #[test]
    fn queued_focus_actions_keep_context_and_refuse_overflow_without_blocking() {
        crate::with_test_capture(|store,dir| {
            let root=dir.join("project");let install=dir.join("install");
            fs::create_dir_all(crate::dot(&root)).unwrap();fs::create_dir_all(&install).unwrap();
            fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(crate::dot(&root).join("focus"),"area-maps").unwrap();
            let (result_tx,result_rx)=std::sync::mpsc::channel();
            let (release_tx,release_rx)=std::sync::mpsc::channel();
            let queue=ActionQueue::start(install.clone(),&["pause","resume","focus"],true,queued_focus,move |root,id,result| {
                result_tx.send((root,id,result)).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            }).unwrap();
            let message=|action:&str,id:&str|json!({"a":action,"root":root,"id":id});
            let _click=tracing::info_span!("fixture.click").entered();
            for invalid in [String::new(),"../outside".into(),"x".repeat(65)] {
                assert!(queue.submit(&message("pause",&invalid)).unwrap_err().contains("correlation id"));
                assert_eq!(crate::data::focus(&root).unwrap(),["area-maps"]);
            }
            queue.submit(&message("pause","tray.1")).unwrap();
            let (reported,id,result)=result_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(reported,root);assert_eq!(id,"tray.1");let applied=result.unwrap()["applied_receipt"].as_str().unwrap().to_string();
            assert!(queue.submit(&message("pause","tray.1")).is_ok(),"the same pending operation coalesces");
            assert!(queue.submit(&message("resume","tray.1")).unwrap_err().contains("different request"),"an id cannot be repurposed while pending");
            let snapshot=crate::control::snapshot(&root,&mut None,&mut None).unwrap();
            assert_eq!(snapshot.command["applied_receipt"],applied);
            let status=json!({"focus_command":snapshot.command,"focus":snapshot.focus,"tasks":[],"complete":true});
            fs::write(crate::dot(&root).join("status.json"),status.to_string()).unwrap();
            assert_eq!(project(&install,root.to_str().unwrap())["focus_command"],status["focus_command"]);
            for n in 0..16 {queue.submit(&message("resume",&format!("queued-{n}"))).unwrap();}
            assert!(queue.submit(&message("resume","overflow")).unwrap_err().contains("queue full"));
            assert!(crate::data::focus(&root).unwrap().is_empty());
            // Queued intent cannot act on a project removed after submission.
            fs::write(install.join("projects"),"").unwrap();
            for n in 0..16 {
                release_tx.send(()).unwrap();
                let (reported,id,result)=result_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                assert_eq!(reported,root);assert_eq!(id,format!("queued-{n}"));assert!(result.unwrap_err().contains("no longer registered"));
            }
            release_tx.send(()).unwrap();
            let ActionQueue{sender,worker,..}=queue;drop(sender);worker.join().unwrap();
            assert!(crate::data::focus(&root).unwrap().is_empty());
            assert!(crate::telemetry::settle(Duration::from_secs(30)));
            let page=store.export(0,0,1024).unwrap();
            let rows:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"]).collect();
            let receipts:Vec<_>=rows.iter().copied().filter(|r|r["source"]=="command").collect();
            assert_eq!(receipts.len(),3,"refused queued actions mutated the project");
            let click=rows.iter().find(|r|r["operation"]=="taskq.fixture.click").unwrap();
            assert_eq!(receipts[0]["trace"],click["trace"]);assert_eq!(receipts[1]["trace"],click["trace"]);
            assert_eq!(receipts[1]["kind"],"command.applied");
            assert_eq!(receipts[2]["kind"],"command.observed");
        });
    }

    // H-A2: a blocked host-policy write cannot delay the independent restrictive Pause command.
    #[test]
    fn blocked_capacity_does_not_block_pause() {
        crate::with_test_capture(|_store,dir| {
            let root=dir.join("project");let install=dir.join("install");
            fs::create_dir_all(crate::dot(&root)).unwrap();fs::create_dir_all(install.join("resources")).unwrap();
            fs::write(root.join("taskq.json"),"{\"repo\":\"fixture/project\"}").unwrap();
            fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(install.join("host.json"),json!({"hosts":{crate::hostname():"capacity-host"},"limits":{"codex":2}}).to_string()).unwrap();
            fs::write(crate::dot(&root).join("focus"),"all").unwrap();
            let policy=crate::host::Policy::read(&install).unwrap();
            assert_eq!(project(&install,root.to_str().unwrap())["cap_host"],"capacity-host");
            let now=SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
            let message=json!({"a":"capacity","root":root,"id":"blocked","issued_at":now,"expires_at":now+300,"ops":[{
                "target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":"codex"},
                "policy_schema":"taskq.host-capacity/1","expected_revision":policy.revision().unwrap(),"changes":{"runtime_limit":3}}]});
            let lock=coreq::file_lock::lock(&install.join("resources/lock")).unwrap();
            let (capacity_tx,capacity_rx)=std::sync::mpsc::channel();
            let blocked=ActionQueue::start(install.clone(),&["capacity"],true,|install,root,message| {
                crate::platform::write_atomic(&install.join("capacity-entered"),"")?;
                capacity(install,root,message)
            },move |_,_,result|capacity_tx.send(result).unwrap()).unwrap();
            blocked.submit(&message).unwrap();
            let until=Instant::now()+Duration::from_secs(3);
            while !install.join("capacity-entered").exists() && Instant::now()<until {std::thread::yield_now();}
            assert!(install.join("capacity-entered").exists());assert!(capacity_rx.try_recv().is_err());
            let (pause_tx,pause_rx)=std::sync::mpsc::channel();
            let focus=ActionQueue::start(install.clone(),&["pause","resume","focus"],true,queued_focus,move |_,_,result|pause_tx.send(result).unwrap()).unwrap();
            focus.submit(&json!({"a":"pause","root":root,"id":"pause"})).unwrap();
            assert!(pause_rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap()["applied_receipt"].is_string());
            assert!(crate::data::focus(&root).unwrap().is_empty());assert!(capacity_rx.try_recv().is_err());
            drop(lock);
            assert_eq!(capacity_rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap()["state"],"saved");
            for queue in [blocked,focus] {let ActionQueue{sender,worker,..}=queue;drop(sender);worker.join().unwrap();}
        });
    }

    #[test]
    fn blocked_answer_does_not_block_pause() {
        crate::with_test_capture(|_store,dir| {
            let root=dir.join("project");let install=dir.join("install");
            fs::create_dir_all(crate::dot(&root)).unwrap();fs::create_dir_all(&install).unwrap();
            fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(crate::dot(&root).join("focus"),"all").unwrap();
            let (entered_tx,entered_rx)=std::sync::mpsc::channel();let (release_tx,release_rx)=std::sync::mpsc::channel();
            let answer=ActionQueue::start(install.clone(),&["answer"],true,move |_,_,_| {entered_tx.send(()).unwrap();release_rx.recv_timeout(Duration::from_secs(3)).unwrap();Ok(json!({"answerReceipt":{"state":"accepted"}}))},|_,_,_|{}).unwrap();
            answer.submit(&json!({"a":"answer","root":root,"id":"tray.1","op":{"id":"tray.1"}})).unwrap();entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            let (pause_tx,pause_rx)=std::sync::mpsc::channel();let focus=ActionQueue::start(install.clone(),&["pause","resume","focus"],true,queued_focus,move |_,_,result|pause_tx.send(result).unwrap()).unwrap();
            focus.submit(&json!({"a":"pause","root":root,"id":"pause"})).unwrap();
            assert!(pause_rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap()["applied_receipt"].is_string());assert!(crate::data::focus(&root).unwrap().is_empty());
            release_tx.send(()).unwrap();
            for queue in [answer,focus] {let ActionQueue{sender,worker,..}=queue;drop(sender);worker.join().unwrap();}
        });
    }

    // Chain: native Resume -> shared command -> failure receipt; retries keep the selected focus.
    #[test]
    fn action_failures_reach_the_tray_caller() {
        crate::with_test_capture(|store, _| {
            let root=std::env::temp_dir().join(format!("taskq-action-receipt-{}",std::process::id()));
            fs::create_dir_all(crate::dot(&root)).unwrap();
            fs::write(crate::dot(&root).join("focus"),"off").unwrap();
            assert!(act("resume",&root,None).unwrap_err().contains("no saved focus"));
            assert_eq!(fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"off");
            fs::write(crate::dot(&root).join("focus.paused"),"telemetry-fixture").unwrap();
            act("resume",&root,None).unwrap();
            assert_eq!(fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"telemetry-fixture");
            assert!(act("focus",&root,Some("stale-label")).is_err());
            fs::remove_dir_all(&root).unwrap();
            assert!(act("pause",&root,None).is_err());
            assert!(crate::telemetry::settle(Duration::from_secs(30)));
            let page=store.export(0,0,1024).unwrap();
            let receipts:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"])
                .filter(|r|r["source"]=="command").collect();
            assert_eq!(receipts.len(),6,"native dispatch bypassed the shared receipt boundary");
            assert_eq!(receipts.iter().map(|r|r["kind"].as_str().unwrap()).collect::<Vec<_>>(),
                ["command.received","command.failed","command.received","command.applied","command.received","command.failed"]);
            assert_ne!(receipts[0]["span"],receipts[2]["span"],"separate clicks reused an identity");
            for pair in receipts.chunks_exact(2) { assert_eq!(pair[0]["span"],pair[1]["span"]); }

        });
    }

    // Chain: R12 tray evidence -> `tray --snapshot --route`. Risk: a filtered run captures the full matrix or drops a route.
    #[test]
    fn snapshot_routes_capture_only_named_pages() {
        let install = std::env::temp_dir().join(format!("taskq-tray-routes-{}", std::process::id()));
        fs::create_dir_all(&install).unwrap();
        let root = std::env::temp_dir().join("taskq-tray-routes-project");
        fs::write(install.join("projects"), root.to_str().unwrap()).unwrap();
        let r = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let names = |v: &[&str]| (0..shots(&install, &r(v)).len()).map(|i| shot_name(&install, &r(v), i)).collect::<Vec<_>>();
        assert_eq!(names(&["attention"]), ["dark-attention-16", "dark-attention-20", "light-attention-16", "light-attention-20"]);
        assert_eq!(names(&["actions", "project"]), ["dark-home-actions-16", "dark-project-16", "light-home-actions-16", "light-project-16"]);
        assert_eq!(names(&["scroll"])[..3], ["dark-home-scroll-start-16", "dark-home-scroll-middle-16", "dark-home-scroll-end-16"]);
        assert_eq!(names(&["scroll"]).len(), 12, "start, middle and end at 16 and 20 px in both themes");
        assert_eq!(names(&["answers"])[..4], ["dark-attention-answers-toast-16", "dark-attention-answers-expanded-16", "dark-attention-answers-dismissed-16", "dark-attention-answers-expired-16"]);
        assert_eq!(names(&["answers"]).len(), 16, "four answer phases at 16 and 20 px in both themes");
        assert!(names(&["icon"]).is_empty(), "icon alone captures no popover page");
        let all = names(&[]);
        assert_eq!(all.len(), 2 * (4 + 2 * (ROUTES.iter().filter(|r|!r.starts_with("agentq-")).count() - 5)), "no filter keeps the menu matrix without scroll or answers");
        assert!(all.iter().all(|name|!name.contains("agentq-")),"invented agentq evidence is opt-in only");
        for route in ROUTES {
            if route.starts_with("agentq-"){assert_eq!(names(&[route]).len(),4,"one state, both themes and sizes");}
            assert!(!names(&[route]).is_empty(), "{route} names a captured page");
        }
        fs::remove_dir_all(&install).ok();
    }

    // Chain: owner tray answer -> canonical CLI. Risk: IPC selects another project or inherits worker fencing.
    #[test]
    fn answer_bridge_restricts_targets_and_preserves_literal_owner_input() {
        let install = std::env::temp_dir().join(format!("taskq-tray-answer-{}", std::process::id()));
        let root = install.join("project");
        fs::create_dir_all(crate::dot(&root)).unwrap();
        fs::write(install.join("projects"), root.to_str().unwrap()).unwrap();
        let mut m = json!({"root": root, "board": "https://github.com/o/r/issues", "op": {"id": "tray.1", "task": 7, "revision": 20, "text": "2; literal $(text)"}});
        let args = answer_args(&install, &m).unwrap();
        assert_eq!(args, ["answer", "7", "--text", "2; literal $(text)", "--json", "--op-id", "tray.1", "--revision", "20"]);
        let command = owner_command(&root).unwrap();
        for key in ["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE"] {
            assert!(command.get_envs().any(|(k, v)| k == key && v.is_none()), "{key}");
        }
        m["op"]["choice"] = json!(2);
        assert!(answer_args(&install, &m).is_err(), "choice and text are exclusive");
        m["op"].as_object_mut().unwrap().remove("text");
        assert_eq!(answer_args(&install, &m).unwrap()[1], "7.2");
        m["root"] = json!(install.join("unregistered"));
        assert!(answer_args(&install, &m).unwrap_err().contains("not registered"));
        m["root"] = json!(root);
        m["op"]["revision"] = Value::Null;
        assert!(answer_args(&install, &m).is_err());
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: live board snapshot -> tray answer. Risk: forged IPC bypasses read-only/close controls.
    #[test]
    fn answer_bridge_checks_board_and_canonical_question() {
        let card = "Done: checked\nProblem: decide\n1. close: ship ★\n2. redo";
        let row = json!({"n": 7, "revision": 20, "card": card, "question": crate::snapshot::question(card)});
        let mut s = json!({"complete": true, "board": {"url": "https://github.com/o/r/issues"}, "tasks": [row]});
        let mut m = json!({"board": "https://github.com/o/r/issues", "op": {"id": "tray.1", "task": 7, "revision": 20, "choice": 1}});
        assert!(answer_snapshot(&s, &m).unwrap_err().contains("close/publication"));
        m["op"]["choice"] = json!(2);
        assert!(answer_snapshot(&s, &m).is_ok());
        s["tasks"][0]["question"]["parsed"] = json!(false);
        assert!(answer_snapshot(&s, &m).unwrap_err().contains("read-only"));
        s["tasks"][0]["revision"] = json!(21);
        assert!(answer_snapshot(&s, &m).is_ok(), "dispatch owns stale and idempotent retry receipts");
        let rows = s["tasks"].take();
        s["tasks"] = json!([]);
        assert!(answer_snapshot(&s, &m).is_err(), "an omitted row never bypasses the consequential-choice guard");
        s["tasks"] = rows;
        s["board"]["url"] = json!("https://github.com/o/other/issues");
        assert!(answer_snapshot(&s, &m).unwrap_err().contains("identity"));
        s["complete"] = json!(false);
        s["reason"] = json!("permission denied");
        assert_eq!(answer_snapshot(&s, &m).unwrap_err(), "permission denied");
    }

    // Chain: no-choice ask -> tray IPC -> CLI dispatch -> board readback. Risk: cancellation publishes a result,
    // stale input acts on another question, or retry/partial failure duplicates a write or skips parking.
    #[test]
    fn answer_bridge_fallback_cancellation_chain() {
        use crate::board::{Board, Fake};
        let install = std::env::temp_dir().join(format!("taskq-tray-cancel-{}", std::process::id()));
        let root = install.join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(install.join("projects"), root.to_str().unwrap()).unwrap();
        fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
        let (cfg, _) = crate::load(&root).unwrap();
        let b = Fake::default();
        b.state.borrow_mut().publish = "pending".into(); // a publication attempt must not succeed
        let n = b.create("Needs a decision", "## Goal\ng", &["taskq".into()]).unwrap();
        let attempt = b.comment(n, "taskq: spawn worker codex:u mac").unwrap();
        b.comment(n, &format!("taskq: result {} attempt {attempt}", "a".repeat(40))).unwrap();
        let reviewer = b.comment(n, "taskq: spawn reviewer codex:v mac").unwrap();
        let rev = b.comment(n, &format!("taskq: ask attempt {reviewer}\nReviewer cannot continue.")).unwrap();
        let mut m = json!({"root": root, "board": "https://github.com/o/r/issues", "op": {"id":"cancel.1", "task":n, "revision":rev, "action":"close", "confirmed":true}});
        let mut snapshot = json!({"complete":true,"board":{"url":m["board"]},"tasks":[{"n":n,"state":"ask","revision":rev,"question":crate::snapshot::question("Reviewer cannot continue.")}]});
        assert!(answer_snapshot(&snapshot, &m).is_ok());
        for st in ["doing", "review", "ready", "waiting"] {
            snapshot["tasks"][0]["state"] = json!(st);
            assert!(answer_snapshot(&snapshot, &m).is_err());
        }
        snapshot["tasks"][0]["state"] = json!("ask");
        snapshot["tasks"][0]["revision"] = json!(rev + 1);
        assert!(answer_snapshot(&snapshot, &m).is_err());
        snapshot["tasks"][0]["revision"] = json!(rev);
        snapshot["tasks"][0]["question"]["choices"] = json!([{"id":1}]);
        assert!(answer_snapshot(&snapshot, &m).is_err());
        snapshot["tasks"][0]["question"]["choices"] = json!([]);
        m["op"]["confirmed"] = json!(false);
        assert!(answer_args(&install, &m).is_err());
        m["op"]["confirmed"] = json!(true);
        let args = answer_args(&install, &m).unwrap();
        assert_eq!(args[0], "later");
        assert_eq!(args[args.len()-3], "cancel.1.later");
        for outcome in ["rejected", "stale", "unknown"] {
            let mut calls = 0;
            let r = answer_dispatch(&m, &args, |_| { calls += 1; json!({"task":n,"op_id":"cancel.1.later","outcome":outcome}) });
            assert_eq!((calls, r["outcome"].as_str().unwrap()), (1, outcome));
        }
        let mut calls = Vec::new();
        let execute = |args: &[String]| {
            let op = crate::dispatch::Op { id: Some(args[args.len()-3].clone()), revision: Some(args.last().unwrap().parse().unwrap()) };
            let receipt = if args[0] == "close" { crate::dispatch::close_one(&b, &cfg, n, &args[3], &op) }
                else { crate::dispatch::act(&b, &root, n, &args[0], &[], &args[3], &op) };
            serde_json::to_value(receipt).unwrap()
        };
        let partial = answer_dispatch(&m, &args, |a| {
            calls.push(a.to_vec());
            if a[0] == "close" { json!({"task":n,"op_id":"cancel.1.close","outcome":"unknown","reason":"response lost"}) }
            else { execute(a) }
        });
        assert!(partial["reason"].as_str().unwrap().contains("Parked; close not verified"));
        assert_eq!(crate::dispatch::current(&b, n).unwrap().1.state, crate::core::State::Later);
        snapshot["tasks"][0]["state"] = json!("later");
        assert!(answer_snapshot(&snapshot, &m).is_ok(), "retry can reconcile its parking receipt");
        let r = answer_dispatch(&m, &args, |a| { calls.push(a.to_vec()); execute(a) });
        assert_eq!(r["state"], "closed");
        assert_eq!(r["outcome"], "accepted", "must cancel without publishing the stored result");
        assert_eq!(calls[0], calls[2]);
        assert_eq!(calls[1], calls[3]);
        assert_eq!(calls[1].last().unwrap(), &rev.to_string());
        let count = b.comments(n).unwrap().len();
        snapshot["tasks"] = json!([]);
        assert!(answer_snapshot(&snapshot, &m).is_err(), "an omitted row never grants cancellation authority");
        assert_eq!(answer_dispatch(&m, &args, execute)["state"], "closed");
        assert_eq!(b.comments(n).unwrap().len(), count);
        for action in ["requeue", "later"] {
            m["op"] = json!({"id":"action.1","task":n,"revision":rev,"action":action});
            if action == "requeue" { m["op"]["text"] = json!("Retry with literal $(text)"); }
            let a = answer_args(&install, &m).unwrap();
            assert_eq!(a[0], action);
            assert_eq!(a[a.len()-3], "action.1");
            let mut seen = 0;
            assert_eq!(answer_dispatch(&m, &a, |_| { seen += 1; json!({"task":n,"op_id":"action.1","outcome":"accepted","state":"ready"}) })["outcome"], "accepted");
            assert_eq!(seen, 1);
        }
        // #1476 Start: held drafts only; accepted counts only when read back ready or waiting.
        m["op"] = json!({"id":"start.1","task":n,"revision":rev,"action":"start"});
        let a = answer_args(&install, &m).unwrap();
        assert_eq!(a, ["start", &n.to_string(), "--json", "--op-id", "start.1", "--revision", &rev.to_string()]);
        for (st, ok) in [("held", true), ("ready", true), ("waiting", true), ("ask", false), ("doing", false)] {
            snapshot["tasks"] = json!([{"n":n,"state":st,"revision":rev}]);
            assert_eq!(answer_snapshot(&snapshot, &m).is_ok(), ok, "{st}");
        }
        for (st, outcome) in [("ready", "accepted"), ("waiting", "accepted"), ("held", "unknown")] {
            assert_eq!(answer_dispatch(&m, &a, |_| json!({"task":n,"op_id":"start.1","outcome":"accepted","state":st}))["outcome"], outcome, "{st}");
        }
        m["op"]["text"] = json!("x");
        assert!(answer_args(&install, &m).is_err());
        m["op"] = json!({"id":"action.1","task":n,"revision":rev,"action":"requeue"});
        m["op"]["text"] = json!("x".repeat(233));
        assert!(answer_args(&install, &m).is_err());
        m["op"]["text"] = json!("a\nb");
        assert!(answer_args(&install, &m).is_err());
        // #1506 published controls: the DashQ fixture argv and state table are what the tray dispatches and offers.
        let f: Value = serde_json::from_str(include_str!("../tests/fixtures/task-controls.json")).unwrap();
        assert_eq!(f["version"], crate::snapshot::CONTROLS_VERSION);
        let table: Vec<Value> = crate::snapshot::CONTROLS.iter().map(|(a, c, t, s)| json!([a, c, t, s])).collect();
        let fixture: Vec<Value> = f["controls"].as_array().unwrap().iter().map(|c| json!([c["action"], c["command"], c["text"], c["states"]])).collect();
        assert_eq!(table, fixture);
        for (st, offered) in f["states"].as_object().unwrap() { assert_eq!(json!(crate::snapshot::controls(st)), *offered, "{st}"); }
        for c in f["cases"].as_array().unwrap() {
            m["op"] = c["op"].clone();
            assert_eq!(json!(answer_args(&install, &m).unwrap()), c["argv"]);
            snapshot["tasks"] = json!([c["row"]]);
            assert!(answer_snapshot(&snapshot, &m).is_ok());
            snapshot["tasks"][0]["state"] = json!("closed");
            assert!(answer_snapshot(&snapshot, &m).is_err(), "closed offers no control");
            snapshot["tasks"][0]["revision"] = json!(99);
            // start reconciles by state (#1476); the others let an old revision reach the CLI, which replays or refuses stale
            assert_eq!(answer_snapshot(&snapshot, &m).is_ok(), c["op"]["action"] != "start");
        }
        m["op"] = json!({"id":"x.1","task":n,"revision":rev,"action":"stop","text":"x"});
        assert!(answer_args(&install, &m).is_err());
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: menu `size` message -> popover window size. Risk: a scrolling menu clipped on the right behind a horizontal
    // scrollbar (#1073), a short menu given an empty scrollbar strip, or Windows Text size shrinking the CSS viewport.
    #[test]
    fn scrolling_menu_window_adds_the_scrollbar_width() {
        assert_eq!(popover_size(&json!({"w": 320, "h": 500, "g": 8, "d": 2}), 600.0, 2.0), (640, 1000));
        assert_eq!(popover_size(&json!({"w": 360, "h": 900, "g": 8, "d": 1}), 600.0, 1.0), (368, 600));
        assert_eq!(popover_size(&json!({"w": 320, "h": 900}), 600.0, 1.0), (320, 600), "overlay scrollbars (macOS) take no width");
        // Text size 150 % at 100 % display scale: 320 CSS px need 480 window px; 400 CSS px tall fit, 500 scroll
        assert_eq!(popover_size(&json!({"w": 320, "h": 400, "g": 10, "d": 1.5}), 600.0, 1.0), (480, 600));
        assert_eq!(popover_size(&json!({"w": 320, "h": 300, "g": 10, "d": 1.5}), 600.0, 1.0), (480, 450));
        assert_eq!(popover_size(&json!({"w": 320, "h": 500, "g": 10, "d": 1.5}), 600.0, 1.0), (495, 600));
        // Text size 140 % at 125 % display scale: 331 CSS px are 579.25 device px, never 579
        assert_eq!(popover_size(&json!({"w": 320, "h": 900, "g": 11, "d": 1.75}), 600.0, 1.25).0, 580);
    }

    // Chain: icon click -> popover shown -> blur. Risk: opened from the Windows 11 hidden-icons flyout, the popover
    // closes 15 ms later because the flyout retakes the foreground; or a later blur no longer closes it.
    #[test]
    fn blur_right_after_opening_keeps_the_popover() {
        assert!(blur_refocuses(Duration::from_millis(15),false));
        assert!(!blur_refocuses(Duration::from_millis(300),false) && !blur_refocuses(Duration::from_secs(5),false));
        assert!(!blur_refocuses(Duration::from_millis(15),true), "an intentional workspace handoff must not pull focus back to the tray");
    }

    // H-A2: host policy/readback cannot be confused with a project's lower ceiling or legacy writable scope.
    #[test]
    fn capacity_projects_to_resource_targets() {
        let saved = json!({"limits":{"claude":0,"codex":3},"load_gate":{"enabled":true,"cpu_max":85,"memory_min":10},"admission":{"mode":"auto","max":null}});
        let cap = json!({"host_policy":saved,"host_runtime_used":{"claude":1,"codex":2},"project_limits":{"codex":1},
            "limits":{"codex":1},"used":{"codex":1},"load_gate":{"enabled":true,"cpu_max":85,"memory_min":10},
            "sample_at":95,"sample_age_seconds":3,"cpu_percent":12.5,"memory_available_percent":40.0,
            "reasons":[{"code":"hold"},{"code":"future"},{"code":"cpu"}]});
        let group = resource_group(&cap, &saved,&json!("command-current"), &json!({"codex":1}), Some(100), None, "taskq");
        let t = group["targets"].as_array().unwrap();
        assert_eq!(t[0]["runtime_usage"][1],json!({"runtime_id":"codex","used":2,"limit":3,"limit_kind":"ceiling"}));
        assert_eq!(t[1]["current_policy"]["limits"]["codex"],1);
        assert!(t[0]["effective_revision"].is_null(),"equal settings without the host's exact receipt cannot look applied");
        assert_eq!(t[0]["supported"][0]["fields"],json!(["maximum","load_gate"]));
        assert_eq!(t[1]["supported"][0]["fields"],json!([]),"a project ceiling has no host write authority");
        assert!(t[3]["target"]["project_id"].is_null(),"runtime ceiling belongs to the host");
        assert!(t[0]["current_policy"]["mode"].is_null());
        let codes:Vec<_>=t[0]["blockers"].as_array().unwrap().iter().map(|b|b["code"].as_str().unwrap()).collect();
        assert_eq!(codes,["host","unknown","cpu"]);
        assert_eq!(t[0]["sample"]["age_seconds"],3);
        let mut stale=cap.clone();stale.as_object_mut().unwrap().remove("host_policy");
        let unknown=resource_group(&stale,&saved,&json!("command-current"),&json!({"codex":1}),Some(100),None,"taskq");
        assert!(unknown["targets"][0]["effective_revision"].is_null());
        assert!(unknown["targets"][0]["runtime_usage"][1]["used"].is_null(),"legacy project snapshot cannot prove host use");
    }

    // H-A2: a stale page cannot repurpose a legacy project operation as an edit to every project on the host.
    #[test]
    fn capacity_bridge_refuses_unqualified_host_edits() {
        for set in [json!({"mode":"auto"}),json!({"limits":{"codex":4}}),json!({"maximum":null})] {
            assert_eq!(capacity(Path::new("/nonexistent-taskq-954"),Path::new("/nonexistent-taskq-954"),&json!({"set":set})).unwrap()["state"],"rejected");
        }
    }

    // Chain: § 4 tray opens an incident report. Risk: tray opens a file outside .taskq/incidents.
    #[test]
    fn incident_report_only_opens_json_inside_its_project() {
        let root = std::env::temp_dir().join(format!("taskq-tray-incident-{}", std::process::id()));
        let dir = crate::dot(&root).join("incidents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("report.json"), "{}").unwrap();
        fs::write(root.join("private.json"), "{}").unwrap();
        fs::write(dir.join("program.exe"), "{}").unwrap();
        assert_eq!(incident_path(&root, &json!({"path": ".taskq/incidents/report.json"})), Some(dir.join("report.json")));
        for path in ["private.json", ".taskq/incidents/../../private.json", ".taskq/incidents/program.exe", ".taskq/incidents/missing.json"] {
            assert!(incident_path(&root, &json!({"path": path})).is_none(), "{path}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    // Risk: page metadata smuggles content or forges a method outside the bundled inventory.
    #[test]
    fn web_metadata_is_allowlisted_and_aggregate_identity_is_explicitly_unknown() {
        let row=json!({"operation":"js.tray.create.mode","kind":"method.aggregate","started_ms":1,"counts":{"calls":2,"finished":2,"failed":0,"unknown":0},"outcome":"ok","error":"none"});
        let accepted=web_observation(&row).unwrap();
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(),||{
            let mut calls=WebCalls::new();
            assert!(calls.metadata(&row),"missing collector must be visible");
            assert!(calls.metadata_unknown);assert!(!calls.failed,"optional metadata disabled command controls");
            assert!(!calls.metadata(&row),"same outage repeated its notification");
        });
        assert_eq!(accepted["unknown"]["trace"],"aggregate-unlinked");
        for (key,value) in [("message",json!("private")),("operation",json!("js.private-data")),("trace",json!("/private/path")),("counts",json!({"private":1})),("error",json!("raw-error"))] {
            let mut invalid=row.clone();invalid[key]=value;assert!(web_observation(&invalid).is_err(),"{key}");
        }
    }

    // A7: removed Custom resolves to Summary; unrelated settings survive a subsequent save.
    #[test]
    fn saved_custom_resolves_to_summary_without_losing_settings() {
        let install=std::env::temp_dir().join(format!("taskq-icon-{}",std::process::id()));
        fs::create_dir_all(&install).unwrap();
        let p=install.join("tray.json");
        for icon in [json!({"version":1}),json!({"version":99}),Value::Null] {
            fs::write(&p,json!({"view":"custom","font":18,"period":"custom","start":5,"end":20,"tz":"UTC","icon":icon,"unrelated":{"keep":true}}).to_string()).unwrap();
            let settings=Settings::load(&install);
            assert_eq!(settings.view,"full");assert_eq!(settings.font,18);assert_eq!(settings.range(50),(5,20));
            assert!(settings.save(&install));let saved:Value=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
            assert_eq!(saved["unrelated"],json!({"keep":true}));assert!(saved.get("icon").is_none());
        }
        fs::remove_dir_all(install).unwrap();
    }

    // #1591 A3: an observation 61-299 s old reads live in the tray, as in the CLI (`snapshot::live`); older than ttl stale.
    // Risk: a 60 s literal in the tray marks a live service unknown between idle passes.
    #[test]
    fn tray_and_cli_share_the_ttl_freshness_rule() {
        let s = json!({"observed": 1000, "tasks_age_seconds": 0, "hosts": [{"name": "mac", "updated": 1000}, "bad"],
            "tasks": [{"n": 1, "executions": [{"id": 7, "observed": 1000}]}]});
        for (age, live) in [(0, true), (61, true), (299, true), (300, true), (301, false)] {
            let mut tasks = s["tasks"].as_array().cloned().unwrap();
            let (hosts, tasks_live) = liveness(&s, Some(age), 300, &mut tasks);
            assert_eq!((tasks_live, hosts[0]["live"].clone(), tasks[0]["executions"][0]["live"].clone()), (live, json!(live), json!(live)), "{age} s");
            assert_eq!(crate::snapshot::live(age as i64, 300), live, "CLI {age} s");
            assert_eq!(hosts[1], "bad");
        }
        let mut tasks = vec![];
        assert!(!liveness(&json!({"observed": 1000}), Some(1), 300, &mut tasks).1, "missing task age is stale");
        assert!(!liveness(&s, None, 300, &mut tasks).1, "missing status file is stale");
        assert!(!crate::snapshot::live(-1, 300), "a future observation is stale");
    }

    // Chain: tray project page and settings. Risk: corrupt status.json or tray.json reads as zero/default and is reset
    #[test]
    fn corrupt_status_and_tray_json_read_unknown_and_stay() {
        let install = std::env::temp_dir().join(format!("taskq-tray-corrupt-{}", std::process::id()));
        let root = install.join("project");
        fs::create_dir_all(crate::dot(&root)).unwrap();
        // R6/M3: the native adapter preserves the same observation used by status and capacity.
        let status=json!({"observed":99,"tasks_age_seconds":3,"hosts":[{"name":"mac","updated":98}],
            "capacity":{"host_runtime_used":{"codex":2},"reservations":[]},
            "tasks":[{"n":1,"executions":[{"id":7,"role":"worker","runtime":"codex","state":"running"}]}]});
        fs::write(crate::dot(&root).join("status.json"),status.to_string()).unwrap();
        let projected=project(&root,root.to_str().unwrap());
        for key in ["observed","tasks_age_seconds","capacity"] {assert_eq!(projected[key],status[key],"{key}");}
        // #1591: rows keep every observed field and gain only the computed `live`
        let strip=|v:&Value|{let mut v=v.clone();for r in v.as_array_mut().into_iter().flatten(){r.as_object_mut().unwrap().remove("live");}v};
        assert_eq!(strip(&projected["hosts"]),status["hosts"]);
        assert_eq!(strip(&projected["tasks"][0]["executions"]),status["tasks"][0]["executions"]);
        assert_eq!(projected["tasks"][0]["local_session"],false);
        fs::write(crate::dot(&root).join("status.json"), "{\"tasks\": [{\"n\": 1").unwrap();
        let p = project(&root, root.to_str().unwrap());
        assert!(p["complete"] == false && p["tasks"] == json!([]) && p["error"].as_str().unwrap().starts_with("status.json: corrupt"), "{p}");
        assert!(!summary(&[p], (0, 1)).known, "counts unknown, never zero");
        assert_eq!(fs::read_to_string(crate::dot(&root).join("status.json")).unwrap(), "{\"tasks\": [{\"n\": 1");
        // a corrupt focus in a fresh status.json names its reason
        fs::write(crate::dot(&root).join("status.json"), r#"{"focus": null, "focus_error": "focus: not UTF-8 text; file kept"}"#).unwrap();
        assert_eq!(project(&root, root.to_str().unwrap())["error"], "focus: not UTF-8 text; file kept");
        fs::write(install.join("tray.json"), "{\"view\": \"fu").unwrap();
        let s = Settings::load(&install);
        assert!(s.view == "icon" && s.font == 16, "defaults shown");
        assert_eq!(fs::read_to_string(install.join("tray.json")).unwrap(), "{\"view\": \"fu", "a read never resets it");
        let reason = state(&install, &s, false, None, &Value::Null)["settings_error"].clone();
        assert!(reason.as_str().unwrap().starts_with("tray.json: corrupt"), "the popover names the reason: {reason}");
        assert!(HTML.contains("Settings unknown:"));
        assert!(s.save(&install));
        assert_eq!(state(&install, &s, false, None, &Value::Null)["settings_error"], Value::Null, "a save clears it");
        assert_eq!(fs::read_to_string(install.join("tray.json.corrupt")).unwrap(), "{\"view\": \"fu", "a save keeps a copy");
        assert_eq!(Settings::load(&install).font, 16);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: § 4 tray counts across projects. Risk: double counting or unknown shown as known.
    #[test]
    fn summary_counts_one_task_per_board_and_issue() {
        let p = |issues: &str, state: &str, focus: Value| json!({"issues": issues, "focus": focus, "tasks": [{"n": 1, "state": state}], "events": [[95, "close", "7"], [-250, "failed", "8"]]});
        let ps = [p("https://github.com/o/a/issues", "ask", json!([])), p("https://github.com/o/a/issues", "ask", json!([])), p("https://github.com/o/b/issues", "doing", json!(["all"]))];
        let s = summary(&ps, (-200, 100));
        assert_eq!(s.counts, [1, 1, 0, 0]);
        assert!(s.run && !summary(&ps[..2], (-200, 100)).run);
        assert_eq!(s.bins.iter().map(|b| b.counts[0]).sum::<u32>(), 2, "same attempt on two boards counts twice, on one board once");
        assert_eq!(s.bins.iter().map(|b| b.counts[1]).sum::<u32>(), 0, "outside the period");
        assert!(!s.known, "a project without a complete snapshot makes the counts unknown");
        let mut ok = ps.clone();
        for p in &mut ok { p["complete"] = json!(true); }
        assert!(summary(&ok, (-200, 100)).known);
        ok[0]["tasks"][0]["state"]=json!("waiting");ok[1]["tasks"][0]["state"]=json!("waiting");
        let blocked=summary(&ok,(-200,100));
        assert_eq!(blocked.blocked,1,"blocked tasks deduplicate by board and issue");
        assert_eq!(blocked.counts[2],0,"ready count stays separate from blocked");
    }

    // Chain: M6/D11 rolling history grid and U-A6 icon. Risk: bars drift on every refresh, or a boundary loses or moves events.
    #[test]
    fn history_grid_moves_one_bin_at_a_boundary() {
        let b = 14400 * 150_000;
        let mut ev:Vec<Value>=(1..6).map(|i|json!([b-i*14400,KINDS[(i%4) as usize],format!("e{i}")])).collect();
        ev.extend([json!([b-3700,"close","late"]),json!([b-3601,"ask","last"]),json!([b+30,"failed","new"])]);
        let at=|now:i64| {let seen:Vec<&Value>=ev.iter().filter(|e|e[0].as_i64().unwrap()<=now).collect();
            summary_at(&[json!({"issues":"u","tasks":[],"focus":["all"],"events":seen,"since":0,"complete":true})],day(now),now)};
        let icons=|s:&Summary|[(1.0,2.0),(16.0/22.0,1.0)].map(|(k,dpr)|draw(s,true,k,dpr).unwrap().png());
        let frames=[b-3600,b-1,b,b+31,b+3600];let sums:Vec<Summary>=frames.iter().map(|&t|at(t)).collect();let pngs:Vec<_>=sums.iter().map(icons).collect();
        for (i,j) in [(0,1),(3,4)] {assert_eq!(sums[i].bins,sums[j].bins);assert_eq!(pngs[i],pngs[j]);}
        let counts=|s:&Summary|s.bins.iter().map(|b|b.counts).collect::<Vec<_>>();let before=counts(&sums[1]);let after=counts(&sums[2]);
        assert_eq!(after[..5],before[1..],"whole grid advances one bin");
        assert_eq!((after[5],sums[3].bins[5].counts),([0;4],[0,1,0,0]));
        assert_ne!(pngs[1],pngs[2]);
        if let Ok(dir) = std::env::var("TASKQ_FRAMES") {
            // ponytail: evidence frames on demand, `TASKQ_FRAMES=DIR cargo test history_grid`
            for (t, p) in frames.iter().zip(&pngs) {
                for (size, png) in ["22pt@2x", "16px@1x"].iter().zip(p) {
                    fs::write(Path::new(&dir).join(format!("icon-summary-{:+}s-{size}.png", t - b)), png).unwrap();
                }
            }
        }
    }

    // Chain: § 4 tray settings input. Risk: webview input stores unknown values.
    #[test]
    fn settings_keep_only_known_values() {
        let mut s = Settings { view: "icon".into(), font: 16, period: "1d".into(), start: 0, end: 0, tz: "local".into(), pin: Pin::default(), notify: Value::Null, navigation: Value::Null, workspace_navigation:Value::Null };
        s.set(&json!({"view": "full", "font": 18, "period": "1w"}));
        s.set(&json!({"view": "x", "font": 15, "period": "2y"}));
        s.set(&json!({"period": "custom", "start": 50, "end": 50, "tz": "UTC"}));
        assert!(s == Settings { view: "full".into(), font: 18, period: "1w".into(), start: 0, end: 0, tz: "local".into(), pin: Pin::default(), notify: Value::Null, navigation: Value::Null, workspace_navigation:Value::Null });
        assert_eq!(s.range(1000), (100800 - 604800, 100800), "whole 28 h bins, the newest holds now");
        s.set(&json!({"period": "custom", "start": 40, "end": 160, "tz": "UTC"}));
        assert_eq!((s.range(1000), s.tz.as_str()), ((40, 160), "UTC"));
    }

    #[test]
    fn window_anchor_preserves_edge_and_clamps_to_work_area() {
        let area=(0,0,1000,800);
        for (edge,y,expected) in [("top",20.0,"top"),("bottom",780.0,"bottom"),("left",700.0,"bottom"),("right",100.0,"top")] {
            assert_eq!(growth_anchor("auto",edge,Some(y),area),expected);
            assert_eq!(growth_anchor("top",edge,Some(y),area),"top","explicit Down survives placement");
            assert_eq!(growth_anchor("bottom",edge,Some(y),area),"bottom","explicit Up survives placement");
        }

        assert_eq!(anchored_position((30,300),200,(320,400),area,"bottom"),(30,100));
        assert_eq!(anchored_position((30,300),400,(320,200),area,"bottom"),(30,500));
        assert_eq!(anchored_position((30,300),200,(320,400),area,"top"),(30,300));
        assert_eq!(anchored_position((30,700),200,(320,400),area,"top"),(30,400));
        assert_eq!(anchored_position((30,10),200,(320,400),area,"bottom"),(30,0));
    }

    // Chain: § 4 pinned tray window (menu.md M13). Risk: pin or position lost on restart; window placed off-screen.
    #[test]
    fn pin_and_position_per_display_survive_restart() {
        let install = std::env::temp_dir().join(format!("taskq-pin-{}", std::process::id()));
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("tray.json"), r#"{"font":18}"#).unwrap();
        let mut s = Settings::load(&install);
        assert!(!s.pin.pinned && s.pin.top && s.pin.size.is_none() && s.pin.size_anchor=="auto", "new growth defaults to actual-anchor Auto");
        for mode in ["top","bottom","fixed"] {
            fs::write(install.join("tray.json"),json!({"size_anchor":mode,"font":20}).to_string()).unwrap();
            let legacy=Settings::load(&install);
            assert_eq!(legacy.pin.size_anchor,mode,"existing explicit growth is never migrated to Auto");
            assert_eq!(legacy.font,20);
        }
        fs::write(install.join("tray.json"), r#"{"size":[-1,0],"top":false}"#).unwrap();
        let invalid = Settings::load(&install);
        assert!(invalid.pin.size.is_none() && !invalid.pin.top, "invalid size cannot hide the header or resize grip");
        s.pin = Pin { size_anchor: "bottom".into(), attached_edge: "top".into(), auto_anchor:"bottom".into(), top: false, size: Some((480.0, 600.0)), pinned: true, display: "B".into(), pos: [("A".into(), (10, 20)), ("B".into(), (-1500, 300))].into() };
        // M8/UI02: navigation shares the existing settings save, independent of release version.
        s.set(&json!({"view":"full","period":"custom","start":40,"end":160,"tz":"UTC",
            "navigation":{"schema":1,"current":{"page":"projects","taskFilter":["doing"],"scroll":120,"open":{"scope":true}},"trail":[]},
            "workspace_navigation":{"schema":1,"current":{"page":"sessions","scroll":32},"trail":[{"page":"messages"}]}}));
        assert!(s.save(&install));
        let r = Settings::load(&install);
        assert!(r == s, "restart must retain navigation and every existing appearance/window setting");
        assert_eq!(r.navigation["current"]["scroll"], 120);
        assert_eq!(r.workspace_navigation["current"]["page"], "sessions", "workspace and tray keep independent roots");
        let mut invalid_navigation = r.clone();
        invalid_navigation.set(&json!({"navigation":{"schema":2}}));
        assert!(invalid_navigation == r, "unsupported navigation cannot overwrite known settings");
        invalid_navigation.set(&json!({"navigation":{"schema":1,"current":{"page":"x".repeat(65536)}}}));
        assert!(invalid_navigation == r, "oversized IPC data cannot replace saved settings");
        invalid_navigation.set(&json!({"workspace_navigation":{"schema":2}}));
        assert!(invalid_navigation == r, "unsupported workspace state cannot overwrite either window");
        invalid_navigation.set(&json!({"workspace_navigation":{"schema":1,"current":{"page":"x".repeat(65536)}}}));
        assert!(invalid_navigation == r, "oversized workspace data cannot replace saved settings");
        assert!(r.pin == s.pin && r.font == 18);
        // M14: modes and cursors survive a restart as saved, unknown values included
        let mut s = r;
        s.notify = json!({"kinds": {"ask": "loud"}, "cursor": {"/p": "100 #7 ask 5"}});
        let st = json!({"projects": [{"root": "/p", "name": "p", "tasks": [], "events": [[100, "ask", "5", 7], [200, "failed", "6", 7]]}]});
        let mut asked = true;
        notes(&install, &mut s, &st, &mut asked, &mut notify::Bursts::new());
        let r = Settings::load(&install);
        assert_eq!((r.notify["cursor"]["/p"].as_str(), r.notify["kinds"]["ask"].as_str()), (Some("200 #7 failed 6"), Some("loud")), "cursor saved before any post");
        let area = (0, 25, 1512, 900);
        assert!(on_screen((10, 30), area) && !on_screen((-1500, 300), area) && !on_screen((1512, 30), area));
        assert_eq!(clamp((1400, 0), (320, 400), area), (1192, 25), "kept under the menu bar and inside the right edge");
        assert_eq!(clamp((100, 800), (320, 2000), area), (100, 25), "a too tall window keeps its top in");
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: § 4 tray history graph. Risk: events land in the wrong bin or period.
    #[test]
    fn history_bins_are_equal_oldest_left_on_one_scale() {
        // M-A5/M-A6 fixture: 3 of each kind in [0, 120); the first and last bins are not empty; unknown kinds drop
        let ev = json!([[0, "close", "1"], [5, "close", "2"], [119, "close", "3"], [0, "failed", "4"], [60, "failed", "5"], [119, "failed", "6"], [120, "failed", "7"], [-1, "close", "8"],
            [0, "ask", "9"], [30, "ask", "10"], [119, "ask", "11"], [10, "blocked", "12"], [60, "blocked", "13"], [119, "blocked", "14"], [50, "later", "15"]]);
        let p = json!([{"issues": "u", "tasks": [], "focus": ["all"], "events": ev, "since": -10}]);
        let s = summary(p.as_array().unwrap(), (0, 120));
        assert_eq!(s.bins.len(), 6);
        assert_eq!((s.bins[0].counts, s.bins[3].counts, s.bins[5].counts), ([2, 1, 1, 1], [0, 1, 0, 1], [1, 1, 1, 1]), "[start, end): 120 and -1 are out");
        assert_eq!((0..4).map(|k| s.bins.iter().map(|b| b.counts[k]).sum::<u32>()).collect::<Vec<_>>(), [3, 3, 3, 3]);
        assert_eq!(s.since, Some(-10));
        let ps = json!([{"issues": "u", "events": []}, {"issues": "v", "events": [], "since": 5}]);
        assert_eq!(summary(&ps.as_array().unwrap()[..1], (0, 1)).since, None, "no events cache: no since");
        assert_eq!(summary(ps.as_array().unwrap(), (0, 1)).since, Some(5));
    }

    // A8: real event identity, half-open bounds and incomplete sources feed the same renderer.
    #[test]
    fn approved_real_history_fixtures_use_one_six_bin_svg() {
        let fixture:Value=serde_json::from_str(include_str!("../docs/reviews/real-data-2026-10-22.json")).unwrap();
        let ps:Vec<Value>=fixture["projects"].as_array().unwrap().iter().map(|p|json!({"name":p["name"],"issues":p["board_issues_url"],"since":p["history_since"],"complete":true,"focus":["all"],"tasks":p["current_tasks"].as_array().unwrap().iter().map(|t|{let mut row=t.clone();row["n"]=t["number"].clone();row}).collect::<Vec<_>>(),"events":p["window_events"].as_array().unwrap().iter().map(|e|json!([e["at"],e["kind"],e["id"],e["task"]])).collect::<Vec<_>>()})).collect();
        let captured=fixture["captured_at"].as_i64().unwrap();
        let range=(fixture["window"]["start"].as_i64().unwrap(),fixture["window"]["end"].as_i64().unwrap());
        let taskq=ps.iter().find(|p|p["name"]=="taskq").unwrap();
        let day=summary_at(&[taskq.clone()],range,captured);
        assert_eq!(day.counts,[1,0,1,0],"frozen current counters keep distinct task identities");
        assert_eq!(day.bins.iter().map(|b|b.counts[0]).collect::<Vec<_>>(),[5,4,7,12,7,4]);
        assert!(day.bins.iter().all(icon::Bin::complete));assert!(day.bins[5].ongoing);
        let hour:Value=serde_json::from_str(include_str!("../docs/reviews/real-data-hour-2026-10-22.json")).unwrap();
        let hourly=summary_at(&[taskq.clone()],(hour["window_start"].as_i64().unwrap(),hour["window_end"].as_i64().unwrap()),captured);
        assert_eq!(hourly.bins.iter().map(|b|b.counts[0]).collect::<Vec<_>>(),[1,0,0,1,0,0]);
        let aggregate=summary_at(&ps,range,captured);
        assert!(aggregate.bins[..5].iter().all(|b|b.missing==["pilotq"]));assert!(aggregate.bins[5].complete());
        #[cfg(target_os="macos")]
        let font=unsafe {objc2_app_kit::NSFont::systemFontOfSize_weight(9.0,objc2_app_kit::NSFontWeightMedium)}.fontName().to_string();
        #[cfg(not(target_os="macos"))]
        let font="Segoe UI (requested; native qualification pending)".to_string();
        let mut paused=day.clone();paused.run=false;
        let mut zero=day.clone();zero.counts=[0;4];zero.blocked=0;
        for b in &mut zero.bins {b.counts=[0;4];}
        let mut failed=zero.clone();failed.bins[2].counts[1]=1;
        let mut unknown=zero.clone();unknown.bins[0].missing=vec!["synthetic missing source".into()];unknown.bins[0].counts[0]=999;
        for (name,s) in [("day",&day),("hour",&hourly),("incomplete",&aggregate),("paused",&paused),("synthetic-zero",&zero),("synthetic-failed",&failed),("synthetic-unknown",&unknown)] {
            for dark in [false,true] {
                let vector=icon::svg(&icon_data(s),dark);
                for n in [18,22,33,44,66,256] {assert_eq!(icon::raster(&vector,n).unwrap().w,n);}
                if std::env::var_os("TASKQ_ICON_EVIDENCE").is_none() {
                    let theme=if dark {"dark"}else{"light"};
                    let saved:Value=serde_json::from_slice(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/icon-history/{name}-{theme}.json"))).expect("canonical browser fixture")).unwrap();
                    assert_eq!(saved["bins"],json!(s.bins),"browser/native bins and exact counts");
                    assert_eq!(saved["peak"],icon::peak(&s.bins));assert_eq!(saved["palette"],icon::css(dark));
                    let chart=|svg:&str|svg.split(r#"<g id="chart""#).nth(1).unwrap().split(r#"</g><g id="counters">"#).next().unwrap().to_string();
                    assert_eq!(chart(saved["svg"].as_str().unwrap()),chart(&vector),"all surfaces retain canonical heights, colors and marks");
                }
                if let Ok(dir)=std::env::var("TASKQ_ICON_EVIDENCE") {
                    let dir=Path::new(&dir);fs::create_dir_all(dir).unwrap();let theme=if dark {"dark"}else{"light"};
                    fs::write(dir.join(format!("{name}-{theme}.svg")),&vector).unwrap();
                    for n in [18,44,66,256] {fs::write(dir.join(format!("{name}-{theme}-{n}.png")),icon::raster(&vector,n).unwrap().png()).unwrap();}
                    fs::write(dir.join(format!("{name}-{theme}.json")),json!({"bins":s.bins,"peak":icon::peak(&s.bins),"svg":vector,"start":s.bins[0].start,"end":s.bins[5].end,"tz":"local","since":s.since,"palette":icon::css(dark),"captured":captured,"font":font,"compact":format!("data:image/png;base64,{}",base64(&compact(s.run,dark,2.0).png()))}).to_string()).unwrap();
                }
            }
        }
        let mut duplicate=ps.clone();duplicate.push(taskq.clone());assert_eq!(summary_at(&duplicate,range,captured).bins,aggregate.bins,"same board cache never doubles events");
        let custom=summary_at(&[taskq.clone()],(1,8),captured);assert_eq!(custom.bins[0].start,1.0);assert_eq!(custom.bins[5].end,8.0);assert!(!custom.bins.iter().any(|b|b.ongoing));
    }

    // Chain: R12 tray agents row. Risk: stale probe shown or missing probe shown as known.
    #[test]
    fn agents_come_from_the_freshest_probe_or_are_unknown() {
        let row = |v: &str| json!({"runtime": "claude", "path": "/c", "version": v, "login": "not logged in", "account": null});
        let ps = [json!({"agents": {"at": 10, "rows": [row("1")]}}), json!({"agents": {"at": 20, "rows": [row("2")]}}), json!({})];
        assert_eq!(agents(&ps), json!({"at": 20, "rows": ["claude 2 · not logged in"]}));
        assert_eq!(agents(&ps[2..]), Value::Null);
    }

    // Chain: § 6 Add project from the tray. Risk: a folder is classified wrong and set up wrong.
    #[test]
    fn add_project_has_four_cases() {
        let d = std::env::temp_dir().join(format!("taskq-add-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let kind = |st: &dyn Fn(&Path) -> Result<(), String>| { let v = classify(&d, st); (v["kind"].as_str().unwrap().to_string(), v["reason"].as_str().unwrap().to_string()) };
        let pass = |_: &Path| Ok(());
        assert_eq!(kind(&pass).0, "new", "empty folder");
        fs::write(d.join("notes.txt"), "x").unwrap();
        assert_eq!(kind(&pass).0, "plain", "files without Git: not a repository");
        fs::create_dir(d.join(".git")).unwrap();
        assert_eq!(kind(&pass).0, "new", "a Git root without taskq.json");
        fs::write(d.join("taskq.json"), "{}").unwrap();
        assert_eq!(kind(&pass).0, "taskq");
        assert_eq!(kind(&|_: &Path| Err("taskq.json: repo is required".into())), ("old".into(), "taskq.json: repo is required".into()), "the verified reason");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn add_generation_ignores_late_picker_or_install_results_after_cancel() {
        let mut adding=json!({"state":"picking","generation":4});
        assert!(apply_added(&mut adding,4,4,json!({"state":"picked","generation":4,"path":"/picked"})));
        assert_eq!(adding["state"],"picked");
        adding=Value::Null;
        assert!(!apply_added(&mut adding,5,4,json!({"state":"picked","generation":4})),"cancelled picker cannot restore a selection");
        adding=json!({"state":"installing","generation":6});
        assert!(!apply_added(&mut adding,6,5,json!({"state":"installed","generation":5})),"another generation cannot claim this install");
        assert!(apply_added(&mut adding,6,6,json!({"state":"installed","generation":6})));assert_eq!(adding["state"],"installed");
        assert!(apply_added(&mut adding,6,6,json!({"state":"cancelled","generation":6})));assert!(adding.is_null(),"a matching picker cancel clears its presentation");
    }

    #[test]
    fn add_owner_keeps_picker_admission_after_cancel_until_its_result() {
        let (sender,receiver)=std::sync::mpsc::sync_channel(1);let state=std::sync::Arc::new(std::sync::atomic::AtomicU8::new(ADD_IDLE));
        let job=|generation|AddJob::Pick(QueuedAction {id:format!("add-{generation}"),root:PathBuf::new(),message:json!({"a":"add","generation":generation}),context:None,dispatch:tracing::dispatcher::get_default(Clone::clone)});
        add_submit(&sender,&state,job(4)).unwrap();
        let mut adding=json!({"state":"picking dismissed","generation":5});
        assert!(!apply_added(&mut adding,5,4,json!({"state":"picked","generation":4})),"cancelled presentation cannot take a late picker result");
        assert!(add_submit(&sender,&state,job(6)).unwrap_err().contains("already in progress"),"cancel did not release the owned picker");
        assert!(matches!(receiver.recv().unwrap(),AddJob::Pick(_)));state.store(ADD_IDLE,std::sync::atomic::Ordering::Release);
        add_submit(&sender,&state,job(6)).unwrap();
    }

    #[test]
    fn owned_effect_reports_keep_their_observable_outcomes() {
        let answer=answer_reply(PathBuf::from("registered"),"tray.1".into(),Err("registration changed".into()));
        assert_eq!(answer["answerReceipt"]["root"],"registered");assert_eq!(answer["answerReceipt"]["id"],"tray.1");assert_eq!(answer["answerReceipt"]["state"],"unknown");
        assert_eq!(resources_reply(Ok(json!({}))),json!({"resources_receipt":{"state":"submitted","reason":"Resources terminal submitted"}}));
        assert_eq!(resources_reply(Err("launcher unavailable".into()))["resources_receipt"]["state"],"unknown");
        assert_eq!(resources_reply(Err("tray.resources.submit: timed out; outcome unknown".into()))["resources_receipt"]["state"],"unknown");
    }

    // A caught effect panic is visible and cannot silently reopen native admission.
    #[test]
    fn add_outcomes_capture_failures_and_hold_unknown_ownership() {
        crate::with_test_capture(|store,_| {
            for state in ["picked","cancelled","pick failed","install failed"] {
                let (value,owner)=add_outcome("tray.add.fixture",||json!({"state":state}));
                assert_eq!(value["state"],state);assert_eq!(owner,ADD_IDLE);
            }
            let (value,owner)=add_outcome("tray.add.fixture",||panic!("owned fixture panic"));
            assert_eq!(value["state"],"add failed");assert_eq!(owner,ADD_UNAVAILABLE);
            let (sender,receiver)=std::sync::mpsc::sync_channel(1);
            let state=std::sync::Arc::new(std::sync::atomic::AtomicU8::new(owner));
            let job=AddJob::Pick(QueuedAction {id:"after-panic".into(),root:PathBuf::new(),message:json!({"generation":2}),context:None,dispatch:tracing::dispatcher::get_default(Clone::clone)});
            assert!(add_submit(&sender,&state,job).unwrap_err().contains("unavailable"));assert!(receiver.try_recv().is_err());
            let worker=std::thread::spawn(||{});let until=Instant::now()+Duration::from_secs(3);
            while !worker.is_finished() {assert!(Instant::now()<until);std::thread::yield_now();}
            let owner=AddOwner {sender,state,_worker:worker};
            assert!(owner.submit(3,None).unwrap_err().contains("worker stopped"));
            tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().unwrap().flush().unwrap());
            let page=store.export(0,0,1024).unwrap();
            let terminals:Vec<_>=page["events"].as_array().unwrap().iter().map(|r|&r["observation"]).filter(|r|r["operation"]=="taskq.operation.tray.add.fixture"&&r["kind"]=="method").collect();
            assert_eq!(terminals.len(),5);assert_eq!(terminals.iter().filter(|r|r["outcome"]=="failed").count(),3);
            assert_eq!(terminals.last().unwrap()["error"],"add-panic");
            assert!(page["events"].as_array().unwrap().iter().any(|r|r["observation"]["operation"]=="taskq.operation.tray.add.dispatch"&&r["observation"]["error"]=="add-admission"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn picker_result_keeps_roots_and_distinguishes_native_cancel_from_error() {
        use std::os::unix::process::ExitStatusExt;
        let output=|code,stdout:&[u8],stderr:&[u8]|std::process::Output {status:std::process::ExitStatus::from_raw(code),stdout:stdout.into(),stderr:stderr.into()};
        assert_eq!(pick_result(output(0,b"/\n",b"")),Ok(Some(PathBuf::from("/"))));
        assert_eq!(pick_result(output(0,b" /picked \r\n",b"")),Ok(Some(PathBuf::from(" /picked "))),"only the protocol newline is removed");
        assert!(pick_result(output(0,&[0xff,b'\n'],b"")).unwrap_err().contains("non-UTF-8"));
        assert_eq!(pick_result(output(1<<8,b"",b"execution error: User canceled. (-128)\n")),Ok(None));
        assert!(pick_result(output(1<<8,b"",b"execution error: denied. (-10004)\n")).is_err());
    }
    // Chain: canonical session -> native row -> shared task link (#1204, #1565). Risk: a Codex thread opens off its host
    // instead of its DashQ progress page, a remote Claude row loses its account-scoped link, or a starting observation opens anything.
    #[test]
    fn project_row_links_claude_anywhere_codex_only_locally() {
        let url = "https://claude.ai/code/session_014abc";
        let mut row = json!({"n": 938, "session": "codex:valid-thread win"});
        task_session(Path::new("."), "win", None, &mut row);
        assert_eq!((row["session_url"].clone(), row["runtime"].clone()), (json!("codex://threads/valid-thread"), json!("codex")), "local codex: deep link");
        let mut codex = json!({"session": "codex:valid-thread mac"});
        task_session(Path::new("."), "win", None, &mut codex);
        assert!(codex["session_url"].is_null());
        assert_eq!(codex["session_reason"], "Codex session opens only on mac", "remote codex: no link, reason");
        let mut paged = json!({"n": 7, "session": "codex:valid-thread mac"});
        task_session(Path::new("."), "win", Some("https://d/projects#board-o%2Fr-task-"), &mut paged);
        assert_eq!(paged["session_url"], "https://d/projects#board-o%2Fr-task-7", "remote codex: DashQ progress page, never codex://");
        let mut remote = json!({"session": "claude:valid-thread mac", "session_url": url});
        task_session(Path::new("."), "win", None, &mut remote);
        assert_eq!((remote["local_session"].clone(), remote["session_url"].clone()), (json!(false), json!(url)), "remote claude: link");
        let mut forged = json!({"session": "claude:valid-thread mac", "session_url": "https://evil.example/session_1"});
        task_session(Path::new("."), "win", None, &mut forged);
        assert!(forged["session_url"].is_null(), "only a canonical claude.ai URL");
        let mut unpublished = json!({"session": "claude:valid-thread mac"});
        task_session(Path::new("."), "win", None, &mut unpublished);
        assert!(unpublished["session_url"].is_null() && unpublished["session_reason"].is_null());
        let mut invalid = json!({"session": "claude:bad/path win", "session_url": url});
        task_session(Path::new("."), "win", None, &mut invalid);
        assert!(invalid["session_url"].is_null());
        let mut local = json!({"session": "claude:valid-thread win"});
        task_session(Path::new("."), "win", None, &mut local);
        assert!(local["session_url"].is_null(), "a local UUID does not prove remote registration");
        let mut starting = json!({"session": "claude:- mac", "session_url": url});
        task_session(Path::new("."), "win", None, &mut starting);
        assert!(starting["session_url"].is_null(), "starting: no link");
        task_session(Path::new("."), "win", None, &mut Value::Null);
    }
}
