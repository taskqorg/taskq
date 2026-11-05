//! Focus correlation only; settings and admission policy remain authoritative.
use crate::{Res,data,dot};
use coreq::telemetry::capture::{CommandReceipt,operation};
use serde_json::{Value,json};
use std::{fs,io::Read,path::Path};

pub struct Pending {
    project: String,
    receipt: CommandReceipt,
    phase: String,
    focus: Option<String>,
    previous: Option<CommandReceipt>,
}

fn descriptor(value: &Value) -> Res<CommandReceipt> {
    let text=value.as_str().ok_or("focus command descriptor invalid; file kept")?;
    let receipt=CommandReceipt::decode(text)?;
    let metadata:Value=serde_json::from_str(text).map_err(|_|"focus command descriptor invalid; file kept")?;
    if metadata["product"]!="taskq" {return Err("focus command product mismatch; file kept".into());}
    Ok(receipt)
}

fn staged(root: &Path) -> Res<bool> {
    match fs::symlink_metadata(dot(root).join("focus.command.pending")) {
        Ok(_)=>Ok(true),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(false),
        Err(_)=>Err("focus command temporary unreadable; file kept".into()),
    }
}

fn read(root: &Path) -> Res<Option<Pending>> {
    if staged(root)? {return Err("focus command temporary retained; interrupted write needs reconciliation".into());}
    read_file(root,&dot(root).join("focus.command"))
}

fn read_file(root: &Path, path: &Path) -> Res<Option<Pending>> {
    operation("command.state.read",|| {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_file()=>{},
            Ok(_)=>return Err("focus command not a regular file; kept".into()),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
            Err(_)=>return Err("focus command unreadable; file kept".into()),
        }
        let file=match fs::File::open(path) {
            Ok(f)=>f,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
            Err(_)=>return Err("focus command unreadable; file kept".into()),
        };
        let mut text=String::new();file.take(4097).read_to_string(&mut text).map_err(|_|"focus command unreadable; file kept")?;
        if text.len()>4096 {return Err("focus command oversized; file kept".into());}
        let v:Value=serde_json::from_str(&text).map_err(|_|"focus command corrupt; file kept")?;
        let object=v.as_object().ok_or("focus command invalid; file kept")?;
        if object.len()!=6 || !["schema","project","command","phase","focus","previous"].iter().all(|k|object.contains_key(*k)) || v["schema"]!=1 {
            return Err("focus command schema unsupported; file kept".into());
        }
        let project=project_id(root)?;
        if v["project"]!=project {return Err("focus command belongs to another project; file kept".into());}
        let receipt=descriptor(&v["command"])?;
        let phase=v["phase"].as_str().filter(|s|matches!(*s,"pending"|"applied"|"failed")).ok_or("focus command phase invalid; file kept")?.to_owned();
        let focus=if phase=="applied" {
            Some(v["focus"].as_str().filter(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)))
                .ok_or("focus command fingerprint invalid; file kept")?.to_owned())
        } else {if !v["focus"].is_null() {return Err("focus command fingerprint invalid; file kept".into());}None};
        let previous=if v["previous"].is_null() {None} else {Some(descriptor(&v["previous"])?)};
        if phase!="pending"&&previous.is_some() {return Err("focus command terminal retains predecessor; file kept".into());}
        if previous.as_ref().is_some_and(|p|p.event_id("received").ok()==receipt.event_id("received").ok()) {
            return Err("focus command predecessor repeats current identity; file kept".into());
        }
        Ok(Some(Pending{project,receipt,phase,focus,previous}))
    },|_|"command-state-read")
}

/// Exclusive signal owner only: reconcile metadata, never replay a setting mutation.
fn recover(root: &Path) -> Res<()> {
    operation("command.recover",|| {
        let path=dot(root).join("focus.command.pending");
        let Some(next)=read_file(root,&path)? else {return Ok(())};
        let current=read_file(root,&dot(root).join("focus.command"))?;
        let same=current.as_ref().is_some_and(|p|p.receipt.encode()==next.receipt.encode());
        if next.phase=="pending" {
            let previous=current.as_ref().filter(|p|p.phase!="failed").map(|p|p.receipt.encode());
            if same || next.previous.as_ref().map(CommandReceipt::encode)!=previous {
                return Err("focus command temporary predecessor mismatch; files kept".into());
            }
            // The initial pointer never replaced current, so this writer could not enter its effect closure.
            crate::telemetry::receipt(&next.receipt,"failed")?;
        } else {
            let old=current.as_ref().filter(|_|same).ok_or("focus command temporary identity mismatch; files kept")?;
            if old.phase!="pending" && (old.phase!=next.phase || old.focus!=next.focus) {
                return Err("focus command temporary terminal mismatch; files kept".into());
            }
            if next.phase=="applied" && next.focus.as_deref()!=Some(fingerprint(&data::focus(root)?).as_str()) {
                return Err("focus command temporary differs from settings; files kept".into());
            }
        }
        // Only a validated, attributable temporary may be removed. Canonical pending stays safe if re-save fails.
        fs::remove_file(&path).map_err(|_|"focus command temporary removal failed; reconciliation unknown")?;
        if next.phase!="pending" {save(root,&next)?;}
        Ok(())
    },|_|"command-recover")
}

fn save(root: &Path, pending: &Pending) -> Res<()> {
    operation("command.state.write",|| {
        let text=json!({"schema":1,"project":pending.project,"command":pending.receipt.encode(),"phase":pending.phase,
            "focus":pending.focus,"previous":pending.previous.as_ref().map(CommandReceipt::encode)}).to_string();
        if text.len()>4096 {return Err("focus command oversized".into());}
        coreq::write_durable(&dot(root).join("focus.command"),&text)
    },|_|"command-state-write")
}

fn fingerprint(focus: &[String]) -> String {
    let bytes=serde_json::to_vec(focus).expect("string vector");
    digest(&bytes)
}

fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()
}

fn project_id(root: &Path) -> Res<String> {
    let root=root.canonicalize().map_err(|_|"focus command project unavailable; file kept")?;
    #[cfg(unix)] {
        use std::os::unix::ffi::OsStrExt;
        Ok(digest(root.as_os_str().as_bytes()))
    }
    #[cfg(windows)] {
        use std::os::windows::ffi::OsStrExt;
        let bytes:Vec<_>=root.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect();
        Ok(digest(&bytes))
    }
}

/// Caller owns the exclusive signal lock; no setting may change before this returns.
pub fn begin(root: &Path, receipt: &CommandReceipt) -> Res<Pending> {
    if staged(root)? {recover(root)?;}
    let prior=read(root)?;
    if let Some(old)=&prior {
        if old.receipt.event_id("received")?==receipt.event_id("received")? {return Err("focus command already tracked; check status before retry".into());}
        if let Some(previous)=&old.previous {crate::telemetry::receipt(previous,"superseded")?;}
    }
    let mut next=Pending{project:project_id(root)?,receipt:receipt.clone(),phase:"pending".into(),focus:None,
        previous:prior.filter(|p|p.phase!="failed").map(|p|p.receipt)};
    save(root,&next)?;
    // Only a persisted invalidation makes the previous command superseded.
    if let Some(previous)=&next.previous {crate::telemetry::receipt(previous,"superseded")?;}
    next.previous=None;
    Ok(next)
}

/// Caller still owns the signal lock and has acknowledged this command's terminal receipt.
pub fn finish(root: &Path, mut pending: Pending, applied: bool) -> Res<()> {
    pending.phase=if applied {"applied"} else {"failed"}.into();
    pending.focus=if applied {Some(fingerprint(&data::focus(root)?))} else {None};
    save(root,&pending)
}

/// Caller holds the shared signal lock through snapshot and receipt acknowledgment.
fn observe(root: &Path, focus: &[String], seen: &mut Option<String>) -> Res<Value> {
    operation("command.observe",|| {
    let Some(pending)=read(root)? else {
        *seen=None;return Ok(json!({"state":"untracked","reason":"legacy or no tracked focus command"}));
    };
    if !dot(root).join("poke").is_file() {return Err("focus command signal missing; observation unknown".into());}
    if let Some(previous)=&pending.previous {crate::telemetry::receipt(previous,"superseded")?;}
    if pending.phase!="applied" {return Err(format!("focus command {}; inspect receipt {} before another command",pending.phase,pending.receipt.event_id("received")?));}
    if pending.focus.as_deref()!=Some(fingerprint(focus).as_str()) {return Err("focus differs from tracked command; observation unknown".into());}
    let id=pending.receipt.event_id("observed")?;
    if seen.as_ref()!=Some(&id) {crate::telemetry::receipt(&pending.receipt,"observed")?;*seen=Some(id.clone());}
    Ok(json!({"state":"observed","scope":"tracked-desired-focus","applied_receipt":pending.receipt.event_id("applied")?,"observed_receipt":id}))
    },|_|"command-observe")
}

pub struct Snapshot {
    pub link: Option<coreq::telemetry::capture::TraceContext>,
    pub focus: Vec<String>,
    pub command: Value,
}

/// Recover with exclusive ownership, then take a fresh shared snapshot; a concurrent command may win between them.
pub fn snapshot(root: &Path, signal: &mut Option<String>, command: &mut Option<String>) -> Res<Snapshot> {
    if staged(root)? {
        let file=fs::OpenOptions::new().read(true).write(true).open(dot(root).join("poke")).map_err(|_|"focus command recovery signal unavailable; files kept")?;
        let _owner=coreq::file_lock::try_lock_file(file).map_err(|_|"focus command recovery busy; files kept")?;
        recover(root)?;
    }
    let (link,(focus,command))=crate::wake_read(&dot(root).join("poke"),signal,|| {
        let focus=data::focus(root)?;
        let observed=observe(root,&focus,command)?;
        Ok((focus,observed))
    })?;
    Ok(Snapshot{link,focus,command})
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
