//! TW5–TW7: local message view marks, never board or native conversation truth.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::Path};
type Res<T> = Result<T, String>;

#[derive(Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Mark {
    #[serde(default)] read: bool,
    #[serde(default)] archived: bool,
    #[serde(default)] deleting: bool,
    #[serde(default)] request: Option<Identity>,
    #[serde(default)] question: Option<Question>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Identity { host: String, id: String, state: String }
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Question { board: String, task: u64, revision: u64 }
#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Store { schema: u8, marks: BTreeMap<String, Mark>, deleted: BTreeSet<String> }
impl Default for Store {
    fn default() -> Self { Self { schema: 1, marks: BTreeMap::new(), deleted: BTreeSet::new() } }
}
fn digest(value: &Value) -> String {
    ring::digest::digest(&ring::digest::SHA256, value.to_string().as_bytes()).as_ref().iter().map(|b|format!("{b:02x}")).collect()
}
fn request_key(id: &str) -> String { digest(&json!(["request-id",id])) }

/// Refuse links/reparse points before reading or deleting; never walk outside the owned directory.
pub(crate) fn plain(path: &Path) -> Res<bool> {
    let meta=match fs::symlink_metadata(path) {
        Ok(meta)=>meta, Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(false),
        Err(e)=>return Err(format!("{} unknown: {e}",path.display())),
    };
    #[cfg(windows)]
    let linked={use std::os::windows::fs::MetadataExt;meta.file_attributes()&0x400!=0};
    #[cfg(not(windows))]
    let linked=meta.file_type().is_symlink();
    if linked || !(meta.is_dir()||meta.is_file()) {return Err(format!("{} is not an owned regular path; kept",path.display()));}
    Ok(true)
}
fn read(install: &Path) -> Res<Store> {
    let path=install.join("messages.json");
    if !plain(&path)? {return Ok(Store::default());}
    let store:Store=serde_json::from_slice(&fs::read(&path).map_err(|e|e.to_string())?).map_err(|e|format!("Message marks unknown; kept: {e}"))?;
    if store.schema!=1 || store.marks.keys().chain(store.deleted.iter()).any(|key|key.len()!=64||!key.bytes().all(|c|c.is_ascii_hexdigit()))
        || store.marks.iter().any(|(key,m)| (m.deleting&&(!m.archived||m.request.is_none()))
            ||m.request.as_ref().is_some_and(|r|!crate::resources::id(&r.id)||r.host.is_empty()||*key!=digest(&json!(["request",r.host,r.id,r.state])))
            ||m.question.as_ref().is_some_and(|q|q.board.is_empty()||q.task==0||q.revision==0||*key!=digest(&json!(["question",q.board,q.task,q.revision])))) {
        return Err("Message marks unknown: unsupported metadata; kept".into());
    }
    Ok(store)
}
fn write(install: &Path, store: &Store) -> Res<()> {
    use std::io::Write;
    let path=install.join("messages.json");let stage=path.with_extension("tmp");plain(&path)?;
    let bytes=serde_json::to_vec(store).map_err(|e|e.to_string())?;
    // The setup lock excludes writers; unlink an interrupted stage without applying it or following its target.
    if plain(&stage)? {fs::remove_file(&stage).map_err(|e|format!("Message marks temporary cleanup unknown: {e}"))?;}
    // Exclusive creation never writes through an existing link.
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(&stage).map_err(|e|format!("Message marks temporary file refused; kept: {e}"))?;
    let result=(|| {
        file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
        drop(file);fs::rename(&stage,&path).map_err(|e|e.to_string())
    })();
    if let Err(e)=result {let _=fs::remove_file(stage);return Err(format!("Message marks write unknown: {e}"));}
    if read(install)?!=*store {return Err("Message marks readback unknown".into());}
    Ok(())
}
pub(crate) fn request_deleted(install: &Path, id: &str) -> Res<bool> {
    Ok(read(install)?.deleted.contains(&request_key(id)))
}
pub(crate) fn request_deleting(install:&Path,id:&str)->Res<bool> {
    Ok(read(install)?.marks.values().any(|m|m.deleting&&m.request.as_ref().is_some_and(|r|r.id==id)))
}
/// Draft deletion uses the same content-free ID fence as sent owner-request deletion. Caller holds setup lock.
pub(crate) fn forget_draft(install:&Path,id:&str)->Res<()> {
    let mut store=read(install)?;store.deleted.insert(request_key(id));write(install,&store)
}

fn entries(install: &Path, projects: &[Value], store: &Store) -> Res<(Vec<Value>, Vec<String>)> {
    let mut entries=BTreeMap::new();let mut reasons=BTreeSet::new();
    for p in projects {
        let board=p["issues"].as_str().unwrap_or("");
        let ttl=p["ttl"].as_i64().unwrap_or(crate::TTL);
        if p["complete"]!=true || p["alive"]!=true
            || !p["tasks_age_seconds"].as_i64().zip(p["age"].as_i64()).is_some_and(|(t,a)|crate::snapshot::live(t.saturating_add(a),ttl)) || board.is_empty() || !p["tasks"].is_array() {
            reasons.insert(format!("{}: messages source stale, incomplete or unavailable",p["name"].as_str().unwrap_or("Project")));
        }
        for t in p["tasks"].as_array().into_iter().flatten().filter(|t|t["state"]=="ask") {
            let (Some(task),Some(revision))=(t["n"].as_u64(),t["revision"].as_u64()) else {reasons.insert("Question identity unknown".into());continue};
            if board.is_empty()||task==0||revision==0 {reasons.insert("Question identity unknown".into());continue;}
            let key=digest(&json!(["question",board,task,revision]));
            entries.entry(key.clone()).or_insert_with(||json!({"key":key,"kind":"question","title":t["title"],"project":p["name"],"root":p["root"],"board":board,"task":task,"revision":revision,"state":"ask","pending":true}));
        }
    }
    let (requests,bad)=crate::setup::all(install)?;reasons.extend(bad);
    for r in &requests {
        // Setup sessions have their own UI and are not owner project-message records.
        if r.root.is_none() {continue;}
        let key=digest(&json!(["request",r.host,r.id,r.state]));
        if store.deleted.contains(&request_key(&r.id)) && !store.marks.get(&key).is_some_and(|m|m.deleting) {continue;}
        entries.insert(key.clone(),json!({"key":key,"kind":"request","title":format!("AI request · {}",r.state),"root":r.root,"host":r.host,"id":r.id,"state":r.state,"created":r.created,"request":crate::setup::view(r),"pending":(["launched","failed","interrupted"].contains(&r.state.as_str()))}));
    }
    for (key,mark) in &store.marks {
        if !mark.archived {continue;}
        if let Some(r)=&mark.request {
            let current=requests.iter().find(|q|q.host==r.host&&q.id==r.id);
            entries.entry(key.clone()).or_insert_with(||json!({"key":key,"kind":"request","title":if mark.deleting {"Deletion incomplete"}else{"Archived AI request"},"host":r.host,"id":r.id,"state":current.map_or(r.state.as_str(),|r|r.state.as_str()),"request":current.map(crate::setup::view),"pending":false}));
        }
        if let Some(q)=&mark.question {
            let project=projects.iter().find(|p|p["issues"]==q.board);
            entries.entry(key.clone()).or_insert_with(||json!({"key":key,"kind":"question","title":"Archived question","root":project.map(|p|&p["root"]),"project":project.map(|p|&p["name"]),"board":q.board,"task":q.task,"revision":q.revision,"state":"archived","pending":false}));
        }
    }
    let rows=entries.into_iter().filter_map(|(key,mut row)| {
        if store.deleted.contains(&key) {return None;}
        let mark=store.marks.get(&key);
        row["read"]=json!(mark.is_some_and(|m|m.read));row["archived"]=json!(mark.is_some_and(|m|m.archived));
        row["deleting"]=json!(mark.is_some_and(|m|m.deleting));
        if row["kind"]=="request" && row["read"]==true {row["pending"]=json!(false);}
        Some(row)
    }).collect();
    Ok((rows,reasons.into_iter().collect()))
}
pub(crate) fn snapshot(install: &Path, projects: &[Value]) -> Value {
    let result=crate::setup::locked(install,|| {
        let store=read(install)?;let (entries,reasons)=entries(install,projects,&store)?;
        let count=entries.iter().filter(|r|r["pending"]==true&&r["archived"]!=true).count();
        Ok(json!({"entries":entries,"count":if reasons.is_empty(){json!(count)}else{Value::Null},"reason":reasons.join("; "),"complete":reasons.is_empty()}))
    });
    result.unwrap_or_else(|reason|json!({"entries":[],"count":null,"complete":false,"reason":reason}))
}

fn validate_tree(base: &Path, path: &Path) -> Res<()> {
    if !plain(path)? {return Ok(());}
    if !path.canonicalize().map_err(|e|e.to_string())?.starts_with(base) {return Err("Message deletion escaped setup; kept".into());}
    if path.is_dir() {for child in fs::read_dir(path).map_err(|e|e.to_string())? {validate_tree(base,&child.map_err(|e|e.to_string())?.path())?;}}
    Ok(())
}
fn check_request(install: &Path, r: &Identity, deleting: bool) -> Res<()> {
    if !crate::resources::id(&r.id) {return Err("Invalid message request identity".into());}
    let base=crate::setup::dir(install).canonicalize().map_err(|e|e.to_string())?;
    let file=base.join(format!("{}.json",r.id));
    if plain(&file)? {
        let current=crate::setup::read(&file)?;
        if current.host!=r.host||current.root.is_none() {return Err("Message request ownership changed; kept".into());}
        if ["pending","launching"].contains(&current.state.as_str()) {return Err(format!("Request {} is {}; Delete refused before launch",r.id,current.state));}
    } else if !deleting {return Err("Archived request record is missing; deletion ownership unknown".into());}
    Ok(())
}
fn delete_request(install: &Path, r: &Identity) -> Res<()> {
    check_request(install,r,true)?;
    let base=crate::setup::dir(install).canonicalize().map_err(|e|e.to_string())?;
    let file=base.join(format!("{}.json",r.id));let folder=base.join(&r.id);let stage=file.with_extension("tmp");
    validate_tree(&base,&file)?;validate_tree(&base,&folder)?;validate_tree(&base,&stage)?;
    // Keep the request until its owned directory is gone, so a failed delete remains inspectable.
    if plain(&folder)? {fs::remove_dir_all(&folder).map_err(|e|format!("Request {} folder remains: {e}",r.id))?;}
    if plain(&stage)? {fs::remove_file(&stage).map_err(|e|format!("Request {} temporary record remains: {e}",r.id))?;}
    if plain(&file)? {fs::remove_file(&file).map_err(|e|format!("Request {} record remains: {e}",r.id))?;}
    if plain(&file)?||plain(&folder)?||plain(&stage)? {return Err(format!("Request {} deletion readback unknown",r.id));}
    Ok(())
}

/// Native IPC passes identities only; paths and live state are always resolved again under the launch lock.
pub(crate) fn act(install: &Path, projects: &[Value], action: &str, keys: &[String]) -> Value {
    let result=crate::setup::locked(install,|| {
        if !["read","archive","restore","delete"].contains(&action)||keys.is_empty() {return Err("Invalid message action".into());}
        let mut store=read(install)?;let (rows,_)=entries(install,projects,&store)?;
        let mut outcomes=vec![];let mut deletions:BTreeMap<(String,String),Res<()>>=BTreeMap::new();
        for key in keys.iter().collect::<BTreeSet<_>>() {
            let source=rows.iter().find(|r|r["key"].as_str()==Some(key.as_str()));
            let identity=source.filter(|r|action=="delete"&&r["kind"]=="request"&&r["archived"]==true).map(|r|(r["host"].as_str().unwrap_or("").to_string(),r["id"].as_str().unwrap_or("").to_string()));
            let prior=identity.as_ref().and_then(|id|deletions.get(id));
            let result=if let Some(result)=prior {result.clone()}else{(|| {
                let row=rows.iter().find(|r|r["key"].as_str()==Some(key.as_str())).ok_or("Message changed or unavailable; refresh")?;
                if row["deleting"]==true&&action!="delete" {return Err("Deletion incomplete; retry Delete".into());}
                if action!="delete" {
                    let mark=store.marks.entry(key.clone()).or_default();
                    if action=="read" {mark.read=true;}else if action=="restore" {mark.archived=false;}else if !mark.archived {
                        mark.archived=true;
                        if row["kind"]=="request" {mark.request=Some(Identity{host:row["host"].as_str().unwrap_or("").into(),id:row["id"].as_str().unwrap_or("").into(),state:row["state"].as_str().unwrap_or("").into()});}
                        else {mark.question=Some(Question{board:row["board"].as_str().unwrap_or("").into(),task:row["task"].as_u64().unwrap_or(0),revision:row["revision"].as_u64().unwrap_or(0)});}
                    }
                    return write(install,&store);
                }
                if row["archived"]!=true {return Err("Delete requires an archived message".into());}
                if row["kind"]=="request" {
                    let mark=store.marks.get_mut(key).ok_or("Archived message metadata missing")?;
                    let r=mark.request.clone().ok_or("Archived request identity missing")?;
                    check_request(install,&r,mark.deleting)?;
                    crate::conversations::preserve_draft(install,&r.id)?;
                    mark.deleting=true;
                    store.deleted.insert(request_key(&r.id));write(install,&store)?;
                    delete_request(install,&r)?;
                    crate::conversations::purge(install,&r.id)?;
                    store.marks.retain(|_,m|m.request.as_ref().is_none_or(|q|q.id!=r.id));
                } else {store.deleted.insert(key.clone());}
                store.marks.remove(key);write(install,&store)
            })()};
            if let Some(id)=identity {deletions.insert(id,result.clone());}
            outcomes.push(match result {Ok(())=>json!({"key":key,"state":"accepted","action":action}),Err(reason)=>json!({"key":key,"state":"unknown","action":action,"reason":reason})});
        }
        Ok(json!({"outcomes":outcomes}))
    });
    result.unwrap_or_else(|reason|json!({"outcomes":[],"state":"unknown","reason":reason}))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new()->Self {let path=std::env::temp_dir().join(format!("taskq-messages-{}",crate::runtime::uuid()));fs::create_dir_all(path.join("setup")).unwrap();Self(path)}
        fn request(&self,id:&str,state:&str)->crate::setup::Request {
            let r=crate::setup::Request{schema:2,id:id.into(),created:1,host:"fixture".into(),platform:"invented".into(),client:"invented".into(),runtime:"claude".into(),targets:vec![],brief:"invented private request".into(),state:state.into(),job:Some("native-job-kept".into()),session:None,url:None,reason:None,root:Some(self.0.display().to_string()),pm:false};
            fs::write(self.0.join(format!("setup/{id}.json")),serde_json::to_vec(&r).unwrap()).unwrap();
            fs::create_dir_all(self.0.join(format!("setup/{id}/attachments"))).unwrap();
            fs::write(self.0.join(format!("setup/{id}/attachments/copy.txt")),"owned copy").unwrap();r
        }
        fn key(&self,id:&str)->String {snapshot(&self.0,&[])["entries"].as_array().unwrap().iter().find(|r|r["id"]==id).unwrap()["key"].as_str().unwrap().into()}
        fn action(&self,action:&str,key:&str)->Value {act(&self.0,&[],action,&[key.into()])}
    }
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
    fn project(revision:u64)->Value {json!({"root":"invented-root","name":"Fixture","issues":"https://example.org/fixture/issues","alive":true,"complete":true,"age":1,"tasks_age_seconds":1,"tasks":[{"n":7,"title":"Invented question","revision":revision,"state":"ask"}]})}

    // A1/A6: duplicate sources, read/unanswered, new revisions, restart, archive and deletion suppression.
    #[test]
    fn pending_messages_keep_identity_and_unknown_separate_from_zero() {
        let f=Fixture::new();f.request("outcome","launched");f.request("starting","pending");
        let projects=[project(9),project(9)];let s=snapshot(&f.0,&projects);assert_eq!(s["count"],2);assert_eq!(s["entries"].as_array().unwrap().len(),3);
        let question=s["entries"].as_array().unwrap().iter().find(|r|r["kind"]=="question").unwrap()["key"].as_str().unwrap().to_string();
        for key in [&question,&f.key("outcome")] {assert_eq!(act(&f.0,&projects,"read",&[key.into()])["outcomes"][0]["state"],"accepted");}
        assert_eq!(snapshot(&f.0,&projects)["count"],1,"reading does not answer a board question");
        assert_eq!(act(&f.0,&projects,"archive",&[question.clone()])["outcomes"][0]["state"],"accepted");
        assert_eq!(snapshot(&f.0,&projects)["count"],0);
        assert_eq!(act(&f.0,&projects,"delete",&[question])["outcomes"][0]["state"],"accepted");
        assert_eq!(snapshot(&f.0,&projects)["count"],0,"deleted question never resurrects on fresh read");
        assert_eq!(snapshot(&f.0,&[project(10)])["count"],1,"new revision is a new message");
        let mut live=project(10);live["age"]=json!(61);assert_eq!(snapshot(&f.0,&[live])["complete"],true,"#1591: 61 s is within ttl");
        let mut stale=project(10);stale["age"]=json!(301);let s=snapshot(&f.0,&[stale]);assert!(s["count"].is_null());assert_eq!(s["complete"],false);
        fs::write(f.0.join("setup/corrupt.json"),"broken").unwrap();assert!(snapshot(&f.0,&[])["count"].is_null());
        fs::write(f.0.join("messages.json"),"broken kept metadata").unwrap();assert_eq!(f.action("archive","invalid")["state"],"unknown");
        assert_eq!(fs::read_to_string(f.0.join("messages.json")).unwrap(),"broken kept metadata");
    }

    // A3–A6: real deletion/readback; no board writes, native execution control or recovery copy.
    #[test]
    fn archive_any_delete_only_archived_and_keep_live_work_and_same_id_fence() {
        let f=Fixture::new();let live_record=f.request("live","launched");f.request("waiting","pending");
        fs::write(f.0.join("setup/live.tmp"),serde_json::to_vec(&live_record).unwrap()).unwrap();
        fs::write(f.0.join("native-session"),"live work").unwrap();fs::write(f.0.join("source.txt"),"source original").unwrap();
        let live=f.key("live");let pending=f.key("waiting");
        assert_eq!(f.action("delete",&live)["outcomes"][0]["state"],"unknown");
        for key in [&live,&pending] {assert_eq!(f.action("archive",key)["outcomes"][0]["state"],"accepted");}
        let result=act(&f.0,&[],"delete",&[pending.clone(),live.clone()]);
        assert_eq!(result["outcomes"].as_array().unwrap().iter().filter(|r|r["state"]=="accepted").count(),1);
        assert!(!f.0.join("setup/live.json").exists());assert!(!f.0.join("setup/live").exists());
        assert!(!f.0.join("setup/live.tmp").exists(),"no staged history copy remains");
        assert!(f.0.join("setup/waiting.json").exists());assert!(f.0.join("setup/waiting").exists());
        assert_eq!(fs::read_to_string(f.0.join("native-session")).unwrap(),"live work");assert_eq!(fs::read_to_string(f.0.join("source.txt")).unwrap(),"source original");
        let store=fs::read_to_string(f.0.join("messages.json")).unwrap();assert!(!store.contains("invented private request"));assert!(!store.contains("live"));
        assert!(crate::setup::open_id(&f.0,&f.0,"replacement work","live","claude",false).unwrap_err().contains("deleted"));
        assert_eq!(snapshot(&f.0,&[])["entries"].as_array().unwrap().len(),1);
        crate::platform::sweep(&f.0,&f.0.join("unused-shim"));
        assert!(f.0.join("messages.json").exists());assert!(f.0.join("setup/waiting.json").exists(),"update preserves pending work and marks");
    }

    // A6: a partial filesystem refusal remains retryable and is never reported deleted.
    #[test]
    fn failed_delete_keeps_identity_and_retry_without_history_backup() {
        let f=Fixture::new();f.request("partial","failed");let key=f.key("partial");f.action("archive",&key);
        fs::remove_dir_all(f.0.join("setup/partial")).unwrap();fs::write(f.0.join("setup/partial"),"unexpected file kept").unwrap();
        assert_eq!(f.action("delete",&key)["outcomes"][0]["state"],"unknown");
        assert!(f.0.join("setup/partial.json").exists());assert_eq!(snapshot(&f.0,&[])["entries"][0]["deleting"],true);
        fs::remove_file(f.0.join("setup/partial")).unwrap();
        assert_eq!(f.action("delete",&key)["outcomes"][0]["state"],"accepted");assert!(snapshot(&f.0,&[])["entries"].as_array().unwrap().is_empty());
    }

    // A6: restart discards an interrupted stage without applying it over committed marks.
    #[test]
    fn interrupted_metadata_stage_does_not_block_the_next_write() {
        let f=Fixture::new();f.request("stale-stage","launched");let key=f.key("stale-stage");
        assert_eq!(f.action("read",&key)["outcomes"][0]["state"],"accepted");
        let committed=fs::read(f.0.join("messages.json")).unwrap();
        for stale in [b"partial write".as_slice(), committed.as_slice()] {
            fs::write(f.0.join("messages.tmp"),stale).unwrap();
            assert_eq!(snapshot(&f.0,&[])["entries"][0]["read"],true);
            assert_eq!(f.action("archive",&key)["outcomes"][0]["state"],"accepted");
            assert!(!f.0.join("messages.tmp").exists());
            assert_eq!(snapshot(&f.0,&[])["entries"][0]["archived"],true);
        }
    }

    // TW7: stale-stage cleanup never writes through a hard link or follows a symbolic link.
    #[test]
    fn existing_temporary_path_cannot_overwrite_source_data() {
        let f=Fixture::new();f.request("temp-path","launched");let key=f.key("temp-path");
        let source=f.0.join("source.txt");fs::write(&source,"source bytes kept").unwrap();
        fs::hard_link(&source,f.0.join("messages.tmp")).unwrap();
        assert_eq!(f.action("archive",&key)["outcomes"][0]["state"],"accepted");
        assert_eq!(fs::read_to_string(&source).unwrap(),"source bytes kept");
        assert!(!f.0.join("messages.tmp").exists());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&source,f.0.join("messages.tmp")).unwrap();
            assert_eq!(f.action("read",&key)["outcomes"][0]["state"],"unknown");
            assert_eq!(fs::read_to_string(&source).unwrap(),"source bytes kept");
            assert!(f.0.join("messages.tmp").is_symlink());
        }
    }

    // Delete all returns verified success for every selected state identity of the same request.
    #[test]
    fn deleting_archived_aliases_in_one_batch_reports_each_absence() {
        let f=Fixture::new();f.request("aliases","pending");let starting=f.key("aliases");f.action("archive",&starting);
        f.request("aliases","launched");let launched=snapshot(&f.0,&[])["entries"].as_array().unwrap().iter().find(|r|r["archived"]!=true).unwrap()["key"].as_str().unwrap().to_string();
        f.action("archive",&launched);
        let result=act(&f.0,&[],"delete",&[starting,launched]);
        assert!(result["outcomes"].as_array().unwrap().iter().all(|r|r["state"]=="accepted"),"{result}");
        assert!(snapshot(&f.0,&[])["entries"].as_array().unwrap().is_empty());
        assert!(!f.0.join("setup/aliases.json").exists());assert!(!f.0.join("setup/aliases").exists());
    }

    // Archive identities survive a board answer or a launch state change without keeping message text.
    #[test]
    fn archived_sources_survive_changes_and_delete_current_owned_record() {
        let f=Fixture::new();f.request("changing","pending");let key=f.key("changing");f.action("archive",&key);
        let question=snapshot(&f.0,&[project(9)])["entries"].as_array().unwrap().iter().find(|r|r["kind"]=="question").unwrap()["key"].as_str().unwrap().to_string();
        act(&f.0,&[project(9)],"archive",&[question.clone()]);
        f.request("changing","launched");let s=snapshot(&f.0,&[]);
        assert_eq!(s["entries"].as_array().unwrap().iter().filter(|r|r["archived"]==true).count(),2);
        assert_eq!(s["count"],1,"new unread outcome has its own identity");
        assert_eq!(f.action("delete",&key)["outcomes"][0]["state"],"accepted");
        assert_eq!(f.action("delete",&question)["outcomes"][0]["state"],"accepted");
        assert!(snapshot(&f.0,&[project(9)])["entries"].as_array().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn junction_attachment_is_refused_without_touching_its_target() {
        let f=Fixture::new();f.request("junction","launched");
        let target=f.0.join("native-chat");fs::create_dir(&target).unwrap();fs::write(target.join("keep.txt"),"native chat kept").unwrap();
        let link=f.0.join("setup/junction/attachments/linked");
        let output=std::process::Command::new("cmd.exe").args(["/d","/c","mklink","/J"]).arg(link.to_string_lossy().replace('/',"\\")).arg(target.to_string_lossy().replace('/',"\\")).output().unwrap();
        assert!(output.status.success(),"owned junction fixture unavailable: {}",String::from_utf8_lossy(&output.stderr));
        let key=f.key("junction");f.action("archive",&key);
        assert_eq!(f.action("delete",&key)["outcomes"][0]["state"],"unknown");
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(),"native chat kept");
        fs::remove_dir(link).unwrap();
    }

    // A6/TW7: recovery and a late callback cannot recreate a record deleted during an uncertain launch.
    #[test]
    fn concurrent_launch_outcome_cannot_resurrect_deleted_history() {
        let f=Fixture::new();f.request("racing","pending");
        fn launch(own:&Path,_:&crate::setup::Request)->crate::setup::Launched {
            let install=own.parent().unwrap().parent().unwrap();
            let key=snapshot(install,&[])["entries"].as_array().unwrap().iter().find(|r|r["archived"]!=true).unwrap()["key"].as_str().unwrap().to_string();
            assert_eq!(act(install,&[],"archive",&[key.clone()])["outcomes"][0]["state"],"accepted");
            assert_eq!(act(install,&[],"delete",&[key])["outcomes"][0]["state"],"unknown","launching refuses deletion");
            crate::setup::recover(install).unwrap();
            let key=snapshot(install,&[])["entries"].as_array().unwrap().iter().find(|r|r["archived"]!=true).unwrap()["key"].as_str().unwrap().to_string();
            act(install,&[],"archive",&[key.clone()]);assert_eq!(act(install,&[],"delete",&[key])["outcomes"][0]["state"],"accepted");
            crate::setup::Launched::Job{job:"still-running".into(),session:None,url:None}
        }
        assert!(crate::setup::step(&f.0,launch).is_err());assert!(!f.0.join("setup/racing.json").exists());
        assert!(snapshot(&f.0,&[])["entries"].as_array().unwrap().is_empty());
    }

    // A5/A6: recursive deletion never follows a link to source/native data.
    #[cfg(unix)]
    #[test]
    fn linked_attachments_and_setup_directory_are_refused() {
        use std::os::unix::fs::symlink;
        let f=Fixture::new();f.request("linked","launched");fs::write(f.0.join("source.txt"),"keep source").unwrap();
        symlink(f.0.join("source.txt"),f.0.join("setup/linked/attachments/link")).unwrap();
        let key=f.key("linked");f.action("archive",&key);assert_eq!(f.action("delete",&key)["outcomes"][0]["state"],"unknown");
        assert_eq!(fs::read_to_string(f.0.join("source.txt")).unwrap(),"keep source");
        fs::rename(f.0.join("setup"),f.0.join("other")).unwrap();symlink(f.0.join("other"),f.0.join("setup")).unwrap();
        assert_eq!(snapshot(&f.0,&[])["complete"],false);assert!(crate::setup::recover(&f.0).is_err());
    }
}
