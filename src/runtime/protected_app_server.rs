//! R8/R9: one protected native app-server launch on an existing external slot, never a grant authority.
use crate::{Res, policy, resources, runtime};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, io::Read, path::{Path, PathBuf}};

const SCHEMA:&str="taskq.app-server/1";
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {schema:String,slot:Value,policy_revision:u64,account:String}
impl Request {
    fn parse(text:&str)->Res<Self> {
        if text.len()>16384 {return Err("app-server request exceeds 16384 bytes".into());}
        let r:Self=serde_json::from_str(text).map_err(|_|"invalid protected app-server request")?;
        if r.schema!=SCHEMA||r.policy_revision==0||!runtime::accounts::valid_label(&r.account)
            ||r.slot["binding"]["runtime"]!="codex" {return Err("invalid protected app-server identity".into());}
        Ok(r)
    }
    fn root(&self)->Res<PathBuf> {
        let root=PathBuf::from(self.slot["binding"]["root"].as_str().ok_or("canonical slot project required")?);
        if !root.is_absolute()||canonical(&root)?!=root {return Err("canonical slot project required".into());}Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {dir:PathBuf,root:PathBuf,home:PathBuf,bin:PathBuf,r:Request,p:policy::Policy}
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.dir);}}
    impl Fixture {
        fn new()->Self {
            let dir=std::env::temp_dir().join(format!("taskq-protected-{}",runtime::uuid()));fs::create_dir(&dir).unwrap();let dir=canonical(&dir).unwrap();
            let root=dir.join("project");let home=dir.join("home");let bin=dir.join("bin/codex");
            fs::create_dir(bin.parent().unwrap()).unwrap();
            fs::create_dir(&root).unwrap();fs::create_dir_all(home.join(".codex")).unwrap();fs::write(&bin,b"invented native fixture").unwrap();
            fs::write(dir.join("projects"),format!("{}\n",root.display())).unwrap();
            fs::write(dir.join("host.json"),json!({"codex":["-s","danger-full-access","-m","invented-model","-c","model_reasoning_effort=high"]}).to_string()).unwrap();
            let p=policy::Policy{revision:29,grants:vec![policy::Grant{runtime:"codex".into(),mode:"workspace".into(),project:Some(root.to_str().unwrap().into()),by:"invented-operator".into(),at:1}]};
            let r=Request{schema:SCHEMA.into(),slot:json!({"binding":{"root":root,"runtime":"codex"}}),policy_revision:29,account:"default".into()};
            let f=Self{dir,root,home,bin,r,p};f.save();f
        }
        fn save(&self){fs::write(self.dir.join(policy::FILE),serde_json::to_vec(&self.p).unwrap()).unwrap();}
        fn plan(&self)->Res<Plan>{resolve(&self.dir,&self.home,&self.r,"macos",&self.bin)}
    }
    // Chain: R8 R9 A1-A3: a request is a match requirement; missing, host-wide, widened or stale authority cannot launch.
    // Risk: accepting a copied receipt, task text or default grant as protected project authority.
    #[test]
    fn protected_authority_refuses_missing_tampered_stale_and_unknown() {
        let f=Fixture::new();let p=f.plan().unwrap();
        assert_eq!(p.authority["saved"]["grant"]["by"],"invented-operator");
        assert_eq!(p.authority["account_home"],json!(f.home.join(".codex")));
        assert!(p.args.contains(&"model=\"invented-model\"".to_string()));assert!(p.args.contains(&"model_reasoning_effort=high".to_string()));
        assert!(p.access.profile.as_ref().unwrap().contains("deny file-write"));
        for case in ["missing","corrupt","full","host","revision","approver","duplicate","account","quota","quota-shape","path","windows","linux"] {
            let mut f=Fixture::new();let os=match case {"windows"=>"windows","linux"=>"linux",_=>"macos"};
            match case {
                "missing"=>f.p.grants.clear(),"full"=>f.p.grants[0].mode="full".into(),"host"=>f.p.grants[0].project=None,
                "revision"=>f.r.policy_revision+=1,"approver"=>f.p.grants[0].by="unknown".into(),
                "duplicate"=>f.p.grants.push(f.p.grants[0].clone()),"account"=>f.r.account="foreign".into(),
                "path"=>f.r.slot["binding"]["root"]=json!(f.root.join("missing")),
                "quota"=>fs::write(f.dir.join(runtime::accounts::STATE),"unreadable limits").unwrap(),
                "quota-shape"=>fs::write(f.dir.join(runtime::accounts::STATE),"[]").unwrap(),_=>{},
            }
            f.save();if case=="corrupt" {fs::write(f.dir.join(policy::FILE),"receipt text is not a grant").unwrap();}
            assert!(resolve(&f.dir,&f.home,&f.r,os,&f.bin).is_err(),"{case}");
        }
        let mut f=Fixture::new();fs::write(f.dir.join("accounts.json"),"unreadable authority").unwrap();assert!(f.plan().is_err());
        fs::remove_file(f.dir.join("accounts.json")).unwrap();f.p.grants.clear();f.save();assert!(f.plan().is_err());
        let text=json!({"schema":SCHEMA,"slot":{"binding":{"runtime":"codex"}},"policy_revision":29,"account":"default","grant":"workspace"}).to_string();
        assert!(Request::parse(&text).is_err());assert!(Request::parse(&"x".repeat(16385)).is_err());
    }
    // Chain: R8 R9 R13 #2135 A1/A2: the existing Workspace/network request resolves without widening authority.
    // Risk: rejecting approved host config, losing model/effort/account, or forwarding an unsupported override.
    #[test]
    fn protected_workspace_network_host_request_retains_authority_and_config() {
        let f=Fixture::new();
        let args=["-s","workspace-write","-c","sandbox_workspace_write.network_access=true"];
        fs::write(f.dir.join("host.json"),json!({"codex":args}).to_string()).unwrap();
        let p=f.plan().unwrap_or_else(|e|panic!("existing host Workspace/network request must resolve: {e}"));
        assert_eq!(p.args,["-c","sandbox_mode=\"danger-full-access\"","-c","approval_policy=\"never\"","-c","sandbox_workspace_write.network_access=true"]);
        assert_eq!(p.access.mode,"workspace");assert!(p.access.profile.as_ref().unwrap().contains("deny file-write"));
        assert_eq!(p.authority["requested"],args.join(" "));
        assert_eq!(p.authority["account"],"default");assert_eq!(p.authority["account_home"],json!(f.home.join(".codex")));
        let profile=p.authority["profile_sha256"].clone();
        fs::write(f.dir.join("host.json"),json!({"codex":["--sandbox","workspace-write","--config","sandbox_workspace_write.network_access=true","--model","invented-model","--config","model_reasoning_effort=high"]}).to_string()).unwrap();
        let p=f.plan().unwrap();
        assert_eq!(p.authority["profile_sha256"],profile);
        assert_eq!(p.args,["-c","sandbox_mode=\"danger-full-access\"","-c","approval_policy=\"never\"","--config","sandbox_workspace_write.network_access=true","-c","model=\"invented-model\"","--config","model_reasoning_effort=high"]);
        for config in ["sandbox_workspace_write.network_access=false","sandbox_workspace_write.network_access=true.extra","sandbox_workspace_write.network_access=\"true\"","sandbox_workspace_write.writable_roots=[]","sandbox_mode=\"danger-full-access\"","approval_policy=\"never\"","unknown=true"] {
            fs::write(f.dir.join("host.json"),json!({"codex":["-s","workspace-write","-c",config]}).to_string()).unwrap();
            assert!(f.plan().is_err(),"unsupported config must refuse: {config}");
        }
        for args in [vec!["-s","read-only"],vec!["-c"],vec!["--unknown"],vec!["--add-dir","/invented"]] {
            fs::write(f.dir.join("host.json"),json!({"codex":args}).to_string()).unwrap();assert!(f.plan().is_err(),"{args:?}");
        }
    }
    // R9/R12 A2/A7: intent precedes spawn; interrupted/unknown/terminal records all consume the same launch.
    // Risk: restarting a supervisor replays an AssetQ operation or a stale PID is treated as live authority.
    #[test]
    fn protected_interruption_never_replays_and_unknown_identity_refuses() {
        let f=Fixture::new();let p=f.plan().unwrap();let id=coreq::process::Identity{pid:42,started:"invented-creation".into()};
        let mut s=resources::State::default();s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{}}),receipt:json!({"occupied":true,"instance":"invented-host"}),expires_at:1,saved_at:None,writing:false,sent:true});
        consume(&mut s,"slot",&f.r,&p.authority,&id).unwrap();
        input_gate(&s,"slot",&f.r).unwrap();
        let original=s.clone();
        let effective=json!({"state":"verified-effective","scope":"invented-native-observation"});
        assert!(bound_gate(&s,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).is_err());
        let launch=&mut s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"];
        launch["native"]=json!({"pid":id.pid,"started":id.started});launch["state"]=json!("verified-effective");launch["verified_effective"]=effective.clone();
        bound_gate(&s,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).unwrap();
        let verified=s.clone();
        for field in ["authority","native","supervisor","state","verified_effective","cancel_requested","host_instance"] {
            let mut changed=verified.clone();changed.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"][field]=json!("tampered-or-unknown");
            assert!(bound_gate(&changed,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).is_err(),"{field}");
        }
        for field in ["executable_sha256","adapter"] {
            let mut changed=verified.clone();changed.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["authority"][field]=json!("tampered-or-unknown");
            assert!(bound_gate(&changed,"slot",&f.r,&p.authority,&id,&id,Some(&effective)).is_err(),"{field}");
        }
        s=original.clone();
        for state in ["unknown","verified-effective","exited","cancelled"] {
            s.ops.get_mut("slot").unwrap().bound["local_slot"]["launch"]["state"]=json!(state);
            let before=s.clone();assert!(consume(&mut s,"slot",&f.r,&p.authority,&id).is_err());assert_eq!(s,before);
        }
        assert_eq!(original.ops["slot"].bound["local_slot"]["launch"]["state"],"unknown");
        assert!(exact(&id,Err("native observation denied".into())).is_err());assert!(exact(&id,Ok(None)).is_err());
        assert!(exact(&id,Ok(Some(coreq::process::Identity{pid:42,started:"reused-pid".into()}))).is_err());assert!(exact(&id,Ok(Some(id.clone()))).is_ok());
        assert!(native(&json!({"pid":u64::MAX,"started":"invented"})).is_err());
        let mut changed=f.r.clone();changed.policy_revision+=1;assert!(launch_record(&s,"slot",&changed).is_err());
        s=original.clone();cancel(&mut s,"slot",&f.r).unwrap();assert!(input_gate(&s,"slot",&f.r).is_err());
        assert!(native(&s.ops["slot"].bound["local_slot"]["launch"]["native"]).is_err(),"pre-spawn cancellation remains durable without a guessed child");
        assert!(consume(&mut s,"slot",&f.r,&p.authority,&id).is_err());
        s=original.clone();s.ops.get_mut("slot").unwrap().receipt["instance"]=json!("restarted-host");assert!(input_gate(&s,"slot",&f.r).is_err());
        resources::within(&f.dir,resources::WAIT,|stored|{*stored=s.clone();Ok(())}).unwrap();
        resources::within(&f.dir,resources::WAIT,|stored|{assert!(consume(stored,"slot",&f.r,&p.authority,&id).is_err());Ok(())}).unwrap();
        fs::write(resources::dir(&f.dir).join("state.json.pending"),"interrupted save").unwrap();
        assert!(resources::within(&f.dir,resources::WAIT,|_|Ok(())).is_err());
    }
    // R9 A3: the canonical grant writer and launch input gate cannot race a revocation.
    #[test]
    fn protected_grant_writer_shares_the_input_authority_lock() {
        let f=Fixture::new();let held=resources::acquire(&f.dir,resources::WAIT).unwrap();let before=fs::read(f.dir.join(policy::FILE)).unwrap();
        assert!(policy::grant(&f.dir,"codex","none",Some(&f.root),"invented-operator",2).is_err());
        assert_eq!(fs::read(f.dir.join(policy::FILE)).unwrap(),before);drop(held);
        policy::grant(&f.dir,"codex","none",Some(&f.root),"invented-operator",2).unwrap();assert!(f.plan().is_err());
    }
    // Chain: R11 §8 — update retention includes the external adapter until whole-turn release, even if launch is unknown.
    #[test]
    fn protected_unknown_launch_retains_its_release_until_slot_release() {
        let f=Fixture::new();fs::create_dir(f.root.join(".taskq")).unwrap();
        fs::write(f.root.join(".taskq/status.json"),json!({"version":crate::platform::VERSION}).to_string()).unwrap();
        resources::within(&f.dir,resources::WAIT,|s| {
            s.ops.insert("slot".into(),resources::Op{bound:json!({"local_slot":{"launch":{"authority":{"adapter":{"version":"invented-retained"}}}}}),
                receipt:json!({"occupied":true}),expires_at:1,saved_at:None,writing:false,sent:true});Ok(())
        }).unwrap();
        assert!(crate::platform::releases_in_use(&f.dir).unwrap().contains(&"invented-retained".into()));
        resources::within(&f.dir,resources::WAIT,|s|{s.ops.get_mut("slot").unwrap().receipt["occupied"]=json!(false);Ok(())}).unwrap();
        assert!(!crate::platform::releases_in_use(&f.dir).unwrap().contains(&"invented-retained".into()));
        fs::write(resources::dir(&f.dir).join("state.json.pending"),"interrupted save").unwrap();
        assert!(crate::platform::releases_in_use(&f.dir).is_none(),"uncertain ownership retains every release");
    }
}
fn canonical(p:&Path)->Res<PathBuf> {p.canonicalize().map_err(|_|"protected launch path identity unknown".into())}
fn digest(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn file_digest(path:&Path)->Res<String> {
    let mut file=fs::File::open(path).map_err(|_|"protected executable unreadable")?;
    let mut hash=ring::digest::Context::new(&ring::digest::SHA256);let mut bytes=[0;65536];
    loop {let n=file.read(&mut bytes).map_err(|_|"protected executable read unknown")?;if n==0 {break;}hash.update(&bytes[..n]);}
    Ok(hash.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect())
}
fn stamp(path:&Path)->Res<Value> {
    let m=fs::metadata(path).map_err(|_|"protected executable identity unknown")?;
    if !m.is_file() {return Err("protected executable is not a file".into());}
    let modified=m.modified().map_err(|_|"protected executable clock unknown")?;
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;return Ok(json!([m.dev(),m.ino(),m.ctime(),m.ctime_nsec(),m.len(),format!("{modified:?}")]));}
    #[cfg(not(unix))] {Ok(json!([m.len(),format!("{modified:?}"),format!("{:?}",m.created().map_err(|_|"protected executable creation unknown")?)]))}
}

struct Plan {authority:Value,access:policy::Effective,args:Vec<String>,root:PathBuf,home:PathBuf,bin:PathBuf}
// Resolve only host inputs. The request's revision/account are equality requirements, never grants.
fn resolve(install:&Path,home:&Path,r:&Request,os:&str,bin:&Path)->Res<Plan> {
    if os!="macos" {return Err("protected app-server supports qualified macOS Codex only; no Full fallback".into());}
    let root=r.root()?;let install=canonical(install)?;let home=canonical(home)?;let bin=canonical(bin)?;
    if !crate::host::projects(&install)?.contains(&root) {return Err("protected project is not registered".into());}
    let p=policy::load(&install)?;
    let grants:Vec<_>=p.grants.iter().filter(|g|g.runtime=="codex"&&g.project.as_deref()==root.to_str()).collect();
    let g=grants.first().filter(|_|grants.len()==1).ok_or("explicit project Codex Workspace grant required")?;
    if p.revision!=r.policy_revision||g.mode!="workspace"||g.by.trim().is_empty()||g.by=="unknown"||g.at==0 {
        return Err("project Workspace authority missing, stale or mismatched".into());
    }
    let host=crate::host::Policy::read(&install)?;
    let accounts=runtime::accounts::Host::read(&install);
    if let Some(state)=crate::data::json(&install.join(runtime::accounts::STATE))? {
        for limits in state.as_object().ok_or("host account limits unknown")?.values() {
            for limit in limits.as_object().ok_or("host account limits unknown")?.values() {
                if limit["limited"].as_i64().is_none()||(!limit["until"].is_null()&&limit["until"].as_i64().is_none()) {
                    return Err("host account limits unknown; no account fallback".into());
                }
            }
        }
    }
    if accounts.error.is_some()||accounts.active("codex",runtime::accounts::now()).as_deref()!=Some(&r.account) {
        return Err("selected host account unavailable or changed; no account fallback".into());
    }
    let base=home.join(".codex");
    let account=canonical(&if r.account==runtime::accounts::DEFAULT {base.clone()}else{base.join(runtime::accounts::HOMES).join(&r.account)})?;
    let mut access=policy::resolve(&Ok(p.clone()),&policy::Host{os,install:&install,root:&root,home:&home},"codex",&host.permission_mode,&host.codex);
    if access.refused.is_some()||access.mode!="workspace"||access.profile.is_none() {return Err("protected enforcement unknown; no native input".into());}
    let adapter=canonical(&std::env::current_exe().map_err(|_|"adapter executable unknown")?)?;
    // Staged qualification binaries need the same write protection as an installed launcher.
    for path in [&adapter,&bin] {
        let parent=path.parent().ok_or("protected executable parent absent")?;
        if root.starts_with(parent) {return Err("protected executable directory overlaps the writable project".into());}
        access.profile.as_mut().unwrap().push_str(&format!("(deny file-write* (subpath {}))\n",serde_json::to_string(&parent.to_string_lossy()).map_err(|_|"invalid executable path")?));
    }
    for path in [home.join(".local/bin"),home.join(".cargo/bin"),PathBuf::from("/opt/homebrew/bin"),PathBuf::from("/usr/local/bin")] {
        if path.is_dir() {access.profile.as_mut().unwrap().push_str(&format!("(deny file-write* (subpath {}))\n",serde_json::to_string(&canonical(&path)?.to_string_lossy()).map_err(|_|"invalid host executable directory")?));}
    }
    // App-server takes config options, not exec's sandbox/model switches. Preserve only known host requests.
    let mut args=vec!["-c".into(),"sandbox_mode=\"danger-full-access\"".into(),"-c".into(),"approval_policy=\"never\"".into()];
    let mut it=host.codex.iter();
    while let Some(a)=it.next() {match a.as_str() {
        "-s"|"--sandbox"=>{let v=it.next().ok_or("missing host sandbox request")?;if v!="danger-full-access"&&v!="workspace-write" {return Err("host sandbox narrowing is not qualified for app-server".into());}},
        "-m"|"--model"=>{let v=it.next().ok_or("missing host model")?;args.extend(["-c".into(),format!("model={}",serde_json::to_string(v).map_err(|_|"host model invalid")?)]);},
        "-c"|"--config"=>{let v=it.next().ok_or("missing host config request")?;
            if !v.starts_with("model_reasoning_effort=")&&v!="sandbox_workspace_write.network_access=true" {return Err("unsupported host app-server config; existing paths unchanged".into());}
            args.extend([a.clone(),v.clone()]);},
        _=>return Err("unsupported host app-server arguments; existing paths unchanged".into()),
    }}
    let authority=json!({"project":root,"runtime":"codex","profile_sha256":digest(access.profile.as_ref().unwrap().as_bytes()),
        "requested":access.requested,"saved":{"revision":p.revision,"grant":g},"resolved":access.json(),
        "executable":bin,"executable_identity":stamp(&bin)?,"executable_sha256":file_digest(&bin)?,
        "adapter":{"executable":adapter,"identity":stamp(&adapter)?,"sha256":file_digest(&adapter)?,"version":crate::platform::VERSION},
        "account":r.account,"account_home":account,"args":args});
    Ok(Plan{authority,access,args,root,home,bin})
}

fn launch_record<'a>(s:&'a resources::State,key:&str,r:&Request)->Res<&'a Value> {
    let launch=&s.ops.get(key).ok_or("slot absent; execution unknown")?.bound["local_slot"]["launch"];
    if launch["schema"]!=SCHEMA||launch["policy_revision"]!=r.policy_revision||launch["account"]!=r.account {
        return Err("launch identity absent, corrupt or mismatched; never retry launch".into());
    }Ok(launch)
}
fn consume(s:&mut resources::State,key:&str,r:&Request,authority:&Value,supervisor:&coreq::process::Identity)->Res<()> {
    let op=s.ops.get_mut(key).ok_or("slot absent; no launch")?;
    if !op.bound["local_slot"]["launch"].is_null() {return Err("slot launch already consumed; observe the same operation, never replay".into());}
    op.bound["local_slot"]["launch"]=json!({"schema":SCHEMA,"policy_revision":r.policy_revision,"account":r.account,
        "authority":authority,"supervisor":{"pid":supervisor.pid,"started":supervisor.started},"state":"unknown",
        "host_instance":op.receipt["instance"],"cancel_requested":false,
        "reason":"launch intent consumed; native execution not yet verified"});
    Ok(())
}
fn input_gate(s:&resources::State,key:&str,r:&Request)->Res<()> {
    let launch=launch_record(s,key,r)?;
    if launch["cancel_requested"]!=false||launch["host_instance"].as_str().filter(|v|!v.is_empty()).is_none()
        ||launch["host_instance"]!=s.ops[key].receipt["instance"] {
        return Err("launch cancelled or host generation changed; input refused, retain slot".into());
    }Ok(())
}
fn bound_gate(s:&resources::State,key:&str,r:&Request,authority:&Value,id:&coreq::process::Identity,supervisor:&coreq::process::Identity,effective:Option<&Value>)->Res<()> {
    input_gate(s,key,r)?;let record=launch_record(s,key,r)?;
    if record["authority"]!=*authority||native(&record["native"])?!=*id||native(&record["supervisor"])?!=*supervisor
        ||effective.is_some_and(|e|record["state"]!="verified-effective"||record["verified_effective"]!=*e) {
        return Err("durable launch binding changed or uncommitted; input refused".into());
    }Ok(())
}
fn cancel(s:&mut resources::State,key:&str,r:&Request)->Res<Value> {
    launch_record(s,key,r)?;
    let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
    launch["cancel_requested"]=json!(true);Ok(launch.clone())
}
fn native(value:&Value)->Res<coreq::process::Identity> {
    Ok(coreq::process::Identity{pid:value["pid"].as_u64().and_then(|v|u32::try_from(v).ok()).filter(|v|*v>0).ok_or("native identity unknown")?,
        started:value["started"].as_str().filter(|s|!s.is_empty()).ok_or("native identity unknown")?.into()})
}
fn exact(expected:&coreq::process::Identity,seen:Res<Option<coreq::process::Identity>>)->Res<()> {
    if seen?.as_ref()!=Some(expected) {return Err("native incarnation absent or changed; retain slot and reconcile".into());}Ok(())
}

pub(crate) fn versions_in_use(install:&Path)->Res<Vec<String>> {
    resources::try_with(install,|s| {
        s.ops.values().filter(|op|!op.bound["local_slot"]["launch"].is_null()&&op.receipt["occupied"]!=false)
            .map(|op|op.bound["local_slot"]["launch"]["authority"]["adapter"]["version"].as_str().filter(|v|!v.is_empty())
                .map(String::from).ok_or_else(||"external adapter release identity unknown".into())).collect()
    })
}

pub fn command(args:&crate::Args)->Res<()> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot launch or control external app servers".into());}
    if args.pos.len()!=2||args.flags.iter().any(|(k,v)|!["action","request"].contains(&k.as_str())||v.len()!=1) {
        return Err("usage: service app-server --action launch|observe|cancel --request JSON".into());
    }
    let r=Request::parse(args.need("request")?)?;
    #[cfg(target_os="macos")] {mac::command(args.need("action")?,&r)}
    #[cfg(not(target_os="macos"))] {let _=r;Err("protected app-server supports qualified macOS Codex only; no Full fallback".into())}
}

#[cfg(target_os="macos")]
mod mac {
    use super::*;
    use crate::external_slots;
    use std::{ffi::{CStr,CString},io::{self,Write},os::{fd::AsRawFd,unix::ffi::OsStrExt},process::Stdio,sync::mpsc,time::{Duration,Instant}};

    // Sandbox SPI declarations match WebKit's Source/WTF/wtf/spi/darwin/SandboxSPI.h.
    #[link(name="sandbox")]
    unsafe extern "C" {fn sandbox_check(pid:i32,operation:*const libc::c_char,filter:i32,...)->i32;static SANDBOX_CHECK_NO_REPORT:i32;}

    fn host_paths()->Res<(PathBuf,PathBuf)> {
        let mut pwd=std::mem::MaybeUninit::<libc::passwd>::uninit();let mut result=std::ptr::null_mut();let mut buf=vec![0u8;65536];
        let status=unsafe{libc::getpwuid_r(libc::geteuid(),pwd.as_mut_ptr(),buf.as_mut_ptr().cast(),buf.len(),&mut result)};
        if status!=0||result.is_null() {return Err("host account identity unknown".into());}
        let home=canonical(Path::new(std::ffi::OsStr::from_bytes(unsafe{CStr::from_ptr((*result).pw_dir)}.to_bytes())))?;
        let install=canonical(&home.join(".local/share/taskq"))?;
        for (name,expected) in [("HOME",&home),("TASKQ_INSTALL",&install),("CODEX_HOME",&home.join(".codex"))] {
            if let Some(value)=std::env::var_os(name) {if canonical(Path::new(&value))?!=canonical(expected)? {
                return Err("ambient host/account override cannot select authority".into());
            }}
        }
        Ok((install,home))
    }
    fn executable(home:&Path)->Res<PathBuf> {
        // shortcut: standard host CLI locations; use host-owned runtime resolution if custom installations need support.
        let defaults=std::ffi::OsStr::new("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin");
        let path=runtime::user_bin_path(defaults,Some(home.as_os_str())).unwrap_or_else(||defaults.to_os_string());
        runtime::codex_native(runtime::which_in("codex",&path))
    }
    fn live(id:&coreq::process::Identity)->Res<()> {exact(id,coreq::process::identity(id.pid).map_err(|_|"native liveness unknown".into()))}
    fn child_of(id:&coreq::process::Identity,supervisor:&coreq::process::Identity)->Res<()> {
        live(id)?;live(supervisor)?;
        let mut info=std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();let size=std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n=unsafe{libc::proc_pidinfo(id.pid as i32,libc::PROC_PIDTBSDINFO,0,info.as_mut_ptr().cast(),size)};
        if n!=size||unsafe{info.assume_init()}.pbi_ppid!=supervisor.pid {return Err("native child ownership unknown or mismatched; slot retained".into());}
        live(id)?;live(supervisor)
    }
    fn image(id:&coreq::process::Identity,path:&Path)->Res<()> {
        live(id)?;let mut buf=[0u8;4096];
        let n=unsafe{libc::proc_pidpath(id.pid as i32,buf.as_mut_ptr().cast(),buf.len() as u32)};
        if n<=0 {return Err("native executable readback unknown".into());}
        let end=buf.iter().position(|b|*b==0).ok_or("native executable path incomplete")?;
        if canonical(Path::new(std::ffi::OsStr::from_bytes(&buf[..end])))?!=path {return Err("native executable has not matched the selected image".into());}
        live(id)
    }
    fn check(pid:u32,operation:&str,path:&Path,denied:bool)->Res<()> {
        let op=CString::new(operation).map_err(|_|"invalid sandbox operation")?;
        let path=CString::new(path.as_os_str().as_bytes()).map_err(|_|"invalid sandbox path")?;
        let result=unsafe{sandbox_check(pid as i32,op.as_ptr(),1|SANDBOX_CHECK_NO_REPORT,path.as_ptr())};
        if result<0||(result!=0)!=denied {return Err("native sandbox readback unknown or mismatched; input refused".into());}Ok(())
    }
    fn verified(id:&coreq::process::Identity,install:&Path,p:&Plan)->Res<Value> {
        image(id,&p.bin)?;
        check(id.pid,"file-write-data",&p.root.join(".taskq-app-server-probe"),false)?;
        let outside=p.home.join(".taskq-app-server-outside");
        let protected=[install.join(policy::FILE),install.join("taskq"),p.bin.clone(),p.home.join(".codex/config.toml"),outside];
        for path in &protected {for op in ["file-write-data","file-write-create","file-write-unlink"] {check(id.pid,op,path,true)?;}}
        image(id,&p.bin)?;
        Ok(json!({"state":"verified-effective","scope":"live native image and named effective sandbox queries; native workload qualification separate",
            "workspace_write":true,"protected_write_create_unlink_denied":protected,"reads":"existing profile","network":"existing profile"}))
    }
    fn refresh(install:&Path,home:&Path,r:&Request,original:&Value)->Res<Plan> {
        let bin=executable(home)?;let p=resolve(install,home,r,"macos",&bin)?;
        if p.authority!=*original {return Err("bound project/profile/account/executable authority changed; native input refused".into());}Ok(p)
    }
    fn key(install:&Path,r:&Request)->Res<String> {
        resources::within(install,resources::WAIT,|s|external_slots::reserved(install,s,&r.slot,"codex"))
    }
    pub(super) fn command(action:&str,r:&Request)->Res<()> {
        if !["launch","observe","cancel"].contains(&action) {return Err("app-server action must be launch, observe or cancel".into());}
        let (install,home)=host_paths()?;
        // Existing slot authentication, including exact payload/key, occurs before any native action.
        let observed=external_slots::exchange(&install,"observe",&r.slot)?;
        let slot_key=format!("slot-{}",digest(format!("{}\0{}",observed["caller"].as_str().ok_or("slot caller unknown")?,observed["op_id"].as_str().ok_or("slot operation unknown")?).as_bytes()));
        if action=="launch" {return launch(&install,&home,r,&slot_key);}
        let record=resources::within(&install,resources::WAIT,|s|if action=="cancel" {cancel(s,&slot_key,r)}else{Ok(launch_record(s,&slot_key,r)?.clone())})?;
        let mut receipt=json!({"schema":SCHEMA,"operation":r.slot["binding"]["op_id"],"caller":r.slot["binding"]["caller"],"launch":record,"state":"unknown"});
        let id=match native(&record["native"]) {
            Ok(id)=>id,Err(reason)=>{receipt["reason"]=json!(if action=="cancel" {"cancellation saved before native identity; input remains closed".into()}else{reason});println!("{receipt}");return Ok(());}
        };
        if action=="cancel" {
            // Revocation closes input, but cannot remove the original owner's exact-process stop authority.
            child_of(&id,&native(&record["supervisor"])?)?;
            image(&id,Path::new(record["authority"]["executable"].as_str().ok_or("bound executable unknown")?))?;
            coreq::process::terminate(&id).map_err(|_|"native cancellation unconfirmed; retain slot and reconcile")?;
            receipt["reason"]=json!("exact native termination requested; turn/descendant completion unproved; slot retained");
        } else {
            let result=(||->Res<()> {
                key(&install,r)?;let p=refresh(&install,&home,r,&record["authority"])?;
                let supervisor=native(&record["supervisor"])?;live(&supervisor)?;
                child_of(&id,&supervisor)?;
                let effective=verified(&id,&install,&p)?;
                resources::within(&install,resources::WAIT,|s|bound_gate(s,&slot_key,r,&p.authority,&id,&supervisor,Some(&effective)))?;
                receipt["verified_effective"]=effective;
                if record["state"]!="verified-effective" {return Err("launch receipt uncommitted; execution unknown".into());}Ok(())})();
            match result {Ok(())=>receipt["state"]=json!("verified-effective"),Err(reason)=>receipt["reason"]=json!(reason)}
        }
        println!("{receipt}");Ok(())
    }
    fn launch(install:&Path,home:&Path,r:&Request,key:&str)->Res<()> {
        let bin=executable(home)?;let p=resolve(install,home,r,"macos",&bin)?;
        let supervisor=coreq::process::identity(std::process::id()).map_err(|_|"supervisor identity unknown")?.ok_or("supervisor identity absent")?;
        resources::within(install,resources::WAIT,|s| {
            if external_slots::reserved(install,s,&r.slot,"codex")?!=key {return Err("slot changed".into());}
            refresh(install,home,r,&p.authority)?;consume(s,key,r,&p.authority,&supervisor)
        })?;
        let exe=std::env::current_exe().map_err(|_|"adapter executable unknown")?;
        let env=runtime::Env{context:"",root:&p.root,exe:&exe,install,permission_mode:"",codex:&p.args,publish:"",board_env:&[],jobs:None};
        let mut cmd=runtime::executable(p.bin.clone(),&p.access,&env)?;
        // Neither authority secrets nor an ambient environment choose native account, configuration or executable.
        cmd.env_clear();for name in ["PATH","LANG","LC_ALL","TMPDIR","TERM"] {if let Some(v)=std::env::var_os(name) {cmd.env(name,v);}}
        cmd.env("HOME",home);
        let spec=runtime::optimized(agentq_runtime::agent::Spec{client:agentq_runtime::agent::Client::Codex,root:p.root.clone(),session:"-".into(),
            resume:false,args:p.args.iter().map(Into::into).collect(),home:Some(PathBuf::from(p.authority["account_home"].as_str().ok_or("account home unknown")?)),env:vec![]})?;
        let mut cmd=agentq_runtime::agent::app_server_command(cmd,&spec)?;
        cmd.stdin(Stdio::piped()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
        resources::within(install,resources::WAIT,|s|{external_slots::reserved(install,s,&r.slot,"codex")?;input_gate(s,key,r)})?;
        let mut child=cmd.spawn().map_err(|_|"native spawn outcome unknown; intent retained, never replay")?;
        let id=coreq::process::identity(child.id()).map_err(|_|"spawned native identity unknown; retain intent")?.ok_or("spawned native absent; retain intent")?;
        resources::within(install,resources::WAIT,|s| {
            launch_record(s,key,r)?;
            let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
            launch["native"]=json!({"pid":id.pid,"started":id.started});Ok(())
        })?;
        let mut input=child.stdin.take().ok_or("native stdin unavailable; intent retained")?;
        let gate=(||->Res<()> {
            let until=Instant::now()+Duration::from_secs(5);
            loop {
                match image(&id,&p.bin) {Ok(())=>break,Err(e)=>{if Instant::now()>=until||child.try_wait().map_err(|_|"native liveness unknown")?.is_some() {return Err(e);}std::thread::sleep(Duration::from_millis(10));}}
            }
            resources::within(install,resources::WAIT,|s| {
                external_slots::reserved(install,s,&r.slot,"codex")?;refresh(install,home,r,&p.authority)?;
                bound_gate(s,key,r,&p.authority,&id,&supervisor,None)?;child_of(&id,&supervisor)?;
                let effective=verified(&id,install,&p)?;
                let launch=&mut s.ops.get_mut(key).ok_or("slot disappeared")?.bound["local_slot"]["launch"];
                launch["verified_effective"]=effective;launch["state"]=json!("verified-effective");launch["reason"]=Value::Null;Ok(())
            })
        })();
        if let Err(e)=gate {drop(input);let _=coreq::process::terminate(&id);return Err(e);}
        let flags=unsafe{libc::fcntl(input.as_raw_fd(),libc::F_GETFL)};
        if flags<0||unsafe{libc::fcntl(input.as_raw_fd(),libc::F_SETFL,flags|libc::O_NONBLOCK)}<0 {return Err("bounded native input unavailable; intent retained".into());}
        let (tx,rx)=mpsc::sync_channel(1);
        std::thread::spawn(move||{let mut stdin=io::stdin().lock();loop {let mut bytes=vec![0;65536];match stdin.read(&mut bytes) {
            Ok(0)=>{let _=tx.send(Ok(Vec::new()));break;},Ok(n)=>{bytes.truncate(n);if tx.send(Ok(bytes)).is_err(){break;}},
            Err(e)=>{let _=tx.send(Err(e.to_string()));break;}
        }}});
        loop {
            if let Some(status)=child.try_wait().map_err(|_|"native liveness unknown; slot retained")? {
                return if status.success(){Ok(())}else{Err("native app-server exited; turn outcome unproved, slot retained".into())};
            }
            let bytes=match rx.recv_timeout(Duration::from_millis(250)) {Ok(bytes)=>bytes?,Err(mpsc::RecvTimeoutError::Timeout)=>continue,
                Err(_)=>return Err("consumer input interrupted; native outcome unknown, slot retained".into())};
            if bytes.is_empty() {drop(input);return Err("consumer input ended; native/descendant completion unproved, slot retained".into());}
            resources::within(install,resources::WAIT,|s| {
                external_slots::reserved(install,s,&r.slot,"codex")?;refresh(install,home,r,&p.authority)?;let effective=verified(&id,install,&p)?;
                bound_gate(s,key,r,&p.authority,&id,&supervisor,Some(&effective))?;child_of(&id,&supervisor)?;
                let until=Instant::now()+Duration::from_secs(1);let mut left=bytes.as_slice();
                while !left.is_empty() {match input.write(left) {
                    Ok(0)=>return Err("native input disconnected; delivery unknown, never replay".into()),Ok(n)=>left=&left[n..],
                    Err(e) if e.kind()==io::ErrorKind::Interrupted=>{},
                    Err(e) if e.kind()==io::ErrorKind::WouldBlock&&Instant::now()<until=>{std::thread::sleep(Duration::from_millis(5));},
                    Err(_)=>return Err("native input deadline/error; delivery unknown, never replay".into()),
                }}Ok(())
            })?;
        }
    }
}
