//! The operation dispatcher (taskq.md § 5): every task write goes through `act`, which checks the expected revision and
//! the op id, appends one entry that `fold` accepts, reads it back and returns a receipt.

use crate::board::{self, Board, Publish, entries};
use crate::core::{self, Entry, Gate, State, Task, entry, fold, options};
use crate::service::{self, Config};
use crate::{Res, join, poke};
use serde::Serialize;
use serde_json::{Value, json};
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
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Answered {
    pub by: String,
    pub text: String,
    /// § 2 owner note detail for Conversations: board time (Unix seconds), the `eyes` read receipt (None: the board
    /// does not say) and attached files; receipts leave them out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<Value>,
    /// Immutable board event/question identity for chronology and exact jumps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
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
    Some(Answered { by: comments.iter().find(|c| c.id == id).map(|c| c.author.clone()).unwrap_or_default(), text: core::delivery(n, log, id), ..Default::default() })
}

/// #1918 conversation history: every accepted answer of `log` in board order, with the card it closed; late answers
/// saved as notes closed no card and stay out. #2188: every other owner note (§ 2) joins as a message without a card,
/// each with its board time, files and `eyes` read receipt.
pub fn history(n: u64, log: &[Entry], p: &core::Projection, comments: &[board::Comment]) -> Vec<(Option<String>, Answered)> {
    let answers: Vec<u64> = p.answers.iter().filter(|a| !a.late).map(|a| a.id).collect();
    core::owner_notes(n, log, p).into_iter().chain(answers.iter().copied()).collect::<std::collections::BTreeSet<_>>().into_iter().map(|id| {
        let c = comments.iter().find(|c| c.id == id);
        let text = core::delivery(n, log, id);
        let files = crate::reply_files::manifest(&text).unwrap_or_default().into_iter().map(|f| json!({"name": f.name, "mime": f.mime, "url": f.url})).collect::<Vec<_>>();
        // the file links and manifest the reply appended (§ 5) show as the files list, not as text
        let shown = match text.rfind(crate::reply_files::MARKER) {
            Some(at) if !files.is_empty() => files.iter().fold(text[..at].trim_end(), |t, _| t.rsplit_once('\n').map_or("", |x| x.0)).trim_end().to_string(),
            _ => text,
        };
        let question = answers.contains(&id).then(|| core::card_before(n, log, id)).flatten();
        let revision=p.answers.iter().find(|a|a.id==id).map(|a|a.revision);
        let asked=revision.and_then(|r|comments.iter().find(|c|c.id==r));
        let worker=revision.and_then(|r|p.changes.iter().find(|(id,_,_)|*id==r).map(|(_,_,t)|t));
        (question, Answered { by: c.map(|c| c.author.clone()).unwrap_or_default(), text: shown, at: c.map(|c| c.created).filter(|t| *t > 0), read: c.and_then(|c| c.read), files,
            source:Some(json!({"id":id,"question":revision,"asked_at":asked.map(|c|c.created),"attempt":worker.and_then(|t|t.attempt),"session":worker.and_then(|t|t.session.as_deref()),"role":worker.and_then(|t|t.role.as_deref())})) })
    }).collect()
}

/// F16: accepted board records share the reader with owner notes; ignored/edited entries never become history truth.
pub fn timeline(n:u64,log:&[Entry],p:&core::Projection,comments:&[board::Comment])->Vec<(Option<String>,Answered)> {
    let mut rows=history(n,log,p,comments);
    let seen:std::collections::BTreeSet<u64>=rows.iter().flat_map(|(_,a)|[a.source.as_ref().and_then(|s|s["id"].as_u64()),a.source.as_ref().and_then(|s|s["question"].as_u64())]).flatten().collect();
    for e in log.iter().filter(|e|p.accepts(e.id)&&!seen.contains(&e.id)) {
        let kind=core::kind(n,&e.body);
        if kind=="ask"&&p.task.state==State::Ask&&p.revision==e.id {continue;}
        let source=p.changes.iter().chain(&p.queue).find(|(id,_,_)|*id==e.id).map(|(_,before,after)|if kind=="spawn"{after}else{before});
        let c=comments.iter().find(|c|c.id==e.id);
        rows.push(((kind=="ask").then(||core::entry_text(n,&e.body).to_string()),Answered {
            by:c.map(|c|c.author.clone()).unwrap_or_default(),text:if kind=="ask"{String::new()}else{e.body.clone()},at:c.map(|c|c.created).filter(|at|*at>0),
            source:Some(json!({"id":e.id,"kind":kind,"question":(kind=="ask").then_some(e.id),"asked_at":c.map(|c|c.created),"attempt":source.and_then(|t|t.attempt),"session":source.and_then(|t|t.session.as_deref()),"role":source.and_then(|t|t.role.as_deref())})),..Default::default()
        }));
    }
    rows.sort_by_key(|(_,a)|a.source.as_ref().and_then(|s|s["id"].as_u64()).unwrap_or(0));rows
}

/// § 2 read receipts: after the live attempt `attempt` has read the task, react `eyes` to each owner note lacking it.
/// Returns the notes it marked; a refused reaction stays unread and is retried by the next read.
pub fn mark_read(b: &dyn Board, n: u64, log: &[Entry], comments: &[board::Comment], attempt: u64) -> Vec<u64> {
    let p = core::project(n, log, false);
    if p.task.attempt != Some(attempt) { return vec![]; }
    core::owner_notes(n, log, &p).into_iter().filter(|id| comments.iter().any(|c| c.id == *id && c.read == Some(false)))
        .filter(|id| b.react_read(n, *id).is_ok()).collect()
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
        let answered = Answered { by, text: core::delivery(n, log, id), ..Default::default() };
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
    if kind == "result" {
        if let Ok((cfg, _)) = crate::load(root) {
            let att = attempt.or(before_write.task.attempt).unwrap_or(0);
            let session = before_write.task.session.as_deref().unwrap_or_default();
            let rt = session.split(':').next().filter(|s| !s.is_empty()).unwrap_or("other");
            let host = session.split(' ').nth(1).filter(|s| !s.is_empty()).unwrap_or(&cfg.me);
            let pid = crate::telemetry::live_worker_pid(&cfg.install, Some(root), n, att, att).unwrap_or_else(std::process::id);
            crate::telemetry::record_worker(&cfg.install, &cfg.repo, host, rt, "work", "result", n, att, pid, "ok", args.first().copied().unwrap_or("handed-in"));
        }
    }
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
    if r.ok() { review_result(cfg, n, seen.0, &seen.1, &child_env()); }
    r
}

/// #2283: a reviewer's close is its hand-in; the service then kills the closing review as replaced, so the
/// `result` lifecycle event is recorded here or never.
fn review_result(cfg: &Config, n: u64, attempt: u64, rt: &str, env: &ChildEnv) {
    if attempt == 0 || own_attempt(n, env) != Some(attempt) { return; }
    let pid = crate::telemetry::live_worker_pid(&cfg.install, Some(&cfg.root), n, attempt, attempt).unwrap_or_else(std::process::id);
    crate::telemetry::record_worker(&cfg.install, &cfg.repo, &cfg.me, rt, "review", "result", n, attempt, pid, "ok", "close");
}

/// `seen`: the attempt and runtime once read, and an outcome override (a pending publication is retried).
fn close_body(b: &dyn Board, cfg: &Config, n: u64, verdict: &str, op: &Op, check: &dyn Fn(&board::Issue, &Task) -> Res<()>, seen: &mut (u64, String, Option<(&'static str, &'static str)>)) -> Receipt {
    let read=||->Res<_> {let issue=b.issue(n)?;let log=entries(b,n)?;Ok((issue,log))};
    let (issue,log)=match read() {Ok(x)=>x,Err(e)=>return Receipt::rejected(n,op,e)};
    let t=fold(n,&log,deps_open(b,n,&issue.body,&log));
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
                        let evidence=crate::readiness::receipt(n,&b.url(n),&log,b.take_publication_evidence());
                        let _ = act_as(b, &cfg.root, n, "enqueued", &[&id], &evidence, &next, None, None);
                    }
                }
                return r;
            }
            let began = std::time::Instant::now();
            let published = b.publish(n, &sha, &cfg.publish, &issue.title, &format!("Task: {}", b.url(n)));
            let evidence=crate::readiness::receipt(n,&b.url(n),&log,b.take_publication_evidence());
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
                    let mut r = act(b, &cfg.root, n, "close", &[], &join(&[verdict, &m, &evidence]), op);
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
    let published=b.resume_publication(n, t.result.as_deref().unwrap_or_default(), &cfg.publish, &issue.title, &format!("Task: {}", b.url(n)), &cfg.root, t.enqueued.is_some());
    let evidence=crate::readiness::receipt(n,&b.url(n),log,b.take_publication_evidence());
    match published {
        Publish::Done(m) => write("close", &[], &join(&[&repeats(n, log, verdict), &format!("delivery {} r{}: {m}", cfg.delivery, cfg.delivery_revision), &evidence])),
        Publish::Enqueued(id) if t.enqueued.is_none() => write("enqueued", &[&id], &evidence),
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
#[path = "dispatch_tests.rs"]
mod tests;
