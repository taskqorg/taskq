    use super::*;
    use crate::{board::{Fake,entries}, dispatch::{self,Op}};
    struct Fixture(PathBuf);
    impl Fixture {fn new()->Self{let p=std::env::temp_dir().join(format!("taskq-reply-test-{}",crate::runtime::uuid()));fs::create_dir(&p).unwrap();Self(p)}}
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}

    // Chain: §5 PilotQ downloaded photo/document -> CLI dispatcher -> board -> worker-readable files.
    // Risk: old C3 caps, missing binary bytes, duplicated commands, or stale replies that still upload media.
    #[test]
    fn pilotq_downloaded_media_reaches_the_worker_with_retry_and_fencing() {
        let fixture=Fixture::new();
        let photo=fixture.0.join("photo.png");let document=fixture.0.join("document.pdf");
        fs::write(&photo,include_bytes!("../tests/fixtures/reply-photo.png")).unwrap();
        fs::write(&document,vec![0xab;20*1024*1024]).unwrap();
        let paths=vec![photo.display().to_string(),document.display().to_string()];
        for (kind,text) in [("answer",""),("steer","mixed text and files"),("requeue","Reason: use these files")] {
            let b=Fake::default();let n=b.create("Invented PilotQ media reply","",&["taskq".into()]).unwrap();
            let attempt=b.comment(n,&core::entry("spawn",&["worker","claude:fixture","invented-host"],None,"")).unwrap();
            if kind=="answer"{b.comment(n,&core::entry("ask",&[],Some(attempt),"Problem: send the document")).unwrap();}
            let op=Op{id:Some(format!("pilotq-message-{kind}")),revision:Some(core::revision(n,&entries(&b,n).unwrap()))};
            let receipt=dispatch::act_files(&b,&fixture.0,n,kind,text,&op,&paths);
            assert!(receipt.ok(),"{}",receipt.text());
            let log=entries(&b,n).unwrap();let files=manifest(&log.last().unwrap().body).unwrap();assert_eq!(files.len(),2);
            assert_eq!(files[1].bytes,20*1024*1024);assert_eq!(files[1].name,"document.pdf");
            let worker=prepare(&b,&fixture.0,n,&log).unwrap();assert!(worker.contains("untrusted data"));
            for (file,original) in files.iter().zip(&paths){let path=crate::dot(&fixture.0).join("reply-files").join(&file.hash).join(&file.name);assert_eq!(fs::read(&path).unwrap(),fs::read(original).unwrap());assert!(worker.contains(&path.to_string_lossy().replace('\\',"\\\\")));}
            let (comments,uploads)=(b.comments(n).unwrap().len(),b.state.borrow().images.len());
            assert!(dispatch::act_files(&b,&fixture.0,n,kind,text,&op,&["missing-file".into()]).ok(),"accepted op reconciles before reading original files");
            assert_eq!((b.comments(n).unwrap().len(),b.state.borrow().images.len()),(comments,uploads));
            let stale=Op{id:Some("other-message".into()),revision:Some(0)};
            let r=dispatch::act_files(&b,&fixture.0,n,kind,text,&stale,&paths);
            if kind=="requeue"{assert_eq!(r.outcome,"stale");assert_eq!(b.state.borrow().images.len(),uploads,"stale requeue uploads nothing");}
            else{assert!(r.ok());assert_eq!(r.note,Some(true),"late replies retain files as a note without answering another question");}
            fs::write(crate::dot(&fixture.0).join("reply-files").join(&files[0].hash).join(&files[0].name),b"corrupt cache").unwrap();
            assert!(prepare(&b,&fixture.0,n,&log).is_ok(),"a corrupt cache is repaired only from verified board bytes");
            fs::remove_file(crate::dot(&fixture.0).join("reply-files").join(&files[0].hash).join(&files[0].name)).unwrap();
            assert!(prepare(&b,&fixture.0,n,&log).is_ok(),"cache reconstructs from board without the original client");
        }
    }

    // Chain: §5 same answer text plus new files -> durable note. Risk: text-only deduplication drops attachments.
    #[test]
    fn new_files_keep_a_repeated_answer_distinct() {
        let fixture=Fixture::new();let photo=fixture.0.join("photo.png");
        fs::write(&photo,include_bytes!("../tests/fixtures/reply-photo.png")).unwrap();
        let b=Fake::default();let n=b.create("Invented repeated reply","",&["taskq".into()]).unwrap();
        let attempt=b.comment(n,"taskq: spawn worker claude:fixture invented-host").unwrap();
        b.comment(n,&core::entry("ask",&[],Some(attempt),"Problem: send the file")).unwrap();
        let revision=core::revision(n,&entries(&b,n).unwrap());
        assert!(dispatch::act(&b,&fixture.0,n,"answer",&[],"Same text",&Op::default()).ok());
        let op=Op{id:Some("reply-with-new-photo".into()),revision:Some(revision)};
        let receipt=dispatch::act_files(&b,&fixture.0,n,"answer","Same text",&op,&[photo.display().to_string()]);
        assert!(receipt.ok(),"{}",receipt.text());
        assert_eq!(receipt.note,Some(true),"new attachments must survive a repeated answer's text");
        let log=entries(&b,n).unwrap();assert_eq!(manifest(&log.last().unwrap().body).unwrap().len(),1);
        assert!(prepare(&b,&fixture.0,n,&log).unwrap().contains("photo.png"));
        let count=b.comments(n).unwrap().len();
        assert!(dispatch::act_files(&b,&fixture.0,n,"answer","Same text",&op,&["missing".into()]).ok());
        assert_eq!(b.comments(n).unwrap().len(),count,"same-operation retry writes nothing");
    }

    // Chain: §5 paste/drop native copy -> saved reply -> upload refusal. Risk: lost draft or partial media moves task.
    #[test]
    fn private_copy_and_refused_upload_preserve_the_draft_and_task() {
        let fixture=Fixture::new();let install=fixture.0.join("install");fs::create_dir(&install).unwrap();
        let data=b"invented binary\0data";
        let m=serde_json::json!({"file":{"id":"file-1","name":"file.bin","mime":"application/octet-stream","bytes":data.len()},"data":STANDARD.encode(data)});
        let file=store_file(&install,&m).unwrap();
        let local=serde_json::json!({"draft":"kept text","files":[file],"op":null});
        assert_eq!(store_local(&install,&local).unwrap(),local);
        let mut unicode=m.clone();unicode["file"]["name"]=Value::String("отчёт [final].bin".into());
        let unicode=store_file(&install,&unicode).unwrap();let paths=draft_paths(&install,&serde_json::json!([unicode.clone()])).unwrap();
        assert_eq!(selected(&paths).unwrap()[0].1.name,unicode.name,"the native copy must keep the original Unicode name");
        assert!(append("",&[File{url:"https://board.example/file".into(),..unicode}]).contains("отчёт \\[final\\].bin"));
        let paths=draft_paths(&install,&local["files"]).unwrap();assert_eq!(fs::read(&paths[0]).unwrap(),data);
        let b=Fake::default();let n=b.create("refused media","",&["taskq".into()]).unwrap();b.comment(n,"taskq: spawn worker claude:s invented-host").unwrap();
        let before=b.comments(n).unwrap().len();b.state.borrow_mut().bad_image=true;
        let r=dispatch::act_files(&b,&fixture.0,n,"steer","kept text",&Op::default(),&paths);
        assert_eq!(r.outcome,"rejected");assert!(r.reason.contains("checksum differs"));assert_eq!(b.comments(n).unwrap().len(),before);
        assert_eq!(fs::read(&paths[0]).unwrap(),data);assert_eq!(store_local(&install,&local).unwrap(),local);
        assert!(dispatch::act_files(&b,&fixture.0,n,"steer",MARKER,&Op::default(),&paths).reason.contains("reserved"));
        let mut many=local.clone();many["files"]=serde_json::json!(vec![file;100]);assert!(store_local(&install,&many).is_ok(),"no file-count cap");
        for bad in ["../escape",""] {let mut m=m.clone();m["file"]["id"]=Value::String(bad.into());assert!(store_file(&install,&m).is_err());}
        for bad in ["../escape",".",".."] {let mut m=m.clone();m["file"]["name"]=Value::String(bad.into());assert!(store_file(&install,&m).is_err());}
    }
