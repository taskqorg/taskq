//! The host service (taskq.md § 3, § 4): one loop around `fold`. It owns this host's children and its heartbeat issue;
//! everything it knows about other hosts comes from the board.

use crate::board::{Board, Comment, Issue};
use crate::core::{self, State, Task};
use crate::runtime::{Launch, Proc};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use crate::admission::{Entry, Res, Want};
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub install: PathBuf,
    pub board: &'static crate::board::Kind,
    pub web_host: String,
    pub repo: String,
    /// Publication mode `direct|pr`, from the delivery preset (§ 7).
    pub publish: String,
    /// § 7 delivery preset and its policy revision.
    pub delivery: String,
    pub delivery_revision: u64,
    pub me: String,
    pub limits: BTreeMap<String, u32>,
    /// What this host offers for `needs-<x>` labels.
    pub has: Vec<String>,
    /// Host task-claim mode (§4/§9); manager/owner turns are independent.
    pub claim: crate::host::ClaimMode,
    pub spillover_grace: u64,
    pub allowed_runtimes: Option<Vec<String>>,
    pub auto_reroute: bool,
    /// host.json `default_runtime` (§ 9): the runtime of reviewers and of workers without `run-*` (§ 4).
    pub default_runtime: String,
    pub routing: Vec<crate::labels::Routing>,
    pub permission_mode: String,
    pub codex: Vec<String>,
    pub ttl: i64,
    pub poll: u64,
    pub update_every: u64,
    pub rollback_after: u64,
    /// `workspace: external` (§ 9): the hourly tidy skips unowned worktrees and touches only TaskQ's own task worktrees.
    pub external: bool,
    /// `host_setup` (§ 6 Shared project bootstrap): the project's one-time host setup command, run by `service install`.
    pub host_setup: Option<String>,
}

pub const HOST_LABEL: &str = "taskq-host";
/// The capacity reason of an empty focus (§ 4).
pub const FOCUS_OFF: &str = "focus off: nothing admitted; set `taskq service focus all|<label..>`";
pub const TASK_LABEL: &str = "taskq";
/// #1935: the first words of the one board comment on a PR that waits for a person to merge it.
const MANUAL_MERGE_NOTE: &str = "Waiting for a manual merge";
const NATIVE_PASS_BUDGET:std::time::Duration=std::time::Duration::from_secs(20);

/// The code the host's tray wrote on an interface fault (#1755) until a new tray clears it: a fixed code, else none.
fn tray_fault(install: &std::path::Path) -> Option<String> {
    let code = std::fs::read_to_string(install.join("tray.fault")).ok()?;
    let code = code.trim();
    (!code.is_empty() && code.len() <= 40 && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')).then(|| code.to_string())
}

/// docs/design/worktrees.md Findings: one held finding per root and tidy condition that has lasted over a day, nothing
/// more while that task is open. `.taskq/findings.json` keeps `{condition: {since, issue}}` and the last reason a
/// finding could not be filed, so the reason logs once (a cache, R1).
fn tidy_findings(b: &dyn Board, cfg: &Config, conditions: &BTreeMap<&'static str, Vec<String>>, now: u64, log: &mut Vec<String>) {
    const DAY: u64 = 86_400;
    let path = crate::dot(&cfg.root).join("findings.json");
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object);
    let last = read(&path).unwrap_or_else(|| json!({}));
    let mut next = json!({});
    if let Some(reason) = last.get("unfiled") { next["unfiled"] = reason.clone(); }
    // a source part: 1-48 of `[A-Za-z0-9._-]`, never only dots
    let part = |s: &str| -> String {
        let p: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).take(48).collect();
        if p.bytes().all(|c| c == b'.') { "x".into() } else { p }
    };
    let (host, project) = (part(&cfg.me), part(&crate::project_name(&cfg.root)));
    for (cond, lines) in conditions {
        let mut entry = last.get(*cond).cloned().filter(|e| e["since"].is_u64()).unwrap_or_else(|| json!({"since": now}));
        if let Some(n) = entry["issue"].as_u64() {
            // open or unreadable: nothing more; closed while the condition holds: a new day starts
            if b.issue(n).is_ok_and(|i| !i.open) { entry = json!({"since": now}); }
            next[*cond] = entry;
            continue;
        }
        let since = entry["since"].as_u64().unwrap_or(now);
        if now.saturating_sub(since) > DAY {
            let (what, goal) = match *cond {
                "dirty" => ("Task worktree dirty", format!("A task worktree on host {} stays dirty while no child of that host runs its task: keep or drop its changes so the tidy pass can remove it.", cfg.me)),
                "stale" => ("Root stale", format!("The project root on host {} has not followed origin/main for over a day: settle the stale reason so the tidy pass can fast-forward it.", cfg.me)),
                _ => ("Closed task worktree kept", format!("A closed task's worktree on host {} stays after each tidy pass: settle its kept reason so the worktree goes.", cfg.me)),
            };
            let new = crate::NewTask {
                conversation: None,
                title: format!("{what} over a day: {project} on {host}").chars().take(80).collect(),
                goal,
                acceptance: vec![format!("taskq cleanup --dry-run on host {} no longer prints the evidence lines of this task.", cfg.me)],
                scope: vec![], deps: vec![], kind: None, runtime: None, priority: None, host: None, labels: vec![],
            };
            let evidence: Vec<String> = lines.iter().take(4).map(|l| l.replace(['\r', '\n'], " ").chars().take(238).collect()).collect();
            let source = format!("{host}/tidy-{cond}/{project}-{since}");
            let raw = read(&cfg.root.join("taskq.json")).unwrap_or(Value::Null);
            let mut op = crate::dispatch::Op::default();
            let reason = match crate::submit_finding(b, &raw, &cfg.root, cfg.board.name, &cfg.web_host, &cfg.repo, &new, &source, &evidence, &mut op, false) {
                Ok((r, _)) if r.ok() => {
                    entry["issue"] = json!(r.task);
                    log.push(format!("#{} finding filed: tidy {cond} over a day", r.task));
                    None
                }
                Ok((r, _)) => Some(r.reason),
                Err(e) => Some(e),
            };
            match reason {
                Some(r) => {
                    let line = format!("tidy finding not filed: {r}");
                    if last["unfiled"].as_str() != Some(&line) { log.push(line.clone()); }
                    next["unfiled"] = json!(line);
                }
                None => { next.as_object_mut().unwrap().remove("unfiled"); }
            }
        }
        next[*cond] = entry;
    }
    if next != last && let Err(e) = std::fs::create_dir_all(crate::dot(&cfg.root)).and_then(|_| std::fs::write(&path, next.to_string())) {
        log.push(format!("findings cache: {e}"));
    }
}

/// Only issues labelled `taskq` are tasks (§ 1): a user issue never starts by itself.
/// Heartbeat lines a write compares: the `at` stamp and diagnostics observation times change every pass (#1358).
fn stable(body: &str) -> Vec<String> {
    body.lines().filter(|l| !l.starts_with("at ")).map(|l| match l.strip_prefix("diagnostics ").and_then(|t| serde_json::from_str::<Value>(t).ok()) {
        Some(Value::Object(mut d)) => {
            d.remove("observed");
            d.values_mut().filter_map(Value::as_object_mut).for_each(|f| { f.remove("at"); });
            format!("diagnostics {}", Value::Object(d))
        }
        _ => l.to_string(),
    }).collect()
}

/// #1616: a host admission refusal (`admission.rs` code, or `host.rs` reason under code `host`) as an admission code.
fn refusal_admission(code: &str, text: &str) -> (&'static str, String) {
    let has = |w: &str| text.contains(w);
    let code = match code {
        "max" => "capacity",
        "spacing" => "load-gate",
        _ if has("recovery pending") || has("poisoned") || has("admission file") => return ("unknown", crate::snapshot::ADMISSION_UNKNOWN[2].into()),
        _ if has("load gate") => "load-gate",
        _ if has("capacity") || has("round-robin") || has("disabled") => "capacity",
        // `version`, an operator hold, draining and a pending restart hold every launch
        _ => "hold",
    };
    (code, text.into())
}

/// R12/§4: task-facing effective availability, published on the existing per-board heartbeat.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Balancing {
    schema: u8,
    mode: String,
    default_runtime: String,
    allowed_runtimes: Option<Vec<String>>,
    auto_reroute: bool,
    has: Vec<String>,
    focus: Option<Vec<String>>,
    routing: Vec<crate::labels::Routing>,
    runtimes: BTreeMap<String, RuntimeAvailability>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeAvailability { code: String, reason: String, full: bool, disabled: bool, quota: Vec<String> }

impl Balancing {
    fn read(body: &str) -> Option<Self> {
        let rows:Vec<_>=body.lines().filter_map(|l|l.strip_prefix("balancing ")).collect();
        if rows.len()!=1 {return None;}
        let b:Self=serde_json::from_str(rows[0]).ok()?;
        if b.schema!=1 || crate::host::ClaimMode::parse(&b.mode).is_err()
            || !crate::runtime::CLI_RUNTIMES.contains(&b.default_runtime.as_str())
            || b.allowed_runtimes.as_ref().is_some_and(|a|a.iter().any(|rt|!RUNTIMES.contains(&rt.as_str())) || a.iter().collect::<BTreeSet<_>>().len()!=a.len())
            || crate::labels::routing(&json!({"routing":b.routing})).is_err()
            || b.runtimes.len()!=RUNTIMES.len() || RUNTIMES.iter().any(|rt|!b.runtimes.contains_key(*rt))
            || b.runtimes.values().any(|r|!crate::snapshot::ADMISSION_CODES.iter().any(|(c,_)|*c==r.code) || r.reason.chars().count()>160 || r.code=="admitted" && r.disabled) {return None;}
        Some(b)
    }

    fn available(&self,v:&View) -> Result<(String,Option<String>),(&'static str,String)> {
        let focus=self.focus.as_ref().ok_or(("unknown","primary focus unknown".into()))?;
        if priority(&v.issue)!=0 && !focus.iter().any(|f|f=="all" || crate::labels::matches(&v.issue.labels,f)) || focus.is_empty() {
            return Err(("focus","primary focus excludes task".into()));
        }
        if let Some(need)=v.issue.labels.iter().filter_map(|l|l.strip_prefix("needs-")).find(|n|*n!="gate" && !self.has.iter().any(|h|h==n)) {
            return Err(("capability",format!("primary lacks needs-{need}")));
        }
        // Assignment is private to the primary's board account, so the projection cannot prove eligibility.
        if v.issue.labels.iter().any(|l|l=="assignee-only") {return Err(("unknown","primary assignee eligibility unknown".into()));}
        let reviewer=v.projection.task.state==State::Review;
        let default=crate::labels::project_default(&v.issue.labels,reviewer,&self.routing,&self.default_runtime);
        crate::labels::select_project(&v.issue.labels,reviewer,default,self.auto_reroute,
            |rt| self.runtimes.get(rt).is_some_and(|r|r.disabled),
            |rt| {
                if self.allowed_runtimes.as_ref().is_some_and(|a|!a.iter().any(|r|r==rt)) {return Err(("capability",format!("{rt} excluded by primary project")));}
                let r=self.runtimes.get(rt).ok_or(("unknown","primary runtime availability missing".into()))?;
                if r.code=="unknown" {return Err(("unknown",r.reason.clone()));}
                if r.code!="admitted" {return Err(("capacity",format!("{}{}",r.reason,if r.quota.is_empty() {String::new()}else{format!("; {}",r.quota.join("; "))})));}
                if needs_gui(&v.issue) && !r.full {return Err(("capability",format!("{rt} has no primary Full grant")));}
                Ok(rt.into())
            })
    }
}

pub fn is_task(i: &Issue) -> bool {
    i.trusted && i.labels.iter().any(|l| l == TASK_LABEL)
}

/// A held task (§ 1): labelled `q-held` without `taskq`, so no version admits it; listed as `held` whatever its log.
pub fn is_held(i: &Issue) -> bool {
    i.trusted && !is_task(i) && i.labels.iter().any(|l| l == "q-held")
}

pub fn is_host(i: &Issue) -> bool {
    i.labels.iter().any(|l| l == HOST_LABEL) && i.title.starts_with("taskq host ")
}

/// The host name of a heartbeat issue (§ 4 step 3); `None` for other issues.
pub fn host_name(i: &Issue) -> Option<&str> {
    is_host(i).then(|| i.title.trim_start_matches("taskq host "))
}

/// `who` is an assignee; an issue listed without assignees (github.com, § 4 budget) costs one `issue(n)` read.
pub fn assigned(board: &dyn Board, i: &Issue, who: &str) -> Result<bool, String> {
    Ok(i.assignees.iter().any(|a| a == who) || i.assignees.is_empty() && board.issue(i.number)?.assignees.iter().any(|a| a == who))
}

fn needs_gui(i: &Issue) -> bool {
    i.labels.iter().any(|l| l == "needs-gui")
}

fn label(i: &Issue, prefix: &str) -> Option<String> {
    i.labels.iter().find_map(|l| l.strip_prefix(prefix).map(String::from))
}

fn route(i: &Issue, reviewer: bool, default: &str) -> String {
    crate::labels::route(&i.labels, reviewer, default)
}

pub fn priority(i: &Issue) -> u8 {
    crate::display::priority(&i.labels)
}

/// The host named in a `spawn` session (`<rt>:<session> <host>`).
pub fn host_of(t: &Task) -> Option<&str> {
    t.session.as_deref()?.rsplit(' ').next()
}

/// `rt` and the session id of a `spawn` session.
fn rt_of(t: &Task) -> Option<(&str, &str)> {
    t.session.as_deref()?.split(' ').next()?.split_once(':')
}

/// Set the `q-<state>` label from the fold; a closed task loses its `q-` labels and its issue is closed (§ 2).
pub fn project(board: &dyn Board, issue: &Issue, t: &Task) -> Result<(), String> {
    let want = board.state_label(t.state.name())?;
    let stale: Vec<String> = issue.labels.iter().filter(|l| l.starts_with("q-") && **l != want).cloned().collect();
    if t.state == State::Closed {
        board.labels(issue.number, &[], &stale)?;
        board.close(issue.number)
    } else if !stale.is_empty() || !issue.labels.contains(&want) {
        board.labels(issue.number, &[want], &stale)
    } else {
        Ok(())
    }
}

pub struct View {
    pub issue: Issue,
    pub projection: core::Projection,
    pub comments: Vec<Comment>,
    pub log: Vec<core::Entry>,
    /// `Depends on` issues that keep a ready task waiting (§ 1).
    pub blockers: Vec<Blocker>,
}

/// A `Depends on` issue that is not done (§ 1): `reason` is `open` or `unknown: <why>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Blocker {
    pub dep: core::Dep,
    pub url: String,
    pub reason: String,
}

/// Closed dependencies already read, `None` when done (§ 4 budget): an issue of this board until the open-issue list
/// shows it again; of another repository only when done. `ponytail:` a done foreign issue reopened later is not seen.
pub type DepCache = HashMap<core::Dep, Option<String>>;

/// The `Depends on` issues of `body` that are not done (§ 1): open, closed other than completed, or unreadable.
/// `open` is this board's open-issue list when the caller read it.
pub fn blockers(b: &dyn Board, body: &str, open: Option<&HashSet<u64>>, done: &mut DepCache) -> Vec<Blocker> {
    core::depends(body).into_iter().filter_map(|dep| {
        let listed = dep.repo.is_none() && open.is_some_and(|o| o.contains(&dep.n));
        if listed {
            done.remove(&dep);
        }
        let reason = match done.get(&dep) {
            _ if listed => Some("open".to_string()),
            Some(r) => r.clone(),
            None => match dep.repo.as_deref().map_or_else(|| b.issue(dep.n), |r| b.foreign(r, dep.n)) {
                Err(e) => Some(format!("unknown: {e}")),
                Ok(i) if i.open => Some("open".into()),
                Ok(i) => {
                    let mut r = i.reason.filter(|r| r != "completed").map(|r| format!("unknown: closed {}", r.replace('_', " ")));
                    // #1950: a close during a worker attempt is not done; its attempt host reopens it (§ 4 step 5)
                    if r.is_none() && dep.repo.is_none() {
                        match crate::board::entries(b, dep.n) {
                            Err(e) => return Some(Blocker { url: b.url(dep.n), reason: format!("unknown: {e}"), dep }),
                            Ok(log) => r = (core::fold(dep.n, &log, false).state == State::Doing).then(|| "unknown: closed during a worker attempt".into()),
                        }
                    }
                    if dep.repo.is_none() || r.is_none() {
                        done.insert(dep.clone(), r.clone());
                    }
                    r
                }
            },
        };
        let url = dep.repo.as_deref().map_or_else(|| b.url(dep.n), |r| b.foreign_url(r, dep.n));
        reason.map(|reason| Blocker { dep, url, reason })
    }).collect()
}

/// § 4 step 5 ask card of a task closed during a worker attempt (#1950); its answer resumes the attempt.
pub(crate) fn closed_card(n: u64, at: &str) -> String {
    crate::service_card(n, &format!("reopened; the issue was closed at {at} during this worker attempt, with no result or review."),
        "only the reviewer's close or a person ends a task (§ 1); a close during an attempt never counts as done.", &[
        "redo: finish the work - + the attempt hands in a result for review; - the attempt runs on".into(),
        format!("later: park it - + nothing runs; `taskq close {n}` then ends it unmerged; - the work stays undone"),
    ], 1)
}

/// § 2 ask card of a start the rules cap refused (#1946); an answer starts a fresh attempt.
pub(crate) fn refused_card(n: u64, why: &str) -> String {
    crate::service_card(n, "the service refused to start this attempt; nothing ran.", &format!("start refused: {}", why.replace("\nNext: ", "; ")), &[
        "retry after trimming - + a fresh attempt starts; - it is refused again until the rules fit the cap".into(),
        core::LATER.into(),
    ], 1)
}

/// § 2 ask card of resumed input that failed to launch `failures` times; the input stays for a retry.
pub(crate) fn input_card(n: u64, text: &str, failures: usize) -> String {
    crate::service_card(n, &format!("the input is retained; {text}"), &format!("input launch failed {failures} times; input retained."), &[
        "retry the input - + resends the undelivered input in this session; - it may fail the same way".into(),
        core::LATER.into(),
    ], 1)
}

/// `git -C dir <args>`: stdout, else the command and its error (#1856: a failed call never reads as empty).
fn git(dir: &std::path::Path, args: &[&str]) -> Result<Vec<u8>, String> {
    crate::tools::run("seed.git", crate::platform::command("git").arg("-C").arg(dir).args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"]).args(args), std::time::Duration::from_secs(60))
}

/// § 7 rule 2 (#2078): before a worker's first start the service, outside the worker sandbox (which may not write the
/// shared `.git`), creates `.worktrees/taskq-<N>` on branch `taskq-<N>` from `origin/main`, or on the existing branch.
/// An existing worktree is continued untouched. #1620 (docs/testing.md#build-cache): a new one gets a copy-on-write
/// clone of the newest `target/debug` of the root or another worktree: a recent build shares the current Cargo.lock,
/// so only the workspace crate recompiles. The clone only reads its source, and each worktree owns its copy. Any
/// failure only logs; the worker brief names the host gap. `ponytail:` a source mid-build clones as is; cargo's
/// fingerprints rebuild whatever does not match.
pub fn seed(root: &std::path::Path, n: u64) -> Option<String> {
    let name = format!("taskq-{n}");
    let wt = root.join(".worktrees").join(&name);
    if wt.exists() { return None; }
    // a stale registration of a deleted folder would refuse the add
    let _ = git(root, &["worktree", "prune"]);
    // offline: the last fetched origin/main still gives the worker its branch; the worker rebases before its push
    let fetched = git(root, &["fetch", "-q", "origin", "main"]).err().map_or(String::new(), |e| format!(" (fetch failed: {e})"));
    let rel = format!(".worktrees/{name}");
    let added = if git(root, &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")]).is_ok() {
        git(root, &["worktree", "add", "-q", &rel, &name])
    } else {
        git(root, &["worktree", "add", "-q", "-b", &name, &rel, "origin/main"])
    };
    if let Err(e) = added { return Some(format!("#{n} worktree not created: {e}{fetched}")); }
    let others = std::fs::read_dir(root.join(".worktrees")).into_iter().flatten().flatten().filter(|e| e.path() != wt).map(|e| e.path().join("target/debug"));
    let Some(src) = std::iter::once(root.join("target/debug")).chain(others)
        .filter_map(|p| Some((std::fs::metadata(&p).ok().filter(|m| m.is_dir())?.modified().ok()?, p))).max().map(|m| m.1)
    else { return Some(format!("#{n} worktree created{fetched}")); };
    let dst = wt.join("target/debug");
    if let Err(e) = std::fs::create_dir_all(wt.join("target")) { return Some(format!("#{n} seed skipped: {e}")); }
    let cloned = clone_dir(&src, &dst);
    if cloned.is_err() { let _ = std::fs::remove_dir_all(&dst); }
    Some(match cloned { Ok(()) => format!("#{n} seeded target/debug{fetched}"), Err(e) => format!("#{n} seed skipped: {e}{fetched}") })
}

#[cfg(target_os = "macos")]
fn clone_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &std::path::Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string());
    let (s, d) = (c(src)?, c(dst)?);
    // One call clones the whole tree on APFS; other volumes fail and skip.
    if unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), 0) } == 0 { Ok(()) } else { Err(format!("clonefile: {}", std::io::Error::last_os_error())) }
}

#[cfg(target_os = "linux")]
fn clone_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    // #2051: `auto` clones on btrfs and XFS and falls back to a plain copy where the volume has no reflink (ext4).
    // ponytail: the plain copy runs inside the 300 s budget; a slower volume times out and skips.
    crate::tools::run("seed.cp", crate::platform::command("cp").args(["-a", "--reflink=auto"]).arg(src).arg(dst), std::time::Duration::from_secs(300))
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn clone_dir(_: &std::path::Path, _: &std::path::Path) -> Result<(), String> {
    // ponytail: Windows ReFS block cloning is the upgrade path; a full copy costs more than sccache-backed builds.
    Err("no copy-on-write clone on this OS".into())
}

/// Seconds after an issue's `updated` before its comment list counts as settled: a list read sooner may lag the write
/// that set `updated`, so the next pass reads it again instead of reusing it for the unchanged `updated` (#1718).
const SETTLE: i64 = 30;

/// Comments per task: the `updated` they were read for, the board time of that read, the list.
pub type CommentCache = HashMap<u64, (i64, i64, Vec<Comment>)>;

/// Fold every open task (heartbeat issues and untrusted authors excluded).
pub fn views(board: &dyn Board, cache: &mut CommentCache, deps: &mut DepCache, ttl: i64) -> Result<(Vec<View>, Vec<Issue>), String> {
    let mut issues = board.issues(None)?;
    // #2083 § 3: a kept heartbeat is never newer than the board; one fresh when listed and past `ttl` now may have been
    // written since, so it relists before a lease reads it stale. One already stale when listed relists no more.
    if let Some(listed) = board.kept() {
        let now = board.now()?;
        if issues.iter().any(|h| is_host(h) && now - h.updated > ttl && listed - h.updated <= ttl) {
            board.forget();
            issues = board.issues(None)?;
        }
    }
    let open: HashSet<u64> = issues.iter().map(|i| i.number).collect();
    let (hosts, tasks): (Vec<Issue>, Vec<Issue>) = issues.into_iter().filter(|i| i.trusted).partition(is_host);
    let mut out = vec![];
    for issue in tasks.into_iter().filter(|i| is_task(i) || is_held(i)) {
        let (read, comments) = match cache.get(&issue.number) {
            Some((u, at, c)) if *u == issue.updated && *at >= issue.updated + SETTLE => (*at, c.clone()),
            _ => { let c = board.comments(issue.number)?; (board.now().unwrap_or(issue.updated), c) },
        };
        cache.insert(issue.number, (issue.updated, read, comments.clone()));
        let log = crate::board::reply::log(issue.number, &comments)?;
        // only a task that would be ready reads its dependencies (§ 4 budget)
        let mut projection = core::project(issue.number, &log, false);
        let ready = !is_held(&issue) && projection.task.state == State::Ready;
        let blockers = if ready { blockers(board, &core::dep_body(&issue.body, &projection.task), Some(&open), deps) } else { vec![] };
        if !blockers.is_empty() { projection.task.state = State::Waiting; }
        if is_held(&issue) {
            projection.task.state = State::Held;
        }
        out.push(View { projection, issue, comments, log, blockers });
    }
    Ok((out, hosts))
}

/// Release only a known submission, with idempotent write and authoritative readback.
fn release_execution(board: &dyn Board, task: u64, attempt: u64, execution: u64, proof: &str) -> Result<(), String> {
    release_execution_text(board, task, attempt, execution, proof, "")
}

pub(crate) fn release_execution_text(board: &dyn Board, task: u64, attempt: u64, execution: u64, proof: &str, text: &str) -> Result<(), String> {
    let expected = match proof { "exit" => core::Release::Exit, "no-start" => core::Release::NoStart, _ => return Err("invalid release proof".into()) };
    let released = |log: &[core::Entry]| core::executions(task, log).into_iter()
        .find(|x| x.id == execution && x.attempt == attempt).map(|x| x.release);
    match released(&crate::board::entries(board, task)?) {
        Some(Some(found)) if found == expected => return Ok(()),
        Some(Some(_)) => return Err(format!("execution {execution} has contradictory release proof")),
        Some(None) => {},
        None => return Err(format!("execution {execution} ownership unknown; release refused")),
    }
    let op = format!("release-{execution}");
    let body = core::with_fields(core::entry("released", &[&execution.to_string(), proof], Some(attempt), text), Some(&op), None);
    let written = board.comment(task, &body);
    if released(&crate::board::entries(board, task)?) == Some(Some(expected)) { Ok(()) }
    else { Err(format!("execution {execution} release unconfirmed: {}", written.err().unwrap_or_else(|| "readback missing".into()))) }
}

/// `seen`: a log this pass already read after the execution's own write; only the acceptance readback reads again (#1359).
fn accept_execution(board: &dyn Board, task: u64, attempt: u64, execution: u64, seen: Option<&[core::Entry]>) -> Result<u64, String> {
    let accepted = |log: &[core::Entry]| core::executions(task, log).into_iter()
        .find(|x| x.id == execution && x.attempt == attempt && x.accepted).map(|x| x.through);
    let log = match seen { Some(l) => l.to_vec(), None => crate::board::entries(board, task)? };
    let known = core::executions(task,&log).into_iter().find(|x| x.id == execution && x.attempt == attempt)
        .ok_or_else(|| format!("execution {execution} ownership unknown; acceptance refused"))?;
    if known.release == Some(core::Release::NoStart) { return Err(format!("execution {execution} confirmed no-start; acceptance refused")); }
    if known.accepted { return Ok(known.through); }
    let op = format!("accept-{execution}");
    let body = core::with_fields(core::entry("accepted", &[&execution.to_string()], Some(attempt), ""), Some(&op), None);
    let written = board.comment(task, &body);
    accepted(&crate::board::entries(board, task)?).ok_or_else(|| format!("execution {execution} acceptance unconfirmed: {}",
        written.err().unwrap_or_else(|| "readback missing or contradictory".into())))
}

pub struct Kid {
    pub execution: u64,
    pub task: u64,
    pub attempt: u64,
    pub rt: String,
    pub proc: Box<dyn Proc>,
    /// Last native observation; absence of evidence never releases this exact execution.
    pub unknown: Option<String>,
    pub stopping: bool,
    pub session_url: Option<String>,
}

impl Kid {
    fn release(&self,board:&dyn Board,code:i32,evidence:&str)->Result<bool,String> {
        let refusal=self.proc.refused(self.task,self.attempt,self.execution,code);
        release_execution_text(board,self.task,self.attempt,self.execution,if refusal.is_some() {"no-start"} else {"exit"},refusal.as_deref().unwrap_or(evidence))?;
        Ok(refusal.is_some())
    }
}

pub struct Service<'a> {
    pub host: Option<crate::host::Shared>,
    pub board: &'a dyn Board,
    pub cfg: Config,
    pub version: String,
    started: u64,
    diagnostics: std::cell::RefCell<Option<(std::time::Instant,Value)>>,
    pub kids: Vec<Kid>,
    /// `service <id>` in the heartbeat (§ 4): a hash of `<install>/installation-id`; empty writes no line.
    pub instance: String,
    /// `link <installation-id> <public key>` in the heartbeat ([host link](../docs/design/host-link.md)); empty writes no line.
    pub link: String,
    /// `machine <hostname>[ wsl]` in the heartbeat (§ 4): the agentq `host.name` DashQ joins hosts by; empty writes no line.
    pub machine: String,
    /// Labels from `<root>/.taskq/focus`; empty: admit nothing, `all`: any task; `p0` passes any non-empty focus (§ 4).
    pub focus: Vec<String>,
    /// Why `focus` could not be read; it then admits nothing (§ 4 Data).
    pub focus_error: Option<String>,
    /// The last inbox id handed to each attempt's child.
    delivered: HashMap<u64, u64>,
    cache: CommentCache,
    deps: DepCache,
    /// Label fix streaks: wanted label, stale labels, passes, board time after the last write, reported.
    label_fixes: HashMap<u64, (String, Vec<String>, u32, i64, bool)>,
    cancelled_publications: HashMap<u64, u64>,
    /// § 2 Summary: tasks this service saw with a summary, and its consecutive rewrites of each (a peer that renders
    /// another text is rewritten once per 60 passes, like a reverted label).
    summarized: HashSet<u64>,
    summary_fixes: HashMap<u64, u32>,
    /// #1820 each publication's last recorded wait reason: one `checks` observation per reason, never one per pass.
    publication_waits: HashMap<u64, String>,
    /// The last invalid `taskq.json` delivery, logged once (#1942).
    delivery_error: Option<String>,
    /// Closed-task terminal rows `repair_ended` proved are not this host's (#1359).
    unowned: HashSet<(u64, u64)>,
    /// The last worktree tidy (docs/design/worktrees.md); `None` runs one on the next pass.
    tidied: Option<std::time::Instant>,
    start: Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>, crate::runtime::StartError> + 'a>,
    pub recover: Option<Box<dyn FnMut(u64, &core::Execution) -> Result<Box<dyn Proc>, String> + 'a>>,
    /// Host boot in Unix seconds (#1386); tests replace the OS read.
    pub boot: fn() -> Result<i64, String>,
    #[cfg(test)]
    agent_status: Value,
    pub log: Vec<String>,
    /// The read model of the last successful pass (for `status.json` and the tray).
    pub snapshot: Option<crate::snapshot::Snapshot>,
    pub tasks_read: Option<std::time::Instant>,
    /// A complete board ownership read is required before drain or handover.
    ownership_known: bool,
    /// This host's heartbeat issue, from the last board read: a lease write needs no second list (§ 4 budget).
    host_issue: Option<u64>,
    /// Rebuilt from board ownership; heartbeat projection only, never a launch/release authority.
    observations: BTreeMap<(u64,u64),core::Execution>,
    /// Owner comments of the last pass (§ 4): task, comment id, text, card.
    /// State of each open task in the previous pass, for `events`.
    seen: HashMap<u64, State>,
    /// R6 stuck tasks already logged, by the board time of their newest log entry: one line until the task moves.
    stuck: HashMap<u64, i64>,
    /// § 4 Churn tasks already logged: one line and one `service.churn` record until the flag clears.
    churned: HashSet<u64>,
    /// The newest comment id an events pass saw; `gone` events come only from newer comments (§ 4)
    gone_after: Option<u64>,
    /// Newest comment id of the previous pass, for queue transitions; None before the first (§ 4 Profiling).
    moved_after: Option<u64>,
    /// First time covered by `<root>/.taskq/events` (§ 4).
    pub events_since: Option<i64>,
    /// Capacity reloaded before each pass (§ 3); an error holds every launch.
    pub cap: Result<Capacity, String>,
    pub gate: Gate,
    /// Monotonic seconds for the gate.
    pub mono: u64,
    /// Won claims the gate holds before launch.
    held: Vec<Launch>,
    /// Attempts without a child that keep their lease: held claims and resumes (§ 3, § 4 heartbeat).
    pending: BTreeSet<(u64, u64)>,
    /// Spawns this process posted whose readback lacked them: no new claim of the task until one shows (#1903).
    /// ponytail: an entry clears at the next claim of its task, so closed tasks keep theirs until restart.
    unseen: HashMap<u64, u64>,
    /// Wall seconds for the host authority, set by the loop.
    pub wall: i64,
    /// Auto reservations the host granted, not yet a child; refused requests of this pass and their reason (§ 3).
    reserved: Vec<Res>,
    requested: Vec<Res>,
    refused: Option<(&'static str, String)>,
    /// Held reservations of the whole host at the last host write; unknown when it failed.
    host_used: Option<u32>,
    /// The host policy revision for this root that set this pass's capacity (#1282 applied readback).
    pub project_revision: Option<String>,
    /// The emergency hold read at the start of each pass (§ 3): refuses every launch.
    pub hold: Option<String>,
    pub stop_progress: Vec<Value>,
    /// Runtimes whose access policy refuses a launch, with the reason (§ 8 Access): no claim, resume or reviewer.
    pub refused_access: BTreeMap<String, String>,
    /// Runtimes held after a usage limit, with board time the hold ends and whether the runtime reported it
    /// ([usage limit](docs/design/capacity.md#usage-limit)): no claim, resume or reviewer until then.
    /// Rebuilt from host-wide account windows and execution release evidence after restart.
    pub limited: BTreeMap<String, (i64, bool)>,
    /// Runtimes whose last probe read `not logged in` (§ 5 `agents`, #1429): no claim, resume or reviewer until a
    /// probe reads them logged in again; unknown login admits as before.
    pub logged_out: BTreeSet<String>,
    /// § 4 heartbeat `cli <bin> <version>`: each CLI's version at the last runtime probe; `unknown` when unread (R12).
    pub clis: BTreeMap<String, String>,
    /// § 3 Runtime accounts: the active account of each listed runtime at the last pass (switch record).
    pub accounts: BTreeMap<String, Option<String>>,
    pub gui_checked: Option<std::time::Instant>,
    pub gui_probe: Result<(), String>,
    /// #1803: the last gui probe read a stale grant (`platform::stale_grant`); the tray notifies once.
    pub gui_stale: bool,
    /// #1845: `platform::grants` of the last gui probe; a `missing` or `stale` one holds gui work.
    pub grants: BTreeMap<String, String>,
    pub full: BTreeSet<String>,
    pub waits: BTreeMap<u64, String>,
    pub execution_waits: BTreeMap<u64, String>,
    pending_releases: BTreeSet<(u64, u64, u64)>,
    /// Children by attempt: role, runtime, and start with attempt log offset; `None` when adopted across a re-exec
    /// (§ 4 Profiling stages).
    begun: HashMap<u64, (String, String, Option<(std::time::Instant, u64)>)>,
    /// This host's tasks seen moving into `ask`: since when, attempt, runtime (§ 4 Profiling stages).
    asked: HashMap<u64, (std::time::Instant, u64, String)>,
    /// Tasks on the last open list, and those gone from it not yet read (§ 4 step 5, #1950).
    listed: HashSet<u64>,
    unlisted: BTreeSet<u64>,
    reduced_at: Option<u64>,
    recovery_after: Option<u64>,
}

const MONTH: i64 = 30 * 86400;
/// Seconds a usage-limited runtime stays held when it reported no reset.
const USAGE_PROBE: i64 = 1800;
/// #1843: continuing steers per attempt after a transient model failure (§ 4 step 1).
const CONTINUE_MAX: usize = 2;

/// Host readback stays separate from a project's effective narrowing limits.
pub fn host_capacity(capacity:&Value)->Option<Value> {
    if !capacity["host_policy"].is_object() {return None;}
    let mut host=capacity.clone();
    for field in ["limits","load_gate","admission"] {host[field]=capacity["host_policy"][field].clone();}
    host["used"]=capacity["host_runtime_used"].clone();
    Some(host)
}

/// Load gate settings (§ 9).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GateCfg {
    pub enabled: bool,
    pub cpu_max: u32,
    pub memory_min: u32,
    /// Auto: CPU is the 60 s mean; the latest 30 s interval at 100 % still vetoes; memory also holds below
    /// [`FLOOR_GIB`] available or at macOS pressure level [`PRESSURE_CRITICAL`] (D03).
    pub mean: bool,
}

pub const GATE_DEFAULT: GateCfg = GateCfg { enabled: false, cpu_max: 85, memory_min: 15, mean: false };
/// Auto admission defaults from the D03 calibration (docs/design/admission-calibration.md).
pub const GATE_AUTO: GateCfg = GateCfg { enabled: true, cpu_max: 90, memory_min: 10, mean: true };
pub const FLOOR_GIB: f64 = 2.5;
pub const PRESSURE_CRITICAL: u32 = 4;
pub use crate::runtime::RUNTIMES;

/// Effective capacity of one host (§ 3, § 9).
#[derive(Clone, PartialEq, Debug)]
pub struct Capacity {
    pub limits: BTreeMap<String, u32>,
    pub gate: GateCfg,
    /// `admission` mode `auto`: the host authority admits; a runtime without a limit has no count ceiling.
    pub auto: bool,
    /// Auto: optional host-wide session ceiling.
    pub max: Option<u32>,
}

impl Capacity {
    pub fn fixed(limits: BTreeMap<String, u32>, gate: GateCfg) -> Capacity {
        Capacity { limits, gate, auto: false, max: None }
    }

    pub fn json(&self) -> Value {
        json!({"limits": self.limits, "load_gate": {"enabled": self.gate.enabled, "cpu_max": self.gate.cpu_max, "memory_min": self.gate.memory_min},
            "admission": {"mode": if self.auto { "auto" } else { "fixed" }, "max": self.max}})
    }
}

/// Capacity of `host` from `taskq.json` (§ 9): its `capacity_hosts` limits override the top-level ones; its
/// `load_gate` replaces the top-level object; omitted gate fields take the defaults. Invalid values are an error.
pub fn capacity(v: &Value, host: &str) -> Result<Capacity, String> {
    let limits = |x: &Value, at: &str| -> Result<BTreeMap<String, u32>, String> {
        match x {
            Value::Null => Ok(BTreeMap::new()),
            Value::Object(m) => m.iter().map(|(k, n)| n.as_u64().and_then(|n| u32::try_from(n).ok()).map(|n| (k.clone(), n)).ok_or(format!("{at}.{k}: not an integer 0..{}", u32::MAX))).collect(),
            _ => Err(format!("{at}: not an object")),
        }
    };
    if !matches!(v["capacity_hosts"], Value::Null | Value::Object(_)) {
        return Err("capacity_hosts: not an object".into());
    }
    let h = &v["capacity_hosts"][host];
    let mut out = limits(&v["limits"], "limits")?;
    out.extend(limits(&h["limits"], &format!("capacity_hosts.{host}.limits"))?);
    let (a, aat) = if h["admission"].is_null() { (&v["admission"], "admission".to_string()) } else { (&h["admission"], format!("capacity_hosts.{host}.admission")) };
    if !matches!(a, Value::Null | Value::Object(_)) {
        return Err(format!("{aat}: not an object"));
    }
    let auto = match &a["mode"] {
        Value::Null => false,
        x if x == "auto" => true,
        x if x == "fixed" => false,
        _ => return Err(format!("{aat}.mode: auto or fixed")),
    };
    let max = match &a["max"] {
        Value::Null => None,
        x => Some(x.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or(format!("{aat}.max: not an integer 0..{}", u32::MAX))?),
    };
    let d = if auto { GATE_AUTO } else { GATE_DEFAULT };
    let (g, at) = if h["load_gate"].is_null() { (&v["load_gate"], "load_gate".to_string()) } else { (&h["load_gate"], format!("capacity_hosts.{host}.load_gate")) };
    if !matches!(g, Value::Null | Value::Object(_)) {
        return Err(format!("{at}: not an object"));
    }
    let pct = |k: &str, d: u32, lo: u64, hi: u64| match &g[k] {
        Value::Null => Ok(d),
        x => x.as_u64().filter(|n| (lo..=hi).contains(n)).map(|n| n as u32).ok_or(format!("{at}.{k}: not an integer {lo}..{hi}")),
    };
    let enabled = match &g["enabled"] {
        Value::Null => d.enabled,
        x => x.as_bool().ok_or(format!("{at}.enabled: not true or false"))?,
    };
    if auto && !enabled {
        return Err(format!("{at}.enabled: Auto admission needs the load gate; use admission mode fixed"));
    }
    let gate = GateCfg { enabled, cpu_max: pct("cpu_max", d.cpu_max, 10, 100)?, memory_min: pct("memory_min", d.memory_min, 1, 95)?, mean: d.mean };
    Ok(Capacity { limits: out, gate, auto, max })
}

/// The load gate (§ 3): samples on a monotonic clock in seconds, decisions on unrounded percentages.
#[derive(Default, Clone)]
pub struct Gate {
    cfg: Option<GateCfg>,
    /// CPU (busy, total) ticks of the previous reading: the baseline of the next.
    base: Option<(u64, u64)>,
    /// The reading before `base`: with it CPU is the mean over two intervals (60 s, `GateCfg::mean`).
    prev: Option<(u64, u64)>,
    /// Latest sample: monotonic time, wall time, CPU busy %, available memory %.
    pub last: Option<(u64, i64, Option<f64>, Option<f64>)>,
    /// Auto: available GiB and pressure level read with the latest sample; `None` is a failed reading.
    pub host_mem: Option<(f64, u32)>,
    /// Consecutive calm samples toward reopening.
    good: u32,
    open: bool,
    /// Monotonic time of the last launch and whether a sample followed it.
    launched: Option<u64>,
    sampled: bool,
}

impl Gate {
    /// Enabling or changing thresholds starts held.
    pub fn configure(&mut self, c: GateCfg) {
        if self.cfg != Some(c) {
            (self.cfg, self.open, self.good) = (Some(c), false, 0);
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.is_some_and(|c| c.enabled)
    }

    /// One sample: CPU ticks `(busy, total)` and available memory %; `None` is a failed reading.
    pub fn observe(&mut self, now: u64, wall: i64, ticks: Option<(u64, u64)>, memory: Option<f64>) {
        // decreasing counters or an empty interval are no reading; the baseline restarts either way
        let busy = |from: Option<(u64, u64)>| match (from, ticks) {
            (Some((b0, t0)), Some((b, t))) if b >= b0 && t > t0 && b - b0 <= t - t0 => Some((b - b0) as f64 * 100.0 / (t - t0) as f64),
            _ => None,
        };
        let raw = busy(self.base);
        let c = self.cfg.unwrap_or(GATE_DEFAULT);
        // the 60 s mean, unless the latest interval alone is at 100 % (veto) or no older reading exists
        let cpu = if c.mean && raw.is_some_and(|p| p < 100.0) { busy(self.prev).or(raw) } else { raw };
        let late = self.last.is_some_and(|l| now.saturating_sub(l.0) > 60);
        self.prev = if raw.is_some() { self.base } else { None };
        self.base = ticks;
        let memory = memory.filter(|m| (0.0..=100.0).contains(m));
        self.last = Some((now, wall, cpu, memory));
        self.sampled = true;
        let (max, min) = (c.cpu_max as f64, c.memory_min as f64);
        let floor = !c.mean || self.host_mem.is_some_and(|(g, l)| g >= FLOOR_GIB && l < PRESSURE_CRITICAL);
        match (cpu, memory) {
            (Some(p), Some(m)) if !late && floor && p < max && m > min => {
                if p <= max - 10.0 && m >= min + 5.0 {
                    self.good += 1;
                    self.open |= self.good >= 3;
                } else {
                    self.good = 0;
                }
            }
            _ => (self.open, self.good) = (false, 0),
        }
    }

    /// `off`, `open`, `held` or `unknown` with every blocker `(code, text)` (§ 4 `capacity`).
    pub fn state(&self, now: u64) -> (&'static str, Vec<(&'static str, String)>) {
        let Some(c) = self.cfg.filter(|c| c.enabled) else { return ("off", vec![]) };
        let mut r = vec![];
        match self.last {
            None => r.push(("sample", "no sample yet".to_string())),
            Some((at, ..)) if now.saturating_sub(at) > 60 => r.push(("sample", format!("last sample {} s old", now - at))),
            Some((_, _, cpu, mem)) => {
                match cpu {
                    None => r.push(("sample", "no CPU reading yet".into())),
                    Some(p) if p >= c.cpu_max as f64 => r.push(("cpu", format!("CPU {p:.0} % >= {} %", c.cpu_max))),
                    _ => {}
                }
                match mem {
                    None => r.push(("sample", "no memory reading".into())),
                    Some(m) if m <= c.memory_min as f64 => r.push(("memory", format!("available memory {m:.0} % <= {} %", c.memory_min))),
                    _ => {}
                }
                match self.host_mem.filter(|_| c.mean) {
                    None if c.mean => r.push(("sample", "no memory floor reading".into())),
                    Some((g, _)) if g < FLOOR_GIB => r.push(("memory", format!("available memory {g:.1} GiB < {FLOOR_GIB} GiB"))),
                    Some((_, l)) if l >= PRESSURE_CRITICAL => r.push(("memory", format!("memory pressure level {l} (critical)"))),
                    _ => {}
                }
            }
        }
        if r.is_empty() && !self.open {
            r.push(("cooldown", format!("{} of 3 calm samples", self.good)));
        }
        (if r.iter().any(|x| x.0 == "sample") { "unknown" } else if r.is_empty() { "open" } else { "held" }, r)
    }

    /// Off, or open with the previous launch at least 30 s ago and sampled since (§ 3).
    pub fn may_launch(&self, now: u64) -> bool {
        !self.enabled() || (self.state(now).0 == "open" && self.launched.is_none_or(|t| self.sampled && now.saturating_sub(t) >= 30))
    }

    pub fn launch(&mut self, now: u64) {
        (self.launched, self.sampled) = (Some(now), false);
    }
}

/// Append `(task, kind, id, outcome)` moves to the events file at board time `now` (§ 4): one line per id and kind
/// (`ask`: its card entry, else the attempt), a `close` followed by its § 7 outcome text;
/// lines older than 30 days dropped. Returns the first time covered.
pub fn record(path: &std::path::Path, now: i64, moves: &[(u64, &str, u64, String)]) -> Result<i64, String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines();
    let since = lines.next().and_then(|l| l.strip_prefix("since ")?.parse().ok()).unwrap_or(now).max(now - MONTH);
    let mut keep: Vec<String> = lines.filter(|l| l.split(' ').next().and_then(|t| t.parse::<i64>().ok()).is_some_and(|t| t >= since)).map(String::from).collect();
    for (n, kind, a, outcome) in moves {
        if !keep.iter().any(|l| l.split(' ').skip(2).take(2).eq([*kind, a.to_string().as_str()])) {
            keep.push(format!("{now} #{n} {kind} {a} {outcome}").trim_end().into());
        }
    }
    let out: String = std::iter::once(format!("since {since}")).chain(keep).map(|l| l + "\n").collect();
    if out != text {
        crate::platform::write_atomic(path, &out)?;
    }
    Ok(since)
}

impl<'a> Service<'a> {
    pub fn new(board: &'a dyn Board, cfg: &'a Config, version: &str, start: Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>, crate::runtime::StartError> + 'a>) -> Self {
        let cap = Ok(Capacity::fixed(cfg.limits.clone(), GATE_DEFAULT));
        Service { recovery_after: None, reduced_at: None, host: None, wall: 0, reserved: vec![], requested: vec![], refused: None, host_used: None, project_revision: None, board, cfg: cfg.clone(), version: version.into(), diagnostics: Default::default(), started: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_secs()), kids: vec![], instance: String::new(), link: String::new(), machine: String::new(), focus: vec![], focus_error: None, delivered: HashMap::new(), cache: HashMap::new(), deps: HashMap::new(), label_fixes: HashMap::new(), cancelled_publications: HashMap::new(), summarized: HashSet::new(), summary_fixes: HashMap::new(), publication_waits: HashMap::new(), delivery_error: None, unowned: HashSet::new(), tidied: None, start, recover: None, boot: if cfg!(test) {|| Err("boot time not read in tests".into())} else {crate::platform::boot_time as fn() -> Result<i64, String>}, #[cfg(test)] agent_status: json!([]), log: vec![], snapshot: None, tasks_read: None, ownership_known: false, host_issue: None, observations: BTreeMap::new(), seen: HashMap::new(), stuck: HashMap::new(), churned: HashSet::new(), gone_after: None, moved_after: None, events_since: None, cap, gate: Gate::default(), mono: 0, held: vec![], pending: BTreeSet::new(), unseen: HashMap::new(), hold: None, stop_progress: vec![], refused_access: BTreeMap::new(), limited: BTreeMap::new(), logged_out: BTreeSet::new(), clis: BTreeMap::new(), accounts: BTreeMap::new(), gui_checked: None, gui_probe: Err("not probed".into()), gui_stale: false, grants: BTreeMap::new(), full: BTreeSet::new(), waits: BTreeMap::new(), execution_waits: BTreeMap::new(), pending_releases: BTreeSet::new(), begun: HashMap::new(), asked: HashMap::new(), listed: HashSet::new(), unlisted: BTreeSet::new(), }
    }

    /// Rebuild board-derived state after unwind; live handles, pending launches and delivery cursors stay owned.
    pub fn rebuild(&mut self) {
        self.unready();
        self.cache.clear();
        self.deps.clear();
        self.label_fixes.clear();
        self.snapshot = None;
        self.tasks_read = None;
    }

    /// Quarantine/backoff: only verified reaping, leases and status; no claims, resumes or recovery writes.
    pub fn reduced(&mut self) -> Result<(), String> {
        self.unready();
        self.full_stop_owned();
        let before = self.kids.len();
        self.reap_owned();
        self.reconcile_ownership()?;
        self.sync_host();
        self.beat_reduced(before != self.kids.len())
    }

    /// #1766: the Stop barrier keeps an idle prepared project out of passes (#1402); it still reads the board for
    /// status.json and beats. No claim, resume, recovery, label or event write; ownership stays as last reported.
    pub fn observe(&mut self) -> Result<(), String> {
        self.unready();
        let (views, hosts) = views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?;
        let now = self.board.now()?;
        self.snapshot = Some(crate::snapshot::from_views("host", self.board, &self.cfg, now, &views, &hosts));
        self.copy_images();
        self.tasks_read = Some(std::time::Instant::now());
        self.beat_reduced(false)
    }

    /// The heartbeat of a reduced or observe pass: at most every `ttl / 5`, at once when its children changed.
    fn beat_reduced(&mut self, changed: bool) -> Result<(), String> {
        if changed || self.reduced_at.is_none_or(|t| self.mono.saturating_sub(t) >= (self.cfg.ttl / 5).max(1) as u64) {
            self.reduced_at = Some(self.mono);
            if let Some(n) = self.host_issue {
                let hb = self.board.issue(n)?;
                let now = self.board.now()?;
                let body = self.heartbeat_body();
                if now - hb.updated >= self.cfg.ttl / 5 || !hb.body.lines().filter(|l| !l.starts_with("at ")).eq(body.lines()) {
                    self.board.edit(n, &format!("{body}\nat {now}"))?;
                }
            }
        }
        Ok(())
    }

    pub fn unready(&self) {
        if let Some(host) = &self.host { if let Ok(mut h) = host.lock() { h.ready(&self.cfg.root, false); } }
    }

    /// R11/E2: cancellation precedes board access, including reduced/offline passes. No release is fabricated.
    pub fn full_stop_owned(&mut self) {
        let full=match crate::lifecycle::full_stop(&self.cfg.install,&self.cfg.root) {
            Ok(value)=>value,Err(reason)=>{self.log.push(reason);return;}
        };
        if !full {return;}
        self.stop_progress.clear();
        for k in &mut self.kids {
            if crate::recovery::selected(&self.cfg.install,&self.cfg.root,k.task) {continue;}
            k.stopping=true;
            let observed=match k.proc.exited() {
                Ok(Some(code))=>Ok(code),
                _=>crate::runtime::stop(k.proc.as_mut()),
            };
            let child=crate::snapshot::Child {exited:observed.is_ok(),unknown:observed.as_ref().err().map(String::as_str),stopping:true};
            let (state,reason)=match crate::snapshot::verdict(&crate::snapshot::Signals {local:true,child:Some(child),..Default::default()}) {
                ("ended",_)=>("stopped",Some("native exit observed; board release checked separately".to_owned())),
                (_,reason)=>{k.unknown=Some(reason.clone());("unknown",Some(reason))},
            };
            self.stop_progress.push(json!({"task":k.task,"attempt":k.attempt,"execution":k.execution,"rt":k.rt,"state":state,"reason":reason}));
        }
    }

    /// Liveness monitoring never frees ownership before the exact execution release is read back.
    pub fn reap_owned(&mut self) {self.reap_owned_within(NATIVE_PASS_BUDGET);}

    fn reap_owned_within(&mut self,budget:std::time::Duration) {
        let until=std::time::Instant::now()+budget;
        let mut deferred=None;
        let mut retired=vec![];
        self.kids.retain_mut(|k| {
            if std::time::Instant::now()>=until {
                deferred.get_or_insert(k.execution);
                k.unknown=Some("native observation deferred; reduced pass budget exhausted".into());return true;
            }
            let status = crate::runtime::observe_until(until,||k.proc.exited());
            let mut acceptance = None;
            match crate::runtime::observe_until(until,||k.proc.accepted()) {
                Ok(true) => match accept_execution(self.board,k.task,k.attempt,k.execution,None) {
                    Ok(through) => { self.delivered.entry(k.attempt).and_modify(|v| *v = (*v).max(through)).or_insert(through); },
                    Err(e) => { k.unknown=Some(e); return true; }
                },
                Ok(false) => {},
                Err(e) => acceptance = Some(e),
            }
            // #1594: a failed exit read, then a failed acceptance read, then a cancellation's earlier failure stays unknown
            let unknown = status.as_ref().err().or(acceptance.as_ref()).or(k.unknown.as_ref().filter(|_| k.stopping)).cloned();
            let child = crate::snapshot::Child { exited: matches!(status, Ok(Some(_))), unknown: unknown.as_deref(), stopping: k.stopping };
            match crate::snapshot::verdict(&crate::snapshot::Signals { local: true, child: Some(child), ..Default::default() }) {
            ("ended", _) => match k.release(self.board,status.unwrap().unwrap(),"") {
                Ok(_)=>{
                    retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).child));
                    false
                },
                Err(e)=>{k.unknown=Some(e);true}
            },
            ("unknown", e) => { k.unknown=Some(e); true }
            _ => { k.unknown=None; true }
            }
        });
        for (task,attempt,execution,native) in retired { self.retired(task,attempt,execution,native); }
        if let Some(first)=deferred.and_then(|id|self.kids.iter().position(|k|k.execution==id)) {self.kids.rotate_left(first);}
    }

    /// Called only after release readback; retained children keep all their live metadata and capacity.
    fn retired(&mut self, task:u64, attempt:u64, execution:u64, native:bool) {
        if native {
            if let Err(e)=crate::retire_kid(&self.cfg,task,attempt,execution) { self.log.push(format!("native recovery cleanup: {e}")); }
        }
        self.pending_releases.remove(&(task,attempt,execution));
        self.observations.remove(&(task,execution));
        self.cache.remove(&task);
        self.retire_reservations(BTreeSet::from([(task,attempt)]));
    }

    fn retire_reservations(&mut self, released:BTreeSet<(u64,u64)>) {
        for (task,attempt) in released {
            if self.kids.iter().any(|k|k.task==task && k.attempt==attempt)
                || self.observations.iter().any(|((n,_),x)|*n==task && x.attempt==attempt)
                || self.held.iter().any(|l|l.task==task && l.attempt==attempt) {continue;}
            self.reserved.retain(|(n,a,_)|*n!=task || *a!=attempt);
            self.pending.remove(&(task,attempt));
        }
    }

    /// New capacity config (§ 3): a gate change starts held.
    pub fn reload(&mut self, cap: Result<Capacity, String>) {
        if let Ok(c) = &cap {
            self.gate.configure(c.gate);
        }
        self.cap = cap;
    }

    /// Rereads `taskq.json` for a pass (§ 3 capacity) and takes its § 7 delivery preset, so a pending publication
    /// follows an owner's preset switch without a restart (#1942). An invalid preset keeps the last one, logged once.
    pub fn reread_project(&mut self) -> Result<Value, String> {
        let v = crate::project_value(&self.cfg.root)?;
        match crate::delivery(&v) {
            Ok(d) => {
                if d != (self.cfg.publish.clone(), self.cfg.delivery.clone(), self.cfg.delivery_revision) {
                    log!("delivery {} r{} (was {} r{})", d.1, d.2, self.cfg.delivery, self.cfg.delivery_revision);
                    (self.cfg.publish, self.cfg.delivery, self.cfg.delivery_revision) = d;
                }
                self.delivery_error = None;
            }
            Err(e) => {
                if self.delivery_error.as_ref() != Some(&e) { log!("{e}; keeping delivery {} r{}", self.cfg.delivery, self.cfg.delivery_revision); }
                self.delivery_error = Some(e);
            }
        }
        Ok(v)
    }

    fn may_launch(&self,task:u64) -> bool {
        self.launch_refusal(task).is_none()
    }

    /// Why no launch of `task` may start now, as a #1616 admission code and reason; the trace keeps its § 3 code
    /// under the `may_launch` span every launch check emits.
    #[tracing::instrument(name="service.Service.may_launch",skip_all)]
    fn launch_refusal(&self,task:u64) -> Option<(&'static str,String)> {
        let hold = self.hold.clone().filter(|_| !crate::recovery::allowed(&self.cfg.install,&self.cfg.root,task))
            .or_else(|| crate::admission::task_hold(&self.cfg.install,&self.cfg.root,task));
        let refusal = if let Some(h) = hold { Some(("hold", "hold", h)) }
            else if self.cap.is_err() { Some(("config", "unknown", crate::snapshot::ADMISSION_UNKNOWN[1].to_string())) }
            else if !self.gate.may_launch(self.mono) {
                let (code, text) = self.gate.state(self.mono).1.into_iter().next().unwrap_or(("launch-interval", "launch interval".into()));
                Some((code, "load-gate", format!("{code}: {text}")))
            } else { None };
        let (code, admission, reason) = refusal?;
        tracing::info!(coreq_refusal = code);
        Some((admission, reason))
    }

    fn auto(&self) -> bool {
        self.cap.as_ref().is_ok_and(|c| c.auto)
    }

    /// This service's entry in the host authority: children and granted reservations are held.
    fn entry(&self) -> Entry {
        let held = self.kids.iter().map(|k| (k.task, k.attempt, k.rt.clone())).chain(self.reserved.iter().cloned())
            .collect::<BTreeSet<_>>().into_iter().collect();
        Entry { version: self.version.clone(), at: 0, held, requested: self.requested.clone() }
    }

    /// Lost native handles still hold reservations and must retain their recovery inputs.
    pub fn retained_attempts(&self)->Vec<u64> {
        self.entry().held.into_iter().map(|(_,attempt,_)|attempt).collect()
    }

    fn observe_ownership(&mut self, views:&[View]) -> Result<(),String> {
        let mut released=BTreeSet::new();
        for task in self.observations.keys().map(|(task,_)|*task).collect::<BTreeSet<_>>() {
            let read;
            let current = if let Some(v)=views.iter().find(|v|v.issue.number==task) { &v.projection.executions }
                else { read=core::project(task, &crate::board::entries(self.board,task)?, false); &read.executions };
            // Open-list absence, label removal and missing history never release known ownership.
            self.observations.retain(|(n,id),old| {
                let terminal=*n==task && !self.kids.iter().any(|k|k.task==task && k.execution==*id)
                    && current.iter().any(|x|x.id==*id && x.attempt==old.attempt && x.host==old.host && x.runtime==old.runtime && x.release.is_some());
                if terminal {released.insert((task,old.attempt));}
                !terminal
            });
        }
        self.observations.extend(views.iter().flat_map(|v|v.projection.executions.iter().cloned()
            .filter(|x|x.host==self.cfg.me && x.release.is_none()).map(|x|((v.issue.number,x.id),x))));
        self.retire_reservations(released);
        for ((task,_),x) in &self.observations {
            let reservation=(*task,x.attempt,x.runtime.clone());
            if !self.kids.iter().any(|k|k.task==*task && k.execution==x.id) && !self.reserved.contains(&reservation) {self.reserved.push(reservation);}
        }
        self.ownership_known=true;
        Ok(())
    }

    /// Recovery monitoring reads ownership only; it cannot resume ordinary task effects.
    pub fn reconcile_ownership(&mut self) -> Result<(),String> {
        if self.ownership_known && (self.handoff_waits(false,false).is_empty()
            || self.tasks_read.is_some_and(|at|at.elapsed().as_secs()<self.cfg.poll.max(1))) {return Ok(());}
        self.ownership_known=false;
        let (views,hosts)=views(self.board,&mut self.cache,&mut self.deps,self.cfg.ttl)?;
        let now=self.board.now()?;
        self.observe_ownership(&views)?;
        self.snapshot=Some(crate::snapshot::from_views("host",self.board,&self.cfg,now,&views,&hosts));
        self.tasks_read=Some(std::time::Instant::now());
        Ok(())
    }

    /// Handles may cross a qualified image switch; childless uncertainty cannot. `handover` (#1601): a pending re-exec
    /// refuses every reservation, so a childless reserved or pending attempt passes to the next image, which reads it
    /// from the board and resumes it from the inbox; held launches are released first (`release_held`) and wait until
    /// then. `exit` (#1435): a Windows image exit hands over no handle and the next image reads the same unresolved
    /// executions from the board, so childless board observations pass too; release state still waits.
    pub fn handoff_waits(&self, handover:bool, exit:bool) -> Vec<String> {
        let owns=|task,attempt|self.kids.iter().any(|k|k.task==task && k.attempt==attempt);
        let mut waits=BTreeSet::new();
        if !self.ownership_known {waits.insert("ownership unknown".into());}
        for (task,attempt) in self.reserved.iter().map(|(n,a,_)|(*n,*a)).chain(self.pending.iter().copied()).filter(|_|!handover)
            .chain(self.held.iter().map(|l|(l.task,l.attempt))) {
            if !owns(task,attempt) {waits.insert(format!("#{task} unresolved ownership"));}
        }
        // #1594: a child of this process hands over through the runtime adapter (lifecycle_waits); the rest wait on the verdict
        for ((task,id),x) in self.observations.iter().filter(|_|!exit) {
            let signals=self.signals(*task,x);
            if signals.child.is_none() && crate::snapshot::verdict(&signals).0!="ended" {waits.insert(format!("#{task} unresolved execution {id}"));}
        }
        for (task,_,_) in &self.pending_releases {waits.insert(format!("#{task} release unconfirmed"));}
        waits.into_iter().collect()
    }

    /// #1601: a pending restart refuses every reservation, so a held launch could never start and the handover would
    /// wait on it. Its start was never called: release it `no-start` and let the next image act on the board.
    // ponytail: a released initial claim ends as `gone` (+1 retry); a held-claim handover record would keep the retry.
    fn release_held(&mut self) -> Vec<u64> {
        if !self.host.as_ref().is_some_and(|h|h.lock().is_ok_and(|h|h.restart)) {return vec![];}
        let mut released=vec![];
        while let Some(l)=self.held.first() {
            let (task,attempt,execution)=(l.task,l.attempt,l.execution.unwrap_or(l.attempt));
            if let Err(e)=release_execution(self.board,task,attempt,execution,"no-start") {
                self.log.push(format!("#{task} held launch release: {e}"));
                break;
            }
            self.held.remove(0);
            self.log.push(format!("#{task} held launch released for the restart"));
            self.retired(task,attempt,execution,false);
            released.push(task);
        }
        if !released.is_empty() {self.sync_host();}
        released
    }

    pub fn idle(&self) -> bool {self.kids.is_empty() && self.handoff_waits(false,false).is_empty()}

    /// Admission of one launch (§ 3): the load gate, then under Auto a reservation from the host authority, which
    /// refuses with a reason; the request stays `requested`. Fixed admits as before.
    fn reserve(&mut self, task: u64, attempt: u64, rt: &str) -> bool {
        if !self.may_launch(task) {
            return false;
        }
        if let Some(host) = self.host.clone() {
            let r = host.lock().map_err(|_| "host admission poisoned; launches held".to_string()).and_then(|mut h| {
                let now = h.elapsed();
                h.reserve(&self.cfg.root, task, attempt, rt, self.cap.as_ref().unwrap(), attempt == 0, now)
            });
            return match r {
                Ok(used) => {
                    let res = (task, attempt, rt.to_string());
                    if !self.reserved.contains(&res) { self.reserved.push(res); }
                    self.host_used = Some(used as u32);
                    true
                }
                Err(e) => { self.refused = Some(("host", e)); false }
            };
        }
        if !self.auto() { return true; }
        let old = crate::admission::old_services(&self.cfg.install);
        let res = (task, attempt, rt.to_string());
        let want = Want { max: self.cap.as_ref().ok().and_then(|c| c.max), sample_at: self.gate.last.map(|l| l.1), old: &old, res: res.clone() };
        let (r, used) = crate::admission::sync(&self.cfg.install, &self.cfg.root.display().to_string(), self.entry(), Some(want), self.wall);
        self.host_used = used;
        match r {
            Ok(()) => {
                self.reserved.push(res);
                true
            }
            Err(e) => {
                self.requested.push(res);
                self.refused = Some(e);
                false
            }
        }
    }

    /// Rewrite this service's host entry: released reservations leave it (§ 3). Fixed services write too, so Auto
    /// counts their children; a failed write only matters to Auto, whose next request refuses.
    pub fn sync_host(&mut self) {
        if let Some(host) = &self.host {
            let recovered=self.handoff_waits(false,false).is_empty();
            let held=self.entry().held;
            match host.lock() {
                Ok(mut h) => {
                    if recovered {h.recovered.insert(self.cfg.root.clone());}else {h.recovered.remove(&self.cfg.root);}
                    if h.rebuilding&&self.ownership_known {h.rebuild_slots(&self.cfg.root,held);}else {h.sync(&self.cfg.root,held);}
                    self.host_used=(!h.rebuilding).then(||h.used().values().sum::<usize>() as u32);
                }
                Err(e) => {
                    self.host_used = None;
                    self.refused = Some(("host", "host admission poisoned; rebuilding from owned handles".into()));
                    let mut h = e.into_inner();
                    h.reload(crate::host::Policy::read(&self.cfg.install));
                    if h.rebuild_slots(&self.cfg.root, self.entry().held) { host.clear_poison(); }
                }
            }
            return;
        }
        let (r, used) = crate::admission::sync(&self.cfg.install, &self.cfg.root.display().to_string(), self.entry(), None, self.wall);
        self.host_used = used;
        if let (Err((_, e)), true) = (r, self.auto()) {
            self.refused = Some(("host", e));
        }
    }

    /// The `capacity` object of `status.json` (§ 4).
    pub fn capacity_json(&self) -> Value {
        let (mut gate, mut reasons) = match &self.cap {
            Err(e) => ("unknown", vec![("config", e.clone())]),
            Ok(_) => self.gate.state(self.mono),
        };
        if let Some(h) = &self.hold {
            (gate, reasons) = ("held", [vec![("hold", h.clone())], reasons].concat());
        }
        if let Some((code, text)) = self.refused.clone().filter(|_| self.auto()) {
            gate = if code == "host" { "unknown" } else if gate == "open" { "held" } else { gate };
            reasons.push((code, text));
        }
        // § 4 Focus: an empty focus admits nothing, so it is a reason like any refusal (#974)
        if self.focus.is_empty() {
            reasons.push(("focus", self.focus_error.clone().unwrap_or_else(|| FOCUS_OFF.into())));
        }
        reasons.extend(self.refused_access.iter().map(|(rt, r)| ("access", format!("{rt}: {r}"))));
        reasons.extend(self.limited.keys().map(|rt| ("usage-limit", format!("{rt}: {}", self.limit_line(rt)))));
        reasons.extend(self.logged_out.iter().map(|rt| ("login", format!("{rt}: not logged in; {}", crate::runtime::relogin(rt)))));
        let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
        if accounts.error.is_some() { gate="unknown"; }
        reasons.extend(accounts.heartbeat(self.board.now().unwrap_or(self.wall)).into_iter().filter(|l|l.starts_with("quota ")).map(|l|("usage-limit",l)));
        reasons.extend(accounts.policy.keys().filter(|rt| accounts.off(rt)).map(|rt| ("capability", format!("{rt}: every account disabled on this host"))));
        let res =|r: &Res, state: &str| json!({"task": r.0, "attempt": r.1, "rt": r.2, "state": state});
        let reservations: Vec<Value> = self.kids.iter().map(|k| res(&(k.task, k.attempt, k.rt.clone()), "held"))
            .chain(self.reserved.iter().map(|r| res(r, "held"))).chain(self.requested.iter().map(|r| res(r, "requested"))).collect();
        let limits = self.cap.as_ref().map(|c| c.limits.clone()).unwrap_or_default();
        let mut used: BTreeMap<String, u32> = limits.keys().map(|k| (k.clone(), 0)).collect();
        for (_, _, rt) in self.entry().held {
            *used.entry(rt).or_default() += 1;
        }
        let last = self.gate.last.filter(|_| self.gate.enabled());
        let (host_policy,host_used,host_command,project_revision)=self.host.as_ref().and_then(|h|h.lock().ok().map(|h| {
            if h.rebuilding {gate="unknown";reasons.push(("host","host occupancy recovery pending".into()));}
            if let Some(reason)=&h.held {gate="unknown";reasons.push(("host",reason.clone()));}
            (h.policy.as_ref().map(|p|p.cap.json()).unwrap_or(Value::Null),if h.rebuilding {Value::Null}else{json!(h.used())},json!(h.capacity_command),json!(self.project_revision.as_ref().filter(|_|self.cap.is_ok())))
        })).unwrap_or((Value::Null,Value::Null,Value::Null,Value::Null));
        json!({"host_policy":host_policy,"host_runtime_used":host_used,"host_command":host_command,"project_revision":project_revision,"project_limits":self.cap.as_ref().ok().map(|_| &self.cfg.limits),"limits": limits, "used": used, "load_gate": self.cap.as_ref().map(|c| c.json()["load_gate"].clone()).unwrap_or(Value::Null),
            "admission": self.cap.as_ref().map(|c| c.json()["admission"].clone()).unwrap_or(Value::Null), "host_used": self.host_used,
            "min_service_version": crate::admission::MIN_SERVICE, "reservations": reservations,
            "gate": gate, "reasons": reasons.iter().map(|(c, t)| json!({"code": c, "text": t})).collect::<Vec<_>>(),
            "cpu_percent": last.and_then(|l| l.2), "memory_available_percent": last.and_then(|l| l.3),
            "sample_at": last.map(|l| l.1), "sample_age_seconds": last.map(|l| self.mono.saturating_sub(l.0))})
    }

    /// Handover carries execution identity; only board acceptance establishes delivery.
    pub fn adopt(&mut self, kid: Kid) -> Result<(),String> {
        let log = crate::board::entries(self.board,kid.task)?;
        let projection=core::project(kid.task,&log,false);
        let x=projection.executions.iter().find(|x|x.id==kid.execution && x.attempt==kid.attempt && x.host==self.cfg.me && x.runtime==kid.rt && x.release.is_none())
            .ok_or_else(||format!("execution {} handover does not match unresolved board ownership",kid.execution))?;
        if self.kids.iter().any(|k|k.task==kid.task || k.execution==kid.execution) { return Err("duplicate native handover".into()); }
        if let Some(through) = projection.delivered(kid.attempt) { self.delivered.insert(kid.attempt, through); }
        // the role is the attempt log that exists; its start and offset stayed in the replaced image
        let role = if crate::runtime::attempt_log(&self.cfg.root, "reviewer", kid.task, kid.attempt).exists() { "reviewer" } else { "worker" };
        self.begun.insert(kid.attempt, (role.into(), kid.rt.clone(), None));
        self.observations.insert((kid.task,kid.execution),x.clone());
        self.kids.push(kid);
        Ok(())
    }

    pub fn delivered(&self, attempt: u64) -> u64 {
        self.delivered.get(&attempt).copied().unwrap_or(0)
    }

    /// The `build` or `review` stage of a child that exited or was stopped (§ 4 Profiling); `v` is its task's view.
    fn ended(&mut self, n: u64, a: u64, v: Option<&View>, failure: Option<&'static str>) {
        let Some((role, rt, begun)) = self.begun.remove(&a) else { return };
        // an issue that left the board read (closed, label removed) is read back once for its hand-in
        let back = if v.is_none() { crate::board::entries(self.board, n).map_err(|_| "unreadable") } else { Ok(vec![]) };
        let log = match v {
            Some(v) => Ok((v.log.as_slice(), Some(v.projection.task.attempt == Some(a)))),
            None => back.as_deref().map(|l| (l, None)).map_err(|e| *e),
        };
        let (outcome, err) = core::stage_outcome(n, log, a, a.max(self.delivered(a)));
        let err = if outcome == "failed" { failure.unwrap_or(err) } else { err };
        let log = crate::runtime::attempt_log(&self.cfg.root, &role, n, a);
        let stage = if role == "reviewer" { "review" } else { "build" };
        let (took, usage) = match begun {
            Some((began, pos)) => (Some(began.elapsed()), crate::runtime::usage(&log, pos, &rt)),
            None => (None, crate::profile::Usage::Unknown("adopted")),
        };
        crate::profile::stage(stage, n, a, &rt, took, outcome, err, usage);
    }

    /// The worker's last message in the attempt log as a public `gone` reason line of at most 240 characters
    /// (§ 4 step 1); empty when unreadable or blank.
    fn last_message(&self, n: u64, a: u64, t: &Task) -> String {
        const LABEL: &str = "Last message: ";
        let log = crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a);
        let line = crate::runtime::last_message(&log).map(|m| crate::runtime::public_line(&m, &self.cfg.root, 240 - LABEL.len()));
        line.filter(|m| !m.is_empty()).map_or_else(String::new, |m| format!("\n{LABEL}{m}"))
    }

    /// End a released attempt; only ordinary failures spend the retry budget (§ 2).
    fn gone(&mut self, n: u64, t: &Task, a: u64, args: &[&str], text: &str) -> Result<u64, String> {
        let text = if args.is_empty() { core::limit_text(n, t, "gone", text) } else { text.to_string() };
        let kind = if args == ["usage-limit"] { "usage-limit" } else { "gone" };
        let id = self.post(n, core::entry(kind, &[], Some(a), &text))?;
        if let Ok(log) = crate::board::entries(self.board, n) {
            let through: Vec<_> = log.into_iter().filter(|e| e.id <= id).collect();
            let before: Vec<_> = through.iter().filter(|e| e.id < id).cloned().collect();
            if core::fold(n, &before, false).attempt == Some(a) && core::fold(n, &through, false).attempt != Some(a) {
                crate::profile::repair(&self.cfg.install, &self.cfg.repo, host_of(t).unwrap_or(&self.cfg.me), n, a,
                    rt_of(t).map_or("other", |r| r.0), t.role.as_deref() == Some("reviewer"));
            }
        } else {
            eprintln!("Profiling: unknown (gone readback failed; terminal record not repaired)");
        }
        Ok(id)
    }

    /// #1879: `x` is task `n`'s initial spawn on this host and no start reached its native call: every start writes the
    /// attempt log's rules line first, and every start error before it is a no-start, so a missing log with no child, held
    /// launch or acceptance is a confirmed no-start.
    fn lost_spawn(&self, n: u64, x: &core::Execution) -> bool {
        x.id == x.attempt && x.host == self.cfg.me && !x.accepted && x.release.is_none()
            && !self.kids.iter().any(|k| k.task == n) && !self.held.iter().any(|l| l.task == n)
            && ["worker", "reviewer"].iter().all(|r| crate::runtime::attempt_log(&self.cfg.root, r, n, x.attempt).try_exists().is_ok_and(|e| !e))
    }

    fn exit_outcome(&mut self, n: u64, t: &Task, a: u64, hold: Option<(i64, bool)>, now: i64, text: &str) -> Result<u64, String> {
        let local = host_of(t) == Some(self.cfg.me.as_str());
        let hold = hold.or_else(|| local.then(|| {
            let log = crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a);
            crate::runtime::usage_limit(&log).map(|reset| reset.filter(|r| *r > now).map_or((now + USAGE_PROBE, false), |r| (r, true)))
        }).flatten());
        if let (Some(hold), Some((rt, _))) = (hold, rt_of(t)) {
            let (at, reported) = hold;
            // § 3 Runtime accounts: the limit is the attempt account's; a spare enabled account takes the next start
            let (next, hold) = if local { self.account_limit(n, t, a, rt, hold, now) } else { (None, hold) };
            if local && next.is_none() { self.limited.insert(rt.to_string(), hold); }
            let next = next.map(|l| format!("; next account {l}")).unwrap_or_default();
            let reason = format!("Usage limit: {rt} {} {}{next}", if reported { "until" } else { "probe" }, crate::board::stamp(at));
            self.gone(n, t, a, &["usage-limit"], &reason)
        } else if local && matches!(t.state,State::Doing|State::Review) && rt_of(t).is_some_and(|(rt,_)| crate::runtime::CLI_RUNTIMES.contains(&rt))
            && !crate::runtime::quota_windows(&crate::runtime::attempt_log(&self.cfg.root,t.role.as_deref().unwrap_or_default(),n,a)).is_empty() {
            let rt = rt_of(t).unwrap().0;
            self.account_limit(n,t,a,rt,(now+USAGE_PROBE,false),now);
            self.post(n,core::entry("steer",&["quota"],Some(a),"Continue this same task and native session after the original account quota hold; preserve completed work."))
        } else if let Some((bin, reason)) = local.then(|| rt_of(t).map(|(rt, _)| crate::runtime::of(rt)).filter(|r| r.codex)
            .and_then(|r| crate::runtime::auth_rejected(&crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a)).map(|e| (r.bin, e)))).flatten() {
            // #1446: a rejected login holds Codex here at once and releases the attempt like a usage limit, retries kept
            if let Err(e) = crate::runtime::mark_login_off(&self.cfg.install, bin, &reason) { eprintln!("login-off not saved: {e}"); }
            self.logged_out.extend(crate::runtime::agent_runtimes().iter().filter(|r| crate::runtime::of(r).codex).map(|r| r.to_string()));
            self.gone(n, t, a, &["usage-limit"], &format!("Login off: {bin} {reason}; {}", crate::runtime::relogin(bin)))
        } else if let Some(steer) = local.then(|| self.continuation(n, t, a)).flatten() {
            self.post(n, steer)
        } else {
            self.gone(n, t, a, &[], text)
        }
    }

    /// #1843: the `steer` that resumes worker attempt `a` after a [transient model failure](docs/design/capacity.md#transient-model-failure)
    /// with its Codex session known; None past [`CONTINUE_MAX`] or when the board log is unread.
    fn continuation(&self, n: u64, t: &Task, a: u64) -> Option<String> {
        if t.state != State::Doing || t.role.as_deref() != Some("worker") || !rt_of(t).is_some_and(|(rt, _)| crate::runtime::of(rt).codex) { return None; }
        let log = crate::runtime::attempt_log(&self.cfg.root, "worker", n, a);
        let failure = crate::runtime::transient_failure(&log)?;
        crate::runtime::codex_thread(&log)?;
        let head = core::entry("steer", &[], Some(a), "");
        let prior = crate::board::entries(self.board, n).ok()?.iter().filter(|e| !e.edited && e.body.lines().next() == Some(head.as_str())).count();
        (prior < CONTINUE_MAX).then(|| core::entry("steer", &[], Some(a), &format!(
            "Continue the task from where the last turn stopped; it failed on a transient model error: {}",
            crate::runtime::public_line(&failure, &self.cfg.root, 160))))
    }

    /// § 4 step 5 (#1950): read each task gone from the open list once; a failed read or write retries next pass.
    /// Returns whether one was reopened. `ponytail:` a close while the attempt's host runs no service is not seen;
    /// § 1 still reads that dependency unknown, never done.
    fn unlisted_tasks(&mut self) -> bool {
        let mut reopened = false;
        for n in std::mem::take(&mut self.unlisted) {
            match self.unlisted_task(n) {
                Ok(r) => reopened |= r,
                Err(e) => {
                    self.log.push(format!("#{n} gone from the open list, unread: {e}"));
                    self.unlisted.insert(n);
                }
            }
        }
        reopened
    }

    /// Closed in `doing` on this host: reopen it, then one `ask` of that attempt with the close as evidence, so the
    /// close never counts as done (desiign#50: a worker closed its own task). Closed otherwise: drop stale `q-*` labels.
    fn unlisted_task(&mut self, n: u64) -> Result<bool, String> {
        let issue = self.board.issue(n)?;
        let t = core::fold(n, &crate::board::entries(self.board, n)?, false);
        match t.attempt.filter(|_| t.state == State::Doing && host_of(&t) == Some(self.cfg.me.as_str())) {
            Some(a) => {
                if !issue.open { self.board.reopen(n)?; }
                let at = issue.closed.map_or_else(|| "an unknown time".to_string(), crate::board::stamp);
                self.post(n, core::entry("ask", &[], Some(a), &closed_card(n, &at)))?;
                Ok(true)
            }
            None if !issue.open => {
                let stale: Vec<String> = issue.labels.iter().filter(|l| l.starts_with("q-")).cloned().collect();
                if !stale.is_empty() { self.board.labels(n, &[], &stale)?; }
                Ok(false)
            }
            None => Ok(false),
        }
    }

    fn post(&mut self, n: u64, body: String) -> Result<u64, String> {
        self.log.push(format!("#{n} {}", body.lines().next().unwrap_or_default()));
        self.cache.remove(&n);
        self.board.comment(n, &body)
    }

    /// #1594 the signals this host holds for one of its executions.
    fn signals(&self, task:u64, x:&core::Execution)->crate::snapshot::Signals<'_> {
        let child=self.kids.iter().find(|k|k.execution==x.id && k.task==task && k.attempt==x.attempt);
        crate::snapshot::Signals {local:true,
            child:child.map(|k|crate::snapshot::Child {unknown:k.unknown.as_deref(),stopping:k.stopping,..Default::default()}),
            held:self.held.iter().any(|l|l.task==task && l.attempt==x.attempt),
            wait:self.execution_waits.get(&task).map(String::as_str),
            input_pending:self.delivered(x.attempt)<x.through,
            ask:self.snapshot.as_ref().is_some_and(|s|s.tasks.iter().any(|r|r.n==task && r.task.state==State::Ask)),
            ..Default::default()}
    }

    fn execution_statuses(&self)->Vec<crate::snapshot::ExecutionStatus> {
        let mut statuses:Vec<_>=self.observations.iter().map(|((task,_),x)| {
            let (state,reason)=crate::snapshot::verdict(&self.signals(*task,x));
            crate::snapshot::ExecutionStatus {task:*task,attempt:x.attempt,id:x.id,through:x.through,state:state.into(),reason}
        }).collect();
        if let Some(snapshot)=&self.snapshot {
            statuses.extend(snapshot.tasks.iter().flat_map(|r|&r.executions).filter(|x|x["host"]==self.cfg.me && x["release"]=="exit")
                .filter_map(|x|serde_json::from_value(x.clone()).ok()));
        }
        statuses
    }

    pub fn service_diagnostics(&self)->Value {
        if let Some((at,value))=self.diagnostics.borrow().as_ref().filter(|(at,_)|at.elapsed().as_secs()<60) {let _=at;return value.clone();}
        let selected=crate::platform::tool(&self.cfg.install).current();
        let pending=selected.as_deref().is_some_and(|v|v!=self.version)
            || self.cfg.install.join("restart").exists() || crate::dot(&self.cfg.root).join("restart").exists();
        let waiting=if let Some(host)=&self.host {
            host.lock().map(|h|h.restart&&h.handoff_wait().is_some()).unwrap_or(false)
        } else {pending&&!crate::lifecycle_waits(self,false).is_empty()};
        let value=crate::service_versions::collect(&self.cfg.install,&self.version,&self.instance,self.started,waiting);
        *self.diagnostics.borrow_mut()=Some((std::time::Instant::now(),value.clone()));
        value
    }

    /// § 3 Runtime accounts: records attempt `a`'s usage limit on the account its log names. Returns the spare account
    /// the next start takes, else the hold: the earliest limit end of an enabled account when there are several.
    fn account_limit(&mut self, n: u64, t: &Task, a: u64, rt: &str, hold: (i64, bool), now: i64) -> (Option<String>, (i64, bool)) {
        if !crate::runtime::CLI_RUNTIMES.contains(&rt) { return (None, hold); }
        let log = crate::runtime::attempt_log(&self.cfg.root, t.role.as_deref().unwrap_or_default(), n, a);
        let label = crate::runtime::accounts::of_log(&log);
        let (end, reported) = hold;
        let seen = if reported { now } else { end - USAGE_PROBE };
        let mut windows = crate::runtime::quota_windows(&log);
        if windows.is_empty() { windows.push(("unknown".into(),reported.then_some(end))); }
        if let Err(e) = crate::runtime::accounts::record_windows(&self.cfg.install, rt, &label, seen, &windows) {
            self.log.push(format!("account {rt} {label} limit not saved: {e}"));
        }
        let h = crate::runtime::accounts::Host::read(&self.cfg.install);
        match h.spare(rt, now) {
            Some(next) => (Some(next), hold),
            None => (None, h.hold(rt, now).unwrap_or(hold)),
        }
    }

    /// § 3 Runtime accounts, every pass: holds of runtimes with several accounts follow the account record, and each
    /// change of a runtime's active account goes to the service log and telemetry.
    fn account_pass(&mut self, now: i64) {
        if let Err(e)=crate::runtime::accounts::recover_pending(&self.cfg.install) {self.log.push(format!("account quota recovery unknown: {e}"));}
        let h = crate::runtime::accounts::Host::read(&self.cfg.install);
        for rt in h.policy.keys().chain(h.state.keys()).collect::<BTreeSet<_>>() {
            match (h.active(rt, now), h.hold(rt, now)) {
                (None, Some(hold)) => { self.limited.insert(rt.clone(), hold); }
                _ => { self.limited.remove(rt); }
            }
        }
        for s in crate::runtime::accounts::switches(&h, now, &mut self.accounts) {
            self.log.push(s.line());
            crate::profile::measure_counted("account.switch", || ((), s.counts(&self.cfg.me)));
        }
    }

    /// `until <time>` for a reported reset, else `probe <time>`, of a held runtime.
    fn limit_line(&self, rt: &str) -> String {
        self.limited.get(rt).map(|(at, reset)| format!("{} {}", if *reset { "until" } else { "probe" }, crate::board::stamp(*at))).unwrap_or_default()
    }

    fn heartbeat_body(&self) -> String {
        let mut lines = vec![format!("version {}", self.version),format!("balancing {}",serde_json::to_string(&self.balancing()).unwrap())];
        if !self.instance.is_empty() {
            lines.push(format!("service {}", self.instance));
            lines.push(format!("diagnostics {}", self.service_diagnostics()));
        }
        if !self.machine.is_empty() {
            lines.push(format!("machine {}", self.machine));
        }
        if !self.link.is_empty() {
            lines.push(self.link.clone());
        }
        if self.focus.is_empty() {
            lines.push(format!("focus {}", if self.focus_error.is_some() { "unknown" } else { "off" }));
        }
        if self.gui().is_ok() {
            lines.push("has gui".into());
        }
        lines.extend(self.limited.keys().map(|rt| format!("limit {rt} {}", self.limit_line(rt))));
        lines.extend(self.logged_out.iter().map(|rt| format!("login {rt} off")));
        lines.extend(self.clis.iter().map(|(bin, v)| format!("cli {bin} {v}")));
        // § 3 Runtime accounts: labels and state only, never an email, path or token
        lines.extend(crate::runtime::accounts::Host::read(&self.cfg.install).heartbeat(self.board.now().unwrap_or_else(|_| crate::runtime::accounts::now())));
        lines.extend(tray_fault(&self.cfg.install).map(|code| format!("tray {code}")));
        // #1985, #2084: this host's billed requests of its last full hour, in all and per API host: its share of each
        // board user's quota
        lines.extend(self.host.as_ref().and_then(|h| { let h = h.lock().ok()?; Some(crate::requests::heartbeat(h.last_hour.as_ref()?, h.hosts_last_hour.as_ref())) }).unwrap_or_default());
        let kids: BTreeSet<_> = self.kids.iter().map(|k| (k.task, k.attempt)).chain(self.pending.iter().copied()).collect();
        lines.extend(kids.iter().map(|(t, a)| format!("#{t} {a}")));
        lines.extend(self.kids.iter().filter_map(|k| k.session_url.as_ref().map(|u| format!("session #{} {} {u}", k.task, k.attempt))));
        lines.extend(self.execution_statuses().iter().map(|s|format!("execution {}",serde_json::to_string(s).unwrap())));
        lines.join("\n")
    }

    /// #1819: the next pass reads `task`'s comments from the board, not from the cache.
    pub fn reread(&mut self, task: u64) {
        self.cache.remove(&task);
    }

    /// One pass of § 4. Tasks written to in a step are left alone by later steps until the next pass re-reads them.
    #[tracing::instrument(name="service.Service.pass",skip_all)]
    pub fn pass(&mut self) -> Result<(), String> {
        self.full_stop_owned();
        self.ownership_known = false;
        if self.host.is_none() {
            if let Err(e) = crate::profile::prune(&self.cfg.install) { eprintln!("taskq: telemetry unknown: {e}"); }
        }
        let r = crate::profile::measure("service.pass", || self.pass_inner(NATIVE_PASS_BUDGET));
        if r.is_err() { self.unready(); }
        let observations=self.execution_statuses();
        if let Some(snapshot) = &mut self.snapshot {
            for row in &mut snapshot.tasks {
                row.executions.retain(|value|value["host"].as_str()!=Some(&self.cfg.me)
                    || observations.iter().any(|s|s.task==row.n && Some(s.id)==value["id"].as_u64()));
                for value in &mut row.executions {
                    if let Some(status)=observations.iter().find(|s|s.task==row.n && Some(s.id)==value["id"].as_u64()
                        && Some(s.through)==value["through"].as_u64()) {
                        value["state"]=json!(status.state);value["reason"]=json!(status.reason);
                        value["host"]=json!(self.cfg.me);value["observed"]=json!(snapshot.observed);
                    }
                }
                if row.executions.iter().any(|x|x["host"].as_str()==Some(&self.cfg.me)) {
                    row.execution_wait=row.executions.iter().find(|x|x["state"]!="running" || x["attempt"].as_u64()!=row.task.attempt)
                        .map(|x|format!("execution {} on {}: {}: {}",x["id"],self.cfg.me,
                            x["state"].as_str().unwrap_or("unknown"),x["reason"].as_str().unwrap_or("observation unavailable")));
                }
                if !self.kids.iter().any(|k| k.task == row.n && row.task.attempt == Some(k.attempt)) {
                    if let Some(reason)=self.execution_waits.get(&row.n).or_else(|| self.waits.get(&row.n)) {row.execution_wait=Some(reason.clone());}
                }
            }
        }

        r
    }

    /// #1386: a boot strictly later than its intent proves every local process of an execution ended. A failed boot read
    /// or a missing intent time writes nothing; the execution stays in `execution_waits`. True when a release was written.
    fn release_before_boot(&mut self, views: &[View]) -> bool {
        let mut wrote = false;
        let mut boot = None;
        for v in views {
            for x in v.projection.executions.iter().filter(|x| x.host == self.cfg.me && x.release.is_none()) {
                if self.kids.iter().any(|k| k.execution == x.id) { continue; }
                let Some(intent) = v.comments.iter().find(|c| c.id == x.id && c.trusted).map(|c| c.created).filter(|t| *t > 0) else { continue };
                let Ok(at) = *boot.get_or_insert_with(self.boot) else { continue };
                let ("ended", proof) = crate::snapshot::verdict(&crate::snapshot::Signals { local: true, boot: Some((at, intent)), ..Default::default() }) else { continue };
                match release_execution_text(self.board, v.issue.number, x.attempt, x.id, "exit", &proof) {
                    Ok(()) => { wrote = true; self.log.push(format!("#{} released {} exit: {proof}", v.issue.number, x.id)); }
                    Err(e) => self.log.push(format!("#{} boot release: {e}", v.issue.number)),
                }
            }
        }
        wrote
    }

    fn pass_inner(&mut self, native_budget:std::time::Duration) -> Result<(), String> {
        for (task, attempt, execution) in self.pending_releases.clone() {
            release_execution(self.board, task, attempt, execution, "no-start")?;
            self.retired(task,attempt,execution,false);
        }
        let previous_fixes = std::mem::take(&mut self.label_fixes);
        let me = self.cfg.me.clone();
        let ttl = self.cfg.ttl;
        let (mut views, mut hosts) = views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?;
        // §5: an interrupted owner reroute remains fenced and finishes without a second owner request.
        let mut rerouted=false;
        for v in &views {
            if v.projection.reroute.is_some() {
                let receipt=crate::dispatch::finish_reroute(self.board,v.issue.number);
                if receipt.ok() {self.cache.remove(&v.issue.number);rerouted=true;}
                else {self.log.push(receipt.text());}
            }
        }
        if rerouted {(views,hosts)=self::views(self.board,&mut self.cache,&mut self.deps,self.cfg.ttl)?;}
        if self.release_before_boot(&views) { (views, hosts) = self::views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?; }
        // § 4 step 5 (#1950): a task reopened here is listed again before its live attempt could read as gone below
        let open: HashSet<u64> = views.iter().map(|v| v.issue.number).collect();
        self.unlisted.extend(self.listed.difference(&open).copied());
        self.listed = open;
        if self.unlisted_tasks() {
            (views, hosts) = self::views(self.board, &mut self.cache, &mut self.deps, self.cfg.ttl)?;
            self.listed = views.iter().map(|v| v.issue.number).collect();
        }
        let now = self.board.now()?;
        let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
        // § 3 Runtime accounts: a runtime with several accounts takes its hold from the account record (`account_pass`)
        for x in views.iter().flat_map(|v| &v.projection.executions).filter(|x| x.host == me && !accounts.multi(&x.runtime)) {
            if let Some(hold) = x.usage_hold.filter(|(at, _)| now < *at) {
                self.limited.entry(x.runtime.clone()).and_modify(|old| { if hold.0 > old.0 { *old = hold; } }).or_insert(hold);
            }
        }
        self.account_pass(now);
        self.limited.retain(|_, (at, _)| now < *at);
        self.observe_ownership(&views)?;
        self.snapshot = Some(crate::snapshot::from_views("host", self.board, &self.cfg, now, &views, &hosts));
        self.copy_images();
        self.tasks_read = Some(std::time::Instant::now());
        self.log_stuck(now);
        self.log_churn();
        // Local eligibility only: another host's heartbeat cannot qualify this runtime or hide its refusal.
        self.waits.clear();
        for v in &views {
            let t = &v.projection.task;
            if !matches!(t.state, State::Ready | State::Doing | State::Review)
                || label(&v.issue, "host-").is_some_and(|h| h != me)
                || t.attempt.is_some() && host_of(t) != Some(me.as_str())
                || self.kids.iter().any(|k| k.task == v.issue.number) { continue; }
            let rt = rt_of(t).map(|(rt, _)| rt.to_string()).unwrap_or_else(|| self.select_runtime(v).map(|(rt, _)| rt)
                .unwrap_or_else(|_| route(&v.issue, t.state == State::Review, &self.cfg.default_runtime)));
            if let Some(reason) = self.gui_reason(&v.issue, Some(&rt)) {
                self.waits.insert(v.issue.number, reason);
            }
        }
        // § 4 one service per host name: another install's fresh heartbeat under this name holds every write (#933)
        let title = format!("taskq host {me}");
        let other = hosts.iter().filter(|h| h.title == title && now - h.updated < ttl).flat_map(|h| h.body.lines()).find_map(|l| l.strip_prefix("service ")).filter(|id| *id != self.instance);
        if let Some(id) = other {
            return Err(format!("another service {id} runs as host {me}; stop it or set TASKQ_HOST"));
        }
        self.events(&views, now)?;
        let by_n: HashMap<u64, &View> = views.iter().map(|v| (v.issue.number, v)).collect();
        let mut touched = HashSet::new();
        let mut released = HashSet::new();
        self.execution_waits.clear();
        // #1886: a lost spawn no longer the live attempt (requeued, parked) waits on nothing; any run of its host, a
        // retired one too, releases it no-start so another host can claim the task. A reached start (attempt log or a
        // hand-in of that attempt) stays unknown (R12).
        let stale_spawns: Vec<_> = views.iter().flat_map(|v| {
            let (n, t) = (v.issue.number, &v.projection.task);
            v.projection.executions.iter().filter(move |x| t.attempt != Some(x.attempt) || !matches!(t.state, State::Doing | State::Review))
                .filter(move |x| !core::handed_in(n, &v.log, x.attempt, x.attempt)).map(move |x| (n, x))
        }).filter(|(n, x)| self.lost_spawn(*n, x)).map(|(n, x)| (n, x.attempt)).collect();
        for (n, a) in stale_spawns {
            release_execution(self.board, n, a, a, "no-start")?;
            self.retired(n, a, a, false);
            released.insert(a);
            self.log.push(format!("#{n} lost spawn {a}: no attempt log on {me} and no longer the live attempt; released no-start"));
        }
        let native_until=std::time::Instant::now()+native_budget;
        // Cold scans leave time for already owned work and control.
        let recovery_until=std::time::Instant::now()+native_budget/4;
        let pivot=self.recovery_after.and_then(|n|views.iter().position(|v|v.issue.number==n)).map_or(0,|i|i+1);
        for v in views[pivot..].iter().chain(&views[..pivot]) {
            let task = v.issue.number;
            let unresolved: Vec<_> = v.projection.executions.iter().filter(|x| x.release.is_none()).collect();
            if self.kids.iter().any(|k| k.task == task) || self.held.iter().any(|l| l.task == task) { continue; }
            let Some(x) = unresolved.iter().find(|x| x.host == me && { let a = crate::runtime::of(&x.runtime); a.remote || a.app }) else { continue };
            if unresolved.len() != 1 {
                self.execution_waits.insert(task, "multiple unresolved executions; verify native ownership before recovery".into());
                continue;
            }
            // #1557: a launch the runtime refused submitted nothing; release it so the task can start again.
            if let Some(refusal) = crate::runtime::refused_launch(&self.cfg.root, task, x) {
                release_execution(self.board, task, x.attempt, x.id, "no-start")?;
                self.retired(task, x.attempt, x.id, false);
                let t = &v.projection.task;
                if t.attempt == Some(x.attempt) && matches!(t.state, State::Doing | State::Review) {
                    self.gone(task, t, x.attempt, &[], &format!("start failed: Claude background launch refused: {refusal}"))?;
                }
                continue;
            }
            // #1879: nothing native to recover; the resume step releases a lost spawn
            if self.lost_spawn(task, x) { continue; }
            let Some(recover) = &mut self.recover else { continue };
            if std::time::Instant::now()>=recovery_until {
                self.execution_waits.insert(task,format!("execution {}: native observation deferred; recovery budget exhausted",x.id));
                continue;
            }
            self.recovery_after=Some(task);
            let result = crate::runtime::observe_until(recovery_until,||recover(task,x))
                .and_then(|proc| self.adopt(Kid { unknown:None, stopping:false, session_url:None, task, attempt: x.attempt, execution: x.id, rt: x.runtime.clone(), proc }));
            if let Err(e) = result {
                self.execution_waits.insert(task, format!("execution {} on {}: recovery unknown: {e}", x.id, x.host));
            }
        }

        self.hold = crate::admission::hold(&self.cfg.install);
        let mut pending_acceptance = HashSet::new();
        // 1. Observe native state once; retry lost evidence writes without resubmitting input.
        let mut exited = vec![];
        let mut retired = vec![];
        let needs_stop=self.kids.iter().any(|k|!by_n.get(&k.task).is_some_and(|v|v.projection.task.attempt==Some(k.attempt)));
        let observation_until=if needs_stop {std::time::Instant::now()+native_until.saturating_duration_since(std::time::Instant::now())/2} else {native_until};
        let mut deferred=None;
        let quota:Vec<_> = self.kids.iter().filter_map(|k| {
            let log=crate::runtime::attempt_log(&self.cfg.root,by_n.get(&k.task)?.projection.task.role.as_deref().unwrap_or_default(),k.task,k.attempt);
            let windows=crate::runtime::quota_windows(&log);
            (!windows.is_empty()).then(||(k.rt.clone(),crate::runtime::accounts::of_log(&log),windows))
        }).collect();
        for (rt,label,windows) in quota {
            if let Err(e)=crate::runtime::accounts::record_windows(&self.cfg.install,&rt,&label,now,&windows) {self.log.push(format!("account {rt} {label} quota unknown: {e}"));}
        }
        self.kids.retain_mut(|k| {
            if std::time::Instant::now()>=observation_until {
                deferred.get_or_insert(k.execution);
                k.unknown=Some("native observation deferred; pass budget exhausted".into());return true;
            }
            let status = crate::runtime::observe_until(observation_until,||k.proc.exited());
            let confirmed = by_n.get(&k.task).and_then(|v| v.projection.executions.iter().find(|x| x.id == k.execution && x.accepted));
            let accepted = if let Some(x) = confirmed { Ok(Some(x.through)) } else {
                crate::runtime::observe_until(observation_until,||k.proc.accepted()).and_then(|yes| if yes {
                    accept_execution(self.board, k.task, k.attempt, k.execution, None).map(Some).inspect_err(|_| { pending_acceptance.insert(k.attempt); })
                } else { Ok(None) })
            };
            if let Ok(Some(through)) = accepted { self.delivered.entry(k.attempt).and_modify(|v| *v = (*v).max(through)).or_insert(through); self.cache.remove(&k.task); }
            // #1594: an exit counts once input acceptance is resolved; failed reads, then a cancellation's earlier failure, stay unknown
            let unknown = status.as_ref().err().or(accepted.as_ref().err()).or(k.unknown.as_ref().filter(|_| k.stopping)).cloned();
            let child = crate::snapshot::Child { exited: matches!(status, Ok(Some(_))) && !pending_acceptance.contains(&k.attempt), unknown: unknown.as_deref(), stopping: k.stopping };
            match (crate::snapshot::verdict(&crate::snapshot::Signals { local: true, child: Some(child), ..Default::default() }), status) {
            (("ended", _), Ok(Some(code))) => {
                let hold = by_n.get(&k.task).and_then(|v| {
                    let log = crate::runtime::attempt_log(&self.cfg.root, v.projection.task.role.as_deref().unwrap_or_default(), k.task, k.attempt);
                    crate::runtime::usage_limit(&log)
                }).map(|reset| reset.filter(|r| *r > now).map_or((now + USAGE_PROBE, false), |r| (r, true)));
                let evidence = hold.map_or(String::new(), |(at, reported)| format!("usage-limit {} {at}", if reported { "until" } else { "probe" }));
                let no_start=match k.release(self.board,code,&evidence) {Ok(no_start)=>no_start,Err(e)=>{k.unknown=Some(e);return true;}};
                retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).child));
                released.insert(k.execution);
                // This pass's view predates release; it cannot authorize a continuation before acceptance is resolved.
                if by_n.get(&k.task).is_some_and(|v|v.projection.task.attempt==Some(k.attempt))
                    && (no_start || !matches!(accepted,Ok(Some(_)))) {touched.insert(k.task);}
                if !no_start {exited.push((k.task, k.attempt, code, k.proc.exit_error()));}
                false
            }
            (("unknown", e), _) => { k.unknown=Some(e); true }
            _ => { k.unknown=None; true }
            }
        });
        for (task,attempt,execution,native) in retired.drain(..) { self.retired(task,attempt,execution,native); }
        for (n, a, code, failure) in exited {
            self.ended(n, a, by_n.get(&n).copied(), failure);
            if code == 0 && let Some(v) = by_n.get(&n) && let Some((rt,_)) = rt_of(&v.projection.task) {
                let log = crate::runtime::attempt_log(&self.cfg.root,v.projection.task.role.as_deref().unwrap_or_default(),n,a);
                if crate::runtime::quota_windows(&log).is_empty() && let Err(e) = crate::runtime::accounts::confirm_probe(&self.cfg.install,rt,&crate::runtime::accounts::of_log(&log),&log) {
                    self.log.push(format!("account {rt} recovery unknown: {e}"));
                }
            }
            if let Some(v) = by_n.get(&n)
                && v.projection.task.attempt == Some(a)
                && matches!(v.projection.task.state, State::Doing | State::Review)
                && !core::handed_in(n, &v.log, a, a.max(self.delivered(a)))
            {
                let t = &v.projection.task;
                let text = format!("exit {code}{}", self.last_message(n, a, t));
                self.exit_outcome(n, t, a, None, now, &text)?;
                touched.insert(n);
            }
        }

        // A replaced, once-accepted attempt is ordinary cancellation, not a double claim.
        for k in &self.kids {
            if let Some(v) = by_n.get(&k.task) {
                let history: Vec<_> = v.log.iter().filter(|e| e.id <= k.attempt).cloned().collect();
                if core::fold(k.task, &history, false).attempt != Some(k.attempt)
                {
                    crate::incident::detect("invariant.double_attempt", concat!("src/service.rs:", line!()), &k.rt, &[], &["claim:rejected"]);
                }
            }
        }

        // 2. stop children of replaced attempts (R11)
        let mut stopped = vec![];
        self.kids.retain_mut(|k| {
            let current = by_n.get(&k.task).is_some_and(|v| v.projection.task.attempt == Some(k.attempt));
            if !current {
                k.stopping=true;
                if std::time::Instant::now()>=native_until {
                    deferred.get_or_insert(k.execution);
                    k.unknown=Some("native observation deferred; cancellation pending".into());return true;
                }
                let stop=crate::runtime::observe_until(native_until,||crate::runtime::stop(k.proc.as_mut()));
                let child=crate::snapshot::Child {exited:stop.is_ok(),unknown:stop.as_ref().err().map(String::as_str),stopping:true};
                match crate::snapshot::verdict(&crate::snapshot::Signals {local:true,child:Some(child),..Default::default()}) {
                    _ if stop.is_ok() && pending_acceptance.contains(&k.attempt) => return true,
                    ("ended",_) => match k.release(self.board,stop.unwrap(),"") {
                        Ok(_)=>{
                            retired.push((k.task,k.attempt,k.execution,crate::runtime::of(&k.rt).child));
                            released.insert(k.execution);stopped.push((k.task,k.attempt));
                        },
                        Err(e)=>{k.unknown=Some(e);return true;}
                    },
                    (_,e) => { k.unknown=Some(e); return true; }
                }
            }
            current
        });
        for (task,attempt,execution,native) in retired { self.retired(task,attempt,execution,native); }
        for (n, a) in stopped {
            self.ended(n, a, by_n.get(&n).copied(), None);
        }
        self.held.retain(|l| by_n.get(&l.task).is_some_and(|v| v.projection.task.attempt == Some(l.attempt)));
        // A Claude Remote Control URL belongs to this exact live execution.
        // shortcut: bounded transcript scans restart until a URL appears; retain a cursor if costly.
        for k in self.kids.iter_mut().filter(|k| std::time::Instant::now()<native_until && crate::runtime::of(&k.rt).remote && k.session_url.is_none()) {
            k.session_url = by_n.get(&k.task).filter(|v| v.projection.task.attempt == Some(k.attempt))
                .and_then(|v| crate::runtime::remote_session(v.projection.task.session.as_deref()?))
                .and_then(|id| crate::runtime::observe_until(native_until,||crate::runtime::claude_remote(id)));
        }
        if let Some(first)=deferred.and_then(|id|self.kids.iter().position(|k|k.execution==id)) {self.kids.rotate_left(first);}

        // 4. Expired heartbeat is not evidence that an external executor stopped.
        // A lease newer than the accepted attempt is a spawn that lost the claim and still runs. An older one is a
        // replaced attempt its host lists until its next pass: normal lag, not a second live attempt (#1140).
        for hb in hosts.iter().filter(|h| now - h.updated <= ttl) {
            for line in hb.body.lines() {
                let Some((n, a)) = line.strip_prefix('#').and_then(|l| l.split_once(' ')) else { continue };
                let (Ok(n), Ok(a)) = (n.parse::<u64>(), a.parse::<u64>()) else { continue };
                if by_n.get(&n).and_then(|v| v.projection.task.attempt).is_some_and(|c| a > c) {
                    crate::incident::detect("invariant.double_attempt", concat!("src/service.rs:", line!()), "none", &[], &["heartbeat:conflict"]);
                }
            }
        }
        let mut unresolved_local = BTreeSet::new();
        for ((task,_),x) in &self.observations {
            let reservation=(*task,x.attempt,x.runtime.clone());
            unresolved_local.insert(reservation.clone());
            if !self.kids.iter().any(|k|k.task==*task && k.execution==x.id) && !self.reserved.contains(&reservation) {self.reserved.push(reservation);}
        }
        for v in &views {
            let (n,t) = (v.issue.number,&v.projection.task);
            if let Some(a) = t.attempt.filter(|_| host_of(t) != Some(&me) && matches!(t.state,State::Doing|State::Review)) {
                let delivered = v.projection.delivered(a).unwrap_or(0);
                if t.inbox.iter().all(|id| *id <= delivered) {
                    let executions = &v.projection.executions;
                    if let Some(x) = executions.iter().rev().find(|x| x.attempt == a).filter(|_| executions.iter().all(|x| x.release.is_some())) {
                        if !core::handed_in(n,&v.log,a,x.through) {
                            self.exit_outcome(n,t,a,x.usage_hold,now,&format!("execution {} confirmed {:?} without task outcome",x.id,x.release.unwrap()))?;
                            touched.insert(n);
                        }
                    }
                }
            }
            // #1491: the current attempt that its owner's fresh heartbeat lists as running is observed, not a wait
            let remote_running = |x: &core::Execution| x.host != me && Some(x.attempt) == v.projection.task.attempt
                && crate::snapshot::execution_status(v.issue.number, x, &hosts, now, ttl)["state"] == "running";
            let unresolved: Vec<_> = v.projection.executions.iter()
                .filter(|x| x.release.is_none() && !released.contains(&x.id) && !remote_running(x)).collect();
            if let Some(x) = unresolved.first().filter(|x| !self.held.iter().any(|l| l.execution.unwrap_or(l.attempt) == x.id)) {
                // #1886: name the host step that can resolve it
                let step = if x.host == me { format!("reboot {me} to prove exit") } else { format!("start the taskq service on {} once, after a reboot if the run had started", x.host) };
                self.execution_waits.entry(v.issue.number).or_insert_with(|| format!("execution {} on {}: termination unconfirmed; {step}", x.id, x.host));
            } else if let Some(x) = crate::snapshot::input_recovery(v) {
                self.execution_waits.insert(v.issue.number, format!("execution {} on {}: agent exited; {}", x.id, x.host,crate::runtime::acceptance_action(&x.runtime)));
            } else if let (Some(a),Some(owner)) = (v.projection.task.attempt,host_of(&v.projection.task)) {
                if owner != me && matches!(v.projection.task.state,State::Doing|State::Review) && v.projection.task.inbox.iter().any(|id| *id > v.projection.delivered(a).unwrap_or(0)) {
                    self.execution_waits.insert(v.issue.number,format!("input pending on {owner}; await or restore the owning service"));
                }
            }
        }

        // 3. heartbeat
        let body = self.heartbeat_body();
        self.host_issue = hosts.iter().find(|h| h.title == title).map(|h| h.number);
        match hosts.iter().find(|h| h.title == title) {
            // execution rows refresh every 60 s, read within ttl (snapshot.rs); without them ttl / 3 keeps every lease fresh (#1358)
            Some(h) if stable(&h.body) == stable(&body)
                && now - h.updated < if body.lines().any(|l| l.starts_with("execution ")) { 60.min(ttl / 3) } else { ttl / 3 } => {}
            // a same-body edit leaves `updated_at` as is (GitHub), so the stamp keeps the heartbeat fresh
            Some(h) => {
                self.board.edit(h.number, &format!("{body}\nat {now}"))?;
                // #1729 A3: the snapshot read this heartbeat before the write; after a wake it kept the pre-sleep age
                // until the next read, so this host's row takes the board time of the write it just made
                if let Some(s) = self.snapshot.as_mut() {
                    let observed = s.observed;
                    if let Some(row) = s.hosts.iter_mut().find(|r| r.name == me && r.updated < now) {
                        row.updated = now;
                        row.diagnostics = crate::service_versions::read(&body, now, observed, ttl as i64, false);
                    }
                }
            }
            None => {
                self.host_issue = Some(self.board.create(&title, &body, &[HOST_LABEL.to_string()])?);
            }
        }
        // § 4 renamed host: another open heartbeat of this service is a retired name (#1483)
        for h in hosts.iter().filter(|h| h.title != title && !self.instance.is_empty() && h.body.lines().any(|l| l.strip_prefix("service ") == Some(&self.instance))) {
            self.board.comment(h.number, &format!("Retired: this service now runs as host {me}."))?;
            self.board.close(h.number)?;
        }

        let open: HashSet<u64> = views.iter().map(|v| v.issue.number).collect();
        crate::profile::repair_ended(&self.cfg.install, &self.cfg.root, &self.cfg.repo, &me, self.board, &open, &mut self.unowned);
        self.tidy(&by_n);

        // 5. projected labels
        let written = touched.clone();
        for v in &views {
            let n = v.issue.number;
            if touched.contains(&n) {
                continue;
            }
            // § 2 timed park: at its board time the service requeues it; one op id per park, so hosts write it once
            if v.projection.task.state == State::Later && let Some(until) = v.projection.task.until.as_deref().filter(|u| crate::board::iso(u).is_some_and(|at| at <= now)) {
                let rev = v.projection.revision;
                let op = crate::dispatch::Op { id: Some(format!("wake-{rev}")), revision: Some(rev) };
                let r = crate::dispatch::act(self.board, &self.cfg.root, n, "requeue", &[], &format!("Reason: Timed park ended at {until}."), &op);
                if !r.ok() { log!("#{n} timed park wake: {}", r.reason); }
                touched.insert(n);
                continue;
            }
            // § 1: a person who adds `taskq` to a held issue starts it; the next pass folds `ready`
            if is_task(&v.issue) && v.projection.task.state == State::Held {
                self.board.comment(n, "taskq: start")?;
                touched.insert(n);
                continue;
            }
            let want = self.board.state_label(v.projection.task.state.name())?;
            let mut stale: Vec<_> = v.issue.labels.iter().filter(|l| l.starts_with("q-") && **l != want).cloned().collect();
            stale.sort();
            let fix = !stale.is_empty() || !v.issue.labels.contains(&want);
            if !fix || v.projection.task.state == State::Closed {
                project(self.board, &v.issue, &v.projection.task)?;
            } else {
                // Adapter success includes assignment readback; one hand-moved label is expected. The same fix again on
                // the next pass means another writer folds differently (#1550: N-1 host): back off, log and report once.
                // ponytail: retry every 60 passes so a fixed peer is caught up; a peer-written-label signal if that lags
                let (count, mut fixed_at, mut reported) = previous_fixes.get(&n).filter(|(w, old, ..)| w == &want && old == &stale)
                    .map_or((1, 0, false), |(_, _, count, at, reported)| (count.saturating_add(1), *at, *reported));
                // #1873: a revert is a later write without the wanted label. A list read from before the fix (reused or
                // lagging) or a peer still moving the same way (its comment lands seconds before its labels) is no loop.
                let reverted = count > 1 && v.issue.updated > fixed_at && !v.issue.labels.contains(&want);
                if count % 60 == 1 {
                    project(self.board, &v.issue, &v.projection.task)?;
                    fixed_at = self.board.now()?;
                } else if reverted && !reported {
                    log!("#{n} label {want} reverted by another writer after this host's fix; backing off (§ 2)");
                    crate::incident::detect("invariant.label_loop", concat!("src/service.rs:", line!()), "none", &[v.projection.task.state.name()], &["passes:2"]);
                    reported = true;
                }
                self.label_fixes.insert(n, (want, stale, count, fixed_at, reported));
            }
            if v.projection.task.state == State::Closed {
                // § 2 Deploy: a close typed on the board gets its waiting label like a written one
                if let Err(e) = crate::deploy::label(self.board, &v.issue, &v.log, crate::deploy::config(&self.cfg.root).as_ref()) { log!("#{n} {} label not written: {e}", crate::deploy::LABEL); }
                touched.insert(n);
            }
        }
        // § 2 Summary: rewrite a stale one from the log like a label
        let previous = std::mem::take(&mut self.summary_fixes);
        let deploy = crate::deploy::config(&self.cfg.root);
        // the label step marks a closed task touched without writing to it
        let skip = |v: &&View| touched.contains(&v.issue.number) && (v.projection.task.state != State::Closed || written.contains(&v.issue.number));
        for v in views.iter().filter(|v| self.board.summaries() && !skip(v)) {
            let n = v.issue.number;
            let have = crate::summary::find(&v.comments);
            // § 2: a missing one is created at a state change (an older task's next write) or when this service saw it
            let moved = self.seen.get(&n).is_some_and(|s| *s != v.projection.task.state);
            if have.is_none() && !moved && !self.summarized.contains(&n) { continue; }
            self.summarized.insert(n);
            let want = crate::summary::text(self.board, n, &v.issue.body, &v.log, &v.projection.task, deploy.as_ref());
            if have.is_some_and(|c| crate::summary::same(&c.body, &want)) { continue; }
            // a missing one is always written; a peer that renders another text is edited back once per 60 passes
            let streak = if have.is_none() { 1 } else { previous.get(&n).map_or(1, |s| s.saturating_add(1)) };
            if streak % 60 == 1 && let Err(e) = crate::summary::put(self.board, n, &v.comments, &want) {
                log!("#{n} summary not written: {e}");
            }
            self.summary_fixes.insert(n, streak);
        }

        for v in &views {
            let n = v.issue.number;
            if v.projection.task.publication.is_some() { continue; }
            let changes = core::changes(n, &v.log);
            let Some(at) = changes.iter().rposition(|(_, before, after)| before.publication.is_some() && after.publication.is_none() && matches!(after.state, State::Ready | State::Later | State::Failed)) else { continue };
            let (id, before, _) = &changes[at];
            // #1535: one cancellation per cancelling entry; a later result supersedes it, so its publication is never dequeued
            if self.cancelled_publications.get(&n) == Some(id) || changes[at + 1..].iter().any(|(_, _, after)| after.result.is_some()) { continue; }
            if let Err(e) = self.board.cancel_publication(n, before.result.as_deref().unwrap_or_default()) {
                log!("#{n} publication cancellation unknown: {e}");
                touched.insert(n);
                continue;
            }
            self.cancelled_publications.insert(n, *id);
        }

        // § 7 publication intent is board state; every host can reconcile the queue after a restart, even with focus off.
        // The attempt host polls while its heartbeat is fresh; others take over only when it is stale or gone (#1359).
        let live = |h: &str| h != me && hosts.iter().any(|i| i.title == format!("taskq host {h}") && now - i.updated <= ttl);
        let mut waiting = BTreeMap::new();
        for v in views.iter().filter(|v| v.projection.task.publication.is_some() && v.projection.task.state == State::Review && !touched.contains(&v.issue.number)) {
            let n = v.issue.number;
            // the publish verdict ends the attempt, so its last execution names the attempt host
            if v.projection.executions.last().is_some_and(|x| live(&x.host)) { continue; }
            // written since `views` read it: the next pass resumes from a fresh read
            if !self.cache.contains_key(&n) { continue; }
            let began = std::time::Instant::now();
            let r = crate::dispatch::resume_publication(self.board, &self.cfg, &v.issue, &v.projection.task, &v.log);
            if r.ok() { self.publication_waits.remove(&n); continue; }
            let published = v.comments.iter().rposition(|c| c.trusted && c.body.lines().next().and_then(|h| h.trim().strip_prefix("taskq:"))
                .and_then(|h| h.split_whitespace().next()) == Some("publish"));
            // #1820 A2 (pilotq #106): a wait the pass cannot end is shown with its reason and age, never only logged
            // #1942: logged when the reason changes, not every pass
            if self.publication_waits.get(&n) != Some(&r.reason) {
                log!("#{n} publication: {}", r.reason);
                let (a, rt) = v.projection.executions.last().map_or((0, "other"), |x| (x.attempt, x.runtime.as_str()));
                crate::profile::stage("checks", n, a, rt, Some(began.elapsed()), "retried", crate::profile::class(&r.reason), crate::profile::Usage::NoModel);
                // #1935: a wait only a person ends gets one plain board comment per PR head (the `publish` entry before it),
                // never a log entry; the board keeps it across restarts (R1)
                if r.reason.contains(crate::board::MANUAL_MERGE) && !v.comments[published.map_or(0, |i| i + 1)..].iter().any(|c| c.trusted && c.body.starts_with(MANUAL_MERGE_NOTE)) {
                    if let Err(e) = self.board.comment(n, &format!("{MANUAL_MERGE_NOTE}: {}", r.reason)) { log!("#{n} manual-merge note unknown: {e}"); }
                }
                self.publication_waits.insert(n, r.reason.clone());
            }
            let since = published.map(|i| v.comments[i].created);
            waiting.insert(n, crate::snapshot::PublicationWait { reason: r.reason, since, age_s: since.map(|t| now - t) });
        }
        self.publication_waits.retain(|n, _| views.iter().any(|v| v.issue.number == *n && v.projection.task.publication.is_some()));
        if let Some(snapshot) = &mut self.snapshot {
            for r in &mut snapshot.tasks { r.publication = waiting.remove(&r.n); }
        }
        touched.extend(views.iter().filter(|v| v.projection.task.publication.is_some()).map(|v| v.issue.number));

        // 6. route to own attempts, then admit within capacity; the gate holds launches, the lease stays (§ 3)
        let mut pending = BTreeSet::new();
        // a reservation lives within one pass: it becomes a child or a held claim, or is released here
        self.reserved.retain(|r| self.held.iter().any(|l| l.task == r.0 && l.attempt == r.1) || unresolved_local.contains(r));
        (self.requested, self.refused) = (vec![], None);
        if self.host.is_some() { self.sync_host(); }
        touched.extend(self.release_held());
        for v in &views {
            let (n, t) = (v.issue.number, &v.projection.task);
            let (Some(a), Some(h)) = (t.attempt, host_of(t)) else { continue };
            if h != me || !matches!(t.state, State::Doing | State::Review) || touched.contains(&n) || self.kids.iter().any(|k| k.task == n) {
                continue;
            }
            if let Some(i) = self.held.iter().position(|l| l.attempt == a) {
                let rt = self.held[i].rt.clone();
                if !self.gui_refused(&v.issue, &rt) && self.reserve(n, a, &rt) {
                    let l = self.held.remove(i);
                    let log = self.seen_log(v)?;
                    self.launch(l, a, log)?;
                } else {
                    pending.insert((n, a));
                }
                continue;
            }
            // #1879 (pilotq#133): a spawn whose start was never reached waits one ttl under its own reason, then ends gone
            if let Some(x) = v.projection.executions.iter().rev().find(|x| x.attempt == a)
                .filter(|x| self.lost_spawn(n, x) && v.projection.executions.iter().filter(|x| x.release.is_none()).count() == 1) {
                let age = now - v.comments.iter().find(|c| c.id == x.id).map_or(now, |c| c.created);
                if age < ttl {
                    self.execution_waits.insert(n, format!("spawn {a} not started on {me}: input not accepted; lost spawn released no-start {ttl} s after the spawn"));
                    pending.insert((n, a));
                } else {
                    self.log.push(format!("#{n} lost spawn {a}: no attempt log on {me} {age} s after the spawn; released no-start"));
                    release_execution(self.board, n, a, a, "no-start")?;
                    self.retired(n, a, a, false);
                    self.execution_waits.remove(&n);
                    self.gone(n, t, a, &[], &format!("start failed: lost spawn {a}: no native start on {me} {age} s after the spawn"))?;
                    touched.insert(n);
                }
                continue;
            }
            if self.execution_waits.contains_key(&n) { pending.insert((n, a)); continue; }
            let seen = v.projection.delivered(a).unwrap_or(0);
            let new: Vec<&Comment> = v.comments.iter().filter(|c| t.inbox.contains(&c.id) && c.id > seen).collect();
            let Some(last) = new.last().map(|c| c.id) else {
                let last = v.projection.executions.iter().rev().find(|x| x.attempt == a);
                if let Some(x) = last.filter(|x| x.release.is_some()) {
                    let text = format!("execution {} confirmed {:?} without task outcome{}", x.id, x.release.unwrap(), self.last_message(n, a, t));
                    self.exit_outcome(n, t, a, x.usage_hold, now, &text)?;
                    touched.insert(n);
                } else {
                    self.execution_waits.insert(n,"native execution evidence missing; reconcile the owning runtime".into());
                }
                continue;
            };
            let quota = new.iter().any(|c| c.body.lines().next() == Some(core::entry("steer", &["quota"], Some(a), "").as_str()));
            if quota && v.projection.executions.iter().rev().find(|x| x.attempt == a).is_some_and(|x| x.release == Some(core::Release::NoStart)) {
                self.execution_waits.insert(n,"quota continuation unavailable; recover the original native session without replay or migration".into());
                pending.insert((n,a));
                continue;
            }
            // #1946: a never-started attempt has no session to resume; the reply to its start refusal starts afresh
            let runs: Vec<_> = v.projection.executions.iter().filter(|x| x.attempt == a).collect();
            let refused:Vec<_>=runs.iter().filter(|x|x.through==last && x.release==Some(core::Release::NoStart)).collect();
            if refused.len()>=core::GONE_MAX as usize {
                let x=refused.last().unwrap();
                let reason=v.log.iter().rev().find(|e|e.body.lines().next().is_some_and(|h|h==format!("taskq: released {} no-start attempt {a}",x.id)))
                    .map(|e|core::entry_text(n,&e.body)).unwrap_or("native launch refused");
                let card=input_card(n,reason,refused.len());
                self.post(n,core::with_fields(core::entry("ask",&[],Some(a),&card),Some(&format!("input-retry-{}",x.id)),None))?;
                touched.insert(n);continue;
            }
            if !runs.is_empty() && runs.iter().all(|x| !x.accepted && x.release == Some(core::Release::NoStart)) {
                let text = "Reason: start refused before launch; the reply starts a fresh attempt";
                // a reviewer gone keeps the result (§ 2); a worker requeue ends the attempt
                if t.state == State::Review { self.gone(n, t, a, &[], text)?; } else { self.post(n, core::entry("requeue", &[], Some(a), text))?; }
                touched.insert(n);
                continue;
            }
            let (rt, session) = rt_of(t).unwrap_or(("", ""));
            let mut session = session.to_string();
            let mut l = Launch { files: String::new(), execution: None, task: n, title: v.issue.title.clone(), attempt: a, role: t.role.clone().unwrap_or_default(), rt: rt.into(), session: session.clone(), url: self.board.url(n), resume: None };
            if session == "-" || crate::runtime::of(rt).remote {
                session = crate::runtime::native_session(rt, &crate::runtime::log_path(&self.cfg.root, &l)).unwrap_or_else(|| if session == "-" { String::new() } else { session });
            }
            let text: Vec<String> = new.iter().map(|c| core::delivery(n, &v.log, c.id)).collect();
            let inbox: Vec<(&str, &str)> = new.iter().zip(&text).map(|(c, t)| (core::kind(n, &c.body), t.as_str())).collect();
            l.session = session;
            l.resume = Some(format!("{}\n{}", text.join("\n\n"), crate::runtime::turn_marker(&inbox)));
            if l.session.is_empty() {
                self.execution_waits.insert(n,"session identity unknown; recover the original native session".into());
                pending.insert((n,a));
                continue;
            }
            let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
            let account = crate::runtime::accounts::of_log(&crate::runtime::log_path(&self.cfg.root,&l));
            if accounts.error.is_some() || accounts.state.get(&l.rt).and_then(|m|m.get(&account)).is_some_and(|q|q.end() > now) || self.refused_access.contains_key(&l.rt) || self.limited.contains_key(&l.rt) || self.logged_out.contains(&l.rt) || self.gui_refused(&v.issue, &l.rt) || !self.reserve(n, a, &l.rt) {
                pending.insert((n, a)); // the inbox stays undelivered
                continue;
            }
            let log = self.seen_log(v)?;
            self.launch(l, last, log)?;
        }
        pending.extend(self.held.iter().map(|l| (l.task, l.attempt)));
        if pending != self.pending {
            self.pending = pending;
            self.beat()?;
        }
        // #1616: every ready task this pass reaches gets one admission code, written into the snapshot rows
        let mut decided: BTreeMap<u64, (&'static str, String)> = BTreeMap::new();
        let focus_off = self.focus.is_empty();
        if focus_off { tracing::info!(coreq_refusal = "focus"); }
        let user = if focus_off { String::new() } else { self.board.me().unwrap_or_default() };
        let mut queue: Vec<&View> = vec![];
        for v in &views {
            let (n, t) = (v.issue.number, &v.projection.task);
            if touched.contains(&n) || self.kids.iter().any(|k| k.task == n) { continue; }
            if !(t.state == State::Ready || (t.state == State::Review && t.attempt.is_none() && t.publication.is_none())) { continue; }
            // `needs-gate`: every host of this version has gates; N-1 lacks them and leaves the task alone (§ 8)
            let lacks = || v.issue.labels.iter().filter_map(|l| l.strip_prefix("needs-"))
                .find(|x| !(*x == "gate" || if *x == "gui" { self.gui().is_ok() } else { self.cfg.has.iter().any(|h| h == x) }));
            let refusal = if self.execution_waits.contains_key(&n) { Some(("unknown", crate::snapshot::ADMISSION_UNKNOWN[3].to_string())) }
                else if let Some(r) = self.launch_refusal(n) { Some(r) }
                else if let Err(e) = crate::labels::validate(&v.issue.labels) { log!("#{n}: {e}; not admitted"); Some(("labels", e.to_string())) }
                else if let Some(reason)=self.host_claim(v,&hosts,now).err() {Some(("host",reason))}
                else if let Some(x) = lacks() { Some(("capability", format!("needs-{x}"))) }
                else if focus_off { Some(("focus", self.focus_error.clone().unwrap_or_else(|| FOCUS_OFF.into()))) }
                else if priority(&v.issue) != 0 && !self.focus.iter().any(|f| f == "all" || crate::labels::matches(&v.issue.labels, f)) {
                    Some(("focus", format!("focus {}", self.focus.join(","))))
                }
                // last: the github.com list carries no assignees (§ 4 budget), so only a candidate reads them
                else if v.issue.labels.iter().any(|l| l == "assignee-only") && !assigned(self.board, &v.issue, &user).unwrap_or(false) {
                    Some(("host", "assignee-only: not assigned to this board user".into()))
                }
                else { None };
            match refusal { Some(r) => { decided.insert(n, r); } None => queue.push(v) }
        }
        if focus_off {
            if let Some(host) = &self.host { if let Ok(mut h) = host.lock() { h.ready(&self.cfg.root, false); } }
            self.decided(decided);
            return Ok(());
        }
        // reviews before ready tasks, then priority and number within each (§ 4 step 6)
        queue.sort_by_key(|v| (v.projection.task.state != State::Review, priority(&v.issue), v.issue.number));
        let ready = queue.iter().any(|v| self.select_runtime(v).is_ok());
        if let Some(host) = &self.host {
            if let Ok(mut h) = host.lock() { h.ready(&self.cfg.root, ready); }
        }
        // a refused reservation ends admission for this pass; the rest of the queue reads the same refusal
        let mut refused: Option<(&'static str, String)> = None;
        for v in queue {
            let n = v.issue.number;
            if let Some(r) = refused.clone().or_else(|| self.launch_refusal(n)) {
                decided.insert(n, r);
                continue;
            }
            let role = if v.projection.task.state == State::Review { "reviewer" } else { "worker" };
            if role == "worker" && label(&v.issue, "run-").is_none() && v.issue.labels.iter().filter(|l| l.starts_with("prefer-")).count() > 1 {
                self.log.push(format!("#{n}: conflicting prefer-* labels; preference ignored"));
            }
            let (rt, fallback) = match self.select_runtime(v) { Ok(rt) => rt, Err(r) => { decided.insert(n, r); continue; } };
            let host_reason=match self.host_claim(v,&hosts,now) {Ok(reason)=>reason,Err(reason)=>{decided.insert(n,("host",reason));continue;}};
            let reasons:Vec<_>=host_reason.into_iter().chain(fallback).collect();
            let fallback=(!reasons.is_empty()).then(||format!("{}; selected {rt}",reasons.join("; ")));
            if !self.reserve(n, 0, &rt) {
                let r = self.launch_refusal(n).unwrap_or_else(|| self.refused.as_ref()
                    .map_or(("unknown", crate::snapshot::ADMISSION_UNKNOWN[2].into()), |(c, t)| refusal_admission(c, t)));
                decided.insert(n, r.clone());
                refused = Some(r);
                continue;
            }
            self.claim(v, role, &rt, fallback.as_deref().unwrap_or_default())?;
            let reason = fallback.map_or_else(|| format!("claimed on {rt}"), |why| { self.log.push(format!("#{n} {why}")); why });
            decided.insert(n, ("admitted", reason));
        }
        self.decided(decided);
        Ok(())
    }

    /// Moves to closed, `failed`, `ask` or `waiting` since the previous pass go to `<root>/.taskq/events` (§ 4); a task
    /// new since then counts from its first state. A task gone from the open list counts only when a fresh read shows it
    /// closed (R12).
    fn events(&mut self, views: &[View], now: i64) -> Result<(), String> {
        // the attempt current at the move; a `close` or `failed` that ended it leaves the last spawn
        let attempt = |t: &Task, c: &[Comment]| t.attempt.or_else(|| c.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: spawn ")).map(|c| c.id));
        let now_seen: HashMap<u64, State> = views.iter().map(|v| (v.issue.number, v.projection.task.state)).collect();
        let first = self.seen.is_empty() && self.events_since.is_none();
        let mut ns: Vec<u64> = self.seen.keys().copied().collect();
        if !first {
            ns.extend(views.iter().map(|v| v.issue.number).filter(|n| !self.seen.contains_key(n)));
        }
        let (mut moves, mut closed) = (vec![], vec![]);
        for n in ns {
            let (to, a) = match views.iter().find(|v| v.issue.number == n) {
                // #1712: `ask` is its card's entry (§ 2 revision), so a second ask of one attempt is its own event
                Some(v) if v.projection.task.state == State::Ask => (State::Ask, Some(v.projection.revision).filter(|r| *r > 0).or(Some(n))),
                // `waiting` without an attempt: the last comment, else the issue number
                Some(v) if v.projection.task.state == State::Waiting => (State::Waiting, attempt(&v.projection.task, &v.comments).or(v.comments.last().map(|c| c.id)).or(Some(n))),
                Some(v) => (v.projection.task.state, attempt(&v.projection.task, &v.comments)),
                None => match self.board.issue(n) {
                    Ok(i) if !i.open => {
                        let c = self.board.comments(n)?;
                        let a = attempt(&Task::default(), &c);
                        closed.push((n, i.created, c));
                        (State::Closed, a)
                    }
                    _ => continue,
                },
            };
            let kind = match to {
                State::Closed => "close",
                State::Failed => "failed",
                State::Ask => "ask",
                State::Waiting => "blocked",
                _ => continue,
            };
            if let Some(a) = a.filter(|_| self.seen.get(&n) != Some(&to)) {
                moves.push((n, kind, a, String::new()));
            }
        }
        // `gone`: each trusted `gone attempt` entry newer than the previous pass; the first pass adds none
        let newest = views.iter().flat_map(|v| v.comments.iter().map(|c| c.id)).max();
        if let Some(after) = self.gone_after {
            for v in views {
                for c in v.comments.iter().filter(|c| c.trusted && c.id > after) {
                    if let Some(a) = c.body.lines().next().and_then(|l| l.strip_prefix("taskq: gone attempt ")).and_then(|a| a.parse().ok()) {
                        moves.push((v.issue.number, "gone", a, String::new()));
                    }
                }
            }
        }
        // queue transitions (§ 4 Profiling): accepted entries newer than the previous pass that changed the fold
        if let Some(after) = self.moved_after {
            let closed: Vec<_> = closed.iter().map(|(n,created,c)| {
                let log: Vec<_> = c.iter().filter(|c| c.trusted).map(|c| core::Entry { id:c.id, body:c.body.clone(), edited:c.edited }).collect();
                (*n, *created, c, core::project(*n, &log, false))
            }).collect();
            let histories = views.iter().map(|v| (v.issue.number, v.issue.created, &v.comments, &v.projection))
                .chain(closed.iter().map(|(n,created,c,p)| (*n,*created,*c,p)));
            // review with a deferred publication is the `merge-queue` state: its wait is not reviewer work (#1344)
            let name = |t: &Task| if t.state == State::Review && t.publication.is_some() { "merge-queue" } else { t.state.name() };
            for (n, created, comments, projection) in histories.filter(|(_,_,c,_)| c.iter().any(|c| c.trusted && c.id > after)) {
                let at = |id: u64| comments.iter().find(|c| c.id == id).map(|c| c.created);
                // the issue creation is the entry into the first state, ready; a board without it leaves it unknown
                let mut since = Some(created).filter(|c| *c > 0);
                let mut moved: Vec<_> = projection.changes.iter().chain(&projection.queue).collect();
                moved.sort_by_key(|m| m.0);
                for (id, from, to) in moved {
                    let id = *id;
                    let entered = std::mem::replace(&mut since, at(id));
                    // `held` is outside the queue model (§ Executable behavioral model): `hold` and `start` add none
                    if id <= after || from.state == State::Held || to.state == State::Held { continue; }
                    let side = if to.session.is_some() { to } else { from };
                    let host = host_of(side);
                    if host.is_none_or(|h| h == self.cfg.me) {
                        let prev = entered.zip(at(id)).and_then(|(a, b)| u64::try_from(b - a).ok());
                        crate::profile::transition(n, id, name(from), name(to), prev, rt_of(side).map(|r| r.0), host);
                    }
                }
            }
        }
        // § 7: a task closed without merging its PR leaves none open; `requeue`, `later` and `failed` keep it.
        // ponytail: a close while no service ran is not seen; a sweep of open `taskq-*` PRs is the upgrade path
        for (n, kind, _, outcome) in &mut moves {
            let n = *n;
            if *kind != "close" { continue; }
            let Some(comments) = views.iter().find(|v| v.issue.number == n).map(|v| &v.comments).or_else(|| closed.iter().find(|x| x.0 == n).map(|x| &x.2)) else { continue };
            let entries: Vec<core::Entry> = comments.iter().filter(|c| c.trusted).map(|c| core::Entry { id: c.id, body: c.body.clone(), edited: c.edited }).collect();
            let verdict = comments.iter().rev().find(|c| c.trusted && c.body.starts_with("taskq: close")).map(|c| core::verdict(n, &entries, c.body.split_once('\n').map_or("", |x| x.1)));
            let verdict = verdict.as_deref();
            // § 7 outcome: a replaced task names its successor's state, read from this pass or the board
            let next = verdict.and_then(crate::display::successor).map(|m| views.iter().find(|v| v.issue.number == m).map(|v| v.projection.task.state.name())
                .or_else(|| crate::dispatch::current(self.board, m).ok().map(|(i, t, _)| if i.open { t.state.name() } else { "closed" })).unwrap_or("unknown"));
            *outcome = crate::display::close_outcome(verdict, next);
            let reason = verdict.and_then(|v| v.lines().find(|l| !l.trim().is_empty())).map_or("issue closed on the board".into(), |l| l.chars().take(160).collect::<String>());
            match self.board.close_task_prs(n, &reason) {
                Ok(prs) => self.log.extend(prs.iter().map(|p| format!("#{n} closed unmerged PR #{p}"))),
                Err(e) => self.log.push(format!("#{n} task PR left open, state unknown: {e}")),
            }
        }
        self.gone_after = self.gone_after.max(newest);
        self.moved_after = Some(self.gone_after.unwrap_or(0));
        moves.sort();
        // `ask` stage (§ 4 Profiling): from the move into `ask` of this host's attempt to the move out
        for v in views.iter().filter(|v| v.projection.task.state == State::Ask && self.seen.get(&v.issue.number).is_some_and(|s| *s != State::Ask)) {
            if let (Some(a), Some((rt, _)), true) = (v.projection.task.attempt, rt_of(&v.projection.task), host_of(&v.projection.task) == Some(self.cfg.me.as_str())) {
                self.asked.insert(v.issue.number, (std::time::Instant::now(), a, rt.to_string()));
            }
        }
        let left: Vec<u64> = self.asked.keys().filter(|n| now_seen.get(n) != Some(&State::Ask)).copied().collect();
        for n in left {
            let (began, a, rt) = self.asked.remove(&n).unwrap();
            let outcome = if matches!(now_seen.get(&n), Some(State::Doing | State::Review | State::Ready)) { "ok" } else { "cancelled" };
            crate::profile::stage("ask", n, a, &rt, Some(began.elapsed()), outcome, "none", crate::profile::Usage::NoModel);
        }
        self.seen = now_seen;
        if first || !moves.is_empty() || self.events_since.is_none_or(|s| s < now - MONTH) {
            let path = crate::dot(&self.cfg.root).join("events");
            match record(&path, now, &moves) {
                Ok(s) => self.events_since = Some(s),
                Err(e) => self.log.push(format!("events: {e}")),
            }
        }
        for (n, kind, a, outcome) in moves {
            self.log.push(format!("#{n} event {kind} {a} {outcome}").trim_end().into());
        }
        Ok(())
    }

    /// § 2 visual evidence: the app has no board token, so the service copies each open ask's images once into
    /// `.taskq/images`; the tray shows them inline. A failed read leaves an empty file: the app keeps the link, and the
    /// next service start retries. Copies of closed asks are removed.
    fn copy_images(&self) {
        let Some(s) = &self.snapshot else { return };
        let urls: Vec<String> = s.tasks.iter().filter_map(|r| r.task.card.as_deref()).flat_map(crate::snapshot::card_images).collect();
        let keep: Vec<PathBuf> = urls.iter().map(|u| crate::snapshot::image_copy(&self.cfg.root, u)).collect();
        for (url, path) in urls.iter().zip(&keep) {
            if path.exists() { continue; }
            let bytes = match self.board.file(url) {
                Ok(b) if crate::board::image_kind(&b).is_some() => b,
                Ok(_) => { eprintln!("ask image {url}: not an image"); vec![] }
                Err(e) => { eprintln!("ask image {e}"); vec![] }
            };
            if let Err(e) = std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(path, bytes)) { eprintln!("ask image copy {}: {e}", path.display()); }
        }
        for old in std::fs::read_dir(crate::dot(&self.cfg.root).join("images")).into_iter().flatten().flatten() {
            if !keep.contains(&old.path()) { let _ = std::fs::remove_file(old.path()); }
        }
    }
    /// R6 stuck: one `#N event stuck <state> <age>` line per task, again only after it moves; observe only.
    fn log_stuck(&mut self, now: i64) {
        let Some(snapshot) = &self.snapshot else { return };
        let stuck: HashMap<u64, (i64, &str, i64)> = snapshot.tasks.iter().filter_map(|r| r.stuck.map(|a| (r.n, (now - a, r.task.state.name(), a)))).collect();
        for (n, (last, state, a)) in &stuck {
            if self.stuck.insert(*n, *last) != Some(*last) {
                self.log.push(format!("#{n} event stuck {state} {}", crate::age(*a)));
            }
        }
        self.stuck.retain(|n, _| stuck.contains_key(n));
    }
    /// § 4 Churn: one `#N event churn ...` line and one `service.churn` telemetry record per flagged task until it clears.
    fn log_churn(&mut self) {
        let Some(snapshot) = &self.snapshot else { return };
        let flagged: Vec<(u64, crate::snapshot::Churn)> = snapshot.tasks.iter().filter_map(|r| r.churn.map(|c| (r.n, c))).collect();
        for (n, c) in &flagged {
            if self.churned.insert(*n) {
                self.log.push(format!("#{n} event churn notes {} spawns {} accepted {}", c.notes, c.spawns, c.accepted));
                crate::profile::measure_counted("service.churn", || ((), [("task", *n), ("notes", c.notes), ("spawns", c.spawns), ("accepted", c.accepted)]
                    .into_iter().map(|(k, v)| (k.to_string(), v)).collect()));
            }
        }
        self.churned.retain(|n| flagged.iter().any(|(m, _)| m == n));
    }

    /// docs/design/worktrees.md: one tidy on the first pass after start, then at most hourly. A child this service
    /// runs keeps its worktree; an open task's state comes from this pass's read.
    fn tidy(&mut self, open: &HashMap<u64, &View>) {
        if self.tidied.is_some_and(|t| t.elapsed().as_secs() < crate::tidy::EVERY) { return; }
        self.tidied = Some(std::time::Instant::now());
        let kids: Vec<u64> = self.kids.iter().map(|k| k.task).collect();
        let states: HashMap<u64, State> = open.iter().map(|(n, v)| (*n, v.projection.task.state)).collect();
        let t = crate::tidy::Tidy { board: self.board, root: &self.cfg.root, me: &self.cfg.me, boot: self.boot, external: self.cfg.external, force: false, dry: false };
        let o = crate::profile::measure_counted("service.tidy", || {
            let o = t.run(&mut |n| kids.contains(&n), &|n| states.get(&n).copied());
            let counts = o.counts.clone();
            (o, counts)
        });
        let skipped = o.report["outcome"].as_str().is_some_and(|s| s.starts_with("skipped"));
        // #1549: a refusal or stale root logs once per changed line, across restarts; `.taskq/cleanup` is a cache (R1)
        let path = crate::dot(&self.cfg.root).join("cleanup");
        let last: BTreeSet<String> = std::fs::read_to_string(&path).unwrap_or_default().lines().map(String::from).collect();
        let sticky = |l: &String| [" kept: ", " unconfirmed: ", "root stale: ", "tidy skipped: "].iter().any(|k| l.contains(k));
        let mut next: BTreeSet<String> = if skipped { last.clone() } else { BTreeSet::new() };
        for line in o.log {
            if sticky(&line) {
                next.insert(line.clone());
                if last.contains(&line) { continue; }
            }
            self.log.push(line);
        }
        if next != last && let Err(e) = std::fs::create_dir_all(crate::dot(&self.cfg.root)).and_then(|_| std::fs::write(&path, next.into_iter().map(|l| l + "\n").collect::<String>())) {
            self.log.push(format!("cleanup cache: {e}"));
        }
        if !skipped {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            tidy_findings(self.board, &self.cfg, &o.conditions, now, &mut self.log);
        }
    }

    fn host_claim(&self,v:&View,hosts:&[Issue],now:i64) -> Result<Option<String>,String> {
        use crate::host::ClaimMode;
        if self.cfg.claim==ClaimMode::None {return Err("manager-only: task execution disabled".into());}
        if let Some(host)=label(&v.issue,"host-") {return if host==self.cfg.me {Ok(None)} else {Err(format!("labelled host-{host}"))};}
        match self.cfg.claim {
            ClaimMode::Any=>return Ok(None),
            ClaimMode::Labelled=>return Err("labelled claim: no host-* label".into()),
            _=>{},
        }
        let since=v.projection.changes.last().and_then(|(id,_,_)|v.comments.iter().find(|c|c.id==*id)).map_or(v.issue.created,|c|c.created);
        if now>=since && now-since>=self.cfg.spillover_grace as i64 {return Ok(Some(format!("spillover: grace {} s elapsed",self.cfg.spillover_grace)));}
        let mut reasons=vec![];
        let mut exhausted=vec![];
        let mut known=false;
        for host in hosts.iter().filter(|h|host_name(h).is_some_and(|name|name!=self.cfg.me)) {
            let Some(balance)=Balancing::read(&host.body) else {reasons.push("primary availability unknown: incompatible heartbeat".into());continue;};
            if balance.mode!="any" && balance.mode!="primary" {continue;}
            known=true;
            let name=host_name(host).unwrap();
            if !crate::snapshot::live(now-host.updated,self.cfg.ttl) {reasons.push(format!("primary {name} availability unknown: stale heartbeat"));continue;}
            match balance.available(v) {
                Ok((rt,_))=>return Err(format!("spillover waits: primary {name} has effective {rt} capacity; grace {} s",self.cfg.spillover_grace)),
                Err(("unknown",reason))=>reasons.push(format!("primary {name}: {reason}")),
                Err((_,reason))=>exhausted.push(format!("primary {name}: {reason}")),
            }
        }
        if !known && reasons.is_empty() {reasons.push("primary availability unknown: no primary heartbeat".into());}
        if reasons.is_empty() {Ok(Some(format!("spillover: zero effective primary availability; {}",exhausted.join("; "))))}
        else {Err(format!("spillover waits: {}; grace {} s",reasons.join("; "),self.cfg.spillover_grace))}
    }

    fn runtime_disabled(&self,rt:&str) -> bool {
        self.cfg.allowed_runtimes.as_ref().is_some_and(|a|!a.iter().any(|r|r==rt))
            || self.cap.as_ref().is_ok_and(|c|c.limits.get(rt)==Some(&0))
            || {let a=crate::runtime::accounts::Host::read(&self.cfg.install);a.error.is_none() && a.off(rt)}
    }

    fn balancing(&self) -> Balancing {
        let accounts=crate::runtime::accounts::Host::read(&self.cfg.install).heartbeat(self.board.now().unwrap_or(0));
        let runtimes=RUNTIMES.iter().map(|rt| {
            // Focus off needs no runtime probe; routine refusals otherwise materialize every idle poll.
            let outcome=if self.focus.is_empty() {
                Err((if self.focus_error.is_some() {"unknown"}else{"focus"},"task focus admits nothing".into()))
            } else {self.launch_refusal(0).map_or_else(||self.admit_runtime(rt,false),Err)};
            let (mut code,mut reason)=match outcome {Ok(_)=>("admitted",String::new()),Err(e)=>e};
            if code=="load-gate" && self.gate.state(self.mono).0=="unknown" {code="unknown";}
            if matches!(code,"access"|"unknown"|"hold") {reason=format!("{rt}: {code}; see host diagnostics");}
            ((*rt).into(),RuntimeAvailability {code:code.into(),reason:reason.chars().take(160).collect(),full:self.full.contains(*rt),disabled:self.runtime_disabled(rt),quota:accounts.iter().filter(|l|l.starts_with(&format!("quota {rt} "))).cloned().collect()})
        }).collect();
        let mut has=self.cfg.has.clone();has.retain(|h|h!="gui");if self.gui().is_ok() {has.push("gui".into());}
        Balancing {schema:1,mode:self.cfg.claim.name().into(),default_runtime:self.cfg.default_runtime.clone(),allowed_runtimes:self.cfg.allowed_runtimes.clone(),
            auto_reroute:self.cfg.auto_reroute,has,focus:self.focus_error.is_none().then(||self.focus.clone()),routing:self.cfg.routing.clone(),runtimes}
    }

    fn select_runtime(&self, v: &View) -> Result<(String, Option<String>), (&'static str, String)> {
        if v.projection.reroute.is_some() {return Err(("unknown",crate::snapshot::ROUTING_WAIT.into()));}
        let reviewer = v.projection.task.state == State::Review;
        let default = crate::labels::project_default(&v.issue.labels, reviewer, &self.cfg.routing, &self.cfg.default_runtime);
        crate::labels::select_project(&v.issue.labels, reviewer, default, self.cfg.auto_reroute,
            |rt|self.runtime_disabled(rt), |rt| self.admit_runtime(rt, needs_gui(&v.issue)))
    }

    /// `rt` when it has free capacity: `limits.<rt>` minus live children of it (§ 3), else the #1616 admission code
    /// and reason. `codex-app` only on a host whose `has` lists it (§ 9 opt-in).
    /// A `needs-gui` task gets only a runtime with effective Full access (§ 4 Filter): never a sandboxed one.
    fn admit_runtime(&self, rt: &str, gui: bool) -> Result<String, (&'static str, String)> {
        if self.cfg.allowed_runtimes.as_ref().is_some_and(|a|!a.iter().any(|r|r==rt)) {return Err(("capability",format!("{rt} excluded by project allowed_runtimes")));}
        if crate::runtime::of(rt).app && !(cfg!(any(target_os = "macos", windows)) && self.cfg.has.iter().any(|h| h == rt)) {
            return Err(("capability", format!("{rt} not in has")));
        }
        if let Some(r) = self.refused_access.get(rt) { return Err(("access", format!("{rt}: {r}"))); }
        let accounts = crate::runtime::accounts::Host::read(&self.cfg.install);
        if let Some(e) = &accounts.error { return Err(("unknown", format!("account availability unknown: {e}"))); }
        if accounts.off(rt) {return Err(("capability",format!("{rt}: every account disabled on this host")));}
        if accounts.active(rt, self.board.now().map_err(|e| ("unknown",e))?).is_none() {
            if let Some(hold) = accounts.hold(rt, self.board.now().map_err(|e| ("unknown",e))?) {
                return Err(("usage-limit",format!("{rt}: {} {}; account quotas {}", if hold.1 {"until"} else {"probe"},crate::board::stamp(hold.0),accounts.heartbeat(self.board.now().unwrap_or(0)).join("; "))));
            }
        }
        if self.limited.contains_key(rt) { return Err(("usage-limit", format!("{rt}: {}", self.limit_line(rt)))); }
        if self.logged_out.contains(rt) { return Err(("login", format!("{rt}: not logged in"))); }
        if gui && !self.full.contains(rt) { return Err(("capability", format!("needs-gui: {rt} has no effective Full grant"))); }
        let c = self.cap.as_ref().map_err(|_| ("unknown", crate::snapshot::ADMISSION_UNKNOWN[1].to_string()))?;
        // Auto: a runtime without a limit has no count ceiling (§ 9); Fixed: only listed runtimes
        let n = if c.auto { RUNTIMES.contains(&rt).then(|| c.limits.get(rt).copied().unwrap_or(u32::MAX)).ok_or(("capability", format!("{rt} unknown runtime")))? }
            else { *c.limits.get(rt).ok_or(("capacity", format!("{rt} not in limits")))? };
        if n == 0 { return Err(("capacity", format!("{rt} disabled (limit 0)"))); }
        let mut left = (n as usize).saturating_sub(self.entry().held.iter().filter(|r| r.2 == rt).count());
        if let Some(host) = &self.host {
            let h = host.lock().map_err(|_| ("unknown", crate::snapshot::ADMISSION_UNKNOWN[2].to_string()))?;
            if h.policy.is_err() || h.rebuilding {return Err(("unknown","host occupancy/policy recovery pending".into()));}
            if h.restart || h.draining.contains(&self.cfg.root) || h.held.is_some() {return Err(("hold","host draining, restart or hold".into()));}
            left = left.min(h.remaining(&self.cfg.root, rt, c));
        }
        if left > 0 { Ok(rt.to_string()) } else { Err(("capacity", format!("{rt} slots full"))) }
    }

    /// #1616: each ready row of this pass's snapshot carries this host's decision; a row it did not reach keeps
    /// `unknown`, reason `no host decision`. A review awaiting its reviewer carries it too (#1685).
    fn decided(&mut self, decided: BTreeMap<u64, (&'static str, String)>) {
        let me = self.cfg.me.clone();
        let Some(snapshot) = &mut self.snapshot else { return };
        let observed = snapshot.observed;
        for r in snapshot.tasks.iter_mut().filter(|r| matches!(r.task.state, State::Ready | State::Review)) {
            // Board-level refusals hold on every host, including an unfinished routing writer.
            if r.admission.as_ref().is_some_and(|a| a.host.is_none() && (a.code=="host" || a.reason==crate::snapshot::ROUTING_WAIT)) { continue; }
            if let Some((code, reason)) = decided.get(&r.n) {
                r.admission = Some(crate::snapshot::Admission { code, reason: reason.clone(), observed, host: Some(me.clone()) });
            }
        }
    }

    /// The view's log while nothing this pass wrote the task (every write drops its cache entry), else a fresh read:
    /// a claim or launch never rereads what `views` just read (#1359).
    fn seen_log(&self, v: &View) -> Result<Vec<core::Entry>, String> {
        if self.cache.contains_key(&v.issue.number) { Ok(v.log.clone()) } else { crate::board::entries(self.board, v.issue.number) }
    }

    /// Post `spawn`, re-read, start the child only if `fold` accepted this spawn (§ 3 claim).
    fn claim(&mut self, v: &View, role: &str, rt: &str, reason: &str) -> Result<(), String> {
        let (n, began) = (v.issue.number, std::time::Instant::now());
        let r = crate::profile::measure("dispatch", || self.claim_inner(v, role, rt, reason));
        let held = self.held.iter().find(|l| l.task == n).map(|l| l.attempt);
        let (a, outcome, err) = match &r {
            Ok(Some((l, _))) => (l.attempt, "ok", "none"),
            Ok(None) => held.map_or((0, "cancelled", "none"), |a| (a, "ok", "none")),
            Err(e) => (0, "failed", crate::profile::class(e)),
        };
        crate::profile::stage("claim", n, a, rt, Some(began.elapsed()), outcome, err, crate::profile::Usage::NoModel);
        match r? {
            Some((l, log)) => { let through = l.attempt; self.launch(l, through, log) },
            None => Ok(()),
        }
    }

    fn claim_inner(&mut self, v: &View, role: &str, rt: &str, reason: &str) -> Result<Option<(Launch, Vec<core::Entry>)>, String> {
        let n = v.issue.number;
        let before = self.seen_log(v)?;
        // #1903: a spawn the readback missed stays unknown; claiming again would post a spawn every pass
        if let Some(&id) = self.unseen.get(&n) {
            if !before.iter().any(|e| e.id == id) { return Ok(None); }
            self.unseen.remove(&n);
        }
        if core::executions(n, &before).iter().any(|x| x.release.is_none()) { return Ok(None); }
        let routing=core::project(n,&before,false);
        if routing.reroute.is_some() {return Ok(None);}
        // Read labels after the routing revision: a completed reroute invalidates a stale runtime selection.
        if routing.routing_revision!=0 {
            let fresh=self.board.issue(n)?;
            if fresh.labels.iter().filter(|l|!l.starts_with("q-")).collect::<BTreeSet<_>>() != v.issue.labels.iter().filter(|l|!l.starts_with("q-")).collect::<BTreeSet<_>>() || !fresh.open {self.cache.remove(&n);return Ok(None);}
        }
        let session = crate::runtime::new_session(rt);
        let reason=if routing.routing_revision==0 {reason.to_string()} else {format!("{reason}\nrouting-revision: {}",routing.routing_revision)};
        let id = self.post(n, core::entry("spawn", &[role, &format!("{rt}:{session}"), &self.cfg.me], None, &reason))?;
        let comments = self.board.comments(n)?;
        let seen = comments.iter().any(|c| c.id == id);
        if !seen {
            self.unseen.insert(n, id);
            crate::incident::detect("adapter.read_after_write", concat!("src/service.rs:", line!()), rt, &[], &["spawn:missing"]);
        }
        let log = crate::board::reply::log(n, &comments)?;
        let t = core::fold(n, &log, false); // only ready or review tasks are claimed
        if t.attempt != Some(id) {
            self.reserved.retain(|r| r.0 != n); // released
            self.sync_host();
            // a readback without the spawn leaves its claim unknown; a won claim ends as a lost spawn (#1879)
            self.log.push(if seen { format!("#{n} spawn {id} lost the claim") } else { format!("#{n} spawn {id} claim unknown: readback lacks the spawn") });
            return Ok(None);
        }
        for r in self.reserved.iter_mut().filter(|r| r.0 == n && r.1 == 0) { r.1 = id; }
        if let Some(host) = &self.host { if let Ok(mut h) = host.lock() { h.claimed(&self.cfg.root, n, id); } }
        let l = Launch { files: String::new(), execution: None, task: n, title: v.issue.title.clone(), attempt: id, role: role.into(), rt: rt.into(), session, url: self.board.url(n), resume: None };
        if !self.may_launch(n) || self.gui_refused(&v.issue, rt) {
            self.pending.insert((n, id));
            self.held.push(l);
            self.beat()?;
            return Ok(None);
        }
        Ok(Some((l, log)))
    }

    /// This host lists `gui` (§ 4 Filter): the probe holds and a runtime runs with Full access.
    pub fn gui(&self) -> Result<(), String> {
        self.gui_probe.clone()?;
        if let Some((k, v)) = self.grants.iter().find(|(_, v)| *v == "missing" || *v == "stale") {
            return Err(format!("{} grant {v} (TCC)", if k == "app_data" { "App Data" } else { "Accessibility" }));
        }
        if self.gui_checked.is_none_or(|at| at.elapsed().as_secs() >= 300) {
            return Err("GUI capture evidence missing or stale (300 s)".into());
        }
        if self.full.is_empty() { Err("no runtime with Full access".into()) } else { Ok(()) }
    }

    /// A `needs-gui` attempt may not run on `rt` now (§ 4 Filter): the probe fails or `rt` lost Full access.
    /// Its resume or held launch waits with the inbox and lease kept.
    fn gui_refused(&self, i: &Issue, rt: &str) -> bool {
        self.gui_reason(i, Some(rt)).is_some()
    }

    fn gui_reason(&self, i: &Issue, rt: Option<&str>) -> Option<String> {
        if !needs_gui(i) { return None; }
        if let Err(reason) = self.gui() { return Some(format!("GUI unavailable on {}: {reason}", self.cfg.me)); }
        if let Some(rt) = rt {
            if let Some(reason) = self.refused_access.get(rt) { return Some(format!("GUI runtime {rt} refused: {reason}")); }
            if !self.full.contains(rt) { return Some(format!("GUI runtime {rt} has no effective Full grant")); }
        }
        None
    }

    /// Write the heartbeat now: the lease of a new child or held launch is listed before the next pass judges it.
    fn beat(&self) -> Result<(), String> {
        match self.host_issue {
            Some(n) => self.board.edit(n, &self.heartbeat_body()),
            None => Ok(()),
        }
    }

    #[tracing::instrument(name="service.Service.launch",skip_all)]
    /// `log`: this pass's read of the task (the view, or the claim's spawn readback); launch reads it again only to
    /// confirm its own writes (#1359).
    fn launch(&mut self, mut l: Launch, through: u64, log: Vec<core::Entry>) -> Result<(), String> {
        let mut readback = None;
        let projection = core::project(l.task, &log, false);
        let (executions, t) = (&projection.executions, &projection.task);
        if t.attempt != Some(l.attempt) || !matches!(t.state, State::Doing | State::Review) {
            if l.resume.is_none() { release_execution(self.board, l.task, l.attempt, l.attempt, "no-start")?; }
            return Ok(());
        }
        match crate::reply_files::prepare(self.board,&self.cfg.root,l.task,&log) {
            Ok(files)=>l.files=files,
            Err(e)=>{
                self.log.push(format!("#{} attachments unknown: {e}",l.task));
                self.pending.insert((l.task,l.attempt));self.held.push(l);
                return self.beat(); // no native submission, execution intent or inbox acknowledgement
            }
        }
        // Only an in-memory admission deferral proves the external start was never called.
        let deferred=l.execution.map(|id|executions.iter().find(|x|x.id==id&&x.attempt==l.attempt&&x.release.is_none())
            .ok_or("deferred execution ownership missing")).transpose()?;
        let through=deferred.map_or(through,|x|x.through);
        let execution = if let Some(x)=deferred {x.id} else if l.resume.is_some() {
            if executions.iter().any(|x| x.release.is_none()) { return Err(format!("#{} prior execution termination unconfirmed", l.task)); }
            let previous = executions.last().ok_or("execution ownership missing")?.id;
            let op = format!("execute-{}-{through}-{previous}", l.attempt);
            // A previously submitted intent is uncertain, never permission to repeat the external call.
            if log.iter().any(|e| core::op_id(l.task, &e.body) == Some(&op)) { return Err("execution intent already submitted; reconcile native ownership".into()); }
            let id = self.post(l.task, core::with_fields(core::entry("executing", &[&through.to_string()], Some(l.attempt), ""), Some(&op), None))?;
            let fresh = crate::board::entries(self.board, l.task)?;
            if !core::executions(l.task, &fresh).iter().any(|x| x.id == id && x.release.is_none()) { return Err("execution intent readback missing".into()); }
            readback = Some(fresh);
            id
        } else {
            if !executions.iter().any(|x| x.id == l.attempt && x.release.is_none()) { return Err("initial execution ownership missing".into()); }
            if executions.iter().any(|x| x.release.is_none() && x.id != l.attempt) {
                release_execution(self.board, l.task, l.attempt, l.attempt, "no-start")?;
                return Err("previous execution termination unconfirmed".into());
            }
            l.attempt
        };
        // The reservation becomes the child; uncertain handoff keeps its execution fence.
        l.execution=Some(execution);
        self.observations.insert((l.task,execution),core::Execution {id:execution,attempt:l.attempt,through,
            host:self.cfg.me.clone(),runtime:l.rt.clone(),accepted:false,release:None,usage_hold:None});
        if l.role == "worker" && l.resume.is_none() && !self.cfg.external { if let Some(line) = seed(&self.cfg.root, l.task) { self.log.push(line); } }
        let pos = std::fs::metadata(crate::runtime::attempt_log(&self.cfg.root, &l.role, l.task, l.attempt)).map_or(0, |m| m.len());
        let began = std::time::Instant::now();
        let started = (self.start)(&l).map(|proc| {
            self.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution, task: l.task, attempt: l.attempt, rt: l.rt.clone(), proc });
            if crate::runtime::of(&l.rt).child {
                // A failed checkpoint retains the handle; polling retries persistence, never launch.
                if let Err(e)=self.kids.last_mut().unwrap().proc.checkpoint(&self.cfg,&l,execution) { self.kids.last_mut().unwrap().unknown=Some(e); }
            }
        });
        if started.as_ref().is_err_and(|e|e.class()=="admission") {
            self.pending.insert((l.task,l.attempt));
            self.held.push(l);
            self.sync_host();
            return self.beat();
        }
        let err = started.as_ref().err().map_or("none", crate::runtime::StartError::class);
        crate::profile::stage("start", l.task, l.attempt, &l.rt, Some(began.elapsed()), if started.is_ok() { "ok" } else { "failed" }, err, crate::profile::Usage::NoModel);
        match started {
            Ok(()) => {
                self.reserved.retain(|r| r.0 != l.task || r.1 != l.attempt);
                self.sync_host();
                crate::profile::open(&self.cfg.install, &self.cfg.repo, &self.cfg.me, l.task, l.attempt, &l.rt, l.role == "reviewer");
                self.begun.insert(l.attempt, (l.role.clone(), l.rt.clone(), Some((std::time::Instant::now(), pos))));
                self.pending.remove(&(l.task, l.attempt));
                self.gate.launch(self.mono);
                // Keep the owned handle even if acceptance/readback fails.
                let k = self.kids.last_mut().unwrap();
                match k.proc.accepted().and_then(|yes| if yes { accept_execution(self.board, l.task, l.attempt, execution, Some(readback.as_deref().unwrap_or(&log))).map(Some) } else { Ok(None) }) {
                    Ok(Some(cursor)) => { self.delivered.insert(l.attempt, cursor); self.cache.remove(&l.task); },
                    Ok(None) => {},
                    Err(e) => { k.unknown=Some(e); },
                }
                self.beat()
            }
            Err(e) => {
                if !e.no_start {
                    let reason = format!("execution {execution} launch outcome unknown: {e}");
                    self.execution_waits.insert(l.task, reason.clone());
                    let reservation = (l.task, l.attempt, l.rt.clone());
                    if !self.reserved.contains(&reservation) { self.reserved.push(reservation); }
                    self.pending.insert((l.task, l.attempt));
                    self.sync_host();
                    self.beat()?;
                    return Err(reason);
                }
                self.pending_releases.insert((l.task, l.attempt, execution));
                release_execution(self.board, l.task, l.attempt, execution, "no-start")?;
                self.retired(l.task,l.attempt,execution,false);
                self.sync_host();
                if e.reportable {
                    #[cfg(not(test))]
                    let agents = crate::runtime::agents(&self.cfg.install);
                    #[cfg(test)]
                    let agents = self.agent_status.clone();
                    if agents.as_array().is_some_and(|rows| rows.iter().any(|r| r["runtime"] == l.rt && r["path"].is_string() && r["login"] == "logged in")) {
                        crate::incident::detect("dispatch.start", concat!("src/service.rs:", line!()), &l.rt, &[], &["runtime:installed"]);
                    }
                }
                let text = format!("start failed: {e}");
                // without a fresh fold a gone could park the task on a card without options: post nothing, report the
                // read error; the next pass reconciles the no-start proof and posts the limit card (#1123).
                let log = crate::board::entries(self.board, l.task).map_err(|r| format!("#{} {text}; gone not posted: {r}", l.task))?;
                if l.resume.is_some() && log.iter().any(|e| t.inbox.contains(&e.id)
                    && e.body.lines().next() == Some(core::entry("steer", &["quota"], Some(l.attempt), "").as_str())) {
                    self.execution_waits.insert(l.task,format!("quota continuation unavailable: {e}; preserve original session/account"));
                    self.pending.insert((l.task,l.attempt));
                    return self.beat();
                }
                // #1946 (pilotq#170): the cap refusal repeats until the contract shrinks, so one ask replaces gone retries
                if e.class() == "rules-cap" {
                    let card = refused_card(l.task, &e.to_string());
                    self.post(l.task, core::with_fields(core::entry("ask", &[], Some(l.attempt), &card), Some(&format!("rules-cap-{execution}")), None))?;
                    return Ok(());
                }
                if l.resume.is_some() {
                    let failures = core::executions(l.task, &log).iter().filter(|x| x.attempt == l.attempt && x.through == through && x.release == Some(core::Release::NoStart)).count();
                    if failures >= core::GONE_MAX as usize {
                        let op = format!("input-retry-{execution}");
                        let card = input_card(l.task, &text, failures);
                        self.post(l.task, core::with_fields(core::entry("ask", &[], Some(l.attempt), &card), Some(&op), None))?;
                        return Ok(());
                    }
                    return Err(format!("#{} {text}; input retained for retry", l.task));
                }
                self.gone(l.task, &core::fold(l.task, &log, false), l.attempt, &[], &text).map(|_| ())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, Fake, entries};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct FakeProc(Rc<RefCell<Option<i32>>>);
    impl Proc for FakeProc {
        fn accepted(&mut self) -> Result<bool, String> { Ok(true) }
        fn exited(&mut self) -> Result<Option<i32>, String> {
            // -999: a failed read
            match *self.0.borrow() {
                Some(-999) => Err("read failed".into()),
                c => Ok(c),
            }
        }
        fn kill(&mut self) -> Result<(), String> {
            *self.0.borrow_mut() = Some(-9); Ok(())
        }
        fn id(&self) -> u32 {
            0
        }
    }

    // Chain: § 2 visual evidence -> service copy -> tray inline (#1782). Risk: the app, which has no board token, never
    // gets the image of an open ask, or keeps copies of answered asks.
    #[test]
    fn open_ask_images_are_copied_for_the_app_and_dropped_after() {
        let root = std::env::temp_dir().join(format!("taskq-1782-images-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let b = Fake::default(); let mut c = cfg("mac"); c.root = root.clone();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:session mac").unwrap();
        let png = b"\x89PNG\r\n\x1a\nimage".to_vec();
        let url = "https://board.example/1/0-front.png".to_string();
        b.state.borrow_mut().images.push((url.clone(), png.clone()));
        b.comment(n, &format!("taskq: ask attempt {a}\nDone: d\nProblem: p\nEvidence: ![front]({url}), ![gone](https://board.example/1/9-x.png)\nTo answer: `taskq answer {n} --text <done>`.")).unwrap();
        let mut s = Service::new(&b, &c, "fixture", Box::new(|_| panic!("no launch")));
        s.pass().unwrap();
        assert_eq!(std::fs::read(crate::snapshot::image_copy(&root, &url)).unwrap(), png);
        assert_eq!(std::fs::read(crate::snapshot::image_copy(&root, "https://board.example/1/9-x.png")).unwrap(), b"", "a failed read leaves the link");
        #[cfg(any(target_os = "macos", windows))]
        {
            let mut task = json!({"question": crate::snapshot::question(&format!("Done: d\nProblem: p\nEvidence: ![front]({url})"))});
            crate::tray::inline_images(&root, &mut task);
            assert_eq!(task["question"]["evidence"][0]["src"], "data:image/png;base64,iVBORw0KGgppbWFnZQ==");
        }
        b.comment(n, "taskq: answer\nok").unwrap();
        s.pass().unwrap();
        assert!(!crate::snapshot::image_copy(&root, &url).exists(), "answered ask: copy removed");
        std::fs::remove_dir_all(&root).unwrap();
    }

    // Chain: #1386 boot release. Risk: a dead pre-boot execution holds capacity forever, or a live one is released.
    #[test]
    fn boot_after_intent_releases_only_this_hosts_older_executions() {
        let after: fn() -> Result<i64, String> = || Ok(2_000_000_000);
        let before: fn() -> Result<i64, String> = || Ok(500_000);
        let failed: fn() -> Result<i64, String> = || Err("sysctl kern.boottime failed".into());
        for (case, boot, host) in [("after", after, "mac"), ("before", before, "mac"), ("failed", failed, "mac"), ("foreign", after, "win")] {
            let b = Fake::default(); b.advance(1_000_000); let mut c = cfg("mac");
            let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
            let a = b.comment(n, &format!("taskq: spawn worker codex:session {host}")).unwrap();
            c.root = started_root("boot", n, a);
            let mut s = Service::new(&b, &c, "fixture", Box::new(|_| panic!("no launch")));
            s.boot = boot;
            s.pass().unwrap(); s.pass().unwrap();
            let log = entries(&b, n).unwrap();
            let releases: Vec<_> = log.iter().filter(|e| e.body.starts_with("taskq: released")).collect();
            if case == "after" {
                assert_eq!(releases.len(), 1, "one release over two passes");
                assert_eq!(releases[0].body, format!("taskq: released {a} exit attempt {a}\nhost booted 2033-05-18T03:33:20Z after intent 1970-01-12T13:46:42Z\nop-id: release-{a}"));
                assert_eq!(core::executions(n, &log)[0].release, Some(core::Release::Exit));
            } else {
                assert!(releases.is_empty(), "{case}");
                assert!(s.execution_waits[&n].contains("termination unconfirmed"), "{case}");
            }
        }
    }

    // Chain: saved refused launch -> no-start release -> admission. Risk: a refused Claude launch blocks its task forever (#1557).
    #[test]
    fn a_saved_refused_claude_launch_reconciles_as_no_start() {
        for case in ["requeued","doing"] {
            let root=std::env::temp_dir().join(format!("taskq-1557-{case}-{}",std::process::id()));
            std::fs::create_dir_all(root.join(".taskq")).unwrap();
            let b=Fake::default();let mut c=cfg("mac");c.root=root.clone();
            let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
            let a=b.comment(n,"taskq: spawn worker claude:session mac").unwrap();
            if case=="requeued" {b.comment(n,"taskq: requeue\ntrust set").unwrap();}
            std::fs::write(crate::runtime::attempt_log(&root,"worker",n,a),format!("{}\n{}\n",
                json!({"type":"taskq.input","execution":a}),json!({"type":"claude.launch","stdout":"","stderr":"Workspace not trusted"}))).unwrap();
            let launched=Rc::new(RefCell::new(0));let count=launched.clone();
            let mut s=Service::new(&b,&c,"fixture",Box::new(move |_|{*count.borrow_mut()+=1;Err("fixture: stop after admission".into())}));
            s.recover=Some(Box::new(|_,_|panic!("a refused launch needs no native observer")));s.focus=vec!["all".into()];
            let first=s.pass();
            let log=entries(&b,n).unwrap();
            assert_eq!(core::executions(n,&log)[0].release,Some(core::Release::NoStart),"{case}");
            assert!(!s.execution_waits.get(&n).is_some_and(|w|w.contains("input")),"{case}: no 'accept input' wait");
            if case=="doing" {
                let gone=log.iter().find(|e|e.body.starts_with("taskq: gone")).expect("doing attempt ends");
                assert!(gone.body.contains("refused: Workspace not trusted"),"{}",gone.body);
            } else {
                let _=s.pass();
                assert_eq!(*launched.borrow(),1,"the requeued task is admitted again: {first:?} {:?}",s.log);
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    // Chain: cold service -> board ownership -> observer -> terminal receipt. Risk: recovery replays an uncertain submit.
    #[test]
    fn cold_native_observers_never_use_the_launch_path() {
        for rt in ["claude","codex-app"] {
            for case in ["live","unknown","missing","foreign","released","ambiguous"] {
                let b=Fake::default();let mut c=cfg("mac");
                let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
                let host=if case=="foreign" {"other"} else {"mac"};
                let a=b.comment(n,&format!("taskq: spawn worker {rt}:session {host}")).unwrap();
                c.root=started_root("cold",n,a);
                if case=="released" {release_execution(&b,n,a,a,"exit").unwrap();}
                if case=="ambiguous" {
                    b.comment(n,&core::entry("gone",&[],Some(a),"legacy gone without native exit")).unwrap();
                    b.comment(n,&format!("taskq: spawn worker {rt}:second mac")).unwrap();
                    assert_eq!(core::executions(n,&entries(&b,n).unwrap()).iter().filter(|x|x.release.is_none()).count(),2);
                }
                let seen=Rc::new(RefCell::new(Vec::new()));let calls=seen.clone();
                let exit=Rc::new(RefCell::new(if case=="unknown" {Some(-999)} else {None}));let flag=exit.clone();
                let mut s=Service::new(&b,&c,"fixture",Box::new(|_|panic!("recovery must never launch")));
                s.recover=Some(Box::new(move |task,x| {
                    calls.borrow_mut().push((task,x.attempt,x.id,x.runtime.clone()));
                    if case=="missing" {Err("original input unavailable; inspect native submission".into())}
                    else {Ok(Box::new(FakeProc(flag.clone())))}
                }));
                s.pass().unwrap();s.pass().unwrap();
                if matches!(case,"foreign"|"released"|"ambiguous") {
                    assert!(seen.borrow().is_empty(),"{rt}: {case}");
                    if case=="ambiguous" {assert!(s.execution_waits[&n].contains("multiple unresolved"));}
                } else if case=="missing" {
                    assert_eq!(seen.borrow().len(),2,"only observation is retried");
                    assert!(s.kids.is_empty());
                    assert!(s.reserved.iter().any(|r|r.0==n && r.1==a));
                    assert!(s.execution_waits[&n].contains("original input unavailable"));
                    assert!(s.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().execution_wait.as_ref().unwrap().contains("original input unavailable"));
                } else {
                    assert_eq!(*seen.borrow(),vec![(n,a,a,rt.to_owned())]);
                    assert_eq!(s.kids.len(),1);
                    assert!(core::executions(n,&entries(&b,n).unwrap())[0].release.is_none());
                    if case=="unknown" {assert_eq!(s.kids.iter().find(|k|k.attempt==a).unwrap().unknown.as_deref().unwrap(),"read failed");}
                    *exit.borrow_mut()=Some(0);
                    s.pass().unwrap();
                    assert!(s.kids.is_empty());
                    assert_eq!(core::executions(n,&entries(&b,n).unwrap())[0].release,Some(core::Release::Exit));
                }
            }
        }
    }

    // Chain: re-exec during pending acceptance -> exact execution. Risk: the old delivered cursor adopts execution zero.
    #[test]
    fn handover_uses_execution_identity_before_input_acceptance() {
        let b=Fake::default(); let n=b.create("T","",&[TASK_LABEL.into()]).unwrap(); let c=cfg("mac");
        let a=b.comment(n,"taskq: spawn worker codex:session mac").unwrap();
        accept_execution(&b,n,a,a,None).unwrap(); release_execution(&b,n,a,a,"exit").unwrap();
        let input=b.comment(n,"taskq: steer\ncontinue").unwrap();
        let execution=b.comment(n,&core::entry("executing",&[&input.to_string()],Some(a),"")).unwrap();
        let mut s=service(&b,&c,Default::default());
        s.adopt(Kid { unknown:None, stopping:false, session_url:None,execution,task:n,attempt:a,rt:"codex".into(),proc:Box::new(FakeProc(Rc::new(RefCell::new(None))))}).unwrap();
        assert_eq!(s.kids[0].execution,execution,"pending acceptance does not change native ownership");
        assert_eq!(s.delivered(a),a,"handover does not invent input acceptance");
    }

    // Chain: R6, §3, §4, #2143: a renewed assignment lease must not hide historical unaccepted Exit from PM observers.
    #[test]
    fn historical_unaccepted_exit_reports_blocked_despite_renewed_lease() {
        let b=Fake::default();let mut c=cfg("mac");
        let n=b.create("Historical writer refusal","",&[TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:original-thread mac").unwrap();
        c.root=started_root("historical-writer",n,a);
        let dirty=c.root.join("dirty.txt");std::fs::write(&dirty,"keep original work").unwrap();
        accept_execution(&b,n,a,a,None).unwrap();
        b.comment(n,&core::entry("ask",&[],Some(a),"Done: checked\nProblem: owner action needed")).unwrap();
        release_execution(&b,n,a,a,"exit").unwrap();
        let input=b.comment(n,"taskq: answer\noriginal pending input").unwrap();
        let x=b.comment(n,&core::entry("executing",&[&input.to_string()],Some(a),"")).unwrap();
        release_execution(&b,n,a,x,"exit").unwrap();
        let quiet=b.create("Quiet running task","",&[TASK_LABEL.into()]).unwrap();
        let qa=b.comment(quiet,"taskq: spawn worker codex:quiet mac").unwrap();
        let lie=json!({"task":n,"attempt":a,"id":x,"through":input,"state":"running","reason":"invented stale running claim"});
        let host=b.create("taskq host mac",&format!("version test\n#{n} {a}\n#{quiet} {qa}\nexecution {lie}"),&[HOST_LABEL.into()]).unwrap();
        let mut s=Service::new(&b,&c,"fixture",Box::new(|_|panic!("historical input cannot be resubmitted")));
        s.pending.insert((n,a));
        s.kids.push(Kid {task:quiet,attempt:qa,execution:qa,rt:"codex".into(),proc:Box::new(FakeProc(Rc::new(RefCell::new(None)))),unknown:None,stopping:false,session_url:None});
        for age in [0,60,3*crate::snapshot::STUCK_AFTER] {
            b.advance(age);if age>0 {b.edit(host,&format!("{}\nat {}",s.heartbeat_body(),b.now().unwrap())).unwrap();}
            s.pass().unwrap();
            let snapshot=s.snapshot.as_ref().unwrap();
            let row=snapshot.tasks.iter().find(|r|r.n==n).unwrap();let data=row.json();
            assert_eq!(data["stuck"],true,"a fresh lease cannot mask known blocked input, even before one hour");
            assert_eq!(data["wait"],"Agent exited; input recovery required");
            assert_eq!(data["executions"][0]["state"],"unknown");
            assert_eq!(data["executions"][0]["release"],"exit");
            assert!(data["execution_wait"].as_str().unwrap().contains("inspect the pending input in Codex"));
            assert!(snapshot.text(&c,&json!({})).unwrap().contains("Stuck:"));
            assert!(snapshot.tasks.iter().find(|r|r.n==quiet).unwrap().stuck.is_none(),"a quiet live worker is not stuck");
            let beat=s.heartbeat_body();assert!(beat.lines().any(|l|l==format!("#{n} {a}")),"assignment ownership stays leased");
            let status=beat.lines().filter_map(|l|l.strip_prefix("execution ")).filter_map(|l|serde_json::from_str::<Value>(l).ok()).find(|r|r["id"]==x).unwrap();
            assert_eq!(status["state"],"unknown","heartbeat must expose blocked input, not a live worker");
            let (views,hosts)=views(&b,&mut Default::default(),&mut Default::default(),c.ttl).unwrap();
            let remote=crate::snapshot::from_views("board",&b,&c,b.now().unwrap(),&views,&hosts);
            assert_eq!(remote.tasks.iter().find(|r|r.n==n).unwrap().json()["stuck"],true,"the PM's board read agrees with local status");
            assert_eq!(task(&b,n).attempt,Some(a));assert_eq!(task(&b,n).session.as_deref(),Some("codex:original-thread mac"));
            assert_eq!(core::delivered(n,&entries(&b,n).unwrap(),a),Some(a));
            assert_eq!(core::executions(n,&entries(&b,n).unwrap()).iter().find(|e|e.id==x).unwrap().release,Some(core::Release::Exit));
        }
        assert_eq!(s.log.iter().filter(|l|l.starts_with(&format!("#{n} event stuck doing "))).count(),1,"one actionable stuck episode, no renewed-lease silence or repeated alert");
        assert_eq!(std::fs::read_to_string(&dirty).unwrap(),"keep original work");
        drop(s);std::fs::remove_dir_all(c.root).unwrap();
    }

    // R2/R11/R12, #2143: native pre-submit writer refusal must retain input, never replay unknown submission or stop the app.
    #[test]
    fn codex_writer_refusal_keeps_unknown_input_fenced_and_bounds_no_start_retry() {
        for case in ["refused", "reap", "unknown", "active", "unbound", "duplicate", "foreign", "signal"] {
            let b=Fake::default(); let mut c=cfg("mac");
            let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
            let thread="11111111-1111-4111-8111-111111111111";
            let a=b.comment(n,&format!("taskq: spawn worker codex:{thread} mac")).unwrap();
            c.root=started_root(case,n,a);
            let dirty=c.root.join("dirty.txt");std::fs::write(&dirty,"keep dirty work").unwrap();
            accept_execution(&b,n,a,a,None).unwrap();release_execution(&b,n,a,a,"exit").unwrap();
            b.comment(n,&core::entry("ask",&[],Some(a),"Done: checked\nProblem: owner action needed")).unwrap();
            let input=b.comment(n,"taskq: answer\nretained input").unwrap();
            let calls=Rc::new(RefCell::new(0));let seen=calls.clone();let root=c.root.clone();
            let mut s=Service::new(&b,&c,"fixture",Box::new(move |l| {
                *seen.borrow_mut()+=1;
                assert!(l.resume.as_ref().unwrap().contains("retained input"));
                let log=crate::runtime::attempt_log(&root,"worker",n,a);
                let binding=json!({"type":"taskq.input","runtime":"codex","task":n,"attempt":a,"execution":l.execution.unwrap()+u64::from(case=="foreign"),
                    "path":"invented-input.txt","input":"retained input","native":{"session":thread}});
                let mut stream=if case=="unbound" {String::new()} else {format!("{binding}\n")};
                if case=="duplicate" {stream+=&format!("{binding}\n");}
                if case=="active" {stream+="{\"type\":\"thread.started\",\"thread_id\":\"owned\"}\n{\"type\":\"turn.started\"}\n";}
                if case=="unknown" {stream+="unexpected submission response\n";}
                stream+=&format!("2026-11-01T17:17:45Z ERROR codex_core::session: Failed to create session: thread-store conflict: thread {thread} already has an active writer\nError: thread/resume: thread/resume failed: thread {thread} already has an active writer (code -32600)\n");
                let start=std::fs::metadata(&log).map_or(0,|m|m.len());
                use std::io::Write;
                std::fs::OpenOptions::new().append(true).create(true).open(&log).unwrap().write_all(stream.as_bytes()).unwrap();
                Ok(Box::new(crate::runtime::CliTurn {proc:Box::new(FakeProc(Rc::new(RefCell::new(Some(if case=="signal" {-1} else {1}))))),log,pos:start,start,
                    stopping:false,accepted:false,read_error:None,session:None,native:None,recovery:None}))
            }));
            s.pass().unwrap();if case=="reap" {s.reap_owned();} else {s.pass().unwrap();}
            let x=core::executions(n,&entries(&b,n).unwrap()).pop().unwrap();
            assert_eq!(x.accepted,case=="active");
            assert_eq!(x.release,Some(if matches!(case,"refused"|"reap") {core::Release::NoStart} else {core::Release::Exit}),"{case}");
            if matches!(case,"refused"|"reap") {
                s.pass().unwrap();s.pass().unwrap();s.pass().unwrap();
                assert_eq!(*calls.borrow(),2,"no-start retry is bounded");
                assert_eq!(task(&b,n).state,State::Ask);
                assert!(task(&b,n).card.unwrap().contains("close that thread in Codex"));
                assert_eq!(core::delivered(n,&entries(&b,n).unwrap(),a),Some(a));
            } else if case!="active" {
                s.pass().unwrap();assert_eq!(*calls.borrow(),1,"unknown submission must never replay");
                assert!(s.execution_waits[&n].contains("inspect the pending input in Codex"));
                assert_eq!(core::delivered(n,&entries(&b,n).unwrap(),a),Some(a));
            } else {assert!(core::delivered(n,&entries(&b,n).unwrap(),a).unwrap()>=input);}
            assert_eq!(std::fs::read_to_string(&dirty).unwrap(),"keep dirty work");
            drop(s);std::fs::remove_dir_all(c.root).unwrap();
        }
    }

    // Chain: undelivered continuation -> no-start -> restart -> retry. Risk: an intent cursor loses input or retries without a bound.
    #[test]
    fn continuation_no_start_retries_retained_input_across_restart() {
        for refused in [1, 2] {
            let b = Fake::default();
            let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
            let a = b.comment(n, "taskq: spawn worker codex:session mac").unwrap();
            accept_execution(&b,n,a,a,None).unwrap(); release_execution(&b,n,a,a,"exit").unwrap();
            let cursor = b.comment(n,"taskq: steer\nretained user input").unwrap();
            let c = cfg("mac");
            let calls = Rc::new(RefCell::new(Vec::new()));
            for count in 0..refused {
                let seen = calls.clone();
                let mut s = Service::new(&b,&c,"fixture",Box::new(move |l| {
                    assert!(l.execution.is_some_and(|id|id>l.attempt));
                    seen.borrow_mut().push((l.resume.clone().unwrap(),l.execution.unwrap()));
                    Err(crate::runtime::StartError::io(std::io::Error::other("pre-submit refusal"),None))
                }));
                let result = s.pass();
                if count + 1 < core::GONE_MAX { assert!(result.unwrap_err().contains("input retained")); } else { result.unwrap(); }
                assert_eq!(core::delivered(n,&entries(&b,n).unwrap(),a),Some(a));
                assert_eq!(task(&b,n).attempt,Some(a));
            }
            if refused == 2 {
                assert_eq!(task(&b,n).state,State::Ask);
                let mut parked = service(&b,&c,Default::default()); parked.pass().unwrap();
                assert!(parked.kids.is_empty());
                b.comment(n,"taskq: answer 1").unwrap();
            }
            let seen = calls.clone();
            let mut recovered = Service::new(&b,&c,"fixture",Box::new(move |l| {
                assert!(l.execution.is_some_and(|id|id>l.attempt));
                seen.borrow_mut().push((l.resume.clone().unwrap(),l.execution.unwrap()));
                Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))))
            }));
            recovered.pass().unwrap();
            assert_eq!(calls.borrow().len(),refused as usize + 1);
            assert!(calls.borrow().iter().all(|(input,_)|input.contains("retained user input")));
            // #1834: the input's marker names the delivered steer as the owner's, for the live worker view.
            let inbox = crate::runtime::turn_inbox(&calls.borrow().last().unwrap().0).unwrap();
            assert_eq!(inbox[0], serde_json::json!({"kind":"steer","text":"retained user input"}));
            assert!(core::delivered(n,&entries(&b,n).unwrap(),a).unwrap() >= cursor);
            let execution = recovered.kids[0].execution;
            assert_eq!(calls.borrow().last().unwrap().1,execution,"adapter receives the exact continuation intent");
            recovered.pass().unwrap();
            assert_eq!(recovered.kids[0].execution,execution);
            drop(recovered);
            let mut restarted = service(&b,&c,Default::default()); restarted.pass().unwrap();
            assert!(restarted.kids.is_empty());
            assert!(restarted.execution_waits[&n].contains("termination unconfirmed"));
            assert!(restarted.retained_attempts().contains(&a),"lost handle retains its input evidence after restart");
        }
    }

    // Chain: native acceptance/exit -> failed or lost board response -> durable readback. Risk: evidence retry replays work or drops its handle.
    #[test]
    fn acceptance_readback_retries_evidence_and_keeps_the_native_handle() {
        struct Deferred { accepted: Rc<std::cell::Cell<bool>>, exit: Rc<std::cell::Cell<bool>> }
        impl Proc for Deferred {
            fn accepted(&mut self) -> Result<bool,String> { Ok(self.accepted.get()) }
            fn exited(&mut self) -> Result<Option<i32>,String> { Ok(self.exit.get().then_some(0)) }
            fn kill(&mut self) -> Result<(),String> { self.exit.set(true); Ok(()) }
            fn id(&self) -> u32 { 0 }
        }
        let b=Fake::default(); let n=b.create("T","",&[TASK_LABEL.into()]).unwrap(); let c=cfg("mac");
        let accepted=Rc::new(std::cell::Cell::new(false)); let exit=Rc::new(std::cell::Cell::new(false));
        let starts=Rc::new(std::cell::Cell::new(0)); let (ac,ex,calls)=(accepted.clone(),exit.clone(),starts.clone());
        let mut s=Service::new(&b,&c,"fixture",Box::new(move |_| {
            calls.set(calls.get()+1); Ok(Box::new(Deferred { accepted:ac.clone(),exit:ex.clone() }))
        }));
        s.focus=vec!["all".into()]; s.pass().unwrap(); let a=task(&b,n).attempt.unwrap();
        assert_eq!(s.delivered(a),0,"a created handle is not acceptance");
        assert_eq!(core::delivered(n,&entries(&b,n).unwrap(),a),None);
        b.comment(n,&core::entry("ask",&[],Some(a),"Which?\n1. Continue ★")).unwrap();
        accepted.set(true); exit.set(true); b.state.borrow_mut().fail_comments=1;
        s.pass().unwrap();
        assert_eq!(s.kids.len(),1,"retain native proof until its receipt is durable");
        assert_eq!(s.delivered(a),0); assert_eq!(starts.get(),1);
        assert!(s.kids.iter().find(|k|k.attempt==a).unwrap().unknown.as_deref().unwrap().contains("acceptance unconfirmed"));
        s.kids[0].stopping=true;
        s.kids[0].session_url=Some("https://claude.ai/code/session_014abc".into());
        s.rebuild();
        assert!(s.kids[0].stopping && s.kids[0].unknown.is_some());
        assert_eq!(s.kids[0].session_url.as_deref(),Some("https://claude.ai/code/session_014abc"));
        b.state.borrow_mut().lose_comment_response=true;
        s.pass().unwrap();
        assert_eq!(s.delivered(a),a); assert!(s.kids.is_empty()); assert_eq!(starts.get(),1);
        assert!(!s.heartbeat_body().contains("session #"),"retired execution cannot leave a session URL behind");
        let log=entries(&b,n).unwrap();
        assert_eq!(log.iter().filter(|e|e.body.starts_with("taskq: accepted ")).count(),1);
        assert_eq!(core::executions(n,&log)[0].release,Some(core::Release::Exit));
        assert!(release_execution(&b,n,a,a,"no-start").unwrap_err().contains("contradictory"));
        let mut restarted=service(&b,&c,Default::default()); restarted.pass().unwrap();
        assert!(restarted.kids.is_empty()); assert_eq!(task(&b,n).state,State::Ask);
    }

    // Chain: pending input -> reduced exit with unreadable acceptance; release cannot authorize resubmission.
    #[test]
    fn terminal_with_unreadable_acceptance_never_resubmits() {
        struct Terminal;
        impl Proc for Terminal {
            fn accepted(&mut self)->Result<bool,String> {Err("acceptance unavailable".into())}
            fn exited(&mut self)->Result<Option<i32>,String> {Ok(Some(0))}
            fn kill(&mut self)->Result<(),String> {Ok(())}
            fn id(&self)->u32 {0}
        }
        let b=Fake::default();let c=cfg("mac");
        let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:fixture mac").unwrap();
        let input=b.comment(n,"taskq: steer\nkeep pending input").unwrap();
        let mut s=service(&b,&c,Default::default());
        s.adopt(Kid {task:n,attempt:a,execution:a,rt:"codex".into(),proc:Box::new(Terminal),unknown:None,stopping:false,session_url:None}).unwrap();
        s.reap_owned();
        assert!(s.kids.is_empty());
        let x=core::executions(n,&entries(&b,n).unwrap()).remove(0);
        assert_eq!(x.release,Some(core::Release::Exit));assert!(!x.accepted);
        let mut restarted=service(&b,&c,Default::default());restarted.pass().unwrap();
        assert!(restarted.kids.is_empty());assert_eq!(restarted.delivered(a),0);
        assert!(restarted.execution_waits[&n].contains("acceptance unknown"));
        assert_eq!(task(&b,n).attempt,Some(a));
        assert_eq!(task(&b,n).inbox,[input]);
    }

    // Chain: terminal old turn -> pending owner input -> unavailable owner/session. Risk: recovery drops input by fabricating gone.
    #[test]
    fn unavailable_owner_or_session_preserves_pending_input() {
        for (runtime, host) in [("codex:session","other"),("codex:-","mac")] {
            let b=Fake::default(); let n=b.create("T","",&[TASK_LABEL.into()]).unwrap(); let c=cfg("mac");
            let a=b.comment(n,&core::entry("spawn",&["worker",runtime,host],None,"")).unwrap();
            accept_execution(&b,n,a,a,None).unwrap(); release_execution(&b,n,a,a,"exit").unwrap();
            let input=b.comment(n,"taskq: steer\nkeep this input").unwrap();
            b.advance(3600);
            let mut s=service(&b,&c,Default::default()); s.pass().unwrap();
            assert_eq!(task(&b,n).attempt,Some(a)); assert_eq!(task(&b,n).inbox,[input]);
            assert!(s.kids.is_empty());
            assert!(!b.comments(n).unwrap().iter().any(|c|c.body.starts_with("taskq: gone")));
            if host == "mac" {
                assert!(s.execution_waits[&n].contains("session identity unknown"));
                assert!(s.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().execution_wait.as_ref().unwrap().contains("session identity unknown"));
            } else {
                assert!(s.execution_waits[&n].contains("input pending on other"));
            }
        }
    }

    // Chain: uncertain native submit -> service restart -> result/requeue -> another host.
    // Risk: task state or a lost process handle permits duplicate external execution.
    #[test]
    fn uncertain_submission_stays_fenced_through_restart_and_task_moves() {
        for result in [true, false] {
            let b = Fake::default();
            let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
            let (ca, cb) = (cfg("mac"), cfg("win"));
            let starts = Rc::new(std::cell::Cell::new(0)); let called = starts.clone();
            let mut a = Service::new(&b, &ca, "fixture", Box::new(move |_| {
                called.set(called.get() + 1);
                let mut error = crate::runtime::StartError::io(std::io::Error::other("response lost after submission"), None);
                error.no_start = false;
                Err(error)
            }));
            a.focus = vec!["all".into()];
            assert!(a.pass().unwrap_err().contains("launch outcome unknown"));
            let attempt = task(&b, n).attempt.unwrap();
            assert_eq!(a.entry().held.len(), 1, "unknown submission retains capacity");
            drop(a);
            let exits: Exits = Default::default();
            let mut restored = service(&b, &ca, exits.clone());
            restored.pass().unwrap();
            assert_eq!(restored.entry().held.len(), 1, "restart rebuilds unresolved ownership");
            assert!(exits.borrow().is_empty());
            b.comment(n, &if result { core::entry("result", &[&"a".repeat(40)], Some(attempt), "ready for review") }
                else { core::entry("requeue", &[], Some(attempt), "retry") }).unwrap();
            b.advance(900);
            let mut other = service(&b, &cb, exits.clone());
            other.pass().unwrap(); restored.pass().unwrap();
            assert_eq!(starts.get(), 1); assert!(exits.borrow().is_empty());
            assert!(other.execution_waits.contains_key(&n));
            release_execution(&b, n, attempt, attempt, "exit").unwrap();
            other.pass().unwrap();
            assert_eq!(exits.borrow().len(), 1, "fresh native exit evidence permits the next role");
            restored.pass().unwrap();
            assert!(restored.entry().held.is_empty(), "another host's new attempt cannot retain the old reservation");
            assert_eq!(task(&b, n).role.as_deref(), Some(if result { "reviewer" } else { "worker" }));
        }
    }

    // Chain: requeue -> stop request -> uncertain exit -> occupied slot -> confirmed exit -> replacement.
    #[test]
    fn cancellation_retains_an_unconfirmed_child_and_blocks_replacement() {
        struct SlowStop(Rc<std::cell::Cell<u8>>);
        impl Proc for SlowStop {
            fn exited(&mut self) -> Result<Option<i32>, String> {
                if self.0.get() == 2 { Err("invented read failure".into()) } else { Ok((self.0.get() == 3).then_some(-9)) }
            }
            fn kill(&mut self) -> Result<(), String> {
                if self.0.get() == 1 { Err("invented stop refusal".into()) } else { Ok(()) }
            }
            fn id(&self) -> u32 { 0 }
        }
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let mut c = cfg("mac"); c.limits = BTreeMap::from([("claude".into(), 3)]);
        let exited = Rc::new(std::cell::Cell::new(0));
        let starts = Rc::new(std::cell::Cell::new(0));
        let mut s = Service::new(&b, &c, "test", Box::new(|_| {
            starts.set(starts.get() + 1); Ok(Box::new(SlowStop(exited.clone())))
        }));
        s.focus = vec!["all".into()]; s.pass().unwrap();
        let attempt = s.kids[0].attempt;
        b.comment(n, "taskq: requeue").unwrap();
        for mode in [1, 0, 2, 0] {
            exited.set(mode);
            s.pass().unwrap();
            assert_eq!(starts.get(), 1, "an unconfirmed old process cannot overlap its replacement even with spare capacity");
            assert_eq!(s.kids[0].attempt, attempt);
            assert!(s.kids.iter().any(|k|k.attempt==attempt && k.unknown.is_some()));
            assert_eq!(task(&b, n).state, State::Ready);
            s.reap_owned();
            assert!(s.kids.iter().any(|k|k.attempt==attempt && k.unknown.is_some()), "reduced monitoring must retain the pending-stop reason");
        }
        exited.set(3); s.pass().unwrap();
        assert_eq!(starts.get(), 2);
        assert!(s.kids.iter().all(|k| k.attempt != attempt));
    }

    // Chain: §4 log retention -> service pass -> collection/selftest. Risk: the hourly timer leaves a full log stuck.
    #[test]
    fn service_pass_restores_bounded_log_collection() {
        let root = std::env::temp_dir().join(format!("taskq-service-retention-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("taskq.log");
        let c = Config { install: root.clone(), ..cfg("win") };
        let b = Fake::default();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no tasks")));
        for size in [crate::data::LOG_MAX - 256, crate::data::LOG_MAX + 1024] {
            let mut text = "old diagnostic\n".repeat(size as usize / 15);
            let newest = "taskq-profile {\"op\":\"spawn\",\"us\":1,\"error\":\"none\",\"at_ms\":1}\n";
            text.push_str(newest);
            let terminal = format!("taskq-stage {}\n", serde_json::json!({"project":"o/r", "host":"win",
                "stage":"build", "task":7, "attempt":41, "rt":"codex", "us":null,
                "outcome":"cancelled", "error":"none", "at_ms":1, "unknown":"adopted"}));
            text.push_str(&terminal);
            std::fs::write(&path, &text).unwrap();
            let mut writer = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            {
                let _lock=crate::profile::log_lock(&path).unwrap();
                let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                crate::telemetry::read(&root,now).unwrap();
            }
            assert!(crate::profile::selftest(&root, "o/r", &vec![], 300).unwrap().contains("no overdue"), "legacy log size no longer stops collection");
            let lock = crate::profile::log_lock(&path).unwrap();
            s.pass().unwrap();
            assert!(std::fs::read_to_string(&path).unwrap()==text, "busy lock preserves evidence");
            drop(lock);
            let (_capture,_guard)=crate::profile::tests::collecting(&root);
            s.pass().unwrap();
            let retained = std::fs::read_to_string(&path).unwrap();
            assert!(retained.len() as u64 <= crate::data::LOG_MAX / 2);
            assert!(retained.starts_with("old diagnostic\n") && retained.ends_with(&terminal));
            use std::io::Write;
            writer.write_all(b"still open diagnostic\n").unwrap();
            crate::profile::append_line(&path, newest).unwrap();
            assert!(crate::profile::selftest(&root, "o/r", &vec![], 300).unwrap().contains("no overdue"));
            assert!(!crate::profile::report(&root).contains("collection stopped"));
            drop(writer);
            drop(_guard);drop(_capture);std::fs::remove_dir_all(&root).unwrap();
            std::fs::create_dir_all(&root).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    fn cfg(me: &str) -> Config {
        Config {
            root: PathBuf::from("/nonexistent"),
            install: PathBuf::from("/nonexistent"),
            board: &crate::board::github::KIND,
            web_host: String::new(),
            repo: "o/r".into(),
            publish: "pr".into(),
            delivery: "auto-merge".into(),
            delivery_revision: 0,
            me: me.into(),
            limits: BTreeMap::from([("claude".to_string(), 1)]),
            has: vec![],
            claim: crate::host::ClaimMode::Any,
            spillover_grace: 300,
            allowed_runtimes: None,
            auto_reroute: false,
            default_runtime: "claude".into(),
            routing: vec![],
            permission_mode: "dontAsk".into(),
            codex: vec![],
            ttl: 300,
            poll: 60,
            update_every: 900,
            rollback_after: 600,
            external: false,
            host_setup: None,
        }
    }

    // Chain: Windows native reviewer launch -> service pass -> gone, on a disposable board only.
    #[cfg(windows)]
    #[test]
    #[ignore = "live Claude reviewer; TASKQ_CLAUDE_ROOT selects the worktree"]
    fn live_claude_reviewer_exit_writes_gone() {
        use crate::runtime::{ClaudeTurn, Env, access, log_path};
        let mut c = cfg("win");
        c.root = PathBuf::from(std::env::var("TASKQ_CLAUDE_ROOT").expect("TASKQ_CLAUDE_ROOT"));
        c.install = std::env::temp_dir().join(format!("taskq-claude-{}", std::process::id()));
        std::fs::create_dir_all(&c.install).unwrap();
        crate::policy::grant(&c.install, "claude", "full", None, "op", 1).unwrap();
        let env = Env { context: "", root: &c.root, exe: std::path::Path::new("taskq"), install: &c.install, permission_mode: "dontAsk", codex: &[], publish: "pr", board_env: &[], jobs: None };
        let b = Fake::default();
        let n = b.create("Exit qualification", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker claude:invented win").unwrap();
        b.comment(n, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(a), "invented candidate")).unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        let mut s = Service::new(&b, &c, "4.4.0", Box::new(|l| {
            assert_eq!(l.role, "reviewer");
            let text = format!("{}\nReply EXIT_CHECK and do nothing else.", "Qualification input only.\n".repeat(1800));
            ClaudeTurn::start(&env, l, &log_path(env.root, l), &text, &access(&env, "claude").unwrap())
                .map(|t| Box::new(t) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        let r = task(&b, n).attempt.unwrap();
        assert_ne!(r, a);
        eprintln!("win reviewer start attempt {r}");
        // End only this disposable native job; do not wait through Claude's spawn-retry backoff.
        crate::runtime::stop(s.kids[0].proc.as_mut()).unwrap();
        s.pass().unwrap();
        let log = b.comments(n).unwrap();
        for comment in &log { eprintln!("{}", comment.body); }
        for kid in &mut s.kids { crate::runtime::stop(kid.proc.as_mut()).unwrap(); }
        assert!(log.iter().any(|e| e.body.contains(&format!("taskq: gone attempt {r}"))), "next pass must reap the native terminal job");
        assert!(task(&b, n).result.is_some());
        std::fs::remove_dir_all(&c.install).unwrap();
    }

    // Chain: section 7 publication intent -> admission -> service restart -> merge receipt/close, focus off.
    // Risk: a restart launches another reviewer, expires the ended attempt, or forgets the approved verdict.
    #[test]
    fn pending_publication_survives_restart_without_a_reviewer() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented publication", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        let verdict = "Changed: tested publication\nChecks: Task fit A1 ok; merge-result CI pending\nOpen: none";
        b.comment(n, &core::entry("publish", &[], Some(r), verdict)).unwrap();
        b.state.borrow_mut().publish = "queue-admitted".into();
        {
            let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no reviewer for pending publication")));
            s.focus = vec!["all".into()];
            s.pass().unwrap();
        }
        assert!(task(&b, n).enqueued.is_some());
        b.state.borrow_mut().publish = "queue-waiting".into();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process while queued")));
        s.pass().unwrap();
        assert_eq!(task(&b, n).publication.as_deref(), Some(verdict));
        assert_eq!(task(&b, n).retries, 0);
        assert!(b.issue(n).unwrap().open);
        b.state.borrow_mut().publish = String::new();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Closed);
        assert!(!b.issue(n).unwrap().open);
        let comments = b.comments(n).unwrap();
        let close = comments.iter().find(|e| e.body.starts_with("taskq: close")).unwrap();
        // § 2 Summary: the close names the publish it repeats; readers resolve it to the verdict
        assert!(close.body.contains(core::REPEATS) && !close.body.contains(verdict), "{}", close.body);
        assert!(core::verdict(n, &entries(&b, n).unwrap(), core::entry_text(n, &close.body)).contains(verdict));
        assert_eq!(comments.iter().filter(|e| e.body.starts_with("taskq: close")).count(), 1);
    }

    // Chain: § 7 GitLab `direct` close -> MR created -> merge refused `ci_still_running` -> `publish` -> service pass -> merge
    // or `publication-failed` (#1939). Defect (cyprusbz #540, MR !1196): close refused while the MR pipeline ran, the
    // reviewer exited, the service wrote `gone` and a second reviewer burned a retry. Retained: the running pipeline is a
    // visible wait with its link; green merges and closes once, red or cancelled fails once with the link and stops polling.
    #[test]
    fn gitlab_close_waits_for_the_mr_pipeline_then_merges_or_fails_once() {
        for end in ["success", "failed", "canceled"] {
            let (b, mut c) = (Fake::default(), cfg("win"));
            c.publish = "direct".into();
            let n = b.create("Invented GitLab close", "", &["taskq".into()]).unwrap();
            let sha = "a".repeat(40);
            let pipeline = Rc::new(RefCell::new("running"));
            // created MRs, merges
            let writes = Rc::new(RefCell::new((0, 0)));
            let (p, w, result) = (pipeline.clone(), writes.clone(), sha.clone());
            let link = "https://gitlab.example/o/r/-/pipelines/9";
            b.state.borrow_mut().publisher = Some(crate::board::replying(&crate::board::gitlab::KIND, move |method, url| {
                let (created, merged) = *w.borrow();
                let mr = json!({"iid": 1196, "sha": result});
                Ok(if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                    // the branch pipeline passed; the MR pipeline on the same SHA starts once the MR exists
                    else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": if created == 0 { "success" } else { *p.borrow() }, "web_url": link}}) }
                    else if url.contains("state=opened") { if created == 0 { json!([]) } else { json!([mr]) } }
                    else if url.contains("state=merged") { json!([]) }
                    else if method == "POST" && url.ends_with("/merge_requests") { w.borrow_mut().0 += 1; mr }
                    else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": "ff", "autoclose_referenced_issues": false}) }
                    else if method == "PUT" && url.ends_with("/merge_requests/1196/merge") { w.borrow_mut().1 += 1; json!({}) }
                    else if url.ends_with("/merge_requests/1196") {
                        let status = match *p.borrow() { "running" => "ci_still_running", "success" => "mergeable", _ => "ci_must_pass" };
                        json!({"state": if merged > 0 { "merged" } else { "opened" }, "detailed_merge_status": status, "head_pipeline": {"status": *p.borrow(), "web_url": link}})
                    }
                    else { panic!("unexpected {method} {url}") })
            }));
            let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
            b.comment(n, &core::entry("result", &[&sha], Some(a), "candidate")).unwrap();
            b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
            let verdict = format!("Changed: x\nChecks: A1 ok; CI green on {sha}\nOpen: none");
            let op = crate::dispatch::Op::default();
            let got = crate::dispatch::close_one(&b, &c, n, &verdict, &op);
            assert!(got.ok() && got.extra.contains(link), "{end}: {got:?}");
            assert_eq!((task(&b, n).state, task(&b, n).publication.as_deref()), (State::Review, Some(verdict.as_str())), "{end}");
            assert!(crate::dispatch::close_one(&b, &c, n, &verdict, &op).ok(), "{end}: a repeated close keeps the saved verdict");
            let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no reviewer while the MR pipeline runs")));
            for _ in 0..2 { s.pass().unwrap(); }
            let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
            assert!(row["publication"]["reason"].as_str().unwrap().contains(link), "{end}: {row}");
            *pipeline.borrow_mut() = end;
            s.pass().unwrap();
            let (t, log) = (task(&b, n), b.comments(n).unwrap());
            let count = |kind: &str| log.iter().filter(|e| e.body.starts_with(&format!("taskq: {kind}"))).count();
            assert_eq!((count("gone"), count("spawn reviewer"), count("publish"), *writes.borrow()), (0, 1, 1, (1, usize::from(end == "success"))), "{end}");
            if end == "success" {
                assert_eq!((t.state, t.retries, count("close")), (State::Closed, 0, 1), "{end}");
            } else {
                assert_eq!((t.state, t.retries, count("publication-failed"), t.publication), (State::Ready, 1, 1, None), "{end}");
                assert!(log.iter().any(|e| e.body.starts_with("taskq: publication-failed") && e.body.contains(&format!("CI {end} on {sha} {link}"))), "{end}");
                let calls = b.state.borrow().publications;
                s.pass().unwrap();
                assert_eq!(b.state.borrow().publications, calls, "{end}: no publication polls after it failed");
            }
        }
    }

    // Chain: section 7 publication -> refused enqueue -> status row reason and age, one `checks` record -> conflict requeue (#1820).
    // Defect (pilotq #106): a publication the pass could not move was only logged, so the task sat in review 50 minutes with no
    // board, menu or telemetry signal. Retained: every pass names the wait in the status row; a conflict requeues in the same pass.
    #[test]
    fn publication_stall_shows_its_reason_and_age_then_requeues_on_conflict() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented stalled publication", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        b.state.borrow_mut().publish = "queue-refused".into();
        crate::profile::STAGED.with(|v| v.borrow_mut().clear());
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a waiting publication")));
        for _ in 0..2 { s.pass().unwrap(); }
        let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
        let reason = "merge queue did not admit PR #7: Required status check build is in progress; CI build pending";
        assert_eq!((&row["state"], &row["publication"]["reason"]), (&json!("review"), &json!(reason)));
        assert!(row["publication"]["since"].is_i64() && row["publication"]["age_s"].is_i64(), "{row}");
        assert!(row["wait"].as_str().unwrap().starts_with(crate::snapshot::PUBLICATION_WAIT) && row["wait"].as_str().unwrap().ends_with(reason), "{row}");
        let checks = crate::profile::STAGED.with(|v| v.borrow().iter().filter(|l| l.starts_with(&format!("checks {n} "))).cloned().collect::<Vec<_>>());
        assert_eq!(checks.len(), 1, "one record per reason, not per pass: {checks:?}");
        assert!(checks[0].ends_with(" retried other"), "{checks:?}");
        b.state.borrow_mut().publish = "queue-conflict".into();
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!((t.state, t.retries), (State::Ready, 0), "main-moved keeps retries");
        assert!(b.comments(n).unwrap().iter().any(|e| e.body.starts_with("taskq: requeue main-moved") && e.body.contains("queue merge conflict")));
        assert!(s.publication_waits.is_empty());
    }

    // Chain: section 7 publication -> manual-merge wait -> status/menu row with the PR link -> one board comment (#1935).
    // Defect (Kirsalina/desiign): review tasks waited up to 75 minutes for a person to merge with no reason on any surface.
    // Retained: the row names the wait and PR; the board gets one plain comment, not one per pass and not a log entry.
    #[test]
    fn manual_merge_wait_is_shown_with_its_pr_and_commented_once() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented manual merge", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        b.state.borrow_mut().publish = "manual-merge".into();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a waiting publication")));
        for _ in 0..3 { s.pass().unwrap(); }
        let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
        assert!(row["wait"].as_str().unwrap().contains(crate::board::MANUAL_MERGE) && row["wait"].as_str().unwrap().contains("https://github.com/o/r/pull/7"), "{row}");
        let notes: Vec<_> = b.comments(n).unwrap().into_iter().filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE)).collect();
        assert_eq!(notes.len(), 1, "one board comment, not one per pass");
        assert!(notes[0].body.contains("https://github.com/o/r/pull/7"));
        assert_eq!(task(&b, n).state, State::Review, "the note is no log entry");
        s.publication_waits.clear();
        s.pass().unwrap();
        assert_eq!(b.comments(n).unwrap().iter().filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE)).count(), 1, "a restart reads the board note back (R1)");
        // #1942: a new PR head (a later `publish` entry) gets its own note
        b.state.borrow_mut().publish = "queue-conflict".into();
        s.pass().unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"b".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        b.state.borrow_mut().publish = "manual-merge".into();
        for _ in 0..2 { s.pass().unwrap(); }
        assert_eq!(b.comments(n).unwrap().iter().filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE)).count(), 2, "one note per PR head");
    }

    // Chain: § 7 Result and close verdicts -> `publish` under `pr` -> manual-merge wait -> preset switched to `direct`
    // -> next service pass fast-forwards main to the PR head -> one close (#1942). Defect (desiign #31, #39, #44): PRs
    // published before a switch to `direct` stayed in review forever, `needs a manual merge` logged every pass.
    // Retained: a pending publication takes the current preset with its exact-SHA CI; the wait is noted once.
    #[test]
    fn a_pr_published_before_a_switch_to_direct_is_delivered_directly() {
        let (b, mut c) = (Fake::default(), cfg("win"));
        c.root = std::env::temp_dir().join(format!("taskq-1942-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&c.root).unwrap();
        let preset = |p: &str, r: u64| std::fs::write(c.root.join("taskq.json"), json!({"repo": "o/r", "delivery": {"preset": p, "revision": r}}).to_string()).unwrap();
        preset("auto-merge", 2);
        let n = b.create("Invented preset switch", "", &["taskq".into()]).unwrap();
        let sha = "a".repeat(40);
        // main fast-forwards
        let forwards = Rc::new(RefCell::new(0));
        let (f, head) = (forwards.clone(), sha.clone());
        b.state.borrow_mut().publisher = Some(crate::board::replying(&crate::board::github::KIND, move |method, url| {
            Ok(if url.contains("/compare/main...") { json!({"status": if *f.borrow() == 0 { "ahead" } else { "identical" }}) }
                else if url.ends_with("/rules/branches/main") { return Err("HTTP 403: Upgrade to GitHub Pro or make this repository public to enable this feature.".into()) }
                else if url.contains("/pulls?head=o:taskq-1&base=main&state=open") { json!([{"number": 37, "head": {"sha": head}, "html_url": "https://github.com/o/r/pull/37"}]) }
                else if url.contains("/check-runs") { json!({"check_runs": [{"name": "ci", "status": "completed", "conclusion": "success"}]}) }
                else if url.ends_with("/status") { json!({"total_count": 0}) }
                else if url.ends_with("/git/ref/heads/taskq-1") { json!({"object": {"sha": head}}) }
                else if method == "PATCH" && url.ends_with("/git/refs/heads/main") { *f.borrow_mut() += 1; json!({"object": {"sha": head}}) }
                else if url.contains("/issues/1/comments") { json!([]) } else { panic!("unexpected {method} {url}") })
        }));
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&sha], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a waiting publication")));
        // the service loop's pass: reread taskq.json, then pass
        let pass = |s: &mut Service| { s.reread_project().unwrap(); s.pass().unwrap() };
        for _ in 0..3 { pass(&mut s); }
        let notes = |b: &Fake| b.comments(n).unwrap().iter().filter(|e| e.body.starts_with(MANUAL_MERGE_NOTE)).count();
        assert_eq!((task(&b, n).state, notes(&b), *forwards.borrow(), s.cfg.publish.as_str()), (State::Review, 1, 0, "pr"), "the `pr` wait is noted once");
        // the owner edits taskq.json; the running service is not restarted
        preset("direct", 3);
        pass(&mut s);
        let log = b.comments(n).unwrap();
        let closes: Vec<_> = log.iter().filter(|e| e.body.starts_with("taskq: close")).collect();
        assert_eq!((task(&b, n).state, closes.len(), *forwards.borrow(), notes(&b)), (State::Closed, 1, 1, 1));
        assert!(closes[0].body.contains(&format!("delivery direct r3: main fast-forwarded to {sha}")), "{}", closes[0].body);
        pass(&mut s);
        assert_eq!((b.comments(n).unwrap().len(), *forwards.borrow()), (log.len(), 1), "closed once");
        drop(s);
        let _ = std::fs::remove_dir_all(&c.root);
    }

    // Chain: section 7 publication -> blocked outcome -> ask in one pass -> answer -> retry -> close (#1413).
    // Risk: a publication only the owner can unblock polls for hours with no board signal.
    #[test]
    fn blocked_publication_asks_the_owner_in_one_pass() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented blocked publication", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        b.state.borrow_mut().publish = "queue-blocked".into();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a blocked publication")));
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(t.state, State::Ask);
        let card = t.card.unwrap();
        assert!(card.contains("Problem: PR #7 merged, merge_group CI tests failure") && card.contains("1. retry:"), "{card}");
        let calls = b.state.borrow().publications;
        s.pass().unwrap();
        assert_eq!(b.state.borrow().publications, calls, "no publication polls while the owner decides");
        b.comment(n, "taskq: answer 1").unwrap();
        b.state.borrow_mut().publish = String::new();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Closed);
    }

    // Chain: section 7 Publication routes, task PR in another repository (#2047) -> service pass enqueues it in that
    // repository -> status row shows the merge queue stage -> merged with green merge_group CI -> one close naming it.
    // Defect (#2009 dashq#240, #2010 pilotq#200, #2015 dashq#241): publication looped on the board repository's compare
    // HTTP 404 with no stage shown. Retained: the foreign PR moves through the same stages and receipt as a local one.
    #[test]
    fn a_task_pr_in_another_repository_shows_its_stage_and_closes_after_its_merge() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented foreign PR", "", &["taskq".into()]).unwrap();
        let (head, merge) = ("a".repeat(40), "b".repeat(40));
        let text = "Changed: [o/x#240](https://github.com/o/x/pull/240)\nChecks: A1 ok\nOpen: none";
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        let result = core::entry("result", &[&head], Some(a), text);
        b.comment(n, &result).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        let publish = core::entry("publish", &[], Some(r), text);
        b.comment(n, &publish).unwrap();
        // 0 open, 1 queued, 2 merged
        let phase = Rc::new(std::cell::Cell::new(0));
        let (p, h, m) = (phase.clone(), head.clone(), merge.clone());
        b.state.borrow_mut().publish = "queue-foreign".into();
        b.state.borrow_mut().publisher = Some(crate::board::replying_to(&crate::board::github::KIND, move |method, url, body| {
            if let Some(rest) = url.strip_prefix("https://api.github.com/repos/o/r") {
                return if rest.starts_with("/compare/") { Err(format!("GET {url}: HTTP 404 Not Found")) }
                    else if rest.starts_with(&format!("/issues/{n}/comments")) {
                        Ok(json!([result.clone(), publish.clone()].iter().enumerate().map(|(i, b)| json!({"id": i + 1, "body": b, "author_association": "OWNER",
                            "created_at": "2026-10-30T00:00:00Z", "updated_at": "2026-10-30T00:00:00Z", "user": {"login": "owner"}})).collect::<Vec<_>>()))
                    } else { panic!("board repository read {method} {url}") };
            }
            if url.ends_with("/graphql") {
                let query = body.unwrap()["query"].as_str().unwrap().to_string();
                if query.contains("enqueuePullRequest") { p.set(1); return Ok(json!({"data": {"enqueuePullRequest": {"mergeQueueEntry": {"id": "MQE_1"}}}})); }
                let entry = if p.get() == 1 { json!({"id": "MQE_1"}) } else { Value::Null };
                return Ok(json!({"data": {"node": {"headRefOid": h, "mergeable": "MERGEABLE", "merged": p.get() == 2, "mergeCommit": if p.get() == 2 { json!({"oid": m}) } else { Value::Null },
                    "mergeQueueEntry": entry, "timelineItems": {"nodes": []}}}}));
            }
            let rest = url.strip_prefix("https://api.github.com/repos/o/x").unwrap_or_else(|| panic!("read outside o/x: {method} {url}"));
            Ok(if rest.starts_with("/compare/") { json!({"status": if rest.ends_with(&m) { "identical" } else { "diverged" }}) }
                else if rest == "/pulls/240" { json!({"number": 240, "node_id": "PR_240", "state": "open", "draft": false, "head": {"sha": h}, "merged_at": null}) }
                else if rest == "/rules/branches/main" { json!([{"type": "merge_queue"}]) }
                else if rest.starts_with("/actions/runs?") { json!({"workflow_runs": [{"head_sha": m, "event": "merge_group", "status": "completed", "conclusion": "success", "name": "ci", "check_suite_id": 5}]}) }
                else if rest.contains("/check-runs") { json!({"check_runs": [{"name": "ci", "status": "completed", "conclusion": "success", "app": {"slug": "github-actions"}, "check_suite": {"id": 5}}]}) }
                else if rest.ends_with("/status") { json!({"total_count": 0}) }
                else { panic!("unexpected {method} {url}") })
        }));
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a waiting publication")));
        for _ in 0..3 { s.pass().unwrap(); }
        let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
        assert_eq!((&row["state"], &row["enqueued"]), (&json!("review"), &json!("MQE_1")), "A2: the merge queue stage, as for a local PR: {row}");
        phase.set(2);
        s.pass().unwrap();
        let closes: Vec<_> = b.comments(n).unwrap().into_iter().filter(|e| e.body.starts_with("taskq: close")).collect();
        assert_eq!((task(&b, n).state, closes.len()), (State::Closed, 1));
        assert!(closes[0].body.contains(&format!("merged #240 in o/x as {merge}; merge-result CI green")), "A1: {}", closes[0].body);
    }

    // Chain: § 7 rule 9 -> worker closes its own task -> § 1 dependency unknown -> § 4 step 5 reopen + one ask (#1950).
    // Risk (desiign#50): issue text said "write done and close"; the worker commented done and closed the issue, so the
    // close read as done, review was skipped and q-doing stayed on a closed issue.
    #[test]
    fn worker_close_during_attempt_reopens_with_one_ask() {
        let (b, c) = (Fake::default(), cfg("mac"));
        let n = b.create("Invented: write done and close", "When finished, write done and close this issue.", &[TASK_LABEL.into()]).unwrap();
        let exit = Rc::new(RefCell::new(None));
        let mut s = Service::new(&b, &c, "test", Box::new(|_| Ok(Box::new(FakeProc(exit.clone())))));
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        let p = b.create("Invented: closed by hand", "", &[TASK_LABEL.into()]).unwrap();
        s.pass().unwrap();
        assert!(b.issue(n).unwrap().labels.contains(&"q-doing".to_string()) && b.issue(p).unwrap().labels.contains(&"q-ready".to_string()));
        // the worker's hand-in by board writes: a plain comment and a close, labels untouched
        b.raw(n, "done", true, false);
        b.close(n).unwrap();
        b.close(p).unwrap();
        let dep = blockers(&b, &format!("Depends on #{n}"), None, &mut DepCache::new());
        assert_eq!(dep.iter().map(|d| d.reason.as_str()).collect::<Vec<_>>(), ["unknown: closed during a worker attempt"], "a dependent never reads it done");
        s.pass().unwrap();
        let (issue, t) = (b.issue(n).unwrap(), task(&b, n));
        assert!(issue.open && t.state == State::Ask && t.attempt == Some(a), "{t:?}");
        assert!(t.card.as_deref().unwrap().contains("closed at") && t.card.as_deref().unwrap().contains("1. redo:"), "{:?}", t.card);
        assert!(issue.labels.contains(&"q-ask".to_string()) && !issue.labels.contains(&"q-doing".to_string()), "{:?}", issue.labels);
        assert_eq!(s.kids.len(), 1, "the live attempt is not stopped as gone");
        let p_issue = b.issue(p).unwrap();
        assert!(!p_issue.open && !p_issue.labels.iter().any(|l| l.starts_with("q-")), "a closed task keeps no lifecycle label: {:?}", p_issue.labels);
        s.pass().unwrap();
        let asks = b.comments(n).unwrap().iter().filter(|c| c.body.starts_with("taskq: ask")).count();
        assert_eq!(asks, 1, "one ask per close");
        b.comment(n, "taskq: answer 1").unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Doing, "the answer resumes the attempt");
    }

    // Chain: section 7 publication -> superseded by merged PRs -> ask with close -> `answer N.3` closes, no merge (#1779, pilotq#84).
    // Risk: the card says close by hand, `close` is refused in `ask` and retry only asks again, so the task never ends.
    #[test]
    fn superseded_publication_closes_from_its_answer_without_a_merge() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented superseded publication", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        b.state.borrow_mut().publish = "superseded".into();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a superseded publication")));
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!(t.state, State::Ask);
        let card = t.card.unwrap();
        assert!(card.contains("1. retry:") && card.contains("2. later:") && card.contains("3. close: superseded, no merge - + merged #86 at ddddddd carries the work; - this result is not merged ★"), "{card}");
        let calls = b.state.borrow().publications;
        let got = crate::dispatch::answer_picks(&b, &c, &[format!("{n}.3")], &crate::dispatch::Op::default()).unwrap().remove(0);
        assert!(got.ok(), "{got:?}");
        assert_eq!(task(&b, n).state, State::Closed);
        assert!(!b.issue(n).unwrap().open);
        assert_eq!(b.state.borrow().publications, calls, "no merge");
        s.pass().unwrap();
        assert_eq!(b.state.borrow().publications, calls, "no publication after close");
    }

    // Chain: section 7 publication-blocked ask -> `answer N --text "close: .."` is option close (#1820, pilotq #106).
    // Risk: the owner's `close:` text reads as free text, publication resumes and the same block asks again.
    #[test]
    fn blocked_publication_answer_text_with_an_option_key_applies_it_or_refuses() {
        let (b, c) = (Fake::default(), cfg("win"));
        let blocked = |kind: &str| {
            let n = b.create("Invented blocked publication", "", &["taskq".into()]).unwrap();
            let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
            b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
            let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
            b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x
Checks: A1 ok
Open: none")).unwrap();
            b.state.borrow_mut().publish = kind.into();
            Service::new(&b, &c, "test", Box::new(|_| panic!("no process for a blocked publication"))).pass().unwrap();
            assert_eq!(task(&b, n).state, State::Ask, "{kind}");
            b.state.borrow_mut().publish = String::new();
            n
        };
        let op = crate::dispatch::Op::default();
        let n = blocked("superseded");
        let calls = b.state.borrow().publications;
        let got = crate::dispatch::answer_text(&b, &c, n, "close: superseded by #86, no merge", &op).unwrap().remove(0);
        assert!(got.ok(), "{got:?}");
        assert_eq!(task(&b, n).state, State::Closed);
        assert_eq!(b.state.borrow().publications, calls, "no merge");
        let n = blocked("queue-blocked");
        let err = crate::dispatch::answer_text(&b, &c, n, "Close: drop it", &op).unwrap_err();
        assert!(err.contains(&format!("#{n} has no `close` option")) && err.contains(&format!("`taskq answer {n}.1` (retry)")), "{err}");
        assert_eq!(task(&b, n).state, State::Ask, "a refusal writes nothing");
        let got = crate::dispatch::answer_text(&b, &c, n, "later: after the freeze", &op).unwrap().remove(0);
        assert!(got.ok(), "{got:?}");
        assert_eq!(task(&b, n).state, State::Later);
        let n = blocked("queue-blocked");
        let got = crate::dispatch::answer_text(&b, &c, n, "token renewed, publish again", &op).unwrap().remove(0);
        assert_eq!(got.state, Some("review"), "free text still resumes publication: {got:?}");
    }

    // Chain: R1 hand-typed cancellation -> queue readback -> allow replacement; unknown never starts a new worker.
    #[test]
    fn hand_typed_cancellation_dequeues_before_another_attempt() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented cancellation", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        b.comment(n, "taskq: publish\naccepted").unwrap();
        b.comment(n, "taskq: enqueued MQ_1").unwrap();
        b.comment(n, "taskq: requeue main-moved").unwrap();
        b.state.borrow_mut().fail_publication_cancel = true;
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("replacement before cancellation readback")));
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        assert!(task(&b, n).attempt.is_none());
        b.state.borrow_mut().fail_publication_cancel = false;
        s.focus.clear();
        s.pass().unwrap();
        let before = b.state.borrow().cancelled_publications.len();
        s.pass().unwrap();
        assert_eq!(b.state.borrow().cancelled_publications.len(), before, "verified cancellation cached by its entry");
        assert_eq!(b.state.borrow().cancelled_publications.last(), Some(&(n, "a".repeat(40))));
        b.comment(n, "taskq: spawn worker codex:w2 other").unwrap();
        s.pass().unwrap();
        assert_eq!(b.state.borrow().cancelled_publications.len(), before, "a later revision does not repeat the cancellation");
    }

    // Defect #1535: a stale view of a re-produced SHA dequeued the newer publication (PR #1529 on #1470).
    #[test]
    fn cancellation_never_dequeues_a_newer_publication_of_the_same_sha() {
        let (b, c) = (Fake::default(), cfg("win"));
        let n = b.create("Invented republication", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        b.comment(n, "taskq: publish\naccepted").unwrap();
        b.comment(n, "taskq: requeue main-moved").unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w2 other").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "same SHA")).unwrap();
        // a restarted service reads the task in review before the new publish entry
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process")));
        s.pass().unwrap();
        b.comment(n, "taskq: publish\naccepted").unwrap();
        b.comment(n, "taskq: enqueued MQ_2").unwrap();
        s.pass().unwrap();
        assert!(b.state.borrow().cancelled_publications.is_empty(), "the newer publication stays queued");
    }

    // Chain: R1, § 2 Summary -> resumed publication close -> pass repair. Risk (#2017 A2, A5): the close copies the
    // publish verdict instead of naming it, the summary loses the verdict, or a stale or deleted summary stays wrong.
    #[test]
    fn close_names_its_publish_and_the_pass_repairs_the_summary() {
        let (b, c) = (Fake::default(), cfg("win"));
        b.state.borrow_mut().summaries = true;
        let n = b.create("Summary repair", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &["a".repeat(40).as_str()], Some(a), "Changed: x")).unwrap();
        let publish = b.comment(n, "taskq: publish\nChanged: the menu shows the summary\nOpen: none").unwrap();
        let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process")));
        s.pass().unwrap();
        let log = entries(&b, n).unwrap();
        let close = log.iter().rfind(|e| core::kind(n, &e.body) == "close").expect("publication resumed and closed");
        let text = core::entry_text(n, &close.body);
        assert!(text.starts_with(&format!("{}{publish}\n", core::REPEATS)), "{text}");
        assert!(!text.contains("Changed:"), "the close names the publish instead of copying it");
        assert!(core::verdict(n, &log, text).starts_with("Changed: the menu shows the summary\nOpen: none\n"), "readers show the publish text");
        let closed = |b: &Fake| b.comments(n).unwrap().into_iter().filter(crate::summary::is).collect::<Vec<_>>();
        s.pass().unwrap();
        let want = crate::summary::text(&b, n, "", &log, &core::fold(n, &log, false), None);
        assert!(want.starts_with(&format!("{}\n**Closed**", crate::summary::MARKER)) && want.contains("\nChanged: the menu shows the summary\nOpen: none"), "{want}");
        assert_eq!(closed(&b).iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), [want.as_str()], "the state change created it");
        // an open task: a stale summary is rewritten from the log, a deleted one recreated; an older task without one
        // keeps none until its state changes
        let summary = |n: u64| b.comments(n).unwrap().into_iter().filter(crate::summary::is).collect::<Vec<_>>();
        let (m, old) = (b.create("Parked", "", &["taskq".into()]).unwrap(), b.create("Older", "", &["taskq".into()]).unwrap());
        for k in [m, old] { b.comment(k, "taskq: later\nReason: Waits for the owner.").unwrap(); }
        let log = entries(&b, m).unwrap();
        crate::summary::write(&b, m, "", &b.comments(m).unwrap(), &log, &core::fold(m, &log, false), None).unwrap();
        let want = summary(m)[0].body.clone();
        assert_eq!(want, format!("{}\n**Later**\nReason: Waits for the owner.", crate::summary::MARKER));
        s.pass().unwrap();
        let id = summary(m)[0].id;
        b.state.borrow_mut().comments.iter_mut().find(|(_, c)| c.id == id).unwrap().1.body = format!("{}\n**Ready**", crate::summary::MARKER);
        s.pass().unwrap();
        assert_eq!(summary(m).iter().map(|c| (c.id, c.body.as_str())).collect::<Vec<_>>(), [(id, want.as_str())], "a stale summary is rewritten");
        b.state.borrow_mut().comments.retain(|(_, c)| c.id != id);
        s.pass().unwrap();
        assert_eq!(summary(m).iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), [want.as_str()], "a deleted summary is recreated");
        assert!(summary(old).is_empty(), "an older task gets its summary at its next state change");
        assert_eq!(core::fold(m, &entries(&b, m).unwrap(), false).state, State::Later);
    }

    type Exits = Rc<RefCell<Vec<(u64, Rc<RefCell<Option<i32>>>)>>>;

    // H-A5: a childless service can still own an uncertain native execution.
    #[test]
    fn project_drain_retains_unresolved_execution_until_release() {
        use std::fs;
        let b=Fake::default();
        let mut c=cfg("mac");
        c.root=std::env::temp_dir().join(format!("taskq-drain-{}",crate::runtime::uuid()));
        c.install=c.root.join("install");
        fs::create_dir_all(crate::dot(&c.root)).unwrap();
        let host=std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err("fixture holds admission".into()))));
        host.lock().unwrap().draining.insert(c.root.clone());
        let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:session mac").unwrap();
        let mut s=Service::new(&b,&c,"fixture",Box::new(|_|panic!("drain must never submit work")));
        s.host=Some(host.clone());
        b.state.borrow_mut().fail_comment_reads=1;
        assert!(crate::stopped_handoff(&mut s,&host,&c).is_err(),"no board observation is not an idle proof");
        s.pass().unwrap();
        assert!(s.kids.is_empty());
        assert!(s.retained_attempts().contains(&a));
        assert!(!crate::stopped_handoff(&mut s,&host,&c).unwrap(),"missing handle must retain the project and its slot");
        assert_eq!(host.lock().unwrap().busy.get(&c.root),Some(&true));
        b.labels(n,&[],&[TASK_LABEL.into()]).unwrap();s.pass().unwrap();
        assert!(s.retained_attempts().contains(&a),"label removal is not native termination");
        b.state.borrow_mut().comments.iter_mut().find(|(_,comment)|comment.id==a).unwrap().1.edited=true;
        s.pass().unwrap();assert!(!s.idle(),"missing execution history cannot release an observed execution");
        b.state.borrow_mut().comments.iter_mut().find(|(_,comment)|comment.id==a).unwrap().1.edited=false;
        b.close(n).unwrap();b.state.borrow_mut().fail_comment_reads=1;
        assert!(s.pass().is_err());
        assert!(s.retained_attempts().contains(&a));
        assert!(!crate::stopped_handoff(&mut s,&host,&c).unwrap(),"failed ownership read cannot certify idle");
        s.pass().unwrap();
        assert!(s.retained_attempts().contains(&a),"leaving the open-issue list is not native termination");
        assert!(!crate::stopped_handoff(&mut s,&host,&c).unwrap());
        release_execution(&b,n,a,a,"exit").unwrap();
        s.tasks_read=Some(std::time::Instant::now()-std::time::Duration::from_secs(c.poll.max(1)));
        assert!(crate::stopped_handoff(&mut s,&host,&c).unwrap(),"read-only monitor must observe terminal delivery and complete drain");
        let n=b.create("Recovered","",&[TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:recovered mac").unwrap();
        s.pass().unwrap();assert!(s.retained_attempts().contains(&a));
        let exit=Rc::new(RefCell::new(None));
        s.adopt(Kid { unknown:None, stopping:false, session_url:None,execution:a,task:n,attempt:a,rt:"codex".into(),proc:Box::new(FakeProc(exit.clone()))}).unwrap();
        assert!(!crate::stopped_handoff(&mut s,&host,&c).unwrap());
        *exit.borrow_mut()=Some(0);
        assert!(crate::stopped_handoff(&mut s,&host,&c).unwrap(),"reduced monitoring must retire the recovered reservation after exact release");
        b.state.borrow_mut().fail_comments=1;
        s.ownership_known=false;
        assert!(crate::stopped_handoff(&mut s,&host,&c).unwrap(),"read-only recovery can prepare without resuming ordinary board writes");
        assert_eq!(b.state.borrow().fail_comments,1,"recovery must not try an ordinary task write");
        drop(s);fs::remove_dir_all(&c.root).unwrap();
    }

    fn service<'a>(b: &'a Fake, c: &'a Config, exits: Exits) -> Service<'a> {
        let mut s = Service::new(b, c, "4.0.0", Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            exits.borrow_mut().push((l.task, flag.clone()));
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        s
    }

    fn task(b: &Fake, n: u64) -> Task {
        core::fold(n, &entries(b, n).unwrap(), false)
    }

    /// A project root holding the attempt log a start writes before its native call: a started execution's evidence (#1879).
    fn started_root(tag: &str, n: u64, a: u64) -> PathBuf {
        let root = std::env::temp_dir().join(format!("taskq-{tag}-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        std::fs::write(crate::runtime::attempt_log(&root, "worker", n, a), "").unwrap();
        root
    }

    // Chain: § 1 held, a person adds `taskq` on the board. Risk: the label path leaves a held log under a `taskq` label,
    // so the task never starts, or a plain labelled issue gets a stray `start`.
    #[test]
    fn a_person_adding_taskq_to_a_held_issue_starts_it() {
        let b = Fake::default(); let mut c = cfg("mac"); c.limits.clear();
        let held = b.create("H", "", &["q-held".into()]).unwrap();
        b.comment(held, "taskq: hold").unwrap();
        let plain = b.create("P", "", &[TASK_LABEL.into()]).unwrap();
        b.labels(held, &[TASK_LABEL.into()], &[]).unwrap();
        assert_eq!(task(&b, held).state, State::Held);
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap(); s.pass().unwrap();
        assert_eq!(task(&b, held).state, State::Ready);
        assert_eq!(entries(&b, held).unwrap().iter().filter(|e| e.body.starts_with("taskq: start")).count(), 1, "one start over two passes");
        assert!(!b.issue(held).unwrap().labels.contains(&"q-held".into()));
        assert!(entries(&b, plain).unwrap().iter().all(|e| !e.body.starts_with("taskq: start")));
        assert_eq!(task(&b, plain).state, State::Ready);
    }

    // #1435: a stale childless execution held the Windows image exit forever. Risk kept: handover and stop still wait on it.
    #[test]
    fn windows_exit_passes_childless_unresolved_execution() {
        let b=Fake::default();
        let c=cfg("mac");
        let n=b.create("T","## Goal\ng",&[TASK_LABEL.into()]).unwrap();
        let mut old=service(&b,&c,Default::default()); old.pass().unwrap();
        assert_eq!(old.kids.len(),1);
        drop(old);
        let mut s=service(&b,&c,Default::default()); s.pass().unwrap();
        assert!(s.kids.is_empty());
        assert!(s.execution_waits[&n].contains("termination unconfirmed"));
        // #1594 A3 (#947): the published heartbeat row and the handover wait read the same childless verdict
        let row=&s.execution_statuses()[0];
        assert_eq!((row.state.as_str(),row.reason.as_str()),("unknown",s.execution_waits[&n].as_str()));
        assert!(s.handoff_waits(true,false).iter().any(|w|w.contains("unresolved execution")),"handover keeps waiting");
        assert!(s.handoff_waits(true,true).is_empty(),"image exit leaves the record to the next image: {:?}",s.handoff_waits(true,true));
        s.pending.insert((n,1));
        assert!(s.handoff_waits(true,true).is_empty(),"#1601: a childless pending attempt passes to the next image");
        assert!(!s.handoff_waits(false,false).is_empty(),"outside a handover it still waits");
        s.pending.remove(&(n,1));
        if cfg!(windows) {assert!(crate::lifecycle_waits(&s,false).is_empty());}
        assert!(!crate::lifecycle_waits(&s,true).is_empty(),"stop still waits");
    }

    // Chain: #1359 § 3 claim -> launch -> acceptance, one pass. Risk: a claim rereads the task log the pass just read.
    #[test]
    fn one_claim_reads_its_task_log_once() {
        let (b, c) = (Fake::default(), cfg("mac"));
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        s.pass().unwrap();
        assert_eq!(s.kids.len(), 1);
        // was 6: views, pre-spawn, spawn readback, launch, acceptance before and after; the two readbacks stay (§ 3)
        assert_eq!(b.state.borrow().comment_reads, 3, "views, spawn readback, acceptance readback");
        assert_eq!(task(&b, n).state, State::Doing);
    }

    // Chain: #1359 § 7 waiting publication -> service pass on the owning and another host. Risk: every pass rereads
    // the issue and log that `views` just read, and every host polls the same publication.
    #[test]
    fn a_waiting_publication_reads_nothing_twice_and_only_its_host_polls() {
        let b = Fake::default();
        let n = b.create("Invented publication", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w win").unwrap();
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        let r = b.comment(n, "taskq: spawn reviewer codex:r win").unwrap();
        b.comment(n, &core::entry("publish", &[], Some(r), "Changed: x\nChecks: A1 ok\nOpen: none")).unwrap();
        b.state.borrow_mut().publish = "queue-waiting".into();
        let (mut win, mut mac) = (cfg("win"), cfg("mac"));
        // A shared /nonexistent can contain other fixtures' profile rows, which add unrelated board reads.
        let install=std::env::temp_dir().join(format!("taskq-publication-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(&install).unwrap();
        win.install=install.clone();mac.install=install.clone();win.root=install.join("win");mac.root=install.join("mac");
        let mut owner = Service::new(&b, &win, "test", Box::new(|_| panic!("no process while queued")));
        let mut other = Service::new(&b, &mac, "test", Box::new(|_| panic!("no process while queued")));
        let count = |b: &Fake| { let s = b.state.borrow(); (s.reads, s.comment_reads, s.publications) };
        let mut passes = vec![];
        for round in 0..3 {
            // a log read within SETTLE of its last write is read once more on a later pass (#1718)
            if round > 0 { b.advance(SETTLE); }
            for s in [&mut owner, &mut other] {
                let before = count(&b);
                s.pass().unwrap();
                let after = count(&b);
                passes.push((after.0 - before.0, after.1 - before.1, after.2 - before.2));
            }
        }
        // (issue reads, comment reads, publication polls) per pass, owner then other; was (1, 3, 1) on both hosts;
        // the second round still reads the log its first round read within SETTLE, the settled third reads none
        assert_eq!(passes, [(0, 1, 1), (0, 1, 0), (0, 1, 1), (0, 1, 0), (0, 0, 1), (0, 0, 0)]);
        // a stale owner heartbeat hands the poll to any host
        b.state.borrow_mut().clock += win.ttl + 1;
        let before = count(&b).2;
        other.pass().unwrap();
        assert_eq!(count(&b).2 - before, 1, "no host polls a publication whose owner is gone");
        std::fs::remove_dir_all(install).unwrap();
        assert_eq!(task(&b, n).state, State::Review);
    }

    // Chain: §5/R12 board reply files -> verified copies -> native start. Risk: missing files acknowledge the inbox or launch a partial reply.
    #[test]
    fn reply_files_hold_submission_until_every_attachment_is_readable() {
        let root=std::env::temp_dir().join(format!("taskq-reply-launch-{}",crate::runtime::uuid()));std::fs::create_dir(&root).unwrap();
        let mut c=cfg("mac");c.root=root.clone();c.install=root.clone();
        let b=Fake::default();let n=b.create("Attached reply","",&[TASK_LABEL.into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker claude:session mac").unwrap();
        accept_execution(&b,n,a,a,None).unwrap();release_execution(&b,n,a,a,"exit").unwrap();
        let original=root.join("document.bin");std::fs::write(&original,b"invented file bytes").unwrap();
        let r=crate::dispatch::act_files(&b,&root,n,"steer","Read the file",&crate::dispatch::Op::default(),&[original.display().to_string()]);assert!(r.ok(),"{}",r.text());
        let log=entries(&b,n).unwrap();let through=log.last().unwrap().id;
        let calls=Rc::new(RefCell::new(Vec::new()));let seen=calls.clone();
        let mut s=Service::new(&b,&c,"test",Box::new(move |l|{seen.borrow_mut().push(l.files.clone());Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))) as Box<dyn Proc>)}));
        let l=Launch{files:String::new(),execution:None,task:n,title:String::new(),attempt:a,role:"worker".into(),rt:"claude".into(),session:"session".into(),url:b.url(n),resume:Some("Read the file".into())};
        b.state.borrow_mut().bad_image=true;
        let before=core::executions(n,&log);
        s.launch(l,through,log).unwrap();
        assert!(calls.borrow().is_empty());assert_eq!(s.delivered(a),0);assert_eq!(s.held.len(),1);assert_eq!(core::executions(n,&entries(&b,n).unwrap()),before,"failed file delivery cannot create execution intent");
        b.state.borrow_mut().bad_image=false;
        let l=s.held.remove(0);s.launch(l,through,entries(&b,n).unwrap()).unwrap();
        assert_eq!(calls.borrow().len(),1);assert!(calls.borrow()[0].contains("untrusted data"));assert!(calls.borrow()[0].contains("document.bin"));assert_eq!(s.delivered(a),through);
        drop(s);std::fs::remove_dir_all(root).unwrap();
    }

    // R12 / §3: a last-moment admission hold is not a failed native submission.
    #[test]
    fn admission_wait_preserves_worker_reviewer_and_resume_execution() {
        for role in ["worker", "reviewer", "resume"] {
            let b=Fake::default();let c=cfg("mac");
            let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
            let w=b.comment(n,"taskq: spawn worker codex:session mac").unwrap();
            let a=if role=="reviewer" {
                accept_execution(&b,n,w,w,None).unwrap();release_execution(&b,n,w,w,"exit").unwrap();
                b.comment(n,&format!("taskq: result {} attempt {w}","a".repeat(40))).unwrap();
                b.comment(n,"taskq: spawn reviewer codex:session mac").unwrap()
            } else {w};
            let through=if role=="resume" {
                accept_execution(&b,n,a,a,None).unwrap();release_execution(&b,n,a,a,"exit").unwrap();
                b.comment(n,"taskq: steer\ncontinue").unwrap()
            } else {a};
            let calls=Rc::new(RefCell::new(0));let called=calls.clone();
            let mut s=Service::new(&b,&c,"test",Box::new(move |_| {
                *called.borrow_mut()+=1;
                if *called.borrow()==1 {Err(crate::runtime::StartError::admission("native inventory pending"))}
                else {Ok(Box::new(FakeProc(Rc::new(RefCell::new(None)))) as Box<dyn Proc>)}
            }));
            let l=Launch {files: String::new(), execution:None,task:n,title:"T".into(),attempt:a,role:if role=="reviewer" {"reviewer"} else {"worker"}.into(),rt:"codex".into(),session:"session".into(),url:b.url(n),resume:(role=="resume").then(||"continue".into())};
            s.launch(l,through,entries(&b,n).unwrap()).unwrap();
            assert_eq!(task(&b,n).attempt,Some(a),"{role}: admission consumed the attempt");
            assert_eq!(task(&b,n).retries,0);
            assert_eq!(s.held.len(),1,"{role}: pending launch lost");
            assert!(s.kids.is_empty());assert_eq!(s.delivered(a),0);
            let before=core::executions(n,&entries(&b,n).unwrap());
            assert_eq!(before.last().unwrap().release,None);
            let held=s.held.remove(0);s.launch(held,a,entries(&b,n).unwrap()).unwrap();
            let after=core::executions(n,&entries(&b,n).unwrap());
            assert_eq!(after.len(),before.len(),"admission retry duplicated execution intent");
            assert_eq!(s.kids.len(),1);assert_eq!(s.delivered(a),through);
            assert_eq!(*calls.borrow(),2);assert_eq!(task(&b,n).retries,0);
        }
    }

    // Chain: Resume process -> project wakeup -> separate scheduler process -> durable admission evidence.
    // Board and process adapters are fakes; real command functions, scheduler and shared collector run unchanged.
    #[test]
    fn resume_handoff_links_the_service_decision() {
        let dir = std::env::temp_dir().join(format!("qh-{}", std::process::id()));
        for held in [true, false] {
            std::fs::create_dir_all(dir.join(".taskq")).unwrap();
            std::fs::create_dir_all(dir.join("i")).unwrap();
            std::fs::write(dir.join(".taskq/focus"), "off").unwrap();
            std::fs::write(dir.join(".taskq/focus.paused"), "all").unwrap();
            if held { std::fs::write(dir.join("i/hold"), "private owner reason").unwrap(); }
            for mode in ["resume", "service"] {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "service::tests::resume_handoff_child", "--ignored"])
                    .env("TASKQ_HANDOFF_ROOT", &dir).env("TASKQ_HANDOFF_MODE", mode)
                    .env_remove("COREQ_TRACE_CONTEXT")
                    .env("TASKQ_INSTALL", dir.join("i")).env("TASKQ_ROOT", &dir).env("TASKQ_INCIDENT_REPORTS", "off")
                    .output().unwrap();
                assert!(child.status.success(), "{mode}: {}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
            }
            let reader = coreq::telemetry::Reader::open(&dir.join("i/telemetry.log/universal.sqlite3")).unwrap();
            let mut observations = Vec::new(); let mut after = 0;
            loop {
                let page = reader.export(0, after, 1024).unwrap();
                let rows = page["events"].as_array().unwrap();
                if rows.is_empty() { break; }
                let next = rows.last().unwrap()["seq"].as_i64().unwrap();
                assert!(next > after, "export cursor did not advance");
                after = next;
                observations.extend(rows.iter().map(|r| r["observation"].clone()));
            }
            let terminal = |name: &str| observations.iter().find(|r| r["operation"] == name && r["kind"] == "method").unwrap();
            let command = terminal("taskq.resume");
            let poke = terminal("taskq.poke");
            let write = terminal("taskq.operation.wake.write");
            let wake = terminal("taskq.continuation");
            assert_eq!(poke["trace"], command["trace"]);
            assert_eq!(write["trace"], command["trace"]);
            assert_eq!(write["parent"], poke["span"]);
            assert_eq!(write["outcome"], "ok");
            assert_eq!(wake["trace"], command["trace"]);
            assert_eq!(wake["parent"], write["span"]);
            assert_eq!(terminal("taskq.service.Service.pass")["trace"], command["trace"]);
            if held {
                assert!(observations.iter().any(|r| r["operation"] == "taskq.service.Service.may_launch" && r["kind"] == "method"
                    && r["trace"] == command["trace"] && r["outcome"] == "refused" && r["error"] == "hold"));
                assert!(!observations.iter().any(|r| r["operation"] == "taskq.service.Service.launch"));
                assert_eq!(std::fs::read_to_string(dir.join("i/hold")).unwrap(), "private owner reason");
            } else {
                let launch = terminal("taskq.service.Service.launch");
                assert_eq!(launch["trace"], command["trace"]);
                assert_eq!(launch["outcome"], "ok");
            }
            if !held {
                assert!(observations.iter().any(|r|r["kind"]=="stage"&&r["operation"]=="start"&&r["trace"]==command["trace"]));
                assert!(observations.iter().any(|r|r["kind"]=="stage"&&r["open"]==true&&r["version"]=="fixture"));
            }
            assert!(!dir.join("i/telemetry.log/telemetry.sqlite3").exists(),"scheduler must not open a second live sink");
            let text = serde_json::to_string(&observations).unwrap();
            assert!(!text.contains("private owner reason") && !text.contains(dir.to_str().unwrap()));
            drop(reader);
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    #[ignore = "subprocess helper exercised by resume_handoff_links_the_service_decision"]
    fn resume_handoff_child() {
        let dir = PathBuf::from(std::env::var_os("TASKQ_HANDOFF_ROOT").unwrap());
        crate::profile::init(&dir.join("i"));
        let _runtime = coreq::telemetry::capture::Runtime::install_bounded(&dir.join("i/telemetry.log/universal.sqlite3"), "taskq", "fixture",crate::telemetry::budget(&dir.join("i"),"telemetry.sqlite3").unwrap()).unwrap();
        if std::env::var("TASKQ_HANDOFF_MODE").unwrap() == "resume" {
            crate::resume(&dir).unwrap();
        } else {
            let mut seen = None;
            let link = crate::wake_context(&dir.join(".taskq/poke"), &mut seen).unwrap().unwrap();
            assert!(crate::wake_context(&dir.join(".taskq/poke"), &mut seen).unwrap().is_none());
            let _wakeup = link.span().entered();
            let mut c = cfg("fixture"); (c.root, c.install) = (dir.clone(), dir.join("i"));
            let b = Fake::default(); let n = b.create("Wake fixture", "", &["taskq".into()]).unwrap();
            let e: Exits = Default::default(); let mut s = service(&b, &c, e.clone());
            s.focus = crate::data::focus(&dir).unwrap(); s.pass().unwrap();
            assert_eq!(task(&b, n).state, if c.install.join("hold").exists() { State::Ready } else { State::Doing });
            assert_eq!(e.borrow().len(), usize::from(!c.install.join("hold").exists()));
        }
    }

    // Chain: Resume during a pass -> wake -> fresh admission; a host hold still refuses the launch.
    // Risk: a poke received during work becomes the sleep baseline, delaying Resume by another poll.
    #[test]
    fn resume_during_pass_wakes_the_next_admission_check() {
        crate::with_test_capture(|_, _| {
            let dir = std::env::temp_dir().join(format!("qr-wake-{}", std::process::id()));
            std::fs::create_dir_all(dir.join(".taskq")).unwrap();
            let mut c = cfg("fixture");
            (c.root, c.install) = (dir.clone(), dir.join("i"));
            std::fs::create_dir_all(&c.install).unwrap();
            let b = Fake::default();
            let n = b.create("Wake fixture", "", &["taskq".into()]).unwrap();
            let e: Exits = Default::default();
            let mut s = service(&b, &c, e.clone());
            std::fs::write(c.install.join("hold"), "owner-held fixture").unwrap();
            for held in [true, false] {
                std::fs::write(dir.join(".taskq/focus"), "all").unwrap();
                crate::pause(&dir,false).unwrap();
                std::fs::remove_file(dir.join(".taskq/poke")).unwrap();
                let pokes = [dir.join(".taskq/poke"), c.install.join("poke")];
                let poke_start = crate::poke_times(&pokes);
                s.focus = crate::data::focus(&dir).unwrap();
                s.pass().unwrap();
                assert_eq!(task(&b, n).state, State::Ready);
                crate::resume(&dir).unwrap();
                let mut ticks = 0;
                crate::sleep_until_poke(&pokes, poke_start, std::time::Instant::now()+std::time::Duration::from_secs(1), &mut || ticks += 1);
                assert_eq!(ticks, 0, "Resume during the pass must skip the next poll wait");
                s.focus = crate::data::focus(&dir).unwrap();
                s.pass().unwrap();
                if held {
                    assert_eq!(task(&b, n).state, State::Ready);
                    assert!(e.borrow().is_empty());
                    assert_eq!(s.capacity_json()["reasons"][0]["code"], "hold");
                    assert_eq!(std::fs::read_to_string(c.install.join("hold")).unwrap(), "owner-held fixture");
                    std::fs::remove_file(c.install.join("hold")).unwrap();
                } else {
                    assert_eq!(task(&b, n).state, State::Doing);
                    assert_eq!(e.borrow().len(), 1);
                }
            }
            let pokes = [dir.join(".taskq/poke"), c.install.join("poke")];
            let poke_start = crate::poke_times(&pokes);
            crate::touch(&pokes[1]);
            assert!(crate::sleep_until_poke(&pokes, poke_start, std::time::Instant::now()+std::time::Duration::from_secs(1), &mut || panic!("host poke during a pass was lost")), "a poke reports itself");
            assert!(!crate::sleep_until_poke(&pokes, crate::poke_times(&pokes), std::time::Instant::now(), &mut || {}), "a timeout is not a poke");
            // A poke during sleep is observed after the current tick, without waiting another second.
            let poke_start = crate::poke_times(&pokes);
            let mut ticks = 0;
            crate::sleep_until_poke(&pokes, poke_start, std::time::Instant::now()+std::time::Duration::from_secs(2), &mut || {
                ticks += 1;
                std::fs::remove_file(&pokes[1]).unwrap();
            });
            assert_eq!(ticks, 1);
            drop(s);
            std::fs::remove_dir_all(dir).unwrap();
        });
    }

    // Chain #1431 A2: board snapshot, children and pokes -> § 4 step 8 wait.
    // Risk: an idle board polls at full rate, or a ready/live task or a local command waits 3×poll.
    #[test]
    fn idle_passes_wait_three_polls_until_work_or_a_poke() {
        let c = cfg("mac");
        let b = Fake::default();
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        assert_eq!(crate::poll_wait(60, true, false, false, None), 60, "no board read yet is not idle");
        s.pass().unwrap();
        let idle = s.snapshot.as_ref();
        assert_eq!(crate::poll_wait(60, true, false, false, idle), 180);
        assert_eq!(crate::poll_wait(60, false, false, false, idle), 60, "a failed pass keeps the normal interval");
        assert_eq!(crate::poll_wait(60, true, true, false, idle), 60, "a running child keeps the normal interval");
        assert_eq!(crate::poll_wait(60, true, false, true, idle), 60, "a poke restores the normal interval");
        let n = b.create("Pending", "", &[TASK_LABEL.into()]).unwrap();
        s.pass().unwrap();
        let snap = s.snapshot.as_mut().unwrap();
        assert!(snap.tasks.iter().any(|r| r.n == n));
        for state in [State::Ready, State::Doing, State::Review, State::Ask] {
            snap.tasks.iter_mut().for_each(|r| r.task.state = state);
            assert_eq!(crate::poll_wait(60, true, false, false, Some(&*snap)), if state == State::Ask {180} else {60}, "{state:?} task");
        }
    }

    // Chain A2: project-thread unwind -> rebuilt board cache -> quarantine with live lease, beside healthy admission.
    // Risk: a panic drops a child/cursor, stops the other project or collides on equal task/attempt numbers.
    #[test]
    fn host_project_panic_preserves_children_and_other_project_admission() {
        use std::sync::{Arc, Mutex, Barrier};
        let policy = crate::host::Policy::parse(json!({"limits": {"claude": 2}, "admission": {"max": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let host = Arc::new(Mutex::new(crate::host::Host::new(Ok(policy.clone()))));
        let barrier = Arc::new(Barrier::new(2));
        let joins: Vec<_> = [true, false].into_iter().map(|panicking| {
            let (host, barrier, policy) = (host.clone(), barrier.clone(), policy.clone());
            std::thread::spawn(move || {
                let b = Fake::default(); let n = b.create("Same number", "", &["taskq".into()]).unwrap();
                let mut c = cfg("invented"); c.root = PathBuf::from(if panicking { "/invented/panic" } else { "/invented/healthy" });
                let exits: Exits = Default::default(); let mut s = service(&b, &c, exits.clone());
                // Seed the panicking project's existing child without consuming a new host launch interval.
                if panicking { s.pass().unwrap(); s.delivered.insert(s.kids[0].attempt, 99); }
                s.host = Some(host.clone()); s.reload(Ok(policy.cap.clone())); s.sync_host();
                barrier.wait();
                if !panicking { s.pass().unwrap(); }
                assert_eq!(s.kids.len(), 1);
                let a = s.kids[0].attempt;
                let mut panics = crate::host::Panics::default();
                for now in [0, 1, 11] {
                    if panicking {
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            s.pass().unwrap();
                            panic!("invented pass panic");
                        }));
                        assert!(r.is_err()); panics.record(now, "invented pass panic"); s.rebuild();
                        s.reduced().unwrap();
                        assert_eq!(s.delivered(a), 99, "the inbox cursor survives every rebuild");
                    } else { s.pass().unwrap(); }
                    assert_eq!(s.kids.len(), 1); assert_eq!(*exits.borrow()[0].1.borrow(), None);
                }
                if panicking {
                    assert!(panics.quarantined()); assert_eq!(panics.reason.as_deref(), Some("quarantined: panic:other"));
                    let before = b.comments(n).unwrap().len();
                    b.advance(900); s.mono = 900;
                    s.reduced().unwrap(); assert_eq!(b.comments(n).unwrap().len(), before, "quarantine writes no task transitions");
                }
                let hb = b.issues(None).unwrap().into_iter().find(is_host).unwrap();
                assert!(hb.body.lines().any(|l| l == format!("#{n} {a}")), "child lease remains listed");
                assert!(b.now().unwrap() - hb.updated < c.ttl, "quarantine refreshes the lease beyond ttl using board time");
                (n, a, s.kids.len())
            })
        }).collect();
        let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
        assert_eq!(results[0], results[1], "equal issue and attempt numbers across projects stay distinct");
        assert_eq!(host.lock().unwrap().sync(std::path::Path::new("unused"), vec![]), 2);
    }

    // A2: scoped Uninstall drains one project while another retains work and admits its next job.
    #[test]
    fn project_uninstall_preserves_other_project_work_and_admission() {
        let base=std::env::temp_dir().join(format!("taskq-project-uninstall-{}",crate::runtime::uuid()));
        let policy=crate::host::Policy::parse(json!({"limits":{"claude":2},"load_gate":{"enabled":false}}),"invented").unwrap();
        let host=std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(policy.clone()))));
        let (ba,bb)=(Fake::default(),Fake::default());let exits=Exits::default();
        let mut ca=cfg("invented");ca.root=base.join("a");ca.install=base.clone();let mut cb=ca.clone();cb.root=base.join("b");
        for root in [&ca.root,&cb.root] {std::fs::create_dir_all(crate::dot(root)).unwrap();}
        let (mut sa,mut sb)=(service(&ba,&ca,exits.clone()),service(&bb,&cb,exits.clone()));
        bb.create("Already running","",&[TASK_LABEL.into()]).unwrap();sb.pass().unwrap();
        let attempt=sb.kids[0].attempt;
        for s in [&mut sa,&mut sb] {s.host=Some(host.clone());s.reload(Ok(policy.cap.clone()));s.sync_host();}
        host.lock().unwrap().draining.insert(ca.root.clone());
        let held=ba.create("Keep queued","",&[TASK_LABEL.into()]).unwrap();sa.pass().unwrap();sa.sync_host();
        assert!(sa.kids.is_empty()&&crate::drained(&sa,&host.lock().unwrap()));
        host.lock().unwrap().ready(&ca.root,false);
        let next=bb.create("Next job","",&[TASK_LABEL.into()]).unwrap();sb.pass().unwrap();sb.sync_host();
        assert_eq!(sb.kids.len(),2,"unrelated admission stays open: {:?}",sb.waits);
        assert!(sb.kids.iter().any(|kid|kid.attempt==attempt),"the existing job keeps its attempt");
        assert!(exits.borrow().iter().all(|(_,exit)|exit.borrow().is_none()),"no job was stopped");
        assert!(entries(&ba,held).unwrap().iter().all(|entry|!entry.body.starts_with("taskq: spawn")));
        assert!(entries(&bb,next).unwrap().iter().any(|entry|entry.body.starts_with("taskq: spawn")));
        drop((sa,sb));std::fs::remove_dir_all(base).unwrap();
    }

    // Chain: § 3 poisoned authority -> each project's retained handles -> verified slot rebuild.
    // Risk: clearing poison early drops another project's occupancy and allows a launch above the host limit.
    #[test]
    fn host_poison_holds_until_every_project_reconciles_owned_handles() {
        let install = std::env::temp_dir().join(format!("taskq-host-poison-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        let raw = json!({"limits": {"claude": 2}, "admission": {"max": 2}, "load_gate": {"enabled": false}});
        std::fs::write(install.join("host.json"), raw.to_string()).unwrap();
        let p = crate::host::Policy::parse(raw, "invented").unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
        let (a, b) = (Fake::default(), Fake::default());
        let mut ca = cfg("invented"); ca.root = install.join("a"); ca.install = install.clone();
        let mut cb = ca.clone(); cb.root = install.join("b");
        let mut sa = service(&a, &ca, Default::default()); let mut sb = service(&b, &cb, Default::default());
        for s in [&mut sa, &mut sb] {
            s.host = Some(host.clone()); s.reload(Ok(p.cap.clone()));
            s.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution: 7, task: 1, attempt: 7, rt: "claude".into(), proc: Box::new(FakeProc(Default::default())) });
            host.lock().unwrap().busy.insert(s.cfg.root.clone(), true);
            s.sync_host();
        }
        let poisoned = host.clone();
        assert!(std::thread::spawn(move || { let _guard = poisoned.lock().unwrap(); panic!("invented admission panic"); }).join().is_err());
        sa.sync_host(); assert!(host.is_poisoned()); assert!(!sa.reserve(2, 8, "claude"));
        sb.sync_host(); assert!(!host.is_poisoned());
        assert_eq!(host.lock().unwrap().sync(std::path::Path::new("unused"), vec![]), 2);
        assert_eq!((sa.kids.len(), sb.kids.len()), (1, 1));
        std::fs::remove_dir_all(install).unwrap();
    }

    // Chain A2: unavailable project leaves round-robin -> multiple healthy grants; failed reduced loop -> safe update.
    // Risk: stale readiness starves later work, or a stopped project can never satisfy the host handoff barrier.
    #[test]
    fn host_quarantine_does_not_starve_and_stopped_handles_can_prepare_restart() {
        let root = std::env::temp_dir().join(format!("taskq-host-stopped-{}", std::process::id()));
        std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let p = crate::host::Policy::parse(json!({"limits": {"claude": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
        let b = Fake::default(); let mut c = cfg("invented"); c.root = root.clone(); c.install = root.clone();
        let mut s = service(&b, &c, Default::default()); s.host = Some(host.clone());
        let healthy = root.join("healthy");
        { let mut h = host.lock().unwrap(); h.ready(&root, true); h.ready(&healthy, true); }
        s.rebuild(); s.reduced().unwrap();
        for n in 1..=3 {
            let mut h = host.lock().unwrap();
            h.reserve(&healthy, n, n, "claude", &p.cap, true, (n - 1) * 30).unwrap();
            h.sync(&healthy, vec![]);
        }
        let n = b.create("Owned", "", &[TASK_LABEL.into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker claude:fixture invented").unwrap();
        let exit = Rc::new(RefCell::new(None));
        s.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution: a, task: n, attempt: a, rt: "claude".into(), proc: Box::new(FakeProc(exit.clone())) });
        s.pass().unwrap();s.rebuild();
        { let mut h = host.lock().unwrap(); h.busy.insert(root.clone(), true); h.restart = true; }
        // This is the monitor entered after a reduced-loop panic: no board pass and no lease renewal.
        assert!(!crate::stopped_handoff(&mut s, &host, &c).unwrap());
        assert!(!host.lock().unwrap().prepared.contains(&root)); assert_eq!(s.kids.len(), 1);
        *exit.borrow_mut() = Some(0);
        assert!(!crate::stopped_handoff(&mut s, &host, &c).unwrap());
        let h = host.lock().unwrap();
        assert!(h.busy.keys().all(|p| h.prepared.contains(p)), "coordinator can restart after verified exit");
        assert!(core::executions(n, &entries(&b,n).unwrap()).iter().all(|x| x.release.is_some()), "only confirmed release permits handoff");
        drop(h); std::fs::remove_dir_all(root).unwrap();
    }

    // #1402 A1: restart pending, a live claude kid without a native identity defers the handover; once it exits and
    // is released, the next pass reports prepared. Risk: a released worker keeps the host on the old image.
    #[test]
    fn released_kid_lets_the_next_pass_prepare_the_restart() {
        let root = std::env::temp_dir().join(format!("taskq-host-released-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let p = crate::host::Policy::parse(json!({"limits": {"claude": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p))));
        let b = Fake::default(); let mut c = cfg("invented"); c.root = root.clone(); c.install = root.clone();
        let mut s = service(&b, &c, Default::default()); s.host = Some(host.clone());
        let n = b.create("Owned", "", &[TASK_LABEL.into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker claude:fixture invented").unwrap();
        let exit = Rc::new(RefCell::new(None));
        s.kids.push(Kid { unknown:Some("native observation deferred; pass budget exhausted".into()), stopping:false, session_url:None, execution: a, task: n, attempt: a, rt: "claude".into(), proc: Box::new(FakeProc(exit.clone())) });
        { let mut h = host.lock().unwrap(); h.busy.insert(root.clone(), true); h.restart = true; }
        let project_pass = |s: &mut Service| {
            assert!(host.lock().unwrap().begin_pass(&root));
            s.pass().unwrap(); s.sync_host();
            let mut h = host.lock().unwrap();
            let drained = crate::drained(s, &h);
            h.busy.insert(root.clone(), !drained);
            crate::report_handoff(s, &mut h, &root, drained).unwrap();
            h.handoff_ready()
        };
        assert!(!project_pass(&mut s), "the live worker defers the handover");
        assert_eq!(crate::lifecycle_waits(&s, false), vec![format!("#{n}")]);
        *exit.borrow_mut() = Some(0);
        assert!(project_pass(&mut s), "the released worker no longer holds the restart");
        assert!(s.kids.is_empty() && host.lock().unwrap().prepared.contains(&root));
        assert!(core::executions(n, &entries(&b,n).unwrap()).iter().all(|x| x.release.is_some()));
        std::fs::remove_dir_all(root).unwrap();
    }

    // #1766 A1-A2: a queued Stop with a live worker in project A froze idle project B: the barrier (#1402) skipped
    // B's loop, so its status.json kept stale counters for the whole drain. Retained protection: the barrier still holds
    // B out of ordinary passes and keeps its prepared report, A keeps monitoring its worker, and neither claims.
    #[test]
    fn drain_with_live_worker_keeps_idle_project_status_fresh_and_claims_nothing() {
        let base = std::env::temp_dir().join(format!("taskq-drain-observe-{}", crate::runtime::uuid()));
        let p = crate::host::Policy::parse(json!({"limits": {"claude": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
        let (ba, bb) = (Fake::default(), Fake::default());
        let mut ca = cfg("invented"); ca.root = base.join("a"); ca.install = base.clone();
        let mut cb = ca.clone(); cb.root = base.join("b");
        for root in [&ca.root, &cb.root] { std::fs::create_dir_all(crate::dot(root)).unwrap(); }
        let exits = Exits::default();
        let (mut sa, mut sb) = (service(&ba, &ca, exits.clone()), service(&bb, &cb, exits.clone()));
        for s in [&mut sa, &mut sb] { s.host = Some(host.clone()); s.reload(Ok(p.cap.clone())); }
        let live = ba.create("Live", "", &[TASK_LABEL.into()]).unwrap();
        let a = ba.comment(live, "taskq: spawn worker claude:fixture invented").unwrap();
        sa.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution: a, task: live, attempt: a, rt: "claude".into(), proc: Box::new(FakeProc(Default::default())) });
        let ready = [(&ba, ba.create("Ready A", "## Goal\ng", &[TASK_LABEL.into()]).unwrap()), (&bb, bb.create("Ready B", "## Goal\ng", &[TASK_LABEL.into()]).unwrap())];
        { let mut h = host.lock().unwrap(); h.restart = true; h.stopping = true; for root in [&ca.root, &cb.root] { h.busy.insert(root.clone(), true); } }
        // One service-loop pass: the barrier's observe pass leaves the handover report untouched.
        let loop_pass = |s: &mut Service| -> bool {
            let observe = crate::observes(Some(&host), &s.cfg.root).unwrap();
            if observe { s.observe().unwrap(); return true; }
            s.pass().unwrap(); s.sync_host();
            let mut h = host.lock().unwrap();
            h.busy.insert(s.cfg.root.clone(), !s.idle());
            crate::report_handoff(s, &mut h, &s.cfg.root, s.idle()).unwrap();
            false
        };
        assert!(!loop_pass(&mut sa) && !loop_pass(&mut sb), "the first pass of each project reports its handover");
        assert!(host.lock().unwrap().prepared.contains(&cb.root) && !host.lock().unwrap().handoff_ready(), "A's live worker holds the Stop");
        for round in 0..3 {
            let arrived = bb.create(&format!("Arrived {round}"), "", &[TASK_LABEL.into()]).unwrap();
            sb.tasks_read = None;
            assert!(loop_pass(&mut sb), "the barrier keeps idle B out of ordinary passes");
            assert!(sb.tasks_read.is_some() && sb.snapshot.as_ref().unwrap().tasks.iter().any(|r| r.n == arrived), "B's status reads the board it observes now");
            assert!(!loop_pass(&mut sa), "A keeps monitoring its live worker");
            assert_eq!(sa.kids.len(), 1);
            assert!(host.lock().unwrap().prepared.contains(&cb.root), "an observe pass keeps B's prepared report");
        }
        assert!(exits.borrow().is_empty(), "the drain admits nothing in either project");
        for (b, n) in ready { assert!(entries(b, n).unwrap().iter().all(|e| !e.body.starts_with("taskq: spawn")), "#{n} stays unclaimed"); }
        { let mut h = host.lock().unwrap(); h.restart = false; h.stopping = false; }
        for _ in 0..2 { assert!(!loop_pass(&mut sa) && !loop_pass(&mut sb), "a cancelled Stop lifts the barrier"); }
        assert!(!exits.borrow().is_empty(), "the fixture claims once admission reopens, so the drain's refusal was real: {:?} {:?}", sa.waits, sb.waits);
        drop((sa, sb)); std::fs::remove_dir_all(base).unwrap();
    }

    // #1601 A4: a pending restart refused the resume of an answered task, and the handover waited on that refused
    // attempt ('#N unresolved ownership') forever. Risk kept: the resume is not started under the restart; the next
    // image still finds it on the board with the inbox undelivered.
    #[test]
    fn refused_resume_lets_the_restart_hand_over() {
        let root = std::env::temp_dir().join(format!("taskq-host-refused-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let p = crate::host::Policy::parse(json!({"limits": {"codex": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p))));
        let b = Fake::default(); let mut c = cfg("invented"); c.root = root.clone(); c.install = root.clone();
        let n = b.create("Answered", "", &[TASK_LABEL.into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:session invented").unwrap();
        accept_execution(&b,n,a,a,None).unwrap(); release_execution(&b,n,a,a,"exit").unwrap();
        b.comment(n, "taskq: steer\ncontinue").unwrap();
        let mut s = service(&b, &c, Default::default()); s.host = Some(host.clone());
        { let mut h = host.lock().unwrap(); h.busy.insert(root.clone(), true); h.restart = true; }
        assert!(handover_pass(&mut s, &host, &root), "one pass reaches the re-exec: {:?}", crate::lifecycle_waits(&s, false));
        assert!(s.kids.is_empty() && s.pending.contains(&(n,a)), "the restart refuses the resume");
        let t = task(&b, n);
        assert_eq!((t.state, t.attempt, s.delivered(a)), (State::Doing, Some(a), 0), "the inbox waits for the next image");
        std::fs::remove_dir_all(root).unwrap();
    }

    // #1769 A1: a drain Stop refused the resume of an answered task (its worker had asked and exited), then waited on
    // that refused attempt ('#N unresolved ownership') forever. Risk kept: nothing starts under the Stop; the task
    // stays Doing with its inbox undelivered, and the next generation resumes it.
    #[test]
    fn stop_drain_passes_a_released_attempt_to_the_next_start() {
        let root = std::env::temp_dir().join(format!("taskq-host-stop-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(crate::dot(&root)).unwrap();
        let p = crate::host::Policy::parse(json!({"limits": {"codex": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p.clone()))));
        let b = Fake::default(); let mut c = cfg("invented"); c.root = root.clone(); c.install = root.clone();
        let n = b.create("Answered", "", &[TASK_LABEL.into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:session invented").unwrap();
        accept_execution(&b,n,a,a,None).unwrap(); release_execution(&b,n,a,a,"exit").unwrap();
        b.comment(n, "taskq: steer\ncontinue").unwrap();
        let mut s = service(&b, &c, Default::default()); s.host = Some(host.clone());
        { let mut h = host.lock().unwrap(); h.busy.insert(root.clone(), true); h.restart = true; h.stopping = true; }
        assert!(handover_pass(&mut s, &host, &root), "the drain completes: {:?}", crate::lifecycle_waits(&s, true));
        assert!(s.kids.is_empty() && s.pending.contains(&(n,a)), "the Stop refuses the resume");
        assert_eq!((task(&b, n).state, s.delivered(a)), (State::Doing, 0), "the inbox waits for the next start");
        drop(s);
        let mut next = service(&b, &c, Default::default());
        next.host = Some(std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p)))));
        next.pass().unwrap();
        assert_eq!(next.kids.iter().map(|k| k.task).collect::<Vec<_>>(), vec![n], "the next generation resumes it");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// One project pass under the host lock protocol; true when the host may re-exec.
    fn handover_pass(s: &mut Service, host: &crate::host::Shared, root: &PathBuf) -> bool {
        assert!(host.lock().unwrap().begin_pass(root));
        s.pass().unwrap(); s.sync_host();
        let mut h = host.lock().unwrap();
        let drained = crate::drained(s, &h);
        h.busy.insert(root.clone(), !drained);
        crate::report_handoff(s, &mut h, root, drained).unwrap();
        h.handoff_ready()
    }

    // #1601 class guard: a pending restart refuses every reservation (`Host::reserve`), so no handover wait may need
    // one. Each attempt kind the refusal leaves childless is set up, the restart is requested, and one pass must
    // reach the re-exec. Kinds: a ready claim, an answered resume, a held claim, a held resume, a held reservation.
    // Live children, ownership reads and release writes resolve without a reservation and keep their own tests.
    #[test]
    fn every_handover_wait_resolves_without_a_reservation() {
        for kind in ["claim", "resume", "held claim", "held resume", "reserved"] {
            let root = std::env::temp_dir().join(format!("taskq-host-class-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(crate::dot(&root)).unwrap();
            let p = crate::host::Policy::parse(json!({"limits": {"codex": 2}, "load_gate": {"enabled": false}}), "invented").unwrap();
            let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Ok(p))));
            let b = Fake::default(); let mut c = cfg("invented"); c.root = root.clone(); c.install = root.clone();
            let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
            if kind.ends_with("resume") {
                let a = b.comment(n, "taskq: spawn worker codex:session invented").unwrap();
                accept_execution(&b,n,a,a,None).unwrap(); release_execution(&b,n,a,a,"exit").unwrap();
                b.comment(n, "taskq: steer\ncontinue").unwrap();
            }
            let mut s = Service::new(&b, &c, "test", Box::new(|_| Err(crate::runtime::StartError::admission("native inventory pending"))));
            s.focus = vec!["all".into()]; s.host = Some(host.clone());
            host.lock().unwrap().busy.insert(root.clone(), true);
            if kind.starts_with("held") {
                handover_pass(&mut s, &host, &root);
                assert_eq!(s.held.len(), 1, "{kind}: setup holds the launch");
            }
            if kind == "reserved" {s.pass().unwrap(); s.reserved.push((n, 99, "codex".into()));}
            host.lock().unwrap().restart = true;
            assert!(handover_pass(&mut s, &host, &root), "{kind}: one pass reaches the re-exec: {:?}", crate::lifecycle_waits(&s, false));
            assert!(s.kids.is_empty() && s.held.is_empty(), "{kind}: nothing starts under the restart");
            assert!(core::executions(n, &entries(&b,n).unwrap()).iter().all(|x| x.release.is_some()), "{kind}: no execution left open");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    // Chain: cross-host recovery. Risk: inbox age or an unlisted lease is mistaken for exit.
    #[test]
    fn cross_host_recovery_needs_exit_regardless_of_inbox_grace() {
        for (entry, trusted, edited, pending_input) in [
            ("taskq: answer 1", true, false, true),
            ("taskq: steer\ncontinue", true, false, true),
            ("taskq: steer attempt 999999\ncontinue", true, false, false),
            ("taskq: steer\ncontinue", false, false, false),
            ("taskq: steer\ncontinue", true, true, false),
            ("unrelated comment", true, false, false),
        ] {
            let b = Fake::default();
            let n = b.create("T", "", &["taskq".into()]).unwrap();
            let a = b.comment(n, &core::entry("spawn", &["worker", "claude:session", "win"], None, "")).unwrap();
            if entry.starts_with("taskq: answer") {
                b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. yes\n2. no")).unwrap();
            }
            b.advance(400);
            b.raw(n, entry, trusted, edited);
            let hb = b.create("taskq host win", "version 4.0.0", &[HOST_LABEL.into()]).unwrap();
            let c = cfg("mac");
            let mut s = service(&b, &c, Default::default());
            s.focus.clear();
            s.pass().unwrap();
            assert_eq!(task(&b, n).attempt, Some(a), "{entry}, trusted={trusted}, edited={edited}");
            b.advance(301);
            b.edit(hb, "version 4.0.0\nat fresh").unwrap();
            s.pass().unwrap();
            assert_eq!(task(&b, n).attempt, Some(a));
            release_execution(&b, n, a, a, "exit").unwrap();
            s.pass().unwrap();
            assert_eq!(task(&b, n).attempt, pending_input.then_some(a), "confirmed exit permits recovery but cannot discard pending input");
        }
    }

    // Chain: § 3 answered attempt across hosts. Risk: another host steals an answered attempt.
    #[test]
    fn answered_attempt_survives_other_host_until_owner_resumes() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let (win, mac) = (cfg("win"), cfg("mac"));
        let exits: Exits = Default::default();
        let mut owner = service(&b, &win, exits.clone());
        let mut other = service(&b, &mac, Default::default());
        owner.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. yes\n2. no")).unwrap();
        *exits.borrow()[0].1.borrow_mut() = Some(0);
        b.advance(400);
        owner.pass().unwrap();
        b.comment(n, &core::entry("answer", &["1"], None, "yes")).unwrap();
        other.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a));
        owner.pass().unwrap();
        assert_eq!((task(&b, n).attempt, exits.borrow().len()), (Some(a), 2));
        b.advance(301);
        other.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a), "the continuation remains owned after heartbeat expiry");
        assert_eq!(core::executions(n, &entries(&b,n).unwrap()).iter().filter(|x| x.release.is_none()).count(), 1);
    }

    // Chain: § 4 step 1 turn end, A1/A2 of #1034. Risk: a worker whose `taskq result` was rate-limited finishes its
    // turn, its process lingers, and the task shows `doing` for hours; or the `gone` hides the worker's last message.
    #[test]
    fn finished_turn_without_result_is_gone_with_last_message() {
        for paused in [false, true] {
            turn_end(paused);
        }
    }

    // Chain: service launch -> failed native checkpoint -> repair -> observation -> terminal receipt.
    // Risk: persistence failure loses a live handle or resubmits the task instead of saving its identity.
    #[cfg(unix)]
    #[test]
    fn native_checkpoint_failure_retains_execution_and_retries_only_storage() {
        let root=std::env::temp_dir().join(format!("taskq-checkpoint-{}",crate::runtime::uuid()));
        let install=root.join("install");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("native"),"blocked directory").unwrap();
        let b=Fake::default();
        let n=b.create("T","",&[TASK_LABEL.into(),"run-codex".into()]).unwrap();
        let c=Config {root:root.clone(),install:install.clone(),limits:BTreeMap::from([("codex".into(),1)]),..cfg("mac")};
        let count=Rc::new(RefCell::new(0)); let launched=count.clone();
        let mut s=Service::new(&b,&c,"fixture",Box::new(|l| {
            *launched.borrow_mut()+=1;
            let log=crate::runtime::log_path(&root,l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log,"").unwrap();
            let child=std::process::Command::new("sleep").arg("30").spawn().unwrap();
            let native=coreq::process::identity(child.id()).unwrap();
            Ok(Box::new(crate::runtime::CliTurn {proc:Box::new(child),log,pos:0,start:0,stopping:false,accepted:false,read_error:None,session:None,native,recovery:None}))
        }));
        s.focus=vec!["all".into()];
        s.pass().unwrap();
        let a=task(&b,n).attempt.unwrap();
        assert_eq!(s.kids.len(),1);
        s.reap_owned();
        assert!(s.kids.iter().find(|k|k.attempt==a).unwrap().unknown.as_deref().unwrap().contains("checkpoint failed"));
        let pid=s.kids[0].proc.id();
        assert!(coreq::process::identity(pid).unwrap().is_some());
        std::fs::remove_file(install.join("native")).unwrap();
        s.reap_owned();
        let rows=crate::kids_file(&install,&root).unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!((rows[0].task,rows[0].attempt,rows[0].execution,rows[0].process.pid),(n,a,a,pid));
        assert_eq!(*count.borrow(),1,"retry storage, never work");
        assert!(!s.kids.iter().any(|k|k.attempt==a && k.unknown.is_some()));
        let held=std::mem::take(&mut s.kids);
        let discarded=crate::save_kids(&s,&root);
        s.kids=held;
        assert!(discarded.unwrap_err().contains("unresolved"));
        assert_eq!(crate::kids_file(&install,&root).unwrap(),rows,"a missing handle cannot erase recovery evidence");
        // A second failed checkpoint must not prevent emergency cancellation.
        let l=Launch {files: String::new(), execution:None,task:n,title:String::new(),attempt:a,role:"worker".into(),rt:"codex".into(),session:"-".into(),url:String::new(),resume:None};
        let path=crate::kids_path(&install,&root).unwrap();
        std::fs::write(&path,"corrupt").unwrap();
        assert!(s.kids[0].proc.checkpoint(&c,&l,a).is_err());
        s.kids[0].proc.kill().unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(3);
        while !s.kids.is_empty() {
            s.reap_owned();
            assert!(std::time::Instant::now()<until,"stop must remain available during storage failure");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(*count.borrow(),1);
        assert_eq!(core::executions(n,&crate::board::entries(&b,n).unwrap())[0].release,Some(core::Release::Exit));
        assert_eq!(std::fs::read_to_string(path).unwrap(),"corrupt","failed recovery evidence is preserved");
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn turn_end(paused: bool) {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("taskq-turn-end-{paused}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let c = Config { root: root.clone(), ..cfg("win") };
        let flags: Exits = Default::default();
        let (seen, r) = (flags.clone(), root.clone());
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n").unwrap();
            Ok(Box::new(crate::runtime::CliTurn { proc: Box::new(FakeProc(flag)), log, pos: 0, start: 0, stopping: false, accepted: false, read_error: None, session: None, native: None, recovery: None }) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a), "a running turn stays doing");

        // the worker's result is rejected by the board rate limit, then its turn ends and its process lingers
        b.state.borrow_mut().fail_comments = 1;
        assert!(b.comment(n, &core::entry("result", &["d3e3479077ac2df8db8071b5bdf12cdacaf10ab4"], Some(a), "Changed: x")).is_err());
        let log = crate::runtime::attempt_log(&root, "worker", n, a);
        let mut f = std::fs::File::options().append(true).open(&log).unwrap();
        let text = format!("Task handoff is incomplete: GitHub rate limits rejected `taskq result`.\n\nSee [log]({}/.taskq/x.log).", root.display());
        writeln!(f, "{}", json!({"type": "item.completed", "item": {"type": "agent_message", "text": text}})).unwrap();
        writeln!(f, "{{\"type\":\"turn.completed\"}}").unwrap();
        if paused {
            // A failed release keeps the exited handle and retries only its evidence write.
            b.state.borrow_mut().fail_comments = 1;
            s.pass().unwrap();
            assert_eq!(s.kids.len(), 1);
            assert!(s.kids.iter().any(|k|k.attempt==a && k.unknown.is_some()));
        }
        s.pass().unwrap();
        assert_eq!(*flags.borrow()[0].1.borrow(), Some(-9), "the lingering process tree is stopped");
        let t = task(&b, n);
        let gone = b.comments(n).unwrap().into_iter().rfind(|c| c.body.starts_with("taskq: gone")).unwrap().body;
        assert_eq!((t.state, t.attempt, t.retries), (State::Ready, None, 1), "paused={paused}");
        let reason = "exit -9";
        assert!(gone.ends_with(&format!("\n{reason}\nLast message: Task handoff is incomplete: GitHub rate limits rejected `taskq result`. See [log](.taskq/x.log).")), "{gone}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: incident worker start. Risk: missing runtime or login reported as a fault.
    #[test]
    fn incident_start_requires_installed_logged_in_runtime_and_non_environment_failure() {
        use std::io::{Error, ErrorKind};
        for (path, login, kind, reports) in [(Some("fake"), "logged in", ErrorKind::Other, true),
            (None, "not installed", ErrorKind::Other, false), (Some("fake"), "not logged in", ErrorKind::Other, false),
            (Some("fake"), "unknown", ErrorKind::Other, false), (Some("fake"), "logged in", ErrorKind::OutOfMemory, false),
            (Some("fake"), "logged in", ErrorKind::PermissionDenied, false)] {
            crate::incident::take_detected();
            let b = Fake::default();
            let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
            let c = cfg("local");
            let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |_| Err(crate::runtime::StartError::io(Error::new(kind, "private failure text"), None))));
            s.agent_status = json!([{"runtime": "claude", "path": path, "login": login}]);
            let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300).unwrap().0.remove(0);
            s.claim(&v, "worker", "claude", "").unwrap();
            assert_eq!(task(&b, n).state, State::Ready);
            assert!(b.comments(n).unwrap().last().unwrap().body.contains("start failed: private failure text"));
            assert_eq!(crate::incident::take_detected(), if reports { vec!["dispatch.start"] } else { vec![] });
        }
    }

    // Chain: § 8 rule delivery cap -> § 2 ask, #1946 (pilotq#170). Risk: a deterministic cap refusal burns two gone
    // retries before the owner hears of it, or the answer resumes a session that never started.
    #[test]
    fn a_rules_cap_refusal_asks_once_and_its_answer_starts_a_fresh_attempt() {
        for role in ["worker", "reviewer"] {
            let b = Fake::default();
            let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
            let result = "a".repeat(40);
            if role == "reviewer" {
                let w = b.comment(n, "taskq: spawn worker claude:w other").unwrap();
                b.comment(n, &core::entry("result", &[&result], Some(w), "Changed: x\nOpen: none")).unwrap();
                release_execution(&b, n, w, w, "exit").unwrap();
            }
            let c = cfg("local");
            let starts = Rc::new(RefCell::new(0));
            let counted = starts.clone();
            let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |_| {
                *counted.borrow_mut() += 1;
                Err(crate::runtime::over_cap(std::path::Path::new("/nonexistent"), 128 * 1024 + 5))
            }));
            let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300).unwrap().0.remove(0);
            s.claim(&v, role, "claude", "").unwrap();
            let t = task(&b, n);
            let a = t.attempt.unwrap();
            assert_eq!((t.state, t.retries, *starts.borrow()), (State::Ask, 0, 1), "{role}: one start, no gone retry");
            let card = t.card.unwrap();
            assert!(card.lines().nth(1).unwrap().starts_with("Problem: start refused: rules bundle 131077 bytes, 5 over the 131072-byte cap; trim taskq.md below the cap"), "{card}");
            assert!(card.contains("1. retry after trimming - + a fresh attempt starts; - it is refused again until the rules fit the cap ★") && card.contains("2. later:"), "{card}");
            assert!(!b.comments(n).unwrap().iter().any(|c| c.body.starts_with("taskq: gone")), "{role}");
            b.comment(n, "taskq: answer 1").unwrap();
            assert_eq!(task(&b, n).attempt, Some(a), "{role}: the answer returns to the never-started attempt");
            s.pass().unwrap();
            let t = task(&b, n);
            assert_eq!(*starts.borrow(), 1, "{role}: no resume of a session that never started");
            if role == "worker" {
                assert_eq!((t.state, t.attempt), (State::Ready, None), "the reply starts a fresh attempt");
            } else {
                assert_eq!((t.state, t.attempt, t.result.as_deref()), (State::Review, None, Some(result.as_str())), "a fresh reviewer keeps the result");
            }
        }
    }

    // Chain: § 4 budget, github.com list without assignees -> `assignee-only` admission and report filter. Risk: an
    // assigned task never starts, or a listed assignee costs a second read.
    #[test]
    fn assignees_missing_from_the_list_are_read_per_issue() {
        let b = Fake::default();
        let n = b.create("t", "", &["assignee-only".into()]).unwrap();
        let listed = b.issue(n).unwrap();
        b.state.borrow_mut().issues[0].assignees = vec!["me".into()];
        assert_eq!((assigned(&b, &listed, "me"), assigned(&b, &listed, "you")), (Ok(true), Ok(false)));
        b.state.borrow_mut().fail_issue_read = Some((n, 1));
        assert!(assigned(&b, &listed, "me").is_err());
        let full = b.issue(n).unwrap();
        b.state.borrow_mut().fail_issue_read = Some((n, 1));
        assert_eq!(assigned(&b, &full, "me"), Ok(true), "a listed assignee needs no read");
    }

    // Chain: incident lease heartbeat (#1140). Risk: a replaced attempt still listed by its host's heartbeat until
    // that host's next pass reported as a second live attempt (#1132, #1137), or a launched lost claim missed.
    #[test]
    fn incident_heartbeats_flag_only_leases_newer_than_the_attempt() {
        for (lease, age, expected) in [("replaced", 0, false), ("current", 0, false), ("lost", 0, true), ("lost", 301, false)] {
            crate::incident::take_detected();
            let b = Fake::default();
            let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
            let replaced = b.comment(n, "taskq: spawn worker claude:a other").unwrap();
            b.comment(n, &format!("taskq: requeue attempt {replaced}")).unwrap();
            let current = b.comment(n, "taskq: spawn worker claude:b first").unwrap();
            let lost = b.comment(n, "taskq: spawn worker claude:c other").unwrap();
            let a = match lease { "replaced" => replaced, "current" => current, _ => lost };
            b.create("taskq host other", &format!("#{n} {a}"), &[HOST_LABEL.into()]).unwrap();
            b.advance(age);
            b.create("taskq host first", &format!("#{n} {current}"), &[HOST_LABEL.into()]).unwrap();
            let c = cfg("local");
            let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
            s.pass().unwrap();
            assert_eq!(crate::incident::take_detected(), if expected { vec!["invariant.double_attempt"] } else { vec![] }, "{lease} lease, age {age}");
        }
    }

    // Chain: incident rejected child. Risk: a replaced child reported as rejected.
    #[test]
    fn incident_rejected_child_excludes_replaced_accepted_child() {
        for rejected in [false, true] {
            crate::incident::take_detected();
            let b = Fake::default();
            let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
            let accepted = b.comment(n, "taskq: spawn worker claude:first local").unwrap();
            let lost = b.comment(n, "taskq: spawn worker claude:second local").unwrap();
            b.comment(n, "taskq: requeue").unwrap();
            let c = cfg("local");
            let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
            s.focus.clear();
            let flag = Rc::new(RefCell::new(None));
            s.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution: if rejected { lost } else { accepted }, task: n, attempt: if rejected { lost } else { accepted }, rt: "claude".into(), proc: Box::new(FakeProc(flag.clone())) });
            s.pass().unwrap();
            assert_eq!(*flag.borrow(), Some(-9));
            assert_eq!(crate::incident::take_detected(), if rejected { vec!["invariant.double_attempt"] } else { vec![] });
        }
    }

    // Chain: incident claim readback. Risk: normal contention reported as a fault.
    #[test]
    fn incident_claim_missing_readback_excludes_normal_contention() {
        for missing in [false, true] {
            crate::incident::take_detected();
            let b = Fake::default();
            let n = b.create("task", "", &[TASK_LABEL.into()]).unwrap();
            let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300).unwrap().0.remove(0);
            if !missing { b.comment(n, "taskq: spawn worker claude:first other").unwrap(); }
            b.state.borrow_mut().hide_latest_comment = missing;
            let c = cfg("local");
            let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
            s.claim(&v, "worker", "claude", "").unwrap();
            assert!(s.kids.is_empty());
            assert_eq!(crate::incident::take_detected(), if missing { vec!["adapter.read_after_write"] } else { vec![] });
        }
    }

    // Chain: incident label loop (#1550). Risk: two hosts folding one task differently (N-1 `later #N attempt`:
    // `later` vs `waiting`) rewrite its label every pass; one relabel reported as a loop.
    #[test]
    fn label_fix_backs_off_when_another_fold_reverts_it() {
        crate::incident::take_detected();
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into(), "q-doing".into()]).unwrap();
        let c = cfg("local");
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        s.focus.clear();
        let has = |l: &str| b.issue(n).unwrap().labels.contains(&l.to_string());
        // the other host's fold (`doing`) undoes this host's `ready` before every pass
        for pass in 1..=61 {
            b.labels(n, &["q-doing".into()], &["q-ready".into()]).unwrap();
            s.pass().unwrap();
            assert_eq!(has("q-ready"), pass == 1 || pass == 61, "pass {pass}");
            assert_eq!(crate::incident::take_detected(), if pass == 2 { vec!["invariant.label_loop"] } else { vec![] }, "pass {pass}");
        }
        s.pass().unwrap(); // no fix breaks the streak
        b.labels(n, &["q-doing".into()], &["q-ready".into()]).unwrap();
        s.pass().unwrap();
        assert!(has("q-ready") && crate::incident::take_detected().is_empty());
        b.labels(n, &["q-ask".into()], &["q-ready".into()]).unwrap();
        s.pass().unwrap(); // different fix also breaks the streak
        assert!(has("q-ready") && crate::incident::take_detected().is_empty());
    }

    // Chain: incident label loop (#1873). Risk: another host's normal spawn lands its comment seconds before its label
    // move; this host fixes the label, then the next pass reads a list from before the fix (or the peer mid-move) and
    // files a loop for a task that moves on normally (2026-10-29: #1813, #1840).
    #[test]
    fn label_fix_after_another_hosts_spawn_is_no_loop() {
        crate::incident::take_detected();
        let b = Fake::default();
        let n = b.create("task", "", &[TASK_LABEL.into(), "q-ready".into()]).unwrap();
        b.comment(n, "taskq: spawn worker claude:peer win").unwrap();
        let c = cfg("local");
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        s.focus.clear();
        let before = b.issues(None).unwrap();
        s.pass().unwrap(); // fold `doing`, label `q-ready`: this host fixes it
        assert!(b.issue(n).unwrap().labels.contains(&"q-doing".to_string()));
        b.state.borrow_mut().stale_list = Some(before);
        s.pass().unwrap(); // the list predates the fix
        b.labels(n, &["q-ready".into(), "q-doing".into()], &[]).unwrap();
        s.pass().unwrap(); // a read while the peer moves the same way shows both labels
        b.labels(n, &[], &["q-ready".into()]).unwrap();
        s.pass().unwrap();
        assert!(crate::incident::take_detected().is_empty());
        assert!(s.label_fixes.is_empty());
    }

    // Chain: § 4 one service per host name (#933). Risk: a second install under the same host name posts `gone` for
    // the first one's live child and respawns, so two workers run one task.
    #[test]
    fn a_second_service_under_one_host_name_writes_nothing_and_the_live_child_stays() {
        let b = Fake::default();
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        let c = cfg("win");
        let (ea, eb): (Exits, Exits) = Default::default();
        let mut a = service(&b, &c, ea.clone());
        let mut w = service(&b, &c, eb.clone());
        (a.instance, w.instance) = ("native".into(), "wsl".into());
        a.pass().unwrap();
        let comments = b.comments(n).unwrap().len();
        for _ in 0..3 {
            b.advance(70);
            assert!(w.pass().unwrap_err().contains("another service native runs as host win"));
            a.pass().unwrap();
        }
        let t = task(&b, n);
        assert_eq!((t.state, t.retries, a.kids.len(), w.kids.len(), eb.borrow().len()), (State::Doing, 0, 1, 0, 0));
        assert_eq!(b.comments(n).unwrap().len(), comments, "no gone, no second spawn");
        // a restart of the same install reads its own id and carries on; the first one stopped, the heartbeat goes
        // stale and the other install takes over the host name
        let mut again = service(&b, &c, Default::default());
        again.instance = "native".into();
        again.pass().unwrap();
        b.advance(301);
        assert!(w.pass().is_ok());
    }

    // D18: every installed service reports metadata without changing legacy heartbeat lines.
    #[test]
    fn heartbeat_reports_service_diagnostics() {
        let b = Fake::default();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.instance = "abcdef012345".into();
        let body=s.heartbeat_body();
        assert!(body.lines().any(|l| l.starts_with("diagnostics ")));
        let diagnostics:Value=serde_json::from_str(body.lines().find_map(|l|l.strip_prefix("diagnostics ")).unwrap()).unwrap();
        let at=diagnostics["observed"].as_i64().unwrap();
        let issue=crate::board::Issue {number:1,title:"taskq host mac".into(),body,labels:vec![],assignees:vec![],updated:at,created:at,closed:None,open:true,trusted:true,reason:None};
        let snapshot=crate::snapshot::from_views("board",&b,&c,at,&[],&[issue]);
        assert_eq!(snapshot.json()["hosts"][0]["diagnostics"]["running"]["value"],"4.0.0");
        let stale=crate::snapshot::from_views("board",&b,&c,at+c.ttl as i64+1,&[],&[crate::board::Issue {
            number:1,title:"taskq host mac".into(),body:s.heartbeat_body(),labels:vec![],assignees:vec![],updated:at,created:at,closed:None,open:true,trusted:true,reason:None}]);
        assert_eq!(stale.json()["hosts"][0]["diagnostics"]["running"]["state"],"stale");
    }

    // Chain: #1985 A4 host hourly requests -> heartbeat `requests <n>/h` -> status Hosts line. Risk: the PM cannot see
    // which host spends the board user's quota, or unbilled 304s count toward it.
    #[test]
    fn host_hourly_requests_reach_hosts_line() {
        let b = Fake::default();
        let c = cfg("win");
        let mut s = service(&b, &c, Default::default());
        assert!(!s.heartbeat_body().lines().any(|l| l.starts_with("requests ")), "no full hour, no line");
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err("none".into()))));
        host.lock().unwrap().last_hour = Some([("GET issues".to_string(), 5), ("GET issues 304".to_string(), 40)].into());
        s.host = Some(host);
        let body = s.heartbeat_body();
        assert!(body.lines().any(|l| l == "requests 5/h"), "{body}");
        let issue = crate::board::Issue { number: 1, title: "taskq host win".into(), body, labels: vec![], assignees: vec![], updated: 100, created: 100, closed: None, open: true, trusted: true, reason: None };
        let text = crate::snapshot::from_views("board", &b, &c, 100, &[], &[issue]).text(&c, &json!({})).unwrap();
        assert!(text.lines().any(|l| l.starts_with("Hosts: win v4.0.0 ") && l.ends_with(" 5 req/h")), "{text}");
    }

    // Chain: #2084 A1/A3 host hourly requests per API host -> heartbeat `requests <api host> <n>/h` -> Hosts line
    // github.com share. Defect: a host with GitHub and GitLab boards showed one sum. Risk: the share is lost, or an
    // older heartbeat with only `requests <n>/h` no longer reads.
    #[test]
    fn host_github_share_reaches_hosts_line() {
        let b = Fake::default();
        let c = cfg("win");
        let mut s = service(&b, &c, Default::default());
        let host = std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err("none".into()))));
        host.lock().unwrap().last_hour = Some([("GET issues".to_string(), 5)].into());
        host.lock().unwrap().hosts_last_hour = Some([("api.github.com".to_string(), 3), ("gitlab.com".to_string(), 2)].into());
        s.host = Some(host);
        let body = s.heartbeat_body();
        for line in ["requests 5/h", "requests api.github.com 3/h", "requests gitlab.com 2/h"] {
            assert!(body.lines().any(|l| l == line), "{line}: {body}");
        }
        let hosts = |body: String| {
            let issue = crate::board::Issue { number: 1, title: "taskq host win".into(), body, labels: vec![], assignees: vec![], updated: 100, created: 100, closed: None, open: true, trusted: true, reason: None };
            let text = crate::snapshot::from_views("board", &b, &c, 100, &[], &[issue]).text(&c, &json!({})).unwrap();
            text.lines().find(|l| l.starts_with("Hosts: win v4.0.0 ")).unwrap_or_default().to_string()
        };
        assert!(hosts(body.clone()).ends_with(" 5 req/h (github.com 3)"), "{body}");
        let old = body.lines().filter(|l| !l.starts_with("requests api.") && !l.starts_with("requests gitlab.")).collect::<Vec<_>>().join("\n");
        assert!(hosts(old).ends_with(" 5 req/h"), "an older heartbeat still reads");
    }

    // Chain: #1755 A4 tray fault file -> heartbeat `tray <code>` -> status Hosts line. Risk: a tray fault stays in a
    // host-local log the PM cannot read, or a hand-written file forwards arbitrary text to the public board.
    #[test]
    fn tray_fault_reaches_heartbeat_and_hosts_line() {
        let dir = std::env::temp_dir().join(format!("taskq-tray-fault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let b = Fake::default();
        let c = Config { install: dir.clone(), ..cfg("win") };
        let s = service(&b, &c, Default::default());
        assert!(!s.heartbeat_body().lines().any(|l| l.starts_with("tray ")), "no fault, no line");
        std::fs::write(dir.join("tray.fault"), "ack-timeout").unwrap();
        let body = s.heartbeat_body();
        assert!(body.lines().any(|l| l == "tray ack-timeout"), "{body}");
        let issue = crate::board::Issue { number: 1, title: "taskq host win".into(), body, labels: vec![], assignees: vec![], updated: 100, created: 100, closed: None, open: true, trusted: true, reason: None };
        let text = crate::snapshot::from_views("board", &b, &c, 100, &[], &[issue]).text(&c, &json!({})).unwrap();
        assert!(text.lines().any(|l| l.starts_with("Hosts: win v4.0.0 ") && l.ends_with(" tray ack-timeout")), "{text}");
        std::fs::write(dir.join("tray.fault"), "C:\\Users\\x secret").unwrap();
        assert!(!s.heartbeat_body().lines().any(|l| l.starts_with("tray ")), "only a fixed code crosses to the board");
        let _ = std::fs::remove_dir_all(dir);
    }

    // Chain: board entry, fold changes, snapshot row `since` (§ 4 Tray age). Risk: the age restarts on a comment that
    // changes no state, or invents a time for a task with no logged state change.
    #[test]
    fn row_since_is_the_last_logged_state_change() {
        let b=Fake::default();let c=cfg("mac");
        let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
        let since=||crate::snapshot::read(&b,&c,&Value::Null,&[]).tasks[0].json()["since"].clone();
        assert_eq!(since(),Value::Null,"no logged state change: unknown");
        b.advance(100);
        let w=b.comment(n,"taskq: spawn worker codex:session mac").unwrap();
        let at=b.comments(n).unwrap().into_iter().find(|x|x.id==w).unwrap().created;
        assert_eq!(since(),at);
        b.advance(100);b.raw(n,"a plain note",true,false);
        assert_eq!(since(),at,"a comment that changes no state keeps the age");
    }

    // Chain: § 4 heartbeat body, DashQ host join (dashq docs/design/hosts.md H2). Risk: a changed or missing line
    // splits one machine into several Hosts rows or breaks the one-service-per-host check.
    #[test]
    fn heartbeat_body_lists_version_service_machine_focus() {
        let legacy=|s:&Service|s.heartbeat_body().lines().filter(|l|!l.starts_with("diagnostics ") && !l.starts_with("balancing ")).collect::<Vec<_>>().join("\n");
        let b = Fake::default();
        let c = cfg("win");
        let mut s = service(&b, &c, Default::default());
        s.instance = "abc123".into();
        s.machine = "DESKTOP wsl".into();
        s.focus = vec![];
        assert_eq!(legacy(&s), "version 4.0.0\nservice abc123\nmachine DESKTOP wsl\nfocus off");
        // #1832: the host link key goes with the heartbeat the board trusts
        s.link = "link aa bb".into();
        assert_eq!(legacy(&s), "version 4.0.0\nservice abc123\nmachine DESKTOP wsl\nlink aa bb\nfocus off");
        s.link = String::new();
        s.machine = String::new();
        s.focus = vec!["all".into()];
        assert_eq!(legacy(&s), "version 4.0.0\nservice abc123");
        // #1204: a live Claude child publishes its account-scoped URL; every host's snapshot reads it back by host and attempt
        s.pending.insert((7, 42));
        s.kids.push(Kid { unknown:None, stopping:false, session_url:Some("https://claude.ai/code/session_014abc".into()), task:7,attempt:42,execution:42,rt:"claude".into(),proc:Box::new(FakeProc(Default::default())) });
        assert_eq!(legacy(&s), "version 4.0.0\nservice abc123\n#7 42\nsession #7 42 https://claude.ai/code/session_014abc");
        let host = |body: String| crate::board::Issue { number: 1, title: "taskq host win".into(), body, labels: vec![], assignees: vec![], updated: 0, created: 0, closed: None, open: true, trusted: true, reason: None };
        let urls = crate::snapshot::session_urls(&[host(s.heartbeat_body()), host("session #8 43 https://evil.example/x".into())]);
        assert_eq!(urls.into_iter().collect::<Vec<_>>(), vec![(("win".to_string(), 42), "https://claude.ai/code/session_014abc".to_string())]);
    }

    // Chain: native observation -> aggregate heartbeat -> remote read -> durable terminal after lost response.
    #[test]
    fn execution_heartbeat_is_fenced_visible_and_never_reopens_terminal() {
        let b=Fake::default();let c=cfg("mac");let remote=cfg("win");
        let n=b.create("T","",&[TASK_LABEL.into()]).unwrap();
        let exits:Exits=Default::default();let mut s=service(&b,&c,exits.clone());
        s.pass().unwrap();s.pass().unwrap();let a=task(&b,n).attempt.unwrap();
        let read=||crate::snapshot::read(&b,&remote,&json!({}),&[]).tasks.into_iter().find(|r|r.n==n).unwrap().json();
        let host=b.issues(None).unwrap().into_iter().find(|i|i.title=="taskq host mac").unwrap();
        let running=host.body.clone();
        assert_eq!(read()["executions"][0]["state"],"running");
        assert_eq!(read()["executions"][0]["runtime"],"claude");
        assert_eq!(read()["executions"][0]["role"],"worker");
        let snapshot=s.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.tasks[0].executions[0]["runtime"],"claude","local overlay keeps runtime");
        assert_eq!(snapshot.tasks[0].executions[0]["role"],"worker","local overlay keeps role");
        assert_eq!(snapshot.status_keys(None).into_iter().find(|(k,_)|*k=="hosts").unwrap().1,snapshot.json()["hosts"]);
        for fault in ["foreign","execution","input","duplicate","malformed","partial-duplicate","invalid-state","untrusted","stale","future"] {
            let mut body=running.clone();
            if fault=="foreign" {body=body.replace("execution ","ignored ");}
            if matches!(fault,"execution"|"input"|"invalid-state") {
                let mut row:Value=serde_json::from_str(running.lines().find_map(|l|l.strip_prefix("execution ")).unwrap()).unwrap();
                match fault {"execution"=>row["id"]=json!(a+999),"input"=>row["through"]=json!(a+999),_=>row["state"]=json!("finished")};
                body=running.lines().filter(|l|!l.starts_with("execution ")).chain(std::iter::once("")).collect::<Vec<_>>().join("\n")+&format!("execution {row}");
            }
            if fault=="duplicate" {body+=&format!("\n{}",running.lines().find(|l|l.starts_with("execution ")).unwrap());}
            if fault=="partial-duplicate" {body+="\nexecution {";}
            if fault=="malformed" {body=running.lines().filter(|l|!l.starts_with("execution ")).collect::<Vec<_>>().join("\n")+"\nexecution {";}
            b.edit(host.number,&body).unwrap();
            {let mut st=b.state.borrow_mut();let now=st.clock;let h=st.issues.iter_mut().find(|i|i.number==host.number).unwrap();
                h.trusted=fault!="untrusted";if fault=="stale" {h.updated=now-remote.ttl as i64-1;}if fault=="future" {h.updated=now+60;}}
            let row=read();assert_eq!(row["executions"][0]["state"],"unknown","{fault}: {row}");
            assert!(row["execution_wait"].is_string(),"{fault}");
            b.state.borrow_mut().issues.iter_mut().find(|i|i.number==host.number).unwrap().trusted=true;
        }
        b.edit(host.number,&running).unwrap();
        s.kids.iter_mut().find(|k|k.attempt==a).unwrap().stopping=true;s.beat().unwrap();assert_eq!(read()["executions"][0]["state"],"waiting");s.kids.iter_mut().find(|k|k.attempt==a).unwrap().stopping=false;
        *exits.borrow().iter().find(|(id,_)|*id==n).unwrap().1.borrow_mut()=Some(-999);s.pass().unwrap();
        assert!(read()["executions"][0]["reason"].as_str().unwrap().contains("read failed"));
        assert!(s.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().json()["execution_wait"].as_str().unwrap().contains("read failed"));
        *exits.borrow().iter().find(|(id,_)|*id==n).unwrap().1.borrow_mut()=Some(0);b.state.borrow_mut().fail_comments=1;
        s.reap_owned();s.beat().unwrap();assert_eq!(s.kids.len(),1);
        assert_eq!(read()["executions"][0]["state"],"unknown","failed final delivery retains ownership");
        b.state.borrow_mut().lose_comment_response=true;s.reap_owned();assert!(s.kids.is_empty());
        b.state.borrow_mut().lose_comment_response=false;
        let log=entries(&b,n).unwrap();assert_eq!(core::executions(n,&log)[0].release,Some(core::Release::Exit));
        assert_eq!(log.iter().filter(|e|e.body.starts_with("taskq: released")).count(),1);
        b.edit(host.number,&running).unwrap();let row=read();assert!(row.get("executions").is_none(),"late heartbeat cannot reopen terminal");
        let mut fresh=service(&b,&c,Default::default());fresh.focus.clear();fresh.pass().unwrap();
        assert!(!fresh.heartbeat_body().lines().any(|l|l.starts_with("execution ")),"restart reads durable terminal");
    }

    // Chain: slow native children -> bounded observation round -> honest heartbeat -> fair next round.
    #[test]
    fn slow_observations_defer_without_releasing_and_rotate_fairly() {
        struct Slow {task:u64,calls:Rc<RefCell<Vec<u64>>>}
        impl Proc for Slow {
            fn accepted(&mut self)->Result<bool,String> {Ok(true)}
            fn exited(&mut self)->Result<Option<i32>,String> {
                self.calls.borrow_mut().push(self.task);std::thread::sleep(std::time::Duration::from_millis(25));Ok(None)
            }
            fn kill(&mut self)->Result<(),String> {Err("fixture must stay owned".into())}
            fn id(&self)->u32 {0}
        }
        let b=Fake::default();let mut c=cfg("mac");c.limits.insert("claude".into(),3);
        for _ in 0..3 {b.create("slow fixture","",&[TASK_LABEL.into()]).unwrap();}
        let mut s=service(&b,&c,Default::default());s.pass().unwrap();s.pass().unwrap();
        assert_eq!(s.kids.len(),3);
        let calls=Rc::new(RefCell::new(vec![]));
        for kid in &mut s.kids {kid.proc=Box::new(Slow {task:kid.task,calls:calls.clone()});}
        for round in 1..=3 {
            s.pass_inner(std::time::Duration::from_millis(10)).unwrap();
            assert_eq!(calls.borrow().len(),round,"a slow poll must not delay status behind all children");
            assert_eq!(s.kids.len(),3);
            assert_eq!(s.kids.iter().filter(|k|k.unknown.is_some()).count(),2,"deferred observations must be unknown, not freshly healthy");
            let heartbeat=b.issue(s.host_issue.unwrap()).unwrap();
            assert!(heartbeat.body.contains("native observation deferred"));
        }
        assert_eq!(calls.borrow().iter().collect::<std::collections::HashSet<_>>().len(),3,"slow first child starved another execution");
        calls.borrow_mut().clear();
        for round in 1..=3 {
            s.reap_owned_within(std::time::Duration::from_millis(10));
            assert_eq!(calls.borrow().len(),round);assert_eq!(s.kids.len(),3);assert_eq!(s.kids.iter().filter(|k|k.unknown.is_some()).count(),2);
        }
        assert_eq!(calls.borrow().iter().collect::<std::collections::HashSet<_>>().len(),3,"reduced pass starved an execution");
        let tasks=s.kids.iter().map(|k|k.task).collect::<Vec<_>>();
        s.kids.clear();calls.borrow_mut().clear();
        let recovery_calls=calls.clone();
        s.recover=Some(Box::new(move |task,_| {
            recovery_calls.borrow_mut().push(task);std::thread::sleep(std::time::Duration::from_millis(25));Err("fixture: native recovery unknown".into())
        }));
        for round in 1..=3 {
            s.pass_inner(std::time::Duration::from_millis(10)).unwrap();assert_eq!(calls.borrow().len(),round);
            assert_eq!(s.execution_waits.len(),3);assert!(s.kids.is_empty(),"unknown recovery cannot replay work");
        }
        assert_eq!(calls.borrow().iter().collect::<std::collections::HashSet<_>>().len(),3,"cold recovery starved an execution");
        for task in tasks {assert!(core::executions(task,&entries(&b,task).unwrap()).iter().all(|x|x.release.is_none()));}
    }

    // Chain: § 4 board budget, heartbeat. Risk: a PATCH every pass burns the shared budget (#1358: the diagnostics
    // observation time changed the body every pass), or a sparse heartbeat lets another host expire a live lease.
    #[test]
    fn idle_heartbeat_is_batched_at_a_third_of_ttl_and_stays_fresh() {
        let b = Fake::default();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.instance="abcdef012345".into();
        let beat = |b: &Fake| b.issues(None).unwrap().into_iter().find(|i| i.title == "taskq host mac").map(|i| i.updated).unwrap();
        s.pass().unwrap();
        let (mut writes, mut last) = (0, beat(&b));
        for _ in 0..360 {
            b.advance(10);
            assert!(b.now().unwrap() - beat(&b) < c.ttl, "lease stays fresh between passes");
            // a live producer clock: every pass reports new observation times
            let mut d = s.service_diagnostics();
            d["observed"] = json!(b.now().unwrap());
            d["running"]["at"] = json!(b.now().unwrap());
            *s.diagnostics.borrow_mut() = Some((std::time::Instant::now(), d));
            s.pass().unwrap();
            if beat(&b) != last { writes += 1; last = beat(&b); }
        }
        assert_eq!(writes, 3600 / (c.ttl / 3), "one periodic write per ttl/3, not one per fast pass");
        let body = b.issues(None).unwrap().into_iter().find(|i| i.title == "taskq host mac").unwrap().body;
        let at = body.lines().find_map(|l| l.strip_prefix("at ")).unwrap();
        assert!(body.contains(&format!("\"observed\":{at},")), "a write carries the current diagnostics");
        let line = body.lines().find(|l| l.starts_with("diagnostics ")).unwrap();
        let mut d: Value = serde_json::from_str(&line["diagnostics ".len()..]).unwrap();
        d["running"]["value"] = json!("9.9.9");
        assert_ne!(stable(&body), stable(&body.replace(line, &format!("diagnostics {d}"))), "a changed diagnostics value is written at once");
    }

    // Chain: § 4 heartbeat, status.json hosts (#1729 A3). Risk: after a sleep the pass reads this host's heartbeat
    // before rewriting it, so status.json shows this host stale for a whole pass interval while the board is fresh.
    #[test]
    fn own_heartbeat_row_is_fresh_after_a_wake() {
        let b = Fake::default();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.instance="abcdef012345".into();
        s.pass().unwrap();
        b.advance(1920);
        s.pass().unwrap();
        let snapshot = s.snapshot.as_ref().unwrap();
        let row = snapshot.hosts.iter().find(|h| h.name == "mac").unwrap();
        assert_eq!(snapshot.observed - row.updated, 0, "the pass that rewrote the heartbeat reports its write");
    }

    // Chain: § 4 heartbeat, host rename (#1483). Risk: a renamed host leaves its old-name heartbeat open in every
    // Hosts line, closes another service's heartbeat, or never replaces its own closed one.
    #[test]
    fn renamed_host_closes_only_its_own_old_heartbeat() {
        let b = Fake::default();
        let c = cfg("mac");
        let old = b.create("taskq host macbook-m2.local", "version 4.0.0\nservice abcdef012345", &[HOST_LABEL.into()]).unwrap();
        let other = b.create("taskq host win", "version 4.0.0\nservice 999999999999", &[HOST_LABEL.into()]).unwrap();
        b.advance(10 * c.ttl);
        let mut s = service(&b, &c, Default::default());
        s.instance = "abcdef012345".into();
        s.pass().unwrap();
        let old_issue = b.issue(old).unwrap();
        assert!(!old_issue.open, "A1: the old-name heartbeat of this service closes");
        assert!(b.comments(old).unwrap().iter().any(|c| c.body.contains("host mac")), "A1: the close names the new host");
        assert!(b.issue(other).unwrap().open, "A3: a stale heartbeat of another service stays open");
        assert!(b.comments(other).unwrap().is_empty());
        // A2: the host's own closed heartbeat is replaced on the next pass; the old name stays closed
        let mine = s.host_issue.unwrap();
        b.close(mine).unwrap();
        b.advance(c.ttl);
        s.pass().unwrap();
        let open: Vec<_> = b.issues(None).unwrap().into_iter().filter(is_host).map(|i| i.title).collect();
        assert_eq!(open, ["taskq host win", "taskq host mac"]);
        assert!(!b.issue(old).unwrap().open && s.host_issue != Some(mine));
    }

    // Chain: R2 two hosts -> lost heartbeat -> still-live executor -> confirmed exit -> replacement.
    #[test]
    fn two_hosts_wait_for_confirmed_exit_before_replacement() {
        let b = Fake::default();
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        let (ca, cb) = (cfg("mac"), cfg("win"));
        let (ea, eb): (Exits, Exits) = Default::default();
        let mut a = service(&b, &ca, ea.clone());
        let mut w = service(&b, &cb, eb.clone());
        a.pass().unwrap();
        w.pass().unwrap();
        let t = task(&b, n);
        assert_eq!((t.state, t.role.as_deref()), (State::Doing, Some("worker")));
        assert_eq!((a.kids.len(), w.kids.len()), (1, 0));
        assert!(b.issue(n).unwrap().labels.contains(&"q-doing".to_string()));

        // a live mac outlasts the ttl: its unchanged heartbeat stays fresh, win leaves the task alone
        for _ in 0..3 {
            b.advance(200);
            a.pass().unwrap();
            w.pass().unwrap();
        }
        assert_eq!(host_of(&task(&b, n)), Some("mac"));
        // #1491: a fresh owner heartbeat listing the execution as running is no wait on the other host
        assert!(!w.execution_waits.contains_key(&n), "{:?}", w.execution_waits);
        let row = w.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
        assert!(row["execution_wait"].is_null(), "{row}");
        assert_eq!((row["executions"][0]["state"].as_str(), row["executions"][0]["host"].as_str()), (Some("running"), Some("mac")));
        // a fresh owner heartbeat without the execution row still waits
        let hb = b.issues(None).unwrap().into_iter().find(|i| i.title == "taskq host mac").unwrap();
        b.edit(hb.number, &hb.body.lines().filter(|l| !l.starts_with("execution ")).collect::<Vec<_>>().join("\n")).unwrap();
        w.pass().unwrap();
        assert!(w.execution_waits[&n].contains("termination unconfirmed"));
        // A missing heartbeat does not kill the old executor.
        b.advance(301);
        w.pass().unwrap();
        assert_eq!(host_of(&task(&b, n)), Some("mac"));
        assert_eq!(ea.borrow().iter().chain(eb.borrow().iter()).filter(|(_, p)| p.borrow().is_none()).count(), 1);
        assert!(w.execution_waits[&n].contains("termination unconfirmed"));
        let snapshot = crate::snapshot::read(&b, &cb, &json!({}), &[]);
        assert!(snapshot.tasks.iter().find(|r| r.n == n).unwrap().json()["execution_wait"].as_str().unwrap().contains("termination unconfirmed"));
        let text = snapshot.text(&cb, &json!({})).unwrap();
        assert!(text.contains("Run state unknown; see service log") && !text.contains("termination unconfirmed"), "#1524 owner text only: {text}");
        assert!(eb.borrow().is_empty());
        *ea.borrow()[0].1.borrow_mut() = Some(0);
        a.focus.clear();
        a.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Ready);
        w.pass().unwrap();
        let t = task(&b, n);
        assert_eq!((t.state, host_of(&t)), (State::Doing, Some("win")));
        assert_eq!(t.retries, 1);
    }

    // Chain: #1226 open stage -> vanished child -> normal gone -> terminal record and freed lease.
    // Existing exit checks do not cross the retained profile store or selftest boundary.
    #[test]
    fn missing_terminal_record_repaired_only_by_normal_recovery() {
        let dir = std::env::temp_dir().join(format!("taskq-terminal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (_capture,_guard)=crate::profile::tests::collecting(&dir);
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, &core::entry("spawn", &["worker", "claude:s", "mac"], None, "")).unwrap();
        let mut c = cfg("mac");
        c.install = dir.clone();
        c.root = dir.clone();
        let record=json!({"project":c.repo,"host":"mac","stage":"build","open":true,"task":n,"attempt":a,"rt":"claude",
            "outcome":"unknown","error":"none","at_ms":1,"unknown":"open-stage"});
        drop(crate::profile::log_lock(&dir.join("taskq.log")).unwrap());
        crate::telemetry::append(&dir,&format!("taskq-stage {record}"),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64).unwrap();
        // #1389: an old gap warns once with a stable reason and passes; unavailable capture still fails (tests/telemetry_process.rs).
        let warn = crate::profile::selftest(&dir, &c.repo, &vec![], 300).unwrap();
        assert!(warn.starts_with("attempt terminal records: WARN (1 older than ttl 300s; not blocking)"), "{warn}");
        assert_eq!(warn.matches(&format!("| o/r | #{n} | {a} | claude | mac |")).count(), 1, "{warn}");
        assert!(warn.contains("| missing terminal record |"), "{warn}");
        // A live attempt stays exempt even after ttl; the check never mutates board state.
        assert!(crate::profile::selftest(&dir, &c.repo, &vec![(c.repo.clone(), a)], 300).unwrap().contains("no overdue"));
        assert_eq!(task(&b, n).attempt, Some(a));
        let report = crate::profile::report(&dir);
        assert!(report.contains(&format!("| o/r | #{n} | {a} | claude | mac |")), "{report}");
        let mut s = service(&b, &c, Default::default());
        s.focus.clear();
        s.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a), "missing child is unknown, not exited");
        assert!(s.execution_waits.contains_key(&n));
        release_execution(&b, n, a, a, "exit").unwrap();
        s.pass().unwrap();
        assert!(b.comments(n).unwrap().iter().any(|c| c.body.starts_with(&format!("taskq: gone attempt {a}"))));
        assert_eq!(task(&b, n).attempt, None);
        assert!(!s.heartbeat_body().lines().any(|l| l == format!("#{n} {a}")));
        let repaired = crate::profile::read_log(&dir).unwrap();
        assert!(repaired.contains("\"outcome\":\"failed\",\"error\":\"exit\""), "{repaired}");
        assert!(crate::profile::selftest(&dir, &c.repo, &vec![], 300).unwrap().contains("no overdue"));
        s.pass().unwrap();
        assert_eq!(crate::profile::read_log(&dir).unwrap(), repaired, "repair is idempotent");

        drop(_guard);drop(_capture);std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: §4 ended review -> left-board -> close -> next-pass repair -> selftest.
    // Risk: historical attempts absent from open views permanently block unrelated workers.
    #[test]
    fn closed_or_left_board_attempt_gets_terminal_record() {
        for closed in [true, false] {
            let dir = std::env::temp_dir().join(format!("taskq-ended-{}-{closed}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let (_capture,_guard)=crate::profile::tests::collecting(&dir);
            let b = Fake::default();
            let n = b.create("T", "", &["taskq".into()]).unwrap();
            let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
            b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40))).unwrap();
            let a = b.comment(n, "taskq: spawn reviewer codex:r mac").unwrap();
            assert_eq!(task(&b, n).attempt, Some(a));
            let mut c = cfg("mac");
            c.install = dir.clone();
            c.root = dir.clone();
            let path = dir.join("taskq.log");
            let record = json!({"project": c.repo, "host":"mac", "stage":"review", "task":n,
                "attempt":a, "rt":"codex", "outcome":"unknown", "error":"left-board", "at_ms":1});
            std::fs::write(&path, format!("taskq-stage {record}\n")).unwrap();
            if closed {
                b.comment(n, "taskq: close").unwrap();
                b.close(n).unwrap();
            } else {
                b.labels(n, &[], &["taskq".into()]).unwrap();
            }
            assert!(crate::profile::selftest(&dir, &c.repo, &vec![], 300).is_err(), "unimported legacy evidence reads unknown");
            let mut s = service(&b, &c, Default::default());
            s.focus.clear();
            s.pass().unwrap();
            assert!(crate::profile::selftest(&dir, &c.repo, &vec![], 300).unwrap().contains("no overdue"), "ended attempt must have committed terminal evidence");
            let repaired = crate::profile::read_log(&dir).unwrap();
            s.pass().unwrap();
            assert_eq!(crate::profile::read_log(&dir).unwrap(), repaired);
            assert!(!b.comments(n).unwrap().iter().any(|c| c.body.starts_with("taskq: gone")));
            drop(_guard);drop(_capture);std::fs::remove_dir_all(dir).unwrap();
        }
    }

    // Chain: #1341 per-pass repair -> full-store Reader snapshot -> pinned WAL -> failed telemetry checkpoint.
    // Risk: every project pass reopens the store, appends wait seconds and lose batches; a stale cache fabricates a repair.
    #[test]
    fn ended_attempt_repair_reads_store_once_per_interval() {
        let dir = std::env::temp_dir().join(format!("taskq-repair-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (_capture,_guard)=crate::profile::tests::collecting(&dir);
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
        let mut c = cfg("mac");
        c.install = dir.clone();
        c.root = dir.clone();
        crate::profile::open(&dir, &c.repo, "mac", n, a, "codex", false);
        let reads = || crate::profile::STORE_READS.with(|n| n.get());
        let before = reads();
        let mut s = service(&b, &c, Default::default());
        s.focus.clear();
        for _ in 0..3 { s.pass().unwrap(); }
        assert_eq!(reads() - before, 1, "open attempt passes reuse one store read");
        // #1950: a close in `doing` is reopened; the attempt ends through a result and the reviewer's close
        b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "candidate")).unwrap();
        b.comment(n, "taskq: close").unwrap();
        b.close(n).unwrap();
        s.pass().unwrap();
        assert_eq!(reads() - before, 2, "a repair confirms the cached row with a fresh read");
        crate::profile::selftest(&dir, &c.repo, &vec![], 300).expect("closed attempt repaired");
        let repaired = crate::profile::read_log(&dir).unwrap();
        s.pass().unwrap();
        assert_eq!(reads() - before, 2);
        assert_eq!(crate::profile::read_log(&dir).unwrap(), repaired, "repair is not repeated from the cache");
        drop(_guard);drop(_capture);std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: §4 historical no-host review -> fresh ended-board proof -> one-pass repair -> selftest.
    // Risk: #1252 stays blocked, or another project/host receives fabricated terminal evidence.
    #[test]
    fn historical_no_host_terminal_repair_requires_local_ownership() {
        let root = std::env::temp_dir().join(format!("taskq-no-host-{}", crate::runtime::uuid()));
        std::fs::create_dir(&root).unwrap();
        let child = coreq::process::output("test.no-host-repair", std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::tests::historical_no_host_repair_child", "--ignored", "--nocapture"])
            .env("TASKQ_NO_HOST_REPAIR_ROOT", &root).env_remove("COREQ_TRACE_CONTEXT"),
            std::time::Duration::from_secs(60)).unwrap();
        assert!(child.status.success(), "{}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
        // Native process exit releases every fixture handle before Windows removes the stores.
        std::fs::remove_dir_all(&root).unwrap();
        assert!(!root.exists(), "fixture cleanup must complete");
    }

    #[test]
    #[ignore = "subprocess helper exercised by historical_no_host_terminal_repair_requires_local_ownership"]
    fn historical_no_host_repair_child() {
        let root = PathBuf::from(std::env::var_os("TASKQ_NO_HOST_REPAIR_ROOT").unwrap());
        for closed in [true, false] {
            for evidence in ["local", "foreign-project", "foreign-host", "missing-log", "empty-log", "unreadable-log", "ignored-spawn", "untrusted-spawn", "runtime-mismatch", "unreadable-board", "unreadable-comments"] {
                let dir = root.join(format!("{closed}-{evidence}"));
                std::fs::create_dir_all(dir.join(".taskq")).unwrap();
                let b = Fake::default();
                let n = b.create("T", "", &["taskq".into()]).unwrap();
                let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
                b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40))).unwrap();
                let host = if evidence == "foreign-host" { "win" } else { "mac" };
                let spawn = format!("taskq: spawn reviewer codex:r {host}");
                let a = b.raw(n, &spawn, evidence != "untrusted-spawn", evidence == "ignored-spawn");
                let mut c = cfg("mac");
                c.install = dir.clone();
                c.root = dir.clone();
                let path = dir.join("taskq.log");
                let project = if evidence == "foreign-project" { "other/project" } else { &c.repo };
                let rt = if evidence == "runtime-mismatch" { "claude" } else { "codex" };
                let mut original = String::new();
                for (stage, outcome, error) in [("claim", "ok", "none"), ("start", "ok", "none"), ("checks", "ok", "none"), ("close", "failed", "other"), ("review", "unknown", "left-board")] {
                    let record = json!({"project":project, "stage":stage, "task":n, "attempt":a,
                        "rt":rt, "outcome":outcome, "error":error, "at_ms":1, "unknown":"no-usage"});
                    original += &format!("taskq-stage {record}\n");
                }
                std::fs::write(&path, &original).unwrap();
                let attempt_log = crate::runtime::attempt_log(&dir, "reviewer", n, a);
                if evidence == "unreadable-log" {
                    std::fs::create_dir(&attempt_log).unwrap();
                } else if evidence != "missing-log" {
                    std::fs::write(&attempt_log, if evidence == "empty-log" { "" } else { "original reviewer output\n" }).unwrap();
                }
                if closed {
                    b.comment(n, &format!("taskq: close attempt {a}")).unwrap();
                    b.close(n).unwrap();
                } else {
                    b.labels(n, &[], &["taskq".into()]).unwrap();
                }
                if evidence == "unreadable-board" { b.state.borrow_mut().fail_issue_read = Some((n, 1)); }
                if evidence == "unreadable-comments" { b.state.borrow_mut().fail_comment_reads = 1; }
                // Prepare the legacy fixture before collection; this case proves ownership, not import contention.
                crate::profile::prune(&dir).unwrap();
                let history = crate::profile::read_log(&dir).unwrap();
                let (_capture,_guard)=crate::profile::tests::collecting(&dir);
                assert!(crate::profile::selftest(&dir, project, &vec![], 300).unwrap().contains("WARN"));
                let mut s = service(&b, &c, Default::default());
                s.focus.clear();
                s.pass().unwrap();
                let repaired = crate::profile::read_log(&dir).unwrap();
                assert!(repaired.starts_with(&history), "original history must survive");
                assert!(std::fs::read_to_string(&path).unwrap().starts_with(&original), "original archive text must survive unchanged");
                if evidence == "local" {
                    assert!(crate::profile::selftest(&dir, project, &vec![], 300).unwrap().contains("no overdue"), "{closed}: {repaired}");
                    let record: serde_json::Value = serde_json::from_str(repaired.lines().last().unwrap().strip_prefix("taskq-stage ").unwrap()).unwrap();
                    assert_eq!(record["host"], "mac");
                    assert_eq!(record["stage"], "review");
                    assert_eq!(record["outcome"], if closed { "ok" } else { "cancelled" });
                    assert!(record["us"].is_null() && record["calls"].is_null() && record["tokens"].is_null());
                    s.pass().unwrap();
                    assert_eq!(crate::profile::read_log(&dir).unwrap(), repaired, "idempotent repair");
                } else {
                    assert_eq!(repaired, history, "{closed}: {evidence} must stay unknown");
                    assert!(crate::profile::selftest(&dir, project, &vec![], 300).unwrap().contains("WARN"));
                }
                if attempt_log.is_file() { assert_eq!(std::fs::read_to_string(attempt_log).unwrap(), if evidence == "empty-log" { "" } else { "original reviewer output\n" }); }
                assert!(!b.comments(n).unwrap().iter().any(|c| c.body.starts_with("taskq: gone")));
                drop(s);drop(_guard);drop(_capture);
            }
        }
    }

    // Chain: § 7 worker exit to review. Risk: a silent exit is not gone; a result gets no reviewer.
    #[test]
    fn exit_without_result_is_gone_and_result_brings_a_reviewer() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let c = cfg("mac");
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        b.comment(n, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(a), "ok")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!((t.state, t.role.as_deref(), t.retries), (State::Review, Some("reviewer"), 0));
        let r = t.attempt.unwrap();
        *e.borrow()[1].1.borrow_mut() = Some(1);
        s.pass().unwrap(); // reviewer exits without a verdict: gone keeps the result
        let t = task(&b, n);
        assert_eq!((t.state, t.retries, t.result.is_some()), (State::Review, 1, true));
        assert_ne!(t.attempt, Some(r));
    }

    // Chain: native terminal reviewer -> same-pass gone -> retained result and classified stage.
    // Risk: native failure detail is lost between adapter and stage record.
    #[test]
    fn terminal_reviewer_records_native_error_and_releases_lease() {
        struct Terminal { root: PathBuf }
        impl Proc for Terminal {
            fn exited(&mut self) -> Result<Option<i32>, String> { Ok(Some(-1)) }
            fn exit_error(&self) -> Option<&'static str> {
                Some(crate::runtime::claude_failure(&serde_json::json!({"id":"abcdef12", "state":"failed"}),
                    &self.root, "abcdef12", Some(&self.root)))
            }
            fn kill(&mut self) -> Result<(), String> { Ok(()) }
            fn id(&self) -> u32 { 0 }
        }
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker claude:invented win").unwrap();
        b.comment(n, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(a), "ok")).unwrap();
        release_execution(&b, n, a, a, "exit").unwrap();
        let c = cfg("win");
        let root = std::env::temp_dir().join(format!("taskq-native-failure-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(root.join("jobs/abcdef12")).unwrap();
        std::fs::write(root.join("jobs/abcdef12/state.json"), serde_json::json!({
            "cwd": root, "state":"failed", "detail":"ENAMETOOLONG: name too long, uv_spawn"
        }).to_string()).unwrap();
        let mut s = Service::new(&b, &c, "4.4.0", Box::new(|_| Ok(Box::new(Terminal { root: root.clone() }))));
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        let r = task(&b, n).attempt.unwrap();
        crate::profile::STAGED.with(|v| v.borrow_mut().clear());
        s.focus.clear();
        s.pass().unwrap();
        assert!(s.kids.is_empty());
        assert!(b.comments(n).unwrap().iter().any(|e| e.body.contains(&format!("taskq: gone attempt {r}"))));
        assert_eq!((task(&b, n).state, task(&b, n).result.is_some()), (State::Review, true));
        crate::profile::STAGED.with(|v| assert!(v.borrow().contains(&format!("review {n} {r} claude failed argument-too-long"))));
        let failure = |row: serde_json::Value, id: &str, project: &std::path::Path| crate::runtime::claude_failure(&row, project, id, Some(&root));
        assert_eq!(failure(serde_json::json!({"state":"failed", "detail":"permission denied"}), "abcdef12", &root), "permission");
        assert_eq!(failure(serde_json::json!({"state":"stopped"}), "abcdef12", &root), "other");
        assert_eq!(failure(serde_json::json!({"state":"failed"}), "../abcdef12", &root), "other");
        assert_eq!(failure(serde_json::json!({"state":"failed"}), "abcdef12", &root.join("jobs")), "other");
        std::fs::remove_file(root.join("jobs/abcdef12/state.json")).unwrap();
        assert_eq!(failure(serde_json::json!({"state":"failed"}), "abcdef12", &root), "other");
        std::fs::remove_dir_all(&root).unwrap();
    }

    // Chain: § 3 claim across hosts to § 2 retry limit (#913, #1123). Risk: two hosts claim one result's review, the
    // loser writes more than its spawn, two reviewers run, or the limit parks the task on a card without options.
    #[test]
    fn lost_reviewer_claim_writes_nothing_more_and_the_gone_limit_asks_with_options() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let w = b.comment(n, &core::entry("spawn", &["worker", "claude:s", "linux"], None, "")).unwrap();
        b.comment(n, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(w), "ok")).unwrap();
        release_execution(&b, n, w, w, "exit").unwrap();
        let (cm, cw) = (cfg("mac"), cfg("win"));
        let (em, ew): (Exits, Exits) = Default::default();
        let (mut mac, mut win) = (service(&b, &cm, em.clone()), service(&b, &cw, ew.clone()));
        // mac reads the task in review before win's spawn lands: both hosts post a reviewer spawn for one result
        let stale = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300).unwrap().0.into_iter().find(|v| v.issue.number == n).unwrap();
        win.pass().unwrap();
        let r = task(&b, n).attempt.unwrap();
        let (labels, comments) = (b.issue(n).unwrap().labels, b.comments(n).unwrap().len());
        mac.claim(&stale, "reviewer", "claude", "").unwrap();
        // Fresh ownership read prevents even a losing spawn for the stale view.
        assert_eq!((b.issue(n).unwrap().labels, b.comments(n).unwrap().len(), mac.kids.len(), em.borrow().len()), (labels, comments, 0, 0));
        assert_eq!((task(&b, n).attempt, task(&b, n).state), (Some(r), State::Review));
        // A3: later passes of both hosts start no second reviewer for this result
        mac.pass().unwrap();
        win.pass().unwrap();
        assert_eq!((em.borrow().len(), ew.borrow().len(), task(&b, n).attempt), (0, 1, Some(r)));
        // the reviewer exits twice without a verdict: the 2nd gone asks with the limit card
        *ew.borrow()[0].1.borrow_mut() = Some(1);
        win.pass().unwrap();
        win.pass().unwrap(); // the gone task is left alone for the rest of its pass
        assert_eq!((task(&b, n).state, task(&b, n).retries, ew.borrow().len()), (State::Review, 1, 2));
        *ew.borrow()[1].1.borrow_mut() = Some(1);
        win.pass().unwrap();
        let t = task(&b, n);
        let card = t.card.clone().unwrap();
        assert_eq!((t.state, t.retries, t.attempt), (State::Ask, 2, None));
        assert!(card.starts_with("exit 1") && card.contains("Problem: retry limit 2 reached by this gone"), "{card}");
        assert_eq!(core::options(&card).into_iter().map(|o| o.1.split(':').next().unwrap().to_string()).collect::<Vec<_>>(), ["requeue", "later"]);
        win.pass().unwrap(); // labels follow the fold on the next pass (§ 2)
        assert!(b.issue(n).unwrap().labels.contains(&"q-ask".to_string()), "{:?}", b.issue(n).unwrap().labels);
    }

    // Chain: § 2 retry limit after a failed start (#1123). Risk: a start failure whose comment read fails posts a
    // gone without a fresh fold, so the limit parks the task on a card without options.
    #[test]
    fn start_failure_with_failed_read_posts_nothing_and_the_next_pass_asks_with_the_card() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let w = b.comment(n, &core::entry("spawn", &["worker", "claude:s", "linux"], None, "")).unwrap();
        b.comment(n, &core::entry("gone", &[], Some(w), "exit 1")).unwrap();
        release_execution(&b, n, w, w, "exit").unwrap();
        assert_eq!((task(&b, n).state, task(&b, n).retries), (State::Ready, 1));
        let c = cfg("mac");
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(|_: &Launch| {
            b.state.borrow_mut().fail_comment_reads = 1; // the read after the failed start fails
            Err(crate::runtime::StartError::io(std::io::Error::other("boom"), Some("claude")))
        }));
        s.focus = vec!["all".into()];
        let comments = b.comments(n).unwrap().len();
        let e = s.pass().unwrap_err();
        assert!(e.contains("comment read failed"), "{e}");
        assert_eq!((b.comments(n).unwrap().len(), task(&b, n).state), (comments + 1, State::Doing), "only the spawn");
        s.pass().unwrap();
        let t = task(&b, n);
        let card = t.card.clone().unwrap();
        assert_eq!((t.state, t.retries), (State::Ask, 2));
        assert!(card.contains("confirmed NoStart") && card.contains("Problem: retry limit 2 reached by this gone"), "{card}");
        assert_eq!(core::options(&card).len(), 2);
    }

    // Chain: #1903 claim readback keeps missing the spawn (cs#528). Risk: the service posts a new spawn on every pass.
    #[test]
    fn a_spawn_the_readback_misses_is_not_reposted_each_pass() {
        let c = cfg("mac");
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        let spawns = || b.comments(n).unwrap().iter().filter(|c| c.body.starts_with("taskq: spawn")).count();
        b.state.borrow_mut().hide_latest_comment = true;
        for _ in 0..3 { s.pass().unwrap(); }
        b.state.borrow_mut().hide_latest_comment = false;
        assert_eq!(spawns(), 1, "{:?}", s.log);
        // once the readback shows it, the spawn is the attempt
        s.pass().unwrap();
        assert_eq!((spawns(), task(&b, n).state), (1, State::Doing));
    }

    // Chain: #1879 (pilotq#133) claim readback lacks the won spawn -> no start, child or attempt log -> named wait for one
    // ttl -> released no-start -> gone -> ready; the repeat reaches the retry-limit card. Risk: the spawn stays doing /
    // starting with `Run state unknown` until the PM requeues it, or a spawn whose start was reached is released.
    #[test]
    fn lost_spawn_waits_one_ttl_then_ends_gone_and_a_reached_start_stays_unknown() {
        let dir = std::env::temp_dir().join(format!("taskq-lost-spawn-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(dir.join(".taskq")).unwrap();
        let mut c = cfg("mac");
        c.root = dir.clone();
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let other = b.create("note", "", &[]).unwrap();
        let exits = Rc::new(RefCell::new(vec![]));
        let mut s = service(&b, &c, exits.clone());
        let wait = |s: &Service| s.snapshot.as_ref().unwrap().json()["tasks"].as_array().unwrap().iter()
            .find(|r| r["n"] == n).map(|r| r["wait"].as_str().unwrap_or_default().to_string()).unwrap();
        for round in 1..=2 {
            // the lag hides only the newest comment: the spawn, never the previous round's gone
            b.comment(other, "unrelated").unwrap();
            b.state.borrow_mut().hide_latest_comment = true;
            s.pass().unwrap();
            b.state.borrow_mut().hide_latest_comment = false;
            let a = task(&b, n).attempt.unwrap_or_else(|| panic!("round {round}: no spawn won"));
            assert_eq!((task(&b, n).state, exits.borrow().len(), s.kids.len()), (State::Doing, 0, 0), "round {round}: start never called");
            s.pass().unwrap();
            assert!(s.execution_waits[&n].contains(&format!("spawn {a} not started on mac")), "round {round}: {:?}", s.execution_waits);
            assert_eq!(wait(&s), crate::snapshot::WAIT_TEXTS[2], "round {round}: named wait, not Run state unknown");
            b.advance(c.ttl);
            s.pass().unwrap();
            let log = entries(&b, n).unwrap();
            assert_eq!(core::executions(n, &log).iter().find(|x| x.id == a).unwrap().release, Some(core::Release::NoStart), "round {round}");
            let gone = b.comments(n).unwrap().into_iter().find(|c| c.body.starts_with(&format!("taskq: gone attempt {a}"))).expect("gone");
            assert!(gone.body.contains(&format!("lost spawn {a}")), "round {round}: {}", gone.body);
            assert!(s.log.iter().any(|l| l.starts_with(&format!("#{n} lost spawn {a}: no attempt log on mac"))), "round {round}: {:?}", s.log);
            let t = task(&b, n);
            if round == 1 {
                assert_eq!((t.state, t.retries, t.attempt), (State::Ready, 1, None));
            } else {
                assert_eq!((t.state, t.retries), (State::Ask, 2));
                assert!(t.card.unwrap().contains("Problem: retry limit 2 reached by this gone"));
            }
        }
        // A start that reached its native call (attempt log written) stays unknown after ttl: never released as no-start.
        let b = Fake::default();
        let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
        let a = b.comment(n, &core::entry("spawn", &["worker", "codex:-", "mac"], None, "")).unwrap();
        std::fs::write(crate::runtime::attempt_log(&dir, "worker", n, a), "").unwrap();
        let mut s = service(&b, &c, Rc::new(RefCell::new(vec![])));
        b.advance(c.ttl);
        s.pass().unwrap();
        assert_eq!((task(&b, n).state, task(&b, n).attempt), (State::Doing, Some(a)));
        assert_eq!(core::executions(n, &entries(&b, n).unwrap())[0].release, None);
        assert!(s.execution_waits[&n].contains("termination unconfirmed"), "{:?}", s.execution_waits);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: #1886 (pilotq#133) lost spawn on a retired host, then the PM requeues -> no host claims while it is unreleased;
    // one run of that host releases it no-start once and another host claims. Risk: the task stays unclaimable until a
    // retired host is restored by hand, a repeated run posts a second release, or a reached start is released.
    #[test]
    fn retired_host_run_releases_a_requeued_lost_spawn_and_a_reached_start_names_the_host_step() {
        let dir = std::env::temp_dir().join(format!("taskq-retired-spawn-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(dir.join(".taskq")).unwrap();
        let (mut mac, mut hermes) = (cfg("mac"), cfg("hermes"));
        (mac.root, hermes.root) = (dir.clone(), dir.clone());
        let b = Fake::default();
        let labels = [TASK_LABEL.to_string(), "host-mac".into()];
        let lost = b.create("lost", "", &labels).unwrap();
        let reached = b.create("reached", "", &labels).unwrap();
        let handed = b.create("handed", "", &labels).unwrap();
        let mut spawns = vec![];
        for n in [lost, reached, handed] {
            let a = b.comment(n, &core::entry("spawn", &["worker", "codex:-", "hermes"], None, "")).unwrap();
            // the child's own requeue proves its start like an attempt log does
            b.comment(n, &core::entry("requeue", &[], (n == handed).then_some(a), "Reason: hermes retired")).unwrap();
            spawns.push(a);
        }
        std::fs::write(crate::runtime::attempt_log(&dir, "worker", reached, spawns[1]), "").unwrap();
        let releases = |n| entries(&b, n).unwrap().iter().filter(|e| e.body.starts_with("taskq: released")).count();
        let mut m = service(&b, &mac, Rc::new(RefCell::new(vec![])));
        m.pass().unwrap();
        for n in [lost, reached] {
            assert_eq!((task(&b, n).state, task(&b, n).attempt), (State::Ready, None), "#{n}: unreleased execution blocks the claim");
            assert!(m.execution_waits[&n].ends_with("termination unconfirmed; start the taskq service on hermes once, after a reboot if the run had started"), "{:?}", m.execution_waits);
        }
        let mut h = service(&b, &hermes, Rc::new(RefCell::new(vec![])));
        for _ in 0..2 {
            h.pass().unwrap();
            assert_eq!(core::executions(lost, &entries(&b, lost).unwrap())[0].release, Some(core::Release::NoStart));
            assert_eq!([lost, reached, handed].map(releases), [1, 0, 0], "one fenced release; a reached start stays unknown (R12)");
        }
        assert!(h.log.iter().any(|l| l.starts_with(&format!("#{lost} lost spawn {}: no attempt log on hermes", spawns[0]))), "{:?}", h.log);
        assert!(h.execution_waits[&reached].ends_with("termination unconfirmed; reboot hermes to prove exit"), "{:?}", h.execution_waits);
        m.pass().unwrap();
        let t = task(&b, lost);
        assert_eq!((t.state, host_of(&t)), (State::Doing, Some("mac")), "claimable elsewhere");
        assert_eq!((task(&b, reached).attempt, task(&b, handed).attempt), (None, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: § 4 Filter, worker runtime admission over tests/golden/admit.json. Risk: unpinned work stalls with free Codex, or fallback overrides an owner pin.
    #[test]
    fn any_runtime_admission_follows_golden_traces() {
        let traces: Vec<serde_json::Value> = serde_json::from_str(include_str!("../tests/golden/admit.json")).unwrap();
        let rts = |v: &serde_json::Value| -> Vec<(String, u32)> { v.as_object().unwrap().iter().map(|(k, n)| (k.clone(), n.as_u64().unwrap() as u32)).collect() };
        let mut first_free_fails = false;
        for tr in &traces {
            let (limits, running, tasks) = (rts(&tr["limits"]), rts(&tr["running"]), tr["tasks"].as_u64().unwrap());
            let want: Vec<Option<String>> = serde_json::from_value(tr["expect"].clone()).unwrap();
            let b = Fake::default();
            // Running children hold doing tasks of this host.
            let held: Vec<(u64, u64, String)> = running.iter().flat_map(|(rt, n)| (0..*n).map(move |_| rt.clone())).map(|rt| {
                let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
                (n, b.comment(n, &format!("taskq: spawn worker {rt}:s mac")).unwrap(), rt)
            }).collect();
            let mut labels = vec![TASK_LABEL.to_string()];
            labels.extend(serde_json::from_value::<Vec<String>>(tr.get("labels").cloned().unwrap_or(json!([]))).unwrap());
            let ns: Vec<u64> = (0..tasks).map(|_| b.create("T", "", &labels).unwrap()).collect();
            let mut c = cfg("mac");
            c.limits = limits.iter().cloned().collect();
            c.default_runtime = tr["default"].as_str().unwrap_or("claude").into();
            let mut s = service(&b, &c, Default::default());
            for (task, attempt, rt) in held {
                s.kids.push(Kid { unknown:None, stopping:false, session_url:None, execution: attempt, task, attempt, rt, proc: Box::new(FakeProc(Rc::new(RefCell::new(None)))) });
            }
            s.pass().unwrap();
            let got: Vec<Option<String>> = ns.iter().map(|&n| rt_of(&task(&b, n)).map(|(rt, _)| rt.to_string())).collect();
            assert_eq!(got, want, "{}", tr["name"]);
            let ignored = s.log.iter().filter(|l| l.contains("conflicting prefer-*")).count();
            assert_eq!(ignored, tr["ignored"].as_u64().unwrap_or(0) as usize, "{}", tr["name"]);
            // The old rule: every task takes the first listed runtime with a free slot.
            let mut used: BTreeMap<String, u32> = running.iter().cloned().collect();
            let old: Vec<Option<String>> = (0..tasks).map(|_| limits.iter().find(|(rt, n)| used.get(rt).copied().unwrap_or(0) < *n).map(|(rt, _)| {
                *used.entry(rt.clone()).or_default() += 1;
                rt.clone()
            })).collect();
            first_free_fails |= old != want;
        }
        assert!(first_free_fails, "the traces must reject the first-free rule");
    }

    // Chain: §4 worker result to unpinned review. Risk: reviews stall on unavailable Claude or override explicit pins.
    #[test]
    fn reviewer_prefers_claude_and_falls_back_to_codex() {
        for (worker, claude, expected) in [
            ("codex", 1, Some("claude")),
            ("codex", 0, Some("codex")),
            ("claude", 1, Some("claude")),
            ("codex-app", 1, Some("claude")),
            ("codex-app", 0, Some("codex")),
        ] {
            let b = Fake::default();
            let n = b.create("T", "", &["taskq".into()]).unwrap();
            let mut c = cfg("mac");
            // The worker preference is separate from the subsequent unpinned review preference.
            c.limits = BTreeMap::from([(worker.into(), 1), ("codex".into(), 1), ("claude".into(), claude.max(u32::from(worker == "claude")))]);
            c.has = vec!["codex-app".into()];
            let e: Exits = Default::default();
            let mut s = service(&b, &c, e.clone());
            s.cfg.default_runtime = worker.into();
            s.pass().unwrap();
            let t = task(&b, n);
            if worker == "codex-app" && !cfg!(any(target_os = "macos", windows)) {
                assert_eq!(t.state, State::Ready, "§ 9: app workers are macOS and Windows only");
                assert!(t.attempt.is_none() && e.borrow().is_empty(), "no app worker claimed or started");
                continue;
            }
            assert_eq!(rt_of(&t).unwrap().0, worker);
            s.cfg.default_runtime = "claude".into();
            // A losing spawn must not change which runtime actually did the work.
            b.comment(n, &core::entry("spawn", &["worker", if worker == "claude" { "codex:ignored" } else { "claude:ignored" }, "mac"], None, "")).unwrap();
            b.comment(n, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], t.attempt, "ok")).unwrap();
            *e.borrow()[0].1.borrow_mut() = Some(0);
            for retry in 0..2 {
                s.pass().unwrap();
                let t = task(&b, n);
                assert_eq!(t.state, State::Review);
                assert_eq!((t.role.as_deref() == Some("reviewer")).then(|| rt_of(&t).unwrap().0), expected, "{worker}, claude {claude}, retry {retry}");
                if retry == 0 && expected.is_some() {
                    *e.borrow().last().unwrap().1.borrow_mut() = Some(1);
                    s.pass().unwrap(); // gone ends the reviewer; the next pass claims its replacement
                    assert_eq!(task(&b, n).attempt, None);
                }
            }
        }
    }

    // A2-A4, R2/R9/R12: worker/reviewer admission, quota fallback and same-session resume; never edit capacity or override pins/grants.
    #[test]
    fn antigravity_admission_and_resume_keep_ownership() {
        for reviewer in [false, true] {
            for why in ["quota", "pin", "disabled", "grant"] {
                let b = Fake::default();
                let root = std::env::temp_dir().join(format!("taskq-antigravity-{}", crate::runtime::uuid()));
                std::fs::create_dir_all(root.join(".taskq")).unwrap();
                let mut c = cfg("mac"); c.root=root.clone(); c.install=root.join("install");
                c.limits=BTreeMap::from([("claude".into(),1),("codex".into(),0),("antigravity".into(),1)]);
                let mut labels=vec!["taskq".into()];
                if why=="pin" {labels.push("run-claude".into());}
                let n=b.create("Invented Antigravity admission", "", &labels).unwrap();
                if reviewer {
                    let a=b.comment(n,"taskq: spawn worker codex:fixture mac").unwrap();
                    b.comment(n,&core::entry("result",&[&"a".repeat(40)],Some(a),"fixture candidate")).unwrap();
                    release_execution(&b,n,a,a,"exit").unwrap();
                }
                let launches=Rc::new(RefCell::new(vec![])); let calls=launches.clone();
                let exits:Exits=Default::default(); let running=exits.clone();
                let mut s=Service::new(&b,&c,"fixture",Box::new(move |l| {
                    calls.borrow_mut().push((l.rt.clone(),l.session.clone(),l.resume.is_some()));
                    let flag=Rc::new(RefCell::new(None));running.borrow_mut().push((l.task,flag.clone()));
                    Ok(Box::new(FakeProc(flag)))
                }));
                s.focus=vec!["all".into()];
                s.limited.insert("claude".into(),(i64::MAX,true));
                if why=="disabled" {s.cap.as_mut().unwrap().limits.insert("antigravity".into(),0);}
                if why=="grant" {s.refused_access.insert("antigravity".into(),"no operator grant".into());}
                let capacity=s.cap.as_ref().unwrap().limits.clone();
                for _ in 0..2 {s.pass().unwrap();}
                assert_eq!(s.cap.as_ref().unwrap().limits,capacity,"quota cannot rewrite operator capacity");
                if why!="quota" {
                    assert!(launches.borrow().is_empty(),"{reviewer} {why}");
                    let row=s.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().json();
                    assert!(row["admission"]["reason"].as_str().unwrap().contains(if why=="pin" {"claude"} else {"antigravity"}));
                } else {
                    assert_eq!(*launches.borrow(),[("antigravity".into(),"-".into(),false)]);
                    let t=task(&b,n);let a=t.attempt.unwrap();
                    assert_eq!(t.role.as_deref(),Some(if reviewer {"reviewer"} else {"worker"}));
                    let path=crate::runtime::attempt_log(&root,t.role.as_deref().unwrap(),n,a);
                    std::fs::write(path,"{\"conversationId\":\"agy-owned\"}\n").unwrap();
                    b.comment(n,&core::entry("ask",&[],Some(a),"fixture question")).unwrap();
                    *exits.borrow()[0].1.borrow_mut()=Some(0);s.pass().unwrap();
                    b.comment(n,"taskq: answer\ncontinue").unwrap();s.pass().unwrap();
                    assert_eq!(launches.borrow().last().unwrap(), &("antigravity".into(),"agy-owned".into(),true));
                    assert_eq!(launches.borrow().len(),2,"only one new turn for the answer");
                    assert_eq!(task(&b,n).attempt,Some(a));
                }
                drop(s);std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    // Defect #2118 (R8/R12/R13, §4/§9): unavailable Claude stalls new work/review despite eligible Codex.
    // Protection: use the actual admission/claim/snapshot chain; pins, grants, common gates and live ownership bind.
    #[test]
    fn fallback_2118_isolated_admission() {
        for reviewer in [false, true] {
            for why in ["disabled", "full", "quota", "login", "missing", "unknown-login", "both", "pin", "grant", "focus", "stop", "load", "gui", "gui-grant", "gui-ok", "codex-unknown", "unresolved"] {
                let b = Fake::default();
                let root = std::env::temp_dir().join(format!("taskq-2118-{}", crate::runtime::uuid()));
                std::fs::create_dir_all(&root).unwrap();
                let mut c = cfg("mac");
                c.root = root.clone(); c.install = root.join("install");
                c.limits = BTreeMap::from([("claude".into(), 0), ("codex".into(), 1)]);
                let mut labels = vec!["taskq".into()];
                if why == "pin" { labels.push("run-claude".into()); }
                if why.starts_with("gui") { labels.push("needs-gui".into()); }
                let n = b.create("Invented isolated admission", "", &labels).unwrap();
                if reviewer {
                    let a = b.comment(n, "taskq: spawn worker claude:invented mac").unwrap();
                    b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "fixture candidate")).unwrap();
                    release_execution(&b, n, a, a, "exit").unwrap();
                }
                let exits: Exits = Default::default();
                let mut s = service(&b, &c, exits.clone());
                let mut probes = json!([{"runtime":"claude","login":"logged in"},{"runtime":"codex","login":"logged in"}]);
                match why {
                    "quota" => { s.limited.insert("claude".into(), (i64::MAX, true)); }
                    "login" => { s.logged_out.insert("claude".into()); }
                    "unknown-login" => { probes[0]["login"] = json!("unknown"); s.cap.as_mut().unwrap().limits.insert("claude".into(), 1); }
                    "missing" => { s.refused_access.insert("claude".into(), "claude CLI not found on the service PATH".into()); }
                    "both" => { s.logged_out.insert("codex".into()); }
                    "grant" => { s.refused_access.insert("codex".into(), "no operator grant".into()); }
                    "focus" => s.focus.clear(),
                    "codex-unknown" => { probes[1]["login"] = json!("unknown"); }
                    "stop" => { std::fs::create_dir_all(&c.install).unwrap(); std::fs::write(c.install.join("hold"), "fixture stop").unwrap(); }
                    "load" => { s.reload(Ok(Capacity::fixed(c.limits.clone(), on()))); }
                    "gui-grant" | "gui-ok" => {
                        s.gui_probe = Ok(()); s.gui_checked = Some(std::time::Instant::now());
                        s.full.insert(if why == "gui-ok" { "codex" } else { "claude" }.into());
                    }
                    "unresolved" => {
                        let a = b.comment(n, &format!("taskq: spawn {} claude:invented elsewhere", if reviewer {"reviewer"} else {"worker"})).unwrap();
                        b.comment(n, &core::entry("requeue", &[], None, "fixture unknown owner")).unwrap();
                        assert!(a > 0);
                    }
                    "full" => {
                        s.cap.as_mut().unwrap().limits.insert("claude".into(), 1);
                        let busy = b.create("Invented active work", "", &["taskq".into()]).unwrap();
                        let a = b.comment(busy, "taskq: spawn worker claude:busy mac").unwrap();
                        s.kids.push(Kid { unknown: None, stopping: false, session_url: None, execution: a, task: busy,
                            attempt: a, rt: "claude".into(), proc: Box::new(FakeProc(Rc::new(RefCell::new(None)))) });
                    }
                    _ => {}
                }
                s.logged_out.extend(crate::login_holds(&probes));
                s.pass().unwrap();
                let blocked = ["both", "pin", "grant", "focus", "stop", "load", "gui", "gui-grant", "unresolved"].contains(&why);
                assert_eq!(exits.borrow().len(), usize::from(!blocked), "{reviewer} {why}");
                if !blocked {
                    let t = task(&b, n);
                    assert_eq!(rt_of(&t).unwrap().0, if why == "unknown-login" { "claude" } else { "codex" }, "{reviewer} {why}");
                    assert_eq!(t.role.as_deref(), Some(if reviewer {"reviewer"} else {"worker"}));
                    let spawn = b.comments(n).unwrap().into_iter().find(|e| Some(e.id) == t.attempt).unwrap();
                    if why == "unknown-login" {
                        assert!(!spawn.body.contains("fallback:"), "unknown login alone must not change runtime");
                    } else {
                        assert!(spawn.body.contains("fallback:") && spawn.body.contains("selected codex"), "{}", spawn.body);
                    }
                    if why == "unknown-login" || why == "codex-unknown" {
                        assert!(crate::runtime::login_lines(&probes).iter().any(|line| line.contains("login unknown")), "probe must stay unknown");
                    }
                    let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
                    assert_eq!(row["admission"]["code"], "admitted");
                    assert!(row["admission"]["reason"].as_str().unwrap().contains(if why == "unknown-login" { "claimed on claude" } else { "selected codex" }));
                    s.pass().unwrap();
                    assert_eq!(task(&b, n).attempt, t.attempt, "no duplicate or migration");
                    assert_eq!(exits.borrow().len(), 1);
                    if why == "unknown-login" || why == "codex-unknown" {
                        if why == "codex-unknown" {
                            let path = crate::runtime::attempt_log(&c.root, t.role.as_deref().unwrap(), n, t.attempt.unwrap());
                            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                            std::fs::write(path, "{\"type\":\"thread.started\",\"thread_id\":\"fixture-native-session\"}\n").unwrap();
                        }
                        b.comment(n, &core::entry("ask", &[], t.attempt, "fixture pause")).unwrap();
                        *exits.borrow()[0].1.borrow_mut() = Some(0);
                        s.pass().unwrap();
                        b.comment(n, &core::entry("answer", &[], None, "continue")).unwrap();
                        s.pass().unwrap();
                        assert_eq!(task(&b, n).attempt, t.attempt, "unknown login must preserve the attempt on resume");
                        assert_eq!(task(&b, n).session, t.session, "unknown login must preserve the session/runtime on resume");
                        assert_eq!(exits.borrow().len(), 2, "the ended turn resumes once");
                    }
                } else if why == "both" || why == "grant" {
                    let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
                    let reason = row["admission"]["reason"].as_str().unwrap();
                    assert!(reason.contains("claude") && reason.contains("codex"), "{reason}");
                    s.logged_out.clear(); s.refused_access.clear();
                    s.pass().unwrap();
                    assert_eq!(rt_of(&task(&b, n)).unwrap().0, "codex", "automatic re-evaluation");
                }
                drop(s); std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    // A7/R12: real service admission -> installed Codex command -> native identity/exit, without a model turn.
    // Board and capacity are isolated; read existing grants/options/login, never write host policy or submit a prompt.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "installed Codex admission qualification; no prompt/model or live board"]
    fn fallback_2118_native_admission_without_model() {
        use std::process::Stdio;
        let install = crate::platform::default_install();
        let host = crate::host::Policy::read(&install).unwrap();
        let agents = crate::runtime::agents(&install);
        assert!(agents.as_array().unwrap().iter().any(|r| r["runtime"] == "codex" && r["login"] == "logged in"), "native proof requires a confirmed Codex login");
        for reviewer in [false, true] {
            let root = std::env::temp_dir().join(format!("taskq-qualification-fallback-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            let mut c = cfg("qualification");
            host.apply(&mut c); // preserve actual model/effort/permission options
            c.root = root.clone(); c.install = root.join("install"); c.external = true; c.claim = crate::host::ClaimMode::Any;
            c.default_runtime = "claude".into(); // staged preference, not a host policy edit
            c.limits = BTreeMap::from([("claude".into(), 0), ("codex".into(), 1)]);
            let env = crate::runtime::Env { context: "", root: &root, exe: std::path::Path::new("taskq"), install: &install,
                permission_mode: &c.permission_mode, codex: &c.codex, publish: "pr", board_env: &[], jobs: None };
            let b = Fake::default();
            let n = b.create("TaskQ Qualification fallback", "", &["taskq".into()]).unwrap();
            if reviewer {
                let a = b.comment(n, "taskq: spawn worker claude:qualification qualification").unwrap();
                b.comment(n, &core::entry("result", &[&"a".repeat(40)], Some(a), "isolated candidate")).unwrap();
                release_execution(&b, n, a, a, "exit").unwrap();
            }
            let launches = std::cell::Cell::new(0);
            let mut s = Service::new(&b, &c, "qualification", Box::new(|l| {
                launches.set(launches.get() + 1);
                assert_eq!(l.rt, "codex");
                let access = crate::runtime::access(&env, &l.rt)?;
                let mut command = crate::runtime::command(&env, l, &access)?;
                let output = std::fs::File::create(root.join("native.log")).unwrap();
                command.stdin(Stdio::piped()).stdout(output).stderr(std::fs::File::create(root.join("native.stderr")).unwrap());
                // Keep stdin open in Child: launch the actual executor, stop before input/model submission.
                command.spawn().map(|p| Box::new(p) as Box<dyn Proc>).map_err(|e| crate::runtime::StartError::io(e, Some("codex")))
            }));
            s.focus = vec!["all".into()];
            s.logged_out = crate::login_holds(&agents);
            s.refused_access = crate::policy::report(&install, &root, &c.permission_mode, &c.codex).into_iter()
                .filter_map(|e| e.refused.map(|r| (e.runtime, r))).collect();
            for rt in crate::runtime::agent_runtimes() {
                if let Some(r) = crate::runtime::missing_cli(rt) { s.refused_access.insert(rt.to_string(), r); }
            }
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                s.pass().unwrap();
                assert_eq!(s.kids.len(), 1, "{:?}", s.snapshot.as_ref().map(|s| s.tasks.iter().map(|r| r.json()).collect::<Vec<_>>()));
                let identity = s.kids[0].proc.native_identity().unwrap();
                assert_eq!(s.kids[0].proc.exited().unwrap(), None);
                let attempt = task(&b, n).attempt;
                s.pass().unwrap();
                assert_eq!(launches.get(), 1, "live execution cannot migrate or duplicate");
                assert_eq!(task(&b, n).attempt, attempt);
                let spawn = b.comments(n).unwrap().into_iter().find(|e| Some(e.id) == attempt).unwrap();
                assert!(spawn.body.contains("fallback:") && spawn.body.contains("selected codex"));
                let codex = agents.as_array().unwrap().iter().find(|r| r["runtime"] == "codex").unwrap();
                println!("role={} version={} pid={} creation={:?} grant={} launches=1; {}",
                    if reviewer {"reviewer"} else {"worker"}, codex["version"], identity.pid, identity,
                    crate::runtime::access(&env, "codex").unwrap().line(), spawn.body);
            }));
            // End this fixture's attempt first (R11), then stop only its owned native process, even after failure.
            b.comment(n, "taskq: later\nReason: qualification complete").unwrap();
            for kid in &mut s.kids {
                let identity = kid.proc.native_identity().ok();
                kid.proc.kill().unwrap();
                let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while kid.proc.exited().unwrap().is_none() && std::time::Instant::now() < until { std::thread::sleep(std::time::Duration::from_millis(10)); }
                assert!(kid.proc.exited().unwrap().is_some(), "native exit unconfirmed");
                if let Some(identity) = identity { assert_eq!(coreq::process::identity(identity.pid).unwrap(), None); }
                println!("pid={} native exit confirmed; no prompt/model submitted", kid.proc.id());
            }
            drop(s);
            assert!(std::fs::read(root.join("native.log")).unwrap_or_default().is_empty(), "no turn without input");
            std::fs::remove_dir_all(root).unwrap();
            if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
        }
    }

    // Chain: § 4 Filter, host.json `default_runtime` codex (#1668) -> worker and reviewer claims -> verdict -> close.
    // Risk: a host without claude routes its reviews or unlabelled work to claude and waits forever, a full codex
    // review moves to a free claude slot, or a codex review takes another path to close.
    #[test]
    fn codex_default_host_runs_work_and_review_on_codex() {
        for (labels, codex, worker, expected) in [
            (vec![], 1, "codex", Some("codex")),
            (vec!["run-claude".to_string()], 1, "claude", Some("claude")),
            (vec!["run-claude".to_string()], 0, "claude", Some("claude")),
        ] {
            let b = Fake::default();
            let n = b.create("T", "", &[vec!["taskq".to_string()], labels.clone()].concat()).unwrap();
            let mut c = Config { default_runtime: "codex".into(), ..cfg("fra") };
            c.limits = BTreeMap::from([("claude".into(), 1), ("codex".into(), codex)]);
            let e: Exits = Default::default();
            let mut s = service(&b, &c, e.clone());
            s.pass().unwrap();
            let t = task(&b, n);
            assert_eq!(rt_of(&t).unwrap().0, worker, "{labels:?}: run-* wins, else the host default");
            b.comment(n, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], t.attempt, "ok")).unwrap();
            *e.borrow()[0].1.borrow_mut() = Some(0);
            s.pass().unwrap();
            s.pass().unwrap();
            let t = task(&b, n);
            assert_eq!(t.state, State::Review);
            assert_eq!((t.role.as_deref() == Some("reviewer")).then(|| rt_of(&t).unwrap().0), expected, "{labels:?}, codex {codex}: claude stays free");
            if expected.is_none() { continue; }
            let verdict = "Changed: codex review
Checks: Task fit A1 ok; merge-result CI ok
Open: none";
            b.comment(n, &core::entry("publish", &[], t.attempt, verdict)).unwrap();
            *e.borrow().last().unwrap().1.borrow_mut() = Some(0);
            s.pass().unwrap();
            s.pass().unwrap();
            assert_eq!(task(&b, n).state, State::Closed, "{labels:?}");
            let log = entries(&b, n).unwrap();
            assert!(log.iter().any(|e| core::kind(n, &e.body) == "close" && core::verdict(n, &log, core::entry_text(n, &e.body)).contains(verdict)));
        }
    }

    // Chain: § 2 answer and requeue on a live child. Risk: a new session on answer; requeue leaves the child running.
    #[test]
    fn answer_resumes_the_same_session_and_requeue_stops_the_child() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        c.root = std::env::temp_dir().join(format!("taskq-session-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(c.root.join(".taskq")).unwrap();
        let e: Exits = Default::default();
        let sessions = Rc::new(RefCell::new(Vec::new()));
        let seen = sessions.clone();
        let exits = e.clone();
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |l| {
            seen.borrow_mut().push(l.session.clone());
            let flag = Rc::new(RefCell::new(None));
            exits.borrow_mut().push((l.task, flag.clone()));
            Ok(Box::new(FakeProc(flag)))
        }));
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        std::fs::write(c.root.join(format!(".taskq/T{n}-{a}.log")), "{\"type\":\"thread.started\",\"thread_id\":\"native-session\"}\n").unwrap();
        b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Ask);
        // free comments, the owner's or not, move nothing (§ 2)
        b.raw(n, "prefer z", true, false);
        b.raw(n, "do x", false, false);
        s.pass().unwrap();
        assert_eq!((task(&b, n).state, e.borrow().len()), (State::Ask, 1), "a free comment moves nothing");
        b.comment(n, &core::entry("answer", &["2"], None, "y")).unwrap();
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), 2, "resumed once");
        assert_eq!(sessions.borrow()[1], "native-session", "resume resolves the pre-launch alias");
        let t = task(&b, n);
        assert_eq!((t.state, t.attempt), (State::Doing, Some(a)));
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), 2, "an answer is delivered once");
        b.comment(n, &core::entry("requeue", &[], None, "redo")).unwrap();
        s.focus = vec![];
        s.pass().unwrap();
        assert_eq!(*e.borrow()[1].1.borrow(), Some(-9));
        assert!(s.kids.is_empty());
        std::fs::remove_dir_all(&c.root).unwrap();
    }

    // Chain: § 8 Access refusal. Risk: a refused runtime is claimed (phantom attempt, retries) or resumed, or its
    // reason is missing from status.
    #[test]
    fn refused_access_claims_and_resumes_nothing() {
        let b = Fake::default();
        let mut c = cfg("mac");
        c.limits = BTreeMap::from([("claude".to_string(), 2)]);
        let n1 = b.create("A", "", &["taskq".into()]).unwrap();
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a1 = task(&b, n1).attempt.unwrap();
        b.comment(n1, &core::entry("ask", &[], Some(a1), "Which?\n1. x\n2. y ★")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        s.refused_access.insert("claude".into(), "workspace enforcement unknown for claude on windows".into());
        b.comment(n1, &core::entry("answer", &["1"], None, "x")).unwrap();
        let n2 = b.create("B", "", &["taskq".into()]).unwrap();
        s.pass().unwrap();
        assert_eq!((task(&b, n2).state, task(&b, n2).attempt, task(&b, n1).state, e.borrow().len()), (State::Ready, None, State::Doing, 1));
        assert!(!b.comments(n2).unwrap().iter().any(|c| c.body.starts_with("taskq: spawn")), "no phantom attempt");
        let cap = s.capacity_json();
        assert_eq!((cap["reasons"][0]["code"].as_str(), cap["reasons"][0]["text"].as_str()), (Some("access"), Some("claude: workspace enforcement unknown for claude on windows")));
        s.refused_access.clear();
        s.pass().unwrap();
        assert_eq!((task(&b, n2).state, e.borrow().len()), (State::Doing, 3), "granted: resume and claim");
    }

    // Chain: usage limit before work -> `usage-limit` -> held runtime -> reset -> admission (#1279). Risk: a
    // limit spends the gone budget (#1276), another eligible runtime waits, the hold outlives its reset or hides
    // from status, or a limit after work destroys its attempt/session instead of waiting for quota.
    #[test]
    fn usage_limit_holds_the_runtime_until_reset_and_keeps_retries() {
        let root = std::env::temp_dir().join(format!("taskq-usage-limit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let b = Fake::default();
        let mut c = Config { root: root.clone(), install: root.join("install"), ..cfg("mac") };
        c.limits = BTreeMap::from([("claude".to_string(), 1), ("codex".to_string(), 1)]);
        let reset = 1000;
        // the next launch's log: before-work limit, after-work limit, or plain work
        let next: Rc<RefCell<&str>> = Rc::new(RefCell::new("limit"));
        let e: Exits = Default::default();
        let (seen, r, kind) = (e.clone(), root.clone(), next.clone());
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            let limit = format!("{{\"type\":\"assistant\",\"apiError\":\"usage_limit_reached\",\"quotaLimits\":{{\"status\":\"rejected\",\"resetsAt\":{reset}}}}}\n");
            let work = "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\"}]}}\n";
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, match *kind.borrow() { "limit" => limit, "late" => format!("{work}{limit}"), _ => work.into() }).unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        let any = b.create("A", "", &["taskq".into()]).unwrap();
        s.pass().unwrap();
        assert!(task(&b, any).session.unwrap().starts_with("claude:"), "unlabeled work runs on claude");
        let only = b.create("B", "", &["taskq".into(), "run-claude".into()]).unwrap();
        *next.borrow_mut() = "work";
        *e.borrow()[0].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        let gone = b.comments(any).unwrap().into_iter().rfind(|c| c.body.starts_with("taskq: usage-limit")).unwrap().body;
        assert!(gone.starts_with("taskq: usage-limit attempt ") && gone.contains("\nUsage limit: claude until 1970-01-01T00:16:40Z"), "{gone}");
        assert_eq!((task(&b, any).state, task(&b, any).retries), (State::Ready, 0), "A2: the gone budget stays");
        assert_eq!(core::executions(any,&entries(&b,any).unwrap()).last().unwrap().usage_hold,Some((reset,true)),"release persists the deadline before outcome publication");
        s.pass().unwrap();
        assert_eq!((task(&b, any).state, e.borrow().len()), (State::Doing, 2), "§4: released unpinned work automatically falls back to Codex");
        assert!(task(&b, any).session.unwrap().starts_with("codex:"), "A4: work for another runtime starts");
        assert_eq!((task(&b, only).state, e.borrow().len()), (State::Ready, 2), "a claude-only task waits for the reset");
        let cap = s.capacity_json();
        assert!(cap["reasons"].as_array().unwrap().iter().any(|r| r["code"] == "usage-limit" && r["text"] == "claude: until 1970-01-01T00:16:40Z"), "{cap}");
        assert!(s.heartbeat_body().contains("\nlimit claude until 1970-01-01T00:16:40Z"));
        let text = s.snapshot.as_ref().unwrap().text(&c, &Value::Null).unwrap();
        assert!(text.contains(" claude limit until 1970-01-01T00:16:40Z"), "A3: {text}");

        b.state.borrow_mut().clock = reset;
        *next.borrow_mut() = "late";
        s.pass().unwrap();
        assert_eq!((task(&b, only).state, e.borrow().len()), (State::Doing, 3), "A5: the reset admits the held work");
        assert!(s.limited.is_empty());
        *e.borrow()[2].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        let kept = task(&b,only);
        assert_eq!((kept.state,kept.retries),(State::Doing,0),"#2137 A7: mid-work quota keeps work and its attempt");
        assert!(b.comments(only).unwrap().iter().any(|c|c.body.starts_with("taskq: steer quota attempt")&&c.body.contains("same task and native session")));
        assert!(!b.comments(only).unwrap().iter().any(|c|c.body.starts_with("taskq: gone")),"quota is not a code defect");
        assert!(crate::runtime::accounts::Host::read(&c.install).active("claude",reset).is_none(),"expired observed reset uses a bounded unknown-reset probe");
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: §3 Runtime accounts (#2043 A4, A6): usage limit before work on the active account -> `usage-limit` with
    // `next account` -> respawn on the next enabled account, retries kept -> every account limited -> the § 4 hold until
    // the earliest end -> reset returns the first account -> all disabled admits nothing. Risk: a spare account waits
    // behind a hold, a failover spends a retry, a switch leaves no log line, or a disabled list still starts work.
    #[test]
    fn usage_limit_fails_over_to_the_next_account_then_holds() {
        let root = std::env::temp_dir().join(format!("taskq-account-failover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let install = root.join("install");
        std::fs::create_dir_all(&install).unwrap();
        let list = |a: bool, w: bool| std::fs::write(install.join(crate::runtime::accounts::FILE),
            format!("{{\"claude\":[{{\"label\":\"default\",\"enabled\":{a}}},{{\"label\":\"work\",\"enabled\":{w}}}]}}")).unwrap();
        list(true, true);
        let b = Fake::default();
        let mut c = Config { root: root.clone(), install: install.clone(), ..cfg("mac") };
        c.limits = BTreeMap::from([("claude".to_string(), 1)]);
        let reset = 1000;
        let e: Exits = Default::default();
        let (seen, r, inst) = (e.clone(), root.clone(), install.clone());
        let clock=&b;
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            // the runtime adapter's start line (spawn_inner) before the native output: the account a new start takes
            let account = crate::runtime::accounts::Host::read(&inst).pick("claude", clock.now().unwrap()).unwrap();
            let limit = format!("{{\"type\":\"assistant\",\"apiError\":\"usage_limit_reached\",\"quotaLimits\":{{\"status\":\"rejected\",\"resetsAt\":{reset}}}}}\n");
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, format!("{{\"type\":\"taskq.account\",\"account\":\"{account}\"}}\n{limit}")).unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        let n = b.create("A", "", &["taskq".into(), "run-claude".into()]).unwrap();
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), 1);
        *e.borrow()[0].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        let gone = b.comments(n).unwrap().into_iter().rfind(|c| c.body.starts_with("taskq: usage-limit")).unwrap().body;
        assert!(gone.contains("\nUsage limit: claude until 1970-01-01T00:16:40Z; next account work"), "{gone}");
        assert!(s.limited.is_empty(), "a spare account holds nothing");
        if e.borrow().len() < 2 { s.pass().unwrap(); }
        assert_eq!((e.borrow().len(), task(&b, n).state, task(&b, n).retries), (2, State::Doing, 0), "respawn on the next account, retries kept");
        let log = |i: u64| crate::runtime::attempt_log(&root, "worker", n, i);
        let attempts: Vec<u64> = core::executions(n, &entries(&b, n).unwrap()).iter().map(|x| x.attempt).collect();
        assert_eq!(crate::runtime::accounts::of_log(&log(*attempts.last().unwrap())), "work");
        assert!(s.log.iter().any(|l| l == "account claude default -> work (limit)"), "{:?}", s.log);
        assert!(s.heartbeat_body().contains("\naccount claude default on limited "), "{}", s.heartbeat_body());
        assert!(s.heartbeat_body().contains("\naccount claude work on active\n"));

        *e.borrow()[1].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        let gone = b.comments(n).unwrap().into_iter().rfind(|c| c.body.starts_with("taskq: usage-limit")).unwrap().body;
        assert!(gone.ends_with("Usage limit: claude until 1970-01-01T00:16:40Z") || gone.contains("until 1970-01-01T00:16:40Z\n"), "none left: {gone}");
        assert!(!gone.contains("next account"), "{gone}");
        assert_eq!(s.limited.get("claude"), Some(&(reset, true)), "every account limited: the usage-limit hold");
        s.pass().unwrap();
        assert_eq!((task(&b, n).state, e.borrow().len(), task(&b, n).retries), (State::Ready, 2, 0), "the hold keeps the task ready");
        assert!(s.log.iter().any(|l| l == "account claude work -> none (limit)"), "{:?}", s.log);

        b.state.borrow_mut().clock = reset;
        s.pass().unwrap();
        assert!(s.log.iter().any(|l| l == "account claude none -> default (reset)"), "{:?}", s.log);
        assert_eq!(e.borrow().len(), 3, "the reset admits the work again");
        assert!(s.limited.is_empty());

        list(false, false);
        let cap = s.capacity_json();
        assert!(cap["reasons"].as_array().unwrap().iter().any(|r| r["code"] == "capability" && r["text"] == "claude: every account disabled on this host"), "{cap}");
        assert!(s.admit_runtime("claude", false).is_err_and(|e| e.0 == "capability"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: #1446 A1/A2 Codex exit with a rejected refresh token -> `usage-limit` release -> login off at once.
    // Risk: a dead login reads logged in, each admitted task burns its gone budget, or claude is held too.
    #[test]
    fn rejected_codex_login_releases_without_retry_and_holds_codex() {
        let root = std::env::temp_dir().join(format!("taskq-login-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let b = Fake::default();
        let mut c = Config { root: root.clone(), install: root.join("install"), ..cfg("mac") };
        c.limits = BTreeMap::from([("claude".to_string(), 1), ("codex".to_string(), 1)]);
        let e: Exits = Default::default();
        let (seen, r) = (e.clone(), root.clone());
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n2026-10-21T08:00:00.000000Z ERROR codex_login::auth::manager: Failed to refresh token: Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.\n").unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        let n = b.create("A", "", &["taskq".into(), "run-codex".into()]).unwrap();
        s.pass().unwrap();
        assert!(task(&b, n).session.unwrap().starts_with("codex:"));
        *e.borrow()[0].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        let gone = b.comments(n).unwrap().into_iter().rfind(|c| c.body.starts_with("taskq: usage-limit")).unwrap().body;
        assert!(gone.contains("\nLogin off: codex refresh token rejected; run `codex login --device-auth`"), "{gone}");
        assert_eq!((task(&b, n).state, task(&b, n).retries, e.borrow().len()), (State::Ready, 0, 1), "A2: released, no retry, not readmitted");
        assert!(s.logged_out.contains("codex") && !s.logged_out.contains("claude"), "{:?}", s.logged_out);
        assert!(s.heartbeat_body().contains("\nlogin codex off"), "A1: at once, not at the next probe");
        let rows = crate::runtime::agents(&c.install);
        if let Some(r) = rows.as_array().unwrap().iter().find(|r| r["runtime"] == "codex" && r["path"].is_string() && r["reason"].is_string()) {
            assert_eq!(r["login"], "not logged in", "A2: taskq agents reads the mark");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: #1843 Codex worker exit 1 on "model is at capacity" mid-work -> continuing `steer` -> same session resumes (§ 4 step 1).
    // Risk: a transient model failure loses the session as `gone`, or continuations loop past their bound without a retry.
    #[test]
    fn transient_model_failure_continues_the_same_session_twice() {
        let root = std::env::temp_dir().join(format!("taskq-transient-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let b = Fake::default();
        let mut c = Config { root: root.clone(), install: root.join("install"), ..cfg("mac") };
        c.limits = BTreeMap::from([("codex".to_string(), 1)]);
        let e: Exits = Default::default();
        let resumes: Rc<RefCell<Vec<(String, Option<String>)>>> = Default::default();
        let (seen, r, rs) = (e.clone(), root.clone(), resumes.clone());
        let mut s = Service::new(&b, &c, "4.0.0", Box::new(move |l: &Launch| {
            let flag = Rc::new(RefCell::new(None));
            seen.borrow_mut().push((l.task, flag.clone()));
            rs.borrow_mut().push((l.session.clone(), l.resume.clone()));
            let log = crate::runtime::log_path(&r, l);
            std::fs::create_dir_all(log.parent().unwrap()).unwrap();
            std::fs::write(&log, "{\"type\":\"thread.started\",\"thread_id\":\"t1\"}
{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\"}}
{\"type\":\"turn.failed\",\"error\":{\"message\":\"Selected model is at capacity. Please try a different model.\"}}
").unwrap();
            Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
        }));
        s.focus = vec!["all".into()];
        let n = b.create("A", "", &["taskq".into(), "run-codex".into()]).unwrap();
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        for k in 0..2 {
            *e.borrow()[k].1.borrow_mut() = Some(1);
            s.pass().unwrap();
            let steers: Vec<_> = b.comments(n).unwrap().into_iter().filter(|c| c.body.starts_with(&format!("taskq: steer attempt {a}
"))).collect();
            assert_eq!(steers.len(), k + 1, "one continuing steer per exit");
            assert!(steers[k].body.contains("Continue the task") && steers[k].body.contains("model is at capacity"), "{}", steers[k].body);
            assert_eq!((task(&b, n).state, task(&b, n).attempt, task(&b, n).retries), (State::Doing, Some(a), 0), "the attempt stays live, no retry spent");
            s.pass().unwrap();
            let (session, resume) = resumes.borrow()[k + 1].clone();
            assert_eq!(session, "t1", "the same Codex session resumes");
            assert!(resume.is_some_and(|r| r.contains("Continue the task")), "the steer reaches the session");
        }
        *e.borrow()[2].1.borrow_mut() = Some(1);
        s.pass().unwrap();
        assert!(b.comments(n).unwrap().iter().any(|c| c.body.starts_with(&format!("taskq: gone attempt {a}
"))), "a third failure ends the attempt");
        assert_eq!((task(&b, n).state, task(&b, n).retries), (State::Ready, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: #1429 A2 runtime login. Risk: a logged-out runtime takes tasks that fail with 401, or its hold is silent.
    #[test]
    fn logged_out_runtime_admits_nothing_and_says_how_to_log_in() {
        let b = Fake::default();
        let mut c = cfg("mac");
        c.limits = BTreeMap::from([("claude".to_string(), 1), ("codex".to_string(), 1)]);
        let mut s = service(&b, &c, Default::default());
        s.logged_out = BTreeSet::from(["claude".to_string()]);
        let only = b.create("A", "", &["taskq".into(), "run-claude".into()]).unwrap();
        let other = b.create("B", "", &["taskq".into(), "run-codex".into()]).unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, only).state, State::Ready, "a claude-only task waits for the login");
        assert!(task(&b, other).session.unwrap().starts_with("codex:"), "a logged-in runtime keeps working");
        let cap = s.capacity_json();
        assert!(cap["reasons"].as_array().unwrap().iter().any(|r| r["code"] == "login" && r["text"].as_str().unwrap().contains("claude: not logged in; run `claude auth login`")), "{cap}");
        assert!(s.heartbeat_body().contains("\nlogin claude off"));
        s.logged_out.clear();
        s.pass().unwrap();
        assert_eq!(task(&b, only).state, State::Doing, "a probe that reads it logged in admits the task");
        assert!(!s.heartbeat_body().contains("login claude"));
    }

    // Chain: #2094 A6 runtime probe rows -> heartbeat `cli` lines. Risk: a host's CLI versions stay invisible on the board.
    #[test]
    fn heartbeat_names_cli_versions() {
        let b = Fake::default();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.clis = crate::runtime::clis_versions(&json!([
            {"runtime": "claude", "path": "/bin/claude", "version": "2.1.300"},
            {"runtime": "codex", "path": "/bin/codex", "version": null},
            {"runtime": "codex-app", "path": "/bin/codex", "version": null},
        ]));
        let body = s.heartbeat_body();
        assert!(body.contains("\ncli claude 2.1.300\ncli codex unknown"), "{body}");
        assert!(crate::runtime::clis_versions(&json!([{"runtime": "claude", "path": null, "version": null}])).is_empty(), "not installed: no line");
    }

    // A2/§4: a crash after release must not turn a quota refusal into a retry, locally or remotely.
    #[test]
    fn usage_limit_release_before_recovery() {
        for host in ["mac", "other"] {
            for role in ["worker", "reviewer"] {
                let b = Fake::default();
                // #2137: quota failures are host-wide; each simulated host needs its own writable store.
                let mut c = cfg("mac");
                c.install=std::env::temp_dir().join(format!("taskq-recovered-quota-{}",crate::runtime::uuid()));
                let n = b.create("T", "", &[TASK_LABEL.into()]).unwrap();
                let w = b.comment(n, &format!("taskq: spawn worker claude:s {host}")).unwrap();
                let a = if role == "reviewer" {
                    release_execution(&b,n,w,w,"exit").unwrap();
                    b.comment(n,&format!("taskq: result {} attempt {w}","a".repeat(40))).unwrap();
                    b.comment(n,&format!("taskq: spawn reviewer claude:r {host}")).unwrap()
                } else { w };
                b.comment(n,&format!("taskq: released {a} exit attempt {a}\nusage-limit until 1000")).unwrap();
                let mut s = service(&b,&c,Default::default());
                s.focus = vec!["off".into()];
                s.pass().unwrap();
                let t = task(&b,n);
                assert_eq!(t.retries,0,"{host}/{role}: recovery spent retries");
                assert_eq!(t.attempt,None);
                assert_eq!(t.state,if role == "reviewer" {State::Review} else {State::Ready});
                assert_eq!(s.limited.contains_key("claude"),host == "mac");
                assert!(b.comments(n).unwrap().iter().any(|c|c.body.starts_with("taskq: usage-limit attempt")));
                s.pass().unwrap();
                assert_eq!(b.comments(n).unwrap().iter().filter(|c|c.body.starts_with("taskq: usage-limit attempt")).count(),1);
                let _=std::fs::remove_dir_all(&c.install);
            }
        }
    }

    // R11/E2: offline Full stop uses owned handles; Drain never cancels and exit must be observed.
    #[test]
    fn full_stop_cancels_owned_work_before_board_access() {
        let b=Fake::default();let dir=std::env::temp_dir().join(format!("taskq-full-stop-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c=cfg("mac");c.install=dir.clone();c.root=dir.clone();
        let n=b.create("owned","",&["taskq".into()]).unwrap();
        let e:Exits=Default::default();let mut s=service(&b,&c,e.clone());s.pass().unwrap();
        let attempt=task(&b,n).attempt.unwrap();
        let mut hold=json!({"taskq_stop":{"id":"stop1","instance":"owner1","roots":[dir],"issued_at":1,"expires_at":301,"action":"stop"}});
        std::fs::write(dir.join("hold"),hold.to_string()).unwrap();
        s.reduced().unwrap();assert_eq!(*e.borrow()[0].1.borrow(),None,"Drain preserves current work");
        hold["taskq_stop"]["mode"]=json!("full");std::fs::write(dir.join("hold"),hold.to_string()).unwrap();
        b.state.borrow_mut().fail_comments=100;
        let _=s.reduced();
        assert_eq!(*e.borrow()[0].1.borrow(),Some(-9),"Full stop must cancel before a failing board write");
        assert_eq!(s.kids.len(),1,"offline release retains native ownership");
        assert_eq!(task(&b,n).attempt,Some(attempt),"no fabricated board retirement");
        assert!(dir.join("hold").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: § 3 emergency hold, § 4 reconciliation and recovery. Risk: a held host launches, an unknown read is
    // marked gone, or a recovery op is applied twice or over a newer revision.
    #[test]
    fn emergency_hold_refuses_launches_and_recovery_fences_attempts() {
        use crate::dispatch::{proposal, recover};
        let b = Fake::default();
        let dir = std::env::temp_dir().join(format!("taskq-hold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = cfg("mac");
        (c.install, c.root, c.limits) = (dir.clone(), dir.clone(), BTreeMap::from([("claude".to_string(), 3)]));
        let (n1, n2) = (b.create("A", "", &["taskq".into()]).unwrap(), b.create("B", "", &["taskq".into()]).unwrap());
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let (a1, a2) = (task(&b, n1).attempt.unwrap(), task(&b, n2).attempt.unwrap());
        // hold: a new task is not claimed, an answer is not resumed; children keep running
        std::fs::write(dir.join("hold"), "drill\n").unwrap();
        let n3 = b.create("C", "", &["taskq".into()]).unwrap();
        *e.borrow()[1].1.borrow_mut() = Some(0);
        b.comment(n2, &core::entry("ask", &[], Some(a2), "Which?\n1. x\n2. y ★")).unwrap();
        s.pass().unwrap();
        b.comment(n2, &core::entry("answer", &["1"], None, "x")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(-999); // n1's liveness read fails
        s.pass().unwrap();
        assert_eq!((task(&b, n3).state, task(&b, n2).state, e.borrow().len()), (State::Ready, State::Doing, 2), "held: nothing launched");
        let cap = s.capacity_json();
        assert_eq!((cap["gate"].as_str(), cap["reasons"][0]["code"].as_str(), cap["reasons"][0]["text"].as_str()), (Some("held"), Some("hold"), Some("drill")));
        // reconciliation: an unknown read keeps the child and the attempt; no gone, no op
        assert_eq!((task(&b, n1).state, task(&b, n1).attempt, s.kids.len()), (State::Doing, Some(a1), 1));
        let kids = |s: &Service| Ok(s.kids.iter().map(|k| json!({"attempt": k.attempt, "pid": 1, "live": if k.unknown.is_some() { "unknown" } else { "running" }, "reason": k.unknown.as_ref()})).collect());
        let rows = proposal(&b, "mac", &kids(&s)).unwrap();
        assert_eq!(rows.iter().map(|r| (r.task, r.live.as_str(), r.revision)).collect::<Vec<_>>(), [(n1, "unknown (read failed)", None), (n2, "no child", Some(core::revision(n2, &entries(&b, n2).unwrap())))]);
        assert!(proposal(&b, "mac", &Err("status.json 90 s old".into())).unwrap().iter().all(|r| r.revision.is_none()), "stale status: all unknown");
        // stale recovery: the worker writes after the proposal; the op is refused and the board is unchanged
        let before = b.comments(n2).unwrap().len();
        b.comment(n2, &core::entry("ask", &[], Some(a2), "Again?\n1. x\n2. y ★")).unwrap();
        let r = recover(&b, &dir, &rows);
        assert_eq!((r.len(), r[0].outcome, b.comments(n2).unwrap().len()), (1, "stale", before + 1));
        // a fresh proposal once n1 reads running: one receipt per op, a repeated apply writes nothing more
        *e.borrow()[0].1.borrow_mut() = None;
        s.pass().unwrap();
        let rows = proposal(&b, "mac", &kids(&s)).unwrap();
        let r = recover(&b, &dir, &rows);
        assert_eq!(r.iter().map(|r| (r.task, r.outcome, r.state)).collect::<Vec<_>>(), [(n1, "accepted", Some("ready"))], "n2 asks: not interrupted");
        let count = b.comments(n1).unwrap().len();
        assert!(recover(&b, &dir, &rows).iter().all(|r| r.ok()) && b.comments(n1).unwrap().len() == count, "retried op ids");
        // the fenced child is stopped (R11); still held, nothing relaunches
        s.pass().unwrap();
        assert_eq!((*e.borrow()[0].1.borrow(), s.kids.len(), e.borrow().len()), (Some(-9), 0, 2));
        // release: admission resumes
        std::fs::remove_file(dir.join("hold")).unwrap();
        s.pass().unwrap();
        assert!(task(&b, n1).attempt.is_some_and(|a| a != a1) && task(&b, n3).state == State::Doing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A5: reopening host admission advances both pending start and review without a focus rewrite.
    #[test]
    fn admission_reopen_advances_pending_start_and_review() {
        for review in [false,true] {
            let root=std::env::temp_dir().join(format!("taskq-admission-progress-{}-{review}",std::process::id()));
            std::fs::create_dir_all(crate::dot(&root)).unwrap();
            std::fs::write(crate::dot(&root).join("focus"),"all").unwrap();
            let b=Fake::default();let n=b.create("Pending","",&[TASK_LABEL.into()]).unwrap();
            if review {
                let a=b.comment(n,"taskq: spawn worker codex:fixture invented").unwrap();
                b.comment(n,&core::entry("result",&["0123456789abcdef0123456789abcdef01234567"],Some(a),"candidate")).unwrap();
                release_execution(&b,n,a,a,"exit").unwrap();
            }
            let p=crate::host::Policy::parse(json!({"limits":{"claude":2},"load_gate":{"enabled":false}}),"invented").unwrap();
            let host=std::sync::Arc::new(std::sync::Mutex::new(crate::host::Host::new(Err("handoff: legacy service still running".into()))));
            let mut c=cfg("invented");c.root=root.clone();c.install=root.clone();
            let exits:Exits=Default::default();let mut s=service(&b,&c,exits.clone());
            s.host=Some(host.clone());s.reload(Ok(p.cap.clone()));s.pass().unwrap();
            assert!(exits.borrow().is_empty(),"held admission cannot claim pending work");
            let paths=[crate::dot(&root).join("poke"),root.join("host-poke")];let baseline=crate::poke_times(&paths);
            host.lock().unwrap().reload_admission(Ok(p),None,&[root.clone()]);
            crate::sleep_until_poke(&paths,baseline,std::time::Instant::now()+std::time::Duration::from_secs(60),&mut ||panic!("project was not awakened"));
            s.pass().unwrap();
            assert_eq!(exits.borrow().len(),1,"reopened admission must advance pending start/review");
            assert_eq!(task(&b,n).role.as_deref(),Some(if review {"reviewer"}else{"worker"}));
            assert_eq!(std::fs::read_to_string(crate::dot(&root).join("focus")).unwrap(),"all");
            drop(s);std::fs::remove_dir_all(root).unwrap();
        }
    }

    // Chain: § 2 early answer. Risk: an early answer marks the attempt gone.
    #[test]
    fn answer_before_the_asking_child_exits_resumes_without_gone() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let c = cfg("mac");
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★")).unwrap();
        b.comment(n, &core::entry("answer", &["2"], None, "y")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let t = task(&b, n);
        assert_eq!((t.state, t.attempt, t.retries, e.borrow().len()), (State::Doing, Some(a), 0, 2), "resumed, no gone");
        // the resumed child exits without a new hand-in: that exit is gone
        *e.borrow()[1].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        assert_eq!(task(&b, n).retries, 1);
    }

    // Chain: § 2 answer resumes the asking attempt, every round (#1718). Risk: a second answer in one attempt stays
    // undelivered while the task reads `doing` with no live worker: the pass that first saw the answer's `updated`
    // read a comment list that lagged it, and every later pass reused that list for the unchanged `updated`.
    #[test]
    fn second_answer_in_one_attempt_resumes_within_one_pass() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let c = cfg("mac");
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        for round in 1..=2 {
            b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★")).unwrap();
            *e.borrow()[round - 1].1.borrow_mut() = Some(0);
            s.pass().unwrap();
            assert_eq!((task(&b, n).state, s.kids.len()), (State::Ask, 0), "round {round}: asked, child released");
            let answer = b.comment(n, &core::entry("answer", &["2"], None, "y")).unwrap();
            if round == 2 {
                b.state.borrow_mut().hide_latest_comment = true;
                s.pass().unwrap();
                b.state.borrow_mut().hide_latest_comment = false;
                assert_eq!((s.kids.len(), e.borrow().len()), (0, 2), "a comment list lagging the answer resumes nothing yet");
            }
            s.pass().unwrap();
            let t = task(&b, n);
            assert_eq!((t.state, t.attempt, e.borrow().len(), s.kids.len()), (State::Doing, Some(a), round + 1, 1), "round {round}: resumed in one pass");
            assert_eq!(s.delivered(a), answer, "round {round}: the answer is accepted input");
        }
    }

    // Chain: § 3 admission filters. Risk: a waiting or filtered task starts.
    #[test]
    fn waiting_and_filters_hold_admission() {
        let b = Fake::default();
        let dep = b.create("dep", "", &["later".into()]).unwrap();
        b.comment(dep, &core::entry("later", &[], None, "")).unwrap();
        let n = b.create("T", &format!("Depends on #{dep}"), &["taskq".into()]).unwrap();
        let other = b.create("O", "", &["taskq".into(), "host-win".into()]).unwrap();
        let user = b.create("U", "", &[]).unwrap();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        assert_eq!(task(&b, other).state, State::Ready);
        assert!(b.issue(n).unwrap().labels.contains(&"q-blocked".to_string()));
        b.close(dep).unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Doing);
        assert!(b.comments(user).unwrap().is_empty() && b.issue(user).unwrap().labels.is_empty(), "no taskq label: never a task");
    }

    // Chain: § 2 worker park (#1486). Risk: a parked task starts while its blocker (a held draft too) is open, never
    // resumes after the close, or hides the blocker link in status.
    #[test]
    fn worker_park_waits_on_its_blocker_until_it_closes() {
        let b = Fake::default();
        let blocker = b.create("fix shots", "", &["q-held".into()]).unwrap();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        c.root = std::env::temp_dir().join(format!("taskq-park-{}", crate::runtime::uuid()));
        c.install = c.root.clone();
        std::fs::create_dir_all(&c.root).unwrap();
        let raw = json!({});
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        b.comment(n, &core::entry("later", &[&format!("#{blocker}")], Some(a), "Reason: r")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        assert_eq!((task(&b, n).attempt, e.borrow().len()), (None, 1), "parked, no new spawn");
        assert!(b.issue(n).unwrap().labels.contains(&"q-blocked".to_string()));
        let report = crate::snapshot::read(&b, &c, &raw, &[]).text(&c, &raw).unwrap();
        assert!(report.contains(&format!("blocked ([#{blocker}](https://board/{blocker}) open)")), "{report}");
        b.close(blocker).unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Doing);
        std::fs::remove_dir_all(&c.root).ok();
    }

    // #2138 A2-A4/R13: mode and collaborator policy must reach real claims; preserve strict pins by default.
    #[test]
    fn balancing_modes_and_project_execution() {
        for (mode, own, unlabelled) in [("primary",true,true),("any",true,true),("pinned-only",true,false),("labelled",true,false),("manager-only",false,false),("none",false,false)] {
            let b=Fake::default();
            let n=b.create("unlabelled","", &["taskq".into()]).unwrap();
            let pin=b.create("pinned","", &["taskq".into(),"host-mac".into()]).unwrap();
            let foreign=b.create("foreign","", &["taskq".into(),"host-win".into()]).unwrap();
            let review=b.create("review","", &["taskq".into(),"host-mac".into()]).unwrap();
            let a=b.comment(review,"taskq: spawn worker claude:previous win").unwrap();
            b.comment(review,&core::entry("result",&["0123456789abcdef0123456789abcdef01234567"],Some(a),"Changed: candidate\nOpen: none")).unwrap();
            accept_execution(&b,review,a,a,None).unwrap();release_execution(&b,review,a,a,"exit").unwrap();
            let mut c=cfg("mac");
            crate::host::Policy::parse(json!({"hosts":{crate::hostname():"mac"},"claim":mode}),&crate::hostname()).unwrap().apply(&mut c);
            c.limits.insert("claude".into(),4);
            service(&b,&c,Default::default()).pass().unwrap();
            assert_eq!(task(&b,n).state==State::Doing,unlabelled,"{mode}");
            assert_eq!(task(&b,pin).state==State::Doing,own,"{mode}");
            assert_eq!(task(&b,foreign).state,State::Ready);
            assert_eq!(task(&b,review).role.as_deref()==Some("reviewer"),own,"review {mode}");
        }
        for reroute in [false,true] {
            let b=Fake::default();
            let n=b.create("collaborator","", &["taskq".into(),"run-claude".into()]).unwrap();
            let mut c=cfg("fleet");
            c.install=std::env::temp_dir().join(format!("taskq-collaborator-{}",crate::runtime::uuid()));
            std::fs::create_dir_all(&c.install).unwrap();
            let p=crate::host::Policy::parse(json!({"load_gate":{"enabled":false},"limits":{"claude":0,"codex":2},"projects":{"/nonexistent":{"default_runtime":"codex","allowed_runtimes":["codex"],"auto_reroute":reroute}}}),"fleet").unwrap();
            p.apply(&mut c);
            c.limits=p.project_limits(&c.root,&json!({})).unwrap();
            let mut s=service(&b,&c,Default::default());
            let mut host=crate::host::Host::new(Ok(p.clone()));host.reload(Ok(p.clone()));
            s.host=Some(std::sync::Arc::new(std::sync::Mutex::new(host)));
            s.reload(Ok(p.project(&c.root,&json!({})).unwrap()));s.pass().unwrap();
            let t=task(&b,n);
            assert_eq!(t.state,if reroute {State::Doing}else{State::Ready},"{:?}",s.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().admission);
            if reroute {assert_eq!(rt_of(&t).unwrap().0,"codex");}
            assert!(b.issue(n).unwrap().labels.contains(&"run-claude".into()),"no board relabel");
            std::fs::remove_dir_all(&c.install).unwrap();
        }
    }

    // #2138 A2/A4/R12: real heartbeat publication -> spillover claim, with quota, unknown and fencing intact.
    #[test]
    fn balancing_spillover_uses_effective_primary_availability() {
        for scenario in ["free","full","quota","spare","unknown","unknown-focus","unknown-load","legacy","corrupt","stale","none","grace","pinned","capability","fenced","one-free","all-full"] {
            let b=Fake::default();
            let root=std::env::temp_dir().join(format!("taskq-spillover-{}",crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            let mut primary=cfg("win");primary.install=root.clone();
            primary.limits=BTreeMap::from([("claude".into(),1),("codex".into(),if scenario=="spare" {1}else{0}),("antigravity".into(),0)]);
            let mut p=service(&b,&primary,Default::default());
            if matches!(scenario,"quota"|"spare") {
                crate::runtime::accounts::record_windows(&root,"claude","default",0,&[("five_hour".into(),Some(18000)),("seven_day".into(),Some(604800))]).unwrap();
            }
            if scenario=="unknown" {p.cap=Err("unreadable policy".into());}
            if scenario=="unknown-focus" {p.focus.clear();p.focus_error=Some("invalid focus".into());}
            if scenario=="unknown-load" {p.gate.configure(GateCfg {enabled:true,..GATE_DEFAULT});}
            if matches!(scenario,"full"|"one-free"|"all-full") {p.reserved.push((99,99,"claude".into()));}
            if scenario!="none" {
                let body=if scenario=="legacy" {"version old".into()} else if scenario=="corrupt" {"balancing {\"schema\":99}".into()} else {p.heartbeat_body()};
                let hb=b.create("taskq host win",&body,&[HOST_LABEL.into()]).unwrap();
                if scenario=="stale" {b.state.borrow_mut().issues.iter_mut().find(|i|i.number==hb).unwrap().updated=-1000;}
            }
            if matches!(scenario,"one-free"|"all-full") {
                let mut second=p.balancing();
                if scenario=="one-free" {let r=second.runtimes.get_mut("claude").unwrap();r.code="admitted".into();r.reason.clear();}
                b.create("taskq host linux",&format!("balancing {}",serde_json::to_string(&second).unwrap()),&[HOST_LABEL.into()]).unwrap();
            }
            let labels=if scenario=="pinned" {vec!["taskq".into(),"host-mac".into()]} else if scenario=="capability" {vec!["taskq".into(),"needs-special".into()]} else {vec!["taskq".into()]};
            let n=b.create("work","",&labels).unwrap();
            if scenario=="fenced" {
                let a=b.comment(n,"taskq: spawn worker claude:s win").unwrap();
                b.comment(n,&core::entry("requeue",&[],Some(a),"Reason: keep old execution fenced")).unwrap();
            }
            let mut c=cfg("mac");c.claim=crate::host::ClaimMode::Spillover;c.spillover_grace=if scenario=="grace" {0}else{300};
            c.has.push("special".into());
            let mut s=service(&b,&c,Default::default());s.pass().unwrap();
            let expected=matches!(scenario,"full"|"quota"|"grace"|"pinned"|"capability"|"all-full");
            assert_eq!(task(&b,n).state==State::Doing,expected,"{scenario}: {:?}",s.log);
            if scenario=="quota" {
                let spawn=entries(&b,n).unwrap().into_iter().find(|e|e.body.starts_with("taskq: spawn")).unwrap();
                assert!(spawn.body.contains("seven_day"),"quota reset/probe evidence retained: {}",spawn.body);
                assert_eq!(p.cfg.limits,primary.limits,"quota never changes saved capacity");
            }
            if !expected && scenario!="fenced" {
                let reason=&s.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().admission.as_ref().unwrap().reason;
                assert!(reason.contains("spillover waits"),"{scenario}: {reason}");
            }
            // A second pass must never duplicate a live claim, even after grace.
            s.cfg.spillover_grace=0;s.pass().unwrap();
            assert!(entries(&b,n).unwrap().iter().filter(|e|e.body.starts_with("taskq: spawn")).count()<=1,"{scenario}");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    // #2138 A2/R1: old issue age cannot skip grace after a new ready transition; unrelated notes never reset it.
    #[test]
    fn balancing_grace_uses_last_queue_transition() {
        let b=Fake::default();let mut c=cfg("mac");c.claim=crate::host::ClaimMode::Spillover;
        let n=b.create("work","",&[TASK_LABEL.into()]).unwrap();
        b.comment(n,"taskq: hold").unwrap();b.advance(1000);
        let start=b.comment(n,"taskq: start").unwrap();
        b.advance(100);b.raw(n,"unrelated note",true,false);
        let mut s=service(&b,&c,Default::default());
        let views=views(&b,&mut s.cache,&mut s.deps,300).unwrap().0;
        let v=views.iter().find(|v|v.issue.number==n).unwrap();
        let at=v.comments.iter().find(|c|c.id==start).unwrap().created;
        assert!(s.host_claim(v,&[],at+299).is_err());
        assert!(s.host_claim(v,&[],at+300).unwrap().unwrap().contains("grace"));
    }

    // #2138 A3/R9: the exception applies to operator disables only, for workers/reviewers, without enlarging access.
    #[test]
    fn balancing_project_reroute_keeps_other_pins_and_restrictions() {
        for reviewer in [false,true] {
            for scenario in ["zero","excluded","accounts-off","quota","full","login","access","unknown","app","none-enabled","fallback-access"] {
                let b=Fake::default();let root=std::env::temp_dir().join(format!("taskq-project-reroute-{}",crate::runtime::uuid()));
                std::fs::create_dir_all(&root).unwrap();
                let mut c=cfg("fleet");c.install=root.clone();c.auto_reroute=true;c.default_runtime="codex".into();
                c.limits=BTreeMap::from([("claude".into(),if matches!(scenario,"zero"|"fallback-access") {0}else{1}),("codex".into(),2)]);
                if scenario=="excluded" {c.allowed_runtimes=Some(vec!["codex".into()]);}
                if scenario=="none-enabled" {c.allowed_runtimes=Some(vec![]);}
                if scenario=="accounts-off" {std::fs::write(root.join("accounts.json"),r#"{"claude":[{"label":"default","enabled":false}]}"#).unwrap();}
                if scenario=="quota" {crate::runtime::accounts::record_windows(&root,"claude","default",0,&[("seven_day".into(),Some(604800))]).unwrap();}
                let pin=if scenario=="app" {"run-codex-app"}else{"run-claude"};
                let n=b.create("collaborator","",&["taskq".into(),pin.into()]).unwrap();
                let mut s=service(&b,&c,Default::default());
                if scenario=="full" {s.kids.push(Kid {unknown:None,stopping:false,session_url:None,execution:99,task:99,attempt:99,rt:"claude".into(),proc:Box::new(FakeProc(Default::default()))});}
                if scenario=="login" {s.logged_out.insert("claude".into());}
                if scenario=="access" {s.refused_access.insert("claude".into(),"missing grant".into());}
                if scenario=="fallback-access" {s.refused_access.insert("codex".into(),"missing fallback grant".into());}
                if scenario=="unknown" {s.cap=Err("invalid policy".into());}
                let mut views=views(&b,&mut s.cache,&mut s.deps,300).unwrap().0;
                let v=views.iter_mut().find(|v|v.issue.number==n).unwrap();
                if reviewer {v.projection.task.state=State::Review;}
                let selected=s.select_runtime(v);
                if matches!(scenario,"zero"|"excluded"|"accounts-off") {
                    let (rt,reason)=selected.unwrap();assert_eq!(rt,"codex");assert!(reason.unwrap().contains("auto_reroute"));
                } else {assert!(selected.is_err(),"{scenario}: {selected:?}");}
                // Reuse exactly the published runtime policy on the other host.
                if !matches!(scenario,"unknown"|"app") {
                    let balance=Balancing::read(&s.heartbeat_body()).unwrap();
                    assert_eq!(balance.available(v).is_ok(),matches!(scenario,"zero"|"excluded"|"accounts-off"),"remote {scenario}");
                }
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    // Chain: § 4 Filter, host.json `claim` (#1493). Risk: a labelled host claims unrouted work, or an any host stops
    // taking it.
    #[test]
    fn labelled_host_claims_only_its_own_host_label() {
        let b = Fake::default();
        let open = b.create("U", "", &["taskq".into()]).unwrap();
        let own = b.create("F", "", &["taskq".into(), "host-fra".into()]).unwrap();
        let fra = Config { claim: crate::host::ClaimMode::Labelled, ..cfg("fra") };
        service(&b, &fra, Default::default()).pass().unwrap();
        assert_eq!((task(&b, open).state, task(&b, own).state), (State::Ready, State::Doing), "labelled host skips the unlabelled task");
        service(&b, &cfg("mac"), Default::default()).pass().unwrap();
        assert_eq!(task(&b, open).state, State::Doing, "an any host still takes unlabelled work");
    }

    // Chain: § 1 `Depends on <repo>#N` (#1126). Risk: a task blocked by another repository never starts, starts on a
    // close that did not complete, or reports no linked reason.
    #[test]
    fn cross_board_dependency_waits_with_link_until_completed() {
        let b = Fake::default();
        let dashq = |open: bool, reason: Option<&str>| ("dashq".to_string(), Issue { number: 36, open, reason: reason.map(String::from), ..Default::default() });
        b.state.borrow_mut().foreign.push(dashq(true, None));
        let dep = b.create("dep", "", &[]).unwrap();
        let n = b.create("T", "Depends on dashq#36\nDepends on owner/gone#5", &["taskq".into()]).unwrap();
        let same = b.create("S", &format!("Depends on #{dep}"), &["taskq".into()]).unwrap();
        // § 8 cutover: N-1 reads only `Depends on #N`; a held task has no `taskq` label, so N-1 (same `is_task`) skips it
        assert!(core::depends("Depends on dashq#36").iter().all(|d| d.repo.is_some()));
        let held = b.create("H", "Depends on dashq#36", &["q-held".into()]).unwrap();
        assert!(!is_task(&b.issue(held).unwrap()) && is_held(&b.issue(held).unwrap()));
        let mut c = cfg("mac");
        // A shared absolute path may contain real profile records on the test host.
        c.root = std::env::temp_dir().join(format!("taskq-cross-board-{}", crate::runtime::uuid()));
        c.install = c.root.clone();
        std::fs::create_dir_all(&c.root).unwrap();
        let raw = json!({});
        let report = || crate::snapshot::read(&b, &c, &raw, &[]).text(&c, &raw).unwrap();
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        assert!(b.issue(n).unwrap().labels.contains(&"q-blocked".to_string()));
        assert!(report().contains("blocked ([dashq#36](https://board/dashq/36) open, [owner/gone#5](https://board/owner/gone/5) unknown: HTTP 404 owner/gone#5)"), "{}", report());
        b.state.borrow_mut().foreign = vec![dashq(false, Some("not_planned")), ("owner/gone".into(), Issue { number: 5, ..Default::default() })];
        b.state.borrow_mut().issues[dep as usize - 1].reason = Some("not_planned".into());
        b.state.borrow_mut().issues[dep as usize - 1].open = false;
        s.pass().unwrap();
        let blocked = |t: u64| b.issue(t).unwrap().labels.contains(&"q-blocked".to_string());
        assert!(blocked(n) && blocked(same), "not completed stays unknown, not done");
        assert!(report().contains("[dashq#36](https://board/dashq/36) unknown: closed not planned)"), "{}", report());
        b.state.borrow_mut().foreign[0] = dashq(false, Some("completed"));
        b.state.borrow_mut().reads = 0;
        s.pass().unwrap();
        assert_eq!(task(&b, n).state, State::Doing, "eligible within one pass after the blocker completed");
        assert_eq!(b.state.borrow().reads, 1, "closed dependencies already read are cached: only dashq#36 read again");
        drop(s);
        std::fs::remove_dir_all(&c.root).unwrap();
    }

    // Chain: § 3 priority order. Risk: legacy priority labels order or filter wrong.
    #[test]
    fn canonical_and_legacy_priorities_filters_and_conflicts() {
        let b = Fake::default();
        let old = b.create("old", "", &["taskq".into(), "priority-1".into()]).unwrap();
        let new = b.create("new", "", &["taskq".into(), "p1".into()]).unwrap();
        let conflict = b.create("bad", "", &["taskq".into(), "p1".into(), "p2".into()]).unwrap();
        assert_eq!(priority(&b.issue(old).unwrap()), priority(&b.issue(new).unwrap()));
        let mut c = cfg("mac"); c.limits.insert("claude".into(), 3);
        let mut s = service(&b, &c, Default::default()); s.focus = vec!["p1".into()];
        s.pass().unwrap();
        assert_eq!(task(&b, old).state, State::Doing);
        assert_eq!(task(&b, new).state, State::Doing);
        assert_eq!(task(&b, conflict).state, State::Ready);
        assert!(b.comments(conflict).unwrap().is_empty(), "conflicting restriction never claimed");
    }

    // Chain: § 4 step 6 order with one slot. Risk: p0 workers starve pending reviews (#1365), or a review jumps a resume.
    #[test]
    fn one_slot_goes_to_resume_then_review_then_ready() {
        for resume in [false, true] {
            let b = Fake::default();
            let live = b.create("L", "", &["taskq".into(), "p2".into()]).unwrap();
            let review = b.create("R", "", &["taskq".into(), "p1".into()]).unwrap();
            let ready = b.create("P0", "", &["taskq".into(), "p0".into()]).unwrap();
            let w = b.comment(review, "taskq: spawn worker codex:w mac").unwrap();
            accept_execution(&b, review, w, w, None).unwrap(); release_execution(&b, review, w, w, "exit").unwrap();
            b.comment(review, &format!("taskq: result {} attempt {w}", "a".repeat(40))).unwrap();
            if resume {
                let a = b.comment(live, "taskq: spawn worker claude:session mac").unwrap();
                accept_execution(&b, live, a, a, None).unwrap(); release_execution(&b, live, a, a, "exit").unwrap();
                b.comment(live, "taskq: steer\ncontinue").unwrap();
            } else {
                b.comment(live, "taskq: later\nparked").unwrap();
            }
            let c = cfg("mac");
            let e: Exits = Default::default();
            let mut s = service(&b, &c, e.clone());
            s.pass().unwrap();
            let launched: Vec<u64> = e.borrow().iter().map(|x| x.0).collect();
            assert_eq!(launched, vec![if resume { live } else { review }], "resume={resume}");
            assert_eq!(task(&b, ready).state, State::Ready, "p0 waits for the slot");
            assert_eq!(task(&b, review).attempt.is_some(), !resume, "a resume keeps precedence over a review");
        }
    }

    // Chain: § 3 p0 admission. Risk: p0 blocked by focus or passes pause.
    #[test]
    fn p0_passes_any_focus_but_not_pause_or_limits() {
        let b = Fake::default();
        let p1 = b.create("P1", "", &["taskq".into(), "p1".into(), "area-maps".into()]).unwrap();
        let p0 = b.create("P0", "", &["taskq".into(), "p0".into(), "area-ui".into()]).unwrap();
        let other = b.create("O", "", &["taskq".into(), "area-ui".into()]).unwrap();
        assert!(priority(&b.issue(p0).unwrap()) < priority(&b.issue(p1).unwrap()));
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.focus = vec![];
        s.pass().unwrap();
        assert_eq!(task(&b, p0).state, State::Ready, "pause: p0 waits too");
        s.focus = vec!["area-maps".into()];
        s.pass().unwrap();
        assert_eq!((task(&b, p0).state, task(&b, p1).state, task(&b, other).state), (State::Doing, State::Ready, State::Ready), "p0 first; limit 1 holds");
    }

    // Chain: § 3 focus and needs. Risk: a task outside focus or needs starts.
    #[test]
    fn focus_and_needs_choose_tasks() {
        let b = Fake::default();
        let win = b.create("W", "", &["taskq".into(), "needs-windows".into()]).unwrap();
        let maps = b.create("M", "", &["taskq".into(), "area-maps".into()]).unwrap();
        let other = b.create("O", "", &["taskq".into()]).unwrap();
        let gated = b.create("G", "", &["taskq".into(), "needs-gate".into()]).unwrap();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.focus = vec![];
        s.pass().unwrap();
        assert_eq!(task(&b, other).state, State::Ready, "focus off: nothing starts");
        s.focus = vec!["area-maps".into()];
        s.pass().unwrap();
        assert_eq!((task(&b, maps).state, task(&b, other).state), (State::Doing, State::Ready));
        let mut c = cfg("mac");
        c.limits.insert("claude".into(), 9);
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        assert_eq!((task(&b, win).state, task(&b, other).state), (State::Ready, State::Doing), "mac has no windows");
        assert_eq!(task(&b, gated).state, State::Doing, "every host of this version has gates (§ 8)");
        let w = Config { has: vec!["windows".into()], ..c.clone() };
        let mut s = service(&b, &w, Default::default());
        s.pass().unwrap();
        assert_eq!(task(&b, win).state, State::Doing);
    }

    // Chain: § 4 needs-gui routing (#1124). Risk: a gui task starts on a sandboxed runtime or an unprobed host, or waits silently.
    #[test]
    fn needs_gui_goes_only_to_a_probed_full_access_runtime() {
        let b = Fake::default();
        let t = b.create("G", "", &["taskq".into(), "needs-gui".into()]).unwrap();
        let mut c = cfg("mac");
        c.limits.insert("claude".into(), 1);
        c.has = vec!["gui".into()]; // config never lists gui: only the probe does
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        let hb = |b: &Fake| b.issues(None).unwrap().into_iter().find(|i| i.title == "taskq host mac").unwrap().body;
        s.pass().unwrap();
        assert_eq!((task(&b, t).state, s.waits.contains_key(&t)), (State::Ready, true), "no probe: no gui host");
        s.gui_checked = Some(std::time::Instant::now());
        s.gui_probe = Ok(());
        s.pass().unwrap();
        assert_eq!((task(&b, t).state, s.waits.contains_key(&t)), (State::Ready, true), "every runtime sandboxed: no gui host");
        assert!(!hb(&b).lines().any(|l| l == "has gui"));
        s.full = BTreeSet::from(["claude".to_string()]);
        s.gui_checked = Some(std::time::Instant::now() - std::time::Duration::from_secs(300));
        s.pass().unwrap();
        assert_eq!(task(&b, t).state, State::Ready, "stale capture cannot admit GUI work");
        s.gui_checked = Some(std::time::Instant::now());
        b.labels(t, &["run-codex".into(), "prefer-claude".into()], &[]).unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, t).state, State::Ready, "hard runtime must not fall back to a Full runtime");
        assert!(s.waits[&t].contains("codex has no effective Full grant"));
        b.labels(t, &[], &["run-codex".into(), "prefer-claude".into()]).unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, t).state, State::Doing);
        assert!(task(&b, t).session.unwrap().starts_with("claude:"), "only the Full access runtime");
        assert!(hb(&b).lines().any(|l| l == "has gui") && s.waits.is_empty());
        // Full -> Workspace, then probe loss: the resume waits, inbox and lease kept; it runs once both hold again
        let a = task(&b, t).attempt.unwrap();
        b.comment(t, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        b.comment(t, &core::entry("answer", &["1"], None, "x")).unwrap();
        for (probe, full) in [(Ok(()), vec![]), (Err("screencapture wrote no PNG".to_string()), vec!["claude".to_string()])] {
            (s.gui_probe, s.full) = (probe, full.into_iter().collect());
            s.pass().unwrap();
            assert_eq!((e.borrow().len(), task(&b, t).attempt), (1, Some(a)), "no sandboxed resume");
            assert!(s.waits.contains_key(&t), "resume refusal must name a reason");
            assert!(hb(&b).lines().any(|l| l == format!("#{t} {a}")), "the waiting resume keeps its lease");
        }
        s.gui_probe = Ok(());
        s.gui_checked = Some(std::time::Instant::now() - std::time::Duration::from_secs(300));
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), 1, "stale evidence must not resume");
        assert!(s.waits[&t].contains("stale"));
        s.gui_checked = Some(std::time::Instant::now());
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), 2, "resumed on the Full runtime");
        let other = b.create("H", "", &["taskq".into(), "needs-gui".into()]).unwrap();
        let w = cfg("win");
        let mut s = service(&b, &w, Default::default());
        s.gui_probe = Err("no gui probe on this OS".into());
        s.pass().unwrap();
        assert!(s.waits[&other].contains("no gui probe on this OS"));
        assert_eq!((task(&b, other).state, s.waits.contains_key(&other)), (State::Ready, true), "a remote heartbeat cannot hide local missing evidence");
    }

    // § 4 GUI eligibility: won claims retain their lease/slot while evidence expires or access is revoked.
    #[test]
    fn needs_gui_held_launch_rechecks_evidence_and_grant() {
        // #1845 A3: a stale Accessibility grant holds the launch with a visible reason instead of a worker ask
        for case in ["missing", "unsupported", "stale", "revoked", "grant"] {
            let b = Fake::default();
            let n = b.create("GUI", "", &[TASK_LABEL.into(), "needs-gui".into()]).unwrap();
            let c = cfg("mac");
            let exits: Exits = Default::default();
            let mut s = service(&b, &c, exits.clone());
            s.gui_probe = Ok(());
            s.gui_checked = Some(std::time::Instant::now());
            s.full.insert("claude".into());
            s.hold = Some("fixture load hold".into());
            let v = views(&b, &mut HashMap::new(), &mut HashMap::new(), 300).unwrap().0.remove(0);
            s.claim(&v, "worker", "claude", "").unwrap();
            let a = task(&b, n).attempt.unwrap();
            assert_eq!(s.held.len(), 1);
            s.hold = None;
            match case {
                "missing" => s.gui_checked = None,
                "unsupported" => s.gui_probe = Err("no gui probe on this OS".into()),
                "stale" => s.gui_checked = Some(std::time::Instant::now() - std::time::Duration::from_secs(300)),
                "grant" => { s.grants.insert("accessibility".into(), "stale".into()); }
                _ => s.full.clear(),
            }
            s.pass().unwrap();
            assert!(exits.borrow().is_empty(), "{case}: no held launch");
            if case == "grant" { assert!(s.waits[&n].contains("Accessibility grant stale (TCC)"), "{}", s.waits[&n]); }
            s.grants.clear();
            assert_eq!(s.held.len(), 1);
            assert_eq!(task(&b, n).attempt, Some(a));
            assert!(s.waits.contains_key(&n), "{case}: visible reason");
            assert!(s.heartbeat_body().lines().any(|l| l == format!("#{n} {a}")));
            s.gui_probe = Ok(());
            s.gui_checked = Some(std::time::Instant::now());
            s.full.insert("claude".into());
            s.pass().unwrap();
            assert_eq!(exits.borrow().len(), 1, "{case}: resume the same held claim once");
            assert_eq!(task(&b, n).attempt, Some(a));
            assert!(s.held.is_empty());
        }
    }

    // Chain: § 4 empty focus (#974). Risk: a host with free slots admits nothing and says nowhere why.
    #[test]
    fn empty_focus_is_a_capacity_reason_and_shows_in_hosts() {
        let b = Fake::default();
        let t = b.create("T", "", &["taskq".into()]).unwrap();
        let c = cfg("mac");
        let mut s = service(&b, &c, Default::default());
        s.focus.clear();
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!(task(&b, t).state, State::Ready);
        let focus = |s: &Service| s.capacity_json()["reasons"].as_array().unwrap().iter().find(|r| r["code"] == "focus").map(|r| r["text"].as_str().unwrap().to_string());
        assert_eq!(focus(&s).as_deref(), Some(FOCUS_OFF));
        let text = s.snapshot.as_ref().unwrap().text(&c, &Value::Null).unwrap();
        assert!(text.contains("Hosts: mac v4.0.0 ") && text.ends_with(" focus off"), "{text}");
        s.focus_error = Some("focus: not UTF-8 text; file kept".into());
        assert_eq!(focus(&s).as_deref(), s.focus_error.as_deref(), "an unreadable focus names its own reason");
        s.focus_error = None;
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!((focus(&s), task(&b, t).state), (None, State::Doing));
        assert!(!s.snapshot.as_ref().unwrap().text(&c, &Value::Null).unwrap().contains("focus"));
    }

    // #2137 A7: a mid-work quota preserves ownership; restart waits, then resumes only the same native session.
    #[test]
    fn balancing_midwork_quota_resumes_same_session_after_restart() {
        for (reviewer,refuse) in [(false,false),(true,false),(false,true),(true,true)] {
        let b=Fake::default();let root=std::env::temp_dir().join(format!("taskq-midwork-quota-{}",crate::runtime::uuid()));
        let mut c=cfg("mac");c.root=root.clone();c.install=root.join("install");
        let n=b.create("Keep work","",&["taskq".into(),"run-claude".into()]).unwrap();
        if reviewer {let a=b.comment(n,"taskq: spawn worker claude:worker mac").unwrap();b.comment(n,&format!("taskq: result {} attempt {a}","a".repeat(40))).unwrap();b.comment(n,&format!("taskq: released {a} exit attempt {a}")).unwrap();}
        let calls=Rc::new(RefCell::new(vec![]));let exits:Exits=Default::default();
        let starter=|| {
            let (calls,exits,root)=(calls.clone(),exits.clone(),root.clone());
            Box::new(move |l:&Launch| {
                calls.borrow_mut().push((l.attempt,l.session.clone(),l.resume.is_some()));
                let log=crate::runtime::log_path(&root,l);std::fs::create_dir_all(log.parent().unwrap()).unwrap();
                if l.resume.is_none() {std::fs::write(&log,json!({"type":"system","subtype":"init","session_id":l.session}).to_string()+"\n"+concat!(
                    "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"Read\"}]}}\n",
                    "{\"type\":\"assistant\",\"apiError\":\"usage_limit_reached\",\"quotaLimits\":{\"status\":\"rejected\",\"resetsAt\":18000}}\n")).unwrap();}
                else {if refuse {return Err("native continuation unavailable; no replacement".into());} crate::runtime::rules_line(&log,"resume","fixture").unwrap();}
                let flag=Rc::new(RefCell::new(None));exits.borrow_mut().push((l.task,flag.clone()));
                Ok(Box::new(FakeProc(flag)) as Box<dyn Proc>)
            }) as Box<dyn FnMut(&Launch) -> Result<Box<dyn Proc>,crate::runtime::StartError>>
        };
        let mut s=Service::new(&b,&c,"fixture",starter());s.focus=vec!["all".into()];s.pass().unwrap();
        let a=task(&b,n).attempt.unwrap();*exits.borrow()[0].1.borrow_mut()=Some(1);s.pass().unwrap();
        assert_eq!((task(&b,n).state,task(&b,n).attempt,task(&b,n).retries),(if reviewer {State::Review} else {State::Doing},Some(a),0));
        drop(s);
        let mut s=Service::new(&b,&c,"fixture",starter());s.focus=vec!["all".into()];
        s.pass().unwrap();assert_eq!(calls.borrow().len(),1,"restart cannot retry the exhausted account");
        b.state.borrow_mut().clock=18000;let _=s.pass();
        assert_eq!(calls.borrow().len(),2);assert_eq!(calls.borrow()[1],(a,calls.borrow()[0].1.clone(),true));
        assert_eq!(task(&b,n).attempt,Some(a));
        if refuse {
            for _ in 0..3 {let _=s.pass();}
            assert_eq!(calls.borrow().len(),2,"unavailable continuation never replays or migrates");
            assert_eq!((task(&b,n).attempt,task(&b,n).retries),(Some(a),0));
            assert!(s.execution_waits[&n].contains("quota continuation unavailable"));
        }
        std::fs::remove_dir_all(root).unwrap();
        }
    }

    // #2137 A3: stale service selections and concurrent spawns cannot cross a board routing fence.
    #[test]
    fn balancing_reroute_fences_stale_service_claims() {
        for concurrent in [false,true] {
            let b=Fake::default();let root=std::env::temp_dir().join(format!("taskq-route-fence-{}",crate::runtime::uuid()));
            let mut c=cfg("mac");c.root=root.clone();c.install=root.join("install");
            c.limits.insert("codex".into(),1);
            let n=b.create("Reroute","",&["taskq".into(),"run-claude".into()]).unwrap();
            let v=views(&b,&mut Default::default(),&mut Default::default(),300).unwrap().0.remove(0);
            let exits:Exits=Default::default();let mut s=service(&b,&c,exits.clone());
            if concurrent {b.state.borrow_mut().race=Some("taskq: reroute claude codex\nrouting-fence: 1".into());}
            else {
                b.comment(n,"taskq: reroute claude codex\nrouting-fence: 1").unwrap();
                assert!(crate::dispatch::finish_reroute(&b,n).ok());
            }
            s.claim(&v,"worker","claude","").unwrap();
            assert!(exits.borrow().is_empty());assert_eq!(task(&b,n).state,State::Ready);
            s.pass().unwrap();s.pass().unwrap();
            assert_eq!(rt_of(&task(&b,n)).unwrap().0,"codex","automatic pending-intent reconciliation selects the new pin");
            assert_eq!(exits.borrow().len(),1);assert_eq!(task(&b,n).retries,0);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    // Chain: §5, #2137 A3. A paused finisher must not write pins after another service completes and admits work.
    #[test]
    fn balancing_reroute_stale_finisher_preserves_live_pins() {
        let b=Fake::default();let root=std::env::temp_dir().join(format!("taskq-stale-finisher-{}",crate::runtime::uuid()));
        let mut c=cfg("mac");c.root=root.clone();c.install=root.join("install");
        c.limits.insert("codex".into(),1);c.limits.insert("antigravity".into(),1);
        let n=b.create("Reroute","",&["taskq".into(),"run-claude".into()]).unwrap();
        b.comment(n,"taskq: reroute claude codex\nrouting-fence: 1").unwrap();
        let exits:Exits=Default::default();
        let (other,launched)=(c.clone(),exits.clone());
        b.state.borrow_mut().before_labels=Some(Box::new(move |b,n| {
            let mut second=service(b,&other,launched);second.focus=vec![];
            second.pass().unwrap();
            let pending=second.snapshot.as_ref().unwrap().tasks.iter().find(|r|r.n==n).unwrap().admission.as_ref().unwrap();
            assert!(pending.reason.contains("reroute"),"pending writer must explain why work waits");
            crate::reroute(b,&other.root,"codex","antigravity").unwrap();
            second.focus=vec!["all".into()];second.pass().unwrap();
        }));
        let receipt=crate::dispatch::finish_reroute(&b,n);
        let p=core::project(n,&crate::board::entries(&b,n).unwrap(),false);
        let pins:Vec<_>=b.issue(n).unwrap().labels.into_iter().filter(|l|l.starts_with("run-")).collect();
        assert_eq!(pins,[format!("run-{}",rt_of(&p.task).map_or("codex",|r|r.0))],"stale finisher corrupted the live pin");
        assert!(receipt.ok(),"the sole writer can finish: {}",receipt.text());
        assert!(exits.borrow().is_empty(),"the second service cannot admit while the first can still write");
        crate::reroute(&b,&c.root,"codex","antigravity").unwrap();
        let mut second=service(&b,&c,exits.clone());second.focus=vec!["all".into()];second.pass().unwrap();
        assert_eq!(rt_of(&task(&b,n)).unwrap().0,"antigravity");
        assert_eq!(b.issue(n).unwrap().labels.iter().filter(|l|l.starts_with("run-")).cloned().collect::<Vec<_>>(),["run-antigravity"]);
        assert_eq!(exits.borrow().len(),1);assert_eq!(task(&b,n).retries,0);
        std::fs::remove_dir_all(root).unwrap();
    }

    // A5/A6: failed quota persistence must fence every project, not select the exhausted default as its own spare.
    #[test]
    fn balancing_quota_write_failure_fences_other_projects() {
        let root=std::env::temp_dir().join(format!("taskq-quota-save-{}",crate::runtime::uuid()));
        let install=root.join("install");std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join(crate::runtime::accounts::FILE),r#"{"claude":[{"label":"default","enabled":true},{"label":"work","enabled":true}]}"#).unwrap();
        // A directory at the atomic writer's temporary path fails the save after a valid state read.
        let failed=install.join(format!("{}.{}.tmp",crate::runtime::accounts::STATE,std::process::id()));
        std::fs::create_dir(&failed).unwrap();
        let b=Fake::default();let mut c=cfg("mac");c.root=root.join("a");c.install=install.clone();
        let n=b.create("Quota","",&["taskq".into(),"run-claude".into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker claude:owned mac").unwrap();
        let t=task(&b,n);let log=crate::runtime::attempt_log(&c.root,"worker",n,a);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log,"{\"type\":\"taskq.account\",\"account\":\"default\"}\n").unwrap();
        let mut s=service(&b,&c,Default::default());
        assert!(s.account_limit(n,&t,a,"claude",(18000,true),10).0.is_none());
        // Once the failing aggregate path disappears, an unsaved observation must still be fenced.
        std::fs::remove_dir(failed).unwrap();
        let mut other=c.clone();other.root=root.join("b");
        let s2=service(&b,&other,Default::default());
        assert!(s2.admit_runtime("claude",false).is_err(),"another project must not select the exhausted default");
        assert!(crate::runtime::accounts::claim(&install,"claude",None,10,&root.join("probe")).is_err());
        drop(s);drop(s2);
        let mut restarted=service(&b,&other,Default::default());restarted.account_pass(10);
        let accounts=crate::runtime::accounts::Host::read(&install);
        assert_eq!(accounts.active("claude",10).as_deref(),Some("work"),"recover the journal, preserving the exhausted default and spare account: {:?}; {:?}",accounts.error,restarted.log);
        assert_eq!(accounts.state["claude"]["default"].until,Some(18000));
        std::fs::remove_dir_all(root).unwrap();
    }

    // #2137 A2/A3/A8: real admission and status share routing, disabled pins and account quota evidence.
    #[test]
    fn balancing_service_routing_and_blockers() {
        let b=Fake::default();let root=std::env::temp_dir().join(format!("taskq-balancing-service-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(root.join("install")).unwrap();
        let mut c=cfg("mac");c.root=root.clone();c.install=root.join("install");
        c.limits=BTreeMap::from([("claude".into(),0),("codex".into(),1),("antigravity".into(),1)]);
        c.routing=crate::labels::routing(&json!({"routing":[{"labels":["ui"],"prefer":"claude"}]})).unwrap();
        let soft=b.create("UI","",&["taskq".into(),"ui".into()]).unwrap();
        let pinned=b.create("Disabled strict pin","",&["taskq".into(),"run-claude".into()]).unwrap();
        let quota=b.create("Quota strict pin","",&["taskq".into(),"run-antigravity".into()]).unwrap();
        crate::runtime::accounts::record_windows(&c.install,"antigravity","default",0,&[("seven_day".into(),Some(604800))]).unwrap();
        let exits:Exits=Default::default();let mut s=service(&b,&c,exits.clone());s.focus=vec!["all".into()];
        for _ in 0..3 {s.pass().unwrap();}
        assert_eq!(rt_of(&task(&b,soft)).unwrap().0,"codex");
        assert_eq!(task(&b,pinned).state,State::Ready);assert_eq!(task(&b,quota).state,State::Ready);
        assert_eq!(exits.borrow().len(),1,"no repeated failed starts for exhausted accounts");
        let snap=s.snapshot.as_ref().unwrap();
        for (n,code) in [(pinned,"capacity"),(quota,"usage-limit")] {assert_eq!(snap.tasks.iter().find(|r|r.n==n).unwrap().admission.as_ref().unwrap().code,code);}
        let report=snap.text(&c,&Value::Null).unwrap();
        assert!(report.contains("claude disabled (limit 0)"),"{report}");
        assert!(report.contains("account default window seven_day"),"{report}");
        assert_eq!(s.cfg.limits,c.limits,"quota observations do not edit owner capacity");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: § 4 step 6 admission -> snapshot rows -> status.json and taskq-board/1 (#1616). Risk: DashQ or the tray
    // guess why a ready task waits, or a host that did not decide reads as admitted or refused.
    #[test]
    fn ready_rows_carry_this_hosts_admission_decision() {
        let b = Fake::default();
        let first = b.create("A", "", &["taskq".into()]).unwrap();
        let second = b.create("B", "", &["taskq".into()]).unwrap();
        let other = b.create("H", "", &["taskq".into(), "host-win".into()]).unwrap();
        b.create("taskq host win", "version 4.0.0", &[HOST_LABEL.into()]).unwrap();
        let win = b.create("W", "", &["taskq".into(), "needs-windows".into()]).unwrap();
        let bad = b.create("X", "", &["taskq".into(), "p1".into(), "p2".into()]).unwrap();
        let maps = b.create("M", "", &["taskq".into(), "area-maps".into()]).unwrap();
        let dep = b.create("D", &format!("Depends on #{first}"), &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        c.install = std::env::temp_dir().join(format!("taskq-admission-rows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&c.install);
        std::fs::create_dir_all(&c.install).unwrap();
        let mut s = service(&b, &c, Default::default());
        let codes = |s: &Service| -> BTreeMap<u64, Value> {
            let snap = s.snapshot.as_ref().unwrap();
            snap.tasks.iter().filter_map(|r| r.json().get("admission").map(|a| {
                if a["code"] != "dependency" { assert_eq!((&a["host"], &a["observed"]), (&json!("mac"), &json!(snap.observed)), "#{} decided by this host on this read", r.n); }
                (r.n, a["code"].clone())
            })).collect()
        };
        s.focus = vec![];
        s.pass().unwrap();
        let want = |pairs: &[(u64, &str)]| pairs.iter().map(|(n, c)| (*n, json!(c))).collect::<BTreeMap<_, _>>();
        assert_eq!(codes(&s), want(&[(first, "focus"), (second, "focus"), (other, "host"), (win, "capability"), (bad, "labels"), (maps, "focus"), (dep, "dependency")]), "focus off");
        s.focus = vec!["all".into()];
        s.pass().unwrap();
        assert_eq!(codes(&s), want(&[(first, "admitted"), (second, "capacity"), (other, "host"), (win, "capability"), (bad, "labels"), (maps, "capacity"), (dep, "dependency")]), "one claude slot");
        s.focus = vec!["area-maps".into()];
        s.logged_out.insert("claude".into());
        s.pass().unwrap();
        let got = codes(&s);
        assert_eq!((&got[&second], &got[&maps], got.contains_key(&first)), (&json!("focus"), &json!("login"), false), "a doing task carries no admission");
        std::fs::write(c.install.join("hold"), "fixture hold").unwrap();
        s.pass().unwrap();
        assert_eq!((&codes(&s)[&second], &codes(&s)[&maps]), (&json!("hold"), &json!("hold")));
        let _ = std::fs::remove_dir_all(&c.install);
        // the CLI read decides nothing: ready reads unknown with its reason, waiting keeps the fold's dependency
        let rows = crate::snapshot::read(&b, &c, &Value::Null, &[]).json()["tasks"].clone();
        let row = |n: u64| rows.as_array().unwrap().iter().find(|r| r["n"] == n).unwrap()["admission"].clone();
        assert_eq!((&row(maps)["code"], &row(maps)["reason"], &row(maps)["host"]), (&json!("unknown"), &json!("no host decision"), &Value::Null));
        assert_eq!(row(dep)["code"], "dependency");
        assert!(row(dep)["reason"].as_str().unwrap().contains(&format!("#{first} open")));
    }

    // Chain: § 4 Filter, reviewer waits for its runtime's slot -> snapshot row -> status/tray wait text (#1685). Risk: a
    // review refused on capacity sits silent in status and the tray, as three GitLab reviews did for two hours.
    #[test]
    fn review_rows_carry_this_hosts_admission_decision() {
        let b = Fake::default();
        let n = b.create("R", "", &["taskq".into()]).unwrap();
        let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
        accept_execution(&b, n, w, w, None).unwrap(); release_execution(&b, n, w, w, "exit").unwrap();
        b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40))).unwrap();
        let mut c = cfg("mac");
        c.limits = BTreeMap::from([("claude".to_string(), 0)]);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        assert!(e.borrow().is_empty(), "no reviewer slot");
        let row = s.snapshot.as_ref().unwrap().tasks.iter().find(|r| r.n == n).unwrap().json();
        assert_eq!((&row["state"], &row["admission"]["code"], &row["admission"]["host"]), (&json!("review"), &json!("capacity"), &json!("mac")));
        assert_eq!(row["wait"], "Waiting for a free slot · claude disabled (limit 0); codex not in limits; antigravity not in limits");
    }

    // Chain: `host-<name>` label -> heartbeat names -> board admission -> status wait text (R6). Risk (#1721): a task
    // labelled `host-host-mac` sits ready for 30 min while status shows plain `ready` on every host.
    #[test]
    fn host_label_without_heartbeat_shows_its_wait_in_status() {
        let b = Fake::default();
        let orphan = b.create("O", "", &["taskq".into(), "host-host-mac".into()]).unwrap();
        let other = b.create("W", "", &["taskq".into(), "host-win".into()]).unwrap();
        b.create("taskq host win", "version 4.0.0", &[HOST_LABEL.into()]).unwrap();
        let mut c = cfg("mac");
        c.limits = BTreeMap::from([("claude".to_string(), 0)]);
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        s.pass().unwrap();
        let rows = s.snapshot.as_ref().unwrap().json()["tasks"].clone();
        let row = |n: u64| rows.as_array().unwrap().iter().find(|r| r["n"] == n).unwrap().clone();
        assert_eq!((&row(orphan)["admission"]["code"], &row(orphan)["admission"]["host"], &row(orphan)["wait"]), (&json!("host"), &Value::Null, &json!("No host host-mac")));
        assert_eq!(row(other)["admission"]["host"], "mac", "a host that beats keeps this host's own decision");
        assert!(row(other)["wait"].is_null(), "another host's task is no wait of this host");
        let cli = crate::snapshot::read(&b, &c, &Value::Null, &[]);
        assert_eq!(cli.json()["tasks"].as_array().unwrap().iter().find(|r| r["n"] == orphan).unwrap()["wait"], "No host host-mac", "the CLI read decides it too");
        assert!(cli.text(&c, &Value::Null).unwrap().contains("| ready · No host host-mac |"));
    }

    // Chain: host admission refusal text -> admission code (#1616). Risk: a host.rs or admission.rs refusal reads as
    // the wrong code, or an unknown host state reads as a decision.
    #[test]
    fn host_refusals_map_to_admission_codes() {
        for (code, text, want) in [("max", "host at its maximum of 2 sessions", "capacity"), ("spacing", "host launched 3 s ago", "load-gate"),
            ("version", "service p 4.1.11 is older than 4.2.0-dev", "hold"), ("host", "admission file unreadable (x); rebuilt", "unknown"),
            ("host", "host occupancy recovery pending", "unknown"), ("host", "host admission poisoned; launches held", "unknown"),
            ("host", "project draining", "hold"), ("host", "host restart pending", "hold"), ("host", "runtime disabled", "capacity"),
            ("host", "round-robin: another project is ready", "capacity"), ("host", "host load gate or 30 s launch spacing", "load-gate"),
            ("host", "host or project capacity full", "capacity")] {
            let (got, reason) = refusal_admission(code, text);
            assert_eq!(got, want, "{code}: {text}");
            if got == "unknown" { assert!(crate::snapshot::ADMISSION_UNKNOWN.contains(&reason.as_str())); }
        }
    }

    // Chain: § 8 codex-app admission. Risk: codex-app runs an unlabelled task.
    #[test]
    fn codex_app_runs_only_labelled_tasks_on_opted_in_hosts() {
        let b = Fake::default();
        let app = b.create("A", "", &["taskq".into(), "run-codex-app".into()]).unwrap();
        let other = b.create("O", "", &["taskq".into()]).unwrap();
        let mut c = cfg("win");
        c.limits = BTreeMap::from([("codex-app".to_string(), 1)]);
        service(&b, &c, Default::default()).pass().unwrap();
        assert_eq!((task(&b, app).state, task(&b, other).state), (State::Ready, State::Ready), "no opt-in: unchanged");
        let m = Config { has: vec!["codex-app".into()], me: "mac".into(), ..c.clone() };
        service(&b, &m, Default::default()).pass().unwrap();
        let os = cfg!(any(target_os = "macos", windows));
        assert_eq!((task(&b, app).state, task(&b, other).state), (if os { State::Doing } else { State::Ready }, State::Ready), "only macOS and Windows admit app turns; unlabelled tasks never get the app");
        if os { assert!(task(&b, app).session.unwrap().starts_with("codex-app:- mac")); }
    }

    // Chain: § 3 capacity config. Risk: host override ignored or invalid limits accepted.
    #[test]
    fn capacity_merges_host_overrides_and_validates() {
        let v = json!({"limits": {"claude": 4, "codex": 4}, "load_gate": {"cpu_max": 90},
            "capacity_hosts": {"win": {"limits": {"codex": 2}, "load_gate": {"enabled": true}}}});
        let win = capacity(&v, "win").unwrap();
        assert_eq!(win.limits, BTreeMap::from([("claude".into(), 4), ("codex".into(), 2)]));
        assert_eq!(win.gate, GateCfg { enabled: true, cpu_max: 85, memory_min: 15, mean: false }, "a host gate replaces the default object");
        assert_eq!(capacity(&v, "mac").unwrap().gate, GateCfg { enabled: false, cpu_max: 90, memory_min: 15, mean: false });
        assert_eq!(capacity(&json!({"limits": {"claude": 1}}), "mac").unwrap(), Capacity::fixed(BTreeMap::from([("claude".into(), 1)]), GATE_DEFAULT), "absent keys: today");
        for bad in [json!({"limits": {"claude": -1}}), json!({"limits": {"claude": 5_000_000_000u64}}), json!({"load_gate": {"cpu_max": 9}}),
            json!({"load_gate": {"memory_min": 96}}), json!({"load_gate": {"enabled": "yes"}}), json!({"capacity_hosts": {"mac": {"limits": {"codex": "2"}}}})] {
            assert!(capacity(&bad, "mac").is_err(), "{bad}");
        }
    }

    fn on() -> GateCfg {
        GateCfg { enabled: true, ..GATE_DEFAULT }
    }

    /// Feed CPU busy percentages at 30 s steps from `t`; ticks advance by 100 per sample.
    fn feed(g: &mut Gate, t: &mut u64, ticks: &mut (u64, u64), samples: &[(f64, f64)]) {
        for (cpu, mem) in samples {
            *ticks = (ticks.0 + *cpu as u64, ticks.1 + 100);
            *t += 30;
            g.host_mem = Some((8.0, 1));
            g.observe(*t, *t as i64, Some(*ticks), Some(*mem));
        }
    }

    // Chain: § 3 load gate. Risk: starts under high load or never reopens.
    #[test]
    fn gate_holds_over_thresholds_and_reopens_after_90_s_calm() {
        let mut g = Gate::default();
        g.configure(GATE_DEFAULT);
        assert!(g.may_launch(0) && g.state(0).0 == "off", "off by default");
        g.configure(on());
        assert_eq!(g.state(0).0, "unknown", "enabled: held until sampled");
        let (mut t, mut k) = (0, (0, 0));
        g.observe(0, 0, Some(k), Some(50.0));
        assert_eq!(g.state(0), ("unknown", vec![("sample", "no CPU reading yet".into())]), "first CPU reading is a baseline");
        feed(&mut g, &mut t, &mut k, &[(50.0, 50.0), (50.0, 50.0)]);
        assert!(!g.may_launch(t) && g.state(t).0 == "held");
        feed(&mut g, &mut t, &mut k, &[(50.0, 50.0)]);
        assert!(g.may_launch(t), "three calm samples: 90 s");
        // exact thresholds hold; both reasons are kept
        feed(&mut g, &mut t, &mut k, &[(85.0, 15.0)]);
        assert_eq!(g.state(t).1.iter().map(|r| r.0).collect::<Vec<_>>(), ["cpu", "memory"]);
        // mid-band (CPU 80 > 85 - 10) resets recovery
        feed(&mut g, &mut t, &mut k, &[(50.0, 50.0), (50.0, 50.0), (80.0, 50.0), (50.0, 50.0), (50.0, 50.0)]);
        assert_eq!(g.state(t).0, "held");
        feed(&mut g, &mut t, &mut k, &[(50.0, 50.0)]);
        assert_eq!(g.state(t).0, "open");
        // a stale sample holds; a failed reading holds and resets
        assert_eq!(g.state(t + 61).0, "unknown");
        g.observe(t + 30, 0, Some(k), None);
        assert!(!g.may_launch(t + 30));
        // a threshold change starts held
        let mut g2 = Gate::default();
        g2.configure(on());
        let (mut t2, mut k2) = (0, (0, 0));
        g2.observe(0, 0, Some(k2), Some(50.0));
        feed(&mut g2, &mut t2, &mut k2, &[(10.0, 50.0); 3]);
        assert!(g2.may_launch(t2));
        g2.configure(GateCfg { cpu_max: 90, ..on() });
        assert!(!g2.may_launch(t2));
        // Auto: the 60 s mean smooths one busy interval; a 100 % interval still vetoes
        let mut g3 = Gate::default();
        g3.configure(GATE_AUTO);
        let (mut t3, mut k3) = (0, (0, 0));
        g3.observe(0, 0, Some(k3), Some(50.0));
        feed(&mut g3, &mut t3, &mut k3, &[(70.0, 50.0), (95.0, 50.0)]);
        assert_eq!(g3.last.unwrap().2, Some(82.5));
        feed(&mut g3, &mut t3, &mut k3, &[(100.0, 50.0)]);
        assert_eq!(g3.state(t3).1[0].0, "cpu");
        // Auto memory floor: max(memory_min %, 2.5 GiB) and macOS pressure level 4 hold; no reading is unknown
        feed(&mut g3, &mut t3, &mut k3, &[(10.0, 50.0); 3]);
        assert_eq!(g3.state(t3).0, "open");
        for (m, code) in [(Some((2.4, 1)), "memory"), (Some((8.0, PRESSURE_CRITICAL)), "memory"), (None, "sample")] {
            g3.host_mem = m;
            g3.observe(t3, t3 as i64, Some(k3), Some(50.0));
            assert_eq!(g3.state(t3).1.last().unwrap().0, code, "{m:?}");
            assert!(!g3.may_launch(t3));
        }
        g2.host_mem = None;
        assert_eq!(g2.state(t2).0, "held", "Fixed ignores the floor reading");
    }

    // Chain: § 3 load gate spacing. Risk: launch burst or bad counters accepted.
    #[test]
    fn gate_spaces_launches_and_rejects_bad_counters() {
        let mut g = Gate::default();
        g.configure(on());
        let (mut t, mut k) = (0, (0, 0));
        g.observe(0, 0, Some(k), Some(50.0));
        feed(&mut g, &mut t, &mut k, &[(10.0, 50.0); 3]);
        g.launch(t);
        assert!(!g.may_launch(t + 40), "no sample after the launch");
        feed(&mut g, &mut t, &mut k, &[(10.0, 50.0)]);
        assert!(g.may_launch(t), "post-launch sample, 30 s later");
        g.observe(t + 30, 0, Some((0, 0)), Some(50.0)); // counters went backwards
        assert_eq!(g.state(t + 30).0, "unknown");
    }

    // Chain: § 3 load gate with live attempts. Risk: the gate drops leases or blocks resumes.
    #[test]
    fn load_gate_holds_claims_and_resumes_keeps_leases_and_recovers() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        c.limits.insert("claude".into(), 2);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let a = task(&b, n).attempt.unwrap();
        let m = b.create("M", "", &["taskq".into(), "host-mac".into()]).unwrap();
        // gate on: held until three calm samples, the running child continues
        s.reload(Ok(Capacity::fixed(c.limits.clone(), on())));
        let (mut t, mut k) = (0, (0, 0));
        s.gate.observe(0, 0, Some(k), Some(50.0));
        feed(&mut s.gate, &mut t, &mut k, &[(90.0, 50.0)]);
        s.mono = t;
        b.comment(n, &core::entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        b.comment(n, &core::entry("answer", &["1"], None, "x")).unwrap();
        s.pass().unwrap();
        assert_eq!((e.borrow().len(), task(&b, m).state), (1, State::Ready), "no resume, no claim while held");
        assert!(b.comments(m).unwrap().is_empty(), "no spawn written while held");
        let hb = b.issues(None).unwrap().into_iter().find(is_host).unwrap();
        assert!(hb.body.lines().any(|l| l == format!("#{n} {a}")), "held resume keeps its lease: {}", hb.body);
        assert_eq!(s.capacity_json()["gate"], "held");
        // other hosts leave a listed lease alone
        let w = cfg("win");
        let mut other = service(&b, &w, Default::default());
        b.advance(400);
        s.pass().unwrap();
        other.pass().unwrap();
        assert_eq!(task(&b, n).attempt, Some(a));
        // recovery: the resume goes first, the claim waits for a post-launch sample and 30 s
        feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0); 3]);
        s.mono = t;
        s.pass().unwrap();
        assert_eq!((e.borrow().len(), task(&b, m).state), (2, State::Ready), "one launch per 30 s");
        feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0)]);
        s.mono = t;
        s.pass().unwrap();
        assert_eq!(task(&b, m).state, State::Doing);
        assert_eq!(s.capacity_json()["used"]["claude"], 2);
        // invalid config holds and reports, the children continue
        s.reload(Err("limits.claude: not an integer".into()));
        assert_eq!(s.capacity_json()["reasons"][0]["code"], "config");
        assert_eq!(s.kids.len(), 2);
    }

    // Chain: resource policy write (shared-resource-controls.md § Host adapter) -> lower limit -> service reload ->
    // pass. Risk: a lowered limit stops or drops the lease of an attempt that is already running.
    #[test]
    fn lowered_limit_keeps_running_attempts() {
        let b = Fake::default();
        let n = b.create("T", "", &["taskq".into()]).unwrap();
        let m = b.create("M", "", &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        c.limits.insert("claude".into(), 2);
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        let (a, am) = (task(&b, n).attempt, task(&b, m).attempt);
        let o = b.create("O", "", &["taskq".into()]).unwrap();
        s.reload(Ok(Capacity::fixed(BTreeMap::from([("claude".to_string(), 0)]), GATE_DEFAULT)));
        b.advance(400);
        s.pass().unwrap();
        assert_eq!((s.kids.len(), task(&b, n).state, task(&b, m).state), (2, State::Doing, State::Doing), "both attempts run on");
        assert_eq!((task(&b, n).attempt, task(&b, m).attempt), (a, am), "leases kept");
        assert_eq!(task(&b, o).state, State::Ready, "no new claim under the lower limit");
    }

    // Chain: § 9 Auto admission config. Risk: Auto silently off, or a cap or invalid mode accepted wrong.
    #[test]
    fn auto_admission_takes_d03_defaults_and_caps() {
        let a = capacity(&json!({"admission": {"mode": "auto", "max": 6}, "limits": {"codex": 2}}), "mac").unwrap();
        assert_eq!((a.auto, a.max, a.gate), (true, Some(6), GATE_AUTO));
        let tuned = capacity(&json!({"capacity_hosts": {"mac": {"admission": {"mode": "auto"}, "load_gate": {"cpu_max": 80}}}}), "mac").unwrap();
        assert_eq!((tuned.gate.cpu_max, tuned.gate.memory_min, tuned.gate.enabled), (80, 10, true), "a host gate field overrides one default");
        assert!(!capacity(&json!({}), "mac").unwrap().auto, "absent: Fixed, today's behavior");
        for bad in [json!({"admission": "auto"}), json!({"admission": {"mode": "fast"}}), json!({"admission": {"max": -1}}),
            json!({"admission": {"mode": "auto"}, "load_gate": {"enabled": false}})] {
            assert!(capacity(&bad, "mac").is_err(), "{bad}");
        }
    }

    // Chain: § 3 host authority under Auto. Risk: a launch without a reservation, a cap overrun or admission on unknown data.
    #[test]
    fn auto_service_reserves_holds_at_cap_releases_and_refuses_unknown() {
        let install = std::env::temp_dir().join(format!("taskq-auto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&install);
        std::fs::create_dir_all(&install).unwrap();
        let b = Fake::default();
        let ns: Vec<u64> = (0..3).map(|i| b.create(&format!("T{i}"), "", &["taskq".into()]).unwrap()).collect();
        let mut c = cfg("mac");
        c.install = install.clone();
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.reload(capacity(&json!({"admission": {"mode": "auto", "max": 1}}), "mac"));
        // unknown: no sample yet, nothing claimed
        s.pass().unwrap();
        assert_eq!((s.capacity_json()["gate"].as_str(), e.borrow().len()), (Some("unknown"), 0));
        let (mut t, mut k) = (0, (0, 0));
        s.gate.observe(0, 0, Some(k), Some(50.0));
        feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0); 3]);
        (s.mono, s.wall) = (t, t as i64);
        // admit: one reservation, held as a child
        s.pass().unwrap();
        assert_eq!((e.borrow().len(), task(&b, ns[0]).state), (1, State::Doing));
        let cap = s.capacity_json();
        assert_eq!((cap["host_used"].as_u64(), cap["reservations"][0]["state"].as_str()), (Some(1), Some("held")));
        // hold at the host cap of 1: requested with a reason, no spawn written
        feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0); 2]);
        (s.mono, s.wall) = (t, t as i64);
        s.pass().unwrap();
        assert_eq!((e.borrow().len(), task(&b, ns[1]).state), (1, State::Ready));
        let cap = s.capacity_json();
        assert_eq!((cap["gate"].as_str(), cap["reasons"][0]["code"].as_str()), (Some("held"), Some("max")));
        assert_eq!(cap["reservations"][1]["state"], "requested");
        // release: the child exits, the next pass admits the next task
        b.comment(ns[0], &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], task(&b, ns[0]).attempt, "ok")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        s.sync_host();
        feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0)]);
        (s.mono, s.wall) = (t, t as i64);
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), 2, "released slot admits one more (reviewer or next task)");
        // unknown host data: an unreadable host file refuses and is rebuilt
        std::fs::write(install.join("admission.json"), "{").unwrap();
        *e.borrow()[1].1.borrow_mut() = Some(0);
        feed(&mut s.gate, &mut t, &mut k, &[(10.0, 50.0)]);
        (s.mono, s.wall) = (t, t as i64);
        let before = e.borrow().len();
        s.pass().unwrap();
        assert_eq!(e.borrow().len(), before, "refused on unknown");
        assert_eq!(s.capacity_json()["gate"], "unknown");
        let _ = std::fs::remove_dir_all(&install);
    }

    // Chain: § 4 history events file. Risk: duplicate events or unbounded growth.
    #[test]
    fn events_append_dedup_and_trim() {
        let p = std::env::temp_dir().join(format!("taskq-events-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let m = |n, k, a, o: &str| (n, k, a, o.to_string());
        assert_eq!(record(&p, 1000, &[m(1, "close", 7, "delivered")]).unwrap(), 1000);
        // #1800: the § 7 outcome follows a close and does not defeat the dedup; other kinds keep 4 fields
        record(&p, 2000, &[m(1, "close", 7, "delivered"), m(2, "failed", 7, ""), m(3, "close", 8, "replaced by #4 (ready)")]).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "since 1000\n1000 #1 close 7 delivered\n2000 #2 failed 7\n2000 #3 close 8 replaced by #4 (ready)\n", "same attempt and kind once");
        let late = 1500 + MONTH;
        assert_eq!(record(&p, late, &[]).unwrap(), 1500);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "since 1500\n2000 #2 failed 7\n2000 #3 close 8 replaced by #4 (ready)\n", "older than 30 days dropped");
        let _ = std::fs::remove_file(&p);
    }

    // Chain: section 7 close without merge -> task PR closed with the close reason; requeue, later and failed keep it
    // open; an unknown PR state is logged and left open.
    #[test]
    fn only_a_closed_task_closes_its_open_pr() {
        for fail in [false, true] {
            let (b, c) = (Fake::default(), cfg("win"));
            let ns: Vec<u64> = ["close", "requeue", "later", "failed"].iter().map(|t| {
                let n = b.create(t, "", &["taskq".into()]).unwrap();
                b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
                b.comment(n, "taskq: later\nparked").unwrap();
                n
            }).collect();
            b.state.borrow_mut().fail_task_prs = fail;
            let mut s = Service::new(&b, &c, "test", Box::new(|_| panic!("no process")));
            s.pass().unwrap();
            b.comment(ns[0], "taskq: close\nsuperseded by #9").unwrap();
            for (n, kind) in ns[1..].iter().zip(["requeue", "later", "failed"]) { b.comment(*n, &format!("taskq: {kind}\nwork may resume")).unwrap(); }
            s.pass().unwrap();
            s.pass().unwrap();
            assert!(!b.issue(ns[0]).unwrap().open);
            assert_eq!(b.state.borrow().closed_task_prs, [(ns[0], "superseded by #9".to_string())], "once, closed task only");
            // #1800: the close event names the successor and its state (#9 is not on the board)
            assert!(s.log.iter().any(|l| l.starts_with(&format!("#{} event close ", ns[0])) && l.ends_with(" replaced by #9 (unknown)")), "{:?}", s.log);
            assert_eq!(s.log.iter().any(|l| l == &format!("#{} task PR left open, state unknown: PR state unknown", ns[0])), fail, "{:?}", s.log);
        }
    }

    // Chain: #1173 logged state change -> pass replay -> `taskq-transition` record -> report. Risk: a transition
    // missed, doubled across passes or a restart, recorded by another host's service, or a zero entry time.
    #[test]
    fn a_pass_records_each_logged_transition_once() {
        let b = Fake::default();
        let (x, y) = (b.create("X", "", &["taskq".into()]).unwrap(), b.create("Y", "", &["taskq".into()]).unwrap());
        let z = b.create("Z", "", &["taskq".into()]).unwrap();
        b.comment(z, &core::entry("hold", &[], None, "")).unwrap();
        let mut c = cfg("mac");
        c.limits.insert("claude".into(), 1);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let _ = crate::profile::take_moves();
        let ax = task(&b, x).attempt.unwrap();
        // another host claims y: its service records that move, not this one
        b.comment(y, &core::entry("spawn", &["worker", "codex:s", "win"], None, "")).unwrap();
        b.comment(x, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(ax), "ok")).unwrap();
        // the result and the requeue happen between two passes: both are recorded; the requeue left `review` without
        // an attempt, so every service records it without host
        b.comment(x, &core::entry("requeue", &[], None, "again")).unwrap();
        // `held` is outside the queue: neither `hold` nor this `start` is a transition
        b.comment(z, &core::entry("start", &[], None, "")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let created = |id: u64| b.state.borrow().comments.iter().find(|(_, c)| c.id == id).unwrap().1.created;
        let ids: Vec<u64> = core::changes(x, &entries(&b, x).unwrap()).iter().map(|e| e.0).collect();
        let (records, report) = crate::profile::take_moves();
        let r: Vec<serde_json::Value> = records.lines().map(|l| serde_json::from_str(l.strip_prefix("taskq-transition ").unwrap()).unwrap()).collect();
        let row = |v: &serde_json::Value| (v["task"].as_u64().unwrap(), v["from"].as_str().unwrap().to_string(), v["to"].as_str().unwrap().to_string(), v["prev_s"].as_u64(), v["host"].as_str().map(String::from));
        let mac = Some("mac".to_string());
        // the issue creation is the entry into ready: no new task shows `no entry time` (#1344)
        let opened = b.state.borrow().issues[x as usize - 1].created;
        assert_eq!(r.iter().map(row).collect::<Vec<_>>()[..3], [(x, "ready".into(), "doing".into(), Some((created(ids[0]) - opened) as u64), mac.clone()),
            (x, "doing".into(), "review".into(), Some((created(ids[1]) - created(ids[0])) as u64), mac.clone()),
            (x, "review".into(), "ready".into(), Some((created(ids[2]) - created(ids[1])) as u64), None)], "{records}");
        assert!(r.iter().all(|v| v["from"] != "held" && v["to"] != "held"), "{records}");
        assert!(r.iter().all(|v| v["task"] != y) && r[..2].iter().all(|v| v["rt"] == "claude") && r[2]["rt"].is_null(), "{records}");
        assert!(report.contains("| doing | review | claude | 1 |") && report.contains("| ready | doing | claude |"), "{report}");
        // the next pass and a restarted service record none of them again
        s.pass().unwrap();
        let again = crate::profile::take_moves().0;
        assert!(!again.contains("\"to\":\"review\""), "{again}");
        let mut restarted = service(&b, &c, Default::default());
        restarted.pass().unwrap();
        restarted.pass().unwrap();
        assert!(!crate::profile::take_moves().0.contains("\"to\":\"review\""));
    }

    // Chain: #1344 ready -> doing -> review -> deferred publish -> merge queue -> close, replayed by a pass. Risk: review
    // time absorbs the merge queue wait, the reviewer's runtime and host are lost, or a state has no entry time.
    #[test]
    fn a_pass_splits_review_from_the_merge_queue_wait() {
        let b = Fake::default();
        let x = b.create("X", "", &["taskq".into()]).unwrap();
        let mut c = cfg("mac");
        c.limits.insert("claude".into(), 1);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let _ = crate::profile::take_moves();
        let aw = task(&b, x).attempt.unwrap();
        b.comment(x, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(aw), "ok")).unwrap();
        let ar = b.comment(x, &core::entry("spawn", &["reviewer", "codex:r", "mac"], None, "")).unwrap();
        b.comment(x, &core::entry("publish", &[], Some(ar), "PR 1")).unwrap();
        b.comment(x, &core::entry("enqueued", &["MQ_1"], None, "")).unwrap();
        b.comment(x, &core::entry("close", &[], None, "merged")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let (records, report) = crate::profile::take_moves();
        let r: Vec<serde_json::Value> = records.lines().map(|l| serde_json::from_str(l.strip_prefix("taskq-transition ").unwrap()).unwrap()).collect();
        let row = |v: &serde_json::Value| format!("{} {} {} {} {}", v["from"].as_str().unwrap(), v["to"].as_str().unwrap(),
            v["rt"].as_str().unwrap_or("-"), v["host"].as_str().unwrap_or("-"), v["prev_s"].is_u64());
        assert_eq!(r.iter().map(row).collect::<Vec<_>>(), ["ready doing claude mac true", "doing review claude mac true",
            "review merge-queue codex mac true", "merge-queue closed - - true"], "{records}");
        assert!(report.contains("| review | merge-queue | codex |") && report.contains("| merge-queue | closed |"), "{report}");
        assert!(!report.contains("no entry time"), "{report}");
    }

    // Chain: § 4 history across restart. Risk: a restart loses or doubles events.
    #[test]
    fn a_pass_records_close_and_failed_and_a_restart_keeps_them() {
        let root = std::env::temp_dir().join(format!("taskq-ev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let b = Fake::default();
        let (x, y) = (b.create("X", "", &["taskq".into()]).unwrap(), b.create("Y", "", &["taskq".into()]).unwrap());
        let mut c = cfg("mac");
        c.root = root.clone();
        c.limits.insert("claude".into(), 2);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        s.pass().unwrap();
        let (ax, ay) = (task(&b, x).attempt.unwrap(), task(&b, y).attempt.unwrap());
        b.comment(x, &core::entry("result", &["0123456789abcdef0123456789abcdef01234567"], Some(ax), "ok")).unwrap();
        *e.borrow()[0].1.borrow_mut() = Some(0);
        s.pass().unwrap();
        let r = task(&b, x).attempt.unwrap();
        b.comment(x, &core::entry("close", &[], Some(r), "ok")).unwrap();
        b.close(x).unwrap();
        b.comment(y, &core::entry("failed", &[], None, "no way")).unwrap();
        s.pass().unwrap();
        let ev = std::fs::read_to_string(root.join(".taskq/events")).unwrap();
        let lines: Vec<&str> = ev.lines().skip(1).map(|l| l.split_once(' ').unwrap().1).collect();
        // #1800: a close line ends with its § 7 outcome (verdict `ok`: delivered)
        assert_eq!(lines, [format!("#{x} close {r} delivered"), format!("#{y} failed {ay}")], "{ev}");
        let mut again = service(&b, &c, Default::default());
        again.pass().unwrap();
        again.pass().unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".taskq/events")).unwrap(), ev, "a restart keeps history");
        assert!(again.events_since.is_some());
        // a new task enters waiting (no comment: the issue number), a worker asks (its attempt)
        let z = b.create("Z", &format!("Depends on #{y}"), &["taskq".into()]).unwrap();
        let w = b.create("W", "", &["taskq".into()]).unwrap();
        again.pass().unwrap();
        let aw = task(&b, w).attempt.unwrap();
        let q1 = b.comment(w, &core::entry("ask", &[], Some(aw), "Which?\n1. x\n2. y ★")).unwrap();
        again.pass().unwrap();
        again.pass().unwrap();
        // #1712: the answer resumes the same attempt; its second ask is a second event, not a duplicate of the first
        b.comment(w, &core::entry("answer", &["1"], None, "")).unwrap();
        again.pass().unwrap();
        let q2 = b.comment(w, &core::entry("ask", &[], Some(aw), "Where?\n1. x\n2. y ★")).unwrap();
        again.pass().unwrap();
        again.pass().unwrap();
        let ev = std::fs::read_to_string(root.join(".taskq/events")).unwrap();
        let lines: Vec<&str> = ev.lines().skip(3).map(|l| l.split_once(' ').unwrap().1).collect();
        assert_eq!(lines, [format!("#{z} blocked {z}"), format!("#{w} ask {q1}"), format!("#{w} ask {q2}")], "{ev}");
        // a `gone` entry after the first pass is one `gone` event, once
        let v = b.create("V", "", &["taskq".into()]).unwrap();
        again.pass().unwrap();
        let av = task(&b, v).attempt.unwrap();
        b.comment(v, &core::entry("gone", &[], Some(av), "lost")).unwrap();
        again.pass().unwrap();
        again.pass().unwrap();
        let ev = std::fs::read_to_string(root.join(".taskq/events")).unwrap();
        assert_eq!(ev.lines().filter(|l| l.ends_with(&format!("#{v} gone {av}"))).count(), 1, "{ev}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: R6 stuck -> service log. Risk: a stuck task logs every pass, or never again after it moves.
    #[test]
    fn stuck_task_logs_once_until_it_moves() {
        let b = Fake::default();
        let c = cfg("mac");
        let n = b.create("Parked", "", &["taskq".into()]).unwrap();
        b.comment(n, &core::entry("later", &[], None, "parked")).unwrap();
        let mut s = service(&b, &c, Default::default());
        let lines = |s: &Service| s.log.iter().filter(|l| l.starts_with(&format!("#{n} event stuck later "))).count();
        s.pass().unwrap();
        assert_eq!(lines(&s), 0, "a fresh park is not stuck");
        b.advance(crate::snapshot::STUCK_AFTER);
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!(lines(&s), 1, "{:?}", s.log);
        assert_eq!(task(&b, n).state, State::Later, "observe only");
        b.comment(n, &core::entry("later", &[], None, "still parked")).unwrap();
        s.pass().unwrap();
        b.advance(crate::snapshot::STUCK_AFTER);
        s.pass().unwrap();
        assert_eq!(lines(&s), 2, "a move rearms the line");
    }

    // Chain: § 4 Churn -> snapshot rows, status lines, service log (#1905). Risk: a looping task (taskq#1903: 144 notes,
    // 64 spawns, 17 accepted) grows its notes unseen, or a normal task with an accepted spawn is flagged.
    #[test]
    fn churning_task_is_flagged_once_and_a_normal_task_not() {
        let b = Fake::default();
        let c = cfg("mac");
        let park = |title: &str| { let n = b.create(title, "", &["taskq".into()]).unwrap(); b.comment(n, &core::entry("later", &[], None, "parked")).unwrap(); n };
        let (looping, noisy, normal) = (park("Looping"), park("Noisy"), park("Normal"));
        for _ in 0..6 { b.comment(looping, "taskq: spawn worker codex:- pds18").unwrap(); }
        for i in 0..100 { b.comment(noisy, &format!("note {i}")).unwrap(); }
        let spawn = b.comment(normal, "taskq: spawn worker codex:- pds18").unwrap();
        b.comment(normal, &format!("taskq: accepted {spawn} attempt {spawn}")).unwrap();
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        s.pass().unwrap();
        let lines = crate::snapshot::churn_lines(&s.snapshot.as_ref().unwrap().json());
        assert_eq!(lines, [format!("#{looping} churn: notes 7 · spawns 6 · accepted 0"), format!("#{noisy} churn: notes 101 · spawns 0 · accepted 0")]);
        let events = |s: &Service| s.log.iter().filter(|l| l.contains(" event churn ")).cloned().collect::<Vec<_>>();
        assert_eq!(events(&s), [format!("#{looping} event churn notes 7 spawns 6 accepted 0"), format!("#{noisy} event churn notes 101 spawns 0 accepted 0")]);
        assert_eq!(task(&b, looping).state, State::Later, "observe only");
        b.advance(3601);
        s.pass().unwrap();
        let lines = crate::snapshot::churn_lines(&s.snapshot.as_ref().unwrap().json());
        assert_eq!(lines, [format!("#{noisy} churn: notes 101 · spawns 0 · accepted 0")], "spawns older than an hour clear the flag");
        assert_eq!(events(&s).len(), 2, "one line per flag");
    }

    // Chain: § 2 timed park -> service requeue at the board time (#1823). Risk: a time-only wait respawns at once and
    // escalates through the retry limit, a restart loses the time, or a bad time wakes the task.
    #[test]
    fn timed_park_wakes_at_its_board_time_after_restart() {
        let b = Fake::default();
        b.state.borrow_mut().clock = crate::board::iso("2026-11-05T08:00:00Z").unwrap();
        let c = cfg("mac");
        let timed = b.create("Timed", "", &["taskq".into()]).unwrap();
        b.comment(timed, &core::entry("later", &["until", "2026-11-05T09:00:00Z"], None, "Reason: 168 qualified hours")).unwrap();
        let bad = b.create("Bad", "", &["taskq".into()]).unwrap();
        b.comment(bad, &core::entry("later", &["until", "2026-02-30T09:00:00Z"], None, "Reason: bad time")).unwrap();
        let owner = b.create("Owner", "", &["taskq".into()]).unwrap();
        b.comment(owner, &core::entry("later", &["until", "2027-01-01T00:00:00Z"], None, "Reason: far")).unwrap();
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        s.pass().unwrap();
        assert_eq!((task(&b, timed).state, task(&b, timed).attempt), (State::Later, None), "no spawn before the time");
        assert_eq!(task(&b, timed).until.as_deref(), Some("2026-11-05T09:00:00Z"));
        b.comment(owner, &core::entry("requeue", &[], None, "Reason: owner wants it now")).unwrap();
        drop(s);
        b.advance(3600);
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        s.pass().unwrap();
        s.pass().unwrap();
        let wakes = || b.state.borrow().comments.iter().filter(|(i, c)| *i == timed && c.body.starts_with("taskq: requeue")).count();
        assert_eq!(wakes(), 1, "one wake after the restart");
        assert!(matches!(task(&b, timed).state, State::Ready | State::Doing) && task(&b, timed).retries == 0, "{:?}", task(&b, timed));
        assert!(task(&b, owner).attempt.is_some(), "an owner requeue overrides the time");
        assert_eq!((task(&b, bad).state, task(&b, bad).until), (State::Later, None), "a bad time parks untimed");
    }

    // Chain: R6 stuck review -> service log (#1752). Risk: a review no host can start sits silent for hours.
    #[test]
    fn review_without_reviewer_logs_stuck_once() {
        let b = Fake::default();
        let n = b.create("R", "", &["taskq".into()]).unwrap();
        let w = b.comment(n, "taskq: spawn worker codex:w mac").unwrap();
        accept_execution(&b, n, w, w, None).unwrap(); release_execution(&b, n, w, w, "exit").unwrap();
        b.comment(n, &format!("taskq: result {} attempt {w}", "a".repeat(40))).unwrap();
        let mut c = cfg("mac");
        c.limits = BTreeMap::from([("claude".to_string(), 0)]);
        let e: Exits = Default::default();
        let mut s = service(&b, &c, e.clone());
        let lines = |s: &Service| s.log.iter().filter(|l| l.starts_with(&format!("#{n} event stuck review "))).count();
        s.pass().unwrap();
        assert_eq!(lines(&s), 0, "a fresh review is not stuck");
        b.advance(crate::snapshot::STUCK_AFTER);
        s.pass().unwrap();
        s.pass().unwrap();
        assert!(e.borrow().is_empty(), "no reviewer slot");
        assert_eq!(lines(&s), 1, "{:?}", s.log);
        assert_eq!(task(&b, n).state, State::Review, "observe only");
    }

    #[test]
    fn new_worktrees_get_a_private_target_seed() {
        // Chain: first worker start -> worktree + cloned target/debug. Risk: a seed that writes into or shares its source.
        let base = std::env::temp_dir().join(format!("taskq-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (origin, root) = (base.join("origin"), base.join("root"));
        let git = |dir: &std::path::Path, args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap().status.success(), "{args:?}");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        git(&origin, &["commit", "-q", "--allow-empty", "-m", "init"]);
        git(&base, &["clone", "-q", origin.to_str().unwrap(), "root"]);
        // Defect cs#548 (#2078 A2): without a target/debug no worktree was made, and a worker whose sandbox cannot write
        // the shared .git burned its attempts. Protection: the service creates it, then continues the existing branch.
        assert_eq!(seed(&root, 6).as_deref(), Some("#6 worktree created"), "no target/debug: the worktree still comes first");
        let wt6 = root.join(".worktrees/taskq-6");
        git(&wt6, &["commit", "-q", "--allow-empty", "-m", "wip"]);
        assert_eq!(seed(&root, 6), None, "an existing worktree is continued untouched");
        std::fs::remove_dir_all(&wt6).unwrap();
        assert_eq!(seed(&root, 6).as_deref(), Some("#6 worktree created"), "a deleted folder's stale registration is pruned");
        let log = std::process::Command::new("git").arg("-C").arg(&wt6).args(["log", "-1", "--format=%s"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "wip", "the existing taskq-6 branch is continued, not reset to origin/main");
        std::fs::create_dir_all(root.join("target/debug/deps")).unwrap();
        std::fs::write(root.join("target/debug/deps/libx.rlib"), "warm").unwrap();
        let line = seed(&root, 7).unwrap();
        let copy = root.join(".worktrees/taskq-7/target/debug/deps/libx.rlib");
        assert!(root.join(".worktrees/taskq-7/.git").exists(), "{line}");
        if cfg!(target_os = "macos") { assert_eq!(line, "#7 seeded target/debug", "APFS temp dir clones"); }
        // #2051: where the volume has no reflink (ext4, as on CI), the seed copies plainly instead of skipping.
        if cfg!(target_os = "linux") {
            let probe = std::process::Command::new("cp").arg("--reflink=always").arg(root.join("target/debug/deps/libx.rlib")).arg(base.join("probe")).output().unwrap();
            eprintln!("reflink here: {}", if probe.status.success() { "supported" } else { "unsupported, plain-copy path" });
            assert_eq!(line, "#7 seeded target/debug", "a volume without reflink still seeds");
        }
        if line == "#7 seeded target/debug" {
            std::fs::write(&copy, "edited").unwrap();
            assert_eq!(std::fs::read_to_string(root.join("target/debug/deps/libx.rlib")).unwrap(), "warm", "the seed never writes into its source");
        } else {
            assert!(line.starts_with("#7 seed skipped: ") && !copy.exists(), "{line}");
        }
        assert_eq!(seed(&root, 7), None, "an existing worktree is continued, never reseeded");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir_all(root.join(".worktrees/taskq-5/target/debug/newest")).unwrap();
        if seed(&root, 8).unwrap() == "#8 seeded target/debug" {
            assert!(root.join(".worktrees/taskq-8/target/debug/newest").exists(), "the newest target/debug is the source");
        }
        // #1856: a failed fetch logs the command and Git's error, never a silent skip; the last fetched origin/main still
        // gives the worker its worktree (#2078)
        std::fs::remove_dir_all(&origin).unwrap();
        let failed = seed(&root, 9).unwrap();
        assert!(failed.contains(" (fetch failed: `git -C ") && failed.contains("fetch -q origin main` failed: "), "{failed}");
        assert!(root.join(".worktrees/taskq-9/.git").exists(), "{failed}");
        let _ = std::fs::remove_dir_all(&base);
    }

    // Chain: closed task -> execution release -> worktree cleanup. Risk: deleting dirty or still-owned work.
    // Defect #1549: other hosts' executions and pre-boot own ones held worktrees forever, logged every restart.
    #[test]
    fn closed_tasks_lose_clean_worktrees_only() {
        fn no_boot() -> Result<i64, String> { Err("boot time not read in tests".into()) }
        fn booted() -> Result<i64, String> { Ok(2_000_000_000) }
        let root = std::env::temp_dir().join(format!("taskq-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&root).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap().status.success(), "{args:?}");
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        for n in ["1", "2", "3", "4", "5"] {
            git(&["worktree", "add", "-q", "-b", &format!("taskq-{n}"), &format!(".worktrees/taskq-{n}")]);
        }
        std::fs::write(root.join(".worktrees/taskq-2/wip"), "x").unwrap();
        let b = Fake::default();
        for _ in 0..5 {
            b.create("T", "", &["taskq".into()]).unwrap();
        }
        let execution = b.comment(4, "taskq: spawn worker claude:owned mac").unwrap();
        b.comment(5, "taskq: spawn worker claude:remote win").unwrap();
        for n in [1, 2, 4, 5] { b.close(n).unwrap(); }
        let mut c = cfg("mac");
        c.root = root.clone();
        let mut s = service(&b, &c, Default::default());
        s.pass().unwrap();
        let wt = |n: &str| root.join(format!(".worktrees/taskq-{n}")).exists();
        assert_eq!((wt("1"), wt("2"), wt("3"), wt("5")), (false, true, true, false), "clean closed removed; dirty and open stay; another host's execution never ran here");
        let kept = |s: &Service| s.log.iter().filter(|l| l.starts_with("#4 worktree kept") && l.contains("reboot this host")).count();
        assert_eq!(kept(&s), 1, "undecided own execution names the owner step: {:?}", s.log);
        let mut restarted = service(&b, &c, Default::default());
        restarted.pass().unwrap();
        assert_eq!(kept(&restarted), 0, "a restart with the same outcome logs nothing: {:?}", restarted.log);
        let cleanup = |boot, force| crate::tidy::Tidy { board: &b, root: &root, me: "mac", boot, external: false, force, dry: false }.run(&mut |_| false, &|_| None).log;
        cleanup(no_boot, true);
        assert_eq!((wt("2"), wt("3")), (false, true), "force removes dirty closed; open stays");
        assert!(wt("4"), "even forced cleanup preserves an unresolved execution's worktree");
        let log = cleanup(booted, false);
        assert!(!wt("4"), "a boot after intent releases and permits cleanup: {log:?}");
        assert!(core::executions(4, &crate::board::entries(&b, 4).unwrap()).iter().all(|x| x.id != execution || x.release == Some(core::Release::Exit)));
        assert!(std::fs::read_to_string(root.join(".taskq/trash/taskq-2.patch")).unwrap().contains("wip"), "dirty work saved");
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: tidy conditions -> `.taskq/findings.json` -> held finding (docs/design/worktrees.md Findings, #2060).
    // Defect: hosts repeated the same dirty, stale and kept tidy lines for weeks and nothing filed a task. Kept
    // protection: one finding per root and condition after a day, none while it is open, a new day once it is closed,
    // and a board without a claim issue logs its reason once.
    #[test]
    fn tidy_conditions_file_one_finding_per_condition_after_a_day() {
        let root = std::env::temp_dir().join(format!("taskq-tidy-findings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let b = Fake::default();
        b.create("Claims", "", &[]).unwrap();
        let mut c = cfg("mac");
        c.root = root.clone();
        let dirty = BTreeMap::from([("dirty", vec!["#7 worktree dirty, untouched 3d".to_string()])]);
        let mut log = vec![];
        let mut run = |conds: &BTreeMap<&'static str, Vec<String>>, now: u64| { log.clear(); tidy_findings(&b, &c, conds, now, &mut log); log.clone() };
        let t0 = 1_800_000_000;
        // no claim issue configured: nothing filed, the reason logs once
        assert!(run(&dirty, t0).is_empty());
        let unfiled = run(&dirty, t0 + 86_401);
        assert_eq!(unfiled, ["tidy finding not filed: configure create_claim_issue before idempotent create"]);
        assert!(run(&dirty, t0 + 86_402).is_empty(), "the same reason logs once");
        std::fs::write(root.join("taskq.json"), r#"{"create_claim_issue": 1}"#).unwrap();
        assert_eq!(run(&dirty, t0 + 86_403), ["#2 finding filed: tidy dirty over a day"]);
        let issue = b.issue(2).unwrap();
        assert!(issue.title.starts_with("Task worktree dirty over a day: ") && is_held(&issue), "{}", issue.title);
        assert!(issue.body.contains(&format!("## Source\nmac/tidy-dirty/{}-{t0}\n", crate::project_name(&root))), "{}", issue.body);
        assert!(issue.body.contains("## Evidence\n- #7 worktree dirty, untouched 3d\n"), "{}", issue.body);
        assert!(run(&dirty, t0 + 5 * 86_400).is_empty(), "no repeat while the finding is open");
        assert_eq!(b.issues(Some(0)).unwrap().len(), 2);
        // closed while the condition holds: a new day starts, then a second finding
        b.close(2).unwrap();
        let t1 = t0 + 6 * 86_400;
        assert!(run(&dirty, t1).is_empty());
        assert_eq!(run(&dirty, t1 + 86_401), ["#3 finding filed: tidy dirty over a day"]);
        // a cleared condition forgets its first-seen time
        assert!(run(&BTreeMap::new(), t1 + 86_402).is_empty());
        let cache: Value = serde_json::from_str(&std::fs::read_to_string(root.join(".taskq/findings.json")).unwrap()).unwrap();
        assert!(cache.get("dirty").is_none(), "{cache}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: six-project service passes and CoreQ identity/status polls -> installed bounded runtime -> drained 15-minute intervals -> physical store bytes (A5).
    // Risk: unchanged polls write per-poll rows and push 30 days past the 384 MiB universal store cap.
    #[test]
    fn simulated_six_project_day_fits_a_thirtieth_of_the_store_cap() {
        let dir = std::env::temp_dir().join(format!("taskq-poll-day-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let size = |name: &str| { let s = coreq::telemetry::Store::open_bounded(&dir.join(name).join("telemetry.log/universal.sqlite3"),
            crate::telemetry::budget(&dir.join(name), "telemetry.sqlite3").unwrap()).unwrap();
            s.reclaim().unwrap(); s.storage().unwrap()["total_bytes"].as_u64().unwrap() };
        std::fs::create_dir_all(dir.join("empty/telemetry.log")).unwrap();
        let empty = size("empty");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::tests::simulated_poll_day_child", "--ignored", "--nocapture"])
            .env("TASKQ_POLL_DAY_ROOT", &dir).env_remove("COREQ_TRACE_CONTEXT").env("TASKQ_INCIDENT_REPORTS", "off")
            .output().unwrap();
        assert!(child.status.success(), "{}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
        // ponytail: CoreQ Reader::open validates the whole store, so a pass costs O(store); 3 of 96 intervals extrapolate within 3% of 6 at a quarter of the time (#1376).
        let day = (size("i") - empty) * 96 / INTERVALS;
        eprintln!("six-project day: {day} stored bytes from {INTERVALS} intervals, limit {}", (384u64 << 20) / 30);
        assert!(day <= (384u64 << 20) / 30, "simulated six-project day stores {day} bytes");
        std::fs::remove_dir_all(dir).unwrap();
    }

    const INTERVALS: u64 = 3;

    #[test]
    #[ignore = "subprocess helper exercised by simulated_six_project_day_fits_a_thirtieth_of_the_store_cap"]
    fn simulated_poll_day_child() {
        let dir = PathBuf::from(std::env::var_os("TASKQ_POLL_DAY_ROOT").unwrap());
        std::fs::create_dir_all(dir.join("i/telemetry.log")).unwrap();
        let _runtime = coreq::telemetry::capture::Runtime::install_bounded(&dir.join("i/telemetry.log/universal.sqlite3"), "taskq", "fixture",
            crate::telemetry::budget(&dir.join("i"), "telemetry.sqlite3").unwrap()).unwrap();
        let boards: Vec<Fake> = (0..6).map(|_| Fake::default()).collect();
        let configs: Vec<Config> = (0..6).map(|p| { let mut c = cfg("mac"); c.repo = format!("o/p{p}");
            (c.root, c.install) = (dir.join(format!("p{p}")), dir.join("i")); std::fs::create_dir_all(c.root.join(".taskq")).unwrap(); c }).collect();
        for b in &boards { b.create("Idle", "", &["taskq".into()]).unwrap(); }
        let mut services: Vec<Service> = boards.iter().zip(&configs).map(|(b, c)| { let mut s = service(b, c, Default::default()); s.focus = vec![]; s }).collect();
        let tool = crate::platform::tool(&dir.join("i"));
        // cfg polls every 60 s: a day is 96 intervals of 15 passes; each interval end drains CoreQ's poll windows as its timer does.
        for _ in 0..INTERVALS {
            for _ in 0..15 {
                for (s, c) in services.iter_mut().zip(&configs) {
                    s.pass().unwrap();
                    coreq::process::identity(std::process::id()).unwrap();
                    coreq::status::write(&tool, &c.root, &serde_json::json!({"project": c.repo, "state": "idle"})).unwrap();
                }
            }
            coreq::telemetry::capture::before_exec();
        }
    }
}
