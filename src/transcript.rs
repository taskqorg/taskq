//! D1/D4: the native transcript is the only dialogue record. TaskQ reads it backwards in bounded chunks by byte
//! cursor and keeps no copy, index or summary; a missing or unreadable file is unknown, never empty.
use serde_json::{json, Value};
use std::{fs, io::{Read, Seek, SeekFrom}, path::Path};
type Res<T> = Result<T, String>;

/// One view read; a record longer than this doubles the read up to `RECORD`.
pub const CHUNK: u64 = 256 * 1024;
const RECORD: u64 = 16 * 1024 * 1024;

/// Complete records between two byte offsets of the transcript: `start` is the cursor for the previous chunk.
#[derive(Debug)]
pub struct Page { pub items: Vec<Value>, pub start: u64, pub end: u64 }

fn range(path: &Path, start: u64, end: u64) -> Res<Vec<u8>> {
    let mut f = fs::File::open(path).map_err(|e| format!("transcript unreadable: {e}"))?;
    f.seek(SeekFrom::Start(start)).map_err(|e| format!("transcript unreadable: {e}"))?;
    let mut bytes = vec![];
    f.take(end - start).read_to_end(&mut bytes).map_err(|e| format!("transcript unreadable: {e}"))?;
    Ok(bytes)
}

fn len(path: &Path) -> Res<u64> { fs::metadata(path).map(|m| m.len()).map_err(|e| format!("transcript unreadable: {e}")) }

/// The complete records that end at `before` (the file end when none): one chunk back, more only for a longer record.
pub fn before(codex: bool, path: &Path, before: Option<u64>) -> Res<Page> {
    let len = len(path)?;
    let end = before.unwrap_or(len);
    if end > len { return Err("transcript shrank; view unknown".into()); }
    let mut size = CHUNK;
    loop {
        let start = end.saturating_sub(size);
        let bytes = range(path, start, end)?;
        // a writer may be mid-line at the file end: only newline-terminated records count
        let last = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        let first = if start == 0 { Some(0) } else { bytes[..last].iter().position(|b| *b == b'\n').map(|i| i + 1).filter(|i| *i < last) };
        match first {
            Some(first) => return Ok(Page { items: items(codex, &bytes[first..last]), start: start + first as u64, end: start + last as u64 }),
            None if start > 0 && size < RECORD => size *= 2,
            None => return Err("transcript record exceeds 16 MiB; view unknown".into()),
        }
    }
}

/// Records appended after `from` (D4 polling while the view is open).
pub fn after(codex: bool, path: &Path, from: u64) -> Res<Page> {
    let len = len(path)?;
    if from > len { return Err("transcript shrank; view unknown".into()); }
    let bytes = range(path, from, len.min(from + RECORD))?;
    let last = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    Ok(Page { items: items(codex, &bytes[..last]), start: from, end: from + last as u64 })
}

/// User and assistant text in order, each tool call as one line; every other record is skipped (D4).
pub fn items(codex: bool, bytes: &[u8]) -> Vec<Value> {
    let mut out = vec![];
    for v in bytes.split(|b| *b == b'\n').filter_map(|l| serde_json::from_slice::<Value>(l).ok()) {
        if codex { codex_item(&v, &mut out) } else { claude_item(&v, &mut out) }
    }
    out
}

fn line(role: &str, text: &str) -> Value { json!({"role": role, "text": text}) }

fn tool(name: &str, input: &Value) -> Value {
    let arg = ["description", "command", "file_path", "pattern", "url", "cmd"].iter().find_map(|k| input[k].as_str().map(String::from))
        .unwrap_or_else(|| input.as_str().map_or_else(|| input.to_string(), String::from));
    let arg: String = arg.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
    line("tool", format!("{name} {arg}").trim())
}

fn claude_item(v: &Value, out: &mut Vec<Value>) {
    let role = v["type"].as_str().unwrap_or_default();
    if !["user", "assistant"].contains(&role) || v["isSidechain"] == true || v["isMeta"] == true { return; }
    let content = &v["message"]["content"];
    if let Some(text) = content.as_str() { return out.push(line(role, text)); }
    for b in content.as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => out.push(line(role, b["text"].as_str().unwrap_or_default())),
            Some("tool_use") => out.push(tool(b["name"].as_str().unwrap_or("tool"), &b["input"])),
            _ => {}
        }
    }
}

fn codex_item(v: &Value, out: &mut Vec<Value>) {
    if v["type"] != "response_item" { return; }
    let p = &v["payload"];
    match (p["type"].as_str(), p["role"].as_str()) {
        (Some("message"), Some(role @ ("user" | "assistant"))) => {
            let text: Vec<&str> = p["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect();
            let text = text.join("\n");
            // Codex sends its environment and AGENTS.md to the model as user input; the owner never wrote them
            if role == "user" && (text.starts_with('<') || text.starts_with("# AGENTS.md")) { return; }
            out.push(line(role, &text));
        }
        (Some("function_call" | "custom_tool_call"), _) => {
            let input = p["arguments"].as_str().or(p["input"].as_str()).map_or(Value::Null, |a| serde_json::from_str(a).unwrap_or_else(|_| json!(a)));
            out.push(tool(p["name"].as_str().unwrap_or("tool"), &input));
        }
        (Some("local_shell_call"), _) => out.push(tool("shell", &p["action"])),
        _ => {}
    }
}

/// D7: the last assistant line from the transcript's last chunk; none when that chunk has no assistant text.
pub fn last_line(codex: bool, path: &Path) -> Res<Option<String>> {
    let page = before(codex, path, None)?;
    Ok(page.items.iter().rev().find(|i| i["role"] == "assistant").and_then(|i| i["text"].as_str()?.lines().rev().find(|l| !l.trim().is_empty()).map(String::from)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, path::PathBuf};

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-transcript-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }

    // D-A3: a 10 MiB transcript opens in one chunk read, scrolls up to its first record and appends while open
    #[test]
    fn ten_mib_claude_transcript_pages_by_cursor() {
        let dir = tmp();
        let id = "0b4eaf42-5e87-4d57-9d31-c37c62bfbc2c";
        fs::create_dir(dir.join("projects")).unwrap();
        fs::create_dir(dir.join("projects/-p")).unwrap();
        let path = dir.join(format!("projects/-p/{id}.jsonl"));
        assert_eq!(crate::runtime::claude_transcript_in(&dir, id).unwrap_err(), "Claude transcript unavailable; completion unknown");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "{}", json!({"type":"user","message":{"role":"user","content":"first"}})).unwrap();
        let filler = "x".repeat(1000);
        let mut n = 1;
        while f.metadata().unwrap().len() < 10 * 1024 * 1024 {
            writeln!(f, "{}", json!({"type":"assistant","message":{"content":[{"type":"text","text":format!("{n} {filler}")},{"type":"thinking","thinking":"hidden"}]}})).unwrap();
            writeln!(f, "{}", json!({"type":"cost-state","n":n})).unwrap();
            n += 1;
        }
        writeln!(f, "{}", json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"cargo  test\n--locked"}}]}})).unwrap();
        write!(f, "{{\"type\":\"user\",\"mess").unwrap();
        drop(f);
        assert_eq!(crate::runtime::claude_transcript_in(&dir, id).unwrap(), path);
        let open = before(false, &path, None).unwrap();
        assert!(open.start > 0 && open.end - open.start <= CHUNK, "one chunk: {open:?}");
        assert_eq!(open.items.last().unwrap(), &json!({"role":"tool","text":"Bash cargo test --locked"}));
        assert!(open.items.iter().all(|i| i["role"] != "user"), "unknown and partial records are skipped");
        let (mut cursor, mut pages, mut first) = (open.start, 1, None);
        while cursor > 0 {
            let page = before(false, &path, Some(cursor)).unwrap();
            assert!(page.end == cursor && page.start < cursor);
            cursor = page.start;
            first = page.items.first().cloned();
            pages += 1;
        }
        assert_eq!(first, Some(json!({"role":"user","text":"first"})));
        assert!(pages >= 40);
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "age\":{{\"content\":\"next\"}}}}\n").unwrap();
        let new = after(false, &path, open.end).unwrap();
        assert_eq!(new.items, vec![json!({"role":"user","text":"next"})]);
        assert_eq!(new.end, fs::metadata(&path).unwrap().len());
        assert_eq!(last_line(false, &path).unwrap(), Some(format!("{} {filler}", n - 1)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_rollout_is_found_and_read() {
        let dir = tmp();
        let id = "01a1624d-88a8-76f0-a9df-9e47cd23b786";
        assert!(crate::runtime::owner::codex_transcript_in(&dir, id).unwrap_err().starts_with("Codex transcript unavailable"), "missing is unknown, never empty");
        fs::create_dir_all(dir.join("sessions/2026/10/22")).unwrap();
        let path = dir.join(format!("sessions/2026/10/22/rollout-2026-10-22T13-30-11-{id}.jsonl"));
        let lines = [
            json!({"type":"session_meta","payload":{"id":id}}),
            json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"rules"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done\nall good"}]}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
        ];
        fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        assert_eq!(crate::runtime::owner::codex_transcript_in(&dir, id).unwrap(), path);
        assert_eq!(before(true, &path, None).unwrap().items, vec![json!({"role":"user","text":"hello"}), json!({"role":"tool","text":"exec_command ls"}), json!({"role":"assistant","text":"done\nall good"})]);
        assert_eq!(last_line(true, &path).unwrap().as_deref(), Some("all good"));
        fs::remove_dir_all(dir).unwrap();
    }
}
