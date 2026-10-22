//! Optional agentq onboarding (taskq.md § 4): bounded native readback, host preference and shared setup requests.
use std::{fs, io::Read, path::{Path,PathBuf}, sync::{Arc,Mutex,OnceLock}, time::{Duration,Instant,SystemTime}};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use crate::Res;

const FILE:&str="tray-agentq.json";
#[derive(Default,Deserialize,Serialize,PartialEq)]
#[serde(deny_unknown_fields)]
struct Preferences { schema:u8, dismissed:bool, warning:bool, request:Option<String> }
fn preferences(install:&Path)->Res<Preferences> {
    match crate::data::json(&install.join(FILE))? {
        None=>Ok(Preferences{schema:1,..Default::default()}),
        Some(v)=>{let p:Preferences=serde_json::from_value(v).map_err(|e|format!("{FILE}: {e}; kept"))?;
            if p.schema!=1 || p.request.as_ref().is_some_and(|id|!crate::resources::id(id)){return Err(format!("{FILE}: unsupported record; kept"));} Ok(p)}
    }
}
fn save(install:&Path,p:&Preferences)->Res<()> {
    crate::platform::write_atomic(&install.join(FILE),&serde_json::to_string(p).map_err(|e|e.to_string())?).map_err(|e|format!("{FILE}: {e}"))?;
    if preferences(install)?!=*p {return Err(format!("{FILE}: readback changed"));} Ok(())
}
fn locked<T>(install:&Path,f:impl FnOnce()->Res<T>)->Res<T> {
    let _lock=coreq::file_lock::try_lock(&install.join("tray-agentq.lock")).map_err(|e|format!("agentq preference busy: {e}"))?; f()
}
pub fn root()->Res<PathBuf> {
    let key=if cfg!(windows){"LOCALAPPDATA"}else{"HOME"};
    let base=std::env::var_os(key).map(PathBuf::from).filter(|p|p.is_absolute()).ok_or_else(||format!("{key} unavailable"))?;
    Ok(if cfg!(windows){base.join("agentq")}else{base.join(".local/share/agentq")})
}
fn targets(runtime:&str)->Res<Vec<String>> {
    let home=std::env::var_os(if cfg!(windows){"USERPROFILE"}else{"HOME"}).map(PathBuf::from).filter(|p|p.is_absolute()).ok_or("native client home unavailable")?;
    Ok(vec![root()?.display().to_string(),home.join(if runtime=="codex"{".codex"}else{".claude"}).display().to_string()])
}
fn unknown(reason:impl ToString)->Value {json!({"kind":"unknown","reason":reason.to_string()})}
fn exists(path:&Path)->Res<bool> {
    match fs::metadata(path) {Ok(m) if m.is_file()=>Ok(true),Ok(_)=>Err("expected a regular file".into()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>match fs::symlink_metadata(path){Ok(_)=>Err("launcher is a dangling link".into()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(false),Err(e)=>Err(e.to_string())},Err(e)=>Err(e.to_string())}
}
fn status(path:&Path)->Res<Option<(Value,u64)>> {
    if !crate::messages::plain(path)?{return Ok(None);}
    if !fs::metadata(path).map_err(|e|e.to_string())?.is_file(){return Err("status is not a regular file".into());}
    let f=match fs::File::open(path){Ok(f)=>f,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(e.to_string())};
    let age=f.metadata().and_then(|m|m.modified()).map_err(|e|e.to_string())?.elapsed().map_err(|_|"status timestamp is in the future")?.as_secs();
    let mut bytes=Vec::new();f.take(1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len()>1024*1024{return Err("status exceeds 1 MiB".into());}
    Ok(Some((serde_json::from_slice(&bytes).map_err(|e|format!("malformed status: {e}"))?,age)))
}
fn now()->u64 {SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0,|d|d.as_secs())}
fn inspect(root:&Path,launcher:Res<bool>,status:Res<Option<(Value,u64)>>,clock:u64,process:impl FnOnce(u32)->Res<Option<PathBuf>>)->Value {
    let (launcher,status)=match (launcher,status){(Ok(l),Ok(s))=>(l,s),(Err(e),_)|(_,Err(e))=>return unknown(e)};
    let Some((s,age))=status else{return if launcher{unknown("installed launcher; service status missing")}else{json!({"kind":"shown"})}};
    let version=s["version"].as_str().unwrap_or("");
    let parts:Vec<_>=version.split('.').collect();
    let compatible=parts.len()==3 && parts[0]=="0" && parts[1].parse::<u32>().is_ok_and(|v|(5..=9).contains(&v)) && parts[2].parse::<u32>().is_ok();
    let Some(at)=s["at"].as_u64() else{return unknown("status observation time missing")};
    let Some(pid)=s["pid"].as_u64().and_then(|p|u32::try_from(p).ok()).filter(|p|*p>0 && *p<=i32::MAX as u32) else{return unknown("status process identity missing")};
    if !compatible || s["project"]!="host" {return unknown("unsupported agentq status version or shape");}
    if age>90 || at>clock || clock-at>90 {return unknown("agentq status stale or timestamp is in the future");}
    if !launcher {return unknown("status exists without the known launcher");}
    match process(pid) {
        Ok(None)=>json!({"kind":"stopped","reason":"agentq installed · service stopped (fresh status, recorded process absent)"}),
        Ok(Some(image))=>{
            let expected=root.join("releases").join(version).join(if cfg!(windows){"agentq.exe"}else{"agentq"});
            let same=if cfg!(windows){image.to_string_lossy().replace('\\',"/").eq_ignore_ascii_case(&expected.to_string_lossy().replace('\\',"/"))}else{image==expected};
            if same {json!({"kind":"running","version":version})} else {unknown("reported PID belongs to another executable")}
        },Err(e)=>unknown(e)
    }
}
fn process(pid:u32)->Res<Option<PathBuf>> {
    let before=coreq::process::identity(pid).map_err(|e|format!("process identity unknown: {e}"))?;
    let Some(before)=before else{return Ok(None)};
    let image=image(pid)?;
    if coreq::process::identity(pid).map_err(|e|e.to_string())?!=Some(before){return Err("process identity changed during readback".into());} Ok(Some(image))
}
#[cfg(target_os="macos")]
fn image(pid:u32)->Res<PathBuf> {
    #[link(name="proc")]
    unsafe extern "C" {fn proc_pidpath(pid:i32,buffer:*mut libc::c_void,buffersize:u32)->i32;}
    let mut buffer=[0u8;4096];
    // SAFETY: native read-only query into a correctly sized caller-owned buffer.
    let n=unsafe{proc_pidpath(pid as i32,buffer.as_mut_ptr().cast(),buffer.len() as u32)};
    if n<=0{return Err(format!("process executable unknown: {}",std::io::Error::last_os_error()));}
    use std::os::unix::ffi::OsStringExt;
    let end=buffer.iter().position(|b|*b==0).unwrap_or(n as usize);
    Ok(PathBuf::from(std::ffi::OsString::from_vec(buffer[..end].to_vec())))
}
#[cfg(windows)]
fn image(pid:u32)->Res<PathBuf> {
    #[link(name="kernel32")]
    unsafe extern "system" {fn OpenProcess(access:u32,inherit:i32,pid:u32)->*mut std::ffi::c_void;fn QueryFullProcessImageNameW(process:*mut std::ffi::c_void,flags:u32,name:*mut u16,size:*mut u32)->i32;fn CloseHandle(handle:*mut std::ffi::c_void)->i32;}
    // SAFETY: query-only process handle; closed on every path after successful open.
    unsafe {
        let handle=OpenProcess(0x1000,0,pid);if handle.is_null(){return Err(format!("process executable unknown: {}",std::io::Error::last_os_error()));}
        let mut buffer=vec![0u16;32768];let mut size=buffer.len() as u32;
        let result=QueryFullProcessImageNameW(handle,0,buffer.as_mut_ptr(),&mut size);
        let error=std::io::Error::last_os_error();CloseHandle(handle);
        if result==0{return Err(format!("process executable unknown: {error}"));}
        use std::os::windows::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size as usize])))
    }
}
#[cfg(not(any(target_os="macos",windows)))]
fn image(_:u32)->Res<PathBuf>{Err("native agentq executable probe unavailable".into())}
fn detect()->Value {
    match root(){Ok(r)=>inspect(&r,exists(&r.join(if cfg!(windows){"agentq.exe"}else{"agentq"})),status(&r.join("status.json")),now(),process),Err(e)=>unknown(e)}
}
fn project(install:&Path,mut state:Value)->Value {
    let result=locked(install,||{
        let mut p=preferences(install)?;let old=serde_json::to_value(&p).unwrap();
        if state["kind"]=="running" {p.dismissed=false;p.warning=false;p.request=None;}
        else if state["kind"]=="shown" {p.warning=true;}
        if serde_json::to_value(&p).unwrap()!=old {save(install,&p)?;}
        state["dismissed"]=json!(p.dismissed);state["warning"]=json!(p.warning&&!p.dismissed);
        if let Some(id)=p.request {
            match crate::setup::read(&crate::setup::dir(install).join(format!("{id}.json"))){
                Ok(r)=>{let s=crate::setup::view(&r);state["kind"]=json!(match s["kind"].as_str(){Some("starting"|"launched")=>"setting-up",Some("interrupted")=>"interrupted",_=>"failed"});state["setup"]=s;},
                Err(e)=>{state["setup"]=unknown(e);state["kind"]=json!("interrupted");}
            }
        } Ok(())
    });
    if let Err(e)=result {state["kind"]=json!("unknown");state["reason"]=json!(e);state["warning"]=json!(true);}
    state
}
fn runtimes(install:&Path)->Value {
    let rows=crate::runtime::agents(install);
    let policy=crate::host::Policy::read_for_hostname(install,crate::hostname);
    ["codex","claude"].iter().map(|rt|{
        let row=rows.as_array().and_then(|rows|rows.iter().find(|r|r["runtime"]==*rt));
        let reason=match (row,&policy){
            (_,Err(e))=>Some(format!("host policy unknown: {e}")),
            (Some(r),_) if r["login"]!="logged in"=>Some(format!("{}",r["login"].as_str().unwrap_or("login unknown"))),
            (Some(_),Ok(p))=>{
                let exe=std::env::current_exe().unwrap_or_default();let work=crate::setup::dir(install);
                let env=crate::runtime::Env{root:&work,exe:&exe,install,permission_mode:&p.permission_mode,codex:&p.codex,publish:"",board_env:&[]};
                crate::runtime::access(&env,if *rt=="codex"{"codex-app"}else{"claude"}).err().map(|e|e.message)
            },_=>Some("runtime unknown".into())
        };
        json!({"runtime":rt,"client":if *rt=="codex"{"Codex"}else{"Claude Code"},"eligible":reason.is_none(),"reason":reason})
    }).collect::<Vec<_>>().into()
}
/// A single background probe at the existing 2 s cadence; renders never run a CLI or read credentials.
static CACHE:OnceLock<Arc<Mutex<Value>>>=OnceLock::new();
pub fn start(install:&Path) {
    CACHE.get_or_init(||{
        let cache=Arc::new(Mutex::new(unknown("agentq readback pending")));let output=cache.clone();let install=install.to_path_buf();
        std::thread::spawn(move||{
            let mut choices=Value::Null;let mut checked=None;
            loop {
                let mut v=project(&install,detect());v["host"]=json!(crate::hostname());v["platform"]=json!(std::env::consts::OS);
                v["targets"]=json!({"codex":targets("codex").ok(),"claude":targets("claude").ok()});v["runtimes"]=choices.clone();
                *output.lock().unwrap()=v;
                if checked.is_none_or(|t:Instant|t.elapsed()>=Duration::from_secs(300)){choices=runtimes(&install);checked=Some(Instant::now());}
                std::thread::sleep(Duration::from_secs(2));
            }
        });cache
    });
}
pub fn snapshot()->Value {CACHE.get().map(|c|c.lock().unwrap().clone()).unwrap_or(Value::Null)}
/// No installer here: the click's id fences transport retries in the existing setup writer.
pub fn action(install:&Path,action:&str,id:&str,runtime:&str)->Res<()> {
    locked(install,||{
        let mut p=preferences(install)?;
        if action=="agentq-dismiss" {p.dismissed=true;save(install,&p)?;return Ok(());}
        if action!="agentq-setup" || !crate::resources::id(id) || !["codex","claude"].contains(&runtime){return Err("invalid agentq setup request".into());}
        let choices=snapshot();
        if !choices["runtimes"].as_array().is_some_and(|rows|rows.iter().any(|r|r["runtime"]==runtime && r["eligible"]==true)){return Err("Selected setup runtime unavailable; check Agents and its access grant".into());}
        // Save the reference first: a crash before/after submission is unknown, never an automatic second install.
        p.request=Some(id.into());save(install,&p)?;
        crate::setup::agentq(install,id,runtime,targets(runtime)?)?;Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // #1391 A2: absence, stale/corrupt/denied observations and PID reuse cannot clear a warning as success.
    #[test]
    fn detection_requires_fresh_compatible_status_and_native_executable() {
        let root=Path::new("/invented/agentq");let exe=root.join("releases/0.9.0").join(if cfg!(windows){"agentq.exe"}else{"agentq"});
        let good=json!({"project":"host","version":"0.9.0","at":100,"pid":42});
        let check=|launcher,status,process:Res<Option<PathBuf>>|inspect(root,launcher,status,100,|_|process)["kind"].clone();
        assert_eq!(check(Ok(false),Ok(None),Ok(None)),"shown");
        assert_eq!(check(Ok(true),Ok(None),Ok(None)),"unknown");
        assert_eq!(check(Err("denied".into()),Ok(None),Ok(None)),"unknown");
        assert_eq!(check(Ok(false),Err("corrupt".into()),Ok(None)),"unknown");
        assert_eq!(check(Ok(true),Ok(Some((good.clone(),90))),Ok(Some(exe.clone()))),"running");
        assert_eq!(check(Ok(true),Ok(Some((good.clone(),91))),Ok(Some(exe.clone()))),"unknown");
        assert_eq!(check(Ok(true),Ok(Some((good.clone(),0))),Ok(None)),"stopped");
        assert_eq!(check(Ok(true),Ok(Some((good.clone(),0))),Ok(Some("/other/agentq".into()))),"unknown");
        assert_eq!(check(Ok(true),Ok(Some((good.clone(),0))),Err("denied".into())),"unknown");
        assert_eq!(check(Ok(false),Ok(Some((good.clone(),0))),Ok(Some(exe.clone()))),"unknown");
        for bad in [json!({}),json!({"project":"host","version":"1.0.0","at":100,"pid":42}),json!({"project":"host","version":"0.9.0","at":101,"pid":42})]{assert_eq!(check(Ok(true),Ok(Some((bad,0))),Ok(Some(exe.clone()))),"unknown");}
    }
    #[cfg(any(target_os="macos",windows))]
    #[test]
    fn native_process_image_matches_creation_identity() {
        assert_eq!(process(std::process::id()).unwrap().unwrap(),std::env::current_exe().unwrap());
    }
    #[test]
    fn bounded_files_keep_malformed_and_future_observations_unknown() {
        let dir=std::env::temp_dir().join(format!("taskq-agentq-status-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let path=dir.join("status.json");assert!(status(&path).unwrap().is_none());
        fs::write(&path,b"{bad").unwrap();assert!(status(&path).is_err());
        fs::write(&path,vec![b' ';1024*1024+1]).unwrap();assert!(status(&path).unwrap_err().contains("1 MiB"));
        fs::remove_dir_all(dir).unwrap();
    }

    // #1391 A4: preferences survive restart; only verified running resets an episode, setup records never replay.
    #[test]
    fn dismissal_and_setup_recovery_preserve_unknown_effects() {
        let dir=std::env::temp_dir().join(format!("taskq-agentq-{}",crate::runtime::uuid()));fs::create_dir(&dir).unwrap();
        let shown=project(&dir,json!({"kind":"shown"}));assert_eq!(shown["warning"],true);
        action(&dir,"agentq-dismiss","","").unwrap();assert_eq!(project(&dir,json!({"kind":"shown"}))["warning"],false);
        let u=project(&dir,unknown("read denied"));assert_eq!(u["kind"],"unknown");assert_eq!(u["dismissed"],true);
        let mut p=preferences(&dir).unwrap();p.request=Some("missing-request".into());save(&dir,&p).unwrap();
        assert_eq!(project(&dir,json!({"kind":"shown"}))["kind"],"interrupted");assert!(crate::setup::all(&dir).unwrap().0.is_empty());
        assert_eq!(project(&dir,json!({"kind":"running"}))["warning"],false);assert!(!preferences(&dir).unwrap().dismissed);
        assert_eq!(project(&dir,json!({"kind":"shown"}))["warning"],true);
        fs::write(dir.join(FILE),"{bad").unwrap();assert_eq!(project(&dir,json!({"kind":"running"}))["kind"],"unknown");assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(),"{bad");
        fs::remove_dir_all(dir).unwrap();
    }
}
