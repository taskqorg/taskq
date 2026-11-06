//! Health checks (#2194, [host service](../docs/design/host-service.md#health-checks-2194)): per board the token
//! permissions taskq needs, installed vs latest release, report collection, the last service error and what a worker
//! launch needs on the host (Claude trust of the root, agentq hooks and data folder, Mac Codex Git metadata; #2300, #2449), each with one fix; the
//! heartbeat `doctor` line that publishes them; the board request that asks every host to run them now and the
//! heartbeat `doctor-answer` line that answers it; and the `taskq doctor --hosts` host and worker rows. Nothing here
//! prints a token, a path or a raw response body into a heartbeat or the request.

use std::path::Path;

use serde_json::{Value, json};

use crate::check::{Item, Probe};
use crate::core::State;
use crate::snapshot::Host;

/// Seconds between two health checks of a project service.
pub const EVERY: u64 = 3600;
/// A published result older than this reads unknown: the service stopped checking.
const STALE: i64 = 3 * EVERY as i64;
/// Board permission name, check id and the fine-grained grant the owner gives.
const TOKEN: [(&str, &str, &str); 3] = [
    ("Issues", "token-issues", "Issues: Read and write"),
    ("Contents", "token-contents", "Contents: Read"),
    (
        "Pull requests",
        "token-pulls",
        "Pull requests: Read and write",
    ),
];
/// The GitLab token scopes and project role of each permission of [`TOKEN`] (#2246): `api` writes issues and merge
/// requests, `read_api` or `read_repository` reads files; Reporter (20) reads the repository, Developer (30) opens
/// merge requests.
const GITLAB: [(&str, &[&str], u64, &str); 3] = [
    (
        "Issues",
        &["api"],
        20,
        "scope api and role Reporter or higher",
    ),
    (
        "Contents",
        &["api", "read_api", "read_repository"],
        20,
        "scope read_api, read_repository or api and role Reporter or higher",
    ),
    (
        "Pull requests",
        &["api"],
        30,
        "scope api and role Developer or higher",
    ),
];

/// The grant the owner gives for permission `name` on a GitHub or GitLab board.
fn grant(name: &str, gitlab: bool) -> &'static str {
    if gitlab {
        GITLAB.iter().find(|g| g.0 == name).map_or("", |g| g.3)
    } else {
        TOKEN.iter().find(|t| t.0 == name).map_or("", |t| t.2)
    }
}

/// GitLab `access` (#2246): the token's own scopes and state (`GET /personal_access_tokens/self`) and the token user's
/// project role (`GET /projects/:id` permissions), one probe per permission of [`TOKEN`]. Any unread fact is unknown
/// with its cause, never ok.
pub fn gitlab_access(
    token: &crate::board::Result<Value>,
    project: &crate::board::Result<Value>,
) -> Vec<(&'static str, Probe)> {
    let level = project.as_ref().map(|p| {
        ["project_access", "group_access"]
            .iter()
            .filter_map(|k| p["permissions"][k]["access_level"].as_u64())
            .max()
            .unwrap_or(0)
    });
    GITLAB
        .iter()
        .map(|(name, scopes, min, _)| {
            (
                *name,
                match token {
                    Err(e) => Probe::Unknown(format!("token scopes unread: {e}")),
                    Ok(t) if t["active"] == false || t["revoked"] == true => {
                        Probe::Gap("token inactive, revoked or expired".into())
                    }
                    Ok(t) => {
                        let have: Vec<&str> = t["scopes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .collect();
                        if !scopes.iter().any(|s| have.contains(s)) {
                            Probe::Gap(format!(
                                "scopes {} lack {}",
                                if have.is_empty() {
                                    "none".into()
                                } else {
                                    have.join(" ")
                                },
                                scopes.join(" or ")
                            ))
                        } else {
                            match &level {
                                Ok(l) if l >= min => Probe::Ok,
                                Ok(l) => Probe::Gap(format!("project role {l} below {min}")),
                                Err(e) => Probe::Unknown(format!("project role unread: {e}")),
                            }
                        }
                    }
                },
            )
        })
        .collect()
}

/// Every check id; a heartbeat `doctor` line carries only these.
pub const IDS: [&str; 11] = [
    "disk",
    "token-issues",
    "token-contents",
    "token-pulls",
    "version",
    "reports",
    "service-error",
    "claude-trust",
    "agentq-hook",
    "agentq-data",
    "codex-git",
];

/// Title of the one open board issue that carries the `taskq doctor --hosts` request. It is labelled `taskq-host` but is
/// no heartbeat (`service::is_host`), so hosts without health checks ignore it.
pub const REQUEST_TITLE: &str = "taskq doctor";
/// Stated seconds of a whole `taskq doctor --hosts` run, board reads included: the published view, and `--fresh`.
pub const READ_WAIT: u64 = 30;
pub const ASK_WAIT: u64 = 300;
/// Seconds between two heartbeat reads of one board in a `--fresh` run: at most one read per board per poll (#2450).
pub const ASK_POLL: u64 = 20;
/// A run stops reading this long before its stated deadline, so printing and the process exit stay inside it.
pub const EXIT_MARGIN: u64 = 5;

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
    match (std::fs::read_to_string(&path), coreq::status::age(&path)) {
        (Ok(v), Some(age)) if age <= STALE as u64 && !v.trim().is_empty() => {
            Ok(v.trim().to_string())
        }
        (Ok(_), _) => Err(format!(
            "latest release last read over {} ago",
            crate::time::age(STALE)
        )),
        _ => Err("latest release not read yet by the host doctor".into()),
    }
}

/// The first line of an error, bounded.
fn short(e: &str) -> String {
    e.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
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
        Ok(l) if coreq::update::newer(l, installed) => Item::gap(
            "version",
            format!("installed {installed} is behind the latest release {l}"),
            "run `taskq update`",
        ),
        Ok(l) => Item::ok(
            "version",
            format!("installed {installed}, latest release {l}"),
        ),
        Err(e) => Item::unknown(
            "version",
            format!(
                "installed {installed}, latest release unknown: {}",
                short(e)
            ),
            "check the network, then run `taskq update --check`",
        ),
    }
}

/// The report rows of a complete collection whose source is unreadable and whose report still waits for the owner. A
/// decided report (hidden: answered, dropped, made, accepted) whose source went missing needs nothing more (#2246).
fn unreadable(cached: &Value) -> Vec<&Value> {
    cached["reports"]
        .as_array()
        .into_iter()
        .chain(cached["history"].as_array())
        .flatten()
        .filter(|r| r["kind"] == "report" && r["reason"].is_string() && r["visible"] != false)
        .collect()
}

/// Why the `reports` check is not ok, as heartbeat-safe words (#2246): one cause of [`REPORT_CAUSES`], then up to five
/// `#<N>` and `+<more>`; `None` when ok. Never report text, paths or response bodies.
pub fn reports_detail(cached: &Value) -> Option<String> {
    if cached["complete"] != true {
        let reason = cached["reason"].as_str().unwrap_or_default();
        return Some(
            if reason.contains("stale") {
                "stale"
            } else if reason.contains("not collected") {
                "uncollected"
            } else {
                "incomplete"
            }
            .into(),
        );
    }
    let failed = unreadable(cached);
    let status = |r: &&Value| r["status"].as_u64();
    let cause = if failed.is_empty() {
        return None;
    } else if failed.iter().any(|r| matches!(status(r), Some(401 | 403))) {
        "denied"
    } else if failed
        .iter()
        .all(|r| r["missing"] == true || status(r) == Some(404))
    {
        "missing"
    } else {
        "unreadable"
    };
    let mut words = vec![cause.to_string()];
    words.extend(failed.iter().take(5).map(|r| format!("#{}", r["n"])));
    if failed.len() > 5 {
        words.push(format!("+{}", failed.len() - 5));
    }
    Some(words.join(" "))
}

/// The causes a `doctor-reports` line may name.
const REPORT_CAUSES: [&str; 6] = [
    "stale",
    "uncollected",
    "incomplete",
    "denied",
    "missing",
    "unreadable",
];

/// A heartbeat `doctor-reports` text: its cause and task numbers; `None` when invalid.
fn parse_reports(text: &str) -> Option<(&str, Vec<&str>)> {
    let mut w = text.split(' ');
    let cause = w.next().filter(|c| REPORT_CAUSES.contains(c))?;
    let rest: Vec<&str> = w.collect();
    rest.iter()
        .all(|t| {
            t.strip_prefix('#')
                .or_else(|| t.strip_prefix('+'))
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        })
        .then_some((cause, rest))
}

/// The fix for a `reports` cause on `host`; `numbers` are the named reports.
fn reports_fix(cause: &str, numbers: &[&str], host: &str, gitlab: bool) -> String {
    let list = numbers.join(" ");
    let first = numbers.first().map_or("<n>", |n| n.trim_start_matches('#'));
    match cause {
        "stale" | "uncollected" | "incomplete" => format!(
            "report collection {cause} on {host}: run `taskq service status` there; `taskq service restart` if it stays"
        ),
        "denied" => format!(
            "reports {list} unreadable (HTTP 401/403): owner: give the board token of {host} {}",
            grant("Contents", gitlab)
        ),
        "missing" => format!(
            "reports {list} have no source file at their result SHA: on {host} run `taskq results {first} --action followup` or `--action reopen`"
        ),
        _ => format!(
            "reports {list} unreadable: on {host} run `taskq results {first}` to read the error"
        ),
    }
}

/// `reports`: the cached report collection ([`crate::reports::cached`]). A collection is complete while single reports
/// stay unreadable (#2195), so a report row with a reason is a gap too, until its report is decided (#2246).
pub fn reports_item(cached: &Value, gitlab: bool) -> Item {
    let Some(detail) = reports_detail(cached) else {
        return Item::ok("reports", "report collection complete");
    };
    let (cause, numbers) = parse_reports(&detail).unwrap_or(("incomplete", vec![]));
    let fix = reports_fix(cause, &numbers, "this host", gitlab);
    if cached["complete"] != true {
        return Item::gap(
            "reports",
            format!(
                "report collection incomplete: {}",
                cached["reason"].as_str().unwrap_or("reason unknown")
            ),
            fix,
        );
    }
    let failed = unreadable(cached);
    let reason = short(failed[0]["reason"].as_str().unwrap_or_default());
    Item::gap(
        "reports",
        format!(
            "{} undecided report(s) unreadable ({cause}: {}), first: {reason}",
            failed.len(),
            numbers.join(" ")
        ),
        fix,
    )
}

/// `service-error`: the last pass error of the project's `status.json` (`age` in seconds).
pub fn error_item(status: Result<Option<Value>, String>, age: Option<u64>, ttl: i64) -> Item {
    match status {
        Ok(Some(s)) if age.is_some_and(|a| crate::snapshot::live(a as i64, ttl)) => {
            match s["error"].as_str().or(s["focus_error"].as_str()) {
                None => Item::ok("service-error", "last service pass had no error"),
                Some(e) => Item::gap(
                    "service-error",
                    format!("last service pass failed: {}", short(e)),
                    "read `taskq service status` and fix the named cause",
                ),
            }
        }
        Ok(Some(_)) => Item::unknown(
            "service-error",
            "service status is stale; the service may be stopped",
            "run `taskq service status`; if stopped, `taskq service restart`",
        ),
        Ok(None) => Item::unknown(
            "service-error",
            "no service status; no service ran this project here",
            "run `taskq service install`",
        ),
        Err(e) => Item::unknown(
            "service-error",
            format!("service status unreadable: {}", short(&e)),
            "run `taskq service status`",
        ),
    }
}

/// `claude-trust` (#2300): Claude trusts the registered root `root`, per the home Claude config (#1555). No Claude
/// config is ok: Claude is not set up for this OS user. A host without a claude binary has no trust to give and no worker
/// it could block, whatever config it holds (#2417).
pub fn trust_item(home: &Path, root: &Path, claude: bool) -> Item {
    if !claude {
        return no_claude();
    }
    let key = crate::runtime::owner::claude_key(root);
    match crate::runtime::owner::trust_state(home, root) {
        Ok(None) => Item::ok("claude-trust", "no Claude config for this OS user"),
        Ok(Some(true)) => Item::ok("claude-trust", format!("Claude trusts {key}")),
        Ok(Some(false)) => Item::gap(
            "claude-trust",
            format!("Claude does not trust {key}; Claude workers cannot start here"),
            format!(
                "run `claude` in {key} and accept its trust prompt; other Claude sessions keep running"
            ),
        ),
        Err(e) => Item::unknown(
            "claude-trust",
            format!("Claude config unreadable: {}", short(&e)),
            format!(
                "repair `~/{}`, then `taskq doctor`",
                crate::runtime::CLAUDE_CONFIG
            ),
        ),
    }
}

/// `codex-git` (#2449): a Mac Codex launch can write the project's Git metadata; `gap` is why it cannot.
pub fn codex_git_item(gap: Option<String>) -> Item {
    match gap {
        None => Item::ok(
            "codex-git",
            "no Mac Codex launch lacks the writable Git metadata root",
        ),
        Some(why) => Item::gap(
            "codex-git",
            why,
            "`taskq service permissions --runtime codex --mode workspace --scope project`, on a root whose `.git` is a real directory",
        ),
    }
}

fn no_claude() -> Item {
    Item::ok("claude-trust", "no claude binary on this host")
}

/// The agentq executable name of this platform.
fn agentq_exe() -> &'static str {
    if cfg!(windows) {
        "agentq.exe"
    } else {
        "agentq"
    }
}

/// The binary an agentq hook command runs: the word before ` hook ` when it is `agentq` or a path to it (quoted or
/// not); `None` for any other command.
fn hook_binary(command: &str) -> Option<String> {
    let head = command.split_once(" hook ")?.0.trim_end();
    let word = match head.strip_suffix('"').or_else(|| head.strip_suffix('\'')) {
        Some(q) => &q[q.rfind(['"', '\'']).map_or(0, |i| i + 1)..],
        None => head.rsplit(char::is_whitespace).next()?,
    };
    let name = Path::new(word)
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase();
    matches!(name.as_str(), "agentq" | "agentq.exe").then(|| word.to_string())
}

/// The client of an agentq hook command, `<binary> hook <client> <event>` as AgentQ writes it: AgentQ's own agent name.
fn hook_client(command: &str) -> Option<&str> {
    command.split_once(" hook ")?.1.split_whitespace().next()
}

/// Every agentq hook in one parsed hook file, as binary and client: any string value that is an agentq hook command.
fn hook_binaries(v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::String(s) => out.extend(hook_binary(s).zip(hook_client(s).map(str::to_string))),
        Value::Array(a) => a.iter().for_each(|x| hook_binaries(x, out)),
        Value::Object(o) => o.values().for_each(|x| hook_binaries(x, out)),
        _ => {}
    }
}

/// AgentQ's disable intent as its own `data/integration.json` records it; only explicit `agentq enable` clears it. A
/// `profiles` entry is one disabled client (agentq `control::disabled`).
enum Intent {
    Clients(Vec<String>),
    HostDisabled,
    Unread,
}

fn agentq_intent(agentq: &Path) -> Intent {
    let text = match std::fs::read_to_string(agentq.join("data").join("integration.json")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Intent::Clients(vec![]),
        Err(_) => return Intent::Unread,
    };
    let Ok(state) = serde_json::from_str::<Value>(&text) else {
        return Intent::Unread;
    };
    match (
        state.get("host_disabled").and_then(Value::as_bool),
        state.get("profiles").and_then(Value::as_object),
    ) {
        (Some(true), _) => Intent::HostDisabled,
        (Some(false), Some(profiles)) => Intent::Clients(profiles.keys().cloned().collect()),
        _ => Intent::Unread,
    }
}

/// `agentq-hook` (#2300, agentq#190): each agentq hook in `files` (client hook files) runs the current release of the
/// agentq install `agentq`. The hook runs in the binary it names, a bare `agentq` being the install launcher, so a
/// binary whose bytes differ from `releases/<current>/agentq` is a gap, a stale dev launcher included. The fix names the
/// stale clients: bare `agentq onboard --yes` refuses while any ready client is disabled, and AgentQ's refusal names
/// `agentq enable`, which would turn that client on (#2417). A disabled host or client is left alone.
pub fn hook_item(agentq: &Path, files: &[std::path::PathBuf]) -> Item {
    const ID: &str = "agentq-hook";
    let unknown = |what: String, onboard: bool| {
        Item::unknown(
            ID,
            what,
            if onboard {
                "run `agentq doctor`; if it names no cause, `agentq onboard --yes`"
            } else {
                "run `agentq doctor`"
            },
        )
    };
    match std::fs::metadata(agentq) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return unknown("agentq install folder is not a directory".into(), true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Item::ok(ID, "agentq is not installed");
        }
        Err(e) => {
            return unknown(
                format!(
                    "agentq install folder unreadable: {}",
                    short(&e.to_string())
                ),
                true,
            );
        }
    }
    let disabled = match agentq_intent(agentq) {
        Intent::HostDisabled => return Item::ok(ID, "agentq is disabled on this host"),
        Intent::Clients(clients) => Some(clients),
        Intent::Unread => None,
    };
    let onboard = disabled.as_ref().is_some_and(Vec::is_empty);
    let mut hooks = vec![];
    for f in files {
        let name = f
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return unknown(
                    format!("hook file {name} unreadable: {}", short(&e.to_string())),
                    onboard,
                );
            }
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return unknown(format!("hook file {name} is not JSON"), onboard);
        };
        let mut found = vec![];
        hook_binaries(&v, &mut found);
        hooks.extend(found.into_iter().map(|(b, c)| (name.clone(), b, c)));
    }
    if hooks.is_empty() {
        return Item::ok(ID, "no agentq hook installed");
    }
    if let Some(off) = &disabled {
        hooks.retain(|(_, _, client)| !off.contains(client));
        if hooks.is_empty() {
            return Item::ok(ID, "agentq hooks belong only to disabled clients");
        }
    }
    let current = match std::fs::read_to_string(agentq.join("current")) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        Ok(_) => return unknown("agentq names no current release".into(), onboard),
        Err(e) => {
            return unknown(
                format!(
                    "agentq current release unreadable: {}",
                    short(&e.to_string())
                ),
                onboard,
            );
        }
    };
    let release = match std::fs::read(agentq.join("releases").join(&current).join(agentq_exe())) {
        Ok(b) => b,
        Err(e) => {
            return unknown(
                format!(
                    "agentq release {current} unreadable: {}",
                    short(&e.to_string())
                ),
                onboard,
            );
        }
    };
    let (mut stale, mut clients) = (None, vec![]);
    for (file, bin, client) in &hooks {
        let path = if bin.contains(['/', '\\']) {
            std::path::PathBuf::from(bin)
        } else {
            agentq.join(agentq_exe())
        };
        let binary = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return unknown(
                    format!(
                        "agentq hook in {file} binary unreadable: {}",
                        short(&e.to_string())
                    ),
                    onboard,
                );
            }
        };
        if binary == release {
            continue;
        }
        let what = format!(
            "agentq hook in {file} runs {}, not the current release {current}",
            path.display()
        );
        if disabled.is_none() {
            return unknown(
                format!("{what}; AgentQ's disable state is unreadable"),
                false,
            );
        }
        stale.get_or_insert(what);
        if !clients.contains(&client.as_str()) {
            clients.push(client.as_str());
        }
    }
    match stale {
        Some(what) => Item::gap(
            ID,
            what,
            format!(
                "run `agentq onboard --yes {}`: it installs the current launcher over any dev build, rewrites these hooks and leaves disabled clients off",
                clients.join(" ")
            ),
        ),
        None => Item::ok(ID, format!("agentq hooks run release {current}")),
    }
}

/// `agentq-data` (#2300, agentq#190): the data folder of the agentq install `agentq` takes a write. One probe file is
/// created and removed; an existing `lifecycle.lock` is opened for append and left unchanged.
pub fn data_item(agentq: &Path) -> Item {
    const ID: &str = "agentq-data";
    const FIX: &str = "give this OS user write access to the agentq data folder";
    if !agentq.exists() {
        return Item::ok(ID, "agentq is not installed");
    }
    let data = agentq.join("data");
    if !data.is_dir() {
        return Item::gap(
            ID,
            format!("agentq data folder {} missing", data.display()),
            "run `agentq onboard --yes`",
        );
    }
    let probe = data.join(format!(".taskq-doctor-{}", std::process::id()));
    let wrote = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map(drop);
    let _ = std::fs::remove_file(&probe);
    let lock = data.join("lifecycle.lock");
    let locked = if lock.exists() {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&lock)
            .map(drop)
    } else {
        Ok(())
    };
    match wrote.and(locked) {
        Ok(()) => Item::ok(ID, "agentq data folder is writable"),
        Err(e) => Item::gap(
            ID,
            format!(
                "agentq data folder {} not writable: {}",
                data.display(),
                short(&e.to_string())
            ),
            FIX,
        ),
    }
}

/// The client hook files agentq installs into: Claude `settings.json`, Codex `hooks.json`, Antigravity `hooks.json`.
fn hook_files(home: Option<&Path>) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = crate::runtime::claude_home()
        .map(|h| h.join("settings.json"))
        .into_iter()
        .collect();
    v.extend(crate::runtime::codex_home().map(|h| h.join("hooks.json")));
    v.extend(home.map(|h| h.join(".gemini").join("config").join("hooks.json")));
    v
}

/// `disk` (§ 3 Disk guard): whether this reading admits new attempts on the project's volumes.
pub fn disk_item(disk: crate::disk::Disk) -> Item {
    match disk.free_text() {
        Some(_) if disk.holds() => Item::gap("disk", disk.reason(), disk.next()),
        Some(free) => Item::ok("disk", free),
        None => Item::unknown("disk", disk.reason(), disk.next()),
    }
}

/// All checks of one board; `latest` is the latest release read once per run.
pub fn board_items(
    b: &dyn crate::board::Board,
    repo: &str,
    gitlab: bool,
    root: &Path,
    install: &Path,
    codex: &[String],
    latest: &Result<String, String>,
) -> Vec<Item> {
    let mut items = token_items(repo, gitlab, &b.access());
    let installed = crate::platform::tool(install)
        .current()
        .unwrap_or_else(|| crate::platform::VERSION.into());
    items.push(version_item(&installed, latest));
    items.push(disk_item(crate::disk::Disk::read(root)));
    items.push(reports_item(&crate::reports::cached(root), gitlab));
    let path = crate::paths::dot(root).join("status.json");
    items.push(error_item(
        crate::data::json(&path),
        coreq::status::age(&path),
        crate::snapshot::ttl(root),
    ));
    // #2300: what a worker launch needs on this host beyond the board
    let home = crate::platform::user_home();
    let claude = crate::runtime::owner::claude_installed();
    items.push(match &home {
        Some(h) => trust_item(
            h,
            &root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            claude,
        ),
        None if !claude => no_claude(),
        None => Item::unknown(
            "claude-trust",
            "no home folder (HOME, USERPROFILE) to read the Claude config from",
            "run `taskq service install` as the OS user that runs Claude",
        ),
    });
    items.push(codex_git_item(crate::policy::git_metadata_gaps(
        install, root, codex,
    )));
    match crate::platform::agentq_root() {
        Ok(a) => {
            items.push(hook_item(&a, &hook_files(home.as_deref())));
            items.push(data_item(&a));
        }
        Err(e) => {
            for id in ["agentq-hook", "agentq-data"] {
                items.push(Item::unknown(
                    id,
                    format!("agentq install folder unknown: {e}"),
                    "run `taskq service install` as the OS user that runs agentq",
                ))
            }
        }
    }
    items
}

/// The checks of a board whose board did not open: the token reads are unknown with the cause.
pub fn unopened(repo: &str, gitlab: bool, e: &str) -> Vec<Item> {
    token_items(
        repo,
        gitlab,
        &TOKEN
            .iter()
            .map(|(n, ..)| (*n, Probe::Unknown(format!("board not opened: {e}"))))
            .collect::<Vec<_>>(),
    )
}

/// `taskq doctor` lines of one board: `ok|gap|unknown <id>: <text>`, then `  fix: ...` under each open item.
pub fn lines(repo: &str, items: &[Item]) -> Vec<String> {
    let mut out = vec![format!("board {repo}")];
    for i in items {
        out.push(format!("  {:<7} {}: {}", i.status, i.id, i.text));
        if i.status != "ok" {
            out.push(format!(
                "          fix: {}",
                i.fix.as_deref().unwrap_or_default()
            ));
        }
    }
    out
}

/// The heartbeat line: `doctor <time> ok`, or `doctor <time> <id>:gap|unknown ...` (ids only, never text).
pub fn line(at: i64, items: &[Item]) -> String {
    let open: Vec<String> = items
        .iter()
        .filter(|i| i.status != "ok" && IDS.contains(&i.id.as_str()))
        .map(|i| format!("{}:{}", i.id, i.status))
        .collect();
    format!(
        "doctor {at} {}",
        if open.is_empty() {
            "ok".to_string()
        } else {
            open.join(" ")
        }
    )
}

/// The text after `doctor ` of a heartbeat line: its time and the ids not ok; `None` when invalid.
pub fn parse(text: &str) -> Option<(i64, Vec<(String, String)>)> {
    let mut w = text.split(' ');
    let at = w.next()?.parse().ok()?;
    let rest: Vec<&str> = w.collect();
    if rest == ["ok"] {
        return Some((at, vec![]));
    }
    if rest.is_empty() {
        return None;
    }
    rest.iter()
        .map(|p| {
            let (id, s) = p.split_once(':')?;
            (IDS.contains(&id) && matches!(s, "gap" | "unknown"))
                .then(|| (id.to_string(), s.to_string()))
        })
        .collect::<Option<Vec<_>>>()
        .map(|v| (at, v))
}

/// A request id: 1 to 32 ASCII letters or digits.
fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The id of the open, trusted `taskq doctor --hosts` request among `issues`, newest first; `None` without one.
pub fn request_of(issues: &[crate::board::Issue]) -> Option<String> {
    issues
        .iter()
        .filter(|i| {
            i.trusted
                && i.title == REQUEST_TITLE
                && i.labels.iter().any(|l| l == crate::service::HOST_LABEL)
        })
        .max_by_key(|i| (i.updated, i.number))
        .and_then(|i| {
            i.body
                .lines()
                .next()?
                .strip_prefix("request ")
                .filter(|id| valid_id(id))
                .map(str::to_string)
        })
}

/// The request issue's body for request `id`.
pub fn request_body(id: &str) -> String {
    format!(
        "request {id}\n\n`taskq doctor --hosts --fresh` asks every TaskQ host service to run its health checks now; each answers in its heartbeat \
        with `doctor-answer {id}`. Keep this issue open: the next request edits it."
    )
}

/// The heartbeat line that answers request `id`.
pub fn answer_line(id: &str) -> String {
    format!("doctor-answer {id}")
}

/// A posted request: its id, or why none went out, and the seconds waited for answers.
pub struct Asked {
    pub id: Result<String, String>,
    pub waited: u64,
}

/// Whether host `h` can answer: its heartbeat is live and carries health checks.
fn answers(h: &Host, now: i64, ttl: i64) -> bool {
    live(h, now, ttl) && h.doctor.as_deref().and_then(parse).is_some()
}

/// Whether heartbeat `h` is fresh at board time `now`. A heartbeat read after `now` was taken is fresh: its board
/// `updated_at` can only be later than the reader's earlier `Date` (#2246: a negative age read `<1m old; unreachable`).
fn live(h: &Host, now: i64, ttl: i64) -> bool {
    crate::snapshot::live((now - h.updated).max(0), ttl)
}

/// Host `h` read fresh by its issue (`GET`, not the list that lags minutes behind); false and unchanged when unreadable.
fn reread(b: &dyn crate::board::Board, h: &mut Host, now: i64, ttl: i64) -> bool {
    let Ok(issue) = b.issue(h.issue).and_then(|i| {
        if i.trusted && crate::service::is_host(&i) {
            Ok(i)
        } else {
            Err("not a trusted heartbeat".into())
        }
    }) else {
        return false;
    };
    *h = crate::snapshot::host(&issue, now, ttl);
    true
}

/// Posts a new request through the board's request issue: edits the open one in `issues`, else creates it.
fn post(b: &dyn crate::board::Board, issues: &[crate::board::Issue]) -> Result<String, String> {
    let id: String = crate::runtime::uuid()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    match issues
        .iter()
        .filter(|i| i.trusted && i.title == REQUEST_TITLE)
        .max_by_key(|i| i.number)
    {
        Some(i) => b.edit(i.number, &request_body(&id)),
        None => b
            .create(
                REQUEST_TITLE,
                &request_body(&id),
                &[crate::service::HOST_LABEL.into()],
            )
            .map(|_| ()),
    }
    .map(|()| id)
    .map_err(|e| format!("request not posted: {}", short(e.text())))
}

/// What one board reports during a `--hosts` run, as soon as it is known.
pub enum Event {
    /// The current row of one host. A settled row no longer changes and prints at once in a `--fresh` run; an unsettled
    /// one is what the table shows if the deadline passes first.
    Row(Value, bool),
    /// The board's host list was read, however many hosts it holds.
    Listed,
    /// Why no host of this board could be read.
    Failed(String),
    Workers(Vec<Value>),
    WorkersFailed(String),
    /// Why the request did not go out on this board.
    Note(String),
}

/// One board's fixed facts during a run; board time is `now` at the run's start plus the seconds since.
pub struct Scope<'a> {
    pub repo: &'a str,
    pub gitlab: bool,
    pub ttl: i64,
    pub now: i64,
}

/// The run's clock and limits: seconds since the start, the seconds to stop polling at and between two polls.
pub struct Pace<'a> {
    pub wait: u64,
    pub poll: u64,
    pub elapsed: &'a dyn Fn() -> u64,
    pub sleep: &'a dyn Fn(u64),
}

impl Scope<'_> {
    fn at(&self, pace: &Pace) -> i64 {
        self.now + (pace.elapsed)() as i64
    }
    fn row(&self, h: &Host, now: i64, asked: Option<&Asked>) -> Value {
        host_row(h, self.repo, self.gitlab, now, self.ttl, asked)
    }
}

/// The hosts of an open issue list, each once (#2291: a page boundary that shifts mid-list repeats an issue).
pub fn listed(issues: &[crate::board::Issue], now: i64, ttl: i64) -> Vec<Host> {
    let mut seen = std::collections::HashSet::new();
    issues
        .iter()
        .filter(|i| i.trusted && crate::service::is_host(i) && seen.insert(i.number))
        .map(|i| crate::snapshot::host(i, now, ttl))
        .collect()
}

/// `taskq doctor --hosts` on one board: every host's last published result, with one fresh read of each heartbeat the
/// lagging list shows stale (#2246). No request and no sleep.
pub fn published(
    b: &dyn crate::board::Board,
    mut hosts: Vec<Host>,
    at: &Scope,
    pace: &Pace,
    emit: &mut dyn FnMut(Event),
) {
    let now = at.at(pace);
    for h in &hosts {
        emit(Event::Row(
            if live(h, now, at.ttl) {
                at.row(h, now, None)
            } else {
                unread_row(h, now)
            },
            false,
        ));
    }
    for h in hosts.iter_mut().filter(|h| !live(h, now, at.ttl)) {
        if reread(b, h, now, at.ttl) {
            emit(Event::Row(at.row(h, at.at(pace), None), true));
        }
    }
}

/// `taskq doctor --hosts --fresh` on one board: asks the hosts through the request issue and reads one heartbeat per
/// poll, in turn, until all answered or `pace.wait` passed. With no live host in the lagging list the first read comes
/// at once (#2246), and the first heartbeat read live posts the request, so a dead board gets no write.
pub fn ask(
    b: &dyn crate::board::Board,
    issues: &[crate::board::Issue],
    mut hosts: Vec<Host>,
    at: &Scope,
    pace: &Pace,
    emit: &mut dyn FnMut(Event),
) {
    use std::collections::VecDeque;
    let (ttl, now) = (at.ttl, at.at(pace));
    let mut queue = VecDeque::new();
    for (i, h) in hosts.iter().enumerate().filter(|(_, h)| !live(h, now, ttl)) {
        emit(Event::Row(unread_row(h, now), false));
        queue.push_back(i);
    }
    for (i, h) in hosts.iter().enumerate().filter(|(_, h)| live(h, now, ttl)) {
        let can = answers(h, now, ttl);
        emit(Event::Row(at.row(h, now, None), !can));
        if can {
            queue.push_back(i);
        }
    }
    let ask_now = |hosts: &[Host],
                   queue: &mut VecDeque<usize>,
                   now: i64,
                   emit: &mut dyn FnMut(Event)|
     -> Result<String, String> {
        let posted = post(b, issues);
        match &posted {
            Ok(id) => {
                for &i in queue.iter().filter(|&&i| answers(&hosts[i], now, ttl)) {
                    emit(Event::Row(
                        at.row(
                            &hosts[i],
                            now,
                            Some(&Asked {
                                id: Ok(id.clone()),
                                waited: pace.wait,
                            }),
                        ),
                        false,
                    ));
                }
            }
            Err(e) => {
                emit(Event::Note(e.clone()));
                for &i in queue.iter().filter(|&&i| answers(&hosts[i], now, ttl)) {
                    emit(Event::Row(at.row(&hosts[i], now, None), true));
                }
                queue.retain(|&i| !answers(&hosts[i], now, ttl));
            }
        }
        posted
    };
    let mut request = hosts
        .iter()
        .any(|h| answers(h, now, ttl))
        .then(|| ask_now(&hosts, &mut queue, now, emit));
    let mut at_once = request.is_none();
    while let Some(&i) = queue.front() {
        if !at_once {
            if (pace.elapsed)() + pace.poll > pace.wait {
                break;
            }
            (pace.sleep)(pace.poll);
        }
        at_once = false;
        queue.pop_front();
        let now = at.at(pace);
        if !reread(b, &mut hosts[i], now, ttl) {
            queue.push_back(i);
            continue;
        }
        if !answers(&hosts[i], now, ttl) {
            emit(Event::Row(at.row(&hosts[i], now, None), true));
            continue;
        }
        queue.push_back(i);
        if request.is_none() {
            request = Some(ask_now(&hosts, &mut queue, now, emit));
        }
        match &request {
            Some(Ok(id)) if hosts[i].answer.as_deref() == Some(id.as_str()) => {
                emit(Event::Row(
                    at.row(
                        &hosts[i],
                        now,
                        Some(&Asked {
                            id: Ok(id.clone()),
                            waited: (pace.elapsed)(),
                        }),
                    ),
                    true,
                ));
                queue.retain(|&q| q != i);
            }
            Some(Ok(_)) => {}
            // a request that did not go out: the published result stands, with its age
            _ => {
                if queue.contains(&i) {
                    emit(Event::Row(at.row(&hosts[i], now, None), true));
                    queue.retain(|&q| q != i);
                }
            }
        }
    }
    let now = at.at(pace);
    for &i in &queue {
        let h = &hosts[i];
        let row = match &request {
            Some(Ok(id)) if live(h, now, ttl) => at.row(
                h,
                now,
                Some(&Asked {
                    id: Ok(id.clone()),
                    waited: (pace.elapsed)(),
                }),
            ),
            _ => unread_row(h, now),
        };
        emit(Event::Row(row, true));
    }
}

/// One board's rows while a `--hosts` run gathers them.
pub struct Gathered {
    pub repo: String,
    pub rows: Vec<Value>,
    pub listed: bool,
    pub failed: Option<String>,
    pub workers: Option<Result<Vec<Value>, String>>,
    pub notes: Vec<String>,
}

impl Gathered {
    pub fn new(repo: &str) -> Self {
        Gathered {
            repo: repo.to_string(),
            rows: vec![],
            listed: false,
            failed: None,
            workers: None,
            notes: vec![],
        }
    }
    /// The newest row of its host replaces the older one, in the board's list order.
    fn take(&mut self, row: Value) {
        match self.rows.iter_mut().find(|r| r["host"] == row["host"]) {
            Some(old) => *old = row,
            None => self.rows.push(row),
        }
    }
}

/// Takes each board's events until every board finished or `deadline` passed, whichever is first: a hung or paused
/// board request never holds the run past its stated deadline. `settled` sees each settled host row as it arrives.
pub fn gather(
    rx: &std::sync::mpsc::Receiver<(usize, Event)>,
    deadline: std::time::Instant,
    boards: &mut [Gathered],
    settled: &mut dyn FnMut(&Value),
) {
    while let Ok((i, event)) =
        rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        let board = &mut boards[i];
        match event {
            Event::Row(mut row, done) => {
                row["board"] = json!(board.repo);
                if done {
                    settled(&row);
                }
                board.take(row);
            }
            Event::Listed => board.listed = true,
            Event::Failed(e) => board.failed = Some(e),
            Event::Workers(mut rows) => {
                for w in &mut rows {
                    w["board"] = json!(board.repo);
                }
                board.workers = Some(Ok(rows));
            }
            Event::WorkersFailed(e) => board.workers = Some(Err(e)),
            Event::Note(n) => board.notes.push(n),
        }
    }
}

/// One fix line for check `id` on `host`; `reports` is the host's `doctor-reports` text.
pub fn fix(
    id: &str,
    status: &str,
    repo: &str,
    host: &str,
    gitlab: bool,
    reports: Option<&str>,
) -> String {
    match TOKEN.iter().find(|t| t.1 == id) {
        // #2246: an unread permission is no missing grant
        Some(_) if status == "unknown" => {
            format!("run `taskq doctor` on {host}: it names why the token read failed")
        }
        Some((name, ..)) if gitlab => format!(
            "owner: give the GitLab board token of {host} {} on {repo}",
            grant(name, true)
        ),
        Some((name, ..)) => format!(
            "owner: grant the board token of {host} {} for {repo}",
            grant(name, false)
        ),
        None => match id {
            "version" => format!("run `taskq update` on {host}"),
            "disk" if status == "unknown" => format!(
                "on {host}: {}",
                crate::disk::Disk::Unknown.next()
            ),
            "disk" => format!("on {host}: {}", crate::disk::Disk::Free(0).next()),
            "reports" => match reports.and_then(parse_reports) {
                Some((cause, numbers)) => reports_fix(cause, &numbers, host, gitlab),
                None => format!("run `taskq doctor` on {host}: its version names no report cause"),
            },
            // #2300: the heartbeat carries no paths; the host's own `taskq doctor` names the folder or hook
            _ if status == "unknown"
                && matches!(id, "claude-trust" | "agentq-hook" | "agentq-data") =>
            {
                format!("run `taskq doctor` on {host}: it names why the check failed")
            }
            "claude-trust" => format!(
                "on {host}: run `claude` in the project root and accept its trust prompt (`taskq doctor` names the folder)"
            ),
            "codex-git" => format!(
                "run `taskq doctor` on {host} and the grant it names: a Mac Codex launch cannot write its project's Git metadata"
            ),
            "agentq-hook" => format!(
                "run `taskq doctor` on {host} and the agentq command it names: its client hooks differ from the current agentq release"
            ),
            "agentq-data" => format!(
                "on {host}: give its OS user write access to the agentq data folder (`taskq doctor` names it)"
            ),
            _ => format!("run `taskq doctor` on {host} and fix the named service error"),
        },
    }
}

/// The age of host `h`'s last published result and that result: `ok`, or its open ids with their statuses.
fn published_result(h: &Host, now: i64) -> (Option<String>, Option<String>) {
    let Some((at, open)) = h.doctor.as_deref().and_then(parse) else {
        return (None, None);
    };
    let last = if open.is_empty() {
        "ok".to_string()
    } else {
        open.iter()
            .map(|(id, s)| format!("{id}:{s}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    (Some(crate::time::age(now - at)), Some(last))
}

/// The row of a heartbeat the lagging list shows stale until its fresh read comes back: unknown, never `not writing`.
fn unread_row(h: &Host, now: i64) -> Value {
    let (age, last) = published_result(h, now);
    json!({"host": h.name, "version": h.version, "running_selected": h.version_text(), "state": "unknown", "gaps": [], "age": age, "last": last,
        "reason": format!("heartbeat listed {} old, fresh read did not finish", crate::time::age(now - h.updated)), "fix": "check the board token and network on this host, then `taskq doctor --hosts` again"})
}

/// One `--hosts` row: host, version, `ok|gap|unknown`, the age of the last published result, the open ids, the reason of
/// an unknown (with that last result in `last`) and one fix line. With a posted request, a host that did not answer it
/// reads unknown; without one, the last published result stands.
pub fn host_row(
    h: &Host,
    repo: &str,
    gitlab: bool,
    now: i64,
    ttl: i64,
    asked: Option<&Asked>,
) -> Value {
    let (age, last) = published_result(h, now);
    let unknown = |reason: String, fix: String| json!({"host": h.name, "version": h.version, "running_selected": h.version_text(), "state": "unknown", "gaps": [], "age": age, "last": last, "reason": reason, "fix": fix});
    if !live(h, now, ttl) {
        return unknown(
            format!(
                "heartbeat {} ago, read fresh; its service is not writing",
                crate::time::age(now - h.updated)
            ),
            format!(
                "on {}: `taskq service status`; if stopped, `taskq service restart`; if retired, close its heartbeat issue",
                h.name
            ),
        );
    }
    let Some((at, open)) = h.doctor.as_deref().and_then(parse) else {
        return unknown(
            format!("version {} runs no health checks", h.version),
            format!("run `taskq update` on {}", h.name),
        );
    };
    match asked.map(|a| &a.id) {
        Some(Ok(id)) if h.answer.as_deref() != Some(id.as_str()) => {
            return unknown(
                format!(
                    "live heartbeat, no answer to the request within {}s: its service has not run a pass since",
                    asked.map_or(0, |a| a.waited)
                ),
                format!(
                    "on {}: `taskq service status` shows the stuck pass; then `taskq doctor --hosts --fresh` again",
                    h.name
                ),
            );
        }
        Some(Ok(_)) => {}
        _ if now - at > STALE => {
            return unknown(
                format!("health checks {} ago", crate::time::age(now - at)),
                format!("run `taskq service restart` on {}", h.name),
            );
        }
        _ => {}
    }
    let state = if open.is_empty() {
        "ok"
    } else if open.iter().any(|(_, s)| s == "gap") {
        "gap"
    } else {
        "unknown"
    };
    let gaps: Vec<String> = open
        .iter()
        .map(|(id, s)| {
            if s == "gap" {
                id.clone()
            } else {
                format!("{id} (unknown)")
            }
        })
        .collect();
    let fixes: Vec<String> = open
        .iter()
        .map(|(id, s)| fix(id, s, repo, &h.name, gitlab, h.reports.as_deref()))
        .collect();
    json!({"host": h.name, "version": h.version, "running_selected": h.version_text(), "state": state, "gaps": gaps, "age": age, "reason": null, "fix": (!fixes.is_empty()).then(|| fixes.join("; "))})
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
            Some(x) if !crate::snapshot::live((s.observed - x.updated).max(0), ttl) => format!("unknown: host heartbeat {} ago", crate::time::age(s.observed - x.updated)),
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
    let width: Vec<usize> = (0..rows.first().map_or(0, Vec::len))
        .map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|r| {
            r.iter()
                .zip(&width)
                .map(|(cell, w)| format!("{cell:<w$}"))
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_string()
        })
        .collect()
}

/// The table header and the cells of one host row.
const HOST_COLUMNS: [&str; 7] = ["board", "host", "version", "state", "age", "gaps", "fix"];
fn host_cells(h: &Value) -> Vec<String> {
    let text = |v: &Value| v.as_str().unwrap_or("-").to_string();
    let gaps = h["gaps"]
        .as_array()
        .filter(|g| !g.is_empty())
        .map(|g| g.iter().map(text).collect::<Vec<_>>().join(", "));
    let gaps = gaps.unwrap_or_else(|| match h["last"].as_str() {
        Some(last) => format!("{} (last result: {last})", text(&h["reason"])),
        None => text(&h["reason"]),
    });
    vec![
        text(&h["board"]),
        text(&h["host"]),
        h["running_selected"]
            .as_str()
            .map_or_else(|| text(&h["version"]), str::to_string),
        text(&h["state"]),
        text(&h["age"]),
        gaps,
        text(&h["fix"]),
    ]
}

/// The header a `--fresh` run prints before its first answer.
pub fn host_header() -> String {
    HOST_COLUMNS.join("  ")
}

/// One host row as a `--fresh` run prints it the moment it answers.
pub fn host_line(h: &Value) -> String {
    host_cells(h).join("  ")
}

/// `taskq doctor --hosts` lines of every board (#2246: one run, one table): the host table, then the worker table. Each
/// row carries its `board`; a board that could not be read is one host row with host `-` and the cause. `unread` names
/// the boards whose workers were not read, so no missing row ever reads `no running worker` (R12).
pub fn hosts_lines(hosts: &[Value], workers: &[Value], unread: &[String]) -> Vec<String> {
    let text = |v: &Value| v.as_str().unwrap_or("-").to_string();
    let mut out = vec![];
    let mut rows = vec![HOST_COLUMNS.map(String::from).to_vec()];
    rows.extend(hosts.iter().map(host_cells));
    if hosts.is_empty() {
        out.push("no host heartbeat on any board".into());
    } else {
        out.extend(table(&rows));
    }
    let alive = |w: &Value| {
        if w["alive"] == true {
            "yes".to_string()
        } else {
            format!("unknown: {}", text(&w["alive_reason"]))
        }
    };
    let stalled = |w: &Value| match w["stalled"].as_bool() {
        Some(true) => format!("yes: {}", w["reason"].as_str().unwrap_or("reason unknown")),
        Some(false) => "no".to_string(),
        None => "unknown".to_string(),
    };
    let mut rows = vec![
        [
            "board", "worker", "task", "attempt", "runtime", "alive", "stalled", "account",
        ]
        .map(String::from)
        .to_vec(),
    ];
    rows.extend(workers.iter().map(|w| {
        vec![
            text(&w["board"]),
            text(&w["host"]),
            format!("#{}", w["task"]),
            w["attempt"].to_string(),
            text(&w["runtime"]),
            alive(w),
            stalled(w),
            text(&w["account"]),
        ]
    }));
    out.push(String::new());
    if workers.is_empty() && unread.is_empty() {
        out.push("no running worker".into());
    }
    if !workers.is_empty() {
        out.extend(table(&rows));
    }
    out.extend(unread.iter().cloned());
    out
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
