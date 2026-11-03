//! Health checks (#2194, [host service](../docs/design/host-service.md#health-checks-2194)): per board the token
//! permissions taskq needs, installed vs latest release, report collection, the last service error and what a worker
//! launch needs on the host (Claude trust of the root, agentq hooks and data folder; #2300), each with one fix; the
//! heartbeat `doctor` line that publishes them; the board request that asks every host to run them now and the
//! heartbeat `doctor-answer` line that answers it; and the `taskq doctor --hosts` host and worker rows. Nothing here
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
/// The GitLab token scopes and project role of each permission of [`TOKEN`] (#2246): `api` writes issues and merge
/// requests, `read_api` or `read_repository` reads files; Reporter (20) reads the repository, Developer (30) opens
/// merge requests.
const GITLAB: [(&str, &[&str], u64, &str); 3] = [
    ("Issues", &["api"], 20, "scope api and role Reporter or higher"),
    ("Contents", &["api", "read_api", "read_repository"], 20, "scope read_api, read_repository or api and role Reporter or higher"),
    ("Pull requests", &["api"], 30, "scope api and role Developer or higher"),
];

/// The grant the owner gives for permission `name` on a GitHub or GitLab board.
fn grant(name: &str, gitlab: bool) -> &'static str {
    if gitlab { GITLAB.iter().find(|g| g.0 == name).map_or("", |g| g.3) } else { TOKEN.iter().find(|t| t.0 == name).map_or("", |t| t.2) }
}

/// GitLab `access` (#2246): the token's own scopes and state (`GET /personal_access_tokens/self`) and the token user's
/// project role (`GET /projects/:id` permissions), one probe per permission of [`TOKEN`]. Any unread fact is unknown
/// with its cause, never ok.
pub fn gitlab_access(token: &Result<Value, String>, project: &Result<Value, String>) -> Vec<(&'static str, Probe)> {
    let level = project.as_ref().map(|p| ["project_access", "group_access"].iter().filter_map(|k| p["permissions"][k]["access_level"].as_u64()).max().unwrap_or(0));
    GITLAB.iter().map(|(name, scopes, min, _)| (*name, match token {
        Err(e) => Probe::Unknown(format!("token scopes unread: {e}")),
        Ok(t) if t["active"] == false || t["revoked"] == true => Probe::Gap("token inactive, revoked or expired".into()),
        Ok(t) => {
            let have: Vec<&str> = t["scopes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !scopes.iter().any(|s| have.contains(s)) {
                Probe::Gap(format!("scopes {} lack {}", if have.is_empty() { "none".into() } else { have.join(" ") }, scopes.join(" or ")))
            } else {
                match &level {
                    Ok(l) if l >= min => Probe::Ok,
                    Ok(l) => Probe::Gap(format!("project role {l} below {min}")),
                    Err(e) => Probe::Unknown(format!("project role unread: {e}")),
                }
            }
        }
    })).collect()
}

/// Every check id; a heartbeat `doctor` line carries only these.
pub const IDS: [&str; 9] = ["token-issues", "token-contents", "token-pulls", "version", "reports", "service-error", "claude-trust", "agentq-hook", "agentq-data"];

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
pub fn token_items(repo: &str, gitlab: bool, access: &[(&'static str, Probe)]) -> Vec<Item> {
    TOKEN.iter().map(|(name, id, _)| (name, id, grant(name, gitlab))).map(|(name, id, grant)| match access.iter().find(|(n, _)| n == name).map(|(_, p)| p) {
        Some(Probe::Ok) => Item::ok(*id, format!("token grants {grant} on {repo}")),
        Some(Probe::Gap(why)) => Item::gap(*id, format!("token lacks {grant} on {repo} ({why})"), if gitlab {
            format!("owner: give the GitLab board token {grant} on {repo}")
        } else { format!("owner: edit the fine-grained board token, Repository permissions > {grant}, for {repo}") }),
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

/// The report rows of a complete collection whose source is unreadable and whose report still waits for the owner. A
/// decided report (hidden: answered, dropped, made, accepted) whose source went missing needs nothing more (#2246).
fn unreadable(cached: &Value) -> Vec<&Value> {
    cached["reports"].as_array().into_iter().chain(cached["history"].as_array()).flatten()
        .filter(|r| r["kind"] == "report" && r["reason"].is_string() && r["visible"] != false).collect()
}

/// Why the `reports` check is not ok, as heartbeat-safe words (#2246): one cause of [`REPORT_CAUSES`], then up to five
/// `#<N>` and `+<more>`; `None` when ok. Never report text, paths or response bodies.
pub fn reports_detail(cached: &Value) -> Option<String> {
    if cached["complete"] != true {
        let reason = cached["reason"].as_str().unwrap_or_default();
        return Some(if reason.contains("stale") { "stale" } else if reason.contains("not collected") { "uncollected" } else { "incomplete" }.into());
    }
    let failed = unreadable(cached);
    let status = |r: &&Value| r["reason"].as_str().and_then(http_status);
    let cause = if failed.is_empty() { return None }
        else if failed.iter().any(|r| matches!(status(r), Some(401 | 403))) { "denied" }
        else if failed.iter().all(|r| r["missing"] == true || status(r) == Some(404)) { "missing" } else { "unreadable" };
    let mut words = vec![cause.to_string()];
    words.extend(failed.iter().take(5).map(|r| format!("#{}", r["n"])));
    if failed.len() > 5 { words.push(format!("+{}", failed.len() - 5)); }
    Some(words.join(" "))
}

/// The causes a `doctor-reports` line may name.
const REPORT_CAUSES: [&str; 6] = ["stale", "uncollected", "incomplete", "denied", "missing", "unreadable"];

/// A heartbeat `doctor-reports` text: its cause and task numbers; `None` when invalid.
fn parse_reports(text: &str) -> Option<(&str, Vec<&str>)> {
    let mut w = text.split(' ');
    let cause = w.next().filter(|c| REPORT_CAUSES.contains(c))?;
    let rest: Vec<&str> = w.collect();
    rest.iter().all(|t| t.strip_prefix('#').or_else(|| t.strip_prefix('+')).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))).then_some((cause, rest))
}

/// The fix for a `reports` cause on `host`; `numbers` are the named reports.
fn reports_fix(cause: &str, numbers: &[&str], host: &str, gitlab: bool) -> String {
    let list = numbers.join(" ");
    let first = numbers.first().map_or("<n>", |n| n.trim_start_matches('#'));
    match cause {
        "stale" | "uncollected" | "incomplete" => format!("report collection {cause} on {host}: run `taskq service status` there; `taskq service restart` if it stays"),
        "denied" => format!("reports {list} unreadable (HTTP 401/403): owner: give the board token of {host} {}", grant("Contents", gitlab)),
        "missing" => format!("reports {list} have no source file at their result SHA: on {host} run `taskq results {first} --action followup` or `--action reopen`"),
        _ => format!("reports {list} unreadable: on {host} run `taskq results {first}` to read the error"),
    }
}

/// `reports`: the cached report collection ([`crate::reports::cached`]). A collection is complete while single reports
/// stay unreadable (#2195), so a report row with a reason is a gap too, until its report is decided (#2246).
pub fn reports_item(cached: &Value, gitlab: bool) -> Item {
    let Some(detail) = reports_detail(cached) else { return Item::ok("reports", "report collection complete") };
    let (cause, numbers) = parse_reports(&detail).unwrap_or(("incomplete", vec![]));
    let fix = reports_fix(cause, &numbers, "this host", gitlab);
    if cached["complete"] != true {
        return Item::gap("reports", format!("report collection incomplete: {}", cached["reason"].as_str().unwrap_or("reason unknown")), fix);
    }
    let failed = unreadable(cached);
    let reason = short(failed[0]["reason"].as_str().unwrap_or_default());
    Item::gap("reports", format!("{} undecided report(s) unreadable ({cause}: {}), first: {reason}", failed.len(), numbers.join(" ")), fix)
}

/// `service-error`: the last pass error of the project's `status.json` (`age` in seconds).
pub fn error_item(status: Result<Option<Value>, String>, age: Option<u64>, ttl: i64) -> Item {
    match status {
        Ok(Some(s)) if age.is_some_and(|a| crate::snapshot::live(a as i64, ttl)) => match s["error"].as_str().or(s["focus_error"].as_str()) {
            None => Item::ok("service-error", "last service pass had no error"),
            Some(e) => Item::gap("service-error", format!("last service pass failed: {}", short(e)), "read `taskq service status` and fix the named cause"),
        },
        Ok(Some(_)) => Item::unknown("service-error", "service status is stale; the service may be stopped", "run `taskq service status`; if stopped, `taskq service restart`"),
        Ok(None) => Item::unknown("service-error", "no service status; no service ran this project here", "run `taskq service install`"),
        Err(e) => Item::unknown("service-error", format!("service status unreadable: {}", short(&e)), "run `taskq service status`"),
    }
}

/// `claude-trust` (#2300): Claude trusts the registered root `root`, per the home Claude config (#1555). No Claude
/// config is ok: Claude is not set up for this OS user.
pub fn trust_item(home: &Path, root: &Path) -> Item {
    let key = crate::runtime::owner::claude_key(root);
    match crate::runtime::owner::trust_state(home, root) {
        Ok(None) => Item::ok("claude-trust", "no Claude config for this OS user"),
        Ok(Some(true)) => Item::ok("claude-trust", format!("Claude trusts {key}")),
        Ok(Some(false)) => Item::gap("claude-trust", format!("Claude does not trust {key}; Claude workers cannot start here"),
            format!("run `claude` in {key} and accept its trust prompt; other Claude sessions keep running")),
        Err(e) => Item::unknown("claude-trust", format!("Claude config unreadable: {}", short(&e)), format!("repair `~/{}`, then `taskq doctor`", crate::runtime::owner::CLAUDE_CONFIG)),
    }
}

/// The agentq executable name of this platform.
fn agentq_exe() -> &'static str { if cfg!(windows) { "agentq.exe" } else { "agentq" } }

/// The binary an agentq hook command runs: the word before ` hook ` when it is `agentq` or a path to it (quoted or
/// not); `None` for any other command.
fn hook_binary(command: &str) -> Option<String> {
    let head = command.split_once(" hook ")?.0.trim_end();
    let word = match head.strip_suffix('"').or_else(|| head.strip_suffix('\'')) {
        Some(q) => &q[q.rfind(['"', '\'']).map_or(0, |i| i + 1)..],
        None => head.rsplit(char::is_whitespace).next()?,
    };
    let name = Path::new(word).file_name()?.to_string_lossy().to_ascii_lowercase();
    matches!(name.as_str(), "agentq" | "agentq.exe").then(|| word.to_string())
}

/// Every agentq hook binary in one parsed hook file: any string value that is an agentq hook command.
fn hook_binaries(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.extend(hook_binary(s)),
        Value::Array(a) => a.iter().for_each(|x| hook_binaries(x, out)),
        Value::Object(o) => o.values().for_each(|x| hook_binaries(x, out)),
        _ => {}
    }
}

/// `agentq-hook` (#2300, agentq#190): each agentq hook in `files` (client hook files) runs the current release of the
/// agentq install `agentq`. The hook runs in the binary it names, a bare `agentq` being the install launcher, so a
/// binary whose bytes differ from `releases/<current>/agentq` runs old hook code.
pub fn hook_item(agentq: &Path, files: &[std::path::PathBuf]) -> Item {
    const ID: &str = "agentq-hook";
    const ONBOARD: &str = "run `agentq onboard --yes`: it installs the current launcher and rewrites the hooks";
    if !agentq.exists() { return Item::ok(ID, "agentq is not installed"); }
    let unknown = |what: String| Item::unknown(ID, what, "run `agentq doctor`; if it names no cause, `agentq onboard --yes`");
    let mut hooks = vec![];
    for f in files {
        let name = f.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return unknown(format!("hook file {name} unreadable: {}", short(&e.to_string()))),
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return unknown(format!("hook file {name} is not JSON")) };
        let mut found = vec![];
        hook_binaries(&v, &mut found);
        hooks.extend(found.into_iter().map(|b| (name.clone(), b)));
    }
    if hooks.is_empty() { return Item::ok(ID, "no agentq hook installed"); }
    let current = match std::fs::read_to_string(agentq.join("current")) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        Ok(_) => return unknown("agentq names no current release".into()),
        Err(e) => return unknown(format!("agentq current release unreadable: {}", short(&e.to_string()))),
    };
    let release = match std::fs::read(agentq.join("releases").join(&current).join(agentq_exe())) {
        Ok(b) => b,
        Err(e) => return unknown(format!("agentq release {current} unreadable: {}", short(&e.to_string()))),
    };
    for (file, bin) in &hooks {
        let path = if bin.contains(['/', '\\']) { std::path::PathBuf::from(bin) } else { agentq.join(agentq_exe()) };
        let same = std::fs::metadata(&path).is_ok_and(|m| m.len() == release.len() as u64) && std::fs::read(&path).is_ok_and(|b| b == release);
        if !same {
            return Item::gap(ID, format!("agentq hook in {file} runs {}, not the current release {current}", path.display()), ONBOARD);
        }
    }
    Item::ok(ID, format!("agentq hooks run release {current}"))
}

/// `agentq-data` (#2300, agentq#190): the data folder of the agentq install `agentq` takes a write. One probe file is
/// created and removed; an existing `lifecycle.lock` is opened for append and left unchanged.
pub fn data_item(agentq: &Path) -> Item {
    const ID: &str = "agentq-data";
    const FIX: &str = "give this OS user write access to the agentq data folder";
    if !agentq.exists() { return Item::ok(ID, "agentq is not installed"); }
    let data = agentq.join("data");
    if !data.is_dir() { return Item::gap(ID, format!("agentq data folder {} missing", data.display()), "run `agentq onboard --yes`"); }
    let probe = data.join(format!(".taskq-doctor-{}", std::process::id()));
    let wrote = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map(drop);
    let _ = std::fs::remove_file(&probe);
    let lock = data.join("lifecycle.lock");
    let locked = if lock.exists() { std::fs::OpenOptions::new().append(true).open(&lock).map(drop) } else { Ok(()) };
    match wrote.and(locked) {
        Ok(()) => Item::ok(ID, "agentq data folder is writable"),
        Err(e) => Item::gap(ID, format!("agentq data folder {} not writable: {}", data.display(), short(&e.to_string())), FIX),
    }
}

/// The client hook files agentq installs into: Claude `settings.json`, Codex `hooks.json`, Antigravity `hooks.json`.
fn hook_files(home: Option<&Path>) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = crate::runtime::claude_home().map(|h| h.join("settings.json")).into_iter().collect();
    v.extend(crate::runtime::codex_home().map(|h| h.join("hooks.json")));
    v.extend(home.map(|h| h.join(".gemini").join("config").join("hooks.json")));
    v
}

/// All checks of one board; `latest` is the latest release read once per run.
pub fn board_items(b: &dyn crate::board::Board, repo: &str, gitlab: bool, root: &Path, install: &Path, latest: &Result<String, String>) -> Vec<Item> {
    let mut items = token_items(repo, gitlab, &b.access());
    let installed = crate::platform::tool(install).current().unwrap_or_else(|| crate::platform::VERSION.into());
    items.push(version_item(&installed, latest));
    items.push(reports_item(&crate::reports::cached(root), gitlab));
    let path = crate::dot(root).join("status.json");
    items.push(error_item(crate::data::json(&path), crate::file_age(&path), crate::snapshot::ttl(root)));
    // #2300: what a worker launch needs on this host beyond the board
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from);
    items.push(match &home {
        Some(h) => trust_item(h, &root.canonicalize().unwrap_or_else(|_| root.to_path_buf())),
        None => Item::unknown("claude-trust", "no home folder (HOME, USERPROFILE) to read the Claude config from", "run `taskq service install` as the OS user that runs Claude"),
    });
    match crate::agentq::root() {
        Ok(a) => {
            items.push(hook_item(&a, &hook_files(home.as_deref())));
            items.push(data_item(&a));
        }
        Err(e) => for id in ["agentq-hook", "agentq-data"] { items.push(Item::unknown(id, format!("agentq install folder unknown: {e}"), "run `taskq service install` as the OS user that runs agentq")) },
    }
    items
}

/// The checks of a board whose board did not open: the token reads are unknown with the cause.
pub fn unopened(repo: &str, gitlab: bool, e: &str) -> Vec<Item> {
    token_items(repo, gitlab, &TOKEN.iter().map(|(n, ..)| (*n, Probe::Unknown(format!("board not opened: {e}")))).collect::<Vec<_>>())
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
    live(h, now, ttl) && h.doctor.as_deref().and_then(parse).is_some()
}

/// Whether heartbeat `h` is fresh at board time `now`. A heartbeat read after `now` was taken is fresh: its board
/// `updated_at` can only be later than the reader's earlier `Date` (#2246: a negative age read `<1m old; unreachable`).
fn live(h: &Host, now: i64, ttl: i64) -> bool {
    crate::snapshot::live((now - h.updated).max(0), ttl)
}

/// Host `h` read fresh by its issue (`GET`, not the list that lags minutes behind); unchanged when unreadable.
fn reread(b: &dyn crate::board::Board, h: &mut Host, ttl: i64) {
    if let Ok(issue) = b.issue(h.issue).and_then(|i| if i.trusted && crate::service::is_host(&i) { Ok(i) } else { Err("not a trusted heartbeat".into()) }) {
        let now = b.now().unwrap_or(issue.updated);
        *h = crate::snapshot::host(&issue, now, ttl);
    }
}

/// `taskq doctor --hosts`, step one on one board: re-reads every heartbeat the list shows stale (a listed heartbeat may
/// lag, #2246), then asks every live host that runs health checks to run them now through the request issue.
pub fn request(b: &dyn crate::board::Board, hosts: &mut [Host], now: i64, ttl: i64) -> Asked {
    for h in hosts.iter_mut().filter(|h| !live(h, now, ttl)) { reread(b, h, ttl); }
    let now = b.now().unwrap_or(now);
    if !hosts.iter().any(|h| answers(h, now, ttl)) { return Asked { id: Err("no live host runs health checks".into()), waited: 0 }; }
    let id: String = crate::runtime::uuid().chars().filter(char::is_ascii_alphanumeric).take(12).collect();
    let posted = b.issues(None).and_then(|issues| {
        match issues.iter().filter(|i| i.trusted && i.title == REQUEST_TITLE).max_by_key(|i| i.number) {
            Some(i) => b.edit(i.number, &request_body(&id)),
            None => b.create(REQUEST_TITLE, &request_body(&id), &[crate::service::HOST_LABEL.into()]).map(|_| ()),
        }
    });
    match posted { Ok(()) => Asked { id: Ok(id), waited: 0 }, Err(e) => Asked { id: Err(format!("request not posted: {}", short(&e))), waited: 0 } }
}

/// One board of a `--hosts` run: its board, hosts, request and the hosts still to answer.
pub struct Ask<'a> { pub board: &'a dyn crate::board::Board, pub hosts: Vec<Host>, pub asked: Asked, pending: Vec<usize> }

impl<'a> Ask<'a> {
    /// The hosts of `board` that can answer the posted request; none when it was not posted.
    pub fn new(board: &'a dyn crate::board::Board, hosts: Vec<Host>, asked: Asked, now: i64, ttl: i64) -> Self {
        let pending = if asked.id.is_ok() { (0..hosts.len()).filter(|&i| answers(&hosts[i], now, ttl)).collect() } else { vec![] };
        Ask { board, hosts, asked, pending }
    }
}

/// `taskq doctor --hosts`, step two for every board at once: reads each unanswered heartbeat fresh every `poll` seconds
/// until all answer or `wait` passes in all, so the wait is one per run, not one per board (#2246).
pub fn collect(asks: &mut [Ask], ttl: i64, wait: u64, poll: u64, sleep: &dyn Fn(u64)) {
    let mut waited = 0;
    while waited < wait && asks.iter().any(|a| !a.pending.is_empty()) {
        sleep(poll);
        waited += poll;
        for a in asks.iter_mut() {
            let Ok(id) = a.asked.id.clone() else { continue };
            for &i in &a.pending { reread(a.board, &mut a.hosts[i], ttl); }
            let hosts = &a.hosts;
            a.pending.retain(|&i| hosts[i].answer.as_deref() != Some(id.as_str()));
        }
    }
    for a in asks.iter_mut() { a.asked.waited = waited; }
}

/// One fix line for check `id` on `host`; `reports` is the host's `doctor-reports` text.
pub fn fix(id: &str, status: &str, repo: &str, host: &str, gitlab: bool, reports: Option<&str>) -> String {
    match TOKEN.iter().find(|t| t.1 == id) {
        // #2246: an unread permission is no missing grant
        Some(_) if status == "unknown" => format!("run `taskq doctor` on {host}: it names why the token read failed"),
        Some((name, ..)) if gitlab => format!("owner: give the GitLab board token of {host} {} on {repo}", grant(name, true)),
        Some((name, ..)) => format!("owner: grant the board token of {host} {} for {repo}", grant(name, false)),
        None => match id {
            "version" => format!("run `taskq update` on {host}"),
            "reports" => match reports.and_then(parse_reports) {
                Some((cause, numbers)) => reports_fix(cause, &numbers, host, gitlab),
                None => format!("run `taskq doctor` on {host}: its version names no report cause"),
            },
            // #2300: the heartbeat carries no paths; the host's own `taskq doctor` names the folder or hook
            _ if status == "unknown" && matches!(id, "claude-trust" | "agentq-hook" | "agentq-data") => format!("run `taskq doctor` on {host}: it names why the check failed"),
            "claude-trust" => format!("on {host}: run `claude` in the project root and accept its trust prompt (`taskq doctor` names the folder)"),
            "agentq-hook" => format!("run `agentq onboard --yes` on {host}: its client hooks run an old agentq (`taskq doctor` names the hook)"),
            "agentq-data" => format!("on {host}: give its OS user write access to the agentq data folder (`taskq doctor` names it)"),
            _ => format!("run `taskq doctor` on {host} and fix the named service error"),
        },
    }
}

/// One `--hosts` row: host, version, `ok|gap|unknown`, the open ids, the reason of an unknown and one fix line. With a
/// posted request, a host that did not answer it reads unknown; without one, the last published result stands.
pub fn host_row(h: &Host, repo: &str, gitlab: bool, now: i64, ttl: i64, asked: Option<&Asked>) -> Value {
    let unknown = |reason: String, fix: String| json!({"host": h.name, "version": h.version, "running_selected": h.version_text(), "state": "unknown", "gaps": [], "reason": reason, "fix": fix});
    if !live(h, now, ttl) {
        return unknown(format!("heartbeat {} old, read fresh; its service is not writing", crate::age(now - h.updated)),
            format!("on {}: `taskq service status`; if stopped, `taskq service restart`; if retired, close its heartbeat issue", h.name));
    }
    let Some((at, open)) = h.doctor.as_deref().and_then(parse) else {
        return unknown(format!("version {} runs no health checks", h.version), format!("run `taskq update` on {}", h.name));
    };
    match asked.map(|a| &a.id) {
        Some(Ok(id)) if h.answer.as_deref() != Some(id.as_str()) => return unknown(
            format!("live heartbeat, no answer to the request within {}s: its service has not run a pass since", asked.map_or(0, |a| a.waited)),
            format!("on {}: `taskq service status` shows the stuck pass; then `taskq doctor --hosts` again", h.name)),
        Some(Ok(_)) => {}
        _ if now - at > STALE => return unknown(format!("health checks {} old", crate::age(now - at)), format!("run `taskq service restart` on {}", h.name)),
        _ => {}
    }
    let state = if open.is_empty() { "ok" } else if open.iter().any(|(_, s)| s == "gap") { "gap" } else { "unknown" };
    let gaps: Vec<String> = open.iter().map(|(id, s)| if s == "gap" { id.clone() } else { format!("{id} (unknown)") }).collect();
    let fixes: Vec<String> = open.iter().map(|(id, s)| fix(id, s, repo, &h.name, gitlab, h.reports.as_deref())).collect();
    json!({"host": h.name, "version": h.version, "running_selected": h.version_text(), "state": state, "gaps": gaps, "reason": null, "fix": (!fixes.is_empty()).then(|| fixes.join("; "))})
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
            Some(x) if !crate::snapshot::live((s.observed - x.updated).max(0), ttl) => format!("unknown: host heartbeat {} old", crate::age(s.observed - x.updated)),
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

/// `taskq doctor --hosts` lines of every board (#2246: one run, one table): the host table, then the worker table. Each
/// row carries its `board`; a board that could not be read is one host row with host `-` and the cause.
pub fn hosts_lines(hosts: &[Value], workers: &[Value]) -> Vec<String> {
    let text = |v: &Value| v.as_str().unwrap_or("-").to_string();
    let mut out = vec![];
    let mut rows = vec![["board", "host", "version", "state", "gaps", "fix"].map(String::from).to_vec()];
    rows.extend(hosts.iter().map(|h| {
        let gaps = h["gaps"].as_array().filter(|g| !g.is_empty()).map(|g| g.iter().map(text).collect::<Vec<_>>().join(", "));
        vec![text(&h["board"]), text(&h["host"]), h["running_selected"].as_str().map_or_else(|| text(&h["version"]), str::to_string), text(&h["state"]), gaps.unwrap_or_else(|| text(&h["reason"])), text(&h["fix"])]
    }));
    if hosts.is_empty() { out.push("no host heartbeat on any board".into()); } else { out.extend(table(&rows)); }
    let alive = |w: &Value| if w["alive"] == true { "yes".to_string() } else { format!("unknown: {}", text(&w["alive_reason"])) };
    let stalled = |w: &Value| match w["stalled"].as_bool() {
        Some(true) => format!("yes: {}", w["reason"].as_str().unwrap_or("reason unknown")),
        Some(false) => "no".to_string(),
        None => "unknown".to_string(),
    };
    let mut rows = vec![["board", "worker", "task", "attempt", "runtime", "alive", "stalled", "account"].map(String::from).to_vec()];
    rows.extend(workers.iter().map(|w| vec![text(&w["board"]), text(&w["host"]), format!("#{}", w["task"]), w["attempt"].to_string(), text(&w["runtime"]),
        alive(w), stalled(w), text(&w["account"])]));
    out.push(String::new());
    if workers.is_empty() { out.push("no running worker".into()); } else { out.extend(table(&rows)); }
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
        let items = token_items("o/r", false, &[("Issues", Probe::Ok), ("Contents", Probe::Gap("HTTP 403".into())), ("Pull requests", Probe::Ok)]);
        let c = items.iter().find(|i| i.id == "token-contents").unwrap();
        assert_eq!((c.status, c.text.as_str()), ("gap", "token lacks Contents: Read on o/r (HTTP 403)"));
        assert!(c.fix.as_deref().unwrap().contains("Contents: Read"), "{:?}", c.fix);
        assert!(items.iter().filter(|i| i.id != "token-contents").all(|i| i.status == "ok"));
        assert_eq!(line(7, &items), "doctor 7 token-contents:gap");
        assert_eq!(http_status("GET /commits?per_page=1: HTTP 403 Resource not accessible"), Some(403));
        assert!(token_items("o/r", false, &[]).iter().all(|i| i.status == "unknown"), "an unsupported board is unknown, never ok");
    }

    // Chain: § 6 GitLab board token -> `taskq doctor` token checks (#2246 A2). Risk: GitLab boards had no token
    // probe, so every token cell read unknown and a read-only token passed setup unnoticed. Protection: the token's own
    // scopes and the project role decide each permission; a missing scope or a low role is a gap with a GitLab fix,
    // an unread fact stays unknown.
    #[test]
    fn doctor_checks_gitlab_token_scopes_and_role() {
        let dev = Ok(json!({"permissions": {"project_access": {"access_level": 30}, "group_access": null}}));
        let full = gitlab_access(&Ok(json!({"active": true, "revoked": false, "scopes": ["api"]})), &dev);
        assert!(full.iter().all(|(_, p)| *p == Probe::Ok), "{full:?}");
        let read = gitlab_access(&Ok(json!({"active": true, "scopes": ["read_api"]})), &Ok(json!({"permissions": {"group_access": {"access_level": 20}}})));
        assert_eq!(read, vec![("Issues", Probe::Gap("scopes read_api lack api".into())), ("Contents", Probe::Ok), ("Pull requests", Probe::Gap("scopes read_api lack api".into()))]);
        let low = gitlab_access(&Ok(json!({"active": true, "scopes": ["api"]})), &Ok(json!({"permissions": {"project_access": {"access_level": 20}}})));
        assert_eq!(low[2].1, Probe::Gap("project role 20 below 30".into()));
        assert!(matches!(gitlab_access(&Ok(json!({"active": false, "scopes": ["api"]})), &dev)[0].1, Probe::Gap(_)));
        assert!(gitlab_access(&Err("HTTP 401".into()), &dev).iter().all(|(_, p)| matches!(p, Probe::Unknown(_))), "unread scopes: unknown, never ok");
        let items = token_items("g/p", true, &read);
        assert_eq!(items[0].fix.as_deref(), Some("owner: give the GitLab board token scope api and role Reporter or higher on g/p"));
        assert!(!items.iter().filter_map(|i| i.fix.as_deref()).any(|f| f.contains("fine-grained")), "no GitHub fix on GitLab");
    }

    // Chain: § 8 update -> `taskq doctor` version check (#2194 A1, A4). Risk: a host stayed on an old release with no
    // sign in doctor. Protection: behind the latest release is a gap with `taskq update`; an unread release is unknown.
    // #2246: a stale service status names restart, never `service install` for a service that exists.
    #[test]
    fn doctor_flags_an_outdated_version() {
        let v = version_item("4.6.58", &Ok("4.6.60".into()));
        assert_eq!((v.status, v.text.as_str(), v.fix.as_deref()), ("gap", "installed 4.6.58 is behind the latest release 4.6.60", Some("run `taskq update`")));
        assert_eq!(version_item("4.6.60", &Ok("4.6.60".into())).status, "ok");
        assert_eq!(version_item("4.7.0-dev.1", &Ok("4.6.60".into())).status, "ok", "a newer dev build is not behind");
        assert_eq!(version_item("4.6.58", &Err("no network".into())).status, "unknown");
        let r = reports_item(&json!({"complete": false, "reason": "Report collection is stale; waiting for the service"}), false);
        assert_eq!((r.status, r.text.as_str()), ("gap", "report collection incomplete: Report collection is stale; waiting for the service"));
        assert_eq!(reports_item(&json!({"complete": true}), false).status, "ok");
        let e = error_item(Ok(Some(json!({"error": "board unavailable: HTTP 502\ndetail"}))), Some(5), 120);
        assert_eq!((e.status, e.text.as_str()), ("gap", "last service pass failed: board unavailable: HTTP 502"));
        assert_eq!(error_item(Ok(Some(json!({"error": null}))), Some(5), 120).status, "ok");
        let stale = error_item(Ok(Some(json!({"error": null}))), Some(500), 120);
        assert_eq!(stale.status, "unknown", "a stale status is unknown");
        assert!(!stale.fix.unwrap().contains("install"), "an existing service is restarted, not installed");
        assert_eq!(error_item(Ok(None), None, 120).status, "unknown");
    }

    // Chain: reports collection (#2195) -> `taskq doctor` reports check -> heartbeat `doctor-reports` -> `--hosts`
    // (#2194 A1, #2246 A2). Risk: decided historical reports whose source went 404 kept a `reports` gap on every host
    // with no cause, and an undecided unreadable report read ok. Protection: only undecided report rows count; the
    // gap and the heartbeat name the cause and the reports; the `--hosts` fix follows the cause.
    #[test]
    fn doctor_reports_name_missing_reports_and_skip_decided_ones() {
        let ok = reports_item(&json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "task", "n": 5, "reason": "No accepted hand-in recorded; result details unknown"},
                {"kind": "report", "n": 6, "visible": false, "missing": true, "reason": "#6 source: HTTP 404"}]}), false);
        assert_eq!(ok.status, "ok", "a decided report with a gone source needs nothing");
        let rows = json!({"complete": true, "reports": [{"kind": "report", "n": 4, "reason": null}],
            "history": [{"kind": "report", "n": 7, "visible": true, "reason": "#7 source: GET /contents/x: HTTP 403 Resource not accessible"},
                {"kind": "report", "n": 8, "missing": true, "reason": "#8 source: HTTP 404"}]});
        assert_eq!(reports_detail(&rows).as_deref(), Some("denied #7 #8"));
        let bad = reports_item(&rows, false);
        assert_eq!((bad.status, bad.text.as_str()), ("gap", "2 undecided report(s) unreadable (denied: #7 #8), first: #7 source: GET /contents/x: HTTP 403 Resource not accessible"));
        assert_eq!(bad.fix.as_deref(), Some("reports #7 #8 unreadable (HTTP 401/403): owner: give the board token of this host Contents: Read"));
        let many: Vec<Value> = (1..=7).map(|n| json!({"kind": "report", "n": n, "missing": true, "reason": "source: HTTP 404"})).collect();
        let gone = json!({"complete": true, "reports": many});
        assert_eq!(reports_detail(&gone).as_deref(), Some("missing #1 #2 #3 #4 #5 +2"));
        assert_eq!(reports_detail(&json!({"complete": false, "reason": "Reports not collected yet"})).as_deref(), Some("uncollected"));
        assert_eq!(fix("reports", "gap", "o/r", "win", false, Some("missing #1 #2 +2")),
            "reports #1 #2 +2 have no source file at their result SHA: on win run `taskq results 1 --action followup` or `--action reopen`");
        assert_eq!(fix("reports", "gap", "o/r", "win", false, Some("evil text")), "run `taskq doctor` on win: its version names no report cause", "only known causes");
    }

    // Chain: § 4 step 3 heartbeat `doctor` line -> `taskq doctor --hosts` (#2194 A2, A4). Risk: a dead or old host
    // read as healthy, a hand-written heartbeat injected text, or (#2246 A1, A2) a heartbeat updated after the reader's
    // clock read `<1m old; host unreachable` with `service install`. Protection: a heartbeat newer than `now` is live; a
    // stale one names the silent service and restart; an old host reads unknown with its version; unknown ids refused.
    #[test]
    fn doctor_hosts_reads_stale_and_old_hosts_unknown_and_fresh_ones_live() {
        let host = |name: &str, updated, version: &str, doctor: Option<&str>| Host { name: name.into(), version: version.into(), updated, doctor: doctor.map(String::from), ..Default::default() };
        let now = 10_000;
        let gone = host_row(&host("mac", now - 1500, "4.6.60", Some("9000 ok")), "o/r", false, now, 120, None);
        assert_eq!((gone["state"].as_str(), gone["reason"].as_str()), (Some("unknown"), Some("heartbeat 25m old, read fresh; its service is not writing")));
        assert!(!gone["fix"].as_str().unwrap().contains("install"), "{gone}");
        let ahead = host_row(&host("pds", now + 30, "4.6.60", Some("9990 ok")), "o/r", false, now, 120, None);
        assert_eq!(ahead["state"], "ok", "a heartbeat written after the reader's clock is fresh: {ahead}");
        let old = host_row(&host("lin", now - 10, "4.6.50", None), "o/r", false, now, 120, None);
        assert_eq!((old["state"].as_str(), old["reason"].as_str()), (Some("unknown"), Some("version 4.6.50 runs no health checks")));
        let gap = host_row(&host("win", now - 10, "4.6.58", Some("9900 token-contents:gap version:gap")), "o/r", false, now, 120, None);
        assert_eq!(gap["state"], "gap");
        assert_eq!(gap["gaps"], json!(["token-contents", "version"]));
        assert_eq!(gap["fix"], "owner: grant the board token of win Contents: Read for o/r; run `taskq update` on win");
        let gl = host_row(&host("pds", now - 10, "4.6.62", Some("9900 token-pulls:gap")), "g/p", true, now, 120, None);
        assert_eq!(gl["fix"], "owner: give the GitLab board token of pds scope api and role Developer or higher on g/p");
        let unread = host_row(&host("pds", now - 10, "4.6.62", Some("9900 token-issues:unknown")), "g/p", true, now, 120, None);
        assert_eq!(unread["fix"], "run `taskq doctor` on pds: it names why the token read failed", "an unread token asks for no grant");
        assert_eq!(host_row(&host("ok", now - 10, "4.6.60", Some("9900 ok")), "o/r", false, now, 120, None)["state"], "ok");
        let lapsed = host_row(&host("ok", now - 10, "4.6.60", Some("100 ok")), "o/r", false, now, 120, None);
        assert_eq!(lapsed["state"], "unknown", "a result the service stopped refreshing is unknown");
        assert_eq!(parse("9900 rm-rf:gap"), None, "only known ids");
        assert_eq!(parse("9900 version:fine"), None, "only known statuses");
        assert_eq!(parse("9900"), None);
        let tag = |mut v: Value, b: &str| { v["board"] = json!(b); v };
        let lines = hosts_lines(&[tag(gone, "o/r"), tag(gap, "o/r"), tag(gl, "g/p")],
            &[json!({"board": "g/p", "host": "win", "task": 7, "attempt": 42, "runtime": "claude", "alive": true, "stalled": false, "reason": null, "account": "ok (account work)"})]);
        assert!(lines[0].starts_with("board") && lines[0].contains("host"), "one host table with a board column: {lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("o/r") && l.contains("mac") && l.contains("not writing")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("g/p") && l.contains("pds")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("g/p ") && l.contains("#7") && l.contains("claude") && l.contains("yes")), "{lines:#?}");
        // #2247 A1: the version column shows running and, during a handover, selected with its waits; equal is one value
        let mut handover = host("win", now - 10, "4.6.61", Some("9900 ok"));
        handover.selected = Some("4.6.62 waits #7 attempt 42".into());
        let row = host_row(&handover, "o/r", false, now, 120, None);
        assert_eq!((row["version"].as_str(), row["running_selected"].as_str()), (Some("4.6.61"), Some("4.6.61 (selected 4.6.62 waits #7 attempt 42)")));
        let lines = hosts_lines(&[tag(row, "o/r"), tag(host_row(&host("lin", now - 10, "4.6.62", Some("9900 ok")), "o/r", false, now, 120, None), "o/r")], &[]);
        assert!(lines.iter().any(|l| l.starts_with("o/r") && l.contains(" win ") && l.contains("4.6.61 (selected 4.6.62 waits #7 attempt 42)")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("o/r") && l.contains(" lin ") && l.contains("4.6.62") && !l.contains("selected")), "{lines:#?}");
    }

    // Chain: `taskq doctor --hosts` request issue -> service pass -> heartbeat `doctor-answer` (#2194 A2, #2246 A1).
    // Risk: a lagging issue list showed a live host stale, every board waited 300s in turn (14 boards: 70 minutes),
    // and a silent host read `busy or unreachable`. Protection: a listed-stale heartbeat is read fresh before the
    // request; one wait covers every board and stops once all answer; a silent live host names its stuck service.
    #[test]
    fn doctor_hosts_requests_every_board_at_once_and_reads_a_silent_host_unknown() {
        use crate::board::{Board, Fake};
        let (b, c) = (Fake::default(), Fake::default());
        b.advance(10_000);
        c.advance(10_000);
        let beat = |b: &Fake, name: &str, body: &str| b.create(&format!("taskq host {name}"), body, &[crate::service::HOST_LABEL.into()]).unwrap();
        let (win, lin) = (beat(&b, "win", "version 4.6.61\ndoctor 9990 ok"), beat(&b, "lin", "version 4.6.61\ndoctor 9990 ok"));
        let mac = beat(&b, "mac", "version 4.6.61\ndoctor 9990 ok");
        beat(&b, "old", "version 4.6.50");
        let pds = beat(&c, "pds", "version 4.6.61\ndoctor 9990 ok");
        let ttl = 120;
        let read = |b: &Fake| b.issues(None).unwrap().iter().filter(|i| crate::service::is_host(i)).map(|i| crate::snapshot::host(i, b.now().unwrap(), ttl)).collect::<Vec<_>>();
        let mut hosts = read(&b);
        // the list lags: mac reads two hours old there while its heartbeat is fresh
        hosts.iter_mut().find(|h| h.name == "mac").unwrap().updated = 1_000;
        let mut gone = read(&c);
        let now = b.now().unwrap();
        let asked = request(&b, &mut hosts, now, ttl);
        assert!(hosts.iter().find(|h| h.name == "mac").unwrap().updated > 1_000, "a listed-stale heartbeat is read fresh");
        let other = request(&c, &mut gone, now, ttl);
        let id = asked.id.clone().unwrap();
        let mut asks = vec![Ask::new(&b, hosts, asked, now, ttl), Ask::new(&c, gone, other, now, ttl)];
        // win, mac and pds answer on their next pass; lin never answers
        let slept = std::cell::Cell::new(0);
        let answer = |_| {
            slept.set(slept.get() + 1);
            let id = request_of(&b.issues(None).unwrap()).expect("request on the board");
            b.edit(win, &format!("version 4.6.61\ndoctor {now} token-pulls:gap\n{}", answer_line(&id))).unwrap();
            b.edit(mac, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap();
            let id = request_of(&c.issues(None).unwrap()).expect("request on the second board");
            c.edit(pds, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap();
        };
        collect(&mut asks, ttl, 60, 20, &answer);
        assert_eq!((asks[0].asked.waited, asks[1].asked.waited, slept.get()), (60, 60, 3), "one wait for both boards, full for the silent host");
        let requests: Vec<_> = b.issues(None).unwrap().into_iter().filter(|i| i.title == REQUEST_TITLE).collect();
        assert_eq!(requests.len(), 1);
        assert!(!crate::service::is_host(&requests[0]), "an old host's heartbeat parser ignores the request");
        assert_eq!(request_of(&requests), Some(id.clone()));
        let rows: Vec<Value> = asks[0].hosts.iter().map(|h| host_row(h, "o/r", false, now + 60, ttl, Some(&asks[0].asked))).collect();
        let row = |name: &str| rows.iter().find(|r| r["host"] == name).unwrap().clone();
        assert_eq!((row("win")["state"].as_str(), &row("win")["gaps"]), (Some("gap"), &json!(["token-pulls"])), "the answer's fresh result");
        assert_eq!(row("mac")["state"], "ok", "a host the list showed stale answers");
        assert_eq!((row("lin")["state"].as_str(), row("lin")["reason"].as_str()), (Some("unknown"), Some("live heartbeat, no answer to the request within 60s: its service has not run a pass since")));
        assert_eq!(row("old")["reason"], "version 4.6.50 runs no health checks");
        assert_eq!(host_row(&asks[1].hosts[0], "g/p", true, now + 60, ttl, Some(&asks[1].asked))["state"], "ok");
        // the next request edits the same issue and stops waiting once every live host answered
        b.edit(lin, &format!("version 4.6.61\ndoctor {now} ok")).unwrap();
        let mut hosts = read(&b);
        let again = request(&b, &mut hosts, b.now().unwrap(), ttl);
        let mut asks = vec![Ask::new(&b, hosts, again, b.now().unwrap(), ttl)];
        let all = |_| {
            let id = request_of(&b.issues(None).unwrap()).unwrap();
            for n in [win, lin, mac] { b.edit(n, &format!("version 4.6.61\ndoctor {now} ok\n{}", answer_line(&id))).unwrap(); }
        };
        collect(&mut asks, ttl, 300, 20, &all);
        assert_eq!(asks[0].asked.waited, 20, "no wait once all answered");
        assert_eq!(b.issues(None).unwrap().iter().filter(|i| i.title == REQUEST_TITLE).count(), 1, "one reused request issue");
        assert_ne!(asks[0].asked.id.as_ref().unwrap(), &id, "every run is a new request");
        let mut none = vec![Host { name: "gone".into(), issue: 9999, updated: 1_000, doctor: Some("900 ok".into()), ..Default::default() }];
        assert!(request(&b, &mut none, now, ttl).id.is_err(), "no live host: no request");
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
        let lines = hosts_lines(&[], &w);
        assert!(lines.iter().any(|l| l.contains("#3") && l.contains("unknown: owning host heartbeat stale")), "{lines:#?}");
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("taskq-health-{tag}-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // Chain: § 6 host setup -> `taskq doctor` claude-trust -> heartbeat -> `--hosts` (#2300 A1, assetq#17). Risk:
    // `doctor --hosts` read all ok while Claude refused a registered root, so the first Claude worker failed there.
    // Protection: an untrusted root is a `claude-trust` gap naming the folder with one fix; a trusted ancestor or no
    // Claude config is ok, an unreadable config unknown; `--hosts` names the gap with one fix.
    #[test]
    fn doctor_flags_a_root_claude_does_not_trust() {
        let home = scratch("trust");
        let root = home.join("work").join("assetq");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(trust_item(&home, &root).status, "ok", "no Claude config: Claude is not set up here");
        let config = home.join(crate::runtime::owner::CLAUDE_CONFIG);
        std::fs::write(&config, json!({"projects": {"/elsewhere": {"hasTrustDialogAccepted": true}}}).to_string()).unwrap();
        let gap = trust_item(&home, &root);
        let key = crate::runtime::owner::claude_key(&root);
        assert_eq!((gap.status, gap.text.as_str()), ("gap", format!("Claude does not trust {key}; Claude workers cannot start here").as_str()));
        assert!(gap.fix.as_deref().unwrap().contains(&key), "{:?}", gap.fix);
        assert_eq!(line(7, &[gap]), "doctor 7 claude-trust:gap");
        let parent = crate::runtime::owner::claude_key(&home.join("work"));
        std::fs::write(&config, json!({"projects": {parent: {"hasTrustDialogAccepted": true}}}).to_string()).unwrap();
        assert_eq!(trust_item(&home, &root).status, "ok", "a trusted parent covers the root");
        std::fs::write(&config, "{").unwrap();
        assert_eq!(trust_item(&home, &root).status, "unknown", "an unreadable config is unknown, never ok");
        let now = 10_000;
        let h = Host { name: "win".into(), version: "4.7.0".into(), updated: now - 10, doctor: Some("9900 claude-trust:gap".into()), ..Default::default() };
        let row = host_row(&h, "o/r", false, now, 120, None);
        assert_eq!((row["state"].as_str(), row["gaps"][0].as_str()), (Some("gap"), Some("claude-trust")), "{row}");
        assert!(row["fix"].as_str().unwrap().contains("accept its trust prompt"), "{row}");
        let _ = std::fs::remove_dir_all(&home);
    }

    // Chain: agentq onboarding -> client hook files -> `taskq doctor` agentq-hook -> `--hosts` (#2300 A1, agentq#190).
    // Risk: a client hook ran an installed agentq launcher older than the current release, so its old hook code
    // refused worker calls while doctor read ok. Protection: a hook whose binary differs from the current release is
    // a gap naming the file and release with one fix; a matching launcher, no hook or no agentq install is ok.
    #[test]
    fn doctor_flags_an_agentq_hook_older_than_the_release() {
        let base = scratch("hook");
        let agentq = base.join("agentq");
        let exe = agentq_exe();
        std::fs::create_dir_all(agentq.join("releases/1.2.0")).unwrap();
        std::fs::create_dir_all(agentq.join("releases/1.1.0")).unwrap();
        std::fs::write(agentq.join("current"), "1.2.0\n").unwrap();
        std::fs::write(agentq.join("releases/1.2.0").join(exe), b"agentq 1.2.0").unwrap();
        std::fs::write(agentq.join("releases/1.1.0").join(exe), b"agentq 1.1.0").unwrap();
        std::fs::write(agentq.join(exe), b"agentq 1.1.0").unwrap();
        let claude = base.join("settings.json");
        let codex = base.join("hooks.json");
        let files = vec![claude.clone(), codex.clone(), base.join("absent.json")];
        assert_eq!(hook_item(&base.join("none"), &files).status, "ok", "no agentq install");
        assert_eq!(hook_item(&agentq, &files).status, "ok", "no hook file");
        std::fs::write(&claude, json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "other hook x"}, {"type": "command", "command": "agentq hook claude pre"}]}]}}).to_string()).unwrap();
        let gap = hook_item(&agentq, &files);
        assert_eq!(gap.status, "gap", "{gap:?}");
        assert!(gap.text.starts_with("agentq hook in settings.json runs ") && gap.text.ends_with("not the current release 1.2.0"), "{}", gap.text);
        assert!(gap.fix.as_deref().unwrap().contains("agentq onboard --yes"), "{:?}", gap.fix);
        std::fs::write(agentq.join(exe), b"agentq 1.2.0").unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "ok", "the launcher is the current release");
        let old = agentq.join("releases/1.1.0").join(exe);
        std::fs::write(&codex, json!({"agentq": [{"command": format!("\"{}\" hook antigravity PreToolUse", old.display())}]}).to_string()).unwrap();
        assert!(hook_item(&agentq, &files).text.contains("hooks.json"), "a quoted path to an old release is a gap");
        std::fs::write(&codex, "{").unwrap();
        assert_eq!(hook_item(&agentq, &files).status, "unknown", "an unreadable hook file is unknown, never ok");
        assert_eq!(hook_binary("'/opt/a b/agentq' hook codex pre").as_deref(), Some("/opt/a b/agentq"));
        assert_eq!(hook_binary("env X=1 agentq.exe hook claude pre").as_deref(), Some("agentq.exe"));
        assert_eq!(hook_binary("notagentq hook claude pre"), None);
        let now = 10_000;
        let h = Host { name: "mac".into(), version: "4.7.0".into(), updated: now - 10, doctor: Some("9900 agentq-hook:gap".into()), ..Default::default() };
        let row = host_row(&h, "o/r", false, now, 120, None);
        assert_eq!((row["state"].as_str(), row["fix"].as_str()), (Some("gap"), Some("run `agentq onboard --yes` on mac: its client hooks run an old agentq (`taskq doctor` names the hook)")), "{row}");
        let _ = std::fs::remove_dir_all(&base);
    }

    // Chain: agentq install -> `taskq doctor` agentq-data -> `--hosts` (#2300 A1, agentq#190). Risk: agentq could not
    // write its `data/lifecycle.lock` (EPERM), so every agentq hook failed while doctor read ok. Protection: a refused
    // write in the data folder or on its lock is a gap with one fix; the probe leaves no file and the lock unchanged.
    #[test]
    fn doctor_flags_an_unwritable_agentq_data_folder() {
        let base = scratch("data");
        let agentq = base.join("agentq");
        assert_eq!(data_item(&agentq).status, "ok", "no agentq install");
        std::fs::create_dir_all(&agentq).unwrap();
        assert_eq!(data_item(&agentq).status, "gap", "a missing data folder");
        let data = agentq.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let lock = data.join("lifecycle.lock");
        std::fs::write(&lock, "held").unwrap();
        assert_eq!(data_item(&agentq).status, "ok");
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 1, "the probe leaves no file");
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "held", "the lock is unchanged");
        let mut p = std::fs::metadata(&lock).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&lock, p.clone()).unwrap();
        let gap = data_item(&agentq);
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        std::fs::set_permissions(&lock, p).unwrap();
        assert_eq!(gap.status, "gap", "{gap:?}");
        assert!(gap.text.contains("not writable"), "{}", gap.text);
        assert_eq!(gap.fix.as_deref(), Some("give this OS user write access to the agentq data folder"));
        let now = 10_000;
        let h = Host { name: "mac".into(), version: "4.7.0".into(), updated: now - 10, doctor: Some("9900 agentq-data:gap agentq-hook:unknown".into()), ..Default::default() };
        let row = host_row(&h, "o/r", false, now, 120, None);
        assert_eq!(row["gaps"], json!(["agentq-data", "agentq-hook (unknown)"]), "{row}");
        assert!(row["fix"].as_str().unwrap().starts_with("on mac: give its OS user write access to the agentq data folder"), "{row}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
