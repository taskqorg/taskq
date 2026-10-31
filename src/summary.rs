//! § 2 Summary: one trusted comment per task that projects the fold for people, like a label. `fold` never reads it.
use crate::{Res, board::{Board, Comment}, core::{self, Entry, State, Task}};

pub const MARKER: &str = "<!-- taskq-summary -->";
/// File links shown before `and N more`.
const FILES: usize = 3;
const EXTENSIONS: [&str; 19] = ["zip", "gz", "tgz", "pdf", "html", "htm", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp4", "mov", "webm", "txt", "log", "csv", "json"];

/// A summary comment: trusted, its first line the marker.
pub fn is(c: &Comment) -> bool {
    c.trusted && c.body.lines().next().is_some_and(|l| l.trim() == MARKER)
}

/// The summary that counts: the lowest id of several.
pub fn find(comments: &[Comment]) -> Option<&Comment> {
    comments.iter().filter(|c| is(c)).min_by_key(|c| c.id)
}

/// The summary text of task `n`: `body` is the issue text, `t` its fold, `link` a comment's URL (0: the issue).
pub fn render(n: u64, body: &str, log: &[Entry], t: &Task, link: &dyn Fn(u64) -> String) -> String {
    let p = core::project(n, log, false);
    let mut accepted: Vec<&Entry> = log.iter().filter(|e| p.accepts(e.id)).collect();
    accepted.sort_by_key(|e| e.id);
    let last = |kinds: &[&str]| accepted.iter().rev().find(|e| kinds.contains(&core::kind(n, &e.body))).copied();
    let mut head = vec![format!("**{}**", title(t.state.name()))];
    if let Some(r) = t.result.as_deref().filter(|_| t.state != State::Closed) {
        head.push(format!("result {}", &r[..r.len().min(7)]));
    }
    let merged = (t.state == State::Closed).then(|| crate::merge_receipt(log)).flatten();
    match (&merged, pr(log)) {
        (Some(m), _) => head.push(m.clone()),
        (None, Some((label, url))) => head.push(format!("[{label}]({url})")),
        _ => {}
    }
    if matches!(t.state, State::Doing | State::Review) && t.session.is_some() {
        head.push(format!("{} running", t.role.as_deref().unwrap_or("worker")));
    }
    let mut lines = vec![MARKER.to_string(), head.join(" · ")];
    match t.state {
        State::Ask => lines.extend(t.card.iter().flat_map(|c| c.lines()).map(|l| l.trim_end().to_string()).filter(|l| !l.trim().is_empty())),
        State::Review => {
            let text = t.publication.clone().or_else(|| last(&["result"]).map(|e| core::entry_text(n, &e.body).to_string()));
            lines.extend(outcome(text.as_deref().unwrap_or("")));
        }
        State::Closed => {
            let text = last(&["close"]).map(|e| core::verdict(n, log, core::entry_text(n, &e.body))).unwrap_or_default();
            lines.extend(outcome(&text));
        }
        State::Later | State::Failed => {
            if let Some(reason) = last(&["later", "failed"]).and_then(|e| core::entry_text(n, &e.body).lines().map(str::trim).find(|l| !l.is_empty())) {
                lines.push(format!("Reason: {}", reason.strip_prefix("Reason:").map_or(reason, str::trim_start)));
            }
        }
        _ => {}
    }
    // § 2: newest outcome first, the issue text last; one link per URL
    let mut files: Vec<(u64, String, String)> = vec![];
    let sources = accepted.iter().rev().filter(|e| matches!(core::kind(n, &e.body), "result" | "ask" | "publish" | "close"))
        .map(|e| (e.id, core::entry_text(n, &e.body).to_string())).chain(std::iter::once((0, body.to_string())));
    for (id, text) in sources {
        for (label, url) in links(&text).into_iter().filter(|(_, u)| file(u)) {
            if !files.iter().any(|f| f.2 == url) { files.push((id, label, url)); }
        }
    }
    if !files.is_empty() {
        let shown: Vec<String> = files.iter().take(FILES).map(|(_, label, url)| format!("[{label}]({url})")).collect();
        let mut line = format!("Files: {}", shown.join(", "));
        if let Some((id, _, _)) = files.get(FILES) {
            line.push_str(&format!(" and [{} more]({})", files.len() - FILES, link(*id)));
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// The current summary of task `n` on board `b`.
pub fn text(b: &dyn Board, n: u64, body: &str, log: &[Entry], t: &Task) -> String {
    render(n, body, log, t, &|id| if id == 0 { b.url(n) } else { b.comment_url(n, id) })
}

/// Edits the summary of task `n` to its current text, or creates it when missing. Returns whether it wrote.
pub fn write(b: &dyn Board, n: u64, body: &str, comments: &[Comment], log: &[Entry], t: &Task) -> Res<bool> {
    put(b, n, comments, &text(b, n, body, log, t))
}

/// Writes `want` unless the summary already reads so.
pub fn put(b: &dyn Board, n: u64, comments: &[Comment], want: &str) -> Res<bool> {
    match find(comments) {
        Some(c) if same(&c.body, want) => Ok(false),
        Some(c) => b.edit_comment(n, c.id, want).map(|_| true),
        None => b.comment(n, want).map(|_| true),
    }
}

/// Board text compared without line-ending or trailing-space differences, so a read back never edits again.
pub fn same(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n").trim_end() == b.trim_end()
}

fn title(state: &str) -> String {
    let mut c = state.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// `Changed:` and `Open:` with their continuation lines; another `Field:`, an option or a `delivery` line ends one.
fn outcome(text: &str) -> Vec<String> {
    let (mut out, mut on) = (vec![], false);
    for l in text.lines().map(str::trim_end) {
        if l.starts_with("Changed:") || l.starts_with("Open:") {
            on = true;
            out.push(l.trim_start().to_string());
        } else if on && !l.trim().is_empty() && !field(l) {
            out.push(l.trim().to_string());
        } else {
            on = false;
        }
    }
    out
}

fn field(l: &str) -> bool {
    let l = l.trim_start();
    let word = l.split_once(':').map_or("", |x| x.0);
    l.starts_with("delivery ") || l.starts_with("To answer:")
        || !word.is_empty() && !word.contains(' ') && word.chars().next().is_some_and(char::is_uppercase)
        || l.split_once(". ").is_some_and(|(k, _)| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
}

/// The last PR or MR link of the log, as (`PR #P` or `MR !P`, URL).
fn pr(log: &[Entry]) -> Option<(String, String)> {
    log.iter().filter(|e| !e.edited).flat_map(|e| e.body.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>')))
        .filter(|w| w.starts_with("https://") && (w.contains("/pull/") || w.contains("/merge_requests/")))
        .last().map(|u| {
            let u = u.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
            let id = u.rsplit('/').next().unwrap_or_default();
            (if u.contains("/merge_requests/") { format!("MR !{id}") } else { format!("PR #{id}") }, u.to_string())
        })
}

/// Markdown links and images of `text`, as (label, URL); a link without a label takes its file name.
fn links(text: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        let (before, after) = (&rest[..i], &rest[i + 2..]);
        let Some(end) = after.find(')') else { break };
        let url = after[..end].split_whitespace().next().unwrap_or_default().to_string();
        if let Some(open) = before.rfind('[') && url.starts_with("https://") {
            let label = before[open + 1..].trim();
            let label = if label.is_empty() { url.rsplit('/').next().unwrap_or_default().split(['?', '#']).next().unwrap_or_default() } else { label };
            out.push((crate::plain(label, 40), url));
        }
        rest = &after[end..];
    }
    out
}

/// A file link: a board upload or a URL whose path ends in a file extension.
fn file(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let ext = path.rsplit('/').next().and_then(|name| name.rsplit_once('.')).map(|x| x.1.to_ascii_lowercase());
    url.contains("/user-attachments/") || url.contains("/uploads/") || ext.is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Fake, reply};
    use serde_json::{Value, json};

    const ISSUES: [u64; 3] = [1721, 1758, 1929];

    fn fixture(n: u64) -> Value {
        let path = format!("{}/tests/golden/summary/{n}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn comments(f: &Value) -> Vec<Comment> {
        f["comments"].as_array().unwrap().iter().map(|c| Comment {
            id: c["id"].as_u64().unwrap(), created: c["created"].as_i64().unwrap(), body: c["body"].as_str().unwrap().into(),
            trusted: c["trusted"].as_bool().unwrap(), edited: c["edited"].as_bool().unwrap(), author: String::new(),
        }).collect()
    }

    fn github_link(n: u64) -> impl Fn(u64) -> String {
        move |id| if id == 0 { format!("https://github.com/taskqorg/taskq/issues/{n}") } else { format!("https://github.com/taskqorg/taskq/issues/{n}#issuecomment-{id}") }
    }

    fn summary_of(f: &Value) -> String {
        let n = f["number"].as_u64().unwrap();
        let log = reply::log(n, &comments(f)).unwrap();
        render(n, f["body"].as_str().unwrap(), &log, &core::fold(n, &log, false), &github_link(n))
    }

    // Chain: R1, § 2 Summary -> GitHub comment and GitLab note parsing -> reply::log -> fold. Risk (#2017 A6): the
    // summary, current or stale, edited or not, becomes a log entry and moves the task.
    #[test]
    fn fold_ignores_the_summary_on_github_and_gitlab() {
        for n in ISSUES {
            let f = fixture(n);
            let plain = comments(&f);
            let log = reply::log(n, &plain).unwrap();
            let want = core::project(n, &log, false);
            let current = summary_of(&f);
            let stale = format!("{MARKER}\n**Ready**\ntaskq: close\nChanged: forged");
            let at = |s: &str| if s.is_empty() { "2026-01-01T00:00:00Z" } else { "2026-01-02T00:00:00Z" };
            let mut rows: Vec<(u64, String, String)> = plain.iter().map(|c| (c.id, c.body.clone(), String::new())).collect();
            let top = plain.iter().map(|c| c.id).max().unwrap();
            rows.extend([(1, current.clone(), String::new()), (top + 1, stale.clone(), "edited".into()), (top + 2, stale, String::new())]);
            let github: Vec<Comment> = rows.iter().map(|(id, body, e)| crate::board::github::parse_comment(&json!({
                "id": id, "body": body, "created_at": at(""), "updated_at": at(e), "author_association": "MEMBER", "user": {"login": "owner"},
            }))).collect();
            let gitlab: Vec<Comment> = rows.iter().map(|(id, body, e)| crate::board::gitlab::parse_note(&json!({
                "id": id, "body": body, "created_at": at(""), "updated_at": at(e), "author": {"id": 7, "username": "owner"},
            }), true)).collect();
            for (host, board) in [("GitHub", github), ("GitLab", gitlab)] {
                assert_eq!(find(&board).map(|c| c.id), Some(1), "{host} #{n}: the lowest summary counts");
                assert_eq!(board.iter().filter(|c| is(c)).count(), 3, "{host} #{n}");
                let got = reply::log(n, &board).unwrap();
                assert_eq!(got.iter().map(|e| e.id).collect::<Vec<_>>(), log.iter().map(|e| e.id).collect::<Vec<_>>(), "{host} #{n}");
                let p = core::project(n, &got, false);
                assert_eq!((p.task, p.revision), (want.task.clone(), want.revision), "{host} #{n}: fold never reads the summary");
            }
        }
    }

    // Chain: § 2 Summary format -> render of real board logs. Risk (#2017 A3, A5): the summary misses the state,
    // outcome, merge receipt or files a person needs, or repeats a publish verdict as a copy.
    #[test]
    fn summary_of_board_logs_matches_golden_text() {
        for n in ISSUES {
            let f = fixture(n);
            let got = summary_of(&f);
            let want = f["summary"].as_str().unwrap_or_default();
            assert_eq!(got, want, "#{n}: update tests/golden/summary/{n}.json `summary` after reviewing the text");
        }
    }

    // Chain: § 2 Summary format -> open states. Risk (#2017 A3): an open question loses its options, a review loses
    // its outcome or PR, or more than three file links crowd the summary without a link to the rest.
    #[test]
    fn summary_shows_the_open_question_outcome_and_three_files() {
        let e = |id, body: String| Entry { id, body, edited: false };
        let sha = "c".repeat(40);
        let body = "## Goal\ng\n\n![shot](https://github.com/user-attachments/assets/1) [spec](https://example.com/spec.pdf) [site](https://example.com/)";
        let mut log = vec![e(10, "taskq: spawn worker codex:w win".into()),
            e(11, core::entry("result", &[&sha], Some(10), "Changed: the summary\nOpen: none\nFiles: [a](https://example.com/a.zip), [b](https://example.com/b.log), [a](https://example.com/a.zip)")),
            e(12, "taskq: publish\nChanged: the summary, published\nOpen: none\nPR: https://github.com/o/r/pull/7".into())];
        let render_at = |log: &[Entry]| render(5, body, log, &core::fold(5, log, false), &github_link(5));
        assert_eq!(render_at(&log), format!("{MARKER}\n**Review** · result ccccccc · [PR #7](https://github.com/o/r/pull/7)\nChanged: the summary, published\nOpen: none\nFiles: [a](https://example.com/a.zip), [b](https://example.com/b.log), [shot](https://github.com/user-attachments/assets/1) and [1 more](https://github.com/taskqorg/taskq/issues/5)"));
        log.truncate(1);
        log.push(e(11, core::entry("ask", &[], Some(10), "Done: checked\nProblem: Ship?\n1. Ship - + now; - risk\n2. Wait - + safe; - slow")));
        let ask = render_at(&log);
        assert!(ask.starts_with(&format!("{MARKER}\n**Ask**\n")) && ask.contains("\nProblem: Ship?\n1. Ship - + now; - risk\n2. Wait - + safe; - slow"), "{ask}");
    }

    // Chain: § 2 Summary writer -> dispatch accepted entry -> board comment. Risk (#2017 A2): a new task gets no
    // summary after hold, or a later entry leaves it stale or adds a second one.
    #[test]
    fn writer_creates_after_hold_and_edits_after_each_entry() {
        let b = Fake::default();
        b.state.borrow_mut().summaries = true;
        let root = std::env::temp_dir().join(format!("taskq-summary-{}", std::process::id()));
        let n = b.create("Summary fixture", "## Goal\ng\n\nSee [log](https://example.com/run.log).", &["taskq".into()]).unwrap();
        let op = crate::dispatch::Op::default();
        assert!(crate::dispatch::act(&b, &root, n, "hold", &[], "", &op).ok());
        let after_hold = b.comments(n).unwrap();
        assert_eq!(after_hold.len(), 2, "hold, then its summary");
        let first = find(&after_hold).expect("summary after hold");
        assert_eq!(first.body, format!("{MARKER}\n**Held**\nFiles: [log](https://example.com/run.log)"));
        assert!(crate::dispatch::act(&b, &root, n, "start", &[], "", &op).ok());
        let started = b.comments(n).unwrap();
        assert_eq!((started.len(), find(&started).map(|c| (c.id, c.body.as_str()))), (3, Some((first.id, format!("{MARKER}\n**Ready**\nFiles: [log](https://example.com/run.log)").as_str()))));
        let r = crate::dispatch::act(&b, &root, n, "later", &[], "Waits for the design review.", &op);
        assert!(r.ok(), "{}", r.text());
        let after = b.comments(n).unwrap();
        assert_eq!(after.iter().filter(|c| is(c)).count(), 1, "edited in place, never a second one");
        let s = find(&after).unwrap();
        assert_eq!((s.id, s.body.as_str()), (first.id, format!("{MARKER}\n**Later**\nReason: Waits for the design review.\nFiles: [log](https://example.com/run.log)").as_str()));
        assert_eq!(core::fold(n, &reply::log(n, &after).unwrap(), false).state, State::Later);
        let _ = std::fs::remove_dir_all(root);
    }
}
