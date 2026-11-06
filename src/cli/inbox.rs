//! The `inbox` and `wait` readers (§ 5).

use crate::Res;
use crate::board::{Board, entries};
use crate::config::load;
use crate::core::{Entry, State, fold};
use crate::dispatch::deps_open;
use crate::paths::dot;
use crate::service::Config;
use crate::snapshot::in_scope;
use crate::{
    board, core, dispatch, display, platform, readiness, reply_files, reports, runtime, service,
    snapshot, transcript,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) fn inbox(b: &dyn Board, root: &Path, n: u64) -> Res<String> {
    let issue = b.issue(n)?;
    let comments = b.comments(n)?;
    let log = board::reply::log(n, &comments)?;
    let projection = core::project(n, &log, deps_open(b, n, &issue.body, &log));
    let t = &projection.task;
    let mut out = vec![
        format!("#{n} {}\n{}", issue.title, b.url(n)),
        core::task_context(n, issue.body.trim(), &log),
        format!("Folded: {}", serde_json::to_string(&t).unwrap_or_default()),
    ];
    out.push(format!("Revision: {}", projection.revision));
    if let Some((k, why)) = &t.pick {
        // § 2: the worker's star and the PM's pick, side by side
        let star = core::options(t.card.as_deref().unwrap_or(""))
            .into_iter()
            .find(|o| o.2)
            .map_or("none".into(), |o| format!("{n}.{}", o.0));
        out.push(format!(
            "Recommendation: worker ★ {star} · PM {n}.{k}: {why}"
        ));
    }
    out.push("Log:".into());
    let files = reply_files::prepare(b, root, n, &log)?;
    if !files.is_empty() {
        out.push(files);
    }
    for e in &log {
        out.push(format!(
            "[{}{}] {}",
            e.id,
            if e.edited { " edited, ignored" } else { "" },
            e.body.trim()
        ));
    }
    read_receipts(b, n, &log, &comments);
    Ok(out.join("\n\n"))
}

/// § 2 read receipt: the live attempt's inbox read reacts `eyes` to each owner note it has now read.
pub(crate) fn read_receipts(b: &dyn Board, n: u64, log: &[Entry], comments: &[board::Comment]) {
    if let Some(a) = std::env::var("TASKQ_ATTEMPT")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        dispatch::mark_read(b, n, log, comments, a);
    }
}

/// § 5 `inbox N --json`: the folded task, its current attempt's progress and each accepted answer with its writer, board
/// time and question revision, from the same issue, comment and dependency reads as the human `inbox` (#1885).
pub(crate) fn inbox_json(b: &dyn Board, root: &Path, me: &str, n: u64) -> Res<Value> {
    let issue = b.issue(n)?;
    let comments = b.comments(n)?;
    let log = board::reply::log(n, &comments)?;
    let p = core::project(n, &log, deps_open(b, n, &issue.body, &log));
    let t = &p.task;
    let at = |id: u64| comments.iter().find(|c| c.id == id);
    let answers: Vec<Value> = p.answers.iter().map(|a| json!({"id": a.id, "by": at(a.id).map(|c| c.author.as_str()), "at": at(a.id).map(|c| c.created),
        "revision": a.revision, "option": a.option, "text": core::delivery(n, &log, a.id), "late": a.late, "read": at(a.id).and_then(|c| c.read)})).collect();
    read_receipts(b, n, &log, &comments);
    let attempt = t.attempt.map(|a| {
        let session = t.session.as_deref().unwrap_or_default();
        let (native, host) = session.rsplit_once(' ').unwrap_or((session, ""));
        let role = t.role.as_deref().unwrap_or_default();
        // R6 3: the attempt log's update time is known only on the attempt's own host
        let updated = (host == me).then(|| fs::metadata(runtime::attempt_log(root, role, n, a)).and_then(|m| m.modified()).ok()).flatten()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
        json!({"id": a, "role": role, "runtime": native.split(':').next(), "host": host, "started": at(a).map(|c| c.created),
            "stage": t.state.name(), "stages": t.stages, "pending_notes": t.inbox.len(), "delivered": p.delivered(a), "log_updated": updated})
    });
    let last = log
        .iter()
        .max_by_key(|e| e.id)
        .map(|e| json!({"id": e.id, "at": at(e.id).map(|c| c.created)}));
    Ok(
        json!({"schema": 1, "task": n, "title": issue.title, "url": b.url(n), "open": issue.open, "state": t.state.name(), "revision": p.revision,
        "source_readiness": readiness::source(n,&b.url(n),&log), "card": t.card, "result": t.result, "attempt": attempt, "last": last, "answers": answers}),
    )
}

/// MP14 Completed: one board page of closed tasks with their sourced outcome, result line, last PR link and merge receipt; no store.
pub(crate) fn closed_history(
    b: &dyn Board,
    page: u32,
    since: Option<i64>,
    gitlab: bool,
) -> Res<Value> {
    let (issues, more) = b.closed_tasks(page, since)?;
    let tasks: Vec<Value> = issues
        .iter()
        .filter(|(_, closed)| since.is_none_or(|t| *closed >= t))
        .map(|(i, closed)| closed_row(b, i, Some(*closed), gitlab))
        .collect();
    Ok(json!({"schema": 1, "source": "board", "page": page.max(1), "more": more, "tasks": tasks}))
}

/// P-W1: exact pin reads share Completed's outcome projection, never depend on its loaded page.
pub(crate) fn closed_row(
    b: &dyn Board,
    i: &board::Issue,
    closed: Option<i64>,
    gitlab: bool,
) -> Value {
    let comments = b.comments(i.number);
    let log = comments
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|comments| board::reply::log(i.number, comments));
    // § 1: GitHub records why an issue closed; GitLab records no reason, so a closed GitLab issue is done
    let outcome = if !log
        .as_ref()
        .is_ok_and(|log| fold(i.number, log, false).state != State::Doing)
    {
        "unknown"
    } else {
        match (i.reason.as_deref(), gitlab) {
            (Some("completed"), _) | (None, true) => "success",
            (Some("not_planned"), _) => "not_planned",
            _ => "unknown",
        }
    };
    let (result, pr) = log.as_ref().map_or((Value::Null, Value::Null), |log| {
        let t = fold(i.number, log, false);
        let pr = log
            .iter()
            .flat_map(|e| e.body.split_whitespace())
            .filter(|w| {
                w.starts_with("https://")
                    && (w.contains("/pull/") || w.contains("/merge_requests/"))
            })
            .last();
        (
            json!(
                t.publication
                    .as_deref()
                    .and_then(|p| p.lines().next())
                    .map(|l| l.trim_start_matches("Changed:").trim())
            ),
            json!(pr.map(|u| u.trim_end_matches(|c: char| !c.is_ascii_alphanumeric()))),
        )
    });
    let conversation = log
        .as_ref()
        .ok()
        .and_then(|log| core::conversation(i.number, log));
    let history = log
        .as_ref()
        .ok()
        .zip(comments.as_ref().ok())
        .map(|(log, comments)| {
            dispatch::timeline(
                i.number,
                log,
                &core::project(i.number, log, false),
                comments,
            )
            .iter()
            .map(|(q, a)| json!({"question":q.as_deref().map(snapshot::question),"answered":a}))
            .collect::<Vec<_>>()
        });
    let attempts = |role| {
        log.as_ref()
            .ok()
            .zip(comments.as_ref().ok())
            .map(|(log, comments)| {
                snapshot::attempts(&core::project(i.number, log, false), log, comments, role)
            })
    };
    let workers = attempts("worker");
    let reviewers = attempts("reviewer");
    let timing = log
        .as_ref()
        .ok()
        .zip(comments.as_ref().ok())
        .map(|(log, comments)| {
            snapshot::task_timing(
                &core::project(i.number, log, false),
                comments,
                !i.open,
                closed,
            )
        });
    let merged = log.as_ref().ok().and_then(|log| merge_receipt(log));
    // § 7 close outcome (#1800): delivered, no change or replaced by #M with its current state, plus unverified items
    let verdict = log.as_ref().ok().and_then(|log| {
        log.iter()
            .rev()
            .find(|e| !e.edited && e.body.starts_with("taskq: close"))
            .map(|e| core::verdict(i.number, log, e.body.split_once('\n').map_or("", |x| x.1)))
    });
    let verdict = verdict.as_deref();
    let next = verdict.and_then(display::successor).map(|m| {
        dispatch::current(b, m).map_or(
            "unknown",
            |(i, t, _)| if i.open { t.state.name() } else { "closed" },
        )
    });
    let close = log.is_ok().then(|| display::close_outcome(verdict, next));
    json!({"n": i.number,"revision":log.as_ref().ok().map(|log|core::revision(i.number,log)), "title": i.title, "url": b.url(i.number), "state": if i.open { "open" } else { "closed" }, "closed": closed, "outcome": outcome, "close": close, "result": result, "pr": pr, "merged": merged, "source_readiness": log.as_ref().map_or_else(|_|readiness::unavailable(i.number,&b.url(i.number),"task log unavailable"),|log|readiness::source(i.number,&b.url(i.number),log)), "timing":timing,"history":history,"workers":workers,"reviewers":reviewers,"brief":i.body,"conversation": conversation.map(|id| json!({"id":id,"brief":i.body,"verdict":verdict})), "error": log.as_ref().err().map(|e| e.text())})
}

pub(crate) fn closed_pin(b: &dyn Board, n: u64, gitlab: bool) -> Res<Value> {
    let i = b.issue(n)?;
    if i.number != n || !service::is_task(&i) {
        return Err("Pinned issue is not the requested trusted task".into());
    }
    Ok(json!({"schema":1,"source":"board","more":false,"tasks":[closed_row(b,&i,i.closed,gitlab)]}))
}

/// The native session of `n`'s closed worker on this host: the board holds a job id for a remote-control launch, the
/// attempt log the session, as a live read resolves it.
pub(crate) fn closed_session(
    root: &Path,
    runtime: &str,
    id: &str,
    n: u64,
    attempt: u64,
) -> Res<String> {
    let session = runtime::session_id(root, runtime, id, n, attempt, "worker");
    if session == "-" || !runtime::plain_thread(&session) {
        return Err("Worker native session unknown".into());
    }
    Ok(session)
}

/// A closed worker's page: from its transcript on this host, else through `remote` (the host link) on its host.
pub(crate) fn closed_transcript(
    b: &dyn Board,
    root: &Path,
    n: u64,
    host: &str,
    before: Option<u64>,
    remote: impl FnOnce(&str, u64) -> Res<Value>,
) -> Res<Value> {
    let value = reports::selected(root, b, n, false, None)?;
    let (runtime, id, owner, attempt) = reports::worker(&value, None)?;
    if owner != host {
        return remote(&owner, attempt);
    }
    let session = closed_session(root, &runtime, &id, n, attempt)?;
    let path = runtime::owner::transcript(&runtime, &session)?;
    Ok(transcript::before(transcript::format(&runtime), &session, &path, before)?.json())
}

/// Host link principle 3: the peer finds a closed worker's session on the board and its own attempt log, never in the request.
pub(crate) fn closed_peer_session(root: &Path, local: &str, n: u64, attempt: u64) -> Res<String> {
    let (cfg, raw) = load(root)?;
    let board = cfg.board.open(
        &cfg.web_host,
        &cfg.repo,
        raw["label_schema"].as_str(),
        &platform::tool(&cfg.install),
    )?;
    let value = reports::selected(root, &*board, n, !cfg.board.closed_reason, None)?;
    let (runtime, id, owner, last) = reports::worker(&value, Some(attempt))?;
    if owner != local || last != attempt {
        return Err("Worker transcript is available only on its host".into());
    }
    Ok(format!(
        "{runtime}:{}",
        closed_session(root, &runtime, &id, n, attempt)?
    ))
}

/// #1679: the PR/MR merge `close` read back (§ 7 `delivery <preset> r<R>: merged #P ..`, GitLab `merged !P`); none stays unknown.
/// Issue closure, a PR link or a direct fast-forward is no merge evidence.
pub(crate) fn merge_receipt(log: &[Entry]) -> Option<String> {
    log.iter()
        .rev()
        .filter(|e| {
            !e.edited
                && e.body.lines().next().is_some_and(|h| {
                    h.trim() == "taskq: close" || h.trim().starts_with("taskq: close attempt ")
                })
        })
        .flat_map(|e| {
            e.body.lines().filter_map(|l| {
                l.trim()
                    .strip_prefix("delivery ")?
                    .split_once(": ")
                    .map(|(_, r)| r.trim())
            })
        })
        .find(|r| r.starts_with("merged #") || r.starts_with("merged !"))
        .map(String::from)
}

/// PM wait (§ 5): the cursor `<root>/.taskq/pm-cursor` holds the board time of the last read and the last seen state
/// per task. Only moves into `ask`, `review` with options, closed or `failed` are printed.
pub(crate) fn wait(
    b: &dyn Board,
    cfg: &Config,
    window_min: u64,
    only: &[String],
) -> Res<Vec<String>> {
    let path = dot(&cfg.root).join("pm-cursor");
    let mut since = None;
    let mut seen: HashMap<u64, String> = HashMap::new();
    if let Ok(text) = fs::read_to_string(&path) {
        let mut lines = text.lines();
        since = lines.next().and_then(|l| l.parse::<i64>().ok());
        seen = lines
            .filter_map(|l| {
                l.split_once(' ')
                    .and_then(|(n, s)| Some((n.parse().ok()?, s.to_string())))
            })
            .collect();
    }
    let deadline = Instant::now() + Duration::from_secs(window_min * 60);
    loop {
        let now = b.now()?;
        let issues = b.issues(since.map(|s| s - 1).or(Some(now)))?;
        let open: Vec<u64> = b.issues(None)?.iter().map(|i| i.number).collect();
        let mut moved = vec![];
        for i in issues
            .iter()
            .filter(|i| service::is_task(i) && in_scope(i, only))
        {
            let t = fold(
                i.number,
                &entries(b, i.number)?,
                core::depends(&i.body)
                    .iter()
                    .any(|d| d.repo.is_none() && open.contains(&d.n)),
            );
            let state = if !i.open {
                "closed".to_string()
            } else if t.state == State::Review && t.card.is_some() {
                "review*".into()
            } else {
                t.state.name().into()
            };
            if seen.get(&i.number) != Some(&state)
                && matches!(state.as_str(), "ask" | "review*" | "closed" | "failed")
            {
                moved.push(format!("{} #{}", state.trim_end_matches('*'), i.number));
            }
            seen.insert(i.number, state);
        }
        since = Some(now);
        let mut text = format!("{now}\n");
        let mut keep: Vec<_> = seen.iter().filter(|(_, s)| *s != "closed").collect();
        keep.sort();
        for (n, s) in keep {
            text += &format!("{n} {s}\n");
        }
        fs::create_dir_all(dot(&cfg.root)).map_err(|e| e.to_string())?;
        platform::write_atomic(&path, &text)?;
        if !moved.is_empty() {
            return Ok(moved);
        }
        if Instant::now() >= deadline {
            return Ok(vec!["tick".into()]);
        }
        std::thread::sleep(
            Duration::from_secs(cfg.poll.clamp(5, 60))
                .min(deadline.saturating_duration_since(Instant::now()) + Duration::from_secs(1)),
        );
    }
}
