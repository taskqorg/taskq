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
        op.receipt["reason"]=json!(crate::cli::text::compact(reason,256));Ok(())
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
    let Some(r)=service_record(install)? else {return Ok(None)};
    let expected=coreq::process::Identity {pid:r.pid,started:r.started};
    Ok(coreq::process::identity(r.pid).map_err(|e|e.to_string())?.filter(|native|native==&expected))
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
    let status=crate::data::json(&crate::paths::dot(&p.root).join("status.json"))?;
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
    crate::host::seed_policy(install)?;
    upgrade(install,root)?;
    install_bound(install,root,None)
}

fn preflight(install:&Path,root:&Path)->Res<()> {
    drop(crate::wake::wake_lock(root)?);
    let (cfg,raw,_)=crate::config::check_load(root)?;
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
    match fs::symlink_metadata(crate::paths::dot(root).join("kids")) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err("legacy project execution uncertain; attachment kept".into()),
    }
    let path=crate::paths::kids_path(install,root)?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
        Ok(meta) if meta.is_file()&&meta.len()<=crate::data::KIDS_MAX=>{
            let (recorded,kids)=crate::data::kids(&fs::read_to_string(&path).map_err(|e|e.to_string())?)?;
            if &recorded!=owner||!kids.is_empty() {return Err("project execution or recovery ownership uncertain; attachment kept".into());}
        },
        _=>return Err("project execution evidence unreadable; attachment kept".into()),
    }
    if let Some(status)=crate::data::json(&crate::paths::dot(root).join("status.json"))? {
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

pub(super) fn finish_cancellation(install:&Path,h:&mut Host,served:bool)->Res<()> {
    let _update=coreq::file_lock::try_lock(&install.join("update.lock")).map_err(|e|e.to_string())?;
    let _projects=coreq::file_lock::try_lock(&install.join("projects.lock")).map_err(|e|e.to_string())?;
    let p=pending(install)?;
    if p.phase!="cancelling"||p.owner.as_ref().is_none_or(|o|o.pid!=std::process::id()||o.instance!=h.instance) {
        return Err("attachment cancellation belongs to another host; kept".into());
    }
    if served||h.slots.keys().any(|(root,_)|root==&p.root) {
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
    coreq::write_durable(&install.join("poke"),"")?;
    h.detached(&p.root);
    fs::remove_file(install.join(INSTALL_PENDING)).map_err(|e|e.to_string())
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
/// #2209: the host asks on every tick, so an intent that superseded another, or an install replay that reset the
/// reason, gets the waits again although they did not change; an equal reason is not rewritten.
pub fn upgrade_waits(install:&Path,running:&str,waits:&str)->bool {
    let Ok(Some(p))=upgrade_intent(install) else {return true};
    let receipt=installation_status(install,Some(&p.id));
    if p.version==running||receipt["state"]!="queued" {return true;}
    let reason=crate::cli::text::compact(&format!("host re-exec into {} waits for {waits}",p.version),256);
    receipt["reason"].as_str()==Some(reason.as_str())||retain_reason(install,&p,&reason).is_ok()
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
        paths.extend(roots.iter().map(|r|crate::paths::dot(r).join("focus")));
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
    let off=crate::cli::service_cmd::install_focus(&root)?;
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
#[path = "host_install_tests.rs"]
mod tests;
