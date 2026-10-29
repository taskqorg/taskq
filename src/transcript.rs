//! D1/D4: the native transcript is the only dialogue record. TaskQ reads it in bounded chunks by byte cursor and
//! keeps no copy, index or summary; a missing or unreadable file is unknown, never empty. Each view item carries its
//! record identity: `at`, the byte offset where its record starts, and `i`, its block ordinal in that record.
use serde_json::{json, Value};
use std::{fs, io::{Read, Seek, SeekFrom}, path::Path};
type Res<T> = Result<T, String>;

/// One view read; a record longer than this doubles the read up to `RECORD`.
pub const CHUNK: u64 = 256 * 1024;
/// At most this many view items per read; a page always ends at a record boundary, so one record is never split.
pub const ITEMS: usize = 200;
const RECORD: u64 = 16 * 1024 * 1024;

/// Complete records between two byte offsets of the transcript: `start` and `end` are record boundaries and the
/// cursors for the previous and next read; `size` is the file length at the read.
#[derive(Debug)]
pub struct Page { pub items: Vec<Value>, pub start: u64, pub end: u64, pub size: u64 }

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
    let size = len(path)?;
    let end = before.unwrap_or(size);
    if end > size { return Err("transcript shrank; view unknown".into()); }
    let mut read = CHUNK;
    loop {
        let from = end.saturating_sub(read);
        let bytes = range(path, from, end)?;
        // a writer may be mid-line at the file end: only newline-terminated records count
        let last = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        let first = if from == 0 { Some(0) } else { bytes[..last].iter().position(|b| *b == b'\n').map(|i| i + 1).filter(|i| *i < last) };
        match first {
            Some(first) => {
                let mut records = records(codex, from + first as u64, &bytes[first..last]);
                let mut kept = 0;
                // the newest records up to ITEMS items; the page then starts at the first kept record
                let start = match records.iter().rposition(|(_, items)| { kept += items.len(); kept > ITEMS }) {
                    Some(i) => { let n = records.len(); records.drain(..(i + 1).min(n - 1)); records[0].0 }
                    None => from + first as u64,
                };
                return Ok(Page { items: records.into_iter().flat_map(|(_, items)| items).collect(), start, end: from + last as u64, size });
            }
            None if from > 0 && read < RECORD => read *= 2,
            None => return Err("transcript record exceeds 16 MiB; view unknown".into()),
        }
    }
}

/// The complete records that start at `from`: D4 polling while the view is open, and forward reads of an evicted
/// range. One chunk, more only for a longer record; a partial record at the file end waits for its newline.
pub fn after(codex: bool, path: &Path, from: u64) -> Res<Page> {
    let size = len(path)?;
    if from > size { return Err("transcript shrank; view unknown".into()); }
    let mut read = CHUNK;
    loop {
        let end = size.min(from + read);
        let bytes = range(path, from, end)?;
        match bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1) {
            Some(last) => {
                let mut records = records(codex, from, &bytes[..last]);
                let mut kept = 0;
                // the oldest records up to ITEMS items; the page then ends where the first left-out record starts
                let end = match records.iter().position(|(_, items)| { kept += items.len(); kept > ITEMS }).map(|i| i.max(1)) {
                    Some(i) if i < records.len() => { let at = records[i].0; records.truncate(i); at }
                    _ => from + last as u64,
                };
                return Ok(Page { items: records.into_iter().flat_map(|(_, items)| items).collect(), start: from, end, size });
            }
            None if end == size => return Ok(Page { items: vec![], start: from, end: from, size }),
            None if read < RECORD => read *= 2,
            None => return Err("transcript record exceeds 16 MiB; view unknown".into()),
        }
    }
}

/// Each record that starts at `base` within `bytes` with its view items: user and assistant text in order, each tool
/// call as one line (D4). Records without view items, unknown types and unparsable lines are left out.
fn records(codex: bool, base: u64, bytes: &[u8]) -> Vec<(u64, Vec<Value>)> {
    let mut out = vec![];
    let mut at = base;
    for l in bytes.split_inclusive(|b| *b == b'\n') {
        let mut items = vec![];
        if let Ok(v) = serde_json::from_slice::<Value>(l) {
            if codex { codex_item(&v, &mut items) } else { claude_item(&v, &mut items) }
            if v["timestamp"].is_string() {for item in &mut items {item["updated"]=v["timestamp"].clone();}}
        }
        if !items.is_empty() {
            for (i, item) in items.iter_mut().enumerate() { item["at"] = json!(at); item["i"] = json!(i); }
            out.push((at, items));
        }
        at += l.len() as u64;
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

/// CS2: timestamp on the newest visible native record; a missing timestamp stays unknown.
pub fn last_update(codex:bool,path:&Path)->Res<Value> {
    Ok(before(codex,path,None)?.items.last().map_or(Value::Null,|i|i["updated"].clone()))
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
    /// The view text of items, without their record identity.
    fn text(items: &[Value]) -> Vec<Value> { items.iter().map(|i| json!({"role": i["role"], "text": i["text"]})).collect() }
    fn key(i: &Value) -> (u64, u64) { (i["at"].as_u64().unwrap(), i["i"].as_u64().unwrap()) }

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
        assert_eq!(text(&open.items).last().unwrap(), &json!({"role":"tool","text":"Bash cargo test --locked"}));
        assert!(open.items.iter().all(|i| i["role"] != "user"), "unknown and partial records are skipped");
        let (mut cursor, mut pages, mut first) = (open.start, 1, None);
        while cursor > 0 {
            let page = before(false, &path, Some(cursor)).unwrap();
            assert!(page.end == cursor && page.start < cursor);
            cursor = page.start;
            first = text(&page.items).first().cloned();
            pages += 1;
        }
        assert_eq!(first, Some(json!({"role":"user","text":"first"})));
        assert!(pages >= 40);
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "age\":{{\"content\":\"next\"}}}}\n").unwrap();
        let new = after(false, &path, open.end).unwrap();
        assert_eq!(text(&new.items), vec![json!({"role":"user","text":"next"})]);
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
        assert_eq!(text(&before(true, &path, None).unwrap().items), vec![json!({"role":"user","text":"hello"}), json!({"role":"tool","text":"exec_command ls"}), json!({"role":"assistant","text":"done\nall good"})]);
        assert_eq!(last_line(true, &path).unwrap().as_deref(), Some("all good"));
        fs::remove_dir_all(dir).unwrap();
    }

    /// Writes invented records in the native format and returns the view text they must produce, in file order.
    /// `ignored` bytes of records without view items come before every `every`th record.
    fn transcript(codex: bool, path: &Path, records: usize, long: usize, every: usize, ignored: usize) -> Vec<Value> {
        let mut f = std::io::BufWriter::new(fs::File::create(path).unwrap());
        let (mut seed, mut want) = (7u64, vec![]);
        let mut line = |v: Value, f: &mut std::io::BufWriter<fs::File>| writeln!(f, "{v}").unwrap();
        for n in 0..records {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // variable height: 1 to 40 lines, multibyte text, now and then one very long message
            let body = if n % 997 == 500 { "ü".repeat(long) } else { vec![format!("ä {n} ✓"); 1 + (seed >> 33) as usize % 40].join("\n") };
            let text = format!("{n} {body}");
            let role = if n % 2 == 0 { "user" } else { "assistant" };
            if n % every == every / 2 {
                let mut left = ignored as i64;
                while left > 0 {
                    let v = if codex { json!({"type":"event_msg","payload":{"type":"token_count","pad":"x".repeat(900)}}) } else { json!({"type":"cost-state","pad":"x".repeat(900)}) };
                    left -= v.to_string().len() as i64 + 1;
                    line(v, &mut f);
                }
            }
            match (codex, role) {
                (false, "user") => line(json!({"type":"user","message":{"role":"user","content":text}}), &mut f),
                // two blocks in one record: the block ordinal tells them apart
                (false, _) => {
                    line(json!({"type":"assistant","message":{"content":[{"type":"text","text":text},{"type":"thinking","thinking":"hidden"},{"type":"tool_use","name":"Bash","input":{"command":format!("echo {n}")}}]}}), &mut f);
                    want.push(json!({"role":role,"text":text}));
                    want.push(json!({"role":"tool","text":format!("Bash echo {n}")}));
                    continue;
                }
                (true, _) => line(json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":"input_text","text":text}]}}), &mut f),
            }
            want.push(json!({"role":role,"text":text}));
        }
        want
    }

    /// Backward pages from the file end: each page's view items in file order.
    fn back(codex: bool, path: &Path) -> Vec<Page> {
        let mut pages = vec![before(codex, path, None).unwrap()];
        while pages.last().unwrap().start > 0 { pages.push(before(codex, path, Some(pages.last().unwrap().start)).unwrap()); }
        pages
    }

    // #1742 Q-A5: 10,000 variable-height visible records of both native renderers. Defect: a bounded or out-of-order
    // read omits or repeats a record, splits one, or stops at a page of only ignored records. Risk kept: every record
    // is reached from either end with the same identity, at most ITEMS items per page.
    #[test]
    fn ten_thousand_records_traverse_both_ways_in_any_order() {
        let dir = tmp();
        for codex in [false, true] {
            let path = dir.join(format!("{codex}.jsonl"));
            let want = transcript(codex, &path, 10_000, 300_000, 1500, 600 * 1024);
            let pages = back(codex, &path);
            assert!(pages.iter().all(|p| p.items.len() <= ITEMS), "bounded pages");
            assert!(pages.iter().any(|p| p.items.is_empty() && p.start > 0), "a page of ignored records still leads earlier");
            for w in pages.windows(2) { assert_eq!(w[1].end, w[0].start, "pages meet at record boundaries"); }
            let backward: Vec<Value> = pages.iter().rev().flat_map(|p| p.items.clone()).collect();
            assert_eq!(text(&backward), want, "codex {codex}: backward reaches the first record without omission or duplicate");
            let keys: Vec<_> = backward.iter().map(key).collect();
            assert!(keys.windows(2).all(|k| k[0] < k[1]), "identity is unique and in file order");
            // forward from the start, as after an eviction, gives the same records and identities
            let (mut forward, mut from, size) = (vec![], 0, fs::metadata(&path).unwrap().len());
            while from < size { let p = after(codex, &path, from).unwrap(); assert!(p.items.len() <= ITEMS && p.end > from && p.size == size); forward.extend(p.items); from = p.end; }
            assert_eq!(forward, backward, "codex {codex}: forward and backward reads agree");
            // the same cursors read in another order give the same pages: a reply has no hidden state
            let mut order: Vec<usize> = (0..pages.len()).collect();
            order.sort_by_key(|i| (i * 7919) % pages.len());
            for i in order { let p = before(codex, &path, Some(pages[i].end)).unwrap(); assert_eq!((p.start, &p.items), (pages[i].start, &pages[i].items)); }
            for p in &pages[1..] { let a = after(codex, &path, p.start).unwrap(); assert_eq!(a.items.first(), p.items.first(), "a saved record cursor reloads its range"); }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // #1742 Q-A5: a 100 MiB source with long single messages still reaches its first record; a record over 16 MiB,
    // a shrunk file and a partial last line stay explicit. Risk kept: no silent truncation or empty view.
    #[test]
    fn hundred_mib_source_and_oversized_record_stay_explicit() {
        let dir = tmp();
        let path = dir.join("stress.jsonl");
        let started = std::time::Instant::now();
        let want = transcript(false, &path, 2_000, 2 * 1024 * 1024, 40, 2 * 1024 * 1024);
        let size = fs::metadata(&path).unwrap().len();
        assert!(size >= 100 * 1024 * 1024, "{size}");
        let pages = back(false, &path);
        assert_eq!(text(&pages.iter().rev().flat_map(|p| p.items.clone()).collect::<Vec<_>>()), want);
        eprintln!("100 MiB stress: {} MiB in {} backward pages, {:?} with writing", size >> 20, pages.len(), started.elapsed());
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "{{\"type\":\"user\",\"mess").unwrap();
        assert!(after(false, &path, size).unwrap().items.is_empty(), "a partial last line waits for its newline");
        assert_eq!(after(false, &path, size).unwrap().end, size);
        writeln!(f, "age\":{{\"content\":\"{}\"}}}}", "y".repeat(17 * 1024 * 1024)).unwrap();
        drop(f);
        assert_eq!(before(false, &path, None).unwrap_err(), "transcript record exceeds 16 MiB; view unknown");
        assert_eq!(after(false, &path, size).unwrap_err(), "transcript record exceeds 16 MiB; view unknown");
        assert_eq!(text(&before(false, &path, Some(size)).unwrap().items), text(&pages[0].items), "earlier records stay readable");
        fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(size / 2).unwrap();
        assert_eq!(before(false, &path, Some(size)).unwrap_err(), "transcript shrank; view unknown");
        assert_eq!(after(false, &path, size).unwrap_err(), "transcript shrank; view unknown");
        fs::remove_dir_all(dir).unwrap();
    }
    // CS2/R12: native record time, not read time or trailing progress/partial writes, orders conversations.
    #[test]
    fn sidebar_activity_uses_the_newest_visible_native_record() {
        let dir=std::env::temp_dir().join(format!("taskq-activity-{}",crate::runtime::uuid()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("transcript.jsonl");
        for codex in [false,true] {
            let record=|text:&str,time:&str|if codex {json!({"type":"response_item","timestamp":time,"payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}})}else{json!({"type":"assistant","timestamp":time,"message":{"content":[{"type":"text","text":text}]}})};
            let a=record("First","2026-10-29T01:00:00.123Z");let b=record("Second","2026-10-29T01:01:00.456Z");
            fs::write(&path,format!("{a}\n{b}\n{{\"type\":\"progress\",\"timestamp\":\"2026-10-29T03:00:00Z\"}}\npartial")).unwrap();
            assert_eq!(last_update(codex,&path).unwrap(),"2026-10-29T01:01:00.456Z");assert_eq!(last_update(codex,&path).unwrap(),"2026-10-29T01:01:00.456Z","reading again never changes activity");
            let mut missing=b.clone();missing.as_object_mut().unwrap().remove("timestamp");fs::write(&path,format!("{a}\n{missing}\n")).unwrap();assert!(last_update(codex,&path).unwrap().is_null(),"do not substitute an older known timestamp");
        }
        fs::remove_file(&path).unwrap();assert!(last_update(false,&path).is_err());fs::remove_dir_all(dir).unwrap();
    }

}
