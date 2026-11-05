//! § 2 Summary: one trusted comment per task that projects the fold for people, like a label. `fold` never reads it.
use crate::{
    Res,
    board::{Board, Comment},
    core::{self, Entry, State, Task},
    deploy::Deploy,
};

pub const MARKER: &str = "<!-- taskq-summary -->";
/// File links shown before `and N more`.
const FILES: usize = 3;
const EXTENSIONS: [&str; 19] = [
    "zip", "gz", "tgz", "pdf", "html", "htm", "png", "jpg", "jpeg", "gif", "webp", "svg", "mp4",
    "mov", "webm", "txt", "log", "csv", "json",
];

/// A summary comment: trusted, its first line the marker.
pub fn is(c: &Comment) -> bool {
    c.trusted && c.body.lines().next().is_some_and(|l| l.trim() == MARKER)
}

/// The summary that counts: the lowest id of several.
pub fn find(comments: &[Comment]) -> Option<&Comment> {
    comments.iter().filter(|c| is(c)).min_by_key(|c| c.id)
}

/// The summary text of task `n`: `body` is the issue text, `t` its fold, `link` a comment's URL (0: the issue),
/// `deploy` the project's § 9 setting that adds a closed task's deploy lines.
pub fn render(
    n: u64,
    body: &str,
    log: &[Entry],
    t: &Task,
    link: &dyn Fn(u64) -> String,
    deploy: Option<&Deploy>,
) -> String {
    let p = core::project(n, log, false);
    let mut accepted: Vec<&Entry> = log.iter().filter(|e| p.accepts(e.id)).collect();
    accepted.sort_by_key(|e| e.id);
    let last = |kinds: &[&str]| {
        accepted
            .iter()
            .rev()
            .find(|e| kinds.contains(&core::kind(n, &e.body)))
            .copied()
    };
    let mut head = vec![format!("**{}**", title(t.state.name()))];
    if let Some(r) = t.result.as_deref().filter(|_| t.state != State::Closed) {
        head.push(format!("result {}", &r[..r.len().min(7)]));
    }
    let merged = (t.state == State::Closed)
        .then(|| crate::cli::inbox::merge_receipt(log))
        .flatten();
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
        State::Ask => lines.extend(
            t.card
                .iter()
                .flat_map(|c| c.lines())
                .map(|l| l.trim_end().to_string())
                .filter(|l| !l.trim().is_empty()),
        ),
        State::Review => {
            let text = t
                .publication
                .clone()
                .or_else(|| last(&["result"]).map(|e| core::entry_text(n, &e.body).to_string()));
            lines.extend(outcome(text.as_deref().unwrap_or("")));
        }
        State::Closed => {
            let text = last(&["close"])
                .map(|e| core::verdict(n, log, core::entry_text(n, &e.body)))
                .unwrap_or_default();
            lines.extend(outcome(&text));
            lines.extend(
                deploy
                    .map(|d| crate::deploy::lines(n, log, d))
                    .unwrap_or_default(),
            );
        }
        State::Later | State::Failed => {
            if let Some(reason) = last(&["later", "failed"]).and_then(|e| {
                core::entry_text(n, &e.body)
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
            }) {
                lines.push(format!(
                    "Reason: {}",
                    reason
                        .strip_prefix("Reason:")
                        .map_or(reason, str::trim_start)
                ));
            }
        }
        _ => {}
    }
    // § 2: newest outcome first, the issue text last; one link per URL
    let mut files: Vec<(u64, String, String)> = vec![];
    let sources = accepted
        .iter()
        .rev()
        .filter(|e| {
            matches!(
                core::kind(n, &e.body),
                "result" | "ask" | "publish" | "close"
            )
        })
        .map(|e| (e.id, core::entry_text(n, &e.body).to_string()))
        .chain(std::iter::once((0, body.to_string())));
    for (id, text) in sources {
        for (label, url) in links(&text).into_iter().filter(|(_, u)| file(u)) {
            if !files.iter().any(|f| f.2 == url) {
                files.push((id, label, url));
            }
        }
    }
    if !files.is_empty() {
        let shown: Vec<String> = files
            .iter()
            .take(FILES)
            .map(|(_, label, url)| format!("[{label}]({url})"))
            .collect();
        let mut line = format!("Files: {}", shown.join(", "));
        if let Some((id, _, _)) = files.get(FILES) {
            line.push_str(&format!(
                " and [{} more]({})",
                files.len() - FILES,
                link(*id)
            ));
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// The current summary of task `n` on board `b`.
pub fn text(
    b: &dyn Board,
    n: u64,
    body: &str,
    log: &[Entry],
    t: &Task,
    deploy: Option<&Deploy>,
) -> String {
    render(
        n,
        body,
        log,
        t,
        &|id| {
            if id == 0 {
                b.url(n)
            } else {
                b.comment_url(n, id)
            }
        },
        deploy,
    )
}

/// Edits the summary of task `n` to its current text, or creates it when missing. Returns whether it wrote.
pub fn write(
    b: &dyn Board,
    n: u64,
    body: &str,
    comments: &[Comment],
    log: &[Entry],
    t: &Task,
    deploy: Option<&Deploy>,
) -> Res<bool> {
    put(b, n, comments, &text(b, n, body, log, t, deploy))
}

/// Writes `want` unless the summary already reads so.
pub fn put(b: &dyn Board, n: u64, comments: &[Comment], want: &str) -> Res<bool> {
    match find(comments) {
        Some(c) if same(&c.body, want) => Ok(false),
        Some(c) => b
            .edit_comment(n, c.id, want)
            .map(|_| true)
            .map_err(String::from),
        None => b.comment(n, want).map(|_| true).map_err(String::from),
    }
}

/// Board text compared without line-ending or trailing-space differences, so a read back never edits again.
pub fn same(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n").trim_end() == b.trim_end()
}

fn title(state: &str) -> String {
    let mut c = state.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
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
    l.starts_with("delivery ")
        || l.starts_with("To answer:")
        || !word.is_empty()
            && !word.contains(' ')
            && word.chars().next().is_some_and(char::is_uppercase)
        || l.split_once(". ")
            .is_some_and(|(k, _)| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
}

/// The last PR or MR link of the log, as (`PR #P` or `MR !P`, URL).
fn pr(log: &[Entry]) -> Option<(String, String)> {
    log.iter()
        .filter(|e| !e.edited)
        .flat_map(|e| {
            e.body.split(|c: char| {
                c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>')
            })
        })
        .filter(|w| {
            w.starts_with("https://") && (w.contains("/pull/") || w.contains("/merge_requests/"))
        })
        .last()
        .map(|u| {
            let u = u.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
            let id = u.rsplit('/').next().unwrap_or_default();
            (
                if u.contains("/merge_requests/") {
                    format!("MR !{id}")
                } else {
                    format!("PR #{id}")
                },
                u.to_string(),
            )
        })
}

/// Markdown links and images of `text`, as (label, URL); a link without a label takes its file name.
fn links(text: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        let (before, after) = (&rest[..i], &rest[i + 2..]);
        let Some(end) = after.find(')') else { break };
        let url = after[..end]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if let Some(open) = before.rfind('[')
            && url.starts_with("https://")
        {
            let label = before[open + 1..].trim();
            let label = if label.is_empty() {
                url.rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .split(['?', '#'])
                    .next()
                    .unwrap_or_default()
            } else {
                label
            };
            out.push((crate::cli::text::plain(label, 40), url));
        }
        rest = &after[end..];
    }
    out
}

/// A file link: a board upload or a URL whose path ends in a file extension.
fn file(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|x| x.1.to_ascii_lowercase());
    url.contains("/user-attachments/")
        || url.contains("/uploads/")
        || ext.is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
