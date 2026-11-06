//! Host policy, occupied slots and project supervision (taskq.md § 3, § 4, § 9).
use crate::{
    Res,
    runtime::RuntimeId,
    service::{self, Capacity, Gate},
};
use coreq::telemetry::capture::CommandReceipt;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

thread_local!(static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
const CAPACITY_COMMAND: &str = "_taskq_capacity_command";
pub fn active() -> bool {
    ACTIVE.get()
}
pub fn enter(root: &Path, install: &Path) {
    ACTIVE.set(true);
    crate::paths::PROJECT_ROOT.set(Some(root.into()));
    crate::platform::project_log(project_log(install, root));
}

/// host.json `default_runtime` values (§ 9); `codex-app` stays opt-in per task.
pub const DEFAULT_RUNTIMES: [&str; 3] = crate::runtime::CLI_RUNTIMES;

/// §4/§9: task admission on this host; manager turns stay outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClaimMode {
    #[default]
    Any,
    Spillover,
    Labelled,
    None,
}
impl ClaimMode {
    pub fn parse(s: &str) -> Res<Self> {
        match s {
            "any" => Ok(Self::Any),
            "spillover" => Ok(Self::Spillover),
            "labelled" => Ok(Self::Labelled),
            "none" => Ok(Self::None),
            _ => Err("claim must be any, spillover, labelled or none".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Spillover => "spillover",
            Self::Labelled => "labelled",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub raw: Value,
    pub cap: Capacity,
    pub me: String,
    pub has: Vec<String>,
    /// Task-claim mode (§4/§9).
    pub claim: ClaimMode,
    pub spillover_grace: u64,
    pub weights: crate::balance::Weights,
    /// `default_runtime`: reviewers and workers without `run-*` run on it (§ 4, § 9).
    pub default_runtime: crate::runtime::RuntimeId,
    pub codex: Vec<String>,
    pub permission_mode: String,
    pub update_every: u64,
    pub rollback_after: u64,
}

impl Policy {
    pub fn apply(&self, cfg: &mut service::Config) {
        cfg.weights = self.weights.clone();
        cfg.me = self.me.clone();
        cfg.has = self.has.clone();
        cfg.claim = self.claim;
        cfg.spillover_grace = self.spillover_grace;
        cfg.codex = self.codex.clone();
        let overlay = self.overlay(&cfg.root).map(|(_, p)| p);
        cfg.default_runtime = overlay
            .and_then(|p| p["default_runtime"].as_str())
            .and_then(RuntimeId::parse)
            .unwrap_or(self.default_runtime);
        cfg.allowed_runtimes = overlay
            .and_then(|p| p["allowed_runtimes"].as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|rt| rt.as_str().and_then(RuntimeId::parse))
                    .collect()
            });
        cfg.auto_reroute = overlay.is_some_and(|p| p["auto_reroute"] == true);
        cfg.permission_mode = self.permission_mode.clone();
        cfg.update_every = self.update_every;
        cfg.rollback_after = self.rollback_after;
    }
    pub fn parse(v: Value, hostname: &str) -> Res<Self> {
        if !v.is_object() || !v["capacity_hosts"].is_null() || !v["admission"]["mode"].is_null() {
            return Err("host.json: expected host policy, without capacity_hosts or mode".into());
        }
        if !matches!(v["admission"], Value::Null | Value::Object(_)) {
            return Err("host.json: admission must be an object".into());
        }
        for key in ["hosts", "has"] {
            if !matches!(v[key], Value::Null | Value::Object(_)) {
                return Err(format!("host.json: {key} must be an object"));
            }
        }
        if let Some(m) = v["hosts"].as_object() {
            if m.values().any(|v| v.as_str().is_none_or(str::is_empty)) {
                return Err("host.json: invalid host mapping".into());
            }
        }
        let strings = |v: &Value, key: &str| -> Res<Vec<String>> {
            v.as_array()
                .ok_or(format!("host.json: {key} must be an array"))?
                .iter()
                .map(|s| {
                    s.as_str()
                        .map(String::from)
                        .ok_or(format!("host.json: {key} must contain strings"))
                })
                .collect()
        };
        if let Some(m) = v["has"].as_object() {
            for x in m.values() {
                strings(x, "has")?;
            }
        }
        let me = std::env::var("TASKQ_HOST")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| v["hosts"][hostname].as_str().map(String::from))
            .unwrap_or_else(|| hostname.into());
        let has = if v["has"][&me].is_null() {
            vec![]
        } else {
            strings(&v["has"][&me], "has")?
        };
        let codex = if v[crate::runtime::CODEX_ARGS].is_null() {
            vec!["-s".into(), "danger-full-access".into()]
        } else {
            strings(&v[crate::runtime::CODEX_ARGS], crate::runtime::CODEX_ARGS)?
        };
        let permission_mode = match &v["permission_mode"] {
            Value::Null => "dontAsk".into(),
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => return Err("host.json: permission_mode must be a nonempty string".into()),
        };
        let claim = match &v["claim"] {
            Value::Null => ClaimMode::Any,
            Value::String(s) => ClaimMode::parse(s).map_err(|e| format!("host.json: {e}"))?,
            _ => return Err("host.json: claim must be a string".into()),
        };
        let spillover_grace = if v["spillover_grace"].is_null() {
            300
        } else {
            v["spillover_grace"]
                .as_u64()
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or("host.json: spillover_grace must be nonnegative seconds")?
        };
        let default_runtime = match v["default_runtime"].as_str() {
            _ if v["default_runtime"].is_null() => RuntimeId::Claude,
            Some(rt) if DEFAULT_RUNTIMES.contains(&rt) => {
                RuntimeId::parse(rt).unwrap_or(RuntimeId::Claude)
            }
            _ => {
                return Err(format!(
                    "host.json: default_runtime must be {}",
                    DEFAULT_RUNTIMES.join(" or ")
                ));
            }
        };
        let timer = |k: &str, d| {
            if v[k].is_null() {
                Ok(d)
            } else {
                v[k].as_u64()
                    .filter(|n| *n > 0)
                    .ok_or(format!("host.json: {k} must be positive seconds"))
            }
        };
        let (update_every, rollback_after) =
            (timer("update_every", 60)?, timer("rollback_after", 600)?);
        crate::platform::channel_of(&v)?;
        let mut c = v.clone();
        if c["admission"].is_null() {
            c["admission"] = json!({});
        }
        c["admission"]["mode"] = json!("auto");
        // The host policy allows the owner to disable the gate, without a Fixed-mode bypass.
        let off = c["load_gate"]["enabled"] == false;
        if off {
            c["load_gate"]["enabled"] = json!(true);
        }
        let mut cap = service::capacity(&c)?;
        if off {
            cap.gate.enabled = false;
        }
        // The project overlay only lowers: a count above the host ceiling is invalid policy, never clamped.
        if !matches!(v["projects"], Value::Null | Value::Object(_)) {
            return Err("host.json: projects must be an object".into());
        }
        for (root, p) in v["projects"].as_object().into_iter().flatten() {
            let bad = || format!("host.json: projects.{root}: invalid limits or execution policy");
            let o = p
                .as_object()
                .filter(|o| {
                    o.keys().all(|k| {
                        [
                            "limits",
                            "op",
                            "default_runtime",
                            "allowed_runtimes",
                            "auto_reroute",
                        ]
                        .contains(&k.as_str())
                    }) && [
                        "limits",
                        "default_runtime",
                        "allowed_runtimes",
                        "auto_reroute",
                    ]
                    .iter()
                    .any(|k| o.contains_key(*k))
                })
                .ok_or_else(bad)?;
            if o.get("default_runtime")
                .is_some_and(|rt| !rt.as_str().is_some_and(|rt| DEFAULT_RUNTIMES.contains(&rt)))
            {
                return Err(bad());
            }
            if o.get("auto_reroute").is_some_and(|b| !b.is_boolean()) {
                return Err(bad());
            }
            if let Some(allowed) = o.get("allowed_runtimes") {
                let a = allowed.as_array().ok_or_else(bad)?;
                let mut seen = BTreeSet::new();
                for rt in a {
                    if !rt
                        .as_str()
                        .is_some_and(|rt| service::RUNTIMES.contains(&rt) && seen.insert(rt))
                    {
                        return Err(bad());
                    }
                }
            }
            if o.get("op")
                .is_some_and(|x| !x.as_str().is_some_and(crate::resources::id))
            {
                return Err(bad());
            }
            if o.get("limits").is_some_and(|v| !v.is_object()) {
                return Err(bad());
            }
            for (rt, n) in p["limits"].as_object().into_iter().flatten() {
                let n = n
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|_| service::RUNTIMES.contains(&rt.as_str()))
                    .ok_or_else(bad)?;
                if cap.limits.get(rt).is_some_and(|host| n > *host) {
                    return Err(format!(
                        "host.json: projects.{root}.limits.{rt} raises host ceiling"
                    ));
                }
            }
        }
        let weights = crate::balance::weights(&v)?;
        crate::runtime::usage::floor_of(&v)?;
        let policy = Self {
            raw: v,
            cap,
            me,
            has,
            claim,
            spillover_grace,
            weights,
            default_runtime,
            codex,
            permission_mode,
            update_every,
            rollback_after,
        };
        policy.command()?;
        Ok(policy)
    }

    fn command(&self) -> Res<Option<CommandReceipt>> {
        let Some(value) = self.raw.get(CAPACITY_COMMAND) else {
            return Ok(None);
        };
        let invalid = "host capacity command unsupported or corrupt; file kept";
        let object = value.as_object().ok_or(invalid)?;
        if object.len() != 4 + usize::from(object.contains_key("request"))
            || !["schema", "host", "policy", "receipt"]
                .iter()
                .all(|key| object.contains_key(*key))
            || value["schema"] != 1
            || value["host"].as_str().is_none_or(str::is_empty)
        {
            return Err(invalid.into());
        }
        if object.contains_key("request")
            && !value["request"].as_str().is_some_and(crate::resources::id)
        {
            return Err(invalid.into());
        }
        let snapshot = value["policy"].as_object().ok_or(invalid)?;
        if snapshot.len() != 3
            || !["limits", "admission", "load_gate"]
                .iter()
                .all(|key| snapshot.contains_key(*key))
            || value["policy"]["admission"]["mode"] != "auto"
        {
            return Err(invalid.into());
        }
        let mut policy = value["policy"].clone();
        policy["admission"]
            .as_object_mut()
            .ok_or(invalid)?
            .remove("mode");
        if Self::parse(policy, &self.me)?.cap.json() != value["policy"] {
            return Err(invalid.into());
        }
        let text = value["receipt"].as_str().ok_or(invalid)?;
        let receipt = CommandReceipt::decode(text).map_err(|_| invalid)?;
        let metadata: Value = serde_json::from_str(text).map_err(|_| invalid)?;
        if metadata["product"] != "taskq" || metadata["operation"] != "taskq.host.policy.write" {
            return Err(invalid.into());
        }
        Ok(Some(receipt))
    }

    pub fn revision(&self) -> Res<String> {
        if self.raw[CAPACITY_COMMAND]["host"] == self.me
            && self.raw[CAPACITY_COMMAND]["policy"] == self.cap.json()
            && let Some(command) = self.command()?
        {
            return command.event_id("observed");
        }
        Ok(ring::digest::digest(
            &ring::digest::SHA256,
            &serde_json::to_vec(&json!([self.me, self.cap.json()])).map_err(|e| e.to_string())?,
        )
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect())
    }

    /// The revision a project's remote overlay operation names: the host revision, that root's overlay entry (whose
    /// `op` makes edit-and-revert a new revision) and the effective project limits, so a taskq.json `limits` edit
    /// is a new revision too. `v` is the root's taskq.json.
    pub fn project_revision(&self, root: &Path, v: &Value) -> Res<String> {
        let parts = json!([
            self.revision()?,
            self.overlay(root).map(|(_, e)| e),
            self.project_limits(root, v)?
        ]);
        Ok(ring::digest::digest(
            &ring::digest::SHA256,
            &serde_json::to_vec(&parts).map_err(|e| e.to_string())?,
        )
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect())
    }

    /// The `projects` entry of a registered root: its exact spelling, else the same canonical path.
    pub(crate) fn overlay(&self, root: &Path) -> Option<(&String, &Value)> {
        let m = self.raw["projects"].as_object()?;
        m.iter()
            .find(|(k, _)| Path::new(k.as_str()) == root)
            .or_else(|| {
                let c = fs::canonicalize(root).ok()?;
                m.iter()
                    .find(|(k, _)| fs::canonicalize(k.as_str()).is_ok_and(|x| x == c))
            })
    }

    pub fn read(install: &Path) -> Res<Self> {
        Self::read_for_hostname(install, crate::config::hostname)
    }

    pub(crate) fn read_for_hostname(
        install: &Path,
        hostname: impl FnOnce() -> String,
    ) -> Res<Self> {
        match fs::symlink_metadata(install.join("host.json.pending")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("host policy prepare unknown: {e}")),
            Ok(_) => {
                return Err(
                    "host policy interrupted write retained; admission held for reconciliation"
                        .into(),
                );
            }
        }
        let v =
            fs::read(install.join("host.json")).map_err(|e| format!("host.json unknown: {e}"))?;
        Self::parse(
            serde_json::from_slice(&v).map_err(|e| format!("host.json unknown: {e}"))?,
            &hostname(),
        )
    }

    /// The project's own limits: taskq.json `limits` and this root's host overlay, the lower of both (§ 3, § 9).
    pub fn project_limits(&self, root: &Path, v: &Value) -> Res<BTreeMap<String, u32>> {
        if !matches!(v["limits"], Value::Null | Value::Object(_)) {
            return Err("project limits: expected an object".into());
        }
        let mut out = BTreeMap::new();
        for (rt, n) in v["limits"].as_object().into_iter().flatten() {
            let n = n
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(format!("project limits.{rt}: invalid count"))?;
            // The shared taskq.json cannot know each host's ceiling: it only lowers, so a higher count clamps (§ 3 Capacity).
            let n = self.cap.limits.get(rt).map_or(n, |host| n.min(*host));
            out.insert(rt.clone(), n);
        }
        for (rt, n) in self
            .overlay(root)
            .and_then(|(_, e)| e["limits"].as_object())
            .into_iter()
            .flatten()
        {
            let n = n.as_u64().unwrap_or_default() as u32;
            out.entry(rt.clone())
                .and_modify(|x| *x = (*x).min(n))
                .or_insert(n);
        }
        Ok(out)
    }

    pub fn project(&self, root: &Path, v: &Value) -> Res<Capacity> {
        let mut c = self.cap.clone();
        c.limits.extend(self.project_limits(root, v)?);
        Ok(c)
    }
}

/// Existing resource-control lock serializes policy writers with grants.
pub fn write_capacity(
    install: &Path,
    host: &str,
    edit: &crate::cli::host_cmd::CapEdit,
    expect: Option<&Value>,
) -> Res<(Capacity, Capacity, CommandReceipt)> {
    crate::resources::locked(install, || {
        write_capacity_locked(install, host, edit, expect, None)
    })
}

#[tracing::instrument(name = "host.policy.write", skip_all)]
fn write_capacity_locked(
    install: &Path,
    host: &str,
    edit: &crate::cli::host_cmd::CapEdit,
    expect: Option<&Value>,
    mut local: Option<(&mut crate::resources::State, &Value)>,
) -> Res<(Capacity, Capacity, CommandReceipt)> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {
        return Err("a worker cannot change host policy".into());
    }
    let path = install.join("host.json");
    let original = fs::read_to_string(&path).map_err(|e| format!("host.json unknown: {e}"))?;
    let hostname = crate::config::hostname();
    let old = Policy::parse(
        serde_json::from_str(&original).map_err(|e| format!("host.json unknown: {e}"))?,
        &hostname,
    )?;
    if host != old.me {
        return Err(
            "host policy edits require this local host; remote edits are unsupported".into(),
        );
    }
    if let Some((state, request)) = local.as_mut() {
        let id = request["id"]
            .as_str()
            .ok_or("capacity request ID missing")?;
        if let Some(op) = state.ops.get_mut(id) {
            if op.bound != json!({"local_capacity":request}) {
                return Err("capacity request ID reused with another payload".into());
            }
            if op.receipt["state"] == "rejected" {
                return Err(op.receipt["reason"]
                    .as_str()
                    .unwrap_or("capacity request rejected")
                    .into());
            }
            let command = if op.receipt["state"] == "saved" {
                let command = CommandReceipt::decode(
                    op.receipt["command"]
                        .as_str()
                        .ok_or("capacity receipt corrupt")?,
                )?;
                if op.receipt["saved_revision"] != command.event_id("observed")? {
                    return Err("capacity receipt corrupt".into());
                }
                command
            } else if old.raw[CAPACITY_COMMAND]["request"] == id
                && old.raw[CAPACITY_COMMAND]["policy"] == old.cap.json()
            {
                old.command()?.ok_or("capacity request outcome unknown")?
            } else {
                return Err(
                    "capacity request outcome unknown; mutation will not be repeated".into(),
                );
            };
            op.writing = false;
            op.sent = true;
            op.receipt = json!({"state":"saved","saved_revision":command.event_id("observed")?,"command":command.encode()});
            return Ok((old.cap.clone(), old.cap, command));
        }
        if state.ops.len() >= crate::resources::MAX_OPS {
            return Err("capacity operation store full; no new request taken".into());
        }
        let now = wall()?;
        if request["issued_at"]
            .as_u64()
            .is_none_or(|at| at > now.saturating_add(30))
            || request["expires_at"].as_u64().is_none_or(|at| now >= at)
        {
            return Err("capacity request expired or clock unknown".into());
        }
        let revision = old.revision()?;
        if request["ops"]
            .as_array()
            .ok_or("capacity operations missing")?
            .iter()
            .any(|op| op["expected_revision"] != revision)
        {
            return Err("host policy changed; review current values before applying".into());
        }
    }
    if edit.mode.is_some() {
        return Err(
            "host policy has no mode selector; use optional limits and the load gate".into(),
        );
    }
    if expect.is_some_and(|v| *v != old.cap.json()) {
        return Err("host policy changed; refresh before applying".into());
    }
    let mut next = old.raw.clone();
    for rt in edit.limits.keys().chain(&edit.unset) {
        if !service::RUNTIMES.contains(&rt.as_str()) {
            return Err(format!("unknown runtime {rt}"));
        }
    }
    if !edit.limits.is_empty() {
        if next["limits"].is_null() {
            next["limits"] = json!({});
        }
        for (rt, n) in &edit.limits {
            next["limits"][rt] = json!(n);
        }
    }
    for rt in &edit.unset {
        if let Some(limits) = next["limits"].as_object_mut() {
            limits.remove(rt);
        }
    }
    if let Some(max) = edit.max {
        if next["admission"].is_null() {
            next["admission"] = json!({});
        }
        next["admission"]["max"] = json!(max);
    }
    if edit.gate.is_some() || edit.cpu_max.is_some() || edit.memory_min.is_some() {
        if next["load_gate"].is_null() {
            next["load_gate"] = json!({});
        }
        if let Some(value) = edit.gate {
            next["load_gate"]["enabled"] = json!(value);
        }
        if let Some(value) = edit.cpu_max {
            next["load_gate"]["cpu_max"] = json!(value);
        }
        if let Some(value) = edit.memory_min {
            next["load_gate"]["memory_min"] = json!(value);
        }
    }
    if let Some(mode) = edit.claim {
        next["claim"] = json!(mode.name());
    }
    if let Some(rt) = &edit.default_runtime {
        next["default_runtime"] = json!(rt);
    }
    if let Some(weights) = &edit.weights {
        next["weights"] = json!(weights);
    }
    let candidate = Policy::parse(next, &hostname)?;
    let command = CommandReceipt::current()?;
    crate::telemetry::receipt(&command, "received")?;
    let mut next = candidate.raw;
    next[CAPACITY_COMMAND] = json!({"schema":1,"host":candidate.me,"policy":candidate.cap.json(),"receipt":command.encode()});
    if let Some((state, request)) = local.as_mut() {
        let id = request["id"]
            .as_str()
            .ok_or("capacity request ID missing")?;
        next[CAPACITY_COMMAND]["request"] = json!(id);
        state.ops.insert(
            id.into(),
            crate::resources::Op {
                bound: json!({"local_capacity":request}),
                receipt: json!({"state":"unknown"}),
                expires_at: request["expires_at"]
                    .as_u64()
                    .ok_or("capacity expiry missing")?,
                saved_at: None,
                writing: true,
                sent: true,
            },
        );
        crate::resources::save(install, state)?;
        #[cfg(test)]
        if std::env::var("TASKQ_HOST_POLICY_PHASE").is_ok_and(|phase| phase == "before-write") {
            crate::platform::write_atomic(&install.join("ack"), "intent-saved")?;
            loop {
                std::thread::park();
            }
        }
        if wall()?
            >= request["expires_at"]
                .as_u64()
                .ok_or("capacity expiry missing")?
        {
            let op = state.ops.get_mut(id).unwrap();
            op.writing = false;
            op.receipt = json!({"state":"rejected","reason":"expired"});
            return Err("capacity request expired before host policy write".into());
        }
    }
    let candidate = Policy::parse(next, &hostname)?;
    replace_owned(
        &path,
        &original,
        &(serde_json::to_string_pretty(&candidate.raw).map_err(|e| e.to_string())? + "\n"),
    )?;
    let saved = Policy::read(install)?;
    if saved.raw != candidate.raw {
        return Err("host policy write uncertain: readback differs".into());
    }
    if let Some((state, request)) = local.as_mut() {
        let op = state.ops.get_mut(request["id"].as_str().unwrap()).unwrap();
        op.writing = false;
        op.saved_at = Some(wall()?);
        op.receipt = json!({"state":"saved","saved_revision":command.event_id("observed")?,"command":command.encode()});
    }
    #[cfg(test)]
    if std::env::var("TASKQ_HOST_POLICY_PHASE").is_ok_and(|phase| phase == "before-applied") {
        crate::platform::write_atomic(&install.join("ack"), "policy-saved")?;
        loop {
            std::thread::park();
        }
    }
    crate::telemetry::receipt(&command, "applied")?;
    crate::platform::write_atomic(&install.join("poke"), "")
        .map_err(|e| format!("host policy saved; host notification unknown: {e}"))?;
    Ok((old.cap, saved.cap, command))
}

/// The first policy of a host: an explicit `service install` writes it when none exists (§ 9); the service never does.
pub(crate) fn seed_policy(install: &Path) -> Res<()> {
    let path = install.join("host.json");
    if path
        .try_exists()
        .map_err(|e| format!("host.json unknown: {e}"))?
    {
        return Ok(());
    }
    let policy = json!({"hosts":{},"has":{},(crate::runtime::CODEX_ARGS):["-s","danger-full-access"],"permission_mode":"dontAsk","update_every":60,"rollback_after":600});
    Policy::parse(policy.clone(), &crate::config::hostname())?;
    fs::create_dir_all(install).map_err(|e| e.to_string())?;
    crate::platform::write_atomic(
        &path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&policy).map_err(|e| e.to_string())?
        ),
    )
}

/// The remote project overlay writer (#1282, shared-resource-controls.md#project-overlay); the caller holds the
/// resource lock. Only `projects.<root>` changes. `Ok(Err(reason))`: refused before any write; `Err`: write uncertain.
pub(crate) fn write_project_limit(
    install: &Path,
    root: &Path,
    rt: &str,
    limit: Option<u32>,
    op: &str,
    expect: &str,
) -> Res<Result<Policy, &'static str>> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() || !service::RUNTIMES.contains(&rt) {
        return Ok(Err("unsupported"));
    }
    let path = install.join("host.json");
    let Ok(original) = fs::read_to_string(&path) else {
        return Ok(Err("conflict"));
    };
    let hostname = crate::config::hostname();
    let Ok(old) = serde_json::from_str(&original)
        .map_err(|e| e.to_string())
        .and_then(|v| Policy::parse(v, &hostname))
    else {
        return Ok(Err("conflict"));
    };
    let Ok((_, project)) = crate::config::load(root) else {
        return Ok(Err("conflict"));
    };
    if old.project_revision(root, &project)? != expect {
        return Ok(Err("conflict"));
    }
    let key = old
        .overlay(root)
        .map_or_else(|| root.display().to_string(), |(k, _)| k.clone());
    let mut next = old.raw.clone();
    if next["projects"].is_null() {
        next["projects"] = json!({});
    }
    let entry = &mut next["projects"][&key];
    if entry.is_null() {
        *entry = json!({});
    }
    if entry["limits"].is_null() {
        entry["limits"] = json!({});
    }
    match limit {
        Some(n) => entry["limits"][rt] = json!(n),
        None => {
            entry["limits"].as_object_mut().map(|l| l.remove(rt));
        }
    }
    // kept when the limits empty: the restore is a new revision too
    entry["op"] = json!(op);
    let Ok(candidate) = Policy::parse(next, &hostname) else {
        return Ok(Err("invalid"));
    };
    replace_owned(
        &path,
        &original,
        &(serde_json::to_string_pretty(&candidate.raw).map_err(|e| e.to_string())? + "\n"),
    )?;
    let saved = Policy::read(install)?;
    if saved.raw != candidate.raw {
        return Err("host policy write uncertain: readback differs".into());
    }
    Ok(Ok(saved))
}

fn wall() -> Res<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| "capacity clock unknown".into())
}

/// Typed native request; OS-owned local controls do not grant the remote exchange any authority.
pub fn capacity_request(request: &Value) -> Res<crate::cli::host_cmd::CapEdit> {
    use crate::resources::{count, id, keys};
    let invalid = "invalid host capacity request";
    if !keys(request, &["id", "issued_at", "expires_at", "ops"])
        || !request["id"].as_str().is_some_and(id)
        || serde_json::to_vec(request)
            .map_err(|e| e.to_string())?
            .len()
            > 16 << 10
    {
        return Err(invalid.into());
    }
    let issued = request["issued_at"].as_u64().ok_or(invalid)?;
    let expires = request["expires_at"].as_u64().ok_or(invalid)?;
    if expires <= issued || expires - issued > 300 {
        return Err(invalid.into());
    }
    let ops = request["ops"]
        .as_array()
        .filter(|ops| !ops.is_empty() && ops.len() <= service::RUNTIMES.len() + 1)
        .ok_or(invalid)?;
    let mut seen = BTreeSet::new();
    let mut edit = crate::cli::host_cmd::CapEdit::default();
    for op in ops {
        if !keys(
            op,
            &["target", "policy_schema", "expected_revision", "changes"],
        ) || op["policy_schema"] != "taskq.host-capacity/1"
            || !op["expected_revision"].as_str().is_some_and(id)
            || !keys(
                &op["target"],
                &["installation_id", "generation", "project_id", "runtime_id"],
            )
            || op["target"]["generation"] != 1
            || !op["target"]["project_id"].is_null()
        {
            return Err(invalid.into());
        }
        let rt = op["target"]["runtime_id"].as_str();
        if (!op["target"]["runtime_id"].is_null()
            && rt.is_none_or(|rt| !service::RUNTIMES.contains(&rt)))
            || !seen.insert(rt)
        {
            return Err(invalid.into());
        }
        let changes = op["changes"]
            .as_object()
            .filter(|c| !c.is_empty())
            .ok_or(invalid)?;
        for (key, value) in changes {
            match key.as_str() {
                "runtime_limit" if rt.is_some() && (value.is_null() || count(value)) => {
                    let rt = rt.unwrap().to_string();
                    if value.is_null() {
                        edit.unset.insert(rt);
                    } else {
                        edit.limits.insert(rt, value.as_u64().unwrap() as u32);
                    }
                }
                "maximum" if rt.is_none() && (value.is_null() || count(value)) => {
                    edit.max = Some(value.as_u64().map(|n| n as u32))
                }
                "load_gate"
                    if rt.is_none()
                        && keys(value, &["enabled", "cpu_max", "memory_min"])
                        && value["enabled"].is_boolean()
                        && value["cpu_max"]
                            .as_u64()
                            .is_some_and(|n| (10..=100).contains(&n))
                        && value["memory_min"]
                            .as_u64()
                            .is_some_and(|n| (1..=95).contains(&n)) =>
                {
                    edit.gate = value["enabled"].as_bool();
                    edit.cpu_max = value["cpu_max"].as_u64().map(|n| n as u32);
                    edit.memory_min = value["memory_min"].as_u64().map(|n| n as u32);
                }
                _ => return Err(invalid.into()),
            }
        }
    }
    Ok(edit)
}

// #1351: the tray's limits save fails fast while another host writer (grant, migration, DashQ) holds the lock.
#[cfg(not(test))]
const LOCAL_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(test)]
const LOCAL_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

pub fn local_capacity(install: &Path, host: &str, request: &Value) -> Res<Value> {
    let edit = capacity_request(request)?;
    let receipt = crate::resources::with_within(install, LOCAL_LOCK_WAIT, |state| {
        crate::resources::prepare(state, install, wall()?)?;
        let installation = match fs::read_to_string(install.join("installation-id")) {
            Ok(id)
                if id.len() == 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
            {
                json!(id)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
            _ => return Err("local installation identity unknown".into()),
        };
        if request["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["target"]["installation_id"] != installation)
        {
            return Err("capacity request belongs to another installation".into());
        }
        let result = write_capacity_locked(install, host, &edit, None, Some((state, request)));
        let mut receipt = state
            .ops
            .get(request["id"].as_str().unwrap())
            .filter(|op| op.bound == json!({"local_capacity":request}))
            .map(|op| op.receipt.clone())
            .unwrap_or(json!({"state":"rejected"}));
        if let Err(error) = result {
            receipt["reason"] = json!(error);
        }
        receipt
            .as_object_mut()
            .ok_or("local capacity receipt corrupt")?
            .remove("command");
        Ok(receipt)
    })?;
    Ok(receipt.unwrap_or_else(||json!({"state":"rejected","reason":"another host settings change is in progress; old limits kept. Try again in a minute"})))
}

/// The host already loaded this snapshot; a later writer cannot borrow its observation.
pub fn observe_capacity(install: &Path, loaded: &Policy) -> Res<Option<String>> {
    let Some(command) = loaded.command()? else {
        return Ok(None);
    };
    let observed = coreq::telemetry::capture::operation(
        "host.policy.observe",
        || {
            crate::resources::try_locked(install, || {
                let current = Policy::read(install)?;
                let binding = &loaded.raw[CAPACITY_COMMAND];
                if current.raw != loaded.raw
                    || binding["host"] != loaded.me
                    || binding["policy"] != loaded.cap.json()
                {
                    return Err("capacity command changed or no longer matches the loaded policy; observation unknown".into());
                }
                crate::telemetry::receipt(&command, "applied")?;
                crate::telemetry::receipt(&command, "observed").map(Some)
            })
        },
        |_| "command-observation",
    )?;
    if observed.is_some()
        && let Ok(roots) = project_paths(install)
    {
        for root in &roots {
            if let Err(error) = crate::wake::poke(root) {
                crate::platform::log(&error);
            }
        }
    }
    Ok(observed)
}

/// Read the exact durable acknowledgment, never infer it from equal values or wall-clock freshness.
pub fn capacity_observed(install: &Path, command: &CommandReceipt) -> Res<bool> {
    let current = Policy::read(install)?;
    if current
        .command()?
        .is_none_or(|saved| saved.encode() != command.encode())
        || current.raw[CAPACITY_COMMAND]["host"] != current.me
        || current.raw[CAPACITY_COMMAND]["policy"] != current.cap.json()
    {
        return Err("host policy saved; command superseded or manually changed".into());
    }
    let metadata: Value = serde_json::from_str(&command.encode()).map_err(|e| e.to_string())?;
    let id = command.event_id("observed")?;
    let reader = coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
    let page = reader.export_kinds(
        &["command.observed"],
        metadata["started_ms"]
            .as_i64()
            .ok_or("capacity command time unknown")?,
        0,
        1024,
    )?;
    let found = page["events"]
        .as_array()
        .ok_or("capacity receipt page invalid")?
        .iter()
        .any(|event| event["id"] == id);
    if !found && page["more"] == true {
        return Err("capacity receipt page limit; observation unknown".into());
    }
    Ok(found)
}

/// Reuse migration's native replacement so a settings edit preserves owner/group/ACL and file permissions.
fn replace_owned(path: &Path, original: &str, next: &str) -> Res<()> {
    use std::io::Write;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.permissions().readonly() {
        return Err("state file is not a writable regular file; kept".into());
    }
    let owner = ownership(path)?;
    let stage = path.with_file_name(&{
        let mut name = path
            .file_name()
            .ok_or("state file name missing")?
            .to_os_string();
        name.push(".pending");
        name
    });
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage)
        .map_err(|e| format!("state file prepare refused; interrupted state kept: {e}"))?;
    let result = (|| {
        stage_owner(&stage, &owner)?;
        file.set_permissions(metadata.permissions())
            .map_err(|e| e.to_string())?;
        file.write_all(next.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if fs::read_to_string(path).map_err(|e| e.to_string())? != original
            || ownership(path)? != owner
        {
            return Err("state file changed while saving; current file kept".into());
        }
        replace_project(path, &stage)?;
        #[cfg(windows)]
        stage_owner(path, &owner)?;
        #[cfg(unix)]
        fs::File::open(path.parent().ok_or("state file parent missing")?)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        if ownership(path)? != owner {
            return Err("state file ownership readback differs".into());
        }
        Ok(())
    })();
    if result.is_err()
        && let Err(cleanup) = fs::remove_file(&stage)
        && cleanup.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!(
            "state file write uncertain: {}; temporary cleanup failed: {cleanup}",
            result.unwrap_err()
        ));
    }
    result.map_err(|e: String| format!("{} write uncertain: {e}", path.display()))
}

// The atomic registry publisher reserves projects.pending for its own bytes.
pub(crate) const INSTALL_PENDING: &str = "projects.installing";

pub type Shared = Arc<Mutex<Host>>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Claim {
    Task(u64, u64),
    External(String),
}
type Slot = (PathBuf, Claim);
fn slot(project: &Path, task: u64, attempt: u64) -> Slot {
    // Before claim there is no fencing token; only that provisional slot uses the issue number.
    (
        project.into(),
        Claim::Task(if attempt == 0 { task } else { 0 }, attempt),
    )
}
pub enum Maintenance {
    Update(Option<String>),
    Installation(String),
}

/// #2255: one project's queue as its last pass saw it, in board time. `project` is named only on the project's own
/// board; other projects of the service stay anonymous there (a public board never names a private one).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Queue {
    pub project: Option<String>,
    /// Ready tasks this host may serve: admitted or held by this host, not refused by labels, routing or focus.
    pub ready: u64,
    pub spawn: Option<i64>,
    /// Since when ready work waits without a spawn; reset by each spawn.
    pub waiting: Option<i64>,
    /// The first refusal of a waiting task; only its admission code for another project's row.
    pub reason: Option<String>,
    /// #2290: `<code>: <reason>` per ready task this pass did not admit; only on the project's own board row.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<u64, String>,
}

/// #2255: heartbeat `queue {json}`: the restart pending since, and the queue of every project the service serves.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Queues {
    pub restart: Option<i64>,
    pub projects: Vec<Queue>,
}

pub struct Host {
    pub instance: String,
    pub policy: Res<Policy>,
    pub capacity_command: Option<String>,
    slots: BTreeMap<Slot, String>,
    pub gate: Gate,
    began: Instant,
    last: Option<u64>,
    /// Registered ready projects, rotated only after a grant.
    ready: BTreeSet<PathBuf>,
    external_ready: BTreeMap<String, PathBuf>,
    turn: Option<PathBuf>,
    pub held: Option<String>,
    pub restart: bool,
    /// Projects were woken for this handover; later passes keep `poll` (#1357).
    restart_woken: bool,
    /// #2254: the handover stayed pending past its 600 s bound; status shows a host incident.
    pub restart_overdue: bool,
    pub stopping: bool,
    pub stop_controller: Option<crate::host_control::Control>,
    /// Each thread reports active turns/children before the host considers an image switch.
    pub busy: BTreeMap<PathBuf, bool>,
    /// The last finished pass of each project reported its handover prepared; `passing` projects have not finished.
    pub prepared: BTreeSet<PathBuf>,
    passing: BTreeSet<PathBuf>,
    pub recovered: BTreeSet<PathBuf>,
    pub updating: Option<std::thread::JoinHandle<Res<Maintenance>>>,
    pub draining: BTreeSet<PathBuf>,
    pub request_since: u64,
    pub hour_start: BTreeMap<String, u64>,
    pub last_hour: Option<BTreeMap<String, u64>>,
    /// Billed requests per API host: cumulative at the last full hour, and that hour's (#2084).
    pub hosts_start: BTreeMap<String, u64>,
    pub hosts_last_hour: Option<BTreeMap<String, u64>>,
    pub rebuilding: bool,
    rebuilt: BTreeSet<PathBuf>,
    published: BTreeMap<PathBuf, Instant>,
    /// #2255: each project's last queue row.
    pub queues: BTreeMap<PathBuf, Queue>,
}

impl Host {
    pub fn new(policy: Res<Policy>) -> Self {
        Self {
            instance: crate::runtime::uuid(),
            policy,
            capacity_command: None,
            slots: BTreeMap::new(),
            gate: Gate::default(),
            began: Instant::now(),
            last: None,
            ready: BTreeSet::new(),
            external_ready: BTreeMap::new(),
            turn: None,
            held: None,
            restart: false,
            restart_woken: false,
            restart_overdue: false,
            stopping: false,
            stop_controller: None,
            busy: BTreeMap::new(),
            prepared: BTreeSet::new(),
            passing: BTreeSet::new(),
            recovered: BTreeSet::new(),
            updating: None,
            draining: BTreeSet::new(),
            request_since: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            hour_start: crate::requests::totals(),
            last_hour: None,
            hosts_start: crate::requests::host_totals(),
            hosts_last_hour: None,
            rebuilding: false,
            rebuilt: BTreeSet::new(),
            published: BTreeMap::new(),
            queues: BTreeMap::new(),
        }
    }

    pub fn reload(&mut self, policy: Res<Policy>) {
        if self.policy != policy {
            self.capacity_command = None;
        }
        if let Ok(p) = &policy {
            self.gate.configure(p.cap.gate);
        }
        self.policy = policy;
    }

    pub(crate) fn reload_admission(
        &mut self,
        policy: Res<Policy>,
        held: Option<String>,
        roots: &[PathBuf],
    ) {
        let policy_changed = policy.is_ok() && self.policy != policy;
        let was_blocked = self.policy.is_err() || self.held.is_some();
        self.reload(policy);
        self.held = held;
        if policy_changed || (was_blocked && self.policy.is_ok() && self.held.is_none()) {
            for root in roots {
                if let Err(error) = crate::wake::poke(root) {
                    crate::platform::log(&error);
                }
            }
        }
    }

    /// A newly attached project may carry old occupancy, even if its root was drained earlier.
    fn attach(&mut self, root: &Path) {
        self.busy.insert(root.into(), true);
        self.prepared.remove(root);
        self.passing.remove(root);
        self.recovered.remove(root);
        self.rebuilt.remove(root);
        self.rebuilding = true;
    }

    fn detached(&mut self, root: &Path) {
        self.busy.remove(root);
        self.prepared.remove(root);
        self.recovered.remove(root);
        self.rebuilt.remove(root);
        self.ready.remove(root);
        self.rebuilding =
            self.rebuilding && !self.busy.keys().all(|root| self.rebuilt.contains(root));
    }

    pub fn sync(&mut self, project: &Path, held: Vec<crate::admission::Res>) -> usize {
        self.slots
            .retain(|(p, claim), _| p != project || matches!(claim, Claim::External(_)));
        for (n, a, rt) in held {
            self.slots.insert(slot(project, n, a), rt);
        }
        self.slots.len()
    }

    /// A poisoned table is unusable until every active project reports its still-owned handles and pending grants.
    pub fn rebuild_slots(&mut self, project: &Path, held: Vec<crate::admission::Res>) -> bool {
        if !self.rebuilding {
            self.rebuilding = true;
            self.rebuilt.clear();
            self.slots
                .retain(|(_, claim), _| matches!(claim, Claim::External(_)));
            self.ready.clear();
            self.gate = Gate::default();
            self.last = Some(self.elapsed());
        }
        self.sync(project, held);
        self.rebuilt.insert(project.into());
        let complete = self.busy.keys().all(|root| self.rebuilt.contains(root));
        if complete {
            self.rebuilding = false;
        }
        complete
    }

    pub fn ready(&mut self, project: &Path, ready: bool) {
        if ready {
            self.ready.insert(project.into());
        } else {
            self.ready.remove(project);
        }
    }

    pub fn publication(&mut self, project: &Path, qualified: bool) {
        if qualified {
            self.published.insert(project.into(), Instant::now());
        } else {
            self.published.remove(project);
        }
    }

    pub fn publications_ready(&self, roots: &[PathBuf]) -> bool {
        self.policy.is_ok()
            && self.held.is_none()
            && !self.rebuilding
            && roots.iter().all(|root| {
                self.published
                    .get(root)
                    .is_some_and(|at| at.elapsed().as_secs() < 60)
            })
    }

    pub fn handoff_ready(&self) -> bool {
        self.handoff_settled() && self.passing.is_empty()
    }

    /// The handover waits only for passes in flight: no project starts another one (#1402).
    pub(crate) fn handoff_settled(&self) -> bool {
        self.restart
            && !self.rebuilding
            && self.held.is_none()
            && self.updating.is_none()
            && (!self.stopping || (self.slots.is_empty() && self.busy.values().all(|busy| !*busy)))
            && self.busy.keys().all(|root| self.prepared.contains(root))
            && self
                .slots
                .keys()
                .all(|(root, _)| self.prepared.contains(root))
    }

    pub fn handoff_wait(&self) -> Option<crate::lifecycle::HandoffWait> {
        use crate::lifecycle::HandoffWait::*;
        if self.handoff_ready() {
            None
        } else if self.stopping && !self.slots.is_empty() {
            Some(Execution)
        } else if self.rebuilding || self.held.is_some() {
            Some(Recovery)
        } else if self.stopping && self.busy.values().any(|busy| *busy)
            || self
                .busy
                .keys()
                .chain(self.slots.keys().map(|(root, _)| root))
                .any(|root| !self.prepared.contains(root))
            || self.updating.is_none() && !self.passing.is_empty()
        {
            Some(Project)
        } else if self.updating.is_some() {
            Some(Maintenance)
        } else {
            None
        }
    }

    /// #1357: a pending handover wakes each project once. Waking every host second made every project list the
    /// board every second; each pass already reports its preparation, so the handover follows within one `poll`.
    pub fn restart_wake(&mut self) -> bool {
        self.restart && !std::mem::replace(&mut self.restart_woken, true)
    }

    /// A drained Stop project cannot revoke preparation while other projects are still draining. #1402: a pass
    /// keeps the project's last report until it ends; once every report is prepared no new pass starts, so six
    /// projects whose passes overlap still reach a moment with none in flight.
    pub fn begin_pass(&mut self, root: &Path) -> bool {
        // A pass that never reported (an error or panic) leaves no acknowledgment.
        if self.passing.remove(root) {
            self.prepared.remove(root);
        }
        if self.handoff_settled()
            || self.stopping
                && self.prepared.contains(root)
                && self.busy.get(root) == Some(&false)
                && !self.slots.keys().any(|(project, _)| project == root)
        {
            return false;
        }
        self.passing.insert(root.into());
        self.recovered.remove(root);
        self.busy.insert(root.into(), true);
        true
    }

    /// The pass's handover report; `prepared` only when nothing it owns blocks the re-exec.
    pub fn end_pass(&mut self, root: &Path, prepared: bool) {
        self.passing.remove(root);
        if prepared {
            self.prepared.insert(root.into());
        } else {
            self.prepared.remove(root);
        }
    }

    /// The § 3 grant under the host policy.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(
        &mut self,
        project: &Path,
        n: u64,
        a: u64,
        rt: &str,
        cap: &Capacity,
        queue: bool,
        now: u64,
    ) -> Res<usize> {
        self.reserve_key(slot(project, n, a), rt, cap, queue, now)
    }

    fn reserve_key(
        &mut self,
        key: Slot,
        rt: &str,
        cap: &Capacity,
        queue: bool,
        now: u64,
    ) -> Res<usize> {
        let project = &key.0;
        let p = self.policy.clone()?;
        if self.rebuilding {
            return Err("host occupancy recovery pending".into());
        }
        if let Some(reason) = &self.held {
            return Err(reason.clone());
        }
        if self.draining.contains(project) && queue {
            return Err("project draining".into());
        }
        if self.restart {
            return Err("host restart pending".into());
        }
        if self.slots.contains_key(&key) {
            if p.cap.limits.get(rt) == Some(&0) || cap.limits.get(rt) == Some(&0) {
                return Err("runtime disabled".into());
            }
            return Ok(self.slots.len());
        }
        let ready: BTreeSet<_> = self
            .ready
            .iter()
            .chain(self.external_ready.values())
            .collect();
        if queue && !ready.is_empty() {
            let next = ready
                .iter()
                .copied()
                .find(|p| self.turn.as_ref().is_none_or(|t| *p > t))
                .or_else(|| ready.first().copied());
            if next.is_some_and(|p| p != project) {
                return Err("round-robin: another project is ready".into());
            }
        }
        if let Some(left) = self
            .last
            .and_then(|t| 30u64.checked_sub(now.saturating_sub(t)))
            .filter(|left| *left > 0)
        {
            return Err(format!(
                "host load gate: 30 s launch spacing ({left} s remaining)"
            ));
        }
        if !self.gate.may_launch(now) {
            let (state, reasons) = self.gate.state(now);
            let reason = if reasons.is_empty() {
                "waiting for post-launch sample".into()
            } else {
                reasons
                    .into_iter()
                    .map(|(_, text)| text)
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            return Err(format!("host load gate {state}: {reason}"));
        }
        let used = self.slots.values().filter(|r| *r == rt).count();
        let own = self
            .slots
            .iter()
            .filter(|((root, _), r)| root == project && *r == rt)
            .count();
        if p.cap.max.is_some_and(|m| self.slots.len() >= m as usize) {
            return Err("host aggregate capacity full".into());
        }
        if p.cap.limits.get(rt).is_some_and(|m| used >= *m as usize) {
            return Err(format!("{rt} host capacity full"));
        }
        if cap.limits.get(rt).is_some_and(|m| own >= *m as usize) {
            return Err(format!("{rt} project capacity full"));
        }
        if queue {
            self.turn = Some(project.into());
        }
        self.slots.insert(key, rt.into());
        self.last = Some(now);
        self.gate.launch(now);
        Ok(self.slots.len())
    }

    pub(crate) fn external_held(&self, id: &str) -> bool {
        self.slots
            .keys()
            .any(|(_, claim)| matches!(claim,Claim::External(key) if key==id))
    }

    pub(crate) fn external_known(&self, known: &BTreeSet<String>) -> bool {
        self.slots.keys().all(|(_, claim)| match claim {
            Claim::External(id) => known.contains(id),
            _ => true,
        })
    }

    pub(crate) fn external_sync(&mut self, id: &str, project: &Path, runtime: &str, held: bool) {
        self.external_ready.remove(id);
        let key = (project.into(), Claim::External(id.into()));
        if held {
            self.slots.insert(key, runtime.into());
        } else {
            self.slots.remove(&key);
        }
    }

    pub(crate) fn external_reset_ready(&mut self) {
        self.external_ready.clear();
    }

    pub(crate) fn external_reserve(
        &mut self,
        id: &str,
        project: &Path,
        runtime: &str,
        cap: &Capacity,
        now: u64,
    ) -> Res<()> {
        self.external_ready.insert(id.into(), project.into());
        self.reserve_key(
            (project.into(), Claim::External(id.into())),
            runtime,
            cap,
            true,
            now,
        )?;
        self.external_ready.remove(id);
        Ok(())
    }

    pub fn claimed(&mut self, project: &Path, n: u64, a: u64) {
        if let Some(rt) = self.slots.remove(&slot(project, n, 0)) {
            self.slots.insert(slot(project, n, a), rt);
        }
    }

    pub fn remaining(&self, project: &Path, rt: &str, cap: &Capacity) -> usize {
        if self.rebuilding {
            return 0;
        }
        let Ok(p) = &self.policy else { return 0 };
        let host = (p.cap.limits.get(rt).copied().unwrap_or(u32::MAX) as usize)
            .saturating_sub(self.slots.values().filter(|r| *r == rt).count());
        let own = (cap.limits.get(rt).copied().unwrap_or(u32::MAX) as usize).saturating_sub(
            self.slots
                .iter()
                .filter(|((root, _), r)| root == project && *r == rt)
                .count(),
        );
        let max = (p.cap.max.unwrap_or(u32::MAX) as usize).saturating_sub(self.slots.len());
        host.min(own).min(max)
    }

    pub fn elapsed(&self) -> u64 {
        self.began.elapsed().as_secs()
    }

    /// Host-wide pending grants and running children (§ 3), for the § 8 compile-jobs share.
    pub fn occupied(&self) -> usize {
        self.slots.len()
    }

    pub fn used(&self) -> BTreeMap<String, usize> {
        let mut used = BTreeMap::new();
        for runtime in self.slots.values() {
            *used.entry(runtime.clone()).or_default() += 1;
        }
        used
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceRecord {
    schema: u8,
    pid: u32,
    started: String,
    instance: String,
    at: u64,
    version: String,
    #[serde(default)]
    slots_protocol: Option<u8>,
    #[serde(default)]
    restart_protocol: Option<u8>,
    #[serde(default)]
    stop_protocol: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full_stop_protocol: Option<u8>,
    #[serde(default)]
    uninstall_protocol: Option<u8>,
    #[serde(default)]
    install_protocol: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    install_cancel_protocol: Option<u8>,
}
fn service_record(install: &Path) -> Res<Option<ServiceRecord>> {
    use std::io::Read;
    let file = match fs::File::open(install.join("service")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("host service owner unknown: {e}")),
        Ok(file) => file,
    };
    let mut bytes = Vec::new();
    file.take(1025)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("host service owner unknown: {e}"))?;
    if bytes.len() > 1024 {
        return Err("host service owner unknown: oversized record".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "host service owner unknown: invalid text")?;
    let r: ServiceRecord =
        serde_json::from_str(&text).map_err(|_| "host service owner unknown: malformed record")?;
    if r.install_cancel_protocol.is_some_and(|v| v != 1) {
        return Err("host service cancellation protocol unknown; kept".into());
    }
    if r.schema != 1
        || r.pid == 0
        || r.pid > i32::MAX as u32
        || r.started.is_empty()
        || r.started.len() > 128
        || !r
            .started
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || !crate::resources::id(&r.instance)
        || r.version.is_empty()
        || r.version.len() > 80
        || r.restart_protocol.is_some_and(|v| v != 1)
        || r.stop_protocol.is_some_and(|v| v != 1 && v != 2)
        || r.full_stop_protocol.is_some_and(|v| v != 1)
        || r.uninstall_protocol.is_some_and(|v| v != 1)
        || r.install_protocol.is_some_and(|v| v != 1)
        || r.slots_protocol.is_some_and(|v| v != 1)
    {
        return Err("host service owner unknown: unsupported record; kept".into());
    }
    Ok(Some(r))
}

fn publish_service(install: &Path, native: &coreq::process::Identity, instance: &str) -> Res<()> {
    let record = ServiceRecord {
        schema: 1,
        pid: native.pid,
        started: native.started.clone(),
        instance: instance.into(),
        at: wall()?,
        version: crate::platform::VERSION.into(),
        slots_protocol: Some(1),
        restart_protocol: Some(1),
        stop_protocol: Some(2),
        full_stop_protocol: cfg!(target_os = "macos").then_some(1),
        uninstall_protocol: Some(1),
        install_protocol: Some(1),
        install_cancel_protocol: Some(1),
    };
    crate::platform::write_synced(
        &install.join("service"),
        &(serde_json::to_string(&record).map_err(|e| e.to_string())? + "\n"),
    )
}

/// A prior recorded owner must have exited; age never proves that. The native lock still owns admission.
pub fn other(install: &Path) -> Res<Option<u32>> {
    let record = match service_record(install) {
        Err(e) => {
            return if unreadable_exited(install)? || zeroed_unowned(install)? {
                Ok(None)
            } else {
                Err(e)
            };
        }
        Ok(record) => record,
    };
    let Some(r) = record else { return Ok(None) };
    let live =
        coreq::process::identity(r.pid).map_err(|e| format!("host service owner unknown: {e}"))?;
    Ok(
        (r.pid != std::process::id() && live.as_ref().is_some_and(|p| p.started == r.started))
            .then_some(r.pid),
    )
}

/// #1445: a power loss can zero the record; the free native owner lock proves no live owner, so keep it aside.
fn zeroed_unowned(install: &Path) -> Res<bool> {
    let Ok(_lock) = coreq::file_lock::try_lock(&install.join("service.lock")) else {
        return Ok(false);
    };
    let moved = crate::data::set_aside_zeroed(&install.join("service"))?;
    if moved {
        crate::platform::log(
            "host service record zeroed by a power loss; kept as service.zeroed-*, startup continues",
        );
    }
    Ok(moved)
}

/// A record this version cannot read (a newer release's protocol or field) is stale once its recorded process
/// has exited (#1310): its pid and start time alone prove that. A live or unidentifiable owner stays unknown.
fn unreadable_exited(install: &Path) -> Res<bool> {
    use std::io::Read;
    let Ok(file) = fs::File::open(install.join("service")) else {
        return Ok(false);
    };
    let mut bytes = Vec::new();
    if file.take(1025).read_to_end(&mut bytes).is_err() || bytes.len() > 1024 {
        return Ok(false);
    }
    let Ok(record) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(false);
    };
    let pid = record["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .filter(|p| *p != 0 && *p <= i32::MAX as u32);
    let (Some(pid), Some(started)) = (
        pid,
        record["started"].as_str().filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        }),
    ) else {
        return Ok(false);
    };
    if pid == std::process::id() {
        return Ok(false);
    }
    let live =
        coreq::process::identity(pid).map_err(|e| format!("host service owner unknown: {e}"))?;
    Ok(live.is_none_or(|p| p.started != started))
}

/// Native process state and projection freshness are separate; neither releases any owned execution.
pub fn service_status(install: &Path) -> Value {
    let result = coreq::telemetry::capture::operation(
        "host.service.observe",
        || -> Res<Value> {
            let r = service_record(install)?.ok_or("host service owner record missing")?;
            let native = coreq::process::identity(r.pid)
                .map_err(|e| format!("host service owner unknown: {e}"))?;
            let live = native.as_ref().is_some_and(|p| p.started == r.started);
            let age = wall()?.checked_sub(r.at);
            let fresh = live && age.is_some_and(|age| age <= 60);
            Ok(
                json!({"state":if live {"running"}else{"exited"},"pid":r.pid,"started":r.started,"instance":r.instance,"version":r.version,
            "at":r.at,"age_seconds":age,"slots_protocol":r.slots_protocol,"fresh":fresh,"restart_protocol":r.restart_protocol,"stop_protocol":r.stop_protocol,"full_stop_protocol":r.full_stop_protocol,"uninstall_protocol":r.uninstall_protocol,"install_protocol":r.install_protocol,"install_cancel_protocol":r.install_cancel_protocol,"reason":if !live {Some("recorded host process exited; worker outcomes remain separate")}else if !fresh {Some("host observation stale or clock unknown")}else{None}}),
            )
        },
        |_| "owner-unknown",
    );
    result.unwrap_or_else(|reason| json!({"state":"unknown","fresh":false,"reason":reason}))
}

pub fn service_line(status: &Value) -> String {
    match status["state"].as_str() {
        Some("running") => format!(
            "Service: running (host, pid {}); observation {}",
            status["pid"],
            if status["fresh"] == true {
                "fresh"
            } else {
                "stale or unknown"
            }
        ),
        Some("exited") => format!(
            "Service: recorded host process exited (pid {}); worker outcomes unknown here",
            status["pid"]
        ),
        _ => format!(
            "Service: unknown ({})",
            status["reason"].as_str().unwrap_or("no native observation")
        ),
    }
}

/// Legacy roots are never served concurrently, even when their status has gone stale. Unknown holds admission.
pub fn project_service(root: &Path) -> Res<Option<u32>> {
    match crate::data::json(&crate::paths::dot(root).join("status.json")) {
        Err(e) => return Err(format!("legacy liveness unknown: {e}")),
        Ok(None) => Ok(None),
        Ok(Some(v)) => {
            let pid = v["pid"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("legacy liveness unknown: missing pid")?;
            if pid == 0 {
                return Err("process liveness unknown: invalid pid".into());
            }
            Ok((pid != std::process::id() && crate::runtime::liveness(pid)?).then_some(pid))
        }
    }
}

fn legacy(install: &Path, roots: &[PathBuf]) -> Res<()> {
    for root in roots {
        // #1445: the host owner lock is held, so a zeroed status.json has no live owner.
        if crate::data::set_aside_zeroed(&crate::paths::dot(root).join("status.json"))? {
            append_project(
                install,
                root,
                "status.json zeroed by a power loss; kept as status.json.zeroed-*",
            );
        }
        if project_service(root)?.is_some() {
            return Err("handoff: legacy service still running".into());
        }
    }
    Ok(())
}

fn startup_status(install: &Path, root: &Path, error: &str, instance: &str) -> Res<()> {
    if project_service(root)?.is_some() {
        return Err("legacy ownership retained; startup status not written".into());
    }
    let mut status = coreq::status::common(
        &crate::platform::tool(install),
        "unknown",
        &crate::paths::project_name(root),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        Some(error.into()),
        None,
    );
    fs::create_dir_all(crate::paths::dot(root)).map_err(|e| e.to_string())?;
    status["host_instance"] = json!(instance);
    crate::platform::write_status(&crate::platform::tool(install), root, &status)
}

fn owner(install: &Path) -> Res<Option<coreq::file_lock::Lock>> {
    if other(install)?.is_some() {
        return Ok(None);
    }
    match coreq::file_lock::try_lock(&install.join("service.lock")) {
        Ok(lock) => Ok(Some(lock)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(e) => Err(format!("host service owner lock unknown: {e}")),
    }
}

// An admission barrier does not replace valid host maintenance intervals.
fn guarded_policy(configured: Res<Policy>, barrier: &Res<()>) -> (Res<Policy>, (u64, u64)) {
    let timers = configured
        .as_ref()
        .map(|p| (p.update_every, p.rollback_after))
        .unwrap_or((60, 600));
    (
        barrier.as_ref().map_err(Clone::clone).and(configured),
        timers,
    )
}

pub fn run(install: &Path) -> Res<()> {
    fs::create_dir_all(install).map_err(|e| e.to_string())?;
    // #2057: after a re-exec the old image's helpers (tray `open`, curl, `log show`) are children no thread waits for
    #[cfg(unix)]
    match crate::runtime::inherit_children() {
        Ok(pids) if pids.is_empty() => {}
        Ok(pids) => crate::platform::log(&format!(
            "host start: {} inherited children reaped on exit",
            pids.len()
        )),
        Err(e) => crate::platform::log(&format!(
            "host start: {e}; inherited children stay unreaped"
        )),
    }
    // #2214: correct the host's low scheduling before starting probe, curl and git children.
    #[cfg(windows)]
    match crate::runtime::windows_process::normal_scheduling() {
        Ok(Some(was)) => crate::platform::log(&format!(
            "host start: scheduling raised to normal (was {was})"
        )),
        Ok(None) => {}
        Err(e) => crate::platform::log(&format!(
            "host start: {e}; probes may time out under worker load"
        )),
    }
    let mut reported = String::new();
    let mut linked = std::time::Instant::now();
    let _owner = loop {
        let message = match owner(install) {
            Ok(Some(owner)) => break owner,
            Ok(None) => "host service owner already active; waiting".into(),
            Err(e) => format!("{e}; waiting"),
        };
        if message != reported {
            crate::platform::log(&message);
            reported = message;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    match crate::resources::retire_superseded(install) {
        Ok(Some(line)) => crate::platform::log(&line),
        Ok(None) => {}
        Err(e) => crate::platform::log(&e),
    }
    // #1625: each start (logon, install, restart) re-arms the 5-minute repeat that Stop disabled.
    #[cfg(windows)]
    match crate::host_control::repeat_trigger(&crate::platform::tool(install), true) {
        Ok("changed") => crate::platform::log("host task 5-minute restart trigger enabled"),
        Ok(_) => {}
        Err(e) => crate::platform::log(&format!("host task restart trigger: {e}")),
    }
    let mut telemetry = crate::telemetry::HostCapture::default();
    // Host identity is stable for this owner; policy and interrupted writes are still read each tick.
    let mut hostname = crate::config::hostname();
    let shared = Arc::new(Mutex::new(Host::new(Policy::read_for_hostname(
        install,
        || hostname.clone(),
    ))));
    shared
        .lock()
        .map_err(|_| "host occupancy recovery unknown")?
        .rebuilding = true;
    let native = coreq::process::identity(std::process::id())
        .map_err(|e| e.to_string())?
        .ok_or("host process identity missing")?;
    let instance = shared
        .lock()
        .map_err(|_| "host owner unknown")?
        .instance
        .clone();
    // § 3 Runtime accounts: one poll per host, independent of project count; the host owner above admits one process.
    if !cfg!(test) {
        crate::runtime::usage::spawn(install.to_path_buf());
    }
    let mut threads: BTreeMap<PathBuf, std::thread::JoinHandle<Res<()>>> = BTreeMap::new();
    let mut roots = vec![];
    let mut stopped = BTreeSet::new();
    let mut drained_roots = BTreeSet::new();
    let mut checked: Option<Instant> = None;
    let mut installation_checked: Option<Instant> = None;
    let mut maintenance_result = String::new();
    let mut upgrade_report: Option<bool> = None;
    let (mut waits_reported, mut stuck) = (String::new(), None);
    let began = Instant::now();
    let mut hour = (Instant::now(), crate::requests::totals());
    let mut tray = None;
    let mut doctor = crate::doctor::Doctor::default();
    let mut registry = Registry::default();
    let mut restart = crate::lifecycle::Monitor::default();
    if let Err(e) = crate::setup::recover(install) {
        crate::platform::log(&e);
    }
    let mut recorded = Instant::now();
    loop {
        if let Some(line) = telemetry.pass(install) {
            crate::platform::log(&line);
        }
        // §4 Board request records: here, ahead of the handover paths that skip the rest of the tick.
        if recorded.elapsed() >= crate::requests::RECORD_EVERY {
            crate::requests::flush(crate::requests::SERVICE);
            recorded = Instant::now();
        }
        // #1297: steady host passes fold like project passes (service_run.rs `service.loop`).
        let _repeating = tracing::info_span!("service.host.loop", coreq_repeating = true).entered();
        if let Err(e) = crate::profile::prune(install) {
            eprintln!("taskq: telemetry unknown: {e}");
        }
        let publication = publish_service(install, &native, &instance).err();
        if let Some(error) = &publication {
            let message = format!("host publication unknown: {error}");
            if reported != message {
                crate::platform::log(&message);
                reported = message;
            }
        }
        registry.refresh(install, began.elapsed().as_secs());
        restart.refresh(install);
        roots.clone_from(&registry.roots);
        for root in &roots {
            match crate::runtime::owner::registered_host_trust(install, root) {
                Ok(lines) => {
                    for line in lines {
                        crate::platform::log(&line);
                    }
                }
                Err(e) => {
                    let message =
                        format!("Trust setup pending: {e}; service retries automatically");
                    if reported != message {
                        crate::platform::log(&message);
                        reported = message;
                    }
                }
            }
        }
        let old = legacy(install, &roots);
        if hostname.is_empty() {
            hostname = crate::config::hostname();
        }
        let configured = Policy::read_for_hostname(install, || hostname.clone());
        let (policy, timers) = guarded_policy(configured, &old);
        // #1769: a Stop whose generation ended can never complete; this owner resolves it instead of holding forever
        if restart
            .current
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .is_some_and(|p| p.stops() && p.origin() != instance)
        {
            match crate::lifecycle::supersede(install, &instance) {
                Ok(Some(id)) => {
                    log_all(
                        install,
                        &roots,
                        &format!(
                            "Stop {id} of an ended host generation dropped; this generation serves on"
                        ),
                    );
                    restart.current = Ok(None);
                }
                Ok(None) => {}
                Err(e) => {
                    if reported != e {
                        crate::platform::log(&e);
                        reported = e;
                    }
                }
            }
        }
        let removal = restart
            .current
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .and_then(|p| p.removal().map(Path::to_path_buf));
        drained_roots.retain(|root| removal.as_ref() == Some(root));
        {
            let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
            let held = installation::admission_reason(install)
                .or_else(|| registry.error.clone())
                .or_else(|| old.as_ref().err().cloned())
                .or_else(|| restart.current.as_ref().err().cloned())
                .or_else(|| {
                    publication
                        .as_ref()
                        .map(|e| format!("host publication unavailable: {e}"))
                });
            h.reload_admission(policy, held, &roots);
            if let Some(p) = restart.current.as_ref().ok().and_then(Option::as_ref) {
                if p.stops() {
                    if !h.stopping {
                        h.prepared.clear();
                    }
                    h.restart = true;
                    h.stopping = true;
                    if p.origin() != instance {
                        h.held = Some(format!(
                            "Stop {} belongs to an ended host generation; dropped after its expiry",
                            p.id()
                        ));
                    }
                } else if p.action() == crate::lifecycle::Action::Restart && p.origin() == instance
                {
                    h.restart = true;
                }
            }
            if let Err(error) = crate::external_slots::tick(install, &mut h) {
                h.held = Some(format!("external slots unknown: {error}"));
            }
            let now = h.elapsed();
            if h.gate.last.is_none_or(|l| now.saturating_sub(l.0) >= 30) {
                h.gate.host_mem = crate::platform::host_memory();
                let wall = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64);
                h.gate.observe(
                    now,
                    wall,
                    crate::platform::cpu_ticks(),
                    crate::platform::memory_available(),
                );
            }
            for root in threads.keys().filter(|p| !roots.contains(p)) {
                h.draining.insert(root.clone());
            }
            for root in &roots {
                if removal.as_ref() == Some(root) {
                    h.draining.insert(root.clone());
                } else {
                    h.draining.remove(root);
                }
                if threads.contains_key(root)
                    || stopped.contains(root)
                    || drained_roots.contains(root)
                {
                    continue;
                }
                h.attach(root);
                let root = root.clone();
                let install = install.to_path_buf();
                let host = shared.clone();
                threads.insert(
                    root.clone(),
                    std::thread::spawn(move || {
                        enter(&root, &install);
                        loop {
                            // Never replace another process's ownership evidence, including on startup/config failure.
                            match project_service(&root) {
                                Ok(None) => {}
                                Ok(Some(_)) | Err(_) => {
                                    std::thread::sleep(std::time::Duration::from_secs(1));
                                    continue;
                                }
                            }
                            // A broken project config holds only this thread; no board or child exists yet.
                            let context = crate::config::load(&root).and_then(|(cfg, raw)| {
                                let board = cfg.board.open(
                                    &cfg.web_host,
                                    &cfg.repo,
                                    raw["label_schema"].as_str(),
                                    &crate::platform::tool(&install),
                                )?;
                                Ok(crate::config::Ctx { cfg, raw, board })
                            });
                            match context {
                                Ok(c) => return crate::service_run::service_run(&c, Some(host)),
                                Err(e) => {
                                    if let Ok(mut h) = host.lock() {
                                        if h.draining.contains(&root)
                                            && installation::cancelling_root(&install).as_deref()
                                                == Some(root.as_path())
                                            && !h.slots.keys().any(|(project, _)| project == &root)
                                            && crate::service_run::kids_file(&install, &root)?
                                                .is_empty()
                                        {
                                            return Err("project drained".into());
                                        }
                                        // No board/native reconciliation exists while configuration is invalid.
                                        h.busy.insert(root.clone(), true);
                                        h.prepared.remove(&root);
                                    }
                                    let instance =
                                        host.lock().map(|h| h.instance.clone()).unwrap_or_default();
                                    let _ = startup_status(&install, &root, &e, &instance);
                                    crate::platform::log(&e);
                                    std::thread::sleep(std::time::Duration::from_secs(1));
                                }
                            }
                        }
                    }),
                );
            }
        }
        {
            let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
            if roots.is_empty() && h.busy.is_empty() && h.held.is_none() {
                h.rebuilding = false;
            }
        }
        // § 4 setup sessions: only this loop launches them. ponytail: a launch blocks this pass up to 30 s (Codex: 60 s); a thread if that matters.
        if let Err(e) = crate::setup::step(install, crate::setup::launch) {
            if reported != e {
                crate::platform::log(&e);
                reported = e;
            }
        }
        // D2/D6 workspace dialogue: owner turns and watches; a holder probe runs only when a turn is pending.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if let Err(e) = crate::dialogue::tick(install, now, &crate::dialogue::NATIVE) {
            if reported != e {
                crate::platform::log(&e);
                reported = e;
            }
        }
        // Missing setup links: one native read per 10 s, not per pass.
        if linked.elapsed() >= std::time::Duration::from_secs(10) {
            linked = std::time::Instant::now();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            if let Err(e) = crate::setup::link(install, now, crate::setup::claude_link) {
                if reported != e {
                    crate::platform::log(&e);
                    reported = e;
                }
            }
        }
        let loaded = {
            let h = shared.lock().unwrap_or_else(|e| e.into_inner());
            h.capacity_command
                .is_none()
                .then(|| h.policy.as_ref().ok().cloned())
                .flatten()
        };
        if let Some(policy) = loaded {
            match observe_capacity(install, &policy) {
                Ok(command) => {
                    let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
                    let changed =
                        h.policy.as_ref().ok() == Some(&policy) && h.capacity_command != command;
                    if changed {
                        h.capacity_command = command;
                    }
                    drop(h);
                    if changed {
                        for root in &roots {
                            if let Err(error) = crate::wake::poke(root) {
                                crate::platform::log(&error);
                            }
                        }
                    }
                }
                Err(e) => {
                    if reported != e {
                        crate::platform::log(&e);
                        reported = e;
                    }
                }
            }
        }
        if let Err(error) = reconcile_projects(
            install,
            &shared,
            &mut threads,
            &roots,
            &mut stopped,
            &mut drained_roots,
        ) {
            if reported != error {
                crate::platform::log(&error);
                reported = error;
            }
        }
        if let Some(p) = restart
            .current
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .filter(|p| p.action() == crate::lifecycle::Action::Uninstall && !p.stops())
        {
            let drained = p.removal().is_some_and(|root| drained_roots.contains(root));
            if drained
                || p.removal()
                    .is_some_and(|root| !roots.iter().any(|p| p == root))
            {
                if let Err(error) =
                    crate::lifecycle::finish_project_uninstall(install, p.id(), &instance, drained)
                {
                    let _ = crate::lifecycle::failed(install, p.id(), &error);
                    if reported != error {
                        crate::platform::log(&error);
                        reported = error;
                    }
                }
            }
        }
        let h = shared.lock().unwrap_or_else(|e| e.into_inner());
        let recovered =
            h.publications_ready(&roots) && roots.iter().all(|r| h.recovered.contains(r));
        drop(h);
        if let Err(e) = crate::lifecycle::observe(install, &instance, recovered) {
            if reported != e {
                crate::platform::log(&e);
                reported = e;
            }
        }
        let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
        let finished = h
            .stop_controller
            .as_mut()
            .and_then(|control| control.poll().map(|result| (control.id.clone(), result)));
        if let Some((id, result)) = finished {
            drop(h);
            let error = result
                .err()
                .unwrap_or_else(|| "controller exited without an observed host stop".into());
            let _ = crate::lifecycle::controller_exited(install, &id, &error);
            crate::platform::log(&format!(
                "Stop controller: {}",
                crate::cli::text::compact(&error, 153)
            ));
            h = shared.lock().unwrap_or_else(|e| e.into_inner());
        }
        if h.updating.as_ref().is_some_and(|job| job.is_finished()) {
            let updated = h.updating.take().unwrap().join().unwrap_or_else(|_| {
                Err("host maintenance panicked; partial effects unknown".into())
            });
            let text = match &updated {
                Ok(Maintenance::Update(Some(v))) => format!("update: {v}"),
                Ok(Maintenance::Update(None)) => "update: up to date".into(),
                Ok(Maintenance::Installation(result)) => format!("installation: {result}"),
                Err(e) => format!("maintenance: {e}"),
            };
            if maintenance_once(&mut maintenance_result, &text) {
                log_all(install, &roots, &text);
            }
            if crate::paths::installed(install)
                && matches!(updated, Ok(Maintenance::Update(Some(_))))
            {
                h.restart = true;
            }
            // § 9 deploy: the release flow records the applied version in each release root that sets `deploy`
            if let (true, Ok(Maintenance::Update(Some(v))), Ok(exe)) = (
                crate::paths::installed(install),
                &updated,
                std::env::current_exe(),
            ) {
                for l in crate::deploy::applied(
                    &exe,
                    &roots,
                    crate::platform::channel(install).unwrap_or("live"),
                    crate::platform::VERSION,
                    v,
                    &crate::config::hostname(),
                ) {
                    log_all(install, &roots, &l);
                }
            }
        }
        let transition_ready = crate::platform::transition_ready(install).is_ok();
        if !transition_ready
            && h.updating.is_none()
            && installation_checked.is_none_or(|at| at.elapsed().as_secs() >= 5)
        {
            installation_checked = Some(Instant::now());
            let install = install.to_path_buf();
            let job = coreq::telemetry::capture::operation(
                "host.install.recover.dispatch",
                || {
                    std::thread::Builder::new()
                        .name("taskq-install-recovery".into())
                        .spawn(coreq::telemetry::capture::bind(move || {
                            coreq::telemetry::capture::operation(
                                "host.install.recover",
                                || installation::recover(&install).map(Maintenance::Installation),
                                |_| "install-pending",
                            )
                        }))
                },
                |_| "spawn",
            );
            match job {
                Ok(job) => h.updating = Some(job),
                Err(error) => {
                    crate::platform::log(&format!("installation recovery not started: {error}"))
                }
            }
        }
        let asked = transition_ready
            && !h.restart
            && h.updating.is_none()
            && (fs::remove_file(install.join("update")).is_ok()
                || roots
                    .iter()
                    .any(|r| fs::remove_file(crate::paths::dot(r).join("update")).is_ok()));
        if transition_ready
            && !h.restart
            && h.updating.is_none()
            && (asked || checked.is_none_or(|t| t.elapsed().as_secs() >= timers.0))
        {
            checked = Some(Instant::now());
            let install = install.to_path_buf();
            let job = coreq::telemetry::capture::operation(
                "host.update.dispatch",
                || {
                    std::thread::Builder::new()
                        .name("taskq-update".into())
                        .spawn(coreq::telemetry::capture::bind(move || {
                            coreq::telemetry::capture::operation(
                                "host.update",
                                || {
                                    (if crate::paths::installed(&install) {
                                        crate::platform::service_update(&install)
                                    } else {
                                        crate::platform::update(&install, true)
                                    })
                                    .map(Maintenance::Update)
                                },
                                |_| "update-failed",
                            )
                        }))
                },
                |_| "spawn",
            );
            match job {
                Ok(job) => h.updating = Some(job),
                Err(error) => crate::platform::log(&format!("update not started: {error}")),
            }
        }
        if install.join("restart").exists()
            || roots
                .iter()
                .any(|r| crate::paths::dot(r).join("restart").exists())
        {
            h.restart = true;
        }
        if h.restart_wake() {
            for root in &roots {
                if let Err(e) = crate::wake::poke(root) {
                    append_project(install, root, &e);
                }
            }
        }
        // The installer's 300 s wait began before this generation started; a reason not retained retries next tick.
        let late = began.elapsed().as_secs() >= 300;
        if upgrade_report != Some(late) {
            if let Some(reason) = installation::stale_upgrade(install, late) {
                if late {
                    log_all(install, &roots, &reason);
                }
                upgrade_report = Some(late);
            }
        }
        let handover = h.restart && !h.stopping;
        if !handover {
            stuck = None;
        }
        h.restart_overdue = stuck == Some(u64::MAX);
        if !h.restart {
            waits_reported.clear();
        }
        if h.restart {
            let wait = h.handoff_wait();
            drop(h);
            // #1601: the receipt and the log name what the re-exec really waits for; a stuck one is reported, never killed
            let waits = restart_waits(&roots);
            if let Err(e) = crate::lifecycle::handoff_wait(install, &instance, wait, &waits) {
                if reported != e {
                    crate::platform::log(&e);
                    reported = e;
                }
            }
            // #2209: every tick, so a superseding intent's receipt names the waits too; the log names each change.
            installation::upgrade_waits(install, crate::platform::VERSION, &waits);
            if waits != waits_reported {
                log_all(install, &roots, &format!("host re-exec waits for {waits}"));
                waits_reported = waits.clone();
            }
            if crate::service_run::restart_watch(
                &mut stuck,
                handover,
                began.elapsed().as_secs(),
                300,
            ) {
                log_all(
                    install,
                    &roots,
                    &format!("restart pending over 600 s; waits for {waits}"),
                );
            }
            h = shared.lock().unwrap_or_else(|e| e.into_inner());
            if h.handoff_ready() {
                if h.stopping && h.stop_controller.is_some() {
                    drop(h);
                    // The controller may end this process at any tick.
                    crate::requests::flush(crate::requests::SERVICE);
                    crate::platform::drain_telemetry();
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
                let stopping = h.stopping;
                drop(h);
                if let Err(e) = crate::lifecycle::prepare(install, &instance) {
                    if stopping && let Ok(Some(p)) = crate::lifecycle::pending(install) {
                        let _ = crate::lifecycle::failed(install, p.id(), &e);
                    }
                    if reported != e {
                        crate::platform::log(&e);
                        reported = e;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
                if stopping {
                    // The controller ends this process without its exit path.
                    crate::requests::flush(crate::requests::SERVICE);
                    crate::platform::drain_telemetry();
                    match crate::lifecycle::pending(install).and_then(|p| {
                        p.filter(|p| p.stops())
                            .ok_or("prepared Stop binding unavailable".into())
                    }) {
                        Ok(p) => match crate::host_control::dispatch(install, &p) {
                            Ok(control) => {
                                crate::platform::log(&format!(
                                    "service stops: Stop {} controller ends this process",
                                    p.id()
                                ));
                                shared
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .stop_controller = Some(control);
                            }
                            Err(e) => {
                                let _ = crate::lifecycle::failed(install, p.id(), &e);
                                crate::platform::log(&e);
                            }
                        },
                        Err(e) => crate::platform::log(&e),
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }
                let _ = fs::remove_file(install.join("restart"));
                for root in &roots {
                    let _ = fs::remove_file(crate::paths::dot(root).join("restart"));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    let mut command = crate::platform::command(crate::paths::launcher(install));
                    command
                        .args(["service", "run"])
                        .current_dir(install)
                        .env_remove("TASKQ_ROOT");
                    crate::requests::flush(crate::requests::SERVICE);
                    crate::platform::before_exec();
                    let e = command.exec();
                    return Err(format!("host re-exec: {e}"));
                }
                #[cfg(not(unix))]
                crate::platform::service_exit(
                    75,
                    &format!(
                        "restart to {}",
                        crate::platform::tool(install).current().unwrap_or_default()
                    ),
                );
            }
        }
        drop(h);
        #[cfg(unix)]
        crate::runtime::reap_inherited();
        if crate::paths::installed(install) {
            // #2048: hourly host doctor, off the tick thread: stale releases and qualification leftovers.
            if let Some(line) = doctor.tick(install) {
                crate::platform::log(&line);
            }
            crate::service_run::keep_tray(install, &mut tray);
            if began.elapsed().as_secs() >= timers.1
                && shared.lock().is_ok_and(|h| h.publications_ready(&roots))
            {
                if let Err(e) = fs::write(
                    install
                        .join("releases")
                        .join(crate::platform::VERSION)
                        .join("healthy"),
                    "",
                ) {
                    crate::platform::log(&format!("release health unknown: {e}"));
                }
            }
        }
        if hour.0.elapsed().as_secs() >= 3600 {
            let total = crate::requests::totals();
            let usage = crate::requests::diff(&total, &hour.1);
            for root in &roots {
                append_project(
                    install,
                    root,
                    &format!("requests 1h: {}", crate::requests::summary(&usage, "")),
                );
            }
            let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
            h.last_hour = Some(usage);
            h.hour_start = total.clone();
            let hosts = crate::requests::host_totals();
            h.hosts_last_hour = Some(crate::requests::diff(&hosts, &h.hosts_start));
            h.hosts_start = hosts;
            hour = (Instant::now(), total);
        }
        // Watch <install>/poke every second; the registry is otherwise polled once a minute.
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// The restart waits each project last published in its status (§ 5), named by project folder.
fn restart_waits(roots: &[PathBuf]) -> String {
    let waits: Vec<String> = roots
        .iter()
        .flat_map(|root| {
            let name = root.file_name().map_or_else(
                || root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let status = crate::data::json(&crate::paths::dot(root).join("status.json"))
                .ok()
                .flatten()
                .unwrap_or_default();
            status["restart_waits"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|w| w.as_str())
                .map(|w| format!("{name} {w}"))
                .collect::<Vec<_>>()
        })
        .collect();
    if waits.is_empty() {
        "project passes in flight".into()
    } else {
        waits.join(", ")
    }
}

// Join before reconciling cancellation; a thread exit alone never releases its execution.
fn reconcile_projects(
    install: &Path,
    shared: &Shared,
    threads: &mut BTreeMap<PathBuf, std::thread::JoinHandle<Res<()>>>,
    roots: &[PathBuf],
    stopped: &mut BTreeSet<PathBuf>,
    drained_roots: &mut BTreeSet<PathBuf>,
) -> Res<()> {
    // Joining is nonblocking: failed projects do not stop unrelated loops, and their occupied slots stay held.
    let ended: Vec<_> = threads
        .iter()
        .filter(|(_, t)| t.is_finished())
        .map(|(p, _)| p.clone())
        .collect();
    for root in ended {
        let outcome = threads.remove(&root).unwrap().join();
        let drained = matches!(&outcome, Ok(Err(e)) if e == "project drained");
        let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
        h.publication(&root, false);
        h.passing.remove(&root);
        if drained {
            h.detached(&root);
            drained_roots.insert(root.clone());
        } else {
            // A reduced loop failed: its attempts expire on other hosts, and local launches hold for inspection.
            log_all(
                install,
                std::slice::from_ref(&root),
                &thread_error(&outcome),
            );
            stopped.insert(root.clone());
            h.prepared.remove(&root);
            h.recovered.remove(&root);
            h.ready(&root, false);
            append_project(
                install,
                &root,
                "project thread stopped; leases expire on other hosts; retained slots have unknown liveness",
            );
        }
    }
    if let Some(root) = installation::cancelling_root(install) {
        let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
        installation::finish_cancellation(
            install,
            &mut h,
            threads.contains_key(&root) || roots.contains(&root),
        )?;
        stopped.remove(&root);
    }
    Ok(())
}

/// #1416: a host line goes to the service log and every project log; project `taskq`'s log is the service's stderr file, so it gets the line once.
fn log_all(install: &Path, roots: &[PathBuf], text: &str) {
    crate::platform::log(text);
    mirror(
        install,
        roots,
        text,
        crate::platform::stderr_file(install).as_deref(),
    );
}

/// #1445: the reason a project thread ended, for the host and project logs.
fn thread_error(outcome: &std::thread::Result<Res<()>>) -> String {
    let reason = match outcome {
        Ok(Ok(())) => "ended without error".to_string(),
        Ok(Err(e)) => e.clone(),
        Err(panic) => panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .map_or("panic".into(), |p| format!("panic: {p}")),
    };
    format!(
        "project thread error: {}",
        crate::cli::text::compact(&reason, 136)
    )
}

fn mirror(install: &Path, roots: &[PathBuf], text: &str, stderr: Option<&Path>) {
    for root in roots
        .iter()
        .filter(|r| stderr != Some(project_log(install, r).as_path()))
    {
        append_project(install, root, text);
    }
}

fn project_log(install: &Path, root: &Path) -> PathBuf {
    install.join(format!("{}.log", crate::paths::project_name(root)))
}

fn append_project(install: &Path, root: &Path, text: &str) {
    use std::io::Write;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(project_log(install, root))
        .and_then(|mut f| writeln!(f, "{}", crate::platform::log_line(now, text)));
}

fn project_text(install: &Path) -> Res<Option<String>> {
    let prepare = format!("{INSTALL_PENDING}.pending");
    for name in ["projects.pending", prepare.as_str()] {
        match fs::symlink_metadata(install.join(name)) {
            Ok(_) => return Err(format!("{name} retained; registry publication unknown")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("projects prepare unknown: {e}")),
        }
    }
    match fs::symlink_metadata(install.join(INSTALL_PENDING)) {
        Ok(_) => return installation::registered_projects(install),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("installation prepare unknown: {e}")),
    }
    let path = install.join("projects");
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keep configured spelling for existing resource IDs; canonical identity owns execution and aliases.
fn project_entries(text: &str) -> Res<Vec<(PathBuf, PathBuf)>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let path = PathBuf::from(line);
            if !path.is_absolute() {
                return Err("projects unknown: a root is not absolute".into());
            }
            let canonical = path
                .canonicalize()
                .map_err(|e| format!("projects unknown: {e}"))?;
            if !canonical.is_dir() {
                return Err("projects unknown: a root is not a directory".into());
            }
            Ok((path, canonical))
        })
        .collect()
}

pub fn project_paths(install: &Path) -> Res<Vec<PathBuf>> {
    let text = project_text(install)?.ok_or_else(|| {
        format!(
            "{}: project registry missing",
            install.join("projects").display()
        )
    })?;
    Ok(project_entries(&text)?
        .into_iter()
        .map(|(path, _)| path)
        .collect())
}

/// Read only the explicit registry. Canonical identity prevents duplicate threads for aliases.
pub fn projects(install: &Path) -> Res<Vec<PathBuf>> {
    let text = project_text(install)?.ok_or("projects unknown: registry missing")?;
    Ok(project_entries(&text)?
        .into_iter()
        .map(|(_, canonical)| canonical)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

/// One owner retains the preflight snapshot and native lock until the registry commit or refusal.
pub struct ProjectEdit {
    install: PathBuf,
    original: Option<String>,
    next: String,
    removed: Vec<PathBuf>,
    adding: bool,
    _lock: coreq::file_lock::Lock,
}
impl ProjectEdit {
    #[tracing::instrument(name = "host.projects.prepare", skip_all)]
    pub fn new(install: &Path, root: &Path, adding: bool) -> Res<Self> {
        Self::with_guard(install, root, adding, || {
            crate::lifecycle::registry_guard(install)
        })
    }
    pub fn for_uninstall(install: &Path, root: &Path, id: &str) -> Res<Self> {
        Self::with_guard(install, root, false, || {
            crate::lifecycle::uninstall_guard(install, id, root)
        })
    }
    fn with_guard(
        install: &Path,
        root: &Path,
        adding: bool,
        guard: impl FnOnce() -> Res<()>,
    ) -> Res<Self> {
        fs::create_dir_all(install).map_err(|e| e.to_string())?;
        let lock = coreq::file_lock::try_lock(&install.join("projects.lock"))
            .map_err(|e| format!("project registry busy or lock unavailable: {e}"))?;
        guard()?;
        crate::platform::transition_ready(install)?;
        let path = install.join("projects");
        let original = project_text(install)?;
        if original.is_none() && !adding {
            return Err(format!(
                "{}: project registry missing; nothing changed",
                path.display()
            ));
        }
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() || meta.permissions().readonly() {
                    return Err("project registry is not a writable regular file; kept".into());
                }
                ownership(&path)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && original.is_none() => {}
            Err(e) => return Err(format!("project registry metadata unknown: {e}")),
        }
        let target = root
            .canonicalize()
            .map_err(|e| format!("project root unknown: {e}"))?;
        // #1858: stored as older entries are, without `\\?\`; matching stays canonical
        let plain = crate::platform::plain_path(&target);
        let target_text = plain.to_str().ok_or("project root is not UTF-8")?;
        if target_text.contains(['\n', '\r']) {
            return Err("project root cannot be represented by the line registry".into());
        }
        let entries = project_entries(original.as_deref().unwrap_or(""))?;
        let removed: Vec<PathBuf> = entries
            .iter()
            .filter(|(_, canonical)| *canonical == target)
            .map(|(path, _)| path.clone())
            .collect();
        let mut next = original.clone().unwrap_or_default();
        if adding && removed.is_empty() {
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(target_text);
            next.push('\n');
        } else if !adding && !removed.is_empty() {
            next = entries
                .iter()
                .filter(|(_, canonical)| *canonical != target)
                .map(|(path, _)| format!("{}\n", path.display()))
                .collect();
        }
        Ok(Self {
            install: install.into(),
            original,
            next,
            removed: if removed.is_empty() {
                vec![target]
            } else {
                removed
            },
            adding,
            _lock: lock,
        })
    }

    #[tracing::instrument(name = "host.projects.commit", skip_all)]
    pub fn commit(self) -> Res<()> {
        let path = self.install.join("projects");
        let write = || {
            if self.original.as_deref() != Some(self.next.as_str()) {
                match &self.original {
                    Some(original) => replace_owned(&path, original, &self.next)?,
                    None => coreq::write_durable(&path, &self.next)?,
                }
            }
            if fs::read_to_string(&path).map_err(|e| e.to_string())? != self.next {
                return Err("project registry write unknown: readback differs".into());
            }
            Ok(())
        };
        if self.adding {
            crate::resources::locked(&self.install, write)?;
        } else {
            crate::resources::unregister(&self.install, &self.removed, write)?;
        }
        crate::platform::write_atomic(&self.install.join("poke"), "")
            .map_err(|e| format!("project registry saved; host notification unknown: {e}"))
    }
}

#[path = "host_install.rs"]
mod installation;
pub use installation::cancel_install;
pub use installation::install_acknowledgement;
pub use installation::install_project;
pub use installation::installation_status;

fn installation_observed(service: &Value, status: Option<&Value>, version: &str) -> bool {
    service["state"] == "running"
        && service["fresh"] == true
        && service["version"] == version
        && service["pid"].as_u64().is_some_and(|pid| pid > 0)
        && service["instance"]
            .as_str()
            .is_some_and(crate::resources::id)
        && status.is_some_and(|status| {
            status["pid"] == service["pid"]
                && status["host_instance"] == service["instance"]
                && crate::snapshot::complete(status)
                && status["error"].is_null()
                && status["focus_error"].is_null()
        })
}

#[derive(Default)]
struct Registry {
    roots: Vec<PathBuf>,
    poke: Option<std::time::SystemTime>,
    checked: Option<u64>,
    error: Option<String>,
}
impl Registry {
    fn refresh(&mut self, install: &Path, now: u64) {
        let poke = fs::metadata(install.join("poke"))
            .and_then(|m| m.modified())
            .ok();
        if self.checked.is_some_and(|t| now.saturating_sub(t) < 60) && self.poke == poke {
            return;
        }
        self.poke = poke;
        self.checked = Some(now);
        match projects(install) {
            Ok(roots) => {
                self.roots = roots;
                self.error = None;
            }
            // A host that never had a registry has zero projects (setup, § 4); one that loses it reads unknown.
            Err(_) if self.roots.is_empty() && matches!(project_text(install), Ok(None)) => {
                self.error = None;
            }
            Err(e) => {
                if self.error.as_ref() != Some(&e) {
                    crate::platform::log(&e);
                }
                self.error = Some(e);
            }
        }
    }
}

/// #1419: a failed candidate
/// selftest logs once per version, not again for each pass's varying flush counts.
fn maintenance_once(last: &mut String, text: &str) -> bool {
    let key = |t: &str| t.find(" selftest failed").unwrap_or(t.len());
    let changed = last[..key(last)] != text[..key(text)];
    if changed {
        text.clone_into(last);
    }
    changed
}

#[cfg(unix)]
pub(crate) fn ownership(path: &Path) -> Res<Value> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    if m.uid() != unsafe { libc::geteuid() } {
        return Err("cannot preserve another user's file ownership".into());
    }
    Ok(json!({"uid": m.uid(), "gid": m.gid()}))
}
#[cfg(unix)]
fn stage_owner(path: &Path, owner: &Value) -> Res<()> {
    use std::os::unix::ffi::OsStrExt;
    let p = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let uid = owner["uid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("file owner unknown")?;
    let gid = owner["gid"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("file group unknown")?;
    if unsafe { libc::chown(p.as_ptr(), uid, gid) } != 0 {
        return Err(format!(
            "cannot preserve ownership: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(unix)]
fn replace_project(path: &Path, stage: &Path) -> Res<()> {
    fs::rename(stage, path).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub(crate) fn ownership(path: &Path) -> Res<Value> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetFileSecurityW(
            path: *const u16,
            info: u32,
            descriptor: *mut u8,
            size: u32,
            needed: *mut u32,
        ) -> i32;
    }
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut size = 0;
    unsafe {
        GetFileSecurityW(path.as_ptr(), 7, std::ptr::null_mut(), 0, &mut size);
    }
    if size == 0 {
        return Err(format!(
            "file ownership unknown: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut descriptor = vec![0; size as usize];
    if unsafe { GetFileSecurityW(path.as_ptr(), 7, descriptor.as_mut_ptr(), size, &mut size) } == 0
    {
        return Err(format!(
            "file ownership unknown: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SetFileSecurityW clears AUTO_INHERITED bookkeeping even with byte-identical owner, group and DACL.
    *descriptor
        .get_mut(3)
        .ok_or("file ownership descriptor truncated")? &= !0x04;
    Ok(json!(descriptor))
}
#[cfg(windows)]
fn stage_owner(path: &Path, owner: &Value) -> Res<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn SetFileSecurityW(path: *const u16, info: u32, descriptor: *const u8) -> i32;
    }
    let descriptor: Vec<u8> = owner
        .as_array()
        .ok_or("file ownership unknown")?
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or("file ownership corrupt")
        })
        .collect::<Result<_, _>>()?;
    // Self-relative SID slices avoid native pointer reads from a retained migration manifest.
    fn sid(descriptor: &[u8], field: usize) -> Res<&[u8]> {
        if descriptor.len() < 20 || descriptor[0] != 1 || descriptor[3] & 0x80 == 0 {
            return Err("file ownership descriptor corrupt".into());
        }
        let offset = u32::from_le_bytes(descriptor[field..field + 4].try_into().unwrap()) as usize;
        if offset == 0 {
            return Ok(&[]);
        }
        let header = descriptor
            .get(offset..offset.saturating_add(8))
            .filter(|s| offset >= 20 && s[0] == 1 && s[1] <= 15)
            .ok_or("file ownership SID corrupt")?;
        descriptor
            .get(offset..offset.saturating_add(8 + 4 * header[1] as usize))
            .ok_or_else(|| "file ownership SID truncated".into())
    }
    let current = ownership(path)?;
    if &current == owner {
        return Ok(());
    }
    let current: Vec<u8> = serde_json::from_value(current).map_err(|_| "file ownership corrupt")?;
    let mut information = 4;
    for (flag, field) in [(1, 4), (2, 8)] {
        if sid(&descriptor, field)? != sid(&current, field)? {
            information |= flag;
        }
    }
    let control = descriptor.get(2..4).ok_or("file ownership corrupt")?;
    let protected = u16::from_le_bytes([control[0], control[1]]) & 0x1000 != 0;
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            information | if protected { 0x80000000 } else { 0x20000000 },
            descriptor.as_ptr(),
        )
    } == 0
    {
        return Err(format!(
            "cannot preserve ownership: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(windows)]
fn replace_project(path: &Path, stage: &Path) -> Res<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            path: *const u16,
            stage: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    if unsafe {
        ReplaceFileW(
            wide(path).as_ptr(),
            wide(stage).as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!("replace: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Per-project restart/quarantine clock. Children stay outside the unwind boundary in the thread's Service.
#[derive(Default)]
pub struct Panics {
    times: Vec<u64>,
    pub until: u64,
    pub reason: Option<String>,
}
impl Panics {
    pub fn quarantined(&self) -> bool {
        self.times.len() >= 3
    }
    pub fn record(&mut self, now: u64, msg: &str) {
        self.times.retain(|t| now.saturating_sub(*t) <= 600);
        self.times.push(now);
        let class = crate::incident::panic_class(msg);
        let delay = [1, 10, 60][self.times.len().min(3) - 1];
        self.until = now + delay;
        self.reason = Some(if self.quarantined() {
            format!("quarantined: {class}")
        } else {
            format!("{class}; restarting in {delay} s")
        });
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
