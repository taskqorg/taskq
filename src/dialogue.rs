//! Workspace dialogue (D2/D3/D5–D7): owner turns into launched sessions and the per-conversation watch. Setup keeps
//! launch truth and the native transcript keeps the dialogue (D1); this module keeps only the turn in flight and the watch.
use crate::setup::Request;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Write, path::{Path, PathBuf}, process::Stdio};
type Res<T> = Result<T, String>;

/// The project events a watch may follow (D6).
pub const EVENTS: [&str; 5] = ["close", "failed", "ask", "blocked", "gone"];
const MINUTE: u128 = 60_000;

/// `<install>/setup/<id>/turn.json`: the last turn. `pending` before dispatch, `dispatching` while it starts (a restart
/// there is unknown, never a resend, C4), `running` with its process identity, `done` with an outcome.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    #[serde(default, skip_serializing_if="Option::is_none")] pub op_id: Option<String>,
    pub input: String, pub state: String, pub at: u128, #[serde(default)] pub watch: bool,
    #[serde(default)] pub pid: Option<u32>, #[serde(default)] pub started: Option<String>, #[serde(default)] pub outcome: Option<String>,
}

/// `<install>/setup/<id>/watch.json` (D6); `seen` is the newest event time already sent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub prompt: String, pub every: u64, pub events: Vec<String>,
    #[serde(default)] pub last_turn: Option<u128>, #[serde(default)] pub last_outcome: Option<String>, pub next_due: u128, #[serde(default)] pub seen: i64,
}

/// The native side, replaceable in tests: the turn command and the D3 holder check.
pub struct Native { pub turn: fn(&Path, &Request) -> Res<std::process::Command>, pub holder: fn(&Path, &Request) -> Res<bool> }
pub const NATIVE: Native = Native { turn: crate::runtime::owner::turn, holder: crate::runtime::owner::holder };

fn own(install: &Path, id: &str) -> Res<PathBuf> {
    if !crate::setup::owner_id(id) { return Err("Invalid or reserved draft identity".into()); }
    let p = crate::setup::dir(install).join(id);
    crate::messages::plain(&p)?;
    Ok(p)
}

fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Res<Option<T>> {
    match fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map(Some).map_err(|e| format!("{} unreadable: {e}; kept", path.file_name().and_then(|s| s.to_str()).unwrap_or("dialogue record"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("dialogue record unreadable: {e}")),
    }
}

fn store<T: Serialize>(path: &Path, v: &T) -> Res<()> {
    fs::create_dir_all(path.parent().unwrap_or(path)).map_err(|e| format!("dialogue folder: {e}"))?;
    crate::platform::write_atomic(path, &serde_json::to_string(v).map_err(|e| e.to_string())?)
}

pub fn turn(install: &Path, id: &str) -> Res<Option<Turn>> { load(&own(install, id)?.join("turn.json")) }
pub fn watch(install: &Path, id: &str) -> Res<Option<Watch>> { load(&own(install, id)?.join("watch.json")) }

fn busy(t: &Option<Turn>) -> bool { t.as_ref().is_some_and(|t| t.state != "done") }

fn launched(install: &Path, id: &str) -> Res<Request> {
    let r = crate::setup::read(&crate::setup::dir(install).join(format!("{id}.json")))?;
    if r.state != "launched" || r.session.is_none() { return Err("No launched native session yet; check delivery or open it in its runtime".into()); }
    Ok(r)
}

/// D2/D3 Send: refuse while a turn runs (`working`) or a native holder has the session; otherwise persist the immutable
/// input before the service dispatches it. The draft stays with the workspace either way.
pub fn send(install: &Path, id: &str, input: &str, native: &Native) -> Res<()> {
    send_op(install,id,input,None,native)
}

pub fn accepted(install:&Path,id:&str,op:&str,input:&str)->Res<bool> {
    Ok(turn(install,id)?.is_some_and(|t|t.op_id.as_deref()==Some(op)&&t.input==input))
}

pub fn send_op(install:&Path,id:&str,input:&str,op:Option<&str>,native:&Native)->Res<()> {
    if let Some(op)=op {if accepted(install,id,op,input)? {return Ok(());}}
    let r = launched(install, id)?;
    let own = own(install, id)?;
    if busy(&turn(install, id)?) { return Err("working".into()); }
    if (native.holder)(&own, &r).map_err(|e| format!("native holder unknown ({e}); not sent"))? { return Err(crate::runtime::owner::open_in(&r.runtime)); }
    crate::setup::locked(install, || queue(install,id,input,false,op))
}

fn queue(install: &Path, id: &str, input: &str, watch: bool, op: Option<&str>) -> Res<()> {
    if busy(&turn(install, id)?) { return Err("working".into()); }
    store(&own(install, id)?.join("turn.json"), &Turn { op_id:op.map(String::from), input: input.into(), state: "pending".into(), at: now(), watch, ..Turn::default() })
}

fn now() -> u128 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) }

/// D6 Set/edit; editing keeps the last turn and outcome.
pub fn set_watch(install: &Path, id: &str, prompt: &str, every: u64, events: &[String], now: u128) -> Res<Watch> {
    let r = launched(install, id)?;
    if prompt.trim().is_empty() || prompt.len() > 65536 { return Err("Write a watch prompt of at most 64 KiB".into()); }
    if every < 5 { return Err("Watch every must be at least 5 minutes".into()); }
    if let Some(e) = events.iter().find(|e| !EVENTS.contains(&e.as_str())) { return Err(format!("Unknown watch event {e:?}; use close, failed, ask, blocked or gone")); }
    crate::setup::locked(install, || {
        let old = watch(install, id)?;
        let seen = new_events(r.root.as_deref(), &[], i64::MIN)?.1;
        let w = Watch { prompt: prompt.into(), every, events: events.to_vec(), last_turn: old.as_ref().and_then(|w| w.last_turn), last_outcome: old.and_then(|w| w.last_outcome), next_due: now + every as u128 * MINUTE, seen };
        store(&own(install, id)?.join("watch.json"), &w)?;
        Ok(w)
    })
}

pub fn remove_watch(install: &Path, id: &str) -> Res<()> {
    let path = own(install, id)?.join("watch.json");
    crate::setup::locked(install, || match fs::remove_file(&path) { Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("watch remove: {e}")), _ => Ok(()) })
}

/// Lines of `<root>/.taskq/events` newer than `seen` whose kind the watch follows, and the newest time read.
fn new_events(root: Option<&str>, kinds: &[String], seen: i64) -> Res<(Vec<String>, i64)> {
    let Some(root) = root else { return Ok((vec![], seen.max(0))) };
    let text = match fs::read_to_string(Path::new(root).join(".taskq/events")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], seen.max(0))),
        Err(e) => return Err(format!("project events unreadable: {e}")),
    };
    let mut newest = seen.max(0);
    let mut lines = vec![];
    for l in text.lines().skip(1) {
        let mut w = l.split(' ');
        let Some(t) = w.next().and_then(|t| t.parse::<i64>().ok()) else { continue };
        if t <= seen { continue; }
        newest = newest.max(t);
        if w.nth(1).is_some_and(|k| kinds.iter().any(|x| x == k)) { lines.push(l.to_string()); }
    }
    Ok((lines, newest))
}

/// One service pass (D2/D5/D6): finish ended turns, dispatch pending ones, then fire due watches unless held.
pub fn tick(install: &Path, now: u128, native: &Native) -> Res<()> {
    let (requests, _) = crate::setup::all(install)?;
    let held = crate::admission::hold(install).is_some();
    let mut first = Ok(());
    for r in requests.iter().filter(|r| r.state == "launched" && r.session.is_some()) {
        if let Err(e) = step(install, r, now, held, native) { if first.is_ok() { first = Err(format!("dialogue {}: {e}", r.id)); } }
    }
    first
}

fn step(install: &Path, r: &Request, now: u128, held: bool, native: &Native) -> Res<()> {
    let own = own(install, &r.id)?;
    let path = own.join("turn.json");
    let mut t = load::<Turn>(&path)?;
    // a turn that ends in this pass leaves the watch to the next one, so its outcome is read first
    let was_busy = busy(&t);
    if let Some(cur) = t.as_mut() {
        match cur.state.as_str() {
            "running" => if let Some(outcome) = ended(&own, r, cur) { finish(install, &own, cur, outcome)? },
            "dispatching" => finish(install, &own, cur, "unknown: the service stopped while the turn started; inspect the transcript".into())?,
            "pending" => dispatch(install, &own, r, cur, native)?,
            _ => {}
        }
    }
    let Some(mut w) = load::<Watch>(&own.join("watch.json"))? else { return Ok(()) };
    if held || was_busy { return Ok(()); }
    let (lines, newest) = new_events(r.root.as_deref(), &w.events, w.seen)?;
    if now < w.next_due && lines.is_empty() { return Ok(()); }
    let mut input = w.prompt.clone();
    if !lines.is_empty() { input.push_str(&format!("\n\nNew TaskQ events:\n{}", lines.join("\n"))); }
    crate::setup::locked(install, || {
        queue(install, &r.id, &input, true,None)?;
        w.last_turn = Some(now); w.next_due = now + w.every as u128 * MINUTE; w.seen = newest;
        store(&own.join("watch.json"), &w)
    })
}

/// A running turn's outcome once its process is gone; none while it lives or its identity is unreadable (unknown).
fn ended(own: &Path, r: &Request, t: &Turn) -> Option<String> {
    let alive = match t.pid.map(coreq::process::identity) {
        Some(Ok(Some(i))) => Some(i.started) == t.started,
        Some(Ok(None)) | None => false,
        Some(Err(_)) => return None,
    };
    if alive { return None; }
    let read = |n: &str| fs::read_to_string(own.join(n)).unwrap_or_default();
    Some(crate::runtime::owner::outcome(&r.runtime, &read("turn.out"), &read("turn.err")))
}

fn finish(install: &Path, own: &Path, t: &mut Turn, outcome: String) -> Res<()> {
    t.state = "done".into(); t.outcome = Some(outcome.clone());
    store(&own.join("turn.json"), t)?;
    if !t.watch { return Ok(()); }
    crate::setup::locked(install, || {
        let Some(mut w) = load::<Watch>(&own.join("watch.json"))? else { return Ok(()) };
        w.last_outcome = Some(outcome);
        store(&own.join("watch.json"), &w)
    })
}

fn dispatch(install: &Path, own: &Path, r: &Request, t: &mut Turn, native: &Native) -> Res<()> {
    match (native.holder)(own, r) {
        Ok(true) => return finish(install, own, t, crate::runtime::owner::open_in(&r.runtime)),
        Err(e) => return finish(install, own, t, format!("not sent: native holder unknown ({e})")),
        Ok(false) => {}
    }
    let mut c = match (native.turn)(own, r) { Ok(c) => c, Err(e) => return finish(install, own, t, format!("not sent: {e}")) };
    t.state = "dispatching".into();
    store(&own.join("turn.json"), t)?;
    let files = (fs::File::create(own.join("turn.out")), fs::File::create(own.join("turn.err")));
    let (Ok(out), Ok(err)) = files else { return finish(install, own, t, "not sent: turn log unavailable".into()) };
    let mut child = match c.stdin(Stdio::piped()).stdout(out).stderr(err).spawn() { Ok(c) => c, Err(e) => return finish(install, own, t, format!("not sent: {e}")) };
    // a failed write still leaves a turn whose outcome only the runtime's end record tells
    let _ = child.stdin.take().map(|mut s| s.write_all(t.input.as_bytes()));
    t.pid = Some(child.id());
    t.started = coreq::process::identity(child.id()).ok().flatten().map(|i| i.started);
    t.state = "running".into();
    // ponytail: one waiting thread per turn reaps it; the next pass reads the end record from the log
    std::thread::spawn(move || child.wait());
    store(&own.join("turn.json"), t)
}

/// CS2: called on the existing background read channel, never during the tray snapshot/UI pass.
pub fn activity(r:&Request)->Value {
    let updated=r.session.as_deref().ok_or("native session unknown".to_string()).and_then(|s|crate::runtime::owner::transcript(&r.runtime,s)).and_then(|p|crate::transcript::last_update(crate::runtime::of(&r.runtime).codex,&p));
    match updated {Ok(updated)=>json!({"updated":updated}),Err(reason)=>json!({"updated":null,"reason":reason})}
}

/// D5/D7 status for one conversation: state, last turn, watch. `live` adds the holder probe and the transcript's last
/// assistant line; a missing transcript is unknown.
pub fn status(install: &Path, r: &Request, live: bool, native: &Native) -> Value {
    let (t, w) = (turn(install, &r.id), watch(install, &r.id));
    let open = crate::runtime::owner::open_in(&r.runtime);
    let mut state = match &t {
        Err(_) => "unknown".to_string(),
        Ok(Some(t)) if t.state == "running" && t.pid.map(coreq::process::identity).is_some_and(|i| i.is_err()) => "unknown".into(),
        Ok(t) if busy(t) => "working".into(),
        Ok(Some(t)) if t.outcome.as_deref() == Some(&open) => open.clone(),
        _ => "idle".into(),
    };
    let mut v = json!({
        "turn": t.as_ref().ok().and_then(|t| t.as_ref()).map(|t| json!({"state": t.state, "at": t.at, "watch": t.watch, "outcome": t.outcome})),
        "watch": match &w { Ok(Some(w)) => json!(w), Ok(None) => json!("no watch"), Err(e) => json!({"unknown": e}) },
    });
    if live {
        let activity=activity(r);v["updated"]=activity["updated"].clone();v["updated_reason"]=activity["reason"].clone();
        let path = r.session.as_deref().ok_or("native session unknown".to_string()).and_then(|s| crate::runtime::owner::transcript(&r.runtime, s));
        match path.and_then(|p| crate::transcript::last_line(crate::runtime::of(&r.runtime).codex, &p)) {
            Ok(line) => v["last_line"] = json!(line),
            Err(e) => { state = "unknown".into(); v["reason"] = json!(e); }
        }
        if state == "idle" || state == open {
            match own(install, &r.id).and_then(|o| (native.holder)(&o, r)) {
                Ok(true) => state = open,
                Ok(false) => state = "idle".into(),
                Err(e) => { state = "unknown".into(); v["reason"] = json!(e); }
            }
        }
    }
    v["state"] = json!(state);
    v
}

/// D4 view: the page that ends at `before` (file end when none) or starts at `after`; `size` tells the view whether
/// it holds the file end.
pub fn transcript(install: &Path, id: &str, before: Option<u64>, after: Option<u64>) -> Res<Value> {
    let r = launched(install, id)?;
    let codex = crate::runtime::of(&r.runtime).codex;
    let path = crate::runtime::owner::transcript(&r.runtime, r.session.as_deref().unwrap_or_default())?;
    let mut page = match after { Some(a) => crate::transcript::after(codex, &path, a)?, None => crate::transcript::before(codex, &path, before)? };
    mark_brief(&mut page.items, &r.brief);
    Ok(json!({"generation":page.generation,"items": page.items, "start": page.start, "end": page.end, "size": page.size}))
}

/// #1831: the live view of a resolved task worker's session `<rt>:<id>` on this host, read like an owner
/// transcript (D1, D4). Every user input of a worker session is TaskQ's, carrying the owner's board entries it delivers
/// (an answer or a steer, § 4 step 6), so each user item names which one it is.
pub fn worker_transcript(session: &str, before: Option<u64>, after: Option<u64>) -> Res<Value> {
    let (rt, id) = session.split_once(':').ok_or("worker session unknown")?;
    let (codex, path) = crate::runtime::worker_transcript(rt, id)?;
    let mut page = match after { Some(a) => crate::transcript::after(codex, &path, a)?, None => crate::transcript::before(codex, &path, before)? };
    worker_roles(&mut page.items);
    Ok(json!({"generation":page.generation,"items": page.items, "start": page.start, "end": page.end, "size": page.size}))
}

/// #1834: a user item whose text, or the turn input file its pointer names, ends with the service's marker is TaskQ's,
/// followed by one owner item per inbox entry it delivers. An older input without the marker keeps the #1825 wording
/// guess: a pointer is the brief, anything else the owner.
fn worker_roles(items: &mut Vec<Value>) {
    // A4: tool input is arbitrary text and may contain credentials; only the native tool name leaves this adapter.
    for i in items.iter_mut().filter(|i|i["role"]=="tool") {
        let name=i["tool_name"].as_str().filter(|s|!s.is_empty()&&s.len()<=120&&s.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='.'||c=='-')).unwrap_or("tool").to_string();
        i["text"]=json!(name);i.as_object_mut().unwrap().remove("tool_name");
    }
    let mut out = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        if item["role"] != "user" { out.push(item); continue; }
        let text = item["text"].as_str().unwrap_or_default().to_string();
        let Some(inbox) = crate::runtime::turn_inbox(&text).or_else(|| turn_input(&text).and_then(|t| crate::runtime::turn_inbox(&t))) else {
            item["role"] = json!(if text.starts_with("Before any work, ") { "brief" } else { "owner" });
            out.push(item);
            continue;
        };
        item["role"] = json!("brief");
        out.push(item.clone());
        for e in inbox {
            let mut owner = item.clone();
            owner["role"] = json!("owner");
            owner["kind"] = e["kind"].clone();
            owner["text"] = e["text"].clone();
            out.push(owner);
        }
    }
    // Delivered entries share their input's record: number items within a record so view keys stay unique.
    let mut record = (Value::Null, 0);
    for item in &mut out {
        if item["at"] != record.0 { record = (item["at"].clone(), 0); }
        item["i"] = json!(record.1);
        record.1 += 1;
    }
    *items = out;
}

/// The text of the turn input file a Claude or App pointer names, only inside a `.taskq/inputs` directory.
fn turn_input(pointer: &str) -> Option<String> {
    let quoted = pointer.strip_prefix("Before any work, ")?;
    let path: String = serde_json::Deserializer::from_str(&quoted[quoted.find('"')?..]).into_iter().next()?.ok()?;
    let path = Path::new(&path);
    let dir = path.parent()?;
    let named = path.extension()? == "txt" && path.file_stem()?.to_str()?.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !named || dir.file_name()? != "inputs" || dir.parent()?.file_name()? != ".taskq" || fs::metadata(path).ok()?.len() > 4 << 20 { return None; }
    fs::read_to_string(path).ok()
}

/// #1773 A4: the launch brief TaskQ sent as the first native input is the setup brief, not an owner message.
fn mark_brief(items: &mut [Value], brief: &str) {
    let Some(head) = brief.lines().map(str::trim).find(|l| !l.is_empty()) else { return };
    for i in items.iter_mut().filter(|i| i["role"] == "user" && i["text"].as_str().is_some_and(|t| t.trim_start().starts_with(head))) { i["role"] = json!("brief"); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf { let p = std::env::temp_dir().join(format!("taskq-dialogue-{}", crate::runtime::uuid())); fs::create_dir(&p).unwrap(); p }

    fn request(install: &Path, id: &str, runtime: &str, root: Option<&Path>) -> Request {
        let r = Request { schema: 2, id: id.into(), created: 1, host: "h".into(), platform: "macos".into(), client: "c".into(), runtime: runtime.into(), targets: vec![], brief: "b".into(),
            state: "launched".into(), job: Some("j".into()), session: Some("0b4eaf42-5e87-4d57-9d31-c37c62bfbc2c".into()), url: None, reason: None, root: root.map(|p| p.display().to_string()), pm: false };
        fs::create_dir_all(crate::setup::dir(install).join(id)).unwrap();
        fs::write(crate::setup::dir(install).join(format!("{id}.json")), serde_json::to_string(&r).unwrap()).unwrap();
        r
    }

    fn free(_: &Path, _: &Request) -> Res<bool> { Ok(false) }
    fn held(_: &Path, _: &Request) -> Res<bool> { Ok(true) }
    fn never(_: &Path, _: &Request) -> Res<std::process::Command> { panic!("no turn may start") }
    fn claude_ok(_: &Path, _: &Request) -> Res<std::process::Command> {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", r#"cat >/dev/null; echo '{"type":"system"}'; echo '{"type":"result","subtype":"success","is_error":false}'"#]);
        Ok(c)
    }
    fn exits(_: &Path, _: &Request) -> Res<std::process::Command> { let mut c = std::process::Command::new("sh"); c.args(["-c", "cat >/dev/null"]); Ok(c) }
    fn codex_writer(_: &Path, _: &Request) -> Res<std::process::Command> {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", "cat >/dev/null; echo 'Error: thread 0b4e already has an active writer' >&2; exit 1"]);
        Ok(c)
    }

    fn settle(install: &Path, id: &str, native: &Native) -> Turn {
        for _ in 0..200 {
            tick(install, now(), native).unwrap();
            let t = turn(install, id).unwrap().unwrap();
            if t.state == "done" { return t; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("turn never ended")
    }

    // #1929 A6: same operation readback never starts or queues another native turn.
    #[test]
    fn conversation_1929_reply_readback_binds_input_and_operation() {
        let install=tmp();request(&install,"owner","claude",None);
        let native=Native{turn:never,holder:free};
        assert!(!accepted(&install,"owner","op-a","Hello").unwrap());
        send_op(&install,"owner","Hello",Some("op-a"),&native).unwrap();
        let first=turn(&install,"owner").unwrap();
        assert!(accepted(&install,"owner","op-a","Hello").unwrap());
        assert!(!accepted(&install,"owner","op-b","Hello").unwrap());
        assert!(!accepted(&install,"owner","op-a","Changed").unwrap());
        send_op(&install,"owner","Hello",Some("op-a"),&Native{turn:never,holder:held}).unwrap();
        assert_eq!(turn(&install,"owner").unwrap(),first);
        assert!(send_op(&install,"owner","Changed",Some("op-a"),&native).is_err());
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn launch_brief_is_not_an_owner_message() {
        let brief = "You are a TaskQ session opened by the host service.\nProject: p\n\nWhy is nothing starting?\n";
        let mut items = vec![json!({"role":"user","text":brief}), json!({"role":"assistant","text":"ok"}), json!({"role":"user","text":"Why is nothing starting?"})];
        mark_brief(&mut items, brief);
        assert_eq!(items.iter().map(|i| i["role"].as_str().unwrap()).collect::<Vec<_>>(), ["brief", "assistant", "user"]);
        mark_brief(&mut items, "");
        assert_eq!(items[2]["role"], "user", "an empty brief marks nothing");
    }

    // D-A1/A2 source: the input persists before dispatch; the outcome comes from the result record only.
    // #1825 A3: in a worker transcript the per-turn brief pointer is TaskQ's and every other input is the owner's
    #[test]
    fn worker_inputs_name_their_author() {
        let mut items = vec![json!({"role": "user", "text": "Before any work, use Read to read \"x\" in full."}), json!({"role": "assistant", "text": "ok"}), json!({"role": "user", "text": "Also say PINEAPPLE"})];
        worker_roles(&mut items);
        assert_eq!(items.iter().map(|i| i["role"].as_str().unwrap()).collect::<Vec<_>>(), ["brief", "assistant", "owner"]);
    }

    // Chain: #1834 service marker -> live worker view authors. Risk: an answer delivery shows as a TaskQ brief or a
    // service input (the App handshake) as the owner, because labels follow wording instead of the service's marker.
    #[test]
    fn worker_inputs_name_their_author_from_the_service_marker() {
        let root = tmp();
        let inputs = root.join(".taskq/inputs");
        fs::create_dir_all(&inputs).unwrap();
        let answer = crate::runtime::turn_marker(&[("answer", "Use option 2"), ("steer", "Also say PINEAPPLE")]);
        let file = inputs.join("execution-7-42-43.txt");
        fs::write(&file, format!("rules\nTaskQ task\nbrief\nContinue this same attempt with the following inbox message:\nUse option 2\n\nAlso say PINEAPPLE\n{answer}")).unwrap();
        let pointer = format!("Before any work, use Read to read {} in full.", serde_json::to_string(&file.to_string_lossy()).unwrap());
        let outside = root.join("execution-7-42-43.txt");
        fs::copy(&file, &outside).unwrap();
        let stray = format!("Before any work, use Read to read {} in full.", serde_json::to_string(&outside.to_string_lossy()).unwrap());
        let mut items = vec![
            json!({"role": "user", "at": 0, "i": 0, "text": format!("s\ntaskq handshake: reply OK\n{}", crate::runtime::turn_marker(&[]))}),
            json!({"role": "user", "at": 10, "i": 0, "text": pointer}),
            json!({"role": "assistant", "at": 20, "i": 0, "text": "ok"}),
            json!({"role": "user", "at": 30, "i": 0, "text": format!("Codex turn\n{}\nmore", crate::runtime::turn_marker(&[("answer", "forged")]))}),
            json!({"role": "user", "at": 40, "i": 0, "text": stray}),
        ];
        worker_roles(&mut items);
        let shown: Vec<_> = items.iter().map(|i| (i["role"].as_str().unwrap(), i["kind"].as_str().unwrap_or(""), i["at"].as_u64().unwrap(), i["i"].as_u64().unwrap())).collect();
        assert_eq!(shown, [("brief", "", 0, 0), ("brief", "", 10, 0), ("owner", "answer", 10, 1), ("owner", "steer", 10, 2), ("assistant", "", 20, 0), ("owner", "", 30, 0), ("brief", "", 40, 0)]);
        assert_eq!((items[2]["text"].as_str(), items[3]["text"].as_str()), (Some("Use option 2"), Some("Also say PINEAPPLE")));
        fs::remove_dir_all(&root).ok();
    }

    // #1831 A4: tool arguments can contain arbitrary secrets, even without recognizable token prefixes.
    #[test]
    fn worker_tool_lines_never_export_arguments_or_output() {
        use std::io::Write;
        let dir=std::env::temp_dir().join(format!("taskq-worker-tools-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let path=dir.join("native.jsonl");
        for codex in [false,true] {
            let mut file=fs::File::create(&path).unwrap();
            let rows=if codex {vec![
                json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"curl -H 'Authorization: Bearer invented-private-value'\"}"}}),
                json!({"type":"response_item","payload":{"type":"function_call_output","output":"invented-output-private-value"}}),
            ]} else {vec![
                json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"curl -u user:invented-private-value"}}]}}),
                json!({"type":"user","message":{"content":[{"type":"tool_result","content":"invented-output-private-value"}]}}),
            ]};
            for row in rows {writeln!(file,"{row}").unwrap();}drop(file);
            let mut page=crate::transcript::before(codex,&path,None).unwrap();worker_roles(&mut page.items);
            assert_eq!(page.items.len(),1);assert_eq!(page.items[0]["role"],"tool");
            assert!(page.items[0]["text"].as_str().unwrap().contains(if codex {"exec_command"} else {"Bash"}));
            assert!(!json!(page.items).to_string().contains("private-value"));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // #1825 A2 live probe, run by hand: TASKQ_PROBE_SESSION=<rt>:<id> follows a session on this host with the task
    // thread's 1 s poll and prints each new item's arrival in Unix milliseconds, to compare with the native stream.
    #[test]
    #[ignore]
    fn live_worker_stream_probe() {
        let session = std::env::var("TASKQ_PROBE_SESSION").expect("TASKQ_PROBE_SESSION");
        let (mut end, until) = (None, std::time::Instant::now() + std::time::Duration::from_secs(60));
        while std::time::Instant::now() < until {
            match worker_transcript(&session, None, end) {
                Ok(v) => {
                    for i in v["items"].as_array().unwrap() { println!("{} {} {}", now(), i["role"], i["text"].as_str().unwrap_or_default().chars().take(40).collect::<String>().replace('\n', " ")); }
                    end = v["end"].as_u64();
                }
                Err(e) => println!("{} unknown: {e}", now()),
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    #[test]
    fn send_dispatches_one_turn_and_reads_its_outcome() {
        let install = tmp();
        let r = request(&install, "c1", "claude", None);
        let native = Native { turn: claude_ok, holder: free };
        send(&install, "c1", "hello", &Native { turn: never, holder: free }).unwrap();
        assert_eq!(turn(&install, "c1").unwrap().unwrap().state, "pending", "input persisted before dispatch");
        assert_eq!(send(&install, "c1", "again", &native).unwrap_err(), "working");
        assert_eq!(status(&install, &r, false, &native)["state"], "working");
        let t = settle(&install, "c1", &native);
        assert_eq!((t.input.as_str(), t.outcome.as_deref()), ("hello", Some("success")));
        assert_eq!(status(&install, &r, false, &native)["state"], "idle");
        send(&install, "c1", "next", &native).unwrap();
        assert_eq!(settle(&install, "c1", &Native { turn: exits, holder: free }).outcome.as_deref(), Some("unknown: the turn ended without a result record"), "exit alone is never success");
        fs::remove_dir_all(install).unwrap();
    }

    // D-A2 source: a Claude holder refuses at Send and at dispatch; Codex's second-writer refusal reads the same text.
    #[test]
    fn native_holder_refuses_with_d3_text() {
        let install = tmp();
        let r = request(&install, "c2", "claude", None);
        assert_eq!(send(&install, "c2", "hi", &Native { turn: never, holder: held }).unwrap_err(), "open in claude; close it there first");
        assert_eq!(turn(&install, "c2").unwrap(), None, "a refused Send queues nothing");
        send(&install, "c2", "hi", &Native { turn: never, holder: free }).unwrap();
        let t = settle(&install, "c2", &Native { turn: never, holder: held });
        assert_eq!(t.outcome.as_deref(), Some("open in claude; close it there first"));
        assert_eq!(status(&install, &r, false, &NATIVE)["state"], "open in claude; close it there first");
        let r = request(&install, "x2", "codex", None);
        send(&install, "x2", "hi", &Native { turn: never, holder: free }).unwrap();
        assert_eq!(settle(&install, "x2", &Native { turn: codex_writer, holder: free }).outcome.as_deref(), Some("open in codex; close it there first"));
        assert_eq!(status(&install, &r, false, &NATIVE)["state"], "open in codex; close it there first");
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn outcome_only_from_end_records() {
        use crate::runtime::owner::outcome;
        assert_eq!(outcome("claude", "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}\n", ""), "success");
        assert_eq!(outcome("claude", "{\"type\":\"result\",\"subtype\":\"error_max_turns\",\"is_error\":true}\n", ""), "failed: error_max_turns");
        assert_eq!(outcome("codex", "{\"type\":\"turn.completed\"}\n", ""), "success");
        assert_eq!(outcome("codex", "{\"type\":\"turn.failed\",\"error\":{\"message\":\"quota\"}}\n", ""), "failed: quota");
        assert!(outcome("codex", "{\"type\":\"turn.started\"}\n", "").starts_with("unknown"));
        assert!(outcome("claude", "", "already has an active writer").starts_with("unknown"), "only Codex reports a writer");
    }

    // D-A4/D-A5 source: schedule, events at once, hold pauses, restart keeps the watch and an unknown outcome waits for idle.
    #[test]
    fn watch_fires_on_schedule_and_events_and_pauses_under_hold() {
        let install = tmp();
        let root = tmp();
        fs::create_dir(root.join(".taskq")).unwrap();
        fs::write(root.join(".taskq/events"), "since 100\n100 #7 ask 1\n").unwrap();
        let r = request(&install, "w1", "claude", Some(&root));
        assert!(set_watch(&install, "w1", "check", 4, &[], 0).unwrap_err().contains("at least 5"));
        assert!(set_watch(&install, "w1", "check", 5, &["late".into()], 0).unwrap_err().contains("Unknown watch event"));
        let w = set_watch(&install, "w1", "check the board", 5, &["ask".into()], 0).unwrap();
        assert_eq!((w.next_due, w.seen), (5 * MINUTE, 100), "existing events are not news");
        let quiet = Native { turn: never, holder: free };
        tick(&install, MINUTE, &quiet).unwrap();
        assert_eq!(turn(&install, "w1").unwrap(), None);
        // schedule
        tick(&install, 5 * MINUTE, &quiet).unwrap();
        let t = turn(&install, "w1").unwrap().unwrap();
        assert_eq!((t.input.as_str(), t.state.as_str(), t.watch), ("check the board", "pending", true));
        let t = settle(&install, "w1", &Native { turn: exits, holder: free });
        assert!(t.outcome.unwrap().starts_with("unknown"));
        let w = watch(&install, "w1").unwrap().unwrap();
        assert_eq!((w.last_turn, w.next_due), (Some(5 * MINUTE), 10 * MINUTE));
        assert!(w.last_outcome.unwrap().starts_with("unknown"), "unknown stays unknown");
        // a new ask line makes it due at once; other kinds only advance the cursor
        fs::write(root.join(".taskq/events"), "since 100\n100 #7 ask 1\n200 #8 close 2\n201 #9 ask 3\n").unwrap();
        fs::write(install.join("hold"), "owner hold\n").unwrap();
        tick(&install, 6 * MINUTE, &quiet).unwrap();
        assert!(turn(&install, "w1").unwrap().unwrap().state == "done", "hold pauses the watch");
        send(&install, "w1", "owner", &quiet).unwrap(); // owner Send still works under hold
        fs::write(crate::setup::dir(&install).join("w1/turn.json"), serde_json::to_string(&t_running()).unwrap()).unwrap();
        fs::remove_file(install.join("hold")).unwrap();
        // restart: a turn that was starting is unknown and the watch waits for that pass to end
        tick(&install, 6 * MINUTE, &quiet).unwrap();
        assert_eq!(turn(&install, "w1").unwrap().unwrap().outcome.as_deref(), Some("unknown: the service stopped while the turn started; inspect the transcript"));
        tick(&install, 6 * MINUTE, &quiet).unwrap();
        let t = turn(&install, "w1").unwrap().unwrap();
        assert_eq!((t.input.as_str(), t.state.as_str()), ("check the board\n\nNew TaskQ events:\n201 #9 ask 3", "pending"));
        let s = status(&install, &r, false, &quiet);
        assert_eq!((s["state"].as_str(), s["watch"]["every"].as_u64(), s["watch"]["seen"].as_i64(), s["watch"]["next_due"].as_u64()), (Some("working"), Some(5), Some(201), Some(11 * MINUTE as u64)));
        remove_watch(&install, "w1").unwrap();
        assert_eq!(status(&install, &r, false, &quiet)["watch"], "no watch");
        fs::remove_dir_all(install).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    fn t_running() -> Turn { Turn { input: "owner".into(), state: "dispatching".into(), at: 1, ..Turn::default() } }
}
