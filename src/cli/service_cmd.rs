//! Service lifecycle commands: install, start, stop, restart, focus and the tray (§ 4, § 5).

use crate::time::age;
use crate::{check, data, display, external_slots, host, host_control, incident, lifecycle, platform, recovery, reports, requests, resources, runtime, service, setup, snapshot, tools};
#[cfg(any(target_os = "macos", windows))]
use crate::tray;
use crate::Res;
use crate::cli::Args;
use crate::cli::host_cmd::{capacity_cmd, channel_cmd, claim_name, dev_build_cmd, dev_line, hold_cmd, host_setup, onboard_cmd, permissions_cmd, pm_cmd, recover_cmd, resources_cmd, setup_check_cmd};
use crate::config::{check_load, ctx};
use crate::paths::{dot, find_root, needs_checkout, project_name};
use crate::wake::{change_focus, wake_lock};
use serde_json::{Value, json};
use std::fs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// #1801: only the OS default install's bundle is `org.taskq.tray`; temp, test and evidence installs take their own id,
/// so their iconless copies never win the notification icon in LaunchServices.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn tray_id(home: bool) -> &'static str {
    if home { "org.taskq.tray" } else { "org.taskq.tray.test" }
}

/// macOS notifications need a bundle identity (§ 4): `<install>/TaskQ.app`, signed as § 8 step 5 (else ad-hoc),
/// holding the current release; the default install's bundle is `~/Applications/TaskQ.app`, which Finder, Spotlight
/// and the Dock open (M15). None: the tray runs unbundled and notifications read unsupported.
#[cfg(target_os = "macos")]
pub(crate) fn tray_app(install: &Path, exe: &Path) -> Option<PathBuf> {
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
    let home = crate::platform::user_home().filter(|_| platform::is_home(install, platform::home_install().as_deref()));
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
pub(crate) fn bundled_tray(exe: &Path, install:&Path) -> bool {
    use std::os::fd::AsRawFd;
    if !exe.ends_with("TaskQ.app/Contents/MacOS/taskq") {return false;}
    if let Ok(f) = fs::OpenOptions::new().create(true).append(true).open(install.join("tray.log")) {
        // SAFETY: both descriptors are open; fd 2 now refers to the log for the life of the process
        unsafe { libc::dup2(f.as_raw_fd(), 2) };
    }
    true
}

/// Labels of the open tasks in the last `status.json`, for a resume that has no focus to restore (§ 4).
pub(crate) fn status_labels(root: &Path) -> Vec<String> {
    let st: Value = fs::read_to_string(dot(root).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut l: Vec<String> = st["tasks"].as_array().into_iter().flatten().flat_map(|t| t["labels"].as_array().cloned().unwrap_or_default())
        .filter_map(|x| x.as_str().map(String::from)).filter(|x| !x.starts_with("q-") && x != service::TASK_LABEL).collect();
    l.sort();
    l.dedup();
    l
}

/// Focus (§ 4): `f` admits; `request` keeps the owner's words, none removes them.
#[tracing::instrument(name="set_focus",skip_all)]
pub(crate) fn set_focus(root: &Path, f: &str, request: Option<&str>) -> Res<String> {
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
pub(crate) fn install_focus(root: &Path) -> Res<bool> {
    let _signal = wake_lock(root)?;
    let d = dot(root);
    if !d.join("focus").exists() {
        platform::write_atomic(&d.join("focus"), "off")?;
    }
    Ok(data::focus(root).is_ok_and(|f| f.is_empty()))
}

/// Pause (§ 4): keep the focus in `focus.paused`, admit nothing.
#[tracing::instrument(name="pause",skip_all)]
pub(crate) fn pause(root: &Path, initialize: bool) -> Res<String> {
    change_focus(root, true, initialize, |d| {
        let f = data::focus(root)?.join(" ");
        if !f.is_empty() { platform::write_atomic(&d.join("focus.paused"), &f)?; }
        platform::write_atomic(&d.join("focus"), "off")
    }).map(|(_,receipt)|receipt)
}

/// Resume (§ 4): restore the saved focus; never `all` by default.
#[tracing::instrument(name="resume",skip_all)]
pub(crate) fn resume(root: &Path) -> Res<(String,String)> {
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
pub(crate) fn service_id(installation: &str) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, installation.as_bytes());
    d.as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// The status line of a pending re-exec; #2254: past its 600 s bound it reads as a host incident.
pub(crate) fn restart_line(status: &Value) -> Option<String> {
    let w = status["restart_waits"].as_array().filter(|w| !w.is_empty())?;
    let overdue = if status["restart_overdue"] == true { "host incident: pending over 600 s; " } else { "" };
    Some(format!("Restart: {overdue}waits for {}", w.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")))
}

pub(crate) fn service_fields(root: &Path) -> String {
    service_project_fields(root,data::json(&dot(root).join("status.json")))
}

pub(crate) fn service_project_fields(root:&Path,status:Res<Option<Value>>)->String {
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
            let default=overlay.and_then(|e|e["default_runtime"].as_str()).unwrap_or(p.default_runtime.name());
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
pub(crate) fn service_project(root:&Path,binding:&str,read:&Res<Option<Value>>)->Value {
    let mut status=read.as_ref().ok().and_then(Option::as_ref).cloned();
    if let Some(s)=status.as_mut().filter(|s|s["reports"]["reports"].is_array()) {s["reports"]=reports::summary(&s["reports"]);}
    json!({"root":root,"host_binding":binding,"status":status,"error":read.as_ref().err()})
}
pub(crate) fn lifecycle_scope(value:&Value)->String {
    let mut lines=if let Some(root)=value["project"].as_str() {
        vec![format!("Scope: project uninstall; {root}"),"Reviewed registry (other projects stay registered):".into()]
    } else {
        vec![if value["action"]=="uninstall" {"Scope: host uninstall; all projects drain; registry kept"}else{"Scope: host service; all registered projects"}.into()]
    };
    for root in value["projects"].as_array().into_iter().flatten() {lines.push(format!("Registered project: {}",root.as_str().unwrap_or("unknown")));}
    lines.join("\n")
}
pub(crate) fn service_report(a:&Args)->Res<()> {
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
pub(crate) fn service(args: &Args) -> Res<()> {
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
    if matches!(sub,"restart"|"stop"|"uninstall") {return lifecycle_request(args,sub);}
    if sub == "run" {return run_host();}
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

/// `service restart|stop|uninstall`: reviews one request, applies it through the service and reports its receipt.
fn lifecycle_request(args: &Args, sub: &str) -> Res<()> {
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

/// `service run`: runs the host service for the default install, from there or from a registered root.
fn run_host() -> Res<()> {
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

#[cfg(any(target_os = "macos", windows))]
pub(crate) fn tray(install: PathBuf, snapshot: Option<&str>, open: bool, routes: &[String], icon: Option<&str>) -> Res<()> {
    if icon.is_some() { return Err("Custom icons were removed; choose Icon or Summary in Tray icon settings".into()); }
    if snapshot.is_none() && !routes.is_empty() {
        return Err("--route needs --snapshot DIR".into());
    }
    tray::run(install, snapshot.map(PathBuf::from), routes.to_vec(), open)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn tray(_install: PathBuf, _snapshot: Option<&str>, _open: bool, _routes: &[String], icon: Option<&str>) -> Res<()> {
    if icon.is_some() { return Err("Custom icons were removed; choose Icon or Summary in Tray icon settings".into()); }
    Err("taskq tray: macOS and Windows only".into())
}
