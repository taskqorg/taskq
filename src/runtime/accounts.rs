//! Runtime accounts per host ([taskq.md § 3 Runtime accounts](../taskq.md#runtime-accounts),
//! [detail](../docs/design/capacity.md#runtime-accounts)): an ordered list of named login homes per CLI runtime,
//! each enabled or disabled. Polled usage and fleet policy ([usage](usage.rs)) rank new starts; a usage limit moves
//! them to another enabled account.
use crate::runtime::Cap;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The owner's list per runtime, in `<install>`.
pub const FILE: &str = "accounts.json";
/// The service's limit record per runtime and label, in `<install>`.
pub const STATE: &str = "accounts-state.json";
const PENDING: &str = "accounts-state.pending.json";
// Retain failed observations across project threads even when the journal itself cannot be written.
static UNSAVED: std::sync::Mutex<BTreeMap<PathBuf, Vec<Value>>> = std::sync::Mutex::new(BTreeMap::new());
/// The runtime's own home.
pub const DEFAULT: &str = "default";
/// The folder under the default home that holds the other accounts' homes.
pub const HOMES: &str = "taskq-accounts";
/// A limit without a reported reset ends at the usage-limit probe (service `USAGE_PROBE`).
const PROBE: i64 = 1800;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limit {
    /// Board time the limit was seen.
    pub at: i64,
    /// The reported reset, when the runtime named one.
    pub until: Option<i64>,
}

impl Limit {
    /// When the limit ends: the reported reset, else the probe after it.
    pub fn end(&self) -> i64 { self.until.unwrap_or(self.at.saturating_add(PROBE)) }
}

pub type Policy = BTreeMap<String, Vec<Entry>>;
pub type Limits = BTreeMap<String, Limit>;

/// Runtimes with accounts: the CLI children. `codex-app` runs in the app's process on Codex's default home.
/// Wall-clock seconds; the service passes board time where it has it.
pub fn now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64) }

pub fn applies(rt: &str) -> bool { crate::runtime::of(rt).has(Cap::Accounts) }

pub fn valid_label(l: &str) -> bool {
    (1..=32).contains(&l.len()) && !l.starts_with('-') && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn default_list() -> Vec<Entry> { vec![Entry { label: DEFAULT.into(), enabled: true }] }

fn parse_policy(v: &Value) -> Result<Policy, String> {
    let mut p = Policy::new();
    for (rt, list) in v.as_object().ok_or(format!("{FILE}: not a JSON object"))? {
        if !applies(rt) { return Err(format!("{FILE}: runtime {rt} has no accounts")); }
        let rows = list.as_array().filter(|a| !a.is_empty()).ok_or(format!("{FILE}: {rt} needs a non-empty list"))?;
        let mut seen = BTreeSet::new();
        let mut entries = vec![];
        for r in rows {
            let label = r["label"].as_str().filter(|l| valid_label(l)).ok_or(format!("{FILE}: {rt} has an invalid label"))?;
            let enabled = r["enabled"].as_bool().ok_or(format!("{FILE}: {rt} {label} needs enabled true or false"))?;
            if !seen.insert(label) { return Err(format!("{FILE}: {rt} lists {label} twice")); }
            entries.push(Entry { label: label.into(), enabled });
        }
        if entries.iter().any(|e| e.label != DEFAULT) { one_account(rt).map_err(|e| format!("{FILE}: {e}"))?; }
        p.insert(rt.clone(), entries);
    }
    Ok(p)
}

fn policy_json(p: &Policy) -> Value {
    Value::Object(p.iter().map(|(rt, list)| (rt.clone(), json!(list.iter().map(|e| json!({"label": e.label, "enabled": e.enabled})).collect::<Vec<_>>()))).collect())
}

/// The owner's lists; a missing file is empty (every runtime has one `default` account).
pub fn read_policy(install: &Path) -> Result<Policy, String> {
    crate::data::json(&install.join(FILE))?.map_or(Ok(Policy::new()), |v| parse_policy(&v))
}

fn write_policy(install: &Path, p: &Policy) -> Result<(), String> {
    std::fs::create_dir_all(install).map_err(|e| e.to_string())?;
    crate::platform::write_synced(&install.join(FILE), &format!("{:#}\n", policy_json(p)))
}

/// Validate availability independently of capacity; unknown state must never enable a launch.
fn state_value(install: &Path) -> Result<Value, String> {
    let v = crate::data::json(&install.join(STATE)).map_err(|_| "host account quota state unreadable")?.unwrap_or_else(|| json!({}));
    for (rt, accounts) in v.as_object().ok_or("account quota state must be an object")? {
        if !crate::runtime::CLI_RUNTIMES.contains(&rt.as_str()) {return Err("quota runtime unknown".into());}
        for (label, q) in accounts.as_object().ok_or("account quota state must list accounts")? {
            if !valid_label(label) {return Err("quota account label invalid".into());}
            parse_limit(q)?;
            if let Some(windows) = q.get("windows") {
                for w in windows.as_array().filter(|w| !w.is_empty()).ok_or("quota windows must be a non-empty array")? {
                    if w["window"].as_str().is_none_or(|w| w.is_empty() || w.len()>40 || !w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')) { return Err("quota window unknown".into()); }
                    parse_limit(w)?;
                }
                let largest = windows.as_array().unwrap().iter().map(parse_limit).collect::<Result<Vec<_>,_>>()?.into_iter().max_by_key(Limit::end).unwrap();
                if parse_limit(q)? != largest { return Err("quota summary does not match windows".into()); }
            }
        }
    }
    Ok(v)
}

fn parse_limit(v: &Value) -> Result<Limit, String> {
    let at = v["limited"].as_i64().ok_or("quota observation time unknown")?;
    let until = match v.get("until") { Some(Value::Null) => None, Some(v) => Some(v.as_i64().ok_or("quota reset unknown")?), None => return Err("quota reset field missing".into()) };
    if at < 0 || until.is_some_and(|t| t < 0) { return Err("quota time invalid".into()); }
    Ok(Limit { at, until })
}

fn windows(q: &Value) -> Vec<Value> {
    q["windows"].as_array().cloned().unwrap_or_else(|| {
        parse_limit(q).ok().map(|l| vec![json!({"window":"unknown", "limited":l.at,"until":l.until})]).unwrap_or_default()
    })
}

fn pack(w: Vec<Value>) -> Value {
    let l = w.iter().filter_map(|w| parse_limit(w).ok()).max_by_key(Limit::end).unwrap();
    json!({"limited":l.at,"until":l.until,"windows":w})
}

fn save_state(install: &Path, v: &Value) -> Result<(), String> { crate::platform::write_synced(&install.join(STATE), &format!("{v:#}\n")) }

fn summaries(v: &Value) -> BTreeMap<String, Limits> {
    v.as_object().into_iter().flatten().map(|(rt,m)| (rt.clone(), m.as_object().into_iter().flatten()
        .filter_map(|(label,l)| parse_limit(l).ok().map(|l| (label.clone(),l))).collect())).collect()
}

/// Merge independent observed windows, retaining every still-active hold. The lock covers all project threads.
pub fn record_windows(install: &Path, rt: &str, label: &str, at: i64, observed: &[(String, Option<i64>)]) -> Result<(), String> {
    if !observed.is_empty() && pending_error(install).is_none() && state_value(install).is_ok_and(|v| {
        let saved=windows(&v[rt][label]);
        observed.iter().all(|(window,until)|saved.iter().any(|w|w["window"]==*window && w["until"]==json!(until.filter(|t|*t>at)) && parse_limit(w).is_ok_and(|l|l.end()>at)))
    }) {return Ok(());}
    let observation=json!({"runtime":rt,"account":label,"at":at,"windows":observed});
    {
        let mut unsaved=UNSAVED.lock().map_err(|_| "quota observation lock poisoned")?;
        let pending=unsaved.entry(install.to_path_buf()).or_default();
        if !pending.iter().any(|p|p["runtime"]==observation["runtime"] && p["account"]==observation["account"] && p["windows"]==observation["windows"]) {pending.push(observation);}
    }
    recover_pending(install)?;
    crate::profile::measure_counted("account.quota", || ((), [format!("runtime:{rt}"),format!("account:{label}")].into_iter().chain(observed.iter().map(|(w,reset)|format!("window:{w}:{}:{}",if reset.is_some(){"until"}else{"probe"},reset.unwrap_or(at.saturating_add(PROBE))))).map(|k|(k,1)).collect()));
    Ok(())
}

/// Replay write-ahead observations before admission, also after a service restart.
pub fn recover_pending(install: &Path) -> Result<(), String> {
    if pending_error(install).is_none() {return Ok(());}
    std::fs::create_dir_all(install).map_err(|e|e.to_string())?;
    let _lock=coreq::file_lock::try_lock(&install.join("accounts-state.lock")).map_err(|e|format!("quota state busy or unavailable: {e}"))?;
    let mut pending=match crate::data::json(&install.join(PENDING))? {
        Some(v)=>v["observations"].as_array().cloned().ok_or("quota journal invalid")?,None=>vec![]
    };
    let memory=UNSAVED.lock().map_err(|_| "quota observation lock poisoned")?.get(install).cloned().unwrap_or_default();
    for observation in memory {if !pending.contains(&observation) {pending.push(observation);}}
    if pending.is_empty() {return Err("quota journal observations missing".into());}
    crate::platform::write_synced(&install.join(PENDING),&json!({"observations":pending}).to_string())?;
    let mut v=state_value(install)?;
    for observation in &pending {
        let rt=observation["runtime"].as_str().filter(|rt|crate::runtime::CLI_RUNTIMES.contains(rt)).ok_or("quota journal runtime invalid")?;
        let label=observation["account"].as_str().filter(|l|valid_label(l)).ok_or("quota journal account invalid")?;
        let at=observation["at"].as_i64().filter(|at|*at>=0).ok_or("quota journal time invalid")?;
        let observed:Vec<(String,Option<i64>)>=serde_json::from_value(observation["windows"].clone()).map_err(|_|"quota journal windows invalid")?;
        if observed.is_empty() {return Err("quota observation missing".into());}
        if v.get(rt).is_none() {v[rt]=json!({});}
        let mut w:Vec<_>=windows(&v[rt][label]).into_iter().filter(|w|w["window"]!="probe" && parse_limit(w).is_ok_and(|l|l.end()>at)).collect();
        for (window,until) in observed {
            if window.is_empty() || window.len()>40 || !window.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-') || until.is_some_and(|t|t<0) {return Err("quota journal window invalid".into());}
            let until=until.filter(|t|*t>at);
            if !w.iter().any(|w|w["window"]==window && w["until"]==json!(until)) {w.push(json!({"window":window,"limited":at,"until":until}));}
        }
        v[rt][label]=pack(w);
    }
    save_state(install,&v)?;
    std::fs::remove_file(install.join(PENDING)).map_err(|e|e.to_string())?;
    let mut unsaved=UNSAVED.lock().map_err(|_|"quota observation lock poisoned")?;
    if let Some(memory)=unsaved.get_mut(install) {memory.retain(|o|!pending.contains(o));if memory.is_empty(){unsaved.remove(install);}}
    Ok(())
}

fn pending_error(install: &Path) -> Option<String> {
    let memory=UNSAVED.lock().map(|m|m.get(install).is_some_and(|p|!p.is_empty()));
    match (memory,std::fs::symlink_metadata(install.join(PENDING))) {
        (Ok(false),Err(e)) if e.kind()==std::io::ErrorKind::NotFound=>None,
        _=>Some("host account quota observation pending durable save".into())
    }
}

/// Atomically reserve a due probe. No second project may probe this account during the bounded interval.
/// A resume supplies its original label; a new launch selects the best-ranked available enabled account.
pub fn claim(install: &Path, rt: &str, label: Option<&str>, now: i64, owner: &Path) -> Result<String, String> {
    std::fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let _lock = coreq::file_lock::try_lock(&install.join("accounts-state.lock")).map_err(|e| format!("quota state busy or unavailable: {e}"))?;
    let h = Host::read(install);
    if let Some(e) = h.error { return Err(e); }
    let label = match label { Some(l) if h.free(rt,l,now) => l.to_string(), Some(_) => return Err(format!("{rt}: original account quota hold")), None => h.active(rt,now).ok_or_else(|| format!("{rt}: no available enabled account"))? };
    let mut v = state_value(install)?;
    let probe=parse_limit(&v[rt][&label]).is_ok_and(|l| l.end() <= now);
    if probe {
        v[rt][&label] = pack(vec![json!({"window":"probe","limited":now,"until":null})]);
        v[rt][&label]["probe_owner"] = json!(owner);
    }
    save_state(install,&v)?;
    drop(_lock);
    if probe {
        crate::profile::measure_counted("account.quota-probe", || ((),[format!("runtime:{rt}"),format!("account:{label}")].into_iter().map(|k|(k,1)).collect()));
    }
    Ok(label)
}

/// Confirm only this execution's due probe after native successful completion; newer observations remain intact.
pub fn confirm_probe(install: &Path, rt: &str, label: &str, owner: &Path) -> Result<(), String> {
    if state_value(install)?[rt][label]["probe_owner"] != json!(owner) { return Ok(()); }
    let _lock = coreq::file_lock::try_lock(&install.join("accounts-state.lock")).map_err(|e| format!("quota state busy or unavailable: {e}"))?;
    let mut v = state_value(install)?;
    if v[rt][label]["probe_owner"] == json!(owner) {
        v[rt].as_object_mut().ok_or("quota accounts unknown")?.remove(label);
        save_state(install,&v)?;
        drop(_lock);
        crate::profile::measure_counted("account.quota-recovered", || ((),[format!("runtime:{rt}"),format!("account:{label}")].into_iter().map(|k|(k,1)).collect()));
    }
    Ok(())
}

/// One host's accounts: the owner's lists and the limit record.
#[derive(Default)]
pub struct Host {
    pub policy: Policy,
    pub state: BTreeMap<String, Limits>,
    pub observations: Value,
    /// Unknown policy or availability holds every new launch.
    pub error: Option<String>,
    /// Polled usage and fleet policy: they order new starts, and their loss holds nothing.
    pub usage: super::usage::View,
}

impl Host {
    pub fn read(install: &Path) -> Host {
        let (policy, error) = match read_policy(install) { Ok(p) => (p, None), Err(_) => (Policy::new(), Some("host account policy unreadable".into())) };
        let error=error.or_else(||pending_error(install));
        let usage=super::usage::View::read(install);
        match state_value(install) {
            Ok(observations) => Host { policy, state: summaries(&observations), observations, error, usage },
            Err(e) => Host { policy, error: Some(error.unwrap_or(e)), usage, ..Default::default() },
        }
    }

    /// The list of `rt`, in priority order.
    pub fn list(&self, rt: &str) -> Vec<Entry> { self.policy.get(rt).cloned().unwrap_or_else(default_list) }

    fn limit(&self, rt: &str, label: &str) -> Option<&Limit> { self.state.get(rt)?.get(label) }

    fn free(&self, rt: &str, label: &str, now: i64) -> bool { self.limit(rt, label).is_none_or(|l| l.end() <= now) }

    /// The best-ranked enabled account whose limit has ended and that fleet policy does not avoid.
    pub fn active(&self, rt: &str, now: i64) -> Option<String> {
        if self.error.is_some() { return None; }
        let candidates: Vec<String> = self.list(rt).into_iter()
            .filter(|e| e.enabled && self.free(rt, &e.label, now) && !self.usage.avoided(rt, &e.label, now)).map(|e| e.label).collect();
        self.usage.rank(rt, &candidates, now)
    }

    /// A selected account can still start: enabled, past its limit, not avoided. Ranking may have moved past it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn usable(&self, rt: &str, label: &str, now: i64) -> bool {
        self.error.is_none() && self.list(rt).iter().any(|e| e.enabled && e.label == label) && self.free(rt, label, now) && !self.usage.avoided(rt, label, now)
    }

    /// A runtime with more than one account: failover and its hold apply; one account keeps the plain § 4 hold.
    pub fn multi(&self, rt: &str) -> bool { applies(rt) && self.list(rt).len() > 1 }

    /// The account a usage limit moves to: only for a runtime with more than one account.
    pub fn spare(&self, rt: &str, now: i64) -> Option<String> {
        self.multi(rt).then(|| self.active(rt, now)).flatten()
    }

    /// The hold end with none left: the earliest live limit end of an enabled account and whether it was reported.
    pub fn hold(&self, rt: &str, now: i64) -> Option<(i64, bool)> {
        self.list(rt).iter().filter(|e| e.enabled && !self.usage.avoided(rt, &e.label, now)).filter_map(|e| self.limit(rt, &e.label)).filter(|l| l.end() > now)
            .map(|l| (l.end(), l.until.is_some())).min()
    }

    /// Every account of `rt` disabled or avoided by fleet policy: the runtime admits nothing on this host.
    pub fn off(&self, rt: &str, now: i64) -> bool { applies(rt) && self.list(rt).iter().all(|e| !e.enabled || self.usage.avoided(rt, &e.label, now)) }

    /// Why [`Host::off`] holds.
    pub fn off_reason(&self, rt: &str) -> &'static str {
        if self.list(rt).iter().all(|e| !e.enabled) { "every account disabled on this host" } else { "every account disabled or avoided on this host" }
    }

    /// Heartbeat lines of the listed runtimes: labels and state only, never an email, path or token.
    pub fn heartbeat(&self, now: i64) -> Vec<String> {
        let mut lines: Vec<String> = self.policy.keys().chain(self.state.keys()).collect::<BTreeSet<_>>().into_iter().flat_map(|rt| {
            let active = self.active(rt, now);
            self.list(rt).into_iter().map(move |e| line(rt, &e, active.as_deref() == Some(e.label.as_str()), self.limit(rt, &e.label))).collect::<Vec<_>>()
        }).collect();
        if let Some(e) = &self.error { lines.push(format!("quota unknown: {e}")); }
        for (rt, accounts) in self.observations.as_object().into_iter().flatten() {
            for (label, q) in accounts.as_object().into_iter().flatten() {
                for w in windows(q) {
                    if let Ok(l) = parse_limit(&w) {
                        lines.push(format!("quota {rt} account {label} window {} {}{}",w["window"].as_str().unwrap_or("unknown"), reset(&l), if l.end() <= now {"; probe due"}else{""}));
                    }
                }
            }
        }
        lines
    }

    /// The heartbeat's usage rows and `account-policy` lines: fingerprints, percents and fixed reasons only.
    pub fn usage_lines(&self, now: i64) -> Vec<String> {
        let mut lines = vec![];
        for rt in crate::runtime::CLI_RUNTIMES.iter().filter(|rt| applies(rt)) {
            for e in self.list(rt) { lines.extend(self.usage.lines(rt, &e.label, e.enabled, now)); }
        }
        lines.extend(self.usage.policy_lines(now, |rt| self.list(rt).iter().filter_map(|e| self.usage.id(rt, &e.label).map(String::from)).collect()));
        lines
    }

    /// The local rows of `taskq accounts`; `email` reads each home's login (None: unknown).
    pub fn rows(&self, host: &str, now: i64, email: impl Fn(&str, &str) -> Option<String>) -> Vec<Row> {
        let mut rows: Vec<Row> = crate::runtime::CLI_RUNTIMES.iter().filter(|rt| applies(rt)).flat_map(|rt| {
            let active = self.active(rt, now);
            self.list(rt).into_iter().map(|e| {
                let l = self.limit(rt, &e.label);
                let email = email(rt, &e.label);
                let id = email.as_deref().map(super::usage::fingerprint).or_else(|| self.usage.id(rt, &e.label).map(String::from));
                Row { active: active.as_deref() == Some(e.label.as_str()), limited: l.map(|l| crate::time::stamp(l.at)), reset: l.map(reset), email, id,
                    ..blank(host, rt, &e.label, e.enabled) }
            }).collect::<Vec<_>>()
        }).collect();
        join_usage(&mut rows, host, self.usage_lines(now).iter().map(String::as_str));
        rows
    }
}

fn reset(l: &Limit) -> String { format!("{} {}", if l.until.is_some() { "until" } else { "probe" }, crate::time::stamp(l.end())) }

fn line(rt: &str, e: &Entry, active: bool, limit: Option<&Limit>) -> String {
    format!("account {rt} {} {}{}{}", e.label, if e.enabled { "on" } else { "off" }, if active { " active" } else { "" },
        limit.map(|l| format!(" limited {} {}", crate::time::stamp(l.at), reset(l))).unwrap_or_default())
}

/// One row of `taskq accounts`.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub host: String,
    pub runtime: String,
    pub label: String,
    /// Only on its own host, when the login probe names one.
    pub email: Option<String>,
    pub enabled: bool,
    pub active: bool,
    pub limited: Option<String>,
    /// `until <time>` (reported) or `probe <time>`.
    pub reset: Option<String>,
    /// The account fingerprint: the one identity a heartbeat carries.
    pub id: Option<String>,
    /// One cell per polled window, or `unknown <reason>`.
    pub usage: Vec<String>,
    /// `use`, `avoid`, `unmet` or `local <reason>`.
    pub policy: Option<String>,
}

impl Row {
    pub fn json(&self) -> Value {
        json!({"host": self.host, "runtime": self.runtime, "label": self.label, "email": self.email, "enabled": self.enabled,
            "active": self.active, "limited": self.limited, "reset": self.reset, "id": self.id, "usage": self.usage, "policy": self.policy})
    }

    pub fn text(&self) -> String {
        format!("{}  {} {}{}  {}  id {}  {}  usage {}  limited {}  reset {}  policy {}", self.host, self.runtime, self.label, if self.active { " *" } else { "" },
            self.email.as_deref().unwrap_or("-"), self.id.as_deref().unwrap_or("-"), if self.enabled { "enabled" } else { "disabled" },
            if self.usage.is_empty() { "-".to_string() } else { self.usage.join(", ") },
            self.limited.as_deref().unwrap_or("-"), self.reset.as_deref().unwrap_or("-"), self.policy.as_deref().unwrap_or("-"))
    }
}

fn blank(host: &str, rt: &str, label: &str, enabled: bool) -> Row {
    Row { host: host.into(), runtime: rt.into(), label: label.into(), email: None, enabled, active: false, limited: None, reset: None, id: None, usage: vec![], policy: None }
}

/// Adds one host's usage rows and `account-policy` lines to its rows: a host without a list has rows from these alone,
/// and a `use` with no home here is a row of its own.
fn join_usage<'a>(rows: &mut Vec<Row>, host: &str, lines: impl Iterator<Item = &'a str>) {
    use super::usage::Usage;
    let disabled = Usage::Unknown("disabled".into());
    let mut policy = vec![];
    for l in lines {
        if let Some(rest) = l.strip_prefix("account-policy ") { policy.push(rest); continue; }
        let Some(u) = super::usage::Row::parse(l) else { continue };
        let i = rows.iter().position(|r| r.runtime == u.rt && r.label == u.label).unwrap_or_else(|| {
            rows.push(blank(host, &u.rt, &u.label, u.usage != disabled));
            rows.len() - 1
        });
        if rows[i].id.is_none() { rows[i].id = u.id.clone(); }
        if u.usage != disabled { rows[i].usage.push(u.text()); }
    }
    for p in policy {
        match p.split(' ').collect::<Vec<_>>()[..] {
            ["local", ..] => rows.iter_mut().for_each(|r| r.policy = Some(p.to_string())),
            ["unmet", rt, id] if applies(rt) => rows.push(Row { id: Some(id.to_string()), policy: Some("unmet".into()), ..blank(host, rt, "-", false) }),
            [state @ ("use" | "avoid"), rt, id] => rows.iter_mut().filter(|r| r.runtime == rt && r.id.as_deref() == Some(id)).for_each(|r| r.policy = Some(state.to_string())),
            _ => {}
        }
    }
}

/// The rows of another host's heartbeat `account`, usage and `account-policy` lines.
pub fn heartbeat_rows(host: &str, body: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = body.lines().filter_map(|l| {
        let w: Vec<&str> = l.strip_prefix("account ")?.split(' ').collect();
        let (rt, label, on) = (*w.first()?, *w.get(1)?, *w.get(2)?);
        if !applies(rt) || !valid_label(label) || !matches!(on, "on" | "off") { return None; }
        let rest = &w[3..];
        let active = rest.first() == Some(&"active");
        let rest = if active { &rest[1..] } else { rest };
        let (limited, reset) = match rest {
            ["limited", at, kind @ ("until" | "probe"), end] => (Some(at.to_string()), Some(format!("{kind} {end}"))),
            _ => (None, None),
        };
        Some(Row { active, limited, reset, ..blank(host, rt, label, on == "on") })
    }).collect();
    join_usage(&mut rows, host, body.lines());
    rows
}

// ---------- homes ----------

/// The runtime's own home, including an explicit override inside a named account.
pub fn default_home(rt: &str) -> Option<PathBuf> {
    crate::runtime::of(rt).native_home.path()
}

/// The parent home containing TaskQ's named accounts.
pub fn base(rt: &str) -> Option<PathBuf> {
    let b = default_home(rt)?;
    if b.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new(HOMES)) {
        return b.parent().and_then(Path::parent).map(Path::to_path_buf);
    }
    Some(b)
}

/// The variable that names a home to the runtime.
pub fn var(rt: &str) -> &'static str { crate::runtime::of(rt).home_var }

/// Native default semantics depend on an absent override, even when the physical path is the same.
pub fn environment<'a>(rt: &str, home: &'a Path) -> Option<(&'static str, &'a Path)> {
    let var = var(rt);
    // a runtime with no home variable has only its native default
    (!var.is_empty() && (std::env::var_os(var).is_some() || default_home(rt).as_deref() != Some(home))).then_some((var, home))
}

/// The home of a non-default account; None for `default` (the child keeps the service's own value).
pub fn home(rt: &str, label: &str) -> Option<PathBuf> {
    (applies(rt) && label != DEFAULT).then(|| base(rt).map(|b| b.join(HOMES).join(label))).flatten()
}

/// The active account's home of `rt` under `base` on `install` (`base` itself when `default` is active, none is
/// available, or `base` is already an account home).
pub fn active_in(install: &Path, rt: &str, base: &Path) -> PathBuf {
    if !applies(rt) || base.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new(HOMES)) {
        return base.to_path_buf();
    }
    match Host::read(install).active(rt, now()) {
        Some(l) if l != DEFAULT => base.join(HOMES).join(l),
        _ => base.to_path_buf(),
    }
}

/// Every home of `rt` on this host: the default first, then each account home that exists (transcript lookup).
pub fn homes(rt: &str) -> Vec<PathBuf> {
    let Some(b) = base(rt) else { return vec![] };
    let mut v = vec![b.clone()];
    let mut more: Vec<PathBuf> = std::fs::read_dir(b.join(HOMES)).into_iter().flatten().flatten()
        .filter(|e| e.file_name().to_str().is_some_and(valid_label) && e.path().is_dir()).map(|e| e.path()).collect();
    more.sort();
    v.extend(more);
    v
}

// ---------- attempt log ----------

/// Records the account of a new start in its attempt log, after the `taskq.rules` line.
pub fn mark(log: &Path, label: &str) -> std::io::Result<()> {
    writeln!(std::fs::File::options().create(true).append(true).open(log)?, "{}", json!({"type": "taskq.account", "account": label}))
}

/// The account an attempt log names; `default` for a log without one (an older attempt).
pub fn of_log(log: &Path) -> String {
    let Ok(f) = std::fs::File::open(log) else { return DEFAULT.into() };
    // the line follows the first rules line; later lines are runtime output
    std::io::BufReader::new(f).lines().take(16).map_while(Result::ok).find_map(|l| {
        let v: Value = serde_json::from_str(&l).ok()?;
        (v["type"] == "taskq.account").then(|| v["account"].as_str().filter(|a| valid_label(a)).map(String::from)).flatten()
    }).unwrap_or_else(|| DEFAULT.into())
}

/// The home an attempt log's account names; None for `default`.
pub fn log_home(rt: &str, log: &Path) -> Option<PathBuf> { home(rt, &of_log(log)) }

// ---------- switches ----------

/// One change of a runtime's active account (A6): the service log line and telemetry counts.
pub struct Switch {
    pub runtime: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub reason: &'static str,
}

impl Switch {
    pub fn line(&self) -> String {
        format!("account {} {} -> {} ({})", self.runtime, self.from.as_deref().unwrap_or("none"), self.to.as_deref().unwrap_or("none"), self.reason)
    }

    pub fn counts(&self, host: &str) -> BTreeMap<String, u64> {
        [format!("host:{host}"), format!("runtime:{}", self.runtime), format!("from:{}", self.from.as_deref().unwrap_or("none")),
            format!("to:{}", self.to.as_deref().unwrap_or("none")), format!("reason:{}", self.reason)].into_iter().map(|k| (k, 1)).collect()
    }
}

/// The switches since `seen` (runtime -> active account); updates `seen`. The first sight of a runtime records it
/// without a switch.
pub fn switches(h: &Host, now: i64, seen: &mut BTreeMap<String, Option<String>>) -> Vec<Switch> {
    let mut out = vec![];
    for rt in crate::runtime::CLI_RUNTIMES {
        let to = h.active(rt, now);
        match seen.insert(rt.to_string(), to.clone()) {
            Some(from) if from != to => {
                let reason = match &from {
                    _ if h.error.is_some() => "unknown",
                    Some(f) if !h.free(rt, f, now) => "limit",
                    // a limit that ended brings back an account ahead of the current one
                    _ if to.as_ref().is_some_and(|t| h.limit(rt, t).is_some()) => "reset",
                    // fleet policy names the account it moves to, or the one it moves from
                    _ if to.as_ref().is_some_and(|t| h.usage.preferred(rt, t, now)) || from.as_ref().is_some_and(|f| h.usage.avoided(rt, f, now)) => "fleet",
                    Some(f) if h.list(rt).iter().any(|e| e.enabled && e.label == *f)
                        && (h.usage.remaining(rt, f, now).is_some() || to.as_ref().is_some_and(|t| h.usage.remaining(rt, t, now).is_some())) => "usage",
                    _ => "policy",
                };
                out.push(Switch { runtime: rt.to_string(), from, to, reason });
            }
            _ => {}
        }
    }
    out
}

// ---------- `taskq accounts` writes (local host only) ----------

fn guard() -> Result<(), String> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() || std::env::var_os("TASKQ_ROLE").is_some_and(|r| r == "worker" || r == "reviewer") {
        return Err("a worker cannot change runtime accounts".into());
    }
    Ok(())
}

fn runtime_arg(rt: &str) -> Result<&str, String> {
    applies(rt).then_some(rt).ok_or_else(|| format!("runtime {rt} has no accounts; use claude, codex or antigravity"))
}

/// A runtime with no home variable cannot keep a second login apart from its default.
fn one_account(rt: &str) -> Result<(), String> {
    if var(rt).is_empty() { Err(format!("{rt} has one `default` account per host until a per-account home is qualified")) } else { Ok(()) }
}

/// enable/disable/move on a list; the caller saves it.
fn change(p: &mut Policy, op: &str, rt: &str, label: &str, position: Option<usize>) -> Result<String, String> {
    let list = p.entry(rt.into()).or_insert_with(default_list);
    let i = list.iter().position(|e| e.label == label).ok_or_else(|| format!("{rt} has no account {label}; `taskq accounts` lists them"))?;
    match op {
        "enable" | "disable" => {
            list[i].enabled = op == "enable";
            let all_off = list.iter().all(|e| !e.enabled);
            let note = if all_off { format!("; every account is disabled, so this host starts no new {rt} work") } else { String::new() };
            Ok(format!("accounts: {rt} {label} {op}d{note}"))
        }
        "move" => {
            let to = position.filter(|n| (1..=list.len()).contains(n)).ok_or(format!("position must be 1 to {}", list.len()))?;
            let e = list.remove(i);
            list.insert(to - 1, e);
            Ok(format!("accounts: {rt} {label} at {to} of {}", list.len()))
        }
        _ => Err(format!("unknown accounts action {op}")),
    }
}

/// `taskq accounts enable|disable|move` on `install`.
pub fn write(install: &Path, op: &str, rt: &str, label: &str, position: Option<usize>) -> Result<String, String> {
    guard()?;
    let rt = runtime_arg(rt)?;
    let mut p = read_policy(install)?;
    let out = change(&mut p, op, rt, label, position)?;
    write_policy(install, &p)?;
    Ok(format!("{out}; the next start obeys it, running workers keep their login"))
}

/// Adds `label` to the end of `rt`'s list after a successful login.
fn append(p: &mut Policy, rt: &str, label: &str) -> Result<(), String> {
    let list = p.entry(rt.into()).or_insert_with(default_list);
    if list.iter().any(|e| e.label == label) { return Err(format!("{rt} already has account {label}")); }
    list.push(Entry { label: label.into(), enabled: true });
    Ok(())
}

/// `taskq accounts add`: a native device login into the new empty home of `label`; never copies a login.
pub fn add(install: &Path, rt: &str, label: &str) -> Result<String, String> {
    guard()?;
    let rt = runtime_arg(rt)?;
    one_account(rt)?;
    let base = base(rt).ok_or("runtime home unknown: no home folder")?;
    let bin = crate::runtime::which(crate::runtime::of(rt).bin);
    if !bin.is_absolute() { return Err(format!("{} CLI not found on PATH", crate::runtime::of(rt).bin)); }
    add_in(install, rt, label, &base, |home, args| {
        crate::platform::console_command(&bin).args(args).env(var(rt), home).status().map_err(|e| format!("{rt} login: {e}")).map(|s| s.success())
    })
}

fn add_in(install: &Path, rt: &str, label: &str, base: &Path, login: impl FnOnce(&Path, &[&str]) -> Result<bool, String>) -> Result<String, String> {
    let rt = runtime_arg(rt)?;
    one_account(rt)?;
    if !valid_label(label) || label == DEFAULT { return Err("label: 1-32 of a-z, 0-9 and -, not `default`".into()); }
    let mut p = read_policy(install)?;
    if p.get(rt).is_some_and(|l| l.iter().any(|e| e.label == label)) { return Err(format!("{rt} already has account {label}")); }
    let home = base.join(HOMES).join(label);
    if std::fs::read_dir(&home).is_ok_and(|mut d| d.next().is_some()) { return Err(format!("{rt} account {label} home is not empty; pick another label")); }
    std::fs::create_dir_all(&home).map_err(|e| format!("account home: {e}"))?;
    let args = crate::runtime::of(rt).device_login;
    // the owner's terminal runs the native login; TaskQ never sees the token
    if !login(&home, args)? { return Err(format!("{rt} login for {label} did not finish; the list is unchanged")); }
    if crate::runtime::of(rt).has(Cap::AuthFile) && !home.join("config.toml").exists() {
        let cfg = std::fs::symlink_metadata(base.join("config.toml")).ok().filter(|m| m.is_file()).and_then(|_| std::fs::read(base.join("config.toml")).ok()).unwrap_or_default();
        std::fs::write(home.join("config.toml"), cfg).map_err(|e| format!("account config.toml: {e}"))?;
    }
    append(&mut p, rt, label)?;
    write_policy(install, &p)?;
    Ok(format!("accounts: {rt} {label} added at {} of {}, enabled", p[rt].len(), p[rt].len()))
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
