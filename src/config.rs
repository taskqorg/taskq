//! Project configuration: `taskq.json`, the host policy overlay and the opened board.

use crate::{board, host, labels, platform, profile};
use crate::Res;
use crate::board::Board;
use crate::paths::find_root;
use crate::service::Config;
use serde_json::Value;
use std::fs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) struct Ctx {
    pub(crate) cfg: Config,
    pub(crate) raw: Value,
    pub(crate) board: Box<dyn Board>,
}

/// A project's `taskq.json`: the one place that names, reads and writes the file. Each caller words its own failure.
pub(crate) struct ProjectFile(PathBuf);

impl ProjectFile {
    pub(crate) fn of(root: &Path) -> Self { Self(root.join("taskq.json")) }
    pub(crate) fn path(&self) -> &Path { &self.0 }
    pub(crate) fn exists(&self) -> bool { self.0.is_file() }
    /// The parsed file; a missing file keeps `NotFound`, a parse failure reads as `InvalidData`.
    pub(crate) fn read(&self) -> std::io::Result<Value> {
        serde_json::from_str(&fs::read_to_string(&self.0)?).map_err(std::io::Error::from)
    }
    /// The file as a record: `None` when absent, an error naming the file when it is not a JSON object.
    pub(crate) fn record(&self) -> Res<Option<Value>> { crate::data::json(&self.0) }
    pub(crate) fn write(&self, v: &Value) -> Res<()> {
        let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
        platform::write_atomic(&self.0, &(text + "\n"))
    }
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
pub(crate) enum DeliveryError {
    /// `publish` names another mode than `delivery.preset`: the setup check reports it as its `delivery` item.
    Disagrees(String),
    Invalid(String),
}

impl From<DeliveryError> for String {
    fn from(e: DeliveryError) -> Self {
        match e {
            DeliveryError::Disagrees(text) | DeliveryError::Invalid(text) => text,
        }
    }
}

pub(crate) fn check_load(root: &Path) -> Res<(Config, Value, Result<Delivery, String>)> {
    let v = project_value(root)?;
    match delivery(&v) {
        Ok((preset, _)) => load_value(root, v).map(|(c, raw)| (c, raw, Ok(preset))),
        Err(DeliveryError::Disagrees(e)) => {
            let mut lenient = v;
            if let Some(o) = lenient.as_object_mut() { o.remove("publish"); }
            load_value(root, lenient).map(|(c, raw)| (c, raw, Err(e)))
        }
        Err(DeliveryError::Invalid(e)) => Err(e),
    }
}

pub(crate) fn project_value(root: &Path) -> Res<Value> {
    let file = ProjectFile::of(root);
    file.read().map_err(|e| format!("{}: {e}", file.path().display()))
}

pub(crate) fn project_config_value(root:&Path, v: Value)->Res<(Config,Value)> {
    let str_or = |k: &str, d: &str| v[k].as_str().unwrap_or(d).to_string();
    let secs = |k: &str, d: u64| v[k].as_u64().unwrap_or(d);
    let me = std::env::var("TASKQ_HOST").ok().filter(|s| !s.is_empty()).unwrap_or_else(hostname);
    let limits: BTreeMap<String, u32> = v["limits"].as_object().into_iter().flatten().map(|(k, n)| (k.clone(), n.as_u64().unwrap_or(0) as u32)).collect();
    let board = board::kind(&str_or("board", "github"))?;
    let (delivery, delivery_revision) = delivery(&v)?;
    let cfg = Config {
        root: root.to_path_buf(),
        install: platform::default_install(),
        board,
        web_host: str_or("host", board.default_host),
        repo: str_or("repo", ""),
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
        default_runtime: crate::runtime::RuntimeId::Claude,
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

/// § 7 delivery preset; its wire names are the `delivery.preset` values of `taskq.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Delivery { AutoMerge, ReviewPr, Direct }

impl Delivery {
    pub(crate) fn name(self) -> &'static str {
        match self { Self::AutoMerge => "auto-merge", Self::ReviewPr => "review-pr", Self::Direct => "direct" }
    }
    /// The publication route a board takes: `pr` or `direct`.
    pub(crate) fn mode(self) -> &'static str {
        match self { Self::AutoMerge | Self::ReviewPr => "pr", Self::Direct => "direct" }
    }
}

impl std::fmt::Display for Delivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.name()) }
}

/// § 7 delivery: `(preset, revision)`. Absent, `publish` names the preset at revision 0; a preset never changes by
/// itself, so `publish: pr` stays `auto-merge`.
pub(crate) fn delivery(v: &Value) -> Result<(Delivery, u64), DeliveryError> {
    let publish = v["publish"].as_str();
    if v["delivery"].is_null() {
        return Ok((if publish == Some("direct") { Delivery::Direct } else { Delivery::AutoMerge }, 0));
    }
    let d = &v["delivery"];
    let revision = d["revision"].as_u64().ok_or_else(|| DeliveryError::Invalid("taskq.json: delivery.revision must be a whole number".into()))?;
    let preset = d["preset"].as_str().unwrap_or_default();
    let parsed = match preset {
        "auto-merge" => Delivery::AutoMerge,
        "review-pr" => Delivery::ReviewPr,
        "direct" => Delivery::Direct,
        // ponytail: B3 pending-handoff states come first; until then these presets cannot finish a task
        "local" | "push-branch" => return Err(DeliveryError::Invalid(format!("taskq.json: delivery.preset {preset} is not available yet: a result needs a published SHA"))),
        _ => return Err(DeliveryError::Invalid(format!("taskq.json: delivery.preset {preset:?}: use auto-merge, review-pr or direct"))),
    };
    if publish.is_some_and(|p| p != parsed.mode()) {
        return Err(DeliveryError::Disagrees(format!("taskq.json: publish {} disagrees with delivery.preset {preset}", publish.unwrap_or_default())));
    }
    Ok((parsed, revision))
}

pub(crate) fn ctx() -> Res<Ctx> {
    let (cfg, raw) = load(&find_root()?)?;
    let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
    Ok(Ctx { cfg, raw, board })
}

/// Default project ttl (§ 9): heartbeat lease and the one freshness bound (#1591).
pub(crate) const TTL: i64 = 300;
