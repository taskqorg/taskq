use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock=std::fs::read_to_string("Cargo.lock").expect("locked dependencies");
    let package=lock.split("[[package]]").find(|p|p.lines().any(|l|l=="name = \"coreq\"")).expect("CoreQ in lock");
    let version=package.lines().find_map(|l|l.strip_prefix("version = \"" )).and_then(|v|v.strip_suffix('"')).expect("CoreQ version");
    println!("cargo:rustc-env=TASKQ_COREQ_VERSION={version}");
    for path in ["src", "scripts/build-web.cjs", "package.json", "package-lock.json"] {println!("cargo:rerun-if-changed={path}");}
    let web=std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("web");
    assert!(Command::new("node").arg("scripts/build-web.cjs").arg(&web).status().expect("Node.js build dependency").success(),"web bundle failed");
    let root=std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    coreq_instrument::generate(&root.join("src"),"main.rs",&std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("instrumented")).expect("automatic telemetry source instrumentation");
    let git = |args: &[&str]| Command::new("git").args(args).output().ok()
        .filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    for name in [Some("HEAD".into()), git(&["symbolic-ref", "-q", "HEAD"])].into_iter().flatten() {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let sha = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=TASKQ_REVISION={sha}");
}
