//! § 2 Deploy: `deployed` entries on closed tasks, the summary's deploy lines, the `deploy-waiting` label and the
//! status line. `fold` ignores the kind, so a closed task stays closed (R1).
use crate::{Res, board::{self, Board, Issue}, core::{self, Entry, State}};
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
    parse(&crate::project_value(root).ok()?)
}

/// `deploy` of a taskq.json value: a nonempty list of distinct words; anything else reads as absent.
pub fn parse(v: &Value) -> Option<Deploy> {
    let envs: Vec<String> = v["deploy"].as_array()?.iter().map(|e| e.as_str().filter(|s| word(s)).map(String::from)).collect::<Option<_>>()?;
    let distinct: BTreeSet<&String> = envs.iter().collect();
    let project = v["repo"].as_str()?.rsplit('/').next().filter(|p| !p.is_empty())?.to_string();
    (!envs.is_empty() && distinct.len() == envs.len()).then_some(Deploy { project, envs })
}

/// A version, environment or host name: one word of letters, digits and `.`, `_`, `-`, `+`.
pub fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
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
    host.map_or(format!("deployed-{env}-{version}"), |h| format!("deployed-{env}-{version}-{h}"))
}

/// The unedited `deployed` entries of task `n` in board order; malformed ones are skipped.
pub fn records(n: u64, log: &[Entry]) -> Vec<Record> {
    let mut entries: Vec<&Entry> = log.iter().filter(|e| !e.edited).collect();
    entries.sort_by_key(|e| e.id);
    entries.into_iter().filter_map(|e| {
        let head = e.body.lines().next()?.trim().strip_prefix("taskq:")?;
        let [kind, env, version] = head.split_whitespace().collect::<Vec<_>>()[..] else { return None };
        if kind != KIND || !word(env) || !word(version) { return None; }
        let text = core::entry_text(n, &e.body);
        let at = text.lines().find_map(|l| l.trim().strip_prefix("at ")).map(str::trim).filter(|t| core::utc(t))?.to_string();
        let host = text.lines().find_map(|l| l.trim().strip_prefix("host ")).map(str::trim).filter(|h| word(h)).map(String::from);
        Some(Record { id: e.id, env: env.into(), version: version.into(), at, host })
    }).collect()
}

/// The accepted `close` of a task closed from `review` as delivered (§ 7): only such a close waits for a deploy.
pub fn delivered(n: u64, log: &[Entry]) -> Option<u64> {
    let p = core::project(n, log, false);
    let (id, before, after) = p.changes.iter().rev().find(|(_, _, t)| t.state == State::Closed)?;
    if before.state != State::Review || after.state != p.task.state { return None; }
    let close = log.iter().find(|e| e.id == *id)?;
    let verdict = core::verdict(n, log, core::entry_text(n, &close.body));
    crate::display::close_outcome(Some(&verdict), None).starts_with("delivered").then_some(*id)
}

/// The summary's deploy lines of a closed task: `Merged, waiting for deploy` before its first record after the
/// close, then the first record of each configured environment in deploy order. None for any other task.
pub fn lines(n: u64, log: &[Entry], d: &Deploy) -> Vec<String> {
    let Some(close) = delivered(n, log) else { return vec![] };
    let recs: Vec<Record> = records(n, log).into_iter().filter(|r| r.id > close).collect();
    if recs.is_empty() { return vec![WAITING.into()]; }
    d.envs.iter().filter_map(|env| recs.iter().find(|r| &r.env == env)).map(|r| {
        let mut l = format!("{} {} · {} · {}", d.project, r.version, r.env, r.at);
        if let Some(h) = &r.host { l.push_str(&format!(" · {h}")); }
        l
    }).collect()
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
        (true, false) => b.labels(i.number, &[LABEL.to_string()], &[]),
        (false, true) => b.labels(i.number, &[], &[LABEL.to_string()]),
        _ => Ok(()),
    }
}

/// The task numbers of this board that `Task: <url>` trailers of `messages` name, and the other URLs (skipped).
pub fn tasks(b: &dyn Board, messages: &str) -> (BTreeSet<u64>, BTreeSet<String>) {
    let (mut mine, mut other) = (BTreeSet::new(), BTreeSet::new());
    for url in messages.lines().filter_map(|l| l.trim().strip_prefix("Task:")).map(|u| u.trim().trim_end_matches('/')).filter(|u| !u.is_empty()) {
        let n = url.rsplit('/').next().and_then(|n| n.parse::<u64>().ok()).filter(|n| *n > 0);
        match n.filter(|n| b.url(*n).trim_end_matches('/') == url) {
            Some(n) => { mine.insert(n); }
            None => { other.insert(url.to_string()); }
        }
    }
    (mine, other)
}

/// The commit messages of `range` (`A..B`) in `root`; a missing end fetches tags once, then reads again.
fn messages(root: &Path, range: &str) -> Res<String> {
    let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(root).args(args).output().map_err(|e| format!("git: {e}"));
    let log = || git(&["log", "--format=%B", range, "--"]);
    let mut o = log()?;
    if !o.status.success() {
        let _ = git(&["fetch", "--quiet", "--tags", "origin"]);
        o = log()?;
    }
    if !o.status.success() { return Err(format!("git log {range}: {}", String::from_utf8_lossy(&o.stderr).trim())); }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `A..B` with two nonempty ends, neither an option.
fn range(r: &str) -> bool {
    r.split_once("..").is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && !a.starts_with('-') && !b.starts_with('.') && !r.contains(char::is_whitespace))
}

/// `taskq deployed`: one `deployed` entry per closed task the range's `Task:` trailers name (§ 2).
pub fn run(b: &dyn Board, root: &Path, version: &str, env: &str, range_arg: &str, host: Option<&str>) -> Res<Value> {
    if !word(version) || !word(env) || host.is_some_and(|h| !word(h)) {
        return Err("deployed: --version, --env and --host are single words (letters, digits, . _ - +)".into());
    }
    if !range(range_arg) { return Err("deployed: --range is A..B, two commits or tags".into()); }
    let d = config(root).ok_or("deployed: taskq.json has no `deploy` environments (taskq.md section 9)")?;
    if !d.envs.iter().any(|e| e == env) { return Err(format!("deployed: --env {env} is not in taskq.json deploy ({})", d.envs.join(", "))); }
    let (mine, other) = tasks(b, &messages(root, range_arg)?);
    record(b, &d, &mine, other, version, env, host)
}

/// Writes the record to each of `tasks`; `skipped` are the trailers of other boards.
pub fn record(b: &dyn Board, d: &Deploy, tasks: &BTreeSet<u64>, skipped: BTreeSet<String>, version: &str, env: &str, host: Option<&str>) -> Res<Value> {
    let at = board::stamp(b.now()?);
    let rows: Vec<Value> = tasks.iter().map(|n| one(b, d, *n, version, env, host, &at)).collect();
    let mut skipped: Vec<Value> = skipped.into_iter().map(|u| json!({"url": u, "reason": "not a task of this board"})).collect();
    skipped.extend(rows.iter().filter(|r| r["outcome"] == "skipped").cloned());
    let rows: Vec<Value> = rows.into_iter().filter(|r| r["outcome"] != "skipped").collect();
    Ok(json!({"version": version, "env": env, "host": host, "tasks": rows, "skipped": skipped}))
}

fn one(b: &dyn Board, d: &Deploy, n: u64, version: &str, env: &str, host: Option<&str>, at: &str) -> Value {
    let op = op_id(env, version, host);
    let row = |outcome: &str, comment: Option<u64>, reason: &str| json!({"issue": n, "url": b.url(n), "outcome": outcome, "comment": comment, "reason": reason, "op_id": op});
    let issue = match b.issue(n) {
        Ok(i) => i,
        Err(e) => return row("unknown", None, &e),
    };
    if issue.open || !crate::service::is_task(&issue) {
        return row("skipped", None, if issue.open { "task is open" } else { "not a taskq task" });
    }
    let read = |b: &dyn Board| b.comments(n).and_then(|cs| board::reply::log(n, &cs).map(|log| (cs, log)));
    let mine = |log: &[Entry]| log.iter().find(|e| !e.edited && core::op_id(n, &e.body) == Some(op.as_str())).map(|e| e.id);
    let (comments, log) = match read(b) {
        Ok(x) => x,
        Err(e) => return row("unknown", None, &e),
    };
    let (outcome, id, comments, log) = match mine(&log) {
        Some(id) => ("repeat", id, comments, log),
        None => {
            let mut body = format!("taskq: {KIND} {env} {version}\nat {at}");
            if let Some(h) = host { body.push_str(&format!("\nhost {h}")); }
            let sent = b.comment(n, &core::with_fields(body, Some(&op), None));
            match (sent, read(b)) {
                (_, Ok((cs, log))) if mine(&log).is_some() => ("accepted", mine(&log).unwrap_or_default(), cs, log),
                (Err(e), _) | (Ok(_), Err(e)) => return row("unknown", None, &format!("{e}; retry the same command")),
                (Ok(id), Ok(_)) => return row("unknown", Some(id), "entry not read back; retry the same command"),
            }
        }
    };
    // § 2: the label and the summary project the log; a failed write is the next run's or pass's to repair
    let mut reason = String::new();
    if let Err(e) = label(b, &issue, &log, Some(d)) { reason = format!("label not written: {e}"); }
    if b.summaries() {
        let t = core::fold(n, &log, false);
        if let Err(e) = crate::summary::write(b, n, &issue.body, &comments, &log, &t, Some(d)) { reason = format!("summary not written: {e}"); }
    }
    row(outcome, Some(id), &reason)
}

/// `Waiting for deploy: N tasks, oldest <age>` and one linked line per task, oldest first; none when no task waits.
pub fn status(b: &dyn Board, d: Option<&Deploy>) -> Res<Vec<String>> {
    let Some(d) = d else { return Ok(vec![]) };
    let mut rows = b.closed_labelled(LABEL)?;
    rows.retain(|(i, _)| crate::service::is_task(i) && !i.open);
    if rows.is_empty() { return Ok(vec![]); }
    rows.sort_by_key(|(i, closed)| (*closed, i.number));
    let now = b.now()?;
    let tasks = if rows.len() == 1 { "1 task".to_string() } else { format!("{} tasks", rows.len()) };
    let mut out = vec![format!("Waiting for deploy: {tasks}, oldest {} ({})", crate::display::age(now - rows[0].1), d.project)];
    out.extend(rows.iter().map(|(i, closed)| format!("  #{} {} · closed {} ago · {}", i.number, i.title, crate::display::age(now - closed), b.url(i.number))));
    Ok(out)
}

/// The release flow (§ 9): a host that applied `new` over `old` records it in each root of the release repository
/// with `deploy`, as a detached `taskq deployed` so a re-exec never waits for the board. Errors are logged.
pub fn applied(exe: &Path, roots: &[std::path::PathBuf], channel: &str, old: &str, new: &str, host: &str) -> Vec<String> {
    let mut log = vec![];
    if old == new || !word(old) || !word(new) || !word(host) { return log; }
    for root in roots {
        let release = crate::project_value(root).ok().is_some_and(|v| v["repo"].as_str() == Some(crate::platform::RELEASE_REPO));
        if !release || config(root).is_none_or(|d| !d.envs.iter().any(|e| e == channel)) { continue; }
        let args = ["deployed", "--version", new, "--env", channel, "--range", &format!("v{old}..v{new}"), "--host", host];
        let out = std::fs::OpenOptions::new().create(true).append(true).open(crate::dot(root).join("deployed.log"));
        let mut cmd = std::process::Command::new(exe);
        cmd.args(args).current_dir(root).env("TASKQ_ROOT", root).stdin(std::process::Stdio::null());
        if let Ok(f) = out { if let Ok(g) = f.try_clone() { cmd.stdout(f).stderr(g); } }
        match cmd.spawn() {
            Ok(_) => log.push(format!("deployed {new} ({channel}) recorded from v{old}..v{new}; see .taskq/deployed.log")),
            Err(e) => log.push(format!("deployed {new} not recorded: {e}")),
        }
    }
    log
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Fake;

    fn deploy() -> Deploy { Deploy { project: "taskq".into(), envs: vec!["dev".into(), "live".into()] } }

    fn entry(id: u64, body: &str) -> Entry { Entry { id, body: body.into(), edited: false } }

    /// The entries after `spawn` (id `attempt`) of a task closed from review with `changed` as its Changed line.
    fn tail(changed: &str, attempt: u64) -> Vec<String> {
        let sha = "a".repeat(40);
        vec![
            format!("taskq: result {sha} attempt {attempt}
Changed: {changed}
Open: none"),
            format!("taskq: close
Changed: {changed}
Open: none
delivery auto-merge r0: merged #7"),
        ]
    }

    fn closed(changed: &str) -> Vec<Entry> {
        std::iter::once("taskq: spawn worker claude:s1 mac".to_string()).chain(tail(changed, 1)).enumerate().map(|(k, b)| entry(k as u64 + 1, &b)).collect()
    }

    fn board() -> Fake {
        let b = Fake::default();
        b.state.borrow_mut().summaries = true;
        b
    }

    fn add_closed(b: &Fake, title: &str, changed: &str) -> u64 {
        let n = b.create(title, "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker claude:s1 mac").unwrap();
        for body in tail(changed, a) { b.comment(n, &body).unwrap(); }
        b.close(n).unwrap();
        n
    }

    fn rec(id: u64, env: &str, version: &str, at: &str, host: Option<&str>) -> Entry {
        let mut body = format!("taskq: deployed {env} {version}\nat {at}");
        if let Some(h) = host { body.push_str(&format!("\nhost {h}")); }
        entry(id, &core::with_fields(body, Some(&op_id(env, version, host)), None))
    }

    // Chain: R1, § 2 Deploy -> fold ignores `deployed` (N-1 too) -> a closed task stays closed with the same fold.
    // Risk: a deploy record reopens a task or changes its fold, so its dependents or labels move.
    #[test]
    fn deployed_entries_never_change_a_closed_fold() {
        let log = closed("the menu shows deploys");
        let before = core::fold(9, &log, false);
        assert_eq!(before.state, State::Closed);
        let mut after = log.clone();
        after.push(rec(4, "live", "4.2.0", "2026-10-31T12:00:00Z", Some("mac")));
        after.push(entry(5, "taskq: deployed dev 4.2.0 attempt 1\nat 2026-10-31T12:00:00Z"));
        assert_eq!(core::fold(9, &after, false), before);
        let p = core::project(9, &after, false);
        assert!(!p.accepts(4) && !p.accepts(5), "a deployed entry is never an accepted state entry");
    }

    // Chain: § 2 Deploy summary -> waiting line, then one line per environment in configured order, first entry wins.
    // Risk: lines in board order instead of deploy order, a repeat shown twice, or a no-change close waiting forever.
    #[test]
    fn summary_lines_follow_the_configured_environment_order() {
        let d = deploy();
        let mut log = closed("the menu shows deploys");
        assert_eq!(lines(9, &log, &d), vec![WAITING.to_string()]);
        assert!(waiting(9, &log, &d));
        log.push(rec(4, "live", "4.2.0", "2026-10-31T12:00:00Z", Some("mac")));
        log.push(rec(5, "dev", "4.2.0-dev.3", "2026-10-30T09:00:00Z", None));
        log.push(rec(6, "live", "4.2.1", "2026-11-01T08:00:00Z", Some("win")));
        log.push(rec(7, "staging", "4.2.1", "2026-11-01T08:00:00Z", None));
        assert_eq!(lines(9, &log, &d), vec![
            "taskq 4.2.0-dev.3 · dev · 2026-10-30T09:00:00Z".to_string(),
            "taskq 4.2.0 · live · 2026-10-31T12:00:00Z · mac".to_string(),
        ]);
        assert!(!waiting(9, &log, &d));
        assert!(lines(9, &closed("none: already on main"), &d).is_empty(), "a no-change close shows no deploy line");
        let open = &closed("x")[..2];
        assert!(lines(9, open, &d).is_empty(), "an open task shows none");
    }

    #[test]
    fn deploy_setting_reads_words_in_order_or_nothing() {
        assert_eq!(parse(&json!({"repo": "o/taskq", "deploy": ["dev", "live"]})), Some(deploy()));
        for bad in [json!({"repo": "o/taskq"}), json!({"repo": "o/taskq", "deploy": []}), json!({"repo": "o/taskq", "deploy": ["dev", "dev"]}),
            json!({"repo": "o/taskq", "deploy": ["two words"]}), json!({"repo": "o/taskq", "deploy": "live"})] {
            assert_eq!(parse(&bad), None, "{bad}");
        }
        assert!(range("v4.1.0..v4.2.0") && !range("v4.2.0") && !range("..v4") && !range("-x..y") && !range("a...b"));
    }

    // A6: the release flow records only in a release root whose `deploy` names the channel. Risk: the tracked
    // taskq.json loses `deploy` or `live`, and a live host update records nothing.
    #[test]
    fn release_root_records_live_deploys() {
        let v: Value = serde_json::from_str(include_str!("../taskq.json")).unwrap();
        assert_eq!(v["repo"].as_str(), Some(crate::platform::RELEASE_REPO));
        assert!(parse(&v).is_some_and(|d| d.envs.iter().any(|e| e == "live")), "taskq.json deploy must name live");
    }

    // Chain: A2 `taskq deployed` -> one entry per trailer task of this board, a rerun is a no-op by op id, the label
    // and summary follow. Risk: a rerun posts twice, another board's task is written, or the waiting label stays.
    #[test]
    fn record_is_idempotent_and_skips_other_boards() {
        let b = board();
        let d = deploy();
        let n = add_closed(&b, "Show deploys", "the menu shows deploys");
        let m = add_closed(&b, "No change", "none: nothing to do");
        let open = b.create("Still open", "", &["taskq".into()]).unwrap();
        let other = "https://github.com/other/repo/issues/5";
        let msgs = format!("fix: x\n\nTask: {}\n\nTask: {}\nTask: {other}\nTask: {}\n", b.url(n), b.url(m), b.url(open));
        let (mine, skip) = tasks(&b, &msgs);
        assert_eq!(mine.iter().copied().collect::<Vec<_>>(), vec![n, m, open]);
        b.labels(n, &[LABEL.into()], &[]).unwrap();
        let first = record(&b, &d, &mine, skip.clone(), "4.2.0", "live", Some("mac")).unwrap();
        assert_eq!(first["tasks"].as_array().unwrap().iter().map(|r| r["outcome"].as_str().unwrap()).collect::<Vec<_>>(), ["accepted", "accepted"]);
        let skipped: Vec<&str> = first["skipped"].as_array().unwrap().iter().map(|r| r["url"].as_str().unwrap()).collect();
        assert_eq!(skipped, [other.to_string(), b.url(open)]);
        let count = |n: u64| b.comments(n).unwrap().iter().filter(|c| c.body.starts_with("taskq: deployed")).count();
        assert_eq!((count(n), count(m), count(open)), (1, 1, 0));
        assert!(!b.issue(n).unwrap().labels.iter().any(|l| l == LABEL), "the first record removes the waiting label");
        let summary = b.comments(n).unwrap().into_iter().find(crate::summary::is).unwrap().body;
        assert!(summary.contains("\ntaskq 4.2.0 · live · ") && summary.ends_with(" · mac"), "{summary}");
        let again = record(&b, &d, &mine, skip, "4.2.0", "live", Some("mac")).unwrap();
        assert!(again["tasks"].as_array().unwrap().iter().all(|r| r["outcome"] == "repeat"), "{again}");
        assert_eq!((count(n), count(m)), (1, 1), "a rerun with the same version and environment writes nothing");
    }

    // Chain: A4 status -> closed tasks labelled deploy-waiting, oldest first, each linked.
    #[test]
    fn status_lists_waiting_tasks_oldest_first() {
        let b = board();
        let d = deploy();
        assert!(status(&b, Some(&d)).unwrap().is_empty());
        let n = add_closed(&b, "Older", "x");
        let m = add_closed(&b, "Newer", "y");
        for k in [m, n] { b.labels(k, &[LABEL.into()], &[]).unwrap(); }
        b.advance(4 * 86400);
        let now = b.now().unwrap();
        b.state.borrow_mut().report_closed.extend([(n, now - 3 * 86400), (m, now - 7200)]);
        let lines = status(&b, Some(&d)).unwrap();
        assert_eq!(lines[0], "Waiting for deploy: 2 tasks, oldest 3d (taskq)");
        assert!(lines[1].starts_with(&format!("  #{n} Older · closed 3d ago · {}", b.url(n))), "{lines:?}");
        assert!(lines[2].starts_with(&format!("  #{m} Newer")), "{lines:?}");
        assert!(status(&b, None).unwrap().is_empty(), "no deploy setting, no line");
    }
}
