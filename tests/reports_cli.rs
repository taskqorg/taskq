use crate::Bounded;
use std::process::Command;
use serde_json::{json,Value};

// #1940 A5/A6/R12: real CLI reads must work without a board credential and leave read/decision state untouched.
#[test]
fn reports_list_and_open_use_collected_data_without_board_credentials() {
    let root=std::env::temp_dir().join(format!("q-reports-cli-{}",std::process::id()));
    std::fs::create_dir_all(root.join(".taskq")).unwrap();
    std::fs::write(root.join("taskq.json"),r#"{"board":"github","repo":"invented/reports"}"#).unwrap();
    let at=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let sha="a".repeat(40);
    let snapshot=json!({"reports":{"complete":true,"observed":at,"reports":[{"n":12,"sha":sha,"closed":40,"read":false,"body":"Invented report body","tasks":[]}]}}).to_string();
    let status=root.join(".taskq/status.json");std::fs::write(&status,&snapshot).unwrap();
    let run=|args:&[&str]| {
        let out=Command::new(env!("CARGO_BIN_EXE_taskq")).args(args).current_dir(&root)
            .env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",root.join("install")).bounded();
        assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    assert_eq!(run(&["reports","--json"])["reports"][0]["n"],12);
    assert_eq!(run(&["reports","12","--sha",&sha,"--json"])["report"]["body"],"Invented report body");
    assert_eq!(std::fs::read_to_string(&status).unwrap(),snapshot,"opening does not acknowledge read or decide a report");
    assert!(!root.join(".taskq/poke").exists(),"navigation never schedules board work");
    std::fs::write(&status,"corrupt").unwrap();
    let unknown=run(&["reports","--json"]);assert_eq!(unknown["complete"],false);assert!(unknown["reason"].is_string());
    let _=std::fs::remove_dir_all(root);
}
