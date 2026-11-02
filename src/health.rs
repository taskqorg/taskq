//! Health checks (#2194, [host service](../docs/design/host-service.md#health-checks-2194)): per board the token
//! permissions taskq needs, installed vs latest release, report collection and the last service error, each with one
//! fix; the heartbeat `doctor` line that publishes them; the board request that asks every host to run them now and
//! the heartbeat `doctor-answer` line that answers it; and the `taskq doctor --hosts` host and worker rows. Nothing here
//! prints a token, a path or a raw response body into a heartbeat or the request.

use std::path::Path;

use serde_json::{json, Value};

use crate::check::{Item, Probe};
use crate::core::State;
use crate::snapshot::Host;

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

/// Title of the one open board issue that carries the `taskq doctor --hosts` request. It is labelled `taskq-host` but is
/// no heartbeat (`service::is_host`), so hosts without health checks ignore it.
pub const REQUEST_TITLE: &str = "taskq doctor";
/// Seconds `taskq doctor --hosts` waits for answers, and between two fresh reads of the unanswered heartbeats.
pub const ASK_WAIT: u64 = 300;
pub const ASK_POLL: u64 = 20;

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
        Some(Probe::Ok) => Item::ok(*id, format!("token grants {grant} on {repo}")),
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

/// `reports`: the cached report collection ([`crate::reports::cached`]). A collection is complete while single reports
/// stay unreadable (#2195), so a report row with a reason is a gap too.
pub fn reports_item(cached: &Value) -> Item {
    if cached["complete"] != true {
        return Item::gap("reports", format!("report collection incomplete: {}", cached["reason"].as_str().unwrap_or("reason unknown")),
            "run `taskq service status`; if collection stays incomplete, run `taskq service restart`");
    }
    let failed: Vec<&Value> = cached["reports"].as_array().into_iter().chain(cached["history"].as_array()).flatten()
        .filter(|r| r["kind"] == "report" && r["reason"].is_string()).collect();
    let Some(first) = failed.first() else { return Item::ok("reports", "report collection complete"); };
    let reason = short(first["reason"].as_str().unwrap_or_default());
    let fix = if failed.iter().any(|r| r["reason"].as_str().is_some_and(|e| matches!(http_status(e), Some(401 | 403)))) {
        "owner: grant the board token Contents: Read, then run `taskq doctor` again"
    } else { "open the named report with `taskq results <n>` and follow up or reopen its task" };
    Item::gap("reports", format!("{} report(s) unreadable, first #{}: {reason}", failed.len(), first["n"]), fix)
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

/// A request id: 1 to 32 ASCII letters or digits.
fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The id of the open, trusted `taskq doctor --hosts` request among `issues`, newest first; `None` without one.
pub fn request_of(issues: &[crate::board::Issue]) -> Option<String> {
    issues.iter().filter(|i| i.trusted && i.title == REQUEST_TITLE && i.labels.iter().any(|l| l == crate::service::HOST_LABEL))
        .max_by_key(|i| (i.updated, i.number))
        .and_then(|i| i.body.lines().next()?.strip_prefix("request ").filter(|id| valid_id(id)).map(str::to_string))
}

/// The request issue's body for request `id`.
pub fn request_body(id: &str) -> String {
    format!("request {id}\n\n`taskq doctor --hosts` asks every TaskQ host service to run its health checks now; each answers in its heartbeat \
        with `doctor-answer {id}`. Keep this issue open: the next request edits it.")
}

/// The heartbeat line that answers request `id`.
pub fn answer_line(id: &str) -> String {
    format!("doctor-answer {id}")
}

/// A posted request: its id, or why none went out, and the seconds waited for answers.
pub struct Asked { pub id: Result<String, String>, pub waited: u64 }

/// Whether host `h` can answer: its heartbeat is live and carries health checks.
fn answers(h: &Host, now: i64, ttl: i64) -> bool {
    crate::snapshot::live(now - h.updated, ttl) && h.doctor.as_deref().and_then(parse).is_some()
}

/// `taskq doctor --hosts` on one board: asks every live host that runs health checks to run them now through the
/// request issue, then reads each unanswered heartbeat fresh every `poll` seconds until all answer or `wait` passes.
pub fn ask(b: &dyn crate::board::Board, hosts: &mut [Host], now: i64, ttl: i64, wait: u64, poll: u64, sleep: &dyn Fn(u64)) -> Asked {
    if !hosts.iter().any(|h| answers(h, now, ttl)) { return Asked { id: Err("no live host runs health checks".into()), waited: 0 }; }
    let id: String = crate::runtime::uuid().chars().filter(char::is_ascii_alphanumeric).take(12).collect();
    let posted = b.issues(None).and_then(|issues| {
        match issues.iter().filter(|i| i.trusted && i.title == REQUEST_TITLE).max_by_key(|i| i.number) {
            Some(i) => b.edit(i.number, &request_body(&id)),
            None => b.create(REQUEST_TITLE, &request_body(&id), &[crate::service::HOST_LABEL.into()]).map(|_| ()),
        }
    });
    if let Err(e) = posted { return Asked { id: Err(format!("request not posted: {}", short(&e))), waited: 0 }; }
    let pending = |hosts: &[Host]| hosts.iter().enumerate().filter(|(_, h)| answers(h, now, ttl) && h.answer.as_deref() != Some(id.as_str())).map(|(i, _)| i).collect::<Vec<_>>();
    let mut waited = 0;
    while waited < wait {
        let open = pending(hosts);
        if open.is_empty() { break; }
        sleep(poll);
        waited += poll;
        for i in open {
            // one fresh read per unanswered heartbeat: the issue list may lag minutes behind
            if let Ok(issue) = b.issue(hosts[i].issue).and_then(|i| if i.trusted && crate::service::is_host(&i) { Ok(i) } else { Err("not a trusted heartbeat".into()) }) {
                hosts[i] = crate::snapshot::host(&issue, now + waited as i64, ttl);
            }
        }
    }
    Asked { id: Ok(id), waited }
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

/// One `--hosts` row: host, version, `ok|gap|unknown`, the open ids, the reason of an unknown and one fix line. With a
/// posted request, a host that did not answer it reads unknown; without one, the last published result stands.
pub fn host_row(h: &Host, repo: &str, now: i64, ttl: i64, asked: Option<&Asked>) -> Value {
    let unknown = |reason: String, fix: String| json!({"host": h.name, "version": h.version, "state": "unknown", "gaps": [], "reason": reason, "fix": fix});
    let age = now - h.updated;
    if !crate::snapshot::live(age, ttl) {
        return unknown(format!("heartbeat {} old; host unreachable", crate::age(age)), format!("start the service on {}: `taskq service install`", h.name));
    }
    let Some((at, open)) = h.doctor.as_deref().and_then(parse) else {
        return unknown(format!("version {} runs no health checks", h.version), format!("run `taskq update` on {}", h.name));
    };
    match asked.map(|a| &a.id) {
        Some(Ok(id)) if h.answer.as_deref() != Some(id.as_str()) => return unknown(
            format!("no answer to the request within {}s; host busy or unreachable", asked.map_or(0, |a| a.waited)),
            format!("run `taskq service status` on {}, then `taskq doctor --hosts` again", h.name)),
        Some(Ok(_)) => {}
        _ if now - at > STALE => return unknown(format!("health checks {} old", crate::age(now - at)), format!("run `taskq service restart` on {}", h.name)),
        _ => {}
    }
    let state = if open.is_empty() { "ok" } else if open.iter().any(|(_, s)| s == "gap") { "gap" } else { "unknown" };
    let gaps: Vec<String> = open.iter().map(|(id, s)| if s == "gap" { id.clone() } else { format!("{id} (unknown)") }).collect();
    let fixes: Vec<String> = open.iter().map(|(id, _)| fix(id, repo, &h.name)).collect();
    json!({"host": h.name, "version": h.version, "state": state, "gaps": gaps, "reason": null, "fix": (!fixes.is_empty()).then(|| fixes.join("; "))})
}

/// The `--hosts` worker rows: each doing or review attempt with its host, runtime and health. Only fresh evidence
/// reads healthy: alive needs the fresh lease and a fresh executor observation, the account an active unlimited account
/// line or fresh runtime output on a live heartbeat; anything else is unknown with its reason (R12).
pub fn worker_rows(s: &crate::snapshot::Snapshot, ttl: i64) -> Vec<Value> {
    s.tasks.iter().filter(|r| matches!(r.task.state, State::Doing | State::Review) && r.task.attempt.is_some()).map(|r| {
        let host = crate::service::host_of(&r.task).unwrap_or("unknown");
        let rt = crate::service::rt_of(&r.task).map_or("unknown", |(rt, _)| rt);
        let h = &r.heartbeat;
        let executor = h["executor"]["state"].as_str().unwrap_or("unknown");
        let alive = if h["freshness"] != "fresh" {
            Err(h["reason"].as_str().unwrap_or("attempt lease unknown").to_string())
        } else if matches!(executor, "active" | "waiting" | "awaiting acceptance") {
            Ok(())
        } else {
            Err(format!("executor {executor}: {}", h["executor"]["reason"].as_str().unwrap_or("no current observation")))
        };
        let warning = h["warning"].as_str().filter(|_| alive.is_ok());
        let output = alive.is_ok() && warning.is_none() && h["progress"]["status"] == "active";
        let account = match s.hosts.iter().find(|x| x.name == host) {
            None => "unknown: host heartbeat missing".to_string(),
            Some(x) if !crate::snapshot::live(s.observed - x.updated, ttl) => format!("unknown: host heartbeat {} old", crate::age(s.observed - x.updated)),
            Some(x) if x.logins.iter().any(|l| l == rt) => format!("login {rt} off"),
            Some(x) => match x.limits.iter().find(|l| l.split(' ').next() == Some(rt)) {
                Some(l) => format!("limit {l}"),
                None => {
                    let lines: Vec<String> = x.accounts.iter().map(|l| format!("account {l}")).collect();
                    let rows = crate::runtime::accounts::heartbeat_rows(&x.name, &lines.join("\n"));
                    match rows.iter().find(|a| a.runtime == rt && a.active) {
                        Some(a) if a.limited.is_some() => format!("limit account {} {}", a.label, a.reset.as_deref().unwrap_or("reset unknown")),
                        Some(a) if a.enabled => format!("ok (account {})", a.label),
                        Some(a) => format!("unknown: active account {} is off", a.label),
                        None if output => "ok (runtime output fresh)".to_string(),
                        None => format!("unknown: no fresh {rt} account facts"),
                    }
                }
            },
        };
        json!({"host": host, "task": r.n, "attempt": r.task.attempt, "runtime": rt, "alive": alive.is_ok().then_some(true),
            "stalled": alive.is_ok().then_some(warning.is_some()), "alive_reason": alive.err(), "reason": warning, "account": account})
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
    let alive = |w: &Value| if w["alive"] == true { "yes".to_string() } else { format!("unknown: {}", text(&w["alive_reason"])) };
    let stalled = |w: &Value| match w["stalled"].as_bool() {
        Some(true) => format!("yes: {}", w["reason"].as_str().unwrap_or("reason unknown")),
        Some(false) => "no".to_string(),
        None => "unknown".to_string(),
    };
    let mut rows = vec![["worker", "task", "attempt", "runtime", "alive", "stalled", "account"].map(String::from).to_vec()];
    rows.extend(workers.iter().map(|w| vec![text(&w["host"]), format!("#{}", w["task"]), w["attempt"].to_string(), text(&w["runtime"]),
        alive(w), stalled(w), text(&w["account"])]));
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

    // Chain: reports collection (#2195) -> `taskq doctor` reports check (#2194 A1). Risk: one unreadable report kept
    // the collection complete, so doctor read ok while the owner's screen showed a raw 403 for that report.
    // Protection: a report row with a reason is a gap naming the count, the first task and one fix; a 403 asks for
    // Contents: Read, task rows without a hand-in stay ok.
    #[test]
    fn doctor_reports_unreadable_rows_of_a_complete_collection() {
        let ok = reports_item(&json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "task", "n": 5, "reason": "No accepted hand-in recorded; result details unknown"}]}));
        assert_eq!(ok.status, "ok");
        let bad = reports_item(&json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "report", "n": 7, "reason": "#7 source: GET /contents/x: HTTP 403 Resource not accessible"},
                {"kind": "report", "n": 8, "missing": true, "reason": "#8 source: HTTP 404"}]}));
        assert_eq!((bad.status, bad.text.as_str()), ("gap", "2 report(s) unreadable, first #7: #7 source: GET /contents/x: HTTP 403 Resource not accessible"));
        assert_eq!(bad.fix.as_deref(), Some("owner: grant the board token Contents: Read, then run `taskq doctor` again"));
        let gone = reports_item(&json!({"complete": true, "reports": [{"kind": "report", "n": 8, "missing": true, "reason": "#8 source: HTTP 404"}]}));
        assert_eq!((gone.status, gone.fix.as_deref()), ("gap", Some("open the named report with `taskq results <n>` and follow up or reopen its task")));
    }

    // Chain: § 4 step 3 heartbeat `doctor` line -> `taskq doctor --hosts` (#2194 A2, A4). Risk: a dead or old host
    // read as healthy, or a hand-written heartbeat injected text into the table. Protection: a stale heartbeat reads
    // unknown `host unreachable`, a host without a valid line reads unknown with its version, unknown ids are refused.
    #[test]
    fn doctor_hosts_reads_unreachable_and_old_hosts_unknown() {
        let host = |name: &str, updated, version: &str, doctor: Option<&str>| Host { name: name.into(), version: version.into(), updated, doctor: doctor.map(String::from), ..Default::default() };
        let now = 10_000;
        let gone = host_row(&host("mac", now - 1500, "4.6.60", Some("9000 ok")), "o/r", now, 120, None);
        assert_eq!((gone["state"].as_str(), gone["reason"].as_str()), (Some("unknown"), Some("heartbeat 25m old; host unreachable")));
        let old = host_row(&host("lin", now - 10, "4.6.50", None), "o/r", now, 120, None);
        assert_eq!((old["state"].as_str(), old["reason"].as_str()), (Some("unknown"), Some("version 4.6.50 runs no health checks")));
        let gap = host_row(&host("win", now - 10, "4.6.58", Some("9900 token-contents:gap version:gap")), "o/r", now, 120, None);
        assert_eq!(gap["state"], "gap");
        assert_eq!(gap["gaps"], json!(["token-contents", "version"]));
        assert_eq!(gap["fix"], "owner: grant the board token of win Contents: Read for o/r; run `taskq update` on win");
        assert_eq!(host_row(&host("ok", now - 10, "4.6.60", Some("9900 ok")), "o/r", now, 120, None)["state"], "ok");
        let lapsed = host_row(&host("ok", now - 10, "4.6.60", Some("100 ok")), "o/r", now, 120, None);
        assert_eq!(lapsed["state"], "unknown", "a result the service stopped refreshing is unknown");
        assert_eq!(parse("9900 rm-rf:gap"), None, "only known ids");
        assert_eq!(parse("9900 version:fine"), None, "only known statuses");
        assert_eq!(parse("9900"), None);
        let lines = hosts_lines("o/r", &[gone, gap], &[json!({"host": "win", "task": 7, "attempt": 42, "runtime": "claude", "alive": true, "stalled": false, "reason": null, "account": "ok (account work)"})]);
        assert!(lines.iter().any(|l| l.contains("mac") && l.contains("unknown") && l.contains("host unreachable")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("  win ") && l.contains("#7") && l.contains("claude") && l.contains("yes")), "{lines:#?}");
    }

    // Chain: `taskq doctor --hosts` request issue -> service pass -> heartbeat `doctor-answer` (#2194 A2). Risk: the
    // table re-read cached heartbeats, so a host whose checks had not run since a token change read healthy, and a
    // host that never ran the request read as checked. Protection: the request goes out on the board as one reused
    // `taskq doctor` issue no heartbeat parser reads as a host; a host that answers reads its fresh result, one that
    // does not answer within the wait reads unknown with the wait, stale and old hosts are not waited for.
    #[test]
    fn doctor_hosts_requests_every_host_and_reads_a_silent_host_unknown() {
        use crate::board::{Board, Fake};
        let b = Fake::default();
        b.advance(10_000);
        let beat = |name: &str, body: &str| b.create(&format!("taskq host {name}"), body, &[crate::service::HOST_LABEL.into()]).unwrap();
        let (win, lin) = (beat("win", "version 4.6.61\ndoctor 9990 ok"), beat("lin", "version 4.6.61\ndoctor 9990 ok"));
        beat("old", "version 4.6.50");
        let ttl = 120;
        let read = |b: &Fake| b.issues(None).unwrap().iter().filter(|i| crate::service::is_host(i)).map(|i| crate::snapshot::host(i, b.now().unwrap(), ttl)).collect::<Vec<_>>();
        let mut hosts = read(&b);
        hosts.push(Host { name: "mac".into(), version: "4.6.61".into(), updated: 1_000, doctor: Some("900 ok".into()), ..Default::default() });
        let now = b.now().unwrap();
        // win's service sees the request on its next pass and answers with a fresh result; lin never answers
        let slept = std::cell::Cell::new(0);
        let answer = |_| {
            slept.set(slept.get() + 1);
            let id = request_of(&b.issues(None).unwrap()).expect("request on the board");
            b.edit(win, &format!("version 4.6.61\ndoctor {now} token-pulls:gap\n{}", answer_line(&id))).unwrap();
        };
        let asked = ask(&b, &mut hosts, now, ttl, 60, 20, &answer);
        let id = asked.id.clone().unwrap();
        assert_eq!((asked.waited, slept.get()), (60, 3), "waits the full time for the silent live host");
        let requests: Vec<_> = b.issues(None).unwrap().into_iter().filter(|i| i.title == REQUEST_TITLE).collect();
        assert_eq!(requests.len(), 1);
        assert!(!crate::service::is_host(&requests[0]), "an old host's heartbeat parser ignores the request");
        assert_eq!(request_of(&requests), Some(id.clone()));
        let rows: Vec<Value> = hosts.iter().map(|h| host_row(h, "o/r", now + 60, ttl, Some(&asked))).collect();
        let row = |name: &str| rows.iter().find(|r| r["host"] == name).unwrap().clone();
        assert_eq!((row("win")["state"].as_str(), &row("win")["gaps"]), (Some("gap"), &json!(["token-pulls"])), "the answer's fresh result");
        assert_eq!((row("lin")["state"].as_str(), row("lin")["reason"].as_str()), (Some("unknown"), Some("no answer to the request within 60s; host busy or unreachable")));
        assert_eq!(row("mac")["reason"], "heartbeat 2h old; host unreachable");
        assert_eq!(row("old")["reason"], "version 4.6.50 runs no health checks");
        // the next request edits the same issue and stops waiting once every live host answered
        b.edit(lin, &format!("version 4.6.61\ndoctor {now} ok")).unwrap();
        let mut hosts = read(&b);
        let both = |_| {
            let id = request_of(&b.issues(None).unwrap()).unwrap();
            for n in [win, lin] { b.edit(n, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap(); }
        };
        let again = ask(&b, &mut hosts, b.now().unwrap(), ttl, 300, 20, &both);
        assert_eq!(again.waited, 20, "no wait once all answered");
        assert_eq!(b.issues(None).unwrap().iter().filter(|i| i.title == REQUEST_TITLE).count(), 1, "one reused request issue");
        assert_ne!(again.id.unwrap(), id, "every run is a new request");
        let mut none = vec![Host { name: "mac".into(), updated: 1_000, doctor: Some("900 ok".into()), ..Default::default() }];
        assert!(ask(&b, &mut none, now, ttl, 60, 20, &|_| panic!("nothing to wait for")).id.is_err(), "no live host: no request");
    }

    // Chain: § 4 heartbeat lease, execution observation and account lines -> `taskq doctor --hosts` workers (#2194
    // A3). Risk: a held lease read as a live worker and a heartbeat with no account facts read as account ok, so a
    // PM trusted a worker whose runtime had died or logged out. Protection: alive needs the fresh lease and a fresh
    // executor; the account is ok only from an active unlimited account line or fresh runtime output on a live
    // heartbeat; `login off` and limits are named; anything else is unknown with its reason.
    #[test]
    fn doctor_hosts_lists_workers_with_health() {
        use crate::snapshot::{Row, Snapshot};
        let now = 10_000;
        let row = |n, rt: &str, host: &str, heartbeat: Value| Row { n, task: crate::core::Task { state: State::Doing, attempt: Some(n * 10),
            session: Some(format!("{rt}:s{n} {host}")), ..Default::default() }, heartbeat, ..Default::default() };
        let beat = |executor: &str, progress: &str, warning: Option<&str>| json!({"freshness": "fresh", "executor": {"state": executor, "reason": "observed"},
            "progress": {"status": progress}, "warning": warning});
        let idle = Row { n: 9, task: crate::core::Task { state: State::Ready, ..Default::default() }, ..Default::default() };
        let s = Snapshot { source: "board", observed: now, reason: None, board: Value::Null, hosts: vec![
            Host { name: "win".into(), updated: now - 5, logins: vec!["codex".into()], limits: vec!["claude until 2026-11-02T10:00:00Z".into()], ..Default::default() },
            Host { name: "lin".into(), updated: now - 5, accounts: vec!["claude work on active".into(), "codex home on".into()], ..Default::default() },
            Host { name: "fra".into(), updated: now - 5, ..Default::default() },
            Host { name: "old".into(), updated: now - 900, accounts: vec!["claude work on active".into()], ..Default::default() }],
            tasks: vec![row(1, "claude", "win", beat("active", "active", None)),
                row(2, "codex", "win", beat("active", "overdue", Some("execution progress observation missing or stale"))),
                row(3, "claude", "mac", json!({"freshness": "stale", "reason": "owning host heartbeat stale or time unknown"})),
                row(4, "claude", "lin", beat("unknown", "active", Some("current executor unknown; inspect execution health"))),
                row(5, "codex", "lin", beat("active", "quiet", None)),
                row(6, "claude", "fra", beat("active", "quiet", None)),
                row(7, "claude", "fra", beat("waiting", "active", None)),
                row(8, "claude", "old", beat("active", "active", None)), idle] };
        let w = worker_rows(&s, 120);
        assert_eq!(w.len(), 8, "only live attempts");
        let get = |n: u64| w.iter().find(|r| r["task"] == n).unwrap();
        assert_eq!((&get(1)["alive"], &get(1)["stalled"], &get(1)["account"]), (&json!(true), &json!(false), &json!("limit claude until 2026-11-02T10:00:00Z")));
        assert_eq!((&get(2)["stalled"], &get(2)["account"], &get(2)["runtime"]), (&json!(true), &json!("login codex off"), &json!("codex")));
        assert_eq!((&get(3)["alive"], &get(3)["alive_reason"], &get(3)["stalled"], &get(3)["account"]), (&Value::Null,
            &json!("owning host heartbeat stale or time unknown"), &Value::Null, &json!("unknown: host heartbeat missing")), "a stale lease is no live worker");
        assert_eq!((&get(4)["alive"], &get(4)["alive_reason"], &get(4)["account"]), (&Value::Null, &json!("executor unknown: observed"), &json!("ok (account work)")),
            "a fresh lease without a fresh executor is unknown, never alive");
        assert_eq!(get(5)["account"], "unknown: no fresh codex account facts", "an inactive account line is no evidence");
        assert_eq!(get(6)["account"], "unknown: no fresh claude account facts", "no account line and no output: unknown, never ok");
        assert_eq!((&get(7)["alive"], &get(7)["account"]), (&json!(true), &json!("ok (runtime output fresh)")));
        assert_eq!(get(8)["account"], "unknown: host heartbeat 15m old", "a stale heartbeat's account line is no evidence");
        let lines = hosts_lines("o/r", &[], &w);
        assert!(lines.iter().any(|l| l.contains("#3") && l.contains("unknown: owning host heartbeat stale")), "{lines:#?}");
    }
}
