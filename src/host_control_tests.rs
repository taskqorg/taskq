    use super::*;
    use serde_json::{Value,json};

    // Defect #2049 A3: nothing kept a test from registering, editing or running the real TaskQ unit or a
    // TaskQ-Stop controller on the host that ran it. Owned `…-Probe-<nonce>` fixtures stay allowed.
    #[test]
    fn tests_never_change_real_scheduled_tasks() {
        assert!(scheduler_write("TaskQ").unwrap_err().contains("real Scheduled Task TaskQ"));
        assert!(scheduler_write("TaskQ-Repeat-Probe-0123456789abcdef0123456789abcdef").is_ok());
        if std::env::var_os("TASKQ_NATIVE_STOP_CASE").is_none() {assert!(scheduler_write("TaskQ-Stop-0123456789abcdef0123456789abcdef").is_err());}
        #[cfg(windows)] {
            let install=std::env::temp_dir().join(format!("taskq-real-task-{}",crate::runtime::uuid()));
            fs::create_dir_all(&install).unwrap();
            let tool=crate::platform::tool(&install);fs::write(install.join(tool.exe_name()),b"").unwrap();
            let refused=start_install(&tool,InstallState::Absent).unwrap_err();
            assert!(!refused.submitted&&refused.reason.contains("real Scheduled Task"),"{}",refused.reason);
            assert!(repeat_trigger(&tool,true).unwrap_err().contains("real Scheduled Task"));
            fs::remove_dir_all(install).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    // Defect #2049 A1: the Stop controller task started `powershell.exe`, a console binary, so each Stop flashed a
    // window. Kept: replay through an install alias yields the same action, and another request a different one.
    fn controller_starts_gui_launcher_and_replays_across_install_aliases() {
        let install=std::env::temp_dir().join(format!("taskq-controller-alias-{}",crate::runtime::uuid()));
        fs::create_dir_all(&install).unwrap();
        let tool=crate::platform::tool(&install);let canonical=install.canonicalize().unwrap();
        assert!(controller_action(&install,"same-request","complete-stop").unwrap_err().contains("launcher missing"));
        fs::write(install.join(tool.exe_name()),b"").unwrap();
        assert_eq!(controller_action(&install,"same-request","complete-stop").unwrap().0,canonical.join(tool.exe_name()),"an older install without the GUI launcher keeps the console one");
        fs::write(install.join(tool.gui_exe_name()),b"").unwrap();
        let action=controller_action(&install,"same-request","complete-stop").unwrap();
        assert_eq!(action,(canonical.join(tool.gui_exe_name()),"service complete-stop --request same-request".to_string()));
        assert_eq!(action,controller_action(&canonical,"same-request","complete-stop").unwrap());
        assert_ne!(action.1,controller_action(&install,"other-request","complete-stop").unwrap().1);
        assert!(controller_action(&install,"bad request","complete-stop").is_err()&&controller_action(&install,"same-request","stop").is_err());
        assert!(controller_action(&install.join("missing"),"same-request","complete-stop").is_err());
        fs::remove_dir_all(install).unwrap();
    }

    #[test]
    fn removed_systemd_failed_history_is_absent_but_incomplete_inventory_is_unknown() {
        for active in ["inactive","failed"] {
            assert!(systemd_absence(&format!("MainPID=0\nLoadState=not-found\nActiveState={active}\n"),true).unwrap());
        }
        assert!(!systemd_absence("MainPID=0\nLoadState=loaded\nActiveState=inactive\n",true).unwrap());
        for (text,success) in [("LoadState=not-found\nActiveState=failed\n",true),("MainPID=7\nLoadState=not-found\nActiveState=failed\n",true),
            ("MainPID=0\nLoadState=not-found\nActiveState=active\n",true),("MainPID=0\nLoadState=not-found\nActiveState=failed\n",false),
            ("MainPID=0\nLoadState=loaded\nActiveState=inactive\n",false),("",false)] {
            assert!(systemd_absence(text,success).unwrap_err().contains("systemd unit inventory unknown"));
        }
    }

    // H-A5: run the real adapter only against a unique, externally owned temporary unit.
    #[test]
    #[ignore="requires TASKQ_NATIVE_STOP_CASE and an owned temporary native unit; no production unit"]
    fn native_unit_stop() {
        let install=PathBuf::from(std::env::var_os("TASKQ_NATIVE_STOP_CASE").expect("owned fixture root required"));
        let nonce=install.file_name().unwrap().to_str().unwrap().strip_prefix("taskq-stop-native-").unwrap();
        let id=nonce;
        assert_eq!(nonce.len(),32);assert!(nonce.bytes().all(|b|b.is_ascii_hexdigit()));
        let name=Box::leak(format!("taskq-stop-probe-{nonce}").into_boxed_str());
        let title=Box::leak(format!("TaskQ-Stop-Probe-{nonce}").into_boxed_str());
        let tool=coreq::Tool {name,title,repo:"fixture/unused",version:crate::platform::VERSION,install:install.clone(),fault:None};
        #[cfg(any(target_os="linux",windows))]
        if std::env::var_os("TASKQ_NATIVE_INSTALL_PREPARE").is_some() {
            assert_eq!(install_state(&tool,None).unwrap(),InstallState::Absent);
            start_install(&tool,InstallState::Absent).unwrap_or_else(|e|panic!("initial native fixture start: {}",e.reason));
            wait_fixture_host(&tool,None);
        }
        let fixture:Value=serde_json::from_slice(&fs::read(install.join("native.json")).unwrap()).unwrap();
        let native=coreq::process::identity(fixture["pid"].as_u64().unwrap().try_into().unwrap()).unwrap().unwrap();
        if let Some(started)=fixture["started"].as_str() {assert_eq!(native.started,started);}
        let revision=revision_for(&tool).unwrap();let definition=definition(&tool).unwrap();let original=fs::read(&definition).unwrap();
        if std::env::var_os("TASKQ_NATIVE_INSTALL_VERIFY").is_some() {
            assert_eq!(install_state(&tool,Some(&native)).unwrap(),InstallState::Loaded);
            assert!(install_state(&tool,None).is_err(),"an unattributed native daemon is never idle");
            let mut wrong=native.clone();wrong.started.push_str("-wrong");
            assert!(install_state(&tool,Some(&wrong)).is_err());
            assert_eq!(fs::read(&definition).unwrap(),original,"verification cannot edit the unit");
            fs::write(&definition,b"foreign invalid definition").unwrap();
            let refused=install_state(&tool,Some(&native));
            let kept=fs::read(&definition).unwrap();fs::write(&definition,&original).unwrap();
            assert!(refused.is_err());assert_eq!(kept,b"foreign invalid definition");
            #[cfg(windows)] {
                let canonical=coreq::Tool {install:install.canonicalize().unwrap(),..tool.clone()};
                assert_eq!(install_state(&canonical,Some(&native)).unwrap(),InstallState::Loaded,"native/saved ordinary paths accept the canonical caller");
                let other=install.join("other.exe");fs::copy(install.join(tool.exe_name()),&other).unwrap();
                for (field,path,accepted) in [("Command",install.join(tool.exe_name()).canonicalize().unwrap(),true),("WorkingDirectory",install.canonicalize().unwrap(),true),
                    ("Command",other.clone(),false),("Command",install.join("missing.exe"),false),("WorkingDirectory",install.parent().unwrap().to_path_buf(),false),("WorkingDirectory",install.join("missing"),false)] {
                    let code=format!("$ErrorActionPreference='Stop';[xml]$xml=Get-Content -Raw -LiteralPath {definition};$xml.Task.Actions.Exec.{field}={path};$xml.Save({definition})",definition=quote(&definition.to_string_lossy()),path=quote(&path.to_string_lossy()));
                    output("host.install.fixture.path",&mut powershell(&code)).unwrap();
                    let changed=fs::read(&definition).unwrap();let checked=install_state(&tool,Some(&native));
                    let kept=fs::read(&definition).unwrap();fs::write(&definition,&original).unwrap();
                    assert_eq!(checked.is_ok(),accepted,"{field} {}: {checked:?}",path.display());assert_eq!(kept,changed);
                    assert_eq!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native));
                }
                fs::remove_file(other).unwrap();
                let sid=output("host.install.fixture.principal",&mut powershell("[Security.Principal.WindowsIdentity]::GetCurrent().User.Value")).unwrap();
                let sid=String::from_utf8(sid.stdout).unwrap();
                for (principal,accepted) in [(sid.trim(),true),("S-1-5-18",false),("S-1-invalid",false)] {
                    let code=format!("$ErrorActionPreference='Stop';[xml]$xml=Get-Content -Raw -LiteralPath {path};$xml.Task.Principals.Principal.UserId={principal};$xml.Save({path})",path=quote(&definition.to_string_lossy()),principal=quote(principal));
                    output("host.install.fixture.definition",&mut powershell(&code)).unwrap();
                    let changed=fs::read(&definition).unwrap();let checked=install_state(&tool,Some(&native));
                    let kept=fs::read(&definition).unwrap();fs::write(&definition,&original).unwrap();
                    assert_eq!(checked.is_ok(),accepted,"principal {principal}: {checked:?}");assert_eq!(kept,changed);
                    assert_eq!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native));
                }
                for change in ["$xml.Task.Triggers.LogonTrigger.UserId='S-1-5-18'","$xml.Task.Triggers.LogonTrigger.UserId='S-1-invalid'","$null=$xml.Task.Triggers.AppendChild($xml.Task.Triggers.LogonTrigger.CloneNode($true))"] {
                    let code=format!("$ErrorActionPreference='Stop';[xml]$xml=Get-Content -Raw -LiteralPath {path};{change};$xml.Save({path})",path=quote(&definition.to_string_lossy()));
                    output("host.install.fixture.trigger",&mut powershell(&code)).unwrap();
                    let changed=fs::read(&definition).unwrap();let checked=install_state(&tool,Some(&native));
                    let kept=fs::read(&definition).unwrap();fs::write(&definition,&original).unwrap();
                    assert!(checked.is_err(),"foreign/invalid/extra saved trigger accepted");assert_eq!(kept,changed);
                    assert_eq!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native));
                }
                let code=format!("$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\\');$t=$r.GetTask({name});$d=$t.Definition;$extra=$d.Triggers.Create(9);$extra.Enabled=$true;$extra.UserId=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;$null=$r.RegisterTaskDefinition({name},$d,4,$null,$null,3,$null)",name=quote(tool.title));
                output("host.install.fixture.native-trigger",&mut powershell(&code)).unwrap();
                let checked=install_state(&tool,Some(&native));
                let restore=format!("$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$null=$s.GetFolder('\\').RegisterTask({name},[IO.File]::ReadAllText({path}),4,$null,$null,3,$null)",name=quote(tool.title),path=quote(&definition.to_string_lossy()));
                output("host.install.fixture.native-restore",&mut powershell(&restore)).unwrap();
                assert!(checked.is_err(),"extra native trigger accepted");assert_eq!(fs::read(&definition).unwrap(),original);
                assert_eq!(install_state(&tool,Some(&native)).unwrap(),InstallState::Loaded);
                // #1858: an older installer's live task without TaskQ.xml exports one instead of refusing with no step
                fs::remove_file(&definition).unwrap();
                let exported=install_state(&tool,Some(&native));let written=definition.is_file();fs::write(&definition,&original).unwrap();
                assert_eq!(exported.unwrap(),InstallState::Loaded,"live task export accepted");assert!(written,"export kept as the saved definition");
                assert!(fs::read_dir(&install).unwrap().all(|e|!e.unwrap().file_name().to_string_lossy().contains(".export-")),"export temp removed");
            }
        }
        #[cfg(windows)] let collateral=if std::env::var_os("TASKQ_NATIVE_INSTALL_START_VERIFY").is_some() {
            let bytes=fs::read(install.join(tool.exe_name())).unwrap();
            let prior=tool.release_exe("4.4.0");fs::create_dir_all(prior.parent().unwrap()).unwrap();fs::write(prior,&bytes).unwrap();
            coreq::write_durable(&install.join("current"),"4.4.0").unwrap();
            coreq::update::prepare_install(&tool,&bytes).unwrap();coreq::update::activate_install(&tool).unwrap();
            let old=install.join(format!("{}.old",tool.exe_name()));
            assert_eq!(fs::read(&old).unwrap(),bytes);assert_eq!(fs::read(install.join(tool.exe_name())).unwrap(),bytes);
            assert_eq!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native),"launcher activation must preserve its existing daemon");
            assert!(fs::remove_file(&old).is_err(),"live launcher collateral must remain locked");
            json!({"old":old,"locked_while_running":true,"daemon_preserved":true})
        }else{Value::Null};
        #[cfg(not(windows))] let collateral=Value::Null;
        let (capture,guard)=crate::profile::tests::collecting(&install);
        fixture_stop_binding(&install,id,&native,&revision);
        #[cfg(windows)] {
            dispatch(&install,&lifecycle::pending(&install).unwrap().unwrap()).unwrap().wait().unwrap();
            let check=format!(r#"$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$t=$s.GetFolder('\').GetTask({name});$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$acl=[Security.AccessControl.RawSecurityDescriptor]::new($t.GetSecurityDescriptor(4));$allowed=@($acl.DiscretionaryAcl|Where-Object {{$_.AceQualifier -eq 'AccessAllowed' -and $_.SecurityIdentifier.Value -eq $sid -and (($_.AccessMask -band 0x10000000) -ne 0 -or ($_.AccessMask -band 0x10000) -ne 0)}})
if(($acl.ControlFlags -band [Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -eq 0 -or @($acl.DiscretionaryAcl).Count -ne 2){{throw 'controller task ACL differs from the requested protected scope'}}
if($allowed.Count -eq 0){{throw 'limited controller cannot retire its own task'}}"#,name=quote(&format!("TaskQ-Stop-{id}")));
            let checked=output("host.stop.controller.permissions",&mut powershell(&check));
            let cleaned=retire_controller(&install,id);cleaned.unwrap();checked.unwrap();
        }
        assert!(stop_unit(&tool,id,&native,&"0".repeat(64)).is_err());
        let mut foreign=native.clone();foreign.started.push_str("-foreign");
        assert!(stop_unit(&tool,id,&foreign,&revision).is_err());
        assert_eq!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native),"refused requests must preserve the original daemon");
        stop_unit(&tool,id,&native,&revision).unwrap();
        assert_ne!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native));
        stop_unit(&tool,id,&native,&revision).unwrap();
        assert_eq!(fs::read(&definition).unwrap(),original,"Stop retains the native definition");
        #[cfg(any(target_os="macos",target_os="linux",windows))]
        let starts=if std::env::var_os("TASKQ_NATIVE_INSTALL_START_VERIFY").is_some() {install_start_matrix(&tool,id,&native,&original)}else{json!([])};
        #[cfg(not(any(target_os="macos",target_os="linux",windows)))] let starts=json!([]);
        let uninstall=std::env::var("TASKQ_NATIVE_UNINSTALL").as_deref()==Ok("1");
        if uninstall {
            assert!(remove_unit(&tool,id,&native,&"0".repeat(64)).is_err());
            assert_eq!(fs::read(&definition).unwrap(),original,"refused Uninstall keeps the definition");
            remove_unit(&tool,id,&native,&revision).unwrap();
            assert!(!definition.try_exists().unwrap());assert!(native_absent(&tool).unwrap());
            remove_unit(&tool,id,&native,&revision).unwrap();
        }
        capture.flush().unwrap();drop(guard);drop(capture);
        fs::write(install.join("adapter-result.json"),json!({"unit":name,"native_pid":native.pid,"native_started":native.started,"unit_revision":revision,"exit":true,"replay":true,"definition_retained_after_stop":true,"definition_retained":!uninstall,"unit_removed":uninstall,"install_starts":starts,"running_launcher_collateral":collateral}).to_string()).unwrap();
    }

    // #1625: a CoreQ v0.14.7 unit repeats every 5 minutes, an older one has no repeat; any other trigger set stays refused.
    #[cfg(windows)]
    #[test]
    fn saved_unit_accepts_only_logon_and_one_repeat_trigger() {
        let install=std::env::temp_dir().join(format!("taskq-repeat-saved-{}",crate::runtime::uuid()));
        let tool=coreq::Tool {install:install.clone(),..coreq::Tool::new("taskq-repeat-saved","TaskQ-Repeat-Saved","fixture/unused","1.2.3")};
        fs::create_dir_all(&install).unwrap();fs::write(install.join(tool.exe_name()),b"").unwrap();
        let unit=coreq::service::unit(&tool,&install,"");
        let repeat="<TimeTrigger><Enabled>true</Enabled><StartBoundary>2026-01-01T00:00:00</StartBoundary><Repetition><Interval>PT5M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition></TimeTrigger>";
        assert!(unit.text.contains(repeat),"pinned CoreQ unit lacks the 5-minute repeat");
        for (text,accepted) in [(unit.text.clone(),true),(unit.text.replace(repeat,""),true),(unit.text.replace(repeat,&repeat.replace("<Enabled>true","<Enabled>false")),true),
            (unit.text.replace(repeat,&repeat.repeat(2)),false),(unit.text.replace("PT5M","PT10M"),false),(unit.text.replace("<LogonTrigger><Enabled>true","<LogonTrigger><Enabled>false"),false)] {
            let bytes:Vec<u8>=std::iter::once(0xFEFFu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect();
            fs::write(definition(&tool).unwrap(),bytes).unwrap();
            let checked=verify_saved_install_scope(&tool);
            assert_eq!(checked.is_ok(),accepted,"{checked:?}");
            if let Err(e)=checked {assert!(e.contains("trigger differs"),"{e}");}
        }
        fs::remove_dir_all(install).unwrap();
    }

    // Defect #2049 A1: CoreQ units now start the GUI launcher `<name>w.exe`, which the saved-unit check refused, so a
    // saved host could not restart. Kept: only this install's console or GUI launcher passes; another exe is refused.
    #[cfg(windows)]
    #[test]
    fn saved_unit_accepts_console_or_gui_launcher_only() {
        let install=std::env::temp_dir().join(format!("taskq-gui-saved-{}",crate::runtime::uuid()));
        let tool=coreq::Tool {install:install.clone(),..coreq::Tool::new("taskq-gui-saved","TaskQ-Gui-Saved","fixture/unused","1.2.3")};
        fs::create_dir_all(&install).unwrap();
        for name in [tool.exe_name(),tool.gui_exe_name(),"other.exe".into()] {fs::write(install.join(name),b"").unwrap();}
        let unit=coreq::service::unit(&tool,&install,"");
        let command=|name:&str|format!("<Command>{}</Command>",install.join(name).display());
        let named=[tool.exe_name(),tool.gui_exe_name()].into_iter().find(|name|unit.text.contains(&command(name))).expect("unit names a launcher");
        for (name,accepted) in [(tool.exe_name(),true),(tool.gui_exe_name(),true),("other.exe".into(),false)] {
            let text=unit.text.replace(&command(&named),&command(&name));
            let bytes:Vec<u8>=std::iter::once(0xFEFFu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect();
            fs::write(definition(&tool).unwrap(),bytes).unwrap();
            let checked=verify_saved_install_scope(&tool);
            assert_eq!(checked.is_ok(),accepted,"{name}: {checked:?}");
        }
        fs::remove_file(install.join(tool.gui_exe_name())).unwrap();
        let text=unit.text.replace(&command(&named),&command(&tool.gui_exe_name()));
        let bytes:Vec<u8>=std::iter::once(0xFEFFu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect();
        fs::write(definition(&tool).unwrap(),bytes).unwrap();
        assert!(verify_saved_install_scope(&tool).is_err(),"a missing GUI launcher never matches");
        fs::remove_dir_all(install).unwrap();
    }

    // Risk (#1858): Task Scheduler exports omit default fields, so a definition saved from the live task was refused.
    #[cfg(windows)]
    #[test]
    fn saved_unit_reads_omitted_run_level_and_logon_enabled_as_defaults() {
        let install=std::env::temp_dir().join(format!("taskq-export-saved-{}",crate::runtime::uuid()));
        let tool=coreq::Tool {install:install.clone(),..coreq::Tool::new("taskq-export-saved","TaskQ-Export-Saved","fixture/unused","1.2.3")};
        fs::create_dir_all(&install).unwrap();fs::write(install.join(tool.exe_name()),b"").unwrap();
        let unit=coreq::service::unit(&tool,&install,"");
        let (level,enabled)=("<RunLevel>LeastPrivilege</RunLevel>","<LogonTrigger><Enabled>true</Enabled>");
        assert!(unit.text.contains(level)&&unit.text.contains(enabled),"pinned CoreQ unit changed its default fields");
        for (text,accepted) in [(unit.text.replace(level,""),true),(unit.text.replace(enabled,"<LogonTrigger>"),true),(unit.text.replace(level,"").replace(enabled,"<LogonTrigger>"),true),
            (unit.text.replace(level,"<RunLevel>HighestAvailable</RunLevel>"),false),(unit.text.replace(level,&level.repeat(2)),false),
            (unit.text.replace(enabled,"<LogonTrigger><Enabled>false</Enabled>"),false),(unit.text.replace(enabled,"<LogonTrigger><Enabled>true</Enabled><Enabled>true</Enabled>"),false)] {
            let bytes:Vec<u8>=std::iter::once(0xFEFFu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect();
            fs::write(definition(&tool).unwrap(),bytes).unwrap();
            let checked=verify_saved_install_scope(&tool);
            assert_eq!(checked.is_ok(),accepted,"{checked:?}\n{text}");
        }
        fs::remove_dir_all(install).unwrap();
    }

    // #1625 A3: an owned temporary unit, never the product unit. Its launcher/daemon probe mirrors `service run`.
    #[cfg(windows)]
    #[test]
    #[ignore="registers an owned temporary scheduled task and waits up to 12 minutes; Windows host only"]
    fn native_repeat_restarts_exited_host() {
        let nonce=crate::runtime::uuid().replace('-',"");
        let install=std::env::temp_dir().join(format!("taskq-repeat-native-{nonce}"));
        let name=Box::leak(format!("taskq-repeat-probe-{nonce}").into_boxed_str());
        let title=Box::leak(format!("TaskQ-Repeat-Probe-{nonce}").into_boxed_str());
        let tool=coreq::Tool {name,title,repo:"fixture/unused",version:crate::platform::VERSION,install:install.clone(),fault:None};
        let exe=install.join(tool.exe_name());
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("probe.rs"),r#"fn main(){
    if std::env::args().nth(1).as_deref()==Some("daemon") {
        std::fs::write("native.json",format!("{{\"pid\":{}}}",std::process::id())).unwrap();
        loop {std::thread::sleep(std::time::Duration::from_secs(60));}
    }
    let _=std::process::Command::new(std::env::current_exe().unwrap()).arg("daemon").status();
    std::process::exit(0);
}"#).unwrap();
        // A clean launcher exit, like the silent one in #1625: RestartOnFailure does not apply, only the repeat restarts it.
        assert!(Command::new("rustc").arg("-o").arg(&exe).arg(install.join("probe.rs")).status().unwrap().success());
        let trace=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||repeat_trace(&tool,&nonce)));
        let cleanup=format!("$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\\')
try{{$t=$r.GetTask({name});foreach($i in @(Get-TaskQInstances $s $t)){{$i.Stop()}};$r.DeleteTask({name},0)}}catch{{}}
Get-CimInstance Win32_Process|Where-Object {{$_.ExecutablePath -eq {exe}}}|ForEach-Object {{Stop-Process -Id $_.ProcessId -Force}}",name=quote(title),exe=quote(&exe.to_string_lossy()));
        output("host.unit.fixture.cleanup",&mut powershell(&cleanup)).unwrap();
        let trace=trace.unwrap_or_else(|e|std::panic::resume_unwind(e));
        println!("native repeat trace: {trace}");
        let _=fs::remove_dir_all(&install);
    }

    #[cfg(windows)]
    fn repeat_trace(tool:&coreq::Tool,nonce:&str)->Value {
        let native=|code:String|String::from_utf8(output("host.unit.fixture",&mut powershell(&format!("$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\\');$t=$r.GetTask({});{code}",quote(tool.title)))).unwrap().stdout).unwrap().trim().to_string();
        let triggers=||native("(@($t.Definition.Triggers|ForEach-Object {\"$($_.Type):$($_.Enabled)\"}) -join ',')".into());
        let exe=quote(&tool.install.join(tool.exe_name()).to_string_lossy());
        let processes=||native(format!("@(Get-CimInstance Win32_Process|Where-Object {{$_.ExecutablePath -eq {exe}}}).Count"));
        start_install(tool,InstallState::Absent).unwrap_or_else(|e|panic!("fixture start: {}",e.reason));
        let first=wait_fixture_host(tool,None);
        verify_saved_install_scope(tool).unwrap();
        assert_eq!(install_state(tool,Some(&first)).unwrap(),InstallState::Loaded);
        assert_eq!((triggers().as_str(),repeat_trigger(tool,true).unwrap()),("9:True,1:True","kept"),"CoreQ unit carries the enabled repeat");
        assert_eq!(repeat_trigger(tool,false).unwrap(),"changed");assert_eq!(triggers(),"9:True,1:False");
        assert_eq!(repeat_trigger(tool,false).unwrap(),"kept");
        assert_eq!(coreq::process::identity(first.pid).unwrap().as_ref(),Some(&first),"a trigger edit keeps the running host");
        assert_eq!(install_state(tool,Some(&first)).unwrap(),InstallState::Loaded,"a disabled repeat is still this host's unit");
        assert_eq!(repeat_trigger(tool,true).unwrap(),"changed");assert_eq!(triggers(),"9:True,1:True");
        native("$d=$t.Definition;for($i=$d.Triggers.Count;$i -ge 1;$i--){if($d.Triggers.Item($i).Type -eq 1){$d.Triggers.Remove($i)}};$null=$r.RegisterTaskDefinition($t.Name,$d,4,$null,$null,3,$null)".into());
        assert_eq!(triggers(),"9:True");assert_eq!(install_state(tool,Some(&first)).unwrap(),InstallState::Loaded,"a pre-v0.14.7 unit stays accepted");
        assert_eq!(repeat_trigger(tool,true).unwrap(),"changed","host start adds the repeat to an older unit");assert_eq!(triggers(),"9:True,1:True");
        native("$null=$t.Run($null)".into());std::thread::sleep(Duration::from_secs(3));
        assert_eq!(processes(),"2","a start while the host runs adds no second launcher or daemon");
        let killed=Instant::now();terminate_owned(&first).unwrap();wait_exit(&first).unwrap();
        let until=killed+Duration::from_secs(360);
        let second=loop {
            if let Some(next)=fs::read(tool.install.join("native.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).and_then(|v|v["pid"].as_u64())
                .and_then(|pid|coreq::process::identity(pid as u32).ok().flatten()).filter(|next|next!=&first) {break next;}
            assert!(Instant::now()<until,"exited host not started again within 6 minutes");
            std::thread::sleep(Duration::from_secs(1));
        };
        let back=killed.elapsed().as_secs();
        assert_eq!(processes(),"2","the restart runs one launcher and one daemon");
        let id=nonce.to_string();let revision=revision_for(tool).unwrap();
        fixture_stop_binding(&tool.install,&id,&second,&revision);
        stop_unit(tool,&id,&second,&revision).unwrap();
        assert_eq!(triggers(),"9:True,1:False","Stop disables only the repeat");
        std::thread::sleep(Duration::from_secs(330));
        assert_eq!(processes(),"0","a stopped host stays stopped across a repeat boundary");
        json!({"unit":tool.name,"first_pid":first.pid,"killed_by":"exact pid","second_pid":second.pid,"back_after_s":back,"processes_after_restart":2,"stop_triggers":"logon enabled, repeat disabled","after_stop_330s_processes":0})
    }

    fn fixture_stop_binding(install:&Path,id:&str,native:&Identity,revision:&str) {
        assert!(crate::resources::id(id),"fixture must use the production lifecycle ID format");
        let trace=crate::runtime::uuid().replace('-',"");
        let command=coreq::telemetry::capture::CommandReceipt::decode(&json!({"wire":1,"product":"taskq","version":"fixture","operation":"taskq.host.stop.request","trace":format!("{trace}.1"),"span":format!("{trace}.2"),"parent":null,"started_ms":1}).to_string()).unwrap();
        crate::resources::with(install,|state| {
            state.ops.insert(id.into(),crate::resources::Op {
                bound:json!({"local_stop":{"request":{"id":id,"instance":"original","roots":[],"issued_at":1,"expires_at":301,"action":"stop"},"command":command.encode()}}),
                receipt:json!({"id":id,"state":"applied","received_receipt":command.event_id("received")?,"applied_receipt":command.event_id("applied")?,"native_pid":native.pid,"native_started":native.started,"unit_revision":revision}),
                expires_at:301,saved_at:Some(1),writing:false,sent:true,
            });Ok(())
        }).unwrap();
        assert!(lifecycle::receipt(install,id).unwrap().is_some());
    }

    // H-A5: native start states share the existing owned-unit fixture, with no product registration.
    #[cfg(any(target_os="macos",target_os="linux",windows))]
    fn install_start_matrix(tool:&coreq::Tool,id:&str,previous:&Identity,original:&[u8])->Value {
        let definition=definition(tool).unwrap();
        let unit=coreq::service::unit(tool,&tool.install,"");
        let mut prior=previous.clone();let mut cases=vec![];
        #[cfg(target_os="macos")] let states=[InstallState::Saved,InstallState::Loaded,InstallState::Absent];
        #[cfg(target_os="linux")] let states=[InstallState::Loaded,InstallState::Absent];
        #[cfg(windows)] let states=[InstallState::Loaded,InstallState::Saved,InstallState::Absent];
        for state in states {
            if state==InstallState::Loaded {
                #[cfg(target_os="macos")] {
                for key in ["RunAtLoad","KeepAlive"] {
                    output("host.install.fixture.idle",Command::new("plutil").args(["-replace",key,"-bool","false"]).arg(&definition)).unwrap();
                }
                commands("host.install.fixture.load",&unit.load).unwrap();
                }
            } else if state==InstallState::Saved {
                #[cfg(windows)] {
                    assert_eq!(install_state(tool,None).unwrap(),InstallState::Loaded);
                    let code=format!("$ErrorActionPreference='Stop';$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$s.GetFolder('\\').DeleteTask({},0)",quote(tool.title));
                    output("host.install.fixture.unload",&mut powershell(&code)).unwrap();
                    assert!(native_absent(tool).unwrap());
                }
            } else if state==InstallState::Absent {
                #[cfg(any(target_os="linux",windows))] remove_unit(tool,&format!("{id}-{}",cases.len()-1),&prior,&revision_for(tool).unwrap()).unwrap();
                assert!(native_absent(tool).unwrap());
                #[cfg(target_os="macos")] fs::remove_file(&definition).unwrap();
            }
            assert_eq!(install_state(tool,None).unwrap(),state);
            let before=fs::read(&definition).ok();
            fs::remove_file(tool.install.join("native.json")).unwrap();
            start_install(tool,state).unwrap_or_else(|e|panic!("native fixture start: {} (submitted={})",e.reason,e.submitted));
            let native=wait_fixture_host(tool,Some(&prior));
            assert_eq!(install_state(tool,Some(&native)).unwrap(),InstallState::Loaded);
            if let Some(before)=before {assert_eq!(fs::read(&definition).unwrap(),before,"saved/idle start cannot rewrite the owner's definition");}
            else {
                #[cfg(not(windows))] assert_eq!(fs::read(&definition).unwrap(),unit.text.as_bytes());
                #[cfg(windows)] verify_saved_install_scope(tool).unwrap();
            }
            let revision=revision_for(tool).unwrap();
            let stop_id=format!("{id}-{}",cases.len());
            #[cfg(windows)] fixture_stop_binding(&tool.install,&stop_id,&native,&revision);
            stop_unit(tool,&stop_id,&native,&revision).unwrap();
            #[cfg(target_os="macos")] assert!(native_absent(tool).unwrap());
            #[cfg(any(target_os="linux",windows))] assert_eq!(install_state(tool,None).unwrap(),InstallState::Loaded);
            assert_ne!(coreq::process::identity(native.pid).unwrap().as_ref(),Some(&native));
            cases.push(json!({"from":match state {InstallState::Saved=>"saved",InstallState::Loaded=>"loaded-idle",InstallState::Absent=>"absent"},"pid":native.pid,"started":native.started,"definition_preserved":state!=InstallState::Absent,"exit":true}));
            prior=native;
        }
        #[cfg(any(target_os="linux",windows))] remove_unit(tool,&format!("{id}-{}",cases.len()-1),&prior,&revision_for(tool).unwrap()).unwrap();
        assert!(native_absent(tool).unwrap());fs::write(&definition,original).unwrap();
        json!(cases)
    }

    #[cfg(any(target_os="macos",target_os="linux",windows))]
    fn wait_fixture_host(tool:&coreq::Tool,prior:Option<&Identity>)->Identity {
        let until=std::time::Instant::now()+Duration::from_secs(10);
        loop {
            let found=fs::read(tool.install.join("native.json")).ok().and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|v|v["pid"].as_u64()).and_then(|pid|u32::try_from(pid).ok())
                .and_then(|pid|coreq::process::identity(pid).ok().flatten()).filter(|identity|Some(identity)!=prior);
            if let Some(native)=found {return native;}
            assert!(std::time::Instant::now()<until,"native fixture did not publish a new daemon");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
