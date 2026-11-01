//! R8/R9/R12: one launchd generation and a kernel-accounted descendant scope, never a scheduler.
use super::*;
use std::{io::{self,BufRead,BufReader,Write},net::Shutdown,os::{fd::{AsRawFd},unix::{fs::{DirBuilderExt,MetadataExt},net::{UnixListener,UnixStream}}},process::{Command,Stdio},time::{Duration,Instant}};
const WAIT:Duration=Duration::from_secs(5);

unsafe extern "C" {fn coalition_info_resource_usage(id:u64,buffer:*mut libc::c_void,size:usize)->i32;}
fn identity()->Res<coreq::process::Identity> {
    coreq::process::identity(std::process::id()).map_err(|_|"scope identity unknown")?.ok_or("scope identity absent".into())
}
fn coalition(pid:u32)->Res<u64> {
    let mut words=[0u64;5];
    let n=unsafe{libc::proc_pidinfo(pid as i32,20,0,words.as_mut_ptr().cast(),40)};
    if n!=40||words[0]==0 {return Err("resource coalition layout/identity unknown".into());}Ok(words[0])
}
fn counts(cid:u64)->Res<[u64;2]> {
    let mut words=[0x123456789abcdef0u64,u64::MAX,u64::MAX,0xfedcba9876543210];
    let rc=unsafe{coalition_info_resource_usage(cid,words.as_mut_ptr().add(1).cast(),16)};
    if rc!=0||words[0]!=0x123456789abcdef0||words[3]!=0xfedcba9876543210||words[1]==u64::MAX||words[2]>words[1] {
        return Err(format!("resource coalition counters unknown: return={rc}; retain original scope"));
    }Ok([words[1],words[2]])
}
pub(super) fn membership(id:&coreq::process::Identity,binding:&Value)->Res<()> {
    super::mac::live(id)?;
    if binding["boot"]!=crate::platform::boot_time()?||binding["coalition"]!=coalition(id.pid)? {return Err("native scope incarnation changed; retain occupancy".into());}
    super::mac::live(id)
}
fn peer(socket:&UnixStream)->Res<coreq::process::Identity> {
    let mut pid:libc::pid_t=0;let mut len=std::mem::size_of_val(&pid) as libc::socklen_t;
    // LOCAL_PEERPID is a kernel fact, not a PID claimed in protocol text.
    let rc=unsafe{libc::getsockopt(socket.as_raw_fd(),0,2,(&mut pid as *mut libc::pid_t).cast(),&mut len)};
    let mut uid=0;let mut gid=0;
    if rc!=0||len as usize!=std::mem::size_of_val(&pid)||pid<=0||unsafe{libc::getpeereid(socket.as_raw_fd(),&mut uid,&mut gid)}!=0||uid!=unsafe{libc::geteuid()} {
        return Err("scope peer credentials unknown".into());
    }
    coreq::process::identity(pid as u32).map_err(|_|"scope peer incarnation unknown")?.ok_or("scope peer absent".into())
}
fn socket_deadlines(socket:&UnixStream)->Res<()> {
    socket.set_read_timeout(Some(WAIT)).and_then(|_|socket.set_write_timeout(Some(WAIT))).map_err(|_|"scope transport deadline unavailable".into())
}
fn line(reader:&mut BufReader<UnixStream>,limit:usize)->Res<String> {
    let mut bytes=Vec::new();
    reader.take((limit+1) as u64).read_until(b'\n',&mut bytes).map_err(|_|"scope binding read interrupted; never redispatch")?;
    if bytes.len()>limit||bytes.last()!=Some(&b'\n') {return Err("scope binding exceeds deadline/size or is incomplete".into());}
    String::from_utf8(bytes).map_err(|_|"invalid scope binding encoding".into())
}
fn accept(listener:&UnixListener,until:Instant)->Res<UnixStream> {
    loop {match listener.accept() {
        Ok((s,_))=>{socket_deadlines(&s)?;return Ok(s);},
        Err(e) if e.kind()==io::ErrorKind::WouldBlock&&Instant::now()<until=>std::thread::sleep(Duration::from_millis(10)),
        Err(_)=>return Err("scope connection unknown; never repeat dispatch".into()),
    }}
}
fn xml(text:&str)->String {text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;").replace('\'',"&apos;")}
fn plist(label:&str,program:&str,key:&str,root:&str,environment:&Value)->Res<String> {
    let mut vars=String::new();
    for (k,v) in environment.as_object().ok_or("scope environment unknown")? {
        vars.push_str(&format!("<key>{}</key><string>{}</string>",xml(k),xml(v.as_str().ok_or("scope environment invalid")?)));
    }
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>{}</string><key>Program</key><string>{}</string><key>ProgramArguments</key><array><string>{}</string><string>service</string><string>app-server</string><string>--action</string><string>scope-run</string><string>--request</string><string>{}</string></array><key>WorkingDirectory</key><string>{}</string><key>EnvironmentVariables</key><dict>{vars}</dict><key>RunAtLoad</key><false/><key>KeepAlive</key><false/><key>LaunchOnlyOnce</key><true/><key>AbandonProcessGroup</key><true/></dict></plist>",xml(label),xml(program),xml(program),xml(key),xml(root)))
}
fn launchctl(args:&[&str])->Res<String> {
    let out=coreq::process::output("protected.scope.launchctl",Command::new("/bin/launchctl").args(args).stdin(Stdio::null()),WAIT)
        .map_err(|_|"native job command unacknowledged; retain intent, never retry")?;
    if out.stdout.len()>65536 {return Err("native job readback exceeds bound; retain intent".into());}
    String::from_utf8(out.stdout).map_err(|_|"native job readback encoding unknown".into())
}
fn record(install:&Path,r:&Request,key:&str)->Res<Value> {resources::within(install,resources::WAIT,|s|Ok(launch_record(s,key,r)?.clone()))}

pub(super) fn launch(install:&Path,home:&Path,r:&Request,key:&str)->Res<()> {
    let bin=super::mac::executable(home)?;let p=resolve(install,home,r,"macos",&bin)?;let frontend=identity()?;
    let directory=scope_directory(install,r)?;
    if directory.as_os_str().as_encoded_bytes().len()+5>103 {return Err("protected native socket path exceeds platform bound; no launch".into());}
    let label=format!("org.taskq.protected.{}",runtime::uuid());let domain=format!("gui/{}",unsafe{libc::geteuid()});
    let mut environment=serde_json::Map::new();
    for name in ["PATH","LANG","LC_ALL","TMPDIR","TERM"] {if let Ok(value)=std::env::var(name) {environment.insert(name.into(),json!(value));}}
    environment.insert("HOME".into(),json!(home));
    let job=plist(&label,p.authority["adapter"]["executable"].as_str().ok_or("adapter path unknown")?,key,p.root.to_str().ok_or("project path invalid")?,&json!(environment))?;
    resources::within(install,resources::WAIT,|s| {
        if crate::external_slots::reserved(install,s,&r.slot,"codex")?!=key {return Err("slot changed".into());}
        super::mac::refresh(install,home,r,&p.authority)?;
        consume(s,key,r,&p.authority,&frontend)?;
        let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
        launch["frontend"]=json!({"pid":frontend.pid,"started":frontend.started});launch["input_closed"]=json!(false);
        launch["request_sha256"]=json!(digest(serde_json::to_string(r).map_err(|_|"request serialization failed")?.as_bytes()));
        launch["scope"]=json!({"label":label,"domain":domain,"directory":directory,"job_sha256":digest(job.as_bytes()),"dispatch_ack":false,"cleanup_intent":false,"cleanup_verified":false});
        Ok(())
    })?;
    fs::create_dir_all(directory.parent().ok_or("scope parent absent")?).map_err(|_|"scope parent unavailable")?;
    // Existing paths/generations are never replaced, including after a lost acknowledgment.
    fs::DirBuilder::new().mode(0o700).create(&directory).map_err(|_|"scope generation unavailable; intent retained, never replace")?;
    let io=UnixListener::bind(directory.join("io")).map_err(|_|"scope stdio unavailable; intent retained")?;
    let err=UnixListener::bind(directory.join("err")).map_err(|_|"scope stderr unavailable; intent retained")?;
    io.set_nonblocking(true).and_then(|_|err.set_nonblocking(true)).map_err(|_|"bounded scope accept unavailable")?;
    crate::platform::write_synced(&directory.join("job.plist"),&job)?;
    let directory_identity=query_identity(&directory)?;
    resources::within(install,resources::WAIT,|s|{s.ops.get_mut(key).ok_or("scope operation absent")?.bound["local_slot"]["launch"]["scope"]["directory_identity"]=directory_identity.clone();Ok(())})?;
    if file_digest(&directory.join("job.plist"))?!=digest(job.as_bytes())||query_identity(&directory)?!=directory_identity {return Err("scope artifact changed before registration; no dispatch".into());}
    launchctl(&["bootstrap",&domain,directory.join("job.plist").to_str().ok_or("job path invalid")?])?;
    let target=format!("{domain}/{label}");launchctl(&["kickstart",&target])?;
    let until=Instant::now()+WAIT;let mut output=BufReader::new(accept(&io,until)?);let errors=accept(&err,until)?;
    let supervisor=peer(output.get_ref())?;
    if peer(&errors)?!=supervisor {return Err("scope stderr belongs to another incarnation".into());}
    super::mac::image(&supervisor,Path::new(p.authority["adapter"]["executable"].as_str().ok_or("adapter path unknown")?))?;
    let binding:Value=serde_json::from_str(&line(&mut output,4096)?).map_err(|_|"scope binding corrupt")?;
    membership(&supervisor,&binding)?;
    if binding["coalition"]==coalition(frontend.pid)? {return Err("scope shares frontend coalition; native input refused".into());}
    resources::within(install,resources::WAIT,|s| {
        crate::external_slots::reserved(install,s,&r.slot,"codex")?;super::mac::refresh(install,home,r,&p.authority)?;
        input_gate(s,key,r)?;
        let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
        if launch["frontend"]!=json!({"pid":frontend.pid,"started":frontend.started})||launch["scope"]["label"]!=label {return Err("scope launch binding changed".into());}
        launch["supervisor"]=json!({"pid":supervisor.pid,"started":supervisor.started});launch["scope"]["binding"]=binding;
        launch["scope"]["dispatch_ack"]=json!(true);Ok(())
    })?;
    output.get_mut().write_all(format!("{}\n",serde_json::to_string(r).map_err(|_|"request serialization failed")?).as_bytes()).map_err(|_|"scope input delivery unknown; never replay")?;
    let mut input=output.get_ref().try_clone().map_err(|_|"scope input unavailable")?;
    std::thread::spawn(move||{let _=io::copy(&mut io::stdin().lock(),&mut input);let _=input.shutdown(Shutdown::Write);});
    errors.set_read_timeout(None).map_err(|_|"scope stderr stream unavailable")?;
    let stderr=std::thread::spawn(move||io::copy(&mut &errors,&mut io::stderr().lock()));
    output.get_ref().set_read_timeout(None).map_err(|_|"scope stdout stream unavailable")?;
    io::copy(&mut output,&mut io::stdout().lock()).map_err(|_|"native stdout interrupted; retain scope")?;
    stderr.join().map_err(|_|"native stderr interrupted")?.map_err(|_|"native stderr delivery unknown")?;
    let ended=record(install,r,key)?;
    if ended["native_ok"]==true {Ok(())}else{Err("native stream ended without a successful root outcome; reconcile original scope".into())}
}

pub(super) fn run(key:&str)->Res<()> {
    if key.len()!=69||!key.starts_with("slot-")||!key[5..].bytes().all(|b|b.is_ascii_hexdigit()) {return Err("invalid native scope operation".into());}
    let (install,home)=super::mac::host_paths()?;
    let launch=resources::within(&install,resources::WAIT,|s|Ok(s.ops.get(key).ok_or("original scope intent absent")?.bound["local_slot"]["launch"].clone()))?;
    if launch["scope"]["dispatch_ack"]!=false||launch["input_closed"]!=false {return Err("scope generation already consumed or sealed; no replay".into());}
    let directory=PathBuf::from(launch["scope"]["directory"].as_str().ok_or("scope directory unknown")?);
    let metadata=fs::symlink_metadata(&directory).map_err(|_|"scope directory unavailable")?;
    if !metadata.is_dir()||metadata.uid()!=unsafe{libc::geteuid()}||metadata.mode()&0o777!=0o700 {return Err("scope directory ownership unknown".into());}
    if query_identity(&directory)?!=launch["scope"]["directory_identity"] {return Err("scope directory identity changed".into());}
    let program=Path::new(launch["authority"]["adapter"]["executable"].as_str().ok_or("adapter image unknown")?);
    let supervisor=identity()?;super::mac::image(&supervisor,program)?;
    if file_digest(program)?!=launch["authority"]["adapter"]["sha256"] {return Err("scope adapter artifact changed".into());}
    let mut socket=UnixStream::connect(directory.join("io")).map_err(|_|"original scope frontend unavailable")?;
    let errors=UnixStream::connect(directory.join("err")).map_err(|_|"original scope stderr unavailable")?;
    socket_deadlines(&socket)?;socket_deadlines(&errors)?;
    let frontend=native(&launch["frontend"])?;
    if peer(&socket)?!=frontend||peer(&errors)?!=frontend {return Err("original scope frontend incarnation changed".into());}
    super::mac::image(&frontend,program)?;
    let binding=json!({"boot":crate::platform::boot_time()?,"coalition":coalition(supervisor.pid)?});
    if binding["coalition"]==coalition(frontend.pid)? {return Err("native scope is not isolated".into());}
    socket.write_all(format!("{binding}\n").as_bytes()).map_err(|_|"scope binding delivery unknown")?;
    let mut input=BufReader::new(socket);let text=line(&mut input,16384)?;let r=Request::parse(text.trim_end())?;
    if slot_id(&r)?!=key||digest(serde_json::to_string(&r).map_err(|_|"request serialization failed")?.as_bytes())!=launch["request_sha256"] {return Err("original scope request changed".into());}
    let bound=record(&install,&r,key)?;
    if bound["supervisor"]!=json!({"pid":supervisor.pid,"started":supervisor.started})||bound["scope"]["binding"]!=binding||bound["scope"]["dispatch_ack"]!=true {return Err("scope binding not committed; input refused".into());}
    membership(&supervisor,&binding)?;
    let c=counts(binding["coalition"].as_u64().ok_or("coalition unknown")?)?;
    if c[0].checked_sub(c[1])!=Some(1) {return Err("scope contains unbound work before native launch; retain occupancy".into());}
    let output=input.get_ref().try_clone().map_err(|_|"scope output unavailable")?;
    let result=super::mac::launch_native(&install,&home,&r,key,input,errors);
    resources::within(&install,resources::WAIT,|s| {
        launch_record(s,key,&r)?;
        let launch=&mut s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"];
        launch["input_closed"]=json!(true);launch["native_ok"]=json!(result.is_ok());launch["reason"]=json!(result.as_ref().err());Ok(())
    })?;
    // From this seal onward the trusted supervisor never creates another child or forwards input.
    let current=record(&install,&r,key)?;
    if let Ok(id)=native(&current["native"]) {
        if coreq::process::identity(id.pid).ok().flatten().as_ref()==Some(&id) {
            membership(&id,&binding)?;super::mac::image(&id,Path::new(current["authority"]["executable"].as_str().ok_or("native image unknown")?))?;
            coreq::process::terminate(&id).map_err(|_|"native stop unconfirmed; scope retained")?;
        }
    }
    loop {
        membership(&supervisor,&binding)?;
        let c=counts(binding["coalition"].as_u64().ok_or("coalition unknown")?)?;
        let done=resources::within(&install,resources::WAIT,|s|certify(s,key,&r,&supervisor,&binding,c))?;
        if done {let _=output.shutdown(Shutdown::Both);break;}
        std::thread::sleep(Duration::from_millis(250));
    }
    // Remain the exact live sentinel until reconcile removes this original one-shot job.
    loop {std::thread::park_timeout(Duration::from_secs(60));}
}

pub(super) fn reconcile(install:&Path,r:&Request,key:&str)->Res<Value> {
    let original=record(install,r,key)?;
    let mut answer=json!({"schema":SCHEMA,"operation":r.slot["binding"]["op_id"],"caller":r.slot["binding"]["caller"],"state":"unknown","launch":original});
    let result=(||->Res<()> {
        if resources::within(install,resources::WAIT,|s|release_ready(s.ops.get(key).ok_or("slot absent")?)).is_ok() {return Ok(());}
        certified(&original)?;
        let supervisor=native(&original["supervisor"])?;
        if original["scope"]["binding"]["boot"]!=crate::platform::boot_time()? {return Err("scope boot changed; use host boot reconciliation".into());}
        if original["scope"]["cleanup_intent"]==false {
            membership(&supervisor,&original["scope"]["binding"])?;
            let cid=original["scope"]["binding"]["coalition"].as_u64().ok_or("coalition unknown")?;
            let c=counts(cid)?;membership(&supervisor,&original["scope"]["binding"])?;
            if json!(c)!=original["scope"]["certificate"]["counts"]||c[0].checked_sub(c[1])!=Some(1) {return Err("native scope changed after certification; retain occupancy".into());}
            let label=original["scope"]["label"].as_str().ok_or("job label unknown")?;
            let domain=original["scope"]["domain"].as_str().ok_or("job domain unknown")?;
            let target=format!("{domain}/{label}");
            let printed=launchctl(&["print",&target])?;
            let program=original["authority"]["adapter"]["executable"].as_str().ok_or("adapter image unknown")?;
            if !printed.lines().any(|l|l.trim()==format!("program = {program}"))||!printed.lines().any(|l|l.trim()==format!("pid = {}",supervisor.pid))
                ||!printed.lines().any(|l|l.trim()==key)||!printed.lines().any(|l|l.trim()=="scope-run") {
                return Err("original job image/argv/PID readback mismatched; no cleanup".into());
            }
            super::mac::image(&supervisor,Path::new(program))?;
            resources::within(install,resources::WAIT,|s| {
                let current=launch_record(s,key,r)?;
                if current!=&original||current["scope"]["cleanup_intent"]!=false {return Err("cleanup already consumed or changed; never retry".into());}
                s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"]["scope"]["cleanup_intent"]=json!(true);Ok(())
            })?;
            launchctl(&["bootout",&target])?;
            resources::within(install,resources::WAIT,|s| {
                let current=launch_record(s,key,r)?;
                if current["scope"]["certificate"]!=original["scope"]["certificate"]||current["scope"]["cleanup_intent"]!=true {return Err("cleanup acknowledgment binding changed".into());}
                s.ops.get_mut(key).unwrap().bound["local_slot"]["launch"]["scope"]["bootout_ack"]=json!(true);Ok(())
            })?;
        }
        let acknowledged=record(install,r,key)?;
        if acknowledged["scope"]["cleanup_intent"]!=true||acknowledged["scope"]["bootout_ack"]!=true {return Err("cleanup outcome unknown; intent retained, never retry bootout".into());}
        let until=Instant::now()+WAIT;
        loop {
            match coreq::process::identity(supervisor.pid) {
                Ok(None)=>break,Ok(Some(id)) if id!=supervisor=>break,
                Ok(_ ) if Instant::now()<until=>std::thread::sleep(Duration::from_millis(10)),
                _=>return Err("scope supervisor exit unknown; cleanup acknowledgment retained".into()),
            }
        }
        resources::within(install,resources::WAIT,|s| {
            let current=launch_record(s,key,r)?;
            if current["scope"]["certificate"]!=original["scope"]["certificate"]||current["scope"]["binding"]!=original["scope"]["binding"]||current["scope"]["bootout_ack"]!=true {return Err("completion binding changed".into());}
            let op=s.ops.get_mut(key).unwrap();op.bound["local_slot"]["launch"]["scope"]["cleanup_verified"]=json!(true);
            release_ready(op)
        })
    })();
    match result {
        Ok(())=>{
            answer["state"]=json!("completed");
            if let Some(path)=original["scope"]["directory"].as_str() {
                let path=Path::new(path);
                if query_identity(path).ok().as_ref()==Some(&original["scope"]["directory_identity"]) {let _=fs::remove_dir_all(path);}
            }
        },
        Err(e)=>answer["reason"]=json!(e),
    }
    answer["launch"]=record(install,r,key)?;Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Chain: R8 R9 R12 R13 #2151 A3/A4/A5 — job registration has no replay/restart trigger or executable chosen by native text.
    // Risk: launchd restarts a consumed generation or a path changes the plist surface.
    #[test]
    fn protected_scope_plan_has_only_one_explicit_dispatch() {
        let plan=plist("invented-label","/invented/adapter & image","slot-invented","/invented/project",&json!({"HOME":"/invented/home"})).unwrap();
        for fact in ["<key>RunAtLoad</key><false/>","<key>KeepAlive</key><false/>","<key>LaunchOnlyOnce</key><true/>","<key>AbandonProcessGroup</key><true/>","<string>scope-run</string>","adapter &amp; image"] {assert!(plan.contains(fact),"{fact}");}
        for forbidden in ["StartInterval","StartCalendarInterval","WatchPaths","QueueDirectories","MachServices","Sockets"] {assert!(!plan.contains(forbidden),"{forbidden}");}
        assert!(plist("invented","invented","invented","invented",&json!({"HOME":null})).is_err());
    }
    // Chain: R8 R9 R12 #2151 A2/A5 — stdio peers and request framing bind native identity, not caller-supplied PID text.
    // Risk: cross-generation connection, incomplete input or buffered bytes become a replayed request.
    #[test]
    fn protected_scope_transport_binds_peer_and_preserves_following_bytes() {
        let (mut writer,reader)=UnixStream::pair().unwrap();socket_deadlines(&reader).unwrap();
        assert_eq!(peer(&reader).unwrap(),identity().unwrap());
        writer.write_all(b"{\"invented\":true}\nnext native input").unwrap();writer.shutdown(Shutdown::Write).unwrap();
        let mut reader=BufReader::new(reader);assert_eq!(line(&mut reader,64).unwrap(),"{\"invented\":true}\n");
        let mut rest=String::new();reader.read_to_string(&mut rest).unwrap();assert_eq!(rest,"next native input");
        let (mut writer,reader)=UnixStream::pair().unwrap();writer.write_all(b"incomplete").unwrap();writer.shutdown(Shutdown::Write).unwrap();
        assert!(line(&mut BufReader::new(reader),32).is_err());
        let (mut writer,reader)=UnixStream::pair().unwrap();writer.write_all(b"oversize\n").unwrap();
        assert!(line(&mut BufReader::new(reader),4).is_err());
    }
}
