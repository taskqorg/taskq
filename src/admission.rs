//! The host admission authority (taskq.md § 3): `<install>/admission.json` under the exclusive lock
//! `<install>/admission.lock`. Every service of the host writes its reservations there; an Auto launch is granted only
//! under the lock, so two projects cannot spend the same headroom. A cache (R1): each service rewrites its own entry.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The oldest service version that writes reservations here; an older live service refuses Auto admission.
pub const MIN_SERVICE: &str = "4.2.0-dev";
/// An entry not rewritten for this many seconds belongs to a stopped service: its reservations are released.
pub const STALE: i64 = 120;
/// One Auto launch per this many seconds host-wide, the next after a newer gate sample.
pub const SPACING: i64 = 30;

/// `(task, attempt, runtime)`; attempt 0 before the claim is written.
pub type Res = (u64, u64, String);

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct Entry {
    pub version: String,
    pub at: i64,
    #[serde(default)]
    pub held: Vec<Res>,
    #[serde(default)]
    pub requested: Vec<Res>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Host {
    #[serde(default)]
    pub launched: i64,
    #[serde(default)]
    pub services: BTreeMap<String, Entry>,
}

/// An Auto launch request: the host ceiling, the wall time of this service's newest gate sample and the fresh
/// services of `<install>/projects` below [`MIN_SERVICE`] (`project version`).
pub struct Want<'a> {
    pub max: Option<u32>,
    pub sample_at: Option<i64>,
    pub old: &'a [String],
    pub res: Res,
}

/// Held reservations of the whole host.
pub fn used(h: &Host) -> u32 {
    h.services.values().map(|e| e.held.len() as u32).sum()
}

/// One decision on the host view (pure): release stale entries, replace `root`'s entry with `mine`, then grant `want`
/// into `held` or refuse it with `(code, reason)` and keep it `requested`.
pub fn decide(h: &mut Host, root: &str, mut mine: Entry, want: Option<Want>, now: i64) -> Result<(), (&'static str, String)> {
    h.services.retain(|r, e| r != root && now - e.at <= STALE);
    let out = match want {
        None => Ok(()),
        Some(w) => {
            let r = if let Some(o) = w.old.first() {
                Err(("version", format!("service {o} is older than {MIN_SERVICE}")))
            } else if w.max.is_some_and(|m| used(h) + mine.held.len() as u32 >= m) {
                Err(("max", format!("host at its maximum of {} sessions", w.max.unwrap_or(0))))
            } else if now - h.launched < SPACING {
                Err(("spacing", format!("host launched {} ago", crate::time::age(now - h.launched))))
            } else if w.sample_at.is_none_or(|s| s <= h.launched) {
                Err(("spacing", "no load sample since the last host launch".into()))
            } else {
                Ok(())
            };
            match r {
                Ok(()) => {
                    mine.held.push(w.res);
                    h.launched = now;
                }
                Err(_) => mine.requested.push(w.res),
            }
            r
        }
    };
    mine.at = now;
    h.services.insert(root.into(), mine);
    out
}

/// Fresh services of `<install>/projects` (status.json at most [`STALE`] s old) below [`MIN_SERVICE`].
pub fn old_services(install: &Path) -> Vec<String> {
    let t = crate::platform::tool(install);
    fs::read_to_string(install.join("projects")).unwrap_or_default().lines().filter(|l| !l.is_empty()).filter_map(|r| {
        let v = coreq::status::read(&t, Path::new(r), STALE as u64)?;
        let ver = v["version"].as_str().unwrap_or("unknown").to_string();
        coreq::update::newer(MIN_SERVICE, &ver).then(|| format!("{} {ver}", crate::paths::project_name(Path::new(r))))
    }).collect()
}

/// The emergency hold (§ 3): `<install>/hold` refuses every launch on the host until `service release` removes it.
/// `None`: no hold; `Some(reason)`: held, an unreadable file included (fail closed).
pub fn hold(install: &Path) -> Option<String> {
    match fs::read_to_string(install.join("hold")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(format!("hold file unreadable ({e})")),
        Ok(t) if t.trim_start().starts_with('{') => {
            let value=crate::lifecycle::stop_hold_status(install);
            Some(if value["state"]=="held" {format!("{} stop {}; reviewed recovery required",value["mode"].as_str().unwrap_or("unknown"),value["id"].as_str().unwrap_or("unknown"))}else{"Stop hold unknown; recovery required".into()})
        },
        Ok(t) => Some(t.lines().next().filter(|l| !l.trim().is_empty()).unwrap_or("emergency hold").to_string()),
    }
}

/// E3 admission is scoped to explicitly recovered tasks; ordinary holds still block everything.
pub fn task_hold(install:&Path,root:&Path,task:u64)->Option<String> {
    hold(install).filter(|_|!crate::recovery::allowed(install,root,task))
}

/// [`decide`] under the host lock, read and written back. An unreadable host file refuses the request (`host`) and is
/// rebuilt from this entry; the other services re-add theirs on their next write. Returns the host's held count.
pub fn sync(install: &Path, root: &str, mine: Entry, want: Option<Want>, now: i64) -> (Result<(), (&'static str, String)>, Option<u32>) {
    let io = |e: std::io::Error| ("host", format!("admission file: {e}"));
    let lock = match fs::OpenOptions::new().create(true).truncate(false).write(true).open(install.join("admission.lock")) {
        Ok(f) => f,
        Err(e) => return (Err(io(e)), None),
    };
    if let Err(e) = lock.lock() {
        return (Err(io(e)), None);
    }
    let path = install.join("admission.json");
    let (mut h, bad) = match fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Host::default(), None),
        Err(e) => (Host::default(), Some(e.to_string())),
        Ok(t) => serde_json::from_str(&t).map_or_else(|e| (Host::default(), Some(e.to_string())), |h| (h, None)),
    };
    let want = if bad.is_some() { None } else { want };
    let mut r = decide(&mut h, root, mine, want, now);
    if let Some(e) = bad {
        r = Err(("host", format!("admission file unreadable ({e}); rebuilt")));
    }
    if let Err(e) = crate::platform::write_atomic(&path, &serde_json::to_string(&h).unwrap_or_default()) {
        return (Err(("host", format!("admission file: {e}"))), None);
    }
    (r, Some(used(&h)))
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
