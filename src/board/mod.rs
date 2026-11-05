//! Board contract (taskq.md § 1, § 2, § 9): the `Board` trait, shared HTTP through the OS `curl`, the registry of
//! board modules (`github`, `gitlab`) and an in-memory board for tests. Every write is a native board object; nothing else is state.

use crate::core::Entry;
use serde_json::{Value, json};
use std::cell::RefCell;

pub type Result<T> = std::result::Result<T, String>;
/// Closed tasks per Completed page; each costs one comment read (§ 4 budget).
pub const CLOSED_PAGE: usize = 10;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub updated: i64,
    /// Board creation time: the entry time of the first state, ready (§ 1)
    pub created: i64,
    /// Board closure time; absent is unknown, never the last edit time.
    pub closed: Option<i64>,
    pub open: bool,
    pub trusted: bool,
    /// Why a closed issue closed, when the board records it (GitHub `state_reason`): `completed`, `not_planned`, ...
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Comment {
    pub id: u64,
    pub created: i64,
    pub body: String,
    pub trusted: bool,
    pub edited: bool,
    /// The writer's board login; empty when the board does not say (§ 5 `answered`).
    pub author: String,
    /// § 2 owner-note read receipt: whether the comment carries the `eyes` reaction; None when the board does not say.
    pub read: Option<bool>,
}

pub trait Board {
    fn canonical_labels(&self) -> Result<bool> { Ok(true) }
    fn state_label(&self, state: &str) -> Result<String> {
        Ok(format!("q-{}", if state == "waiting" && self.canonical_labels()? { "blocked" } else { state }))
    }
    fn ensure_labels(&self, _names: &[String]) -> Result<()> { Ok(()) }
    /// Open issues; with `since` (board seconds) every issue, closed too, updated at or after it.
    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>>;
    /// #2083: `taskq` issues, closed too, updated at or after `since` (board seconds); heartbeat writes never change it.
    fn tasks(&self, since: i64) -> Result<Vec<Issue>> {
        Ok(self.issues(Some(since))?.into_iter().filter(|i| i.labels.iter().any(|l| l == crate::service::TASK_LABEL)).collect())
    }
    /// #2083: the board time of the open list the last `issues(None)` reused instead of reading; None when it read in full.
    fn kept(&self) -> Option<i64> { None }
    fn issue(&self, n: u64) -> Result<Issue>;
    /// One page (from 1) of closed `taskq` issues, most recently updated first, with each close time and whether another
    /// page follows: the Completed view reads board history on demand, never a local task store (MP14).
    fn closed_tasks(&self, _page: u32, _since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> { Err("closed task history unsupported on this board".into()) }
    /// § 2 Deploy: every closed `taskq` issue with `label`, with its close time; demand reads only.
    fn closed_labelled(&self, _label: &str) -> Result<Vec<(Issue, i64)>> { Err("closed label lists unsupported on this board".into()) }
    /// Repository text at an immutable accepted result SHA; never the local checkout or moving main.
    fn source(&self, _path: &str, _sha: &str) -> Result<String> { Err("report source unsupported on this board".into()) }
    /// I3: earliest recorded addition of `taskq`, including removed labels; None proves no recorded admission.
    fn first_admission(&self, _n: u64) -> Result<Option<(u64, i64)>> { Err("first-admission history unsupported".into()) }
    fn comments(&self, n: u64) -> Result<Vec<Comment>>;
    fn comment(&self, n: u64, body: &str) -> Result<u64>;
    /// Edits comment `id` of issue `n`: an edited entry is ignored by `fold` (§ 2).
    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()>;
    /// § 2 read receipt: adds the `eyes` reaction to comment `id` of issue `n`, which leaves the comment unedited.
    /// `ponytail:` GitLab awards are not qualified to leave `updated_at` alone, so it stays unsupported there.
    fn react_read(&self, _n: u64, _id: u64) -> Result<()> { Err("read receipts unsupported on this board".into()) }
    /// The web URL of comment `id` of issue `n`.
    fn comment_url(&self, n: u64, id: u64) -> String { format!("{}#issuecomment-{id}", self.url(n)) }
    /// Whether writers keep the § 2 summary comment; in-memory test boards opt in.
    fn summaries(&self) -> bool { true }
    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()>;
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64>;
    fn edit(&self, n: u64, body: &str) -> Result<()>;
    fn close(&self, n: u64) -> Result<()>;
    /// § 4 step 5: reopens a task closed during a worker attempt.
    fn reopen(&self, _n: u64) -> Result<()> { Err("reopen unsupported on this board".into()) }
    /// The board's clock in seconds; leases never use local clocks (§ 4).
    fn now(&self) -> Result<i64>;
    fn me(&self) -> Result<String>;
    /// § 7 publication of task `n` at `sha` into `main`; `mode` is `direct` or `pr`.
    fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Publish;
    /// § 7: durable intent ends the reviewer; only the service performs asynchronous publication.
    fn defer_publication(&self, _mode: &str) -> bool { false }
    /// Advisory facts retained by the publication readback, never a separate status poll.
    fn take_publication_evidence(&self) -> Vec<crate::readiness::Publication> { vec![] }
    fn cancel_publication(&self, _n: u64, _sha: &str) -> Result<()> { Ok(()) }
    /// § 7: a task that closed without merging closes its open `taskq-<n>` PRs unmerged, branch kept; returns them.
    fn close_task_prs(&self, _n: u64, _reason: &str) -> Result<Vec<u64>> { Ok(vec![]) }
    /// Service retry of a saved `publish` in the configured `mode` (§ 7).
    fn resume_publication(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str, _root: &std::path::Path, _enqueued: bool) -> Publish {
        self.publish(n, sha, mode, title, task_line)
    }
    fn url(&self, n: u64) -> String;
    /// Issue `n` of another repository on this board's host, for `Depends on <repo>#N` (§ 1); a bare name is under
    /// this board's owner.
    fn foreign(&self, repo: &str, _n: u64) -> Result<Issue> {
        Err(format!("{repo}: other repositories unsupported on this board"))
    }
    fn foreign_url(&self, repo: &str, n: u64) -> String {
        format!("{repo}#{n}")
    }
    /// Request accounting and rate-limit pause of this process (§ 4); none for boards without HTTP.
    fn requests(&self) -> crate::requests::Budget { Default::default() }
    /// The budget for `Board quota:` (#2084): [`Board::requests`] after a free read when no reply carried the core budget.
    fn quota(&self) -> crate::requests::Budget { self.requests() }
    /// #1819: drop shortcuts over unchanged board reads; the next list reads the board in full.
    fn forget(&self) {}
    /// § 8 item 1 dev install: Ok only when `sha`'s `merge_group` CI passed; pending, failed or unknown is Err.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn merge_ci_green(&self, sha: &str) -> Result<()> { Err(format!("merge_group CI unknown on {sha}")) }
    /// #1935 `selftest`: why publication route `mode` cannot finish on this repository's plan; None when no gap is found.
    fn delivery_gap(&self, _mode: &str) -> Result<Option<String>> { Ok(None) }
    /// § 2 visual evidence: stores the image file `path` for task `n` on the board and returns its absolute https URL.
    fn upload_file(&self, _n: u64, _path: &std::path::Path) -> Result<String> { Err("image upload unsupported on this board".into()) }
    /// § 2 evidence files (#2019): stores a zip, tar.gz, PDF or HTML file `path` for task `n` and returns the absolute
    /// URL that downloads it; [`Board::file`] reads it back. Boards without a separate store keep it like an image.
    fn upload_attachment(&self, n: u64, path: &std::path::Path) -> Result<String> { self.upload_file(n, path) }
    /// § 2 evidence files (#2019): false when the board serves an uploaded HTML file as text, so one click shows its
    /// source; `--file` then uploads the page inside a zip, which downloads.
    fn html_downloads(&self) -> bool { true }
    /// The bytes behind an image URL this board serves, read with the board token: the load check and the app's copy.
    fn file(&self, url: &str) -> Result<Vec<u8>> { Err(format!("image unknown: {url} is not served by this board")) }
    /// § 2 visual evidence (#1844): true when `![name](url)` of an uploaded video plays inline in a comment; else a link.
    fn video_inline(&self) -> bool { false }
    /// § 6 setup check `pr-route`: whether `main` can merge a PR result (§ 7); unknown where unsupported.
    fn pr_route(&self) -> crate::check::Probe { crate::check::Probe::Unknown("PR route check unsupported on this board".into()) }
    /// #2194 doctor `token-*`: one read per permission taskq needs (`Issues`, `Contents`, `Pull requests`); a gap
    /// carries the HTTP status only. Empty where unsupported.
    fn access(&self) -> Vec<(&'static str, crate::check::Probe)> { vec![] }
}

/// #1935: a PR wait only a person can end (no queue or protection on the repository's plan); one board comment names it.
pub const MANUAL_MERGE: &str = "needs a manual merge";

#[derive(Debug, PartialEq)]
pub enum Publish {
    /// Merged or already on `main`: close.
    Done(String),
    /// CI pending or missing: refuse, state unchanged.
    Pending(String),
    /// Board CI still runs on a mergeable result: save `publish`, the service merges when it ends (#1939).
    Waiting(String),
    /// CI failed or conflict: requeue with this message.
    Failed(String),
    /// Queue admission read back; save its id on the board.
    Enqueued(String),
    /// Private fallback pushed a new head; save it before checking CI.
    Rebased(String),
    /// Main/queue conflict: requeue without consuming the worker retry budget.
    MainMoved(String),
    /// Only the owner can unblock it (merged with failed CI, removed from the queue): ask (§ 7, #1413).
    Blocked(String),
    /// No PR at the result, task-branch PRs merged at other heads (#1777): the owner may close with no merge (#1779).
    Superseded { reason: String, merged: String },
}

impl Publish {
    /// Profile outcome and error text: CI pending retries, an owner blocker asks; only a failed call counts in `Failed %` (#1572).
    pub fn outcome(&self) -> (&'static str, Option<&str>) {
        match self {
            Publish::Done(_) | Publish::Enqueued(_) | Publish::Rebased(_) => ("ok", None),
            Publish::Pending(e) | Publish::Waiting(e) => ("retried", Some(e)),
            Publish::Blocked(e) | Publish::Superseded { reason: e, .. } => ("ask", Some(e)),
            Publish::Failed(e) | Publish::MainMoved(e) => ("failed", Some(e)),
        }
    }
}

/// The trusted log of issue `n` (§ 2).
pub fn entries(board: &dyn Board, n: u64) -> Result<Vec<Entry>> {
    reply::log(n, &board.comments(n)?)
}

pub mod reply;

// ---------- direct issue access (§ 5 `taskq issue`) ----------

/// § 5 `taskq issue N`: any issue of the configured board, task or not, with every comment.
pub fn issue_json(board: &dyn Board, n: u64) -> Result<Value> {
    let i = board.issue(n)?;
    let comments: Vec<Value> = board.comments(n)?.into_iter()
        .map(|c| json!({"id": c.id, "author": c.author, "created": stamp(c.created), "edited": c.edited, "trusted": c.trusted, "body": c.body})).collect();
    Ok(json!({"number": n, "url": board.url(n), "title": i.title, "state": if i.open { "open" } else { "closed" }, "labels": i.labels, "body": i.body, "comments": comments}))
}

pub enum IssueWrite<'a> { Comment(&'a str), Label(&'a str) }

/// § 5 `taskq issue N --comment|--label`: one write, read back. The receipt's `outcome` is `accepted`, `rejected`
/// (nothing written) or `unknown` (written, no readback: retry the same op id).
pub fn issue_write(board: &dyn Board, n: u64, write: IssueWrite, op_id: Option<&str>) -> Value {
    let (key, value) = match write { IssueWrite::Comment(_) => ("comment", Value::Null), IssueWrite::Label(l) => ("label", json!(l)) };
    let receipt = |outcome: &str, value: Value, reason: &str, next: &str| {
        let mut r = json!({"issue": n, "url": board.url(n), "op_id": op_id, "outcome": outcome, "reason": reason, "next": next});
        r[key] = value;
        r
    };
    let retry = op_id.map_or(format!("Read taskq issue {n} before writing again."), |id| format!("Retry with --op-id {id}."));
    match write {
        IssueWrite::Comment(text) => {
            let text = text.trim_end();
            if text.trim().is_empty() { return receipt("rejected", value, "comment text is empty", "Pass --comment with text."); }
            if text.trim_start().starts_with("taskq:") {
                return receipt("rejected", value, "a `taskq:` first line is a log entry", "Use the task command (answer, steer, requeue, ...).");
            }
            let body = op_id.map_or(text.to_string(), |id| format!("{text}\nop-id: {id}"));
            // the op id's comment: the first unedited trusted one ending in its line
            let mine = |cs: &[Comment]| op_id.and_then(|id| cs.iter().find(|c| c.trusted && !c.edited && c.body.trim_end().lines().last() == Some(&format!("op-id: {id}"))).cloned());
            if op_id.is_some() {
                match board.comments(n).map(|cs| mine(&cs)) {
                    Err(e) => return receipt("rejected", value, &e, &format!("Read taskq issue {n}, then retry.")),
                    Ok(Some(c)) if c.body.trim_end() == body => return receipt("accepted", json!(c.id), "", ""),
                    Ok(Some(c)) => return receipt("rejected", json!(c.id), &format!("op id already names comment {} with other text", c.id), "Use a new --op-id."),
                    Ok(None) => {}
                }
            }
            let sent = board.comment(n, &body);
            let found = board.comments(n).map(|cs| match &sent {
                Ok(id) => cs.into_iter().find(|c| c.id == *id && c.body.trim_end() == body),
                Err(_) => mine(&cs).filter(|c| c.body.trim_end() == body),
            });
            match (sent, found) {
                (_, Ok(Some(c))) => receipt("accepted", json!(c.id), "", ""),
                (Err(e), _) | (Ok(_), Err(e)) => receipt("unknown", value, &e, &retry),
                (Ok(id), Ok(None)) => receipt("unknown", json!(id), &format!("comment {id} not read back"), &retry),
            }
        }
        IssueWrite::Label(label) => {
            if label.trim().is_empty() || label.trim() != label || label.contains(',') {
                return receipt("rejected", value, "label: one name without commas or edge spaces", "Pass --label NAME.");
            }
            if label == crate::service::TASK_LABEL || label == "taskq-host" || label == crate::runtime::usage::POLICY_LABEL || label.starts_with("q-") {
                return receipt("rejected", value, &format!("label {label} is task state"), "Use the task command (start, requeue, ...).");
            }
            let present = || board.issue(n).map(|i| i.labels.iter().any(|l| l == label));
            match present() {
                Err(e) => return receipt("rejected", value, &e, &format!("Read taskq issue {n}, then retry.")),
                Ok(true) => return receipt("accepted", value, "", ""),
                Ok(false) => {}
            }
            let sent = board.labels(n, &[label.to_string()], &[]);
            match (sent, present()) {
                (_, Ok(true)) => receipt("accepted", value, "", ""),
                (Err(e), _) | (_, Err(e)) => receipt("unknown", value, &e, &retry),
                (Ok(()), Ok(false)) => receipt("unknown", value, &format!("label {label} not read back"), &retry),
            }
        }
    }
}

// ---------- create claim issue (§ 1 `taskq claim-issue`) ----------

/// The title `taskq claim-issue` creates, and finds again after a lost create reply.
pub const CLAIM_TITLE: &str = "taskq create claims";
const CLAIM_BODY: &str = "TaskQ create claims (taskq.md § 1): `taskq add --op-id` and `taskq finding` comment here before they create an issue.\n\nKeep this issue. Never edit or delete its comments.";

/// § 1: why issue `i` cannot hold create claims, `None` when it can.
pub fn claim_issue_refusal(i: &Issue) -> Option<&'static str> {
    if !i.trusted { Some("its author is not trusted") }
    else if crate::service::is_task(i) || crate::service::is_held(i) { Some("it is a task") }
    else if i.labels.iter().any(|l| l == crate::service::HOST_LABEL) { Some("it is a host heartbeat") }
    else if i.labels.iter().any(|l| l == crate::runtime::usage::POLICY_LABEL) { Some("it is the fleet account policy") }
    else { None }
}

/// § 1 `taskq claim-issue [N]`: reuse the configured claim issue, adopt `N`, find the one created earlier by title,
/// or create one; every result is read back. Receipt `{issue, url, source, outcome, reason, next}`: `source` is
/// `configured`, `adopted`, `found` or `created`; `outcome` is `accepted`, `rejected` (nothing written) or `unknown`.
pub fn claim_issue(board: &dyn Board, configured: Option<u64>, adopt: Option<u64>) -> Value {
    let receipt = |outcome: &str, n: Option<u64>, source: &str, reason: &str, next: &str|
        json!({"issue": n, "url": n.map(|n| board.url(n)), "source": source, "outcome": outcome, "reason": reason, "next": next});
    let check = |n: u64, source: &str| match board.issue(n) {
        Err(e) => receipt("rejected", Some(n), source, &e, "Retry taskq claim-issue."),
        Ok(i) => match claim_issue_refusal(&i) {
            Some(why) => receipt("rejected", Some(n), source, &format!("#{n} cannot hold create claims: {why}"), "Pass an issue by a trusted author that is not a task, or omit N."),
            None => receipt("accepted", Some(n), source, "", ""),
        },
    };
    match (configured, adopt) {
        (Some(c), Some(a)) if c != a => receipt("rejected", Some(c), "configured", &format!("create_claim_issue is already #{c}; another issue would orphan its claims"),
            &format!("Keep #{c}: run taskq claim-issue without N.")),
        (Some(c), _) => check(c, "configured"),
        (None, Some(a)) => check(a, "adopted"),
        (None, None) => {
            let found = match board.issues(Some(0)) {
                Err(e) => return receipt("rejected", None, "found", &e, "Retry taskq claim-issue."),
                Ok(list) => list.into_iter().filter(|i| i.title == CLAIM_TITLE && claim_issue_refusal(i).is_none()).map(|i| i.number).collect::<Vec<_>>(),
            };
            if found.len() > 1 {
                let list = found.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ");
                return receipt("rejected", None, "found", &format!("several claim issues: {list}"), "Pick one: taskq claim-issue N.");
            }
            if let Some(&n) = found.first() { return check(n, "found"); }
            let retry = "Retry taskq claim-issue: it finds the created issue by its title.";
            match board.create(CLAIM_TITLE, CLAIM_BODY, &[]) {
                Err(e) => receipt("unknown", None, "created", &e, retry),
                Ok(n) => match board.issue(n) {
                    Ok(i) if i.title == CLAIM_TITLE && claim_issue_refusal(&i).is_none() => receipt("accepted", Some(n), "created", "", ""),
                    Ok(_) => receipt("unknown", Some(n), "created", &format!("#{n} not read back as a claim issue"), retry),
                    Err(e) => receipt("unknown", Some(n), "created", &e, retry),
                },
            }
        }
    }
}

/// § 4 Profiling board operations: each call of the inner board is one `taskq-board` record; cached clock, user
/// and label-schema reads are not operations and pass through; `ensure_labels` is a `label` operation.
struct Profiled(Box<dyn Board>);

impl Profiled {
    #[tracing::instrument(name="board.request",skip_all,fields(coreq_operation=kind))]
    fn op<T>(&self, kind: &'static str, f: impl FnOnce(&dyn Board) -> Result<T>) -> Result<T> {
        let began = std::time::Instant::now();
        let r = f(&*self.0);
        crate::profile::board(kind, began.elapsed(), r.as_ref().err().map(String::as_str), self.0.requests().remaining);
        r
    }
}

impl Board for Profiled {
    fn canonical_labels(&self) -> Result<bool> { self.0.canonical_labels() }
    fn state_label(&self, state: &str) -> Result<String> { self.0.state_label(state) }
    fn ensure_labels(&self, names: &[String]) -> Result<()> { self.op("label", |b| b.ensure_labels(names)) }
    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>> { self.op("list", |b| b.issues(since)) }
    fn tasks(&self, since: i64) -> Result<Vec<Issue>> { self.op("list", |b| b.tasks(since)) }
    fn kept(&self) -> Option<i64> { self.0.kept() }
    fn issue(&self, n: u64) -> Result<Issue> { self.op("read", |b| b.issue(n)) }
    fn closed_tasks(&self, page: u32, since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> { self.op("list", |b| b.closed_tasks(page, since)) }
    fn closed_labelled(&self, label: &str) -> Result<Vec<(Issue, i64)>> { self.op("list", |b| b.closed_labelled(label)) }
    fn source(&self, path: &str, sha: &str) -> Result<String> { self.op("read", |b| b.source(path, sha)) }
    fn first_admission(&self, n: u64) -> Result<Option<(u64, i64)>> { self.op("history", |b| b.first_admission(n)) }
    fn comments(&self, n: u64) -> Result<Vec<Comment>> { self.op("inbox", |b| b.comments(n)) }
    fn comment(&self, n: u64, body: &str) -> Result<u64> {
        self.op(if body.starts_with("taskq: spawn ") { "claim" } else { "comment" }, |b| b.comment(n, body))
    }
    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()> { self.op("comment", |b| b.edit_comment(n, id, body)) }
    fn react_read(&self, n: u64, id: u64) -> Result<()> { self.op("comment", |b| b.react_read(n, id)) }
    fn comment_url(&self, n: u64, id: u64) -> String { self.0.comment_url(n, id) }
    fn summaries(&self) -> bool { self.0.summaries() }
    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()> { self.op("label", |b| b.labels(n, add, remove)) }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64> { self.op("write", |b| b.create(title, body, labels)) }
    fn edit(&self, n: u64, body: &str) -> Result<()> { self.op("write", |b| b.edit(n, body)) }
    fn close(&self, n: u64) -> Result<()> { self.op("close", |b| b.close(n)) }
    fn reopen(&self, n: u64) -> Result<()> { self.op("close", |b| b.reopen(n)) }
    fn now(&self) -> Result<i64> { self.0.now() }
    fn me(&self) -> Result<String> { self.0.me() }
    fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Publish {
        let began = std::time::Instant::now();
        let p = self.0.publish(n, sha, mode, title, task_line);
        let (outcome, err) = p.outcome();
        crate::profile::board_as("publish", began.elapsed(), outcome, err, self.0.requests().remaining);
        p
    }
    fn defer_publication(&self, mode: &str) -> bool { self.0.defer_publication(mode) }
    fn cancel_publication(&self, n: u64, sha: &str) -> Result<()> { self.op("publish", |b| b.cancel_publication(n, sha)) }
    fn close_task_prs(&self, n: u64, reason: &str) -> Result<Vec<u64>> { self.op("publish", |b| b.close_task_prs(n, reason)) }
    fn resume_publication(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str, root: &std::path::Path, enqueued: bool) -> Publish {
        let began = std::time::Instant::now();
        let p = self.0.resume_publication(n, sha, mode, title, task_line, root, enqueued);
        let (outcome, err) = p.outcome();
        crate::profile::board_as("publish", began.elapsed(), outcome, err, self.0.requests().remaining);
        p
    }
    fn url(&self, n: u64) -> String { self.0.url(n) }
    fn foreign(&self, repo: &str, n: u64) -> Result<Issue> { self.op("read", |b| b.foreign(repo, n)) }
    fn foreign_url(&self, repo: &str, n: u64) -> String { self.0.foreign_url(repo, n) }
    fn requests(&self) -> crate::requests::Budget { self.0.requests() }
    fn quota(&self) -> crate::requests::Budget { self.0.quota() }
    fn forget(&self) { self.0.forget() }
    fn merge_ci_green(&self, sha: &str) -> Result<()> { self.op("read", |b| b.merge_ci_green(sha)) }
    fn delivery_gap(&self, mode: &str) -> Result<Option<String>> { self.op("read", |b| b.delivery_gap(mode)) }
    fn upload_file(&self, n: u64, path: &std::path::Path) -> Result<String> { self.op("write", |b| b.upload_file(n, path)) }
    fn upload_attachment(&self, n: u64, path: &std::path::Path) -> Result<String> { self.op("write", |b| b.upload_attachment(n, path)) }
    fn file(&self, url: &str) -> Result<Vec<u8>> { self.op("read", |b| b.file(url)) }
    fn video_inline(&self) -> bool { self.0.video_inline() }
    fn html_downloads(&self) -> bool { self.0.html_downloads() }
    fn pr_route(&self) -> crate::check::Probe { self.op("read", |b| Ok(b.pr_route())).unwrap_or_else(crate::check::Probe::Unknown) }
    fn access(&self) -> Vec<(&'static str, crate::check::Probe)> { self.op("read", |b| Ok(b.access())).unwrap_or_default() }
}

// ---------- time ----------

fn days(y: i64, m: i64, d: i64) -> i64 {
    // days from 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

/// `2026-10-12T10:00:00Z`, with optional fraction or `+00:00`; UTC only, as both boards send.
pub fn iso(s: &str) -> Option<i64> {
    let n = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    Some(days(n(0, 4)?, n(5, 7)?, n(8, 10)?) * 86400 + n(11, 13)? * 3600 + n(14, 16)? * 60 + n(17, 19)?)
}

/// HTTP `Date`: `Mon, 12 Oct 2026 10:00:00 GMT`.
fn wall() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn http_date(s: &str) -> Option<i64> {
    let w: Vec<&str> = s.split_whitespace().collect();
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mon = *w.get(2)?;
    let m = months.iter().position(|x| *x == mon)? as i64 + 1;
    let t: Vec<i64> = w.get(4)?.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    Some(days(w.get(3)?.parse().ok()?, m, w.get(1)?.parse().ok()?) * 86400 + t.first()? * 3600 + t.get(1)? * 60 + t.get(2)?)
}

// ---------- HTTP through curl ----------

use crate::http::quote;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    #[serde(skip, default = "read_operation")]
    operation: &'static str,
}

fn read_operation() -> &'static str { "read" }

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn json(&self) -> Result<Value> {
        serde_json::from_str(&self.body).map_err(|e| {
            crate::incident::detect("adapter.bad_json", concat!(file!(), ":", line!()), "none", &[self.operation], &["http:2xx"]);
            format!("bad JSON from board: {e}")
        })
    }
    fn next(&self) -> Option<String> {
        // Link: <url>; rel="next", <url>; rel="last"
        self.header("link")?.split(',').find(|p| p.contains("rel=\"next\""))
            .and_then(|p| Some(p.split_once('<')?.1.split_once('>')?.0.to_string()))
    }
}

fn operation(method: &str) -> &'static str {
    match method { "GET" => "read", "POST" => "create", "PUT" | "PATCH" => "update", "DELETE" => "delete", _ => "other" }
}

/// §5: private file transport keeps reply uploads/downloads outside bounded process pipes.
struct HttpFiles(std::path::PathBuf);
impl HttpFiles {
    fn new() -> Result<Self> {
        let dir=std::env::temp_dir().join(format!("taskq-http-{}",crate::runtime::uuid()));
        let mut options=std::fs::DirBuilder::new();
        #[cfg(unix)] {use std::os::unix::fs::DirBuilderExt;options.mode(0o700);}
        options.create(&dir).map_err(|e|format!("board HTTP storage: {e}"))?;
        Ok(Self(dir))
    }
    fn path(&self,name:&str)->std::path::PathBuf{self.0.join(name)}
    fn response(&self)->Result<Vec<u8>> {
        match std::fs::read(self.path("response")){Ok(bytes)=>Ok(bytes),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(vec![]),Err(e)=>Err(format!("board HTTP response: {e}"))}
    }
    fn body(&self,body:&Value)->Result<()> {
        use std::io::Write;
        let mut options=std::fs::OpenOptions::new();options.create_new(true).write(true);
        #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let mut file=options.open(self.path("request")).map_err(|e|e.to_string())?;
        file.write_all(body.to_string().as_bytes()).map_err(|e|e.to_string())
    }
}
impl Drop for HttpFiles {fn drop(&mut self){for name in ["request","response"]{let _=std::fs::remove_file(self.path(name));}let _=std::fs::remove_dir(&self.0);}}

/// One request, any HTTP status. Token on stdin, body/response in private files, never argv.
/// `etag` makes a GET conditional (`If-None-Match`).
fn curl(method: &str, url: &str, auth: &str, body: Option<&Value>, etag: Option<&str>) -> Result<Reply> {
    let files=HttpFiles::new()?;
    let mut cfg = format!(
        "silent\nshow-error\ndump-header = \"-\"\nrequest = {}\nurl = {}\nheader = {}\nheader = \"Accept: application/json\"\nheader = \"Expect:\"\nmax-time = 60\n",
        quote(method), quote(url), quote(auth)
    );
    cfg+=&format!("output = {}\n",quote(&files.path("response").to_string_lossy()));
    if let Some(e) = etag {
        cfg += &format!("header = {}\n", quote(&format!("If-None-Match: {e}")));
    }
    if let Some(b) = body {
        files.body(b)?;
        cfg += &format!("header = \"Content-Type: application/json\"\ndata-binary = {}\n", quote(&format!("@{}",files.path("request").display())));
    }
    let out = crate::http::with_config("board.http", &["-K", "-"], &cfg, std::time::Duration::from_secs(65)).map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl {method} {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8(files.response()?).map_err(|e|format!("board HTTP response: {e}"))?);
    let mut rest = text.as_str();
    loop {
        let (head, body) = rest.split_once("\r\n\r\n").ok_or("curl: no HTTP header")?;
        let mut lines = head.lines();
        let status: u16 = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).unwrap_or(0);
        if status == 100 && body.starts_with("HTTP/") {
            rest = body;
            continue;
        }
        let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
        return Ok(Reply { status, headers, body: body.to_string(), operation: operation(method) });
    }
}

pub mod github;
pub mod gitlab;

/// ` into <base>` of a § 7 receipt whose PR/MR merged into a branch other than `main`.
fn into(base: &str) -> String {
    if base.is_empty() || base == "main" { String::new() } else { format!(" into {base}") }
}

/// § 2 visual evidence: the image kind of `bytes` by signature (PNG, JPEG, GIF, WebP), else none.
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [0xff, 0xd8, 0xff, ..] => Some("jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

/// § 2 visual evidence videos (#1844): `mp4` (ISO `ftyp` box) or `webm` (EBML header), else none. Boards pick the
/// player by the file extension, so an uploaded video's name ends in its kind.
pub fn video_kind(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => Some("mp4"),
        [0x1a, 0x45, 0xdf, 0xa3, ..] => Some("webm"),
        _ => None,
    }
}

/// The file name an uploaded image keeps: ASCII letters, digits, `.`, `-`, `_`; anything else becomes `-`.
fn image_name(path: &std::path::Path) -> String {
    let name: String = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        .chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' }).collect();
    if name.trim_matches(['.', '-']).is_empty() { "image".into() } else { name }
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

/// A `Depends on <repo>#N` reply (§ 1, R12): issue `n` (key `num`) whose `state` is `open` or `closed`, else unknown.
fn dep_reply(v: Value, num: &str, n: u64, open: &str) -> Result<Value> {
    match (v[num].as_u64(), v["state"].as_str()) {
        (Some(m), Some(st)) if m == n && (st == open || st == "closed") => Ok(v),
        _ => Err(format!("malformed reply for issue {n}")),
    }
}

fn names(v: &Value, k: &str) -> Vec<String> {
    v.as_array().into_iter().flatten().map(|x| x.as_str().map(String::from).unwrap_or_else(|| s(x, k))).collect()
}

fn verify_assignment(board: &dyn Board, n: u64, add: &[String], remove: &[String]) -> Result<()> {
    let issue = board.issue(n)?;
    if add.iter().any(|l| !issue.labels.contains(l)) || remove.iter().any(|l| issue.labels.contains(l)) {
        return Err(format!("#{n}: label assignment readback differs; issue may already exist/changes may be partial; inspect before retry"));
    }
    Ok(())
}

fn encode(s: &str) -> String {
    s.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect()
}

fn label_matches(v: &Value, name: &str, color: &str, description: &str) -> bool {
    v["name"] == name && s(v, "color").trim_start_matches('#').eq_ignore_ascii_case(color) && v["description"] == description
}


// ---------- boards ----------

/// One board: its setup facts and constructors. Each lives in its own module (taskq.md § 9); nothing outside a
/// board module branches on which board it is.
pub struct Kind {
    /// This board records whether closure completed the task (§ 1).
    pub closed_reason: bool,
    /// This board sends the core budget on every reply, so a read without it is a client defect (R6).
    pub core_budget: bool,
    /// `board` in `taskq.json`.
    pub name: &'static str,
    /// The user name field of the CLI's `api user`.
    pub user_key: &'static str,
    pub default_host: &'static str,
    token: fn(&coreq::Tool, &str) -> Result<String>,
    /// This provider supports reloading its installed credential before requests.
    reload_token: bool,
    token_env: &'static [&'static str],
    api: fn(&str, &str, &str) -> Api,
    board: fn(Api) -> Box<dyn Board>,
}

pub const BOARDS: [&Kind; 2] = [&github::KIND, &gitlab::KIND];

pub fn kind(name: &str) -> Result<&'static Kind> {
    BOARDS.into_iter().find(|k| k.name == name).ok_or(format!("unknown board {name}; expected {}", BOARDS.map(|k| k.name).join("|")))
}

fn label_schema(schema: Option<&str>) -> Result<Option<bool>> {
    match schema {
        None => Ok(None), Some("canonical") => Ok(Some(true)), Some("legacy") => Ok(Some(false)),
        Some(s) => Err(format!("unknown label_schema {s}; expected canonical|legacy")),
    }
}

impl Kind {
    pub fn worker_env(&self, tool: &coreq::Tool, host: &str) -> Result<Vec<(String, String)>> {
        let token = (self.token)(tool, host)?;
        Ok(self.token_env.iter().map(|name| (name.to_string(), token.clone())).collect())
    }

    /// The board of `repo` on the web `host`, with the token this board's lookup finds (§ 9).
    pub fn open(&self, host: &str, repo: &str, schema: Option<&str>, tool: &coreq::Tool) -> Result<Box<dyn Board>> {
        let schema = label_schema(schema)?;
        let token = (self.token)(tool, host)?;
        let mut api = (self.api)(host, repo, &token);
        api.credentials.get_mut().pause = Some(crate::requests::pause_file(&tool.install, &api.root, &token));
        api.credentials.get_mut().user = Some(crate::requests::user_file(&tool.install, &api.root, &token));
        api.credentials.get_mut().store = Some(crate::requests::cache_dir(&tool.install, &api.root, &token));
        if self.reload_token {
            let (tool, host, lookup) = (tool.clone(), host.to_string(), self.token);
            api.token_source = Some((tool.install.clone(), Box::new(move || lookup(&tool, &host))));
        }
        *api.label_schema.borrow_mut() = schema;
        Ok(Box::new(Profiled((self.board)(api))))
    }
}

#[cfg(test)]
type Transport = Box<dyn Fn(&str, &str, Option<Value>) -> Result<Reply>>;

/// The HTTP side every board shares: one API root, its token, the board clock and cached reads.
pub struct Api {
    /// `https://api.github.com/repos/o/r`, `https://host/api/v4/projects/o%2Fr`
    base: String,
    /// `https://api.github.com`, `https://host/api/v4`: the token goes nowhere else.
    root: String,
    credentials: RefCell<Credentials>,
    generation: std::cell::Cell<u64>,
    /// Re-read the operator credential before requests; test-only APIs may use a fixed token.
    token_source: Option<(std::path::PathBuf, Box<dyn Fn() -> Result<String>>)>,
    me: RefCell<Option<String>>,
    clock: RefCell<i64>,
    label_schema: RefCell<Option<bool>>,
    /// Labels whose catalog definition this process has read back: verified once per process (§ 4 budget).
    labels_ok: RefCell<std::collections::HashSet<String>>,
    /// The last `ETag` and reply of each GET; a 304 reuses that reply, which GitHub does not count (§ 4 budget, #1819).
    etags: RefCell<std::collections::HashMap<String, (String, Reply)>>,
    /// The last GET was answered 304 Not Modified.
    unchanged: std::cell::Cell<bool>,
    /// While set, relative URLs resolve under this repository API instead of `base`: a task PR in another repository
    /// of the same host and token (#2047).
    scope: RefCell<Option<String>>,
    #[cfg(test)]
    transport: Option<Transport>,
}

/// All credential-scoped HTTP state changes together when the operator replaces the token.
struct Credentials {
    auth: String,
    budget: crate::requests::Shared,
    pause: Option<std::path::PathBuf>,
    /// The login kept for every process of the install (§ 4 budget).
    user: Option<std::path::PathBuf>,
    /// [`store`] directory of this credential; none for test and one-off APIs.
    store: Option<std::path::PathBuf>,
}

/// Cached conditional replies per process; past this the cache restarts empty.
const ETAGS: usize = 256;

/// Reads whose conditional reply every process of the install shares (#1985): a CLI process or a restarted service
/// asks with the last `ETag` too, so an unchanged issue, its comments or its events answer 304, which GitHub does not bill.
const SHARED: [&str; 3] = ["GET issues/N", "GET issues/N/comments", "GET issues/N/events"];

/// The on-disk side of [`SHARED`] reads: one file per URL under `<install>/board-cache-<key>/`.
mod store {
    use super::Reply;
    use std::path::Path;

    /// Entries older than this are pruned; past [`MAX`] entries the oldest go first.
    const AGE: std::time::Duration = std::time::Duration::from_secs(86_400);
    const MAX: usize = 4096;

    fn file(dir: &Path, url: &str) -> std::path::PathBuf {
        let h: String = ring::digest::digest(&ring::digest::SHA256, url.as_bytes()).as_ref()[..16].iter().map(|b| format!("{b:02x}")).collect();
        dir.join(format!("{h}.json"))
    }

    /// The stored `ETag` and reply of `url`; a malformed or foreign entry reads as none.
    pub fn get(dir: &Path, url: &str) -> Option<(String, Reply)> {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(file(dir, url)).ok()?).ok()?;
        if v["url"] != url { return None; }
        Some((v["etag"].as_str()?.to_string(), serde_json::from_value(v["reply"].clone()).ok()?))
    }

    /// Writes through a temporary file, so a concurrent reader sees the old entry or the new one; failures only lose the entry.
    pub fn put(dir: &Path, url: &str, etag: &str, reply: &Reply) {
        static PRUNED: std::sync::Once = std::sync::Once::new();
        if std::fs::create_dir_all(dir).is_err() { return; }
        PRUNED.call_once(|| prune(dir));
        let path = file(dir, url);
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        let text = serde_json::json!({"url": url, "etag": etag, "reply": reply}).to_string();
        if std::fs::write(&tmp, text).is_err() || std::fs::rename(&tmp, &path).is_err() { let _ = std::fs::remove_file(&tmp); }
    }

    pub fn forget(dir: &Path, url: &str) { let _ = std::fs::remove_file(file(dir, url)); }

    /// Once per process: drops entries past [`AGE`], then the oldest past [`MAX`].
    fn prune(dir: &Path) {
        let now = std::time::SystemTime::now();
        let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten()
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
        entries.retain(|(at, path)| now.duration_since(*at).is_ok_and(|age| age <= AGE) || std::fs::remove_file(path).is_err());
        entries.sort();
        let over = entries.len().saturating_sub(MAX);
        for (_, path) in entries.into_iter().take(over) { let _ = std::fs::remove_file(path); }
    }
}

impl Api {
    fn new(root: String, base: String, token: &str) -> Api {
        let budget = crate::requests::Shared::for_api(&root, token);
        Api { base, root, token_source: None, generation: Default::default(), credentials: RefCell::new(Credentials {auth: format!("Authorization: Bearer {token}"), budget, pause: None, user: None, store: None}), me: RefCell::new(None), clock: RefCell::new(0), label_schema: RefCell::new(None), labels_ok: RefCell::default(), etags: RefCell::default(), unchanged: Default::default(), scope: RefCell::new(None), #[cfg(test)] transport: None }
    }

    /// Missing credentials fail closed; replacement drops user/conditional caches and the old token's pause.
    fn refresh(&self) -> Result<()> {
        let Some((install, source)) = &self.token_source else { return Ok(()) };
        let token = source()?;
        let auth = format!("Authorization: Bearer {token}");
        if self.credentials.borrow().auth != auth {
            *self.credentials.borrow_mut() = Credentials {auth, budget: crate::requests::Shared::for_api(&self.root, &token),
                pause: Some(crate::requests::pause_file(install, &self.root, &token)), user: Some(crate::requests::user_file(install, &self.root, &token)),
                store: Some(crate::requests::cache_dir(install, &self.root, &token))};
            *self.me.borrow_mut() = None;
            self.etags.borrow_mut().clear();
            self.labels_ok.borrow_mut().clear();
            self.clock.replace(0);
            self.unchanged.set(false);
            self.generation.set(self.generation.get() + 1);
        }
        Ok(())
    }

    fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<Reply> {
        self.send(method, url, body, &self.op(method, url))
    }

    /// The accounting key of `url`, relative to this board's API (§ 4).
    fn op(&self, method: &str, url: &str) -> String {
        let base = self.repo();
        crate::requests::op(method, url.strip_prefix(&base).or_else(|| url.strip_prefix(&self.root)).unwrap_or(url))
    }

    /// The repository API relative URLs resolve under: `base`, or the scope of [`Api::scoped`].
    fn repo(&self) -> String {
        self.scope.borrow().clone().unwrap_or_else(|| self.base.clone())
    }

    /// Run `f` with relative URLs under the repository API `base` (same root, token and budget), then restore (#2047).
    fn scoped<T>(&self, base: String, f: impl FnOnce() -> T) -> T {
        let before = self.scope.replace(Some(base));
        let out = f();
        self.scope.replace(before);
        out
    }

    /// The reset while this budget, or the install's pause file, holds requests (§ 4).
    fn paused(&self) -> Option<i64> {
        let now = std::time::Instant::now();
        if let Some((reset, wait)) = self.credentials.borrow().pause.as_deref().and_then(|f| crate::requests::recorded_pause(f, wall())) {
            self.credentials.borrow().budget.borrow_mut().hold(reset, wait, now);
        }
        self.credentials.borrow().budget.borrow_mut().paused(now)
    }

    /// absolute URLs come from pagination headers: the token goes only to the configured API
    fn absolute(&self, url: &str) -> String {
        if url.starts_with("https://") { url.to_string() } else { format!("{}{url}", self.repo()) }
    }

    /// One request counted under `op`. While the board budget is exhausted nothing is sent (§ 4).
    fn send(&self, method: &str, url: &str, body: Option<Value>, op: &str) -> Result<Reply> {
        let url = self.absolute(url);
        if !url.starts_with(&format!("{}/", self.root)) {
            return Err(format!("refused: {url} is outside the board API"));
        }
        self.refresh()?;
        if let Some(reason) = &self.credentials.borrow().budget.borrow().unknown { return Err(format!("board requests held: {reason}")); }
        if let Some(reset) = self.paused() {
            return Err(format!("{method} {url}: {}", crate::requests::paused_text(reset)));
        }
        let pace = self.credentials.borrow().budget.borrow().pace(*self.clock.borrow());
        if let Some(wait) = pace { std::thread::sleep(wait); }
        self.credentials.borrow().budget.borrow_mut().count(op.into());
        crate::requests::tally(op, &crate::requests::api_host(&self.root));
        self.unchanged.set(false);
        let conditional = method == "GET" && body.is_none();
        let shared = self.credentials.borrow().store.clone().filter(|_| conditional && SHARED.contains(&op));
        if let Some((e, cached)) = shared.as_deref().filter(|_| !self.etags.borrow().contains_key(&url)).and_then(|d| store::get(d, &url)) {
            self.etags.borrow_mut().insert(url.clone(), (e, cached));
        }
        let etag = conditional.then(|| self.etags.borrow().get(&url).map(|(e, _)| e.clone())).flatten();
        crate::profile::measure_classified("forge.call", || {
            #[cfg(test)]
            let r = match &self.transport { Some(transport) => transport(method, &url, body), None => curl(method, &url, &self.credentials.borrow().auth, body.as_ref(), etag.as_deref()) };
            #[cfg(not(test))]
            let r = curl(method, &url, &self.credentials.borrow().auth, body.as_ref(), etag.as_deref());
            let r=r.map_err(|message|(crate::profile::class(&message),message))?;
            if let Some(t) = r.header("date").and_then(http_date) {
                *self.clock.borrow_mut() = t;
            }
            // GitHub GraphQL refuses an exhausted budget with a RATE_LIMIT or RATE_LIMITED error, over HTTP 200 or 403 (#1356)
            let limited = url.ends_with("/graphql") && ["\"RATE_LIMIT\"", "\"RATE_LIMITED\""].iter().any(|t| r.body.contains(t));
            let status = if limited { 403 } else { r.status };
            let class=match status {400..=499=>"http4xx",500..=599=>"http5xx",_=>"other"};
            let paused = self.credentials.borrow().budget.borrow_mut().observe(status, limited, |n| r.header(n).map(String::from), *self.clock.borrow(), std::time::Instant::now());
            if let Some(reset) = paused {
                // ponytail: the install's processes compare against their own wall clock, not the board's
                if let Some(f) = &self.credentials.borrow().pause { let _ = std::fs::write(f, (wall() + reset - *self.clock.borrow()).to_string()); }
                return Err((class,format!("{method} {url}: HTTP {status} {}", crate::requests::paused_text(reset))));
            }
            if r.status == 304 && etag.is_some() {
                if let Some((_, cached)) = self.etags.borrow().get(&url) {
                    self.credentials.borrow().budget.borrow_mut().unbilled(op);
                    crate::requests::untally(op, &crate::requests::api_host(&self.root));
                    self.unchanged.set(true);
                    return Ok(cached.clone());
                }
            }
            if conditional && (200..300).contains(&r.status) {
                let mut etags = self.etags.borrow_mut();
                match r.header("etag") {
                    Some(e) => {
                        if etags.len() >= ETAGS && !etags.contains_key(&url) { etags.clear(); }
                        etags.insert(url.clone(), (e.to_string(), r.clone()));
                        if let Some(d) = &shared { store::put(d, &url, e, &r); }
                    }
                    None => {
                        etags.remove(&url);
                        if let Some(d) = &shared { store::forget(d, &url); }
                    }
                }
            }
            if !(200..300).contains(&r.status) {
                // one log line: a multi-line error body folds into it (§ 4)
                let body = r.body.split_whitespace().collect::<Vec<_>>().join(" ");
                return Err((class,format!("{method} {url}: HTTP {} {}", r.status, body.chars().take(300).collect::<String>())));
            }
            Ok(r)
        },|error|error.0).map_err(|(_,message)|message)
    }

    /// The full path of `repo` in a `Depends on <repo>#N` (§ 1); a bare name is under this board's owner. `sep` ends the
    /// API part of `base`.
    fn sibling(&self, sep: &str, repo: &str) -> String {
        let own = self.base.rsplit_once(sep).map(|x| x.1.replace("%2F", "/")).unwrap_or_default();
        match own.rsplit_once('/') {
            Some((owner, _)) if !repo.contains('/') => format!("{owner}/{repo}"),
            _ => repo.to_string(),
        }
    }

    /// A binary GET, or a multipart POST of the file `form` (GitLab uploads), under the same token, API and budget
    /// rules as [`Api::send`]; any HTTP error fails. The test transport sees a multipart POST as `{"file": <name>}`.
    fn raw(&self, url: &str, form: Option<&std::path::Path>) -> Result<Vec<u8>> {
        let url = if url.starts_with("https://") { url.to_string() } else { format!("{}{url}", self.base) };
        if !url.starts_with(&format!("{}/", self.root)) { return Err(format!("refused: {url} is outside the board API")); }
        self.refresh()?;
        if let Some(reason) = &self.credentials.borrow().budget.borrow().unknown { return Err(format!("board requests held: {reason}")); }
        if let Some(reset) = self.paused() { return Err(format!("{url}: {}", crate::requests::paused_text(reset))); }
        let method = if form.is_some() { "POST" } else { "GET" };
        let op = self.op(method, &url);
        self.credentials.borrow().budget.borrow_mut().count(op.clone());
        crate::requests::tally(&op, &crate::requests::api_host(&self.root));
        #[cfg(test)]
        if let Some(transport) = &self.transport {
            let form = form.map(|f| serde_json::json!({"file": f.file_name().map(|n| n.to_string_lossy().into_owned())}));
            return transport(method, &url, form).map(|r| r.body.into_bytes());
        }
        let mut cfg = format!("silent\nshow-error\nfail\nurl = {}\nheader = {}\nheader = \"Expect:\"\nmax-time = 120\n", quote(&url), quote(&self.credentials.borrow().auth));
        let files=HttpFiles::new()?;
        cfg+=&format!("output = {}\n",quote(&files.path("response").to_string_lossy()));
        if let Some(f) = form { cfg += &format!("form = {}\n", quote(&format!("file=@{}", f.display()))); }
        let out = crate::http::with_config("board.http", &["-K", "-"], &cfg, std::time::Duration::from_secs(125)).map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() { return Err(format!("curl {method} {url}: {}", String::from_utf8_lossy(&out.stderr).trim())); }
        files.response()
    }

    fn all(&self, url: &str) -> Result<Vec<Value>> {
        let mut out = vec![];
        let op = self.op("GET", url);
        let mut next = Some(url.to_string());
        let page = |r: &Reply| -> Result<Vec<Value>> { Ok(r.json()?.as_array().ok_or("invalid board list: expected array")?.clone()) };
        while let Some(u) = next {
            // later pages count under their first page's operation
            let mut r = self.send("GET", &u, None, &op)?;
            let mut items = page(&r)?;
            // #1903: a 304 replays the cached reply's Link; a full last page may have grown a next page since
            let size = u.split(['?', '&']).find_map(|p| p.strip_prefix("per_page=")?.parse::<usize>().ok());
            if self.unchanged.get() && r.next().is_none() && size == Some(items.len()) {
                self.forget_etag(&self.absolute(&u));
                r = self.send("GET", &u, None, &op)?;
                items = page(&r)?;
            }
            out.extend(items);
            next = r.next();
        }
        Ok(out)
    }

    /// Drops the conditional reply of `url` in this process and in the install's store.
    fn forget_etag(&self, url: &str) {
        self.etags.borrow_mut().remove(url);
        if let Some(d) = &self.credentials.borrow().store { store::forget(d, url); }
    }

    /// The board user, `key` its name field.
    fn me(&self, key: &str) -> Result<String> {
        self.refresh()?;
        if let Some(m) = self.me.borrow().clone() {
            return Ok(m);
        }
        let file = self.credentials.borrow().user.clone();
        let kept = file.as_deref().and_then(|f| std::fs::read_to_string(f).ok()).map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        let m = match kept {
            Some(m) => m,
            None => {
                let m = s(&self.call("GET", &format!("{}/user", self.root), None)?.json()?, key);
                if let Some(f) = &file { let _ = std::fs::write(f, &m); }
                m
            }
        };
        *self.me.borrow_mut() = Some(m.clone());
        Ok(m)
    }

    /// The board clock from the last reply's `Date`; before any reply, `free` (an endpoint the budget does not
    /// count) or `GET /user` sets it.
    fn now(&self, key: &str, free: Option<&str>) -> Result<i64> {
        self.refresh()?;
        if *self.clock.borrow() == 0 {
            match free {
                Some(path) => { self.call("GET", &format!("{}{path}", self.root), None)?; }
                None => { self.me(key)?; }
            }
        }
        Ok(*self.clock.borrow())
    }

    fn canonical_labels(&self) -> Result<bool> {
        self.refresh()?;
        if let Some(c) = *self.label_schema.borrow() { return Ok(c); }
        let all = self.all("/labels?per_page=100")?;
        let canonical = !all.iter().any(|v| matches!(v["name"].as_str(), Some("q-waiting" | "priority-1" | "priority-2")));
        *self.label_schema.borrow_mut() = Some(canonical);
        Ok(canonical)
    }

    /// Catalog sync: `color` renders a catalog color for the board; `write(current, payload)` creates or updates one label.
    fn ensure_labels(&self, names: &[String], color: fn(&str) -> String, write: impl Fn(Option<&Value>, Value) -> Result<()>) -> Result<()> {
        if !self.canonical_labels()? { return Err("legacy board: catalog adoption requires all-host gate and label_schema=canonical".into()); }
        let fresh: Vec<String> = names.iter().filter(|n| !self.labels_ok.borrow().contains(*n)).cloned().collect();
        if fresh.is_empty() { return Ok(()); }
        let names = &fresh[..];
        let all = self.all("/labels?per_page=100")?;
        let mut seen = std::collections::HashSet::new();
        for name in names {
            if !seen.insert(name) { continue; }
            let Some((c, description)) = crate::labels::definition(name) else { continue };
            let current = all.iter().find(|v| v["name"] == name.as_str());
            if current.is_some_and(|v| label_matches(v, name, c, &description)) { continue; }
            write(current, json!({"name": name, "color": color(c), "description": description}))
                .map_err(|e| format!("label {name} not verified; retry catalog: {e}"))?;
        }
        let readback = self.all("/labels?per_page=100")?;
        for name in names {
            if let Some((color, desc)) = crate::labels::definition(name) {
                if !readback.iter().any(|v| label_matches(v, name, color, &desc)) {
                    return Err(format!("label {name}: readback differs; retry catalog"));
                }
            }
        }
        self.labels_ok.borrow_mut().extend(fresh);
        Ok(())
    }

    /// The labels a new issue needs on a canonical board: the fixed catalog and its own.
    fn creation_labels(&self, labels: &[String]) -> Result<Option<Vec<String>>> {
        if !self.canonical_labels()? { return Ok(None); }
        let mut names = crate::labels::FIXED.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        names.extend_from_slice(labels);
        Ok(Some(names))
    }
}

/// `publish` outcome of a board's attempt: a refusal by the board fails, anything else is pending (§ 7).
fn outcome(r: Result<Publish>) -> Publish {
    match r {
        Ok(p) => p,
        Err(e) if e.contains("HTTP 405") || e.contains("HTTP 409") || e.contains("HTTP 422") => Publish::Failed(e),
        Err(e) => Publish::Pending(e),
    }
}

fn hex40(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}


pub fn stamp(t: i64) -> String {
    let (d, r) = (t.div_euclid(86400), t.rem_euclid(86400));
    // civil from days (Hinnant)
    let z = d + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}Z", r / 3600, r % 3600 / 60, r % 60)
}

#[cfg(test)]
pub mod fake;
#[cfg(test)]
pub use fake::{Fake, replying, replying_to};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
