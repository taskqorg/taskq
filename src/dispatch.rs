//! The operation dispatcher (taskq.md § 5): every task write goes through `act`, which checks the expected revision and
//! the op id, appends one entry that `fold` accepts, reads it back and returns a receipt.

use crate::board::{self, Board, Publish, entries};
use crate::core::{self, Entry, Gate, State, Task, entry, fold, options};
use crate::service::{self, Config};
use crate::{Res, join, poke};
use serde::Serialize;
use std::path::Path;

/// Whether a `Depends on` issue of `body` or the worker park's blocker in `log` is not done (§ 1, § 2); an unreadable
/// one counts as not done.
pub fn deps_open(b: &dyn Board, n: u64, body: &str, log: &[Entry]) -> bool {
    !service::blockers(b, &core::dep_body(body, &fold(n, log, false)), None, &mut Default::default()).is_empty()
}

/// This process's `TASKQ_TASK` and `TASKQ_ATTEMPT`.
type ChildEnv = (Option<String>, Option<String>);

/// `TASKQ_ATTEMPT` when this process is a child working on task `n` (§ 3 fencing).
fn own_attempt(n: u64, (task, attempt): &ChildEnv) -> Option<u64> {
    let task: u64 = task.as_deref()?.parse().ok()?;
    (task == n).then(|| attempt.as_deref()?.parse().ok()).flatten()
}

/// § 5: a child's `result`, `ask`, `requeue` or `gone` must come from the live attempt of its own task; a reused
/// session's stale `TASKQ_TASK`/`TASKQ_ATTEMPT` is refused by name instead of writing unfenced or ignored.
fn stale_child(n: u64, kind: &str, live: Option<u64>, (task, attempt): &ChildEnv) -> Option<String> {
    let attempt = attempt.as_deref()?;
    if !matches!(kind, "result" | "ask" | "requeue" | "gone") {
        return None;
    }
    let task = task.as_deref().unwrap_or_default();
    if task.parse() != Ok(n) {
        return Some(format!("TASKQ_TASK {task:?} is not #{n}: stale session environment"));
    }
    (attempt.parse().ok() != live).then(|| format!("TASKQ_ATTEMPT {attempt} is not the live attempt {}: stale session environment", live.map_or("none".into(), |a| a.to_string())))
}

pub fn current(b: &dyn Board, n: u64) -> Res<(board::Issue, Task, bool)> {
    let issue = b.issue(n)?;
    let log = entries(b, n)?;
    let deps = deps_open(b, n, &issue.body, &log);
    Ok((issue, fold(n, &log, deps), deps))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RoutingWriter {
    machine: String,
    pid: u32,
    started: String,
}

/// A crashed writer may leave an HTTP child; a later boot proves the entire request tree exited.
fn routing_writer_exited(writer: &RoutingWriter, machine: &str) -> Res<bool> {
    if machine.is_empty() || writer.machine!=machine {return Err("routing writer belongs to another machine; waiting for its host reconciliation".into());}
    if writer.pid==0 || writer.started.is_empty() {return Err("routing writer native identity unknown".into());}
    if crate::platform::started_before_boot(&writer.started).unwrap_or(false) {return Ok(true);}
    match coreq::process::identity(writer.pid).map_err(|e|format!("routing writer native identity unreadable: {e}"))? {
        Some(p) if p.started==writer.started => Ok(false),
        _ => Err("routing writer exited; HTTP child exit unknown; waiting for verified later host boot".into()),
    }
}

/// §5: finish the durable owner routing intent, shared by the CLI and automatic service reconciliation.
pub fn finish_reroute(b: &dyn Board, n: u64) -> Receipt {
    let finish=|| -> Res<Receipt> {
        let log=entries(b,n)?;
        let p=core::project(n,&log,false);
        let (id,from,to)=p.reroute.as_ref().ok_or("routing intent no longer pending")?;
        let machine=crate::hostname();
        if let Some(writing)=p.routing_writer {
            let writer=log.iter().find(|e|e.id==writing).and_then(|e|e.body.lines().find_map(|s|s.strip_prefix("writer: ")))
                .and_then(|s|serde_json::from_str::<RoutingWriter>(s).ok()).ok_or("routing writer native identity unknown; fence retained")?;
            if !routing_writer_exited(&writer,&machine)? {return Err("routing writer still live; fence retained".into());}
            b.comment(n,&format!("taskq: routing-release {writing}"))?;
        }
        if machine.is_empty() {return Err("routing writer machine unknown".into());}
        let native=coreq::process::identity(std::process::id()).map_err(|e|e.to_string())?.ok_or("routing writer native identity unavailable")?;
        let writer=serde_json::to_string(&RoutingWriter {machine,pid:native.pid,started:native.started}).map_err(|e|e.to_string())?;
        let writing=b.comment(n,&format!("taskq: routing-write {id}\nwriter: {writer}"))?;
        // No label operation may outlive this exclusive board claim, including a paused API call.
        let write=|| -> Res<()> {
            let (issue,task,_)=current(b,n)?;
            let fresh=core::project(n,&entries(b,n)?,false);
            if fresh.routing_writer!=Some(writing) || fresh.reroute!=p.reroute {return Err("routing writer claim not acquired; no labels changed".into());}
            if !issue.open || task.state!=State::Ready || fresh.executions.iter().any(|x|x.release.is_none()) {
                return Err("routing intent cannot change a non-ready task or unresolved execution".into());
            }
            let (old,new)=(format!("run-{from}"),format!("run-{to}"));
            // GitHub can commit the add before a failed remove; only this intent's two pins may overlap.
            let labels:Vec<_>=issue.labels.iter().filter(|l|**l!=new || !issue.labels.contains(&old)).cloned().collect();
            crate::labels::validate(&labels)?;
            if issue.labels.contains(&old) {b.labels(n,std::slice::from_ref(&new),std::slice::from_ref(&old))?;}
            else if !issue.labels.contains(&new) {return Err("runtime pins changed outside the pending reroute".into());}
            let (got,t,_)=current(b,n)?;
            let fresh=core::project(n,&entries(b,n)?,false);
            if !got.open || t.state!=State::Ready || fresh.routing_writer!=Some(writing) || fresh.reroute!=p.reroute
                || fresh.executions.iter().any(|x|x.release.is_none()) || !got.labels.contains(&new) || got.labels.contains(&old) {
                return Err("runtime pin/state/execution readback unconfirmed; routing fence retained".into());
            }
            Ok(())
        };
        let written=write();
        // Even a failed label/readback call has finished before this release; a later boot recovers an unconfirmed request tree.
        b.comment(n,&format!("taskq: routing-release {writing}"))?;
        written?;
        let done=b.comment(n,&format!("taskq: rerouted {id}"))?;
        let got=b.issue(n)?;
        let log=entries(b,n)?;
        let t=core::fold(n,&log,deps_open(b,n,&got.body,&log));
        let (old,new)=(format!("run-{from}"),format!("run-{to}"));
        if !core::project(n,&log,false).accepts(done) || !got.labels.contains(&new) || got.labels.contains(&old) {
            return Err("reroute completion readback unconfirmed".into());
        }
        Ok(Receipt {reason:format!("{old} -> {new}"),..Receipt::accepted(n,&Op::default(),if got.open {t.state} else {State::Closed},core::revision(n,&log))})
    };
    finish().unwrap_or_else(|e|Receipt {outcome:"unknown",next:"The service reconciles the routing intent; retry the same reroute command for readback.".into(),..Receipt::rejected(n,&Op::default(),e)})
}

/// `--op-id` and `--revision` of one command (§ 5).
#[derive(Clone, Default)]
pub struct Op {
    pub id: Option<String>,
    pub revision: Option<u64>,
}

/// What a command did (§ 5 receipt): `accepted` with the readback, `rejected`, `stale` or `unknown`.
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub task: u64,
    pub op_id: Option<String>,
    pub outcome: &'static str,
    pub state: Option<&'static str>,
    pub revision: Option<u64>,
    pub reason: String,
    pub next: String,
    /// Extra success lines, e.g. the publication of `close`.
    #[serde(skip)]
    pub extra: String,
    /// A late answer (§ 5): who wrote the answer that closed the question first and what it delivered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered: Option<Answered>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<bool>,
}

/// § 5 `answered`: the board login and delivered text of the answer that closed the question.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Answered {
    pub by: String,
    pub text: String,
}

impl Answered {
    /// The one § 5 wording every surface reads: `already answered by <by>: <text>`.
    fn line(&self) -> String {
        let by = if self.by.is_empty() { String::new() } else { format!(" by {}", self.by) };
        format!("already answered{by}: {}", self.text)
    }
}

/// § 5 `answered` of `log` from its `comments`: the answer that closed the last question, its writer and delivered text.
pub fn answered_in(n: u64, log: &[Entry], p: &core::Projection, comments: &[board::Comment]) -> Option<Answered> {
    let id = p.closing_answer(n, log)?;
    Some(Answered { by: comments.iter().find(|c| c.id == id).map(|c| c.author.clone()).unwrap_or_default(), text: core::delivery(n, log, id) })
}

/// #1918 conversation history: every accepted answer of `log` in board order, with the card it closed; late answers
/// saved as notes closed no card and stay out.
pub fn history(n: u64, log: &[Entry], p: &core::Projection, comments: &[board::Comment]) -> Vec<(Option<String>, Answered)> {
    p.answers.iter().filter(|a| !a.late).map(|a| {
        let by = comments.iter().find(|c| c.id == a.id).map(|c| c.author.clone()).unwrap_or_default();
        (core::card_before(n, log, a.id), Answered { by, text: core::delivery(n, log, a.id) })
    }).collect()
}

/// The answer that beat a late `answer` on `log` (#1785); one more comment read names its writer, when it can.
fn answered(b: &dyn Board, n: u64, kind: &str, log: &[Entry], deps: bool) -> Option<Answered> {
    let p = core::project(n, log, deps);
    p.closing_answer(n, log).filter(|_| kind == "answer")?;
    let comments = b.comments(n).unwrap_or_default();
    answered_in(n, log, &p, &comments)
}

impl Receipt {
    /// § 5 late answer: `already answered by <by>: <text>` in place of stale or refusal advice; no other receipt changes.
    fn late(self, answered: Option<Answered>) -> Receipt {
        let Some(a) = answered else { return self };
        let next = if self.state == Some("doing") { format!("To add to it: `taskq steer {} --text T`.", self.task) } else { format!("Read `taskq inbox {}`.", self.task) };
        Receipt { reason: format!("#{} {}", self.task, a.line()), next, answered: Some(a), ..self }
    }
    fn new(n: u64, op: &Op, outcome: &'static str, reason: impl Into<String>, next: impl Into<String>) -> Receipt {
        Receipt { task: n, op_id: op.id.clone(), outcome, state: None, revision: None, reason: reason.into(), next: next.into(), extra: String::new(), answered: None, note: None }
    }
    pub fn accepted(n: u64, op: &Op, state: State, revision: u64) -> Receipt {
        Receipt { state: Some(state.name()), revision: Some(revision), ..Receipt::new(n, op, "accepted", "", "") }
    }
    pub fn rejected(n: u64, op: &Op, reason: impl Into<String>) -> Receipt {
        Receipt::new(n, op, "rejected", reason, "Check the input and board/service status, then retry; use `taskq help` for syntax.")
    }
    pub fn ok(&self) -> bool {
        self.outcome == "accepted"
    }
    /// Notification failure cannot undo a confirmed board mutation or invite a duplicate write.
    pub fn wake(mut self, root: &Path) -> Self {
        if let Err(error) = poke(root) {
            if self.ok() {
                self.reason = error;
                self.next = "Board change accepted; check service status; do not repeat the board write.".into();
            }
        }
        self
    }
    /// The § 5 text: `#N <state>` (plus extra lines and, once, who answered first), else `Error:` and `Next:`.
    pub fn text(&self) -> String {
        if self.ok() {
            let won = self.answered.as_ref().map(Answered::line).unwrap_or_default();
            join(&[&format!("#{} {}", self.task, self.state.unwrap_or("unknown")), &self.extra, &won, &self.reason, &self.next]).replace("\n\n", "\n")
        } else {
            let why = if self.reason.starts_with('#') { self.reason.clone() } else { format!("#{}: {}", self.task, self.reason) };
            format!("Error: {}\nNext: {}", crate::compact(&why, 153), crate::compact(&self.next, 154))
        }
    }
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Preserve a late answer's original meaning; a reused option number never answers a newer card (§ 2/§ 5).
/// `now`: the task's state and revision. A repeat of the winning answer writes nothing (#1861); a closed task takes no note.
#[allow(clippy::too_many_arguments)]
fn answer_note(b: &dyn Board, root: &Path, n: u64, args: &[&str], text: &str, op: &Op, log: &[Entry], now: (State, u64), files: &[String]) -> Receipt {
    let source = op.revision.or_else(|| {
        let p = core::project(n, log, false);
        let id = p.closing_answer(n, log)?;
        Some(core::revision(n, &before(log, id)))
    });
    let said = if let Some(k) = args.first() {
        source.ok_or("original question revision unavailable; answer text is required").and_then(|r| {
            let p = core::project(n, &log.iter().filter(|e| e.id <= r).cloned().collect::<Vec<_>>(), false);
            if p.revision != r { return Err("original question revision unavailable") }
            p.task.card.as_deref().and_then(|card| options(card).into_iter().find(|o| o.0.to_string() == *k)).map(|o| o.1)
                .ok_or("choice not found in the original question")
        })
    } else { Ok(text.to_string()) };
    if let Some(id) = said.as_ref().ok().zip(source).filter(|_| files.is_empty()).and_then(|(said, r)| same_answer(n, log, r, said)) {
        let by = b.comments(n).unwrap_or_default().into_iter().find(|c| c.id == id).map(|c| c.author).unwrap_or_default();
        let answered = Answered { by, text: core::delivery(n, log, id) };
        return Receipt { reason: "Same answer; nothing saved".into(), answered: Some(answered), ..Receipt::accepted(n, op, now.0, now.1) };
    }
    match said {
        _ if now.0 == State::Closed => Receipt::rejected(n, op, format!("#{n} is closed")),
        Err(why) => Receipt::rejected(n, op, why),
        Ok(said) => {
            let r = source.unwrap_or(0).to_string();
            act_input(b, root, n, "steer", &["answer", &r], &said, &Op { id: op.id.clone(), revision: None }, None, None, files)
        }
    }
}

/// The entries of `log` before entry `id`.
fn before(log: &[Entry], id: u64) -> Vec<Entry> {
    log.iter().filter(|e| e.id < id).cloned().collect()
}

/// The accepted answer of `log` to the question at revision `source` that delivered `said`, whitespace aside (#1861).
fn same_answer(n: u64, log: &[Entry], source: u64, said: &str) -> Option<u64> {
    let p = core::project(n, log, false);
    let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    log.iter().filter(|e| p.accepts(e.id) && core::kind(n, &e.body) == "answer")
        .find(|e| core::revision(n, &before(log, e.id)) == source && words(&core::delivery(n, log, e.id)) == words(said))
        .map(|e| e.id)
}

fn note_receipt(b: &dyn Board, mut r: Receipt, n: u64, log: &[Entry], op: &Op) -> Receipt {
    if let Some(e) = log.iter().rev().find(|e| !e.edited && op.id.as_deref().is_none_or(|id| core::op_id(n, &e.body) == Some(id))).filter(|e| e.body.starts_with("taskq: steer")) {
        r.note = Some(true);
        r.reason = if r.state == Some("doing") { "Saved as a task note; queued to steer the worker" } else { "Saved as a task note; the current question is unchanged" }.into();
        if e.body.starts_with("taskq: steer answer ") { r.answered = answered(b, n, "answer", log, false); }
    }
    r
}

/// R4: append one entry that `fold` accepts, set the projected label, poke the local service. A seen op id returns
/// the first receipt; a stale revision or a refused entry writes nothing and names the current state (§ 5).
pub fn act(b: &dyn Board, root: &Path, n: u64, kind: &str, args: &[&str], text: &str, op: &Op) -> Receipt {
    act_child(b, root, n, kind, args, text, op, child_env())
}

/// The child environment; tests see none, so a worker's own `TASKQ_TASK` cannot change their outcome.
fn child_env() -> ChildEnv {
    if cfg!(test) { return (None, None); }
    (std::env::var("TASKQ_TASK").ok(), std::env::var("TASKQ_ATTEMPT").ok())
}

/// [`act`] for a process whose child environment is `env`.
#[allow(clippy::too_many_arguments)]
fn act_child(b: &dyn Board, root: &Path, n: u64, kind: &str, args: &[&str], text: &str, op: &Op, env: ChildEnv) -> Receipt {
    act_as(b, root, n, kind, args, text, op, own_attempt(n, &env), Some(&env))
}

/// [`act`] for an entry that names `attempt` (emergency recovery fences another process's attempt, § 4).
#[allow(clippy::too_many_arguments)]
fn act_as(b: &dyn Board, root: &Path, n: u64, kind: &str, args: &[&str], text: &str, op: &Op, attempt: Option<u64>, child: Option<&ChildEnv>) -> Receipt {
    act_input(b,root,n,kind,args,text,op,attempt,child,&[])
}

pub fn act_files(b:&dyn Board,root:&Path,n:u64,kind:&str,text:&str,op:&Op,files:&[String])->Receipt {
    let env=child_env();
    act_input(b,root,n,kind,&[],text,op,own_attempt(n,&env),Some(&env),files)
}

#[allow(clippy::too_many_arguments)]
fn act_input(b:&dyn Board,root:&Path,n:u64,kind:&str,args:&[&str],text:&str,op:&Op,attempt:Option<u64>,child:Option<&ChildEnv>,files:&[String])->Receipt {
    let (original_text,original_files)=(text,files);
    let read = || -> Res<(board::Issue, bool, Vec<Entry>)> {
        let issue = b.issue(n)?;
        let log = entries(b, n)?;
        let deps = deps_open(b, n, &issue.body, &log);
        Ok((issue, deps, log))
    };
    let (issue, deps, mut log) = match read() {
        Ok(x) => x,
        Err(e) => return Receipt { outcome: "unknown", ..Receipt::rejected(n, op, e) },
    };
    let projection = core::project(n, &log, deps);
    let (before, rev) = (projection.task, projection.revision);
    let accepted = |s: State, rev: u64| Receipt::accepted(n, op, s, rev);
    if let Some(id) = &op.id
        && log.iter().any(|e| !e.edited && core::op_id(n, &e.body) == Some(id.as_str()))
    {
        // a retry: the first write stands, no further board write (even after it cleared the attempt)
        return note_receipt(b, accepted(if issue.open { before.state } else { State::Closed }, rev), n, &log, op).wake(root);
    }
    if let Some(why) = child.and_then(|env| stale_child(n, kind, before.attempt, env)) {
        return Receipt { state: Some(before.state.name()), revision: Some(rev), next: "Run taskq from a fresh session of the live attempt, or unset TASKQ_TASK and TASKQ_ATTEMPT.".into(), ..Receipt::rejected(n, op, why) };
    }
    // Even a plain CLI answer fences the question it read, including a write racing its POST.
    let bound;
    let op = if kind == "answer" && attempt.is_none() && before.state == State::Ask && op.revision.is_none() {
        bound = Op { id: op.id.clone(), revision: Some(rev) }; &bound
    } else { op };
    if !issue.open {
        if kind == "answer" && attempt.is_none() { return answer_note(b, root, n, args, text, op, &log, (State::Closed, rev), files); }
        return Receipt::rejected(n, op, format!("#{n} is closed"));
    }
    // § 1: only `start` or a person admits an issue; a `ready` without `taskq` would never spawn or show in status
    if kind == "requeue" && !issue.labels.iter().any(|l| l == service::TASK_LABEL) {
        let r = Receipt::rejected(n, op, format!("#{n} lacks the {} label; requeue refused", service::TASK_LABEL));
        return Receipt { state: Some(before.state.name()), revision: Some(rev), next: format!("Owner: add the {} label to #{n} on the board, then retry.", service::TASK_LABEL), ..r };
    }
    if kind == "answer" && attempt.is_none() && (op.revision.is_some_and(|r| r != rev) || before.state != State::Ask) {
        return answer_note(b, root, n, args, text, op, &log, (before.state, rev), files);
    }
    let owner_note = kind == "steer" && attempt.is_none();
    if !owner_note && op.revision.is_some_and(|r| r != rev) {
        let next = format!("Read `taskq inbox {n}`; retry with --revision {rev} if still wanted.");
        let late = answered(b, n, kind, &log, deps);
        return Receipt { state: Some(before.state.name()), revision: Some(rev), ..Receipt::new(n, op, "stale", format!("revision {} is stale; current is {rev}", op.revision.unwrap_or(0)), next) }.late(late).wake(root);
    }
    if core::has_fields(text) {
        return Receipt::rejected(n, op, "text ends with an `op-id:` or `revision:` line; reword it");
    }
    let human = matches!(kind, "answer" | "steer") || kind == "requeue" && attempt.is_none();
    if !files.is_empty() && !human {return Receipt::rejected(n,op,"attachments require a human answer, steer or owner requeue");}
    let wrapped;
    let text = if human {
        if text.contains(board::reply::MARKER)||text.contains(crate::reply_files::MARKER) { return Receipt::rejected(n, op, "text contains a reserved reply marker; reword it, then send again"); }
        if args.is_empty() && kind != "requeue" && (files.is_empty()||!text.trim().is_empty()) && let Err(e) = crate::task_text(kind, text) { return Receipt::rejected(n, op, e); }
        wrapped = crate::wrap_reply(text);
        if core::has_fields(&wrapped) { return Receipt::rejected(n, op, "wrapped text ends with an `op-id:` or `revision:` line; reword it"); }
        &wrapped
    } else { text };
    let text = if !(kind == "requeue" && args == ["main-moved"]) && attempt.is_some() && attempt == before.attempt { core::limit_text(n, &before, kind, text) } else { text.into() };
    // Check the transition before uploading anything; file-only replies still carry a nonempty attachment manifest.
    let preview=if text.trim().is_empty()&&!files.is_empty(){"Attached files"}else{&text};
    let body = core::with_fields(entry(kind, args, attempt, preview), op.id.as_deref(), op.revision);
    log.push(Entry { id: u64::MAX, body: body.clone(), edited: false });
    if !core::project(n, &log, deps).accepts(u64::MAX) {
        let late = answered(b, n, kind, &log[..log.len() - 1], deps);
        return Receipt { state: Some(before.state.name()), revision: Some(rev), ..Receipt::rejected(n, op, format!("#{n} is {}: `{kind}` not accepted", before.state.name())) }.late(late);
    }
    let unknown = |e: String| Receipt::new(n, op, "unknown", e, format!("Run `taskq inbox {n}`, then retry with the same --op-id."));
    let files=match crate::reply_files::upload(b,n,files){Ok(files)=>files,Err(e)=>{tracing::info!(coreq_refusal=crate::reply_files::refusal_kind(&e));return Receipt::rejected(n,op,e);}};
    if before.publication.is_some() && matches!(kind, "requeue" | "later" | "failed" | "publication-failed")
        && let Err(e) = b.cancel_publication(n, before.result.as_deref().unwrap_or_default())
    {
        return Receipt::rejected(n, op, e);
    }
    let text=crate::reply_files::append(&text,&files);
    let body=core::with_fields(entry(kind,args,attempt,&text),op.id.as_deref(),op.revision);
    let id = match if human { board::reply::post(b, n, &body) } else { b.comment(n, &body) } {
        Ok(id) => id,
        Err(e) => return unknown(e),
    };
    // re-read: report what the board holds (R12); a concurrent writer may have won the same revision
    let comments = match b.comments(n) {
        Ok(c) => c,
        Err(e) => return unknown(e),
    };
    let log = match board::reply::log(n, &comments) {
        Ok(l) => l,
        Err(e) => return unknown(e),
    };
    let mut log: Vec<Entry> = log.into_iter().filter(|e| !e.edited).collect();
    log.sort_by_key(|e| e.id);
    let Some(i) = log.iter().position(|e| e.id == id) else { return unknown(format!("comment {id} not read back")) };
    // lost: an earlier entry moved the revision, or `fold` ignores this one (a raced op id included)
    let before_write = core::project(n, &log[..i], deps);
    if !owner_note && op.revision.is_some_and(|r| r != before_write.revision) || !core::project(n, &log[..=i], deps).accepts(id) {
        // the losing entry edits itself: an edited entry is ignored by this and older versions alike (§ 2, § 8)
        let stored = if human { b.comments(n).ok().and_then(|cs| cs.into_iter().find(|c| c.id == id)).map(|c| c.body) } else { Some(body.clone()) };
        let Some(stored) = stored else { return unknown(format!("comment {id} not read back for stale-write cleanup")); };
        if let Err(e) = b.edit_comment(n, id, &format!("{stored}\n\nignored: another write won")) {
            return unknown(e);
        }
        log[i].edited = true;
        if kind == "answer" && attempt.is_none() {
            let p = core::project(n, &log, deps);
            return answer_note(b, root, n, args, original_text, op, &log, (p.task.state, p.revision), original_files);
        }
        let projection = core::project(n, &log, deps);
        let (t, rev) = (projection.task, projection.revision);
        let late = answered(b, n, kind, &log, deps);
        return Receipt { state: Some(t.state.name()), revision: Some(rev), ..Receipt::new(n, op, "stale", "another write won; this entry is ignored", format!("Read `taskq inbox {n}`; current revision is {rev}.")) }.late(late).wake(root);
    }
    let projection = core::project(n, &log, deps);
    let (t, rev) = (projection.task, projection.revision);
    if let Err(e) = service::project(b, &issue, &t) {
        return unknown(e);
    }
    let deploy = crate::deploy::config(root);
    // § 2 Deploy: a delivered close waits for its first `deployed` entry; a failed write is the service pass's to repair
    if t.state == core::State::Closed && let Err(e) = crate::deploy::label(b, &issue, &log, deploy.as_ref()) {
        tracing::info!(task = n, "deploy label not written: {e}");
    }
    // § 2 Summary: a projection like the label; a failed write is the next pass's to repair, never the entry's
    if b.summaries() && let Err(e) = crate::summary::write(b, n, &issue.body, &comments, &log, &t, deploy.as_ref()) {
        tracing::info!(task = n, "summary not written: {e}");
    }
    note_receipt(b, accepted(t.state, rev), n, &log, op).wake(root)
}

/// § 7: publish a reviewed result, then `close`; later and failed tasks close without publication.
/// § 4 Profiling: the `close` stage times the whole command, from entry through every receipt.
pub fn close_one(b: &dyn Board, cfg: &Config, n: u64, verdict: &str, op: &Op) -> Receipt {
    close_checked(b, cfg, n, verdict, op, &|_, _| Ok(()))
}

/// `close_one` after `check` accepts the task read; the CLI verdict validation runs inside the timed stage.
pub fn close_checked(b: &dyn Board, cfg: &Config, n: u64, verdict: &str, op: &Op, check: &dyn Fn(&board::Issue, &Task) -> Res<()>) -> Receipt {
    let began = std::time::Instant::now();
    let mut seen = (0, "other".to_string(), None);
    let r = close_body(b, cfg, n, verdict, op, check, &mut seen);
    let (outcome, err) = seen.2.unwrap_or(if r.ok() { ("ok", "none") } else { ("failed", crate::profile::class(&r.reason)) });
    crate::profile::stage("close", n, seen.0, &seen.1, Some(began.elapsed()), outcome, err, crate::profile::Usage::NoModel);
    r
}

/// `seen`: the attempt and runtime once read, and an outcome override (a pending publication is retried).
fn close_body(b: &dyn Board, cfg: &Config, n: u64, verdict: &str, op: &Op, check: &dyn Fn(&board::Issue, &Task) -> Res<()>, seen: &mut (u64, String, Option<(&'static str, &'static str)>)) -> Receipt {
    let (issue, t, _) = match current(b, n) {
        Ok(x) => x,
        Err(e) => return Receipt::rejected(n, op, e),
    };
    *seen = (t.attempt.unwrap_or(0), t.session.as_deref().and_then(|s| s.split(':').next()).unwrap_or("other").to_string(), None);
    if let Err(e) = check(&issue, &t) {
        return Receipt::rejected(n, op, e);
    }
    // a retried close returns its first receipt before publishing again
    if t.state == State::Closed || op.id.is_some() && entries(b, n).is_ok_and(|l| l.iter().any(|e| core::op_id(n, &e.body) == op.id.as_deref())) {
        return act(b, &cfg.root, n, "close", &[], verdict, op);
    }
    // § 7: a named successor moves only once every other check passed, right before the close is written
    let successor = || successor_moving(b, &cfg.root, n, verdict, op, agent()).map_err(|r| Receipt { state: Some(t.state.name()), ..r });
    match t.state {
        State::Later | State::Failed => successor().map_or_else(|r| r, |_| act(b, &cfg.root, n, "close", &[], verdict, op)),
        State::Review if needs_approval(cfg, &t) && t.gate != Some(Gate::Approved) => {
            // § 7: no publication before the owner's approval of this result; the current policy decides (B4 fencing)
            let sha = t.result.unwrap_or_default();
            let r = Receipt::rejected(n, op, format!("gate is {}: the owner has not approved {}", t.gate.map_or("required", Gate::name), &sha[..sha.len().min(7)]));
            Receipt { state: Some(t.state.name()), next: format!("Owner: `taskq approve {n}`, or pick the approve option."), ..r }
        }
        State::Review => {
            let sha = t.result.unwrap_or_default();
            let rev = entries(b, n).map(|l| core::revision(n, &l)).unwrap_or(0);
            if op.revision.is_some_and(|r| r != rev) {
                return act(b, &cfg.root, n, "close", &[], verdict, op); // the stale receipt, before any publication
            }
            if let Err(r) = successor() { return r; }
            // a saved verdict (queued, or waiting on GitLab CI) is the service's to publish
            if t.publication.is_some() {
                return Receipt { extra: "publication pending; the service reconciles it".into(), ..Receipt::accepted(n, op, t.state, rev) };
            }
            if let Err(e) = verdict_label(b, &issue, verdict) {
                return Receipt { state: Some(t.state.name()), ..Receipt::rejected(n, op, e) };
            }
            // § 4 Profiling: `checks` is the CI verification and merge call
            if b.defer_publication(&cfg.publish) {
                let mut r = act(b, &cfg.root, n, "publish", &[], verdict, op);
                if r.ok() {
                    r.extra = "publication pending; the service reconciles it".into();
                    if let Publish::Enqueued(id) = b.publish(n, &sha, &cfg.publish, &issue.title, &format!("Task: {}", b.url(n))) {
                        // Intent is durable before admission; the service recovers even if this receipt write is lost.
                        let next = Op { revision: r.revision, ..Default::default() };
                        let _ = act_as(b, &cfg.root, n, "enqueued", &[&id], "", &next, None, None);
                    }
                }
                return r;
            }
            let began = std::time::Instant::now();
            let published = b.publish(n, &sha, &cfg.publish, &issue.title, &format!("Task: {}", b.url(n)));
            let (outcome, err) = match &published {
                Publish::Done(_) => ("ok", "none"),
                Publish::Pending(_) | Publish::Waiting(_) | Publish::Enqueued(_) | Publish::Rebased(_) => ("retried", "none"),
                Publish::MainMoved(m) => ("failed", crate::profile::class(m)),
                Publish::Failed(m) | Publish::Blocked(m) | Publish::Superseded { reason: m, .. } => ("failed", crate::profile::class(m)),
            };
            crate::profile::stage("checks", n, seen.0, &seen.1, Some(began.elapsed()), outcome, err, crate::profile::Usage::NoModel);
            if outcome == "retried" { seen.2 = Some((outcome, err)); }
            match published {
                Publish::Done(m) => {
                    // § 7 publication receipt: the policy it ran under and what the board read back
                    let m = format!("delivery {} r{}: {m}", cfg.delivery, cfg.delivery_revision);
                    let mut r = act(b, &cfg.root, n, "close", &[], &join(&[verdict, &m]), op);
                    r.extra = m;
                    r
                }
                Publish::Pending(m) | Publish::Blocked(m) | Publish::Superseded { reason: m, .. } => Receipt::rejected(n, op, format!("not closed: {m}")),
                // #1939: a running pipeline ends the reviewer with a saved verdict, never `gone`
                Publish::Waiting(m) => {
                    let mut r = act(b, &cfg.root, n, "publish", &[], verdict, op);
                    if r.ok() { r.extra = format!("publication pending: {m}; the service merges it when CI ends"); }
                    r
                }
                Publish::Enqueued(_) | Publish::Rebased(_) => Receipt::rejected(n, op, "publication needs the service"),
                Publish::MainMoved(m) => {
                    act(b, &cfg.root, n, "requeue", &["main-moved"], &m, &Op::default())
                }
                Publish::Failed(m) => {
                    let r = act(b, &cfg.root, n, "requeue", &[], &format!("publication failed: {m}"), &Op::default());
                    Receipt::rejected(n, op, if r.ok() { format!("requeued: {m}") } else { format!("publication failed: {m}; {}", r.reason) })
                }
            }
        }
        s => Receipt { state: Some(s.name()), ..Receipt::rejected(n, op, format!("#{n} is {}: `close` not accepted", s.name())) },
    }
}

/// #1979: an experiment's `Verdict:` becomes the `verdict-<v>` label before publication, confirmed by readback.
fn verdict_label(b: &dyn Board, issue: &board::Issue, verdict: &str) -> Res<()> {
    let Some(v) = crate::reports::outcome_verdict(verdict) else { return Ok(()) };
    let want = format!("verdict-{v}");
    let stale: Vec<String> = issue.labels.iter().filter(|l| l.starts_with("verdict-") && **l != want).cloned().collect();
    if !issue.labels.contains(&want) || !stale.is_empty() {
        b.labels(issue.number, std::slice::from_ref(&want), &stale)?;
    }
    let back = b.issue(issue.number)?;
    if back.labels.contains(&want) && !back.labels.iter().any(|l| l.starts_with("verdict-") && *l != want) { Ok(()) } else { Err(format!("not closed: label {want} did not read back")) }
}

/// Service continuation of the saved reviewer verdict; writes are not attempt-bound (section 7).
/// `issue`, `t` and `log` are the service pass's view; resuming reads no task state again (#1359).
pub fn resume_publication(b: &dyn Board, cfg: &Config, issue: &board::Issue, t: &Task, log: &[core::Entry]) -> Receipt {
    let n = issue.number;
    let Some(verdict) = &t.publication else { return Receipt::rejected(n, &Op::default(), "no pending publication") };
    if t.state != State::Review || needs_approval(cfg, &t) && t.gate != Some(Gate::Approved) {
        return Receipt::rejected(n, &Op::default(), "publication no longer approved");
    }
    let rev = core::revision(n, log);
    let op = Op { revision: Some(rev), ..Default::default() };
    let write = |kind, args: &[&str], text: &str| act_as(b, &cfg.root, n, kind, args, text, &op, None, None);
    match b.resume_publication(n, t.result.as_deref().unwrap_or_default(), &cfg.publish, &issue.title, &format!("Task: {}", b.url(n)), &cfg.root, t.enqueued.is_some()) {
        Publish::Done(m) => write("close", &[], &join(&[&repeats(n, log, verdict), &format!("delivery {} r{}: {m}", cfg.delivery, cfg.delivery_revision)])),
        Publish::Enqueued(id) if t.enqueued.is_none() => write("enqueued", &[&id], ""),
        Publish::Enqueued(_) => Receipt::accepted(n, &op, t.state, rev),
        Publish::Rebased(sha) => write("rebased", &[&sha], ""),
        Publish::MainMoved(m) => write("requeue", &["main-moved"], &m),
        Publish::Failed(m) => write("publication-failed", &[], &core::limit_text(n, &t, "requeue", &format!("publication failed: {m}"))),
        Publish::Blocked(m) => write("publication-blocked", &[], &blocked_card(n, &m, None)),
        Publish::Superseded { reason, merged } => write("publication-blocked", &[], &blocked_card(n, &reason, Some(&merged))),
        Publish::Pending(m) | Publish::Waiting(m) => Receipt::rejected(n, &op, m),
    }
}

/// § 2 Summary: the `close` of a saved verdict names its `publish` entry instead of copying it; without one, the copy.
fn repeats(n: u64, log: &[Entry], verdict: &str) -> String {
    log.iter().filter(|e| !e.edited && core::kind(n, &e.body) == "publish" && core::entry_text(n, &e.body) == verdict).map(|e| e.id).max()
        .map_or_else(|| verdict.to_string(), |id| format!("{}{id}", core::REPEATS))
}

/// § 2 ask card of a publication only the owner can unblock (#1413); its answer resumes publication.
/// `merged`: superseded by these merged task PRs (#1779), so a `close` option ends the task with no merge.
pub(crate) fn blocked_card(n: u64, reason: &str, merged: Option<&str>) -> String {
    let mut opts = vec![
        "retry: publish again - + resumes publication; - fix it on the board first (rerun CI or re-enqueue the PR)".to_string(),
        "later: park the task - + no publication runs; - waits until the owner requeues it".into(),
    ];
    if let Some(m) = merged {
        opts.push(format!("close: superseded, no merge - + merged {} carries the work; - this result is not merged", crate::plain(m, 60)));
    }
    crate::service_card(n, "the reviewed result is in publication; the service cannot finish it.", reason, &opts, if merged.is_some() { 3 } else { 1 })
}

/// § 7: a close whose verdict names a successor (`superseded by #M`) leaves it moving. A held successor is started
/// first, by the owner or PM only: an agent asks instead. An unreadable successor, or one closed without completing,
/// refuses the close.
fn successor_moving(b: &dyn Board, root: &Path, n: u64, verdict: &str, op: &Op, agent: bool) -> Result<(), Receipt> {
    let Some(m) = crate::display::successor(verdict) else { return Ok(()) };
    let refuse = |why: String, next: String| Err(Receipt { next, ..Receipt::rejected(n, op, why) });
    let (issue, t, _) = match current(b, m) {
        Ok(x) => x,
        Err(e) => return refuse(format!("successor #{m} unreadable: {e}"), "Ask the owner, or name no successor in the verdict.".into()),
    };
    if !issue.open {
        // § 1: GitLab records no reason, so a closed GitLab issue is done
        return if issue.reason.as_deref().is_none_or(|r| r == "completed") { Ok(()) } else {
            refuse(format!("successor #{m} closed without completing"), "Ask the owner which task carries the work.".into())
        };
    }
    if !issue.trusted {
        return refuse(format!("successor #{m} is not a trusted task"), "File the successor with `taskq add`, then close.".into());
    }
    if t.state != State::Held && !service::is_held(&issue) {
        return Ok(());
    }
    if agent {
        return refuse(format!("successor #{m} is held; an agent cannot start it"), format!("Ask the owner to start #{m}, then close."));
    }
    let r = crate::start_one(b, root, m, &Op { id: op.id.as_ref().map(|i| format!("{i}.start-{m}")), revision: None });
    if r.ok() { Ok(()) } else { refuse(format!("successor #{m} not started: {}", r.reason), format!("Run `taskq start {m}`, then close again.")) }
}

/// § 7: a gated task or a `review-pr` project publishes only the owner's approved result.
fn needs_approval(cfg: &Config, t: &Task) -> bool {
    t.gate.is_some() || cfg.delivery == "review-pr"
}

/// Whether this process is an agent (`TASKQ_ATTEMPT` set): it cannot approve a gate (§ 7).
pub fn agent() -> bool {
    std::env::var_os("TASKQ_ATTEMPT").is_some()
}

/// The owner's gate approval of the current result (§ 7). An agent cannot approve; `sha`, when given, must be that
/// result, so an approval never binds to a revision the owner did not see.
pub fn approve(b: &dyn Board, root: &Path, n: u64, sha: Option<&str>, op: &Op, agent: bool) -> Receipt {
    if agent {
        return Receipt::rejected(n, op, "an agent cannot approve a gate; the owner runs `approve`");
    }
    let t = match current(b, n) {
        Ok((_, t, _)) => t,
        Err(e) => return Receipt::rejected(n, op, e),
    };
    let Some(result) = t.result.clone() else {
        return Receipt { state: Some(t.state.name()), ..Receipt::rejected(n, op, format!("#{n} has no result to approve")) };
    };
    if let Some(s) = sha.filter(|s| !result.starts_with(s) || s.len() < 7) {
        return Receipt { state: Some(t.state.name()), ..Receipt::rejected(n, op, format!("{s} is not the current result {}", &result[..result.len().min(7)])) };
    }
    act_as(b, root, n, "approve", &[&result], "", op, None, None)
}

/// `pick N.K --text R`: the PM's view of an open `ask` card; never an answer (§ 2).
pub fn pm_pick(b: &dyn Board, root: &Path, pick: &str, reason: &str, op: &Op) -> Res<Receipt> {
    let (n, k) = pick.trim_start_matches('#').split_once('.').and_then(|(n, k)| Some((n.parse::<u64>().ok()?, k.parse::<usize>().ok()?))).ok_or(format!("not N.K: {pick}"))?;
    let reason = reason.trim();
    if reason.is_empty() || reason.contains('\n') || reason.chars().count() > 160 {
        return Err("pick needs a one-line --text reason of at most 160 characters".into());
    }
    let (issue, t, _) = current(b, n)?;
    if !issue.open || t.state != State::Ask {
        return Err(format!("#{n} is {} without an open question", if issue.open { t.state.name() } else { "closed" }));
    }
    if !options(t.card.as_deref().unwrap_or("")).iter().any(|o| o.0 == k) {
        return Err(format!("#{n} has no option {k}"));
    }
    Ok(act(b, root, n, "pick", &[&k.to_string()], reason, op))
}

/// `answer N --text`: on a blocked publication's card (#1820, pilotq #106), text that starts with an option key
/// (`close:`, `later:`, `retry:`) is that option; as free text it resumed the publication, which blocked again.
/// A key the card lacks is refused with the `N.K` form.
pub fn answer_text(b: &dyn Board, cfg: &Config, n: u64, text: &str, op: &Op) -> Res<Vec<Receipt>> {
    let key = text.split_once(':').map(|(k, _)| k.trim().to_lowercase()).filter(|k| ["close", "later", "retry"].contains(&k.as_str()));
    let card = match key {
        Some(_) => { let (_, t, _) = current(b, n)?; t.card.filter(|_| t.state == State::Ask && t.publication.is_some()) }
        None => None,
    };
    let (Some(key), Some(card)) = (key, card) else { return Ok(vec![act(b, &cfg.root, n, "answer", &[], text, op)]) };
    let opts = core::options(&card);
    let Some((k, ..)) = opts.iter().find(|o| o.1.to_lowercase().starts_with(&format!("{key}:"))) else {
        let forms: Vec<String> = opts.iter().map(|o| format!("`taskq answer {n}.{}` ({})", o.0, o.1.split(':').next().unwrap_or(""))).collect();
        return Err(format!("#{n} has no `{key}` option; answer {}", forms.join(" or ")));
    };
    answer_picks(b, cfg, &[format!("{n}.{k}")], op)
}

/// `answer N.K [M.K..]`: every pick is checked before any is written (§ 5).
pub fn answer_picks(b: &dyn Board, cfg: &Config, picks: &[String], op: &Op) -> Res<Vec<Receipt>> {
    let mut plan = vec![];
    for p in picks {
        let (n, k) = p.trim_start_matches('#').split_once('.').and_then(|(n, k)| Some((n.parse::<u64>().ok()?, k.parse::<usize>().ok()?))).ok_or(format!("not N.K: {p}"))?;
        let issue = b.issue(n)?;
        let log = entries(b, n)?;
        let deps = deps_open(b, n, &issue.body, &log);
        let t = fold(n, &log, deps);
        // A late answer goes to `act`, which saves its original meaning as a durable note.
        if op.id.as_deref().is_some_and(|id| log.iter().any(|e| !e.edited && core::op_id(n, &e.body) == Some(id)))
            || op.revision.is_some_and(|r| r != core::revision(n, &log))
            || !(matches!(t.state, State::Ask | State::Review) && t.card.is_some()) && core::project(n, &log, deps).closing_answer(n, &log).is_some()
        {
            plan.push((n, k, None));
            continue;
        }
        let card = t.card.clone().filter(|_| matches!(t.state, State::Ask | State::Review)).ok_or(format!("#{n} is {} without a question", t.state.name()))?;
        let opt = options(&card).into_iter().find(|o| o.0 == k).ok_or(format!("#{n} has no option {k}"))?.1;
        plan.push((n, k, Some((opt, t.state, needs_approval(cfg, &t), t.publication.is_some(), core::revision(n, &log)))));
    }
    let mut out = vec![];
    for (n, k, card) in plan {
        let Some((opt, state, gated, pending, revision)) = card else {
            out.push(act(b, &cfg.root, n, "answer", &[&k.to_string()], "", op));
            continue;
        };
        let bound = Op { id: op.id.clone(), revision: Some(op.revision.unwrap_or(revision)) };
        let op = &bound;
        let close = opt.to_lowercase().starts_with("close");
        if gated && opt.to_lowercase().starts_with("approve") {
            let Some(sha) = core::approval_sha(&opt) else {
                out.push(Receipt::rejected(n, op, "approval option must name the displayed result SHA"));
                continue;
            };
            // § 7: approve only the displayed revision, then resume the reviewer.
            let first = Op { id: op.id.as_ref().map(|i| format!("{i}.approve")), revision: op.revision };
            let r = approve(b, &cfg.root, n, Some(sha), &first, agent());
            if state == State::Review || !r.ok() {
                out.push(r);
                continue;
            }
            out.push(act(b, &cfg.root, n, "answer", &[&k.to_string()], &opt, op));
            continue;
        }
        out.push(match (state, close) {
            // § 2: an option that starts with `later` parks the task, like the retry limit card's (#1123)
            _ if opt.to_lowercase().starts_with("later") => {
                let r = act(b, &cfg.root, n, "later", &[], &opt, op);
                if r.outcome == "stale" { act(b, &cfg.root, n, "answer", &[&k.to_string()], &opt, op) } else { r }
            }
            (State::Ask, _) => {
                let r = act(b, &cfg.root, n, "answer", &[&k.to_string()], &opt, op);
                // the close an option starts runs after the answer, as one more op of the same id
                let then = Op { id: op.id.as_ref().map(|i| format!("{i}.close")), revision: None };
                // a blocked publication's close (superseded, #1779) ends the task with no merge; a publish would block again
                if close && r.ok() && r.note != Some(true) && r.state == Some("review") && pending { act(b, &cfg.root, n, "close", &[], &opt, &then) }
                else if close && r.ok() && r.note != Some(true) && r.state == Some("review") { close_one(b, cfg, n, &opt, &then) } else { r }
            }
            (_, true) => close_one(b, cfg, n, &opt, op),
            (_, false) => act(b, &cfg.root, n, "requeue", &[], &opt, op),
        });
    }
    Ok(out)
}

/// One interrupted attempt of this host in the emergency recovery proposal (§ 4).
#[derive(Debug, PartialEq)]
pub struct Row {
    pub task: u64,
    pub state: &'static str,
    pub attempt: u64,
    /// `running pid P`, `no child` or `unknown (<reason>)`.
    pub live: String,
    /// The fencing op `gone attempt <attempt>` at this revision; none while liveness is unknown.
    pub revision: Option<u64>,
}

impl Row {
    pub fn op_id(&self) -> String {
        format!("recover.{}.{}", self.task, self.attempt)
    }
    pub fn text(&self) -> String {
        let head = format!("#{} {} attempt {}: {}", self.task, self.state, self.attempt, self.live);
        match self.revision {
            Some(r) => format!("{head}; op: gone attempt {} --revision {r} --op-id {}", self.attempt, self.op_id()),
            None => format!("{head}; no op: blocks relaunch until its liveness reads"),
        }
    }
}

/// The recovery proposal (§ 4): every `doing` or `review` attempt of host `me`, read against the service's children
/// (`status.json` `kids`, `Err`: unreadable or stale). Only a child read running or absent gets a fencing op.
pub fn proposal(b: &dyn Board, me: &str, kids: &Result<Vec<serde_json::Value>, String>) -> Res<Vec<Row>> {
    let mut rows = vec![];
    for i in b.issues(None)?.iter().filter(|i| service::is_task(i)) {
        let n = i.number;
        let log = entries(b, n)?;
        let projection = core::project(n, &log, false);
        let t = &projection.task;
        let (Some(a), Some(h)) = (t.attempt, service::host_of(&t)) else { continue };
        if h != me || !matches!(t.state, State::Doing | State::Review) {
            continue;
        }
        let kid = kids.as_ref().map(|k| k.iter().find(|k| k["attempt"].as_u64() == Some(a)));
        let live = match &kid {
            Err(e) => Err(e.to_string()),
            Ok(None) => Ok("no child".to_string()),
            Ok(Some(k)) if k["live"] == "unknown" => Err(k["reason"].as_str().unwrap_or("no reason").to_string()),
            Ok(Some(k)) => Ok(format!("running pid {}", k["pid"])),
        };
        let (live, revision) = match live {
            Ok(l) => (l, Some(projection.revision)),
            Err(e) => (format!("unknown ({e})"), None),
        };
        rows.push(Row { task: n, state: t.state.name(), attempt: a, live, revision });
    }
    rows.sort_by_key(|r| r.task);
    Ok(rows)
}

/// Apply the proposal's ops through [`act`]: one receipt per op; a repeated apply returns the first receipts.
pub fn recover(b: &dyn Board, root: &Path, rows: &[Row]) -> Vec<Receipt> {
    rows.iter().filter_map(|r| {
        let op = Op { id: Some(r.op_id()), revision: Some(r.revision?) };
        Some(act_as(b, root, r.task, "gone", &[], "emergency recovery: attempt fenced", &op, Some(r.attempt), None))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Fake;

    // Chain: §5, #2137 A3. Recovery must not steal a live/foreign writer, and must finish a crashed partial label write.
    #[test]
    fn balancing_reroute_writer_recovery_preserves_ownership() {
        let b=Fake::default();
        let n=b.create("Reroute","",&["taskq".into(),"run-claude".into()]).unwrap();
        let intent=b.comment(n,"taskq: reroute claude codex\nrouting-fence: 1").unwrap();
        let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let machine=crate::hostname();
        let writer=|machine:&str,started:&str|serde_json::to_string(&RoutingWriter {machine:machine.into(),pid:native.pid,started:started.into()}).unwrap();
        for identity in [writer("invented-remote-host",&native.started),writer(&machine,&native.started),"invalid invented identity".into()] {
            let claim=b.comment(n,&format!("taskq: routing-write {intent}\nwriter: {identity}")).unwrap();
            let before=b.comments(n).unwrap().len();
            let r=finish_reroute(&b,n);
            assert_eq!(r.outcome,"unknown");assert!(r.reason.contains("writer"));
            assert_eq!(b.comments(n).unwrap().len(),before,"unknown or live writer cannot be released");
            assert_eq!(b.issue(n).unwrap().labels,["taskq","run-claude"]);
            b.comment(n,&format!("taskq: routing-release {claim}")).unwrap();
        }
        // Caller absence alone cannot release an orphaned HTTP child.
        let dead=writer(&machine,"invented-previous-incarnation");
        let claim=b.comment(n,&format!("taskq: routing-write {intent}\nwriter: {dead}")).unwrap();
        assert_eq!(finish_reroute(&b,n).outcome,"unknown");
        b.comment(n,&format!("taskq: routing-release {claim}")).unwrap();
        // Invented prior-boot identity: only this boundary proves the whole request tree cannot write again.
        #[cfg(target_os="macos")] let before_boot="1-0";
        #[cfg(target_os="linux")] let before_boot="00000000-0000-0000-0000-000000000000-1";
        #[cfg(windows)] let before_boot="1";
        #[cfg(not(any(target_os="macos",target_os="linux",windows)))] let before_boot="unsupported";
        let dead=writer(&machine,before_boot);
        b.comment(n,&format!("taskq: routing-write {intent}\nwriter: {dead}")).unwrap();
        b.labels(n,&["run-codex".into()],&[]).unwrap();
        assert!(finish_reroute(&b,n).ok());
        assert_eq!(b.issue(n).unwrap().labels,["taskq","run-codex"]);
        let p=core::project(n,&entries(&b,n).unwrap(),false);
        assert!(p.reroute.is_none()&&p.routing_writer.is_none());assert_eq!(p.task.retries,0);
    }

    // Chain: § 2 ask/answer log -> snapshot history -> conversation. Risk (#1918 A5): a second question replaced the
    // first question and its answer, so the conversation lost earlier decisions.
    #[test]
    fn history_keeps_every_answered_question_in_board_order() {
        let ask = |q: &str| format!("taskq: ask attempt 10\nDone: checked\nProblem: {q}\n1. Ship - + now; - risk\n2. Wait - + safe; - slow");
        let log: Vec<Entry> = [(10, "taskq: spawn worker claude:s win".to_string()), (11, ask("First?")), (12, "taskq: answer 1".into()),
            (13, ask("Second?")), (14, "taskq: answer\nnot yet".into()), (15, ask("Third?"))]
            .into_iter().map(|(id, body)| Entry { id, body, edited: false }).collect();
        let comment = |id, author: &str| board::Comment { id, created: 0, body: String::new(), trusted: true, edited: false, author: author.into() };
        let comments = [comment(12, "sam"), comment(14, "alex")];
        let p = core::project(7, &log, false);
        assert_eq!(p.task.state, State::Ask, "the third question is open");
        let h = history(7, &log, &p, &comments);
        let shown: Vec<_> = h.iter().map(|(c, a)| (c.as_deref().and_then(|c| c.lines().find(|l| l.starts_with("Problem: "))), a.by.as_str(), a.text.as_str())).collect();
        assert_eq!(shown, [(Some("Problem: First?"), "sam", "Ship - + now; - risk"), (Some("Problem: Second?"), "alex", "not yet")]);
        assert_eq!(answered_in(7, &log, &p, &comments), None, "the open question has no answer yet");
    }

    // Chain: § 5 all clients -> shared dispatcher -> stored board text. Risk: length rejects or loses a human reply.
    #[test]
    fn oversized_human_reply_is_complete_before_the_task_moves() {
        let text = "A reply with words and 🦀.\n".repeat(6000);
        for kind in ["answer", "steer", "requeue"] {
            let b = Fake::default();
            let n = b.create("Large reply fixture", "", &["taskq".into()]).unwrap();
            let a = b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
            if kind == "answer" { b.comment(n, &entry("ask", &[], Some(a), "Problem: reply")).unwrap(); }
            let before = b.comments(n).unwrap().len();
            let op = Op { id: Some("large-reply-once".into()), revision: Some(core::revision(n, &entries(&b, n).unwrap())) };
            let r = act(&b, Path::new(""), n, kind, &[], &text, &op);
            assert!(r.ok(), "{kind}: {}", r.text());
            let raw = b.comments(n).unwrap();
            assert!(raw.iter().all(|c| c.body.len() < 65536), "{kind}: one oversized comment still reaches the board");
            assert!(raw.len() > before + 2, "reply needs ordered parts plus one command");
            let log = entries(&b, n).unwrap();
            assert_eq!(core::delivery(n, &log, raw.last().unwrap().id).split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
            let legacy: Vec<_> = raw.iter().map(|c| Entry { id: c.id, body: c.body.clone(), edited: c.edited }).collect();
            assert_eq!(fold(n, &legacy, false), fold(n, &log, false), "N-1 folds the same task");
            for prefix in before..raw.len() - 1 {
                assert_eq!(fold(n, &legacy[..prefix], false), fold(n, &legacy[..before], false), "parts never move the task");
            }
            assert!(act(&b, Path::new(""), n, kind, &[], &text, &op).ok());
            assert_eq!(b.comments(n).unwrap().len(), raw.len(), "retry cannot duplicate parts or the command");
            b.edit_comment(n, raw[before].id, "changed part").unwrap();
            assert!(entries(&b, n).unwrap_err().contains("reply"), "corruption is unknown, never a partial delivery");
        }
    }

    // Chain: § 5 interrupted multipart POST -> board readback -> same operation. Risk: partial text resumes work or retry duplicates it.
    #[test]
    fn oversized_human_reply_recovers_a_lost_part_response() {
        let b = Fake::default();
        let n = b.create("Interrupted reply", "", &["taskq".into()]).unwrap();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let before = entries(&b, n).unwrap();
        let op = Op { id: Some("resume-reply".into()), revision: Some(core::revision(n, &before)) };
        let text = "Words before interruption.\n".repeat(6000);
        b.state.borrow_mut().lose_comment_response = true;
        let r = act(&b, Path::new(""), n, "steer", &[], &text, &op);
        assert_eq!(r.outcome, "unknown");
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false), fold(n, &before, false));
        let first = b.comments(n).unwrap().last().unwrap().body.clone();
        assert!(first.starts_with("taskq: reply-part "));
        assert!(act(&b, Path::new(""), n, "steer", &[], &text, &op).ok());
        let comments = b.comments(n).unwrap();
        assert_eq!(comments.iter().filter(|c| c.body == first).count(), 1);
        let log = entries(&b, n).unwrap();
        assert_eq!(core::delivery(n, &log, comments.last().unwrap().id).trim(), text.trim());
        for damage in ["missing", "untrusted", "edited", "forged-fields", "out-of-order"] {
            let mut bad = comments.clone();
            let part = bad.iter().position(|c| c.body == first).unwrap();
            match damage {
                "missing" => { bad.remove(part); }
                "untrusted" => bad[part].trusted = false,
                "edited" => bad[part].edited = true,
                "forged-fields" => bad.last_mut().unwrap().body.push_str("\nextra command text"),
                _ => bad[part].id = u64::MAX,
            }
            assert!(board::reply::log(n, &bad).is_err(), "{damage} must be unknown");
        }
        for body in ["A documented marker:\n<!-- taskq-reply example -->", "taskq: ask attempt 1\nProblem: inspect this syntax\n<!-- taskq-reply example -->", "taskq: answer\nA literal marker example\n<!-- taskq-reply example -->"] {
            let comments = [board::Comment { id: 1, body: body.into(), trusted: true, ..Default::default() }];
            assert_eq!(board::reply::log(n, &comments).unwrap()[0].body, body, "historical comments stay verbatim");
        }
    }

    // Chain: § 5 multipart write -> concurrent card -> bounded stale cleanup. Risk: an oversized edit leaves a losing command live.
    #[test]
    fn oversized_human_reply_race_edits_only_its_stored_reference() {
        let b = Fake::default();
        let n = b.create("Raced reply", "", &["taskq".into()]).unwrap();
        let a = b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        b.state.borrow_mut().race = Some(entry("ask", &[], Some(a), "Problem: a new decision"));
        let op = Op { id: Some("raced-reply".into()), revision: Some(a) };
        let r = act(&b, Path::new(""), n, "requeue", &[], &"word ".repeat(30_000), &op);
        assert_eq!(r.outcome, "stale", "{}", r.text());
        let raw = b.comments(n).unwrap();
        assert!(raw.last().unwrap().edited);
        assert!(raw.iter().all(|c| c.body.len() < 65536), "cleanup must edit the bounded stored reference");
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).card.as_deref(), Some("Problem: a new decision"));
    }

    #[test]
    fn human_replies_wrap_without_losing_words() {
        let short = format!("{}ok", "word ".repeat(53));
        let many = "word\n".repeat(400);
        assert_eq!((short.chars().count(), many.chars().count()), (267, 2000));
        for kind in ["answer", "steer", "requeue"] {
            for text in [&short, &many, &"🦀".repeat(481), &format!("{}\n\nlast paragraph", "word ".repeat(80))] {
                let b = Fake::default();
                let n = b.create("Reply fixture", "", &["taskq".into()]).unwrap();
                let attempt = b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
                if kind == "answer" { b.comment(n, &entry("ask", &[], Some(attempt), "Problem: reply")).unwrap(); }
                let before = b.comments(n).unwrap().len();
                let op = Op { id: Some("reply-once".into()), revision: None };
                let receipt = act(&b, Path::new(""), n, kind, &[], text, &op);
                assert!(receipt.ok(), "{kind}: {}", receipt.text());
                let log = entries(&b, n).unwrap();
                let stored = log.last().unwrap().body.split_once('\n').unwrap().1.rsplit_once("\nop-id:").unwrap().0.trim();
                assert!(stored.lines().all(|l| l.chars().count() <= 240), "{kind}: {stored}");
                assert_eq!(stored.split_whitespace().collect::<String>(), text.split_whitespace().collect::<String>());
                if text.contains(' ') || text.contains("word") { assert_eq!(stored.split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>()); }
                if text.contains("\n\n") { assert!(stored.contains("\n\nlast paragraph")); }
                assert!(act(&b, Path::new(""), n, kind, &[], text, &op).ok());
                assert_eq!(b.comments(n).unwrap().len(), before + 1, "same operation never writes twice");
            }
        }
        for text in ["", " \n "] {
            for kind in ["answer", "steer"] { assert!(crate::task_text(kind, text).unwrap_err().contains("type a reply")); }
        }
        let b = Fake::default();
        let n = b.create("Reserved suffix", "", &["taskq".into()]).unwrap();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let text = format!("{} op-id: forged", "x".repeat(239));
        assert!(!act(&b, Path::new(""), n, "steer", &[], &text, &Op::default()).ok(), "wrapping cannot inject metadata");
        assert_eq!(b.comments(n).unwrap().len(), 1);
    }

    // Chain: committed board mutation -> failed wake -> same op retry; never duplicate the board write.
    #[test]
    fn wake_failure_preserves_acceptance_and_retry_only_notifies() {
        let b = Fake::default();
        let root = std::env::temp_dir().join(format!("taskq-wake-receipt-{}", std::process::id()));
        let path = crate::dot(&root).join("poke");
        std::fs::create_dir_all(&path).unwrap();
        let n = b.create("Wake fixture", "## Goal\ng", &["taskq".into()]).unwrap();
        let op = Op { id: Some("hold-once".into()), revision: None };
        let first = act(&b, &root, n, "hold", &[], "", &op);
        assert!(first.ok(), "{}", first.text());
        assert!(first.text().contains("service wake failed"));
        assert!(first.json().contains("wake-open"));
        let before = entries(&b, n).unwrap();
        assert_eq!(first.state, Some("held"));
        std::fs::remove_dir(&path).unwrap();
        let retry = act(&b, &root, n, "hold", &[], "", &op);
        assert!(retry.ok()); assert!(retry.reason.is_empty());
        assert_eq!(retry.revision, first.revision);
        assert_eq!(entries(&b, n).unwrap().len(), before.len());
        assert!(path.is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: § 5 stale child environment (#1072). Risk: a reused session's TASKQ_TASK/TASKQ_ATTEMPT writes unfenced or ignored.
    #[test]
    fn stale_child_environment_is_refused() {
        let s = |k: &str, task: Option<&str>, attempt: Option<&str>| stale_child(7, k, Some(42), &(task.map(Into::into), attempt.map(Into::into)));
        assert_eq!(s("requeue", Some("7"), Some("42")), None);
        assert_eq!(s("requeue", None, None), None); // the owner, not a child
        assert_eq!(s("hold", Some("9"), Some("1")), None); // a child may add and hold another task
        assert_eq!(s("requeue", Some("9"), Some("42")).unwrap(), "TASKQ_TASK \"9\" is not #7: stale session environment");
        assert_eq!(s("result", None, Some("42")).unwrap(), "TASKQ_TASK \"\" is not #7: stale session environment");
        assert_eq!(s("ask", Some("7"), Some("41")).unwrap(), "TASKQ_ATTEMPT 41 is not the live attempt 42: stale session environment");
        assert!(stale_child(7, "gone", None, &(Some("7".into()), Some("42".into()))).unwrap().contains("live attempt none"));
    }

    // Chain: § 5 op-id replay over § 3 fencing (#1072). Risk: retrying an accepted requeue, whose write cleared the
    // attempt, is refused as stale instead of returning the first receipt.
    #[test]
    fn retried_child_op_id_returns_the_first_receipt_after_its_attempt_clears() {
        let (b, root) = (Fake::default(), std::env::temp_dir().join(format!("taskq-replay-{}", std::process::id())));
        let n = b.create("T", "## Goal
g", &["taskq".into()]).unwrap();
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
        let env = (Some(n.to_string()), Some(a.to_string()));
        let op = |id: &str| Op { id: Some(id.into()), revision: None };
        let count = || b.comments(n).unwrap().len();
        let first = act_child(&b, &root, n, "requeue", &[], "blocked", &op("rq"), env.clone());
        assert_eq!((first.outcome, first.state), ("accepted", Some("ready")), "{}", first.text());
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).attempt, None);
        let before = count();
        let again = act_child(&b, &root, n, "requeue", &[], "blocked", &op("rq"), env.clone());
        assert_eq!((again.outcome, again.state, again.revision, count()), ("accepted", Some("ready"), first.revision, before));
        // a new op from the same, now stale, environment is still refused by name
        let other = act_child(&b, &root, n, "requeue", &[], "blocked", &op("rq2"), env);
        assert_eq!((other.outcome, count()), ("rejected", before));
        assert!(other.text().contains("is not the live attempt none"), "{}", other.text());
    }

    // Chain: § 7 superseded close (pilotq#90). Risk: #90 closed for #104 while #104 stayed held with nobody to start it.
    #[test]
    fn superseded_close_starts_a_held_successor_or_refuses() {
        let (b, root) = (Fake::default(), std::env::temp_dir().join(format!("taskq-successor-{}", std::process::id())));
        let held = || {
            let m = b.create("Successor", "## Goal\ng", &["q-held".into()]).unwrap();
            b.comment(m, &entry("hold", &[], None, "")).unwrap();
            m
        };
        let (old, m) = (b.create("Old", "## Goal\ng", &["taskq".into()]).unwrap(), held());
        b.comment(old, &entry("later", &[], None, "parked")).unwrap();
        let verdict = format!("Changed: none; #{old} closes as superseded by #{m}\nChecks: A1 unknown");
        // an agent never starts work: it asks instead, and nothing is written to either task
        let before = (b.comments(old).unwrap().len(), b.comments(m).unwrap().len());
        let r = successor_moving(&b, &root, old, &verdict, &Op::default(), true).unwrap_err();
        assert!(r.reason.contains("is held") && r.next.contains(&format!("start #{m}")), "{}", r.text());
        assert_eq!((b.comments(old).unwrap().len(), b.comments(m).unwrap().len()), before);
        // the owner's or PM's close starts it first, then closes
        successor_moving(&b, &root, old, &verdict, &Op::default(), false).unwrap();
        let (issue, t, _) = current(&b, m).unwrap();
        assert_eq!((t.state, service::is_task(&issue)), (State::Ready, true));
        assert_eq!(act(&b, &root, old, "close", &[], &verdict, &Op::default()).state, Some("closed"));
        assert_eq!(crate::display::close_outcome(Some(&verdict), Some(t.state.name())), format!("replaced by #{m} (ready)"));
        // a moving successor needs nothing; one closed without completing refuses
        successor_moving(&b, &root, old, &verdict, &Op::default(), true).unwrap();
        b.close(m).unwrap();
        successor_moving(&b, &root, old, &verdict, &Op::default(), true).unwrap();
        assert!(successor_moving(&b, &root, old, "Changed: replaced by #9999", &Op::default(), false).unwrap_err().reason.contains("unreadable"));
    }

    // Chain: § 1 admission over § 2 `requeue` (#1735). Risk: requeue of a legacy `q-later` issue without `taskq`
    // prints `ready`, yet status hides it and no service spawns it.
    #[test]
    fn requeue_without_task_label_is_refused_by_name() {
        let (b, root) = (Fake::default(), std::env::temp_dir().join(format!("taskq-unlabelled-{}", std::process::id())));
        let n = b.create("Legacy", "## Goal\ng", &["q-later".into()]).unwrap();
        b.comment(n, &entry("later", &[], None, "parked")).unwrap();
        let before = b.comments(n).unwrap().len();
        let r = act(&b, &root, n, "requeue", &[], "", &Op::default());
        assert_eq!((r.outcome, r.state, b.comments(n).unwrap().len()), ("rejected", Some("later"), before), "{}", r.text());
        assert!(r.text().contains("lacks the taskq label"), "{}", r.text());
        assert_eq!(b.issue(n).unwrap().labels, ["q-later"]);
        b.labels(n, &["taskq".into()], &[]).unwrap();
        let r = act(&b, &root, n, "requeue", &[], "", &Op::default());
        assert_eq!((r.outcome, r.state), ("accepted", Some("ready")), "{}", r.text());
        assert!(service::is_task(&b.issue(n).unwrap()));
    }

    // Chain: § 2 retry limit to the owner's answer (#913, #1123). Risk: a reviewer's 3rd requeue after a lost double
    // claim parks the task in `ask` on a card without options, so no answer moves it.
    #[test]
    fn retry_limit_requeue_asks_with_requeue_and_later_options() {
        let (b, root) = (Fake::default(), std::env::temp_dir().join(format!("taskq-limit-{}", std::process::id())));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
        let (cfg, _) = crate::load(&root).unwrap();
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let at_limit = |reviewers: &[&str]| {
            let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
            for _ in 0..2 {
                let a = b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
                let r = act_child(&b, &root, n, "requeue", &[], "blocked", &Op::default(), (Some(n.to_string()), Some(a.to_string())));
                assert_eq!(r.state, Some("ready"), "{}", r.text());
            }
            let t = fold(n, &entries(&b, n).unwrap(), false);
            assert_eq!((t.retries, t.card), (2, None), "a requeue below the limit carries no card");
            let a = b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
            b.comment(n, &entry("result", &[sha], Some(a), "ok")).unwrap();
            // both hosts spawn a reviewer for this result; the first wins, the second is ignored
            let ids: Vec<u64> = reviewers.iter().map(|h| b.comment(n, &entry("spawn", &["reviewer", "codex:v", h], None, "")).unwrap()).collect();
            assert_eq!(fold(n, &entries(&b, n).unwrap(), false).attempt, Some(ids[0]));
            let r = act_child(&b, &root, n, "requeue", &[], "Fix:
1. x", &Op::default(), (Some(n.to_string()), Some(ids[0].to_string())));
            assert_eq!(r.state, Some("ask"), "{}", r.text());
            n
        };
        let n = at_limit(&["win", "mac"]);
        let t = fold(n, &entries(&b, n).unwrap(), false);
        let card = t.card.unwrap();
        assert_eq!((t.retries, card.lines().take(4).collect::<Vec<_>>()), (3, vec!["Fix:", "1) x", "Done: the service stopped retrying; no new attempt starts until the owner answers.", "Problem: retry limit 3 reached by this requeue; the task waits for the owner."]));
        assert_eq!(options(&card).into_iter().map(|o| (o.0, o.1.split(':').next().unwrap().to_string())).collect::<Vec<_>>(), [(1, "requeue".into()), (2, "later".into())]);
        assert!(b.issue(n).unwrap().labels.contains(&"q-ask".to_string()));
        assert_eq!(card.lines().last(), Some(format!("To answer: `taskq answer {n}.K`; a bare comment `K` is a plain answer, never `close` or `later`.").as_str()));
        // `taskq answer N.2` parks the task; a bare board comment `2` is a plain answer to `ready` (§ 2)
        let r = answer_picks(&b, &cfg, &[format!("{n}.2")], &Op::default()).unwrap().remove(0);
        assert_eq!((r.outcome, r.state), ("accepted", Some("later")), "{}", r.text());
        let n = at_limit(&["win", "mac"]);
        b.comment(n, "2").unwrap();
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).state, State::Ready);
        // `requeue` retries it with a fresh attempt
        let n = at_limit(&["mac", "win"]);
        let r = answer_picks(&b, &cfg, &[format!("{n}.1")], &Op::default()).unwrap().remove(0);
        assert_eq!((r.outcome, r.state), ("accepted", Some("ready")), "{}", r.text());
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).retries, 0);
    }

    // Chain: § 2 PM pick. Risk: a pick lands on a closed card, names no option, or becomes the answer.
    #[test]
    fn pm_pick_records_a_view_and_refuses_bad_input() {
        let (b, root) = (Fake::default(), std::env::temp_dir().join(format!("taskq-pick-{}", std::process::id())));
        let n = b.create("T", "## Goal\ng", &["taskq".into()]).unwrap();
        let pick = |k: &str, r: &str| pm_pick(&b, &root, &format!("{n}.{k}"), r, &Op::default());
        assert!(pick("1", "cheaper").is_err_and(|e| e.contains("without an open question")), "no card yet");
        b.comment(n, &entry("spawn", &["worker", "claude:u", "mac"], None, "")).unwrap();
        let a = fold(n, &entries(&b, n).unwrap(), false).attempt.unwrap();
        b.comment(n, &entry("ask", &[], Some(a), "Which?\n1. x\n2. y ★")).unwrap();
        let count = || b.comments(n).unwrap().len();
        let before = count();
        assert!(pick("3", "r").is_err_and(|e| e.contains("no option 3")));
        assert!(pick("1", " ").is_err() && pick("1", "a\nb").is_err() && pick("1", &"x".repeat(161)).is_err());
        assert_eq!(count(), before, "refusals write nothing");
        let r = pick("1", "x is cheaper").unwrap();
        assert_eq!((r.outcome, r.state), ("accepted", Some("ask")), "{}", r.text());
        assert_eq!(fold(n, &entries(&b, n).unwrap(), false).pick, Some((1, "x is cheaper".into())));
        b.close(n).unwrap();
        assert!(pick("1", "x").is_err_and(|e| e.contains("closed")));
    }

    // Chain: § 2 § 5 owner send -> board readback -> durable note -> attempt inbox.
    // Risk: a race drops intent, reuses a new option number, bypasses other revisions or duplicates delivery.
    #[test]
    fn late_answers_preserve_meaning_and_reconcile_once() {
        let b = Fake::default();
        let root = std::env::temp_dir().join(format!("taskq-notes-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("taskq.json"), r#"{"repo":"o/r"}"#).unwrap();
        let (cfg, _) = crate::load(&root).unwrap();
        let n = b.create("T", "## Goal\nkeep safe\n\n## Acceptance\nA1. no loss", &["taskq".into()]).unwrap();
        let a = b.comment(n, "taskq: spawn worker codex:s win").unwrap();
        let card = b.comment(n, &entry("ask", &[], Some(a), "Problem: Which?\n1. old x\n2. old y")).unwrap();
        let op = |id: &str, r| Op { id: Some(id.into()), revision: Some(r) };
        let count = || b.comments(n).unwrap().len();
        let before = count();
        assert_eq!(act(&b,&root,n,"requeue",&[],"Reason: retry",&op("stale-action",a)).outcome,"stale");
        assert_eq!(count(),before,"non-answer stale actions still write nothing");
        assert_eq!(act(&b,&root,n,"answer",&["1"],"",&op("plain",card)).outcome,"accepted");
        for (_, c) in b.state.borrow_mut().comments.iter_mut().filter(|(i, _)| *i == n) { c.author = "alex".into(); }
        let late = act(&b,&root,n,"answer",&["2"],"",&op("late",card));
        assert_eq!((late.outcome,late.state),("accepted",Some("doing")));
        assert_eq!(late.answered.as_ref().unwrap().text,"old x");
        assert!(late.reason.contains("task note") && !late.reason.contains("old x"), "{}", late.reason);
        assert_eq!(late.text().matches("old x").count(), 1, "#1861: the receipt names the winner once
{}", late.text());
        assert!(late.text().contains("already answered by alex: old x"));
        let written = count();
        for (id, args, text) in [("same", &["1"][..], ""), ("same-text", &[][..], " old  x ")] {
            let same = act(&b,&root,n,"answer",args,text,&op(id,card));
            assert_eq!((same.outcome,same.state,same.reason.as_str()),("accepted",Some("doing"),"Same answer; nothing saved"),"{id}");
            assert_eq!(same.text().matches("old x").count(), 1, "{id}: {}", same.text());
            assert_eq!(same.answered.as_ref().map(|a| a.by.as_str()), Some("alex"));
        }
        assert_eq!(count(),written,"#1861: a repeat of the winning answer saves no note and steers nothing");
        let log = entries(&b,n).unwrap();
        assert_eq!(core::delivery(n,&log,*fold(n,&log,false).inbox.last().unwrap()),"old y");
        let written = count();
        for id in ["plain","late"] { assert!(act(&b,&root,n,"answer",&["2"],"",&op(id,card)).ok()); }
        assert_eq!(count(),written,"lost receipts reconcile the same ID without another note");
        let newer = b.comment(n,&entry("ask",&[],Some(a),"Problem: New question\n1. new x\n2. new y")).unwrap();
        let r = answer_picks(&b,&cfg,&[format!("{n}.2")],&op("old-choice",card)).unwrap().remove(0);
        assert_eq!((r.outcome,r.state,r.revision),("accepted",Some("ask"),Some(newer)));
        assert!(r.answered.is_none(), "a newer unanswered question has no invented winner");
        let log = entries(&b,n).unwrap();
        assert_eq!(core::delivery(n,&log,*core::project(n,&log,false).notes.last().unwrap()),"old y");
        assert!(fold(n,&log,false).card.unwrap().contains("New question"));
        assert_eq!(act(&b,&root,n,"answer",&["9"],"",&op("invalid-choice",card)).outcome,"rejected");
        assert_eq!(act(&b,&root,n,"answer",&["1"],"",&op("unknown-question",a)).outcome,"rejected");
        assert_eq!(act(&b,&root,n,"steer",&[],"text\nop-id: spoof",&Op::default()).outcome,"rejected");
        assert_eq!(act(&b,&root,n,"result",&["abc"],"",&Op::default()).outcome,"rejected");
        b.state.borrow_mut().race = Some(core::with_fields(entry("answer",&["1"],None,"new x"),Some("winner"),Some(newer)));
        let r = act(&b,&root,n,"answer",&["2"],"",&op("raced",newer));
        assert_eq!((r.outcome,r.state),("accepted",Some("doing")));
        let log = entries(&b,n).unwrap();
        assert!(log.iter().any(|e| e.edited),"the losing answer is fenced before its note is written");
        assert_eq!(core::delivery(n,&log,*fold(n,&log,false).inbox.last().unwrap()),"new y");
        let written=count();
        assert!(act(&b,&root,n,"answer",&["2"],"",&op("raced",newer)).ok());
        assert_eq!(count(),written);
        let context=core::task_context(n,&b.issue(n).unwrap().body,&log);
        for text in ["A1. no loss","old x","old y","new x","new y","ask once immediately","Before hand-in"] {assert!(context.contains(text),"{text}");}
        let v:serde_json::Value=serde_json::from_str(&r.json()).unwrap();
        assert_eq!((v["task"].as_u64(),v["op_id"].as_str()),(Some(n),Some("raced")));
        let third = b.comment(n,&entry("ask",&[],Some(a),"Problem: Third
1. z
2. w")).unwrap();
        b.state.borrow_mut().race = Some(core::with_fields(entry("answer",&["1"],None,"z"),Some("winner-z"),Some(third)));
        let notes = |b: &Fake| entries(b,n).unwrap().iter().filter(|e| core::kind(n,&e.body) == "steer").count();
        let noted = notes(&b);
        let r = act(&b,&root,n,"answer",&["1"],"",&op("raced-same",third));
        assert_eq!((r.outcome,r.reason.as_str()),("accepted","Same answer; nothing saved"),"{}",r.text());
        assert_eq!(notes(&b),noted,"#1861: losing the race to the same answer writes no note");
        b.close(n).unwrap();
        let written = count();
        assert_eq!(act(&b,&root,n,"answer",&["1"],"",&op("closed-same",third)).outcome,"accepted");
        let other = act(&b,&root,n,"answer",&["2"],"",&op("closed-other",third));
        assert_eq!((other.outcome,other.reason.as_str()),("rejected",format!("#{n} is closed").as_str()));
        assert_eq!(count(),written,"a closed task takes no note");
        std::fs::remove_dir_all(root).unwrap();
    }

    // Chain: owner answer plan -> concurrent board write -> note receipt -> no consequential continuation.
    // Risk: a note authorizes close/publication, reuses a new question number, or disappears on a steer race.
    #[test]
    fn review_note_never_closes_or_publishes() {
      for action in ["close", "later"] {
        for pending in [false,true] {
            let b=Fake::default();
            let root=std::env::temp_dir().join(format!("taskq-note-close-{}",crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("taskq.json"),r#"{"repo":"o/r"}"#).unwrap();
            let (cfg,_)=crate::load(&root).unwrap();
            let n=b.create("T","## Goal\ng",&["taskq".into()]).unwrap();
            let a=b.comment(n,"taskq: spawn worker codex:s win").unwrap();
            b.comment(n,&entry("result",&[&"a".repeat(40)],Some(a),"")).unwrap();
            let text=format!("Problem: Publish?\n1. {action}: ship\n2. keep reviewing");
            let card=if pending {
                b.comment(n,"taskq: publish\nChanged: accepted\nChecks: pending\nOpen: none").unwrap();
                b.comment(n,&entry("publication-blocked",&[],None,&text)).unwrap()
            } else {
                let r=b.comment(n,"taskq: spawn reviewer codex:r win").unwrap();
                b.comment(n,&entry("ask",&[],Some(r),&text)).unwrap()
            };
            b.state.borrow_mut().race=Some(entry("answer",&["2"],None,"keep reviewing"));
            let r=answer_picks(&b,&cfg,&[format!("{n}.1")],&Op{id:Some("close-race".into()),revision:Some(card)}).unwrap().remove(0);
            assert_eq!(r.note,Some(true));
            assert_eq!(fold(n,&entries(&b,n).unwrap(),false).state,State::Review,"a saved note cannot close");
            assert_eq!(b.state.borrow().publications,0,"a saved note cannot authorize publication");
            assert!(b.issue(n).unwrap().open);
            std::fs::remove_dir_all(root).unwrap();
        }
      }
    }

    #[test]
    fn review_note_keeps_a_numbered_answers_original_snapshot() {
        let b=Fake::default();
        let root=std::env::temp_dir().join(format!("taskq-note-read-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("taskq.json"),r#"{"repo":"o/r"}"#).unwrap();
        let (cfg,_)=crate::load(&root).unwrap();
        let n=b.create("T","## Goal\ng",&["taskq".into()]).unwrap();
        let a=b.comment(n,"taskq: spawn worker codex:s win").unwrap();
        b.comment(n,&entry("ask",&[],Some(a),"Problem: First?\n1. original meaning\n2. other")).unwrap();
        let read=b.state.borrow().comment_reads+1;
        b.state.borrow_mut().after_comment_read=Some((n,read,vec![entry("answer",&["2"],None,"other"),entry("ask",&[],Some(a),"Problem: New question\n1. different meaning\n2. new other")]));
        let r=answer_picks(&b,&cfg,&[format!("{n}.1")],&Op{id:Some("read-race".into()),revision:None}).unwrap().remove(0);
        assert_eq!(r.note,Some(true));
        let log=entries(&b,n).unwrap();
        let p=core::project(n,&log,false);
        assert_eq!(p.task.state,State::Ask);
        assert!(p.task.card.unwrap().contains("New question"));
        assert_eq!(core::delivery(n,&log,*p.notes.last().unwrap()),"original meaning");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_note_preserves_steers_across_question_races() {
        for during_write in [false,true] {
            let b=Fake::default();
            let root=std::env::temp_dir().join(format!("taskq-note-steer-{}",crate::runtime::uuid()));
            let n=b.create("T","## Goal\ng",&["taskq".into()]).unwrap();
            let a=b.comment(n,"taskq: spawn worker codex:s win").unwrap();
            let ask=entry("ask",&[],Some(a),"Problem: Still need a decision");
            if during_write {b.state.borrow_mut().race=Some(ask);}else{b.comment(n,&ask).unwrap();}
            let op=Op{id:Some("steer-race".into()),revision:Some(a)};
            let r=act(&b,&root,n,"steer",&[],"Keep acceptance in scope",&op);
            assert!(r.ok(),"{}",r.text());
            let log=entries(&b,n).unwrap();let p=core::project(n,&log,false);
            assert_eq!(p.task.state,State::Ask);
            assert_eq!(core::delivery(n,&log,*p.notes.last().unwrap()),"Keep acceptance in scope");
            let count=log.len();assert!(act(&b,&root,n,"steer",&[],"Keep acceptance in scope",&op).ok());
            assert_eq!(entries(&b,n).unwrap().len(),count);
            assert!(!act_as(&b,&root,n,"steer",&[],"wrong worker",&Op::default(),Some(a+99),None).ok());
        }
    }

}
