//! Antigravity owner sessions: original AgentQ I/O, exact native identity and no submission replay.
use super::owner::Launched;
use crate::runtime::Cap;
use crate::setup::Request;
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, time::Duration};
type Res<T> = Result<T, String>;
pub fn handles(runtime: &str) -> bool {
    super::of(runtime).has(Cap::StreamInput)
}
pub fn log(own: &Path) -> std::path::PathBuf {
    own.join("native.jsonl")
}
fn input(own: &Path, text: &str) -> Res<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log(own))
        .map_err(|_| "Native input log unavailable")?;
    writeln!(
        file,
        "{}",
        json!({"event":"user","message":{"content":text},"delivery":"intent"})
    )
    .and_then(|_| file.sync_all())
    .map_err(|_| "Native input intent write unknown; no replay".into())
}
pub fn launch(own: &Path, r: &Request) -> Launched {
    match start(own, r) {
        Ok(session) => Launched::Job {
            job: r.id.clone(),
            session: Some(session),
            url: None,
        },
        Err(reason) => Launched::Unknown(reason),
    }
}
fn start(own: &Path, r: &Request) -> Res<String> {
    fs::write(own.join("brief.txt"), &r.brief).map_err(|_| "Owner brief unavailable")?;
    let text = super::owner::read_brief(&own.join("brief.txt"));
    let request = super::owner::request(own, r, &text)?;
    write_input(own, &r.runtime, &text)?;
    let adapter = super::agent(&r.runtime)?;
    let mut turn = adapter.launch(&request).map_err(|error| error.detail)?;
    let pid = turn
        .id()
        .ok_or("Native process identity unavailable; no replay")?;
    if !adapter.descriptor().has("acp") {
        let saved = adapter.checkpoint(&turn).map_err(|e| e.to_string())?;
        coreq::write_durable(&own.join("runtime-run.json"), &saved.to_string())?;
    }
    let identity_result = coreq::process::identity(pid);
    let identity = identity_result.as_ref().ok().and_then(|i| i.clone());
    let checkpoint = identity
        .as_ref()
        .ok_or("Native process already exited; acceptance unknown")
        .and_then(|identity| {
            coreq::write_durable(
                &own.join("native-process.json"),
                &json!({"identity":{"pid":identity.pid,"started":identity.started},"session":null})
                    .to_string(),
            )
            .map_err(|_| "Native identity save unknown; no replay")
        });
    let path = own.to_path_buf();
    std::thread::spawn(move || {
        loop {
            match adapter.poll(&mut turn).and_then(|_| adapter.report(&turn)) {
                Ok(report)
                    if matches!(
                        report.termination,
                        agentq_runtime::agent::ProcessTermination::Observed
                    ) =>
                {
                    let exit = report.exit.unwrap_or(-1);
                    let _=coreq::write_durable(&path.join("native-exit.json"),&json!({"identity":identity.map(|i|json!({"pid":i.pid,"started":i.started})),"exit":exit,"session":report.session,"outcome":report.outcome}).to_string());
                    break;
                }
                Ok(_) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    });
    identity_result.map_err(|_| "Native process identity unknown; no replay")?;
    checkpoint?;
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(session) = session(
            &r.runtime,
            &fs::read_to_string(log(own)).map_err(|_| "Native session read unknown; no replay")?,
        )? {
            return Ok(session);
        }
        if own.join("native-exit.json").exists() || std::time::Instant::now() >= until {
            return Err(
                "Antigravity session receipt unknown; original launch retained, never replayed"
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub(super) fn native_source(session: &str) -> Res<()> {
    let home = crate::platform::user_home().ok_or("Native home unknown")?;
    if ["ANTIGRAVITY_HOME", "ANTIGRAVITY_APP_DATA_DIR"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        return Err("Custom Antigravity state mapping unqualified; no input sent".into());
    }
    readable_source(
        &home
            .join(".gemini/antigravity-cli/conversations")
            .join(format!("{session}.db")),
    )
}
fn readable_source(path: &Path) -> Res<()> {
    if !crate::messages::plain(path)? || !path.is_file() {
        return Err("Native Antigravity conversation is missing; resume refused before input, no new conversation fallback".into());
    }
    let db = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "Native conversation database unreadable; no input sent")?;
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "Native conversation database invalid; no input sent")?;
    if tables == 0 {
        return Err("Native conversation database has no supported state; no input sent".into());
    }
    Ok(())
}
/// agy may ignore a missing --conversation. Never send until its own startup receipt binds the expected ID.
#[cfg(test)]
pub fn ready(
    own: &Path,
    rt: &str,
    expected: &str,
    offset: u64,
    child: &mut std::process::Child,
) -> Res<()> {
    ready_before_input(own, rt, expected, offset, child.id())
}

#[cfg(test)]
pub(crate) fn ready_before_input(
    own: &Path,
    rt: &str,
    expected: &str,
    offset: u64,
    pid: u32,
) -> Res<()> {
    use std::io::{Read, Seek, SeekFrom};
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let mut file = fs::File::open(log(own))
            .map_err(|_| "Native startup receipt unavailable; input not sent")?;
        if file
            .metadata()
            .map_err(|_| "Native startup source size unknown")?
            .len()
            < offset
        {
            return Err("Native startup source changed; input not sent".into());
        }
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| "Native startup boundary unknown")?;
        let mut text = String::new();
        file.take(16 * 1024 * 1024)
            .read_to_string(&mut text)
            .map_err(|_| "Native startup read failed; input not sent")?;
        if let Some(session) = session(rt, &text)? {
            return if session == expected {
                Ok(())
            } else {
                Err("Antigravity returned a different session; input not sent, no fallback".into())
            };
        }
        if coreq::process::identity(pid)
            .map_err(|_| "Native startup liveness unknown")?
            .is_none()
            || std::time::Instant::now() >= until
        {
            return Err("Native startup identity unverified; input not sent".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn holder(own: &Path, r: &Request) -> Res<bool> {
    match fs::read(own.join("turn.json")) {
        Ok(bytes) => {
            let turn: Value =
                serde_json::from_slice(&bytes).map_err(|_| "Native reply receipt unreadable")?;
            if matches!(turn["state"].as_str(), Some("dispatching" | "running"))
                || turn["outcome"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("unknown"))
            {
                return Err(
                    "Native reply acceptance/release unverified; original input retained".into(),
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Native reply ownership unknown".into()),
    }

    let native: Value = serde_json::from_slice(
        &fs::read(own.join("native-process.json"))
            .map_err(|_| "Native process ownership unavailable")?,
    )
    .map_err(|_| "Native process ownership unreadable")?;
    let identity = coreq::process::Identity {
        pid: native["identity"]["pid"]
            .as_u64()
            .and_then(|p| p.try_into().ok())
            .ok_or("Native PID unknown")?,
        started: native["identity"]["started"]
            .as_str()
            .ok_or("Native creation identity unknown")?
            .into(),
    };
    if let Some(current) =
        coreq::process::identity(identity.pid).map_err(|_| "Native process liveness unknown")?
    {
        if current == identity {
            return Ok(true);
        }
    }
    let ended: Value = serde_json::from_slice(
        &fs::read(own.join("native-exit.json"))
            .map_err(|_| "Native execution release unknown; no input sent")?,
    )
    .map_err(|_| "Native exit receipt unreadable")?;
    if ended["identity"] != native["identity"]
        || ended["session"].as_str() != r.session.as_deref()
        || ended["outcome"] == "unknown"
    {
        return Err("Native session completion unverified; no input sent".into());
    }
    Ok(false)
}
pub fn session(rt: &str, text: &str) -> Res<Option<String>> {
    let mut session = None;
    for value in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        if let Some(id) = super::classify(rt, &value.to_string()).and_then(|r| r.session) {
            if !super::plain_thread(&id) || session.as_ref().is_some_and(|old| old != &id) {
                return Err("Native conversation identity changed; no replay".into());
            }
            session = Some(id);
        }
    }
    Ok(session)
}
pub fn transcript(own: &Path, r: &Request) -> Res<std::path::PathBuf> {
    let path = log(own);
    let mut bytes = vec![];
    use std::io::Read;
    fs::File::open(&path)
        .and_then(|f| f.take(65536).read_to_end(&mut bytes))
        .map_err(|_| "Native Antigravity stream unavailable")?;
    let text = String::from_utf8_lossy(&bytes);
    if session(&r.runtime, &text)?.as_deref() != r.session.as_deref() {
        return Err("Native Antigravity source identity changed".into());
    }
    Ok(path)
}
pub fn write_input(own: &Path, runtime: &str, text: &str) -> Res<()> {
    if super::agent(runtime)?.descriptor().has("acp") {
        // Intent is already durable in brief.txt or turn.json; ACP owns its physical representation.
        fs::OpenOptions::new().create(true).append(true).open(log(own))
            .map_err(|_| "Native source unavailable")?;
        Ok(())
    } else {
        input(own, text)
    }
}
pub fn outcome(rt: &str, text: &str) -> String {
    let rows: Vec<Value> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let outcome = rows
        .last()
        .and_then(|v| super::classify(rt, &v.to_string()))
        .map_or("unknown", |r| r.outcome);
    if outcome == "unknown" {
        "unknown: native Antigravity completion unverified".into()
    } else if outcome == "ok" {
        "success".into()
    } else {
        format!("failed: {outcome}")
    }
}
pub fn accepted(own: &Path, r: &Request, turn: &crate::dialogue::Turn) -> Res<bool> {
    use std::io::{Read, Seek, SeekFrom};
    if !turn.stream_written {
        return Ok(false);
    }
    let Some(offset) = turn.stream_offset else {
        return Ok(false);
    };
    let mut file = fs::File::open(log(own)).map_err(|_| "Native acceptance source unavailable")?;
    if file
        .metadata()
        .map_err(|_| "Native acceptance size unknown")?
        .len()
        < offset
    {
        return Err("Native acceptance source shrank".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "Native acceptance offset unavailable")?;
    let mut text = String::new();
    file.take(16 * 1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|_| "Native acceptance unreadable")?;
    if session(&r.runtime, &text)?.as_deref() != r.session.as_deref() {
        return Ok(false);
    }
    let mut history = crate::transcript::at(
        super::table::OWNER_STREAM_FORMAT,
        &log(own),
        r.session
            .as_deref()
            .ok_or("Native session identity unknown")?,
        offset,
    )?;
    let end = offset.saturating_add(text.len() as u64);
    loop {
        let before = history.checkpoint()["offset"]
            .as_u64()
            .ok_or("Native acceptance cursor unknown")?;
        let page = history.poll(&log(own))?;
        if page.events.iter().any(|event| {
            event["end"].as_u64().is_some_and(|at| at <= end)
                && event["kind"] == "message"
                && event["author"] == "user"
                && event["state"] == "completed"
        }) {
            return Ok(true);
        }
        let after = history.checkpoint()["offset"]
            .as_u64()
            .ok_or("Native acceptance cursor unknown")?;
        if !page.more || after == before || after >= end {
            return Ok(false);
        }
    }
}

#[cfg(test)]
#[path = "owner_stream_tests.rs"]
mod tests;
