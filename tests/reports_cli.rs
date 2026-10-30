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
    let exported=Command::new(env!("CARGO_BIN_EXE_taskq")).args(["profile","export","--since","1h"]).current_dir(&root)
        .env("TASKQ_ROOT",&root).env("TASKQ_INSTALL",root.join("install")).bounded();
    assert!(exported.status.success());
    let events:Vec<Value>=String::from_utf8(exported.stdout).unwrap().lines().map(|l|serde_json::from_str(l).unwrap()).collect();
    for op in ["reports.list.cached","reports.open.cached"] {
        assert!(events.iter().any(|e|e["observation"]["operation"].as_str().is_some_and(|s|s.ends_with(op))&&e["observation"]["duration_us"].is_number()),"{op} must record committed load time without opt-in method profiling");
    }
    std::fs::write(&status,"corrupt").unwrap();
    let unknown=run(&["reports","--json"]);assert_eq!(unknown["complete"],false);assert!(unknown["reason"].is_string());
    let _=std::fs::remove_dir_all(root);
}
