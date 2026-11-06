//! D1/D4: the native transcript is the only dialogue record. AgentQ's `agent::history` adapter reads and normalizes
//! it in bounded pages (#2176, RUNTIME.md history wire contract); TaskQ keeps no copy, index or summary and owns only
//! the view: masking, image preview limits and roles. A missing or unreadable file is unknown, never empty. Each view
//! item carries its record identity: `at`, the byte offset where its record starts, and `i`, its ordinal in that record.
use agentq_runtime::agent::history::{self, History};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, io::Read, path::Path, sync::Mutex};
type Res<T> = Result<T, String>;

/// Complete records between two byte offsets of the transcript: `start` and `end` are record boundaries and the
/// cursors for the previous and next read; `size` is the file length at the read. AgentQ bounds a page to 128
/// records and 16 MiB. `capability` is AgentQ's history readback for the source format (A5).
#[derive(Debug)]
pub struct Page {
    pub generation: String,
    pub items: Vec<Value>,
    pub start: u64,
    pub end: u64,
    pub size: u64,
    pub capability: Value,
}

impl Page {
    pub fn json(&self) -> Value {
        json!({"generation":self.generation,"items":self.items,"start":self.start,"end":self.end,"size":self.size,"capability":self.capability})
    }
}

/// The AgentQ history format of a runtime's native transcript.
pub fn format(runtime: &str) -> &'static str {
    crate::runtime::of(runtime).history
}

/// A5: AgentQ's adapter status and native qualification for `format`, with the installed CLI version (null: unknown).
pub fn capability(format: &str) -> Value {
    let version = crate::runtime::history_version(format);
    match history::capabilities(format, version.as_deref(), std::env::consts::OS) {
        Ok(c) => {
            json!({"format":format,"version":version,"history":c["features"]["history"],"native":c["native"]})
        }
        Err(e) => {
            json!({"format":format,"version":version,"history":{"status":"unsupported","reason":e}})
        }
    }
}

/// Native file identity plus its immutable prefix; appends preserve it, replacement is never mixed into a window.
fn generation(path: &Path) -> Res<String> {
    let mut file = fs::File::open(path).map_err(|e| format!("transcript identity unknown: {e}"))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("transcript identity unknown: {e}"))?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    };
    #[cfg(windows)]
    let identity = {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx},
        };
        let mut id = FILE_ID_INFO::default();
        unsafe {
            GetFileInformationByHandleEx(
                HANDLE(file.as_raw_handle()),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        }
        .map_err(|e| format!("transcript identity unknown: {e}"))?;
        format!("{}:{:?}", id.VolumeSerialNumber, id.FileId.Identifier)
    };
    #[cfg(not(any(unix, windows)))]
    let identity = format!(
        "{:?}",
        meta.created()
            .map_err(|_| "transcript creation identity unavailable")?
    );
    let mut prefix = vec![];
    (&mut file)
        .take(meta.len().min(128))
        .read_to_end(&mut prefix)
        .map_err(|e| format!("transcript unreadable: {e}"))?;
    crate::profile::source_count("generation_read_bytes", prefix.len() as u64);
    let prefix = &prefix[..prefix
        .iter()
        .position(|b| *b == b'\n')
        .map_or(prefix.len(), |i| i + 1)];
    crate::profile::source_count("taskq_hash_bytes", prefix.len() as u64);
    // #2176: AgentQ item ordinals replace the TaskQ parser's; old saved positions must never bind to another item.
    Ok(format!(
        "history-v1:{identity}:{}",
        hex(ring::digest::digest(&ring::digest::SHA256, prefix).as_ref())
    ))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

fn len(path: &Path) -> Res<u64> {
    fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| format!("transcript unreadable: {e}"))
}

/// AgentQ file identities by source, learnt from its own checkpoints; bounded, a miss costs one tail read.
static IDENTITY: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static CHECKPOINTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

fn remember(h: &History) {
    let c = h.checkpoint();
    let mut saved = CHECKPOINTS.lock().unwrap_or_else(|e| e.into_inner());
    saved.retain(|v| {
        v["format"] != c["format"]
            || v["source"] != c["source"]
            || v["session"] != c["session"]
            || v["offset"] != c["offset"]
    });
    if saved.len() >= 64 {
        saved.remove(0);
    }
    saved.push(c.clone());
    drop(saved);
    if let (Some(source), Some(id)) = (c["source"].as_str(), c["identity"].as_str()) {
        let mut m = IDENTITY.lock().unwrap_or_else(|e| e.into_inner());
        let m = m.get_or_insert_with(HashMap::new);
        if m.len() >= 64 && !m.contains_key(source) {
            m.clear();
        }
        m.insert(source.into(), id.into());
    }
}

/// An AgentQ cursor at the record boundary `offset`. The checkpoint is AgentQ's: identity from its own readback,
/// prefix the SHA-256 of the bytes before `offset`; AgentQ verifies both and refuses a replaced or rewritten source.
// AgentQ still verifies the entire checkpoint prefix on open and hashes it again after polling.
// Reusing its exact checkpoint removes only TaskQ's duplicate synthesis, never either native integrity check.
fn at(format: &str, path: &Path, session: &str, offset: u64) -> Res<History> {
    let source = path.to_string_lossy();
    let h = History::new(format, &source, session)?;
    if offset == 0 {
        return Ok(h);
    }
    let reuse = true;
    #[cfg(test)]
    let reuse = !crate::profile::SOURCE_BASELINE.get();
    if reuse
        && let Some(saved) = CHECKPOINTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .rev()
            .find(|v| {
                v["format"] == format
                    && v["source"] == source.as_ref()
                    && v["session"] == session
                    && v["offset"] == offset
            })
            .cloned()
    {
        crate::profile::source_count("checkpoint_hits", 1);
        return History::recover(&saved);
    }
    let known = IDENTITY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(source.as_ref()).cloned());
    let identity = match known {
        Some(id) => id,
        None => {
            let mut fresh = h.clone();
            fresh.poll_before(path)?;
            remember(&fresh);
            fresh.checkpoint()["identity"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        }
    };
    let mut file = fs::File::open(path).map_err(|e| format!("unavailable: {e}"))?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let (mut left, mut buf) = (offset, vec![0; 65536]);
    while left > 0 {
        let n = file
            .read(&mut buf[..left.min(65536) as usize])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("truncated".into());
        }
        crate::profile::source_count("taskq_hash_bytes", n as u64);
        crate::profile::source_count("taskq_prefix_read_bytes", n as u64);
        ctx.update(&buf[..n]);
        left -= n as u64;
    }
    History::recover(
        &json!({"schema":1,"format":format,"source":source,"session":session,"offset":offset,"identity":identity,"prefix":hex(ctx.finish().as_ref())}),
    )
}

/// AgentQ errors read as TaskQ's view states; the reason stays exact.
fn unknown(e: String) -> String {
    if e.contains("exceeds 16 MiB") {
        return "transcript record exceeds 16 MiB; view unknown".into();
    }
    if ["replaced", "truncated", "rewritten"]
        .iter()
        .any(|w| e.contains(w))
    {
        return "transcript replaced during read; view unknown".into();
    }
    format!(
        "transcript {}; view unknown",
        e.trim_start_matches("history ")
    )
}

fn page(
    format: &str,
    path: &Path,
    generation: String,
    events: &[Value],
    start: u64,
    end: u64,
    size: u64,
) -> Res<Page> {
    if self::generation(path)? != generation {
        return Err("transcript replaced during read; view unknown".into());
    }
    let mut items = view(format, events);
    let (mut count, mut bytes) = (0, 0);
    for item in &mut items {
        if let Some(src) = item["image"]["src"].as_str() {
            count += 1;
            bytes += src.len();
            if count > 4 || bytes > 2 * 1024 * 1024 {
                item["image"].as_object_mut().unwrap().remove("src");
                item["image"]["reason"] =
                    json!("Image page preview limit; inspect the native source");
            }
        }
    }
    Ok(Page {
        generation,
        items,
        start,
        end,
        size,
        capability: capability(format),
    })
}

/// The complete records that end at `before` (the file end when none): one AgentQ backward page.
pub fn before(format: &str, session: &str, path: &Path, before: Option<u64>) -> Res<Page> {
    crate::profile::source_read(|| read_before(format, session, path, before))
}
fn read_before(format: &str, session: &str, path: &Path, before: Option<u64>) -> Res<Page> {
    crate::profile::source_count("native_polls", 1);
    let generation = generation(path)?;
    let size = len(path)?;
    if before.is_some_and(|b| b > size) {
        return Err("transcript shrank; view unknown".into());
    }
    if before == Some(0) {
        return page(format, path, generation, &[], 0, 0, size);
    }
    let mut h = match before {
        Some(b) => at(format, path, session, b),
        None => History::new(format, &path.to_string_lossy(), session),
    }
    .map_err(unknown)?;
    let p = h.poll_before(path).map_err(unknown)?;
    remember(&h);
    let start = h.checkpoint()["offset"].as_u64().unwrap_or(0);
    let end = before
        .or_else(|| p.events.last().and_then(|e| e["end"].as_u64()))
        .unwrap_or(start);
    page(format, path, generation, &p.events, start, end, size)
}

/// The complete records that start at `from`: D4 polling while the view is open, and forward reads of an evicted
/// range. A partial record at the file end waits for its newline.
pub fn after(format: &str, session: &str, path: &Path, from: u64) -> Res<Page> {
    crate::profile::source_read(|| read_after(format, session, path, from))
}
fn read_after(format: &str, session: &str, path: &Path, from: u64) -> Res<Page> {
    crate::profile::source_count("native_polls", 1);
    let generation = generation(path)?;
    let size = len(path)?;
    if from > size {
        return Err("transcript shrank; view unknown".into());
    }
    let mut h = at(format, path, session, from).map_err(unknown)?;
    let p = h.poll(path).map_err(unknown)?;
    remember(&h);
    let end = h.checkpoint()["offset"].as_u64().unwrap_or(from);
    page(format, path, generation, &p.events, from, end, size)
}

/// A2: AgentQ events as view items: user and assistant text and images in order, each tool call as one line (D4)
/// and each tool result as its own item at its record, its body opened only on request (owner answer on #2176: arguments stay omitted on worker views).
/// Lifecycle events are one compact line each, named by their native type; usage counters and unknown shapes stay
/// in the exact source only.
fn view(format: &str, events: &[Value]) -> Vec<Value> {
    let mut out = Vec::<Value>::new();
    let (mut model, mut effort, mut version) = (Value::Null, Value::Null, Value::Null);
    let (claude, codex) = (format.starts_with("claude-"), format.starts_with("codex-"));
    for e in events {
        let first = out.len();
        let raw: Value =
            serde_json::from_str(e["raw"].as_str().unwrap_or_default()).unwrap_or_default();
        if let Some(v) = raw["payload"]["cli_version"]
            .as_str()
            .or(raw["claude_code_version"].as_str())
        {
            version = json!(v);
        }
        // Only source-recorded metadata: a page without its earlier context keeps provenance unknown.
        if let Some(m) = raw["message"]["model"]
            .as_str()
            .or(raw["payload"]["model"].as_str())
            .or(raw["model"].as_str())
        {
            model = json!(m);
            effort = Value::Null;
        }
        if let Some(e) = raw["payload"]["effort"]
            .as_str()
            .or(raw["payload"]["reasoning_effort"].as_str())
            .or(raw["reasoning_effort"].as_str())
        {
            effort = json!(e);
        }
        let author = e["author"].as_str().unwrap_or("unknown");
        let blocks = e["blocks"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        match e["kind"].as_str().unwrap_or_default() {
            // agy-stream: step deltas are partial; the result record carries the complete response
            "message" if format == "agy-stream" => {}
            "message" if matches!(author, "user" | "assistant") => {
                if claude {
                    let raw: Value = serde_json::from_str(e["raw"].as_str().unwrap_or_default())
                        .unwrap_or_default();
                    if raw["isSidechain"] == true || raw["isMeta"] == true {
                        continue;
                    }
                }
                let joined: Vec<&str> = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
                // Codex sends its environment and AGENTS.md to the model as user input; the owner never wrote them
                if codex
                    && author == "user"
                    && joined
                        .first()
                        .is_some_and(|t| t.starts_with('<') || t.starts_with("# AGENTS.md"))
                {
                    continue;
                }
                let mut text = Vec::new();
                for b in blocks {
                    match b["type"].as_str().unwrap_or_default() {
                        "text" => {
                            if let Some(t) = b["text"].as_str() {
                                text.push(t);
                            }
                            continue;
                        }
                        "image" | "tool_call" | "tool_result" if !text.is_empty() => {
                            out.push(line(author, &text.join("\n")));
                            text.clear();
                        }
                        _ => {}
                    }
                    block(author, b, &mut out);
                }
                if !text.is_empty() {
                    out.push(line(author, &text.join("\n")));
                }
            }
            "tool" => {
                for b in blocks {
                    if b["type"] == "unknown" {
                        out.push(tool(
                            b["native"]["name"].as_str().unwrap_or("tool"),
                            &Value::Null,
                        ));
                    } else {
                        block("tool", b, &mut out);
                    }
                }
            }
            "result" if format == "agy-stream" => {
                let text: Vec<&str> = blocks.iter().filter_map(|b| b["text"].as_str()).collect();
                if !text.is_empty() {
                    out.push(line("assistant", &text.join("\n")));
                }
            }
            "lifecycle" => {
                if let Some(t) = lifecycle(e) {
                    out.push(line("event", &t));
                }
            }
            // TaskQ's own owner input record in the Antigravity stream: input intent, not native acceptance
            _ if format == "agy-stream" => {
                let raw: Value =
                    serde_json::from_str(e["raw"].as_str().unwrap_or_default()).unwrap_or_default();
                if raw["event"] == "user" {
                    if let Some(t) = raw["message"]["content"].as_str() {
                        out.push(json!({"role":"user","text":t,"delivery":"intent"}));
                    }
                }
            }
            _ => {}
        }
        for (i, item) in out[first..].iter_mut().enumerate() {
            item["at"] = e["start"].clone();
            item["i"] = json!(i);
            item["model"] = model.clone();
            item["effort"] = effort.clone();
            item["version"] = version.clone();
            if e["time"].is_string() {
                item["updated"] = e["time"].clone();
            }
        }
    }
    out
}

/// A2: the native name of a lifecycle event (`turn complete`, `system compact boundary`); usage counters give none.
fn lifecycle(e: &Value) -> Option<String> {
    let raw: Value = serde_json::from_str(e["raw"].as_str().unwrap_or_default()).ok()?;
    let p = &raw["payload"];
    let name = match e["format"].as_str().unwrap_or_default() {
        f if f.starts_with("claude-") => [raw["type"].as_str(), raw["subtype"].as_str()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
        "agy-stream" => raw["step_update"]["step_type"]
            .as_str()
            .or(raw["event"].as_str())?
            .into(),
        _ => p["type"]
            .as_str()
            .or(raw["type"].as_str())
            .or(raw["method"].as_str())?
            .into(),
    };
    if matches!(name.as_str(), "token_count" | "rate_limit_event" | "") {
        return None;
    }
    let name = name.replace("task_", "turn_").replace(['_', '.', '/'], " ");
    Some(if name == "init" {
        "session started".into()
    } else {
        name
    })
}

fn block(author: &str, b: &Value, out: &mut Vec<Value>) {
    match b["type"].as_str().unwrap_or_default() {
        "image" => out.push(image(author, &b["native"])),
        "tool_call" => {
            let name = b["name"].as_str().unwrap_or("tool");
            let raw = b["input"].as_str();
            // a custom tool's input is free text: an exec script or an apply_patch envelope, never shown as is
            let summary = raw
                .filter(|_| b["native"]["type"] == "custom_tool_call")
                .and_then(|a| {
                    if name == "apply_patch" {
                        Some(patch_files(a))
                    } else {
                        codex_script(a)
                    }
                });
            let input = match (summary, raw) {
                (Some(cmd), _) => json!({"cmd": cmd}),
                (None, Some(a)) => serde_json::from_str(a).unwrap_or_else(|_| json!(a)),
                _ => b["input"].clone(),
            };
            out.push(tool(name, &input));
        }
        "tool_result" => {
            let mut text = Vec::new();
            match &b["output"] {
                Value::String(s) => text.push(s.clone()),
                Value::Array(parts) => {
                    for p in parts {
                        if matches!(
                            p["type"].as_str(),
                            Some("image" | "input_image" | "image_url" | "output_image")
                        ) {
                            out.push(image("tool", p));
                        } else if let Some(t) = p["text"].as_str() {
                            text.push(t.into());
                        }
                    }
                }
                _ => {}
            }
            if text.is_empty() {
                return;
            }
            // tool output may echo credentials: the same masking as arguments, line by line
            let mut body = text
                .join("\n")
                .lines()
                .map(mask)
                .collect::<Vec<_>>()
                .join("\n");
            if body.len() > 32 * 1024 {
                let mut cut = 32 * 1024;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                body.push_str("\n[result cut at 32 KiB; inspect the native source]");
            }
            // the result stays at its own record: a call and its output may fall in different pages, and the view must
            // not depend on page boundaries (#2176 native Codex evidence)
            let mut item = line("tool", "result");
            item["tool_name"] = json!("result");
            item["result"] = json!(body);
            out.push(item);
        }
        _ => {}
    }
}
fn line(role: &str, text: &str) -> Value {
    json!({"role": role, "text": text})
}

fn tool(name: &str, input: &Value) -> Value {
    let arg = [
        "description",
        "command",
        "file_path",
        "pattern",
        "url",
        "cmd",
    ]
    .iter()
    .find_map(|k| input[k].as_str().map(String::from))
    .unwrap_or_else(|| {
        input
            .as_str()
            .map_or_else(|| input.to_string(), String::from)
    });
    let arg: String = mask(&arg).chars().take(160).collect();
    let mut item = line("tool", format!("{name} {arg}").trim());
    item["tool_name"] = json!(name);
    item
}

/// #1825 A4, #1833 A2: a tool line never shows a token-shaped word, the value of a credential assignment or JSON
/// field, or the credential of an Authorization header or bearer scheme; command output is never in the view.
fn mask(arg: &str) -> String {
    let (mut out, mut header, mut secret) = (vec![], false, false);
    for w in json_masked(arg).split_whitespace() {
        let bare = w.replace(['"', '\''], "").to_ascii_lowercase();
        let scheme = ["bearer", "basic", "token", "digest"].contains(&bare.as_str());
        out.push(if secret || header && !scheme {
            "[redacted]".into()
        } else {
            redact(w)
        });
        // `Authorization: <scheme> <credential>` or `Authorization: <credential>`; `Bearer <credential>` anywhere
        (secret, header) = (
            !secret && (header && scheme || bare == "bearer"),
            bare.ends_with("authorization:"),
        );
    }
    out.join(" ")
}

/// `"<name>": "<value>"`, also backslash-escaped as JSON inside a quoted command, with a credential-like name.
fn json_masked(s: &str) -> String {
    let (mut out, mut rest) = (String::new(), s);
    // every quote may open a key: a shell quote around the JSON never shifts the pairing
    while let Some(i) = rest.find('"') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        let Some(j) = rest.find('"') else { break };
        let quote = if rest[..j].ends_with('\\') {
            "\\\""
        } else {
            "\""
        };
        let name = rest[..j].trim_end_matches('\\').to_ascii_lowercase();
        let named = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
            && [
                "token",
                "secret",
                "password",
                "passwd",
                "api_key",
                "apikey",
                "authorization",
            ]
            .iter()
            .any(|n| name.contains(n));
        let value = rest[j + 1..]
            .trim_start()
            .strip_prefix(':')
            .map(str::trim_start)
            .and_then(|v| v.strip_prefix(quote))
            .filter(|_| named);
        let end = value.and_then(|v| {
            if quote == "\"" {
                v.match_indices('"')
                    .map(|(k, _)| k)
                    .find(|k| !v[..*k].ends_with('\\'))
            } else {
                v.find(quote)
            }
        });
        if let (Some(v), Some(k)) = (value, end) {
            out.push_str(&rest[..rest.len() - v.len()]);
            out.push_str("[redacted]");
            out.push_str(quote);
            rest = &v[k + quote.len()..];
        }
    }
    out + rest
}

/// #1833 A1: newer Codex runs commands through its `exec` tool as JavaScript; the line shows what the script calls,
/// `tools.exec_command({cmd})` as the command, other tools by name and target, never the script. None: no tool call.
fn codex_script(js: &str) -> Option<String> {
    let s: Vec<char> = js.chars().collect();
    let (mut i, mut calls, mut key) = (0, Vec::<(String, Option<String>)>::new(), None::<String>);
    let ident = |i: usize| {
        s[i..]
            .iter()
            .take_while(|c| c.is_alphanumeric() || **c == '_' || **c == '$')
            .collect::<String>()
    };
    let colon = |i: usize| s[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
    while i < s.len() {
        match s[i] {
            '/' if s.get(i + 1) == Some(&'/') => {
                while i < s.len() && s[i] != '\n' {
                    i += 1
                }
            }
            '/' if s.get(i + 1) == Some(&'*') => {
                i = (i + 3..s.len())
                    .find(|&k| s[k - 1] == '*' && s[k] == '/')
                    .map_or(s.len(), |k| k + 1)
            }
            q @ ('"' | '\'' | '`') => {
                let (lit, end) = literal(&s, i + 1, q);
                i = end;
                if colon(i) {
                    key = Some(lit);
                    continue;
                }
                if let Some((name, target @ None)) = calls.last_mut() {
                    if matches!(
                        (name.as_str(), key.as_deref()),
                        ("exec_command", Some("cmd"))
                            | ("view_image", Some("path"))
                            | ("apply_patch", None)
                    ) {
                        *target = Some(lit);
                    }
                }
                key = None;
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let word = ident(i);
                i += word.chars().count();
                if word == "tools" && s.get(i) == Some(&'.') {
                    let name = ident(i + 1);
                    i += 1 + name.chars().count();
                    if s.get(i) == Some(&'(') {
                        calls.push((name, None));
                        key = None;
                    }
                } else if colon(i) {
                    key = Some(word);
                }
            }
            _ => i += 1,
        }
    }
    let line: Vec<String> = calls
        .into_iter()
        .map(|(name, target)| match (name.as_str(), target) {
            ("exec_command", Some(cmd)) => cmd,
            ("apply_patch", Some(patch)) => format!("apply_patch {}", patch_files(&patch)),
            (_, Some(target)) => format!("{name} {target}"),
            (_, None) => name,
        })
        .collect();
    (!line.is_empty()).then(|| line.join(" · "))
}

/// The JavaScript string literal that starts at `i`, closed by `q`, and the index after it; an open one ends the script.
fn literal(s: &[char], mut i: usize, q: char) -> (String, usize) {
    let mut out = String::new();
    while i < s.len() && s[i] != q {
        if s[i] == '\\' && i + 1 < s.len() {
            i += 1;
            let hex = |n: usize| {
                s.get(i + 1..i + 1 + n)
                    .and_then(|h| u32::from_str_radix(&h.iter().collect::<String>(), 16).ok())
                    .and_then(char::from_u32)
            };
            match s[i] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' if hex(4).is_some() => {
                    out.push(hex(4).unwrap());
                    i += 4;
                }
                'x' if hex(2).is_some() => {
                    out.push(hex(2).unwrap());
                    i += 2;
                }
                c => out.push(c),
            }
        } else {
            out.push(s[i]);
        }
        i += 1;
    }
    (out, (i + 1).min(s.len()))
}

/// The files an `apply_patch` envelope adds, updates or deletes.
fn patch_files(patch: &str) -> String {
    patch
        .lines()
        .filter_map(|l| {
            ["*** Add File: ", "*** Update File: ", "*** Delete File: "]
                .iter()
                .find_map(|p| l.strip_prefix(p))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact(w: &str) -> String {
    let (name, value) = w.rsplit_once('=').unwrap_or(("", w));
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    let upper = name.to_ascii_uppercase();
    let secret = (!value.is_empty()
        && ["TOKEN", "SECRET", "PASSWORD", "API_KEY"]
            .iter()
            .any(|k| upper.ends_with(k)))
        || value.len() >= 20
            && [
                "ghp_",
                "gho_",
                "ghs_",
                "ghu_",
                "github_pat_",
                "glpat-",
                "sk-",
                "xoxb-",
                "xoxp-",
                "AKIA",
            ]
            .iter()
            .any(|p| value.starts_with(p));
    match (secret, name.is_empty()) {
        (false, _) => w.into(),
        (true, true) => "[redacted]".into(),
        (true, false) => format!("{name}=[redacted]"),
    }
}

/// Only recorded bytes authorize an inline image. Paths and remote URLs never trigger a file/network read.
fn image(role: &str, b: &Value) -> Value {
    use base64::Engine;
    let name = b["alt"]
        .as_str()
        .or(b["name"].as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Recorded image");
    // Native user-image wrappers also carry tool images; their human/board authorship is unknown.
    let mut item = line(if role == "user" { "image" } else { role }, "");
    item["native_role"] = json!(role);
    item["image"] = json!({"name":name.chars().take(160).collect::<String>()});
    let url = b["image_url"]
        .as_str()
        .or(b["image_url"]["url"].as_str())
        .or(b["source"]["url"].as_str());
    if let Some(url) =
        url.filter(|s| s.len() <= 4096 && (s.starts_with("https://") || s.starts_with("http://")))
    {
        item["image"]["url"] = json!(url);
    }
    let data = url
        .and_then(|s| s.strip_prefix("data:image/"))
        .and_then(|s| s.split_once(";base64,"))
        .map(|(_, data)| data)
        .or_else(|| {
            (b["source"]["type"] == "base64")
                .then(|| b["source"]["data"].as_str())
                .flatten()
        });
    let preview = (|| {
        let data =
            data.ok_or("Image bytes missing or access unavailable; no file or URL fetched")?;
        if data.len() > 2 * 1024 * 1024 {
            return Err("Image exceeds 2 MiB preview limit");
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| "Image encoding unreadable")?;
        let kind = crate::board::image_kind(&bytes).ok_or("Unsupported image format")?;
        let size = imagesize::blob_size(&bytes).map_err(|_| "Image header unreadable")?;
        if size.width == 0
            || size.height == 0
            || size.width > 4096
            || size.height > 4096
            || size.width.saturating_mul(size.height) > 4_000_000
        {
            return Err("Image exceeds 4 million pixel preview limit");
        }
        item["image"]["width"] = json!(size.width);
        item["image"]["height"] = json!(size.height);
        item["image"]["src"] = json!(format!("data:image/{kind};base64,{data}"));
        Ok(())
    })();
    if let Err(reason) = preview {
        item["image"]["reason"] = json!(reason);
    }
    item
}

/// CS2: timestamp on the newest visible native record; a missing timestamp stays unknown.
pub fn last_update(format: &str, session: &str, path: &Path) -> Res<Value> {
    Ok(before(format, session, path, None)?
        .items
        .last()
        .map_or(Value::Null, |i| i["updated"].clone()))
}

/// D7: the last assistant prose line in the last chunk; complete citation suffixes stay secondary.
pub fn last_line(format: &str, session: &str, path: &Path) -> Res<Option<String>> {
    let page = before(format, session, path, None)?;
    Ok(page
        .items
        .iter()
        .rev()
        .filter(|i| i["role"] == "assistant")
        .find_map(|i| {
            let mut lines = i["text"].as_str()?.lines().rev();
            while let Some(line) = lines.next() {
                if line.trim().is_empty() {
                    continue;
                }
                if line.trim() == "</oai-mem-citation>" {
                    let mut before = lines.clone();
                    if before.any(|l| l.trim() == "<oai-mem-citation>") {
                        lines = before;
                        continue;
                    }
                }
                return Some(line.into());
            }
            None
        }))
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
