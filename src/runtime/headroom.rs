//! Structured quota observations rank new launches; they never release quota holds.
use crate::runtime::Cap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub(crate) const FILE: &str = "quota-headroom.json";
const TTL: i64 = 300;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    source: String,
    at: i64,
    remaining: f64,
    reset: Option<i64>,
}
type Store = BTreeMap<String, BTreeMap<String, BTreeMap<String, Observation>>>;

fn read(install: &Path) -> Result<Store, String> {
    let store: Store = crate::data::json(&install.join(FILE))?.map_or(Ok(Store::new()), |v| {
        serde_json::from_value(v).map_err(|e| format!("quota headroom unknown: {e}"))
    })?;
    if store.iter().any(|(rt, accounts)| {
        !super::CLI_RUNTIMES.contains(&rt.as_str())
            || accounts.iter().any(|(a, windows)| {
                !super::accounts::valid_label(a)
                    || windows.len() > 32
                    || windows.iter().any(|(w, o)| {
                        w.is_empty()
                            || w.len() > 40
                            || !w
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                            || o.at < 0
                            || o.reset.is_some_and(|r| r < 0)
                            || !o.remaining.is_finite()
                            || !(0.0..=1.0).contains(&o.remaining)
                    })
            })
    }) {
        return Err("quota headroom storage invalid".into());
    }
    Ok(store)
}

/// The latest reading per window since the attempt's last start or resume, read per record by AgentQ so each keeps its
/// `.headroom` event ID. A window without a name or used fraction stays unobserved: unknown, never healthy.
fn observations(rt: &str, events: &[Value]) -> BTreeMap<String, (usize, f64, Option<i64>)> {
    let mut rows = BTreeMap::new();
    let Ok(agent) = super::agent(rt) else {
        return rows;
    };
    let start = events
        .iter()
        .rposition(|e| {
            e["type"] == "taskq.rules" && matches!(e["phase"].as_str(), Some("start" | "resume"))
        })
        .unwrap_or(0);
    for (index, e) in events.iter().enumerate().skip(start) {
        for w in unique_names(agent.log_limits(&e.to_string(), None, None)) {
            let (Some(name), Some(used)) = (w.window, w.used.filter(|u| (0.0..=1.0).contains(u)))
            else {
                continue;
            };
            if !name.is_empty()
                && name.len() <= 40
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                rows.insert(
                    name,
                    (
                        index,
                        1.0 - used,
                        w.resets_at.and_then(|r| i64::try_from(r).ok()),
                    ),
                );
            }
        }
    }
    rows
}

/// Codex limit groups share window lengths: the second `300m` of one record reads `300m-2`, so it cannot hide an
/// exhausted sibling. ponytail: a group that leaves a record shifts its siblings' suffixes; the 300 s expiry covers it.
pub(super) fn unique_names(
    mut windows: Vec<agentq_runtime::agent::Window>,
) -> Vec<agentq_runtime::agent::Window> {
    let mut seen = BTreeMap::<String, u32>::new();
    for w in &mut windows {
        if let Some(name) = &mut w.window {
            let count = seen.entry(name.clone()).or_default();
            *count += 1;
            if *count > 1 {
                *name = format!("{name}-{count}");
            }
        }
    }
    windows
}

pub fn observe(install: &Path, rt: &str, log: &Path, now: i64) -> Result<(), String> {
    observe_in(install, rt, log, now, super::rollout)
}

fn observe_in(
    install: &Path,
    rt: &str,
    log: &Path,
    now: i64,
    rollout: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> Result<(), String> {
    let Some(mut events) = super::events(log, 0) else {
        return Ok(());
    };
    let mut source = log.to_path_buf();
    // #2263: `codex exec --json` logs carry no rate limits; the thread's rollout records them in `token_count`.
    // ponytail: rereads the whole rollout each pass; read from a saved offset if rollouts grow large.
    if super::of(rt).has(Cap::RolloutQuota) && observations(rt, &events).is_empty() {
        let thread = super::log_text(log, 0)
            .and_then(|text| super::classify(rt, &text))
            .and_then(|r| r.session);
        if let Some(path) = thread.and_then(|t| rollout(&t)) {
            if let Some(rollout) = super::events(&path, 0) {
                events = rollout;
                source = path;
            }
        }
    }
    let rows = observations(rt, &events);
    if rows.is_empty() {
        return Ok(());
    }
    let label = super::accounts::of_log(log);
    let modified = std::fs::metadata(&source)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    let at = now.min(modified);
    let marker = log.with_extension("headroom");
    let read_seen = || -> Result<BTreeMap<String, Observation>, String> {
        crate::data::json(&marker)?.map_or(Ok(BTreeMap::new()), |v| {
            serde_json::from_value(v).map_err(|_| "quota headroom event marker invalid".into())
        })
    };
    let rows: Vec<_> = rows
        .into_iter()
        .map(|(name, (index, remaining, reset))| {
            let source = format!("{index}:{}", events[index]);
            let source = ring::digest::digest(&ring::digest::SHA256, source.as_bytes())
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            (
                name,
                Observation {
                    source,
                    at,
                    remaining,
                    reset: reset.filter(|r| *r >= 0),
                },
            )
        })
        .collect();
    let seen = read_seen()?;
    if rows
        .iter()
        .all(|(name, o)| seen.get(name).is_some_and(|old| old.source == o.source))
    {
        return Ok(());
    }
    std::fs::create_dir_all(install).map_err(|e| e.to_string())?;
    let _lock = coreq::file_lock::try_lock(&install.join("accounts-state.lock"))
        .map_err(|e| format!("quota headroom save busy: {e}"))?;
    let mut saved = read(install)?;
    let mut seen = read_seen()?;
    let windows = saved
        .entry(rt.into())
        .or_default()
        .entry(label)
        .or_default();
    for (name, o) in rows {
        if seen.get(&name).is_some_and(|old| old.source == o.source) {
            continue;
        }
        if windows.get(&name).is_none_or(|old| old.at <= o.at) {
            windows.insert(name.clone(), o.clone());
        }
        seen.insert(name, o);
    }
    crate::platform::write_synced(
        &install.join(FILE),
        &serde_json::to_string(&saved).map_err(|e| e.to_string())?,
    )?;
    crate::platform::write_synced(
        &marker,
        &serde_json::to_string(&seen).map_err(|e| e.to_string())?,
    )?;
    Ok(())
}

/// Every saved window per runtime, label and name: (at, remaining fraction, reset). Unreadable storage is empty, so
/// ranking reads unknown usage; it never holds a start.
pub(crate) type Observed =
    BTreeMap<String, BTreeMap<String, BTreeMap<String, (i64, f64, Option<i64>)>>>;

pub(crate) fn observed(install: &Path) -> Observed {
    read(install)
        .unwrap_or_default()
        .into_iter()
        .map(|(rt, accounts)| {
            (
                rt,
                accounts
                    .into_iter()
                    .map(|(label, windows)| {
                        (
                            label,
                            windows
                                .into_iter()
                                .map(|(name, o)| (name, (o.at, o.remaining, o.reset)))
                                .collect(),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

/// Saves a poll's windows as observations; the caller holds the account lock. A newer observation stays.
pub(crate) fn record_polled(
    install: &Path,
    at: i64,
    polls: &[(&str, &str, &[super::usage::Window])],
) -> Result<(), String> {
    if polls.iter().all(|(_, _, windows)| windows.is_empty()) {
        return Ok(());
    }
    let mut saved = read(install)?;
    for (rt, label, polled) in polls {
        if polled.is_empty() {
            continue;
        }
        let windows = saved
            .entry(rt.to_string())
            .or_default()
            .entry(label.to_string())
            .or_default();
        for w in polled.iter() {
            if windows.len() >= 32 && !windows.contains_key(&w.name) {
                continue;
            }
            if windows.get(&w.name).is_none_or(|old| old.at <= at) {
                windows.insert(
                    w.name.clone(),
                    Observation {
                        source: "poll".into(),
                        at,
                        remaining: 1.0 - f64::from(w.used) / 100.0,
                        reset: w.reset,
                    },
                );
            }
        }
    }
    crate::platform::write_synced(
        &install.join(FILE),
        &serde_json::to_string(&saved).map_err(|e| e.to_string())?,
    )
}

pub fn remaining(install: &Path, rt: &str, now: i64) -> Result<Option<f64>, String> {
    let saved = read(install)?;
    let accounts = super::accounts::Host::read(install);
    if let Some(e) = accounts.error {
        return Err(e);
    }
    let Some(label) = accounts.active(rt, now) else {
        return Ok(None);
    };
    if accounts.usage.incomplete(rt, &label, now) {
        return Ok(None);
    }
    Ok(saved
        .get(rt)
        .and_then(|a| a.get(&label))
        .into_iter()
        .flat_map(|w| w.values())
        .filter(|o| now >= o.at && now - o.at <= TTL && o.reset.is_none_or(|reset| now < reset))
        .map(|o| o.remaining)
        .min_by(f64::total_cmp))
}

/// #2263: why `remaining` has no fraction, so status names a cause instead of "unknown".
pub fn cause(install: &Path, rt: &str, now: i64) -> String {
    let accounts = super::accounts::Host::read(install);
    let Some(label) = accounts.active(rt, now) else {
        return "every account on quota hold".into();
    };
    if accounts.usage.incomplete(rt, &label, now) {
        return format!("account {label}: unreadable quota windows");
    }
    let latest = read(install).ok().and_then(|s| {
        s.get(rt)
            .and_then(|a| a.get(&label))
            .and_then(|w| w.values().map(|o| o.at).max())
    });
    match latest {
        None if super::of(rt).has(Cap::Background) => {
            format!("account {label}: Claude background jobs report no quota")
        }
        None => format!("account {label}: no quota observation yet"),
        // A fixed board time, not an age: the heartbeat body stays equal across passes until the state changes (§ 4 step 3).
        Some(at) => format!(
            "account {label}: last quota observation {} expired",
            crate::time::utc(at)
        ),
    }
}

#[cfg(test)]
#[path = "headroom_tests.rs"]
mod tests;
