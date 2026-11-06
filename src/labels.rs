//! Shared board catalog; label spellings do not change the fold.
use crate::board::Result;

pub const FIXED: &[&str] = &["taskq", "taskq-host", "taskq-accounts", "q-held", "q-ready", "q-blocked", "q-doing", "q-review", "q-ask", "q-later", "q-failed", "p0", "p1", "p2", "code", "docs", "research", "asset", "run-codex", "run-claude", "run-codex-app", "run-antigravity", "prefer-codex", "prefer-claude", "prefer-antigravity", "assignee-only", "verdict-confirmed", "verdict-refuted", "verdict-inconclusive"];

pub fn canonical(name: &str) -> &str {
    match name { "q-waiting" => "q-blocked", "priority-1" => "p1", "priority-2" => "p2", _ => name }
}

/// §4/§9: ordered project rules; every label (including task type) must match.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routing { pub labels: Vec<String>, pub prefer: String }

pub fn routing(value: &serde_json::Value) -> Result<Vec<Routing>> {
    let Some(value) = value.get("routing") else { return Ok(vec![]) };
    value.as_array().ok_or("routing must be an array")?.iter().map(|r| {
        let labels: Vec<String> = r["labels"].as_array().ok_or("routing labels must be an array")?.iter()
            .map(|l| l.as_str().filter(|l| !l.is_empty() && !l.contains(char::is_whitespace)).map(String::from).ok_or("routing labels must be non-empty words"))
            .collect::<std::result::Result<_, _>>()?;
        let prefer = r["prefer"].as_str().filter(|rt| crate::runtime::CLI_RUNTIMES.contains(rt)).ok_or("routing prefer must be claude, codex or antigravity")?;
        if labels.is_empty() { return Err("routing labels must not be empty".into()); }
        Ok(Routing { labels, prefer: prefer.into() })
    }).collect()
}

pub fn project_default<'a>(labels: &[String], reviewer: bool, rules: &'a [Routing], default: &'a str) -> &'a str {
    if reviewer { return default; }
    rules.iter().find(|r| r.labels.iter().all(|l| matches(labels, l))).map_or(default, |r| r.prefer.as_str())
}

/// §4: explicit owner pins bind both roles; without a pin, route returns a preference.
pub fn route(labels: &[String], reviewer: bool, default: &str) -> String {
    if let Some(rt) = labels.iter().find_map(|l| l.strip_prefix("run-")) { return rt.into(); }
    if reviewer { return default.into(); }
    let prefer: Vec<&str> = labels.iter().filter_map(|l| l.strip_prefix("prefer-")).collect();
    if prefer.len() == 1 && crate::runtime::CLI_RUNTIMES.contains(&prefer[0]) { prefer[0] } else { default }.into()
}

/// §4/R13: preferred CLI, then the other subscriptions; strict pins and the app never fall back.
pub fn candidates(labels: &[String], reviewer: bool, default: &str) -> Vec<String> {
    let preferred = route(labels, reviewer, default);
    let fallback = crate::runtime::CLI_RUNTIMES.contains(&preferred.as_str()) && !labels.iter().any(|l| l.starts_with("run-"));
    let mut candidates = vec![preferred.clone()];
    if fallback { candidates.extend(crate::runtime::CLI_RUNTIMES.iter().filter(|rt| **rt != preferred).map(|rt| rt.to_string())); }
    candidates
}

/// §4/§9 admission policy; the caller checks each candidate independently before claiming anything.
pub fn select(labels: &[String], reviewer: bool, default: &str,
    mut eligible: impl FnMut(&str) -> std::result::Result<String, (&'static str, String)>) -> std::result::Result<(String, Option<String>), (&'static str, String)> {
    let mut failures = vec![];
    let mut code = "unknown";
    for candidate in candidates(labels, reviewer, default) {
        match eligible(&candidate) {
            Ok(rt) => return Ok((rt, (!failures.is_empty()).then(|| format!("fallback: {}; selected {candidate}", failures.join("; "))))),
            Err(e) if e.0 == "unknown" => return Err(e),
            Err(e) => { if failures.is_empty() { code = e.0; } failures.push(e.1); }
        }
    }
    Err((code, failures.join("; ")))
}

/// R13/§4: only an owner-enabled project may ignore an explicitly disabled CLI pin.
pub fn select_project(labels: &[String], reviewer: bool, default: &str, auto_reroute: bool,
    disabled: impl Fn(&str) -> bool,
    eligible: impl FnMut(&str) -> std::result::Result<String, (&'static str, String)>) -> std::result::Result<(String, Option<String>), (&'static str, String)> {
    let pin=labels.iter().find_map(|l|l.strip_prefix("run-"))
        .filter(|rt|auto_reroute && crate::runtime::CLI_RUNTIMES.contains(rt) && disabled(rt));
    let Some(pin)=pin else {return select(labels,reviewer,default,eligible)};
    let filtered:Vec<_>=labels.iter().filter(|l|!l.starts_with("run-")).cloned().collect();
    let prefix=format!("auto_reroute: run-{pin} disabled by project/host policy");
    match select(&filtered,reviewer,default,eligible) {
        Ok((rt,reason))=>Ok((rt.clone(),Some(format!("{prefix}; {}",reason.unwrap_or_else(||format!("selected {rt}")))))),
        Err((code,reason))=>Err((code,format!("{prefix}; {reason}"))),
    }
}

pub fn matches(labels: &[String], filter: &str) -> bool {
    labels.iter().any(|l| canonical(l) == canonical(filter))
}

pub fn definition(name: &str) -> Option<(&'static str, String)> {
    let (color, meaning, change) = match name {
        "q-doing" => ("368477", "Worker active; attempt owns this task", "taskq commands/log"),
        "q-ask" => ("D99536", "Owner answer needed; no admission", "taskq answer"),
        "q-failed" => ("C65365", "Task failed; no admission", "taskq requeue"),
        "q-ready" => ("B7BEC7", "Ready for an eligible host", "taskq commands/log"),
        "q-blocked" => ("AEB5BF", "Open dependency; no admission", "close dependencies"),
        "q-review" => ("8C99A5", "Result awaiting review; not success", "taskq commands/log"),
        "q-held" => ("D5D9DE", "Held; listed, no admission until start", "taskq start"),
        "q-later" => ("D5D9DE", "Parked; no admission", "taskq requeue"),
        "taskq" => ("D5D9DE", "Enables trusted issue admission", "edit issue labels"),
        "taskq-host" => ("D5D9DE", "Host heartbeat; not a task", "service manages this label"),
        "taskq-accounts" => ("D5D9DE", "Fleet account policy; not a task", "taskq accounts fleet manages this label"),
        "p0" => ("D1495B", "Urgent; passes any focus, admitted first", "edit issue labels"),
        "p1" => ("D5D9DE", "Priority 1; admitted before priority 2", "edit issue labels"),
        "p2" => ("D5D9DE", "Priority 2; also the absent default", "edit issue labels"),
        "code" | "docs" | "research" | "asset" => ("D5D9DE", "Result type; no admission restriction", "edit issue labels"),
        "verdict-confirmed" | "verdict-refuted" | "verdict-inconclusive" => ("D5D9DE", "Experiment verdict; read by Decide", "taskq close"),
        "assignee-only" => ("BAC7D3", "Board account must be an assignee", "edit assignees/labels"),
        "run-codex" | "run-claude" | "run-codex-app" | "run-antigravity" => ("BAC7D3", "Requires this enabled runtime", "edit issue labels"),
        "prefer-codex" | "prefer-claude" | "prefer-antigravity" => ("BAC7D3", "Soft worker preference; eligible subscription fallback allowed", "edit issue labels"),
        n if n.starts_with("host-") && n.len() > 5 => ("BAC7D3", "Requires this host", "edit issue labels"),
        n if n.starts_with("needs-") && n.len() > 6 => ("BAC7D3", "Requires this capability; all needs must match", "edit issue labels/host has"),
        n if n.starts_with("area-") && n.len() > 5 => ("D5D9DE", "Subsystem; used by focus/status filters", "edit issue labels"),
        _ => return None,
    };
    Some((color, format!("{meaning}; {change}.")))
}

pub fn validate(labels: &[String]) -> Result<()> {
    for (group, values) in [
        ("type", vec!["code", "docs", "research", "asset"]),
        ("priority", vec!["p0", "p1", "p2"]),
    ] {
        let mut found: Vec<_> = labels.iter().map(|l| canonical(l)).filter(|l| values.contains(l)).collect();
        found.sort(); found.dedup();
        if found.len() > 1 { return Err(format!("conflicting {group} labels: {}", found.join(", ")).into()); }
    }
    for prefix in ["run-", "host-"] {
        let mut found: Vec<_> = labels.iter().filter(|l| l.starts_with(prefix)).collect();
        found.sort(); found.dedup();
        if found.len() > 1 { return Err(format!("conflicting {prefix} labels: {found:?}").into()); }
    }
    Ok(())
}

#[cfg(test)]
#[path = "labels_tests.rs"]
mod tests;
