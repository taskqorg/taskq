//! Held-draft body amendment (R1/R3/R4/R12, §1).
use crate::{
    Res,
    board::{self, Board},
    core::{self, State},
    dispatch::{self, Op, Receipt},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Intent {
    board: String,
    revision: u64,
    deps: Vec<String>,
    before: String,
    after: String,
}

fn replacement(body: &str, deps: &[String]) -> String {
    let mut out = String::new();
    let mut inserted = false;
    let lines = deps
        .iter()
        .map(|d| format!("Depends on {d}\n"))
        .collect::<String>();
    for line in body.split_inclusive('\n') {
        if line.trim_start().starts_with("Depends on ") {
            if !inserted {
                out.push_str(&lines);
                inserted = true;
            }
        } else {
            out.push_str(line);
        }
    }
    if !inserted && !lines.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&lines);
    }
    out
}

fn held(b: &dyn Board, n: u64) -> Res<board::Issue> {
    let issue = b.issue(n)?;
    let log = board::entries(b, n)?;
    let p = core::project(n, &log, false);
    if !issue.trusted
        || !issue.open
        || p.task.state != State::Held
        || !crate::service::is_held(&issue)
        || issue.labels.iter().any(|l| l == "taskq")
        || log
            .iter()
            .any(|e| p.accepts(e.id) && core::kind(n, &e.body) == "start")
    {
        return Err("dependency amendment requires a trusted open never-started held draft".into());
    }
    Ok(issue)
}

/// All authorization and request validation precede board mutation; a retry binds the original payload.
#[allow(clippy::too_many_arguments)]
pub fn amend(
    b: &dyn Board,
    root: &Path,
    n: u64,
    board_url: &str,
    list: &str,
    op: &Op,
    authorized: bool,
) -> Receipt {
    let writing = std::cell::Cell::new(false);
    let run = || -> Res<Receipt> {
        if !authorized {
            return Err("dependency amendment requires an owner/PM local CLI; child and service callers refused".into());
        }
        let url = b.url(0);
        if board_url != url.trim_end_matches("/0") {
            return Err("--board must exactly match the configured board URL".into());
        }
        let id = op
            .id
            .as_deref()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && s.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            })
            .ok_or("--op-id requires 1-128 ASCII letters, digits, dot, underscore or hyphen")?;
        let revision = op.revision.ok_or("--revision is required")?;
        let mut deps = vec![];
        if !list.is_empty() {
            for item in list.split(',') {
                let item = item.trim();
                let parsed = core::depends(&format!("Depends on {item}"));
                if parsed.len() != 1
                    || parsed[0].n == 0
                    || parsed[0].name() != item
                    || parsed[0].repo.is_none() && parsed[0].n == n
                {
                    return Err("--deps requires comma-separated #N or repo#N references; no self dependency".into());
                }
                if !deps.iter().any(|v| v == item) {
                    deps.push(item.to_string());
                }
            }
        }
        let log = board::entries(b, n)?;
        let p = core::project(n, &log, false);
        if let Some(e) = log
            .iter()
            .find(|e| !e.edited && core::op_id(n, &e.body) == Some(id))
        {
            let original = e
                .body
                .lines()
                .nth(1)
                .and_then(|s| serde_json::from_str::<Intent>(s).ok())
                .ok_or("op-id already used by another operation")?;
            if core::kind(n, &e.body) != "deps"
                || original.board != board_url
                || original.revision != revision
                || original.deps != deps
            {
                return Err("op-id already used with a different request".into());
            }
            if !p.accepts(e.id) {
                return Err("dependency intent lost its revision fence; no body changed".into());
            }
            if let Some(done) = log.iter().find(|done| {
                p.accepts(done.id) && done.body == format!("taskq: deps-done {}", e.id)
            }) {
                return Ok(Receipt::accepted(n, op, State::Held, done.id));
            }
        } else {
            let issue = held(b, n)?;
            if p.revision != revision {
                return Ok(Receipt {
                    outcome: "stale",
                    revision: Some(p.revision),
                    ..Receipt::rejected(n, op, "revision is stale; read inbox before retrying")
                });
            }
            if p.deps_intent.is_some() {
                return Err("another dependency amendment is pending".into());
            }
            let after = replacement(&issue.body, &deps);
            let payload = serde_json::to_string(&Intent {
                board: board_url.into(),
                revision,
                deps,
                before: issue.body,
                after,
            })
            .map_err(|e| e.to_string())?;
            writing.set(true);
            b.comment(
                n,
                &core::with_fields(
                    core::entry("deps", &[&revision.to_string()], None, &payload),
                    Some(id),
                    Some(revision),
                ),
            )?;
            let log = board::entries(b, n)?;
            let p = core::project(n, &log, false);
            if !log
                .iter()
                .any(|e| p.deps_intent == Some(e.id) && core::op_id(n, &e.body) == Some(id))
            {
                return Err("dependency intent lost its revision fence; no body changed".into());
            }
        }
        Ok(finish(b, n, op))
    };
    let receipt = run().unwrap_or_else(|e| {
        if writing.get() {
            Receipt {
                outcome: "unknown",
                next: "Retry the identical deps command for durable readback.".into(),
                ..Receipt::rejected(n, op, e)
            }
        } else {
            Receipt::rejected(n, op, e)
        }
    });
    if receipt.ok() || receipt.outcome == "unknown" {
        receipt.wake(root)
    } else {
        receipt
    }
}

/// Service and CLI reconciliation share one exclusive native writer. Unknown request trees retain the fence.
pub fn finish(b: &dyn Board, n: u64, op: &Op) -> Receipt {
    let run = || -> Res<Receipt> {
        let log = board::entries(b, n)?;
        let p = core::project(n, &log, false);
        let intent = p.deps_intent.ok_or("dependency intent no longer pending")?;
        let payload = log
            .iter()
            .find(|e| e.id == intent)
            .and_then(|e| e.body.lines().nth(1))
            .ok_or("dependency intent unreadable")?;
        let data: Intent =
            serde_json::from_str(payload).map_err(|_| "dependency intent corrupt")?;
        if let Some(writer) = p.deps_writer {
            let native = log
                .iter()
                .find(|e| e.id == writer)
                .and_then(|e| e.body.lines().nth(1))
                .ok_or("dependency writer identity unknown")?;
            let native = serde_json::from_str::<dispatch::RoutingWriter>(native)
                .map_err(|_| "dependency writer identity corrupt")?;
            if !dispatch::routing_writer_exited(&native, &crate::config::hostname())? {
                return Err("dependency writer still live".into());
            }
            b.comment(n, &format!("taskq: deps-release {writer}"))?;
        }
        let native = coreq::process::identity(std::process::id())
            .map_err(|e| e.to_string())?
            .ok_or("native writer identity unavailable")?;
        let native = dispatch::RoutingWriter {
            machine: crate::config::hostname(),
            pid: native.pid,
            started: native.started,
        };
        let writer = b.comment(
            n,
            &format!(
                "taskq: deps-write {intent}\n{}",
                serde_json::to_string(&native).map_err(|e| e.to_string())?
            ),
        )?;
        let fresh = core::project(n, &board::entries(b, n)?, false);
        if fresh.deps_intent != Some(intent) || fresh.deps_writer != Some(writer) {
            return Err("dependency writer claim not acquired".into());
        }
        let written = || -> Res<()> {
            let issue = held(b, n)?;
            if issue.body != data.after {
                if issue.body != data.before {
                    return Err("draft body changed outside the intent; overwrite refused".into());
                }
                b.edit(n, &data.after)?;
            }
            if held(b, n)?.body != data.after {
                return Err("dependency body readback unconfirmed".into());
            }
            Ok(())
        }();
        // The synchronous request has returned; no in-flight HTTP child remains on this path.
        b.comment(n, &format!("taskq: deps-release {writer}"))?;
        written?;
        let done = b.comment(n, &format!("taskq: deps-done {intent}"))?;
        let log = board::entries(b, n)?;
        if !core::project(n, &log, false).accepts(done) || held(b, n)?.body != data.after {
            return Err("dependency completion readback unconfirmed".into());
        }
        if b.summaries() {
            let issue = b.issue(n)?;
            let comments = b.comments(n)?;
            if let Err(e) = crate::summary::write(
                b,
                n,
                &issue.body,
                &comments,
                &log,
                &core::fold(n, &log, false),
                None,
            ) {
                tracing::info!(task = n, "summary not written: {e}");
            }
        }
        Ok(Receipt::accepted(n, op, State::Held, done))
    };
    run().unwrap_or_else(|e| Receipt {
        outcome: "unknown",
        next: "Service reconciles the held intent; retry the identical deps command for readback."
            .into(),
        ..Receipt::rejected(n, op, e)
    })
}

#[cfg(test)]
#[path = "draft_deps_tests.rs"]
mod tests;
