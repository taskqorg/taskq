//! § 2 Deploy: `deployed` entries on closed tasks, the summary's deploy lines, the `deploy-waiting` label and the
//! status line. `fold` ignores the kind, so a closed task stays closed (R1).
use crate::{
    Res,
    board::{self, Board, Issue},
    core::{self, Entry, State},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

/// The label of a closed task that waits for its first `deployed` entry (§ 2).
pub const LABEL: &str = "deploy-waiting";
const KIND: &str = "deployed";
pub const WAITING: &str = "Merged, waiting for deploy";

/// § 9 `deploy`: the project name the summary shows and its environments in deploy order.
#[derive(Clone, Debug, PartialEq)]
pub struct Deploy {
    pub project: String,
    pub envs: Vec<String>,
}

/// The `deploy` setting of the project at `root`; none when absent or unreadable.
pub fn config(root: &Path) -> Option<Deploy> {
    parse(&crate::config::project_value(root).ok()?)
}

/// `deploy` of a taskq.json value: a nonempty list of distinct words; anything else reads as absent.
pub fn parse(v: &Value) -> Option<Deploy> {
    let envs: Vec<String> = v["deploy"]
        .as_array()?
        .iter()
        .map(|e| e.as_str().filter(|s| word(s)).map(String::from))
        .collect::<Option<_>>()?;
    let distinct: BTreeSet<&String> = envs.iter().collect();
    let project = v["repo"]
        .as_str()?
        .rsplit('/')
        .next()
        .filter(|p| !p.is_empty())?
        .to_string();
    (!envs.is_empty() && distinct.len() == envs.len()).then_some(Deploy { project, envs })
}

/// A version, environment or host name: one word of letters, digits and `.`, `_`, `-`, `+`.
pub fn word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
}

/// One `deployed` entry: `taskq: deployed <env> <version>`, text `at <UTC>` and optionally `host <name>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub id: u64,
    pub env: String,
    pub version: String,
    pub at: String,
    pub host: Option<String>,
}

/// The op id of a deploy record: one per version, environment and host (§ 2).
pub fn op_id(env: &str, version: &str, host: Option<&str>) -> String {
    host.map_or(format!("deployed-{env}-{version}"), |h| {
        format!("deployed-{env}-{version}-{h}")
    })
}

/// The unedited `deployed` entries of task `n` in board order; malformed ones are skipped.
pub fn records(n: u64, log: &[Entry]) -> Vec<Record> {
    let mut entries: Vec<&Entry> = log.iter().filter(|e| !e.edited).collect();
    entries.sort_by_key(|e| e.id);
    entries
        .into_iter()
        .filter_map(|e| {
            let head = e.body.lines().next()?.trim().strip_prefix("taskq:")?;
            let [kind, env, version] = head.split_whitespace().collect::<Vec<_>>()[..] else {
                return None;
            };
            if kind != KIND || !word(env) || !word(version) {
                return None;
            }
            let text = core::entry_text(n, &e.body);
            let at = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("at "))
                .map(str::trim)
                .filter(|t| core::utc(t))?
                .to_string();
            let host = text
                .lines()
                .find_map(|l| l.trim().strip_prefix("host "))
                .map(str::trim)
                .filter(|h| word(h))
                .map(String::from);
            Some(Record {
                id: e.id,
                env: env.into(),
                version: version.into(),
                at,
                host,
            })
        })
        .collect()
}

/// The accepted `close` of a task closed from `review` as delivered (§ 7): only such a close waits for a deploy.
pub fn delivered(n: u64, log: &[Entry]) -> Option<u64> {
    let p = core::project(n, log, false);
    let (id, before, after) = p
        .changes
        .iter()
        .rev()
        .find(|(_, _, t)| t.state == State::Closed)?;
    if before.state != State::Review || after.state != p.task.state {
        return None;
    }
    let close = log.iter().find(|e| e.id == *id)?;
    let verdict = core::verdict(n, log, core::entry_text(n, &close.body));
    crate::display::close_outcome(Some(&verdict), None)
        .starts_with("delivered")
        .then_some(*id)
}

/// The summary's deploy lines of a closed task: `Merged, waiting for deploy` before its first record after the
/// close, then the first record of each configured environment in deploy order. None for any other task.
pub fn lines(n: u64, log: &[Entry], d: &Deploy) -> Vec<String> {
    let Some(close) = delivered(n, log) else {
        return vec![];
    };
    let recs: Vec<Record> = records(n, log)
        .into_iter()
        .filter(|r| r.id > close)
        .collect();
    if recs.is_empty() {
        return vec![WAITING.into()];
    }
    d.envs
        .iter()
        .filter_map(|env| recs.iter().find(|r| &r.env == env))
        .map(|r| {
            let mut l = format!("{} {} · {} · {}", d.project, r.version, r.env, r.at);
            if let Some(h) = &r.host {
                l.push_str(&format!(" · {h}"));
            }
            l
        })
        .collect()
}

/// Whether task `n` waits for its first deploy (§ 2).
pub fn waiting(n: u64, log: &[Entry], d: &Deploy) -> bool {
    lines(n, log, d).first().is_some_and(|l| l == WAITING)
}

/// Sets or removes `deploy-waiting` on closed issue `i` to match its log; no write when it already matches.
pub fn label(b: &dyn Board, i: &Issue, log: &[Entry], d: Option<&Deploy>) -> Res<()> {
    let want = d.is_some_and(|d| waiting(i.number, log, d));
    let has = i.labels.iter().any(|l| l == LABEL);
    match (want, has) {
        (true, false) => b
            .labels(i.number, &[LABEL.to_string()], &[])
            .map_err(String::from),
        (false, true) => b
            .labels(i.number, &[], &[LABEL.to_string()])
            .map_err(String::from),
        _ => Ok(()),
    }
}

/// The task numbers of this board that `Task: <url>` trailers of `messages` name, and the other URLs (skipped).
pub fn tasks(b: &dyn Board, messages: &str) -> (BTreeSet<u64>, BTreeSet<String>) {
    let (mut mine, mut other) = (BTreeSet::new(), BTreeSet::new());
    for url in messages
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Task:"))
        .map(|u| u.trim().trim_end_matches('/'))
        .filter(|u| !u.is_empty())
    {
        let n = url
            .rsplit('/')
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .filter(|n| *n > 0);
        match n.filter(|n| b.url(*n).trim_end_matches('/') == url) {
            Some(n) => {
                mine.insert(n);
            }
            None => {
                other.insert(url.to_string());
            }
        }
    }
    (mine, other)
}

/// The commit messages of `range` (`A..B`) in `root`; a missing end fetches tags once, then reads again.
fn messages(root: &Path, range: &str) -> Res<String> {
    let git = |args: &[&str]| {
        crate::platform::command("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))
    };
    let log = || git(&["log", "--format=%B", range, "--"]);
    let mut o = log()?;
    if !o.status.success() {
        let _ = git(&["fetch", "--quiet", "--tags", "origin"]);
        o = log()?;
    }
    if !o.status.success() {
        return Err(format!(
            "git log {range}: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `A..B` with two nonempty ends, neither an option.
fn range(r: &str) -> bool {
    r.split_once("..").is_some_and(|(a, b)| {
        !a.is_empty()
            && !b.is_empty()
            && !a.starts_with('-')
            && !b.starts_with('.')
            && !r.contains(char::is_whitespace)
    })
}

/// `taskq deployed`: one `deployed` entry per closed task the range's `Task:` trailers name (§ 2).
pub fn run(
    b: &dyn Board,
    root: &Path,
    version: &str,
    env: &str,
    range_arg: &str,
    host: Option<&str>,
) -> Res<Value> {
    if !word(version) || !word(env) || host.is_some_and(|h| !word(h)) {
        return Err(
            "deployed: --version, --env and --host are single words (letters, digits, . _ - +)"
                .into(),
        );
    }
    if !range(range_arg) {
        return Err("deployed: --range is A..B, two commits or tags".into());
    }
    let d = config(root)
        .ok_or("deployed: taskq.json has no `deploy` environments (taskq.md section 9)")?;
    if !d.envs.iter().any(|e| e == env) {
        return Err(format!(
            "deployed: --env {env} is not in taskq.json deploy ({})",
            d.envs.join(", ")
        ));
    }
    let (mine, other) = tasks(b, &messages(root, range_arg)?);
    record(b, &d, &mine, other, version, env, host)
}

/// Writes the record to each of `tasks`; `skipped` are the trailers of other boards.
pub fn record(
    b: &dyn Board,
    d: &Deploy,
    tasks: &BTreeSet<u64>,
    skipped: BTreeSet<String>,
    version: &str,
    env: &str,
    host: Option<&str>,
) -> Res<Value> {
    let at = crate::time::stamp(b.now()?);
    let rows: Vec<Value> = tasks
        .iter()
        .map(|n| one(b, d, *n, version, env, host, &at))
        .collect();
    let mut skipped: Vec<Value> = skipped
        .into_iter()
        .map(|u| json!({"url": u, "reason": "not a task of this board"}))
        .collect();
    skipped.extend(rows.iter().filter(|r| r["outcome"] == "skipped").cloned());
    let rows: Vec<Value> = rows
        .into_iter()
        .filter(|r| r["outcome"] != "skipped")
        .collect();
    Ok(json!({"version": version, "env": env, "host": host, "tasks": rows, "skipped": skipped}))
}

fn one(
    b: &dyn Board,
    d: &Deploy,
    n: u64,
    version: &str,
    env: &str,
    host: Option<&str>,
    at: &str,
) -> Value {
    let op = op_id(env, version, host);
    let row = |outcome: &str, comment: Option<u64>, reason: &str| json!({"issue": n, "url": b.url(n), "outcome": outcome, "comment": comment, "reason": reason, "op_id": op});
    let issue = match b.issue(n) {
        Ok(i) => i,
        Err(e) => return row("unknown", None, e.text()),
    };
    if issue.open || !crate::service::is_task(&issue) {
        return row(
            "skipped",
            None,
            if issue.open {
                "task is open"
            } else {
                "not a taskq task"
            },
        );
    }
    let read = |b: &dyn Board| {
        b.comments(n)
            .and_then(|cs| board::reply::log(n, &cs).map(|log| (cs, log)))
    };
    let mine = |log: &[Entry]| {
        log.iter()
            .find(|e| !e.edited && core::op_id(n, &e.body) == Some(op.as_str()))
            .map(|e| e.id)
    };
    let (comments, log) = match read(b) {
        Ok(x) => x,
        Err(e) => return row("unknown", None, e.text()),
    };
    let (outcome, id, comments, log) = match mine(&log) {
        Some(id) => ("repeat", id, comments, log),
        None => {
            let mut body = format!("taskq: {KIND} {env} {version}\nat {at}");
            if let Some(h) = host {
                body.push_str(&format!("\nhost {h}"));
            }
            let sent = b.comment(n, &core::with_fields(body, Some(&op), None));
            match (sent, read(b)) {
                (_, Ok((cs, log))) if mine(&log).is_some() => {
                    ("accepted", mine(&log).unwrap_or_default(), cs, log)
                }
                (Err(e), _) | (Ok(_), Err(e)) => {
                    return row("unknown", None, &format!("{e}; retry the same command"));
                }
                (Ok(id), Ok(_)) => {
                    return row(
                        "unknown",
                        Some(id),
                        "entry not read back; retry the same command",
                    );
                }
            }
        }
    };
    // § 2: the label and the summary project the log; a failed write is the next run's or pass's to repair
    let mut reason = String::new();
    if let Err(e) = label(b, &issue, &log, Some(d)) {
        reason = format!("label not written: {e}");
    }
    if b.summaries() {
        let t = core::fold(n, &log, false);
        if let Err(e) = crate::summary::write(b, n, &issue.body, &comments, &log, &t, Some(d)) {
            reason = format!("summary not written: {e}");
        }
    }
    row(outcome, Some(id), &reason)
}

/// `Waiting for deploy: N tasks, oldest <age>` and one linked line per task, oldest first; none when no task waits.
pub fn status(b: &dyn Board, d: Option<&Deploy>) -> Res<Vec<String>> {
    let Some(d) = d else { return Ok(vec![]) };
    let mut rows = b.closed_labelled(LABEL)?;
    rows.retain(|(i, _)| crate::service::is_task(i) && !i.open);
    if rows.is_empty() {
        return Ok(vec![]);
    }
    rows.sort_by_key(|(i, closed)| (*closed, i.number));
    let now = b.now()?;
    let tasks = if rows.len() == 1 {
        "1 task".to_string()
    } else {
        format!("{} tasks", rows.len())
    };
    let mut out = vec![format!(
        "Waiting for deploy: {tasks}, oldest {} ({})",
        crate::time::age(now - rows[0].1),
        d.project
    )];
    out.extend(rows.iter().map(|(i, closed)| {
        format!(
            "  #{} {} · closed {} ago · {}",
            i.number,
            i.title,
            crate::time::age(now - closed),
            b.url(i.number)
        )
    }));
    Ok(out)
}

/// The release flow (§ 9): a host that applied `new` over `old` records it in each root of the release repository
/// with `deploy`, as a detached `taskq deployed` so a re-exec never waits for the board. Errors are logged.
pub fn applied(
    exe: &Path,
    roots: &[std::path::PathBuf],
    channel: &str,
    old: &str,
    new: &str,
    host: &str,
) -> Vec<String> {
    let mut log = vec![];
    if old == new || !word(old) || !word(new) || !word(host) {
        return log;
    }
    for root in roots {
        let release = crate::config::project_value(root)
            .ok()
            .is_some_and(|v| v["repo"].as_str() == Some(crate::platform::RELEASE_REPO));
        if !release || config(root).is_none_or(|d| !d.envs.iter().any(|e| e == channel)) {
            continue;
        }
        let args = [
            "deployed",
            "--version",
            new,
            "--env",
            channel,
            "--range",
            &format!("v{old}..v{new}"),
            "--host",
            host,
        ];
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crate::paths::dot(root).join("deployed.log"));
        let mut cmd = crate::platform::command(exe);
        cmd.args(args)
            .current_dir(root)
            .env("TASKQ_ROOT", root)
            .stdin(std::process::Stdio::null());
        if let Ok(f) = out {
            if let Ok(g) = f.try_clone() {
                cmd.stdout(f).stderr(g);
            }
        }
        match cmd.spawn() {
            Ok(_) => log.push(format!(
                "deployed {new} ({channel}) recorded from v{old}..v{new}; see .taskq/deployed.log"
            )),
            Err(e) => log.push(format!("deployed {new} not recorded: {e}")),
        }
    }
    log
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod tests;
