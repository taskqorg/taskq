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
                    // the closed question joins the conversation history with its answer, after the earlier ones
                    let question = if r["question"].is_null() { op.asked.clone() } else { r["question"].take() };
                    let answered = if op.answered.is_object() { op.answered.clone() } else { json!({"by": "", "text": ""}) };
                    if !r["history"].is_array() { r["history"] = json!([]); }
                    r["history"].as_array_mut().unwrap().push(json!({"question": question, "answered": answered}));
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
#[path = "pending_tests.rs"]
mod tests;
