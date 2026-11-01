//! Host installation recovery in the existing migration originals and registry prepare record.
use super::*;
use serde::{Deserialize,Serialize};

#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Original {path:PathBuf,digest:Option<String>,owner:Value,mode:u32,readonly:bool}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {schema:u8,version:String,roots:Vec<PathBuf>,files:Vec<Original>}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    schema:u8,id:String,baseline:String,root:PathBuf,version:String,candidate:String,
    original:Option<String>,next:String,phase:String,restore:bool,attach:bool,owner:Option<InstallOwner>,command:String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    activation:Option<Baseline>,
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallOwner {pid:u32,started:String,instance:String}
fn digest(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn directory(install:&Path)->PathBuf {install.join("host-policy-migration/install-originals")}
fn manifest(install:&Path)->PathBuf {install.join("host-policy-migration/install.json")}
fn activation_directory(install:&Path)->PathBuf {directory(install).with_file_name("activation-originals")}
fn raw_projects(install:&Path)->Res<Option<String>> {
    match fs::read_to_string(install.join("projects")) {
        Ok(text)=>Ok(Some(text)),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(e)=>Err(e.to_string()),
    }
}
fn pending(install:&Path)->Res<Pending> {
    match fs::symlink_metadata(install.join(format!("{INSTALL_PENDING}.pending"))) {
        Ok(_)=>return Err("installation cursor publication incomplete; recovery evidence kept".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
    }
    read_cursor(install,&install.join(INSTALL_PENDING))
}
/// #1306: the receipt-bound upgrade intent waits here, outside `projects.installing`, which an older host
/// cannot hold transitions on until it re-execs into the candidate.
const UPGRADE_INTENT:&str="projects.upgrading";
fn upgrade_intent(install:&Path)->Res<Option<Pending>> {
    match fs::symlink_metadata(install.join(UPGRADE_INTENT)) {
        Ok(_)=>read_cursor(install,&install.join(UPGRADE_INTENT)).map(Some),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(e)=>Err(e.to_string()),
    }
}
fn read_cursor(install:&Path,path:&Path)->Res<Pending> {
    let meta=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
    if !meta.is_file()||meta.len()>1<<20 {return Err("projects.installing unsupported; recovery evidence kept".into());}
    let p:Pending=serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?).map_err(|_|"projects.installing retained; installation binding unknown")?;
    if p.schema!=1||!crate::resources::id(&p.id)||!p.root.is_absolute()||p.version.is_empty()||p.version.contains("..")||!p.version.bytes().all(|b|b.is_ascii_alphanumeric()||b".-".contains(&b))
        ||!matches!(p.phase.as_str(),"initial"|"prepared"|"activated"|"starting"|"owned"|"registered"|"cancelling")
        ||[&p.baseline,&p.candidate].iter().any(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())) {
        return Err("projects.installing unsupported; installation binding kept".into());
    }
    let bytes=fs::read(manifest(install)).map_err(|e|format!("installation originals unavailable: {e}"))?;
    if digest(&bytes)!=p.baseline {return Err("installation originals changed; recovery held".into());}
    let command=CommandReceipt::decode(&p.command)?;
    let metadata:Value=serde_json::from_str(&command.encode()).map_err(|e|e.to_string())?;
    if metadata["product"]!="taskq"||metadata["operation"]!="taskq.host.install" {return Err("installation command identity invalid; retained".into());}
    let roots=project_entries(&p.next)?;
    if !roots.iter().any(|(_,root)|root==&p.root)||p.owner.as_ref().is_some_and(|o|o.pid==0||o.started.is_empty()||!crate::resources::id(&o.instance)) {return Err("installation scope/owner binding invalid; retained".into());}
    Ok(p)
}
fn save(install:&Path,p:&Pending)->Res<()> {
    coreq::write_durable(&install.join(INSTALL_PENDING),serde_json::to_vec(p).map_err(|e|e.to_string())?)?;
    #[cfg(test)] tests::interrupt(install,&p.phase)?;
    Ok(())
}
fn binding(p:&Pending)->Value {
    let mut bound=json!({"local_install":{"schema":1,"id":p.id,"root":p.root,"version":p.version,"candidate":p.candidate,"command":p.command}});
    if let Some(activation)=&p.activation {bound["local_install"]["activation"]=json!(digest(json!(activation).to_string().as_bytes()));}
    bound
}
fn retained(install:&Path,p:&Pending)->Res<Value> {
    crate::resources::within(install,crate::resources::WAIT,|state| {
        let op=state.ops.get(&p.id).ok_or("original installation receipt missing; transition held")?;
        if op.bound!=binding(p) {return Err("original installation receipt binding changed; transition held".into());}
        Ok(checked_receipt(op)?.clone())
    })
}
fn record(install:&Path,p:&Pending,phase:&str)->Res<()> {
    let command=CommandReceipt::decode(&p.command)?;
    let bound=binding(p);
    crate::resources::within(install,crate::resources::WAIT,|state| {
        let mut receipt=match state.ops.get(&p.id) {
            Some(op) if op.bound==bound=>{
                let receipt=checked_receipt(op)?;
                if receipt["state"]=="observed"&&matches!(phase,"received"|"applied") {return Ok(());}
                if receipt["state"]=="failed"&&phase!="failed" {return Err("original installation already failed; command cannot run again".into());}
                receipt.clone()
            },
            Some(_)=>return Err("installation ID belongs to another command; retained".into()),
            None=>{
                if state.ops.len()>=crate::resources::MAX_OPS {return Err("installation receipt store full; no active effect permitted".into());}
                json!({"id":p.id,"action":"install","root":p.root,"version":p.version,"state":"queued"})
            },
        };
        let event=crate::telemetry::receipt(&command,phase)?;
        receipt.as_object_mut().ok_or("installation receipt invalid")?.remove("reason");
        receipt[format!("{phase}_receipt")]=json!(event);
        receipt["state"]=json!(match phase {"received"=>"queued","applied"=>"applied","observed"=>"observed",_=>"failed"});
        let now=wall()?;
        state.ops.insert(p.id.clone(),crate::resources::Op {bound,receipt,expires_at:now,saved_at:Some(now),writing:false,sent:true});
        Ok(())
    })?;
    #[cfg(test)] tests::interrupt(install,phase)?;
    Ok(())
}
fn retain_reason(install:&Path,p:&Pending,reason:&str)->Res<()> {
    crate::resources::within(install,crate::resources::WAIT,|state| {
        let op=state.ops.get_mut(&p.id).ok_or("installation receipt unavailable")?;
        if op.bound!=binding(p) {return Err("installation reason belongs to another command".into());}
        checked_receipt(op)?;
        op.receipt["reason"]=json!(crate::compact(reason,256));Ok(())
    })
}

fn checked_receipt(op:&crate::resources::Op)->Res<&Value> {
let b=&op.bound["local_install"];
let mut keys=vec!["schema","id","root","version","candidate","command"];
if let Some(activation)=b.get("activation") {
    if activation.as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())) {return Err("installation activation binding corrupt; evidence kept".into());}
    keys.push("activation");
}
if !crate::resources::keys(&op.bound,&["local_install"])||!crate::resources::keys(b,&keys)
    ||b["schema"]!=1||b["id"]!=op.receipt["id"]||op.receipt["action"]!="install"||op.receipt["root"]!=b["root"]||op.receipt["version"]!=b["version"] {
    return Err("installation receipt binding corrupt; evidence kept".into());
}
let command=CommandReceipt::decode(b["command"].as_str().ok_or("installation command missing")?)?;
let metadata:Value=serde_json::from_str(&command.encode()).map_err(|e|e.to_string())?;
if metadata["product"]!="taskq"||metadata["operation"]!="taskq.host.install" {return Err("installation command identity invalid".into());}
let required=match op.receipt["state"].as_str() {Some("queued")=>vec!["received"],Some("applied")=>vec!["received","applied"],Some("observed")=>vec!["received","applied","observed"],Some("failed")=>vec!["received","failed"],_=>return Err("installation receipt phase unsupported".into())};
for phase in required {if op.receipt.get(format!("{phase}_receipt")).is_none() {return Err("installation phase proof missing".into());}}
for phase in ["received","applied","observed","failed"] {
    if let Some(value)=op.receipt.get(format!("{phase}_receipt")) {if *value!=command.event_id(phase)? {return Err("installation phase identity differs".into());}}
}
if let Some(cursor)=op.receipt.get("cancelled_attachment") {
    let p:Pending=serde_json::from_value(cursor.clone()).map_err(|_|"cancelled attachment evidence corrupt")?;
    if op.receipt["state"]!="failed"||p.phase!="cancelling"||!p.attach||p.restore||p.original.is_none()||binding(&p)!=op.bound {
        return Err("cancelled attachment binding differs; evidence kept".into());
    }
}
    Ok(&op.receipt)
}

pub fn installation_status(install:&Path,id:Option<&str>)->Value {
    crate::resources::within(install,crate::resources::WAIT,|state| {
        let op=if let Some(id)=id {state.ops.get(id).filter(|op|op.bound.get("local_install").is_some())}
            else {
                let latest=state.ops.values().filter(|op|op.bound.get("local_install").is_some()).max_by_key(|op|op.saved_at);
                if latest.is_some_and(|latest|state.ops.values().filter(|op|op.bound.get("local_install").is_some()&&op.saved_at==latest.saved_at).count()>1) {
                    return Err("installation observation time is ambiguous; use service status --request ID".into());
                }
                latest
            };
        let Some(op)=op else {return Ok(json!({"state":"unknown","reason":"no retained installation receipt"}));};
        Ok(checked_receipt(op).cloned().unwrap_or_else(|reason|json!({"id":op.bound["local_install"]["id"],"action":"install","state":"unknown","reason":reason})))
    }).unwrap_or_else(|reason|json!({"state":"unknown","reason":reason}))
}

pub(super) fn registered_projects(install:&Path)->Res<Option<String>> {
    let p=pending(install)?;
    if p.phase=="cancelling" {
        cancellation_registry(install,&p)?;
        return Ok(p.original);
    }
    if p.phase!="registered"||raw_projects(install)?.as_deref()!=Some(p.next.as_str()) {
        return Err("projects.installing retained; project membership unknown".into());
    }
    Ok(Some(p.next))
}
/// A prior status for the same PID/root cannot acknowledge another registration.
pub fn install_acknowledgement(install:&Path,root:&Path,instance:&str)->Option<String> {
    let p=pending(install).ok()?;
    (p.phase=="registered"&&p.root==root&&p.owner.as_ref().is_some_and(|owner|owner.instance==instance)
        &&raw_projects(install).ok().flatten().as_deref()==Some(p.next.as_str())).then_some(p.id)
}
pub(super) fn admission_reason(install:&Path)->Option<String> {
    match fs::symlink_metadata(install.join(INSTALL_PENDING)) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,
        Err(e)=>Some(format!("installation prepare unknown: {e}")),
        Ok(_)=>Some("installation acknowledgement pending; owned work remains supervised".into()),
    }
}

fn snapshot(install:&Path,roots:Vec<PathBuf>,paths:Vec<PathBuf>)->Res<String> {
    let path=manifest(install);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file()=>return fs::read(path).map(|b|digest(&b)).map_err(|e|e.to_string()),
        Ok(_)=>return Err("installation originals are not a regular file; kept".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
    }
    let baseline=capture_originals(&directory(install),roots,paths)?;
    let bytes=serde_json::to_vec(&baseline).map_err(|e|e.to_string())?;
    coreq::write_durable(&path,&bytes)?;
    #[cfg(test)] tests::interrupt(install,"snapshot")?;
    Ok(digest(&bytes))
}

fn capture_originals(folder:&Path,roots:Vec<PathBuf>,paths:Vec<PathBuf>)->Res<Baseline> {
    // No active effect precedes the manifest. Rebuild only this installer's fixed, uncommitted copy slots.
    match fs::symlink_metadata(&folder) {
        Ok(meta) if meta.is_dir()=>{
            ownership(&folder)?;
            for entry in fs::read_dir(&folder).map_err(|e|e.to_string())? {
                let entry=entry.map_err(|e|e.to_string())?;
                if entry.file_name().to_str().and_then(|n|n.parse::<usize>().ok()).is_none_or(|n|n>=paths.len())
                    ||!entry.file_type().map_err(|e|e.to_string())?.is_file() {return Err("unknown installation snapshot contents; originals kept".into());}
            }
        },
        Ok(_)=>return Err("installation snapshot directory is not owned regular storage; kept".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
    }
    fs::create_dir_all(&folder).map_err(|e|e.to_string())?;
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&folder,fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;}
    let mut files=vec![];
    for (i,path) in paths.into_iter().enumerate() {
        let meta=match fs::symlink_metadata(&path) {
            Ok(m) if m.is_file()=>Some(m),Ok(_)=>return Err(format!("installation input {} is not a regular file; kept",path.display())),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,Err(e)=>return Err(e.to_string()),
        };
        let (hash,owner,mode,readonly)=if let Some(meta)=meta {
            let owner=ownership(&path)?;
            let bytes=fs::read(&path).map_err(|e|e.to_string())?;
            let dst=folder.join(i.to_string());
            if fs::read(&dst).ok().as_deref()!=Some(bytes.as_slice()) {coreq::write_durable(&dst,&bytes)?;}
            fs::set_permissions(&dst,meta.permissions()).map_err(|e|e.to_string())?;
            #[cfg(not(unix))] let mode=0;
            #[cfg(unix)] let mode={use std::os::unix::fs::PermissionsExt;meta.permissions().mode()};
            if fs::read(&dst).map_err(|e|e.to_string())?!=bytes {return Err("installation snapshot readback differs".into());}
            (Some(digest(&bytes)),owner,mode,meta.permissions().readonly())
        } else {(None,Value::Null,0,false)};
        files.push(Original {path,digest:hash,owner,mode,readonly});
    }
    for saved in &files {
        let current=match fs::read(&saved.path) {Ok(b)=>Some(digest(&b)),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,Err(e)=>return Err(e.to_string())};
        if current!=saved.digest||saved.digest.is_some()&&ownership(&saved.path)?!=saved.owner {return Err("installation input changed during snapshot; active files unchanged".into());}
    }
    Ok(Baseline {schema:1,version:crate::platform::VERSION.into(),roots,files})
}

fn verify_originals(install:&Path)->Res<()> {
    let path=manifest(install);let meta=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
    if !meta.is_file()||meta.len()>1<<20 {return Err("installation originals manifest unsupported; kept".into());}
    let baseline:Baseline=serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if baseline.schema!=1 {return Err("installation originals schema unsupported; kept".into());}
    verify_copies(&directory(install),&baseline)
}

fn verify_copies(folder:&Path,baseline:&Baseline)->Res<()> {
    if baseline.schema!=1 {return Err("installation originals schema unsupported; kept".into());}
    if !fs::symlink_metadata(&folder).map_err(|e|e.to_string())?.is_dir() {return Err("installation original storage is not a directory".into());}
    for (i,saved) in baseline.files.iter().enumerate() {
        if let Some(hash)=&saved.digest {
            let path=folder.join(i.to_string());
            if !fs::symlink_metadata(&path).map_err(|e|e.to_string())?.is_file()||digest(&fs::read(path).map_err(|e|e.to_string())?)!=*hash {
                return Err("installation original checksum differs; transition held before active effects".into());
            }
        }
    }
    Ok(())
}

/// Restore only pre-native activation effects. Native submission uncertainty never enters this route.
fn restore_activation(install:&Path,p:&Pending)->Res<()> {
    let initial:Baseline;
    let (baseline,folder)=if let Some(activation)=&p.activation {(activation,activation_directory(install))}else {
        initial=serde_json::from_slice(&fs::read(manifest(install)).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        (&initial,directory(install))
    };
    if !p.restore||baseline.schema!=1||baseline.version!=p.version||!matches!(p.phase.as_str(),"prepared"|"activated") {
        return Err("native installation may own work; automatic topology restore refused".into());
    }
    if native_owner(install)?.is_some()||!crate::host_control::native_absent(&crate::platform::tool(install))? {return Err("native owner/start cannot be excluded; originals retained".into());}
    let _owner=coreq::file_lock::try_lock(&install.join("service.lock")).map_err(|e|format!("native ownership unknown during restore: {e}"))?;
    verify_copies(&folder,baseline)?;
    guard_restore(install,p,&folder,baseline)?;
    // A caller lost during restore must resume the failure, never activate again.
    record(install,p,"failed")?;
    restore_files(install,&folder,baseline)?;
    fs::remove_file(install.join(INSTALL_PENDING)).map_err(|e|e.to_string())
}

fn activation_paths(install:&Path)->Res<Vec<PathBuf>> {
    let paths=vec![install.join(crate::platform::tool(install).exe_name()),install.join("current"),install.join("previous"),crate::host_control::definition(&crate::platform::tool(install))?];
    #[cfg(all(windows,target_env="gnu"))] let paths={let mut paths=paths;paths.push(install.join("WebView2Loader.dll"));paths};
    Ok(paths)
}

// Validate the whole write set before restoring any file, including an otherwise untouched native definition.
fn guard_restore(install:&Path,p:&Pending,folder:&Path,baseline:&Baseline)->Res<()> {
    let tool=crate::platform::tool(install);
    // The cursor uses the same create/rename writer and install directory as activation's fresh files.
    let created=fs::metadata(install.join(INSTALL_PENDING)).map_err(|e|e.to_string())?;
    let created_owner=ownership(&install.join(INSTALL_PENDING))?;
    let previous=baseline.files.iter().enumerate().find(|(_,saved)|saved.path==install.join("current"))
        .filter(|(_,saved)|saved.digest.is_some()).map(|(i,_)|fs::read_to_string(folder.join(i.to_string())).map_err(|e|e.to_string())).transpose()?;
    for path in activation_paths(install)? {
        let saved=baseline.files.iter().find(|saved|saved.path==path).ok_or_else(||format!("installation original missing: {}",path.display()))?;
        let metadata=match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file()=>Some(meta),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,
            _=>return Err(format!("installation input changed before restore: {}; originals retained",path.display())),
        };
        let actual=metadata.as_ref().map(|_|fs::read(&path).map(|b|digest(&b)).map_err(|e|e.to_string())).transpose()?;
        let expected=if path==install.join(tool.exe_name()) {Some(p.candidate.clone())}
            else if path==install.join("current") {previous.as_deref().is_none_or(|v|v.trim()!=p.version).then(||digest(p.version.as_bytes()))}
            else if path==install.join("previous") {previous.as_deref().filter(|v|v.trim()!=p.version).map(|v|digest(v.trim().as_bytes()))}
            else {None};
        #[cfg(all(windows,target_env="gnu"))] let expected=if path==install.join("WebView2Loader.dll") {
            Some(digest(&fs::read(tool.release_exe(&p.version).with_file_name("WebView2Loader.dll")).map_err(|e|e.to_string())?))
        } else {expected};
        if actual!=saved.digest&&(expected.is_none()||actual!=expected) {
            return Err(format!("installation input changed before restore: {}; originals retained",path.display()));
        }
        if let Some(meta)=metadata {
            let owner=ownership(&path)?;
            let original=actual==saved.digest&&owner==saved.owner&&meta.permissions().readonly()==saved.readonly;
            let candidate=actual==expected&&owner==created_owner;
            #[cfg(unix)] let (original,candidate)={
                use std::os::unix::fs::PermissionsExt;
                let mode=if path==install.join(tool.exe_name()) {
                    if saved.digest.is_some() {saved.mode}else{fs::metadata(tool.release_exe(&p.version)).map_err(|e|e.to_string())?.permissions().mode()}
                }else{created.permissions().mode()};
                (original&&meta.permissions().mode()==saved.mode,candidate&&meta.permissions().mode()==mode)
            };
            #[cfg(windows)] let candidate=candidate&&meta.permissions().readonly()==if path==install.join(tool.exe_name()) {
                fs::metadata(tool.release_exe(&p.version)).map_err(|e|e.to_string())?.permissions().readonly()
            }else{created.permissions().readonly()};
            if !original&&!candidate {return Err("installation ownership/permissions changed before restore; originals retained".into());}
        }
    }
    Ok(())
}

fn original_file_matches(saved:&Original,bytes:&[u8])->Res<bool> {
    let metadata=match fs::symlink_metadata(&saved.path) {
        Ok(meta) if meta.is_file()=>meta,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(false),
        Ok(_)=>return Err("installation restore target is not a regular file; originals retained".into()),
        Err(e)=>return Err(format!("installation restore metadata {}: {e}",saved.path.display())),
    };
    let permissions=metadata.permissions();
    #[cfg(unix)] let same_permissions={use std::os::unix::fs::PermissionsExt;permissions.mode()==saved.mode};
    #[cfg(windows)] let same_permissions=permissions.readonly()==saved.readonly;
    Ok(same_permissions&&fs::read(&saved.path).map_err(|e|format!("installation restore readback {}: {e}",saved.path.display()))?==bytes&&ownership(&saved.path)?==saved.owner)
}

fn restore_files(install:&Path,folder:&Path,baseline:&Baseline)->Res<()> {
    let mut originals=vec![];
    let names=vec![crate::platform::tool(install).exe_name(),"current".into(),"previous".into()];
    #[cfg(all(windows,target_env="gnu"))] let names={let mut names=names;names.push("WebView2Loader.dll".into());names};
    let mut paths:Vec<_>=names.into_iter().map(|name|install.join(name)).collect();
    let definition=crate::host_control::definition(&crate::platform::tool(install))?;
    if baseline.files.iter().any(|f|f.path==definition) {paths.push(definition);}
    for path in paths {
        let (i,saved)=baseline.files.iter().enumerate().find(|(_,f)|f.path==path).ok_or_else(||format!("installation original missing: {}",path.display()))?;
        if let Some(hash)=&saved.digest {
            let bytes=fs::read(folder.join(i.to_string())).map_err(|e|e.to_string())?;
            if digest(&bytes)!=*hash {return Err("installation original checksum differs; restore held".into());}
            originals.push((saved,Some(bytes)));
        } else {originals.push((saved,None));}
    }
    for (saved,bytes) in originals {
        let path=&saved.path;
        if let Some(bytes)=bytes {
            if original_file_matches(saved,&bytes)? {continue;}
            coreq::write_durable(path,&bytes).map_err(|e|format!("installation restore write {}: {e}",path.display()))?;
            // Windows needs a writable handle for flush, retained before restoring readonly/DACL.
            let file=fs::OpenOptions::new().write(true).open(path).map_err(|e|format!("installation restore sync handle {}: {e}",path.display()))?;
            #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&path,fs::Permissions::from_mode(saved.mode)).map_err(|e|e.to_string())?;}
            #[cfg(windows)] {let mut mode=fs::metadata(&path).map_err(|e|e.to_string())?.permissions();mode.set_readonly(saved.readonly);fs::set_permissions(&path,mode).map_err(|e|e.to_string())?;}
            stage_owner(&path,&saved.owner)?;
            file.sync_all().map_err(|e|format!("installation restore sync {}: {e}",path.display()))?;
            drop(file);
            if !original_file_matches(saved,&bytes)? {return Err(format!("installation restore readback differs: {}",path.display()));}
        } else {
            match fs::remove_file(&path) {Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string())}
        }
        #[cfg(test)] tests::interrupt(install,"restored")?;
    }
    Ok(())
}

fn native_owner(install:&Path)->Res<Option<coreq::process::Identity>> {
    match service_record(install)? {
        None=>Ok(None),Some(OwnerRecord::Legacy(_))=>Err("legacy host owner requires migration; installation refused".into()),
        Some(OwnerRecord::Current(r))=>{
            let expected=coreq::process::Identity {pid:r.pid,started:r.started};
            Ok(coreq::process::identity(r.pid).map_err(|e|e.to_string())?.filter(|native|native==&expected))
        },
    }
}

fn context(p:&Pending)->Res<String> {
    let command:Value=serde_json::from_str(&p.command).map_err(|e|e.to_string())?;
    coreq::telemetry::capture::TraceContext::decode(&format!("1 {} {}",command["trace"].as_str().ok_or("installation trace missing")?,command["span"].as_str().ok_or("installation span missing")?)).map(|context|context.encode())
}

pub(super) fn recover(install:&Path)->Res<String> {
    let p=pending(install)?;
    let context=coreq::telemetry::capture::TraceContext::decode(&context(&p)?)?;
    let _context=context.span().entered();
    if p.attach&&!matches!(p.phase.as_str(),"registered"|"cancelling") {preflight(install,&p.root)?;}
    install_bound(install,&p.root,Some(&p.id))
}

fn acknowledge(install:&Path,expected:&Pending)->Res<bool> {
    let _update=match coreq::file_lock::try_lock(&install.join("update.lock")) {
        Ok(lock)=>lock,Err(std::fs::TryLockError::WouldBlock)=>return Ok(false),Err(e)=>return Err(e.to_string()),
    };
    let _projects=match coreq::file_lock::try_lock(&install.join("projects.lock")) {
        Ok(lock)=>lock,Err(std::fs::TryLockError::WouldBlock)=>return Ok(false),Err(e)=>return Err(e.to_string()),
    };
    match fs::symlink_metadata(install.join(INSTALL_PENDING)) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return if retained(install,expected)?["state"]=="observed" {Ok(true)}else{Err("installation cursor absent without its observed receipt".into())},
        Err(e)=>return Err(e.to_string()),Ok(_)=>{},
    }
    let p=pending(install)?;
    if p.id!=expected.id||p.command!=expected.command||p.root!=expected.root||p.version!=expected.version||p.candidate!=expected.candidate||p.next!=expected.next {
        return Err("installation binding changed before acknowledgement; retained".into());
    }
    if p.phase!="registered"||raw_projects(install)?.as_deref()!=Some(p.next.as_str()) {return Ok(false);}
    let owner=p.owner.as_ref().ok_or("installation native owner missing")?;
    let service=service_status(install);
    let status=crate::data::json(&crate::dot(&p.root).join("status.json"))?;
    if !status.as_ref().is_some_and(|status|status["host_install"]==p.id)||!installation_observed(&service,status.as_ref(),&p.version)
        ||service["pid"]!=owner.pid||service["started"]!=owner.started||service["instance"]!=owner.instance {return Ok(false);}
    record(install,&p,"observed")?;
    fs::remove_file(install.join(INSTALL_PENDING)).map_err(|e|e.to_string())?;
    Ok(true)
}

#[tracing::instrument(name="host.install",skip_all)]
pub fn install_project(install:&Path,root:&Path)->Res<String> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot install the host service".into());}
    preflight(install,root)?;
    upgrade(install,root)?;
    install_bound(install,root,None)
}

fn preflight(install:&Path,root:&Path)->Res<()> {
    drop(crate::wake_lock(root)?);
    let (cfg,raw,_)=crate::check_load(root)?;
    let board=cfg.board.open(&cfg.web_host,&cfg.repo,raw["label_schema"].as_str(),&crate::platform::tool(install))?;
    board.me()?;
    board.issues(None)?;
    Ok(())
}

// A3: cancellation can undo only the new membership of this exact live owner, never its execution.
fn cancellation_registry(install:&Path,p:&Pending)->Res<()> {
    if !p.attach||p.restore||p.activation.is_some()||!matches!(p.phase.as_str(),"owned"|"registered"|"cancelling") {
        return Err("only an owned or registered unused attachment can be cancelled; installation kept".into());
    }
    let original=p.original.as_deref().ok_or("attachment original registry missing; kept")?;
    if project_entries(original)?.iter().any(|(_,root)|root==&p.root) {return Err("attachment root was already registered; cancellation refused".into());}
    let mut next=original.to_string();if !next.is_empty()&&!next.ends_with('\n') {next.push('\n');}
    next.push_str(crate::platform::plain_path(&p.root).to_str().ok_or("attachment root is not UTF-8")?);next.push('\n');
    // Older cursors may store the canonical Windows prefix; compare identities without changing original bytes.
    if !project_entries(&next)?.into_iter().map(|(_,root)|root).eq(project_entries(&p.next)?.into_iter().map(|(_,root)|root)) {
        return Err("attachment changed more than its own membership; kept".into());
    }
    let current=raw_projects(install)?;
    let matches=match p.phase.as_str() {
        "owned"=>current==p.original,"registered"=>current.as_deref()==Some(p.next.as_str()),
        _=>current==p.original||current.as_deref()==Some(p.next.as_str()),
    };
    if !matches {return Err("attachment registry changed; cancellation refused".into());}
    Ok(())
}

fn cancellation_guard(install:&Path,p:&Pending)->Res<()> {
    cancellation_registry(install,p)?;
    verify_originals(install)?;
    let owner=p.owner.as_ref().ok_or("attachment owner missing; kept")?;
    let service=service_status(install);
    let expected=coreq::process::Identity {pid:owner.pid,started:owner.started.clone()};
    if native_owner(install)?.as_ref()!=Some(&expected)||service["fresh"]!=true||service["instance"]!=owner.instance {
        return Err("attachment ownership uncertain; cancellation refused".into());
    }
    match coreq::file_lock::try_lock(&install.join("service.lock")) {
        Err(std::fs::TryLockError::WouldBlock)=>{},_=>return Err("attachment host ownership lock unobserved; kept".into()),
    }
    empty_attachment_execution(install,&p.root,&expected)?;
    let receipt=retained(install,p)?;
    if receipt["state"]=="observed" {return Err("attachment already observed; use project Uninstall".into());}
    Ok(())
}

fn empty_attachment_execution(install:&Path,root:&Path,owner:&coreq::process::Identity)->Res<()> {
    match fs::symlink_metadata(crate::dot(root).join("kids")) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err("legacy project execution uncertain; attachment kept".into()),
    }
    let path=crate::kids_path(install,root)?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        Ok(meta) if meta.is_file()&&meta.len()<=crate::data::KIDS_MAX=>{
            let (recorded,kids)=crate::data::kids(&fs::read_to_string(&path).map_err(|e|e.to_string())?)?;
            if &recorded!=owner||!kids.is_empty() {return Err("project execution or recovery ownership uncertain; attachment kept".into());}
        },
        _=>return Err("project execution evidence unreadable; attachment kept".into()),
    }
    if let Some(status)=crate::data::json(&crate::dot(root).join("status.json"))? {
        if status["pid"]!=owner.pid||status["host_instance"]!=service_status(install)["instance"] {
            return Err("project ownership uncertain; attachment kept".into());
        }
    }
    Ok(())
}

pub fn cancel_install(install:&Path,root:&Path,id:&str)->Res<String> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot cancel host installation".into());}
    if !crate::resources::id(id) {return Err("invalid installation ID".into());}
    let _update=crate::platform::update_lock_within(install,crate::resources::WAIT)?;
    let _projects=coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|format!("project registry busy: {e}"))?;
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let receipt=installation_status(install,Some(id));
    if receipt["state"]=="failed"&&receipt["root"]==json!(root)&&receipt.get("cancelled_attachment").is_some()
        &&!install.join(INSTALL_PENDING).try_exists().map_err(|e|e.to_string())? {
        return Ok(format!("Install receipt {id} cancelled; original registry restored"));
    }
    let mut p=pending(install)?;
    if p.id!=id||p.root!=root {return Err("installation ID/root differs; cancellation refused".into());}
    cancellation_guard(install,&p)?;
    if service_status(install)["install_cancel_protocol"]!=1 {return Err("running host cannot reconcile attachment cancellation; installation kept".into());}
    p.phase="cancelling".into();save(install,&p)?;
    retain_reason(install,&p,"attachment cancellation pending; host must join its unused project thread")?;
    coreq::write_durable(&install.join("poke"),"")?;
    Ok(format!("Install receipt {id} cancellation requested; check service status --request {id}"))
}

pub(super) fn cancelling_root(install:&Path)->Option<PathBuf> {
    pending(install).ok().filter(|p|p.phase=="cancelling").map(|p|p.root)
}

pub(super) fn finish_cancellation(install:&Path,h:&Host,served:bool)->Res<()> {
    let _update=coreq::file_lock::try_lock(&install.join("update.lock")).map_err(|e|e.to_string())?;
    let _projects=coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|e.to_string())?;
    let p=pending(install)?;
    if p.phase!="cancelling"||p.owner.as_ref().is_none_or(|o|o.pid!=std::process::id()||o.instance!=h.instance) {
        return Err("attachment cancellation belongs to another host; kept".into());
    }
    if served||h.busy.contains_key(&p.root)||h.slots.keys().any(|(root,_)|root==&p.root) {
        return Err("attachment project drain/execution unconfirmed; kept".into());
    }
    cancellation_guard(install,&p)?;
    let original=p.original.as_deref().ok_or("attachment original registry missing")?;
    if raw_projects(install)?.as_deref()!=Some(original) {replace_owned(&install.join("projects"),&p.next,original)?;}
    if raw_projects(install)?.as_deref()!=Some(original) {return Err("attachment cancellation registry readback differs; kept".into());}
    let context=coreq::telemetry::capture::TraceContext::decode(&context(&p)?)?;
    let _context=context.span().entered();
    record(install,&p,"failed")?;
    crate::resources::within(install,crate::resources::WAIT,|state| {
        let op=state.ops.get_mut(&p.id).ok_or("installation receipt missing")?;
        if op.bound!=binding(&p) {return Err("installation cancellation binding changed".into());}
        checked_receipt(op)?;
        op.receipt["cancelled_attachment"]=json!(p);
        op.receipt["reason"]=json!("attachment cancelled; exact original registry restored; project thread joined without execution");
        Ok(())
    })?;
    fs::remove_file(install.join(INSTALL_PENDING)).map_err(|e|e.to_string())?;
    coreq::write_durable(&install.join("poke"),"")
}

fn receive_initial(install:&Path,p:&mut Pending)->Res<()> {
    let saved=pending(install)?;
    if saved.phase!="initial"||saved.id!=p.id||saved.command!=p.command||binding(&saved)!=binding(p) {
        return Err("original initial installation intent not committed; held".into());
    }
    let exists=crate::resources::within(install,crate::resources::WAIT,|state|Ok(state.ops.contains_key(&p.id)))?;
    if !exists {record(install,p,"received")?;}
    if retained(install,p)?["state"]!="queued" {return Err("initial installation intent has an incompatible receipt; held".into());}
    p.phase=if p.attach {"owned"}else{"prepared"}.into();save(install,p)
}

/// #1306, § 8 step 3 under a running older host: the receipt comes first, then the candidate becomes `current`
/// (previous and rollback floor at the running release) and the host's own update step re-execs into it once live
/// work hands over. No `projects.installing` exists meanwhile, so a host predating this step keeps its transitions;
/// the attach that follows adopts this command. A caller that stops waiting repeats the install to continue it.
fn upgrade(install:&Path,root:&Path)->Res<bool> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot install the host service".into());}
    let tool=crate::platform::tool(install);
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let version={
        let _update=crate::platform::update_lock_within(install,crate::resources::WAIT)?;
        match fs::symlink_metadata(install.join(INSTALL_PENDING)) {
            Ok(_)=>return Ok(false),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
        }
        let Some(owner)=native_owner(install)? else {return Ok(false)};
        let service=service_status(install);
        let Some(running)=service["version"].as_str() else {return Ok(false)};
        if !coreq::update::newer(tool.version,running)&&upgrade_intent(install)?.is_none() {return Ok(false);}
        crate::host_control::install_state(&tool,Some(&owner))?;
        match request_upgrade(install,&tool,&root,&service,&owner)? {Some(v)=>v,None=>return Ok(true)}
    };
    let until=Instant::now()+std::time::Duration::from_secs(300);
    loop {
        let service=service_status(install);
        if service["fresh"]==true&&service["version"]==version {return Ok(true);}
        if Instant::now()>=until {
            // #1601: name the waits the host retained on the receipt, not a generic promise
            let reason=upgrade_intent(install).ok().flatten().and_then(|p|installation_status(install,Some(&p.id))["reason"].as_str().map(String::from))
                .unwrap_or_else(||"handover waits unknown".into());
            return Err(format!("current is {version}; host still runs {} ({reason}); repeat service install for this root",service["version"].as_str().unwrap_or("unknown")));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Under the update lock: the version the running `owner` must re-exec into, or None once it runs the candidate.
fn request_upgrade(install:&Path,tool:&coreq::Tool,root:&Path,service:&Value,owner:&coreq::process::Identity)->Res<Option<String>> {
    let intent=match upgrade_intent(install)?.filter(|p|installation_status(install,Some(&p.id))["state"]!="observed") {
        Some(p) if coreq::update::newer(tool.version,&p.version)=>{supersede_upgrade(install,&p,tool.version)?;None},
        intent=>intent,
    };
    let p=if let Some(p)=intent {
        if p.root!=root {return Err(format!("an upgrade to {} is pending for another root; repeat service install there",p.version));}
        p
    } else {
        if !service["version"].as_str().is_some_and(|running|coreq::update::newer(tool.version,running)) {return Ok(None);}
        if service["install_protocol"]!=1||service["fresh"]!=true {return Err("running host has no compatible installation protocol; update/reconcile it before installing a newer build".into());}
        let edit=ProjectEdit::new(install,root,true)?;
        let baseline=fs::read(manifest(install)).map_err(|e|format!("host installation originals missing; upgrade refused: {e}"))?;
        let p=Pending {schema:1,id:crate::runtime::uuid(),baseline:digest(&baseline),root:root.into(),version:tool.version.into(),
            candidate:crate::platform::prepare_install(tool)?,original:edit.original.clone(),next:edit.next.clone(),phase:"initial".into(),restore:false,attach:true,
            owner:Some(InstallOwner {pid:owner.pid,started:owner.started.clone(),instance:service["instance"].as_str().ok_or("host generation missing")?.into()}),
            command:CommandReceipt::current()?.encode(),activation:None};
        coreq::write_durable(&install.join(UPGRADE_INTENT),serde_json::to_vec(&p).map_err(|e|e.to_string())?)?;
        p
    };
    if service["fresh"]==true&&service["version"]==p.version {return Ok(None);}
    let received=!crate::resources::within(install,crate::resources::WAIT,|state|Ok(state.ops.contains_key(&p.id)))?;
    if received {record(install,&p,"received")?;}
    if retained(install,&p)?["state"]!="queued" {return Err("upgrade intent has an incompatible receipt; held".into());}
    if tool.version!=p.version {return Err(format!("an upgrade to {} is pending; repeat service install with that build",p.version));}
    if tool.current().as_deref()!=Some(tool.version) {crate::platform::activate_install(tool)?;}
    if !install.join("rollback-floor").exists() {coreq::launcher::set_rollback_floor(tool,service["version"].as_str().ok_or("running host version unknown")?)?;}
    coreq::write_durable(&install.join("update"),"")?;
    // a replay keeps the waits the host retained (#1601)
    if received {retain_reason(install,&p,"host update step pending; the host names its re-exec waits here")?;}
    Ok(Some(p.version))
}

/// #1357: an intent whose candidate already runs waits only for its repeated install. The host reports it in the
/// log and the receipt instead of keeping it silently; the repeated install still continues the same command.
/// #1402: the receipt drops the pre-re-exec handover reason at once; None until the reason is retained.
pub fn stale_upgrade(install:&Path,late:bool)->Option<String> {
    let p=upgrade_intent(install).ok()??;
    if p.version!=crate::platform::VERSION||installation_status(install,Some(&p.id))["state"]!="queued" {return None;}
    let reason=if late {format!("upgrade {} already runs {}; repeat service install in {} to finish it",p.id,p.version,p.root.display())}
        else {format!("upgrade {} runs {}; the waiting service install in {} finishes it",p.id,p.version,p.root.display())};
    retain_reason(install,&p,&reason).ok()?;
    Some(reason)
}

/// #1601: while the host's re-exec into a queued upgrade waits, the receipt names the real waits instead of a promise.
/// True once retained, or when no queued upgrade waits for this image to re-exec.
pub fn upgrade_waits(install:&Path,running:&str,waits:&str)->bool {
    let Ok(Some(p))=upgrade_intent(install) else {return true};
    if p.version==running||installation_status(install,Some(&p.id))["state"]!="queued" {return true;}
    retain_reason(install,&p,&format!("host re-exec into {} waits for {waits}",p.version)).is_ok()
}

/// #1601: a newer build supersedes a queued upgrade it cannot finish (the version ran, or never will); the old
/// receipt fails with the reason and the intent goes, so dev installs never wait on an older build by hand.
fn supersede_upgrade(install:&Path,p:&Pending,version:&str)->Res<()> {
    record(install,p,"failed")?;
    retain_reason(install,p,&format!("superseded by {version}"))?;
    fs::remove_file(install.join(UPGRADE_INTENT)).map_err(|e|e.to_string())
}

/// The attach under a host that already runs an upgrade's candidate continues that command and its receipt.
fn adopt_upgrade(install:&Path,root:&Path,running:&str)->Res<Option<Pending>> {
    Ok(upgrade_intent(install)?.filter(|p|p.root==root&&p.version==running&&installation_status(install,Some(&p.id))["state"]=="queued"))
}

pub(super) fn install_bound(install:&Path,root:&Path,expected:Option<&str>)->Res<String> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot install the host service".into());}
    // the installer waits for the re-exec'd host's update steps; host recovery (`expected`) must not block its loop
    let _update=crate::platform::update_lock_within(install,if expected.is_none() {crate::resources::WAIT} else {std::time::Duration::ZERO})?;
    let root=root.canonicalize().map_err(|e|e.to_string())?;
    let tool=crate::platform::tool(install);
    let replay=match fs::symlink_metadata(install.join(INSTALL_PENDING)) {
        Ok(_)=>Some(pending(install)?),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,Err(e)=>return Err(e.to_string()),
    };
    if let Some(id)=expected {
        match &replay {
            Some(p) if p.id==id=>{},Some(_)=>return Err("another installation owns the transition; original recovery cannot start a command".into()),
            None=>return if installation_status(install,Some(id))["state"]=="observed" {Ok(format!("Install receipt {id} already observed"))}else{Err("original installation cursor absent; recovery cannot start a command".into())},
        }
    }
    if replay.as_ref().is_some_and(|p|p.phase=="cancelling") {return Err("attachment cancellation pending; host project drain required".into());}
    let (edit,mut p)=if let Some(p)=replay {
        if !p.attach&&p.version!=tool.version {return Err("installation candidate version differs; use the original installer to reconcile".into());}
        if p.root!=root {return Err("another installation owns projects.installing; reconcile its exact root".into());}
        let lock=coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|format!("project registry busy: {e}"))?;
        let current=raw_projects(install)?;
        if current!=p.original&&current.as_deref()!=Some(p.next.as_str()) {return Err("installation registry changed; originals retained".into());}
        let edit=ProjectEdit {install:install.into(),original:current,next:p.next.clone(),removed:vec![root.clone()],adding:true,_lock:lock};
        (edit,p)
    } else {
        let edit=ProjectEdit::new(install,&root,true)?;
        let owner=native_owner(install)?;
        let mut roots:Vec<_>=project_entries(&edit.next)?.into_iter().map(|(_,root)|root).collect();roots.sort();roots.dedup();
        for project in &roots {
            if project_service(project)?.is_some_and(|pid|owner.as_ref().is_none_or(|owner|owner.pid!=pid)) {return Err("another live service owns a registered root; stop it before host installation".into());}
        }
        crate::host_control::install_state(&tool,owner.as_ref())?;
        let command=CommandReceipt::current()?.encode();
        let p=if let Some(owner)=owner {
            let service=service_status(install);
            if service["install_protocol"]!=1||service["fresh"]!=true {return Err("running host has no compatible installation protocol; update/reconcile it before attaching a root".into());}
            let running=service["version"].as_str().ok_or("running host version unknown")?;
            let baseline=fs::read(manifest(install)).map_err(|e|format!("host installation originals missing; attach refused: {e}"))?;
            let intent=adopt_upgrade(install,&root,running)?;
            let (id,command,candidate)=match intent {Some(p)=>(p.id,p.command,p.candidate),None=>(crate::runtime::uuid(),command,digest(&fs::read(tool.release_exe(running)).map_err(|e|e.to_string())?))};
            Pending {schema:1,id,baseline:digest(&baseline),root:root.clone(),version:running.into(),
                candidate,original:edit.original.clone(),next:edit.next.clone(),
                phase:"owned".into(),restore:false,attach:true,owner:Some(InstallOwner {pid:owner.pid,started:owner.started,instance:service["instance"].as_str().ok_or("host generation missing")?.into()}),command,activation:None}
        } else {
        let mut paths=vec![install.join(tool.exe_name()),install.join("current"),install.join("previous"),crate::host_control::definition(&tool)?,install.join("projects"),install.join("rollback-floor")];
        #[cfg(all(windows,target_env="gnu"))] paths.push(install.join("WebView2Loader.dll"));
        paths.extend(roots.iter().map(|r|crate::dot(r).join("focus")));
        for name in ["current","previous"] {
            match fs::read_to_string(install.join(name)) {
                Ok(v)=>{let v=v.trim();if v.is_empty()||v.contains("..")||!v.bytes().all(|b|b.is_ascii_alphanumeric()||b".-".contains(&b)) {return Err("installation release selection unknown; kept".into());}let path=tool.release_exe(v);if !fs::symlink_metadata(&path).map_err(|e|e.to_string())?.is_file() {return Err("installation retained release unavailable".into());}if !paths.contains(&path) {paths.push(path);}},
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string()),
            }
        }
        let first=!manifest(install).try_exists().map_err(|e|e.to_string())?;
        let candidate=crate::platform::prepare_install(&tool)?;
        let activation=if first {None}else{Some(capture_originals(&activation_directory(install),vec![],activation_paths(install)?)?)};
        let baseline=snapshot(install,roots,paths)?;
        let p=Pending {schema:1,id:crate::runtime::uuid(),baseline,root:root.clone(),version:tool.version.into(),candidate,
            original:edit.original.clone(),next:edit.next.clone(),phase:"prepared".into(),restore:true,attach:false,owner:None,command,activation};
            p
        };
        let mut p=p;p.phase="initial".into();
        save(install,&p)?;
        if let Err(e)=fs::remove_file(install.join(UPGRADE_INTENT)) && e.kind()!=std::io::ErrorKind::NotFound {return Err(e.to_string());}
        (edit,p)
    };
    let result=(||->Res<String> {
    verify_originals(install)?;
    if let Some(activation)=&p.activation {verify_copies(&activation_directory(install),activation)?;}
    if p.phase=="initial" {receive_initial(install,&mut p)?;}
    let receipt=retained(install,&p)?;
    if receipt["state"]=="failed" {
        restore_activation(install,&p)?;
        return Err("original installation failed; originals restored and failed cursor retired; a new command is required".into());
    }
    if receipt["state"]=="observed" {
        if p.phase!="registered" {return Err("observed installation has an incompatible cursor; transition held".into());}
        drop(edit);drop(_update);
        return if acknowledge(install,&p)? {Ok(format!("Install receipt {} already observed",p.id))}else{Err("original installation was observed; current native binding requires reconciliation".into())};
    }
    if digest(&fs::read(tool.release_exe(&p.version)).map_err(|e|e.to_string())?)!=p.candidate {return Err("prepared installation candidate changed; originals retained".into());}
    if matches!(p.phase.as_str(),"prepared"|"activated") {
        let activation=crate::platform::activate_install(&tool);
        #[cfg(test)] let activation=activation.and_then(|_|tests::activation_result(install));
        if let Err(error)=activation {
            return match restore_activation(install,&p) {Ok(())=>Err(format!("installation activation failed; originals restored: {error}")),Err(restore)=>Err(format!("installation activation unknown: {error}; {restore}"))};
        }
        let state=crate::host_control::install_state(&tool,None)?;
        p.phase="starting".into();save(install,&p)?;
        // Persist before native acceptance; replay observes it, never blindly issues another start.
        if let Err(error)=crate::host_control::start_install(&tool,state) {
            if !error.submitted {
                p.phase="prepared".into();save(install,&p)?;
                return match restore_activation(install,&p) {Ok(())=>Err(format!("native start not submitted; originals restored: {}",error.reason)),Err(restore)=>Err(format!("native start not submitted: {}; {restore}",error.reason))};
            }
            return Err(format!("native installation submitted; result unknown, repeat service install for this root to observe: {}",error.reason));
        }
    }
    if p.phase=="starting" {
        retain_reason(install,&p,"native host ownership acknowledgement pending; registry admission held")?;
        let until=Instant::now()+std::time::Duration::from_secs(60);
        loop {
            if let Some(owner)=native_owner(install)? {
                let service=service_status(install);
                if service["fresh"]==true&&service["version"]==tool.version {
                    crate::host_control::install_state(&tool,Some(&owner))?;
                    match coreq::file_lock::try_lock(&install.join("service.lock")) {
                        Err(std::fs::TryLockError::WouldBlock)=>{},_=>return Err("native host ownership lock not observed; installation pending".into()),
                    }
                    p.owner=Some(InstallOwner {pid:owner.pid,started:owner.started,instance:service["instance"].as_str().ok_or("host generation missing")?.into()});
                    p.phase="owned".into();save(install,&p)?;break;
                }
            }
            if Instant::now()>=until {return Err("native installation unknown; projects.installing and originals retained; repeat service install for the same root to reconcile".into());}
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    // Past the native boundary, failure cannot restore the old topology under an owner.
    let owner=native_owner(install)?.ok_or("host installation pending; native owner not observed")?;
    crate::host_control::install_state(&tool,Some(&owner))?;
    let service=service_status(install);
    if service["fresh"]!=true||service["version"]!=p.version||service["install_protocol"]!=1 {
        return Err("host installation pending; exact candidate version/protocol and fresh ownership not observed".into());
    }
    if let Some(old)=&p.owner {
        let expected=coreq::process::Identity {pid:old.pid,started:old.started.clone()};
        if expected!=owner&&coreq::process::identity(old.pid).map_err(|e|e.to_string())?.as_ref()==Some(&expected) {return Err("prior installation owner remains live; reconcile native scope".into());}
    }
    p.owner=Some(InstallOwner {pid:owner.pid,started:owner.started,instance:service["instance"].as_str().ok_or("host generation missing")?.into()});save(install,&p)?;
    if !p.attach {
        if tool.current().as_deref()!=Some(tool.version) {crate::platform::activate_install(&tool)?;}
        let floor=match fs::read_to_string(install.join("rollback-floor")) {Ok(v)=>v,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>service["version"].as_str().ok_or("host version missing")?.into(),Err(e)=>return Err(e.to_string())};
        coreq::launcher::set_rollback_floor(&tool,floor.trim())?;
    }
    let off=crate::install_focus(&root)?;
    p.phase="registered".into();save(install,&p)?;
    edit.commit()?;
    #[cfg(test)] tests::interrupt(install,"registry")?;
    record(install,&p,"applied")?;
    retain_reason(install,&p,"project registration acknowledgement pending; host owns installation")?;
    // The host may finish the original command if this caller disappears during project recovery.
    drop(_update);
    let until=Instant::now()+std::time::Duration::from_secs(60);
    loop {
        if acknowledge(install,&p)? {
            let service=service_status(install);
            return Ok(format!("installed host service (pid {}), project {}{}; Install receipt {}",service["pid"],root.display(),if off {"; Focus: off"}else{""},p.id));
        }
        if Instant::now()>=until {return Err("host owns installation; project acknowledgement pending, repeat service install for this root; originals retained".into());}
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    })();
    if let Err(reason)=&result {
        retain_reason(install,&p,reason).map_err(|e|format!("{reason}; installation reason could not be retained: {e}"))?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    // Chain: §8 Project attachment (A1/A4, H-A5): missing GitLab credentials cannot hold the healthy host.
    #[test]
    fn unauthenticated_gitlab_attachment_preserves_running_host() {
        let (install,root,paths)=fixture();snapshot(&install,vec![root.clone()],paths).unwrap();
        fs::write(root.join("taskq.json"),json!({"board":"gitlab","repo":"invented/project","host":"gitlab.invalid"}).to_string()).unwrap();
        let existing=[install.parent().unwrap().join("existing-a"),install.parent().unwrap().join("existing-b")];
        for root in &existing {fs::create_dir_all(crate::dot(root)).unwrap();fs::write(root.join("taskq.json"),"{}").unwrap();fs::write(crate::dot(root).join("focus"),"off").unwrap();}
        // Existing GitHub loops use in-memory boards; their worker environment gets only this invented value.
        fs::write(install.join("token"),"invented-fixture-not-a-credential").unwrap();
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(install.join("token"),fs::Permissions::from_mode(0o600)).unwrap();}
        let original=existing.iter().map(|root|format!("{}\n",root.display())).collect::<String>();
        fs::write(install.join("projects"),&original).unwrap();
        let mut live=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","host::installation::tests::attachment_live_host_actor","--ignored","--nocapture"])
            .env("TASKQ_ATTACHMENT_CASE",&install).env("TASKQ_INSTALL",&install)
            .env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROOT").env_remove("COREQ_TRACE_CONTEXT")
            .stdout(fs::File::create(install.join("fixture-host.log")).unwrap()).stderr(std::process::Stdio::inherit()).spawn().unwrap();
        let mut cmd=std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact","host::installation::tests::unauthenticated_attachment_actor","--ignored","--nocapture"])
            .env("TASKQ_ATTACHMENT_CASE",&install).env("TASKQ_ATTACHMENT_ROOT",&root)
            .env("TASKQ_INSTALL",&install).env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_ROOT")
            .env_remove("GITLAB_TOKEN").env_remove("GLAB_TOKEN").env_remove("TASKQ_TOKEN").env_remove("COREQ_TRACE_CONTEXT");
        // Exclude CLI login fallback while retaining Cargo's runtime DLL directories.
        let path=std::env::join_paths(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|dir|!dir.join(if cfg!(windows) {"glab.exe"}else{"glab"}).is_file())).unwrap();
        cmd.env("PATH",path);
        let result=(||->Res<()> {
            wait_attachment(&mut live,||existing.iter().all(|root|crate::data::json(&crate::dot(root).join("status.json")).ok().flatten().is_some_and(|s|s["host_instance"]=="invented-auth-host")))?;
            coreq::process::output("test.attachment-auth",&mut cmd,std::time::Duration::from_secs(60)).map_err(|e|format!("credential preflight: {e:?}"))?;
            cmd.env("TASKQ_ATTACHMENT_WAKE_CASE","1");
            coreq::process::output("test.attachment-wake",&mut cmd,std::time::Duration::from_secs(60)).map_err(|e|format!("wake preflight: {e:?}"))?;
            coreq::write_durable(&install.join("verify-continuity"),"")?;
            wait_attachment(&mut live,||install.join("continuity-ok").exists())
        })();
        fs::write(install.join("exit-fixture"),"").unwrap();
        let stopped=wait_attachment(&mut live,||install.join("fixture-joined").exists());
        if stopped.is_err()&&live.try_wait().unwrap().is_none() {live.kill().unwrap();}
        let exit=live.wait().unwrap();
        assert!(result.is_ok()&&stopped.is_ok()&&exit.success(),"attachment: {result:?}; drain: {stopped:?}; host: {exit}\n{}",fs::read_to_string(install.join("fixture-host.log")).unwrap());
        assert_eq!(fs::read_to_string(install.join("projects")).unwrap(),original);
        assert!(!install.join(INSTALL_PENDING).exists()&&!install.join(UPGRADE_INTENT).exists());
        assert!(!crate::dot(&root).join("focus").exists());
        assert_eq!(service_status(&install)["instance"],"invented-auth-host");
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
    fn wait_attachment(child:&mut std::process::Child,ready:impl Fn()->bool)->Res<()> {
        let until=Instant::now()+std::time::Duration::from_secs(60);
        while !ready() {
            if let Some(exit)=child.try_wait().map_err(|e|e.to_string())? {return Err(format!("attachment fixture exited: {exit}"));}
            if Instant::now()>=until {return Err("attachment fixture completion signal expired".into());}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }
    // A4 process boundary: real multi-project loops, in-memory boards; no native manager or agent launches.
    #[test]
    #[ignore="owned live attachment fixture host"]
    fn attachment_live_host_actor() {
        let install=PathBuf::from(std::env::var_os("TASKQ_ATTACHMENT_CASE").unwrap());
        assert!(install.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("taskq-install-originals-"));
        let _owner=owner(&install).unwrap().unwrap();let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let policy=Policy::parse(json!({"limits":{"codex":4},"load_gate":{"enabled":false}}),"invented").unwrap();
        let mut h=Host::new(Ok(policy.clone()));h.instance="invented-auth-host".into();let shared=Arc::new(Mutex::new(h));
        let roots=projects(&install).unwrap();let mut threads=vec![];
        for root in &roots {
            shared.lock().unwrap().attach(root);
            let (root,install,host,policy)=(root.clone(),install.clone(),shared.clone(),policy.clone());
            threads.push(std::thread::spawn(move || {
                enter(&root,&install);let mut cfg=crate::tests::cfg();cfg.root=root;cfg.install=install;cfg.poll=1;policy.apply(&mut cfg);
                crate::service_run(&crate::Ctx {cfg,raw:json!({}),board:Box::new(crate::board::Fake::default())},Some(host))
            }));
        }
        let until=Instant::now()+std::time::Duration::from_secs(120);
        loop {
            publish_service(&install,&native,"invented-auth-host").unwrap();
            let mut h=shared.lock().unwrap();h.held=admission_reason(&install);
            if let Some(index)=threads.iter().position(|t|t.is_finished()) {panic!("existing project loop stopped: {:?}",threads.swap_remove(index).join());}
            if install.join("verify-continuity").exists()&&!install.join("continuity-ok").exists()&&h.publications_ready(&roots) {
                h.reserve(&roots[0],99,100,"codex",&policy.cap,true,0).unwrap();
                h.sync(&roots[0],vec![]);
                coreq::write_durable(&install.join("continuity-ok"),"").unwrap();
            }
            if install.join("exit-fixture").exists() {h.draining.extend(roots.iter().cloned());drop(h);for root in &roots {crate::poke(root).unwrap();}break;}
            assert!(Instant::now()<until,"live attachment fixture expired");drop(h);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for thread in threads {
            while !thread.is_finished() {assert!(Instant::now()<until,"project fixture drain expired");std::thread::sleep(std::time::Duration::from_millis(10));}
            assert_eq!(thread.join().unwrap().unwrap_err(),"project drained");
        }
        coreq::write_durable(&install.join("fixture-joined"),"").unwrap();
    }
    #[test]
    #[ignore="owned credential-less attachment process"]
    fn unauthenticated_attachment_actor() {
        let install=PathBuf::from(std::env::var_os("TASKQ_ATTACHMENT_CASE").unwrap());
        assert!(install.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("taskq-install-originals-"));
        let root=PathBuf::from(std::env::var_os("TASKQ_ATTACHMENT_ROOT").unwrap());
        let wake=std::env::var_os("TASKQ_ATTACHMENT_WAKE_CASE").map(|_|crate::wake_lock(&root).unwrap());
        for _ in 0..2 {
            let error=install_project(&install,&root).unwrap_err();
            if wake.is_some() {assert!(error.contains("wake-lock"),"wake eligibility must precede host transition: {error}");}
            else {assert!(error.contains("no GitLab token"),"missing credential must precede host transition: {error}");}
            assert!(admission_reason(&install).is_none());
        }
    }

    // Chain: §8 Project attachment (A2/A3, H-A5): registered failure -> cancel -> joined -> restored -> failed -> replay.
    // Retain occupied children of other roots, original registry bytes, and every original command receipt.
    #[test]
    fn failed_attachment_cancellation_replays_only_after_owned_empty_drain() {
        for initial in ["registered","owned"] {
            let committed=initial=="registered";
            let (install,root,paths)=fixture();let baseline=snapshot(&install,vec![root.clone()],paths).unwrap();
            let original_bytes=fs::read(manifest(&install)).unwrap();
            let existing=install.parent().unwrap().join("existing");fs::create_dir(&existing).unwrap();
            let existing=existing.canonicalize().unwrap();
            let original=format!("{}\r\n\n",existing.display());
            let next=format!("{original}{}\n",crate::platform::plain_path(&root).display());
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            let _owner=owner(&install).unwrap().unwrap();publish_service(&install,&native,"invented-cancel-host").unwrap();
            let (capture,guard)=crate::profile::tests::collecting(&install);
            let span=tracing::info_span!("host.install");let entered=span.enter();
            let p=Pending {schema:1,id:"invented-failed-attachment".into(),baseline,root:root.clone(),version:crate::platform::VERSION.into(),candidate:"a".repeat(64),
                original:Some(original.clone()),next:next.clone(),phase:initial.into(),restore:false,attach:true,
                owner:Some(InstallOwner {pid:native.pid,started:native.started.clone(),instance:"invented-cancel-host".into()}),activation:None,command:CommandReceipt::current().unwrap().encode()};
            let registry=if committed {&next}else{&original};
            save(&install,&p).unwrap();fs::write(install.join("projects"),registry).unwrap();
            record(&install,&p,"received").unwrap();if committed {record(&install,&p,"applied").unwrap();}
            let applied=retained(&install,&p).unwrap();
            fs::create_dir_all(crate::dot(&root)).unwrap();startup_status(&install,&root,"no GitLab token","invented-cancel-host").unwrap();
            let policy=Policy::parse(json!({"limits":{"codex":4},"load_gate":{"enabled":false}}),"invented").unwrap();
            let mut h=Host::new(Ok(policy.clone()));h.instance="invented-cancel-host".into();
            h.sync(&existing,vec![(7,8,"codex".into())]);h.busy.insert(existing.clone(),true);h.rebuilt.insert(existing.clone());
            h.attach(&root);h.held=admission_reason(&install);
            assert!(h.reserve(&existing,9,10,"codex",&policy.cap,true,0).is_err());
            assert!(cancel_install(&install,&root,"wrong-command").unwrap_err().contains("differs"));
            assert_eq!(fs::read_to_string(install.join("projects")).unwrap(),*registry);
            assert_eq!(installation_status(&install,Some(&p.id))["state"],if committed {"applied"}else{"queued"});
            fs::write(install.join("projects"),if committed {&original}else{&next}).unwrap();
            assert!(cancel_install(&install,&root,&p.id).unwrap_err().contains("registry changed"));
            fs::write(install.join("projects"),registry).unwrap();
            // Corrupt native recovery, a prior generation, and legacy execution all refuse before cursor mutation.
            let kids=crate::kids_path(&install,&root).unwrap();fs::create_dir_all(kids.parent().unwrap()).unwrap();
            fs::write(&kids,"unknown native execution").unwrap();
            assert!(cancel_install(&install,&root,&p.id).is_err());fs::remove_file(&kids).unwrap();
            publish_service(&install,&native,"another-host-generation").unwrap();
            assert!(cancel_install(&install,&root,&p.id).unwrap_err().contains("ownership uncertain"));
            publish_service(&install,&native,"invented-cancel-host").unwrap();
            fs::write(crate::dot(&root).join("kids"),"unknown legacy execution").unwrap();
            assert!(cancel_install(&install,&root,&p.id).unwrap_err().contains("legacy"));fs::remove_file(crate::dot(&root).join("kids")).unwrap();
            assert_eq!(pending(&install).unwrap().phase,initial);
            for _ in 0..2 {
                assert!(cancel_install(&install,&root,&p.id).unwrap().contains("cancellation requested"));
                assert_eq!(pending(&install).unwrap().command,p.command);
                assert_eq!(projects(&install).unwrap(),vec![existing.clone()]);
                assert_eq!(install_acknowledgement(&install,&root,"invented-cancel-host"),None);
                assert!(!acknowledge(&install,&p).unwrap());
                assert!(finish_cancellation(&install,&h,false).unwrap_err().contains("drain/execution"));
                assert_eq!(h.occupied(),1,"existing child remains owned and supervised");
            }
            h.detached(&root);
            assert!(!h.rebuilding,"the removed startup failure cannot strand original occupancy recovery");
            assert!(finish_cancellation(&install,&h,true).is_err(),"cached membership/thread still fences retirement");
            h.sync(&root,vec![(11,12,"codex".into())]);
            assert!(finish_cancellation(&install,&h,false).is_err(),"unknown occupied execution refuses removal");
            h.sync(&root,vec![]);
            // Crash after restoring registry: same cursor and command must finish the remaining audit writes.
            if committed {replace_owned(&install.join("projects"),&next,&original).unwrap();}
            assert!(admission_reason(&install).is_some());
            finish_cancellation(&install,&h,false).unwrap();
            assert_eq!(fs::read_to_string(install.join("projects")).unwrap(),original);
            assert_eq!(fs::read(manifest(&install)).unwrap(),original_bytes);
            let failed=installation_status(&install,Some(&p.id));
            assert_eq!(failed["state"],"failed");assert!(failed.get("observed_receipt").is_none());
            assert_eq!(failed["received_receipt"],applied["received_receipt"]);assert_eq!(failed["applied_receipt"],applied["applied_receipt"]);
            if !committed {assert!(failed.get("applied_receipt").is_none(),"uncommitted cancellation never fabricates application");}
            assert_eq!(failed["cancelled_attachment"]["original"],original);assert_eq!(failed["cancelled_attachment"]["command"],p.command);
            // Crash after the terminal receipt but before cursor retirement still needs host reconciliation.
            let cancelled:Pending=serde_json::from_value(failed["cancelled_attachment"].clone()).unwrap();save(&install,&cancelled).unwrap();
            assert!(cancel_install(&install,&root,&p.id).unwrap().contains("cancellation requested"));
            assert!(admission_reason(&install).is_some());finish_cancellation(&install,&h,false).unwrap();
            for _ in 0..2 {assert!(cancel_install(&install,&root,&p.id).unwrap().contains("cancelled"));}
            assert_eq!(crate::resources::with(&install,|state|Ok(state.ops.len())).unwrap(),1);
            h.reload_admission(Ok(policy.clone()),admission_reason(&install),std::slice::from_ref(&existing));
            assert!(h.reserve(&existing,9,10,"codex",&policy.cap,true,0).is_ok(),"unrelated project can launch after cancellation");
            assert_eq!(h.occupied(),2,"the prior child keeps its slot");
            drop(entered);drop(span);capture.flush().unwrap();drop(guard);drop(capture);drop(_owner);
            fs::remove_dir_all(install.parent().unwrap()).unwrap();
        }
    }

    // Chain: §8 Project attachment: reject known WSL UNC paths before any filesystem or native lock probe.
    #[cfg(windows)]
    #[test]
    fn wsl_unc_attachment_refuses_before_native_wake() {
        let install=std::env::temp_dir().join(format!("taskq-unc-preflight-{}",crate::runtime::uuid()));
        for path in [r"\\wsl.localhost\InventedDistro\project",r"\\WSL$\InventedDistro\project",r"\\?\UNC\wsl.localhost\InventedDistro\project",r"\\?\UNC\WSL$\InventedDistro\project"] {
            let error=install_project(&install,Path::new(path)).unwrap_err();
            assert!(error.contains("unsupported on Windows WSL UNC"),"{error}");
            assert!(!install.exists(),"no installation hold or receipt can precede path eligibility");
        }
    }
    pub(super) fn interrupt(install:&Path,phase:&str)->Res<()> {
        if std::env::var("TASKQ_INSTALL_INTERRUPT").ok().as_deref()!=Some(phase) {return Ok(());}
        if std::env::var_os("TASKQ_INSTALL_PROCESS_CASE").as_deref()!=Some(install.as_os_str())
            ||!install.parent().and_then(Path::file_name).is_some_and(|name|name.to_string_lossy().starts_with("taskq-install-process-")) {return Err("owned installation process fixture required".into());}
        coreq::write_durable(&install.join("interrupted-at"),phase)?;
        let until=Instant::now()+std::time::Duration::from_secs(90);
        while !install.join("release-interruption").exists() {
            if Instant::now()>=until {return Err("installation fixture interruption expired".into());}
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }

    pub(super) fn activation_result(install:&Path)->Res<()> {
        interrupt(install,"activation")?;
        if std::env::var("TASKQ_INSTALL_FAIL_ACTIVATION").as_deref()==Ok("1") {
            if std::env::var_os("TASKQ_INSTALL_PROCESS_CASE").as_deref()!=Some(install.as_os_str()) {return Err("owned activation fixture required".into());}
            return Err("owned fixture activation failure after replacement".into());
        }
        Ok(())
    }

    #[cfg(target_os="macos")]
    fn process_case(case:&str) {
        let report=std::env::temp_dir().join(format!("taskq-install-report-{}.json",crate::runtime::uuid()));
        let result=coreq::process::output("test.install-restore",std::process::Command::new("python3")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/install-process-macos.py"))
            .arg("--binary").arg(std::env::current_exe().unwrap()).arg("--output").arg(&report).arg(case),std::time::Duration::from_secs(150));
        let evidence=fs::read_to_string(&report).unwrap_or_default();
        if report.exists() {fs::remove_file(&report).unwrap();}
        assert!(result.is_ok(),"installation {case}: {result:?}\n{evidence}");
    }

    // H-A5: death between restore writes must finish rollback, never reactivate or start a native unit.
    #[cfg(target_os="macos")]
    #[test]
    fn interrupted_restore_never_reactivates_the_failed_installation() {process_case("interrupted-restore");}

    // H-A5 process instrument: real installer/cursor/receipt, isolated native-manager substitute.
    #[cfg(target_os="macos")]
    #[test]
    #[ignore="requires an owned TASKQ_INSTALL_PROCESS_CASE; native manager is substituted, not OS qualification"]
    fn installation_process_fixture() {
        let install=PathBuf::from(std::env::var_os("TASKQ_INSTALL_PROCESS_CASE").expect("owned installation fixture"));
        assert!(install.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("taskq-install-process-"));
        let base=install.parent().unwrap();
        assert_eq!(std::env::var_os("HOME"),Some(base.join("home").into_os_string()));
        assert_eq!(std::env::split_paths(&std::env::var_os("PATH").unwrap()).next(),Some(base.join("bin")));
        assert!(fs::symlink_metadata(base.join("bin/launchctl")).unwrap().file_type().is_file());
        let role=std::env::var("TASKQ_INSTALL_PROCESS_ROLE").unwrap();
        if role=="host" {
            let _owner=owner(&install).unwrap().expect("one fixture host");
            let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
            let shared=Arc::new(Mutex::new(Host::new(Policy::read(&install))));
            let instance=shared.lock().unwrap().instance.clone();
            let mut threads=BTreeMap::<PathBuf,std::thread::JoinHandle<Res<()>>>::new();
            let until=Instant::now()+std::time::Duration::from_secs(120);
            loop {
                publish_service(&install,&native,&instance).unwrap();
                shared.lock().unwrap().held=admission_reason(&install);
                if !install.join("hold-project-ack").exists() {
                    for root in projects(&install).unwrap_or_default() {
                        if threads.contains_key(&root) {continue;}
                        shared.lock().unwrap().attach(&root);
                        let host=shared.clone();let install=install.clone();
                        threads.insert(root.clone(),std::thread::spawn(move || {
                            enter(&root,&install);
                            let mut cfg=crate::tests::cfg();cfg.root=root;cfg.install=install;
                            cfg.poll=1;Policy::read(&cfg.install)?.apply(&mut cfg);
                            let board=crate::board::Fake::default();
                            crate::board::Board::create(&board,"Must remain ready","",&["taskq".into()])?;
                            crate::service_run(&crate::Ctx {cfg,raw:json!({}),board:Box::new(board)},Some(host))
                        }));
                    }
                }
                for thread in threads.values() {assert!(!thread.is_finished(),"project loop exited before fixture drain");}
                if install.join("exit-host").exists() {
                    shared.lock().unwrap().draining.extend(threads.keys().cloned());
                    // A pass reading the wake signal refuses the poke; the next poll still sees `draining`.
                    for root in threads.keys() {let _=crate::poke(root);}
                    break;
                }
                assert!(Instant::now()<until,"fixture host expired");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            for (root,thread) in threads {
                while !thread.is_finished() {assert!(Instant::now()<until,"project drain expired: {}",root.display());std::thread::sleep(std::time::Duration::from_millis(10));}
                assert_eq!(thread.join().unwrap().unwrap_err(),"project drained");
            }
            return;
        }
        let root=PathBuf::from(std::env::var_os("TASKQ_INSTALL_PROCESS_ROOT").expect("fixture root"));
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let span=tracing::info_span!("host.install");let entered=span.enter();
        let request=pending(&install).ok().map(|p|p.id);
        // This native-manager instrument uses in-memory boards; the credential process case owns real preflight.
        let result=if role=="recover" {request.as_deref().ok_or_else(||"original installation cursor absent".to_string()).and_then(|id|install_bound(&install,&root,Some(id)))}else{upgrade(&install,&root).and_then(|_|install_bound(&install,&root,None))};
        let operation_count=crate::resources::with(&install,|state|Ok(state.ops.len())).unwrap();
        fs::write(install.join("installer-result.json"),serde_json::to_vec(&json!({"ok":result.is_ok(),"result":result.as_ref().ok(),"error":result.as_ref().err(),"status":installation_status(&install,request.as_deref()),"operation_count":operation_count})).unwrap()).unwrap();
        drop(entered);drop(span);
        capture.flush().unwrap();drop(guard);drop(capture);
    }

    fn fixture()->(PathBuf,PathBuf,Vec<PathBuf>) {
        let base=std::env::temp_dir().join(format!("taskq-install-originals-{}",crate::runtime::uuid()));
        let install=base.join("install");let root=base.join("project");
        fs::create_dir_all(&install).unwrap();fs::create_dir_all(&root).unwrap();
        let paths=vec![install.join(crate::platform::tool(&install).exe_name()),install.join("current"),install.join("previous")];
        #[cfg(all(windows,target_env="gnu"))] let paths={let mut paths=paths;paths.push(install.join("WebView2Loader.dll"));paths};
        fs::write(&paths[0],[0,255,13,10,42]).unwrap();fs::write(&paths[1],"original-current\r\n").unwrap();
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&paths[0],fs::Permissions::from_mode(0o751)).unwrap();}
        (install,root.canonicalize().unwrap(),paths)
    }
    // H-A5: partial activation and a failed retry restore bytes, metadata and original absence together.
    #[test]
    fn snapshot_restore_preserves_binary_bytes_metadata_and_absence() {
        let (install,root,paths)=fixture();
        let original:Vec<_>=paths.iter().map(|p|fs::read(p).ok()).collect();
        let hash=snapshot(&install,vec![root.clone()],paths.clone()).unwrap();
        let manifest_bytes=fs::read(manifest(&install)).unwrap();
        for path in &paths {fs::write(path,"partial-activation").unwrap();}
        let baseline:Baseline=serde_json::from_slice(&manifest_bytes).unwrap();
        restore_files(&install,&directory(&install),&baseline).unwrap();
        for (path,bytes) in paths.iter().zip(original) {assert_eq!(fs::read(path).ok(),bytes);}
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;assert_eq!(fs::metadata(&paths[0]).unwrap().permissions().mode()&0o777,0o751);}
        assert_eq!(snapshot(&install,vec![root],paths.clone()).unwrap(),hash,"repeat/attach cannot overwrite topology originals");
        assert_eq!(fs::read(manifest(&install)).unwrap(),manifest_bytes);
        for label in ["later installation","new owner before intent"] {
            for path in &paths {fs::write(path,label).unwrap();}
            let folder=activation_directory(&install);
            let latest=capture_originals(&folder,vec![],paths.clone()).unwrap();
            for path in &paths {fs::write(path,"partial later activation").unwrap();}
            restore_files(&install,&folder,&latest).unwrap();
            for path in &paths {assert_eq!(fs::read_to_string(path).unwrap(),label);}
            assert_eq!(fs::read_dir(folder).unwrap().count(),paths.len(),"reuse one fixed copy set; no per-command archive");
            assert_eq!(fs::read(manifest(&install)).unwrap(),manifest_bytes,"latest-state recovery cannot replace first migration originals");
        }
        #[cfg(windows)] {
            let writable=fs::metadata(&paths[1]).unwrap().permissions();
            let mut readonly=writable.clone();readonly.set_readonly(true);fs::set_permissions(&paths[1],readonly).unwrap();
            let folder=activation_directory(&install);
            let latest=capture_originals(&folder,vec![],paths.clone()).unwrap();
            fs::set_permissions(&paths[1],writable.clone()).unwrap();
            fs::write(&paths[1],"partial readonly activation").unwrap();
            for _ in 0..2 {
                restore_files(&install,&folder,&latest).unwrap();
                assert_eq!(fs::read_to_string(&paths[1]).unwrap(),"new owner before intent");
                assert!(fs::metadata(&paths[1]).unwrap().permissions().readonly());
                assert_eq!(ownership(&paths[1]).unwrap(),latest.files[1].owner);
            }
            fs::set_permissions(&paths[1],writable.clone()).unwrap();
            fs::set_permissions(folder.join("1"),writable).unwrap();
        }
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
    // H-A5: corrupt evidence must be detected before the first restore effect, rather than halfway through.
    #[test]
    fn corrupt_snapshot_refuses_restore_before_any_active_write() {
        let (install,root,paths)=fixture();snapshot(&install,vec![root],paths.clone()).unwrap();
        let baseline:Baseline=serde_json::from_slice(&fs::read(manifest(&install)).unwrap()).unwrap();
        fs::write(directory(&install).join("1"),"corrupt").unwrap();
        fs::write(&paths[0],"current-launcher").unwrap();
        assert!(verify_originals(&install).unwrap_err().contains("checksum"));
        assert!(restore_files(&install,&directory(&install),&baseline).unwrap_err().contains("checksum"));
        assert_eq!(fs::read(&paths[0]).unwrap(),b"current-launcher");
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
    // #1306: under an older running host the receipt comes first, then `current`, rollback floor and the host's own
    // update poke; no `projects.installing` exists until the host runs the candidate, so an older host keeps its
    // transitions. Replay keeps the command, the attach under the candidate adopts it, and an equal host stays attach-only.
    #[test]
    fn older_running_host_upgrades_only_after_the_receipt() {
        let (install,root,paths)=fixture();snapshot(&install,vec![root.clone()],paths).unwrap();
        let root=root.canonicalize().unwrap();
        let tool=crate::platform::tool(&install);
        fs::create_dir_all(install.join("releases/0.0.1")).unwrap();
        fs::write(tool.release_exe("0.0.1"),"invented-release").unwrap();fs::write(install.join("current"),"0.0.1").unwrap();
        let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
        let host=|version:&str| {fs::write(install.join("service"),json!({"schema":1,"pid":native.pid,"started":native.started,"instance":"invented-host",
            "at":wall().unwrap(),"version":version,"install_protocol":1}).to_string()).unwrap();service_status(&install)};
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let span=tracing::info_span!("host.install");let entered=span.enter();
        let old=host("0.0.1");
        let v=request_upgrade(&install,&tool,&root,&old,&native).unwrap();
        assert_eq!(v.as_deref(),Some(crate::platform::VERSION));
        let p=upgrade_intent(&install).unwrap().unwrap();
        for _ in 0..2 {
            assert_eq!(installation_status(&install,Some(&p.id))["state"],"queued","the receipt precedes activation");
            assert!(!install.join(INSTALL_PENDING).exists(),"an older host keeps its transitions");
            assert_eq!(tool.current().as_deref(),Some(crate::platform::VERSION));
            assert_eq!(fs::read_to_string(install.join("previous")).unwrap(),"0.0.1");
            assert_eq!(fs::read_to_string(install.join("rollback-floor")).unwrap(),"0.0.1");
            assert!(install.join("update").exists()&&!install.join("restart").exists(),"the host's own update step re-execs");
            assert_eq!(request_upgrade(&install,&tool,&root,&old,&native).unwrap(),v,"replay repeats nothing destructive");
            assert_eq!(upgrade_intent(&install).unwrap().unwrap().id,p.id);
        }
        assert!(installation_status(&install,Some(&p.id))["reason"].as_str().unwrap().contains("re-exec"));
        // #1601: the waiting host names its real re-exec waits on the receipt; a replayed install keeps them.
        assert!(upgrade_waits(&install,"0.0.1","taskq #1580 unresolved ownership"));
        assert_eq!(request_upgrade(&install,&tool,&root,&old,&native).unwrap(),v);
        let reason=installation_status(&install,Some(&p.id))["reason"].as_str().unwrap().to_string();
        assert!(reason.ends_with("waits for taskq #1580 unresolved ownership"),"{reason}");
        assert!(adopt_upgrade(&install,&root,"0.0.1").unwrap().is_none(),"the old host never binds the candidate's command");
        let new=host(crate::platform::VERSION);
        assert_eq!(request_upgrade(&install,&tool,&root,&new,&native).unwrap(),None);
        // #1402: the host that runs the candidate drops the pre-re-exec reason at once, before the 300 s report.
        assert!(stale_upgrade(&install,false).unwrap().contains("the waiting service install"));
        let reason=installation_status(&install,Some(&p.id))["reason"].as_str().unwrap().to_string();
        assert!(!reason.contains("re-exec waits")&&reason.contains(&p.id),"{reason}");
        assert!(stale_upgrade(&install,true).unwrap().contains("repeat service install"));
        // #1415: another host writer holding the resources lock delays the re-exec'd host's receipt instead of failing it;
        // one held past the bound is refused with the holder named.
        let hold=|release:std::sync::mpsc::Receiver<()>| {
            let (held,ready)=std::sync::mpsc::channel();let install=install.clone();
            let writer=std::thread::spawn(move ||crate::resources::locked(&install,|| {held.send(()).unwrap();release.recv().unwrap();Ok(())}).unwrap());
            ready.recv().unwrap();writer
        };
        let (release,receive)=std::sync::mpsc::channel();let writer=hold(receive);
        let reason=installation_status(&install,Some(&p.id))["reason"].as_str().unwrap().to_string();
        assert!(reason.starts_with("resources lock unavailable:")&&reason.contains(&format!("held by pid {}",std::process::id())),"{reason}");
        let late=std::thread::spawn(move ||{std::thread::sleep(std::time::Duration::from_millis(300));release.send(()).unwrap();});
        let adopted=adopt_upgrade(&install,&root,crate::platform::VERSION).unwrap().unwrap();
        late.join().unwrap();writer.join().unwrap();
        assert_eq!((adopted.id.as_str(),adopted.command.as_str()),(p.id.as_str(),p.command.as_str()),"the attach continues the same command");
        fs::remove_file(install.join(UPGRADE_INTENT)).unwrap();
        assert_eq!(request_upgrade(&install,&tool,&root,&new,&native).unwrap(),None,"attach-only: an equal host is not upgraded");
        assert!(upgrade_intent(&install).unwrap().is_none());
        // #1601: a queued upgrade to an older build failed every newer install until that build repeated it by hand.
        // The newer build supersedes it; the old receipt keeps its failure and reason.
        let mut stale:Pending=serde_json::from_value(json!(adopted)).unwrap();stale.id=crate::runtime::uuid();stale.version="0.0.1".into();
        coreq::write_durable(&install.join(UPGRADE_INTENT),serde_json::to_vec(&stale).unwrap()).unwrap();
        record(&install,&stale,"received").unwrap();
        assert_eq!(request_upgrade(&install,&tool,&root,&new,&native).unwrap(),None);
        let old_receipt=installation_status(&install,Some(&stale.id));
        assert_eq!(old_receipt["state"],"failed");
        assert!(old_receipt["reason"].as_str().unwrap().contains("superseded by"),"{old_receipt}");
        assert!(upgrade_intent(&install).unwrap().is_none());
        drop(entered);drop(span);capture.flush().unwrap();drop(guard);drop(capture);
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
    // H-A5: an interrupted pre-effect snapshot resumes, while an unknown file is preserved and refused.
    #[test]
    fn snapshot_rebuilds_only_its_uncommitted_slots() {
        let (install,root,paths)=fixture();fs::create_dir_all(directory(&install)).unwrap();
        fs::write(directory(&install).join("0"),"partial-copy").unwrap();
        fs::write(directory(&install).join("owner-note"),"preserve").unwrap();
        assert!(snapshot(&install,vec![root.clone()],paths.clone()).unwrap_err().contains("unknown"));
        assert_eq!(fs::read_to_string(directory(&install).join("owner-note")).unwrap(),"preserve");
        fs::remove_file(directory(&install).join("owner-note")).unwrap();
        snapshot(&install,vec![root],paths).unwrap();
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
    // H-A5: only this persisted registration and host generation can publish an installation acknowledgement.
    #[test]
    fn registered_cursor_holds_admission_and_binds_project_acknowledgement() {
        let (install,root,paths)=fixture();let baseline=snapshot(&install,vec![root.clone()],paths).unwrap();
        let mut p=Pending {schema:1,id:"invented-registration".into(),baseline,root:root.clone(),version:crate::platform::VERSION.into(),candidate:"a".repeat(64),
            original:None,next:format!("{}\n",root.display()),phase:"prepared".into(),restore:true,attach:false,
            owner:Some(InstallOwner {pid:7,started:"invented".into(),instance:"invented-host".into()}),activation:None,command:json!({"wire":1,"product":"taskq","version":"fixture","operation":"taskq.host.install","trace":format!("{}.1","a".repeat(32)),"span":format!("{}.2","a".repeat(32)),"parent":null,"started_ms":1}).to_string()};
        save(&install,&p).unwrap();assert!(registered_projects(&install).is_err());
        p.phase="registered".into();save(&install,&p).unwrap();
        assert!(registered_projects(&install).is_err(),"cursor cannot fabricate a registry commit");
        let lock=coreq::file_lock::try_lock(&install.join("projects.lock")).unwrap();
        ProjectEdit {install:install.clone(),original:None,next:p.next.clone(),removed:vec![],adding:true,_lock:lock}.commit().unwrap();
        assert_eq!(registered_projects(&install).unwrap(),Some(p.next.clone()));
        assert!(admission_reason(&install).is_some(),"membership is readable for recovery; launches remain held");
        assert_eq!(install_acknowledgement(&install,&root,"invented-host"),Some(p.id.clone()));
        assert_eq!(install_acknowledgement(&install,&root,"prior-host"),None);
        let prepare=install.join(format!("{INSTALL_PENDING}.pending"));
        fs::write(&prepare,"incomplete cursor publication").unwrap();
        assert!(registered_projects(&install).unwrap_err().contains("publication incomplete"));
        assert!(project_text(&install).is_err());
        assert_eq!(install_acknowledgement(&install,&root,"invented-host"),None);
        fs::remove_file(prepare).unwrap();
        p.version="4.4.0".into();p.attach=true;save(&install,&p).unwrap();
        assert_eq!(install_acknowledgement(&install,&root,"invented-host"),Some(p.id.clone()),"protocol-compatible reader accepts another installer version");
        assert!(ProjectEdit::new(&install,&root,true).is_err(),"another editor cannot consume a registered cursor");
        let retained=fs::read(install.join(INSTALL_PENDING)).unwrap();
        fs::write(manifest(&install),"changed-originals").unwrap();
        assert!(registered_projects(&install).unwrap_err().contains("originals changed"));
        assert_eq!(install_acknowledgement(&install,&root,"invented-host"),None);
        assert_eq!(fs::read(install.join(INSTALL_PENDING)).unwrap(),retained);
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }

    // H-A5: the original observed receipt remains queryable after the installer/cursor disappears.
    #[test]
    fn installation_receipt_survives_cursor_removal_and_rejects_missing_phase_proof() {
        let (install,root,paths)=fixture();let baseline=snapshot(&install,vec![root.clone()],paths.clone()).unwrap();
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let span=tracing::info_span!("host.install");let entered=span.enter();
        let command=CommandReceipt::current().unwrap().encode();
        let mut p=Pending {schema:1,id:"invented-install-receipt".into(),baseline,root:root.clone(),version:crate::platform::VERSION.into(),candidate:"a".repeat(64),
            original:None,next:format!("{}\n",root.display()),phase:"initial".into(),restore:false,attach:true,
            owner:Some(InstallOwner {pid:7,started:"invented".into(),instance:"invented-host".into()}),command,activation:None};
        assert!(receive_initial(&install,&mut p).is_err(),"no receipt before durable intent");
        save(&install,&p).unwrap();
        receive_initial(&install,&mut p).unwrap();assert_eq!(p.phase,"owned");
        p.phase="initial".into();save(&install,&p).unwrap();receive_initial(&install,&mut p).unwrap();
        assert_eq!(crate::resources::with(&install,|state|Ok(state.ops.len())).unwrap(),1,"received replay cannot orphan another command");
        p.activation=Some(capture_originals(&activation_directory(&install),vec![],paths).unwrap());
        assert!(retained(&install,&p).unwrap_err().contains("binding changed"),"latest restore metadata is part of the original command binding");
        p.activation=None;
        retain_reason(&install,&p,"native start response lost; original installation held").unwrap();
        assert_eq!(installation_status(&install,Some(&p.id))["reason"],"native start response lost; original installation held");
        for phase in ["applied","observed"] {record(&install,&p,phase).unwrap();}
        record(&install,&p,"applied").unwrap();
        assert_eq!(installation_status(&install,Some(&p.id))["state"],"observed","a late observer cannot regress the original outcome");
        fs::remove_file(install.join(INSTALL_PENDING)).unwrap();
        let receipt=installation_status(&install,Some(&p.id));
        assert_eq!(receipt["state"],"observed");assert!(receipt["observed_receipt"].is_string());
        assert!(receipt.get("reason").is_none(),"verified progress clears the old wait reason");
        let (first_id,first_command)=(p.id.clone(),p.command.clone());
        let second_span=tracing::info_span!("host.install");let second_enter=second_span.enter();
        p.id="invented-other-installation".into();p.command=CommandReceipt::current().unwrap().encode();
        record(&install,&p,"received").unwrap();
        crate::resources::with(&install,|state| {for id in [&first_id,&p.id] {state.ops.get_mut(id).unwrap().saved_at=Some(42);}Ok(())}).unwrap();
        assert_eq!(installation_status(&install,None)["state"],"unknown","equal observation clocks cannot attribute one command's result to another");
        assert_eq!(installation_status(&install,Some(&first_id))["state"],"observed");
        assert_eq!(installation_status(&install,Some(&p.id))["state"],"queued");
        drop(second_enter);drop(second_span);p.id=first_id;p.command=first_command;
        crate::resources::with(&install,|state| {state.ops.get_mut(&p.id).unwrap().receipt.as_object_mut().unwrap().remove("observed_receipt");Ok(())}).unwrap();
        assert_eq!(installation_status(&install,Some(&p.id))["state"],"unknown");
        drop(entered);drop(span);capture.flush().unwrap();drop(guard);drop(capture);
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }

    // H-A5 model: caller/host observers race after registry commit; only the original command may complete.
    #[test]
    fn detached_install_observers_share_one_acknowledgement_and_cannot_create_a_command() {
        let (install,root,paths)=fixture();let baseline=snapshot(&install,vec![root.clone()],paths.clone()).unwrap();
        let original=fs::read(&paths[0]).unwrap();
        let (capture,guard)=crate::profile::tests::collecting(&install);
        let span=tracing::info_span!("host.install");let entered=span.enter();
        let native=coreq::process::identity(std::process::id()).unwrap().unwrap();
        publish_service(&install,&native,"invented-install-host").unwrap();
        let mut p=Pending {schema:1,id:"invented-detached-install".into(),baseline,root:root.clone(),version:crate::platform::VERSION.into(),candidate:"a".repeat(64),
            original:None,next:format!("{}\n",root.display()),phase:"registered".into(),restore:false,attach:true,
            owner:Some(InstallOwner {pid:native.pid,started:native.started,instance:"invented-install-host".into()}),command:CommandReceipt::current().unwrap().encode(),activation:None};
        save(&install,&p).unwrap();fs::write(install.join("projects"),&p.next).unwrap();
        fs::create_dir_all(crate::dot(&root)).unwrap();
        record(&install,&p,"received").unwrap();record(&install,&p,"applied").unwrap();
        let mut status=json!({"pid":p.owner.as_ref().unwrap().pid,"host_instance":"invented-install-host","host_install":"prior-registration","complete":true,"tasks":[]});
        let path=crate::dot(&root).join("status.json");fs::write(&path,status.to_string()).unwrap();
        assert!(!acknowledge(&install,&p).unwrap(),"same PID/generation cannot acknowledge a different registration");
        status["host_install"]=json!(p.id);status["host_instance"]=json!("prior-host");fs::write(&path,status.to_string()).unwrap();
        assert!(!acknowledge(&install,&p).unwrap(),"old project generation cannot acknowledge the installation");
        status["host_instance"]=json!("invented-install-host");fs::write(&path,status.to_string()).unwrap();
        let mut h=Host::new(Err("publication fixture".into()));h.instance="invented-install-host".into();let shared=Arc::new(Mutex::new(h));
        for (key,value) in [("complete",json!(false)),("error",json!("board credential rejected")),("focus_error",json!("focus unreadable"))] {
            let mut failed=status.clone();failed[key]=value;
            assert!(!crate::publish_status(&crate::platform::tool(&install),&root,&failed,Some(&shared)));
            assert!(crate::data::json(&path).unwrap().unwrap()["host_install"].is_null(),"failed producer cannot stamp acknowledgement");
            fs::write(&path,failed.to_string()).unwrap(); // an older publisher's matching ID still cannot pass the consumer
            assert!(!acknowledge(&install,&p).unwrap(),"failed project publication cannot observe installation: {key}");
            assert_eq!(installation_status(&install,Some(&p.id))["state"],"applied");
            assert!(install.join(INSTALL_PENDING).exists());
        }
        assert!(crate::publish_status(&crate::platform::tool(&install),&root,&status,Some(&shared)));
        let lock=coreq::file_lock::try_lock(&install.join("projects.lock")).unwrap();
        assert!(!acknowledge(&install,&p).unwrap(),"observer never waits on the registry owner");drop(lock);
        assert!(acknowledge(&install,&p).unwrap());assert!(acknowledge(&install,&p).unwrap(),"the other observer reads the retained outcome");
        assert!(install_bound(&install,&root,Some(&p.id)).unwrap().contains("already observed"));
        assert!(!install.join(INSTALL_PENDING).exists());assert_eq!(fs::read(&paths[0]).unwrap(),original);
        assert_eq!(crate::resources::with(&install,|state|Ok(state.ops.len())).unwrap(),1);
        p.id="another-installation".into();save(&install,&p).unwrap();let marker=fs::read(install.join(INSTALL_PENDING)).unwrap();
        assert!(install_bound(&install,&root,Some("invented-detached-install")).unwrap_err().contains("another installation"));
        assert_eq!(fs::read(install.join(INSTALL_PENDING)).unwrap(),marker);
        capture.flush().unwrap();drop(entered);drop(span);drop(guard);drop(capture);
        fs::remove_dir_all(install.parent().unwrap()).unwrap();
    }
}
