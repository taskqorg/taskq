//! § 5 human replies: board-sized transport, one final command, no input length cap.
use super::{Board, Comment, Result};
use crate::core::{self, Entry};

const PART: usize = 60_000; // UTF-8 bytes, leaving room below GitHub's comment limit for the envelope.
pub const MARKER: &str = "<!-- taskq-reply ";
const END: &str = "\n<!-- end taskq-reply-part -->";

fn digest(body: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, body.as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn reference(n: u64, body: &str, hash: &str, count: usize) -> String {
    let head = body.lines().next().unwrap_or_default();
    let fields = if core::op_id(n, body).is_some() { &body[body.rfind("\nop-id:").unwrap()..] } else { "" };
    format!("{head}\nLong reply: {count} ordered comments; read all parts with taskq inbox {n}.\n{MARKER}{hash} {count} -->{fields}")
}

/// Post inert, content-addressed parts before the ordinary command. A lost response never authorizes a retry POST.
pub fn post(b: &dyn Board, n: u64, body: &str) -> Result<u64> {
    if body.len() <= PART { return b.comment(n, body); }
    let hash = digest(body);
    let mut rest = body;
    let mut parts = Vec::new();
    while !rest.is_empty() {
        let mut end = rest.len().min(PART);
        while !rest.is_char_boundary(end) { end -= 1; }
        // Prefer a board line break, preserving that break as part of the content.
        if end < rest.len() && let Some(i) = rest[..end].rfind('\n') { end = i + 1; }
        parts.push(&rest[..end]);
        rest = &rest[end..];
    }
    let existing = b.comments(n)?;
    let mut expected = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let text = format!("taskq: reply-part {hash} {}/{}\n{part}{END}", i + 1, parts.len());
        let id = match existing.iter().find(|c| c.trusted && !c.edited && c.body == text) {
            Some(c) => c.id,
            None => b.comment(n, &text)?,
        };
        expected.push((id, text));
    }
    let readback = b.comments(n)?;
    if expected.iter().any(|(id, text)| !readback.iter().any(|c| c.id == *id && c.trusted && !c.edited && c.body == *text)) {
        return Err("reply parts not read back; check delivery with the same operation before sending again".into());
    }
    b.comment(n, &reference(n, body, &hash, parts.len()))
}

/// All board-to-model readers use the same assembly. An incomplete committed reply makes the read unknown.
pub fn log(n: u64, comments: &[Comment]) -> Result<Vec<Entry>> {
    comments.iter().filter(|c| c.trusted).map(|c| {
        let mut body = c.body.clone();
        let mut lines = body.lines();
        let command = lines.next().and_then(|l| l.strip_prefix("taskq: ")).and_then(|l| l.split_whitespace().next());
        let notice = lines.next().unwrap_or_default();
        let marker = lines.next().unwrap_or_default();
        if !c.edited && matches!(command, Some("answer" | "steer" | "requeue")) && notice.starts_with("Long reply: ") && marker.starts_with("<!-- taskq-reply") {
            let bad = || format!("reply at comment {} is incomplete or changed; inspect taskq inbox {n} and the board", c.id);
            let words: Vec<_> = marker.strip_prefix(MARKER).and_then(|m| m.strip_suffix(" -->")).ok_or_else(bad)?.split_whitespace().collect();
            if words.len() != 2 { return Err(bad()); }
            let (hash, count) = (words[0], words[1].parse::<usize>().map_err(|_| bad())?);
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || count == 0 || count > comments.len() { return Err(bad()); }
            let mut full = String::new();
            for i in 1..=count {
                let head = format!("taskq: reply-part {hash} {i}/{count}\n");
                let mut matches = comments.iter().filter(|p| p.trusted && !p.edited && p.id < c.id)
                    .filter_map(|p| p.body.strip_prefix(&head)?.strip_suffix(END));
                let part = matches.next().ok_or_else(bad)?;
                if matches.any(|other| other != part) { return Err(bad()); }
                full.push_str(part);
            }
            if digest(&full) != hash || reference(n, &full, hash, count) != body { return Err(bad()); }
            body = full;
        }
        Ok(Entry { id: c.id, body, edited: c.edited })
    }).collect()
}
