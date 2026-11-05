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
    #[serde(default,skip_serializing_if="Option::is_none")] pub stream_offset:Option<u64>,
    #[serde(default,skip_serializing_if="std::ops::Not::not")] pub stream_written:bool,
    #[serde(default, skip_serializing_if="Option::is_none")] pub op_id: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub app_baseline: Option<usize>,
    #[serde(default, skip_serializing_if="Option::is_none")] pub app_after: Option<u64>,
    #[serde(default, skip_serializing_if="std::ops::Not::not")] pub app_accepted: bool,
    #[serde(default, skip_serializing_if="Option::is_none")] pub prior_outcome: Option<String>,
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
#[derive(Clone,Copy)]
pub struct AppNative {
    pub baseline: fn(&Path,&Request)->Res<(usize,u64)>,
    pub submit: fn(&Path,&Request,&str)->Res<()>,
    pub observe: fn(&Path,&Request,&Turn)->Res<AppState>,
}
pub struct AppState {pub accepted:bool,pub outcome:Option<String>,pub active:bool}
pub struct Native { pub app:Option<AppNative>, pub turn: fn(&Path, &Request) -> Res<std::process::Command>, pub holder: fn(&Path, &Request) -> Res<bool> }
pub const NATIVE: Native = Native { app:Some(AppNative {baseline:crate::runtime::owner::app_baseline,submit:crate::runtime::owner::app_submit,observe:crate::runtime::owner::app_observe}), turn: crate::runtime::owner::turn, holder: crate::runtime::owner::holder };

#[derive(Debug)]
pub struct SendError { pub reason: String, pub uncertain: bool }
impl From<String> for SendError {
    fn from(reason:String)->Self {Self{reason,uncertain:false}}
}

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
    send_op(install,id,input,None,native).map_err(|e|e.reason)
}

pub fn accepted(install:&Path,id:&str,op:&str,input:&str)->Res<bool> {
    let r=launched(install,id)?;
    let saved=turn(install,id)?;
    let Some(t)=saved.filter(|t|t.op_id.as_deref()==Some(op)&&t.input==input)else{return Ok(false)};
    if crate::runtime::owner_stream::handles(&r.runtime){return crate::runtime::owner_stream::accepted(&own(install,id)?,&r,&t);}
    Ok(!crate::runtime::of(&r.runtime).has("thread-link")||t.app_accepted)
}

pub fn send_op(install:&Path,id:&str,input:&str,op:Option<&str>,native:&Native)->Result<(),SendError> {
    if let Some(op)=op {if turn(install,id)?.is_some_and(|t|t.op_id.as_deref()==Some(op)&&t.input==input) {return Ok(());}}
    let r = launched(install, id)?;
    let own = own(install, id)?;
    if busy(&turn(install, id)?) { return Err("working".to_string().into()); }
    if (native.holder)(&own, &r).map_err(|e| format!("native holder unknown ({e}); not sent"))? { return Err(crate::runtime::owner::open_in(&r.runtime).into()); }
    if crate::runtime::of(&r.runtime).has("thread-link") {if let Some(app)=native.app {(app.baseline)(&own,&r)?;}}
    // Lock acquisition is a refusal; only an attempted store can leave an uncertain receipt.
    crate::setup::locked(install, || Ok(queue(install,id,input,false,op)))?
}

pub fn retry_refused(install:&Path,id:&str,op:&str,native:&Native)->Res<()> {
    let r=launched(install,id)?;let own=own(install,id)?;
    if !crate::runtime::of(&r.runtime).has("thread-link")||native.app.is_none() {return Err("No App-owned refused input".into());}
    if (native.holder)(&own,&r)? {return Err("Native App is working; original refused input kept".into());}
    (native.app.unwrap().baseline)(&own,&r)?;
    crate::setup::locked(install,||{
        let mut t=turn(install,id)?.ok_or("Original turn unavailable")?;
        if t.op_id.as_deref()!=Some(op)||t.state!="done"||t.app_baseline.is_some()||t.outcome.as_deref()!=Some(crate::runtime::owner::open_in(&r.runtime).as_str()) {return Err("Original refusal changed; nothing retried".into());}
        match t.pid.map(coreq::process::identity) {Some(Ok(None))=>{},Some(Ok(Some(p))) if t.started.as_deref().is_some_and(|old|old!=p.started.as_str())=>{},_=>return Err("Prior CLI execution release unknown; no retry".into())}
        t.prior_outcome=t.outcome.take();t.state="pending".into();t.pid=None;t.started=None;
        store(&own.join("turn.json"),&t)
    })
}

fn queue(install: &Path, id: &str, input: &str, watch: bool, op: Option<&str>) -> Result<(),SendError> {
    if busy(&turn(install, id)?) { return Err("working".to_string().into()); }
    store(&own(install, id)?.join("turn.json"), &Turn { op_id:op.map(String::from), input: input.into(), state: "pending".into(), at: now(), watch, ..Turn::default() })
        .map_err(|reason|SendError{reason,uncertain:true})
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
            "running" | "dispatching" if cur.app_baseline.is_some() && native.app.is_some()=>{
                match (native.app.unwrap().observe)(&own,r,cur) {
                    Ok(seen)=>{cur.app_accepted|=seen.accepted;if let Some(outcome)=seen.outcome {finish(install,&own,cur,outcome)?;}else{cur.state="running".into();cur.outcome=(!cur.app_accepted).then(||"unknown: App input acceptance pending; no resend".into());store(&path,cur)?;}},
                    Err(reason)=>{cur.outcome=Some(format!("unknown: {reason}; no resend"));store(&path,cur)?;}
                }
            },
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
        if crate::conversations::reply_pending(install,&r.id)? {return Ok(());}
        queue(install, &r.id, &input, true,None).map_err(|e|e.reason)?;
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
    if let Some(offset)=t.stream_offset {
        if !t.stream_written{return Some(t.outcome.clone().unwrap_or_else(||"unknown: native input write unverified; no replay".into()));}
        use std::io::{Read,Seek,SeekFrom};
        let mut file=fs::File::open(crate::runtime::owner_stream::log(own)).ok()?;
        if file.metadata().ok()?.len()<offset{return None;}
        file.seek(SeekFrom::Start(offset)).ok()?;let mut text=String::new();file.take(16*1024*1024).read_to_string(&mut text).ok()?;
        if crate::runtime::owner_stream::session(&r.runtime,&text).ok().flatten().as_deref()!=r.session.as_deref(){return Some("unknown: native reply session identity changed".into());}
        return Some(crate::runtime::owner::outcome(&r.runtime,&text,""));
    }
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
    if crate::runtime::of(&r.runtime).has("thread-link") {if let Some(app)=native.app {
        let baseline=match (app.baseline)(own,r) {Ok(n)=>n,Err(e)=>return finish(install,own,t,format!("not sent: {e}"))};
        t.app_baseline=Some(baseline.0);t.app_after=Some(baseline.1);t.state="dispatching".into();t.pid=None;t.started=None;
        store(&own.join("turn.json"),t)?;
        let queued=(app.submit)(own,r,&t.input);
        t.state="running".into();t.outcome=Some(queued.err().map_or("unknown: App acceptance pending; no resend".into(),|e|format!("unknown: {e}; no resend")));
        return store(&own.join("turn.json"),t);
    }}
    let mut c = match (native.turn)(own, r) { Ok(c) => c, Err(e) => return finish(install, own, t, format!("not sent: {e}")) };
    t.state = "dispatching".into();
    store(&own.join("turn.json"), t)?;
    let stream=crate::runtime::owner_stream::handles(&r.runtime);
    let files=if stream {
        let path=crate::runtime::owner_stream::log(own);crate::runtime::owner_stream::write_input(own,&t.input)?;
        t.stream_offset=Some(fs::metadata(&path).map_err(|_|"Native input boundary unavailable")?.len());store(&own.join("turn.json"),t)?;
        (fs::OpenOptions::new().create(true).append(true).open(path),fs::File::create(own.join("turn.err")))
    }else {(fs::File::create(own.join("turn.out")), fs::File::create(own.join("turn.err")))};
    let (Ok(out), Ok(err)) = files else { return finish(install, own, t, "not sent: turn log unavailable".into()) };
    let mut child = match c.stdin(Stdio::piped()).stdout(out).stderr(err).spawn() { Ok(c) => c, Err(e) => return finish(install, own, t, format!("not sent: {e}")) };
    t.pid = Some(child.id());
    t.started = coreq::process::identity(child.id()).ok().flatten().map(|i| i.started);
    if stream {
        store(&own.join("turn.json"),t)?;
        if let Err(reason)=crate::runtime::owner_stream::ready(own,&r.runtime,r.session.as_deref().unwrap_or_default(),t.stream_offset.unwrap_or(0),&mut child){
            drop(child.stdin.take());std::thread::spawn(move||child.wait());t.state="running".into();t.outcome=Some(format!("unknown: {reason}"));return store(&own.join("turn.json"),t);
        }
    }
    // a failed write still leaves a turn whose outcome only the runtime's end record tells
    let written=child.stdin.take().is_some_and(|mut s|s.write_all(crate::runtime::owner_input(&r.runtime,&t.input).as_bytes()).is_ok());
    if stream {t.stream_written=written;if !written {t.outcome=Some("unknown: native input write unverified; no replay".into());}}
    t.state = "running".into();
    // ponytail: one waiting thread per turn reaps it; the next pass reads the end record from the log
    std::thread::spawn(move || child.wait());
    store(&own.join("turn.json"), t)
}

/// CS2: called on the existing background read channel, never during the tray snapshot/UI pass.
pub fn activity(install:&Path,r:&Request)->Value {
    let updated=crate::runtime::owner::transcript_for(install,r).and_then(|p|crate::transcript::last_update(crate::transcript::format(&r.runtime),session(r),&p));
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
        "turn": t.as_ref().ok().and_then(|t| t.as_ref()).map(|t| json!({"state": t.state, "at": t.at, "watch": t.watch, "outcome": t.outcome,"op_id":t.op_id,"prior_outcome":t.prior_outcome,"retry_refused":crate::runtime::of(&r.runtime).has("thread-link")&&t.state=="done"&&t.outcome.as_deref()==Some(open.as_str())&&t.app_baseline.is_none()})),
        "watch": match &w { Ok(Some(w)) => json!(w), Ok(None) => json!("no watch"), Err(e) => json!({"unknown": e}) },
    });
    if live {
        if let Ok(Some(turn))=&t {v["turn"]["input"]=json!(turn.input);}
        if crate::runtime::of(&r.runtime).has("thread-link") {if let Some(app)=native.app {
            let initial=Turn {input:crate::runtime::owner::read_brief(&crate::setup::dir(install).join(&r.id).join("brief.txt")),app_baseline:Some(0),app_after:Some(0),..Turn::default()};
            let observed=t.as_ref().ok().and_then(|t|t.as_ref()).filter(|t|t.app_baseline.is_some()).unwrap_or(&initial);
            match own(install,&r.id).and_then(|own|(app.observe)(&own,r,observed)) {
                Ok(seen) if seen.active=>state="working".into(),
                Ok(seen) if seen.accepted&&seen.outcome.is_some()&&state!="working"=>state="idle".into(),
                Ok(_) if !observed.app_accepted&&busy(t.as_ref().ok().unwrap_or(&None))=>state="unknown".into(),
                Ok(seen) if !seen.accepted||seen.outcome.is_none()=>{state="unknown".into();v["reason"]=json!("App input acceptance or completion unknown");},
                Err(reason)=>{state="unknown".into();v["reason"]=json!(reason);},
                _=>{}
            }
        }}
        let activity=activity(install,r);v["updated"]=activity["updated"].clone();v["updated_reason"]=activity["reason"].clone();
        let path = crate::runtime::owner::transcript_for(install,r);
        match path.and_then(|p| crate::transcript::last_line(crate::transcript::format(&r.runtime), session(r), &p)) {
            Ok(line) => v["last_line"] = json!(line),
            Err(e) => { state = "unknown".into(); v["reason"] = json!(e); }
        }
        if !crate::runtime::of(&r.runtime).has("thread-link") && (state == "idle" || state == open) {
            match own(install, &r.id).and_then(|o| (native.holder)(&o, r)) {
                Ok(true) => state = if crate::runtime::owner_stream::handles(&r.runtime){"working".into()}else{open},
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
    let format = crate::transcript::format(&r.runtime);
    let path = crate::runtime::owner::transcript_for(install,&r)?;
    let mut page = match after { Some(a) => crate::transcript::after(format, session(&r), &path, a)?, None => crate::transcript::before(format, session(&r), &path, before)? };
    mark_brief(&mut page.items, &r.brief, &crate::setup::dir(install).join(id));
    for item in &mut page.items{item["session"]=json!(session(&r));}
    Ok(page.json())
}

/// The native session a request's transcript must carry; AgentQ refuses a record bound to another (#2176).
fn session(r: &Request) -> &str { r.session.as_deref().unwrap_or("-") }

/// #1831: the live view of a resolved task worker's session `<rt>:<id>` on this host, read like an owner
/// transcript (D1, D4). Every user input of a worker session is TaskQ's, carrying the owner's board entries it delivers
/// (an answer or a steer, § 4 step 6), so each user item names which one it is.
/// `log` is the attempt log, the Antigravity worker's only native stream (A3).
pub fn worker_transcript(session: &str, log: &Path, before: Option<u64>, after: Option<u64>) -> Res<Value> {
    let (rt, id) = session.split_once(':').ok_or("worker session unknown")?;
    let (format, path) = crate::runtime::worker_transcript(rt, id, log)?;
    let mut page = match after { Some(a) => crate::transcript::after(format, id, &path, a)?, None => crate::transcript::before(format, id, &path, before)? };
    worker_roles(&mut page.items);
    for item in &mut page.items{item["session"]=json!(session);}
    Ok(page.json())
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
fn mark_brief(items: &mut [Value], brief: &str, own:&Path) {
    let Some(head) = brief.lines().map(str::trim).find(|l| !l.is_empty()) else { return };
    let pointer=crate::runtime::owner::read_brief(&own.join("brief.txt"));let mut handshake=false;
    for item in items {
        let text=item["text"].as_str().unwrap_or_default();
        if item["role"]=="user"&&(text.trim_start().starts_with(head)||text==pointer){item["role"]=json!("brief");handshake=false;}
        else if item["role"]=="user"&&text.trim()==crate::runtime::HANDSHAKE.trim(){item["role"]=json!("transport");handshake=true;}
        else if handshake&&item["role"]=="assistant"&&text.trim()=="OK"{item["role"]=json!("transport");handshake=false;}
        else if item["role"]!="tool"&&item["role"]!="event"{handshake=false;}
    }
}

#[cfg(test)]
#[path = "dialogue_tests.rs"]
mod tests;
