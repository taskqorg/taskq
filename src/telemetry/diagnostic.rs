//! Product controls and read-only views over the existing collector.
use std::{collections::BTreeMap,path::Path,time::{Duration,Instant},sync::Mutex};
use serde_json::{Value,json};
use std::io::Write;

fn capture()->Result<coreq::telemetry::capture::Capture,String> {
    tracing::dispatcher::get_default(|d|d.downcast_ref::<coreq::telemetry::capture::Capture>().cloned())
        .ok_or("telemetry collector unavailable".into())
}

pub fn select(selector:&str,period:Duration,limit:u64)->Result<(),String> {
    capture()?.diagnose(selector,period,limit)?;
    let until=crate::profile::now_ms().ok_or("diagnostic clock unknown")?.saturating_add(period.as_millis() as u64);
    *WEB_SELECTION.lock().map_err(|_|"web diagnostic control unavailable")?=Some(json!({"id":crate::runtime::uuid(),"selector":selector,"until_ms":until,"limit":limit}));
    Ok(())
}
// One in-memory handoff to this process's WebView; it carries no effects or authority.
static WEB_SELECTION:Mutex<Option<Value>>=Mutex::new(None);
pub fn take_web_selection()->Option<Value>{WEB_SELECTION.lock().ok()?.take()}


#[derive(serde::Serialize,serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {id:String,pid:u32,started:String,selector:String,from_ms:u64,until_ms:u64,limit:u64}
#[derive(Default)]
struct Control {checked:Option<Instant>,seen:Option<String>,error:Option<String>}
static CONTROL:Mutex<Control>=Mutex::new(Control {checked:None,seen:None,error:None});

fn limits(selector:&str,milliseconds:u64,limit:u64)->Result<(),String> {
    if milliseconds==0||milliseconds>60000||!(1..=256).contains(&limit)
        ||(selector!="*"&&(selector.is_empty()||selector.len()>128||!selector.bytes().all(|c|c.is_ascii_alphanumeric()||b"._:-".contains(&c)))) {
        return Err("invalid diagnostic selection limits".into());
    }
    Ok(())
}
fn read(install:&Path)->Result<Option<Selection>,String> {
    use std::io::Read;
    let file=match std::fs::File::open(install.join("telemetry.log/diagnostic.json")) {
        Ok(file)=>file,Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err("selection-request-unreadable".into())
    };
    let mut text=String::new();file.take(2049).read_to_string(&mut text).map_err(|_|"selection-request-unreadable")?;
    if text.len()>2048 {return Err("selection-request-oversized".into());}
    let value:Selection=serde_json::from_str(&text).map_err(|_|"selection-request-invalid")?;
    let duration=value.until_ms.checked_sub(value.from_ms).ok_or("selection-request-invalid")?;
    limits(&value.selector,duration,value.limit)?;
    if value.pid==0||value.started.is_empty()||value.started.len()>128||value.id.len()!=32||!value.id.bytes().all(|c|c.is_ascii_hexdigit()) {return Err("selection-request-invalid".into());}
    Ok(Some(value))
}

pub fn request(install:&Path,pid:u32,selector:&str,milliseconds:u64,limit:u64)->Result<String,String> {
    limits(selector,milliseconds,limit)?;
    let identity=coreq::process::identity(pid).map_err(|_|"diagnostic target identity unavailable")?.ok_or("diagnostic target exited")?;
    let now=crate::profile::now_ms().ok_or("diagnostic clock unknown")?;
    let _lock=crate::profile::log_lock(&install.join("taskq.log")).map_err(|e|e.to_string())?;
    if read(install)?.is_some_and(|old|old.until_ms>now) {return Err("diagnostic request active; await expiry".into());}
    let selection=Selection {id:crate::runtime::uuid().replace('-',""),pid,started:identity.started,selector:selector.into(),from_ms:now,until_ms:now+milliseconds,limit};
    let dir=install.join("telemetry.log");std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    coreq::write_durable(&dir.join("diagnostic.json"),&serde_json::to_string(&selection).map_err(|e|e.to_string())?)?;
    // Existing host wake; the target still has to observe and acknowledge the selection.
    let wake=std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(install.join("poke")).map_err(|_|"selection saved; host wake failed")?;
    let wake=coreq::file_lock::try_lock_file(wake).map_err(|_|"selection saved; host wake locked")?;
    crate::poke_locked(&wake)?;
    Ok(selection.id)
}

fn apply(install:&Path,seen:&mut Option<String>)->Result<(),String> {
    let Some(value)=read(install)? else {return Ok(())};
    if value.pid!=std::process::id()||seen.as_deref()==Some(&value.id) {return Ok(())}
    let now=crate::profile::now_ms().ok_or("diagnostic clock unknown")?;
    if now>=value.until_ms {return Err("selection-request-expired".into())}
    if now<value.from_ms {return Err("selection-clock-unknown".into())}
    if coreq::process::identity(value.pid).map_err(|_|"selection-identity-unknown")?.is_none_or(|id|id.started!=value.started) {return Err("selection-identity-mismatch".into())}
    select(&value.selector,Duration::from_millis(value.until_ms-now),value.limit)?;
    *seen=Some(value.id);Ok(())
}

pub fn poll(install:&Path) {
    let Ok(mut control)=CONTROL.try_lock() else {return};
    if control.checked.is_some_and(|at|at.elapsed()<Duration::from_secs(1)) {return}
    control.checked=Some(Instant::now());
    let error=apply(install,&mut control.seen).err();
    if error!=control.error {if let Some(error)=&error {let _=writeln!(std::io::stderr(),"Diagnostic control: unknown ({error})");}control.error=error;}
}

/// A scoped snapshot, never a claim about another process or the retained history.
pub fn health()->Value {health_at(crate::profile::now_ms())}

pub(crate) const HEALTH_WINDOW_MS:u64=15*60*1000;
thread_local! {static LAST_HEALTH:std::cell::RefCell<Option<(u64,Value)>>=const {std::cell::RefCell::new(None)};}

pub(crate) fn health_at(now:Option<u64>)->Value {
    let result=(|| {
        let capture=capture()?;
        let mut health=capture.health();
        let started=now.ok_or("telemetry clock unavailable")?;
        let counts=["accepted","committed","lost","failed_batches","inflight","queued_bytes","queued_events"].into_iter()
            .map(|key|(key.into(),health[key].clone())).collect::<serde_json::Map<_,_>>();
        // #1297: CoreQ folds only fixed JS counters, so each row here also materialized the pass root. Counters are
        // cumulative: one row per thread (project loop) per 15-minute window, or when state/lost/failed_batches change, keeps them.
        // ponytail: gauges (inflight, queued_*) are sampled once per window; status.json keeps their live value.
        let key=(started/HEALTH_WINDOW_MS,json!([health["state"],health["lost"],health["failed_batches"]]));
        if LAST_HEALTH.with(|last|last.borrow().as_ref()!=Some(&key)) {
            coreq::telemetry::capture::observe(json!({"product":"taskq","version":crate::platform::VERSION,"operation":"collector.health",
                "kind":"collection","started_ms":started,"process_id":std::process::id(),
                "trace":null,"span":null,"unknown":{"trace":"not-recorded","span":"not-recorded"},
                "source":"process","outcome":if health["state"]=="collecting" {"ok"} else {"unknown"},
                "error":if health["state"]=="collecting" {"none"} else {"collection-incomplete"},"counts":counts}))?;
            LAST_HEALTH.with(|last|*last.borrow_mut()=Some(key));
        }
        health["scope"]=json!("this-process");health["process_id"]=json!(std::process::id());
        health["historical_completeness"]=json!("unknown");
        health["control"]=CONTROL.lock().map(|state|json!({"applied_request":state.seen,"error":state.error})).unwrap_or(json!({"error":"control-state-unavailable"}));
        Ok::<_,String>(health)
    })();
    result.unwrap_or_else(|_|json!({"state":"unknown","scope":"this-process","reason":"collector-snapshot-unavailable"}))
}

#[derive(Default)]
struct Stats {times:Vec<u64>,slowest:Option<(u64,u64)>,failed:u64,unknown:u64,refused:u64,errors:BTreeMap<String,u64>}

pub fn actions(install:&Path,since:Option<u64>)->Result<String,String> {
    // Never recursively instrument the telemetry reader's own decoding work.
    let capture=capture().ok();
    if let Some(capture)=capture {capture.flush()?;}
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(),|| {
        let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
        let mut page=reader.export_kinds(&["method.start","method","collection"],since.unwrap_or(0).min(i64::MAX as u64) as i64,0,1024)?;
        let mut out=format!("Actions and diagnostics: committed observations\nWindow UTC ms: {}..{}; pruned {} records; historical completeness unknown\n",
            page["retained_ms"][0],page["retained_ms"][1],page["pruned_count"]);
        let mut stats=BTreeMap::<(String,String),Stats>::new();
        let mut open=BTreeMap::<(String,String),(String,String)>::new();
        let (mut health,mut incomplete)=(0u64,0u64);
        loop {
            for row in page["events"].as_array().ok_or("invalid action page")? {
                let event=&row["observation"];
                if event["kind"]=="collection" {health+=1;if event["outcome"]!="ok" {incomplete+=1;}continue;}
                let key=(event["trace"].as_str().ok_or("missing action trace")?.into(),event["span"].as_str().ok_or("missing action span")?.into());
                let operation=(event["source"].as_str().ok_or("missing action source")?.to_owned(),event["operation"].as_str().ok_or("missing action operation")?.to_owned());
                if event["kind"]=="method.start" {open.insert(key,operation);continue;}
                open.remove(&key);
                // CoreQ store spans (#1517) name their trigger and outcome, so a stall row says which election, scan or checkpoint waited.
                let operation=match event["from"].as_str() {Some(from)=>(operation.0,format!("{} {from}->{}",operation.1,event["to"].as_str().unwrap_or("unknown"))),None=>operation};
                let s=stats.entry(operation).or_default();
                if let Some(time)=event["duration_us"].as_u64() {
                    s.times.push(time);
                    if s.slowest.is_none_or(|(us,_)|time>us) {s.slowest=Some((time,event["started_ms"].as_u64().unwrap_or(0)));}
                }
                match event["outcome"].as_str() {Some("failed")=>s.failed+=1,Some("unknown")=>s.unknown+=1,Some("refused")=>s.refused+=1,_=>{}}
                if let Some(error)=event["error"].as_str().filter(|error|*error!="none") {*s.errors.entry(error.into()).or_default()+=1;}
            }
            if page["more"]==false {break;}
            page=reader.export_kinds(&["method.start","method","collection"],since.unwrap_or(0).min(i64::MAX as u64) as i64,
                page["next"].as_i64().ok_or("missing action cursor")?,1024)?;
        }
        for (_,operation) in open {stats.entry(operation).or_default().unknown+=1;}
        out+=&format!("Collector snapshots: {health}; incomplete: {incomplete}; missing process coverage unknown\n");
        let mut rows=stats.into_iter().collect::<Vec<_>>();
        for (_,s) in &mut rows {s.times.sort_unstable();}
        rows.sort_by(|a,b|(b.1.failed+b.1.unknown).cmp(&(a.1.failed+a.1.unknown)).then_with(||b.1.times.last().cmp(&a.1.times.last())).then(a.0.cmp(&b.0)));
        out+="\n| Source | Operation | Timed | p95/max ms | Slowest at UTC ms | Failed/refused/unknown | Errors | Budget |\n|---|---|---:|---|---|---|---|---|\n";
        if rows.is_empty() {out+="| unknown | no retained actions | 0 | unknown | unknown | unknown | unknown | unknown |\n";}
        for ((source,operation),s) in rows {
            let times=if s.times.is_empty() {"unknown".into()} else {format!("{:.3}/{:.3}",s.times[(s.times.len()*95).div_ceil(100)-1] as f64/1000.,*s.times.last().unwrap() as f64/1000.)};
            let budget=if operation=="taskq.service.Service.pass" {"native observations 20s; whole pass unknown"} else {"unknown (unassigned)"};
            let errors=if s.errors.is_empty() {"none".into()} else {s.errors.into_iter().map(|(k,v)|format!("{k}:{v}")).collect::<Vec<_>>().join(", ")};
            let at=s.slowest.filter(|(_,at)|*at>0).map_or("unknown".into(),|(_,at)|at.to_string());
            out+=&format!("| {source} | {operation} | {} | {times} | {at} | {}/{}/{} | {errors} | {budget} |\n",s.times.len(),s.failed,s.refused,s.unknown);
        }
        Ok(out)
    })
}

/// #1910 `profile watch`: failed actions with an error class per operation, counted before `split` and from it on.
pub fn action_errors(install:&Path,from:u64,split:u64)->Result<BTreeMap<(String,String),[u64;2]>,String> {
    let capture=capture().ok();
    if let Some(capture)=capture {capture.flush()?;}
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(),|| {
        let reader=coreq::telemetry::Reader::open(&install.join("telemetry.log/universal.sqlite3"))?;
        let mut counts=BTreeMap::new();
        let mut cursor=0;
        loop {
            let page=reader.export_kinds(&["method"],from.min(i64::MAX as u64) as i64,cursor,1024)?;
            for row in page["events"].as_array().ok_or("invalid action page")? {
                let event=&row["observation"];
                if event["outcome"]!="failed" {continue;}
                let error=event["error"].as_str().filter(|e|*e!="none").unwrap_or("unknown");
                let operation=event["operation"].as_str().ok_or("missing action operation")?;
                let at=event["started_ms"].as_u64().ok_or("missing action time")?;
                if at<from {continue;}
                let c:&mut [u64;2]=counts.entry((operation.to_owned(),error.to_owned())).or_default();
                c[usize::from(at>=split)]+=1;
            }
            if page["more"]==false {break;}
            cursor=page["next"].as_i64().ok_or("missing action cursor")?;
        }
        Ok(counts)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Risk: product flags target another collector, reports omit real failures, or expired selection keeps collecting.
    #[test]
    fn product_selection_failure_report_export_and_expiry() {
        use std::sync::Arc;
        use tracing_subscriber::prelude::*;
        let dir=std::env::temp_dir().join(format!("taskq-diagnose-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(dir.join("telemetry.log")).unwrap();
        let store=Arc::new(coreq::telemetry::Store::open(&dir.join("telemetry.log/universal.sqlite3")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()),|| {
            let args=["profile","missing","--diagnose","profile","--diagnose-ms","60000","--diagnose-limit","1"].map(String::from);
            assert!(crate::run(&args).unwrap_err().contains("unknown profile view"));
            assert_eq!(capture.health()["diagnostics"]["state"],"limited");
            assert_eq!(capture.health()["diagnostics"]["admitted"],1);
            let expiry=["profile","missing","--diagnose","service","--diagnose-ms","1","--diagnose-limit","256"].map(String::from);
            assert!(crate::run(&expiry).is_err());
            let deadline=std::time::Instant::now()+Duration::from_secs(2);
            while capture.health()["diagnostics"]["state"]=="active" {
                assert!(std::time::Instant::now()<deadline,"product selection never expired");std::thread::yield_now();
            }
            assert_eq!(capture.health()["diagnostics"]["state"],"expired");
            let admitted=capture.health()["diagnostics"]["admitted"].as_u64().unwrap();assert_eq!(admitted,0);
            crate::profile::view(&dir,"guide",None).unwrap();
            assert_eq!(capture.health()["diagnostics"]["admitted"],admitted,"expired product selection admitted more methods");
            let id=request(&dir,std::process::id(),"profile",5000,1).unwrap();
            assert!(request(&dir,std::process::id(),"profile",5000,1).unwrap_err().contains("active"));
            let mut seen=None;apply(&dir,&mut seen).unwrap();assert_eq!(seen.as_deref(),Some(id.as_str()));
            crate::profile::view(&dir,"guide",None).unwrap();assert_eq!(capture.health()["diagnostics"]["state"],"limited");
            apply(&dir,&mut seen).unwrap();assert_eq!(capture.health()["diagnostics"]["admitted"],1,"live request reset its volume cap");
            let mut stale=read(&dir).unwrap().unwrap();stale.id=crate::runtime::uuid().replace('-',"");stale.started+="-wrong";
            coreq::write_durable(&dir.join("telemetry.log/diagnostic.json"),&serde_json::to_string(&stale).unwrap()).unwrap();
            assert!(apply(&dir,&mut seen).unwrap_err().contains("identity-mismatch"));assert_eq!(seen.as_deref(),Some(id.as_str()));
            std::fs::write(dir.join("telemetry.log/diagnostic.json"),"{broken").unwrap();assert!(apply(&dir,&mut seen).is_err());
            assert_eq!(std::fs::read_to_string(dir.join("telemetry.log/diagnostic.json")).unwrap(),"{broken");
            let open=tracing::info_span!("owned-open-fixture");
            let failed=coreq::telemetry::capture::operation("diagnostic.fixture",||Err::<(),_>("private payload"),|_|"fixture-failure");assert!(failed.is_err());
            let local=health();assert_eq!(local["scope"],"this-process");assert_eq!(local["lost"],0);
            drop(coreq::telemetry::Store::open(&dir.join("store-span-fixture.sqlite3")).unwrap());
            let report=actions(&dir,None).unwrap();
            // #1517 A1: a store span row names its trigger and outcome and dates its slowest wait.
            let span=report.lines().find(|row|row.contains("taskq.operation.telemetry.open first-open->scan")).unwrap_or_else(||panic!("{report}"));
            assert!(span.split('|').nth(5).unwrap().trim().parse::<u64>().is_ok(),"{span}");
            assert!(report.contains("taskq.operation.diagnostic.fixture"),"{report}");assert!(report.contains("fixture-failure:1"));
            let row=report.lines().find(|row|row.contains("owned-open-fixture")).unwrap();assert!(row.contains("0/0/1"),"open span was not unknown: {row}");
            let exported=super::super::export(&dir,None).unwrap();assert!(!exported.contains("private payload"));
            let values=exported.lines().skip(1).map(|line|serde_json::from_str::<Value>(line).unwrap()["observation"].clone()).collect::<Vec<_>>();
            assert!(values.iter().any(|v|v["source"]=="diagnostic"&&v["kind"]=="method"));
            assert!(values.iter().any(|v|v["operation"]=="collector.diagnostic.state"&&v["source"]=="expired"));
            assert!(values.iter().any(|v|v["operation"]=="collector.health"));drop(open);
        });
        capture.flush().unwrap();drop(capture);drop(store);std::fs::remove_dir_all(dir).unwrap();
    }

    // #1297 A2: defect was one `collection` row per status write plus a materialized `service.loop` row each pass
    // (its counters bypass CoreQ folding). Retained protection: a simulated hour of 2 s passes over four 15-minute
    // windows keeps one health row per window, each with cumulative counters, and a lost-count change stays a row.
    #[test]
    fn steady_health_writes_one_row_per_window() {
        use std::sync::Arc;
        use tracing_subscriber::prelude::*;
        let dir=std::env::temp_dir().join(format!("taskq-health-{}",crate::runtime::uuid()));
        std::fs::create_dir_all(dir.join("telemetry.log")).unwrap();
        let store=Arc::new(coreq::telemetry::Store::open(&dir.join("telemetry.log/universal.sqlite3")).unwrap());
        let capture=coreq::telemetry::capture::Capture::new(store.clone(),"taskq","fixture").unwrap();
        let start=crate::profile::now_ms().unwrap()/HEALTH_WINDOW_MS*HEALTH_WINDOW_MS;
        tracing::subscriber::with_default(tracing_subscriber::registry().with(capture.clone()),|| for tick in 0..1800 {
            let _repeating=tracing::info_span!("service.loop",coreq_repeating=true,project="owner/repo").entered();
            assert_eq!(health_at(Some(start+tick*2000))["state"],"collecting");
        });
        capture.flush().unwrap();
        let rows=store.export(0,0,1024).unwrap()["events"].as_array().unwrap().iter().map(|r|r["observation"].clone()).collect::<Vec<_>>();
        let health:Vec<_>=rows.iter().filter(|r|r["operation"]=="collector.health").collect();
        assert_eq!(health.len(),4,"one health row per window: {}",health.len());
        assert!(health.iter().all(|r|r["counts"]["lost"]==0&&r["counts"]["accepted"].is_u64()));
        assert!(rows.iter().filter(|r|r["operation"]=="taskq.service.loop").count()<=8,"health materialized the pass root");
        drop(capture);drop(store);std::fs::remove_dir_all(dir).unwrap();
    }
}
