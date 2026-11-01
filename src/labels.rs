//! Shared board catalog; label spellings do not change the fold.
use crate::board::Result;

pub const FIXED: &[&str] = &["taskq", "taskq-host", "q-held", "q-ready", "q-blocked", "q-doing", "q-review", "q-ask", "q-later", "q-failed", "p0", "p1", "p2", "code", "docs", "research", "asset", "run-codex", "run-claude", "run-codex-app", "run-antigravity", "prefer-codex", "prefer-claude", "prefer-antigravity", "assignee-only", "verdict-confirmed", "verdict-refuted", "verdict-inconclusive"];

pub fn canonical(name: &str) -> &str {
    match name { "q-waiting" => "q-blocked", "priority-1" => "p1", "priority-2" => "p2", _ => name }
}

/// §4/§9: ordered project rules; every label (including task type) must match.
#[derive(Clone, Debug)]
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
        if found.len() > 1 { return Err(format!("conflicting {group} labels: {}", found.join(", "))); }
    }
    for prefix in ["run-", "host-"] {
        let mut found: Vec<_> = labels.iter().filter(|l| l.starts_with(prefix)).collect();
        found.sort(); found.dedup();
        if found.len() > 1 { return Err(format!("conflicting {prefix} labels: {found:?}")); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // A3/A4, R9/R13: subscription fallback must include Antigravity without overriding pins or unknown ownership.
    #[test]
    fn antigravity_fallback_keeps_pins_and_all_blockers() {
        for reviewer in [false, true] {
            for default in ["claude", "codex", "antigravity"] {
                let (rt, why) = select(&[], reviewer, default, |rt| if rt == "antigravity" { Ok(rt.into()) } else { Err(("usage-limit", format!("{rt} quota"))) }).unwrap();
                assert_eq!(rt, "antigravity");
                assert_eq!(why.is_none(), default == "antigravity");
                for pin in ["claude", "codex", "antigravity", "codex-app"] {
                    assert_eq!(candidates(&[format!("run-{pin}")], reviewer, default), [pin]);
                }
            }
            let error = select(&[], reviewer, "claude", |rt| Err(("capacity", format!("{rt} disabled")))).unwrap_err();
            for rt in ["claude", "codex", "antigravity"] { assert!(error.1.contains(rt), "{error:?}"); }
            let mut tried = vec![];
            assert!(select(&[], reviewer, "claude", |rt| { tried.push(rt.to_string()); Err(("unknown", "host policy unreadable".into())) }).is_err());
            assert_eq!(tried, ["claude"], "unknown common state must not permit fallback");
        }
        assert_eq!(candidates(&["prefer-codex".into()], false, "antigravity"), ["codex", "claude", "antigravity"], "§1 soft worker preference");
        assert!(definition("run-antigravity").is_some());
    }

    // #2137 A2: type/label routing is ordered and soft; pins and explicit preferences win.
    #[test]
    fn balancing_task_type_rules() {
        let rules = routing(&serde_json::json!({"routing":[{"labels":["code","3d"],"prefer":"antigravity"},{"labels":["ui"],"prefer":"codex"}]})).unwrap();
        let labels = vec!["code".into(),"3d".into(),"ui".into()];
        assert_eq!(project_default(&labels,false,&rules,"claude"),"antigravity");
        assert_eq!(project_default(&labels,true,&rules,"claude"),"claude");
        assert_eq!(project_default(&["code".into()],false,&rules,"claude"),"claude");
        let default = project_default(&labels,false,&rules,"claude");
        assert_eq!(route(&["prefer-codex".into()],false,default),"codex");
        assert_eq!(route(&["run-claude".into()],false,default),"claude");
        for rule in [serde_json::json!({"routing":null}),serde_json::json!({"routing":[{"labels":[],"prefer":"codex"}]}),serde_json::json!({"routing":[{"labels":["ui"],"prefer":"codex-app"}]}),serde_json::json!({"routing":[{"labels":[42],"prefer":"codex"}]})] { assert!(routing(&rule).is_err(),"{rule}"); }
    }

    // #2137 A2: soft preferences must fall back; explicit pins must still bind both roles.
    #[test]
    fn balancing_soft_preferences_fall_back() {
        for rt in ["claude", "codex", "antigravity"] {
            let label = vec![format!("prefer-{rt}")];
            assert_eq!(route(&label, false, "claude"), rt);
            assert_eq!(candidates(&label, false, "claude").len(), 3);
            let (chosen, reason) = select(&label, false, "claude", |c| if c == rt { Err(("capacity", format!("{c} disabled (limit 0)"))) } else { Ok(c.into()) }).unwrap();
            assert_ne!(chosen, rt);
            assert!(reason.unwrap().contains("disabled (limit 0)"));
            assert_eq!(candidates(&[format!("run-{rt}")], true, "codex"), [rt]);
        }
        assert_eq!(route(&["prefer-claude".into(), "prefer-codex".into()],false,"antigravity"),"antigravity","conflicting board preferences remain ignored");
    }

    // Chain: § 1 task labels on add. Risk: conflicting priority, runtime or host labels accepted.
    #[test]
    fn catalog_and_aliases() {
        for n in FIXED { let (hex, desc) = definition(n).unwrap(); assert_eq!(hex.len(), 6); assert!(desc.contains(';')); }
        assert!(definition("run-any").is_none());
        assert!(definition("bug").is_none());
        assert!(matches(&["priority-1".into()], "p1"));
        assert!(matches(&["q-blocked".into()], "q-waiting"));
        assert!(validate(&["p1".into(), "priority-1".into()]).is_ok());
        for pair in [["p1", "priority-2"], ["run-codex", "run-claude"], ["host-win", "host-mac"], ["code", "docs"]] {
            assert!(validate(&pair.map(String::from)).is_err());
        }
        assert!(validate(&["needs-a".into(), "needs-b".into()]).is_ok());
    }

    // Chain: § 4 Filter -> host.json `default_runtime` -> admitted runtime. Risk: a host without claude
    // routes its reviews or unlabelled work to claude and waits forever, or the default overrides `run-*`.
    #[test]
    fn route_uses_the_host_default_runtime_unless_run_names_one() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for default in crate::runtime::CLI_RUNTIMES {
            assert_eq!(route(&l(&["p1"]), true, default), default, "reviewer");
            assert_eq!(route(&l(&["run-claude"]), true, default), "claude", "explicit pin binds review");
            assert_eq!(route(&l(&[]), false, default), default, "worker without run-*");
            assert_eq!(route(&l(&["prefer-claude"]), false, default), "claude");
            assert_eq!(route(&l(&["prefer-codex", "prefer-claude"]), false, default), default);
            assert_eq!(route(&l(&["prefer-codex"]), false, default), "codex");
            assert_eq!(route(&l(&["run-claude"]), false, default), "claude", "run-* wins");
            assert_eq!(route(&l(&["run-codex-app", "prefer-codex"]), false, default), "codex-app");
        }
    }
}
