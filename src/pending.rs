//! The tray's shared pending operations (#1819, docs/design/menu.md#shared-pending-operations-1819): one native store
//! of the owner's in-flight writes. #1918: the tray applies it to the projects it reads before any surface is built, so
//! the list, dots, counts, Messages, icon, notifications and Conversations read one snapshot; the board stays the truth
//! (R1, R12) and only an op-id readback changes a row.

use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// A confirmed operation still holds this long, so a lagging board read never shows the old state again.
const CONFIRMED: Duration = Duration::from_secs(60);
/// A refused or stale write shows the real state this long.
const MISMATCH: Duration = Duration::from_secs(30);
/// A failed operation, or one GitHub never shows, is dropped after this.
const KEEP: Duration = Duration::from_secs(600);

struct Op {
    root: String,
    id: String,
    task: u64,
    /// The request as submitted: Retry resends it unchanged, same op id.
    request: Value,
    phase: &'static str,
    reason: String,
    /// The question the owner answered and its revision, as the row showed them when sent.
    asked: Value,
    revision: u64,
    /// The readback of an accepted or refused write: state, § 2 revision and the answer that closed the question.
    state: Option<String>,
    read: Option<u64>,
    answered: Value,
    /// When a snapshot row first carried the op id (or the task left the snapshot).
    seen: Option<Instant>,
    at: Instant,
    span: tracing::Span,
}

#[derive(Default)]
pub struct Pending {
    ops: Vec<Op>,
}

/// Whether snapshot `row` was read after write `id`: its log lists the op id (a cancellation's close step `<id>.close` too).
fn carries(row: &Value, id: &str) -> bool {
    row["ops"].as_array().into_iter().flatten().filter_map(Value::as_str).any(|o| o == id || o.strip_prefix(id).is_some_and(|r| r.starts_with('.')))
}

fn row<'a>(projects: &'a [Value], root: &str, task: u64) -> Option<&'a Value> {
    projects.iter().filter(|p| p["root"] == root)
        .flat_map(|p| p["tasks"].as_array().into_iter().flatten()).find(|r| r["n"] == task)
}

impl Pending {
    /// `request` (`{root, op: {id, task, revision, action?, text?, choice?}}`) was handed to the queue; `projects` is the
    /// last tray snapshot. The row keeps the board's state and shows Sending; nothing is expected ahead of the board.
    pub fn begin(&mut self, request: &Value, projects: &Value) {
        let (root, id) = (request["root"].as_str().unwrap_or("").to_string(), request["op"]["id"].as_str().unwrap_or("").to_string());
        let task = request["op"]["task"].as_u64().unwrap_or(0);
        let asked = row(projects.as_array().map_or(&[][..], |v| v), &root, task).map_or(Value::Null, |r| r["question"].clone());
        self.ops.retain(|o| !(o.root == root && o.id == id));
        let span = coreq::telemetry::capture::detached_span("tray.sync");
        self.ops.push(Op { root, id, task, request: request.clone(), phase: "sending", reason: String::new(), asked,
            revision: request["op"]["revision"].as_u64().unwrap_or(0), state: None, read: None, answered: Value::Null, seen: None,
            at: Instant::now(), span });
    }

    /// The request a Retry resends: only a failed operation's.
    pub fn retry(&self, root: &str, id: &str) -> Option<Value> {
        self.ops.iter().find(|o| o.root == root && o.id == id && o.phase == "failed").map(|o| o.request.clone())
    }

    /// The queue's reply: `{answerReceipt: {root, id, state, reason, answered?}, sync?: {state, revision}}`. An accepted
    /// receipt is the op-id readback (dispatch.rs): its state and revision are the board's, whatever their order.
    pub fn receipt(&mut self, reply: &Value) -> bool {
        let r = &reply["answerReceipt"];
        let Some(op) = self.ops.iter_mut().find(|o| r["root"] == o.root.as_str() && r["id"] == o.id.as_str()) else { return false };
        op.state = reply["sync"]["state"].as_str().filter(|s| !s.is_empty()).map(String::from);
        op.read = reply["sync"]["revision"].as_u64();
        op.at = Instant::now();
        match r["state"].as_str() {
            Some("accepted") => {
                op.phase = "accepted";
                op.reason.clear();
                // the owner's own answer closes the question: its text, or the label of the choice it picked
                if op.request["op"].get("action").is_none() {
                    let text = op.request["op"]["text"].as_str().map(String::from).or_else(|| {
                        let k = op.request["op"]["choice"].as_u64()?;
                        op.asked["choices"].as_array()?.iter().find(|c| c["id"] == k).and_then(|c| c["text"].as_str()).map(String::from)
                    });
                    if let Some(text) = text { op.answered = json!({"by": "you", "text": text}); }
                }
            }
            // the board refused it as it stands: show what it holds, never ask again
            Some("stale") => op.settle("mismatch", r["answered"]["by"].as_str().map_or("The task changed first".into(), |by| format!("Answered first by {by}")), &r["answered"]),
            Some("rejected") if r["answered"].is_object() => op.settle("mismatch", format!("Answered first by {}", r["answered"]["by"].as_str().unwrap_or("someone")), &r["answered"]),
            _ => {
                op.phase = "failed";
                op.reason = r["reason"].as_str().filter(|s| !s.is_empty()).unwrap_or("Not confirmed").into();
                op.span.record("coreq_result", "observed");
                op.span.record("coreq_error", "failed");
                op.span = tracing::Span::none();
            }
        }
        true
    }

    /// The board snapshot `projects` was read (before `apply`): an accepted operation is confirmed once its row carries
    /// the op id, or the task left the snapshot; a revision compare never settles it (§ 2 revision drops after an
    /// answer). Expired operations are dropped. True when anything changed.
    pub fn observe(&mut self, projects: &[Value], now: Instant) -> bool {
        let mut changed = false;
        for op in self.ops.iter_mut().filter(|o| o.phase == "accepted" && o.seen.is_none()) {
            if !projects.iter().any(|p| p["root"] == op.root.as_str()) { continue; }
            if row(projects, &op.root, op.task).is_none_or(|r| carries(r, &op.id)) {
                op.seen = Some(now);
                op.span.record("coreq_result", "observed");
                // the span closes here: its duration is the sync latency
                op.span = tracing::Span::none();
                changed = true;
            }
        }
        let before = self.ops.len();
        self.ops.retain(|o| match (o.phase, o.seen) {
            (_, Some(seen)) => now.saturating_duration_since(seen) < CONFIRMED,
            ("mismatch", _) => now.saturating_duration_since(o.at) < MISMATCH,
            _ => now.saturating_duration_since(o.at) < KEEP,
        });
        changed || self.ops.len() != before
    }

    /// The one channel (#1918): every operation onto the tray's `projects` before counts, Messages, icon, notifications
    /// or any window read them. A row the write predates (no op id) takes the readback; a row read after it is the
    /// board's alone. Sending, failed and refused operations add their `sync` line to the row.
    pub fn apply(&self, projects: &mut [Value]) {
        for op in &self.ops {
            let Some(tasks) = projects.iter_mut().find(|p| p["root"] == op.root.as_str()).and_then(|p| p["tasks"].as_array_mut()) else { continue };
            let Some(i) = tasks.iter().position(|r| r["n"] == op.task) else { continue };
            let predates = !carries(&tasks[i], &op.id);
            let readback = match op.phase {
                "accepted" => predates,
                // a refused write corrects only the snapshot of the question the owner answered
                "mismatch" => predates && tasks[i]["revision"].as_u64() == Some(op.revision),
                _ => false,
            };
            if readback && op.state.as_deref() == Some("closed") { tasks.remove(i); continue; }
            let r = &mut tasks[i];
            if readback && let (Some(state), Some(read)) = (&op.state, op.read) {
                if r["state"] == "ask" && state != "ask" {
                    let asked = if r["question"].is_null() { op.asked.clone() } else { r["question"].take() };
                    r["asked"] = asked;
                    r["card"] = Value::Null;
                    r["question"] = Value::Null;
                }
                if op.answered.is_object() && state != "ask" { r["answered"] = op.answered.clone(); }
                r["state"] = json!(state);
                r["revision"] = json!(read);
                r["controls"] = json!(crate::snapshot::controls(state));
            }
            if op.phase != "accepted" {
                r["sync"] = json!({"root": op.root, "id": op.id, "task": op.task, "action": op.request["op"]["action"], "phase": op.phase, "reason": op.reason});
            }
        }
    }

    /// The store as test evidence: one entry per operation, without ages.
    #[cfg(test)]
    pub fn json(&self) -> Value {
        json!(self.ops.iter().map(|o| json!({"root": o.root, "id": o.id, "task": o.task, "phase": o.phase, "reason": o.reason,
            "state": o.state, "confirmed": o.seen.is_some()})).collect::<Vec<_>>())
    }
}

impl Op {
    /// Ends a refused operation's latency span: `mismatch` read back the board and names why.
    fn settle(&mut self, phase: &'static str, reason: String, answered: &Value) {
        self.phase = phase;
        self.reason = reason;
        if answered.is_object() { self.answered = json!({"by": answered["by"], "text": answered["text"]}); }
        self.span.record("coreq_result", "observed");
        self.span.record("coreq_error", "mismatch");
        self.span = tracing::Span::none();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUESTION: &str = "Problem: Pick one\n1. Ship ★\n2. Wait";

    /// One project whose task 7 reads `state` at § 2 `revision`, its log carrying `ops`.
    fn projects(state: &str, revision: u64, ops: &[&str]) -> Value {
        let mut t = json!({"n": 7, "state": state, "revision": revision, "ops": ops});
        if state == "ask" { t["card"] = json!(QUESTION); t["question"] = crate::snapshot::question(QUESTION); }
        json!([{"root": "/p", "tasks": [t]}])
    }
    fn request(id: &str, action: Option<&str>) -> Value {
        let mut m = json!({"root": "/p", "op": {"id": id, "task": 7, "revision": 30, "choice": 1}});
        if let Some(a) = action { m["op"]["action"] = json!(a); m["op"].as_object_mut().unwrap().remove("choice"); }
        m
    }
    fn shown(p: &Pending, projects: &Value) -> Value {
        let mut ps = projects.as_array().unwrap().clone();
        p.apply(&mut ps);
        ps[0]["tasks"].get(0).cloned().unwrap_or(Value::Null)
    }

    // Chain: owner answer -> op-id readback -> one tray snapshot. Risk (#1918): § 2 revision drops after an answer
    // (ask entry 30 -> spawn 25), so a revision compare settles on the pre-answer status.json and every surface
    // shows the question open again.
    #[test]
    fn an_accepted_answer_holds_until_a_row_carries_its_op_id_though_the_revision_drops() {
        let mut p = Pending::default();
        let before = projects("ask", 30, &["ask-op"]);
        p.begin(&request("tray.1", None), &before);
        let row = shown(&p, &before);
        assert_eq!((row["state"].as_str(), row["sync"]["phase"].as_str()), (Some("ask"), Some("sending")), "no expected state ahead of the board");
        assert!(p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.1", "state": "accepted"}, "sync": {"state": "doing", "revision": 25}})));
        let now = Instant::now();
        assert!(!p.observe(before.as_array().unwrap(), now), "a stale snapshot never settles it, though its revision 30 >= 25");
        let row = shown(&p, &before);
        assert_eq!((row["state"].as_str(), row["revision"].as_u64()), (Some("doing"), Some(25)));
        assert_eq!((row["card"].clone(), row["question"].clone()), (Value::Null, Value::Null), "the question is closed on every surface");
        assert_eq!(row["asked"]["text"], QUESTION, "the conversation keeps the question");
        assert_eq!(row["answered"], json!({"by": "you", "text": "Ship"}));
        assert!(row.get("sync").is_none(), "accepted is the board's readback, not a pending mark");
        assert_eq!(row["controls"], json!(crate::snapshot::controls("doing")));

        let after = projects("doing", 25, &["tray.1", "ask-op"]);
        assert!(p.observe(after.as_array().unwrap(), now), "the op id confirms it");
        assert_eq!(shown(&p, &after), after[0]["tasks"][0], "a row read after the write is the board's alone");
        assert_eq!(shown(&p, &before)["state"], "doing", "a lagging read after confirmation still shows the readback");
        assert!(p.observe(before.as_array().unwrap(), now + CONFIRMED));
        assert_eq!(p.json(), json!([]));
    }

    // Chain: control while offline -> failed -> one Retry. Risk (#1819): an outbox replays it later, or Retry
    // sends a new operation id.
    #[test]
    fn an_offline_write_fails_with_one_retry_of_the_same_operation() {
        let mut p = Pending::default();
        let m = request("tray.start.7.30", Some("start"));
        let held = projects("held", 30, &[]);
        p.begin(&m, &held);
        assert_eq!(p.retry("/p", "tray.start.7.30"), None, "only a failed operation retries");
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.start.7.30", "state": "unknown", "reason": "service unreachable"}}));
        let row = shown(&p, &held);
        assert_eq!((row["state"].as_str(), row["sync"]["phase"].as_str(), row["sync"]["reason"].as_str()), (Some("held"), Some("failed"), Some("service unreachable")));
        assert_eq!(p.retry("/p", "tray.start.7.30"), Some(m.clone()));
        p.begin(&m, &held);
        assert_eq!((p.json().as_array().unwrap().len(), p.json()[0]["phase"].as_str()), (1, Some("sending")));
    }

    // Chain: two actors answer the same question -> stale receipt. Risk (#1819): the tray asks again or shows the
    // owner's answer as the one that won.
    #[test]
    fn a_conflict_shows_the_real_state_and_a_reason() {
        let mut p = Pending::default();
        let before = projects("ask", 30, &[]);
        p.begin(&request("tray.2", None), &before);
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.2", "state": "stale", "answered": {"by": "alex", "text": "Wait"}}, "sync": {"state": "doing", "revision": 26}}));
        let row = shown(&p, &before);
        assert_eq!((row["state"].as_str(), row["sync"]["phase"].as_str(), row["sync"]["reason"].as_str()), (Some("doing"), Some("mismatch"), Some("Answered first by alex")));
        assert_eq!(row["answered"], json!({"by": "alex", "text": "Wait"}));
        let moved = projects("review", 40, &[]);
        assert_eq!(shown(&p, &moved)["state"], "review", "a newer snapshot is the board's");
    }

    // Chain: owner cancels -> later then close readback -> the task leaves the open snapshot. Risk: a stale row
    // keeps the cancelled task on the list until the next pass.
    #[test]
    fn a_closed_readback_removes_the_stale_row() {
        let mut p = Pending::default();
        let before = projects("doing", 25, &[]);
        p.begin(&request("tray.close.7.25", Some("close")), &before);
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.close.7.25", "state": "accepted"}, "sync": {"state": "closed", "revision": 41}}));
        assert_eq!(shown(&p, &before), Value::Null);
        assert!(p.observe(projects("doing", 25, &["tray.close.7.25.close"]).as_array().unwrap(), Instant::now()), "the close step's op id confirms the cancellation");
    }
}
