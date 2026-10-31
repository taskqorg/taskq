//! The task model of taskq.md § 2: one pure fold from log entries to a task. No I/O.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Ready,
    Waiting,
    Doing,
    Review,
    Ask,
    Later,
    Failed,
    Closed,
    Held,
}

impl State {
    pub fn name(self) -> &'static str {
        use State::*;
        match self {
            Ready => "ready",
            Waiting => "waiting",
            Doing => "doing",
            Review => "review",
            Ask => "ask",
            Later => "later",
            Failed => "failed",
            Closed => "closed",
            Held => "held",
        }
    }
}

/// One `Depends on` reference (§ 1): issue `n` of this board (`repo` none) or of another repository on its host.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Dep {
    pub repo: Option<String>,
    pub n: u64,
}

impl Dep {
    /// `#N` or `<repo>#N`, as written in the issue text.
    pub fn name(&self) -> String {
        format!("{}#{}", self.repo.as_deref().unwrap_or_default(), self.n)
    }
}

/// `Depends on #N` and `Depends on <repo>#N` lines of an issue text (§ 1); `<repo>` is `name` or `owner/name`.
pub fn depends(body: &str) -> Vec<Dep> {
    body.lines().filter_map(|l| {
        let (repo, n) = l.trim().strip_prefix("Depends on ")?.split_whitespace().next()?.split_once('#')?;
        let path = |s: &str| s.split('/').all(|p| !p.is_empty() && !p.starts_with('.') && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)));
        Some(Dep { repo: (!repo.is_empty()).then(|| repo.to_string()).filter(|r| path(r)), n: n.parse().ok()? }).filter(|d| repo.is_empty() || d.repo.is_some())
    }).collect()
}

/// A log entry's comment body (§ 2).
pub fn entry(kind: &str, args: &[&str], attempt: Option<u64>, text: &str) -> String {
    let mut head = std::iter::once(kind).chain(args.iter().copied()).collect::<Vec<_>>().join(" ");
    if let Some(a) = attempt {
        head += &format!(" attempt {a}");
    }
    if text.is_empty() { format!("taskq: {head}") } else { format!("taskq: {head}\n{text}") }
}

/// § 2 retry limits: an attempt `requeue` that makes `retries` this, or a `gone` that makes it [`GONE_MAX`], asks.
pub const REQUEUE_MAX: u32 = 3;
pub const GONE_MAX: u32 = 2;

/// The `later` option of a service card (#2012): only an answer `N.K` parks the task (§ 2).
pub const LATER: &str = "later: park the task - + no attempt runs; - waits until the owner requeues it";

/// The text of attempt `kind` (`requeue`, `gone`) on `t` (§ 2 limit card, #1123): when it reaches the retry limit, the
/// reason plus a card naming the limit with requeue and later options, so the parked task never waits on a card
/// without choices. The writer adds it, so every version folds the same card.
pub fn limit_text(n: u64, t: &Task, kind: &str, text: &str) -> String {
    let max = match kind {
        "requeue" => REQUEUE_MAX,
        "gone" => GONE_MAX,
        _ => return text.into(),
    };
    let is_card = text.lines().any(|l| l.starts_with("Problem: ")) && options(text).iter().filter(|o| o.2).count() == 1;
    if !matches!(t.state, State::Doing | State::Review) || t.retries + 1 < max || is_card {
        return text.into();
    }
    // the reason's own numbered lines (a fix list) must not become options of the card
    let reason = text.lines().map(|l| {
        let body = l.trim_start();
        match body.split_once(". ") {
            Some((k, rest)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => format!("{}{k}) {rest}", &l[..l.len() - body.len()]),
            _ => l.to_string(),
        }
    });
    // a bare comment `K` folds as a plain answer (ready), so `later` needs the CLI form (§ 2)
    let card = crate::service_card(n, "the service stopped retrying; no new attempt starts until the owner answers.",
        &format!("retry limit {max} reached by this {kind}; the task waits for the owner."), &[
        "requeue: retry - + a fresh attempt starts now; - it may fail the same way".into(),
        LATER.into(),
    ], 1);
    reason.filter(|s| !s.trim().is_empty()).chain(card.lines().map(String::from)).collect::<Vec<_>>().join("\n")
}

/// Appends the optional § 2 fields to an entry body: `op-id`, then `revision` (written only with an op id).
pub fn with_fields(body: String, op: Option<&str>, revision: Option<u64>) -> String {
    match (op, revision) {
        (Some(o), Some(r)) => format!("{body}\nop-id: {o}\nrevision: {r}"),
        (Some(o), None) => format!("{body}\nop-id: {o}"),
        _ => body,
    }
}

/// The op id a log entry of task `n` carries (§ 2); a bare comment carries none.
pub fn op_id(n: u64, body: &str) -> Option<&str> {
    parse(n, body)?.op
}

/// The text of a log entry of task `n` (§ 2): after its first line, without the op id fields.
pub fn entry_text(n: u64, body: &str) -> &str {
    parse(n, body).map_or("", |l| l.text)
}

/// Whether `text` ends with lines that would read as the § 2 fields: such text is refused before writing.
pub fn has_fields(text: &str) -> bool {
    fields(text).1.is_some()
}

/// Text without its trailing fields and the op id: `op-id: <id>`, optionally followed by `revision: <R>`. Without
/// the op id line a trailing `revision:` stays text, so historical entries fold unchanged (R7).
fn fields(text: &str) -> (&str, Option<&str>) {
    let t = text.trim_end();
    let (rest, last) = t.rsplit_once('\n').unwrap_or(("", t));
    let (rest, last) = match last.trim().strip_prefix("revision: ").and_then(|r| r.trim().parse::<u64>().ok()) {
        Some(_) => rest.trim_end().rsplit_once('\n').unwrap_or(("", rest.trim_end())),
        None => (rest, last),
    };
    match last.trim().strip_prefix("op-id: ") {
        Some(o) => (rest.trim_end(), Some(o.trim())),
        None => (text, None),
    }
}

/// An owner gate (§ 2): `required` until the owner approves the current result, `stale` once a later result or a
/// move to `ready` invalidated an approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gate {
    Required,
    Approved,
    Stale,
}

impl Gate {
    pub fn name(self) -> &'static str {
        match self {
            Gate::Required => "required",
            Gate::Approved => "approved",
            Gate::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Task {
    pub state: State,
    pub attempt: Option<u64>,
    pub role: Option<String>,
    pub session: Option<String>,
    pub retries: u32,
    pub card: Option<String>,
    pub result: Option<String>,
    pub inbox: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enqueued: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    /// The PM's pick of an open `ask` card (§ 2): option K and its one-line reason; never an answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<(usize, String)>,
    /// A worker park's blocker issue (§ 2 `later <ref> attempt`): read like a `Depends on` line until the next spawn or an owner requeue outside `later`/`failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
    /// A timed park's UTC time (§ 2 `later until <UTC>`): the service writes `requeue` at that board time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// Finished stages (§ 2 Stages): the last accepted `later` entry's `Stages:` line, sorted by number; kept until the next one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<String>,
}

/// The issue text whose `Depends on` lines keep `t` waiting (§ 1): `body` plus a worker park's blocker (§ 2).
pub fn dep_body(body: &str, t: &Task) -> String {
    t.blocker.as_ref().map_or_else(|| body.to_string(), |r| format!("{body}\nDepends on {r}\n"))
}

/// The task's stages (§ 2 Stages): its Acceptance item ids `A<k>` in body order, each once.
pub fn acceptance(body: &str) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for l in body.lines() {
        let l = l.trim_start();
        let l = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).unwrap_or(l);
        let Some((id, _)) = l.split_once(". ") else { continue };
        if stage_id(id) && !ids.iter().any(|x| x == id) { ids.push(id.to_string()); }
    }
    ids
}

fn stage_id(s: &str) -> bool {
    s.strip_prefix('A').is_some_and(|k| !k.is_empty() && k.len() <= 3 && k.bytes().all(|c| c.is_ascii_digit()) && !k.starts_with('0'))
}

/// A `later` text's `Stages:` line (§ 2 Stages): the finished set sorted by number, empty for `none`; `None` without a valid line.
pub fn stages_line(text: &str) -> Option<Vec<String>> {
    let rest = text.lines().find_map(|l| l.strip_prefix("Stages:"))?.trim();
    if rest == "none" { return Some(vec![]); }
    let mut ids: Vec<&str> = rest.split_whitespace().collect();
    if ids.is_empty() || !ids.iter().all(|i| stage_id(i)) { return None; }
    ids.sort_by_key(|i| i[1..].parse::<u32>().unwrap_or(0));
    ids.dedup();
    Some(ids.into_iter().map(String::from).collect())
}

/// R6 `stage k/n`: k is the position of the first open stage (all done: n), n the Acceptance item count; `None` before any record.
pub fn stage(body: &str, done: &[String]) -> Option<(usize, usize)> {
    let ids = acceptance(body);
    if done.is_empty() || ids.is_empty() { return None; }
    Some((ids.iter().position(|i| !done.contains(i)).map_or(ids.len(), |k| k + 1), ids.len()))
}

/// Whether `s` is a timed park's time (§ 2): `YYYY-MM-DDTHH:MM:SSZ`, a real UTC date and time.
pub fn utc(s: &str) -> bool {
    let b = s.as_bytes();
    let n = |a: usize, z: usize| s.get(a..z).filter(|x| x.bytes().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<u32>().ok());
    let (Some(y), Some(m), Some(d), Some(h), Some(mi), Some(sec)) = (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19)) else { return false };
    let feb = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 29 } else { 28 };
    b.len() == 20 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[16] == b':' && b[19] == b'Z'
        && (1..=12).contains(&m) && d >= 1 && d <= [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m as usize - 1] && h < 24 && mi < 60 && sec < 60
}

/// Whether `r` is one `#N` or `<repo>#N` blocker reference (§ 2 worker park).
pub fn blocker_ref(r: &str) -> bool {
    !r.contains(char::is_whitespace) && depends(&format!("Depends on {r}")).len() == 1
}

/// A comment by a trusted author; the board adapter drops everyone else's.
#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub body: String,
    #[serde(default)]
    pub edited: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release { Exit, NoStart }

/// A board intent, its exact-input acceptance and independent native release evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Execution {
    pub id: u64,
    pub attempt: u64,
    pub through: u64,
    pub host: String,
    pub runtime: String,
    pub accepted: bool,
    pub release: Option<Release>,
    pub usage_hold: Option<(i64, bool)>,
}

/// All derived facts for one immutable board read; dependency blocking only adjusts the current task.
pub struct Projection {
    pub task: Task,
    pub revision: u64,
    accepted: Vec<u64>,
    /// Accepted answers and steers in board order, across all attempts (§ 2).
    pub notes: Vec<u64>,
    /// Accepted answers in board order, across all attempts (§ 5 `inbox --json`).
    pub answers: Vec<Answer>,
    pub executions: Vec<Execution>,
    pub changes: Vec<(u64, Task, Task)>,
    /// Review entries that start or end a deferred publication: the merge queue, a profile-only state (#1344)
    pub queue: Vec<(u64, Task, Task)>,
}

/// An accepted answer (§ 2 row 20) or a late answer saved as a note (`steer answer <R>`, § 5): its entry id, the
/// question revision it answered (0: unknown) and the chosen option number, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub id: u64,
    pub revision: u64,
    pub option: Option<usize>,
    pub late: bool,
}

impl Projection {
    pub fn accepts(&self, id: u64) -> bool { self.accepted.contains(&id) }
    pub fn delivered(&self, attempt: u64) -> Option<u64> {
        self.executions.iter().filter(|x| x.attempt == attempt && x.accepted).map(|x| x.through).max()
    }
    /// The accepted `answer` of `entries` that closed the last question, while the task stays out of `ask` (§ 5 late answer, #1785).
    pub fn closing_answer(&self, n: u64, entries: &[Entry]) -> Option<u64> {
        if self.task.state == State::Ask { return None; }
        let (id, kind) = entries.iter().filter(|e| self.accepted.contains(&e.id)).filter_map(|e| Some((e.id, parse(n, &e.body)?.kind)))
            .filter(|(_, k)| matches!(*k, "ask" | "answer")).max_by_key(|(id, _)| *id)?;
        (kind == "answer").then_some(id)
    }
}

pub fn executions(n: u64, entries: &[Entry]) -> Vec<Execution> { project(n, entries, false).executions }

pub fn delivered(n: u64, entries: &[Entry], attempt: u64) -> Option<u64> {
    project(n, entries, false).delivered(attempt)
}

struct Line<'a> {
    kind: &'a str,
    args: Vec<&'a str>,
    attempt: Option<u64>,
    text: &'a str,
    bare: bool,
    op: Option<&'a str>,
}

/// The first line of a log entry of task `n`; `taskq answer K|N.K` (the CLI form) counts as `taskq: answer K`, any other
/// comment as a bare answer that `pick` resolves against the open card.
fn parse(n: u64, body: &str) -> Option<Line<'_>> {
    let (head, text) = body.split_once('\n').unwrap_or((body, ""));
    let (text, op) = fields(text);
    if let Some(arg) = head.trim().strip_prefix("taskq answer ") {
        let k = match arg.trim().split_once('.') {
            Some((m, k)) => (m.trim_start_matches('#').parse() == Ok(n)).then_some(k)?,
            None => arg.trim(),
        };
        k.parse::<usize>().ok()?;
        return Some(Line { kind: "answer", args: vec![k], attempt: None, text: text.trim(), bare: false, op });
    }
    let Some(head) = head.trim().strip_prefix("taskq:") else {
        return Some(Line { kind: "answer", args: vec![body.trim()], attempt: None, text: "", bare: true, op: None });
    };
    let mut words: Vec<&str> = head.split_whitespace().collect();
    let mut attempt = None;
    if words.len() >= 3 && words[words.len() - 2] == "attempt" {
        attempt = Some(words.pop()?.parse().ok()?);
        words.pop();
    }
    let kind = words.first().copied()?;
    Some(Line { kind, args: words[1..].to_vec(), attempt, text: text.trim(), bare: false, op })
}

fn sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn text(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// Option lines `K. text [★]` of a card.
pub fn options(card: &str) -> Vec<(usize, String, bool)> {
    card.lines()
        .filter_map(|l| {
            let (k, rest) = l.trim().split_once(". ")?;
            let k: usize = k.parse().ok()?;
            let star = rest.trim_end().ends_with('★');
            Some((k, rest.trim_end().trim_end_matches('★').trim().to_string(), star))
        })
        .collect()
}

/// The revision displayed by an approval option; no implicit approval of an unseen result.
pub fn approval_sha(option: &str) -> Option<&str> {
    let mut words = option.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("approve") { return None; }
    let sha = words.next()?;
    ((7..=40).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit())).then_some(sha)
}

/// The option a bare comment `s` names on `card` (§ 2): its number, or its text with or without `K. `, case-insensitive;
/// None unless exactly one option matches.
fn pick(card: &str, s: &str) -> Option<usize> {
    let s = s.trim_end_matches('★').trim().to_lowercase();
    let m: Vec<usize> = options(card)
        .into_iter()
        .filter(|(k, o, _)| [k.to_string(), o.to_lowercase(), format!("{k}. {}", o.to_lowercase())].contains(&s))
        .map(|o| o.0)
        .collect();
    (m.len() == 1).then(|| m[0])
}

/// The open card just before entry `id`: the question an answer at `id` closed (#1918 conversation history).
pub fn card_before(n: u64, entries: &[Entry], id: u64) -> Option<String> {
    let before: Vec<Entry> = entries.iter().filter(|x| x.id < id).cloned().collect();
    fold(n, &before, false).card
}

/// What a resumed child receives for inbox entry `id` (§ 2): its text, else option K of the card it answers, else its
/// first line; never empty.
pub fn delivery(n: u64, entries: &[Entry], id: u64) -> String {
    let Some(e) = entries.iter().find(|e| e.id == id) else { return String::new() };
    let head = e.body.split_once('\n').map_or(e.body.as_str(), |h| h.0);
    let text = parse(n, &e.body).map_or("", |l| l.text);
    if !text.is_empty() {
        return text.to_string();
    }
    let before: Vec<Entry> = entries.iter().filter(|x| x.id < id).cloned().collect();
    let card = fold(n, &before, false).card.unwrap_or_default();
    let k = parse(n, &e.body).and_then(|l| if l.bare { pick(&card, l.args[0]) } else { l.args.first()?.parse::<usize>().ok() });
    options(&card).into_iter().find(|o| Some(o.0) == k).map(|o| o.1).unwrap_or_else(|| head.trim().to_string())
}

/// § 2 Summary: a `close` names the `publish` it repeats (`Repeats: publish <id>`); that entry's text stands in the
/// line's place. Older closes that copy the verdict, or a reference to no `publish`, read unchanged.
pub fn verdict(n: u64, entries: &[Entry], text: &str) -> String {
    text.lines().map(|l| {
        let id = l.trim().strip_prefix(REPEATS).and_then(|id| id.trim().parse::<u64>().ok());
        id.and_then(|id| entries.iter().find(|e| e.id == id && !e.edited && kind(n, &e.body) == "publish"))
            .map_or_else(|| l.to_string(), |e| entry_text(n, &e.body).to_string())
    }).collect::<Vec<_>>().join("\n")
}

/// The `close` line naming the `publish` entry it repeats (§ 2 Summary).
pub const REPEATS: &str = "Repeats: publish ";

/// The kind of log entry `body` of task `n`: `answer` for a bare comment (§ 2).
pub fn kind(n: u64, body: &str) -> &str {
    parse(n, body).map_or("answer", |l| l.kind)
}

/// Whether attempt `a` wrote `ask`, `result` or `requeue` after entry `after`: its child then exits clean (§ 4 step 1).
pub fn handed_in(n: u64, entries: &[Entry], a: u64, after: u64) -> bool {
    entries.iter().filter(|e| e.id > after && !e.edited).filter_map(|e| parse(n, &e.body)).any(|l| l.attempt == Some(a) && matches!(l.kind, "ask" | "result" | "requeue" | "publish"))
}

/// The kind of attempt `a`'s last `ask`, `result`, `requeue` or `close` after entry `after` that `fold` accepted:
/// invalid, duplicate and wrong-state entries change nothing and count as no hand-in.
fn hand_in(n: u64, entries: &[Entry], a: u64, after: u64) -> Option<&'static str> {
    let accepted = project(n, entries, false).accepted;
    let mut log: Vec<&Entry> = entries.iter().filter(|e| e.id > after && accepted.contains(&e.id)).collect();
    log.sort_by_key(|e| e.id);
    log.into_iter().filter_map(|e| parse(n, &e.body)).filter(|l| l.attempt == Some(a))
        .filter_map(|l| ["ask", "result", "requeue", "close", "publish"].into_iter().find(|k| *k == l.kind)).last()
}

/// Outcome and error class of attempt `a`'s `build`/`review` stage when its child exits or is stopped (§ 4 Profiling):
/// its accepted hand-in after entry `after` wins over a stop. `live` is whether the board read shows `a` as the task's
/// attempt, `None` once the issue left the board read (`log` then read back); an unread log is unknown with its reason.
pub fn stage_outcome(n: u64, log: Result<(&[Entry], Option<bool>), &'static str>, a: u64, after: u64) -> (&'static str, &'static str) {
    let (entries, live) = match log {
        Ok(x) => x,
        Err(why) => return ("unknown", why),
    };
    match hand_in(n, entries, a, after) {
        Some("result" | "close" | "publish") => ("ok", "none"),
        Some("ask") => ("ask", "none"),
        Some(_) => ("retried", "none"),
        // still the live attempt: the child ended without hand-in
        None if live == Some(true) => ("failed", "exit"),
        // replaced on the board (owner requeue, answer, retry)
        None if live == Some(false) => ("cancelled", "none"),
        // label removed or closed without its accepted hand-in: no evidence either way
        None => ("unknown", "left-board"),
    }
}

fn has_options(s: &str) -> bool {
    s.lines().any(|l| l.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
}

fn end(t: &mut Task) {
    t.attempt = None;
    t.role = None;
    t.session = None;
    t.inbox.clear();
    t.publication = None;
    t.enqueued = None;
    t.until = None;
}

fn ready(t: &mut Task) {
    end(t);
    t.blocker = None;
    t.card = None;
    t.result = None;
    t.state = State::Ready;
    invalidate(t);
}

/// § 2: an approval binds to the result it named; the next result or a move to `ready` makes it stale.
fn invalidate(t: &mut Task) {
    if t.gate == Some(Gate::Approved) {
        t.gate = Some(Gate::Stale);
    }
}

pub fn fold(n: u64, entries: &[Entry], deps_open: bool) -> Task {
    project(n, entries, deps_open).task
}

/// Logged state changes (§ 4 Profiling): per accepted entry that changed the folded state, its id and the task
/// before and after it, collected during the same replay.
pub fn changes(n: u64, entries: &[Entry]) -> Vec<(u64, Task, Task)> {
    project(n, entries, false).changes
}

/// The task's revision (§ 2): the entry that set its open card, else its attempt, else its last accepted entry; 0: none.
pub fn revision(n: u64, entries: &[Entry]) -> u64 {
    project(n, entries, false).revision
}


/// Parse and reduce once, retaining facts needed by the service and command surfaces.
pub fn project(n: u64, entries: &[Entry], deps_open: bool) -> Projection {
    let mut log: Vec<&Entry> = entries.iter().filter(|e| !e.edited).collect();
    log.sort_by_key(|e| e.id);
    let mut t = Task::default();
    let mut resume = None; // the state an `ask` entry left; None when a retry limit asked
    let mut accepted = vec![];
    let mut notes = vec![];
    let mut answers = vec![];
    let mut executions: Vec<Execution> = vec![];
    let (mut changes, mut queue) = (vec![], vec![]);
    let (mut ops, mut card_at, mut last) = (std::collections::HashSet::new(), None, 0);
    for e in log {
        if let Some(l) = parse(n, &e.body) {
            // § 2: a repeated op id is ignored; a stale revision is the writer's to undo (dispatch::act)
            if l.op.is_some_and(|o| !ops.insert(o)) {
                continue;
            }
            let before = t.clone();
            step(&mut t, &mut resume, e.id, &l, last == 0);
            // § 2: a pick belongs to the card it was made on
            if t.card != before.card || t.state != State::Ask {
                t.pick = None;
            }
            if l.kind == "spawn" && t.attempt == Some(e.id) && before.attempt != t.attempt {
                executions.push(Execution { id:e.id, attempt:e.id, through:e.id, host:l.args[2].into(),
                    runtime:l.args[1].split(':').next().unwrap_or("").into(), accepted:false, release:None,usage_hold:None });
            } else if l.kind == "executing" && l.args.len() == 1 && l.attempt.is_some() && l.attempt == t.attempt
                && matches!(t.state,State::Doing|State::Review) && executions.iter().all(|x|x.release.is_some()) {
                if let Ok(through) = l.args[0].parse::<u64>() {
                    let attempt=l.attempt.unwrap();
                    if t.inbox.last() == Some(&through) && executions.iter().filter(|x|x.attempt==attempt)
                        .all(|x| (x.accepted && x.through < through) || (x.release == Some(Release::NoStart) && x.through <= through)) {
                        if let Some(previous)=executions.iter().find(|x|x.attempt==attempt) {
                            executions.push(Execution {id:e.id,attempt,through,host:previous.host.clone(),runtime:previous.runtime.clone(),accepted:false,release:None,usage_hold:None});
                        }
                    }
                }
            } else if l.kind == "accepted" && l.args.len() == 1 {
                if let Ok(id) = l.args[0].parse::<u64>() {
                    if let Some(x) = executions.iter_mut().find(|x| x.id == id && Some(x.attempt) == l.attempt && x.release != Some(Release::NoStart)) {
                        x.accepted = true;
                    }
                }
            } else if l.kind == "released" && l.args.len() == 2 && matches!(l.args[1],"exit"|"no-start") {
                if let Ok(id)=l.args[0].parse::<u64>() {
                    let proof = if l.args[1] == "exit" { Release::Exit } else { Release::NoStart };
                    if let Some(x)=executions.iter_mut().find(|x|x.id==id && Some(x.attempt)==l.attempt && x.release.is_none()) {
                        if proof != Release::NoStart || !x.accepted {
                            x.release = Some(proof);
                            let words: Vec<_> = l.text.lines().next().unwrap_or_default().split_whitespace().collect();
                            if proof == Release::Exit && words.len() == 3 && words[0] == "usage-limit" && matches!(words[1], "until" | "probe") {
                                x.usage_hold = words[2].parse().ok().map(|at| (at, words[1] == "until"));
                            }
                        }
                    }
                }
            }
            let note = l.kind == "steer" && before.state != State::Closed && !l.text.is_empty()
                && l.attempt.is_none_or(|a| Some(a) == before.attempt);
            if t != before || note {
                last = e.id;
                accepted.push(e.id);
                if note || l.kind == "answer" { notes.push(e.id); }
                if l.kind == "answer" {
                    let card = before.card.as_deref().unwrap_or("");
                    let option = if l.bare { pick(card, l.args[0]) } else { l.args.first().and_then(|k| k.parse().ok()) };
                    answers.push(Answer { id: e.id, revision: card_at.unwrap_or(0), option, late: false });
                } else if note && l.args.first() == Some(&"answer") {
                    answers.push(Answer { id: e.id, revision: l.args.get(1).and_then(|r| r.parse().ok()).unwrap_or(0), option: None, late: true });
                }
                if t.card.is_some() && t.card != before.card {
                    card_at = Some(e.id);
                }
            }
            if t.state != before.state { changes.push((e.id, before, t.clone())); }
            else if t.state == State::Review && t.publication.is_some() != before.publication.is_some() { queue.push((e.id, before, t.clone())); }
        }
    }
    // ponytail: dependencies are judged at read time only, so a hand-typed spawn while waiting is accepted
    if t.state == State::Ready && deps_open {
        t.state = State::Waiting;
    }
    let rev = t.card.as_ref().and(card_at).or(t.attempt).unwrap_or(last);
    Projection { task:t, revision:rev, accepted, notes, answers, executions, changes, queue }
}

/// The board-backed context repeated at every attempt/resume and before hand-in (§ 7).
pub fn task_context(n: u64, body: &str, entries: &[Entry]) -> String {
    let p = project(n, entries, false);
    let notes: Vec<String> = p.notes.iter().map(|id| format!("Note {id}:\n{}", delivery(n, entries, *id))).collect();
    // § 2 Stages: the next attempt continues at the first open stage, in the same worktree (§ 7 rule 2)
    let ids = acceptance(body);
    let stages = match ids.iter().find(|i| !p.task.stages.contains(i)) {
        _ if p.task.stages.is_empty() || ids.is_empty() => String::new(),
        Some(open) => format!("\n\nStages done: {}; continue at {open} (stage {}/{}) in the existing worktree; park again with `later --stage` or hand in once all are done.",
            p.task.stages.join(" "), ids.iter().position(|i| i == open).unwrap_or(0) + 1, ids.len()),
        None => format!("\n\nStages done: {}; all {} stages are done: check them and hand in the result.", p.task.stages.join(" "), ids.len()),
    };
    format!("Task goal and acceptance (binding):\n{body}{stages}\n\nTask notes (oldest first):\n{}\n\nAcceptance remains binding. Check every note now; if one conflicts with acceptance, ask once immediately. Before hand-in, read inbox again and check the result against acceptance with all notes.", if notes.is_empty() { "none".into() } else { notes.join("\n\n") })
}

/// R1/#1929: only the first accepted hold associates a task with an owner chat; lifecycle is unchanged.
pub fn conversation(n: u64, entries: &[Entry]) -> Option<String> {
    let p = project(n, entries, false);
    let id = p.changes.iter().find(|(_, _, t)| t.state == State::Held)?.0;
    let e = entries.iter().find(|e| e.id == id && !e.edited)?;
    let ids: Vec<_> = e.body.lines().filter_map(|l| l.strip_prefix("conversation: ")).collect();
    match ids.as_slice() { [id] if crate::setup::owner_id(id) => Some((*id).into()), _ => None }
}

fn step(t: &mut Task, resume: &mut Option<State>, id: u64, l: &Line, first: bool) {
    use State::*;
    if l.attempt.is_some() && l.attempt != t.attempt {
        return; // fencing: a replaced attempt writes nothing
    }
    let own = l.attempt.is_some();
    let spawn = |t: &mut Task, role: &str| {
        t.attempt = Some(id);
        t.role = Some(role.to_string());
        t.session = Some(format!("{} {}", l.args[1], l.args[2]));
        t.inbox.clear();
        t.blocker = None;
    };
    let limit = |t: &mut Task, resume: &mut Option<State>, max: u32| {
        t.retries += 1;
        if t.retries >= max {
            end(t);
            t.card = text(l.text);
            *resume = None;
            t.state = Ask;
            true
        } else {
            false
        }
    };
    match (t.state, l.kind, own) {
        // § 2: `hold` only as the first accepted entry; `start` leaves it
        (Ready, "hold", false) if first => t.state = Held,
        (Held, "start", false) => t.state = Ready,
        // § 2 gate: set before work; only the owner (no attempt) approves, and only the current result; an approval
        // without a gate serves a `review-pr` project (§ 7)
        (Held | Ready, "gate", false) if t.gate.is_none() => t.gate = Some(Gate::Required),
        (Review | Ask, "approve", false) if t.gate != Some(Gate::Approved) && l.args.len() == 1 && t.result.as_deref() == Some(l.args[0]) => {
            t.gate = Some(Gate::Approved)
        }
        (Ready, "spawn", false) if l.args.len() == 3 && l.args[0] == "worker" => {
            spawn(t, "worker");
            t.state = Doing;
        }
        (Review, "spawn", false) if l.args.len() == 3 && l.args[0] == "reviewer" && t.attempt.is_none() && t.publication.is_none() => {
            spawn(t, "reviewer")
        }
        (Doing, "result", true) if l.args.len() == 1 && sha(l.args[0]) => {
            end(t);
            t.result = Some(l.args[0].to_string());
            t.card = text(l.text).filter(|s| has_options(s));
            t.state = Review;
            invalidate(t);
        }
        (Doing | Review, "ask", true) => {
            t.card = text(l.text);
            *resume = Some(t.state);
            t.state = Ask;
        }
        (Review, "publish", _) if t.publication.is_none() && !l.text.is_empty() && t.gate.is_none_or(|g| g == Gate::Approved) => {
            end(t);
            t.card = None;
            t.publication = text(l.text);
        }
        (Review, "enqueued", false) if t.publication.is_some() && t.enqueued.is_none() && l.args.len() == 1 => {
            t.enqueued = Some(l.args[0].to_string());
        }
        (Review, "rebased", false) if t.publication.is_some() && l.args.len() == 1 && sha(l.args[0]) && t.result.as_deref() != Some(l.args[0]) => {
            t.result = Some(l.args[0].to_string());
            if t.gate.is_some() {
                invalidate(t);
                t.publication = None;
            }
        }
        (Review, "publication-blocked", false) if t.publication.is_some() && !l.text.is_empty() => {
            t.card = text(l.text);
            *resume = Some(Review);
            t.state = Ask;
        }
        (Review, "publication-failed", false) if t.publication.is_some() => {
            if !limit(t, resume, REQUEUE_MAX) { ready(t) }
        }
        (Doing | Review, "requeue", _) if l.args == ["main-moved"] => ready(t),
        (Doing | Review, "requeue", true) => {
            if !limit(t, resume, REQUEUE_MAX) {
                ready(t)
            }
        }
        (Doing | Review, "usage-limit", true) if l.args.is_empty() => if t.state == Review { end(t) } else { ready(t) },
        (Doing | Review, "gone", true) => {
            if !limit(t, resume, GONE_MAX) {
                if t.state == Review { end(t) } else { ready(t) }
            }
        }
        (Doing, "steer", _) => t.inbox.push(id),
        (Review, "close", _) if t.gate.is_none_or(|g| g == Gate::Approved) => {
            end(t);
            t.state = Closed;
        }
        (Later | Failed, "close", false) => {
            end(t);
            t.state = Closed;
        }
        (Ask, "pick", false) if l.args.len() == 1 && !l.text.is_empty() && !l.text.contains('\n') => {
            let card = t.card.as_deref().unwrap_or("");
            if let Some(k) = l.args[0].parse().ok().filter(|k| options(card).iter().any(|o| o.0 == *k)) {
                t.pick = Some((k, l.text.to_string()));
            }
        }
        (Ask, "answer", _) if !l.bare || pick(t.card.as_deref().unwrap_or(""), l.args[0]).is_some() => {
            if !own {
                let card = t.card.as_deref().unwrap_or("");
                let k = if l.bare { pick(card, l.args[0]) } else { l.args.first().and_then(|s| s.parse().ok()) };
                if options(card).iter().any(|(n, option, _)| Some(*n) == k
                    && approval_sha(option).is_some_and(|sha| t.result.as_deref().is_some_and(|r| r.starts_with(sha))))
                {
                    t.gate = Some(Gate::Approved);
                }
            }
            t.retries = 0;
            t.card = None;
            match resume.take() {
                Some(s) => {
                    t.state = s;
                    t.inbox.push(id);
                }
                None => ready(t),
            }
        }
        (Ready | Waiting | Doing | Review | Ask | Later | Failed, "requeue", false) => {
            // § 2: Retry of an owner park returns to the worker park's blocker; a requeue elsewhere overrides it
            let blocker = if matches!(t.state, Later | Failed) { t.blocker.take() } else { None };
            t.retries = 0;
            ready(t);
            t.blocker = blocker;
        }
        // § 2 worker park: the attempt ends and the blocker keeps the task waiting until it is done
        (Doing | Review, "later", true) if l.args.len() == 1 && blocker_ref(l.args[0]) => {
            ready(t);
            t.blocker = Some(l.args[0].to_string());
            if let Some(s) = stages_line(l.text) { t.stages = s; }
        }
        (Ready | Waiting | Doing | Review | Ask, "later" | "failed", _) => {
            end(t);
            t.state = if l.kind == "later" { Later } else { Failed };
            // § 2 timed park: a malformed time parks untimed, as N-1 folds every timed park
            t.until = (l.kind == "later" && l.args.len() == 2 && l.args[0] == "until" && utc(l.args[1])).then(|| l.args[1].to_string());
            // § 2 Stages: a `later` records the finished stages; a stage park counts no retry
            if let Some(s) = stages_line(l.text).filter(|_| l.kind == "later") { t.stages = s; }
        }
        _ => {}
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trace {
    name: String,
    #[serde(default)]
    task: u64,
    #[serde(default)]
    deps_open: bool,
    entries: Vec<Entry>,
    expect: Task,
    #[serde(default)]
    notes: Option<Vec<u64>>,
}

/// Fold every golden trace; Ok(count) or the first mismatch.
pub fn golden(json: &str) -> Result<usize, String> {
    let traces: Vec<Trace> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    for tr in &traces {
        let got = fold(tr.task, &tr.entries, tr.deps_open);
        if got != tr.expect {
            return Err(format!("{}: got {:?}, want {:?}", tr.name, got, tr.expect));
        }
        if tr.notes.as_ref().is_some_and(|notes| *notes != project(tr.task, &tr.entries, tr.deps_open).notes) {
            return Err(format!("{}: task notes differ", tr.name));
        }
    }
    Ok(traces.len())
}

pub const GOLDEN: &str = include_str!("../tests/golden/fold.json");

#[cfg(test)]
mod tests {
    // Chain: native observations -> replay -> continuation. Risk: late, duplicate or contradictory evidence consumes/replays input.
    #[test]
    fn execution_observations_keep_acceptance_and_exit_independent() {
        use super::*;
        let mut log = vec![Entry { id:10, body:"taskq: spawn worker codex:s mac".into(), edited:false }];
        for (id, body, edited, delivered_cursor, release) in [
            (11,"taskq: accepted 10 attempt 9",false,None,None),
            (12,"taskq: accepted 10 attempt 10",true,None,None),
            (13,"taskq: released 10 exit attempt 10\nop-id: release-10",false,None,Some(Release::Exit)),
            (14,"taskq: released 10 no-start attempt 10",false,None,Some(Release::Exit)),
            (15,"taskq: accepted 99 attempt 10",false,None,Some(Release::Exit)),
            (16,"taskq: accepted 10 attempt 10\nop-id: accept-10",false,Some(10),Some(Release::Exit)),
            (17,"taskq: accepted 10 attempt 10\nop-id: accept-10",false,Some(10),Some(Release::Exit)),
        ] {
            log.push(Entry {id,body:body.into(),edited});
            assert_eq!(delivered(7,&log,10),delivered_cursor,"entry {id}");
            assert_eq!(executions(7,&log)[0].release,release,"entry {id}");
            assert_eq!(revision(7,&log),10);
            let mut reversed=log.clone(); reversed.reverse();
            assert_eq!(executions(7,&reversed),executions(7,&log),"transport order is not board order");
        }
        log.extend([
            Entry {id:18,body:"taskq: steer\nnext".into(),edited:false},
            Entry {id:19,body:"taskq: executing 18 attempt 10".into(),edited:false},
            Entry {id:20,body:"taskq: released 19 no-start attempt 10".into(),edited:false},
            Entry {id:21,body:"taskq: accepted 19 attempt 10".into(),edited:false},
            Entry {id:22,body:"taskq: released 19 exit attempt 10".into(),edited:false},
        ]);
        assert_eq!(delivered(7,&log,10),Some(10),"no-start cannot later become acceptance");
        assert_eq!(executions(7,&log)[1].release,Some(Release::NoStart));
        log.extend([
            Entry {id:23,body:"taskq: executing 18 attempt 10\nop-id: retry".into(),edited:false},
            Entry {id:24,body:"taskq: executing 18 attempt 10\nop-id: retry".into(),edited:false},
            Entry {id:25,body:"taskq: accepted 23 attempt 10".into(),edited:false},
            Entry {id:26,body:"taskq: released 23 no-start attempt 10".into(),edited:false},
        ]);
        assert_eq!(executions(7,&log).len(),3);
        assert_eq!(executions(7,&log)[2].release,None,"accepted input cannot be declared never started");
        assert_eq!(delivered(7,&log,10),Some(18));
        log.extend([
            Entry {id:27,body:"taskq: requeue".into(),edited:false},
            Entry {id:28,body:"taskq: released 23 exit attempt 10".into(),edited:false},
            Entry {id:29,body:"taskq: spawn worker claude:new win".into(),edited:false},
            Entry {id:30,body:"taskq: accepted 23 attempt 10".into(),edited:false},
            Entry {id:31,body:"taskq: released 29 exit attempt 10".into(),edited:false},
        ]);
        assert_eq!(delivered(7,&log,29),None,"old evidence cannot accept a replacement");
        assert_eq!(executions(7,&log).last().unwrap().release,None);
    }

    // Chain: continuation refusal -> restart -> same input retry. Risk: intent consumes undelivered input.
    #[test]
    fn no_start_preserves_the_continuation_cursor() {
        use super::*;
        let bodies = [
            "taskq: spawn worker claude:s mac",
            "taskq: accepted 10 attempt 10",
            "taskq: released 10 exit attempt 10",
            "taskq: steer\ncontinue",
            "taskq: executing 13 attempt 10\nop-id: execute-10-13-10",
            "taskq: released 14 no-start attempt 10",
            "taskq: executing 13 attempt 10\nop-id: execute-10-13-14",
        ];
        let log: Vec<_> = bodies.iter().enumerate().map(|(i, body)| Entry { id: 10+i as u64, body: (*body).into(), edited:false }).collect();
        assert_eq!(executions(7,&log).iter().map(|x|x.id).collect::<Vec<_>>(), [10,14,16]);
        assert_eq!(revision(7,&log),10);
    }

    // Chain: submission -> task mutation/continuation -> exact release. Risk: task state is mistaken for execution exit.
    #[test]
    fn execution_release_survives_replacement_and_distinguishes_continuations() {
        use super::*;
        let mut log=vec![];
        for (id,body,edited,pending) in [
            (10,"taskq: spawn worker claude:s mac",false,vec![10]),
            (11,"taskq: requeue",false,vec![10]),
            (12,"taskq: released 10 exit attempt 999",false,vec![10]),
            (13,"taskq: released 10 exit attempt 10",true,vec![10]),
            (14,"taskq: released 10 exit attempt 10",false,vec![]),
            (15,"taskq: released 16 exit attempt 16",false,vec![]),
            (16,"taskq: spawn worker codex:s win",false,vec![16]),
            (17,"taskq: released 16 timeout attempt 16",false,vec![16]),
            (18,"taskq: released 16 no-start attempt 16\nop-id: release.16",false,vec![]),
            (19,"taskq: released 16 no-start attempt 16\nop-id: release.16",false,vec![]),
        ] {
            log.push(Entry {id,body:body.into(),edited});
            assert_eq!(executions(7,&log).iter().filter(|x|x.release.is_none()).map(|x|x.id).collect::<Vec<_>>(),pending,"entry{id}");
        }
        assert_eq!(revision(7,&log),16,"execution evidence does not change task revision");
        log.clear();
        for (id,body,pending) in [
            (8,"taskq: spawn worker claude:s mac",vec![8]),
            (9,"taskq: accepted 8 attempt 8",vec![8]),
            (11,"taskq: ask attempt 8\nWhich?\n1. x ★",vec![8]),
            (12,"taskq: released 8 exit attempt 8",vec![]),
            (13,"taskq: answer 1",vec![]),
            (14,"taskq: executing 13 attempt 8",vec![14]),
            (15,"taskq: released 8 exit attempt 8",vec![14]),
            (16,"taskq: released 14 exit attempt 8",vec![]),
            (17,"taskq: executing 13 attempt 8",vec![]),
            (18,"taskq: steer\nnew input",vec![]),
            (19,"taskq: accepted 14 attempt 8",vec![]),
            (20,"taskq: executing 18 attempt 8",vec![20]),
            (21,"taskq: released 20 exit attempt 9",vec![20]),
            (22,"taskq: released 20 exit attempt 8",vec![]),
        ] {
            log.push(Entry {id,body:body.into(),edited:false});
            let executions=executions(7,&log);
            assert_eq!(executions.iter().filter(|x|x.release.is_none()).map(|x|x.id).collect::<Vec<_>>(),pending,"entry{id}");
            assert!(executions.iter().all(|x|x.attempt==8 && x.host=="mac" && x.runtime=="claude"));
        }
        assert_eq!(executions(7,&log).iter().map(|x|x.through).collect::<Vec<_>>(),[8,13,18]);
    }

    // §8/§9: old readers ignore the new kind, so this is a cutover, never a mixed-reader rollout.
    #[test]
    fn usage_limit_requires_cutover() {
        let entries = |kind: &str| vec![
            super::Entry {id:1,body:"taskq: spawn worker claude:s mac".into(),edited:false},
            super::Entry {id:2,body:"taskq: gone attempt 1".into(),edited:false},
            super::Entry {id:3,body:"taskq: spawn worker claude:s mac".into(),edited:false},
            super::Entry {id:4,body:format!("taskq: {kind} attempt 3"),edited:false},
        ];
        let current = super::fold(1,&entries("usage-limit"),false);
        // Unknown kinds model the N-1 parser; legacy gone still retains its retry protection.
        let old = super::fold(1,&entries("unknown-to-reader"),false);
        let gone = super::fold(1,&entries("gone usage-limit"),false);
        assert_eq!((current.state,current.retries,current.attempt),(super::State::Ready,1,None));
        assert_eq!((old.state,old.retries,old.attempt),(super::State::Doing,1,Some(3)));
        assert_eq!((gone.state,gone.retries),(super::State::Ask,2));
    }

    // Chain: § 2 fold over tests/golden. Risk: fold drifts from the traces; a wrong expectation still passes.
    #[test]
    fn golden_traces() {
        assert!(super::golden(super::GOLDEN).unwrap() > 20);
        let wrong = super::GOLDEN.replacen("\"retries\": 2}", "\"retries\": 1}", 1);
        assert!(super::golden(&wrong).is_err(), "a wrong expectation must fail");
    }

    // Chain: § 2 Stages -> R6 `stage k/n` and the § 7 brief (#1880). Risk: a stage park loses the first open stage,
    // counts a non-Acceptance line, or a resumed attempt restarts at A1 outside its worktree.
    #[test]
    fn stages_name_the_first_open_acceptance_item() {
        use super::*;
        let body = "## Acceptance\n- A1. spec\nA2. code\n* A3. tests\nA04. not an id\nA2. duplicate\nNote A9 is prose";
        assert_eq!(acceptance(body), ["A1", "A2", "A3"]);
        assert_eq!(stages_line("Reason: x\nStages: A3 A1 A1"), Some(vec!["A1".into(), "A3".into()]));
        assert_eq!(stages_line("Stages: none"), Some(vec![]));
        for bad in ["Stages: A1 B2", "Stages:", "Reason: x"] { assert_eq!(stages_line(bad), None, "{bad}"); }
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!((stage(body, &[]), stage(body, &s(&["A1", "A3"])), stage(body, &s(&["A1", "A2", "A3"]))), (None, Some((2, 3)), Some((3, 3))));
        let log = |text: &str| vec![
            Entry { id: 10, body: "taskq: spawn worker claude:s1 mac".into(), edited: false },
            Entry { id: 11, body: format!("taskq: later until 2026-11-05T09:00:00Z attempt 10\nReason: wait\n{text}"), edited: false },
            Entry { id: 12, body: "taskq: requeue\nop-id: wake-11".into(), edited: false },
        ];
        assert!(task_context(7, body, &log("Stages: A1")).contains("Stages done: A1; continue at A2 (stage 2/3) in the existing worktree"));
        assert!(task_context(7, body, &log("Stages: A1 A2 A3")).contains("all 3 stages are done"));
        assert!(!task_context(7, body, &log("")).contains("Stages done"), "a task without stage records reads as before");
    }

    // Chain: historical prefix semantics, including ignored entries; timings are observations, not a gate.
    #[test]
    fn projection_preserves_prefix_transitions() {
        use super::*;
        let check = |n, entries: &[Entry]| {
            let mut log: Vec<_> = entries.iter().filter(|e| !e.edited).cloned().collect();
            log.sort_by_key(|e| e.id);
            let (mut expected, mut before) = (vec![], Task::default());
            for i in 0..log.len() {
                let after = fold(n, &log[..=i], false);
                if before.state != after.state { expected.push((log[i].id, before, after.clone())); }
                before = after;
            }
            assert_eq!(changes(n, entries), expected);
        };
        for tr in serde_json::from_str::<Vec<Trace>>(GOLDEN).unwrap() {
            check(tr.task, &tr.entries);
            let mut log = tr.entries.clone();
            log.reverse();
            check(tr.task, &log);
            log.extend(tr.entries.iter().cloned());
            check(tr.task, &log);
        }
        for cycles in [4, 128] {
            let mut log = vec![];
            for i in 0..cycles {
                let a = i * 4 + 1;
                for (id, body) in [(a, entry("spawn", &["worker", "codex:s", "mac"], None, "")),
                    (a+1, entry("ask", &[], Some(a), "Problem: detail\n1. continue ★\n2. later")),
                    (a+2, entry("answer", &["1"], None, "")),
                    (a+3, entry("requeue", &[], None, "fresh attempt"))] {
                    log.push(Entry {id, body, edited:false});
                }
            }
            check(7, &log);
            let began = std::time::Instant::now();
            for _ in 0..3 { std::hint::black_box(changes(7, &log)); }
            eprintln!("changes entries={} mean_us={}", log.len(), began.elapsed().as_micros()/3);
        }
    }

    // Chain: § 2 limit card (#1123). Risk: a card below the limit, a card added twice, or fix lines read as options.
    #[test]
    fn limit_text_adds_the_card_only_at_the_limit() {
        let t = |state, retries| super::Task { state, retries, ..Default::default() };
        let card = "Done: d\nProblem: p\n1. a ★\n2. b\nTo answer: x";
        assert_eq!(super::limit_text(7, &t(super::State::Review, 1), "requeue", "1. a"), "1. a");
        assert_eq!(super::limit_text(7, &t(super::State::Review, 2), "requeue", card), card);
        assert_eq!(super::limit_text(7, &t(super::State::Ask, 2), "requeue", "r"), "r");
        assert_eq!(super::limit_text(7, &t(super::State::Doing, 2), "steer", "r"), "r");
        let got = super::limit_text(7, &t(super::State::Doing, 1), "gone", "exit 1\n  2. b");
        assert_eq!(got.lines().take(4).collect::<Vec<_>>(), ["exit 1", "  2) b", "Done: the service stopped retrying; no new attempt starts until the owner answers.", "Problem: retry limit 2 reached by this gone; the task waits for the owner."]);
        assert_eq!(super::options(&got).iter().map(|o| (o.0, o.2)).collect::<Vec<_>>(), [(1, true), (2, false)]);
        assert!(super::limit_text(7, &t(super::State::Doing, 2), "requeue", "").starts_with("Done: "));
    }

    /// R8 made mechanical: every (state, entry) pair of the § 2 table is accepted by some golden trace.
    // Chain: R8 § 2 table to golden traces; spec size. Risk: a § 2 row without a trace, taskq.md over 300 lines or 60 KiB, or dated.
    #[test]
    fn every_table_row_has_a_golden_trace() {
        let spec = include_str!("../taskq.md");
        let mut seen = std::collections::HashSet::new();
        for tr in serde_json::from_str::<Vec<super::Trace>>(super::GOLDEN).unwrap() {
            let mut log = tr.entries.clone();
            log.sort_by_key(|e| e.id);
            for i in 0..log.len() {
                let (before, after) = (super::fold(tr.task, &log[..i], tr.deps_open), super::fold(tr.task, &log[..=i], tr.deps_open));
                if let (Some(l), true) = (super::parse(tr.task, &log[i].body), before != after) {
                    seen.insert((before.state.name().to_string(), l.kind.to_string()));
                }
            }
        }
        let ticks = |cell: &str| -> Vec<String> {
            cell.split('`').skip(1).step_by(2).map(|s| s.split(' ').next().unwrap().to_string()).collect()
        };
        let rows = spec.lines().skip_while(|l| !l.starts_with("| State | Entry |")).skip(2).take_while(|l| l.starts_with('|'));
        let mut missing = vec![];
        for row in rows {
            let cells: Vec<&str> = row.split('|').collect();
            let states = if cells[1].contains("any open") {
                ["ready", "waiting", "doing", "review", "ask"].map(String::from).to_vec()
            } else {
                ticks(cells[1])
            };
            for s in &states {
                for k in ticks(cells[2]) {
                    if !seen.contains(&(s.clone(), k.clone())) {
                        missing.push(format!("{s} + {k}"));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "§ 2 rows without a golden trace: {missing:?}");
        assert!(spec.lines().count() <= 300, "taskq.md over 300 lines");
        assert!(spec.replace("\r\n", "\n").len() <= 60 * 1024, "taskq.md over 60 KiB");
        let dated = |w: &[u8]| w.len() == 10 && w[4] == b'-' && w[7] == b'-' && w.iter().filter(|b| b.is_ascii_digit()).count() == 8;
        assert!(!spec.as_bytes().windows(10).any(dated), "taskq.md holds a date: history goes to git and issues");
    }

    // Chain: § 2 ask answer to resumed worker. Risk: the worker resumes with the wrong answer text.
    #[test]
    fn delivery_falls_back_to_option_then_first_line() {
        let e = |id, body: &str| super::Entry { id, body: body.into(), edited: false };
        let mut log = vec![e(10, "taskq: spawn worker claude:s1 mac"), e(11, "taskq: ask attempt 10\nWhich?\n1. x\n2. y ★")];
        log.push(e(12, "taskq answer 7.2"));
        assert_eq!(super::delivery(7, &log, 12), "y");
        log[2] = e(12, "taskq: answer 2\nuse y, keep x");
        assert_eq!(super::delivery(7, &log, 12), "use y, keep x");
        log[2] = e(12, "2");
        assert_eq!(super::delivery(7, &log, 12), "y");
        log[2] = e(12, " 2. Y ★");
        assert_eq!(super::delivery(7, &log, 12), "y");
        log[2] = e(12, "taskq answer 3");
        assert_eq!(super::delivery(7, &log, 12), "taskq answer 3");
        // § 2: a PM pick is never the answer; the worker receives the owner's option
        log[2] = e(12, "taskq: pick 1\nx is cheaper");
        log.push(e(13, "taskq answer 7.2"));
        assert_eq!(super::delivery(7, &log, 13), "y");
    }

    // Chain: § 2 timed park time. Risk: a malformed or unreal time keeps a task parked forever or wakes it at once.
    #[test]
    fn timed_park_time_is_one_real_utc_instant() {
        for ok in ["2026-11-05T09:00:00Z", "2028-02-29T23:59:59Z", "2000-02-29T00:00:00Z"] { assert!(super::utc(ok), "{ok}"); }
        for bad in ["2026-11-05T09:00:00", "2026-11-05T09:00:00+00:00", "2026-11-05T09:00:00.5Z", "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z", "2026-13-01T00:00:00Z", "2026-04-31T00:00:00Z", "2026-11-05T24:00:00Z", "2026-11-05 09:00:00Z",
            "2026-11-0aT09:00:00Z", "+026-11-05T09:00:00Z", "", "tomorrow"] {
            assert!(!super::utc(bad), "{bad}");
        }
    }
}
