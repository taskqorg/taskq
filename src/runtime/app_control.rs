//! Parent desktop IPC; exact-turn cancellation never establishes native exit.
use std::{fs,io::{Read,Write},path::Path,time::{Duration,Instant}};
#[cfg(unix)]
use std::os::unix::{fs::{FileTypeExt,MetadataExt},net::UnixStream as Stream};
#[cfg(target_os="macos")]
use std::path::PathBuf;
#[cfg(windows)]
mod windows_pipe;
#[cfg(windows)]
use windows_pipe::Stream;
use serde_json::{Value,json};

#[derive(Debug,Clone,PartialEq,Eq)]
pub(crate) enum PassiveWriterOutcome {
    Absent,
    Released,
    Retained(String),
}

#[cfg(target_os="macos")]
pub(super) fn cancel(thread:&str,turn:&str)->Result<(),String> {
    coreq::telemetry::capture::operation("runtime.app-cancel",|| {
        let home=std::env::var_os("CODEX_HOME").map(PathBuf::from)
            .or_else(||crate::platform::user_home().map(|h|h.join(".codex"))).ok_or("Codex App home unavailable")?;
        cancel_at(&home.join("ipc/ipc.sock"),thread,turn,super::observation_deadline(Duration::from_secs(10)))
    }, |_|"app-cancel")
}

#[cfg(windows)]
pub(super) fn cancel(thread:&str,turn:&str)->Result<(),String> {
    coreq::telemetry::capture::operation("runtime.app-cancel",|| {
        cancel_at(Path::new(r"\\.\pipe\codex-ipc"),thread,turn,super::observation_deadline(Duration::from_secs(10)))
    }, |_|"app-cancel")
}

pub(super) fn release_passive_writer(home:&Path,root:&Path,thread:&str)->PassiveWriterOutcome {
    let until=super::observation_deadline(Duration::from_millis(250));
    #[cfg(windows)]
    return release_passive_writer_at(home,root,thread,Path::new(r"\\.\pipe\codex-ipc"),until);
    #[cfg(unix)]
    release_passive_writer_at(home,root,thread,&home.join("ipc/ipc.sock"),until)
}

pub(super) fn release_passive_writer_at(home:&Path,root:&Path,thread:&str,ipc_path:&Path,until:Instant)->PassiveWriterOutcome {
    if !id(thread) {return PassiveWriterOutcome::Retained("Codex thread identity invalid".into());}
    let lock_dir=home.join("thread-writer-locks");
    let lock_path=lock_dir.join(format!("{thread}.lock"));
    let initial_id=match path_identity(&lock_dir,&lock_path) {
        Ok(None)=>return PassiveWriterOutcome::Absent,
        Ok(Some(id))=>id,
        Err(reason)=>return PassiveWriterOutcome::Retained(reason),
    };
    let (initial_anchor,anchor_id)=match open_lock_file(&lock_path) {
        Ok(pair)=>pair,
        Err(reason)=>return PassiveWriterOutcome::Retained(reason),
    };
    if anchor_id!=initial_id {
        return PassiveWriterOutcome::Retained("Codex writer lock handle differs from path identity".into());
    }
    if let Err(reason)=super::rollout_completed_idle(home,root,thread) {
        return PassiveWriterOutcome::Retained(reason);
    }
    match discover_owner_at(ipc_path,thread,until) {
        Ok(Some(owner))=>return PassiveWriterOutcome::Retained(format!("Codex App owner {owner} still claims thread {thread}")),
        Ok(None)=>{},
        Err(reason)=>return PassiveWriterOutcome::Retained(reason),
    }
    let coord_path=lock_dir.join(".coordination.lock");
    let coord_deadline=Instant::now()+Duration::from_millis(100);
    let _coord_lock=loop {
        let coord_file=match open_coordination_file(&lock_dir,&coord_path) {
            Ok(file)=>file,
            Err(reason)=>return PassiveWriterOutcome::Retained(reason),
        };
        match coreq::file_lock::try_lock_file(coord_file) {
            Ok(lock)=>break lock,
            Err(_) if Instant::now()<coord_deadline=>std::thread::sleep(Duration::from_millis(5)),
            Err(_)=>return PassiveWriterOutcome::Retained("Codex writer coordination lock busy".into()),
        }
    };
    let before_open=match path_identity(&lock_dir,&lock_path) {
        Ok(None)=>return PassiveWriterOutcome::Absent,
        Ok(Some(id)) if id==initial_id=>id,
        Ok(Some(_))=>return PassiveWriterOutcome::Retained("Codex writer lock file changed before lock check".into()),
        Err(reason)=>return PassiveWriterOutcome::Retained(reason),
    };
    let (lock_file,open_id)=match open_lock_file(&lock_path) {
        Ok(pair)=>pair,
        Err(reason)=>return PassiveWriterOutcome::Retained(reason),
    };
    if open_id!=before_open {
        return PassiveWriterOutcome::Retained("Codex writer lock handle differs from path identity".into());
    }
    let held_lock=match coreq::file_lock::try_lock_file(lock_file) {
        Ok(lock)=>lock,
        Err(_)=>return PassiveWriterOutcome::Retained("Codex writer lock is actively held by a live process".into()),
    };
    match path_identity(&lock_dir,&lock_path) {
        Ok(Some(after_id)) if after_id==initial_id=>{},
        Ok(Some(_))=>return PassiveWriterOutcome::Retained("Codex writer lock file replaced during lock acquisition".into()),
        Ok(None)=>return PassiveWriterOutcome::Absent,
        Err(reason)=>return PassiveWriterOutcome::Retained(reason),
    }
    if let Err(reason)=super::rollout_completed_idle(home,root,thread) {
        return PassiveWriterOutcome::Retained(reason);
    }
    drop(initial_anchor);
    #[cfg(windows)]
    drop(held_lock);
    if let Err(e)=fs::remove_file(&lock_path) {
        return PassiveWriterOutcome::Retained(format!("Codex passive writer lock removal failed: {e}"));
    }
    #[cfg(unix)]
    drop(held_lock);
    PassiveWriterOutcome::Released
}

fn discover_owner_at(path:&Path,thread:&str,until:Instant)->Result<Option<String>,String> {
    if !id(thread) {return Err("Codex App thread identity invalid".into());}
    #[cfg(unix)]
    {
        let uid=unsafe {libc::geteuid()};
        if let Some(parent)=path.parent() {
            match fs::symlink_metadata(parent) {
                Ok(meta) if meta.uid()!=uid || meta.mode()&0o022!=0 || !meta.is_dir()=>return Err("Codex App IPC ownership/protection invalid".into()),
                Ok(_)=>{},
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
                Err(_)=>return Err("Codex App IPC unavailable".into()),
            }
        }
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.uid()!=uid || meta.mode()&0o022!=0 || !meta.file_type().is_socket()=>return Err("Codex App IPC ownership/protection invalid".into()),
            Ok(_)=>{},
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
            Err(_)=>return Err("Codex App IPC unavailable".into()),
        }
    }
    #[cfg(unix)]
    let stream=match coreq::local_socket::connect(path,until) {
        Ok(stream)=>stream,
        Err(e) if e=="local socket unavailable"=>return Ok(None),
        Err(e)=>return Err(e),
    };
    #[cfg(windows)]
    let stream=match Stream::connect(path,until) {
        Ok(stream)=>stream,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(format!("Codex App IPC connection unknown: {e}")),
    };
    let mut ipc=Ipc {stream,until,received:0,client:"initializing-client".into()};
    let init=ipc.request("initialize",json!({"clientType":"cli"}),0,None)?;
    let client=init["result"]["clientId"].as_str().filter(|s|id(s)).ok_or("Codex App IPC client identity missing")?;
    if init["handledByClientId"]!=client {return Err("Codex App IPC client identity mismatch".into());}
    ipc.client=client.into();
    let request=super::uuid();
    ipc.send(&json!({"type":"request","requestId":request,"sourceClientId":ipc.client,"version":1,"method":"thread-owner-discovery","params":{"hostId":"local","conversationId":thread},"timeoutMs":250}))?;
    loop {
        if ipc.received>RESPONSE_LIMIT-4 {return Err("Codex App IPC response budget exceeded".into());}
        let Some(size)=ipc.read_prefix()? else {return Ok(None);};
        ipc.received=frame_end(ipc.received,size)?;
        let mut body=vec![0;size];ipc.read(&mut body)?;
        let reply:Value=serde_json::from_slice(&body).map_err(|_|"Codex App IPC malformed response")?;
        if reply["type"]=="client-discovery-request" {
            let rid=reply["requestId"].as_str().filter(|s|id(s)).ok_or("Codex App IPC discovery identity invalid")?;
            ipc.send(&json!({"type":"client-discovery-response","requestId":rid,"response":{"canHandle":false}}))?;
        }
        if reply["type"]!="response" || reply["requestId"]!=request {continue;}
        if reply["resultType"]!="success" {
            if reply["error"].as_str()==Some("no-client-found") {return Ok(None);}
            let reason:String=reply["error"].as_str().unwrap_or("native error without message").chars()
                .map(|c|if c.is_control() {' '} else {c}).take(160).collect();
            return Err(format!("Codex App IPC thread-owner-discovery refused: {reason}"));
        }
        if reply["method"]!="thread-owner-discovery" {return Err("Codex App IPC response method mismatch".into());}
        let owner=reply["handledByClientId"].as_str().filter(|s|id(s)&&*s!=ipc.client).ok_or("Codex App owner identity invalid")?;
        return Ok(Some(owner.into()));
    }
}

#[cfg(unix)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct LockIdentity(u64,u64);

#[cfg(unix)]
fn path_identity(lock_dir:&Path,lock_path:&Path)->Result<Option<LockIdentity>,String> {
    let uid=unsafe {libc::geteuid()};
    let dir_meta=match fs::symlink_metadata(lock_dir) {
        Ok(meta)=>meta,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(format!("Codex writer lock directory unreadable: {e}")),
    };
    if dir_meta.uid()!=uid || dir_meta.mode()&0o022!=0 || !dir_meta.file_type().is_dir() {
        return Err("Codex writer lock directory ownership/protection invalid".into());
    }
    let meta=match fs::symlink_metadata(lock_path) {
        Ok(meta)=>meta,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(format!("Codex writer lock unreadable: {e}")),
    };
    if meta.uid()!=uid || meta.mode()&0o022!=0 || !meta.file_type().is_file() {
        return Err("Codex writer lock ownership/protection invalid".into());
    }
    Ok(Some(LockIdentity(meta.dev(),meta.ino())))
}

#[cfg(unix)]
fn open_coordination_file(lock_dir:&Path,coord_path:&Path)->Result<fs::File,String> {
    use std::os::unix::fs::OpenOptionsExt;
    let uid=unsafe {libc::geteuid()};
    let dir_meta=fs::symlink_metadata(lock_dir).map_err(|e|format!("Codex writer lock directory unreadable: {e}"))?;
    if dir_meta.uid()!=uid || dir_meta.mode()&0o022!=0 || !dir_meta.file_type().is_dir() {
        return Err("Codex writer lock directory ownership/protection invalid".into());
    }
    let file=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW).open(coord_path)
        .map_err(|e|format!("Codex writer coordination lock open failed: {e}"))?;
    let meta=file.metadata().map_err(|e|format!("Codex writer coordination lock metadata unreadable: {e}"))?;
    let path_meta=fs::symlink_metadata(coord_path).map_err(|e|format!("Codex writer coordination lock path unreadable: {e}"))?;
    if meta.uid()!=uid || meta.mode()&0o022!=0 || !meta.file_type().is_file()
        || (meta.dev(),meta.ino())!=(path_meta.dev(),path_meta.ino()) {
        return Err("Codex writer coordination lock ownership/protection invalid".into());
    }
    Ok(file)
}

#[cfg(unix)]
fn open_lock_file(lock_path:&Path)->Result<(fs::File,LockIdentity),String> {
    use std::os::unix::fs::OpenOptionsExt;
    let uid=unsafe {libc::geteuid()};
    let file=fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_NOFOLLOW).open(lock_path)
        .map_err(|e|format!("Codex writer lock open failed: {e}"))?;
    let meta=file.metadata().map_err(|e|format!("Codex writer lock handle metadata unreadable: {e}"))?;
    if meta.uid()!=uid || meta.mode()&0o022!=0 || !meta.file_type().is_file() {
        return Err("Codex writer lock ownership/protection invalid".into());
    }
    Ok((file,LockIdentity(meta.dev(),meta.ino())))
}

#[cfg(windows)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct LockIdentity(u64,[u8;16]);

#[cfg(windows)]
fn handle_file_id(file:&fs::File)->Result<LockIdentity,String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE,Storage::FileSystem::{FILE_ID_INFO,FileIdInfo,GetFileInformationByHandleEx}};
    let meta=file.metadata().map_err(|e|format!("Codex writer lock metadata unreadable: {e}"))?;
    if !meta.is_file() {return Err("Codex writer lock is not a regular file".into());}
    let mut id=FILE_ID_INFO::default();
    unsafe {GetFileInformationByHandleEx(HANDLE(file.as_raw_handle()),FileIdInfo,
        (&mut id as *mut FILE_ID_INFO).cast(),std::mem::size_of::<FILE_ID_INFO>() as u32)}
        .map_err(|e|format!("Codex writer lock identity unavailable: {e}"))?;
    Ok(LockIdentity(id.VolumeSerialNumber,id.FileId.Identifier))
}

#[cfg(windows)]
fn path_identity(lock_dir:&Path,lock_path:&Path)->Result<Option<LockIdentity>,String> {
    match fs::symlink_metadata(lock_dir) {
        Ok(meta) if meta.file_type().is_dir()=>{},
        Ok(_)=>return Err("Codex writer lock directory ownership/protection invalid".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(format!("Codex writer lock directory unreadable: {e}")),
    }
    match fs::symlink_metadata(lock_path) {
        Ok(meta) if meta.file_type().is_file()=>{},
        Ok(_)=>return Err("Codex writer lock ownership/protection invalid".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(format!("Codex writer lock unreadable: {e}")),
    }
    let file=fs::OpenOptions::new().read(true).open(lock_path).map_err(|e|format!("Codex writer lock open failed: {e}"))?;
    handle_file_id(&file).map(Some)
}

#[cfg(windows)]
fn open_coordination_file(lock_dir:&Path,coord_path:&Path)->Result<fs::File,String> {
    if !fs::symlink_metadata(lock_dir).map_err(|e|format!("Codex writer lock directory unreadable: {e}"))?.file_type().is_dir() {
        return Err("Codex writer lock directory ownership/protection invalid".into());
    }
    if let Ok(meta)=fs::symlink_metadata(coord_path) {
        if !meta.file_type().is_file() {return Err("Codex writer coordination lock ownership/protection invalid".into());}
    }
    let file=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(coord_path)
        .map_err(|e|format!("Codex writer coordination lock open failed: {e}"))?;
    let _=handle_file_id(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn open_lock_file(lock_path:&Path)->Result<(fs::File,LockIdentity),String> {
    if !fs::symlink_metadata(lock_path).map_err(|e|format!("Codex writer lock unreadable: {e}"))?.file_type().is_file() {
        return Err("Codex writer lock ownership/protection invalid".into());
    }
    let file=fs::OpenOptions::new().read(true).write(true).open(lock_path)
        .map_err(|e|format!("Codex writer lock open failed: {e}"))?;
    let id=handle_file_id(&file)?;
    Ok((file,id))
}

#[cfg(any(target_os="macos",windows,test))]
fn cancel_at(path:&Path,thread:&str,turn:&str,until:Instant)->Result<(),String> {
    if !id(thread)||!id(turn) {return Err("Codex App cancellation identity invalid".into());}
    #[cfg(unix)]
    {
    let parent=path.parent().ok_or("Codex App IPC parent missing")?;
    let uid=unsafe {libc::geteuid()};
    for (p,directory) in [(parent,true),(path,false)] {
        let meta=fs::symlink_metadata(p).map_err(|_|"Codex App IPC unavailable")?;
        if meta.uid()!=uid || meta.mode()&0o022!=0 || if directory {!meta.is_dir()} else {!meta.file_type().is_socket()} {
            return Err("Codex App IPC ownership/protection invalid".into());
        }
    }
    }
    #[cfg(unix)]
    let stream=coreq::local_socket::connect(path,until)?;
    #[cfg(windows)]
    let stream=Stream::connect(path,until).map_err(|e|format!("Codex App IPC connection unknown: {e}"))?;
    let mut ipc=Ipc {stream,until,received:0,client:"initializing-client".into()};
    let init=ipc.request("initialize",json!({"clientType":"cli"}),0,None)?;
    let client=init["result"]["clientId"].as_str().filter(|s|id(s)).ok_or("Codex App IPC client identity missing")?;
    if init["handledByClientId"]!=client {return Err("Codex App IPC client identity mismatch".into());}
    ipc.client=client.into();
    let found=ipc.request("thread-owner-discovery",json!({"hostId":"local","conversationId":thread}),1,None)?;
    let owner=found["handledByClientId"].as_str().filter(|s|id(s)&&*s!=ipc.client).ok_or("Codex App owner unavailable")?;
    let stopped=ipc.request("thread-follower-interrupt-turn",json!({"conversationId":thread,"mode":"system","expectedTurnId":turn}),4,Some(owner))?;
    if stopped["handledByClientId"]!=owner || stopped["result"]["ok"]!=true || stopped["result"]["interruptedTurnId"]!=turn {
        return Err("Codex App exact-turn cancellation unconfirmed; retain execution and reconcile native history".into());
    }
    Ok(())
}
fn id(s:&str)->bool { !s.is_empty() && s.len()<=128 && s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-') }

const RESPONSE_LIMIT:usize=4*1024*1024;
const FRAME_LIMIT:usize=1024*1024;
fn frame_end(received:usize,size:usize)->Result<usize,String> {
    received.checked_add(4).and_then(|n|n.checked_add(size))
        .filter(|end|size>0 && size<=FRAME_LIMIT && *end<=RESPONSE_LIMIT)
        .ok_or_else(||"Codex App IPC response budget exceeded".into())
}

struct Ipc {stream:Stream,until:Instant,received:usize,client:String}
impl Ipc {
    fn left(&self)->Result<Duration,String> {
        let left=self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {Err("Codex App IPC deadline; cancellation outcome unknown".into())} else {Ok(left)}
    }
    fn read_prefix(&mut self)->Result<Option<usize>,String> {
        let mut prefix=[0u8;4];let mut offset=0;
        while offset<4 {
            let left=self.until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return if offset==0 {Ok(None)} else {Err("Codex App IPC partial frame prefix before deadline".into())};
            }
            self.stream.set_read_timeout(Some(left)).map_err(|_|"Codex App IPC read timeout unavailable")?;
            match self.stream.read(&mut prefix[offset..]) {
                Ok(0)=>return Err("Codex App IPC disconnected; cancellation outcome unknown".into()),
                Ok(n)=>offset+=n,
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>{},
                Err(e) if offset==0 && matches!(e.kind(),std::io::ErrorKind::TimedOut|std::io::ErrorKind::WouldBlock)=>return Ok(None),
                Err(e)=>return Err(format!("Codex App IPC read failed: {e}; {} bytes pending; cancellation outcome unknown",4-offset)),
            }
        }
        Ok(Some(u32::from_le_bytes(prefix) as usize))
    }
    fn read(&mut self,mut bytes:&mut [u8])->Result<(),String> {
        while !bytes.is_empty() {
            self.stream.set_read_timeout(Some(self.left()?)).map_err(|_|"Codex App IPC read timeout unavailable")?;
            match self.stream.read(bytes) {
                Ok(0)=>return Err("Codex App IPC disconnected; cancellation outcome unknown".into()),
                Ok(n)=>bytes=&mut bytes[n..],
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>{},
                Err(e)=>return Err(format!("Codex App IPC read failed: {e}; {} bytes pending; cancellation outcome unknown",bytes.len())),
            }
        } Ok(())
    }
    fn send(&mut self,value:&Value)->Result<(),String> {
        let body=serde_json::to_vec(value).map_err(|_|"Codex App IPC request invalid")?;
        if body.len()>4096 {return Err("Codex App IPC request too large".into());}
        let mut wire=(body.len() as u32).to_le_bytes().to_vec();wire.extend(body);let mut bytes=wire.as_slice();
        while !bytes.is_empty() {
            self.stream.set_write_timeout(Some(self.left()?)).map_err(|_|"Codex App IPC write timeout unavailable")?;
            match self.stream.write(bytes) {
                Ok(0)=>return Err("Codex App IPC write incomplete; cancellation outcome unknown".into()),
                Ok(n)=>bytes=&bytes[n..],
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted=>{},
                Err(e)=>return Err(format!("Codex App IPC write failed: {e}; cancellation outcome unknown")),
            }
        } Ok(())
    }
    fn request(&mut self,method:&str,params:Value,version:u32,target:Option<&str>)->Result<Value,String> {
        let request=super::uuid();let mut value=json!({"type":"request","requestId":request,"sourceClientId":self.client,"version":version,"method":method,"params":params,"timeoutMs":4000});
        if let Some(target)=target {value["targetClientId"]=target.into();}
        self.send(&value)?;
        loop {
            if self.received>RESPONSE_LIMIT-4 {return Err("Codex App IPC response budget exceeded".into());}
            let mut prefix=[0;4];self.read(&mut prefix)?;let size=u32::from_le_bytes(prefix) as usize;
            self.received=frame_end(self.received,size)?;
            let mut body=vec![0;size];self.read(&mut body)?;
            let reply:Value=serde_json::from_slice(&body).map_err(|_|"Codex App IPC malformed response")?;
            if reply["type"]=="client-discovery-request" {
                let rid=reply["requestId"].as_str().filter(|s|id(s)).ok_or("Codex App IPC discovery identity invalid")?;
                self.send(&json!({"type":"client-discovery-response","requestId":rid,"response":{"canHandle":false}}))?;
            }
            if reply["type"]!="response" || reply["requestId"]!=request {continue;}
            if reply["resultType"]!="success" {
                let reason:String=reply["error"].as_str().unwrap_or("native error without message").chars()
                    .map(|c|if c.is_control() {' '} else {c}).take(160).collect();
                return Err(format!("Codex App IPC {method} refused: {reason}; retain execution and reconcile native history"));
            }
            if reply["method"]!=method {return Err("Codex App IPC response method mismatch".into());}
            return Ok(reply);
        }
    }
}

#[cfg(test)]
#[path = "app_control_tests.rs"]
mod tests;
