//! Board contract (taskq.md § 1, § 2, § 9): the `Board` trait, shared HTTP through the OS `curl`, the registry of
//! board modules (`github`, `gitlab`) and an in-memory board for tests. Every write is a native board object; nothing else is state.

use crate::core::Entry;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::process::Command;

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
}

pub trait Board {
    fn canonical_labels(&self) -> Result<bool> { Ok(true) }
    fn state_label(&self, state: &str) -> Result<String> {
        Ok(format!("q-{}", if state == "waiting" && self.canonical_labels()? { "blocked" } else { state }))
    }
    fn ensure_labels(&self, _names: &[String]) -> Result<()> { Ok(()) }
    /// Open issues; with `since` (board seconds) every issue, closed too, updated at or after it.
    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>>;
    fn issue(&self, n: u64) -> Result<Issue>;
    /// One page (from 1) of closed `taskq` issues, most recently updated first, with each close time and whether another
    /// page follows: the Completed view reads board history on demand, never a local task store (MP14).
    fn closed_tasks(&self, _page: u32, _since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> { Err("closed task history unsupported on this board".into()) }
    fn comments(&self, n: u64) -> Result<Vec<Comment>>;
    fn comment(&self, n: u64, body: &str) -> Result<u64>;
    /// Edits comment `id` of issue `n`: an edited entry is ignored by `fold` (§ 2).
    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()>;
    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()>;
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64>;
    fn edit(&self, n: u64, body: &str) -> Result<()>;
    fn close(&self, n: u64) -> Result<()>;
    /// The board's clock in seconds; leases never use local clocks (§ 4).
    fn now(&self) -> Result<i64>;
    fn me(&self) -> Result<String>;
    /// § 7 publication of task `n` at `sha` into `main`; `mode` is `direct` or `pr`.
    fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Publish;
    /// § 7: durable intent ends the reviewer; only the service performs asynchronous publication.
    fn defer_publication(&self, _mode: &str) -> bool { false }
    fn cancel_publication(&self, _n: u64, _sha: &str) -> Result<()> { Ok(()) }
    /// § 7: a task that closed without merging closes its open `taskq-<n>` PRs unmerged, branch kept; returns them.
    fn close_task_prs(&self, _n: u64, _reason: &str) -> Result<Vec<u64>> { Ok(vec![]) }
    fn resume_publication(&self, n: u64, sha: &str, title: &str, task_line: &str, _root: &std::path::Path, _enqueued: bool) -> Publish {
        self.publish(n, sha, "pr", title, task_line)
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
    /// #1819: drop shortcuts over unchanged board reads; the next list reads the board in full.
    fn forget(&self) {}
    /// § 8 item 1 dev install: Ok only when `sha`'s `merge_group` CI passed; pending, failed or unknown is Err.
    fn merge_ci_green(&self, sha: &str) -> Result<()> { Err(format!("merge_group CI unknown on {sha}")) }
    /// § 2 visual evidence: stores the image file `path` for task `n` on the board and returns its absolute https URL.
    fn upload_image(&self, _n: u64, _path: &std::path::Path) -> Result<String> { Err("image upload unsupported on this board".into()) }
    /// The bytes behind an image URL this board serves, read with the board token: the load check and the app's copy.
    fn image(&self, url: &str) -> Result<Vec<u8>> { Err(format!("image unknown: {url} is not served by this board")) }
    /// § 2 visual evidence (#1844): true when `![name](url)` of an uploaded video plays inline in a comment; else a link.
    fn video_inline(&self) -> bool { false }
}

#[derive(Debug, PartialEq)]
pub enum Publish {
    /// Merged or already on `main`: close.
    Done(String),
    /// CI pending or missing: refuse, state unchanged.
    Pending(String),
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
            Publish::Pending(e) => ("retried", Some(e)),
            Publish::Blocked(e) | Publish::Superseded { reason: e, .. } => ("ask", Some(e)),
            Publish::Failed(e) | Publish::MainMoved(e) => ("failed", Some(e)),
        }
    }
}

/// The trusted log of issue `n` (§ 2).
pub fn entries(board: &dyn Board, n: u64) -> Result<Vec<Entry>> {
    Ok(board
        .comments(n)?
        .into_iter()
        .filter(|c| c.trusted)
        .map(|c| Entry { id: c.id, body: c.body, edited: c.edited })
        .collect())
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
    fn issue(&self, n: u64) -> Result<Issue> { self.op("read", |b| b.issue(n)) }
    fn closed_tasks(&self, page: u32, since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> { self.op("list", |b| b.closed_tasks(page, since)) }
    fn comments(&self, n: u64) -> Result<Vec<Comment>> { self.op("inbox", |b| b.comments(n)) }
    fn comment(&self, n: u64, body: &str) -> Result<u64> {
        self.op(if body.starts_with("taskq: spawn ") { "claim" } else { "comment" }, |b| b.comment(n, body))
    }
    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()> { self.op("comment", |b| b.edit_comment(n, id, body)) }
    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()> { self.op("label", |b| b.labels(n, add, remove)) }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64> { self.op("write", |b| b.create(title, body, labels)) }
    fn edit(&self, n: u64, body: &str) -> Result<()> { self.op("write", |b| b.edit(n, body)) }
    fn close(&self, n: u64) -> Result<()> { self.op("close", |b| b.close(n)) }
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
    fn resume_publication(&self, n: u64, sha: &str, title: &str, task_line: &str, root: &std::path::Path, enqueued: bool) -> Publish {
        let began = std::time::Instant::now();
        let p = self.0.resume_publication(n, sha, title, task_line, root, enqueued);
        let (outcome, err) = p.outcome();
        crate::profile::board_as("publish", began.elapsed(), outcome, err, self.0.requests().remaining);
        p
    }
    fn url(&self, n: u64) -> String { self.0.url(n) }
    fn foreign(&self, repo: &str, n: u64) -> Result<Issue> { self.op("read", |b| b.foreign(repo, n)) }
    fn foreign_url(&self, repo: &str, n: u64) -> String { self.0.foreign_url(repo, n) }
    fn requests(&self) -> crate::requests::Budget { self.0.requests() }
    fn forget(&self) { self.0.forget() }
    fn merge_ci_green(&self, sha: &str) -> Result<()> { self.op("read", |b| b.merge_ci_green(sha)) }
    fn upload_image(&self, n: u64, path: &std::path::Path) -> Result<String> { self.op("write", |b| b.upload_image(n, path)) }
    fn image(&self, url: &str) -> Result<Vec<u8>> { self.op("read", |b| b.image(url)) }
    fn video_inline(&self) -> bool { self.0.video_inline() }
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

#[derive(Clone)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    operation: &'static str,
}

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

fn quote(s: &str) -> String {
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            '\n' => q.push_str("\\n"),
            '\r' => q.push_str("\\r"),
            '\t' => q.push_str("\\t"),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

/// One request, any HTTP status. The token and body go through curl's config on stdin, never argv (R12: no secrets in `ps`).
/// `etag` makes a GET conditional (`If-None-Match`).
fn curl(method: &str, url: &str, auth: &str, body: Option<&Value>, etag: Option<&str>) -> Result<Reply> {
    let mut cfg = format!(
        "silent\nshow-error\ndump-header = \"-\"\nrequest = {}\nurl = {}\nheader = {}\nheader = \"Accept: application/json\"\nheader = \"Expect:\"\nmax-time = 60\n",
        quote(method), quote(url), quote(auth)
    );
    if let Some(e) = etag {
        cfg += &format!("header = {}\n", quote(&format!("If-None-Match: {e}")));
    }
    if let Some(b) = body {
        cfg += &format!("header = \"Content-Type: application/json\"\ndata-binary = {}\n", quote(&b.to_string()));
    }
    let out = match coreq::process::exchange("board.http", Command::new("curl").args(["-K", "-"]), cfg.as_bytes(), std::time::Duration::from_secs(65)) {
        Ok(out) | Err(coreq::process::Failure::Exit(out)) => out,
        Err(e) => return Err(format!("curl: {e}")),
    };
    if !out.status.success() {
        return Err(format!("curl {method} {url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
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
    /// `board` in `taskq.json`.
    pub name: &'static str,
    /// The user name field of the CLI's `api user`.
    pub user_key: &'static str,
    pub default_host: &'static str,
    token: fn(&coreq::Tool, &str) -> Result<String>,
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
        api.pause = Some(crate::requests::pause_file(&tool.install, &api.root, &token));
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
    auth: String,
    me: RefCell<Option<String>>,
    clock: RefCell<i64>,
    label_schema: RefCell<Option<bool>>,
    /// Request accounting and rate-limit pause of this process (§ 4).
    budget: crate::requests::Shared,
    /// Labels whose catalog definition this process has read back: verified once per process (§ 4 budget).
    labels_ok: RefCell<std::collections::HashSet<String>>,
    /// The install's pause file of this API host and credential: every process there stops until its reset (§ 4).
    pause: Option<std::path::PathBuf>,
    /// The last `ETag` and reply of each GET; a 304 reuses that reply, which GitHub does not count (§ 4 budget, #1819).
    etags: RefCell<std::collections::HashMap<String, (String, Reply)>>,
    /// The last GET was answered 304 Not Modified.
    unchanged: std::cell::Cell<bool>,
    #[cfg(test)]
    transport: Option<Transport>,
}

/// Cached conditional replies per process; past this the cache restarts empty.
const ETAGS: usize = 256;

impl Api {
    fn new(root: String, base: String, token: &str) -> Api {
        let budget = crate::requests::Shared::for_api(&root, token);
        Api { base, root, auth: format!("Authorization: Bearer {token}"), me: RefCell::new(None), clock: RefCell::new(0), label_schema: RefCell::new(None), budget, labels_ok: RefCell::default(), pause: None, etags: RefCell::default(), unchanged: Default::default(), #[cfg(test)] transport: None }
    }

    fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<Reply> {
        self.send(method, url, body, &self.op(method, url))
    }

    /// The accounting key of `url`, relative to this board's API (§ 4).
    fn op(&self, method: &str, url: &str) -> String {
        crate::requests::op(method, url.strip_prefix(&self.base).or_else(|| url.strip_prefix(&self.root)).unwrap_or(url))
    }

    /// The reset while this budget, or the install's pause file, holds requests (§ 4).
    fn paused(&self) -> Option<i64> {
        let now = std::time::Instant::now();
        if let Some((reset, wait)) = self.pause.as_deref().and_then(|f| crate::requests::recorded_pause(f, wall())) {
            self.budget.borrow_mut().hold(reset, wait, now);
        }
        self.budget.borrow_mut().paused(now)
    }

    /// One request counted under `op`. While the board budget is exhausted nothing is sent (§ 4).
    fn send(&self, method: &str, url: &str, body: Option<Value>, op: &str) -> Result<Reply> {
        // absolute URLs come from pagination headers: the token goes only to the configured API
        let url = if url.starts_with("https://") { url.to_string() } else { format!("{}{url}", self.base) };
        if !url.starts_with(&format!("{}/", self.root)) {
            return Err(format!("refused: {url} is outside the board API"));
        }
        if let Some(reason) = &self.budget.borrow().unknown { return Err(format!("board requests held: {reason}")); }
        if let Some(reset) = self.paused() {
            return Err(format!("{method} {url}: {}", crate::requests::paused_text(reset)));
        }
        self.budget.borrow_mut().count(op.into());
        crate::requests::tally(op);
        self.unchanged.set(false);
        let conditional = method == "GET" && body.is_none();
        let etag = conditional.then(|| self.etags.borrow().get(&url).map(|(e, _)| e.clone())).flatten();
        crate::profile::measure_classified("forge.call", || {
            #[cfg(test)]
            let r = match &self.transport { Some(transport) => transport(method, &url, body), None => curl(method, &url, &self.auth, body.as_ref(), etag.as_deref()) };
            #[cfg(not(test))]
            let r = curl(method, &url, &self.auth, body.as_ref(), etag.as_deref());
            let r=r.map_err(|message|(crate::profile::class(&message),message))?;
            if let Some(t) = r.header("date").and_then(http_date) {
                *self.clock.borrow_mut() = t;
            }
            // GitHub GraphQL refuses an exhausted budget with a RATE_LIMIT or RATE_LIMITED error, over HTTP 200 or 403 (#1356)
            let limited = url.ends_with("/graphql") && ["\"RATE_LIMIT\"", "\"RATE_LIMITED\""].iter().any(|t| r.body.contains(t));
            let status = if limited { 403 } else { r.status };
            let class=match status {400..=499=>"http4xx",500..=599=>"http5xx",_=>"other"};
            let paused = self.budget.borrow_mut().observe(status, limited, |n| r.header(n).map(String::from), *self.clock.borrow(), std::time::Instant::now());
            if let Some(reset) = paused {
                // ponytail: the install's processes compare against their own wall clock, not the board's
                if let Some(f) = &self.pause { let _ = std::fs::write(f, (wall() + reset - *self.clock.borrow()).to_string()); }
                return Err((class,format!("{method} {url}: HTTP {status} {}", crate::requests::paused_text(reset))));
            }
            if r.status == 304 && etag.is_some() {
                if let Some((_, cached)) = self.etags.borrow().get(&url) {
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
                    }
                    None => { etags.remove(&url); }
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
    /// rules as [`Api::send`]; any HTTP error fails. ponytail: no test transport, the URL builders around it are tested.
    fn raw(&self, url: &str, form: Option<&std::path::Path>) -> Result<Vec<u8>> {
        let url = if url.starts_with("https://") { url.to_string() } else { format!("{}{url}", self.base) };
        if !url.starts_with(&format!("{}/", self.root)) { return Err(format!("refused: {url} is outside the board API")); }
        if let Some(reason) = &self.budget.borrow().unknown { return Err(format!("board requests held: {reason}")); }
        if let Some(reset) = self.paused() { return Err(format!("{url}: {}", crate::requests::paused_text(reset))); }
        let method = if form.is_some() { "POST" } else { "GET" };
        let op = self.op(method, &url);
        self.budget.borrow_mut().count(op.clone());
        crate::requests::tally(&op);
        let mut cfg = format!("silent\nshow-error\nfail\nurl = {}\nheader = {}\nheader = \"Expect:\"\nmax-time = 120\n", quote(&url), quote(&self.auth));
        if let Some(f) = form { cfg += &format!("form = {}\n", quote(&format!("file=@{}", f.display()))); }
        let out = match coreq::process::exchange("board.http", Command::new("curl").args(["-K", "-"]), cfg.as_bytes(), std::time::Duration::from_secs(125)) {
            Ok(out) | Err(coreq::process::Failure::Exit(out)) => out,
            Err(e) => return Err(format!("curl: {e}")),
        };
        if !out.status.success() { return Err(format!("curl {method} {url}: {}", String::from_utf8_lossy(&out.stderr).trim())); }
        Ok(out.stdout)
    }

    fn all(&self, url: &str) -> Result<Vec<Value>> {
        let mut out = vec![];
        let op = self.op("GET", url);
        let mut next = Some(url.to_string());
        while let Some(u) = next {
            // later pages count under their first page's operation
            let r = self.send("GET", &u, None, &op)?;
            out.extend(r.json()?.as_array().ok_or("invalid board list: expected array")?.iter().cloned());
            next = r.next();
        }
        Ok(out)
    }

    /// The board user, `key` its name field.
    fn me(&self, key: &str) -> Result<String> {
        if let Some(m) = self.me.borrow().clone() {
            return Ok(m);
        }
        let v = self.call("GET", &format!("{}/user", self.root), None)?.json()?;
        let m = s(&v, key);
        *self.me.borrow_mut() = Some(m.clone());
        Ok(m)
    }

    fn now(&self, key: &str) -> Result<i64> {
        if *self.clock.borrow() == 0 {
            self.me(key)?;
        }
        Ok(*self.clock.borrow())
    }

    fn canonical_labels(&self) -> Result<bool> {
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

// ---------- in-memory board ----------

/// A board in memory for tests: one clock tick per write, comment ids shared across issues like real boards.
#[cfg(test)]
#[derive(Default)]
pub struct Fake {
    pub state: RefCell<FakeState>,
}

#[cfg(test)]
#[derive(Default)]
pub struct FakeState {
    pub issues: Vec<Issue>,
    pub comments: Vec<(u64, Comment)>,
    pub clock: i64,
    pub next_id: u64,
    /// `pending` or `failed` makes `publish` refuse; anything else merges.
    pub publish: String,
    pub hide_latest_comment: bool,
    pub cancelled_publications: Vec<(u64, String)>,
    pub fail_publication_cancel: bool,
    /// `close_task_prs` calls `(task, reason)`; `fail_task_prs` makes them unknown.
    pub closed_task_prs: Vec<(u64, String)>,
    pub fail_task_prs: bool,
    pub fail_issue_read: Option<(u64, usize)>,
    pub fail_labels: bool,
    /// The next this many `comment` calls fail like a board rate-limit pause.
    pub fail_comments: usize,
    /// The write commits, but its response is lost.
    pub lose_comment_response: bool,
    /// The next this many `comments` reads fail.
    pub fail_comment_reads: usize,
    /// A concurrent writer: this body lands just before the next `comment`.
    pub race: Option<String>,
    /// Issues of other repositories, `(repo, issue)`.
    pub foreign: Vec<(String, Issue)>,
    /// Issue reads, `foreign` included, for budget checks.
    pub reads: usize,
    /// Open-list reads, visible outside the thread that owns this board.
    pub lists: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    /// Comment-list reads and publication attempts, for request-count checks (#1359).
    pub comment_reads: usize,
    pub publications: usize,
    /// Uploaded images by URL; `bad_image` makes the board serve other bytes back.
    pub images: Vec<(String, Vec<u8>)>,
    pub bad_image: bool,
    /// The board plays an uploaded video inline (GitLab), else it links one (GitHub).
    pub video_inline: bool,
}

#[cfg(test)]
impl FakeState {
    fn tick(&mut self) -> i64 {
        self.clock += 1;
        self.clock
    }
    fn get(&mut self, n: u64) -> Result<&mut Issue> {
        self.issues.iter_mut().find(|i| i.number == n).ok_or(format!("no issue #{n}"))
    }
}

#[cfg(test)]
impl Fake {
    pub fn advance(&self, secs: i64) {
        self.state.borrow_mut().clock += secs;
    }
    /// A comment by an outsider or an edited one, for trust tests.
    pub fn raw(&self, n: u64, body: &str, trusted: bool, edited: bool) -> u64 {
        let mut st = self.state.borrow_mut();
        st.next_id += 1;
        let id = st.next_id;
        let t = st.tick();
        if let Ok(i) = st.get(n) {
            i.updated = t;
        }
        st.comments.push((n, Comment { id, created: t, body: body.into(), trusted, edited, author: String::new() }));
        id
    }
}

#[cfg(test)]
impl Board for Fake {
    fn upload_image(&self, n: u64, path: &std::path::Path) -> Result<String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut st = self.state.borrow_mut();
        let url = format!("https://board.example/{n}/{}-{}", st.images.len(), image_name(path));
        st.images.push((url.clone(), bytes));
        Ok(url)
    }
    fn image(&self, url: &str) -> Result<Vec<u8>> {
        let st = self.state.borrow();
        let b = st.images.iter().find(|i| i.0 == url).map(|i| i.1.clone()).ok_or(format!("image unknown: {url}"))?;
        Ok(if st.bad_image { b[..b.len() / 2].to_vec() } else { b })
    }
    fn video_inline(&self) -> bool { self.state.borrow().video_inline }
    fn closed_tasks(&self, page: u32, since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> {
        let mut all: Vec<Issue> = self.state.borrow().issues.iter().filter(|i| !i.open && since.is_none_or(|t| i.updated >= t)).cloned().collect();
        all.sort_by_key(|i| std::cmp::Reverse(i.updated));
        let start = (page.max(1) as usize - 1) * CLOSED_PAGE;
        Ok((all.iter().skip(start).take(CLOSED_PAGE).map(|i| (i.clone(), i.updated)).collect(), all.len() > start + CLOSED_PAGE))
    }
    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>> {
        if let Some(lists) = &self.state.borrow().lists { lists.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
        Ok(self.state.borrow().issues.iter().filter(|i| since.map_or(i.open, |t| i.updated >= t)).cloned().collect())
    }
    fn issue(&self, n: u64) -> Result<Issue> {
        let mut st = self.state.borrow_mut();
        if let Some((task, remaining)) = &mut st.fail_issue_read {
            if *task == n {
                *remaining -= 1;
                if *remaining == 0 {
                    st.fail_issue_read = None;
                    return Err("issue read failed".into());
                }
            }
        }
        st.reads += 1;
        drop(st);
        self.state.borrow_mut().get(n).cloned()
    }
    fn foreign(&self, repo: &str, n: u64) -> Result<Issue> {
        let mut st = self.state.borrow_mut();
        st.reads += 1;
        st.foreign.iter().find(|(r, i)| r == repo && i.number == n).map(|x| x.1.clone()).ok_or(format!("HTTP 404 {repo}#{n}"))
    }
    fn foreign_url(&self, repo: &str, n: u64) -> String {
        format!("https://board/{repo}/{n}")
    }
    fn comments(&self, n: u64) -> Result<Vec<Comment>> {
        if self.state.borrow().fail_comment_reads > 0 {
            self.state.borrow_mut().fail_comment_reads -= 1;
            return Err("comment read failed".into());
        }
        self.state.borrow_mut().comment_reads += 1;
        let st = self.state.borrow();
        Ok(st.comments.iter().filter(|(i, c)| *i == n && !(st.hide_latest_comment && c.id == st.next_id)).map(|(_, c)| c.clone()).collect())
    }
    fn comment(&self, n: u64, body: &str) -> Result<u64> {
        self.state.borrow_mut().get(n)?;
        if self.state.borrow().fail_comments > 0 {
            self.state.borrow_mut().fail_comments -= 1;
            return Err("rate limit: board requests paused".into());
        }
        let race = self.state.borrow_mut().race.take();
        if let Some(other) = race {
            self.raw(n, &other, true, false);
        }
        let id = self.raw(n, body, true, false);
        if std::mem::take(&mut self.state.borrow_mut().lose_comment_response) { return Err("comment response lost".into()); }
        Ok(id)
    }
    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let c = st.comments.iter_mut().find(|(i, c)| *i == n && c.id == id).ok_or(format!("no comment {id}"))?;
        if c.1.body != body {
            c.1.body = body.into();
            c.1.edited = true;
        }
        Ok(())
    }
    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()> {
        let mut st = self.state.borrow_mut();
        if std::mem::take(&mut st.fail_labels) { return Err("label write failed".into()); }
        let t = st.tick();
        let i = st.get(n)?;
        i.labels.retain(|l| !remove.contains(l));
        for l in add {
            if !i.labels.contains(l) {
                i.labels.push(l.clone());
            }
        }
        i.updated = t;
        Ok(())
    }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        st.next_id += 1;
        let number = st.issues.len() as u64 + 1;
        st.issues.push(Issue { number, title: title.into(), body: body.into(), labels: labels.to_vec(), assignees: vec![], updated: t, created: t, closed: None, open: true, trusted: true, reason: None });
        Ok(number)
    }
    fn edit(&self, n: u64, body: &str) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        let i = st.get(n)?;
        // like GitHub: an unchanged body keeps `updated_at`
        if i.body != body {
            i.body = body.into();
            i.updated = t;
        }
        Ok(())
    }
    fn close(&self, n: u64) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        let i = st.get(n)?;
        i.open = false;
        i.closed = Some(t);
        i.reason = Some("completed".into());
        i.updated = t;
        Ok(())
    }
    fn now(&self) -> Result<i64> {
        Ok(self.state.borrow().clock)
    }
    fn me(&self) -> Result<String> {
        Ok("owner".into())
    }
    fn defer_publication(&self, mode: &str) -> bool { mode == "pr" && self.state.borrow().publish.starts_with("queue-") }
    fn cancel_publication(&self, n: u64, sha: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.cancelled_publications.push((n, sha.into()));
        if state.fail_publication_cancel { Err("queue cancellation unknown".into()) } else { Ok(()) }
    }
    fn close_task_prs(&self, n: u64, reason: &str) -> Result<Vec<u64>> {
        let mut state = self.state.borrow_mut();
        state.closed_task_prs.push((n, reason.into()));
        if state.fail_task_prs { Err("PR state unknown".into()) } else { Ok(vec![]) }
    }

    fn publish(&self, _n: u64, sha: &str, _mode: &str, _title: &str, _task_line: &str) -> Publish {
        self.state.borrow_mut().publications += 1;
        match self.state.borrow().publish.as_str() {
            "pending" => Publish::Pending("CI pending".into()),
            "failed" => Publish::Failed("CI failed".into()),
            "queue-admitted" => Publish::Enqueued("MQ_1".into()),
            "queue-waiting" => Publish::Pending("merge_group CI pending".into()),
            "queue-refused" => Publish::Pending("merge queue did not admit PR #7: Required status check build is in progress; CI build pending".into()),
            "queue-conflict" => Publish::MainMoved("queue merge conflict".into()),
            "queue-blocked" => Publish::Blocked("PR #7 merged, merge_group CI tests failure".into()),
            "superseded" => Publish::Superseded { reason: "no open taskq-1 PR at result; merged taskq-1 PRs: #86 at ddddddd".into(), merged: "#86 at ddddddd".into() },
            _ => Publish::Done(format!("merged {sha}")),
        }
    }
    fn url(&self, n: u64) -> String {
        format!("https://board/{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Chain: real curl/config stdin -> local HTTP refusal -> request accounting and secret-free execution trace.
    #[test]
    fn native_http_preserves_refusal_without_exporting_request_data() {
        let dir = std::env::temp_dir().join(format!("taskq-native-http-{}-{}", std::process::id(),crate::runtime::uuid()));
        std::fs::create_dir(&dir).unwrap();
        let out = coreq::process::output("test.http", Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "board::tests::native_http_child", "--ignored", "--nocapture"])
            .env("TASKQ_NATIVE_HTTP_TEST_ROOT", &dir), std::time::Duration::from_secs(15));
        assert!(out.is_ok(), "native HTTP chain failed: {out:?}; phases: {}",std::fs::read_to_string(dir.join("phases.log")).unwrap_or_else(|_|"child entry not observed".into()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "native curl and collector fixture; local loopback only"]
    fn native_http_child() {
        use std::{io::{Read, Write}, net::TcpListener, time::{Duration, Instant}};
        let dir = std::path::PathBuf::from(std::env::var_os("TASKQ_NATIVE_HTTP_TEST_ROOT").unwrap());
        let began = Instant::now();
        let phase=|name:&str|writeln!(std::fs::OpenOptions::new().create(true).append(true).open(dir.join("phases.log")).unwrap(),"{name} {}ms",began.elapsed().as_millis()).unwrap();
        phase("capture.start");
        let _runtime = coreq::telemetry::capture::Runtime::install(&dir.join("db"), "fixtureq", "1").unwrap();
        phase("capture.ready");
        let installed = began.elapsed();
        crate::profile::take_report();
        // The HTTP deadline starts after collector setup, which has its own outer bound.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let body = json!({"text": "private-body".repeat(4000)});
        let expected = body.clone();
        let server = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until, "native curl did not reach the local fixture");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut part = [0; 4096];
                let n = socket.read(&mut part).unwrap();
                assert_ne!(n, 0, "incomplete request"); bytes.extend_from_slice(&part[..n]);
                assert!(bytes.len() <= 1024 * 1024);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                    if bytes.len() < end + 4 + length { continue; }
                    assert!(head.contains("authorization: bearer private-token"));
                    assert_eq!(serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length]).unwrap(), expected);
                    break;
                }
            }
            let reply = "private-response";
            write!(socket, "HTTP/1.1 503 Unavailable\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
        });
        phase("http.start");
        let result = Api::new(base.clone(), base, "private-token").call("POST", "/request", Some(body));
        phase("http.returned");
        let joined = server.join();
        phase("http.joined");
        assert!(joined.is_ok(), "HTTP fixture failed; runtime install={installed:?}, through request={:?}, client error={:?}", began.elapsed(), result.as_ref().err());
        assert!(result.err().unwrap().contains("HTTP 503"));
        assert!(crate::profile::take_report().contains("http5xx:1"));
        phase("capture.qualify");
        let health = coreq::telemetry::capture::qualify().unwrap();
        phase("capture.qualified");
        let store = coreq::telemetry::Reader::open(&dir.join("db")).unwrap();
        let exported = store.export(0, 0, 1024).unwrap();
        assert!(exported["events"].as_array().unwrap().iter().any(|r| r["observation"]["operation"] == "fixtureq.operation.board.http"
            && r["observation"]["kind"] == "method" && r["observation"]["outcome"] == "ok"));
        assert!(!exported.to_string().contains("private-"));
        assert_eq!(health["lost"], 0);
        phase("capture.drop");drop(_runtime);phase("complete");
    }


    fn http(kind: &Kind, schema: Option<bool>, transport: Transport) -> Box<dyn Board> {
        let mut api = (kind.api)(if kind.name == "github" { "github.com" } else { "gitlab.example" }, "o/r", "test");
        api.transport = Some(transport);
        *api.label_schema.borrow_mut() = schema;
        (kind.board)(api)
    }

    // #1829 A1/A3: exact GitHub/GitLab issue reads retain closure time independently of later edits.
    #[test]
    fn closed_pin_adapters_retain_board_closure() {
        for kind in BOARDS {
            let b=http(kind,Some(true),Box::new(|method,url,_| {
                assert_eq!(method,"GET");
                let v=if url.contains("/members/") { json!({"access_level":20}) } else {
                    assert!(url.ends_with("/issues/7"),"exact pin read: {url}");
                    json!({"number":7,"iid":7,"state":"closed","state_reason":"completed","author_association":"OWNER","author":{"id":1},
                        "labels":["taskq"],"closed_at":"2026-10-28T00:00:00Z","updated_at":"2026-10-29T00:00:00Z"})
                };
                Ok(Reply {status:200,headers:vec![],body:v.to_string(),operation:"read"})
            }));
            let i=b.issue(7).unwrap();
            assert_eq!((i.number,i.open,i.closed),(7,false,iso("2026-10-28T00:00:00Z")));
            assert!(i.trusted && i.closed.unwrap()<i.updated);
        }
    }
    /// A recorded GitHub response; tests/fixtures/github-merge-queue/README.md says how to re-record.
    fn recorded(name: &str) -> Value {
        macro_rules! files { ($($n:literal),*) => { match name {
            $($n => include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/github-merge-queue/", $n, ".json")),)*
            _ => panic!("no recording {name}"),
        } } }
        serde_json::from_str(files!("queue-open", "queue-enqueued", "queue-merged", "removal-merge-conflict", "removal-failed-checks", "removal-manual",
            "enqueue-admitted", "enqueue-conflict", "enqueue-already-queued", "readback-enqueued", "readback-none", "ready-for-review",
            "cancel-queued", "cancel-open", "cancel-merged", "dequeue-done", "node-not-found", "rules-merge-queue", "rules-none", "repo-public",
            "pulls-open", "runs-green", "runs-failed", "runs-checks-queued", "runs-none", "checks-green", "checks-failed", "checks-queued", "status-none")).unwrap()
    }
    fn publication_board(kind: &'static Kind, mode: &str, first_merged: bool, second_merged: Option<bool>, ancestry: Option<bool>) -> (Box<dyn Board>, std::rc::Rc<RefCell<Vec<(String, String)>>>) {
        let calls = std::rc::Rc::new(RefCell::new(vec![]));
        let captured = calls.clone();
        let reads = RefCell::new(0);
        let put = RefCell::new(false);
        let comparisons = RefCell::new(0);
        let direct = mode == "direct";
        let h = http(kind, None, Box::new(move |method, url, _| {
            captured.borrow_mut().push((method.into(), url.into()));
            let result = "a".repeat(40);
            let merged = "b".repeat(40);
            let value = if url.contains("/compare/") || url.contains("/merge_base?") {
                *comparisons.borrow_mut() += 1;
                let initial = *comparisons.borrow() == 1;
                if !initial {
                    assert!(url.ends_with(if direct { &result } else { &merged }), "ancestry must use the published SHA");
                    if ancestry.is_none() { return Err("HTTP 503".into()); }
                }
                let on_main = !initial && ancestry == Some(true);
                if kind.name == "github" { json!({"status": if on_main { "identical" } else { "ahead" }}) }
                else { json!({"id": if on_main { if direct { result } else { merged } } else { "c".repeat(40) }}) }
            } else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
            else if url.ends_with("/status") { json!({"total_count": 0}) }
            else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
            else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": "c".repeat(40)}}) }
            else if url.ends_with("/repos/o/r") { json!({"private": true}) }
            else if url.ends_with("/rules/branches/main") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) }
            else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
            else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": "ff"}) }
            else if url.contains("/git/ref/heads/taskq-") { json!({"object": {"sha": result}}) }
            else if url.ends_with("/git/refs/heads/main") { json!({"object": {"sha": result}}) }
            else if url.contains("/pulls?") { json!([{"number": 7, "head": {"sha": result}}]) }
            else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result}]) }
            else if url.ends_with("/merge") { *put.borrow_mut() = true; json!({"sha": merged, "squash_commit_sha": merged}) }
            else if url.ends_with("/merge_requests/7") && !*put.borrow() { json!({"state": "opened", "detailed_merge_status": "mergeable"}) }
            else if url.ends_with("/pulls/7") || url.ends_with("/merge_requests/7") {
                *reads.borrow_mut() += 1;
                let yes = if *reads.borrow() == 1 { first_merged } else { second_merged.ok_or("HTTP 503")? };
                json!({"merged": yes, "state": if yes { "merged" } else { "opened" }, "merge_commit_sha": merged})
            } else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        (h, calls)
    }

    // Chain: § 7 publication on both boards. Risk: publication uses the wrong SHA or writes twice.
    #[test]
    fn incident_publication_uses_merge_sha_and_keeps_outcomes() {
        for kind in BOARDS {
            for mode in ["direct", "pr"] {
                for ancestry in [Some(true), Some(false), None] {
                    crate::incident::take_detected();
                    let (h, calls) = publication_board(kind, mode, true, Some(true), ancestry);
                    let out = if mode == "direct" { h.publish(1, &"a".repeat(40), mode, "title", "Task: fake") }
                        else { h.resume_publication(1, &"a".repeat(40), "title", "Task: fake", std::path::Path::new("unused"), false) };
                    assert!(matches!(out, Publish::Done(_)));
                    assert_eq!(crate::incident::take_detected(), if ancestry == Some(false) { vec!["publication.not_on_main"] } else { vec![] });
                    assert_eq!(calls.borrow().iter().filter(|(method, _)| method != "GET").count(), 1);
                }
            }
        }
    }

    // Chain: § 7 merged, not closed, on both boards. Risk: a close retried after a merge requeues or merges twice;
    // a PR/MR merged into a release branch (#1039) never closes its task.
    #[test]
    fn a_merged_pr_at_the_result_sha_reconciles_to_one_publication() {
        for (kind, base) in BOARDS.iter().flat_map(|k| [(k, "main"), (k, "release/4.1")]) {
            let calls = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = calls.clone();
            let h = http(kind, None, Box::new(move |method, url, _| {
                captured.borrow_mut().push(method.to_string());
                assert!(!url.contains("main&state=closed") && !url.contains("main&state=merged"), "merged lookup must not filter the base: {url}");
                let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                    else if url.ends_with("/rules/branches/main") { json!([]) }
                    else if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                    else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                    else if url.ends_with("/status") { json!({"total_count": 0}) }
                    else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                    else if url.contains("state=open") { json!([]) }
                    else if url.contains("state=closed") || url.contains("state=merged") {
                        json!([{"number": 6, "iid": 6, "head": {"sha": "d".repeat(40)}, "sha": "d".repeat(40), "merged_at": "x"},
                               {"number": 7, "iid": 7, "head": {"sha": "a".repeat(40)}, "sha": "a".repeat(40), "merged_at": "x", "merge_commit_sha": "b".repeat(40),
                                "base": {"ref": base}, "target_branch": base}])
                    } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let Publish::Done(m) = h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake") else { panic!("not reconciled") };
            let into = if base == "main" { "" } else { " into release/4.1" };
            assert!(m.starts_with(&if kind.name == "github" { format!("merged #7{into} as bbbb") } else { format!("merged !7{into}") }), "{m}");
            assert!(calls.borrow().iter().all(|m| m == "GET"), "no second merge");
        }
    }

    // Chain: § 7 publication with no open task PR and merged PRs only at other heads (#1777, pilotq#84).
    // Risk: `Failed` makes `publication-failed` cancel a PR that is gone, its write is refused and publish retries each pass.
    #[test]
    fn a_result_superseded_by_merged_prs_asks_once() {
        let calls = std::rc::Rc::new(RefCell::new(vec![]));
        let captured = calls.clone();
        let h = http(&github::KIND, None, Box::new(move |method, url, _| {
            captured.borrow_mut().push(method.to_string());
            let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                else if url.ends_with("/rules/branches/main") { json!([]) }
                else if url.contains("state=open") { json!([]) }
                else if url.contains("state=closed") {
                    json!([{"number": 86, "head": {"sha": "d".repeat(40)}, "merged_at": "x"},
                           {"number": 87, "head": {"sha": "a".repeat(40)}, "merged_at": null},
                           {"number": 88, "head": {"sha": "e".repeat(40)}, "merged_at": "x"}])
                } else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        let sha = "a".repeat(40);
        let got = h.resume_publication(1, &sha, "title", "Task: fake", std::path::Path::new("missing-worktree"), false);
        let Publish::Superseded { reason, merged } = got else { panic!("{got:?}") };
        assert_eq!(reason, format!("no open taskq-1 PR at result {sha}; merged taskq-1 PRs: #86 at ddddddd, #88 at eeeeeee"));
        assert_eq!(merged, "#86 at ddddddd, #88 at eeeeeee");
        assert!(calls.borrow().iter().all(|m| m == "GET"), "no publication write");
    }

    // Chain: §7 Result and close verdicts: a task PR merged only at another head closes when it carries the result's patch-id.
    // #1820 (pilotq #106, PR #114): a rebase in review moved the head, the PR merged there and the owner was asked to close it.
    // Retained: a patch that differs (a conflict resolved in the rebase) or an omitted file patch still asks the owner.
    #[test]
    fn a_pr_merged_at_a_rebased_head_with_the_result_patch_closes() {
        let patch = |sp: &str| json!({"files": [{"filename": "src/a.rs", "status": "modified", "patch": format!("@@ -1,2 +1,2 @@
 fn a() {{
-{sp}old();
+{sp}new();")}]});
        for case in ["same", "other", "binary"] {
            let calls = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = calls.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                captured.borrow_mut().push(method.to_string());
                let value = if url.contains("/compare/main...") { json!({"status": "diverged"}) }
                    else if url.ends_with("/rules/branches/main") { json!([]) }
                    else if url.contains("state=open") { json!([]) }
                    else if url.contains("state=closed") {
                        json!([{"number": 114, "head": {"sha": "d".repeat(40)}, "merged_at": "x", "merge_commit_sha": "b".repeat(40), "base": {"ref": "main"}}])
                    } else if url.ends_with(&format!("/commits/{}", "b".repeat(40))) { json!({"parents": [{"sha": "c".repeat(40)}]}) }
                    // the rebased head: other line numbers and indentation, same change
                    else if url.contains(&format!("...{}", "d".repeat(40))) { patch("    ").to_string().replace("-1,2 +1,2", "-40,2 +40,2").parse().unwrap() }
                    else if url.contains(&format!("...{}", "a".repeat(40))) {
                        match case { "same" => patch(""), "other" => patch("").to_string().replace("new()", "merged()").parse().unwrap(),
                            _ => json!({"files": [{"filename": "src/a.rs", "status": "modified"}]}) }
                    }
                    else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                    else if url.ends_with("/status") { json!({"total_count": 0}) }
                    else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let sha = "a".repeat(40);
            let got = h.resume_publication(1, &sha, "title", "Task: fake", std::path::Path::new("missing-worktree"), false);
            match case {
                "same" => assert_eq!(got, Publish::Done(format!("merged #114 as {} (head ddddddd, the result's patch-id); exact-SHA CI green on {}", "b".repeat(40), "d".repeat(40)))),
                _ => assert!(matches!(&got, Publish::Superseded { merged, .. } if merged == "#114 at ddddddd"), "{case}: {got:?}"),
            }
            assert!(calls.borrow().iter().all(|m| m == "GET"), "no publication write");
        }
    }

    // Chain: section 7 intent -> expected-head admission -> merge_group CI -> observed merge receipt.
    // Risks: divergent heads spend retries, lost mutations count as admission, or head CI closes an unmerged task.
    // #1820 (pilotq #106): a PR rebased in review, a conflict after a sibling merged, or a refused enqueue left the task in review,
    // silent, for 50 minutes. Retained: each such pass ends requeued with the PR, files or check named, re-enqueued, or waiting with a reason.
    #[test]
    fn github_queue_qualifies_the_merge_result_and_preserves_unknowns() {
        for scenario in ["new", "draft", "waiting", "merged", "conflict", "failed", "removed", "unknown", "readback", "public", "graphql", "rules-unknown", "merge-ci-missing", "merge-ci-failed", "merge-ci-pending", "merge-push-pending", "merge-push-failed", "merge-check-failed", "enqueue-conflict", "enqueue-race", "merged-flag-lag",
            "head-moved", "conflicting", "conflict-cleared", "conflict-unknown", "enqueue-checks-running", "enqueue-checks-failed"] {
            // #1595: every body is a recorded GitHub response; invented removal reasons once hid GitHub's lowercase codes.
            let merged = scenario == "merged" || scenario.starts_with("merge-");
            let mut state = recorded(if scenario == "waiting" { "queue-enqueued" } else if merged { "queue-merged" } else { "queue-open" });
            // Overlay: GitHub keeps no unmerged PR after a removal, so the open PR takes a recorded removal timeline.
            let removal = match scenario { "conflict" | "conflict-cleared" | "conflict-unknown" => Some(recorded("removal-merge-conflict")), "failed" => Some(recorded("removal-failed-checks")),
                "removed" => Some(recorded("removal-manual")), "merged-flag-lag" => Some(recorded("queue-merged")), _ => None };
            if let Some(r) = removal { state["data"]["node"]["timelineItems"] = r["data"]["node"]["timelineItems"].clone(); }
            // Overlay: GitHub's mergeability after a sibling PR merged (CONFLICTING) or the conflicting entry ahead left (MERGEABLE).
            match scenario { "conflict" | "conflicting" => state["data"]["node"]["mergeable"] = "CONFLICTING".into(), "conflict-cleared" => state["data"]["node"]["mergeable"] = "MERGEABLE".into(), _ => {} }
            // Overlay: no recording holds a non-conflict refusal; only its UNPROCESSABLE type matters, never its wording.
            let mut refusal = recorded("enqueue-already-queued");
            refusal["errors"][0]["message"] = "Required status check build is in progress".into();
            let (runs, mut checks) = match scenario {
                "merge-ci-missing" => (recorded("runs-none"), Value::Null),
                "merge-ci-failed" | "enqueue-checks-failed" => (recorded("runs-failed"), recorded("checks-failed")),
                "merge-ci-pending" | "enqueue-checks-running" => (recorded("runs-checks-queued"), recorded("checks-queued")),
                _ => (recorded("runs-green"), recorded("checks-green")),
            };
            // Overlay: the merge result is the commit the CI recording ran on.
            if let (true, Some(sha)) = (merged, runs["workflow_runs"][0]["head_sha"].as_str()) { state["data"]["node"]["mergeCommit"]["oid"] = sha.into(); }
            // Overlay: #1343 the push suite still running after the merge; #1413 a failed push retest never holds close; a failed merge_group check does.
            let flip = match scenario { "merge-push-pending" => Some(("build", "in_progress", Value::Null)), "merge-push-failed" => Some(("build", "completed", json!("failure"))),
                "merge-check-failed" => Some(("plan", "completed", json!("failure"))), _ => None };
            if let Some((name, status, conclusion)) = flip {
                let run = checks["check_runs"].as_array_mut().unwrap().iter_mut().find(|c| c["name"] == name).unwrap();
                run["status"] = status.into();
                run["conclusion"] = conclusion;
            }
            let head = state["data"]["node"]["headRefOid"].as_str().unwrap().to_string();
            let merge = state["data"]["node"]["mergeCommit"]["oid"].as_str().unwrap_or_default().to_string();
            // Overlay: the PR list names the recorded PR state's head.
            let mut pulls = recorded("pulls-open");
            pulls[0]["head"]["sha"] = head.clone().into();
            // Overlay: #106 the reviewer pushed a rebased head after the result.
            let moved = "f".repeat(40);
            if scenario == "head-moved" { pulls[0]["head"]["sha"] = moved.clone().into(); }
            pulls[0]["draft"] = (scenario == "draft").into();
            let pr = pulls[0]["node_id"].clone();
            let writes = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = writes.clone();
            let rules = std::rc::Rc::new(RefCell::new(0));
            let ruled = rules.clone();
            let sent = head.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                if method != "GET" { captured.borrow_mut().push(body.clone().unwrap_or_default()); }
                let value = if url.contains("/compare/") {
                    // Overlay: compare bodies with a merge base and changed files; the PR and main both changed src/service.rs.
                    let base = "c".repeat(40);
                    let files = if url.contains(&format!("/compare/{base}...main")) { json!([{"filename": "src/service.rs"}, {"filename": "docs/x.md"}]) }
                        else { json!([{"filename": "README.md"}, {"filename": "src/service.rs"}]) };
                    json!({"status": if !merge.is_empty() && url.ends_with(&merge) { "identical" } else { "diverged" }, "merge_base_commit": {"sha": base}, "files": files})
                }
                    else if url.ends_with("/rules/branches/main") { *ruled.borrow_mut() += 1; if scenario == "rules-unknown" { return Err("HTTP 403".into()); } recorded(if scenario == "public" { "rules-none" } else { "rules-merge-queue" }) }
                    else if url.ends_with("/repos/o/r") { assert_eq!(scenario, "public"); recorded("repo-public") }
                    else if url.contains("/pulls?") { pulls.clone() }
                    else if url.ends_with("/graphql") {
                        let body = body.unwrap();
                        let query = body["query"].as_str().unwrap();
                        assert_eq!(body["variables"]["id"], pr);
                        if scenario == "graphql" { recorded("node-not-found") }
                        else if query.contains("enqueuePullRequest") {
                            assert_eq!(body["variables"]["sha"], head);
                            match scenario { "enqueue-checks-running" | "enqueue-checks-failed" => refusal.clone(),
                                _ => recorded(match scenario { "enqueue-conflict" => "enqueue-conflict", "enqueue-race" => "enqueue-already-queued", _ => "enqueue-admitted" }) }
                        } else if query.contains("markPullRequestReadyForReview") { recorded("ready-for-review") }
                        else if !query.contains("timelineItems") { recorded(if scenario == "readback" { "readback-none" } else { "readback-enqueued" }) }
                        else { state.clone() }
                    }
                    else if url.contains("/actions/runs?") { assert!(url.contains(&merge)); runs.clone() }
                    else if url.contains("/check-runs") { assert!(url.contains(&merge)); checks.clone() }
                    else if url.ends_with("/status") { assert!(url.contains(&merge)); recorded("status-none") }
                    else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let prior = !matches!(scenario, "new" | "draft" | "readback" | "public" | "graphql" | "enqueue-conflict" | "enqueue-race" | "head-moved" | "conflicting" | "enqueue-checks-running" | "enqueue-checks-failed");
            let got = h.resume_publication(1, &sent, "title", "Task: invented", std::path::Path::new("missing-worktree"), prior);
            match scenario {
                "new" | "draft" | "waiting" | "conflict-cleared" => assert!(matches!(got, Publish::Enqueued(_)), "{scenario}: {got:?}"),
                "conflict" | "enqueue-conflict" | "conflicting" => assert_eq!(got, Publish::MainMoved("PR #1590 conflicts with main; conflict files: src/service.rs".into()), "{scenario}"),
                "head-moved" => assert_eq!(got, Publish::MainMoved(format!("PR #1590 head {moved:.7} is not the result {sent:.7}; deliver the PR head as a new result"))),
                "enqueue-checks-running" => assert!(matches!(&got, Publish::Pending(m) if m.starts_with("merge queue did not admit PR #1590: Required status check build is in progress; CI ") && m.contains(" pending on ")), "{got:?}"),
                "conflict-unknown" => assert_eq!(got, Publish::Pending("merge queue removed PR #1590 as merge_conflict; mergeability not computed yet".into())),
                "enqueue-checks-failed" => assert!(matches!(&got, Publish::Failed(m) if m.starts_with("merge queue did not admit PR #1590: ")), "{got:?}"),
                "merged" | "merge-push-pending" => assert!(matches!(&got, Publish::Done(m) if !m.contains("push CI")), "{scenario}: {got:?}"),
                "merge-push-failed" => assert!(matches!(&got, Publish::Done(m) if m.ends_with("merge-result CI green; push CI build failure ignored")), "{got:?}"),
                // #1413: merged with failed merge-result CI or removed from the queue: only the owner can unblock it
                "merge-ci-failed" | "merge-check-failed" | "removed" => assert!(matches!(got, Publish::Blocked(_)), "{scenario}: {got:?}"),
                "failed" => assert!(matches!(got, Publish::Failed(_)), "{got:?}"),
                // #1489: reason merged before the merged flag rereads next pass instead of asking; an enqueue race rereads too
                _ => assert!(matches!(got, Publish::Pending(_)), "{scenario}: {got:?}"),
            }
            if scenario == "waiting" {
                // #1359: a waiting publication polls the queue each pass; main rules stay read once per process.
                assert!(matches!(h.resume_publication(1, &sent, "title", "Task: invented", std::path::Path::new("missing-worktree"), prior), Publish::Enqueued(_)));
                assert_eq!(*rules.borrow(), 1, "main rules reread");
            }
            assert!(!writes.borrow().iter().any(|b| b.get("merge_method").is_some()), "queue never calls REST merge");
            assert_eq!(writes.borrow().iter().filter(|b| b["query"].as_str().is_some_and(|q| q.contains("enqueuePullRequest"))).count(), usize::from(matches!(scenario, "new" | "draft" | "readback" | "enqueue-conflict" | "enqueue-race" | "conflict-cleared" | "enqueue-checks-running" | "enqueue-checks-failed")), "{scenario}: one enqueue call");
        }
    }

    // Chain: section 7 rule 4 publish once -> main moved after the push, no conflict -> enqueue at the result SHA -> merge_group CI on the merge result -> close (#1842).
    // #1842: workers rebased and rechecked up to 5 times per attempt because main moved. Retained: a clean PR behind a moved main
    // is enqueued at the unchanged result, never rebased or pushed, and closes on the merge result's CI.
    #[test]
    fn a_moved_main_without_conflict_publishes_the_result_unrebased() {
        let state = std::rc::Rc::new(RefCell::new(recorded("queue-open")));
        let head = state.borrow()["data"]["node"]["headRefOid"].as_str().unwrap().to_string();
        state.borrow_mut()["data"]["node"]["mergeable"] = "MERGEABLE".into();
        let runs = recorded("runs-green");
        let merge = runs["workflow_runs"][0]["head_sha"].as_str().unwrap().to_string();
        let mut pulls = recorded("pulls-open");
        pulls[0]["head"]["sha"] = head.clone().into();
        let writes = std::rc::Rc::new(RefCell::new(vec![]));
        let (captured, read, sent, merged) = (writes.clone(), state.clone(), head.clone(), merge.clone());
        let h = http(&github::KIND, None, Box::new(move |method, url, body| {
            // GraphQL reads are POSTs too; only mutations and REST writes change the PR or its branch.
            let read_only = body.as_ref().is_some_and(|b| b["query"].as_str().is_some_and(|q| q.starts_with("query")));
            if method != "GET" && !read_only { captured.borrow_mut().push(format!("{method} {url} {}", body.clone().unwrap_or_default())); }
            // Overlay: main moved after the push; the result is neither on main nor its ancestor.
            let value = if url.contains("/compare/") { json!({"status": if url.ends_with(&merged) { "identical" } else { "diverged" }}) }
                else if url.ends_with("/rules/branches/main") { recorded("rules-merge-queue") }
                else if url.contains("/pulls?") { pulls.clone() }
                else if url.ends_with("/graphql") {
                    let body = body.unwrap();
                    let query = body["query"].as_str().unwrap();
                    if query.contains("enqueuePullRequest") { assert_eq!(body["variables"]["sha"], sent, "enqueued at the result SHA"); recorded("enqueue-admitted") }
                    else if !query.contains("timelineItems") { recorded("readback-enqueued") }
                    else { read.borrow().clone() }
                }
                else if url.contains("/actions/runs?") { assert!(url.contains(&merged), "CI read on the merge result: {url}"); runs.clone() }
                else if url.contains("/check-runs") { assert!(url.contains(&merged), "CI read on the merge result: {url}"); recorded("checks-green") }
                else if url.ends_with("/status") { recorded("status-none") }
                else { panic!("unexpected {method} {url}") };
            Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
        }));
        // A rebase would need the task worktree; this host has none, so any rebase attempt fails the pass.
        let root = std::path::Path::new("missing-worktree");
        assert!(matches!(h.resume_publication(1, &head, "title", "Task: invented", root, false), Publish::Enqueued(_)));
        assert!(writes.borrow().iter().all(|w| w.contains("enqueuePullRequest")), "only the enqueue writes: {:?}", writes.borrow());
        // Overlay: the merge queue merged the PR at the result head; the merge result is the commit the CI recording ran on.
        let mut done = recorded("queue-merged");
        done["data"]["node"]["headRefOid"] = head.clone().into();
        done["data"]["node"]["mergeCommit"]["oid"] = merge.clone().into();
        *state.borrow_mut() = done;
        let got = h.resume_publication(1, &head, "title", "Task: invented", root, true);
        assert!(matches!(&got, Publish::Done(m) if m.contains(&format!("as {merge}; merge-result CI green"))), "{got:?}");
        assert_eq!(writes.borrow().len(), 1, "one enqueue, no rebase or push: {:?}", writes.borrow());
    }

    // Chain: section 7 private fallback real Git fetch/rebase/leased push, or conflict/dirty preservation.
    // An HTTP-only fixture cannot prove preservation of the worktree or the pushed branch.
    #[test]
    fn private_fallback_rebases_its_owned_worktree_and_preserves_conflicts() {
        for mode in ["clean", "conflict", "dirty"] {
            let dir = std::env::temp_dir().join(format!("taskq-fallback-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&dir).unwrap();
            let root = dir.join("project");
            let remote = dir.join("remote.git");
            let git = |at: &std::path::Path, args: &[&str]| {
                let out = std::process::Command::new("git").current_dir(at).args(args).env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
                assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
                String::from_utf8(out.stdout).unwrap().trim().to_string()
            };
            git(&dir, &["init", "--bare", remote.to_str().unwrap()]);
            git(&dir, &["init", "-b", "main", root.to_str().unwrap()]);
            for (key, value) in [("user.name", "Invented fixture"), ("user.email", "fixture@example.invalid"), ("commit.gpgsign", "false"), ("core.autocrlf", "false")] { git(&root, &["config", key, value]); }
            std::fs::write(root.join("base"), "base\n").unwrap();
            git(&root, &["add", "."]); git(&root, &["commit", "-m", "base"]);
            git(&root, &["remote", "add", "origin", remote.to_str().unwrap()]);
            git(&root, &["push", "origin", "main"]);
            git(&root, &["worktree", "add", "-b", "taskq-1", ".worktrees/taskq-1", "origin/main"]);
            let wt = root.join(".worktrees/taskq-1");
            std::fs::write(wt.join(if mode == "conflict" { "base" } else { "worker" }), "worker\n").unwrap();
            git(&wt, &["add", "."]); git(&wt, &["commit", "-m", "worker"]);
            // The invented token must reach GitHub credentials only, including inside Git's rebase subprocess.
            std::fs::write(root.join(".git/hooks/pre-rebase"), r#"#!/bin/sh
board=$(printf 'protocol=https\nhost=github.com\n\n' | git credential fill) || exit 1
case "$board" in *password=test*) ;; *) exit 1;; esac
other=$(printf 'protocol=https\nhost=elsewhere.invalid\n\n' | git credential fill 2>/dev/null)
case "$other" in *password=test*) exit 1;; esac
exit 0
"#).unwrap();
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(root.join(".git/hooks/pre-rebase"), std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let sha = git(&wt, &["rev-parse", "HEAD"]);
            git(&wt, &["push", "origin", "taskq-1"]);
            std::fs::write(root.join(if mode == "conflict" { "base" } else { "main" }), "main\n").unwrap();
            git(&root, &["add", if mode == "conflict" { "base" } else { "main" }]); git(&root, &["commit", "-m", "main"]);
            git(&root, &["push", "origin", "main"]);
            // Invented board URL, rewritten to this test's local bare remote; no network or credentials.
            git(&root, &["config", "remote.origin.url", "https://github.com/o/r.git"]);
            git(&root, &["config", &format!("url.{}.insteadOf", remote.to_str().unwrap().replace('\\', "/")), "https://github.com/o/r.git"]);
            if mode == "dirty" { std::fs::write(wt.join("untracked"), "keep me").unwrap(); }
            let original = sha.clone();
            let bare = remote.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, _| {
                let value = if url.contains("/compare/") { json!({"status": "diverged"}) }
                    else if url.contains("/pulls?") { json!([{"number": 7, "head": {"sha": original}}]) }
                    else if url.ends_with("/rules/branches/main") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) }
                    else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
                    else if url.ends_with("/repos/o/r") { json!({"private": true}) }
                    else if url.ends_with("/git/ref/heads/taskq-1") {
                        let out = std::process::Command::new("git").arg("--git-dir").arg(&bare).args(["rev-parse", "refs/heads/taskq-1"]).output().unwrap();
                        json!({"object": {"sha": String::from_utf8(out.stdout).unwrap().trim()}})
                    } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let out = h.resume_publication(1, &sha, "title", "Task: invented", &root, false);
            match mode {
                "clean" => {
                    let Publish::Rebased(head) = out else { panic!("{out:?}") };
                    assert_ne!(head, sha);
                    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), head);
                    assert_eq!(git(&root, &["ls-remote", "origin", "refs/heads/taskq-1"]).split_whitespace().next(), Some(head.as_str()));
                    git(&wt, &["merge-base", "--is-ancestor", "origin/main", "HEAD"]);
                }
                "conflict" => {
                    assert!(matches!(out, Publish::MainMoved(_)), "{out:?}");
                    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), sha);
                    assert!(git(&wt, &["status", "--porcelain"]).is_empty());
                }
                _ => {
                    assert!(matches!(out, Publish::Pending(_)), "{out:?}");
                    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), sha);
                    assert_eq!(std::fs::read_to_string(wt.join("untracked")).unwrap(), "keep me");
                }
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    // Chain: section 7 queue cancellation -> dequeue readback; unknown/merged state never claims cancellation.
    #[test]
    fn github_queue_cancellation_reads_back_before_releasing_the_task() {
        for scenario in ["cancel", "absent", "merged", "unknown", "lost-readback"] {
            let removed = std::rc::Rc::new(RefCell::new(false));
            let done = removed.clone();
            let pulls = recorded("pulls-open");
            let head = pulls[0]["head"]["sha"].as_str().unwrap().to_string();
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                let value = if url.contains("/pulls?") { pulls.clone() }
                    else if url.ends_with("/graphql") {
                        if body.as_ref().unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest") {
                            // #1472: GitHub's DequeuePullRequestInput names the PR `id`; `pullRequestId` fails missingRequiredInputObjectAttribute.
                            assert!(body.as_ref().unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest(input: {id: $id})"), "{body:?}");
                            assert_eq!(body.as_ref().unwrap()["variables"]["id"], pulls[0]["node_id"]);
                            *done.borrow_mut() = true;
                            recorded("dequeue-done")
                        } else if scenario == "unknown" { recorded("node-not-found") }
                        else { recorded(match scenario { "merged" => "cancel-merged", "absent" => "cancel-open", _ if *done.borrow() && scenario != "lost-readback" => "cancel-open", _ => "cancel-queued" }) }
                    } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let result = h.cancel_publication(1, &head);
            assert_eq!(result.is_ok(), matches!(scenario, "cancel" | "absent"), "{scenario}: {result:?}");
            assert_eq!(*removed.borrow(), matches!(scenario, "cancel" | "lost-readback"));
        }
    }

    // Chain: section 7 close without merge -> dequeue, one comment, unmerged close; merged/unknown PRs stay untouched.
    #[test]
    fn github_task_close_closes_its_open_pr_once_and_keeps_the_branch() {
        // scenario -> (result ok, PRs closed, comments posted, dequeued)
        for (scenario, ok, closed, posted, dequeued) in [("open", true, 1, 1, false), ("queued", true, 1, 1, true), ("commented", true, 1, 0, false),
            ("closed", true, 0, 0, false), ("merged", false, 0, 0, false), ("unknown", false, 0, 0, false)] {
            let calls = std::rc::Rc::new(RefCell::new(vec![]));
            let seen = calls.clone();
            let queued = std::rc::Rc::new(RefCell::new(scenario == "queued"));
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                seen.borrow_mut().push(format!("{method} {}", url.rsplit_once("/o/r").map_or(url, |x| x.1)));
                let value = if url.contains("/pulls?") {
                    assert!(url.contains("head=o:taskq-1&state=open") && !url.contains("base="), "{url}");
                    if scenario == "closed" { json!([]) } else { recorded("pulls-open") }
                } else if url.ends_with("/graphql") {
                    if body.as_ref().unwrap()["query"].as_str().unwrap().contains("dequeuePullRequest") {
                        *queued.borrow_mut() = false;
                        recorded("dequeue-done")
                    } else if scenario == "unknown" { recorded("node-not-found") }
                    else { recorded(if scenario == "merged" { "cancel-merged" } else if *queued.borrow() { "cancel-queued" } else { "cancel-open" }) }
                } else if url.contains("/issues/1590/comments") && method == "GET" {
                    if scenario == "commented" { json!([{"id": 1, "body": "Task #1 closed without merging this PR: earlier"}]) } else { json!([]) }
                } else if url.contains("/issues/1590/comments") {
                    assert!(body.as_ref().unwrap()["body"].as_str().unwrap().starts_with("Task #1 closed without merging this PR: superseded\nBranch `taskq-1` kept. Task: https://github.com/o/r/issues/1"));
                    json!({"id": 2})
                } else if url.ends_with("/pulls/1590") && method == "PATCH" {
                    assert_eq!(body.as_ref().unwrap(), &json!({"state": "closed"}));
                    json!({"state": "closed", "merged": false})
                } else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let result = h.close_task_prs(1, "superseded");
            let calls = calls.borrow();
            let count = |p: &str| calls.iter().filter(|c| c.starts_with(p)).count();
            assert_eq!(result.is_ok(), ok, "{scenario}: {result:?}");
            let graphql = count("POST") - count("POST /issues");
            assert_eq!((count("PATCH /pulls/1590"), count("POST /issues/1590/comments"), graphql), (closed, posted, match (scenario, dequeued) { ("closed", _) => 0, (_, true) => 3, _ => 1 }), "{scenario}: {calls:?}");
            assert!(!calls.iter().any(|c| c.starts_with("DELETE")), "{scenario}: branch never deleted");
        }
    }

    // Chain: section 7 unchanged GitLab stale-main/merge fence; GitHub has queue coverage above.
    #[test]
    fn gitlab_pr_merges_only_onto_the_main_its_ci_built() {
        // (rebased, how main refuses stale merges, main moves before the merge) -> outcome
        let cases = [
            (false, "ruleset:never", false, "Failed"),
            (true, "none", false, "Pending"),
            (true, "ruleset:always", false, "Pending"),
            (true, "ruleset:pull_requests_only", false, "Pending"),
            (true, "classic:admins exempt", false, "Pending"),
            (true, "ruleset:never", true, "Failed"),
            (true, "classic:enforced", true, "Failed"),
        ];
        for (rebased, guard, moved, want) in cases {
            for kind in [&gitlab::KIND] {
                let calls = std::rc::Rc::new(RefCell::new(vec![]));
                let captured = calls.clone();
                let enforced = matches!(guard, "ruleset:never" | "classic:enforced");
                let h = http(kind, None, Box::new(move |method, url, _| {
                    captured.borrow_mut().push(method.to_string());
                    let result = "a".repeat(40);
                    let value = if url.contains("/compare/") { json!({"status": if rebased { "ahead" } else { "diverged" }}) }
                        else if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                        else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": if rebased { "c" } else { "d" }.repeat(40)}}) }
                        else if url.ends_with("/rules/branches/main") {
                            if guard.starts_with("ruleset:") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) } else { json!([]) }
                        }
                        else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": guard.trim_start_matches("ruleset:")}) }
                        else if url.ends_with("/branches/main/protection") {
                            if !guard.starts_with("classic:") { return Err("HTTP 403 Resource not accessible by personal access token".into()) }
                            json!({"required_status_checks": {"strict": true}, "enforce_admins": {"enabled": guard == "classic:enforced"}})
                        }
                        else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": if enforced { "ff" } else { "merge" }}) }
                        else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                        else if url.ends_with("/status") { json!({"total_count": 0}) }
                        else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                        else if url.contains("/pulls?") { json!([{"number": 7, "head": {"sha": result}}]) }
                        else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result}]) }
                        else if url.ends_with("/merge_requests/7") { json!({"detailed_merge_status": "mergeable"}) }
                        else if url.ends_with("/merge") && moved { return Err("HTTP 405 Head branch is out of date".into()) }
                        else { panic!("unexpected {method} {url}") };
                    Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
                }));
                let got = match h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake") {
                    Publish::Failed(m) => { assert!(m.contains(if moved { "HTTP 405" } else { "not rebased on current main" }), "{m}"); "Failed" }
                    Publish::Pending(_) => "Pending",
                    Publish::Done(m) => panic!("merged: {m}"),
                    other => panic!("unexpected {other:?}"),
                };
                assert_eq!(got, want, "{} rebased={rebased} guard={guard} moved={moved}", kind.name);
                assert_eq!(calls.borrow().iter().filter(|m| *m != "GET").count(), usize::from(moved), "merge only behind the fence");
            }
        }
    }

    // Chain: § 7 GitLab `pr` close vs asynchronous mergeability (#1687). Risk: a merge sent while GitLab rechecks the MR
    // gets 422 'Branch cannot be merged' and requeues a reviewed result; a real blocker merges or loses its reason.
    #[test]
    fn gitlab_merge_waits_for_mergeability() {
        // (merge statuses read in order, PUT merge refusal) -> outcome, merge writes
        let cases: [(&[&str], Option<&str>, &str, usize); 7] = [
            (&["unchecked", "checking", "mergeable"], None, "Done", 1),
            (&["checking"], None, "Pending", 0),
            (&["conflict"], None, "Failed:conflict", 0),
            (&["discussions_not_resolved"], None, "Failed:discussions_not_resolved", 0),
            (&["ci_must_pass"], None, "Failed:ci_must_pass", 0),
            (&["mergeable", "checking"], Some("HTTP 422 Branch cannot be merged"), "Pending", 1),
            (&["mergeable", "conflict"], Some("HTTP 422 Branch cannot be merged"), "Failed:conflict", 1),
        ];
        for (statuses, refusal, want, writes) in cases {
            let statuses: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
            let refusal = refusal.map(String::from);
            let reads = std::rc::Rc::new(RefCell::new(0usize));
            let seen = reads.clone();
            let calls = std::rc::Rc::new(RefCell::new(0usize));
            let captured = calls.clone();
            let merged = std::rc::Rc::new(RefCell::new(false));
            let h = http(&gitlab::KIND, None, Box::new(move |method, url, _| {
                let result = "a".repeat(40);
                let value = if url.contains("/merge_base?") { json!({"id": "c".repeat(40)}) }
                    else if url.ends_with("/repository/branches/main") { json!({"commit": {"id": "c".repeat(40)}}) }
                    else if url.ends_with("/projects/o%2Fr") { json!({"merge_method": "ff"}) }
                    else if url.contains("/repository/commits/") { json!({"last_pipeline": {"status": "success"}}) }
                    else if url.contains("/merge_requests?") { json!([{"iid": 7, "sha": result}]) }
                    else if url.ends_with("/merge") {
                        *captured.borrow_mut() += 1;
                        if let Some(e) = &refusal { return Err(e.clone()) }
                        *merged.borrow_mut() = true;
                        json!({"squash_commit_sha": "b".repeat(40)})
                    }
                    else if url.ends_with("/merge_requests/7") {
                        if *merged.borrow() { json!({"state": "merged"}) } else {
                            let i = *seen.borrow();
                            *seen.borrow_mut() += 1;
                            json!({"state": "opened", "detailed_merge_status": statuses[i.min(statuses.len() - 1)]})
                        }
                    }
                    else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let got = match h.publish(1, &"a".repeat(40), "pr", "title", "Task: fake") {
                Publish::Done(_) => "Done".to_string(),
                Publish::Pending(_) => "Pending".to_string(),
                Publish::Failed(m) => format!("Failed:{}", m.split(": ").nth(1).unwrap_or("")),
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(got, want, "{want}");
            assert_eq!(*calls.borrow(), writes, "{want}: merge writes");
            if want == "Pending" && writes == 0 { assert_eq!(*reads.borrow(), gitlab::MERGE_STATUS_READS as usize, "bounded wait"); }
        }
    }

    // Chain: § 7 `pr` publication of a draft PR. Risk: GitHub refuses to merge a draft (HTTP 405), so close never publishes.
    #[test]
    fn private_fallback_readies_a_draft_then_merges_the_checked_head() {
        for undrafted in [true, false] {
            let writes = std::rc::Rc::new(RefCell::new(vec![]));
            let captured = writes.clone();
            let h = http(&github::KIND, None, Box::new(move |method, url, body| {
                let (result, merged) = ("a".repeat(40), "b".repeat(40));
                if method != "GET" { captured.borrow_mut().push(url.rsplit_once(".com").map_or(url, |x| x.1).to_string()); }
                let value = if url.contains("/compare/") { json!({"status": "ahead"}) }
                    else if url.contains("/check-runs") { json!({"check_runs": [{"status": "completed", "conclusion": "success"}]}) }
                    else if url.ends_with("/status") { json!({"total_count": 0}) }
                    else if url.ends_with("/repos/o/r") { json!({"private": true}) }
            else if url.ends_with("/rules/branches/main") { json!([{"type": "required_status_checks", "ruleset_id": 1, "parameters": {"strict_required_status_checks_policy": true}}]) }
                    else if url.ends_with("/rulesets/1") { json!({"current_user_can_bypass": "never"}) }
                    else if url.contains("/pulls?") { json!([{"number": 7, "node_id": "PR_7", "draft": true, "head": {"sha": result}}]) }
                    else if url == "https://api.github.com/graphql" {
                        assert_eq!(body.unwrap()["variables"]["id"], "PR_7");
                        if undrafted { json!({"data": {"markPullRequestReadyForReview": {"pullRequest": {"isDraft": false}}}}) }
                        else { json!({"data": null, "errors": [{"message": "forbidden"}]}) }
                    }
                    else if url.ends_with("/pulls/7/merge") { assert_eq!(body.unwrap()["sha"], result); json!({"sha": merged}) }
                    else if url.ends_with("/pulls/7") { json!({"merged": true, "merge_commit_sha": merged}) }
                    else { panic!("unexpected {method} {url}") };
                Ok(Reply { status: 200, headers: vec![], body: value.to_string(), operation: operation(method) })
            }));
            let out = h.resume_publication(1, &"a".repeat(40), "title", "Task: fake", std::path::Path::new("unused"), false);
            if undrafted {
                assert!(matches!(&out, Publish::Done(m) if m.starts_with("merged #7 as bbbb")));
                assert_eq!(*writes.borrow(), ["/graphql", "/repos/o/r/pulls/7/merge"], "ready before merge");
            } else {
                assert!(matches!(&out, Publish::Pending(m) if m.contains("still a draft")));
                assert_eq!(*writes.borrow(), ["/graphql"], "no merge of a draft");
            }
        }
    }

    // Chain: § 7 publication readback. Risk: one flaky read reports a lost publication.
    #[test]
    fn incident_publication_readback_requires_two_negative_reads() {
        for kind in BOARDS {
            for second in [Some(false), Some(true), None] {
                crate::incident::take_detected();
                let (h, calls) = publication_board(kind, "pr", false, second, Some(true));
                assert!(matches!(h.resume_publication(1, &"a".repeat(40), "title", "Task: fake", std::path::Path::new("unused"), false), Publish::Pending(_)), "reporting preserves first read outcome");
                assert_eq!(crate::incident::take_detected(), if second == Some(false) { vec!["publication.readback"] } else { vec![] });
                let readback = calls.borrow().iter().skip_while(|(m, _)| m == "GET").filter(|(m, u)| m == "GET" && (u.ends_with("/pulls/7") || u.ends_with("/merge_requests/7"))).count();
                assert_eq!(readback, 2);
                assert_eq!(calls.borrow().iter().filter(|(method, _)| method != "GET").count(), 1);
            }
        }
    }

    // Chain: incident adapter.bad_json. Risk: HTTP failures reported as bad JSON.
    #[test]
    fn incident_bad_json_is_only_a_successful_response() {
        for fail in [false, true] {
            crate::incident::take_detected();
            let mut h = (github::KIND.api)("github.com", "o/r", "fake");
            h.transport = Some(Box::new(move |_, _, _| if fail { Err("HTTP 503 malformed response".into()) }
                else { Ok(Reply { status: 200, headers: vec![], body: "private invalid JSON".into(), operation: "read" }) }));
            assert!(h.call("GET", "/issues", None).and_then(|r| r.json()).is_err());
            assert_eq!(crate::incident::take_detected(), if fail { vec![] } else { vec!["adapter.bad_json"] });
        }
    }

    // Chain: § 1 `Depends on <repo>#N` adapters (#1126). Risk: a wrong repository path or a lost close reason makes a
    // dependency of another repository never or wrongly done.
    #[test]
    fn foreign_dependency_reads_sibling_repositories() {
        for (kind, body, api, web, reason) in [
            (&github::KIND, r#"{"number":36,"state":"closed","state_reason":"not_planned"}"#, "https://api.github.com/repos/o/dashq/issues/36",
                "https://github.com/o/dashq/issues/36", Some("not_planned")),
            (&gitlab::KIND, r#"{"iid":36,"state":"closed"}"#, "https://gitlab.example/api/v4/projects/o%2Fdashq/issues/36",
                "https://gitlab.example/o/dashq/-/issues/36", None),
        ] {
            let urls = std::rc::Rc::new(RefCell::new(vec![]));
            let seen = urls.clone();
            let b = http(kind, Some(true), Box::new(move |_, url, _| {
                seen.borrow_mut().push(url.to_string());
                Ok(Reply { status: 200, headers: vec![], body: body.into(), operation: "read" })
            }));
            let i = b.foreign("dashq", 36).unwrap();
            assert_eq!((i.number, i.open, i.reason.as_deref()), (36, false, reason));
            assert_eq!((urls.borrow()[0].as_str(), b.foreign_url("dashq", 36)), (api, web.to_string()));
            assert_eq!(b.foreign_url("x/y", 1).replace("x/y", "o/r"), b.url(1), "a full path is used as written");
        }
        // R12: a reply that is not that issue in a known state stays unknown, never done
        for (kind, body) in [(&github::KIND, "{}"), (&github::KIND, r#"{"number":37,"state":"closed"}"#), (&github::KIND, r#"{"number":36,"state":"gone"}"#),
            (&gitlab::KIND, "{}"), (&gitlab::KIND, r#"{"iid":36}"#), (&gitlab::KIND, r#"{"iid":36,"state":"open"}"#)] {
            let b = http(kind, Some(true), Box::new(move |_, _, _| Ok(Reply { status: 200, headers: vec![], body: body.into(), operation: "read" })));
            assert_eq!(b.foreign("dashq", 36).unwrap_err(), "malformed reply for issue 36", "{body}");
        }
    }

    // Chain: A3 forge response -> profile report. Risk: HTTP/GraphQL failures count as successes or pauses send again.
    #[test]
    fn forge_outcomes_reach_profile_report() {
        crate::profile::take_report();
        for (status, body, limited) in [(200, "{}", false), (204, "", false), (404, "private response", false),
            (503, "private response HTTP 404", false), (200, "{\"errors\":[{\"type\":\"RATE_LIMITED\"}]}", true)] {
            let mut api = (github::KIND.api)("github.com", "o/r", "private token");
            api.transport = Some(Box::new(move |_, _, _| Ok(Reply { status, body: body.into(), operation: "read",
                headers: if limited { vec![("retry-after".into(), "60".into())] } else { vec![] } })));
            assert_eq!(api.call("POST", "https://api.github.com/graphql", None).is_ok(), status < 300 && !limited);
            if limited {
                assert!(api.call("POST", "https://api.github.com/graphql", None).err().unwrap().contains("paused until"));
                assert_eq!(api.budget.borrow().total(), 1);
            }
        }
        let report = crate::profile::take_report();
        assert!(report.contains("| forge.call | 5 |"), "{report}");
        assert!(report.contains("| 2/3 | 60.00 |"), "{report}");
        assert!(report.contains("http4xx:2, http5xx:1"), "{report}");
        assert!(!report.contains("private"));
    }

    // Chain: § 4 board budget, #1356. Risk: a GraphQL RATE_LIMIT reply without budget headers starts no pause, so every
    // pass of every project sends again until the reset.
    #[test]
    fn graphql_rate_limit_reply_pauses_board_requests() {
        // the exact reply body of 2026-10-20 08:40Z
        const BODY: &str = r#"{"errors":[{"type":"RATE_LIMIT","code":"graphql_rate_limit","message":"API rate limit already exceeded for user ID 1640869."}]}"#;
        for status in [200, 403] {
            let sent = std::rc::Rc::new(RefCell::new(0));
            let count = sent.clone();
            let mut api = (github::KIND.api)("github.com", "o/r", "private token");
            api.transport = Some(Box::new(move |_, _, _| {
                *count.borrow_mut() += 1;
                Ok(Reply { status, body: BODY.into(), operation: "read", headers: vec![] })
            }));
            let first = api.call("POST", "https://api.github.com/graphql", None).err().unwrap();
            assert!(first.contains("HTTP 403 rate limit: board requests paused until"), "{status}: {first}");
            assert!(api.call("POST", "https://api.github.com/graphql", None).err().unwrap().contains("paused until"));
            assert!(api.budget.borrow().paused_until().is_some());
            assert_eq!(*sent.borrow(), 1, "{status}: no GraphQL request is sent while paused");
        }
    }

    // Chain: § 4 board budget. Risk: a rate-limit reply pauses only the process that got it; PM, worker and DashQ
    // CLI processes of the same install keep sending until the reset and stretch the board's block.
    #[test]
    fn rate_limit_pause_stops_every_process_of_the_install() {
        let install = std::env::temp_dir().join(format!("taskq-pause-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&install).unwrap();
        let sent = std::rc::Rc::new(RefCell::new(0));
        let process = |status: u16| {
            let count = sent.clone();
            let mut api = (github::KIND.api)("github.com", "o/r", "private token");
            api.pause = Some(crate::requests::pause_file(&install, &api.root, "private token"));
            api.transport = Some(Box::new(move |_, _, _| {
                *count.borrow_mut() += 1;
                Ok(Reply { status, body: "{}".into(), operation: "read", headers: vec![("retry-after".into(), "60".into())] })
            }));
            api
        };
        assert!(process(429).call("GET", "/issues", None).err().unwrap().contains("paused until"));
        let other = process(200);
        assert!(other.call("GET", "/issues", None).err().unwrap().contains("paused until"));
        assert!(other.budget.borrow().paused_until().is_some(), "service status shows the install's pause");
        assert_eq!(*sent.borrow(), 1, "no process of the install sends while paused");
        let mut elsewhere = process(200);
        elsewhere.pause = Some(crate::requests::pause_file(&install, &elsewhere.root, "other token"));
        assert!(elsewhere.call("GET", "/issues", None).is_ok(), "another credential keeps its own budget");
        std::fs::write(crate::requests::pause_file(&install, "https://api.github.com", "private token"), "1").unwrap();
        assert!(process(200).call("GET", "/issues", None).is_ok(), "a past reset holds nothing");
        let _ = std::fs::remove_dir_all(&install);
    }

    // Chain: board list/readback -> malformed successful HTTP page -> explicit failure, never an empty queue/catalog.
    #[test]
    fn malformed_list_is_not_an_empty_board() {
        for kind in [&github::KIND, &gitlab::KIND] {
            for body in ["{}", "null", "\"private-error\""] {
                let board = http(kind, None, Box::new(move |_, _, _| Ok(Reply {
                    status: 200, headers: vec![], body: body.into(), operation: "read",
                })));
                let expected = Some("invalid board list: expected array");
                assert_eq!(board.issues(Some(0)).err().as_deref(), expected, "{} issue list: {body}", kind.name);
                assert_eq!(board.comments(1).err().as_deref(), expected, "{} comments: {body}", kind.name);
                assert_eq!(board.canonical_labels().err().as_deref(), expected, "{} catalog: {body}", kind.name);
            }
        }
    }

    fn label_board(kind: &'static Kind, fail: bool, mismatch: bool) -> (Box<dyn Board>, std::rc::Rc<RefCell<Vec<Value>>>, std::rc::Rc<RefCell<usize>>) {
        let catalog = std::rc::Rc::new(RefCell::new(vec![json!({"id": 99, "name": "bug", "color": "FF0000", "description": "user owned"})]));
        let writes = std::rc::Rc::new(RefCell::new(0));
        let (data, count) = (catalog.clone(), writes.clone());
        let h = http(kind, None, Box::new(move |method, url, body| {
            assert!(url.contains("/labels"));
            if method == "GET" { return Ok(Reply { status: 200, headers: vec![], operation: "read", body: json!(*data.borrow()).to_string() }); }
            assert!(matches!(method, "POST" | "PATCH" | "PUT"));
            if method == "POST" { assert!(url.ends_with("/labels")); }
            else if kind.name == "gitlab" { assert_eq!(method, "PUT"); assert!(url.rsplit('/').next().unwrap().parse::<u64>().is_ok()); }
            else { assert_eq!(method, "PATCH"); }
            assert_eq!(body.as_ref().unwrap()["color"].as_str().unwrap().starts_with('#'), kind.name == "gitlab");
            *count.borrow_mut() += 1;
            if fail && *count.borrow() == 2 { return Err("HTTP 403 denied".into()); }
            let mut v = body.unwrap();
            if mismatch { v["color"] = json!("000000"); }
            let mut all = data.borrow_mut();
            if let Some(old) = all.iter_mut().find(|old| old["name"] == v["name"]) {
                v["id"] = old["id"].clone(); *old = v;
            } else { v["id"] = json!(all.len() + 100); all.push(v); }
            Ok(Reply { status: 200, headers: vec![], operation: "read", body: "{}".into() })
        }));
        (h, catalog, writes)
    }

    // Chain: § 6 label setup on both boards. Risk: repeat writes or user label metadata overwritten.
    #[test]
    fn label_adapters_readback_idempotence_and_preservation() {
        for kind in BOARDS {
            let (h, catalog, writes) = label_board(kind, false, false);
            let mut names = crate::labels::FIXED.iter().chain(["host-win", "needs-gpu", "area-ui"].iter()).map(|s| s.to_string()).collect::<Vec<_>>();
            names.push("taskq".into());
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), names.len() - 1);
            let reads = h.requests().total();
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), names.len() - 1, "repeat makes no writes");
            assert_eq!(h.requests().total(), reads, "repeat sends no catalog read");
            assert_eq!(catalog.borrow()[0]["description"], "user owned");
            let i = catalog.borrow().iter().position(|v| v["name"] == "q-ask").unwrap();
            catalog.borrow_mut()[i]["color"] = json!("000000");
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), names.len() - 1, "verified once per process (§ 4 budget)");
            let (fresh, fresh_catalog, fresh_writes) = label_board(kind, false, false);
            *fresh_catalog.borrow_mut() = catalog.borrow().clone();
            fresh.ensure_labels(&names).unwrap();
            assert_eq!(*fresh_writes.borrow(), 1, "a new process repairs existing metadata");
        }
    }

    // Chain: § 9 label cutover gate. Risk: partial label writes or legacy board relabelled early.
    #[test]
    fn label_adapter_failures_and_legacy_gate() {
        for kind in BOARDS {
            let names = vec!["q-doing".into(), "q-ask".into()];
            let (h, _, writes) = label_board(kind, true, false);
            assert!(h.ensure_labels(&names).unwrap_err().contains("403"));
            h.ensure_labels(&names).unwrap();
            assert_eq!(*writes.borrow(), 3, "safe retry only writes missing label");
            let (h, _, _) = label_board(kind, false, true);
            assert!(h.ensure_labels(&names).unwrap_err().contains("readback differs"));
            let (h, catalog, writes) = label_board(kind, false, false);
            catalog.borrow_mut().push(json!({"name":"q-waiting"}));
            assert!(!h.canonical_labels().unwrap());
            assert_eq!(h.state_label("waiting").unwrap(), "q-waiting");
            assert!(h.ensure_labels(&names).unwrap_err().contains("all-host gate"));
            assert_eq!(*writes.borrow(), 0);
            let h = http(kind, Some(true), Box::new(|_, url, _| Err(format!("unexpected {url}"))));
            assert_eq!(h.state_label("waiting").unwrap(), "q-blocked");
            assert!(label_schema(Some("typo")).is_err());
        }
    }

    // Chain: § 4 board budget on both boards. Risk: an exhausted budget keeps sending the heartbeat every pass, or a
    // multi-line error body splits a log line.
    #[test]
    fn exhausted_budget_sends_nothing_until_reset() {
        for kind in BOARDS {
            let sent = std::rc::Rc::new(RefCell::new(0));
            let count = sent.clone();
            let reset = iso("2026-10-14T04:41:00Z").unwrap().to_string();
            let h = http(kind, Some(true), Box::new(move |_, url, _| {
                *count.borrow_mut() += 1;
                let date = ("Date".to_string(), "Wed, 14 Oct 2026 04:27:00 GMT".to_string());
                if url.ends_with("/issues/6") {
                    return Ok(Reply { status: 500, headers: vec![date], body: "<html>\n  <p>down</p>\n</html>".into(), operation: "update" });
                }
                let headers = vec![date, ("X-RateLimit-Remaining".into(), "0".into()), ("X-RateLimit-Reset".into(), reset.clone())];
                Ok(Reply { status: 403, headers, body: "{\"message\": \"API rate limit exceeded\"}".into(), operation: "update" })
            }));
            assert!(h.edit(6, "beat").unwrap_err().ends_with("HTTP 500 <html> <p>down</p> </html>"));
            let first = h.edit(5, "beat").unwrap_err();
            assert!(first.contains("HTTP 403 rate limit: board requests paused until 2026-10-14T04:41:00Z"), "{first}");
            let again = h.edit(5, "beat").unwrap_err();
            assert!(again.ends_with("rate limit: board requests paused until 2026-10-14T04:41:00Z"), "{again}");
            assert_eq!(*sent.borrow(), 2, "nothing is sent while paused");
            let b = h.requests();
            assert_eq!((b.total(), b.remaining, b.paused_until()), (2, Some(0), iso("2026-10-14T04:41:00Z")));
        }
    }

    // Chain: § 4 two project adapters -> shared host/token pause. Risk: another project keeps sending after 429,
    // or a different credential inherits that pause. Fake transport proves it without board writes.
    #[test]
    fn host_project_adapters_share_budget_and_pause_by_credential() {
        std::thread::spawn(|| {
            crate::host::enter(std::path::Path::new("invented-root"), std::path::Path::new("invented-install"));
            let sent = std::rc::Rc::new(RefCell::new(0));
            let make = |token: &str, status: u16| {
                let mut api = (github::KIND.api)("github.com", "invented/project", token);
                let sent = sent.clone();
                api.transport = Some(Box::new(move |_, _, _| {
                    *sent.borrow_mut() += 1;
                    Ok(Reply { status, headers: vec![("retry-after".into(), "60".into())], body: "{}".into(), operation: "read" })
                })); api
            };
            let a = make("invented-host-budget-token", 429);
            let b = make("invented-host-budget-token", 200);
            let different = make("invented-other-budget-token", 200);
            assert!(a.call("GET", "/issues", None).err().unwrap().contains("paused"));
            assert!(b.call("GET", "/issues", None).err().unwrap().contains("paused"));
            assert_eq!(*sent.borrow(), 1); assert_eq!(b.budget.borrow().total(), 1);
            assert!(different.call("GET", "/issues", None).is_ok()); assert_eq!(*sent.borrow(), 2);
        }).join().unwrap();
    }

    // Chain: § 4 board budget, GitHub open list over GraphQL. Risk: the list loses a page, a field or trust, falls back
    // to core REST, keeps sending after a GraphQL RATE_LIMITED reply, or relists an unchanged board (#1819).
    #[test]
    fn github_open_list_is_one_graphql_request_per_page() {
        let kind = BOARDS.iter().find(|k| k.name == "github").unwrap();
        let limited = std::rc::Rc::new(RefCell::new(false));
        let flag = limited.clone();
        let probes = std::rc::Rc::new(RefCell::new(0));
        let probed = probes.clone();
        let h = http(kind, Some(true), Box::new(move |method, url, body| {
            if method == "GET" {
                // the newest-updated probe: its first reply carries the ETag, every later one is 304 Not Modified
                assert_eq!(url, "https://api.github.com/repos/o/r/issues?state=all&sort=updated&direction=desc&per_page=1");
                *probed.borrow_mut() += 1;
                let first = *probed.borrow() == 1;
                return Ok(Reply { status: if first { 200 } else { 304 }, headers: vec![("ETag".into(), "\"e1\"".into())],
                    body: if first { "[]".into() } else { String::new() }, operation: "read" });
            }
            assert_eq!((method, url), ("POST", "https://api.github.com/graphql"));
            let body = body.unwrap();
            // labels are the only nested connection: assignees would double the points (#1043)
            assert!(!body["query"].as_str().unwrap().contains("assignees"));
            assert_eq!((&body["variables"]["owner"], &body["variables"]["name"]), (&json!("o"), &json!("r")));
            let left = if *flag.borrow() { "0" } else { "4990" };
            let headers = vec![("X-RateLimit-Resource".into(), "graphql".into()), ("X-RateLimit-Remaining".into(), left.into()),
                ("X-RateLimit-Limit".into(), "5000".into()), ("X-RateLimit-Reset".into(), "1791964800".into())];
            let value = if *flag.borrow() { json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit exceeded"}]}) }
                else if body["variables"]["after"].is_null() { json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": true, "endCursor": "c1"},
                    "nodes": [{"number": 5, "title": "t", "body": "b", "updatedAt": "2026-10-14T04:27:00Z", "createdAt": "2026-10-14T04:20:00Z", "authorAssociation": "OWNER",
                        "labels": {"nodes": [{"name": "q-ready"}]}}]}}}}) }
                else { json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": false, "endCursor": null},
                    "nodes": [{"number": 6, "title": "u", "body": null, "updatedAt": "2026-10-14T04:28:00Z", "authorAssociation": "NONE",
                        "labels": {"nodes": []}}]}}}}) };
            Ok(Reply { status: 200, headers, body: value.to_string(), operation: operation(method) })
        }));
        let list = h.issues(None).unwrap();
        assert_eq!(list.iter().map(|i| (i.number, i.open, i.trusted)).collect::<Vec<_>>(), [(5, true, true), (6, true, false)]);
        assert_eq!((list[0].labels.clone(), list[0].assignees.clone(), list[0].updated, list[0].created), (vec!["q-ready".to_string()], vec![], iso("2026-10-14T04:27:00Z").unwrap(), iso("2026-10-14T04:20:00Z").unwrap()));
        let b = h.requests();
        assert_eq!((b.ops.get("POST graphql"), b.resource.clone(), b.remaining), (Some(&2), Some("graphql".into()), Some(4990)));
        assert_eq!(h.issues(None).unwrap(), list, "a 304 probe keeps the last list");
        assert_eq!((h.requests().ops.get("POST graphql"), *probes.borrow()), (Some(&2), 2));
        h.forget();
        assert_eq!(h.issues(None).unwrap(), list);
        assert_eq!(h.requests().ops.get("POST graphql"), Some(&4), "a forgotten list is read in full");
        h.forget();
        *limited.borrow_mut() = true;
        assert!(h.issues(None).unwrap_err().contains("HTTP 403 rate limit: board requests paused until"));
        assert!(h.issues(None).unwrap_err().contains("paused"));
        assert_eq!(h.requests().total(), 9, "nothing is sent while paused");
    }

    // Chain: § 4 budget (#1819). Risk: a repeated GET is counted by GitHub, or a 304 loses the body.
    #[test]
    fn repeated_get_reuses_its_body_on_304() {
        let mut api = (BOARDS[0].api)("github.com", "o/r", "test");
        let calls = std::rc::Rc::new(RefCell::new(0));
        let seen = calls.clone();
        api.transport = Some(Box::new(move |_, _, _| {
            *seen.borrow_mut() += 1;
            let n = *seen.borrow();
            Ok(match n {
                1 => Reply { status: 200, headers: vec![("ETag".into(), "W/\"a\"".into())], body: "[1]".into(), operation: "read" },
                2 => Reply { status: 304, headers: vec![], body: String::new(), operation: "read" },
                _ => Reply { status: 200, headers: vec![], body: "[2]".into(), operation: "read" },
            })
        }));
        assert_eq!((api.call("GET", "/x", None).unwrap().body, api.unchanged.get()), ("[1]".to_string(), false));
        assert_eq!((api.call("GET", "/x", None).unwrap().body, api.unchanged.get()), ("[1]".to_string(), true));
        assert_eq!((api.call("GET", "/x", None).unwrap().body, api.unchanged.get()), ("[2]".to_string(), false));
        assert!(api.etags.borrow().is_empty(), "a reply without ETag drops the cached one");
    }

    // Chain: § 2 state labels on both boards. Risk: a state change removes user labels.
    #[test]
    fn issue_label_adapters_preserve_unrelated_assignments() {
        for kind in BOARDS {
            let labels = std::rc::Rc::new(RefCell::new(vec!["bug".to_string(), "q-ready".to_string()]));
            let data = labels.clone();
            let h = http(kind, Some(false), Box::new(move |method, url, body| {
                let reply = |body: Value| Ok(Reply { status: 200, headers: vec![], operation: "read", body: body.to_string() });
                if url.contains("/members/") { return reply(json!({"access_level": 20})); }
                if method == "GET" {
                    assert!(url.ends_with("/issues/7"));
                    return reply(json!({"number": 7, "iid": 7, "state": "open", "author_association": "OWNER", "labels": *data.borrow()}));
                }
                if method == "POST" && url.ends_with("/issues") {
                    let body = body.unwrap();
                    *data.borrow_mut() = if kind.name == "gitlab" { body["labels"].as_str().unwrap().split(',').map(String::from).collect() }
                        else { names(&body["labels"], "name") };
                    return reply(json!({"number": 7, "iid": 7}));
                }
                if kind.name == "gitlab" {
                    assert_eq!(method, "PUT"); assert!(url.ends_with("/issues/7"));
                    let body = body.unwrap();
                    let remove = body["remove_labels"].as_str().unwrap();
                    data.borrow_mut().retain(|l| !remove.split(',').any(|r| r == l));
                    for label in body["add_labels"].as_str().unwrap().split(',').filter(|s| !s.is_empty()) { data.borrow_mut().push(label.into()); }
                } else if method == "POST" {
                    assert!(url.ends_with("/issues/7/labels"));
                    data.borrow_mut().extend(names(&body.unwrap()["labels"], "name"));
                } else {
                    assert_eq!(method, "DELETE"); assert!(url.ends_with("/issues/7/labels/q-ready"));
                    data.borrow_mut().retain(|l| l != "q-ready");
                }
                reply(json!({}))
            }));
            h.labels(7, &["q-doing".into()], &["q-ready".into()]).unwrap();
            assert_eq!(*labels.borrow(), vec!["bug", "q-doing"]);
            h.create("new", "", &["taskq".into(), "q-ready".into()]).unwrap();
            assert_eq!(*labels.borrow(), vec!["taskq", "q-ready"]);
        }
    }

    // Chain: R12 label readback. Risk: a partial label change reported as applied.
    #[test]
    fn assignment_readback_rejects_partial_application() {
        let b = Fake::default();
        let n = b.create("t", "", &["bug".into(), "q-ready".into()]).unwrap();
        assert!(verify_assignment(&b, n, &["q-doing".into()], &["q-ready".into()]).is_err());
        b.labels(n, &["q-doing".into()], &["q-ready".into()]).unwrap();
        verify_assignment(&b, n, &["q-doing".into()], &["q-ready".into()]).unwrap();
        assert!(b.issue(n).unwrap().labels.contains(&"bug".into()));
    }

    // Chain: § 3 lease time from board stamps. Risk: board dates parsed wrong, leases expire wrong.
    #[test]
    fn clocks() {
        assert_eq!(iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso("2026-10-12T10:20:30.123Z"), Some(1791800430));
        assert_eq!(http_date("Mon, 12 Oct 2026 10:20:30 GMT"), Some(1791800430));
        assert_eq!(stamp(1791800430), "2026-10-12T10:20:30Z");
    }

    // Chain: R1 board entries to fold. Risk: untrusted or edited comments move the queue.
    #[test]
    fn untrusted_and_edited_entries_never_reach_fold() {
        let b = Fake::default();
        let n = b.create("t", "", &[]).unwrap();
        b.raw(n, "taskq: spawn worker claude:x evil", false, false);
        b.raw(n, "1", false, false);
        b.raw(n, "taskq: later", true, true);
        let e = entries(&b, n).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(crate::core::fold(0, &e, false).state, crate::core::State::Ready);
    }

    // Chain: § 9 GitLab token lookup (coreq v0.14.8 `<install>/gitlab-token`) -> board read. Risk: a GitLab host with only
    // the install token file finds no token, or the GitHub `token` file goes to GitLab.
    #[test]
    fn gitlab_board_reads_install_token_file() {
        if std::env::var_os("GITLAB_TOKEN").is_some() { return; }
        let install = std::env::temp_dir().join(format!("taskq-gitlab-token-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("token"), "invented-github-token\n").unwrap();
        std::fs::write(install.join("gitlab-token"), "invented-gitlab-token\n").unwrap();
        let tool = coreq::Tool { install: install.clone(), ..coreq::Tool::new("taskqgitlabtokentest", "TaskQ", "fixture/unused", "1.2.3") };
        let token = (gitlab::KIND.token)(&tool, "gitlab.example").unwrap();
        assert_eq!(token, "invented-gitlab-token");
        assert_eq!(gitlab::KIND.worker_env(&tool, "gitlab.example").unwrap(), vec![("GITLAB_TOKEN".into(), token.clone())]);
        let mut api = (gitlab::KIND.api)("gitlab.example", "o/r", &token);
        assert_eq!(api.auth, "Authorization: Bearer invented-gitlab-token");
        api.transport = Some(Box::new(|_, url, _| {
            assert!(url.starts_with("https://gitlab.example/api/v4/projects/o%2Fr/"), "{url}");
            let body = if url.ends_with("/issues/7") { r#"{"iid":7,"title":"t","state":"opened"}"# } else { r#"{"access_level":30}"# };
            Ok(Reply { status: 200, headers: vec![], body: body.into(), operation: "read" })
        }));
        assert_eq!((gitlab::KIND.board)(api).issue(7).unwrap().number, 7);
        std::fs::remove_dir_all(install).unwrap();
    }

    // Chain: board token boundary. Risk: the token is sent to another host.
    #[test]
    fn token_goes_only_to_the_board_api() {
        let h = (github::KIND.api)("github.com", "o/r", "t");
        for url in ["https://evil.example/x", "https://api.github.com.evil.example/x"] {
            assert!(h.call("GET", url, None).err().is_some_and(|e| e.starts_with("refused")), "{url}");
        }
    }
}
