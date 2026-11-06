//! Local manager slots use the existing operation store and host admission authority (§3).
use crate::{Res, host, resources};
use coreq::process::Identity;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "taskq.slots/1";
const RETAIN: u64 = 24 * 3600;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: String,
    caller: String,
    op_id: String,
    root: PathBuf,
    runtime: String,
    pid: u32,
    started: String,
    boot: i64,
    expires_at: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    binding: Binding,
    key: String,
}
fn now() -> Res<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| "slot clock unknown".into())
}
fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn id(b: &Binding) -> String {
    format!(
        "slot-{}",
        digest(format!("{}\0{}", b.caller, b.op_id).as_bytes())
    )
}
fn bound(op: &resources::Op) -> Res<Binding> {
    serde_json::from_value(op.bound["local_slot"]["binding"].clone())
        .map_err(|_| "slot binding corrupt; occupancy unknown".into())
}
fn active(op: &resources::Op) -> bool {
    op.receipt["occupied"] == true
}
fn outcome(op: &mut resources::Op, state: &str, reason: &str, instance: &str, at: u64) {
    if op.receipt["state"] == state
        && op.receipt["reason"] == reason
        && op.receipt["instance"] == instance
        && op.receipt["at"]
            .as_u64()
            .is_some_and(|old| at >= old && at - old < 30)
    {
        return;
    }
    op.receipt["state"] = json!(state);
    if state == "reserved" {
        op.receipt["occupied"] = json!(true);
    }
    if matches!(state, "released" | "denied") {
        op.receipt["occupied"] = json!(false);
    }
    op.receipt["reason"] = json!(reason);
    op.receipt["instance"] = json!(instance);
    op.receipt["at"] = json!(at);
    if matches!(state, "released" | "denied") {
        op.expires_at = at;
    }
}
fn validate(b: &Binding) -> Res<()> {
    if b.schema != SCHEMA
        || !resources::id(&b.caller)
        || !resources::id(&b.op_id)
        || !b.root.is_absolute()
        || !crate::runtime::RUNTIMES.contains(&b.runtime.as_str())
        || b.pid == 0
        || b.pid > i32::MAX as u32
        || b.started.is_empty()
        || b.started.len() > 128
        || !b
            .started
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || b.boot <= 0
    {
        return Err("invalid local slot identity or release key".into());
    }
    Ok(())
}

pub(crate) fn exchange(install: &Path, action: &str, value: &Value) -> Res<Value> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {
        return Err("a worker cannot reserve or release external slots".into());
    }
    if !matches!(action, "reserve" | "observe" | "release") {
        return Err("slot action must be reserve, observe or release".into());
    }
    let r: Request =
        serde_json::from_value(value.clone()).map_err(|_| "invalid local slot request")?;
    validate(&r.binding)?;
    if r.key.len() != 64 || !r.key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid local slot release key".into());
    }
    let key = id(&r.binding);
    let hash = digest(r.key.as_bytes());
    let at = now()?;
    let service = host::service_status(install);
    let result = resources::within(install, resources::WAIT, |s| {
        if let Some(op) = s.ops.get_mut(&key) {
            if bound(op)? != r.binding || op.bound["local_slot"]["key_hash"] != hash {
                return Err("slot belongs to another caller, payload or release key".into());
            }
            if action == "release"
                && !matches!(op.receipt["state"].as_str(), Some("released" | "denied"))
            {
                crate::runtime::protected_app_server::release_ready(op)?;
                // Cancellation is durable before acknowledgment; even queued release cannot later admit a turn.
                op.bound["local_slot"]["release"] = json!(true);
                op.receipt["state"] = json!(if active(op) {
                    "releasing"
                } else {
                    "cancelling"
                });
            }
        } else {
            if action != "reserve" {
                return Err(
                    "slot identity absent or no longer retained; never start from this observation"
                        .into(),
                );
            }
            if service["state"] != "running"
                || service["fresh"] != true
                || service["slots_protocol"] != 1
            {
                return Err(
                    "slot authority unknown: a fresh taskq.slots/1 host is required".into(),
                );
            }
            if at >= r.binding.expires_at || r.binding.expires_at - at > 300 {
                return Err("slot admission deadline must be within 300 seconds".into());
            }
            if s.ops.len() >= resources::MAX_OPS {
                return Err("operation store full; no slot requested".into());
            }
            s.ops.insert(key.clone(),resources::Op {bound:json!({"local_slot":{"binding":r.binding,"key_hash":hash,"release":false}}),
                receipt:json!({"schema":SCHEMA,"caller":r.binding.caller,"op_id":r.binding.op_id,"state":"queued","occupied":false,"reason":"waiting for host admission","at":at}),
                expires_at:r.binding.expires_at,saved_at:None,writing:false,sent:true});
        }
        // within() durably saves and verifies the whole state before returning this receipt.
        Ok(s.ops[&key].receipt.clone())
    })?;
    let mut result = result;
    let service = host::service_status(install);
    let at = now()?;
    if service["state"] != "running"
        || service["fresh"] != true
        || service["slots_protocol"] != 1
        || result["instance"] != service["instance"]
        || result["at"].as_u64().is_none_or(|t| at < t || at - t > 60)
    {
        result["stored_state"] = result["state"].clone();
        result["state"] = json!("unknown");
        result["reason"] =
            json!("fresh matching host readback required; reconcile the same identity");
    }
    Ok(result)
}

/// Protected launch consumes an already admitted slot; possession of its key grants no filesystem access.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn reserved(
    install: &Path,
    s: &resources::State,
    value: &Value,
    runtime: &str,
) -> Res<String> {
    let r: Request =
        serde_json::from_value(value.clone()).map_err(|_| "invalid local slot request")?;
    validate(&r.binding)?;
    let key = id(&r.binding);
    let op = s.ops.get(&key).ok_or("slot identity absent; no launch")?;
    if r.key.len() != 64
        || !r.key.bytes().all(|c| c.is_ascii_hexdigit())
        || bound(op)? != r.binding
        || op.bound["local_slot"]["key_hash"] != digest(r.key.as_bytes())
    {
        return Err("slot belongs to another caller, payload or release key".into());
    }
    let service = host::service_status(install);
    let at = now()?;
    if r.binding.runtime != runtime
        || op.receipt["state"] != "reserved"
        || !active(op)
        || op.bound["local_slot"]["release"] != false
        || service["state"] != "running"
        || service["fresh"] != true
        || service["slots_protocol"] != 1
        || op.receipt["instance"] != service["instance"]
        || op.receipt["at"]
            .as_u64()
            .is_none_or(|t| at < t || at - t > 60)
        || crate::platform::boot_time()?.abs_diff(r.binding.boot) > 2
        || coreq::process::identity(r.binding.pid)
            .map_err(|_| "slot caller liveness unknown")?
            .as_ref()
            != Some(&Identity {
                pid: r.binding.pid,
                started: r.binding.started,
            })
    {
        return Err(
            "fresh reserved runtime slot and live caller required; retain occupancy".into(),
        );
    }
    Ok(key)
}

pub fn command(args: &crate::cli::Args) -> Res<()> {
    if args.pos.len() != 2
        || args.flags.iter().any(|(k, v)| {
            !["identity", "action", "request", "json"].contains(&k.as_str()) || v.len() != 1
        })
    {
        return Err("usage: service slots --identity PID | --action reserve|observe|release --request JSON [--json]".into());
    }
    let result = if let Some(pid) = args.one("identity") {
        if args.one("request").is_some() || args.one("action").is_some() {
            return Err("slot identity cannot include an action".into());
        }
        let pid = pid
            .parse::<u32>()
            .ok()
            .filter(|p| *p > 0 && *p <= i32::MAX as u32)
            .ok_or("invalid slot owner pid")?;
        let owner = coreq::process::identity(pid)
            .map_err(|e| e.to_string())?
            .ok_or("slot caller is not live")?;
        json!({"schema":SCHEMA,"pid":owner.pid,"started":owner.started,"boot":crate::platform::boot_time()?})
    } else {
        let text = args
            .one("request")
            .filter(|t| t.len() <= 8192)
            .ok_or("slot request required, at most 8192 bytes")?;
        exchange(
            &crate::platform::default_install(),
            args.one("action").ok_or("slot action required")?,
            &serde_json::from_str(text).map_err(|_| "invalid slot request JSON")?,
        )?
    };
    if args.flags.contains_key("json") || args.one("identity").is_some() {
        println!("{result}");
    } else {
        println!("Slot: {}", result["state"].as_str().unwrap_or("unknown"));
        if let Some(reason) = result["reason"].as_str() {
            println!("Reason: {reason}");
        }
    }
    Ok(())
}

/// The host mutex stays held across store read, admission, durable receipt and slot synchronization.
/// Store errors fence every new host launch; they never discard an in-memory reservation.
pub fn tick(install: &Path, h: &mut host::Host) -> Res<()> {
    let at = now()?;
    let boot = crate::platform::boot_time();
    let mut seen = std::collections::BTreeMap::new();
    reconcile(install, h, at, boot, |owner| {
        seen.entry((owner.pid, owner.started.clone()))
            .or_insert_with(|| {
                coreq::process::identity(owner.pid)
                    .map(|p| p.as_ref() == Some(owner))
                    .map_err(|e| e.to_string())
            })
            .clone()
    })
}
fn reconcile(
    install: &Path,
    h: &mut host::Host,
    at: u64,
    boot: Res<i64>,
    mut live: impl FnMut(&Identity) -> Res<bool>,
) -> Res<()> {
    resources::try_with(install, |s| {
        let ids: Vec<_> = s
            .ops
            .iter()
            .filter(|(_, op)| op.bound.get("local_slot").is_some())
            .map(|(id, _)| id.clone())
            .collect();
        let mut known = BTreeSet::new();
        // Restore every previous grant before attempting any new one, including across service restart.
        for key in &ids {
            let op = s.ops.get_mut(key).unwrap();
            let b = bound(op)?;
            validate(&b)?;
            if id(&b) != *key
                || !matches!(
                    op.receipt["state"].as_str(),
                    Some(
                        "queued"
                            | "busy"
                            | "cancelling"
                            | "reserved"
                            | "releasing"
                            | "unknown"
                            | "released"
                            | "denied"
                    )
                )
            {
                return Err("slot receipt corrupt; occupancy unknown".into());
            }
            if h.external_held(key)
                && !active(op)
                && !matches!(op.receipt["state"].as_str(), Some("released" | "denied"))
            {
                op.receipt["occupied"] = json!(true);
                op.receipt["state"] = json!("unknown");
                op.receipt["granted_at"] = json!(at);
            }
            if op.receipt["schema"] != SCHEMA
                || op.receipt["caller"] != b.caller
                || op.receipt["op_id"] != b.op_id
                || !op.receipt["occupied"].is_boolean()
                || !op.bound["local_slot"]["release"].is_boolean()
                || op.bound["local_slot"]["key_hash"]
                    .as_str()
                    .is_none_or(|hash| {
                        hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit())
                    })
                || active(op) && !op.receipt["granted_at"].is_u64()
                || matches!(op.receipt["state"].as_str(), Some("released" | "denied")) && active(op)
                || op.receipt["state"] == "reserved" && !active(op)
            {
                return Err("slot receipt identity invalid; occupancy unknown".into());
            }
            known.insert(key.clone());
            if active(op) {
                h.external_sync(key, &b.root, &b.runtime, true);
            }
        }
        if !h.external_known(&known) {
            return Err("previous slot disappeared from storage; occupancy retained".into());
        }
        // Rebuild eligible external demand under the host mutex; task passes own only task readiness.
        h.external_reset_ready();
        for key in ids {
            let op = s.ops.get_mut(&key).unwrap();
            let b = bound(op)?;
            let state = op.receipt["state"].as_str().unwrap().to_owned();
            if matches!(state.as_str(), "released" | "denied") {
                h.external_sync(&key, &b.root, &b.runtime, false);
                if at > op.expires_at.saturating_add(RETAIN) {
                    s.ops.remove(&key);
                } else {
                    let expires_at = op.expires_at;
                    let reason = op.receipt["reason"]
                        .as_str()
                        .unwrap_or("terminal slot retained")
                        .to_owned();
                    outcome(op, &state, &reason, &h.instance, at);
                    op.expires_at = expires_at;
                }
                continue;
            }
            if op.bound["local_slot"]["release"] == true {
                if crate::runtime::protected_app_server::release_ready(op).is_err()
                    && !boot.as_ref().is_ok_and(|boot| {
                        *boot > op.receipt["granted_at"].as_i64().unwrap_or(i64::MAX)
                    })
                {
                    outcome(
                        op,
                        "unknown",
                        "protected native completion unknown; original slot retained",
                        &h.instance,
                        at,
                    );
                    continue;
                }
                // Only the exact immutable binding and release key can persist this completion attestation.
                outcome(
                    op,
                    "released",
                    "consumer confirmed entire turn ended",
                    &h.instance,
                    at,
                );
                resources::save(install, s)?;
                h.external_sync(&key, &b.root, &b.runtime, false);
                continue;
            }
            if active(op) {
                if boot
                    .as_ref()
                    .is_ok_and(|boot| *boot > op.receipt["granted_at"].as_i64().unwrap_or(i64::MAX))
                {
                    outcome(
                        op,
                        "released",
                        "verified later host boot ended prior execution",
                        &h.instance,
                        at,
                    );
                    resources::save(install, s)?;
                    h.external_sync(&key, &b.root, &b.runtime, false);
                    continue;
                }
                let (state, reason) = match (
                    &boot,
                    live(&Identity {
                        pid: b.pid,
                        started: b.started.clone(),
                    }),
                ) {
                    (Ok(current), Ok(true)) if current.abs_diff(b.boot) <= 2 => {
                        ("reserved", "slot retained for caller".to_owned())
                    }
                    (_, Err(e)) => (
                        "unknown",
                        format!("caller liveness unknown: {e}; slot retained"),
                    ),
                    _ => (
                        "unknown",
                        "caller absent or boot uncertain; entire turn exit unproved; slot retained"
                            .into(),
                    ),
                };
                outcome(op, state, &reason, &h.instance, at);
                continue;
            }
            let decision = (|| -> Res<()> {
                if at >= b.expires_at {
                    return Err("denied: admission deadline expired".into());
                }
                if boot.as_ref().is_err()
                    || !boot.as_ref().is_ok_and(|boot| boot.abs_diff(b.boot) <= 2)
                {
                    return Err("unknown: caller boot cannot be verified".into());
                }
                match live(&Identity {
                    pid: b.pid,
                    started: b.started.clone(),
                }) {
                    Ok(true) => {}
                    Ok(false) => return Err("denied: caller exited before admission".into()),
                    Err(e) => return Err(format!("unknown: caller liveness: {e}")),
                }
                let roots = host::projects(install).map_err(|e| format!("unknown: {e}"))?;
                if !roots.contains(&b.root) || b.root.canonicalize().ok().as_ref() != Some(&b.root)
                {
                    return Err("denied: caller root is not registered canonically".into());
                }
                let v = crate::config::ProjectFile::of(&b.root)
                    .record()
                    .map_err(|e| format!("unknown: {e}"))?
                    .ok_or("unknown: project configuration absent")?;
                // The host loop's guarded policy may forbid admission despite a valid host.json.
                let policy = h.policy.as_ref().map_err(|e| format!("unknown: {e}"))?;
                let cap = policy
                    .project(&b.root, &v)
                    .map_err(|e| format!("unknown: {e}"))?;
                h.external_reserve(&key, &b.root, &b.runtime, &cap, h.elapsed())
            })();
            match decision {
                Ok(()) => {
                    outcome(op, "reserved", "host slot granted", &h.instance, at);
                    op.receipt["granted_at"] = json!(at);
                }
                Err(reason) => outcome(
                    op,
                    if reason.starts_with("denied:") {
                        "denied"
                    } else if reason.contains("unknown") || h.rebuilding {
                        "unknown"
                    } else {
                        "busy"
                    },
                    &reason,
                    &h.instance,
                    at,
                ),
            }
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "external_slots_tests.rs"]
mod tests;
