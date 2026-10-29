//! The tray's shared pending operations (#1819, docs/design/menu.md#shared-pending-operations-1819): one native store
//! of the owner's in-flight writes. Every window renders it over its snapshot; the board stays the truth (R1, R12).

use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// A done operation stays visible this long, then the snapshot alone shows the task.
const DONE: Duration = Duration::from_secs(5);
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
    expected: String,
    phase: &'static str,
    reason: String,
    revision: u64,
    at: Instant,
    span: tracing::Span,
}

#[derive(Default)]
pub struct Pending {
    ops: Vec<Op>,
}

/// The state task `row` reads after `action` lands (§ 2); an answer resumes the run when one is live.
pub fn expected(action: Option<&str>, row: &Value) -> String {
    let before = row["state"].as_str().unwrap_or("");
    let live = row["executions"].as_array().is_some_and(|e| !e.is_empty());
    match action {
        Some("stop") => "failed",
        Some("later" | "close") => "later",
        Some("start" | "retry" | "requeue") => "ready",
        Some(_) => before,
        None if live => "doing",
        None => "ready",
    }.to_string()
}

impl Pending {
    /// `request` (`{root, op: {id, task, action?}}`) was handed to the queue; `projects` is the last tray snapshot.
    pub fn begin(&mut self, request: &Value, projects: &Value) {
        let (root, id) = (request["root"].as_str().unwrap_or("").to_string(), request["op"]["id"].as_str().unwrap_or("").to_string());
        let task = request["op"]["task"].as_u64().unwrap_or(0);
        let row = projects.as_array().into_iter().flatten().filter(|p| p["root"] == root.as_str())
            .flat_map(|p| p["tasks"].as_array().into_iter().flatten()).find(|r| r["n"] == task).cloned().unwrap_or(Value::Null);
        self.ops.retain(|o| !(o.root == root && o.id == id));
        let span = coreq::telemetry::capture::detached_span("tray.sync");
        self.ops.push(Op { root, id, task, request: request.clone(), expected: expected(request["op"]["action"].as_str(), &row),
            phase: "sending", reason: String::new(), revision: 0, at: Instant::now(), span });
    }

    /// The request a Retry resends: only a failed operation's.
    pub fn retry(&self, root: &str, id: &str) -> Option<Value> {
        self.ops.iter().find(|o| o.root == root && o.id == id && o.phase == "failed").map(|o| o.request.clone())
    }

    /// The queue's reply: `{answerReceipt: {root, id, state, reason, answered?}, sync?: {state, revision}}`.
    pub fn receipt(&mut self, reply: &Value) -> bool {
        let r = &reply["answerReceipt"];
        let Some(op) = self.ops.iter_mut().find(|o| r["root"] == o.root.as_str() && r["id"] == o.id.as_str()) else { return false };
        let state = reply["sync"]["state"].as_str().filter(|s| !s.is_empty());
        op.at = Instant::now();
        match r["state"].as_str() {
            Some("accepted") => {
                op.phase = "waiting";
                op.revision = reply["sync"]["revision"].as_u64().unwrap_or(0);
                if let Some(s) = state.filter(|s| *s != op.expected) {
                    op.reason = format!("The board shows {s}");
                    op.expected = s.into();
                }
            }
            // the board refused it as it stands: show what it holds, never ask again
            Some("stale") => op.settle("mismatch", state, r["answered"]["by"].as_str().map_or("The task changed first".into(), |by| format!("Answered first by {by}"))),
            Some("rejected") if r["answered"].is_object() => op.settle("mismatch", state, format!("Answered first by {}", r["answered"]["by"].as_str().unwrap_or("someone"))),
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

    /// The tray snapshot `projects` was read: a waiting operation is done once its row carries the write's revision
    /// (or the task left the snapshot); expired ones are dropped. True when anything changed.
    pub fn observe(&mut self, projects: &Value, now: Instant) -> bool {
        let rows = |root: &str| projects.as_array().into_iter().flatten().find(|p| p["root"] == root).map(|p| p["tasks"].clone());
        let mut changed = false;
        for op in self.ops.iter_mut().filter(|o| o.phase == "waiting") {
            let Some(tasks) = rows(&op.root) else { continue };
            let row = tasks.as_array().into_iter().flatten().find(|r| r["n"] == op.task);
            if row.is_none_or(|r| r["revision"].as_u64().unwrap_or(0) >= op.revision.max(1)) {
                op.settle("done", row.and_then(|r| r["state"].as_str()), String::new());
                changed = true;
            }
        }
        let before = self.ops.len();
        self.ops.retain(|o| now.duration_since(o.at) < match o.phase { "done" => DONE, "mismatch" => MISMATCH, _ => KEEP });
        changed || self.ops.len() != before
    }

    /// What every window renders: one entry per operation, without ages, so an unchanged store keeps the tick signature.
    pub fn json(&self) -> Value {
        json!(self.ops.iter().map(|o| json!({"root": o.root, "id": o.id, "task": o.task, "action": o.request["op"]["action"],
            "expected": o.expected, "phase": o.phase, "reason": o.reason})).collect::<Vec<_>>())
    }
}

impl Op {
    /// Ends the operation's latency span: `done` and `mismatch` read back the board, `mismatch` names why.
    fn settle(&mut self, phase: &'static str, state: Option<&str>, reason: String) {
        self.phase = phase;
        if let Some(s) = state { self.expected = s.into(); }
        if !reason.is_empty() || phase == "mismatch" { self.reason = reason; }
        self.at = Instant::now();
        self.span.record("coreq_result", "observed");
        if phase == "mismatch" { self.span.record("coreq_error", "mismatch"); }
        // the span closes here: its duration is the sync latency
        self.span = tracing::Span::none();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects(state: &str, revision: u64) -> Value {
        json!([{"root": "/p", "tasks": [{"n": 7, "state": state, "revision": revision}]}])
    }
    fn request(id: &str, action: Option<&str>) -> Value {
        let mut m = json!({"root": "/p", "op": {"id": id, "task": 7, "revision": 20}});
        if let Some(a) = action { m["op"]["action"] = json!(a); }
        m
    }

    // Chain: owner answer -> native store -> every window -> board readback. Risk (#1819): the expected state waits
    // for the 2 s tick, or a window shows a status the board never confirmed as final.
    #[test]
    fn an_answer_shows_its_expected_state_at_once_until_the_board_confirms() {
        let mut p = Pending::default();
        p.begin(&request("tray.1", None), &projects("ask", 20));
        let shown = p.json();
        assert_eq!((shown[0]["expected"].as_str(), shown[0]["phase"].as_str()), (Some("ready"), Some("sending")));
        let reply = json!({"answerReceipt": {"root": "/p", "id": "tray.1", "state": "accepted"}, "sync": {"state": "ready", "revision": 21}});
        assert!(p.receipt(&reply));
        assert_eq!(p.json()[0]["phase"], "waiting", "accepted is not yet on the board snapshot");
        let now = Instant::now();
        assert!(!p.observe(&projects("ask", 20), now), "an older snapshot keeps it waiting");
        assert!(p.observe(&projects("ready", 21), now));
        assert_eq!((p.json()[0]["phase"].as_str(), p.json()[0]["expected"].as_str()), (Some("done"), Some("ready")));
        assert!(p.observe(&projects("ready", 21), Instant::now() + DONE), "done is dropped after 5 s");
        assert_eq!(p.json(), json!([]));
    }

    // Chain: control while offline -> failed -> one Retry. Risk (#1819): an outbox replays it later, or Retry
    // sends a new operation id.
    #[test]
    fn an_offline_write_fails_with_one_retry_of_the_same_operation() {
        let mut p = Pending::default();
        let m = request("tray.start.7.20", Some("start"));
        p.begin(&m, &projects("held", 20));
        assert_eq!(p.json()[0]["expected"], "ready");
        assert_eq!(p.retry("/p", "tray.start.7.20"), None, "only a failed operation retries");
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.start.7.20", "state": "unknown", "reason": "service unreachable"}}));
        assert_eq!((p.json()[0]["phase"].as_str(), p.json()[0]["reason"].as_str()), (Some("failed"), Some("service unreachable")));
        assert_eq!(p.retry("/p", "tray.start.7.20"), Some(m.clone()));
        p.begin(&m, &projects("held", 20));
        assert_eq!((p.json().as_array().unwrap().len(), p.json()[0]["phase"].as_str()), (1, Some("sending")));
    }

    // Chain: two actors answer the same question -> stale receipt. Risk (#1819): the tray asks again or keeps
    // showing the expected state.
    #[test]
    fn a_conflict_shows_the_real_state_and_a_reason() {
        let mut p = Pending::default();
        p.begin(&request("tray.2", None), &json!([{"root": "/p", "tasks": [{"n": 7, "state": "ask", "executions": [{"id": 1}]}]}]));
        assert_eq!(p.json()[0]["expected"], "doing", "an answer resumes a live run");
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.2", "state": "stale", "answered": {"by": "alex", "text": "x"}}, "sync": {"state": "doing", "revision": 22}}));
        let shown = &p.json()[0];
        assert_eq!((shown["phase"].as_str(), shown["expected"].as_str(), shown["reason"].as_str()), (Some("mismatch"), Some("doing"), Some("Answered first by alex")));
        p.begin(&request("tray.3", Some("stop")), &projects("doing", 22));
        p.receipt(&json!({"answerReceipt": {"root": "/p", "id": "tray.3", "state": "accepted"}, "sync": {"state": "review", "revision": 23}}));
        assert_eq!((p.json()[1]["expected"].as_str(), p.json()[1]["reason"].as_str()), (Some("review"), Some("The board shows review")));
    }
}
