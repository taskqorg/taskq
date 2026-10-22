//! Windows §8: one inherited native token; control state never lives in the writable runtime home.
use std::{fs, io::Read, path::{Path, PathBuf}, process::Command};
use serde_json::Value;
mod security;
mod preparation;

// Historical binary identity does not qualify the failed containment boundary.
const NATIVE_SHA256: Option<&str> = None;
const CODE_HOST_SHA256: &str = "9f865a748c984492ca9b69aac75680e4954f31311c43ec33ddbdae0ed77bd4bd";
const BOOT: &str = "__windows-codex";

fn native_digest() -> Result<&'static str, String> {
    NATIVE_SHA256.ok_or_else(|| "Windows sandbox: Workspace containment qualification incomplete".into())
}

fn hash(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = [0u8; 65536];
    loop { let n = file.read(&mut buf).map_err(|e| e.to_string())?; if n == 0 { break; } digest.update(&buf[..n]); }
    Ok(digest.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect())
}

fn checked(path: &Path) -> Result<PathBuf, String> {
    use std::os::windows::fs::MetadataExt;
    for p in path.ancestors() {
        if fs::symlink_metadata(p).map_err(|e| e.to_string())?.file_attributes() & 0x400 != 0 {
            return Err(format!("Windows sandbox refuses reparse point {}", p.display()));
        }
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let text = path.to_str().ok_or("Windows sandbox requires Unicode paths")?;
    if text.starts_with("\\\\?\\UNC\\") { return Err("Windows sandbox refuses network paths".into()); }
    Ok(PathBuf::from(text.strip_prefix("\\\\?\\").unwrap_or(text)))
}

fn home() -> Result<PathBuf, String> {
    checked(&std::env::var_os("CODEX_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")))
        .ok_or("Windows sandbox: runtime home missing")?)
}

struct Paths { bin: PathBuf, state: PathBuf, temp: PathBuf }
fn paths(root: &Path, install: &Path, home: &Path) -> Result<Paths, String> {
    let key = format!("{}\n{}", root.display(), home.display());
    let digest = ring::digest::digest(&ring::digest::SHA256, key.as_bytes());
    let id: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let local = checked(&PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("Windows sandbox: LOCALAPPDATA missing")?))?;
    Ok(Paths { bin: install.join("windows-native/codex.exe"), state: install.join("windows-native/state").join(&id),
        temp: local.join("Temp/taskq-windows").join(id) })
}

pub fn ready(root: &Path, install: &Path) -> Result<(), String> {
    let native = native_digest()?;
    let root = checked(root)?; let install = checked(install)?; let home = home()?;
    if install.starts_with(&root) || root.starts_with(&install) { return Err("Windows sandbox: installation and workspace overlap".into()); }
    for p in [root.join(".codex"), home.join("config.toml"), home.join("auth.json")] { checked(&p)?; }
    let bin = paths(&root, &install, &home)?.bin;
    for (file, expected) in [(&bin, native), (&bin.with_file_name("codex-code-mode-host.exe"), CODE_HOST_SHA256)] {
        checked(file)?;
        if hash(file)? != expected { return Err("Windows sandbox: native dependency not qualified".into()); }
    }
    preparation::ready(&install)?;
    Ok(())
}

fn restricted() -> Result<bool, String> {
    use std::ffi::c_void;
    #[link(name="advapi32")]
    unsafe extern "system" { fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32; fn IsTokenRestricted(token: *mut c_void) -> i32; }
    #[link(name="kernel32")]
    unsafe extern "system" { fn GetCurrentProcess() -> *mut c_void; fn CloseHandle(handle: *mut c_void) -> i32; }
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) } == 0 { return Err("Windows sandbox: cannot inspect caller token".into()); }
    let result = unsafe { IsTokenRestricted(token) != 0 };
    unsafe { CloseHandle(token); }
    Ok(result)
}

fn control(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    name.ends_with(".toml") || ["auth.json", "hooks.json", "agents.md", "cap_sid", ".sandbox", ".sandbox-bin", ".sandbox-secrets", "plugins", "skills", "packages", "app-server-control", "app-server-daemon"].contains(&name.as_str())
}

fn prepare(root: &Path, install: &Path, home: &Path, p: &mut Paths, bootstrap: &Path) -> Result<(), String> {
    if restricted()? { return Err("Windows sandbox preparation requires an unrestricted operator process".into()); }
    security::create_directory(&p.state)?; security::create_directory(&p.temp)?;
    p.state = checked(&p.state)?; p.temp = checked(&p.temp)?;
    let mut entries = vec![(":root".to_string(), "read"), (root.display().to_string(), "write"),
        (home.display().to_string(), "write"), (p.temp.display().to_string(), "write"),
        (install.display().to_string(), "read"), (root.join(".codex").display().to_string(), "read")];
    for entry in fs::read_dir(home).map_err(|e| e.to_string())? {
        let f = entry.map_err(|e| e.to_string())?.path();
        if control(&f) {
            checked(&f)?;
            entries.push((f.display().to_string(), "read"));
        }
    }
    let config = format!("[permissions.taskq.filesystem]\n{}\n[permissions.taskq.network]\nenabled=true\n", entries.iter()
        .map(|(p, mode)| format!("{} = \"{mode}\"", serde_json::to_string(&p.replace('\\', "/")).unwrap())).collect::<Vec<_>>().join("\n"));
    let helper = p.bin.with_file_name("codex-code-mode-host.exe");
    let protection = preparation::Preparation::begin(root, install, home, &p.state, &p.temp, &[bootstrap, &p.bin, &helper])?;
    let preflight = (|| {
    fs::write(p.state.join("config.toml"), config).map_err(|e| e.to_string())?;
    let out = match coreq::process::output("sandbox.preflight", Command::new(&p.bin).args(["-c", "windows.sandbox=unelevated", "sandbox", "-P", "taskq", "--include-managed-config", "--", "cmd.exe", "/d", "/c", "exit 0"])
        .current_dir(root).env("CODEX_HOME", &p.state).env("TASKQ_WINDOWS_SANDBOX_HOME", &p.state)
        .env("TEMP", &p.temp).env("TMP", &p.temp), std::time::Duration::from_secs(30)) {
        Ok(out) | Err(coreq::process::Failure::Exit(out)) => out,
        Err(e) => return Err(format!("Windows native preflight: {e}")),
    };
    if !out.status.success() { return Err(format!("Windows native preflight: {}", String::from_utf8_lossy(&out.stderr))); }
    let sid_file = p.state.join("cap_sid");
    let mut caps: Value = serde_json::from_slice(&fs::read(&sid_file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    // A command in a subdirectory still uses the same approved workspace root SID.
    let aliases = caps["workspace_by_cwd"].as_object().ok_or("Windows native workspace capabilities missing")?.clone();
    let roots = caps["writable_root_by_path"].as_object_mut().ok_or("Windows native root capabilities missing")?;
    for (key, sid) in aliases { roots.entry(key).or_insert(sid); }
    fs::write(sid_file, serde_json::to_vec(&caps).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(())
    })();
    match preflight {
        Ok(()) => protection.apply(),
        Err(error) => { protection.cancel()?; Err(error) },
    }
}

pub fn command(env: &crate::runtime::Env) -> Result<Command, String> {
    ready(env.root, env.install)?;
    let root = checked(env.root)?; let install = checked(env.install)?; let home = home()?;
    let mut p = paths(&root, &install, &home)?;
    prepare(&root, &install, &home, &mut p, env.exe)?;
    let mut c = Command::new(p.bin);
    c.args(["-c", "windows.sandbox=unelevated", "sandbox", "-P", "taskq", "--include-managed-config", "--"])
        .arg(env.exe).arg(BOOT).arg(root).arg(install).arg(home)
        .env("CODEX_HOME", &p.state).env("TASKQ_WINDOWS_SANDBOX_HOME", &p.state)
        .env("TEMP", &p.temp).env("TMP", &p.temp);
    Ok(c)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let native = native_digest()?;
    if args.len() < 4 || args[3] != "exec" || args[4..].iter().any(|a| a.contains("danger-full-access") || ["--yolo", "--dangerously-bypass-approvals-and-sandbox"].contains(&a.as_str())) {
        return Err("Windows sandbox: bounded Codex exec expected".into());
    }
    if !restricted()? { return Err("Windows sandbox: inherited native token required".into()); }
    let root = checked(Path::new(&args[0]))?; let install = checked(Path::new(&args[1]))?; let home = checked(Path::new(&args[2]))?;
    let policy = crate::policy::load(&install)?;
    let scope = root.canonicalize().map_err(|e| e.to_string())?.display().to_string();
    if !policy.grants.iter().any(|g| crate::runtime::of(&g.runtime).codex && crate::runtime::of(&g.runtime).child && g.mode == "workspace" && g.project.as_deref() == Some(scope.as_str())) {
        return Err("Windows sandbox: explicit Workspace project grant required".into());
    }
    let p = paths(&root, &install, &home)?;
    if std::env::var_os("CODEX_HOME").map(PathBuf::from).as_ref() != Some(&p.state) || hash(&p.bin)? != native
        || hash(&p.bin.with_file_name("codex-code-mode-host.exe"))? != CODE_HOST_SHA256 {
        return Err("Windows sandbox: control state or native dependency changed".into());
    }
    // The bootstrap is callable by a child too; project input never selects profiles or wider config.
    let mut it = args[4..].iter();
    while let Some(arg) = it.next() {
        let valid = match arg.as_str() {
            "--json" | "-" => true,
            "-s" => it.next().is_some_and(|v| v == "workspace-write"),
            "-m" | "--model" => it.next().is_some_and(|v| !v.starts_with('-')),
            "-C" => it.next().is_some_and(|v| checked(Path::new(v)).is_ok_and(|p| p == root)),
            "resume" => it.next().is_some_and(|v| v.len() == 36 && v.chars().all(|c| c.is_ascii_hexdigit() || c == '-')),
            "-c" | "--config" => it.next().is_some_and(|v| v.starts_with("model_reasoning_effort=") || ["approval_policy=never", "sandbox_workspace_write.writable_roots=[]", "sandbox_workspace_write.network_access=true", "windows.sandbox=unelevated"].contains(&v.as_str())),
            _ => false,
        };
        if !valid { return Err("Windows sandbox: unsupported bootstrap argument".into()); }
    }
    security::lower_restricted_process()?;
    let status = Command::new(p.bin).args(["exec", "--add-dir"]).arg(home).args(&args[4..])
        .env("CODEX_HOME", &args[2]).env("TASKQ_WINDOWS_SANDBOX_HOME", p.state)
        .current_dir(root).status().map_err(|e| e.to_string())?;
    std::process::exit(status.code().unwrap_or(1));
}

pub fn restore(args: &[String]) -> Result<(), String> {
    if args.len() != 1 { return Err("Windows security restore requires one installation path".into()); }
    preparation::restore(&checked(Path::new(&args[0]))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    // R9: unknown containment must refuse policy admission, command preparation and direct bootstrap alike.
    #[test]
    fn unqualified_workspace_refuses_all_launch_entries() {
        let base = std::env::temp_dir().join(format!("taskq-unqualified-{}", crate::runtime::uuid()));
        assert!(!base.exists());
        let root = base.join("project"); let install = base.join("install"); let exe = base.join("taskq.exe");
        let env = crate::runtime::Env { root: &root, install: &install, exe: &exe, codex: &[], permission_mode: "dontAsk", publish: "pr", board_env: &[] };
        let args = [root.display().to_string(), install.display().to_string(), base.join("home").display().to_string(), "exec".into(), "--json".into(), "-".into()];
        for result in [ready(&root, &install), command(&env).map(|_| ()), run(&args)] {
            assert_eq!(result.unwrap_err(), "Windows sandbox: Workspace containment qualification incomplete");
        }
        let policy = Ok(crate::policy::Policy { revision: 1, grants: vec![crate::policy::Grant {
            runtime: "codex".into(), mode: "workspace".into(), project: Some(root.display().to_string()), by: "fixture".into(), at: 1,
        }] });
        let host = crate::policy::Host { os: "windows", root: &root, install: &install, home: &base };
        let requested = ["-s".into(), "workspace-write".into(), "-c".into(), "sandbox_workspace_write.network_access=true".into()];
        let access = crate::policy::resolve(&policy, &host, "codex", "dontAsk", &requested);
        assert_eq!(access.refused.as_deref(), Some("Windows sandbox: Workspace containment qualification incomplete"));
        assert!(!access.windows && access.codex.is_none(), "a saved grant cannot enable unqualified enforcement");
        assert!(!base.exists(), "unqualified launch must not prepare files or control state");
    }

    // No paid turn: exercise the production preparation/bootstrap with controller-owned fake home.
    #[test]
    #[ignore = "requires an owned disposable fixture and exact private native dependencies"]
    fn native_security_without_model() {
        let base = checked(Path::new(&std::env::var("TASKQ_SECURITY_FIXTURE").expect("owned fixture"))).unwrap();
        assert_eq!(fs::read_to_string(base.join(".fixture-owner")).unwrap(), "taskq-owned-source-security-probe\n");
        let root = base.join("project"); let install = base.join("install"); let home = base.join("home");
        if std::env::var_os("TASKQ_SECURITY_CHILD").is_some() {
            assert!(restricted().unwrap());
            assert_eq!(security::lower_restricted_process().unwrap(), 4096);
            let python = std::env::var("TASKQ_QUALIFY_PYTHON").expect("native Python");
            let status = Command::new(python).arg(root.join("check.py")).current_dir(&root)
                .env("CODEX_HOME", &home).env("TASKQ_INSTALL", &install).env("TASKQ_ROLE", "worker").status().unwrap();
            assert!(status.success());
            return;
        }
        assert!(!restricted().unwrap());
        assert_eq!(std::env::var_os("CODEX_HOME").map(PathBuf::from), Some(home.clone()));
        let mut p = paths(&root, &install, &home).unwrap();
        let native_hash = std::env::var("TASKQ_SECURITY_CODEX_SHA256").expect("development artifact identity");
        assert_eq!(hash(&p.bin).unwrap(), native_hash);
        assert_eq!(hash(&p.bin.with_file_name("codex-code-mode-host.exe")).unwrap(), CODE_HOST_SHA256);
        let before = security::Snapshot::capture(&root).unwrap();
        let other = base.join("outside");
        let other_before = security::Snapshot::capture(&other).unwrap();
        let policy = crate::policy::load(&install).unwrap();
        for project in [&root, &other] {
            let scope = project.canonicalize().unwrap().display().to_string();
            assert!(policy.grants.iter().any(|g|g.runtime == "codex" && g.mode == "workspace" && g.project.as_deref() == Some(scope.as_str())), "both owned roots need explicit fixture grants");
        }
        let mut other_paths = paths(&other, &install, &home).unwrap();
        prepare(&other, &install, &home, &mut other_paths, &install.join("taskq.exe")).unwrap();
        prepare(&root, &install, &home, &mut p, &install.join("taskq.exe")).unwrap();
        let caps: Value = serde_json::from_slice(&fs::read(p.state.join("cap_sid")).unwrap()).unwrap();
        let other_caps: Value = serde_json::from_slice(&fs::read(other_paths.state.join("cap_sid")).unwrap()).unwrap();
        let ids = |value: &Value| value["workspace_by_cwd"].as_object().unwrap().values().map(|v|v.as_str().unwrap().to_owned()).collect::<Vec<_>>();
        assert!(ids(&caps).iter().all(|id|!ids(&other_caps).contains(id)), "independent SDK workspace capabilities");
        let other_file = security::Snapshot::capture(&other.join("canary.txt")).unwrap();
        assert!(other_file.descriptor.contains(";;;LW)"), "cross-root target must actually be Low");
        fs::write(base.join("cross-root-capabilities.json"), serde_json::to_vec_pretty(&serde_json::json!({"root": root, "other_root": other, "root_caps": caps, "other_caps": other_caps, "other_root_security": security::Snapshot::capture(&other).unwrap(), "other_file_security":other_file})).unwrap()).unwrap();
        let mut child = Command::new(&p.bin);
        child.args(["-c", "windows.sandbox=unelevated", "sandbox", "-P", "taskq", "--include-managed-config", "--"])
            .arg(install.join("harness.exe")).args(["windows_sandbox::tests::native_security_without_model", "--ignored", "--exact", "--nocapture"])
            .current_dir(&root).env("CODEX_HOME", &p.state).env("TASKQ_WINDOWS_SANDBOX_HOME", &p.state)
            .env("TEMP", &p.temp).env("TMP", &p.temp).env("TASKQ_SECURITY_CHILD", "1");
        let output = coreq::process::output("sandbox.security-proof", &mut child, std::time::Duration::from_secs(60)).unwrap();
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let proof: Value = serde_json::from_slice(&fs::read(root.join("proof.json")).unwrap()).unwrap();
        assert_eq!(proof["allowed"], true, "{proof}");
        assert_eq!(proof["allowed_mutations"]["write"]["readback"], true, "{proof}");
        assert_eq!(proof["allowed_mutations"]["rename"]["readback"], true, "{proof}");
        assert_eq!(proof["allowed_mutations"]["delete"]["deleted"], true, "{proof}");
        for key in ["outside", "policy", "config", "runtime_config", "junction", "git", "agents", "aws"] {
            assert_eq!(proof[key]["native_error"], 5, "{proof}");
            for action in ["delete", "rename", "replace"] {
                let mutation = &proof["mutations"][key][action];
                assert_eq!(mutation["allowed"], false, "{proof}");
                assert_eq!(mutation["native_error"], 5, "{proof}");
                assert!(mutation["before"]["ino"].as_u64().is_some_and(|id| id != 0), "{proof}");
                assert_eq!(mutation["before"], mutation["after"], "{proof}");
            }
        }
        assert_ne!(proof["child"]["exit"], 0, "{proof}");
        assert_ne!(proof["acl"]["exit"], 0, "{proof}");
        assert_ne!(proof["nested_launcher"]["exit"], 0, "{proof}");
        preparation::restore(&install).unwrap();
        assert_eq!(security::Snapshot::capture(&root).unwrap(), before);
        assert_eq!(security::Snapshot::capture(&other).unwrap(), other_before);
        println!("QUALIFIED no-model source security; development native digest {native_hash}; not a product-pin qualification");
    }
    // Paid chain: real worker/reviewer start, continuation, rules and inherited native write boundaries.
    #[test]
    #[ignore = "requires the pinned native candidate and an explicit disposable Windows project grant"]
    fn live_windows_boundary_matrix() {
        use crate::runtime::{Env, Launch};
        let root = PathBuf::from(std::env::var_os("TASKQ_QUALIFY_ROOT").expect("disposable project"));
        let install = PathBuf::from(std::env::var_os("TASKQ_INSTALL").expect("disposable installation"));
        let exe = install.join("taskq.exe");
        let python = std::env::var("TASKQ_QUALIFY_PYTHON").expect("native Python executable");
        assert!(Path::new(&python).is_file());
        let codex = vec!["-s".into(), "workspace-write".into(), "-c".into(), "sandbox_workspace_write.network_access=true".into()];
        let env = Env { root: &root, install: &install, exe: &exe, codex: &codex, permission_mode: "dontAsk", publish: "pr", board_env: &[] };
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::create_dir_all(root.join(".taskq")).unwrap();
        for name in [".git", ".agents", ".aws"] { fs::create_dir_all(root.join(name)).unwrap(); }
        fs::create_dir_all(root.parent().unwrap().join("outside")).unwrap();
        let home = home().unwrap();
        let canaries = [root.parent().unwrap().join("outside/canary.txt"), install.join("canary.txt"), root.join(".codex/canary.toml"), home.join("config-canary.toml"), root.join(".git/canary.toml"), root.join(".agents/canary.toml"), root.join(".aws/canary.toml")];
        for path in &canaries { fs::write(path, "UNCHANGED\n").unwrap(); }
        assert!(Command::new("cmd.exe").args(["/d", "/c", "mklink", "/J"]).arg(root.join("outside-junction")).arg(canaries[0].parent().unwrap()).status().unwrap().success());
        fs::write(root.join("check.py"), include_str!("../tests/fixtures/windows-boundary.py")).unwrap();
        fs::write(root.join("taskq.md"), format!("# Owner-authorized isolated runtime qualification\nThis fixture has no board task: do not run inbox, create worktrees, commit, publish, change permissions or request elevation. These owner instructions override generic board workflow. Both roles must execute exactly `& '{}' check.py` from this root, inspect its JSON stdout and compare baseline, expected and candidate active/repeated/empty cases. Identify a regression even when the command exits 0. Report actual canary and nested-launcher errors, never infer successful denial from tool startup failure. Do not edit check.py or repair the controller-injected candidate. Repeat the exact check on continuation after stage changes.\n", python.replace('\'', "''"))).unwrap();
        fs::write(root.join("AGENTS.md"), "Read taskq.md; disposable qualification only.\n").unwrap();
        let attempt = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() * 2;
        let rules = [false, true].map(|resume| crate::runtime::instructions(&root, "", resume).unwrap());
        for (i, role) in ["worker", "reviewer"].iter().enumerate() {
            let mut l = Launch { execution: None, task: 1, title: "Owner-authorized isolated Windows boundary qualification; follow fixture taskq.md, no board or publication.".into(), attempt: attempt + i as u64,
                role: role.to_string(), rt: "codex".into(), session: crate::runtime::uuid(), url: "https://github.com/taskqorg/taskq".into(), resume: None };
            for turn in 0..2 {
                fs::write(root.join("stage"), turn.to_string()).unwrap();
                fs::remove_file(root.join("proof.json")).ok();
                if turn == 1 { l.resume = Some("Owner: repeat the fixture check against the changed stage; keep this session, no other actions.".into()); }
                let path = crate::runtime::log_path(&root, &l);
                let offset = fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
                let mut child = crate::runtime::spawn(&env, &l).unwrap_or_else(|e| panic!("{e}"));
                let began = std::time::Instant::now();
                let code = loop {
                    if let Some(code) = child.exited().unwrap() { break code; }
                    if began.elapsed().as_secs() > 180 { crate::runtime::stop(child.as_mut()).expect("owned qualification cleanup"); panic!("owned qualification deadline"); }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                };
                let appended = fs::read_to_string(path).unwrap();
                let log = &appended[offset..];
                let events: Vec<Value> = log.lines().filter_map(|s| serde_json::from_str(s).ok()).collect();
                let ids: Vec<String> = events.iter().filter(|v| v["type"] == "thread.started").filter_map(|v| v["thread_id"].as_str().map(String::from)).collect();
                if turn == 0 { l.session = ids.first().expect("native session").clone(); }
                assert!(!ids.is_empty() && ids.iter().all(|id| *id == l.session), "same native session");
                assert!(events.iter().any(|v| v["type"] == "turn.completed"), "native completion");
                assert!(!events.iter().any(|v| v["type"] == "turn.failed"), "native failure: {log}");
                let transcript = fs::read_to_string(crate::runtime::rollout(&l.session).expect("native transcript")).unwrap();
                let inputs: Vec<String> = transcript.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
                    .filter(|v| v["type"] == "response_item" && v["payload"]["role"] == "user")
                    .filter_map(|v| v["payload"]["content"].as_array().map(|items| items.iter().filter_map(|v| v["text"].as_str()).collect::<Vec<_>>().join("\n")))
                    .collect();
                let loaded = rules.each_ref().map(|r| inputs.iter().filter(|text| text.contains(r.as_str())).count());
                assert_eq!(loaded, [1, turn], "contract, core and index at start; header and index at each resume");
                let proof: Value = events.iter().filter(|v| v["type"] == "item.completed" && v["item"]["type"] == "command_execution")
                    .filter_map(|v| serde_json::from_str::<Value>(v["item"]["aggregated_output"].as_str()?).ok())
                    .filter(|v| v["stage"] == turn.to_string()).last().expect("real command proof");
                assert_eq!(proof, serde_json::from_slice::<Value>(&fs::read(root.join("proof.json")).unwrap()).unwrap());
                assert_eq!(proof["role"], *role);
                assert_eq!(proof["allowed"], true);
                assert_eq!(fs::read_to_string(root.join("allowed.txt")).unwrap(), format!("{role}:{turn}"));
                assert_eq!(proof["allowed_mutations"]["write"]["readback"], true, "{proof}");
                assert_eq!(proof["allowed_mutations"]["rename"]["readback"], true, "{proof}");
                assert_eq!(proof["allowed_mutations"]["delete"]["deleted"], true, "{proof}");
                for key in ["outside", "policy", "config", "runtime_config", "junction", "git", "agents", "aws"] {
                    assert_eq!(proof[key]["result"], "denied", "{proof}");
                    assert_eq!(proof[key]["errno"], 13, "OS access denial, not a missing file: {proof}");
                    assert_eq!(proof[key]["native_error"], 5, "native write handle must be denied: {proof}");
                    for action in ["delete", "rename", "replace"] {
                        let mutation = &proof["mutations"][key][action];
                        assert_eq!(mutation["allowed"], false, "native {action} {key}: {proof}");
                        assert_eq!(mutation["native_error"], 5, "OS denial, not a missing target: {proof}");
                        assert!(mutation["before"]["ino"].as_u64().is_some_and(|id| id != 0), "file identity missing: {proof}");
                        assert!(mutation["before"]["sha256"].as_str().is_some_and(|s| s.len() == 64), "content digest missing: {proof}");
                        assert_eq!(mutation["before"], mutation["after"], "native {action} changed {key}: {proof}");
                    }
                }
                assert_ne!(proof["child"]["exit"], 0); assert_ne!(proof["acl"]["exit"], 0);
                assert_eq!(proof["after_acl"]["errno"], 13);
                assert_ne!(proof["nested_launcher"]["exit"], 0);
                assert!(proof["nested_launcher"]["error"].as_str().unwrap().contains("unsupported bootstrap argument"), "{proof}");
                for path in &canaries { assert_eq!(fs::read_to_string(path).unwrap(), "UNCHANGED\n", "{}", path.display()); }
                let reply = events.iter().filter(|v| v["item"]["type"] == "agent_message").filter_map(|v| v["item"]["text"].as_str()).collect::<Vec<_>>().join("\n");
                if turn == 1 { assert!(reply.contains("empty") && reply.contains('9') && reply.contains('0'), "must detect semantic regression: {reply}"); }
                println!("QUALIFIED Windows Workspace {role} turn={turn} session={} retired_process_exit={code} proof={proof}", l.session);
            }
            let store = install.join("worker-policy.json"); let saved = fs::read(&store).unwrap();
            crate::policy::grant(&install, "codex", "none", Some(&root), "qualification revocation", 2).unwrap();
            let refused = crate::runtime::spawn(&env, &l).err(); fs::write(&store, saved).unwrap();
            assert!(refused.is_some(), "revoked continuation must not launch");
        }
    }
}
