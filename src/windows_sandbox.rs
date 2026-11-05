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

/// Removes a stale preparation test fixture whose denials refuse a plain removal, restoring them from its own journal
/// first (#2417). A real preparation journal lives under the installation, never in such a folder.
pub fn remove_denied_fixture(base: &Path) -> Result<(), String> { preparation::remove_fixture(base) }

#[cfg(test)]
pub(crate) use preparation::tests::prepared_fixture;

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
    let out = match coreq::process::output("sandbox.preflight", crate::platform::command(&p.bin).args(["-c", "windows.sandbox=unelevated", "sandbox", "-P", "taskq", "--include-managed-config", "--", "cmd.exe", "/d", "/c", "exit 0"])
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
    let mut c = crate::platform::command(p.bin);
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
    if !policy.grants.iter().any(|g| crate::runtime::of(&g.runtime).has("sandbox-args") && crate::runtime::of(&g.runtime).has("cli") && g.mode == "workspace" && g.project.as_deref() == Some(scope.as_str())) {
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
    let status = crate::platform::command(p.bin).args(["exec", "--add-dir"]).arg(home).args(&args[4..])
        .env("CODEX_HOME", &args[2]).env("TASKQ_WINDOWS_SANDBOX_HOME", p.state)
        .current_dir(root).status().map_err(|e| e.to_string())?;
    crate::platform::exit(status.code().unwrap_or(1));
}

pub fn restore(args: &[String]) -> Result<(), String> {
    if args.len() != 1 { return Err("Windows security restore requires one installation path".into()); }
    preparation::restore(&checked(Path::new(&args[0]))?)
}

#[cfg(test)]
#[path = "windows_sandbox_tests.rs"]
mod tests;
