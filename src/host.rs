//! Host policy, occupied slots and project supervision (taskq.md § 3, § 4, § 9).
use crate::{Res, service::{self, Capacity, Gate}};
use serde_json::{Value, json};
use coreq::telemetry::capture::CommandReceipt;
use std::{collections::{BTreeMap, BTreeSet}, fs, path::{Path, PathBuf}, sync::{Arc, Mutex}, time::Instant};

thread_local!(static ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
const CAPACITY_COMMAND:&str="_taskq_capacity_command";
pub fn active() -> bool { ACTIVE.get() }
pub fn enter(root: &Path, install: &Path) {
    ACTIVE.set(true);
    crate::PROJECT_ROOT.set(Some(root.into()));
    crate::platform::project_log(project_log(install, root));
}

/// host.json `default_runtime` values (§ 9); `codex-app` stays opt-in per task.
pub const DEFAULT_RUNTIMES: [&str; 3] = crate::runtime::CLI_RUNTIMES;

/// §4/§9: aliases preserve existing policy while extending task admission, not manager turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClaimMode { #[default] Any, Spillover, Labelled, None }
impl ClaimMode {
    pub fn parse(s: &str) -> Res<Self> {
        match s {
            "primary" | "any" => Ok(Self::Any), "spillover" => Ok(Self::Spillover),
            "pinned-only" | "labelled" => Ok(Self::Labelled), "manager-only" | "none" => Ok(Self::None),
            _ => Err("claim must be primary/any, spillover, pinned-only/labelled or manager-only/none".into()),
        }
    }
    pub fn name(self) -> &'static str {
        match self { Self::Any => "any", Self::Spillover => "spillover", Self::Labelled => "labelled", Self::None => "none" }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub raw: Value,
    pub cap: Capacity,
    pub me: String,
    pub has: Vec<String>,
    /// Task-claim mode (§4/§9); aliases preserve any/labelled policies.
    pub claim: ClaimMode,
    pub spillover_grace: u64,
    /// `default_runtime`: reviewers and workers without `run-*` run on it (§ 4, § 9).
    pub default_runtime: String,
    pub codex: Vec<String>,
    pub permission_mode: String,
    pub update_every: u64,
    pub rollback_after: u64,
}

impl Policy {
    pub fn apply(&self, cfg: &mut service::Config) {
        cfg.me = self.me.clone(); cfg.has = self.has.clone(); cfg.claim = self.claim; cfg.spillover_grace = self.spillover_grace; cfg.codex = self.codex.clone();
        let overlay = self.overlay(&cfg.root).map(|(_,p)|p);
        cfg.default_runtime = overlay.and_then(|p|p["default_runtime"].as_str()).unwrap_or(&self.default_runtime).into();
        cfg.allowed_runtimes = overlay.and_then(|p|p["allowed_runtimes"].as_array()).map(|a|a.iter().map(|rt|rt.as_str().unwrap().to_string()).collect());
        cfg.auto_reroute = overlay.is_some_and(|p|p["auto_reroute"]==true);
        cfg.permission_mode = self.permission_mode.clone(); cfg.update_every = self.update_every; cfg.rollback_after = self.rollback_after;
    }
    pub fn parse(v: Value, hostname: &str) -> Res<Self> {
        if !v.is_object() || !v["capacity_hosts"].is_null() || !v["admission"]["mode"].is_null() {
            return Err("host.json: expected host policy, without capacity_hosts or mode".into());
        }
        if !matches!(v["admission"], Value::Null | Value::Object(_)) { return Err("host.json: admission must be an object".into()); }
        for key in ["hosts", "has"] {
            if !matches!(v[key], Value::Null | Value::Object(_)) { return Err(format!("host.json: {key} must be an object")); }
        }
        if let Some(m) = v["hosts"].as_object() {
            if m.values().any(|v| v.as_str().is_none_or(str::is_empty)) { return Err("host.json: invalid host mapping".into()); }
        }
        let strings = |v: &Value, key: &str| -> Res<Vec<String>> {
            v.as_array().ok_or(format!("host.json: {key} must be an array"))?.iter()
                .map(|s| s.as_str().map(String::from).ok_or(format!("host.json: {key} must contain strings"))).collect()
        };
        if let Some(m) = v["has"].as_object() { for x in m.values() { strings(x, "has")?; } }
        let me = std::env::var("TASKQ_HOST").ok().filter(|s| !s.is_empty())
            .or_else(|| v["hosts"][hostname].as_str().map(String::from)).unwrap_or_else(|| hostname.into());
        let has = if v["has"][&me].is_null() { vec![] } else { strings(&v["has"][&me], "has")? };
        let codex = if v[crate::runtime::CODEX_ARGS].is_null() { vec!["-s".into(), "danger-full-access".into()] } else { strings(&v[crate::runtime::CODEX_ARGS], crate::runtime::CODEX_ARGS)? };
        let permission_mode = match &v["permission_mode"] {
            Value::Null => "dontAsk".into(), Value::String(s) if !s.is_empty() => s.clone(),
            _ => return Err("host.json: permission_mode must be a nonempty string".into()),
        };
        let claim = match &v["claim"] {
            Value::Null => ClaimMode::Any,
            Value::String(s) => ClaimMode::parse(s).map_err(|e|format!("host.json: {e}"))?,
            _ => return Err("host.json: claim must be a string".into()),
        };
        let spillover_grace = if v["spillover_grace"].is_null() {300} else {v["spillover_grace"].as_u64().filter(|n|*n<=i64::MAX as u64).ok_or("host.json: spillover_grace must be nonnegative seconds")?};
        let default_runtime = match v["default_runtime"].as_str() {
            _ if v["default_runtime"].is_null() => crate::runtime::OWNER.to_string(),
            Some(rt) if DEFAULT_RUNTIMES.contains(&rt) => rt.to_string(),
            _ => return Err(format!("host.json: default_runtime must be {}", DEFAULT_RUNTIMES.join(" or "))),
        };
        let timer = |k: &str, d| if v[k].is_null() { Ok(d) } else { v[k].as_u64().filter(|n| *n > 0).ok_or(format!("host.json: {k} must be positive seconds")) };
        let (update_every, rollback_after) = (timer("update_every", 60)?, timer("rollback_after", 600)?);
        crate::platform::channel_of(&v)?;
        let mut c = v.clone();
        if c["admission"].is_null() { c["admission"] = json!({}); }
        c["admission"]["mode"] = json!("auto");
        // The host policy allows the owner to disable the gate, without a Fixed-mode bypass.
        let off = c["load_gate"]["enabled"] == false;
        if off { c["load_gate"]["enabled"] = json!(true); }
        let mut cap = service::capacity(&c, &me)?;
        if off { cap.gate.enabled = false; }
        // The project overlay only lowers: a count above the host ceiling is invalid policy, never clamped.
        if !matches!(v["projects"], Value::Null | Value::Object(_)) { return Err("host.json: projects must be an object".into()); }
        for (root, p) in v["projects"].as_object().into_iter().flatten() {
            let bad = || format!("host.json: projects.{root}: invalid limits or execution policy");
            let o = p.as_object().filter(|o| o.keys().all(|k| ["limits","op","default_runtime","allowed_runtimes","auto_reroute"].contains(&k.as_str()))
                && ["limits","default_runtime","allowed_runtimes","auto_reroute"].iter().any(|k|o.contains_key(*k))).ok_or_else(bad)?;
            if o.get("default_runtime").is_some_and(|rt|!rt.as_str().is_some_and(|rt|DEFAULT_RUNTIMES.contains(&rt))) {return Err(bad());}
            if o.get("auto_reroute").is_some_and(|b|!b.is_boolean()) {return Err(bad());}
            if let Some(allowed)=o.get("allowed_runtimes") {
                let a=allowed.as_array().ok_or_else(bad)?;
                let mut seen=BTreeSet::new();
                for rt in a {if !rt.as_str().is_some_and(|rt|service::RUNTIMES.contains(&rt) && seen.insert(rt)) {return Err(bad());}}
            }
            if o.get("op").is_some_and(|x| !x.as_str().is_some_and(crate::resources::id)) { return Err(bad()); }
            if o.get("limits").is_some_and(|v|!v.is_object()) {return Err(bad());}
            for (rt, n) in p["limits"].as_object().into_iter().flatten() {
                let n = n.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|_| service::RUNTIMES.contains(&rt.as_str())).ok_or_else(bad)?;
                if cap.limits.get(rt).is_some_and(|host| n > *host) { return Err(format!("host.json: projects.{root}.limits.{rt} raises host ceiling")); }
            }
        }
        let policy=Self { raw: v, cap, me, has, claim, spillover_grace, default_runtime, codex, permission_mode, update_every, rollback_after };
        policy.command()?;
        Ok(policy)
    }

    fn command(&self)->Res<Option<CommandReceipt>> {
        let Some(value)=self.raw.get(CAPACITY_COMMAND) else {return Ok(None)};
        let invalid="host capacity command unsupported or corrupt; file kept";
        let object=value.as_object().ok_or(invalid)?;
        if object.len()!=4+usize::from(object.contains_key("request")) || !["schema","host","policy","receipt"].iter().all(|key|object.contains_key(*key))
            || value["schema"]!=1 || value["host"].as_str().is_none_or(str::is_empty) {
            return Err(invalid.into());
        }
        if object.contains_key("request") && !value["request"].as_str().is_some_and(crate::resources::id) {return Err(invalid.into());}
        let snapshot=value["policy"].as_object().ok_or(invalid)?;
        if snapshot.len()!=3 || !["limits","admission","load_gate"].iter().all(|key|snapshot.contains_key(*key))
            || value["policy"]["admission"]["mode"]!="auto" {return Err(invalid.into());}
        let mut policy=value["policy"].clone();
        policy["admission"].as_object_mut().ok_or(invalid)?.remove("mode");
        if Self::parse(policy,&self.me)?.cap.json()!=value["policy"] {return Err(invalid.into());}
        let text=value["receipt"].as_str().ok_or(invalid)?;
        let receipt=CommandReceipt::decode(text).map_err(|_|invalid)?;
        let metadata:Value=serde_json::from_str(text).map_err(|_|invalid)?;
        if metadata["product"]!="taskq" || metadata["operation"]!="taskq.host.policy.write" {return Err(invalid.into());}
        Ok(Some(receipt))
    }

    pub fn revision(&self)->Res<String> {
        if self.raw[CAPACITY_COMMAND]["host"]==self.me && self.raw[CAPACITY_COMMAND]["policy"]==self.cap.json()
            && let Some(command)=self.command()? {return command.event_id("observed");}
        Ok(ring::digest::digest(&ring::digest::SHA256,&serde_json::to_vec(&json!([self.me,self.cap.json()])).map_err(|e|e.to_string())?).as_ref().iter().map(|v|format!("{v:02x}")).collect())
    }

    /// The revision a project's remote overlay operation names: the host revision, that root's overlay entry (whose
    /// `op` makes edit-and-revert a new revision) and the effective project limits, so a taskq.json `limits` edit
    /// is a new revision too. `v` is the root's taskq.json.
    pub fn project_revision(&self, root: &Path, v: &Value) -> Res<String> {
        let parts = json!([self.revision()?, self.overlay(root).map(|(_, e)| e), self.project_limits(root, v)?]);
        Ok(ring::digest::digest(&ring::digest::SHA256,&serde_json::to_vec(&parts).map_err(|e|e.to_string())?).as_ref().iter().map(|v|format!("{v:02x}")).collect())
    }

    /// The `projects` entry of a registered root: its exact spelling, else the same canonical path.
    pub(crate) fn overlay(&self, root: &Path) -> Option<(&String, &Value)> {
        let m = self.raw["projects"].as_object()?;
        m.iter().find(|(k, _)| Path::new(k.as_str()) == root)
            .or_else(|| { let c = fs::canonicalize(root).ok()?; m.iter().find(|(k, _)| fs::canonicalize(k.as_str()).is_ok_and(|x| x == c)) })
    }

    pub fn read(install: &Path) -> Res<Self> { Self::read_for_hostname(install,crate::hostname) }

    pub(crate) fn read_for_hostname(install:&Path,hostname:impl FnOnce()->String)->Res<Self> {
        match fs::symlink_metadata(install.join("host.json.pending")) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            Err(e)=>return Err(format!("host policy prepare unknown: {e}")),
            Ok(_)=>return Err("host policy interrupted write retained; admission held for reconciliation".into()),
        }
        let v = fs::read(install.join("host.json")).map_err(|e| format!("host.json unknown: {e}"))?;
        Self::parse(serde_json::from_slice(&v).map_err(|e| format!("host.json unknown: {e}"))?, &hostname())
    }

    /// The project's own limits: taskq.json `limits` and this root's host overlay, the lower of both (§ 3, § 9).
    pub fn project_limits(&self, root: &Path, v: &Value) -> Res<BTreeMap<String, u32>> {
        if !matches!(v["limits"], Value::Null | Value::Object(_)) { return Err("project limits: expected an object".into()); }
        let mut out = BTreeMap::new();
        for (rt, n) in v["limits"].as_object().into_iter().flatten() {
            let n = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(format!("project limits.{rt}: invalid count"))?;
            // The shared taskq.json cannot know each host's ceiling: it only lowers, so a higher count clamps (§ 3 Capacity).
            let n = self.cap.limits.get(rt).map_or(n, |host| n.min(*host));
            out.insert(rt.clone(), n);
        }
        for (rt, n) in self.overlay(root).and_then(|(_, e)| e["limits"].as_object()).into_iter().flatten() {
            let n = n.as_u64().unwrap_or_default() as u32;
            out.entry(rt.clone()).and_modify(|x| *x = (*x).min(n)).or_insert(n);
        }
        Ok(out)
    }

    pub fn project(&self, root: &Path, v: &Value) -> Res<Capacity> {
        let mut c = self.cap.clone();
        c.limits.extend(self.project_limits(root, v)?);
        Ok(c)
    }
}

/// Existing resource-control lock serializes policy writers with grants and migration.
pub fn write_capacity(install:&Path, host:&str, edit:&crate::CapEdit, expect:Option<&Value>)->Res<(Capacity,Capacity,CommandReceipt)> {
    crate::resources::locked(install,||write_capacity_locked(install,host,edit,expect,None))
}

#[tracing::instrument(name="host.policy.write",skip_all)]
fn write_capacity_locked(install:&Path, host:&str, edit:&crate::CapEdit, expect:Option<&Value>,mut local:Option<(&mut crate::resources::State,&Value)>)->Res<(Capacity,Capacity,CommandReceipt)> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot change host policy".into());}
        migration_done(install)?;
        let path=install.join("host.json");
        let original=fs::read_to_string(&path).map_err(|e|format!("host.json unknown: {e}"))?;
        let hostname=crate::hostname();
        let old=Policy::parse(serde_json::from_str(&original).map_err(|e|format!("host.json unknown: {e}"))?,&hostname)?;
        if host!=old.me {return Err("host policy edits require this local host; remote edits are unsupported".into());}
        if let Some((state,request))=local.as_mut() {
            let id=request["id"].as_str().ok_or("capacity request ID missing")?;
            if let Some(op)=state.ops.get_mut(id) {
                if op.bound!=json!({"local_capacity":request}) {return Err("capacity request ID reused with another payload".into());}
                if op.receipt["state"]=="rejected" {return Err(op.receipt["reason"].as_str().unwrap_or("capacity request rejected").into());}
                let command=if op.receipt["state"]=="saved" {
                    let command=CommandReceipt::decode(op.receipt["command"].as_str().ok_or("capacity receipt corrupt")?)?;
                    if op.receipt["saved_revision"]!=command.event_id("observed")? {return Err("capacity receipt corrupt".into());}
                    command
                } else if old.raw[CAPACITY_COMMAND]["request"]==id && old.raw[CAPACITY_COMMAND]["policy"]==old.cap.json() {
                    old.command()?.ok_or("capacity request outcome unknown")?
                } else {return Err("capacity request outcome unknown; mutation will not be repeated".into());};
                op.writing=false;op.sent=true;
                op.receipt=json!({"state":"saved","saved_revision":command.event_id("observed")?,"command":command.encode()});
                return Ok((old.cap.clone(),old.cap,command));
            }
            if state.ops.len()>=crate::resources::MAX_OPS {return Err("capacity operation store full; no new request taken".into());}
            let now=wall()?;
            if request["issued_at"].as_u64().is_none_or(|at|at>now.saturating_add(30)) || request["expires_at"].as_u64().is_none_or(|at|now>=at) {return Err("capacity request expired or clock unknown".into());}
            let revision=old.revision()?;
            if request["ops"].as_array().ok_or("capacity operations missing")?.iter().any(|op|op["expected_revision"]!=revision) {return Err("host policy changed; review current values before applying".into());}
        }
        if edit.mode.is_some() {return Err("host policy has no mode selector; use optional limits and the load gate".into());}
        if expect.is_some_and(|v|*v!=old.cap.json()) {return Err("host policy changed; refresh before applying".into());}
        let mut next=old.raw.clone();
        for rt in edit.limits.keys().chain(&edit.unset) {
            if !service::RUNTIMES.contains(&rt.as_str()) {return Err(format!("unknown runtime {rt}"));}
        }
        if !edit.limits.is_empty() {
            if next["limits"].is_null() {next["limits"]=json!({});}
            for (rt,n) in &edit.limits {next["limits"][rt]=json!(n);}
        }
        for rt in &edit.unset {if let Some(limits)=next["limits"].as_object_mut() {limits.remove(rt);}}
        if let Some(max)=edit.max {if next["admission"].is_null() {next["admission"]=json!({});}next["admission"]["max"]=json!(max);}
        if edit.gate.is_some()||edit.cpu_max.is_some()||edit.memory_min.is_some() {
            if next["load_gate"].is_null() {next["load_gate"]=json!({});}
            if let Some(value)=edit.gate {next["load_gate"]["enabled"]=json!(value);}
            if let Some(value)=edit.cpu_max {next["load_gate"]["cpu_max"]=json!(value);}
            if let Some(value)=edit.memory_min {next["load_gate"]["memory_min"]=json!(value);}
        }
        if let Some(mode)=edit.claim {next["claim"]=json!(mode.name());}
        if let Some(rt)=&edit.default_runtime {next["default_runtime"]=json!(rt);}
        let candidate=Policy::parse(next,&hostname)?;
        let command=CommandReceipt::current()?;
        crate::telemetry::receipt(&command,"received")?;
        let mut next=candidate.raw;
        next[CAPACITY_COMMAND]=json!({"schema":1,"host":candidate.me,"policy":candidate.cap.json(),"receipt":command.encode()});
        if let Some((state,request))=local.as_mut() {
            let id=request["id"].as_str().ok_or("capacity request ID missing")?;
            next[CAPACITY_COMMAND]["request"]=json!(id);
            state.ops.insert(id.into(),crate::resources::Op {bound:json!({"local_capacity":request}),receipt:json!({"state":"unknown"}),
                expires_at:request["expires_at"].as_u64().ok_or("capacity expiry missing")?,saved_at:None,writing:true,sent:true});
            crate::resources::save(install,state)?;
            #[cfg(test)]
            if std::env::var("TASKQ_HOST_POLICY_PHASE").is_ok_and(|phase|phase=="before-write") {
                crate::platform::write_atomic(&install.join("ack"),"intent-saved")?;loop {std::thread::park();}
            }
            if wall()?>=request["expires_at"].as_u64().ok_or("capacity expiry missing")? {
                let op=state.ops.get_mut(id).unwrap();op.writing=false;op.receipt=json!({"state":"rejected","reason":"expired"});
                return Err("capacity request expired before host policy write".into());
            }
        }
        let candidate=Policy::parse(next,&hostname)?;
        replace_owned(&path,&original,&(serde_json::to_string_pretty(&candidate.raw).map_err(|e|e.to_string())?+"\n"))?;
        let saved=Policy::read(install)?;
        if saved.raw!=candidate.raw {return Err("host policy write uncertain: readback differs".into());}
        if let Some((state,request))=local.as_mut() {
            let op=state.ops.get_mut(request["id"].as_str().unwrap()).unwrap();op.writing=false;op.saved_at=Some(wall()?);
            op.receipt=json!({"state":"saved","saved_revision":command.event_id("observed")?,"command":command.encode()});
        }
        #[cfg(test)]
        if std::env::var("TASKQ_HOST_POLICY_PHASE").is_ok_and(|phase|phase=="before-applied") {
            crate::platform::write_atomic(&install.join("ack"),"policy-saved")?;
            loop {std::thread::park();}
        }
        crate::telemetry::receipt(&command,"applied")?;
        crate::platform::write_atomic(&install.join("poke"),"").map_err(|e|format!("host policy saved; host notification unknown: {e}"))?;
        Ok((old.cap,saved.cap,command))
}

fn migration_done(install:&Path)->Res<()> {
    let migration=install.join("host-policy-migration");
    if migration.join("plan.json").try_exists().map_err(|e|e.to_string())?
        && !migration.join("complete").try_exists().map_err(|e|e.to_string())? {
        return Err("host policy migration incomplete; edits held until verified readback".into());
    }
    Ok(())
}

/// The remote project overlay writer (#1282, shared-resource-controls.md#project-overlay); the caller holds the
/// resource lock. Only `projects.<root>` changes. `Ok(Err(reason))`: refused before any write; `Err`: write uncertain.
pub(crate) fn write_project_limit(install:&Path, root:&Path, rt:&str, limit:Option<u32>, op:&str, expect:&str)->Res<Result<Policy,&'static str>> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() || migration_done(install).is_err() || !service::RUNTIMES.contains(&rt) {return Ok(Err("unsupported"));}
    let path=install.join("host.json");
    let Ok(original)=fs::read_to_string(&path) else {return Ok(Err("conflict"))};
    let hostname=crate::hostname();
    let Ok(old)=serde_json::from_str(&original).map_err(|e|e.to_string()).and_then(|v|Policy::parse(v,&hostname)) else {return Ok(Err("conflict"))};
    let Ok((_,project))=crate::load(root) else {return Ok(Err("conflict"))};
    if old.project_revision(root,&project)?!=expect {return Ok(Err("conflict"));}
    let key=old.overlay(root).map_or_else(||root.display().to_string(),|(k,_)|k.clone());
    let mut next=old.raw.clone();
    if next["projects"].is_null() {next["projects"]=json!({});}
    let entry=&mut next["projects"][&key];
    if entry.is_null() {*entry=json!({});}
    if entry["limits"].is_null() {entry["limits"]=json!({});}
    match limit {
        Some(n)=>entry["limits"][rt]=json!(n),
        None=>{entry["limits"].as_object_mut().map(|l|l.remove(rt));}
    }
    // kept when the limits empty: the restore is a new revision too
    entry["op"]=json!(op);
    let Ok(candidate)=Policy::parse(next,&hostname) else {return Ok(Err("invalid"))};
    replace_owned(&path,&original,&(serde_json::to_string_pretty(&candidate.raw).map_err(|e|e.to_string())?+"\n"))?;
    let saved=Policy::read(install)?;
    if saved.raw!=candidate.raw {return Err("host policy write uncertain: readback differs".into());}
    Ok(Ok(saved))
}

fn wall()->Res<u64> {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs()).map_err(|_|"capacity clock unknown".into())}

/// Typed native request; OS-owned local controls do not grant the remote exchange any authority.
pub fn capacity_request(request:&Value)->Res<crate::CapEdit> {
    use crate::resources::{keys,count,id};
    let invalid="invalid host capacity request";
    if !keys(request,&["id","issued_at","expires_at","ops"]) || !request["id"].as_str().is_some_and(id)
        || serde_json::to_vec(request).map_err(|e|e.to_string())?.len()>16<<10 {return Err(invalid.into());}
    let issued=request["issued_at"].as_u64().ok_or(invalid)?;let expires=request["expires_at"].as_u64().ok_or(invalid)?;
    if expires<=issued || expires-issued>300 {return Err(invalid.into());}
    let ops=request["ops"].as_array().filter(|ops|!ops.is_empty()&&ops.len()<=4).ok_or(invalid)?;
    let mut seen=BTreeSet::new();let mut edit=crate::CapEdit::default();
    for op in ops {
        if !keys(op,&["target","policy_schema","expected_revision","changes"]) || op["policy_schema"]!="taskq.host-capacity/1"
            || !op["expected_revision"].as_str().is_some_and(id) || !keys(&op["target"],&["installation_id","generation","project_id","runtime_id"])
            || op["target"]["generation"]!=1 || !op["target"]["project_id"].is_null() {return Err(invalid.into());}
        let rt=op["target"]["runtime_id"].as_str();
        if (!op["target"]["runtime_id"].is_null() && rt.is_none_or(|rt|!service::RUNTIMES.contains(&rt))) || !seen.insert(rt) {return Err(invalid.into());}
        let changes=op["changes"].as_object().filter(|c|!c.is_empty()).ok_or(invalid)?;
        for (key,value) in changes {
            match key.as_str() {
                "runtime_limit" if rt.is_some() && (value.is_null()||count(value))=> {
                    let rt=rt.unwrap().to_string();if value.is_null() {edit.unset.insert(rt);}else {edit.limits.insert(rt,value.as_u64().unwrap() as u32);}
                }
                "maximum" if rt.is_none() && (value.is_null()||count(value))=>edit.max=Some(value.as_u64().map(|n|n as u32)),
                "load_gate" if rt.is_none() && keys(value,&["enabled","cpu_max","memory_min"]) && value["enabled"].is_boolean()
                    && value["cpu_max"].as_u64().is_some_and(|n|(10..=100).contains(&n)) && value["memory_min"].as_u64().is_some_and(|n|(1..=95).contains(&n))=> {
                        edit.gate=value["enabled"].as_bool();edit.cpu_max=value["cpu_max"].as_u64().map(|n|n as u32);edit.memory_min=value["memory_min"].as_u64().map(|n|n as u32);
                    }
                _=>return Err(invalid.into()),
            }
        }
    }
    Ok(edit)
}

// #1351: the tray's limits save fails fast while another host writer (grant, migration, DashQ) holds the lock.
#[cfg(not(test))] const LOCAL_LOCK_WAIT:std::time::Duration=std::time::Duration::from_secs(10);
#[cfg(test)] const LOCAL_LOCK_WAIT:std::time::Duration=std::time::Duration::from_secs(1);

pub fn local_capacity(install:&Path,host:&str,request:&Value)->Res<Value> {
    let edit=capacity_request(request)?;
    let receipt=crate::resources::with_within(install,LOCAL_LOCK_WAIT,|state| {
        crate::resources::prepare(state,install,wall()?)?;
        let installation=match fs::read_to_string(install.join("installation-id")) {
            Ok(id) if id.len()==64 && id.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))=>json!(id),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Value::Null,
            _=>return Err("local installation identity unknown".into()),
        };
        if request["ops"].as_array().unwrap().iter().any(|op|op["target"]["installation_id"]!=installation) {return Err("capacity request belongs to another installation".into());}
        let result=write_capacity_locked(install,host,&edit,None,Some((state,request)));
        let mut receipt=state.ops.get(request["id"].as_str().unwrap()).filter(|op|op.bound==json!({"local_capacity":request})).map(|op|op.receipt.clone()).unwrap_or(json!({"state":"rejected"}));
        if let Err(error)=result {receipt["reason"]=json!(error);}
        receipt.as_object_mut().ok_or("local capacity receipt corrupt")?.remove("command");
        Ok(receipt)
    })?;
    Ok(receipt.unwrap_or_else(||json!({"state":"rejected","reason":"another host settings change is in progress; old limits kept. Try again in a minute"})))
}

/// The host already loaded this snapshot; a later writer cannot borrow its observation.
pub fn observe_capacity(install:&Path,loaded:&Policy)->Res<Option<String>> {
    let Some(command)=loaded.command()? else {return Ok(None)};
    coreq::telemetry::capture::operation("host.policy.observe",||crate::resources::try_locked(install,|| {
        let current=Policy::read(install)?;
        let binding=&loaded.raw[CAPACITY_COMMAND];
        if current.raw!=loaded.raw || binding["host"]!=loaded.me || binding["policy"]!=loaded.cap.json() {
            return Err("capacity command changed or no longer matches the loaded policy; observation unknown".into());
        }
        crate::telemetry::receipt(&command,"applied")?;
        crate::telemetry::receipt(&command,"observed").map(Some)
    }),|_|"command-observation")
}

/// Read the exact durable acknowledgment, never infer it from equal values or wall-clock freshness.
pub fn capacity_observed(install:&Path,command:&CommandReceipt)->Res<bool> {
    let current=Policy::read(install)?;
    if current.command()?.is_none_or(|saved|saved.encode()!=command.encode())
        || current.raw[CAPACITY_COMMAND]["host"]!=current.me || current.raw[CAPACITY_COMMAND]["policy"]!=current.cap.json() {
        return Err("host policy saved; command superseded or manually changed".into());
    }
    let metadata:Value=serde_json::from_str(&command.encode()).map_err(|e|e.to_string())?;
    let id=command.event_id("observed")?;
    let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
    let page=reader.export_kinds(&["command.observed"],metadata["started_ms"].as_i64().ok_or("capacity command time unknown")?,0,1024)?;
    let found=page["events"].as_array().ok_or("capacity receipt page invalid")?.iter().any(|event|event["id"]==id);
    if !found && page["more"]==true {return Err("capacity receipt page limit; observation unknown".into());}
    Ok(found)
}

/// Reuse migration's native replacement so a settings edit preserves owner/group/ACL and file permissions.
fn replace_owned(path:&Path, original:&str, next:&str)->Res<()> {
    use std::io::Write;
    let metadata=fs::symlink_metadata(path).map_err(|e|e.to_string())?;
    if !metadata.is_file() || metadata.permissions().readonly() {return Err("state file is not a writable regular file; kept".into());}
    let owner=ownership(path)?;
    let stage=path.with_file_name(&{let mut name=path.file_name().ok_or("state file name missing")?.to_os_string();name.push(".pending");name});
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&stage)
        .map_err(|e|format!("state file prepare refused; interrupted state kept: {e}"))?;
    let result=(|| {
        stage_owner(&stage,&owner)?;
        file.set_permissions(metadata.permissions()).map_err(|e|e.to_string())?;
        file.write_all(next.as_bytes()).map_err(|e|e.to_string())?;
        file.sync_all().map_err(|e|e.to_string())?;
        drop(file);
        if fs::read_to_string(path).map_err(|e|e.to_string())?!=original || ownership(path)?!=owner {
            return Err("state file changed while saving; current file kept".into());
        }
        replace_project(path,&stage)?;
        #[cfg(windows)] stage_owner(path,&owner)?;
        #[cfg(unix)] fs::File::open(path.parent().ok_or("state file parent missing")?).and_then(|f|f.sync_all()).map_err(|e|e.to_string())?;
        if ownership(path)?!=owner {return Err("state file ownership readback differs".into());}
        Ok(())
    })();
    if result.is_err() && let Err(cleanup)=fs::remove_file(&stage) && cleanup.kind()!=std::io::ErrorKind::NotFound {
        return Err(format!("state file write uncertain: {}; temporary cleanup failed: {cleanup}",result.unwrap_err()));
    }
    result.map_err(|e:String|format!("{} write uncertain: {e}",path.display()))
}

// The atomic registry publisher reserves projects.pending for its own bytes.
pub(crate) const INSTALL_PENDING:&str="projects.installing";

pub type Shared = Arc<Mutex<Host>>;
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
enum Claim {Task(u64,u64),External(String)}
type Slot = (PathBuf, Claim);
fn slot(project: &Path, task: u64, attempt: u64) -> Slot {
    // Before claim there is no fencing token; only that provisional slot uses the issue number.
    (project.into(), Claim::Task(if attempt == 0 { task } else { 0 }, attempt))
}
pub enum Maintenance {Update(Option<String>),Installation(String)}

pub struct Host {
    pub instance: String,
    pub policy: Res<Policy>,
    pub capacity_command:Option<String>,
    slots: BTreeMap<Slot, String>,
    pub gate: Gate,
    began: Instant,
    last: Option<u64>,
    /// Registered ready projects, rotated only after a grant.
    ready: BTreeSet<PathBuf>,
    external_ready: BTreeMap<String,PathBuf>,
    turn: Option<PathBuf>,
    pub held: Option<String>,
    pub restart: bool,
    /// Projects were woken for this handover; later passes keep `poll` (#1357).
    restart_woken: bool,
    pub stopping: bool,
    pub stop_controller:Option<crate::host_control::Control>,
    /// Each thread reports active turns/children before the host considers an image switch.
    pub busy: BTreeMap<PathBuf, bool>,
    /// The last finished pass of each project reported its handover prepared; `passing` projects have not finished.
    pub prepared: BTreeSet<PathBuf>,
    passing: BTreeSet<PathBuf>,
    pub recovered: BTreeSet<PathBuf>,
    pub updating:Option<std::thread::JoinHandle<Res<Maintenance>>>,
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
}

impl Host {
    pub fn new(policy: Res<Policy>) -> Self {
        Self { instance:crate::runtime::uuid(),policy, capacity_command:None, slots: BTreeMap::new(), gate: Gate::default(), began: Instant::now(), last: None,
            ready: BTreeSet::new(), external_ready:BTreeMap::new(), turn: None, held: None, restart: false, restart_woken:false, stopping:false,stop_controller:None,busy: BTreeMap::new(), prepared: BTreeSet::new(), passing: BTreeSet::new(), recovered:BTreeSet::new(),updating:None, draining: BTreeSet::new(),
            request_since: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()), hour_start: crate::requests::totals(), last_hour: None,
            hosts_start: crate::requests::host_totals(), hosts_last_hour: None,
            rebuilding: false, rebuilt: BTreeSet::new(), published: BTreeMap::new() }
    }

    pub fn reload(&mut self, policy: Res<Policy>) {
        if self.policy!=policy {self.capacity_command=None;}
        if let Ok(p) = &policy { self.gate.configure(p.cap.gate); }
        self.policy = policy;
    }

    pub(crate) fn reload_admission(&mut self, policy: Res<Policy>, held: Option<String>, roots: &[PathBuf]) {
        let was_blocked=self.policy.is_err()||self.held.is_some();
        self.reload(policy);
        self.held=held;
        if was_blocked&&self.policy.is_ok()&&self.held.is_none() {
            for root in roots {
                if let Err(error)=crate::poke(root) {crate::platform::log(&error);}
            }
        }
    }

    /// A newly attached project may carry old occupancy, even if its root was drained earlier.
    fn attach(&mut self,root:&Path) {
        self.busy.insert(root.into(),true);
        self.prepared.remove(root);self.passing.remove(root);self.recovered.remove(root);self.rebuilt.remove(root);
        self.rebuilding=true;
    }

    fn detached(&mut self,root:&Path) {
        self.busy.remove(root);self.prepared.remove(root);self.recovered.remove(root);self.rebuilt.remove(root);self.ready.remove(root);
        self.rebuilding=self.rebuilding&&!self.busy.keys().all(|root|self.rebuilt.contains(root));
    }

    pub fn sync(&mut self, project: &Path, held: Vec<crate::admission::Res>) -> usize {
        self.slots.retain(|(p, claim), _| p != project || matches!(claim,Claim::External(_)));
        for (n, a, rt) in held { self.slots.insert(slot(project, n, a), rt); }
        self.slots.len()
    }

    /// A poisoned table is unusable until every active project reports its still-owned handles and pending grants.
    pub fn rebuild_slots(&mut self, project: &Path, held: Vec<crate::admission::Res>) -> bool {
        if !self.rebuilding {
            self.rebuilding = true; self.rebuilt.clear(); self.slots.retain(|(_,claim),_|matches!(claim,Claim::External(_))); self.ready.clear();
            self.gate = Gate::default(); self.last = Some(self.elapsed());
        }
        self.sync(project, held);
        self.rebuilt.insert(project.into());
        let complete = self.busy.keys().all(|root| self.rebuilt.contains(root));
        if complete { self.rebuilding = false; }
        complete
    }

    pub fn ready(&mut self, project: &Path, ready: bool) {
        if ready { self.ready.insert(project.into()); } else { self.ready.remove(project); }
    }

    pub fn publication(&mut self, project: &Path, qualified: bool) {
        if qualified { self.published.insert(project.into(), Instant::now()); } else { self.published.remove(project); }
    }

    pub fn publications_ready(&self, roots: &[PathBuf]) -> bool {
        self.policy.is_ok() && self.held.is_none() && !self.rebuilding && roots.iter().all(|root|
            self.published.get(root).is_some_and(|at| at.elapsed().as_secs() < 60))
    }

    pub fn handoff_ready(&self) -> bool {self.handoff_settled()&&self.passing.is_empty()}

    /// The handover waits only for passes in flight: no project starts another one (#1402).
    pub(crate) fn handoff_settled(&self) -> bool {
        self.restart && !self.rebuilding && self.held.is_none()&&self.updating.is_none()
            &&(!self.stopping||(self.slots.is_empty()&&self.busy.values().all(|busy|!*busy)))
            && self.busy.keys().all(|root|self.prepared.contains(root))
            && self.slots.keys().all(|(root,_)|self.prepared.contains(root))
    }

    pub fn handoff_wait(&self)->Option<crate::lifecycle::HandoffWait> {
        use crate::lifecycle::HandoffWait::*;
        if self.handoff_ready() {None}
        else if self.stopping&&!self.slots.is_empty() {Some(Execution)}
        else if self.rebuilding||self.held.is_some() {Some(Recovery)}
        else if self.stopping&&self.busy.values().any(|busy|*busy)
            ||self.busy.keys().chain(self.slots.keys().map(|(root,_)|root)).any(|root|!self.prepared.contains(root))
            ||self.updating.is_none()&&!self.passing.is_empty() {Some(Project)}
        else if self.updating.is_some() {Some(Maintenance)}
        else {None}
    }

    /// #1357: a pending handover wakes each project once. Waking every host second made every project list the
    /// board every second; each pass already reports its preparation, so the handover follows within one `poll`.
    pub fn restart_wake(&mut self) -> bool {
        self.restart && !std::mem::replace(&mut self.restart_woken,true)
    }

    /// A drained Stop project cannot revoke preparation while other projects are still draining. #1402: a pass
    /// keeps the project's last report until it ends; once every report is prepared no new pass starts, so six
    /// projects whose passes overlap still reach a moment with none in flight.
    pub fn begin_pass(&mut self, root:&Path) -> bool {
        // A pass that never reported (an error or panic) leaves no acknowledgment.
        if self.passing.remove(root) {self.prepared.remove(root);}
        if self.handoff_settled() || self.stopping && self.prepared.contains(root)
            && self.busy.get(root)==Some(&false) && !self.slots.keys().any(|(project,_)|project==root) {return false;}
        self.passing.insert(root.into());
        self.recovered.remove(root);
        self.busy.insert(root.into(),true);
        true
    }

    /// The pass's handover report; `prepared` only when nothing it owns blocks the re-exec.
    pub fn end_pass(&mut self, root:&Path, prepared:bool) {
        self.passing.remove(root);
        if prepared {self.prepared.insert(root.into());} else {self.prepared.remove(root);}
    }

    /// The § 3 grant under the host policy.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(&mut self, project: &Path, n: u64, a: u64, rt: &str, cap: &Capacity, queue: bool, now: u64) -> Res<usize> {
        self.reserve_key(slot(project,n,a),rt,cap,queue,now)
    }

    fn reserve_key(&mut self,key:Slot,rt:&str,cap:&Capacity,queue:bool,now:u64)->Res<usize> {
        let project=&key.0;
        let p = self.policy.clone()?;
        if self.rebuilding {return Err("host occupancy recovery pending".into());}
        if let Some(reason) = &self.held { return Err(reason.clone()); }
        if self.draining.contains(project) && queue { return Err("project draining".into()); }
        if self.restart { return Err("host restart pending".into()); }
        if self.slots.contains_key(&key) {
            if p.cap.limits.get(rt) == Some(&0) || cap.limits.get(rt) == Some(&0) { return Err("runtime disabled".into()); }
            return Ok(self.slots.len());
        }
        let ready:BTreeSet<_>=self.ready.iter().chain(self.external_ready.values()).collect();
        if queue && !ready.is_empty() {
            let next = ready.iter().copied().find(|p| self.turn.as_ref().is_none_or(|t| *p > t)).or_else(|| ready.first().copied());
            if next.is_some_and(|p| p != project) { return Err("round-robin: another project is ready".into()); }
        }
        if let Some(left)=self.last.and_then(|t|30u64.checked_sub(now.saturating_sub(t))).filter(|left|*left>0) {
            return Err(format!("host load gate: 30 s launch spacing ({left} s remaining)"));
        }
        if !self.gate.may_launch(now) {
            let (state,reasons)=self.gate.state(now);
            let reason=if reasons.is_empty() {"waiting for post-launch sample".into()}else{reasons.into_iter().map(|(_,text)|text).collect::<Vec<_>>().join("; ")};
            return Err(format!("host load gate {state}: {reason}"));
        }
        let used = self.slots.values().filter(|r| *r == rt).count();
        let own = self.slots.iter().filter(|((root, _), r)| root == project && *r == rt).count();
        if p.cap.max.is_some_and(|m| self.slots.len() >= m as usize) {return Err("host aggregate capacity full".into());}
        if p.cap.limits.get(rt).is_some_and(|m| used >= *m as usize) {return Err(format!("{rt} host capacity full"));}
        if cap.limits.get(rt).is_some_and(|m| own >= *m as usize) {return Err(format!("{rt} project capacity full"));}
        if queue { self.turn = Some(project.into()); }
        self.slots.insert(key, rt.into());
        self.last = Some(now);
        self.gate.launch(now);
        Ok(self.slots.len())
    }

    pub(crate) fn external_held(&self,id:&str)->bool {
        self.slots.keys().any(|(_,claim)|matches!(claim,Claim::External(key) if key==id))
    }

    pub(crate) fn external_known(&self,known:&BTreeSet<String>)->bool {
        self.slots.keys().all(|(_,claim)|match claim {Claim::External(id)=>known.contains(id),_=>true})
    }

    pub(crate) fn external_sync(&mut self, id:&str, project:&Path, runtime:&str, held:bool) {
        self.external_ready.remove(id);
        let key=(project.into(),Claim::External(id.into()));
        if held {self.slots.insert(key,runtime.into());}else{self.slots.remove(&key);}
    }

    pub(crate) fn external_reset_ready(&mut self) {self.external_ready.clear();}

    pub(crate) fn external_reserve(&mut self,id:&str,project:&Path,runtime:&str,cap:&Capacity,now:u64)->Res<()> {
        self.external_ready.insert(id.into(),project.into());
        self.reserve_key((project.into(),Claim::External(id.into())),runtime,cap,true,now)?;
        self.external_ready.remove(id);
        Ok(())
    }

    pub fn claimed(&mut self, project: &Path, n: u64, a: u64) {
        if let Some(rt) = self.slots.remove(&slot(project, n, 0)) { self.slots.insert(slot(project, n, a), rt); }
    }

    pub fn remaining(&self, project: &Path, rt: &str, cap: &Capacity) -> usize {
        if self.rebuilding {return 0;}
        let Ok(p) = &self.policy else { return 0 };
        let host = (p.cap.limits.get(rt).copied().unwrap_or(u32::MAX) as usize)
            .saturating_sub(self.slots.values().filter(|r| *r == rt).count());
        let own = (cap.limits.get(rt).copied().unwrap_or(u32::MAX) as usize)
            .saturating_sub(self.slots.iter().filter(|((root, _), r)| root == project && *r == rt).count());
        let max = (p.cap.max.unwrap_or(u32::MAX) as usize).saturating_sub(self.slots.len());
        host.min(own).min(max)
    }

    pub fn elapsed(&self) -> u64 { self.began.elapsed().as_secs() }

    /// Host-wide pending grants and running children (§ 3), for the § 8 compile-jobs share.
    pub fn occupied(&self)->usize {self.slots.len()}

    pub fn used(&self)->BTreeMap<String,usize> {
        let mut used=BTreeMap::new();
        for runtime in self.slots.values() {*used.entry(runtime.clone()).or_default()+=1;}
        used
    }
}

#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceRecord {
    schema:u8,pid:u32,started:String,instance:String,at:u64,version:String,
    #[serde(default)] slots_protocol:Option<u8>,
    #[serde(default)] restart_protocol:Option<u8>,
    #[serde(default)] stop_protocol:Option<u8>,
    #[serde(default,skip_serializing_if="Option::is_none")] full_stop_protocol:Option<u8>,
    #[serde(default)] uninstall_protocol:Option<u8>,
    #[serde(default)] install_protocol:Option<u8>,
    #[serde(default,skip_serializing_if="Option::is_none")] install_cancel_protocol:Option<u8>,
}
enum OwnerRecord { Legacy(u32), Current(ServiceRecord) }

fn service_record(install:&Path)->Res<Option<OwnerRecord>> {
    use std::io::Read;
    let file=match fs::File::open(install.join("service")) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(format!("host service owner unknown: {e}")),
        Ok(file)=>file,
    };
    let mut bytes=Vec::new();file.take(1025).read_to_end(&mut bytes).map_err(|e|format!("host service owner unknown: {e}"))?;
    if bytes.len()>1024 {return Err("host service owner unknown: oversized record".into());}
    let text=String::from_utf8(bytes).map_err(|_|"host service owner unknown: invalid text")?;
    if let Ok(pid)=text.trim().parse::<u32>() {
        if pid==0||pid>i32::MAX as u32 {return Err("process liveness unknown: invalid pid".into());}
        return Ok(Some(OwnerRecord::Legacy(pid)));
    }
    let r:ServiceRecord=serde_json::from_str(&text).map_err(|_|"host service owner unknown: malformed record")?;
    if r.install_cancel_protocol.is_some_and(|v|v!=1) {return Err("host service cancellation protocol unknown; kept".into());}
    if r.schema!=1||r.pid==0||r.pid>i32::MAX as u32||r.started.is_empty()||r.started.len()>128
        ||!r.started.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-')||!crate::resources::id(&r.instance)||r.version.is_empty()||r.version.len()>80||r.restart_protocol.is_some_and(|v|v!=1)||r.stop_protocol.is_some_and(|v|v!=1&&v!=2)||r.full_stop_protocol.is_some_and(|v|v!=1)||r.uninstall_protocol.is_some_and(|v|v!=1)||r.install_protocol.is_some_and(|v|v!=1)||r.slots_protocol.is_some_and(|v|v!=1) {
        return Err("host service owner unknown: unsupported record; kept".into());
    }
    Ok(Some(OwnerRecord::Current(r)))
}

fn publish_service(install:&Path,native:&coreq::process::Identity,instance:&str)->Res<()> {
    let record=ServiceRecord {schema:1,pid:native.pid,started:native.started.clone(),instance:instance.into(),at:wall()?,version:crate::platform::VERSION.into(),slots_protocol:Some(1),restart_protocol:Some(1),stop_protocol:Some(2),full_stop_protocol:cfg!(target_os="macos").then_some(1),uninstall_protocol:Some(1),install_protocol:Some(1),install_cancel_protocol:Some(1)};
    crate::platform::write_synced(&install.join("service"),&(serde_json::to_string(&record).map_err(|e|e.to_string())?+"\n"))
}

/// A prior recorded owner must have exited; age never proves that. The native lock still owns admission.
pub fn other(install: &Path) -> Res<Option<u32>> {
    let record=match service_record(install) {
        Err(e)=>return if unreadable_exited(install)?||zeroed_unowned(install)? {Ok(None)} else {Err(e)},
        Ok(record)=>record,
    };
    match record {
        None=>Ok(None),
        Some(OwnerRecord::Legacy(pid))=>Ok((pid!=std::process::id()&&crate::runtime::liveness(pid)?).then_some(pid)),
        Some(OwnerRecord::Current(r))=> {
            let live=coreq::process::identity(r.pid).map_err(|e|format!("host service owner unknown: {e}"))?;
            Ok((r.pid!=std::process::id()&&live.as_ref().is_some_and(|p|p.started==r.started)).then_some(r.pid))
        }
    }
}

/// #1445: a power loss can zero the record; the free native owner lock proves no live owner, so keep it aside.
fn zeroed_unowned(install:&Path)->Res<bool> {
    let Ok(_lock)=coreq::file_lock::try_lock(&install.join("service.lock")) else {return Ok(false)};
    let moved=crate::data::set_aside_zeroed(&install.join("service"))?;
    if moved {crate::platform::log("host service record zeroed by a power loss; kept as service.zeroed-*, startup continues");}
    Ok(moved)
}

/// A record this version cannot read (a newer release's protocol or field) is stale once its recorded process
/// has exited (#1310): its pid and start time alone prove that. A live or unidentifiable owner stays unknown.
fn unreadable_exited(install:&Path)->Res<bool> {
    use std::io::Read;
    let Ok(file)=fs::File::open(install.join("service")) else {return Ok(false)};
    let mut bytes=Vec::new();
    if file.take(1025).read_to_end(&mut bytes).is_err()||bytes.len()>1024 {return Ok(false);}
    let Ok(record)=serde_json::from_slice::<Value>(&bytes) else {return Ok(false)};
    let pid=record["pid"].as_u64().and_then(|p|u32::try_from(p).ok()).filter(|p|*p!=0&&*p<=i32::MAX as u32);
    let (Some(pid),Some(started))=(pid,record["started"].as_str().filter(|s|!s.is_empty()&&s.len()<=128&&s.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'))) else {return Ok(false)};
    if pid==std::process::id() {return Ok(false);}
    let live=coreq::process::identity(pid).map_err(|e|format!("host service owner unknown: {e}"))?;
    Ok(live.is_none_or(|p|p.started!=started))
}

/// Native process state and projection freshness are separate; neither releases any owned execution.
pub fn service_status(install:&Path)->Value {
    let result=coreq::telemetry::capture::operation("host.service.observe",||->Res<Value> {
        let record=service_record(install)?.ok_or("host service owner record missing")?;
        let OwnerRecord::Current(r)=record else {return Err("legacy PID-only owner record; host identity unknown".into());};
        let native=coreq::process::identity(r.pid).map_err(|e|format!("host service owner unknown: {e}"))?;
        let live=native.as_ref().is_some_and(|p|p.started==r.started);
        let age=wall()?.checked_sub(r.at);
        let fresh=live&&age.is_some_and(|age|age<=60);
        Ok(json!({"state":if live {"running"}else{"exited"},"pid":r.pid,"started":r.started,"instance":r.instance,"version":r.version,
            "at":r.at,"age_seconds":age,"slots_protocol":r.slots_protocol,"fresh":fresh,"restart_protocol":r.restart_protocol,"stop_protocol":r.stop_protocol,"full_stop_protocol":r.full_stop_protocol,"uninstall_protocol":r.uninstall_protocol,"install_protocol":r.install_protocol,"install_cancel_protocol":r.install_cancel_protocol,"reason":if !live {Some("recorded host process exited; worker outcomes remain separate")}else if !fresh {Some("host observation stale or clock unknown")}else{None}}))
    },|_|"owner-unknown");
    result.unwrap_or_else(|reason|json!({"state":"unknown","fresh":false,"reason":reason}))
}

pub fn service_line(status:&Value)->String {
    match status["state"].as_str() {
        Some("running")=>format!("Service: running (host, pid {}); observation {}",status["pid"],if status["fresh"]==true {"fresh"}else{"stale or unknown"}),
        Some("exited")=>format!("Service: recorded host process exited (pid {}); worker outcomes unknown here",status["pid"]),
        _=>format!("Service: unknown ({})",status["reason"].as_str().unwrap_or("no native observation")),
    }
}

/// Legacy roots are never served concurrently, even when their status has gone stale. Unknown holds admission.
pub fn project_service(root: &Path) -> Res<Option<u32>> {
    match crate::data::json(&crate::dot(root).join("status.json")) {
            Err(e) => return Err(format!("legacy liveness unknown: {e}")),
            Ok(None) => Ok(None),
            Ok(Some(v)) => {
                let pid = v["pid"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("legacy liveness unknown: missing pid")?;
                if pid == 0 { return Err("process liveness unknown: invalid pid".into()); }
    Ok((pid != std::process::id() && crate::runtime::liveness(pid)?).then_some(pid))
            }
    }
}

fn legacy(install:&Path,roots: &[PathBuf]) -> Res<()> {
    for root in roots {
        // #1445: the host owner lock is held, so a zeroed status.json has no live owner.
        if crate::data::set_aside_zeroed(&crate::dot(root).join("status.json"))? {append_project(install,root,"status.json zeroed by a power loss; kept as status.json.zeroed-*");}
        if project_service(root)?.is_some() { return Err("handoff: legacy service still running".into()); }
    }
    Ok(())
}

fn startup_status(install: &Path, root: &Path, error: &str,instance:&str) -> Res<()> {
    if project_service(root)?.is_some() { return Err("legacy ownership retained; startup status not written".into()); }
    let mut status = coreq::status::common(&crate::platform::tool(install), "unknown", &crate::project_name(root),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()), Some(error.into()), None);
    fs::create_dir_all(crate::dot(root)).map_err(|e| e.to_string())?;
    status["host_instance"]=json!(instance);
    crate::platform::write_status(&crate::platform::tool(install), root, &status)
}

fn owner(install:&Path)->Res<Option<coreq::file_lock::Lock>> {
    if other(install)?.is_some() {return Ok(None);}
    match coreq::file_lock::try_lock(&install.join("service.lock")) {
        Ok(lock)=>Ok(Some(lock)),
        Err(std::fs::TryLockError::WouldBlock)=>Ok(None),
        Err(e)=>Err(format!("host service owner lock unknown: {e}")),
    }
}

// An admission barrier does not replace valid host maintenance intervals.
fn guarded_policy(configured:Res<Policy>,barrier:&Res<()>)->(Res<Policy>,(u64,u64)) {
    let timers=configured.as_ref().map(|p|(p.update_every,p.rollback_after)).unwrap_or((60,600));
    (barrier.as_ref().map_err(Clone::clone).and(configured),timers)
}

pub fn run(install: &Path) -> Res<()> {
    fs::create_dir_all(install).map_err(|e| e.to_string())?;
    // #2057: after a re-exec the old image's helpers (tray `open`, curl, `log show`) are children no thread waits for
    #[cfg(unix)]
    match crate::runtime::inherit_children() {
        Ok(pids) if pids.is_empty() => {},
        Ok(pids) => crate::platform::log(&format!("host start: {} inherited children reaped on exit", pids.len())),
        Err(e) => crate::platform::log(&format!("host start: {e}; inherited children stay unreaped")),
    }
    let mut reported=String::new();
    let mut linked=std::time::Instant::now();
    let _owner=loop {
        let message=match owner(install) {
            Ok(Some(owner))=>break owner,
            Ok(None)=>"host service owner already active; waiting".into(),
            Err(e)=>format!("{e}; waiting"),
        };
        if message!=reported {crate::platform::log(&message);reported=message;}
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    match crate::resources::retire_superseded(install) {
        Ok(Some(line))=>crate::platform::log(&line),
        Ok(None)=>{},
        Err(e)=>crate::platform::log(&e),
    }
    // #1625: each start (logon, install, restart) re-arms the 5-minute repeat that Stop disabled.
    #[cfg(windows)]
    match crate::host_control::repeat_trigger(&crate::platform::tool(install),true) {
        Ok("changed")=>crate::platform::log("host task 5-minute restart trigger enabled"),
        Ok(_)=>{},
        Err(e)=>crate::platform::log(&format!("host task restart trigger: {e}")),
    }
    let mut telemetry=crate::telemetry::HostCapture::default();
    // Host identity is stable for this owner; policy and interrupted writes are still read each tick.
    let mut hostname=crate::hostname();
    let shared = Arc::new(Mutex::new(Host::new(Policy::read_for_hostname(install,||hostname.clone()))));
    shared.lock().map_err(|_|"host occupancy recovery unknown")?.rebuilding=true;
    let native=coreq::process::identity(std::process::id()).map_err(|e|e.to_string())?.ok_or("host process identity missing")?;
    let instance=shared.lock().map_err(|_|"host owner unknown")?.instance.clone();
    let mut threads: BTreeMap<PathBuf, std::thread::JoinHandle<Res<()>>> = BTreeMap::new();
    let mut roots = vec![];
    let mut stopped = BTreeSet::new();
    let mut drained_roots = BTreeSet::new();
    let mut checked: Option<Instant> = None;
    let mut installation_checked:Option<Instant>=None;
    let mut maintenance_result=String::new();
    let mut upgrade_report:Option<bool>=None;
    let (mut waits_reported,mut stuck)=(String::new(),None);
    let began = Instant::now();
    let mut hour = (Instant::now(), crate::requests::totals());
    let mut tray = None;
    let mut doctor = crate::doctor::Doctor::default();
    let mut registry = Registry::default();
    let mut restart=crate::lifecycle::Monitor::default();
    if let Err(e)=crate::setup::recover(install) {crate::platform::log(&e);}
    loop {
        if let Some(line)=telemetry.pass(install) {crate::platform::log(&line);}
        // #1297: steady host passes fold like project passes (main.rs `service.loop`).
        let _repeating = tracing::info_span!("service.host.loop", coreq_repeating = true).entered();
        if let Err(e) = crate::profile::prune(install) { eprintln!("taskq: telemetry unknown: {e}"); }
        let publication=publish_service(install,&native,&instance).err();
        if let Some(error)=&publication {
            let message=format!("host publication unknown: {error}");if reported!=message {crate::platform::log(&message);reported=message;}
        }
        registry.refresh(install, began.elapsed().as_secs());
        restart.refresh(install);
        roots.clone_from(&registry.roots);
        let old = legacy(install,&roots);
        if hostname.is_empty() {hostname=crate::hostname();}
        let configured = match &old { Ok(()) => migrate_for_hostname(install, &roots,&hostname).and_then(|_| Policy::read_for_hostname(install,||hostname.clone())), Err(_) => Policy::read_for_hostname(install,||hostname.clone()) };
        let (policy,timers)=guarded_policy(configured,&old);
        // #1769: a Stop whose generation ended can never complete; this owner resolves it instead of holding forever
        if restart.current.as_ref().ok().and_then(Option::as_ref).is_some_and(|p|p.stops()&&p.origin()!=instance) {
            match crate::lifecycle::supersede(install,&instance) {
                Ok(Some(id))=>{log_all(install,&roots,&format!("Stop {id} of an ended host generation dropped; this generation serves on"));restart.current=Ok(None);},
                Ok(None)=>{},
                Err(e)=>if reported!=e {crate::platform::log(&e);reported=e;},
            }
        }
        let removal=restart.current.as_ref().ok().and_then(Option::as_ref).and_then(|p|p.removal().map(Path::to_path_buf));
        drained_roots.retain(|root|removal.as_ref()==Some(root));
        {
            let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
            let held = installation::admission_reason(install).or_else(||registry.error.clone()).or_else(|| old.as_ref().err().cloned()).or_else(||restart.current.as_ref().err().cloned())
                .or_else(||publication.as_ref().map(|e|format!("host publication unavailable: {e}")));
            h.reload_admission(policy,held,&roots);
            if let Some(p)=restart.current.as_ref().ok().and_then(Option::as_ref) {
                if p.stops() {
                    if !h.stopping {h.prepared.clear();}
                    h.restart=true;h.stopping=true;
                    if p.origin()!=instance {h.held=Some(format!("Stop {} belongs to an ended host generation; dropped after its expiry",p.id()));}
                } else if p.action()==crate::lifecycle::Action::Restart&&p.origin()==instance {h.restart=true;}
            }
            if let Err(error)=crate::external_slots::tick(install,&mut h) {h.held=Some(format!("external slots unknown: {error}"));}
            let now = h.elapsed();
            if h.gate.last.is_none_or(|l| now.saturating_sub(l.0) >= 30) {
                h.gate.host_mem = crate::platform::host_memory();
                let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
                h.gate.observe(now, wall, crate::platform::cpu_ticks(), crate::platform::memory_available());
            }
            for root in threads.keys().filter(|p| !roots.contains(p)) { h.draining.insert(root.clone()); }
            for root in &roots {
                if removal.as_ref()==Some(root) {h.draining.insert(root.clone());}else{h.draining.remove(root);}
                if threads.contains_key(root) || stopped.contains(root)||drained_roots.contains(root) { continue; }
                h.attach(root);
                let root = root.clone();
                let install = install.to_path_buf();
                let host = shared.clone();
                threads.insert(root.clone(), std::thread::spawn(move || {
                    enter(&root, &install);
                    loop {
                        // Never replace another process's ownership evidence, including on startup/config failure.
                        match project_service(&root) {
                            Ok(None) => {},
                            Ok(Some(_)) | Err(_) => { std::thread::sleep(std::time::Duration::from_secs(1)); continue; }
                        }
                        // A broken project config holds only this thread; no board or child exists yet.
                        let context = crate::load(&root).and_then(|(cfg, raw)| {
                            let board = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &crate::platform::tool(&install))?;
                            Ok(crate::Ctx { cfg, raw, board })
                        });
                        match context {
                            Ok(c) => return crate::service_run(&c, Some(host)),
                            Err(e) => {
                                if let Ok(mut h) = host.lock() {
                                    if h.draining.contains(&root)&&installation::cancelling_root(&install).as_deref()==Some(root.as_path())
                                        &&!h.slots.keys().any(|(project,_)|project==&root)&&crate::kids_file(&install,&root)?.is_empty() {
                                        return Err("project drained".into());
                                    }
                                    // No board/native reconciliation exists while configuration is invalid.
                                    h.busy.insert(root.clone(), true);
                                    h.prepared.remove(&root);
                                }
                                let instance=host.lock().map(|h|h.instance.clone()).unwrap_or_default();
                                let _ = startup_status(&install, &root, &e,&instance);
                                crate::platform::log(&e);
                                std::thread::sleep(std::time::Duration::from_secs(1));
                            }
                        }
                    }
                }));
            }
        }
        {let mut h=shared.lock().unwrap_or_else(|e|e.into_inner());if roots.is_empty()&&h.busy.is_empty()&&h.held.is_none() {h.rebuilding=false;}}
        // § 4 setup sessions: only this loop launches them. ponytail: a launch blocks this pass up to 30 s (Codex: 60 s); a thread if that matters.
        if let Err(e)=crate::setup::step(install,crate::setup::launch) {
            if reported!=e {crate::platform::log(&e);reported=e;}
        }
        // D2/D6 workspace dialogue: owner turns and watches; a holder probe runs only when a turn is pending.
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_millis());
        if let Err(e)=crate::dialogue::tick(install,now,&crate::dialogue::NATIVE) {
            if reported!=e {crate::platform::log(&e);reported=e;}
        }
        // Missing setup links: one native read per 10 s, not per pass.
        if linked.elapsed()>=std::time::Duration::from_secs(10) {
            linked=std::time::Instant::now();
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_millis());
            if let Err(e)=crate::setup::link(install,now,crate::setup::claude_link) {
                if reported!=e {crate::platform::log(&e);reported=e;}
            }
        }
        let loaded={let h=shared.lock().unwrap_or_else(|e|e.into_inner());
            h.capacity_command.is_none().then(||h.policy.as_ref().ok().cloned()).flatten()};
        if let Some(policy)=loaded {
            match observe_capacity(install,&policy) {
                Ok(command)=> {
                    let mut h=shared.lock().unwrap_or_else(|e|e.into_inner());
                    if h.policy.as_ref().ok()==Some(&policy) {h.capacity_command=command;}
                }
                Err(e)=> {if reported!=e {crate::platform::log(&e);reported=e;}}
            }
        }
        if let Err(error)=reconcile_projects(install,&shared,&mut threads,&roots,&mut stopped,&mut drained_roots) {
            if reported!=error {crate::platform::log(&error);reported=error;}
        }
        if let Some(p)=restart.current.as_ref().ok().and_then(Option::as_ref).filter(|p|p.action()==crate::lifecycle::Action::Uninstall&&!p.stops()) {
            let drained=p.removal().is_some_and(|root|drained_roots.contains(root));
            if drained||p.removal().is_some_and(|root|!roots.iter().any(|p|p==root)) {
                if let Err(error)=crate::lifecycle::finish_project_uninstall(install,p.id(),&instance,drained) {
                    let _=crate::lifecycle::failed(install,p.id(),&error);if reported!=error {crate::platform::log(&error);reported=error;}
                }
            }
        }
        let h = shared.lock().unwrap_or_else(|e| e.into_inner());
        let recovered=h.publications_ready(&roots)&&roots.iter().all(|r|h.recovered.contains(r));
        drop(h);
        if let Err(e)=crate::lifecycle::observe(install,&instance,recovered) {if reported!=e {crate::platform::log(&e);reported=e;}}
        let mut h=shared.lock().unwrap_or_else(|e|e.into_inner());
        let finished=h.stop_controller.as_mut().and_then(|control|control.poll().map(|result|(control.id.clone(),result)));
        if let Some((id,result))=finished {
            drop(h);
            let error=result.err().unwrap_or_else(||"controller exited without an observed host stop".into());
            let _=crate::lifecycle::controller_exited(install,&id,&error);crate::platform::log(&format!("Stop controller: {}",crate::compact(&error,153)));
            h=shared.lock().unwrap_or_else(|e|e.into_inner());
        }
        if h.updating.as_ref().is_some_and(|job|job.is_finished()) {
            let updated=h.updating.take().unwrap().join().unwrap_or_else(|_|Err("host maintenance panicked; partial effects unknown".into()));
            let text=match &updated {Ok(Maintenance::Update(Some(v)))=>format!("update: {v}"),Ok(Maintenance::Update(None))=>"update: up to date".into(),Ok(Maintenance::Installation(result))=>format!("installation: {result}"),Err(e)=>format!("maintenance: {e}")};
            if maintenance_once(&mut maintenance_result,&text) {log_all(install,&roots,&text);}
            if crate::installed(install)&&matches!(updated,Ok(Maintenance::Update(Some(_)))) {h.restart=true;}
            // § 9 deploy: the release flow records the applied version in each release root that sets `deploy`
            if let (true,Ok(Maintenance::Update(Some(v))),Ok(exe))=(crate::installed(install),&updated,std::env::current_exe()) {
                for l in crate::deploy::applied(&exe,&roots,crate::platform::channel(install).unwrap_or("live"),crate::platform::VERSION,v,&crate::hostname()) {log_all(install,&roots,&l);}
            }
        }
        let transition_ready=crate::platform::transition_ready(install).is_ok();
        if !transition_ready&&h.updating.is_none()&&installation_checked.is_none_or(|at|at.elapsed().as_secs()>=5) {
            installation_checked=Some(Instant::now());
            let install=install.to_path_buf();
            let job=coreq::telemetry::capture::operation("host.install.recover.dispatch",||std::thread::Builder::new().name("taskq-install-recovery".into()).spawn(coreq::telemetry::capture::bind(move || {
                coreq::telemetry::capture::operation("host.install.recover",||installation::recover(&install).map(Maintenance::Installation),|_|"install-pending")
            })),|_|"spawn");
            match job {Ok(job)=>h.updating=Some(job),Err(error)=>crate::platform::log(&format!("installation recovery not started: {error}"))}
        }
        let asked=transition_ready&&!h.restart&&h.updating.is_none()&&(fs::remove_file(install.join("update")).is_ok()||roots.iter().any(|r|fs::remove_file(crate::dot(r).join("update")).is_ok()));
        if transition_ready&&!h.restart&&h.updating.is_none()&&(asked || checked.is_none_or(|t| t.elapsed().as_secs() >= timers.0)) {
            checked = Some(Instant::now());
            let install=install.to_path_buf();
            let job=coreq::telemetry::capture::operation("host.update.dispatch",||std::thread::Builder::new().name("taskq-update".into()).spawn(coreq::telemetry::capture::bind(move || {
                coreq::telemetry::capture::operation("host.update",|| {
                    (if crate::installed(&install) {crate::platform::service_update(&install)}else{crate::platform::update(&install,true)}).map(Maintenance::Update)
                },|_|"update-failed")
            })),|_|"spawn");
            match job {Ok(job)=>h.updating=Some(job),Err(error)=>crate::platform::log(&format!("update not started: {error}"))}
        }
        if install.join("restart").exists() || roots.iter().any(|r| crate::dot(r).join("restart").exists()) { h.restart = true; }
        if h.restart_wake() {
            for root in &roots {
                if let Err(e) = crate::poke(root) { append_project(install, root, &e); }
            }
        }
        // The installer's 300 s wait began before this generation started; a reason not retained retries next tick.
        let late=began.elapsed().as_secs()>=300;
        if upgrade_report!=Some(late) {if let Some(reason)=installation::stale_upgrade(install,late) {
            if late {log_all(install,&roots,&reason);}
            upgrade_report=Some(late);
        }}
        let handover=h.restart&&!h.stopping;
        if !handover {stuck=None;}
        if h.restart {
            let wait=h.handoff_wait();
            drop(h);
            // #1601: the receipt and the log name what the re-exec really waits for; a stuck one is reported, never killed
            let waits=restart_waits(&roots);
            if let Err(e)=crate::lifecycle::handoff_wait(install,&instance,wait,&waits) {if reported!=e {crate::platform::log(&e);reported=e;}}
            if waits!=waits_reported&&installation::upgrade_waits(install,crate::platform::VERSION,&waits) {waits_reported=waits.clone();}
            if crate::restart_watch(&mut stuck,handover,began.elapsed().as_secs(),300) {log_all(install,&roots,&format!("restart pending over 600 s; waits for {waits}"));}
            h=shared.lock().unwrap_or_else(|e|e.into_inner());
            if h.handoff_ready() {
                if h.stopping&&h.stop_controller.is_some() {drop(h);std::thread::sleep(std::time::Duration::from_secs(1));continue;}
                let stopping=h.stopping;
                drop(h);
                if let Err(e)=crate::lifecycle::prepare(install,&instance) {
                    if stopping&&let Ok(Some(p))=crate::lifecycle::pending(install) {let _=crate::lifecycle::failed(install,p.id(),&e);}
                    if reported!=e {crate::platform::log(&e);reported=e;}
                    std::thread::sleep(std::time::Duration::from_secs(1));continue;
                }
                if stopping {
                    match crate::lifecycle::pending(install).and_then(|p|p.filter(|p|p.stops()).ok_or("prepared Stop binding unavailable".into())) {
                        Ok(p)=>match crate::host_control::dispatch(install,&p) {
                        Ok(control)=>{crate::platform::log(&format!("service stops: Stop {} controller ends this process",p.id()));shared.lock().unwrap_or_else(|e|e.into_inner()).stop_controller=Some(control);},
                        Err(e)=>{let _=crate::lifecycle::failed(install,p.id(),&e);crate::platform::log(&e);}
                        },
                        Err(e)=>crate::platform::log(&e),
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));continue;
                }
                let _ = fs::remove_file(install.join("restart"));
                for root in &roots { let _ = fs::remove_file(crate::dot(root).join("restart")); }
                #[cfg(unix)] {
                    use std::os::unix::process::CommandExt;
                    let mut command = crate::platform::command(crate::launcher(install));
                    command.args(["service", "run"]).current_dir(install).env_remove("TASKQ_ROOT");
                    coreq::telemetry::capture::before_exec();
                    let e = command.exec();
                    return Err(format!("host re-exec: {e}"));
                }
                #[cfg(not(unix))] crate::platform::service_exit(75,&format!("restart to {}",crate::platform::tool(install).current().unwrap_or_default()));
            }
        }
        drop(h);
        #[cfg(unix)] crate::runtime::reap_inherited();
        if crate::installed(install) {
            // #2048: hourly host doctor, off the tick thread: stale releases and qualification leftovers.
            if let Some(line)=doctor.tick(install) {crate::platform::log(&line);}
            crate::keep_tray(install, &mut tray);
            if began.elapsed().as_secs() >= timers.1 && shared.lock().is_ok_and(|h| h.publications_ready(&roots)) {
                if let Err(e) = fs::write(install.join("releases").join(crate::platform::VERSION).join("healthy"), "") {
                    crate::platform::log(&format!("release health unknown: {e}"));
                }
            }
        }
        if hour.0.elapsed().as_secs() >= 3600 {
            let total = crate::requests::totals();
            let usage = crate::requests::diff(&total, &hour.1);
            crate::requests::record("service", &usage);
            for root in &roots { append_project(install, root, &format!("requests 1h: {}", crate::requests::summary(&usage, ""))); }
            let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
            h.last_hour = Some(usage); h.hour_start = total.clone();
            let hosts = crate::requests::host_totals();
            h.hosts_last_hour = Some(crate::requests::diff(&hosts, &h.hosts_start)); h.hosts_start = hosts;
            hour = (Instant::now(), total);
        }
        // Watch <install>/poke every second; the registry is otherwise polled once a minute.
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// The restart waits each project last published in its status (§ 5), named by project folder.
fn restart_waits(roots: &[PathBuf]) -> String {
    let waits:Vec<String>=roots.iter().flat_map(|root| {
        let name=root.file_name().map_or_else(||root.display().to_string(),|n|n.to_string_lossy().into_owned());
        let status=crate::data::json(&crate::dot(root).join("status.json")).ok().flatten().unwrap_or_default();
        status["restart_waits"].as_array().into_iter().flatten().filter_map(|w|w.as_str()).map(|w|format!("{name} {w}")).collect::<Vec<_>>()
    }).collect();
    if waits.is_empty() {"project passes in flight".into()} else {waits.join(", ")}
}

// Join before reconciling cancellation; a thread exit alone never releases its execution.
fn reconcile_projects(install:&Path,shared:&Shared,threads:&mut BTreeMap<PathBuf,std::thread::JoinHandle<Res<()>>>,
    roots:&[PathBuf],stopped:&mut BTreeSet<PathBuf>,drained_roots:&mut BTreeSet<PathBuf>)->Res<()> {
    // Joining is nonblocking: failed projects do not stop unrelated loops, and their occupied slots stay held.
    let ended: Vec<_> = threads.iter().filter(|(_, t)| t.is_finished()).map(|(p, _)| p.clone()).collect();
    for root in ended {
        let outcome = threads.remove(&root).unwrap().join();
        let drained = matches!(&outcome, Ok(Err(e)) if e == "project drained");
        let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
        h.publication(&root, false);
        h.passing.remove(&root);
        if drained { h.detached(&root);drained_roots.insert(root.clone()); }
        else {
            // A reduced loop failed: its attempts expire on other hosts, and local launches hold for inspection.
            log_all(install,std::slice::from_ref(&root),&thread_error(&outcome));
            stopped.insert(root.clone());
            h.prepared.remove(&root);
            h.recovered.remove(&root);
            h.ready(&root, false);
            append_project(install, &root, "project thread stopped; leases expire on other hosts; retained slots have unknown liveness");
        }
    }
    if let Some(root)=installation::cancelling_root(install) {
        let mut h=shared.lock().unwrap_or_else(|e|e.into_inner());
        installation::finish_cancellation(install,&mut h,threads.contains_key(&root)||roots.contains(&root))?;
        stopped.remove(&root);
    }
    Ok(())
}

/// #1416: a host line goes to the service log and every project log; project `taskq`'s log is the service's stderr file, so it gets the line once.
fn log_all(install: &Path, roots: &[PathBuf], text: &str) {
    crate::platform::log(text);
    mirror(install, roots, text, crate::platform::stderr_file(install).as_deref());
}

/// #1445: the reason a project thread ended, for the host and project logs.
fn thread_error(outcome:&std::thread::Result<Res<()>>)->String {
    let reason=match outcome {
        Ok(Ok(()))=>"ended without error".to_string(),
        Ok(Err(e))=>e.clone(),
        Err(panic)=>panic.downcast_ref::<String>().cloned().or_else(||panic.downcast_ref::<&str>().map(|s|s.to_string())).map_or("panic".into(),|p|format!("panic: {p}")),
    };
    format!("project thread error: {}",crate::compact(&reason,136))
}

fn mirror(install: &Path, roots: &[PathBuf], text: &str, stderr: Option<&Path>) {
    for root in roots.iter().filter(|r| stderr != Some(project_log(install, r).as_path())) { append_project(install, root, text); }
}

fn project_log(install: &Path, root: &Path) -> PathBuf { install.join(format!("{}.log", crate::project_name(root))) }

fn append_project(install: &Path, root: &Path, text: &str) {
    use std::io::Write;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let _ = fs::OpenOptions::new().create(true).append(true).open(project_log(install, root))
        .and_then(|mut f| writeln!(f, "{}", crate::platform::log_line(now, text)));
}

fn project_text(install:&Path)->Res<Option<String>> {
    let prepare=format!("{INSTALL_PENDING}.pending");
    for name in ["projects.pending",prepare.as_str()] {
        match fs::symlink_metadata(install.join(name)) {
            Ok(_)=>return Err(format!("{name} retained; registry publication unknown")),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            Err(e)=>return Err(format!("projects prepare unknown: {e}")),
        }
    }
    match fs::symlink_metadata(install.join(INSTALL_PENDING)) {
        Ok(_)=>return installation::registered_projects(install),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        Err(e)=>return Err(format!("installation prepare unknown: {e}")),
    }
    let path=install.join("projects");
    match fs::read_to_string(&path) {
        Ok(text)=>Ok(Some(text)),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),
        Err(e)=>Err(format!("{}: {e}",path.display())),
    }
}

/// Keep configured spelling for existing resource IDs; canonical identity owns execution and aliases.
fn project_entries(text:&str)->Res<Vec<(PathBuf,PathBuf)>> {
    text.lines().filter(|line|!line.is_empty()).map(|line| {
        let path=PathBuf::from(line);
        if !path.is_absolute() {return Err("projects unknown: a root is not absolute".into());}
        let canonical=path.canonicalize().map_err(|e|format!("projects unknown: {e}"))?;
        if !canonical.is_dir() {return Err("projects unknown: a root is not a directory".into());}
        Ok((path,canonical))
    }).collect()
}

pub fn project_paths(install:&Path)->Res<Vec<PathBuf>> {
    let text=project_text(install)?.ok_or_else(||format!("{}: project registry missing",install.join("projects").display()))?;
    Ok(project_entries(&text)?.into_iter().map(|(path,_)|path).collect())
}

/// Read only the explicit registry. Canonical identity prevents duplicate threads for aliases.
pub fn projects(install: &Path) -> Res<Vec<PathBuf>> {
    let text=project_text(install)?.ok_or("projects unknown: registry missing")?;
    Ok(project_entries(&text)?.into_iter().map(|(_,canonical)|canonical).collect::<BTreeSet<_>>().into_iter().collect())
}

/// One owner retains the preflight snapshot and native lock until the registry commit or refusal.
pub struct ProjectEdit {
    install:PathBuf, original:Option<String>, next:String, removed:Vec<PathBuf>, adding:bool,
    _lock:coreq::file_lock::Lock,
}
impl ProjectEdit {
    #[tracing::instrument(name="host.projects.prepare",skip_all)]
    pub fn new(install:&Path,root:&Path,adding:bool)->Res<Self> {
        Self::with_guard(install,root,adding,||crate::lifecycle::registry_guard(install))
    }
    pub fn for_uninstall(install:&Path,root:&Path,id:&str)->Res<Self> {
        Self::with_guard(install,root,false,||crate::lifecycle::uninstall_guard(install,id,root))
    }
    fn with_guard(install:&Path,root:&Path,adding:bool,guard:impl FnOnce()->Res<()>)->Res<Self> {
        fs::create_dir_all(install).map_err(|e|e.to_string())?;
        let lock=coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|format!("project registry busy or lock unavailable: {e}"))?;
        guard()?;
        crate::platform::transition_ready(install)?;
        let path=install.join("projects");
        let original=project_text(install)?;
        if original.is_none() && !adding {return Err(format!("{}: project registry missing; nothing changed",path.display()));}
        match fs::symlink_metadata(&path) {
            Ok(meta)=> {
                if !meta.is_file() || meta.permissions().readonly() {return Err("project registry is not a writable regular file; kept".into());}
                ownership(&path)?;
            }
            Err(e) if e.kind()==std::io::ErrorKind::NotFound && original.is_none()=>{},
            Err(e)=>return Err(format!("project registry metadata unknown: {e}")),
        }
        let target=root.canonicalize().map_err(|e|format!("project root unknown: {e}"))?;
        // #1858: stored as older entries are, without `\\?\`; matching stays canonical
        let plain=crate::platform::plain_path(&target);
        let target_text=plain.to_str().ok_or("project root is not UTF-8")?;
        if target_text.contains(['\n','\r']) {return Err("project root cannot be represented by the line registry".into());}
        let entries=project_entries(original.as_deref().unwrap_or(""))?;
        let removed:Vec<PathBuf>=entries.iter().filter(|(_,canonical)|*canonical==target).map(|(path,_)|path.clone()).collect();
        let mut next=original.clone().unwrap_or_default();
        if adding && removed.is_empty() {
            if !next.is_empty()&&!next.ends_with('\n') {next.push('\n');}
            next.push_str(target_text);next.push('\n');
        } else if !adding && !removed.is_empty() {
            next=entries.iter().filter(|(_,canonical)|*canonical!=target).map(|(path,_)|format!("{}\n",path.display())).collect();
        }
        Ok(Self {install:install.into(),original,next,removed:if removed.is_empty(){vec![target]}else{removed},adding,_lock:lock})
    }

    #[tracing::instrument(name="host.projects.commit",skip_all)]
    pub fn commit(self)->Res<()> {
        let path=self.install.join("projects");
        let write=|| {
            if self.original.as_deref()!=Some(self.next.as_str()) {
                match &self.original {
                    Some(original)=>replace_owned(&path,original,&self.next)?,
                    None=>coreq::write_durable(&path,&self.next)?,
                }
            }
            if fs::read_to_string(&path).map_err(|e|e.to_string())?!=self.next {return Err("project registry write unknown: readback differs".into());}
            Ok(())
        };
        if self.adding {crate::resources::locked(&self.install,write)?;}
        else {crate::resources::unregister(&self.install,&self.removed,write)?;}
        crate::platform::write_atomic(&self.install.join("poke"),"").map_err(|e|format!("project registry saved; host notification unknown: {e}"))
    }
}

#[path="host_install.rs"]
mod installation;
pub use installation::install_project;
pub use installation::cancel_install;
pub use installation::install_acknowledgement;
pub use installation::installation_status;

fn installation_observed(service:&Value,status:Option<&Value>,version:&str)->bool {
    service["state"]=="running"&&service["fresh"]==true&&service["version"]==version
        &&service["pid"].as_u64().is_some_and(|pid|pid>0)
        &&service["instance"].as_str().is_some_and(crate::resources::id)
        &&status.is_some_and(|status|status["pid"]==service["pid"]&&status["host_instance"]==service["instance"]
            &&crate::snapshot::complete(status)&&status["error"].is_null()&&status["focus_error"].is_null())
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
        let poke = fs::metadata(install.join("poke")).and_then(|m| m.modified()).ok();
        if self.checked.is_some_and(|t| now.saturating_sub(t) < 60) && self.poke == poke { return; }
        self.poke = poke; self.checked = Some(now);
        match projects(install) {
            Ok(roots) => { self.roots = roots; self.error = None; }
            // A host that never had a registry has zero projects (setup, § 4); one that loses it reads unknown.
            Err(_) if self.roots.is_empty() && matches!(project_text(install), Ok(None)) => { self.error = None; }
            Err(e) => { if self.error.as_ref() != Some(&e) { crate::platform::log(&e); } self.error = Some(e); }
        }
    }
}

/// #1419: a failed candidate
/// selftest logs once per version, not again for each pass's varying flush counts.
fn maintenance_once(last:&mut String,text:&str)->bool {
    let key=|t:&str|t.find(" selftest failed").unwrap_or(t.len());
    let changed=last[..key(last)]!=text[..key(text)];
    if changed {text.clone_into(last);}
    changed
}

/// Compare all copies before any write. The durable plan is a configuration backup, never task state (R1).
#[cfg(test)]
pub fn migrate(install: &Path, roots: &[PathBuf]) -> Res<()> {migrate_for_hostname(install,roots,&crate::hostname())}

fn migrate_for_hostname(install:&Path,roots:&[PathBuf],hostname:&str)->Res<()> {
    crate::resources::try_locked(install,||migrate_locked(install,roots,hostname))
}

fn migrate_locked(install: &Path, roots: &[PathBuf],hostname:&str) -> Res<()> {
    let backup = install.join("host-policy-migration");
    let manifest = backup.join("plan.json");
    if backup.join("complete").exists() { Policy::read_for_hostname(install,||hostname.into())?; return Ok(()); }
    if !manifest.exists() && install.join("host.json").exists() { Policy::read_for_hostname(install,||hostname.into())?; return Ok(()); }
    let plan: Value = if manifest.exists() {
        serde_json::from_slice(&fs::read(&manifest).map_err(|e| e.to_string())?).map_err(|e| format!("migration backup unknown: {e}"))?
    } else {
        if roots.is_empty() { return Err("host.json unknown: no registered policy inputs".into()); }
        let mut files = vec![];
        let mut policies = vec![];
        for root in roots {
            if root.join("taskq.json.host-policy.tmp").exists() { return Err("migration staging path already exists; originals unchanged".into()); }
            let owner = ownership(&root.join("taskq.json")).map_err(|e| format!("migration input unknown: {e}"))?;
            let original = fs::read_to_string(root.join("taskq.json")).map_err(|e| format!("migration input unknown: {e}"))?;
            let mut v: Value = serde_json::from_str(&original).map_err(|e| format!("migration input unknown: {e}"))?;
            let host = std::env::var("TASKQ_HOST").ok().filter(|s| !s.is_empty()).or_else(|| v["hosts"][hostname].as_str().map(String::from)).unwrap_or_else(||hostname.into());
            let cap = service::capacity(&v, &host)?;
            if !cap.auto { return Err(format!("migration owner choice: {} uses Fixed; originals unchanged", v["repo"].as_str().unwrap_or("project"))); }
            let mut policy = cap.json();
            policy["admission"].as_object_mut().unwrap().remove("mode");
            for (key, default) in [("hosts", json!({})), ("has", json!({})), (crate::runtime::CODEX_ARGS, json!(["-s", "danger-full-access"])),
                ("permission_mode", json!("dontAsk")), ("update_every", json!(60)), ("rollback_after", json!(600))] {
                policy[key] = v.get(key).cloned().unwrap_or(default);
            }
            Policy::parse(policy.clone(), hostname)?;
            policies.push((v["repo"].as_str().unwrap_or("project").to_string(), policy));
            for key in ["capacity_hosts", "admission", "load_gate", "hosts", "has", crate::runtime::CODEX_ARGS, "permission_mode", "update_every", "rollback_after"] {
                v.as_object_mut().ok_or("migration input is not an object")?.remove(key);
            }
            // Identical effective policies need no project cap; top-level limits may be overridden locally.
            v.as_object_mut().unwrap().remove("limits");
            files.push(json!({"root": root, "owner": owner, "original": original, "next": serde_json::to_string_pretty(&v).unwrap() + "\n"}));
        }
        let first = &policies[0].1;
        let differences: Vec<_> = policies.iter().filter(|(_, p)| p != first).map(|(repo, p)| {
            let fields: Vec<_> = first.as_object().unwrap().keys().filter(|k| first[*k] != p[*k]).cloned().collect();
            format!("{repo}: {}", fields.join(", "))
        }).collect();
        if !differences.is_empty() { return Err(format!("migration owner choice: differing {}; originals unchanged", differences.join("; "))); }
        let plan = json!({"policy": first, "files": files});
        fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
        for (i, f) in files.iter().enumerate() {
            fs::copy(Path::new(f["root"].as_str().unwrap()).join("taskq.json"), backup.join(format!("{i}.json"))).map_err(|e| e.to_string())?;
        }
        crate::platform::write_atomic(&manifest, &serde_json::to_string_pretty(&plan).unwrap())?;
        plan
    };
    let policy = plan.get("policy").ok_or("migration plan missing policy")?;
    Policy::parse(policy.clone(), hostname)?;
    let files = plan["files"].as_array().ok_or("migration plan missing files")?;
    let planned: BTreeSet<PathBuf> = files.iter().map(|f| f["root"].as_str().map(PathBuf::from).ok_or("migration plan invalid root")).collect::<Result<_, _>>()?;
    if planned != roots.iter().cloned().collect() || planned.len() != files.len() { return Err("migration registry changed; originals kept, launches held".into()); }
    // Validate every input before resuming any partial replacement; never overwrite a later owner edit.
    for f in files {
        let root = Path::new(f["root"].as_str().ok_or("migration plan invalid root")?);
        let current = fs::read_to_string(root.join("taskq.json")).map_err(|e| e.to_string())?;
        if current != f["original"].as_str().ok_or("migration plan invalid original")? && current != f["next"].as_str().ok_or("migration plan invalid next")? {
            return Err("migration interrupted by changed input; originals kept in backup, launches held".into());
        }
        if ownership(&root.join("taskq.json"))? != f["owner"] { return Err("migration ownership changed; originals kept, launches held".into()); }
    }
    // Stage every replacement with its final metadata before any target changes. Plan ownership makes retry safe.
    let mut staged = vec![];
    for (i, f) in files.iter().enumerate() {
        let path = Path::new(f["root"].as_str().unwrap()).join("taskq.json");
        let stage = path.with_file_name("taskq.json.host-policy.tmp");
        let metadata = fs::metadata(backup.join(format!("{i}.json"))).map_err(|e| e.to_string())?;
        let next = f["next"].as_str().unwrap();
        if fs::read_to_string(&path).map_err(|e| e.to_string())? == next {
            // A completed replacement still verifies/restores metadata after an interrupted prior run.
            fs::set_permissions(&path, metadata.permissions()).map_err(|e| e.to_string())?;
            staged.push(None); continue;
        }
        if stage.exists() { fs::remove_file(&stage).map_err(|e| e.to_string())?; }
        fs::copy(&path, &stage).map_err(|e| e.to_string())?;
        let mut writable = fs::metadata(&stage).map_err(|e| e.to_string())?.permissions();
        #[cfg(windows)] writable.set_readonly(false);
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; writable.set_mode(writable.mode() | 0o200); }
        fs::set_permissions(&stage, writable).map_err(|e| e.to_string())?;
        fs::write(&stage, next).map_err(|e| e.to_string())?;
        fs::set_permissions(&stage, metadata.permissions()).map_err(|e| e.to_string())?;
        stage_owner(&stage, &f["owner"])?;
        if ownership(&stage)? != f["owner"] { return Err("migration staging ownership differs; originals unchanged".into()); }
        if fs::read_to_string(&stage).map_err(|e| e.to_string())? != next { return Err("migration staging readback differs".into()); }
        staged.push(Some(stage));
    }
    crate::platform::write_atomic(&install.join("host.json"), &(serde_json::to_string_pretty(policy).unwrap() + "\n"))?;
    for (i, f) in files.iter().enumerate() {
        let path = Path::new(f["root"].as_str().unwrap()).join("taskq.json");
        let next = f["next"].as_str().unwrap();
        if let Some(stage) = &staged[i] {
            replace_project(&path, stage)?;
            // ReplaceFileW merges DACL inheritance flags; restore the recorded descriptor before readback.
            #[cfg(windows)]
            stage_owner(&path, &f["owner"])?;
        }
        if fs::read_to_string(&path).map_err(|e| e.to_string())? != next || ownership(&path)? != f["owner"] { return Err("migration content/ownership readback differs; launches held".into()); }
    }
    if Policy::read_for_hostname(install,||hostname.into())?.raw != *policy { return Err("host.json migration readback differs".into()); }
    crate::platform::write_atomic(&backup.join("complete"), "verified\n")?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn ownership(path: &Path) -> Res<Value> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    if m.uid() != unsafe { libc::geteuid() } { return Err("migration needs owner: cannot preserve another user's file ownership".into()); }
    Ok(json!({"uid": m.uid(), "gid": m.gid()}))
}
#[cfg(unix)]
fn stage_owner(path: &Path, owner: &Value) -> Res<()> {
    use std::os::unix::ffi::OsStrExt;
    let p = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let uid = owner["uid"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("migration owner unknown")?;
    let gid = owner["gid"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("migration group unknown")?;
    if unsafe { libc::chown(p.as_ptr(), uid, gid) } != 0 { return Err(format!("migration cannot preserve ownership: {}", std::io::Error::last_os_error())); }
    Ok(())
}
#[cfg(unix)]
fn replace_project(path: &Path, stage: &Path) -> Res<()> { fs::rename(stage, path).map_err(|e| e.to_string()) }

#[cfg(windows)]
pub(crate) fn ownership(path: &Path) -> Res<Value> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "advapi32")]
    unsafe extern "system" { fn GetFileSecurityW(path: *const u16, info: u32, descriptor: *mut u8, size: u32, needed: *mut u32) -> i32; }
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut size = 0;
    unsafe { GetFileSecurityW(path.as_ptr(), 7, std::ptr::null_mut(), 0, &mut size); }
    if size == 0 { return Err(format!("migration ownership unknown: {}", std::io::Error::last_os_error())); }
    let mut descriptor = vec![0; size as usize];
    if unsafe { GetFileSecurityW(path.as_ptr(), 7, descriptor.as_mut_ptr(), size, &mut size) } == 0 { return Err(format!("migration ownership unknown: {}", std::io::Error::last_os_error())); }
    // SetFileSecurityW clears AUTO_INHERITED bookkeeping even with byte-identical owner, group and DACL.
    *descriptor.get_mut(3).ok_or("migration ownership descriptor truncated")? &= !0x04;
    Ok(json!(descriptor))
}
#[cfg(windows)]
fn stage_owner(path: &Path, owner: &Value) -> Res<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "advapi32")]
    unsafe extern "system" { fn SetFileSecurityW(path: *const u16, info: u32, descriptor: *const u8) -> i32; }
    let descriptor: Vec<u8> = owner.as_array().ok_or("migration ownership unknown")?.iter()
        .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or("migration ownership corrupt")).collect::<Result<_, _>>()?;
    // Self-relative SID slices avoid native pointer reads from a retained migration manifest.
    fn sid(descriptor:&[u8],field:usize)->Res<&[u8]> {
        if descriptor.len()<20||descriptor[0]!=1||descriptor[3]&0x80==0 {return Err("migration ownership descriptor corrupt".into());}
        let offset=u32::from_le_bytes(descriptor[field..field+4].try_into().unwrap()) as usize;
        if offset==0 {return Ok(&[]);}
        let header=descriptor.get(offset..offset.saturating_add(8)).filter(|s|offset>=20&&s[0]==1&&s[1]<=15).ok_or("migration ownership SID corrupt")?;
        descriptor.get(offset..offset.saturating_add(8+4*header[1] as usize)).ok_or_else(||"migration ownership SID truncated".into())
    }
    let current=ownership(path)?;
    if &current==owner {return Ok(());}
    let current:Vec<u8>=serde_json::from_value(current).map_err(|_|"migration ownership corrupt")?;
    let mut information=4;
    for (flag,field) in [(1,4),(2,8)] {
        if sid(&descriptor,field)?!=sid(&current,field)? {information|=flag;}
    }
    let control = descriptor.get(2..4).ok_or("migration ownership corrupt")?;
    let protected = u16::from_le_bytes([control[0], control[1]]) & 0x1000 != 0;
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { SetFileSecurityW(path.as_ptr(), information | if protected { 0x80000000 } else { 0x20000000 }, descriptor.as_ptr()) } == 0 {
        return Err(format!("migration cannot preserve ownership: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}
#[cfg(windows)]
fn replace_project(path: &Path, stage: &Path) -> Res<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" { fn ReplaceFileW(path: *const u16, stage: *const u16, backup: *const u16, flags: u32, exclude: *mut std::ffi::c_void, reserved: *mut std::ffi::c_void) -> i32; }
    let wide = |p: &Path| p.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    if unsafe { ReplaceFileW(wide(path).as_ptr(), wide(stage).as_ptr(), std::ptr::null(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 { return Err(format!("migration replace: {}", std::io::Error::last_os_error())); }
    Ok(())
}

/// Per-project restart/quarantine clock. Children stay outside the unwind boundary in the thread's Service.
#[derive(Default)]
pub struct Panics { times: Vec<u64>, pub until: u64, pub reason: Option<String> }
impl Panics {
    pub fn quarantined(&self) -> bool { self.times.len() >= 3 }
    pub fn record(&mut self, now: u64, msg: &str) {
        self.times.retain(|t| now.saturating_sub(*t) <= 600);
        self.times.push(now);
        let class = crate::incident::panic_class(msg);
        let delay = [1, 10, 60][self.times.len().min(3) - 1];
        self.until = now + delay;
        self.reason = Some(if self.quarantined() { format!("quarantined: {class}") } else { format!("{class}; restarting in {delay} s") });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // #1416: project `taskq` logs into the service's stderr file; a host line reaches every log once.
    #[test]
    fn host_line_written_once_per_log() {
        let install=std::env::temp_dir().join(format!("taskq-log-once-{}",crate::runtime::uuid()));fs::create_dir(&install).unwrap();
        let roots=[install.join("p/taskq"),install.join("p/other")];
        let stderr=install.join("taskq.log");
        fs::write(&stderr,"x update: up to date\n").unwrap();
        mirror(&install,&roots,"update: up to date",Some(&stderr));
        for f in ["taskq.log","other.log"] {assert_eq!(fs::read_to_string(install.join(f)).unwrap().matches("update: up to date").count(),1,"{f}");}
        mirror(&install,&roots,"next",None);
        assert_eq!(fs::read_to_string(&stderr).unwrap().matches("next").count(),1);
        fs::remove_dir_all(&install).unwrap();
    }

    // Defect #1419: the failed 4.6.26 selftest line was logged every pass.
    #[test]
    fn failed_selftest_logs_once_per_version() {
        let mut last=String::new();
        let fail=|v:&str,n:u8|format!("maintenance: {v} selftest failed: Error: telemetry: FAIL telemetry flush unacknowledged (accepted {n} inflight {n})");
        let passes=[fail("4.6.26",3),fail("4.6.26",4),fail("4.6.27",3),"update: up to date".into(),"update: up to date".into()];
        let logged:Vec<_>=passes.iter().filter(|text|maintenance_once(&mut last,text)).map(String::as_str).collect();
        assert_eq!(logged,[fail("4.6.26",3).as_str(),&fail("4.6.27",3),"update: up to date"]);
    }

    // A stable owner identity must not cache policy changes or hide an interrupted write.
    #[test]
    fn owner_identity_keeps_policy_readback_fresh() {
        let root=std::env::temp_dir().join(format!("taskq-policy-reload-{}",crate::runtime::uuid()));fs::create_dir(&root).unwrap();
        let value=json!({"hosts":{"owner-identity":"mapped-owner"},"limits":{"codex":2}});
        fs::write(root.join("host.json"),value.to_string()).unwrap();
        let first=Policy::read_for_hostname(&root,||"owner-identity".into()).unwrap();
        if std::env::var("TASKQ_HOST").ok().filter(|s|!s.is_empty()).is_none() {assert_eq!(first.me,"mapped-owner");}
        migrate_for_hostname(&root,&[],"owner-identity").unwrap();
        let mut changed=value;changed["limits"]["codex"]=json!(0);fs::write(root.join("host.json"),changed.to_string()).unwrap();
        assert_eq!(Policy::read_for_hostname(&root,||"owner-identity".into()).unwrap().cap.limits["codex"],0);
        fs::write(root.join("host.json.pending"),"retained").unwrap();
        assert!(Policy::read_for_hostname(&root,||panic!("identity ran before pending-write refusal")).unwrap_err().contains("interrupted"));
        assert!(migrate_for_hostname(&root,&[],"owner-identity").is_err());fs::remove_file(root.join("host.json.pending")).unwrap();
        fs::write(root.join("host.json"),"{").unwrap();assert!(Policy::read_for_hostname(&root,||"owner-identity".into()).is_err());
        fs::remove_file(root.join("host.json")).unwrap();assert!(Policy::read_for_hostname(&root,||"owner-identity".into()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    // A5: a released admission hold must wake a project sleeping on its previous blocked pass, without editing focus.
    #[test]
    fn released_admission_hold_wakes_pending_work() {
        let root=temp("inventory-wake");let roots=vec![root.clone()];
        fs::create_dir_all(crate::dot(&root)).unwrap();
        fs::write(crate::dot(&root).join("focus"),"approved-label").unwrap();
        let paths=[crate::dot(&root).join("poke"),root.join("host-poke")];
        let baseline=crate::poke_times(&paths);
        let p=policy();let mut h=Host::new(Err("inventory pending".into()));
        for barrier in [Err::<(),String>("handoff: legacy service still running".into()),Err("legacy liveness unknown".into())] {
            h.reload_admission(barrier.clone().map(|_|p.clone()),barrier.err(),&roots);
            assert!(h.reserve(&root,1,1,"codex",&p.cap,true,0).is_err());
            assert_eq!(crate::poke_times(&paths),baseline,"blocked refresh cannot wake admission");
        }
        h.reload_admission(Ok(p.clone()),None,&roots);
        assert_ne!(crate::poke_times(&paths),baseline,"released hold must wake pending review/start");
        crate::sleep_until_poke(&paths,baseline,Instant::now()+std::time::Duration::from_secs(60),&mut ||panic!("wake was lost"));
        assert!(h.reserve(&root,1,1,"codex",&p.cap,true,0).is_ok());
        assert_eq!(fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"approved-label");
        fs::remove_dir_all(root).unwrap();
    }

    // An admission hold keeps valid maintenance intervals; unknown configuration keeps safe defaults.
    #[test]
    fn admission_hold_keeps_maintenance_intervals() {
        let mut configured=policy();configured.update_every=86400;configured.rollback_after=1200;
        let (blocked,timers)=guarded_policy(Ok(configured),&Err("handoff: legacy service still running".into()));
        assert!(blocked.unwrap_err().contains("still running"),"a live root owner still blocks admission");
        assert_eq!(timers,(86400,1200),"valid maintenance intervals survive an admission hold");
        let (invalid,timers)=guarded_policy(Err("host.json invalid".into()),&Ok(()));
        assert!(invalid.is_err());assert_eq!(timers,(60,600),"unknown configuration retains safe maintenance defaults");
    }

    // H-A4/H-A5: first launch waits for every project's occupancy, including unknown native ownership.
    #[test]
    fn cold_host_accounts_every_project_before_admission() {
        let p=policy();let mut h=Host::new(Ok(p.clone()));h.gate.configure(p.cap.gate);
        let a=PathBuf::from("a");let b=PathBuf::from("b");h.busy.insert(a.clone(),true);h.busy.insert(b.clone(),true);h.rebuilding=true;
        assert!(h.reserve(&a,1,0,"codex",&p.cap,true,0).is_err());
        assert!(!h.rebuild_slots(&a,vec![(1,7,"codex".into())]));
        assert_eq!(h.remaining(&a,"codex",&p.cap),0);assert!(h.reserve(&a,2,0,"codex",&p.cap,true,0).is_err());
        assert!(h.rebuild_slots(&b,vec![(2,8,"codex".into())]));assert!(!h.rebuilding);
        assert_eq!(h.used()["codex"],2);assert!(h.reserve(&a,3,0,"codex",&p.cap,true,0).is_err());
        h.sync(&b,vec![]);assert!(h.reserve(&a,3,0,"codex",&p.cap,true,0).is_ok());

        h.sync(&a,vec![(1,7,"codex".into())]);
        let c=PathBuf::from("later");h.attach(&c);
        assert_eq!(h.reserve(&a,4,0,"codex",&p.cap,true,30).unwrap_err(),"host occupancy recovery pending","registration may carry unresolved prior occupancy");
        assert!(!h.rebuild_slots(&a,vec![(1,7,"codex".into())]));
        assert!(h.rebuild_slots(&c,vec![(4,9,"codex".into())]));
        assert_eq!(h.used()["codex"],2);
        h.sync(&c,vec![]);h.busy.remove(&c);h.attach(&c);
        assert!(!h.rebuild_slots(&a,vec![(1,7,"codex".into())]),"re-registration cannot reuse the earlier root's recovery acknowledgment");
        assert!(h.rebuild_slots(&c,vec![]));assert!(!h.rebuilding);
    }

    // H-A5: the final handover barrier covers orphaned slots and closes before re-entry.
    #[test]
    fn handover_preparation_latches_only_after_every_owned_project() {
        let a=PathBuf::from("a");let b=PathBuf::from("b");
        let mut h=Host::new(Err("fixture".into()));
        assert!(h.begin_pass(&a));h.restart=true;h.end_pass(&a,true);
        h.sync(&b,vec![(1,7,"codex".into())]);
        assert!(!h.handoff_ready(),"an unrepresented slot is not an idle project");
        assert!(h.begin_pass(&a));assert!(!h.handoff_ready(),"a pass in flight holds the handover");
        h.end_pass(&a,true);assert!(h.begin_pass(&b));
        h.end_pass(&b,true);
        h.held=Some("registry unknown".into());assert!(!h.handoff_ready());
        h.held=None;assert!(h.handoff_ready());
        for _ in 0..3 {
            assert!(!h.begin_pass(&a));assert!(!h.begin_pass(&b));assert!(h.handoff_ready());
        }
        h.prepared.remove(&b);assert!(!h.handoff_ready(),"failed ownership cannot keep an old acknowledgment");
        assert!(h.begin_pass(&a));assert!(!h.handoff_ready());
        h.end_pass(&a,true);h.end_pass(&b,true);
        let (send,receive)=std::sync::mpsc::channel();
        h.updating=Some(std::thread::spawn(move || {receive.recv().map_err(|e|e.to_string())?;Ok(Maintenance::Update(None))}));
        assert!(!h.handoff_ready(),"an update owner must finish before image replacement");
        assert!(h.begin_pass(&a),"projects keep heartbeat/native monitoring while the update waits");
        h.end_pass(&a,true);send.send(()).unwrap();h.updating.take().unwrap().join().unwrap().unwrap();assert!(h.handoff_ready());
        h.stopping=true;
        assert!(!h.handoff_ready(),"Stop cannot reuse a CLI handover acknowledgment while a native slot remains");
        h.sync(&b,vec![]);assert!(!h.handoff_ready(),"Stop still waits for active project/operator ownership");
        for busy in h.busy.values_mut() {*busy=false;}
        assert!(h.handoff_ready(),"only the prepared host with no occupied work may stop");

    }
    // #1402: six projects whose passes overlap. Revoking preparation at each pass start left no moment with all
    // six prepared, so the host kept the old image. Risk: the handover waits on passes forever, or re-execs mid-pass.
    #[test]
    fn overlapping_passes_reach_the_handover() {
        let roots:Vec<_>=(0..6).map(|n|PathBuf::from(format!("project-{n}"))).collect();
        let mut h=Host::new(Err("fixture".into()));h.restart=true;
        for root in &roots {h.busy.insert(root.clone(),true);}
        for root in &roots {assert!(h.begin_pass(root));}
        // Staggered: each project ends a pass prepared and, before the others end, would start its next one.
        for (n,root) in roots.iter().enumerate() {
            h.end_pass(root,true);
            assert!(!h.handoff_ready(),"no re-exec while a pass is in flight");
            if n+1<roots.len() {assert!(h.begin_pass(root),"a project keeps passing while another has not reported");}
        }
        for root in &roots[..5] {
            assert_eq!(h.handoff_wait(),Some(crate::lifecycle::HandoffWait::Project));
            assert!(!h.begin_pass(&roots[5]),"every report prepared: no new pass starts");
            h.end_pass(root,true);assert!(!h.begin_pass(root));
        }
        assert!(h.handoff_ready());
        let mut h=Host::new(Err("fixture".into()));h.restart=true;
        for root in &roots {h.busy.insert(root.clone(),true);}
        for root in &roots {assert!(h.begin_pass(root));h.end_pass(root,true);}
        assert!(h.handoff_ready());
        h.prepared.remove(&roots[1]);assert!(h.begin_pass(&roots[0]),"an unprepared report lifts the barrier");
        assert!(h.begin_pass(&roots[0]),"a pass that never reported");assert!(!h.prepared.contains(&roots[0]),"leaves no acknowledgment");
        h.end_pass(&roots[0],true);assert!(h.begin_pass(&roots[1]));h.end_pass(&roots[1],false);
        assert!(!h.handoff_ready(),"an unprepared pass result keeps the old image");
    }

    // H-A3: idle projects must not revoke Stop preparation while another project is still draining.
    #[test]
    fn six_project_stop_retains_idle_preparation() {
        let roots:Vec<_>=(0..6).map(|n|PathBuf::from(format!("project-{n}"))).collect();
        let mut h=Host::new(Err("fixture".into()));h.restart=true;h.stopping=true;
        for root in &roots {h.busy.insert(root.clone(),true);}
        h.prepared.insert(roots[0].clone());h.sync(&roots[0],vec![(1,7,"codex".into())]);h.busy.insert(roots[0].clone(),false);
        assert!(h.begin_pass(&roots[0]),"an owned execution must continue to be monitored");
        h.sync(&roots[0],vec![]);h.end_pass(&roots[0],true);
        assert!(h.begin_pass(&roots[0]),"an active operator/project cannot freeze");
        for (n,root) in roots.iter().enumerate() {
            assert!(h.begin_pass(root));h.busy.insert(root.clone(),false);h.end_pass(root,true);
            if n+1<roots.len() {
                assert!(!h.handoff_ready());
                for ready in &roots[..=n] {assert!(!h.begin_pass(ready),"a drained project cannot reopen the Stop barrier");}
            }
        }
        assert!(h.handoff_ready());
    }

    // #1357 A4: live work elsewhere defers the handover. The host loop poked every project each second, so every
    // project listed the board each second (about 20k requests/h for six projects). The list stays at one per poll.
    #[test]
    fn deferred_handover_lists_the_board_once_per_poll() {
        use std::sync::atomic::{AtomicUsize,Ordering};
        let base=std::env::temp_dir().join(format!("taskq-deferred-handover-{}",crate::runtime::uuid()));
        let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(&root).unwrap();
        fs::write(install.join("token"),"owned-fixture-not-a-credential").unwrap();
        let root=root.canonicalize().unwrap();
        let shared=Arc::new(Mutex::new(Host::new(Err("fixture".into()))));
        {let mut h=shared.lock().unwrap();h.attach(&root);h.restart=true;h.sync(&base.join("busy"),vec![(1,7,"codex".into())]);}
        let lists=Arc::new(AtomicUsize::new(0));
        let project={let (root,install,host,lists)=(root.clone(),install.clone(),shared.clone(),lists.clone());std::thread::spawn(move || {
            enter(&root,&install);
            let mut cfg=crate::tests::cfg();cfg.root=root;cfg.install=install;cfg.poll=2;
            let board=crate::board::Fake::default();board.state.borrow_mut().lists=Some(lists);
            crate::board::Board::create(&board,"Waits for the handover","",&["taskq".into()])?;
            crate::service_run(&crate::Ctx {cfg,raw:json!({}),board:Box::new(board)},Some(host))
        })};
        // #1871: the first pass may run slow before its list (macOS gui probe: `log show`, `security`), so the 5 s
        // window starts at the first list. Retained protection: more than one list per poll, or a ready handover, fails.
        let mut until=None;
        let first=Instant::now()+std::time::Duration::from_secs(120);
        while until.is_none_or(|u|Instant::now()<u) {
            if shared.lock().unwrap().restart_wake() {crate::poke(&root).unwrap();}
            assert!(!shared.lock().unwrap().handoff_ready(),"the unrepresented slot defers the handover");
            if project.is_finished() {panic!("project loop ended: {:?}",project.join().unwrap());}
            if until.is_none() && lists.load(Ordering::SeqCst)>0 {until=Some(Instant::now()+std::time::Duration::from_secs(5));}
            assert!(until.is_some() || Instant::now()<first,"no board list within 120 s");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let listed=lists.load(Ordering::SeqCst);
        assert!((1..=4).contains(&listed),"5 s after the first list at poll 2 allows one wake and two polls; listed {listed}");
        shared.lock().unwrap().draining.insert(root.clone());crate::poke(&root).unwrap();
        assert_eq!(project.join().unwrap().unwrap_err(),"project drained");
        let _=fs::remove_dir_all(base);
    }

    // #1297 A1/A2: the defect was one detailed row per identity and status poll (63 MB/h live against 0.56 MB/h).
    // #1548: the tray tick's push, render and fit resize still stored two rows each (7 MB/h live); the tray tick fixture adds them.
    // Retained protection: six projects' hour of those polls through the bounded store stays under 384 MiB/720
    // inside repeating roots, the unmarked control fails the same gate, and no event is lost.
    // Span rows use the real clock, so the hour spans one or two of its four 15-minute windows: the gate scales the
    // measured growth to four windows. Health rows take the simulated clock (#1297 review: they materialized service.loop).
    // ponytail: root names copy main/tray.
    #[test]
    fn six_project_poll_hour_fits_store_budget() {
        use tracing_subscriber::prelude::*;
        let me=coreq::process::identity(std::process::id()).unwrap().unwrap();
        for folded in [false,true] {
            let install=temp(&format!("poll-hour-{folded}"));let _=fs::remove_dir_all(&install);fs::create_dir_all(install.join("telemetry.log")).unwrap();
            publish_service(&install,&me,"fixture").unwrap();
            let path=install.join("telemetry.log/universal.sqlite3");let budget=crate::telemetry::budget(&install,"telemetry.sqlite3").unwrap();
            let store=std::sync::Arc::new(coreq::telemetry::Store::open_bounded(&path,budget.clone()).unwrap());
            store.reclaim().unwrap();let empty=store.storage().unwrap()["total_bytes"].as_u64().unwrap();
            let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","test").unwrap();
            let dispatch=tracing::Dispatch::new(tracing_subscriber::registry().with(capture.clone()));
            let window=crate::telemetry::diagnostic::HEALTH_WINDOW_MS;
            let (first,simulated)=(crate::profile::now_ms().unwrap(),crate::profile::now_ms().unwrap()/window*window);
            // The tray module builds only on macOS and Windows; Linux hosts run no tray, so their hour has no tick rows.
            #[cfg(any(target_os = "macos", windows))]
            let mut tray=crate::tray::steady_tick();
            #[cfg(not(any(target_os = "macos", windows)))]
            let mut tray=||();
            tracing::dispatcher::with_default(&dispatch,|| for tick in 0..1800 {
                if tick%30==0 {
                    // The hour runs in seconds: let the writer catch up each simulated minute, as real pacing would, so a
                    // loaded runner never meets CoreQ's 8192-event queue cap (merge_group flake on Windows).
                    let until=std::time::Instant::now()+std::time::Duration::from_secs(60);
                    while capture.health()["queued_events"].as_u64().unwrap()>1024 {
                        assert!(std::time::Instant::now()<until,"writer stalled: {}",capture.health());std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    for project in 0..6 {
                        let project=format!("owner/project{project}");
                        let _repeating=tracing::info_span!("service.loop",coreq_repeating=folded,project=project.as_str()).entered();
                        assert!(!crate::hostname().is_empty());assert_eq!(service_status(&install)["state"],"running");
                        assert_eq!(crate::telemetry::diagnostic::health_at(Some(simulated+tick*2000))["lost"],0);
                    }
                }
                let _repeating=tracing::info_span!("tray.refresh",coreq_repeating=folded).entered();
                assert!(!crate::hostname().is_empty());assert_eq!(service_status(&install)["state"],"running");
                tray();
            });
            // #1339: folded aggregates persist only through finish() or the last Capture drop; another thread's
            // tracing interest rebuild can briefly own a Dispatch clone, so `late` makes that drop order deterministic.
            let late=dispatch.clone();
            capture.finish().unwrap();
            let health=capture.health();
            assert_eq!((&health["lost"],&health["failed_batches"]),(&json!(0),&json!(0)),"{health}");
            drop(dispatch);drop(capture);drop(store);
            let store=coreq::telemetry::Store::open_bounded(&path,budget).unwrap();store.reclaim().unwrap();
            let windows=crate::profile::now_ms().unwrap()/window-first/window+1;
            let bytes=(store.storage().unwrap()["total_bytes"].as_u64().unwrap()-empty)*4/windows;
            let limit=384*1024*1024/720;
            eprintln!("poll hour folded={folded}: {bytes} bytes over four windows; limit={limit}");
            assert_eq!(bytes<=limit,folded,"hourly growth gate must reject unfolded polling");
            if folded {
                let page=store.export(0,0,1024).unwrap();let rows=page["events"].as_array().unwrap();
                let rows:Vec<_>=rows.iter().map(|r|&r["observation"]).collect();
                // Per window: one health row and the one service.loop pass it materializes (start and end).
                let health=rows.iter().filter(|r|r["operation"]=="collector.health").count();
                let passes=rows.iter().filter(|r|r["operation"]=="taskq.service.loop"&&r["kind"]=="method").count() as u64;
                assert!((1..=4).contains(&health)&&passes==health as u64,"{page}");
                assert_eq!(rows.iter().filter(|r|r["kind"]!="method.aggregate").count(),health*3,"{page}");
                let calls=|op:&str|rows.iter().filter(|r|r["operation"]==op).filter_map(|r|r["calls"].as_u64()).sum::<u64>();
                assert_eq!((calls("taskq.tray.refresh"),calls("taskq.service.loop")+passes),(1800,360),"aggregates keep every poll");
                let counts=|op:&str|rows.iter().filter(|r|r["operation"]==op).filter_map(|r|r["counts"]["calls"].as_u64()).sum::<u64>();
                if cfg!(any(target_os = "macos", windows)) {assert_eq!((calls("taskq.operation.tray.web.tick"),counts("js.ui.render.tick")),(1800,1800),"{page}");}
            }
            drop(late);drop(store);fs::remove_dir_all(&install).unwrap();
        }
    }

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("taskq-host-{name}-{}", std::process::id()));
        fs::create_dir_all(&p).unwrap(); p
    }
    // Chain: § 9 `claim` (#1493). Risk: a typo silently makes a labelled host claim everything.
    #[test]
    fn claim_parses_any_or_labelled_only() {
        assert_eq!(policy().claim, ClaimMode::Any, "missing claim: any");
        assert_eq!(Policy::parse(json!({"claim": "labelled"}), "invented").unwrap().claim, ClaimMode::Labelled);
        assert_eq!(Policy::parse(json!({"claim": "any"}), "invented").unwrap().claim, ClaimMode::Any);
        assert!(Policy::parse(json!({"claim": "label"}), "invented").is_err());
    }
    // #2138 A2/A3/R13: invalid new policy must hold launches; defaults and lower-only overlays remain intact.
    #[test]
    fn balancing_policy_validates_modes_and_project_execution() {
        for (alias,mode) in [("primary",ClaimMode::Any),("any",ClaimMode::Any),("spillover",ClaimMode::Spillover),("pinned-only",ClaimMode::Labelled),("labelled",ClaimMode::Labelled),("manager-only",ClaimMode::None),("none",ClaimMode::None)] {
            assert_eq!(Policy::parse(json!({"claim":alias,"spillover_grace":0}),"invented").unwrap().claim,mode);
        }
        assert_eq!(policy().spillover_grace,300);
        for invalid in [json!({"spillover_grace":-1}),json!({"spillover_grace":"5"}),json!({"claim":true}),json!({"claim":"primari"})] {assert!(Policy::parse(invalid,"invented").is_err());}
        for entry in [json!({"default_runtime":"unknown"}),json!({"default_runtime":"codex-app"}),json!({"auto_reroute":1}),json!({"allowed_runtimes":null}),json!({"allowed_runtimes":["codex","codex"]}),json!({"allowed_runtimes":["unknown"]}),json!({"limits":null}),json!({"auto_reroute":false,"unexpected":true})] {
            assert!(Policy::parse(json!({"projects":{"/invented":entry}}),"invented").is_err());
        }
        for entry in [json!({"default_runtime":"codex"}),json!({"allowed_runtimes":[]}),json!({"auto_reroute":true})] {
            let p=Policy::parse(json!({"projects":{"/invented":entry}}),"invented").unwrap();
            assert!(p.overlay(Path::new("/other")).is_none());
            assert_ne!(p.project_revision(Path::new("/invented"),&json!({})).unwrap(),policy().project_revision(Path::new("/invented"),&json!({})).unwrap());
        }
    }
    // Chain: § 9 `default_runtime` (#1668). Risk: a host without the key stops running reviews on claude,
    // or a typo or `codex-app` silently becomes the runtime of every review on the host.
    #[test]
    fn default_runtime_parses_cli_subscriptions_only() {
        assert_eq!(policy().default_runtime, "claude", "missing key: claude");
        for rt in crate::runtime::CLI_RUNTIMES {
            assert_eq!(Policy::parse(json!({"default_runtime": rt}), "invented").unwrap().default_runtime, rt);
        }
        for bad in [json!("codex-app"), json!("Codex"), json!(""), json!(1)] {
            assert_eq!(Policy::parse(json!({"default_runtime": bad}), "invented").unwrap_err(), "host.json: default_runtime must be claude or codex or antigravity");
        }
    }
    fn policy() -> Policy {
        Policy::parse(json!({"limits": {"codex": 2}, "admission": {"max": 2}, "load_gate": {"enabled": false}}), "invented").unwrap()
    }

    // H-A5: native owner + exact host generation acknowledge install; project PID or freshness alone cannot.
    #[test]
    fn installation_observed_requires_current_native_owner_and_project_generation() {
        let service=json!({"state":"running","fresh":true,"version":crate::platform::VERSION,"pid":123,"instance":"owned-host"});
        let project=json!({"pid":123,"host_instance":"owned-host","complete":true,"tasks":[]});
        assert!(installation_observed(&service,Some(&project),crate::platform::VERSION));
        for (key,value) in [("state",json!("unknown")),("fresh",json!(false)),("version",json!("older")),("pid",json!(0)),("instance",json!(""))] {
            let mut other=service.clone();other[key]=value;assert!(!installation_observed(&other,Some(&project),crate::platform::VERSION),"{key}");
        }
        for other in [None,Some(json!({"pid":124,"host_instance":"owned-host"})),Some(json!({"pid":123,"host_instance":"prior-host"}))] {
            assert!(!installation_observed(&service,other.as_ref(),crate::platform::VERSION));
        }
    }

    // H-A5: refusal precedes native installation, focus changes, release replacement and registry publication.
    #[test]
    fn host_install_refuses_another_live_root_owner_without_effects() {
        let base=temp("install-legacy-owner");let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(crate::dot(&root)).unwrap();
        fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
        fs::write(crate::dot(&root).join("focus"),"retained-focus").unwrap();
        let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","runtime::tests::stop_child","--ignored"])
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        fs::write(crate::dot(&root).join("status.json"),json!({"pid":child.id(),"kids":[{"task":7}]}).to_string()).unwrap();
        let before=fs::read(install.join("projects")).unwrap();
        let result=installation::install_bound(&install,&root,None);
        child.kill().unwrap();child.wait().unwrap();
        assert!(result.unwrap_err().contains("another live service owns a registered root"));
        assert_eq!(fs::read(install.join("projects")).unwrap(),before);
        assert_eq!(fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"retained-focus");
        assert!(!install.join("releases").exists()&&!install.join("poke").exists());
        fs::write(install.join("service"),"{broken").unwrap();
        assert!(installation::install_bound(&install,&root,None).unwrap_err().contains("malformed"));
        assert_eq!(fs::read(install.join("projects")).unwrap(),before);
        fs::remove_dir_all(base).unwrap();
    }

    // H-A5: one editor preserves unrelated roots/permissions, rejects interrupted state and removes canonical aliases.
    #[test]
    fn registry_editor_preserves_roots_and_refuses_unknown() {
        let base=temp("registry-editor");let install=base.join("install");let a=base.join("a");let b=base.join("b");
        for path in [&install,&a,&b] {fs::create_dir_all(path).unwrap();}
        assert!(ProjectEdit::new(&install,&a,false).is_err(),"only registration initializes a missing registry");
        ProjectEdit::new(&install,&a,true).unwrap().commit().unwrap();
        let path=install.join("projects");
        let before=fs::read_to_string(&path).unwrap();
        assert_eq!(before,format!("{}\n",crate::platform::plain_path(&a.canonicalize().unwrap()).display()));
        assert!(install.join("poke").exists());
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&path,fs::Permissions::from_mode(0o640)).unwrap();}
        let owner=ownership(&path).unwrap();
        ProjectEdit::new(&install,&b,true).unwrap().commit().unwrap();
        assert_eq!(ownership(&path).unwrap(),owner);
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;assert_eq!(fs::metadata(&path).unwrap().permissions().mode()&0o777,0o640);}
        let saved=fs::read(&path).unwrap();
        let mut registry=Registry::default();registry.refresh(&install,0);
        let roots=registry.roots.clone();
        fs::write(install.join("projects.pending"),"interrupted").unwrap();
        registry.refresh(&install,60);
        assert!(registry.error.is_some() && registry.roots==roots);
        assert!(ProjectEdit::new(&install,&a,false).is_err());
        assert_eq!(fs::read(&path).unwrap(),saved);
        fs::remove_file(install.join("projects.pending")).unwrap();
        let alias=a.join(".");
        fs::write(&path,format!("{}{}\n{}\n",String::from_utf8(saved).unwrap(),alias.display(),b.display())).unwrap();
        ProjectEdit::new(&install,&a,false).unwrap().commit().unwrap();
        assert_eq!(projects(&install).unwrap(),vec![b.canonicalize().unwrap()]);
        assert_eq!(project_paths(&install).unwrap().len(),2,"unrelated configured spelling is retained");
        fs::write(&path,b"\xff").unwrap();
        assert!(ProjectEdit::new(&install,&a,true).is_err());
        assert_eq!(fs::read(&path).unwrap(),b"\xff");
        fs::remove_dir_all(base).unwrap();
    }

    // Risk (#1858): Windows registration stored `\\?\C:\...` beside older plain entries, so path and trust checks missed.
    #[cfg(windows)]
    #[test]
    fn windows_registry_stores_plain_roots_and_matches_verbatim_entries() {
        let base=temp("registry-plain");let install=base.join("install");let a=base.join("a");let b=base.join("b");
        for path in [&install,&a,&b] {fs::create_dir_all(path).unwrap();}
        let verbatim=|p:&Path|{let c=p.canonicalize().unwrap();assert!(c.to_str().unwrap().starts_with(r"\\?\"));c};
        let plain=|p:&Path|crate::platform::plain_path(&verbatim(p));
        ProjectEdit::new(&install,&verbatim(&a),true).unwrap().commit().unwrap();
        let path=install.join("projects");
        assert_eq!(fs::read_to_string(&path).unwrap(),format!("{}\n",plain(&a).display()),"stored without the verbatim prefix");
        // an older prefixed entry and a plain one of the same root resolve to one project, and re-adding writes nothing
        fs::write(&path,format!("{}\n{}\n{}\n",verbatim(&a).display(),plain(&a).display(),plain(&b).display())).unwrap();
        assert_eq!(projects(&install).unwrap(),vec![verbatim(&a),verbatim(&b)]);
        let before=fs::read(&path).unwrap();
        ProjectEdit::new(&install,&plain(&a),true).unwrap().commit().unwrap();
        assert_eq!(fs::read(&path).unwrap(),before);
        ProjectEdit::new(&install,&plain(&a),false).unwrap().commit().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(),format!("{}\n",plain(&b).display()),"removal drops both spellings");
        fs::remove_dir_all(base).unwrap();
    }

    // H-A1: an owner before PID publication excludes a second process, survives sweep, and releases on exit.
    #[test]
    fn one_host_owner_before_heartbeat_publication() {
        let base=temp("host-owner");let install=base.join("install");fs::create_dir(&install).unwrap();
        let (first,second)=(base.join("first"),base.join("second"));
        let mut holder=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::owner_probe","--ignored"])
            .env("TASKQ_OWNER_INSTALL",&install).env("TASKQ_OWNER_MARKER",&first).env("TASKQ_OWNER_HOLD","1")
            .stdout(std::process::Stdio::null()).stderr(fs::File::create(base.join("owner.stderr")).unwrap()).spawn().unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(5);
        while !first.is_file() && holder.try_wait().unwrap().is_none() && Instant::now()<until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let entered=fs::read_to_string(&first).unwrap_or_default();
        crate::platform::sweep(&install,&base.join("absent-shim"));
        let probe=coreq::process::output("test.host-owner",std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::owner_probe","--ignored"])
            .env("TASKQ_OWNER_INSTALL",&install).env("TASKQ_OWNER_MARKER",&second).env_remove("TASKQ_OWNER_HOLD"),
            std::time::Duration::from_secs(5));
        if holder.try_wait().unwrap().is_none() {holder.kill().unwrap();holder.wait().unwrap();}
        let observed=fs::read_to_string(&second).unwrap_or_default();
        let released=owner(&install).unwrap();let freed=released.is_some();
        drop(released);
        let diagnostic=fs::read_to_string(base.join("owner.stderr")).unwrap_or_default();
        fs::remove_dir_all(base).unwrap();
        assert_eq!(entered,"claimed","first owner never reached the controlled startup boundary: {diagnostic}");
        probe.unwrap();
        assert_eq!(observed,"blocked","two processes claimed the host before its PID was published");
        assert!(freed,"native owner exit did not release ownership");
    }

    // H-A1/H-A5: exact native incarnation, launch generation and observation age have independent meanings.
    #[test]
    fn host_owner_projection_separates_identity_generation_and_freshness() {
        let install=temp("owner-observation");
        let marker=install.join("published");
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::owner_probe","--ignored"])
            .env("TASKQ_OWNER_INSTALL",&install).env("TASKQ_OWNER_MARKER",&marker).env("TASKQ_OWNER_PUBLISH","first")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(5);
        while !marker.exists()&&Instant::now()<until&&child.try_wait().unwrap().is_none() {std::thread::yield_now();}
        assert_eq!(fs::read_to_string(marker).unwrap(),"claimed");
        let native=coreq::process::identity(child.id()).unwrap().unwrap();
        let status=service_status(&install);
        assert_eq!(status["state"],"running");assert_eq!(status["fresh"],true);assert_eq!(status["instance"],"first");
        assert!(service_line(&status).contains("running (host, pid"));
        assert_eq!(other(&install).unwrap(),Some(child.id()));
        let original=fs::read_to_string(install.join("service")).unwrap();
        let mut record:Value=serde_json::from_str(&original).unwrap();
        for at in [wall().unwrap()-120,wall().unwrap()+120] {
            record["at"]=json!(at);fs::write(install.join("service"),record.to_string()).unwrap();
            let stale=service_status(&install);assert_eq!(stale["state"],"running");assert_eq!(stale["fresh"],false);
            assert!(service_line(&stale).contains("observation stale or unknown"));
            assert_eq!(other(&install).unwrap(),Some(child.id()),"stale/future projection released a live owner");
        }
        publish_service(&install,&native,"second").unwrap();
        assert_eq!(service_status(&install)["instance"],"second","same native PID may have a new host launch");
        record=serde_json::from_str(&original).unwrap();record["started"]=json!(format!("{}-older",native.started));
        fs::write(install.join("service"),record.to_string()).unwrap();
        assert_eq!(service_status(&install)["state"],"exited","another incarnation must not look like the recorded daemon");
        assert_eq!(other(&install).unwrap(),None);assert!(coreq::process::identity(child.id()).unwrap().is_some());
        assert!(owner(&install).unwrap().is_none(),"record mismatch bypassed the native ownership lock");
        record["schema"]=json!(2);
        fs::write(install.join("service"),record.to_string()).unwrap();
        assert_eq!(service_status(&install)["state"],"unknown");assert_eq!(other(&install).unwrap(),None,"another incarnation's unreadable record kept the host waiting");
        record["started"]=json!(native.started);
        for text in [record.to_string(),"{".into(),"x".repeat(1025)] {
            fs::write(install.join("service"),&text).unwrap();
            assert_eq!(service_status(&install)["state"],"unknown");assert!(other(&install).is_err());
            assert_eq!(fs::read_to_string(install.join("service")).unwrap(),text);
        }
        fs::write(install.join("service"),child.id().to_string()).unwrap();
        assert_eq!(service_status(&install)["state"],"unknown","PID-only compatibility cannot certify a native host owner");
        assert_eq!(other(&install).unwrap(),Some(child.id()));
        publish_service(&install,&native,"second").unwrap();
        child.kill().unwrap();child.wait().unwrap();
        assert_eq!(service_status(&install)["state"],"exited");assert_eq!(other(&install).unwrap(),None);
        // #1310: a newer release's record whose process exited no longer holds an older host forever.
        let mut newer:Value=serde_json::from_str(&fs::read_to_string(install.join("service")).unwrap()).unwrap();
        newer["stop_protocol"]=json!(3);let mut extra=newer.clone();extra["future_protocol"]=json!(1);
        for text in [newer.to_string(),extra.to_string()] {
            fs::write(install.join("service"),&text).unwrap();
            assert_eq!(service_status(&install)["state"],"unknown");assert_eq!(other(&install).unwrap(),None,"exited unreadable record kept the host waiting");
            assert_eq!(fs::read_to_string(install.join("service")).unwrap(),text);
        }
        // R12: corrupt identities cannot prove exit, even when their PID has exited.
        for started in ["invalid start time!".to_owned(), "x".repeat(129), String::new()] {
            newer["started"]=json!(started);
            let text=newer.to_string();
            fs::write(install.join("service"),&text).unwrap();
            assert!(other(&install).is_err(),"corrupt identity released ownership: {started:?}");
            assert_eq!(fs::read_to_string(install.join("service")).unwrap(),text);
        }
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    #[ignore="owned native process for one_host_owner_before_heartbeat_publication"]
    fn owner_probe() {
        let install=PathBuf::from(std::env::var_os("TASKQ_OWNER_INSTALL").unwrap());
        let claim=owner(&install).unwrap();
        let marker=PathBuf::from(std::env::var_os("TASKQ_OWNER_MARKER").unwrap());
        if claim.is_some()&&let Ok(instance)=std::env::var("TASKQ_OWNER_PUBLISH") {
            use std::io::Read;
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            publish_service(&install,&native,&instance).unwrap();
            crate::platform::write_atomic(&marker,"claimed").unwrap();
            let _=std::io::stdin().read(&mut [0]); // Parent pipe closure also retires a failed fixture.
            return;
        }
        crate::platform::write_atomic(&marker,if claim.is_some(){"claimed"}else{"blocked"}).unwrap();
        if std::env::var_os("TASKQ_OWNER_HOLD").is_some() && claim.is_some() {loop {std::thread::park();}}
    }

    // H-A3: committed policy and its original command survive writer death, even before the applied receipt.
    #[test]
    fn acknowledged_policy_survives_writer_termination() {
        for phase in ["committed","before-applied"] {
        let root=temp(&format!("policy-commit-{phase}"));
        fs::write(root.join("host.json"),json!({"limits":{"codex":2},"owner_field":"preserve"}).to_string()).unwrap();
        let mut child=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::tests::policy_writer","--ignored"])
            .env("TASKQ_HOST_POLICY_FIXTURE",&root).env("TASKQ_HOST_POLICY_PHASE",phase)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let until=Instant::now()+std::time::Duration::from_secs(10);
        while !root.join("ack").is_file() && child.try_wait().unwrap().is_none() && Instant::now()<until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let acknowledged=fs::read_to_string(root.join("ack")).unwrap_or_default();
        if child.try_wait().unwrap().is_none() {child.kill().unwrap();child.wait().unwrap();}
        assert_eq!(acknowledged,if phase=="committed" {"committed"} else {"policy-saved"},"writer did not reach {phase}");
        let read=Policy::read(&root).unwrap();assert_eq!(read.cap.limits["codex"],3);assert_eq!(read.raw["owner_field"],"preserve");
        let receipt=read.command().unwrap().unwrap();
        let before=fs::read(root.join("host.json")).unwrap();
        let (capture,guard)=crate::profile::tests::collecting(&root);
        assert!(!capacity_observed(&root,&receipt).unwrap());
        assert_eq!(observe_capacity(&root,&read).unwrap(),Some(receipt.event_id("observed").unwrap()));
        assert!(observed_soon(&root,&receipt));
        assert_eq!(fs::read(root.join("host.json")).unwrap(),before,"recovery replayed a settings mutation");
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    #[ignore="owned subprocess used by acknowledged_policy_survives_writer_termination"]
    fn policy_writer() {
        let root=PathBuf::from(std::env::var_os("TASKQ_HOST_POLICY_FIXTURE").unwrap());
        let (_capture,_guard)=crate::profile::tests::collecting(&root);
        let owner=Policy::read(&root).unwrap().me;
        if let Ok(request)=std::env::var("TASKQ_HOST_POLICY_REQUEST") {
            local_capacity(&root,&owner,&serde_json::from_str(&request).unwrap()).unwrap();
        } else {write_capacity(&root,&owner,&crate::CapEdit {limits:BTreeMap::from([("codex".into(),3)]),..Default::default()},None).unwrap();}
        crate::platform::write_atomic(&root.join("ack"),"committed").unwrap();
        loop {std::thread::park();}
    }

    fn local_fixture(name:&str)->(PathBuf,PathBuf,String,Value) {
        let base=temp(name);let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(&root).unwrap();
        fs::write(install.join("host.json"),"{\"limits\":{\"codex\":2}}").unwrap();
        fs::write(root.join("taskq.json"),"{\"repo\":\"fixture/project\"}").unwrap();
        fs::write(install.join("projects"),format!("{}\n",root.display())).unwrap();
        let p=Policy::read(&install).unwrap();let now=wall().unwrap();
        let request=json!({"id":"local-capacity","issued_at":now,"expires_at":now+300,"ops":[{
            "target":{"installation_id":null,"generation":1,"project_id":null,"runtime_id":"codex"},
            "policy_schema":"taskq.host-capacity/1","expected_revision":p.revision().unwrap(),"changes":{"runtime_limit":3}}]});
        (base,install,p.me,request)
    }

    // Receipt commits run beside the operation (#1351): a reader polls within coreq's append deadline.
    fn observed_soon(root:&Path,receipt:&CommandReceipt)->bool {
        crate::telemetry::settle(std::time::Duration::from_secs(30));
        let until=Instant::now()+std::time::Duration::from_secs(15);
        while Instant::now()<until {if capacity_observed(root,receipt).unwrap() {return true;} std::thread::sleep(std::time::Duration::from_millis(20));}
        false
    }

    // #1351: a stalled telemetry writer (budget lock held) cannot delay or fail a limits save; a refused save keeps the old limits.
    #[test]
    fn local_capacity_saves_while_telemetry_is_stalled() {
        let (base,install,owner,request)=local_fixture("local-stalled");
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let budget=coreq::file_lock::lock(&install.join("taskq-profile.lock")).unwrap();
        let started=Instant::now();
        let saved=local_capacity(&install,&owner,&request).unwrap();
        assert_eq!(saved["state"],"saved","{saved}");
        assert!(started.elapsed()<std::time::Duration::from_secs(3),"save waited {:?} on telemetry",started.elapsed());
        assert_eq!(Policy::read(&install).unwrap().cap.limits["codex"],3);
        let bytes=fs::read(install.join("host.json")).unwrap();
        let mut stale=request.clone();stale["id"]=json!("stale");stale["ops"][0]["changes"]["runtime_limit"]=json!(5);
        let started=Instant::now();
        assert_eq!(local_capacity(&install,&owner,&stale).unwrap()["state"],"rejected");
        assert!(started.elapsed()<std::time::Duration::from_secs(3));
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"refused save changed policy");
        drop(budget);capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
    }

    // #1351 A3: a held host lock turns the save into a bounded "Not saved" with the old limits kept; the same request saves after release.
    #[test]
    fn local_capacity_refuses_within_bound_while_host_lock_is_held() {
        let (base,install,owner,request)=local_fixture("local-locked");
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let bytes=fs::read(install.join("host.json")).unwrap();
        fs::create_dir_all(crate::resources::dir(&install)).unwrap();
        let held=coreq::file_lock::lock(&crate::resources::dir(&install).join("lock")).unwrap();
        let started=Instant::now();
        let refused=local_capacity(&install,&owner,&request).unwrap();
        assert_eq!(refused["state"],"rejected","{refused}");
        assert!(refused["reason"].as_str().unwrap().contains("old limits kept"),"{refused}");
        assert!(started.elapsed()<LOCAL_LOCK_WAIT+std::time::Duration::from_secs(2),"save waited {:?} on the host lock",started.elapsed());
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"refused save changed policy");
        drop(held);
        assert_eq!(local_capacity(&install,&owner,&request).unwrap()["state"],"saved");
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
    }

    // H-A2: immutable same-ID replay, current revision and typed local scope hold before policy effects.
    #[test]
    fn local_capacity_request_replays_without_mutation_and_refuses_foreign_scope() {
        let (base,install,owner,request)=local_fixture("local-request");
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let saved=local_capacity(&install,&owner,&request).unwrap();assert_eq!(saved["state"],"saved");
        let bytes=fs::read(install.join("host.json")).unwrap();
        assert_eq!(local_capacity(&install,&owner,&request).unwrap(),saved);
        for change in ["payload","stale","expired","installation","generation","project","runtime","schema","type","future","lifetime","empty","extra","large"] {
            let mut other=request.clone();
            if change!="payload" {other["id"]=json!(change);}
            match change {
                "payload"=>other["ops"][0]["changes"]["runtime_limit"]=json!(4),
                "expired"=>{let now=wall().unwrap();other["issued_at"]=json!(now-301);other["expires_at"]=json!(now-1);}
                "installation"=>other["ops"][0]["target"]["installation_id"]=json!("a".repeat(64)),
                "generation"=>other["ops"][0]["target"]["generation"]=json!(2),
                "project"=>other["ops"][0]["target"]["project_id"]=json!("project"),
                "runtime"=>other["ops"][0]["target"]["runtime_id"]=json!("unknown"),
                "schema"=>other["ops"][0]["policy_schema"]=json!("taskq.capacity/1"),
                "type"=>other["ops"][0]["changes"]["runtime_limit"]=json!(true),
                "future"=>{let now=wall().unwrap();other["issued_at"]=json!(now+60);other["expires_at"]=json!(now+300);}
                "lifetime"=>other["expires_at"]=json!(request["issued_at"].as_u64().unwrap()+301),
                "empty"=>other["ops"]=json!([]),
                "extra"=>other["set"]=json!({"codex":4}),
                "large"=>other["ops"][0]["changes"]["runtime_limit"]=json!("x".repeat(16<<10)),
                _=>{},
            }
            let result=local_capacity(&install,&owner,&other);
            assert!(result.is_err()||result.unwrap()["state"]=="rejected","{change} accepted");
            assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"{change} changed policy");
        }
        let current=Policy::read(&install).unwrap();observe_capacity(&install,&current).unwrap();
        assert_eq!(local_capacity(&install,&owner,&request).unwrap()["saved_revision"],current.revision().unwrap());
        let mut newer=request.clone();newer["id"]=json!("newer");newer["ops"][0]["expected_revision"]=json!(current.revision().unwrap());
        let next=local_capacity(&install,&owner,&newer).unwrap();assert_eq!(next["state"],"saved");assert_ne!(next["saved_revision"],saved["saved_revision"]);
        let bytes=fs::read(install.join("host.json")).unwrap();
        assert_eq!(local_capacity(&install,&owner,&request).unwrap(),saved,"a later command must not change the original receipt");
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes);
        crate::resources::with(&install,|state| {
            let op=state.ops["local-capacity"].clone();
            for n in state.ops.len()..crate::resources::MAX_OPS {state.ops.insert(format!("retained-{n}"),op.clone());}
            Ok(())
        }).unwrap();
        newer["id"]=json!("store-full");newer["ops"][0]["expected_revision"]=json!(Policy::read(&install).unwrap().revision().unwrap());
        assert_eq!(local_capacity(&install,&owner,&newer).unwrap()["state"],"rejected");
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes);
        fs::write(install.join("resource-state/state.json"),"{").unwrap();
        assert!(local_capacity(&install,&owner,&newer).unwrap_err().contains("file kept"));
        assert_eq!(fs::read_to_string(install.join("resource-state/state.json")).unwrap(),"{");
        assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes);
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
    }

    // H-A2/H-A3: crash before effect remains unknown; crash after replacement recovers the same receipt read-only.
    #[test]
    fn local_capacity_crashes_never_replay_an_uncertain_write() {
        for phase in ["before-write","before-applied"] {
            let (base,install,owner,request)=local_fixture(&format!("local-crash-{phase}"));
            let mut child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","host::tests::policy_writer","--ignored"]).env("TASKQ_HOST_POLICY_FIXTURE",&install)
                .env("TASKQ_HOST_POLICY_REQUEST",request.to_string()).env("TASKQ_HOST_POLICY_PHASE",phase)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
            let until=Instant::now()+std::time::Duration::from_secs(10);
            while !install.join("ack").exists() && child.try_wait().unwrap().is_none() && Instant::now()<until {std::thread::sleep(std::time::Duration::from_millis(5));}
            let signal=fs::read_to_string(install.join("ack")).unwrap_or_default();let _=child.kill();child.wait().unwrap();
            assert_eq!(signal,if phase=="before-write" {"intent-saved"}else{"policy-saved"});
            let bytes=fs::read(install.join("host.json")).unwrap();
            let (capture,guard)=crate::profile::tests::collecting(&install);
            let result=local_capacity(&install,&owner,&request).unwrap();
            assert_eq!(result["state"],if phase=="before-write" {"unknown"}else{"saved"},"{phase}: {result}");
            assert_eq!(fs::read(install.join("host.json")).unwrap(),bytes,"recovery rewrote policy at {phase}");
            if phase=="before-applied" {
                let p=Policy::read(&install).unwrap();assert_eq!(observe_capacity(&install,&p).unwrap(),result["saved_revision"].as_str().map(String::from));
            }
            capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(base).unwrap();
        }
    }

    // H-A2: identical settings have different command identities; stale/manual readback cannot confirm a new edit.
    #[test]
    fn capacity_receipts_follow_the_loaded_command_not_equal_values() {
        let root=temp("capacity-receipt");
        fs::write(root.join("host.json"),"{\"limits\":{\"codex\":2}}").unwrap();
        let owner=Policy::read(&root).unwrap().me;
        let edit=crate::CapEdit {limits:BTreeMap::from([("codex".into(),3)]),gate:Some(false),..Default::default()};
        assert!(write_capacity(&root,&owner,&edit,None).is_err(),"missing required receipts must refuse before effect");
        assert_eq!(Policy::read(&root).unwrap().cap.limits["codex"],2);
        let (capture,guard)=crate::profile::tests::collecting(&root);
        let (_,_,first)=write_capacity(&root,&owner,&edit,None).unwrap();
        let loaded=Policy::read(&root).unwrap();
        assert!(!capacity_observed(&root,&first).unwrap());
        let (_,_,second)=write_capacity(&root,&owner,&edit,None).unwrap();
        let current=Policy::read(&root).unwrap();
        assert_eq!(loaded.cap,current.cap);assert_ne!(first.encode(),second.encode());
        assert!(observe_capacity(&root,&loaded).is_err(),"late old loader confirmed a different command");
        assert!(!capacity_observed(&root,&second).unwrap());
        assert_eq!(observe_capacity(&root,&current).unwrap(),Some(second.event_id("observed").unwrap()));
        assert!(observed_soon(&root,&second));
        assert!(capacity_observed(&root,&first).is_err());
        let mut edited=current.raw.clone();edited["limits"]["codex"]=json!(4);
        fs::write(root.join("host.json"),edited.to_string()).unwrap();
        assert!(observe_capacity(&root,&Policy::read(&root).unwrap()).is_err(),"manual edit borrowed old receipt");
        for invalid in [json!({"schema":2}),json!({"schema":1,"host":owner,"policy":current.cap.json(),"receipt":"{}"})] {
            edited[CAPACITY_COMMAND]=invalid;
            let bytes=edited.to_string();fs::write(root.join("host.json"),&bytes).unwrap();
            assert!(Policy::read(&root).is_err());assert!(write_capacity(&root,&owner,&edit,None).is_err());
            assert_eq!(fs::read_to_string(root.join("host.json")).unwrap(),bytes);
        }
        capture.flush().unwrap();drop(guard);drop(capture);fs::remove_dir_all(root).unwrap();
    }

    // Chain: published project passes -> host release qualification; failure keeps owned work and recovery can qualify.
    #[test]
    fn release_health_requires_current_successful_publications() {
        let root=std::env::temp_dir().join(format!("taskq-publication-{}",std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let tool=crate::platform::tool(&root.join("install"));
        let host=Arc::new(Mutex::new(Host::new(Ok(policy()))));
        let roots=vec![root.clone()];
        host.lock().unwrap().sync(&root,vec![(1,99,"codex".into())]);
        let mut status=coreq::status::common(&tool,"fixture","project",1,None,None);
        status["complete"]=json!(true);status["tasks"]=json!([]);
        assert!(!host.lock().unwrap().publications_ready(&roots));
        assert!(crate::publish_status(&tool,&root,&status,Some(&host)));
        assert!(host.lock().unwrap().publications_ready(&roots));
        let path=coreq::status::path(&tool,&root);
        assert_eq!(crate::data::json(&path).unwrap().unwrap()["host_instance"],host.lock().unwrap().instance);
        fs::remove_file(&path).unwrap();fs::create_dir(&path).unwrap();
        assert!(!crate::publish_status(&tool,&root,&status,Some(&host)));
        assert!(!host.lock().unwrap().publications_ready(&roots));
        assert_eq!(host.lock().unwrap().slots.len(),1,"publication must not lose child ownership");
        fs::remove_dir(&path).unwrap();
        assert!(crate::publish_status(&tool,&root,&status,Some(&host)));
        for failed in [json!({"complete":false,"tasks":[]}),json!({"complete":true,"tasks":[],"error":"board unavailable"}),
            json!({"complete":true,"tasks":[],"focus_error":"focus command pending"})] {
            assert!(!crate::publish_status(&tool,&root,&failed,Some(&host)));
            assert_eq!(crate::data::json(&path).unwrap().unwrap()["host_instance"],host.lock().unwrap().instance,"failed status lost its owner generation");
            assert!(!host.lock().unwrap().publications_ready(&roots));
        }
        assert!(crate::publish_status(&tool,&root,&status,Some(&host)));
        let mut h=host.lock().unwrap();
        h.published.insert(root.clone(),Instant::now()-std::time::Duration::from_secs(61));
        assert!(!h.publications_ready(&roots),"old evidence cannot qualify a blocked thread");
        h.publication(&root,true);
        assert!(!h.publications_ready(&[root.clone(),root.join("new-project")]));
        h.held=Some("registry unreadable".into());assert!(!h.publications_ready(&roots));
        h.held=None;assert!(h.publications_ready(&roots));
        h.publication(&root,false);assert!(!h.publications_ready(&roots),"thread exit invalidates evidence");
        drop(h);fs::remove_dir_all(root).unwrap();
    }

    // Chain A3: grant -> accepted attempt -> pending/running union -> verified exit -> new grant.
    // Risk: double-counting retained grants, or cross-project equal numbers colliding and exceeding host ceilings.
    #[test]
    fn host_slots_survive_pending_running_and_project_identity() {
        let p = policy(); let cap = p.project(Path::new("/p"), &json!({"limits": {"codex": 2}})).unwrap();
        let mut h = Host::new(Ok(p));
        let (a, b) = (Path::new("a"), Path::new("b"));
        assert_eq!(h.reserve(a, 1, 0, "codex", &cap, false, 0), Ok(1));
        h.claimed(a, 1, 77);
        assert_eq!(h.sync(a, vec![(1, 77, "codex".into()), (1, 77, "codex".into())]), 1);
        assert_eq!(h.reserve(b, 1, 77, "codex", &cap, false, 30), Ok(2));
        assert_eq!(h.sync(b, vec![(1, 77, "codex".into()), (1, 77, "codex".into())]), 2);
        assert!(h.reserve(a, 2, 78, "codex", &cap, false, 60).is_err());
        assert_eq!(h.sync(a, vec![]), 1); // only a verified exit releases the attempt
        assert_eq!(h.reserve(a, 2, 78, "codex", &cap, false, 60), Ok(2));
        h.sync(a, vec![]); h.sync(b, vec![]);
        let lower = h.policy.as_ref().unwrap().project(Path::new("/p"), &json!({"limits": {"codex": 1}})).unwrap();
        assert!(h.reserve(a, 1, 80, "codex", &lower, false, 90).is_ok());
        assert!(h.reserve(a, 2, 81, "codex", &lower, false, 120).is_err());
        assert!(h.reserve(b, 2, 81, "codex", &cap, false, 120).is_ok());
    }

    // Chain A3: simultaneous project requests serialize through the same authority.
    // Risk: both threads observe the final free slot and grant it; failed contenders must not reserve anything.
    #[test]
    fn concurrent_grants_never_exceed_host_limit_and_round_robin_rotates() {
        let h = Arc::new(Mutex::new(Host::new(Ok(policy()))));
        let cap = policy().cap;
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let joins: Vec<_> = ["a", "b"].into_iter().map(|root| {
            let (h, cap, barrier) = (h.clone(), cap.clone(), barrier.clone());
            std::thread::spawn(move || { barrier.wait(); h.lock().unwrap().reserve(Path::new(root), 1, 1, "codex", &cap, false, 0).is_ok() })
        }).collect();
        barrier.wait();
        assert_eq!(joins.into_iter().map(|j| usize::from(j.join().unwrap())).sum::<usize>(), 1, "host spacing is serialized too");
        let mut h = h.lock().unwrap(); h.slots.clear(); h.last = None;
        h.ready(Path::new("a"), true); h.ready(Path::new("b"), true);
        assert!(h.reserve(Path::new("b"), 1, 1, "codex", &cap, true, 0).is_err());
        assert!(h.reserve(Path::new("a"), 1, 1, "codex", &cap, true, 0).is_ok());
        h.sync(Path::new("a"), vec![]);
        assert!(h.reserve(Path::new("a"), 2, 2, "codex", &cap, true, 30).is_err());
        assert!(h.reserve(Path::new("b"), 1, 1, "codex", &cap, true, 30).is_ok());
    }

    // Chain: R9 R12 §3 A3 — external demand survives task passes and rotates with a worker backlog.
    // Risk: PilotQ starves, overrides worker fairness or leaves stale demand after admission/cancellation.
    #[test]
    fn external_demand_rotates_without_task_pass_ownership() {
        for (manager,worker) in [("a","b"),("b","a"),("a","a")] {
            let (manager,worker)=(Path::new(manager),Path::new(worker));
            let p=policy();let cap=p.cap.clone();let mut h=Host::new(Ok(p));
            h.ready(worker,true);
            let first=h.external_reserve("turn-1",manager,"codex",&cap,0);
            if manager>worker {
                assert!(first.unwrap_err().contains("round-robin"));
                // A project pass with no task demand cannot remove the external request.
                h.ready(manager,false);
                assert!(h.reserve(worker,1,77,"codex",&cap,true,0).is_ok());
                h.sync(worker,vec![]);
                assert!(h.reserve(worker,2,78,"codex",&cap,true,30).unwrap_err().contains("round-robin"));
                assert!(h.external_reserve("turn-1",manager,"codex",&cap,30).is_ok());
            } else {assert!(first.is_ok());}
            assert_eq!(h.occupied(),1);
            h.external_sync("turn-1",manager,"codex",false);
            assert!(h.external_ready.is_empty(),"admission removes only its own demand");
            assert!(h.reserve(worker,3,79,"codex",&cap,true,60).is_ok());
            h.sync(worker,vec![]);
            // The next host reconciliation rebuilds demand from surviving eligible requests only.
            assert!(h.external_reserve("turn-2",manager,"codex",&cap,60).is_err());
            h.external_reset_ready();
            assert!(h.reserve(worker,4,80,"codex",&cap,true,90).is_ok());
        }
    }

    // Chain A4: six effective copies -> preflight -> durable originals -> all readbacks -> repeat.
    // Risk: conflict/unknown inputs cause partial edits, foreign-host policy leaks in, or retry changes owner policy.
    #[test]
    fn six_copy_migration_is_atomic_on_conflict_and_restart_safe() {
        let install = temp("migration");
        let _ = fs::remove_dir_all(install.join("host-policy-migration"));
        let _ = fs::remove_file(install.join("host.json"));
        let roots: Vec<_> = (0..6).map(|i| { let p = install.join(format!("project{i}")); fs::create_dir_all(&p).unwrap(); p }).collect();
        let me = std::env::var("TASKQ_HOST").ok().filter(|s| !s.is_empty()).unwrap_or_else(crate::hostname);
        let originals: Vec<_> = roots.iter().enumerate().map(|(i, r)| {
            let text = format!("  {{\"repo\":\"invented/p{i}\",\"untouched\":{{\"x\":7}},\"limits\":{{\"codex\":0}},\"capacity_hosts\":{},\"admission\":{{\"mode\":\"auto\",\"max\":2}}}}\n",
                json!({me.clone(): {"limits": {"codex": 2}}, "invented-other-host": {"limits": {"codex": 999}}}));
            fs::write(r.join("taskq.json"), &text).unwrap(); text
        }).collect();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            for r in &roots { fs::set_permissions(r.join("taskq.json"), fs::Permissions::from_mode(0o640)).unwrap(); }
        }
        let owners: Vec<_> = roots.iter().map(|r| ownership(&r.join("taskq.json")).unwrap()).collect();
        let path = roots[5].join("taskq.json");
        let mut conflict: Value = serde_json::from_str(&originals[5]).unwrap();
        conflict["capacity_hosts"][&me]["limits"]["codex"] = json!(1);
        let conflicting = serde_json::to_string(&conflict).unwrap(); fs::write(&path, &conflicting).unwrap();
        let e = migrate(&install, &roots).unwrap_err();
        assert!(e.contains("invented/p5: limits"), "{e}");
        assert!(!install.join("host.json").exists()); assert!(!install.join("host-policy-migration").exists());
        for (i, r) in roots.iter().enumerate() { assert_eq!(fs::read_to_string(r.join("taskq.json")).unwrap(), if i == 5 { &conflicting } else { &originals[i] }.as_str()); }
        fs::write(&path, &originals[5]).unwrap();
        fs::rename(roots[4].join("taskq.json"), roots[4].join("missing")).unwrap();
        assert!(migrate(&install, &roots).unwrap_err().contains("unknown")); assert!(!install.join("host.json").exists());
        fs::rename(roots[4].join("missing"), roots[4].join("taskq.json")).unwrap();
        migrate(&install, &roots).unwrap();
        assert_eq!(Policy::read(&install).unwrap().cap.limits["codex"], 2, "only the effective local host copy migrates");
        for (i, r) in roots.iter().enumerate() {
            assert_eq!(fs::read_to_string(install.join(format!("host-policy-migration/{i}.json"))).unwrap(), originals[i]);
            let v: Value = serde_json::from_slice(&fs::read(r.join("taskq.json")).unwrap()).unwrap();
            assert_eq!(v["untouched"], json!({"x": 7})); assert!(v["capacity_hosts"].is_null()); assert!(v["limits"].is_null());
            assert_eq!(ownership(&r.join("taskq.json")).unwrap(), owners[i]);
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(fs::metadata(r.join("taskq.json")).unwrap().permissions().mode() & 0o777, 0o640); }
        }
        // Simulate a crash halfway through staged replacements. Resume consumes the saved mapping, not remaining copies.
        fs::remove_file(install.join("host-policy-migration/complete")).unwrap();
        let mut interrupted = fs::metadata(roots[0].join("taskq.json")).unwrap().permissions();
        #[cfg(windows)] interrupted.set_readonly(true);
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; interrupted.set_mode(0o600); }
        fs::set_permissions(roots[0].join("taskq.json"), interrupted).unwrap();
        fs::write(roots[2].join("taskq.json"), &originals[2]).unwrap();
        migrate(&install, &roots).unwrap();
        assert!(!fs::metadata(roots[0].join("taskq.json")).unwrap().permissions().readonly());
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(fs::metadata(roots[0].join("taskq.json")).unwrap().permissions().mode() & 0o777, 0o640); }
        let mut edited = Policy::read(&install).unwrap().raw; edited["limits"]["codex"] = json!(1);
        fs::write(install.join("host.json"), serde_json::to_string(&edited).unwrap()).unwrap();
        migrate(&install, &roots).unwrap(); assert_eq!(Policy::read(&install).unwrap().raw, edited, "completed retry preserves later owner edits");
        fs::remove_dir_all(install).unwrap();
    }

    // Chain A4: Windows protected DACL -> staged replacement. Risk: changing file access during migration.
    #[cfg(windows)]
    #[test]
    fn migration_preserves_protected_windows_acl() {
        let root = temp("protected-acl");
        let path = root.join("taskq.json");
        fs::write(&path, r#"{"repo":"invented/protected","limits":{"codex":2},"admission":{"mode":"auto","max":2}}"#).unwrap();
        let owner = ownership(&path).unwrap();
        let mut protected = owner.clone();
        protected[3] = json!(protected[3].as_u64().unwrap() | 0x10);
        stage_owner(&path, &protected).unwrap();
        let expected = ownership(&path).unwrap();
        migrate(&root, &[root.clone()]).unwrap();
        assert_eq!(ownership(&path).unwrap(), expected);
        assert_eq!(Policy::read(&root).unwrap().cap.limits["codex"], 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    fn modify_only_windows_acl(path:&Path) {
        // CI failed to load the Security module; use the framework ACL API directly.
        let code=r#"$ErrorActionPreference='Stop';$p=$env:TASKQ_ACL_FIXTURE;$a=[IO.Directory]::GetAccessControl($p);$a.SetAccessRuleProtection($true,$false);$s=[Security.Principal.WindowsIdentity]::GetCurrent().User;$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'TakeOwnership','ContainerInherit,ObjectInherit','None','Deny'));$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'Modify','ContainerInherit,ObjectInherit','None','Allow'));[IO.Directory]::SetAccessControl($p,$a)"#;
        let out=crate::platform::command("powershell").args(["-NoProfile","-NonInteractive","-Command",code]).env("TASKQ_ACL_FIXTURE",path).output().unwrap();
        assert!(out.status.success(),"ACL fixture: {}",String::from_utf8_lossy(&out.stderr));
    }

    // R9/section 9: an ordinary owner can replace data and DACLs without WRITE_OWNER.
    // Risk: unconditional owner/group copy rejects a CLI-written registry under a limited service token.
    #[cfg(windows)]
    #[test]
    fn registry_editor_preserves_modify_only_windows_acl() {
        for protected in [false,true] {
            let base=temp("registry-modify-acl");let install=base.join("install");
            let a=base.join("a");let b=base.join("b");
            for p in [&install,&a,&b] {fs::create_dir_all(p).unwrap();}
            modify_only_windows_acl(&install);
            ProjectEdit::new(&install,&a,true).unwrap().commit().unwrap();
            let path=install.join("projects");
            if protected {
                let code=r#"$ErrorActionPreference='Stop';$p=$env:TASKQ_ACL_FIXTURE;$a=[IO.File]::GetAccessControl($p);$a.SetAccessRuleProtection($true,$true);[IO.File]::SetAccessControl($p,$a)"#;
                let out=crate::platform::command("powershell").args(["-NoProfile","-NonInteractive","-Command",code]).env("TASKQ_ACL_FIXTURE",&path).output().unwrap();
                assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            }
            let expected=ownership(&path).unwrap();
            ProjectEdit::new(&install,&b,true).unwrap().commit().unwrap();
            assert_eq!(ownership(&path).unwrap(),expected,"registration preserves owner/group/DACL");
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            publish_service(&install,&native,"registry-fixture").unwrap();
            let (capture,guard)=crate::profile::tests::collecting(&install);
            let request=crate::lifecycle::uninstall_review(&install,"registry-uninstall",Some(&a)).unwrap();
            let received=crate::lifecycle::submit(&install,&request).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap_err().contains("project drain not observed"));
            let mode=fs::metadata(&path).unwrap().permissions();let mut readonly=mode.clone();readonly.set_readonly(true);
            fs::set_permissions(&path,readonly).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",true).is_err());
            let applied=crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap();
            let retained_applied=crate::resources::try_with(&install,|state|Ok(state.ops["registry-uninstall"].receipt.clone())).unwrap();
            assert_eq!(applied["state"],"applied");assert_eq!(applied["project_drained"],true);
            assert!(applied.get("observed_receipt").is_none());
            assert_eq!(projects(&install).unwrap().len(),2,"refused write retains both roots");
            assert_eq!(ownership(&path).unwrap(),expected);
            fs::set_permissions(&path,mode).unwrap();
            crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",true).unwrap();
            assert_eq!(projects(&install).unwrap(),vec![b.canonicalize().unwrap()]);
            assert_eq!(ownership(&path).unwrap(),expected,"removal preserves owner/group/DACL");
            let observed=crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap();
            assert_eq!(observed["state"],"observed");assert_eq!(observed["unregistered"],true);
            assert_eq!(observed["received_receipt"],received["received_receipt"]);
            assert_eq!(crate::lifecycle::submit(&install,&request).unwrap(),observed);
            // Resume the crash cut after registry commit but before the terminal receipt was saved.
            crate::resources::with(&install,|state| {
                state.ops.get_mut("registry-uninstall").unwrap().receipt=retained_applied.clone();Ok(())
            }).unwrap();
            let mut changed=expected.clone();changed[3]=json!(changed[3].as_u64().unwrap()^0x10);
            stage_owner(&path,&changed).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap_err().contains("ownership differs"));
            assert_eq!(crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap(),applied,"changed ACL cannot invent observation after registry commit");
            stage_owner(&path,&expected).unwrap();
            crate::resources::with(&install,|state| {state.ops.get_mut("registry-uninstall").unwrap().receipt.as_object_mut().unwrap().remove("registry_owner");Ok(())}).unwrap();
            assert!(crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap_err().contains("original registry ownership unknown"));
            crate::resources::with(&install,|state| {state.ops.get_mut("registry-uninstall").unwrap().receipt["registry_owner"]=expected.clone();Ok(())}).unwrap();
            crate::lifecycle::finish_project_uninstall(&install,"registry-uninstall","registry-fixture",false).unwrap();
            assert_eq!(crate::lifecycle::receipt(&install,"registry-uninstall").unwrap().unwrap(),observed);
            assert!(crate::lifecycle::pending(&install).unwrap().is_none(),"completed project removal releases the lifecycle barrier");
            capture.flush().unwrap();drop(guard);drop(capture);
            let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
            let events=reader.export_kinds(&["command.received","command.applied","command.observed"],0,0,1024).unwrap();
            for field in ["received_receipt","applied_receipt","observed_receipt"] {
                assert!(events["events"].as_array().unwrap().iter().any(|e|e["id"]==observed[field]),"missing native {field}");
            }
            println!("Windows registry protected={protected}: native owner {} {} receipt {observed}",native.pid,native.started);
            drop(reader);
            let mut foreign=expected.as_array().unwrap().clone();let offset=(foreign.len() as u32).to_le_bytes();
            for (i,byte) in offset.into_iter().enumerate() {foreign[4+i]=json!(byte);}
            foreign.extend([1,1,0,0,0,0,0,5,18,0,0,0].map(|byte|json!(byte))); // LocalSystem, not the ordinary owner.
            assert!(stage_owner(&path,&json!(foreign)).is_err(),"a differing owner still requires ownership rights");
            for i in 4..8 {foreign[i]=json!(255);}
            assert!(stage_owner(&path,&json!(foreign)).unwrap_err().contains("SID corrupt"));
            assert_eq!(ownership(&path).unwrap(),expected,"refused or corrupt ownership changes keep protection");
            fs::remove_dir_all(base).unwrap();
        }
    }

    // Chain A4: unknown or corrupt host policy -> refused launch, never fallback to project copies.
    #[test]
    fn unknown_policy_holds_without_discarding_occupied_slots() {
        let install = temp("unknown"); let _ = fs::remove_file(install.join("host.json"));
        let mut h = Host::new(Ok(policy())); let cap = policy().cap;
        h.reserve(Path::new("a"), 1, 7, "codex", &cap, false, 0).unwrap();
        h.reload(Policy::read(&install));
        assert!(h.policy.as_ref().unwrap_err().contains("unknown"));
        assert!(h.reserve(Path::new("b"), 1, 7, "codex", &cap, false, 30).is_err());
        assert_eq!(h.slots.len(), 1);
        for v in [json!({"admission": "bad"}), json!({"codex": [false]}), json!({"load_gate": {"enabled": "yes"}}), json!({"update_every": 0})] {
            assert!(Policy::parse(v, "invented").is_err());
        }
        assert_eq!(policy().project(Path::new("/p"), &json!({"limits": {"codex": 3}})).unwrap().limits["codex"], 2, "a project count above the host clamps, never holds");
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: host.json projects overlay -> project capacity and revision (#1282). Risk: an overlay raising the host
    // ceiling, reaching another root, or an edit-and-revert reusing a revision a stale operation names.
    #[test]
    fn project_overlay_only_lowers_its_own_root() {
        let host = json!({"limits": {"codex": 5, "claude": 5}});
        let base = Policy::parse(host.clone(), "invented").unwrap();
        let mut v = host.clone(); v["projects"] = json!({"/a": {"limits": {"codex": 4}, "op": "op-1"}});
        let p = Policy::parse(v.clone(), "invented").unwrap();
        assert_eq!(p.project(Path::new("/a"), &json!({})).unwrap().limits["codex"], 4);
        assert_eq!(p.project(Path::new("/a"), &json!({"limits": {"codex": 3}})).unwrap().limits["codex"], 3, "the lower of both");
        assert_eq!(p.project(Path::new("/b"), &json!({})).unwrap().limits["codex"], 5, "another root untouched");
        assert_eq!(p.project(Path::new("/b"), &json!({"limits": {"codex": 9}})).unwrap().limits["codex"], 5, "a taskq.json count above the host ceiling clamps, never holds");
        let off = Policy::parse(json!({"limits": {"codex": 5, "claude": 0}}), "invented").unwrap();
        assert_eq!(off.project(Path::new("/b"), &json!({"limits": {"claude": 4}})).unwrap().limits["claude"], 0, "a host 0 stays disabled");
        assert_eq!(p.project(Path::new("/a"), &json!({})).unwrap().limits["claude"], 5, "another runtime untouched");
        assert_eq!(p.project_limits(Path::new("/a"), &json!({})).unwrap(), BTreeMap::from([("codex".to_string(), 4)]));
        let e = json!({});
        assert_eq!(base.project_revision(Path::new("/a"), &e).unwrap(), base.project_revision(Path::new("/b"), &e).unwrap());
        assert_eq!(p.project_revision(Path::new("/b"), &e).unwrap(), base.project_revision(Path::new("/b"), &e).unwrap());
        let changed = p.project_revision(Path::new("/a"), &e).unwrap();
        assert_ne!(changed, base.project_revision(Path::new("/a"), &e).unwrap());
        assert_ne!(p.project_revision(Path::new("/a"), &json!({"limits": {"codex": 1}})).unwrap(), changed, "a taskq.json limits edit is a new revision");
        v["projects"]["/a"] = json!({"limits": {}, "op": "op-2"});
        let restored = Policy::parse(v.clone(), "invented").unwrap();
        assert_eq!(restored.project(Path::new("/a"), &json!({})).unwrap().limits["codex"], 5);
        let reverted = restored.project_revision(Path::new("/a"), &e).unwrap();
        assert!(reverted != changed && reverted != base.project_revision(Path::new("/a"), &e).unwrap(), "edit-and-revert is a new revision");
        for bad in [json!({"/a": {"limits": {"codex": 6}}}), json!({"/a": {"limits": {"other": 1}}}), json!({"/a": {"limits": {"codex": -1}}}),
            json!({"/a": {"limits": {}, "admission": {}}}), json!({"/a": {"op": "x"}}), json!({"/a": {"limits": {}, "op": "bad id"}}), json!(["/a"])] {
            let mut v = host.clone(); v["projects"] = bad.clone();
            assert!(Policy::parse(v, "invented").is_err(), "{bad} must hold launches");
        }
    }

    // Chain A5: host pid ownership and registry poke -> exact-root reload. Risk: duplicate host or discovery.
    #[test]
    fn power_loss_zeroed_records_are_set_aside_only_without_an_owner() {
        // Defect #1445: a zeroed service or status.json held startup forever after a power cut.
        let install = temp("zeroed");
        let zeroed = |p: &Path| fs::write(p, [0u8; 64]).unwrap();
        let aside = |dir: &Path, prefix: &str| fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(prefix)).count();
        zeroed(&install.join("service"));
        {
            let _owner = coreq::file_lock::try_lock(&install.join("service.lock")).unwrap();
            assert!(other(&install).is_err(), "a live native owner keeps the zeroed record");
            assert_eq!(fs::read(install.join("service")).unwrap(), [0u8; 64]);
        }
        assert_eq!(other(&install).unwrap(), None);
        assert!(!install.join("service").exists());
        assert_eq!(aside(&install, "service.zeroed-"), 1);
        fs::create_dir_all(crate::dot(&install)).unwrap();
        zeroed(&crate::dot(&install).join("status.json"));
        assert!(project_service(&install).is_err());
        assert!(crate::data::set_aside_zeroed(&crate::dot(&install).join("status.json")).unwrap());
        assert_eq!(project_service(&install).unwrap(), None);
        assert_eq!(aside(&crate::dot(&install), "status.json.zeroed-"), 1);
        fs::write(install.join("service"), "bad").unwrap();
        assert!(other(&install).is_err(), "a nonzero unreadable record is never moved");
    }

    #[test]
    fn ended_project_thread_logs_its_error() {
        // Defect #1445: an ended project thread logged no reason.
        assert_eq!(thread_error(&Ok(Err("kids: unreadable".into()))), "project thread error: kids: unreadable");
        let panic = std::thread::spawn(|| -> Res<()> { panic!("boom") }).join();
        assert_eq!(thread_error(&panic), "project thread error: panic: boom");
        let install = temp("thread-error");
        let root = install.join("proj");
        fs::create_dir_all(&root).unwrap();
        log_all(&install, std::slice::from_ref(&root), &thread_error(&Ok(Err("adopt failed".into()))));
        assert!(fs::read_to_string(project_log(&install, &root)).unwrap().contains("project thread error: adopt failed"));
    }

    #[test]
    fn pid_check_and_poke_use_host_paths_and_unknown_is_held() {
        let install = temp("pid");
        let child = if cfg!(windows) { std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn() }
            else { std::process::Command::new("sleep").arg("30").spawn() }.unwrap();
        let mut child = child;
        fs::create_dir_all(crate::dot(&install)).unwrap();
        let legacy_status = format!("{{\"pid\":{}}}", child.id());
        fs::write(crate::dot(&install).join("status.json"), &legacy_status).unwrap();
        assert!(startup_status(&install, &install, "invented malformed config","fixture").is_err());
        assert_eq!(fs::read_to_string(crate::dot(&install).join("status.json")).unwrap(), legacy_status, "startup failure cannot overwrite a live legacy pid");
        assert!(legacy(&install,&[install.clone()]).is_err());
        fs::write(install.join("service"), child.id().to_string()).unwrap(); assert_eq!(other(&install).unwrap(), Some(child.id()));
        let stale=std::time::SystemTime::now()-std::time::Duration::from_secs(120);
        fs::OpenOptions::new().write(true).open(install.join("service")).unwrap()
            .set_times(fs::FileTimes::new().set_modified(stale)).unwrap();
        assert_eq!(other(&install).unwrap(),Some(child.id()),"stale heartbeat cannot release a live service owner");

        fs::write(install.join("service"), std::process::id().to_string()).unwrap(); assert_eq!(other(&install).unwrap(), None);
        fs::write(install.join("service"), "bad").unwrap(); assert!(other(&install).unwrap_err().contains("unknown"));
        fs::write(install.join("projects"), format!("{}\n", install.display())).unwrap();
        assert_eq!(projects(&install).unwrap(), vec![install.canonicalize().unwrap()]);
        let _ = fs::remove_file(install.join("poke"));
        let mut registry = Registry::default(); registry.refresh(&install, 0);
        fs::write(install.join("projects"), "").unwrap(); registry.refresh(&install, 1);
        assert_eq!(registry.roots.len(), 1, "no folder discovery or unnecessary registry reads");
        crate::touch(&install.join("poke")); registry.refresh(&install, 2);
        assert!(registry.roots.is_empty(), "host poke loads the changed exact-root list before the minute poll");
        fs::write(install.join("projects"), "relative\n").unwrap(); registry.refresh(&install, 62);
        assert!(registry.error.as_ref().unwrap().contains("unknown")); assert!(registry.roots.is_empty(), "malformed registry retains the last good list");
        child.kill().unwrap(); let _ = child.wait(); fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn never_registered_host_has_zero_projects_but_a_lost_registry_is_unknown() {
        let install = std::env::temp_dir().join(format!("taskq-registry-{}", crate::runtime::uuid())); fs::create_dir(&install).unwrap();
        let mut registry = Registry::default(); registry.refresh(&install, 0);
        assert!(registry.error.is_none() && registry.roots.is_empty(), "fresh host: setup must not wait on a registry");
        fs::write(install.join("projects"), format!("{}\n", install.display())).unwrap(); registry.refresh(&install, 60);
        assert_eq!(registry.roots.len(), 1);
        fs::remove_file(install.join("projects")).unwrap(); registry.refresh(&install, 120);
        assert!(registry.error.is_some() && registry.roots.len() == 1, "a vanished registry keeps the last roots and reads unknown");
        fs::remove_dir_all(install).unwrap();
    }
}
