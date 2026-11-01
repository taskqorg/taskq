//! Hourly worktree tidy and root freshness ([worktrees](../docs/design/worktrees.md)): the service owns
//! `<root>/.worktrees` and the root's Git worktree registry; foreign entries are only reported. No command here can lose
//! uncommitted work or unmerged commits: no reset, stash, checkout or clean, and `worktree remove --force` only for
//! `taskq cleanup --force` after its patch is saved, or for a worktree with submodules once it and each submodule read
//! clean (Git refuses to remove those without it).

use crate::board::Board;
use crate::core::{self, State};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Seconds between two service tidy passes of one root.
pub const EVERY: u64 = 3600;
/// An unowned worktree whose newest file and HEAD commit are this old is removed when clean.
const UNOWNED_AFTER: u64 = 7 * 86400;
const CLASSES: [&str; 4] = ["task", "reserved", "unowned", "foreign"];
/// Git lock files of the common dir that skip the pass: another Git command is running or died mid-write.
const GIT_LOCKS: [&str; 4] = ["index.lock", "HEAD.lock", "packed-refs.lock", "config.lock"];
/// Git dir state of an operation in progress; the root then stays as it is.
const OPERATIONS: [(&str, &str); 6] = [("MERGE_HEAD", "merge"), ("rebase-merge", "rebase"), ("rebase-apply", "rebase"),
    ("CHERRY_PICK_HEAD", "cherry-pick"), ("REVERT_HEAD", "revert"), ("BISECT_LOG", "bisect")];

pub struct Tidy<'a> {
    pub board: &'a dyn Board,
    pub root: &'a Path,
    pub me: &'a str,
    pub boot: fn() -> Result<i64, String>,
    /// `workspace: external` (§ 9): other worktrees are host-owned, so only `.worktrees/taskq-<N>`, `review-<N>` and
    /// branches `taskq-<N>` are tidied besides the registry prune and the root step.
    pub external: bool,
    /// `taskq cleanup --force`: a dirty task worktree is saved to `.taskq/trash` and then force-removed.
    pub force: bool,
    /// `taskq cleanup --dry-run`: every line of the pass, nothing changed (no fetch, prune, removal, release or report).
    pub dry: bool,
}

/// One pass: its log lines (removals, refusals, root moves), all lines for `taskq cleanup` (the log plus why each open
/// task's worktree stays), the `status.json` `tidy` report, telemetry counts and the conditions a finding may file.
pub struct Outcome {
    pub log: Vec<String>,
    pub lines: Vec<String>,
    pub report: Value,
    pub counts: BTreeMap<String, u64>,
    /// `dirty`, `kept`, `stale` ([findings](../docs/design/worktrees.md#report)): the lines that show each one.
    pub conditions: BTreeMap<&'static str, Vec<String>>,
}

impl Outcome {
    fn new(log: Vec<String>, report: Value, counts: BTreeMap<String, u64>) -> Outcome {
        Outcome { lines: log.clone(), log, report, counts, conditions: BTreeMap::new() }
    }
}

/// What step 2 or 3 did with one directory: removed, kept on a refusal (logged), kept as expected (cleanup only).
enum Step { Done(String), Refused(String), Note(String) }

#[derive(Clone, Copy, Debug, PartialEq)]
enum Class { Worker(u64), Review(u64), Reserved, Unowned, Foreign }

impl Class {
    fn key(self) -> &'static str {
        match self { Class::Worker(_) | Class::Review(_) => "task", Class::Reserved => "reserved", Class::Unowned => "unowned", Class::Foreign => "foreign" }
    }
}

/// The class of directory `name` under `.worktrees`: `taskq-<N>`, `review-<N>` (also the older `review-<N>-<attempt>`),
/// `dev-build`, else unowned.
fn class(name: &str) -> Class {
    let num = |s: &str| (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u64>().ok()).flatten();
    if name == "dev-build" { return Class::Reserved; }
    if let Some(n) = name.strip_prefix("taskq-").and_then(num) { return Class::Worker(n); }
    if let Some(r) = name.strip_prefix("review-") {
        let (n, rest) = r.split_once('-').map_or((r, None), |(n, a)| (n, Some(a)));
        if let Some(n) = num(n).filter(|_| rest.is_none_or(|a| num(a).is_some())) { return Class::Review(n); }
    }
    Class::Unowned
}

/// How a line names a task-class directory.
fn what(n: u64, class: Class) -> String {
    if matches!(class, Class::Review(_)) { format!("#{n} review worktree") } else { format!("#{n} worktree") }
}

/// A path as Git and the OS may spell it, for comparison only. On Windows the verbatim prefix (`\\?\`, `\\?\UNC\`),
/// backslashes, the MSYS `/c/` form and letter case read alike.
fn norm(p: &str, windows: bool) -> String {
    let mut s = p.replace('\\', "/");
    if windows {
        if let Some(r) = s.strip_prefix("//?/UNC/") { s = format!("//{r}"); } else if let Some(r) = s.strip_prefix("//?/") { s = r.into(); }
        let b = s.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' { s = format!("{}:{}", b[1] as char, &s[2..]); }
        s = s.to_lowercase();
    }
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") { s.pop(); }
    s
}

/// The comparison key of an existing path: its canonical form (macOS `/var` is `/private/var`, Windows short names).
fn key(p: &Path) -> String {
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    norm(&c.to_string_lossy(), cfg!(windows))
}

/// Where registry path `entry` lies against `root`: `None` for the root itself, `Some(Some(name))` for a directory
/// directly under `<root>/.worktrees`, `Some(None)` for anything else (foreign).
fn place(root: &str, entry: &str) -> Option<Option<String>> {
    if entry == root { return None; }
    let name = entry.strip_prefix(&format!("{root}/.worktrees/")).filter(|n| !n.is_empty() && !n.contains('/'));
    Some(name.map(String::from))
}

/// Telemetry name per Git subcommand: expected non-zero exits (`merge-base --is-ancestor`, `symbolic-ref -q`,
/// `branch -d`) stay apart from removal failures.
fn label(args: &[&str]) -> &'static str {
    match args.first().copied().unwrap_or("") {
        "worktree" => if args.get(1) == Some(&"remove") { "cleanup.git.remove" } else { "cleanup.git.worktree" },
        "status" => "cleanup.git.status",
        "branch" => "cleanup.git.branch",
        "merge-base" => "cleanup.git.merge_base",
        "merge" => "cleanup.git.merge",
        "fetch" => "cleanup.git.fetch",
        _ => "cleanup.git",
    }
}

thread_local! {
    /// The pass's root key and its `safe.directory` values; set while [`Tidy::run`] runs on this thread.
    static SAFE: std::cell::RefCell<Option<(String, Vec<String>)>> = const { std::cell::RefCell::new(None) };
    #[cfg(test)]
    static TEST_ENV: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(vec![]) };
}

/// A path as Git spells it for `safe.directory`: canonical (Git compares the real path, letter case included), with
/// `/` and without the Windows verbatim prefix.
fn spelled(p: &Path) -> String {
    let s = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().replace('\\', "/");
    match s.strip_prefix("//?/UNC/") { Some(r) => format!("//{r}"), None => s.strip_prefix("//?/").map_or(s.clone(), String::from) }
}

/// #2060: Git refuses a repository another account owns (`detected dubious ownership`, Windows roots); the pass trusts
/// only its own root and the directories under its `.worktrees`, never `*` and never a foreign entry.
struct Safe;

impl Safe {
    fn set(root: &Path) -> Safe {
        let base = spelled(root);
        let mut values = vec![base.clone(), format!("{base}/.worktrees/*")];
        values.extend(std::fs::read_dir(root.join(".worktrees")).into_iter().flatten().flatten().map(|d| spelled(&d.path())));
        SAFE.with_borrow_mut(|s| *s = Some((key(root), values)));
        Safe
    }
}

impl Drop for Safe {
    fn drop(&mut self) { SAFE.with_borrow_mut(|s| *s = None); }
}

/// The `safe.directory` values for a Git call in `dir`: the pass's, plus `dir` itself when it lies under the root's
/// `.worktrees` (a submodule); none outside a pass.
fn safe(dir: &Path) -> Vec<String> {
    SAFE.with_borrow(|s| s.as_ref().map(|(root, values)| {
        let mut v = values.clone();
        if key(dir).starts_with(&format!("{root}/.worktrees/")) { v.push(spelled(dir)); }
        v
    })).unwrap_or_default()
}

/// `core.longpaths`: Git for Windows otherwise fails a removal at 260 characters (`Filename too long`, deep `target/`
/// paths), after dropping the registry entry and `.git` file, so the rest of the directory stays; other Gits ignore it.
fn run(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut c = crate::platform::command("git");
    c.arg("-C").arg(dir).args(["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.longpaths=true", "--no-optional-locks"]);
    for v in safe(dir) { c.arg("-c").arg(format!("safe.directory={v}")); }
    #[cfg(test)]
    TEST_ENV.with_borrow(|e| { c.envs(e.iter().map(|(k, v)| (k, v))); });
    c.args(args);
    match coreq::process::output(label(args), &mut c, std::time::Duration::from_secs(120)) {
        Ok(o) => Ok(o.stdout),
        Err(coreq::process::Failure::Exit(o)) => Err(String::from_utf8_lossy(&o.stderr).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("git failed").to_string()),
        Err(e) => Err(format!("git {}: {e}", args.first().unwrap_or(&""))),
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(dir, args).map(|o| String::from_utf8_lossy(&o).replace("\r\n", "\n"))
}

#[derive(Clone, Debug, Default)]
struct Entry { path: String, branch: Option<String>, locked: bool, prunable: bool }

fn registry(root: &Path) -> Result<Vec<Entry>, String> {
    Ok(git(root, &["worktree", "list", "--porcelain"])?.split("\n\n").filter_map(|block| {
        let mut e = Entry::default();
        for l in block.lines() {
            if let Some(p) = l.strip_prefix("worktree ") { e.path = p.into(); }
            if let Some(b) = l.strip_prefix("branch refs/heads/") { e.branch = Some(b.into()); }
            if l == "locked" || l.starts_with("locked ") { e.locked = true; }
            if l == "prunable" || l.starts_with("prunable ") { e.prunable = true; }
        }
        (!e.path.is_empty()).then_some(e)
    }).collect())
}

/// Uncommitted changes in `dir` or any submodule under it, as Git's own removal check reads them; unknown reads changed.
fn changed(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--ignore-submodules=none"]).map_or(true, |s| !s.trim().is_empty())
}

/// Checked-out submodules under `dir`, nested ones too, as `/` paths relative to `dir`.
fn submodules(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = vec![];
    for line in git(dir, &["ls-files", "-z", "--stage"])?.split('\0') {
        let Some((meta, path)) = line.split_once('\t') else { continue };
        if !meta.starts_with("160000 ") || !dir.join(path).join(".git").exists() { continue; }
        out.push(path.to_string());
        out.extend(submodules(&dir.join(path))?.into_iter().map(|s| format!("{path}/{s}")));
    }
    Ok(out)
}

/// Total file bytes and newest modification (Unix seconds) under `dir`, else of `dir` itself; symlinks are not followed.
/// `ponytail:` a full walk each hour (a 32 GB Windows `.worktrees` of 55k files took 3 s); a size cache is the upgrade.
fn walk(dir: &Path) -> (u64, u64) {
    let secs = |m: &std::fs::Metadata| m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let (mut bytes, mut newest) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(m) = e.metadata() else { continue };
            newest = newest.max(secs(&m));
            if m.is_dir() { stack.push(e.path()); } else { bytes += m.len(); }
        }
    }
    // an empty leftover directory has no entry to age it
    if newest == 0 { newest = std::fs::metadata(dir).map_or(0, |m| secs(&m)); }
    (bytes, newest)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

struct Item { name: String, path: PathBuf, class: Class, entry: Option<Entry>, dirty: bool, bytes: u64, newest: u64, removed: bool }

impl Item {
    fn new(name: String, path: PathBuf, class: Class, entry: Option<Entry>) -> Item {
        // a status that cannot run reads dirty: nothing is removed on an unknown
        let dirty = entry.is_some() && changed(&path);
        let (bytes, newest) = walk(&path);
        Item { name, path, class, entry, dirty, bytes, newest, removed: false }
    }
}

impl Tidy<'_> {
    /// One tidy pass of the root. `live(n)`: a child of this service still runs task N. `known(n)`: the state of an
    /// open task the caller just read; otherwise a fresh board read decides (R12).
    pub fn run(&self, live: &mut dyn FnMut(u64) -> bool, known: &dyn Fn(u64) -> Option<State>) -> Outcome {
        let at = now();
        if !self.root.join(".git").exists() {
            return Outcome::new(vec![], Value::Null, BTreeMap::new());
        }
        let _safe = Safe::set(self.root);
        let dot = crate::dot(self.root);
        if !self.dry && let Err(e) = std::fs::create_dir_all(&dot) { return self.skip(at, format!(".taskq unwritable: {e}")); }
        // a dry run creates no `.taskq`; one that exists is locked as for a real pass
        let _lock = if self.dry && !dot.exists() { None } else {
            match coreq::file_lock::try_lock(&dot.join("tidy.lock")) {
                Ok(l) => Some(l),
                Err(std::fs::TryLockError::WouldBlock) => return self.skip(at, "taskq lock held".into()),
                Err(std::fs::TryLockError::Error(e)) => return self.skip(at, format!("taskq lock unreadable: {e}")),
            }
        };
        let dirs = match git(self.root, &["rev-parse", "--git-dir", "--git-common-dir"]) {
            Ok(t) => t.lines().map(|l| self.root.join(l.trim())).collect::<Vec<_>>(),
            Err(e) => return self.skip(at, format!("git unreadable: {e}")),
        };
        let (gitdir, common) = (dirs.first().cloned().unwrap_or_default(), dirs.get(1).cloned().unwrap_or_default());
        if let Some(l) = GIT_LOCKS.iter().find(|l| common.join(l).exists() || gitdir.join(l).exists()) {
            return self.skip(at, format!("Git lock {l} held"));
        }
        let mut log = vec![];
        // indices of `log` lines only `taskq cleanup` prints
        let mut notes = BTreeSet::new();
        let mut counts = BTreeMap::new();
        let mut conditions: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        // a dry run reads the last fetched `origin/main`
        let fetched = if self.dry { Ok(String::new()) } else { git(self.root, &["fetch", "-q", "origin", "main"]) };
        // 1. registry prune: entries whose directory is gone; Git keeps locked ones
        let before = registry(self.root).map_or(0, |r| r.len());
        let pruned = if self.dry {
            let n = registry(self.root).map_or(0, |r| r.iter().filter(|e| e.prunable).count());
            if n > 0 { log.push(format!("tidy would prune {n} worktree registry entries without a directory")); }
            n
        } else {
            let after = git(self.root, &["worktree", "prune"]).and_then(|_| registry(self.root)).map_or(before, |r| r.len());
            if after < before { log.push(format!("tidy pruned {} worktree registry entries without a directory", before - after)); }
            before - after.min(before)
        };
        counts.insert("pruned".into(), pruned as u64);
        let mut items = self.items();
        let mut stages = HashMap::new();
        let (mut removed, mut refused, mut branches) = (0, 0, 0);
        // 2. task class, 3. unowned class (not in an external workspace)
        for it in items.iter_mut() {
            let task = match it.class { Class::Worker(n) | Class::Review(n) => Some(n), _ => None };
            let alive = task.is_some_and(|n| live(n));
            let outcome = match it.class {
                Class::Worker(n) | Class::Review(n) if alive => Some(Step::Note(format!("{} kept: running child on this host", what(n, it.class)))),
                Class::Worker(n) | Class::Review(n) => Some(self.task(n, it, known, &mut stages, &mut log)),
                Class::Unowned if !self.external && it.entry.is_some() => self.unowned(it, at),
                Class::Unowned | Class::Reserved | Class::Foreign => None,
            };
            let refusal = match outcome {
                Some(Step::Done(line)) => { it.removed = !self.dry; removed += 1; log.push(line); None }
                Some(Step::Refused(line)) => { refused += 1; log.push(line.clone()); Some(line) }
                Some(Step::Note(line)) => { notes.insert(log.len()); log.push(line); None }
                None => None,
            };
            // findings: a task worktree dirty while no child runs it; a closed task's worktree that stays
            if let Some(n) = task.filter(|_| !it.removed && !alive) {
                if it.dirty { conditions.entry("dirty").or_default().push(format!("{} dirty, untouched {}", what(n, it.class), crate::age(at.saturating_sub(it.newest) as i64))); }
                if stages.get(&n) == Some(&Some(State::Closed)) && let Some(line) = refusal { conditions.entry("kept").or_default().push(line); }
            }
        }
        // 4. local `taskq-<N>` branches without a worktree
        let (deleted, kept) = self.branches(live, known, &mut stages, &mut log);
        branches += deleted;
        refused += kept;
        // 5. root
        let (root, behind, moved) = self.root_step(&fetched, &gitdir, &mut log);
        if root.starts_with("stale") { conditions.entry("stale").or_default().push(format!("root {root}")); }
        let mut report = json!({"at": at, "outcome": "ok", "root": root});
        for c in CLASSES {
            let kept: Vec<&Item> = items.iter().filter(|it| !it.removed && it.class.key() == c).collect();
            let oldest = kept.iter().map(|it| at.saturating_sub(it.newest)).max().unwrap_or(0);
            let v = json!({"count": kept.len(), "dirty": kept.iter().filter(|it| it.dirty).count(), "bytes": kept.iter().map(|it| it.bytes).sum::<u64>(), "oldest_s": oldest});
            for k in ["count", "dirty", "bytes", "oldest_s"] {
                counts.insert(if k == "count" { c.to_string() } else { format!("{c}_{k}") }, v[k].as_u64().unwrap_or(0));
            }
            report[c] = v;
        }
        items.clear();
        counts.extend([("removed".into(), removed), ("refused".into(), refused), ("branches_deleted".into(), branches),
            ("root_behind".into(), behind), ("root_moved".into(), moved as u64), ("skipped".into(), 0)]);
        self.save(&report);
        let lines = log.clone();
        let log = log.into_iter().enumerate().filter(|(i, _)| !notes.contains(i)).map(|(_, l)| l).collect();
        Outcome { log, lines, report, counts, conditions }
    }

    /// A skipped pass keeps the last report and says why.
    fn skip(&self, at: u64, reason: String) -> Outcome {
        let path = crate::dot(self.root).join("tidy.json");
        let mut report = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
        report["at"] = json!(at);
        report["outcome"] = json!(format!("skipped {reason}"));
        self.save(&report);
        Outcome::new(vec![format!("tidy skipped: {reason}")], report, BTreeMap::from([("skipped".into(), 1)]))
    }

    fn save(&self, report: &Value) {
        // a cache of the last outcome for `status.json` (R1); a failed write only loses the report; a dry run writes none
        if !self.dry { let _ = std::fs::write(crate::dot(self.root).join("tidy.json"), report.to_string()); }
    }

    /// Directories under `.worktrees` and foreign registry entries, each with its class.
    fn items(&self) -> Vec<Item> {
        let root = key(self.root);
        let mut items = vec![];
        let mut named = BTreeMap::new();
        for e in registry(self.root).unwrap_or_default() {
            match place(&root, &key(Path::new(&e.path))) {
                None => {}
                Some(Some(name)) => { named.insert(name, e); }
                Some(None) => items.push(Item::new(e.path.clone(), PathBuf::from(&e.path), Class::Foreign, Some(e))),
            }
        }
        let dirs: BTreeSet<String> = std::fs::read_dir(self.root.join(".worktrees")).into_iter().flatten().flatten()
            .filter(|d| d.file_type().is_ok_and(|t| t.is_dir())).map(|d| d.file_name().to_string_lossy().into_owned()).collect();
        // a registered name differing only in case (Windows) matches its directory
        for name in dirs {
            let entry = named.keys().find(|k| norm(k, cfg!(windows)) == norm(&name, cfg!(windows))).cloned().and_then(|k| named.remove(&k));
            items.push(Item::new(name.clone(), self.root.join(".worktrees").join(&name), class(&name), entry));
        }
        items
    }

    /// The task's state: `known` from the caller, else a fresh read (`None`: unknown, nothing is removed).
    fn stage(&self, n: u64, known: &dyn Fn(u64) -> Option<State>, stages: &mut HashMap<u64, Option<State>>) -> Option<State> {
        *stages.entry(n).or_insert_with(|| known(n).or_else(|| match self.board.issue(n) {
            Err(_) => None,
            Ok(i) if !i.open => Some(State::Closed),
            Ok(_) => crate::board::entries(self.board, n).ok().map(|e| core::project(n, &e, false).task.state),
        }))
    }

    /// Step 2: `taskq-<N>` leaves with a closed task, `review-<N>` once the task is no longer in `review`; every other
    /// outcome names why the directory stays.
    fn task(&self, n: u64, it: &Item, known: &dyn Fn(u64) -> Option<State>, stages: &mut HashMap<u64, Option<State>>, log: &mut Vec<String>) -> Step {
        let review = matches!(it.class, Class::Review(_));
        let what = what(n, it.class);
        let Some(state) = self.stage(n, known, stages) else { return Step::Note(format!("{what} kept: task state unknown")) };
        let closed = state == State::Closed;
        if !(closed || review && state != State::Review) {
            return Step::Note(format!("{what} kept: {}", if review { "task in review".into() } else { format!("task open ({})", state.name()) }));
        }
        // #2060: a task directory outside the registry is no worktree; Git reads the root's status in it
        if it.entry.is_none() {
            const WHY: &str = "empty directory, not a registered worktree";
            if std::fs::read_dir(&it.path).map_or(true, |mut d| d.next().is_some()) {
                return Step::Refused(format!("{what} kept: not a registered worktree; inspect it and delete it by hand"));
            }
            if self.dry { return Step::Done(format!("{what} would be removed: {WHY}")); }
            return match std::fs::remove_dir(&it.path) {
                Ok(()) => Step::Done(format!("{what} removed: {WHY}")),
                Err(e) => Step::Refused(format!("{what} kept: {WHY}, removal failed: {e}")),
            };
        }
        if it.entry.as_ref().is_some_and(|e| e.locked) { return Step::Refused(format!("{what} kept: locked (git worktree unlock it first)")); }
        let subs = match submodules(&it.path) {
            Ok(s) => s,
            Err(e) => return Step::Refused(format!("{what} kept: submodules unreadable: {e}")),
        };
        if it.dirty && !self.force {
            let at = subs.iter().find(|s| changed(&it.path.join(s))).map_or(String::new(), |s| format!(" in submodule {s}"));
            return Step::Refused(format!("{what} kept: uncommitted changes{at}; inspect them before `taskq cleanup --force`"));
        }
        // #1875: a submodule's commits live only in this worktree's Git dir, so removal would lose those on no remote
        if let Some(s) = subs.iter().find(|s| git(&it.path.join(s), &["rev-list", "-n", "1", "HEAD", "--branches", "--not", "--remotes"]).map_or(true, |o| !o.trim().is_empty())) {
            return Step::Refused(format!("{what} kept: submodule {s} has commits on no remote; push them first"));
        }
        if let Err(e) = self.held(n, closed, log) { return Step::Refused(format!("{what} kept: {e}")); }
        if self.dry {
            return Step::Done(format!("{what} would be removed{}", if it.dirty { " after saving a patch to .taskq/trash" } else { "" }));
        }
        let rel = format!(".worktrees/{}", it.name);
        let force = || git(self.root, &["worktree", "remove", "--force", &rel]).map(drop);
        // the refusal line carries Git's first stderr line; the service logs it once while it stays the same
        let removed = if self.force && it.dirty {
            let trash = crate::dot(self.root).join("trash");
            let save = |dir: &Path, file: String| run(dir, &["add", "-A"]).and_then(|_| run(dir, &["diff", "--cached", "--binary"])).and_then(|patch| {
                if patch.is_empty() { return Ok(()); }
                std::fs::create_dir_all(&trash).and_then(|_| std::fs::write(trash.join(file), &patch)).map_err(|e| format!("patch save failed: {e}"))
            });
            subs.iter().try_for_each(|s| save(&it.path.join(s), format!("{}--{}.patch", it.name, s.replace('/', "-"))))
                .and_then(|_| save(&it.path, format!("{}.patch", it.name))).and_then(|_| force())
        } else {
            // #1875: Git refuses any worktree with submodules ("working trees containing submodules cannot be moved or
            // removed"); a fresh clean read of it and its submodules stands in for Git's own check
            git(self.root, &["worktree", "remove", &rel]).map(drop).or_else(|e| match e.contains("containing submodules") {
                true if changed(&it.path) => Err(format!("{} contains modified or untracked files", rel)),
                true => force(),
                false => Err(e),
            })
        };
        match removed {
            Ok(()) => Step::Done(format!("{what} removed")),
            Err(e) => Step::Refused(format!("{what} removal unconfirmed: {e}; {}", if self.force { "inspect the worktree" } else { "inspect uncommitted work before `taskq cleanup --force`" })),
        }
    }

    /// #1549: only this host's executions (its name or OS hostname) hold a worktree. Of a closed task, a boot after
    /// intent releases them (#1386); of an open task any unreleased one holds it.
    fn held(&self, n: u64, closed: bool, log: &mut Vec<String>) -> Result<(), String> {
        let comments = self.board.comments(n).map_err(|e| format!("board read failed: {e}"))?;
        let entries: Vec<_> = comments.iter().filter(|c| c.trusted).map(|c| core::Entry { id: c.id, body: c.body.clone(), edited: c.edited }).collect();
        let mut held = None;
        for x in core::executions(n, &entries).into_iter().filter(|x| x.release.is_none() && (x.host == self.me || x.host == crate::hostname())) {
            if !closed { return Err(format!("execution {} on {} not released", x.id, x.host)); }
            let intent = comments.iter().find(|c| c.id == x.id).map(|c| c.created).filter(|t| *t > 0);
            let ended = intent.zip((self.boot)().ok()).map(|(intent, at)| crate::snapshot::verdict(&crate::snapshot::Signals { local: true, boot: Some((at, intent)), ..Default::default() }));
            let released = match ended {
                Some(("ended", proof)) if self.dry => { log.push(format!("#{n} would release {} exit: {proof}", x.id)); Ok(()) }
                Some(("ended", proof)) => crate::service::release_execution_text(self.board, n, x.attempt, x.id, "exit", &proof)
                    .map(|_| log.push(format!("#{n} released {} exit: {proof}", x.id))),
                _ => Err(format!("execution {} on {} termination unknown; stop its process or reboot this host", x.id, x.host)),
            };
            if let Err(e) = released { held.get_or_insert(e); }
        }
        held.map_or(Ok(()), Err)
    }

    /// Step 3: a clean unowned worktree untouched for 7 days; its branch stays.
    fn unowned(&self, it: &Item, at: u64) -> Option<Step> {
        let commit = git(&it.path, &["log", "-1", "--format=%ct", "HEAD"]).ok().and_then(|t| t.trim().parse::<u64>().ok())?;
        let touched = it.newest.max(commit);
        if it.dirty || it.entry.as_ref().is_some_and(|e| e.locked) || at.saturating_sub(touched) < UNOWNED_AFTER { return None; }
        let rel = format!(".worktrees/{}", it.name);
        let age = crate::age(at.saturating_sub(touched) as i64);
        if self.dry { return Some(Step::Done(format!("{rel} would be removed: unowned, untouched {age}"))); }
        Some(match git(self.root, &["worktree", "remove", &rel]) {
            Ok(_) => Step::Done(format!("{rel} removed: unowned, untouched {age}")),
            Err(e) => Step::Refused(format!("{rel} kept: {e}")),
        })
    }

    /// Step 4: delete local `taskq-<N>` branches without a worktree of closed tasks. `-D` only when the tip is in
    /// `origin/main` or the close receipt records a merge of the task's result at this tip; else `-d`, whose refusal
    /// keeps the branch. Returns (deleted, kept).
    fn branches(&self, live: &mut dyn FnMut(u64) -> bool, known: &dyn Fn(u64) -> Option<State>, stages: &mut HashMap<u64, Option<State>>, log: &mut Vec<String>) -> (u64, u64) {
        let Ok(refs) = git(self.root, &["for-each-ref", "--format=%(refname:short) %(objectname)", "refs/heads/taskq-*"]) else { return (0, 0) };
        let checked: BTreeSet<String> = registry(self.root).unwrap_or_default().into_iter().filter_map(|e| e.branch).collect();
        let (mut deleted, mut kept) = (0, 0);
        for (name, tip) in refs.lines().filter_map(|l| l.split_once(' ')) {
            let Class::Worker(n) = class(name) else { continue };
            if checked.contains(name) || live(n) || self.stage(n, known, stages) != Some(State::Closed) { continue; }
            let merged = git(self.root, &["merge-base", "--is-ancestor", tip, "origin/main"]).is_ok() || self.receipt(n, tip);
            // a dry run reads what `branch -d` checks: the tip merged into HEAD
            if self.dry {
                if merged || git(self.root, &["merge-base", "--is-ancestor", tip, "HEAD"]).is_ok() { deleted += 1; log.push(format!("#{n} branch would be deleted")); }
                else { kept += 1; log.push(format!("#{n} branch kept: not merged into origin/main")); }
                continue;
            }
            match git(self.root, &["branch", if merged { "-D" } else { "-d" }, name]) {
                Ok(_) => { deleted += 1; log.push(format!("#{n} branch deleted")); }
                Err(e) => { kept += 1; log.push(format!("#{n} branch kept: {}{e}", if merged { "" } else { "not merged into origin/main: " })); }
            }
        }
        (deleted, kept)
    }

    /// A close receipt (`merged #P as SHA`, GitLab `merged !P`) of a task whose result is this tip: a squash merge.
    fn receipt(&self, n: u64, tip: &str) -> bool {
        let Ok(entries) = crate::board::entries(self.board, n) else { return false };
        let merged = entries.iter().any(|e| e.body.lines().next().is_some_and(|h| h.trim().starts_with("taskq: close"))
            && (e.body.contains("merged #") || e.body.contains("merged !")));
        merged && core::project(n, &entries, false).task.result.is_some_and(|r| r.len() >= 7 && tip.starts_with(&r))
    }

    /// Step 5: fast-forward the root only on `main`; anything else is `stale <reason>`. Returns (report, behind, moved).
    fn root_step(&self, fetched: &Result<String, String>, gitdir: &Path, log: &mut Vec<String>) -> (String, u64, bool) {
        let counts = git(self.root, &["rev-list", "--left-right", "--count", "HEAD...origin/main"]).ok();
        let mut n = counts.as_deref().unwrap_or("").split_whitespace().map(|x| x.parse::<u64>().unwrap_or(0));
        let (ahead, behind) = (n.next().unwrap_or(0), n.next().unwrap_or(0));
        let branch = git(self.root, &["symbolic-ref", "-q", "HEAD"]).ok().map(|h| h.trim().trim_start_matches("refs/heads/").to_string());
        let reason = match branch.as_deref() {
            None => Some("detached HEAD".to_string()),
            Some(b) if b != "main" => Some(format!("on branch {b}")),
            _ => OPERATIONS.iter().find(|(f, _)| gitdir.join(f).exists()).map(|(_, op)| format!("{op} in progress"))
                .or_else(|| fetched.as_ref().err().map(|_| "fetch failed".to_string()))
                .or_else(|| counts.is_none().then(|| "origin/main unknown".to_string()))
                .or_else(|| (ahead > 0 && behind > 0).then(|| "diverged".to_string())),
        };
        let reason = match reason {
            Some(r) => r,
            None if behind == 0 => return ("current".into(), 0, false),
            None if self.dry => {
                log.push(format!("root would fast-forward {behind} commits to origin/main"));
                return (format!("behind {behind}"), behind, false);
            }
            None => match git(self.root, &["-c", "merge.autoStash=false", "merge", "--ff-only", "-q", "origin/main"]) {
                Ok(_) => {
                    log.push(format!("root fast-forwarded {behind} commits to origin/main"));
                    return ("current".into(), 0, true);
                }
                Err(e) if e.contains("would be overwritten") => "overwrite conflict".into(),
                Err(_) => "fast-forward refused".into(),
            },
        };
        log.push(format!("root stale: {reason}"));
        (if behind > 0 { format!("stale {reason} (behind {behind})") } else { format!("stale {reason}") }, behind, false)
    }
}

/// `taskq profile health` (docs/design/worktrees.md): `<project>: root ..; .worktrees K (dirty D, S); foreign F (dirty G); tidy <age> <outcome>`.
pub fn health_line(project: &str, tidy: &Value, now: u64) -> String {
    let Some(at) = tidy["at"].as_u64() else { return format!("{project}: tidy unknown (no report yet)") };
    let n = |c: &str, k: &str| tidy[c][k].as_u64().unwrap_or(0);
    let own = ["task", "reserved", "unowned"];
    let (k, d, s) = own.iter().fold((0, 0, 0), |(k, d, s), c| (k + n(c, "count"), d + n(c, "dirty"), s + n(c, "bytes")));
    format!("{project}: root {}; .worktrees {k} (dirty {d}, {}); foreign {} (dirty {}); tidy {} {}", tidy["root"].as_str().unwrap_or("unknown"),
        size(s), n("foreign", "count"), n("foreign", "dirty"), crate::age(now.saturating_sub(at) as i64), tidy["outcome"].as_str().unwrap_or("unknown"))
}

fn size(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GiB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{} MiB", b >> 20),
        b => format!("{} KiB", b.div_ceil(1024)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Fake;

    fn no_boot() -> Result<i64, String> { Err("boot time not read in tests".into()) }

    /// Origin with one commit on `main` and a clone `root`; `sh` runs git in a directory and asserts success.
    struct Repo { base: PathBuf, origin: PathBuf, root: PathBuf }
    impl Repo {
        fn new(tag: &str) -> Repo {
            let base = std::env::temp_dir().join(format!("taskq-tidy-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            let (origin, root) = (base.join("origin"), base.join("root"));
            std::fs::create_dir_all(&origin).unwrap();
            let r = Repo { base, origin, root };
            r.sh(&r.origin, &["init", "-q", "-b", "main"]);
            std::fs::write(r.origin.join("a"), "1").unwrap();
            r.sh(&r.origin, &["add", "a"]);
            r.sh(&r.origin, &["commit", "-q", "-m", "init"]);
            r.sh(&r.base, &["clone", "-q", r.origin.to_str().unwrap(), "root"]);
            r
        }
        fn sh(&self, dir: &Path, args: &[&str]) -> String {
            let o = std::process::Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
            assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        /// A new commit on origin `main` touching file `f`.
        fn upstream(&self, f: &str, text: &str) {
            std::fs::write(self.origin.join(f), text).unwrap();
            self.sh(&self.origin, &["add", f]);
            self.sh(&self.origin, &["commit", "-q", "-m", f]);
        }
        fn head(&self) -> String { self.sh(&self.root, &["rev-parse", "HEAD"]) }
        fn tidy<'a>(&'a self, b: &'a Fake, external: bool) -> Tidy<'a> {
            Tidy { board: b, root: &self.root, me: "mac", boot: no_boot, external, force: false, dry: false }
        }
        fn wt(&self, name: &str) -> bool { self.root.join(".worktrees").join(name).exists() }
        /// Everything a pass may change: registry, branches, HEAD, `origin/main`, each status and `.taskq`.
        fn state(&self) -> String {
            let mut s = [self.sh(&self.root, &["worktree", "list", "--porcelain"]), self.sh(&self.root, &["branch", "--list"]),
                self.head(), self.sh(&self.root, &["status", "--porcelain"]), format!("taskq {}", self.root.join(".taskq").exists())].join("\n");
            for d in std::fs::read_dir(self.root.join(".worktrees")).into_iter().flatten().flatten() {
                s += &format!("\n{}: {:?}", d.file_name().to_string_lossy(), std::fs::read_dir(d.path()).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>());
            }
            s
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.base); }
    }

    fn pass(t: &Tidy) -> Outcome { t.run(&mut |_| false, &|_| None) }

    // Chain: hourly tidy -> root step 5. Defect: root checkouts 99 and 322 commits behind origin/main (Mac survey).
    // Risk protected: a fast-forward that resets, stashes or overwrites local work, or moves a root off `main`.
    #[test]
    fn root_fast_forwards_only_on_main_and_never_overwrites() {
        let r = Repo::new("root");
        let b = Fake::default();
        r.upstream("b", "1");
        std::fs::write(r.root.join("local.txt"), "mine").unwrap();
        std::fs::write(r.root.join("a"), "edited").unwrap();
        let o = pass(&r.tidy(&b, false));
        assert_eq!(o.report["root"], "current", "{:?}", o.log);
        assert!(o.log.contains(&"root fast-forwarded 1 commits to origin/main".to_string()), "{:?}", o.log);
        assert_eq!(std::fs::read_to_string(r.root.join("local.txt")).unwrap(), "mine", "untracked file untouched");
        assert_eq!(std::fs::read_to_string(r.root.join("a")).unwrap(), "edited", "unrelated modified file untouched");
        assert!(pass(&r.tidy(&b, false)).log.is_empty(), "a pass that changes nothing logs nothing");

        // overwrite conflict: the merge would overwrite a modified tracked file
        r.upstream("a", "2");
        let before = r.head();
        let o = pass(&r.tidy(&b, false));
        assert_eq!((o.report["root"].as_str().unwrap(), r.head()), ("stale overwrite conflict (behind 1)", before.clone()));
        assert_eq!(std::fs::read_to_string(r.root.join("a")).unwrap(), "edited", "local change kept");
        assert_eq!(r.sh(&r.root, &["stash", "list"]), "", "never stashes");
        r.sh(&r.root, &["checkout", "-q", "--", "a"]);

        // diverged: a local commit on main
        std::fs::write(r.root.join("c"), "local").unwrap();
        r.sh(&r.root, &["add", "c"]);
        r.sh(&r.root, &["commit", "-q", "-m", "local"]);
        let local = r.head();
        let o = pass(&r.tidy(&b, false));
        assert_eq!((o.report["root"].as_str().unwrap(), r.head()), ("stale diverged (behind 1)", local.clone()), "never resets");

        // another branch, then a detached HEAD
        r.sh(&r.root, &["checkout", "-q", "-b", "topic"]);
        assert_eq!(pass(&r.tidy(&b, false)).report["root"], "stale on branch topic (behind 1)");
        r.sh(&r.root, &["checkout", "-q", "--detach"]);
        assert_eq!(pass(&r.tidy(&b, false)).report["root"], "stale detached HEAD (behind 1)");
        assert_eq!(r.head(), local);

        // an operation in progress on main
        r.sh(&r.root, &["checkout", "-q", "main"]);
        std::fs::write(r.root.join(".git/MERGE_HEAD"), format!("{local}\n")).unwrap();
        assert_eq!(pass(&r.tidy(&b, false)).report["root"], "stale merge in progress (behind 1)");
        std::fs::remove_file(r.root.join(".git/MERGE_HEAD")).unwrap();

        // a held Git lock skips the whole pass and says so; the report keeps the last root
        std::fs::write(r.root.join(".git/index.lock"), "").unwrap();
        let o = pass(&r.tidy(&b, false));
        assert_eq!((o.report["outcome"].as_str().unwrap(), o.report["root"].as_str().unwrap()), ("skipped Git lock index.lock held", "stale merge in progress (behind 1)"));
        assert_eq!(o.log, ["tidy skipped: Git lock index.lock held"]);
        std::fs::remove_file(r.root.join(".git/index.lock")).unwrap();

        // a held `.taskq` lock skips too
        let held = coreq::file_lock::try_lock(&r.root.join(".taskq/tidy.lock")).unwrap();
        assert_eq!(pass(&r.tidy(&b, false)).report["outcome"], "skipped taskq lock held");
        drop(held);
        assert_eq!(r.head(), local, "no refusal moved the root");
    }

    // Chain: tidy steps 2 and 4 on a temp repo. Defect: reviewer worktrees and `taskq-*` branches of closed tasks
    // that no code removed (Mac survey). Risk protected: removing a dirty, open-stage or foreign worktree, `dev-build`,
    // or deleting an unmerged branch.
    #[test]
    fn task_worktrees_and_branches_leave_with_their_stage_only() {
        let r = Repo::new("task");
        let b = Fake::default();
        for _ in 0..6 { b.create("T", "", &["taskq".into()]).unwrap(); }
        let add = |args: &[&str]| { r.sh(&r.root, &[&["worktree", "add", "-q"], args].concat()); };
        add(&["-b", "taskq-1", ".worktrees/taskq-1"]);
        add(&["-b", "taskq-2", ".worktrees/taskq-2"]);
        add(&["--detach", ".worktrees/review-3"]);
        add(&["--detach", ".worktrees/review-4-6177396825"]);
        add(&["--detach", ".worktrees/dev-build"]);
        let foreign = r.base.join("foreign");
        r.sh(&r.root, &["worktree", "add", "-q", "--detach", foreign.to_str().unwrap()]);
        std::fs::write(r.root.join(".worktrees/taskq-2/wip"), "x").unwrap();
        std::fs::write(foreign.join("wip"), "x").unwrap();
        // closed tasks' branches: 5 merged into origin/main, 6 holds an unmerged commit
        r.sh(&r.root, &["branch", "taskq-5", "origin/main"]);
        r.sh(&r.root, &["checkout", "-q", "-b", "taskq-6"]);
        std::fs::write(r.root.join("u"), "unmerged").unwrap();
        r.sh(&r.root, &["add", "u"]);
        r.sh(&r.root, &["commit", "-q", "-m", "unmerged"]);
        r.sh(&r.root, &["checkout", "-q", "main"]);
        for n in [1, 2, 3, 5, 6] { b.close(n).unwrap(); }
        // #4 is open and still in review: its reviewer worktree stays
        let known = |n: u64| (n == 4).then_some(State::Review);
        let o = r.tidy(&b, false).run(&mut |_| false, &known);
        assert_eq!((r.wt("taskq-1"), r.wt("taskq-2"), r.wt("review-3"), r.wt("review-4-6177396825"), r.wt("dev-build"), foreign.exists()),
            (false, true, false, true, true, true), "{:?}", o.log);
        assert!(o.log.contains(&"#2 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`".into()), "{:?}", o.log);
        assert!(o.log.contains(&"#1 worktree removed".into()) && o.log.contains(&"#3 review worktree removed".into()), "{:?}", o.log);
        let branch = |b: &str| r.sh(&r.root, &["branch", "--list", b]) != "";
        assert!(!branch("taskq-1") && !branch("taskq-5"), "merged branches without a worktree go: {:?}", o.log);
        assert!(branch("taskq-6") && o.log.contains(&"#6 branch kept: not merged into origin/main: error: the branch 'taskq-6' is not fully merged".into()), "unmerged stays: {:?}", o.log);
        assert!(branch("taskq-2"), "a branch with a worktree stays");
        assert_eq!((o.report["task"]["count"].as_u64(), o.report["task"]["dirty"].as_u64()), (Some(2), Some(1)));
        assert_eq!((o.report["reserved"]["count"].as_u64(), o.report["foreign"]["count"].as_u64(), o.report["foreign"]["dirty"].as_u64()), (Some(1), Some(1), Some(1)));
        assert_eq!((o.counts["removed"], o.counts["refused"], o.counts["branches_deleted"]), (2, 2, 2));

        // #4 leaves review: its reviewer worktree goes; a live child keeps a worktree whatever the board says
        let o = r.tidy(&b, false).run(&mut |n| n == 4, &|n| (n == 4).then_some(State::Doing));
        assert!(r.wt("review-4-6177396825"), "a live child keeps it: {:?}", o.log);
        r.tidy(&b, false).run(&mut |_| false, &|n| (n == 4).then_some(State::Doing));
        assert!(!r.wt("review-4-6177396825"));

        // #2060 (Mac cs, 55 closed-task worktrees): `workspace: external` still tidies TaskQ's own task worktrees and
        // `taskq-<N>` branches; dev-build and foreign entries stay, the unmerged branch too
        std::fs::remove_file(r.root.join(".worktrees/taskq-2/wip")).unwrap();
        let o = pass(&r.tidy(&b, true));
        assert!(!r.wt("taskq-2") && r.wt("dev-build") && foreign.exists() && branch("taskq-6"), "{:?}", o.log);
        assert!(o.log.contains(&"#2 worktree removed".into()), "{:?}", o.log);
        // a squash merge: the close receipt records the merge of the task's result at the branch tip
        let tip = r.sh(&r.root, &["rev-parse", "taskq-6"]);
        let attempt = b.comment(6, "taskq: spawn worker claude:owned other").unwrap();
        b.comment(6, &core::entry("result", &[&tip], Some(attempt), "")).unwrap();
        b.comment(6, &core::entry("close", &[], Some(attempt), "ok\ndelivery auto-merge r0: merged #9 as abcdef1; merge-result CI green")).unwrap();
        let o = pass(&r.tidy(&b, false));
        assert!(!branch("taskq-6") && !r.wt("taskq-2"), "{:?}", o.log);

        // Defect (live Windows pass): an empty leftover directory reported its age since 1970. It stays, aged by its own time.
        std::fs::create_dir_all(r.root.join(".worktrees/taskq-8")).unwrap();
        let o = pass(&r.tidy(&b, false));
        assert!(r.wt("taskq-8") && o.report["task"]["count"] == 1 && o.report["task"]["oldest_s"].as_u64() < Some(86400), "{}", o.report);
    }

    // Chain: tidy step 2 removal on Windows. Defect (PM steer, win 4.6.35): clean closed-task worktrees stayed and
    // `cleanup.git` failed 163 of 308 calls with one log line and no cause. Git for Windows fails `worktree remove` at
    // 260 characters (`Filename too long`) on deep ignored `target/` paths and drops the registry entry. Risk protected:
    // a refusal line without Git's reason, and a long ignored path that keeps a closed task's worktree.
    #[test]
    fn removal_handles_long_ignored_paths_and_names_git_refusals() {
        let r = Repo::new("long");
        let b = Fake::default();
        for _ in 0..2 { b.create("T", "", &["taskq".into()]).unwrap(); }
        std::fs::write(r.root.join(".git/info/exclude"), "target/
").unwrap();
        for n in [1, 2] { r.sh(&r.root, &["worktree", "add", "-q", "-b", &format!("taskq-{n}"), &format!(".worktrees/taskq-{n}")]); }
        let deep = (0..12).fold(r.root.join(".worktrees/taskq-1/target/debug/build"), |p, i| p.join(format!("aaaaaaaaaaaaaaaaaaaaaaaaaa{i}")));
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("f.rlib"), "x").unwrap();
        assert!(deep.join("f.rlib").as_os_str().len() > 300);
        for n in [1, 2] { b.close(n).unwrap(); }
        // work written between the status read and the removal: Git refuses, and the line says why
        let wip = r.root.join(".worktrees/taskq-2/wip");
        let o = r.tidy(&b, false).run(&mut |n| { if n == 2 { let _ = std::fs::write(&wip, "x"); } false }, &|_| None);
        assert!(!r.wt("taskq-1") && o.log.contains(&"#1 worktree removed".into()), "long ignored paths go: {:?}", o.log);
        assert!(r.wt("taskq-2") && o.log.iter().any(|l| l.starts_with("#2 worktree removal unconfirmed: fatal: '.worktrees/taskq-2' contains modified or untracked files")), "{:?}", o.log);
    }

    // Chain: tidy step 2 removal with submodules. Defect (#1875, hermes-fra1): `cleanup` and `cleanup --force` kept 13
    // clean closed-task worktrees, since Git refuses any worktree with submodules ("working trees containing submodules
    // cannot be moved or removed"), until the disk filled. Risk protected: removing uncommitted work in the parent or a
    // submodule, or submodule commits on no remote (they live only in the worktree's Git dir), and a kept line
    // without its reason.
    #[test]
    fn submodule_worktrees_leave_when_clean_and_keep_their_work() {
        let r = Repo::new("sub");
        let b = Fake::default();
        for _ in 0..5 { b.create("T", "", &["taskq".into()]).unwrap(); }
        let lib = r.base.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        r.sh(&lib, &["init", "-q", "-b", "main"]);
        std::fs::write(lib.join("l"), "1").unwrap();
        r.sh(&lib, &["add", "l"]);
        r.sh(&lib, &["commit", "-q", "-m", "lib"]);
        for n in 1..=5 {
            let wt = r.root.join(format!(".worktrees/taskq-{n}"));
            r.sh(&r.root, &["worktree", "add", "-q", "-b", &format!("taskq-{n}"), wt.to_str().unwrap()]);
            r.sh(&wt, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", lib.to_str().unwrap(), "sub"]);
            r.sh(&wt, &["commit", "-q", "-m", "sub"]);
        }
        let wt = |n: u64| r.root.join(format!(".worktrees/taskq-{n}"));
        // 2: untracked file in the submodule; 3: modified file in the parent; 4: a submodule commit on no remote,
        // recorded in the parent; 5: a modified submodule file, then `--force`
        std::fs::write(wt(2).join("sub/wip"), "x").unwrap();
        std::fs::write(wt(3).join("a"), "edited").unwrap();
        std::fs::write(wt(4).join("sub/l"), "2").unwrap();
        r.sh(&wt(4).join("sub"), &["commit", "-q", "-am", "local"]);
        r.sh(&wt(4), &["commit", "-q", "-am", "bump"]);
        std::fs::write(wt(5).join("sub/l"), "mine").unwrap();
        for n in 1..=5 { b.close(n).unwrap(); }
        let o = pass(&r.tidy(&b, false));
        assert!(!r.wt("taskq-1") && o.log.contains(&"#1 worktree removed".into()), "a clean one goes: {:?}", o.log);
        assert_eq!(r.sh(&r.root, &["branch", "--list", "taskq-1"]), "taskq-1", "its unmerged branch stays");
        for (n, line) in [(2, "#2 worktree kept: uncommitted changes in submodule sub; inspect them before `taskq cleanup --force`"),
            (3, "#3 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`"),
            (4, "#4 worktree kept: submodule sub has commits on no remote; push them first"),
            (5, "#5 worktree kept: uncommitted changes in submodule sub; inspect them before `taskq cleanup --force`")] {
            assert!(r.wt(&format!("taskq-{n}")) && o.log.contains(&line.into()), "#{n}: {:?}", o.log);
        }
        assert_eq!(std::fs::read_to_string(wt(2).join("sub/wip")).unwrap(), "x");
        // `--force` saves the submodule's patch first; commits on no remote still keep #4
        let o = Tidy { force: true, ..r.tidy(&b, false) }.run(&mut |_| false, &|_| None);
        assert!(!r.wt("taskq-5") && r.wt("taskq-4"), "{:?}", o.log);
        assert!(std::fs::read_to_string(r.root.join(".taskq/trash/taskq-5--sub.patch")).unwrap().contains("+mine"), "{:?}", o.log);
    }

    // Chain: `taskq cleanup --dry-run [--force]` on a temp repo. Defect (#2060): cleanup had no preview, and `--force`
    // removed dirty worktrees without a list. Risk protected: a dry run that fetches, prunes, removes, deletes a branch,
    // moves the root or writes `.taskq`, and a line that differs from what the real pass then does.
    #[test]
    fn dry_run_prints_every_line_and_changes_nothing() {
        let r = Repo::new("dry");
        let b = Fake::default();
        for _ in 0..5 { b.create("T", "", &["taskq".into()]).unwrap(); }
        for w in ["taskq-1", "taskq-2", "taskq-9"] { r.sh(&r.root, &["worktree", "add", "-q", "-b", w, &format!(".worktrees/{w}")]); }
        r.sh(&r.root, &["worktree", "add", "-q", "--detach", ".worktrees/review-3"]);
        std::fs::remove_dir_all(r.root.join(".worktrees/taskq-9")).unwrap();
        std::fs::write(r.root.join(".worktrees/taskq-2/wip"), "x").unwrap();
        r.sh(&r.root, &["branch", "taskq-5", "origin/main"]);
        for n in [1, 2, 5] { b.close(n).unwrap(); }
        r.upstream("b", "1");
        r.sh(&r.root, &["fetch", "-q", "origin", "main"]);
        let known = |n: u64| (n == 3).then_some(State::Review);
        let before = r.state();
        let o = Tidy { dry: true, ..r.tidy(&b, false) }.run(&mut |_| false, &known);
        assert_eq!(o.lines, ["tidy would prune 1 worktree registry entries without a directory", "#3 review worktree kept: task in review",
            "#1 worktree would be removed", "#2 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`",
            "#5 branch would be deleted", "root would fast-forward 1 commits to origin/main"]);
        let o = Tidy { dry: true, force: true, ..r.tidy(&b, false) }.run(&mut |_| false, &known);
        assert!(o.lines.contains(&"#2 worktree would be removed after saving a patch to .taskq/trash".into()), "{:?}", o.lines);
        assert_eq!(r.state(), before, "tree, registry, branches, root and .taskq unchanged");
        // the real pass does what the preview said
        let o = r.tidy(&b, false).run(&mut |_| false, &known);
        assert!(!r.wt("taskq-1") && r.wt("taskq-2") && r.wt("review-3") && o.report["root"] == "current", "{:?}", o.lines);
        assert_eq!(r.sh(&r.root, &["branch", "--list", "taskq-5"]), "");
    }

    // Chain: tidy step 2 on directories outside the registry. Defect (#2060, Windows taskq): review-1594, taskq-948 and
    // others were empty leftovers without `.git`, so Git read the root's 1336 changes there, and the pass skipped them
    // with no line. Risk protected: deleting a non-empty unregistered directory, and a kept task worktree with no reason.
    #[test]
    fn unregistered_task_directories_and_kept_reasons() {
        let r = Repo::new("unreg");
        let b = Fake::default();
        for _ in 0..4 { b.create("T", "", &["taskq".into()]).unwrap(); }
        std::fs::create_dir_all(r.root.join(".worktrees/review-1")).unwrap();
        std::fs::create_dir_all(r.root.join(".worktrees/taskq-2")).unwrap();
        std::fs::write(r.root.join(".worktrees/taskq-2/notes"), "x").unwrap();
        for w in ["taskq-3", "taskq-4"] { r.sh(&r.root, &["worktree", "add", "-q", "-b", w, &format!(".worktrees/{w}")]); }
        std::fs::write(r.root.join(".worktrees/taskq-4/wip"), "x").unwrap();
        for n in [1, 2, 4] { b.close(n).unwrap(); }
        let o = r.tidy(&b, false).run(&mut |_| false, &|n| (n == 3).then_some(State::Doing));
        assert!(!r.wt("review-1") && r.wt("taskq-2") && r.wt("taskq-3") && r.wt("taskq-4"), "{:?}", o.lines);
        let kept2 = "#2 worktree kept: not a registered worktree; inspect it and delete it by hand";
        let kept4 = "#4 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`";
        assert_eq!(o.lines, ["#1 review worktree removed: empty directory, not a registered worktree", kept2, "#3 worktree kept: task open (doing)", kept4]);
        assert!(!o.log.iter().any(|l| l.contains("task open")), "the service log keeps only removals and refusals: {:?}", o.log);
        assert_eq!(o.conditions["kept"], [kept2, kept4]);
        assert!(o.conditions["dirty"].len() == 1 && o.conditions["dirty"][0].starts_with("#4 worktree dirty, untouched "), "{:?}", o.conditions);
        // a live child keeps it with its own line, and no finding condition
        let o = r.tidy(&b, false).run(&mut |n| n == 4, &|n| (n == 3).then_some(State::Doing));
        assert!(o.lines.contains(&"#4 worktree kept: running child on this host".into()) && !o.conditions.contains_key("dirty"), "{:?}", o.lines);
    }

    // Chain: tidy on a root Git calls dubious ownership. Defect (#2060, Windows coreq, dashq, desiign, serverq, specq):
    // `tidy skipped: git unreadable: fatal: detected dubious ownership in repository`. Risk protected: trusting any path
    // besides the root and its `.worktrees` (no `*`, no global config write).
    #[test]
    fn dubious_ownership_root_is_tidied() {
        let r = Repo::new("owner");
        let b = Fake::default();
        b.create("T", "", &["taskq".into()]).unwrap();
        r.sh(&r.root, &["worktree", "add", "-q", "-b", "taskq-1", ".worktrees/taskq-1"]);
        let foreign = r.base.join("foreign");
        r.sh(&r.root, &["worktree", "add", "-q", "--detach", foreign.to_str().unwrap()]);
        b.close(1).unwrap();
        r.upstream("b", "1");
        TEST_ENV.with_borrow_mut(|e| *e = vec![("GIT_TEST_ASSUME_DIFFERENT_OWNER".into(), "1".into()), ("GIT_ALLOW_PROTOCOL".into(), "https".into())]);
        match git(&r.root, &["status"]) {
            Err(e) => assert!(e.contains("dubious ownership"), "outside a pass Git refuses the root: {e}"),
            // Git before 2.45 honours the test owner switch only on Windows (WSL git 2.43): nothing to simulate
            Ok(_) => { TEST_ENV.with_borrow_mut(Vec::clear); eprintln!("skipped: this Git cannot simulate another owner"); return; }
        }
        let o = pass(&r.tidy(&b, false));
        TEST_ENV.with_borrow_mut(Vec::clear);
        // Refuse the fixture's file transport explicitly: Git versions differ in upload-pack ownership checks.
        // All root/worktree trust and foreign-status assertions below remain unchanged.
        assert!(!r.wt("taskq-1") && o.report["root"] == "stale fetch failed", "{:?} {}", o.lines, o.report);
        assert_eq!(o.log, ["#1 worktree removed", "#1 branch deleted", "root stale: fetch failed"]);
        assert_eq!(pass(&r.tidy(&b, false)).report["root"], "current", "the same root reaches origin/main once Git trusts its owner");
        // the clean foreign checkout reads dirty: Git still refused its status, so it stayed untrusted
        assert_eq!((o.report["foreign"]["count"].as_u64(), o.report["foreign"]["dirty"].as_u64()), (Some(1), Some(1)));
    }

    // Chain: tidy step 3. Risk protected: removing recent or dirty unowned work.
    #[test]
    fn unowned_worktrees_leave_only_clean_and_a_week_old() {
        let r = Repo::new("unowned");
        let b = Fake::default();
        r.sh(&r.root, &["worktree", "add", "-q", "-b", "scratch", ".worktrees/scratch"]);
        r.sh(&r.root, &["worktree", "add", "-q", "--detach", ".worktrees/fresh"]);
        assert!(pass(&r.tidy(&b, false)).log.is_empty() && r.wt("scratch"), "recent stays");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86400);
        let age = |dir: &Path| {
            let mut stack = vec![dir.to_path_buf()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).unwrap().flatten() {
                    if e.file_type().unwrap().is_dir() { stack.push(e.path()); }
                    let _ = std::fs::File::options().write(true).open(e.path()).and_then(|f| f.set_modified(old));
                }
            }
        };
        // HEAD commit is recent: still kept
        age(&r.root.join(".worktrees/scratch"));
        assert!(pass(&r.tidy(&b, false)).log.is_empty() && r.wt("scratch"));
        // an old commit and old files: removed, branch kept
        let date = "2000-01-01T00:00:00";
        let o = std::process::Command::new("git").arg("-C").arg(r.root.join(".worktrees/scratch")).args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "old"])
            .env("GIT_COMMITTER_DATE", date).env("GIT_AUTHOR_DATE", date).output().unwrap();
        assert!(o.status.success());
        age(&r.root.join(".worktrees/scratch"));
        std::fs::write(r.root.join(".worktrees/fresh/wip"), "x").unwrap();
        // #2060: `workspace: external` keeps every other path host-owned, old and clean ones too
        assert!(pass(&r.tidy(&b, true)).log.is_empty() && r.wt("scratch"));
        let o = pass(&r.tidy(&b, false));
        assert!(!r.wt("scratch") && r.wt("fresh"), "{:?}", o.log);
        assert!(o.log[0].starts_with(".worktrees/scratch removed: unowned, untouched "), "{:?}", o.log);
        assert_eq!(r.sh(&r.root, &["branch", "--list", "scratch"]), "scratch", "branch kept");
    }

    // Chain: registry paths on Windows. Risk: a verbatim, backslash, MSYS or case variant of the root reads foreign
    // (never tidied) or a foreign path reads owned (removed).
    #[test]
    fn windows_path_spellings_compare_alike() {
        let root = norm(r"\\?\C:\Users\me\Codex\taskq", true);
        assert_eq!(root, "c:/users/me/codex/taskq");
        assert_eq!(place(&root, &norm("C:/Users/me/Codex/taskq", true)), None);
        assert_eq!(place(&root, &norm("C:/Users/Me/Codex/taskq/.worktrees/taskq-7", true)), Some(Some("taskq-7".into())));
        assert_eq!(place(&root, &norm(r"c:\users\me\codex\taskq\.worktrees\review-7\", true)), Some(Some("review-7".into())));
        assert_eq!(place(&root, &norm("/c/Users/me/Codex/taskq/.worktrees/dev-build", true)), Some(Some("dev-build".into())));
        assert_eq!(place(&root, &norm(r"C:\Users\me\AppData\Local\Temp\taskq-r1073-1", true)), Some(None), "temp dir is foreign");
        assert_eq!(place(&root, &norm("C:/Users/me/Codex/taskq/.worktrees/a/b", true)), Some(None), "nested is not a .worktrees entry");
        assert_eq!(place(&root, &norm("C:/Users/me/Codex/taskq-other/.worktrees/taskq-7", true)), Some(None));
        assert_eq!(norm(r"\\?\UNC\server\share\taskq", true), "//server/share/taskq");
        assert_eq!(norm("/Users/Me/taskq/", false), "/Users/Me/taskq", "case kept off Windows");
        assert_eq!([class("taskq-12"), class("review-12"), class("review-12-6177396825"), class("dev-build"), class("taskq-+1"), class("review-1-x")],
            [Class::Worker(12), Class::Review(12), Class::Review(12), Class::Reserved, Class::Unowned, Class::Unowned]);
    }

    #[test]
    fn health_line_reads_the_tidy_report() {
        let t = json!({"at": 1000, "outcome": "ok", "root": "behind 3", "task": {"count": 2, "dirty": 1, "bytes": 3u64 << 30}, "reserved": {"count": 1, "dirty": 0, "bytes": 0},
            "unowned": {"count": 0, "dirty": 0, "bytes": 0}, "foreign": {"count": 4, "dirty": 2, "bytes": 5}});
        assert_eq!(health_line("taskq", &t, 1000 + 7200), "taskq: root behind 3; .worktrees 3 (dirty 1, 3.0 GiB); foreign 4 (dirty 2); tidy 2h ok");
        assert_eq!(health_line("csgo", &Value::Null, 0), "csgo: tidy unknown (no report yet)");
    }
}
