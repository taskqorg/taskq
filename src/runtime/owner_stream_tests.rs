use super::*;
// A3/A5/R12: stream identity, intent vs acceptance, completion and lost-release recovery remain distinct.
#[test]
fn native_stream_fences_identity_and_unknown_release() {
    let session_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let text = format!(
        "{}\n{}\n",
        json!({"event":"init","conversation_id":session_id}),
        json!({"event":"result","result":{"conversation_id":session_id,"status":"SUCCESS","response":"literal response"}})
    );
    assert_eq!(
        session("antigravity", &text).unwrap().as_deref(),
        Some(session_id)
    );
    assert_eq!(outcome("antigravity", &text), "success");
    assert!(
        session(
            "antigravity",
            &(text + &json!({"conversationId":"foreign-conversation"}).to_string())
        )
        .is_err()
    );
    let root = std::env::temp_dir().join(format!("taskq-owner-stream-{}", super::super::uuid()));
    fs::create_dir_all(&root).unwrap();
    let r:Request=serde_json::from_value(json!({"schema":2,"id":"invented","created":1,"host":"invented","platform":"invented","client":"invented","runtime":"antigravity","targets":[],"brief":"invented","state":"launched","session":session_id})).unwrap();
    let current = coreq::process::identity(std::process::id())
        .unwrap()
        .unwrap();
    let identity = json!({"pid":current.pid,"started":current.started});
    fs::write(
        root.join("native-process.json"),
        json!({"identity":identity}).to_string(),
    )
    .unwrap();
    assert_eq!(holder(&root, &r), Ok(true));
    fs::write(
        root.join("turn.json"),
        json!({"state":"done","outcome":"unknown: interrupted submission"}).to_string(),
    )
    .unwrap();
    assert!(holder(&root, &r).unwrap_err().contains("unverified"));
    fs::write(
        log(&root),
        format!("{}\n", json!({"event":"init","conversation_id":session_id})),
    )
    .unwrap();
    let turn = crate::dialogue::Turn {
        stream_offset: Some(0),
        stream_written: true,
        ..Default::default()
    };
    assert_eq!(accepted(&root, &r, &turn), Ok(false));
    writeln!(fs::OpenOptions::new().append(true).open(log(&root)).unwrap(),"{}",json!({"event":"step_update","step_update":{"conversation_id":session_id,"step_type":"user_input","state":"DONE"}})).unwrap();
    assert_eq!(accepted(&root, &r, &turn), Ok(true));
    fs::remove_dir_all(root).unwrap();
}
// Native #2165 reproduction: agy ignored a missing --conversation and returned a new ID. Refuse before spawn/input.
#[test]
fn missing_native_state_and_wrong_startup_never_receive_input() {
    let root = std::env::temp_dir().join(format!("taskq-owner-refusal-{}", super::super::uuid()));
    fs::create_dir_all(&root).unwrap();
    assert!(
        readable_source(&root.join("missing.db"))
            .unwrap_err()
            .contains("resume refused before input")
    );
    fs::write(root.join("invalid.db"), b"invented invalid native state").unwrap();
    assert!(readable_source(&root.join("invalid.db")).is_err());
    #[cfg(unix)]
    {
        use std::process::{Command, Stdio};
        input(&root, &"invented long retained input".repeat(700000)).unwrap();
        let offset = fs::metadata(log(&root)).unwrap().len();
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(log(&root))
                .unwrap(),
            "{}",
            json!({"event":"init","conversation_id":"foreign-native-session"})
        )
        .unwrap();
        let marker = root.join("input-received");
        let mut child = Command::new("sh")
            .args(["-c", "if read input; then touch \"$1\"; fi", "fixture"])
            .arg(&marker)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        assert!(
            ready(
                &root,
                "antigravity",
                "expected-native-session",
                offset,
                &mut child
            )
            .unwrap_err()
            .contains("different session")
        );
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
        assert!(!marker.exists());
    }
    fs::remove_dir_all(root).unwrap();
}
