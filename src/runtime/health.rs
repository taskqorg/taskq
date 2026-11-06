//! R6/R12: dependency observations never release an execution or submit input.
use super::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Monitor {
    paths: BTreeMap<(u64, u64), PathBuf>,
    pub faults: Vec<Value>,
}

#[cfg(test)]
thread_local! {
    pub static FAKE: std::cell::RefCell<Option<(PathBuf, Result<Option<u32>, String>)>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn resolve(rt: &str) -> Result<PathBuf, String> {
    let a = of(rt);
    if a.has(Cap::SandboxArgs) {
        agent(rt)?.executable(None)
    } else {
        Ok(which(a.bin))
    }
}

pub fn binary(rt: &str) -> Result<PathBuf, String> {
    let path = resolve(rt)?;
    if let Some(reason) = path_fault(&path) {
        return Err(format!("{rt} {reason}: {} (service PATH)", path.display()));
    }
    Ok(path)
}

fn path_fault(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return Some("executable missing".into());
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => None,
        Ok(_) => Some("executable is not a regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some("executable missing".into()),
        Err(e) => Some(format!("executable unknown: {e}")),
    }
}

fn read_daemon(rt: &str, bin: &Path, home: Option<&Path>) -> Result<Option<u32>, String> {
    let status = super::daemon_sandbox::status_of(super::probe_in(
        "runtime.daemon-health",
        bin,
        &["daemon", "status"],
        home.and_then(|h| accounts::environment(rt, h)),
    ))?;
    match status {
        Some(pid) if liveness(pid)? => Ok(Some(pid)),
        _ => Ok(None),
    }
}

fn resolved(rt: &str) -> PathBuf {
    #[cfg(test)]
    if let Some((path, _)) = FAKE.with(|f| f.borrow().clone()) {
        return path;
    }
    binary(rt).unwrap_or_else(|_| which(of(rt).bin))
}

impl Monitor {
    pub fn remember(&mut self, task: u64, execution: u64, rt: &str) {
        if of(rt).has(Cap::Cli) {
            self.paths
                .entry((task, execution))
                .or_insert_with(|| resolved(rt));
        }
    }

    pub fn observe(&mut self, kids: &[(u64, u64, String, Option<PathBuf>, bool)]) {
        #[cfg(test)]
        if FAKE.with(|f| f.borrow().is_none()) {
            return;
        }
        self.observe_with(kids, |rt, home| {
            #[cfg(test)]
            if let Some((_, status)) = FAKE.with(|f| f.borrow().clone()) {
                return status;
            }
            read_daemon(rt, &binary(rt)?, home)
        });
    }

    fn observe_with(
        &mut self,
        kids: &[(u64, u64, String, Option<PathBuf>, bool)],
        mut daemon: impl FnMut(&str, Option<&Path>) -> Result<Option<u32>, String>,
    ) {
        let live: BTreeSet<_> = kids.iter().map(|(n, x, _, _, _)| (*n, *x)).collect();
        self.paths.retain(|key, _| live.contains(key));
        let mut daemons = BTreeMap::new();
        let mut faults: BTreeMap<(String, PathBuf, String), Vec<u64>> = BTreeMap::new();
        for (n, x, rt, home, background) in kids {
            if !of(rt).has(Cap::Cli) {
                continue;
            }
            self.remember(*n, *x, rt);
            let path = &self.paths[&(*n, *x)];
            let reason = path_fault(path).or_else(|| {
                if !background || !of(rt).has(Cap::Background) {
                    return None;
                }
                match daemons
                    .entry((rt.clone(), home.clone()))
                    .or_insert_with(|| daemon(rt, home.as_deref()))
                {
                    Ok(Some(_)) => None,
                    Ok(None) => Some("daemon not running".to_owned()),
                    Err(e) => Some(format!("daemon unknown: {e}")),
                }
            });
            if let Some(reason) = reason {
                faults
                    .entry((rt.clone(), path.clone(), reason))
                    .or_default()
                    .push(*n);
            }
        }
        self.faults = faults
            .into_iter()
            .map(|((rt, path, reason), mut tasks)| {
                tasks.sort_unstable();
                tasks.dedup();
                json!({"runtime":rt,"path":path,"reason":reason,"tasks":tasks})
            })
            .collect();
    }

    pub fn reason(&self, task: u64) -> Option<&str> {
        self.faults.iter().find(|f| {
            f["tasks"]
                .as_array()
                .is_some_and(|tasks| tasks.iter().any(|n| n.as_u64() == Some(task)))
        })?["reason"]
            .as_str()
    }
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
