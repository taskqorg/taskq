//! TaskQ authority and native process ownership around AgentQ's launch/resume API.
use super::*;
use agentq_runtime::agent::{Approval, Launch as Request, Mode, Run};

pub(super) fn request(env: &Env, l: &Launch, log: &Path, text: &str, access: &crate::policy::Effective) -> Result<Request, String> {
    let base = cli(&l.rt, access, env)?;
    let mut request = Request::new(env.root, log, text, "taskq");
    request.command = std::iter::once(base.get_program()).chain(base.get_args()).map(Into::into).collect();
    request.env = spec(env, l, env.root.into(), vec![]).env;
    // AgentQ leaves its launcher marker to a TaskQ launch: without it, only a service that inherited one marks its workers.
    request.env.push(("AGENTQ_LAUNCHER".into(), "taskq".into()));
    request.home = accounts::log_home(&l.rt, log);
    request.install = agentq_runtime::optimization::install_root();
    let intent = request.env.iter().rev().find(|(key,_)|key=="AGENTQ_OPTIMIZATION").map(|(_,value)|value.clone()).or_else(||std::env::var_os("AGENTQ_OPTIMIZATION"));
    request.optimization = intent.map_or(Ok(agentq_runtime::optimization::Intent::Inherit), |value|
        value.to_str().and_then(agentq_runtime::optimization::Intent::parse).ok_or("AGENTQ_OPTIMIZATION must be inherit, off or on"))?;
    let a = of(&l.rt);
    let build = build_cache_for(&l.rt, access, env);
    if let Some(path) = &build { request.env.push(("RUSTC_WRAPPER".into(), path.as_os_str().into())); }
    if a.has(Cap::SandboxArgs) { request.host_args = access.codex.as_deref().unwrap_or(env.codex).iter().map(Into::into).collect(); }
    else if a.has(Cap::RemoteControl) {
        request.policy.approval = match access.claude.as_deref().unwrap_or(env.permission_mode) {
            "dontAsk" => Approval::Deny, "bypassPermissions" => Approval::Bypass, "acceptEdits" => Approval::Edits,
            "auto" => Approval::Auto, "plan" => Approval::Plan, "default" => Approval::Native,
            _ => return Err("unsupported Claude permission mode; no launch".into()),
        };
        let settings: serde_json::Value = serde_json::from_str(&compact_settings(env.root, env.install, l, env.jobs, build.as_deref())).map_err(|e| e.to_string())?;
        request.compact_hook = settings["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().map(Into::into);
        request.settings_env = settings["env"].as_object().ok_or("session environment missing")?.iter()
            .map(|(key, value)| Ok((key.clone(), value.as_str().ok_or("session environment invalid")?.into()))).collect::<Result<_, &str>>()?;
        request.title = Some(session_name(l));
    } else if a.has(Cap::Grant) && matches!(access.mode.as_str(), "workspace" | "full") { request.policy.approval = Approval::Bypass; }
    let vars: Vec<_> = base.get_envs().map(|(key, value)| (key.to_os_string(), value.map(std::ffi::OsStr::to_os_string))).collect();
    let linux = access.linux;
    let operator = l.role == "operator";
    if operator {
        request.env.retain(|(key,_)| !["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT"].iter().any(|name|key==name));
        request.env.extend(["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT"].map(|key|(key.into(), "".into())));
    }
    request.process.configure = Some(std::sync::Arc::new(move |command| {
        crate::platform::quiet(command);
        for (key, value) in &vars { match value { Some(value) => {command.env(key, value);}, None => {command.env_remove(key);} } }
        if operator { for key in ["TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT"] { command.env_remove(key); } }
        #[cfg(target_os="linux")] if linux { crate::linux_sandbox::own(command); }
        #[cfg(not(target_os="linux"))] let _ = linux;
    }));
    Ok(request)
}

pub(super) fn submit(rt: &str, session: Option<&str>, request: &Request) -> Result<Run, String> {
    let adapter = agent(rt)?;
    let run = match session { Some(session) => adapter.resume(session, request), None => adapter.launch(request) }?;
    eprintln!("AgentQ optimization ({}): {}", of(rt).agent, run.optimization().json());
    Ok(run)
}

pub(super) fn background(env: &Env, l: &Launch, log: &Path, input: &str, access: &crate::policy::Effective) -> Result<Result<std::process::Output, coreq::process::Failure>, StartError> {
    let mut request = request(env, l, log, input, access)?;
    request.mode = Mode::Background;
    let mut run = submit(&l.rt, l.resume.as_ref().map(|_| l.session.as_str()), &request)?;
    agent(&l.rt)?.launcher_output(&mut run).map_err(|cause| StartError::from(cause.0))
}

pub(super) struct Native {
    rt: String,
    run: Run,
    #[cfg(windows)] scope: windows_process::Execution,
}

impl Proc for Native {
    fn native_identity(&self) -> Result<coreq::process::Identity, String> {
        #[cfg(windows)] { self.scope.native_identity() }
        #[cfg(not(windows))] { coreq::process::identity(self.id()).map_err(|e|e.to_string())?.ok_or("native process already exited".into()) }
    }
    fn job(&self) -> Option<String> {
        #[cfg(windows)] { self.scope.job() }
        #[cfg(not(windows))] { None }
    }
    fn id(&self) -> u32 { self.run.id().expect("CLI launch has a child") }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        let adapter = agent(&self.rt)?;
        let state = adapter.poll(&mut self.run)?;
        #[cfg(windows)] let scope_exit = match self.scope.exited()? {Some(code)=>code,None=>return Ok(None)};
        if state == agentq_runtime::agent::State::Running { return Ok(None); }
        // A terminal report is distinct from native process exit, which it must explicitly carry.
        let exit = adapter.report(&self.run)?.exit.ok_or("native process exit unconfirmed")?;
        #[cfg(windows)] {let _=exit;Ok(Some(scope_exit))}
        #[cfg(not(windows))] {Ok(Some(exit))}
    }
    fn kill(&mut self) -> Result<(), String> {
        #[cfg(windows)] self.scope.kill()?;
        agent(&self.rt)?.cancel(&mut self.run).map(|_| ())
    }
}

pub(super) fn start(env: &Env, l: &Launch, log: &Path, text: &str, access: &crate::policy::Effective) -> Result<Box<dyn Proc>, StartError> {
    let mut request = request(env, l, log, text, access)?;
    native(&l.rt, l.resume.as_ref().map(|_|l.session.as_str()), &mut request).map(|native|Box::new(native) as Box<dyn Proc>)
}

fn native(rt: &str, session: Option<&str>, request: &mut Request) -> Result<Native, StartError> {
    #[cfg(windows)] let scope = windows_process::Execution::launch_hooks(&mut request.process)?;
    let run = submit(rt, session, request)?;
    #[cfg(windows)] let scope = scope.lock().unwrap_or_else(|e|e.into_inner()).take().expect("spawn hook keeps its execution scope");
    Ok(Native { rt:rt.into(), run, #[cfg(windows)] scope })
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
