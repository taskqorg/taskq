//! R9/R12: launch preferences request capabilities; only the operator policy grants them.
use std::{fs,path::{Path,PathBuf}};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
type Res<T> = Result<T,String>;
#[derive(Clone,Default,Debug,PartialEq,Serialize,Deserialize)]
#[serde(default,deny_unknown_fields)]
pub struct Choice {
    pub model:String,
    pub effort:String,
    pub permission:String,
    pub grant_mode:String,
    pub grant_revision:Option<u64>,
}

fn models_at(path:&Path)->Res<Value> {
    let value:Value=serde_json::from_slice(&fs::read(path).map_err(|_|"Installed model catalog unavailable; use native defaults or refresh Codex")?).map_err(|_|"Installed model catalog unreadable")?;
    let at=value["fetched_at"].as_str().and_then(crate::board::iso).ok_or("Model catalog time unknown")?;
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"Model catalog clock unknown")?.as_secs() as i64;
    if !(0..=86400).contains(&(now-at)) {return Err("Model catalog stale; refresh Codex before choosing a model".into());}
    let rows=value["models"].as_array().ok_or("Model catalog incomplete")?;
    Ok(Value::Array(rows.iter().filter(|m|m["visibility"]=="list").map(|m|json!({"id":m["slug"],"name":m["display_name"],"efforts":m["supported_reasoning_levels"],"default_effort":m["default_reasoning_level"]})).collect()))
}
pub fn catalog(runtime:&str)->Value {
    let result=if super::of(runtime).codex {super::codex_home().ok_or("Codex home unknown".into()).and_then(|home|models_at(&home.join("models_cache.json")))}
        else {return native_catalog(runtime)};
    match result {Ok(models)=>json!({"models":models,"reason":null}),Err(reason)=>json!({"models":[],"reason":reason})}
}
struct NativeCatalog {value:Value,at:Option<std::time::Instant>,pending:Option<std::sync::mpsc::Receiver<Value>>}
fn native_catalog(runtime:&str)->Value {
    static CATALOGS:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<String,NativeCatalog>>>=std::sync::OnceLock::new();
    let mut cache=match CATALOGS.get_or_init(Default::default).lock(){Ok(cache)=>cache,Err(_)=>return json!({"models":[],"reason":"Native model catalog lock unavailable"})};
    let entry=cache.entry(runtime.into()).or_insert_with(||NativeCatalog{value:json!({"models":[],"reason":"Reading installed model catalog"}),at:None,pending:None});
    if let Some(receiver)=&entry.pending {match receiver.try_recv(){Ok(value)=>{entry.value=value;entry.pending=None;entry.at=Some(std::time::Instant::now());},Err(std::sync::mpsc::TryRecvError::Disconnected)=>{entry.pending=None;entry.at=Some(std::time::Instant::now());entry.value=json!({"models":[],"reason":"Native catalog read stopped"});},_=>{}}}
    let ttl=if entry.value["reason"].is_null(){600}else{30};
    if entry.pending.is_none()&&entry.at.is_none_or(|at|at.elapsed().as_secs()>=ttl) {
        let (tx,rx)=std::sync::mpsc::channel();entry.pending=Some(rx);let runtime=runtime.to_string();
        std::thread::spawn(move||{let value=read_native_catalog(&runtime).unwrap_or_else(|reason|json!({"models":[],"reason":reason}));let _=tx.send(value);});
    }
    let mut value=entry.value.clone();if entry.pending.is_some(){value["reason"]=json!("Refreshing installed model catalog; explicit model launch waits");}value
}
fn read_native_catalog(runtime:&str)->Res<Value> {
    let adapter=super::of(runtime);let bin=super::which(adapter.bin);
    if !bin.is_absolute(){return Err("Native CLI unavailable; model support unknown".into());}
    let (ok,text)=super::probe("runtime.models",&bin,if adapter.remote {&["--help"]}else{&["models"]}).ok_or("Native model catalog read timed out")?;
    if !ok {return Err("Native model catalog refused; inspect the installed client".into());}
    native_models(adapter.remote,&text)
}
fn native_models(aliases:bool,text:&str)->Res<Value> {
    let models=if aliases {
        // CLI aliases track the installed client's catalog; they are labelled as aliases, never exact model IDs.
        ["opus","sonnet","fable"].into_iter().filter(|alias|text.contains(&format!("'{alias}'"))).map(|alias|json!({"id":alias,"name":format!("{alias} (installed alias)"),"efforts":[{"effort":""}],"default_effort":"","effort_reason":"Installed Claude CLI does not expose model-specific effort pairs; native effort only"})).collect::<Vec<_>>()
    }else {
        text.lines().filter_map(|line|line.split_once('\t')).filter(|(id,_)|!id.is_empty()&&id.len()<=120&&id.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'||c==b'.')).map(|(id,name)|{
            let effort=id.rsplit_once('-').map(|(_,effort)|effort).filter(|effort|["low","medium","high","xhigh","max"].contains(effort)).unwrap_or("");
            json!({"id":id,"name":name,"efforts":[{"effort":effort}],"default_effort":effort})
        }).collect()
    };
    if models.is_empty(){return Err("Installed client returned no supported model catalog".into());}Ok(json!({"models":models,"reason":null}))
}

impl Choice {
    pub fn validate(&self,runtime:&str)->Res<()> {
        if !["","restricted","workspace","full","legacy"].contains(&self.grant_mode.as_str())||!["","inherit","workspace","full"].contains(&self.permission.as_str()) {return Err("Unsupported permission request; draft kept".into());}
        if self.model.is_empty() {return if self.effort.is_empty(){Ok(())}else{Err("Choose a supported model before effort".into())};}
        let catalog=if super::of(runtime).codex {catalog(runtime)}else{read_native_catalog(runtime)?};
        if let Some(reason)=catalog["reason"].as_str(){return Err(reason.into());}
        let model=catalog["models"].as_array().and_then(|rows|rows.iter().find(|m|m["id"]==self.model)).ok_or_else(||catalog["reason"].as_str().unwrap_or("Selected model unavailable in the installed catalog; no fallback").to_string())?;
        if !model["efforts"].as_array().is_some_and(|rows|rows.iter().any(|e|e["effort"]==self.effort)) {return Err("Selected effort unsupported for this model; no fallback".into());}Ok(())
    }
    pub fn codex(&self,original:&[String])->Vec<String> {
        if self.model.is_empty(){return original.to_vec();}
        let mut args=vec![];let mut it=original.iter();
        while let Some(arg)=it.next() {match arg.as_str(){"-m"|"--model"=>{it.next();},"-c"|"--config"=>{if let Some(value)=it.next(){if !value.starts_with("model_reasoning_effort="){args.extend([arg.clone(),value.clone()]);}}},s if s.starts_with("--model=")||s.starts_with("--config=model_reasoning_effort=")=>{},_=>args.push(arg.clone())}}
        args.extend(["-m".into(),self.model.clone(),"-c".into(),format!("model_reasoning_effort={}",self.effort)]);args
    }
    pub fn access(&self,install:&Path,root:&Path,runtime:&str,mode:&str,args:&[String])->Res<crate::policy::Effective> {
        let mut policy=crate::policy::load(install)?;
        if self.grant_revision!=Some(policy.revision) {return Err("Operator grant changed or freshness unknown; review launch choices again".into());}
        let home=std::env::var_os("HOME").or_else(||std::env::var_os("USERPROFILE")).map(PathBuf::from).ok_or("Host home unknown")?;
        let host=crate::policy::Host::here(install,root,&home);
        let original=crate::policy::resolve(&Ok(policy.clone()),&host,runtime,mode,args);
        if self.permission=="full"&&original.mode!="full" {return Err("Full/bypass request requires an existing operator Full grant for this scope".into());}
        if self.permission=="workspace"&&!matches!(original.mode.as_str(),"workspace"|"restricted"|"full") {return Err("Workspace request requires an existing operator grant with supported containment".into());}
        if (self.permission.is_empty()||self.permission=="inherit")&&matches!(self.grant_mode.as_str(),""|"legacy")&&original.mode!=self.grant_mode {return Err("Original inherited authority changed; review a new request".into());}
        let requested=if self.permission.is_empty()||self.permission=="inherit" {self.grant_mode.as_str()}else{self.permission.as_str()};
        if matches!(requested,"restricted"|"workspace"|"full")&&!matches!(original.mode.as_str(),"restricted"|"workspace"|"full"){return Err("Original operator authority is unavailable for this project; no fallback".into());}
        if matches!(requested,"restricted"|"workspace")&&original.mode=="full" || requested=="restricted"&&original.mode=="workspace" {
            let canonical=root.canonicalize().map_err(|_|"Launch project unavailable")?.display().to_string();
            if let Some(grant)=policy.grants.iter_mut().find(|g|g.runtime==runtime&&g.project.as_deref()==Some(&canonical)){grant.mode=requested.into();}
            else if let Some(grant)=policy.grants.iter_mut().find(|g|g.runtime==runtime&&g.project.is_none()){grant.mode=requested.into();}
        }
        let mut effective=crate::policy::resolve(&Ok(policy),&host,runtime,mode,args);
        effective.grant=original.grant;
        if let Some(reason)=&effective.refused {return Err(reason.clone());}Ok(effective)
    }
}

fn worker_defaults(runtime:&str,args:&[String])->String {
    if !super::of(runtime).codex {return "Native model and effort".into();}
    let(mut model,mut effort)=("Native model".to_string(),"Native effort".to_string());let mut it=args.iter();
    while let Some(arg)=it.next() {match arg.as_str(){"-m"|"--model"=>{if let Some(value)=it.next(){model=value.clone();}},"-c"|"--config"=>{if let Some(value)=it.next().and_then(|v|v.strip_prefix("model_reasoning_effort=")){effort=value.into();}},value if value.starts_with("--model=")=>model=value[8..].into(),value if value.starts_with("--config=model_reasoning_effort=")=>effort=value[32..].into(),_=>{}}}
    let effort=if effort=="low" {"Light (low)"}else{&effort};format!("{model} · {effort}")
}

fn access_view(access:&crate::policy::Effective)->Value {let mut value=access.json();value.as_object_mut().unwrap().remove("requested");value}

/// Fresh project-scoped facts, not a saved grant. Host policy/model defaults are read-only here.
pub fn view(install:&Path,root:&Path,runtime:&str)->Value {
    let result=(||->Res<Value>{
        let policy=crate::host::Policy::read_for_hostname(install,crate::hostname)?;
        let grants=crate::policy::load(install)?;
        let home=std::env::var_os("HOME").or_else(||std::env::var_os("USERPROFILE")).map(PathBuf::from).ok_or("Host home unknown")?;
        let target=if super::of(runtime).codex{super::of(runtime).setup}else{runtime};
        let effective=crate::policy::resolve(&Ok(grants.clone()),&crate::policy::Host::here(install,root,&home),target,&policy.permission_mode,&policy.codex);
        let worker=worker_defaults(runtime,&policy.codex);
        let requests=["inherit","workspace","full"].into_iter().map(|permission|{
            let choice=Choice {permission:permission.into(),grant_revision:Some(grants.revision),grant_mode:effective.mode.clone(),..Default::default()};
            let value=match choice.access(install,root,target,&policy.permission_mode,&policy.codex){Ok(access)=>access_view(&access),Err(reason)=>json!({"refused":reason})};
            (permission.to_string(),value)
        }).collect::<serde_json::Map<String,Value>>();
        Ok(json!({"requests":requests,"grant_revision":grants.revision,"access":access_view(&effective),"worker_defaults":worker,"scope":root,"catalog":catalog(runtime)}))
    })();match result{Ok(v)=>v,Err(reason)=>json!({"reason":reason,"catalog":catalog(runtime)})}
}

#[cfg(test)]
mod tests {
    use super::*;
    // R9/A4: permission requests only narrow existing authority; neither stale state nor Full requests grant access.
    #[test]
    fn requests_narrow_and_preserve_the_operator_store() {
        let root=std::env::temp_dir().join(format!("taskq-choice-{}",super::super::uuid()));fs::create_dir_all(&root).unwrap();
        let root=root.canonicalize().unwrap();let file=root.join(crate::policy::FILE);let policy=json!({"revision":7,"grants":[{"runtime":"claude","mode":"full","project":root,"by":"invented operator","at":1}]});fs::write(&file,policy.to_string()).unwrap();
        let mut choice=Choice{permission:"workspace".into(),grant_mode:"full".into(),grant_revision:Some(7),..Default::default()};
        let result=choice.access(&root,&root,"claude","dontAsk",&[]);
        if cfg!(target_os="macos"){let access=result.unwrap();assert_eq!(access.mode,"workspace");assert!(access.profile.is_some());assert!(access.grant.starts_with("full (project)"));}else{assert!(result.is_err(),"unsupported containment refuses");}
        assert_eq!(fs::read_to_string(&file).unwrap(),policy.to_string());
        choice.grant_revision=Some(6);assert!(choice.access(&root,&root,"claude","dontAsk",&[]).unwrap_err().contains("grant changed"));
        choice.grant_revision=Some(7);choice.permission="full".into();let mut narrowed=policy.clone();narrowed["grants"][0]["mode"]=json!("workspace");fs::write(&file,narrowed.to_string()).unwrap();assert!(choice.access(&root,&root,"claude","dontAsk",&[]).unwrap_err().contains("requires an existing"));
        fs::remove_dir_all(root).unwrap();
    }
    // A1/R12: a generic CLI effort flag is not a model-specific catalog.
    #[test]
    fn native_catalogs_never_invent_model_effort_pairs() {
        let aliases=native_models(true,"Provide an alias e.g. 'opus' or 'sonnet'; --effort low medium high xhigh max").unwrap();
        assert!(aliases["models"].as_array().unwrap().iter().all(|model|model["efforts"]==json!([{"effort":""}])));
        let native=native_models(false,"gemini-4-argon-low\tGemini 4 Argon (Low)\ninvalid row").unwrap();assert_eq!(native["models"][0]["id"],"gemini-4-argon-low");assert_eq!(native["models"][0]["efforts"],json!([{"effort":"low"}]));assert!(native_models(false,"unreadable output").is_err());
    }

    // A1/R9: one supported exact choice replaces only model/effort, preserving permission/network requests.
    #[test]
    fn explicit_codex_choice_keeps_other_requests() {
        let choice=Choice{model:"gpt-6.1-sol".into(),effort:"low".into(),..Default::default()};
        let original=["-s","workspace-write","--model=old","-c","model_reasoning_effort=high","-c","sandbox_workspace_write.network_access=true"].map(String::from);
        assert_eq!(choice.codex(&original),["-s","workspace-write","-c","sandbox_workspace_write.network_access=true","-m","gpt-6.1-sol","-c","model_reasoning_effort=low"].map(String::from));
        assert!(!worker_defaults("codex",&["-c".into(),"api_key=invented-secret".into()]).contains("invented-secret"));
    }
}
