//! Runtime accounts per host ([taskq.md § 3 Runtime accounts](../taskq.md#runtime-accounts),
//! [detail](../docs/design/capacity.md#runtime-accounts)): an ordered list of named login homes per CLI runtime,
//! each enabled or disabled. A usage limit on one account moves new starts to the next enabled one.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// The owner's list per runtime, in `<install>`.
pub const FILE: &str = "accounts.json";
/// The service's limit record per runtime and label, in `<install>`.
pub const STATE: &str = "accounts-state.json";
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

pub fn applies(rt: &str) -> bool { matches!(rt, "claude" | "codex") }

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
    std::fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let _lock = coreq::file_lock::lock(&install.join("accounts-state.lock")).map_err(|e| format!("quota state busy or unavailable: {e}"))?;
    let mut v = state_value(install)?;
    if v.get(rt).is_none() { v[rt] = json!({}); }
    let mut w: Vec<_> = windows(&v[rt][label]).into_iter().filter(|w| w["window"] != "probe" && parse_limit(w).is_ok_and(|l| l.end() > at)).collect();
    for (window, until) in observed {
        let until = until.filter(|t| *t > at);
        if !w.iter().any(|w| w["window"] == *window && w["until"] == json!(until)) {
            w.push(json!({"window":window,"limited":at,"until":until}));
        }
    }
    if w.is_empty() { return Err("quota observation missing".into()); }
    let packed=pack(w);
    if v[rt][label] == packed { return Ok(()); }
    v[rt][label] = packed;
    save_state(install, &v)?;
    drop(_lock);
    crate::profile::measure_counted("account.quota", || ((), [format!("runtime:{rt}"),format!("account:{label}")].into_iter().chain(observed.iter().map(|(w,_)|format!("window:{w}"))).map(|k|(k,1)).collect()));
    Ok(())
}

pub fn record_limit(install: &Path, rt: &str, label: &str, at: i64, until: Option<i64>) -> Result<(), String> {
    record_windows(install, rt, label, at, &[("unknown".into(),until)])
}

/// Atomically reserve a due probe. No second project may probe this account during the bounded interval.
/// A resume supplies its original label; a new launch selects the first available enabled account.
pub fn claim(install: &Path, rt: &str, label: Option<&str>, now: i64, owner: &Path) -> Result<String, String> {
    std::fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let _lock = coreq::file_lock::try_lock(&install.join("accounts-state.lock")).map_err(|e| format!("quota state busy or unavailable: {e}"))?;
    let h = Host::read(install);
    if let Some(e) = h.error { return Err(e); }
    let label = match label { Some(l) if h.free(rt,l,now) => l.to_string(), Some(_) => return Err(format!("{rt}: original account quota hold")), None => h.active(rt,now).ok_or_else(|| format!("{rt}: no available enabled account"))? };
    let mut v = state_value(install)?;
    if parse_limit(&v[rt][&label]).is_ok_and(|l| l.end() <= now) {
        v[rt][&label] = pack(vec![json!({"window":"probe","limited":now,"until":null})]);
        v[rt][&label]["probe_owner"] = json!(owner);
        save_state(install,&v)?;
        drop(_lock);
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
}

impl Host {
    pub fn read(install: &Path) -> Host {
        let (policy, error) = match read_policy(install) { Ok(p) => (p, None), Err(_) => (Policy::new(), Some("host account policy unreadable".into())) };
        match state_value(install) {
            Ok(observations) => Host { policy, state: summaries(&observations), observations, error },
            Err(e) => Host { policy, error: Some(error.unwrap_or(e)), ..Default::default() },
        }
    }

    /// The list of `rt`, in priority order.
    pub fn list(&self, rt: &str) -> Vec<Entry> { self.policy.get(rt).cloned().unwrap_or_else(default_list) }

    fn limit(&self, rt: &str, label: &str) -> Option<&Limit> { self.state.get(rt)?.get(label) }

    fn free(&self, rt: &str, label: &str, now: i64) -> bool { self.limit(rt, label).is_none_or(|l| l.end() <= now) }

    /// The first enabled account whose limit has ended.
    pub fn active(&self, rt: &str, now: i64) -> Option<String> {
        self.list(rt).into_iter().find(|e| self.error.is_none() && e.enabled && self.free(rt, &e.label, now)).map(|e| e.label)
    }

    /// The account a new start takes: the active one, else the first enabled one; None when all are disabled.
    pub fn pick(&self, rt: &str, now: i64) -> Option<String> {
        self.active(rt, now)
    }

    /// A runtime with more than one account: failover and its hold apply; one account keeps the plain § 4 hold.
    pub fn multi(&self, rt: &str) -> bool { applies(rt) && self.list(rt).len() > 1 }

    /// The account a usage limit moves to: only for a runtime with more than one account.
    pub fn spare(&self, rt: &str, now: i64) -> Option<String> {
        self.multi(rt).then(|| self.active(rt, now)).flatten()
    }

    /// The hold end with none left: the earliest live limit end of an enabled account and whether it was reported.
    pub fn hold(&self, rt: &str, now: i64) -> Option<(i64, bool)> {
        self.list(rt).iter().filter(|e| e.enabled).filter_map(|e| self.limit(rt, &e.label)).filter(|l| l.end() > now)
            .map(|l| (l.end(), l.until.is_some())).min()
    }

    /// Every account of `rt` disabled: the runtime admits nothing on this host.
    pub fn off(&self, rt: &str) -> bool { applies(rt) && self.list(rt).iter().all(|e| !e.enabled) }

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

    /// The local rows of `taskq accounts`; `email` reads each home's login (None: unknown).
    pub fn rows(&self, host: &str, now: i64, email: impl Fn(&str, &str) -> Option<String>) -> Vec<Row> {
        ["claude", "codex"].iter().flat_map(|rt| {
            let active = self.active(rt, now);
            self.list(rt).into_iter().map(|e| {
                let l = self.limit(rt, &e.label);
                Row { host: host.into(), runtime: rt.to_string(), email: email(rt, &e.label), active: active.as_deref() == Some(e.label.as_str()),
                    limited: l.map(|l| crate::board::stamp(l.at)), reset: l.map(reset), label: e.label, enabled: e.enabled }
            }).collect::<Vec<_>>()
        }).collect()
    }
}

fn reset(l: &Limit) -> String { format!("{} {}", if l.until.is_some() { "until" } else { "probe" }, crate::board::stamp(l.end())) }

fn line(rt: &str, e: &Entry, active: bool, limit: Option<&Limit>) -> String {
    format!("account {rt} {} {}{}{}", e.label, if e.enabled { "on" } else { "off" }, if active { " active" } else { "" },
        limit.map(|l| format!(" limited {} {}", crate::board::stamp(l.at), reset(l))).unwrap_or_default())
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
}

impl Row {
    pub fn json(&self) -> Value {
        json!({"host": self.host, "runtime": self.runtime, "label": self.label, "email": self.email, "enabled": self.enabled,
            "active": self.active, "limited": self.limited, "reset": self.reset})
    }

    pub fn text(&self) -> String {
        format!("{}  {} {}{}  {}  {}  limited {}  reset {}", self.host, self.runtime, self.label, if self.active { " *" } else { "" },
            self.email.as_deref().unwrap_or("-"), if self.enabled { "enabled" } else { "disabled" },
            self.limited.as_deref().unwrap_or("-"), self.reset.as_deref().unwrap_or("-"))
    }
}

/// The rows of another host's heartbeat `account` lines.
pub fn heartbeat_rows(host: &str, body: &str) -> Vec<Row> {
    body.lines().filter_map(|l| {
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
        Some(Row { host: host.into(), runtime: rt.into(), label: label.into(), email: None, enabled: on == "on", active, limited, reset })
    }).collect()
}

// ---------- homes ----------

/// The runtime's own home: `CLAUDE_CONFIG_DIR` / `CODEX_HOME` of this process, else `~/.claude` / `~/.codex`.
pub fn base(rt: &str) -> Option<PathBuf> {
    if rt == "claude" { crate::runtime::claude_home() } else { crate::runtime::codex_home() }
}

/// The variable that names a home to the runtime.
pub fn var(rt: &str) -> &'static str { if rt == "claude" { "CLAUDE_CONFIG_DIR" } else { "CODEX_HOME" } }

/// The home of a non-default account; None for `default` (the child keeps the service's own value).
pub fn home(rt: &str, label: &str) -> Option<PathBuf> {
    (applies(rt) && label != DEFAULT).then(|| base(rt).map(|b| b.join(HOMES).join(label))).flatten()
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
    applies(rt).then_some(rt).ok_or_else(|| format!("runtime {rt} has no accounts; use claude or codex"))
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
    if !valid_label(label) || label == DEFAULT { return Err("label: 1-32 of a-z, 0-9 and -, not `default`".into()); }
    let mut p = read_policy(install)?;
    if p.get(rt).is_some_and(|l| l.iter().any(|e| e.label == label)) { return Err(format!("{rt} already has account {label}")); }
    let home = home(rt, label).ok_or("runtime home unknown: no home folder")?;
    if std::fs::read_dir(&home).is_ok_and(|mut d| d.next().is_some()) { return Err(format!("{rt} account {label} home is not empty; pick another label")); }
    std::fs::create_dir_all(&home).map_err(|e| format!("account home: {e}"))?;
    let bin = crate::runtime::which(crate::runtime::of(rt).bin);
    if !bin.is_absolute() { return Err(format!("{} CLI not found on PATH", crate::runtime::of(rt).bin)); }
    let args: &[&str] = if rt == "claude" { &["auth", "login"] } else { &["login", "--device-auth"] };
    // the owner's terminal runs the native login; TaskQ never sees the token
    let ok = crate::platform::console_command(&bin).args(args).env(var(rt), &home).status().map_err(|e| format!("{} login: {e}", rt))?.success();
    if !ok { return Err(format!("{rt} login for {label} did not finish; the list is unchanged")); }
    append(&mut p, rt, label)?;
    write_policy(install, &p)?;
    Ok(format!("accounts: {rt} {label} added at {} of {}, enabled", p[rt].len(), p[rt].len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(policy: &[(&str, &[(&str, bool)])], limits: &[(&str, &str, i64, Option<i64>)]) -> Host {
        let mut h = Host::default();
        for (rt, list) in policy { h.policy.insert(rt.to_string(), list.iter().map(|(l, e)| Entry { label: l.to_string(), enabled: *e }).collect()); }
        for (rt, l, at, until) in limits { h.state.entry(rt.to_string()).or_default().insert(l.to_string(), Limit { at: *at, until: *until }); }
        h
    }

    // #2137 A5-A8: one account probe across projects/restart, independent windows and unknown-state fencing.
    #[test]
    fn balancing_quota_windows_and_bounded_probes() {
        let dir = std::env::temp_dir().join(format!("taskq-quota-probes-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        record_windows(&dir,"claude","default",0,&[("five_hour".into(),Some(18000)),("seven_day".into(),Some(604800))]).unwrap();
        let h=Host::read(&dir);
        assert!(h.active("claude",18001).is_none());
        assert!(h.heartbeat(18001).iter().any(|l|l.contains("account default window seven_day until")));
        let (first,second)=(dir.join("attempt-one"),dir.join("attempt-two"));
        assert!(claim(&dir,"claude",None,18001,&first).is_err());
        assert_eq!(claim(&dir,"claude",None,604801,&first).unwrap(),"default");
        assert!(claim(&dir,"claude",None,604802,&second).is_err(),"restart must retain the claimed probe");
        confirm_probe(&dir,"claude","default",&second).unwrap();
        assert!(Host::read(&dir).active("claude",604802).is_none(),"another execution cannot clear the probe");
        confirm_probe(&dir,"claude","default",&first).unwrap();
        assert_eq!(Host::read(&dir).active("claude",604802).as_deref(),Some("default"));
        record_limit(&dir,"codex","default",100,None).unwrap();
        assert!(claim(&dir,"codex",None,1899,&first).is_err());
        assert!(claim(&dir,"codex",None,1900,&first).is_ok());
        assert!(claim(&dir,"codex",None,1901,&second).is_err());
        std::fs::write(dir.join(STATE),"corrupt").unwrap();
        assert!(Host::read(&dir).error.is_some());
        assert!(claim(&dir,"codex",None,999999,&first).is_err(),"unknown is never available");
        std::fs::remove_dir_all(dir).unwrap();
    }

    // #2137 A5/A7: a shorter quota window must not erase a longer hold across restart.
    #[test]
    fn balancing_overlapping_quota_survives_restart() {
        let dir = std::env::temp_dir().join(format!("taskq-quota-overlap-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        record_limit(&dir, "codex", "default", 0, Some(604800)).unwrap();
        record_limit(&dir, "codex", "default", 1, Some(18000)).unwrap();
        let h = Host::read(&dir);
        assert_eq!(h.active("codex", 18001), None, "a five-hour reset cannot clear a weekly window");
        assert_eq!(h.hold("codex", 18001), Some((604800, true)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: §3 Runtime accounts (#2043 A3, A4): order and disable pick the start account; a limit moves to the next
    // enabled account; with none left the hold ends at the earliest limit end. Risk: a disabled or limited account
    // still takes new workers, or a host with a spare account holds the runtime.
    #[test]
    fn order_disable_failover_and_exhausted_hold() {
        let none = host(&[], &[]);
        assert_eq!(none.list("codex"), default_list(), "no list: one default account");
        assert_eq!((none.pick("codex", 0), none.spare("codex", 0)), (Some("default".into()), None), "a lone account never fails over");

        let h = host(&[("codex", &[("default", true), ("work", true), ("spare", true)])], &[]);
        assert_eq!(h.pick("codex", 0).as_deref(), Some("default"), "list order is priority");
        let h = host(&[("codex", &[("default", false), ("work", true), ("spare", true)])], &[]);
        assert_eq!(h.pick("codex", 0).as_deref(), Some("work"), "a disabled account takes no start");

        let h = host(&[("codex", &[("default", true), ("work", true), ("spare", false)])], &[("codex", "default", 100, None)]);
        assert_eq!(h.spare("codex", 200).as_deref(), Some("work"), "a limit moves to the next enabled account");
        assert_eq!(h.hold("codex", 200), Some((100 + PROBE, false)));
        assert_eq!(h.spare("codex", 100 + PROBE).as_deref(), Some("default"), "the probe ends the limit and order returns");

        let h = host(&[("codex", &[("default", true), ("work", true), ("spare", false)])], &[("codex", "default", 100, Some(9000)), ("codex", "work", 150, None)]);
        assert_eq!(h.spare("codex", 200), None, "every enabled account limited: no spare");
        assert_eq!(h.hold("codex", 200), Some((150 + PROBE, false)), "the hold ends at the earliest enabled limit end");
        assert_eq!(h.pick("codex", 200), None, "an exhausted account cannot start");
        assert!(!h.off("codex"));
        let h = host(&[("claude", &[("default", false)])], &[]);
        assert!(h.off("claude") && h.pick("claude", 0).is_none(), "all disabled: the runtime admits nothing");
        assert!(!h.off("codex-app") && h.spare("codex-app", 0).is_none(), "codex-app has no accounts");
    }

    // Chain: §3 Runtime accounts (#2043 A2, A6): heartbeat lines carry labels and state, never an email; another host
    // reads them back as rows; every change of the active account is one switch with its reason.
    #[test]
    fn heartbeat_rows_and_switches() {
        let h = host(&[("codex", &[("default", true), ("work", false)])], &[("codex", "default", 0, Some(60))]);
        let lines = h.heartbeat(10);
        assert_eq!(lines, ["account codex default on limited 1970-01-01T00:00:00Z until 1970-01-01T00:01:00Z", "account codex work off"]);
        let rows = heartbeat_rows("mac", &format!("version 1\n{}\naccount codex work on active", lines.join("\n")));
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].limited.as_deref(), rows[0].reset.as_deref(), rows[0].email.as_ref()), (Some("1970-01-01T00:00:00Z"), Some("until 1970-01-01T00:01:00Z"), None));
        assert!(rows[2].active && rows[2].enabled && !rows[1].enabled);
        assert!(heartbeat_rows("mac", "account codex-app default on\naccount codex Bad on").is_empty(), "foreign runtimes and labels are dropped");

        let mut seen = BTreeMap::new();
        let mut h = host(&[("codex", &[("default", true), ("work", true)])], &[]);
        assert!(switches(&h, 0, &mut seen).is_empty(), "the first pass records without a switch");
        h.state.entry("codex".into()).or_default().insert("default".into(), Limit { at: 0, until: Some(100) });
        let s = switches(&h, 10, &mut seen);
        assert_eq!(s.iter().map(Switch::line).collect::<Vec<_>>(), ["account codex default -> work (limit)"]);
        assert_eq!(s[0].counts("mac").keys().cloned().collect::<Vec<_>>(), ["from:default", "host:mac", "reason:limit", "runtime:codex", "to:work"]);
        assert_eq!(switches(&h, 100, &mut seen).iter().map(Switch::line).collect::<Vec<_>>(), ["account codex work -> default (reset)"]);
        h.policy.get_mut("codex").unwrap()[0].enabled = false;
        assert_eq!(switches(&h, 100, &mut seen).iter().map(Switch::line).collect::<Vec<_>>(), ["account codex default -> work (policy)"]);
    }

    // Chain: §3 Runtime accounts (#2043 A3, A5): enable, disable and move edit the local list; add appends once; the
    // attempt log names the account a resume keeps. Risk: a reorder drops an account or a resume changes login.
    #[test]
    fn list_writes_and_attempt_log_account() {
        let mut p = Policy::new();
        assert!(change(&mut p, "disable", "codex", "default", None).unwrap().contains("every account is disabled"));
        append(&mut p, "codex", "work").unwrap();
        assert!(append(&mut p, "codex", "work").is_err(), "a label is added once");
        change(&mut p, "move", "codex", "work", Some(1)).unwrap();
        assert_eq!(p["codex"], [Entry { label: "work".into(), enabled: true }, Entry { label: "default".into(), enabled: false }]);
        assert!(change(&mut p, "move", "codex", "work", Some(3)).is_err() && change(&mut p, "enable", "codex", "gone", None).is_err());
        assert_eq!(parse_policy(&policy_json(&p)).unwrap(), p, "the file round-trips");
        assert!(parse_policy(&json!({"codex-app": [{"label": "default", "enabled": true}]})).is_err());
        assert!(parse_policy(&json!({"codex": [{"label": "a", "enabled": true}, {"label": "a", "enabled": false}]})).is_err());

        let dir = std::env::temp_dir().join(format!("taskq-accounts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("T1-2.log");
        std::fs::write(&log, "{\"type\":\"taskq.rules\"}\n").unwrap();
        assert_eq!(of_log(&log), DEFAULT, "an older log without the line uses default");
        mark(&log, "work").unwrap();
        std::fs::OpenOptions::new().append(true).open(&log).unwrap().write_all(b"{\"type\":\"taskq.rules\",\"phase\":\"resume\"}\n").unwrap();
        assert_eq!(of_log(&log), "work");
        assert!(home("codex", DEFAULT).is_none() && home("codex-app", "work").is_none());
        assert!(home("codex", "work").is_some_and(|h| h.ends_with(Path::new(HOMES).join("work"))));
        record_limit(&dir, "codex", "work", 5, None).unwrap();
        record_limit(&dir, "codex", "default", 6, Some(9)).unwrap();
        assert_eq!(Host::read(&dir).state["codex"]["work"], Limit { at: 5, until: None });
        assert_eq!(Host::read(&dir).state["codex"]["default"].end(), 9);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
