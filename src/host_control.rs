//! Short native controller outside the host unit; task/worker ownership stays with the host.
use crate::{Res, lifecycle};
use coreq::process::{Identity,Failure};
use std::{fs,path::{Path,PathBuf},process::Command,time::{Duration,Instant}};

pub struct Control {pub id:String,worker:Option<std::thread::JoinHandle<Res<()>>>}
impl Control {
    pub fn poll(&mut self)->Option<Res<()>> {
        if !self.worker.as_ref().is_some_and(|worker|worker.is_finished()) {return None;}
        Some(self.worker.take().unwrap().join().unwrap_or_else(|_|Err("Stop controller owner panicked; outcome unknown".into())))
    }
    pub fn wait(mut self)->Res<()> {
        self.worker.take().map_or(Ok(()),|worker|worker.join().unwrap_or_else(|_|Err("Stop controller owner panicked; outcome unknown".into())))
    }
}

pub(crate) fn definition(tool:&coreq::Tool)->Res<PathBuf> {
    if cfg!(windows) {return Ok(tool.install.join(format!("{}.xml",tool.title)));}
    let home=crate::platform::user_home().ok_or("native user home unknown")?;
    Ok(home.join(if cfg!(target_os="macos") {format!("Library/LaunchAgents/com.{}.service.plist",tool.name)}else{format!(".config/systemd/user/{}.service",tool.name)}))
}
pub fn revision(install:&Path)->Res<String> {revision_for(&crate::platform::tool(install))}

/// Read native scope before install attaches a root; a daemon projection alone cannot authorize a unit edit.
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum InstallState {Absent,Saved,Loaded}

pub fn install_state(tool:&coreq::Tool,owner:Option<&Identity>)->Res<InstallState> {
    let saved=match fs::symlink_metadata(definition(tool)?) {
        Ok(_)=>{revision_for(tool)?;true},
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>false,
        Err(e)=>return Err(format!("host definition unknown: {e}")),
    };
    if saved {verify_saved_install_scope(tool)?;}
    if native_absent(tool)? {
        if owner.is_some() {return Err("live host has no native unit; installation refused".into());}
        return Ok(if saved {InstallState::Saved}else{InstallState::Absent});
    }
    if !saved {
        #[cfg(not(windows))] return Err("native host has no saved definition; installation refused".into());
        #[cfg(windows)] {export_definition(tool)?;return install_state(tool,owner);}
    }
    #[cfg(target_os="macos")] {
        let unit=coreq::service::unit(tool,&tool.install,"");
        let target=unit.stop.first().and_then(|command|command.last()).ok_or("native host target missing")?;
        let exe=tool.install.join(tool.exe_name());
        let native=output("host.install.owner",crate::platform::command("launchctl").args(["print",target]))?;
        let text=String::from_utf8(native.stdout).map_err(|e|e.to_string())?;
        let field=|key:&str|text.lines().find_map(|line|line.trim().strip_prefix(key));
        if field("program = ")!=exe.to_str()||field("working directory = ")!=tool.install.to_str()
            ||field("path = ")!=unit.path.to_str() {return Err("loaded host unit scope differs; kept".into());}
        let arguments:Vec<_>=text.lines().skip_while(|line|line.trim()!="arguments = {").skip(1).take_while(|line|line.trim()!="}").map(str::trim).collect();
        if arguments!=[exe.to_str().ok_or("host executable is not UTF-8")?,"service","run"] {return Err("loaded host arguments differ; kept".into());}
        let pid=field("pid = ").map(str::parse::<u32>).transpose().map_err(|e|e.to_string())?.unwrap_or(0);
        verify_install_pid(pid,owner)?;
    }
    #[cfg(target_os="linux")] {
        let unit=coreq::service::unit(tool,&tool.install,"");
        let exe=tool.install.join(tool.exe_name());
        let query=output("host.install.owner",crate::platform::command("systemctl").args(["--user","show",&format!("{}.service",tool.name),"--property=MainPID","--property=ExecStart","--property=WorkingDirectory","--property=FragmentPath","--no-pager"]))?;
        let text=String::from_utf8(query.stdout).map_err(|e|e.to_string())?;
        let field=|key:&str|text.lines().find_map(|line|line.strip_prefix(key));
        if field("WorkingDirectory=")!=tool.install.to_str()||field("FragmentPath=")!=unit.path.to_str()
            ||!field("ExecStart=").is_some_and(|value|value.starts_with(&format!("{{ path={} ; argv[]={} service run ;",exe.display(),exe.display()))) {return Err("loaded host unit scope differs; kept".into());}
        let pid=field("MainPID=").ok_or("native host pid unknown")?.parse::<u32>().map_err(|e|e.to_string())?;
        verify_install_pid(pid,owner)?;
    }
    #[cfg(windows)] {
        let code=format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$t=$s.GetFolder('\').GetTask({name});$d=$t.Definition;[xml]$xml=$t.Xml;[xml]$saved=Get-Content -Raw -LiteralPath {definition};$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$a=$d.Actions.Item(1);$e=$saved.Task.Actions.Exec
$principal=[string]$xml.Task.Principals.Principal.UserId
$owner=if($principal.StartsWith('S-1-',[StringComparison]::OrdinalIgnoreCase)){{[Security.Principal.SecurityIdentifier]::new($principal).Value}}else{{[Security.Principal.NTAccount]::new($principal).Translate([Security.Principal.SecurityIdentifier]).Value}}
if($d.Principal.LogonType -ne 3 -or $d.Principal.RunLevel -ne 0 -or $owner -ne $sid -or $d.Actions.Count -ne 1 -or @($e).Count -ne 1 -or !(Test-TaskQLauncher $a.Path) -or $a.Arguments -ne 'service run' -or !(Test-CoreQPath $a.WorkingDirectory {cwd}) -or !(Test-CoreQPath $a.Path ([string]$e.Command)) -or $a.Arguments -ne [string]$e.Arguments -or !(Test-CoreQPath $a.WorkingDirectory ([string]$e.WorkingDirectory))){{throw 'host task scope differs'}}
$logon=@($d.Triggers|Where-Object {{$_.Type -eq 9}});$time=@($d.Triggers|Where-Object {{$_.Type -eq 1}})
if($logon.Count -ne 1 -or !$logon[0].Enabled -or $time.Count -gt 1 -or $d.Triggers.Count -ne $logon.Count+$time.Count -or ($time.Count -eq 1 -and $time[0].Repetition.Interval -ne 'PT5M')){{throw 'host task trigger differs'}}
$principal=[string]$logon[0].UserId
$trigger=if($principal.StartsWith('S-1-',[StringComparison]::OrdinalIgnoreCase)){{[Security.Principal.SecurityIdentifier]::new($principal).Value}}else{{[Security.Principal.NTAccount]::new($principal).Translate([Security.Principal.SecurityIdentifier]).Value}}
if($trigger -ne $sid){{throw 'host task trigger owner differs'}}
if({pid} -ne 0){{$acl=[Security.AccessControl.RawSecurityDescriptor]::new($t.GetSecurityDescriptor(4));$aces=@($acl.DiscretionaryAcl)
if(($acl.ControlFlags -band [Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -eq 0 -or $aces.Count -ne 2){{throw 'host task requires explicit idle installer repair'}}
foreach($who in @($sid,'S-1-5-18')){{$rights=@($aces|Where-Object {{$_.AceQualifier -eq 'AccessAllowed' -and $_.SecurityIdentifier.Value -eq $who -and (($_.AccessMask -band 0x10000000) -ne 0 -or ($_.AccessMask -band 0x1f01ff) -eq 0x1f01ff)}});if($rights.Count -ne 1){{throw 'host task management rights not observed; drain before installer repair'}}}}}}
$instances=@(Get-TaskQInstances $s $t);if($instances.Count -gt 1){{throw 'multiple native host instances'}}
if($instances.Count -eq 0){{if({pid} -ne 0){{throw 'live host has no scheduler instance'}};exit 0}}
if({pid} -eq 0){{throw 'scheduler instance has no attributable host'}}
$engine=$instances[0].EnginePID;$daemon=Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}'
if(!$daemon -or $daemon.ParentProcessId -ne $engine -or (Get-Process -Id {pid}).StartTime.ToFileTimeUtc().ToString() -ne {started}){{throw 'native host incarnation differs'}}
"#,name=quote(tool.title),definition=quote(&definition(tool)?.to_string_lossy()),cwd=quote(&tool.install.to_string_lossy()),pid=owner.map_or(0,|p|p.pid),started=quote(owner.map_or("",|p|p.started.as_str())));
        let code=format!("{}\n{}\n{code}",coreq::service::windows_path_identity(),launcher_identity(tool));
        output("host.install.owner",&mut powershell(&code))?;
    }
    Ok(InstallState::Loaded)
}

/// Starting a verified saved/idle definition never replaces its configuration.
pub struct InstallFailure {pub submitted:bool,pub reason:String}
impl InstallFailure {
    fn before(reason:String)->Self {Self {submitted:false,reason}}
    fn unknown(reason:String)->Self {Self {submitted:true,reason}}
}
pub fn start_install(tool:&coreq::Tool,state:InstallState)->Result<(),InstallFailure> {
    #[cfg(windows)] scheduler_write(tool.title).map_err(InstallFailure::before)?;
    if install_state(tool,None).map_err(InstallFailure::before)?!=state {return Err(InstallFailure::before("native host scope changed before start; kept".into()));}
    let unit=coreq::service::unit(tool,&tool.install,"");
    if state==InstallState::Absent {
        #[cfg(windows)] coreq::service::host_unit(tool).map_err(InstallFailure::unknown)?;
        #[cfg(not(windows))] {
            coreq::service::write_unit(&unit).map_err(InstallFailure::before)?;
            commands("host.install.start",&unit.load).map_err(InstallFailure::unknown)?;
        }
        return Ok(());
    }
    #[cfg(target_os="macos")] {
        if state==InstallState::Saved {commands("host.install.start",&unit.load).map_err(InstallFailure::unknown)?;}
        else {
            let target=unit.stop.first().and_then(|command|command.last()).ok_or_else(||InstallFailure::before("native host target missing".into()))?;
            output("host.install.start",crate::platform::command("launchctl").args(["kickstart",target])).map_err(InstallFailure::unknown)?;
        }
    }
    #[cfg(target_os="linux")] {commands("host.install.start",&unit.load).map_err(InstallFailure::unknown)?;}
    #[cfg(windows)] {
        let code=if state==InstallState::Saved {format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;$xml=[IO.File]::ReadAllText({path});$acl='D:P(A;;GA;;;'+$sid+')(A;;GA;;;SY)';$t=$s.GetFolder('\').RegisterTask({name},$xml,18,$null,$null,3,$acl);$null=$t.Run($null)"#,path=quote(&unit.path.to_string_lossy()),name=quote(tool.title))}
            else {format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$null=$s.GetFolder('\').GetTask({}).Run($null)"#,quote(tool.title))};
        output("host.install.start",&mut powershell(&code)).map_err(InstallFailure::unknown)?;
    }
    Ok(())
}

fn verify_saved_install_scope(tool:&coreq::Tool)->Res<()> {
    #[cfg(target_os="macos")] {
        let saved=output("host.install.definition",crate::platform::command("plutil").args(["-convert","json","-o","-"]).arg(definition(tool)?))?;
        let saved:serde_json::Value=serde_json::from_slice(&saved.stdout).map_err(|e|e.to_string())?;
        if saved["Label"]!=format!("com.{}.service",tool.name)||saved["WorkingDirectory"]!=tool.install.to_string_lossy().as_ref()
            ||saved["ProgramArguments"]!=serde_json::json!([tool.install.join(tool.exe_name()),"service","run"]) {return Err("saved host unit scope differs; kept".into());}
    }
    #[cfg(target_os="linux")] {
        let text=fs::read_to_string(definition(tool)?).map_err(|e|e.to_string())?;
        if !text.lines().any(|line|line==format!("ExecStart=\"{}\" service run",tool.install.join(tool.exe_name()).display()))
            ||!text.lines().any(|line|line==format!("WorkingDirectory={}",tool.install.display())) {return Err("saved host unit scope differs; kept".into());}
    }
    #[cfg(windows)] {
        let code=format!(r#"$ErrorActionPreference='Stop';[xml]$saved=Get-Content -Raw -LiteralPath {definition};$e=$saved.Task.Actions.Exec
$p=$saved.Task.Principals.Principal;$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
function Get-Field($node,$name,$default){{$f=@($node.ChildNodes|Where-Object {{$_.LocalName -eq $name}});if($f.Count -eq 0){{$default}}elseif($f.Count -eq 1){{[string]$f[0].InnerText}}else{{$null}}}}
$principal=[string]$p.UserId
$owner=if($principal.StartsWith('S-1-',[StringComparison]::OrdinalIgnoreCase)){{[Security.Principal.SecurityIdentifier]::new($principal).Value}}else{{[Security.Principal.NTAccount]::new($principal).Translate([Security.Principal.SecurityIdentifier]).Value}}
if(@($saved.Task.Principals.Principal).Count -ne 1 -or $owner -ne $sid -or [string]$p.LogonType -ne 'InteractiveToken' -or (Get-Field $p 'RunLevel' 'LeastPrivilege') -ne 'LeastPrivilege' -or @($saved.Task.Actions.Exec).Count -ne 1 -or @($saved.Task.Actions.ChildNodes).Count -ne 1 -or !(Test-TaskQLauncher ([string]$e.Command)) -or [string]$e.Arguments -ne 'service run' -or !(Test-CoreQPath ([string]$e.WorkingDirectory) {cwd})){{throw 'saved host task scope differs'}}
$time=@($saved.Task.Triggers.ChildNodes|Where-Object {{$_.LocalName -eq 'TimeTrigger'}})
if($time.Count -gt 1 -or @($saved.Task.Triggers.ChildNodes).Count -ne 1+$time.Count -or @($saved.Task.Triggers.ChildNodes|Where-Object {{$_.LocalName -eq 'LogonTrigger'}}).Count -ne 1 -or (Get-Field $saved.Task.Triggers.LogonTrigger 'Enabled' 'true') -ne 'true' -or ($time.Count -eq 1 -and [string]$time[0].Repetition.Interval -ne 'PT5M')){{throw 'saved host task trigger differs'}}
$principal=[string]$saved.Task.Triggers.LogonTrigger.UserId
$trigger=if($principal.StartsWith('S-1-',[StringComparison]::OrdinalIgnoreCase)){{[Security.Principal.SecurityIdentifier]::new($principal).Value}}else{{[Security.Principal.NTAccount]::new($principal).Translate([Security.Principal.SecurityIdentifier]).Value}}
if($trigger -ne $sid){{throw 'saved host task trigger owner differs'}}
"#,definition=quote(&definition(tool)?.to_string_lossy()),cwd=quote(&tool.install.to_string_lossy()));
        let code=format!("{}\n{}\n{code}",coreq::service::windows_path_identity(),launcher_identity(tool));
        output("host.install.definition",&mut powershell(&code))?;
    }
    Ok(())
}

/// #1858: a live task with no saved definition (registered by an older installer) gets one exported from the task
/// itself; the export never replaces a file and is removed again unless it passes the saved scope check.
#[cfg(windows)]
fn export_definition(tool:&coreq::Tool)->Res<()> {
    let path=definition(tool)?;
    let temp=path.with_extension(format!("xml.export-{}",crate::runtime::uuid()));
    let code=format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$t=$s.GetFolder('\').GetTask({name})
try{{[IO.File]::WriteAllText({temp},$t.Xml,[Text.Encoding]::Unicode);[IO.File]::Move({temp},{path})}}finally{{Remove-Item -LiteralPath {temp} -Force -ErrorAction SilentlyContinue}}
"#,name=quote(tool.title),temp=quote(&temp.to_string_lossy()),path=quote(&path.to_string_lossy()));
    output("host.install.export",&mut powershell(&code)).map_err(|e|format!("native host has no saved definition; export from the live task failed: {e}"))?;
    if let Err(e)=revision_for(tool).and_then(|_|verify_saved_install_scope(tool)) {
        let _=fs::remove_file(&path);
        return Err(format!("native host has no saved definition; the live task export differs ({e}); export removed. Next: `schtasks /End /TN {name}`, `schtasks /Delete /TN {name} /F`, then repeat `service install`",name=tool.title));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_install_pid(pid:u32,owner:Option<&Identity>)->Res<()> {
    if pid==0&&owner.is_none() {return Ok(());}
    let expected=owner.ok_or("native host has no attributable service projection")?;
    if pid!=expected.pid||coreq::process::identity(pid).map_err(|e|e.to_string())?.as_ref()!=Some(expected) {return Err("native host incarnation differs; kept".into());}
    Ok(())
}
fn revision_for(tool:&coreq::Tool)->Res<String> {
    let path=definition(tool)?;let metadata=fs::symlink_metadata(&path).map_err(|e|format!("host unit definition {} unavailable: {e}",path.display()))?;
    if !metadata.is_file()||metadata.len()>65536 {return Err("host unit definition unsupported; kept".into());}
    fs::read(path).map(|bytes|ring::digest::digest(&ring::digest::SHA256,&bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()).map_err(|e|e.to_string())
}
fn output(name:&'static str,command:&mut Command)->Res<std::process::Output> {
    // ponytail: test runs share a loaded CI runner, where cold PowerShell starts passed 30 s (#1673, #1678 merge_group); product keeps 30 s.
    match coreq::process::output(name,command,Duration::from_secs(if cfg!(test) {120} else {30})) {
        Ok(output)=>Ok(output),Err(Failure::Exit(output))=>Err(format!("{name}: {}: {}",output.status,String::from_utf8_lossy(&output.stderr))),Err(error)=>Err(format!("{name}: {error}")),
    }
}
fn commands(name:&'static str,commands:&[Vec<String>])->Res<()> {
    for args in commands {
        let (program,args)=args.split_first().ok_or("native command missing")?;
        output(name,crate::platform::command(program).args(args))?;
    }
    Ok(())
}
/// #2049 A3: a test build never registers, changes or runs a real Scheduled Task. The product unit (`TaskQ`) and its
/// `TaskQ-Stop-<id>` controllers stay untouched; native fixtures register their own `…-Probe-<nonce>` unit, and a Stop
/// controller only for the owned `TASKQ_NATIVE_STOP_CASE` root. Product builds always pass.
#[cfg(any(windows,test))]
fn scheduler_write(title:&str)->Res<()> {
    if !cfg!(test) {return Ok(());}
    let controller=title.starts_with("TaskQ-Stop-")&&std::env::var_os("TASKQ_NATIVE_STOP_CASE").is_none();
    if title==crate::platform::tool(Path::new("")).title||controller {return Err(format!("a test cannot register or change the real Scheduled Task {title}"));}
    Ok(())
}
#[cfg(windows)]
fn quote(value:&str)->String {format!("'{}'",value.replace('\'',"''"))}
#[cfg(windows)]
pub(crate) fn powershell(code:&str)->Command {
    // GetInstances(0) omits hidden tasks; match the documented hidden-inclusive query by exact task path.
    let code=format!("function Get-TaskQInstances($service,$task) {{ $service.GetRunningTasks(1) | Where-Object {{ $_.Path -eq $task.Path }} }}\n{code}");
    let bytes:Vec<u8>=code.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut command=crate::platform::command("powershell.exe");command.args(["-NoProfile","-NonInteractive","-WindowStyle","Hidden","-EncodedCommand",&crate::tray::base64(&bytes)]);command
}

/// #1625: the CoreQ unit's 5-minute repeat restarts an exited host; Stop disables only it, each host start enables it
/// (adding it to a pre-v0.14.7 unit). Only this install's own unit is edited; a missing unit or exe is `absent`.
#[cfg(windows)]
pub fn repeat_trigger(tool:&coreq::Tool,enable:bool)->Res<&'static str> {
    let exe=tool.install.join(tool.exe_name());
    if !exe.is_file() {return Ok("absent");}
    scheduler_write(tool.title)?;
    let code=format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\')
try{{$t=$r.GetTask({name})}}catch{{if(($_.Exception.GetBaseException()).HResult -ne -2147024894){{throw}};'absent';exit 0}}
$d=$t.Definition;$a=$d.Actions.Item(1)
if($d.Actions.Count -ne 1 -or $a.Arguments -ne 'service run' -or !(Test-TaskQLauncher $a.Path)){{'absent';exit 0}}
$time=@($d.Triggers|Where-Object {{$_.Type -eq 1}});if($time.Count -gt 1 -or ($time.Count -eq 1 -and $time[0].Repetition.Interval -ne 'PT5M')){{throw 'host task repeat trigger differs'}}
if({enable}){{if($time.Count -eq 1 -and $time[0].Enabled -and $d.Settings.MultipleInstances -eq 2){{'kept';exit 0}}
if($time.Count -eq 0){{$x=$d.Triggers.Create(1);$x.StartBoundary='2026-01-01T00:00:00';$x.Repetition.Interval='PT5M';$x.Repetition.StopAtDurationEnd=$false}}else{{$x=$time[0]}}
$x.Enabled=$true;$d.Settings.MultipleInstances=2}}else{{if($time.Count -eq 0 -or !$time[0].Enabled){{'kept';exit 0}};$time[0].Enabled=$false}}
$null=$r.RegisterTaskDefinition({name},$d,4,$null,$null,3,$null);'changed'"#,name=quote(tool.title),enable=if enable {"$true"}else{"$false"});
    let code=format!("{}\n{}\n{code}",coreq::service::windows_path_identity(),launcher_identity(tool));
    let out=output("host.unit.repeat",&mut powershell(&code))?;
    match String::from_utf8_lossy(&out.stdout).trim() {"absent"=>Ok("absent"),"kept"=>Ok("kept"),"changed"=>Ok("changed"),other=>Err(format!("host task repeat trigger unknown: {other}"))}
}

/// #2049 A1: CoreQ units start the GUI launcher `<name>w.exe` (no console); a unit registered by an older binary still
/// names the console launcher `<name>.exe`. Both are this install's launcher; a missing one never matches.
#[cfg(windows)]
fn launcher_identity(tool:&coreq::Tool)->String {
    format!("$launchers=@({},{}|Where-Object {{Test-Path -LiteralPath $_ -PathType Leaf}})\nfunction Test-TaskQLauncher([string]$path){{@($launchers|Where-Object {{Test-CoreQPath $path $_}}).Count -gt 0}}",
        quote(&tool.install.join(tool.exe_name()).to_string_lossy()),quote(&tool.install.join(tool.gui_exe_name()).to_string_lossy()))
}

/// #2049 A1: the Stop controller task starts the GUI launcher, which runs the current release without a console; the
/// console launcher (it flashes a window) only where an older install has no GUI launcher yet. ponytail: the
/// controller no longer continues the dispatching trace (a task action sets no environment); the CLI has no collector.
#[cfg(windows)]
fn controller_action(install:&Path,id:&str,action:&str)->Res<(PathBuf,String)> {
    if !crate::resources::id(id)||action!="complete-stop" {return Err("native controller scope refused".into());}
    let install=install.canonicalize().map_err(|e|format!("controller installation identity unknown: {e}"))?;
    let tool=crate::platform::tool(&install);
    let exe=[tool.gui_exe_name(),tool.exe_name()].into_iter().map(|name|install.join(name)).find(|exe|exe.is_file()).ok_or("controller launcher missing; run `taskq update`")?;
    Ok((exe,format!("service {action} --request {id}")))
}

/// Dispatch only a prepared, immutable command. Repeated dispatch is serialized again by projects.lock.
pub fn dispatch(install:&Path,pending:&lifecycle::Pending)->Res<Control> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some() {return Err("a worker cannot dispatch host Stop".into());}
    if !pending.stops()||!pending.prepared() {return Err("Stop has no qualified prepared intent".into());}
    dispatch_command(install,pending.id(),"complete-stop",&pending.context()?)
}

#[cfg(target_os="macos")]
fn controller_command(exe:&Path)->Command {
    use std::os::unix::process::CommandExt;
    // CoreQ captures pipes; exec redirects them before the controller outlives their reader.
    let mut command=crate::platform::command("/bin/sh");
    command.args(["-c","exec \"$@\" </dev/null >/dev/null 2>&1","taskq-controller"]).arg(exe).process_group(0);
    command
}

pub(crate) fn dispatch_command(install:&Path,id:&str,action:&str,context:&str)->Res<Control> {
    if std::env::var_os("TASKQ_ATTEMPT").is_some()||!crate::resources::id(id)||action!="complete-stop" {return Err("native controller scope refused".into());}
    coreq::telemetry::capture::TraceContext::decode(context)?;
    #[cfg(not(windows))] let exe=std::env::current_exe().map_err(|e|e.to_string())?;
    #[cfg(target_os="macos")] {
        let mut command=controller_command(&exe);
        command.args(["service",action,"--request",id]);
        command.current_dir(install)
            .env("TASKQ_INSTALL",install).env("COREQ_TRACE_CONTEXT",&context).env_remove("TASKQ_ROOT").env_remove("TASKQ_ATTEMPT").env_remove("TASKQ_TASK");
        let context=context.to_string();
        let worker=std::thread::Builder::new().name("taskq-stop-controller".into()).spawn(coreq::telemetry::capture::bind(move || {
            let context=coreq::telemetry::capture::TraceContext::decode(&context)?;let _context=context.span().entered();
            coreq::process::output("host.stop.dispatch",&mut command,Duration::from_secs(120)).map(|_|()).map_err(|e|e.to_string())
        })).map_err(|e|format!("Stop controller not started: {e}"))?;
        return Ok(Control {id:id.into(),worker:Some(worker)});
    }
    #[cfg(target_os="linux")] {
        output("host.stop.dispatch",crate::platform::command("systemd-run").args(["--user","--quiet","--collect","--property=RuntimeMaxSec=120"])
            .arg(format!("--unit=taskq-stop-{id}")).arg(format!("--working-directory={}",install.display()))
            .arg(format!("--setenv=TASKQ_INSTALL={}",install.display())).arg(format!("--setenv=COREQ_TRACE_CONTEXT={context}"))
            .arg(exe).args(["service",action,"--request",id]))?;
    }
    #[cfg(windows)] {
        scheduler_write(&format!("TaskQ-Stop-{id}"))?;
        let (exe,args)=controller_action(install,id,action)?;
        let code=format!(r#"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\');$name={name};$path={exe};$args={args};$cwd={cwd};$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
try{{$t=$r.GetTask($name)}}catch{{if(($_.Exception.GetBaseException()).HResult -ne -2147024894){{throw}};$t=$null}}
if($t){{$a=$t.Definition.Actions.Item(1);if($t.Definition.Principal.LogonType -ne 3 -or $t.Definition.Principal.RunLevel -ne 0 -or ($t.Definition.Principal.UserId -ne $identity.Name -and $t.Definition.Principal.UserId -ne $identity.User.Value) -or $t.Definition.Triggers.Count -ne 0 -or $t.Definition.Settings.ExecutionTimeLimit -ne 'PT2M' -or $t.Definition.Actions.Count -ne 1 -or !(Test-TaskQLauncher $a.Path) -or $a.Arguments -ne $args -or !(Test-CoreQPath $a.WorkingDirectory $cwd)){{throw 'existing controller definition differs'}}}}
else{{$d=$s.NewTask(0);$d.Principal.UserId=$identity.Name;$d.Principal.LogonType=3;$d.Principal.RunLevel=0;$d.Settings.ExecutionTimeLimit='PT2M';$d.Settings.MultipleInstances=2;$d.Settings.DisallowStartIfOnBatteries=$false;$d.Settings.StopIfGoingOnBatteries=$false;$a=$d.Actions.Create(0);$a.Path=$path;$a.Arguments=$args;$a.WorkingDirectory=$cwd;$sddl='D:P(A;;GA;;;'+$identity.User.Value+')(A;;GA;;;SY)';$t=$r.RegisterTaskDefinition($name,$d,18,$null,$null,3,$sddl)}}
if($t.State -ne 4){{$null=$t.Run($null)}}"#,name=quote(&format!("TaskQ-Stop-{id}")),exe=quote(&exe.to_string_lossy()),args=quote(&args),cwd=quote(&install.to_string_lossy()));
        let code=format!("{}\n{}\n{code}",coreq::service::windows_path_identity(),launcher_identity(&crate::platform::tool(install)));
        output("host.stop.dispatch",&mut powershell(&code))?;
    }
    #[cfg(not(any(target_os="macos",target_os="linux",windows)))] return Err("native Stop controller unsupported".into());
    #[cfg(not(target_os="macos"))] Ok(Control {id:id.into(),worker:None})
}

/// Never signal a replacement or use daemon exit as evidence about workers.
pub fn stop(install:&Path,id:&str,expected:&Identity,unit_revision:&str)->Res<()> {stop_unit(&crate::platform::tool(install),id,expected,unit_revision)}

pub fn remove(install:&Path,id:&str,expected:&Identity,unit_revision:&str)->Res<()> {
    remove_unit(&crate::platform::tool(install),id,expected,unit_revision)
}
fn remove_unit(tool:&coreq::Tool,id:&str,expected:&Identity,unit_revision:&str)->Res<()> {
    let install=&tool.install;let path=definition(tool)?;
    let present=match fs::symlink_metadata(&path) {Ok(_)=>true,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>false,Err(e)=>return Err(e.to_string())};
    if present&&revision_for(&tool)?!=unit_revision {return Err("Uninstall unit definition changed; kept".into());}
    if !native_absent(&tool)? {stop_unit(&tool,id,expected,unit_revision)?;}
    wait_exit(expected)?;
    if present {
        let unit=coreq::service::unit(&tool,install,"");
        #[cfg(target_os="linux")] commands("host.uninstall.disable",&unit.unload)?;
        #[cfg(windows)] if !native_absent(&tool)? {scheduler_write(tool.title)?;commands("host.uninstall.delete",std::slice::from_ref(unit.unload.last().ok_or("native delete command missing")?))?;}
        #[cfg(not(target_os="linux"))] if !native_absent(&tool)? {return Err("native unit remains; definition kept".into());}
        #[cfg(target_os="macos")] let _=unit;
        if revision_for(&tool)?!=unit_revision {return Err("Uninstall unit changed before removal; kept".into());}
        crate::host::ownership(&path)?;fs::remove_file(&path).map_err(|e|e.to_string())?;
    }
    #[cfg(target_os="linux")] output("host.uninstall.reload",crate::platform::command("systemctl").args(["--user","daemon-reload"]))?;
    if path.try_exists().map_err(|e|e.to_string())?||!native_absent(&tool)? {return Err("host unit removal not observed; retry the same Uninstall".into());}
    Ok(())
}

pub(crate) fn native_absent(tool:&coreq::Tool)->Res<bool> {
    #[cfg(target_os="macos")] {
        let unit=coreq::service::unit(tool,&tool.install,"");
        let target=unit.stop.first().and_then(|command|command.last()).ok_or("native unit target missing")?;
        let (domain,label)=target.rsplit_once('/').ok_or("launchd domain missing")?;
        let query=output("host.uninstall.readback",crate::platform::command("launchctl").args(["print",domain]))?;
        return Ok(!String::from_utf8(query.stdout).map_err(|e|e.to_string())?.lines().any(|line|line.split_whitespace().any(|field|field.trim_matches('"')==label)));
    }
    #[cfg(windows)] {
        let code=format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\')
try{{$null=$r.GetTask({name});'present'}}catch{{if(($_.Exception.GetBaseException()).HResult -ne -2147024894){{throw}};'absent'}}
exit 0"#,name=quote(tool.title));
        let query=output("host.uninstall.readback",&mut powershell(&code))?;
        return match String::from_utf8(query.stdout).map_err(|e|e.to_string())?.trim() {"absent"=>Ok(true),"present"=>Ok(false),_=>Err("native unit inventory unknown".into())};
    }
    #[cfg(target_os="linux")] {
        let query=crate::platform::output("host.uninstall.readback",crate::platform::command("systemctl").args(["--user","show",&format!("{}.service",tool.name),"--property=LoadState","--property=MainPID","--property=ActiveState","--no-pager"]),Duration::from_secs(30))?;
        let text=String::from_utf8(query.stdout).map_err(|e|e.to_string())?;
        return systemd_absence(&text,query.status.success()).map_err(|reason|format!("{reason}; {}",query.status));
    }
    #[cfg(not(any(target_os="macos",target_os="linux",windows)))] Err("native unit inventory unsupported".into())
}

#[cfg(any(target_os="linux",test))]
fn systemd_absence(text:&str,success:bool)->Res<bool> {
    let field=|key:&str|text.lines().find_map(|line|line.strip_prefix(key));
    let (load,pid,active)=(field("LoadState="),field("MainPID="),field("ActiveState="));
    // systemd retains failed history after the definition and daemon have gone.
    if success&&load==Some("not-found")&&pid==Some("0")&&matches!(active,Some("inactive"|"failed")) {return Ok(true);}
    if success&&load==Some("loaded") {return Ok(false);}
    Err(format!("systemd unit inventory unknown (LoadState={}, MainPID={}, ActiveState={})",load.unwrap_or("missing"),pid.unwrap_or("missing"),active.unwrap_or("missing")))
}
fn stop_unit(tool:&coreq::Tool,id:&str,expected:&Identity,unit_revision:&str)->Res<()> {
    #[cfg(not(windows))] let _=id;
    let install=&tool.install;
    if revision_for(tool)?!=unit_revision {return Err("prepared host unit changed; Stop outcome unknown".into());}
    if coreq::process::identity(expected.pid).map_err(|e|e.to_string())?.is_some_and(|native|&native!=expected) {return Err("native host incarnation changed; Stop refused".into());}
    #[cfg(not(windows))] let unit=coreq::service::unit(tool,install,"");
    #[cfg(not(windows))] if unit.path!=definition(tool)? {return Err("native unit scope differs; Stop refused".into());}
    #[cfg(target_os="macos")] {
        let target=unit.stop.first().and_then(|c|c.last()).ok_or("native Stop target missing")?;
        let live=coreq::process::identity(expected.pid).map_err(|e|e.to_string())?;
        if live.is_some() {
            if live.as_ref()!=Some(expected) {return Err("native host incarnation changed; Stop refused".into());}
            let query=output("host.stop.owner",crate::platform::command("launchctl").args(["print",target]))?;
            let text=String::from_utf8(query.stdout).map_err(|e|e.to_string())?;
            if !text.lines().any(|line|line.trim()==format!("pid = {}",expected.pid)) {return Err("launchd owner differs; Stop refused".into());}
            commands("host.stop.effect",&unit.stop)?;
        } else {
            // A lost response can be reconciled only by a successful native-domain inventory.
            let (domain,label)=target.rsplit_once('/').ok_or("launchd domain missing")?;
            let query=output("host.stop.readback",crate::platform::command("launchctl").args(["print",domain]))?;
            if String::from_utf8(query.stdout).map_err(|e|e.to_string())?.lines().any(|l|l.split_whitespace().any(|f|f.trim_matches('"')==label)) {return Err("host unit remains loaded with unknown owner; Stop outcome unknown".into());}
        }
    }
    #[cfg(target_os="linux")] {
        let name=unit.stop.first().and_then(|command|command.last()).ok_or("systemd unit target missing")?.as_str();
        let query=output("host.stop.owner",crate::platform::command("systemctl").args(["--user","show",name,"--property=MainPID","--property=LoadState","--property=ActiveState","--no-pager"]))?;
        let text=String::from_utf8(query.stdout).map_err(|e|e.to_string())?;
        let pid=text.lines().find_map(|l|l.strip_prefix("MainPID=")).and_then(|p|p.parse::<u32>().ok()).ok_or("systemd owner unknown")?;
        if !text.lines().any(|l|l=="LoadState=loaded")||pid!=0&&pid!=expected.pid {return Err("systemd owner differs; Stop refused".into());}
        if pid==0&&coreq::process::identity(expected.pid).map_err(|e|e.to_string())?.as_ref()==Some(expected) {return Err("live daemon has no attributable systemd owner; Stop refused".into());}
        if pid!=0&&coreq::process::identity(pid).map_err(|e|e.to_string())?.as_ref()!=Some(expected) {return Err("systemd native incarnation differs; Stop refused".into());}
        commands("host.stop.effect",&unit.stop)?;
        let query=output("host.stop.readback",crate::platform::command("systemctl").args(["--user","show",name,"--property=MainPID","--property=ActiveState","--no-pager"]))?;
        let text=String::from_utf8(query.stdout).map_err(|e|e.to_string())?;
        if !text.lines().any(|l|l=="MainPID=0")||!text.lines().any(|l|l=="ActiveState=inactive") {return Err("systemd Stop not observed".into());}
    }
    #[cfg(windows)] {
        scheduler_write(tool.title)?;
        let setup=format!(r#"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\');$t=$r.GetTask({name});$a=$t.Definition.Actions.Item(1);[xml]$saved=Get-Content -Raw -LiteralPath {definition};$exec=$saved.Task.Actions.Exec
if($t.Definition.Actions.Count -ne 1 -or @($exec).Count -ne 1 -or !(Test-CoreQPath $a.Path ([string]$exec.Command)) -or $a.Arguments -ne [string]$exec.Arguments -or !(Test-CoreQPath $a.WorkingDirectory ([string]$exec.WorkingDirectory)) -or !(Test-CoreQPath $a.WorkingDirectory {cwd})){{throw 'scheduled host scope differs'}}
"#,name=quote(tool.title),definition=quote(&definition(tool)?.to_string_lossy()),cwd=quote(&install.to_string_lossy()));
        let setup=format!("{}\n{setup}",coreq::service::windows_path_identity());
        // #1625: disable the repeat before the native stop so it cannot restart the host; logon and install still start it.
        repeat_trigger(tool,false)?;
        let stopped=(||->Res<()> {
            let query=format!(r#"{setup}
$instances=@(Get-TaskQInstances $s $t);if($instances.Count -gt 1){{throw 'multiple native host instances'}}
if($instances.Count -eq 0){{'null';exit 0}}
$instance=$instances[0];$engine=$instance.EnginePID;$daemon=Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}' -ErrorAction Stop
$created=(Get-Process -Id $engine -ErrorAction Stop).StartTime.ToFileTimeUtc()
if(!$daemon -or $daemon.ParentProcessId -ne $engine -or (Get-Process -Id {pid}).StartTime.ToFileTimeUtc().ToString() -ne {started} -or $created -gt [long]{started}){{throw 'scheduled host owner differs'}}
[pscustomobject]@{{pid=$engine;started=$created.ToString();instance=$instance.InstanceGuid}}|ConvertTo-Json -Compress"#,pid=expected.pid,started=quote(&expected.started));
            let observed=output("host.stop.owner",&mut powershell(&query))?;
            let observed:serde_json::Value=serde_json::from_slice(&observed.stdout).map_err(|e|format!("native launcher observation invalid: {e}"))?;
            let mut scope=lifecycle::saved_launcher(install,id,expected)?;
            if !observed.is_null() {lifecycle::save_launcher(install,id,expected,&observed)?;scope=Some(observed);}
            if let Some(scope)=scope {
                let launcher=Identity {pid:scope["pid"].as_u64().ok_or("launcher pid unknown")? as u32,started:scope["started"].as_str().ok_or("launcher creation unknown")?.into()};
                let stop=format!(r#"{setup}
$instances=@(Get-TaskQInstances $s $t);if($instances.Count -gt 1){{throw 'multiple native host instances'}}
if($instances.Count -eq 1){{$instance=$instances[0];if($instance.InstanceGuid -ne {instance} -or $instance.EnginePID -ne {pid} -or (Get-Process -Id {pid} -ErrorAction Stop).StartTime.ToFileTimeUtc().ToString() -ne {started}){{throw 'native launcher incarnation differs'}};$instance.Stop()}}
$until=[DateTime]::UtcNow.AddSeconds(10);while(@(Get-TaskQInstances $s $t).Count -ne 0 -and [DateTime]::UtcNow -lt $until){{Start-Sleep -Milliseconds 20}}
if(@(Get-TaskQInstances $s $t).Count -ne 0){{throw 'scheduled host stop not observed'}}"#,instance=quote(scope["instance"].as_str().ok_or("launcher instance unknown")?),pid=launcher.pid,started=quote(&launcher.started));
                output("host.stop.native",&mut powershell(&stop))?;
                terminate_owned(&launcher)?;
                wait_exit(&launcher)?;
                terminate_owned(expected)?;
            } else if coreq::process::identity(expected.pid).map_err(|e|e.to_string())?.as_ref()==Some(expected) {
                return Err("live daemon has no attributable scheduler launcher; Stop refused".into());
            }
            Ok(())
        })();
        // A refused or failed Stop leaves the host running: restore its restart source.
        if stopped.is_err()&&coreq::process::identity(expected.pid).ok().flatten().as_ref()==Some(expected) {let _=repeat_trigger(tool,true);}
        stopped?;
    }
    wait_exit(expected)?;
    if revision_for(tool)?!=unit_revision {return Err("host unit definition changed during Stop; outcome unknown".into());}
    Ok(())
}

#[cfg(windows)]
fn terminate_owned(expected:&Identity)->Res<()> {
    if coreq::process::identity(expected.pid).map_err(|e|e.to_string())?.as_ref()!=Some(expected) {return Ok(());}
    match coreq::process::terminate(expected) {
        Ok(())=>Ok(()),Err(error)=>if coreq::process::identity(expected.pid).map_err(|e|e.to_string())?.as_ref()!=Some(expected) {Ok(())}else{Err(error.to_string())},
    }
}
fn wait_exit(expected:&Identity)->Res<()> {
    let until=Instant::now()+Duration::from_secs(10);
    loop {
        if coreq::process::identity(expected.pid).map_err(|e|e.to_string())?.as_ref()!=Some(expected) {break;}
        if Instant::now()>=until {return Err("native host exit unconfirmed; inspect service status".into());}
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

pub fn retire_controller(install:&Path,id:&str)->Res<()> {
    #[cfg(windows)] {
        let pending=lifecycle::pending(install)?.filter(|p|p.id()==id&&p.stops()).ok_or("Stop controller binding unavailable")?;
        return retire_command(install,id,"complete-stop",&pending.context()?);
    }
    #[cfg(not(windows))] let _=(install,id);
    Ok(())
}

#[cfg(windows)]
fn retire_command(install:&Path,id:&str,action:&str,context:&str)->Res<()> {
    if !crate::resources::id(id)||action!="complete-stop" {return Err("native controller retirement scope refused".into());}
    coreq::telemetry::capture::TraceContext::decode(context)?;
    {
        let (_,args)=controller_action(install,id,action)?;
        let code=format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\');$name={name}
try{{$t=$r.GetTask($name)}}catch{{if(($_.Exception.GetBaseException()).HResult -ne -2147024894){{throw}};$t=$null}}
if($t){{$a=$t.Definition.Actions.Item(1);if($t.Definition.Actions.Count -ne 1 -or !(Test-TaskQLauncher $a.Path) -or $a.Arguments -ne {args} -or !(Test-CoreQPath $a.WorkingDirectory {cwd}) -or $t.Definition.Principal.RunLevel -ne 0 -or $t.Definition.Triggers.Count -ne 0){{throw 'controller ownership differs; definition kept'}};$r.DeleteTask($name,0)}}
try{{$null=$r.GetTask($name);throw 'controller definition remains'}}catch{{if(($_.Exception.GetBaseException()).HResult -ne -2147024894){{throw}}}}
exit 0"#,name=quote(&format!("TaskQ-Stop-{id}")),args=quote(&args),cwd=quote(&install.to_string_lossy()));
        let code=format!("{}\n{}\n{code}",coreq::service::windows_path_identity(),launcher_identity(&crate::platform::tool(install)));
        output("host.stop.controller.retire",&mut powershell(&code))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "host_control_tests.rs"]
mod tests;
