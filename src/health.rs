//! Health checks (#2194, [host service](../docs/design/host-service.md#health-checks-2194)): per board the token
//! permissions taskq needs, installed vs latest release, report collection and the last service error, each with one
//! fix; the heartbeat `doctor` line that publishes them; and the `taskq doctor --hosts` host and worker rows, read from
//! the board alone. Nothing here prints a token, a path or a raw response body into a heartbeat.

use std::path::Path;

use serde_json::{json, Value};

use crate::check::{Item, Probe};
use crate::core::State;

/// Seconds between two health checks of a project service.
pub const EVERY: u64 = 600;
/// A published result older than this reads unknown: the service stopped checking.
const STALE: i64 = 3 * EVERY as i64;
/// Board permission name, check id and the fine-grained grant the owner gives.
const TOKEN: [(&str, &str, &str); 3] = [
    ("Issues", "token-issues", "Issues: Read and write"),
    ("Contents", "token-contents", "Contents: Read"),
    ("Pull requests", "token-pulls", "Pull requests: Read and write"),
];
/// Every check id; a heartbeat `doctor` line carries only these.
pub const IDS: [&str; 6] = ["token-issues", "token-contents", "token-pulls", "version", "reports", "service-error"];

/// `<install>/latest-release`: the latest release version the host doctor read last.
const LATEST: &str = "latest-release";

/// The host doctor's release read: saves the latest release version for every project service of this install.
pub fn record_latest(install: &Path) -> Result<String, String> {
    let v = crate::platform::latest_release(install)?;
    crate::platform::write_atomic(&install.join(LATEST), &v).map_err(|e| e.to_string())?;
    Ok(v)
}

/// The latest release the host doctor saved; unknown when unread or older than [`STALE`].
pub fn cached_latest(install: &Path) -> Result<String, String> {
    let path = install.join(LATEST);
    match (std::fs::read_to_string(&path), crate::file_age(&path)) {
        (Ok(v), Some(age)) if age <= STALE as u64 && !v.trim().is_empty() => Ok(v.trim().to_string()),
        (Ok(_), _) => Err(format!("latest release last read over {} ago", crate::age(STALE))),
        _ => Err("latest release not read yet by the host doctor".into()),
    }
}

/// The HTTP status a board error names (`... HTTP 403 ...`).
pub fn http_status(e: &str) -> Option<u16> {
    let i = e.find("HTTP ")?;
    e[i + 5..].get(..3)?.parse().ok()
}

/// The first line of an error, bounded.
fn short(e: &str) -> String {
    e.lines().next().unwrap_or_default().chars().take(160).collect()
}

/// `token-*`: one item per permission of [`TOKEN`] from the board's [`access`](crate::board::Board::access) probes.
pub fn token_items(repo: &str, access: &[(&'static str, Probe)]) -> Vec<Item> {
    TOKEN.iter().map(|(name, id, grant)| match access.iter().find(|(n, _)| n == name).map(|(_, p)| p) {
        Some(Probe::Ok) => Item::ok(*id, format!("token reads {name} on {repo}")),
        Some(Probe::Gap(why)) => Item::gap(*id, format!("token lacks {grant} on {repo} ({why})"),
            format!("owner: edit the fine-grained board token, Repository permissions > {grant}, for {repo}")),
        Some(Probe::Unknown(e)) => Item::unknown(*id, format!("token {name} read on {repo} unknown: {}", short(e)),
            "check the network and the board, then run `taskq doctor` again"),
        None => Item::unknown(*id, format!("token {name} check unsupported on this board"), "check the board token permissions in the board settings"),
    }).collect()
}

/// `version`: the installed version against the latest release.
pub fn version_item(installed: &str, latest: &Result<String, String>) -> Item {
    match latest {
        Ok(l) if coreq::update::newer(l, installed) => Item::gap("version", format!("installed {installed} is behind the latest release {l}"), "run `taskq update`"),
        Ok(l) => Item::ok("version", format!("installed {installed}, latest release {l}")),
        Err(e) => Item::unknown("version", format!("installed {installed}, latest release unknown: {}", short(e)), "check the network, then run `taskq update --check`"),
    }
}

/// `reports`: the cached report collection ([`crate::reports::cached`]).
pub fn reports_item(cached: &Value) -> Item {
    if cached["complete"] == true { return Item::ok("reports", "report collection complete"); }
    Item::gap("reports", format!("report collection incomplete: {}", cached["reason"].as_str().unwrap_or("reason unknown")),
        "run `taskq service status`; if collection stays incomplete, run `taskq service restart`")
}

/// `service-error`: the last pass error of the project's `status.json` (`age` in seconds).
pub fn error_item(status: Result<Option<Value>, String>, age: Option<u64>, ttl: i64) -> Item {
    match status {
        Ok(Some(s)) if age.is_some_and(|a| crate::snapshot::live(a as i64, ttl)) => match s["error"].as_str().or(s["focus_error"].as_str()) {
            None => Item::ok("service-error", "last service pass had no error"),
            Some(e) => Item::gap("service-error", format!("last service pass failed: {}", short(e)), "read `taskq service status` and fix the named cause"),
        },
        Ok(Some(_)) => Item::unknown("service-error", "service status is stale; the service may be stopped", "run `taskq service status`; start it with `taskq service install`"),
        Ok(None) => Item::unknown("service-error", "no service status; no service ran this project here", "run `taskq service install`"),
        Err(e) => Item::unknown("service-error", format!("service status unreadable: {}", short(&e)), "run `taskq service status`"),
    }
}

/// All checks of one board; `latest` is the latest release read once per run.
pub fn board_items(b: &dyn crate::board::Board, repo: &str, root: &Path, install: &Path, latest: &Result<String, String>) -> Vec<Item> {
    let mut items = token_items(repo, &b.access());
    let installed = crate::platform::tool(install).current().unwrap_or_else(|| crate::platform::VERSION.into());
    items.push(version_item(&installed, latest));
    items.push(reports_item(&crate::reports::cached(root)));
    let path = crate::dot(root).join("status.json");
    items.push(error_item(crate::data::json(&path), crate::file_age(&path), crate::snapshot::ttl(root)));
    items
}

/// The checks of a board whose board did not open: the token reads are unknown with the cause.
pub fn unopened(repo: &str, e: &str) -> Vec<Item> {
    token_items(repo, &TOKEN.iter().map(|(n, ..)| (*n, Probe::Unknown(format!("board not opened: {e}")))).collect::<Vec<_>>())
}

/// `taskq doctor` lines of one board: `ok|gap|unknown <id>: <text>`, then `  fix: ...` under each open item.
pub fn lines(repo: &str, items: &[Item]) -> Vec<String> {
    let mut out = vec![format!("board {repo}")];
    for i in items {
        out.push(format!("  {:<7} {}: {}", i.status, i.id, i.text));
        if i.status != "ok" { out.push(format!("          fix: {}", i.fix.as_deref().unwrap_or_default())); }
    }
    out
}

/// The heartbeat line: `doctor <time> ok`, or `doctor <time> <id>:gap|unknown ...` (ids only, never text).
pub fn line(at: i64, items: &[Item]) -> String {
    let open: Vec<String> = items.iter().filter(|i| i.status != "ok" && IDS.contains(&i.id.as_str())).map(|i| format!("{}:{}", i.id, i.status)).collect();
    format!("doctor {at} {}", if open.is_empty() { "ok".to_string() } else { open.join(" ") })
}

/// The text after `doctor ` of a heartbeat line: its time and the ids not ok; `None` when invalid.
pub fn parse(text: &str) -> Option<(i64, Vec<(String, String)>)> {
    let mut w = text.split(' ');
    let at = w.next()?.parse().ok()?;
    let rest: Vec<&str> = w.collect();
    if rest == ["ok"] { return Some((at, vec![])); }
    if rest.is_empty() { return None; }
    rest.iter().map(|p| {
        let (id, s) = p.split_once(':')?;
        (IDS.contains(&id) && matches!(s, "gap" | "unknown")).then(|| (id.to_string(), s.to_string()))
    }).collect::<Option<Vec<_>>>().map(|v| (at, v))
}

/// One fix line for check `id` on `host`.
pub fn fix(id: &str, repo: &str, host: &str) -> String {
    match TOKEN.iter().find(|t| t.1 == id) {
        Some((_, _, grant)) => format!("owner: grant the board token of {host} {grant} for {repo}"),
        None => match id {
            "version" => format!("run `taskq update` on {host}"),
            "reports" => format!("run `taskq service status` on {host}; restart its service if collection stays incomplete"),
            _ => format!("run `taskq doctor` on {host} and fix the named service error"),
        },
    }
}

/// One `--hosts` row: host, version, `ok|gap|unknown`, the open ids, the reason of an unknown and one fix line.
pub fn host_row(h: &crate::snapshot::Host, repo: &str, now: i64, ttl: i64) -> Value {
    let unknown = |reason: String, fix: String| json!({"host": h.name, "version": h.version, "state": "unknown", "gaps": [], "reason": reason, "fix": fix});
    let age = now - h.updated;
    if !crate::snapshot::live(age, ttl) {
        return unknown(format!("heartbeat {} old; host unreachable", crate::age(age)), format!("start the service on {}: `taskq service install`", h.name));
    }
    let Some((at, open)) = h.doctor.as_deref().and_then(parse) else {
        return unknown(format!("version {} runs no health checks", h.version), format!("run `taskq update` on {}", h.name));
    };
    if now - at > STALE {
        return unknown(format!("health checks {} old", crate::age(now - at)), format!("run `taskq service restart` on {}", h.name));
    }
    let state = if open.is_empty() { "ok" } else if open.iter().any(|(_, s)| s == "gap") { "gap" } else { "unknown" };
    let gaps: Vec<String> = open.iter().map(|(id, s)| if s == "gap" { id.clone() } else { format!("{id} (unknown)") }).collect();
    let fixes: Vec<String> = open.iter().map(|(id, _)| fix(id, repo, &h.name)).collect();
    json!({"host": h.name, "version": h.version, "state": state, "gaps": gaps, "reason": null, "fix": (!fixes.is_empty()).then(|| fixes.join("; "))})
}

/// The `--hosts` worker rows: each doing or review attempt with its host, runtime and health.
pub fn worker_rows(s: &crate::snapshot::Snapshot) -> Vec<Value> {
    s.tasks.iter().filter(|r| matches!(r.task.state, State::Doing | State::Review) && r.task.attempt.is_some()).map(|r| {
        let host = crate::service::host_of(&r.task).unwrap_or("unknown");
        let rt = crate::service::rt_of(&r.task).map_or("unknown", |(rt, _)| rt);
        let warning = r.heartbeat["warning"].as_str();
        let account = match s.hosts.iter().find(|h| h.name == host) {
            None => "unknown: host heartbeat missing".to_string(),
            Some(h) if h.logins.iter().any(|l| l == rt) => format!("login {rt} off"),
            Some(h) => h.limits.iter().find(|l| l.split(' ').next() == Some(rt)).map_or("ok".to_string(), |l| format!("limit {l}")),
        };
        json!({"host": host, "task": r.n, "attempt": r.task.attempt, "runtime": rt, "alive": r.heartbeat["freshness"] == "fresh",
            "stalled": warning.is_some(), "reason": warning, "account": account})
    }).collect()
}

/// Left-aligned columns, two spaces apart.
fn table(rows: &[Vec<String>]) -> Vec<String> {
    let width: Vec<usize> = (0..rows.first().map_or(0, Vec::len)).map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0)).collect();
    rows.iter().map(|r| r.iter().zip(&width).map(|(cell, w)| format!("{cell:<w$}")).collect::<Vec<_>>().join("  ").trim_end().to_string()).collect()
}

/// `taskq doctor --hosts` lines of one board: the host table, then the worker table.
pub fn hosts_lines(repo: &str, hosts: &[Value], workers: &[Value]) -> Vec<String> {
    let text = |v: &Value| v.as_str().unwrap_or("-").to_string();
    let mut rows = vec![["host", "version", "state", "gaps", "fix"].map(String::from).to_vec()];
    rows.extend(hosts.iter().map(|h| {
        let gaps = h["gaps"].as_array().filter(|g| !g.is_empty()).map(|g| g.iter().map(text).collect::<Vec<_>>().join(", "));
        vec![text(&h["host"]), text(&h["version"]), text(&h["state"]), gaps.unwrap_or_else(|| text(&h["reason"])), text(&h["fix"])]
    }));
    let mut out = vec![format!("board {repo}")];
    out.extend(table(&rows).into_iter().map(|l| format!("  {l}")));
    if hosts.is_empty() { out.push("  no host heartbeat on this board".into()); }
    let yes = |v: &Value| if v == true { "yes" } else { "no" }.to_string();
    let mut rows = vec![["worker", "task", "attempt", "runtime", "alive", "stalled", "account"].map(String::from).to_vec()];
    rows.extend(workers.iter().map(|w| vec![text(&w["host"]), format!("#{}", w["task"]), w["attempt"].to_string(), text(&w["runtime"]),
        yes(&w["alive"]), w["reason"].as_str().map_or_else(|| "no".to_string(), |r| format!("yes: {r}")), text(&w["account"])]));
    if workers.is_empty() { out.push("  no running worker".into()); } else { out.extend(table(&rows).into_iter().map(|l| format!("  {l}"))); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Host;

    // Chain: § 6 host setup -> `taskq doctor` token checks (#2194 A1, A4). Risk: a fine-grained token without
    // Contents: Read let the board work while publication and reports failed with raw 403 errors and no named cause.
    // Protection: a 403 on the contents read is a `token-contents` gap naming the permission, the status and one fix.
    #[test]
    fn doctor_names_a_token_without_contents_read() {
        let items = token_items("o/r", &[("Issues", Probe::Ok), ("Contents", Probe::Gap("HTTP 403".into())), ("Pull requests", Probe::Ok)]);
        let c = items.iter().find(|i| i.id == "token-contents").unwrap();
        assert_eq!((c.status, c.text.as_str()), ("gap", "token lacks Contents: Read on o/r (HTTP 403)"));
        assert!(c.fix.as_deref().unwrap().contains("Contents: Read"), "{:?}", c.fix);
        assert!(items.iter().filter(|i| i.id != "token-contents").all(|i| i.status == "ok"));
        assert_eq!(line(7, &items), "doctor 7 token-contents:gap");
        assert_eq!(http_status("GET /commits?per_page=1: HTTP 403 Resource not accessible"), Some(403));
        assert!(token_items("o/r", &[]).iter().all(|i| i.status == "unknown"), "an unsupported board is unknown, never ok");
    }

    // Chain: § 8 update -> `taskq doctor` version check (#2194 A1, A4). Risk: a host stayed on an old release with no
    // sign in doctor. Protection: behind the latest release is a gap with `taskq update`; an unread release is unknown.
    #[test]
    fn doctor_flags_an_outdated_version() {
        let v = version_item("4.6.58", &Ok("4.6.60".into()));
        assert_eq!((v.status, v.text.as_str(), v.fix.as_deref()), ("gap", "installed 4.6.58 is behind the latest release 4.6.60", Some("run `taskq update`")));
        assert_eq!(version_item("4.6.60", &Ok("4.6.60".into())).status, "ok");
        assert_eq!(version_item("4.7.0-dev.1", &Ok("4.6.60".into())).status, "ok", "a newer dev build is not behind");
        assert_eq!(version_item("4.6.58", &Err("no network".into())).status, "unknown");
        let r = reports_item(&json!({"complete": false, "reason": "Report collection is stale; waiting for the service"}));
        assert_eq!((r.status, r.text.as_str()), ("gap", "report collection incomplete: Report collection is stale; waiting for the service"));
        assert_eq!(reports_item(&json!({"complete": true})).status, "ok");
        let e = error_item(Ok(Some(json!({"error": "board unavailable: HTTP 502\ndetail"}))), Some(5), 120);
        assert_eq!((e.status, e.text.as_str()), ("gap", "last service pass failed: board unavailable: HTTP 502"));
        assert_eq!(error_item(Ok(Some(json!({"error": null}))), Some(5), 120).status, "ok");
        assert_eq!(error_item(Ok(Some(json!({"error": null}))), Some(500), 120).status, "unknown", "a stale status is unknown");
        assert_eq!(error_item(Ok(None), None, 120).status, "unknown");
    }

    // Chain: § 4 step 3 heartbeat `doctor` line -> `taskq doctor --hosts` (#2194 A2, A4). Risk: a dead or old host
    // read as healthy, or a hand-written heartbeat injected text into the table. Protection: a stale heartbeat reads
    // unknown `host unreachable`, a host without a valid line reads unknown with its version, unknown ids are refused.
    #[test]
    fn doctor_hosts_reads_unreachable_and_old_hosts_unknown() {
        let host = |name: &str, updated, version: &str, doctor: Option<&str>| Host { name: name.into(), version: version.into(), updated, doctor: doctor.map(String::from), ..Default::default() };
        let now = 10_000;
        let gone = host_row(&host("mac", now - 1500, "4.6.60", Some("9000 ok")), "o/r", now, 120);
        assert_eq!((gone["state"].as_str(), gone["reason"].as_str()), (Some("unknown"), Some("heartbeat 25m old; host unreachable")));
        let old = host_row(&host("lin", now - 10, "4.6.50", None), "o/r", now, 120);
        assert_eq!((old["state"].as_str(), old["reason"].as_str()), (Some("unknown"), Some("version 4.6.50 runs no health checks")));
        let gap = host_row(&host("win", now - 10, "4.6.58", Some("9900 token-contents:gap version:gap")), "o/r", now, 120);
        assert_eq!(gap["state"], "gap");
        assert_eq!(gap["gaps"], json!(["token-contents", "version"]));
        assert_eq!(gap["fix"], "owner: grant the board token of win Contents: Read for o/r; run `taskq update` on win");
        assert_eq!(host_row(&host("ok", now - 10, "4.6.60", Some("9900 ok")), "o/r", now, 120)["state"], "ok");
        let lapsed = host_row(&host("ok", now - 10, "4.6.60", Some("100 ok")), "o/r", now, 120);
        assert_eq!(lapsed["state"], "unknown", "a result the service stopped refreshing is unknown");
        assert_eq!(parse("9900 rm-rf:gap"), None, "only known ids");
        assert_eq!(parse("9900 version:fine"), None, "only known statuses");
        assert_eq!(parse("9900"), None);
        let lines = hosts_lines("o/r", &[gone, gap], &[json!({"host": "win", "task": 7, "attempt": 42, "runtime": "claude", "alive": true, "stalled": false, "reason": null, "account": "ok"})]);
        assert!(lines.iter().any(|l| l.contains("mac") && l.contains("unknown") && l.contains("host unreachable")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("  win ") && l.contains("#7") && l.contains("claude") && l.contains("yes")), "{lines:#?}");
    }

    // Chain: § 4 heartbeat lease and execution health -> `taskq doctor --hosts` workers (#2194 A3). Risk: a PM read a
    // stalled worker or one on a logged-out account as running. Protection: each live attempt is listed with alive from
    // the lease, stalled from the execution warning and the host's `limit`/`login off` line for its runtime.
    #[test]
    fn doctor_hosts_lists_workers_with_health() {
        use crate::snapshot::{Row, Snapshot};
        let row = |n, rt: &str, host: &str, fresh: bool, warning: Option<&str>| Row { n, task: crate::core::Task { state: State::Doing, attempt: Some(n * 10),
            session: Some(format!("{rt}:s{n} {host}")), ..Default::default() },
            heartbeat: json!({"freshness": if fresh { "fresh" } else { "stale" }, "warning": warning}), ..Default::default() };
        let idle = Row { n: 9, task: crate::core::Task { state: State::Ready, ..Default::default() }, ..Default::default() };
        let s = Snapshot { source: "board", observed: 0, reason: None, board: Value::Null, hosts: vec![
            Host { name: "win".into(), logins: vec!["codex".into()], limits: vec!["claude until 2026-11-02T10:00:00Z".into()], ..Default::default() }],
            tasks: vec![row(1, "claude", "win", true, None), row(2, "codex", "win", true, Some("execution progress observation missing or stale")),
                row(3, "claude", "mac", false, None), idle] };
        let w = worker_rows(&s);
        assert_eq!(w.len(), 3, "only live attempts");
        assert_eq!((&w[0]["alive"], &w[0]["stalled"], &w[0]["account"]), (&json!(true), &json!(false), &json!("limit claude until 2026-11-02T10:00:00Z")));
        assert_eq!((&w[1]["stalled"], &w[1]["account"], &w[1]["runtime"]), (&json!(true), &json!("login codex off"), &json!("codex")));
        assert_eq!((&w[2]["alive"], &w[2]["account"]), (&json!(false), &json!("unknown: host heartbeat missing")));
    }
}
