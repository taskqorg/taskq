use super::*;
use crate::board::Fake;

fn no_boot() -> Result<i64, String> {
    Err("boot time not read in tests".into())
}

/// Origin with one commit on `main` and a clone `root`; `sh` runs git in a directory and asserts success.
struct Repo {
    base: PathBuf,
    origin: PathBuf,
    root: PathBuf,
}
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
        r.sh(
            &r.base,
            &["clone", "-q", r.origin.to_str().unwrap(), "root"],
        );
        r
    }
    fn sh(&self, dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    /// A new commit on origin `main` touching file `f`.
    fn upstream(&self, f: &str, text: &str) {
        std::fs::write(self.origin.join(f), text).unwrap();
        self.sh(&self.origin, &["add", f]);
        self.sh(&self.origin, &["commit", "-q", "-m", f]);
    }
    fn head(&self) -> String {
        self.sh(&self.root, &["rev-parse", "HEAD"])
    }
    fn tidy<'a>(&'a self, b: &'a Fake, external: bool) -> Tidy<'a> {
        Tidy {
            board: b,
            root: &self.root,
            me: "mac",
            boot: no_boot,
            external,
            force: false,
            dry: false,
        }
    }
    fn wt(&self, name: &str) -> bool {
        self.root.join(".worktrees").join(name).exists()
    }
    /// Everything a pass may change: registry, branches, HEAD, `origin/main`, each status and `.taskq`.
    fn state(&self) -> String {
        let mut s = [
            self.sh(&self.root, &["worktree", "list", "--porcelain"]),
            self.sh(&self.root, &["branch", "--list"]),
            self.head(),
            self.sh(&self.root, &["status", "--porcelain"]),
            format!("taskq {}", self.root.join(".taskq").exists()),
        ]
        .join("\n");
        for d in std::fs::read_dir(self.root.join(".worktrees"))
            .into_iter()
            .flatten()
            .flatten()
        {
            s += &format!(
                "\n{}: {:?}",
                d.file_name().to_string_lossy(),
                std::fs::read_dir(d.path())
                    .unwrap()
                    .flatten()
                    .map(|e| e.file_name())
                    .collect::<Vec<_>>()
            );
        }
        s
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn pass(t: &Tidy) -> Outcome {
    t.run(&mut |_| false, &|_| None)
}

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
    assert!(
        o.log
            .contains(&"root fast-forwarded 1 commits to origin/main".to_string()),
        "{:?}",
        o.log
    );
    assert_eq!(
        std::fs::read_to_string(r.root.join("local.txt")).unwrap(),
        "mine",
        "untracked file untouched"
    );
    assert_eq!(
        std::fs::read_to_string(r.root.join("a")).unwrap(),
        "edited",
        "unrelated modified file untouched"
    );
    assert!(
        pass(&r.tidy(&b, false)).log.is_empty(),
        "a pass that changes nothing logs nothing"
    );

    // overwrite conflict: the merge would overwrite a modified tracked file
    r.upstream("a", "2");
    let before = r.head();
    let o = pass(&r.tidy(&b, false));
    assert_eq!(
        (o.report["root"].as_str().unwrap(), r.head()),
        ("stale overwrite conflict (behind 1)", before.clone())
    );
    assert_eq!(
        std::fs::read_to_string(r.root.join("a")).unwrap(),
        "edited",
        "local change kept"
    );
    assert_eq!(r.sh(&r.root, &["stash", "list"]), "", "never stashes");
    r.sh(&r.root, &["checkout", "-q", "--", "a"]);

    // diverged: a local commit on main
    std::fs::write(r.root.join("c"), "local").unwrap();
    r.sh(&r.root, &["add", "c"]);
    r.sh(&r.root, &["commit", "-q", "-m", "local"]);
    let local = r.head();
    let o = pass(&r.tidy(&b, false));
    assert_eq!(
        (o.report["root"].as_str().unwrap(), r.head()),
        ("stale diverged (behind 1)", local.clone()),
        "never resets"
    );

    // another branch, then a detached HEAD
    r.sh(&r.root, &["checkout", "-q", "-b", "topic"]);
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale on branch topic (behind 1)"
    );
    r.sh(&r.root, &["checkout", "-q", "--detach"]);
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale detached HEAD (behind 1)"
    );
    assert_eq!(r.head(), local);

    // an operation in progress on main
    r.sh(&r.root, &["checkout", "-q", "main"]);
    std::fs::write(r.root.join(".git/MERGE_HEAD"), format!("{local}\n")).unwrap();
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale merge in progress (behind 1)"
    );
    std::fs::remove_file(r.root.join(".git/MERGE_HEAD")).unwrap();

    // a held Git lock skips the whole pass and says so; the report keeps the last root
    std::fs::write(r.root.join(".git/index.lock"), "").unwrap();
    let o = pass(&r.tidy(&b, false));
    assert_eq!(
        (
            o.report["outcome"].as_str().unwrap(),
            o.report["root"].as_str().unwrap()
        ),
        (
            "skipped Git lock index.lock held",
            "stale merge in progress (behind 1)"
        )
    );
    assert_eq!(o.log, ["tidy skipped: Git lock index.lock held"]);
    std::fs::remove_file(r.root.join(".git/index.lock")).unwrap();

    // a held `.taskq` lock skips too
    let held = coreq::file_lock::try_lock(&r.root.join(".taskq/tidy.lock")).unwrap();
    assert_eq!(
        pass(&r.tidy(&b, false)).report["outcome"],
        "skipped taskq lock held"
    );
    drop(held);
    assert_eq!(r.head(), local, "no refusal moved the root");
}

/// The PAH shape: `origin` defaults to `master` and the clone has no `refs/remotes/origin/HEAD`.
fn master_repo(tag: &str) -> Repo {
    let r = Repo::new(tag);
    r.sh(&r.origin, &["branch", "-m", "main", "master"]);
    r.sh(&r.root, &["fetch", "-q", "--prune", "origin"]);
    r.sh(&r.root, &["branch", "-q", "-m", "main", "master"]);
    r.sh(
        &r.root,
        &["branch", "-q", "--set-upstream-to=origin/master", "master"],
    );
    r.sh(&r.root, &["remote", "set-head", "origin", "--delete"]);
    r
}

/// Everything the task forbids a pass to change: refs, local Git config and the stash.
fn git_policy(r: &Repo) -> String {
    [
        r.sh(&r.root, &["for-each-ref", "--format=%(refname) %(symref)"]),
        r.sh(&r.root, &["config", "--local", "--list"]),
        r.sh(&r.root, &["stash", "list"]),
    ]
    .join("\n")
}

// Chain: tidy step 5 on a `master` repository. Defect (gradus/pah#3): the pass fetched `origin main`, so a current
// `master` root read `stale on branch master` and filed a false finding, also with `origin/HEAD` absent as on PAH.
// Risk protected: a fix that needs or writes `origin/HEAD`, or changes a branch, remote or tracking setting.
#[test]
fn master_default_stays_current_and_fast_forwards_without_origin_head() {
    let r = master_repo("master");
    let b = Fake::default();
    let policy = git_policy(&r);
    let o = pass(&r.tidy(&b, false));
    assert_eq!(o.report["root"], "current", "{:?}", o.log);
    assert!(
        !o.conditions.contains_key("stale"),
        "no false finding: {:?}",
        o.conditions
    );
    r.upstream("b", "1");
    std::fs::write(r.root.join("local.txt"), "mine").unwrap();
    let o = pass(&r.tidy(&b, false));
    assert_eq!(o.report["root"], "current", "{:?}", o.log);
    assert_eq!(o.log, ["root fast-forwarded 1 commits to origin/master"]);
    assert_eq!(r.head(), r.sh(&r.origin, &["rev-parse", "HEAD"]));
    assert_eq!(
        std::fs::read_to_string(r.root.join("local.txt")).unwrap(),
        "mine"
    );
    assert_eq!(git_policy(&r), policy, "no ref name, config or stash write");
    assert!(
        git(&r.root, &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"]).is_err(),
        "origin/HEAD stays absent"
    );
    // a recorded `origin/HEAD` left behind by a rename loses to what `origin` states now
    r.sh(
        &r.root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    assert_eq!(pass(&r.tidy(&b, false)).report["root"], "current");
}

// Chain: tidy step 5 refusals on a `master` repository. Risk protected: following the origin default weakens a
// protection of the `main` case, or moves a root that a person left on another branch.
#[test]
fn master_default_keeps_every_root_refusal() {
    let r = master_repo("master-refuse");
    let b = Fake::default();
    r.upstream("a", "2");
    std::fs::write(r.root.join("a"), "edited").unwrap();
    let before = r.head();
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale overwrite conflict (behind 1)"
    );
    assert_eq!(
        (
            r.head(),
            std::fs::read_to_string(r.root.join("a")).unwrap().as_str()
        ),
        (before.clone(), "edited"),
        "local change kept"
    );
    assert_eq!(r.sh(&r.root, &["stash", "list"]), "", "never stashes");
    r.sh(&r.root, &["checkout", "-q", "--", "a"]);
    std::fs::write(r.root.join("c"), "local").unwrap();
    r.sh(&r.root, &["add", "c"]);
    r.sh(&r.root, &["commit", "-q", "-m", "local"]);
    let local = r.head();
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale diverged (behind 1)"
    );
    // a local `main` is another branch here: it is neither followed nor created
    r.sh(&r.root, &["checkout", "-q", "-b", "main"]);
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale on branch main (behind 1)"
    );
    r.sh(&r.root, &["checkout", "-q", "-b", "topic"]);
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale on branch topic (behind 1)"
    );
    r.sh(&r.root, &["checkout", "-q", "--detach"]);
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale detached HEAD (behind 1)"
    );
    r.sh(&r.root, &["checkout", "-q", "master"]);
    std::fs::write(r.root.join(".git/MERGE_HEAD"), format!("{local}\n")).unwrap();
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale merge in progress (behind 1)"
    );
    std::fs::remove_file(r.root.join(".git/MERGE_HEAD")).unwrap();
    std::fs::write(r.root.join(".git/index.lock"), "").unwrap();
    assert_eq!(
        pass(&r.tidy(&b, false)).report["outcome"],
        "skipped Git lock index.lock held"
    );
    std::fs::remove_file(r.root.join(".git/index.lock")).unwrap();
    assert_eq!(r.head(), local, "no refusal moved the root");
}

// Chain: default resolution -> root step and branch step. Risk protected: guessing the default from the checked-out
// branch or the name `main`, and `branch -D` against a branch the remote never named as its default.
#[test]
fn unknown_default_refuses_and_never_reads_the_checkout() {
    let r = master_repo("unknown");
    let b = Fake::default();
    b.create("T", "", &["taskq".into()]).unwrap();
    // merged only into `origin/master`: without a verified default its tip counts as unmerged
    r.sh(
        &r.root,
        &["branch", "--no-track", "taskq-1", "origin/master"],
    );
    b.close(1).unwrap();
    std::fs::write(r.root.join("c"), "local").unwrap();
    r.sh(&r.root, &["add", "c"]);
    r.sh(&r.root, &["commit", "-q", "-m", "local"]);
    r.sh(&r.root, &["checkout", "-q", "--orphan", "topic"]);
    r.sh(&r.root, &["commit", "-q", "-m", "topic"]);
    r.sh(&r.origin, &["checkout", "-q", "--detach"]);
    let head = r.head();
    let o = pass(&r.tidy(&b, false));
    assert_eq!(
        o.report["root"], "stale default branch unknown",
        "{:?}",
        o.lines
    );
    assert!(
        o.conditions["stale"] == ["root stale default branch unknown"],
        "{:?}",
        o.conditions
    );
    assert_eq!(o.log, ["#1 branch kept: not merged into the default branch: error: the branch 'taskq-1' is not fully merged", "root stale: default branch unknown"].map(String::from));
    assert_eq!(r.head(), head);
    assert_eq!(r.sh(&r.root, &["branch", "--list", "taskq-1"]), "taskq-1");
    // an unreachable origin with no recorded default refuses the same way, here on a branch named like the default
    r.sh(&r.root, &["checkout", "-q", "master"]);
    r.sh(
        &r.root,
        &[
            "remote",
            "set-url",
            "origin",
            r.base.join("gone").to_str().unwrap(),
        ],
    );
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "stale default branch unknown"
    );
    // a recorded default keeps branch cleanup while unreachable; the root does not move on an unverified read
    r.sh(
        &r.root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/master",
        ],
    );
    let o = pass(&r.tidy(&b, false));
    assert_eq!(o.report["root"], "stale fetch failed", "{:?}", o.log);
    assert_eq!(
        r.sh(&r.root, &["branch", "--list", "taskq-1"]),
        "",
        "{:?}",
        o.log
    );
}

// Chain: `taskq cleanup --dry-run` and step 4 on a `master` repository. Risk protected: a dry run that fetches or
// writes, and branch cleanup on a `master` repository that deletes unmerged work or a foreign worktree's branch.
#[test]
fn master_default_dry_run_and_branch_cleanup_keep_their_protections() {
    let r = master_repo("master-dry");
    let b = Fake::default();
    for _ in 0..7 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    // 5 merged into origin/master; 6 unmerged; 7 merged but checked out in a foreign worktree
    r.sh(&r.root, &["branch", "taskq-5", "origin/master"]);
    r.sh(&r.root, &["checkout", "-q", "-b", "taskq-6"]);
    std::fs::write(r.root.join("u"), "unmerged").unwrap();
    r.sh(&r.root, &["add", "u"]);
    r.sh(&r.root, &["commit", "-q", "-m", "unmerged"]);
    r.sh(&r.root, &["checkout", "-q", "master"]);
    let foreign = r.base.join("foreign");
    r.sh(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "taskq-7",
            foreign.to_str().unwrap(),
        ],
    );
    for n in [5, 6, 7] {
        b.close(n).unwrap();
    }
    r.upstream("b", "1");
    r.sh(&r.root, &["fetch", "-q", "origin", "master"]);
    r.upstream("c", "1");
    std::fs::remove_file(r.root.join(".git/FETCH_HEAD")).unwrap();
    let (state, policy, tracked) = (
        r.state(),
        git_policy(&r),
        r.sh(&r.root, &["rev-parse", "origin/master"]),
    );
    let o = Tidy {
        dry: true,
        ..r.tidy(&b, false)
    }
    .run(&mut |_| false, &|_| None);
    assert_eq!(
        o.lines,
        [
            "#5 branch would be deleted",
            "#6 branch kept: not merged into origin/master",
            "root would fast-forward 1 commits to origin/master"
        ]
    );
    assert_eq!(
        (
            r.state(),
            git_policy(&r),
            r.sh(&r.root, &["rev-parse", "origin/master"])
        ),
        (state, policy, tracked),
        "a dry run changes nothing"
    );
    assert!(
        !r.root.join(".git/FETCH_HEAD").exists(),
        "a dry run does not fetch"
    );
    let o = pass(&r.tidy(&b, false));
    let branch = |b: &str| r.sh(&r.root, &["branch", "--list", b]) != "";
    assert!(
        !branch("taskq-5") && branch("taskq-6") && branch("taskq-7") && foreign.exists(),
        "{:?}",
        o.log
    );
    assert!(o.log.contains(&"#6 branch kept: not merged into origin/master: error: the branch 'taskq-6' is not fully merged".into()), "{:?}", o.log);
    assert_eq!(o.report["root"], "current", "{:?}", o.log);
    assert_eq!(o.counts["root_moved"], 1);
}

// Chain: tidy steps 2 and 4 on a temp repo. Defect: reviewer worktrees and `taskq-*` branches of closed tasks
// that no code removed (Mac survey). Risk protected: removing a dirty, open-stage or foreign worktree,
// or deleting an unmerged branch.
#[test]
fn task_worktrees_and_branches_leave_with_their_stage_only() {
    let r = Repo::new("task");
    let b = Fake::default();
    for _ in 0..6 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    let add = |args: &[&str]| {
        r.sh(&r.root, &[&["worktree", "add", "-q"], args].concat());
    };
    add(&["-b", "taskq-1", ".worktrees/taskq-1"]);
    add(&["-b", "taskq-2", ".worktrees/taskq-2"]);
    add(&["--detach", ".worktrees/review-3"]);
    add(&["--detach", ".worktrees/review-4-6177396825"]);
    let foreign = r.base.join("foreign");
    r.sh(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            foreign.to_str().unwrap(),
        ],
    );
    std::fs::write(r.root.join(".worktrees/taskq-2/wip"), "x").unwrap();
    std::fs::write(foreign.join("wip"), "x").unwrap();
    // closed tasks' branches: 5 merged into origin/main, 6 holds an unmerged commit
    r.sh(&r.root, &["branch", "taskq-5", "origin/main"]);
    r.sh(&r.root, &["checkout", "-q", "-b", "taskq-6"]);
    std::fs::write(r.root.join("u"), "unmerged").unwrap();
    r.sh(&r.root, &["add", "u"]);
    r.sh(&r.root, &["commit", "-q", "-m", "unmerged"]);
    r.sh(&r.root, &["checkout", "-q", "main"]);
    for n in [1, 2, 3, 5, 6] {
        b.close(n).unwrap();
    }
    // #4 is open and still in review: its reviewer worktree stays
    let known = |n: u64| (n == 4).then_some(State::Review);
    let o = r.tidy(&b, false).run(&mut |_| false, &known);
    assert_eq!(
        (
            r.wt("taskq-1"),
            r.wt("taskq-2"),
            r.wt("review-3"),
            r.wt("review-4-6177396825"),
            foreign.exists()
        ),
        (false, true, false, true, true),
        "{:?}",
        o.log
    );
    assert!(
        o.log.contains(
            &"#2 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`"
                .into()
        ),
        "{:?}",
        o.log
    );
    assert!(
        o.log.contains(&"#1 worktree removed".into())
            && o.log.contains(&"#3 review worktree removed".into()),
        "{:?}",
        o.log
    );
    let branch = |b: &str| r.sh(&r.root, &["branch", "--list", b]) != "";
    assert!(
        !branch("taskq-1") && !branch("taskq-5"),
        "merged branches without a worktree go: {:?}",
        o.log
    );
    assert!(branch("taskq-6") && o.log.contains(&"#6 branch kept: not merged into origin/main: error: the branch 'taskq-6' is not fully merged".into()), "unmerged stays: {:?}", o.log);
    assert!(branch("taskq-2"), "a branch with a worktree stays");
    assert_eq!(
        (
            o.report["task"]["count"].as_u64(),
            o.report["task"]["dirty"].as_u64()
        ),
        (Some(2), Some(1))
    );
    assert_eq!(
        (
            o.report["foreign"]["count"].as_u64(),
            o.report["foreign"]["dirty"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert_eq!(
        (
            o.counts["removed"],
            o.counts["refused"],
            o.counts["branches_deleted"]
        ),
        (2, 2, 2)
    );

    // #4 leaves review: its reviewer worktree goes; a live child keeps a worktree whatever the board says
    let o = r
        .tidy(&b, false)
        .run(&mut |n| n == 4, &|n| (n == 4).then_some(State::Doing));
    assert!(
        r.wt("review-4-6177396825"),
        "a live child keeps it: {:?}",
        o.log
    );
    r.tidy(&b, false)
        .run(&mut |_| false, &|n| (n == 4).then_some(State::Doing));
    assert!(!r.wt("review-4-6177396825"));

    // #2060 (Mac cs, 55 closed-task worktrees): `workspace: external` still tidies TaskQ's own task worktrees and
    // `taskq-<N>` branches; foreign entries stay, the unmerged branch too
    std::fs::remove_file(r.root.join(".worktrees/taskq-2/wip")).unwrap();
    let o = pass(&r.tidy(&b, true));
    assert!(
        !r.wt("taskq-2") && foreign.exists() && branch("taskq-6"),
        "{:?}",
        o.log
    );
    assert!(o.log.contains(&"#2 worktree removed".into()), "{:?}", o.log);
    // a squash merge: the close receipt records the merge of the task's result at the branch tip
    let tip = r.sh(&r.root, &["rev-parse", "taskq-6"]);
    let attempt = b
        .comment(6, "taskq: spawn worker claude:owned other")
        .unwrap();
    b.comment(6, &core::entry("result", &[&tip], Some(attempt), ""))
        .unwrap();
    b.comment(
        6,
        &core::entry(
            "close",
            &[],
            Some(attempt),
            "ok\ndelivery auto-merge r0: merged #9 as abcdef1; merge-result CI green",
        ),
    )
    .unwrap();
    let o = pass(&r.tidy(&b, false));
    assert!(!branch("taskq-6") && !r.wt("taskq-2"), "{:?}", o.log);

    // Defect (live Windows pass): an empty leftover directory reported its age since 1970. It stays, aged by its own time.
    std::fs::create_dir_all(r.root.join(".worktrees/taskq-8")).unwrap();
    let o = pass(&r.tidy(&b, false));
    assert!(
        r.wt("taskq-8")
            && o.report["task"]["count"] == 1
            && o.report["task"]["oldest_s"].as_u64() < Some(86400),
        "{}",
        o.report
    );
}

// Chain: tidy step 2 removal on Windows. Defect (PM steer, win 4.6.35): clean closed-task worktrees stayed and
// `cleanup.git` failed 163 of 308 calls with one log line and no cause. Git for Windows fails `worktree remove` at
// 260 characters (`Filename too long`) on deep ignored `target/` paths and drops the registry entry. Risk protected:
// a refusal line without Git's reason, and a long ignored path that keeps a closed task's worktree.
#[test]
fn removal_handles_long_ignored_paths_and_names_git_refusals() {
    let r = Repo::new("long");
    let b = Fake::default();
    for _ in 0..2 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    std::fs::write(
        r.root.join(".git/info/exclude"),
        "target/
",
    )
    .unwrap();
    for n in [1, 2] {
        r.sh(
            &r.root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &format!("taskq-{n}"),
                &format!(".worktrees/taskq-{n}"),
            ],
        );
    }
    let deep = (0..12).fold(
        r.root.join(".worktrees/taskq-1/target/debug/build"),
        |p, i| p.join(format!("aaaaaaaaaaaaaaaaaaaaaaaaaa{i}")),
    );
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("f.rlib"), "x").unwrap();
    assert!(deep.join("f.rlib").as_os_str().len() > 300);
    for n in [1, 2] {
        b.close(n).unwrap();
    }
    // work written between the status read and the removal: Git refuses, and the line says why
    let wip = r.root.join(".worktrees/taskq-2/wip");
    let o = r.tidy(&b, false).run(
        &mut |n| {
            if n == 2 {
                let _ = std::fs::write(&wip, "x");
            }
            false
        },
        &|_| None,
    );
    assert!(
        !r.wt("taskq-1") && o.log.contains(&"#1 worktree removed".into()),
        "long ignored paths go: {:?}",
        o.log
    );
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
    for _ in 0..5 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    let lib = r.base.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    r.sh(&lib, &["init", "-q", "-b", "main"]);
    std::fs::write(lib.join("l"), "1").unwrap();
    r.sh(&lib, &["add", "l"]);
    r.sh(&lib, &["commit", "-q", "-m", "lib"]);
    for n in 1..=5 {
        let wt = r.root.join(format!(".worktrees/taskq-{n}"));
        r.sh(
            &r.root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &format!("taskq-{n}"),
                wt.to_str().unwrap(),
            ],
        );
        r.sh(
            &wt,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                lib.to_str().unwrap(),
                "sub",
            ],
        );
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
    for n in 1..=5 {
        b.close(n).unwrap();
    }
    let o = pass(&r.tidy(&b, false));
    assert!(
        !r.wt("taskq-1") && o.log.contains(&"#1 worktree removed".into()),
        "a clean one goes: {:?}",
        o.log
    );
    assert_eq!(
        r.sh(&r.root, &["branch", "--list", "taskq-1"]),
        "taskq-1",
        "its unmerged branch stays"
    );
    for (n, line) in [
        (
            2,
            "#2 worktree kept: uncommitted changes in submodule sub; inspect them before `taskq cleanup --force`",
        ),
        (
            3,
            "#3 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`",
        ),
        (
            4,
            "#4 worktree kept: submodule sub has commits on no remote; push them first",
        ),
        (
            5,
            "#5 worktree kept: uncommitted changes in submodule sub; inspect them before `taskq cleanup --force`",
        ),
    ] {
        assert!(
            r.wt(&format!("taskq-{n}")) && o.log.contains(&line.into()),
            "#{n}: {:?}",
            o.log
        );
    }
    assert_eq!(std::fs::read_to_string(wt(2).join("sub/wip")).unwrap(), "x");
    // `--force` saves the submodule's patch first; commits on no remote still keep #4
    let o = Tidy {
        force: true,
        ..r.tidy(&b, false)
    }
    .run(&mut |_| false, &|_| None);
    assert!(!r.wt("taskq-5") && r.wt("taskq-4"), "{:?}", o.log);
    assert!(
        std::fs::read_to_string(r.root.join(".taskq/trash/taskq-5--sub.patch"))
            .unwrap()
            .contains("+mine"),
        "{:?}",
        o.log
    );
}

// Chain: `taskq cleanup --dry-run [--force]` on a temp repo. Defect (#2060): cleanup had no preview, and `--force`
// removed dirty worktrees without a list. Risk protected: a dry run that fetches, prunes, removes, deletes a branch,
// moves the root or writes `.taskq`, and a line that differs from what the real pass then does.
#[test]
fn dry_run_prints_every_line_and_changes_nothing() {
    let r = Repo::new("dry");
    let b = Fake::default();
    for _ in 0..5 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    for w in ["taskq-1", "taskq-2", "taskq-9"] {
        r.sh(
            &r.root,
            &["worktree", "add", "-q", "-b", w, &format!(".worktrees/{w}")],
        );
    }
    r.sh(
        &r.root,
        &["worktree", "add", "-q", "--detach", ".worktrees/review-3"],
    );
    std::fs::remove_dir_all(r.root.join(".worktrees/taskq-9")).unwrap();
    std::fs::write(r.root.join(".worktrees/taskq-2/wip"), "x").unwrap();
    r.sh(&r.root, &["branch", "taskq-5", "origin/main"]);
    for n in [1, 2, 5] {
        b.close(n).unwrap();
    }
    r.upstream("b", "1");
    r.sh(&r.root, &["fetch", "-q", "origin", "main"]);
    let known = |n: u64| (n == 3).then_some(State::Review);
    let before = r.state();
    let o = Tidy {
        dry: true,
        ..r.tidy(&b, false)
    }
    .run(&mut |_| false, &known);
    assert_eq!(
        o.lines,
        [
            "tidy would prune 1 worktree registry entries without a directory",
            "#3 review worktree kept: task in review",
            "#1 worktree would be removed",
            "#2 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`",
            "#5 branch would be deleted",
            "root would fast-forward 1 commits to origin/main"
        ]
    );
    let o = Tidy {
        dry: true,
        force: true,
        ..r.tidy(&b, false)
    }
    .run(&mut |_| false, &known);
    assert!(
        o.lines
            .contains(&"#2 worktree would be removed after saving a patch to .taskq/trash".into()),
        "{:?}",
        o.lines
    );
    assert_eq!(
        r.state(),
        before,
        "tree, registry, branches, root and .taskq unchanged"
    );
    // the real pass does what the preview said
    let o = r.tidy(&b, false).run(&mut |_| false, &known);
    assert!(
        !r.wt("taskq-1") && r.wt("taskq-2") && r.wt("review-3") && o.report["root"] == "current",
        "{:?}",
        o.lines
    );
    assert_eq!(r.sh(&r.root, &["branch", "--list", "taskq-5"]), "");
}

// Chain: tidy step 2 on directories outside the registry. Defect (#2060, Windows taskq): review-1594, taskq-948 and
// others were empty leftovers without `.git`, so Git read the root's 1336 changes there, and the pass skipped them
// with no line. Risk protected: deleting a non-empty unregistered directory, and a kept task worktree with no reason.
#[test]
fn unregistered_task_directories_and_kept_reasons() {
    let r = Repo::new("unreg");
    let b = Fake::default();
    for _ in 0..4 {
        b.create("T", "", &["taskq".into()]).unwrap();
    }
    std::fs::create_dir_all(r.root.join(".worktrees/review-1")).unwrap();
    std::fs::create_dir_all(r.root.join(".worktrees/taskq-2")).unwrap();
    std::fs::write(r.root.join(".worktrees/taskq-2/notes"), "x").unwrap();
    for w in ["taskq-3", "taskq-4"] {
        r.sh(
            &r.root,
            &["worktree", "add", "-q", "-b", w, &format!(".worktrees/{w}")],
        );
    }
    std::fs::write(r.root.join(".worktrees/taskq-4/wip"), "x").unwrap();
    for n in [1, 2, 4] {
        b.close(n).unwrap();
    }
    let o = r
        .tidy(&b, false)
        .run(&mut |_| false, &|n| (n == 3).then_some(State::Doing));
    assert!(
        !r.wt("review-1") && r.wt("taskq-2") && r.wt("taskq-3") && r.wt("taskq-4"),
        "{:?}",
        o.lines
    );
    let kept2 = "#2 worktree kept: not a registered worktree; inspect it and delete it by hand";
    let kept4 =
        "#4 worktree kept: uncommitted changes; inspect them before `taskq cleanup --force`";
    assert_eq!(
        o.lines,
        [
            "#1 review worktree removed: empty directory, not a registered worktree",
            kept2,
            "#3 worktree kept: task open (doing)",
            kept4
        ]
    );
    assert!(
        !o.log.iter().any(|l| l.contains("task open")),
        "the service log keeps only removals and refusals: {:?}",
        o.log
    );
    assert_eq!(o.conditions["kept"], [kept2, kept4]);
    assert!(
        o.conditions["dirty"].len() == 1
            && o.conditions["dirty"][0].starts_with("#4 worktree dirty, untouched "),
        "{:?}",
        o.conditions
    );
    // a live child keeps it with its own line, and no finding condition
    let o = r
        .tidy(&b, false)
        .run(&mut |n| n == 4, &|n| (n == 3).then_some(State::Doing));
    assert!(
        o.lines
            .contains(&"#4 worktree kept: running child on this host".into())
            && !o.conditions.contains_key("dirty"),
        "{:?}",
        o.lines
    );
}

// Chain: tidy on a root Git calls dubious ownership. Defect (#2060, Windows coreq, dashq, desiign, serverq, specq):
// `tidy skipped: git unreadable: fatal: detected dubious ownership in repository`. Risk protected: trusting any path
// besides the root and its `.worktrees` (no `*`, no global config write).
#[test]
fn dubious_ownership_root_is_tidied() {
    let r = Repo::new("owner");
    let b = Fake::default();
    b.create("T", "", &["taskq".into()]).unwrap();
    r.sh(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "taskq-1",
            ".worktrees/taskq-1",
        ],
    );
    let foreign = r.base.join("foreign");
    r.sh(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            foreign.to_str().unwrap(),
        ],
    );
    b.close(1).unwrap();
    r.upstream("b", "1");
    TEST_ENV.with_borrow_mut(|e| {
        *e = vec![
            ("GIT_TEST_ASSUME_DIFFERENT_OWNER".into(), "1".into()),
            ("GIT_ALLOW_PROTOCOL".into(), "https".into()),
        ]
    });
    match git(&r.root, &["status"]) {
        Err(e) => assert!(
            e.contains("dubious ownership"),
            "outside a pass Git refuses the root: {e}"
        ),
        // Git before 2.45 honours the test owner switch only on Windows (WSL git 2.43): nothing to simulate
        Ok(_) => {
            TEST_ENV.with_borrow_mut(Vec::clear);
            eprintln!("skipped: this Git cannot simulate another owner");
            return;
        }
    }
    let o = pass(&r.tidy(&b, false));
    TEST_ENV.with_borrow_mut(Vec::clear);
    // Refuse the fixture's file transport explicitly: Git versions differ in upload-pack ownership checks.
    // All root/worktree trust and foreign-status assertions below remain unchanged.
    assert!(
        !r.wt("taskq-1") && o.report["root"] == "stale fetch failed",
        "{:?} {}",
        o.lines,
        o.report
    );
    assert_eq!(
        o.log,
        [
            "#1 worktree removed",
            "#1 branch deleted",
            "root stale: fetch failed"
        ]
    );
    assert_eq!(
        pass(&r.tidy(&b, false)).report["root"],
        "current",
        "the same root reaches origin/main once Git trusts its owner"
    );
    // the clean foreign checkout reads dirty: Git still refused its status, so it stayed untrusted
    assert_eq!(
        (
            o.report["foreign"]["count"].as_u64(),
            o.report["foreign"]["dirty"].as_u64()
        ),
        (Some(1), Some(1))
    );
}

// Chain: tidy step 3. Risk protected: removing recent or dirty unowned work.
#[test]
fn unowned_worktrees_leave_only_clean_and_a_week_old() {
    let r = Repo::new("unowned");
    let b = Fake::default();
    r.sh(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "scratch",
            ".worktrees/scratch",
        ],
    );
    r.sh(
        &r.root,
        &["worktree", "add", "-q", "--detach", ".worktrees/fresh"],
    );
    assert!(
        pass(&r.tidy(&b, false)).log.is_empty() && r.wt("scratch"),
        "recent stays"
    );
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86400);
    let age = |dir: &Path| {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                }
                let _ = std::fs::File::options()
                    .write(true)
                    .open(e.path())
                    .and_then(|f| f.set_modified(old));
            }
        }
    };
    // HEAD commit is recent: still kept
    age(&r.root.join(".worktrees/scratch"));
    assert!(pass(&r.tidy(&b, false)).log.is_empty() && r.wt("scratch"));
    // an old commit and old files: removed, branch kept
    let date = "2000-01-01T00:00:00";
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(r.root.join(".worktrees/scratch"))
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "old",
        ])
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_AUTHOR_DATE", date)
        .output()
        .unwrap();
    assert!(o.status.success());
    age(&r.root.join(".worktrees/scratch"));
    std::fs::write(r.root.join(".worktrees/fresh/wip"), "x").unwrap();
    // #2060: `workspace: external` keeps every other path host-owned, old and clean ones too
    assert!(pass(&r.tidy(&b, true)).log.is_empty() && r.wt("scratch"));
    let o = pass(&r.tidy(&b, false));
    assert!(!r.wt("scratch") && r.wt("fresh"), "{:?}", o.log);
    assert!(
        o.log[0].starts_with(".worktrees/scratch removed: unowned, untouched "),
        "{:?}",
        o.log
    );
    assert_eq!(
        r.sh(&r.root, &["branch", "--list", "scratch"]),
        "scratch",
        "branch kept"
    );
}

// Chain: registry paths on Windows. Risk: a verbatim, backslash, MSYS or case variant of the root reads foreign
// (never tidied) or a foreign path reads owned (removed).
#[test]
fn windows_path_spellings_compare_alike() {
    let root = norm(r"\\?\C:\Users\me\Codex\taskq", true);
    assert_eq!(root, "c:/users/me/codex/taskq");
    assert_eq!(place(&root, &norm("C:/Users/me/Codex/taskq", true)), None);
    assert_eq!(
        place(
            &root,
            &norm("C:/Users/Me/Codex/taskq/.worktrees/taskq-7", true)
        ),
        Some(Some("taskq-7".into()))
    );
    assert_eq!(
        place(
            &root,
            &norm(r"c:\users\me\codex\taskq\.worktrees\review-7\", true)
        ),
        Some(Some("review-7".into()))
    );
    assert_eq!(
        place(
            &root,
            &norm("/c/Users/me/Codex/taskq/.worktrees/dev-build", true)
        ),
        Some(Some("dev-build".into()))
    );
    assert_eq!(
        place(
            &root,
            &norm(r"C:\Users\me\AppData\Local\Temp\taskq-r1073-1", true)
        ),
        Some(None),
        "temp dir is foreign"
    );
    assert_eq!(
        place(&root, &norm("C:/Users/me/Codex/taskq/.worktrees/a/b", true)),
        Some(None),
        "nested is not a .worktrees entry"
    );
    assert_eq!(
        place(
            &root,
            &norm("C:/Users/me/Codex/taskq-other/.worktrees/taskq-7", true)
        ),
        Some(None)
    );
    assert_eq!(
        norm(r"\\?\UNC\server\share\taskq", true),
        "//server/share/taskq"
    );
    assert_eq!(
        norm("/Users/Me/taskq/", false),
        "/Users/Me/taskq",
        "case kept off Windows"
    );
    assert_eq!(
        [
            class("taskq-12"),
            class("review-12"),
            class("review-12-6177396825"),
            class("taskq-+1"),
            class("review-1-x")
        ],
        [
            Class::Worker(12),
            Class::Review(12),
            Class::Review(12),
            Class::Unowned,
            Class::Unowned
        ]
    );
}

#[test]
fn health_line_reads_the_tidy_report() {
    let t = json!({"at": 1000, "outcome": "ok", "root": "behind 3", "task": {"count": 2, "dirty": 1, "bytes": 3u64 << 30}, "reserved": {"count": 1, "dirty": 0, "bytes": 0},
            "unowned": {"count": 0, "dirty": 0, "bytes": 0}, "foreign": {"count": 4, "dirty": 2, "bytes": 5}});
    assert_eq!(
        health_line("taskq", &t, 1000 + 7200),
        "taskq: root behind 3; .worktrees 2 (dirty 1, 3.0 GiB); foreign 4 (dirty 2); tidy 2h ok"
    );
    assert_eq!(
        health_line("csgo", &Value::Null, 0),
        "csgo: tidy unknown (no report yet)"
    );
}
