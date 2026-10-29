//! The one display model (#1592): state names and order, counts with one deduplication rule, run/paused/watch, task
//! order, age text, history series, notification labels and history periods. The CLI prints from it and the tray page
//! renders it; neither keeps its own copy (docs/design/task-counters.md, menu.md M3/M6/M14).

use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// Open states in counter-strip order with their owner names (task-counters.md K-A1, menu.md Tasks list order).
pub const STATES: [(&str, &str); 8] = [("doing", "Doing"), ("review", "Review"), ("ask", "Ask"), ("failed", "Failed"),
    ("ready", "Ready"), ("waiting", "Blocked"), ("later", "Later"), ("held", "Held")];
/// History event kinds in stacking order from the bottom (M6) with their legend names.
pub const SERIES: [(&str, &str); 4] = [("close", "Closed"), ("failed", "Failed"), ("ask", "Ask"), ("blocked", "Blocked")];
/// Notification kinds (M14) with the label of their banner and setting.
pub const NOTIFY: [(&str, &str); 4] = [("ask", "Question"), ("close", "Closed"), ("failed", "Failed"), ("gone", "Attempt gone")];
/// History periods: id, seconds, label. `custom` is the owner's own range.
pub const PERIODS: [(&str, i64, &str); 5] = [("5m", 300, "5 min"), ("1h", 3600, "Hour"), ("1d", 86400, "Day"), ("1w", 604800, "7 days"), ("1mo", 2592000, "30 days")];

/// The owner name of a state; `closed` has no counter but rows and notifications still name it.
pub fn name(state: &str) -> &str {
    STATES.iter().find(|s| s.0 == state).map_or(if state == "closed" { "Closed" } else { state }, |s| s.1)
}

/// § 7 the successor a close verdict names: `superseded by #M` or `replaced by #M`, any case.
pub fn successor(verdict: &str) -> Option<u64> {
    let v = verdict.to_lowercase();
    ["superseded by #", "replaced by #"].iter().find_map(|p| {
        let rest = &v[v.find(p)? + p.len()..];
        rest[..rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len())].parse().ok()
    })
}

/// § 7 close outcome, the text after `closed: ` in events, notifications and `status --closed`: `replaced by #M
/// (<its state>)`, `no change` (Changed `none`) or `delivered`, plus `, N items unverified` for each distinct
/// `A<k> gap|unknown` of its Checks lines (moved items of a replaced task are not counted). No verdict: the issue
/// closed on the board without `taskq: close`.
pub fn close_outcome(verdict: Option<&str>, successor_state: Option<&str>) -> String {
    let Some(v) = verdict else { return "closed on the board".into() };
    if let Some(m) = successor(v) {
        return format!("replaced by #{m} ({})", successor_state.unwrap_or("unknown"));
    }
    let changed = v.lines().find_map(|l| l.trim().strip_prefix("Changed:")).map(str::trim);
    let what = if changed.is_some_and(|c| c.to_lowercase().starts_with("none")) { "no change" } else { "delivered" };
    let mut items: Vec<&str> = v.lines().filter(|l| l.trim_start().starts_with("Checks:")).flat_map(|l| {
        let w: Vec<&str> = l.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|w| !w.is_empty()).collect();
        w.windows(2).filter(|p| p[0].starts_with('A') && p[0].len() > 1 && p[0][1..].bytes().all(|b| b.is_ascii_digit()) && matches!(p[1], "gap" | "unknown")).map(|p| p[0]).collect::<Vec<_>>()
    }).collect();
    items.sort();
    items.dedup();
    match items.len() {
        0 => what.into(),
        1 => format!("{what}, 1 item unverified"),
        k => format!("{what}, {k} items unverified"),
    }
}

/// Short age (menu.md M3): `<1m`, `5m`, `2h`, `3d`; a negative age reads as `<1m`.
pub fn age(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => "<1m".into(),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

/// `p0` first, then `p1`, then the rest (labels match their legacy `priority-N` spellings).
pub fn priority(labels: &[String]) -> u8 {
    if crate::labels::matches(labels, "p0") { 0 } else if crate::labels::matches(labels, "p1") { 1 } else { 2 }
}

/// Run admits (focus set), paused keeps a saved focus, watch observes only.
pub fn mode(focus: bool, saved: bool) -> &'static str {
    if focus { "run" } else if saved { "paused" } else { "watch" }
}

/// A saved `focus.paused` other than `off` (service pause).
pub fn saved(dot: &std::path::Path) -> bool {
    std::fs::read_to_string(dot.join("focus.paused")).is_ok_and(|f| !matches!(f.trim(), "" | "off"))
}

fn labels(t: &Value) -> Vec<String> {
    t["labels"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect()
}

/// Rows sort in counter-strip order, then priority, then number.
fn order(t: &Value) -> (usize, u8, u64) {
    let state = t["state"].as_str().unwrap_or("");
    (STATES.iter().position(|s| s.0 == state).unwrap_or(STATES.len()), priority(&labels(t)), t["n"].as_u64().unwrap_or(0))
}

/// A board identifies provider/server/repository; registration paths never deduplicate.
fn board(p: &Value) -> String {
    match p["issues"].as_str().map(|u| u.trim_end_matches('/')).filter(|u| !u.is_empty()) {
        Some(u) => format!("{}:{u}", p["board_kind"].as_str().unwrap_or("unknown")),
        None => String::new(),
    }
}

/// Open-state totals over projects, one row per board and number, and the projects whose data is partial: incomplete
/// or stale, without a board, or disagreeing with another registration of the same task (K-A2/K-A3).
pub fn tally(ps: &[Value]) -> (BTreeMap<&'static str, usize>, Vec<String>, usize) {
    let mut rows: HashMap<String, &Value> = HashMap::new();
    let mut partial: Vec<String> = vec![];
    let mut mark = |p: &Value| { let n = p["name"].as_str().unwrap_or("").to_string(); if !partial.contains(&n) { partial.push(n); } };
    for p in ps {
        let b = board(p);
        if b.is_empty() || p["complete"] != true { mark(p); }
        for t in p["tasks"].as_array().into_iter().flatten() {
            let key = format!("{b}/{}", t["n"]);
            if rows.get(&key).is_some_and(|x| ["state", "revision", "title"].iter().any(|k| x[*k] != t[*k])) { mark(p); }
            rows.insert(key, t);
        }
    }
    let totals = crate::snapshot::totals(rows.values().filter_map(|t| t["state"].as_str()));
    let open = rows.values().filter(|t| t["state"] != "closed").count();
    (totals, partial, open)
}

/// Counters in strip order; unknown (`null`) for every state while any project is partial, never zero (§ 4).
fn observation(ps: &[Value]) -> Value {
    let (t, partial, open) = tally(ps);
    let known = partial.is_empty();
    json!({"counts": STATES.iter().map(|(s, n)| json!({"state": s, "name": n, "n": known.then(|| t[s])})).collect::<Vec<_>>(),
        "partial": partial, "open": known.then_some(open)})
}

/// The page model over tray projects at board time `now`: each project gains `counts`, `partial`, `open`, `mode` and
/// `age_text` (status file age), `sample_age_text` (CPU/memory sample age);
/// each row gains `key`, `state_name`, `age`, `total_age`, step `age`, `rank` (global order) and `dup` (another
/// registration of the same board task shows it). Returns the shared vocabulary and the all-projects counters.
pub fn model(ps: &mut [Value], now: i64) -> Value {
    let elapsed = |from: &Value, to: Option<i64>| from.as_i64().filter(|f| *f > 0 && *f <= now && to.is_none_or(|t| t >= *f && t <= now)).map(|f| age(to.unwrap_or(now) - f));
    let mut ranked = vec![];
    let mut seen = std::collections::HashSet::new();
    for (i, p) in ps.iter_mut().enumerate() {
        let o = observation(std::slice::from_ref(p));
        let b = board(p);
        p["age_text"] = json!(p["age"].as_i64().map(age));
        // #1661 R17: the load sample ages by its own offset plus the status file age; absent while the gate is off
        p["sample_age_text"] = json!(p["capacity"]["sample_age_seconds"].as_i64().zip(p["age"].as_i64()).map(|(s, a)| age(s + a)));
        p["mode"] = json!(mode(p["focus"].as_array().is_some_and(|f| !f.is_empty()), p["saved"] == true));
        for k in ["counts", "partial", "open"] { p[k] = o[k].clone(); }
        let Some(ts) = p["tasks"].as_array_mut() else { continue };
        ts.sort_by_key(order);
        for (j, t) in ts.iter_mut().enumerate() {
            let key = format!("{b}/{}", t["n"]);
            t["key"] = json!(key);
            t["state_name"] = json!(name(t["state"].as_str().unwrap_or("")));
            t["age"] = json!(elapsed(&t["since"], None));
            t["total_age"] = json!(elapsed(&t["created"], None));
            for s in t["steps"].as_array_mut().into_iter().flatten() {
                let end = s["end"].as_i64();
                s["age"] = json!(if s["end"].is_null() || end.is_some() { elapsed(&s["start"], end) } else { None });
            }
            let dup = !seen.insert(key.clone());
            t["dup"] = json!(dup);
            if !dup { ranked.push((order(t), key, i, j)); }
        }
    }
    ranked.sort();
    for (rank, (_, _, i, j)) in ranked.into_iter().enumerate() { ps[i]["tasks"][j]["rank"] = json!(rank); }
    let pair = |v: &[(&str, &str)]| v.iter().map(|(id, n)| json!({"id": id, "name": n})).collect::<Vec<_>>();
    let mut periods: Vec<Value> = PERIODS.iter().map(|(id, _, n)| json!({"id": id, "name": n})).collect();
    periods.push(json!({"id": "custom", "name": "Custom"}));
    json!({"states": pair(&STATES), "series": pair(&SERIES), "notify": pair(&NOTIFY), "periods": periods,
        "all": observation(ps), "run": ps.iter().any(|p| p["mode"] == "run")})
}

#[cfg(test)]
mod tests {
    use super::*;

    // Chain: § 7 close outcome. Risk: a superseded, unchanged or partly unverified close reads as delivered work.
    #[test]
    fn close_outcome_names_successor_no_change_and_unverified_items() {
        // pilotq#90 verbatim shape: closed for #104 with all items moved
        let p90 = "Changed: none; owner chose option 2, #90 closes as superseded by #104, which delivers voice input\nChecks: Task fit: A1 unknown, A2 unknown, A3 unknown, A4 unknown; owner answer 2 moves all four to open #104\nOpen: #104 is held";
        assert_eq!(successor(p90), Some(104));
        assert_eq!(close_outcome(Some(p90), Some("ready")), "replaced by #104 (ready)");
        assert_eq!(close_outcome(Some("Changed: Replaced by #7"), None), "replaced by #7 (unknown)");
        assert_eq!(close_outcome(Some("Changed: none\nChecks: A1 ok"), None), "no change");
        let partly = "Changed: icon ships\nChecks: A1 ok, A2 gap\nChecks: A3 unknown; A4 unknown; A2 gap\nOpen: none";
        assert_eq!(close_outcome(Some(partly), None), "delivered, 3 items unverified");
        assert_eq!(close_outcome(Some("Changed: x\nChecks: A1 ok\nOpen: A2 unknown"), None), "delivered", "only Checks lines count");
        assert_eq!(close_outcome(None, None), "closed on the board");
        assert_eq!(successor("closes #12 after #13"), None);
    }

    fn project(name: &str, tasks: Value) -> Value {
        json!({"name": name, "issues": "https://github.com/o/r/issues/", "board_kind": "github", "complete": true, "focus": [], "saved": false, "tasks": tasks})
    }

    // Chain: menu.md M3 age. Risk: CLI and page print different units for one elapsed time.
    #[test]
    fn age_is_short_units() {
        assert_eq!([-5, 59, 300, 7200, 259200].map(age), ["<1m", "<1m", "5m", "2h", "3d"]);
    }

    // Chain: K-A2/K-A3 deduplication. Risk: a task registered twice counts twice, or a conflict shows a number.
    #[test]
    fn one_task_per_board_and_conflicts_are_unknown() {
        let row = json!({"n": 1, "state": "doing", "revision": 4, "title": "T"});
        let mut ps = [project("a", json!([row])), project("b", json!([row, {"n": 2, "state": "closed"}]))];
        let m = model(&mut ps, 100);
        assert_eq!((m["all"]["counts"][0].clone(), m["all"]["open"].clone()), (json!({"state": "doing", "name": "Doing", "n": 1}), json!(1)));
        assert_eq!((ps[0]["tasks"][0]["dup"].clone(), ps[1]["tasks"][0]["dup"].clone()), (json!(false), json!(true)));
        let mut ps = [project("a", json!([row])), project("b", json!([{"n": 1, "state": "review", "revision": 5, "title": "T"}]))];
        let m = model(&mut ps, 100);
        assert_eq!((m["all"]["partial"].clone(), m["all"]["open"].clone(), m["all"]["counts"][0]["n"].clone()), (json!(["b"]), Value::Null, Value::Null));
        let mut ps = [json!({"name": "c", "issues": "x", "complete": false, "focus": ["p0"], "tasks": []})];
        let m = model(&mut ps, 100);
        assert_eq!((m["all"]["partial"].clone(), ps[0]["mode"].clone(), m["run"].clone()), (json!(["c"]), json!("run"), json!(true)));
    }

    // Chain: menu.md Tasks list order and M3 age. Risk: rows leave strip/priority/number order or show an invented age.
    #[test]
    fn rows_sort_by_state_priority_number_and_carry_age_text() {
        let mut ps = [project("a", json!([{"n": 9, "state": "ready"}, {"n": 3, "state": "ready", "labels": ["priority-1"]},
            {"n": 7, "state": "doing", "since": 700, "created": 0, "steps": [{"start": 400, "end": 700}, {"start": 700, "end": null}, {"start": null, "end": null}]}]))];
        model(&mut ps, 1000);
        let t = ps[0]["tasks"].as_array().unwrap();
        assert_eq!(t.iter().map(|t| t["n"].as_u64().unwrap()).collect::<Vec<_>>(), [7, 3, 9]);
        assert_eq!(t.iter().map(|t| t["rank"].as_u64().unwrap()).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!((t[0]["age"].as_str(), t[0]["total_age"].as_str(), t[0]["state_name"].as_str()), (Some("5m"), None, Some("Doing")));
        assert_eq!(t[0]["steps"].as_array().unwrap().iter().map(|s| s["age"].clone()).collect::<Vec<_>>(), [json!("5m"), json!("5m"), Value::Null]);
        assert_eq!(ps[0]["mode"], "watch");
        // #1661 R17: the CPU/memory sample is as old as its offset plus the status file; no sample, no age
        let mut ps = [json!({"age": 30, "capacity": {"sample_age_seconds": 90}}), json!({"age": 30, "capacity": {}})];
        model(&mut ps, 1000);
        assert_eq!((ps[0]["sample_age_text"].as_str(), ps[1]["sample_age_text"].clone()), (Some("2m"), Value::Null));
    }
}
