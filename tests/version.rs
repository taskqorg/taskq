use crate::Bounded;
use std::process::Command;

// Chain: § 5 `taskq --version` (#1900). Risk: the flag is parsed as one that needs a value, so clients such as PilotQ
// fall back to an owner-written receipt; or it reaches the install, board or service before it prints.
#[test]
fn version_prints_build_version_without_install_access() {
    let root = std::env::temp_dir().join(format!("q-version-{}", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_taskq")).arg("--version").env("TASKQ_ROOT", &root).env("TASKQ_INSTALL", root.join("install")).bounded();
    let created = root.exists();
    let _ = std::fs::remove_dir_all(&root);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{}\n", env!("CARGO_PKG_VERSION")));
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!created, "--version created {}", root.display());
}
