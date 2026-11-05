//! Task creation commands: add, start, claim, finding and reroute (§ 5).

use crate::{board, core, dispatch, labels, recipe, runtime, service, setup};
use crate::{Res, deploy, respond};
use crate::cli::Args;
use crate::cli::host_cmd::{add_host, serve_hint};
use crate::config::Ctx;
use crate::cli::{PM_COMMANDS, USAGE};
use crate::board::{Board, entries};
use crate::cli::text::{all, compact, labelled_acceptance, limit, one_line};
use crate::core::State;
use crate::dispatch::{Op, Receipt, act, current, deps_open};
use serde_json::{Value, json};
use std::path::Path;

/// The fields of one new task (§ 1), from `add` or a recipe (§ 7).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NewTask {
    pub conversation: Option<String>,
    pub title: String,
    pub goal: String,
    pub acceptance: Vec<String>,
    pub scope: Vec<String>,
    pub deps: Vec<u64>,
    pub kind: Option<String>,
    pub runtime: Option<String>,
    pub priority: Option<String>,
    pub host: Option<String>,
    /// Extra labels, e.g. `needs-gate`.
    pub labels: Vec<String>,
    /// § 1 Task text: the rule IDs this task changes, or `all`; the rule gate passes edits inside them.
    pub rules: Option<String>,
}

/// § 1 Task text: rule IDs (`R<n>`, `§<n>`, `§2.<k>`) and `taskq <command>` spans in goal or acceptance text that
/// neither taskq.md nor the command catalog defines; `new R14` names a rule the task adds.
pub(crate) fn unknown_terms(text: &str) -> Vec<String> {
    // ponytail: checks the command word only; subcommand words when a vague subcommand causes an ask
    let mut rules: Vec<String> = vec![];
    let mut in_log = false;
    for l in runtime::CONTRACT.lines() {
        if let Some(h) = l.strip_prefix("### R") { rules.push(format!("R{}", h.split('.').next().unwrap_or(""))); }
        if let Some(h) = l.strip_prefix("## ") {
            let n = h.split('.').next().unwrap_or("");
            in_log = n == "2";
            if h.contains('.') && n.parse::<u32>().is_ok() { rules.push(format!("§{n}")); }
        }
        if in_log && l.starts_with("| ") {
            if let Some(id) = l.trim_end_matches(['|', ' ']).rsplit('|').next().map(str::trim).filter(|id| id.parse::<u32>().is_ok()) { rules.push(format!("§2.{id}")); }
        }
    }
    let catalog = format!("{USAGE}\n{PM_COMMANDS}\n{}", runtime::CONTRACT);
    let commands: Vec<&str> = catalog.split("taskq ").skip(1).filter_map(|s| s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).next()).collect();
    let mut out: Vec<String> = vec![];
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        let rule = (c == 'R' && !(prev.is_alphanumeric() || ".#/-_".contains(prev))) || c == '§';
        if rule {
            let mut j = i + 1;
            if c == '§' && chars.get(j) == Some(&' ') { j += 1; }
            let start = j;
            while j < chars.len() && (chars[j].is_ascii_digit() || (chars[j] == '.' && chars.get(j + 1).is_some_and(char::is_ascii_digit) && c == '§')) { j += 1; }
            let next = chars.get(j).copied().unwrap_or(' ');
            if j > start && !next.is_alphanumeric() {
                let id = format!("{}{}", if c == 'R' { "R" } else { "§" }, chars[start..j].iter().collect::<String>());
                let fresh = chars[..i].iter().collect::<String>().to_lowercase().ends_with("new ");
                if !fresh && !rules.contains(&id) && !out.contains(&id) { out.push(id); }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    for (k, span) in text.split('`').enumerate() {
        if k % 2 == 1 {
            if let Some(cmd) = span.strip_prefix("taskq ").and_then(|s| s.split_whitespace().next()) {
                let term = format!("taskq {cmd}");
                if !cmd.starts_with('-') && !commands.contains(&cmd) && !out.contains(&term) { out.push(term); }
            }
        }
    }
    out
}

/// § 1 Task text: add and start refuse goal or acceptance terms no contract or catalog defines, naming each.
pub(crate) fn lint_terms(text: &str) -> Res<()> {
    let bad = unknown_terms(text);
    if bad.is_empty() { Ok(()) } else { Err(format!("unknown term: {}; fix the task text (taskq.md, `taskq help`) or write `new <ID>`", bad.join(", "))) }
}

/// Task body and create claim shared by direct adds and recipes; limits checked before any write.
pub(crate) fn task_body(b: &dyn Board, t: &NewTask, op: &Op) -> Res<(String, Vec<String>)> {
    let title = t.title.chars().count();
    let goal: Vec<String> = t.goal.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let acc: Vec<String> = t.acceptance.iter().enumerate().map(|(i, s)| format!("A{}. {s}", i + 1)).collect();
    let scope: Vec<String> = t.scope.iter().map(|s| format!("- {s}")).collect();
    all([
        if t.title.trim().is_empty() || t.title.contains(['\r', '\n']) { Err("title: one nonempty line".into()) } else { Ok(()) },
        if title > 80 { Err(format!("title: {title} characters, limit 80; shorten it")) } else { Ok(()) },
        limit("Goal", &goal, 1, 2, 240),
        one_line("--acceptance", &t.acceptance),
        labelled_acceptance(&t.acceptance),
        one_line("--scope", &t.scope),
        limit("Acceptance", &acc, 1, 8, 240).map_err(|e| if acc.len() > 8 { format!("{e} (split the task, R2)") } else { e }),
        limit("Scope", &scope, 0, 4, 240),
        lint_terms(&format!("{}\n{}", t.goal, t.acceptance.join("\n"))),
    ])?;
    let mut body = format!("## Goal\n{}\n\n## Acceptance\n{}\n", goal.join("\n"), acc.join("\n"));
    if !scope.is_empty() {
        body += &format!("\n## Scope\n{}\n", scope.join("\n"));
    }
    if let Some(r) = &t.rules {
        let ids: Vec<&str> = r.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if ids != ["all"] && (ids.is_empty() || ids.iter().any(|id| !id.starts_with(['R', '§']) || !unknown_terms(id).is_empty())) {
            return Err(format!("--rules {r}: takes contract rule IDs (R<n>, §<n>, §2.<k>, §<n> <Title>) comma-separated, or all"));
        }
        body += &format!("\nRules: {}\n", ids.join(", "));
    }
    if !t.deps.is_empty() {
        body += "\n";
        body += &t.deps.iter().map(|d| format!("Depends on #{d}\n")).collect::<String>();
    }
    for (flag, v, allowed) in [("type", &t.kind, &["code", "docs", "research", "asset"][..]), ("runtime", &t.runtime, &runtime::RUNTIMES[..]), ("priority", &t.priority, &["0", "1", "2"][..])] {
        if v.as_deref().is_some_and(|v| !allowed.contains(&v)) {
            return Err(format!("--{flag} requires one of {}", allowed.join("|")));
        }
    }
    if t.host.as_deref().is_some_and(|v| v.is_empty() || v.contains(',') || v.chars().any(char::is_whitespace)) {
        return Err("--host requires one nonempty host name".into());
    }
    // § 1: a host label no heartbeat names waits forever (#1721: `host-host-mac` sat ready 30 min)
    if let Some(h) = t.host.as_deref() {
        if let Some(name) = h.strip_prefix("host-") {
            return Err(format!("--host takes the host name without host-: --host {name}"));
        }
        let issues = b.issues(None)?;
        let known: Vec<&str> = issues.iter().filter(|i| i.trusted).filter_map(service::host_name).collect();
        if !known.contains(&h) {
            return Err(format!("--host {h}: no host named {h}; hosts: {}", if known.is_empty() { "none".into() } else { known.join(", ") }));
        }
    }
    if t.priority.as_deref() == Some("0") && !b.canonical_labels()? {
        return Err("--priority 0 needs the canonical label schema (§ 9)".into());
    }
    if let Some(id) = &op.id {
        body += &format!("\nop-id: {id}\n");
    }
    // § 1: every new task is held: `q-held` instead of `taskq`, so no service of any version admits it before `start`
    let mut labels = vec![b.state_label("held")?];
    for (v, prefix) in [(&t.kind, ""), (&t.runtime, "run-"), (&t.priority, if b.canonical_labels()? { "p" } else { "priority-" }), (&t.host, "host-")] {
        if let Some(v) = v {
            labels.push(format!("{prefix}{v}"));
        }
    }
    labels.extend(t.labels.iter().cloned());
    labels::validate(&labels)?;
    // the service ignores both `prefer-*` labels (§ 4 Filter); add refuses them
    if labels.iter().filter(|l| l.starts_with("prefer-")).count() > 1 {
        return Err("conflicting prefer-* labels".into());
    }
    Ok((body, labels))
}

pub(crate) fn add_task(b: &dyn Board, root: &Path, raw: &Value, t: &NewTask, op: &Op) -> Res<Receipt> {
    let (body, labels) = task_body(b, t, op)?;
    if t.conversation.as_ref().is_some_and(|id| !setup::owner_id(id)) { return Err("Invalid conversation ID".into()); }
    if t.conversation.is_some() && op.id.is_none() { return Err("--conversation requires --op-id for same-request reconciliation".into()); }
    let origin = t.conversation.as_ref().map_or(String::new(), |id| format!("conversation: {id}"));
    let n = if op.id.is_some() {
        match claimed_create(b, raw, &t.title, &body, &labels, &origin, op) {
            Ok(n) => n,
            Err(r) => return Ok(r),
        }
    } else { b.create(&t.title, &body, &labels)? };
    let r = act(b, root, n, "hold", &[], &origin, &Op::default());
    Ok(Receipt { op_id: op.id.clone(), ..r }.wake(root))
}

/// § 7 owner gate on a held or ready task: label `needs-gate`, then the `gate` entry (op id `<op>.gate`).
pub(crate) fn gate_task(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Receipt {
    if let Err(e) = b.labels(n, &["needs-gate".into()], &[]) {
        return Receipt::rejected(n, op, e);
    }
    act(b, root, n, "gate", &[], "", &Op { id: op.id.as_ref().map(|i| format!("{i}.gate")), revision: None })
}

/// § 7 recipe run: every task is added held with op id `recipe.<name>.<version>.<run>.<k>` and read back; only then,
/// with `start`, do they start. A rerun returns the first receipts.
pub(crate) fn run_recipe(b: &dyn Board, root: &Path, raw: &Value, name: &str, run: &str, goal: Option<&str>, start: bool) -> Res<Vec<Receipt>> {
    let (version, items) = recipe::load(raw, name, run, goal)?;
    let base = |k: usize| format!("recipe.{name}.{version}.{run}.{}", k + 1);
    let (mut out, mut made) = (vec![], vec![]);
    for (k, item) in items.into_iter().enumerate() {
        let new = NewTask { deps: item.deps.iter().map(|d| made[d - 1]).collect(), ..item.task };
        let r = add_task(b, root, raw, &new, &Op { id: Some(base(k)), revision: None })?;
        let r = if r.ok() && item.gate { act(b, root, r.task, "gate", &[], "", &Op { id: Some(format!("{}.gate", base(k))), revision: None }) } else { r };
        let ok = r.ok();
        made.push(r.task);
        out.push(r);
        if !ok {
            return Ok(out);
        }
    }
    if start {
        out = made.iter().enumerate().map(|(k, n)| start_task(b, root, *n, &Op { id: Some(format!("{}.start", base(k))), revision: None })).collect::<Res<_>>()?;
    }
    Ok(out)
}

/// § 1 `start N [M..]`: every task is read and checked held before the first write, then started in order;
/// with several tasks an op id becomes `<id>.<N>` per task.
pub(crate) fn start_tasks(b: &dyn Board, root: &Path, nums: &[String], op: &Op) -> Res<Vec<Receipt>> {
    let ns: Vec<u64> = nums.iter().map(|s| s.trim_start_matches('#').parse().map_err(|_| format!("not a task number: {s}"))).collect::<Res<_>>()?;
    if ns.is_empty() { return Err("task number missing".into()) }
    if ns.len() > 1 && op.revision.is_some() { return Err("--revision names one task; start several without it".into()) }
    for &n in &ns {
        let issue = b.issue(n)?;
        let log = entries(b, n)?;
        let projection = core::project(n, &log, false);
        let held = service::is_held(&issue) || projection.task.state == State::Held;
        let repair = !service::is_task(&issue) && projection.task.state == State::Ready && service::start_logged(n, &log, &projection);
        // a retried op id may find its task already started; `act` returns the first receipt
        if !issue.trusted || !issue.open || !(held || repair || op.id.is_some()) {
            return Err(format!("#{n} is not a held task; nothing started"));
        }
        // § 1 Task text: only the Goal and Acceptance sections; Scope and links may name paths and issues
        let text: String = issue.body.split("\n## ").map(|s| s.trim_start_matches("## ")).filter(|s| s.starts_with("Goal") || s.starts_with("Acceptance")).collect::<Vec<_>>().join("\n");
        lint_terms(&text).map_err(|e| format!("#{n}: {e}; nothing started"))?;
    }
    ns.iter().map(|&n| {
        let op = if ns.len() > 1 { Op { id: op.id.as_ref().map(|id| format!("{id}.{n}")), revision: None } } else { op.clone() };
        start_task(b, root, n, &op)
    }).collect()
}

/// § 1 `start`: `taskq` goes on after the `start` entry; a retry adds the label a failed write left off.
pub(crate) fn start_task(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Res<Receipt> {
    Ok(start_one(b, root, n, op))
}

pub(crate) fn create_claim_issue(b: &dyn Board, raw: &Value, op: &Op) -> Result<u64, Receipt> {
    let read = |reason: String| Receipt {
        outcome: "unknown", next: "Retry the identical add with the same --op-id for read-only discovery.".into(),
        ..Receipt::rejected(0, op, reason)
    };
    let claim_issue = raw["create_claim_issue"].as_u64().filter(|n| *n > 0)
        .ok_or_else(|| Receipt::rejected(0, op, "configure create_claim_issue before idempotent create"))?;
    let issue = b.issue(claim_issue).map_err(read)?;
    if board::claim_issue_refusal(&issue).is_some() {
        return Err(Receipt::rejected(0, op, "create_claim_issue must be trusted and outside the queue/hosts"));
    }
    Ok(claim_issue)
}

/// § 1 `taskq claim-issue [N]`: settle the board's claim issue, then write `create_claim_issue` to taskq.json
/// and read it back. An existing key is never replaced; the receipt adds `key` to the board receipt.
pub(crate) fn claim_issue_setup(b: &dyn Board, root: &Path, adopt: Option<u64>) -> Res<Value> {
    let project = crate::config::ProjectFile::of(root);
    let read = || -> Res<Value> {
        let v = project.read().map_err(|e| format!("taskq.json unknown: {e}"))?;
        if v.is_object() { Ok(v) } else { Err("taskq.json unknown: not an object".into()) }
    };
    let mut v = read()?;
    let configured = match &v["create_claim_issue"] {
        Value::Null => None,
        k => Some(k.as_u64().filter(|n| *n > 0).ok_or("create_claim_issue in taskq.json is not an issue number")?),
    };
    let mut r = board::claim_issue(b, configured, adopt);
    r["key"] = json!("create_claim_issue");
    let Some(n) = r["issue"].as_u64().filter(|_| r["outcome"] == "accepted" && configured.is_none()) else { return Ok(r) };
    v["create_claim_issue"] = json!(n);
    let written = project.write(&v)
        .and_then(|_| read())
        .and_then(|v| if v["create_claim_issue"].as_u64() == Some(n) { Ok(()) } else { Err("taskq.json read back without the new create_claim_issue".into()) });
    if let Err(e) = written {
        r["outcome"] = json!("unknown");
        r["reason"] = json!(e);
        r["next"] = json!("Retry the same taskq claim-issue: it reuses the issue and rewrites the key.");
    }
    Ok(r)
}

/// §1 create claim: only the invocation that reads back its winning claim may POST an issue.
pub(crate) fn claimed_create(b: &dyn Board, raw: &Value, title: &str, body: &str, labels: &[String], later_text: &str, op: &Op) -> Result<u64, Receipt> {
    let id = op.id.as_deref().expect("claimed create needs an op id");
    let unknown = |n, reason: String| Receipt {
        outcome: "unknown", next: "Retry the identical add with the same --op-id for read-only discovery.".into(),
        ..Receipt::rejected(n, op, reason)
    };
    let read = |e| unknown(0, e);
    let claim_issue = create_claim_issue(b, raw, op)?;
    let request = json!({"actor": b.me().map_err(read)?, "title": title, "body": body, "labels": labels, "later_text": later_text}).to_string();
    let hash: String = ring::digest::digest(&ring::digest::SHA256, request.as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let header = format!("taskq: create-claim {id}");
    let claim = format!("{header}\nrequest-sha256: {hash}");
    // GitLab keeps a title and description without surrounding whitespace, dropping the newline that ends `add` bodies (#1914)
    let same = |stored: &str, sent: &str| stored.trim() == sent.trim();
    let first = || -> Res<Option<board::Comment>> {
        Ok(b.comments(claim_issue)?.into_iter().filter(|c| c.trusted && !c.edited && c.body.lines().next() == Some(header.as_str())).min_by_key(|c| c.id))
    };
    let discover = || -> Result<Receipt, Receipt> {
        let marker = format!("\nop-id: {id}");
        let matches: Vec<_> = b.issues(Some(0)).map_err(read)?.into_iter().filter(|i| i.body.trim_end().ends_with(&marker)).collect();
        if matches.len() != 1 {
            return Err(unknown(0, "create claim exists; issue absent or duplicated, no further POST allowed".into()));
        }
        let i = b.issue(matches[0].number).map_err(read)?;
        if !i.trusted || !same(&i.title, title) || !same(&i.body, body) {
            return Err(unknown(i.number, "created issue conflicts with the immutable request".into()));
        }
        let log = entries(b, i.number).map_err(read)?;
        let projection = core::project(i.number, &log, deps_open(b, i.number, &i.body, &log));
        let state = if !i.open { State::Closed } else if service::is_held(&i) { State::Held } else { projection.task.state };
        Ok(Receipt::accepted(i.number, op, state, projection.revision))
    };
    if let Some(first) = first().map_err(read)? {
        if first.body != claim {
            return Err(Receipt::rejected(0, op, "op id already binds a different request or actor"));
        }
        return Err(discover().unwrap_or_else(|r| r));
    }
    // Legacy op ids cannot be upgraded by blindly creating another issue.
    let marker = format!("\nop-id: {id}");
    if b.issues(Some(0)).map_err(read)?.iter().any(|i| i.body.trim_end().ends_with(&marker)) {
        return Err(Receipt::rejected(0, op, "existing issue has no create claim; owner reconciliation required"));
    }
    let own = b.comment(claim_issue, &claim).map_err(read)?;
    let winner = first().map_err(read)?.ok_or_else(|| unknown(0, "claim missing on readback; no create POST allowed".into()))?;
    if winner.body != claim {
        return Err(Receipt::rejected(0, op, "op id already binds a different request or actor"));
    }
    if winner.id != own {
        return Err(discover().unwrap_or_else(|r| r));
    }
    let n = b.create(title, body, labels).map_err(read)?;
    let issue = b.issue(n).map_err(|e| unknown(n, e))?;
    if !issue.trusted || !same(&issue.title, title) || !same(&issue.body, body) || issue.labels.len() != labels.len() || labels.iter().any(|l| !issue.labels.contains(l)) {
        return Err(unknown(n, "create readback does not match the request".into()));
    }
    Ok(n)
}

/// Section 1 finding -> existing claim/create -> separate admission; drafts remain caller-owned.
pub(crate) fn finding_submit(b: &dyn Board, raw: &Value, root: &Path, title: &str, body: &str, labels: &[String], request: &str, op: &Op, start: bool) -> (Receipt, Option<Receipt>) {
    let create = match claimed_create(b, raw, title, body, labels, request, op) {
        Ok(n) => Receipt::accepted(n, op, State::Held, 0),
        Err(r) if r.outcome == "unknown" => Receipt { next: "Retry the identical finding command for read-only create discovery; optional start has its own receipt.".into(), ..r },
        Err(r) => r,
    };
    if !create.ok() || !start { return (create, None) }
    let n = create.task;
    let admission = Op { id: Some(format!("{}.start", op.id.as_deref().unwrap())), revision: None };
    let r = start_one(b, root, n, &admission);
    (create, Some(r))
}

/// One finding's body (Source, Evidence, op id) and its identity-derived op id, then its claimed create
/// (docs/design/finding-tasks.md); `taskq finding` and the service's tidy findings (docs/design/worktrees.md) share it.
/// `evidence` lines come without the `- ` bullet. Errors come only from the task body.
#[allow(clippy::too_many_arguments)]
pub(crate) fn submit_finding(b: &dyn Board, raw: &Value, root: &Path, board: &str, web_host: &str, repo: &str, new: &NewTask, source: &str, evidence: &[String], op: &mut Op, start: bool) -> Res<(Receipt, Option<Receipt>)> {
    let (mut body, labels) = task_body(b, new, &Op::default())?;
    let board_url = b.url(0);
    let identity = json!({"kind": board, "host": web_host, "repo": repo, "url": board_url.trim_end_matches("/0"), "source": source});
    let hash: String = ring::digest::digest(&ring::digest::SHA256, identity.to_string().as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    op.id = Some(format!("finding.{hash}"));
    body += &format!("\n## Source\n{source}\n");
    if !evidence.is_empty() { body += &format!("\n## Evidence\n{}\n", evidence.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n")); }
    let request = json!({"board": identity, "start": start}).to_string();
    body += &format!("\nop-id: {}\n", op.id.as_deref().unwrap());
    Ok(finding_submit(b, raw, root, &new.title, &body, &labels, &request, op, start))
}

/// Section 1 start log -> eligibility label -> authoritative readback, shared by start and finding.
pub(crate) fn start_one(b: &dyn Board, root: &Path, n: u64, op: &Op) -> Receipt {
    let mut start_op = op.clone();
    let mut repair = false;
    // A finding's q-held projection prevents admission even before its hold log is initialized.
    let mut initialize = || -> Res<Option<Receipt>> {
        let issue = b.issue(n)?;
        let log = entries(b, n)?;
        let projection = core::project(n, &log, false);
        let revision = projection.revision;
        if op.revision.is_some_and(|r| r != revision) { return Ok(None) }
        repair = issue.trusted && issue.open && projection.task.state == State::Ready && service::start_logged(n, &log, &projection)
            && (!service::is_task(&issue) || issue.labels.iter().any(|l| l == "q-held"));
        if issue.open && service::is_held(&issue) && revision == 0 {
            let hold = Op { id: op.id.as_ref().map(|id| format!("{id}.hold")), revision: Some(revision) };
            let r = act(b, root, n, "hold", &[], "", &hold);
            return Ok(Some(Receipt { op_id: op.id.clone(), ..r }));
        }
        Ok(None)
    };
    match initialize() {
        Ok(Some(r)) if !r.ok() => return r,
        Ok(Some(r)) => { if start_op.revision.is_some() { start_op.revision = r.revision; } }
        Err(e) => return Receipt { outcome: "unknown", ..Receipt::rejected(n, op, e) },
        Ok(None) => {}
    }
    if !repair {
        let r = act(b, root, n, "start", &[], "", &start_op);
        if !r.ok() { return r }
    }
    let readback = || -> Res<Receipt> {
        let (issue, t, _) = current(b, n)?;
        let log = entries(b, n)?;
        let started = service::start_logged(n, &log, &core::project(n, &log, false));
        if started && issue.open { service::project(b, &issue, &t)?; }
        if started && issue.open && !service::is_task(&issue) {
            b.labels(n, &[service::TASK_LABEL.into()], &[])?;
        }
        let (issue, t, _) = current(b, n)?;
        let want = b.state_label(t.state.name())?;
        if issue.open && (!service::is_task(&issue) || !issue.labels.contains(&want) || issue.labels.iter().any(|l| l.starts_with("q-") && l != &want)) {
            return Err("start eligibility labels not read back".into());
        }
        Ok(Receipt::accepted(n, op, if issue.open { t.state } else { State::Closed }, core::revision(n, &entries(b, n)?)))
    };
    readback().map(|r| if repair { r.wake(root) } else { r }).unwrap_or_else(|e| Receipt { outcome: "unknown", next: format!("Read taskq inbox {n}; retry the same start operation."), ..Receipt::rejected(n, op, e) })
}

/// §5: explicit owner request; labels change only on ready tasks without unreleased execution.
pub(crate) fn reroute(b: &dyn Board, root: &Path, from: &str, to: &str) -> Res<Vec<Receipt>> {
    if from == to || ![from, to].iter().all(|rt| runtime::CLI_RUNTIMES.contains(rt)) {
        return Err("reroute requires two different CLI runtimes: claude, codex or antigravity".into());
    }
    let (old, new) = (format!("run-{from}"), format!("run-{to}"));
    b.ensure_labels(std::slice::from_ref(&new))?;
    let mut out = vec![];
    for issue in b.issues(None)?.into_iter().filter(|i| i.open && service::is_task(i) && (i.labels.contains(&old) || i.labels.contains(&new))) {
        let n = issue.number;
        let pending=match entries(b,n) {
            Ok(log)=>core::project(n,&log,false).reroute,
            Err(e)=>{out.push(Receipt {outcome:"unknown",..Receipt::rejected(n,&Op::default(),e)}.wake(root));continue;}
        };
        if !issue.labels.contains(&old) && !pending.as_ref().is_some_and(|(_,source,target)| source==from && target==to) {continue;}
        let mut submitted=pending.is_some();
        let mut change = || -> Res<Receipt> {
            let (issue, task, _) = current(b, n)?;
            let log = entries(b, n)?;
            if !issue.open || task.state != State::Ready { return Err(format!("#{} is {}; reroute only changes open ready tasks", n, task.state.name())); }
            if pending.is_none() {labels::validate(&issue.labels)?;}
            if !issue.labels.contains(&old) && pending.is_none() { return Err("source runtime pin changed; read the task again".into()); }
            if core::project(n, &log, false).executions.iter().any(|x| x.release.is_none()) { return Err("execution unresolved; preserve its runtime and session".into()); }
            let projection=core::project(n,&log,false);
            if let Some((_,source,target))=&projection.reroute {
                if source!=from || target!=to {return Err("another reroute is pending; wait for its readback".into());}
            } else {
                submitted=true;
                let id=b.comment(n, &format!("taskq: reroute {from} {to}\nStrict runtime pin: {old} -> {new}\nrouting-fence: 1"))?;
                let fresh=core::project(n,&entries(b,n)?,false);
                if fresh.reroute.as_ref().map(|r|r.0)!=Some(id) {return Err("routing fence not acquired; no labels changed".into());}
            }
            Ok(dispatch::finish_reroute(b,n))
        };
        out.push(change().unwrap_or_else(|e| {
            let mut r=Receipt::rejected(n,&Op::default(),e);
            if submitted {r.outcome="unknown";r.next="The service reconciles pending routing intents; retry the same command for readback.".into();}
            r
        }).wake(root));
    }
    Ok(out)
}

pub(crate) fn emit_finding(create: Receipt, start: Option<Receipt>, json: bool) -> Res<()> {
    let ok = create.ok() && start.as_ref().is_none_or(Receipt::ok);
    if json {
        println!("{}", json!({"create": create, "start": start}));
    } else {
        println!("Create: {}", create.text());
        if let Some(r) = start { println!("Start: {}", r.text()); }
    }
    if ok { Ok(()) } else { Err(String::new()) }
}

/// `add` and `finding`: the § 1 task body from flags; limits are checked before any write.
pub(crate) fn add_cmd(c: &Ctx, cmd: &str, a: &Args, op: &mut Op) -> Res<()> {
    let (b, cfg, root) = (c.board.as_ref(), &c.cfg, c.cfg.root.as_path());
    let json = a.flags.contains_key("json");
    let finding = cmd == "finding";
    if finding && a.one("conversation").is_some() { return Err("--conversation requires add".into()); }
    // § 1 task body: Goal, Acceptance, Scope, dependencies; limits checked before any write
    let title = a.pos.get(1).filter(|t| !t.trim().is_empty()).ok_or("title missing")?;
    let deps: Vec<u64> = a.all("deps").iter().flat_map(|d| d.split([',', ' '])).filter(|d| !d.is_empty()).map(|d| d.trim_start_matches('#').parse().map_err(|_| format!("bad --deps {d}"))).collect::<Res<_>>()?;
    // §4: only an explicit --runtime adds a binding pin; type never adds one.
    if a.flags.contains_key("later") {
        return Err("--later is retired: every add is held (§ 1); park it with `taskq later N`".into());
    }
    if a.flags.contains_key("prefer") {
        return Err("--prefer is retired; use --runtime only for an owner pin (none: host preference with eligible fallback)".into());
    }
    // a repeated single-value flag is refused before any write; `one` would keep only the last value
    let mut repeated: Vec<_> = a.flags.iter().filter(|(f, v)| v.len() > 1 && !["acceptance", "scope", "deps", "evidence"].contains(&f.as_str())).map(|(f, _)| format!("--{f}")).collect();
    repeated.sort();
    if !repeated.is_empty() {
        return Err(format!("{} given more than once; pass each once (a two-line --goal uses one value with a newline)", repeated.join(", ")));
    }
    let new = NewTask {
        conversation: a.one("conversation").map(String::from),
        title: title.clone(),
        goal: a.need("goal")?.to_string(),
        acceptance: a.all("acceptance"),
        scope: a.all("scope"),
        deps,
        kind: a.one("type").map(String::from),
        runtime: a.one("runtime").map(String::from),
        priority: a.one("priority").map(String::from),
        host: add_host(a.one("host"), cfg),
        labels: if a.flags.contains_key("gate") { vec!["needs-gate".into()] } else { vec![] },
        rules: a.one("rules").map(String::from),
    };
    if finding {
        task_body(b, &new, &Op::default()).map_err(|e| serve_hint(e, cfg))?;
        for (flag, values) in &a.flags {
            if !["board", "source", "goal", "acceptance", "scope", "deps", "type", "runtime", "priority", "host", "evidence", "start", "json"].contains(&flag.as_str()) {
                return Err(format!("finding does not accept --{flag}"));
            }
            if !["acceptance", "scope", "deps", "evidence"].contains(&flag.as_str()) && values.len() != 1 {
                return Err(format!("finding --{flag} requires one value"));
            }
        }
        let board_url = b.url(0);
        let board_url = board_url.trim_end_matches("/0");
        if a.all("board").len() != 1 || a.need("board")? != board_url {
            return Err("finding --board must match the configured board URL from taskq status --json".into());
        }
        let source = a.need("source")?;
        let parts: Vec<_> = source.split('/').collect();
        if a.all("source").len() != 1 || parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.bytes().all(|c| c == b'.') || p.len() > 64 || !p.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))) {
            return Err("--source: installation/rule/episode; each part 1-64 ASCII letters, digits, dot, underscore or hyphen".into());
        }
        let evidence: Vec<_> = a.all("evidence").into_iter().map(String::from).collect();
        let bullets: Vec<_> = evidence.iter().map(|e| format!("- {e}")).collect();
        one_line("--evidence", &bullets)?;
        limit("Evidence", &bullets, 0, 4, 240)?;
        let (create, start) = submit_finding(b, &c.raw, root, cfg.board.name, &cfg.web_host, &cfg.repo, &new, source, &evidence, op, a.flags.contains_key("start"))
            .map_err(|e| serve_hint(e, cfg))?;
        let create = if create.ok() { create.wake(root) } else { create };
        return emit_finding(create, start, json);
    } else if ["start", "source", "board", "evidence"].iter().any(|f| a.flags.contains_key(*f)) {
        return Err("--start/--source/--board/--evidence require finding".into());
    }
    let r = add_task(b, root, &c.raw, &new, op).map_err(|e| serve_hint(e, cfg))?;
    let r = if r.ok() && a.flags.contains_key("gate") { gate_task(b, root, r.task, op) } else { r };
    respond(a, &[r], json)
}

/// `issue`: reads one issue, or writes one comment or label.
pub(crate) fn issue_cmd(c: &Ctx, a: &Args, op: &Op) -> Res<()> {
    let b = c.board.as_ref();
    let json = a.flags.contains_key("json");
    let n = a.num(1)?;
    if a.flags.iter().any(|(k, v)| !["comment", "label", "op-id", "json"].contains(&k.as_str()) || v.len() != 1) {
        return Err("issue: use --comment or --label, --op-id and --json once each".into());
    }
    let write = match (a.one("comment"), a.one("label")) {
        (None, None) if op.id.is_none() => { println!("{}", board::issue_json(b, n)?); return Ok(()); }
        (Some(t), None) => board::IssueWrite::Comment(t),
        (None, Some(l)) => board::IssueWrite::Label(l),
        _ => return Err("issue: pass one of --comment or --label to write".into()),
    };
    let r = board::issue_write(b, n, write, op.id.as_deref());
    let ok = r["outcome"] == "accepted";
    if json { println!("{r}"); }
    else if ok { println!("#{n} {}", if r["label"].is_string() { format!("label {}", r["label"].as_str().unwrap_or("")) } else { format!("comment {}", r["comment"]) }); }
    else { eprintln!("Error: #{n}: {}\nNext: {}", compact(r["reason"].as_str().unwrap_or(""), 153), compact(r["next"].as_str().unwrap_or(""), 154)); }
    if ok { Ok(()) } else { Err(String::new()) }
}

/// `deployed`: records one deployment on the tasks its range names.
pub(crate) fn deployed_cmd(c: &Ctx, a: &Args) -> Res<()> {
    let (b, cfg) = (c.board.as_ref(), &c.cfg);
    if a.flags.iter().any(|(k, v)| !["version", "env", "range", "host", "json"].contains(&k.as_str()) || v.len() != 1) {
        return Err("deployed: use --version, --env, --range, optionally --host and --json, once each".into());
    }
    let need = |k: &str| a.one(k).ok_or(format!("deployed: --{k} is required"));
    let v = deploy::run(b, &cfg.root, need("version")?, need("env")?, need("range")?, a.one("host"))?;
    if a.flags.contains_key("json") { println!("{v}"); return Ok(()); }
    for r in v["tasks"].as_array().into_iter().flatten() {
        let reason = r["reason"].as_str().filter(|s| !s.is_empty()).map_or(String::new(), |s| format!(": {s}"));
        println!("#{} {}{reason}", r["issue"], r["outcome"].as_str().unwrap_or_default());
    }
    for r in v["skipped"].as_array().into_iter().flatten() {
        println!("skipped {}: {}", r["url"].as_str().unwrap_or_default(), r["reason"].as_str().unwrap_or_default());
    }
    if v["tasks"].as_array().is_none_or(|t| t.is_empty()) && v["skipped"].as_array().is_none_or(|t| t.is_empty()) { println!("no Task: trailers in the range"); }
    if v["tasks"].as_array().into_iter().flatten().any(|r| r["outcome"] == "unknown") { return Err("deployed: some entries are unknown; rerun the same command".into()); }
    Ok(())
}

/// `claim-issue`: sets `create_claim_issue` for this project.
pub(crate) fn claim_issue_cmd(c: &Ctx, a: &Args) -> Res<()> {
    let (b, root) = (c.board.as_ref(), c.cfg.root.as_path());
    let json = a.flags.contains_key("json");
    if a.flags.iter().any(|(k, v)| k != "json" || v.len() != 1) { return Err("claim-issue: only --json, once".into()); }
    if std::env::var_os("TASKQ_ATTEMPT").is_some() { return Err("a worker cannot change create_claim_issue".into()); }
    let adopt = if a.pos.len() > 1 { Some(a.num(1)?) } else { None };
    let r = claim_issue_setup(b, root, adopt)?;
    let ok = r["outcome"] == "accepted";
    if json { println!("{r}"); }
    else if ok { println!("#{} create_claim_issue {} ({})", r["issue"], r["url"].as_str().unwrap_or(""), r["source"].as_str().unwrap_or("")); }
    else { eprintln!("Error: {}\nNext: {}", compact(r["reason"].as_str().unwrap_or(""), 153), compact(r["next"].as_str().unwrap_or(""), 154)); }
    if ok { Ok(()) } else { Err(String::new()) }
}
