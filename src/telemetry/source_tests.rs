    use super::*;
    use rusqlite::OptionalExtension;
    const LINE:&str=r#"taskq-profile {"op":"spawn","us":10,"error":"none","at_ms":null}"#;
    fn dir() -> std::path::PathBuf {
        let dir=std::env::temp_dir().join(format!("taskq-source-{}-{}",std::process::id(),crate::runtime::uuid()));std::fs::create_dir(&dir).unwrap();dir
    }
    fn now() -> i64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64}
    fn add(db: &Connection) {db.execute("INSERT INTO records(retained_ms,line) VALUES(?1,?2)",params![now(),LINE]).unwrap();}
    #[test]
    fn source_import_refuses_undrained_or_rewritten_legacy_text_before_identity_or_target_changes() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);
        let target=Store::open(&dir.join("common")).unwrap();
        std::fs::write(dir.join("taskq.log"),format!("{LINE}\n")).unwrap();
        assert!(import(&mut archive,&target,&dir).is_err(),"undrained text must refuse source import");
        assert_eq!(target.import_cursor(SOURCE).unwrap(),None);
        assert!(!archive.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='telemetry_source')",[],|r|r.get::<_,bool>(0)).unwrap());
        let partial=serde_json::json!({"bytes":LINE.len(),"sha256":super::super::hash(LINE),"trimmed_bytes":0,"trimmed_sha256":super::super::hash("")});
        archive.execute("INSERT INTO legacy(id,text) VALUES(1,?1)",[partial.to_string()]).unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("rewritten"));
        super::super::import(&mut archive,&dir,now() as u64).unwrap();
        let state:serde_json::Value=serde_json::from_str(&archive.query_row("SELECT text FROM legacy WHERE id=1",[],|r|r.get::<_,String>(0)).unwrap()).unwrap();
        assert_eq!(state["gaps"],1);
        assert_eq!(import(&mut archive,&target,&dir).unwrap(),2);
        let before=target.export(0,0,1024).unwrap();
        std::fs::write(dir.join("taskq.log"),"rewritten\n").unwrap();
        assert!(import(&mut archive,&target,&dir).is_err(),"rewritten text must refuse source import");
        assert_eq!(target.export(0,0,1024).unwrap(),before);
        drop(target);drop(archive);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rotated_text_checkpoint_accepts_only_drained_rows_and_preserves_receipt_clocks() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();let log=dir.join("taskq.log");
        let original=format!("{LINE}\n{LINE}\n");std::fs::write(&log,&original).unwrap();
        let received=now()-10_000;super::super::import(&mut archive,&dir,received as u64).unwrap();
        let target=Store::open(&dir.join("common")).unwrap();
        std::fs::write(&log,format!("{LINE}\nservice diagnostic\n")).unwrap();
        assert_eq!(import(&mut archive,&target,&dir).unwrap(),2);
        let before=target.export(0,0,1024).unwrap();
        for row in before["events"].as_array().unwrap() {assert_eq!(row["received_ms"],received);}
        let checkpoint:String=archive.query_row("SELECT text FROM legacy WHERE id=1",[],|r|r.get(0)).unwrap();
        assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);
        std::fs::write(&log,format!("{LINE}\nservice diagnostic\n{LINE}\n")).unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("undrained"));
        assert_eq!(target.export(0,0,1024).unwrap(),before);
        assert_eq!(archive.query_row("SELECT text FROM legacy WHERE id=1",[],|r|r.get::<_,String>(0)).unwrap(),checkpoint);
        std::fs::write(&log,format!("{LINE}\n{LINE}")).unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("truncated"));
        assert_eq!(target.export(0,0,1024).unwrap(),before);
        drop(target);drop(archive);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn identical_observations_survive_sequence_reuse_and_old_writers() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);add(&archive);
        let older=super::super::open(&dir).unwrap();let mut insert=older.prepare("INSERT INTO records(retained_ms,line) VALUES(?1,?2)").unwrap();
        let target=Store::open(&dir.join("common")).unwrap();assert_eq!(import(&mut archive,&target,&dir).unwrap(),2);assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);
        let identity=state(&archive).unwrap().namespace;let first=target.export(0,0,1024).unwrap();assert_ne!(first["events"][0]["id"],first["events"][1]["id"]);
        archive.execute("DELETE FROM records WHERE seq=2",[]).unwrap();insert.execute(params![now(),LINE]).unwrap();assert_eq!(import(&mut archive,&target,&dir).unwrap(),1);
        assert_eq!(archive.query_row("SELECT telemetry_epoch FROM records WHERE seq=2",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        archive.execute("DELETE FROM records",[]).unwrap();insert.execute(params![now(),LINE]).unwrap();assert_eq!(import(&mut archive,&target,&dir).unwrap(),1);
        let report=target.export(0,0,1024).unwrap();assert_eq!(report["events"].as_array().unwrap().len(),4);let ids=report["events"].as_array().unwrap().iter().map(|row|row["id"].as_str().unwrap()).collect::<std::collections::HashSet<_>>();assert_eq!(ids.len(),4);
        assert_eq!(state(&archive).unwrap().namespace,identity);assert_eq!(archive.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0)).unwrap(),1);
        drop(insert);drop(older);drop(archive);let mut archive=super::super::open(&dir).unwrap();assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);
        assert_eq!(super::super::read(&dir,now() as u64).unwrap(),format!("{LINE}\n"));drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn large_summary_crosses_batch_boundary_atomically_and_restores_every_counter() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();
        {let tx=archive.transaction().unwrap();for _ in 0..1023 {add(&tx);}tx.commit().unwrap();}
        let ops=(0..1200).map(|mut i|{let mut route=String::from("route");for _ in 0..4 {route.push((b'a'+(i%26) as u8) as char);i/=26;}(format!("GET issues/N/{route}"),serde_json::json!(1))}).collect::<serde_json::Map<_,_>>();
        let logical=serde_json::json!({"at_ms":1000,"source":"cli status","ops":ops});let line=format!("taskq-requests {logical}");assert!(line.len()>16<<10&&line.len()<=super::super::LINE_MAX);let received=now()-1000;
        archive.execute("INSERT INTO records(retained_ms,line) VALUES(?1,?2)",params![received,line]).unwrap();install_identity(&mut archive).unwrap();
        let state=state(&archive).unwrap();let parts=Event::split_identified(&format!("taskq.legacy.{}",state.namespace),"0.1024",super::super::semantic::observation(&line).unwrap()).unwrap();assert!(parts.len()>1);
        let path=dir.join("common");let target=Store::open(&path).unwrap();let inject=Connection::open(&path).unwrap();
        inject.execute_batch(&format!("CREATE TRIGGER reject_summary BEFORE INSERT ON events WHEN new.id='{}' BEGIN SELECT RAISE(ABORT,'injected summary part failure'); END",parts.last().unwrap().id())).unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("injected summary part failure"));assert!(target.import_cursor(SOURCE).unwrap().unwrap().ends_with(".0.1023"));
        assert_eq!(target.export(0,0,1024).unwrap()["events"].as_array().unwrap().len(),1023,"no partial summary or cursor can commit");
        inject.execute_batch("DROP TRIGGER reject_summary").unwrap();drop(inject);assert_eq!(import(&mut archive,&target,&dir).unwrap(),1);assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);
        let mut values=Vec::new();let mut after=0;loop {let page=target.export(0,after,1024).unwrap();for row in page["events"].as_array().unwrap() {if row["observation"].get("group_id").is_some() {assert_eq!(row["received_ms"],received);}values.push(row["observation"].clone());}after=page["next"].as_i64().unwrap();if page["more"]==false {break;}}
        assert_eq!(values.len(),1023+parts.len());assert!(super::super::semantic::line(&values[1023]).is_err());
        let restored=super::super::semantic::lines(&values).unwrap();assert_eq!(restored.len(),1024);assert_eq!(crate::requests::legacy_value(restored.last().unwrap()).unwrap(),crate::requests::legacy_value(&line).unwrap());
        assert!(super::super::semantic::lines(&values[..values.len()-1]).is_err());
        drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn removed_unimported_records_and_mutation_are_explicit_unknowns() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);add(&archive);install_identity(&mut archive).unwrap();let target=Store::open(&dir.join("common")).unwrap();
        archive.execute("DELETE FROM records WHERE seq=2",[]).unwrap();assert!(import(&mut archive,&target,&dir).unwrap_err().contains("unimported records pruned"));assert_eq!(target.import_cursor(SOURCE).unwrap(),None);
        archive.execute("UPDATE records SET line=?1 WHERE seq=1",[LINE.replace("10","11")]).unwrap();assert!(import(&mut archive,&target,&dir).unwrap_err().contains("mutated"));
        assert_eq!(target.export(0,0,10).unwrap()["events"],serde_json::json!([]));drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn replacement_and_rewound_identity_never_resume_another_stream() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);add(&archive);let target=Store::open(&dir.join("common")).unwrap();import(&mut archive,&target,&dir).unwrap();let before=target.export(0,0,10).unwrap();
        let replacement=dir.join("replacement");std::fs::create_dir(&replacement).unwrap();let mut other=super::super::open(&replacement).unwrap();add(&other);
        assert!(import(&mut other,&target,&replacement).unwrap_err().contains("replaced archive identity"));
        archive.execute("UPDATE telemetry_source SET high_seq=1",[]).unwrap();assert!(import(&mut archive,&target,&dir).unwrap_err().contains("moved backwards"));assert_eq!(target.export(0,0,10).unwrap(),before);
        drop(other);drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn late_target_failure_reuses_the_durable_source_namespace_and_cursor() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);add(&archive);let path=dir.join("common");let target=Store::open(&path).unwrap();
        let inject=Connection::open(&path).unwrap();inject.execute_batch("CREATE TABLE imports(source TEXT PRIMARY KEY,cursor TEXT NOT NULL,receipt TEXT NOT NULL); CREATE TRIGGER refuse_cursor BEFORE INSERT ON imports BEGIN SELECT RAISE(ABORT,'injected source progress failure'); END").unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("injected source progress failure"));let identity=state(&archive).unwrap().namespace;
        assert_eq!(target.import_cursor(SOURCE).unwrap(),None);assert_eq!(target.export(0,0,10).unwrap()["events"],serde_json::json!([]));
        inject.execute_batch("DROP TRIGGER refuse_cursor").unwrap();drop(inject);drop(archive);let mut archive=super::super::open(&dir).unwrap();
        assert_eq!(import(&mut archive,&target,&dir).unwrap(),2);assert_eq!(state(&archive).unwrap().namespace,identity);assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);
        drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore="source identity qualification on frozen TASKQ_SEMANTIC_SOURCE and TASKQ_SEMANTIC_LOG snapshots"]
    fn retained_source_import_replays_and_preserves_all_reports_and_receipt_clocks() {
        let snapshot=std::path::PathBuf::from(std::env::var_os("TASKQ_SEMANTIC_SOURCE").expect("explicit snapshot"));
        let mut wal=snapshot.as_os_str().to_os_string();wal.push("-wal");assert!(std::fs::metadata(std::path::Path::new(&wal)).map_or(true,|m|m.len()==0),"snapshot must have no pending WAL");
        let dir=dir();std::fs::create_dir(dir.join("telemetry.log")).unwrap();std::fs::copy(&snapshot,dir.join("telemetry.log/telemetry.sqlite3")).unwrap();
        let log=std::env::var_os("TASKQ_SEMANTIC_LOG").expect("explicit matching frozen taskq.log");std::fs::copy(log,dir.join("taskq.log")).unwrap();
        let mut archive=super::super::open(&dir).unwrap();let common=dir.join("common");let mut expected_common=None;
        if let Some(snapshot)=std::env::var_os("TASKQ_SEMANTIC_COMMON") {let snapshot=std::path::PathBuf::from(snapshot);let mut wal=snapshot.as_os_str().to_os_string();wal.push("-wal");assert!(std::fs::metadata(std::path::Path::new(&wal)).map_or(true,|m|m.len()==0));std::fs::copy(snapshot,&common).unwrap();expected_common=Some(std::env::var("TASKQ_SEMANTIC_COMMON_SHA").expect("explicit original common export SHA256"));}
        let target=Store::open(&common).unwrap();let mut original_ids=std::collections::HashSet::new();let mut original_hash=ring::digest::Context::new(&ring::digest::SHA256);let mut after=0;
        loop {let page=target.export(0,after,1024).unwrap();for row in page["events"].as_array().unwrap() {assert!(!super::super::semantic::KINDS.contains(&row["observation"]["kind"].as_str().unwrap()));original_ids.insert(row["id"].as_str().unwrap().to_owned());original_hash.update((row.to_string()+"\n").as_bytes());}after=page["next"].as_i64().unwrap();if page["more"]==false {break;}}
        let original_hash=original_hash.finish();if let Some(expected)=expected_common {assert_eq!(original_hash.as_ref().iter().map(|byte|format!("{byte:02x}")).collect::<String>(),expected,"schema upgrade preserves the original common export");}
        let mut expected=ring::digest::Context::new(&ring::digest::SHA256);let mut text=String::new();let mut count=0;
        let canonical=|line:&str|if line.starts_with("taskq-requests ") {crate::requests::legacy_value(line)} else {crate::profile::legacy_value(line)};
        {let mut read=archive.prepare("SELECT retained_ms,line FROM records ORDER BY seq").unwrap();let rows=read.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).unwrap();
        for row in rows {let (received,line)=row.unwrap();expected.update(format!("{received} {} {}\n",line.split_once(' ').unwrap().0,canonical(&line).unwrap()).as_bytes());text.push_str(&line);text.push('\n');count+=1;}}
        let legacy:Option<String>=archive.query_row("SELECT text FROM legacy WHERE id=1",[],|r|r.get(0)).optional().unwrap();
        let at=now() as u64;let reports=crate::profile::tests::legacy_reports(&text,at).unwrap();
        assert_eq!(import(&mut archive,&target,&dir).unwrap(),count);let cursor=target.import_cursor(SOURCE).unwrap();let identity=state(&archive).unwrap().namespace;
        drop(archive);let mut archive=super::super::open(&dir).unwrap();assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);assert_eq!(state(&archive).unwrap().namespace,identity);assert_eq!(target.import_cursor(SOURCE).unwrap(),cursor);
        let mut actual=ring::digest::Context::new(&ring::digest::SHA256);let mut restored=String::new();let mut after=0;let mut retained=0;
        loop {let page=target.export_kinds(super::super::semantic::KINDS,0,after,1024).unwrap();assert_eq!(page["unindexed_count"],0);for row in page["events"].as_array().unwrap() {
            let line=super::super::semantic::line(&row["observation"]).unwrap();let received=row["received_ms"].as_i64().unwrap();actual.update(format!("{received} {} {}\n",line.split_once(' ').unwrap().0,canonical(&line).unwrap()).as_bytes());restored.push_str(&line);restored.push('\n');retained+=1;
        }after=page["next"].as_i64().unwrap();if page["more"]==false {break;}}
        let digest=|value:ring::digest::Digest|value.as_ref().iter().map(|byte|format!("{byte:02x}")).collect::<String>();
        let expected=digest(expected.finish());let actual=digest(actual.finish());assert_eq!(expected,actual);assert_eq!(retained,count);assert_eq!(target.storage().unwrap()["pruned_count"],0);
        let indexed=super::super::semantic::read(&coreq::telemetry::Reader::open(&common).unwrap(),super::super::semantic::KINDS,"").unwrap();assert_eq!(crate::profile::tests::legacy_reports(&indexed,at).unwrap(),reports);assert_eq!(crate::profile::tests::legacy_reports(&restored,at).unwrap(),reports);
        let mut preserved=ring::digest::Context::new(&ring::digest::SHA256);let mut found=0;let mut after=0;
        loop {let page=target.export(0,after,1024).unwrap();for row in page["events"].as_array().unwrap() {if original_ids.contains(row["id"].as_str().unwrap()) {preserved.update((row.to_string()+"\n").as_bytes());found+=1;}}after=page["next"].as_i64().unwrap();if page["more"]==false {break;}}
        assert_eq!(found,original_ids.len());assert_eq!(preserved.finish().as_ref(),original_hash.as_ref());
        let unchanged={let mut read=archive.prepare("SELECT line FROM records ORDER BY seq").unwrap();read.query_map([],|r|r.get::<_,String>(0)).unwrap().map(|line|line.unwrap()+"\n").collect::<String>()};assert_eq!(unchanged,text);
        assert_eq!(archive.query_row("SELECT text FROM legacy WHERE id=1",[],|r|r.get::<_,String>(0)).optional().unwrap(),legacy);
        println!("source identity import: {count} rows; canonical+receipt SHA256 {actual}; 9 indexed reports equal; replay 0; pruning 0; {found} existing generic observations preserved");
        drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore="subprocess helper exercised by identity_installation_survives_kill_before_and_after_commit"]
    fn identity_crash_writer() {
        let root=std::path::PathBuf::from(std::env::var_os("TASKQ_SOURCE_IDENTITY_ROOT").expect("owned fixture"));let mut archive=super::super::open(&root).unwrap();install_identity(&mut archive).unwrap();
        std::fs::write(std::env::var_os("TASKQ_SOURCE_IDENTITY_MARKER").unwrap(),state(&archive).unwrap().namespace).unwrap();loop {std::thread::park();}
    }
    #[test]
    fn identity_installation_survives_kill_before_and_after_commit() {
        for phase in ["open","committed"] {
            let dir=dir();let archive=super::super::open(&dir).unwrap();add(&archive);add(&archive);drop(archive);let marker=dir.join("identity-marker");
            let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","telemetry::source::tests::identity_crash_writer","--ignored","--nocapture"])
                .env("TASKQ_SOURCE_IDENTITY_ROOT",&dir).env("TASKQ_SOURCE_IDENTITY_MARKER",&marker).env("TASKQ_SOURCE_IDENTITY_PHASE",phase)
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::inherit()).spawn().unwrap();
            let until=std::time::Instant::now()+std::time::Duration::from_secs(15);let mut acknowledged=None;
            while std::time::Instant::now()<until {
                if let Ok(text)=std::fs::read_to_string(&marker) {if (phase=="open"&&text=="transaction-open")||(phase=="committed"&&namespace(&text)) {acknowledged=Some(text);break;}}
                if child.try_wait().unwrap().is_some() {break;}std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let _=child.kill();child.wait().unwrap();let acknowledged=acknowledged.expect("child never reached identity boundary");
            let mut archive=super::super::open(&dir).unwrap();
            if phase=="open" {
                assert_eq!(acknowledged,"transaction-open");assert_eq!(archive.query_row("SELECT count(*) FROM sqlite_master WHERE name='telemetry_source'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
                assert_eq!(archive.query_row("SELECT count(*) FROM pragma_table_info('records') WHERE name='telemetry_epoch'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
            } else {assert_eq!(state(&archive).unwrap().namespace,acknowledged);}
            assert_eq!(archive.query_row("SELECT count(*) FROM records",[],|r|r.get::<_,i64>(0)).unwrap(),2);
            let target=Store::open(&dir.join("common")).unwrap();assert_eq!(import(&mut archive,&target,&dir).unwrap(),2);assert_eq!(import(&mut archive,&target,&dir).unwrap(),0);
            drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn pinned_source_reader_refuses_identity_installation_without_partial_metadata() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);let target=Store::open(&dir.join("common")).unwrap();
        let reader=Connection::open(dir.join("telemetry.log/telemetry.sqlite3")).unwrap();reader.execute_batch("BEGIN; SELECT * FROM records").unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("checkpoint busy"));
        assert_eq!(archive.query_row("SELECT count(*) FROM pragma_table_info('records') WHERE name='telemetry_epoch'",[],|r|r.get::<_,i64>(0)).unwrap(),0);assert_eq!(target.import_cursor(SOURCE).unwrap(),None);
        reader.execute_batch("COMMIT").unwrap();drop(reader);assert_eq!(import(&mut archive,&target,&dir).unwrap(),1);
        drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn incomplete_source_identity_is_refused_without_advancing_target() {
        let dir=dir();let mut archive=super::super::open(&dir).unwrap();add(&archive);let target=Store::open(&dir.join("common")).unwrap();
        archive.execute_batch("CREATE TRIGGER taskq_source_insert AFTER INSERT ON records BEGIN SELECT 1; END").unwrap();
        assert!(import(&mut archive,&target,&dir).unwrap_err().contains("incomplete identity"));
        assert_eq!(archive.query_row("SELECT count(*) FROM pragma_table_info('records') WHERE name='telemetry_epoch'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(archive.query_row("SELECT count(*) FROM records",[],|r|r.get::<_,i64>(0)).unwrap(),1);assert_eq!(target.import_cursor(SOURCE).unwrap(),None);
        drop(archive);drop(target);std::fs::remove_dir_all(dir).unwrap();
    }
