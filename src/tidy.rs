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
const OPERATIONS: [(&str, &str); 6] = [
    ("MERGE_HEAD", "merge"),
    ("rebase-merge", "rebase"),
    ("rebase-apply", "rebase"),
    ("CHERRY_PICK_HEAD", "cherry-pick"),
    ("REVERT_HEAD", "revert"),
    ("BISECT_LOG", "bisect"),
];

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
        Outcome {
            lines: log.clone(),
            log,
            report,
            counts,
            conditions: BTreeMap::new(),
        }
    }
}

/// What step 2 or 3 did with one directory: removed, kept on a refusal (logged), kept as expected (cleanup only).
enum Step {
    Done(String),
    Refused(String),
    Note(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Class {
    Worker(u64),
    Review(u64),
    Reserved,
    Unowned,
    Foreign,
}

impl Class {
    fn key(self) -> &'static str {
        match self {
            Class::Worker(_) | Class::Review(_) => "task",
            Class::Reserved => "reserved",
            Class::Unowned => "unowned",
            Class::Foreign => "foreign",
        }
    }
}

/// The class of directory `name` under `.worktrees`: `taskq-<N>`, `review-<N>` (also the older `review-<N>-<attempt>`),
/// `dev-build`, else unowned.
fn class(name: &str) -> Class {
    let num = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    if name == "dev-build" {
        return Class::Reserved;
    }
    if let Some(n) = name.strip_prefix("taskq-").and_then(num) {
        return Class::Worker(n);
    }
    if let Some(r) = name.strip_prefix("review-") {
        let (n, rest) = r.split_once('-').map_or((r, None), |(n, a)| (n, Some(a)));
        if let Some(n) = num(n).filter(|_| rest.is_none_or(|a| num(a).is_some())) {
            return Class::Review(n);
        }
    }
    Class::Unowned
}

/// How a line names a task-class directory.
fn what(n: u64, class: Class) -> String {
    if matches!(class, Class::Review(_)) {
        format!("#{n} review worktree")
    } else {
        format!("#{n} worktree")
    }
}

/// A path as Git and the OS may spell it, for comparison only. On Windows the verbatim prefix (`\\?\`, `\\?\UNC\`),
/// backslashes, the MSYS `/c/` form and letter case read alike.
fn norm(p: &str, windows: bool) -> String {
    let mut s = p.replace('\\', "/");
    if windows {
        if let Some(r) = s.strip_prefix("//?/UNC/") {
            s = format!("//{r}");
        } else if let Some(r) = s.strip_prefix("//?/") {
            s = r.into();
        }
        let b = s.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/' {
            s = format!("{}:{}", b[1] as char, &s[2..]);
        }
        s = s.to_lowercase();
    }
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
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
    if entry == root {
        return None;
    }
    let name = entry
        .strip_prefix(&format!("{root}/.worktrees/"))
        .filter(|n| !n.is_empty() && !n.contains('/'));
    Some(name.map(String::from))
}

/// Telemetry name per Git subcommand: expected non-zero exits (`merge-base --is-ancestor`, `symbolic-ref -q`,
/// `branch -d`) stay apart from removal failures.
fn label(args: &[&str]) -> &'static str {
    match args.first().copied().unwrap_or("") {
        "worktree" => {
            if args.get(1) == Some(&"remove") {
                "cleanup.git.remove"
            } else {
                "cleanup.git.worktree"
            }
        }
        "status" => "cleanup.git.status",
        "branch" => "cleanup.git.branch",
        "merge-base" => "cleanup.git.merge_base",
        "merge" => "cleanup.git.merge",
        "fetch" => "cleanup.git.fetch",
        "ls-remote" => "cleanup.git.ls_remote",
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
    let s = std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    match s.strip_prefix("//?/UNC/") {
        Some(r) => format!("//{r}"),
        None => s.strip_prefix("//?/").map_or(s.clone(), String::from),
    }
}

/// #2060: Git refuses a repository another account owns (`detected dubious ownership`, Windows roots); the pass trusts
/// only its own root and the directories under its `.worktrees`, never `*` and never a foreign entry.
struct Safe;

impl Safe {
    fn set(root: &Path) -> Safe {
        let base = spelled(root);
        let mut values = vec![base.clone(), format!("{base}/.worktrees/*")];
        values.extend(
            std::fs::read_dir(root.join(".worktrees"))
                .into_iter()
                .flatten()
                .flatten()
                .map(|d| spelled(&d.path())),
        );
        SAFE.with_borrow_mut(|s| *s = Some((key(root), values)));
        Safe
    }
}

impl Drop for Safe {
    fn drop(&mut self) {
        SAFE.with_borrow_mut(|s| *s = None);
    }
}

/// The `safe.directory` values for a Git call in `dir`: the pass's, plus `dir` itself when it lies under the root's
/// `.worktrees` (a submodule); none outside a pass.
fn safe(dir: &Path) -> Vec<String> {
    SAFE.with_borrow(|s| {
        s.as_ref().map(|(root, values)| {
            let mut v = values.clone();
            if key(dir).starts_with(&format!("{root}/.worktrees/")) {
                v.push(spelled(dir));
            }
            v
        })
    })
    .unwrap_or_default()
}

/// `core.longpaths`: Git for Windows otherwise fails a removal at 260 characters (`Filename too long`, deep `target/`
/// paths), after dropping the registry entry and `.git` file, so the rest of the directory stays; other Gits ignore it.
fn run(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut c = crate::platform::command("git");
    c.arg("-C").arg(dir).args([
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.longpaths=true",
        "--no-optional-locks",
    ]);
    for v in safe(dir) {
        c.arg("-c").arg(format!("safe.directory={v}"));
    }
    #[cfg(test)]
    TEST_ENV.with_borrow(|e| {
        c.envs(e.iter().map(|(k, v)| (k, v)));
    });
    c.args(args);
    match coreq::process::output(label(args), &mut c, std::time::Duration::from_secs(120)) {
        Ok(o) => Ok(o.stdout),
        Err(coreq::process::Failure::Exit(o)) => Err(String::from_utf8_lossy(&o.stderr)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("git failed")
            .to_string()),
        Err(e) => Err(format!("git {}: {e}", args.first().unwrap_or(&""))),
    }
}

/// The origin default branch ([rule](../docs/design/worktrees.md#origin-default-branch)) and whether origin answered:
/// what origin states now, read without a write; only when it is unreachable, Git's recorded `refs/remotes/origin/HEAD`.
/// Never the checked-out branch. `None` is unknown and refuses.
fn default_branch(root: &Path) -> (Option<String>, bool) {
    let listed = git(root, &["ls-remote", "--symref", "origin", "HEAD"]);
    let name = match &listed {
        Ok(text) => text
            .lines()
            .find_map(|l| l.strip_prefix("ref: refs/heads/")?.strip_suffix("\tHEAD"))
            .map(String::from),
        Err(_) => git(root, &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"])
            .ok()
            .and_then(|t| {
                t.trim()
                    .strip_prefix("refs/remotes/origin/")
                    .map(String::from)
            }),
    };
    let name =
        name.filter(|n| git(root, &["check-ref-format", &format!("refs/heads/{n}")]).is_ok());
    (name, listed.is_ok())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    run(dir, args).map(|o| String::from_utf8_lossy(&o).replace("\r\n", "\n"))
}

#[derive(Clone, Debug, Default)]
struct Entry {
    path: String,
    branch: Option<String>,
    locked: bool,
    prunable: bool,
}

fn registry(root: &Path) -> Result<Vec<Entry>, String> {
    Ok(git(root, &["worktree", "list", "--porcelain"])?
        .split("\n\n")
        .filter_map(|block| {
            let mut e = Entry::default();
            for l in block.lines() {
                if let Some(p) = l.strip_prefix("worktree ") {
                    e.path = p.into();
                }
                if let Some(b) = l.strip_prefix("branch refs/heads/") {
                    e.branch = Some(b.into());
                }
                if l == "locked" || l.starts_with("locked ") {
                    e.locked = true;
                }
                if l == "prunable" || l.starts_with("prunable ") {
                    e.prunable = true;
                }
            }
            (!e.path.is_empty()).then_some(e)
        })
        .collect())
}

/// Uncommitted changes in `dir` or any submodule under it, as Git's own removal check reads them; unknown reads changed.
fn changed(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--ignore-submodules=none"])
        .map_or(true, |s| !s.trim().is_empty())
}

/// Checked-out submodules under `dir`, nested ones too, as `/` paths relative to `dir`.
fn submodules(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = vec![];
    for line in git(dir, &["ls-files", "-z", "--stage"])?.split('\0') {
        let Some((meta, path)) = line.split_once('\t') else {
            continue;
        };
        if !meta.starts_with("160000 ") || !dir.join(path).join(".git").exists() {
            continue;
        }
        out.push(path.to_string());
        out.extend(
            submodules(&dir.join(path))?
                .into_iter()
                .map(|s| format!("{path}/{s}")),
        );
    }
    Ok(out)
}

/// Total file bytes and newest modification (Unix seconds) under `dir`, else of `dir` itself; symlinks are not followed.
/// `ponytail:` a full walk each hour (a 32 GB Windows `.worktrees` of 55k files took 3 s); a size cache is the upgrade.
fn walk(dir: &Path) -> (u64, u64) {
    let secs = |m: &std::fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    let (mut bytes, mut newest) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(m) = e.metadata() else { continue };
            newest = newest.max(secs(&m));
            if m.is_dir() {
                stack.push(e.path());
            } else {
                bytes += m.len();
            }
        }
    }
    // an empty leftover directory has no entry to age it
    if newest == 0 {
        newest = std::fs::metadata(dir).map_or(0, |m| secs(&m));
    }
    (bytes, newest)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

struct Item {
    name: String,
    path: PathBuf,
    class: Class,
    entry: Option<Entry>,
    dirty: bool,
    bytes: u64,
    newest: u64,
    removed: bool,
}

impl Item {
    fn new(name: String, path: PathBuf, class: Class, entry: Option<Entry>) -> Item {
        // a status that cannot run reads dirty: nothing is removed on an unknown
        let dirty = entry.is_some() && changed(&path);
        let (bytes, newest) = walk(&path);
        Item {
            name,
            path,
            class,
            entry,
            dirty,
            bytes,
            newest,
            removed: false,
        }
    }
}

impl Tidy<'_> {
    /// One tidy pass of the root. `live(n)`: a child of this service still runs task N. `known(n)`: the state of an
    /// open task the caller just read; otherwise a fresh board read decides (R12).
    pub fn run(
        &self,
        live: &mut dyn FnMut(u64) -> bool,
        known: &dyn Fn(u64) -> Option<State>,
    ) -> Outcome {
        let at = now();
        if !self.root.join(".git").exists() {
            return Outcome::new(vec![], Value::Null, BTreeMap::new());
        }
        let _safe = Safe::set(self.root);
        let dot = crate::paths::dot(self.root);
        if !self.dry
            && let Err(e) = std::fs::create_dir_all(&dot)
        {
            return self.skip(at, format!(".taskq unwritable: {e}"));
        }
        // a dry run creates no `.taskq`; one that exists is locked as for a real pass
        let _lock = if self.dry && !dot.exists() {
            None
        } else {
            match coreq::file_lock::try_lock(&dot.join("tidy.lock")) {
                Ok(l) => Some(l),
                Err(std::fs::TryLockError::WouldBlock) => {
                    return self.skip(at, "taskq lock held".into());
                }
                Err(std::fs::TryLockError::Error(e)) => {
                    return self.skip(at, format!("taskq lock unreadable: {e}"));
                }
            }
        };
        let dirs = match git(self.root, &["rev-parse", "--git-dir", "--git-common-dir"]) {
            Ok(t) => t
                .lines()
                .map(|l| self.root.join(l.trim()))
                .collect::<Vec<_>>(),
            Err(e) => return self.skip(at, format!("git unreadable: {e}")),
        };
        let (gitdir, common) = (
            dirs.first().cloned().unwrap_or_default(),
            dirs.get(1).cloned().unwrap_or_default(),
        );
        if let Some(l) = GIT_LOCKS
            .iter()
            .find(|l| common.join(l).exists() || gitdir.join(l).exists())
        {
            return self.skip(at, format!("Git lock {l} held"));
        }
        let mut log = vec![];
        // indices of `log` lines only `taskq cleanup` prints
        let mut notes = BTreeSet::new();
        let mut counts = BTreeMap::new();
        let mut conditions: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        let (default, reachable) = default_branch(self.root);
        let default = default.as_deref();
        // a dry run reads the last fetched `origin/<default>`; an unreachable origin costs no second network wait
        let fetched = match default {
            Some(d) if !self.dry && reachable => git(
                self.root,
                &["fetch", "-q", "origin", &format!("refs/heads/{d}")],
            ),
            Some(_) if !self.dry => Err("origin unreachable".to_string()),
            _ => Ok(String::new()),
        };
        // 1. registry prune: entries whose directory is gone; Git keeps locked ones
        let before = registry(self.root).map_or(0, |r| r.len());
        let pruned = if self.dry {
            let n = registry(self.root).map_or(0, |r| r.iter().filter(|e| e.prunable).count());
            if n > 0 {
                log.push(format!(
                    "tidy would prune {n} worktree registry entries without a directory"
                ));
            }
            n
        } else {
            let after = git(self.root, &["worktree", "prune"])
                .and_then(|_| registry(self.root))
                .map_or(before, |r| r.len());
            if after < before {
                log.push(format!(
                    "tidy pruned {} worktree registry entries without a directory",
                    before - after
                ));
            }
            before - after.min(before)
        };
        counts.insert("pruned".into(), pruned as u64);
        let mut items = self.items();
        let mut stages = HashMap::new();
        let (mut removed, mut refused, mut branches) = (0, 0, 0);
        // 2. task class, 3. unowned class (not in an external workspace)
        for it in items.iter_mut() {
            let task = match it.class {
                Class::Worker(n) | Class::Review(n) => Some(n),
                _ => None,
            };
            let alive = task.is_some_and(|n| live(n));
            let outcome = match it.class {
                Class::Worker(n) | Class::Review(n) if alive => Some(Step::Note(format!(
                    "{} kept: running child on this host",
                    what(n, it.class)
                ))),
                Class::Worker(n) | Class::Review(n) => {
                    Some(self.task(n, it, known, &mut stages, &mut log))
                }
                Class::Unowned if !self.external && it.entry.is_some() => self.unowned(it, at),
                Class::Unowned | Class::Reserved | Class::Foreign => None,
            };
            let refusal = match outcome {
                Some(Step::Done(line)) => {
                    it.removed = !self.dry;
                    removed += 1;
                    log.push(line);
                    None
                }
                Some(Step::Refused(line)) => {
                    refused += 1;
                    log.push(line.clone());
                    Some(line)
                }
                Some(Step::Note(line)) => {
                    notes.insert(log.len());
                    log.push(line);
                    None
                }
                None => None,
            };
            // findings: a task worktree dirty while no child runs it; a closed task's worktree that stays
            if let Some(n) = task.filter(|_| !it.removed && !alive) {
                if it.dirty {
                    conditions.entry("dirty").or_default().push(format!(
                        "{} dirty, untouched {}",
                        what(n, it.class),
                        crate::time::age(at.saturating_sub(it.newest) as i64)
                    ));
                }
                if stages.get(&n) == Some(&Some(State::Closed))
                    && let Some(line) = refusal
                {
                    conditions.entry("kept").or_default().push(line);
                }
            }
        }
        // 4. local `taskq-<N>` branches without a worktree
        let (deleted, kept) = self.branches(default, live, known, &mut stages, &mut log);
        branches += deleted;
        refused += kept;
        // 5. root
        let (root, behind, moved) = self.root_step(default, &fetched, &gitdir, &mut log);
        if root.starts_with("stale") {
            conditions
                .entry("stale")
                .or_default()
                .push(format!("root {root}"));
        }
        let mut report = json!({"at": at, "outcome": "ok", "root": root});
        for c in CLASSES {
            let kept: Vec<&Item> = items
                .iter()
                .filter(|it| !it.removed && it.class.key() == c)
                .collect();
            let oldest = kept
                .iter()
                .map(|it| at.saturating_sub(it.newest))
                .max()
                .unwrap_or(0);
            let v = json!({"count": kept.len(), "dirty": kept.iter().filter(|it| it.dirty).count(), "bytes": kept.iter().map(|it| it.bytes).sum::<u64>(), "oldest_s": oldest});
            for k in ["count", "dirty", "bytes", "oldest_s"] {
                counts.insert(
                    if k == "count" {
                        c.to_string()
                    } else {
                        format!("{c}_{k}")
                    },
                    v[k].as_u64().unwrap_or(0),
                );
            }
            report[c] = v;
        }
        items.clear();
        counts.extend([
            ("removed".into(), removed),
            ("refused".into(), refused),
            ("branches_deleted".into(), branches),
            ("root_behind".into(), behind),
            ("root_moved".into(), moved as u64),
            ("skipped".into(), 0),
        ]);
        self.save(&report);
        let lines = log.clone();
        let log = log
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !notes.contains(i))
            .map(|(_, l)| l)
            .collect();
        Outcome {
            log,
            lines,
            report,
            counts,
            conditions,
        }
    }

    /// A skipped pass keeps the last report and says why.
    fn skip(&self, at: u64, reason: String) -> Outcome {
        let path = crate::paths::dot(self.root).join("tidy.json");
        let mut report = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        report["at"] = json!(at);
        report["outcome"] = json!(format!("skipped {reason}"));
        self.save(&report);
        Outcome::new(
            vec![format!("tidy skipped: {reason}")],
            report,
            BTreeMap::from([("skipped".into(), 1)]),
        )
    }

    fn save(&self, report: &Value) {
        // a cache of the last outcome for `status.json` (R1); a failed write only loses the report; a dry run writes none
        if !self.dry {
            let _ = std::fs::write(
                crate::paths::dot(self.root).join("tidy.json"),
                report.to_string(),
            );
        }
    }

    /// Directories under `.worktrees` and foreign registry entries, each with its class.
    fn items(&self) -> Vec<Item> {
        let root = key(self.root);
        let mut items = vec![];
        let mut named = BTreeMap::new();
        for e in registry(self.root).unwrap_or_default() {
            match place(&root, &key(Path::new(&e.path))) {
                None => {}
                Some(Some(name)) => {
                    named.insert(name, e);
                }
                Some(None) => items.push(Item::new(
                    e.path.clone(),
                    PathBuf::from(&e.path),
                    Class::Foreign,
                    Some(e),
                )),
            }
        }
        let dirs: BTreeSet<String> = std::fs::read_dir(self.root.join(".worktrees"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|d| d.file_type().is_ok_and(|t| t.is_dir()))
            .map(|d| d.file_name().to_string_lossy().into_owned())
            .collect();
        // a registered name differing only in case (Windows) matches its directory
        for name in dirs {
            let entry = named
                .keys()
                .find(|k| norm(k, cfg!(windows)) == norm(&name, cfg!(windows)))
                .cloned()
                .and_then(|k| named.remove(&k));
            items.push(Item::new(
                name.clone(),
                self.root.join(".worktrees").join(&name),
                class(&name),
                entry,
            ));
        }
        items
    }

    /// The task's state: `known` from the caller, else a fresh read (`None`: unknown, nothing is removed).
    fn stage(
        &self,
        n: u64,
        known: &dyn Fn(u64) -> Option<State>,
        stages: &mut HashMap<u64, Option<State>>,
    ) -> Option<State> {
        *stages.entry(n).or_insert_with(|| {
            known(n).or_else(|| match self.board.issue(n) {
                Err(_) => None,
                Ok(i) if !i.open => Some(State::Closed),
                Ok(_) => crate::board::entries(self.board, n)
                    .ok()
                    .map(|e| core::project(n, &e, false).task.state),
            })
        })
    }

    /// Step 2: `taskq-<N>` leaves with a closed task, `review-<N>` once the task is no longer in `review`; every other
    /// outcome names why the directory stays.
    fn task(
        &self,
        n: u64,
        it: &Item,
        known: &dyn Fn(u64) -> Option<State>,
        stages: &mut HashMap<u64, Option<State>>,
        log: &mut Vec<String>,
    ) -> Step {
        let review = matches!(it.class, Class::Review(_));
        let what = what(n, it.class);
        let Some(state) = self.stage(n, known, stages) else {
            return Step::Note(format!("{what} kept: task state unknown"));
        };
        let closed = state == State::Closed;
        if !(closed || review && state != State::Review) {
            return Step::Note(format!(
                "{what} kept: {}",
                if review {
                    "task in review".into()
                } else {
                    format!("task open ({})", state.name())
                }
            ));
        }
        // #2060: a task directory outside the registry is no worktree; Git reads the root's status in it
        if it.entry.is_none() {
            const WHY: &str = "empty directory, not a registered worktree";
            if std::fs::read_dir(&it.path).map_or(true, |mut d| d.next().is_some()) {
                return Step::Refused(format!(
                    "{what} kept: not a registered worktree; inspect it and delete it by hand"
                ));
            }
            if self.dry {
                return Step::Done(format!("{what} would be removed: {WHY}"));
            }
            return match std::fs::remove_dir(&it.path) {
                Ok(()) => Step::Done(format!("{what} removed: {WHY}")),
                Err(e) => Step::Refused(format!("{what} kept: {WHY}, removal failed: {e}")),
            };
        }
        if it.entry.as_ref().is_some_and(|e| e.locked) {
            return Step::Refused(format!(
                "{what} kept: locked (git worktree unlock it first)"
            ));
        }
        let subs = match submodules(&it.path) {
            Ok(s) => s,
            Err(e) => return Step::Refused(format!("{what} kept: submodules unreadable: {e}")),
        };
        if it.dirty && !self.force {
            let at = subs
                .iter()
                .find(|s| changed(&it.path.join(s)))
                .map_or(String::new(), |s| format!(" in submodule {s}"));
            return Step::Refused(format!(
                "{what} kept: uncommitted changes{at}; inspect them before `taskq cleanup --force`"
            ));
        }
        // #1875: a submodule's commits live only in this worktree's Git dir, so removal would lose those on no remote
        if let Some(s) = subs.iter().find(|s| {
            git(
                &it.path.join(s),
                &[
                    "rev-list",
                    "-n",
                    "1",
                    "HEAD",
                    "--branches",
                    "--not",
                    "--remotes",
                ],
            )
            .map_or(true, |o| !o.trim().is_empty())
        }) {
            return Step::Refused(format!(
                "{what} kept: submodule {s} has commits on no remote; push them first"
            ));
        }
        if let Err(e) = self.held(n, closed, log) {
            return Step::Refused(format!("{what} kept: {e}"));
        }
        if self.dry {
            return Step::Done(format!(
                "{what} would be removed{}",
                if it.dirty {
                    " after saving a patch to .taskq/trash"
                } else {
                    ""
                }
            ));
        }
        let rel = format!(".worktrees/{}", it.name);
        let force = || git(self.root, &["worktree", "remove", "--force", &rel]).map(drop);
        // the refusal line carries Git's first stderr line; the service logs it once while it stays the same
        let removed = if self.force && it.dirty {
            let trash = crate::paths::dot(self.root).join("trash");
            let save = |dir: &Path, file: String| {
                run(dir, &["add", "-A"])
                    .and_then(|_| run(dir, &["diff", "--cached", "--binary"]))
                    .and_then(|patch| {
                        if patch.is_empty() {
                            return Ok(());
                        }
                        std::fs::create_dir_all(&trash)
                            .and_then(|_| std::fs::write(trash.join(file), &patch))
                            .map_err(|e| format!("patch save failed: {e}"))
                    })
            };
            subs.iter()
                .try_for_each(|s| {
                    save(
                        &it.path.join(s),
                        format!("{}--{}.patch", it.name, s.replace('/', "-")),
                    )
                })
                .and_then(|_| save(&it.path, format!("{}.patch", it.name)))
                .and_then(|_| force())
        } else {
            // #1875: Git refuses any worktree with submodules ("working trees containing submodules cannot be moved or
            // removed"); a fresh clean read of it and its submodules stands in for Git's own check
            git(self.root, &["worktree", "remove", &rel])
                .map(drop)
                .or_else(|e| match e.contains("containing submodules") {
                    true if changed(&it.path) => {
                        Err(format!("{} contains modified or untracked files", rel))
                    }
                    true => force(),
                    false => Err(e),
                })
        };
        match removed {
            Ok(()) => Step::Done(format!("{what} removed")),
            Err(e) => Step::Refused(format!(
                "{what} removal unconfirmed: {e}; {}",
                if self.force {
                    "inspect the worktree"
                } else {
                    "inspect uncommitted work before `taskq cleanup --force`"
                }
            )),
        }
    }

    /// #1549: only this host's executions (its name or OS hostname) hold a worktree. Of a closed task, a boot after
    /// intent releases them (#1386); of an open task any unreleased one holds it.
    fn held(&self, n: u64, closed: bool, log: &mut Vec<String>) -> Result<(), String> {
        let comments = self
            .board
            .comments(n)
            .map_err(|e| format!("board read failed: {e}"))?;
        let entries: Vec<_> = comments
            .iter()
            .filter(|c| c.trusted)
            .map(|c| core::Entry {
                id: c.id,
                body: c.body.clone(),
                edited: c.edited,
            })
            .collect();
        let mut held = None;
        for x in core::executions(n, &entries).into_iter().filter(|x| {
            x.release.is_none() && (x.host == self.me || x.host == crate::config::hostname())
        }) {
            if !closed {
                return Err(format!("execution {} on {} not released", x.id, x.host));
            }
            let intent = comments
                .iter()
                .find(|c| c.id == x.id)
                .map(|c| c.created)
                .filter(|t| *t > 0);
            let ended = intent.zip((self.boot)().ok()).map(|(intent, at)| {
                crate::snapshot::verdict(&crate::snapshot::Signals {
                    local: true,
                    boot: Some((at, intent)),
                    ..Default::default()
                })
            });
            let released = match ended {
                Some(("ended", proof)) if self.dry => {
                    log.push(format!("#{n} would release {} exit: {proof}", x.id));
                    Ok(())
                }
                Some(("ended", proof)) => crate::service::release_execution_text(
                    self.board, n, x.attempt, x.id, "exit", &proof,
                )
                .map(|_| log.push(format!("#{n} released {} exit: {proof}", x.id))),
                _ => Err(format!(
                    "execution {} on {} termination unknown; stop its process or reboot this host",
                    x.id, x.host
                )),
            };
            if let Err(e) = released {
                held.get_or_insert(e);
            }
        }
        held.map_or(Ok(()), Err)
    }

    /// Step 3: a clean unowned worktree untouched for 7 days; its branch stays.
    fn unowned(&self, it: &Item, at: u64) -> Option<Step> {
        let commit = git(&it.path, &["log", "-1", "--format=%ct", "HEAD"])
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())?;
        let touched = it.newest.max(commit);
        if it.dirty
            || it.entry.as_ref().is_some_and(|e| e.locked)
            || at.saturating_sub(touched) < UNOWNED_AFTER
        {
            return None;
        }
        let rel = format!(".worktrees/{}", it.name);
        let age = crate::time::age(at.saturating_sub(touched) as i64);
        if self.dry {
            return Some(Step::Done(format!(
                "{rel} would be removed: unowned, untouched {age}"
            )));
        }
        Some(match git(self.root, &["worktree", "remove", &rel]) {
            Ok(_) => Step::Done(format!("{rel} removed: unowned, untouched {age}")),
            Err(e) => Step::Refused(format!("{rel} kept: {e}")),
        })
    }

    /// Step 4: delete local `taskq-<N>` branches without a worktree of closed tasks. `-D` only when the tip is in
    /// `origin/<default>` or the close receipt records a merge of the task's result at this tip; else `-d`, whose
    /// refusal keeps the branch. An unknown default has no such tip. Returns (deleted, kept).
    fn branches(
        &self,
        default: Option<&str>,
        live: &mut dyn FnMut(u64) -> bool,
        known: &dyn Fn(u64) -> Option<State>,
        stages: &mut HashMap<u64, Option<State>>,
        log: &mut Vec<String>,
    ) -> (u64, u64) {
        let Ok(refs) = git(
            self.root,
            &[
                "for-each-ref",
                "--format=%(refname:short) %(objectname)",
                "refs/heads/taskq-*",
            ],
        ) else {
            return (0, 0);
        };
        let checked: BTreeSet<String> = registry(self.root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| e.branch)
            .collect();
        let (mut deleted, mut kept) = (0, 0);
        let target = default.map_or("the default branch".to_string(), |d| format!("origin/{d}"));
        for (name, tip) in refs.lines().filter_map(|l| l.split_once(' ')) {
            let Class::Worker(n) = class(name) else {
                continue;
            };
            if checked.contains(name)
                || live(n)
                || self.stage(n, known, stages) != Some(State::Closed)
            {
                continue;
            }
            let merged = default.is_some_and(|d| {
                git(
                    self.root,
                    &[
                        "merge-base",
                        "--is-ancestor",
                        tip,
                        &format!("refs/remotes/origin/{d}"),
                    ],
                )
                .is_ok()
            }) || self.receipt(n, tip);
            // a dry run reads what `branch -d` checks: the tip merged into HEAD
            if self.dry {
                if merged || git(self.root, &["merge-base", "--is-ancestor", tip, "HEAD"]).is_ok() {
                    deleted += 1;
                    log.push(format!("#{n} branch would be deleted"));
                } else {
                    kept += 1;
                    log.push(format!("#{n} branch kept: not merged into {target}"));
                }
                continue;
            }
            match git(
                self.root,
                &["branch", if merged { "-D" } else { "-d" }, name],
            ) {
                Ok(_) => {
                    deleted += 1;
                    log.push(format!("#{n} branch deleted"));
                }
                Err(e) => {
                    kept += 1;
                    log.push(if merged {
                        format!("#{n} branch kept: {e}")
                    } else {
                        format!("#{n} branch kept: not merged into {target}: {e}")
                    });
                }
            }
        }
        (deleted, kept)
    }

    /// A close receipt (`merged #P as SHA`, GitLab `merged !P`) of a task whose result is this tip: a squash merge.
    fn receipt(&self, n: u64, tip: &str) -> bool {
        let Ok(entries) = crate::board::entries(self.board, n) else {
            return false;
        };
        let merged = entries.iter().any(|e| {
            e.body
                .lines()
                .next()
                .is_some_and(|h| h.trim().starts_with("taskq: close"))
                && (e.body.contains("merged #") || e.body.contains("merged !"))
        });
        merged
            && core::project(n, &entries, false)
                .task
                .result
                .is_some_and(|r| r.len() >= 7 && tip.starts_with(&r))
    }

    /// Step 5: fast-forward the root only on the origin default branch; anything else is `stale <reason>`.
    /// Returns (report, behind, moved).
    fn root_step(
        &self,
        default: Option<&str>,
        fetched: &Result<String, String>,
        gitdir: &Path,
        log: &mut Vec<String>,
    ) -> (String, u64, bool) {
        let tracked = default.map(|d| format!("refs/remotes/origin/{d}"));
        let counts = tracked.as_ref().and_then(|t| {
            git(
                self.root,
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...{t}"),
                ],
            )
            .ok()
        });
        let mut n = counts
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(|x| x.parse::<u64>().unwrap_or(0));
        let (ahead, behind) = (n.next().unwrap_or(0), n.next().unwrap_or(0));
        let branch = git(self.root, &["symbolic-ref", "-q", "HEAD"])
            .ok()
            .map(|h| h.trim().trim_start_matches("refs/heads/").to_string());
        let name = default.unwrap_or("");
        let reason = match branch.as_deref() {
            None => Some("detached HEAD".to_string()),
            Some(_) if default.is_none() => Some("default branch unknown".to_string()),
            Some(b) if b != name => Some(format!("on branch {b}")),
            _ => OPERATIONS
                .iter()
                .find(|(f, _)| gitdir.join(f).exists())
                .map(|(_, op)| format!("{op} in progress"))
                .or_else(|| fetched.as_ref().err().map(|_| "fetch failed".to_string()))
                .or_else(|| counts.is_none().then(|| format!("origin/{name} unknown")))
                .or_else(|| (ahead > 0 && behind > 0).then(|| "diverged".to_string())),
        };
        let reason = match reason {
            Some(r) => r,
            None if behind == 0 => return ("current".into(), 0, false),
            None if self.dry => {
                log.push(format!(
                    "root would fast-forward {behind} commits to origin/{name}"
                ));
                return (format!("behind {behind}"), behind, false);
            }
            None => match git(
                self.root,
                &[
                    "-c",
                    "merge.autoStash=false",
                    "merge",
                    "--ff-only",
                    "-q",
                    tracked.as_deref().unwrap_or(""),
                ],
            ) {
                Ok(_) => {
                    log.push(format!(
                        "root fast-forwarded {behind} commits to origin/{name}"
                    ));
                    return ("current".into(), 0, true);
                }
                Err(e) if e.contains("would be overwritten") => "overwrite conflict".into(),
                Err(_) => "fast-forward refused".into(),
            },
        };
        log.push(format!("root stale: {reason}"));
        (
            if behind > 0 {
                format!("stale {reason} (behind {behind})")
            } else {
                format!("stale {reason}")
            },
            behind,
            false,
        )
    }
}

/// `taskq profile health` (docs/design/worktrees.md): `<project>: root ..; .worktrees K (dirty D, S); foreign F (dirty G); tidy <age> <outcome>`.
pub fn health_line(project: &str, tidy: &Value, now: u64) -> String {
    let Some(at) = tidy["at"].as_u64() else {
        return format!("{project}: tidy unknown (no report yet)");
    };
    let n = |c: &str, k: &str| tidy[c][k].as_u64().unwrap_or(0);
    let own = ["task", "reserved", "unowned"];
    let (k, d, s) = own.iter().fold((0, 0, 0), |(k, d, s), c| {
        (k + n(c, "count"), d + n(c, "dirty"), s + n(c, "bytes"))
    });
    format!(
        "{project}: root {}; .worktrees {k} (dirty {d}, {}); foreign {} (dirty {}); tidy {} {}",
        tidy["root"].as_str().unwrap_or("unknown"),
        size(s),
        n("foreign", "count"),
        n("foreign", "dirty"),
        crate::time::age(now.saturating_sub(at) as i64),
        tidy["outcome"].as_str().unwrap_or("unknown")
    )
}

fn size(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GiB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{} MiB", b >> 20),
        b => format!("{} KiB", b.div_ceil(1024)),
    }
}

#[cfg(test)]
#[path = "tidy_tests.rs"]
mod tests;
