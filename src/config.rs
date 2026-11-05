//! Project configuration: `taskq.json`, the host policy overlay and the opened board.

use crate::{board, host, labels, platform, profile};
use crate::Res;
use crate::board::Board;
use crate::paths::find_root;
use crate::service::Config;
use serde_json::Value;
use std::fs;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) struct Ctx {
    pub(crate) cfg: Config,
    pub(crate) raw: Value,
    pub(crate) board: Box<dyn Board>,
}

pub(crate) fn hostname() -> String {
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

pub(crate) fn load(root: &Path) -> Res<(Config, Value)> {
    load_value(root, project_value(root)?)
}

pub(crate) fn load_value(root: &Path, v: Value) -> Res<(Config, Value)> {
    let (mut cfg,raw)=project_config_value(root, v)?;
    if let Ok(policy)=host::Policy::read(&cfg.install) {policy.apply(&mut cfg);}
    Ok((cfg,raw))
}

/// § 6 setup check: `load`, except that `publish` disagreeing with `delivery` is the `delivery` item, not an error.
pub(crate) fn check_load(root: &Path) -> Res<(Config, Value, Result<String, String>)> {
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

pub(crate) fn project_value(root: &Path) -> Res<Value> {
    let p = root.join("taskq.json");
    serde_json::from_str(&fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))
}

pub(crate) fn project_config_value(root:&Path, v: Value)->Res<(Config,Value)> {
    let str_or = |k: &str, d: &str| v[k].as_str().unwrap_or(d).to_string();
    let secs = |k: &str, d: u64| v[k].as_u64().unwrap_or(d);
    let me = std::env::var("TASKQ_HOST").ok().filter(|s| !s.is_empty()).unwrap_or_else(hostname);
    let limits: BTreeMap<String, u32> = v["limits"].as_object().into_iter().flatten().map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0) as u32)).collect();
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
        has: vec![],
        claim: host::ClaimMode::Any,
        spillover_grace: 300,
        weights: BTreeMap::new(),
        allowed_runtimes: None,
        auto_reroute: false,
        default_runtime: crate::runtime::OWNER.into(),
        routing: labels::routing(&v)?,
        permission_mode: "dontAsk".into(),
        codex: vec!["-s".into(), "danger-full-access".into()],
        ttl: secs("ttl", TTL as u64) as i64,
        poll: secs("poll", 60),
        update_every: 60,
        rollback_after: 600,
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
pub(crate) fn delivery(v: &Value) -> Res<(String, String, u64)> {
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

pub(crate) fn ctx() -> Res<Ctx> {
    let (cfg, raw) = load(&find_root()?)?;
    let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
    Ok(Ctx { cfg, raw, board })
}

/// Default project ttl (§ 9): heartbeat lease and the one freshness bound (#1591).
pub(crate) const TTL: i64 = 300;
