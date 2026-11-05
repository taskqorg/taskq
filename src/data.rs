//! Local data rules (taskq.md § 4 Data): every growing file has a named bound and a pruner the service runs; a corrupt
//! state file reads unknown with a reason and stays where it is, never reset by a reader.

use serde_json::Value;
use std::fs;
use std::path::Path;

/// `<install>/taskq.log` and `<install>/<project>.log`: past it the newest half stays.
pub const LOG_MAX: u64 = 4 << 20;
/// `<root>/.taskq/cli-errors.log`: past it the newest half stays.
pub const CLI_ERRORS_MAX: u64 = 256 << 10;
/// `<root>/.taskq/<T|R><N>-<attempt>.log`: the newest by mtime; a live attempt's log always stays.
pub const ATTEMPT_LOGS: usize = 50;
/// `<root>/.taskq/incidents/<fingerprint>.json`: the newest by mtime.
pub const INCIDENTS: usize = 50;
/// `<root>/.taskq/inputs/*.txt`: newest by mtime plus locked Claude/App files; CLI stdin owns an open read handle.
pub const INPUTS: usize = 50;
/// A turn owns the lock, even when a forked child still holds a copy of its descriptor.
pub type InputLock = coreq::file_lock::Lock;

const FOCUS_MAX: u64 = 4096;

/// Keep the newest `max / 2` bytes of `p`, from a line start, once it is over `max`. Rewritten in place, not renamed:
/// the launcher and the service's own stderr append to the open file.
pub fn trim_tail(p: &Path, max: u64) -> bool {
    let profile_log = p.file_name().is_some_and(|n| n == "taskq.log");
    let _profile_lock = if profile_log {
        match crate::profile::log_lock(p) { Ok(lock) => Some(lock), Err(_) => return false }
    } else { None };
    trim_tail_locked(p,max).unwrap_or(false)
}

/// Caller owns the profile lock for taskq.log; keep import and in-place rotation in that same critical section.
pub(crate) fn trim_tail_locked(p: &Path, max: u64) -> Result<bool,String> {
    let profile_log=p.file_name().is_some_and(|n|n=="taskq.log");
    let threshold=if profile_log {max.saturating_sub(512)} else {max};
    match fs::metadata(p) {
        Ok(meta) if meta.len()<=threshold=>return Ok(false),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(false),
        Err(e)=>return Err(format!("log retention: {e}")),
        Ok(_)=>{},
    }
    let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0,|d|d.as_millis() as u64);
    if profile_log {crate::telemetry::read(p.parent().unwrap(),now)?;}
    let b=fs::read(p).map_err(|e|format!("log retention: {e}"))?;
    let tail=&b[b.len().saturating_sub((max/2) as usize)..];
    let tail=tail.iter().position(|c|*c==b'\n').map_or(tail,|i|&tail[i+1..]);
    fs::write(p,tail).map_err(|e|format!("log retention: {e}"))?;
    if profile_log {crate::telemetry::read(p.parent().unwrap(),now)?;}
    Ok(true)
}

/// Remove the files of `dir` that `pick` names beyond the `n` newest by mtime, except those `keep` names. Returns
/// how many went.
pub fn keep_newest(dir: &Path, n: usize, pick: impl Fn(&str) -> bool, keep: impl Fn(&str) -> bool) -> usize {
    let mut v: Vec<(std::time::SystemTime, std::path::PathBuf, String)> = fs::read_dir(dir).into_iter().flatten().flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let m = e.metadata().ok().filter(|m| m.is_file() && pick(&name))?;
            Some((m.modified().ok()?, e.path(), name))
        }).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.iter().skip(n).filter(|(_, p, name)| !keep(name) && fs::remove_file(p).is_ok()).count()
}

/// The attempt of a `<T|R><N>-<attempt>.log` name.
fn attempt_of(name: &str) -> Option<u64> {
    let (n, a) = name.strip_suffix(".log")?.strip_prefix(['T', 'R'])?.split_once('-')?;
    n.parse::<u64>().ok()?;
    a.parse().ok()
}

/// Input bindings of retained attempts; uncertainty preserves inputs instead of deleting recovery evidence.
fn bound_inputs(root:&Path,live:&[u64])->Result<std::collections::HashSet<String>,String> {
    use std::io::Read;
    let mut names=std::collections::HashSet::new();
    if live.is_empty() {return Ok(names);}
    let mut budget=8*1024*1024u64;
    let mut seen=std::collections::HashSet::new();
    for entry in fs::read_dir(crate::dot(root)).map_err(|e|e.to_string())? {
        let entry=entry.map_err(|e|e.to_string())?;
        let name=entry.file_name();
        let Some(attempt)=name.to_str().and_then(attempt_of).filter(|a|live.contains(a)) else {continue};
        if !entry.file_type().map_err(|e|e.to_string())?.is_file() {return Err("nonregular native recovery log".into());}
        let mut text=String::new();
        fs::File::open(entry.path()).map_err(|e|e.to_string())?.take(budget+1).read_to_string(&mut text).map_err(|e|e.to_string())?;
        if text.len() as u64>budget {return Err("native recovery scan exceeds 8 MiB; inputs kept".into());}
        budget-=text.len() as u64;seen.insert(attempt);
        for line in text.lines().filter(|l|l.trim_start().starts_with('{')) {
            let v:Value=serde_json::from_str(line).map_err(|_|"incomplete native recovery log; inputs kept")?;
            if v["type"]!="taskq.input" {continue;}
            if v["attempt"].as_u64()!=Some(attempt) || !v["execution"].as_u64().is_some_and(|e|e>=attempt) {return Err("invalid native input binding; inputs kept".into());}
            let path=Path::new(v["path"].as_str().ok_or("native input path missing; inputs kept")?);
            let name=path.file_name().and_then(|n|n.to_str()).filter(|n|n.ends_with(".txt")).ok_or("native input filename invalid; inputs kept")?;
            names.insert(name.to_owned());
        }
    }
    if live.iter().any(|a|*a>0 && !seen.contains(a)) {return Err("retained attempt log missing; inputs kept".into());}
    Ok(names)
}

/// One pruning round of the service (§ 4 Data); `live`: native handles plus unresolved reservations.
pub fn prune(root: &Path, install: &Path, project: &str, live: &[u64]) -> Vec<String> {
    let d = crate::dot(root);
    let mut out = vec![];
    for p in [install.join("taskq.log"), install.join(format!("{project}.log"))] {
        if !(crate::host::active() && p.file_name().is_some_and(|n| n == "taskq.log")) && trim_tail(&p, LOG_MAX) {
            out.push(format!("pruned {} to its newest {} KiB", p.display(), LOG_MAX / 2048));
        }
    }
    if trim_tail(&d.join("cli-errors.log"), CLI_ERRORS_MAX) {
        out.push(format!("pruned cli-errors.log to its newest {} KiB", CLI_ERRORS_MAX / 2048));
    }
    let n = keep_newest(&d, ATTEMPT_LOGS, |f| attempt_of(f).is_some(), |f| attempt_of(f).is_some_and(|a| live.contains(&a)));
    if n > 0 {
        out.push(format!("pruned {n} attempt logs beyond the newest {ATTEMPT_LOGS}"));
    }
    // Quota event markers share the lifetime of their exact retained attempt log.
    for entry in fs::read_dir(&d).into_iter().flatten().flatten() {
        let path=entry.path();
        if path.extension().is_some_and(|e|e=="headroom") {
            let log=path.with_extension("log");
            if log.file_name().and_then(|n|n.to_str()).and_then(attempt_of).is_some_and(|a|!live.contains(&a))
                && matches!(log.try_exists(),Ok(false)) {let _=fs::remove_file(path);}
        }
    }
    let inputs = d.join("inputs");
    let bound=bound_inputs(root,live);
    if let Err(e)=&bound {out.push(format!("input retention unknown: {e}"));}
    let n = if let Ok(bound)=bound {keep_newest(&inputs, INPUTS, |f| f.ends_with(".txt"), |f| {
        if bound.contains(f) {return true;}
        // A live turn holds a shared lock; an unreadable file stays rather than risking its input.
        fs::File::options().read(true).write(true).open(inputs.join(f))
            .map_or(true, |file| file.try_lock().is_err())
    })} else {0};
    if n > 0 {
        out.push(format!("pruned {n} input files beyond the newest {INPUTS}"));
    }
    let n = keep_newest(&d.join("incidents"), INCIDENTS, |f| f.ends_with(".json"), |_| false);
    if n > 0 {
        out.push(format!("pruned {n} incident reports beyond the newest {INCIDENTS}"));
    }
    out
}

/// #1445: a power loss can leave a record of zero length or only NUL bytes. The caller proves no live owner; the
/// record is kept as `<name>.zeroed-<ns>` beside it. Ok(true) when moved; missing or nonzero records stay.
pub fn set_aside_zeroed(p: &Path) -> Result<bool, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    match fs::File::open(p).and_then(|f| f.take(KIDS_MAX + 1).read_to_end(&mut bytes)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: unreadable ({e}); file kept", p.display())),
        Ok(_) => {}
    }
    if bytes.iter().any(|b| *b != 0) { return Ok(false); }
    let ns = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    fs::rename(p, p.with_file_name(format!("{name}.zeroed-{ns}"))).map_err(|e| format!("{name}: zeroed record not moved ({e}); file kept"))?;
    Ok(true)
}

/// A JSON state file: Ok(None) when missing; Err with the reason when it is unreadable or not a JSON object (unknown;
/// the file stays as it is).
pub fn json(p: &Path) -> Result<Option<Value>, String> {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match fs::read(p) {
        Ok(b) => match serde_json::from_slice::<Value>(&b) {
            Ok(v) if v.is_object() => Ok(Some(v)),
            Ok(_) => Err(format!("{name}: not a JSON object; file kept")),
            Err(e) => Err(format!("{name}: corrupt ({e}); file kept")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{name}: unreadable ({e}); file kept")),
    }
}

/// The labels of `<root>/.taskq/focus` (§ 4): missing, empty or `off` is none. Err with the reason when it is not
/// short UTF-8 text of labels (unknown: the service admits nothing; the file stays).
pub fn focus(root: &Path) -> Result<Vec<String>, String> {
    let b = match fs::read(crate::dot(root).join("focus")) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("focus: unreadable ({e}); file kept")),
    };
    if b.len() as u64 > FOCUS_MAX {
        return Err(format!("focus: over {FOCUS_MAX} bytes; file kept"));
    }
    let t = String::from_utf8(b).map_err(|_| "focus: not UTF-8 text; file kept".to_string())?;
    if t.chars().any(|c| c.is_control() && !c.is_whitespace()) {
        return Err("focus: control characters; file kept".into());
    }
    Ok(t.split_whitespace().filter(|f| *f != "off").map(String::from).collect())
}

pub const KIDS_MAX: u64 = 1 << 20;

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct KidLine {
    pub task:u64, pub attempt:u64, pub runtime:String, pub process:coreq::process::Identity,
    pub execution:u64, pub offset:u64, pub job:Option<String>,
}

/// #2266: a `kids` job scope token; `job-kill:` jobs end every member when their last handle closes, `job-global:` (older) ones do not.
pub fn job_scope(token:&str)->Option<(&str,bool)> {
    let (key,kill)=match token.strip_prefix("job-kill:") {Some(key)=>(key,true),None=>(token.strip_prefix("job-global:")?,false)};
    job_key(key).then_some((key,kill))
}

/// #2266: a vanished job name proves exit only for a kill-on-close scope whose original root is gone.
pub fn missing_scope_exit(token:&str)->Result<(),String> {
    match job_scope(token) {
        Some((_,true))=>Ok(()),
        Some((_,false))=>Err("native job name unavailable after handle loss; descendants may survive; native reconciliation required".into()),
        None=>Err("native job identity invalid".into()),
    }
}

pub fn job_key(key:&str)->bool {
    key.len()==36 && key.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i) {b==b'-'} else {b.is_ascii_digit() || (b'a'..=b'f').contains(&b)})
}

/// Existing handover record; legacy/invalid evidence is preserved, never guessed from a delivery cursor.
pub fn kids(text: &str) -> Result<(coreq::process::Identity, Vec<KidLine>), String> {
    if text.len() as u64 > KIDS_MAX { return Err("kids: over 1 MiB; file kept".into()); }
    let identity=|pid:&str,started:&str|->Option<coreq::process::Identity> {
        let pid=pid.parse::<u32>().ok().filter(|n| *n>0 && *n<=i32::MAX as u32)?;
        if started.is_empty() || started.len()>128 || !started.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'-') { return None; }
        Some(coreq::process::Identity {pid,started:started.into()})
    };
    let mut lines = text.lines();
    let header=lines.next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();
    let owner=match header.as_slice() { ["pid",pid,started,"executor-v1"]=>identity(pid,started), _=>None }
        .ok_or("kids: native executor identity version missing; legacy launcher or malformed file kept")?;
    let kid = |l: &str| -> Option<KidLine> {
        let w: Vec<&str> = l.split_whitespace().collect();
        let (fields,job)=match w.as_slice() {
            [_,_,_,_,_,_,_] =>(w.as_slice(),None),
            [fields @ ..,scope] if fields.len()==7 =>{
                job_scope(scope)?;
                (fields,Some((*scope).to_owned()))
            },
            _=>return None,
        };
        let [task, attempt, rt, pid, started, execution, offset] = fields[..] else { return None };
        let task=task.parse::<u64>().ok().filter(|n|*n>0)?;
        let attempt=attempt.parse::<u64>().ok().filter(|n|*n>0)?;
        let execution=execution.parse::<u64>().ok().filter(|n|*n>=attempt)?;
        let a=crate::runtime::of(rt); if !a.has("cli") || a.has("remote-control") && (offset!="0" || job.is_some()) { return None; }
        Some(KidLine {task,attempt,runtime:rt.into(),process:identity(pid,started)?,execution,offset:offset.parse().ok()?,job})
    };
    let v: Vec<KidLine> = lines.enumerate().map(|(i, l)| kid(l).ok_or(format!("kids: line {} malformed; file kept", i + 2))).collect::<Result<_, _>>()?;
    let (mut tasks,mut pids,mut jobs)=(std::collections::HashSet::new(),std::collections::HashSet::new(),std::collections::HashSet::new());
    if v.iter().any(|k|!tasks.insert(k.task) || !pids.insert(k.process.pid) || k.process.pid==owner.pid || k.job.as_ref().is_some_and(|job|!jobs.insert(job))) { return Err("kids: duplicate or service identity used as child; file kept".into()); }
    Ok((owner, v))
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod tests;
