//! Handle-bound security changes. The caller journals both snapshots before applying one.
use std::{ffi::c_void, os::windows::{ffi::OsStrExt, fs::MetadataExt}, path::{Path, PathBuf}, ptr};

type Handle = *mut c_void;
const READ_CONTROL: u32 = 0x20000;
const WRITE_DAC: u32 = 0x40000;
const WRITE_OWNER: u32 = 0x80000;
const LABEL: u32 = 0x10;
const INFORMATION: u32 = 7 | LABEL;

#[repr(C)]
#[derive(Default)]
struct FileInfo { attributes: u32, created: [u32; 2], accessed: [u32; 2], written: [u32; 2], volume: u32, size_high: u32, size_low: u32, links: u32, index_high: u32, index_low: u32 }
#[repr(C)]
struct Descriptor { revision: u8, reserved: u8, control: u16, owner: Handle, group: Handle, sacl: Handle, dacl: Handle }
#[repr(C)]
struct MandatoryLabel { sid: Handle, attributes: u32 }

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, security: Handle, disposition: u32, flags: u32, template: Handle) -> Handle;
    fn GetFileInformationByHandle(file: Handle, info: *mut FileInfo) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn LocalFree(memory: Handle) -> Handle;
    fn GetCurrentProcess() -> Handle;
    fn GetCurrentThreadId() -> u32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn GetSecurityInfo(handle: Handle, kind: u32, information: u32, owner: *mut Handle, group: *mut Handle, dacl: *mut Handle, sacl: *mut Handle, descriptor: *mut Handle) -> u32;
    fn SetSecurityInfo(handle: Handle, kind: u32, information: u32, owner: Handle, group: Handle, dacl: Handle, sacl: Handle) -> u32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(text: *const u16, revision: u32, descriptor: *mut Handle, size: *mut u32) -> i32;
    fn ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor: Handle, revision: u32, information: u32, text: *mut *mut u16, size: *mut u32) -> i32;
    fn GetSecurityDescriptorOwner(descriptor: Handle, owner: *mut Handle, defaulted: *mut i32) -> i32;
    fn GetSecurityDescriptorGroup(descriptor: Handle, group: *mut Handle, defaulted: *mut i32) -> i32;
    fn GetSecurityDescriptorDacl(descriptor: Handle, present: *mut i32, acl: *mut Handle, defaulted: *mut i32) -> i32;
    fn GetSecurityDescriptorSacl(descriptor: Handle, present: *mut i32, acl: *mut Handle, defaulted: *mut i32) -> i32;
    fn GetSecurityDescriptorControl(descriptor: Handle, control: *mut u16, revision: *mut u32) -> i32;
    fn InitializeAcl(acl: Handle, size: u32, revision: u32) -> i32;
    fn AddAce(acl: Handle, revision: u32, index: u32, entries: Handle, size: u32) -> i32;
    fn GetAce(acl: Handle, index: u32, ace: *mut Handle) -> i32;
    fn EqualSid(first: Handle, second: Handle) -> i32;
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn IsTokenRestricted(token: Handle) -> i32;
    fn ConvertStringSidToSidW(text: *const u16, sid: *mut Handle) -> i32;
    fn GetLengthSid(sid: Handle) -> u32;
    fn SetTokenInformation(token: Handle, class: u32, information: Handle, size: u32) -> i32;
    fn GetTokenInformation(token: Handle, class: u32, information: Handle, size: u32, needed: *mut u32) -> i32;
    fn GetSidSubAuthorityCount(sid: Handle) -> *mut u8;
    fn GetSidSubAuthority(sid: Handle, index: u32) -> *mut u32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetThreadDesktop(thread: u32) -> Handle;
    fn GetUserObjectInformationW(handle: Handle, index: i32, information: Handle, size: u32, needed: *mut u32) -> i32;
    fn OpenDesktopW(name: *const u16, flags: u32, inherit: i32, access: u32) -> Handle;
    fn CloseDesktop(desktop: Handle) -> i32;
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetSecurityObject(handle: Handle, information: u32, descriptor: Handle) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
struct Local(Handle);
impl Drop for Local { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }
fn wide(text: &std::ffi::OsStr) -> Result<Vec<u16>, String> {
    let mut result: Vec<_> = text.encode_wide().collect();
    if result.contains(&0) { return Err("Windows security: embedded NUL".into()); }
    result.push(0); Ok(result)
}
fn last(operation: &str) -> String { format!("Windows security {operation}: {}", std::io::Error::last_os_error()) }
fn boolean(result: i32, operation: &str) -> Result<(), String> { if result == 0 { Err(last(operation)) } else { Ok(()) } }
fn status(result: u32, operation: &str) -> Result<(), String> { if result != 0 { Err(format!("Windows security {operation}: {}", std::io::Error::from_raw_os_error(result as i32))) } else { Ok(()) } }
fn parse(text: &str) -> Result<Local, String> {
    let text = wide(std::ffi::OsStr::new(text))?; let mut descriptor = ptr::null_mut();
    boolean(unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut descriptor, ptr::null_mut()) }, "parse descriptor")?;
    Ok(Local(descriptor))
}
fn stringify(descriptor: Handle) -> Result<String, String> {
    let mut descriptor = components(descriptor)?;
    // LABEL-only removal leaves a null SACL; without control flags it still means no mandatory label.
    if descriptor.sacl.is_null() && descriptor.control & 0x2a00 == 0 { descriptor.control &= !0x10; }
    let mut text = ptr::null_mut(); let mut size = 0;
    boolean(unsafe { ConvertSecurityDescriptorToStringSecurityDescriptorW((&mut descriptor as *mut Descriptor).cast(), 1, INFORMATION, &mut text, &mut size) }, "serialize descriptor")?;
    let _allocation = Local(text.cast());
    if size == 0 { return Err("Windows security: empty descriptor".into()); }
    // Windows reports allocation capacity, not the string's character count.
    let buffer = unsafe { std::slice::from_raw_parts(text, size as usize) };
    let end = buffer.iter().position(|c| *c == 0).ok_or("Windows security: unterminated descriptor")?;
    String::from_utf16(&buffer[..end]).map_err(|e| e.to_string())
}
fn components(descriptor: Handle) -> Result<Descriptor, String> {
    let mut result = Descriptor { revision: 1, reserved: 0, control: 0, owner: ptr::null_mut(), group: ptr::null_mut(), sacl: ptr::null_mut(), dacl: ptr::null_mut() };
    let mut defaulted = 0; let mut present = 0; let mut revision = 0;
    boolean(unsafe { GetSecurityDescriptorControl(descriptor, &mut result.control, &mut revision) }, "descriptor control")?;
    result.control &= !0x8000; // The pointers below form an absolute descriptor.
    boolean(unsafe { GetSecurityDescriptorOwner(descriptor, &mut result.owner, &mut defaulted) }, "descriptor owner")?;
    boolean(unsafe { GetSecurityDescriptorGroup(descriptor, &mut result.group, &mut defaulted) }, "descriptor group")?;
    boolean(unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut result.dacl, &mut defaulted) }, "descriptor DACL")?;
    boolean(unsafe { GetSecurityDescriptorSacl(descriptor, &mut present, &mut result.sacl, &mut defaulted) }, "descriptor label")?;
    Ok(result)
}
fn read_descriptor(handle: Handle) -> Result<String, String> {
    let mut descriptor = ptr::null_mut();
    status(unsafe { GetSecurityInfo(handle, 1, INFORMATION, ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), &mut descriptor) }, "read descriptor")?;
    let descriptor = Local(descriptor); stringify(descriptor.0)
}

// Retained ancestor handles prevent replacing a checked directory with a junction.
struct Opened { file: OwnedHandle, _ancestors: Vec<OwnedHandle>, info: FileInfo }
pub(super) fn create_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            if open(path, false)?.info.attributes & 0x10 == 0 { return Err("Windows security: expected directory".into()); }
            return Ok(());
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
    }
    let parent = path.parent().ok_or("Windows security: directory parent missing")?;
    create_directory(parent)?;
    let _parent = open(parent, false)?;
    match std::fs::create_dir(path) { Ok(()) => (), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (), Err(e) => return Err(e.to_string()) }
    if open(path, false)?.info.attributes & 0x10 == 0 { return Err("Windows security: created directory identity changed".into()); }
    Ok(())
}
fn open(path: &Path, mutate: bool) -> Result<Opened, String> {
    if !path.is_absolute() || path.components().any(|p| matches!(p, std::path::Component::ParentDir)) { return Err("Windows security: absolute normalized path required".into()); }
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
        || path.components().any(|p| matches!(p, std::path::Component::Normal(name) if name.encode_wide().any(|c| c == u16::from(b':')))) {
        return Err("Windows security: local disk path without alternate streams required".into());
    }
    let mut ancestors = Vec::new();
    let paths: Vec<_> = path.ancestors().collect();
    for ancestor in paths.iter().rev() {
        let leaf = *ancestor == path;
        let name = wide(ancestor.as_os_str())?;
        let directory = std::fs::symlink_metadata(ancestor).map_err(|e| e.to_string())?.file_attributes() & 0x10 != 0;
        // Pin directory identity while preserving owner watchers and open data files.
        let access = READ_CONTROL | 0x80 | if leaf && mutate { WRITE_DAC | WRITE_OWNER } else { 0 };
        let share = if !leaf || directory { 3 } else { 7 };
        let handle = unsafe { CreateFileW(name.as_ptr(), access, share, ptr::null_mut(), 3, 0x02200000, ptr::null_mut()) };
        if handle == -1isize as Handle { return Err(last("open identity-bound path")); }
        let handle = OwnedHandle(handle); let mut info = FileInfo::default();
        boolean(unsafe { GetFileInformationByHandle(handle.0, &mut info) }, "read identity")?;
        if (info.attributes & 0x10 != 0) != directory { return Err("Windows security: path type changed during open".into()); }
        if info.attributes & 0x400 != 0 { return Err(format!("Windows security refuses reparse point {}", ancestor.display())); }
        if leaf {
            if info.links != 1 && info.attributes & 0x10 == 0 { return Err("Windows security refuses hard-linked files".into()); }
            return Ok(Opened { file: handle, _ancestors: ancestors, info });
        }
        ancestors.push(handle);
    }
    Err("Windows security: missing path".into())
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Snapshot {
    pub path: PathBuf,
    pub volume: u32,
    pub file_id: u64,
    pub directory: bool,
    pub descriptor: String,
}
impl Snapshot {
    fn from_open(path: &Path, opened: &Opened) -> Result<Self, String> {
        Ok(Self { path: path.to_owned(), volume: opened.info.volume, file_id: (u64::from(opened.info.index_high) << 32) | u64::from(opened.info.index_low), directory: opened.info.attributes & 0x10 != 0, descriptor: read_descriptor(opened.file.0)? })
    }
    pub fn capture(path: &Path) -> Result<Self, String> { Self::from_open(path, &open(path, false)?) }
    pub fn low(&self) -> Result<Self, String> { self.with_label("LW", true) }
    pub fn medium(&self) -> Result<Self, String> { self.with_label("ME", false) }
    pub fn deny(&self, mask: u32, sid: &str) -> Result<Self, String> {
        if !sid.starts_with("S-1-") || !sid[2..].bytes().all(|c| c.is_ascii_digit() || c == b'-') { return Err("Windows security: numeric SID required".into()); }
        self.with_deny(&format!("D:(D;;0x{mask:08x};;;{sid})"))
    }
    fn with_deny(&self, deny: &str) -> Result<Self, String> {
        let original = parse(&self.descriptor)?; let mut descriptor = components(original.0)?;
        if descriptor.dacl.is_null() || descriptor.control & 4 == 0 { return Err("Windows security refuses to narrow a missing/null DACL".into()); }
        let deny = parse(deny)?; let deny_acl = components(deny.0)?.dacl;
        let mut wanted = ptr::null_mut();
        boolean(unsafe { GetAce(deny_acl, 0, &mut wanted) }, "read denial")?;
        let wanted = wanted.cast::<u8>();
        let count = unsafe { *descriptor.dacl.cast::<u16>().add(2) };
        let mut old_ace_bytes = 0usize;
        for index in 0..u32::from(count) {
            let mut ace = ptr::null_mut();
            boolean(unsafe { GetAce(descriptor.dacl, index, &mut ace) }, "read existing ACE")?;
            let ace = ace.cast::<u8>();
            old_ace_bytes += unsafe { *ace.add(2).cast::<u16>() } as usize;
            if unsafe { *ace == 1 && *ace.add(1) & 0x0f == *wanted.add(1) & 0x0f
                && *ace.add(4).cast::<u32>() & *wanted.add(4).cast::<u32>() == *wanted.add(4).cast::<u32>()
                && EqualSid(ace.add(8).cast(), wanted.add(8).cast()) != 0 } { return Ok(self.clone()); }
        }
        let deny_size = unsafe { *wanted.add(2).cast::<u16>() } as usize;
        let size = old_ace_bytes + deny_size + 8;
        if size > u16::MAX as usize { return Err("Windows security: DACL too large".into()); }
        let mut acl_storage = vec![0u32; size.div_ceil(4)];
        let acl = acl_storage.as_mut_ptr().cast();
        boolean(unsafe { InitializeAcl(acl, size as u32, 4) }, "initialize DACL")?;
        boolean(unsafe { AddAce(acl, 4, u32::MAX, wanted.cast(), deny_size as u32) }, "add denial")?;
        if old_ace_bytes != 0 { boolean(unsafe { AddAce(acl, 4, u32::MAX, descriptor.dacl.cast::<u8>().add(8).cast(), old_ace_bytes as u32) }, "preserve DACL")?; }
        descriptor.dacl = acl;
        let mut result = self.clone(); result.descriptor = stringify((&mut descriptor as *mut Descriptor).cast())?; Ok(result)
    }
    fn with_label(&self, integrity: &str, deny_delete_child: bool) -> Result<Self, String> {
        let original = parse(&self.descriptor)?; let mut descriptor = components(original.0)?;
        let inheritance = if self.directory { "OICI" } else { "" };
        let label = parse(&format!("S:(ML;{inheritance};NW;;;{integrity})"))?;
        descriptor.sacl = components(label.0)?.sacl; descriptor.control |= 0x10;
        let mut result = self.clone(); result.descriptor = stringify((&mut descriptor as *mut Descriptor).cast())?;
        // SDDL's DC means directory-service mask 0x2; FILE_DELETE_CHILD is 0x40.
        if deny_delete_child && self.directory { result.with_deny("D:(D;OICI;0x40;;;WD)") } else { Ok(result) }
    }
    /// `self` is the exact expected current state; the caller persists `replacement` first.
    pub fn apply(&self, replacement: &Self) -> Result<(), String> {
        if self.path != replacement.path || self.volume != replacement.volume || self.file_id != replacement.file_id || self.directory != replacement.directory { return Err("Windows security: replacement identity mismatch".into()); }
        let opened = open(&self.path, true)?;
        if Self::from_open(&self.path, &opened)? != *self { return Err(format!("Windows security: identity or ACL changed at {}", self.path.display())); }
        let expected = parse(&self.descriptor)?; let expected = components(expected.0)?;
        let target = parse(&replacement.descriptor)?; let mut target = components(target.0)?;
        // Only DACL and mandatory label are writable here. Owner/group and SACL protection stay intact.
        let saved_owner = target.owner; let saved_group = target.group; let saved_control = target.control;
        target.owner = expected.owner; target.group = expected.group;
        target.control = (target.control & !0x2000) | (expected.control & 0x2000);
        if stringify((&mut target as *mut Descriptor).cast())? != replacement.descriptor || saved_control & 0x2000 != expected.control & 0x2000 {
            return Err("Windows security: owner/group or SACL protection change refused".into());
        }
        target.owner = saved_owner; target.group = saved_group;
        // Reapplying UNPROTECTED re-inherits ACEs even when the saved protection state is unchanged.
        let protection = if (expected.control ^ target.control) & 0x1000 == 0 { 0 }
            else if target.control & 0x1000 != 0 { 0x80000000 } else { 0x20000000 };
        // The kernel consumes AUTO_INHERIT_REQ to retain the existing AUTO_INHERITED bits.
        target.control |= (target.control & 0xc00) >> 2;
        // SetSecurityInfo rewrites SACL control flags and propagates ACEs to unjournaled children.
        let result = unsafe { NtSetSecurityObject(opened.file.0, 4 | LABEL | protection, (&mut target as *mut Descriptor).cast()) };
        if result < 0 { status(unsafe { RtlNtStatusToDosError(result) }, "apply descriptor")?; }
        let actual = Self::from_open(&self.path, &opened)?;
        if actual != *replacement {
            #[cfg(test)]
            eprintln!("descriptor mismatch: expected={:?}; actual={:?}", replacement.descriptor, actual.descriptor);
            return Err(format!("Windows security: descriptor readback mismatch at {}; journal retained", self.path.display()));
        }
        Ok(())
    }
}

fn private_desktop(name: &str) -> bool {
    name.strip_prefix("CodexSandboxDesktop-").is_some_and(|suffix| (31..=32).contains(&suffix.len()) && suffix.bytes().all(|c| c.is_ascii_hexdigit()))
}
pub(super) fn lower_restricted_process() -> Result<u32, String> {
    let mut token = ptr::null_mut();
    boolean(unsafe { OpenProcessToken(GetCurrentProcess(), 0x88, &mut token) }, "open token")?;
    let token = OwnedHandle(token);
    if unsafe { IsTokenRestricted(token.0) } == 0 { return Err("Windows security: restricted token required".into()); }
    let mut name = [0u16; 256]; let mut needed = 0;
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    boolean(unsafe { GetUserObjectInformationW(desktop, 2, name.as_mut_ptr().cast(), (name.len() * 2) as u32, &mut needed) }, "read desktop name")?;
    let end = name.iter().position(|c| *c == 0).ok_or("Windows security: unterminated desktop name")?;
    let text = String::from_utf16(&name[..end]).map_err(|e| e.to_string())?;
    if !private_desktop(&text) { return Err("Windows security: SDK private desktop required".into()); }
    let owned = unsafe { OpenDesktopW(name.as_ptr(), 0, 0, READ_CONTROL | WRITE_OWNER) };
    if owned.is_null() { return Err(last("open private desktop")); }
    let label_result = (|| {
        let descriptor = parse("S:(ML;;NW;;;LW)")?;
        status(unsafe { SetSecurityInfo(owned, 7, LABEL, ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), components(descriptor.0)?.sacl) }, "label private desktop")
    })();
    unsafe { CloseDesktop(owned); } label_result?;
    let sid_text = wide(std::ffi::OsStr::new("S-1-16-4096"))?; let mut sid = ptr::null_mut();
    boolean(unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) }, "parse Low integrity SID")?;
    let sid = Local(sid); let mut label = MandatoryLabel { sid: sid.0, attributes: 0x20 };
    boolean(unsafe { SetTokenInformation(token.0, 25, (&mut label as *mut MandatoryLabel).cast(), std::mem::size_of::<MandatoryLabel>() as u32 + GetLengthSid(sid.0)) }, "lower restricted token")?;
    let mut buffer = [0usize; 64]; needed = 0;
    boolean(unsafe { GetTokenInformation(token.0, 25, buffer.as_mut_ptr().cast(), std::mem::size_of_val(&buffer) as u32, &mut needed) }, "read token integrity")?;
    let label = unsafe { &*buffer.as_ptr().cast::<MandatoryLabel>() };
    let count = unsafe { *GetSidSubAuthorityCount(label.sid) };
    if count == 0 { return Err("Windows security: missing integrity RID".into()); }
    let rid = unsafe { *GetSidSubAuthority(label.sid, u32::from(count) - 1) };
    if rid != 4096 { return Err(format!("Windows security: expected Low integrity, read {rid}")); }
    Ok(rid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_sdk_private_desktop_names() {
        for count in [31, 32] { assert!(private_desktop(&format!("CodexSandboxDesktop-{}", "a".repeat(count)))); }
        for name in ["Default", "WinSta0", "CodexSandboxDesktop-", "CodexSandboxDesktop-123456789abcdef0123456789abcdefg"] { assert!(!private_desktop(name)); }
    }
    #[test]
    fn only_unprotected_null_label_is_absent() {
        let absent = parse("D:(A;;FA;;;WD)").unwrap();
        let null = parse("D:(A;;FA;;;WD)S:NO_ACCESS_CONTROL").unwrap();
        assert_eq!(stringify(absent.0).unwrap(), stringify(null.0).unwrap());
        for sacl in ["PNO_ACCESS_CONTROL", "AINO_ACCESS_CONTROL", "ARNO_ACCESS_CONTROL", "(ML;;NW;;;ME)"] {
            let other = parse(&format!("D:(A;;FA;;;WD)S:{sacl}")).unwrap();
            assert_ne!(stringify(absent.0).unwrap(), stringify(other.0).unwrap());
        }
    }
    #[test]
    fn native_identity_label_dacl_roundtrip() {
        let root = std::env::temp_dir().join(format!("taskq-security-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let file = root.join("owner.txt"); std::fs::write(&file, "before").unwrap();
        let original = Snapshot::capture(&root).unwrap(); let child_original = Snapshot::capture(&file).unwrap();
        let low = original.low().unwrap(); original.apply(&low).unwrap();
        assert_eq!(low.low().unwrap(), low, "preparation must be idempotent");
        assert_eq!(Snapshot::capture(&file).unwrap(), child_original, "no unjournaled child propagation");
        std::fs::write(&file, "owner write").unwrap(); std::fs::remove_file(&file).unwrap();
        let new_file = root.join("new.txt");
        std::fs::write(&new_file, "owner create").expect("owner can create a file in prepared directory");
        std::fs::remove_file(&new_file).expect("owner can delete a newly created file");
        low.apply(&original).unwrap(); assert_eq!(Snapshot::capture(&root).unwrap(), original);
        std::fs::write(&file, "identity replacement").unwrap();
        assert!(child_original.apply(&child_original.medium().unwrap()).is_err());
        let current = Snapshot::capture(&file).unwrap(); let medium = current.medium().unwrap(); current.apply(&medium).unwrap();
        assert!(current.apply(&current.low().unwrap()).is_err(), "concurrent label change must refuse");
        medium.apply(&current).unwrap(); assert_eq!(Snapshot::capture(&file).unwrap(), current);
        use std::os::windows::fs::OpenOptionsExt;
        let live_owner_file = std::fs::OpenOptions::new().read(true).write(true).share_mode(7).open(&file).unwrap();
        let current = Snapshot::capture(&file).unwrap(); let changed_acl = current.deny(0x40, "S-1-1-0").unwrap();
        assert_eq!(changed_acl.deny(0x40, "S-1-1-0").unwrap(), changed_acl);
        current.apply(&changed_acl).unwrap();
        assert!(current.apply(&current.low().unwrap()).is_err(), "concurrent DACL change must refuse");
        changed_acl.apply(&current).unwrap(); drop(live_owner_file);
        let junction = root.join("junction");
        assert!(std::process::Command::new("cmd.exe").args(["/d", "/c", "mklink", "/J"]).arg(&junction).arg(&root).output().unwrap().status.success());
        assert!(Snapshot::capture(&junction).is_err()); assert!(Snapshot::capture(&junction.join("owner.txt")).is_err());
        assert!(create_directory(&junction.join("must-not-exist")).is_err());
        assert!(!root.join("must-not-exist").exists(), "directory creation must not follow a junction");
        std::fs::remove_dir(&junction).unwrap(); std::fs::remove_file(&file).unwrap(); std::fs::remove_dir(&root).unwrap();
    }
}
