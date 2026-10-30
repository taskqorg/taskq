//! Tray notifications (taskq.md § 4, menu.md M14): new `ask`, `close`, `failed` and `gone` lines of each project's
//! `events` become native OS notifications. The core here is pure over the `notify` value of `<install>/tray.json`
//! (`kinds`: host modes, `projects`: per-project overrides, `cursor`: the last handled event line per project); the
//! macOS adapter posts through UNUserNotificationCenter, which needs the `<install>/TaskQ.app` bundle.

use serde_json::{Value, json};
use std::collections::HashMap;

pub const KINDS: [&str; 4] = ["ask", "close", "failed", "gone"];
/// Off, Silent (menu badge only), Banner, Banner with sound.
pub const MODES: [&str; 4] = ["off", "silent", "banner", "sound"];
/// More than `BURST` banners of one project within `WINDOW` seconds of board time become one summary (M14).
const BURST: usize = 3;
const WINDOW: i64 = 10;
/// Per project (tray memory, not `tray.json`): banners of the last `WINDOW` seconds, and the open summary (its
/// start, kinds and last event time), which grows while events stay under `WINDOW` apart.
#[derive(Default)]
pub struct Burst {
    recent: Vec<(i64, String)>,
    sum: Option<(i64, Vec<String>, i64)>,
}
pub type Bursts = HashMap<String, Burst>;

pub fn default(kind: &str) -> &'static str {
    match kind {
        "ask" => "sound",
        "failed" | "gone" => "banner",
        _ => "off",
    }
}

fn valid(m: &Value) -> Option<&'static str> {
    MODES.iter().find(|x| m.as_str() == Some(x)).copied()
}

/// The host mode of `kind`: an unknown value reads as the default (the file is kept).
pub fn host(n: &Value, kind: &str) -> &'static str {
    valid(&n["kinds"][kind]).unwrap_or(default(kind))
}

/// The effective mode of `kind` in `root`: its override, else the host mode.
pub fn mode(n: &Value, root: &str, kind: &str) -> &'static str {
    valid(&n["projects"][root][kind]).unwrap_or_else(|| host(n, kind))
}

/// Page action `{kind, value, root?}`: a host mode, or a project override (`inherit` removes it). False: refused.
pub fn set(n: &mut Value, m: &Value) -> bool {
    let Some(kind) = m["kind"].as_str().filter(|k| KINDS.contains(k)) else { return false };
    if !n.is_object() {
        *n = json!({});
    }
    match (m["root"].as_str(), valid(&m["value"])) {
        (Some(root), v) if v.is_some() || m["value"] == "inherit" => {
            if !n["projects"][root].is_object() {
                n["projects"][root] = json!({});
            }
            let p = n["projects"][root].as_object_mut().unwrap();
            match v {
                Some(v) => p.insert(kind.into(), json!(v)),
                None => p.remove(kind),
            };
        }
        (None, Some(v)) => n["kinds"][kind] = json!(v),
        _ => return false,
    }
    true
}

/// M14 local presentation metadata, never a task/decision operation. IDs bind to exact displayed events.
pub fn inbox_set(n: &mut Value, m: &Value) -> bool {
    let Some(action) = m["action"].as_str().filter(|a| ["read", "clear"].contains(a)) else { return false };
    let Some(ids) = m["ids"].as_array().filter(|ids| !ids.is_empty()) else { return false };
    if ids.iter().any(|id| id.as_str().is_none_or(|id| id.len()>4096 || serde_json::from_str::<Vec<Value>>(id).is_err())) { return false }
    if !n.is_object() { *n=json!({}); }
    if !n["inbox"].is_object() { n["inbox"]=json!({}); }
    for id in ids {
        let id=id.as_str().unwrap();
        if n["inbox"][id]!="cleared" { n["inbox"][id]=json!(if action=="clear" {"cleared"} else {"read"}); }
    }
    true
}

/// True when any kind of any project may post a banner: the first such setting asks the OS for permission.
pub fn wants_banner(n: &Value, roots: &[String]) -> bool {
    KINDS.iter().any(|k| ["banner", "sound"].contains(&host(n, k)) || roots.iter().any(|r| ["banner", "sound"].contains(&mode(n, r, k))))
}

/// One OS notification. `n` is None for a burst summary (a click opens the project). `id` carries what a click needs.
#[derive(Debug, PartialEq)]
pub struct Post {
    pub root: String,
    pub n: Option<u64>,
    pub title: String,
    pub body: String,
    pub sound: bool,
    /// Unique per event line: the OS replaces a notification with the same identifier.
    pub id: String,
}

pub fn label(kind: &str) -> &'static str {
    crate::display::NOTIFY.iter().find(|k| k.0 == kind).map_or("Event", |k| k.1)
}

/// Events of the tray's projects (`root`, `name`, `tasks`, `events` lines `[time, kind, attempt, n, outcome?]`) after each
/// project's cursor, as posts; the cursor moves past every handled line, Off and Silent included. A project seen for
/// the first time starts at `now`: older events never notify. The 4th banner within 10 s (sliding) opens one summary
/// under one identifier, which the OS replaces as it grows. Task titles come from `tasks`, else from `titles`: the
/// open tasks' titles of the previous tick, kept in `tray.json` so a closed task keeps its title across a restart.
/// Last comes the host's stale-grant notice, whatever the kind modes (#1803).
pub fn due(n: &mut Value, ps: &[Value], now: i64, bursts: &mut Bursts) -> Vec<Post> {
    if !n.is_object() {
        *n = json!({});
    }
    for k in ["cursor", "titles"] {
        if !n[k].is_object() {
            n[k] = json!({});
        }
    }
    let mut out = vec![];
    for p in ps {
        let Some(root) = p["root"].as_str() else { continue };
        let tasks: serde_json::Map<String, Value> = p["tasks"].as_array().into_iter().flatten()
            .filter_map(|t| Some((t["n"].as_u64()?.to_string(), json!(t["title"].as_str()?)))).collect();
        let known = std::mem::replace(&mut n["titles"][root], json!(tasks));
        let lines: Vec<&Value> = p["events"].as_array().into_iter().flatten().collect();
        let line = |e: &Value| format!("{} #{} {} {}", e[0], e[3], e[1].as_str().unwrap_or(""), e[2].as_str().unwrap_or(""));
        let Some(cursor) = n["cursor"][root].as_str().map(String::from) else {
            n["cursor"][root] = json!(now.to_string());
            continue;
        };
        // after the cursor line; when it is gone (trimmed), after its time
        let start = match lines.iter().rposition(|e| line(e) == cursor) {
            Some(i) => i + 1,
            None => {
                let t: i64 = cursor.split(' ').next().and_then(|t| t.parse().ok()).unwrap_or(i64::MAX);
                lines.iter().position(|e| e[0].as_i64().is_some_and(|x| x > t)).unwrap_or(lines.len())
            }
        };
        let new = &lines[start..];
        let Some(last) = new.last() else { continue };
        n["cursor"][root] = json!(line(last));
        let name = p["name"].as_str().unwrap_or(root);
        let b = bursts.entry(root.into()).or_default();
        let (mut singles, mut grown, mut shown, mut sums) = (vec![], false, vec![], vec![]);
        for e in new {
            let Some(kind) = e[1].as_str() else { continue };
            let m = mode(n, root, kind);
            if !["banner", "sound"].contains(&m) {
                continue;
            }
            let t = e[0].as_i64().unwrap_or(now);
            shown.push((t, m));
            match &mut b.sum {
                Some((_, kinds, last)) if t - *last < WINDOW => {
                    kinds.push(kind.into());
                    *last = t;
                }
                _ => {
                    // a quiet 10 s ends the summary: post its last state before this event starts afresh
                    if let (true, Some(s)) = (grown, b.sum.take()) {
                        sums.push(s);
                    }
                    grown = false;
                    b.recent.retain(|(x, _)| t - x < WINDOW);
                    b.recent.push((t, kind.into()));
                    if b.recent.len() <= BURST {
                        singles.push((*e, m, t));
                        continue;
                    }
                    let first = b.recent[0].0;
                    // this tick's singles in the window join the summary instead
                    singles.retain(|s| s.2 < first);
                    b.sum = Some((first, b.recent.drain(..).map(|r| r.1).collect(), t));
                }
            }
            grown = true;
        }
        if let (true, Some(s)) = (grown, &b.sum) {
            sums.push(s.clone());
        }
        for (e, m, _) in singles {
            let num = e[3].as_u64();
            let key = num.map(|x| x.to_string()).unwrap_or_default();
            let title = n["titles"][root][&key].as_str().or(known[&key].as_str()).map(String::from);
            // § 7: a close names its outcome, never a bare `Closed` (#1800)
            let head = match (num, e[4].as_str()) {
                (Some(x), Some(o)) => format!("#{x} closed: {o}"),
                (Some(x), None) => format!("#{x}"),
                _ => String::new(),
            };
            let body = match &title {
                Some(t) if e[4].is_string() => format!("{head} · {t}"),
                Some(t) => format!("{head} {t}"),
                None => head,
            };
            out.push(Post { root: root.into(), n: num, title: format!("{name} · {}", label(e[1].as_str().unwrap_or(""))), body, sound: m == "sound", id: json!({"root": root, "n": num, "k": e[1], "t": title, "e": line(e)}).to_string() });
        }
        for (first, kinds, last) in sums {
            let sound = shown.iter().any(|(t, m)| (first..=last).contains(t) && *m == "sound");
            let counts: Vec<String> = KINDS.iter().filter_map(|k| {
                let c = kinds.iter().filter(|x| x == k).count();
                (c > 0).then(|| format!("{c} {}", label(k).to_lowercase()))
            }).collect();
            out.push(Post { root: root.into(), n: None, title: format!("{name} · {} events", kinds.len()), body: counts.join(", "), sound, id: json!({"root": root, "sum": first}).to_string() });
        }
    }
    out.extend(stale_grant(n, ps));
    out.extend(grant_notices(n, ps));
    out
}

/// The owner step for a stale Screen Recording grant (#1803, worker-permissions.md § macOS signing identity).
pub const STALE_GRANT: &str = "System Settings > Privacy & Security > Screen Recording: select taskq, remove it (-), then allow taskq at the next prompt.";

/// One stale-grant notice (#1803 A2): the first status that reads `gui.stale_grant` posts it and records `stale_grant`
/// in `n`, so later ticks and restarts never repeat it; a status whose gui probe passes ends the episode.
pub fn stale_grant(n: &mut Value, ps: &[Value]) -> Option<Post> {
    if !n.is_object() {
        *n = json!({});
    }
    if ps.iter().any(|p| p["gui"]["listed"] == true) {
        n.as_object_mut().unwrap().remove("stale_grant");
        return None;
    }
    if n["stale_grant"] == true || !ps.iter().any(|p| p["gui"]["stale_grant"] == true) {
        return None;
    }
    n["stale_grant"] = json!(true);
    Some(Post { root: String::new(), n: None, title: "taskq · Screen Recording grant is stale".into(), body: STALE_GRANT.into(), sound: false, id: json!({"stale_grant": true}).to_string() })
}

/// The owner step for an Accessibility or App Data grant that reads `missing` or `stale` (#1845); `bin` is `<install>/taskq`.
pub fn grant_step(key: &str, state: &str, bin: &str) -> String {
    let pane = if key == "app_data" { "App Data" } else { "Accessibility" };
    let remove = if state == "stale" { "select taskq, remove it (-), then " } else { "" };
    format!("System Settings > Privacy & Security > {pane}: {remove}add {bin} with + and switch it on.")
}

/// One notice per Accessibility or App Data episode (#1845 A2), like `stale_grant`: the first status that reads the
/// grant `missing` or `stale` posts it and records `grants.<key>` in `n`; a pass where no status reads it so ends it.
pub fn grant_notices(n: &mut Value, ps: &[Value]) -> Vec<Post> {
    if !n.is_object() {
        *n = json!({});
    }
    let mut out = vec![];
    for (key, _) in crate::platform::GRANTS {
        let bad = ps.iter().find(|p| matches!(p["gui"]["grants"][key].as_str(), Some("missing" | "stale")));
        let Some(p) = bad else {
            if let Some(g) = n["grants"].as_object_mut() { g.remove(key); }
            continue;
        };
        if n["grants"][key] == true {
            continue;
        }
        if !n["grants"].is_object() { n["grants"] = json!({}); }
        n["grants"][key] = json!(true);
        let state = p["gui"]["grants"][key].as_str().unwrap_or_default();
        let pane = if key == "app_data" { "App Data" } else { "Accessibility" };
        let body = grant_step(key, state, p["gui"]["binary"].as_str().unwrap_or("taskq"));
        out.push(Post { root: String::new(), n: None, title: format!("taskq · {pane} grant is {state}"), body, sound: false, id: json!({"grant": key}).to_string() });
    }
    out
}

/// `Send test`: one sample of `kind` (its host mode decides the sound).
pub fn sample(n: &Value, kind: &str, now: i64) -> Post {
    Post { root: String::new(), n: None, title: format!("taskq · {}", label(kind)), body: "#1 Sample task".into(), sound: host(n, kind) == "sound", id: json!({"test": kind, "at": now}).to_string() }
}

/// OS permission as the Settings page shows it: `allowed`, `blocked`, `not asked`, `asking` (the OS prompt is open),
/// `request failed`, `unknown` or `unsupported`; `failure` is the error of a failed request (macOS), also written to `tray.log`.
pub use os::{failure, open_settings, post, request, status, watch};

#[cfg(target_os = "macos")]
mod os {
    use super::Post;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AnyThread, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::*;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::{Mutex, OnceLock};

    /// -1 unknown, else UNAuthorizationStatus; refreshed asynchronously.
    static STATUS: AtomicI64 = AtomicI64::new(-1);
    static CLICK: OnceLock<Mutex<Box<dyn Fn(String) + Send>>> = OnceLock::new();
    static DELEGATE: OnceLock<usize> = OnceLock::new();
    /// The error of a failed authorization request, kept for the process: the page shows it instead of `not asked`.
    static FAILED: Mutex<Option<String>> = Mutex::new(None);
    /// A request waits for the owner's answer; meanwhile the OS reports denied.
    static ASKING: AtomicBool = AtomicBool::new(false);

    /// UNUserNotificationCenter raises outside an app bundle: only `<install>/TaskQ.app` has one.
    fn center() -> Option<Retained<UNUserNotificationCenter>> {
        NSBundle::mainBundle().bundleIdentifier()?;
        Some(UNUserNotificationCenter::currentNotificationCenter())
    }

    fn refresh() {
        let Some(c) = center() else { return };
        let block = RcBlock::new(|s: NonNull<UNNotificationSettings>| {
            STATUS.store(unsafe { s.as_ref() }.authorizationStatus().0 as i64, Ordering::Relaxed);
        });
        c.getNotificationSettingsWithCompletionHandler(&block);
    }

    pub fn status() -> &'static str {
        if center().is_none() {
            return "unsupported";
        }
        refresh();
        if ASKING.load(Ordering::Relaxed) {
            return "asking";
        }
        match STATUS.load(Ordering::Relaxed) {
            0 | 1 if FAILED.lock().is_ok_and(|f| f.is_some()) => "request failed",
            0 => "not asked",
            1 => "blocked",
            2..=4 => "allowed",
            _ => "unknown",
        }
    }

    /// Asks the OS once; a later call while denied changes nothing (the OS asks only once).
    pub fn request() {
        let Some(c) = center() else { return };
        if STATUS.load(Ordering::Relaxed) != 0 {
            return;
        }
        ASKING.store(true, Ordering::Relaxed);
        let block = RcBlock::new(|_: objc2::runtime::Bool, e: *mut NSError| {
            ASKING.store(false, Ordering::Relaxed);
            // SAFETY: the OS passes null or a valid NSError for the duration of the call
            if let Some(e) = unsafe { e.as_ref() } {
                let m = format!("{} ({} {})", e.localizedDescription(), e.domain(), e.code());
                eprintln!("taskq tray: notification authorization request failed: {m}");
                if let Ok(mut f) = FAILED.lock() {
                    *f = Some(m);
                }
            }
            refresh();
        });
        c.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &block);
    }

    /// Posts only when allowed: blocked or unknown posts nothing and falls back to no other channel.
    pub fn post(p: &Post) -> bool {
        let Some(c) = center() else { return false };
        if !(2..=4).contains(&STATUS.load(Ordering::Relaxed)) {
            return false;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&p.title));
        content.setBody(&NSString::from_str(&p.body));
        content.setThreadIdentifier(&NSString::from_str(&p.root));
        if p.sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let req = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&p.id), &content, None);
        c.addNotificationRequest_withCompletionHandler(&req, None);
        true
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; the delegate has no Drop.
        #[unsafe(super(NSObject))]
        #[name = "TaskqNotifyDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // the tray is an accessory app: show banners while it is frontmost too
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(&self, _c: &UNUserNotificationCenter, n: &UNNotification, done: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
                let sound = n.request().content().sound().is_some();
                let o = UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List;
                done.call((if sound { o | UNNotificationPresentationOptions::Sound } else { o },));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(&self, _c: &UNUserNotificationCenter, r: &UNNotificationResponse, done: &block2::DynBlock<dyn Fn()>) {
                if let Some(f) = CLICK.get().and_then(|f| f.lock().ok()) {
                    f(r.notification().request().identifier().to_string());
                }
                done.call(());
            }
        }
    );

    /// Sets the click handler (it gets the request identifier) and reads the OS status once.
    pub fn watch(f: impl Fn(String) + Send + 'static) {
        let Some(c) = center() else { return };
        if CLICK.set(Mutex::new(Box::new(f))).is_ok() {
            let d: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
            c.setDelegate(Some(ProtocolObject::from_ref(&*d)));
            // the center holds its delegate weakly: keep it for the process
            let _ = DELEGATE.set(Retained::into_raw(d) as usize);
        }
        refresh();
    }

    pub fn failure() -> Option<String> {
        FAILED.lock().ok()?.clone()
    }

    pub fn open_settings() {
        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension").spawn();
    }
}

#[cfg(windows)]
#[path = "notify_windows.rs"]
mod os;

#[cfg(not(any(target_os = "macos", windows)))]
mod os {
    pub fn status() -> &'static str {
        "unsupported"
    }
    pub fn request() {}
    pub fn failure() -> Option<String> {
        None
    }
    pub fn post(_: &super::Post) -> bool {
        false
    }
    pub fn watch(_: impl Fn(String) + Send + 'static) {}
    pub fn open_settings() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    // M14: read/clear survives restart, never alters OS cursors or downgrades a cleared event; reject invalid IPC atomically.
    #[test]
    fn inbox_markers_are_exact_idempotent_and_persisted() {
        let id=json!(["https://example.org/issues","event",7,[100,"ask","9"]]).to_string();
        let mut n=json!({"cursor":{"/p":"prior"},"kinds":{"ask":"sound"}});
        assert!(inbox_set(&mut n,&json!({"action":"read","ids":[id]})));
        assert_eq!(n["inbox"][&id],"read");
        let mut saved:Value=serde_json::from_str(&n.to_string()).unwrap();
        assert!(inbox_set(&mut saved,&json!({"action":"clear","ids":[id]})));
        assert!(inbox_set(&mut saved,&json!({"action":"read","ids":[id]})));
        assert_eq!(saved["inbox"][&id],"cleared");
        let before=saved.clone();
        assert!(!inbox_set(&mut saved,&json!({"action":"read","ids":[id,"invalid"]})));
        assert_eq!(saved,before,"invalid batch changes nothing");
        assert_eq!(saved["cursor"],n["cursor"]);
        assert_eq!(saved["kinds"],n["kinds"]);
    }

    fn proj(events: Value) -> Value {
        json!({"root": "/p", "name": "p", "tasks": [{"n": 7, "title": "Fix it"}], "events": events})
    }

    // Defect #1845 A2: only Screen Recording had an owner notice; a stale Accessibility grant became a worker ask.
    // Risk: the notice repeats each tick or restart, or never returns for a new episode.
    #[test]
    fn grant_notices_once_per_episode() {
        let bad = |k: &str, s: &str| [json!({"gui": {"grants": {k: s}, "binary": "/i/taskq"}})];
        let mut n = json!({});
        assert!(grant_notices(&mut n, &bad("accessibility", "allowed")).is_empty());
        let post = grant_notices(&mut n, &bad("accessibility", "stale"));
        assert_eq!(post.len(), 1, "first stale read posts");
        assert_eq!(post[0].body, "System Settings > Privacy & Security > Accessibility: select taskq, remove it (-), then add /i/taskq with + and switch it on.");
        assert!(grant_notices(&mut n, &bad("accessibility", "missing")).is_empty(), "repeated within the episode");
        let saved = n.to_string();
        assert!(grant_notices(&mut serde_json::from_str(&saved).unwrap(), &bad("accessibility", "stale")).is_empty(), "repeated after a restart");
        assert!(grant_notices(&mut n, &bad("accessibility", "allowed")).is_empty());
        assert_eq!(grant_notices(&mut n, &bad("accessibility", "missing"))[0].body, "System Settings > Privacy & Security > Accessibility: add /i/taskq with + and switch it on.", "a new episode notifies again");
        let app = grant_notices(&mut n, &bad("app_data", "stale"));
        assert_eq!((app.len(), app[0].title.as_str()), (1, "taskq · App Data grant is stale"));
        assert!(grant_notices(&mut n, &bad("app_data", "unknown")).is_empty() && n["grants"].get("app_data").is_none(), "unknown ends the App Data episode");
    }

    // Defect #1803 A2: a cdhash-pinned grant re-prompted forever with no owner step. Risk: the notice repeats each
    // tick or restart, or a fixed and later re-broken grant never notifies again.
    #[test]
    fn stale_grant_notifies_once_per_episode() {
        let stale = [json!({"gui": {"listed": false, "stale_grant": true}}), json!({"gui": {"listed": false, "stale_grant": false}})];
        let mut n = json!({});
        assert_eq!(stale_grant(&mut n, &[json!({"gui": {"listed": false, "stale_grant": false}})]), None);
        let post = stale_grant(&mut n, &stale).expect("first stale read posts");
        assert_eq!(post.body, STALE_GRANT);
        assert_eq!(stale_grant(&mut n, &stale), None, "repeated on the next tick");
        let saved = serde_json::to_string(&n).unwrap();
        assert_eq!(stale_grant(&mut serde_json::from_str(&saved).unwrap(), &stale), None, "repeated after a restart");
        assert_eq!(stale_grant(&mut n, &[json!({"gui": {"listed": true, "stale_grant": false}})]), None);
        assert!(stale_grant(&mut n, &stale).is_some(), "a new episode after a passing probe notifies again");
        let mut t = json!({});
        let p = json!({"root": "/p", "name": "p", "tasks": [], "events": [], "gui": {"listed": false, "stale_grant": true}});
        assert_eq!(due(&mut t, &[p.clone()], 100, &mut Bursts::new()).len(), 1, "the tray tick posts it");
        assert!(due(&mut t, &[p], 200, &mut Bursts::new()).is_empty());
    }

    // Chain: M14 modes. Risk: a bad value or a missing override silences or floods a kind.
    #[test]
    fn modes_default_override_and_unknown() {
        let mut n = json!({"kinds": {"close": "loud", "failed": "silent"}});
        assert_eq!([host(&n, "ask"), host(&n, "close"), host(&n, "failed"), host(&n, "gone")], ["sound", "off", "silent", "banner"], "unknown reads as default");
        assert!(set(&mut n, &json!({"kind": "close", "value": "banner", "root": "/p"})));
        assert_eq!((mode(&n, "/p", "close"), mode(&n, "/q", "close")), ("banner", "off"));
        assert!(set(&mut n, &json!({"kind": "close", "value": "inherit", "root": "/p"})));
        assert_eq!(mode(&n, "/p", "close"), "off");
        assert!(!set(&mut n, &json!({"kind": "close", "value": "inherit"})), "host has no inherit");
        assert!(!set(&mut n, &json!({"kind": "blocked", "value": "banner"})));
        assert!(set(&mut n, &json!({"kind": "ask", "value": "off"})));
        assert!(!wants_banner(&json!({"kinds": {"ask": "off", "failed": "silent", "gone": "off"}}), &["/p".into()]));
        assert!(wants_banner(&json!({"kinds": {"ask": "off", "failed": "silent", "gone": "off"}, "projects": {"/p": {"close": "banner"}}}), &["/p".into()]));
    }

    // Chain: M14 once per event across restarts. Risk: a restart repeats or floods old events.
    #[test]
    fn cursor_once_per_event_across_restart() {
        let old = json!([[100, "ask", "5", 7]]);
        let mut n = json!(null);
        assert!(due(&mut n, &[proj(old.clone())], 150, &mut Bursts::new()).is_empty(), "first start: history never notifies");
        let ev = json!([[100, "ask", "5", 7], [200, "failed", "6", 7], [200, "close", "6", 8]]);
        let p = due(&mut n, &[proj(ev.clone())], 210, &mut Bursts::new());
        assert_eq!(p.len(), 1, "close is off by default");
        assert_eq!((p[0].title.as_str(), p[0].body.as_str(), p[0].n, p[0].sound), ("p · Failed", "#7 Fix it", Some(7), false));
        // a restart reloads the saved value: nothing again
        let mut saved: Value = serde_json::from_str(&n.to_string()).unwrap();
        assert!(due(&mut saved, &[proj(ev)], 220, &mut Bursts::new()).is_empty());
        // the cursor line trimmed away: later times only
        let ev = json!([[200, "close", "6", 8], [300, "ask", "9", 7]]);
        let p = due(&mut saved, &[proj(ev)], 310, &mut Bursts::new());
        assert_eq!((p.len(), p[0].sound), (1, true));
        // silent and off still move the cursor
        set(&mut saved, &json!({"kind": "ask", "value": "silent"}));
        assert!(due(&mut saved, &[proj(json!([[300, "ask", "9", 7], [400, "ask", "10", 7]]))], 410, &mut Bursts::new()).is_empty());
        set(&mut saved, &json!({"kind": "ask", "value": "banner"}));
        assert!(due(&mut saved, &[proj(json!([[300, "ask", "9", 7], [400, "ask", "10", 7]]))], 420, &mut Bursts::new()).is_empty());
    }

    // Chain: M14 burst. Risk: a busy pass floods the screen.
    #[test]
    fn burst_becomes_one_summary() {
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev = json!([[10, "failed", "1", 1], [10, "failed", "2", 2], [10, "ask", "3", 3], [10, "gone", "4", 4], [10, "close", "5", 5]]);
        let p = due(&mut n, &[proj(ev)], 20, &mut Bursts::new());
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].title.as_str(), p[0].body.as_str(), p[0].n, p[0].sound), ("p · 4 events", "1 question, 2 failed, 1 attempt gone", None, true));
        let mut n = json!({"cursor": {"/p": "0"}});
        assert_eq!(due(&mut n, &[proj(json!([[10, "failed", "1", 1], [10, "failed", "2", 2], [10, "ask", "3", 3]]))], 20, &mut Bursts::new()).len(), 3, "three stay single");
        // spread over ticks: the 4th banner within 10 s turns the window into one summary, replaced as it grows
        let (mut n, mut b) = (json!({"cursor": {"/p": "0"}}), Bursts::new());
        let mut ev = vec![];
        let mut tick = |t: i64, e: Value| { ev.push(e); due(&mut n, &[proj(json!(ev))], t, &mut b) };
        for t in [100, 102, 104] {
            assert_eq!(tick(t, json!([t, "failed", t.to_string(), 7]))[0].n, Some(7));
        }
        let s1 = tick(106, json!([106, "gone", "106", 7]));
        assert_eq!((s1.len(), s1[0].title.as_str(), s1[0].body.as_str()), (1, "p · 4 events", "3 failed, 1 attempt gone"));
        let s2 = tick(109, json!([109, "ask", "109", 7]));
        assert_eq!((s2[0].title.as_str(), &s2[0].id), ("p · 5 events", &s1[0].id), "same identifier: the OS replaces it");
        let p = tick(118, json!([118, "failed", "118", 7]));
        assert_eq!((p[0].title.as_str(), &p[0].id), ("p · 6 events", &s1[0].id), "under 10 s apart: the summary grows");
        let p = tick(130, json!([130, "failed", "130", 7]));
        assert_eq!((p.len(), p[0].n), (1, Some(7)), "a quiet 10 s ends it");
        // sliding: 108..111 is a burst although 100 started an earlier window
        let (mut n, mut b) = (json!({"cursor": {"/p": "0"}}), Bursts::new());
        let mut ev = vec![];
        let mut tick = |t: i64| { ev.push(json!([t, "failed", t.to_string(), 7])); due(&mut n, &[proj(json!(ev))], t, &mut b) };
        let singles: Vec<usize> = [100, 108, 109, 110].into_iter().map(|t| tick(t).len()).collect();
        assert_eq!(singles, [1, 1, 1, 1]);
        let s = tick(111);
        assert_eq!((s.len(), s[0].title.as_str(), s[0].n), (1, "p · 4 events", None), "108, 109, 110, 111");
        // one tick: an early single, then a burst
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev = json!([[50, "failed", "1", 1], [70, "failed", "2", 2], [71, "failed", "3", 3], [72, "ask", "4", 4], [73, "gone", "5", 5]]);
        let p = due(&mut n, &[proj(ev)], 80, &mut Bursts::new());
        assert_eq!(p.iter().map(|p| (p.n, p.title.as_str())).collect::<Vec<_>>(), [(Some(1), "p · Failed"), (None, "p · 4 events")]);
        // one tick: a burst, then a single after a quiet 10 s; and two separated bursts
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev = json!([[100, "failed", "1", 1], [101, "failed", "2", 2], [102, "ask", "3", 3], [103, "gone", "4", 4], [120, "failed", "5", 5]]);
        let p = due(&mut n, &[proj(ev)], 130, &mut Bursts::new());
        assert_eq!(p.iter().map(|p| (p.n, p.title.as_str())).collect::<Vec<_>>(), [(Some(5), "p · Failed"), (None, "p · 4 events")]);
        let mut n = json!({"cursor": {"/p": "0"}});
        let ev: Vec<Value> = [100, 101, 102, 103, 104, 200, 201, 202, 203].iter().map(|t| json!([t, "failed", t.to_string(), 7])).collect();
        let p = due(&mut n, &[proj(json!(ev))], 210, &mut Bursts::new());
        let ids: Vec<Value> = p.iter().map(|p| serde_json::from_str::<Value>(&p.id).unwrap()["sum"].clone()).collect();
        assert_eq!((p.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ids), (vec!["p · 5 events", "p · 4 events"], vec![json!(100), json!(200)]));
    }

    // Chain: M14 a closed task's banner and card name it. Risk: `status.json` drops closed tasks, so the title is lost,
    // also when the tray restarts between the last open sighting and the close.
    #[test]
    fn closed_task_keeps_its_title() {
        let mut n = json!({"kinds": {"close": "banner"}});
        due(&mut n, &[proj(json!([]))], 100, &mut Bursts::new());
        let mut saved: Value = serde_json::from_str(&n.to_string()).unwrap();
        let gone = json!({"root": "/p", "name": "p", "tasks": [], "events": [[200, "close", "6", 7], [201, "close", "8", 90, "replaced by #104 (ready)"]]});
        let p = due(&mut saved, &[gone], 210, &mut Bursts::new());
        assert_eq!(p[0].body, "#7 Fix it");
        // #1800 (pilotq#90): a replaced close names its successor's state instead of a bare `Closed`
        assert_eq!(p[1].body, "#90 closed: replaced by #104 (ready)");
        assert_eq!(serde_json::from_str::<Value>(&p[0].id).unwrap()["t"], "Fix it", "the click opens a card with its title");
        assert_eq!(saved["titles"]["/p"], json!({}), "only open tasks stay: bounded by the open tasks");
    }
}
