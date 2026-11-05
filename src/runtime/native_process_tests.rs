//! Native no-input and file-transport instruments; product launches use Runtime::launch.
use super::*;

pub fn command(env: &Env, l: &Launch, access: &crate::policy::Effective) -> Result<Command, String> {
    let codex = access.codex.as_deref().unwrap_or(env.codex);
    let mode = access.claude.as_deref().unwrap_or(env.permission_mode);
    let build = build_cache_for(&l.rt, access, env);
    let a = of(&l.rt);
    let args: Vec<std::ffi::OsString> = if a.has(Cap::RemoteControl) {
        ["--permission-mode", mode, "--settings", &compact_settings(env.root, env.install, l, env.jobs, build.as_deref())].map(Into::into).to_vec()
    } else if a.has(Cap::SandboxArgs) { codex.iter().map(Into::into).collect() }
    else if a.has(Cap::Grant) && access.refused.is_none() && matches!(access.mode.as_str(), "workspace" | "full") {
        vec!["--dangerously-skip-permissions".into()]
    } else { vec![] };
    let mut c = agent(&l.rt)?.command(cli(&l.rt, access, env)?, &optimized(&l.rt, spec(env, l, env.root.into(), args))?)?;
    if let Some(p) = build { c.env("RUSTC_WRAPPER", p); }
    Ok(c)
}

/// Prepare stdin before spawn: a child that never reads cannot block its execution owner.
pub fn start_command(command: &mut Command, root: &Path, text: &str) -> Result<Box<dyn Proc>, StartError> {
    coreq::telemetry::capture::operation("runtime.launch", || {
        let (path, _input) = turn_input(root, text, None).map_err(StartError::from)?;
        let input = File::open(path).map_err(|e| StartError::io(e, None))?;
        start_file(command,input)
    }, |_| "launch")
}

pub fn start_file(command:&mut Command,input:File)->Result<Box<dyn Proc>,StartError> {
    coreq::telemetry::capture::inherit_context(command);
    command.stdin(input);
    #[cfg(windows)] {windows_process::Execution::spawn(command).map(|child|Box::new(child) as Box<dyn Proc>)}
    #[cfg(not(windows))] {command.spawn().map(|child|Box::new(child) as Box<dyn Proc>).map_err(|e|StartError::io(e,None))}
}

