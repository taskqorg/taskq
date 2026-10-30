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
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Window, WindowBuilder};
use tray_icon::{Icon, MouseButton, MouseButtonState, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent};
use wry::{WebView, WebViewBuilder};

use crate::icon::{self, Rgba, compact};
use crate::notify;

const HTML: &str = include_str!(concat!(env!("OUT_DIR"),"/web/tray.html"));
const WORKSPACE_HTML: &str = include_str!(concat!(env!("OUT_DIR"),"/web/workspace.html"));
/// R22: the Settings window's web surface; `#` keeps it apart from detached conversation ids.
const SETTINGS_SURFACE: &str = "#settings";
const VIEWS_SURFACE: &str = "#views";
const FONTS: [u64; 4] = [14, 16, 18, 20];
use crate::display::PERIODS;

/// False when another tray wrote the heartbeat `<install>/tray` in the last 10 s (it ticks every 2 s): of two trays
/// started at once, the first to see the other's pid exits.
fn alone(install: &Path) -> bool {
    let hb = install.join("tray");
    let other = fs::read_to_string(&hb).ok().and_then(|p| p.trim().parse::<u32>().ok()).is_some_and(|p| p != std::process::id());
    !(other && crate::file_age(&hb).is_some_and(|a| a < 10))
}

/// M15: one TaskQ app per host across install folders: the tray holds this OS temp lock for its life.
fn host_lock() -> Option<fs::File> {
    let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(std::env::temp_dir().join("taskq-tray.lock")).ok()?;
    f.try_lock().ok().map(|_| f)
}
/// M15: a second Open asks the running tray to show its window; its next tick takes the request.
fn raise_request() -> PathBuf { std::env::temp_dir().join("taskq-tray.raise") }

/// M15 Quit and Restart name the active work they wait for (doing and review tasks); `phase` is `Quitting` or
/// `Restarting` once one is under way. App text says TaskQ, never service; menu labels stay within 32 characters.
fn quit_label(st: &Value, phase: Option<&str>) -> String {
    let work: Vec<String> = st["projects"].as_array().into_iter().flatten().flat_map(|p| p["tasks"].as_array().into_iter().flatten())
        .filter(|t| matches!(t["state"].as_str(), Some("doing" | "review"))).map(|t| format!("#{}", t["n"])).collect();
    if work.is_empty() { return phase.map_or("No active work".into(), |p| format!("{p} TaskQ")); }
    let mut s = phase.map_or("Quit waits for".into(), |p| format!("{p} after"));
    for (i, w) in work.iter().enumerate() {
        if s.len() + w.len() + 1 > 26 { s += &format!(" +{}", work.len() - i); break; }
        s += " "; s += w;
    }
    s
}

enum Ev {
    Tick,
    /// M15: Quit TaskQ from the tray menu or Cmd+Q; a refused Quit returns its reason.
    Quit,
    /// M15: Restart TaskQ drain-stops as Quit, then starts a new tray that runs Open.
    Restart,
    QuitFailed(String),
    /// M15: a second Open shows the running tray's window.
    Raise,
    Ipc(String),
    /// #1819: the shared pending store changed; every window gets it at once, not on the next tick.
    Sync,
    Workspace(String, String),
    /// R22: IPC from the one Settings window (the tray page as its `settings` surface).
    Settings(String),
    /// P-W4: IPC from the one View window (the tray page as its `views` surface).
    Views(String),
    WorkspaceEvaluated(String, u64, &'static str),
    Evaluated(u64, &'static str),
    Click(Rect),
    Shot,
    Shot2,
    /// Add project: the generation fences a late picker or install result.
    Added(u64, Value),
    Answer(Value),
    /// D4/D7 dialogue read receipt for one workspace surface.
    Dialogue(String, Value),
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
    opacity: u64,
    opacity_mode: String,
    background_opacity: u64,
    opacity_error: Option<String>,
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
        let mut s = Settings { view: "icon".into(), font: 16, opacity: 100, opacity_mode: "whole".into(), background_opacity: 100, opacity_error: None, period: "1d".into(), start: 0, end: 0, tz: "local".into(), pin,
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
        if let Some(x) = v["opacity"].as_u64().filter(|x| (60..=100).contains(x)) { self.opacity = x; }
        if let Some(x) = v["opacity_mode"].as_str().filter(|x| ["whole","background"].contains(x)) { self.opacity_mode = x.into(); }
        if let Some(x) = v["background_opacity"].as_u64().filter(|x| *x <= 100) { self.background_opacity = x; }
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

    fn native_opacity(&self) -> u64 { if self.opacity_mode == "background" { 100 } else { self.opacity } }

    /// `[start, end)` of the period at `now`.
    fn range(&self, now: i64) -> (i64, i64) {
        if self.period == "custom" {
            return (self.start, self.end);
        }
        rolling(now, PERIODS.iter().find(|p| p.0 == self.period).map_or(86400, |p| p.1))
    }

    fn save(&self, install: &Path) -> bool {
        let fields = json!({"view": self.view, "font": self.font, "opacity": self.opacity, "opacity_mode": self.opacity_mode, "background_opacity": self.background_opacity, "period": self.period, "start": self.start, "end": self.end, "tz": self.tz,
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

// M1.1/M1.2: reject malformed appearance IPC before applying any part of the request.
fn appearance_input(v: &Value) -> Result<(), String> {
    for (key,min,max) in [("opacity",60,100),("background_opacity",0,100)] {
        if v.get(key).is_some() && !v[key].as_u64().is_some_and(|x| (min..=max).contains(&x)) {
            return Err(format!("{key} refused: use an integer from {min} to {max}"));
        }
    }
    if v.get("opacity_mode").is_some() && !v["opacity_mode"].as_str().is_some_and(|x| ["whole","background"].contains(&x)) {
        return Err("Opacity mode refused: use whole or background".into());
    }
    Ok(())
}

/// M1.1: strengthen text without changing the tray icon's palette or geometry.
fn menu_palette(dark:bool,opacity:u64)->String {
    if opacity==100 {return icon::css(dark);}
    let p=icon::palette(dark);
    [("work",p.work),("ask",p.ask),("failed",p.failed),("ready",p.muted),("muted",p.muted),("review",p.review)].iter().map(|(key,c)| {
        let channel=|v:u8|if dark {(u64::from(v)*10+255*90+50)/100} else {(u64::from(v)+1)/2};
        format!("--{key}:#{:02X}{:02X}{:02X}",channel(c[0]),channel(c[1]),channel(c[2]))
    }).collect::<Vec<_>>().join(";")
}

#[cfg(target_os="macos")]
fn window_opacity(window:&Window,percent:u64)->Result<(),String> {
    use tao::platform::macos::WindowExtMacOS;
    // SAFETY: the event loop owns this live NSWindow and calls us on its UI thread.
    let window=unsafe {&*(window.ns_window() as *const objc2_app_kit::NSWindow)};
    let alpha=percent as f64/100.0;
    window.setAlphaValue(alpha);
    if (window.alphaValue()-alpha).abs()>0.000001 {return Err("NSWindow alpha readback differs".into());} Ok(())
}

#[cfg(windows)]
fn window_opacity(window:&Window,percent:u64)->Result<(),String> {
    use tao::platform::windows::WindowExtWindows;
    use windows::Win32::{Foundation::{HWND,COLORREF,GetLastError,SetLastError,WIN32_ERROR},UI::WindowsAndMessaging::*};
    let hwnd=HWND(window.hwnd() as *mut _);
    // SAFETY: a live, owned window on the UI thread; existing style bits are preserved.
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let style=GetWindowLongPtrW(hwnd,GWL_EXSTYLE);
        if style==0 && GetLastError().0!=0 {return Err(format!("window style: {}",GetLastError().0));}
        if percent==100 && style & WS_EX_LAYERED.0 as isize==0 {return Ok(());}
        let (mut key,mut flags)=(COLORREF(0),LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
        if style & WS_EX_LAYERED.0 as isize!=0 {
            GetLayeredWindowAttributes(hwnd,Some(&mut key),None,Some(&mut flags)).map_err(|e|e.to_string())?;
        }
        if style & WS_EX_LAYERED.0 as isize==0 {
            SetLastError(WIN32_ERROR(0));
            if SetWindowLongPtrW(hwnd,GWL_EXSTYLE,style|WS_EX_LAYERED.0 as isize)==0 && GetLastError().0!=0 {return Err(format!("layered window: {}",GetLastError().0));}
        }
        let alpha=((percent*255+50)/100) as u8;
        flags|=LWA_ALPHA;
        SetLayeredWindowAttributes(hwnd,key,alpha,flags).map_err(|e|e.to_string())?;
        let (mut actual,mut actual_key,mut actual_flags)=(0,COLORREF(0),LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
        GetLayeredWindowAttributes(hwnd,Some(&mut actual_key),Some(&mut actual),Some(&mut actual_flags)).map_err(|e|e.to_string())?;
        if actual!=alpha || actual_key!=key || actual_flags!=flags {return Err("window alpha readback differs".into());}
    } Ok(())
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
    if key.len()>2048 || !local.is_object() || !local["draft"].is_string() {
        return Err("Workspace draft is invalid; not saved".into());
    }
    let mut f=requests_file_checked(install)?;
    if !f["answers"].is_object(){f["answers"]=json!({});}
    f["answers"][key]=crate::reply_files::store_local(install,local)?;
    crate::conversations::persist(install,&f).map_err(|e|format!("Workspace draft was not saved: {e}"))
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
    let files=crate::reply_files::draft_paths(install,&op["files"])?;
    if !files.is_empty() && (op.get("choice").is_some()||control.is_some()||action==Some("close")) {return Err("Attachments require a text answer, steer or requeue; remove files before this action".into());}
    let mut args: Vec<String> = if let Some((name, command, text, _)) = control {
        if op.get("choice").is_some() || op.get("text").is_some() { return Err(format!("{name} takes no choice or text")); }
        let mut args = vec![command.to_string(), n.to_string()];
        if !text.is_empty() { args.extend(["--text".into(), text.to_string()]); }
        args
    } else if let Some(action) = action {
        if op.get("choice").is_some() { return Err("fallback actions cannot include a choice".into()); }
        let (command, text) = match action {
            "requeue" => {
                let reason = op["text"].as_str().filter(|s| !s.trim().is_empty()||!files.is_empty()).ok_or("type a requeue reason, then send again")?;
                ("requeue", format!("Reason: {reason}"))
            }
            // #1785: after another actor's answer won the race, the owner's kept draft steers the running worker
            "steer" => ("steer", op["text"].as_str().ok_or("supply steer text")?.to_string()),
            "close" if op.get("text").is_none() => ("later", "Reason: Owner requested cancellation without publication from the tray.".into()),
            _ => return Err("unsupported tray action".into()),
        };
        if command != "requeue" && (files.is_empty()||!text.trim().is_empty()) { crate::task_text(command, &text)?; }
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
                if files.is_empty()||!text.trim().is_empty(){crate::task_text("answer", text)?;}
                args.extend([n.to_string(), "--text".into(), text.into()]);
            }
            _ => return Err("supply one choice or literal text".into()),
        }
        args
    };
    for file in files {args.extend(["--file".into(),file]);}
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
        if op["action"] == "steer" {
            return if matches!(row["state"].as_str(), Some("held" | "ready" | "waiting" | "doing" | "review" | "ask" | "later" | "failed")) { Ok(()) }
                else { Err("task is not open; steer was not saved".into()) };
        }
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
            if c["consequential"] == true { return Err("close/publication/approval requires review on the board".into()); }
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
        // #1662 A4: the first step applied, so the operation is partial, never a plain rejection or unknown
        r["outcome"] = json!("partial");
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
        let out = answer_cli(root, &["status".into(), "--json".into()], Duration::from_secs(30))?;
        let snapshot = serde_json::from_slice(&out).map_err(|_| "Invalid status JSON; refresh the project")?;
        answer_snapshot(&snapshot, m)?;
        Ok((root, args))
    };
    let (root, args) = match prepare() {
        Ok(c) => c,
        Err(e) => return response("rejected", &e, "Refresh the question or open it on the board."),
    };
    let r = answer_dispatch(m, &args, |args| {
        match answer_cli(root, args, Duration::from_secs(60)) {
            Ok(out) => serde_json::from_slice(&out).unwrap_or_else(|_| json!({"task": m["op"]["task"], "op_id": args[args.len()-3], "outcome": "unknown", "reason": "Invalid CLI receipt JSON", "next": "Check the board; retry the same operation id."})),
            Err(e) => json!({"task": m["op"]["task"], "op_id": args[args.len()-3], "outcome": "unknown", "reason": e, "next": "Check the board; retry the same operation id."}),
        }
    });
    // #1819: an accepted write rereads its task at once; the readback feeds the shared pending store
    if r["outcome"] == "accepted" && let Some(n) = m["op"]["task"].as_u64() {
        if let Err(e) = crate::ask_reread(root, n) { eprintln!("taskq tray: {e}"); }
    }
    let mut out = answer_receipt(m, &r);
    out["sync"] = json!({"state": r["state"], "revision": r["revision"]});
    out
}

/// § 5: keep text and JSON outside bounded process pipes; only the child lifetime/output diagnostics use CoreQ limits.
fn answer_cli(root: &Path, args: &[String], budget: Duration) -> Result<Vec<u8>, String> {
    let dir = std::env::temp_dir().join(format!("taskq-reply-{}", crate::runtime::uuid()));
    let mut options = fs::DirBuilder::new();
    #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; options.mode(0o700); }
    options.create(&dir).map_err(|e| format!("Cannot prepare reply transport: {e}"))?;
    let input_path = dir.join("input");
    let output_path = dir.join("receipt.json");
    let result = (|| {
        let (mut argv, input) = answer_input(args, &input_path);
        if let Some(input) = input { fs::write(&input_path, input).map_err(|e| format!("Cannot write reply file: {e}"))?; }
        let mut files=vec![];
        while let Some(i)=argv.iter().position(|s|s=="--file") {argv.remove(i);files.push(argv.remove(i));}
        if !files.is_empty(){let path=dir.join("files.json");fs::write(&path,serde_json::to_vec(&files).map_err(|e|e.to_string())?).map_err(|e|format!("Cannot write attachment list: {e}"))?;argv.extend(["--files-from".into(),path.to_string_lossy().into_owned()]);}
        argv.extend(["--json-file".into(), output_path.to_string_lossy().into_owned()]);
        let out = crate::platform::output("tray.answer-command", owner_command(root)?.args(&argv), budget)?;
        fs::read(&output_path).map_err(|e| if out.stderr.is_empty() { format!("JSON receipt unavailable: {e}") } else { String::from_utf8_lossy(&out.stderr).into_owned() })
    })();
    for path in [&input_path, &output_path, &output_path.with_extension("tmp"),&dir.join("files.json")] { let _ = fs::remove_file(path); }
    let _ = fs::remove_dir(&dir);
    result
}

/// § 5: file transport avoids OS argument and bounded process-exchange input limits.
fn answer_input(args: &[String], path: &Path) -> (Vec<String>, Option<String>) {
    let mut argv = args.to_vec();
    let input = if matches!(args.first().map(String::as_str), Some("answer" | "steer" | "requeue")) && let Some(i) = argv.iter().position(|a| a == "--text") {
        argv[i] = "--text-file".into();
        Some(std::mem::replace(&mut argv[i + 1], path.to_string_lossy().into_owned()))
    } else { None };
    (argv, input)
}

/// The tray receipt of CLI receipt `r`. #1785: a stale one speaks plain words, never revisions or CLI commands, and
/// carries who answered first.
fn answer_receipt(m: &Value, r: &Value) -> Value {
    let outcome = r["outcome"].as_str().unwrap_or("unknown");
    // § 5 late answer: a stale or rejected receipt that names the answer that won shows only that answer, no revision or CLI advice.
    let won = r["answered"].is_object();
    let (reason, next) = if won && outcome != "accepted" { ("", "") } else if outcome == "stale" {
        ("The task changed before this action arrived; nothing sent", "")
    } else {
        (r["reason"].as_str().filter(|s| !s.is_empty()).or_else(|| r["state"].as_str()).unwrap_or(""), r["next"].as_str().unwrap_or(""))
    };
    let mut out = json!({"answerReceipt": {"root": m["root"], "id": m["op"]["id"], "state": outcome, "reason": reason, "next": next}});
    if won {
        out["answerReceipt"]["answered"] = json!({"by": r["answered"]["by"], "text": r["answered"]["text"]});
    }
    if r["note"] == true { out["answerReceipt"]["note"] = json!(true); }
    out
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

/// § 2 visual evidence: each ask image the service copied becomes a `src` data URL; without a copy the card keeps the link.
pub(crate) fn inline_images(root: &Path, task: &mut Value) {
    for e in task["question"]["evidence"].as_array_mut().into_iter().flatten().filter(|e| e["image"] == true) {
        let Some(url) = e["url"].as_str() else { continue };
        let Ok(bytes) = fs::read(crate::snapshot::image_copy(root, url)) else { continue };
        // ponytail: the page rebuilds on each refresh, so copies over 2 MB stay links; serve files if owners need them inline
        if bytes.len() > 2 << 20 { continue }
        if let Some(kind) = crate::board::image_kind(&bytes) { e["src"] = json!(format!("data:image/{kind};base64,{}", base64(&bytes))); }
    }
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
    let saved = crate::display::saved(&d);
    let (text, events_error) = match fs::read_to_string(d.join("events")) {
        Ok(t) => (t, None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), Some("no events file".to_string())),
        Err(e) => (String::new(), Some(format!("events unreadable: {e}"))),
    };
    let since = text.lines().next().and_then(|l| l.strip_prefix("since ")?.parse::<i64>().ok());
    // #1712 A3: the Source line names why a project's history is unknown
    let events_error = events_error.or_else(|| since.is_none().then(|| "events file has no since line".to_string()));
    // `[time, kind, attempt, n]`, a close with its § 7 outcome text as a 5th item
    let events: Vec<Value> = text.lines().skip(1).filter_map(|l| {
        let w: Vec<&str> = l.splitn(5, ' ').collect();
        let mut e = json!([w.first()?.parse::<i64>().ok()?, w.get(2)?, w.get(3)?, w.get(1)?.strip_prefix('#')?.parse::<u64>().ok()?]);
        if let Some(o) = w.get(4) { e.as_array_mut()?.push(json!(o)); }
        Some(e)
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
    for task in &mut tasks { task_session(r, cap_host.as_str().unwrap_or(""), page.as_deref(), task); inline_images(r, task); }
    let installation = fs::read_to_string(install.join("installation-id")).ok().filter(|i| i.len() == 64 && i.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    let id: String = crate::project_name(r).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(64).collect();
    let observed_at=s["at"].as_i64().filter(|at|crate::snapshot::live(now()-*at,ttl)).and(age.map(|a|now()-a as i64));
    let resources = resource_group(&s["capacity"], &cap_saved,&cap_revision, &project_limits, observed_at, installation, &id);
    let mut out = json!({"root": root, "name": s["project"].as_str().map(String::from).unwrap_or_else(|| crate::project_name(r)),
        "issues": s["issues"].as_str().unwrap_or(""), "board_kind": board_kind,
        "delivery": crate::load(r).ok().map(|(c,_)|json!({"preset":c.delivery,"revision":c.delivery_revision})),
        "log": s["log"].is_string(), "alive": live, "age": age, "ttl": ttl,
        "complete": crate::snapshot::complete(&s) && live,
        "error": bad.map(Value::from).or_else(|| [&s["error"], &s["focus_error"]].into_iter().find(|e| e.is_string()).cloned()).unwrap_or(Value::Null), "focus": s["focus"].as_array().cloned().unwrap_or_default(), "saved": saved, "labels": crate::status_labels(r),
        "focus_command": s["focus_command"],"service_pid":s["pid"],"host_instance":s["host_instance"],"restart_waits":s["restart_waits"],"stop_progress":s["stop_progress"],"uninstall_review":crate::lifecycle::uninstall_review(install,"review",Some(r)).unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})), "request": s["request"], "since": since, "events": events, "tasks": tasks, "host": s["host"], "version": s["version"], "update": s["update"],
        "hosts": hosts, "observed": s["observed"], "tasks_age_seconds": s["tasks_age_seconds"], "tasks_live": tasks_live, "capacity": s["capacity"], "resources": resources, "cap_host": cap_host, "cap_saved": cap_saved, "agents": s["agents"], "access": s["access"],
        "incident": s["incident"], "incident_line": s.get("incident").map(crate::incident::line), "incident_notice": s["incident_notice"],
        "incident_repo": crate::platform::RELEASE_REPO});
    out["events_error"] = json!(events_error);
    out["incoming"] = s["incoming"].clone();
    // #1819: the stale line compares the board read age with the service poll
    out["poll"] = s["poll"].clone();
    out["reports"] = crate::reports::freshness(s["reports"].clone(),now(),ttl,bad_report_reason(&s,live));
    out
}

fn bad_report_reason(s:&Value,live:bool)->Option<&str> {
    if !live {Some("Service status missing or stale; report collection unknown")} else {s["error"].as_str()}
}

/// #1591: the page gets each observation's live state from the one Rust rule. Task, host and execution rows age by
/// the board read (`tasks_age_seconds`) plus the `status.json` file age `age`.
fn liveness(s: &Value, age: Option<u64>, ttl: i64, tasks: &mut [Value]) -> (Value, bool) {
    let elapsed = age.zip(s["tasks_age_seconds"].as_i64()).map(|(a, t)| a as i64 + t);
    let row_live = |row: &mut Value, at: &str| if let Some(o) = row.as_object_mut() {
        let a = elapsed.zip(s["observed"].as_i64()).zip(o.get(at).and_then(Value::as_i64)).map(|((e, obs), at)| obs - at + e);
        o.insert("live".into(), json!(a.is_some_and(|a| crate::snapshot::live(a, ttl))));
        o.insert("age".into(), json!(a));
        o.insert("age_text".into(), json!(a.filter(|a| *a >= 0).map(crate::display::age)));
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
    object.insert("native_session_url".into(), Value::Null);
    object.insert("progress_url".into(), Value::Null);
    object.insert("native_session_reason".into(), json!("No verified native session target"));
    object.insert("progress_reason".into(), json!("No read-only progress target published"));
    let session = row["session"].as_str().unwrap_or("").to_string();
    let words: Vec<&str> = session.split_whitespace().collect();
    let Some((rt, id)) = words.first().and_then(|s| s.split_once(':')) else { return };
    let host = words.get(1).copied().unwrap_or("");
    let own = !local.is_empty() && host == local;
    row["runtime"] = json!(rt);
    row["host"] = json!(host);
    row["local_session"] = json!(own);
    // M3/UX30-10: native and read-only progress are distinct adapter observations.
    if !own {
        row["progress_url"] = json!(page.map(|p| format!("{p}{}", row["n"].as_u64().unwrap_or(0))));
    }
    let a = crate::runtime::of(rt);
    if !a.remote && !own {
        if a.codex {
            match page {
                Some(p) => row["session_url"] = json!(format!("{p}{}", row["n"].as_u64().unwrap_or(0))),
                None => row["session_reason"] = json!(format!("Codex session opens only on {host}")),
            }
            row["native_session_reason"] = json!(format!("Codex session opens only on {host}"));
        }
        return;
    }
    let (_, url) = crate::runtime::session_link(root, own, rt, id, row["n"].as_u64().unwrap_or(0), row["attempt"].as_u64().unwrap_or(0), row["role"].as_str().unwrap_or(""), published);
    row["session_url"] = json!(url);
    row["native_session_url"] = json!(url);
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

/// Retained event-detail order (M6); incoming has separately versioned board coverage.
const KINDS: [&str; 4] = { let s = crate::display::SERIES; [s[0].0, s[1].0, s[2].0, s[3].0] };

fn summary(ps: &[Value], (start, end): (i64, i64)) -> Summary {
    summary_at(ps, (start, end), now())
}

fn summary_at(ps: &[Value], (start, end): (i64, i64), now:i64) -> Summary {
    let mut bins:Vec<icon::Bin> = (0..6).map(|i| {
        let a=start as f64+(end as f64-start as f64)*i as f64/6.0;
        let b=start as f64+(end as f64-start as f64)*(i+1) as f64/6.0;
        let mut missing:Vec<String>=ps.iter().filter(|p|p["since"].as_i64().is_none_or(|t|t as f64>a)).map(|p|p["name"].as_str().or(p["issues"].as_str()).unwrap_or("unknown source").to_string()).collect();
        if a>now as f64 {missing.push("future interval".into());}
        let mut incoming_missing:Vec<String>=ps.iter().filter_map(|p| {
            let source=p["name"].as_str().or(p["issues"].as_str()).unwrap_or("unknown source");
            let v=&p["incoming"];
            let identity=format!("{}/0",p["issues"].as_str().unwrap_or("").trim_end_matches('/'));
            let reason=if !crate::incoming::valid(v,&identity) {Some(v["reason"].as_str().unwrap_or("New not collected or source changed").to_string())}
                else if v["missing"].as_array().is_none_or(|m|!m.is_empty()) {Some(format!("New incomplete: {}",v["missing"]))}
                else if v["start"].as_i64().is_none_or(|t|t as f64>a) || v["end"].as_i64().is_none_or(|t|(t as f64)<b.min(now as f64)) {Some("New outside verified coverage".to_string())}
                else {None};
            let another=ps.iter().any(|other|other["issues"]==p["issues"] && crate::incoming::valid(&other["incoming"],&identity)
                && other["incoming"]["missing"].as_array().is_some_and(|m|m.is_empty())
                && other["incoming"]["start"].as_i64().is_some_and(|t|t as f64<=a)
                && other["incoming"]["end"].as_i64().is_some_and(|t|t as f64>=b.min(now as f64)));
            reason.filter(|_|!another).map(|r|format!("{source}: {r}"))
        }).collect();
        incoming_missing.sort();incoming_missing.dedup();
        icon::Bin {start:a,end:b,counts:[0;4],incoming:0,incoming_missing,missing,ongoing:a<=now as f64&&(now as f64)<b}
    }).collect();
    let mut ev = std::collections::HashSet::new();
    for p in ps {
        let identity=format!("{}/0",p["issues"].as_str().unwrap_or("").trim_end_matches('/'));
        let incoming=p["incoming"]["events"].as_array().filter(|_|crate::incoming::valid(&p["incoming"],&identity));
        for e in p["events"].as_array().into_iter().flatten().filter(|e|e[1]!="new").chain(incoming.into_iter().flatten()) {
            let (t, kind) = (e[0].as_i64().unwrap_or(0), e[1].as_str().unwrap_or(""));
            let k = KINDS.iter().position(|x| *x == kind);
            if k.is_none() && kind!="new" {continue;}
            if end > start && t >= start && t < end && ev.insert(format!("{} {} {} {kind}", p["issues"], e[3], e[2])) {
                let bin=&mut bins[(((t as i128-start as i128)*6/(end as i128-start as i128)) as usize).min(5)];
                let count=if let Some(k)=k {&mut bin.counts[k]} else {&mut bin.incoming};
                *count=count.saturating_add(1);
            }
        }
    }
    let since = ps.iter().filter_map(|p| p["since"].as_i64()).max();
    // #1592: the display model's one deduplication and partial rule
    let (t, partial, _) = crate::display::tally(ps);
    let run = ps.iter().any(|p| crate::display::mode(p["focus"].as_array().is_some_and(|f| !f.is_empty()), false) == "run");
    Summary { blocked: t["waiting"], counts: [t["doing"] + t["review"], t["ask"], t["ready"], t["failed"]], known: partial.is_empty(), run, bins, since }
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
                let operation=match job.message["a"].as_str() {Some("reports")=>"tray.reports.execute",Some("answer")=>"tray.answer.execute",Some("resources")=>"tray.resources.execute",Some("completed")=>"tray.history.execute",Some("recovery-read")=>"tray.recovery.read",Some("recovery-apply")=>"tray.recovery.apply",Some("restart")=>"tray.restart.execute",Some("stop")=>"tray.stop.execute",Some("uninstall")=>"tray.uninstall.execute",_=>operation};
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
        let name=match action {"reports"=>"tray.reports.dispatch","answer"=>"tray.answer.dispatch","completed"=>"tray.history.dispatch","resources"=>"tray.resources.dispatch","recovery-read"|"recovery-apply"=>"tray.recovery.dispatch","pause"=>"tray.pause.dispatch","resume"=>"tray.resume.dispatch","focus"=>"tray.focus.dispatch","capacity"=>"tray.capacity.dispatch","restart"=>"tray.restart.dispatch","stop"=>"tray.stop.dispatch","uninstall"=>"tray.uninstall.dispatch",_=>"tray.invalid.dispatch"};
        coreq::telemetry::capture::operation(name,|| {
            if !self.allowed.contains(&action) {return Err(("invalid-command","Unsupported queued command"));}
            if id.is_empty() || id.len()>64 || !id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'.')) {
                return Err(("invalid-id","Invalid command correlation id; action not submitted"));
            }
            let mut frozen=message.clone();frozen.as_object_mut().map(|m|m.remove("context"));
            let bytes=serde_json::to_vec(&frozen).map_err(|_|("request-too-large","Command payload too large; action not submitted"))?;
            // § 5: the envelope stays bounded; a person's literal reply has no length cap.
            let reply_bytes=if action=="answer" && frozen["op"]["text"].is_string() {frozen["op"]["text"].to_string().len()} else {0};
            if bytes.len().saturating_sub(reply_bytes)>20<<10 {return Err(("request-too-large","Command payload too large; action not submitted"));}
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
/// MP14 Completed: one board history page through the owner CLI; the tray itself stays token-free.
fn queued_history(_install:&Path,root:&Path,m:&Value)->crate::Res<Value> {
    if m["transcript"]!=true {return crate::reports::cached_history(root,m);}
    let mut c=owner_command(root)?;c.args(history_args(m));
    let out=crate::platform::output("tray.history-read",&mut c,Duration::from_secs(60))?;
    serde_json::from_slice(&out.stdout).map_err(|_|String::from_utf8_lossy(&out.stderr).lines().find(|l|!l.trim().is_empty()).unwrap_or("board history read failed").to_string())
}

/// §7 Reports use the owner CLI, the registered project and its board token. Never trust task data as authority.
fn queued_reports(_install:&Path,root:&Path,m:&Value)->crate::Res<Value> {
    let action=m["action"].as_str().ok_or("report action missing")?;
    if action=="list" {return crate::reports::cached_list(root,m["page"].as_u64().unwrap_or(1) as usize,None);}
    if action=="open" {return crate::reports::cached_open(root,m["n"].as_u64().ok_or("report task missing")?,m["sha"].as_str().ok_or("report SHA missing")?);}
    if !["read","accept","followup","reopen","later","drop","make"].contains(&action) {return Err("unsupported result action".into());}
    let n=m["n"].as_u64().filter(|n|*n>0).ok_or("report task missing")?;
    let mut command=owner_command(root)?;
    command.arg("reports").arg(n.to_string()).args(["--action",action,"--sha",m["sha"].as_str().ok_or("report SHA missing")?]);
    if let Some(reason)=m["text"].as_str() {command.args(["--text",reason]);}
    command.arg("--json");
    let out=crate::platform::output("tray.reports-command",&mut command,Duration::from_secs(180))?;
    if !out.status.success() {return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());}
    serde_json::from_slice(&out.stdout).map_err(|_|"report receipt invalid; refresh before retrying".into())
}
fn history_args(m:&Value)->Vec<String> {
    let mut a=vec!["status".into(),"--closed".into(),"--page".into(),m["page"].as_u64().unwrap_or(1).max(1).to_string()];
    if let Some(n)=m["task"].as_u64().filter(|n|*n>0) {a.extend(["--task".into(),n.to_string()]);}
    if m["transcript"]==true {a.push("--transcript".into());if let Some(b)=m["before"].as_u64(){a.extend(["--before".into(),b.to_string()]);}}
    if let Some(t)=m["since"].as_i64() {a.extend(["--since".into(),t.to_string()]);}
    a
}
/// One answer operation as the queue sees it: correlated by its op id, its draft side channel already saved apart.
/// #1662 A1: a readback or Check delivery resend omits or changes `local`; with it the same op read as id-mismatch.
fn answer_request(m:&Value)->Value {let mut request=m.clone();request["id"]=m["op"]["id"].clone();request.as_object_mut().map(|r|r.remove("local"));request}
fn answer_reply(root:PathBuf,id:String,result:crate::Res<Value>)->Value {result.unwrap_or_else(|error|json!({"answerReceipt":{"root":root,"id":id,"state":"unknown","reason":error,"next":"Check delivery with the same operation"}}))}
/// #1819: hands `request` to the answer queue and the shared pending store, then pushes the store to every window.
fn send_answer(queue:&ActionQueue,sync:&mut crate::pending::Pending,request:&Value,projects:&Value,proxy:&tao::event_loop::EventLoopProxy<Ev>)->crate::Res<()> {
    let result=queue.submit(request);
    match &result {
        Ok(())=>sync.begin(request,projects),
        Err(reason)=>{sync.receipt(&json!({"answerReceipt":{"root":request["root"],"id":request["op"]["id"],"state":"failed","reason":reason}}));}
    }
    let _=proxy.send_event(Ev::Sync);
    result
}
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
    logged: u8,
    /// The loop turn that last checked deadlines, and the window's last tick push.
    seen: Instant,
    tick: Option<u64>,
}
/// #1755: the running tray's `<install>/tray.fault`; the service heartbeat carries its code (`tray <code>`, § 4).
static FAULT_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static TICK_QUEUED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl WebCalls {
    fn new() -> Self {
        let mut calls=Self{workspace:false,surface:"main".into(),next:0,pending:Default::default(),counts:[0;4],saturated:false,failed:false,notified:false,metadata_unknown:false,logged:0,seen:Instant::now(),tick:None};
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
            if let Some(file)=FAULT_FILE.get() {let _=fs::write(file,reason);}
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
    // #1755: the tick timer wakes the loop every 2 s, so a longer gap is the loop itself stalled (a slow refresh);
    // callbacks queue behind it, and the deadline counts only time the loop could deliver them.
    fn expire(&mut self, now: Instant) {
        let stall=now.saturating_duration_since(self.seen);self.seen=now;
        if stall>Duration::from_secs(5) {for (start,_) in self.pending.values_mut() {*start+=stall;}}
        let expired:Vec<_>=self.pending.iter().filter(|(_, (start,_))|now.duration_since(*start)>=Duration::from_secs(30)).map(|(id,_)|*id).collect();
        for id in expired {self.finish(id,"ack-timeout");}
    }
    /// Ready for a tick push: booted, and the previous tick push acknowledged; a later tick carries newer state.
    fn idle(&self) -> bool {!self.pending.contains_key(&0) && self.tick.is_none_or(|id|!self.pending.contains_key(&id))}
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
            if delta>0 && let Some((error,site))=web_site(message) {self.log(format_args!("interface error ({class}) {error} at {site}"));}
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

// #1680 A3: a failing workspace route keeps its shell; log its class and site without faulting the window.
impl WebCalls {
    fn route(&mut self, message: &Value) {
        let route=message["route"].as_str().filter(|r|!r.is_empty()&&r.len()<=20&&r.bytes().all(|b|b.is_ascii_lowercase()));
        let (Some(route),Some((error,site)))=(route,web_site(message)) else {self.log(format_args!("workspace route failed; report invalid"));return;};
        let _=coreq::telemetry::capture::operation("tray.web.route",||Err::<(),_>("route-error"),|code|*code);
        self.log(format_args!("workspace route {route} failed: {error} at {site}; Home and Close stay available"));
    }
    // A broken page can repeat one error each draw; stderr keeps the first few.
    fn log(&mut self, line: std::fmt::Arguments) {
        if self.logged<8 {self.logged+=1;eprintln!("taskq tray: {line}");}
    }
}

/// The web guard's error class and code site; both are identifiers (no message text or URL), else none.
fn web_site(message: &Value) -> Option<(&str,&str)> {
    let ident=|s:&str|s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'$'||b==b'.');
    let error=message["error"].as_str().filter(|e|!e.is_empty()&&e.len()<=40&&ident(e))?;
    let site=message["site"].as_str().filter(|s|!s.is_empty()&&s.len()<=80)?;
    let mut parts=site.split(':');
    parts.next().filter(|n|!n.is_empty()&&ident(n))?;
    let at:Vec<&str>=parts.collect();
    (at.is_empty()||at.len()==2&&at.iter().all(|n|!n.is_empty()&&n.len()<=9&&n.bytes().all(|b|b.is_ascii_digit()))).then_some((error,site))
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
    if name==TICK {calls.tick=Some(id);}
    let p=proxy.clone();
    let workspace=calls.workspace;let surface=calls.surface.clone();
    if wv.evaluate_script_with_callback(&web_script(script),move |value| {let _=p.send_event(if workspace {Ev::WorkspaceEvaluated(surface.clone(),id,web_result(&value))} else {Ev::Evaluated(id,web_result(&value))});}).is_err() {
        calls.finish(id,"submit-error");
    }
}

pub fn run(install: PathBuf, snapshot: Option<PathBuf>, routes: Vec<String>, raise: bool) -> Result<(), String> {
    let mut lock = if snapshot.is_none() { host_lock() } else { None };
    if snapshot.is_none() && (lock.is_none() || !alone(&install)) {
        if raise { let _ = fs::write(raise_request(), ""); return Ok(()); }
        return Err("taskq tray already runs on this host".into());
    }
    if snapshot.is_none() {
        // a new page clears the last tray's fault from the heartbeat
        let fault = install.join("tray.fault");
        let _ = fs::remove_file(&fault);
        let _ = FAULT_FILE.set(fault);
        // #1755: a startup election that failed is retried, and lost batches resolve to a line in tray.log.
        crate::telemetry::keep_capture(install.clone(), Duration::from_secs(30), |line| eprintln!("taskq tray: {line}"));
        let install = install.clone();
        std::thread::spawn(move || match crate::lifecycle::open(&install, |root| crate::host::install_project(&install, root)) {
            Ok(Some(receipt)) => eprintln!("taskq tray: Open started the service: {receipt}"),
            Ok(None) => {}
            Err(e) => eprintln!("taskq tray: Open left the service stopped: {e}"),
        });
    }
    if let Some(r) = routes.iter().find(|r| r.as_str() != "icon" && !ROUTES.contains(&r.as_str()) && !REPORT_SHOTS.iter().any(|(name,_)|r==name)) {
        return Err(format!("unknown route {r}; known: icon, {}, reports, reports-conversations", ROUTES.join(", ")));
    }
    if snapshot.is_none(){crate::agentq::start(&install);}
    let opacity_shots=snapshot.is_some() && routes.iter().any(|r|r=="opacity");
    if opacity_shots && routes.len()!=1 {return Err("route opacity renders its ten cases alone".into());}
    let agentq_shots=snapshot.is_some() && routes.iter().any(|r|r.starts_with("agentq-"));
    // P-W4: the View window is its own surface, so its snapshot renders alone at the window's default size
    let views_shots=snapshot.is_some() && routes.iter().any(|r|r=="views");
    if views_shots && routes.len()>1 {return Err("route views renders the View window alone; run it without other routes".into());}
    // #1699 A8: Conversations is the workspace surface, so its snapshot renders the workspace page alone at that window's size
    let conversation_shots=snapshot.is_some() && routes.iter().any(|r|r=="conversations"||r=="reports-conversations"||r=="results-conversations");
    if conversation_shots && routes.len()>1 {return Err("route conversations renders the workspace alone; run it without other routes".into());}
    if snapshot.is_some()&&routes.iter().any(|r|r=="reports")&&routes.len()>1 {return Err("route reports renders its menu cases alone".into());}
    let mut settings = Settings::load(&install);
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // M15: TaskQ is always a regular app (Dock, Cmd+Tab); a snapshot stays an accessory out of the Dock.
        // AppKit menu accelerators enable native text editing.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(if snapshot.is_none() { ActivationPolicy::Regular } else { ActivationPolicy::Accessory });
        // a snapshot never takes the owner's focus (taskq.md R12)
        event_loop.set_activate_ignoring_other_apps(snapshot.is_none());
        if snapshot.is_none() {
            use tray_icon::menu::{AboutMetadata, Submenu, accelerator::Accelerator};
            let about = AboutMetadata { name: Some("TaskQ".into()), version: Some(crate::platform::VERSION.into()), ..Default::default() };
            let restart = MenuItem::with_id("restart", "Restart TaskQ", true, None);
            let quit = MenuItem::with_id("quit", "Quit TaskQ", true, "CmdOrCtrl+Q".parse::<Accelerator>().ok());
            let app = Submenu::with_items("TaskQ", true, &[&PredefinedMenuItem::about(Some("About TaskQ"), Some(about)), &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(Some("Hide TaskQ")), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &PredefinedMenuItem::separator(),
                &restart, &quit]).map_err(|e| e.to_string())?;
            let edit = Submenu::with_items("Edit", true, &[&PredefinedMenuItem::undo(None), &PredefinedMenuItem::redo(None), &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::cut(None), &PredefinedMenuItem::copy(None), &PredefinedMenuItem::paste(None), &PredefinedMenuItem::select_all(None)]).map_err(|e| e.to_string())?;
            let menu = Menu::with_items(&[&app, &edit]).map_err(|e| e.to_string())?;
            menu.init_for_nsapp();
            // ponytail: the app menu lives as long as the process
            std::mem::forget(menu);
        }
    }
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { rect, button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            let _ = proxy.send_event(Ev::Click(rect));
        }
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let ev = match e.id.0.as_str() { "quit" => Ev::Quit, "restart" => Ev::Restart, "open" => Ev::Raise, _ => return };
        let _ = proxy.send_event(ev);
    }));
    let proxy = event_loop.create_proxy();
    // #1755: at most one queued tick; ticks behind a stalled loop fold into one refresh
    std::thread::spawn(move || loop {
        if !TICK_QUEUED.swap(true, std::sync::atomic::Ordering::Relaxed) && proxy.send_event(Ev::Tick).is_err() {break;}
        std::thread::sleep(Duration::from_secs(2));
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
    let replies=proxy.clone();
    let history_reads=ActionQueue::start(install.clone(),&["completed"],true,queued_history,move |root,id,result| {
        let mut page=result.unwrap_or_else(|reason|json!({"tasks":[],"more":false,"reason":reason}));page["root"]=json!(root);page["id"]=json!(id);
        let _=coreq::telemetry::capture::operation("tray.history.report",||replies.send_event(Ev::Answer(json!({"completed":page}))),|_|"ui-closed");
    })?;
    let add_actions=AddOwner::start(install.clone(),proxy.clone())?;
    let replies=proxy.clone();
    let report_actions=ActionQueue::start(install.clone(),&["reports"],true,queued_reports,move |root,id,result| {
        let mut receipt=result.unwrap_or_else(|reason|json!({"reason":reason}));receipt["root"]=json!(root);receipt["id"]=json!(id);
        let _=coreq::telemetry::capture::operation("tray.reports.report",||replies.send_event(Ev::Answer(json!({"reportReceipt":receipt}))),|_|"ui-closed");
    })?;
    let mut ui: Option<(Window, WebView)> = None;
    let mut workspace: Option<(Window, WebView)> = None;
    let mut workspace_web: Option<WebCalls> = None;
    let mut detached:std::collections::BTreeMap<String,(Window,WebView,WebCalls)>=Default::default();
    // #1819: each detached window keeps the signature it last received, never the menu's
    let mut detached_shown:std::collections::BTreeMap<String,String>=Default::default();
    let mut workspace_handoff=false;
    let mut workspace_open=Value::Null;
    let mut settings_window: Option<(Window, WebView)> = None;
    let mut settings_web: Option<WebCalls> = None;
    let mut settings_open=Value::Null;
    let mut settings_shown=String::new();
    let mut views_window: Option<(Window, WebView)> = None;
    let mut views_web: Option<WebCalls> = None;
    let mut views_shown=String::new();
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
    // #1819: the owner's in-flight writes, and the projects of the last tick they are matched against
    let mut sync=crate::pending::Pending::default();
    let mut seen_projects=Value::Null;
    // M15: `Quitting` or `Restarting` while the drain-stop runs
    let (work_item, mut quitting) = (MenuItem::new("No active work", false, None), None::<&str>);
    event_loop.run(move |event, target, flow| {
        let _ = &lock;
        crate::telemetry::diagnostic::poll(&install);
        if matches!(event,Event::UserEvent(Ev::Tick)) {TICK_QUEUED.store(false,std::sync::atomic::Ordering::Relaxed);}
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
            if let Some(t)=&tray {let _=t.set_tooltip(Some("TaskQ: interface outcomes unknown; check results before retry"));}
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
                if let (Some((w,wv)),Some(calls))=(&workspace,&mut workspace_web) {w.set_visible(false);web_eval(wv,"taskq.receive({workerPanelClosed:true})","tray.web.receive",&proxy,calls);}
            }
            Event::WindowEvent {window_id,..} if workspace.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {}
            Event::WindowEvent {window_id,event:WindowEvent::CloseRequested,..} if settings_window.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {
                if let Some((w,_))=&settings_window {w.set_visible(false);}
            }
            Event::WindowEvent {window_id,..} if settings_window.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {}
            Event::WindowEvent {window_id,event:WindowEvent::CloseRequested,..} if views_window.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {
                if let Some((w,_))=&views_window {w.set_visible(false);}
            }
            Event::WindowEvent {window_id,..} if views_window.as_ref().is_some_and(|(w,_)|w.id()==window_id) => {}
            Event::NewEvents(StartCause::Init) => {
                let b = WindowBuilder::new().with_window_icon(Some(window_icon(&compact(true,false,256.0/22.0)))).with_title("TaskQ").with_transparent(true).with_decorations(false).with_resizable(false).with_always_on_top(settings.pin.top)
                    .with_visible(snapshot.is_some()).with_inner_size(if views_shots {LogicalSize::new(480.0,600.0)} else if conversation_shots {LogicalSize::new(800.0,600.0)} else {LogicalSize::new(320.0, 400.0)}).with_position(tao::dpi::LogicalPosition::new(40.0, 40.0));
                // ordinary snapshots render off-screen; opacity evidence must remain visible and unoccluded (R12)
                let b = if snapshot.is_some() { b.with_position(tao::dpi::LogicalPosition::new(if agentq_shots||opacity_shots{40.0}else{-20000.0}, if agentq_shots||opacity_shots{40.0}else{-20000.0})).with_always_on_top(opacity_shots).with_focused(false) } else { b };
                #[cfg(windows)]
                let b = tao::platform::windows::WindowBuilderExtWindows::with_skip_taskbar(b, !settings.pin.pinned);
                let p = proxy.clone();
                let built = b.build(target).map_err(|e| e.to_string()).and_then(|w| {
                    // only our page: a link goes to the browser through the bridge
                    WebViewBuilder::new().with_transparent(true).with_accept_first_mouse(true).with_html(if conversation_shots {WORKSPACE_HTML} else {HTML}).with_initialization_script(if views_shots {"window.taskqSurface='views';"} else {""}).with_navigation_handler(own_page)
                        .with_ipc_handler(move |r| {
                            let _ = p.send_event(Ev::Ipc(r.into_body()));
                        })
                        .build(&w).map(|v| (w, v)).map_err(|e| e.to_string())
                });
                match built {
                    Ok(x) => {
                        if !views_shots && !conversation_shots && (settings.opacity_mode=="background" || settings.opacity<100) && let Err(error)=window_opacity(&x.0,settings.native_opacity()) {
                            settings.opacity_error=Some(format!("Menu opacity unavailable: {error}; Off shown"));
                            settings.opacity=100;
                            settings.opacity_mode="whole".into();
                            if let Err(error)=window_opacity(&x.0,100) {settings.opacity_error=Some(format!("Menu opacity unknown: {error}"));}
                        }
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
                // M15: Open, Restart TaskQ and Quit TaskQ in the tray menu (right click), naming the work they wait for
                let menu = Menu::with_items(&[&work_item, &PredefinedMenuItem::separator(), &MenuItem::with_id("open", "Open", true, None),
                    &MenuItem::with_id("restart", "Restart TaskQ", true, None), &MenuItem::with_id("quit", "Quit TaskQ", true, None)]);
                match TrayIconBuilder::new().with_icon(rgba_icon(&compact(false, dark, 2.0))).with_tooltip("taskq").with_menu_on_left_click(false)
                    .with_menu(Box::new(menu.unwrap_or_default())).build() {
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
                // a removed install (a test's temp folder) has no current version: its tray exits too (M15)
                if !alone(&install) || crate::platform::tool(&install).current().as_deref() != Some(crate::platform::VERSION) {
                    *flow = ControlFlow::Exit;
                    return;
                }
                // M15: Quit ends the tray once the service has stopped; no heartbeat stays behind. Restart then frees the
                // host lock and starts a new tray, whose Open starts a new service generation.
                if let Some(phase) = quitting && crate::host::service_status(&install)["state"] != "running" {
                    let _ = fs::remove_file(install.join("tray"));
                    if phase == "Restarting" {
                        drop(lock.take());
                        if let Err(e) = relaunch(&install) { eprintln!("taskq tray: Restart could not start TaskQ: {e}"); }
                    }
                    *flow = ControlFlow::Exit;
                    return;
                }
                if fs::remove_file(raise_request()).is_ok() { let _ = proxy.send_event(Ev::Raise); }
                let _ = fs::write(install.join("tray"), std::process::id().to_string());
                // a drag sends many moves: save the last one per tick
                if std::mem::take(&mut moved) {
                    settings.save(&install);
                }
                let st = state(&install, &settings, dark, None, &adding, &mut sync);
                seen_projects=st["projects"].clone();
                // #1819: while any window is open the service polls the board every 15 s
                let visible=|o:&Option<(Window,WebView)>|o.as_ref().is_some_and(|(w,_)|w.is_visible());
                if visible(&ui) || visible(&workspace) || visible(&settings_window) || visible(&views_window) || detached.values().any(|(w,_,_)|w.is_visible()) {
                    crate::touch(&install.join("menu-open"));
                }
                if !web.failed {refresh_icon(&tray, &st, &settings, dark, &mut icon_sig, ui.as_ref().map(|(w,_)|w));}
                notes(&install, &mut settings, &st, &mut asked, &mut bursts);
                work_item.set_text(quit_label(&st, quitting));
                let mut comparable=st.clone();comparable.as_object_mut().expect("tray snapshot object").remove("now");
                let sig=comparable.to_string();
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {
                    if sig!=workspace_shown && !calls.failed && calls.idle() {workspace_shown=sig.clone();web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}
                }
                if let (Some((_,wv)),Some(calls))=(&settings_window,&mut settings_web) {
                    if sig!=settings_shown && !calls.failed && calls.idle() {settings_shown=sig.clone();web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}
                }
                if let (Some((_,wv)),Some(calls))=(&views_window,&mut views_web) {
                    if sig!=views_shown && !calls.failed && calls.idle() {views_shown=sig.clone();web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}
                }
                detached_shown.retain(|id,_|detached.contains_key(id));
                for (id,(_,wv,calls)) in detached.iter_mut(){if take_due(detached_shown.entry(id.clone()).or_default(),&sig,!calls.failed && calls.idle()){web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}}
                if sig != shown && web.idle() {
                    shown = sig;
                    if let Some((_,wv))=&ui && !web.failed {web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,&mut web);}
                }
            }
            Event::UserEvent(ev @ (Ev::Quit | Ev::Restart)) if snapshot.is_none() && quitting.is_none() => {
                // M15: Restart drain-stops exactly as Quit does
                quitting = Some(if matches!(ev, Ev::Restart) { "Restarting" } else { "Quitting" });
                let (install, p) = (install.clone(), proxy.clone());
                std::thread::spawn(move || if let Err(e) = crate::lifecycle::quit(&install) {
                    eprintln!("taskq tray: Quit refused: {e}");
                    let _ = p.send_event(Ev::QuitFailed(e));
                });
            }
            Event::UserEvent(Ev::QuitFailed(e)) => {
                let what = if quitting == Some("Restarting") { "Restart" } else { "Quit" };
                quitting = None;
                work_item.set_text(format!("{what} refused; see tray.log"));
                if let Some(t) = &tray { let _ = t.set_tooltip(Some(format!("TaskQ: {what} refused: {e}").chars().take(160).collect::<String>())); }
            }
            Event::UserEvent(Ev::Raise) | Event::Reopen { .. } if snapshot.is_none() => {
                let Some((w, _)) = &ui else { return };
                if !settings.pin.pinned && let Some(r) = tray.as_ref().and_then(TrayIcon::rect) { settings.pin.attached_edge = place(w, r, &mut settings.pin).into(); }
                w.set_visible(true);
                w.set_focus();
                shown_at = Instant::now();
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
                if receipt["answerReceipt"].is_object() && sync.receipt(&receipt) {let _=proxy.send_event(Ev::Sync);}
                push(&mut web,&ui,&receipt);
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);}
                // #1819: a detached conversation sees the answer it may show too
                if receipt["answerReceipt"].is_object()||receipt["reportReceipt"].is_object() {for (_,wv,calls) in detached.values_mut() {if !calls.failed {web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);}}}
            },
            // #1918: a store change rebuilds the one snapshot at once: every window, the icon and notifications read it
            Event::UserEvent(Ev::Sync) => {let _=proxy.send_event(Ev::Tick);},
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
            Event::UserEvent(Ev::Dialogue(surface,receipt)) => {
                let script=format!("taskq.receive({})",json!({"dialogueReceipt":receipt}));
                if surface=="main" {if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&script,"tray.web.receive",&proxy,calls);}}
                else if let Some((_,wv,calls))=detached.get_mut(&surface) {web_eval(wv,&script,"tray.web.receive",&proxy,calls);}
            }
            Event::UserEvent(Ev::WorkspaceEvaluated(surface,id,result)) => {if surface=="main" {if let Some(calls)=&mut workspace_web {calls.finish(id,result);}}else if surface==SETTINGS_SURFACE {if let Some(calls)=&mut settings_web {calls.finish(id,result);}}else if surface==VIEWS_SURFACE {if let Some(calls)=&mut views_web {calls.finish(id,result);}}else if let Some((_,_,calls))=detached.get_mut(&surface){calls.finish(id,result);}}
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
                        let built=WindowBuilder::new().with_title("TaskQ Request").with_decorations(false).with_inner_size(LogicalSize::new(640.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window|{
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
                        if action=="return" {web_eval(wv,&format!("taskq.receive({});taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync),json!({"open":{"page":"conversations"}})),"tray.web.receive",&proxy,calls);let select=json!({"workspaceOwner":{"id":"main","select":id,"detached":detached.keys().collect::<Vec<_>>()}});web_eval(wv,&format!("taskq.receive({select})"),"tray.web.receive",&proxy,calls);}
                        else {let r=json!({"conversationReceipt":{"id":input["id"],"state":"accepted","snapshot":crate::conversations::snapshot(&install)}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}
                    }
                    return;
                }
                let detached_ids:Vec<String>=detached.keys().cloned().collect();
                let (w,wv,calls)=if surface=="main" {let (Some((w,wv)),Some(calls))=(&workspace,&mut workspace_web) else{return};(w,wv,calls)}else{let Some((w,wv,calls))=detached.get_mut(&surface) else{return};(&*w,&*wv,calls)};
                let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
                if m["a"]=="web-error" {calls.errors(&m);return;}
                if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
                if m["a"]=="route-error" {calls.route(&m);return;}
                let context=match m["context"].as_str(){Some(text)=>match coreq::telemetry::capture::TraceContext::decode(text){Ok(context)=>Some(context),Err(_)=>{calls.fault("invalid-web-context");return;}},None=>None};
                let _continuation=context.as_ref().map(|context|context.span().entered());
                if calls.failed {return;}
                match m["a"].as_str() {
                    Some("reports")=>{if let Err(reason)=report_actions.submit(&m) {web_eval(wv,&format!("taskq.receive({})",json!({"reportReceipt":{"id":m["id"],"root":m["root"],"reason":reason}})),"tray.web.receive",&proxy,calls);}}
                    Some("sync-retry") => {
                        if let Some(request)=sync.retry(m["root"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or("")) {let _=send_answer(&answer_actions,&mut sync,&request,&seen_projects,&proxy);}
                    }
                    Some("ready") | Some("refresh") => {
                        let _=proxy.send_event(Ev::Sync);
                        calls.finish(0,"ok");
                        web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync)),"tray.web.receive",&proxy,calls);
                        if surface=="main" && !workspace_open.is_null() && !workspace_open["page"].is_null(){web_eval(wv,&format!("taskq.receive({})",json!({"open":workspace_open.take()})),"tray.web.receive",&proxy,calls);}
                        let own=if surface=="main" {json!({"id":"main","detached":detached_ids})}else{json!({"id":surface,"conversation":surface,"detached":[]})};
                        web_eval(wv,&format!("taskq.receive({})",json!({"workspacePin":w.is_always_on_top(),"workspaceMaximized":w.is_maximized()})),"tray.web.receive",&proxy,calls);
                        let script=if surface=="main" {format!("taskq.receive({})",json!({"workspaceOwner":own}))}else{format!("taskq.receive({});taskq.receive({})",json!({"open":{"page":"conversations"}}),json!({"workspaceOwner":own}))};
                        web_eval(wv,&script,"tray.web.receive",&proxy,calls);
                    }
                    Some("hide") => w.set_visible(false),
                    Some("workspace-window")=>{
                        match m["action"].as_str().unwrap_or("") {
                            "pin"=>{let pin=!w.is_always_on_top();w.set_always_on_top(pin);if surface=="main" {if let Ok(mut p)=requests_file_checked(&install){p["workspace_pin"]=json!(pin);let _=crate::platform::write_atomic(&install.join("tray-requests.json"),&p.to_string());}}web_eval(wv,&format!("taskq.receive({})",json!({"workspacePin":pin})),"tray.web.receive",&proxy,calls);}
                            "drag"=>{let _=w.drag_window();}
                            "enlarge"=>{let enlarged=!w.is_maximized();w.set_maximized(enlarged);web_eval(wv,&format!("taskq.receive({})",json!({"workspaceMaximized":enlarged})),"tray.web.receive",&proxy,calls);},
                            "size"=>{if let (Some(width),Some(height))=(m["width"].as_f64(),m["height"].as_f64()){if width.is_finite()&&height.is_finite(){let area=w.current_monitor().map(|m|work_area(&m));let scale=w.scale_factor();let max_w=area.map_or(2000.0,|(_,_,w,_)|f64::from(w)/scale).max(320.0);let max_h=area.map_or(1600.0,|(_,_,_,h)|f64::from(h)/scale).max(240.0);w.set_inner_size(LogicalSize::new(width.clamp(320.0,max_w),height.clamp(240.0,max_h)));}}}
                            _=>{}
                        }
                    }
                    Some("conversation-navigation") if m["sidebar"].is_object()=>{let receipt=crate::conversations::sidebar(&install,&m);web_eval(wv,&format!("taskq.receive({})",json!({"conversationSidebarReceipt":receipt})),"tray.web.receive",&proxy,calls);}
                    Some("conversation-navigation")=>{let error=if surface=="main" {crate::conversations::navigation(&install,&m["navigation"]).err()}else{None};web_eval(wv,&format!("taskq.receive({})",json!({"conversationNavigationError":error})),"tray.web.receive",&proxy,calls);}
                    Some("conversation-action")=>{
                        let id=m["conversation"].as_str().unwrap_or("");
                        // CS5: local archive/restore may organize a detached chat; draft edits stay with its owner.
                        let allowed=matches!(m["action"].as_str(),Some("archive"|"restore"))||if surface=="main" {!detached_ids.iter().any(|s|s==id)}else{surface==id};
                        let receipt=if allowed {crate::conversations::act(&install,&m,&roots(&install))}else{json!({"id":m["id"],"state":"unknown","reason":"Draft ownership moved; edit only in its owner window"})};
                        web_eval(wv,&format!("taskq.receive({})",json!({"conversationReceipt":receipt})),"tray.web.receive",&proxy,calls);
                        shown.clear();workspace_shown.clear();detached_shown.clear();
                    }
                    Some("conversation-dialogue")=>{
                        if !w.is_visible() && m["conversation"].as_str().is_some_and(|id|id.starts_with("task:")) {
                            let receipt=json!({"id":m["id"],"conversation":m["conversation"],"action":m["action"],"state":"accepted","value":{"ended":true,"items":[]}});
                            web_eval(wv,&format!("taskq.receive({})",json!({"dialogueReceipt":receipt})),"tray.web.receive",&proxy,calls);return;
                        }
                        // Reads only, off the event loop: the D3 holder probe starts the runtime CLI.
                        let (p,install,surface,m)=(proxy.clone(),install.clone(),surface.clone(),m.clone());
                        std::thread::spawn(move||{let receipt=crate::conversations::dialogue(&install,&m);let _=coreq::telemetry::capture::operation("tray.dialogue.report",||p.send_event(Ev::Dialogue(surface,receipt)),|_|"ui-closed");});
                    }
                    Some("message-action") => {
                        let ps:Vec<Value>=roots(&install).iter().map(|r|project(&install,r)).collect();
                        let keys:Vec<String>=m["keys"].as_array().into_iter().flatten().filter_map(|v|v.as_str().map(String::from)).collect();
                        let mut receipt=crate::messages::act(&install,&ps,m["action"].as_str().unwrap_or(""),&keys);receipt["id"]=m["id"].clone();
                        web_eval(wv,&format!("taskq.receive({})",json!({"messageReceipt":receipt})),"tray.web.receive",&proxy,calls);
                        web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync)),"tray.web.receive",&proxy,calls);
                        push(&mut web,&ui,&state(&install,&settings,dark,None,&adding,&mut sync));
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
                    Some("reply-file") => {
                        let result=crate::reply_files::store_file(&install,&m);
                        let receipt=match result{Ok(file)=>json!({"id":m["id"],"state":"accepted","file":file}),Err(e)=>{tracing::info!(coreq_refusal=crate::reply_files::refusal_kind(&e));json!({"id":m["id"],"state":"rejected","kind":"storage","reason":e})}};
                        web_eval(wv,&format!("taskq.receive({})",json!({"replyFileReceipt":receipt})),"tray.web.receive",&proxy,calls);
                    }
                    Some("answer") => {
                        let key=format!("{}#{}",m["board"].as_str().unwrap_or(""),m["op"]["task"]);
                        let result=if m["local"].is_object(){answer_local(&install,&key,&m["local"])}else{Ok(())}.and_then(|_|send_answer(&answer_actions,&mut sync,&answer_request(&m),&seen_projects,&proxy));
                        if let Err(error)=result {let r=json!({"answerReceipt":{"root":m["root"],"id":m["op"]["id"],"state":"failed","reason":error,"next":"Nothing reached the board; send again"}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}
                    }
                    Some("open") => {if let Some(url)=m["url"].as_str().filter(|u|openable(u)){open(url);}}
                    Some(a) if matches!(a,"ai-send"|"ai-draft"|"ai-clear") => {
                        let result=ai(&install,a,&m);
                        let receipt=if a=="ai-send" {json!({"aiReceipt":{"id":m["id"],"state":if result.is_ok(){"accepted"}else{"unknown"},"reason":result.err().unwrap_or_else(||"Request saved; native launch is separate".into())}})}else{json!({"workspaceLocalError":result.err()})};
                        web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);
                        if a=="ai-send" {web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync)),"tray.web.receive",&proxy,calls);}
                    }
                    // Workspace reviews use the same queues, native picker and receipt adapters as the tray.
                    Some(a) if workspace_operation(a) => {let _=proxy.send_event(Ev::Ipc(message));}
                    _ => calls.fault("unsupported-workspace-action"),
                }
            }
            // R22: Settings is one undecorated window over the tray page; window chrome stays here and every other
            // action takes the menu's path, so the Window page still configures the menu.
            Event::UserEvent(Ev::Settings(message)) => {
                let (Some((sw,swv)),Some(calls))=(&settings_window,&mut settings_web) else {return};
                let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
                if m["a"]=="web-error" {calls.errors(&m);return;}
                if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
                if calls.failed {return;}
                let chrome=|w:&Window|json!({"settingsWindow":{"top":w.is_always_on_top(),"maximized":w.is_maximized()}});
                match m["a"].as_str().unwrap_or("") {
                    "ready" => {
                        calls.finish(0,"ok");
                        web_eval(swv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync)),"tray.web.receive",&proxy,calls);
                        web_eval(swv,&format!("taskq.receive({})",chrome(sw)),"tray.web.receive",&proxy,calls);
                        if !settings_open.is_null() {web_eval(swv,&format!("taskq.receive({})",json!({"settingsOpen":settings_open.take()})),"tray.web.receive",&proxy,calls);}
                    }
                    "hide" => sw.set_visible(false),
                    "drag" => {let _=sw.drag_window();}
                    "settings-top" => {
                        let top=!sw.is_always_on_top();sw.set_always_on_top(top);
                        if let Ok(mut r)=requests_file_checked(&install){r["settings_top"]=json!(top);let _=crate::platform::write_atomic(&install.join("tray-requests.json"),&r.to_string());}
                        web_eval(swv,&format!("taskq.receive({})",chrome(sw)),"tray.web.receive",&proxy,calls);
                    }
                    "settings-enlarge" => {sw.set_maximized(!sw.is_maximized());web_eval(swv,&format!("taskq.receive({})",chrome(sw)),"tray.web.receive",&proxy,calls);}
                    "resize" => {if let (Some(width),Some(height))=(m["w"].as_f64(),m["h"].as_f64()){if width.is_finite()&&height.is_finite(){let area=sw.current_monitor().map(|m|work_area(&m));let scale=sw.scale_factor();let max_w=area.map_or(2000.0,|(_,_,w,_)|f64::from(w)/scale).max(320.0);let max_h=area.map_or(1600.0,|(_,_,_,h)|f64::from(h)/scale).max(240.0);sw.set_inner_size(LogicalSize::new(width.clamp(320.0,max_w),height.clamp(240.0,max_h)));}}}
                    // the menu's own geometry and navigation are not this window's
                    "size" | "resize-end" | "overflow" | "pin" => {}
                    "set" if m.get("navigation").is_some() => {}
                    _ => {let _=proxy.send_event(Ev::Ipc(message));}
                }
            }
            // P-W4: the View window edits the menu's dashboard model; the menu stays its only writer to tray.json.
            Event::UserEvent(Ev::Views(message)) => {
                let (Some((vw,vwv)),Some(calls))=(&views_window,&mut views_web) else {return};
                let Ok(m)=serde_json::from_str::<Value>(&message) else {calls.fault("invalid-ipc");return};
                if m["a"]=="web-error" {calls.errors(&m);return;}
                if m["a"]=="web-observation" {calls.metadata(&m["row"]);return;}
                if calls.failed {return;}
                match m["a"].as_str().unwrap_or("") {
                    "ready" => {
                        calls.finish(0,"ok");
                        web_eval(vwv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync)),"tray.web.receive",&proxy,calls);
                        if settings.navigation["dashboard"].is_object() {web_eval(vwv,&format!("taskq.receive({})",json!({"dashboard":settings.navigation["dashboard"],"initial":true})),"tray.web.receive",&proxy,calls);}
                    }
                    "hide" => vw.set_visible(false),
                    "drag" => {let _=vw.drag_window();}
                    "views-dashboard" if m["dashboard"].is_object() && m["dashboard"].to_string().len()<=65536 => push(&mut web,&ui,&json!({"dashboard":m["dashboard"]})),
                    // ponytail: the View window owns no other action; add arms when it gains one
                    _ => {}
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
                        if value["projects"].is_array() || ["capmsg","focus_receipt","action_error","resources_receipt","aiError","agentqError","answerReceipt"].iter().any(|key|value.get(*key).is_some()) {
                            if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({value})"),"tray.web.receive",&proxy,calls);}
                            if let (Some((_,wv)),Some(calls))=(&settings_window,&mut settings_web) && !calls.failed {web_eval(wv,&format!("taskq.receive({value})"),"tray.web.receive",&proxy,calls);}
                        }
                    }};
                }
                match m["a"].as_str().unwrap_or("") {
                    "workspace" if snapshot.is_none() => {
                        workspace_open=m["open"].clone();
                        // #1699: the header entry (it names a thread) opens the main workspace, never a detached conversation
                        if workspace_open["page"]=="conversations" && workspace_open["thread"].is_null() {if let Some(id)=requests_file(&install)["conversation_navigation"]["selected"].as_str(){if let Some((window,_,_))=detached.get(id){window.set_visible(true);window.set_focus();return;}}}
                        if workspace.is_none() {
                            let p=proxy.clone();
                            let built=WindowBuilder::new().with_title("TaskQ Workspace").with_decorations(false).with_always_on_top(requests_file(&install)["workspace_pin"]==true).with_inner_size(LogicalSize::new(800.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                                WebViewBuilder::new().with_html(WORKSPACE_HTML).with_navigation_handler(own_page).with_ipc_handler(move |r|{let _=p.send_event(Ev::Workspace("main".into(),r.into_body()));}).build(&window).map(|wv|(window,wv)).map_err(|e|e.to_string())
                            });
                            match built {
                                Ok(window)=>{workspace=Some(window);let mut calls=WebCalls::new();calls.workspace=true;workspace_web=Some(calls);}
                                Err(error)=>{reply!(&json!({"workspaceError":format!("Workspace unavailable: {error}")}));return;}
                            }
                        }
                        if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {
                            if !calls.pending.contains_key(&0) && !workspace_open.is_null() && !workspace_open["page"].is_null(){web_eval(wv,&format!("taskq.receive({})",json!({"open":workspace_open.take()})),"tray.web.receive",&proxy,calls);}
                        }
                        workspace_handoff=w.is_visible() && !settings.pin.pinned;
                        if let Some((window,_))=&workspace {window.set_visible(true);window.set_focus();}
                        reply!(&json!({"workspaceError":Value::Null}));
                    }
                    "settings-open" if snapshot.is_none() => {
                        settings_open=if m["page"].is_string() {json!({"page":m["page"]})} else {Value::Null};
                        if settings_window.is_none() {
                            let p=proxy.clone();
                            let built=WindowBuilder::new().with_title("TaskQ Settings").with_decorations(false).with_always_on_top(requests_file(&install)["settings_top"]==true).with_inner_size(LogicalSize::new(420.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                                WebViewBuilder::new().with_html(HTML).with_initialization_script("window.taskqSurface='settings';").with_navigation_handler(own_page).with_ipc_handler(move |r|{let _=p.send_event(Ev::Settings(r.into_body()));}).build(&window).map(|wv|(window,wv)).map_err(|e|e.to_string())
                            });
                            match built {
                                Ok(window)=>{settings_window=Some(window);let mut calls=WebCalls::new();calls.workspace=true;calls.surface=SETTINGS_SURFACE.into();settings_web=Some(calls);settings_shown.clear();}
                                Err(error)=>{reply!(&json!({"workspaceError":format!("Settings unavailable: {error}")}));return;}
                            }
                        }
                        if let (Some((_,wv)),Some(calls))=(&settings_window,&mut settings_web) && !calls.pending.contains_key(&0) && !settings_open.is_null() {web_eval(wv,&format!("taskq.receive({})",json!({"settingsOpen":settings_open.take()})),"tray.web.receive",&proxy,calls);}
                        workspace_handoff=w.is_visible() && !settings.pin.pinned;
                        if let Some((window,_))=&settings_window {window.set_visible(true);window.set_focus();}
                        reply!(&json!({"workspaceError":Value::Null}));
                    }
                    "views-open" if snapshot.is_none() => {
                        if views_window.is_none() {
                            let p=proxy.clone();
                            let built=WindowBuilder::new().with_title("TaskQ View").with_decorations(false).with_inner_size(LogicalSize::new(480.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                                WebViewBuilder::new().with_html(HTML).with_initialization_script("window.taskqSurface='views';").with_navigation_handler(own_page).with_ipc_handler(move |r|{let _=p.send_event(Ev::Views(r.into_body()));}).build(&window).map(|wv|(window,wv)).map_err(|e|e.to_string())
                            });
                            match built {
                                Ok(window)=>{views_window=Some(window);let mut calls=WebCalls::new();calls.workspace=true;calls.surface=VIEWS_SURFACE.into();views_web=Some(calls);views_shown.clear();}
                                Err(error)=>{reply!(&json!({"workspaceError":format!("View window unavailable: {error}")}));return;}
                            }
                        }
                        workspace_handoff=w.is_visible() && !settings.pin.pinned;
                        if let Some((window,_))=&views_window {window.set_visible(true);window.set_focus();}
                        reply!(&json!({"workspaceError":Value::Null}));
                    }
                    "answer" if snapshot.is_none() && crate::snapshot::CONTROLS.iter().any(|c| m["op"]["action"]==c.0) => {
                        if let Err(error)=send_answer(&answer_actions,&mut sync,&answer_request(&m),&seen_projects,&proxy){reply!(&json!({"answerReceipt":{"root":m["root"],"id":m["op"]["id"],"state":"failed","reason":error,"next":"Nothing reached the board; send again"}}));}
                    }
                    "sync-retry" if snapshot.is_none() => {
                        if let Some(request)=sync.retry(m["root"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or("")) {let _=send_answer(&answer_actions,&mut sync,&request,&seen_projects,&proxy);}
                    }
                    "overflow" if snapshot.is_some() => overflow.push(format!("{} by {} px", m["shot"].as_str().unwrap_or("?"), m["x"])),
                    "ready" => {
                        let _=proxy.send_event(Ev::Sync);
                        web.finish(0,"ok");
                        if snapshot.is_some() {
                            let _ = proxy.send_event(Ev::Shot);
                            return;
                        }
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                        // R22: Settings opens its own window; a snapshot keeps it in the menu
                        push(&mut web,&ui,&json!({"settingsWindow":true,"viewsWindow":true}));
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
                        reply!(&state(&install,&settings,dark,None,&adding,&mut sync));
                    }
                    "resize-end" => { settings.save(&install); }
                    "hide" => w.set_visible(false),
                    "top" => {
                        settings.pin.top = !settings.pin.top;
                        w.set_always_on_top(settings.pin.top);
                        settings.save(&install);
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                    }
                    "pin" | "drag" => {
                        settings.pin.pinned = m["a"]=="drag" || !settings.pin.pinned;
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
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                        if m["a"]=="drag" { let _=w.drag_window(); }
                    }
                    "completed" => {
                        let page=if snapshot.is_some() {Some(json!({"tasks":[],"more":false,"reason":"snapshot: no board read"}))} else {history_reads.submit(&m).err().map(|reason|json!({"tasks":[],"more":false,"reason":reason}))};
                        if let Some(mut page)=page {page["root"]=m["root"].clone();page["id"]=m["id"].clone();reply!(&json!({"completed":page}));}
                    }
                    "reports" => {
                        if let Err(reason)=report_actions.submit(&m) {reply!(&json!({"reportReceipt":{"id":m["id"],"root":m["root"],"reason":reason}}));}
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
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                    }
                    "notify-test" if snapshot.is_none() => {
                        if let Some(k) = m["kind"].as_str().filter(|k| notify::KINDS.contains(k)) {
                            notify::post(&notify::sample(&settings.notify, k, now()));
                        }
                    }
                    "notify-os" => notify::open_settings(),
                    "set" => {
                        if let Err(error)=appearance_input(&m) {
                            settings.opacity_error=Some(error);
                            reply!(&state(&install,&settings,dark,None,&adding,&mut sync));return;
                        }
                        let appearance=["opacity","opacity_mode","background_opacity"].iter().any(|key|m.get(*key).is_some());
                        let previous=settings.clone();
                        settings.set(&m);
                        if appearance {
                            settings.opacity_error=match window_opacity(w,settings.native_opacity()) {
                                Ok(())=>None,
                                Err(error)=>{
                                    settings=previous;
                                    let rollback=window_opacity(w,settings.native_opacity()).err().map(|e|format!("; restore unknown: {e}")).unwrap_or_default();
                                    Some(format!("Menu opacity was not applied: {error}{rollback}"))
                                }
                            };
                        }
                        let saved = (m.get("navigation").is_none() || settings.navigation == m["navigation"]) && settings.save(&install);
                        if !saved && appearance {settings.opacity_error=Some("Opacity save failed; restart may restore the previous value".into());}
                        if m.get("navigation").is_some() {
                            if let (Some((_,wv)),Some(calls))=(&views_window,&mut views_web) && !calls.failed && !calls.pending.contains_key(&0) && m["navigation"]["dashboard"].is_object() {web_eval(wv,&format!("taskq.receive({})",json!({"dashboard":m["navigation"]["dashboard"]})),"tray.web.receive",&proxy,calls);}
                            push(&mut web, &ui, &json!({"viewStateError": if saved { Value::Null } else { json!("tray.json: save failed; view state is not persisted") }}));
                            return;
                        }
                let st = state(&install, &settings, dark, None, &adding, &mut sync);
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
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                        if let Err(error)=add_actions.submit(generation,None) {adding=json!({"state":"add failed","reason":error});reply!(&state(&install,&settings,dark,None,&adding,&mut sync));}
                    }
                    "agentq-setup" | "agentq-dismiss" if snapshot.is_none() => {
                        let error=crate::agentq::action(&install,m["a"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or(""),m["runtime"].as_str().unwrap_or("")).err();
                        reply!(&json!({"agentqError":error}));
                        reply!(&state(&install,&settings,dark,None,&adding,&mut sync));
                    }
                    "agentq-check" => {reply!(&state(&install,&settings,dark,None,&adding,&mut sync));}
                    "add-cancel" if adding["state"]=="picking" => {
                        add_generation=add_generation.wrapping_add(1);
                        adding = json!({"state":"picking dismissed","generation":add_generation});
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                    }
                    "add-cancel" if adding["state"]=="installing" => {
                        add_generation=add_generation.wrapping_add(1);
                        adding = json!({"state":"installing dismissed","generation":add_generation});
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                    }
                    "add-cancel" => {adding=Value::Null;reply!(&state(&install,&settings,dark,None,&adding,&mut sync));}
                    "add-install" if m["generation"].is_u64() && m["generation"]==adding["generation"] && adding["kind"] == "taskq" && adding["state"] == "picked" => {
                        adding["state"] = json!("installing");
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
                        let generation=adding["generation"].as_u64().unwrap_or(0);
                        if let Err(error)=add_actions.submit(generation,Some(adding.clone())) {adding["state"]=json!("install failed");adding["reason"]=json!(error);reply!(&state(&install,&settings,dark,None,&adding,&mut sync));}
                    }
                    "add-setup" if m["generation"].is_u64() && m["generation"]==adding["generation"] && matches!(adding["kind"].as_str(), Some("new" | "plain")) => {
                        if let Err(reason) = crate::setup::submit(&install, crate::runtime::OWNER, None, &[adding["path"].as_str().unwrap_or_default().to_string()]) {reply!(&json!({"aiError":reason}));}
                        adding = Value::Null;
                        reply!(&state(&install, &settings, dark, None, &adding, &mut sync));
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
                if matches!(adding["state"].as_str(),Some("picking dismissed"|"installing dismissed"))&&add_generation==generation.wrapping_add(1) {adding=Value::Null;push(&mut web,&ui,&state(&install,&settings,dark,None,&adding,&mut sync));return;}
                if !apply_added(&mut adding,add_generation,generation,v) {return;}
                push(&mut web,&ui, &state(&install, &settings, dark, None, &adding, &mut sync));
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({})",state(&install,&settings,dark,None,&adding,&mut sync)),"tray.web.receive",&proxy,calls);}
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
                if opacity_shots {
                    let (_,mode,value)=OPACITY_SHOTS.iter().find(|(js,_,_)|*js==script).expect("opacity snapshot");
                    s.opacity_mode=(*mode).into();
                    if *mode=="whole" {s.opacity=*value;} else {s.background_opacity=*value;}
                    if let Some((w,_))=&ui {
                        if let Err(error)=window_opacity(w,s.native_opacity()) {capture_errors.push(error);}
                    }
                }
                let st=state(&install, &s, theme == "dark", Some((theme, &page)), &Value::Null, &mut crate::pending::Pending::default());
                // the Conversations and question fixtures merge invented tasks into the real pushed state (palette, font, theme)
                let merged=conversation_shots||ANSWERS.iter().any(|(_, s)| *s==script)||REPORT_SHOTS.iter().any(|(_,s)|*s==script);
                if merged {if let Some((_, wv))=&ui {web_eval(wv,&format!("{script}({st})"),"tray.web.fixture",&proxy,&mut web);}}
                else {push(&mut web,&ui,&st);}
                if let (Some((_, wv)), false) = (&ui, script.is_empty() || merged) {
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
                // the workspace page has no menu to measure
                if !conversation_shots {web_eval(wv,&format!("taskq.measure({})",json!(&name)),"tray.web.measure",&proxy,&mut web);}
                #[cfg(windows)]
                if opacity_shots {capture_errors.push(format!("{name}: native composition capture is unavailable on this snapshot adapter"));}
                #[cfg(target_os="macos")]
                if (agentq_shots||opacity_shots) && let Err(error)=capture_screen(window,&dir.join(format!("screen-{name}"))){capture_errors.push(format!("{name}: {error}"));}
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
/// The `answers` snapshots (#1781): Home with one open question, then that row's details; only when named.
const ANSWERS: [(&str, &str); 2] = [
    ("menu", concat!(include_str!("../tests/fixtures/question-menu.js"), "taskqQuestionMenu.bind(null,'menu')")),
    ("details", concat!(include_str!("../tests/fixtures/question-menu.js"), "taskqQuestionMenu.bind(null,'details')")),
];
/// The `views` snapshots (P-W4): the View window with a custom saved view whose working conditions add Review, so
/// Save, Reset and Delete are live and the Review choice is checked; at its top, then scrolled to the States; only when named.
const VIEWS: [(&str, &str); 2] = [
    ("top", "taskq.receive({dashboard:{selected:'review-work',views:[{id:'review-work',name:'Review work',saved:{states:['doing']},working:{states:['doing','review']}}]},initial:true})"),
    ("states", "taskq.receive({dashboard:{selected:'review-work',views:[{id:'review-work',name:'Review work',saved:{states:['doing']},working:{states:['doing','review']}}]},initial:true});setTimeout(()=>document.querySelector('[data-facet=states][data-value=review]').scrollIntoView({block:'center'}),100)"),
];
/// The `conversations` snapshots (#1699 A8): the workspace page after the menu badge opens the unread task thread,
/// with a typed answer draft in that thread, then the owner conversation with its draft, then a PM chat with its
/// transcript and a typed reply (#1773); only when named.
const CONVERSATIONS: [(&str, &str); 4] = [
    ("badge", concat!(include_str!("../tests/fixtures/conversation-shots.js"), "taskqConversationShot.bind(null,'badge')")),
    ("question", concat!(include_str!("../tests/fixtures/conversation-shots.js"), "taskqConversationShot.bind(null,'question')")),
    ("owner", concat!(include_str!("../tests/fixtures/conversation-shots.js"), "taskqConversationShot.bind(null,'owner')")),
    ("chat", concat!(include_str!("../tests/fixtures/conversation-shots.js"), "taskqConversationShot.bind(null,'chat')")),
];
// M1.1: explicit native-alpha cases; a webview snapshot alone cannot capture window alpha.
const OPACITY_SHOTS:[(&str,&str,u64);5]=[("/* opacity: off */","whole",100),("/* opacity: contrast */","whole",90),("/* opacity: minimum */","whole",60),("/* background: clear */","background",0),("/* background: half */","background",50)];
const ROUTES: [&str; 25] = ["home", "actions", "scroll", "answers", "project", "projects", "attention", "settings", "appearance", "cadence", "notifications", "agents", "services", "about", "hosts", "delivery", "stop", "recovery", "agentq-shown", "agentq-setting-up", "agentq-running", "agentq-unknown", "views", "conversations", "opacity"];
const REPORT_SHOTS:[(&str,&str);4]=[
    ("reports",concat!(include_str!("../tests/fixtures/reports-shots.js"),"taskqReportShot.bind(null,'menu')")),
    ("reports-conversations",concat!(include_str!("../tests/fixtures/reports-shots.js"),"taskqReportShot.bind(null,'conversations')")),
    ("results",concat!(include_str!("../tests/fixtures/reports-shots.js"),"taskqResultShot.bind(null,'menu')")),
    ("results-conversations",concat!(include_str!("../tests/fixtures/reports-shots.js"),"taskqResultShot.bind(null,'conversations')")),
];

/// Snapshot steps: theme, page (`home` or a project root), text size, a script run on the page; only `routes` unless empty.
fn shots(install: &Path, routes: &[String]) -> Vec<(&'static str, String, u64, &'static str)> {
    if routes.iter().any(|r|REPORT_SHOTS.iter().any(|(name,_)|r==name)) {
        return ["dark","light"].into_iter().flat_map(|theme|REPORT_SHOTS.iter().filter(|(name,_)|routes.iter().any(|r|r==name)).map(move |(_,js)|(theme,"home".into(),16,*js))).collect();
    }
    if routes.iter().any(|r|r=="opacity") {
        return ["dark","light"].into_iter().flat_map(|theme|OPACITY_SHOTS.iter().map(move |(js,_,_)|(theme,"appearance".to_string(),16,*js))).collect();
    }
    let first = roots(install).into_iter().next();
    ["dark", "light"].iter().flat_map(|t| {
        let mut v = vec![(*t, "home".to_string(), 16, ""), (*t, "home".to_string(), 20, ""), (*t, "home".to_string(), 16, ACTIONS)];
        for font in [16, 20] { v.extend(SCROLLS.iter().map(|(_, js)| (*t, "home".to_string(), font, *js))); }
        for font in [16, 20] { v.extend(ANSWERS.iter().map(|(_, js)| (*t, "home".to_string(), font, *js))); }
        v.extend(first.clone().map(|r| (*t, r, 16, "")));
        if routes.iter().any(|r|r=="views") { for font in [16, 20] { v.extend(VIEWS.iter().map(|(_, js)| (*t, "home".to_string(), font, *js))); } }
        // #1781: light adds the empty-answer question (badge) and chat screens at 18 px, so both themes stay inside the ten-screen review set
        if *t=="dark" && routes.iter().any(|r|r=="conversations") { for font in [18, 20] { v.extend(CONVERSATIONS.iter().map(|(_, js)| (*t, "home".to_string(), font, *js))); } }
        if *t=="light" && routes.iter().any(|r|r=="conversations") { v.extend(CONVERSATIONS.iter().filter(|(n, _)| matches!(*n, "badge" | "chat")).map(|(_, js)| (*t, "home".to_string(), 18, *js))); }
        for route in &ROUTES[5..] {
            if *route=="opacity" || *route=="views" || *route=="conversations" || route.starts_with("agentq-") && !routes.iter().any(|r|r==route){continue;}
            for font in [16, 20] { v.push((*t, route.to_string(), font, if *route=="recovery" {RECOVERY}else{""})); }
        }
        v
    }).filter(|(_, p, _, js)| if routes.is_empty() { !matches!(route_of(p, js),"scroll"|"answers") } else { routes.iter().any(|r| r == route_of(p, js)) }).collect()
}

fn route_of<'a>(page: &'a str, script: &str) -> &'a str {
    if let Some((name,_))=REPORT_SHOTS.iter().find(|(_,js)|*js==script) {return name;}
    if script == RECOVERY { "recovery" } else if CONVERSATIONS.iter().any(|(_, s)| *s == script) { "conversations" } else if VIEWS.iter().any(|(_, s)| *s == script) { "views" } else if script == ACTIONS { "actions" } else if ANSWERS.iter().any(|(_, s)| *s == script) { "answers" } else if !script.is_empty() { "scroll" } else if Path::new(page).is_absolute() { "project" } else { page }
}

fn shot_name(install: &Path, routes: &[String], step: usize) -> String {
    shots(install, routes).get(step).map_or(step.to_string(), |(t, p, f, js)| {
        if let Some((name,_))=REPORT_SHOTS.iter().find(|(_,s)|s==js) {return format!("{t}-{name}-{f}");}
        if let Some((_,mode,value))=OPACITY_SHOTS.iter().find(|(s,_,_)|s==js) {return format!("{t}-{mode}-{value}-{f}");}
        if let Some((n, _))=VIEWS.iter().find(|(_, s)| s == js) {return format!("{t}-views-{n}-{f}");}
        if let Some((n, _))=CONVERSATIONS.iter().find(|(_, s)| s == js) {return format!("{t}-conversations-{n}-{f}");}
        let suffix = if js.is_empty() || *js==RECOVERY { String::new() } else { format!("-{}", SCROLLS.iter().chain(&ANSWERS).find(|(_, s)| s == js).map_or("actions", |(n, _)| n)) };
        let scroll = match route_of(p, js) { "scroll" => "-scroll", "answers" => "-answers", _ => "" };
        format!("{t}-{}{scroll}{suffix}-{f}", if Path::new(p).is_absolute() { "project" } else { p })
    })
}

fn state(install: &Path, s: &Settings, dark: bool, snap: Option<(&str, &str)>, add: &Value, sync: &mut crate::pending::Pending) -> Value {
    let mut ps: Vec<Value> = roots(install).iter().map(|r| project(install, r)).collect();
    // #1918 one channel: the pending store settles on the board's rows, then applies to them before anything below reads them
    sync.observe(&ps, Instant::now());
    sync.apply(&mut ps);
    // #1592: the page renders this model; it keeps no counts, order, labels or age of its own
    let observed=now();
    let display = crate::display::model(&mut ps, observed);
    let mut seen = agents(&ps);
    // AS5: a probe older than one probe cycle plus a minute reads "Signed in at <time>", never current
    if let Some(at) = seen["at"].as_i64() { seen["age"] = json!(crate::display::age(observed - at)); seen["fresh"] = json!((0..=360).contains(&(observed - at))); }
    // a probe runs at most 40 s (2 clients × 2 probes × 10 s); an older mark is a pass that never finished, not Checking
    if !seen.is_null() { seen["checking"] = json!(seen["checking"].as_i64().is_some_and(|c| (0..=60).contains(&(observed - c)))); }
    let messages=crate::messages::snapshot(install,&ps);
    let onboarding=snap.and_then(|(_,route)|agentq_fixture(route)).unwrap_or_else(crate::agentq::snapshot);
    let (start, end) = s.range(observed);
    let sum = summary_at(&ps, (start, end), observed);
    let url = |rgba: &Rgba| format!("data:image/png;base64,{}", base64(&rgba.png()));
    let mut st = json!({"platform": if cfg!(windows) { "windows" } else { "macos" }, "version": crate::platform::VERSION,
        "agentq":onboarding,"messages":messages,"navigation": s.navigation, "workspace_navigation":s.workspace_navigation, "host": ps.iter().find_map(|p| p["host"].as_str()).unwrap_or("?"), "view": s.view, "font": s.font, "period": s.period, "pinned": s.pin.pinned, "top": s.pin.top, "size": s.pin.size, "size_anchor": s.pin.size_anchor, "attached_edge": s.pin.attached_edge, "auto_anchor":s.pin.auto_anchor,
        "now": observed, "palette":icon::css(dark), "service":crate::host::service_status(install),"lifecycle":crate::lifecycle::status(install,None),"restart_review":crate::lifecycle::review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"stop_review":crate::lifecycle::stop_review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"full_stop_review":crate::lifecycle::full_stop_review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"stop_hold":crate::lifecycle::stop_hold_status(install),"stop_modes":cfg!(target_os="macos"),"projects": ps, "notify": notify_state(&s.notify, &ps), "agents": seen, "workspace_local": requests_file(install)["answers"], "conversations":crate::conversations::snapshot(install), "ai": {"request": crate::setup::last(install, true), "runtime":owner_runtime(&ps), "root":requests_file(install)["root"], "local":requests_file(install)["local"], "error":requests_file(install)["error"], "draft": requests_file(install)["draft"], "history": requests_file(install)["history"]}, "add": add, "setup": crate::setup::last(install, false), "history": {"start": start, "end": end, "tz": s.tz, "bins": sum.bins, "peak":icon::peak(&sum.bins), "svg":icon::svg(&icon::Data {paused:false,..icon_data(&sum)},dark), "since": sum.since},
        // § 4 Data: a corrupt tray.json names its reason while the defaults show
        "settings_error": crate::data::json(&install.join("tray.json")).err()});
    st["display"] = display;
    st["opacity"]=json!(s.opacity);
    st["opacity_mode"]=json!(s.opacity_mode);
    st["background_opacity"]=json!(s.background_opacity);
    st["opacity_error"]=json!(s.opacity_error);
    st["opacity_palette"]=json!(menu_palette(dark,s.opacity));
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

/// Host-level `Agents` (§ 4): the rows of the freshest probe in any `status.json` in the AS5 view; null when none has one.
fn agents(ps: &[Value]) -> Value {
    // AS5 Checking: a service marks a due probe before it runs; a probe read after the mark ends it
    let checking = ps.iter().filter_map(|p| p["agents"]["checking"].as_u64()).max();
    let Some(a) = ps.iter().map(|p| &p["agents"]).filter(|a| a["at"].is_u64()).max_by_key(|a| a["at"].as_u64()) else {
        return checking.map_or(Value::Null, |c| json!({"at": null, "checking": c, "rows": []}));
    };
    json!({"at": a["at"], "checking": checking.filter(|c| a["at"].as_u64().is_some_and(|at| *c >= at)), "rows": a["rows"].as_array().into_iter().flatten().map(crate::runtime::agent_view).collect::<Vec<_>>()})
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
    draw_svg(&icon::svg(&icon_data(s),dark),k,dpr)
}
fn draw_svg(svg:&str,k:f32,dpr:f32)->Result<Rgba,String> {
    let raster=if cfg!(target_os="macos") {icon::raster_slot}else{icon::raster};
    raster(svg,(22.0*k*dpr).round() as u32)
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
        let rendered = svg.as_ref().map(|svg| draw_svg(svg,k,dpr));
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
    let counts = if sum.known { format!("in work {w} · waiting for answer {a} · ready + blocked {}",r + sum.blocked) } else { "counts unknown (status error or stale)".into() };
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

/// M15 Restart: a new tray that runs Open, started after this one has freed the host lock and heartbeat. A bundled
/// tray reopens its `TaskQ.app` through LaunchServices as Open does (M14); else this executable runs `tray --open`.
fn relaunch(install: &Path) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut c = std::process::Command::new(&exe);
    c.args(["tray", "--open"]);
    #[cfg(target_os = "macos")]
    if let Some(app) = exe.ancestors().nth(3).filter(|_| exe.ends_with("TaskQ.app/Contents/MacOS/taskq")) {
        c = std::process::Command::new("open");
        c.arg("-n").arg(app);
    }
    let err = fs::OpenOptions::new().create(true).append(true).open(install.join("tray.log")).map(std::process::Stdio::from).unwrap_or_else(|_| std::process::Stdio::null());
    c.current_dir(install).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(err);
    // its own process group, as the service starts it: a later service stop leaves this tray alone
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    crate::platform::quiet(&mut c).spawn().map(drop)
}

fn workspace_operation(action:&str)->bool {
    matches!(action,"agentq-setup"|"agentq-dismiss"|"agentq-check"|"capacity"|"restart"|"stop"|"uninstall"|"recovery-read"|"recovery-apply"|"pause"|"resume"|"focus"|"update"|"add"|"add-cancel"|"add-install"|"add-setup"|"resources"|"log"|"folder")
}

/// A blur this soon after opening can be the Windows hidden-icons flyout retaking focus.
/// #1819: a window takes the snapshot when it differs from what this window last received and it can take it now.
fn take_due(shown:&mut String,sig:&str,ready:bool)->bool {
    if !ready || shown==sig {return false}
    sig.clone_into(shown);
    true
}

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

/// `taskq selftest` in `dir`: its first error line on failure. A host tool gap is no project fault: `service install`
/// prints it with its fix ([host tools](docs/design/host-service.md#host-tools)).
fn selftest(dir: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let o = crate::platform::output("tray.selftest",std::process::Command::new(me).arg("selftest").current_dir(dir).env("TASKQ_ROOT", dir).env("TASKQ_SELFTEST_TOOLS","off"),Duration::from_secs(30))?;
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

/// Web links and Codex app threads (setup sessions, #1522).
fn openable(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://") || crate::runtime::link_ok(u)
}

fn open(arg: &str) {
    let _ = coreq::telemetry::capture::operation("tray.open-request",||crate::runtime::open_link(arg, false),|_|"spawn");
}

/// The menu bar's theme: macOS follows the app appearance; the Windows taskbar has its own setting.
fn is_dark(_w: &Window) -> bool {
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("reg");
        c.args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "/v", "SystemUsesLightTheme"]);
        // #1856: an unreadable setting is logged with its command and reads dark
        let out = crate::tools::run("tray.theme", &mut c, Duration::from_secs(10)).map(|o| String::from_utf8_lossy(&o).into_owned()).map_err(|e| crate::platform::log(&format!("tray theme: {e}"))).unwrap_or_default();
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

    // M15: Quit and Restart name the doing and review work they wait for within the 32-character menu label, and app
    // text never says service; a second tray on the host never takes the lock. Risk: Quit or Restart hides active
    // work, the app shows itself as a service, or two trays run across install folders.
    #[test]
    fn quit_names_active_work_and_host_lock_is_single() {
        let st = |ns: &[(u64, &str)]| json!({"projects": [{"tasks": ns.iter().map(|(n, s)| json!({"n": n, "state": s})).collect::<Vec<_>>()}]});
        assert_eq!(quit_label(&st(&[]), None), "No active work");
        assert_eq!(quit_label(&st(&[(12, "doing"), (13, "ready"), (14, "review")]), None), "Quit waits for #12 #14");
        let many = quit_label(&st(&[(1001, "doing"), (1002, "doing"), (1003, "review"), (1004, "doing")]), Some("Quitting"));
        assert_eq!(many, "Quitting after #1001 #1002 +2");
        assert!(many.len() <= 32);
        let restart = quit_label(&st(&[(1001, "doing"), (1002, "doing"), (1003, "review"), (1004, "doing")]), Some("Restarting"));
        assert_eq!(restart, "Restarting after #1001 +3");
        for phase in ["Quitting", "Restarting"] { assert_eq!(quit_label(&st(&[]), Some(phase)), format!("{phase} TaskQ")); }
        if let Some(held) = host_lock() {
            assert!(host_lock().is_none(), "a second tray on this host is refused");
            drop(held);
        }
    }

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

    // #1679 A2: the exact Completed read parses as `status --closed` with its page; before, `--closed` ate `--page`.
    #[test]
    fn history_read_is_a_parsable_status_closed_page() {
        for (m,page,since) in [(json!({}),"1",None),(json!({"page":0}),"1",None),(json!({"page":3,"since":42}),"3",Some("42"))] {
            let a=crate::Args::parse(&history_args(&m)).unwrap();
            assert_eq!((a.pos.as_slice(),a.flags.contains_key("closed"),a.one("page"),a.one("since")),(&["status".to_string()][..],true,Some(page),since),"{m}");
        }
        let a=crate::Args::parse(&history_args(&json!({"task":7,"transcript":true,"before":80}))).unwrap();
        assert_eq!((a.one("task"),a.flags.contains_key("transcript"),a.one("before")),(Some("7"),true,Some("80")),"#1829: exact native read retains task and byte cursor; no session-open command");
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
        let large=json!({"draft":"word ".repeat(1_700_000),"op":{"text":"word ".repeat(1_700_000)}});
        answer_local(&base,"large-draft",&large).unwrap();
        crate::conversations::navigation(&base,&json!({"search":"after a large reply"})).unwrap();
        answer_local(&base,"another-draft",&local).unwrap();
        assert_eq!(requests_file_checked(&base).unwrap()["answers"]["large-draft"],large,"§ 5: a large reply is saved without a length cap");
        let bytes=fs::read(base.join("tray-requests.json")).unwrap();
        assert!(answer_local(&base,"x",&json!({"draft":7})).is_err(),"invalid draft types are still refused");
        assert!(answer_local(&base,"x",&json!({"draft":"ok","extra":"x".repeat(8*1024*1024)})).is_err(),"non-reply metadata retains its 8 MiB budget");
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
            calls.seen=now;
            for s in 1..=29 {calls.expire(now+Duration::from_secs(s));}
            assert!(calls.pending.contains_key(&expired));
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

    // #1755 A1: a slow refresh stalls the native loop and WebView2 callbacks queue behind it; the stall is not the page's
    // ack-timeout, and a tick push waits for the previous one. Risk: a 28 s refresh under load fences the whole window
    // as Interface error, or queued ticks stack evaluations to the pending limit.
    #[test]
    fn loop_stall_is_not_an_ack_timeout_and_tick_pushes_do_not_stack() {
        let mut calls=WebCalls::new();calls.finish(0,"ok");
        let now=calls.seen;
        assert!(calls.idle());
        let tick=calls.begin(TICK,now).unwrap();calls.tick=Some(tick);
        assert!(!calls.idle(),"a tick push waits for the previous one");
        calls.expire(now+Duration::from_secs(2));
        calls.expire(now+Duration::from_secs(60));
        calls.expire(now+Duration::from_secs(61));
        assert!(calls.pending.contains_key(&tick)&&!calls.failed,"the loop stall counted against the page");
        calls.finish(tick,"ok");assert!(calls.idle()&&!calls.failed);
        let lost=calls.begin(TICK,now+Duration::from_secs(61)).unwrap();
        for s in 62..=91 {calls.expire(now+Duration::from_secs(s));}
        assert!(!calls.pending.contains_key(&lost)&&calls.failed,"a responsive loop still times out a lost callback");
    }

    // #1755 A3: a tray whose startup writer election failed retries off the UI loop; once the writer is free, a page
    // error report commits as tray.web.error. Risk: one failed election leaves tray faults out of the store all lifetime.
    #[test]
    fn tray_collector_recovers_after_failed_election() {
        use std::process::{Command,Stdio};
        let d=std::env::temp_dir().join(format!("taskq-tray-capture-{}",std::process::id()));
        let _=fs::remove_dir_all(&d);fs::create_dir_all(&d).unwrap();
        struct Child(std::process::Child);
        impl Drop for Child {fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}}
        let spawn=|mode|Child(Command::new(std::env::current_exe().unwrap())
            .args(["--exact","tray::tests::tray_capture_process","--ignored","--nocapture"])
            .env("TASKQ_CAPTURE_FIXTURE",&d).env("TASKQ_CAPTURE_MODE",mode).stdin(Stdio::null()).spawn().unwrap());
        let mut writer=spawn("writer");
        let until=Instant::now()+Duration::from_secs(30);
        while !d.join("ready").exists() {
            assert!(writer.0.try_wait().unwrap().is_none(),"refusing writer failed before readiness");
            assert!(Instant::now()<until,"refusing writer did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut tray=spawn("tray");
        let status=loop {
            if let Some(status)=tray.0.try_wait().unwrap() {break status;}
            assert!(Instant::now()<until,"tray collector did not recover");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(),"tray collector did not retry and commit");
        assert!(writer.0.wait().unwrap().success());
        let reader=coreq::telemetry::Reader::open(&d.join("telemetry.log/universal.sqlite3")).unwrap();
        let rows=reader.export(0,0,1024).unwrap().to_string();
        assert!(rows.contains("taskq.operation.tray.web.error"),"page error report missing after recovery");
        drop(reader);fs::remove_dir_all(d).unwrap();
    }

    #[test]
    #[ignore="owned refusing-writer/late-tray process fixture"]
    fn tray_capture_process() {
        let d=PathBuf::from(std::env::var_os("TASKQ_CAPTURE_FIXTURE").unwrap());
        if std::env::var("TASKQ_CAPTURE_MODE").unwrap()=="writer" {
            let _runtime=crate::telemetry::install_capture(&d).unwrap();
            // Keep the writer alive but refuse every other election, as an unanswering owner does.
            let endpoint=d.join("telemetry.log/telemetry-ipc/endpoint");
            let mut value:Value=serde_json::from_slice(&fs::read(&endpoint).unwrap()).unwrap();
            value["wire"]=json!(1);fs::write(endpoint,serde_json::to_vec(&value).unwrap()).unwrap();
            fs::write(d.join("ready"),"").unwrap();
            let until=Instant::now()+Duration::from_secs(30);
            while !d.join("release").exists() {assert!(Instant::now()<until,"tray never failed its election");std::thread::sleep(Duration::from_millis(10));}
            return;
        }
        let (send,lines)=std::sync::mpsc::channel::<String>();
        crate::telemetry::keep_capture(d.clone(),Duration::from_millis(200),move|line|{let _=send.send(line.to_string());});
        let first=lines.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(first.starts_with("Telemetry: unknown (") && first.ends_with("; retrying every 1s"),"{first}");
        fs::write(d.join("release"),"").unwrap();
        assert_eq!(lines.recv_timeout(Duration::from_secs(20)).as_deref(),Ok("Telemetry: collecting"),"one line per state change");
        WebCalls::new().errors(&json!({"counts":[1,0,0,0],"saturated":false}));
        coreq::telemetry::capture::qualify().unwrap();
    }

    // #1680 A3: route and page errors log an identifier-only class and site; text, URLs or paths are dropped and a
    // failing route leaves the window usable. Risk: stderr carries page content, or a route error trips the fence.
    #[test]
    fn web_error_site_is_identifiers_only_and_route_errors_keep_the_window() {
        assert_eq!(web_site(&json!({"error":"TypeError","site":"title:1:7034"})),Some(("TypeError","title:1:7034")));
        assert_eq!(web_site(&json!({"error":"TypeError","site":"anonymous"})),Some(("TypeError","anonymous")));
        for bad in [json!({"error":"TypeError: secret","site":"x:1:2"}),json!({"error":"TypeError","site":"/private/x.js:1:2"}),
            json!({"error":"TypeError","site":"x:1"}),json!({"error":"TypeError","site":"x:a:2"}),json!({"site":"x:1:2"}),json!({"error":"E","site":""})] {
            assert_eq!(web_site(&bad),None,"{bad}");
        }
        let mut calls=WebCalls::new();
        calls.route(&json!({"a":"route-error","route":"conversations","error":"TypeError","site":"title:1:7034"}));
        calls.route(&json!({"a":"route-error","route":"../x","error":"TypeError","site":"title:1:7034"}));
        assert!(!calls.failed,"a failing route must not fault the window");
        for _ in 0..20 {calls.route(&json!({"route":"messages","error":"TypeError","site":"x"}));}
        assert_eq!(calls.logged,8,"stderr lines stay bounded");
        calls.errors(&json!({"counts":[1,0,0,0],"saturated":false,"error":"TypeError","site":"title:1:2"}));
        assert!(calls.failed,"page-wide errors keep the unknown-outcome fence");
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

    // Chain: workspace answer send -> readback resend while the first is still queued. Defect (#1662 A1): the resend
    // carried no draft side channel, so the same op read as id-mismatch and the pending answer as rejected. Risk kept:
    // a different payload under a pending op id is still refused.
    #[test]
    fn answer_resend_keeps_its_pending_correlation() {
        crate::with_test_capture(|_store,dir| {
          // § 5: 30 KiB crosses this queue's old 20 KiB cap; file/storage checks cover the larger boundaries.
          for op in [json!({"id":"ws.1","task":7,"revision":20,"choice":2}),json!({"id":"ws.1","task":7,"revision":20,"text":"word 🦀\n".repeat(3_000)})] {
            let root=dir.join("project");let install=dir.join("install");
            fs::create_dir_all(&root).unwrap();fs::create_dir_all(&install).unwrap();
            fs::write(install.join("projects"),format!("{}
",root.display())).unwrap();
            let (entered_tx,entered_rx)=std::sync::mpsc::channel();let (release_tx,release_rx)=std::sync::mpsc::channel();
            let queue=ActionQueue::start(install.clone(),&["answer"],true,move |_,_,_| {entered_tx.send(()).unwrap();release_rx.recv_timeout(Duration::from_secs(3)).unwrap();Ok(json!({}))},|_,_,_|{}).unwrap();
            let first=json!({"a":"answer","root":root,"board":"https://github.com/o/r/issues","op":op,"local":{"draft":"","receipt":{"state":"sending"}}});
            queue.submit(&answer_request(&first)).unwrap();entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            let readback=json!({"a":"answer","root":root,"board":"https://github.com/o/r/issues","op":op});
            let check=json!({"a":"answer","root":root,"board":"https://github.com/o/r/issues","op":op,"local":{"draft":"","receipt":{"state":"unknown"}}});
            // the cause: the prior bridge queued the message as sent, draft included
            let raw=|m:&Value|{let mut m=m.clone();m["id"]=json!("ws.0");m["op"]["id"]=json!("ws.0");m};
            queue.submit(&raw(&first)).unwrap();
            assert!(queue.submit(&raw(&readback)).unwrap_err().contains("already pending"),"the unnormalized resend reproduces id-mismatch");
            queue.submit(&answer_request(&readback)).unwrap();
            queue.submit(&answer_request(&check)).unwrap();
            let mut other=readback.clone();other["op"]["choice"]=json!(1);
            assert!(queue.submit(&answer_request(&other)).unwrap_err().contains("already pending"),"the correlation guard stays");
            let mut oversized=readback.clone();oversized["extra"]=json!("x".repeat(21<<10));
            assert!(queue.submit(&answer_request(&oversized)).unwrap_err().contains("payload too large"),"non-reply envelope fields retain their bound");
            release_tx.send(()).unwrap();
            entered_rx.recv_timeout(Duration::from_secs(3)).unwrap(); // the deliberately unnormalized ws.0 job
            release_tx.send(()).unwrap();
            let ActionQueue{sender,worker,..}=queue;drop(sender);worker.join().unwrap();
          }
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
        assert_eq!(names(&["answers"])[..2], ["dark-home-answers-menu-16", "dark-home-answers-details-16"]);
        assert_eq!(names(&["answers"]).len(), 8, "menu question and its details at 16 and 20 px in both themes");
        assert!(names(&["icon"]).is_empty(), "icon alone captures no popover page");
        assert_eq!(names(&["views"]), ["dark-views-top-16", "dark-views-states-16", "dark-views-top-20", "dark-views-states-20", "light-views-top-16", "light-views-states-16", "light-views-top-20", "light-views-states-20"], "P-W4: the View window in both themes and sizes");
        assert_eq!(names(&["conversations"]), ["dark-conversations-badge-18", "dark-conversations-question-18", "dark-conversations-owner-18", "dark-conversations-chat-18", "dark-conversations-badge-20", "dark-conversations-question-20", "dark-conversations-owner-20", "dark-conversations-chat-20", "light-conversations-badge-18", "light-conversations-chat-18"], "#1699 A8: at most ten screens");
        assert_eq!(names(&["opacity"]), ["dark-whole-100-16","dark-whole-90-16","dark-whole-60-16","dark-background-0-16","dark-background-50-16","light-whole-100-16","light-whole-90-16","light-whole-60-16","light-background-0-16","light-background-50-16"], "M1.1/M1.2: ten explicit native-alpha and background cases");
        let all = names(&[]);
        assert_eq!(all.len(), 2 * (4 + 2 * (ROUTES.iter().filter(|r|!r.starts_with("agentq-")&&**r!="views"&&**r!="conversations"&&**r!="opacity").count() - 5)), "no filter keeps the menu matrix without scroll or answers");
        assert!(all.iter().all(|name|!name.contains("agentq-")&&!name.contains("-views-")&&!name.contains("-conversations-")&&!name.contains("-opacity-")),"invented agentq, View and opacity evidence is opt-in only");
        for route in ROUTES {
            if route.starts_with("agentq-"){assert_eq!(names(&[route]).len(),4,"one state, both themes and sizes");}
            assert!(!names(&[route]).is_empty(), "{route} names a captured page");
        }
        fs::remove_dir_all(&install).ok();
    }

    // Chain: §7 Reports -> native snapshot routes. Risk: reports lack a native capture or change the default matrix.
    #[test]
    fn reports_snapshots_are_opt_in_and_cover_both_surfaces_and_themes() {
        let install=std::env::temp_dir();
        for route in ["reports","reports-conversations","results","results-conversations"] {
            let selected=vec![route.to_string()];let cases=shots(&install,&selected);
            assert_eq!(cases.len(),2);assert_eq!(shot_name(&install,&selected,0),format!("dark-{route}-16"));
            assert_eq!(shot_name(&install,&selected,1),format!("light-{route}-16"));
        }
        assert!(shots(&install,&[]).iter().all(|(_,p,_,js)|!route_of(p,js).starts_with("reports")));
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
        m["op"]["revision"] = json!(20);
        m["op"].as_object_mut().unwrap().remove("choice");
        m["op"]["text"] = json!("word ".repeat(53) + "ok");
        assert_eq!(answer_args(&install, &m).unwrap()[3], "word ".repeat(53) + "ok");
        m["op"]["action"] = json!("requeue");
        m["op"]["text"] = json!("word\n".repeat(400));
        assert_eq!(answer_args(&install, &m).unwrap()[3], format!("Reason: {}", "word\n".repeat(400)));
        for action in ["answer", "steer", "requeue"] {
            if action == "answer" { m["op"].as_object_mut().unwrap().remove("action"); } else { m["op"]["action"] = json!(action); }
            let text = "word 🦀\n".repeat(200_000);
            m["op"]["text"] = json!(text);
            let args = answer_args(&install, &m).unwrap();
            let path = install.join("reply-input");
            let (argv, input) = answer_input(&args, &path);
            let input = input.unwrap();
            let expected = if action == "requeue" { format!("Reason: {text}") } else { text };
            assert_eq!(input, expected, "§ 5: the entire reply uses file transport, above the old 1 MiB exchange limit");
            assert!(argv.contains(&"--text-file".into()) && argv.iter().map(String::len).sum::<usize>() < 512);
            fs::write(&path, input.as_bytes()).unwrap();
            let mut parsed = crate::Args::parse(&argv).unwrap();
            parsed.read_text(&mut std::io::empty()).unwrap();
            assert_eq!(parsed.one("text"), Some(expected.as_str()));
        }
        for cmd in ["later", "close"] {
            let original = vec![cmd.into(), "7".into(), "--text".into(), "Reason: cancelled".into()];
            assert_eq!(answer_input(&original, Path::new("unused")), (original, None), "§ 5: cancellation commands retain literal text");
        }
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

    // Chain: lost answer race -> CLI stale receipt -> tray receipt -> steer. Risk (#1785): the tray showed revision
    // ids and CLI advice instead of who answered, and offered no way to pass the losing answer on.
    #[test]
    fn answer_bridge_stale_receipt_names_the_winner_and_steers() {
        let m = json!({"root": "/p", "op": {"id": "tray.1", "task": 7, "revision": 20, "text": "use y"}});
        let cli = json!({"task": 7, "op_id": "tray.1", "outcome": "stale", "revision": 22, "reason": "revision 20 is stale; current is 22",
            "next": "Read `taskq inbox 7`; retry with --revision 22", "answered": {"by": "alex", "text": "use x"}});
        let r = answer_receipt(&m, &cli)["answerReceipt"].clone();
        assert_eq!((r["state"].as_str(), r["answered"]["by"].as_str(), r["answered"]["text"].as_str()), (Some("stale"), Some("alex"), Some("use x")));
        let mut lost = cli.clone();
        lost.as_object_mut().unwrap().remove("answered");
        // A late answer the fold rejects (task already doing) names the winner too, not the CLI refusal text.
        let rejected = json!({"task": 7, "op_id": "tray.1", "outcome": "rejected", "reason": "#7 already answered by alex: use x",
            "next": "To add to it: `taskq steer 7 --text T`.", "answered": {"by": "alex", "text": "use x"}});
        let late = answer_receipt(&m, &rejected)["answerReceipt"].clone();
        assert_eq!((late["state"].as_str(), late["answered"]["by"].as_str(), late["reason"].as_str()), (Some("rejected"), Some("alex"), Some("")));
        for r in [r, answer_receipt(&m, &lost)["answerReceipt"].clone(), late] {
            let words = r.to_string();
            assert!(!words.contains("revision") && !words.contains("taskq") && !words.contains("22"), "{words}");
        }
        let install = std::env::temp_dir().join(format!("taskq-tray-steer-{}", std::process::id()));
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("projects"), "/p").unwrap();
        let mut m = json!({"root": "/p", "board": "https://github.com/o/r/issues", "op": {"id": "tray.2", "task": 7, "revision": 22, "action": "steer", "text": "use y"}});
        assert_eq!(answer_args(&install, &m).unwrap(), ["steer", "7", "--text", "use y", "--json", "--op-id", "tray.2", "--revision", "22"]);
        let mut s = json!({"complete": true, "board": {"url": m["board"]}, "tasks": [{"n": 7, "state": "doing", "revision": 22}]});
        assert!(answer_snapshot(&s, &m).is_ok());
        s["tasks"][0]["state"] = json!("review");
        assert!(answer_snapshot(&s, &m).is_ok(), "steer remains a durable note outside doing");
        s["tasks"][0]["state"] = json!("ask");
        s["tasks"][0]["revision"] = json!(23);
        assert!(answer_snapshot(&s, &m).is_ok(), "a new question cannot drop a steer");
        s["tasks"][0]["state"] = json!("closed");
        assert!(answer_snapshot(&s, &m).is_err(), "notes never reopen closed tasks");
        m["op"]["text"] = json!("1
2
3
4
5");
        assert!(answer_args(&install, &m).is_ok(), "§ 5: steer accepts multiline human text");
        fs::remove_dir_all(install).unwrap();
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
        m["op"].as_object_mut().unwrap().remove("confirmed");
        assert!(answer_args(&install, &m).is_ok(), "one explicit cancellation action needs no confirmation flag");
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
        assert_eq!(partial["outcome"], "partial", "a parked task whose close failed reads as partly applied");
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
        assert!(answer_args(&install, &m).is_ok(), "§ 5: owner requeue is wrapped by dispatch");
        m["op"]["text"] = json!("a\nb");
        assert!(answer_args(&install, &m).is_ok(), "§ 5: owner requeue accepts multiple lines");
        // #1506 published controls: the DashQ fixture argv and state table are what the tray dispatches and offers.
        let f: Value = serde_json::from_str(include_str!("../tests/fixtures/task-controls.json")).unwrap();
        assert_eq!(f["version"], crate::snapshot::CONTROLS_VERSION);
        let table: Vec<Value> = crate::snapshot::CONTROLS.iter().map(|(a, c, t, s)| json!([a, c, t, s])).collect();
        let fixture: Vec<Value> = f["controls"].as_array().unwrap().iter().map(|c| json!([c["action"], c["command"], c["text"], c["states"]])).collect();
        assert_eq!(table, fixture);
        for (st, offered) in f["states"].as_object().unwrap() { assert_eq!(json!(crate::snapshot::controls(st)), *offered, "{st}"); }
        for c in f["cases"].as_array().unwrap() {
            m["op"] = c["op"].clone();
            let args = answer_args(&install, &m).unwrap();
            assert_eq!(json!(args), c["argv"]);
            let mut file_args = args;file_args.extend(["--json-file".into(), "receipt.json".into()]);
            assert!(crate::Args::parse(&file_args).is_ok(), "§ 5: every published control supports file receipts: {}", c["action"]);
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

    // Chain: M8 one store feeds every window. Risk: a detached conversation compares against the menu's shown
    // snapshot, so it misses a change the menu already took while the detached window was busy.
    #[test]
    fn each_window_takes_a_snapshot_it_has_not_received() {
        let (mut menu, mut detached) = (String::new(), String::new());
        assert!(take_due(&mut menu, "a", true));
        assert!(!take_due(&mut detached, "a", false), "a busy window waits");
        assert!(take_due(&mut detached, "a", true), "the menu having it does not skip the detached window");
        assert!(!take_due(&mut detached, "a", true) && !take_due(&mut menu, "a", true));
        assert!(take_due(&mut detached, "b", true));
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

    // #1789 A1/A3: macOS can widen its slot; Windows retains a square notification-area image.
    #[test]
    fn two_digit_summary_uses_the_native_canvas() {
        let sum=Summary {counts:[12,3,15,0],blocked:0,known:true,run:true,..summary(&[],(0,1))};
        let image=draw(&sum,false,1.0,2.0).unwrap();
        assert_eq!(image.h,44);
        assert_eq!(image.w>44,cfg!(target_os="macos"));
        assert_eq!(draw_svg(&icon::svg(&icon_data(&sum),false),1.0,2.0).unwrap().px,image.px,"published/native paths agree");
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
        // #1591/#1592: rows keep every observed field and gain only the computed `live`, `age` and `age_text`
        let strip=|v:&Value|{let mut v=v.clone();for r in v.as_array_mut().into_iter().flatten(){for k in ["live","age","age_text"]{r.as_object_mut().unwrap().remove(k);}}v};
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
        let reason = state(&install, &s, false, None, &Value::Null, &mut crate::pending::Pending::default())["settings_error"].clone();
        assert!(reason.as_str().unwrap().starts_with("tray.json: corrupt"), "the popover names the reason: {reason}");
        assert!(HTML.contains("Settings unknown:"));
        assert!(s.save(&install));
        assert_eq!(state(&install, &s, false, None, &Value::Null, &mut crate::pending::Pending::default())["settings_error"], Value::Null, "a save clears it");
        assert_eq!(fs::read_to_string(install.join("tray.json.corrupt")).unwrap(), "{\"view\": \"fu", "a save keeps a copy");
        assert_eq!(Settings::load(&install).font, 16);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: owner answer -> accepted op-id readback -> stale status.json refresh -> every tray surface. Risk (#1918):
    // the list, dot, badge, icon, notification or conversation reads a stale pre-answer snapshot and shows the
    // question open again after the board accepted the answer.
    #[test]
    fn an_accepted_answer_stays_closed_on_every_surface_across_a_stale_refresh() {
        let base = std::env::temp_dir().join(format!("taskq-tray-one-channel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (install, root) = (base.join("install"), base.join("project"));
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(crate::dot(&root)).unwrap();
        fs::write(install.join("projects"), format!("{}\n", root.display())).unwrap();
        let r = root.to_str().unwrap();
        let card = "Problem: Pick one\n1. Ship\n2. Wait";
        let status = |state: &str, revision: u64, ops: &[&str]| {
            let mut t = json!({"n": 7, "title": "T", "state": state, "revision": revision, "ops": ops});
            if state == "ask" { t["card"] = json!(card); t["question"] = crate::snapshot::question(card); }
            let s = json!({"complete": true, "issues": "https://github.com/o/r/issues", "observed": now(), "tasks_age_seconds": 0, "tasks": [t]});
            fs::write(crate::dot(&root).join("status.json"), s.to_string()).unwrap();
        };
        // the ask was notified once, before the answer
        fs::write(crate::dot(&root).join("events"), format!("since 1\n{} #7 ask 1\n", now() - 5)).unwrap();
        status("ask", 30, &["ask-op"]);
        let s = Settings::load(&install);
        let mut sync = crate::pending::Pending::default();
        let (mut n, mut bursts) = (json!({"host": {"ask": "banner"}}), notify::Bursts::default());
        let before = state(&install, &s, false, None, &Value::Null, &mut sync);
        notify::due(&mut n, before["projects"].as_array().unwrap(), now(), &mut bursts);
        let ask = |st: &Value| {
            let ps = st["projects"].as_array().unwrap();
            let t = &ps[0]["tasks"][0];
            let dot = ps[0]["counts"].as_array().unwrap().iter().find(|c| c["state"] == "ask").unwrap()["n"].clone();
            let badge = st["messages"]["entries"].as_array().unwrap().iter().any(|e| e["kind"] == "question" && e["pending"] == true);
            (t["state"].clone(), dot, badge, summary_at(ps, (0, 1), now()).counts[1])
        };
        assert_eq!(ask(&before), (json!("ask"), json!(1), true, 1), "the question is open before the answer");

        let request = json!({"root": r, "op": {"id": "tray.1", "task": 7, "revision": 30, "choice": 1}});
        sync.begin(&request, &before["projects"]);
        assert!(sync.receipt(&json!({"answerReceipt": {"root": r, "id": "tray.1", "state": "accepted"}, "sync": {"state": "doing", "revision": 25}})));
        // the service has not read the board again: status.json still holds the question, twice
        for _ in 0..2 {
            status("ask", 30, &["ask-op"]);
            let st = state(&install, &s, false, None, &Value::Null, &mut sync);
            assert_eq!(ask(&st), (json!("doing"), json!(0), false, 0), "list, dot, badge and icon keep the readback");
            assert!(notify::due(&mut n, st["projects"].as_array().unwrap(), now(), &mut bursts).is_empty(), "no notification asks again");
            let t = &st["projects"][0]["tasks"][0];
            assert_eq!((t["history"][0]["question"]["text"].as_str(), t["history"][0]["answered"].clone(), t["card"].clone()), (Some(card), json!({"by": "you", "text": "Ship"}), Value::Null),
                "the conversation keeps the question, then the answer");
        }
        // the board's readback carries the op id; a lagging read after it still never reopens the question
        status("doing", 25, &["tray.1", "ask-op"]);
        assert_eq!(ask(&state(&install, &s, false, None, &Value::Null, &mut sync)).0, json!("doing"));
        status("ask", 30, &["ask-op"]);
        assert_eq!(ask(&state(&install, &s, false, None, &Value::Null, &mut sync)), (json!("doing"), json!(0), false, 0));
        fs::remove_dir_all(base).unwrap();
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
        let mut s = Settings { view: "icon".into(), font: 16, opacity: 100, opacity_mode: "whole".into(), background_opacity: 100, opacity_error: None, period: "1d".into(), start: 0, end: 0, tz: "local".into(), pin: Pin::default(), notify: Value::Null, navigation: Value::Null, workspace_navigation:Value::Null };
        s.set(&json!({"view": "full", "font": 18, "period": "1w"}));
        s.set(&json!({"view": "x", "font": 15, "period": "2y"}));
        s.set(&json!({"period": "custom", "start": 50, "end": 50, "tz": "UTC"}));
        assert!(s == Settings { view: "full".into(), font: 18, opacity: 100, opacity_mode: "whole".into(), background_opacity: 100, opacity_error: None, period: "1w".into(), start: 0, end: 0, tz: "local".into(), pin: Pin::default(), notify: Value::Null, navigation: Value::Null, workspace_navigation:Value::Null });
        assert_eq!(s.range(1000), (100800 - 604800, 100800), "whole 28 h bins, the newest holds now");
        s.set(&json!({"period": "custom", "start": 40, "end": 160, "tz": "UTC"}));
        assert_eq!((s.range(1000), s.tz.as_str()), ((40, 160), "UTC"));
    }

    // Chain: M1.1 / #1812 A2. Risk: restart loses opacity, invalid IPC bypasses the opacity range, or save loses other preferences.
    #[test]
    fn opacity_validation_and_restart_preserve_other_preferences() {
        let install=std::env::temp_dir().join(format!("taskq-opacity-{}",std::process::id()));
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("tray.json"),r#"{"font":20,"future":{"keep":true}}"#).unwrap();
        let mut settings=Settings::load(&install);
        assert_eq!(settings.opacity,100,"existing installs default to Off");
        for value in [60,61,89,90,95,100] {
            assert!(appearance_input(&json!({"opacity":value})).is_ok());
            settings.set(&json!({"opacity":value}));assert!(settings.save(&install));
            settings=Settings::load(&install);assert_eq!((settings.opacity,settings.font),(value,20));
        }
        settings.set(&json!({"opacity":95}));
        for value in [json!(null),json!(59),json!(101),json!(-1),json!(90.5),json!("95")] {
            assert!(appearance_input(&json!({"opacity":value})).is_err());
            settings.set(&json!({"opacity":value}));assert_eq!(settings.opacity,95,"refuse invalid opacity");
        }
        assert!(settings.save(&install));
        assert_eq!(state(&install,&settings,false,None,&Value::Null,&mut crate::pending::Pending::default())["opacity"],95,"native readback reaches every menu route");
        let saved:Value=serde_json::from_str(&fs::read_to_string(install.join("tray.json")).unwrap()).unwrap();
        assert_eq!(saved["future"]["keep"],true);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: M1.2 / #1876 A2–A3. Risk: legacy backing blocks transparency, restart loses either mode, or invalid IPC applies.
    #[test]
    fn background_opacity_validation_and_reload() {
        let install=std::env::temp_dir().join(format!("taskq-background-{}",std::process::id()));
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("tray.json"),r#"{"opacity":60,"font":20,"backing_strength":98,"future":true}"#).unwrap();
        let mut s=Settings::load(&install);
        assert_eq!((s.opacity_mode.as_str(),s.native_opacity()),("whole",60),"legacy backing does not block new alpha");
        for background in [0,50,100] {
            let v=json!({"opacity_mode":"background","background_opacity":background});
            assert!(appearance_input(&v).is_ok());s.set(&v);assert!(s.save(&install));s=Settings::load(&install);
            assert_eq!((s.opacity,s.native_opacity(),s.background_opacity,s.font),(60,100,background,20));
        }
        for v in [json!({"opacity_mode":null}),json!({"opacity_mode":"other"}),json!({"background_opacity":-1}),json!({"background_opacity":101}),json!({"background_opacity":0.5}),json!({"opacity":59})] {
            assert!(appearance_input(&v).is_err(),"reject {v} before native application");
        }
        let st=state(&install,&s,false,None,&Value::Null,&mut crate::pending::Pending::default());
        assert_eq!((st["opacity_mode"].as_str(),st["background_opacity"].as_u64()),(Some("background"),Some(100)));
        assert!(st.get("backing_strength").is_none(),"retired backing never reaches the renderer");
        s.set(&json!({"opacity_mode":"whole"}));assert_eq!(s.native_opacity(),60,"switch restores the independent whole-window value");
        let saved:Value=serde_json::from_str(&fs::read_to_string(install.join("tray.json")).unwrap()).unwrap();
        assert_eq!(saved["backing_strength"],98,"legacy field stays inert without destroying preferences");
        assert_eq!(saved["future"],true);fs::remove_dir_all(install).unwrap();
    }

    // Chain: M1.1 / #1812 A3. Risk: native alpha washes primary, secondary or semantic body text below AA on a busy backdrop.
    #[test]
    fn opacity_text_palette_meets_aa_after_adverse_composition() {
        let rgb=|hex:&str|->Vec<f64>{(1..7).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16).unwrap() as f64).collect()};
        let luminance=|rgb:Vec<f64>|->f64{rgb.iter().zip([0.2126,0.7152,0.0722]).map(|(v,w)|{let c=v/255.0;w*if c<=0.04045 {c/12.92}else{((c+0.055)/1.055).powf(2.4)}}).sum()};
        for dark in [false,true] {
            assert_eq!(menu_palette(dark,100),icon::css(dark),"Off retains the accepted palette");
            for percent in 90..100 {
                let palette=menu_palette(dark,percent);
                let colors=std::iter::once(if dark {"#FFFFFF"}else{"#12181C"}).chain(palette.split(';').map(|p|p.split_once(':').unwrap().1));
                let surfaces=if dark {["#252B32","#303840","#37424C","#354A40","#323B45"]}else{["#FAFBFC","#EDF0F3","#E9EDF1","#DCE7E1","#FFFFFF"]};
                for color in colors {for surface in surfaces {for alpha in [percent as f64/100.0,((percent*255+50)/100) as f64/255.0] {
                    let fg=luminance(rgb(color).iter().map(|v|alpha*v+(1.0-alpha)*if dark {0.0}else{255.0}).collect());
                    let bg=luminance(rgb(surface).iter().map(|v|alpha*v+(1.0-alpha)*if dark {255.0}else{0.0}).collect());
                    let contrast=(fg.max(bg)+0.05)/(fg.min(bg)+0.05);
                    assert!(contrast>=4.5,"{dark} {percent}% {color} on {surface}: {contrast}");
                }}}
            }
        }
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

    // Chain: #1712 events file -> project() -> Hour summary. Risk: a second ask of one attempt or a close in the
    // window is not counted, or a missing source reads as zero without a reason.
    #[test]
    fn hour_window_counts_each_ask_and_close_and_names_missing_sources() {
        let root = std::env::temp_dir().join(format!("taskq-1712-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(crate::dot(&root)).unwrap();
        let install = root.join("install");
        let r = root.to_str().unwrap();
        assert_eq!((project(&install, r)["since"].clone(), project(&install, r)["events_error"].clone()), (Value::Null, json!("no events file")));
        fs::write(crate::dot(&root).join("events"), "1000 #9 ask 3\n").unwrap();
        assert_eq!(project(&install, r)["events_error"], "events file has no since line");
        // the first ask before the window; an answer, then a second ask and a close of the same task inside it
        fs::write(crate::dot(&root).join("events"), "since 0\n1000 #9 ask 11\n5000 #9 ask 22\n6000 #9 close 33 replaced by #10 (ready)\n").unwrap();
        let p = project(&install, r);
        // #1800: a close carries its § 7 outcome as a 5th item; other events keep 4
        assert_eq!((p["events"][2].clone(), p["events"][1].as_array().unwrap().len()), (json!([6000, "close", "33", 9, "replaced by #10 (ready)"]), 4));
        assert_eq!((p["since"].clone(), p["events_error"].clone()), (json!(0), Value::Null));
        let s = summary_at(&[p], (3600, 7200), 7200);
        assert_eq!((0..4).map(|k| s.bins.iter().map(|b| b.counts[k]).sum::<u32>()).collect::<Vec<_>>(), [1, 0, 1, 0], "closed, failed, ask, blocked");
        assert!(s.bins.iter().all(|b| b.missing.is_empty()));
        let _ = fs::remove_dir_all(&root);
    }

    // Chain: M6. UX30-21: source identity, complete zero, lower bounds, half-open boundaries and multi-host deduplication.
    #[test]
    fn incoming_coverage_keeps_legacy_and_exact_event_sources_distinct() {
        let legacy=json!({"name":"legacy","issues":"u","tasks":[],"since":0,"events":[[1,"close","1",7],[2,"ask","2",7],[3,"new","unproven",8]]});
        let old=summary_at(&[legacy.clone()],(1,7),7);
        assert!(old.bins.iter().all(|b|!b.incoming_complete()));
        assert_eq!(old.bins.iter().map(|b|b.incoming).sum::<u32>(),0,"unversioned lines never prove incoming");
        assert_eq!(icon::peak(&old.bins),1,"known completed/question stacks remain");
        let vector=icon::svg(&icon_data(&old),false);
        assert!(vector.contains("data-mark=\"completed\"")&&vector.contains("data-mark=\"ask\""));
        let mut p=legacy.clone();p["incoming"]=json!({"schema":1,"board":"u/0","start":1,"end":7,"missing":[],"events":[[1,"new","10",7],[2,"new","11",8],[7,"new","12",9]]});
        let fresh=summary_at(&[p.clone(),p.clone(),legacy.clone()],(1,7),7);
        assert!(fresh.bins.iter().all(icon::Bin::incoming_complete),"another host's legacy cache cannot erase verified board coverage");
        assert_eq!(fresh.bins.iter().map(|b|b.incoming).collect::<Vec<_>>(),[1,1,0,0,0,0],"exact [start,end) and one event across hosts");
        assert_eq!(icon::peak(&fresh.bins),2);
        assert!(fresh.bins[2..].iter().all(|b|b.incoming==0&&b.incoming_complete()),"complete zero differs from unknown");
        p["incoming"]["start"]=json!(3);
        let partial=summary_at(&[p.clone()],(1,7),7);
        assert!(!partial.bins[0].incoming_complete()&&partial.bins[2].incoming_complete());
        assert!(!icon::svg(&icon_data(&partial),false).split("id=\"bin-0\"").nth(1).unwrap().split("</g>").next().unwrap().contains("data-mark=\"completed\""),"an interval gap stays unknown, retaining bounds in detail");
        p["incoming"]["board"]=json!("foreign/0");
        assert_eq!(summary_at(&[p],(1,7),7).bins.iter().map(|b|b.incoming).sum::<u32>(),0);
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
        for b in &mut zero.bins {b.counts=[0;4];b.incoming=0;b.incoming_missing.clear();}
        let mut failed=zero.clone();failed.bins[2].counts[1]=1;
        let mut unknown=zero.clone();unknown.bins[0].missing=vec!["synthetic missing source".into()];unknown.bins[0].counts[0]=999;
        let mut ask=zero.clone();ask.bins[0].counts[2]=3;
        ask.bins[2].counts=[2,1,1,0];
        ask.bins[4].missing=vec!["synthetic missing source".into()];ask.bins[4].counts=[999,0,2,0];
        let mut stack=zero.clone();stack.counts=[12,3,15,0];
        for (i,b) in stack.bins.iter_mut().enumerate() {b.counts=[(i+1) as u32,1,2,7];b.incoming=(6-i) as u32;}
        let mut partial=stack.clone();partial.bins[2].incoming_missing=vec!["synthetic incoming gap".into()];
        let mut rare=zero.clone();rare.bins[0].counts=[10000,1,1,0];rare.bins[0].incoming=1;
        let mut overflow=stack.clone();overflow.counts=[100,99,12,0];
        for (name,s) in [("day",&day),("hour",&hourly),("incomplete",&aggregate),("paused",&paused),("synthetic-zero",&zero),("synthetic-failed",&failed),("synthetic-unknown",&unknown),("synthetic-ask",&ask),("synthetic-stack",&stack),("synthetic-partial-new",&partial),("synthetic-rare",&rare),("synthetic-overflow",&overflow)] {
            for dark in [false,true] {
                let vector=icon::svg(&icon_data(s),dark);
                let history_vector=icon::svg(&icon::Data {paused:false,..icon_data(s)},dark);
                for n in [18,22,33,44,66,256] {assert_eq!(icon::raster(&vector,n).unwrap().w,n);}
                if std::env::var_os("TASKQ_ICON_EVIDENCE").is_none() {
                    let theme=if dark {"dark"}else{"light"};
                    let saved:Value=serde_json::from_slice(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/icon-history/{name}-{theme}.json"))).expect("canonical browser fixture")).unwrap();
                    assert_eq!(saved["bins"],json!(s.bins),"browser/native bins and exact counts");
                    assert_eq!(saved["peak"],icon::peak(&s.bins));assert_eq!(saved["palette"],icon::css(dark));
                    let chart=|svg:&str|svg.split(r#"<g id="chart""#).nth(1).unwrap().split(r#"</g><g id="counters">"#).next().unwrap().to_string();
                    assert_eq!(chart(saved["svg"].as_str().unwrap()),chart(&history_vector),"History retains canonical heights, colors and marks");
                }
                if let Ok(dir)=std::env::var("TASKQ_ICON_EVIDENCE") {
                    let dir=Path::new(&dir);fs::create_dir_all(dir).unwrap();let theme=if dark {"dark"}else{"light"};
                    fs::write(dir.join(format!("{name}-{theme}.svg")),&vector).unwrap();
                    for n in [18,44,66,256] {fs::write(dir.join(format!("{name}-{theme}-{n}.png")),icon::raster(&vector,n).unwrap().png()).unwrap();}
                    fs::write(dir.join(format!("{name}-{theme}.json")),json!({"bins":s.bins,"peak":icon::peak(&s.bins),"svg":history_vector,"icon_svg":vector,"start":s.bins[0].start,"end":s.bins[5].end,"tz":"local","since":s.since,"palette":icon::css(dark),"captured":captured,"font":font,"compact":format!("data:image/png;base64,{}",base64(&compact(s.run,dark,2.0).png()))}).to_string()).unwrap();
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
        assert_eq!(agents(&ps), json!({"at": 20, "checking": null, "rows": [{"runtime": "claude", "version": "2", "state": "sign-in-required", "account": null, "method": null, "expires": null, "refreshed": null, "reason": null, "next": "run `claude auth login` with the PM (docs/relogin.md)"}]}));
        assert_eq!(agents(&ps[2..]), Value::Null);
        // #1661 A2: a probe in progress reads Checking over its prior rows; a probe read after the mark ends it
        let ps = [json!({"agents": {"at": 20, "checking": 30, "rows": [row("2")]}}), json!({"agents": {"at": 25, "rows": [row("3")]}})];
        assert_eq!((agents(&ps)["checking"].clone(), agents(&ps)["rows"][0]["version"].clone()), (json!(30), json!("3")));
        let ps = [json!({"agents": {"at": 20, "checking": 30, "rows": []}}), json!({"agents": {"at": 40, "rows": []}})];
        assert_eq!(agents(&ps)["checking"], Value::Null);
        assert_eq!(agents(&[json!({"agents": {"checking": 30}})]), json!({"at": null, "checking": 30, "rows": []}));
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
        assert_eq!(row["native_session_url"], row["session_url"]);
        assert!(row["progress_url"].is_null());
        let mut codex = json!({"session": "codex:valid-thread mac"});
        task_session(Path::new("."), "win", None, &mut codex);
        assert!(codex["session_url"].is_null());
        assert_eq!(codex["session_reason"], "Codex session opens only on mac", "remote codex: no link, reason");
        let mut paged = json!({"n": 7, "session": "codex:valid-thread mac"});
        task_session(Path::new("."), "win", Some("https://d/projects#board-o%2Fr-task-"), &mut paged);
        assert_eq!(paged["session_url"], "https://d/projects#board-o%2Fr-task-7", "remote codex: DashQ progress page, never codex://");
        assert_eq!(paged["progress_url"], paged["session_url"]);
        assert!(paged["native_session_url"].is_null());
        assert_eq!(paged["native_session_reason"], "Codex session opens only on mac");
        for runtime in ["codex", "codex-app"] {
            let mut app = json!({"n": 7, "session": format!("{runtime}:valid-thread win")});
            task_session(Path::new("."), "win", None, &mut app);
            assert_eq!(app["native_session_url"], "codex://threads/valid-thread");
            task_session(Path::new("."), "mac", Some("http://relay.example/progress/"), &mut app);
            assert_eq!(app["progress_url"], "http://relay.example/progress/7");
            assert!(app["native_session_url"].is_null());
        }
        let mut remote = json!({"session": "claude:valid-thread mac", "session_url": url});
        task_session(Path::new("."), "win", None, &mut remote);
        assert_eq!((remote["local_session"].clone(), remote["session_url"].clone()), (json!(false), json!(url)), "remote claude: link");
        assert_eq!(remote["native_session_url"], url);
        let mut both = json!({"n": 7, "session": "claude:valid-thread mac", "session_url": url});
        task_session(Path::new("."), "win", Some("https://relay.example/progress/"), &mut both);
        assert_eq!(both["native_session_url"], url);
        assert_eq!(both["progress_url"], "https://relay.example/progress/7");
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
