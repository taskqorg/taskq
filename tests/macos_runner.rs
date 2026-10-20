//! taskq.md § 8 step 5 (#1329): the macOS cargo runner signs a local build and the taskq binary beside it, then
//! execs the build. Risk: a local build starts ad-hoc, so its TCC grant never matches a release.
#![cfg(target_os = "macos")]
use std::{fs, path::Path, process::Command};

#[test]
fn runner_signs_local_builds_like_a_release_then_runs_them() {
    let dir = std::env::temp_dir().join(format!("taskq-runner-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("deps")).unwrap();
    let (signer, exe) = (dir.join("taskq"), dir.join("deps/build"));
    fs::copy(env!("CARGO_BIN_EXE_taskq"), &signer).unwrap();
    fs::copy("/usr/bin/true", &exe).unwrap();
    let out = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/macos-runner.sh")).arg(&exe).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let ids = Command::new("security").args(["find-identity", "-p", "codesigning"]).output().unwrap();
    // ponytail: CI has no identity; there the runner must only run the build ad-hoc
    if String::from_utf8_lossy(&ids.stdout).contains("\"TaskQ Local Signing\"") {
        for p in [&signer, &exe] {
            let req = String::from_utf8(Command::new("codesign").args(["-d", "-r-"]).arg(p).output().unwrap().stdout).unwrap();
            assert!(req.contains("identifier \"org.taskq.taskq\" and certificate leaf"), "{}: {req}", p.display());
        }
    }
    let _ = fs::remove_dir_all(&dir);
}
