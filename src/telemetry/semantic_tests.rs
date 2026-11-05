    use super::*;
    const TAGS:[(&str,&str);5]=[("taskq-profile","operation"),("taskq-stage","stage"),("taskq-board","board"),("taskq-transition","transition"),("taskq-requests","requests")];
    /// Fixture text: one `taskq-<kind> <json>` line per typed sample.
    impl Sample {
        pub(crate) const KINDS:&'static [&'static str]=&["operation","stage","board","transition","requests"];
        pub(crate) fn fixture(line:&str)->Res<Self> {
            let (tag,text)=line.split_once(' ').ok_or("unknown fixture kind")?;
            let (_,kind)=TAGS.iter().find(|(t,_)|*t==tag).ok_or("unknown fixture kind")?;
            Self::decode(kind,serde_json::from_str(text).map_err(|e|e.to_string())?)
        }
        /// Unmarked lines and marked lines that do not parse are not samples.
        pub(crate) fn fixtures(text:&str)->Vec<Self> {
            text.lines().filter_map(|line|Self::fixture(line).ok()).collect()
        }
        pub(crate) fn line(&self)->Res<String> {
            let (tag,_)=TAGS.iter().find(|(_,k)|*k==self.kind()).ok_or("unknown semantic kind")?;
            Ok(format!("{tag} {}",self.canonical()?))
        }
    }
    fn canonical(line:&str)->Res<Value> {Sample::fixture(line)?.canonical()}
    fn observation(line:&str)->Res<Value> {let sample=Sample::fixture(line)?;encode(sample.kind(),sample.canonical()?)}
    fn line(value:&Value)->Res<String> {decoded(value)?.line()}
    fn read(store:&coreq::telemetry::Reader,kinds:&[&str])->Res<String> {
        let mut lanes:[Vec<Sample>;2]=Default::default();visit_samples(Some(store),kinds,|lane,sample|lanes[lane].push(sample))?;
        let mut text=String::new();for sample in lanes.into_iter().flatten() {text.push_str(&sample.line()?);text.push('\n');}Ok(text)
    }
    #[test]
    fn all_semantic_kinds_roundtrip_through_the_shared_store() {
        let source=[
            r#"taskq-profile {"op":"spawn","us":18446744073709551615,"error":"none","at_ms":null,"trace":{"run":null,"process":42,"span":0,"parent":0}}"#,
            // #1654: a tidy pass keeps its counts through the store
            r#"taskq-profile {"op":"service.tidy","us":9,"error":"none","at_ms":7,"counts":{"foreign":2,"removed":1,"task_bytes":18446744073709551615}}"#,
            r#"taskq-stage {"host":"win","open":true,"project":"group/nested/repo","stage":"build","task":1230,"attempt":6185426441,"rt":"codex","us":null,"outcome":"unknown","error":"none","at_ms":null,"unknown":"open-stage"}"#,
            r#"taskq-stage {"stage":"claim","task":1230,"attempt":9,"rt":"other","us":20,"outcome":"ok","error":"none","at_ms":1000,"unknown":"no-model"}"#,
            r#"taskq-stage {"stage":"build","task":1230,"attempt":9,"rt":"codex","us":30,"outcome":"ask","error":"none","at_ms":1001,"calls":0,"tokens":null,"unknown":"no-usage"}"#,
            r#"taskq-stage {"stage":"build","task":1614,"attempt":3,"rt":"claude","us":40,"outcome":"ok","error":"none","at_ms":1004,"calls":2,"tokens":900,"model":"claude-opus-5-5","output_tokens":70}"#,
            r#"taskq-board {"op":"close","us":12,"outcome":"failed","error":"http4xx","remaining":null,"at_ms":1002}"#,
            r#"taskq-transition {"project":"other","task":1230,"entry":18446744073709551615,"from":"doing","to":"ask","prev_s":null,"rt":null,"host":null,"at_ms":null}"#,
            r#"taskq-requests {"at_ms":1003,"source":"worker service status","ops":{"GET issues/N/comments":18446744073709551615,"POST graphql":0,"GET repos/*/issues":2}}"#,
        ];
        let dir=std::env::temp_dir().join(format!("taskq-semantic-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&dir).unwrap();
        let store=coreq::telemetry::Store::open(&dir.join("db")).unwrap();
        for (index,text) in source.iter().enumerate() {
            let value=observation(text).unwrap();let event=coreq::telemetry::Event::identified("test.archive",&index.to_string(),value).unwrap();store.append(&[event]).unwrap();
        }
        let rows=store.export(0,0,1024).unwrap();assert_eq!(rows["events"].as_array().unwrap().len(),source.len());
        for (row,text) in rows["events"].as_array().unwrap().iter().zip(source) {assert_eq!(canonical(&line(&row["observation"]).unwrap()).unwrap(),canonical(text).unwrap());}
        drop(store);std::fs::remove_dir_all(dir).unwrap();
    }
    // Chain: runtime usage -> stage observation (gap G3). Risk: a missing model or output count read as zero or
    // dropped without a reason; a known one lost.
    #[test]
    fn stage_model_and_token_split_are_known_or_null_with_reason() {
        let known=observation(r#"taskq-stage {"stage":"build","task":1,"attempt":2,"rt":"claude","us":4,"outcome":"ok","error":"none","at_ms":5,"calls":2,"tokens":9,"input_tokens":1,"cache_read_tokens":6,"cache_write_tokens":2,"model":"claude-opus-5-5","output_tokens":7}"#).unwrap();
        assert_eq!((&known["model"],&known["output_tokens"],known["unknown"].get("model")),(&json!("claude-opus-5-5"),&json!(7),None));
        // #1647: the context split survives the observation; a record from before the split has it null with a reason
        assert_eq!((&known["input_tokens"],&known["cache_read_tokens"],&known["cache_write_tokens"],known["unknown"].get("cache_read_tokens")),(&json!(1),&json!(6),&json!(2),None));
        let codex=observation(r#"taskq-stage {"stage":"build","task":1,"attempt":2,"rt":"codex","us":4,"outcome":"ok","error":"none","at_ms":5,"tokens":9,"output_tokens":7}"#).unwrap();
        assert_eq!((&codex["model"],&codex["unknown"]["model"],&codex["output_tokens"]),(&Value::Null,&json!("not-reported"),&json!(7)));
        assert_eq!((&codex["input_tokens"],&codex["unknown"]["input_tokens"],&codex["unknown"]["cache_write_tokens"]),(&Value::Null,&json!("not-reported"),&json!("not-reported")));
        let service=observation(r#"taskq-stage {"stage":"claim","task":1,"attempt":2,"rt":"other","us":4,"outcome":"ok","error":"none","at_ms":5,"unknown":"no-model"}"#).unwrap();
        assert_eq!((&service["unknown"]["model"],&service["unknown"]["output_tokens"],&service["unknown"]["cache_read_tokens"]),(&json!("no-model"),&json!("no-model"),&json!("no-model")));
    }
    #[test]
    fn indexed_report_reader_selects_stages_without_exposing_generic_calls() {
        let dir=std::env::temp_dir().join(format!("taskq-indexed-read-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&dir).unwrap();let path=dir.join("db");let store=coreq::telemetry::Store::open(&path).unwrap();
        let generic=coreq::telemetry::Event::new(json!({"product":"taskq","version":"1","operation":"generic.method","kind":"method","trace":"trace","span":"span","started_ms":1,"outcome":"ok","error":"none"})).unwrap();store.append(&[generic]).unwrap();
        let stage=r#"taskq-stage {"stage":"claim","task":1,"attempt":2,"rt":"other","us":1,"outcome":"ok","error":"none","at_ms":1,"unknown":"no-model"}"#;store.append(&[coreq::telemetry::Event::new(observation(stage).unwrap()).unwrap()]).unwrap();
        let reader=coreq::telemetry::Reader::open(&path).unwrap();let text=read(&reader,&["stage"]).unwrap();assert_eq!(text.lines().count(),1);assert_eq!(canonical(text.trim()).unwrap(),canonical(stage).unwrap());assert!(read(&reader,&["board"]).unwrap().is_empty());
        drop(reader);drop(store);std::fs::remove_dir_all(dir).unwrap();
    }
    /// DashQ's telemetry line key rule (dashq `counter_field`, dashq#181): ≤ 128 bytes, each `:` segment an identifier
    /// of ≤ 64 bytes `[A-Za-z0-9._-]` outside the deny list.
    fn dashq_key(k:&str)->bool {
        const DENY:[&str;18]=["prompt","command","cmd","args","stdout","stderr","output","input","content","text","message","path","cwd","env","token","secret","password","credential"];
        k.len()<=128&&k.split(':').all(|s| {let s=s.to_ascii_lowercase();
            !s.is_empty()&&s.len()<=64&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b))&&!DENY.iter().any(|d|s==*d||s.ends_with(&format!("_{d}")))})
    }
    // Defect (#1621): `GET compare/*` was sent as `GET:compare.:`, the API root as `GET:`; DashQ refused both blocks.
    // Risk kept: any request path a board module builds (literal or `format!` template, placeholders filled with numbers,
    // names, SHAs, mixed case and 30-byte words) yields a count key DashQ refuses, or a key the store cannot read back.
    #[test]
    fn every_board_request_path_yields_a_dashq_count_key() {
        let mut paths=std::collections::BTreeSet::from([String::new()]);
        for source in [include_str!("../board/mod.rs"),include_str!("../board/github.rs"),include_str!("../board/gitlab.rs")] {
            for literal in source.split('"').skip(1).step_by(2).filter(|l|l.starts_with('/')||l.starts_with("{}/")) {
                let long="abcdefghijklmnopqrstuvwxyzabcd";
                for sample in ["7","main","0bc5c954a3c5fce342039fbb4c3a19b0e04c11ab","Q-Held",long,&format!("{long}/{long}")] {
                    // a leading `{}` is the API root, which `Api::op` strips
                    let mut rest=literal.strip_prefix("{}").unwrap_or(literal);let mut path=String::new();
                    while let Some((head,tail))=rest.split_once('{') {path.push_str(head);path.push_str(sample);rest=tail.split_once('}').map_or("",|x|x.1);}
                    path.push_str(rest);paths.insert(path);
                }
            }
        }
        let ops=paths.iter().flat_map(|p|["GET","POST","PUT","PATCH","DELETE"].map(|m|crate::requests::op(m,p))).collect::<std::collections::BTreeSet<_>>();
        for known in ["GET compare/*","GET ","PATCH git/refs/heads","GET repos/*/issues"] {assert!(ops.contains(known),"{known}");}
        let ops=ops.into_iter().map(|op|(op,json!(1))).collect::<serde_json::Map<_,_>>();
        let value=observation(&format!("taskq-requests {}",json!({"at_ms":1,"source":"cli status","ops":ops}))).unwrap();
        for key in value["counts"].as_object().unwrap().keys() {assert!(dashq_key(key),"{key}");}
        assert_eq!(value["counts"]["GET:compare.ID"],1);assert_eq!(value["counts"]["GET:ROOT"],1);
        let back:Value=serde_json::from_str(line(&value).unwrap().split_once(' ').unwrap().1).unwrap();
        for op in ["GET compare/*","GET ","GET issues/N/comments","POST graphql"] {assert_eq!(back["ops"][op],1,"{op}");}
    }
    // A3 (#1621): records stored before the key change, with `*` spelled `:`, read back unchanged.
    #[test]
    fn older_colon_count_keys_still_read_back() {
        let old=json!({"product":"taskq","version":null,"kind":"requests","operation":"requests.summary","started_ms":1,"source":"cli.status",
            "counts":{"GET:compare.:":2,"GET:":3,"GET:commits.:.status":4,"GET:issues.N":5}});
        let back:Value=serde_json::from_str(line(&old).unwrap().split_once(' ').unwrap().1).unwrap();
        assert_eq!(back["ops"],json!({"GET compare/*":2,"GET ":3,"GET commits/*/status":4,"GET issues/N":5}));
    }
    // #1985: an unbilled `<op> 304` passes validation and reads back as itself.
    #[test]
    fn unbilled_304_ops_round_trip() {
        let value=observation(r#"taskq-requests {"at_ms":1,"source":"cli status","ops":{"GET issues/N/comments 304":2,"GET issues/N/comments":3,"GET  304":1}}"#).unwrap();
        for key in value["counts"].as_object().unwrap().keys() {assert!(dashq_key(key),"{key}");}
        assert_eq!(value["counts"]["GET:issues.N.comments.304"],2);assert_eq!(value["counts"]["GET:ROOT.304"],1);
        let back:Value=serde_json::from_str(line(&value).unwrap().split_once(' ').unwrap().1).unwrap();
        assert_eq!(back["ops"],json!({"GET issues/N/comments 304":2,"GET issues/N/comments":3,"GET  304":1}));
    }
    /// The published field list CoreQ pins (#2013), released as `taskq-<v>-telemetry-fields-fixtures.json`.
    const FIELDS_FIXTURE:&str=include_str!("../../tests/fixtures/telemetry-fields.json");
    // Defect (#1981 finding 11): CoreQ kept a hand copy of TaskQ's field names, so a new TaskQ field was refused until
    // CoreQ changed. Risk kept: a field TaskQ writes missing from the published fixture, a field without a type, or a
    // fixture observation the pinned CoreQ store refuses or alters. `TASKQ_TELEMETRY_FIXTURE=1` rewrites the fixture.
    #[test]
    fn published_field_fixture_lists_every_taskq_field() {
        // one fully populated record per kind first, then records whose nulls carry an unknown reason
        let full=[
            r#"taskq-profile {"op":"spawn","us":9,"error":"none","at_ms":1,"trace":{"run":1,"process":2,"span":3,"parent":4},"counts":{"removed":1}}"#,
            r#"taskq-stage {"host":"win","open":true,"project":"o/r","stage":"build","task":1,"attempt":2,"rt":"claude","us":9,"outcome":"ok","error":"none","at_ms":1,"calls":2,"tokens":30,"input_tokens":10,"cache_read_tokens":15,"cache_write_tokens":5,"model":"claude-opus-5-5","output_tokens":3,"unknown":"app-turn"}"#,
            r#"taskq-board {"op":"close","us":12,"outcome":"ok","error":"none","remaining":4000,"at_ms":2}"#,
            r#"taskq-transition {"project":"o/r","task":1,"entry":2,"from":"doing","to":"ask","prev_s":30,"rt":"claude","host":"win","at_ms":3,"counts":{"ask.technical.rule-gate":1}}"#,
            r#"taskq-requests {"at_ms":4,"source":"cli status","ops":{"GET issues/N/comments":2}}"#,
        ];
        let sparse=[
            r#"taskq-profile {"op":"spawn","us":9,"error":"none","at_ms":null,"trace":{"run":null,"process":2,"span":0,"parent":0}}"#,
            r#"taskq-stage {"host":"win","open":true,"project":"o/r","stage":"build","task":1,"attempt":2,"rt":"codex","us":null,"outcome":"unknown","error":"none","at_ms":null,"calls":null,"tokens":null,"unknown":"open-stage"}"#,
            r#"taskq-board {"op":"close","us":12,"outcome":"failed","error":"http4xx","remaining":null,"at_ms":null}"#,
            r#"taskq-transition {"project":"o/r","task":1,"entry":2,"from":"doing","to":"ask","prev_s":null,"rt":null,"host":null,"at_ms":null}"#,
        ];
        let mut observations=Vec::new();
        for (index,text) in full.iter().enumerate() {
            let value=observation(text).unwrap();let kind=Sample::KINDS[index];assert_eq!(value["kind"],kind);
            // a new field mapping needs a value in this kind's full record, or the fixture would not list it
            for (old,new) in fields(kind).unwrap() {assert!(value.get(*new).is_some_and(|v|!v.is_null()),"{kind}: full record misses {old}, so the fixture lacks {new}");}
            observations.push(value);
        }
        for text in sparse {observations.push(observation(text).unwrap());}
        let counts=["accepted","committed","lost","failed_batches","inflight","queued_bytes","queued_events"].into_iter().map(|k|(k.into(),json!(1))).collect();
        observations.push(super::super::diagnostic::health_observation("0.0.0",5,6,true,counts));
        observations.push(super::super::worker_observation(&super::super::WorkerEvent {
            at_ms: 7, project: "o/r".into(), host: "win".into(), runtime: "claude".into(), role: "work".into(),
            event: "spawn".into(), task: 1, attempt: 2, pid: 3208, outcome: "ok".into(), reason: "claimed".into(),
        }));
        let mut types=std::collections::BTreeMap::<String,std::collections::BTreeSet<&str>>::new();
        for value in &observations {for (key,field) in value.as_object().unwrap() {
            types.entry(key.clone()).or_default().insert(match field {Value::Null=>"null",Value::Bool(_)=>"boolean",Value::Number(_)=>"number",Value::String(_)=>"string",Value::Array(_)=>"array",Value::Object(_)=>"object"});
        }}
        // CoreQ accepts an unknown reason only for a field it knows: every reason names a listed field
        for value in &observations {for key in value["unknown"].as_object().unwrap().keys() {assert!(types.contains_key(key),"unknown reason for unlisted field {key}");}}
        let got=json!({"schema":"taskq-telemetry-fields/1","source":"src/telemetry/semantic.rs published_field_fixture_lists_every_taskq_field","fields":types,"observations":observations});
        if std::env::var("TASKQ_TELEMETRY_FIXTURE").is_ok_and(|v|v=="1") {
            std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"),"/tests/fixtures/telemetry-fields.json"),serde_json::to_string_pretty(&got).unwrap()+"\n").unwrap();
        }
        assert_eq!(got,serde_json::from_str::<Value>(FIELDS_FIXTURE).unwrap(),"TaskQ fields drifted from the published fixture; rerun with TASKQ_TELEMETRY_FIXTURE=1 after an intended change");
        // the pinned CoreQ store keeps every fixture observation unchanged
        let dir=std::env::temp_dir().join(format!("taskq-fields-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&dir).unwrap();
        let store=coreq::telemetry::Store::open(&dir.join("db")).unwrap();
        for (index,value) in observations.iter().enumerate() {store.append(&[coreq::telemetry::Event::identified("taskq.fields",&index.to_string(),value.clone()).unwrap()]).unwrap();}
        let rows=store.export(0,0,1024).unwrap();let rows=rows["events"].as_array().unwrap();assert_eq!(rows.len(),observations.len());
        for (row,value) in rows.iter().zip(&observations) {assert_eq!(&row["observation"],value);}
        drop(store);std::fs::remove_dir_all(dir).unwrap();
        // each release publishes the fixture, covered by SHA256SUMS
        assert!(include_str!("../../.github/workflows/release-build.yml").contains(r#"cp tests/fixtures/telemetry-fields.json "dist/taskq-$VER-telemetry-fields-fixtures.json""#),"release misses the field fixture");
    }
    #[test]
    fn unsupported_or_unsafe_source_metadata_is_refused() {
        for text in [r#"taskq-profile {"op":"unknown","us":1,"error":"none","at_ms":1}"#,
            r#"taskq-stage {"stage":"build","task":1,"attempt":1,"rt":"codex","us":1,"outcome":"ok","error":"none","at_ms":1,"prompt":"secret"}"#,
            r#"taskq-requests {"at_ms":1,"source":"cli private content","ops":{"GET issues":1}}"#,
            r#"taskq-requests {"at_ms":1,"source":"cli status","ops":{"GET issues/123/private?token=secret":1}}"#] {assert!(observation(text).is_err(),"{text}");}
    }
