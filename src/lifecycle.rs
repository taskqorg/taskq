//! Exact host lifecycle intent and outcome in the existing local operation store (H-A5).
use crate::{Res,host,resources};
use coreq::telemetry::capture::CommandReceipt;
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use std::{collections::BTreeSet,fs,path::{Path,PathBuf},time::{Instant,SystemTime}};

#[derive(Clone,Copy,Debug,Default,Deserialize,Serialize,PartialEq)]
#[serde(rename_all="lowercase")]
pub enum Action {#[default] Restart,Stop,Uninstall}
impl Action {
    fn restart(&self)->bool {*self==Self::Restart}
    fn key(self)->&'static str {match self {Self::Restart=>"local_restart",Self::Stop=>"local_stop",Self::Uninstall=>"local_uninstall"}}
    pub fn title(self)->&'static str {match self {Self::Restart=>"Restart",Self::Stop=>"Stop",Self::Uninstall=>"Uninstall"}}
}
#[derive(Clone,Copy,Debug,Default,Deserialize,Serialize,PartialEq)]
#[serde(rename_all="lowercase")]
/// Quit (M15): a drain under the tray's own hold, which Open clears.
pub enum StopMode {#[default] Drain,Full,Quit}
impl StopMode {fn drain(&self)->bool {*self==Self::Drain}}

#[derive(Clone,Debug,Deserialize,Serialize,PartialEq)]
#[serde(deny_unknown_fields)]
struct Request {id:String,instance:String,roots:Vec<PathBuf>,issued_at:u64,expires_at:u64,#[serde(default,skip_serializing_if="Action::restart")] action:Action,
    #[serde(default,skip_serializing_if="Option::is_none")] project:Option<PathBuf>,#[serde(default,skip_serializing_if="StopMode::drain")] mode:StopMode}
impl Request {
    fn stops(&self)->bool {self.action==Action::Stop||self.action==Action::Uninstall&&(self.project.is_none()||self.roots.len()==1)}
    fn after(&self)->Vec<PathBuf> {self.roots.iter().filter(|root|Some(*root)!=self.project.as_ref()).cloned().collect()}
}

#[derive(Clone)]
pub struct Pending {request:Request,command:CommandReceipt,phase:String}
impl Pending {
    pub fn origin(&self)->&str {&self.request.instance}
    pub fn id(&self)->&str {&self.request.id}
    pub fn action(&self)->Action {self.request.action}
    pub fn stops(&self)->bool {self.request.stops()}
    pub fn removal(&self)->Option<&Path> {self.request.project.as_deref()}
    pub fn prepared(&self)->bool {self.phase=="applied"}
    pub fn context(&self)->Res<String> {let m:Value=serde_json::from_str(&self.command.encode()).map_err(|e|e.to_string())?;coreq::telemetry::capture::TraceContext::decode(&format!("1 {} {}",m["trace"].as_str().ok_or("trace missing")?,m["span"].as_str().ok_or("span missing")?)).map(|c|c.encode())}
}

fn now()->Res<u64> {SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d|d.as_secs()).map_err(|_|"lifecycle clock unknown".into())}
fn scope(install:&Path)->Res<Vec<PathBuf>> {host::projects(install)}
fn typed(value:&Value)->Res<Request> {
    if serde_json::to_vec(value).map_err(|e|e.to_string())?.len()>16<<10 {return Err("lifecycle request too large".into());}
    let request:Request=serde_json::from_value(value.clone()).map_err(|_|"invalid lifecycle request")?;
    if request.action!=Action::Stop && request.mode!=StopMode::Drain || !resources::id(&request.id)||!resources::id(&request.instance)||request.expires_at<=request.issued_at
        ||request.expires_at-request.issued_at>300||request.roots.iter().any(|p|!p.is_absolute())
        ||request.project.as_ref().is_some_and(|root|request.action!=Action::Uninstall||!request.roots.contains(root))
        ||request.roots.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>()!=request.roots {
        return Err("invalid lifecycle request".into());
    }
    Ok(request)
}
fn binding(op:&resources::Op)->Option<&Value> {op.bound.get("local_restart").or_else(||op.bound.get("local_stop")).or_else(||op.bound.get("local_uninstall"))}
fn parse(id:&str,op:&resources::Op)->Res<Pending> {
    let invalid="local lifecycle binding unsupported or corrupt; state kept";
    let bound=binding(op).ok_or(invalid)?;
    let request=typed(&bound["request"])?;
    if !resources::keys(&op.bound,&[request.action.key()])||!resources::keys(bound,&["request","command"]) {return Err(invalid.into());}
    if request.id!=id||op.writing||!op.sent {return Err(invalid.into());}
    let text=bound["command"].as_str().ok_or(invalid)?;
    let command=CommandReceipt::decode(text)?;
    let metadata:Value=serde_json::from_str(text).map_err(|_|invalid)?;
    if metadata["product"]!="taskq"||metadata["operation"]!=match request.action {Action::Restart=>"taskq.host.restart.request",Action::Stop=>"taskq.host.stop.request",Action::Uninstall=>"taskq.host.uninstall.request"} {return Err(invalid.into());}
    let phase=op.receipt["state"].as_str().filter(|p|matches!(*p,"queued"|"applied"|"observed")).ok_or(invalid)?.to_owned();
    let keys:&[&str]=match phase.as_str() {
        "queued"=>&["id","state","received_receipt"],"applied"=>&["id","state","received_receipt","applied_receipt"],
        _=>&["id","state","received_receipt","applied_receipt","observed_receipt","service_instance"],
    };
    let mut keys=keys.to_vec();
    if request.action==Action::Uninstall&&phase!="queued" {
        if request.stops() {if phase=="observed" {keys.push("unit_removed");if op.receipt["unit_removed"]!=true {return Err(invalid.into());}}}
        else {keys.push("project_drained");if op.receipt["project_drained"]!=true {return Err(invalid.into());}}
        if phase=="observed"&&request.project.is_some() {keys.push("unregistered");if op.receipt["unregistered"]!=true {return Err(invalid.into());}}
    }
    if let Some(owner)=op.receipt.get("registry_owner") {
        if request.action!=Action::Uninstall||request.project.is_none()||phase=="queued"||!(owner.is_array()||owner.is_object())||owner.to_string().len()>1<<20 {return Err(invalid.into());}
        keys.push("registry_owner");
    }
    if request.stops()&&phase!="queued" {
        if phase=="observed" {keys.retain(|key|*key!="service_instance");keys.push("native_exit");if op.receipt["native_exit"]!=true {return Err(invalid.into());}}
        keys.extend(["native_pid","native_started","unit_revision"]);
        let pid=op.receipt["native_pid"].as_u64().filter(|n|*n>0&&*n<=i32::MAX as u64).ok_or(invalid)?;
        let started=op.receipt["native_started"].as_str().ok_or(invalid)?;
        if pid==0||started.is_empty()||started.len()>128||!started.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-')
            ||op.receipt["unit_revision"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())) {return Err(invalid.into());}
        if let Some(launcher)=op.receipt.get("launcher") {launcher_scope(launcher)?;keys.push("launcher");}
    }
    if op.receipt.get("reason").is_some() {
        if op.receipt["reason"].as_str().is_none_or(|s|s.len()>256) {return Err(invalid.into());}keys.push("reason");
    }
    if let Some(unknown)=op.receipt.get("telemetry_unknown") {
        let phases=unknown.as_array().ok_or(invalid)?;
        if request.action!=Action::Stop || phases.is_empty() || phases.len()>3
            || phases.iter().any(|p|!matches!(p.as_str(),Some("received"|"applied"|"observed")))
            || phases.iter().enumerate().any(|(i,p)|phases[..i].contains(p)) {return Err(invalid.into());}
        keys.push("telemetry_unknown");
    }
    if !resources::keys(&op.receipt,&keys) {return Err(invalid.into());}
    if op.receipt["id"]!=id||op.receipt["received_receipt"]!=command.event_id("received")?
        ||(phase!="queued"&&op.receipt["applied_receipt"]!=command.event_id("applied")?)
        ||(phase=="observed"&&(op.receipt["observed_receipt"]!=command.event_id("observed")?
            ||(request.action==Action::Restart&&op.receipt["service_instance"].as_str().is_none_or(|s|!resources::id(s)||s==request.instance)))) {return Err(invalid.into());}
    Ok(Pending {request,command,phase})
}
fn active(state:&resources::State)->Res<Option<Pending>> {
    let mut pending=None;
    for (id,op) in &state.ops {
        if binding(op).is_none() {continue;}
        let candidate=parse(id,op)?;
        if candidate.phase!="observed" {
            if pending.is_some() {return Err("multiple local lifecycle intents; reconcile before continuing".into());}
            pending=Some(candidate);
        }
    }
    Ok(pending)
}
pub fn pending(install:&Path)->Res<Option<Pending>> {resources::try_with(install,|state|active(state))}

pub fn registry_guard(install:&Path)->Res<()> {
    // #1415: install takes this guard once; a brief holder must not fail it.
    if let Some(p)=resources::within(install,resources::WAIT,|state|active(state))? {return Err(format!("host lifecycle {} pending; project registry unchanged",p.id()));}
    Ok(())
}

pub fn uninstall_guard(install:&Path,id:&str,root:&Path)->Res<()> {
    let p=pending(install)?.ok_or("Uninstall intent missing; registry kept")?;
    if p.id()!=id||p.action()!=Action::Uninstall||p.removal()!=Some(root)||p.phase!="applied" {return Err("Uninstall registry binding differs; kept".into());}
    let roots=scope(install)?;
    if roots!=p.request.roots&&roots!=p.request.after() {return Err("Uninstall registry changed; kept".into());}
    Ok(())
}

/// Construct a review, not a command. Old PID-only/protocol-less services cannot take this operation.
pub fn review(install:&Path,id:&str)->Res<Value> {review_action(install,id,Action::Restart)}
pub fn stop_review(install:&Path,id:&str)->Res<Value> {review_action(install,id,Action::Stop)}
pub fn full_stop_review(install:&Path,id:&str)->Res<Value> {
    let mut value=stop_review(install,id)?;
    if !cfg!(target_os="macos") || host::service_status(install)["full_stop_protocol"]!=1 {return Err("Full stop unavailable on this host".into());}
    value["mode"]=json!("full");Ok(value)
}
pub fn uninstall_review(install:&Path,id:&str,project:Option<&Path>)->Res<Value> {
    let mut value=review_action(install,id,Action::Uninstall)?;
    if let Some(project)=project {value["project"]=json!(project.canonicalize().map_err(|e|e.to_string())?);}
    typed(&value)?;Ok(value)
}
fn review_action(install:&Path,id:&str,action:Action)->Res<Value> {
    let service=host::service_status(install);
    if service["state"]!="running"||service["fresh"]!=true||service["restart_protocol"]!=1 {return Err("host restart protocol unavailable or observation stale; check service status".into());}
    if action==Action::Stop {if !matches!(service["stop_protocol"].as_u64(),Some(1|2)) {return Err("host Stop protocol unavailable; check service status".into());}crate::host_control::revision(install)?;}
    if action==Action::Uninstall&&service["uninstall_protocol"]!=1 {return Err("host Uninstall protocol unavailable; check service status".into());}
    let at=now()?;
    serde_json::to_value(Request {id:id.into(),instance:service["instance"].as_str().ok_or("host generation unknown")?.into(),roots:scope(install)?,issued_at:at,expires_at:at+300,action,project:None,mode:StopMode::Drain}).map_err(|e|e.to_string())
}
fn public(op:&resources::Op)->Value {
    let mut value=op.receipt.clone();
    if let Some(fields)=value.as_object_mut() {fields.remove("registry_owner");}
    if let Some(phases)=op.receipt["telemetry_unknown"].as_array() {
        for phase in phases {value[format!("{}_receipt",phase.as_str().unwrap())]=Value::Null;}
        value["telemetry"]=json!("unknown");
    }
    value["scope"]=json!("host");if let Some(bound)=binding(op) {value["projects"]=bound["request"]["roots"].clone();value["origin_instance"]=bound["request"]["instance"].clone();value["action"]=bound["request"].get("action").cloned().unwrap_or(json!("restart"));if let Some(root)=bound["request"].get("project") {value["scope"]=json!("project");value["project"]=root.clone();}}
    if value["action"]=="stop" {value["mode"]=binding(op).unwrap()["request"].get("mode").cloned().unwrap_or(json!("drain"));value["restart_condition"]=json!(if cfg!(target_os="macos") {"next logon or explicit service install; admission remains held until reviewed recovery (#1285)"}else if cfg!(target_os="linux") {"explicit service install or user-manager startup"}else{"next logon or explicit service install"});}value
}
fn required(op:&mut resources::Op,command:&CommandReceipt,phase:&str)->Res<()> {
    if let Err(error)=crate::telemetry::receipt(command,phase) {
        if op.bound.get("local_stop").is_none() {
            op.receipt["reason"]=json!(format!("required lifecycle receipt unavailable: {}; see host log",error.chars().take(48).collect::<String>()));
            return Err(error);
        }
        op.receipt.as_object_mut().ok_or("lifecycle receipt corrupt")?.remove("reason");
        let unknown=op.receipt.as_object_mut().ok_or("lifecycle receipt corrupt")?.entry("telemetry_unknown").or_insert(json!([]));
        let phases=unknown.as_array_mut().ok_or("lifecycle receipt corrupt")?;
        if !phases.contains(&json!(phase)) {phases.push(json!(phase));}
        return Ok(());
    }
    // Missing earlier phases stay unknown even when a later write succeeds.
    op.receipt.as_object_mut().ok_or("lifecycle receipt corrupt")?.remove("reason");Ok(())
}
pub fn receipt(install:&Path,id:&str)->Res<Option<Value>> {
    if !resources::id(id) {return Err("invalid lifecycle request ID".into());}
    resources::try_with(install,|state| {
        let Some(op)=state.ops.get(id) else {return Ok(None)};
        parse(id,op)?;Ok(Some(public(op)))
    })
}
pub fn status(install:&Path,id:Option<&str>)->Value {
    let result=if let Some(id)=id {receipt(install,id)}else {resources::try_with(install,|state| {
        if let Some(p)=active(state)? {return Ok(Some(public(&state.ops[p.id()])));}
        let mut last=None;
        for (id,op) in &state.ops {
            if binding(op).is_none() {continue;}
            parse(id,op)?;
            let text=binding(op).ok_or("lifecycle binding missing")?["command"].as_str().ok_or("restart command missing")?;
            let metadata:Value=serde_json::from_str(text).map_err(|_|"restart command corrupt")?;
            let at=metadata["started_ms"].as_i64().ok_or("lifecycle clock unknown")?;
            if last.as_ref().is_none_or(|(old,_)|at>*old) {last=Some((at,public(op)));}
        }
        Ok(last.map(|(_,value)|value))
    })};
    match result {Ok(Some(value))=>value,Ok(None)=>json!({"state":"unknown","reason":"no retained lifecycle receipt"}),Err(reason)=>json!({"state":"unknown","reason":reason})}
}

pub fn submit(install:&Path,value:&Value)->Res<Value> {
    match typed(value)?.action {Action::Restart=>submit_restart(install,value),Action::Stop=>submit_stop(install,value),Action::Uninstall=>submit_uninstall(install,value)}
}
#[tracing::instrument(name="host.restart.request",skip_all)]
fn submit_restart(install:&Path,value:&Value)->Res<Value> {submit_inner(install,value)}
#[tracing::instrument(name="host.stop.request",skip_all)]
fn submit_stop(install:&Path,value:&Value)->Res<Value> {submit_inner(install,value)}
#[tracing::instrument(name="host.uninstall.request",skip_all)]
fn submit_uninstall(install:&Path,value:&Value)->Res<Value> {submit_inner(install,value)}
fn submit_inner(install:&Path,value:&Value)->Res<Value> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot restart the host service or request Stop".into());}
    let request=typed(value)?;
    let _projects=coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|format!("project registry busy: {e}"))?;
    let result=resources::with(install,|state| {
        if let Some(op)=state.ops.get(&request.id) {
            let old=parse(&request.id,op)?;
            if old.request!=request {return Err("lifecycle request ID reused with another payload".into());}
            return Ok(public(op));
        }
        if let Some(p)=active(state)? {return Err(format!("host restart {} already pending; no second request taken",p.id()));}
        resources::prepare(state,install,now()?)?;
        if state.ops.len()>=resources::MAX_OPS {return Err("operation store full; lifecycle not requested".into());}
        let at=now()?;
        if request.issued_at>at.saturating_add(30)||at>=request.expires_at {return Err("lifecycle review expired or clock unknown".into());}
        let service=host::service_status(install);
        if service["state"]!="running"||service["fresh"]!=true||service["restart_protocol"]!=1||service["instance"]!=request.instance
            ||(request.action==Action::Stop&&!matches!(service["stop_protocol"].as_u64(),Some(1|2)))||(request.action==Action::Uninstall&&service["uninstall_protocol"]!=1)||scope(install)?!=request.roots {return Err("host or project scope changed; review lifecycle action again".into());}
        if request.mode==StopMode::Full && (!cfg!(target_os="macos") || service["full_stop_protocol"]!=1) {return Err("Full stop capability changed; review again".into());}
        if request.stops() {crate::host_control::revision(install)?;}
        let command=match CommandReceipt::current() {
            Ok(command)=>command,
            Err(_) if request.action==Action::Stop=>{
                let identity=format!("{}.1",crate::runtime::uuid().replace('-', ""));
                CommandReceipt::decode(&json!({"wire":1,"product":"taskq","version":crate::platform::VERSION,
                    "operation":"taskq.host.stop.request","trace":identity,"span":identity,"parent":null,
                    "started_ms":request.issued_at.saturating_mul(1000)}).to_string())?
            },
            // #1336: no collector means no receipt; the host still honors the untracked restart file.
            Err(error) if request.action==Action::Restart=>{
                crate::platform::write_atomic(&install.join("restart"),"")?;
                return Ok(json!({"id":request.id,"state":"untracked","action":"restart","scope":"host","telemetry":"unknown",
                    "reason":format!("{error}; untracked host restart requested")}));
            },
            Err(error)=>return Err(error),
        };
        let mut op=resources::Op {bound:json!({(request.action.key()):{"request":request,"command":command.encode()}}),
            receipt:json!({"id":request.id,"state":"queued","received_receipt":command.event_id("received")?}),
            expires_at:request.expires_at,saved_at:Some(at),writing:false,sent:true};
        required(&mut op,&command,"received")?;
        if op.receipt.get("telemetry_unknown").is_some() && service["stop_protocol"]!=2 {
            return Err("Stop telemetry unavailable; upgrade the service before receipt-degraded Stop; no intent written".into());
        }
        if cfg!(target_os="macos") && request.action==Action::Stop {save_stop_hold(install,&request)?;}
        let result=public(&op);state.ops.insert(request.id.clone(),op);Ok(result)
    })?;
    if let Err(error)=crate::platform::write_atomic(&install.join("poke"),"") {
        let mut result=result;result["reason"]=json!(format!("restart saved; host notification unknown: {error}"));return Ok(result);
    }
    Ok(result)
}

/// R11: the existing hold is a safety interlock, not task state or exit evidence.
fn save_stop_hold(install:&Path,request:&Request)->Res<()> {
    let _hold=coreq::file_lock::try_lock(&install.join("admission.lock")).map_err(|e|format!("hold busy: {e}"))?;
    let path=install.join("hold");
    let mut previous=match fs::read_to_string(&path) {Ok(text)=>Some(text),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,Err(e)=>return Err(format!("hold unreadable: {e}"))};
    if let Some(prior)=stop_hold(install)? {
        if prior==*request {return Ok(());}
        let value:Value=serde_json::from_str(previous.as_deref().unwrap_or_default()).map_err(|_|"Stop hold corrupt")?;
        if prior.id==request.id || !value["recovery"].as_object().is_some_and(|ops|ops.values().any(|o|o["state"]=="accepted")) {
            return Err("another reviewed Stop hold remains; reconcile it before another Stop".into());
        }
        previous=value["previous_hold"].as_str().map(String::from);
    }
    // Old binaries must not bypass operator cancellation after an automatic rollback.
    coreq::launcher::set_rollback_floor(&crate::platform::tool(install),crate::platform::VERSION)?;
    crate::platform::write_atomic(&path,&json!({"taskq_stop":request,"previous_hold":previous}).to_string())?;
    if stop_hold(install)?.as_ref()!=Some(request) {return Err("Stop hold readback differs; cancellation unknown".into());}
    Ok(())
}
fn stop_hold(install:&Path)->Res<Option<Request>> {
    let text=match fs::read_to_string(install.join("hold")) {Ok(t)=>t,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(format!("hold unreadable: {e}"))};
    if !text.trim_start().starts_with('{') {return Ok(None);}
    let value:Value=serde_json::from_str(&text).map_err(|_|"Stop hold corrupt; ownership unknown")?;
    let request=typed(value.get("taskq_stop").ok_or("Stop hold unsupported; ownership unknown")?)?;
    if request.action!=Action::Stop {return Err("Stop hold action invalid".into());}
    Ok(Some(request))
}
pub fn full_stop(install:&Path,root:&Path)->Res<bool> {
    Ok(stop_hold(install)?.is_some_and(|r|r.mode==StopMode::Full && r.roots.iter().any(|p|p==root)))
}
pub fn stop_hold_status(install:&Path)->Value {
    match stop_hold(install) {
        Ok(Some(r))=>{
            let selected=fs::read_to_string(install.join("hold")).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok()).and_then(|v|v["recovery"].as_object().map(|ops|ops.values().filter(|o|o["state"]=="accepted").count())).unwrap_or(0);
            json!({"id":r.id,"mode":r.mode,"roots":r.roots,"state":"held","selected_count":selected})
        },
        Ok(None)=>Value::Null,Err(reason)=>json!({"state":"unknown","reason":reason}),
    }
}
/// M15 Quit: a drain Stop of a running service under the tray's own Quit hold.
pub fn quit(install:&Path)->Res<()> {
    if host::service_status(install)["state"]!="running" {return Ok(());}
    let mut review=stop_review(install,&crate::runtime::uuid().replace('-',""))?;
    review["mode"]=json!("quit");submit(install,&review).map(|_|())
}
/// M15 Open: clears only a Quit hold whose generation has gone, never a reviewed Stop hold; then starts a stopped
/// service for the registered projects.
pub fn open(install:&Path,start:impl FnOnce(&Path)->Res<String>)->Res<Option<String>> {
    let service=host::service_status(install);
    {
        let _hold=coreq::file_lock::try_lock(&install.join("admission.lock")).map_err(|e|format!("hold busy: {e}"))?;
        if let Some(r)=stop_hold(install)? {
            if r.mode!=StopMode::Quit {return Err("reviewed Stop hold remains; Open never clears it, use reviewed recovery (#1285)".into());}
            if service["state"]=="running"&&service["instance"]==json!(r.instance) {return Ok(None);}
            let path=install.join("hold");
            let value:Value=serde_json::from_str(&fs::read_to_string(&path).map_err(|e|e.to_string())?).map_err(|_|"Quit hold corrupt")?;
            match value["previous_hold"].as_str() {Some(p)=>crate::platform::write_atomic(&path,p)?,None=>fs::remove_file(&path).map_err(|e|e.to_string())?}
            let _=crate::platform::write_atomic(&install.join("poke"),"");
        }
    }
    if service["state"]=="running" {return Ok(None);}
    match scope(install)?.first() {Some(root)=>start(root).map(Some),None=>Ok(None)}
}
pub fn release_guard(install:&Path)->Res<()> {
    if stop_hold(install)?.is_some() {return Err("reviewed Stop hold requires selective recovery (#1285); ordinary release cannot clear it".into());}Ok(())
}

/// The coordinator already latched all project preparation before this required receipt.
pub fn prepare(install:&Path,instance:&str)->Res<()> {
    let unit=if pending(install)?.is_some_and(|p|p.stops()) {Some(crate::host_control::revision(install)?)}else{None};
    prepare_with(install,instance,unit.as_deref())
}
fn prepare_with(install:&Path,instance:&str,unit:Option<&str>)->Res<()> {
    resources::try_with(install,|state| {
        let Some(p)=active(state)? else {return Ok(())};
        if p.origin()!=instance {return Err("original restart already has another service generation; reconcile before another switch".into());}
        if p.action()==Action::Uninstall&&!p.stops() {return Err("project Uninstall requires a qualified project drain; compatibility preparation refused".into());}
        if scope(install)?!=p.request.roots {return Err("restart scope changed; restore the reviewed registry before continuing".into());}
        let native=if p.stops() {
            let revision=unit.ok_or("Stop unit definition unavailable")?;
            let native=coreq::process::identity(std::process::id()).map_err(|e|e.to_string())?.ok_or("native owner missing")?;
            let receipt=&state.ops[p.id()].receipt;
            if p.phase=="applied"&&(receipt["unit_revision"]!=revision||receipt["native_pid"]!=native.pid||receipt["native_started"]!=native.started) {return Err("prepared Stop identity changed; outcome unknown".into());}
            Some((native,revision))
        }else{None};
        let op=state.ops.get_mut(p.id()).unwrap();required(op,&p.command,"applied")?;
        if let Some((native,revision))=native {op.receipt["native_pid"]=json!(native.pid);op.receipt["native_started"]=json!(native.started);op.receipt["unit_revision"]=json!(revision);}
        op.receipt["state"]=json!("applied");op.receipt["applied_receipt"]=json!(p.command.event_id("applied")?);
        Ok(())
    })
}

/// Only the new native owner after full project recovery may acknowledge the original command.
pub fn observe(install:&Path,instance:&str,ready:bool)->Res<()> {
    if !ready {return Ok(());}
    resources::try_with(install,|state| {
        let Some(p)=active(state)? else {return Ok(())};
        if p.action()!=Action::Restart||p.origin()==instance {return Ok(());}
        if scope(install)?!=p.request.roots {return Err("restart outcome unknown: reviewed registry changed".into());}
        let service=host::service_status(install);
        if service["state"]!="running"||service["fresh"]!=true||service["instance"]!=instance||service["pid"]!=std::process::id() {
            return Err("restart observer is not the current native owner".into());
        }
        let op=state.ops.get_mut(p.id()).unwrap();
        required(op,&p.command,"applied")?;required(op,&p.command,"observed")?;
        op.receipt["state"]=json!("observed");op.receipt["applied_receipt"]=json!(p.command.event_id("applied")?);
        op.receipt["observed_receipt"]=json!(p.command.event_id("observed")?);op.receipt["service_instance"]=json!(instance);
        op.expires_at=now()?;
        Ok(())
    })
}

/// #1769: only its origin generation may complete a Stop, so one whose generation ended held the next generation
/// forever. The current native owner drops a queued one (no native effect began) and an applied one after its
/// expiry (its controller had that long). A reviewed Stop hold stays (#1285). Returns the dropped request ID.
pub fn supersede(install:&Path,instance:&str)->Res<Option<String>> {
    resources::try_with(install,|state| {
        let Some(p)=active(state)? else {return Ok(None)};
        if !p.stops()||p.origin()==instance||p.phase!="queued"&&now()?<state.ops[p.id()].expires_at {return Ok(None);}
        let service=host::service_status(install);
        if service["state"]!="running"||service["fresh"]!=true||service["instance"]!=instance||service["pid"]!=std::process::id() {
            return Err("Stop of an ended generation kept: this process is not the current native owner".into());
        }
        state.ops.remove(p.id());
        Ok(Some(p.id().to_owned()))
    })
}

#[derive(Clone,Copy,Debug,PartialEq)]
pub enum HandoffWait {Execution,Recovery,Project,Maintenance}
impl HandoffWait {
    pub fn reason(self)->&'static str {match self {
        Self::Execution=>"native execution drain pending; preserving occupied capacity",
        Self::Recovery=>"host ownership recovery pending; waiting for qualified reconciliation",
        Self::Project=>"project handoff pending; waiting for owned work and preparation",
        Self::Maintenance=>"host maintenance in progress; waiting for its bounded result",
    }}
}

/// Refresh only our provisional reason; a concrete controller/recovery failure remains authoritative. #1769: past
/// its expiry the reason also names what the project passes wait for (`waits`).
pub fn handoff_wait(install:&Path,instance:&str,wait:Option<HandoffWait>,waits:&str)->Res<()> {
    let at=now()?;
    resources::try_with(install,|state| {
        if let Some(p)=active(state)? && p.origin()==instance && p.phase=="queued" {
            let op=&state.ops[p.id()];
            let reason=op.receipt["reason"].as_str();
            if reason.is_none()||[HandoffWait::Execution,HandoffWait::Recovery,HandoffWait::Project,HandoffWait::Maintenance].iter().any(|wait|reason.is_some_and(|r|r.starts_with(wait.reason()))) {
                let late=at>=op.expires_at;
                match wait {
                    Some(wait)=>{
                        let text=if late {format!("{}; over expiry, waits for {waits}",wait.reason())}else{wait.reason().into()};
                        let text:String=text.char_indices().take_while(|(offset,ch)|offset+ch.len_utf8()<=256).map(|(_,ch)|ch).collect();
                        state.ops.get_mut(p.id()).unwrap().receipt["reason"]=json!(text);
                    },
                    None=>{state.ops.get_mut(p.id()).unwrap().receipt.as_object_mut().unwrap().remove("reason");},
                }
            }
        }
        Ok(())
    })
}

pub fn failed(install:&Path,id:&str,error:&str)->Res<()> {
    record_failure(install,id,error,false)
}

pub fn controller_exited(install:&Path,id:&str,error:&str)->Res<()> {
    record_failure(install,id,error,true)
}

fn record_failure(install:&Path,id:&str,error:&str,only_missing:bool)->Res<()> {
    resources::try_with(install,|state| {
        if let Some(op)=state.ops.get_mut(id) {
            let p=parse(id,op)?;
            if op.receipt["state"]!="observed"&&(!only_missing||op.receipt["reason"].is_null()) {
                let detail:String=error.char_indices().take_while(|(offset,ch)|offset+ch.len_utf8()<=112).map(|(_,ch)|ch).collect();
                op.receipt["reason"]=json!(format!("{} outcome unknown: {detail}; retry service {} --request {id}",p.action().title(),p.action().title().to_ascii_lowercase()));
            }
        }
        Ok(())
    })
}

#[tracing::instrument(name="host.stop.complete",skip_all)]
pub fn complete_stop(install:&Path,id:&str)->Res<Value> {
    let uninstall=pending(install)?.is_some_and(|p|p.id()==id&&p.action()==Action::Uninstall);
    let result=complete_stop_with(install,id,|native,revision| {
        if uninstall {crate::host_control::remove(install,id,native,revision)?;}else{crate::host_control::stop(install,id,native,revision)?;}
        crate::host_control::retire_controller(install,id)
    });
    if let Err(error)=&result {if let Err(save)=failed(install,id,error) {crate::platform::log(&format!("Stop failure readback unavailable: {save}"));}}
    result
}

fn launcher_scope(value:&Value)->Res<()> {
    if !resources::keys(value,&["pid","started","instance"])||value["pid"].as_u64().is_none_or(|n|n==0||n>i32::MAX as u64)
        ||value["started"].as_str().is_none_or(|s|s.is_empty()||s.len()>20||s.parse::<u64>().is_err())
        ||value["instance"].as_str().is_none_or(|s| {let s=if s.len()==38&&s.starts_with('{')&&s.ends_with('}') {&s[1..37]}else{s};s.len()!=36||s.bytes().enumerate().any(|(i,b)|if [8,13,18,23].contains(&i){b!=b'-'}else{!b.is_ascii_hexdigit()})}) {
        return Err("native launcher scope invalid; state kept".into());
    }
    Ok(())
}
#[cfg(any(windows,test))]
pub fn saved_launcher(install:&Path,id:&str,native:&coreq::process::Identity)->Res<Option<Value>> {
    resources::try_with(install,|state| {
        let op=state.ops.get(id).ok_or("Stop binding missing")?;let p=parse(id,op)?;
        if !p.stops()||p.phase!="applied"||op.receipt["native_pid"]!=native.pid||op.receipt["native_started"]!=native.started {return Err("Stop launcher binding differs".into());}
        Ok(op.receipt.get("launcher").cloned())
    })
}
#[cfg(any(windows,test))]
pub fn save_launcher(install:&Path,id:&str,native:&coreq::process::Identity,launcher:&Value)->Res<()> {
    launcher_scope(launcher)?;
    resources::try_with(install,|state| {
        let op=state.ops.get_mut(id).ok_or("Stop binding missing")?;let p=parse(id,op)?;
        if !p.stops()||p.phase!="applied"||op.receipt["native_pid"]!=native.pid||op.receipt["native_started"]!=native.started {return Err("Stop launcher binding differs".into());}
        if op.receipt.get("launcher").is_some_and(|old|old!=launcher) {return Err("native launcher identity changed; original scope kept".into());}
        op.receipt["launcher"]=launcher.clone();Ok(())
    })
}

fn registry_owner(install:&Path,request:&Request,op:&resources::Op,roots:&[PathBuf])->Res<Value> {
    let current=host::ownership(&install.join("projects"))?;
    let owner=match op.receipt.get("registry_owner") {
        Some(owner)=>owner.clone(),None if roots==request.roots=>current.clone(),
        None=>return Err("Uninstall original registry ownership unknown; removal remains applied".into()),
    };
    if current!=owner {return Err("Uninstall registry ownership differs; removal remains applied".into());}
    Ok(owner)
}

fn complete_stop_with(install:&Path,id:&str,effect:impl FnOnce(&coreq::process::Identity,&str)->Res<()>)->Res<Value> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot complete host Stop".into());}
    if !resources::id(id) {return Err("invalid Stop request ID".into());}
    let candidate=pending(install)?;
    let edit=match candidate.as_ref().filter(|p|p.id()==id&&p.action()==Action::Uninstall).and_then(Pending::removal) {
        Some(root)=>Some(host::ProjectEdit::for_uninstall(install,root,id)?),None=>None,
    };
    let _projects=if edit.is_none() {Some(coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|format!("Stop registry busy: {e}"))?)}else{None};
    let (pending,original)=resources::try_with(install,|state| {
        let op=state.ops.get_mut(id).ok_or("Stop request not retained")?;
        let pending=parse(id,op)?;
        if !pending.stops()||pending.phase=="queued" {return Err("Stop has no qualified applied receipt; native effect refused".into());}
        if pending.phase!="observed"&&pending.removal().is_some() {op.receipt["registry_owner"]=registry_owner(install,&pending.request,op,&scope(install)?)?;}
        Ok((pending,op.clone()))
    })?;
    if pending.phase=="observed" {return Ok(public(&original));}
    let roots=scope(install)?;
    if roots!=pending.request.roots&&!(pending.action()==Action::Uninstall&&roots==pending.request.after()) {return Err("Stop registry changed; native effect refused".into());}
    let native=coreq::process::Identity {pid:original.receipt["native_pid"].as_u64().ok_or("Stop pid unknown")? as u32,started:original.receipt["native_started"].as_str().ok_or("Stop identity unknown")?.into()};
    let service=host::service_status(install);
    if service["instance"]!=pending.origin()||service["pid"]!=native.pid||service["started"]!=native.started||service["state"]=="unknown" {
        return Err("Stop host incarnation differs or is unknown; reconcile the original owner".into());
    }
    effect(&native,original.receipt["unit_revision"].as_str().ok_or("Stop unit revision missing")?)?;
    if coreq::process::identity(native.pid).map_err(|e|e.to_string())?.as_ref()==Some(&native) {return Err("Stop native exit not observed".into());}
    let _owner=coreq::file_lock::try_lock(&install.join("service.lock")).map_err(|e|format!("host owner still held after Stop: {e}"))?;
    let service=host::service_status(install);
    if service["state"]!="exited"||service["instance"]!=pending.origin() {return Err("host changed after Stop; outcome unknown".into());}
    if let Some(edit)=edit {edit.commit()?;}
    if pending.removal().is_some()&&host::ownership(&install.join("projects"))?!=original.receipt["registry_owner"] {return Err("Uninstall registry ownership readback differs; removal remains applied".into());}
    resources::try_with(install,|state| {
        let op=state.ops.get_mut(id).ok_or("Stop request lost; outcome unknown")?;
        let current=parse(id,op)?;
        if current.request!=pending.request||op.bound!=original.bound {return Err("Stop binding changed; outcome unknown".into());}
        required(op,&pending.command,"applied")?;required(op,&pending.command,"observed")?;
        op.receipt["state"]=json!("observed");op.receipt["observed_receipt"]=json!(pending.command.event_id("observed")?);op.receipt["native_exit"]=json!(true);
        if pending.action()==Action::Uninstall {op.receipt["unit_removed"]=json!(true);if pending.removal().is_some() {op.receipt["unregistered"]=json!(true);}}
        op.expires_at=now()?;
        Ok(public(op))
    })
}

/// The host calls this only after a qualified project-thread drain, or an already-applied removal readback.
pub fn finish_project_uninstall(install:&Path,id:&str,instance:&str,drained:bool)->Res<()> {
    let p=pending(install)?.filter(|p|p.id()==id&&p.action()==Action::Uninstall&&!p.stops()).ok_or("project Uninstall intent missing")?;
    let root=p.removal().ok_or("project Uninstall root missing")?;
    let service=host::service_status(install);
    if service["state"]!="running"||service["fresh"]!=true||service["instance"]!=instance||service["pid"]!=std::process::id() {return Err("Uninstall observer is not the current native owner".into());}
    let roots=scope(install)?;
    if roots!=p.request.roots&&roots!=p.request.after() {return Err("Uninstall scope changed; registry kept".into());}
    if !drained&&!(p.phase=="applied"&&roots==p.request.after()) {return Err("project drain not observed; registry kept".into());}
    let owner=resources::try_with(install,|state| {
        let op=state.ops.get_mut(id).ok_or("Uninstall binding lost")?;
        if parse(id,op)?.request!=p.request {return Err("Uninstall binding changed".into());}
        let owner=registry_owner(install,&p.request,op,&roots)?;
        required(op,&p.command,"applied")?;op.receipt["state"]=json!("applied");op.receipt["project_drained"]=json!(true);op.receipt["applied_receipt"]=json!(p.command.event_id("applied")?);op.receipt["registry_owner"]=owner.clone();Ok(owner)
    })?;
    host::ProjectEdit::for_uninstall(install,root,id)?.commit()?;
    if scope(install)?!=p.request.after() {return Err("Uninstall registry readback differs".into());}
    if host::ownership(&install.join("projects"))?!=owner {return Err("Uninstall registry ownership readback differs; removal remains applied".into());}
    resources::try_with(install,|state| {
        let op=state.ops.get_mut(id).ok_or("Uninstall binding lost")?;
        if parse(id,op)?.request!=p.request {return Err("Uninstall binding changed".into());}
        required(op,&p.command,"observed")?;op.receipt["state"]=json!("observed");op.receipt["observed_receipt"]=json!(p.command.event_id("observed")?);op.receipt["unregistered"]=json!(true);op.receipt["service_instance"]=json!(instance);op.expires_at=now()?;Ok(())
    })
}

pub struct Monitor {checked:Option<Instant>,modified:Res<Option<SystemTime>>,pub current:Res<Option<Pending>>}
impl Default for Monitor {fn default()->Self {Self {checked:None,modified:Ok(None),current:Ok(None)}}}
impl Monitor {
    pub fn refresh(&mut self,install:&Path) {
        let modified=match fs::metadata(resources::dir(install).join("state.json")) {
            Ok(m)=>m.modified().map(Some).map_err(|e|e.to_string()),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(e)=>Err(e.to_string()),
        };
        let busy=self.current.as_ref().err().is_some_and(|e|e.starts_with("resources lock unavailable:"));
        if !busy&&self.checked.is_some_and(|at|at.elapsed().as_secs()<60)&&self.modified==modified {return;}
        self.checked=Some(Instant::now());self.modified=modified;
        self.current=self.modified.as_ref().map_err(Clone::clone).and_then(|_|pending(install));
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
