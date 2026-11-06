//! §7 evidence outcomes: owner binding, read-only proof, and no source adapter calls.
use crate::{board::{self, Board, Issue}, core::{self, Entry, Task}, dispatch::{Op, Receipt}, Res};
use std::path::Path;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "taskq: evidence-authorized\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    task: u64, board: String, candidate: String, baseline: String, result: u64,
    revision: u64, through: u64, scope: String, url: String, sha256: String, op: String,
    #[serde(default)] source_record: Option<String>,
}
fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()
}
fn scope(issue: &Issue) -> Res<String> {
    let types: Vec<_> = issue.labels.iter().filter(|s|matches!(s.as_str(), "code"|"research"|"docs"|"asset")).collect();
    if types.len()!=1 || types[0]=="code" {return Err("evidence completion requires one non-code task type".into());}
    Ok(hash(format!("{}\n{}\n{}",issue.title,issue.body,types[0]).as_bytes()))
}
fn hex(s: &str, len: usize) -> bool {s.len()==len && s.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}
fn git(root: &Path, args: &[&str]) -> Res<String> {
    let mut c=crate::platform::command("git");
    c.arg("-C").arg(root).args(["--no-optional-locks","-c","core.fsmonitor=false","-c","core.hooksPath=/dev/null"]).args(args);
    let output=coreq::process::output("evidence.git",&mut c,std::time::Duration::from_secs(10)).map_err(|_|"evidence workspace observation unknown".to_string())?;
    String::from_utf8(output.stdout).map_err(|_|"evidence workspace identity unreadable".into())
}
fn workspace(root: &Path,n: u64,candidate: &str,baseline: &str) -> Res<()> {
    if !hex(candidate,40)||!hex(baseline,40) {return Err("evidence candidate/baseline requires full lowercase SHA".into());}
    let name=format!("taskq-{n}");
    let dir=if root.file_name().is_some_and(|s|s==name.as_str()) && root.parent().and_then(Path::file_name).is_some_and(|s|s==".worktrees") {root.to_path_buf()} else {root.join(".worktrees").join(name)};
    let branch=format!("refs/heads/taskq-{n}");
    if git(&dir,&["symbolic-ref","HEAD"])?.trim()!=branch || git(&dir,&["rev-parse","HEAD"])?.trim()!=candidate {
        return Err("evidence task worktree does not name the candidate".into());
    }
    // A missing/truncated creation record is unknown, never an inferred baseline.
    let reflog=git(&dir,&["reflog","show","--format=%H %gs",&branch])?;
    let created=reflog.lines().last().ok_or("evidence attempt baseline unknown")?;
    if !created.starts_with(&format!("{baseline} branch: Created from ")) {
        return Err("evidence attempt baseline changed or unknown".into());
    }
    if !git(&dir,&["status","--porcelain","--untracked-files=all"])?.trim().is_empty() ||
        !git(&dir,&["diff","--no-ext-diff","--name-only",baseline,candidate,"--"])?.trim().is_empty() {
        return Err("evidence task has a source delta".into());
    }
    Ok(())
}
/// Reads one small identity member, never extracts an archive or executes its contents.
/// The ZIP directory, local header, sizes and CRC must agree; ZIP64/encryption/streaming refuse.
fn zip_record(bytes: &[u8], name: &str) -> Res<String> {
    use std::io::Read;
    let bad=||"archive source record missing, corrupt or unsupported".to_string();
    if name.is_empty() || name.contains(char::is_control) || name.starts_with('/') || name.contains('\\') || name.split('/').any(|s|s.is_empty()||s=="."||s=="..") {return Err(bad());}
    let u16at=|at:usize|->Res<usize>{Ok(u16::from_le_bytes(bytes.get(at..at+2).ok_or_else(bad)?.try_into().map_err(|_|bad())?) as usize)};
    let u32at=|at:usize|->Res<usize>{Ok(u32::from_le_bytes(bytes.get(at..at+4).ok_or_else(bad)?.try_into().map_err(|_|bad())?) as usize)};
    let end=bytes.len().checked_sub(22).ok_or_else(bad)?;
    if bytes.get(end..end+4)!=Some(b"PK\x05\x06") || u16at(end+4)?!=0 || u16at(end+6)?!=0 || u16at(end+20)?!=0 {return Err(bad());}
    let count=u16at(end+10)?;
    let mut at=u32at(end+16)?;
    if count==65535 || u16at(end+8)?!=count || at.checked_add(u32at(end+12)?)!=Some(end) {return Err(bad());}
    let mut found=None;
    for _ in 0..count {
        if bytes.get(at..at+4)!=Some(b"PK\x01\x02") {return Err(bad());}
        let len=u16at(at+28)?;let extra=u16at(at+30)?;let comment=u16at(at+32)?;
        let entry=bytes.get(at+46..at+46+len).ok_or_else(bad)?;
        if entry==name.as_bytes() {
            if found.is_some() || u16at(at+8)?&!0x0806!=0 || u16at(at+34)?!=0 {return Err(bad());}
            let method=u16at(at+10)?;let crc=u32at(at+16)? as u32;let compressed=u32at(at+20)?;let size=u32at(at+24)?;
            if size>65536 {return Err(bad());}
            let local=u32at(at+42)?;
            if local>=at || bytes.get(local..local+4)!=Some(b"PK\x03\x04") || u16at(local+6)?!=u16at(at+8)? || u16at(local+8)?!=method
                || u32at(local+14)?!=crc as usize || u32at(local+18)?!=compressed || u32at(local+22)?!=size || u16at(local+26)?!=len
                || bytes.get(local+30..local+30+len)!=Some(entry) {return Err(bad());}
            let start=local+30+len+u16at(local+28)?;
            let finish=start.checked_add(compressed).filter(|f|*f<=at).ok_or_else(bad)?;
            let data=bytes.get(start..finish).ok_or_else(bad)?;
            let decoded=match method {
                0=>data.to_vec(),
                8=>{let mut out=Vec::new();flate2::read::DeflateDecoder::new(data).take(65537).read_to_end(&mut out).map_err(|_|bad())?;out},
                _=>return Err(bad()),
            };
            let mut actual=flate2::Crc::new();actual.update(&decoded);
            if decoded.len()!=size || actual.sum()!=crc {return Err(bad());}
            found=Some(String::from_utf8(decoded).map_err(|_|bad())?);
        }
        at=at.checked_add(46+len+extra+comment).filter(|v|*v<=end).ok_or_else(bad)?;
    }
    if at!=end {return Err(bad());}
    found.ok_or_else(bad)
}
fn result(n: u64,log: &[Entry],t: &Task) -> Res<u64> {
    let p=core::project(n,log,false);
    p.changes.iter().rev().find(|(id,_,after)|after.result==t.result && log.iter().any(|e|e.id==*id&&core::kind(n,&e.body)=="result"))
        .map(|(id,_,_)|*id).ok_or("evidence result identity unknown".into())
}
fn names(text: &str,url: &str) -> bool {
    text.split(|c:char|c.is_whitespace()||matches!(c,'"'|'(' | ')' | '<' | '>' )).any(|s|s==url)
}
fn validate(b: &dyn Board, root: &Path, issue: &Issue, t: &Task, log: &[Entry], a: &Authorization) -> Res<()> {
    let n=issue.number;
    if a.task!=n || a.board!=b.url(n) || Some(a.candidate.as_str())!=t.result.as_deref() || a.result!=result(n,log,t)? || a.scope!=scope(issue)? {
        return Err("evidence authorization no longer matches task/result identity".into());
    }
    let rev=core::revision(n,log);
    let projection=core::project(n,log,false);
    if log.iter().any(|e|e.id>a.through && projection.accepts(e.id) && core::kind(n,&e.body)!="publish") {
        return Err("evidence authorization outcome changed".into());
    }
    let published=log.iter().find(|e|e.id==rev && !e.edited && core::kind(n,&e.body)=="publish");
    if rev!=a.revision && !published.is_some_and(|p|core::revision(n,&log.iter().filter(|e|e.id<p.id).cloned().collect::<Vec<_>>())==a.revision) {
        return Err("evidence authorization revision is stale".into());
    }
    if !a.url.starts_with("https://") || a.url.contains(char::is_whitespace) || !hex(&a.sha256,64) {return Err("evidence attachment identity invalid".into());}
    if !log.iter().any(|e|e.id==a.result && names(&e.body,&a.url)) && !t.publication.as_ref().is_some_and(|v|names(v,&a.url)) {
        return Err("evidence attachment is absent from the accepted result/verdict".into());
    }
    let bytes=b.file(&a.url).map_err(|_|"evidence attachment read unknown".to_string())?;
    if bytes.is_empty() || bytes.len()>25*1024*1024 || hash(&bytes)!=a.sha256 {return Err("evidence attachment missing or changed".into());}
    if let Some(name)=&a.source_record {
        if !hex(&a.candidate,40) || a.candidate!=a.baseline {return Err("archive source baseline differs from candidate".into());}
        let record=zip_record(&bytes,name)?;
        let source:Vec<_>=record.lines().filter(|l|l.split_whitespace().next()==Some("source")).collect();
        if source.len()!=1 || source[0].split_whitespace().collect::<Vec<_>>()!=["source",a.candidate.as_str(),"dirty=","0"] {
            return Err("archive clean-source identity missing or changed".into());
        }
        // Archived proof is explicit, never an implicit fallback from an unknown workspace read.
        if root.join(format!(".worktrees/taskq-{n}")).try_exists().map_err(|_|"evidence workspace existence unknown")?
            || root.file_name().is_some_and(|s|s==format!("taskq-{n}").as_str()) {
            workspace(root,n,&a.candidate,&a.baseline)?;
        }
    } else {workspace(root,n,&a.candidate,&a.baseline)?;}
    let fresh=b.issue(n)?;
    let fresh_log=board::entries(b,n)?;
    if scope(&fresh)?!=a.scope || fresh_log.iter().map(|e|(e.id,&e.body,e.edited)).ne(log.iter().map(|e|(e.id,&e.body,e.edited))) {
        return Err("evidence task changed during verification".into());
    }
    Ok(())
}
/// An audit selects this path even when invalid: never fall through to source publication.
pub fn completion(b: &dyn Board,root: &Path,issue: &Issue,t: &Task,log: &[Entry]) -> Option<Res<String>> {
    let audit=log.iter().rev().find(|e|e.body.starts_with(PREFIX))?;
    Some((|| {
        if audit.edited {return Err("evidence authorization was edited".into());}
        let a: Authorization=serde_json::from_str(audit.body.strip_prefix(PREFIX).unwrap()).map_err(|_|"evidence authorization unreadable".to_string())?;
        if log.iter().any(|e|e.id!=audit.id && e.body.starts_with(PREFIX) && serde_json::from_str::<Authorization>(&e.body[PREFIX.len()..]).is_ok_and(|v|v.op==a.op) && (e.edited || e.body!=audit.body)) {
            return Err("evidence authorization operation is ambiguous".into());
        }
        validate(b,root,issue,t,log,&a)?;
        Ok(format!("Completion: evidence-only v1: authorization {}; candidate {}; evidence {} sha256 {}; source not published",audit.id,a.candidate,a.url,a.sha256))
    })())
}
/// Owner-only audit. It neither approves a gate nor changes the fold.
#[allow(clippy::too_many_arguments)]
pub fn authorize(b: &dyn Board,root: &Path,n: u64,candidate: &str,baseline: &str,url: &str,sha256: &str,op: &Op,owner: bool,readers: bool,source_record: Option<&str>) -> Receipt {
    let run=|| -> Res<Receipt> {
        if !owner || !readers {return Err("owner authorization and --readers-current yes required".into());}
        let id=op.id.as_deref().filter(|s|!s.is_empty()&&!s.contains(char::is_whitespace)).ok_or("evidence requires --op-id")?;
        let revision=op.revision.ok_or("evidence requires --revision")?;
        let issue=b.issue(n)?;let log=board::entries(b,n)?;let t=core::fold(n,&log,false);
        if let Some(prior)=log.iter().find(|e|e.body.starts_with(PREFIX)&&serde_json::from_str::<Authorization>(&e.body[PREFIX.len()..]).is_ok_and(|v|v.op==id)) {
            let a: Authorization=serde_json::from_str(&prior.body[PREFIX.len()..]).map_err(|_|"evidence authorization unreadable".to_string())?;
            if prior.edited || a.task!=n || a.board!=b.url(n) || a.candidate!=candidate || a.baseline!=baseline || a.url!=url || a.sha256!=sha256 || a.revision!=revision || a.source_record.as_deref()!=source_record {
                return Err("evidence operation identity changed".into());
            }
            return Ok(Receipt::accepted(n,op,t.state,a.revision));
        }
        if !issue.open || !issue.trusted || t.state!=core::State::Review {return Err("evidence requires an open trusted reviewed result".into());}
        let projection=core::project(n,&log,false);
        let a=Authorization{task:n,board:b.url(n),candidate:candidate.into(),baseline:baseline.into(),result:result(n,&log,&t)?,revision,through:log.iter().filter(|e|projection.accepts(e.id)).map(|e|e.id).max().unwrap_or(0),scope:scope(&issue)?,url:url.into(),sha256:sha256.into(),op:id.into(),source_record:source_record.map(String::from)};
        let body=format!("{PREFIX}{}",serde_json::to_string(&a).map_err(|e|e.to_string())?);
        if core::revision(n,&log)!=revision {return Err("evidence authorization revision is stale".into());}
        validate(b,root,&issue,&t,&log,&a)?;
        let receipt=b.comment(n,&body)?;
        if !board::entries(b,n)?.iter().any(|e|e.id==receipt&&!e.edited&&e.body==body) {return Err("evidence authorization readback unknown; retry same operation".into());}
        Ok(Receipt::accepted(n,op,t.state,revision).wake(root))
    };
    run().unwrap_or_else(|e|Receipt::rejected(n,op,e))
}

/// Accepted completion identity for projections; never qualifies source publication.
pub fn receipt(n: u64, log: &[Entry]) -> Option<(u64, String)> {
    let p=core::project(n,log,false);
    log.iter().rev().filter(|e|p.accepts(e.id)&&core::kind(n,&e.body)=="close")
        .find_map(|e|e.body.lines().find(|l|l.starts_with("Completion: evidence-only v1:")).map(|l|(e.id,l.to_string())))
}

/// Stable completion operation survives a lost POST response without a second receipt.
pub fn operation(proof: &str, revision: u64) -> Op {
    Op {id:Some(format!("evidence-close-{}",hash(proof.as_bytes()))),revision:Some(revision)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{board::Fake, core::{entry, State}, dispatch};
    struct Fixture {b: Fake,root: std::path::PathBuf,n:u64,sha:String,cfg:crate::service::Config}
    impl Fixture {
        fn new() -> Self {
            let root=std::env::temp_dir().join(format!("taskq-evidence-{}",crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            let run=|args:&[&str]|git(&root,args).unwrap();
            run(&["init","-q"]);run(&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-q","--allow-empty","-m","invented baseline"]);
            let sha=run(&["rev-parse","HEAD"]).trim().to_string();
            let dir=root.join(".worktrees/taskq-1");
            run(&["worktree","add","-q","-b","taskq-1",dir.to_str().unwrap(),&sha]);
            std::fs::write(root.join("taskq.json"),r#"{"repo":"invented/evidence"}"#).unwrap();
            let (mut cfg,_)=crate::config::load(&root).unwrap();
            cfg.install=root.join("install");
            let b=Fake::default();
            let n=b.create("Evidence fixture","## Goal\nqualify an immutable candidate",&["taskq".into(),"research".into()]).unwrap();
            let a=b.comment(n,&entry("spawn",&["worker","codex:invented","fixture"],None,"")).unwrap();
            b.comment(n,&entry("result",&[&sha],Some(a),"Changed: collected evidence https://board.example/evidence.zip\nOpen: native coverage remains limited")).unwrap();
            b.comment(n,&entry("spawn",&["reviewer","codex:invented-review","fixture"],None,"")).unwrap();
            b.state.borrow_mut().images.push(("https://board.example/evidence.zip".into(),b"invented evidence".to_vec()));
            Self{b,root,n,sha,cfg}
        }
        fn log(&self)->Vec<Entry>{board::entries(&self.b,self.n).unwrap()}
        fn task(&self)->Task{core::fold(self.n,&self.log(),false)}
        fn authorize(&self)->Receipt {
            authorize(&self.b,&self.root,self.n,&self.sha,&self.sha,"https://board.example/evidence.zip",&hash(b"invented evidence"),&Op{id:Some("invented-authorization".into()),revision:Some(core::revision(self.n,&self.log()))},true,true,None)
        }
        fn save(&self) {
            self.b.comment(self.n,&entry("publish",&[],None,"Changed: collected evidence\nOpen: native limitations retained")).unwrap();
        }
        fn resume(&self)->Receipt {
            dispatch::resume_publication(&self.b,&self.cfg,&self.b.issue(self.n).unwrap(),&self.task(),&self.log())
        }
        fn initial(&self)->Receipt {dispatch::close_one(&self.b,&self.cfg,self.n,"Changed: evidence checked\nOpen: native limitations retained",&Op::default())}
        fn closes(&self)->usize {self.log().iter().filter(|e|!e.edited&&core::kind(self.n,&e.body)=="close").count()}
    }
    impl Drop for Fixture {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.root);}}

    // §7/R5: production dispatch, real bounded Git observations, substituted board bytes.
    // Risk: off-main evidence enters source publication, or restart repeats the close/input.
    #[test]
    fn evidence_completion_initial_and_restart_never_publish_source() {
        for saved in [false,true] {
            let f=Fixture::new();
            f.b.state.borrow_mut().publish="superseded".into();
            if saved {f.save();}
            assert!(f.authorize().ok());
            let receipt=if saved {f.resume()}else{f.initial()};
            assert!(receipt.ok(),"{}",receipt.text());
            assert_eq!(f.task().state,State::Closed);
            assert_eq!(f.closes(),1);
            assert_eq!(f.b.state.borrow().publications,0);
            let log=f.log();
            let summary=crate::summary::text(&f.b,f.n,&f.b.issue(f.n).unwrap().body,&log,&f.task(),None);
            assert!(summary.contains("evidence-only"));
            let close=log.iter().find(|e|core::kind(f.n,&e.body)=="close").unwrap();
            assert!(close.body.contains("Completion: evidence-only v1:")&&close.body.contains(&f.sha)&&close.body.contains(&hash(b"invented evidence")));
            assert!(!close.body.contains("already on main"));
            assert_eq!(crate::deploy::delivered(f.n,&log),None,"evidence completion cannot announce a source deployment");
            let source=crate::readiness::source(f.n,&f.b.url(f.n),&log);
            assert_eq!(source["accepted_result_state"],"proven");assert_eq!(source["state"],"unknown");
            assert!(!f.resume().ok());assert_eq!(f.closes(),1);
        }
        let f=Fixture::new();f.save();assert!(f.authorize().ok());
        f.b.state.borrow_mut().lose_comment_response=true;
        assert_eq!(f.resume().outcome,"unknown");
        // Rebuild exclusively from board entries, as after restart; no launch interface is involved.
        assert_eq!(f.task().state,State::Closed);
        assert!(!f.resume().ok());assert_eq!(f.closes(),1);assert_eq!(f.b.state.borrow().publications,0);
        let mut restarted=crate::service::Service::new(&f.b,&f.cfg,"fixture",Box::new(|_|panic!("evidence restart must not launch or replay input")));
        restarted.pass().unwrap();
        assert_eq!(f.closes(),1);assert_eq!(f.b.state.borrow().publications,0);
        crate::service::project(&f.b,&f.b.issue(f.n).unwrap(),&f.task()).unwrap();
        assert!(!f.b.issue(f.n).unwrap().open);
    }

    // A2/A3: fail closed at both adapters; stale authorization is never a missing-PR exemption.
    #[test]
    fn evidence_completion_refuses_changed_unknown_and_unapproved_identity() {
        for saved in [false,true] {
            for fault in ["bytes","missing","scope","type","code","candidate","baseline","dirty","edited","malformed","revision","steer","gate","stale-gate"] {
                let f=Fixture::new();if saved {f.save();}assert!(f.authorize().ok());
                match fault {
                    "bytes"=>f.b.state.borrow_mut().bad_image=true,
                    "missing"=>f.b.state.borrow_mut().images.clear(),
                    "scope"=>f.b.edit(f.n,"changed goal").unwrap(),
                    "type"=>f.b.labels(f.n,&[],&["research".into()]).unwrap(),
                    "code"=>f.b.labels(f.n,&["code".into()],&["research".into()]).unwrap(),
                    "candidate"|"baseline"|"malformed"=>{
                        let mut st=f.b.state.borrow_mut();let c=&mut st.comments.last_mut().unwrap().1;
                        let mut a:Authorization=serde_json::from_str(c.body.strip_prefix(PREFIX).unwrap()).unwrap();
                        if fault=="candidate" {a.candidate="a".repeat(40);}else{a.baseline="b".repeat(40);}
                        c.body=if fault=="malformed"{format!("{PREFIX}{{}}") }else{format!("{PREFIX}{}",serde_json::to_string(&a).unwrap())};
                    }
                    "dirty"=>std::fs::write(f.root.join(".worktrees/taskq-1/untracked"),"invented delta").unwrap(),
                    "edited"=>f.b.state.borrow_mut().comments.last_mut().unwrap().1.edited=true,
                    "revision"=>{
                        if saved {f.b.comment(f.n,"taskq: publication-blocked\nProblem: new decision").unwrap();}
                        else {f.b.comment(f.n,&entry("ask",&[],f.task().attempt,"Problem: new decision")).unwrap();}
                        assert_eq!(f.task().state,State::Ask);
                        f.b.comment(f.n,"taskq: answer\nContinue").unwrap();
                        assert_eq!(f.task().state,State::Review);
                    }
                    "steer"=>{f.b.comment(f.n,"taskq: steer\nRecheck changed evidence").unwrap();}
                    "gate"|"stale-gate"=>{},
                    _=>unreachable!(),
                }
                let r=if fault=="gate"||fault=="stale-gate" {
                    let mut cfg=f.cfg.clone();cfg.delivery=crate::config::Delivery::ReviewPr;
                    if saved {let mut t=f.task();t.gate=Some(if fault=="gate"{core::Gate::Required}else{core::Gate::Stale});dispatch::resume_publication(&f.b,&cfg,&f.b.issue(f.n).unwrap(),&t,&f.log())}
                    else {dispatch::close_one(&f.b,&cfg,f.n,"Changed: evidence checked\nOpen: none",&Op::default())}
                } else if saved {f.resume()}else{f.initial()};
                assert!(!r.ok(),"{saved} {fault}: {}",r.text());assert_eq!(f.closes(),0,"{fault}");assert_eq!(f.b.state.borrow().publications,0,"{fault}");
            }
        }
        for saved in [false,true] {
            let f=Fixture::new();if saved {f.save();}
            f.b.state.borrow_mut().publish="superseded".into();
            if saved {f.resume();} else {f.initial();}
            assert_ne!(f.task().state,State::Closed,"no authorization must retain source-publication guards");
            assert_eq!(f.closes(),0);assert_eq!(f.b.state.borrow().publications,1);
        }
        let f=Fixture::new();
        let op=Op{id:Some("invented-owner".into()),revision:Some(core::revision(f.n,&f.log()))};
        for (owner,readers) in [(false,true),(true,false)] {
            assert!(!authorize(&f.b,&f.root,f.n,&f.sha,&f.sha,"https://board.example/evidence.zip",&hash(b"invented evidence"),&op,owner,readers,None).ok());
        }
        assert!(f.authorize().ok());assert!(f.authorize().ok());
        assert_eq!(f.log().iter().filter(|e|e.body.starts_with(PREFIX)).count(),1);
    }
    // A2: an explicitly named candidate still cannot hide a committed task source delta.
    #[test]
    fn evidence_authorization_refuses_a_committed_source_delta() {
        let f=Fixture::new();let dir=f.root.join(".worktrees/taskq-1");
        std::fs::write(dir.join("source.rs"),"// invented task change\n").unwrap();
        git(&dir,&["add","source.rs"]).unwrap();
        git(&dir,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","-q","-m","invented source delta"]).unwrap();
        let sha=git(&dir,&["rev-parse","HEAD"]).unwrap();let sha=sha.trim();
        f.b.comment(f.n,"taskq: requeue").unwrap();
        let a=f.b.comment(f.n,"taskq: spawn worker codex:invented fixture").unwrap();
        f.b.comment(f.n,&entry("result",&[sha],Some(a),"Changed: invented https://board.example/evidence.zip\nOpen: none")).unwrap();
        let op=Op{id:Some("invented-code-delta".into()),revision:Some(core::revision(f.n,&f.log()))};
        let r=authorize(&f.b,&f.root,f.n,sha,&f.sha,"https://board.example/evidence.zip",&hash(b"invented evidence"),&op,true,true,None);
        assert!(!r.ok() && r.reason.contains("source delta"),"{}",r.text());
        assert_eq!(f.b.state.borrow().publications,0);assert_eq!(f.closes(),0);
    }

    // A4/A5: a frozen clean-clone record supports evidence-only work with no task checkout.
    // Hash/ZIP/source identity mutations refuse; nothing reconstructs a branch or reruns a probe.
    #[test]
    fn evidence_archive_completion_binds_clean_source_without_a_checkout() {
        for fault in ["none","missing-record","dirty-record","other-source","corrupt","oversize","truncated"] {
            let f=Fixture::new();
            let dir=f.root.join(".worktrees/taskq-1");
            git(&f.root,&["worktree","remove",dir.to_str().unwrap()]).unwrap();
            let record=match fault {
                "dirty-record"=>format!("source {} dirty= 1\n",f.sha),
                "other-source"=>format!("source {} dirty= 0\n","b".repeat(40)),
                "oversize"=>"x".repeat(65537),
                _=>format!("source {} dirty= 0\n",f.sha),
            };
            let name="invented-evidence/identity-before.txt";
            let mut data=crate::cli::text::zip_one(name,record.as_bytes()).unwrap();
            if fault=="corrupt" {data[14]^=1;}
            if fault=="truncated" {data.truncate(data.len()-1);}
            let digest=hash(&data);f.b.state.borrow_mut().images[0].1=data;
            f.save();
            let op=Op{id:Some("invented-archive-authority".into()),revision:Some(core::revision(f.n,&f.log()))};
            let selected=if fault=="missing-record"{"invented-evidence/missing.txt"}else{name};
            let r=authorize(&f.b,&f.root,f.n,&f.sha,&f.sha,"https://board.example/evidence.zip",&digest,&op,true,true,Some(selected));
            if fault=="none" {
                assert!(r.ok(),"{}",r.text());assert!(f.resume().ok());assert_eq!(f.closes(),1);
                let mut restarted=crate::service::Service::new(&f.b,&f.cfg,"fixture",Box::new(|_|panic!("archive completion must not launch a runtime")));
                restarted.pass().unwrap();assert_eq!(f.closes(),1);
            } else {assert!(!r.ok(),"{fault}: {}",r.text());assert_eq!(f.closes(),0);}
            assert!(!dir.exists());assert_eq!(f.b.state.borrow().publications,0);
        }
    }

    // A5 read-only development checkpoint: real sanitized receipts/bytes, substituted board only.
    #[test]
    #[ignore = "requires the explicitly supplied #2462 board and attachment snapshots"]
    fn evidence_2462_snapshot_checkpoint() {
        let f=Fixture::new();
        let snapshot=std::env::var("TASKQ_EVIDENCE_SNAPSHOT").expect("sanitized board snapshot required");
        let archive=std::env::var("TASKQ_EVIDENCE_ARCHIVE").expect("sanitized attachment snapshot required");
        let value:serde_json::Value=serde_json::from_slice(&std::fs::read(snapshot).unwrap()).unwrap();
        let data=std::fs::read(archive).unwrap();
        let candidate="12699ff8eed7d16ac721090654e802648ccc0c63";
        let digest="b2d942f439bff45ec87e1468e0e1c7aba3052f90af98f529e5cc5f341b7c715d";
        assert_eq!(hash(&data),digest);assert_eq!(value["number"],2462);
        let url="https://github.com/taskqorg/taskq/raw/1d585528371cda28cdb571e6acd167b3b20a3848/2462/b2d942f439bf-taskq-2462-mac-evidence.zip";
        {
            let mut st=f.b.state.borrow_mut();
            st.issues=vec![Issue{number:2462,title:value["title"].as_str().unwrap().into(),body:value["body"].as_str().unwrap().into(),
                labels:serde_json::from_value(value["labels"].clone()).unwrap(),open:true,trusted:true,..Default::default()}];
            st.comments=value["comments"].as_array().unwrap().iter().map(|c|(2462,board::Comment{id:c["id"].as_u64().unwrap(),body:c["body"].as_str().unwrap().into(),trusted:c["trusted"]==true,edited:c["edited"]==true,..Default::default()})).collect();
            st.next_id=st.comments.iter().map(|(_,c)|c.id).max().unwrap();st.images=vec![(url.into(),data)];
        }
        let n=2462;let read=||{let log=board::entries(&f.b,n).unwrap();let t=core::fold(n,&log,false);(t,log)};
        let (before,_)=read();assert_eq!(before.state,State::Ask);assert_eq!(before.result.as_deref(),Some(candidate));
        // Simulated owner answer/authorization exist only on this in-memory board, never the live task.
        f.b.comment(n,"taskq: answer\nPreserve the evidence-only outcome and all limitations").unwrap();
        let (_,log)=read();let op=Op{id:Some("invented-snapshot-authorization".into()),revision:Some(core::revision(n,&log))};
        let r=authorize(&f.b,&f.root,n,candidate,candidate,url,digest,&op,true,true,Some("taskq-2462-mac-evidence/identity-before.txt"));
        assert!(r.ok(),"{}",r.text());
        let (t,log)=read();let r=dispatch::resume_publication(&f.b,&f.cfg,&f.b.issue(n).unwrap(),&t,&log);
        assert!(r.ok(),"{}",r.text());let (t,log)=read();assert_eq!(t.state,State::Closed);
        let close=log.iter().find(|e|core::kind(n,&e.body)=="close").unwrap();
        assert!(core::verdict(n,&log,core::entry_text(n,&close.body)).contains("installed-service not qualified"));
        assert_eq!(f.b.state.borrow().publications,0);
        assert_eq!(log.iter().filter(|e|core::kind(n,&e.body)=="close").count(),1);
        assert!(crate::deploy::delivered(n,&log).is_none());
        println!("#2462 snapshot: exact candidate/archive verified; one evidence receipt; zero source publication; no live task or native probe changed");
    }

}
