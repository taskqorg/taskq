//! Host commands: capacity, resources, permissions, channel, doctor, setup and onboarding (§ 3, § 5, § 6).

use crate::{admission, balance, board, check, data, dispatch, doctor, health, host, labels, lifecycle, platform, pm, policy, resources, runtime, service, setup, snapshot, tools};
use crate::runtime::accounts;
use crate::{Res, emit};
use crate::cli::Args;
use crate::cli::text::one_line;
use crate::config::{TTL, check_load, ctx, hostname, load};
use crate::paths::{dot, find_root, project_name, touch};
use crate::service::{Config, RUNTIMES};
use crate::wake::poke;
use serde_json::{Value, json};
use std::fs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// One owner edit of a host's capacity: runtime limits and load gate fields; `None` keeps a field.
#[derive(Default, Clone)]
pub(crate) struct CapEdit {
    /// `admission.mode` auto (true) or fixed (false), § 9
    pub(crate) mode: Option<bool>,
    /// `admission.max`: `Some(None)` removes it
    pub(crate) max: Option<Option<u32>>,
    pub(crate) limits: BTreeMap<String, u32>,
    /// runtimes whose host limit goes (`runtime_limit: null`)
    pub(crate) unset: std::collections::BTreeSet<String>,
    pub(crate) gate: Option<bool>,
    pub(crate) cpu_max: Option<u32>,
    pub(crate) memory_min: Option<u32>,
    /// host.json task-claim mode, § 9
    pub(crate) claim: Option<host::ClaimMode>,
    /// host.json `default_runtime`, § 9
    pub(crate) default_runtime: Option<String>,
    pub(crate) weights: Option<balance::Weights>,
}

impl CapEdit {
    pub(crate) fn from_args(a: &Args) -> Res<CapEdit> {
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

    pub(crate) fn is_empty(&self) -> bool {
        self.mode.is_none() && self.max.is_none() && self.limits.is_empty() && self.unset.is_empty() && self.gate.is_none() && self.cpu_max.is_none() && self.memory_min.is_none() && self.claim.is_none()
            && self.default_runtime.is_none() && self.weights.is_none()
    }
}

pub(crate) fn cap_changes(old: &service::Capacity, new: &service::Capacity) -> String {
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
pub(crate) fn add_host(given: Option<&str>, cfg: &Config) -> Option<String> {
    given.map(String::from).or_else(|| (cfg.claim == host::ClaimMode::Labelled).then(|| cfg.me.clone()))
}

/// § 9: a labelled host keeps its own host on every board; a board this host does not serve refuses the add,
/// and the refusal names the fix (#1912).
pub(crate) fn serve_hint(e: String, cfg: &Config) -> String {
    if e.starts_with(&format!("--host {0}: no host named {0};", cfg.me)) {
        format!("{e}\nNext: run taskq service install for this project on host {} so it serves this board", cfg.me)
    } else { e }
}

/// host.json `claim` value (§ 9).
pub(crate) fn claim_name(mode: host::ClaimMode) -> &'static str { mode.name() }

pub(crate) fn runtime_name(rt: &str) -> String {
    let mut c = rt.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The gate line of a `status.json` capacity (§ 4 tray text).
pub(crate) fn gate_text(c: &Value) -> String {
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
pub(crate) fn admission_text(c: &Value) -> String {
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
pub(crate) fn permissions_cmd(root: &Path, a: &Args) -> Res<()> {
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
pub(crate) fn resources_cmd(root: &Path, a: &Args) -> Res<()> {
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

pub(crate) fn pm_cmd(root:&Path,a:&Args)->Res<()> {
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
pub(crate) fn balance_cli(a:&Args) -> Res<()> {
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
pub(crate) fn capacity_cmd(a: &Args) -> Res<()> {
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
pub(crate) fn dev_line(dev: &Value) -> String {
    let sha = |k: &str| dev[k].as_str().map(|s| s.chars().take(7).collect::<String>());
    let mut line = format!("Dev: installed {}", sha("installed").unwrap_or("unknown".into()));
    if let Some(p) = sha("pending") { line += &format!(" · pending {p}{}", if dev["building"] == true { " (building)" } else { "" }); }
    if let Some(f) = sha("failed") { line += &format!(" · failed {f}"); }
    if dev["requested"] == true { line += " · requested"; }
    line
}

/// § 8 item 1 (#2311): request one dev build; the service starts the newest green main SHA on its next pass.
pub(crate) fn dev_build_cmd(a: &Args, install: &Path) -> Res<()> {
    if a.pos.len() != 2 || !a.flags.is_empty() { return Err("usage: taskq service dev-build".into()); }
    let channel = platform::channel(install)?;
    if channel != "dev" { return Err(format!("channel {channel}: dev builds run only on channel dev (taskq service channel dev)")); }
    let request = platform::dev_request(install);
    fs::create_dir_all(request.parent().unwrap_or(install)).map_err(|e| e.to_string())?;
    coreq::write_durable(&request, "requested")?;
    println!("Dev build requested; the host service starts the newest green main SHA on its next pass");
    Ok(())
}

pub(crate) fn channel_cmd(a: &Args) -> Res<()> {
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

/// `taskq doctor [--dry-run] [--hosts] [--json]` ([host doctor](docs/design/host-service.md#host-doctor)): without
/// `--hosts`, one cleanup pass now (#2060), the host-setup gap (#2078) and the health checks of every registered board
/// (#2194); with `--hosts`, per board one table of every host's published health and one of the running workers, read
/// from the board alone. `--json` prints the same rows.
pub(crate) fn doctor_cmd(a: &Args) -> Res<()> {
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
pub(crate) fn host_setup(cfg: &Config) -> Option<String> {
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
pub(crate) fn setup_items(cfg: &Config, limits: &BTreeMap<String, u32>, delivery: Result<String, String>, agents: &Value, access: &[policy::Effective],
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
pub(crate) fn setup_check_cmd(root: &Path, json_out: bool) -> Res<()> {
    let (cfg, raw, delivery) = check_load(root)?;
    let limits = host::Policy::read(&cfg.install).and_then(|p| p.project(&cfg.root, &raw)).map_or_else(|_| cfg.limits.clone(), |c| c.limits);
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
pub(crate) fn onboard_cmd(install: &Path) -> Res<()> {
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
pub(crate) fn hold_cmd(on: bool, reason: Option<&str>) -> Res<()> {
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
pub(crate) fn recover_cmd(root: &Path, apply: bool, json: bool) -> Res<()> {
    let c = ctx()?;
    let hold = admission::hold(&c.cfg.install).ok_or("no emergency hold on this host; run `taskq service hold` first")?;
    // the service's children, from a live status.json (§ 4); else every row is unknown
    let p = dot(root).join("status.json");
    let kids = match (data::json(&p), coreq::status::age(&p)) {
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

/// `taskq accounts` ([§ 3 Runtime accounts](../taskq.md#runtime-accounts)): this host's rows from its own files and a
/// read-only login probe per home (the email only here); other hosts' rows from their heartbeats, never an email.
pub(crate) fn accounts_cli(a: &Args) -> Res<()> {
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
