//! Host link ([detail](../docs/design/host-link.md#contract-detail), #1832): a host reads one bounded page of a live
//! worker session on another host of its board through DashQ's blind relay (dashq#234). Every relay body is sealed
//! end to end: static X25519 installation keys (RFC 7748, below; ring offers no static X25519), HKDF-SHA256 and
//! ChaCha20-Poly1305 from ring. Pull only: one request names one attempt and one cursor and gets one page; nothing is
//! kept beyond the key, and a lost message is asked again by the viewer's next read.
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::{aead, hkdf, rand::{SecureRandom, SystemRandom}};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::{Duration, Instant}};
type Res<T> = Result<T, String>;

pub const SEND: &str = "/link/v1/send";
pub const RECV: &str = "/link/v1/recv";
/// Principle 6: a host that does not answer within this reads unknown.
pub const WAIT: Duration = Duration::from_secs(15);
/// Curl's limit on one relay send.
const SEND_WAIT: u64 = 5;
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
        let out = match coreq::process::exchange("link.http", crate::platform::command("curl").args(["-sS", "--proto", "=https", "-w", "\n%{http_code}", "--config", "-"]),
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
        match self.post(SEND, &send_request(to, id, body), SEND_WAIT)? {
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
    ask_within(install, key, peer, read, relay, WAIT)
}

fn ask_within(install: &Path, key: &Key, peer: &Peer, read: &Read, relay: &mut dyn Relay, limit: Duration) -> Res<Value> {
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
            if began.elapsed() >= limit { return Err(format!("host {} did not answer within {} s", peer.host, limit.as_secs())) }
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    let _ = fs::remove_file(&wait);
    let _ = fs::remove_file(&reply);
    result
}

/// One relay message on the listening host. A `read` from a live peer of the board it names (`from` and key both match
/// a heartbeat there) gets one sealed page from `serve`; a `page` goes to the spool only while its viewer waits.
/// Everything else is dropped.
pub fn handle(install: &Path, key: &Key, msg: &Value, peers: impl FnOnce(&str) -> Vec<Peer>, relay: &mut dyn Relay, serve: impl FnOnce(&Read) -> Res<Value>) -> Res<()> {
    let (from, id, body) = (msg["from"].as_str().unwrap_or(""), msg["id"].as_str().unwrap_or(""), msg["body"].as_str().unwrap_or(""));
    if !id_ok(id) { return Err("host link message id invalid".into()) }
    let (sender, plain) = open(key, body)?;
    let mut v: Value = serde_json::from_slice(&plain).map_err(|_| "host link message unreadable")?;
    if v["id"] != id { return Err("host link message id mismatch".into()) }
    match v["kind"].as_str() {
        Some("read") => {
            let o = v.as_object_mut().unwrap();
            o.remove("kind");
            o.remove("id");
            let read: Read = serde_json::from_value(v).map_err(|_| "host link request unreadable")?;
            let peer = peers(&read.board).into_iter().find(|p| p.key == sender && p.installation == from).ok_or("host link request from no live host of this board")?;
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
    /// One step per service pass: returns the heartbeat `link` line of every project on an enrolled host, and
    /// (re)starts the host's one listener on the project that speaks for the connection (elsewhere it stops after the
    /// current long poll).
    pub fn tick(&mut self, install: &Path, snap: Option<&crate::snapshot::Snapshot>) -> &str {
        let state = crate::enrollment::state(install).ok().flatten().filter(|s| crate::enrollment::approved(&s["active"]).is_some());
        if !state.as_ref().is_some_and(|s| crate::enrollment::speaks(&s["active"], snap)) {
            if let Some((_, stop)) = self.job.take() { stop.store(true, Ordering::Relaxed) }
        } else if self.job.as_ref().is_none_or(|j| j.0.is_finished()) {
            let (install, stop) = (install.to_path_buf(), Arc::new(AtomicBool::new(false)));
            let flag = stop.clone();
            self.job = Some((std::thread::spawn(move || listen(&install, &flag)), stop));
        }
        self.line = match (state.as_ref().and_then(|s| s["installation"].as_str()), key(install)) {
            (Some(i), Ok(k)) => line(i, &k),
            _ => String::new(),
        };
        &self.line
    }
}

fn listen(install: &Path, stop: &AtomicBool) {
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
                for m in msgs {
                    if let Err(e) = handle(install, &key, &m, |board| crate::conversations::board_peers(install, board), &mut relay, |r| crate::conversations::serve_board(install, r)) { say(format!("message dropped: {e}")); }
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
#[path = "link_tests.rs"]
mod tests;
