//! Installation-local telemetry. All storage mutations share the installation budget lock; export is read-only.
mod semantic;
pub(crate) use semantic::Sample;
pub(crate) mod diagnostic;
mod source;

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::io::Read;

const APP: i64 = 0x5451544d;
const LINE_MAX: usize = 64 << 10;
const WINDOW_MS: u64 = 30 * 86400 * 1000;
const LEGACY_MAX: u64 = crate::data::LOG_MAX + LINE_MAX as u64;

/// Legacy import store cap: with its WAL, SHM and taskq.log it fits the 64 MiB peer share.
const LEGACY_DB_MAX: i64 = 28 << 20;

/// 1 GiB installation budget, half held in reserve; the peer share holds legacy text and the legacy import store.
pub(crate) fn budget(install:&Path,peer:&str)->Result<coreq::telemetry::Budget,String> {
    let install=std::path::absolute(install).map_err(|e|e.to_string())?;
    let dir=install.join("telemetry.log");
    Ok(coreq::telemetry::Budget {lock:install.join("taskq-profile.lock"),
        peers:vec![dir.join(peer),dir.join(format!("{peer}-wal")),dir.join(format!("{peer}-shm")),install.join("taskq.log")],
        total:coreq::telemetry::INSTALLATION_BYTES,reserved:coreq::telemetry::PEER_BYTES})
}

/// #1351: receipts are mandatory but never block; the id is deterministic, so callers keep it without waiting for the ack.
/// A slow or failed commit leaves that phase unknown in the host log; the operation proceeds.
pub(crate) fn receipt(command:&coreq::telemetry::capture::CommandReceipt,phase:&str)->Result<String,String> {
    let id=command.event_id(phase)?;
    // A missing collector is known at once, so its existing refusal policy stays synchronous.
    if !tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().is_some()) {return Err("command receipt collector unavailable".into());}
    let job:Receipt=(command.clone(),phase.to_owned(),tracing::dispatcher::get_default(Clone::clone));
    let (pending,_)=&RECEIPTS;
    *pending.lock().unwrap()+=1;
    if RECEIPT_QUEUE.lock().unwrap().get_or_insert_with(receipt_worker).send(job).is_err() {
        settled(1);crate::platform::log(&format!("command receipt {phase} unknown: receipt worker unavailable"));
    }
    Ok(id)
}

type Receipt=(coreq::telemetry::capture::CommandReceipt,String,tracing::Dispatch);
static RECEIPTS:(std::sync::Mutex<usize>,std::sync::Condvar)=(std::sync::Mutex::new(0),std::sync::Condvar::new());
static RECEIPT_QUEUE:std::sync::Mutex<Option<std::sync::mpsc::Sender<Receipt>>>=std::sync::Mutex::new(None);

// ponytail: one FIFO worker keeps phase order; a stalled store delays later receipts by coreq's append deadline each, never callers.
// A receipt still queued at process exit stays unknown.
fn receipt_worker()->std::sync::mpsc::Sender<Receipt> {
    let (send,jobs)=std::sync::mpsc::channel::<Receipt>();
    std::thread::Builder::new().name("taskq-receipts".into()).spawn(move||for (command,phase,dispatch) in jobs {
        tracing::dispatcher::with_default(&dispatch,||if let Err(error)=command.checkpoint(&phase) {
            crate::platform::log(&format!("command receipt {phase} unknown: {error}"));
        });
        drop(dispatch); // Settlement also releases the collector's SQLite handles.
        settled(1);
    }).expect("receipt worker");
    send
}

fn settled(n:usize) {let (pending,done)=&RECEIPTS;*pending.lock().unwrap()-=n;done.notify_all();}

/// Waits until submitted receipts have committed or failed; readers of this process's receipts call it first.
pub(crate) fn settle(limit:std::time::Duration)->bool {
    let (pending,done)=&RECEIPTS;
    !done.wait_timeout_while(pending.lock().unwrap(),limit,|n|*n>0).unwrap().1.timed_out()
}

pub(crate) fn install_capture(install:&Path)->Result<coreq::telemetry::capture::Runtime,String> {
    coreq::telemetry::capture::Runtime::install_bounded(&install.join("telemetry.log/universal.sqlite3"),"taskq",crate::platform::VERSION,budget(install,"telemetry.sqlite3")?)
}

/// Called only after the host has acquired its installation owner lock.
pub(crate) fn host_capture(install:&Path)->Result<Option<coreq::telemetry::capture::Runtime>,String> {
    if tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().is_some()) {return Ok(None);}
    let mut error=match install_capture(install) {Ok(runtime)=>return Ok(Some(runtime)),Err(error)=>error};
    let recoverable=|error:&str|matches!(error,
        "telemetry IPC unsupported wire version; quiesce the old writer" |
        "telemetry IPC installation budget mismatch; quiesce incompatible writer" |
        "telemetry IPC writer election unavailable");
    if !recoverable(&error) {return Err(error);}
    let heartbeat=install.join("tray");
    let previous=std::fs::read_to_string(&heartbeat).map_err(|_|format!("{error}; tray owner unknown"))?;
    let pid=previous.trim().parse::<u32>().map_err(|_|format!("{error}; tray owner unknown"))?;
    if pid==std::process::id() || !crate::file_age(&heartbeat).is_some_and(|age|age<10) {
        return Err(format!("{error}; tray owner not fresh"));
    }
    let identity=coreq::process::identity(pid).map_err(|_|format!("{error}; tray identity unknown"))?
        .ok_or_else(||format!("{error}; tray already exited"))?;
    if std::fs::read_to_string(&heartbeat).ok().as_ref()!=Some(&previous)
        || coreq::process::identity(pid).ok().flatten().as_ref()!=Some(&identity) {
        return Err(format!("{error}; tray owner changed"));
    }
    // Older trays already leave when a different PID owns their fresh heartbeat (tray::alone).
    crate::platform::write_atomic(&heartbeat,&std::process::id().to_string())?;
    let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
    loop {
        match install_capture(install) {
            Ok(runtime)=>return Ok(Some(runtime)),
            Err(next)=>{error=next;if !recoverable(&error) || std::time::Instant::now()>=until {return Err(error);}}
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// #1336: a host without a collector retries `host_capture` each pass, so a tray that becomes fresh
/// later still hands over its writer; `pass` returns one log line per state change.
#[derive(Default)]
pub(crate) struct HostCapture {runtime:Option<coreq::telemetry::capture::Runtime>,state:String}
impl HostCapture {
    pub(crate) fn pass(&mut self,install:&Path)->Option<String> {
        if self.runtime.is_some() {return None;}
        let state=match host_capture(install) {
            Ok(runtime)=>{self.runtime=runtime;"Telemetry: collecting".to_string()},
            Err(error)=>format!("Telemetry: unknown ({error}); retrying each pass"),
        };
        (state!=self.state).then(||{self.state.clone_from(&state);state})
    }
}

/// #1755: the tray has no service pass, so its own thread (never the UI loop) retries a failed install, and a
/// collector that lost batches says when commits resume; `log` gets one line per state change.
pub(crate) fn keep_capture(install:std::path::PathBuf,every:std::time::Duration,log:impl Fn(&str)+Send+'static) {
    use coreq::telemetry::capture::Capture;
    std::thread::spawn(move || {
        // Holds a late collector for the process lifetime; the exit path drains it like the startup one.
        let mut _runtime=None;
        let mut seen=(0,0);
        let mut state=if tracing::dispatcher::get_default(|d|d.downcast_ref::<Capture>().is_some()) {"Telemetry: collecting".to_string()} else {String::new()};
        loop {
            let next=match tracing::dispatcher::get_default(|d|d.downcast_ref::<Capture>().map(Capture::health)) {
                Some(health)=>{
                    let now=(health["lost"].as_u64().unwrap_or(0),health["committed"].as_u64().unwrap_or(0));
                    let next=capture_state(seen,now,&state);seen=now;next
                }
                None=>match install_capture(&install) {
                    Ok(runtime)=>{_runtime=Some(runtime);"Telemetry: collecting".to_string()},
                    Err(error)=>format!("Telemetry: unknown ({error}); retrying every {}s",every.as_secs().max(1)),
                },
            };
            if next!=state {log(&next);state=next;}
            std::thread::sleep(every);
        }
    });
}

/// Health counts are cumulative: a new loss makes the state unknown until a later commit lands.
fn capture_state(seen:(u64,u64),now:(u64,u64),state:&str)->String {
    if now.0>seen.0 {"Telemetry: unknown (observations lost); collector retries".to_string()}
    else if now.1>seen.1 {"Telemetry: collecting".to_string()}
    else {state.to_string()}
}

fn open(install: &Path) -> Result<Connection, String> {
    // Older release sweep keeps *.log entries, including directories; rollback must preserve DB and sidecars together.
    let dir = install.join("telemetry.log");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("telemetry.sqlite3");
    let ceiling=budget(install,"universal.sqlite3")?.check_files(&path)?.min(LEGACY_DB_MAX);
    let exists = path.exists();
    let db = Connection::open(&path).map_err(|e| format!("telemetry SQLite: {e}"))?;
    db.busy_timeout(std::time::Duration::from_secs(1)).map_err(|e| e.to_string())?;
    let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if exists && (app != APP || version != 1) {
        return Err("telemetry SQLite: foreign or unsupported schema; file kept".into());
    }
    let page:i64=db.pragma_query_value(None,"page_size",|r|r.get(0)).map_err(|e|e.to_string())?;
    let pages:i64=db.pragma_query_value(None,"page_count",|r|r.get(0)).map_err(|e|e.to_string())?;
    if page<1024||pages>ceiling/page {return Err("telemetry installation budget exhausted; legacy archive preserved".into());}
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY; PRAGMA cache_spill=OFF; PRAGMA cache_size=-32768; PRAGMA wal_autocheckpoint=0;")
        .map_err(|e| e.to_string())?;
    db.pragma_update(None, "max_page_count", ceiling/page).map_err(|e| e.to_string())?;
    if !exists {
        db.execute_batch(&format!("BEGIN IMMEDIATE; PRAGMA application_id={APP}; PRAGMA user_version=1;
            CREATE TABLE records(seq INTEGER PRIMARY KEY, retained_ms INTEGER NOT NULL, line TEXT NOT NULL);
            CREATE INDEX records_time ON records(retained_ms);
            CREATE TABLE legacy(id INTEGER PRIMARY KEY CHECK(id=1), text TEXT NOT NULL);
            COMMIT;")).map_err(|e| e.to_string())?;
    }
    Ok(db)
}

// No writer starts behind a pinned reader. With cache spilling off, each database page enters this WAL at most once.
fn checkpoint(db: &Connection) -> Result<(), String> {
    let (busy, frames, done): (i64, i64, i64) = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|e| e.to_string())?;
    if busy != 0 || frames != done { return Err("telemetry SQLite: checkpoint busy; write refused".into()); }
    Ok(())
}


fn insert(db: &Connection, line: &str, now: u64) -> Result<(), String> {
    if line.len() > LINE_MAX { return Err("telemetry record exceeds 64 KiB; original kept".into()); }
    db.prepare_cached("INSERT INTO records(retained_ms,line) VALUES(?1,?2)").map_err(|e| e.to_string())?
        .execute(params![now.min(i64::MAX as u64) as i64, line]).map_err(|e| e.to_string())?;
    Ok(())
}

/// #2273: one streaming pass over taskq.log. Memory holds the rotation tail, one line head and the telemetry
/// lines, never the whole file, so a log that outgrew rotation still imports, rotates and exports. The scan never writes it.
/// Offsets and digests cover complete lines only: a diagnostic still being appended is outside the snapshot.
struct LegacyLog {
    bytes: usize, sha256: String, tail_bytes: usize, tail_sha256: String,
    /// Digests of the prefixes that end at the requested line boundaries.
    marks: Vec<(usize, String)>,
    /// Telemetry lines with their start offsets, `str::lines` form.
    telemetry: Vec<(usize, String)>,
}

impl LegacyLog {
    fn digest(&self, n: usize) -> Option<&str> { self.marks.iter().find(|(at, _)| *at == n).map(|(_, d)| d.as_str()) }
    /// The checkpoint offset this log still continues, and whether it continues the rotated tail.
    fn matched(&self, old: &Checkpoint) -> Option<(usize, bool)> {
        if self.digest(old.bytes) == Some(old.sha256.as_str()) { Some((old.bytes, false)) }
        else if old.trimmed_bytes > 0 && self.digest(old.trimmed_bytes) == Some(old.trimmed_sha256.as_str()) { Some((old.trimmed_bytes, true)) }
        else { None }
    }
    fn offset(&self, old: Option<&Checkpoint>) -> Result<usize, String> {
        match old {
            None => Ok(0),
            Some(old) => self.matched(old).map(|(n, _)| n).ok_or_else(|| "legacy taskq.log rewritten or lost; retained SQLite history kept; coverage unknown".to_string()),
        }
    }
    fn after(&self, start: usize) -> impl Iterator<Item = &str> { self.telemetry.iter().filter(move |(at, _)| *at >= start).map(|(_, l)| l.as_str()) }
    fn pending(&self, start: usize) -> bool { self.after(start).next().is_some() }
}

#[derive(Debug)]
struct Checkpoint { bytes: usize, sha256: String, trimmed_bytes: usize, trimmed_sha256: String, gaps: u64, gap: Option<serde_json::Value> }

fn legacy_checkpoint(db: &Connection) -> Result<Option<Checkpoint>, String> {
    let row: Option<String> = db.query_row("SELECT text FROM legacy WHERE id=1", [], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    let Some(row) = row else { return Ok(None) };
    let old = serde_json::from_str::<serde_json::Value>(&row).map_err(|e| format!("legacy checkpoint: {e}"))?;
    Ok(Some(Checkpoint {
        bytes: old["bytes"].as_u64().ok_or("legacy checkpoint: missing length")? as usize,
        sha256: old["sha256"].as_str().ok_or("legacy checkpoint: missing digest")?.to_string(),
        trimmed_bytes: old["trimmed_bytes"].as_u64().ok_or("legacy checkpoint: missing trimmed length")? as usize,
        trimmed_sha256: old["trimmed_sha256"].as_str().ok_or("legacy checkpoint: missing trimmed digest")?.to_string(),
        gaps: old["gaps"].as_u64().unwrap_or(0),
        gap: old.get("gap").cloned(),
    }))
}

/// Reads the checkpoint, then scans taskq.log once at its offsets.
fn legacy_scan(db: &Connection, install: &Path) -> Result<(LegacyLog, Option<Checkpoint>), String> {
    let old = legacy_checkpoint(db)?;
    let at = old.as_ref().map_or(Vec::new(), |c| vec![c.bytes, c.trimmed_bytes]);
    Ok((legacy_log(install, &at)?, old))
}

fn legacy_log(install: &Path, at: &[usize]) -> Result<LegacyLog, String> {
    use std::io::{BufRead, Seek};
    let fail = |e: std::io::Error| format!("legacy taskq.log: {e}");
    let empty = hash("");
    let mut log = LegacyLog { bytes: 0, sha256: empty.clone(), tail_bytes: 0, tail_sha256: empty.clone(), marks: Vec::new(), telemetry: Vec::new() };
    if at.contains(&0) { log.marks.push((0, empty)); }
    let file = match std::fs::File::open(install.join("taskq.log")) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(log),
        Err(e) => return Err(fail(e)),
    };
    let mut input = std::io::BufReader::with_capacity(64 << 10, file);
    let mut all = ring::digest::Context::new(&ring::digest::SHA256);
    let mut complete = all.clone();
    let (mut offset, mut start, mut held) = (0usize, 0usize, 0usize);
    // Head of the current line without its '\n': enough to classify it and to hold one bounded record.
    let mut line = Vec::new();
    loop {
        let chunk = input.fill_buf().map_err(fail)?;
        if chunk.is_empty() { break; }
        let (n, end) = chunk.iter().position(|c| *c == b'\n').map_or((chunk.len(), false), |i| (i + 1, true));
        all.update(&chunk[..n]);
        let body = &chunk[..n - usize::from(end)];
        line.extend_from_slice(&body[..body.len().min((LINE_MAX + 2).saturating_sub(line.len()))]);
        input.consume(n);
        offset += n;
        if !end { continue; }
        complete = all.clone();
        if at.contains(&offset) { log.marks.push((offset, hex(complete.clone().finish()))); }
        if telemetry(&line) {
            let mut len = offset - start - 1;
            if line.len() == len && line.ends_with(b"\r") { line.pop(); len -= 1; }
            if len > LINE_MAX { return Err("legacy telemetry record exceeds 64 KiB; file kept".into()); }
            held += offset - start;
            if held as u64 > LEGACY_MAX { return Err("legacy telemetry exceeds migration bound; file kept".into()); }
            let text = String::from_utf8(std::mem::take(&mut line)).map_err(|_| "legacy telemetry record is not UTF-8; file kept".to_string())?;
            log.telemetry.push((start, text));
        }
        line.clear();
        start = offset;
    }
    // Only service diagnostics may bypass the profile lock while completing an append.
    if offset > start && telemetry(&line) { return Err("legacy telemetry record truncated; file kept".into()); }
    log.bytes = start;
    log.sha256 = hex(complete.finish());
    // Rotation keeps the newest half; its digest lets the next pass continue the trimmed file.
    let lo = start.saturating_sub((crate::data::LOG_MAX / 2) as usize);
    let mut file = input.into_inner();
    file.seek(std::io::SeekFrom::Start(lo as u64)).map_err(fail)?;
    let mut tail = vec![0; start - lo];
    file.read_exact(&mut tail).map_err(fail)?;
    let tail = tail.iter().position(|c| *c == b'\n').map_or(&tail[..], |i| &tail[i + 1..]);
    log.tail_bytes = tail.len();
    log.tail_sha256 = hex(ring::digest::digest(&ring::digest::SHA256, tail));
    Ok(log)
}

fn hex(digest: ring::digest::Digest) -> String { digest.as_ref().iter().map(|b| format!("{b:02x}")).collect() }

fn hash(text: &str) -> String { hex(ring::digest::digest(&ring::digest::SHA256, text.as_bytes())) }

const TELEMETRY: [&str; 5] = ["taskq-profile ", "taskq-stage ", "taskq-board ", "taskq-transition ", "taskq-requests "];

fn telemetry(line: &[u8]) -> bool { TELEMETRY.iter().any(|p| line.starts_with(p.as_bytes())) }

fn space(db: &Connection, bytes: usize) -> Result<(), String> {
    checkpoint(db)?;
    let page:i64=db.pragma_query_value(None,"page_size",|r|r.get(0)).map_err(|e|e.to_string())?;
    let ceiling:i64=db.pragma_query_value(None,"max_page_count",|r|r.get(0)).map_err(|e|e.to_string())?;
    let pages:i64=db.pragma_query_value(None,"page_count",|r|r.get(0)).map_err(|e|e.to_string())?;
    let free:i64=db.pragma_query_value(None,"freelist_count",|r|r.get(0)).map_err(|e|e.to_string())?;
    let reserve=(bytes as u64).div_ceil(page as u64)*2+32;
    if (pages-free).max(0) as u64+reserve>=ceiling as u64 {
        return Err("legacy import full; source archive preserved; migration required".into());
    }
    Ok(())
}


/// Existing records have no event IDs: track stream position, never deduplicate by record contents.
/// Rewritten or truncated `taskq.log` re-baselines from byte 0 with a recorded gap and logs once (#2237).
fn import(db: &mut Connection, install: &Path, now: u64) -> Result<(), String> {
    let (log, old) = legacy_scan(db, install)?;
    let (prior_gaps, prior_gap) = old.as_ref().map_or((0, None), |c| (c.gaps, c.gap.clone()));
    let (previous, rotated, rebaselined) = match old.as_ref() {
        None => (None, false, None),
        Some(old) => match log.matched(old) {
            Some((n, rotated)) => (Some(n), rotated, None),
            None => (None, false, Some((prior_gaps + 1, old.bytes, old.sha256.clone()))),
        },
    };
    let start = previous.unwrap_or(0);
    if start == log.bytes && previous.is_some() && !rotated && rebaselined.is_none() { return Ok(()); }
    // Store hashes only: service diagnostics are neither telemetry nor retained database payload.
    let mut state_obj = serde_json::json!({"bytes":log.bytes,"sha256":log.sha256,"trimmed_bytes":log.tail_bytes,"trimmed_sha256":log.tail_sha256});
    if let Some((gaps, prev_bytes, ref prev_sha)) = rebaselined {
        state_obj["gaps"] = serde_json::json!(gaps);
        state_obj["gap"] = serde_json::json!({
            "at_ms": now,
            "previous_bytes": prev_bytes,
            "previous_sha256": prev_sha,
            "rebaselined_bytes": log.bytes,
            "rebaselined_sha256": log.sha256,
        });
    } else if prior_gaps > 0 {
        state_obj["gaps"] = serde_json::json!(prior_gaps);
        if let Some(gap) = prior_gap { state_obj["gap"] = gap; }
    }
    let state = state_obj.to_string();
    let added: Vec<_> = log.after(start).collect();
    space(db, added.iter().map(|l| l.len()).sum())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    for line in added { insert(&tx, line, now)?; }
    tx.execute("INSERT INTO legacy(id,text) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET text=excluded.text", [&state]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    checkpoint(db)?;
    if let Some((gaps, prev_bytes, _)) = rebaselined {
        crate::platform::log(&format!(
            "telemetry: legacy taskq.log rewritten or lost ({prev_bytes} -> {} bytes); re-baselined with recorded gap {gaps}; import resumed",
            log.bytes
        ));
    }
    Ok(())
}

/// Metadata-only product records share the installed capture queue and its health.
#[cfg(test)]
pub fn observe(line:&str)->Result<(),String> {
    if line.len()>LINE_MAX {return Err("telemetry append exceeds 64 KiB".into());}
    coreq::telemetry::capture::observe(semantic::observation(line.trim_end())?)
}

/// Live producers submit existing typed records directly; text decoding belongs to historical input.
pub(crate) fn record(sample:Sample)->std::io::Result<()> {
    let prepare=||->Result<serde_json::Value,String> {
        let kind=sample.kind();let original=sample.canonical()?;
        if semantic::encoded_size(kind,&original)?>LINE_MAX {return Err("telemetry append exceeds 64 KiB".into());}
        semantic::encode(kind,original)
    };
    prepare().and_then(coreq::telemetry::capture::observe).map_err(std::io::Error::other)
}

/// One owner uses the existing election and budget; live collectors must be quiesced first.
pub fn migrate(install:&Path)->Result<u64,String> {
    let path=install.join("telemetry.log/universal.sqlite3");
    let _owner=coreq::telemetry::maintenance(&path)?;
    if !has_legacy(install)? {return Ok(0);}
    let mut archive=source::prepare(install)?;
    let target=coreq::telemetry::Store::open_bounded(&path,budget(install,"telemetry.sqlite3")?)?;
    source::import(&mut archive,&target,install)
}

pub(crate) fn visit_samples(install:&Path,kinds:&[&str],accept:impl FnMut(usize,Sample))->Result<(),String> {
    // Include this process's admitted records before taking the committed snapshot.
    tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().cloned()).map(|capture|capture.flush()).transpose()?;
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(),|| {
        let path=install.join("telemetry.log/universal.sqlite3");
        let legacy_lock=legacy_snapshot_lock(install)?;
        let reader=path.try_exists().map_err(|e|e.to_string())?.then(||coreq::telemetry::Reader::open(&path)).transpose()?;
        let cursor=reader.as_ref().map(|r|r.import_cursor("taskq.legacy")).transpose()?.flatten();
        let (rows,checkpoint)=legacy_rows(install,cursor.as_deref(),legacy_lock.as_ref())?;
        drop(legacy_lock);
        if let Some(checkpoint)=checkpoint.filter(|c|c.gaps>0) {
            return Err(format!("legacy import coverage unknown: {} recorded recovery gap(s)",checkpoint.gaps));
        }
        let archive=rows.into_iter().map(|(_,_,line)|line+"\n").collect::<String>();
        semantic::visit_samples(reader.as_ref(),kinds,&archive,accept)
    })
}

pub(crate) fn report_samples(install:&Path,kinds:&[&str])->Result<Vec<Sample>,String> {
    let mut lanes:[Vec<Sample>;3]=Default::default();visit_samples(install,kinds,|lane,sample|lanes[lane].push(sample))?;Ok(lanes.into_iter().flatten().collect())
}

#[cfg(test)]
pub fn report_input(install:&Path)->Result<String,String> {
    let mut text=String::new();for sample in report_samples(install,semantic::KINDS)? {text.push_str(&sample.line()?);text.push('\n');}Ok(text)
}

#[cfg(test)]
pub fn append(install: &Path, line: &str, now: u64) -> Result<(), String> {
    if line.len() > LINE_MAX { return Err("telemetry append exceeds 64 KiB".into()); }
    let mut db = open(install)?;
    let imported = import(&mut db, install, now);
    space(&db, line.len())?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    for line in line.lines() { insert(&tx, line, now)?; }
    tx.commit().map_err(|e| e.to_string())?;
    checkpoint(&db)?;
    imported
}

pub fn read(install: &Path, now: u64) -> Result<String, String> {
    if !has_legacy(install)? {return Ok(String::new());}
    let mut db = open(install)?;
    import(&mut db, install, now)?;
    let mut statement = db.prepare("SELECT line FROM records ORDER BY seq").map_err(|e| e.to_string())?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    let mut text = String::new();
    for row in rows { text.push_str(&row.map_err(|e| e.to_string())?); text.push('\n'); }
    Ok(text)
}

// Match the writer's order: budget lock before SQLite snapshot, never the reverse.
fn legacy_snapshot_lock(install:&Path)->Result<Option<crate::profile::LogLock>,String> {
    if !install.join("telemetry.log/telemetry.sqlite3").try_exists().map_err(|e|e.to_string())? {return Ok(None);}
    crate::profile::read_log_lock(install).map(Some).map_err(|e|format!("legacy snapshot lock: {e}"))
}

/// Transitional archive view; caller holds the shared budget before opening the target snapshot.
fn legacy_rows(install: &Path,cursor:Option<&str>,lock:Option<&crate::profile::LogLock>) -> Result<(Vec<(i64,i64,String)>,Option<Checkpoint>),String> {
    let path=install.join("telemetry.log/telemetry.sqlite3");
    match std::fs::metadata(&path) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound => {
            if cursor.is_some() {return Err("legacy source: imported archive missing; coverage unknown".into());}
            if legacy_log(install,&[])?.pending(0) {return Err("legacy telemetry awaits service import".into());}
            return Ok((Vec::new(),None));
        }
        Err(e)=>return Err(e.to_string()),Ok(_)=>{}
    }
    if lock.is_none() {return Err("legacy archive appeared during snapshot; coverage unknown".into());}
    // #2306: a WAL archive without -wal opens immutable, so a reader that cannot create -shm (worker sandbox) still
    // reads it; the held log lock keeps the writer out. With a -wal the plain read-only open sees its frames.
    let wal=std::path::PathBuf::from(format!("{}-wal",path.display()));
    let db=if wal.try_exists().map_err(|e|e.to_string())? {
        Connection::open_with_flags(&path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    } else {
        let p=crate::platform::plain_path(&path).to_string_lossy().replace('\\',"/").replace('%',"%25").replace('?',"%3f").replace('#',"%23");
        Connection::open_with_flags(format!("file:{}{p}?immutable=1",if p.starts_with('/') {""} else {"/"}),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY|rusqlite::OpenFlags::SQLITE_OPEN_URI)
    }.map_err(|e|e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(1)).map_err(|e|e.to_string())?;
    db.execute_batch("PRAGMA query_only=ON; BEGIN;").map_err(|e|e.to_string())?;
    let app:i64=db.pragma_query_value(None,"application_id",|r|r.get(0)).map_err(|e|e.to_string())?;
    let version:i64=db.pragma_query_value(None,"user_version",|r|r.get(0)).map_err(|e|e.to_string())?;
    if app!=APP||version!=1 {return Err("legacy snapshot: foreign or unsupported schema; preserved".into());}
    let (log,old)=legacy_scan(&db,install)?;
    if log.pending(log.offset(old.as_ref())?) {return Err("legacy telemetry awaits service import".into());}
    let after=source::pending(&db,cursor)?;
    let sql=if after.is_some() {"SELECT seq,retained_ms,line FROM records WHERE telemetry_epoch>?1 OR (telemetry_epoch=?1 AND seq>?2) ORDER BY telemetry_epoch,seq"} else {"SELECT seq,retained_ms,line FROM records WHERE ?1=-1 AND ?2=0 ORDER BY seq"};
    let mut statement=db.prepare(sql).map_err(|e|e.to_string())?;
    let after=after.unwrap_or((-1,0));
    let rows=statement.query_map(params![after.0,after.1],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?
        .collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    drop(statement);drop(db);
    if rows.iter().any(|(seq,received,line)|*seq<=0||*received<0||line.len()>LINE_MAX) {
        return Err("legacy snapshot: invalid retained record".into());
    }
    Ok((rows,old))
}

fn legacy_export(since:Option<u64>,rows:Vec<(i64,i64,String)>,present:bool)->Result<(serde_json::Value,String),String> {
    use serde_json::json;
    if !present {
        return Ok((json!({"state":"absent","samples":0}),String::new()));
    }
    let bounds=rows.first().zip(rows.last()).map(|(first,last)|[first.0,last.0]);
    let mut out=String::new();let mut samples=0;let mut unknown=0;
    let (mut first,mut last)=(None::<u64>,None::<u64>);
    for (seq,received,line) in rows {
        let value=semantic::observation(&line)?;
        if let Some(time)=value["started_ms"].as_u64() {
            if time>i64::MAX as u64 {return Err("legacy snapshot: invalid source time".into());}
            first=Some(first.map_or(time,|old|old.min(time)));last=Some(last.map_or(time,|old|old.max(time)));
        } else {unknown+=1;}
        if since.is_some_and(|since|value["started_ms"].as_u64().is_some_and(|time|time<since)) {continue;}
        out.push_str(&json!({"source":"legacy","seq":seq,"id":null,"received_ms":received,"observation":value}).to_string());out.push('\n');samples+=1;
    }
    Ok((json!({"state":"read","samples":samples,"retained_seq":bounds,"retained_ms":[first,last],"unknown_started_count":unknown,"id":"not-recorded","pruned_count":null,"unknown":"legacy-retention-not-recorded"}),out))
}

/// Read committed metadata without acquiring write permission, migration, retention or network access.
#[tracing::instrument(name="telemetry.export",skip_all)]
pub fn export(install:&Path,since:Option<u64>) -> Result<String,String> {
    // Record the export boundary, not a recursive trace for each decoded telemetry row.
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(),||export_snapshot(install,since))
}

fn export_snapshot(install:&Path,since:Option<u64>) -> Result<String,String> {
    use serde_json::json;
    let legacy_lock=legacy_snapshot_lock(install)?;
    let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
    let cursor=reader.import_cursor("taskq.legacy")?;
    let present=legacy_lock.is_some();
    let (rows,checkpoint)=legacy_rows(install,cursor.as_deref(),legacy_lock.as_ref())?;
    drop(legacy_lock);
    let (mut legacy,old)=legacy_export(since,rows,present)?;
    if let Some(checkpoint)=checkpoint.filter(|c|c.gaps>0) {
        legacy["import_gaps"]=json!(checkpoint.gaps);
        legacy["last_import_gap"]=json!(checkpoint.gap);
        legacy["unknown"]=json!("legacy-import-recovery-gap; legacy-retention-not-recorded");
    }
    let since_i64=since.unwrap_or(0).min(i64::MAX as u64) as i64;
    let mut page=reader.export(since_i64,0,1024)?;
    let mut universal=page.clone();
    for key in ["events","after","next","more","schema"] {universal.as_object_mut().unwrap().remove(key);}
    universal["state"]=json!("read");
    let mut out=String::new();let mut samples=0;
    loop {
        for row in page["events"].as_array().ok_or("invalid telemetry export page")? {
            let mut row=row.clone();row["source"]=json!("universal");out.push_str(&row.to_string());out.push('\n');samples+=1;
        }
        if page["more"]==false {break;}
        page=reader.export(since_i64,page["next"].as_i64().ok_or("invalid telemetry export cursor")?,1024)?;
    }
    universal["samples"]=json!(samples);
    drop(reader); // Source filtering and target rows share one import-cursor snapshot.
    let header=json!({"schema":2,"coverage":"committed-import-cursor-snapshot","since_ms":since,"sources":{"universal":universal,"legacy":legacy}});
    Ok(header.to_string()+"\n"+&out+&old)
}

fn has_legacy(install:&Path)->Result<bool,String> {
    Ok(install.join("telemetry.log/telemetry.sqlite3").try_exists().map_err(|e|e.to_string())?
        || legacy_log(install,&[])?.pending(0))
}

/// Existing service owns archive retention; never create an empty replacement legacy sink.
pub fn prune(install: &Path, now: u64) -> Result<(), String> {
    if !has_legacy(install)? {return Ok(());}
    let mut db = open(install)?;
    import(&mut db, install, now)?;
    if source::preserved(install)? {return checkpoint(&db);}
    loop {
        checkpoint(&db)?;
        let n = db.execute("DELETE FROM records WHERE seq IN (SELECT seq FROM records WHERE retained_ms < ?1 LIMIT 256)", [now.saturating_sub(WINDOW_MS).min(i64::MAX as u64) as i64]).map_err(|e| e.to_string())?;
        if n == 0 { break; }
    }
    // Reuse free pages; no VACUUM copy and no transient second database.
    checkpoint(&db)
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkerEvent {
    pub at_ms: u64,
    pub project: String,
    pub host: String,
    pub runtime: String,
    pub role: String,
    pub event: String,
    pub task: u64,
    pub attempt: u64,
    pub pid: u32,
    pub outcome: String,
    pub reason: String,
}

fn code_token(s: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-') {
            out.push(ch);
            prev_dash = ch == '-';
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 128 { break; }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { fallback.to_string() } else { trimmed.to_string() }
}

fn project_token(s: &str) -> String {
    let valid_part = |part: &str| !part.is_empty() && part.len() <= 128
        && part.bytes().all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
        && !matches!(part, "." | "..");
    if s.len() <= 128 && s.contains('/') && s.split('/').all(valid_part) {
        s.to_string()
    } else {
        "other".to_string()
    }
}

pub fn worker_observation(ev: &WorkerEvent) -> serde_json::Value {
    let outcome = match ev.outcome.as_str() {
        "ok" | "failed" | "refused" | "cancelled" | "retried" | "unknown" | "ask" => ev.outcome.as_str(),
        _ => "unknown",
    };
    let event = code_token(&ev.event, "other");
    let reason = code_token(&ev.reason, "none");
    let error = if outcome == "ok" { "none".to_string() } else { reason.clone() };
    serde_json::json!({
        "product": "taskq",
        "version": null,
        "kind": "worker",
        "operation": event,
        "trace": null,
        "span": null,
        "started_ms": ev.at_ms.min(i64::MAX as u64),
        "project": project_token(&ev.project),
        "host": code_token(&ev.host, "unknown"),
        "runtime": code_token(&ev.runtime, "other"),
        "task": ev.task,
        "attempt": ev.attempt,
        "process_id": ev.pid,
        "source": code_token(&ev.role, "work"),
        "from": event,
        "to": reason,
        "outcome": outcome,
        "error": error,
        "unknown": {
            "version": "not-recorded",
            "trace": "not-recorded",
            "span": "not-recorded"
        }
    })
}

fn decode_worker_observation(v: &serde_json::Value) -> Option<WorkerEvent> {
    if v["kind"].as_str() != Some("worker") { return None; }
    Some(WorkerEvent {
        at_ms: v["started_ms"].as_u64().unwrap_or(0),
        project: v["project"].as_str().unwrap_or("other").to_string(),
        host: v["host"].as_str().unwrap_or("unknown").to_string(),
        runtime: v["runtime"].as_str().unwrap_or("other").to_string(),
        role: v["source"].as_str().unwrap_or("work").to_string(),
        event: v["operation"].as_str().unwrap_or("other").to_string(),
        task: v["task"].as_u64().unwrap_or(0),
        attempt: v["attempt"].as_u64().unwrap_or(0),
        pid: v["process_id"].as_u64().unwrap_or(0) as u32,
        outcome: v["outcome"].as_str().unwrap_or("ok").to_string(),
        reason: v["to"].as_str().unwrap_or("none").to_string(),
    })
}

static WORKER_EVENTS: std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, Vec<WorkerEvent>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

#[allow(clippy::too_many_arguments)]
pub fn record_worker(
    install: &Path,
    project: &str,
    host: &str,
    runtime: &str,
    role: &str,
    event: &str,
    task: u64,
    attempt: u64,
    pid: u32,
    outcome: &str,
    reason: &str,
) {
    let at_ms = crate::profile::now_ms().unwrap_or(0);
    let raw = WorkerEvent {
        at_ms,
        project: project.to_string(),
        host: host.to_string(),
        runtime: runtime.to_string(),
        role: role.to_string(),
        event: event.to_string(),
        task,
        attempt,
        pid,
        outcome: outcome.to_string(),
        reason: reason.to_string(),
    };
    let obs = worker_observation(&raw);
    let normalized = decode_worker_observation(&obs).unwrap_or(raw);
    if let Ok(mut map) = WORKER_EVENTS.lock() {
        let list = map.entry(install.to_path_buf()).or_default();
        if list.len() >= 1024 {
            list.drain(..512);
        }
        list.push(normalized);
    }
    let _ = coreq::telemetry::capture::observe(obs);
}

pub fn worker_events(install: &Path) -> Vec<WorkerEvent> {
    let _ = tracing::dispatcher::get_default(|d| d.downcast_ref::<coreq::telemetry::capture::Capture>().cloned())
        .map(|capture| capture.flush());
    let mut out = Vec::new();
    let path = install.join("telemetry.log/universal.sqlite3");
    if path.try_exists().unwrap_or(false) {
        if let Ok(reader) = tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
            coreq::telemetry::Reader::open(&path)
        }) {
            let mut after = 0;
            while let Ok(page) = reader.export_kinds(&["worker"], 0, after, 512) {
                let Some(rows) = page["events"].as_array() else { break };
                if rows.is_empty() { break; }
                for row in rows {
                    if let Some(ev) = decode_worker_observation(&row["observation"]) {
                        out.push(ev);
                    }
                }
                if page["more"] == false { break; }
                let Some(next) = page["next"].as_i64() else { break };
                after = next;
            }
        }
    }
    if let Ok(map) = WORKER_EVENTS.lock() {
        if let Some(mem) = map.get(install) {
            for ev in mem {
                if !out.contains(ev) {
                    out.push(ev.clone());
                }
            }
        }
    }
    out
}

/// Live worker pid for `(task, attempt, execution)` from telemetry or `.kids`; returns `Some(pid)` while the OS process lives (#2237).
pub fn live_worker_pid(install: &Path, root: Option<&Path>, task: u64, attempt: u64, execution: u64) -> Option<u32> {
    let check_events = |events: &[WorkerEvent]| -> Option<Option<u32>> {
        let last = events.iter().rev().find(|e| {
            e.task == task && (attempt == 0 || e.attempt == 0 || e.attempt == attempt || e.attempt == execution)
                && matches!(e.event.as_str(), "spawn" | "heartbeat" | "slot-hold" | "result" | "alive-after-result" | "exit" | "slot-release" | "kill")
        })?;
        if matches!(last.event.as_str(), "exit" | "slot-release") {
            return Some(None);
        }
        if last.pid > 1 && crate::runtime::alive(last.pid) {
            return Some(Some(last.pid));
        }
        None
    };
    if let Ok(map) = WORKER_EVENTS.lock() {
        if let Some(mem) = map.get(install) {
            if let Some(res) = check_events(mem) {
                return res;
            }
        }
    }
    if let Some(r) = root {
        if let Ok(path) = crate::kids_path(install, r) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok((_, kids)) = crate::data::kids(&text) {
                    if let Some(k) = kids.into_iter().find(|k| {
                        k.task == task && (attempt == 0 || k.attempt == attempt || k.execution == execution || k.execution == attempt)
                    }) {
                        if k.process.pid > 1 && crate::runtime::alive(k.process.pid) {
                            return Some(k.process.pid);
                        }
                    }
                }
            }
        }
    }
    let db_events = worker_events(install);
    if let Some(res) = check_events(&db_events) {
        return res;
    }
    None
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
