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
pub struct Page { pub generation: String, pub items: Vec<Value>, pub start: u64, pub end: u64, pub size: u64 }

/// Native file identity plus its immutable prefix; appends preserve it, replacement is never mixed into a window.
// shortcut: later in-place rewrites are undetected; add a provider epoch if one occurs.
fn generation(path: &Path) -> Res<String> {
    let mut file=fs::File::open(path).map_err(|e|format!("transcript identity unknown: {e}"))?;
    let meta=file.metadata().map_err(|e|format!("transcript identity unknown: {e}"))?;
    #[cfg(unix)]
    let identity={use std::os::unix::fs::MetadataExt;format!("{}:{}",meta.dev(),meta.ino())};
    #[cfg(windows)]
    let identity={
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::HANDLE,Storage::FileSystem::{FILE_ID_INFO,FileIdInfo,GetFileInformationByHandleEx}};
        let mut id=FILE_ID_INFO::default();
        unsafe {GetFileInformationByHandleEx(HANDLE(file.as_raw_handle()),FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),std::mem::size_of::<FILE_ID_INFO>() as u32)}
            .map_err(|e|format!("transcript identity unknown: {e}"))?;
        format!("{}:{:?}",id.VolumeSerialNumber,id.FileId.Identifier)
    };
    #[cfg(not(any(unix,windows)))]
    let identity=format!("{:?}",meta.created().map_err(|_|"transcript creation identity unavailable")?);
    let mut prefix=vec![];
    (&mut file).take(meta.len().min(128)).read_to_end(&mut prefix).map_err(|e|format!("transcript unreadable: {e}"))?;
    let prefix=&prefix[..prefix.iter().position(|b|*b==b'\n').map_or(prefix.len(),|i|i+1)];
    let digest=ring::digest::digest(&ring::digest::SHA256,&prefix);
    // Image extraction changes block ordinals; old saved positions must never bind to a different block.
    Ok(format!("images-v1:{identity}:{}",digest.as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>()))
}
fn page(path:&Path,generation:String,mut items:Vec<Value>,start:u64,end:u64,size:u64)->Res<Page> {
    if self::generation(path)?!=generation {return Err("transcript replaced during read; view unknown".into());}
    let (mut count,mut bytes)=(0,0);
    for item in &mut items {
        if let Some(src)=item["image"]["src"].as_str() {
            count+=1;bytes+=src.len();
            if count>4 || bytes>2*1024*1024 {
                item["image"].as_object_mut().unwrap().remove("src");
                item["image"]["reason"]=json!("Image page preview limit; inspect the native source");
            }
        }
    }
    Ok(Page{generation,items,start,end,size})
}

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
    let generation=generation(path)?;
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
                return page(path,generation,records.into_iter().flat_map(|(_,items)|items).collect(),start,from+last as u64,size);
            }
            None if from > 0 && read < RECORD => read *= 2,
            None => return Err("transcript record exceeds 16 MiB; view unknown".into()),
        }
    }
}

/// The complete records that start at `from`: D4 polling while the view is open, and forward reads of an evicted
/// range. One chunk, more only for a longer record; a partial record at the file end waits for its newline.
pub fn after(codex: bool, path: &Path, from: u64) -> Res<Page> {
    let generation=generation(path)?;
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
                return page(path,generation,records.into_iter().flat_map(|(_,items)|items).collect(),from,end,size);
            }
            None if end == size => return page(path,generation,vec![],from,from,size),
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
    let arg: String = mask(&arg).chars().take(160).collect();
    let mut item=line("tool", format!("{name} {arg}").trim());
    item["tool_name"]=json!(name);
    item
}

/// #1825 A4, #1833 A2: a tool line never shows a token-shaped word, the value of a credential assignment or JSON
/// field, or the credential of an Authorization header or bearer scheme; command output is never in the view.
fn mask(arg: &str) -> String {
    let (mut out, mut header, mut secret) = (vec![], false, false);
    for w in json_masked(arg).split_whitespace() {
        let bare = w.replace(['"', '\''], "").to_ascii_lowercase();
        let scheme = ["bearer", "basic", "token", "digest"].contains(&bare.as_str());
        out.push(if secret || header && !scheme { "[redacted]".into() } else { redact(w) });
        // `Authorization: <scheme> <credential>` or `Authorization: <credential>`; `Bearer <credential>` anywhere
        (secret, header) = (!secret && (header && scheme || bare == "bearer"), bare.ends_with("authorization:"));
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
        let quote = if rest[..j].ends_with('\\') { "\\\"" } else { "\"" };
        let name = rest[..j].trim_end_matches('\\').to_ascii_lowercase();
        let named = name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
            && ["token", "secret", "password", "passwd", "api_key", "apikey", "authorization"].iter().any(|n| name.contains(n));
        let value = rest[j + 1..].trim_start().strip_prefix(':').map(str::trim_start).and_then(|v| v.strip_prefix(quote)).filter(|_| named);
        let end = value.and_then(|v| if quote == "\"" { v.match_indices('"').map(|(k, _)| k).find(|k| !v[..*k].ends_with('\\')) } else { v.find(quote) });
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
    let ident = |i: usize| s[i..].iter().take_while(|c| c.is_alphanumeric() || **c == '_' || **c == '$').collect::<String>();
    let colon = |i: usize| s[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
    while i < s.len() {
        match s[i] {
            '/' if s.get(i + 1) == Some(&'/') => while i < s.len() && s[i] != '\n' { i += 1 },
            '/' if s.get(i + 1) == Some(&'*') => i = (i + 3..s.len()).find(|&k| s[k - 1] == '*' && s[k] == '/').map_or(s.len(), |k| k + 1),
            q @ ('"' | '\'' | '`') => {
                let (lit, end) = literal(&s, i + 1, q);
                i = end;
                if colon(i) { key = Some(lit); continue; }
                if let Some((name, target @ None)) = calls.last_mut() {
                    if matches!((name.as_str(), key.as_deref()), ("exec_command", Some("cmd")) | ("view_image", Some("path")) | ("apply_patch", None)) { *target = Some(lit); }
                }
                key = None;
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => {
                let word = ident(i);
                i += word.chars().count();
                if word == "tools" && s.get(i) == Some(&'.') {
                    let name = ident(i + 1);
                    i += 1 + name.chars().count();
                    if s.get(i) == Some(&'(') { calls.push((name, None)); key = None; }
                } else if colon(i) { key = Some(word); }
            }
            _ => i += 1,
        }
    }
    let line: Vec<String> = calls.into_iter().map(|(name, target)| match (name.as_str(), target) {
        ("exec_command", Some(cmd)) => cmd,
        ("apply_patch", Some(patch)) => format!("apply_patch {}", patch_files(&patch)),
        (_, Some(target)) => format!("{name} {target}"),
        (_, None) => name,
    }).collect();
    (!line.is_empty()).then(|| line.join(" · "))
}

/// The JavaScript string literal that starts at `i`, closed by `q`, and the index after it; an open one ends the script.
fn literal(s: &[char], mut i: usize, q: char) -> (String, usize) {
    let mut out = String::new();
    while i < s.len() && s[i] != q {
        if s[i] == '\\' && i + 1 < s.len() {
            i += 1;
            let hex = |n: usize| s.get(i + 1..i + 1 + n).and_then(|h| u32::from_str_radix(&h.iter().collect::<String>(), 16).ok()).and_then(char::from_u32);
            match s[i] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' if hex(4).is_some() => { out.push(hex(4).unwrap()); i += 4; }
                'x' if hex(2).is_some() => { out.push(hex(2).unwrap()); i += 2; }
                c => out.push(c),
            }
        } else { out.push(s[i]); }
        i += 1;
    }
    (out, (i + 1).min(s.len()))
}

/// The files an `apply_patch` envelope adds, updates or deletes.
fn patch_files(patch: &str) -> String {
    patch.lines().filter_map(|l| ["*** Add File: ", "*** Update File: ", "*** Delete File: "].iter().find_map(|p| l.strip_prefix(p))).collect::<Vec<_>>().join(" ")
}

fn redact(w: &str) -> String {
    let (name, value) = w.rsplit_once('=').unwrap_or(("", w));
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    let upper = name.to_ascii_uppercase();
    let secret = (!value.is_empty() && ["TOKEN", "SECRET", "PASSWORD", "API_KEY"].iter().any(|k| upper.ends_with(k)))
        || value.len() >= 20 && ["ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "glpat-", "sk-", "xoxb-", "xoxp-", "AKIA"].iter().any(|p| value.starts_with(p));
    match (secret, name.is_empty()) { (false, _) => w.into(), (true, true) => "[redacted]".into(), (true, false) => format!("{name}=[redacted]") }
}

/// Only recorded bytes authorize an inline image. Paths and remote URLs never trigger a file/network read.
fn image(role:&str,b:&Value)->Value {
    use base64::Engine;
    let name=b["alt"].as_str().or(b["name"].as_str()).filter(|s|!s.trim().is_empty()).unwrap_or("Recorded image");
    // Native user-image wrappers also carry tool images; their human/board authorship is unknown.
    let mut item=line(if role=="user" {"image"} else {role},"");item["native_role"]=json!(role);
    item["image"]=json!({"name":name.chars().take(160).collect::<String>()});
    let url=b["image_url"].as_str().or(b["image_url"]["url"].as_str()).or(b["source"]["url"].as_str());
    if let Some(url)=url.filter(|s|s.len()<=4096&&(s.starts_with("https://")||s.starts_with("http://"))) { item["image"]["url"]=json!(url); }
    let data=url.and_then(|s|s.strip_prefix("data:image/")).and_then(|s|s.split_once(";base64,")).map(|(_,data)|data)
        .or_else(||(b["source"]["type"]=="base64").then(||b["source"]["data"].as_str()).flatten());
    let preview=(|| {
        let data=data.ok_or("Image bytes missing or access unavailable; no file or URL fetched")?;
        if data.len()>2*1024*1024 {return Err("Image exceeds 2 MiB preview limit")}
        let bytes=base64::engine::general_purpose::STANDARD.decode(data).map_err(|_|"Image encoding unreadable")?;
        let kind=crate::board::image_kind(&bytes).ok_or("Unsupported image format")?;
        let size=imagesize::blob_size(&bytes).map_err(|_|"Image header unreadable")?;
        if size.width==0||size.height==0||size.width>4096||size.height>4096||size.width.saturating_mul(size.height)>4_000_000 {return Err("Image exceeds 4 million pixel preview limit")}
        item["image"]["width"]=json!(size.width);item["image"]["height"]=json!(size.height);
        item["image"]["src"]=json!(format!("data:image/{kind};base64,{data}"));Ok(())
    })();
    if let Err(reason)=preview {item["image"]["reason"]=json!(reason);}
    item
}

fn images(role:&str,content:&Value,out:&mut Vec<Value>) {
    for b in content.as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("image"|"input_image"|"image_url"|"output_image")=>out.push(image(role,b)),
            Some("tool_result")=>images(role,&b["content"],out),
            _=>{}
        }
    }
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
            Some("image")=>out.push(image(role,b)),
            Some("tool_result")=>images("tool",&b["content"],out),
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
            let mut text=Vec::new();
            for b in p["content"].as_array().into_iter().flatten() {
                if let Some(t)=b["text"].as_str() {text.push(t);} else if matches!(b["type"].as_str(),Some("image"|"input_image"|"image_url"|"output_image")) {
                    if !text.is_empty() {out.push(line(role,&text.join("\n")));text.clear();}
                    out.push(image(role,b));
                }
            }
            if !text.is_empty() {out.push(line(role,&text.join("\n")));}
        }
        (Some(kind @ ("function_call" | "custom_tool_call")), _) => {
            let raw = p["arguments"].as_str().or(p["input"].as_str());
            let name = p["name"].as_str().unwrap_or("tool");
            // a custom tool's input is free text: an exec script or an apply_patch envelope, never shown as is
            let summary = raw.filter(|_| kind == "custom_tool_call").and_then(|a| if name == "apply_patch" { Some(patch_files(a)) } else { codex_script(a) });
            let input = match summary { Some(cmd) => json!({"cmd": cmd}), None => raw.map_or(Value::Null, |a| serde_json::from_str(a).unwrap_or_else(|_| json!(a))) };
            out.push(tool(name, &input));
        }
        (Some("function_call_output"|"custom_tool_call_output"),_)=>images("tool",&p["output"],out),
        (Some("local_shell_call"), _) => out.push(tool("shell", &p["action"])),
        _ => {}
    }
}

/// CS2: timestamp on the newest visible native record; a missing timestamp stays unknown.
pub fn last_update(codex:bool,path:&Path)->Res<Value> {
    Ok(before(codex,path,None)?.items.last().map_or(Value::Null,|i|i["updated"].clone()))
}

/// D7: the last assistant prose line in the last chunk; complete citation suffixes stay secondary.
pub fn last_line(codex: bool, path: &Path) -> Res<Option<String>> {
    let page = before(codex, path, None)?;
    Ok(page.items.iter().rev().find(|i| i["role"] == "assistant").and_then(|i| {
        let mut lines=i["text"].as_str()?.lines().rev();
        while let Some(line)=lines.next() {
            if line.trim().is_empty(){continue;}
            if line.trim()=="</oai-mem-citation>" {
                let mut before=lines.clone();
                if before.any(|l|l.trim()=="<oai-mem-citation>") {lines=before;continue;}
            }
            return Some(line.into());
        }
        None
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, path::PathBuf};

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-transcript-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }
    /// The view text of items, without their record identity.
    fn text(items: &[Value]) -> Vec<Value> { items.iter().map(|i| json!({"role": i["role"], "text": i["text"]})).collect() }
    fn key(i: &Value) -> (u64, u64) { (i["at"].as_u64().unwrap(), i["i"].as_u64().unwrap()) }

    // D4/#2159: recorded image bytes, not a text path, are the only preview authority.
    #[test]
    fn recorded_images_keep_source_and_bound_preview_without_fetching_paths() {
        let png="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";
        let v=json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Review the image"},{"type":"input_image","image_url":format!("data:image/png;base64,{png}")} ]}});
        let rows=records(true,40,format!("{v}\n").as_bytes());let items=&rows[0].1;
        assert_eq!(items.len(),2,"recorded image must remain a separate inspectable item");
        assert_eq!(items[1]["role"],"image","a provider user-image wrapper is not proof of an owner/board author");assert_eq!(items[1]["native_role"],"user");
        assert_eq!(items[1]["image"]["width"],1);assert!(items[1]["image"]["src"].as_str().unwrap().starts_with("data:image/png;"));
        let v=json!({"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":png}}]},{"type":"image","source":{"type":"url","url":"file:///unrelated/private.png"}}]}});
        let rows=records(false,0,format!("{v}\n").as_bytes());assert_eq!(rows[0].1.len(),2);
        assert!(rows[0].1[0]["image"]["src"].is_string());assert!(rows[0].1[1]["image"]["reason"].is_string());assert!(rows[0].1[1]["image"]["src"].is_null());
        let dir=tmp();let path=dir.join("images.jsonl");
        let recorded=json!({"type":"user","message":{"content":[{"type":"image","source":{"type":"base64","data":png}}]}});
        fs::write(&path,format!("{recorded}\n").repeat(8)).unwrap();let p=before(false,&path,None).unwrap();
        assert!(p.generation.starts_with("images-v1:"),"new image ordinals require a parser-generation fence");
        assert_eq!(p.items.iter().filter(|i|i["image"]["src"].is_string()).count(),4,"page has at most four inline image sources");
        assert!(p.items[7]["image"]["reason"].as_str().unwrap().contains("limit"));
        let mut malformed=recorded.clone();malformed["message"]["content"][0]["source"]["data"]=json!("broken");
        let rows=records(false,0,format!("{malformed}\n").as_bytes());assert!(rows[0].1[0]["image"]["reason"].is_string());
        fs::remove_dir_all(dir).unwrap();
    }

    // D-A3: a 10 MiB transcript opens in one chunk read, scrolls up to its first record and appends while open
    // #1929 A4: byte offsets alone must not merge replacement sources; appending keeps generation.
    #[test]
    fn conversation_1929_generation_survives_append_and_detects_replacement() {
        let dir=std::env::temp_dir().join(format!("taskq-generation-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();let p=dir.join("log");
        let line=b"{\"type\":\"ignored\"}\n";fs::write(&p,line).unwrap();let first=before(false,&p,None).unwrap().generation;
        use std::io::Write;fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(line).unwrap();
        assert_eq!(before(false,&p,None).unwrap().generation,first);
        let replacement=dir.join("new");fs::write(&replacement,line).unwrap();
        #[cfg(windows)] {
            use std::os::windows::fs::FileTimesExt;
            let created=fs::metadata(&p).unwrap().created().unwrap();
            fs::OpenOptions::new().write(true).open(&replacement).unwrap().set_times(fs::FileTimes::new().set_created(created)).unwrap();
            assert_eq!(fs::metadata(&replacement).unwrap().created().unwrap(),created);
        }
        fs::remove_file(&p).unwrap();fs::rename(replacement,&p).unwrap();
        assert_ne!(before(false,&p,None).unwrap().generation,first);fs::remove_dir_all(dir).unwrap();
    }

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

    // #2159 D7: status previews prose, while metadata, partial text and exact source remain inspectable.
    #[test]
    fn last_assistant_preview_excludes_complete_citation_suffix_only() {
        let dir=tmp();let path=dir.join("preview.jsonl");
        for codex in [false,true] {
            for (text,expected) in [
                ("Reviewed result.\n<oai-mem-citation>\nMEMORY.md:1-2\n</oai-mem-citation>\n",Some("Reviewed result.")),
                ("Ответ принят.\n\n<oai-mem-citation>\nfirst\n</oai-mem-citation>\n<oai-mem-citation>\nsecond\n</oai-mem-citation>",Some("Ответ принят.")),
                ("<oai-mem-citation>\nmetadata only\n</oai-mem-citation>",None),
                ("Reviewed.\n<oai-mem-citation>\npartial metadata",Some("partial metadata")),
                ("Reviewed.\n</oai-mem-citation>",Some("</oai-mem-citation>")),
                ("Literal example:\n```xml\n<oai-mem-citation>\nexample\n</oai-mem-citation>\n```",Some("```")),
                ("Reviewed.\n<oai-mem-citation>\nmetadata\n</oai-mem-citation>\nNext action.",Some("Next action.")),
                ("",None),
            ] {
                let record=if codex {json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}})}
                    else {json!({"type":"assistant","message":{"content":[{"type":"text","text":text}]}})};
                let source=format!("{record}\n");fs::write(&path,&source).unwrap();
                assert_eq!(last_line(codex,&path).unwrap().as_deref(),expected,"codex={codex}, text={text}");
                assert_eq!(fs::read_to_string(&path).unwrap(),source,"preview never rewrites native history");
                if !text.is_empty(){assert_eq!(before(codex,&path,None).unwrap().items[0]["text"],text,"exact source remains complete");}
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // #1825 A4: a token-shaped word or a credential assignment in a tool call never reaches the view
    #[test]
    fn tool_lines_redact_credentials() {
        let token = format!("ghp_{}", "a".repeat(36));
        let input = json!({"command": format!("GH_TOKEN={token} gh api -H \"Authorization: token {token}\" x; DB_PASSWORD='p' run; sk-short; KEY=v")});
        assert_eq!(tool("Bash", &input)["text"], "Bash GH_TOKEN=[redacted] gh api -H \"Authorization: token [redacted] x; DB_PASSWORD=[redacted] run; sk-short; KEY=v");
    }

    // #1833 A2/A3: bearer tokens, Authorization headers and credential-named JSON fields never reach the view;
    // other words and fields stay readable. Values are invented.
    #[test]
    fn tool_lines_mask_headers_bearer_and_json_fields() {
        let line = |cmd: &str| tool("Bash", &json!({"command": cmd}))["text"].as_str().unwrap().to_string();
        assert_eq!(line("curl -H 'Authorization: Bearer abc.def' u"), "Bash curl -H 'Authorization: Bearer [redacted] u");
        assert_eq!(line("curl -H \"authorization: s3cr3t\" -H 'Accept: json' u"), "Bash curl -H \"authorization: [redacted] -H 'Accept: json' u");
        assert_eq!(line("http u Bearer xyz next"), "Bash http u Bearer [redacted] next");
        assert_eq!(line("requests.get(u, headers={'Authorization': 'Basic dXNlcg=='})"), "Bash requests.get(u, headers={'Authorization': 'Basic [redacted]");
        assert_eq!(line(r#"curl -d '{"access_token":"t1","name":"n","Password": "p w","max_tokens":5,"client_secret":"s"}' u"#),
            r#"Bash curl -d '{"access_token":"[redacted]","name":"n","Password": "[redacted]","max_tokens":5,"client_secret":"[redacted]"}' u"#);
        assert_eq!(line(r#"curl -d "{\"api_key\":\"k\",\"q\":\"x\"}" u"#), r#"Bash curl -d "{\"api_key\":\"[redacted]\",\"q\":\"x\"}" u"#);
        assert_eq!(line(r#"x '{"Authorization":"Bearer z","a":"b \" c"}'"#), r#"Bash x '{"Authorization":"[redacted]","a":"b \" c"}'"#);
        // a tool input without a known command field is shown as JSON and masked the same way
        assert_eq!(tool("mcp", &json!({"token": "t2", "repo": "r"}))["text"], r#"mcp {"token":"[redacted]","repo":"r"}"#);
    }

    // #1833 A1/A3: a Codex custom tool call shows the commands its script runs, never the script; other tools show
    // their name and target. Records follow the native rollout shape; paths and values are invented.
    #[test]
    fn codex_custom_tool_calls_read_as_commands() {
        let dir = tmp();
        let path = dir.join("rollout.jsonl");
        let call = |name: &str, input: &str| json!({"type":"response_item","payload":{"type":"custom_tool_call","status":"completed","call_id":"c","name":name,"input":input}});
        let lines = [
            call("exec", "text(await tools.exec_command({cmd:\"echo two\",max_output_tokens:2000}));\n"),
            call("exec", "// @exec: {\"yield_time_ms\": 1000}\nconst r=await Promise.allSettled([\ntools.exec_command({cmd:'git log -1 --format=\\'%H\\'', workdir:w}),\ntools.exec_command({\"cmd\": `rg -n \"tools.exec_command(\" src`}),\ntools.write_stdin({session_id:7,chars:\"secret\\n\"})\n]); r.forEach((v,i)=>text({i,...v}));\n"),
            call("exec", "text(await tools.apply_patch(\"*** Begin Patch\\n*** Update File: src/a.rs\\n@@\\n-x\\n+y\\n*** Add File: docs/b.md\\n+z\\n*** End Patch\\n\"));"),
            call("exec", "const r=await tools.view_image({path:\"shots/a.png\"});image(r.image_url);"),
            call("exec", "text(await tools.exec_command({cmd:\"GH_TOKEN=ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa gh api -H 'Authorization: Bearer q' x\"}));"),
            call("apply_patch", "*** Begin Patch\n*** Delete File: old.txt\n*** End Patch\n"),
            call("exec", "let x = 1;"),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}"}}),
        ];
        fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        let tools: Vec<Value> = text(&before(true, &path, None).unwrap().items).into_iter().map(|i| i["text"].clone()).collect();
        assert_eq!(tools, vec![
            json!("exec echo two"),
            json!("exec git log -1 --format='%H' · rg -n \"tools.exec_command(\" src · write_stdin"),
            json!("exec apply_patch src/a.rs docs/b.md"),
            json!("exec view_image shots/a.png"),
            json!("exec GH_TOKEN=[redacted] gh api -H 'Authorization: Bearer [redacted] x"),
            json!("apply_patch old.txt"),
            json!("exec let x = 1;"),
            json!("exec_command ls"),
        ]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// #1833 review: a quote that never closes (a regex literal, a cut script) read past the script and panicked.
    #[test]
    fn codex_scripts_with_open_quotes_and_comments_read_without_panic() {
        assert_eq!(codex_script("text(r.replace(/'/g, ''))"), None);
        assert_eq!(codex_script("text('abc"), None);
        assert_eq!(codex_script("text(await tools.exec_command({cmd:\"echo a"), Some("echo a".into()));
        assert_eq!(codex_script("/* tools.exec_command({cmd:\"x\"}) 'it's */ text(await tools.exec_command({cmd:\"y\"}))"), Some("y".into()));
        assert_eq!(codex_script("/* open"), None);
        assert_eq!(codex_script("/*/ tools.exec_command({cmd:\"x\"})"), None);
        // every cut of a real script reads: Codex records may end mid-literal
        let js = "/* c */ const r=await tools.exec_command({cmd:'a \\'b\\' \\u00e9',\"workdir\": `w`}); // x\ntools.view_image({path:\"p\"})";
        for (k, _) in js.char_indices() { codex_script(&js[..k]); }
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

    // #2110 A5 / Q-A5: 100,000 variable-height visible records of both native renderers. Defect: a bounded or out-of-order
    // read omits or repeats a record, splits one, or stops at a page of only ignored records. Risk kept: every record
    // is reached from either end with the same identity, at most ITEMS items per page.
    #[test]
    fn hundred_thousand_records_traverse_both_ways_in_any_order() {
        let dir = tmp();
        for codex in [false, true] {
            let path = dir.join(format!("{codex}.jsonl"));
            let want = transcript(codex, &path, 100_000, 300_000, 1500, 600 * 1024);
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
            // An ignored prefix can exceed CHUNK: the saved anchor is the visible record offset, not the page boundary.
            for item in pages.iter().skip(1).filter_map(|p|p.items.first()) {let a=after(codex,&path,item["at"].as_u64().unwrap()).unwrap();assert_eq!(a.items.first(),Some(item),"a saved record cursor reloads its range");}
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
    // #1930 A3/A4/A7: sanitized actual Claude/Codex metadata, never invented times or cursor-to-date inference.
    #[test]
    fn conversation_1930_observed_native_times_survive_pages_and_gaps() {
        let fixture:Value=serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/conversation-time-provenance.json"))).unwrap();
        let dir=tmp();let path=dir.join("native.jsonl");
        for sample in fixture["records"].as_array().unwrap() {
            let codex=sample["provider"]=="codex";let record=&sample["record"];
            fs::write(&path,format!("{record}\n")).unwrap();
            let page=before(codex,&path,None).unwrap();assert!(!page.items.is_empty());
            for item in &page.items {assert_eq!(item["updated"],record["timestamp"]);assert_eq!(item["at"],0);}
            assert_eq!(after(codex,&path,0).unwrap().items,page.items,"both read directions preserve source time and byte identity");
            let mut missing=record.clone();missing.as_object_mut().unwrap().remove("timestamp");
            fs::write(&path,format!("{record}\n{missing}\n")).unwrap();
            let page=before(codex,&path,None).unwrap();assert!(page.items.last().unwrap()["updated"].is_null());
            assert!(last_update(codex,&path).unwrap().is_null(),"a source gap cannot borrow the earlier record time");
        }
        for event in fixture["board"].as_array().unwrap() {assert_eq!(crate::board::iso(event["created_at"].as_str().unwrap()),event["seconds"].as_i64());}
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
