//! Native no-input and file-transport instruments; product launches use Runtime::launch.
use super::*;

pub fn command(
    env: &Env,
    l: &Launch,
    access: &crate::policy::Effective,
) -> Result<Command, String> {
    let codex = access.codex.as_deref().unwrap_or(env.codex);
    let mode = access.claude.as_deref().unwrap_or(env.permission_mode);
    let build = build_cache_for(&l.rt, access, env);
    let a = of(&l.rt);
    let args: Vec<std::ffi::OsString> = if a.has(Cap::RemoteControl) {
        [
            "--permission-mode",
            mode,
            "--settings",
            &compact_settings(env.root, env.install, l, env.jobs, build.as_deref()),
        ]
        .map(Into::into)
        .to_vec()
    } else if a.has(Cap::SandboxArgs) {
        codex.iter().map(Into::into).collect()
    } else if a.has(Cap::Grant)
        && access.refused.is_none()
        && matches!(access.mode.as_str(), "workspace" | "full")
    {
        vec!["--dangerously-skip-permissions".into()]
    } else {
        vec![]
    };
    let selected = spec(env, l, env.root.into(), args);
    let mut c = cli(&l.rt, access, env)?;
    c.args(agent(&l.rt)?.native_arguments(&selected)?)
        .envs(selected.env)
        .current_dir(&selected.root);
    if let Some(home) = selected.home {
        c.env(a.home_var, home);
    }
    if let Some(p) = build {
        c.env("RUSTC_WRAPPER", p);
    }
    Ok(c)
}

/// Prepare stdin before spawn: a child that never reads cannot block its execution owner.
pub fn start_command(
    command: &mut Command,
    root: &Path,
    text: &str,
) -> Result<Box<dyn Proc>, StartError> {
    coreq::telemetry::capture::operation(
        "runtime.launch",
        || {
            let (path, _input) = turn_input(root, text, None).map_err(StartError::from)?;
            let input = File::open(path).map_err(|e| StartError::io(e, None))?;
            start_file(command, input)
        },
        |_| "launch",
    )
}

pub fn start_file(command: &mut Command, input: File) -> Result<Box<dyn Proc>, StartError> {
    coreq::telemetry::capture::inherit_context(command);
    command.stdin(input);
    #[cfg(windows)]
    {
        windows_process::Execution::spawn(command).map(|child| Box::new(child) as Box<dyn Proc>)
    }
    #[cfg(not(windows))]
    {
        command
            .spawn()
            .map(|child| Box::new(child) as Box<dyn Proc>)
            .map_err(|e| StartError::io(e, None))
    }
}

/// Invented native rows enter through the public decoder, never a consumer constructor.
pub(crate) fn background_rows(
    rows: &[serde_json::Value],
    root: &Path,
    home: &Path,
) -> Result<Vec<agentq_runtime::agent::BackgroundCandidate>, String> {
    let mut rows = rows.to_vec();
    for row in &mut rows {
        let object = row
            .as_object_mut()
            .ok_or("invented row must be an object")?;
        object
            .entry("kind")
            .or_insert(serde_json::json!("background"));
        object.entry("id").or_insert(serde_json::json!("c37eb75a"));
        object
            .entry("sessionId")
            .or_insert(serde_json::json!("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"));
        object.entry("cwd").or_insert(serde_json::json!(root));
    }
    let python = super::which(if cfg!(windows) { "python" } else { "python3" });
    let mut command = crate::platform::command(python);
    command.args([
        "-c",
        "import sys;print(sys.argv[1])",
        &serde_json::to_string(&rows).unwrap(),
    ]);
    super::agent(super::CLAUDE)?.background_candidates(
        command,
        &super::claude_spec(root, Some(home)),
        false,
        std::time::Duration::from_secs(10),
    )
}
