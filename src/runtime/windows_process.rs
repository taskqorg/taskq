//! Native Windows execution scope; a launcher exit does not release its surviving descendants.
use super::{Proc, StartError};
use std::{
    ffi::c_void,
    io,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command},
    time::{Duration, Instant},
};
type Handle = *mut c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn DuplicateHandle(
        source: Handle,
        handle: Handle,
        target: Handle,
        remote: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn CreateJobObjectW(attributes: Handle, name: *const u16) -> Handle;
    fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn IsProcessInJob(process: Handle, job: Handle, result: *mut i32) -> i32;
    fn QueryInformationJobObject(
        job: Handle,
        class: u32,
        data: Handle,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn SetInformationJobObject(
        job: Handle,
        class: u32,
        data: *const ExtendedLimits,
        size: u32,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn TerminateJobObject(job: Handle, code: u32) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut Thread) -> i32;
    fn Thread32Next(snapshot: Handle, entry: *mut Thread) -> i32;
    fn OpenThread(access: u32, inherit: i32, tid: u32) -> Handle;
    fn GetProcessIdOfThread(thread: Handle) -> u32;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn TerminateProcess(process: isize, code: u32) -> i32;
    fn ResumeThread(thread: Handle) -> u32;
    fn GetCurrentProcessId() -> u32;
    fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn GetPriorityClass(process: Handle) -> u32;
    fn SetPriorityClass(process: Handle, class: u32) -> i32;
    fn GetProcessInformation(process: Handle, class: u32, data: *mut u32, size: u32) -> i32;
    fn SetProcessInformation(process: Handle, class: u32, data: *const u32, size: u32) -> i32;
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationProcess(
        process: Handle,
        class: u32,
        data: *mut u32,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn NtSetInformationProcess(process: Handle, class: u32, data: *const u32, size: u32) -> i32;
}
const IDLE_PRIORITY_CLASS: u32 = 0x40;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x4000;
const NORMAL_PRIORITY_CLASS: u32 = 0x20;
const PROCESS_IO_PRIORITY: u32 = 33;
const IO_PRIORITY_NORMAL: u32 = 2;
const PROCESS_MEMORY_PRIORITY: u32 = 0;
const MEMORY_PRIORITY_NORMAL: u32 = 5;
#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    id: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    exe: [u16; 260],
}
#[repr(C)]
#[derive(Default)]
struct Thread {
    size: u32,
    usage: u32,
    id: u32,
    process: u32,
    priority: i32,
    delta: i32,
    flags: u32,
}
/// JOBOBJECT_EXTENDED_LIMIT_INFORMATION (class 9); only `flags` is set.
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_set: usize,
    max_set: usize,
    processes: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process: usize,
    peak_job: usize,
}
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
#[repr(C)]
#[derive(Default)]
struct Accounting {
    user: i64,
    kernel: i64,
    period_user: i64,
    period_kernel: i64,
    faults: u32,
    total: u32,
    active: u32,
    terminated: u32,
}

fn new_job() -> Result<(String, OwnedHandle), StartError> {
    let key = format!("job-kill:{}", super::uuid());
    let wide = name(&key)?;
    let raw = unsafe { CreateJobObjectW(std::ptr::null_mut(), wide.as_ptr()) };
    let error = io::Error::last_os_error();
    let collision = error.raw_os_error() == Some(183);
    let job = owned(raw, error)?;
    if collision {
        return Err("native job identity already exists; no process started".into());
    }
    let limits = ExtendedLimits {
        flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        ..ExtendedLimits::default()
    };
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            9,
            &limits,
            std::mem::size_of::<ExtendedLimits>() as u32,
        )
    } == 0
    {
        return Err(format!(
            "native job kill-on-close unavailable; no process started: {}",
            io::Error::last_os_error()
        )
        .into());
    }
    Ok((key, job))
}

fn name(scope: &str) -> Result<Vec<u16>, String> {
    let (key, _) = crate::data::job_scope(scope).ok_or("native job identity invalid")?;
    Ok(format!("Global\\TaskQ-execution-{key}\0")
        .encode_utf16()
        .collect())
}
fn owned(raw: Handle, error: io::Error) -> Result<OwnedHandle, String> {
    // Capture native errors before entering this automatically instrumented helper.
    if raw.is_null() || raw as isize == -1 {
        return Err(error.to_string());
    }
    // SAFETY: callers pass a newly created or opened native handle; OwnedHandle closes it on every path.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn active(job: &OwnedHandle) -> Result<u32, String> {
    let mut info = Accounting::default();
    let mut returned = 0;
    // SAFETY: JobObjectBasicAccountingInformation is the documented 48-byte layout.
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle(),
            1,
            (&mut info as *mut Accounting).cast(),
            std::mem::size_of::<Accounting>() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    if returned != std::mem::size_of::<Accounting>() as u32 {
        return Err("native job accounting incomplete".into());
    }
    Ok(info.active)
}
fn members(
    job: &OwnedHandle,
    known: &mut std::collections::BTreeMap<u32, OwnedHandle>,
) -> Result<(), String> {
    let mut ended = Vec::new();
    for (pid, handle) in known.iter() {
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
            0 => ended.push(*pid),
            258 => {}
            _ => return Err(io::Error::last_os_error().to_string()),
        }
    }
    for pid in ended {
        known.remove(&pid);
    }
    // Native membership is bounded; a larger/changing scope stays unknown instead of truncating proof.
    let mut ids = vec![0usize; 4098];
    let size = (ids.len() * std::mem::size_of::<usize>()) as u32;
    let mut returned = 0;
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle(),
            3,
            ids.as_mut_ptr().cast(),
            size,
            &mut returned,
        )
    } == 0
    {
        return Err(format!(
            "native job membership unavailable or over4096 processes: {}",
            io::Error::last_os_error()
        ));
    }
    let header = ids.as_ptr().cast::<u32>();
    let (assigned, count) = unsafe { (*header, *header.add(1)) };
    if assigned != count
        || count > 4096
        || returned < 8 + count * std::mem::size_of::<usize>() as u32
        || returned > size
    {
        return Err("native job membership incomplete; retry observation".into());
    }
    let pids = unsafe {
        std::slice::from_raw_parts(
            ids.as_ptr().cast::<u8>().add(8).cast::<usize>(),
            count as usize,
        )
    };
    for pid in pids {
        let pid = u32::try_from(*pid)
            .ok()
            .filter(|id| *id > 0 && *id <= i32::MAX as u32)
            .ok_or("native job process id invalid")?;
        if known.contains_key(&pid) {
            continue;
        }
        let raw = unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) };
        let error = io::Error::last_os_error();
        if raw.is_null() && error.raw_os_error() == Some(87) {
            continue;
        }
        let handle = owned(raw, error)?;
        let mut member = 0;
        if unsafe { IsProcessInJob(handle.as_raw_handle(), job.as_raw_handle(), &mut member) } == 0
        {
            return Err(io::Error::last_os_error().to_string());
        }
        if member != 0 {
            match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
                0 => {}
                258 => {
                    if known.len() >= 4096 {
                        return Err(
                            "native job wait-handle limit; retain execution until cleanup".into(),
                        );
                    }
                    known.insert(pid, handle);
                }
                _ => return Err(io::Error::last_os_error().to_string()),
            }
        }
    }
    Ok(())
}

fn resume(child: &Child) -> Result<u32, String> {
    // A suspended, newly owned process has only its primary thread; keep its process handle alive throughout discovery.
    let snapshot = owned(
        unsafe { CreateToolhelp32Snapshot(4, 0) },
        io::Error::last_os_error(),
    )?;
    let mut entry = Thread {
        size: std::mem::size_of::<Thread>() as u32,
        ..Thread::default()
    };
    let mut found = None;
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while more != 0 {
        if entry.process == child.id() {
            if found.is_some() {
                return Err("suspended native process has multiple threads".into());
            }
            found = Some(entry.id);
        }
        entry.size = std::mem::size_of::<Thread>() as u32;
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    if io::Error::last_os_error().raw_os_error() != Some(18) {
        return Err(io::Error::last_os_error().to_string());
    }
    let thread = owned(
        unsafe {
            OpenThread(
                2 | 0x0800,
                0,
                found.ok_or("suspended native thread missing")?,
            )
        },
        io::Error::last_os_error(),
    )?;
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != child.id() {
        return Err("native thread ownership changed".into());
    }
    let count = unsafe { ResumeThread(thread.as_raw_handle()) };
    if count == u32::MAX {
        Err(io::Error::last_os_error().to_string())
    } else {
        Ok(count)
    }
}

fn parent(pid: u32) -> Result<u32, String> {
    let snapshot = owned(
        unsafe { CreateToolhelp32Snapshot(2, 0) },
        io::Error::last_os_error(),
    )?;
    let mut entry = ProcessEntry {
        size: std::mem::size_of::<ProcessEntry>() as u32,
        usage: 0,
        id: 0,
        heap: 0,
        module: 0,
        threads: 0,
        parent: 0,
        priority: 0,
        flags: 0,
        exe: [0; 260],
    };
    let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    while more != 0 {
        if entry.id == pid {
            return Ok(entry.parent);
        }
        more = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    Err("process not in snapshot".into())
}
fn image(process: &OwnedHandle) -> Result<std::path::PathBuf, String> {
    let mut buffer = vec![0u16; 32768];
    let mut size = buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut size)
    } == 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_wide(&buffer[..size as usize]).into())
}
/// The parent process when its image is one of `launchers`; a parent pid can be reused, so the image proves the parent.
fn launched_by(launchers: &[std::path::PathBuf]) -> Result<Option<OwnedHandle>, String> {
    let pid = parent(unsafe { GetCurrentProcessId() })?;
    let raw = unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) };
    let handle = owned(raw, io::Error::last_os_error())?;
    let same = |p: &std::path::Path| {
        p.canonicalize()
            .ok()
            .map(|p| p.to_string_lossy().to_lowercase())
    };
    let path = same(&image(&handle)?);
    Ok((path.is_some() && launchers.iter().any(|l| same(l) == path)).then_some(handle))
}
/// #1435 A2: ending the Scheduled Task ends only the launcher; `service run` then exits too, so no old release keeps
/// running. Workers stay in their own jobs and the next service rebuilds from the board (§ 4). #2049 A1: the task
/// starts the GUI launcher; a task an older binary registered starts the console one.
pub fn exit_with_launcher(launchers: &[std::path::PathBuf]) {
    let handle = match launched_by(launchers) {
        Ok(Some(h)) => h,
        Ok(None) => return,
        Err(e) => {
            log!("launcher watch unavailable: {e}");
            return;
        }
    };
    std::thread::spawn(move || {
        if unsafe { WaitForSingleObject(handle.as_raw_handle(), u32::MAX) } == 0 {
            crate::platform::service_exit(0, "launcher ended");
        }
    });
}

/// CPU class, I/O priority and memory priority of one process.
fn scheduling(process: Handle) -> Result<(u32, u32, u32), String> {
    let class = unsafe { GetPriorityClass(process) };
    if class == 0 {
        return Err(format!(
            "priority class unreadable: {}",
            io::Error::last_os_error()
        ));
    }
    let mut io_priority = 0;
    let status = unsafe {
        NtQueryInformationProcess(
            process,
            PROCESS_IO_PRIORITY,
            &mut io_priority,
            4,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!("I/O priority unreadable: NTSTATUS {status:#x}"));
    }
    let mut memory = 0;
    if unsafe { GetProcessInformation(process, PROCESS_MEMORY_PRIORITY, &mut memory, 4) } == 0 {
        return Err(format!(
            "memory priority unreadable: {}",
            io::Error::last_os_error()
        ));
    }
    Ok((class, io_priority, memory))
}

/// #2214: Task Scheduler starts the host task at its default priority 7: below-normal CPU, low I/O, memory priority 2.
/// Low I/O and memory priorities can penalize probe, curl and git children during worker builds.
/// Raises only what is below normal and returns the previous values when it changed anything.
fn raise_scheduling(process: Handle) -> Result<Option<String>, String> {
    let (class, io_priority, memory) = scheduling(process)?;
    let low_class = matches!(class, IDLE_PRIORITY_CLASS | BELOW_NORMAL_PRIORITY_CLASS);
    if !low_class && io_priority >= IO_PRIORITY_NORMAL && memory >= MEMORY_PRIORITY_NORMAL {
        return Ok(None);
    }
    if low_class && unsafe { SetPriorityClass(process, NORMAL_PRIORITY_CLASS) } == 0 {
        return Err(format!(
            "priority class not raised: {}",
            io::Error::last_os_error()
        ));
    }
    if io_priority < IO_PRIORITY_NORMAL {
        let status = unsafe {
            NtSetInformationProcess(process, PROCESS_IO_PRIORITY, &IO_PRIORITY_NORMAL, 4)
        };
        if status != 0 {
            return Err(format!("I/O priority not raised: NTSTATUS {status:#x}"));
        }
    }
    if memory < MEMORY_PRIORITY_NORMAL
        && unsafe {
            SetProcessInformation(process, PROCESS_MEMORY_PRIORITY, &MEMORY_PRIORITY_NORMAL, 4)
        } == 0
    {
        return Err(format!(
            "memory priority not raised: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(Some(format!(
        "class {class:#x}, I/O {io_priority}, memory {memory}"
    )))
}

/// Gives the host service and the children it starts normal scheduling.
pub fn normal_scheduling() -> Result<Option<String>, String> {
    raise_scheduling(unsafe { GetCurrentProcess() })
}

pub struct Execution {
    members: std::collections::BTreeMap<u32, OwnedHandle>,
    launcher: Option<OwnedHandle>,
    child: Option<Child>,
    native: Option<coreq::process::Identity>,
    key: String,
    job: Option<OwnedHandle>,
    assigned: bool,
    scope_known: bool,
    terminal: Option<super::Exit>,
    fault: Option<String>,
    cancelled: bool,
}
impl Execution {
    /// AgentQ owns the child handle; TaskQ keeps its exact native scope before allowing user code.
    pub(super) fn launch_hooks(
        process: &mut agentq_runtime::agent::Process,
    ) -> Result<std::sync::Arc<std::sync::Mutex<Option<Self>>>, StartError> {
        let (key, job) = new_job()?;
        let scope = std::sync::Arc::new(std::sync::Mutex::new(Some(Self {
            members: Default::default(),
            launcher: None,
            child: None,
            native: None,
            key,
            job: Some(job),
            assigned: false,
            scope_known: true,
            terminal: None,
            fault: None,
            cancelled: false,
        })));
        let configure = process.configure.take();
        process.configure = Some(std::sync::Arc::new(move |command| {
            if let Some(configure) = &configure {
                configure(command);
            }
            command.creation_flags(0x08000004);
        }));
        let target = scope.clone();
        process.spawned = Some(std::sync::Arc::new(move |child| {
            let mut target = target.lock().unwrap_or_else(|e| e.into_inner());
            let scope = target.as_mut().unwrap();
            let prepared = (|| -> Result<u32, String> {
                let mut handle = std::ptr::null_mut();
                if unsafe {
                    DuplicateHandle(
                        GetCurrentProcess(),
                        child.as_raw_handle(),
                        GetCurrentProcess(),
                        &mut handle,
                        0,
                        0,
                        2,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error().to_string());
                }
                scope.launcher = Some(owned(handle, io::Error::last_os_error())?);
                scope.native = coreq::process::identity(child.id()).map_err(|e| e.to_string())?;
                if scope.native.is_none() {
                    return Err("suspended native identity unavailable".into());
                }
                if unsafe {
                    AssignProcessToJobObject(
                        scope.job.as_ref().unwrap().as_raw_handle(),
                        child.as_raw_handle(),
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error().to_string());
                }
                scope.assigned = true;
                let mut remote = std::ptr::null_mut();
                if unsafe {
                    DuplicateHandle(
                        GetCurrentProcess(),
                        scope.job.as_ref().unwrap().as_raw_handle(),
                        child.as_raw_handle(),
                        &mut remote,
                        4 | 8,
                        0,
                        0,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error().to_string());
                }
                resume(child)
            })();
            match prepared {
                Ok(1) => {}
                Ok(count) => {
                    scope.scope_known = false;
                    scope.fault = Some(format!(
                        "native thread resume count {count}; scope unknown; inspect original execution before release"
                    ));
                }
                Err(reason) => {
                    let cleanup = scope.kill();
                    let until = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < until {
                        if scope.termination().is_ok_and(|ended| ended.is_some()) {
                            return Err(format!(
                                "native setup failed before resume; child cleanup confirmed: {reason}"
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    scope.fault = Some(format!(
                        "native setup failed; cleanup unconfirmed: {reason}; stop={cleanup:?}"
                    ));
                }
            }
            // A partial setup is retained for observation, never returned as a no-start launch error.
            Ok(())
        }));
        Ok(scope)
    }
    /// #2266: a vanished kill-on-close scope whose launcher is gone (checked in `open`) is an ended execution.
    pub fn recover(native: coreq::process::Identity, key: &str) -> Result<Self, String> {
        if let Some(scope) = Self::open(native.clone(), key)? {
            return Ok(scope);
        }
        crate::data::missing_scope_exit(key)?;
        Ok(Self {
            members: std::collections::BTreeMap::new(),
            launcher: None,
            child: None,
            native: Some(native),
            key: key.into(),
            job: None,
            assigned: true,
            scope_known: true,
            terminal: Some(super::Exit { code: None }),
            fault: None,
            cancelled: false,
        })
    }
    pub fn open(native: coreq::process::Identity, key: &str) -> Result<Option<Self>, String> {
        let wide = name(key)?;
        let raw = unsafe { OpenJobObjectW(4 | 8, 0, wide.as_ptr()) };
        let error = io::Error::last_os_error();
        let job = if raw.is_null() {
            if error.raw_os_error() != Some(2) {
                return Err(format!("native job readback unavailable: {error}"));
            }
            None
        } else {
            Some(owned(raw, error)?)
        };
        let current = coreq::process::identity(native.pid).map_err(|e| e.to_string())?;
        if current.as_ref() == Some(&native) && job.is_none() {
            return Err(
                "native job missing while original launcher is live; reconcile scope".into(),
            );
        }
        if current.as_ref() == Some(&native) {
            let handle = owned(
                unsafe { OpenProcess(0x1000, 0, native.pid) },
                io::Error::last_os_error(),
            )?;
            let mut member = 0;
            if unsafe {
                IsProcessInJob(
                    handle.as_raw_handle(),
                    job.as_ref().unwrap().as_raw_handle(),
                    &mut member,
                )
            } == 0
            {
                return Err(io::Error::last_os_error().to_string());
            }
            if coreq::process::identity(native.pid)
                .map_err(|e| e.to_string())?
                .as_ref()
                != Some(&native)
            {
                return Err(
                    "native launcher changed during job membership read; retry observation".into(),
                );
            }
            if member == 0 {
                return Err("native launcher does not belong to recorded job".into());
            }
        }
        if job.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            members: std::collections::BTreeMap::new(),
            launcher: None,
            child: None,
            native: Some(native),
            key: key.into(),
            job,
            assigned: true,
            scope_known: true,
            terminal: None,
            fault: None,
            cancelled: false,
        }))
    }
}
impl Proc for Execution {
    fn native_identity(&self) -> Result<coreq::process::Identity, String> {
        self.native
            .clone()
            .ok_or("native creation identity unavailable before resume".into())
    }
    fn job(&self) -> Option<String> {
        (self.assigned && self.scope_known).then(|| self.key.clone())
    }
    fn id(&self) -> u32 {
        self.child
            .as_ref()
            .map(Child::id)
            .unwrap_or_else(|| self.native.as_ref().unwrap().pid)
    }
    fn exited(&mut self) -> Result<Option<i32>, String> {
        self.termination()?.map(|exit| exit.code.ok_or_else(|| "native exit code unknown".into())).transpose()
    }
    fn termination(&mut self) -> Result<Option<super::Exit>, String> {
        if self.terminal.is_some() {
            return Ok(self.terminal);
        }
        if !self.scope_known {
            return Err(self
                .fault
                .clone()
                .unwrap_or("native execution scope unknown".into()));
        }
        let root = if let Some(child) = self.child.as_mut() {
            child
                .try_wait()
                .map_err(|e| e.to_string())?
                .map(|status| super::Exit { code: status.code() })
        } else if let Some(launcher) = &self.launcher {
            match unsafe { WaitForSingleObject(launcher.as_raw_handle(), 0) } {
                258 => None,
                0 => {
                    let mut code = 0;
                    if unsafe { GetExitCodeProcess(launcher.as_raw_handle(), &mut code) } == 0 {
                        return Err(io::Error::last_os_error().to_string());
                    }
                    Some(super::Exit { code: Some(code as i32) })
                }
                _ => return Err(io::Error::last_os_error().to_string()),
            }
        } else {
            let native = self.native_identity()?;
            coreq::process::identity(native.pid)
                .map_err(|e| e.to_string())?
                .filter(|id| id == &native)
                .is_none()
                .then_some(super::Exit { code: None })
        };
        if let Some(job) = &self.job {
            members(job, &mut self.members)?;
        }
        let members = self.job.as_ref().map(active).transpose()?.unwrap_or(0);
        if let Some(exit) = root {
            if members == 0 && self.members.is_empty() {
                self.terminal = Some(exit);
                return Ok(self.terminal);
            }
        }
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        Ok(None)
    }
    fn exit_error(&self) -> Option<&'static str> {
        self.fault.as_deref().map(crate::profile::class)
    }
    fn kill(&mut self) -> Result<(), String> {
        if self.cancelled
            || self.terminal.is_some()
            || self.termination().is_ok_and(|ended| ended.is_some())
        {
            return Ok(());
        }
        let result = if self.assigned {
            let job = self
                .job
                .as_ref()
                .ok_or("native job unavailable; termination unknown")?;
            if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
                Err(io::Error::last_os_error().to_string())
            } else {
                Ok(())
            }
        } else if let Some(launcher) = &self.launcher {
            if unsafe { TerminateProcess(launcher.as_raw_handle() as isize, 1) } == 0 {
                Err(io::Error::last_os_error().to_string())
            } else {
                Ok(())
            }
        } else {
            self.child
                .as_mut()
                .ok_or("native launcher handle missing")?
                .kill()
                .map_err(|e| e.to_string())
        };
        if result.is_ok() {
            self.cancelled = true;
        }
        result
    }
}

#[cfg(test)]
#[path = "windows_process_tests.rs"]
mod tests;
