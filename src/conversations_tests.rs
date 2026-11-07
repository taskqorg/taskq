use super::*;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p =
            std::env::temp_dir().join(format!("taskq-conversations-{}", crate::runtime::uuid()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn revision(&self, id: &str) -> u64 {
        snapshot(&self.0)["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .and_then(|c| c["revision"].as_u64())
            .unwrap_or(0)
    }
    fn act(&self, id: &str, action: &str, mut data: Value) -> Value {
        data["id"] = json!("receipt");
        data["conversation"] = json!(id);
        data["action"] = json!(action);
        data["revision"] = json!(self.revision(id));
        act(&self.0, &data, &[self.0.display().to_string()])
    }
    fn save(&self, id: &str, text: &str) -> Value {
        self.act(
            id,
            "save",
            json!({"draft":{"text":text,"root":self.0,"title":"","caret":[1,2],"scroll":12}}),
        )
    }
    fn attach(&self, id: &str, name: &str, mime: &str, data: &[u8]) -> Value {
        self.act(id,"attach",json!({"file":{"id":"invented-file","name":name,"mime":mime,"hash":"0".repeat(64),"bytes":data.len()},"data":STANDARD.encode(data)}))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn catalog_polling_ages_the_completed_read_and_preserves_failures() {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let finished = Instant::now();
    let (tx, rx) = mpsc::sync_channel(1);
    let mut c = Catalog {
        root: f.0.clone(),
        value: json!({"items":[],"complete":true}),
        at: Some(finished - Duration::from_secs(10)),
        epoch: 7,
        pending: Some((finished - Duration::from_secs(8), 7, rx)),
    };
    tx.send((
        finished,
        json!({"items":[],"complete":true,"observed_at":1000}),
    ))
    .unwrap();
    let fresh = poll_catalog(&mut c, &f.0);
    assert_eq!(
        fresh["complete"], true,
        "a completed read must not inherit eight seconds of launch/poll delay: {fresh}"
    );
    assert_eq!(
        fresh["refreshing"], false,
        "ordinary reads wait two seconds after actual completion"
    );
    assert_eq!(fresh["observed_at"], 1000);
    let (tx, rx) = mpsc::sync_channel(1);
    {
        c.at = Some(finished - Duration::from_secs(5));
        c.pending = Some((finished, 7, rx));
    }
    let stale = poll_catalog(&mut c, &f.0);
    assert_eq!(stale["complete"], false);
    assert_eq!(stale["absence_complete"], false);
    assert_eq!(stale["refreshing"], true);
    assert!(stale["observation_age_seconds"].as_u64().unwrap() >= 5);
    tx.send((
        finished,
        json!({"items":[],"complete":false,"reason":"Invented permanent read failure"}),
    ))
    .unwrap();
    let failure = poll_catalog(&mut c, &f.0);
    assert_eq!(failure["reason"], "Invented permanent read failure");
    assert_eq!(failure["absence_complete"], false);
    let (hold, rx) = mpsc::sync_channel(1);
    {
        c.at = Some(finished - Duration::from_secs(5));
        c.pending = Some((finished, 7, rx));
    }
    let old_failure = poll_catalog(&mut c, &f.0);
    assert_eq!(
        old_failure["reason"], "Invented permanent read failure",
        "age cannot replace a genuine failure with a refresh notice"
    );
    drop(hold);
    // A result completed long ago remains stale even when delivered to this poll only now.
    let (tx, rx) = mpsc::sync_channel(1);
    {
        c.pending = Some((finished, 7, rx));
    }
    tx.send((
        finished - Duration::from_secs(5),
        json!({"items":[],"complete":true}),
    ))
    .unwrap();
    assert_eq!(
        poll_catalog(&mut c, &f.0)["complete"],
        false,
        "poll receipt cannot renew a source observation"
    );
    let (_, _, read) = c.pending.take().unwrap();
    read.recv_timeout(Duration::from_secs(5)).unwrap();
    let (hold, rx) = mpsc::sync_channel(1);
    {
        c.at = None;
        c.value = catalog_unknown("Reading conversation catalog");
        c.value["loading"] = json!(true);
        c.pending = Some((finished - Duration::from_secs(5), 7, rx));
    }
    let stalled = poll_catalog(&mut c, &f.0);
    assert_eq!(stalled["loading"], false);
    assert_eq!(stalled["complete"], false);
    assert!(
        stalled["reason"]
            .as_str()
            .unwrap()
            .contains("has not completed"),
        "a stalled first read cannot stay silently loading forever"
    );
    drop(hold);
    let disconnected = poll_catalog(&mut c, &f.0);
    assert!(
        disconnected["reason"]
            .as_str()
            .unwrap()
            .contains("read stopped")
    );
    let (_, _, read) = c.pending.take().unwrap();
    read.recv_timeout(Duration::from_secs(5)).unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    {
        c.epoch = 8;
        c.at = None;
        c.value["absence_complete"] = json!(false);
        c.pending = Some((finished, 7, rx));
    }
    tx.send((finished, json!({"items":[],"complete":true})))
        .unwrap();
    assert_eq!(
        poll_catalog(&mut c, &f.0)["absence_complete"],
        false,
        "pre-mutation scans never prove absence at a new epoch"
    );
    let (_, _, read) = c.pending.take().unwrap();
    read.recv_timeout(Duration::from_secs(5)).unwrap();
    c.value = json!({"items":[],"complete":true,"catalog_epoch":7,"observed_at":1000});
    c.at = Some(Instant::now());
    c.epoch = 8;
    let updating = poll_catalog(&mut c, &f.0);
    assert_eq!(
        updating["refreshing"], true,
        "a newer mutation starts a read without waiting for the ordinary refresh interval"
    );
    assert_eq!(updating["complete"], false);
    assert_eq!(updating["absence_complete"], false);
    assert_eq!(
        updating["loading"], true,
        "a fresh superseded observation is quiet while its replacement reads"
    );
    assert_eq!(
        updating["observed_at"], 1000,
        "invalidation never renews the completed source time"
    );
    let (_, _, read) = c.pending.take().unwrap();
    read.recv_timeout(Duration::from_secs(5)).unwrap();
    let (hold, rx) = mpsc::sync_channel(1);
    c.pending = Some((Instant::now(), 8, rx));
    c.at = Some(Instant::now() - Duration::from_secs(5));
    let aged = poll_catalog(&mut c, &f.0);
    assert_eq!(aged["loading"], false);
    assert_eq!(aged["complete"], false);
    assert!(
        aged["reason"].as_str().unwrap().contains("stale"),
        "an aged superseded source cannot stay quietly loading"
    );
    drop(hold);
    let now = Instant::now();
    c.value = json!({"items":[],"complete":true,"catalog_epoch":8,"observed_at":2000});
    c.at = Some(now);
    let (tx, rx) = mpsc::sync_channel(1);
    c.pending = Some((now, 8, rx));
    tx.send((
        now - Duration::from_secs(1),
        json!({"items":[],"complete":true,"observed_at":1000}),
    ))
    .unwrap();
    assert_eq!(
        poll_catalog(&mut c, &f.0)["observed_at"],
        2000,
        "an older async completion cannot replace a newer completed receipt read"
    );
    let (tx, rx) = mpsc::sync_channel(1);
    c.pending = Some((now, 7, rx));
    drop(tx);
    assert_eq!(
        poll_catalog(&mut c, &f.0)["complete"],
        true,
        "a disconnected superseded epoch cannot invalidate the newer completed source"
    );
}

// A2/A4/C4: real durable path; stale authority cannot freeze input or replace the original launch/files.
#[test]
fn manager_choice_is_fenced_frozen_and_reconciled_with_its_files() {
    let f = Fixture::new();
    let root = f.0.display().to_string();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let mut policy = json!({"revision":7,"grants":[{"runtime":"claude","mode":"full","project":f.0.canonicalize().unwrap(),"by":"invented operator","at":1}]});
    fs::write(f.0.join(crate::policy::FILE), policy.to_string()).unwrap();
    let mut choice = json!({"model":"","effort":"","permission":"inherit","grant_mode":"full","grant_revision":7});
    let draft = |choice: &Value| json!({"draft":{"text":"Keep exact input","title":"","root":root,"runtime":"claude","caret":[1,3],"scroll":9,"choice":choice}});
    assert_eq!(f.act("owner", "save", draft(&choice))["state"], "accepted");
    assert_eq!(
        f.attach("owner", "fixture.txt", "text/plain", b"invented bytes")["state"],
        "accepted"
    );
    let before = fs::read(f.0.join("tray-requests.json")).unwrap();
    policy["revision"] = json!(8);
    fs::write(f.0.join(crate::policy::FILE), policy.to_string()).unwrap();
    let refused = f.act("owner", "send", json!({"runtime":"claude"}));
    assert_eq!(refused["state"], "unknown");
    assert!(
        refused["reason"]
            .as_str()
            .unwrap()
            .contains("grant changed")
    );
    assert_eq!(fs::read(f.0.join("tray-requests.json")).unwrap(), before);
    assert!(!crate::setup::dir(&f.0).join("owner.json").exists());
    choice["grant_revision"] = json!(8);
    assert_eq!(f.act("owner", "save", draft(&choice))["state"], "accepted");
    assert_eq!(
        preferences(&f.0).unwrap()["launch_defaults"]["choice"],
        choice
    );
    let sent = f.act("owner", "send", json!({"runtime":"claude"}));
    assert_eq!(sent["state"], "accepted", "{sent}");
    let request = crate::setup::read(&crate::setup::dir(&f.0).join("owner.json")).unwrap();
    assert_eq!(json!(request.choice), choice);
    assert!(request.brief.contains("fixture.txt"));
    let frozen = fs::read(crate::setup::dir(&f.0).join("owner.json")).unwrap();
    choice["permission"] = json!("workspace");
    assert_eq!(f.act("owner", "save", draft(&choice))["state"], "accepted");
    assert_eq!(f.act("owner", "reconcile", json!({}))["state"], "accepted");
    assert_eq!(
        fs::read(crate::setup::dir(&f.0).join("owner.json")).unwrap(),
        frozen,
        "recovery retains the original choice and operation"
    );
    let row = &snapshot(&f.0)["items"][0];
    assert_eq!(row["files"].as_array().unwrap().len(), 1);
    assert_eq!(row["sent"]["choice"]["permission"], "inherit");
}

// R9/C4: per-draft runtime survives storage; a stale Send cannot freeze another runtime or erase work.
#[test]
fn owner_runtime_choice_is_saved_and_send_is_fenced() {
    let f = Fixture::new();
    f.save("owner", "Keep literal text");
    let saved=f.act("owner","save",json!({"draft":{"text":"Keep literal text","root":f.0,"title":"","runtime":"codex","caret":[2,4],"scroll":12}}));
    assert_eq!(saved["state"], "accepted", "{saved}");
    assert_eq!(
        snapshot(&f.0)["items"][0]["runtime"],
        "codex",
        "explicit choice survives native storage"
    );
    let before = fs::read(f.0.join("tray-requests.json")).unwrap();
    assert_eq!(
        f.act("owner", "send", json!({"runtime":"claude"}))["state"],
        "unknown"
    );
    assert_eq!(
        fs::read(f.0.join("tray-requests.json")).unwrap(),
        before,
        "stale runtime sends preserve draft and immutable input"
    );
    assert!(crate::setup::all(&f.0).unwrap().0.is_empty());
    assert_eq!(f.act("owner","save",json!({"draft":{"text":"Keep literal text","root":f.0,"title":"","runtime":"unqualified","caret":[2,4],"scroll":12}}))["state"],"unknown");
    assert_eq!(
        fs::read(f.0.join("tray-requests.json")).unwrap(),
        before,
        "unsupported choice must not corrupt the store"
    );
}

// #1929 A6: definite refusal releases Send, but uncertain storage never permits a replay.
// #2110 A5: unrelated bodies never cross the bridge; selected source/revision and durable bytes survive.
#[test]
fn lazy_catalog_keeps_totals_and_reads_only_the_requested_revision() {
    let f = Fixture::new();
    for count in [600, 10_000] {
        let drafts: BTreeMap<_, _> = (0..count)
            .map(|n| {
                (
                    format!("owner-{n}"),
                    Draft {
                        revision: 1,
                        text: "Invented saved Unicode body 🐈".repeat(4),
                        root: "invented".into(),
                        ..Draft::default()
                    },
                )
            })
            .collect();
        let p = json!({"draft":"","history":[],"conversations":drafts});
        persist(&f.0, &p).unwrap();
        let before = fs::read(f.0.join("tray-requests.json")).unwrap();
        let at = std::time::Instant::now();
        let index = summary(&f.0);
        assert_eq!(index["complete"], true, "{index}");
        assert_eq!(index["items"].as_array().unwrap().len(), count);
        assert!(
            index["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["lazy"] == true && r.get("text").is_none() && r.get("files").is_none())
        );
        let read = dialogue(
            &f.0,
            &json!({"id":"read","action":"body","conversation":"owner-0","revision":1}),
        );
        assert_eq!(read["state"], "accepted");
        assert_eq!(read["value"]["text"], drafts["owner-0"].text);
        assert_eq!(
            dialogue(
                &f.0,
                &json!({"id":"stale","action":"body","conversation":"owner-0","revision":0})
            )["state"],
            "unknown"
        );
        assert_eq!(
            fs::read(f.0.join("tray-requests.json")).unwrap(),
            before,
            "reads never mutate source or draft"
        );
        eprintln!(
            "#2110 {count} catalog: {} bytes, {:?}; selected body {} bytes",
            index.to_string().len(),
            at.elapsed(),
            read.to_string().len()
        );
    }
}

#[test]
fn recorded_questions_page_exact_revision_and_registered_source() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(f.0.join("projects"), format!("{}\n", root.display())).unwrap();
    let mut history:Vec<_>=(0..100_000).map(|n|json!({"question":{"text":format!("Invented question {n}")},"answered":{"source":{"question":n+1000,"id":n+2000}}})).collect();
    history[64]["question"]["text"] = json!("Invented Unicode 🐈".repeat(20_000));
    let path = root.join(".taskq/status.json");
    let source=json!({"complete":true,"issues":"https://example.org/issues","tasks":[{"n":7,"revision":71,"history":history}]}).to_string();
    fs::write(&path, &source).unwrap();
    let mut m = json!({"conversation":"task:https://example.org/issues/7","root":root,"revision":71,"action":"history","start":0});
    let first = history_page(&f.0, &m).unwrap();
    assert_eq!(first["rows"].as_array().unwrap().len(), 64);
    assert_eq!(first["total"], 100_000);
    m["start"] = first["next"].clone();
    let large = history_page(&f.0, &m).unwrap();
    assert_eq!(
        large["rows"].as_array().unwrap().len(),
        1,
        "one whole Unicode record, never truncation"
    );
    assert_eq!(large["rows"][0], history[64]);
    m["before"] = large["next"].clone();
    let back = history_page(&f.0, &m).unwrap();
    assert_eq!(back["start"], 64);
    assert_eq!(back["rows"][0], history[64]);
    m.as_object_mut().unwrap().remove("before");
    m["question"] = json!(91000);
    let exact = history_page(&f.0, &m).unwrap();
    assert_eq!(exact["start"], 90000);
    assert_eq!(
        exact["rows"][0], history[90000],
        "F17: jump finds the immutable question beyond the first page"
    );
    m["question"] = json!(999999);
    assert!(history_page(&f.0, &m).unwrap_err().contains("unavailable"));
    m.as_object_mut().unwrap().remove("question");
    m["revision"] = json!(70);
    assert!(history_page(&f.0, &m).unwrap_err().contains("changed"));
    m["revision"] = json!(71);
    m["conversation"] = json!("task:https://other.example/issues/7");
    assert!(history_page(&f.0, &m).is_err());
    m["root"] = json!(f.0.join("unregistered"));
    assert!(
        history_page(&f.0, &m)
            .unwrap_err()
            .contains("not registered")
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        source,
        "read-only paging keeps its exact source"
    );
}

#[test]
fn current_task_body_reads_only_the_registered_exact_revision() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(f.0.join("projects"), format!("{}\n", root.display())).unwrap();
    let row = json!({"n":7,"revision":71,"state":"ask","question":{"text":"Exact invented question","choices":[]},"conversation":{"id":"invented-owner","brief":"Exact invented brief"}});
    let path = root.join(".taskq/status.json");
    let source =
        json!({"complete":true,"issues":"https://example.org/issues","tasks":[row]}).to_string();
    fs::write(&path, &source).unwrap();
    let mut m = json!({"id":"read","conversation":"task:https://example.org/issues/7","root":root,"revision":71,"source_revision":71,"action":"task-body"});
    let r = dialogue(&f.0, &m);
    assert_eq!(r["state"], "accepted");
    assert_eq!(
        r["value"]["tasks"][0]["question"]["text"],
        "Exact invented question"
    );
    assert_eq!(
        r["value"]["tasks"][0]["conversation"]["brief"],
        "Exact invented brief"
    );
    m["source_revision"] = json!(70);
    assert_eq!(dialogue(&f.0, &m)["state"], "unknown");
    m["source_revision"] = json!(71);
    m["conversation"] = json!("task:https://other.example/issues/7");
    assert_eq!(dialogue(&f.0, &m)["state"], "unknown");
    m["root"] = json!(f.0.join("unregistered"));
    assert_eq!(dialogue(&f.0, &m)["state"], "unknown");
    assert_eq!(fs::read_to_string(path).unwrap(), source);
}

// A3/A4: >8 MiB full closed history must serve metadata, board pages and the exact native worker without HTTP.
#[test]
fn large_closed_history_keeps_metadata_pages_and_worker_reads() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"o/r"}"#,
    )
    .unwrap();
    fs::write(f.0.join("projects"), format!("{}\n", root.display())).unwrap();
    fs::write(f.0.join("token"), "invented-test-token").unwrap();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let host = crate::host::Policy::read(&f.0).unwrap().me;
    let board = "https://github.com/o/r/issues";
    let session = "invented-large-history";
    let comment = |id, body: String| json!({"id":id,"body":body,"author_association":"OWNER","user":{"login":"invented"},"created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:00:00Z"});
    let mut comments = vec![comment(
        71,
        format!("taskq: spawn worker antigravity:{session} {host}"),
    )];
    for id in 72..232 {
        comments.push(comment(
            id,
            format!(
                "taskq: steer\nInvented retained note {id}: {}",
                "x".repeat(60 * 1024)
            ),
        ));
    }
    comments.push(comment(
        232,
        format!(
            "taskq: result {} attempt 71\nChanged: invented result",
            "a".repeat(40)
        ),
    ));
    comments.push(comment(
        233,
        "taskq: close\nChanged: invented result".into(),
    ));
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let seen = calls.clone();
    let b = crate::board::replying(&crate::board::BOARDS[0], move |method, url| {
        assert_eq!(
            method, "GET",
            "history reads must never send input or write the board"
        );
        seen.set(seen.get() + 1);
        if url.ends_with("/issues/7") {
            Ok(
                json!({"number":7,"title":"Invented large task","body":"Invented brief","labels":[{"name":"taskq"}],"state":"closed","state_reason":"completed","author_association":"OWNER","closed_at":"2026-10-01T00:00:00Z"}),
            )
        } else {
            assert!(url.ends_with("/issues/7/comments?per_page=100"));
            Ok(json!(comments))
        }
    });
    let source = crate::reports::selected(&root, &*b, 7, false, None).unwrap();
    let row = &source["tasks"][0];
    let revision = row["revision"].as_u64().unwrap();
    assert!(row["history"].to_string().len() > 8 * 1024 * 1024);
    assert_eq!(
        row["history"].as_array().unwrap().len(),
        163,
        "all accepted entries survive the full fold"
    );
    assert_eq!(calls.get(), 2);
    let cold = b.requests().ops;
    let status = json!({"pid":std::process::id(),"issues":board,"tasks_age_seconds":0,"complete":true,"tasks":[]});
    let status_path = root.join(".taskq/status.json");
    fs::write(&status_path, status.to_string()).unwrap();
    let log = crate::runtime::attempt_log(&root, "worker", 7, 71);
    fs::write(&log,format!("{}\n{}\n",json!({"event":"init","conversationId":session,"conversation_id":session}),json!({"event":"result","result":{"conversation_id":session,"status":"SUCCESS","response":"Invented native response"}}))).unwrap();
    let mut m = json!({"conversation":format!("task:{board}/7"),"root":root,"revision":revision,"source_revision":revision,"attempt":71,"closed":true,"before":row["history"].as_array().unwrap().len()});
    for _ in 0..2 {
        let metadata = task_body(&f.0, &m).unwrap();
        assert_eq!(metadata["tasks"][0]["history_source_count"], 163);
        assert_eq!(metadata["tasks"][0]["brief"], "Invented brief");
        let page = history_page(&f.0, &m).unwrap();
        assert_eq!(page["total"], 163);
        assert!(page["start"].as_u64().unwrap() > 0);
        assert_eq!(
            page["rows"].as_array().unwrap().last().unwrap(),
            &row["history"][m["before"].as_u64().unwrap() as usize - 1]
        );
        let mut native = m.clone();
        native.as_object_mut().unwrap().remove("before");
        let worker = worker_read(&f.0, &native).unwrap();
        assert_eq!(worker["activity"], "ended");
        assert!(
            worker["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["text"] == "Invented native response")
        );
        native["attempt"] = json!(999);
        assert!(
            worker_read(&f.0, &native)
                .unwrap_err()
                .contains("No recorded worker")
        );
        crate::reports::selected(&root, &*b, 7, false, Some(revision)).unwrap();
        assert_eq!(calls.get(), 2, "warm reads must not revalidate board HTTP");
        assert_eq!(b.requests().ops, cold);
        m["before"] = page["start"].clone();
    }
    m["conversation"] = json!("task:https://other.example/issues/7");
    assert!(history_page(&f.0, &m).is_err());
    assert_eq!(
        fs::read_to_string(status_path).unwrap(),
        status.to_string(),
        "paging preserves the service projection"
    );
    println!(
        "LARGE_CLOSED_COST projection_bytes={} history_rows=163 cold_http={} warm_http=0 warm_304=0 warm_billed=0 (adapter fixture)",
        source.to_string().len(),
        calls.get()
    );
}

#[test]
fn conversation_1929_reply_refusal_keeps_editable_input() {
    fn never(_: &Path, _: &crate::setup::Request) -> Res<agentq_runtime::agent::Launch> {
        panic!("no native dispatch")
    }
    fn unknown(_: &Path, _: &crate::setup::Request) -> Res<bool> {
        Err("invented probe failure".into())
    }
    fn free(_: &Path, _: &crate::setup::Request) -> Res<bool> {
        Ok(false)
    }
    let f = Fixture::new();
    f.save("owner", "Keep this reply");
    f.attach("owner", "note.txt", "text/plain", b"Keep this file");
    let request = crate::setup::Request {
        schema: 2,
        id: "owner".into(),
        created: 1,
        host: "h".into(),
        platform: "macos".into(),
        client: "c".into(),
        runtime: "claude".into(),
        targets: vec![],
        brief: "b".into(),
        state: "launched".into(),
        job: None,
        session: Some("invented-session".into()),
        url: None,
        reason: None,
        root: None,
        pm: false,
        choice: None,
    };
    let dir = crate::setup::dir(&f.0);
    fs::create_dir_all(dir.join("owner")).unwrap();
    fs::write(
        dir.join("owner.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let original = rows(&preferences(&f.0).unwrap()).unwrap()["owner"].clone();
    let message = json!({"id":"operation","revision":original.revision});
    let error = reply(
        &f.0,
        "owner",
        &message,
        &crate::dialogue::Native {
            app: None,
            turn: never,
            holder: unknown,
        },
    )
    .unwrap_err();
    assert!(error.contains("native holder unknown"));
    assert_eq!(
        rows(&preferences(&f.0).unwrap()).unwrap()["owner"],
        original,
        "a pre-store refusal must clear intent without changing text/files"
    );
    assert!(crate::dialogue::turn(&f.0, "owner").unwrap().is_none());
    // A failed store is not proof that no turn was saved; retain its exact operation and input.
    fs::create_dir(dir.join("owner/turn.json").with_extension("tmp")).unwrap();
    assert!(
        reply(
            &f.0,
            "owner",
            &message,
            &crate::dialogue::Native {
                app: None,
                turn: never,
                holder: free
            }
        )
        .is_err()
    );
    let saved = rows(&preferences(&f.0).unwrap()).unwrap()["owner"].clone();
    assert_eq!(saved.text, original.text);
    assert_eq!(saved.files, original.files);
    assert_eq!(saved.reply.unwrap().id, "operation");
}

// D2/C4/#2159: an accepted local App intent is not native delivery; retain exact draft/files until readback.
#[test]
fn app_reply_pending_acceptance_keeps_draft_and_original_operation() {
    fn never(_: &Path, _: &crate::setup::Request) -> Res<agentq_runtime::agent::Launch> {
        panic!("no CLI writer")
    }
    fn free(_: &Path, _: &crate::setup::Request) -> Res<bool> {
        Ok(false)
    }
    fn baseline(_: &Path, _: &crate::setup::Request) -> Res<(usize, u64)> {
        Ok((0, 0))
    }
    fn submit(_: &Path, _: &crate::setup::Request, _: &str) -> Res<()> {
        panic!("UI never submits native input")
    }
    fn observe(
        _: &Path,
        _: &crate::setup::Request,
        _: &crate::dialogue::Turn,
    ) -> Res<crate::dialogue::AppState> {
        panic!("UI acknowledgement never probes or resends")
    }
    let f = Fixture::new();
    f.save("owner", "Keep exact App reply");
    f.attach("owner", "note.txt", "text/plain", b"Keep file");
    let request = crate::setup::Request {
        schema: 2,
        id: "owner".into(),
        created: 1,
        host: "h".into(),
        platform: "macos".into(),
        client: "c".into(),
        runtime: "codex".into(),
        targets: vec![],
        brief: "b".into(),
        state: "launched".into(),
        job: None,
        session: Some("invented-session".into()),
        url: None,
        reason: None,
        root: None,
        pm: false,
        choice: None,
    };
    let dir = crate::setup::dir(&f.0);
    fs::create_dir_all(dir.join("owner")).unwrap();
    fs::write(
        dir.join("owner.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let before = rows(&preferences(&f.0).unwrap()).unwrap()["owner"].clone();
    let native = crate::dialogue::Native {
        app: Some(crate::dialogue::AppNative {
            baseline,
            submit,
            observe,
        }),
        turn: never,
        holder: free,
    };
    let m = json!({"id":"original-op","revision":before.revision});
    assert!(
        reply(&f.0, "owner", &m, &native)
            .unwrap_err()
            .contains("acceptance pending")
    );
    let retained = rows(&preferences(&f.0).unwrap()).unwrap()["owner"].clone();
    assert_eq!(retained.text, before.text);
    assert_eq!(retained.files, before.files);
    assert_eq!(retained.reply.as_ref().unwrap().id, "original-op");
    let mut turn = crate::dialogue::turn(&f.0, "owner").unwrap().unwrap();
    let input = turn.input.clone();
    assert!(!crate::dialogue::accepted(&f.0, "owner", "original-op", &input).unwrap());
    assert!(
        reply(&f.0, "owner", &m, &native)
            .unwrap_err()
            .contains("unresolved")
    );
    turn.app_accepted = true;
    turn.app_baseline = Some(0);
    turn.state = "running".into();
    crate::platform::write_atomic(
        &dir.join("owner/turn.json"),
        &serde_json::to_string(&turn).unwrap(),
    )
    .unwrap();
    assert!(crate::dialogue::accepted(&f.0, "owner", "original-op", &input).unwrap());
    assert!(!crate::dialogue::accepted(&f.0, "owner", "original-op", "changed").unwrap());
    assert_eq!(
        rows(&preferences(&f.0).unwrap()).unwrap()["owner"].text,
        before.text,
        "native readback does not erase independent edits"
    );
}

// #1929 A6: restart/check reads the immutable turn, never dispatches; new edits remain independent.
#[test]
fn conversation_1929_unknown_reply_keeps_draft_and_reconciles_without_dispatch() {
    let f = Fixture::new();
    f.save("owner", "Original reply");
    let mut p = preferences(&f.0).unwrap();
    let mut drafts = rows(&p).unwrap();
    let d = drafts.get_mut("owner").unwrap();
    d.reply = Some(Reply {
        id: "operation".into(),
        input: "Immutable native input".into(),
        revision: d.revision,
    });
    p["conversations"] = json!(drafts);
    persist(&f.0, &p).unwrap();
    let dir = crate::setup::dir(&f.0).join("owner");
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        f.act("owner", "reply-check", json!({"op_id":"operation"}))["state"],
        "unknown"
    );
    let t = crate::dialogue::Turn {
        op_id: Some("operation".into()),
        input: "Immutable native input".into(),
        state: "pending".into(),
        ..Default::default()
    };
    fs::write(dir.join("turn.json"), serde_json::to_vec(&t).unwrap()).unwrap();
    assert_eq!(
        f.act("owner", "reply-check", json!({"op_id":"operation"}))["state"],
        "unknown",
        "a turn without launched source identity cannot prove delivery"
    );
    let request = crate::setup::Request {
        schema: 2,
        id: "owner".into(),
        created: 1,
        host: "h".into(),
        platform: "macos".into(),
        client: "c".into(),
        runtime: "codex".into(),
        targets: vec![],
        brief: "b".into(),
        state: "launched".into(),
        job: None,
        session: Some("invented-session".into()),
        url: None,
        reason: None,
        root: None,
        pm: false,
        choice: None,
    };
    fs::write(
        crate::setup::dir(&f.0).join("owner.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.act("owner", "reply-check", json!({"op_id":"operation"}))["state"],
        "unknown",
        "a saved App intent without native acceptance must retain text/files"
    );
    let t = crate::dialogue::Turn {
        app_accepted: true,
        ..t
    };
    fs::write(dir.join("turn.json"), serde_json::to_vec(&t).unwrap()).unwrap();
    assert_eq!(
        f.act("owner", "reply-check", json!({"op_id":"operation"}))["value"]["unchanged"],
        true
    );
    f.save("owner", "New independent draft");
    assert_eq!(
        f.act("owner", "reply-check", json!({"op_id":"operation"}))["value"]["unchanged"],
        false
    );
    assert_eq!(
        rows(&preferences(&f.0).unwrap()).unwrap()["owner"].text,
        "New independent draft"
    );
    assert_eq!(crate::dialogue::turn(&f.0, "owner").unwrap(), Some(t));
    assert_eq!(
        f.act("owner", "reply-check", json!({"op_id":"foreign"}))["state"],
        "unknown"
    );
}

// #1831 A2/A3: forged client session/host, replaced attempts and remote or unknown executions never select a file.
#[test]
fn worker_reads_use_only_the_live_local_attempt() {
    let f = Fixture::new();
    fs::create_dir(f.0.join(".taskq")).unwrap();
    fs::write(
        f.0.join(".taskq/T7-71.log"),
        "{\"type\":\"thread.started\",\"thread_id\":\"native-from-attempt\"}\n",
    )
    .unwrap();
    let mut status = json!({"tasks":[{"n":7,"state":"doing","role":"worker","attempt":71,"session":"codex:- local","executions":[{"attempt":71,"state":"running"}]}]});
    let m = crate::link::Read {
        board: "https://example.org/r/issues".into(),
        task: 7,
        attempt: 71,
        before: None,
        after: Some(123),
    };
    let read = |s: &Value| {
        worker_page(&f.0, "local", s, &m, |session, before, after| {
            assert_eq!(session, "codex:native-from-attempt");
            assert_eq!(before, None);
            assert_eq!(after, Some(123));
            Ok(json!({"read":true}))
        })
    };
    assert_eq!(read(&status).unwrap()["read"], true);
    status["tasks"][0]["state"] = json!("ask");
    assert_eq!(
        read(&status).unwrap()["following"],
        false,
        "Ask retains read access without following"
    );
    status["tasks"][0]["state"] = json!("doing");
    assert_eq!(
        read(&status).unwrap()["read"],
        true,
        "an answer resumes the same attempt"
    );
    for (field, value) in [("attempt", json!(72)), ("role", json!("reviewer"))] {
        let mut changed = status.clone();
        changed["tasks"][0][field] = value;
        assert_eq!(read(&changed).unwrap()["ended"], true, "{field}");
    }
    status["tasks"][0]["session"] = json!("codex:- remote");
    assert!(read(&status).unwrap_err().contains("only on its host"));
    status["tasks"][0]["session"] = json!("codex:- local");
    status["tasks"][0]["executions"][0]["state"] = json!("unknown");
    assert_eq!(
        read(&status).unwrap()["activity"],
        "unknown",
        "read access never proves execution health"
    );
    status["tasks"][0]["executions"][0]["state"] = json!("ended");
    assert_eq!(
        read(&status).unwrap()["following"],
        false,
        "ended execution retains read access"
    );
    status["tasks"] = json!([]);
    assert_eq!(read(&status).unwrap()["ended"], true);
    let refused = dialogue(
        &f.0,
        &json!({"conversation":"task:https://example.org/r/issues/7","action":"transcript","root":f.0,"session":"codex:forged"}),
    );
    assert_eq!(
        refused["state"], "unknown",
        "unregistered roots never reach native transcripts"
    );
}

// #2159 A4: retained read access never selects a client session or grants execution/input authority.
#[test]
fn retained_worker_history_keeps_attempt_host_and_read_only_boundary() {
    let f = Fixture::new();
    fs::create_dir(f.0.join(".taskq")).unwrap();
    fs::write(
        f.0.join(".taskq/T7-71.log"),
        "{\"type\":\"thread.started\",\"thread_id\":\"retained-native\"}\n",
    )
    .unwrap();
    let mut status = json!({"tasks":[{"n":7,"state":"ask","role":"worker","attempt":71,"session":"codex:- local","worker":{"attempt":71,"role":"worker","session":"codex:- local"},"executions":[{"attempt":71,"state":"ended"}]}]});
    let mut m = crate::link::Read {
        board: "https://example.org/r/issues".into(),
        task: 7,
        attempt: 71,
        before: Some(123),
        after: None,
    };
    let read = |s: &Value, m: &crate::link::Read| {
        worker_page(&f.0, "local", s, m, |session, before, after| {
            assert_eq!(session, "codex:retained-native");
            assert_eq!(before, Some(123));
            assert_eq!(after, None);
            Ok(json!({"read":true}))
        })
    };
    for state in ["ask", "later", "failed", "ready", "review"] {
        status["tasks"][0]["state"] = json!(state);
        if state != "ask" {
            status["tasks"][0]["attempt"] = Value::Null;
            status["tasks"][0]["role"] = Value::Null;
            status["tasks"][0]["session"] = Value::Null;
        }
        let page = read(&status, &m).unwrap();
        assert_eq!(
            page["read"], true,
            "{state}: retained document must remain readable"
        );
        assert_eq!(
            page["following"], false,
            "history is not proof of running execution"
        );
    }
    status["tasks"][0]["worker"]["session"] = json!("codex:- remote");
    assert!(read(&status, &m).unwrap_err().contains("only on its host"));
    status["tasks"][0]["worker"]["session"] = json!("codex:- local");
    m.attempt = 72;
    assert_eq!(
        read(&status, &m).unwrap()["ended"],
        true,
        "a mismatched attempt never selects native content"
    );
    m.attempt = 71;
    status["tasks"][0]["worker"]["attempt"] = json!(72);
    assert_eq!(
        read(&status, &m).unwrap()["ended"],
        true,
        "a replaced worker cannot reinterpret the previous source"
    );
    status["tasks"][0]["workers"] = json!([{"attempt":71,"role":"worker","session":"codex:- local"},{"attempt":72,"role":"worker","session":"codex:another local"}]);
    assert_eq!(
        read(&status, &m).unwrap()["read"],
        true,
        "an explicitly accepted earlier source stays readable without becoming current"
    );
    m.attempt = 73;
    assert_eq!(
        read(&status, &m).unwrap()["ended"],
        true,
        "an unaccepted source cannot use another worker's content"
    );
}

// A2/A3/R12: actual IPC read boundary validates registration, board, service freshness and current attempt.
#[test]
fn worker_dialogue_refuses_stale_or_cross_project_reads() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/worker-panel"}"#,
    )
    .unwrap();
    fs::write(f.0.join("projects"), format!("{}\n", root.display())).unwrap();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let m = json!({"root":root,"conversation":"task:https://github.com/invented/worker-panel/issues/7","attempt":71,"session":"codex:forged","action":"transcript","id":"read"});
    let mut status = json!({"pid":std::process::id(),"issues":"https://github.com/invented/worker-panel/issues","tasks_age_seconds":0,"complete":true,"tasks":[]});
    let path = root.join(".taskq/status.json");
    let read = |s: &Value| {
        fs::write(&path, s.to_string()).unwrap();
        dialogue(&f.0, &m)
    };
    assert_eq!(
        read(&status)["value"]["ended"],
        true,
        "an ended attempt returns before any native read"
    );
    status["complete"] = json!(false);
    assert_eq!(read(&status)["state"], "unknown");
    status["complete"] = json!(true);
    status["tasks_age_seconds"] = json!(i64::MAX);
    assert_eq!(read(&status)["state"], "unknown");
    status["tasks_age_seconds"] = json!(0);
    status["tasks_age_seconds"] = json!(-1);
    fs::write(&path, status.to_string()).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2)),
        )
        .unwrap();
    assert_eq!(
        dialogue(&f.0, &m)["state"],
        "unknown",
        "a corrupt negative board age cannot become fresh as the file ages"
    );
    status["tasks_age_seconds"] = json!(0);
    status["pid"] = json!(0);
    assert_eq!(read(&status)["state"], "unknown");
    status["pid"] = json!(std::process::id());
    read(&status);
    let mut cross = m.clone();
    cross["conversation"] = json!("task:https://github.com/invented/other/issues/7");
    assert_eq!(dialogue(&f.0, &cross)["state"], "unknown");
    fs::write(&path, "corrupt").unwrap();
    assert_eq!(dialogue(&f.0, &m)["state"], "unknown");
    // #1832 A3: a worker on another host is asked over the host link; without its key or a connection the read
    // is unknown with the reason, never an empty page
    status["tasks"] = json!([{"n":7,"state":"doing","role":"worker","attempt":71,"session":"codex:- win","executions":[{"attempt":71,"state":"running"}]}]);
    let remote = read(&status);
    assert_eq!(
        (remote["state"].as_str(), remote["reason"].as_str()),
        (
            Some("unknown"),
            Some("Host win publishes no live host link")
        ),
        "{remote}"
    );
    status["observed"] = json!(1000);
    status["hosts"] = json!([{"name":"win","updated":1000,"link":format!("{} {}","ab".repeat(32),"cd".repeat(32))}]);
    let remote = read(&status);
    assert!(
        remote["state"] == "unknown"
            && remote["reason"]
                .as_str()
                .unwrap()
                .contains("DashQ connection"),
        "{remote}"
    );
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/worker-panel","link_hosts":["mac"]}"#,
    )
    .unwrap();
    assert_eq!(
        read(&status)["reason"],
        "Host win publishes no live host link",
        "the project allowlist narrows peers"
    );
}

// #1832 principle 7: the host link request and the local panel share one path; a board this host does not serve
// reads unknown, never another project's worker.
#[test]
fn host_link_requests_serve_only_the_named_board_on_this_host() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/worker-panel"}"#,
    )
    .unwrap();
    fs::write(
        f.0.join("projects"),
        format!(
            "{}
",
            root.display()
        ),
    )
    .unwrap();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let status = json!({"pid":std::process::id(),"issues":"https://github.com/invented/worker-panel/issues","tasks_age_seconds":0,"complete":true,"tasks":[]});
    fs::write(root.join(".taskq/status.json"), status.to_string()).unwrap();
    let read = |board: &str| {
        serve(
            &f.0,
            &crate::link::Read {
                board: board.into(),
                task: 7,
                attempt: 71,
                before: None,
                after: None,
            },
        )
    };
    assert_eq!(
        read("https://github.com/invented/worker-panel/issues").unwrap()["ended"],
        true
    );
    assert!(
        read("https://github.com/invented/other/issues")
            .unwrap_err()
            .contains("not served on this host")
    );
}

// #1832 principle 2: peers come from the listening project's heartbeats, so a board-A peer must not read a
// worker of board B, another project registered on the same host; the local panel still reads both.
#[test]
fn host_link_peers_read_only_the_listening_project_board() {
    let f = Fixture::new();
    let mut roots = vec![];
    for repo in ["board-a", "board-b"] {
        let root = f.0.join(repo);
        fs::create_dir_all(root.join(".taskq")).unwrap();
        fs::write(
            root.join("taskq.json"),
            format!(r#"{{"board":"github","repo":"invented/{repo}"}}"#),
        )
        .unwrap();
        let status = json!({"pid":std::process::id(),"issues":format!("https://github.com/invented/{repo}/issues"),"tasks_age_seconds":0,"complete":true,"tasks":[]});
        fs::write(root.join(".taskq/status.json"), status.to_string()).unwrap();
        roots.push(root);
    }
    fs::write(
        f.0.join("projects"),
        roots
            .iter()
            .map(|r| {
                format!(
                    "{}
",
                    r.display()
                )
            })
            .collect::<String>(),
    )
    .unwrap();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let read = |repo: &str| crate::link::Read {
        board: format!("https://github.com/invented/{repo}/issues"),
        task: 7,
        attempt: 71,
        before: None,
        after: None,
    };
    assert_eq!(
        serve_peer(&f.0, &roots[0], &read("board-a")).unwrap()["ended"],
        true
    );
    assert!(
        serve_peer(&f.0, &roots[0], &read("board-b"))
            .unwrap_err()
            .contains("not the board of this host link")
    );
    assert_eq!(
        serve(&f.0, &read("board-b")).unwrap()["ended"],
        true,
        "the local panel path is unchanged"
    );
}

// Closed task on another host. Before: "Transcript is on host win; open it there" or an empty ended view.
// After: this host's retained attempt pages with tool output stripped, and only for the named board's peers.
#[test]
fn peers_read_closed_history_of_any_board_they_serve() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/closed-history"}"#,
    )
    .unwrap();
    fs::write(f.0.join("projects"), format!("{}\n", root.display())).unwrap();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let board = "https://github.com/invented/closed-history/issues";
    let status = json!({"pid":std::process::id(),"issues":board,"tasks_age_seconds":0,"complete":true,"observed":1000,"tasks":[],
            "hosts":[{"name":"win","updated":1000,"link":format!("{} {}","ab".repeat(32),"cd".repeat(32))}]});
    fs::write(root.join(".taskq/status.json"), status.to_string()).unwrap();
    assert_eq!(
        board_peers(&f.0, board).len(),
        1,
        "a board other than the speaking one has peers"
    );
    assert!(
        board_peers(&f.0, "https://github.com/invented/other/issues").is_empty(),
        "a board this host does not serve has none"
    );
    let read = crate::link::Read {
        board: board.into(),
        task: 7,
        attempt: 71,
        before: None,
        after: None,
    };
    let unread = serve_peer_with(
        &f.0,
        &root,
        &read,
        |_| panic!("no log, no board read"),
        |_, _, _| panic!("no log, no page"),
    )
    .unwrap();
    assert_eq!(unread["ended"], true);
    fs::write(crate::runtime::attempt_log(&root, "worker", 7, 71), "").unwrap();
    let value=serve_peer_with(&f.0,&root,&read,|_|Ok("codex:native".into()),|session,_,_|{assert_eq!(session,"codex:native");
            Ok(json!({"start":0,"end":1,"size":1,"items":[{"role":"tool","text":"result","result":"invented-secret","at":0,"i":0}]}))}).unwrap();
    assert!(!value.to_string().contains("invented-secret"));
    assert_eq!(
        (value["activity"].as_str(), value["following"].as_bool()),
        (Some("ended"), Some(false))
    );
    assert!(
        serve_peer_with(
            &f.0,
            &root,
            &read,
            |_| Err("Worker transcript is available only on its host".into()),
            |_, _, _| panic!("refused")
        )
        .is_err()
    );
}

// #2225 A2: cross-host reads cannot export arbitrary result text, structured output or tool images.
#[test]
fn peer_reads_omit_tool_result_bodies() {
    let f = Fixture::new();
    let root = f.0.join("project");
    fs::create_dir_all(root.join(".taskq")).unwrap();
    fs::write(
        root.join("taskq.json"),
        r#"{"board":"github","repo":"invented/worker-panel"}"#,
    )
    .unwrap();
    fs::write(f.0.join("host.json"), "{}").unwrap();
    let local = crate::host::Policy::read(&f.0).unwrap().me;
    let status = json!({"pid":std::process::id(),"issues":"https://github.com/invented/worker-panel/issues","tasks_age_seconds":0,"complete":true,
            "tasks":[{"n":7,"state":"doing","role":"worker","attempt":71,"session":format!("codex:native {local}")}]});
    fs::write(root.join(".taskq/status.json"), status.to_string()).unwrap();
    for after in [None, Some(0)] {
        let read = crate::link::Read {
            board: status["issues"].as_str().unwrap().into(),
            task: 7,
            attempt: 71,
            before: None,
            after,
        };
        let value=serve_peer_with(&f.0,&root,&read,|_|unreachable!(),|_,_,_|Ok(json!({"start":0,"end":12,"size":12,"items":[
                {"role":"tool","text":"result","result":"invented-unrecognized-secret","future_output":{"value":"invented-structured-secret"},"at":0,"i":0},
                {"role":"tool","text":"image","image":{"src":"invented-private-image"},"at":0,"i":1},
                {"role":"assistant","text":"done","at":0,"i":2}]}))).unwrap();
        assert!(
            !value.to_string().contains("invented-"),
            "tool output must stay on its host"
        );
        assert_eq!(value["items"][2]["text"], "done");
        assert_eq!(value["items"][1]["i"], 1);
        assert_eq!(value["end"], 12);
    }
}

// #1665 D4/D5: view reads of a launched conversation whose native transcript is missing stay unknown, never empty,
// idle or done, and never take the draft lock or rebuild the snapshot.
#[test]
fn dialogue_reads_of_a_missing_transcript_are_unknown_never_empty() {
    let f = Fixture::new();
    let id = "dialogue-1665";
    let r = crate::setup::Request {
        schema: 2,
        id: id.into(),
        created: 1,
        host: "h".into(),
        platform: "p".into(),
        client: "c".into(),
        runtime: "codex".into(),
        targets: vec![],
        brief: "b".into(),
        state: "launched".into(),
        job: None,
        session: Some(crate::runtime::uuid()),
        url: None,
        reason: None,
        root: None,
        pm: false,
        choice: None,
    };
    fs::create_dir_all(crate::setup::dir(&f.0).join(id)).unwrap();
    fs::write(
        crate::setup::dir(&f.0).join(format!("{id}.json")),
        serde_json::to_string(&r).unwrap(),
    )
    .unwrap();
    let read = |action: &str| {
        dialogue(
            &f.0,
            &json!({"id":"read","conversation":id,"action":action}),
        )
    };
    let status = read("status");
    assert_eq!(status["state"], "accepted", "{status}");
    assert!(status.get("snapshot").is_none());
    assert_eq!(status["value"]["state"], "unknown");
    assert_eq!(status["value"]["watch"], "no watch");
    assert!(status["value"]["reason"].is_string());
    let transcript = read("transcript");
    assert_eq!(transcript["state"], "unknown", "{transcript}");
    assert!(transcript["value"].is_null());
    assert!(transcript["reason"].is_string());
    assert_eq!(
        read("reply")["state"],
        "unknown",
        "a read never sends a turn"
    );
    assert_eq!(
        dialogue(
            &f.0,
            &json!({"id":"read","conversation":"../x","action":"status"})
        )["state"],
        "unknown"
    );
    assert!(
        !f.0.join("tray-requests.json").exists(),
        "reads never create draft storage"
    );
}

// C-A1/4/5/6/7: real preference/byte/setup chain. Context, sent bytes and IDs must survive crossed/partial receipts.
#[test]
fn drafts_files_and_launches_keep_immutable_identity_through_failure_and_restart() {
    assert!(valid_navigation(
        &json!({"scrolls":{"recent":240,"archive":0}})
    ));
    for filter in [
        "all", "new", "needs", "reply", "decision", "activity", "finished",
    ] {
        assert!(valid_navigation(&json!({"filter":filter})));
    }
    assert!(!valid_navigation(&json!({"filter":"invented"})));
    for scrolls in [json!("invalid"), json!({"recent":-1}), json!({"other":12})] {
        assert!(!valid_navigation(&json!({"scrolls":scrolls})));
    }
    let f = Fixture::new();
    assert_eq!(f.save("first", "first draft")["state"], "accepted");
    assert_eq!(f.save("second", "independent draft")["state"], "accepted");
    let stale = act(
        &f.0,
        &json!({"id":"stale","conversation":"first","action":"save","revision":0,"draft":{"text":"overwritten"}}),
        &[],
    );
    assert_eq!(stale["state"], "unknown");
    assert!(stale["reason"].as_str().unwrap().contains("revision"));
    let source = f.0.join("source.txt");
    fs::write(&source, "invented secret file bytes").unwrap();
    assert_eq!(
        f.attach(
            "first",
            "source.txt",
            "text/plain",
            &fs::read(&source).unwrap()
        )["state"],
        "accepted"
    );
    fs::write(&source, "source changed").unwrap();
    let row = snapshot(&f.0)["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "first")
        .unwrap()
        .clone();
    let file: File = serde_json::from_value(row["files"][0].clone()).unwrap();
    assert_eq!(
        bytes(&f.0, "first", &file).unwrap(),
        b"invented secret file bytes"
    );
    let result = f.act("first", "send", json!({"runtime":crate::runtime::OWNER}));
    assert_eq!(result["state"], "accepted", "{result}");
    let requests = crate::setup::all(&f.0).unwrap().0;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].brief.contains(&file.hash));
    assert!(
        !requests[0].brief.contains("invented secret file bytes"),
        "only file identities/paths go into the launch brief"
    );
    let original = requests[0].clone();
    f.save("first", "next draft while launching");
    assert_eq!(f.act("first", "reconcile", json!({}))["state"], "accepted");
    assert_eq!(
        crate::setup::all(&f.0).unwrap().0,
        [original.clone()],
        "same-ID readback never launches again"
    );
    assert_eq!(
        f.act("first", "send", json!({"runtime":crate::runtime::OWNER}))["state"],
        "unknown"
    );
    let p = preferences(&f.0).unwrap();
    assert_eq!(p["conversations"]["second"]["text"], "independent draft");
    assert_eq!(p["conversations"]["first"]["sent"]["text"], "first draft");
    assert_eq!(p["history"], json!(["first draft"]));
    assert_eq!(
        f.act("first", "remove", json!({"file":file.id}))["state"],
        "accepted"
    );
    assert_eq!(
        bytes(&f.0, "first", &file).unwrap(),
        b"invented secret file bytes",
        "draft Remove never changes sent bytes"
    );
    fs::write(
        folder(&f.0, "first").unwrap().join(&file.hash),
        "damaged owned copy",
    )
    .unwrap();
    assert_eq!(f.act("first", "reconcile", json!({}))["state"], "unknown");
    assert_eq!(
        crate::setup::all(&f.0).unwrap().0,
        [original],
        "corrupt attachment cannot alter the bound request"
    );
    assert_eq!(fs::read_to_string(source).unwrap(), "source changed");
    let saved = fs::read(f.0.join("tray-requests.json")).unwrap();
    assert!(navigation(&f.0, &json!({"search":7})).is_err());
    assert_eq!(fs::read(f.0.join("tray-requests.json")).unwrap(), saved);
    navigation(&f.0,&json!({"list":"recent","selected":"second","selection":{"recent":"second"},"search":"independent","listOnly":false})).unwrap();
    assert_eq!(snapshot(&f.0)["navigation"]["selected"], "second");
    fs::write(f.0.join("tray-requests.json"), "broken preferences kept").unwrap();
    assert_eq!(f.save("second", "lost draft")["state"], "unknown");
    assert_eq!(snapshot(&f.0)["complete"], false);
    assert_eq!(
        fs::read_to_string(f.0.join("tray-requests.json")).unwrap(),
        "broken preferences kept"
    );
}

// C-A4/7/TW7: unsupported/oversize input never replaces the draft; archive/Undo never controls execution.
#[test]
fn archive_restore_delete_and_acknowledged_cancel_preserve_execution_and_sources() {
    let f = Fixture::new();
    f.save("draft", "kept");
    assert!(
        crate::setup::open_id(
            &f.0,
            &f.0,
            "must not disappear",
            "request",
            crate::runtime::OWNER,
            false
        )
        .is_err(),
        "legacy request.json is not a launch identity"
    );
    assert!(!crate::setup::dir(&f.0).join("request.json").exists());
    for (name, mime, data) in [
        (
            "document.pdf",
            "application/pdf",
            b"%PDF-fixture".as_slice(),
        ),
        ("binary.txt", "text/plain", b"bad\0bytes"),
        ("fake.png", "image/png", b"not an image"),
    ] {
        assert_eq!(f.attach("draft", name, mime, data)["state"], "unknown");
    }
    assert_eq!(
        f.attach(
            "draft",
            "huge.txt",
            "text/plain",
            &vec![b'a'; FILE_BYTES + 1]
        )["state"],
        "unknown"
    );
    assert_eq!(
        preferences(&f.0).unwrap()["conversations"]["draft"]["text"],
        "kept"
    );
    f.attach("draft", "file.txt", "text/plain", b"owned bytes");
    assert_eq!(f.act("draft", "archive", json!({}))["state"], "accepted");
    assert_eq!(f.act("draft", "restore", json!({}))["state"], "accepted");
    assert_eq!(
        f.act("draft", "delete", json!({}))["state"],
        "unknown",
        "Delete requires Archive"
    );
    f.act("draft", "archive", json!({}));
    assert_eq!(f.act("draft", "delete", json!({}))["state"], "accepted");
    assert_eq!(f.save("draft", "resurrected")["state"], "unknown");
    assert!(!folder(&f.0, "draft").unwrap().exists());
    f.save("queued-request", "visible session");
    f.act(
        "queued-request",
        "send",
        json!({"runtime":crate::runtime::OWNER}),
    );
    f.act("queued-request", "archive", json!({}));
    assert_eq!(
        f.act("queued-request", "delete", json!({}))["state"],
        "unknown",
        "pending request still belongs to launcher"
    );
    assert_eq!(
        f.act("queued-request", "restore", json!({}))["state"],
        "accepted"
    );
    assert_eq!(
        f.act("queued-request", "cancel", json!({}))["state"],
        "accepted"
    );
    assert_eq!(crate::setup::all(&f.0).unwrap().0[0].state, "cancelled");
    crate::setup::step(&f.0, |_, _| {
        panic!("acknowledged cancellation must prevent native launch")
    })
    .unwrap();
    f.act("queued-request", "archive", json!({}));
    assert_eq!(
        f.act("queued-request", "delete", json!({}))["state"],
        "accepted"
    );
    assert!(preferences(&f.0).unwrap()["conversations"]["queued-request"].is_null());
    assert_eq!(
        f.save("queued-request", "deleted request")["state"],
        "unknown"
    );
    f.save("running", "keep native work");
    f.act("running", "send", json!({"runtime":crate::runtime::OWNER}));
    crate::setup::step(&f.0, |_, _| crate::setup::Launched::Job {
        job: "invented-native-job".into(),
        session: None,
        url: None,
    })
    .unwrap();
    let before = crate::setup::all(&f.0).unwrap().0;
    assert_eq!(f.act("running", "cancel", json!({}))["state"], "unknown");
    assert_eq!(
        crate::setup::all(&f.0).unwrap().0,
        before,
        "running cancellation has no lifecycle effect"
    );
}

#[cfg(unix)]
#[test]
fn attachments_and_preferences_refuse_links_without_touching_targets() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.save("safe", "draft");
    let outside = f.0.join("source-kept");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("kept"), "source").unwrap();
    fs::create_dir_all(crate::setup::dir(&f.0).join("safe")).unwrap();
    symlink(&outside, crate::setup::dir(&f.0).join("safe/attachments")).unwrap();
    assert_eq!(
        f.attach("safe", "x.txt", "text/plain", b"private")["state"],
        "unknown"
    );
    f.act("safe", "archive", json!({}));
    assert_eq!(f.act("safe", "delete", json!({}))["state"], "unknown");
    assert_eq!(fs::read_to_string(outside.join("kept")).unwrap(), "source");
}

// TW7/C-A4: deleting sent history cannot destroy independent next drafting or copy deleted sent content back.
#[test]
fn next_draft_files_transfer_once_and_survive_message_deletion_without_sent_history() {
    let f = Fixture::new();
    f.save("sent", "original sent text");
    f.act("sent", "send", json!({"runtime":crate::runtime::OWNER}));
    crate::setup::step(&f.0, |_, _| crate::setup::Launched::Job {
        job: "native-owned".into(),
        session: None,
        url: None,
    })
    .unwrap();
    f.save("sent", "independent next request");
    f.attach("sent", "next.txt", "text/plain", b"next draft bytes");
    let result = f.act("sent", "fork", json!({"fork_id":"next"}));
    assert_eq!(result["state"], "accepted", "{result}");
    assert_eq!(
        f.act("sent", "fork", json!({"fork_id":"next"}))["state"],
        "accepted",
        "same-ID recovery makes no duplicate"
    );
    let p = preferences(&f.0).unwrap();
    assert_eq!(
        p["conversations"]["next"]["text"],
        "independent next request"
    );
    assert!(p["conversations"]["next"]["sent"].is_null());
    assert_eq!(p["conversations"]["sent"]["text"], "");
    assert_eq!(
        p["conversations"]["sent"]["sent"]["text"],
        "original sent text"
    );
    let file: File =
        serde_json::from_value(p["conversations"]["next"]["files"][0].clone()).unwrap();
    assert_eq!(bytes(&f.0, "next", &file).unwrap(), b"next draft bytes");
    f.save("sent", "another independent draft");
    f.attach("sent", "new.txt", "text/plain", b"new unsent bytes");
    f.act("sent", "archive", json!({}));
    let deleted = f.act("sent", "delete", json!({}));
    assert_eq!(deleted["state"], "accepted", "{deleted}");
    let p = preferences(&f.0).unwrap();
    assert!(p["conversations"]["sent"].is_null());
    let drafts = rows(&p).unwrap();
    let (id, recovered) = drafts
        .iter()
        .find(|(_, d)| d.text == "another independent draft")
        .unwrap();
    assert!(recovered.sent.is_none());
    assert_eq!(
        bytes(&f.0, id, &recovered.files[0]).unwrap(),
        b"new unsent bytes"
    );
    assert!(
        !serde_json::to_string(&drafts)
            .unwrap()
            .contains("original sent text")
    );
    assert_eq!(drafts.len(), 2);
    assert!(crate::setup::all(&f.0).unwrap().0.is_empty());
    assert_eq!(f.save("sent", "resurrect history")["state"], "unknown");
    f.save("other", "separate context");
    assert_eq!(
        f.act("other", "fork", json!({"fork_id":"next"}))["state"],
        "unknown"
    );
    assert_eq!(
        f.act("other", "fork", json!({"fork_id":"sent"}))["state"],
        "unknown",
        "deleted IDs cannot become draft destinations"
    );
    assert_eq!(
        preferences(&f.0).unwrap()["conversations"]["other"]["text"],
        "separate context"
    );
    f.save("repeat", "same text");
    f.act("repeat", "send", json!({"runtime":crate::runtime::OWNER}));
    crate::setup::step(&f.0, |_, _| crate::setup::Launched::Job {
        job: "native-owned".into(),
        session: None,
        url: None,
    })
    .unwrap();
    f.act("repeat","save",json!({"draft":{"text":"same text","title":"","root":f.0,"caret":[0,0],"scroll":0,"next":true}}));
    f.act("repeat", "archive", json!({}));
    assert_eq!(f.act("repeat", "delete", json!({}))["state"], "accepted");
    assert!(
        rows(&preferences(&f.0).unwrap())
            .unwrap()
            .values()
            .any(|d| d.text == "same text" && d.sent.is_none()),
        "an independently retyped identical draft is not deleted sent history"
    );
}
// Chain: R1/R12, CS2–CS6. Risk: view organization writes a request, loses drafts or lies about recency.
#[test]
fn sidebar_marks_and_activity_survive_reopen_without_execution_effects() {
    let f = Fixture::new();
    f.save("owner", "kept draft");
    let mut p = preferences(&f.0).unwrap();
    p["conversation_updated"]["owner"] = json!(100);
    persist(&f.0, &p).unwrap();
    let same = f.act(
        "owner",
        "save",
        json!({"draft":{"title":"","text":"kept draft","root":f.0,"caret":[1,2],"scroll":8}}),
    );
    assert_eq!(same["state"], "accepted");
    assert_eq!(
        preferences(&f.0).unwrap()["conversation_updated"]["owner"],
        100,
        "caret/scroll never manufacture activity"
    );
    let before = fs::read(f.0.join("tray-requests.json")).unwrap();
    let task = "task:https://example.org/r/issues/12";
    let group = json!([f.0.to_str().unwrap(), "https://example.org/r/issues"]).to_string();
    for data in [
        json!({"action":"pin","key":"owner","value":true}),
        json!({"action":"pin","key":task,"value":true}),
        json!({"action":"collapse","key":group,"value":true}),
        json!({"action":"archive","key":task,"root":f.0}),
    ] {
        let r = sidebar(&f.0, &json!({"id":"write","sidebar":data}));
        assert_eq!(r["state"], "accepted", "{r}");
    }
    let now = snapshot(&f.0);
    assert_eq!(now["sidebar"]["pins"], json!(["owner", task]));
    assert_eq!(now["sidebar"]["collapsed"], json!([group]));
    assert_eq!(now["sidebar"]["archived"][task], json!(f.0));
    assert_eq!(now["items"][0]["text"], "kept draft");
    assert_eq!(now["items"][0]["updated"], 100);
    assert!(
        crate::setup::all(&f.0).unwrap().0.is_empty(),
        "pin/collapse/archive never enqueue a request"
    );
    let saved = fs::read(f.0.join("tray-requests.json")).unwrap();
    assert_ne!(saved, before);
    for data in [
        json!({"action":"pin","key":"../escape","value":true}),
        json!({"action":"pin","key":"owner","value":"true"}),
        json!({"action":"collapse","key":"alpha","value":true}),
        json!({"action":"delete","key":task}),
    ] {
        assert_eq!(
            sidebar(&f.0, &json!({"id":"bad","sidebar":data}))["state"],
            "unknown"
        );
        assert_eq!(
            fs::read(f.0.join("tray-requests.json")).unwrap(),
            saved,
            "invalid metadata must preserve the entire store"
        );
    }
    assert_eq!(
        sidebar(
            &f.0,
            &json!({"id":"restore","sidebar":{"action":"restore","key":task}})
        )["state"],
        "accepted"
    );
    assert!(snapshot(&f.0)["sidebar"]["archived"].get(task).is_none());
    assert_eq!(
        snapshot(&f.0)["sidebar"]["pins"],
        json!(["owner", task]),
        "restore keeps pins"
    );
    f.save("owner", "changed content");
    assert!(snapshot(&f.0)["items"][0]["updated"].as_u64().unwrap() > 100);
    let mut old = preferences(&f.0).unwrap();
    old.as_object_mut().unwrap().remove("conversation_sidebar");
    old.as_object_mut().unwrap().remove("conversation_updated");
    persist(&f.0, &old).unwrap();
    assert!(
        snapshot(&f.0)["items"][0]["updated"].is_null(),
        "historical drafts have unknown activity, not poll time"
    );
}
pub(crate) fn snapshot(install: &Path) -> Value {
    snapshot_inner(install, true)
}
