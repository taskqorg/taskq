    use super::*;
    use tracing_subscriber::{layer::{Context,SubscriberExt},Layer};
    #[link(name="kernel32")]
    unsafe extern "system" {fn SetLastError(code:u32);}
    struct ChangesNativeError;
    // The subscriber fixture must not instrument its own tracing callback recursively.
    macro_rules! native_callback {()=> {
        impl<S:tracing::Subscriber> Layer<S> for ChangesNativeError {
            fn on_enter(&self,_:&tracing::span::Id,_:Context<'_,S>) {unsafe {SetLastError(123);}}
        }
    }}
    native_callback!();
    // #1435 A2: the watch must recognize its launcher and nothing else. Risk: a reused parent pid ends the service.
    #[test]
    fn launcher_watch_matches_parent_image_only() {
        let mut child=Command::new("cmd").args(["/c","ping -n 3 127.0.0.1 >nul"]).spawn().unwrap();
        assert_eq!(parent(child.id()).unwrap(),unsafe {GetCurrentProcessId()});
        let own=owned(unsafe {OpenProcess(0x1000,0,GetCurrentProcessId())},io::Error::last_os_error()).unwrap();
        assert_eq!(image(&own).unwrap().canonicalize().unwrap(),std::env::current_exe().unwrap().canonicalize().unwrap());
        assert!(launched_by(&[std::env::current_exe().unwrap()]).unwrap().is_none(),"the test runner is not its own parent");
        // #2049 A1: any listed launcher (console or GUI) is the parent.
        let up=owned(unsafe {OpenProcess(0x1000,0,parent(GetCurrentProcessId()).unwrap())},io::Error::last_os_error()).unwrap();
        assert!(launched_by(&[std::env::current_exe().unwrap(),image(&up).unwrap()]).unwrap().is_some(),"the runner's parent is a listed launcher");
        child.kill().ok();child.wait().ok();
    }
    // #2214 A1: Task Scheduler priority 7 -> host below-normal CPU, low I/O, memory 2 -> probes starve behind worker builds.
    // Risk: the host keeps the task's low scheduling, or the raise lowers a process that already runs higher.
    #[test]
    fn host_scheduling_rises_from_task_priority_to_normal_and_never_lowers() {
        let mut child=Command::new("cmd").args(["/c","ping -n 30 127.0.0.1 >nul"]).spawn().unwrap();
        let process=child.as_raw_handle();
        let (low_io,low_memory)=(1u32,2u32);
        assert_ne!(unsafe {SetPriorityClass(process,BELOW_NORMAL_PRIORITY_CLASS)},0);
        assert_eq!(unsafe {NtSetInformationProcess(process,PROCESS_IO_PRIORITY,&low_io,4)},0);
        assert_ne!(unsafe {SetProcessInformation(process,PROCESS_MEMORY_PRIORITY,&low_memory,4)},0);
        assert_eq!(scheduling(process).unwrap(),(BELOW_NORMAL_PRIORITY_CLASS,1,2),"fixture matches a priority-7 scheduled task");
        assert_eq!(raise_scheduling(process).unwrap().as_deref(),Some("class 0x4000, I/O 1, memory 2"));
        assert_eq!(scheduling(process).unwrap(),(NORMAL_PRIORITY_CLASS,IO_PRIORITY_NORMAL,MEMORY_PRIORITY_NORMAL));
        assert_eq!(raise_scheduling(process).unwrap(),None,"a normal host is left alone");
        assert_ne!(unsafe {SetPriorityClass(process,0x8000)},0);
        assert_eq!(raise_scheduling(process).unwrap(),None);
        assert_eq!(scheduling(process).unwrap().0,0x8000,"above-normal is never lowered");
        child.kill().ok();child.wait().ok();
    }
    // #2214 A1 / R12: opt-in process qualification, using installed CLIs without dispatching agent work.
    // Run in its own filtered harness: scheduling changes apply only to that harness and its descendants.
    #[test]
    #[ignore = "native CLI login/version and curl qualification; TASKQ_PROBE_PHASE=before|after"]
    fn native_probes_under_worker_load() {
        let phase=std::env::var("TASKQ_PROBE_PHASE").expect("select before or after");
        assert!(matches!(phase.as_str(),"before"|"after"));
        let process=unsafe {GetCurrentProcess()};
        let (low_io,low_memory)=(1u32,2u32);
        assert_ne!(unsafe {SetPriorityClass(process,BELOW_NORMAL_PRIORITY_CLASS)},0);
        assert_eq!(unsafe {NtSetInformationProcess(process,PROCESS_IO_PRIORITY,&low_io,4)},0);
        assert_ne!(unsafe {SetProcessInformation(process,PROCESS_MEMORY_PRIORITY,&low_memory,4)},0);
        if phase=="after" {normal_scheduling().unwrap();}
        println!("phase={phase} scheduling={:?}",scheduling(process).unwrap());
        struct Work(Vec<Execution>);
        impl Drop for Work {
            fn drop(&mut self) {
                for child in &mut self.0 {child.kill().unwrap();}
                let until=Instant::now()+Duration::from_secs(5);
                for child in &mut self.0 {
                    while child.exited().unwrap().is_none() {
                        assert!(Instant::now()<until,"qualification child did not exit");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }
        let mut work=Work(Vec::new());
        for _ in 0..2 {
            work.0.push(Execution::spawn(Command::new("cmd").args(["/c","ping -n 180 127.0.0.1 >nul"])).unwrap());
        }
        let mut failures=0;
        for round in 0..30 {
            for (rt,args) in [("claude",&["--version"][..]),("claude",&["auth","status"][..]),
                ("codex",&["--version"][..]),("codex",&["login","status"][..]),
                ("curl.exe",&["--silent","--show-error","--fail","--max-time","10","--output","NUL","https://github.com/robots.txt"][..])] {
                let bin=super::super::which(rt);
                assert!(bin.is_absolute(),"installed {rt} required");
                let start=Instant::now();
                let result=super::super::probe("qualification.windows-probe",&bin,args);
                // Never print auth output: it may identify the operator.
                let ok=result.as_ref().is_some_and(|(success,_)|*success);
                println!("{phase} round={round} {rt} {} elapsed_ms={} ok={ok}",args[..args.len().min(2)].join(" "),start.elapsed().as_millis());
                failures+=usize::from(!ok);
            }
            let running=work.0.iter_mut().map(|child| {
                let observed=child.exited();
                let reason=observed.as_ref().err().map(String::as_str);
                let signals=crate::snapshot::Signals {local:true,child:Some(crate::snapshot::Child {
                    exited:observed.as_ref().is_ok_and(Option::is_some),unknown:reason,stopping:false}),..Default::default()};
                crate::snapshot::verdict(&signals).0=="running"
            }).filter(|running|*running).count();
            println!("{phase} round={round} native status running={running}/2");
            assert_eq!(running,2,"owned native children must remain observable during probes");
        }
        println!("{phase} failures={failures}/150");
        if phase=="after" {assert_eq!(failures,0,"native probe qualification failed");}
    }
    // Chain: native failure -> automatically captured adapter -> original diagnostic.
    #[test]
    fn native_error_survives_method_instrumentation() {
        let subscriber=tracing_subscriber::registry().with(ChangesNativeError);
        tracing::subscriber::with_default(subscriber,|| {
            unsafe {SetLastError(5);}
            let error=owned(std::ptr::null_mut(),io::Error::last_os_error()).unwrap_err();
            assert_eq!(error,io::Error::from_raw_os_error(5).to_string(),"method telemetry must not replace the failing operation's native error");
        });
    }

// Native process instruments retain the file-stdin launch without a model turn.
impl Execution {
    pub fn spawn(command:&mut Command)->Result<Self,StartError> {
        // #2266: once the last handle closes (service and launcher both gone) Windows ends every member, so a vanished name proves exit.
        let (key,job)=new_job()?;
        // Assignment precedes all user code. No breakaway flag is set.
        let child=command.creation_flags(0x08000004).spawn().map_err(|e|StartError::io(e,None))?;
        let observed=coreq::process::identity(child.id());
        let native=observed.as_ref().ok().and_then(Clone::clone);
        let mut execution=Self {members:std::collections::BTreeMap::new(),launcher:None,child:Some(child),native,key,job:Some(job),assigned:false,scope_known:true,terminal:None,fault:None,cancelled:false};
        let prepared=(|| {
            if execution.native.is_none() {return Err(format!("suspended native identity unavailable: {observed:?}"));}
            if unsafe {AssignProcessToJobObject(execution.job.as_ref().unwrap().as_raw_handle(),execution.child.as_ref().unwrap().as_raw_handle())}==0 {return Err(io::Error::last_os_error().to_string());}
            execution.assigned=true;
            // Kernel association keeps the object alive, but only an open handle preserves its name.
            let mut remote=std::ptr::null_mut();
            if unsafe {DuplicateHandle(GetCurrentProcess(),execution.job.as_ref().unwrap().as_raw_handle(),execution.child.as_ref().unwrap().as_raw_handle(),&mut remote,4|8,0,0)}==0 {return Err(io::Error::last_os_error().to_string());}
            resume(execution.child.as_ref().unwrap())
        })();
        match prepared {
            Ok(1)=>Ok(execution),
            Ok(count)=>{execution.scope_known=false;execution.fault=Some(format!("native thread resume count {count}; scope unknown; inspect original execution before release"));Ok(execution)},
            Err(reason)=>{
                let cleanup=execution.kill();let until=Instant::now()+Duration::from_secs(2);
                while Instant::now()<until {
                    if execution.exited().is_ok_and(|ended|ended.is_some()) {return Err(format!("native setup failed before resume; child cleanup confirmed: {reason}").into());}
                    std::thread::sleep(Duration::from_millis(10));
                }
                execution.fault=Some(format!("native setup failed; cleanup unconfirmed: {reason}; stop={cleanup:?}"));Ok(execution)
            },
        }
    }
}
