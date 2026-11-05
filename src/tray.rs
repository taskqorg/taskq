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

/// #2365: the menu's adoption surface; `#` keeps it apart from detached conversation ids.
const MENU_SURFACE: &str = "#menu";
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
    let work: Vec<String> = st["projects"].as_array().into_iter().flatten().flat_map(|p| p["tasks"].as_array().into_iter().flatten().map(move |t| (p["name"].as_str().unwrap_or("Board"), t)))
        .filter(|(_, t)| matches!(t["state"].as_str(), Some("doing" | "review"))).map(|(name, t)| format!("{name}#{}", t["n"])).collect();
    if work.is_empty() { return phase.map_or("No active work".into(), |p| format!("{p} TaskQ")); }
    let mut s = phase.map_or("Quit waits for".into(), |p| format!("{p} after"));
    for (i, w) in work.iter().enumerate() {
        if s.len() + w.len() + 1 > 26 { s += &format!(" +{}", work.len() - i); break; }
        s += " "; s += w;
    }
    s
}

/// The native windows a stamped page message belongs to; a retired page's message never gets authority.
#[derive(Clone, Copy)]
enum PageSurface { Menu, Settings, Views }

enum Ev {
    Tick,
    Page(PageSurface,u64,String),
    /// #2365: move one surface to the active bundle; the window keeps its visibility and focus.
    AdoptPage(String),
    BundleChecked(crate::ui_bundle::Checked),
    /// M15: Quit TaskQ from the tray menu or Cmd+Q; a refused Quit returns its reason.
    Quit,
    /// M15: Restart TaskQ drain-stops as Quit, then starts a new tray that runs Open.
    Restart,
    QuitFailed(String),
    /// M15: a second Open shows the running tray's window.
    Raise,
    /// Recreate only Conversations; true opens its safe list.
    RecoverWorkspace(String,bool),
    WorkspaceClose,
    Ipc(String),
    /// #1819: the shared pending store changed; every window gets it at once, not on the next tick.
    Sync,
    Workspace(String, String),
    /// R22: IPC from the one Settings window (the tray page as its `settings` surface).
    Settings(String),
    /// P-W4: IPC from the one View window (the tray page as its `views` surface).
    Views(String),
    WorkspaceEvaluated(u64, String, u64, &'static str),
    Evaluated(u64, u64, &'static str),
    Click(Rect),
    Shot,
    Shot2,
    /// Add project: the generation fences a late picker or install result.
    Added(u64, Value),
    Answer(Value),
    /// D4/D7 dialogue read receipt for one workspace surface.
    Dialogue(String, Value),
    Conversation(String, Value),
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
    conversation_geometry: Option<ConversationGeometry>,
}

/// Logical size and work-area-relative position; native settings, independent of page content.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct ConversationGeometry { display:String, position:[f64;2], size:[f64;2] }
impl ConversationGeometry {
    fn valid(&self)->bool {self.display.len()<=1024 && self.position.iter().chain(&self.size).all(|n|n.is_finite()) && self.size.iter().all(|n|*n>0.0&&*n<=32768.0)}
    fn restore(&self,area:(i32,i32,i32,i32),scale:f64,frame:(u32,u32))->((i32,i32),(u32,u32)) {
        let width=(self.size[0]*scale).round().max(1.0).min((area.2 as f64-frame.0 as f64).max(1.0)) as u32;
        let height=(self.size[1]*scale).round().max(1.0).min((area.3 as f64-frame.1 as f64).max(1.0)) as u32;
        let position=clamp((area.0.saturating_add((self.position[0]*scale).round() as i32),area.1.saturating_add((self.position[1]*scale).round() as i32)),((width+frame.0) as i32,(height+frame.1) as i32),area);
        (position,(width,height))
    }
    fn capture(window:&Window)->Option<Self> {
        if window.is_maximized() || !window.is_visible(){return None;}
        let monitor=window.current_monitor()?;let area=work_area(&monitor);let scale=window.scale_factor();let p=window.outer_position().ok()?;let size=client_size(window);
        let value=Self{display:key(&monitor),position:[f64::from(p.x-area.0)/scale,f64::from(p.y-area.1)/scale],size:[f64::from(size.width)/scale,f64::from(size.height)/scale]};
        value.valid().then_some(value)
    }
    fn monitor(&self,window:&Window)->Option<tao::monitor::MonitorHandle> {
        window.available_monitors().find(|m|key(m)==self.display).or_else(||window.primary_monitor()).or_else(||window.current_monitor())
    }
    fn apply(&self,window:&Window) {
        let Some(monitor)=self.monitor(window) else {return;};
        let scale=monitor.scale_factor();let (outer,inner)=(window.outer_size(),client_size(window));let frame=(outer.width.saturating_sub(inner.width),outer.height.saturating_sub(inner.height));
        let (p,size)=self.restore(work_area(&monitor),scale,frame);
        window.set_outer_position(PhysicalPosition::new(p.0,p.1));window.set_inner_size(PhysicalSize::new(size.0,size.1));
    }
}

fn conversations_window(target:&tao::event_loop::EventLoopWindowTarget<Ev>,reference:&Window,geometry:Option<&ConversationGeometry>,pinned:bool)->Result<Window,String> {
    let mut builder=WindowBuilder::new().with_title("TaskQ Conversations").with_decorations(true).with_always_on_top(pinned).with_inner_size(LogicalSize::new(800.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false);
    // Select the target DPI before measuring decorations; a later cross-display move changes the frame.
    if let Some(monitor)=geometry.and_then(|g|g.monitor(reference)) {
        let area=work_area(&monitor);builder=builder.with_position(PhysicalPosition::new(area.0+area.2/2,area.1+area.3/2));
    }
    let window=builder.build(target).map_err(|e|e.to_string())?;
    if let Some(geometry)=geometry {geometry.apply(&window);}
    Ok(window)
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
            navigation: Value::Null, workspace_navigation: Value::Null, conversation_geometry: serde_json::from_value::<ConversationGeometry>(v["conversation_geometry"].clone()).ok().filter(ConversationGeometry::valid), notify: if v["notify"].is_object() { v["notify"].clone() } else { Value::Null } };
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
            "pinned": self.pin.pinned, "top": self.pin.top, "size": self.pin.size, "size_anchor": self.pin.size_anchor, "auto_anchor": self.pin.auto_anchor, "display": self.pin.display, "pos": self.pin.pos, "notify": self.notify, "navigation": self.navigation, "workspace_navigation": self.workspace_navigation, "conversation_geometry": self.conversation_geometry});
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

// Preserve the last durable navigation when validation, write or readback fails.
fn save_workspace_navigation(settings:&mut Settings,install:&Path,navigation:&Value)->Option<&'static str> {
    if !navigation.is_object() || navigation["schema"]!=1 {return Some("Workspace view state was not saved: unsupported schema; previous saved state retained");}
    if navigation.to_string().len()>65536 {return Some("Workspace view state was not saved: exceeds 64 KiB; previous saved state retained");}
    let previous=settings.workspace_navigation.clone();
    settings.workspace_navigation=navigation.clone();
    if settings.save(install) && crate::data::json(&install.join("tray.json")).ok().flatten().is_some_and(|v|v["workspace_navigation"]==*navigation) {return None;}
    settings.workspace_navigation=previous;
    Some("Workspace view state was not saved: storage write/readback failed; current view kept in this window")
}

// F19: retain legacy task pins in both surfaces without writing preferences during a read.
fn legacy_task_pins(st:&Value)->Vec<(String,Value)> {
    st["conversations"]["sidebar"]["pins"].as_array().into_iter().flatten().filter_map(|v| {
        let key=v.as_str()?.strip_prefix("task:")?;let (board,n)=key.rsplit_once('/')?;let n=n.parse::<u64>().ok().filter(|n|*n>0)?;
        let project=st["projects"].as_array().into_iter().flatten().find(|p|p["issues"]==board);
        let reference=project.and_then(|p|p["root"].as_str()).and_then(|root|crate::load(Path::new(root)).ok()).map(|(cfg,_)|format!("{}://{}/{}#{n}",cfg.board.name,cfg.web_host.to_lowercase(),cfg.repo)).unwrap_or_else(||format!("task:{key}"));
        let title=project.and_then(|p|p["tasks"].as_array()).and_then(|ts|ts.iter().find(|t|t["n"]==n)).and_then(|t|t["title"].as_str()).unwrap_or(key);
        Some((format!("task:{key}"),json!({"ref":reference,"title":title,"url":key})))
    }).collect()
}
fn project_task_pins(st:&mut Value) {
    let legacy=legacy_task_pins(st);if legacy.is_empty(){return;}
    if !st["navigation"].is_object(){st["navigation"]=json!({"schema":1});}
    if !st["navigation"]["dashboard"].is_object(){st["navigation"]["dashboard"]=json!({"schema":1,"pins":[]});}
    if st["navigation"]["dashboard"]["pins"].is_null(){st["navigation"]["dashboard"]["pins"]=json!([]);}
    if let Some(pins)=st["navigation"]["dashboard"]["pins"].as_array_mut(){for (_,pin) in legacy {if !pins.iter().any(|p|p["ref"]==pin["ref"]){pins.push(pin);}}}
}
fn clear_legacy_task_pins(install:&Path,st:&Value,dashboard:&Value)->Result<(),String> {
    let pins=dashboard["pins"].as_array().ok_or("Task pin readback unavailable")?;
    for (key,pin) in legacy_task_pins(st) {if !pins.iter().any(|p|p["ref"]==pin["ref"]) {
        let receipt=crate::conversations::sidebar(install,&json!({"sidebar":{"action":"pin","key":key,"value":false}}));
        if receipt["state"]!="accepted" {return Err(format!("Task pin partially saved; legacy unpin readback unknown: {}",receipt["reason"].as_str().unwrap_or("refresh before retrying")));}
    }}Ok(())
}

// Global task pins share the same dashboard metadata across task lists and Conversations.
fn task_pin(settings:&mut Settings,install:&Path,st:&Value,m:&Value)->Result<Value,String> {
    let pinned=m["pinned"].as_bool().ok_or("Pin selection missing")?;
    let observed=m["closedSeen"].as_i64().is_some_and(|at|at>=0);
    let (reference,row)=if (!pinned||observed)&&m["ref"].is_string() {
        (m["ref"].as_str().unwrap().to_string(),None)
    }else{
        let root=m["root"].as_str().ok_or("Pin project missing")?;
        let n=m["task"].as_u64().filter(|n|*n>0).ok_or("Pin task missing")?;
        let p=st["projects"].as_array().and_then(|ps|ps.iter().find(|p|p["root"]==root)).ok_or("Pin project unavailable")?;
        let (cfg,_)=crate::load(Path::new(root))?;
        let row=p["tasks"].as_array().into_iter().flatten().chain(p["reports"]["reports"].as_array().into_iter().flatten()).find(|t|t["n"]==n);
        (format!("{}://{}/{}#{n}",cfg.board.name,cfg.web_host.to_lowercase(),cfg.repo),row)
    };
    let mut projected=st.clone();projected["navigation"]=settings.navigation.clone();project_task_pins(&mut projected);
    let mut nav=if projected["navigation"].is_object(){projected["navigation"].clone()}else{json!({"schema":1})};
    if nav["dashboard"].is_null(){nav["dashboard"]=json!({"schema":1,"pins":[]});}
    let pins=nav["dashboard"]["pins"].as_array_mut().ok_or("Task views unavailable; refresh before pinning")?;
    if pinned&&observed {
        let pin=pins.iter_mut().find(|p|p["ref"]==reference).ok_or("Task pin unavailable; no observation saved")?;
        if m["closedSeen"]==0 {if let Some(pin)=pin.as_object_mut(){pin.remove("closedSeen");}}
        else if !pin["closedSeen"].as_i64().is_some_and(|at|at>0){pin["closedSeen"]=json!(now());}
    }else{
        if pinned&&row.is_none(){return Err("Task unavailable; no pin added".into());}
        if pinned&&pins.len()>=256&&!pins.iter().any(|pin|pin["ref"]==reference){return Err("Task pin limit reached; prior pins kept".into());}
        pins.retain(|pin|pin["ref"]!=reference);
        if pinned {let t=row.unwrap();pins.push(json!({"ref":reference,"title":t["title"],"url":t["url"]}));}
    }
    if nav.to_string().len()>65536{return Err("Task view exceeds 64 KiB; prior pins kept".into());}
    let previous=std::mem::replace(&mut settings.navigation,nav);
    if !settings.save(install)||!crate::data::json(&install.join("tray.json")).ok().flatten().is_some_and(|v|v["navigation"]==settings.navigation){settings.navigation=previous;return Err("Task pin save/readback unknown; refresh before changing it again".into());}
    clear_legacy_task_pins(install,st,&settings.navigation["dashboard"])?;
    Ok(settings.navigation["dashboard"].clone())
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

/// The drawable client size. Wry replaces a macOS window's content view, so tao's `inner_size` keeps its build-time value.
#[cfg(target_os="macos")]
fn client_size(window:&Window)->PhysicalSize<u32> {
    use tao::platform::macos::WindowExtMacOS;
    // SAFETY: the event loop owns this live NSWindow and calls us on its UI thread.
    let window_ref=unsafe {&*(window.ns_window() as *const objc2_app_kit::NSWindow)};
    let client=window_ref.contentRectForFrameRect(window_ref.frame()).size;
    LogicalSize::new(client.width,client.height).to_physical(window.scale_factor())
}
#[cfg(not(target_os="macos"))]
fn client_size(window:&Window)->PhysicalSize<u32> {window.inner_size()}

/// A transparent window keeps background-only body opacity; the titlebar alone gets the ordinary material behind its controls.
#[cfg(target_os="macos")]
fn native_titlebar(window:&Window)->Result<(),String> {
    use objc2_app_kit::{NSAutoresizingMaskOptions,NSVisualEffectBlendingMode,NSVisualEffectMaterial,NSVisualEffectState,NSVisualEffectView,NSWindowButton,NSWindowOrderingMode};
    use tao::platform::macos::WindowExtMacOS;
    let main=objc2::MainThreadMarker::new().ok_or("titlebar needs the UI thread")?;
    // SAFETY: the event loop owns this live NSWindow and calls us on its UI thread.
    let window_ref=unsafe {&*(window.ns_window() as *const objc2_app_kit::NSWindow)};
    let close=window_ref.standardWindowButton(NSWindowButton::CloseButton).ok_or("window has no Close button")?;
    let bar=unsafe {close.superview()}.ok_or("Close button has no titlebar view")?;
    let backing=NSVisualEffectView::new(main);
    backing.setMaterial(NSVisualEffectMaterial::Titlebar);
    backing.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    backing.setState(NSVisualEffectState::FollowsWindowActiveState);
    backing.setFrame(bar.bounds());
    backing.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable|NSAutoresizingMaskOptions::ViewHeightSizable);
    bar.addSubview_positioned_relativeTo(&backing,NSWindowOrderingMode::Below,None);
    Ok(())
}
#[cfg(not(target_os="macos"))]
fn native_titlebar(_:&Window)->Result<(),String> {Ok(())}

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
    let choices:Vec<_>=crate::runtime::setup_runtimes().map(|runtime|{
        let name=crate::runtime::of(runtime).client;
        let row=probe.and_then(|a|a["rows"].as_array()).and_then(|rows|rows.iter().find(|r|r["runtime"]==runtime));
        let reason=if !crate::runtime::owner::supported(runtime){format!("{name} owner sessions are unsupported on this platform.")}else{match row {
            Some(r) if r["login"]=="logged in" => String::new(),
            Some(r) if r["login"]=="not logged in" => format!("{name} is not logged in; inspect Agents in the tray."),
            _ => format!("{name} owner-session readiness is unknown; refresh or inspect Agents in the tray."),
        }};
        json!({"id":runtime,"name":name,"available":reason.is_empty(),"reason":reason})
    }).collect();
    let mut default=choices.iter().find(|r|r["id"]==crate::runtime::OWNER).cloned().unwrap();
    default["choices"]=json!(choices);default
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
    let mut c = crate::platform::command(std::env::current_exe().map_err(|e| e.to_string())?);
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
        let out = answer_cli(root, &["status".into(), "--json".into()])?;
        let snapshot = serde_json::from_slice(&out).map_err(|_| "Invalid status JSON; refresh the project")?;
        answer_snapshot(&snapshot, m)?;
        Ok((root, args))
    };
    let (root, args) = match prepare() {
        Ok(c) => c,
        Err(e) => return response("rejected", &e, "Refresh the question or open it on the board."),
    };
    let r = answer_dispatch(m, &args, |args| {
        match answer_cli(root, args) {
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

/// § 5: keep text and JSON outside process pipes; the own CLI bounds its board and lock waits, so wait for its exit (#2239).
fn answer_cli(root: &Path, args: &[String]) -> Result<Vec<u8>, String> {
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
        let out = crate::platform::complete("tray.answer-command", owner_command(root)?.args(&argv))?;
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
    let (cap_host, cap_saved, cap_revision,board_kind, project_limits,routing) = match crate::load(r) {
        Ok((c, raw)) => {let policy=crate::host::Policy::read(install);let rev=policy.as_ref().ok().and_then(|p|p.revision().ok());let host=policy.as_ref().map(|p|json!(p.me)).unwrap_or(Value::Null);
            (host, policy.map(|p|p.cap.json()).unwrap_or_else(|e| json!({"error": e})),json!(rev), json!(c.board.name), if raw["limits"].is_null() {json!({})} else {raw["limits"].clone()}, raw.get("routing").cloned().unwrap_or_else(||json!([])))},
        Err(e) => (Value::Null, json!({"error": e}), Value::Null,Value::Null, Value::Null,Value::Null),
    };
    let mut tasks = s["tasks"].as_array().cloned().unwrap_or_default();
    let (hosts, tasks_live) = liveness(&s, age, ttl, &mut tasks);
    let page = crate::load(r).ok().and_then(|(c, _)| crate::enrollment::progress_page(install, &c.repo));
    for task in &mut tasks {
        task_session(r, cap_host.as_str().unwrap_or(""), page.as_deref(), task);
        let n=task["n"].clone();
        for worker in task["workers"].as_array_mut().into_iter().flatten(){worker["n"]=n.clone();task_session(r,cap_host.as_str().unwrap_or(""),page.as_deref(),worker);}
        if task["worker"].is_object() {
            let mut worker=task["worker"].take();worker["n"]=task["n"].clone();
            task_session(r,cap_host.as_str().unwrap_or(""),page.as_deref(),&mut worker);task["worker"]=worker;
        }
        inline_images(r, task);
    }
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
    out["routing"] = routing;
    out["events_error"] = json!(events_error);
    out["report"] = s["report"].clone(); // R6: the manager view text for tray Attention
    out["incoming"] = s["incoming"].clone();
    // #1819: the stale line compares the board read age with the service poll
    out["poll"] = s["poll"].clone();
    out["requests"] = s["requests"].clone();
    out["reports"] = crate::reports::freshness(crate::reports::stored(r,&s),now(),ttl,bad_report_reason(&s,live));
    for field in ["reports","history"] {for report in out["reports"][field].as_array_mut().into_iter().flatten(){if let Some(row)=report.as_object_mut(){row.remove("body");row.remove("verdict");row.remove("tasks");row.insert("lazy".into(),json!(true));}}}

    out
}

// History paging reads the source revision; a confirmed answer survives the source's polling lag separately.
fn history_header(task:&mut Value) {
    if !task["revision"].is_number() {return;}
    let rows=match task["history"].take(){Value::Array(rows)=>rows,_=>Vec::new()};let prefix=task["history_source_count"].as_u64().unwrap_or(rows.len() as u64) as usize;
    let thin=task["history_thin"].take()==true;task.as_object_mut().map(|r|r.remove("history_thin"));
    task["history_count"]=json!(if thin{prefix+rows.len()}else{rows.len()});task["history_lazy"]=json!(true);let tail=rows.into_iter().skip(if thin{0}else{prefix}).collect::<Vec<_>>();
    if tail.is_empty(){task.as_object_mut().map(|t|t.remove("history"));}else{task["history_tail"]=json!(true);task["history"]=json!(tail);}
}

// Only the reader projection drops bodies; Pending, Home and OS delivery retain their canonical source rows.
fn workspace_snapshot(st:&Value)->Value {
    let mut out=serde_json::Map::new();
    for (key,value) in st.as_object().into_iter().flatten(){if !["projects","workspace_local"].contains(&key.as_str()){out.insert(key.clone(),value.clone());}}
    out.insert("projects".into(),json!(st["projects"].as_array().into_iter().flatten().map(|p|{
        let mut header=serde_json::Map::new();for(key,value)in p.as_object().into_iter().flatten(){if key!="tasks"{header.insert(key.clone(),value.clone());}}
        header.insert("tasks".into(),json!(p["tasks"].as_array().into_iter().flatten().map(crate::conversations::task_header).collect::<Vec<_>>()));Value::Object(header)
    }).collect::<Vec<_>>()));let mut out=Value::Object(out);
    if out.to_string().len()>16*1024*1024 {
        let reason="Conversation snapshot exceeds 16 MiB; coverage unknown; source and drafts kept";
        out=json!({"platform":st["platform"],"version":st["version"],"now":st["now"],"font":st["font"],"theme":st["theme"],"palette":st["palette"],"workspace_navigation":st["workspace_navigation"],"display":st["display"],
            "projects":st["projects"].as_array().into_iter().flatten().map(|p|json!({"root":p["root"],"name":p["name"],"issues":p["issues"],"alive":false,"complete":false,"error":reason,"tasks":[],"events":[],"reports":{"complete":false,"reports":[],"history":[],"reason":reason}})).collect::<Vec<_>>(),
            "messages":{"entries":[],"complete":false,"reason":reason},"conversations":{"items":[],"complete":false,"absence_complete":false,"reason":reason},"ai":{"runtime":{"available":false,"reason":reason}}});
        if out.to_string().len()>16*1024*1024 {out=json!({"projects":[],"messages":{"entries":[],"complete":false,"reason":reason},"conversations":{"items":[],"complete":false,"absence_complete":false,"reason":reason},"ai":{"runtime":{"available":false,"reason":reason}}});}
    }out
}

fn task_body_receipt(sync:&crate::pending::Pending,receipt:&mut Value) {
    if receipt["action"]!="task-body"||receipt["state"]!="accepted"{return;}
    let expected=receipt["value"]["expected_revision"].clone();let mut ps=vec![receipt["value"].take()];
    let locals=ps[0]["workspace_local"].take();
    for t in ps[0]["tasks"].as_array_mut().into_iter().flatten(){if t["history_thin"]!=true{t["history_source_revision"]=t["revision"].clone();t["history_source_count"]=json!(t["history"].as_array().map_or(0,Vec::len));}}
    sync.apply(&mut ps);
    let task=ps[0]["tasks"].as_array_mut().and_then(|a|a.first_mut());
    if let Some(t)=task.filter(|t|t["revision"]==expected){history_header(t);if t.to_string().len()>16*1024*1024{receipt["state"]=json!("unknown");receipt["reason"]=json!("Task card exceeds the 16 MiB source ceiling; open its board");return;}t["body_lazy"]=json!(false);receipt["value"]=t.take();receipt["workspace_local"]=locals;}
    else{receipt["state"]=json!("unknown");receipt["reason"]=json!("Task revision changed; refresh before reading its body");}
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
    for t in tasks {
        let active=matches!(t["state"].as_str(),Some("doing"|"review"))&&t["attempt"].is_u64();
        if let Some(h)=t.get_mut("heartbeat").filter(|h|h.is_object()) {
            if let Some(now)=elapsed.zip(s["observed"].as_i64()).map(|(e,obs)|obs+e) {
                crate::snapshot::refresh_heartbeat(h,now,active);
            } else if active {
                h["freshness"]=json!("unknown");h["warning"]=json!("heartbeat snapshot age unknown");
            }
        }
    }
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
    if !a.has("remote-control") && !own {
        if a.has("thread-link") {
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
                let operation=match job.message["a"].as_str() {Some("conversation-action")=>"tray.conversation.execute",Some("reports")=>"tray.reports.execute",Some("answer")=>"tray.answer.execute",Some("resources")=>"tray.resources.execute",Some("completed")=>"tray.history.execute",Some("recovery-read")=>"tray.recovery.read",Some("recovery-apply")=>"tray.recovery.apply",Some("restart")=>"tray.restart.execute",Some("stop")=>"tray.stop.execute",Some("uninstall")=>"tray.uninstall.execute",_=>operation};
                tracing::dispatcher::with_default(&job.dispatch,|| {
                    let _context=job.context.as_ref().map(|c|c.span().entered());
                    let result=coreq::telemetry::capture::operation(operation,||std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if registered&&!crate::host::project_paths(&install)?.iter().any(|r|r==&job.root) {
                            return Err("Project is no longer registered; command not applied".into());
                        }
                        let value=execute(&install,&job.root,&job.message)?;
                        if operation=="tray.capacity.execute" {
                            match value["state"].as_str() {
                                Some("rejected")=>tracing::info!(coreq_refusal="capacity-rejected"),
                                Some("unknown")=>tracing::info!(coreq_error="capacity-unknown"),
                                _=>{}
                            }
                        }
                        Ok(value)
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
        let name=match action {"conversation-action"=>"tray.conversation.dispatch","reports"=>"tray.reports.dispatch","answer"=>"tray.answer.dispatch","completed"=>"tray.history.dispatch","resources"=>"tray.resources.dispatch","recovery-read"|"recovery-apply"=>"tray.recovery.dispatch","pause"=>"tray.pause.dispatch","resume"=>"tray.resume.dispatch","focus"=>"tray.focus.dispatch","capacity"=>"tray.capacity.dispatch","restart"=>"tray.restart.dispatch","stop"=>"tray.stop.dispatch","uninstall"=>"tray.uninstall.dispatch",_=>"tray.invalid.dispatch"};
        coreq::telemetry::capture::operation(name,|| {
            if !self.allowed.contains(&action) {return Err(("invalid-command","Unsupported queued command"));}
            if id.is_empty() || id.len()>64 || !id.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'.')) {
                return Err(("invalid-id","Invalid command correlation id; action not submitted"));
            }
            let mut frozen=message.clone();frozen.as_object_mut().map(|m|m.remove("context"));
            let bytes=serde_json::to_vec(&frozen).map_err(|_|("request-too-large","Command payload too large; action not submitted"))?;
            // § 5: the envelope stays bounded; a person's literal reply has no length cap.
            let reply_bytes=if action=="conversation-action" && frozen["prompt"].is_string() {frozen["prompt"].to_string().len()}else if action=="answer" && frozen["op"]["text"].is_string() {frozen["op"]["text"].to_string().len()} else {0};
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
    let out=crate::platform::complete("tray.history-read",&mut c)?;
    serde_json::from_slice(&out.stdout).map_err(|_|String::from_utf8_lossy(&out.stderr).lines().find(|l|!l.trim().is_empty()).unwrap_or("board history read failed").to_string())
}

/// §7 Reports use the owner CLI, the registered project and its board token. Never trust task data as authority.
fn queued_reports(install:&Path,root:&Path,m:&Value)->crate::Res<Value> {
    let action=m["action"].as_str().ok_or("report action missing")?;
    if action=="list" {return crate::reports::cached_list(root,m["page"].as_u64().unwrap_or(1) as usize,None);}
    if action=="open" {
        let n=m["n"].as_u64().ok_or("report task missing")?;let sha=m["sha"].as_str().ok_or("report SHA missing")?;
        let mut reply=crate::reports::cached_open(root,n,sha)?;
        let locals=crate::conversations::preferences(install)?;
        if let Some((board,_))=reply["report"]["url"].as_str().and_then(|url|url.rsplit_once('/')) {let prefix=format!("result:{board}#{n}@{sha}:");reply["workspace_local"]=json!(locals["answers"].as_object().into_iter().flatten().filter(|(key,_)|key.starts_with(&prefix)).map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<_,_>>());}return Ok(reply);
    }
    if !["read","accept","followup","reopen","later","drop","make"].contains(&action) {return Err("unsupported result action".into());}
    let n=m["n"].as_u64().filter(|n|*n>0).ok_or("report task missing")?;
    let mut command=owner_command(root)?;
    command.arg("reports").arg(n.to_string()).args(["--action",action,"--sha",m["sha"].as_str().ok_or("report SHA missing")?]);
    if let Some(reason)=m["text"].as_str() {command.args(["--text",reason]);}
    if let Some(until)=m["until"].as_str().filter(|u|action=="later"&&crate::core::utc(u)) {command.args(["--until",until]);}
    command.arg("--json");
    let out=crate::platform::complete("tray.reports-command",&mut command)?;
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
    generation: u64,
    safe: bool,
    recovery_ready: bool,
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
    /// #2365: the bundle this page was built from, when it acknowledged boot, whether its boot or the next 5 s faulted,
    /// whether it replaced a page with no user present, and when it was last offered the active bundle.
    bundle: String,
    booted: Option<Instant>,
    start_fault: Option<&'static str>,
    background: bool,
    offered: Option<Instant>,
}
/// #1755: the running tray's `<install>/tray.fault`; the service heartbeat carries its code (`tray <code>`, § 4).
static FAULT_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static TICK_QUEUED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl WebCalls {
    fn new() -> Self {
        static GENERATION: std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
        let mut calls=Self{generation:GENERATION.fetch_add(1,std::sync::atomic::Ordering::Relaxed),safe:false,recovery_ready:false,workspace:false,surface:"main".into(),next:0,pending:Default::default(),counts:[0;4],saturated:false,failed:false,notified:false,metadata_unknown:false,logged:0,seen:Instant::now(),tick:None,bundle:crate::ui_bundle::active().version.clone(),booted:None,start_fault:None,background:false,offered:None};
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
            if reason!="page-not-ready" && self.booted.is_none_or(|at|at.elapsed()<START_PROOF) {self.start_fault=Some(reason);}
            eprintln!("taskq tray: interface outcomes unknown ({reason}); check service status before retry");
            if let Some(file)=FAULT_FILE.get() {let _=fs::write(file,reason);}
            let _=coreq::telemetry::capture::operation("tray.web.delivery",||Err::<(),_>(reason),|code|*code);
        }
    }
    fn finish(&mut self, id: u64, result: &'static str) {
        let Some((_,span))=self.pending.remove(&id) else {return};
        if id==0 && result=="ok" {self.booted=Some(Instant::now());}
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

/// #2160 A2: a capacity receipt refreshes snapshot revisions and reaches Settings as well as the menu and Workspace.
fn capmsg_refresh(receipt:&Value,shown:&mut String,workspace_shown:&mut String,settings_shown:&mut String)->bool {
    let cap=receipt["capmsg"].is_object();
    if cap {shown.clear();workspace_shown.clear();settings_shown.clear();}
    cap
}

// Ignore boot/snapshot checkpoints until the restored renderer acknowledges its recovery handoff.
fn workspace_checkpoint(calls:&WebCalls,checkpoints:&mut std::collections::BTreeMap<String,Value>,surface:&str,value:&Value)->Result<bool,&'static str> {
    if !calls.recovery_ready {return Ok(false);}
    let bytes=checkpoints.iter().filter(|(key,_)|key.as_str()!=surface).map(|(_,v)|v.to_string().len()).sum::<usize>()+value.to_string().len();
    if !value.is_object() || bytes>32*1024*1024 {return Err("Recovery checkpoint exceeds 32 MiB; reload refused, current reader retained");}
    checkpoints.insert(surface.into(),value.clone());Ok(true)
}

// The native closure stamps generation; retired renderer IPC never gets authority after reload.
fn recreate_workspace<W,V>(window:&W,view:&mut V,calls:&mut WebCalls,safe:bool,build:impl FnOnce(&W,&WebCalls)->Result<V,String>)->Result<(),String> {
    let mut reader=WebCalls::new();reader.workspace=calls.workspace;reader.surface=calls.surface.clone();reader.safe=safe;reader.background=std::mem::take(&mut calls.background);
    let next=build(window,&reader)?;
    *view=next;*calls=reader;Ok(())
}

/// A bundle has to stay clean this long after boot before it counts as started.
const START_PROOF: Duration = Duration::from_secs(5);
/// A page that declined adoption is asked again after this long.
const ADOPT_RETRY: Duration = Duration::from_secs(15);
const UI_CHECK_EVERY: Duration = Duration::from_secs(15*60);

/// #2365 A6: one navigation per webview, to the inline page wry loads; nothing after it.
fn navigation_allowed(armed:&std::sync::atomic::AtomicBool,url:&str)->bool {
    (url.starts_with("about:")||url.starts_with("data:"))&&armed.swap(false,std::sync::atomic::Ordering::SeqCst)
}

/// Every tray webview: only its inline page, no new windows; a link goes to the browser through the bridge.
fn page_builder<'a>(window:&Window,html:&str,calls:&WebCalls)->WebViewBuilder<'a> {
    let armed=std::sync::atomic::AtomicBool::new(true);
    WebViewBuilder::new().with_accept_first_mouse(true).with_html(html).with_focused(!calls.background||window.is_focused())
        .with_navigation_handler(move |url|navigation_allowed(&armed,&url))
        .with_new_window_req_handler(|_,_|wry::NewWindowResponse::Deny)
}

/// #2365 A6: the inline page has no origin (`about:blank`, a `data:` document); any other document's message stops here.
fn bridge_message(request:wry::http::Request<String>)->Option<String> {
    let inline=request.uri().scheme_str().is_none_or(|scheme|scheme.eq_ignore_ascii_case("about")||scheme.eq_ignore_ascii_case("data"));
    inline.then(||request.into_body())
}

fn surface_view(window:&Window,proxy:tao::event_loop::EventLoopProxy<Ev>,html:&str,init:&str,surface:PageSurface,calls:&WebCalls)->Result<WebView,String> {
    let generation=calls.generation;
    page_builder(window,html,calls).with_transparent(matches!(surface,PageSurface::Menu)).with_initialization_script(init).with_ipc_handler(move |r|{
        if let Some(body)=bridge_message(r) {let _=proxy.send_event(Ev::Page(surface,generation,body));}
    }).build(window).map_err(|e|e.to_string())
}

fn workspace_view(window:&Window, proxy:tao::event_loop::EventLoopProxy<Ev>, surface:String, calls:&WebCalls)->Result<WebView,String> {
    let generation=calls.generation;
    page_builder(window,&crate::ui_bundle::active().workspace,calls)
        .with_initialization_script(&format!("window.taskqNativeFrame=true;window.taskqSafeRecovery={};",calls.safe))
        .with_ipc_handler(move |r|{
            let Some(body)=bridge_message(r) else {return};
            if let Ok(mut m)=serde_json::from_str::<Value>(&body) {
                if let Some(m)=m.as_object_mut(){m.insert("readerGeneration".into(),json!(generation));}
                let _=proxy.send_event(Ev::Workspace(surface.clone(),m.to_string()));
            }
        }).build(window).map_err(|e|e.to_string())
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
    if !row["operation"].as_str().is_some_and(crate::ui_bundle::known_operation) {return Err("unknown-web-operation".into());}
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
fn web_eval(wv: &WebView, script: &str, name: &'static str, proxy: &tao::event_loop::EventLoopProxy<Ev>, calls: &mut WebCalls) {
    if calls.pending.contains_key(&0) {
        // #2365: a page booting behind a live window gets everything from its ready snapshot.
        if !calls.background {calls.fault("page-not-ready");}
        return;
    }
    let Some(id)=calls.begin(name,Instant::now()) else {return};
    if name==TICK {calls.tick=Some(id);}
    let p=proxy.clone();
    let workspace=calls.workspace;let surface=calls.surface.clone();let generation=calls.generation;
    if wv.evaluate_script_with_callback(&web_script(script),move |value| {let _=p.send_event(if workspace {Ev::WorkspaceEvaluated(generation,surface.clone(),id,web_result(&value))} else {Ev::Evaluated(generation,id,web_result(&value))});}).is_err() {
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
    let conversation_shots=snapshot.is_some() && routes.iter().any(|r|r=="conversations"||r=="conversations-checkpoint"||r=="reports-conversations"||r=="results-conversations"||r=="health-conversation"||r=="errors-conversations");
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
                &MenuItem::with_id("conversations-reload", "Reload Conversations safely", true, "CmdOrCtrl+R".parse::<Accelerator>().ok()), &MenuItem::with_id("conversations-back", "Conversations safe view", true, None), &MenuItem::with_id("conversations-close", "Close Conversations", true, "CmdOrCtrl+W".parse::<Accelerator>().ok()), &restart, &quit]).map_err(|e| e.to_string())?;
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
        let ev = match e.id.0.as_str() { "quit" => Ev::Quit, "restart" => Ev::Restart, "open" => Ev::Raise, "conversations-reload"=>Ev::RecoverWorkspace(String::new(),false), "conversations-back"=>Ev::RecoverWorkspace(String::new(),true), "conversations-close"=>Ev::WorkspaceClose, _ => return };
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
    // #1929 A6: the existing bounded queue keeps native holder probes off the window event loop.
    let replies=proxy.clone();
    let conversation_actions=ActionQueue::start(install.clone(),&["conversation-action"],false,
        |install,_,m|Ok(crate::conversations::act(install,m,&roots(install))),move |surface,id,result|{
            let receipt=result.unwrap_or_else(|reason|json!({"id":id,"state":"unknown","reason":reason}));
            let _=coreq::telemetry::capture::operation("tray.conversation.report",||replies.send_event(Ev::Conversation(surface.to_string_lossy().into(),receipt)),|_|"ui-closed");
        })?;
    let mut ui: Option<(Window, WebView)> = None;
    let mut workspace: Option<(Window, WebView)> = None;
    let mut workspace_web: Option<WebCalls> = None;
    let mut detached:std::collections::BTreeMap<String,(Window,WebView,WebCalls)>=Default::default();
    // #1819: each detached window keeps the signature it last received, never the menu's
    let mut detached_shown:std::collections::BTreeMap<String,String>=Default::default();
    let mut workspace_handoff=false;
    let mut workspace_open=Value::Null;
    let mut focused_workspace=String::from("main");
    let mut workspace_checkpoints:std::collections::BTreeMap<String,Value>=Default::default();
    let mut workspace_checkpoint_errors:std::collections::BTreeSet<String>=Default::default();
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
    let mut geometry_dirty=false;
    let mut hidden_at = Instant::now() - Duration::from_secs(1);
    let mut shown_at = hidden_at;
    let mut anchor: Option<Rect> = None;
    let mut step = 0usize;
    // snapshot pages wider than their window (#1073): `tray --snapshot` fails naming them
    let mut overflow: Vec<String> = Vec::new();
    let mut capture_errors:Vec<String>=Vec::new();
    let mut checkpoint_complete=false;
    let mut adding = Value::Null;
    let mut add_generation=0u64;
    let mut moved = false;
    let mut asked = false;
    let mut bursts = notify::Bursts::new();
    let mut shot_wait=None;
    // #2365: a snapshot shows the embedded copy unless a proof asks for the store.
    let mut ui_store=(snapshot.is_none()||crate::ui_bundle::snapshot_uses_store()).then(||crate::ui_bundle::Store::open(&install));
    let (mut next_ui_check,mut ui_checking)=(Instant::now(),false);
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

        if let Event::WindowEvent{window_id,event:WindowEvent::Focused(true),..}=&event {
            if workspace.as_ref().is_some_and(|(w,_)|w.id()==*window_id){focused_workspace="main".into();}
            else if let Some((id,_))=detached.iter().find(|(_, (w,_,_))|w.id()==*window_id){focused_workspace=id.clone();}
        }
        if snapshot.is_none() {
            if let Event::WindowEvent{window_id,event:WindowEvent::Moved(_)|WindowEvent::Resized(_),..}=&event {
                if let Some((window,_))=&workspace && window.id()==*window_id && let Some(value)=ConversationGeometry::capture(window) {
                    if settings.conversation_geometry.as_ref()!=Some(&value){settings.conversation_geometry=Some(value);geometry_dirty=true;}
                }
            }
            if geometry_dirty && matches!(&event,Event::UserEvent(Ev::Tick|Ev::WorkspaceClose)|Event::WindowEvent{event:WindowEvent::CloseRequested,..}) {
                geometry_dirty=false;
                if !settings.save(&install) || Settings::load(&install).conversation_geometry!=settings.conversation_geometry {
                    if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web){web_eval(wv,"taskq.receive({workspaceError:'Window geometry save failed; previous saved position may be retained'})","tray.web.notice",&proxy,calls);}
                }
            }
        }
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
            Event::UserEvent(Ev::WorkspaceClose)=>{if focused_workspace=="main" {if let Some((w,_))=&workspace {w.set_visible(false);}}else if let Some((w,_,_))=detached.get(&focused_workspace){w.set_visible(false);}}
            Event::UserEvent(Ev::Page(surface,generation,body))=>{
                let current=match surface {PageSurface::Menu=>Some(&web),PageSurface::Settings=>settings_web.as_ref(),PageSurface::Views=>views_web.as_ref()};
                if current.is_none_or(|calls|calls.generation!=generation) {return;}
                let _=proxy.send_event(match surface {PageSurface::Menu=>Ev::Ipc(body),PageSurface::Settings=>Ev::Settings(body),PageSurface::Views=>Ev::Views(body)});
            }
            Event::UserEvent(Ev::BundleChecked(checked))=>{
                if let Some(store)=&mut ui_store {store.apply(checked);}
                ui_checking=false;
                shown.clear();workspace_shown.clear();settings_shown.clear();views_shown.clear();detached_shown.clear();
            }
            // Same recreation as Reload; the checkpoint and the ready handoff carry the reader across.
            Event::UserEvent(Ev::AdoptPage(surface))=>{
                if workspace_checkpoint_errors.contains(&surface) {return;}
                let bundle=crate::ui_bundle::active();
                let (mut failure,mut retired)=(None,None);
                match surface.as_str() {
                    MENU_SURFACE=>if let Some((window,view))=&mut ui {
                        web.background=true;shown.clear();
                        if let Err(error)=recreate_workspace(window,view,&mut web,false,|window,reader|surface_view(window,proxy.clone(),&bundle.tray,"",PageSurface::Menu,reader)) {failure=Some(error);retired=Some(&mut web);}
                    },
                    SETTINGS_SURFACE=>if let (Some((window,view)),Some(calls))=(&mut settings_window,&mut settings_web) {
                        calls.background=true;settings_shown.clear();
                        if let Err(error)=recreate_workspace(window,view,calls,false,|window,reader|surface_view(window,proxy.clone(),&bundle.tray,"window.taskqSurface='settings';",PageSurface::Settings,reader)) {failure=Some(error);retired=Some(calls);}
                    },
                    VIEWS_SURFACE=>if let (Some((window,view)),Some(calls))=(&mut views_window,&mut views_web) {
                        calls.background=true;views_shown.clear();
                        if let Err(error)=recreate_workspace(window,view,calls,false,|window,reader|surface_view(window,proxy.clone(),&bundle.tray,"window.taskqSurface='views';",PageSurface::Views,reader)) {failure=Some(error);retired=Some(calls);}
                    },
                    "main"=>if let (Some((window,view)),Some(calls))=(&mut workspace,&mut workspace_web) {
                        calls.background=true;workspace_shown.clear();
                        let safe=calls.safe;
                        if let Err(error)=recreate_workspace(window,view,calls,safe,|window,reader|workspace_view(window,proxy.clone(),"main".into(),reader)) {failure=Some(error);retired=Some(calls);}
                    },
                    id=>if let Some((window,view,calls))=detached.get_mut(id) {
                        calls.background=true;detached_shown.remove(id);
                        let safe=calls.safe;
                        if let Err(error)=recreate_workspace(window,view,calls,safe,|window,reader|workspace_view(window,proxy.clone(),surface.clone(),reader)) {failure=Some(error);retired=Some(calls);}
                    },
                }
                // The old page stays; it is not asked again until a newer bundle, and Reload remains its way out.
                if let (Some(error),Some(calls))=(failure,retired) {calls.log(format_args!("interface bundle not adopted: {error}"));calls.bundle=bundle.version.clone();}
            }
            Event::UserEvent(Ev::RecoverWorkspace(surface,safe))=>{
                let surface=if surface.is_empty(){focused_workspace.clone()}else{surface};
                if workspace_checkpoint_errors.contains(&surface){eprintln!("taskq tray: Conversations recovery checkpoint exceeds 32 MiB; reader retained, reload refused");if let Some(tray)=&tray {let _=tray.set_tooltip(Some("Conversations recovery unknown: checkpoint exceeds 32 MiB; reader retained"));}return;}
                if surface!="main" {
                    if let Some((window,view,calls))=detached.get_mut(&surface){
                        match recreate_workspace(window,view,calls,safe,|window,reader|workspace_view(window,proxy.clone(),surface.clone(),reader)){
                            Ok(())=>{window.set_visible(true);window.set_focus();}
                            Err(reason)=>eprintln!("taskq tray: detached Conversations reload unavailable: {reason}"),
                        }
                    }
                    return;
                }
                if let (Some((window,view)),Some(calls))=(&mut workspace,&mut workspace_web) {
                    match recreate_workspace(window,view,calls,safe,|window,reader|workspace_view(window,proxy.clone(),"main".into(),reader)) {
                        Ok(())=>{workspace_shown.clear();if safe {workspace_open=json!({"page":"conversations","thread":"recovery"});}window.set_visible(true);window.set_focus();}
                        Err(reason)=>{eprintln!("taskq tray: Conversations reload unavailable: {reason}; existing window retained");}
                    }
                }
            }
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
                let b = WindowBuilder::new().with_window_icon(Some(window_icon(&compact(true,false,256.0/22.0)))).with_title("TaskQ").with_transparent(true).with_decorations(true).with_resizable(false).with_always_on_top(settings.pin.top)
                    .with_visible(snapshot.is_some()).with_inner_size(if routes.iter().any(|r|r.starts_with("health-")) {if conversation_shots {LogicalSize::new(1100.0,900.0)} else {LogicalSize::new(420.0,900.0)}} else if views_shots {LogicalSize::new(480.0,600.0)} else if conversation_shots && routes.iter().any(|r|r!="conversations") {LogicalSize::new(1280.0,760.0)} else if conversation_shots {LogicalSize::new(800.0,600.0)} else {LogicalSize::new(320.0, 400.0)}).with_position(tao::dpi::LogicalPosition::new(40.0, 40.0));
                // ordinary snapshots render off-screen; opacity evidence must remain visible and unoccluded (R12)
                let b = if snapshot.is_some() { b.with_position(tao::dpi::LogicalPosition::new(if agentq_shots||opacity_shots{40.0}else{-20000.0}, if agentq_shots||opacity_shots{40.0}else{-20000.0})).with_always_on_top(opacity_shots).with_focused(false) } else { b };
                #[cfg(windows)]
                let b = tao::platform::windows::WindowBuilderExtWindows::with_skip_taskbar(b, !settings.pin.pinned);
                let p = proxy.clone();
                let built = b.build(target).map_err(|e| e.to_string()).and_then(|w| {
                    let bundle=crate::ui_bundle::active();
                    surface_view(&w,p,if conversation_shots {&bundle.workspace} else {&bundle.tray},if views_shots {"window.taskqSurface='views';"} else {""},PageSurface::Menu,&web).map(|v| (w, v))
                });
                match built {
                    Ok(x) => {
                        if let Err(error)=native_titlebar(&x.0) {eprintln!("taskq tray: titlebar unchanged: {error}");}
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
                    &MenuItem::with_id("conversations-reload", "Reload Conversations safely", true, None), &MenuItem::with_id("conversations-back", "Conversations safe view", true, None), &MenuItem::with_id("conversations-close", "Close Conversations", true, None), &MenuItem::with_id("restart", "Restart TaskQ", true, None), &MenuItem::with_id("quit", "Quit TaskQ", true, None)]);
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
            Event::WindowEvent { window_id,event: WindowEvent::Moved(p), .. } if snapshot.is_none()&&ui.as_ref().is_some_and(|(w,_)|w.id()==window_id&&(settings.pin.pinned||native_header_drag(w))) => {
                let Some((w, _)) = &ui else { return };
                if !settings.pin.pinned {settings.pin.pinned=true;#[cfg(windows)] {use tao::platform::windows::WindowExtWindows;w.set_skip_taskbar(false);}settings.save(&install);let _=proxy.send_event(Ev::Sync);}
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
                // #1819, #2352: while any window is visible and focused the service polls the board every 15 s (M8)
                let front=|w:&Window|w.is_visible() && w.is_focused();
                let visible=|o:&Option<(Window,WebView)>|o.as_ref().is_some_and(|(w,_)|front(w));
                if visible(&ui) || visible(&workspace) || visible(&settings_window) || visible(&views_window) || detached.values().any(|(w,_,_)|front(w)) {
                    crate::touch(&install.join("menu-open"));
                }
                // #2365 A3/A5: faults retire a bundle, a clean start proves it, and each page moves when it holds nothing in flight.
                if let Some(store)=&mut ui_store {
                    let mut pages:Vec<&mut WebCalls>=vec![&mut web];
                    pages.extend(workspace_web.as_mut());pages.extend(settings_web.as_mut());pages.extend(views_web.as_mut());pages.extend(detached.values_mut().map(|(_,_,calls)|calls));
                    for calls in &mut pages {
                        let Some(reason)=calls.start_fault.take() else {continue};
                        let bundle=calls.bundle.clone();
                        if bundle!=crate::ui_bundle::embedded().version {
                            eprintln!("taskq tray: interface bundle {bundle} failed to start ({reason})");
                            store.fail(&bundle,&format!("failed to start ({reason})"));
                            let _=fs::remove_file(install.join("tray.fault"));
                            icon_sig.clear();
                        }
                    }
                    let active=crate::ui_bundle::active();
                    if store.on_trial() && pages.iter().any(|calls|calls.bundle==active.version&&!calls.failed&&calls.booted.is_some_and(|at|at.elapsed()>=START_PROOF)) {store.commit();}
                    let now=Instant::now();
                    // A page with a user on it decides for itself; the receive hook declines while an edit is open.
                    let offer=|view:&WebView,calls:&mut WebCalls,surface:&str,eligible:bool|{
                        if calls.bundle==active.version {return;}
                        if calls.failed {let _=proxy.send_event(Ev::AdoptPage(surface.into()));return;}
                        if !eligible||!calls.idle()||calls.offered.is_some_and(|at|now.saturating_duration_since(at)<ADOPT_RETRY) {return;}
                        calls.offered=Some(now);
                        web_eval(view,"taskq.receive({bundleAdopt:true})","tray.web.receive",&proxy,calls);
                    };
                    if let Some((window,view))=&ui {offer(view,&mut web,MENU_SURFACE,!window.is_visible()||settings.pin.pinned);}
                    if let (Some((window,view)),Some(calls))=(&settings_window,&mut settings_web) {offer(view,calls,SETTINGS_SURFACE,!window.is_visible());}
                    if let (Some((window,view)),Some(calls))=(&views_window,&mut views_web) {offer(view,calls,VIEWS_SURFACE,!window.is_visible());}
                    if let (Some((_,view)),Some(calls))=(&workspace,&mut workspace_web) {offer(view,calls,"main",true);}
                    for (id,(_,view,calls)) in detached.iter_mut() {offer(view,calls,id,true);}
                    if crate::ui_bundle::follows_channel() && !ui_checking && now>=next_ui_check {
                        ui_checking=true;next_ui_check=now+UI_CHECK_EVERY;
                        let (check,p)=(store.check(),proxy.clone());
                        std::thread::spawn(move ||{let _=p.send_event(Ev::BundleChecked(check.run(&crate::ui_bundle::fetch)));});
                    }
                }
                if !web.failed {refresh_icon(&tray, &st, &settings, dark, &mut icon_sig, ui.as_ref().map(|(w,_)|w));}
                notes(&install, &mut settings, &st, &mut asked, &mut bursts);
                work_item.set_text(quit_label(&st, quitting));
                let mut comparable=st.clone();comparable.as_object_mut().expect("tray snapshot object").remove("now");
                let sig=comparable.to_string();
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {
                    if sig!=workspace_shown && !calls.failed && calls.idle() {workspace_shown=sig.clone();web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&st)),TICK,&proxy,calls);}
                }
                if let (Some((_,wv)),Some(calls))=(&settings_window,&mut settings_web) {
                    if sig!=settings_shown && !calls.failed && calls.idle() {settings_shown=sig.clone();web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}
                }
                if let (Some((_,wv)),Some(calls))=(&views_window,&mut views_web) {
                    if sig!=views_shown && !calls.failed && calls.idle() {views_shown=sig.clone();web_eval(wv,&format!("taskq.receive({st})"),TICK,&proxy,calls);}
                }
                detached_shown.retain(|id,_|detached.contains_key(id));
                for (id,(_,wv,calls)) in detached.iter_mut(){if take_due(detached_shown.entry(id.clone()).or_default(),&sig,!calls.failed && calls.idle()){web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&st)),TICK,&proxy,calls);}}
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
                let packet=if receipt["answerReceipt"].is_object() && sync.receipt(&receipt) {
                    let snapshot=state(&install,&settings,dark,None,&adding,&mut sync);
                    seen_projects=snapshot["projects"].clone();
                    let _=proxy.send_event(Ev::Sync);
                    json!({"updates":[snapshot,receipt]})
                } else {receipt.clone()};
                let cap=capmsg_refresh(&receipt,&mut shown,&mut workspace_shown,&mut settings_shown);
                if cap {let _=proxy.send_event(Ev::Sync);}
                push(&mut web,&ui,&packet);
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);}
                if cap && let (Some((_,wv)),Some(calls))=(&settings_window,&mut settings_web) && !calls.failed {web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);}
                // #1819: a detached conversation sees the answer it may show too.
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
                    push(&mut web,&ui, &json!({"open": {"root": id["root"], "n": id["n"], "kind": id["k"], "title": id["t"], "event":id["e"]}}));
                }
            }
            Event::UserEvent(Ev::Evaluated(generation,id,result)) => {
                if web.generation!=generation {return;}
                web.finish(id,result);
                if web.pending.is_empty() && let Some(next)=shot_wait.take() {let _=proxy.send_event(next);}
            }
            Event::UserEvent(Ev::Dialogue(surface,mut receipt)) => {
                let current=if surface=="main" {workspace_web.as_ref()}else{detached.get(&surface).map(|(_,_,c)|c)};
                if current.is_none_or(|c|receipt["readerGeneration"].as_u64()!=Some(c.generation)){return;}
                task_body_receipt(&sync,&mut receipt);
                let script=format!("taskq.receive({})",json!({"dialogueReceipt":receipt}));
                if surface=="main" {if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&script,"tray.web.receive",&proxy,calls);}}
                else if let Some((_,wv,calls))=detached.get_mut(&surface) {web_eval(wv,&script,"tray.web.receive",&proxy,calls);}
            }
            Event::UserEvent(Ev::Conversation(surface,receipt)) => {
                let script=format!("taskq.receive({})",json!({"conversationReceipt":receipt}));
                if surface=="main" {if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&script,"tray.web.receive",&proxy,calls);}}
                else if let Some((_,wv,calls))=detached.get_mut(&surface) {web_eval(wv,&script,"tray.web.receive",&proxy,calls);}
                shown.clear();workspace_shown.clear();detached_shown.clear();
            }
            Event::UserEvent(Ev::WorkspaceEvaluated(generation,surface,id,result)) => {if surface=="main" {if let Some(calls)=&mut workspace_web {if calls.generation==generation {calls.finish(id,result);}}}else if surface==SETTINGS_SURFACE {if let Some(calls)=&mut settings_web {if calls.generation==generation {calls.finish(id,result);}}}else if surface==VIEWS_SURFACE {if let Some(calls)=&mut views_web {if calls.generation==generation {calls.finish(id,result);}}}else if let Some((_,_,calls))=detached.get_mut(&surface){if calls.generation==generation {calls.finish(id,result);}}}
            Event::UserEvent(Ev::Workspace(surface,message)) => {
                let Ok(input)=serde_json::from_str::<Value>(&message) else {return};
                let current=if surface=="main" {workspace_web.as_ref()}else{detached.get(&surface).map(|(_,_,c)|c)};
                if current.is_none_or(|c|input["readerGeneration"].as_u64()!=Some(c.generation)){return;}
                if input["a"]=="workspace-origin" && surface=="main" && input["origin"]["surface"]=="settings" {
                    // TW1: Settings keeps its DOM, route, scroll and focus while the shared review is open.
                    if let Some((window,_))=&settings_window {window.set_visible(true);window.set_focus();if let Some((window,_))=&workspace {window.set_visible(false);}}
                    return;
                }
                if input["a"]=="conversation-window" {
                    let id=input["conversation"].as_str().unwrap_or("");
                    if !crate::resources::id(id){return;}
                    let action=input["windowAction"].as_str().or_else(||input["action"].as_str()).unwrap_or("");
                    if action=="raise" {if let Some((window,_,calls))=detached.get(id){if calls.failed {let _=proxy.send_event(Ev::RecoverWorkspace(id.into(),false));}else{window.set_visible(true);window.set_focus();}}return;}
                    if action=="detach" && surface=="main" && !detached.contains_key(id) {
                        let saved=crate::conversations::summary(&install);
                        let valid=saved["items"].as_array().is_some_and(|rows|rows.iter().any(|c|c["id"]==id&&c["revision"]==input["revision"]));
                        if !valid {return;}
                        let p=proxy.clone();let source=id.to_string();let mut reader=WebCalls::new();reader.workspace=true;reader.surface=source.clone();
                        let built=WindowBuilder::new().with_title("TaskQ Conversations").with_decorations(true).with_inner_size(LogicalSize::new(640.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window|{
                            workspace_view(&window,p,source,&reader).map(|wv|(window,wv))
                        });
                        match built {
                            Ok((window,wv))=>{window.set_visible(true);window.set_focus();detached.insert(id.into(),(window,wv,reader));}
                            Err(_)=>{if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web){let r=json!({"conversationReceipt":{"id":input["id"],"state":"unknown","reason":"Detached window unavailable; main retains ownership"}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}return;}
                        }
                    }else if action=="return" && surface==id {detached.remove(id);workspace_checkpoints.remove(id);if let Some((window,_))=&workspace{window.set_visible(true);window.set_focus();}}
                    else {return;}
                    if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web){
                        let owner=json!({"workspaceOwner":{"id":"main","detached":detached.keys().collect::<Vec<_>>()}});
                        web_eval(wv,&format!("taskq.receive({owner})"),"tray.web.receive",&proxy,calls);
                        if action=="return" {web_eval(wv,&format!("taskq.receive({});taskq.receive({})",workspace_snapshot(&state(&install,&settings,dark,None,&adding,&mut sync)),json!({"open":{"page":"conversations"}})),"tray.web.receive",&proxy,calls);let select=json!({"workspaceOwner":{"id":"main","select":id,"detached":detached.keys().collect::<Vec<_>>()}});web_eval(wv,&format!("taskq.receive({select})"),"tray.web.receive",&proxy,calls);}
                        else {let r=json!({"conversationReceipt":{"id":input["id"],"state":"accepted","snapshot":crate::conversations::summary(&install)}});web_eval(wv,&format!("taskq.receive({r})"),"tray.web.receive",&proxy,calls);}
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
                if m["a"]=="workspace-recovered" {if !calls.pending.contains_key(&0){calls.recovery_ready=true;}return;}
                if m["a"]=="workspace-checkpoint" {
                    match workspace_checkpoint(calls,&mut workspace_checkpoints,&surface,&m["value"]) {
                        Ok(true)=>{workspace_checkpoint_errors.remove(&surface);}
                        Ok(false)=>{}
                        Err(reason)=>{workspace_checkpoint_errors.insert(surface.clone());web_eval(wv,&format!("taskq.receive({})",json!({"workspaceLocalError":reason})),"tray.web.receive",&proxy,calls);}
                    }
                    return;
                }
                if m["a"]=="workspace-reload" {let _=proxy.send_event(if m["adopt"]==true {Ev::AdoptPage(surface.clone())} else {Ev::RecoverWorkspace(surface.clone(),m["safe"]==true)});return;}
                if m["a"]=="update"||m["a"]=="ui-check" {next_ui_check=Instant::now();}
                if m["a"]=="ui-check" {return;}
                if m["a"]=="hide" {w.set_visible(false);return;}
                if calls.failed {return;}
                match m["a"].as_str() {
                    Some("reports")=>{let result=if m["local"].is_object(){answer_local(&install,m["local_key"].as_str().unwrap_or(""),&m["local"])}else{Ok(())}.and_then(|_|{let mut request=m.clone();request.as_object_mut().map(|v|{v.remove("local");v.remove("local_key");});report_actions.submit(&request)});if let Err(reason)=result {web_eval(wv,&format!("taskq.receive({})",json!({"reportReceipt":{"id":m["id"],"root":m["root"],"reason":reason}})),"tray.web.receive",&proxy,calls);}}
                    Some("sync-retry") => {
                        if let Some(request)=sync.retry(m["root"].as_str().unwrap_or(""),m["id"].as_str().unwrap_or("")) {let _=send_answer(&answer_actions,&mut sync,&request,&seen_projects,&proxy);}
                    }
                    Some("ready") | Some("refresh") => {
                        let _=proxy.send_event(Ev::Sync);
                        calls.finish(0,"ok");
                        let mut messages=vec![workspace_snapshot(&state(&install,&settings,dark,None,&adding,&mut sync))];
                        if m["a"]=="ready" && let Some(value)=workspace_checkpoints.get(&surface) {messages.push(json!({"workspaceRecovery":value}));}
                        if surface=="main" && !workspace_open.is_null() && !workspace_open["page"].is_null(){messages.push(json!({"open":workspace_open.take()}));}
                        let own=if surface=="main" {json!({"id":"main","detached":detached_ids})}else{json!({"id":surface,"conversation":surface,"detached":[]})};
                        messages.push(json!({"workspacePin":w.is_always_on_top(),"workspaceMaximized":w.is_maximized()}));
                        if surface!="main" && m["a"]=="ready" {messages.push(json!({"open":{"page":"conversations","thread":if calls.safe {json!("recovery")} else {Value::Null}}}));}
                        messages.push(json!({"workspaceOwner":own}));
                        if m["a"]=="ready" {messages.push(json!({"workspaceRecoveryReady":true}));}
                        // One evaluation: a failed restore cannot acknowledge or replace its previous checkpoint.
                        let script=messages.iter().map(|v|format!("taskq.receive({v})")).collect::<Vec<_>>().join(";");
                        web_eval(wv,&script,"tray.web.receive",&proxy,calls);
                    }
                    Some("task-pin")=>{let st=state(&install,&settings,dark,None,&adding,&mut sync);let receipt=match task_pin(&mut settings,&install,&st,&m){Ok(dashboard)=>{push(&mut web,&ui,&json!({"dashboard":dashboard}));json!({"dashboard":dashboard})},Err(reason)=>json!({"taskPinError":reason})};web_eval(wv,&format!("taskq.receive({receipt})"),"tray.web.receive",&proxy,calls);}
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
                        if allowed&&matches!(m["action"].as_str(),Some("reply"|"reply-check"|"watch"|"unwatch")) {
                            let mut request=m.clone();request["root"]=json!(surface);
                            if let Err(reason)=conversation_actions.submit(&request){web_eval(wv,&format!("taskq.receive({})",json!({"conversationReceipt":{"id":m["id"],"state":"rejected","reason":reason}})),"tray.web.receive",&proxy,calls);}return;
                        }
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
                        std::thread::spawn(move||{let mut receipt=crate::conversations::dialogue(&install,&m);receipt["readerGeneration"]=m["readerGeneration"].clone();let _=coreq::telemetry::capture::operation("tray.dialogue.report",||p.send_event(Ev::Dialogue(surface,receipt)),|_|"ui-closed");});
                    }
                    Some("message-action") => {
                        let ps:Vec<Value>=roots(&install).iter().map(|r|project(&install,r)).collect();
                        let keys:Vec<String>=m["keys"].as_array().into_iter().flatten().filter_map(|v|v.as_str().map(String::from)).collect();
                        let mut receipt=crate::messages::act(&install,&ps,m["action"].as_str().unwrap_or(""),&keys);receipt["id"]=m["id"].clone();
                        web_eval(wv,&format!("taskq.receive({})",json!({"messageReceipt":receipt})),"tray.web.receive",&proxy,calls);
                        web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&state(&install,&settings,dark,None,&adding,&mut sync))),"tray.web.receive",&proxy,calls);
                        push(&mut web,&ui,&state(&install,&settings,dark,None,&adding,&mut sync));
                    }
                    Some("workspace-set") => {
                        let reason=save_workspace_navigation(&mut settings,&install,&m["navigation"]);
                        web_eval(wv,&format!("taskq.receive({})",json!({"viewStateError":reason})),"tray.web.receive",&proxy,calls);
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
                        if a=="ai-send" {web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&state(&install,&settings,dark,None,&adding,&mut sync))),"tray.web.receive",&proxy,calls);}
                    }
                    // Workspace reviews use the same queues, native picker and receipt adapters as the tray.
                    Some(a) if workspace_operation(a) => {if let Some(message)=workspace_forward(&m) {let _=proxy.send_event(Ev::Ipc(message));}}
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
                    "bundle-reload" => {let _=proxy.send_event(Ev::AdoptPage(SETTINGS_SURFACE.into()));}
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
                    "bundle-reload" => {let _=proxy.send_event(Ev::AdoptPage(VIEWS_SURFACE.into()));}
                    "ready" => {
                        calls.finish(0,"ok");
                        let st=state(&install,&settings,dark,None,&adding,&mut sync);
                        web_eval(vwv,&format!("taskq.receive({st})"),"tray.web.receive",&proxy,calls);
                        if st["navigation"]["dashboard"].is_object() {web_eval(vwv,&format!("taskq.receive({})",json!({"dashboard":st["navigation"]["dashboard"],"initial":true})),"tray.web.receive",&proxy,calls);}
                    }
                    "hide" => vw.set_visible(false),
                    "drag" => {let _=vw.drag_window();}
                    "workspace" | "task-pin" => {let _=proxy.send_event(Ev::Ipc(message));}
                    "views-dashboard" if m["dashboard"].is_object() && m["dashboard"].to_string().len()<=65536 => push(&mut web,&ui,&json!({"dashboard":m["dashboard"]})),
                    // ponytail: the View window owns no other action; add arms when it gains one
                    _ => {}
                }
            }
            Event::UserEvent(Ev::Ipc(m)) => {
                let Ok(m) = serde_json::from_str::<Value>(&m) else {web.fault("invalid-ipc");return;};
                if m["a"]=="bundle-reload" {let _=proxy.send_event(Ev::AdoptPage(MENU_SURFACE.into()));return;}
                if m["a"]=="ui-check" {next_ui_check=Instant::now();return;}
                if m["a"]=="update" {next_ui_check=Instant::now();}
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
                        if workspace_web.as_ref().is_some_and(|c|c.failed) {let _=proxy.send_event(Ev::RecoverWorkspace("main".into(),false));return;}
                        // #1699: the header entry (it names a thread) opens the main workspace, never a detached conversation
                        if workspace_open["page"]=="conversations" && workspace_open["thread"].is_null() && workspace_open["task"].is_null() {if let Some(id)=requests_file(&install)["conversation_navigation"]["selected"].as_str(){if let Some((window,_,_))=detached.get(id){window.set_visible(true);window.set_focus();return;}}}
                        if workspace.is_none() {
                            let p=proxy.clone();let mut reader=WebCalls::new();reader.workspace=true;
                            let built=conversations_window(target,w,settings.conversation_geometry.as_ref(),requests_file(&install)["workspace_pin"]==true).and_then(|window| {
                                workspace_view(&window,p,"main".into(),&reader).map(|wv|(window,wv))
                            });
                            match built {
                                Ok(window)=>{workspace=Some(window);workspace_web=Some(reader);}
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
                            let mut calls=WebCalls::new();calls.workspace=true;calls.surface=SETTINGS_SURFACE.into();
                            let built=WindowBuilder::new().with_title("TaskQ Settings").with_decorations(true).with_always_on_top(requests_file(&install)["settings_top"]==true).with_inner_size(LogicalSize::new(420.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                                surface_view(&window,p,&crate::ui_bundle::active().tray,"window.taskqSurface='settings';",PageSurface::Settings,&calls).map(|wv|(window,wv))
                            });
                            match built {
                                Ok(window)=>{settings_window=Some(window);settings_web=Some(calls);settings_shown.clear();}
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
                            let mut calls=WebCalls::new();calls.workspace=true;calls.surface=VIEWS_SURFACE.into();
                            let built=WindowBuilder::new().with_title("TaskQ View").with_decorations(true).with_inner_size(LogicalSize::new(480.0,600.0)).with_min_inner_size(LogicalSize::new(320.0,240.0)).with_visible(false).build(target).map_err(|e|e.to_string()).and_then(|window| {
                                surface_view(&window,p,&crate::ui_bundle::active().tray,"window.taskqSurface='views';",PageSurface::Views,&calls).map(|wv|(window,wv))
                            });
                            match built {
                                Ok(window)=>{views_window=Some(window);views_web=Some(calls);views_shown.clear();}
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
                    "snapshot-checkpoint" if snapshot.is_some() && routes.iter().any(|r|r=="conversations-checkpoint") && m["state"]=="passed" => {checkpoint_complete=true;println!("PASS native Conversations checkpoint: {}",m["value"]);}
                    "overflow" if snapshot.is_some() => overflow.push(format!("{} by {} px", m["shot"].as_str().unwrap_or("?"), m["x"])),
                    "ready" => {
                        let _=proxy.send_event(Ev::Sync);
                        web.finish(0,"ok");
                        if snapshot.is_some() {
                            let _ = proxy.send_event(Ev::Shot);
                            return;
                        }
                        // M3.26: restore navigation and native window controls before the first fit escapes.
                        reply!(&json!({"updates":[state(&install,&settings,dark,None,&adding,&mut sync),{"settingsWindow":true,"viewsWindow":true}]}));
                    }
                    "size" | "resize" => {
                        let Some((width, height)) = m["w"].as_f64().zip(m["h"].as_f64()).filter(|(w, h)| w.is_finite() && h.is_finite()) else { return };
                        let Some(scale) = m["d"].as_f64().filter(|k| k.is_finite() && (0.5..=8.0).contains(k)) else { return };
                        let (max_w, max_h) = w.current_monitor().or_else(|| w.primary_monitor()).map_or((1920.0, 800.0), |monitor| {
                            let area = work_area(&monitor);
                            (f64::from(area.2) / scale, f64::from(area.3) / scale)
                        });
                        let outer=w.outer_size();let inner=client_size(w);
                        let frame=(outer.width.saturating_sub(inner.width),outer.height.saturating_sub(inner.height));
                        let max_w=(max_w-f64::from(frame.0)/scale).max(240.0);let max_h=(max_h-f64::from(frame.1)/scale).max(120.0);
                        let size = (width.max(240.0).min(max_w), height.max(120.0).min(max_h));
                        let measurement = json!({"w": size.0, "h": if m["a"] == "resize" { size.1 } else { height.max(120.0) }, "d": scale, "g": m["g"]});
                        let (width, height) = popover_size(&measurement, max_h * scale / w.scale_factor(), w.scale_factor());
                        let old_size = w.outer_size();
                        let old_position = w.outer_position().ok();
                        w.set_inner_size(PhysicalSize::new(width.min((max_w * scale) as u32), height));
                        if m["a"] == "size" && settings.pin.size_anchor != "fixed" {
                            if let (Some(p), Some(monitor)) = (old_position, w.current_monitor()) {
                                let p = anchored_position((p.x,p.y),old_size.height as i32,((width+frame.0) as i32,(height+frame.1) as i32),work_area(&monitor),if settings.pin.size_anchor=="auto" { &settings.pin.auto_anchor } else { &settings.pin.size_anchor });
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
                        if settings.pin.size_anchor == "fixed" { let size=client_size(w).to_logical::<f64>(w.scale_factor()); settings.pin.size=Some((size.width,size.height)); }
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
                    "notification-state" => {
                        let previous=settings.notify.clone();
                        let reason=if !notify::inbox_set(&mut settings.notify,&m) {Some("Notification update rejected: invalid event identities")}
                            else if !settings.save(&install) {settings.notify=previous;Some("Notification state was not saved; previous state retained")}
                            else {None};
                        reply!(&state(&install,&settings,dark,None,&adding,&mut sync));
                        reply!(&json!({"notificationReceipt":{"reason":reason}}));
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
                    "task-pin" => {
                        let st=state(&install,&settings,dark,None,&adding,&mut sync);
                        let receipt=match task_pin(&mut settings,&install,&st,&m){Ok(dashboard)=>json!({"dashboard":dashboard}),Err(reason)=>json!({"viewStateError":reason})};
                        reply!(&receipt);
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
                            let readback=saved&&crate::data::json(&install.join("tray.json")).ok().flatten().is_some_and(|v|v["navigation"]==settings.navigation);
                            let error=if !readback {Some("tray.json: save/readback failed; view state persistence unknown")}else{None};
                            if error.is_none() {if let (Some((_,wv)),Some(calls))=(&views_window,&mut views_web) && !calls.failed && !calls.pending.contains_key(&0) && m["navigation"]["dashboard"].is_object() {web_eval(wv,&format!("taskq.receive({})",json!({"dashboard":m["navigation"]["dashboard"]})),"tray.web.receive",&proxy,calls);}}
                            push(&mut web, &ui, &json!({"viewStateError":error}));
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
                if let (Some((_,wv)),Some(calls))=(&workspace,&mut workspace_web) {web_eval(wv,&format!("taskq.receive({})",workspace_snapshot(&state(&install,&settings,dark,None,&adding,&mut sync))),"tray.web.receive",&proxy,calls);}
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
                checkpoint_complete=false;
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
                if routes.iter().any(|r|r=="conversations-checkpoint") && !checkpoint_complete {capture_errors.push(format!("{name}: native checkpoint did not complete"));}
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
const ROUTES: [&str; 26] = ["home", "actions", "scroll", "answers", "project", "projects", "attention", "settings", "appearance", "cadence", "notifications", "notification-settings", "agents", "services", "about", "hosts", "delivery", "stop", "recovery", "agentq-shown", "agentq-setting-up", "agentq-running", "agentq-unknown", "views", "conversations", "opacity"];
const REPORT_SHOTS:[(&str,&str);15]=[
    ("conversations-checkpoint",concat!(include_str!("../tests/fixtures/conversations-checkpoint.js"),"taskqConversationsCheckpoint")),
    ("errors",concat!(include_str!("../tests/fixtures/errors.js"),"taskqErrorsShot.bind(null,'tray')")),
    ("errors-conversations",concat!(include_str!("../tests/fixtures/errors.js"),"taskqErrorsShot.bind(null,'conversations')")),
    ("health-list",concat!(include_str!("../tests/fixtures/execution-health.js"),"taskqExecutionHealthShot.bind(null,'list')")),
    ("health-detail",concat!(include_str!("../tests/fixtures/execution-health.js"),"taskqExecutionHealthShot.bind(null,'detail')")),
    ("health-recovered",concat!(include_str!("../tests/fixtures/execution-health.js"),"taskqExecutionHealthShot.bind(null,'recovered')")),
    ("health-conversation",concat!(include_str!("../tests/fixtures/execution-health.js"),"taskqExecutionHealthShot.bind(null,'conversation')")),
    ("notifications-home",concat!(include_str!("../tests/fixtures/notifications-shots.js"),"taskqNotificationsShot.bind(null,'home')")),
    ("notifications-panel",concat!(include_str!("../tests/fixtures/notifications-shots.js"),"taskqNotificationsShot.bind(null,'panel')")),
    ("notifications-expanded",concat!(include_str!("../tests/fixtures/notifications-shots.js"),"taskqNotificationsShot.bind(null,'expanded')")),
    ("notifications-result",concat!(include_str!("../tests/fixtures/notifications-shots.js"),"taskqNotificationsShot.bind(null,'result')")),
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
    // Capture the immutable source prefix before the accepted-receipt overlay appends an answer.
    for p in &mut ps {for t in p["tasks"].as_array_mut().into_iter().flatten() {t["history_source_revision"]=t["revision"].clone();t["history_source_count"]=json!(t["history"].as_array().map_or(0,Vec::len));}}
    sync.observe(&ps, Instant::now());
    sync.apply(&mut ps);
    for p in &mut ps {for t in p["tasks"].as_array_mut().into_iter().flatten() {history_header(t);}}
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
        "now": observed, "palette":icon::css(dark), "service":crate::host::service_status(install),"lifecycle":crate::lifecycle::status(install,None),"restart_review":crate::lifecycle::review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"stop_review":crate::lifecycle::stop_review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"full_stop_review":crate::lifecycle::full_stop_review(install,"review").unwrap_or_else(|reason|json!({"state":"unknown","reason":reason})),"stop_hold":crate::lifecycle::stop_hold_status(install),"stop_modes":cfg!(target_os="macos"),"projects": ps, "notify": notify_state(&s.notify, &ps), "agents": seen, "workspace_local": requests_file(install)["answers"], "conversations":crate::conversations::observed(install), "ai": {"request": crate::setup::last(install, true), "runtime":owner_runtime(&ps), "launch_choices":ps.iter().filter_map(|p|p["root"].as_str()).map(|root|(root.to_string(),json!(crate::runtime::setup_runtimes().map(|runtime|(runtime.to_string(),crate::runtime::choices::view(install,Path::new(root),runtime))).collect::<serde_json::Map<String,Value>>()))).collect::<serde_json::Map<String,Value>>(), "root":requests_file(install)["root"], "local":requests_file(install)["local"], "error":requests_file(install)["error"], "draft": requests_file(install)["draft"], "history": requests_file(install)["history"]}, "add": add, "setup": crate::setup::last(install, false), "history": {"start": start, "end": end, "tz": s.tz, "bins": sum.bins, "peak":icon::peak(&sum.bins), "svg":icon::svg(&icon::Data {paused:false,..icon_data(&sum)},dark), "since": sum.since},
        // § 4 Data: a corrupt tray.json names its reason while the defaults show
        "settings_error": crate::data::json(&install.join("tray.json")).err()});
    st["ui"]=crate::ui_bundle::status();
    project_task_pins(&mut st);
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
    json!({"status": notify::status(), "failure": notify::failure(), "inbox": n["inbox"], "host": host, "projects": projects})
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
    crate::platform::output("tray.agentq-screencapture",crate::platform::command("/usr/sbin/screencapture").args(["-x","-o","-l",&id.to_string()]).arg(path),Duration::from_secs(5)).map_err(|e|e.to_string())?;
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
    let mut c = crate::platform::command(&exe);
    c.args(["tray", "--open"]);
    #[cfg(target_os = "macos")]
    if let Some(app) = exe.ancestors().nth(3).filter(|_| exe.ends_with("TaskQ.app/Contents/MacOS/taskq")) {
        c = crate::platform::command("open");
        c.arg("-n").arg(app);
    }
    let err = fs::OpenOptions::new().create(true).append(true).open(install.join("tray.log")).map(std::process::Stdio::from).unwrap_or_else(|_| std::process::Stdio::null());
    c.current_dir(install).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(err);
    // its own process group, as the service starts it: a later service stop leaves this tray alone
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    c.spawn().map(drop)
}

fn workspace_operation(action:&str)->bool {
    matches!(action,"completed"|"agentq-setup"|"agentq-dismiss"|"agentq-check"|"capacity"|"restart"|"stop"|"uninstall"|"recovery-read"|"recovery-apply"|"pause"|"resume"|"focus"|"update"|"add"|"add-cancel"|"add-install"|"add-setup"|"resources"|"log"|"folder")
}
/// The page's own message for the tray path: `readerGeneration` is the native reader stamp, already checked against
/// the current reader; strict payloads (capacity, lifecycle) refuse it as an unknown field (#2167).
fn workspace_forward(m:&Value)->Option<String> {
    if !workspace_operation(m["a"].as_str()?) {return None;}
    let mut m=m.clone();m.as_object_mut()?.remove("readerGeneration");Some(m.to_string())
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

#[cfg(windows)]
fn native_header_drag(window:&Window)->bool {
    use tao::platform::windows::WindowExtWindows;
    #[link(name="user32")] unsafe extern "system" {fn GetCapture()->isize;}
    // Native caption moves capture the top-level window; a WebView control cannot count as a header drag.
    unsafe {GetCapture()==window.hwnd()}
}
#[cfg(target_os="macos")]
fn native_header_drag(window:&Window)->bool {
    use tao::platform::macos::WindowExtMacOS;
    let Some(mtm)=objc2::MainThreadMarker::new() else{return false;};
    let Some(event)=objc2_app_kit::NSApplication::sharedApplication(mtm).currentEvent() else{return false;};
    objc2_app_kit::NSEvent::pressedMouseButtons()&1!=0&&event.window(mtm).is_some_and(|w|std::ptr::from_ref(&*w).cast::<std::ffi::c_void>()==window.ns_window().cast_const())
        &&event.locationInWindow().y>f64::from(client_size(window).height)/window.scale_factor()
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
        let path=file.display().to_string().replace('\'',"''");let mut c=crate::platform::command("powershell");
        c.args(["-NoProfile","-Command",&format!("$args=@('powershell','-NoProfile','-ExecutionPolicy','Bypass','-File','\"{path}\"'); Start-Process -FilePath 'conhost.exe' -ArgumentList $args -ErrorAction Stop")]);c
    } else {
        #[cfg(unix)]
        fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).map_err(|e|e.to_string())?;
        let mut c=crate::platform::command("open");c.args(["-a", "Terminal"]).arg(&file);c
    };
    let output=crate::platform::output("tray.resources.submit",&mut command,Duration::from_secs(30))?;
    if !output.status.success() {return Err(String::from_utf8_lossy(&output.stderr).lines().find(|line|!line.trim().is_empty()).unwrap_or("resource terminal submission failed").chars().take(160).collect());}
    Ok(json!({"state":"submitted"}))
}

/// The OS folder picker; None on cancel.
fn pick() -> Result<Option<PathBuf>,String> {
    let mut c = if cfg!(windows) {
        let mut c = crate::platform::command("powershell");
        c.args(["-NoProfile", "-STA", "-Command", "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = 'Add project'; if ($d.ShowDialog() -eq 'OK') { [Console]::Out.Write($d.SelectedPath) }"]);
        c
    } else {
        let mut c = crate::platform::command("osascript");
        c.args(["-e", "activate", "-e", "POSIX path of (choose folder with prompt \"Add project\")"]);
        c
    };
    coreq::telemetry::capture::operation("tray.picker",|| {
    let o = c.output().map_err(|e|format!("folder picker did not start: {e}"))?;
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
    let o = crate::platform::complete("tray.selftest",crate::platform::command(me).arg("selftest").current_dir(dir).env("TASKQ_ROOT", dir).env("TASKQ_SELFTEST_TOOLS","off"))?;
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
        let mut c = crate::platform::command(me);
        c.args(["service", "install"]).current_dir(&dir).env("TASKQ_ROOT", &dir).stdin(std::process::Stdio::null());
        // a separate process group: launchctl must not take the tray with it
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        // #2239: `service install` waits up to its own handover deadlines; a tray deadline would cut it short
        let o = crate::platform::complete("tray.install", &mut c)?;
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
        let out=crate::platform::complete("tray.recovery",&mut command)?;
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
    let receipt=if !crate::resources::keys(&input,&["a","root","id","ops","issued_at","expires_at"]) {
        json!({"state":"rejected","reason":"Unsupported host capacity payload; refresh and review current settings"})
    } else {
        let request=json!({"id":input["id"],"ops":input["ops"],"issued_at":input["issued_at"],"expires_at":input["expires_at"]});
        match crate::host::capacity_request(&request) {
            Err(error)=>json!({"state":"rejected","reason":error}),
            Ok(_)=>{let policy=crate::host::Policy::read(install)?;crate::host::local_capacity(install,&policy.me,&request)?}
        }
    };
    match receipt["state"].as_str() {
        Some("rejected")=>tracing::info!(coreq_refusal="capacity-rejected"),
        Some("unknown")=>tracing::info!(coreq_error="capacity-unknown"),
        _=>{}
    }
    Ok(receipt)
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
        let mut c = crate::platform::command("reg");
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

#[cfg(test)]
#[path = "tray_tests.rs"]
pub(crate) mod tests;
