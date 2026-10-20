//! Telemetry upload to DashQ (docs/design/standalone-telemetry.md#remote-sync-1353): export rows of the coreq
//! universal store as gzip blocks under the opt-in `taskq-telemetry/1` grant, after a durable cursor. Capture, CLI
//! and service passes never wait on it; it never deletes a row.

use crate::{Res, enrollment, resources, snapshot::Snapshot};
use base64::Engine;
use coreq::enroll::{self, Backoff, Delivery, Https, Wire};
use serde_json::{Value, json};
use std::{fs, io::Write, path::{Path, PathBuf}, thread::JoinHandle, time::{Duration, Instant}};

pub const SCHEMA: &str = "taskq-telemetry/1";
/// Compressed bytes per record: base64 and the envelope stay under coreq's 1 MiB `/ingest` body.
const BLOCK: usize = 512 << 10;
/// Raw export bytes per block before compression.
const RAW: usize = 1 << 20;
/// Blocks read from one Reader snapshot, which is dropped before any network I/O.
const BLOCKS: usize = 8;
/// Seconds of sending per job; then the pass starts the next one.
const RUN: u64 = 30;
/// Pause after catching up or a stop.
const IDLE: u64 = 60;
const STOP: u64 = 300;

/// Whether the active grant lists the telemetry schema; without it no telemetry code path runs (A3).
pub fn granted(active: &Value) -> bool {
    active["schemas"].as_array().is_some_and(|a| a.iter().any(|s| s == SCHEMA))
}

fn cursor_path(install: &Path) -> PathBuf {
    enrollment::dir(install).join("telemetry.json")
}

/// The cursor of `destination`: the last accepted row `seq`, counted `gap_rows`, `last_synced` and the `pending`
/// last seq of a block sent without a receipt, so a retry rebuilds the same block and event id. Another
/// destination's cursor starts over; an unreadable one is unknown and kept.
fn load(install: &Path, destination: &str) -> Res<Value> {
    let fresh = json!({"destination": destination, "seq": 0, "gap_rows": 0, "last_synced": null, "pending": null});
    let c: Value = match fs::read_to_string(cursor_path(install)) {
        Ok(t) => serde_json::from_str(&t).map_err(|_| "cursor-unreadable")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(fresh),
        Err(_) => return Err("cursor-unreadable".into()),
    };
    if c["seq"].as_i64().is_none() || c["gap_rows"].as_u64().is_none() { return Err("cursor-unreadable".into()) }
    Ok(if c["destination"] == destination { c } else { fresh })
}

fn save(install: &Path, c: &Value) -> Res<()> {
    let (path, tmp) = (cursor_path(install), cursor_path(install).with_extension("json.tmp"));
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(c.to_string().as_bytes()).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

struct Block { last: i64, event_id: String, observed: u64, payload: Value, raw: usize }

/// What one Reader snapshot shows after the cursor.
struct Read { blocks: Vec<Block>, gap: u64, base: i64, retained_last: i64, end: bool }

fn gzip(lines: &[(i64, String, i64, String)]) -> Res<Vec<u8>> {
    let mut z = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
    for l in lines { z.write_all(l.3.as_bytes()).map_err(|e| e.to_string())? }
    z.finish().map_err(|e| e.to_string())
}

/// Reads at most [`BLOCKS`] blocks after `seq` (the first ends at `pending` when set). Rows pruned before upload
/// become `gap`; `base` is the cursor after them.
fn read(install: &Path, installation: &str, seq: i64, pending: Option<i64>) -> Res<Read> {
    let reader = coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
    let mut page = reader.export(0, seq, 1024)?;
    let bounds = |p: &Value, i: usize| p["retained_seq"][i].as_i64();
    // a replaced store restarts below the cursor: its rows have new ids, so starting over sends no duplicate
    let seq = if bounds(&page, 1).is_some_and(|l| l < seq) { page = reader.export(0, 0, 1024)?; 0 } else { seq };
    let oldest = bounds(&page, 0).map_or(page["pruned_through"].as_i64().unwrap_or(0) + 1, |f| f);
    let (gap, base) = if oldest > seq + 1 { ((oldest - seq - 1) as u64, oldest - 1) } else { (0, seq) };
    let retained_last = bounds(&page, 1).unwrap_or(base).max(base);
    let mut rows: Vec<(i64, String, i64, String)> = vec![];
    let (mut raw, mut end) = (0, false);
    while raw < RAW * BLOCKS {
        for e in page["events"].as_array().ok_or("export-page-invalid")? {
            let s = e["seq"].as_i64().ok_or("export-page-invalid")?;
            let line = format!("{e}\n");
            raw += line.len();
            rows.push((s, e["id"].as_str().unwrap_or_default().into(), e["received_ms"].as_i64().unwrap_or(0), line));
        }
        if page["more"] != true { end = true; break }
        page = reader.export(0, page["next"].as_i64().ok_or("export-page-invalid")?, 1024)?;
    }
    drop(reader);
    let schema = page["schema"].clone();
    let mut blocks = vec![];
    let mut rest = &rows[..];
    while !rest.is_empty() {
        // a block ends at RAW bytes, at the pending seq, or halves until it compresses under BLOCK
        let mut n = match (blocks.is_empty(), pending) {
            (true, Some(p)) => rest.iter().take_while(|r| r.0 <= p).count().max(1),
            _ => { let mut b = 0; rest.iter().take_while(|r| { b += r.3.len(); b <= RAW }).count().max(1) }
        };
        let mut z = gzip(&rest[..n])?;
        while z.len() > BLOCK && n > 1 { n /= 2; z = gzip(&rest[..n])? }
        if z.len() > BLOCK { return Err("row-over-limit".into()) }
        let (b, first, last) = (&rest[..n], &rest[0], &rest[n - 1]);
        let id = ring::digest::digest(&ring::digest::SHA256, format!("{installation}\0{}\0{}", first.1, last.1).as_bytes());
        let hex: String = id.as_ref()[..16].iter().map(|x| format!("{x:02x}")).collect();
        let raw: usize = b.iter().map(|r| r.3.len()).sum();
        blocks.push(Block {
            last: last.0, event_id: format!("t-{hex}"), observed: u64::try_from(last.2 / 1000).unwrap_or(0), raw,
            payload: json!({"schema": schema, "first": {"seq": first.0, "id": first.1}, "last": {"seq": last.0, "id": last.1},
                "rows": n, "bytes": raw, "gzip": base64::engine::general_purpose::STANDARD.encode(z)}),
        });
        rest = &rest[n..];
    }
    Ok(Read { blocks, gap, base, retained_last, end })
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One job: sends blocks one at a time until caught up, a failure, or `deadline`. Returns the status and the
/// seconds to wait before the next job.
fn run(install: &Path, wire: &mut dyn Wire, backoff: &mut Backoff, deadline: Instant) -> (Value, u64) {
    let status = |state: &str, c: &Value, backlog: Option<(i64, Option<usize>)>, error: Option<&str>| json!({
        "state": state, "last_synced": c["last_synced"], "gap_rows": c["gap_rows"], "last_error": error,
        "backlog_rows": backlog.map(|b| b.0), "backlog_bytes": backlog.and_then(|b| b.1),
    });
    let unknown = |e: &str| (json!({"state": "unknown", "last_error": e}), IDLE);
    let d = enrollment::dir(install);
    let a = match enroll::active(&d) {
        Ok(Some(a)) if granted(&a) => a,
        Ok(_) => return (json!({"state": "off"}), IDLE),
        Err(_) => return unknown("connection-unreadable"),
    };
    let (dest, installation) = (a["destination"].as_str().unwrap_or_default(), a["installation"].as_str().unwrap_or_default());
    let mut c = match load(install, dest) { Ok(c) => c, Err(e) => return unknown(&e) };
    loop {
        let r = match read(install, installation, c["seq"].as_i64().unwrap_or(0), c["pending"].as_i64()) { Ok(r) => r, Err(e) => return unknown(&e) };
        if r.gap > 0 || r.base != c["seq"].as_i64().unwrap_or(0) {
            c["gap_rows"] = json!(c["gap_rows"].as_u64().unwrap_or(0) + r.gap);
            c["seq"] = json!(r.base);
            c["pending"] = Value::Null;
            if save(install, &c).is_err() { return unknown("cursor-unwritable") }
        }
        let backlog = |c: &Value, sent: usize| (r.retained_last - c["seq"].as_i64().unwrap_or(0),
            r.end.then(|| r.blocks.iter().map(|b| b.raw).sum::<usize>() - sent));
        if r.blocks.is_empty() { return (status("idle", &c, Some((0, Some(0))), None), IDLE) }
        let mut sent = 0;
        for b in &r.blocks {
            c["pending"] = json!(b.last);
            if save(install, &c).is_err() { return unknown("cursor-unwritable") }
            let rec = match enroll::stamp(&d, SCHEMA, &b.event_id, b.observed, b.payload.clone()) {
                Ok(rec) => rec,
                Err(e) => return (status("stopped", &c, Some(backlog(&c, sent)), Some(&e)), STOP),
            };
            match enroll::deliver(&d, &[rec], backoff, wire) {
                Ok(x) if x[0]["state"] == "accepted" => {
                    (c["seq"], c["pending"], c["last_synced"]) = (json!(b.last), Value::Null, json!(now()));
                    if save(install, &c).is_err() { return unknown("cursor-unwritable") }
                    sent += b.raw;
                }
                // a rejected block stays unsent: the cursor waits, nothing is dropped
                Ok(x) => return (status("stopped", &c, Some(backlog(&c, sent)), Some(&format!("rejected: {}", x[0]["reason"].as_str().unwrap_or("unknown")))), STOP),
                Err(Delivery::Retry(s)) => return (status("retrying", &c, Some(backlog(&c, sent)), Some("unreachable")), s),
                Err(Delivery::Stopped(e)) => return (status("stopped", &c, Some(backlog(&c, sent)), Some(&e)), STOP),
            }
            if Instant::now() >= deadline { return (status("syncing", &c, Some(backlog(&c, sent)), None), 0) }
        }
        if r.end { return (status("idle", &c, Some(backlog(&c, sent)), None), IDLE) }
    }
}

type Job = JoinHandle<(Backoff, Value, u64)>;

/// The service's telemetry sender beside [`enrollment::Sender`]: one job at a time on its own thread, only on the
/// approved board's service and only under the telemetry grant.
#[derive(Default)]
pub struct Sender {
    job: Option<Job>,
    backoff: Backoff,
    wait_until: u64,
    /// `status.json` `telemetry_sync`.
    pub status: Value,
}

impl Sender {
    /// One step per service pass; reads only the connection file. Returns a log line when the state changes.
    pub fn tick(&mut self, install: &Path, snap: Option<&Snapshot>, now: u64) -> Option<String> {
        self.tick_with(install, snap, now, |i| Box::new(resources::Fenced::new(i, Https { browser: false })))
    }

    fn tick_with(&mut self, install: &Path, snap: Option<&Snapshot>, now: u64, wire: fn(&Path) -> Box<dyn Wire + Send>) -> Option<String> {
        let before = (self.status["state"].clone(), self.status["last_error"].clone());
        if self.job.as_ref().is_some_and(|j| j.is_finished()) {
            let (b, s, wait) = self.job.take().unwrap().join().unwrap_or_else(|_| (Backoff::default(), json!({"state": "unknown", "last_error": "sender-panic"}), IDLE));
            (self.backoff, self.status, self.wait_until) = (b, s, now + wait);
        }
        if self.job.is_none() && now >= self.wait_until {
            match enrollment::state(install) {
                Err(_) => self.status = json!({"state": "unknown", "last_error": "connection-unreadable"}),
                Ok(st) => match st.map(|s| s["active"].clone()).filter(|a| a.is_object() && granted(a)) {
                    None => self.status = json!({"state": "off"}),
                    Some(a) if !enrollment::speaks(&a, snap) => self.status = json!({"state": "off", "reason": "another project's service sends"}),
                    Some(_) => {
                        let (install, mut backoff) = (install.to_path_buf(), std::mem::take(&mut self.backoff));
                        self.status["state"] = json!("syncing");
                        self.job = Some(std::thread::spawn(move || {
                            let (s, wait) = run(&install, &mut *wire(&install), &mut backoff, Instant::now() + Duration::from_secs(RUN));
                            (backoff, s, wait)
                        }));
                    }
                },
            }
        }
        let after = (self.status["state"].clone(), self.status["last_error"].clone());
        (after != before && after.0 != "syncing").then(|| format!("telemetry sync: {}{}", after.0.as_str().unwrap_or("unknown"),
            after.1.as_str().map(|e| format!(" ({e})")).unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, io::Read as _};

    /// DashQ stand-in: stores records by event id (a replay returns the stored receipt), or fails as told.
    #[derive(Default)]
    struct Dash { fail: Option<u16>, lose: bool, sent: Vec<String>, stored: BTreeMap<String, Value> }
    impl Wire for Dash {
        fn post(&mut self, _: &str, route: &str, _: Option<&str>, body: &Value) -> Result<(u16, Value), String> {
            assert_eq!(route, "/ingest");
            let r = &body["records"][0];
            self.sent.push(r["event_id"].as_str().unwrap().into());
            if let Some(code) = self.fail { return Ok((code, json!({"protocol": 1, "error": "revoked", "retry_after": 7}))) }
            self.stored.entry(r["event_id"].as_str().unwrap().into()).or_insert(r["payload"].clone());
            if std::mem::take(&mut self.lose) { return Err("reply lost".into()) }
            Ok((200, json!({"protocol": 1, "receipts": [{"event_id": r["event_id"], "state": "accepted"}]})))
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    fn seqs(d: &Dash) -> Vec<i64> {
        let mut out = vec![];
        for p in d.stored.values() {
            let gz = base64::engine::general_purpose::STANDARD.decode(p["gzip"].as_str().unwrap()).unwrap();
            let mut text = String::new();
            flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut text).unwrap();
            out.extend(text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["seq"].as_i64().unwrap()));
        }
        out.sort();
        out
    }

    fn setup(name: &str, schemas: Value) -> (PathBuf, coreq::telemetry::Store) {
        let install = std::env::temp_dir().join(format!("taskq-tsync-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&install);
        fs::create_dir_all(enrollment::dir(&install)).unwrap();
        let conn = json!({"destination": "https://d/machine", "credential": "c", "generation": 1, "installation": "i",
            "scope": {"name": enrollment::SCHEMA, "value": "github:github.com/o/a"}, "schemas": schemas});
        fs::write(enrollment::dir(&install).join("connection.json"), json!({"installation": "i", "pending": null, "active": conn, "revoke": []}).to_string()).unwrap();
        fs::create_dir_all(install.join("telemetry.log")).unwrap();
        let store = coreq::telemetry::Store::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
        (install, store)
    }

    fn append(store: &coreq::telemetry::Store, n: usize) {
        let e = || coreq::telemetry::Event::new(json!({"product": "taskq", "version": "1.0.0", "operation": "op", "kind": "operation",
            "trace": "t", "span": "s", "started_ms": 1000, "duration_us": 10, "outcome": "ok", "error": "none"})).unwrap();
        for c in (0..n).collect::<Vec<_>>().chunks(1000) { store.append(&c.iter().map(|_| e()).collect::<Vec<_>>()).unwrap(); }
    }

    fn go(install: &Path, d: &mut Dash) -> (Value, u64) {
        run(install, d, &mut Backoff::default(), Instant::now() + Duration::from_secs(60))
    }

    // Chain: grant -> blocks after the cursor -> receipts -> restart -> lost reply -> 429/5xx -> revoke.
    // Risk: duplicate or missing rows across restarts and retries, a cursor advanced without a receipt, or sending
    // without the telemetry grant (A2-A4).
    #[test]
    fn sync_resumes_from_the_cursor_without_duplicates_or_gaps() {
        let (install, store) = setup("nogrant", json!([enrollment::SCHEMA]));
        append(&store, 3000);
        let mut d = Dash::default();
        assert_eq!(go(&install, &mut d).0, json!({"state": "off"}), "no grant: off");
        assert!(d.sent.is_empty() && !cursor_path(&install).exists(), "no grant: no /ingest call, no cursor");
        drop(store);
        fs::remove_dir_all(install).unwrap();
        let (install, store) = setup("resume", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 3000);
        let (s, wait) = go(&install, &mut d);
        assert_eq!((s["state"].as_str(), s["backlog_rows"].as_i64(), s["backlog_bytes"].as_u64(), s["gap_rows"].as_u64(), wait), (Some("idle"), Some(0), Some(0), Some(0), IDLE));
        assert_eq!(seqs(&d), (1..=3000).collect::<Vec<_>>());
        // restart: a new job reads the cursor file; only new rows go, the last block's reply is lost
        append(&store, 500);
        d.lose = true;
        let (s, _) = go(&install, &mut d);
        assert_eq!((s["state"].as_str(), s["backlog_rows"].as_i64()), (Some("retrying"), Some(500)), "lost reply: cursor waits");
        let lost = d.sent.last().cloned().unwrap();
        append(&store, 10);
        assert_eq!(go(&install, &mut d).0["state"], "idle");
        assert_eq!(d.sent.iter().filter(|e| **e == lost).count(), 2, "the retry repeats the pending block's event id");
        assert_eq!(seqs(&d), (1..=3510).collect::<Vec<_>>(), "no duplicate or missing row");
        // 429 and 5xx back off with the cursor kept; 401 stops and ends the grant: nothing more is sent
        append(&store, 5);
        for (code, want) in [(429, "retrying"), (503, "retrying"), (401, "stopped")] {
            d.fail = Some(code);
            let (s, wait) = go(&install, &mut d);
            assert_eq!((s["state"].as_str(), s["backlog_rows"].as_i64()), (Some(want), Some(5)), "{code}");
            if code == 429 { assert_eq!(wait, 7, "Retry-After wins") }
        }
        let n = d.sent.len();
        assert_eq!(go(&install, &mut d).0["state"], "off", "revoked: off");
        assert_eq!(d.sent.len(), n);
        fs::remove_dir_all(install).unwrap();
    }

    // Chain: retention or capacity pruning before upload -> next job. Risk: pruned rows resent, invented or silently
    // lost; sync deleting rows itself (S4).
    #[test]
    fn rows_pruned_before_upload_count_as_gap() {
        let (install, store) = setup("gap", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 100);
        let mut d = Dash::default();
        go(&install, &mut d);
        append(&store, 300);
        drop(store);
        let db = rusqlite::Connection::open(install.join("telemetry.log/universal.sqlite3")).unwrap();
        db.execute("DELETE FROM events WHERE seq <= 250", []).unwrap();
        drop(db);
        let (s, _) = go(&install, &mut d);
        assert_eq!((s["state"].as_str(), s["gap_rows"].as_u64()), (Some("idle"), Some(150)));
        assert_eq!(seqs(&d), (1..=100).chain(251..=400).collect::<Vec<_>>());
        let r = coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3")).unwrap();
        assert_eq!(r.export(0, 0, 1).unwrap()["retained_seq"], json!([251, 400]), "sync deletes nothing");
        drop(r);
        // an unreadable cursor is unknown and kept
        fs::write(cursor_path(&install), "x").unwrap();
        assert_eq!(go(&install, &mut d).0, json!({"state": "unknown", "last_error": "cursor-unreadable"}));
        assert_eq!(fs::read_to_string(cursor_path(&install)).unwrap(), "x");
        fs::remove_dir_all(install).unwrap();
    }

    struct Slow;
    impl Wire for Slow {
        fn post(&mut self, _: &str, _: &str, _: Option<&str>, _: &Value) -> Result<(u16, Value), String> {
            std::thread::sleep(Duration::from_secs(3));
            Err("timeout".into())
        }
        fn now(&self) -> u64 { 0 }
        fn sleep(&mut self, _: u64) {}
        fn show(&mut self, _: &str, _: &str) {}
    }

    fn snap(repo: &str) -> Snapshot {
        Snapshot { source: "host", observed: 1_700_000_000, reason: None, board: json!({"kind": "github", "host": "github.com", "repo": repo}), tasks: vec![], hosts: vec![] }
    }

    // Chain: service pass -> tick -> sender thread on a slow DashQ. Risk: a slow network delaying the pass that
    // launches workers (A4), or another project's service sending.
    #[test]
    fn tick_never_waits_on_the_network() {
        let (install, store) = setup("tick", json!([enrollment::SCHEMA, SCHEMA]));
        append(&store, 10);
        let mut s = Sender::default();
        s.tick_with(&install, Some(&snap("o/b")), 1, |_| Box::new(Slow));
        assert!(s.job.is_none() && s.status["state"] == "off", "another board: off");
        let t = Instant::now();
        s.tick_with(&install, Some(&snap("o/a")), 1, |_| Box::new(Slow));
        assert!(s.job.is_some() && t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        s.tick_with(&install, Some(&snap("o/a")), 2, |_| Box::new(Slow));
        assert!(t.elapsed() < Duration::from_secs(1) && s.status["state"] == "syncing");
        while !s.job.as_ref().unwrap().is_finished() { std::thread::sleep(Duration::from_millis(50)) }
        assert_eq!(s.tick_with(&install, Some(&snap("o/a")), 3, |_| Box::new(Slow)).as_deref(), Some("telemetry sync: retrying (unreachable)"));
        assert_eq!(s.status["backlog_rows"], 10);
        fs::remove_dir_all(install).unwrap();
    }
}
