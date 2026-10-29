//! Host link ([detail](../docs/design/host-link.md#contract-detail), #1832): a host reads one bounded page of a live
//! worker session on another host of its board through DashQ's blind relay (dashq#234). Every relay body is sealed
//! end to end: static X25519 installation keys (RFC 7748, below; ring offers no static X25519), HKDF-SHA256 and
//! ChaCha20-Poly1305 from ring. Pull only: one request names one attempt and one cursor and gets one page; nothing is
//! kept beyond the key, and a lost message is asked again by the viewer's next read.
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::{aead, hkdf, rand::{SecureRandom, SystemRandom}};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, process::Command, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::{Duration, Instant}};
type Res<T> = Result<T, String>;

pub const SEND: &str = "/link/v1/send";
pub const RECV: &str = "/link/v1/recv";
/// Principle 6: a host that does not answer within this reads unknown.
pub const WAIT: Duration = Duration::from_secs(5);
/// dashq#234 A1: a relay body is base64 of at most 256 KiB.
const BODY: usize = 256 * 1024;
/// The page JSON that still fits a sealed base64 body with its reply envelope.
pub const PAGE: usize = 180 * 1024;
/// dashq#234 A3: a relay message lives 60 s; a reply nobody fetched is removed after the same time.
const TTL: u64 = 60;
const VERSION: u8 = 1;
const INFO: &[u8] = b"taskq host link 1";

// --- X25519 (RFC 7748), field elements mod 2^255-19 as 16 signed 16-bit limbs (TweetNaCl layout) ---

type Gf = [i64; 16];
const A24: Gf = [0xdb41, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn carry(o: &mut Gf) {
    for i in 0..16 {
        o[i] += 1 << 16;
        let c = o[i] >> 16;
        if i < 15 { o[i + 1] += c - 1 } else { o[0] += 38 * (c - 1) }
        o[i] -= c << 16;
    }
}

/// Constant-time swap of `p` and `q` when `b` is 1.
fn swap(p: &mut Gf, q: &mut Gf, b: i64) {
    let c = !(b - 1);
    for i in 0..16 {
        let t = c & (p[i] ^ q[i]);
        p[i] ^= t;
        q[i] ^= t;
    }
}

fn add(a: &Gf, b: &Gf) -> Gf { std::array::from_fn(|i| a[i] + b[i]) }
fn sub(a: &Gf, b: &Gf) -> Gf { std::array::from_fn(|i| a[i] - b[i]) }

fn mul(a: &Gf, b: &Gf) -> Gf {
    let mut t = [0i64; 31];
    for i in 0..16 { for j in 0..16 { t[i + j] += a[i] * b[j]; } }
    for i in 0..15 { t[i] += 38 * t[i + 16]; }
    let mut o: Gf = std::array::from_fn(|i| t[i]);
    carry(&mut o);
    carry(&mut o);
    o
}

fn invert(i: &Gf) -> Gf {
    let mut c = *i;
    for a in (0..=253).rev() {
        c = mul(&c, &c);
        if a != 2 && a != 4 { c = mul(&c, i); }
    }
    c
}

fn pack(n: &Gf) -> [u8; 32] {
    let mut t = *n;
    for _ in 0..3 { carry(&mut t); }
    for _ in 0..2 {
        let mut m = [0i64; 16];
        m[0] = t[0] - 0xffed;
        for i in 1..15 {
            m[i] = t[i] - 0xffff - ((m[i - 1] >> 16) & 1);
            m[i - 1] &= 0xffff;
        }
        m[15] = t[15] - 0x7fff - ((m[14] >> 16) & 1);
        let b = (m[15] >> 16) & 1;
        m[14] &= 0xffff;
        swap(&mut t, &mut m, 1 - b);
    }
    let mut o = [0u8; 32];
    for i in 0..16 {
        o[2 * i] = (t[i] & 0xff) as u8;
        o[2 * i + 1] = (t[i] >> 8) as u8;
    }
    o
}

/// RFC 7748 X25519: scalar `k` times the u-coordinate `u`, with a constant-time Montgomery ladder.
pub fn x25519(k: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    let mut z = *k;
    z[31] = (k[31] & 127) | 64;
    z[0] &= 248;
    let mut x: Gf = std::array::from_fn(|i| u[2 * i] as i64 + ((u[2 * i + 1] as i64) << 8));
    x[15] &= 0x7fff;
    let (mut a, mut b, mut c, mut d) = ([0i64; 16], x, [0i64; 16], [0i64; 16]);
    a[0] = 1;
    d[0] = 1;
    for i in (0..=254).rev() {
        let r = ((z[i >> 3] >> (i & 7)) & 1) as i64;
        swap(&mut a, &mut b, r);
        swap(&mut c, &mut d, r);
        let e = add(&a, &c);
        a = sub(&a, &c);
        c = add(&b, &d);
        b = sub(&b, &d);
        d = mul(&e, &e);
        let f = mul(&a, &a);
        a = mul(&c, &a);
        c = mul(&b, &e);
        let e = add(&a, &c);
        a = sub(&a, &c);
        b = mul(&a, &a);
        c = sub(&d, &f);
        a = mul(&c, &A24);
        a = add(&a, &d);
        c = mul(&c, &a);
        a = mul(&d, &f);
        d = mul(&b, &x);
        b = mul(&e, &e);
        swap(&mut a, &mut b, r);
        swap(&mut c, &mut d, r);
    }
    pack(&mul(&a, &invert(&c)))
}

// --- keys and sealing ---

/// Principle 1: this installation's key pair; the secret never leaves `<install>/link-key`.
pub struct Key { secret: [u8; 32], pub public: [u8; 32] }

impl Key {
    fn from(secret: [u8; 32]) -> Self {
        let mut base = [0u8; 32];
        base[0] = 9;
        Key { public: x25519(&secret, &base), secret }
    }
}

/// The key pair in `<install>/link-key`, created once like the installation id; a lost key is a new key, published
/// with the next heartbeat.
pub fn key(install: &Path) -> Res<Key> {
    let hex = crate::enrollment::random_file(install, "link-key")?;
    Ok(Key::from(unhex(&hex).ok_or("link-key: corrupt; file kept")?))
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|b| format!("{b:02x}")).collect() }

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) { return None }
    let mut o = [0u8; 32];
    for (i, b) in o.iter_mut().enumerate() { *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?; }
    Some(o)
}

fn cipher(secret: &[u8; 32], peer: &[u8; 32], salt: &[u8], from: &[u8; 32], to: &[u8; 32]) -> Res<aead::LessSafeKey> {
    let shared = x25519(secret, peer);
    // a low-order peer key gives an all-zero secret that anyone can compute
    if shared == [0; 32] { return Err("host link key invalid".into()) }
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(&shared);
    let info = [INFO, from.as_slice(), to.as_slice()];
    let okm = prk.expand(&info, &aead::CHACHA20_POLY1305).map_err(|_| "host link key derivation failed")?;
    Ok(aead::LessSafeKey::new(okm.into()))
}

/// One relay body for `to`: version, sender key, random salt, then the ChaCha20-Poly1305 sealed text. The salt makes
/// every message key new, so the nonce can stay zero.
pub fn seal(key: &Key, to: &[u8; 32], plain: &[u8]) -> Res<String> {
    let mut salt = [0u8; 32];
    SystemRandom::new().fill(&mut salt).map_err(|_| "host link: randomness unavailable")?;
    let mut data = plain.to_vec();
    cipher(&key.secret, to, &salt, &key.public, to)?
        .seal_in_place_append_tag(aead::Nonce::assume_unique_for_key([0; 12]), aead::Aad::from([VERSION]), &mut data)
        .map_err(|_| "host link seal failed")?;
    let mut out = vec![VERSION];
    out.extend(key.public);
    out.extend(salt);
    out.extend(data);
    Ok(STANDARD.encode(out))
}

/// The sender key and text of a body sealed for `key`; anything else, or any changed byte, is refused.
pub fn open(key: &Key, body: &str) -> Res<([u8; 32], Vec<u8>)> {
    let raw = STANDARD.decode(body).map_err(|_| "host link message unreadable")?;
    if raw.len() < 65 + 16 || raw[0] != VERSION { return Err("host link message unreadable".into()) }
    let sender: [u8; 32] = raw[1..33].try_into().unwrap();
    let mut data = raw[65..].to_vec();
    let plain = cipher(&key.secret, &sender, &raw[33..65], &sender, &key.public)?
        .open_in_place(aead::Nonce::assume_unique_for_key([0; 12]), aead::Aad::from([VERSION]), &mut data)
        .map_err(|_| "host link message not for this host or altered")?;
    Ok((sender, plain.to_vec()))
}

// --- requests and peers ---

/// Principle 3: one request names one attempt of one task on one board and one D4 cursor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub board: String,
    pub task: u64,
    pub attempt: u64,
    pub before: Option<u64>,
    pub after: Option<u64>,
}

/// A host of the board with a live heartbeat `link <installation-id> <public key>` line.
#[derive(Clone, Debug, PartialEq)]
pub struct Peer { pub host: String, pub installation: String, pub key: [u8; 32] }

/// The heartbeat line of this installation (§ 4 step 3).
pub fn line(installation: &str, key: &Key) -> String { format!("link {installation} {}", hex(&key.public)) }

/// Principle 2: the live link peers in a `status.json`: trusted heartbeats no older than `ttl` at its board read, and
/// only `allow` hosts when the project lists them (`taskq.json` `link_hosts`).
pub fn peers(status: &Value, ttl: i64, allow: Option<&[String]>) -> Vec<Peer> {
    let observed = status["observed"].as_i64().unwrap_or(0);
    status["hosts"].as_array().into_iter().flatten().filter_map(|h| {
        let host = h["name"].as_str()?;
        let updated = h["updated"].as_i64()?;
        if !crate::snapshot::live(observed - updated, ttl) || allow.is_some_and(|a| !a.iter().any(|x| x == host)) { return None }
        let (installation, key) = h["link"].as_str()?.split_once(' ')?;
        unhex(installation)?;
        Some(Peer { host: host.into(), installation: installation.into(), key: unhex(key)? })
    }).collect()
}

/// `taskq.json` `link_hosts`: the hosts this project serves; absent serves every live host of the board.
pub fn allowed(raw: &Value) -> Res<Option<Vec<String>>> {
    match &raw["link_hosts"] {
        Value::Null => Ok(None),
        Value::Array(a) => a.iter().map(|v| v.as_str().map(String::from).ok_or_else(|| "taskq.json: link_hosts must list host names".to_string())).collect::<Res<_>>().map(Some),
        _ => Err("taskq.json: link_hosts must list host names".into()),
    }
}

fn id_ok(id: &str) -> bool { id.len() == 32 && id.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }

/// D4 over the relay: a page over [`PAGE`] keeps whole records from its cursor side (`after`: the oldest, else the
/// newest) and moves its far cursor to the first record left out, so the next read continues there. A single record
/// over the bound keeps its items with shortened text, marked `truncated`.
pub fn fit(mut page: Value, forward: bool) -> Value {
    if page.to_string().len() <= PAGE { return page }
    let items = page["items"].take();
    let room = PAGE.saturating_sub(page.to_string().len());
    let mut records: Vec<(Value, Vec<Value>)> = vec![];
    for i in items.as_array().into_iter().flatten() {
        match records.last_mut() {
            Some((at, r)) if *at == i["at"] => r.push(i.clone()),
            _ => records.push((i["at"].clone(), vec![i.clone()])),
        }
    }
    if !forward { records.reverse() }
    let size = |r: &[Value]| r.iter().map(|i| i.to_string().len() + 1).sum::<usize>();
    let (mut kept, mut used) = (0, 0);
    while kept < records.len() && used + size(&records[kept].1) <= room { used += size(&records[kept].1); kept += 1; }
    if kept == 0 && !records.is_empty() {
        let each = room / records[0].1.len().max(1) / 2;
        for i in &mut records[0].1 {
            let text: String = i["text"].as_str().unwrap_or_default().chars().take(each / 2).collect();
            i["text"] = json!(text);
            i["truncated"] = json!(true);
        }
        kept = 1;
    }
    if kept < records.len() { page[if forward { "end" } else { "start" }] = records[if forward { kept } else { kept - 1 }].0.clone(); }
    let mut out: Vec<Value> = records.into_iter().take(kept).flat_map(|r| r.1).collect();
    if !forward {
        out.reverse();
        out.sort_by_key(|i| (i["at"].as_u64(), i["i"].as_u64()));
    }
    page["items"] = json!(out);
    page
}

// --- relay ---

/// dashq#234: send one opaque body to an installation, and long-poll this installation's messages.
pub trait Relay {
    fn send(&mut self, to: &str, id: &str, body: &str) -> Res<()>;
    fn recv(&mut self) -> Res<Vec<Value>>;
}

/// The relay of the active DashQ connection over the OS `curl`, like the board and connection calls: URL, credential
/// and body go on stdin, never argv.
pub struct Dash { destination: String, credential: String }

impl Dash {
    /// From the connection file only (#1328: no coreq load on a read path).
    pub fn new(install: &Path) -> Res<(Self, String)> {
        let s = crate::enrollment::state(install)?.ok_or("no DashQ connection on this host")?;
        let a = &s["active"];
        let text = |v: &Value| v.as_str().filter(|t| !t.is_empty() && !t.chars().any(char::is_control)).map(String::from);
        match (text(&a["destination"]), text(&a["credential"]), text(&s["installation"])) {
            (Some(destination), Some(credential), Some(installation)) => Ok((Dash { destination, credential }, installation)),
            _ => Err("no active DashQ connection on this host".into()),
        }
    }

    fn post(&self, route: &str, body: &Value, wait: u64) -> Res<(u16, Value)> {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let cfg = format!("url = \"{}{route}\"\nrequest = \"POST\"\nheader = \"Content-Type: application/json\"\nheader = \"Authorization: Bearer {}\"\ndata-binary = \"{}\"\nmax-time = {wait}\n",
            esc(self.destination.trim_end_matches('/')), esc(&self.credential), esc(&body.to_string()));
        let out = match coreq::process::exchange("link.http", Command::new("curl").args(["-sS", "--proto", "=https", "-w", "\n%{http_code}", "--config", "-"]),
            cfg.as_bytes(), Duration::from_secs(wait + 5)) {
            Ok(o) | Err(coreq::process::Failure::Exit(o)) => o,
            Err(e) => return Err(format!("curl: {e}")),
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
        let code: u16 = code.trim().parse().ok().filter(|c| *c != 0).ok_or("relay unreachable")?;
        Ok((code, serde_json::from_str(body).unwrap_or(Value::Null)))
    }
}

/// dashq 0.18.0 request bodies: every call names `protocol` 1; a receive waits at most 25 s.
fn send_request(to: &str, id: &str, body: &str) -> Value { json!({"protocol": 1, "to": to, "id": id, "body": body}) }
fn recv_request() -> Value { json!({"protocol": 1, "wait": 25}) }

/// A refusal with the relay's error code (`scope`, `cap`, `rate_limited`, ...) when it names one.
fn refused(call: &str, code: u16, v: &Value) -> String {
    match v["error"].as_str() { Some(e) => format!("relay {call} http {code} {e}"), None => format!("relay {call} http {code}") }
}

impl Relay for Dash {
    fn send(&mut self, to: &str, id: &str, body: &str) -> Res<()> {
        match self.post(SEND, &send_request(to, id, body), WAIT.as_secs())? {
            (200..=299, _) => Ok(()),
            (code, v) => Err(refused("send", code, &v)),
        }
    }

    fn recv(&mut self) -> Res<Vec<Value>> {
        match self.post(RECV, &recv_request(), 40)? {
            (200..=299, v) => Ok(v.get("messages").unwrap_or(&v).as_array().cloned().unwrap_or_default()),
            (code, v) => Err(refused("recv", code, &v)),
        }
    }
}

// --- viewer and server ---

/// Replies wait here, still sealed, for the viewer that asked (`<id>.wait`), at most [`TTL`].
fn spool(install: &Path) -> PathBuf { install.join("link") }

/// The viewer side: asks `peer` for one page and waits [`WAIT`] for its sealed reply, which this host's listener
/// leaves in the spool. No answer, a refused reply or an unreadable one is unknown, never an empty page.
pub fn ask(install: &Path, key: &Key, peer: &Peer, read: &Read, relay: &mut dyn Relay) -> Res<Value> {
    let mut id = [0u8; 16];
    SystemRandom::new().fill(&mut id).map_err(|_| "host link: randomness unavailable")?;
    let id = hex(&id);
    let dir = spool(install);
    fs::create_dir_all(&dir).map_err(|e| format!("host link spool: {e}"))?;
    let (wait, reply) = (dir.join(format!("{id}.wait")), dir.join(format!("{id}.reply")));
    fs::write(&wait, "").map_err(|e| format!("host link spool: {e}"))?;
    let began = Instant::now();
    let result = (|| {
        let mut request = serde_json::to_value(read).map_err(|e| e.to_string())?;
        request["kind"] = json!("read");
        request["id"] = json!(id);
        relay.send(&peer.installation, &id, &seal(key, &peer.key, request.to_string().as_bytes())?)
            .map_err(|e| format!("host {} unreachable: {e}", peer.host))?;
        loop {
            if let Ok(body) = fs::read_to_string(&reply) {
                let (sender, plain) = open(key, &body)?;
                let v: Value = serde_json::from_slice(&plain).map_err(|_| "host link reply unreadable")?;
                if sender != peer.key || v["kind"] != "page" || v["id"] != id.as_str() { return Err(format!("host {} reply refused", peer.host)) }
                return match v["state"].as_str() {
                    Some("accepted") if v["value"].is_object() => Ok(v["value"].clone()),
                    _ => Err(format!("{} on {}", v["reason"].as_str().unwrap_or("worker read unknown"), peer.host)),
                };
            }
            if began.elapsed() >= WAIT { return Err(format!("host {} did not answer within {} s", peer.host, WAIT.as_secs())) }
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    let _ = fs::remove_file(&wait);
    let _ = fs::remove_file(&reply);
    result
}

/// One relay message on the listening host. A `read` from a live peer of the board (`from` and key both match its
/// heartbeat) gets one sealed page from `serve`; a `page` goes to the spool only while its viewer waits. Everything
/// else is dropped.
pub fn handle(install: &Path, key: &Key, msg: &Value, peers: &[Peer], relay: &mut dyn Relay, serve: impl FnOnce(&Read) -> Res<Value>) -> Res<()> {
    let (from, id, body) = (msg["from"].as_str().unwrap_or(""), msg["id"].as_str().unwrap_or(""), msg["body"].as_str().unwrap_or(""));
    if !id_ok(id) { return Err("host link message id invalid".into()) }
    let (sender, plain) = open(key, body)?;
    let mut v: Value = serde_json::from_slice(&plain).map_err(|_| "host link message unreadable")?;
    if v["id"] != id { return Err("host link message id mismatch".into()) }
    match v["kind"].as_str() {
        Some("read") => {
            let peer = peers.iter().find(|p| p.key == sender && p.installation == from).ok_or("host link request from no live host of this board")?;
            let o = v.as_object_mut().unwrap();
            o.remove("kind");
            o.remove("id");
            let read: Read = serde_json::from_value(v).map_err(|_| "host link request unreadable")?;
            let mut reply = match serve(&read) {
                Ok(page) => json!({"kind": "page", "id": id, "state": "accepted", "value": page}),
                Err(reason) => json!({"kind": "page", "id": id, "state": "unknown", "reason": reason}),
            };
            let mut body = seal(key, &peer.key, reply.to_string().as_bytes())?;
            if body.len() > BODY {
                reply = json!({"kind": "page", "id": id, "state": "unknown", "reason": "worker page over the host link bound"});
                body = seal(key, &peer.key, reply.to_string().as_bytes())?;
            }
            relay.send(from, id, &body)
        }
        Some("page") => {
            let dir = spool(install);
            if !dir.join(format!("{id}.wait")).exists() { return Ok(()) }
            let temp = dir.join(format!("{id}.tmp"));
            fs::write(&temp, body).and_then(|_| fs::rename(&temp, dir.join(format!("{id}.reply")))).map_err(|e| format!("host link spool: {e}"))
        }
        _ => Err("host link message kind unknown".into()),
    }
}

/// Spool entries older than [`TTL`]: a viewer that stopped waiting.
fn sweep(install: &Path) {
    for e in fs::read_dir(spool(install)).into_iter().flatten().flatten() {
        if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > TTL) {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// The listener: one long-poll thread per host (dashq#234), run by the project thread whose board the DashQ grant
/// approves, like the snapshot sender.
#[derive(Default)]
pub struct Server { job: Option<(std::thread::JoinHandle<()>, Arc<AtomicBool>)>, line: String }

impl Server {
    /// One step per service pass: (re)starts the listener and returns the heartbeat `link` line, empty when this
    /// project does not speak for the connection (its listener then stops after the current long poll).
    pub fn tick(&mut self, install: &Path, root: &Path, snap: Option<&crate::snapshot::Snapshot>) -> &str {
        let speaks = crate::enrollment::state(install).ok().flatten().filter(|s| crate::enrollment::speaks(&s["active"], snap));
        let Some(s) = speaks else {
            if let Some((_, stop)) = self.job.take() { stop.store(true, Ordering::Relaxed) }
            self.line.clear();
            return &self.line;
        };
        if self.job.as_ref().is_none_or(|j| j.0.is_finished()) {
            let (install, root, stop) = (install.to_path_buf(), root.to_path_buf(), Arc::new(AtomicBool::new(false)));
            let flag = stop.clone();
            self.job = Some((std::thread::spawn(move || listen(&install, &root, &flag)), stop));
        }
        self.line = match (s["installation"].as_str(), key(install)) {
            (Some(i), Ok(k)) => line(i, &k),
            _ => String::new(),
        };
        &self.line
    }
}

fn listen(install: &Path, root: &Path, stop: &AtomicBool) {
    let mut backoff = 0;
    let mut said = String::new();
    let mut say = |text: String| if text != said { crate::platform::log(&format!("host link: {text}")); said = text; };
    while !stop.load(Ordering::Relaxed) {
        let Ok((mut relay, _)) = Dash::new(install) else { say("stopped: no DashQ connection".into()); return };
        let key = match key(install) { Ok(k) => k, Err(e) => { say(format!("stopped: {e}")); return } };
        sweep(install);
        match relay.recv() {
            Ok(msgs) => {
                backoff = 0;
                say("listening".into());
                let (status, ttl, allow) = match (crate::data::json(&crate::dot(root).join("status.json")), crate::load(root)) {
                    (Ok(Some(s)), Ok((cfg, raw))) => (s, cfg.ttl, allowed(&raw).unwrap_or(Some(vec![]))),
                    _ => (Value::Null, 0, Some(vec![])),
                };
                let peers = peers(&status, ttl, allow.as_deref());
                for m in msgs {
                    if let Err(e) = handle(install, &key, &m, &peers, &mut relay, |r| crate::conversations::serve(install, r)) { say(format!("message dropped: {e}")); }
                }
            }
            Err(e) => {
                backoff = (backoff * 2).clamp(5, 300);
                say(format!("{e}; retry in {backoff} s"));
                std::thread::sleep(Duration::from_secs(backoff));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(s: &str) -> [u8; 32] { unhex(s).unwrap() }

    // RFC 7748 § 5.2 and § 6.1 vectors, plus ring's ephemeral X25519 as a second implementation: a wrong ladder
    // would derive keys no peer shares.
    #[test]
    fn x25519_matches_rfc_7748_and_ring() {
        assert_eq!(hex(&x25519(&bytes("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"), &bytes("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"))),
            "c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552");
        let alice = Key::from(bytes("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"));
        let bob = Key::from(bytes("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb"));
        assert_eq!(hex(&alice.public), "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a");
        assert_eq!(hex(&bob.public), "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f");
        let shared = "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742";
        assert_eq!((hex(&x25519(&alice.secret, &bob.public)), hex(&x25519(&bob.secret, &alice.public))), (shared.into(), shared.into()));
        let rng = SystemRandom::new();
        for _ in 0..4 {
            let eph = ring::agreement::EphemeralPrivateKey::generate(&ring::agreement::X25519, &rng).unwrap();
            let public: [u8; 32] = eph.compute_public_key().unwrap().as_ref().try_into().unwrap();
            let theirs = ring::agreement::agree_ephemeral(eph, &ring::agreement::UnparsedPublicKey::new(&ring::agreement::X25519, alice.public), |k| k.to_vec()).unwrap();
            assert_eq!(x25519(&alice.secret, &public).to_vec(), theirs);
        }
    }

    // Principle 1: only the addressed key opens a body, a changed byte or a low-order key is refused, and the
    // opener learns the sender's key.
    #[test]
    fn sealed_bodies_open_only_for_their_host_and_name_the_sender() {
        let (a, b, c) = (Key::from([1; 32]), Key::from([2; 32]), Key::from([3; 32]));
        let body = seal(&a, &b.public, b"worker text").unwrap();
        assert!(!String::from_utf8_lossy(&STANDARD.decode(&body).unwrap()).contains("worker text"), "the relay sees no text");
        assert_eq!(open(&b, &body).unwrap(), (a.public, b"worker text".to_vec()));
        assert!(open(&c, &body).is_err(), "another host cannot read it");
        let mut raw = STANDARD.decode(&body).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        assert!(open(&b, &STANDARD.encode(&raw)).is_err(), "altered");
        raw[last] ^= 1;
        raw[1..33].copy_from_slice(&c.public);
        assert!(open(&b, &STANDARD.encode(&raw)).is_err(), "a swapped sender key fails authentication");
        assert!(seal(&a, &[0; 32], b"x").is_err(), "low-order key");
        assert_ne!(seal(&a, &b.public, b"x").unwrap(), seal(&a, &b.public, b"x").unwrap(), "fresh salt per message");
    }

    #[test]
    fn key_file_is_stable_and_corruption_is_kept() {
        let install = std::env::temp_dir().join(format!("taskq-link-key-{}", crate::runtime::uuid()));
        let first = key(&install).unwrap().public;
        assert_eq!(key(&install).unwrap().public, first);
        fs::write(install.join("link-key"), "broken").unwrap();
        assert!(key(&install).is_err());
        assert_eq!(fs::read_to_string(install.join("link-key")).unwrap(), "broken");
        let _ = fs::remove_dir_all(&install);
    }

    // Principle 2: stale, untrusted (no `link`), malformed and unlisted heartbeats are no peers.
    #[test]
    fn peers_are_live_heartbeats_of_the_board_and_the_allowlist() {
        let k = hex(&Key::from([4; 32]).public);
        let id = "ab".repeat(32);
        let status = json!({"observed": 1000, "hosts": [
            {"name": "mac", "updated": 990, "link": format!("{id} {k}")},
            {"name": "old", "updated": 600, "link": format!("{id} {k}")},
            {"name": "plain", "updated": 990},
            {"name": "bad", "updated": 990, "link": format!("{id} zz")},
        ]});
        let names = |allow: Option<&[String]>| peers(&status, 300, allow).into_iter().map(|p| p.host).collect::<Vec<_>>();
        assert_eq!(names(None), ["mac"]);
        assert_eq!(names(Some(&["win".into()])), Vec::<String>::new());
        assert_eq!(allowed(&json!({"link_hosts": ["mac"]})).unwrap(), Some(vec!["mac".to_string()]));
        assert!(allowed(&json!({"link_hosts": "mac"})).is_err());
    }

    // D4 over a 256 KiB relay body: whole records from the cursor side; the far cursor moves to the first record left
    // out, so no record is skipped; one oversized record is shortened, never dropped.
    #[test]
    fn pages_fit_the_relay_bound_without_skipping_records() {
        let big = "x".repeat(40 * 1024);
        let items: Vec<Value> = (0..8).map(|n| json!({"at": n * 100, "i": 0, "role": "assistant", "text": big})).collect();
        let page = json!({"items": items, "start": 0, "end": 800, "size": 900});
        let f = fit(page.clone(), true);
        assert!(f.to_string().len() <= PAGE);
        let kept = f["items"].as_array().unwrap().len();
        assert!(kept > 0 && kept < 8);
        assert_eq!((f["start"].clone(), f["end"].clone()), (json!(0), json!(kept * 100)));
        let b = fit(page.clone(), false);
        let kept = b["items"].as_array().unwrap().len();
        assert_eq!((b["start"].clone(), b["end"].clone()), (json!((8 - kept) * 100), json!(800)));
        assert_eq!(b["items"][kept - 1]["at"], 700, "newest kept, in order");
        let one = fit(json!({"items": [{"at": 0, "i": 0, "role": "assistant", "text": "y".repeat(400 * 1024)}], "start": 0, "end": 10, "size": 10}), true);
        assert!(one.to_string().len() <= PAGE);
        assert_eq!((one["items"][0]["truncated"].clone(), one["end"].clone()), (json!(true), json!(10)));
        let small = json!({"items": [{"at": 0, "i": 0, "text": "a"}], "start": 0, "end": 5, "size": 5});
        assert_eq!(fit(small.clone(), true), small);
    }

    /// dashq#234 in memory: per-installation queues; `from` is the authenticated sender.
    #[derive(Clone, Default)]
    struct Fake(std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>, String);
    impl Relay for Fake {
        fn send(&mut self, to: &str, id: &str, body: &str) -> Res<()> {
            if body.len() > BODY { return Err("relay send http 413".into()) }
            self.0.lock().unwrap().push((to.into(), json!({"from": self.1, "id": id, "body": body})));
            Ok(())
        }
        fn recv(&mut self) -> Res<Vec<Value>> {
            let mut q = self.0.lock().unwrap();
            let (mine, rest): (Vec<_>, Vec<_>) = q.drain(..).partition(|(to, _)| *to == self.1);
            *q = rest;
            Ok(mine.into_iter().map(|m| m.1).collect())
        }
    }
    impl Fake { fn on(&self, me: &str) -> Self { Fake(self.0.clone(), me.into()) } }

    // A1-A3 against a fake relay: the viewer's read reaches the worker host, which serves only a live peer of its
    // board and only the named request; the sealed reply reaches the viewer through its listener. A silent or
    // refusing host reads unknown within WAIT, never an empty page.
    #[test]
    fn a_read_crosses_the_fake_relay_sealed_and_fenced() {
        let dir = std::env::temp_dir().join(format!("taskq-link-{}", crate::runtime::uuid()));
        let (viewer_install, worker_install) = (dir.join("mac"), dir.join("win"));
        let (vk, wk, stranger) = (key(&viewer_install).unwrap(), key(&worker_install).unwrap(), Key::from([7; 32]));
        let (vid, wid) = ("aa".repeat(32), "bb".repeat(32));
        let relay = Fake::default();
        let viewer_peer = Peer { host: "mac".into(), installation: vid.clone(), key: vk.public };
        let worker_peer = Peer { host: "win".into(), installation: wid.clone(), key: wk.public };
        let read = Read { board: "https://example.org/o/r/issues".into(), task: 7, attempt: 71, before: None, after: Some(20) };
        let pump = |relay: Fake, served: std::sync::Arc<std::sync::Mutex<Vec<Read>>>, peers: Vec<Peer>, answer: bool| {
            let (wi, vi, wkey, vkey) = (worker_install.clone(), viewer_install.clone(), Key::from(wk.secret), Key::from(vk.secret));
            std::thread::spawn(move || {
                let (mut w, mut v) = (relay.on(&"bb".repeat(32)), relay.on(&"aa".repeat(32)));
                let until = Instant::now() + WAIT + Duration::from_secs(1);
                while Instant::now() < until {
                    for m in w.recv().unwrap() {
                        if answer {
                            let _ = handle(&wi, &wkey, &m, &peers, &mut w.clone(), |r| { served.lock().unwrap().push(r.clone()); Ok(json!({"items": [{"at": 20, "i": 0, "role": "assistant", "text": "remote line"}], "start": 20, "end": 40, "size": 40})) });
                        }
                    }
                    for m in v.recv().unwrap() { let _ = handle(&vi, &vkey, &m, &[], &mut v.clone(), |_| Err("viewer serves nothing".into())); }
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        };
        let served = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let t = pump(relay.clone(), served.clone(), vec![viewer_peer.clone()], true);
        let began = Instant::now();
        let page = ask(&viewer_install, &vk, &worker_peer, &read, &mut relay.on(&vid)).unwrap();
        assert!(began.elapsed() < Duration::from_secs(2), "A1: well within 2 s on a fast link");
        assert_eq!(page["items"][0]["text"], "remote line");
        assert_eq!(*served.lock().unwrap(), vec![read.clone()], "A2: the server sees exactly the named attempt and cursor");
        assert!(fs::read_dir(viewer_install.join("link")).unwrap().next().is_none(), "principle 4: nothing kept");
        t.join().unwrap();

        // a host that is not a live peer of the board gets nothing, and the viewer reads unknown
        let served = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let t = pump(relay.clone(), served.clone(), vec![Peer { key: stranger.public, ..viewer_peer.clone() }], true);
        let began = Instant::now();
        let e = ask(&viewer_install, &vk, &worker_peer, &read, &mut relay.on(&vid)).unwrap_err();
        assert!(e.contains("did not answer within 5 s") && began.elapsed() >= WAIT, "A3: {e}");
        assert!(served.lock().unwrap().is_empty(), "principle 2");
        t.join().unwrap();

        // a forged `from` with a peer's key and a reply the viewer did not ask for are dropped
        let mut w = relay.on(&wid);
        let forged = json!({"from": "cc".repeat(32), "id": "11".repeat(16), "body": seal(&vk, &wk.public, json!({"kind": "read", "id": "11".repeat(16), "board": "b", "task": 1, "attempt": 1, "before": null, "after": null}).to_string().as_bytes()).unwrap()});
        assert!(handle(&worker_install, &wk, &forged, &[viewer_peer.clone()], &mut w, |_| panic!("served a forged sender")).is_err());
        let stray = json!({"from": wid, "id": "22".repeat(16), "body": seal(&wk, &vk.public, json!({"kind": "page", "id": "22".repeat(16)}).to_string().as_bytes()).unwrap()});
        handle(&viewer_install, &vk, &stray, &[], &mut relay.on(&vid), |_| unreachable!()).unwrap();
        assert!(!viewer_install.join("link").join(format!("{}.reply", "22".repeat(16))).exists(), "no viewer waits: not kept");
        let _ = fs::remove_dir_all(&dir);
    }

    // The client spoke an unversioned body that dashq 0.18.0 refuses with 400 unsupported_protocol; its own tests
    // send these shapes.
    #[test]
    fn relay_requests_match_dashq_0_18() {
        assert_eq!(send_request("host-b", "m1", "AA=="), json!({"protocol": 1, "to": "host-b", "id": "m1", "body": "AA=="}));
        assert_eq!(recv_request(), json!({"protocol": 1, "wait": 25}));
        assert_eq!(refused("send", 403, &json!({"protocol": 1, "error": "scope"})), "relay send http 403 scope");
    }

    // Live relay probe (dashq#234): on a host with an active DashQ connection, run
    // `TASKQ_LINK_LIVE=<install> cargo test --locked --bin taskq link::tests::live_relay_round_trip -- --ignored --nocapture`.
    // One sealed read goes to this installation through the real relay and its page comes back; it prints the round
    // trip, the host link delay that A1 adds to the 2 s panel bound. A running service of this build on that host
    // takes over the receive, so stop it first.
    #[test]
    #[ignore]
    fn live_relay_round_trip() {
        let live = PathBuf::from(std::env::var("TASKQ_LINK_LIVE").expect("TASKQ_LINK_LIVE names an enrolled install folder"));
        let spool_dir = std::env::temp_dir().join(format!("taskq-link-live-{}", crate::runtime::uuid()));
        let mut k = [0u8; 32];
        SystemRandom::new().fill(&mut k).unwrap();
        let me = Dash::new(&live).unwrap().1;
        let peer = Peer { host: "self".into(), installation: me.clone(), key: Key::from(k).public };
        let read = Read { board: "live-probe".into(), task: 1, attempt: 1, before: None, after: None };
        let stop = Arc::new(AtomicBool::new(false));
        let listener = {
            let (live, dir, peer, stop) = (live.clone(), spool_dir.clone(), peer.clone(), stop.clone());
            std::thread::spawn(move || {
                let (mut relay, key) = (Dash::new(&live).unwrap().0, Key::from(k));
                while !stop.load(Ordering::Relaxed) {
                    for m in relay.recv().unwrap() {
                        let served = handle(&dir, &key, &m, std::slice::from_ref(&peer), &mut Dash::new(&live).unwrap().0,
                            |r| Ok(json!({"items": [{"at": 0, "i": 0, "role": "assistant", "text": format!("live probe #{}", r.task)}], "start": 0, "end": 1, "size": 1})));
                        if let Err(e) = served { println!("dropped: {e}") }
                    }
                }
            })
        };
        std::thread::sleep(Duration::from_secs(1));
        let mut rounds = vec![];
        for _ in 0..5 {
            let began = Instant::now();
            let page = ask(&spool_dir, &Key::from(k), &peer, &read, &mut Dash::new(&live).unwrap().0).unwrap();
            rounds.push(began.elapsed().as_millis());
            assert_eq!(page["items"][0]["text"], "live probe #1");
            std::thread::sleep(Duration::from_secs(1));
        }
        stop.store(true, Ordering::Relaxed);
        println!("host link round trips through the relay (ms): {rounds:?}");
        assert!(rounds.iter().all(|r| *r < WAIT.as_millis()));
        listener.join().unwrap();
        let _ = fs::remove_dir_all(&spool_dir);
    }
}
