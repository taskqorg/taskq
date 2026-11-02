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
            .or_else(||std::env::var_os("HOME").map(|h|Path::new(&h).join(".codex"))).ok_or("Codex App home unavailable")?;
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
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::{fs::PermissionsExt,net::UnixListener as Listener};
    #[cfg(windows)]
    use super::windows_pipe::tests::Listener;
    // Chain: installed desktop -> absent exact owner -> explicit refusal before any interruption.
    #[cfg(any(target_os="macos",windows))]
    #[test]
    #[ignore="read-only installed desktop IPC qualification; no model, queue or existing turn control"]
    fn native_absent_owner_never_sends_interrupt() {
        let thread=super::super::uuid();let turn=super::super::uuid();let began=Instant::now();
        let error=cancel(&thread,&turn).unwrap_err();
        assert!(error.contains("thread-owner-discovery refused: no-client-found"),"{error}");
        assert!(began.elapsed()<Duration::from_secs(11));
        println!("native desktop same-user connection and absent-owner refusal passed; no interrupt or submission: {error}");
    }

    // Chain: untrusted frame sizes -> cumulative wire cap, independent of native transfer speed.
    #[test]
    fn response_budget_counts_prefixes_and_refuses_overflow() {
        let mut received=0;
        for _ in 0..3 {received=frame_end(received,FRAME_LIMIT).unwrap();}
        assert_eq!(frame_end(received,FRAME_LIMIT-16).unwrap(),RESPONSE_LIMIT);
        assert!(frame_end(received,FRAME_LIMIT-12).is_err(),"payload-only accounting would admit an extra prefix");
        for (received,size) in [(0,0),(0,FRAME_LIMIT+1),(RESPONSE_LIMIT,1),(usize::MAX,1)] {
            assert!(frame_end(received,size).is_err());
        }
    }

    // Chain: §8, R2, R9, R11, R12, #2157 A1/A2 -> passive writer lock release verifies completed turn, absent desktop owner, coordination lock, unheld OS lock and unchanged inode; live/owned/active/unknown writers stay untouched.
    #[test]
    fn passive_writer_lock_releases_only_confirmed_idle_unheld_locks() {
        let write_rollout=|home:&Path,project:&Path,thread:&str,state:&str| {
            let sessions=home.join("sessions/2026/11/02");fs::create_dir_all(&sessions).unwrap();
            let rollout=sessions.join(format!("rollout-2026-11-02T00-00-00-{thread}.jsonl"));
            let mut body=format!("{}\n",json!({"timestamp":"2026-11-02T00:00:00Z","type":"session_meta","payload":{"id":thread,"timestamp":"2026-11-02T00:00:00Z","cwd":project,"originator":"taskq-2157-fixture","cli_version":"0.159.3","source":"cli","model_provider":"openai","forked_from_id":if state=="forked" {json!("01a10000-0000-7000-8000-000000000000")} else {Value::Null}}}));
            if state!="empty" {
                body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}})));
                body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:02Z","type":"event_msg","payload":{"type":"user_message","turn_id":"turn-1","message":"fixture prompt"}})));
            }
            match state {
                "completed"=>body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}))),
                "aborted"=>body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"turn-1"}}))),
                "trailing-item"=>{
                    body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}})));
                    body.push_str(&format!("{}\n",json!({"timestamp":"2026-11-02T00:00:04Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"turn-1","item":{"type":"CommandExecution"}}})));
                }
                _=>{}
            }
            fs::write(&rollout,body).unwrap();
        };
        for case in [
            "absent","released-no-ipc","released-aborted","released-no-client-found","released-discovery-timeout",
            "retained-desktop-owner","retained-live-flock","retained-coord-busy","retained-active-turn",
            "retained-empty-rollout","retained-trailing-item","retained-wrong-cwd","retained-forked",
            "retained-ipc-malformed","retained-ipc-refused","retained-lock-symlink","retained-lock-mode","retained-replaced-inode",
        ] {
            let scratch=std::env::temp_dir().join(format!("qpw-{}",super::super::uuid()));
            let home=scratch.join("home");let project=scratch.join("project");
            fs::create_dir_all(&home).unwrap();fs::create_dir_all(&project).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&scratch,fs::Permissions::from_mode(0o700)).unwrap();
            let thread=super::super::uuid();
            let lock_dir=home.join("thread-writer-locks");fs::create_dir_all(&lock_dir).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&lock_dir,fs::Permissions::from_mode(0o700)).unwrap();
            let lock_path=lock_dir.join(format!("{thread}.lock"));
            if case!="absent" {
                fs::write(&lock_path,b"").unwrap();
                #[cfg(unix)]
                fs::set_permissions(&lock_path,fs::Permissions::from_mode(0o600)).unwrap();
            }
            #[cfg(windows)]
            if matches!(case,"retained-lock-symlink"|"retained-lock-mode"|"retained-replaced-inode") {
                fs::remove_dir_all(scratch).unwrap();continue;
            }
            #[cfg(unix)]
            if case=="retained-lock-symlink" {
                let target=scratch.join("target.lock");fs::write(&target,b"").unwrap();
                fs::remove_file(&lock_path).unwrap();
                std::os::unix::fs::symlink(&target,&lock_path).unwrap();
            }
            #[cfg(unix)]
            if case=="retained-lock-mode" {
                fs::set_permissions(&lock_path,fs::Permissions::from_mode(0o666)).unwrap();
            }
            let rollout_state=match case {
                "released-aborted"=>"aborted",
                "retained-active-turn"=>"active",
                "retained-empty-rollout"=>"empty",
                "retained-trailing-item"=>"trailing-item",
                "retained-forked"=>"forked",
                _=>"completed",
            };
            let rollout_project=if case=="retained-wrong-cwd" {&scratch} else {&project};
            write_rollout(&home,rollout_project,&thread,rollout_state);

            let _live_lock=if case=="retained-live-flock" {
                let file=fs::OpenOptions::new().read(true).write(true).open(&lock_path).unwrap();
                Some(coreq::file_lock::try_lock_file(file).unwrap())
            } else {None};
            let _coord_lock=if case=="retained-coord-busy" {
                let coord=fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_dir.join(".coordination.lock")).unwrap();
                #[cfg(unix)]
                fs::set_permissions(lock_dir.join(".coordination.lock"),fs::Permissions::from_mode(0o600)).unwrap();
                Some(coreq::file_lock::try_lock_file(coord).unwrap())
            } else {None};

            #[cfg(unix)]
            let ipc_path=scratch.join("s");
            #[cfg(windows)]
            let ipc_path=std::path::PathBuf::from(format!(r"\\.\pipe\taskq-pw-{}",super::super::uuid()));

            let (finished,completion)=std::sync::mpsc::channel();
            let server=if matches!(case,"released-no-client-found"|"released-discovery-timeout"|"retained-desktop-owner"|"retained-ipc-malformed"|"retained-ipc-refused"|"retained-replaced-inode") {
                let listener=Listener::bind(&ipc_path).unwrap();
                #[cfg(unix)]
                fs::set_permissions(&ipc_path,fs::Permissions::from_mode(0o600)).unwrap();
                let lock_for_race=lock_path.clone();
                let mode=case.to_string();
                let expected_thread=thread.clone();
                Some(std::thread::spawn(move|| ->std::io::Result<()> {
                    let (mut stream,_)=listener.accept()?;
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                    let mut size=[0;4];stream.read_exact(&mut size)?;
                    let mut bytes=vec![0;u32::from_le_bytes(size) as usize];stream.read_exact(&mut bytes)?;
                    let init:Value=serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(init["method"],"initialize");
                    let init_reply=serde_json::to_vec(&json!({"type":"response","requestId":init["requestId"],"method":"initialize","resultType":"success","handledByClientId":"cli-1","result":{"clientId":"cli-1"}})).unwrap();
                    stream.write_all(&(init_reply.len() as u32).to_le_bytes())?;stream.write_all(&init_reply)?;

                    let mut size=[0;4];stream.read_exact(&mut size)?;
                    let mut bytes=vec![0;u32::from_le_bytes(size) as usize];stream.read_exact(&mut bytes)?;
                    let disc:Value=serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(disc["method"],"thread-owner-discovery");
                    assert_eq!(disc["params"]["conversationId"],expected_thread);

                    let ask=serde_json::to_vec(&json!({"type":"client-discovery-request","requestId":"disc-1","request":disc})).unwrap();
                    stream.write_all(&(ask.len() as u32).to_le_bytes())?;stream.write_all(&ask)?;
                    let mut size=[0;4];stream.read_exact(&mut size)?;
                    let mut bytes=vec![0;u32::from_le_bytes(size) as usize];stream.read_exact(&mut bytes)?;
                    let cr:Value=serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(cr["type"],"client-discovery-response");
                    assert_eq!(cr["response"]["canHandle"],false);

                    #[cfg(unix)]
                    if mode=="retained-replaced-inode" {
                        let replacement=lock_for_race.with_extension("replacement");
                        fs::write(&replacement,b"new").unwrap();
                        fs::set_permissions(&replacement,fs::Permissions::from_mode(0o600)).unwrap();
                        fs::remove_file(&lock_for_race).unwrap();
                        fs::rename(&replacement,&lock_for_race).unwrap();
                    }

                    match mode.as_str() {
                        "released-no-client-found"|"retained-replaced-inode"=>{
                            let reply=serde_json::to_vec(&json!({"type":"response","requestId":disc["requestId"],"method":"thread-owner-discovery","resultType":"error","error":"no-client-found"})).unwrap();
                            stream.write_all(&(reply.len() as u32).to_le_bytes())?;stream.write_all(&reply)?;
                        }
                        "released-discovery-timeout"=>{
                            let _=completion.recv_timeout(Duration::from_secs(5));
                            return Ok(());
                        }
                        "retained-desktop-owner"=>{
                            let reply=serde_json::to_vec(&json!({"type":"response","requestId":disc["requestId"],"method":"thread-owner-discovery","resultType":"success","handledByClientId":"desktop-owner","result":{}})).unwrap();
                            stream.write_all(&(reply.len() as u32).to_le_bytes())?;stream.write_all(&reply)?;
                        }
                        "retained-ipc-refused"=>{
                            let reply=serde_json::to_vec(&json!({"type":"response","requestId":disc["requestId"],"method":"thread-owner-discovery","resultType":"error","error":"request-version-mismatch"})).unwrap();
                            stream.write_all(&(reply.len() as u32).to_le_bytes())?;stream.write_all(&reply)?;
                        }
                        "retained-ipc-malformed"=>{
                            stream.write_all(&[1,0,0,0,b'{'])?;return Ok(());
                        }
                        _=>{}
                    }
                    let _=stream.read(&mut [0]);
                    Ok(())
                }))
            } else {None};

            let outcome=release_passive_writer_at(&home,&project,&thread,&ipc_path,Instant::now()+Duration::from_millis(200));
            let _=finished.send(());
            if let Some(server)=server {let _=server.join().unwrap();}
            match case {
                "absent"=>assert_eq!(outcome,PassiveWriterOutcome::Absent,"{case}"),
                c if c.starts_with("released-")=>{
                    assert_eq!(outcome,PassiveWriterOutcome::Released,"{case}: {outcome:?}");
                    assert!(!lock_path.exists(),"{case}: passive lock file should be removed");
                }
                _=>{
                    assert!(matches!(outcome,PassiveWriterOutcome::Retained(_)),"{case}: expected Retained, got {outcome:?}");
                    assert!(fs::symlink_metadata(&lock_path).is_ok(),"{case}: active/owned/unknown lock must stay untouched");
                }
            }
            drop(_live_lock);drop(_coord_lock);
            fs::remove_dir_all(scratch).unwrap();
        }
    }

    // Chain: exact input -> current owner -> addressed interrupt -> validated receipt or explicit unknown.
    // Risk: stale/foreign responses, partial I/O or a silent peer release or stall an execution.
    #[test]
    fn cancellation_is_owner_bound_bounded_and_never_replays_work() {
        for case in ["ok","client","owner","turn","null","method","refused","oversize","broadcast","malformed","drop","timeout","timeout-before-init","unrelated","directory-mode","socket-mode","symlink"] {
            let root=std::env::temp_dir().join(format!("qipc-{}",super::super::uuid()));fs::create_dir(&root).unwrap();
            #[cfg(unix)]
            let path=root.join("s");
            #[cfg(windows)]
            let path=std::path::PathBuf::from(format!(r"\\.\pipe\taskq-ipc-{}",super::super::uuid()));
            #[cfg(windows)]
            if matches!(case,"directory-mode"|"socket-mode"|"symlink") {fs::remove_dir_all(root).unwrap();continue;}
            let listener=Listener::bind(&path).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap();
            #[cfg(unix)]
            if matches!(case,"directory-mode"|"socket-mode"|"symlink") {
                let checked=if case=="symlink" {
                    let link=root.join("link");std::os::unix::fs::symlink(&path,&link).unwrap();link
                } else {
                    fs::set_permissions(if case=="directory-mode" {&root} else {&path},fs::Permissions::from_mode(0o777)).unwrap();path.clone()
                };
                assert!(cancel_at(&checked,"thread","turn",Instant::now()+Duration::from_secs(1)).unwrap_err().contains("ownership/protection"));
                listener.set_nonblocking(true).unwrap();
                assert_eq!(listener.accept().unwrap_err().kind(),std::io::ErrorKind::WouldBlock,"untrusted endpoint must not receive a request");
                fs::remove_dir_all(root).unwrap();continue;
            }
            let (finished,completion)=std::sync::mpsc::channel();
            let server=std::thread::spawn(move|| ->std::io::Result<()> {
                let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                if case=="timeout-before-init" {completion.recv_timeout(Duration::from_secs(5)).unwrap();}
                for method in ["initialize","thread-owner-discovery","thread-follower-interrupt-turn"] {
                    let mut size=[0;4];stream.read_exact(&mut size)?;let size=u32::from_le_bytes(size) as usize;assert!(size<=4096);
                    let mut bytes=vec![0;size];stream.read_exact(&mut bytes)?;let request:Value=serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(request["method"],method,"cancellation never queues or resumes a turn");
                    let mut result=match method {
                        "initialize"=>json!({"clientId":"client"}),
                        "thread-owner-discovery"=>{assert_eq!(request["params"]["conversationId"],"thread");json!({})},
                        _=>{assert_eq!(request["targetClientId"],"owner");assert_eq!(request["version"],4);assert_eq!(request["params"],json!({"conversationId":"thread","expectedTurnId":"turn","mode":"system"}));json!({"ok":true,"interruptedTurnId":"turn"})},
                    };
                    if method=="initialize" && case=="client" {result["clientId"]="wrong".into();}
                    let control=method=="thread-follower-interrupt-turn";
                    let mut reply=json!({"type":"response","requestId":request["requestId"],"method":method,"resultType":"success","handledByClientId":if method=="initialize" {"client"} else {"owner"},"result":result});
                    if control {
                        match case {
                            "owner"=>reply["handledByClientId"]="foreign".into(),
                            "turn"=>reply["result"]["interruptedTurnId"]="foreign".into(),
                            "null"=>reply["result"]["interruptedTurnId"]=Value::Null,
                            "method"=>reply["method"]="foreign".into(),
                            "refused"=>{reply["resultType"]="error".into();reply["error"]="request-version-mismatch".into();},
                            "oversize"=>{stream.write_all(&(1024u32*1024+1).to_le_bytes())?;return Ok(());},
                            "broadcast"=>{
                                let body=serde_json::to_vec(&json!({"type":"broadcast","event":"changed"})).unwrap();
                                stream.write_all(&(body.len() as u32).to_le_bytes())?;stream.write_all(&body)?;
                            },
                            "malformed"=>{stream.write_all(&[1,0,0,0,b'{'])?;return Ok(());},
                            "drop"=>return Ok(()),
                            "timeout"=>{let _=completion.recv_timeout(Duration::from_secs(5));return Ok(());},
                            "unrelated"=>reply["requestId"]="another-request".into(),
                            _=>{},
                        }
                    }
                    let body=serde_json::to_vec(&reply).unwrap();stream.write_all(&(body.len() as u32).to_le_bytes())?;
                    // Fragmentation cannot turn one frame into another response.
                    for chunk in body.chunks(7) {stream.write_all(chunk)?;}
                    if case=="client" {return Ok(());}
                }
                let _=stream.read(&mut [0]);
                Ok(())
            });
            let began=Instant::now();let budget=if case.starts_with("timeout") {Duration::from_millis(150)} else {Duration::from_secs(3)};
            let outcome=cancel_at(&path,"thread","turn",began+budget);
            let _=finished.send(());
            assert_eq!(outcome.is_ok(),matches!(case,"ok"|"broadcast"),"{case}: {outcome:?}");assert!(began.elapsed()<budget+Duration::from_secs(1));
            if case=="refused" {assert!(outcome.as_ref().unwrap_err().contains("request-version-mismatch"));}
            if let Err(e)=server.join().unwrap() {
                // A deadline may close the peer during any fixture I/O, before its interrupt request.
                assert!(case.starts_with("timeout") && matches!(e.kind(),std::io::ErrorKind::BrokenPipe|std::io::ErrorKind::ConnectionReset|std::io::ErrorKind::UnexpectedEof|std::io::ErrorKind::TimedOut),"{case}: fixture I/O: {e}");
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}
