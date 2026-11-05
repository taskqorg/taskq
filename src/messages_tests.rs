    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new()->Self {let path=std::env::temp_dir().join(format!("taskq-messages-{}",crate::runtime::uuid()));fs::create_dir_all(path.join("setup")).unwrap();Self(path)}
        fn request(&self,id:&str,state:&str)->crate::setup::Request {
            let r=crate::setup::Request{schema:2,id:id.into(),created:1,host:"fixture".into(),platform:"invented".into(),client:"invented".into(),runtime:"claude".into(),targets:vec![],brief:"invented private request".into(),state:state.into(),job:Some("native-job-kept".into()),session:None,url:None,reason:None,root:Some(self.0.display().to_string()),pm:false,choice:None};
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
