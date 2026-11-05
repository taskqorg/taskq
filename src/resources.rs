//! Resource policy host adapter (docs/design/shared-resource-controls.md § Host adapter, `taskq.resources/1`): the
//! host operator's own resource grants, opaque project IDs and policy revisions, and the exchange that applies
//! DashQ-delivered operations through the canonical capacity writer. No listener, shell or second task queue: the
//! enrolled service posts its receipts and receives operations. State: `<install>/resource-state/` (kept by sweep).

use crate::{CapEdit, Res, enrollment, snapshot::Snapshot};
use coreq::enroll::{Https, Wire};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, thread::JoinHandle};

pub const SCHEMA: &str = "taskq.resources/1";
/// Relative to the connection's destination (`https://<dashq>/machine`): DashQ serves `/machine/resources/v1/exchange`.
pub const EXCHANGE: &str = "/resources/v1/exchange";
const FILE: &str = "state.json";
/// v1 bounds (§ Bounds and receipts).
const BODY: usize = 16 << 10;
const ITEMS: usize = 8;
const SPACING: u64 = 5;
/// Between exchanges with nothing outstanding.
const IDLE: u64 = 30;
const FRESH: u64 = 60;
const AHEAD: u64 = 30;
const RECONCILE: u64 = 300;
const RETAIN: u64 = 24 * 3600;
pub(crate) const MAX_OPS: usize = 4096;
/// Longest host grant, in hours.
pub const MAX_HOURS: u64 = 720;
use crate::service::RUNTIMES;

#[derive(Default, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct State {
    #[serde(default)]
    pub grants: BTreeMap<String, Grant>,
    #[serde(default)]
    pub projects: BTreeMap<String, Project>,
    #[serde(default)]
    pub ops: BTreeMap<String, Op>,
}

/// The host operator's resource grant: only operations naming this grant ID and actor, for this installation and
/// generation and within this scope, can write. DashQ's own grant is necessary, never sufficient.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub actor_id: String,
    pub installation_id: String,
    pub generation: u64,
    pub project_ids: Vec<String>,
    pub runtime_ids: Vec<String>,
    pub host_policy: bool,
    /// Admits `taskq.project-limits/1` within these projects and runtimes; absent in every older grant.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub project_limits: bool,
    pub expires_at: u64,
    pub revoked: bool,
    /// Provenance: the local OS account that wrote it and when.
    pub by: String,
    pub at: u64,
}

/// One registered project root: its opaque ID never moves to another root and is never reused.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub root: String,
    /// Opaque; new on every change of `policy` and on every canonical write.
    pub revision: String,
    /// Saved authority's capacity; host policy on host installs, legacy project policy otherwise. Null: unreadable.
    pub policy: Value,
    pub since: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Op {
    /// The parsed submit (or the cancellation that came first): immutable once recorded.
    pub bound: Value,
    pub receipt: Value,
    pub expires_at: u64,
    /// Time of the canonical write; a readback must be at least this new.
    #[serde(default)]
    pub saved_at: Option<u64>,
    /// Set before the write and cleared after: found set, the outcome is unknown and the write is never repeated.
    #[serde(default)]
    pub writing: bool,
    /// DashQ took this receipt.
    #[serde(default)]
    pub sent: bool,
}

fn local(o:&Op)->bool {o.bound.get("local_slot").is_some()||o.bound.get("local_capacity").is_some()||o.bound.get("local_restart").is_some()||o.bound.get("local_stop").is_some()||o.bound.get("local_uninstall").is_some()||o.bound.get("local_install").is_some()}

/// Never `resources`: with it, macOS reads `<install>` (`.../taskq` holding `taskq`) as a bundle, the launcher's signature
/// stops validating and TCC can never match its Screen Recording grant (#1395). An old store moves once; a failed move
/// keeps using it.
pub fn dir(install: &Path) -> PathBuf {
    let (new, old) = (install.join("resource-state"), install.join("resources"));
    if !new.exists() && old.is_dir() && fs::rename(&old, &new).is_err() { return old }
    new
}

/// An older host can recreate `resources` after the move (#1395), so `<install>` is a bundle again. Called once this
/// service owns the host, when no older host writes it: the superseded store moves into the real one as `superseded`,
/// kept for the operator (one copy, a later one replaces it).
pub fn retire_superseded(install: &Path) -> Res<Option<String>> {
    let (new, old) = (dir(install), install.join("resources"));
    if !old.is_dir() || !new.is_dir() { return Ok(None) }
    let kept = new.join("superseded");
    if kept.exists() { fs::remove_dir_all(&kept).map_err(|e| format!("resources: superseded copy kept: {e}"))? }
    fs::rename(&old, &kept).map_err(|e| format!("resources: superseded store not moved: {e}"))?;
    Ok(Some("resources: superseded <install>/resources moved to resource-state/superseded".into()))
}

fn read(install: &Path) -> Res<State> {
    match fs::symlink_metadata(dir(install).join("state.json.pending")) {
        Ok(_)=>return Err("resources state prepare retained; authority unknown".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        Err(e)=>return Err(format!("resources state prepare unknown: {e}")),
    }
    match fs::read_to_string(dir(install).join(FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(format!("resources: {e}")),
        // a corrupt store stays for the operator, never overwritten
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("resources state unreadable ({e}); file kept")),
    }
}

pub(crate) fn save(install: &Path, s: &State) -> Res<()> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())? + "\n";
    coreq::write_durable(&dir(install).join(FILE), &text)?;
    if read(install)? != *s {
        return Err("resources: readback differs".into());
    }
    Ok(())
}

/// The host lock, held until the file drops: resource writes and changes of the DashQ connection take turns under it.
fn lock(install: &Path) -> Res<Held> {
    fs::create_dir_all(dir(install)).map_err(|e| format!("resources: {e}"))?;
    coreq::file_lock::lock(&dir(install).join("lock")).map(|l|Held::new(l,install)).map_err(|e|format!("resources lock: {e}"))
}

/// The host lock with its holder's pid in `lock.holder`, removed before release, so a waiter can name it (#1415).
/// A sibling file: Windows locks are mandatory, so no other process can read the lock file itself.
pub(crate) struct Held(coreq::file_lock::Lock, PathBuf);
impl Held {
    fn new(lock: coreq::file_lock::Lock, install: &Path) -> Self {
        let holder = dir(install).join("lock.holder");
        let _ = fs::write(&holder, std::process::id().to_string());
        Held(lock, holder)
    }
}
impl Drop for Held {
    fn drop(&mut self) { let _ = fs::remove_file(&self.1); }
}

/// Install receipts and the DashQ resources send wait this long for a brief holder (#1415).
#[cfg(not(test))] pub(crate) const WAIT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(test)] pub(crate) const WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// The host lock within `wait` (zero: one try); a timeout names the holder pid when `lock.holder` records one.
pub(crate) fn acquire(install: &Path, wait: std::time::Duration) -> Res<Held> {
    poll(install, wait).map_err(|e| {
        let holder = fs::read_to_string(dir(install).join("lock.holder")).ok().and_then(|s| s.trim().parse::<u32>().ok()).map_or("holder unknown".into(), |pid| format!("held by pid {pid}"));
        format!("resources lock unavailable: {e}; waited {} ms, {holder}", wait.as_millis())
    })
}
fn poll(install: &Path, wait: std::time::Duration) -> Result<Held, fs::TryLockError> {
    fs::create_dir_all(dir(install)).map_err(fs::TryLockError::Error)?;
    let until = std::time::Instant::now() + wait;
    // ponytail: 50 ms polling of a non-blocking lock; a blocking lock with OS timeout if contention ever matters
    loop {
        match coreq::file_lock::try_lock(&dir(install).join("lock")) {
            Ok(lock) => return Ok(Held::new(lock, install)),
            Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => return Err(e),
        }
    }
}

/// `f` changes the DashQ connection under the host lock (`taskq disconnect`), so it never lands between an
/// operation's final connection check and its write.
pub fn locked<T>(install: &Path, f: impl FnOnce() -> Res<T>) -> Res<T> {
    let _lock = lock(install)?;
    f()
}

/// coreq's wire for calls whose reply can replace the active connection: once an ack (pairing activates a new one)
/// or a 401 (DashQ revoked it) arrives, it takes the host lock before coreq saves, and holds it until dropped.
pub struct Fenced<W> {
    wire: W,
    install: PathBuf,
    held: Option<Held>,
}

impl<W: Wire> Fenced<W> {
    pub fn new(install: &Path, wire: W) -> Self {
        Fenced { wire, install: install.to_path_buf(), held: None }
    }
}

impl<W: Wire> Wire for Fenced<W> {
    fn post(&mut self, dest: &str, route: &str, auth: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
        let r = self.wire.post(dest, route, auth, body)?;
        if self.held.is_none() && (route == "/connect/ack" || r.0 == 401) {
            self.held = Some(lock(&self.install)?);
        }
        Ok(r)
    }
    fn now(&self) -> u64 { self.wire.now() }
    fn sleep(&mut self, secs: u64) { self.wire.sleep(secs) }
    fn show(&mut self, uri: &str, code: &str) { self.wire.show(uri, code) }
}

/// `f` on the state under the host lock; written back when changed.
pub(crate) fn with<T>(install: &Path, f: impl FnOnce(&mut State) -> Res<T>) -> Res<T> {
    let _lock = lock(install)?;
    let mut s = read(install)?;
    let before = s.clone();
    let r = f(&mut s);
    if s != before {
        save(install, &s)?;
    }
    r
}

/// `with`, but waits at most `wait` for the host lock; `Ok(None)`: another writer held it, nothing read or written.
pub(crate) fn with_within<T>(install:&Path,wait:std::time::Duration,f:impl FnOnce(&mut State)->Res<T>)->Res<Option<T>> {
    let _lock=match poll(install,wait) {
        Ok(lock)=>lock,
        Err(fs::TryLockError::WouldBlock)=>return Ok(None),
        Err(fs::TryLockError::Error(e))=>return Err(format!("resources lock: {e}")),
    };
    let mut s=read(install)?;let before=s.clone();let r=f(&mut s);
    if s!=before {save(install,&s)?;}
    r.map(Some)
}

/// Host/UI observers retry on a later tick; they must not wait behind a policy writer.
pub(crate) fn try_with<T>(install:&Path,f:impl FnOnce(&mut State)->Res<T>)->Res<T> {within(install,std::time::Duration::ZERO,f)}
/// `try_with` for commands that cannot retry on a later tick: waits at most `wait` (#1415).
pub(crate) fn within<T>(install:&Path,wait:std::time::Duration,f:impl FnOnce(&mut State)->Res<T>)->Res<T> {
    let _lock=acquire(install,wait)?;
    let mut state=read(install)?;let before=state.clone();let result=f(&mut state);
    if state!=before {save(install,&state)?;}result
}
pub(crate) fn try_locked<T>(install:&Path,f:impl FnOnce()->Res<T>)->Res<T> {
    let _lock=acquire(install,std::time::Duration::ZERO)?;
    f()
}

fn token(prefix: &str) -> String {
    let mut b = [0u8; 12];
    SystemRandom::new().fill(&mut b).expect("system random");
    format!("{prefix}-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

pub(crate) fn id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn host_authority(install:&Path)->bool {
    ["host.json","host.json.pending"].iter().any(|name|match fs::symlink_metadata(install.join(name)) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>false,
        _=>true,
    })
}

/// The one remote profile a host-policy install takes: this root's host.json overlay (#1282).
const OVERLAY: &str = "taskq.project-limits/1";

/// Observe the actual saved authority; an unreadable host policy never falls back to legacy settings. On a host
/// install: the host capacity, this root's effective `project_limits` and its project revision.
fn policy(install:&Path, root: &Path) -> Res<(Value, Option<String>)> {
    if host_authority(install) {
        let host=crate::host::Policy::read(install)?;
        let project=crate::load(root)?.1;
        let mut v=host.cap.json();
        v["project_limits"]=json!(host.project_limits(root,&project)?);
        return Ok((v,Some(host.project_revision(root,&project)?)));
    }
    let (cfg, v) = crate::load(root)?;
    Ok((crate::service::capacity(&v, &cfg.me)?.json(),None))
}

fn registered(install: &Path, root: &str) -> Res<bool> {
    Ok(crate::host::project_paths(install)?.iter().any(|r| r == Path::new(root)))
}

/// Retire old scopes before dropping roots. A failed registry write remains restrictive, never revives an ID.
pub fn unregister(install: &Path, roots: &[PathBuf], write: impl FnOnce() -> Res<()>) -> Res<()> {
    let _lock=lock(install)?;
    let mut s=read(install)?;
    let before=s.projects.len();
    s.projects.retain(|_, p| !roots.contains(&PathBuf::from(&p.root)));
    let retired=s.projects.len()!=before;
    if retired {save(install,&s)?;}
    write().map_err(|e|if retired {format!("resource scope retired; registry write failed: {e}")} else {e})
}

/// Registered roots get an opaque ID once; an unregistered root loses it for good. A capacity that differs from the
/// recorded one, including a hand edit, gets a new revision.
fn sync(s: &mut State, install: &Path, now: u64) -> Res<()> {
    let roots: Vec<String> = crate::host::project_paths(install)?.into_iter().map(|r| r.display().to_string()).collect();
    s.projects.retain(|_, p| roots.contains(&p.root));
    for r in roots {
        if !s.projects.values().any(|p| p.root == r) {
            s.projects.insert(token("p"), Project { root: r, revision: String::new(), policy: Value::Null, since: now });
        }
    }
    for p in s.projects.values_mut() {
        let (now_policy,revision) = policy(install,Path::new(&p.root)).unwrap_or((Value::Null,None));
        if p.revision.is_empty() || now_policy != p.policy || revision.as_ref().is_some_and(|r|r!=&p.revision) {
            *p = Project { root: p.root.clone(), revision: revision.unwrap_or_else(||token("r")), policy: now_policy, since: now };
        }
    }
    Ok(())
}

fn receipt(target: &Value, op: &str, state: &str, reason: Option<&str>, saved: Option<&str>, effective: Option<&str>, at: Option<u64>) -> Value {
    json!({"schema": SCHEMA, "op_id": op, "target": target, "state": state, "reason": reason,
        "saved_revision": saved, "effective_revision": effective, "observed_at": at})
}

pub(crate) fn keys(v: &Value, want: &[&str]) -> bool {
    v.as_object().is_some_and(|o| o.len() == want.len() && want.iter().all(|k| o.contains_key(*k)))
}

fn target_ok(t: &Value) -> bool {
    keys(t, &["installation_id", "generation", "project_id", "runtime_id"])
        && t["installation_id"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        && t["generation"].as_u64().is_some_and(|g| g > 0)
        && (t["project_id"].is_null() || t["project_id"].as_str().is_some_and(id))
        && (t["runtime_id"].is_null() || t["runtime_id"].as_str().is_some_and(|r| RUNTIMES.contains(&r)))
        && (t["runtime_id"].is_null() || !t["project_id"].is_null())
}

pub(crate) fn count(v: &Value) -> bool {
    v.as_u64().is_some_and(|n| n <= u32::MAX as u64)
}

/// The typed submit rules of `taskq.resources/1`: exact keys and types, ID grammar, lifetime and per-schema fields.
pub fn typed(v: &Value) -> Result<(), &'static str> {
    let ids = ["op_id", "actor_id", "grant_id", "expected_revision"];
    if !keys(v, &["schema", "op_id", "actor_id", "grant_id", "target", "policy_schema", "expected_revision", "issued_at", "expires_at", "changes"])
        || v["schema"] != SCHEMA || !ids.iter().all(|k| v[k].as_str().is_some_and(id)) || !target_ok(&v["target"])
        || !v["policy_schema"].is_string()
    {
        return Err("invalid");
    }
    let (Some(issued), Some(expires)) = (v["issued_at"].as_u64(), v["expires_at"].as_u64()) else { return Err("invalid") };
    let Some(changes) = v["changes"].as_object().filter(|c| !c.is_empty()) else { return Err("invalid") };
    if expires <= issued || expires - issued > 300 {
        return Err("invalid");
    }
    let (project, runtime) = (!v["target"]["project_id"].is_null(), !v["target"]["runtime_id"].is_null());
    let auto = match v["policy_schema"].as_str() {
        Some("taskq.capacity/1") => false,
        Some("taskq.admission/1") => true,
        // exactly one runtime of one project; null removes the overlay entry
        Some(OVERLAY) => return if runtime && changes.len() == 1 && changes.get("runtime_limit").is_some_and(|x| x.is_null() || count(x)) { Ok(()) } else { Err("invalid") },
        _ => return Err("unsupported"),
    };
    for (k, x) in changes {
        let valid = match k.as_str() {
            "runtime_limit" => runtime && (count(x) || (auto && x.is_null())),
            "maximum" => auto && !runtime && (x.is_null() || count(x)),
            "mode" => auto && !runtime && (x == "auto" || x == "fixed"),
            "load_gate" => project && !runtime && keys(x, &["enabled", "cpu_max", "memory_min"]) && x["enabled"].is_boolean()
                && x["cpu_max"].as_u64().is_some_and(|n| (10..=100).contains(&n)) && x["memory_min"].as_u64().is_some_and(|n| (1..=95).contains(&n)),
            _ => false,
        };
        if !valid {
            return Err("invalid");
        }
    }
    if changes.get("mode") == Some(&json!("auto")) && changes.get("load_gate").is_some_and(|g| g["enabled"] == false) {
        return Err("invalid");
    }
    Ok(())
}

/// The connection this exchange speaks for: `(installation, generation)`.
type Conn<'a> = (&'a str, u64);

/// The host grant check, against the state as it is now (enqueue and again immediately before the write).
fn authorize(s: &State, v: &Value, conn: Conn, now: u64) -> Result<String, &'static str> {
    let t = &v["target"];
    let g = s.grants.get(v["grant_id"].as_str().unwrap_or_default()).filter(|g| g.actor_id == v["actor_id"]).ok_or("unauthorized")?;
    if g.revoked || now >= g.expires_at {
        return Err("revoked");
    }
    let (inst, generation) = conn;
    if t["installation_id"] != inst || t["generation"] != generation || g.installation_id != inst || g.generation != generation {
        return Err("scope");
    }
    let Some(pid) = t["project_id"].as_str() else {
        // The legacy remote profiles have no qualified host mutation authority.
        return if g.host_policy { Err("unsupported") } else { Err("scope") };
    };
    if !s.projects.contains_key(pid) || !g.project_ids.iter().any(|x| x == pid) {
        return Err("scope");
    }
    if t["runtime_id"].as_str().is_some_and(|rt| !g.runtime_ids.iter().any(|x| x == rt)) {
        return Err("scope");
    }
    if v["policy_schema"] == OVERLAY && !g.project_limits {
        return Err("scope");
    }
    Ok(pid.to_string())
}

/// The edit of a typed change set; `v["limits"]`: the project-wide limits a removed override would fall back to.
fn edit(changes: &Value, runtime: Option<&str>, project_limits: &Value) -> Result<CapEdit, &'static str> {
    let mut e = CapEdit::default();
    for (k, x) in changes.as_object().into_iter().flatten() {
        match k.as_str() {
            "runtime_limit" => {
                let rt = runtime.ok_or("invalid")?.to_string();
                match x.as_u64() {
                    Some(n) => { e.limits.insert(rt, n as u32); }
                    // `null` removes this host's override; a project-wide limit would stay, so that is unsupported here
                    None if project_limits[&rt].is_null() => { e.unset.insert(rt); }
                    None => return Err("unsupported"),
                }
            }
            "mode" => e.mode = Some(x == "auto"),
            "maximum" => e.max = Some(x.as_u64().map(|n| n as u32)),
            "load_gate" => {
                e.gate = x["enabled"].as_bool();
                e.cpu_max = x["cpu_max"].as_u64().map(|n| n as u32);
                e.memory_min = x["memory_min"].as_u64().map(|n| n as u32);
            }
            _ => return Err("invalid"),
        }
    }
    Ok(e)
}

/// What `decide` re-checks immediately before the write: the time now (never earlier than the checks before) and
/// whether the connection the operation came over is still the active one.
struct Fence<'a> {
    clock: &'a dyn Fn() -> u64,
    live: &'a dyn Fn() -> bool,
}

/// One delivered submit to its receipt; `None` keeps the original binding of a reused op ID and sends nothing.
fn decide(install: &Path, s: &mut State, v: &Value, conn: Conn, clock: bool, now: u64, fence: &Fence) -> Res<Option<String>> {
    let (Some(op), true) = (v["op_id"].as_str().filter(|o| id(o)), target_ok(&v["target"])) else {
        return Ok(Some("resources: dropped an operation without a valid ID or target".into()));
    };
    let op = op.to_string();
    if let Some(o) = s.ops.get_mut(&op) {
        if local(o) {return Ok(Some(format!("resources: {op} belongs to a local command; ignored")));}
        // the host deduplicates by op ID: the same submit (or the cancellation that came first) repeats its receipt
        let same = o.bound == *v || (o.bound.get("changes").is_none() && ["schema", "actor_id", "grant_id", "target"].iter().all(|k| o.bound[k] == v[k]));
        if !same {
            return Ok(Some(format!("resources: {op} reused with another payload; original kept")));
        }
        o.sent = false;
        return Ok(None);
    }
    let t = v["target"].clone();
    let expires = v["expires_at"].as_u64().unwrap_or(now);
    let record = |s: &mut State, state: &str, reason: Option<&str>| {
        s.ops.insert(op.clone(), Op { bound: v.clone(), receipt: receipt(&t, &op, state, reason, None, None, None), expires_at: expires, saved_at: None, writing: false, sent: false });
        Some(format!("resources: {op} {}", reason.unwrap_or(state)))
    };
    if s.ops.len() >= MAX_OPS {
        // never evict a retained binding: the new ID is not recorded and expires undelivered at DashQ
        return Ok(Some(format!("resources: {MAX_OPS} operations retained; {op} not taken")));
    }
    if let Err(r) = typed(v) {
        return Ok(record(s, "rejected", Some(r)));
    }
    let pid = match authorize(s, v, conn, now) {
        Err(r) => return Ok(record(s, "rejected", Some(r))),
        Ok(p) => p,
    };
    if !clock || v["issued_at"].as_u64().is_some_and(|i| i > now + AHEAD) {
        return Ok(record(s, "rejected", Some("clock_unknown")));
    }
    if now >= expires {
        return Ok(record(s, "expired", None));
    }
    let p = s.projects[&pid].clone();
    if v["expected_revision"] != p.revision.as_str() {
        return Ok(record(s, "rejected", Some("conflict")));
    }
    // The legacy wire profiles own legacy project policy, never fallback authority for host.json; the overlay
    // exists only in host.json.
    let overlay = v["policy_schema"] == OVERLAY;
    if host_authority(install) != overlay {return Ok(record(s,"rejected",Some("unsupported")));}
    let root = PathBuf::from(&p.root);
    if overlay {
        record(s, "unknown", None);
        s.ops.get_mut(&op).unwrap().writing = true;
        save(install, s)?;
        s.ops.get_mut(&op).unwrap().writing = false;
        let now = now.max((fence.clock)());
        if !(fence.live)() || !registered(install, &p.root)? {
            return Ok(record(s, "rejected", Some("scope")));
        }
        if let Err(r) = authorize(s, v, conn, now) {
            return Ok(record(s, "rejected", Some(r)));
        }
        if now >= expires {
            return Ok(record(s, "expired", None));
        }
        let limit = v["changes"]["runtime_limit"].as_u64().map(|n| n as u32);
        let written = match crate::host::write_project_limit(install, &root, t["runtime_id"].as_str().unwrap_or_default(), limit, &op, &p.revision) {
            Ok(Ok(written)) => written,
            Ok(Err(r)) => return Ok(record(s, "rejected", Some(r))),
            Err(x) => return Ok(Some(format!("resources: {op} unknown: {x}"))),
        };
        let wake = crate::platform::write_atomic(&install.join("poke"), "");
        let (Ok((saved, Some(rev))), Ok(want)) = (policy(install, &root), crate::load(&root).and_then(|(_,v)|written.project_revision(&root,&v))) else {
            return Ok(Some(format!("resources: {op} unknown: readback failed")));
        };
        if rev != want {
            return Ok(Some(format!("resources: {op} unknown: readback differs")));
        }
        let o = s.ops.get_mut(&op).unwrap();
        o.receipt = receipt(&t, &op, "saved", None, Some(&rev), None, None);
        o.saved_at = Some(now);
        s.projects.insert(pid, Project { revision: rev, policy: saved, since: now, ..p });
        return Ok(Some(match wake { Ok(()) => format!("resources: {op} saved"), Err(error) => format!("resources: {op} saved; host notification: {error}") }));
    }
    let Ok((cfg, cv)) = crate::load(&root) else { return Ok(record(s, "rejected", Some("unsupported"))) };
    let Ok(cur) = crate::service::capacity(&cv, &cfg.me) else { return Ok(record(s, "rejected", Some("unsupported"))) };
    if cur.json() != p.policy {
        return Ok(record(s, "rejected", Some("conflict")));
    }
    // taskq.capacity/1 is the legacy Fixed authority
    if v["policy_schema"] == "taskq.capacity/1" && cur.auto {
        return Ok(record(s, "rejected", Some("unsupported")));
    }
    let e = match edit(&v["changes"], v["target"]["runtime_id"].as_str(), &cv["limits"]) {
        Ok(e) => e,
        Err(r) => return Ok(record(s, "rejected", Some(r))),
    };
    let path = root.join("taskq.json");
    let Ok(text) = fs::read_to_string(&path) else { return Ok(record(s, "rejected", Some("conflict"))) };
    let Ok(out) = crate::edit_capacity(&text, &cfg.me, &cfg.me, &e, Some(&cur.json())) else { return Ok(record(s, "rejected", Some("invalid"))) };
    let want = serde_json::from_str::<Value>(&out).ok().and_then(|x| crate::service::capacity(&x, &cfg.me).ok()).map(|c| c.json());
    // the marker is durable before the write: a crash from here on reads unknown and never writes again
    record(s, "unknown", None);
    s.ops.get_mut(&op).unwrap().writing = true;
    save(install, s)?;
    s.ops.get_mut(&op).unwrap().writing = false;
    // the marker save and earlier waits took time: deadline, grant and connection again on a fresh clock
    let now = now.max((fence.clock)());
    if !(fence.live)() || !registered(install, &p.root)? {
        return Ok(record(s, "rejected", Some("scope")));
    }
    if let Err(r) = authorize(s, v, conn, now) {
        return Ok(record(s, "rejected", Some(r)));
    }
    if now >= expires {
        return Ok(record(s, "expired", None));
    }
    if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        return Ok(record(s, "rejected", Some("conflict")));
    }
    if let Err(x) = crate::platform::write_atomic(&path, &out) {
        return Ok(Some(format!("resources: {op} unknown: {x}")));
    }
    let wake = crate::poke(&root);
    let saved = policy(install,&root).ok().map(|x| x.0);
    let Some(saved) = saved.filter(|x| Some(x) == want.as_ref()) else {
        return Ok(Some(format!("resources: {op} unknown: readback differs")));
    };
    let rev = token("r");
    let o = s.ops.get_mut(&op).unwrap();
    o.receipt = receipt(&t, &op, "saved", None, Some(&rev), None, None);
    o.saved_at = Some(now);
    s.projects.insert(pid, Project { revision: rev, policy: saved, since: now, ..p });
    Ok(Some(match wake { Ok(()) => format!("resources: {op} saved"), Err(error) => format!("resources: {op} saved; {error}") }))
}

/// A cancellation: a recorded operation repeats its receipt (a completed write is never rolled back); an unknown
/// one is recorded cancelled, so a later delivery of it cannot write.
fn cancel(s: &mut State, c: &Value, now: u64) -> Option<String> {
    let valid = keys(c, &["schema", "op_id", "actor_id", "grant_id", "target"]) && c["schema"] == SCHEMA
        && ["op_id", "actor_id", "grant_id"].iter().all(|k| c[k].as_str().is_some_and(id)) && target_ok(&c["target"]);
    let op = c["op_id"].as_str().filter(|_| valid)?.to_string();
    let full = s.ops.len() >= MAX_OPS;
    if s.ops.get(&op).is_some_and(|o|local(o)) {return Some(format!("resources: cancellation of local command {op} ignored"));}
    match s.ops.get_mut(&op) {
        Some(o) if ["schema", "actor_id", "grant_id", "target"].iter().all(|k| o.bound[k] == c[k]) => o.sent = false,
        Some(_) => return Some(format!("resources: cancellation of {op} does not match it; ignored")),
        None if !full => {
            s.ops.insert(op.clone(), Op { bound: c.clone(), receipt: receipt(&c["target"], &op, "cancelled", None, None, None, None), expires_at: now + 300, saved_at: None, writing: false, sent: false });
        }
        None => return None,
    }
    Some(format!("resources: {op} cancelled"))
}

/// Saved operations become applied on a fresh service readback of the saved revision (§ 4 `status.json`).
fn readback(s: &mut State, install:&Path, now: u64) {
    for o in s.ops.values_mut() {
        if local(o) {continue;}
        let Some(at) = o.saved_at.filter(|t| o.receipt["state"] == "saved" && now < t + RECONCILE) else { continue };
        let overlay = o.bound["policy_schema"] == OVERLAY;
        if host_authority(install) && !overlay {
            o.receipt["state"]=json!("unknown");
            o.receipt["reason"]=json!("unsupported");
            o.sent=false;
            continue;
        }
        let Some(p) = o.receipt["target"]["project_id"].as_str().and_then(|pid| s.projects.get(pid)) else { continue };
        if o.receipt["saved_revision"] != p.revision.as_str() {
            continue;
        }
        let root = PathBuf::from(&p.root);
        let st: Value = fs::read_to_string(crate::dot(&root).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let me = if overlay { crate::host::Policy::read(install).map(|x| x.me) } else { crate::load(&root).map(|x| x.0.me) }.unwrap_or_default();
        let seen = st["at"].as_u64().filter(|t| *t >= at && *t <= now && now - t <= FRESH && st["host"] == me.as_str());
        // the overlay: the service ran the exact saved host revision for this root, with the saved project limits
        let same = if overlay { st["capacity"]["project_revision"] == p.revision.as_str() && st["capacity"]["project_limits"] == p.policy["project_limits"] }
            else { ["limits", "load_gate", "admission"].iter().all(|k| st["capacity"][k] == p.policy[k]) };
        if let (Some(seen), true) = (seen, same) {
            o.receipt["state"] = json!("applied");
            o.receipt["effective_revision"] = json!(p.revision);
            o.receipt["observed_at"] = json!(seen);
            o.sent = false;
        }
    }
}

/// Before an exchange: interrupted writes read unknown, projects and revisions are current, retention is applied.
pub(crate) fn prepare(s: &mut State, install: &Path, now: u64) -> Res<()> {
    sync(s, install, now)?;
    for o in s.ops.values_mut().filter(|o| o.writing) {
        o.writing = false;
        o.receipt["state"] = json!("unknown");
        o.sent = false;
    }
    s.ops.retain(|_,o|now<o.expires_at.saturating_add(RETAIN)
        ||o.bound.get("local_slot").is_some()
        ||((o.bound.get("local_restart").is_some()||(o.bound.get("local_stop").is_some()||o.bound.get("local_uninstall").is_some()))&&o.receipt["state"]!="observed")
        ||(o.bound.get("local_install").is_some()&&!matches!(o.receipt["state"].as_str(),Some("observed"|"failed"))));
    readback(s, install, now);
    Ok(())
}

/// Whether there is anything to exchange: an active host grant for this connection, or a receipt DashQ has not taken.
fn wanted(s: &State, conn: Conn, now: u64) -> bool {
    s.grants.values().any(|g| !g.revoked && now < g.expires_at && g.installation_id == conn.0 && g.generation == conn.1)
        || s.ops.values().any(|o| !o.sent && !local(o))
}

/// One exchange with DashQ over the active connection: up to 8 receipts out, up to 8 operations and cancellations
/// in. Returns log lines and whether to come back after the minimum spacing rather than the idle interval.
pub fn exchange(install: &Path, active: &Value, w: &mut dyn Wire, now: u64) -> Res<(Vec<String>, bool)> {
    let (Some(inst), Some(generation), Some(dest), Some(cred)) = (active["installation"].as_str(), active["generation"].as_u64(), active["destination"].as_str(), active["credential"].as_str()) else {
        return Err("connection has no installation, generation or credential".into());
    };
    let conn = (inst, generation);
    // no store: no host grant was ever made, nothing to exchange or to create
    if !dir(install).join(FILE).exists() {
        read(install)?;
        return Ok((vec![], false));
    }
    let sent: Vec<Value> = with(install, |s| {
        prepare(s, install, now)?;
        Ok(if wanted(s, conn, now) { s.ops.values().filter(|o| !o.sent && !local(o)).take(ITEMS).map(|o| o.receipt.clone()).collect() } else { vec![json!(null)] })
    })?;
    if sent.first().is_some_and(Value::is_null) {
        return Ok((vec![], false));
    }
    let body = json!({"schema": SCHEMA, "installation_id": inst, "generation": generation, "receipts": sent});
    let (code, r) = w.post(dest, EXCHANGE, Some(cred), &body)?;
    if code == 429 {
        return Ok((vec![], true));
    }
    if code != 200 {
        return Err(format!("exchange refused: http {code} {}", r["reason"].as_str().filter(|x| id(x)).unwrap_or("unknown")));
    }
    let size = serde_json::to_vec(&r).map_or(usize::MAX, |b| b.len());
    let list = |k: &str| r[k].as_array().filter(|a| a.len() <= ITEMS).cloned();
    let (Some(ops), Some(cancels), Some(server)) = (list("operations"), list("cancellations"), r["server_at"].as_u64()) else {
        return Err("exchange reply invalid".into());
    };
    if size > BODY || !keys(&r, &["schema", "server_at", "operations", "cancellations"]) || r["schema"] != SCHEMA {
        return Err("exchange reply invalid".into());
    }
    let clock = now.abs_diff(server) <= AHEAD;
    // the operations belong to this connection: a disconnect, revoke or new generation during the POST drops them
    let live = || enrollment::active(install).ok().flatten().is_some_and(|a| ["installation", "generation", "destination", "credential"].iter().all(|k| a[k] == active[k]));
    let w = &*w;
    let fence = Fence { clock: &|| w.now(), live: &live };
    with(install, |s| {
        // never at or after the deadline on either clock, read after the POST and the lock wait
        let now = now.max(server).max(w.now());
        if !live() {
            return Ok((vec![format!("resources: connection changed during the exchange; {} operations not taken", ops.len())], false));
        }
        for rc in &sent {
            if let Some(o) = s.ops.get_mut(rc["op_id"].as_str().unwrap_or_default()).filter(|o| o.receipt == *rc) {
                o.sent = true;
            }
        }
        let mut log: Vec<String> = cancels.iter().filter_map(|c| cancel(s, c, now)).collect();
        for v in &ops {
            log.extend(decide(install, s, v, conn, clock, now, &fence)?);
        }
        if !clock && !ops.is_empty() {
            log.push(format!("resources: host clock differs from DashQ's by {} s", now.abs_diff(server)));
        }
        let more = !ops.is_empty() || !cancels.is_empty() || s.ops.values().any(|o| !o.sent && !local(o));
        Ok((log, more))
    })
}

/// The service's exchange: one at a time on its own thread, never blocking a pass, only from the service whose board
/// the connection approved (the host's one DashQ speaker).
#[derive(Default)]
pub struct Exchanger {
    job: Option<JoinHandle<(u64, Vec<String>)>>,
    wait_until: u64,
}

impl Exchanger {
    pub fn tick(&mut self, install: &Path, snap: Option<&Snapshot>, now: u64) -> Vec<String> {
        let mut log = vec![];
        if self.job.as_ref().is_some_and(|j| j.is_finished()) {
            let (wait, l) = self.job.take().unwrap().join().unwrap_or((60, vec!["resources: exchange failed".into()]));
            self.wait_until = now + wait;
            log = l;
        }
        if self.job.is_some() || now < self.wait_until {
            return log;
        }
        let Some(active) = enrollment::active(install).ok().flatten().filter(|a| enrollment::speaks(a, snap)) else {
            self.wait_until = now + IDLE;
            return log;
        };
        let install = install.to_path_buf();
        self.job = Some(std::thread::spawn(move || {
            let mut w = Https { browser: false };
            let now = w.now();
            match exchange(&install, &active, &mut w, now) {
                Ok((log, more)) => (if more { SPACING } else { IDLE }, log),
                Err(e) => (60, vec![format!("resources: {e}")]),
            }
        }));
        log
    }
}

/// The scope a host grant names: `project` entries are project IDs or registered names.
pub struct Scope {
    pub host_policy: bool,
    pub project_limits: bool,
    pub projects: Vec<String>,
    pub runtimes: Vec<String>,
    pub hours: u64,
}

/// `service resources --grant`: add or replace one host grant for the active connection.
pub fn grant(install: &Path, grant_id: &str, actor: &str, scope: &Scope, by: &str, now: u64) -> Res<Grant> {
    if !id(grant_id) || !id(actor) {
        return Err("grant and actor IDs: 1-64 of A-Z a-z 0-9 _ -".into());
    }
    if !(1..=MAX_HOURS).contains(&scope.hours) {
        return Err(format!("--hours: 1..{MAX_HOURS}"));
    }
    if let Some(rt) = scope.runtimes.iter().find(|r| !RUNTIMES.contains(&r.as_str())) {
        return Err(format!("unknown runtime {rt}"));
    }
    if scope.projects.is_empty() && !scope.host_policy {
        return Err("name a --project or --host-policy".into());
    }
    let a = enrollment::active(install)?.ok_or("no active DashQ connection: run taskq enroll first")?;
    let (Some(inst), Some(generation)) = (a["installation"].as_str(), a["generation"].as_u64()) else {
        return Err("connection has no installation or generation".into());
    };
    let names = names(install);
    with(install, |s| {
        sync(s, install, now)?;
        let mut ids = vec![];
        for x in &scope.projects {
            let found: Vec<&String> = s.projects.iter().filter(|(k, p)| *k == x || names.get(&p.root) == Some(x)).map(|(k, _)| k).collect();
            match found[..] {
                [one] => ids.push(one.clone()),
                [] => return Err(format!("unknown project {x}: see taskq service resources")),
                _ => return Err(format!("project name {x} is ambiguous: give its ID")),
            }
        }
        ids.sort();
        ids.dedup();
        let mut runtimes = scope.runtimes.clone();
        runtimes.sort();
        runtimes.dedup();
        let g = Grant { actor_id: actor.into(), installation_id: inst.into(), generation, project_ids: ids, runtime_ids: runtimes,
            host_policy: scope.host_policy, project_limits: scope.project_limits, expires_at: now + scope.hours * 3600, revoked: false, by: by.into(), at: now };
        s.grants.insert(grant_id.into(), g.clone());
        Ok(g)
    })
}

/// Registered roots by display path, named like status.json (else the folder name).
fn names(install: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(install.join("projects")).unwrap_or_default().lines().filter(|l| !l.is_empty()).map(|r| {
        let st: Value = fs::read_to_string(crate::dot(Path::new(r)).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        (r.to_string(), st["project"].as_str().map(String::from).unwrap_or_else(|| crate::project_name(Path::new(r))))
    }).collect()
}

/// `service resources --revoke`: the grant stays, revoked, so its ID is never silently reissued.
pub fn revoke(install: &Path, grant_id: &str) -> Res<()> {
    with(install, |s| match s.grants.get_mut(grant_id) {
        Some(g) => { g.revoked = true; Ok(()) }
        None => Err(format!("no grant {grant_id}")),
    })
}

/// `service resources`: projects with IDs and revisions, grants and operation receipts.
pub fn report(install: &Path, now: u64) -> Res<Vec<String>> {
    let names = names(install);
    let conn = enrollment::active(install).ok().flatten();
    with(install, |s| {
        prepare(s, install, now)?;
        let mut out = vec![match &conn {
            Some(a) => format!("Connection: {} generation {}", a["destination"].as_str().unwrap_or("?"), a["generation"]),
            None => "Connection: none (grants need an active DashQ connection)".into(),
        }];
        if host_authority(install) {
            out.push(match crate::host::Policy::read(install).and_then(|p|p.revision()) {
                Ok(revision)=>format!("Host policy: revision {revision}; local edits only; remote writes: {OVERLAY} under a --project-limits grant"),
                Err(e)=>format!("Host policy: unknown ({e}); remote writes refused"),
            });
        }
        for (k, p) in &s.projects {
            let name = names.get(&p.root).map_or("?", String::as_str);
            out.push(format!("Project {name}: {k} revision {}{}", p.revision, if p.policy.is_null() { " (config unreadable)" } else { "" }));
        }
        for (k, g) in &s.grants {
            let state = if g.revoked { "revoked".into() } else if now >= g.expires_at { "expired".into() } else { format!("expires {}", crate::board::stamp(g.expires_at as i64)) };
            let scope = [g.host_policy.then(|| "host".to_string()), g.project_limits.then(|| "project limits".to_string()), (!g.project_ids.is_empty()).then(|| format!("projects {}", g.project_ids.join(" "))),
                (!g.runtime_ids.is_empty()).then(|| format!("runtimes {}", g.runtime_ids.join(" ")))].into_iter().flatten().collect::<Vec<_>>().join(", ");
            out.push(format!("Grant {k}: actor {} generation {}, {scope}; {state}", g.actor_id, g.generation));
        }
        let mut states: BTreeMap<String, usize> = BTreeMap::new();
        for o in s.ops.values() {
            *states.entry(o.receipt["reason"].as_str().or(o.receipt["state"].as_str()).unwrap_or("?").into()).or_default() += 1;
        }
        out.push(if states.is_empty() { "Operations: none".into() } else { format!("Operations: {}", states.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ")) });
        Ok(out)
    })
}

/// `service resources --json`: the allowlisted `taskq.project-limits/1` projection of every registered project on a
/// host-policy install (shared-resource-controls.md#project-overlay). Current is the saved overlay (null: no
/// override) at the project revision; effective is what a fresh status of this host ran, else unknown (null).
/// `observed_at` is the oldest status time read, so a consumer ages the readback, never the print.
/// `ponytail:` one observation time for all projects; per-project groups if one idle project hides the rest.
/// `wait: false` (the service's enrollment sender) never waits behind a policy writer; it retries on a later pass.
pub fn projection(install: &Path, now: u64, wait: bool) -> Res<Value> {
    let host = crate::host::Policy::read(install)?;
    let conn = enrollment::active(install).ok().flatten();
    let (inst, generation) = conn.as_ref().map_or((Value::Null, Value::Null), |a| (a["installation"].clone(), a["generation"].clone()));
    let mut targets = vec![];
    let mut observed: Option<Option<u64>> = None;
    let body = |s: &mut State| {
        prepare(s, install, now)?;
        for (pid, p) in &s.projects {
            let root = PathBuf::from(&p.root);
            let st: Value = fs::read_to_string(crate::dot(&root).join("status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let at = st["at"].as_u64().filter(|_| st["host"] == host.me.as_str());
            observed = Some(match (observed, at) { (Some(None), _) | (_, None) => None, (Some(Some(o)), Some(a)) => Some(o.min(a)), (None, Some(a)) => Some(a) });
            let fresh = at.is_some_and(|t| t <= now && now - t <= FRESH);
            let ran = |k: &str| if fresh { st["capacity"][k].clone() } else { Value::Null };
            let overlay = host.overlay(&root).map(|(_, e)| e["limits"].clone()).unwrap_or_default();
            for rt in crate::service::RUNTIMES {
                let t = json!({"installation_id": inst, "generation": generation, "project_id": pid, "runtime_id": rt});
                // an unreadable taskq.json is a known config blocker with nothing effective; status reasons keep their knowledge,
                // and a stale or missing status keeps this host's last-known ones and adds an unknown sample
                let mut bs = if p.policy.is_null() { vec![json!({"code": "config", "scope": t, "knowledge": "known"})] } else { vec![] };
                if at.is_some() { bs.extend(blockers(&st["capacity"], |_| t.clone()).as_array().into_iter().flatten().cloned()); }
                if !fresh { bs.push(json!({"code": "sample", "scope": t, "knowledge": "unknown"})); }
                let completeness = if !fresh || p.policy.is_null() { "unknown" } else if bs.iter().any(|b| b["knowledge"] == "unknown") { "partial" } else { "complete" };
                let known = fresh && !p.policy.is_null();
                targets.push(json!({"target": t,
                    "service_version": crate::platform::VERSION,
                    "supported": [{"policy_schema": OVERLAY, "modes": [], "fields": ["runtime_limit"], "scopes": ["runtime"]}],
                    "current_revision": p.revision, "effective_revision": if known { ran("project_revision") } else { Value::Null },
                    "current_policy": {"runtime_limit": overlay[rt]}, "effective_policy": {"runtime_limit": if known { ran("project_limits")[rt].clone() } else { Value::Null }},
                    "runtime_usage": [], "sample": null, "blockers": bs, "completeness": completeness}));
            }
        }
        Ok(())
    };
    if wait { with(install, body) } else { within(install, WAIT, body) }?;
    Ok(json!({"schema": SCHEMA, "observed_at": observed.flatten(), "targets": targets}))
}

/// `taskq.resources/1` blockers from a status `capacity`'s reasons; `scope(host_scope)` names the target each one holds.
pub fn blockers(capacity: &Value, scope: impl Fn(bool) -> Value) -> Value {
    const CODES: [&str; 18] = ["cpu", "memory", "pressure", "sample", "config", "cooldown", "spacing", "max", "version", "host", "runtime_disabled", "pause", "focus", "dependency", "routing", "access", "provider", "unknown"];
    json!(capacity["reasons"].as_array().into_iter().flatten().map(|r| {
        let code = match r["code"].as_str().unwrap_or("") { "hold" => "host", c if CODES.contains(&c) => c, _ => "unknown" };
        let host_scope = ["host", "max", "spacing", "version", "cpu", "memory", "pressure", "sample", "config"].contains(&code);
        json!({"code": code, "scope": scope(host_scope), "knowledge": if code == "sample" || code == "unknown" || (code == "host" && capacity["gate"] == "unknown") { "unknown" } else { "known" }})
    }).collect::<Vec<_>>())
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
