//! #1803 A5: `scripts/macos-tcc-evidence.sh --label` names the cause of a re-prompting Screen Recording grant.
//! Risk: the owner reads a stale grant (TCC row pins an older cdhash) as the bundle trap, or the reverse, and takes
//! the wrong step. Fixture lines are invented in the shapes tccd and sqlite3 print.
#![cfg(unix)]
use std::{io::Write, path::Path, process::{Command, Stdio}};

const MATCH: &str = "2026-10-29 10:00:00.000 Df tccd[123:456] [com.apple.TCC:access] Failed to match existing code requirement for subject /Users/owner/.local/share/taskq/taskq and service kTCCServiceScreenCapture";
const NO_RESOURCES: &str = "2026-10-29 10:00:00.001 E tccd[123:456] [com.apple.TCC:access] /Users/owner/.local/share/taskq/taskq: code has no resources but signature indicates they must be present";
const CDHASH_ROW: &str = "/Users/owner/.local/share/taskq/taskq|1|2|2026-01-01 00:00:00|FADE0C00000000280000000100000008000000141111111111111111111111111111111111111111";
const LEAF_ROW: &str = "/Users/owner/.local/share/taskq/taskq|1|2|2026-10-29 10:00:00|FADE0C000000004C0000000100000006000000020000000F6F72672E7461736B712E7461736B71";

fn label(codesign: &str) -> String {
    let mut c = Command::new("sh").arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/macos-tcc-evidence.sh"))
        .args(["--label", codesign]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    writeln!(c.stdin.take().unwrap(), "{MATCH}\n{NO_RESOURCES}\n{CDHASH_ROW}\n{LEAF_ROW}").unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn evidence_labels_stale_grant_and_bundle_trap() {
    let ok: Vec<String> = label("ok").lines().map(String::from).collect();
    assert_eq!(ok, [format!("{MATCH}  <- stale grant"), format!("{NO_RESOURCES}  <- bundle trap"),
        format!("{CDHASH_ROW}  <- stale grant (cdhash-pinned row)"), LEAF_ROW.to_string()]);
    let failed: Vec<String> = label("failed").lines().map(String::from).collect();
    assert_eq!(failed[0], format!("{MATCH}  <- bundle trap"), "a failed codesign -v is the bundle trap");
}
